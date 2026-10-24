#!/usr/bin/env bash
# 逐条精确跑失败测试并抓 panic 消息（libtest 只支持单 --filter，故串行）。
#
# 两版修正记录（首版两处都错，记下来别重犯）：
#   ① 过滤失效：cargo 的风格建议以 `3079 -let base = …` 形式落盘，**行首是
#      行号不是 "warning"**，故 `grep -v "^warning"` 全漏。改法：先
#      `--no-run` 编译一次把警告一次性打完并丢弃，之后每条都是纯运行期输出。
#   ② panic 消息抓不到（更隐蔽）：加了 `2>/dev/null` 想滤掉编译噪声，结果
#      **panic 消息正是走 stderr 的** —— 于是一条都没抓到，只剩
#      `test … FAILED` 一行。改法：**不丢 stderr**，改用 `-A 8` 抓
#      `panicked at` 之后的 8 行（消息可能多行，`-A` 比分隔符稳）。
#
# 输出流式落盘：每条跑完立刻 append，异常中断也留得下已完成部分。
set -u
cd "$(dirname "$0")/.."   # kernel/
OUT=target-wb/fail_detail.txt

cargo test --lib --no-run >/dev/null 2>&1 || { echo "PRECOMPILE_FAILED" > "$OUT"; exit 1; }

: > "$OUT"
n=0
while IFS= read -r full; do
    [ -z "$full" ] && continue
    n=$((n + 1))
    echo "########## [$n] $full" >> "$OUT"
    timeout 300 cargo test --lib -- --exact --nocapture --test-threads=1 "$full" 2>&1 \
        | grep -v "^warning:\|^ *-->\|^ *=\|^help:\|^ *|" \
        | grep -A 8 "panicked at" >> "$OUT"
    echo "" >> "$OUT"
done < target-wb/failed_names.txt
echo "ALLDONE n=$n" >> "$OUT"