//! Append-only log file next to the DLL, with millisecond timestamps since load.

use std::fs::{File, OpenOptions};
use std::io::Write;
use std::sync::{Mutex, OnceLock};
use std::time::Instant;

static LOG: OnceLock<(Mutex<File>, Instant)> = OnceLock::new();

pub fn init(addr_in_dll: usize, file_name: &str) {
    let dir = crate::module::module_path_of(addr_in_dll)
        .and_then(|p| p.parent().map(|d| d.to_path_buf()))
        .unwrap_or_else(|| ".".into());
    if let Ok(f) = OpenOptions::new().create(true).append(true).open(dir.join(file_name)) {
        let _ = LOG.set((Mutex::new(f), Instant::now()));
        line(format!("---- {file_name} (ds2 {}) opened", env!("CARGO_PKG_VERSION")));
    }
}

pub fn line(msg: String) {
    if let Some((file, start)) = LOG.get() {
        if let Ok(mut f) = file.lock() {
            let _ = writeln!(f, "[{:>9.3}] {msg}", start.elapsed().as_secs_f64());
            let _ = f.flush();
        }
    }
}
