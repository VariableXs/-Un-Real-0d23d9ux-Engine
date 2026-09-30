#!/usr/bin/env bash
# AURORA-1000 步骤 0025 · 性能基准 harness
# 对每个基准跑 3 次取中位数，输出 docs/perf/基线.json。
# 宿主基准基于 ktest 计时（启动开销已剔除时用 VARIX_BENCH_FAST=1 只跑内核内基准）。
set -euo pipefail
cd "$(dirname "$0")/.."

mkdir -p docs/perf

bench_one() {
    # $1 名称；把命令跑 3 次，取中位（秒，2 位小数）
    local name="$1"; shift
    local times=()
    for _ in 1 2 3; do
        local s=$(date +%s%N)
        "$@" >/dev/null 2>&1
        times+=($((($(date +%s%N) - s) / 1000000)))
    done
    printf '%s\n' "${times[@]}" | sort -n | sed -n 2p
}

KT=$(bench_one "ktest" bash -c 'cd kernel && cargo ktest 2>/dev/null | tail -1')
KB=$(bench_one "kbuild" bash -c 'cd kernel && cargo kbuild 2>/dev/null | tail -1')
KC=$(bench_one "kcheck" bash -c 'cd kernel && cargo kcheck 2>/dev/null | tail -1')

TS=$(date -u +%Y-%m-%dT%H:%M:%SZ)
cat > docs/perf/基线.json <<EOF
{
  "timestamp": "$TS",
  "note": "中位数(3次)，单位毫秒；宿主机差异大，仅供回归相对比较",
  "ktest_full_ms": ${KT:-null},
  "kbuild_full_ms": ${KB:-null},
  "kcheck_full_ms": ${KC:-null}
}
EOF
echo "OK: docs/perf/基线.json（ktest=${KT}ms kbuild=${KB}ms kcheck=${KC}ms）"
