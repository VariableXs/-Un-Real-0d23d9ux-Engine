#!/usr/bin/env bash
# audit-status.sh · AURORA-1000 文档漂移门禁（步骤 0260）
# 校验 1) gen-numbers.py 的 W1=200；2) docs/CHECKSET.md 无遗留「待填」的 W1 域行。
set -euo pipefail
cd "$(dirname "$0")/.."

echo "[1/2] feature numbers..."
python scripts/gen-numbers.py

echo "[2/2] CHECKSET drift..."
for dom in display render2d typography gpu compositor image ainput aaudio window motion desktop appfw w1_integration; do
  line=$(grep -E "^\| $dom \|" docs/CHECKSET.md || true)
  if [ -z "$line" ]; then
    echo "DRIFT: CHECKSET.md missing row for $dom"; exit 1
  fi
  if echo "$line" | grep -q "待填"; then
    echo "DRIFT: CHECKSET.md row '$dom' not filled"; exit 1
  fi
done

echo "audit-status: zero drift, all PASS"
