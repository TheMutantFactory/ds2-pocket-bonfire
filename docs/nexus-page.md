# Nexus page draft

Draft for the Dark Souls II page. Publishing is manual: paste these fields, upload the zip from
`scripts/package.ps1`, and add screenshots.

| Field | Value |
|---|---|
| Name | Pocket Bonfire |
| Author | mutantfactory |
| Version | 0.10.0 |
| Category | Gameplay |
| Requirements | None, Lazy Loader is included |
| Online | Offline only: the mod blocks network access |
| Source | https://github.com/TheMutantFactory/ds2-pocket-bonfire |

## Summary

A reusable feather that makes the spot you're standing on your respawn point and a place to rest. Shorter boss runs, anywhere you like. Unzip and play: offline, with a fast start.

## Description (BBCode)

```bbcode
[size=5][b]Pocket Bonfire[/b][/size]

A reusable feather that makes the spot you're standing on your respawn point and a place to rest. Its main use is cutting down boss runs. DS2 being DS2, you'll probably find others.

[b]This mod keeps the game offline and starts it faster.[/b] While it's installed, the game can't connect to its servers, so there's no multiplayer, messages or summoning. It also skips the logos and the network check, and goes straight to the title menu.

[b]How it works[/b]
[list]
[*][b]Place:[/b] use the feather and that spot becomes your pocket bonfire. A high beep confirms it. Any earlier one is gone, so you only ever have one.
[*][b]Respawn:[/b] when you die, you come back at that spot.
[*][b]Rest:[/b] use the feather within a few steps of the spot. The real bonfire menu opens, including Travel, and enemies respawn as they do at any bonfire. It shows the name of the real bonfire you last rested at. The Level up entry in the screenshot comes from the Bonfire Level Up mod.
[/list]
Every character gets one feather the first time you load it after installing. Put it in an item slot to use it.

[b]Install[/b]
You need the Steam version of Scholar of the First Sin, 1.03. On any other exe, the mod logs that and turns off everything except the network block.
[list=1]
[*]Back up your saves, from %APPDATA%\DarkSoulsII.
[*]If Game\dinput8.dll already exists and came from a mod other than Lazy Loader, rename it to something like othermod.dll and move it to Game\dllMods\, creating the folder if needed.
[*]Extract the zip into the game's Steam folder, the one that contains Game\. Vortex's DS2 support deploys to this folder too.
[/list]
That's all. The zip includes [url=https://www.nexusmods.com/darksouls3/mods/677]Lazy Loader[/url], which loads the mod. If you already use it, your lazyLoad.ini isn't touched.

[b]Rules and limits[/b]
[list]
[*]The bonfire you last rested at has to be in the map you're standing in. If it isn't, you hear a low beep and nothing is placed.
[*]Nothing shows at the spot. There's no flame or marker, so remember where you put it.
[*]Using the feather away from the spot moves it, with no confirmation.
[*]You can Travel out from it, but it isn't listed as a destination.
[*]It isn't blocked anywhere. You can place it in combat or inside a boss arena, and a respawn inside an arena hasn't been tested. Placing it outside the fog is the safe choice.
[*]Resting at a real bonfire makes that bonfire your respawn point. Rest at the pocket bonfire, or place it again, to switch back.
[*]You may see the real bonfire for a moment when you respawn, because the mod moves you after the game's own respawn.
[*]The feather's name and description show in English in every language.
[/list]

[b]Other mods[/b]
The feather's data and text are added in memory, so the mod doesn't replace any game files and can't conflict with mods that change params or text.

Played alongside Bonfire Level Up, StatFixer, True Easy Parry, RIP Enemies, No More Adaptability, Load Loose Param Files, Mod Engine, DS2 Lighting Engine and Texture Renewal Project.

[b]Uninstall and bug reports[/b]
Uninstall steps are in the included README. Loading a save that still holds the feather after removing the mod is untested, so keep your save backup.

For bug reports, attach Game\dllMods\pocket_bonfire.log.

[b]Credits[/b]
[list]
[*][url=https://github.com/pseudostripy/DS2S-META]DS2S-META[/url] by pseudostripy: offsets and signatures
[*]Bob's DS2 cheat table: the remote rest call and how the game's functions are called
[*][url=https://github.com/katalash/ModEngine]Mod Engine[/url] by Katalash: the approach to blocking the network
[*][url=https://www.nexusmods.com/darksouls3/mods/677]Lazy Loader[/url] by Church Guard: loading the mod
[*][url=https://github.com/TsudaKageyu/minhook]MinHook[/url] by Tsuda Kageyu and the minhook crate by Jakobzs: function hooking
[/list]

Source and license (MIT): [url=https://github.com/TheMutantFactory/ds2-pocket-bonfire]GitHub[/url]

Don't go hollow, I love you.
[url=https://mutantfactory.net/feedback]https://mutantfactory.net/feedback[/url]
```
