#!/bin/bash
set -e
set -x

cargo build
# cross build --target aarch64-unknown-linux-gnu
# cross build --target armv7-unknown-linux-gnueabihf

# cargo fmt --all
cargo fmt --all -- --check

cargo clippy --all -- -D warnings

cargo test --all

CWD=$(pwd)

cd "$CWD/lib/config-wasm"
wasm-pack build --release -t nodejs --scope fs
cd tests/
npm install
npm test

cd "$CWD//lib/hdr-histogram-wasm"
wasm-pack build --release -t nodejs --scope fs
cd tests/
npm install
npm test

cd "$CWD"

cargo deny check --hide-inclusion-graph license sources advisories
