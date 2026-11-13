//! In-game regression test. `scripts/regression.ps1` drops this DLL into `Game\dllMods\` for one launch
//! per scenario, next to the mods under test, and reads the results file it writes.
//!
//! It loads the most recent save with a virtual controller, then plays through what has broken before:
//! item text, placing a pocket bonfire, a double use, dying and respawning at it (with a check for
//! damage from the move, which wore out a Silvercat Ring), resting there, and Easy Go's quit. Item uses
//! are simulated the way the game reports them: by applying the item's effect id to the player.
//!
//! `ds2_regression.cfg`, written by the runner, picks the scenario's options:
//! - `cycles`: how many times to load the save in one launch, quitting to the title in between.
//! - `extras`: also use the items mid-roll, with the menu open and right before a quit, and check the
//!   edge cases: no bonfire in the map, and a real bonfire's rest taking over.
//! - `block_network`: block the network here, for scenarios where the mods are told not to.
//!
//! Each step writes `PASS`, `FAIL`, `SKIP` or `INFO` and a reason to `ds2_regression.results`.
//! The runner checks the mods' own logs and the crash records on top of that.

#![cfg(all(windows, target_pointer_width = "64"))]

mod suspend_trace;

use ds2::game::{self, Addresses, ItemText, POCKET_BONFIRE_ITEM};
use ds2::{log, main_thread, scan};
use minhook::MinHook;
use std::ffi::c_void;
use std::fs::OpenOptions;
use std::io::Write;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::OnceLock;
use std::time::{Duration, Instant};

const DLL_PROCESS_ATTACH: u32 = 1;
const EASY_GO_ITEM: u32 = 60_595_010;
const MAIN_THREAD_WAIT: Duration = Duration::from_secs(3);
/// Pocket Bonfire ignores uses for 3 s after one does something; waiting longer starts afresh.
const AFTER_COOLDOWN: Duration = Duration::from_millis(3500);
const PAD_START: u16 = 0x0010;
/// Easy Come, Easy Go's quit, `FeGroupInGameReturnTitleCheck`'s "yes": saves and goes to the title.
const AOB_QUIT_TO_TITLE: &str = "48 83 EC 28 48 8B 0D ? ? ? ? 48 8B 89 E0 22 00 00 E8 ? ? ? ? 48 8B 05 ? ? ? ? 33 D2";

static RESULTS: OnceLock<PathBuf> = OnceLock::new();
static DIR: OnceLock<PathBuf> = OnceLock::new();
static NETWORK_REFUSED: AtomicBool = AtomicBool::new(false);

struct Config {
    scenario: String,
    cycles: u32,
    extras: bool,
    block_network: bool,
    trace_suspends: bool,
}

impl Config {
    fn load() -> Config {
        let mut c = Config { scenario: "default".into(), cycles: 1, extras: false, block_network: false, trace_suspends: false };
        let text = DIR.get().and_then(|d| std::fs::read_to_string(d.join("ds2_regression.cfg")).ok()).unwrap_or_default();
        for line in text.lines() {
            let Some((k, v)) = line.split_once('=') else { continue };
            let (k, v) = (k.trim(), v.trim());
            match k {
                "scenario" => c.scenario = v.to_string(),
                "cycles" => c.cycles = v.parse().unwrap_or(1).clamp(1, 10),
                "extras" => c.extras = v == "true",
                "block_network" => c.block_network = v == "true",
                "trace_suspends" => c.trace_suspends = v == "true",
                _ => {}
            }
        }
        c
    }
}

#[no_mangle]
pub extern "system" fn DllMain(_module: *mut c_void, reason: u32, _reserved: *mut c_void) -> i32 {
    if reason == DLL_PROCESS_ATTACH {
        let dir = ds2::module::module_path_of(DllMain as *const () as usize)
            .and_then(|p| p.parent().map(|d| d.to_path_buf()))
            .unwrap_or_default();
        let _ = DIR.set(dir.clone());
        let config = Config::load();
        // Who suspends which thread, for a freeze that leaves threads suspended. Off by default: the
        // tracing changes the timing enough that the freeze it's for stopped happening.
        unsafe {
            if config.trace_suspends && suspend_trace::install(&dir).is_ok() {
                let _ = MinHook::enable_all_hooks();
            }
        }
        // When the mods are told to leave the network alone, the test still keeps a modded game
        // offline. If that can't be done, the game doesn't get to run at all.
        if config.block_network && !unsafe { block_network() } {
            unsafe { TerminateProcess(GetCurrentProcess(), 3) };
        }
        std::thread::spawn(run);
    }
    1
}

#[link(name = "kernel32")]
extern "system" {
    fn GetModuleHandleW(name: *const u16) -> *mut c_void;
    fn ExitProcess(code: u32) -> !;
    fn TerminateProcess(process: *mut c_void, code: u32) -> i32;
    fn GetCurrentProcess() -> *mut c_void;
}

unsafe fn block_network() -> bool {
    MinHook::create_hook_api("ws2_32", "WSAStartup", wsa_startup_detour as *mut c_void)
        .and_then(|_| MinHook::enable_all_hooks())
        .is_ok()
}

unsafe extern "system" fn wsa_startup_detour(_version: u16, _data: *mut c_void) -> i32 {
    NETWORK_REFUSED.store(true, Ordering::SeqCst);
    10091 // WSASYSNOTREADY
}

fn record(status: &str, step: &str, detail: impl AsRef<str>) {
    let line = format!("{status} {step}: {}", detail.as_ref());
    log::line(line.clone());
    if let Some(path) = RESULTS.get() {
        if let Ok(mut f) = OpenOptions::new().create(true).append(true).open(path) {
            let _ = writeln!(f, "{line}");
        }
    }
}

fn pass(step: &str, detail: impl AsRef<str>) {
    record("PASS", step, detail);
}

fn fail(step: &str, detail: impl AsRef<str>) {
    record("FAIL", step, detail);
}

fn skip(step: &str, detail: impl AsRef<str>) {
    record("SKIP", step, detail);
}

fn info(step: &str, detail: impl AsRef<str>) {
    record("INFO", step, detail);
}

/// Polls `check` every 100 ms until it's true or `limit` runs out.
fn wait_for(limit: Duration, mut check: impl FnMut() -> bool) -> bool {
    let start = Instant::now();
    while start.elapsed() < limit {
        if check() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    false
}

fn module_loaded(name: &str) -> bool {
    let wide: Vec<u16> = name.encode_utf16().chain(Some(0)).collect();
    !unsafe { GetModuleHandleW(wide.as_ptr()) }.is_null()
}

/// How many lines of a mod's log contain `text`, so a step can check what the mod did.
fn log_count(file: &str, text: &str) -> usize {
    DIR.get()
        .and_then(|d| std::fs::read_to_string(d.join(file)).ok())
        .map(|t| t.lines().filter(|l| l.contains(text)).count())
        .unwrap_or(0)
}

/// Uses an item the way the game reports it: by applying its effect id to the player.
fn use_item(a: Addresses, id: u32) -> bool {
    main_thread::call("use item", MAIN_THREAD_WAIT, move || unsafe { a.apply_sp_effect(id as i32) }) == Some(true)
}

fn distance(a: [f32; 3], b: [f32; 6]) -> f32 {
    (0..3).map(|i| (a[i] - b[i]).powi(2)).sum::<f32>().sqrt()
}

fn at_title(a: Addresses) -> bool {
    let s = a.snapshot();
    s.game_state.is_some() && s.last_bonfire.is_none()
}

/// Everything a step needs about the run.
#[derive(Clone, Copy)]
struct Run {
    a: Addresses,
    quit_to_title: usize,
    pocket: bool,
    easy: bool,
    extras: bool,
}

impl Run {
    /// Saves and goes to the title, as confirming Quit Game in the System menu does.
    fn quit_to_title(&self) -> bool {
        if self.quit_to_title == 0 {
            return false;
        }
        let quit = self.quit_to_title;
        main_thread::call("quit to title", MAIN_THREAD_WAIT, move || unsafe {
            let f: unsafe extern "system" fn(usize) = std::mem::transmute(quit);
            f(0);
        })
        .is_some()
    }
}

fn run() {
    let dll = DllMain as *const () as usize;
    log::init(dll, "ds2_regression.log");
    let dir = DIR.get().cloned().unwrap_or_default();
    let _ = RESULTS.set(dir.join("ds2_regression.results"));
    let config = Config::load();
    let started = Instant::now();

    let module = match game::game_module() {
        Ok(m) => m,
        Err(e) => return fail("start", e.to_string()),
    };
    let a = match game::resolve(&module) {
        Ok(a) => a,
        Err(e) => return fail("start", e.to_string()),
    };
    if let Err(e) = ds2::hooks::serialized(|| main_thread::install(&module)) {
        return fail("start", format!("main thread: {e}"));
    }
    let r = Run {
        a,
        quit_to_title: scan::find_function(&module, AOB_QUIT_TO_TITLE).unwrap_or(0),
        pocket: module_loaded("pocket_bonfire.dll"),
        easy: module_loaded("easy_come_easy_go.dll"),
        extras: config.extras,
    };
    info(
        "scenario",
        format!(
            "{}: {} cycles, extras {}, test blocks the network {}; pocket_bonfire.dll {}, easy_come_easy_go.dll {}, ds2_hookfirst.dll {}",
            config.scenario,
            config.cycles,
            config.extras,
            config.block_network,
            r.pocket,
            r.easy,
            module_loaded("ds2_hookfirst.dll")
        ),
    );

    // The title: the game has loaded its params once the item table pointer exists.
    let mut polls = 0u32;
    if !wait_for(Duration::from_secs(180), || {
        main_thread::find_main_thread();
        polls += 1;
        if polls % 100 == 0 {
            log::line(format!(
                "waiting for the title: item table {}, main thread running {}",
                a.item_param().is_some(),
                main_thread::is_running()
            ));
        }
        a.item_param().is_some() && main_thread::is_running()
    }) {
        return fail("title", "the game didn't reach the title within 3 minutes");
    }
    std::thread::sleep(Duration::from_secs(3));
    pass("title", format!("reached after {:.1} s", started.elapsed().as_secs_f32()));
    if config.block_network {
        info("network", format!("blocked by the test; a connection was refused: {}", NETWORK_REFUSED.load(Ordering::SeqCst)));
    }

    for cycle in 1..=config.cycles {
        let last = cycle == config.cycles;
        if config.cycles > 1 {
            info("cycle", format!("{cycle} of {}", config.cycles));
        }
        load_save(a);
        if cycle == 1 {
            check_text(a, r.pocket, r.easy);
        }
        if r.pocket {
            if r.extras && cycle == 1 {
                no_bonfire_in_map(a);
            }
            pocket_bonfire(a);
            if r.extras && cycle == 1 {
                busy_uses(r);
            }
            if r.extras && last {
                real_bonfire_takes_over(a);
            }
        } else if cycle == 1 {
            skip("pocket bonfire", "not installed");
        }
        if !last {
            back_to_title(r, cycle == 1);
        }
    }

    if r.easy {
        std::thread::sleep(Duration::from_secs(4));
        info("easy go", "using it: the game should save, reach the title and close");
        if r.extras {
            // Mid-roll, as a cancelled animation would be.
            std::thread::spawn(|| main_thread::press_pad(main_thread::PAD_B, Duration::from_millis(200)));
            std::thread::sleep(Duration::from_millis(50));
        }
        if !use_item(a, EASY_GO_ITEM) {
            fail("easy go", "couldn't simulate the use");
        }
        // The runner checks the game closes. If it hasn't after a while, say so and close it.
        std::thread::sleep(Duration::from_secs(90));
        fail("easy go", "the game was still running 90 s after using Easy Go");
    } else {
        skip("easy go", "not installed");
        info("done", "closing the game");
    }
    unsafe { ExitProcess(0) };
}

/// Loads the most recent save: confirm on the title menu, then on the save list, with a virtual
/// controller's A button, with B and Start now and then for the network check's dialogs. The state is logged every few presses, so a run that doesn't get in shows
/// where it stood. Ends the run if it can't.
fn load_save(a: Addresses) {
    let mut presses = 0u32;
    let loaded = wait_for(Duration::from_secs(240), || {
        if a.playable() {
            return true;
        }
        let s = a.snapshot();
        if presses % 4 == 0 {
            log::line(format!(
                "load save: {presses} presses so far, game_state {:?}, loading {:?}, player {}",
                s.game_state,
                s.loading,
                a.player_ctrl().is_some()
            ));
        }
        // Only press while nothing is loading: a press during a load can land on the next screen.
        if s.loading.is_some_and(|l| l != 0) {
            return false;
        }
        // Mostly A, which confirms the title menu and the save list. B now and then answers "Cancel"
        // when the network check can't log in ("OK" retries, "Cancel" starts offline), and Start
        // gets past a "press start" screen.
        const CYCLE: [u16; 4] = [main_thread::PAD_A, main_thread::PAD_A, main_thread::PAD_B, PAD_START];
        let button = CYCLE[presses as usize % CYCLE.len()];
        main_thread::press_pad(button, Duration::from_millis(150));
        presses += 1;
        std::thread::sleep(Duration::from_millis(1500));
        false
    });
    if !loaded {
        fail("load save", "never got control of a character");
        // Time for the runner to take a screenshot of where it got stuck.
        std::thread::sleep(Duration::from_secs(20));
        unsafe { ExitProcess(1) };
    }
    std::thread::sleep(Duration::from_secs(4));
    let name = a.character_name().map(|n| String::from_utf16_lossy(&n)).unwrap_or_default();
    pass("load save", format!("playing as {name}"));
}

fn check_text(a: Addresses, pocket: bool, easy: bool) {
    let cases = [(pocket, POCKET_BONFIRE_ITEM, "Pocket Bonfire"), (easy, EASY_GO_ITEM, "Easy Go")];
    for (installed, id, want) in cases {
        let step = format!("{want} text");
        if !installed {
            skip(&step, "not installed");
            continue;
        }
        let name = main_thread::call("item text", MAIN_THREAD_WAIT, move || unsafe { a.item_text(ItemText::Name, id) });
        let detail = main_thread::call("item text", MAIN_THREAD_WAIT, move || unsafe { a.item_text(ItemText::Detailed, id) });
        match (name.flatten(), detail.flatten()) {
            (Some(n), Some(d)) if n == want && !d.is_empty() => pass(&step, "name and description present"),
            (n, d) => fail(&step, format!("name {n:?}, description present {}", d.is_some_and(|d| !d.is_empty()))),
        }
    }
}

/// Dies, waits for control to come back, and returns where the game first put the player and where
/// they stood a few seconds later, after any move by the mod.
fn die_and_respawn(a: Addresses, step: &str) -> Option<([f32; 6], [f32; 6])> {
    let before = a.hp();
    log::line(format!("{step}: HP before dying: {before:?}"));
    if !main_thread::call("kill", MAIN_THREAD_WAIT, move || a.set_hp(0)).unwrap_or(false) {
        fail(step, format!("couldn't set HP, read {before:?}"));
        return None;
    }
    if !wait_for(Duration::from_secs(30), || !a.playable()) {
        fail(step, "the player didn't die");
        return None;
    }
    // Watch closely for control coming back, to see where the game put the player before the mod
    // moved them.
    let deadline = Instant::now() + Duration::from_secs(240);
    let mut first = None;
    while Instant::now() < deadline {
        if a.playable() {
            first = a.snapshot().pos;
            break;
        }
        std::thread::sleep(Duration::from_millis(2));
    }
    let Some(first) = first else {
        fail(step, "never got control back after dying");
        return None;
    };
    std::thread::sleep(Duration::from_secs(5));
    let Some(settled) = a.snapshot().pos else {
        fail(step, "no position after respawning");
        return None;
    };
    Some((first, settled))
}

fn pocket_bonfire(a: Addresses) {
    let s = a.snapshot();
    let map = a.current_map();
    let Some(here) = s.pos else { return fail("place", "no position") };
    if s.last_bonfire.is_none_or(|b| b == 0) || s.last_bonfire_area != map {
        return skip("pocket bonfire", "the save's last bonfire isn't in this map, so nothing can be placed");
    }
    let spot = [here[0], here[1], here[2]];

    // Place, then use it twice in quick succession, which used to start overlapping game calls.
    let placed_before = log_count("pocket_bonfire.log", "placed:");
    let rests_before = log_count("pocket_bonfire.log", "opened=true");
    if !use_item(a, POCKET_BONFIRE_ITEM) {
        return fail("place", "couldn't simulate the use");
    }
    std::thread::sleep(Duration::from_millis(300));
    use_item(a, POCKET_BONFIRE_ITEM);
    std::thread::sleep(Duration::from_millis(150));
    use_item(a, POCKET_BONFIRE_ITEM);
    std::thread::sleep(Duration::from_secs(4));
    // Standing beside an earlier pocket bonfire, the use rests there instead, which also makes it
    // the respawn point. Either is fine; both at once isn't.
    let placed = log_count("pocket_bonfire.log", "placed:") - placed_before;
    let rested = log_count("pocket_bonfire.log", "opened=true") - rests_before;
    if placed + rested == 1 {
        pass("place", if placed == 1 { "placed" } else { "already here, so it rested instead" });
    } else {
        fail("place", format!("placed {placed} times and rested {rested} times from three quick uses"));
    }
    close_menus(a);
    if a.playable() && !a.rest_menu_open() {
        pass("double use", "the game is still fine after three quick uses");
    } else {
        fail("double use", format!("playable {}, rest menu open {}", a.playable(), a.rest_menu_open()));
    }

    // Die, and check the respawn lands on the spot without costing health.
    let Some((first, settled)) = die_and_respawn(a, "respawn") else { return };
    let d = distance(spot, settled);
    if d < 1.5 {
        pass("respawn", "back at the pocket bonfire");
    } else {
        fail("respawn", format!("{d:.1} m from the spot"));
    }
    // How far the move went, and which way: a long drop is where fall damage would show.
    info(
        "respawn move",
        format!(
            "the game put the player at ({:.2}, {:.2}, {:.2}), the spot is ({:.2}, {:.2}, {:.2}), {:.1} m away",
            first[0],
            first[1],
            first[2],
            spot[0],
            spot[1],
            spot[2],
            distance(spot, first)
        ),
    );
    // Hollowing caps HP below the maximum, so the respawn's HP is compared with the HP after resting,
    // which refills to that cap.
    let respawn_hp = a.hp();
    log::line(format!("HP after respawning: {respawn_hp:?}"));

    // Rest at it, then leave the menu.
    std::thread::sleep(Duration::from_secs(2));
    if !use_item(a, POCKET_BONFIRE_ITEM) {
        return fail("rest", "couldn't simulate the use");
    }
    if wait_for(Duration::from_secs(8), || a.rest_menu_open()) {
        pass("rest", "the bonfire menu opened");
    } else {
        fail("rest", "the bonfire menu didn't open");
    }
    std::thread::sleep(Duration::from_secs(2));
    let rested_hp = a.hp();
    match (respawn_hp, rested_hp) {
        (Some((hp, _)), Some((full, max))) if hp >= full => {
            pass("respawn damage", format!("none: {hp} after respawning, {full}/{max} after resting"))
        }
        (Some((hp, _)), Some((full, max))) => {
            fail("respawn damage", format!("the move cost health: {hp} after respawning, {full}/{max} after resting"))
        }
        (r, f) => fail("respawn damage", format!("couldn't read HP: {r:?} after respawning, {f:?} after resting")),
    }
    close_menus(a);
    std::thread::sleep(AFTER_COOLDOWN);
}

/// With the last bonfire in another map, using the item mustn't place anything: arriving there would
/// drop the player through the world. The test points the last bonfire elsewhere for a moment.
fn no_bonfire_in_map(a: Addresses) {
    let s = a.snapshot();
    let (Some(host), Some(area)) = (s.last_bonfire, s.last_bonfire_area) else {
        return skip("no bonfire in map", "no last bonfire to move");
    };
    let elsewhere = if area == 0x0a0a_0000 { 0x0a0b_0000 } else { 0x0a0a_0000 };
    let (refusals, rests) = (log_count("pocket_bonfire.log", "not placed:"), log_count("pocket_bonfire.log", "opened=true"));
    if !a.set_last_bonfire(host, elsewhere) {
        return skip("no bonfire in map", "couldn't move the last bonfire");
    }
    let used = use_item(a, POCKET_BONFIRE_ITEM);
    std::thread::sleep(Duration::from_secs(2));
    let restored = a.set_last_bonfire(host, area);
    let refused = log_count("pocket_bonfire.log", "not placed:") > refusals;
    let rested = log_count("pocket_bonfire.log", "opened=true") > rests;
    if !restored {
        fail("no bonfire in map", "couldn't put the last bonfire back");
    } else if !used {
        fail("no bonfire in map", "couldn't simulate the use");
    } else if rested {
        skip("no bonfire in map", "the save stands beside a pocket bonfire, so the use rested instead");
    } else if refused {
        pass("no bonfire in map", "nothing placed");
    } else {
        fail("no bonfire in map", "the mod didn't refuse, or didn't say so");
    }
    close_menus(a);
    std::thread::sleep(AFTER_COOLDOWN);
}

/// Uses the item when the game is busy: mid-roll and with the system menu open. Using it right before
/// a quit is in `back_to_title`. The game has to come through each in control.
fn busy_uses(r: Run) {
    let a = r.a;
    std::thread::spawn(|| main_thread::press_pad(main_thread::PAD_B, Duration::from_millis(200)));
    std::thread::sleep(Duration::from_millis(50));
    use_item(a, POCKET_BONFIRE_ITEM);
    std::thread::sleep(Duration::from_secs(3));
    close_menus(a);
    if wait_for(Duration::from_secs(10), || a.playable()) {
        pass("use mid-roll", "still in control");
    } else {
        fail("use mid-roll", "not in control afterwards");
    }
    std::thread::sleep(AFTER_COOLDOWN);

    main_thread::press_pad(PAD_START, Duration::from_millis(150));
    std::thread::sleep(Duration::from_secs(1));
    use_item(a, POCKET_BONFIRE_ITEM);
    std::thread::sleep(Duration::from_secs(3));
    // Back out of whatever is open: B closes the bonfire menu and the system menu, and only rolls
    // once they're gone.
    for _ in 0..4 {
        main_thread::press_pad(main_thread::PAD_B, Duration::from_millis(150));
        std::thread::sleep(Duration::from_millis(1200));
    }
    close_menus(a);
    if wait_for(Duration::from_secs(10), || a.playable() && !a.rest_menu_open()) {
        pass("use with the menu open", "still in control");
    } else {
        fail("use with the menu open", "not in control afterwards");
    }
    std::thread::sleep(AFTER_COOLDOWN);
}

/// Sitting at a real bonfire makes it the respawn point again. The test opens a real bonfire's rest
/// menu without the item, then dies: the mod mustn't move the player.
fn real_bonfire_takes_over(a: Addresses) {
    let Some(host) = a.snapshot().last_bonfire.filter(|&h| h != 0) else {
        return skip("real bonfire takes over", "no last bonfire");
    };
    let moves = log_count("pocket_bonfire.log", "respawned at the pocket bonfire");
    let opened = main_thread::call("real rest", MAIN_THREAD_WAIT, move || unsafe { a.rest_at_bonfire(host) }).unwrap_or(false);
    if !opened || !wait_for(Duration::from_secs(8), || a.rest_menu_open()) {
        return fail("real bonfire takes over", "couldn't open a real bonfire's menu");
    }
    std::thread::sleep(Duration::from_secs(2));
    close_menus(a);
    if log_count("pocket_bonfire.log", "rested at a real bonfire") == 0 {
        fail("real bonfire takes over", "the mod didn't notice the rest");
    }
    if die_and_respawn(a, "real bonfire takes over").is_none() {
        return;
    }
    if log_count("pocket_bonfire.log", "respawned at the pocket bonfire") == moves {
        pass("real bonfire takes over", "respawned at the real bonfire, not moved");
    } else {
        fail("real bonfire takes over", "the mod still moved the player to the pocket bonfire");
    }
}

/// Quits to the title, waits for it, and leaves the next load to `load_save`. On the first cycle with
/// extras, the item is used in the same moment as the quit, as a cancelled animation would be.
fn back_to_title(r: Run, busy: bool) {
    let a = r.a;
    if busy && r.extras && r.pocket {
        use_item(a, POCKET_BONFIRE_ITEM);
    }
    if !r.quit_to_title() {
        fail("quit to title", "couldn't call the game's quit");
        unsafe { ExitProcess(1) };
    }
    if wait_for(Duration::from_secs(120), || at_title(a)) {
        pass("quit to title", "back at the title");
    } else {
        fail("quit to title", "never reached the title");
        unsafe { ExitProcess(1) };
    }
    std::thread::sleep(Duration::from_secs(3));
}

/// Backs out of the bonfire menu with B.
fn close_menus(a: Addresses) {
    if !a.rest_menu_open() {
        return;
    }
    let closed = wait_for(Duration::from_secs(20), || {
        if !a.rest_menu_open() && a.playable() {
            return true;
        }
        main_thread::press_pad(main_thread::PAD_B, Duration::from_millis(150));
        std::thread::sleep(Duration::from_millis(1500));
        false
    });
    if closed {
        pass("leave menu", "back in control");
    } else {
        fail("leave menu", "couldn't back out of the bonfire menu");
    }
}
