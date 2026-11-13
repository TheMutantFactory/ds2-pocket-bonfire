//! Whether the game is running under Seamless Co-op, which needs the network: Steam's networking closes
//! the game when Windows sockets refuse to start, and the game's own network check has to run.
//!
//! `ds2sc_launcher.exe` starts the game and loads `ds2sc.dll` into it about a second later, after the
//! mods in `dllMods`, so at startup the launcher being the game's parent is the sign. Later on, the
//! DLL being loaded is.

use crate::win::{self, Handle};
use std::ffi::c_void;

const LAUNCHER: &str = "ds2sc_launcher.exe";
const DLL: &str = "ds2sc.dll";

/// Why the game counts as running under Seamless Co-op, or `None`. Safe to call from `DllMain`: it
/// only asks Windows about processes and modules, and loads nothing.
pub fn detect() -> Option<&'static str> {
    if dll_loaded() {
        return Some("ds2sc.dll is loaded");
    }
    if parent_name().is_some_and(|name| is_launcher(&name)) {
        return Some("started by ds2sc_launcher.exe");
    }
    None
}

/// Whether `ds2sc.dll` is in the game. Cheap enough for a hook.
pub fn dll_loaded() -> bool {
    let name: Vec<u16> = DLL.encode_utf16().chain(Some(0)).collect();
    !unsafe { win::GetModuleHandleW(name.as_ptr()) }.is_null()
}

fn is_launcher(path: &str) -> bool {
    let file = path.rsplit(['\\', '/']).next().unwrap_or(path);
    file.eq_ignore_ascii_case(LAUNCHER)
}

#[repr(C)]
#[derive(Default)]
struct ProcessBasicInformation {
    exit_status: isize,
    peb_base_address: usize,
    affinity_mask: usize,
    base_priority: isize,
    unique_process_id: usize,
    inherited_from_unique_process_id: usize,
}

#[link(name = "ntdll")]
extern "system" {
    fn NtQueryInformationProcess(process: Handle, class: u32, info: *mut c_void, len: u32, ret: *mut u32) -> i32;
}

#[link(name = "kernel32")]
extern "system" {
    fn OpenProcess(access: u32, inherit: i32, pid: u32) -> Handle;
    fn QueryFullProcessImageNameW(process: Handle, flags: u32, name: *mut u16, len: *mut u32) -> i32;
    fn CloseHandle(handle: Handle) -> i32;
}

const PROCESS_QUERY_LIMITED_INFORMATION: u32 = 0x1000;

/// The full path of the process that started this one, if it's still running.
fn parent_name() -> Option<String> {
    unsafe {
        let mut info = ProcessBasicInformation::default();
        let status = NtQueryInformationProcess(
            win::GetCurrentProcess(),
            0,
            &mut info as *mut _ as *mut c_void,
            std::mem::size_of::<ProcessBasicInformation>() as u32,
            std::ptr::null_mut(),
        );
        if status != 0 || info.inherited_from_unique_process_id == 0 {
            return None;
        }
        let parent = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, info.inherited_from_unique_process_id as u32);
        if parent.is_null() {
            return None;
        }
        let mut buf = [0u16; 1024];
        let mut len = buf.len() as u32;
        let ok = QueryFullProcessImageNameW(parent, 0, buf.as_mut_ptr(), &mut len);
        CloseHandle(parent);
        (ok != 0).then(|| String::from_utf16_lossy(&buf[..len as usize]))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn launcher_by_file_name_any_case() {
        assert!(is_launcher(r"C:\Games\Steam\steamapps\common\Dark Souls II Scholar of the First Sin\Game\ds2sc_launcher.exe"));
        assert!(is_launcher(r"c:\x\DS2SC_LAUNCHER.EXE"));
        assert!(is_launcher("ds2sc_launcher.exe"));
        assert!(!is_launcher(r"C:\Program Files (x86)\Steam\steam.exe"));
        assert!(!is_launcher(r"C:\x\not_ds2sc_launcher.exe"));
    }

    #[test]
    fn test_runner_is_not_seamless() {
        // cargo test runs under cargo, with no ds2sc.dll loaded.
        assert_eq!(detect(), None);
    }
}
