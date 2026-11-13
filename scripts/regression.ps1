<#
.SYNOPSIS
Regression tests for Pocket Bonfire and Easy Come, Easy Go.

.DESCRIPTION
Always runs the offline checks for both repos: unit tests, and every signature against the game's exe,
including how each one copes with another mod's hook, and that a damaged exe turns features off cleanly.

With -InGame it also plays the game, once per scenario. Each launch installs fresh builds of both mods
and the test DLL (crates/regression), and the test DLL loads the most recent save and plays through the
risky moments. Between and after scenarios the saves, the dllMods folder and lazyLoad.ini are put back
exactly as they were, so each scenario starts from the same place.

Scenarios:
  full      Your dllMods as they are, three loads in one launch (quitting to the title in between),
            item uses mid-roll, with the menu open and right before a quit, no bonfire in the map,
            and a real bonfire's rest taking over.
  alone     Only the two mods: no other dllMods.
  hooked    A test DLL (crates/hookfirst) loads first and hooks every function the mods hook, the
            way another mod would.
  seamless  The settings Seamless Co-op players use: the network check runs and neither mod blocks
            the network. The test DLL blocks it instead, so the modded game stays offline.
  coop      Real Seamless Co-op, started through ds2sc_launcher.exe, with the network open. Skipped
            when Seamless isn't installed. Plays alone: nobody joins.
  fresh     The per-character records are gone, as for a new character: both items must be given.
  damaged   Both settings files and the saved bonfire are garbage: the mods must fall back to the
            defaults and still work.

The in-game run takes over the screen and presses buttons on a virtual controller, for about two to
six minutes per scenario. The game only runs while it's the foreground window, so the runner keeps
bringing it back to the front. Don't use the PC while it runs. If Steam Cloud is on for the game, Steam may
ask about a sync conflict on the next launch, since the saves are put back after Steam has seen the
test's; keep the local files.

.EXAMPLE
.\scripts\regression.ps1
.\scripts\regression.ps1 -InGame
.\scripts\regression.ps1 -InGame -Scenario hooked,seamless
#>
param(
    [switch]$InGame,
    [string[]]$Scenario = @('full', 'alone', 'hooked', 'seamless', 'coop', 'fresh', 'damaged'),
    [string]$EasyComeEasyGo = (Join-Path (Split-Path (Split-Path $PSScriptRoot)) 'ds2-easy-come-easy-go'),
    [string]$GameDir = 'C:\Games\Steam\steamapps\common\Dark Souls II Scholar of the First Sin\Game',
    [string]$Exe = '',
    [int]$TimeoutMinutes = 15,
    # Logs every SuspendThread and ResumeThread call in the game to ds2_suspend_trace.log. It changes
    # the timing, so a startup freeze may not happen while it's on.
    [switch]$TraceSuspends
)
$ErrorActionPreference = 'Stop'
# Through -File a list arrives as one string, so split it here.
$Scenario = @($Scenario | ForEach-Object { $_ -split ',' } | Where-Object { $_ })
$PocketBonfire = Split-Path $PSScriptRoot
if (-not $Exe) { $Exe = Join-Path $GameDir 'DarkSoulsII.exe' }
$results = New-Object System.Collections.Generic.List[string]
$script:prefix = ''
function Note($status, $what) {
    $line = '{0} {1}{2}' -f $status, $script:prefix, $what
    $results.Add($line)
    Write-Host $line
}

function Invoke-Cargo($dir) {
    Push-Location $dir
    try {
        $out = & cargo @args 2>&1
        return @{ Ok = ($LASTEXITCODE -eq 0); Out = ($out | Out-String) }
    }
    finally { Pop-Location }
}

function Write-Summary {
    $failed = @($results | Where-Object { $_ -like 'FAIL*' }).Count
    Write-Host ''
    Write-Host ('{0} failed, {1} passed' -f $failed, @($results | Where-Object { $_ -like 'PASS*' }).Count)
    exit ([int]($failed -gt 0))
}

# --- Offline: unit tests and signatures, for both repos ---
foreach ($repo in @($PocketBonfire, $EasyComeEasyGo)) {
    $name = Split-Path $repo -Leaf
    if (-not (Test-Path $repo)) { Note 'SKIP' "$name offline checks: not found at $repo"; continue }
    $test = Invoke-Cargo $repo test --workspace --quiet
    if ($test.Ok) { Note 'PASS' "$name unit tests" } else { Note 'FAIL' "$name unit tests`n$($test.Out)" }
    $scan = Invoke-Cargo $repo run --release --quiet -p ds2 --example scan_exe -- $Exe
    if ($scan.Ok) { Note 'PASS' "$name signatures, plain and hooked" } else { Note 'FAIL' "$name signatures`n$($scan.Out)" }
    $damaged = Invoke-Cargo $repo run --release --quiet -p ds2 --example scan_exe -- $Exe --damaged
    if ($damaged.Ok) { Note 'PASS' "$name damaged exe: each broken signature fails alone, by name" } else { Note 'FAIL' "$name damaged exe`n$($damaged.Out)" }
}
if (-not $InGame) { Write-Summary }

if (Get-Process DarkSoulsII -ErrorAction SilentlyContinue) { throw 'Close the game first.' }
$dllMods = Join-Path $GameDir 'dllMods'
$lazyLoad = Join-Path $GameDir 'lazyLoad.ini'
$saves = Join-Path $env:APPDATA 'DarkSoulsII'
$run = Join-Path $PocketBonfire ("out\regression-{0:yyyyMMdd-HHmmss}" -f (Get-Date))
New-Item -ItemType Directory -Force $run | Out-Null

$built = Invoke-Cargo $PocketBonfire build --release --quiet -p pocket-bonfire -p regression -p hookfirst
if (-not $built.Ok) { throw "build failed in $PocketBonfire`n$($built.Out)" }
$haveEasy = Test-Path $EasyComeEasyGo
if ($haveEasy) {
    $built = Invoke-Cargo $EasyComeEasyGo build --release --quiet -p easy-come-easy-go
    if (-not $built.Ok) { throw "build failed in $EasyComeEasyGo`n$($built.Out)" }
}
$ourDlls = @((Join-Path $PocketBonfire 'target\release\pocket_bonfire.dll'), (Join-Path $PocketBonfire 'target\release\ds2_regression.dll'))
if ($haveEasy) { $ourDlls += Join-Path $EasyComeEasyGo 'target\release\easy_come_easy_go.dll' }
$ourNames = $ourDlls | ForEach-Object { Split-Path $_ -Leaf }

# Snapshots to put back after each scenario, whatever happens.
Copy-Item $saves (Join-Path $run 'saves') -Recurse
Copy-Item $dllMods (Join-Path $run 'dllMods-before') -Recurse
Copy-Item $lazyLoad (Join-Path $run 'lazyLoad-before.ini')
$modEngineIni = Join-Path $GameDir 'modengine.ini'
$haveModEngineIni = Test-Path $modEngineIni
if ($haveModEngineIni) { Copy-Item $modEngineIni (Join-Path $run 'modengine-before.ini') }

function Restore-Game {
    Copy-Item (Join-Path $run 'saves\*') $saves -Recurse -Force
    Remove-Item $dllMods -Recurse -Force
    Copy-Item (Join-Path $run 'dllMods-before') $dllMods -Recurse
    Copy-Item (Join-Path $run 'lazyLoad-before.ini') $lazyLoad -Force
    if ($haveModEngineIni) { Copy-Item (Join-Path $run 'modengine-before.ini') $modEngineIni -Force }
}

Add-Type -Namespace Regression -Name Win -MemberDefinition @'
[DllImport("user32.dll")] public static extern IntPtr GetForegroundWindow();
[DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr hWnd);
[DllImport("user32.dll")] public static extern bool ShowWindow(IntPtr hWnd, int cmd);
[DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr hWnd);
[DllImport("user32.dll")] public static extern bool IsIconic(IntPtr hWnd);
[DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr hWnd, out uint pid);
[DllImport("user32.dll")] public static extern void keybd_event(byte vk, byte scan, uint flags, UIntPtr extra);
public delegate bool EnumProc(IntPtr hWnd, IntPtr param);
[DllImport("user32.dll")] public static extern bool EnumWindows(EnumProc proc, IntPtr param);
public static IntPtr GameWindow(uint pid) {
    IntPtr found = IntPtr.Zero;
    EnumWindows((h, p) => {
        uint owner;
        GetWindowThreadProcessId(h, out owner);
        if (owner == pid && IsWindowVisible(h)) { found = h; return false; }
        return true;
    }, IntPtr.Zero);
    return found;
}
'@

# Brings the game's window to the front if something else has it, and says whether it had to. Windows
# only lets the process that last had input change the foreground, so a tap of Alt counts as input.
function Set-GameForeground($game) {
    # Leave it alone while any of the game's windows is in front: an Alt tap reaching the game would
    # open its window menu and pause it.
    $owner = [uint32]0
    [Regression.Win]::GetWindowThreadProcessId([Regression.Win]::GetForegroundWindow(), [ref]$owner) | Out-Null
    if ($owner -eq $game.Id) { return $false }
    $window = [Regression.Win]::GameWindow([uint32]$game.Id)
    if ($window -eq [IntPtr]::Zero) { return $false }
    [Regression.Win]::keybd_event(0x12, 0, 0, [UIntPtr]::Zero)
    [Regression.Win]::keybd_event(0x12, 0, 2, [UIntPtr]::Zero)
    if ([Regression.Win]::IsIconic($window)) { [Regression.Win]::ShowWindow($window, 9) | Out-Null }
    [Regression.Win]::SetForegroundWindow($window) | Out-Null
    return $true
}

function Save-Screenshot($path) {
    Add-Type -AssemblyName System.Windows.Forms, System.Drawing
    $bounds = [System.Windows.Forms.Screen]::PrimaryScreen.Bounds
    $bitmap = New-Object System.Drawing.Bitmap $bounds.Width, $bounds.Height
    $g = [System.Drawing.Graphics]::FromImage($bitmap)
    $g.CopyFromScreen($bounds.Location, [System.Drawing.Point]::Empty, $bounds.Size)
    $bitmap.Save($path, [System.Drawing.Imaging.ImageFormat]::Png)
    $g.Dispose(); $bitmap.Dispose()
}

function Set-Ini($path, $values) {
    $text = ($values.GetEnumerator() | ForEach-Object { '{0} = {1}' -f $_.Key, $_.Value }) -join "`r`n"
    Set-Content -Path $path -Value $text -Encoding ascii
}

# What each scenario sets up, and what the mods' logs must and mustn't say.
$always = @{
    'pocket_bonfire.log'    = @('ready', 'game calls run on the main thread', 'item use ignored', 'respawned at the pocket bonfire, moved=true', 'opened=true')
    'easy_come_easy_go.log' = @('ready', 'item used: saving and going to the title', 'at the title: closing=true')
}
$never = @('disabled', "didn't happen", "didn't pick it up", 'hooks failed', 'settings file:')
$scenarios = [ordered]@{
    full     = @{ Cfg = @{ cycles = 3; extras = 'true' }; Want = @{ 'pocket_bonfire.log' = @('not placed:', 'rested at a real bonfire') } }
    alone    = @{
        Cfg   = @{ cycles = 1 }
        Setup = { Get-ChildItem $dllMods -File -Filter '*.dll' | Where-Object { $ourNames -notcontains $_.Name } | Remove-Item }
    }
    hooked   = @{
        Cfg   = @{ cycles = 1 }
        Setup = {
            Copy-Item (Join-Path $PocketBonfire 'target\release\ds2_hookfirst.dll') $dllMods
            # First in the load order, so its hooks are in place before either mod looks.
            $out = foreach ($l in Get-Content $lazyLoad) {
                if ($l -match '^\[LOADORDER\]') { $l; '0=ds2_hookfirst.dll' }
                elseif ($l -match '^(\d+)=(.+)$') { '{0}={1}' -f ([int]$matches[1] + 1), $matches[2] }
                else { $l }
            }
            Set-Content $lazyLoad $out -Encoding ascii
        }
        Want  = @{ 'ds2_hookfirst.log' = @('hooked ') }
        Bad   = @{ 'ds2_hookfirst.log' = @('not found', 'hook failed', 'nothing hooked', 'not enabled') }
    }
    seamless = @{
        Cfg   = @{ cycles = 1; block_network = 'true' }
        Setup = {
            foreach ($ini in 'pocket_bonfire.ini', 'easy_come_easy_go.ini') {
                Set-Ini (Join-Path $dllMods $ini) ([ordered]@{ skip_logos = 'true'; skip_network_check = 'false'; block_network = 'false' })
            }
        }
        Want  = @{ 'pocket_bonfire.log' = @('network check left alone'); 'easy_come_easy_go.log' = @('network check left alone') }
        Bad   = @{ 'pocket_bonfire.log' = @('network blocked', 'network start refused'); 'easy_come_easy_go.log' = @('network blocked', 'network start refused') }
    }
    fresh    = @{
        Cfg   = @{ cycles = 1 }
        Setup = {
            foreach ($d in 'pocket_bonfire', 'easy_come_easy_go') {
                $p = Join-Path $dllMods $d
                if (Test-Path $p) { Get-ChildItem $p -File | Where-Object { $_.Extension -in '.item', '.txt' } | Remove-Item }
            }
        }
        Want  = @{ 'pocket_bonfire.log' = @('gave the item to'); 'easy_come_easy_go.log' = @('gave the item to') }
    }
    # Real Seamless Co-op: started through its launcher, which injects ds2sc.dll, with the network left
    # open since Seamless needs it: Steam's networking asserts and kills the game if WSAStartup fails.
    # Seamless keeps the game off FromSoftware's servers itself. It plays from its own .co2 save.
    coop     = @{
        Cfg      = @{ cycles = 1 }
        Launcher = 'ds2sc_launcher.exe'
        Setup    = {
            foreach ($ini in 'pocket_bonfire.ini', 'easy_come_easy_go.ini') {
                Set-Ini (Join-Path $dllMods $ini) ([ordered]@{ skip_logos = 'true'; skip_network_check = 'false'; block_network = 'false' })
            }
            # Mod Engine's own network block, put back by Restore-Game.
            if ($haveModEngineIni) { (Get-Content $modEngineIni) -replace '^blockNetworkAccess=1', 'blockNetworkAccess=0' | Set-Content $modEngineIni -Encoding ascii }
        }
        Want     = @{ 'pocket_bonfire.log' = @('network check left alone'); 'easy_come_easy_go.log' = @('network check left alone') }
        Bad      = @{ 'pocket_bonfire.log' = @('network blocked', 'network start refused'); 'easy_come_easy_go.log' = @('network blocked', 'network start refused') }
    }
    damaged  = @{
        Cfg   = @{ cycles = 1 }
        Setup = {
            $junk = "skip_logos = maybe`r`n" + [char]0 + [char]1 + "garbage without an equals sign`r`nblock_network`r`nmystery = true`r`n"
            foreach ($ini in 'pocket_bonfire.ini', 'easy_come_easy_go.ini') { Set-Content (Join-Path $dllMods $ini) $junk -NoNewline -Encoding utf8 }
            $p = Join-Path $dllMods 'pocket_bonfire'
            if (Test-Path $p) { Get-ChildItem $p -Filter '*.txt' | ForEach-Object { Set-Content $_.FullName "map=banana`r`nhost=-1`r`ny=NaN" -Encoding ascii } }
        }
        Want  = @{ 'pocket_bonfire.log' = @('settings file:', 'placed:'); 'easy_come_easy_go.log' = @('settings file:') }
        Allow = @('settings file:')
    }
}

foreach ($name in $Scenario) {
    $sc = $scenarios[$name]
    $script:prefix = "[$name] "
    if (-not $sc) { Note 'FAIL' "unknown scenario"; continue }
    if ($sc.Launcher -and -not (Test-Path (Join-Path $GameDir $sc.Launcher))) { Note 'SKIP' "$($sc.Launcher) isn't installed"; continue }
    $dir = Join-Path $run $name
    New-Item -ItemType Directory -Force $dir | Out-Null
    $started = Get-Date
    try {
        Restore-Game
        foreach ($dll in $ourDlls) { Copy-Item $dll $dllMods -Force }
        Get-ChildItem $dllMods -File | Where-Object { $_.Extension -in '.log', '.results' } | Remove-Item
        $cfg = [ordered]@{ scenario = $name }
        foreach ($k in $sc.Cfg.Keys) { $cfg[$k] = $sc.Cfg[$k] }
        if ($TraceSuspends) { $cfg['trace_suspends'] = 'true' }
        Set-Ini (Join-Path $dllMods 'ds2_regression.cfg') $cfg
        if ($sc.Setup) { & $sc.Setup }

        if ($sc.Launcher) {
            Write-Host "${script:prefix}launching the game through $($sc.Launcher). Hands off until it closes."
            # Through Explorer, as a double-click would: started straight from this script, Seamless
            # stops itself before the title. The launcher finds its DLL relative to the current
            # folder, so a small batch file changes to the game folder first.
            $cmd = Join-Path $dir 'launch.cmd'
            Set-Content $cmd "@echo off`r`ncd /d `"$GameDir`"`r`nstart `"`" `"$($sc.Launcher)`"`r`n" -Encoding ascii
            Start-Process explorer.exe "`"$cmd`""
        }
        else {
            Write-Host "${script:prefix}launching the game through Steam. Hands off until it closes."
            Start-Process 'steam://rungameid/335300'
        }
        $game = $null
        $deadline = (Get-Date).AddMinutes(3)
        while (-not $game -and (Get-Date) -lt $deadline) { Start-Sleep 2; $game = Get-Process DarkSoulsII -ErrorAction SilentlyContinue }
        if ($game) {
            # Wait for it to close, taking a screenshot if the test DLL reports a failure, or before
            # stopping a game that ran too long, so a stuck run shows where it stood.
            $liveResults = Join-Path $dllMods 'ds2_regression.results'
            $deadline = (Get-Date).AddMinutes($TimeoutMinutes)
            $shot = $false
            $refocused = 0
            $hungSince = $null
            while (-not $game.WaitForExit(5000)) {
                # A game that stopped responding won't recover: keep every thread's stack, then stop it.
                $game.Refresh()
                # Where each DLL sits, so return addresses in ds2_suspend_trace.log can be named.
                $modulesFile = Join-Path $dir 'modules.txt'
                if (-not (Test-Path $modulesFile) -and ((Get-Date) - $game.StartTime).TotalSeconds -gt 40) {
                    $game.Modules | ForEach-Object { '0x{0:x} 0x{1:x} {2}' -f [int64]$_.BaseAddress, ([int64]$_.BaseAddress + $_.ModuleMemorySize), $_.ModuleName } |
                        Set-Content $modulesFile
                }
                if ($game.Responding) { $hungSince = $null }
                elseif (-not $hungSince) { $hungSince = Get-Date }
                elseif (((Get-Date) - $hungSince).TotalSeconds -gt 30) {
                    & (Join-Path $PSScriptRoot 'thread-stacks.ps1') -ProcessId $game.Id | Set-Content (Join-Path $dir 'stacks.txt')
                    Save-Screenshot (Join-Path $dir 'screen.png')
                    Note 'FAIL' "the game stopped responding, so it was stopped; every thread's stack is in stacks.txt"
                    $game | Stop-Process -Force
                    break
                }
                # The game stops running while it isn't the foreground window, so keep it there.
                if (Set-GameForeground $game) { $refocused++ }
                $failedNow = (Test-Path $liveResults) -and (Select-String -Path $liveResults -Pattern '^FAIL' -Quiet)
                if (-not $shot -and ($failedNow -or (Get-Date) -gt $deadline)) {
                    Save-Screenshot (Join-Path $dir 'screen.png')
                    $shot = $true
                }
                if ((Get-Date) -gt $deadline) {
                    Note 'FAIL' "the game was still running after $TimeoutMinutes minutes, so it was stopped"
                    $game | Stop-Process -Force
                    break
                }
            }
            if ($refocused) { Note 'INFO' "brought the game back to the front $refocused times" }
            Start-Sleep 5
        }
        else { Note 'FAIL' 'the game never started' }
        Get-ChildItem $dllMods -File | Where-Object { $_.Extension -in '.log', '.results' } | ForEach-Object { Copy-Item $_.FullName $dir }
    }
    finally { Restore-Game }

    # The test DLL's own results.
    $resultsFile = Join-Path $dir 'ds2_regression.results'
    if (Test-Path $resultsFile) {
        foreach ($line in Get-Content $resultsFile) {
            $status, $rest = $line -split ' ', 2
            Note $status $rest
        }
    }
    else { Note 'FAIL' 'the test DLL wrote no results' }

    # The mods' own logs from this launch.
    $want = @{}
    foreach ($k in $always.Keys) { $want[$k] = @($always[$k]) }
    if ($sc.Want) { foreach ($k in $sc.Want.Keys) { $want[$k] = @($want[$k]) + @($sc.Want[$k]) | Where-Object { $_ } } }
    if (-not $haveEasy) { $want.Remove('easy_come_easy_go.log') }
    foreach ($log in $want.Keys) {
        $path = Join-Path $dir $log
        if (-not (Test-Path $path)) { Note 'FAIL' "$log missing"; continue }
        $text = Get-Content $path -Raw
        $bad = @($never | Where-Object { $sc.Allow -notcontains $_ })
        if ($sc.Bad -and $sc.Bad[$log]) { $bad += $sc.Bad[$log] }
        foreach ($b in $bad) {
            if ($text -match [regex]::Escape($b)) { Note 'FAIL' "$log says '$b'" }
        }
        foreach ($w in $want[$log]) {
            if ($text -match [regex]::Escape($w)) { Note 'PASS' "$log has '$w'" } else { Note 'FAIL' "$log lacks '$w'" }
        }
    }

    # Crashes during the run. +0xa5b3d6 happens on every close, with or without these mods.
    $crashes = Get-WinEvent -FilterHashtable @{ LogName = 'Application'; ProviderName = 'Application Error'; StartTime = $started } -ErrorAction SilentlyContinue |
        Where-Object Message -match 'DarkSoulsII'
    foreach ($c in $crashes) {
        $offset = if ($c.Message -match 'Fault offset: (\S+)') { $matches[1] } else { '?' }
        $module = if ($c.Message -match 'Faulting module name: ([^,]+),') { $matches[1] } else { '?' }
        if ($offset -match 'a5b3d6$') { Note 'INFO' 'the known closing crash at +0xa5b3d6' }
        else { Note 'FAIL' "crash in $module at $offset" }
    }
}
$script:prefix = ''
Write-Host "Logs and results kept in $run"
Write-Summary
