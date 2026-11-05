//! Shared groundwork for DS2 SotFS DLL mods: guarded memory reads, AOB scanning over the game
//! module, and the offsets we rely on.
//!
//! Offsets and signatures come from DS2S-META (MIT, pseudostripy/DS2S-META,
//! `Utils/Offsets/DS2REData.cs`) and target Steam SotFS 1.03 / regulation 2.02 only.

#![cfg(all(windows, target_pointer_width = "64"))]

pub mod game;
pub mod mem;
pub mod module;
pub mod param;
pub mod scan;
mod win;
