//! Prints `.param` rows as u32 words, to compare how existing items are set up.
//! With no ids, checks the file round-trips byte for byte and prints a summary.
//!
//! cargo run -p ds2 --example param_dump -- "C:\...\Param\ItemParam.param" 60350000 60355000

use ds2::param::Param;

fn main() {
    let mut args = std::env::args().skip(1);
    let path = args.next().expect("usage: param_dump <file.param> [row ids...]");
    let bytes = std::fs::read(&path).expect("read param");
    let param = Param::parse(&bytes).expect("parse param");
    let rebuilt = param.to_bytes().expect("rebuild param");
    println!(
        "{path}: {} rows of {} bytes, round trip {}",
        param.rows().count(),
        param.row_size(),
        if rebuilt == bytes { "identical" } else { "DIFFERENT" }
    );
    for id in args {
        let id: u32 = id.parse().expect("row id");
        match param.row(id) {
            Some(row) => {
                let words: Vec<String> =
                    row.as_chunks::<4>().0.iter().map(|w| format!("{:x}", u32::from_le_bytes(*w))).collect();
                println!("{id}: {}", words.join(" "));
            }
            None => println!("{id}: no such row"),
        }
    }
}
