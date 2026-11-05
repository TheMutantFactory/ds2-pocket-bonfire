//! Finds little-endian u32 values in every file under a directory, for tracing where the game
//! keeps a given id. Compressed containers (.dcx) only match if they're inflated first.
//!
//! cargo run -p ds2 --example find_u32 -- "C:\Games\DS2-dev\Game" 60355000 2210

use std::path::Path;

const MAX_FILE: u64 = 300 * 1024 * 1024;

fn walk(dir: &Path, needles: &[(u32, [u8; 4])]) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            if path.file_name().is_some_and(|n| n == "_backup") {
                continue;
            }
            walk(&path, needles);
            continue;
        }
        if entry.metadata().map_or(true, |m| m.len() > MAX_FILE) {
            continue;
        }
        let Ok(bytes) = std::fs::read(&path) else { continue };
        for (value, needle) in needles {
            let hits: Vec<usize> = bytes.windows(4).enumerate().filter(|(_, w)| w == needle).map(|(i, _)| i).collect();
            if !hits.is_empty() {
                let shown: Vec<String> = hits.iter().take(6).map(|h| format!("{h:#x}")).collect();
                println!("{value}: {} hit(s) in {} at {}", hits.len(), path.display(), shown.join(" "));
            }
        }
    }
}

fn main() {
    let mut args = std::env::args().skip(1);
    let dir = args.next().expect("usage: find_u32 <dir> <values...>");
    let needles: Vec<(u32, [u8; 4])> = args
        .map(|a| {
            let v: u32 = a.parse().expect("value");
            (v, v.to_le_bytes())
        })
        .collect();
    walk(Path::new(&dir), &needles);
}
