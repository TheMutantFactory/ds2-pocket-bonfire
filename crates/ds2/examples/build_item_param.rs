//! Writes the release ItemParam: the input plus the Pocket Bonfire's one row, a clone of Aged Feather
//! (reusable, max 1) whose Effect ID is its own id. Refuses a file that already has the row.
//!
//! cargo run -p ds2 --example build_item_param -- <ItemParam.param> <output.param>

use ds2::game::POCKET_BONFIRE_ITEM;
use ds2::param::Param;

const AGED_FEATHER: u32 = 60355000;
const OFF_EFFECT_ID: usize = 0x04;

fn main() {
    let mut args = std::env::args().skip(1);
    let input = args.next().expect("usage: build_item_param <ItemParam.param> <output.param>");
    let output = args.next().expect("usage: build_item_param <ItemParam.param> <output.param>");

    let mut param = Param::parse(&std::fs::read(&input).expect("read input")).expect("parse input");
    assert!(param.row(POCKET_BONFIRE_ITEM).is_none(), "{input} already has the item");
    let mut row = param.row(AGED_FEATHER).expect("Aged Feather row").to_vec();
    row[OFF_EFFECT_ID..OFF_EFFECT_ID + 4].copy_from_slice(&POCKET_BONFIRE_ITEM.to_le_bytes());
    param.insert(POCKET_BONFIRE_ITEM, row).expect("insert row");
    std::fs::write(&output, param.to_bytes().expect("build param")).expect("write output");
    println!("wrote {output}: {} rows", param.rows().count());
}
