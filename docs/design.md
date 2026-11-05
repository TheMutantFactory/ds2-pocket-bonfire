# Pocket Bonfire: design

## Decisions (2026-10-27)

| Question | Decision |
|---|---|
| Item | **Reusable key item.** Never consumed. Using it moves your single pocket bonfire to where you stand. |
| Rest | **Full bonfire.** Estus and HP refill, enemies respawn, it becomes your respawn point, and Travel is available from it. |
| Warp | It appears in the Travel list of every real bonfire, and you can travel from it to any lit bonfire. |
| Limits | It can't be placed during a boss fight, inside a boss fog or arena, or while enemies are aggroed. |
| Uniqueness | Placing one removes every other pocket bonfire. There is at most one per character. |
| Repo | Standalone and MIT-licensed. The Mutant mod manager installs it like any other mod. |

## Open questions

- Does the item work in NG+ / Bonfire Intensity? (Proposal: a pocket bonfire has no intensity and
  can't take Bonfire Ascetics.)
- Where does the player get the item? (Proposal: you start with it, or it's sold by the Emerald Herald.)
- Should it persist across quitting the game? (Proposal: yes. Store it per save slot, next to the save
  file, so it survives quitting and reloading.)
- What should happen online? (Proposal: offline only, matching every other DLL mod in the stack.)

## Architecture

The core idea is that **a pocket bonfire is a position override attached to a real "host" bonfire.**
DS2 can't create a bonfire at runtime, but it can warp to a bonfire by its ID, and it lets you set the
player's position after a load. A pocket bonfire is therefore a saved `{area, host bonfire id, xyz,
facing}`. Every time the game would take you to the pocket bonfire, it warps to the host bonfire in
the same area and then moves you to the saved position.

| Piece | Where it lives | How |
|---|---|---|
| The item | loose params: `ItemParam`, `ItemUsageParam`, `SpEffectParam` | A new reusable key item whose use applies a custom SpEffect. On its own the item does nothing; the DLL watches for that SpEffect. |
| Item name, description, icon | FMG + icon via Mod Engine `mod\menu\…` | Mod Engine overrides are known to work for DS2 menu text (Nexus 1633 ships FMGs this way). The exact file names need a UXM-unpacked copy to confirm. |
| Place | DLL | When the SpEffect turns up on the player and the location is allowed: read the area and position, choose the host bonfire, save it. |
| Rest | DLL | Using the item **while standing at your own pocket bonfire** rests there instead of moving it. Same item, two meanings. Resting opens the real bonfire menu for the host ID (Bob's table calls `+17DC40` with a bonfire id). |
| Travel to it | params + DLL | A new `MapObjectBonfireParam` row, "Pocket Bonfire", shown in the Travel list (Firelink Edition, Nexus 1509, proves new rows can appear). A hook on `SetWarpTarget` catches that ID and swaps in the host ID, and after the load the DLL writes the saved position. |
| Travel from it | free | Resting opens the host's real bonfire menu, so Travel just works. |
| Respawn point | DLL | Resting sets the game's LastBonfire to the host (`EventManager+0x16c/+0x164`) and marks the pocket as the respawn point. Death reloads make no `WarpFunc` call, so the DLL watches `game_state`: a reload with no warp, ending at the host, means move the player to the saved position. |
| Limits | DLL | Blocks placement while a boss battle is active or enemies are aggroed. The flags behind that still need reverse engineering. |
| Visible bonfire | later | No runtime object-spawn function is known. Options, cheapest first: a long-lived stationary bullet carrying the bonfire-flame SFX; a character SFX; spawning an MSB object (real reverse engineering). |
| Persistence | DLL | `Game\dllMods\pocket_bonfire\<steamid>-<slot>.json`, so the bonfire survives quitting the game. |

Known building blocks, from DS2S-META (MIT, `Utils/Offsets/DS2REData.cs`, `Resources/Assembly/BonfireWarp64.txt`):
- GameManagerImp AOB `48 8B 05 ? ? ? ? 48 8B 58 38 48 85 DB 74 ? F6`
- `SetWarpTarget` AOB `48 89 5C 24 08 48 89 74 24 20 57 48 83 EC 60 0F B7 FA`, called as
  `(struct*, u16 bonfireId, 2)`. Writing `[struct]=3` then means "warp to bonfire".
- `WarpFunc` AOB `40 53 48 83 EC 60 8B 02 48 8B D9 89 01 8B 42 04`, called as `(*(EventManager+0x70), struct*)`
- Player position `PlayerCtrl+0xf8+0xf0` (+0x20/0x24/0x28). To move the player, write the "stable" warp position, all three copies at `+0x1a0..0x1c8`.
- SpEffectCtrl is at `PlayerCtrl+0x3e0`. The BONFIREREST SpEffect refills you after a warp.
- Bonfire IDs are the `MapObjectBonfireParam` row IDs (4650 = Majula).
- `ApplySpEffect` AOB `48 89 6C 24 F8 48 8D 64 24 F8 48 8D 2D 33 A7 0A 00`, called as `(SpEffectCtrl, struct*, _, _, _, -1.0f)`. Rest effect ID: 110000010.
- Remote rest (Bob's table; no license file, so facts only, no code): `+0x17DC40`, called as `(*(EventManager+0x58), bonfireId)`.

Observed in play (probe 0.1, 5 sessions, 33 warps):
- **Warp struct:** +00 kind (3 = bonfire; 4 = a non-bonfire map transition with no `SetWarpTarget` call and a location ID at +18), +04 and +30 mode (2), +08 map ID as `0xAABBCCDD` = `mAA_BB_CC_DD`, +18 bonfire ID. **It has no coordinates,** so rewriting the destination in the struct is out.
- **Bonfire IDs start with their map:** 4650 is m10_04, 16670 is m10_16, 31655 is m10_31.
- **Travel** updates LastBonfire within about 70 ms of `WarpFunc`, before the load starts.
- **Reloads without a warp:** 65 of them, probably mostly deaths (not individually confirmed), make no `WarpFunc` call and land on the bonfire's fixed arrival position.
- **`game_state`:** 30 is playable, 31 is leaving, then a mix of 18-21, 24 and 25, then 19, 28, 30. Position is readable from state 19 onward.

Risks, highest first:
1. The Travel list may reject a row that has no MSB bonfire behind it. Fallback: a "Pocket Bonfire" entry in the bonfire menu instead, the way Bonfire Level Up swaps out an entry.
2. **Choosing the host.** The host has to be in the map you're standing in, but we don't yet know where the game keeps the current map. LastBonfire isn't it: a kind 4 transition takes you into another map without changing it. The probe dumps `WarpManager` and nearby regions on Ctrl+F5 and F9 to look for it. Fallback: a table of map bounds.
3. Moving the player right after a load may land before the ground has loaded at the target. The probe's Ctrl+F7 test watches for this.
4. Resting by calling the menu with no bonfire object nearby could soft-lock. Fallback: apply BONFIREREST and set the respawn point without the menu.
5. Steam patches. Everything is resolved by AOB, and the DLL switches itself off when the exe isn't the known version (module size `0x1D76000`).

## Build plan

The mod is written in Rust: a `cdylib` using the `minhook` crate, loaded by Lazy Loader. That's the same language as Mutant, and it needs no C++ toolchain beyond the MSVC compiler the crate builds MinHook with.

0. **Dev install.** Make a UXM-unpacked copy of the game for reading params, FMGs and MSBs, so the play install stays packed.
1. **Probe DLL (reverse-engineering spike).** 0.1 logged real warps (results above). 0.2 adds the write tests in `crates/probe/src/test_keys.rs`: save a spot, move to it, warp to the host and then move, the rest effect, the remote rest call, and moving after a death.
2. **Item.** Add the param rows and text, and confirm the item shows up, can be used, and applies the SpEffect.
3. **Place and persist.** On the SpEffect, save the location to the per-slot json.
4. **Travel to it.** Add the bonfire row, hook the warp, and reposition after the load.
5. **Rest and respawn.**
6. **Limits.**
7. **Visible marker.**
8. **Package** for Mutant and Nexus: dllMods DLL, loose params, `mod\` text.

## Target environment

- Steam DS2 SotFS, current version, DirectX 11, 64-bit.
- Lazy Loader in `Game\dinput8.dll`. The mod's DLL goes in `Game\dllMods\`.
- Load Loose Param Files, so param changes ship as individual `Game\Param\*.param` files.
- Mod Engine (`mod\` overrides) for item text and any other archived files.
