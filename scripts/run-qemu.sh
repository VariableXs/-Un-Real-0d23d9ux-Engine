#!/usr/bin/env bash
# AURORA-1000 步骤 0011 · QEMU 启动脚本（ISO 引导）
# 依赖: qemu-system-x86_64（winget install SoftwareFreedomConservancy.QEMU）
set -euo pipefail
cd "$(dirname "$0")/.."

bash scripts/make-iso.sh
exec qemu-system-x86_64 \
    -cdrom varix.iso \
    -serial stdio -no-reboot -no-shutdown \
    -m 512M -M q35 "$@"
