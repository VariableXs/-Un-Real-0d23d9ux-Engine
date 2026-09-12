#!/usr/bin/env bash
# AURORA-1000 步骤 0030 · 域收口门
# 用法: scripts/domain-gate.sh <域名>   （如 display / render2d / aurora::display）
# 门禁：kcheck 零 error → 该域 ktest 全 PASS → kbuild 零 error。
set -euo pipefail
cd "$(dirname "$0")/../kernel"

DOMAIN="${1:?用法: domain-gate.sh <域名>}"

echo "[gate 1/3] cargo kcheck"
cargo kcheck 2>&1 | grep -E "^error" && { echo "FAIL: kcheck 有 error" >&2; exit 1; } || true

echo "[gate 2/3] cargo ktest -- $DOMAIN::"
cargo ktest --target x86_64-pc-windows-msvc -- "$DOMAIN::"

echo "[gate 3/3] cargo kbuild"
cargo kbuild 2>&1 | grep -E "^error" && { echo "FAIL: kbuild 有 error" >&2; exit 1; } || true

echo "PASS: 域 $DOMAIN 收口门通过"
