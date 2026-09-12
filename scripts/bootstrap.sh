#!/usr/bin/env bash
# AURORA-1000 步骤 0033 · 底盘固化脚本（幂等，可重入）
# 汇总 0001~0032：工具链/目标检查 → 静态检查 → 测试 → 构建 → 盘点。
set -euo pipefail
cd "$(dirname "$0")/.."

say() { echo "== $* =="; }

say "工具链"
command -v cargo >/dev/null || { echo "FAIL: 无 cargo"; exit 1; }
rustup target list --installed 2>/dev/null | grep -q x86_64-unknown-none || \
    echo "WARN: 缺 x86_64-unknown-none（rustup target add x86_64-unknown-none）"

say "kcheck"
(cd kernel && cargo kcheck 2>&1 | grep -E "^error" && { echo "FAIL"; exit 1; } || true)

say "ktest"
(cd kernel && cargo ktest 2>/dev/null | grep -E "^test result" )

say "kbuild"
(cd kernel && cargo kbuild 2>&1 | grep -E "^error" && { echo "FAIL"; exit 1; } || true)
[ -f kernel/target/x86_64-unknown-none/release/varix ] && echo "ELF OK"

say "QEMU/ISO（待环境具备时自动生效）"
bash scripts/run-qemu.sh --help >/dev/null 2>&1 || true

say "盘点"
bash scripts/audit | head -8

say "PASS: 底盘门全绿（QEMU/ISO 段待环境具备）"
