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
| Respawn point | DLL | Resting sets the game's LastBonfire to the host (`EventManager+0x16c/+0x164`) and marks the pocket as the respawn point. After a death-reload at the host, the DLL moves you to the saved position. |
| Limits | DLL | Blocks placement while a boss battle is active or enemies are aggroed. The flags behind that still need reverse engineering. |
| Visible bonfire | later | No runtime object-spawn function is known. Options, cheapest first: a long-lived stationary bullet carrying the bonfire-flame SFX; a character SFX; spawning an MSB object (real reverse engineering). |
| Persistence | DLL | `Game\dllMods\pocket_bonfire\<steamid>-<slot>.json`, so the bonfire survives quitting the game. |

Known building blocks, from DS2S-META (MIT, `Utils/Offsets/DS2REData.cs`, `Resources/Assembly/BonfireWarp64.txt`):
- GameManagerImp AOB `48 8B 05 ? ? ? ? 48 8B 58 38 48 85 DB 74 ? F6`
- `SetWarpTarget` AOB `48 89 5C 24 08 48 89 74 24 20 57 48 83 EC 60 0F B7 FA`, called as
  `(struct*, u16 bonfireId, 2)`. Writing `[struct]=3` then means "warp to bonfire".
- `WarpFunc` AOB `40 53 48 83 EC 60 8B 02 48 8B D9 89 01 8B 42 04`, called as `(WarpManager = EventManager+0x70, struct*)`
- Player position `PlayerCtrl+0xf8+0xf0` (+0x20/0x24/0x28). To move the player, write the "stable" warp position, all three copies at `+0x1a0..0x1c8`.
- SpEffectCtrl is at `PlayerCtrl+0x3e0`. The BONFIREREST SpEffect refills you after a warp.
- Bonfire IDs are the `MapObjectBonfireParam` row IDs (4650 = Majula).

Risks, highest first:
1. The Travel list may reject a row that has no MSB bonfire behind it. Fallback: a "Pocket Bonfire" entry in the bonfire menu instead, the way Bonfire Level Up swaps out an entry.
2. The warp may read its destination from map data after our hook runs. The fallback, warping first and then moving the player, is already the plan.
3. Resting by calling the menu with no bonfire object nearby could soft-lock. Fallback: apply BONFIREREST and set the respawn point without the menu.
4. Steam patches. Everything is resolved by AOB, and the DLL switches itself off when the exe isn't the known version (module size `0x1D76000`).

## Build plan

The mod will be written in Rust: a `cdylib` using the `windows` and `minhook` crates, loaded by Lazy Loader. That's the same language as Mutant, and it needs no C++ toolchain.

0. **Dev install.** Make a UXM-unpacked copy of the game for reading params, FMGs and MSBs, so the play install stays packed.
1. **Probe DLL (reverse-engineering spike).** Hook `SetWarpTarget` and `WarpFunc` and log their arguments and struct bytes, plus area and position, to a file. You do a few normal Travel warps; from the log we learn the struct layout and how area IDs map to bonfires.
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
