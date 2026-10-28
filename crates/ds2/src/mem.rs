//! Guarded reads. Game pointers are often null or dangling during loads, so every read goes
//! through ReadProcessMemory on our own process, which fails instead of faulting.

use crate::win;
use std::ffi::c_void;
use std::mem::{size_of, MaybeUninit};

/// Reads a `T` at `addr`, or `None` if the memory isn't readable.
pub fn read<T: Copy>(addr: usize) -> Option<T> {
    if addr < 0x10000 {
        return None;
    }
    let mut out = MaybeUninit::<T>::uninit();
    let mut n = 0usize;
    let ok = unsafe {
        win::ReadProcessMemory(
            win::GetCurrentProcess(),
            addr as *const c_void,
            out.as_mut_ptr().cast(),
            size_of::<T>(),
            &mut n,
        )
    };
    (ok != 0 && n == size_of::<T>()).then(|| unsafe { out.assume_init() })
}

/// Reads `len` bytes at `addr`.
pub fn read_bytes(addr: usize, len: usize) -> Option<Vec<u8>> {
    if addr < 0x10000 {
        return None;
    }
    let mut buf = vec![0u8; len];
    let mut n = 0usize;
    let ok = unsafe {
        win::ReadProcessMemory(win::GetCurrentProcess(), addr as *const c_void, buf.as_mut_ptr().cast(), len, &mut n)
    };
    (ok != 0 && n == len).then_some(buf)
}

/// Reads a non-null pointer at `addr`.
pub fn ptr(addr: usize) -> Option<usize> {
    read::<usize>(addr).filter(|&p| p >= 0x10000)
}

/// Follows a chain the DS2S-META way: for each offset, `addr = *(addr + offset)`.
pub fn chain(base: usize, offsets: &[usize]) -> Option<usize> {
    offsets.iter().try_fold(base, |addr, &off| ptr(addr + off))
}
