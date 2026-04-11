#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "$0")/../.." && pwd)"
CMDLIB_DIR="${ROOT_DIR}/CmdLib"

cargo build --manifest-path "${CMDLIB_DIR}/Cargo.toml" --release --example Lchika
"${CMDLIB_DIR}/target/release/examples/Lchika"