//! Logs every `SuspendThread` and `ResumeThread` call in the game to `ds2_suspend_trace.log`, with the
//! caller's thread and return addresses, so a startup freeze with threads left suspended shows who
//! suspended them. A frozen game can hold the heap lock, so nothing here allocates: each line is
//! formatted on the stack and written straight to the file handle.

use minhook::MinHook;
use std::ffi::c_void;
use std::sync::atomic::{AtomicPtr, AtomicUsize, Ordering};

#[link(name = "kernel32")]
extern "system" {
    fn CreateFileW(name: *const u16, access: u32, share: u32, sa: *mut c_void, disposition: u32, flags: u32, template: *mut c_void) -> *mut c_void;
    fn WriteFile(file: *mut c_void, buf: *const u8, len: u32, written: *mut u32, overlapped: *mut c_void) -> i32;
    fn GetThreadId(thread: *mut c_void) -> u32;
    fn GetCurrentThreadId() -> u32;
    fn GetTickCount64() -> u64;
}

#[link(name = "ntdll")]
extern "system" {
    fn RtlCaptureStackBackTrace(skip: u32, count: u32, frames: *mut *mut c_void, hash: *mut u32) -> u16;
}

type ThreadFn = unsafe extern "system" fn(*mut c_void) -> u32;

static FILE: AtomicPtr<c_void> = AtomicPtr::new(std::ptr::null_mut());
static ORIG_SUSPEND: AtomicUsize = AtomicUsize::new(0);
static ORIG_RESUME: AtomicUsize = AtomicUsize::new(0);

/// Opens the log next to the DLL and hooks both functions. Call from DllMain, before the hooks are
/// enabled.
pub unsafe fn install(dir: &std::path::Path) -> Result<(), String> {
    let path: Vec<u16> = dir.join("ds2_suspend_trace.log").as_os_str().encode_wide_nul();
    // GENERIC_WRITE, share read, CREATE_ALWAYS, FILE_FLAG_WRITE_THROUGH.
    let file = CreateFileW(path.as_ptr(), 0x4000_0000, 1, std::ptr::null_mut(), 2, 0x8000_0000, std::ptr::null_mut());
    if file as isize == -1 {
        return Err("can't open ds2_suspend_trace.log".into());
    }
    FILE.store(file, Ordering::SeqCst);
    let s = MinHook::create_hook_api("kernelbase", "SuspendThread", suspend_detour as *mut c_void).map_err(|e| format!("{e:?}"))?;
    ORIG_SUSPEND.store(s as usize, Ordering::SeqCst);
    let r = MinHook::create_hook_api("kernelbase", "ResumeThread", resume_detour as *mut c_void).map_err(|e| format!("{e:?}"))?;
    ORIG_RESUME.store(r as usize, Ordering::SeqCst);
    Ok(())
}

trait WideNul {
    fn encode_wide_nul(&self) -> Vec<u16>;
}

impl WideNul for std::ffi::OsStr {
    fn encode_wide_nul(&self) -> Vec<u16> {
        use std::os::windows::ffi::OsStrExt;
        self.encode_wide().chain(Some(0)).collect()
    }
}

unsafe extern "system" fn suspend_detour(thread: *mut c_void) -> u32 {
    let orig: ThreadFn = std::mem::transmute(ORIG_SUSPEND.load(Ordering::SeqCst));
    let prev = orig(thread);
    trace(b'S', thread, prev);
    prev
}

unsafe extern "system" fn resume_detour(thread: *mut c_void) -> u32 {
    let orig: ThreadFn = std::mem::transmute(ORIG_RESUME.load(Ordering::SeqCst));
    let prev = orig(thread);
    trace(b'R', thread, prev);
    prev
}

/// One line: `<ms> <S|R> target=<tid> prev=<count> by=<tid> at <return addresses>`.
unsafe fn trace(kind: u8, thread: *mut c_void, prev: u32) {
    let file = FILE.load(Ordering::Relaxed);
    if file.is_null() {
        return;
    }
    let mut frames = [std::ptr::null_mut::<c_void>(); 5];
    let n = RtlCaptureStackBackTrace(2, frames.len() as u32, frames.as_mut_ptr(), std::ptr::null_mut()) as usize;
    let mut line = Line { buf: [0; 256], len: 0 };
    line.num(GetTickCount64(), 10);
    line.push(b" ");
    line.push(&[kind]);
    line.push(b" target=");
    line.num(GetThreadId(thread) as u64, 10);
    line.push(b" prev=");
    line.num(prev as u64, 10);
    line.push(b" by=");
    line.num(GetCurrentThreadId() as u64, 10);
    line.push(b" at");
    for f in &frames[..n] {
        line.push(b" 0x");
        line.num(*f as u64, 16);
    }
    line.push(b"\r\n");
    let mut written = 0;
    WriteFile(file, line.buf.as_ptr(), line.len as u32, &mut written, std::ptr::null_mut());
}

struct Line {
    buf: [u8; 256],
    len: usize,
}

impl Line {
    fn push(&mut self, bytes: &[u8]) {
        let n = bytes.len().min(self.buf.len() - self.len);
        self.buf[self.len..self.len + n].copy_from_slice(&bytes[..n]);
        self.len += n;
    }

    fn num(&mut self, mut v: u64, base: u64) {
        let mut digits = [0u8; 20];
        let mut i = digits.len();
        loop {
            i -= 1;
            digits[i] = b"0123456789abcdef"[(v % base) as usize];
            v /= base;
            if v == 0 {
                break;
            }
        }
        self.push(&digits[i..]);
    }
}
