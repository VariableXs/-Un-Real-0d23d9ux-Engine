#!/usr/bin/env bash
# Varix 编译闸门（一次性给出：错误数 / 分类 / 明细文件）。
#
# 为什么需要脚本而不是直接敲 cargo：仓内模块数已过 5,300，单次全量
# `cargo check --lib` 约 10~14 分钟，且并行施工会话持续写入——手工反复
# 跑 cargo 既慢又容易看到「上一轮的过期快照」。本脚本固定：独立 target 目录
# （不与并行会话抢构建锁）、关闭增量（避免脏增量误绿）、统一抓取格式。
#
# 用法:
#   bash _tools/gate.sh          # 只看汇总
#   bash _tools/gate.sh --head   # 汇总 + 前 30 条明细
set -u
cd "$(dirname "$0")/.." || exit 1

OUT=_tools/_gate_err.txt
LOG=_tools/_gate_full.txt

CARGO_INCREMENTAL=0 \
CARGO_TARGET_DIR=target_wb1022 \
RUST_MIN_STACK=67108864 \
  cargo check --lib --message-format short > "$LOG" 2>&1

grep -E "^(varix|error)" "$LOG" | grep -E ": error" > "$OUT"

n=$(wc -l < "$OUT" | tr -d ' ')
echo "=== 错误总数: $n ==="
if [ "$n" != "0" ]; then
  echo "--- 按错误码 ---"
  sed -E 's/.*(error\[E[0-9]+\]).*/\1/' "$OUT" | grep -o 'E[0-9]*' | sort | uniq -c | sort -rn
  echo "--- 按文件（前 20）---"
  sed -E 's/^varix.src.([^:]+):.*/\1/' "$OUT" | sort | uniq -c | sort -rn | head -20
  if [ "${1:-}" = "--head" ]; then
    echo "--- 明细 ---"
    head -30 "$OUT"
  fi
fi
