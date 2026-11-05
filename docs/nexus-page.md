# Nexus page draft

Draft for the Dark Souls II page. Publishing is manual: paste these fields, upload the zip from
`scripts/package.ps1`, and add screenshots.

| Field | Value |
|---|---|
| Name | Pocket Bonfire |
| Author | mutantfactory |
| Version | 0.8.0 |
| Category | Gameplay |
| Requirements | Load Loose Param Files (DS2 1092), Lazy Loader (DS3 677), Mod Engine |
| Online | Offline only |
| Source | https://github.com/TheMutantFactory/ds2-pocket-bonfire |

## Summary

A reusable feather that makes the spot you're standing on your respawn point and a place to rest. Shorter boss runs, anywhere you like.

## Description (BBCode)

```bbcode
[size=5][b]Pocket Bonfire[/b][/size]

A reusable feather that makes the spot you're standing on your respawn point and a place to rest. Its main use is cutting down boss runs. DS2 being DS2, you'll probably find others.

[b]How it works[/b]
[list]
[*][b]Place:[/b] use the feather and that spot becomes your pocket bonfire. A high beep confirms it. Any earlier one is gone, so you only ever have one.
[*][b]Respawn:[/b] when you die, you come back at that spot.
[*][b]Rest:[/b] use the feather within a few steps of the spot. The real bonfire menu opens, including Travel, and enemies respawn as they do at any bonfire.
[/list]
Every character gets one feather the first time you load it after installing. Put it in an item slot to use it.

[b]Rules and limits[/b]
[list]
[*]The bonfire you last rested at has to be in the map you're standing in. If it isn't, you hear a low beep and nothing is placed.
[*]Nothing shows at the spot. There's no flame or marker yet, so remember where you put it.
[*]Using the feather away from the spot moves it, with no confirmation.
[*]You can Travel out from it, but it isn't listed as a destination.
[*]It isn't blocked anywhere. You can place it in combat or inside a boss arena, and a respawn inside an arena hasn't been tested. Placing it outside the fog is the safe choice.
[*]Resting at a real bonfire makes that bonfire your respawn point. Rest at the pocket bonfire, or place it again, to switch back.
[*]You may see the real bonfire for a moment when you respawn, because the mod moves you after the game's own respawn.
[*]English text only.
[/list]

[b]Requirements[/b]
[list]
[*]Steam version of Scholar of the First Sin, 1.03. On any other exe the mod logs that and turns itself off.
[*][url=https://www.nexusmods.com/darksouls2/mods/1092]Load Loose Param Files[/url]
[*][url=https://www.nexusmods.com/darksouls3/mods/677]Lazy Loader[/url] (listed under DS3, works here)
[*]Mod Engine, bundled with many DS2 mods. [url=https://www.nexusmods.com/darksouls2/mods/1329]True Easy Parry[/url] is the copy this was tested with.
[/list]
[b]Play offline.[/b] Mod Engine blocks network access by default. Leave that on, since going online with a modded game risks a ban.

[b]Install[/b]
Back up your saves first, from %APPDATA%\DarkSoulsII.
[list=1]
[*]Install Load Loose Param Files into Game\, the folder with DarkSoulsII.exe.
[*]Install Lazy Loader: dinput8.dll and lazyLoad.ini go in Game\, without the zip's v2.0.0\ folder.
[*]Set up Mod Engine under Lazy Loader. Rename its dinput8.dll to modengine.dll and put it in Game\dllMods\. Its modengine.ini goes in Game\.
[*]Extract this mod's zip into the game's Steam folder, the one that contains Game\. Vortex's DS2 support deploys to this folder too.
[*]In Game\lazyLoad.ini, list modengine.dll and pocket_bonfire.dll under [LOADORDER], keeping any others you have.
[/list]
Install this mod after Load Loose Param Files, or the framework will overwrite this mod's ItemParam.param.

[b]Conflicts[/b]
This mod replaces Game\Param\ItemParam.param and three text files in Game\mod\menu\text\english\ (itemname, simpleexplanation, detailedexplanation). It conflicts with any mod that ships the same files. If another mod's ItemParam wins, the feather won't exist in the game. If its text files win, the feather has no name or description.

Played alongside Bonfire Level Up, StatFixer, True Easy Parry, RIP Enemies, No More Adaptability, DS2 Lighting Engine and Texture Renewal Project.

[b]Uninstall and bug reports[/b]
Uninstall steps are in the included README. Loading a save that still holds the feather after removing the mod is untested, so keep your save backup.

For bug reports, attach Game\dllMods\pocket_bonfire.log.

[b]Credits[/b]
[list]
[*][url=https://github.com/pseudostripy/DS2S-META]DS2S-META[/url] by pseudostripy: offsets and signatures
[*]Bob's DS2 cheat table: the remote rest call and how the game's functions are called
[*][url=https://github.com/TsudaKageyu/minhook]MinHook[/url] by Tsuda Kageyu and the minhook crate by Jakobzs: function hooking
[/list]

Source and license (MIT): [url=https://github.com/TheMutantFactory/ds2-pocket-bonfire]GitHub[/url]
```
