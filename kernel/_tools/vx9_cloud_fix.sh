#!/bin/bash
# 云端 rust 工具链修复：补齐 cargo 组件
source ~/.cargo/env 2>/dev/null || true
rustup toolchain install stable --profile default --force --component cargo clippy rust-src 2>&1 | tail -3
hash -r
echo "CARGO=$(cargo --version)"
echo "RUSTC=$(rustc --version)"
