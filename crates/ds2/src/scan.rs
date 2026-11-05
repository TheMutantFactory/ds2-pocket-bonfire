//! IDA-style byte patterns ("48 8B 05 ? ? ? ?") scanned over a module's readable pages.

use crate::module::Module;
use crate::{mem, win};
use std::ffi::c_void;

#[derive(Debug, Clone)]
pub struct Pattern(Vec<Option<u8>>);

impl Pattern {
    pub fn parse(s: &str) -> Pattern {
        Pattern(
            s.split_whitespace()
                .map(|t| if t.starts_with('?') { None } else { Some(u8::from_str_radix(t, 16).expect("bad pattern byte")) })
                .collect(),
        )
    }

    fn matches(&self, hay: &[u8]) -> bool {
        self.0.iter().zip(hay).all(|(p, b)| p.is_none_or(|p| p == *b))
    }

    /// Every match offset in `bytes`.
    pub fn find_in(&self, bytes: &[u8]) -> Vec<usize> {
        if bytes.len() < self.0.len() {
            return Vec::new();
        }
        (0..=bytes.len() - self.0.len()).filter(|&i| self.matches(&bytes[i..])).collect()
    }

    /// Every match address in `module`.
    pub fn find_all(&self, module: &Module) -> Vec<usize> {
        readable_regions(module)
            .into_iter()
            .flat_map(|(start, bytes)| self.find_in(&bytes).into_iter().map(move |i| start + i))
            .collect()
    }

    /// The single match in `module`; `Err` carries the hit count when it isn't exactly one.
    pub fn find_unique(&self, module: &Module) -> Result<usize, usize> {
        match self.find_all(module).as_slice() {
            [one] => Ok(*one),
            hits => Err(hits.len()),
        }
    }
}

/// Resolves a RIP-relative operand: `target = hit + instr_len + disp32 at (hit + disp_offset)`.
pub fn rip_relative(hit: usize, disp_offset: usize, instr_len: usize) -> Option<usize> {
    let disp = mem::read::<i32>(hit + disp_offset)?;
    Some((hit as isize + instr_len as isize + disp as isize) as usize)
}

/// Committed, readable regions inside the module, copied out.
fn readable_regions(module: &Module) -> Vec<(usize, Vec<u8>)> {
    let end = module.base + module.size;
    let mut out = Vec::new();
    let mut addr = module.base;
    while addr < end {
        let mut info = win::MemoryBasicInformation::default();
        let got = unsafe {
            win::VirtualQuery(addr as *const c_void, &mut info, std::mem::size_of::<win::MemoryBasicInformation>())
        };
        if got == 0 || info.region_size == 0 {
            break;
        }
        let region_end = (info.base_address + info.region_size).min(end);
        let readable = info.state == win::MEM_COMMIT
            && info.protect & (win::PAGE_NOACCESS | win::PAGE_GUARD) == 0
            && info.protect != 0;
        if readable {
            if let Some(bytes) = mem::read_bytes(addr, region_end - addr) {
                out.push((addr, bytes));
            }
        }
        addr = region_end;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::Pattern;

    #[test]
    fn wildcards_match_anything() {
        let p = Pattern::parse("48 8B ? ?? 05");
        assert!(p.matches(&[0x48, 0x8b, 0x00, 0xff, 0x05]));
        assert!(!p.matches(&[0x48, 0x8c, 0x00, 0xff, 0x05]));
    }
}
