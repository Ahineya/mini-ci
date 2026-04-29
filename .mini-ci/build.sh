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

echo "==> Done! Binary at target/release/mini-ci"
ls -lh target/release/mini-ci
