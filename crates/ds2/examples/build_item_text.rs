//! Writes the Pocket Bonfire's name and descriptions into copies of the English text files, ready for
//! Mod Engine's `mod\menu\text\english\`. It also reports how faithfully the originals survive a parse
//! and rebuild, because the game gets the rebuilt file.
//!
//! cargo run -p ds2 --example build_item_text -- <unpacked menu\text\english> <output dir>

use ds2::fmg::Fmg;
use ds2::game::POCKET_BONFIRE_ITEM;
use std::path::Path;

const TEXT: [(&str, &str); 3] = [
    ("itemname.fmg", "Pocket Bonfire"),
    ("simpleexplanation.fmg", "Places your own bonfire where you stand. Use it there to rest."),
    (
        "detailedexplanation.fmg",
        "A flame that refuses to stay where it was lit.\n\nUse it to place a bonfire at your feet; any bonfire placed before goes out. Use it again beside that bonfire to rest, as you would at any other.\n\nRequires having rested at a bonfire in this land.",
    ),
];

fn main() {
    let mut args = std::env::args().skip(1);
    let input = args.next().expect("usage: build_item_text <menu\\text\\english> <output dir>");
    let output = args.next().expect("usage: build_item_text <menu\\text\\english> <output dir>");
    std::fs::create_dir_all(&output).expect("create output dir");

    for (file, text) in TEXT {
        let bytes = std::fs::read(Path::new(&input).join(file)).expect("read fmg");
        let mut fmg = Fmg::parse(&bytes).expect("parse fmg");
        let rebuilt = fmg.to_bytes();
        let same_text = Fmg::parse(&rebuilt).expect("reparse fmg") == fmg;
        println!(
            "{file}: {} entries, rebuild {}, text {}",
            fmg.len(),
            if rebuilt == bytes { "byte-identical" } else { "differs in layout" },
            if same_text { "identical" } else { "DIFFERENT" }
        );
        assert!(same_text, "{file} didn't survive a rebuild");
        assert_eq!(fmg.get(POCKET_BONFIRE_ITEM), None, "{file} already has this id");
        fmg.set(POCKET_BONFIRE_ITEM, text);
        std::fs::write(Path::new(&output).join(file), fmg.to_bytes()).expect("write fmg");
    }
}
