#!/usr/bin/env bash
# iso_check.sh — 单文件隔离校验（编译 + 跑该文件自带判据），2 秒级。
#
# 用法: bash _tools/iso_check.sh <候选文件相对路径> <判据聚合函数名> [依赖模块,...]
#   bash _tools/iso_check.sh varix/src/cgpu/cga15_csconv.rs run_cga15_checks
#   bash _tools/iso_check.sh varix/src/cgpu/cga17_yuvdec.rs run_cga17_checks cga15_csconv
#
# 原理: 在 _iso_cgpu/src/cand.rs 写一个 #[path] 垫片指向候选文件，
#       用真实 checks.rs 编译并执行聚合函数。不复制源码 → 不会与主仓漂移。
#
# 依赖模块（第三参数）在垫片里以 `cand::<name>` 平行挂载——因为候选文件被挂在
# `cand::inner` 下，它写的 `use super::<name>::...` 正好解析到 `cand::<name>`，
# 与它在主仓里 `use super::<name>::...`（super = cgpu）同形。
set -u
cd "$(dirname "$0")/.." || exit 1

SRC="$1"
FN="$2"
DEPS="${3:-}"
ROOT="$(pwd)"
SHIM="$ROOT/_iso_cgpu/src/cand.rs"
REL="../../${SRC}"

{
  echo "// 自动生成（_tools/iso_check.sh）——不要手改。"
  for d in ${DEPS//,/ }; do
    echo "#[path = \"../../varix/src/cgpu/${d}.rs\"]"
    echo "pub mod ${d};"
    echo ""
  done
  echo "#[path = \"${REL}\"]"
  echo "pub mod inner;"
  echo ""
  echo "pub fn run_candidate_checks() -> crate::checks::CheckSet {"
  echo "    inner::${FN}()"
  echo "}"
} > "$SHIM"

cd "$ROOT/_iso_cgpu" || exit 1
cargo test --offline --quiet -- --nocapture 2>&1 | tail -40
