# Pocket Bonfire

A mod for Dark Souls II: Scholar of the First Sin that cuts down boss runs. It adds a reusable feather
that makes the spot you're standing on your respawn point and a place to rest.

![The Pocket Bonfire in the inventory](docs/images/inventory.webp)

![Resting at a pocket bonfire](docs/images/rest-menu.webp)

- **Place:** use the feather and that spot becomes your pocket bonfire. A high beep confirms it. Any
  earlier one is gone, so you only ever have one.
- **Respawn:** when you die, you come back at that spot.
- **Rest:** use the feather within a few steps of the spot. The real bonfire menu opens, including
  Travel, and enemies respawn as they do at any bonfire. It shows the name of the real bonfire you
  last rested at. The Level up entry in the screenshot comes from the Bonfire Level Up mod.

Every character gets one feather in its inventory the first time you load it after installing. Put it
in an item slot to use it.

**This mod keeps the game offline and starts it faster** by default. While it's installed, the game
can't connect to its servers, so there's no multiplayer, messages or summoning. It also skips the logos
and the network check, and goes straight to the title menu. Under Seamless Co-op it leaves the network
alone by itself: see Settings.

**Help us find what breaks.** Hit a bug, or found a mod that doesn't get along with this one? Tell us in
the mod's Nexus Mods thread or at https://mutantfactory.net/feedback. Which other mods you run, and
`Game\dllMods\pocket_bonfire.log` from a launch where it went wrong, help most. We can't beat the second
law of thermodynamics, but we can fix what breaks if we can.

What changed in each version is in [CHANGELOG.md](CHANGELOG.md).

## Install

You need the Steam version of Scholar of the First Sin, 1.03. On any other exe, the mod logs that and
turns off everything except the network block.

1. Back up your saves, from `%APPDATA%\DarkSoulsII`.
2. If `Game\dinput8.dll` already exists and came from a mod other than Lazy Loader, rename it to something
   like `othermod.dll` and move it to `Game\dllMods\`, creating the folder if needed.
3. Extract the zip into the game's Steam folder, the one that contains `Game\`. Vortex's DS2 support
   deploys to this folder too.

That's all. The zip includes [Lazy Loader](https://www.nexusmods.com/darksouls3/mods/677), which loads
the mod. If you already use it, your `lazyLoad.ini` isn't touched.

## Settings

The first launch writes `Game\dllMods\pocket_bonfire.ini`. Each line is a setting, `true` or `false`:

| Setting | Default | What it does |
|---|---|---|
| `skip_logos` | true | Skips the warning screen and logos. |
| `skip_network_check` | true | Skips the network check and goes straight to the title menu, offline. |
| `block_network` | true | Blocks all network access, so a modded game can't go online. |

**Seamless Co-op works with the defaults.** When `ds2sc_launcher.exe` starts the game, the mod sees it and
leaves the network check and the network alone, whatever these settings say, since Seamless needs both
and keeps the game off FromSoftware's servers itself. Launched through Steam as usual, the game stays
offline. The log says which it saw.

**If you use Mod Engine,** it blocks the network too: set `blockNetworkAccess=0` in `modengine.ini` to play
Seamless, or Steam closes the game a few seconds after the launcher starts it. Set it back to `1` before
launching through Steam, or a game with Mod Engine's mods goes online.

## Rules and limits

- **You need a bonfire in the same map.** The bonfire you last rested at has to be in the map you're
  standing in. If it isn't, you hear a low beep and nothing is placed.
- **A respawn may cost a little health.** Moving you to the spot can count as a fall. The regression test
  checks for it, and a fix is still being worked on.
- **Nothing shows at the spot.** There's no flame or marker, so remember where you put it.
- **Using the feather away from the spot moves it.** There's no confirmation.
- **It can't be travelled to.** You can Travel out from it, but it isn't listed as a destination.
- **It isn't blocked anywhere.** You can place it in combat or inside a boss arena, and a respawn inside
  an arena hasn't been tested. Placing it outside the fog is the safe choice.
- **Resting at a real bonfire takes over.** That bonfire becomes your respawn point. Rest at the pocket
  bonfire, or place it again, to switch back.
- **The respawn is a two-step move.** The game returns you to the real bonfire, then the mod moves you,
  so you may see the real bonfire for a moment.
- **The text is in English.** The feather's name and description show in English in every language.

## Other mods

The feather's data and text are added in memory, so the mod doesn't replace any game files and can't
conflict with mods that change params or text. It has been played alongside Easy Come, Easy Go, Bonfire
Level Up, StatFixer, True Easy Parry, RIP Enemies, No More Adaptability, Load Loose Param Files, Mod
Engine, DS2 Lighting Engine, Texture Renewal Project, Seamless Co-op 0.0.7 (from 0.11.0) and DS2 Inventory
Overhaul / Inventory Suite 1.0.1 (from 0.12.0).

## Uninstall

1. Delete `Game\dllMods\pocket_bonfire.dll`, `pocket_bonfire.log` and the `pocket_bonfire\` folder.
2. If no other mod uses Lazy Loader, delete `Game\dinput8.dll` as well.

Loading a save that still holds the feather after removing the mod is untested, so keep your save backup.

## Troubleshooting

`Game\dllMods\pocket_bonfire.log` records what the mod did: placing, resting, respawning and why anything
was refused. Attach it to bug reports.

- **"signature ... matched 0 times":** the game's code isn't where the mod expects. The line names what it
  found there, and if another mod has hooked that spot, which DLL it belongs to. Please include that line.
- **The item never arrives:** look for "couldn't add the item row" or a disabled line in the log. The quick
  start keeps working either way, since each part starts on its own.
- **"using its known address":** another mod changed that part of the game's code past recognition, and
  the mod found it anyway. The line names the DLL. Nothing to do unless something stops working.
- **"another mod changed it after our hook went in":** a mod that loaded later hooked the same function.
  Usually both keep working. If the feature named there stops, that mod is the first suspect.
- **"other DLLs:"** lists the mods and overlays in your game. It's what we'd ask you for, so please keep it
  in the log you send.
- **No log file at all:** the mod never loaded. Usually another mod's `dinput8.dll` replaced Lazy Loader's;
  rename that one, move it to `Game\dllMods\`, and put Lazy Loader's back.

Your saved spot is a small text file per character in `Game\dllMods\pocket_bonfire\`. Characters are told
apart by name, so two with the same name share a spot and a single feather. To get a new feather, delete
that character's `.item` and `.txt` files.

## Building

You need Rust and the MSVC compiler, plus the Lazy Loader 2.0.0 zip from Nexus:

```powershell
.\scripts\package.ps1 -LazyLoaderZip <path to the Lazy Loader zip>
```

This writes `out\pocket-bonfire-<version>.zip`.

### Regression tests

`.\scripts\regression.ps1` runs the unit tests and checks every signature against the game's exe, for
this repo and `..\ds2-easy-come-easy-go`. `.\scripts\regression.ps1 -InGame` also plays the game once per
scenario: with and without other mods, with another mod's hooks in the way, under Seamless Co-op, as a new
character, and with garbage settings. It takes over the screen for about 25 minutes.

[docs/testing.md](docs/testing.md) has the full matrix of scenarios and checks, how releases are made,
the last results, and what isn't covered yet.

### Forking or reimplementing

[docs/building-on-this.md](docs/building-on-this.md) has the lessons learned and caveats: the game's
quirks, adding items without changing game files, living with other mods' hooks, the network and Seamless
Co-op, and testing in the real game.

`crates/probe` is the test DLL used to work out how the
game behaves, and it isn't part of a release. [docs/design.md](docs/design.md) has the architecture and
what each test found.

## Credits

- [DS2S-META](https://github.com/pseudostripy/DS2S-META) by pseudostripy: offsets and signatures.
- Bob's DS2 cheat table: the remote rest call and how the game's functions are called.
- [Mod Engine](https://github.com/katalash/ModEngine) by Katalash: the approach to blocking the network.
- [Lazy Loader](https://www.nexusmods.com/darksouls3/mods/677) by Church Guard: loading the mod.
- [MinHook](https://github.com/TsudaKageyu/minhook) by Tsuda Kageyu and the
  [minhook crate](https://github.com/Jakobzs/minhook) by Jakobzs: function hooking.

## License

MIT. See [LICENSE](LICENSE). Release zips include `THIRD-PARTY-NOTICES.txt` for the libraries linked into
the mod, and for Lazy Loader.

---

Don't go hollow, I love you.

https://mutantfactory.net/feedback
