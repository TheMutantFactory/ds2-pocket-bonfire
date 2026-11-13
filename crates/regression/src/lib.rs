//! In-game regression test. `scripts/regression.ps1` drops this DLL into `Game\dllMods\` for one launch,
//! next to the mods under test, and reads the results file it writes.
//!
//! It loads the most recent save with the keyboard, then plays through what has broken before:
//! item text, placing a pocket bonfire, a double use, dying and respawning at it (with a check for
//! damage from the move, which wore out a Silvercat Ring), resting there, and Easy Go's quit. Item uses
//! are simulated the way the game reports them: by applying the item's effect id to the player.
//!
//! Each step writes `PASS`, `FAIL` or `SKIP` and a reason to `Game\dllMods\ds2_regression.results`.
//! The runner checks the mods' own logs and the crash records on top of that.

#![cfg(all(windows, target_pointer_width = "64"))]

use ds2::game::{self, Addresses, ItemText, POCKET_BONFIRE_ITEM};
use ds2::{log, main_thread};
use std::ffi::c_void;
use std::fs::OpenOptions;
use std::io::Write;
use std::path::PathBuf;
use std::sync::OnceLock;
use std::time::{Duration, Instant};

const DLL_PROCESS_ATTACH: u32 = 1;
const EASY_GO_ITEM: u32 = 60_595_010;
const MAIN_THREAD_WAIT: Duration = Duration::from_secs(3);

static RESULTS: OnceLock<PathBuf> = OnceLock::new();

#[no_mangle]
pub extern "system" fn DllMain(_module: *mut c_void, reason: u32, _reserved: *mut c_void) -> i32 {
    if reason == DLL_PROCESS_ATTACH {
        std::thread::spawn(run);
    }
    1
}

#[link(name = "kernel32")]
extern "system" {
    fn GetModuleHandleW(name: *const u16) -> *mut c_void;
    fn ExitProcess(code: u32) -> !;
}

#[link(name = "user32")]
extern "system" {
    fn SendInput(count: u32, inputs: *const Input, size: i32) -> u32;
    fn SetForegroundWindow(window: *mut c_void) -> i32;
    fn EnumWindows(callback: unsafe extern "system" fn(*mut c_void, isize) -> i32, param: isize) -> i32;
    fn GetWindowThreadProcessId(window: *mut c_void, process: *mut u32) -> u32;
    fn IsWindowVisible(window: *mut c_void) -> i32;
}

#[link(name = "kernel32")]
extern "system" {
    fn GetCurrentProcessId() -> u32;
}

/// `INPUT` for a keyboard event, padded to the size of the union.
#[repr(C)]
struct Input {
    kind: u32,
    _pad: u32,
    vk: u16,
    scan: u16,
    flags: u32,
    time: u32,
    extra: usize,
    _union_tail: [u8; 8],
}

const KEYEVENTF_KEYUP: u32 = 0x2;
const KEYEVENTF_SCANCODE: u32 = 0x8;
const SCAN_ENTER: u16 = 0x1c;
const SCAN_E: u16 = 0x12;
const SCAN_ESCAPE: u16 = 0x01;
const SCAN_Q: u16 = 0x10;

/// Presses and releases a key by scan code, which is what the game's DirectInput reads.
fn press(scan: u16) {
    focus_game();
    let key = |flags| Input { kind: 1, _pad: 0, vk: 0, scan, flags, time: 0, extra: 0, _union_tail: [0; 8] };
    unsafe {
        SendInput(1, &key(KEYEVENTF_SCANCODE), std::mem::size_of::<Input>() as i32);
        std::thread::sleep(Duration::from_millis(80));
        SendInput(1, &key(KEYEVENTF_SCANCODE | KEYEVENTF_KEYUP), std::mem::size_of::<Input>() as i32);
    }
    std::thread::sleep(Duration::from_millis(250));
}

fn focus_game() {
    unsafe extern "system" fn each(window: *mut c_void, _: isize) -> i32 {
        let mut pid = 0u32;
        GetWindowThreadProcessId(window, &mut pid);
        if pid == GetCurrentProcessId() && IsWindowVisible(window) != 0 {
            SetForegroundWindow(window);
            return 0;
        }
        1
    }
    unsafe { EnumWindows(each, 0) };
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

/// Uses an item the way the game reports it: by applying its effect id to the player.
fn use_item(a: Addresses, id: u32) -> bool {
    main_thread::call("use item", MAIN_THREAD_WAIT, move || unsafe { a.apply_sp_effect(id as i32) }) == Some(true)
}

fn distance(a: [f32; 3], b: [f32; 6]) -> f32 {
    (0..3).map(|i| (a[i] - b[i]).powi(2)).sum::<f32>().sqrt()
}

fn run() {
    let dll = DllMain as *const () as usize;
    log::init(dll, "ds2_regression.log");
    let dir = ds2::module::module_path_of(dll).and_then(|p| p.parent().map(|d| d.to_path_buf())).unwrap_or_default();
    let _ = RESULTS.set(dir.join("ds2_regression.results"));
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
    let pocket = module_loaded("pocket_bonfire.dll");
    let easy = module_loaded("easy_come_easy_go.dll");
    record("INFO", "mods", format!("pocket_bonfire.dll {pocket}, easy_come_easy_go.dll {easy}"));

    // The title: the game has loaded its params once the item table pointer exists.
    if !wait_for(Duration::from_secs(120), || {
        main_thread::find_main_thread();
        a.item_param().is_some() && main_thread::is_running()
    }) {
        return fail("title", "the game didn't reach the title within 2 minutes");
    }
    std::thread::sleep(Duration::from_secs(3));
    pass("title", format!("reached after {:.1} s", started.elapsed().as_secs_f32()));

    // Load the most recent save: confirm on the title menu, then on the save list. Enter and E are
    // both tried, since which one confirms depends on the key bindings.
    let loaded = wait_for(Duration::from_secs(240), || {
        if a.playable() {
            return true;
        }
        if a.snapshot().loading != Some(0) || a.player_ctrl().is_some() {
            return false;
        }
        press(SCAN_ENTER);
        std::thread::sleep(Duration::from_millis(1200));
        if !a.playable() && a.player_ctrl().is_none() {
            press(SCAN_E);
            std::thread::sleep(Duration::from_millis(1200));
        }
        false
    });
    if !loaded {
        fail("load save", "never got control of a character");
        unsafe { ExitProcess(1) };
    }
    std::thread::sleep(Duration::from_secs(4));
    let name = a.character_name().map(|n| String::from_utf16_lossy(&n)).unwrap_or_default();
    pass("load save", format!("playing as {name}"));

    check_text(a, pocket, easy);
    if pocket {
        pocket_bonfire(a);
    } else {
        skip("pocket bonfire", "not installed");
    }

    if easy {
        std::thread::sleep(Duration::from_secs(4));
        record("INFO", "easy go", "using it: the game should save, reach the title and close");
        if !use_item(a, EASY_GO_ITEM) {
            fail("easy go", "couldn't simulate the use");
        }
        // The runner checks the game closes. If it hasn't after a while, say so and close it.
        std::thread::sleep(Duration::from_secs(90));
        fail("easy go", "the game was still running 90 s after using Easy Go");
    } else {
        skip("easy go", "not installed");
        record("INFO", "done", "closing the game");
    }
    unsafe { ExitProcess(0) };
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

fn pocket_bonfire(a: Addresses) {
    let s = a.snapshot();
    let map = a.current_map();
    let Some(here) = s.pos else { return fail("place", "no position") };
    if s.last_bonfire.is_none_or(|b| b == 0) || s.last_bonfire_area != map {
        return skip("pocket bonfire", "the save's last bonfire isn't in this map, so nothing can be placed");
    }
    let spot = [here[0], here[1], here[2]];

    // Place, then use it twice in quick succession, which used to start overlapping game calls.
    if !use_item(a, POCKET_BONFIRE_ITEM) {
        return fail("place", "couldn't simulate the use");
    }
    std::thread::sleep(Duration::from_millis(300));
    use_item(a, POCKET_BONFIRE_ITEM);
    std::thread::sleep(Duration::from_millis(150));
    use_item(a, POCKET_BONFIRE_ITEM);
    std::thread::sleep(Duration::from_secs(4));
    if a.playable() && !a.rest_menu_open() {
        pass("double use", "the game is still fine after three quick uses");
    } else {
        fail("double use", format!("playable {}, rest menu open {}", a.playable(), a.rest_menu_open()));
        close_menus(a);
    }

    // Die, and check the respawn lands on the spot without costing health.
    if !a.set_hp(0) {
        return fail("respawn", "couldn't set HP");
    }
    if !wait_for(Duration::from_secs(30), || !a.playable()) {
        return fail("respawn", "the player didn't die");
    }
    if !wait_for(Duration::from_secs(240), || a.playable()) {
        return fail("respawn", "never got control back after dying");
    }
    std::thread::sleep(Duration::from_secs(5));
    match a.snapshot().pos {
        Some(pos) if distance(spot, pos) < 1.5 => pass("respawn", "back at the pocket bonfire"),
        Some(pos) => fail("respawn", format!("{:.1} m from the spot", distance(spot, pos))),
        None => fail("respawn", "no position after respawning"),
    }
    match a.hp() {
        Some((hp, max)) if hp >= max => pass("respawn damage", format!("full HP, {hp}/{max}")),
        Some((hp, max)) => fail("respawn damage", format!("the move cost health: {hp}/{max}")),
        None => fail("respawn damage", "couldn't read HP"),
    }

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
    close_menus(a);
}

/// Backs out of the bonfire menu. Escape and Q are both tried, for the same reason as Enter and E.
fn close_menus(a: Addresses) {
    if !a.rest_menu_open() {
        return;
    }
    let closed = wait_for(Duration::from_secs(20), || {
        if !a.rest_menu_open() && a.playable() {
            return true;
        }
        press(SCAN_ESCAPE);
        std::thread::sleep(Duration::from_millis(1500));
        if a.rest_menu_open() {
            press(SCAN_Q);
            std::thread::sleep(Duration::from_millis(1500));
        }
        false
    });
    if closed {
        pass("leave menu", "back in control");
    } else {
        fail("leave menu", "couldn't back out of the bonfire menu");
    }
}
