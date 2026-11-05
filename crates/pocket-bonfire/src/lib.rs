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

use ds2::game::{self, Addresses, ItemText, Snapshot, GAME_STATE_PLAYABLE, POCKET_BONFIRE_ITEM};
use ds2::{item, log};
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
static ORIG_FMG_LOOKUP: AtomicUsize = AtomicUsize::new(0);

/// The hooks only raise these; the game's threads never run the mod's logic.
static ITEM_USED: AtomicBool = AtomicBool::new(false);
static WARP_SEEN: AtomicBool = AtomicBool::new(false);
static TEXT_SERVED: AtomicBool = AtomicBool::new(false);

/// The item's strings as the game wants them, NUL-terminated UTF-16 that lives as long as the process.
static ITEM_TEXT: OnceLock<[Vec<u16>; 3]> = OnceLock::new();

fn utf16(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(Some(0)).collect()
}

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
    let _ = ITEM_TEXT.set([utf16(item::NAME), utf16(item::SIMPLE), utf16(item::DETAILED)]);

    let hooked = unsafe {
        install(addrs.apply_sp_effect, apply_sp_effect_detour as *mut c_void, &ORIG_APPLY_SP_EFFECT)
            .and_then(|_| install(addrs.warp_func, warp_func_detour as *mut c_void, &ORIG_WARP_FUNC))
            .and_then(|_| install(addrs.fmg_lookup, fmg_lookup_detour as *mut c_void, &ORIG_FMG_LOOKUP))
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

/// Reads a whole param from memory. It starts with its own length, as the file does.
fn read_param(addr: usize) -> Option<Vec<u8>> {
    let len = ds2::mem::read::<u32>(addr)? as usize;
    (0x40..=16 << 20).contains(&len).then(|| ds2::mem::read_bytes(addr, len)).flatten()
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

type FmgLookupFn = unsafe extern "system" fn(usize, u32) -> *const u16;

/// Supplies the item's text in whichever language is loaded, so no text files need changing. Text a
/// file really has wins, which lets another mod or a translation override it. Called constantly, so
/// anything but the item's id passes straight through.
unsafe extern "system" fn fmg_lookup_detour(fmg: usize, id: u32) -> *const u16 {
    let orig: FmgLookupFn = std::mem::transmute(ORIG_FMG_LOOKUP.load(Ordering::SeqCst));
    let found = orig(fmg, id);
    if id != POCKET_BONFIRE_ITEM || !found.is_null() {
        return found;
    }
    let index = match ADDRS.get().and_then(|a| a.item_text_kind(fmg)) {
        Some(ItemText::Name) => 0,
        Some(ItemText::Simple) => 1,
        Some(ItemText::Detailed) => 2,
        None => return found,
    };
    match ITEM_TEXT.get() {
        Some(text) => {
            TEXT_SERVED.store(true, Ordering::SeqCst);
            text[index].as_ptr()
        }
        None => found,
    }
}

/// Where the item row stands in the param the game is using right now.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ItemRow {
    /// Nothing checked yet, or the game hasn't loaded ItemParam.
    Unknown,
    /// This param has the row, so the item is safe to give.
    In(usize),
    /// This param can't take the row. Giving the item could crash the game, so it isn't given.
    Failed(usize),
}

struct Mod {
    store: Store,
    item_row: ItemRow,
    text_logged: bool,
    character: Option<Vec<u16>>,
    bonfire: Option<Bonfire>,
    /// A respawn is loading; move the player when it finishes.
    arriving: Option<Instant>,
    /// A rest menu the mod opened is up, so rest effects come from the mod's own rest.
    our_rest_open: bool,
    /// This character's item has been dealt with, so `grant_item` has nothing left to do.
    item_settled: bool,
}

impl Mod {
    fn new(store: Store) -> Mod {
        Mod {
            store,
            item_row: ItemRow::Unknown,
            text_logged: false,
            character: None,
            bonfire: None,
            arriving: None,
            our_rest_open: false,
            item_settled: false,
        }
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

        self.keep_item_row(addrs);
        if !self.text_logged && TEXT_SERVED.load(Ordering::SeqCst) {
            self.text_logged = true;
            log::line("item text served".into());
        }
        if playable {
            self.load_character(addrs);
            self.grant_item(addrs);
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

    /// Points the game at a copy of ItemParam with the item's row added, whenever the param it's using
    /// doesn't have one. The old copy stays in place, since other code may still be reading it.
    fn keep_item_row(&mut self, addrs: &Addresses) {
        let Some(current) = addrs.item_param() else { return };
        if matches!(self.item_row, ItemRow::In(p) | ItemRow::Failed(p) if p == current) {
            return;
        }
        self.item_row = match read_param(current).map(|bytes| item::add_row(&bytes)) {
            Some(Ok(Some(with_row))) => {
                let copy: &'static mut [u8] = Vec::leak(with_row);
                let new = copy.as_ptr() as usize;
                if addrs.set_item_param(new) {
                    log::line(format!("item row added: ItemParam {current:#x} -> {new:#x}, {} bytes", copy.len()));
                    ItemRow::In(new)
                } else {
                    log::line("couldn't update ItemParam's pointer".into());
                    ItemRow::Failed(current)
                }
            }
            Some(Ok(None)) => {
                log::line("ItemParam already has the item".into());
                ItemRow::In(current)
            }
            Some(Err(e)) => {
                log::line(format!("couldn't add the item row: {e}"));
                ItemRow::Failed(current)
            }
            None => {
                log::line(format!("couldn't read ItemParam at {current:#x}"));
                ItemRow::Failed(current)
            }
        };
    }

    /// Saved state is per character. Reading it again each time the player gains control would be
    /// wasteful, so it only happens when the name changes.
    fn load_character(&mut self, addrs: &Addresses) {
        let Some(name) = addrs.character_name() else { return };
        if self.character.as_ref() != Some(&name) {
            self.bonfire = self.store.load(&name);
            log::line(format!("character {}: {:?}", String::from_utf16_lossy(&name), self.bonfire));
            self.character = Some(name);
            self.item_settled = false;
        }
    }

    /// Gives each character one item, the first time it's seen in control. A character with a saved
    /// bonfire has already used one, so it counts as given. If the bag isn't available yet, this
    /// tries again on the next tick.
    fn grant_item(&mut self, addrs: &Addresses) {
        if self.item_settled || !addrs.playable() || !matches!(self.item_row, ItemRow::In(_)) {
            return;
        }
        let Some(name) = &self.character else { return };
        if self.store.item_given(name) || self.bonfire.is_some() {
            self.item_settled = true;
            return;
        }
        if unsafe { addrs.give_item(POCKET_BONFIRE_ITEM, 1) } {
            self.item_settled = true;
            log::line(format!("gave the item to {}", String::from_utf16_lossy(name)));
            if let Err(e) = self.store.mark_item_given(name) {
                log::line(format!("couldn't record the item, it will be given again next launch: {e}"));
            }
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
