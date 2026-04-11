#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "$0")/../.." && pwd)"
CMDLIB_DIR="${ROOT_DIR}/CmdLib"

echo "[1/5] Building cmdlibd"
cargo build --manifest-path "${CMDLIB_DIR}/Cargo.toml" --release --bin cmdlibd

echo "[2/5] Installing binary"
sudo install -m 0755 "${CMDLIB_DIR}/target/release/cmdlibd" /usr/local/bin/cmdlibd

echo "[3/5] Installing systemd unit"
sudo install -m 0644 "${CMDLIB_DIR}/systemd/cmdlibd.service" /etc/systemd/system/cmdlibd.service

echo "[4/5] Reloading daemon"
sudo systemctl daemon-reload

echo "[5/5] Enabling and starting cmdlibd"
sudo systemctl enable --now cmdlibd.service

echo "Done. Check status with:"
echo "  sudo systemctl status cmdlibd.service"