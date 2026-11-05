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
/// `SetWarpTarget(out_struct*, u16 bonfire_id, i32 mode)`: returns the struct. The game passes mode 2.
///
/// Warp struct, as logged from real Travel warps: +00 kind (3 = bonfire), +04 and +30 mode,
/// +08 map id as 0xAABBCCDD = mAA_BB_CC_DD, +18 bonfire id. There are no coordinates in it: the game
/// resolves the destination from map data, so a warp can only go to a real bonfire.
pub const AOB_SET_WARP_TARGET: &str = "48 89 5C 24 08 48 89 74 24 20 57 48 83 EC 60 0F B7 FA";
/// `WarpFunc(warp_manager*, warp_struct*)`, where `warp_manager` is `*(EventManager + 0x70)`.
pub const AOB_WARP_FUNC: &str = "40 53 48 83 EC 60 8B 02 48 8B D9 89 01 8B 42 04";
/// `ApplySpEffect(sp_effect_ctrl*, effect_struct*, _, _, _, f32 = -1.0)` (DS2S-META, S103 entry).
pub const AOB_APPLY_SP_EFFECT: &str = "48 89 6C 24 F8 48 8D 64 24 F8 48 8D 2D 33 A7 0A 00";
/// `(*(EventManager + 0x58), u32 bonfire_id)`: Bob's table calls it "Rest at Bonfire" (+0x17DC40 on
/// 1.03). What it does with no bonfire nearby is what the probe is here to find out.
pub const AOB_REST_AT_BONFIRE: &str = "40 53 48 83 EC 20 83 79 38 00 48 8B D9 75 39 85 D2 74 35 48 83 79 30 00 75 2E E8";

/// Every signature the mod depends on, for `resolve` and the offline scanner.
pub const SIGNATURES: [(&str, &str); 5] = [
    ("BaseA", AOB_BASE_A),
    ("SetWarpTarget", AOB_SET_WARP_TARGET),
    ("WarpFunc", AOB_WARP_FUNC),
    ("ApplySpEffect", AOB_APPLY_SP_EFFECT),
    ("RestAtBonfire", AOB_REST_AT_BONFIRE),
];

/// DS2S-META's `SPECIAL_EFFECT.BONFIREREST`: refills HP and estus without touching enemies.
pub const SP_EFFECT_BONFIRE_REST: i32 = 110_000_010;

/// Within the warp struct.
pub const OFF_WARP_MAP_ID: usize = 0x08;
/// BaseA child and field of the two values that follow the player's map. Found by probe, not from
/// DS2S-META: they were right after 6 of 6 warps and through 2 walked boundaries, and unlike the
/// other candidates they never read as junk during a load or lag behind an arrival.
pub const CURRENT_MAP_FIELDS: [(usize, usize); 2] = [(0x30, 0x360), (0x78, 0x580)];

/// BaseA children.
pub const OFF_PLAYER_CTRL: usize = 0xd0;
pub const OFF_EVENT_MANAGER: usize = 0x70;
/// EventManager children.
pub const OFF_WARP_MANAGER: usize = 0x70;
pub const OFF_BONFIRE_MANAGER: usize = 0x58;
/// PlayerCtrl child.
pub const OFF_SP_EFFECT_CTRL: usize = 0x3e0;
/// `game_state` while the player is in control.
pub const GAME_STATE_PLAYABLE: u32 = 30;
/// Within PlayerDataMap: three copies of y, z, x, 0x10 apart. Writing all three moves the player.
pub const OFF_STABLE_POS: [usize; 3] = [0x1a0, 0x1b0, 0x1c0];
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
    pub apply_sp_effect: usize,
    pub rest_at_bonfire: usize,
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
        apply_sp_effect: find("ApplySpEffect", AOB_APPLY_SP_EFFECT)?,
        rest_at_bonfire: find("RestAtBonfire", AOB_REST_AT_BONFIRE)?,
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

    /// The map the player is standing in, as 0xAABBCCDD = mAA_BB_CC_DD. `None` if the two fields that
    /// track it disagree, so a caller never picks a host bonfire on a doubtful answer.
    pub fn current_map(&self) -> Option<u32> {
        let base = self.base_a()?;
        let read = |(child, field): (usize, usize)| mem::read::<u32>(mem::ptr(base + child)? + field);
        let (a, b) = (read(CURRENT_MAP_FIELDS[0])?, read(CURRENT_MAP_FIELDS[1])?);
        (a == b).then_some(a)
    }

    pub fn warp_manager(&self) -> Option<usize> {
        mem::ptr(self.event_manager()? + OFF_WARP_MANAGER)
    }

    /// True when the player is in control and nothing is loading.
    pub fn playable(&self) -> bool {
        let s = self.snapshot();
        s.game_state == Some(GAME_STATE_PLAYABLE) && s.loading == Some(0) && s.pos.is_some()
    }

    /// Moves the player within the loaded map by writing the three "stable" position copies.
    /// `pos` is y, z, x, the same order as `Snapshot::pos`.
    pub fn set_position(&self, pos: [f32; 3]) -> bool {
        let Some(map) = self.player_data_map() else { return false };
        OFF_STABLE_POS.iter().all(|off| mem::write(map + off, pos))
    }

    /// Warps to a lit bonfire, the way the Travel menu does: the same two calls with the same mode.
    ///
    /// # Safety
    /// Calls into the game. `manager` must be the live warp manager, and the player must be in control.
    ///
    /// Returns the bonfire's map id, from the warp struct. Unlike Travel this leaves LastBonfire
    /// alone, so follow it with `set_last_bonfire` if death should bring the player back here.
    pub unsafe fn warp_to_bonfire(&self, manager: usize, bonfire_id: u16) -> u32 {
        #[repr(align(16))]
        struct WarpStruct([u8; 0x200]);
        type SetTarget = unsafe extern "system" fn(*mut u8, u64, u64, u64) -> *mut u8;
        type Warp = unsafe extern "system" fn(usize, *mut u8) -> u64;

        let mut warp = WarpStruct([0; 0x200]);
        let set_target: SetTarget = std::mem::transmute(self.set_warp_target);
        let do_warp: Warp = std::mem::transmute(self.warp_func);
        let filled = set_target(warp.0.as_mut_ptr(), bonfire_id as u64, 2, 0);
        // Kind 3 = "warp to the bonfire"; anything else lands at the area's default start.
        *(filled as *mut u32) = 3;
        let map_id = *(filled.add(OFF_WARP_MAP_ID) as *const u32);
        do_warp(manager, filled);
        map_id
    }

    /// Sets where the player respawns: the fields the Travel menu updates when it warps.
    pub fn set_last_bonfire(&self, bonfire_id: u16, map_id: u32) -> bool {
        let Some(em) = self.event_manager() else { return false };
        mem::write(em + OFF_LAST_BONFIRE, bonfire_id) && mem::write(em + OFF_LAST_BONFIRE_AREA, map_id)
    }

    /// Applies a SpEffect to the player. Returns false if the player isn't available.
    ///
    /// # Safety
    /// Calls into the game. The player must be in control.
    pub unsafe fn apply_sp_effect(&self, id: i32) -> bool {
        #[repr(C, align(16))]
        struct Effect {
            id: i32,
            one: i32,
            _pad: i32,
            flags: i32,
            _rest: [u8; 0x10],
        }
        type Apply = unsafe extern "system" fn(usize, *const Effect, u64, u64, u64, f32) -> u64;

        let Some(ctrl) = self.player_ctrl().and_then(|pc| mem::ptr(pc + OFF_SP_EFFECT_CTRL)) else { return false };
        let effect = Effect { id, one: 1, _pad: 0, flags: 0x219, _rest: [0; 0x10] };
        let apply: Apply = std::mem::transmute(self.apply_sp_effect);
        apply(ctrl, &effect, 0, 0, 0, -1.0);
        true
    }

    /// Bob's table's remote "Rest at Bonfire" call. Returns false if the manager isn't available.
    ///
    /// # Safety
    /// Calls into the game. The player must be in control.
    pub unsafe fn rest_at_bonfire(&self, bonfire_id: u16) -> bool {
        type Rest = unsafe extern "system" fn(usize, u32) -> u64;
        let Some(manager) = self.event_manager().and_then(|em| mem::ptr(em + OFF_BONFIRE_MANAGER)) else { return false };
        let rest: Rest = std::mem::transmute(self.rest_at_bonfire);
        rest(manager, bonfire_id as u32);
        true
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
