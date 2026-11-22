param(
    [string]$BinDir = (Join-Path (Split-Path $PSScriptRoot -Parent) 'target\release')
)
$ErrorActionPreference = 'Stop'
$root = Split-Path $PSScriptRoot -Parent
$version = '0.2.0-beta.1'
$output = Join-Path $root 'release'
$stage = Join-Path $output ('package-' + $version)
New-Item -ItemType Directory -Force $stage | Out-Null
Copy-Item -LiteralPath (Join-Path $BinDir 'hypersearch-gui.exe') -Destination (Join-Path $stage 'HyperSearch.exe')
Copy-Item -LiteralPath (Join-Path $BinDir 'hypersearch.exe') -Destination (Join-Path $stage 'hypersearch-cli.exe')
foreach ($name in @('BETA-0.2.md','BETA_TEST_REPORT.md','README.md','UI_PERFORMANCE.md','STORAGE_ANALYSIS.md','CACHE.md','MEMORY_MODES.md','VALIDATION.md','PERFORMANCE.md','CHANGELOG.md','THIRD_PARTY_NOTICES.md')) {
    Copy-Item -LiteralPath (Join-Path $root $name) -Destination (Join-Path $stage $name)
}
$revision = git -C $root rev-parse HEAD
if ($LASTEXITCODE) { throw 'Cannot identify source revision' }
$manifest = @{
    version = $version
    source_revision = $revision
    platform = 'Windows x64'
    signed = $false
    gui_sha256 = (Get-FileHash -LiteralPath (Join-Path $stage 'HyperSearch.exe') -Algorithm SHA256).Hash.ToLowerInvariant()
    cli_sha256 = (Get-FileHash -LiteralPath (Join-Path $stage 'hypersearch-cli.exe') -Algorithm SHA256).Hash.ToLowerInvariant()
}
$manifest | ConvertTo-Json | Set-Content -LiteralPath (Join-Path $stage 'manifest.json') -Encoding UTF8
$base = 'HyperSearch-' + $version + '-windows-x64'
Copy-Item -LiteralPath (Join-Path $stage 'HyperSearch.exe') -Destination (Join-Path $output ($base + '.exe'))
Compress-Archive -Path (Join-Path $stage '*') -DestinationPath (Join-Path $output ($base + '.zip')) -Force
$checksums = foreach ($extension in @('.exe','.zip')) {
    $name = $base + $extension
    (Get-FileHash -LiteralPath (Join-Path $output $name) -Algorithm SHA256).Hash.ToLowerInvariant() + '  ' + $name
}
$checksums | Set-Content -LiteralPath (Join-Path $output 'SHA256SUMS.txt') -Encoding ASCII
Write-Host ('Packaged ' + $base)
