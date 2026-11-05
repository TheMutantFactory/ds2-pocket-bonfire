//! The Pocket Bonfire. See `docs/design.md`.
//!
//! A pocket bonfire is a saved position on a real "host" bonfire in the same map. Using the item
//! places it, or, standing beside it, rests there. Once placed, dying brings the player back: the game
//! respawns them at the host, and the DLL moves them to the saved position.
//!
//! Not in this version: Travel to it from other bonfires, the limits on where it can be placed, and
//! anything visible at the spot.

#![cfg(all(windows, target_pointer_width = "64"))]

mod state;

use ds2::game::{self, Addresses, Snapshot, GAME_STATE_PLAYABLE, POCKET_BONFIRE_ITEM};
use ds2::log;
use minhook::MinHook;
use state::{Bonfire, Store};
use std::ffi::c_void;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::OnceLock;
use std::time::{Duration, Instant};

const DLL_PROCESS_ATTACH: u32 = 1;
const TICK: Duration = Duration::from_millis(50);
/// How close counts as "beside" the bonfire: using the item within this rests instead of moving it.
const USE_RADIUS: f32 = 3.0;
/// Loads with 4K textures can be slow; this only gives up on the move, never the game.
const ARRIVE_TIMEOUT: Duration = Duration::from_secs(180);

static ADDRS: OnceLock<Addresses> = OnceLock::new();
static ORIG_APPLY_SP_EFFECT: AtomicUsize = AtomicUsize::new(0);
static ORIG_WARP_FUNC: AtomicUsize = AtomicUsize::new(0);

/// The hooks only raise these; the game's threads never run the mod's logic.
static ITEM_USED: AtomicBool = AtomicBool::new(false);
static WARP_SEEN: AtomicBool = AtomicBool::new(false);

#[link(name = "kernel32")]
extern "system" {
    fn Beep(frequency: u32, duration_ms: u32) -> i32;
}

/// The only feedback for now, since nothing shows at the spot. `Beep` blocks, hence the thread.
fn beep(accepted: bool) {
    std::thread::spawn(move || unsafe {
        if accepted {
            Beep(1200, 80);
        } else {
            Beep(300, 250);
        }
    });
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
    let dll = DllMain as *const () as usize;
    log::init(dll, "pocket_bonfire.log");
    let addrs = match game::resolve() {
        Ok(a) => a,
        Err(e) => {
            log::line(format!("disabled: {e}"));
            return;
        }
    };
    let _ = ADDRS.set(addrs);

    let hooked = unsafe {
        install(addrs.apply_sp_effect, apply_sp_effect_detour as *mut c_void, &ORIG_APPLY_SP_EFFECT)
            .and_then(|_| install(addrs.warp_func, warp_func_detour as *mut c_void, &ORIG_WARP_FUNC))
            .and_then(|_| MinHook::enable_all_hooks().map_err(|e| format!("enable: {e:?}")))
    };
    if let Err(e) = hooked {
        log::line(format!("disabled, hooks failed: {e}"));
        return;
    }
    log::line("ready".into());

    let dir = ds2::module::module_path_of(dll)
        .and_then(|p| p.parent().map(|d| d.join("pocket_bonfire")))
        .unwrap_or_else(|| PathBuf::from("pocket_bonfire"));
    Mod::new(Store::new(dir)).run(&addrs);
}

unsafe fn install(target: usize, detour: *mut c_void, orig: &AtomicUsize) -> Result<(), String> {
    let o = MinHook::create_hook(target as *mut c_void, detour).map_err(|e| format!("create {target:#x}: {e:?}"))?;
    orig.store(o as usize, Ordering::SeqCst);
    Ok(())
}

type ApplySpEffectFn = unsafe extern "system" fn(usize, *const u8, u64, u64, u64, u64, u64, u64) -> u64;
type WarpFuncFn = unsafe extern "system" fn(*mut u8, *mut u8) -> u64;

/// `ApplySpEffect(ctrl, effect_struct, ...)`. The full argument list isn't known, so the stack slots
/// pass through raw. Enemies use this too, so the effect id is checked first, and the pointer reads
/// behind the player check only happen for the item's id.
unsafe extern "system" fn apply_sp_effect_detour(
    ctrl: usize,
    effect: *const u8,
    r8: u64,
    r9: u64,
    s5: u64,
    s6: u64,
    s7: u64,
    s8: u64,
) -> u64 {
    let orig: ApplySpEffectFn = std::mem::transmute(ORIG_APPLY_SP_EFFECT.load(Ordering::SeqCst));
    let ret = orig(ctrl, effect, r8, r9, s5, s6, s7, s8);
    let id = ds2::mem::read::<i32>(effect as usize).unwrap_or(0);
    if id == POCKET_BONFIRE_ITEM as i32 && ADDRS.get().and_then(|a| a.sp_effect_ctrl()) == Some(ctrl) {
        ITEM_USED.store(true, Ordering::SeqCst);
    }
    ret
}

unsafe extern "system" fn warp_func_detour(manager: *mut u8, warp: *mut u8) -> u64 {
    let orig: WarpFuncFn = std::mem::transmute(ORIG_WARP_FUNC.load(Ordering::SeqCst));
    WARP_SEEN.store(true, Ordering::SeqCst);
    orig(manager, warp)
}

struct Mod {
    store: Store,
    character: Option<Vec<u16>>,
    bonfire: Option<Bonfire>,
    /// A respawn is loading; move the player when it finishes.
    arriving: Option<Instant>,
    /// A rest menu the mod opened is up, so rest effects come from the mod's own rest.
    our_rest_open: bool,
}

impl Mod {
    fn new(store: Store) -> Mod {
        Mod { store, character: None, bonfire: None, arriving: None, our_rest_open: false }
    }

    fn run(mut self, addrs: &Addresses) {
        let mut last = Snapshot::default();
        loop {
            std::thread::sleep(TICK);
            let now = addrs.snapshot();
            self.tick(addrs, &now, &last);
            last = now;
        }
    }

    fn tick(&mut self, addrs: &Addresses, now: &Snapshot, last: &Snapshot) {
        let playable = now.game_state == Some(GAME_STATE_PLAYABLE);
        let was_playable = last.game_state == Some(GAME_STATE_PLAYABLE);

        if playable {
            self.load_character(addrs);
        }
        self.watch_rests(addrs);
        if ITEM_USED.swap(false, Ordering::SeqCst) {
            self.on_item_used(addrs, now);
        }

        // Order matters: decide about this load before the flag it depends on is cleared.
        let reload = was_playable && !playable && !WARP_SEEN.load(Ordering::SeqCst);
        if reload && self.bonfire.is_some_and(|b| b.respawn_here) {
            self.arriving = Some(Instant::now());
        }
        if playable && !was_playable {
            WARP_SEEN.store(false, Ordering::SeqCst);
        }
        self.finish_arrival(addrs, now);
    }

    /// Saved state is per character. Reading it again each time the player gains control would be
    /// wasteful, so it only happens when the name changes.
    fn load_character(&mut self, addrs: &Addresses) {
        let Some(name) = addrs.character_name() else { return };
        if self.character.as_ref() != Some(&name) {
            self.bonfire = self.store.load(&name);
            log::line(format!("character {}: {:?}", String::from_utf16_lossy(&name), self.bonfire));
            self.character = Some(name);
        }
    }

    /// A rest menu the mod didn't open means the player sat at a real bonfire, and that's where they
    /// respawn now, so the pocket bonfire stops being the respawn point. This matters most for its own
    /// host, where LastBonfire can't show the difference. The rest effect can't be used for this:
    /// the game also applies it during a death respawn.
    fn watch_rests(&mut self, addrs: &Addresses) {
        let open = addrs.rest_menu_open();
        if self.our_rest_open {
            self.our_rest_open = open;
        } else if open {
            self.stop_respawning_here("rested at a real bonfire");
        }
    }

    fn stop_respawning_here(&mut self, why: &str) {
        if let Some(mut bonfire) = self.bonfire.filter(|b| b.respawn_here) {
            bonfire.respawn_here = false;
            self.save(bonfire);
            log::line(format!("{why}, no longer the respawn point"));
        }
    }

    fn on_item_used(&mut self, addrs: &Addresses, now: &Snapshot) {
        if !addrs.playable() {
            log::line("item used, but the player isn't in control".into());
            return;
        }
        self.load_character(addrs);
        let Some(here) = now.pos else { return };
        let map = addrs.current_map();
        let beside = self.bonfire.filter(|b| Some(b.map) == map && b.distance_to(here) <= USE_RADIUS);
        match beside {
            Some(bonfire) => self.rest(addrs, bonfire),
            None => self.place(now, map, here),
        }
    }

    fn rest(&mut self, addrs: &Addresses, mut bonfire: Bonfire) {
        self.our_rest_open = true;
        let opened = unsafe { addrs.rest_at_bonfire(bonfire.host) };
        let set = addrs.set_last_bonfire(bonfire.host, bonfire.map);
        bonfire.respawn_here = true;
        self.save(bonfire);
        log::line(format!("rest at host {}: opened={opened}, last bonfire set={set}", bonfire.host));
    }

    fn place(&mut self, now: &Snapshot, map: Option<u32>, here: [f32; 6]) {
        // The host must be in this map, or arriving would drop the player through the world. The last
        // bonfire is the only one known to be usable, so without it here, there's no host.
        let host = now.last_bonfire.filter(|&id| id != 0 && now.last_bonfire_area == map);
        let (Some(map), Some(host)) = (map, host) else {
            log::line(format!(
                "not placed: last bonfire {:?} in {:?}, standing in {map:?}",
                now.last_bonfire, now.last_bonfire_area
            ));
            beep(false);
            return;
        };
        // Placing alone makes it the respawn point, so the host being the last bonfire isn't just a
        // convenience: it's already where the game will send the player back to.
        let bonfire = Bonfire { map, host, pos: [here[0], here[1], here[2]], respawn_here: true };
        self.save(bonfire);
        log::line(format!("placed: {bonfire:?}"));
        beep(true);
    }

    fn save(&mut self, bonfire: Bonfire) {
        self.bonfire = Some(bonfire);
        if let Some(name) = &self.character {
            if let Err(e) = self.store.save(name, &bonfire) {
                log::line(format!("couldn't save: {e}"));
            }
        }
    }

    fn finish_arrival(&mut self, addrs: &Addresses, now: &Snapshot) {
        let Some(since) = self.arriving else { return };
        if now.game_state.is_some() && now.last_bonfire.is_none() {
            log::line("respawn move dropped, back at the title screen".into());
            self.arriving = None;
        } else if addrs.playable() {
            self.arriving = None;
            let Some(bonfire) = self.bonfire else { return };
            // Writing into a map that isn't loaded drops the player through the world, so check both.
            if now.last_bonfire == Some(bonfire.host) && addrs.current_map() == Some(bonfire.map) {
                let moved = addrs.set_position(bonfire.pos);
                log::line(format!("respawned at the pocket bonfire, moved={moved}"));
            } else {
                log::line(format!(
                    "respawn move skipped: last bonfire {:?} in {:?}, wanted {} in {:#x}",
                    now.last_bonfire,
                    addrs.current_map(),
                    bonfire.host,
                    bonfire.map
                ));
                self.stop_respawning_here("respawned elsewhere");
            }
        } else if since.elapsed() > ARRIVE_TIMEOUT {
            log::line("respawn move dropped, load took too long".into());
            self.arriving = None;
        }
    }
}
