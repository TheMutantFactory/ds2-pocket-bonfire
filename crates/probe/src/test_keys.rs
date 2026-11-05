//! Write tests for the Pocket Bonfire design. Each one answers a question the logs can't:
//!
//! | Keys      | Test |
//! |-----------|------|
//! | Ctrl+F5   | Save this spot, with the last bonfire as its host. |
//! | Ctrl+F6   | Move to the saved spot within the current map, 2 s after the key press. |
//! | Ctrl+F7   | Warp to the host bonfire, then move to the saved spot after the load. |
//! | Ctrl+F8   | Rest effect only (SpEffect 110000010): HP and estus. |
//! | Ctrl+F10  | Remote "rest at bonfire" call for the last bonfire. |
//! | Ctrl+F11  | Toggle: after a reload with no warp (a death), move to the saved spot. |
//!
//! Every key needs the game focused and the player in control, so nothing fires during a load or
//! a menu transition. Each move is then watched for 5 s, to catch falling through unloaded ground.
//! A high beep means the key was accepted, a low one that it was refused (the log says why).
//!
//! Ctrl+F6's delay is there to separate the key from the write: the player starts parrying after
//! it, and the position samples logged before the write show which of the two sets that off.

use crate::{dump, dump_regions, fmt_snapshot, key_is_down, log, GAME_WARP_MANAGER, OUR_WARP, WARP_SEEN};
use ds2::game::{Addresses, Snapshot, GAME_STATE_PLAYABLE, SP_EFFECT_BONFIRE_REST};
use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};

const VK_CONTROL: i32 = 0x11;
const VK_F5: i32 = 0x74;
const VK_F6: i32 = 0x75;
const VK_F7: i32 = 0x76;
const VK_F8: i32 = 0x77;
const VK_F10: i32 = 0x79;
const VK_F11: i32 = 0x7A;
const KEYS: [i32; 6] = [VK_F5, VK_F6, VK_F7, VK_F8, VK_F10, VK_F11];

/// A warp that hasn't started loading after this long has failed.
const LEAVE_TIMEOUT: Duration = Duration::from_secs(15);
/// Loads with 4K textures can be slow; this only gives up on the move, never the game.
const ARRIVE_TIMEOUT: Duration = Duration::from_secs(180);
/// When to log the position after a move, as seconds since the move.
const WATCH_AT: [f32; 6] = [0.1, 0.5, 1.0, 2.0, 3.5, 5.0];
/// Falling further than this below the target counts as "the ground wasn't there".
const FELL_BELOW: f32 = 5.0;
/// How long Ctrl+F6 waits between the key press and the write.
const MOVE_DELAY: Duration = Duration::from_secs(2);
/// How often the position is logged during that wait.
const DELAY_SAMPLE: Duration = Duration::from_millis(500);

#[link(name = "user32")]
extern "system" {
    fn GetForegroundWindow() -> *mut std::ffi::c_void;
    fn GetWindowThreadProcessId(window: *mut std::ffi::c_void, process: *mut u32) -> u32;
}

#[link(name = "kernel32")]
extern "system" {
    fn GetCurrentProcessId() -> u32;
    fn Beep(frequency: u32, duration_ms: u32) -> i32;
}

/// `Beep` blocks for its duration, so it gets its own thread to keep the sampler on time.
fn beep(accepted: bool) {
    std::thread::spawn(move || unsafe {
        if accepted {
            Beep(1200, 80);
        } else {
            Beep(300, 250);
        }
    });
}

fn game_is_focused() -> bool {
    let mut pid = 0u32;
    unsafe {
        GetWindowThreadProcessId(GetForegroundWindow(), &mut pid);
        pid == GetCurrentProcessId()
    }
}

#[derive(Clone, Copy)]
struct Spot {
    /// y, z, x, then angles, as in `Snapshot::pos`.
    pos: [f32; 6],
    host: u16,
}

impl Spot {
    fn target(&self) -> [f32; 3] {
        [self.pos[0], self.pos[1], self.pos[2]]
    }
}

#[derive(Default)]
enum Pending {
    #[default]
    Idle,
    /// Ctrl+F6 was pressed; the write happens at `at`.
    DelayedMove { at: Instant, next_sample: Instant },
    /// Our warp was requested and the load hasn't started yet.
    WaitLeave { since: Instant },
    /// A load is under way; move the player when it finishes.
    WaitArrive { since: Instant, why: &'static str },
    /// A move was written; watch where the player ends up.
    Watch { since: Instant, next: usize },
}

#[derive(Default)]
pub struct TestKeys {
    down: [bool; KEYS.len()],
    spot: Option<Spot>,
    pending: Pending,
    respawn_armed: bool,
}

impl TestKeys {
    pub fn tick(&mut self, addrs: &Addresses, now: &Snapshot, last: &Snapshot) {
        let playable = now.game_state == Some(GAME_STATE_PLAYABLE);
        let was_playable = last.game_state == Some(GAME_STATE_PLAYABLE);

        self.advance(addrs, now);

        // Order matters: decide about this load before the flag it depends on is cleared.
        if self.respawn_armed && self.spot.is_some() && matches!(self.pending, Pending::Idle) && was_playable && !playable {
            if WARP_SEEN.load(Ordering::SeqCst) {
                log::line("test: reload came from a warp, leaving it alone".into());
            } else {
                log::line("test: reload with no warp, will move to the saved spot".into());
                self.pending = Pending::WaitArrive { since: Instant::now(), why: "respawn" };
            }
        }
        if playable && !was_playable {
            WARP_SEEN.store(false, Ordering::SeqCst);
        }

        let ctrl = key_is_down(VK_CONTROL);
        for (i, &key) in KEYS.iter().enumerate() {
            let down = key_is_down(key);
            if down && !self.down[i] && ctrl && game_is_focused() {
                beep(self.press(addrs, now, key));
            }
            self.down[i] = down;
        }
    }

    /// Runs the test for `key`, returning whether it was accepted.
    fn press(&mut self, addrs: &Addresses, now: &Snapshot, key: i32) -> bool {
        // The toggle only changes what the probe does later, so it's allowed anytime.
        if key == VK_F11 {
            self.respawn_armed = !self.respawn_armed;
            log::line(format!("test: respawn move {}", if self.respawn_armed { "ARMED" } else { "off" }));
            return true;
        }
        if !addrs.playable() {
            log::line(format!("test: key {key:#x} ignored, player not in control: {}", fmt_snapshot(now)));
            return false;
        }
        if !matches!(self.pending, Pending::Idle) {
            log::line(format!("test: key {key:#x} ignored, a test is still running"));
            return false;
        }
        match key {
            VK_F5 => self.save_spot(addrs, now),
            VK_F6 => self.delay_move(now),
            VK_F7 => self.warp_then_move(addrs),
            VK_F8 => {
                let ok = unsafe { addrs.apply_sp_effect(SP_EFFECT_BONFIRE_REST) };
                log::line(format!("test: rest effect applied={ok}, before: {}", fmt_snapshot(now)));
                ok
            }
            VK_F10 => rest_call(addrs, now),
            _ => false,
        }
    }

    fn save_spot(&mut self, addrs: &Addresses, now: &Snapshot) -> bool {
        let (Some(pos), Some(host)) = (now.pos, now.last_bonfire) else {
            log::line("test: can't save, no position or last bonfire".into());
            return false;
        };
        self.spot = Some(Spot { pos, host });
        log::line(format!("test: SAVED spot, host bonfire {host}: {}\n{}", fmt_snapshot(now), dump_regions(addrs)));
        true
    }

    fn delay_move(&mut self, now: &Snapshot) -> bool {
        if self.spot.is_none() {
            log::line("test: no saved spot, press Ctrl+F5 first".into());
            return false;
        }
        log::line(format!("test: Ctrl+F6 pressed, writing in {MOVE_DELAY:?}: {}", fmt_snapshot(now)));
        let pressed = Instant::now();
        self.pending = Pending::DelayedMove { at: pressed + MOVE_DELAY, next_sample: pressed + DELAY_SAMPLE };
        true
    }

    fn move_now(&mut self, addrs: &Addresses, why: &str) {
        let Some(spot) = self.spot else {
            log::line("test: no saved spot, press Ctrl+F5 first".into());
            return;
        };
        let ok = addrs.set_position(spot.target());
        log::line(format!("test: {why} wrote position ok={ok}, target {:?}", spot.target()));
        self.pending = Pending::Watch { since: Instant::now(), next: 0 };
    }

    fn warp_then_move(&mut self, addrs: &Addresses) -> bool {
        let Some(spot) = self.spot else {
            log::line("test: no saved spot, press Ctrl+F5 first".into());
            return false;
        };
        let Some(manager) = addrs.warp_manager() else {
            log::line("test: warp refused, no warp manager".into());
            return false;
        };
        // The chain has matched the game's own value in play, so a first warp needn't wait for a Travel.
        let game = GAME_WARP_MANAGER.load(Ordering::SeqCst);
        if game != 0 && game != manager {
            log::line(format!("test: warp refused, manager mismatch: game {game:#x}, chain {manager:#x}"));
            return false;
        }
        log::line(format!("test: warping to host bonfire {} (manager {manager:#x})", spot.host));
        OUR_WARP.store(true, Ordering::SeqCst);
        let map_id = unsafe { addrs.warp_to_bonfire(manager, spot.host) };
        OUR_WARP.store(false, Ordering::SeqCst);
        // Our warp doesn't update these the way the Travel menu does.
        let ok = addrs.set_last_bonfire(spot.host, map_id);
        log::line(format!("test: set last bonfire to {} in map {map_id:#x}, ok={ok}", spot.host));
        self.pending = Pending::WaitLeave { since: Instant::now() };
        true
    }

    /// Runs the in-flight test forward.
    fn advance(&mut self, addrs: &Addresses, now: &Snapshot) {
        let playable = now.game_state == Some(GAME_STATE_PLAYABLE) && now.loading == Some(0) && now.pos.is_some();
        match self.pending {
            Pending::Idle => {}
            Pending::DelayedMove { at, next_sample } => {
                if !playable {
                    log::line(format!("test: Ctrl+F6 move dropped, player not in control: {}", fmt_snapshot(now)));
                    self.pending = Pending::Idle;
                } else if Instant::now() >= at {
                    log::line(format!("test: Ctrl+F6 about to write, last position: {}", fmt_snapshot(now)));
                    self.move_now(addrs, "Ctrl+F6");
                } else if Instant::now() >= next_sample {
                    log::line(format!("test: Ctrl+F6 waiting: {}", fmt_snapshot(now)));
                    self.pending = Pending::DelayedMove { at, next_sample: next_sample + DELAY_SAMPLE };
                }
            }
            Pending::WaitLeave { since } => {
                if now.game_state != Some(GAME_STATE_PLAYABLE) {
                    self.pending = Pending::WaitArrive { since: Instant::now(), why: "warp" };
                } else if since.elapsed() > LEAVE_TIMEOUT {
                    log::line(format!("test: warp never started loading: {}", fmt_snapshot(now)));
                    self.pending = Pending::Idle;
                }
            }
            Pending::WaitArrive { since, why } => {
                let host = self.spot.map(|s| s.host);
                if now.game_state.is_some() && now.last_bonfire.is_none() {
                    log::line(format!("test: {why} move dropped, back at the title screen"));
                    self.pending = Pending::Idle;
                } else if playable {
                    log::line(format!(
                        "test: {why} load took {:.1}s, arrived at: {}",
                        since.elapsed().as_secs_f32(),
                        fmt_snapshot(now)
                    ));
                    // A warp was aimed at the host. A respawn goes to the last bonfire, which might not
                    // be the host, and then the spot's map isn't loaded: the write would drop the
                    // player through the world.
                    if why == "warp" || now.last_bonfire == host {
                        self.move_now(addrs, why);
                    } else {
                        log::line(format!("test: {why} move skipped, last bonfire isn't the host {host:?}"));
                        self.pending = Pending::Idle;
                    }
                } else if since.elapsed() > ARRIVE_TIMEOUT {
                    log::line(format!("test: {why} move dropped, load took too long: {}", fmt_snapshot(now)));
                    self.pending = Pending::Idle;
                }
            }
            Pending::Watch { since, next } => {
                if since.elapsed().as_secs_f32() >= WATCH_AT[next] {
                    let target = self.spot.map(|s| s.target());
                    log::line(format!("test: +{:.1}s {}{}", WATCH_AT[next], fmt_snapshot(now), verdict(target, now)));
                    self.pending = match next + 1 {
                        n if n < WATCH_AT.len() => Pending::Watch { since, next: n },
                        _ => Pending::Idle,
                    };
                }
            }
        }
    }
}

/// How far the player is from the target, flagging a fall.
fn verdict(target: Option<[f32; 3]>, now: &Snapshot) -> String {
    let (Some(t), Some(p)) = (target, now.pos) else { return String::new() };
    let (dy, dz, dx) = (p[0] - t[0], p[1] - t[1], p[2] - t[2]);
    let off = (dy * dy + dz * dz + dx * dx).sqrt();
    let fell = if dz < -FELL_BELOW { "  FELL BELOW TARGET" } else { "" };
    format!("  off-target {off:.3} (height {dz:+.3}){fell}")
}

fn rest_call(addrs: &Addresses, now: &Snapshot) -> bool {
    let Some(id) = now.last_bonfire.filter(|&id| id != 0) else {
        log::line("test: rest call needs a last bonfire".into());
        return false;
    };
    let manager = addrs.event_manager().and_then(|em| ds2::mem::ptr(em + ds2::game::OFF_BONFIRE_MANAGER));
    let state = |label: &str| {
        let body = manager.map_or_else(|| "    (unavailable)".to_string(), |m| dump(m, 0x40));
        format!("  bonfire manager {label}:\n{body}")
    };
    log::line(format!("test: rest call for bonfire {id}, before: {}\n{}", fmt_snapshot(now), state("before")));
    let ok = unsafe { addrs.rest_at_bonfire(id) };
    log::line(format!("test: rest call returned, ok={ok}\n{}", state("after")));
    ok
}
