//! Writes the release ItemParam: the input plus the Pocket Bonfire's one row, a clone of Aged Feather
//! (reusable, max 1) whose Effect ID is its own id. Refuses a file that already has the row.
//!
//! cargo run -p ds2 --example build_item_param -- <ItemParam.param> <output.param>

use ds2::item;

fn main() {
    let mut args = std::env::args().skip(1);
    let input = args.next().expect("usage: build_item_param <ItemParam.param> <output.param>");
    let output = args.next().expect("usage: build_item_param <ItemParam.param> <output.param>");

    let bytes = std::fs::read(&input).expect("read input");
    let with_row = item::add_row(&bytes).expect("add row").expect("input already has the item");
    std::fs::write(&output, with_row).expect("write output");
    println!("wrote {output}");
}
