#!/usr/bin/env bash
# double-confirm · 关键路径双确认制（AI-S 任务 79 / 专项 J）
# 危险操作包装器：执行前要求人工输入 CONFIRM，防误触。
# 用法: scripts/double-confirm.sh <命令...>
set -euo pipefail
CMD="$*"
[ -z "$CMD" ] && { echo "用法: scripts/double-confirm.sh <命令...>"; exit 2; }
echo "即将执行（关键路径双确认）："
echo "  $CMD"
printf '输入 CONFIRM 继续: '
read -r ans
[ "$ans" = "CONFIRM" ] || { echo "已取消（未双确认）"; exit 1; }
sh -c "$CMD"
