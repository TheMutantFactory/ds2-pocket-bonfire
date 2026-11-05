//! The Pocket Bonfire item itself: its row in ItemParam and its text, with nothing tied to where they
//! end up, so the DLL can add them in memory and the tools can write them to files.

use crate::game::POCKET_BONFIRE_ITEM;
use crate::param::{Param, ParamError};

/// Reusable and limited to one, which is what the item needs. It also lends the row its icon.
pub const AGED_FEATHER: u32 = 60_355_000;
const OFF_EFFECT_ID: usize = 0x04;

pub const NAME: &str = "Pocket Bonfire";
pub const SIMPLE: &str = "Places your own bonfire where you stand. Use it there to rest.";
/// The detail panel doesn't wrap, so the line breaks are part of the text: 57 characters were cut off
/// in play, and 49 fit.
pub const DETAILED: &str = "A flame that refuses to stay where it was lit.\n\nUse it to place a bonfire at your feet. Any bonfire\nplaced before goes out. Use it again beside that\nbonfire to rest, as you would at any other.\n\nRequires having rested at a bonfire in this land.";

/// Returns ItemParam with the item's row added, or `None` if it's already there. Using the item applies
/// its Effect ID, which is the row's own id, and that's what the DLL watches for.
pub fn add_row(item_param: &[u8]) -> Result<Option<Vec<u8>>, AddRowError> {
    let mut param = Param::parse(item_param).map_err(AddRowError::Param)?;
    if param.row(POCKET_BONFIRE_ITEM).is_some() {
        return Ok(None);
    }
    let mut row = param.row(AGED_FEATHER).ok_or(AddRowError::NoFeather)?.to_vec();
    row[OFF_EFFECT_ID..OFF_EFFECT_ID + 4].copy_from_slice(&POCKET_BONFIRE_ITEM.to_le_bytes());
    param.insert(POCKET_BONFIRE_ITEM, row).map_err(AddRowError::Param)?;
    param.to_bytes().map(Some).map_err(AddRowError::Param)
}

#[derive(Debug, PartialEq, Eq)]
pub enum AddRowError {
    Param(ParamError),
    NoFeather,
}

impl std::fmt::Display for AddRowError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AddRowError::Param(e) => write!(f, "{e}"),
            AddRowError::NoFeather => write!(f, "ItemParam has no Aged Feather row to copy"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// An ItemParam holding the feather and a later row, with 8-byte rows.
    fn item_param() -> Vec<u8> {
        const HEADER: usize = 0x40;
        const ENTRY: usize = 24;
        let rows = [(AGED_FEATHER, [1u8, 2, 3, 4, 5, 6, 7, 8]), (POCKET_BONFIRE_ITEM + 1, [9; 8])];
        let data = HEADER + rows.len() * ENTRY;
        let mut b = vec![0u8; HEADER];
        b[0..4].copy_from_slice(&((data + rows.len() * 8) as u32).to_le_bytes());
        b[0x0a..0x0c].copy_from_slice(&(rows.len() as u16).to_le_bytes());
        b[0x30..0x38].copy_from_slice(&(data as u64).to_le_bytes());
        for (i, (id, _)) in rows.iter().enumerate() {
            b.extend_from_slice(&(*id as u64).to_le_bytes());
            b.extend_from_slice(&((data + i * 8) as u64).to_le_bytes());
            b.extend_from_slice(&[0u8; 8]);
        }
        for (_, row) in &rows {
            b.extend_from_slice(row);
        }
        b
    }

    #[test]
    fn adds_a_feather_copy_with_its_own_effect_and_keeps_rows_sorted() {
        let out = add_row(&item_param()).unwrap().unwrap();
        let param = Param::parse(&out).unwrap();
        let ids: Vec<u32> = param.rows().map(|(id, _)| id).collect();
        assert_eq!(ids, [AGED_FEATHER, POCKET_BONFIRE_ITEM, POCKET_BONFIRE_ITEM + 1]);
        let row = param.row(POCKET_BONFIRE_ITEM).unwrap();
        assert_eq!(row[..4], [1, 2, 3, 4]);
        assert_eq!(row[4..8], POCKET_BONFIRE_ITEM.to_le_bytes());
        assert_eq!(param.row(AGED_FEATHER).unwrap(), [1, 2, 3, 4, 5, 6, 7, 8]);
    }

    #[test]
    fn a_param_that_already_has_the_row_is_left_alone() {
        let once = add_row(&item_param()).unwrap().unwrap();
        assert_eq!(add_row(&once), Ok(None));
    }

    #[test]
    fn detailed_text_fits_the_panel() {
        assert!(DETAILED.lines().all(|l| l.chars().count() <= 52));
    }
}
