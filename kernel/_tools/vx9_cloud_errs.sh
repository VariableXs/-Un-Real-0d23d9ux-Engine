#!/bin/bash
# 云端编译错误按文件分组统计
source ~/.cargo/env
cd /root/autodl-tmp/work/varix
cargo check -j 96 2>&1 | grep -E "^error(\[|:)" -A4 | grep -oE "src/[a-zA-Z0-9_/]+\.rs:[0-9]+:[0-9]+" | cut -d: -f1 | sort | uniq -c | sort -rn | head -40
