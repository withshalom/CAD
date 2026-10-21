# Build and test the CAD core and the `cad` test build on Windows.
# Needs Rust from https://rustup.rs (default MSVC toolchain).
# Usage (PowerShell):  .\scripts\build-windows.ps1
$ErrorActionPreference = "Stop"
Set-Location (Split-Path $PSScriptRoot -Parent)

cargo test --release
if ($LASTEXITCODE -ne 0) { throw "tests failed" }
cargo build --release
if ($LASTEXITCODE -ne 0) { throw "build failed" }

$out = "dist\windows-x86_64"
New-Item -ItemType Directory -Force -Path $out | Out-Null
Copy-Item "target\release\cad.exe" $out -Force
Write-Host "Built $out\cad.exe"
& "$out\cad.exe" version
