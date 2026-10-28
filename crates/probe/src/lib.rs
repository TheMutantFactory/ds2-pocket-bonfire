//! Probe DLL for the Pocket Bonfire reverse-engineering spike.
//!
//! Loaded by Lazy Loader from `Game\dllMods\`. It changes nothing in the game. It logs to
//! `Game\dllMods\pocket_bonfire_probe.log`:
//! - every `SetWarpTarget` and `WarpFunc` call, with their arguments and the warp struct bytes,
//! - changes to the last-rested bonfire, the loading flag and the game state,
//! - a full snapshot whenever F9 is pressed (to mark "I'm standing at X now").

#![cfg(all(windows, target_pointer_width = "64"))]

mod log;

use ds2::game::{self, Addresses, Snapshot};
use ds2::mem;
use minhook::MinHook;
use std::ffi::c_void;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::OnceLock;
use std::time::Duration;

const DLL_PROCESS_ATTACH: u32 = 1;
const VK_F9: i32 = 0x78;
const STRUCT_DUMP: usize = 0x80;

static ADDRS: OnceLock<Addresses> = OnceLock::new();
static ORIG_SET_WARP_TARGET: AtomicUsize = AtomicUsize::new(0);
static ORIG_WARP_FUNC: AtomicUsize = AtomicUsize::new(0);

#[link(name = "user32")]
extern "system" {
    fn GetAsyncKeyState(key: i32) -> i16;
}

#[no_mangle]
pub extern "system" fn DllMain(_module: *mut c_void, reason: u32, _reserved: *mut c_void) -> i32 {
    if reason == DLL_PROCESS_ATTACH {
        // No real work under the loader lock; the thread starts once DllMain returns.
        std::thread::spawn(run);
    }
    1
}

fn run() {
    log::init(DllMain as *const () as usize, "pocket_bonfire_probe.log");
    let addrs = match game::resolve() {
        Ok(a) => a,
        Err(e) => {
            log::line(format!("probe disabled: {e}"));
            return;
        }
    };
    let rel = |a: usize| a - addrs.module.base;
    log::line(format!(
        "DarkSoulsII.exe base {:#x} size {:#x} ts {:#x}; BaseA global +{:#x}, SetWarpTarget +{:#x}, WarpFunc +{:#x}",
        addrs.module.base,
        addrs.module.size,
        addrs.module.timestamp,
        rel(addrs.base_a_global),
        rel(addrs.set_warp_target),
        rel(addrs.warp_func)
    ));
    let _ = ADDRS.set(addrs);

    unsafe {
        match install(addrs.set_warp_target, set_warp_target_detour as *mut c_void, &ORIG_SET_WARP_TARGET)
            .and_then(|_| install(addrs.warp_func, warp_func_detour as *mut c_void, &ORIG_WARP_FUNC))
            .and_then(|_| MinHook::enable_all_hooks().map_err(|e| format!("enable: {e:?}")))
        {
            Ok(()) => log::line("hooks enabled".into()),
            Err(e) => log::line(format!("hook install failed, logging state only: {e}")),
        }
    }

    sample_loop(&addrs);
}

unsafe fn install(target: usize, detour: *mut c_void, orig: &AtomicUsize) -> Result<(), String> {
    let o = MinHook::create_hook(target as *mut c_void, detour).map_err(|e| format!("create {target:#x}: {e:?}"))?;
    orig.store(o as usize, Ordering::SeqCst);
    Ok(())
}

type SetWarpTargetFn = unsafe extern "system" fn(*mut u8, u64, u64, u64) -> *mut u8;
type WarpFuncFn = unsafe extern "system" fn(*mut u8, *mut u8) -> u64;

/// `SetWarpTarget(out, bonfire_id: u16, mode: i32)`. A fourth register is passed through untouched
/// in case it's used.
unsafe extern "system" fn set_warp_target_detour(out: *mut u8, id: u64, mode: u64, r9: u64) -> *mut u8 {
    let orig: SetWarpTargetFn = std::mem::transmute(ORIG_SET_WARP_TARGET.load(Ordering::SeqCst));
    let ret = orig(out, id, mode, r9);
    log::line(format!(
        "SetWarpTarget(out={out:p}, bonfire_id={}, mode={}, r9={r9:#x}) -> {ret:p}\n{}",
        id as u16,
        mode as u32 as i32,
        dump(ret as usize, STRUCT_DUMP)
    ));
    ret
}

unsafe extern "system" fn warp_func_detour(manager: *mut u8, warp: *mut u8) -> u64 {
    let orig: WarpFuncFn = std::mem::transmute(ORIG_WARP_FUNC.load(Ordering::SeqCst));
    let expected = ADDRS.get().and_then(|a| a.event_manager()).map(|em| em + 0x70);
    log::line(format!(
        "WarpFunc(manager={manager:p} [EventManager+0x70 = {:#x?}], warp={warp:p})\n{}\n  state before: {}",
        expected,
        dump(warp as usize, STRUCT_DUMP),
        ADDRS.get().map(|a| fmt_snapshot(&a.snapshot())).unwrap_or_default()
    ));
    orig(manager, warp)
}

fn sample_loop(addrs: &Addresses) {
    let mut last = Snapshot::default();
    let mut f9_down = false;
    loop {
        std::thread::sleep(Duration::from_millis(100));
        let now = addrs.snapshot();
        let changed = now.last_bonfire != last.last_bonfire
            || now.last_bonfire_area != last.last_bonfire_area
            || now.loading != last.loading
            || now.game_state != last.game_state
            || now.pos.is_some() != last.pos.is_some();
        if changed {
            log::line(format!("state: {}", fmt_snapshot(&now)));
        }
        last = now;

        let down = unsafe { GetAsyncKeyState(VK_F9) } as u16 & 0x8000 != 0;
        if down && !f9_down {
            log::line(format!(
                "F9 snapshot: {}\n  PlayerDataMap 0x140..0x200:\n{}\n  EventManager 0x140..0x180:\n{}",
                fmt_snapshot(&now),
                addrs.player_data_map().map(|p| dump(p + 0x140, 0xc0)).unwrap_or_else(|| "    (unavailable)".into()),
                addrs.event_manager().map(|p| dump(p + 0x140, 0x40)).unwrap_or_else(|| "    (unavailable)".into()),
            ));
        }
        f9_down = down;
    }
}

fn fmt_snapshot(s: &Snapshot) -> String {
    let pos = s
        .pos
        .map(|p| format!("y={:.3} z={:.3} x={:.3} ang=({:.3},{:.3},{:.3})", p[0], p[1], p[2], p[3], p[4], p[5]))
        .unwrap_or_else(|| "none".into());
    format!(
        "pos[{pos}] last_bonfire={:?} area={:#x?} loading={:?} game_state={:?}",
        s.last_bonfire, s.last_bonfire_area, s.loading, s.game_state
    )
}

/// Hex dump, 16 bytes a row, with each row also read as four u32 and four f32.
fn dump(addr: usize, len: usize) -> String {
    let Some(bytes) = mem::read_bytes(addr, len) else {
        return format!("    {addr:#x}: (unreadable)");
    };
    bytes
        .chunks(16)
        .enumerate()
        .map(|(i, row)| {
            let hex: Vec<String> = row.iter().map(|b| format!("{b:02x}")).collect();
            let words = row.as_chunks::<4>().0;
            let u: Vec<String> = words.iter().map(|w| format!("{:#x}", u32::from_le_bytes(*w))).collect();
            let f: Vec<String> = words.iter().map(|w| format!("{:.3}", f32::from_le_bytes(*w))).collect();
            format!("    +{:02x}: {}  u32[{}]  f32[{}]", i * 16, hex.join(" "), u.join(" "), f.join(" "))
        })
        .collect::<Vec<_>>()
        .join("\n")
}
