//! Reader and writer for DS2 SotFS `.param` data, in a file or in the game's memory.
//!
//! Layout, as read from the files themselves:
//! - 0x00 u32 string table offset (the total length when there are no names)
//! - 0x08 u16 def version, 0x0a u16 row count, 0x0c 32-byte param type
//! - 0x30 u64 offset of the row data; the row table starts at 0x40
//! - row table: { u64 id, u64 data offset, u64 name offset }, sorted by id
//! - row data: fixed-size rows, then the string table holding row names
//!
//! The game's own params have no names, but params saved by some editors do. Names are kept: the
//! string table is carried over as it is, and each row keeps its place in it.

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
struct Row {
    id: u32,
    data: Vec<u8>,
    /// Where the row's name starts within the string table, if it has one.
    name: Option<usize>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Param {
    /// The first 0x30 bytes, kept verbatim apart from the two fields `to_bytes` recomputes.
    header: [u8; 0x30],
    row_size: usize,
    rows: Vec<Row>,
    /// Everything after the row data: the row names.
    strings: Vec<u8>,
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
        if data != HEADER + count * ENTRY {
            return Err(Bad("row data doesn't follow the row table"));
        }
        if strings > bytes.len() || strings < data {
            return Err(Bad("string table is outside the data"));
        }
        if count == 0 {
            return Err(Bad("no rows to take the row size from"));
        }
        let row_size = (strings - data) / count;
        if row_size == 0 || !(strings - data).is_multiple_of(count) {
            return Err(Bad("row data isn't a whole number of rows"));
        }

        let mut rows = Vec::with_capacity(count);
        for i in 0..count {
            let entry = HEADER + i * ENTRY;
            let id = u64_at(bytes, entry);
            let offset = u64_at(bytes, entry + 8) as usize;
            let name = u64_at(bytes, entry + 16) as usize;
            if id > u32::MAX as u64 {
                return Err(Bad("row id doesn't fit in a u32"));
            }
            if offset != data + i * row_size {
                return Err(Bad("rows aren't stored in table order"));
            }
            let name = match name {
                0 => None,
                at if (strings..bytes.len()).contains(&at) => Some(at - strings),
                _ => return Err(Bad("row name is outside the string table")),
            };
            rows.push(Row { id: id as u32, data: bytes[offset..offset + row_size].to_vec(), name });
        }
        if rows.windows(2).any(|w| w[0].id >= w[1].id) {
            return Err(Bad("row ids aren't sorted"));
        }
        let mut header = [0u8; 0x30];
        header.copy_from_slice(&bytes[..0x30]);
        Ok(Param { header, row_size, rows, strings: bytes[strings..].to_vec() })
    }

    pub fn to_bytes(&self) -> Result<Vec<u8>, ParamError> {
        let count: u16 = self.rows.len().try_into().map_err(|_| ParamError::TooManyRows)?;
        let data = HEADER + self.rows.len() * ENTRY;
        let strings = data + self.rows.len() * self.row_size;

        let mut out = Vec::with_capacity(strings + self.strings.len());
        out.extend_from_slice(&self.header);
        out[0..4].copy_from_slice(&(strings as u32).to_le_bytes());
        out[0x0a..0x0c].copy_from_slice(&count.to_le_bytes());
        out.extend_from_slice(&(data as u64).to_le_bytes());
        out.extend_from_slice(&[0u8; 8]);
        for (i, row) in self.rows.iter().enumerate() {
            out.extend_from_slice(&(row.id as u64).to_le_bytes());
            out.extend_from_slice(&((data + i * self.row_size) as u64).to_le_bytes());
            out.extend_from_slice(&(row.name.map_or(0, |at| strings + at) as u64).to_le_bytes());
        }
        for row in &self.rows {
            out.extend_from_slice(&row.data);
        }
        out.extend_from_slice(&self.strings);
        Ok(out)
    }

    pub fn row_size(&self) -> usize {
        self.row_size
    }

    pub fn rows(&self) -> impl Iterator<Item = (u32, &[u8])> {
        self.rows.iter().map(|row| (row.id, row.data.as_slice()))
    }

    pub fn row(&self, id: u32) -> Option<&[u8]> {
        let i = self.rows.binary_search_by_key(&id, |row| row.id).ok()?;
        Some(&self.rows[i].data)
    }

    /// Adds a row with no name, keeping the table sorted as the game expects.
    pub fn insert(&mut self, id: u32, data: Vec<u8>) -> Result<(), ParamError> {
        if data.len() != self.row_size {
            return Err(ParamError::WrongRowSize { expected: self.row_size, got: data.len() });
        }
        match self.rows.binary_search_by_key(&id, |row| row.id) {
            Ok(_) => Err(ParamError::DuplicateId(id)),
            Err(at) => {
                self.rows.insert(at, Row { id, data, name: None });
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

    /// The same, with both rows named, as some param editors save them.
    fn named() -> Vec<u8> {
        let mut b = sample();
        let strings = b.len();
        b[0..4].copy_from_slice(&(strings as u32).to_le_bytes());
        let names = ["ten\0", "thirty\0"];
        let mut at = strings;
        for (i, name) in names.iter().enumerate() {
            let entry = HEADER + i * ENTRY + 16;
            b[entry..entry + 8].copy_from_slice(&(at as u64).to_le_bytes());
            at += name.len();
        }
        for name in names {
            b.extend_from_slice(name.as_bytes());
        }
        b
    }

    #[test]
    fn round_trips_byte_for_byte() {
        let bytes = sample();
        assert_eq!(Param::parse(&bytes).unwrap().to_bytes().unwrap(), bytes);
        let bytes = named();
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
    fn insert_into_a_named_param_keeps_every_name_pointing_at_its_text() {
        let mut p = Param::parse(&named()).unwrap();
        p.insert(20, vec![2; 4]).unwrap();
        let bytes = p.to_bytes().unwrap();
        let name_of = |row: usize| {
            let at = u64_at(&bytes, HEADER + row * ENTRY + 16) as usize;
            (at != 0).then(|| bytes[at..].split(|&b| b == 0).next().unwrap().to_vec())
        };
        assert_eq!(name_of(0).as_deref(), Some(&b"ten"[..]));
        assert_eq!(name_of(1), None);
        assert_eq!(name_of(2).as_deref(), Some(&b"thirty"[..]));
        assert_eq!(Param::parse(&bytes).unwrap().row(30), Some(&[3u8; 4][..]));
    }

    #[test]
    fn insert_rejects_duplicates_and_wrong_sizes() {
        let mut p = Param::parse(&sample()).unwrap();
        assert_eq!(p.insert(10, vec![0; 4]), Err(ParamError::DuplicateId(10)));
        assert_eq!(p.insert(20, vec![0; 5]), Err(ParamError::WrongRowSize { expected: 4, got: 5 }));
    }
}
