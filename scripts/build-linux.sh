#!/usr/bin/env sh
# Build and test the CAD core and the `cad` test build on Linux (or macOS).
# Needs Rust from https://rustup.rs.
set -eu
cd "$(dirname "$0")/.."

cargo test --release
cargo build --release

arch="$(uname -m)"
os="$(uname -s | tr '[:upper:]' '[:lower:]')"
out="dist/${os}-${arch}"
mkdir -p "$out"
cp target/release/cad "$out/"
echo "Built $out/cad"
"$out/cad" version
