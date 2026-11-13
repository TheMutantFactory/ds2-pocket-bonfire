//! A small settings file of `name = true` / `name = false` lines next to the DLL. A missing file is
//! written out with the defaults, so players can find and edit it.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// One setting: its name, default, and the comment written above it.
pub struct Setting {
    pub name: &'static str,
    pub default: bool,
    pub help: &'static str,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Settings {
    values: HashMap<String, bool>,
    /// Lines that couldn't be read, for the log.
    pub problems: Vec<String>,
}

impl Settings {
    /// Parses the file's text; anything unrecognised keeps its default and is reported.
    pub fn parse(text: &str, known: &[Setting]) -> Settings {
        let mut values: HashMap<String, bool> = known.iter().map(|s| (s.name.to_string(), s.default)).collect();
        let mut problems = Vec::new();
        for (n, line) in text.lines().enumerate() {
            let line = line.trim();
            if line.is_empty() || line.starts_with(';') || line.starts_with('#') || line.starts_with('[') {
                continue;
            }
            let Some((name, value)) = line.split_once('=') else {
                problems.push(format!("line {}: no '='", n + 1));
                continue;
            };
            let name = name.trim().to_ascii_lowercase();
            let value = match value.trim().to_ascii_lowercase().as_str() {
                "true" | "yes" | "on" | "1" => true,
                "false" | "no" | "off" | "0" => false,
                other => {
                    problems.push(format!("line {}: '{other}' isn't true or false", n + 1));
                    continue;
                }
            };
            match values.get_mut(&name) {
                Some(slot) => *slot = value,
                None => problems.push(format!("line {}: unknown setting '{name}'", n + 1)),
            }
        }
        Settings { values, problems }
    }

    pub fn get(&self, name: &str) -> bool {
        self.values.get(name).copied().unwrap_or(false)
    }

    /// One line per setting, for the log.
    pub fn summary(&self, known: &[Setting]) -> String {
        known.iter().map(|s| format!("{}={}", s.name, self.get(s.name))).collect::<Vec<_>>().join(", ")
    }
}

/// The text of a settings file holding the defaults.
pub fn default_text(title: &str, known: &[Setting]) -> String {
    let mut out = format!("; {title} settings. Each line is a setting = true or false.\r\n");
    for s in known {
        out.push_str(&format!("\r\n; {}\r\n{} = {}\r\n", s.help, s.name, s.default));
    }
    out
}

/// The settings file's path: the DLL's own path with `.ini` in place of `.dll`.
pub fn path_for(dll: &Path) -> PathBuf {
    dll.with_extension("ini")
}

/// Reads the settings, or the defaults if the file is missing or unreadable. Reading only, so it's
/// safe from `DllMain`.
pub fn load(path: &Path, known: &[Setting]) -> Settings {
    match std::fs::read_to_string(path) {
        Ok(text) => Settings::parse(&text, known),
        Err(_) => Settings::parse("", known),
    }
}

/// Writes the defaults if there's no file yet, and says whether it did.
pub fn write_defaults_if_missing(path: &Path, title: &str, known: &[Setting]) -> std::io::Result<bool> {
    if path.exists() {
        return Ok(false);
    }
    std::fs::write(path, default_text(title, known)).map(|_| true)
}

#[cfg(test)]
mod tests {
    use super::*;

    const KNOWN: [Setting; 2] = [
        Setting { name: "skip_logos", default: true, help: "Skip the logos." },
        Setting { name: "block_network", default: true, help: "Block the network." },
    ];

    #[test]
    fn defaults_round_trip_through_the_written_file() {
        let parsed = Settings::parse(&default_text("Test", &KNOWN), &KNOWN);
        assert!(parsed.get("skip_logos") && parsed.get("block_network"));
        assert!(parsed.problems.is_empty());
    }

    #[test]
    fn values_override_defaults_and_mistakes_are_reported_not_fatal() {
        let parsed = Settings::parse("block_network = False\nSKIP_LOGOS=no\nmystery = true\nbroken line\nskip_logos = maybe\n", &KNOWN);
        assert!(!parsed.get("block_network"));
        assert!(!parsed.get("skip_logos"));
        assert_eq!(parsed.problems.len(), 3);
    }
}
