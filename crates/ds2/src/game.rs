//! What we know about DarkSoulsII.exe 1.03 (Steam SotFS, regulation 2.02).
//!
//! Source: DS2S-META `DS2REData.cs`, ANYSOTFS / S103 entries. Pointer chains are written the
//! DS2S-META way: each offset is added and then dereferenced.

use crate::module::{self, Module};
use crate::scan::{rip_relative, Pattern};
use crate::mem;

/// `SizeOfImage` of the only exe we support. Anything else and the mod switches itself off.
pub const EXPECTED_IMAGE_SIZE: usize = 0x1D7_6000;
/// PE timestamp of that exe, logged for reference.
pub const EXPECTED_TIMESTAMP: u32 = 0x6322_EF1D;

/// `mov rax, [rip+disp32]` loading the GameManagerImp ("BaseA") global.
pub const AOB_BASE_A: &str = "48 8B 05 ? ? ? ? 48 8B 58 38 48 85 DB 74 ? F6";
/// `SetWarpTarget(out_struct*, u16 bonfire_id, i32 mode)`: returns the struct. Mode 2 is the
/// default; writing 3 to `[struct]` afterwards means "warp to that bonfire".
pub const AOB_SET_WARP_TARGET: &str = "48 89 5C 24 08 48 89 74 24 20 57 48 83 EC 60 0F B7 FA";
/// `WarpFunc(warp_manager*, warp_struct*)`, where `warp_manager` is `EventManager + 0x70` (not dereferenced).
pub const AOB_WARP_FUNC: &str = "40 53 48 83 EC 60 8B 02 48 8B D9 89 01 8B 42 04";

/// BaseA children.
pub const OFF_PLAYER_CTRL: usize = 0xd0;
pub const OFF_EVENT_MANAGER: usize = 0x70;
/// PlayerCtrl -> 0xf8 -> 0xf0 is the position block.
pub const CHAIN_POSITION: [usize; 2] = [0xf8, 0xf0];
/// PlayerCtrl -> 0x100 -> 0x320 -> 0x20: holds the three "stable" warp positions at 0x1a0..0x1c8.
pub const CHAIN_PLAYER_DATA_MAP: [usize; 3] = [0x100, 0x320, 0x20];
/// Within the position block: y, z, x, then angles y, z, x (DS2S-META's naming).
pub const POS_FIELDS: usize = 0x20;
pub const OFF_LAST_BONFIRE_AREA: usize = 0x164;
pub const OFF_LAST_BONFIRE: usize = 0x16c;
/// BaseA -> 0x80 -> 0x8, + 0xbb4.
pub const CHAIN_LOADING: [usize; 2] = [0x80, 0x8];
pub const OFF_LOADING: usize = 0xbb4;
pub const OFF_GAME_STATE: usize = 0x24ac;

#[derive(Debug, Clone, Copy)]
pub struct Addresses {
    pub module: Module,
    /// Address of the BaseA global (dereference it to get BaseA).
    pub base_a_global: usize,
    pub set_warp_target: usize,
    pub warp_func: usize,
}

#[derive(Debug)]
pub enum ResolveError {
    NoModule,
    WrongVersion { size: usize, timestamp: u32 },
    Pattern { name: &'static str, hits: usize },
}

impl std::fmt::Display for ResolveError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ResolveError::NoModule => write!(f, "couldn't read the main module"),
            ResolveError::WrongVersion { size, timestamp } => write!(
                f,
                "unsupported DarkSoulsII.exe (SizeOfImage {size:#x}, timestamp {timestamp:#x}); expected {EXPECTED_IMAGE_SIZE:#x} (Steam 1.03)"
            ),
            ResolveError::Pattern { name, hits } => write!(f, "signature {name} matched {hits} times, expected 1"),
        }
    }
}

pub fn resolve() -> Result<Addresses, ResolveError> {
    let module = module::main_module().ok_or(ResolveError::NoModule)?;
    if module.size != EXPECTED_IMAGE_SIZE {
        return Err(ResolveError::WrongVersion { size: module.size, timestamp: module.timestamp });
    }
    let find = |name: &'static str, aob: &str| {
        Pattern::parse(aob).find_unique(&module).map_err(|hits| ResolveError::Pattern { name, hits })
    };
    let base_hit = find("BaseA", AOB_BASE_A)?;
    Ok(Addresses {
        module,
        base_a_global: rip_relative(base_hit, 3, 7).ok_or(ResolveError::Pattern { name: "BaseA", hits: 0 })?,
        set_warp_target: find("SetWarpTarget", AOB_SET_WARP_TARGET)?,
        warp_func: find("WarpFunc", AOB_WARP_FUNC)?,
    })
}

/// A point-in-time read of the state we care about. Fields are `None` when unavailable (menus, loads).
#[derive(Debug, Default, Clone, Copy, PartialEq)]
pub struct Snapshot {
    /// Raw y, z, x, ang_y, ang_z, ang_x.
    pub pos: Option<[f32; 6]>,
    pub last_bonfire: Option<u16>,
    pub last_bonfire_area: Option<u32>,
    pub loading: Option<u8>,
    pub game_state: Option<u32>,
}

impl Addresses {
    pub fn base_a(&self) -> Option<usize> {
        mem::ptr(self.base_a_global)
    }
    pub fn player_ctrl(&self) -> Option<usize> {
        mem::ptr(self.base_a()? + OFF_PLAYER_CTRL)
    }
    pub fn event_manager(&self) -> Option<usize> {
        mem::ptr(self.base_a()? + OFF_EVENT_MANAGER)
    }
    pub fn player_data_map(&self) -> Option<usize> {
        mem::chain(self.player_ctrl()?, &CHAIN_PLAYER_DATA_MAP)
    }

    pub fn snapshot(&self) -> Snapshot {
        let base = self.base_a();
        let em = self.event_manager();
        Snapshot {
            pos: self
                .player_ctrl()
                .and_then(|pc| mem::chain(pc, &CHAIN_POSITION))
                .and_then(|p| mem::read::<[f32; 6]>(p + POS_FIELDS)),
            last_bonfire: em.and_then(|e| mem::read::<u16>(e + OFF_LAST_BONFIRE)),
            last_bonfire_area: em.and_then(|e| mem::read::<u32>(e + OFF_LAST_BONFIRE_AREA)),
            loading: base.and_then(|b| mem::chain(b, &CHAIN_LOADING)).and_then(|l| mem::read::<u8>(l + OFF_LOADING)),
            game_state: base.and_then(|b| mem::read::<u32>(b + OFF_GAME_STATE)),
        }
    }
}
