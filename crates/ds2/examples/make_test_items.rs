//! Writes an ItemParam with two test items added, each a clone of Aged Feather (reusable, max 1) with
//! its own Effect ID, to see what the game does when they're used:
//! - 60355010 keeps the feather's usage (2210),
//! - 60355020 uses the general Buff / Utility usage (1700).
//!
//! cargo run -p ds2 --example make_test_items -- <ItemParam.param> <output.param>

use ds2::param::Param;

const AGED_FEATHER: u32 = 60355000;
const OFF_EFFECT_ID: usize = 0x04;
const OFF_ITEM_USAGE_ID: usize = 0x44;

fn main() {
    let mut args = std::env::args().skip(1);
    let input = args.next().expect("usage: make_test_items <ItemParam.param> <output.param>");
    let output = args.next().expect("usage: make_test_items <ItemParam.param> <output.param>");

    let mut param = Param::parse(&std::fs::read(&input).expect("read input")).expect("parse input");
    let feather = param.row(AGED_FEATHER).expect("Aged Feather row").to_vec();
    for (id, usage) in [(60355010u32, 2210u32), (60355020, 1700)] {
        let mut row = feather.clone();
        row[OFF_EFFECT_ID..OFF_EFFECT_ID + 4].copy_from_slice(&id.to_le_bytes());
        row[OFF_ITEM_USAGE_ID..OFF_ITEM_USAGE_ID + 4].copy_from_slice(&usage.to_le_bytes());
        param.insert(id, row).expect("insert row");
    }
    std::fs::write(&output, param.to_bytes().expect("build param")).expect("write output");
    println!("wrote {output}: {} rows", param.rows().count());
}
