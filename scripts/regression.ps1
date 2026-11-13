<#
.SYNOPSIS
Regression tests for Pocket Bonfire and Easy Come, Easy Go.

.DESCRIPTION
Always runs the offline checks for both repos: unit tests, and every signature against the game's exe,
including how each one copes with another mod's hook.

With -InGame it also plays the game once: it installs fresh builds of both mods and the test DLL
(crates/regression), launches the game through Steam, and the test DLL loads the most recent save
and plays through the risky moments. Afterwards it puts back the saves and the dllMods folder exactly
as they were, and reports the results, both mods' logs and any crashes.

The in-game run takes over the keyboard and the screen for a few minutes. Don't use the PC while
it runs. If Steam Cloud is on for the game, Steam may ask about a sync conflict on the next launch,
since the saves are put back after Steam has seen the test's; keep the local files.

.EXAMPLE
.\scripts\regression.ps1
.\scripts\regression.ps1 -InGame
#>
param(
    [switch]$InGame,
    [string]$EasyComeEasyGo = (Join-Path (Split-Path (Split-Path $PSScriptRoot)) 'ds2-easy-come-easy-go'),
    [string]$GameDir = 'C:\Games\Steam\steamapps\common\Dark Souls II Scholar of the First Sin\Game',
    [string]$Exe = '',
    [int]$TimeoutMinutes = 15
)
$ErrorActionPreference = 'Stop'
$PocketBonfire = Split-Path $PSScriptRoot
if (-not $Exe) { $Exe = Join-Path $GameDir 'DarkSoulsII.exe' }
$results = New-Object System.Collections.Generic.List[string]
function Note($status, $what) {
    $line = '{0} {1}' -f $status, $what
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

# --- Offline: unit tests and signatures, for both repos ---
foreach ($repo in @($PocketBonfire, $EasyComeEasyGo)) {
    $name = Split-Path $repo -Leaf
    if (-not (Test-Path $repo)) { Note 'SKIP' "$name offline checks: not found at $repo"; continue }
    $test = Invoke-Cargo $repo test --workspace --quiet
    if ($test.Ok) { Note 'PASS' "$name unit tests" } else { Note 'FAIL' "$name unit tests`n$($test.Out)" }
    $scan = Invoke-Cargo $repo run --release --quiet -p ds2 --example scan_exe -- $Exe
    if ($scan.Ok) { Note 'PASS' "$name signatures, plain and hooked" } else { Note 'FAIL' "$name signatures`n$($scan.Out)" }
}

if ($InGame) {
    if (Get-Process DarkSoulsII -ErrorAction SilentlyContinue) { throw 'Close the game first.' }
    $dllMods = Join-Path $GameDir 'dllMods'
    $saves = Join-Path $env:APPDATA 'DarkSoulsII'
    $run = Join-Path $PocketBonfire ("out\regression-{0:yyyyMMdd-HHmmss}" -f (Get-Date))
    New-Item -ItemType Directory -Force $run | Out-Null

    $builds = @(
        @{ Repo = $PocketBonfire; Args = @('build', '--release', '--quiet', '-p', 'pocket-bonfire', '-p', 'regression'); Dlls = @('pocket_bonfire.dll', 'ds2_regression.dll') },
        @{ Repo = $EasyComeEasyGo; Args = @('build', '--release', '--quiet', '-p', 'easy-come-easy-go'); Dlls = @('easy_come_easy_go.dll') }
    )
    foreach ($b in $builds) {
        if (-not (Test-Path $b.Repo)) { continue }
        $built = Invoke-Cargo $b.Repo @($b.Args)
        if (-not $built.Ok) { throw "build failed in $($b.Repo)`n$($built.Out)" }
    }

    # Snapshots to put back afterwards, whatever happens.
    Copy-Item $saves (Join-Path $run 'saves') -Recurse
    Copy-Item $dllMods (Join-Path $run 'dllMods-before') -Recurse
    $started = Get-Date
    try {
        foreach ($b in $builds) {
            if (-not (Test-Path $b.Repo)) { continue }
            foreach ($dll in $b.Dlls) { Copy-Item (Join-Path $b.Repo "target\release\$dll") $dllMods -Force }
        }
        Get-ChildItem $dllMods -Filter '*.log' | Remove-Item
        Remove-Item (Join-Path $dllMods 'ds2_regression.results') -ErrorAction SilentlyContinue

        Write-Host 'Launching the game through Steam. Hands off the keyboard until it closes.'
        Start-Process 'steam://rungameid/335300'
        $game = $null
        $deadline = (Get-Date).AddMinutes(3)
        while (-not $game -and (Get-Date) -lt $deadline) { Start-Sleep 2; $game = Get-Process DarkSoulsII -ErrorAction SilentlyContinue }
        if (-not $game) { throw 'The game never started.' }
        if (-not $game.WaitForExit($TimeoutMinutes * 60 * 1000)) {
            Note 'FAIL' "the game was still running after $TimeoutMinutes minutes, so it was stopped"
            $game | Stop-Process -Force
            Start-Sleep 3
        }
        Start-Sleep 3
        foreach ($f in Get-ChildItem $dllMods -File | Where-Object { $_.Extension -in '.log', '.results' }) { Copy-Item $f.FullName $run }
    }
    finally {
        # Put the saves and dllMods back exactly as they were.
        Copy-Item (Join-Path $run 'saves\*') $saves -Recurse -Force
        Remove-Item $dllMods -Recurse -Force
        Copy-Item (Join-Path $run 'dllMods-before') $dllMods -Recurse
    }

    # The test DLL's own results.
    $resultsFile = Join-Path $run 'ds2_regression.results'
    if (Test-Path $resultsFile) {
        foreach ($line in Get-Content $resultsFile) { $results.Add($line); Write-Host $line }
    }
    else { Note 'FAIL' 'the test DLL wrote no results' }

    # The mods' own logs from this launch.
    $expect = @{
        'pocket_bonfire.log'    = @('ready', 'game calls run on the main thread', 'placed:', 'item use ignored', 'respawned at the pocket bonfire, moved=true', 'opened=true')
        'easy_come_easy_go.log' = @('ready', 'item used: saving and going to the title', 'at the title: closing=true')
    }
    foreach ($log in $expect.Keys) {
        $path = Join-Path $run $log
        if (-not (Test-Path $path)) { Note 'FAIL' "$log missing"; continue }
        $text = Get-Content $path -Raw
        foreach ($bad in @('disabled', "didn't happen", "didn't pick it up", 'hooks failed')) {
            if ($text -match [regex]::Escape($bad)) { Note 'FAIL' "$log says '$bad'" }
        }
        foreach ($want in $expect[$log]) {
            if ($text -match [regex]::Escape($want)) { Note 'PASS' "$log has '$want'" } else { Note 'FAIL' "$log lacks '$want'" }
        }
    }

    # Crashes during the run. +0xa5b3d6 happens on every close, with or without these mods.
    $crashes = Get-WinEvent -FilterHashtable @{ LogName = 'Application'; ProviderName = 'Application Error'; StartTime = $started } -ErrorAction SilentlyContinue |
        Where-Object Message -match 'DarkSoulsII'
    foreach ($c in $crashes) {
        $offset = if ($c.Message -match 'Fault offset: (\S+)') { $matches[1] } else { '?' }
        $module = if ($c.Message -match 'Faulting module name: ([^,]+),') { $matches[1] } else { '?' }
        if ($offset -match 'a5b3d6$') { Note 'INFO' "the known closing crash at +0xa5b3d6" }
        else { Note 'FAIL' "crash in $module at $offset" }
    }
    Write-Host "Logs and results kept in $run"
}

$failed = @($results | Where-Object { $_ -like 'FAIL*' }).Count
Write-Host ''
Write-Host ('{0} failed, {1} passed' -f $failed, @($results | Where-Object { $_ -like 'PASS*' }).Count)
exit ([int]($failed -gt 0))
