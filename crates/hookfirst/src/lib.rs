//! Regression test helper. `scripts/regression.ps1` loads this DLL ahead of the mods in its "hooked"
//! scenario. From `DllMain`, before any mod starts, it hooks the start of every game function either
//! mod hooks, with MinHook, the way another mod would. Each hook passes straight through, so the game
//! behaves the same, but the first bytes of each function are now a jump. That's what made players'
//! logs say "signature ... matched 0 times".

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
    }
    1
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
