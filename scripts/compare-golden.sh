#!/usr/bin/env bash
# AURORA-1000 步骤 0018 · QEMU golden 比对门
# 用法: scripts/compare-golden.sh <串口输出文件>
# 与 fixtures/qemu-smoke.golden 逐行比对；golden 缺失则提示先固化。
set -euo pipefail
cd "$(dirname "$0")/.."

GOLDEN="fixtures/qemu-smoke.golden"
ACTUAL="${1:-}"

if [ -z "$ACTUAL" ] || [ ! -f "$ACTUAL" ]; then
    echo "用法: $0 <串口输出文件>" >&2
    exit 2
fi
if [ ! -f "$GOLDEN" ]; then
    echo "SKIP: $GOLDEN 不存在 —— QEMU 首启（步骤 0014/0015）待环境具备后固化" >&2
    exit 3
fi

if diff -u "$GOLDEN" "$ACTUAL"; then
    echo "OK: 串口输出与 golden 一致"
else
    echo "FAIL: 串口输出偏离 golden（检查启动横幅/自检行）" >&2
    exit 1
fi
