//! Runs game calls on the game's main thread.
//!
//! The game's menus and item code aren't built to be called from another thread: doing it from the
//! mod's own thread could follow a pointer the game was changing at that moment, and crash. The game
//! polls the controller from its main thread every frame through `XInputGetState`, so the mod points
//! the exe's import of that function at its own, which runs queued calls and then polls as normal.
//! Each mod keeps the pointer it replaced, so several mods chain.

use crate::module::Module;
use crate::{log, mem};
use std::ffi::c_void;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicUsize, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::time::Duration;

/// `XInputGetState`, which the exe imports from XINPUT1_3.dll by ordinal.
const XINPUT_DLL: &str = "xinput1_3.dll";
const XINPUT_GET_STATE_ORDINAL: u16 = 2;

static NEXT: AtomicUsize = AtomicUsize::new(0);
static MAIN_THREAD: AtomicU32 = AtomicU32::new(0);
static FIRED: AtomicBool = AtomicBool::new(false);
static QUEUE: Mutex<Vec<Arc<Job>>> = Mutex::new(Vec::new());

type Work = Box<dyn FnOnce() + Send>;

struct Job {
    work: Mutex<Option<Work>>,
    done: Mutex<bool>,
    signal: Condvar,
}

impl Job {
    /// Runs the work unless someone already took it, and wakes whoever waits.
    fn run(&self) {
        let work = self.work.lock().ok().and_then(|mut w| w.take());
        if let Some(work) = work {
            work();
        }
        if let Ok(mut done) = self.done.lock() {
            *done = true;
        }
        self.signal.notify_all();
    }
}

#[link(name = "kernel32")]
extern "system" {
    fn GetCurrentThreadId() -> u32;
    fn GetCurrentProcessId() -> u32;
}

#[link(name = "user32")]
extern "system" {
    fn EnumWindows(callback: unsafe extern "system" fn(*mut c_void, isize) -> i32, param: isize) -> i32;
    fn GetWindowThreadProcessId(window: *mut c_void, process: *mut u32) -> u32;
    fn IsWindowVisible(window: *mut c_void) -> i32;
}

type XInputGetState = unsafe extern "system" fn(u32, *mut c_void) -> u32;

unsafe extern "system" fn xinput_get_state(index: u32, state: *mut c_void) -> u32 {
    let main = MAIN_THREAD.load(Ordering::Relaxed);
    if main != 0 && GetCurrentThreadId() == main {
        FIRED.store(true, Ordering::Relaxed);
        let jobs: Vec<Arc<Job>> = QUEUE.lock().map(|mut q| q.drain(..).collect()).unwrap_or_default();
        for job in jobs {
            job.run();
        }
    }
    let next: XInputGetState = std::mem::transmute(NEXT.load(Ordering::SeqCst));
    next(index, state)
}

/// Points the exe's `XInputGetState` import at the mod. Call it inside `hooks::serialized`, so two
/// mods don't both replace the same pointer at once.
pub fn install(module: &Module) -> Result<(), String> {
    let slot = import_slot(module, XINPUT_DLL, XINPUT_GET_STATE_ORDINAL).ok_or("no XInputGetState import")?;
    let current = mem::read::<usize>(slot).ok_or("can't read the import")?;
    NEXT.store(current, Ordering::SeqCst);
    if !mem::write(slot, xinput_get_state as *const () as usize) {
        return Err("can't write the import".into());
    }
    Ok(())
}

/// Finds the game window's thread, which is the main thread. Call from the mod's own thread until it
/// returns true; the window appears a moment after the DLL loads.
pub fn find_main_thread() -> bool {
    unsafe extern "system" fn each(window: *mut c_void, found: isize) -> i32 {
        let mut pid = 0u32;
        let thread = GetWindowThreadProcessId(window, &mut pid);
        if pid == GetCurrentProcessId() && IsWindowVisible(window) != 0 {
            *(found as *mut u32) = thread;
            return 0;
        }
        1
    }
    if MAIN_THREAD.load(Ordering::Relaxed) != 0 {
        return true;
    }
    let mut found = 0u32;
    unsafe { EnumWindows(each, &mut found as *mut u32 as isize) };
    if found != 0 {
        MAIN_THREAD.store(found, Ordering::Relaxed);
    }
    found != 0
}

/// Whether queued calls have run on the main thread at least once.
pub fn is_running() -> bool {
    FIRED.load(Ordering::Relaxed)
}

/// Runs `work` on the main thread and returns its result, waiting up to `wait`. If the main thread
/// never picks it up, for instance because the game stopped polling, it's taken back and not run:
/// running it here instead is what this exists to avoid.
pub fn call<T: Send + 'static>(what: &str, wait: Duration, work: impl FnOnce() -> T + Send + 'static) -> Option<T> {
    let result: Arc<Mutex<Option<T>>> = Arc::new(Mutex::new(None));
    let out = result.clone();
    let job = Arc::new(Job {
        work: Mutex::new(Some(Box::new(move || {
            let value = work();
            if let Ok(mut slot) = out.lock() {
                *slot = Some(value);
            }
        }))),
        done: Mutex::new(false),
        signal: Condvar::new(),
    });
    QUEUE.lock().ok()?.push(job.clone());
    let done = job.done.lock().ok()?;
    let (done, _) = job.signal.wait_timeout_while(done, wait, |done| !*done).ok()?;
    if !*done {
        drop(done);
        let unrun = job.work.lock().ok().and_then(|mut w| w.take()).is_some();
        if unrun {
            log::line(format!("{what}: the game's main thread didn't pick it up, so it wasn't run"));
            return None;
        }
    }
    let value = result.lock().ok()?.take();
    value
}

/// The address of the import table slot for `ordinal` from `dll`, read from the exe's headers in memory.
fn import_slot(module: &Module, dll: &str, ordinal: u16) -> Option<usize> {
    let base = module.base;
    let pe = base + mem::read::<u32>(base + 0x3c)? as usize;
    let imports = base + mem::read::<u32>(pe + 0x18 + 0x70 + 8)? as usize;
    for i in 0.. {
        let desc = imports + i * 20;
        let (original, name, first) =
            (mem::read::<u32>(desc)?, mem::read::<u32>(desc + 12)?, mem::read::<u32>(desc + 16)?);
        if name == 0 {
            return None;
        }
        let bytes = mem::read_bytes(base + name as usize, dll.len())?;
        if !bytes.eq_ignore_ascii_case(dll.as_bytes()) {
            continue;
        }
        let lookup = if original != 0 { original } else { first };
        for j in 0.. {
            let entry = mem::read::<u64>(base + lookup as usize + j * 8)?;
            if entry == 0 {
                break;
            }
            if entry >> 63 == 1 && entry as u16 == ordinal {
                return Some(base + first as usize + j * 8);
            }
        }
    }
    None
}
