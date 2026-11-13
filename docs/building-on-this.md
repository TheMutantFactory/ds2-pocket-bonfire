# Building on this

For anyone forking Pocket Bonfire, reimplementing it, or writing their own DS2 DLL mod. The code is MIT,
so take what helps. This page is the short version of what cost us time; [design.md](design.md) has the
raw notes, offsets and what each probe test found.

If you build something on it, we'd love to hear about it: the Nexus Mods thread or
https://mutantfactory.net/feedback.

## Where to start

- **`crates/ds2`** is the reusable part: guarded memory reads and writes, signature scanning that copes
  with other mods' hooks, the known offsets, the settings file, the hook lock, main-thread calls and
  Seamless Co-op detection. Easy Come, Easy Go carries a copy of the same crate.
- **`crates/probe`** is the throwaway DLL we used to find things out, with test keys. Expect to write
  your own: most of the work was observing the game, not writing the mod.
- **`cargo run -p ds2 --example scan_exe -- <path to DarkSoulsII.exe>`** checks every signature against an
  exe, also under simulated hooks and broken one at a time.
- **`scripts\regression.ps1 -InGame`** plays the game through your build. See [testing.md](testing.md).

## Lessons learned

### The game

- **Only Steam SotFS 1.03 is targeted.** Everything is found by signature, and the mods also check the
  module size (`0x1D76000`) before trusting offsets. On any other exe they log it and turn off.
- **Coordinates are world-wide, but only the loaded map has ground.** Writing a position in another map
  "works", and the player falls through the world. Anything that moves the player must check the current
  map first: `*(BaseA+0x30)+0x360`.
- **Deaths don't go through `WarpFunc`.** A death reload makes no warp call and lands on the bonfire's
  fixed arrival point, so respawn logic has to watch `game_state` (30 is playable) instead of hooking the
  warp.
- **The rest effect isn't a rest.** The game applies `110000010` during every death respawn too. A real
  rest shows as the bonfire manager's `+0x38` flag set by someone other than you.
- **`WarpFunc` doesn't update the last bonfire.** The Travel menu does that around it, so warping alone
  leaves deaths going to the old bonfire.
- **The warp struct has no coordinates.** Rewriting where a warp goes means warp to the host, then move
  the player after the load.
- **There's no known way to spawn an object at a position.** That's why there's no visible marker.

### Items without changing game files

- **An item can be added in memory.** Point the game at a copy of ItemParam with your row added (the
  pointer is at `*(*(BaseA+0xa8)+0x20)+0xd8`), and clear the "owned" byte at `+0xe8`, or the game's
  allocator asserts on your copy when it closes. Keep checking the table pointer while the game runs: if
  it changes, add the row again. Another mod may have done the same, so add your row to whatever table is
  there now, not to the stock one.
- **Text comes from hooking `FmgLookup`** for your item's id. Let text a file really has win.
- **Item use shows up in `ApplySpEffect`.** DS2 has no SpEffect param to define an effect in, but using an
  item still applies its Effect ID after the animation. Set the Effect ID to the row's own id and watch for
  it on the player. Item state `0x0d` makes it reusable.
- **Expect double uses.** A cancelled animation or a quick double press can fire twice. Ignore uses for a
  few seconds after one is handled.
- **Pick row ids nobody else uses.** Ours are `60355010` (Pocket Bonfire) and `60595010` (Easy Go).
- **Characters are told apart by name.** Two characters with the same name share records. We haven't found
  a better id that's readable at load time.

### Living with other mods

- **Other DLL mods hook the same functions.** StatFixer, Bonfire Level Up, Mod Engine and DS2 Lighting
  Engine each carry their own MinHook. Find functions with signatures whose first bytes are wildcards, so
  someone else's jump there doesn't hide the function, and when a signature fails, log which DLL the jump
  leads to.
- **Take turns installing hooks.** Two MinHook copies creating and enabling hooks at the same moment can cut
  one out of the chain, differently on each launch. We hold a named mutex,
  `Local\DS2-TheMutantFactory-Hooks`, from creating hooks to enabling them. Please join it: any mod can, by
  using the same name (`ds2::hooks::serialized`).
- **Don't call game code from your own thread.** Menus and item code follow pointers the game changes
  mid-frame. We run calls on the main thread by pointing the exe's `XInputGetState` import at our own
  function, which runs queued calls and then polls as normal. Keep the pointer you replaced so several
  mods chain.
- **Do almost nothing in `DllMain`.** It runs under the loader lock. We read the settings file and hook
  `WSAStartup` there, since the game may start networking right away, and start everything else on a
  thread.
- **Start each feature separately.** If one signature breaks, only that feature should switch off.
- **An intermittent startup freeze** turned up with several hooking mods installed: threads left suspended
  while one held the heap lock. It happened after our hooks were done and its cause isn't proven. If you
  see a grey "Not Responding" at launch, `scripts\thread-stacks.ps1` shows where every thread is stuck.

### The network and Seamless Co-op

- **Refusing `WSAStartup` keeps a modded game offline,** as Mod Engine does. It's the safe default: a
  modded game on FromSoftware's servers risks a ban.
- **But it breaks Seamless Co-op.** Steam's networking runs inside the game and asserts when `WSAStartup`
  fails ("IClientNetworkingSockets_Create_User - WSAStartup failed"), and Steam closes the game a few
  seconds in. The assert dumps land in Steam's `dumps` folder.
- **Detect Seamless by the parent process.** `ds2sc_launcher.exe` starts the game, and `ds2sc.dll` loads
  about a second after the mods in `dllMods`, so checking for the DLL in `DllMain` misses it. We check
  both, and also let `WSAStartup` through if the DLL has arrived by then (`ds2::seamless`).
- **Seamless needs the game's network check to run,** so skip it only when Seamless isn't there.
- **Mod Engine's `blockNetworkAccess`** breaks Seamless the same way, and it's not ours to switch.
- Seamless is closed source, packed, and its author asks that it not be modified or redistributed. Work
  alongside it; don't hook into it.

### Settings and failure

- **A bad settings file must never stop the mod.** Fall back to the defaults, log which lines were
  ignored, and write a default file if there's none.
- **Build with `panic = "abort"`.** A Rust panic must never unwind into game code.
- **Log in plain sentences.** Players attach the log to bug reports, and it's often all there is.

### Testing in the real game

- **The game only runs while it's the foreground window,** so an automated run has to keep bringing it
  back, and the PC is unusable meanwhile.
- **Simulated keyboard input never reached the game.** A virtual controller through the same
  `XInputGetState` hook does.
- **HP lives in `PlayerCtrl`,** and hollowing caps it below the maximum, so compare HP with the HP right
  after a rest, not the maximum.
- **Seamless only starts when launched the way a double-click does.** Started straight from a script, it
  stops itself before the title, for reasons we haven't found. The runner goes through Explorer.
- **Test keys collide with other mods.** DS2 Lighting Engine uses F3 (free camera) and possibly F6. Our
  released mods use no hotkeys at all.
- **The game crashes on closing at `DarkSoulsII.exe+0xa5b3d6`,** with or without mods. Don't chase it as
  your bug.

## Caveats

- **Credit and licenses.** Offsets and signatures come from [DS2S-META](https://github.com/pseudostripy/DS2S-META)
  (MIT). The remote rest call (`+0x17DC40`) and how the game's functions are called come from Bob's DS2
  cheat table, which has no license, so we use facts from it and no code. Please keep it that way.
- **Unproven:** Travel *to* a pocket bonfire (a new bonfire row in the Travel list may be rejected), hosts
  that aren't lit, map changes that aren't warps or walks, and a spot below its host (as after a Silvercat
  drop).
- **Not tested:** a real co-op session with a second player, a freshly created character (only simulated),
  and languages other than English.
- **Offsets are for one exe.** If Steam patches the game, every signature needs checking again with
  `scan_exe`.

---

Don't go hollow, I love you.

https://mutantfactory.net/feedback
