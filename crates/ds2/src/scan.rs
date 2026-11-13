//! IDA-style byte patterns ("48 8B 05 ? ? ? ?") scanned over a module's readable pages.

use crate::module::Module;
use crate::{mem, win};
use std::ffi::c_void;

/// How many leading bytes another mod's hook may overwrite when it isn't a jump we recognise. Function
/// signatures are long enough to stay unique without them.
pub const HOOK_BYTES: usize = 14;

/// Past a recognised hook, at least this many of the signature's bytes must still be checked. The hook
/// being there at all is most of the evidence.
const MIN_TAIL_BYTES: usize = 6;

/// How many leading bytes a hook jump replaced: the jump itself, plus the NOP or INT3 bytes a hooking
/// library pads it with up to the next whole instruction. A 14-byte `jmp [rip+0]` over a function whose
/// instructions end at byte 16 leaves two bytes of padding, which is what DS2 Inventory Suite does.
/// `None` if `bytes` doesn't start with a jump we know.
pub fn hook_len(bytes: &[u8]) -> Option<usize> {
    let jump = match bytes {
        [0xE9, ..] => 5,
        [0xFF, 0x25, 0, 0, 0, 0, ..] => 14,
        [0x48, 0xB8, rest @ ..] if rest.len() >= 10 && rest[8..10] == [0xFF, 0xE0] => 12,
        _ => return None,
    };
    if bytes.len() < jump {
        return None;
    }
    let padding = bytes[jump..].iter().take_while(|&&b| b == 0x90 || b == 0xCC).count();
    Some(jump + padding)
}

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

    /// The same pattern with its first `n` bytes made wildcards.
    pub fn with_wild_head(&self, n: usize) -> Pattern {
        Pattern(self.0.iter().enumerate().map(|(i, b)| if i < n { None } else { *b }).collect())
    }

    /// Bytes the pattern actually checks.
    pub fn known_bytes(&self) -> usize {
        self.0.iter().filter(|b| b.is_some()).count()
    }

    /// How many of the pattern's checked bytes `hay` has right, and how many it checks.
    pub fn known_matches(&self, hay: &[u8]) -> (usize, usize) {
        let right = self.0.iter().zip(hay).filter(|(p, b)| p.is_some_and(|p| p == **b)).count();
        (right, self.known_bytes())
    }

    /// Bytes the pattern spans, wild ones included.
    pub fn len(&self) -> usize {
        self.0.len()
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    fn matches(&self, hay: &[u8]) -> bool {
        self.matches_from(0, hay)
    }

    /// Whether the pattern matches `hay`, ignoring its first `skip` bytes.
    fn matches_from(&self, skip: usize, hay: &[u8]) -> bool {
        hay.len() >= self.0.len() && self.0[skip..].iter().zip(&hay[skip..]).all(|(p, b)| p.is_none_or(|p| p == *b))
    }

    /// Every offset in `bytes` where a hook jump sits and the pattern matches past it.
    pub fn find_hooked_in(&self, bytes: &[u8]) -> Vec<usize> {
        if bytes.len() < self.0.len() {
            return Vec::new();
        }
        (0..=bytes.len() - self.0.len())
            .filter(|&i| {
                let Some(skip) = hook_len(&bytes[i..]) else { return false };
                skip < self.0.len()
                    && self.0[skip..].iter().filter(|b| b.is_some()).count() >= MIN_TAIL_BYTES
                    && self.matches_from(skip, &bytes[i..])
            })
            .collect()
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

/// Finds a function by a signature that starts at its first byte. If that fails, it looks for the
/// function behind another mod's hook: first a jump we recognise followed by the rest of the
/// signature, then the signature without the bytes an unknown hook could have overwritten. Calling or
/// hooking it then goes through their hook, as it should.
pub fn find_function(module: &Module, aob: &str) -> Result<usize, usize> {
    let regions = readable_regions(module);
    let borrowed: Vec<(usize, &[u8])> = regions.iter().map(|(start, bytes)| (*start, bytes.as_slice())).collect();
    find_function_in_regions(&borrowed, aob)
}

/// `find_function` over a plain byte slice, such as an exe read from disk; offsets are into `bytes`.
pub fn find_function_in(bytes: &[u8], aob: &str) -> Result<usize, usize> {
    find_function_in_regions(&[(0, bytes)], aob)
}

fn find_function_in_regions(regions: &[(usize, &[u8])], aob: &str) -> Result<usize, usize> {
    let pattern = Pattern::parse(aob);
    let unique = |hits: Vec<usize>| match hits.as_slice() {
        [one] => Ok(*one),
        hits => Err(hits.len()),
    };
    let all = |find: &dyn Fn(&[u8]) -> Vec<usize>| -> Vec<usize> {
        regions.iter().flat_map(|(start, bytes)| find(bytes).into_iter().map(move |i| start + i)).collect()
    };
    let strict = unique(all(&|b| pattern.find_in(b)));
    strict.or_else(|strict_hits| {
        if let Ok(hit) = unique(all(&|b| pattern.find_hooked_in(b))) {
            return Ok(hit);
        }
        let tolerant = pattern.with_wild_head(HOOK_BYTES);
        if tolerant.known_bytes() < 8 {
            return Err(strict_hits);
        }
        unique(all(&|b| tolerant.find_in(b))).map_err(|_| strict_hits)
    })
}

/// Resolves a RIP-relative operand: `target = hit + instr_len + disp32 at (hit + disp_offset)`.
pub fn rip_relative(hit: usize, disp_offset: usize, instr_len: usize) -> Option<usize> {
    let disp = mem::read::<i32>(hit + disp_offset)?;
    Some((hit as isize + instr_len as isize + disp as isize) as usize)
}

/// Where a jump written over a function's start leads, for the common hook shapes: `jmp rel32`,
/// `jmp [rip+0]` followed by the address, and `mov rax, imm64; jmp rax`.
pub fn hook_target(addr: usize) -> Option<usize> {
    let bytes = mem::read_bytes(addr, 12)?;
    match bytes.as_slice() {
        [0xE9, ..] => rip_relative(addr, 1, 5),
        [0xFF, 0x25, 0, 0, 0, 0, ..] => mem::read::<usize>(addr + 6),
        [0x48, 0xB8, rest @ ..] if rest[8..10] == [0xFF, 0xE0] => Some(usize::from_le_bytes(rest[..8].try_into().ok()?)),
        _ => None,
    }
}

/// The DLL a hook jump leads into. Hooking libraries such as MinHook jump first to a small relay in
/// memory of their own, which jumps on into the DLL, so up to three jumps are followed.
pub fn hook_owner(target: usize) -> Option<String> {
    let mut at = target;
    for _ in 0..3 {
        if let Some(dll) = crate::module::module_path_of(at).and_then(|p| p.file_name().map(|n| n.to_string_lossy().into_owned())) {
            return Some(dll);
        }
        at = hook_target(at)?;
    }
    None
}

/// For the log when a signature fails: the bytes where the 1.03 exe has the function, and if they
/// are a hook, the DLL it leads into. That names the mod to ask about.
pub fn describe(module: &Module, rva: usize) -> String {
    let addr = module.base + rva;
    let Some(bytes) = mem::read_bytes(addr, 16) else { return format!("+{rva:#x} unreadable") };
    let hex: Vec<String> = bytes.iter().map(|b| format!("{b:02X}")).collect();
    let owner = hook_target(addr).map(|target| {
        let dll = hook_owner(target).unwrap_or_else(|| "memory outside any DLL".into());
        format!(", hooked: jumps to {target:#x} in {dll}")
    });
    format!("+{rva:#x} is {}{}", hex.join(" "), owner.unwrap_or_default())
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
    use super::*;

    const ITEM_GIVE: [u8; 23] = [
        0x48, 0x89, 0x5C, 0x24, 0x18, 0x56, 0x57, 0x41, 0x56, 0x48, 0x83, 0xEC, 0x30, 0x45, 0x8B, 0xF1, 0x41, 0x8B, 0xF0, 0x48,
        0x8B, 0xDA, 0x48,
    ];
    const ITEM_GIVE_AOB: &str = "48 89 5C 24 18 56 57 41 56 48 83 EC 30 45 8B F1 41 8B F0 48 8B DA 48";

    #[test]
    fn a_hooked_start_still_matches_once_the_head_is_wild() {
        let pattern = Pattern::parse(ITEM_GIVE_AOB);
        assert_eq!(pattern.find_in(&ITEM_GIVE), [0]);

        let mut relative = ITEM_GIVE;
        relative[..5].copy_from_slice(&[0xE9, 1, 2, 3, 4]);
        assert!(pattern.find_in(&relative).is_empty());
        assert_eq!(pattern.with_wild_head(HOOK_BYTES).find_in(&relative), [0]);

        let mut absolute = ITEM_GIVE;
        absolute[..14].copy_from_slice(&[0xFF, 0x25, 0, 0, 0, 0, 1, 2, 3, 4, 5, 6, 7, 8]);
        assert_eq!(pattern.with_wild_head(HOOK_BYTES).find_in(&absolute), [0]);
    }

    /// DS2 Inventory Suite's hook on ItemGive, as it was in the game: a 14-byte jump padded with two
    /// NOPs to the end of the instruction it cut into. The 14-byte wild head alone missed it.
    #[test]
    fn a_padded_absolute_jump_is_skipped_whole() {
        let mut hooked = ITEM_GIVE;
        hooked[..16].copy_from_slice(&[0xFF, 0x25, 0, 0, 0, 0, 0x89, 0x44, 0xDC, 0x51, 0xFB, 0x7F, 0, 0, 0x90, 0x90]);
        assert_eq!(hook_len(&hooked), Some(16));
        let pattern = Pattern::parse(ITEM_GIVE_AOB);
        assert!(pattern.with_wild_head(HOOK_BYTES).find_in(&hooked).is_empty());
        assert_eq!(pattern.find_hooked_in(&hooked), [0]);
        assert_eq!(find_function_in(&hooked, ITEM_GIVE_AOB), Ok(0));
    }

    #[test]
    fn the_rest_must_still_match_behind_a_hook() {
        let mut other = ITEM_GIVE;
        other[..5].copy_from_slice(&[0xE9, 1, 2, 3, 4]);
        other[20] ^= 0xFF;
        assert!(Pattern::parse(ITEM_GIVE_AOB).find_hooked_in(&other).is_empty());
        assert!(hook_len(&ITEM_GIVE).is_none());
    }
}
