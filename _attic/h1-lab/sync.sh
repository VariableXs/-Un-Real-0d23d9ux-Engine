#!/usr/bin/env bash
# AI-H1 验证舱同步：把主树 h1star 已落位的模块镜像到隔离舱。
# 用法：bash sync.sh [m1] [m2] ...   （不带参数 = 同步全部已存在模块）
set -e
ROOT="$(cd "$(dirname "$0")" && pwd)"
SRC="$ROOT/../../kernel/varix/src/h1star"
DST="$ROOT/src/h1star"
mkdir -p "$DST"
if [ "$#" -gt 0 ]; then MODULES="$@"; else
  MODULES=$(ls "$SRC" | grep '\.rs$' | sed 's/\.rs$//' | grep -v '^mod$' || true)
fi
for m in $MODULES; do
  [ -f "$SRC/$m.rs" ] && cp "$SRC/$m.rs" "$DST/$m.rs"
done
# 舱内 mod.rs 只声明已同步模块（主树 mod.rs 声明全集，主树未落位文件不拷）。
{
  echo "//! 隔离舱 mod.rs（sync.sh 生成——勿手改）：已落位模块逐个声明。"
  echo "pub mod h1base;"
  for m in $MODULES; do
    [ "$m" = "h1base" ] && continue
    [ -f "$DST/$m.rs" ] && echo "pub mod $m;"
  done
} > "$DST/mod.rs"
echo "synced: $MODULES"
