//! What we know about DarkSoulsII.exe 1.03 (Steam SotFS, regulation 2.02).
//!
//! Source: DS2S-META `DS2REData.cs`, ANYSOTFS / S103 entries. Pointer chains are written the
//! DS2S-META way: each offset is added and then dereferenced.

use crate::module::{self, Module};
use crate::scan::{self, find_function, rip_relative, Pattern};
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
pub const AOB_SET_WARP_TARGET: &str = "48 89 5C 24 08 48 89 74 24 20 57 48 83 EC 60 0F B7 FA 48 8B F1 48";
/// `WarpFunc(warp_manager*, warp_struct*)`, where `warp_manager` is `*(EventManager + 0x70)`.
pub const AOB_WARP_FUNC: &str = "40 53 48 83 EC 60 8B 02 48 8B D9 89 01 8B 42 04 89 41 04 8B 42 08 89 41 08 8B 42 0C 89 41 0C 8B 42 10 89 41 10 8B 42 14 89 41 14 8B 42 18 89 41 18 8B 42 1C 89 41 1C 8B 42 20 89 41 20 8B 42 24 89 41 24 8B 42 28 89 41 28 8B 42 2C 89 41 2C 8B 42 30 89 41 30 8B 42 34 89 41 34 8B 42 38 89 41 38 8B 42 3C 89 41 3C 48 8B 05";
/// `ApplySpEffect(sp_effect_ctrl*, effect_struct*, _, _, _, f32 = -1.0)` (DS2S-META, S103 entry).
pub const AOB_APPLY_SP_EFFECT: &str = "48 89 6C 24 F8 48 8D 64 24 F8 48 8D 2D 33 A7 0A 00 48 87 2C 24 48";
/// `(*(EventManager + 0x58), u32 bonfire_id)`: Bob's table calls it "Rest at Bonfire" (+0x17DC40 on
/// 1.03). What it does with no bonfire nearby is what the probe is here to find out.
pub const AOB_REST_AT_BONFIRE: &str = "40 53 48 83 EC 20 83 79 38 00 48 8B D9 75 39 85 D2 74 35 48 83 79 30 00 75 2E E8";

/// `ItemGive(item_bag*, item_structs*, u32 count, 0)`, adding items with no on-screen window.
pub const AOB_ITEM_GIVE: &str = "48 89 5C 24 18 56 57 41 56 48 83 EC 30 45 8B F1 41 8B F0 48 8B DA 48";

/// `FmgLookup(fmg*, u32 id) -> *const u16`: finds a string in a loaded text file, null if it has none.
/// Every piece of item text the menus show goes through it.
pub const AOB_FMG_LOOKUP: &str = "48 89 5C 24 08 48 89 7C 24 10 33 DB 44 8B DA 48 8B F9 48 85 C9 75 0D 33 C0 48 8B";
/// `mov rsi, [rip+disp32]` loading the text manager global, in the function that fetches a string by
/// category and id: its text file for category `c` is at `+0x1d8 + c * 0x20`.
pub const AOB_TEXT_MANAGER: &str = "48 8B 35 ? ? ? ? 8B EA 4C 63 F1 48 85 F6";

/// The title flow's first state, `FeSubStateTitleInitBranch`: a byte saying the opening has been shown
/// sends it to the title (state 0x17) instead of the warning and logos, as when returning from a game.
pub const AOB_TITLE_INIT_BRANCH: &str = "33 C0 38 05 ? ? ? ? C6 05 ? ? ? ? 01 0F 95 C0 89 41 10 48";
/// `FeSubStateTitleSteamNetworkCheck`'s enter function, which picks the next state through `+0x10`: 1
/// is the online login, 2 is "not connected to Steam", 3 is a frame rate failure.
pub const AOB_TITLE_NETWORK_CHECK: &str = "48 89 5C 24 08 57 48 83 EC 20 48 8B 05 ? ? ? ? 48 8B D9 48 8B B8 F0 22 00 00";
/// Where that check's result 2 gets its target state, as `mov dword [rax+8], 0x24`.
pub const AOB_TITLE_NOT_CONNECTED_TARGET: &str = "C7 40 08 24 00 00 00 C6 40 0C 01 C7 40 10 FF FF FF FF 48 89 28 C7 40 20 02 00 00 00";

/// The check's result meaning "not connected to Steam", the game's own route to playing offline.
pub const NETWORK_CHECK_NOT_CONNECTED: u32 = 2;
pub const OFF_NETWORK_CHECK_RESULT: usize = 0x10;
/// The title menu. The game's offline route goes through two dialogs and then here.
pub const TITLE_STATE_TOP_MENU: u32 = 0x47;

/// A signature, and where the 1.03 exe has it, for the log when it can't be found.
pub struct Signature {
    pub name: &'static str,
    pub aob: &'static str,
    pub rva: usize,
}

/// Every signature the mod depends on, for `resolve` and the offline scanner.
pub const SIGNATURES: [Signature; 11] = [
    Signature { name: "TitleInitBranch", aob: AOB_TITLE_INIT_BRANCH, rva: 0xfd930 },
    Signature { name: "TitleNetworkCheck", aob: AOB_TITLE_NETWORK_CHECK, rva: 0xf8fb0 },
    Signature { name: "TitleNotConnectedTarget", aob: AOB_TITLE_NOT_CONNECTED_TARGET, rva: 0xf9470 },
    Signature { name: "FmgLookup", aob: AOB_FMG_LOOKUP, rva: 0x503410 },
    Signature { name: "TextManager", aob: AOB_TEXT_MANAGER, rva: 0x503630 },
    Signature { name: "BaseA", aob: AOB_BASE_A, rva: 0x3f5219 },
    Signature { name: "SetWarpTarget", aob: AOB_SET_WARP_TARGET, rva: 0x1843b0 },
    Signature { name: "WarpFunc", aob: AOB_WARP_FUNC, rva: 0x184830 },
    Signature { name: "ApplySpEffect", aob: AOB_APPLY_SP_EFFECT, rva: 0x1b3cbe1 },
    Signature { name: "RestAtBonfire", aob: AOB_REST_AT_BONFIRE, rva: 0x17dc40 },
    Signature { name: "ItemGive", aob: AOB_ITEM_GIVE, rva: 0x1a7470 },
];

/// DS2S-META's `SPECIAL_EFFECT.BONFIREREST`: refills HP and estus without touching enemies. The game
/// itself applies this during a death respawn, so seeing it doesn't mean the player rested.
pub const SP_EFFECT_BONFIRE_REST: i32 = 110_000_010;

/// The item's row in ItemParam. Using an item applies its Effect ID through `ApplySpEffect` after the
/// animation, even when no such effect exists, so the row's Effect ID is set to this same number and
/// the DLL watches for it.
pub const POCKET_BONFIRE_ITEM: u32 = 60_355_010;

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
/// Set in the bonfire manager while a rest menu it opened is up; the entry check of the rest call
/// reads it, and the probe saw it go to 1 on the call and back to 0 by the next one.
pub const OFF_REST_MENU_ACTIVE: usize = 0x38;
/// PlayerCtrl child.
pub const OFF_SP_EFFECT_CTRL: usize = 0x3e0;
/// BaseA child, and the character name within it (DS2S-META `PlayerName`).
pub const OFF_PLAYER_MISC: usize = 0xa8;
pub const OFF_PLAYER_NAME: usize = 0x114;
/// `game_state` while the player is in control.
pub const GAME_STATE_PLAYABLE: u32 = 30;
/// Within PlayerDataMap: three copies of y, z, x, 0x10 apart. Writing all three moves the player.
pub const OFF_STABLE_POS: [usize; 3] = [0x1a0, 0x1b0, 0x1c0];
/// BaseA -> 0xa8 -> 0x10 -> 0x10 is the player's item bag.
pub const CHAIN_ITEM_BAG: [usize; 3] = [0xa8, 0x10, 0x10];
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
/// BaseA child holding the ItemParam resource (DS2S-META `ParamOffsets.txt`: A8;20;D8), and within
/// it, the pointer to the param, laid out the same as the file.
pub const CHAIN_ITEM_PARAM_RES: [usize; 2] = [0xa8, 0x20];
pub const OFF_PARAM_DATA: usize = 0xd8;
/// Byte saying the resource owns that data and must free it. A pointer from elsewhere trips an
/// assertion in the game's allocator when the game closes (`+0xaf6950` skips the free if this is 0).
pub const OFF_PARAM_DATA_OWNED: usize = 0xe8;
/// Within the text manager: ItemName, SimpleExplanation and DetailedExplanation, categories 8 to 10.
pub const OFF_TEXT_ITEM_NAME: usize = 0x2d8;
pub const OFF_TEXT_SIMPLE: usize = 0x2f8;
pub const OFF_TEXT_DETAILED: usize = 0x318;
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
    pub item_give: usize,
    pub fmg_lookup: usize,
    /// Address of the text manager global (dereference it to get the manager).
    pub text_manager_global: usize,
}

/// The quick start's addresses, found separately so it can work when the item's can't, and the
/// other way round.
#[derive(Debug, Clone, Copy)]
pub struct QuickStart {
    pub title_init_branch: usize,
    /// The byte saying the opening has been shown.
    pub opening_shown: usize,
    pub title_network_check: usize,
    pub title_not_connected_target: usize,
}

/// Which of the item's three strings a text file holds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ItemText {
    Name,
    Simple,
    Detailed,
}

#[derive(Debug)]
pub enum ResolveError {
    NoModule,
    WrongVersion { size: usize, timestamp: u32 },
    /// A signature didn't match once. `detail` says what's at its usual place, and if another mod's
    /// hook is there, which DLL it leads to.
    Pattern { name: &'static str, hits: usize, detail: String },
}

impl std::fmt::Display for ResolveError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ResolveError::NoModule => write!(f, "couldn't read the main module"),
            ResolveError::WrongVersion { size, timestamp } => write!(
                f,
                "unsupported DarkSoulsII.exe (SizeOfImage {size:#x}, timestamp {timestamp:#x}); expected {EXPECTED_IMAGE_SIZE:#x} (Steam 1.03)"
            ),
            ResolveError::Pattern { name, hits, detail } => {
                write!(f, "signature {name} matched {hits} times, expected 1 ({detail})")
            }
        }
    }
}

/// The game's exe, if it's the version the mod supports.
pub fn game_module() -> Result<Module, ResolveError> {
    let module = module::main_module().ok_or(ResolveError::NoModule)?;
    if module.size != EXPECTED_IMAGE_SIZE {
        return Err(ResolveError::WrongVersion { size: module.size, timestamp: module.timestamp });
    }
    Ok(module)
}

/// Finds one signature: as a function start, tolerating another mod's hook there, or inside a
/// function, exactly. A failure explains what's at the usual place.
fn find(module: &Module, name: &'static str) -> Result<usize, ResolveError> {
    let sig = SIGNATURES.iter().find(|s| s.name == name).expect("known signature");
    let found = if sig.aob.starts_with('?') || name == "BaseA" || name == "TextManager" || name == "TitleNotConnectedTarget" {
        Pattern::parse(sig.aob).find_unique(module)
    } else {
        find_function(module, sig.aob)
    };
    found.map_err(|hits| ResolveError::Pattern { name, hits, detail: scan::describe(module, sig.rva) })
}

fn target(name: &'static str, hit: usize, disp: usize, len: usize) -> Result<usize, ResolveError> {
    rip_relative(hit, disp, len).ok_or(ResolveError::Pattern { name, hits: 0, detail: "unreadable operand".into() })
}

pub fn resolve(module: &Module) -> Result<Addresses, ResolveError> {
    let module = *module;
    Ok(Addresses {
        module,
        base_a_global: target("BaseA", find(&module, "BaseA")?, 3, 7)?,
        set_warp_target: find(&module, "SetWarpTarget")?,
        warp_func: find(&module, "WarpFunc")?,
        apply_sp_effect: find(&module, "ApplySpEffect")?,
        rest_at_bonfire: find(&module, "RestAtBonfire")?,
        item_give: find(&module, "ItemGive")?,
        fmg_lookup: find(&module, "FmgLookup")?,
        text_manager_global: target("TextManager", find(&module, "TextManager")?, 3, 7)?,
    })
}

pub fn resolve_quick_start(module: &Module) -> Result<QuickStart, ResolveError> {
    let title_init_branch = find(module, "TitleInitBranch")?;
    Ok(QuickStart {
        title_init_branch,
        // From the `mov byte [rip+x], 1` at +8 rather than the read at +2, which a hook would cover.
        opening_shown: target("TitleInitBranch", title_init_branch + 8, 2, 7)?,
        title_network_check: find(module, "TitleNetworkCheck")?,
        title_not_connected_target: find(module, "TitleNotConnectedTarget")? + 3,
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

    /// The resource holding ItemParam, once the game has loaded it.
    fn item_param_res(&self) -> Option<usize> {
        self.base_a().and_then(|b| mem::chain(b, &CHAIN_ITEM_PARAM_RES))
    }

    /// The ItemParam the game is using right now.
    pub fn item_param(&self) -> Option<usize> {
        mem::ptr(self.item_param_res()? + OFF_PARAM_DATA)
    }

    /// Points the game at `data` as its ItemParam. The game won't try to free it, or the old copy.
    pub fn set_item_param(&self, data: usize) -> bool {
        let Some(res) = self.item_param_res() else { return false };
        mem::write::<u8>(res + OFF_PARAM_DATA_OWNED, 0) && mem::write(res + OFF_PARAM_DATA, data)
    }

    /// Which item string a loaded text file is for, if any.
    pub fn item_text_kind(&self, fmg: usize) -> Option<ItemText> {
        let manager = mem::ptr(self.text_manager_global)?;
        [(OFF_TEXT_ITEM_NAME, ItemText::Name), (OFF_TEXT_SIMPLE, ItemText::Simple), (OFF_TEXT_DETAILED, ItemText::Detailed)]
            .into_iter()
            .find(|&(off, _)| mem::ptr(manager + off) == Some(fmg))
            .map(|(_, kind)| kind)
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

    /// The character's name as raw UTF-16, up to the terminator. Saved state is kept per character.
    pub fn character_name(&self) -> Option<Vec<u16>> {
        let bytes = mem::read_bytes(mem::ptr(self.base_a()? + OFF_PLAYER_MISC)? + OFF_PLAYER_NAME, 32)?;
        let units: Vec<u16> =
            bytes.as_chunks::<2>().0.iter().map(|b| u16::from_le_bytes(*b)).take_while(|&u| u != 0).collect();
        (!units.is_empty()).then_some(units)
    }

    /// Whether a rest menu opened by `rest_at_bonfire` is still open.
    pub fn rest_menu_open(&self) -> bool {
        self.event_manager()
            .and_then(|em| mem::ptr(em + OFF_BONFIRE_MANAGER))
            .and_then(|manager| mem::read::<u32>(manager + OFF_REST_MENU_ACTIVE))
            .is_some_and(|active| active != 0)
    }

    /// The player's SpEffectCtrl, the first argument of `ApplySpEffect` for effects on the player.
    pub fn sp_effect_ctrl(&self) -> Option<usize> {
        mem::ptr(self.player_ctrl()? + OFF_SP_EFFECT_CTRL)
    }

    /// Adds `amount` of an item to the inventory, silently. Returns false if the bag isn't available.
    ///
    /// # Safety
    /// Calls into the game. The player must be in control, and `id` must be a row in ItemParam.
    pub unsafe fn give_item(&self, id: u32, amount: u16) -> bool {
        #[repr(C, align(16))]
        struct ItemStruct {
            kind: i32,
            id: u32,
            durability: f32,
            amount: u16,
            upgrade: u8,
            infusion: u8,
        }
        type Give = unsafe extern "system" fn(usize, *const ItemStruct, u32, u32) -> u64;

        let Some(bag) = self.base_a().and_then(|b| mem::chain(b, &CHAIN_ITEM_BAG)) else { return false };
        let item = ItemStruct { kind: 3, id, durability: f32::MAX, amount, upgrade: 0, infusion: 0 };
        let give: Give = std::mem::transmute(self.item_give);
        give(bag, &item, 1, 0);
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
