# Pocket Bonfire: design

## Status (0.10.0)

Released: placing, resting and respawning. Not built: Travel to it, the placement limits and a visible
marker, so the decisions below are partly still plans. Where the sections below describe loose params,
Mod Engine or text files, the release differs:

- **Item:** `keep_item_row` points the game at a copy of ItemParam with the row added: the pointer is at
  `*(*(BaseA+0xa8)+0x20)+0xd8`. Clearing the "owned" byte at `+0xe8` stops the game's allocator asserting on
  the copy when the game closes.
- **Text:** `FmgLookup` (`+0x503410`) is hooked for the item's id. The text manager global is `+0x1616cb0`,
  and categories 8, 9 and 10 (ItemName, SimpleExplanation, DetailedExplanation) are at `+0x2d8`, `+0x2f8` and
  `+0x318`. Text a file really has wins.
- **Offline:** `WSAStartup` is refused from `DllMain`, the approach Mod Engine uses.
- **Title flow:** states are numbered, with `FeSubState*` classes. `FeSubStateTitleInitBranch` (`+0xfd930`) goes to
  the title instead of the warning and logos if the byte it reads is set. `FeSubStateTitleSteamNetworkCheck`
  (state 0x20, `+0xf8fb0`) is forced to "not connected", whose target changes from the dialogs (0x24, then
  0x2a) to the title menu (0x47). `FeSubStateTitleOnlineCheck` exists too, and hasn't caused a wait in play.
- **Packaging:** one zip with the DLL and Lazy Loader, and no `lazyLoad.ini`: Lazy Loader works without one.

A crash when the game closes, at `+0xa5b3d6`, predates this mod (seen from 2026-10-20) and has no mod DLL on
its stack. It happens after quitting and its cause is unknown.

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
- Where does the player get the item? (For now: the mod gives each character one the first time it loads. A marker file next to the saved bonfire stops it giving another.)
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
| The item | loose `ItemParam` | Row 60355010, cloned from Aged Feather, with its Effect ID set to its own row ID. DS2 has no SpEffect param to define effects in, but using an item still applies its Effect ID through `ApplySpEffect` after the animation, even when no such effect exists. It does nothing by itself, and the DLL hooks `ApplySpEffect` and watches for that ID on the player. State `0x0d` makes it reusable. |
| Item name, description | `itemname`, `simpleexplanation` and `detailedexplanation.fmg` via Mod Engine `mod\menu\text\english\` | Text is keyed by item ID. Nexus 1633 ships FMGs this way. It borrows the feather's icon for now. |
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
- **Travel** updates LastBonfire within about 70 ms of `WarpFunc`, before the load starts (the menu does it, not `WarpFunc`; see below).
- **Reloads without a warp:** 65 of them, probably mostly deaths (not individually confirmed), make no `WarpFunc` call and land on the bonfire's fixed arrival position.
- **`game_state`:** 30 is playable, 31 is leaving, then a mix of 18-21, 24 and 25, then 19, 28, 30. Position is readable from state 19 onward.

Tested in play (probe 0.2, one session):
- **Moving the player works.** Writing the three position copies lands within 0.05 units, on solid ground, in the same map. Four out of four.
- **The rest call works anywhere.** `+0x17DC40` with the last bonfire's ID, away from any bonfire, opens the full bonfire menu, including level up and Travel, and the player stays where they stood. Three out of three. The manager's +0x30 and +0x38 fill in while the menu is open and are clear again by the next press.
- **Travel from that menu works.** The game warped to Majula with its usual calls.
- **The warp manager is `*(EventManager+0x70)`.** The game's own value logged `MATCH`.
- **The rest effect** refills estus (reported in play), but the rest call does the whole job, so the mod doesn't need it.

Tested in play (probe 0.3, one session):
- **The rest call respawns enemies** (reported in play), so it's a real rest.
- **Moving after a death works.** A reload with no warp, ending at the host, then the write: on target to 0.000 units and still there 5 s later.
- **Parrying isn't caused by the write.** It started during the 2 s wait, before the write, and F6 on its own stops it. Writes with no key involved, like the respawn move, left the player standing still. So it comes from the test keys (which game input they trigger isn't known). The mod uses no hotkeys.
- **Coordinates are world-wide, and only the loaded map has ground.** A write into another map "succeeds", and the player falls through the world (z 27 to -160 in 5 s). Standing in m10_23 with LastBonfire in m10_04 was fine, so LastBonfire doesn't say what's loaded.
- **`WarpFunc` doesn't update LastBonfire.** The Travel menu does that around it. After a probe warp, LastBonfire stayed on the old bonfire, and a death went back there. This corrects the 0.1 note above.

Tested in play (probe 0.4, one session):
- **Warp then move works.** From Majula to the host, then the write: on target and still there 5 s later, on solid ground. So Travel to a pocket bonfire has a working path.
- **Moving after a death** worked again, also on target.
- **Current map candidates.** With LastBonfire in m10_23 (`0xa170000`), 32 fields read m10_17 (`0xa110000`) at each of five F9 presses, so they follow the player, not LastBonfire. `EventManager+0x2a4` is the leading one. Harvest Valley and Earthen Peak turned out to be the same map, so the walk between them couldn't separate the candidates; probe 0.5 logs eight of them through real transitions.

Tested in play (probe 0.5, one session):
- **The current map is at `*(BaseA+0x30)+0x360`, mirrored at `*(BaseA+0x78)+0x580`.** Both matched the target map after 6 of 6 warps, flipped to m10_10 as the player walked from Majula into Forest of the Fallen Giants, and read m10_17 after the earlier walk there from m10_23.
- **Rejected candidates:** `EventManager+0x2a4` (my leading guess) and `*(BaseA+0xb0)+0x6a8` aren't map IDs at all. `*(EventManager+0x30)+0xc0` holds pointers much of the time. `*(EventManager+0x20)+0x2d8` and `*(BaseA+0x40)+0x2f4` read `0xffffffff` during loads. `*(BaseA+0xa8)+0x1b8` lags an arrival by about 100 ms.

Tested in play (probe 0.6, one session):
- **Item use shows up in `ApplySpEffect`.** Using a test item applied `150000000` (the use starting, presumably), then about 2.75 s later the item's own Effect ID. Each test item did this, and the game returned 0 with no other effect. The feather's usage (2210) didn't warp, so the warp isn't tied to the usage.
- **State `0x0d` means not consumed.** One copy of `60355010` was used twice.
- **The game applies `110000010` during a death respawn.** So it can't tell a real rest from anything else. The first mod build assumed it could, and each death switched off the respawn point. A real rest is now recognised by the rest menu's flag (the bonfire manager's +0x38) being set when the mod didn't open it, assuming real bonfires use the same call.
- F3 toggled a first-person view: `dxgi.dll` (DS2 Lighting Engine) contains "free camera", so it's almost certainly that, not the mod. The F6 parry may be Lighting Engine too (not confirmed).

Still unproven:
- Whether the map fields are right after a kind 4 transition. Only walks and bonfire warps were seen.
- Whether the warp works with an unlit host. If you've lit no bonfire in the map you're standing in, the only hosts are unlit ones.
- Dying after we've set LastBonfire ourselves. The last probe test ran with LastBonfire already on the host, so the write changed nothing. The write held through the load (0.4), but no death followed it. DS2S-META's setter for the area field writes to the wrong address, so it's no evidence.

Risks, highest first:
1. The Travel list may reject a row that has no MSB bonfire behind it. Fallback: a "Pocket Bonfire" entry in the bonfire menu instead, the way Bonfire Level Up swaps out an entry.
2. **Choosing the host.** The host has to be in the map you're standing in, or the arrival drops the player through the world. `Addresses::current_map` now gives that map, so the open part is a map with no lit bonfire. Fallback: refuse placement there.
3. Moving the player right after a load may land before the ground has loaded at the target. The probe's Ctrl+F7 test watches for this.
4. Steam patches. Everything is resolved by AOB, and the DLL switches itself off when the exe isn't the known version (module size `0x1D76000`).

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
