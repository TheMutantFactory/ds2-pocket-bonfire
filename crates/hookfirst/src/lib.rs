//! Regression test helper. `scripts/regression.ps1` loads this DLL ahead of the mods in its "hooked"
//! scenario. From `DllMain`, before any mod starts, it hooks the start of every game function either
//! mod hooks, with MinHook, the way another mod would. Each hook passes straight through, so the game
//! behaves the same, but the first bytes of each function are now a jump. That's what made players'
//! logs say "signature ... matched 0 times".
//!
//! Once a save is loaded, it also swaps the game's item table for a copy without the mods' rows, as a
//! mod that edits items might, so the test checks both mods put their rows back.

#![cfg(all(windows, target_pointer_width = "64"))]

use ds2::game::{self, SIGNATURES};
use ds2::{log, scan};
use minhook::MinHook;
use std::ffi::c_void;

const DLL_PROCESS_ATTACH: u32 = 1;
/// Easy Come, Easy Go's quit function, which this repo's signature list doesn't have.
const AOB_QUIT_TO_TITLE: &str = "48 83 EC 28 48 8B 0D ? ? ? ? 48 8B 89 E0 22 00 00 E8 ? ? ? ? 48 8B 05 ? ? ? ? 33 D2";
/// Room for each pass-through detour: `jmp [rip+0]` and the address it jumps to, 14 bytes.
const THUNK_STRIDE: usize = 16;

#[link(name = "kernel32")]
extern "system" {
    fn VirtualAlloc(address: *mut c_void, size: usize, kind: u32, protect: u32) -> *mut u8;
}
const MEM_COMMIT_RESERVE: u32 = 0x3000;
const PAGE_EXECUTE_READWRITE: u32 = 0x40;

#[no_mangle]
pub extern "system" fn DllMain(_module: *mut c_void, reason: u32, _reserved: *mut c_void) -> i32 {
    if reason == DLL_PROCESS_ATTACH {
        log::init(DllMain as *const () as usize, "ds2_hookfirst.log");
        let summary = ds2::hooks::serialized(|| unsafe { hook_everything() });
        log::line(summary);
        std::thread::spawn(swap_item_table);
    }
    1
}

/// The two mods' item rows, to say whether the stock copy really lacks them.
const MOD_ITEMS: [(u32, &str); 2] = [(60_355_010, "Pocket Bonfire"), (60_595_010, "Easy Go")];

fn read_param(addr: usize) -> Option<Vec<u8>> {
    let len = ds2::mem::read::<u32>(addr)? as usize;
    (0x40..=16 << 20).contains(&len).then(|| ds2::mem::read_bytes(addr, len)).flatten()
}

/// Does what another mod that edits items might: keeps a copy of the item table the game first made,
/// and once a save is loaded and the mods have added their rows, points the game at that copy, which
/// lacks them. Both mods have to notice and add their rows again, or their items lose their names and
/// stop working.
fn swap_item_table() {
    use std::time::{Duration, Instant};
    let Ok(module) = game::game_module() else { return };
    let Ok(addrs) = game::resolve(&module) else {
        log::line("item table swap: the game's addresses weren't found".into());
        return;
    };
    let started = Instant::now();
    let stock = loop {
        if let Some(bytes) = addrs.item_param().and_then(read_param) {
            break bytes;
        }
        if started.elapsed() > Duration::from_secs(180) {
            log::line("item table swap: the game never loaded its item table".into());
            return;
        }
        std::thread::sleep(Duration::from_millis(5));
    };
    let rows: Vec<&str> = match ds2::param::Param::parse(&stock) {
        Ok(p) => MOD_ITEMS.iter().filter(|(id, _)| p.row(*id).is_some()).map(|(_, n)| *n).collect(),
        Err(_) => vec!["unreadable"],
    };
    log::line(format!(
        "item table copied as the game made it, {} bytes, with the mods' rows: {}",
        stock.len(),
        if rows.is_empty() { "none".into() } else { rows.join(", ") }
    ));
    while !addrs.playable() {
        if started.elapsed() > Duration::from_secs(600) {
            log::line("item table swap: no save was loaded".into());
            return;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    std::thread::sleep(Duration::from_secs(1));
    let copy: &'static mut [u8] = Vec::leak(stock);
    let swapped = addrs.set_item_param(copy.as_ptr() as usize);
    log::line(format!("swapped the item table for the copy without the mods' rows: {swapped}"));
}

unsafe fn hook_everything() -> String {
    let module = match game::game_module() {
        Ok(m) => m,
        Err(e) => return format!("nothing hooked: {e}"),
    };
    let mut targets: Vec<(&str, &str)> = SIGNATURES.iter().filter(|s| s.start).map(|s| (s.name, s.aob)).collect();
    targets.push(("QuitToTitle", AOB_QUIT_TO_TITLE));

    let thunks = VirtualAlloc(std::ptr::null_mut(), 4096, MEM_COMMIT_RESERVE, PAGE_EXECUTE_READWRITE);
    if thunks.is_null() {
        return "nothing hooked: no memory for the detours".into();
    }
    let mut hooked = Vec::new();
    for (i, (name, aob)) in targets.iter().enumerate() {
        let Ok(target) = scan::find_function(&module, aob) else {
            log::line(format!("{name}: not found, not hooked"));
            continue;
        };
        let thunk = thunks.add(i * THUNK_STRIDE);
        // jmp [rip+0]; the address is filled in once MinHook has made the trampoline.
        std::ptr::copy_nonoverlapping([0xFFu8, 0x25, 0, 0, 0, 0].as_ptr(), thunk, 6);
        match MinHook::create_hook(target as *mut c_void, thunk as *mut c_void) {
            Ok(trampoline) => {
                std::ptr::write_unaligned(thunk.add(6) as *mut usize, trampoline as usize);
                hooked.push(format!("{name} at +{:#x}", target - module.base));
            }
            Err(e) => log::line(format!("{name}: hook failed: {e:?}")),
        }
    }
    match MinHook::enable_all_hooks() {
        Ok(()) => format!("hooked {} functions first: {}", hooked.len(), hooked.join(", ")),
        Err(e) => format!("hooks created but not enabled: {e:?}"),
    }
}
