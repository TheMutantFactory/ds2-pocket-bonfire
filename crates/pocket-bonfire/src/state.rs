//! The saved pocket bonfire: one small text file per character, next to the DLL.

use std::fmt::Write as _;
use std::path::PathBuf;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Bonfire {
    pub map: u32,
    pub host: u16,
    /// y, z, x, the same order as `Snapshot::pos`.
    pub pos: [f32; 3],
    /// The player has rested here, so dying brings them back here.
    pub respawn_here: bool,
}

impl Bonfire {
    /// Distance from a `Snapshot::pos`, which carries angles after the coordinates.
    pub fn distance_to(&self, here: [f32; 6]) -> f32 {
        (0..3).map(|i| (self.pos[i] - here[i]).powi(2)).sum::<f32>().sqrt()
    }

    fn to_text(self) -> String {
        format!(
            "map={:#x}\nhost={}\ny={}\nz={}\nx={}\nrespawn_here={}\n",
            self.map, self.host, self.pos[0], self.pos[1], self.pos[2], self.respawn_here
        )
    }

    fn from_text(text: &str) -> Option<Bonfire> {
        let field = |key: &str| {
            text.lines().find_map(|line| line.strip_prefix(key)?.strip_prefix('='))
        };
        Some(Bonfire {
            map: u32::from_str_radix(field("map")?.strip_prefix("0x")?, 16).ok()?,
            host: field("host")?.parse().ok()?,
            pos: [field("y")?.parse().ok()?, field("z")?.parse().ok()?, field("x")?.parse().ok()?],
            respawn_here: field("respawn_here")?.parse().ok()?,
        })
    }
}

pub struct Store {
    dir: PathBuf,
}

impl Store {
    pub fn new(dir: PathBuf) -> Store {
        Store { dir }
    }

    /// Names can hold characters a file name can't, so the file is named by the name's hex.
    fn path(&self, character: &[u16]) -> PathBuf {
        let mut name = String::new();
        for unit in character {
            let _ = write!(name, "{unit:04x}");
        }
        self.dir.join(format!("{name}.txt"))
    }

    pub fn load(&self, character: &[u16]) -> Option<Bonfire> {
        Bonfire::from_text(&std::fs::read_to_string(self.path(character)).ok()?)
    }

    pub fn save(&self, character: &[u16], bonfire: &Bonfire) -> std::io::Result<()> {
        std::fs::create_dir_all(&self.dir)?;
        std::fs::write(self.path(character), bonfire.to_text())
    }

    /// Whether this character has already been given the item.
    pub fn item_given(&self, character: &[u16]) -> bool {
        self.path(character).with_extension("item").exists()
    }

    pub fn mark_item_given(&self, character: &[u16]) -> std::io::Result<()> {
        std::fs::create_dir_all(&self.dir)?;
        std::fs::write(self.path(character).with_extension("item"), "")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Bonfire {
        Bonfire { map: 0xa170000, host: 23660, pos: [-343.464, 37.098, 90.577], respawn_here: true }
    }

    #[test]
    fn text_round_trips_exactly() {
        assert_eq!(Bonfire::from_text(&sample().to_text()), Some(sample()));
    }

    #[test]
    fn damaged_text_is_no_bonfire() {
        assert_eq!(Bonfire::from_text("map=0xa170000\nhost=not a number\n"), None);
        assert_eq!(Bonfire::from_text(""), None);
    }

    #[test]
    fn distance_ignores_the_angles() {
        let b = Bonfire { pos: [0.0, 0.0, 0.0], ..sample() };
        assert_eq!(b.distance_to([3.0, 4.0, 0.0, 9.0, 9.0, 9.0]), 5.0);
    }

    #[test]
    fn store_round_trips_and_keeps_characters_apart() {
        let dir = std::env::temp_dir().join(format!("pocket_bonfire_test_{}", std::process::id()));
        let store = Store::new(dir.clone());
        let (a, b): (Vec<u16>, Vec<u16>) = ("Ana".encode_utf16().collect(), "Bo/b".encode_utf16().collect());
        store.save(&a, &sample()).unwrap();
        assert_eq!(store.load(&a), Some(sample()));
        assert_eq!(store.load(&b), None);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn item_marker_is_per_character_and_leaves_the_bonfire_alone() {
        let dir = std::env::temp_dir().join(format!("pocket_bonfire_item_test_{}", std::process::id()));
        let store = Store::new(dir.clone());
        let (a, b): (Vec<u16>, Vec<u16>) = ("Ana".encode_utf16().collect(), "Bo".encode_utf16().collect());
        assert!(!store.item_given(&a));
        store.mark_item_given(&a).unwrap();
        assert!(store.item_given(&a));
        assert!(!store.item_given(&b));
        assert_eq!(store.load(&a), None);
        store.save(&a, &sample()).unwrap();
        assert_eq!(store.load(&a), Some(sample()));
        assert!(store.item_given(&a));
        std::fs::remove_dir_all(dir).unwrap();
    }
}
