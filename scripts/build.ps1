$ErrorActionPreference = 'Stop'
$cargo = Join-Path $env:USERPROFILE '.cargo\bin\cargo.exe'
$rustc = Join-Path $env:USERPROFILE '.cargo\bin\rustc.exe'
if (!(Test-Path -LiteralPath $cargo)) { $cargo = (Get-Command cargo -ErrorAction Stop).Source; $rustc = (Get-Command rustc -ErrorAction Stop).Source }
$oldTarget = $env:CARGO_TARGET_DIR
$oldFlags = $env:RUSTFLAGS
$oldPath = $env:PATH
try {
    $env:CARGO_TARGET_DIR = Join-Path $env:LOCALAPPDATA 'HyperSearch\build'
    if ((& $rustc -vV) -match 'host: x86_64-pc-windows-gnu') {
        $env:RUSTFLAGS = '-C link-self-contained=yes'
        $sysroot = & $rustc --print sysroot
        $env:PATH = "$sysroot\lib\rustlib\x86_64-pc-windows-gnu\bin\self-contained;" + $env:PATH
    }
    Push-Location (Split-Path $PSScriptRoot -Parent)
    try {
        & $cargo test --workspace
        if ($LASTEXITCODE) { throw 'Tests failed' }
        & $cargo build --release
        if ($LASTEXITCODE) { throw 'Build failed' }
        New-Item -ItemType Directory -Force dist | Out-Null
        Copy-Item -LiteralPath (Join-Path $env:CARGO_TARGET_DIR 'release\hypersearch.exe') -Destination dist\hypersearch.exe
        Copy-Item -LiteralPath (Join-Path $env:CARGO_TARGET_DIR 'release\hypersearch-gui.exe') -Destination dist\hypersearch-gui.exe
        Write-Host 'Built dist\hypersearch.exe and dist\hypersearch-gui.exe'
    } finally { Pop-Location }
} finally { $env:CARGO_TARGET_DIR=$oldTarget; $env:RUSTFLAGS=$oldFlags; $env:PATH=$oldPath }
