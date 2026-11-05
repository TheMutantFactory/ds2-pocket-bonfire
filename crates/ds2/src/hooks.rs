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

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU32, Ordering};
    use std::time::Duration;

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
