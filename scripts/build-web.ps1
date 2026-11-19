# Builds the web app into apps/erpcraft-web/www (serve that folder with any static server).
# Needs: rustup target add wasm32-unknown-unknown
#        cargo install wasm-bindgen-cli --version <the wasm-bindgen version in Cargo.lock>
$ErrorActionPreference = "Stop"
$root = Split-Path -Parent $PSScriptRoot
Set-Location $root
$env:CARGO_TARGET_DIR = "target-web"
cargo build -p erpcraft-web --target wasm32-unknown-unknown --release
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
wasm-bindgen --target web --no-typescript --out-dir apps/erpcraft-web/www/pkg target-web/wasm32-unknown-unknown/release/erpcraft_web.wasm
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
Write-Host "Built. Serve it with: python -m http.server 8765 --directory apps/erpcraft-web/www"
