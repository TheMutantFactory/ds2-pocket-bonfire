//! Reader and writer for DS2 text files (`menu\text\<language>\*.fmg`).
//!
//! Layout, as read from the files themselves (all little-endian u32):
//! - 0x00 format (0x10000), 0x04 file size, 0x08 1, 0x0c group count, 0x10 string count,
//!   0x14 offset of the string offset table, 0x18 0
//! - 0x1c groups: { first string index, first id, last id }, one per run of consecutive ids
//! - string offset table: one u32 per string, 0 for "no text"
//! - UTF-16LE strings, NUL-terminated

use std::collections::BTreeMap;
use std::fmt;

const HEADER: usize = 0x1c;
const GROUP: usize = 12;

#[derive(Debug, PartialEq, Eq)]
pub enum FmgError {
    TooShort,
    Malformed(&'static str),
}

impl fmt::Display for FmgError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            FmgError::TooShort => write!(f, "file too short to be an fmg"),
            FmgError::Malformed(why) => write!(f, "malformed fmg: {why}"),
        }
    }
}

impl std::error::Error for FmgError {}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Fmg {
    format: [u8; 4],
    /// Text by id, as raw UTF-16 code units without the terminator. `None` means no text.
    entries: BTreeMap<u32, Option<Vec<u16>>>,
}

fn u32_at(b: &[u8], at: usize) -> Result<u32, FmgError> {
    let bytes = b.get(at..at + 4).ok_or(FmgError::Malformed("offset past the end of the file"))?;
    Ok(u32::from_le_bytes(bytes.try_into().unwrap()))
}

impl Fmg {
    pub fn parse(bytes: &[u8]) -> Result<Fmg, FmgError> {
        if bytes.len() < HEADER {
            return Err(FmgError::TooShort);
        }
        if u32_at(bytes, 4)? as usize != bytes.len() {
            return Err(FmgError::Malformed("size field doesn't match the file"));
        }
        let groups = u32_at(bytes, 0x0c)? as usize;
        let strings = u32_at(bytes, 0x10)? as usize;
        let table = u32_at(bytes, 0x14)? as usize;
        if table != HEADER + groups * GROUP {
            return Err(FmgError::Malformed("string table doesn't follow the groups"));
        }

        let mut entries = BTreeMap::new();
        for g in 0..groups {
            let at = HEADER + g * GROUP;
            let (index, first, last) = (u32_at(bytes, at)? as usize, u32_at(bytes, at + 4)?, u32_at(bytes, at + 8)?);
            if last < first {
                return Err(FmgError::Malformed("group ends before it starts"));
            }
            for id in first..=last {
                let i = index + (id - first) as usize;
                if i >= strings {
                    return Err(FmgError::Malformed("group points past the string table"));
                }
                let offset = u32_at(bytes, table + i * 4)? as usize;
                let text = if offset == 0 { None } else { Some(read_utf16(bytes, offset)?) };
                entries.insert(id, text);
            }
        }
        let mut format = [0u8; 4];
        format.copy_from_slice(&bytes[..4]);
        Ok(Fmg { format, entries })
    }

    pub fn to_bytes(&self) -> Vec<u8> {
        // Runs of consecutive ids become groups, in id order.
        let mut groups: Vec<(u32, u32, u32)> = Vec::new();
        for (i, &id) in self.entries.keys().enumerate() {
            match groups.last_mut() {
                Some((_, _, last)) if *last + 1 == id => *last = id,
                _ => groups.push((i as u32, id, id)),
            }
        }

        let table = HEADER + groups.len() * GROUP;
        let mut offsets = Vec::with_capacity(self.entries.len());
        let mut text = Vec::new();
        let text_start = table + self.entries.len() * 4;
        for entry in self.entries.values() {
            match entry {
                None => offsets.push(0u32),
                Some(units) => {
                    offsets.push((text_start + text.len()) as u32);
                    for unit in units.iter().chain(&[0u16]) {
                        text.extend_from_slice(&unit.to_le_bytes());
                    }
                }
            }
        }

        let total = text_start + text.len();
        let mut out = Vec::with_capacity(total);
        out.extend_from_slice(&self.format);
        for word in [total as u32, 1, groups.len() as u32, self.entries.len() as u32, table as u32, 0] {
            out.extend_from_slice(&word.to_le_bytes());
        }
        for (index, first, last) in groups {
            for word in [index, first, last] {
                out.extend_from_slice(&word.to_le_bytes());
            }
        }
        for offset in offsets {
            out.extend_from_slice(&offset.to_le_bytes());
        }
        out.extend_from_slice(&text);
        out
    }

    pub fn get(&self, id: u32) -> Option<String> {
        self.entries.get(&id)?.as_ref().map(|units| String::from_utf16_lossy(units))
    }

    pub fn set(&mut self, id: u32, text: &str) {
        self.entries.insert(id, Some(text.encode_utf16().collect()));
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

fn read_utf16(bytes: &[u8], offset: usize) -> Result<Vec<u16>, FmgError> {
    let tail = bytes.get(offset..).ok_or(FmgError::Malformed("string offset past the end of the file"))?;
    let mut units = Vec::new();
    for pair in tail.as_chunks::<2>().0 {
        let unit = u16::from_le_bytes(*pair);
        if unit == 0 {
            return Ok(units);
        }
        units.push(unit);
    }
    Err(FmgError::Malformed("unterminated string"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Fmg {
        let mut f = Fmg { format: 0x10000u32.to_le_bytes(), entries: BTreeMap::new() };
        f.set(10, "ten");
        f.entries.insert(11, None);
        f.set(30, "thirty");
        f
    }

    #[test]
    fn round_trips_through_bytes() {
        let f = sample();
        let reparsed = Fmg::parse(&f.to_bytes()).unwrap();
        assert_eq!(reparsed, f);
        assert_eq!(reparsed.get(10).as_deref(), Some("ten"));
        assert_eq!(reparsed.get(11), None);
    }

    #[test]
    fn consecutive_ids_share_a_group() {
        let bytes = sample().to_bytes();
        // 10..=11 and 30.
        assert_eq!(u32_at(&bytes, 0x0c).unwrap(), 2);
    }

    #[test]
    fn set_adds_text_between_existing_ids() {
        let mut f = sample();
        f.set(20, "Pocket Bonfire");
        let reparsed = Fmg::parse(&f.to_bytes()).unwrap();
        assert_eq!(reparsed.get(20).as_deref(), Some("Pocket Bonfire"));
        assert_eq!(reparsed.get(30).as_deref(), Some("thirty"));
    }

    #[test]
    fn rejects_a_wrong_size_field() {
        let mut bytes = sample().to_bytes();
        bytes.push(0);
        assert!(Fmg::parse(&bytes).is_err());
    }
}
