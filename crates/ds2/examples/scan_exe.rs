//! Offline signature check, run by the regression tests and after any Steam patch: scans a
//! DarkSoulsII.exe on disk for every signature the mod uses. Each must match once, at the address the
//! mod expects, and every function signature must still match once after another mod's hook has
//! overwritten the function's first bytes, both as a 5-byte and a 14-byte jump.
//!
//! cargo run -p ds2 --example scan_exe -- "C:\...\Game\DarkSoulsII.exe"

use ds2::game;
use ds2::scan::{Pattern, HOOK_BYTES};

fn main() {
    let path = std::env::args().nth(1).expect("usage: scan_exe <path to DarkSoulsII.exe>");
    let file = std::fs::read(&path).expect("read exe");
    let pe = u32::from_le_bytes(file[0x3c..0x40].try_into().unwrap()) as usize;
    let size_of_image = u32::from_le_bytes(file[pe + 24 + 56..pe + 24 + 60].try_into().unwrap()) as usize;
    let sections = sections(&file, pe);
    let supported = size_of_image == game::EXPECTED_IMAGE_SIZE;
    println!("{path}\n  SizeOfImage {size_of_image:#x} ({})", if supported { "supported" } else { "NOT the supported version" });

    let mut failures = Vec::new();
    for sig in &game::SIGNATURES {
        let pattern = Pattern::parse(sig.aob);
        let hits: Vec<usize> = pattern.find_in(&file).into_iter().filter_map(|off| to_rva(&sections, off)).collect();
        let at = match hits.as_slice() {
            [rva] if *rva == sig.rva => format!("+{rva:#x}"),
            [rva] => {
                failures.push(format!("{} found at +{rva:#x}, expected +{:#x}", sig.name, sig.rva));
                format!("+{rva:#x} (expected +{:#x})", sig.rva)
            }
            _ => {
                failures.push(format!("{} matched {} times", sig.name, hits.len()));
                format!("{} hits", hits.len())
            }
        };
        let hooked = if sig.start { hooked_check(&file, &sections, sig, &mut failures) } else { "-".into() };
        println!("  {:<24} {at:<12} hooked: {hooked}", sig.name);
    }
    if !supported {
        failures.push("unsupported exe".into());
    }
    if failures.is_empty() {
        println!("all signatures ok");
    } else {
        println!("FAILED:\n  {}", failures.join("\n  "));
    }
    std::process::exit(if failures.is_empty() { 0 } else { 1 });
}


/// Overwrites the function's start with each kind of hook jump, and checks the signature with its
/// head made wild still finds exactly that function.
fn hooked_check(file: &[u8], sections: &[Section], sig: &game::Signature, failures: &mut Vec<String>) -> String {
    let Some(off) = to_file(sections, sig.rva) else {
        failures.push(format!("{} has no file offset", sig.name));
        return "no file offset".into();
    };
    let tolerant = Pattern::parse(sig.aob).with_wild_head(HOOK_BYTES);
    let jumps: [&[u8]; 2] = [&[0xE9, 0x11, 0x22, 0x33, 0x44], &[0xFF, 0x25, 0, 0, 0, 0, 1, 2, 3, 4, 5, 6, 7, 8]];
    for jump in jumps {
        let mut copy = file.to_vec();
        copy[off..off + jump.len()].copy_from_slice(jump);
        let hits = tolerant.find_in(&copy);
        if hits != [off] {
            failures.push(format!("{} not found once after a {}-byte hook ({} hits)", sig.name, jump.len(), hits.len()));
            return "FAILS".into();
        }
    }
    "ok".into()
}

struct Section {
    rva: usize,
    raw: usize,
    raw_size: usize,
}

fn sections(file: &[u8], pe: usize) -> Vec<Section> {
    let count = u16::from_le_bytes(file[pe + 6..pe + 8].try_into().unwrap()) as usize;
    let opt_size = u16::from_le_bytes(file[pe + 20..pe + 22].try_into().unwrap()) as usize;
    let table = pe + 24 + opt_size;
    (0..count)
        .map(|i| {
            let s = &file[table + i * 40..table + i * 40 + 40];
            let u = |o: usize| u32::from_le_bytes(s[o..o + 4].try_into().unwrap()) as usize;
            Section { rva: u(12), raw_size: u(16), raw: u(20) }
        })
        .collect()
}

fn to_rva(sections: &[Section], off: usize) -> Option<usize> {
    sections.iter().find(|s| off >= s.raw && off < s.raw + s.raw_size).map(|s| off - s.raw + s.rva)
}

fn to_file(sections: &[Section], rva: usize) -> Option<usize> {
    sections.iter().find(|s| rva >= s.rva && rva < s.rva + s.raw_size).map(|s| rva - s.rva + s.raw)
}
