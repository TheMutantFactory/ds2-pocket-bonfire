//! The handful of Win32 calls we need, declared directly.

use std::ffi::c_void;

pub type Handle = *mut c_void;

#[repr(C)]
#[derive(Default, Clone, Copy)]
pub struct MemoryBasicInformation {
    pub base_address: usize,
    pub allocation_base: usize,
    pub allocation_protect: u32,
    pub partition_id: u16,
    pub region_size: usize,
    pub state: u32,
    pub protect: u32,
    pub kind: u32,
}

pub const MEM_COMMIT: u32 = 0x1000;
pub const PAGE_NOACCESS: u32 = 0x01;
pub const PAGE_GUARD: u32 = 0x100;

pub const GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS: u32 = 0x4;
pub const GET_MODULE_HANDLE_EX_FLAG_UNCHANGED_REFCOUNT: u32 = 0x2;

#[link(name = "kernel32")]
extern "system" {
    pub fn GetModuleHandleW(name: *const u16) -> Handle;
    pub fn GetModuleHandleExW(flags: u32, name: *const c_void, module: *mut Handle) -> i32;
    pub fn GetModuleFileNameW(module: Handle, buf: *mut u16, len: u32) -> u32;
    pub fn GetCurrentProcess() -> Handle;
    pub fn ReadProcessMemory(
        process: Handle,
        base: *const c_void,
        buf: *mut c_void,
        size: usize,
        read: *mut usize,
    ) -> i32;
    pub fn VirtualQuery(addr: *const c_void, info: *mut MemoryBasicInformation, len: usize) -> usize;
}
