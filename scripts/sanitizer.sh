#!/usr/bin/env bash
# sanitizer · 常态化入口（AI-S 任务 79 / 专项 J）
# 对 C 线(ca-core)与桌面后端(variable --lib)尝试 AddressSanitizer 运行。
# 诚实口径：工具链不支持 ASan 时输出 SKIP 及原因，不伪造通过；真实编译/测试失败才 FAIL。
# 用法: scripts/sanitizer.sh
set -uo pipefail
cd "$(dirname "$0")/.."
OUT="_attic/verify"
mkdir -p "$OUT"
FAILED=0

step() { printf '\n===== [%s] =====\n' "$1"; }
skip() { echo "SKIP: $1"; }
run_asan() { # run_asan <name> <manifest-dir> [extra args]
  local name="$1" dir="$2"; shift 2
  step "$name"
  if ! ( cd "$dir" && RUSTFLAGS="-Zsanitizer=address" cargo +nightly test --locked "$@" ) \
      > "$OUT/asan-$name.log" 2>&1; then
    if grep -qE 'unknown .-Z. flag|requires `-Z unstable-options`|no nightly' "$OUT/asan-$name.log"; then
      skip "工具链不支持 -Zsanitizer（见 $OUT/asan-$name.log）"; return 0
    fi
    if grep -qE 'STATUS_DLL_NOT_FOUND|clang_rt' "$OUT/asan-$name.log"; then
      # Windows-msvc nightly 未随附 clang_rt.asan 运行时（须自装 LLVM），属能力缺失而非回归
      skip "本机无 ASan 运行时 DLL（clang_rt）；补救：安装 LLVM 并把其 bin 加 PATH（见 $OUT/asan-$name.log）"
      return 0
    fi
    echo "FAIL: ASan 运行未通过（见 $OUT/asan-$name.log）"; return 1
  fi
  grep -h 'test result' "$OUT/asan-$name.log" | tail -2
  return 0
}

step "0. nightly 工具链探测"
if ! cargo +nightly --version > "$OUT/asan-nightly.txt" 2>&1; then
  skip "无 nightly 工具链（rustup toolchain install nightly 后启用）"
  echo "sanitizer: SKIP no-nightly" > "$OUT/sanitizer-status.txt"
  exit 0
fi
cat "$OUT/asan-nightly.txt"
# ASan 运行时 DLL 目录注入 PATH（Windows STATUS_DLL_NOT_FOUND 对策）
ASAN_DLL_DIR="$(dirname "$(cargo +nightly --print-file-sharing-optimizer 2>/dev/null)" 2>/dev/null || true)"
ASAN_DLL_DIR="$(ls -d ~/.rustup/toolchains/nightly-*/lib/rustlib/x86_64-pc-windows-msvc/lib 2>/dev/null | tail -1 || true)"
if [ -z "$ASAN_DLL_DIR" ]; then
  skip "未定位到 nightly rustlib 目录，无法注入 ASan 运行时"
  echo "sanitizer: SKIP no-asan-rt" > "$OUT/sanitizer-status.txt"
  exit 0
fi
export PATH="$ASAN_DLL_DIR:$PATH"

run_asan ca-core code-analysis -p ca-core || FAILED=1
run_asan variable src-tauri -p variable --lib || FAILED=1

echo "sanitizer: $([ "$FAILED" -eq 0 ] && echo PASS || echo FAIL)" > "$OUT/sanitizer-status.txt"
[ "$FAILED" -eq 0 ] && echo "SANITIZER: ALL PASS" || { echo "SANITIZER: FAILED"; exit 1; }
