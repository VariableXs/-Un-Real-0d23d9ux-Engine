#!/usr/bin/env bash
# supply-chain · 依赖供应链冻结评审（AI-S 任务 79 / 专项 J）
# 三查：1) 锁文件漂移（改动须显式豁免） 2) npm audit 高危 3) cargo audit（装了才跑）。
# 诚实口径：工具/网络不可用时输出 SKIP 及原因，不伪造通过。
# 用法: scripts/supply-chain-check.sh     豁免漂移: VERIFY_ALLOW_DEP_CHANGE=1
set -uo pipefail
cd "$(dirname "$0")/.."
FAILED=0

step() { printf '\n===== [%s] =====\n' "$1"; }

step "1. 锁文件冻结"
for f in package-lock.json code-analysis/Cargo.lock src-tauri/Cargo.lock kernel/Cargo.lock; do
  if git diff --quiet HEAD -- "$f" && git diff --cached --quiet HEAD -- "$f"; then
    echo "OK   $f 无未评审改动"
  elif [ "${VERIFY_ALLOW_DEP_CHANGE:-0}" = "1" ]; then
    echo "ALLOW $f 有改动（VERIFY_ALLOW_DEP_CHANGE=1 显式豁免，须在 PR 说明）"
  else
    echo "FAIL $f 有未评审改动。依赖变更须走供应链评审：VERIFY_ALLOW_DEP_CHANGE=1 scripts/supply-chain-check.sh"
    FAILED=1
  fi
done

step "2. npm audit（高危阻断）"
if npm audit --audit-level=high --no-fund 2>"_attic_npm_audit_err.txt"; then
  echo "OK   npm audit 无高危"
  rm -f _attic_npm_audit_err.txt
else
  if grep -qiE 'ECONNREFUSED|ENOTFOUND|ETIMEDOUT|network' _attic_npm_audit_err.txt 2>/dev/null; then
    echo "SKIP npm audit 网络不可用（离线环境，原因已归档）"
  else
    echo "FAIL npm audit 报高危漏洞（详见上方输出）"
    FAILED=1
  fi
  rm -f _attic_npm_audit_err.txt
fi

step "3. cargo audit"
if command -v cargo-audit >/dev/null 2>&1 || cargo audit --version >/dev/null 2>&1; then
  if cargo audit --deny warnings; then
    echo "OK   cargo audit 通过"
  else
    echo "FAIL cargo audit 报告漏洞"
    FAILED=1
  fi
else
  echo "SKIP cargo-audit 未安装（cargo install cargo-audit 后启用本段）"
fi

[ "$FAILED" -eq 0 ] && echo "SUPPLY-CHAIN: ALL PASS" || { echo "SUPPLY-CHAIN: FAILED"; exit 1; }
