<#
.SYNOPSIS
Builds the release zip in out\. Its root is the Steam game folder, so everything sits under Game\.

.DESCRIPTION
Game files aren't in this repo, so the item param and text are built from your own copies:
- StockItemParam: ItemParam.param from Load Loose Param Files (Nexus 1092), unmodified.
- EnglishText: menu\text\english from a UXM-unpacked game.
#>
param(
    [string]$StockItemParam = 'D:\DS2-param-backup\ItemParam.param',
    [string]$EnglishText = 'C:\Games\DS2-dev\Game\menu\text\english'
)
$ErrorActionPreference = 'Stop'
Set-Location (Split-Path $PSScriptRoot)

# Nexus 1092's ItemParam. Anything else would ship someone else's balance changes to every user.
$stockHash = 'DA4DCDDC7275F1C2F6B788F1DA432E99856DEC8B1BAC2C3627AFB9EE0EF358ED'
if ((Get-FileHash $StockItemParam -Algorithm SHA256).Hash -ne $stockHash) {
    throw "$StockItemParam isn't the unmodified ItemParam from Nexus 1092"
}

function Invoke-Cargo {
    & cargo @args
    if ($LASTEXITCODE -ne 0) { throw "cargo $args failed" }
}

Invoke-Cargo test --workspace --quiet
Invoke-Cargo build --release -p pocket-bonfire --quiet
$metadata = (& cargo metadata --format-version 1) -join "`n" | ConvertFrom-Json
$version = ($metadata.packages | Where-Object name -eq 'pocket-bonfire').version

$stage = Join-Path 'out' "stage-$version"
if (Test-Path $stage) { Remove-Item $stage -Recurse -Force }
$textDir = Join-Path $stage 'Game\mod\menu\text\english'
$dllDir = Join-Path $stage 'Game\dllMods'
$paramDir = Join-Path $stage 'Game\Param'
New-Item -ItemType Directory -Force $textDir, $dllDir, $paramDir | Out-Null

Copy-Item 'target\release\pocket_bonfire.dll' $dllDir
Invoke-Cargo run --quiet -p ds2 --example build_item_param -- $StockItemParam (Join-Path $paramDir 'ItemParam.param')
Invoke-Cargo run --quiet -p ds2 --example build_item_text -- $EnglishText $textDir
Copy-Item 'README.md', 'LICENSE' $stage

# The hook library's licence requires its notice in binary releases, and the other crates linked into
# the DLL are MIT or dual-licensed, so all of them go in.
$notices = New-Object System.Text.StringBuilder
[void]$notices.AppendLine('Third-party software linked into pocket_bonfire.dll')
foreach ($name in 'minhook', 'log', 'once_cell', 'pin-project-lite', 'tracing', 'tracing-core') {
    $package = $metadata.packages | Where-Object name -eq $name
    if (-not $package) { throw "$name is no longer a dependency; update this list" }
    $dir = Split-Path $package.manifest_path
    $license = 'LICENSE-MIT', 'LICENSE' | ForEach-Object { Join-Path $dir $_ } | Where-Object { Test-Path $_ } | Select-Object -First 1
    if (-not $license) { throw "no licence file found for $name" }
    [void]$notices.AppendLine("`n===== $name $($package.version) =====`n")
    [void]$notices.AppendLine((Get-Content $license -Raw))
    if ($name -eq 'minhook') {
        [void]$notices.AppendLine("`n===== MinHook, bundled by the minhook crate =====`n")
        [void]$notices.AppendLine((Get-Content (Join-Path $dir 'minhook\LICENSE.txt') -Raw))
    }
}
Set-Content (Join-Path $stage 'THIRD-PARTY-NOTICES.txt') $notices.ToString() -Encoding utf8

$zip = Join-Path (Resolve-Path 'out') "pocket-bonfire-$version.zip"
if (Test-Path $zip) { Remove-Item $zip }
Add-Type -AssemblyName System.IO.Compression.FileSystem
[IO.Compression.ZipFile]::CreateFromDirectory((Resolve-Path $stage), $zip)

$root = (Resolve-Path $stage).Path
Get-ChildItem $stage -Recurse -File | ForEach-Object { '{0,10}  {1}' -f $_.Length, $_.FullName.Substring($root.Length + 1) }
"Wrote $zip, sha256 $((Get-FileHash $zip -Algorithm SHA256).Hash)"
