<#
.SYNOPSIS
Builds the release zip in out\. Its root is the Steam game folder, so everything sits under Game\.

.DESCRIPTION
Lazy Loader goes in the zip so the mod is a single download. Its lazyLoad.ini is left out on purpose:
Lazy Loader works without one, and this way the zip can't overwrite someone's load order.
#>
param(
    [Parameter(Mandatory = $true)]
    [string]$LazyLoaderZip
)
$ErrorActionPreference = 'Stop'
$LazyLoaderZip = (Resolve-Path $LazyLoaderZip).Path
Set-Location (Split-Path $PSScriptRoot)
Add-Type -AssemblyName System.IO.Compression.FileSystem

# Lazy Loader 2.0.0's dinput8.dll, the version this was tested with.
$lazyLoaderHash = 'F7B482F66C5E380AFE0D6EB6C9DC3CD2CC55E2652064E10BA163E948D45ADDC1'

function Invoke-Cargo {
    & cargo @args
    if ($LASTEXITCODE -ne 0) { throw "cargo $args failed" }
}

function Read-ZipEntry($zip, $name) {
    $entry = $zip.Entries | Where-Object { $_.FullName -replace '\\', '/' -eq $name }
    if (-not $entry) { throw "$name isn't in the Lazy Loader zip" }
    return $entry
}

Invoke-Cargo test --workspace --quiet
Invoke-Cargo build --release -p pocket-bonfire --quiet
$metadata = (& cargo metadata --format-version 1) -join "`n" | ConvertFrom-Json
$version = ($metadata.packages | Where-Object name -eq 'pocket-bonfire').version

$stage = Join-Path 'out' "stage-$version"
if (Test-Path $stage) { Remove-Item $stage -Recurse -Force }
$dllDir = Join-Path $stage 'Game\dllMods'
New-Item -ItemType Directory -Force $dllDir | Out-Null

Copy-Item 'target\release\pocket_bonfire.dll' $dllDir
Copy-Item 'README.md', 'CHANGELOG.md', 'LICENSE' $stage

$dinput8 = Join-Path $stage 'Game\dinput8.dll'
$zip = [IO.Compression.ZipFile]::OpenRead($LazyLoaderZip)
try {
    [IO.Compression.ZipFileExtensions]::ExtractToFile((Read-ZipEntry $zip 'v2.0.0/dinput8.dll'), (Join-Path (Resolve-Path $stage) 'Game\dinput8.dll'))
    $reader = New-Object IO.StreamReader((Read-ZipEntry $zip 'v2.0.0/LICENSES.txt').Open())
    $lazyLoaderLicenses = $reader.ReadToEnd()
    $reader.Dispose()
}
finally {
    $zip.Dispose()
}
if ((Get-FileHash $dinput8 -Algorithm SHA256).Hash -ne $lazyLoaderHash) {
    throw "$LazyLoaderZip doesn't hold the tested Lazy Loader 2.0.0"
}

# The hook library's licence requires its notice in binary releases, and the other crates linked into
# the DLL are MIT or dual-licensed, so all of them go in, along with Lazy Loader's.
$notices = New-Object System.Text.StringBuilder
[void]$notices.AppendLine('Third-party software in this release')
[void]$notices.AppendLine("`n===== Lazy Loader 2.0.0 (Game\dinput8.dll), by Church Guard =====`n")
[void]$notices.AppendLine($lazyLoaderLicenses)
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

$out = Join-Path (Resolve-Path 'out') "pocket-bonfire-$version.zip"
if (Test-Path $out) { Remove-Item $out }
[IO.Compression.ZipFile]::CreateFromDirectory((Resolve-Path $stage), $out)

$root = (Resolve-Path $stage).Path
Get-ChildItem $stage -Recurse -File | ForEach-Object { '{0,10}  {1}' -f $_.Length, $_.FullName.Substring($root.Length + 1) }
"Wrote $out, sha256 $((Get-FileHash $out -Algorithm SHA256).Hash)"
