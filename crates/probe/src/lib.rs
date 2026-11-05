//! Probe DLL for the Pocket Bonfire reverse-engineering spike.
//!
//! Loaded by Lazy Loader from `Game\dllMods\`. It logs to `Game\dllMods\pocket_bonfire_probe.log`:
//! - every `SetWarpTarget` and `WarpFunc` call, with their arguments and the warp struct bytes,
//! - changes to the last-rested bonfire, the loading flag and the game state,
//! - a full snapshot whenever F9 is pressed (to mark "I'm standing at X now").
//!
//! It changes nothing in the game unless a Ctrl+F-key test in `test_keys` is pressed.

#![cfg(all(windows, target_pointer_width = "64"))]

mod log;
mod test_keys;

use ds2::game::{self, Addresses, Snapshot};
use ds2::mem;
use minhook::MinHook;
use std::ffi::c_void;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicUsize, Ordering};
use std::sync::OnceLock;
use std::time::Duration;

const DLL_PROCESS_ATTACH: u32 = 1;
const VK_F9: i32 = 0x78;
const STRUCT_DUMP: usize = 0x80;

static ADDRS: OnceLock<Addresses> = OnceLock::new();
static ORIG_SET_WARP_TARGET: AtomicUsize = AtomicUsize::new(0);
static ORIG_WARP_FUNC: AtomicUsize = AtomicUsize::new(0);
/// The warp manager the game itself last passed to `WarpFunc`; 0 until the game has warped.
pub(crate) static GAME_WARP_MANAGER: AtomicUsize = AtomicUsize::new(0);
/// Set by any `WarpFunc` call, cleared when the next load finishes. A reload that happens with this
/// clear didn't go through a warp, like a death respawn.
pub(crate) static WARP_SEEN: AtomicBool = AtomicBool::new(false);
/// The map the last `WarpFunc` was aimed at.
static LAST_WARP_MAP: AtomicU32 = AtomicU32::new(0);
/// True while the probe itself is calling `WarpFunc`, so `GAME_WARP_MANAGER` stays the game's own value.
pub(crate) static OUR_WARP: AtomicBool = AtomicBool::new(false);

#[link(name = "user32")]
extern "system" {
    fn GetAsyncKeyState(key: i32) -> i16;
}

pub(crate) fn key_is_down(vk: i32) -> bool {
    unsafe { GetAsyncKeyState(vk) as u16 & 0x8000 != 0 }
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
        "DarkSoulsII.exe base {:#x} size {:#x} ts {:#x}; BaseA global +{:#x}, SetWarpTarget +{:#x}, WarpFunc +{:#x}, ApplySpEffect +{:#x}, RestAtBonfire +{:#x}",
        addrs.module.base,
        addrs.module.size,
        addrs.module.timestamp,
        rel(addrs.base_a_global),
        rel(addrs.set_warp_target),
        rel(addrs.warp_func),
        rel(addrs.apply_sp_effect),
        rel(addrs.rest_at_bonfire)
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
    let ours = OUR_WARP.load(Ordering::SeqCst);
    if !ours {
        GAME_WARP_MANAGER.store(manager as usize, Ordering::SeqCst);
    }
    WARP_SEEN.store(true, Ordering::SeqCst);
    if let Some(map_id) = mem::read::<u32>(warp as usize + game::OFF_WARP_MAP_ID) {
        LAST_WARP_MAP.store(map_id, Ordering::SeqCst);
    }
    let expected = ADDRS.get().and_then(|a| a.warp_manager());
    log::line(format!(
        "WarpFunc[{}](manager={manager:p}, *(EventManager+0x70)={}, warp={warp:p})\n{}\n  state before: {}",
        if ours { "probe" } else { "game" },
        expected.map_or("none".to_string(), |m| format!(
            "{m:#x} {}",
            if m == manager as usize { "MATCH" } else { "MISMATCH" }
        )),
        dump(warp as usize, STRUCT_DUMP),
        ADDRS.get().map(|a| fmt_snapshot(&a.snapshot())).unwrap_or_default()
    ));
    orig(manager, warp)
}

fn sample_loop(addrs: &Addresses) {
    let mut last = Snapshot::default();
    let mut f9_down = false;
    let mut tests = test_keys::TestKeys::default();
    let mut last_fields = [None; MAP_FIELDS.len()];
    loop {
        std::thread::sleep(Duration::from_millis(50));
        let now = addrs.snapshot();
        let changed = now.last_bonfire != last.last_bonfire
            || now.last_bonfire_area != last.last_bonfire_area
            || now.loading != last.loading
            || now.game_state != last.game_state
            || now.pos.is_some() != last.pos.is_some();
        if changed {
            log::line(format!("state: {}", fmt_snapshot(&now)));
        }
        tests.tick(addrs, &now, &last);

        let fields = read_map_fields(addrs);
        let arrived = now.game_state == Some(game::GAME_STATE_PLAYABLE) && last.game_state != Some(game::GAME_STATE_PLAYABLE);
        if fields != last_fields || arrived {
            // Each warp's target map says what a correct field should read on arrival.
            log::line(format!(
                "map fields{}: {}  last warp target {:#x}  {}",
                if arrived { " (arrived)" } else { "" },
                fmt_map_fields(&fields),
                LAST_WARP_MAP.load(Ordering::SeqCst),
                fmt_snapshot(&now)
            ));
            last_fields = fields;
        }
        last = now;

        let down = key_is_down(VK_F9);
        if down && !f9_down {
            log::line(format!(
                "F9 snapshot: {}\n{}\n  map id candidates:\n{}",
                fmt_snapshot(&now),
                dump_regions(addrs),
                map_id_candidates(addrs)
            ));
        }
        f9_down = down;
    }
}

/// The regions most likely to hold what we're still looking for, such as the map you're standing in.
pub(crate) fn dump_regions(addrs: &Addresses) -> String {
    let region = |name: &str, base: Option<usize>, off: usize, len: usize| {
        let body = base.map_or_else(|| "    (unavailable)".to_string(), |p| dump(p + off, len));
        format!("  {name} {off:#x}..{:#x}:\n{body}", off + len)
    };
    [
        region("PlayerDataMap", addrs.player_data_map(), 0x140, 0xc0),
        region("EventManager", addrs.event_manager(), 0x140, 0x40),
        region("WarpManager", addrs.warp_manager(), 0, 0x80),
    ]
    .join("\n")
}

/// Fields that read m10_17 while LastBonfire was in m10_23, so they follow the player: name, root,
/// child offset (dereferenced, 0 for the root itself) and field offset. One map can't tell them
/// apart, so they're logged whenever they change, to see which stay right across real transitions.
const MAP_FIELDS: [(&str, Root, usize, usize); 8] = [
    ("EM+2a4", Root::Event, 0, 0x2a4),
    ("EM>20+2d8", Root::Event, 0x20, 0x2d8),
    ("EM>30+c0", Root::Event, 0x30, 0xc0),
    ("Base>30+360", Root::Base, 0x30, 0x360),
    ("Base>40+2f4", Root::Base, 0x40, 0x2f4),
    ("Base>78+580", Root::Base, 0x78, 0x580),
    ("Base>a8+1b8", Root::Base, 0xa8, 0x1b8),
    ("Base>b0+6a8", Root::Base, 0xb0, 0x6a8),
];

#[derive(Clone, Copy)]
enum Root {
    Base,
    Event,
}

fn read_map_fields(addrs: &Addresses) -> [Option<u32>; MAP_FIELDS.len()] {
    MAP_FIELDS.map(|(_, root, child, field)| {
        let base = match root {
            Root::Base => addrs.base_a(),
            Root::Event => addrs.event_manager(),
        }?;
        let parent = if child == 0 { base } else { mem::ptr(base + child)? };
        mem::read::<u32>(parent + field)
    })
}

fn fmt_map_fields(values: &[Option<u32>; MAP_FIELDS.len()]) -> String {
    MAP_FIELDS
        .iter()
        .zip(values)
        .map(|((name, ..), v)| format!("{name}={}", v.map_or("none".to_string(), |v| format!("{v:#x}"))))
        .collect::<Vec<_>>()
        .join(" ")
}

/// Read-only hunt for where the game keeps the map the player is standing in: every u32 shaped like a
/// map id (0xAABB0000, AA a known map group) in the game manager, the event manager, the player and
/// anything they point directly at. Comparing two F9 presses in different maps, with no rest between
/// them, leaves the fields that follow the player rather than the last bonfire.
fn map_id_candidates(addrs: &Addresses) -> String {
    const SCAN: usize = 0x800;
    let is_map_id = |v: u32| v & 0xffff == 0 && matches!(v >> 24, 10 | 20 | 40 | 50) && (v >> 16) & 0xff != 0;
    let scan = |name: &str, base: usize, out: &mut Vec<String>| {
        let Some(bytes) = mem::read_bytes(base, SCAN) else { return };
        for (i, w) in bytes.as_chunks::<4>().0.iter().enumerate() {
            let v = u32::from_le_bytes(*w);
            if is_map_id(v) {
                out.push(format!("    {name}+{:#x} = {v:#x}", i * 4));
            }
        }
    };
    let mut out = Vec::new();
    let roots = [("BaseA", addrs.base_a()), ("EventManager", addrs.event_manager()), ("PlayerCtrl", addrs.player_ctrl())];
    for (name, base) in roots {
        let Some(base) = base else { continue };
        scan(name, base, &mut out);
        let Some(bytes) = mem::read_bytes(base, SCAN) else { continue };
        for (i, q) in bytes.as_chunks::<8>().0.iter().enumerate() {
            let child = u64::from_le_bytes(*q) as usize;
            // User-mode heap and module pointers only; everything else in here is plain data.
            if (0x10000..0x7fff_ffff_ffff).contains(&child) {
                scan(&format!("{name}->{:#x}", i * 8), child, &mut out);
            }
        }
    }
    if out.is_empty() {
        "    (none)".into()
    } else {
        out.join("\n")
    }
}

pub(crate) fn fmt_snapshot(s: &Snapshot) -> String {
    let pos = s
        .pos
        .map(|p| format!("y={:.3} z={:.3} x={:.3} ang=({:.3},{:.3},{:.3})", p[0], p[1], p[2], p[3], p[4], p[5]))
        .unwrap_or_else(|| "none".into());
    format!(
        "pos[{pos}] last_bonfire={:?} area={} loading={:?} game_state={:?}",
        s.last_bonfire,
        s.last_bonfire_area.map_or("none".to_string(), |a| format!("{a:#x}")),
        s.loading,
        s.game_state
    )
}

/// Hex dump, 16 bytes a row, with each row also read as four u32 and four f32.
pub(crate) fn dump(addr: usize, len: usize) -> String {
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
