//! Offline signature check: scans a DarkSoulsII.exe on disk for every signature the mod uses and
//! prints the hit count and RVA of each. Run it after a Steam patch before trusting the DLL.
//!
//! cargo run -p ds2 --example scan_exe -- "C:\...\Game\DarkSoulsII.exe"

use ds2::game;
use ds2::scan::Pattern;

fn main() {
    let path = std::env::args().nth(1).expect("usage: scan_exe <path to DarkSoulsII.exe>");
    let file = std::fs::read(&path).expect("read exe");
    let pe = u32::from_le_bytes(file[0x3c..0x40].try_into().unwrap()) as usize;
    let size_of_image = u32::from_le_bytes(file[pe + 24 + 56..pe + 24 + 60].try_into().unwrap()) as usize;
    let sections = sections(&file, pe);
    println!(
        "{path}\n  SizeOfImage {size_of_image:#x} ({})",
        if size_of_image == game::EXPECTED_IMAGE_SIZE { "supported" } else { "NOT the supported version" }
    );

    let mut ok = true;
    for (name, aob) in [
        ("BaseA", game::AOB_BASE_A),
        ("SetWarpTarget", game::AOB_SET_WARP_TARGET),
        ("WarpFunc", game::AOB_WARP_FUNC),
    ] {
        let hits = Pattern::parse(aob).find_in(&file);
        let rvas: Vec<String> = hits
            .iter()
            .map(|&off| match to_rva(&sections, off) {
                Some((rva, sec)) => format!("+{rva:#x} ({sec})"),
                None => format!("file {off:#x} (no section)"),
            })
            .collect();
        ok &= hits.len() == 1;
        println!("  {name:<14} {} hit(s) {}", hits.len(), rvas.join(", "));
        if name == "BaseA" {
            if let [off] = hits.as_slice() {
                let disp = i32::from_le_bytes(file[off + 3..off + 7].try_into().unwrap());
                if let Some((rva, _)) = to_rva(&sections, *off) {
                    println!("  {:<14} global at +{:#x}", "", (rva as i64 + 7 + disp as i64));
                }
            }
        }
    }
    std::process::exit(if ok { 0 } else { 1 });
}

struct Section {
    name: String,
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
            Section {
                name: String::from_utf8_lossy(&s[..8]).trim_end_matches('\0').to_string(),
                rva: u(12),
                raw_size: u(16),
                raw: u(20),
            }
        })
        .collect()
}

fn to_rva(sections: &[Section], off: usize) -> Option<(usize, &str)> {
    sections
        .iter()
        .find(|s| off >= s.raw && off < s.raw + s.raw_size)
        .map(|s| (off - s.raw + s.rva, s.name.as_str()))
}
