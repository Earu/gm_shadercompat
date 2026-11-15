#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")"

if ! command -v cargo >/dev/null; then
	echo "cargo not found, install Rust from https://rustup.rs" >&2
	exit 1
fi

if [ -z "${CXX:-}" ] && ! command -v c++ >/dev/null && ! command -v g++ >/dev/null && ! command -v clang++ >/dev/null; then
	echo "no C++ compiler found, install g++ or clang++, or point CXX at one" >&2
	exit 1
fi

cargo build --release
echo "built target/release/gm_shadercompat"
