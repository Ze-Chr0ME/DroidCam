$ErrorActionPreference = 'Stop'
$root = Split-Path $PSScriptRoot -Parent
Push-Location $root
try {
    python scripts/rust_licenses.py
    if ($LASTEXITCODE -ne 0) { throw 'License collection failed.' }
    cargo test --locked --workspace
    if ($LASTEXITCODE -ne 0) { throw 'Rust tests failed.' }
    cargo build --locked --release -p camerabridge-desktop
    if ($LASTEXITCODE -ne 0) { throw 'Rust Windows build failed.' }
    $package = Join-Path $root 'dist\CameraBridge-Rust'
    New-Item -ItemType Directory -Force -Path $package | Out-Null
    Copy-Item -LiteralPath (Join-Path $root 'target\release\CameraBridge.exe') -Destination $package -Force
    New-Item -ItemType Directory -Force -Path (Join-Path $package 'third_party') | Out-Null
    Copy-Item -LiteralPath (Join-Path $root 'third_party\unitycapture') -Destination (Join-Path $package 'third_party') -Recurse -Force
    Copy-Item -LiteralPath (Join-Path $root 'third_party\licenses') -Destination (Join-Path $package 'third_party') -Recurse -Force
    Copy-Item -LiteralPath (Join-Path $root 'README.md'),(Join-Path $root 'THIRD_PARTY.md') -Destination $package -Force
    Copy-Item -LiteralPath (Join-Path $root 'assets'),(Join-Path $root 'docs') -Destination $package -Recurse -Force
    Write-Host "Windows app: $package\CameraBridge.exe"
} finally { Pop-Location }
