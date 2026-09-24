#!/usr/bin/env bash
# B-2903 panic 百次对练（WP-106 · MD3 4.1 施工要点：
# "panic 路径的百次演练必须在 m1 出口前完成——panic 打不开的花，
#  m2 桌面期间死给你看"）。
#
# 用法: bash scripts/panic-drill.sh [目标轮数，默认 100]
#
# 形态：一次 QEMU 进程内自动循环——
#   panic_test=1 注入 → panic 四环节（保护屏/现场带/十秒倒计时）
#   → FADT RESET_REG(0xCF9) 复位（QEMU 不带 -no-reboot，复位即重启）
#   → 下一轮 boot 期 guard-band 重放打印上次现场 → 再次注入 → ……
# 日志里 "guard-band: last panic" 的行数 = 完整闭环次数
# （panic → 复位 → 重启 → 现场带可读）。
#
# ISO 管线：make-iso-qemu.py（pycdlib，本机 xorriso.exe 损坏——"Exec
# format error"，2026-09-24 实测登记）。产物落 build/panic-drill.iso：
# varix.iso（真机正统 xorriso 产物）全程不被触碰，真机事实源纪律天然
# 成立，无需事后重建。pycdlib 产物无 isohybrid，仅 QEMU 引导用，不可
# dd 到 U 盘。
#
# 依赖：qemu-system-x86_64、python3 + pycdlib。
set -uo pipefail
cd "$(dirname "$0")/.."

ROUNDS="${1:-100}"
LOG="build/panic-drill.log"
CONF="_attic/limine-panic-drill.conf"
ISO="build/panic-drill.iso"
PID=""

cleanup() {
    [ -n "$PID" ] && kill "$PID" 2>/dev/null
    echo ""
    local loops
    loops=$(grep -c "guard-band: last panic" "$LOG" 2>/dev/null || echo 0)
    echo "=== B-2903 对练收账：完整闭环 ${loops} 轮（目标 ${ROUNDS}）==="
    [ "$loops" -ge "$ROUNDS" ] && echo "DRILL-OK" || echo "DRILL-PARTIAL"
    exit 0
}
trap cleanup INT TERM

command -v qemu-system-x86_64 >/dev/null 2>&1 || { echo "ERROR: 缺少 qemu-system-x86_64" >&2; exit 127; }
PY="${PYTHON:-python}"
command -v "$PY" >/dev/null 2>&1 || PY=python3
command -v "$PY" >/dev/null 2>&1 || { echo "ERROR: 缺少 python" >&2; exit 127; }
"$PY" -c "import pycdlib" 2>/dev/null || { echo "ERROR: python 缺 pycdlib（pip install pycdlib）" >&2; exit 127; }
[ -f "$CONF" ] || { echo "ERROR: 缺少 $CONF" >&2; exit 1; }

# 1) 对练 ISO（pycdlib 管线；对练 conf 经 --conf 注入 panic_test=1）。
if ! "$PY" scripts/make-iso-qemu.py --conf "$CONF" --out "$ISO"; then
    echo "ERROR: ISO 构建失败" >&2
    exit 1
fi

# 2) 起 QEMU（单进程循环；-serial file 全程留痕；不带 -no-reboot）。
rm -f "$LOG"
qemu-system-x86_64 \
    -cdrom "$ISO" \
    -serial file:"$LOG" \
    -m 512M -M q35 \
    -display none &
PID=$!
echo "panic-drill: QEMU pid=$PID 目标 ${ROUNDS} 轮，监控 $LOG"

# 3) 监控闭环计数，达到目标即收账（上限 180 分钟——实测单轮 ≈ 60-100s：
#    boot ~50s（input-probe 慢段）+ 十秒倒计时 TSC 真实自旋 + 复位 POST）。
DEADLINE=$(( $(date +%s) + 10800 ))
while [ "$(date +%s)" -lt "$DEADLINE" ]; do
    if ! kill -0 "$PID" 2>/dev/null; then
        echo "panic-drill: QEMU exited unexpectedly"; break
    fi
    LOOPS=$(grep -c "guard-band: last panic" "$LOG" 2>/dev/null || echo 0)
    if [ "$LOOPS" -ge "$ROUNDS" ]; then
        echo "panic-drill: reached ${LOOPS} loops — stopping"
        break
    fi
    sleep 5
done
cleanup
