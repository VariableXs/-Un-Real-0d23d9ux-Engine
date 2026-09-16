#!/usr/bin/env bash
# sanitizer · 常态化入口（AI-S 任务 79 / 专项 J）
# 对 C 线(ca-core)与桌面后端(variable --lib)尝试 AddressSanitizer 运行。
# 诚实口径：工具链不支持 ASan 时输出 SKIP 及原因，不伪造通过。
# 用法: scripts/sanitizer.sh
set -uo pipefail
cd "$(dirname "$0")/.."
OUT="_attic/verify"
mkdir -p "$OUT"
FAILED=0

step() { printf '\n===== [%s] =====\n' "$1"; }
note() { echo "$1"; FAILED=1; }

step "1. 工具链 ASan 能力探测"
if rustup toolchain list 2>/dev/null | grep -q nightly; then
  TC="+nightly"; echo "nightly 可用"
else
  TC=""; echo "SKIP: 无 nightly 工具链（-Zsanitizer 需要 nightly）。归档原因后放行本段。"
  echo "sanitizer: SKIP no-nightly" > "$OUT/sanitizer-status.txt"
  exit 0
fi

step "2. ca-core ASan 测试"
if ( cd code-analysis && cargo $TC test -Z sanitizer=address --locked -p ca-core ) \
    > "$OUT/asan-ca-core.log" 2>&1; then
  grep -h 'test result' "$OUT/asan-ca-core.log" | tail -2
else
  note "FAIL: ca-core ASan 运行未通过（见 $OUT/asan-ca-core.log）"
fi

step "3. variable --lib ASan 测试"
if ( cd src-tauri && cargo $TC test -Z sanitizer=address --locked -p variable --lib ) \
    > "$OUT/asan-variable.log" 2>&1; then
  grep -h 'test result' "$OUT/asan-variable.log" | tail -2
else
  note "FAIL: variable ASan 运行未通过（见 $OUT/asan-variable.log）"
fi

echo "sanitizer: SKIP no-nightly" > "$OUT/sanitizer-status.txt"
[ "$FAILED" -eq 0 ] && echo "SANITIZER: ALL PASS" || { echo "SANITIZER: FAILED"; exit 1; }
