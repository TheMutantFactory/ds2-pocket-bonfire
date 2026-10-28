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
