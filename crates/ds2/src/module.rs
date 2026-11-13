//! The game module (DarkSoulsII.exe) and our own DLL's location.

use crate::{mem, win};
use std::ffi::c_void;
use std::path::PathBuf;

#[derive(Debug, Clone, Copy)]
pub struct Module {
    pub base: usize,
    pub size: usize,
    pub timestamp: u32,
}

/// The process's main module, read from its in-memory PE header.
pub fn main_module() -> Option<Module> {
    let base = unsafe { win::GetModuleHandleW(std::ptr::null()) } as usize;
    let pe = base + mem::read::<u32>(base + 0x3c)? as usize;
    if mem::read::<u32>(pe)? != 0x0000_4550 {
        return None;
    }
    Some(Module {
        base,
        timestamp: mem::read::<u32>(pe + 8)?,
        size: mem::read::<u32>(pe + 24 + 56)? as usize,
    })
}

#[link(name = "kernel32")]
extern "system" {
    fn K32EnumProcessModules(process: win::Handle, modules: *mut win::Handle, size: u32, needed: *mut u32) -> i32;
    fn GetWindowsDirectoryW(buf: *mut u16, len: u32) -> u32;
}

#[link(name = "version")]
extern "system" {
    fn GetFileVersionInfoSizeW(file: *const u16, handle: *mut u32) -> u32;
    fn GetFileVersionInfoW(file: *const u16, handle: u32, len: u32, data: *mut c_void) -> i32;
    fn VerQueryValueW(block: *const c_void, sub: *const u16, buf: *mut *mut c_void, len: *mut u32) -> i32;
}

fn module_file(handle: win::Handle) -> Option<PathBuf> {
    let mut buf = vec![0u16; 32768];
    let n = unsafe { win::GetModuleFileNameW(handle, buf.as_mut_ptr(), buf.len() as u32) } as usize;
    (n > 0).then(|| PathBuf::from(String::from_utf16_lossy(&buf[..n])))
}

/// A file's version, from its version resource, as "1.2.3.4".
fn file_version(path: &std::path::Path) -> Option<String> {
    use std::os::windows::ffi::OsStrExt;
    let wide: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
    unsafe {
        let size = GetFileVersionInfoSizeW(wide.as_ptr(), std::ptr::null_mut());
        if size == 0 {
            return None;
        }
        let mut data = vec![0u8; size as usize];
        if GetFileVersionInfoW(wide.as_ptr(), 0, size, data.as_mut_ptr() as *mut c_void) == 0 {
            return None;
        }
        let root: Vec<u16> = "\\".encode_utf16().chain(Some(0)).collect();
        let mut info: *mut c_void = std::ptr::null_mut();
        let mut len = 0u32;
        if VerQueryValueW(data.as_ptr() as *const c_void, root.as_ptr(), &mut info, &mut len) == 0 || len < 52 {
            return None;
        }
        // VS_FIXEDFILEINFO: dwFileVersionMS at +8, dwFileVersionLS at +12.
        let ms = std::ptr::read_unaligned((info as *const u8).add(8) as *const u32);
        let ls = std::ptr::read_unaligned((info as *const u8).add(12) as *const u32);
        Some(format!("{}.{}.{}.{}", ms >> 16, ms & 0xffff, ls >> 16, ls & 0xffff))
    }
}

/// Every DLL in the game that isn't part of Windows, as "name version (folder)". These are the mods,
/// overlays and drivers that could change the game's code, for the log, so a bug report shows them.
pub fn other_dlls() -> Vec<String> {
    let mut handles = vec![std::ptr::null_mut(); 1024];
    let mut needed = 0u32;
    let ok = unsafe {
        K32EnumProcessModules(
            win::GetCurrentProcess(),
            handles.as_mut_ptr(),
            (handles.len() * std::mem::size_of::<win::Handle>()) as u32,
            &mut needed,
        )
    };
    if ok == 0 {
        return Vec::new();
    }
    handles.truncate((needed as usize / std::mem::size_of::<win::Handle>()).min(handles.len()));
    let mut windows = vec![0u16; 260];
    let n = unsafe { GetWindowsDirectoryW(windows.as_mut_ptr(), windows.len() as u32) } as usize;
    let windows = String::from_utf16_lossy(&windows[..n]).to_lowercase();
    let game_dir = module_file(std::ptr::null_mut()).and_then(|p| p.parent().map(|d| d.to_path_buf()));
    handles
        .into_iter()
        .skip(1) // the exe
        .filter_map(module_file)
        .filter(|p| windows.is_empty() || !p.to_string_lossy().to_lowercase().starts_with(&windows))
        .map(|p| {
            let name = p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
            let version = file_version(&p).map(|v| format!(" {v}")).unwrap_or_default();
            let folder = match (&game_dir, p.parent()) {
                (Some(game), Some(dir)) if dir.starts_with(game) => {
                    let rel = dir.strip_prefix(game).unwrap_or(dir).to_string_lossy().into_owned();
                    if rel.is_empty() { "Game".to_string() } else { format!("Game\\{rel}") }
                }
                (_, Some(dir)) => dir.to_string_lossy().into_owned(),
                _ => String::new(),
            };
            format!("{name}{version} ({folder})")
        })
        .collect()
}

/// Logs the other DLLs now, and once more half a minute later for the ones that loaded since, such
/// as Seamless Co-op's and DS2 Lighting Engine's, which arrive after the mods start.
pub fn log_other_dlls() {
    std::thread::spawn(|| {
        let first = other_dlls();
        crate::log::line(format!("other DLLs: {}", first.join(", ")));
        std::thread::sleep(std::time::Duration::from_secs(30));
        let later: Vec<String> = other_dlls().into_iter().filter(|d| !first.contains(d)).collect();
        if !later.is_empty() {
            crate::log::line(format!("DLLs loaded since: {}", later.join(", ")));
        }
    });
}

/// Full path of the module containing `addr_in_module`, i.e. our own DLL when given one of our
/// functions.
pub fn module_path_of(addr_in_module: usize) -> Option<PathBuf> {
    let mut handle: win::Handle = std::ptr::null_mut();
    let flags = win::GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS | win::GET_MODULE_HANDLE_EX_FLAG_UNCHANGED_REFCOUNT;
    if unsafe { win::GetModuleHandleExW(flags, addr_in_module as *const c_void, &mut handle) } == 0 {
        return None;
    }
    let mut buf = vec![0u16; 32768];
    let n = unsafe { win::GetModuleFileNameW(handle, buf.as_mut_ptr(), buf.len() as u32) } as usize;
    (n > 0).then(|| PathBuf::from(String::from_utf16_lossy(&buf[..n])))
}
