# Changelog

Versions follow the release tags. Unreleased work is on `develop` or a `feature/*` branch until it's
merged and tagged.

## 0.11.0 (unreleased)

If you also use Easy Come, Easy Go and play Seamless Co-op, update it to 0.2.0 too: either mod blocking
the network is enough to break Seamless.

### Added

- `pocket_bonfire.ini`, written on first launch: turns the logo skip, the network check skip and the
  network block on or off. A missing, unreadable or garbled file falls back to the defaults, and the log
  says which lines it ignored.
- Seamless Co-op support: when `ds2sc_launcher.exe` starts the game, or Seamless's `ds2sc.dll` is loaded,
  the mod leaves the network check and the network alone, whatever the settings say. Seamless needs both,
  and keeps the game off FromSoftware's servers itself. A normal Steam launch stays offline.
- Log lines that say what happened: which settings were used, whether Seamless was seen, network starts
  refused or let through, item uses ignored while the last one is still being handled.

### Fixed

- **Released versions crash under Seamless Co-op.** 0.10.x always refused to start Windows networking,
  and Steam closes the game a few seconds after the Seamless launcher starts it. Fixed by the Seamless
  support above.
- Works when another mod has hooked the same functions first: each function is still found, and when a
  signature fails, the log says what's there and which DLL a hook leads to.
- The quick start and the bonfire start separately, so one failing leaves the other working.
- Rest, giving the item and the respawn move run on the game's main thread instead of the mod's own, so
  they can't race the game.
- A double use, or a use during a cancelled animation, can no longer start a second rest while the first
  is opening.
- Item tables that carry row names are accepted.

### Testing

- `scripts/regression.ps1`: unit tests and signature checks for both mods, and with `-InGame`, seven
  scenarios played in the real game. See [docs/testing.md](docs/testing.md).

## 0.10.1 (2026-11-05)

### Fixed

- Pocket Bonfire and Easy Come, Easy Go could knock each other's hooks out when both installed at the same
  moment, and which one lost changed from launch to launch. Both now take turns through a named lock.

## 0.10.0 (2026-11-05)

First public release.

- The Pocket Bonfire, a reusable key item each character gets once, on first load. Use it to place a
  pocket bonfire where you stand, in any area with a real bonfire; use it beside the spot to rest there
  with the full bonfire menu. Dying brings you back to the spot.
- The item and its text are added in memory, so no game files change and no other mods are needed.
- Starts straight at the title menu, offline: skips the logos and the network check, and blocks the
  network so a modded game can't go online.
- Ships as one zip with Lazy Loader.

## Before 0.10.0

Development builds, tested in play but not released: the probe DLL that worked out how warps, deaths and
the current map behave (0.1 to 0.6), the first place, rest and respawn (0.6), keeping the respawn point
through deaths (0.7), and the one-item-per-character version that needed Mod Engine and loose param files
(0.8, never published; its `release/0.8.0` branch is still on GitHub).

---

Don't go hollow, I love you.

https://mutantfactory.net/feedback
