//! Offline signature check, run by the regression tests and after any Steam patch: scans a
//! DarkSoulsII.exe on disk for every signature the mod uses. Each must match once, at the address the
//! mod expects, and every function signature must still be found after another mod's hook has
//! overwritten the function's first bytes: 5-, 12- and 14-byte jumps, with and without padding.
//!
//! With `--damaged`, it checks what a changed exe does instead: it lays the exe out in memory as the
//! game would, then breaks each signature in turn. Only the feature using that signature may fail,
//! and its error has to name it, so a player's log points at the cause and the other features keep
//! working.
//!
//! cargo run -p ds2 --example scan_exe -- "C:\...\Game\DarkSoulsII.exe" [--damaged]

use ds2::game::{self, ResolveError};
use ds2::module::Module;
use ds2::scan::{find_function_in, Pattern};

fn main() {
    let path = std::env::args().nth(1).expect("usage: scan_exe <path to DarkSoulsII.exe> [--damaged]");
    let file = std::fs::read(&path).expect("read exe");
    let pe = u32::from_le_bytes(file[0x3c..0x40].try_into().unwrap()) as usize;
    let size_of_image = u32::from_le_bytes(file[pe + 24 + 56..pe + 24 + 60].try_into().unwrap()) as usize;
    let sections = sections(&file, pe);
    if std::env::args().any(|a| a == "--damaged") {
        damaged(&file, pe, size_of_image, &sections);
    }
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


/// Overwrites the function's start with each kind of hook jump, and checks the mods' own lookup
/// (`find_function`) still finds exactly that function. The padded jumps are the shape hooking
/// libraries write when the jump ends inside an instruction; DS2 Inventory Suite's is one of them.
fn hooked_check(file: &[u8], sections: &[Section], sig: &game::Signature, failures: &mut Vec<String>) -> String {
    let Some(off) = to_file(sections, sig.rva) else {
        failures.push(format!("{} has no file offset", sig.name));
        return "no file offset".into();
    };
    let absolute: &[u8] = &[0xFF, 0x25, 0, 0, 0, 0, 1, 2, 3, 4, 5, 6, 7, 8];
    let jumps: [(&str, Vec<u8>); 5] = [
        ("5-byte", vec![0xE9, 0x11, 0x22, 0x33, 0x44]),
        ("5-byte padded", vec![0xE9, 0x11, 0x22, 0x33, 0x44, 0x90, 0x90, 0x90]),
        ("12-byte", vec![0x48, 0xB8, 1, 2, 3, 4, 5, 6, 7, 8, 0xFF, 0xE0]),
        ("14-byte", absolute.to_vec()),
        ("14-byte padded", [absolute, &[0x90, 0x90]].concat()),
    ];
    for (kind, jump) in jumps {
        let mut copy = file.to_vec();
        copy[off..off + jump.len()].copy_from_slice(&jump);
        match find_function_in(&copy, sig.aob) {
            Ok(hit) if hit == off => {}
            other => {
                failures.push(format!("{} not found after a {kind} hook ({other:?})", sig.name));
                return "FAILS".into();
            }
        }
    }
    "ok".into()
}

/// Which of the two resolvers failed on a signature, and the name in its error.
fn outcome(module: &Module) -> [Result<(), String>; 2] {
    let name = |e: ResolveError| match e {
        ResolveError::Pattern { name, .. } => name.to_string(),
        other => format!("not a signature error: {other}"),
    };
    [
        game::resolve(module).map(|_| ()).map_err(name),
        game::resolve_quick_start(module).map(|_| ()).map_err(name),
    ]
}

/// Lays the exe out as the loader would, then breaks one signature at a time.
fn damaged(file: &[u8], pe: usize, size_of_image: usize, sections: &[Section]) -> ! {
    let headers = u32::from_le_bytes(file[pe + 24 + 60..pe + 24 + 64].try_into().unwrap()) as usize;
    let mut image = vec![0u8; size_of_image];
    image[..headers].copy_from_slice(&file[..headers]);
    for s in sections {
        let len = s.raw_size.min(size_of_image - s.rva).min(file.len() - s.raw);
        image[s.rva..s.rva + len].copy_from_slice(&file[s.raw..s.raw + len]);
    }
    let module = Module { base: image.as_ptr() as usize, size: size_of_image, timestamp: 0 };
    let mut failures = Vec::new();
    let baseline = outcome(&module);
    if baseline.iter().any(|r| r.is_err()) {
        println!("FAILED: the undamaged image doesn't resolve: {baseline:?}");
        std::process::exit(1);
    }
    for sig in &game::SIGNATURES {
        let len = Pattern::parse(sig.aob).len();
        let saved = image[sig.rva..sig.rva + len].to_vec();
        // A plausible change from a patch: the code moved or was rewritten.
        for b in &mut image[sig.rva..sig.rva + len] {
            *b = 0xCC;
        }
        let module = Module { base: image.as_ptr() as usize, size: size_of_image, timestamp: 0 };
        let got = outcome(&module);
        image[sig.rva..sig.rva + len].copy_from_slice(&saved);
        let failed: Vec<&String> = got.iter().filter_map(|r| r.as_ref().err()).collect();
        let verdict = match failed.as_slice() {
            [one] if one.as_str() == sig.name => "ok: only its feature is off, and the error names it".to_string(),
            [] => "FAILS: nothing noticed".to_string(),
            [one] => format!("FAILS: the error names {one}"),
            _ => "FAILS: both features are off".to_string(),
        };
        if verdict.starts_with("FAILS") {
            failures.push(format!("{}: {verdict}", sig.name));
        }
        println!("  {:<24} {verdict}", sig.name);
    }
    by_address_checks(&mut image, size_of_image, &mut failures);
    opening_flag_checks(&mut image, size_of_image, &mut failures);
    if failures.is_empty() {
        println!("damaged exe: every signature fails alone, by name, and a changed function is still found by address");
    } else {
        println!("FAILED:
  {}", failures.join("
  "));
    }
    std::process::exit(if failures.is_empty() { 0 } else { 1 });
}

/// Another mod's change that no signature survives: a padded hook jump at the start and a byte changed
/// further in. On the 1.03 build each function is then found by its known address. On a build with
/// another timestamp it isn't, and neither is a function whose code is gone entirely.
fn by_address_checks(image: &mut [u8], size_of_image: usize, failures: &mut Vec<String>) {
    let jump = [0xFF, 0x25, 0, 0, 0, 0, 1, 2, 3, 4, 5, 6, 7, 8, 0x90, 0x90];
    for sig in game::SIGNATURES.iter().filter(|s| s.start) {
        let len = Pattern::parse(sig.aob).len();
        let saved = image[sig.rva..sig.rva + len].to_vec();
        image[sig.rva..sig.rva + jump.len()].copy_from_slice(&jump);
        image[sig.rva + len - 1] ^= 0xFF;
        let base = image.as_ptr() as usize;
        let module = |timestamp| Module { base, size: size_of_image, timestamp };
        let expected = base + sig.rva;
        let on_103 = game::find(&module(game::EXPECTED_TIMESTAMP), sig.name).ok();
        let elsewhere = game::find(&module(0), sig.name).ok();
        for b in &mut image[sig.rva..sig.rva + len] {
            *b = 0xCC;
        }
        let gone = game::find(&module(game::EXPECTED_TIMESTAMP), sig.name).ok();
        image[sig.rva..sig.rva + len].copy_from_slice(&saved);
        let verdict = match (on_103 == Some(expected), elsewhere, gone) {
            (true, None, None) => "ok".to_string(),
            (false, _, _) => "FAILS: not found by address on 1.03".to_string(),
            (_, Some(_), _) => "FAILS: found by address on another build".to_string(),
            (_, _, Some(_)) => "FAILS: found by address with its code gone".to_string(),
        };
        if verdict != "ok" {
            failures.push(format!("{} by address: {verdict}", sig.name));
        }
        println!("  {:<24} hooked and patched: {verdict}", sig.name);
    }
}

/// The quick start reads the "opening shown" flag's address out of the title function's instructions.
/// Behind a short hook it must come out the same as on the stock exe; behind a jump covering both
/// instructions that name it, the quick start must refuse, by name, rather than use garbage.
fn opening_flag_checks(image: &mut [u8], size_of_image: usize, failures: &mut Vec<String>) {
    let sig = game::SIGNATURES.iter().find(|s| s.name == "TitleInitBranch").expect("known");
    let module = Module { base: image.as_ptr() as usize, size: size_of_image, timestamp: game::EXPECTED_TIMESTAMP };
    let stock = game::resolve_quick_start(&module).map(|q| q.opening_shown).ok();
    let saved = image[sig.rva..sig.rva + 16].to_vec();
    let mut check = |jump: &[u8], want_ok: bool, kind: &str| {
        image[sig.rva..sig.rva + jump.len()].copy_from_slice(jump);
        let module = Module { base: image.as_ptr() as usize, size: size_of_image, timestamp: game::EXPECTED_TIMESTAMP };
        let got = game::resolve_quick_start(&module);
        image[sig.rva..sig.rva + 16].copy_from_slice(&saved);
        let verdict = match (&got, want_ok) {
            (Ok(q), true) if Some(q.opening_shown) == stock => "ok".to_string(),
            (Ok(q), _) => format!("FAILS: flag at {:#x}, stock {stock:x?}", q.opening_shown),
            (Err(game::ResolveError::Pattern { name: "TitleInitBranch", .. }), false) => "ok, refused by name".to_string(),
            (Err(e), _) => format!("FAILS: {e}"),
        };
        if verdict.starts_with("FAILS") {
            failures.push(format!("opening flag after a {kind} hook: {verdict}"));
        }
        println!("  opening flag, {kind} hook: {verdict}");
    };
    check(&[0xE9, 1, 2, 3, 4], true, "5-byte");
    check(&[0xFF, 0x25, 0, 0, 0, 0, 1, 2, 3, 4, 5, 6, 7, 8, 0x90, 0x90], false, "16-byte");
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
