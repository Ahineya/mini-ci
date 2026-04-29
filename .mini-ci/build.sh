#!/bin/sh
set -euo pipefail

echo "==> Installing frontend dependencies"
cd frontend
# Fresh tree avoids broken .bin stubs (e.g. vite missing under node_modules/vite).
rm -rf node_modules
npm ci

echo "==> Building frontend"
npm run build
cd ..

echo "==> Building mini-ci (release)"
cargo build --release

echo "==> Copying release binary to dist/"
mkdir -p dist
cp -f target/release/mini-ci dist/mini-ci

echo "==> Done!"
ls -lh target/release/mini-ci dist/mini-ci
