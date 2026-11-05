# Pocket Bonfire

A mod for Dark Souls II: Scholar of the First Sin that cuts down boss runs. It adds a reusable feather
that makes the spot you're standing on your respawn point and a place to rest.

- **Place:** use the feather and that spot becomes your pocket bonfire. A high beep confirms it. Any
  earlier one is gone, so you only ever have one.
- **Respawn:** when you die, you come back at that spot.
- **Rest:** use the feather within a few steps of the spot. The real bonfire menu opens, including
  Travel, and enemies respawn as they do at any bonfire.

Every character gets one feather in its inventory the first time you load it after installing. Put it
in an item slot to use it.

## Rules and limits

- **You need a bonfire in the same map.** The bonfire you last rested at has to be in the map you're
  standing in. If it isn't, you hear a low beep and nothing is placed.
- **Nothing shows at the spot.** There's no flame or marker yet, so remember where you put it.
- **Using the feather away from the spot moves it.** There's no confirmation.
- **It can't be travelled to.** You can Travel out from it, but it isn't listed as a destination.
- **It isn't blocked anywhere.** You can place it in combat or inside a boss arena, and a respawn inside
  an arena hasn't been tested. Placing it outside the fog is the safe choice.
- **Resting at a real bonfire takes over.** That bonfire becomes your respawn point. Rest at the pocket
  bonfire, or place it again, to switch back.
- **The respawn is a two-step move.** The game returns you to the real bonfire, then the mod moves you,
  so you may see the real bonfire for a moment.
- **English text only.** In other languages the item's name and description will be blank or missing.

## Requirements

- The Steam version of Scholar of the First Sin, 1.03. On any other exe the mod logs that and turns
  itself off.
- [Load Loose Param Files](https://www.nexusmods.com/darksouls2/mods/1092), which lets the game read the
  item's data.
- [Lazy Loader](https://www.nexusmods.com/darksouls3/mods/677), listed under DS3 but it works here, which
  loads the mod's DLL.
- Mod Engine, which loads the item's text. It's bundled with many DS2 mods.
  [True Easy Parry](https://www.nexusmods.com/darksouls2/mods/1329) is the copy this was tested with.

**Play offline.** Mod Engine blocks network access by default (`blockNetworkAccess=1`). Leave that on,
since going online with a modded game risks a ban.

## Install

Back up your saves first, from `%APPDATA%\DarkSoulsII`.

1. Install Load Loose Param Files into `Game\`, the folder with `DarkSoulsII.exe`.
2. Install Lazy Loader: `dinput8.dll` and `lazyLoad.ini` go in `Game\`, without the zip's `v2.0.0\`
   folder.
3. Set up Mod Engine under Lazy Loader. Rename its `dinput8.dll` to `modengine.dll` and put it in
   `Game\dllMods\`. Its `modengine.ini` goes in `Game\`.
4. Extract this mod's zip into the game's Steam folder, the one that contains `Game\`. Its files merge
   into `Game\`. Vortex's DS2 support deploys to this folder too.
5. In `Game\lazyLoad.ini`, list both DLLs under `[LOADORDER]`, keeping any others you have:

   ```ini
   [LOADORDER]
   0=modengine.dll
   1=pocket_bonfire.dll
   ```

Install this mod after Load Loose Param Files, or the framework's `Param\` folder will overwrite this
mod's `ItemParam.param`.

## Conflicts

This mod replaces whole files, so it conflicts with any mod that ships the same ones:

- `Game\Param\ItemParam.param`
- `Game\mod\menu\text\english\itemname.fmg`, `simpleexplanation.fmg` and `detailedexplanation.fmg`

If another mod's `ItemParam.param` wins, the feather won't exist in the game. If its text files win, the
feather has no name or description. To combine them, `build_item_param` and `build_item_text` in
`crates/ds2/examples` add the row and text to another mod's files.

Other DLL mods are fine as long as they load through Lazy Loader. It has been played alongside Bonfire
Level Up, StatFixer, True Easy Parry, RIP Enemies, No More Adaptability, DS2 Lighting Engine and Texture
Renewal Project.

## Uninstall

1. Delete `Game\dllMods\pocket_bonfire.dll`, `pocket_bonfire.log` and the `pocket_bonfire\` folder.
2. Remove `pocket_bonfire.dll` from `lazyLoad.ini`.
3. Put back the original `ItemParam.param` from Load Loose Param Files.
4. Delete the three text files above from `Game\mod\menu\text\english\`, unless another mod needs them.

Loading a save that still holds the feather after removing it is untested, so keep your save backup.

## Troubleshooting

`Game\dllMods\pocket_bonfire.log` records what the mod did: placing, resting, respawning and why anything
was refused. Attach it to bug reports.

Your saved spot is a small text file per character in `Game\dllMods\pocket_bonfire\`. Characters are told
apart by name, so two with the same name share a spot and a single feather. To get a new feather, delete
that character's `.item` and `.txt` files.

## Building

You need Rust and the MSVC compiler. The param and text come from your own game files, since they aren't
in this repo:

```powershell
.\scripts\package.ps1 -StockItemParam <1092's ItemParam.param> -EnglishText <unpacked menu\text\english>
```

This writes `out\pocket-bonfire-<version>.zip`. `crates/probe` is the test DLL used to work out how the
game behaves, and it isn't part of a release. [docs/design.md](docs/design.md) has the architecture and
what each test found.

## Credits

- [DS2S-META](https://github.com/pseudostripy/DS2S-META) by pseudostripy: offsets and signatures.
- Bob's DS2 cheat table: the remote rest call and how the game's functions are called.
- [MinHook](https://github.com/TsudaKageyu/minhook) by Tsuda Kageyu and the
  [minhook crate](https://github.com/Jakobzs/minhook) by Jakobzs: function hooking.

## License

MIT. See [LICENSE](LICENSE). Release zips include `THIRD-PARTY-NOTICES.txt` for the linked libraries.
They also contain modified copies of the game's own `ItemParam` and text files, which aren't covered by
that license and remain FromSoftware's.
