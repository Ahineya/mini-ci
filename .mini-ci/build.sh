#!/bin/sh
set -euo pipefail

echo "==> Installing frontend dependencies"
cd frontend
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
