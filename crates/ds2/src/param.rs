//! Reader and writer for DS2 SotFS loose `.param` files.
//!
//! Layout, as read from the files themselves:
//! - 0x00 u32 string table offset (file length when there are no names)
//! - 0x08 u16 def version, 0x0a u16 row count, 0x0c 32-byte param type
//! - 0x30 u64 offset of the row data; the row table starts at 0x40
//! - row table: { u64 id, u64 data offset, u64 name offset }, sorted by id
//! - row data: fixed-size rows, then the string table
//!
//! Only name-less params are supported, which covers the item tables. Anything else is an error
//! rather than a silently damaged file.

use std::fmt;

const HEADER: usize = 0x40;
const ENTRY: usize = 24;

#[derive(Debug, PartialEq, Eq)]
pub enum ParamError {
    TooShort,
    UnsupportedLayout(&'static str),
    DuplicateId(u32),
    WrongRowSize { expected: usize, got: usize },
    TooManyRows,
}

impl fmt::Display for ParamError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ParamError::TooShort => write!(f, "file too short to be a param"),
            ParamError::UnsupportedLayout(why) => write!(f, "unsupported param layout: {why}"),
            ParamError::DuplicateId(id) => write!(f, "row {id} already exists"),
            ParamError::WrongRowSize { expected, got } => write!(f, "row is {got} bytes, this param uses {expected}"),
            ParamError::TooManyRows => write!(f, "row count doesn't fit in a u16"),
        }
    }
}

impl std::error::Error for ParamError {}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Param {
    /// The first 0x30 bytes, kept verbatim apart from the two fields `to_bytes` recomputes.
    header: [u8; 0x30],
    row_size: usize,
    rows: Vec<(u32, Vec<u8>)>,
}

fn u32_at(b: &[u8], at: usize) -> u32 {
    u32::from_le_bytes(b[at..at + 4].try_into().unwrap())
}

fn u64_at(b: &[u8], at: usize) -> u64 {
    u64::from_le_bytes(b[at..at + 8].try_into().unwrap())
}

impl Param {
    pub fn parse(bytes: &[u8]) -> Result<Param, ParamError> {
        use ParamError::UnsupportedLayout as Bad;
        if bytes.len() < HEADER {
            return Err(ParamError::TooShort);
        }
        let count = u16::from_le_bytes([bytes[0x0a], bytes[0x0b]]) as usize;
        let strings = u32_at(bytes, 0) as usize;
        let data = u64_at(bytes, 0x30) as usize;
        if strings != bytes.len() {
            return Err(Bad("has a string table"));
        }
        if data != HEADER + count * ENTRY {
            return Err(Bad("row data doesn't follow the row table"));
        }
        if count == 0 {
            return Err(Bad("no rows to take the row size from"));
        }
        let row_size = (bytes.len() - data) / count;
        if row_size == 0 || !(bytes.len() - data).is_multiple_of(count) {
            return Err(Bad("row data isn't a whole number of rows"));
        }

        let mut rows = Vec::with_capacity(count);
        for i in 0..count {
            let entry = HEADER + i * ENTRY;
            let id = u64_at(bytes, entry);
            let offset = u64_at(bytes, entry + 8) as usize;
            if id > u32::MAX as u64 || u64_at(bytes, entry + 16) != 0 {
                return Err(Bad("row entry has a big id or a name"));
            }
            if offset != data + i * row_size {
                return Err(Bad("rows aren't stored in table order"));
            }
            rows.push((id as u32, bytes[offset..offset + row_size].to_vec()));
        }
        if rows.windows(2).any(|w| w[0].0 >= w[1].0) {
            return Err(Bad("row ids aren't sorted"));
        }
        let mut header = [0u8; 0x30];
        header.copy_from_slice(&bytes[..0x30]);
        Ok(Param { header, row_size, rows })
    }

    pub fn to_bytes(&self) -> Result<Vec<u8>, ParamError> {
        let count: u16 = self.rows.len().try_into().map_err(|_| ParamError::TooManyRows)?;
        let data = HEADER + self.rows.len() * ENTRY;
        let total = data + self.rows.len() * self.row_size;

        let mut out = Vec::with_capacity(total);
        out.extend_from_slice(&self.header);
        out[0..4].copy_from_slice(&(total as u32).to_le_bytes());
        out[0x0a..0x0c].copy_from_slice(&count.to_le_bytes());
        out.extend_from_slice(&(data as u64).to_le_bytes());
        out.extend_from_slice(&[0u8; 8]);
        for (i, (id, _)) in self.rows.iter().enumerate() {
            out.extend_from_slice(&(*id as u64).to_le_bytes());
            out.extend_from_slice(&((data + i * self.row_size) as u64).to_le_bytes());
            out.extend_from_slice(&[0u8; 8]);
        }
        for (_, row) in &self.rows {
            out.extend_from_slice(row);
        }
        Ok(out)
    }

    pub fn row_size(&self) -> usize {
        self.row_size
    }

    pub fn rows(&self) -> impl Iterator<Item = (u32, &[u8])> {
        self.rows.iter().map(|(id, row)| (*id, row.as_slice()))
    }

    pub fn row(&self, id: u32) -> Option<&[u8]> {
        let i = self.rows.binary_search_by_key(&id, |(r, _)| *r).ok()?;
        Some(&self.rows[i].1)
    }

    /// Adds a row, keeping the table sorted as the game expects.
    pub fn insert(&mut self, id: u32, row: Vec<u8>) -> Result<(), ParamError> {
        if row.len() != self.row_size {
            return Err(ParamError::WrongRowSize { expected: self.row_size, got: row.len() });
        }
        match self.rows.binary_search_by_key(&id, |(r, _)| *r) {
            Ok(_) => Err(ParamError::DuplicateId(id)),
            Err(at) => {
                self.rows.insert(at, (id, row));
                Ok(())
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A two-row file with 4-byte rows, built by hand so the layout is spelled out.
    fn sample() -> Vec<u8> {
        let mut b = vec![0u8; HEADER];
        let total = HEADER + 2 * ENTRY + 2 * 4;
        b[0..4].copy_from_slice(&(total as u32).to_le_bytes());
        b[0x08] = 0x15;
        b[0x0a] = 2;
        b[0x0c..0x10].copy_from_slice(b"ITEM");
        b[0x2d] = 4;
        b[0x30..0x38].copy_from_slice(&((HEADER + 2 * ENTRY) as u64).to_le_bytes());
        for (i, id) in [10u64, 30].iter().enumerate() {
            b.extend_from_slice(&id.to_le_bytes());
            b.extend_from_slice(&((HEADER + 2 * ENTRY + i * 4) as u64).to_le_bytes());
            b.extend_from_slice(&[0u8; 8]);
        }
        b.extend_from_slice(&[1, 1, 1, 1, 3, 3, 3, 3]);
        b
    }

    #[test]
    fn round_trips_byte_for_byte() {
        let bytes = sample();
        assert_eq!(Param::parse(&bytes).unwrap().to_bytes().unwrap(), bytes);
    }

    #[test]
    fn insert_keeps_rows_sorted_and_the_file_parseable() {
        let mut p = Param::parse(&sample()).unwrap();
        p.insert(20, vec![2; 4]).unwrap();
        let reparsed = Param::parse(&p.to_bytes().unwrap()).unwrap();
        assert_eq!(reparsed.rows().map(|(id, _)| id).collect::<Vec<_>>(), [10, 20, 30]);
        assert_eq!(reparsed.row(20), Some(&[2u8; 4][..]));
        assert_eq!(reparsed.row(30), Some(&[3u8; 4][..]));
    }

    #[test]
    fn insert_rejects_duplicates_and_wrong_sizes() {
        let mut p = Param::parse(&sample()).unwrap();
        assert_eq!(p.insert(10, vec![0; 4]), Err(ParamError::DuplicateId(10)));
        assert_eq!(p.insert(20, vec![0; 5]), Err(ParamError::WrongRowSize { expected: 4, got: 5 }));
    }

    #[test]
    fn rejects_a_param_with_a_string_table() {
        let mut bytes = sample();
        bytes.extend_from_slice(b"name\0");
        assert_eq!(Param::parse(&bytes), Err(ParamError::UnsupportedLayout("has a string table")));
    }
}
