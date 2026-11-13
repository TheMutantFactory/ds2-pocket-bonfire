//! Lets mods that hook the same game functions take turns.
//!
//! Each mod has its own copy of MinHook, which reads a function's first bytes when creating a hook and
//! overwrites them when enabling it. If a second mod reads between those two steps, its later write cuts
//! the first mod out of the chain, and which mod loses changes from launch to launch. Holding this lock
//! from creating the hooks to enabling them keeps them chained. Any mod can join by using the same name.

use std::ffi::c_void;

const LOCK_NAME: &str = "Local\\DS2-TheMutantFactory-Hooks";
/// Longer than installing hooks takes, and short enough not to hold up the game if something's wrong.
const WAIT_MS: u32 = 10_000;

#[link(name = "kernel32")]
extern "system" {
    fn CreateMutexW(attributes: *const c_void, initial_owner: i32, name: *const u16) -> *mut c_void;
    fn WaitForSingleObject(handle: *mut c_void, milliseconds: u32) -> u32;
    fn ReleaseMutex(handle: *mut c_void) -> i32;
    fn CloseHandle(handle: *mut c_void) -> i32;
}

const WAIT_OBJECT_0: u32 = 0;
const WAIT_ABANDONED: u32 = 0x80;

/// Runs `install` while holding the lock. If the lock can't be had, `install` runs anyway: a possible
/// race is better than a mod that never starts.
pub fn serialized<T>(install: impl FnOnce() -> T) -> T {
    let name: Vec<u16> = LOCK_NAME.encode_utf16().chain(Some(0)).collect();
    unsafe {
        let lock = CreateMutexW(std::ptr::null(), 0, name.as_ptr());
        if lock.is_null() {
            return install();
        }
        let held = matches!(WaitForSingleObject(lock, WAIT_MS), WAIT_OBJECT_0 | WAIT_ABANDONED);
        let result = install();
        if held {
            ReleaseMutex(lock);
        }
        CloseHandle(lock);
        result
    }
}

/// How many bytes of each hooked function's start are remembered: enough for any hook jump.
const WATCH_BYTES: usize = 16;

/// Our own mods, which hook some of the same functions, take turns through the lock above and keep
/// each other in the chain. One hooking after the other isn't worth a line in the log.
const PARTNERS: [&str; 2] = ["pocket_bonfire.dll", "easy_come_easy_go.dll"];

struct Watched {
    name: &'static str,
    addr: usize,
    bytes: Vec<u8>,
}

static WATCHED: std::sync::Mutex<Vec<Watched>> = std::sync::Mutex::new(Vec::new());

/// Remembers how a function we hooked starts now, with our hook in place, so `changed` can tell when
/// a mod that loaded later wrote over it.
pub fn watch(name: &'static str, addr: usize) {
    let Some(bytes) = crate::mem::read_bytes(addr, WATCH_BYTES) else { return };
    if let Ok(mut list) = WATCHED.lock() {
        list.retain(|w| w.addr != addr);
        list.push(Watched { name, addr, bytes });
    }
}

/// One line for each watched function whose start changed since it was last seen, naming the DLL its
/// jump now leads into. A later mod hooking on top of ours usually keeps ours in its chain, so this
/// says so rather than calling it broken; if a feature then stops, the line names the likely cause.
pub fn changed() -> Vec<String> {
    let Ok(mut list) = WATCHED.lock() else { return Vec::new() };
    let mut out = Vec::new();
    for w in list.iter_mut() {
        let Some(now) = crate::mem::read_bytes(w.addr, WATCH_BYTES) else { continue };
        if now == w.bytes {
            continue;
        }
        w.bytes = now;
        let owner = crate::scan::hook_target(w.addr).map(crate::scan::hook_owner);
        let into = match owner {
            None => "it no longer starts with a jump".to_string(),
            Some(Some(dll)) if PARTNERS.iter().any(|p| dll.eq_ignore_ascii_case(p)) => continue,
            Some(Some(dll)) => format!("it now jumps into {dll}"),
            Some(None) => "it now jumps into memory outside any DLL".to_string(),
        };
        out.push(format!(
            "{}: another mod changed it after our hook went in; {into}. If what uses it stops working, that mod may have replaced our hook",
            w.name
        ));
    }
    out
}

static LAST_CHECK: std::sync::Mutex<Option<std::time::Instant>> = std::sync::Mutex::new(None);

/// Logs `changed`'s lines, at most every five seconds however often it's called. For a mod's tick.
pub fn log_changes() {
    let Ok(mut last) = LAST_CHECK.lock() else { return };
    if last.is_some_and(|t| t.elapsed() < std::time::Duration::from_secs(5)) {
        return;
    }
    *last = Some(std::time::Instant::now());
    drop(last);
    for line in changed() {
        crate::log::line(line);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU32, Ordering};
    use std::time::Duration;

    #[test]
    fn a_changed_start_is_reported_once() {
        let code = Box::leak(Box::new([0x90u8; 32]));
        let addr = code.as_ptr() as usize;
        watch("Test", addr);
        assert!(changed().is_empty());
        unsafe { std::ptr::write_volatile(code.as_mut_ptr(), 0xE9) };
        let lines = changed();
        assert_eq!(lines.len(), 1);
        assert!(lines[0].starts_with("Test: another mod changed it"));
        assert!(changed().is_empty());
    }

    #[test]
    fn only_one_runs_at_a_time_and_results_come_back() {
        static RUNNING: AtomicU32 = AtomicU32::new(0);
        let threads: Vec<_> = (0..4)
            .map(|i| {
                std::thread::spawn(move || {
                    serialized(|| {
                        assert_eq!(RUNNING.fetch_add(1, Ordering::SeqCst), 0);
                        std::thread::sleep(Duration::from_millis(20));
                        RUNNING.fetch_sub(1, Ordering::SeqCst);
                        i
                    })
                })
            })
            .collect();
        let results: Vec<i32> = threads.into_iter().map(|t| t.join().unwrap()).collect();
        assert_eq!(results, [0, 1, 2, 3]);
    }
}
