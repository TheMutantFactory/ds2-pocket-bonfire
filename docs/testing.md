# Testing

One runner, `scripts\regression.ps1`, tests Pocket Bonfire and Easy Come, Easy Go together, since players
run them side by side. It expects `..\ds2-easy-come-easy-go` next to this repo, and skips that mod's checks
if it isn't there.

```powershell
.\scripts\regression.ps1                              # offline checks only, about a minute
.\scripts\regression.ps1 -InGame                      # plus every in-game scenario, about 25 minutes
.\scripts\regression.ps1 -InGame -Scenario coop,alone # only these
```

## No hosted CI

Nothing runs on GitHub. Most checks need the game's exe, which can't be put on a build server, and the
in-game ones need the game itself, Steam, a save and the screen. So the runner is the pipeline: it runs on
the developer's PC before every merge and every release.

| Stage | Where | When |
|---|---|---|
| Offline checks | Local, needs the game's exe | Before every commit that touches game code |
| In-game scenarios | Local, needs the game, Steam and a save; takes over the screen | Before merging into `develop`, and before every release |
| Package | `scripts\package.ps1`, local | When cutting a release |
| Clean install | By hand, from the zip, on a game without the mods | Before publishing |
| Publish | GitHub release by hand; Nexus upload by the author | After the clean install |

Branches follow Git Flow: work on `feature/*`, merged into `develop`; a `release/*` or `hotfix/*` branch
is merged into `main` and tagged `vX.Y.Z`. See [CHANGELOG.md](../CHANGELOG.md) for what each tag holds.

## Offline checks

Run for both repos.

| Check | What it proves |
|---|---|
| Unit tests (`cargo test --workspace`) | Settings files, including binary garbage; saved bonfires and per-character records, including damaged ones; adding the item's row to the item table; the hook lock letting one mod in at a time; a signature still matching a hooked function; Seamless launcher detection by file name |
| Signatures, plain | Every function and value the mods use is found in the 1.03 exe, exactly once |
| Signatures, hooked | Each is still found with another mod's hook over the function's first bytes |
| Damaged exe | Breaking one signature at a time turns off only the feature that uses it, with an error naming it |

## In-game scenarios

Each scenario installs fresh builds of both mods and a test DLL (`crates/regression`), launches the game,
loads the most recent save with a virtual controller, and plays through. Afterwards the saves, `dllMods`,
`lazyLoad.ini` and `modengine.ini` are put back exactly as they were. Results, both mods' logs, any crash
and a screenshot of a stuck run are kept in `out\regression-<time>\`.

### What each scenario sets up

| Scenario | Launched by | Other DLL mods | Mods' settings | Network |
|---|---|---|---|---|
| `full` | Steam | As installed | Defaults | Blocked by the mods |
| `alone` | Steam | None | Defaults | Blocked by the mods |
| `hooked` | Steam | As installed, plus `crates/hookfirst` loaded first, hooking every function the mods hook | Defaults | Blocked by the mods |
| `seamless` | Steam | As installed | Network check and network block off | Blocked by the test DLL instead |
| `coop` | `ds2sc_launcher.exe` (Seamless Co-op), through Explorer | As installed, plus Seamless | Defaults | Open: the mods must see Seamless; Mod Engine's block is turned off for the run |
| `fresh` | Steam | As installed | Defaults, no per-character records | Blocked by the mods |
| `damaged` | Steam | As installed | Garbage settings files and a garbage saved bonfire | Blocked by the mods |

### What each scenario checks

| Check | full | alone | hooked | seamless | coop | fresh | damaged |
|---|:-:|:-:|:-:|:-:|:-:|:-:|:-:|
| Reaches the title and loads the save | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ |
| Both items' names and descriptions show | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ |
| Placing a pocket bonfire | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ |
| Three quick uses don't start three rests | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ |
| Dying respawns at the pocket bonfire, without damage from the move | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ |
| Resting opens the bonfire menu, and the menu closes | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ |
| Easy Go saves, reaches the title and closes the game | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ |
| No crash, except the known closing one | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ |
| Three loads in one launch, quitting to the title between | ✓ | | | | | | |
| Item used mid-roll, with the menu open, and right before a quit | ✓ | | | | | | |
| Placing with no bonfire in the map is refused | ✓ | | | | | | |
| A real bonfire's rest takes over from the pocket one | ✓ | | | | | | |
| The other mod's hook is in place and still works | | | ✓ | | | | |
| Network check left alone, no network start refused | | | | ✓ | ✓ | | |
| Seamless Co-op seen by both mods | | | | | ✓ | | |
| Seamless Co-op *not* seen (a false detection fails the run) | ✓ | ✓ | ✓ | ✓ | | ✓ | ✓ |
| Both items given to a character without records | | | | | | ✓ | |
| Garbage settings fall back to the defaults, with a log line | | | | | | | ✓ |
| Garbage saved bonfire is ignored and placing still works | | | | | | | ✓ |

`coop` is skipped when Seamless Co-op isn't installed. Seamless starts the game only when launched the way
a double-click does, so the runner starts it through Explorer.

### Requirements

- The Steam version of Scholar of the First Sin 1.03.
- A save whose last bonfire is in the map the character stands in.
- For `coop`: Seamless Co-op in the game folder with a password set, and a `.co2` copy of the save.
- Hands off the PC: the game only runs while it's the foreground window, so the runner keeps bringing it
  back, and it presses buttons on a virtual controller.

### Freeze capture

If the game stops responding for 30 seconds, the runner saves every thread's stack to `stacks.txt`
(`scripts\thread-stacks.ps1`) and stops it. `-TraceSuspends` also logs every `SuspendThread` and
`ResumeThread` call with its caller.

## Last results

2026-11-13, on the development PC with Bonfire Level Up, StatFixer, Mod Engine and DS2 Lighting Engine
installed:

| Run | Result |
|---|---|
| Offline checks, both repos | All pass |
| `full`, `alone`, `hooked`, `seamless`, `fresh`, `damaged` | All pass, repeatedly (147 of 147 in the last full suite) |
| `coop`, Seamless Co-op 0.0.7, default settings | 28 of 28 |
| `alone` after the Seamless change | All pass, no false detection |

## Not covered

- A real co-op session with a second player joining. `coop` plays alone in a Seamless session.
- Creating a new character. `fresh` simulates one by deleting the records.
- A pocket bonfire below its host bonfire, as in a Silvercat ring drop.
- Languages other than English, and any exe other than Steam 1.03.

## Known issues

- An intermittent freeze at startup, seen about 6 times in 13 early runs with other hooking mods
  installed, 12 to 40 seconds in, after both mods had finished hooking. Threads from the NVIDIA driver and
  DS2 Lighting Engine were left suspended, one holding the heap lock. Not seen since, including in 19 runs
  with `-TraceSuspends`; the cause isn't proven.
- Once, in `coop`, the virtual B press didn't close the bonfire menu. It passed on the rerun.
- The game crashes on closing at `DarkSoulsII.exe+0xa5b3d6`. It happens without these mods too, and the
  runner notes it instead of failing.
