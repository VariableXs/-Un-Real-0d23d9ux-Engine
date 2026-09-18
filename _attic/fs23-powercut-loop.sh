#!/bin/bash
# 任务17 · QEMU 实机掉电注入循环 ×11 轮
# 每轮：启动 QEMU（NVMe 盘持久文件）→ 内核 fs23 探针 open/append 3 条
# → 串口出现 verify → kill QEMU（模拟掉电）→ 下一轮。
# 预期：entries 0→3→6→...→30 单调增长，torn 恒 0，最终 total=33。
cd "$(dirname "$0")/../kernel" || exit 1
QEMU="/c/Program Files/qemu/qemu-system-x86_64"
for r in 1 2 3 4 5 6 7 8 9 10 11; do
  rm -f "qemu-serial-fs23-r${r}.log"
  "$QEMU" -m 512M -M q35 -cdrom ../varix-qemu.iso -boot d \
    -drive file=nvme-test.img,if=none,id=nvme0,format=raw \
    -device nvme,drive=nvme0,serial=VARIX16 \
    -serial "file:qemu-serial-fs23-r${r}.log" \
    -no-reboot -no-shutdown &
  QPID=$!
  seen=0
  for i in $(seq 1 150); do
    if grep -q "fs23-disk verify" "qemu-serial-fs23-r${r}.log" 2>/dev/null; then
      seen=1
      break
    fi
    sleep 1
  done
  if [ "$seen" = "1" ]; then
    echo "=== ROUND ${r} ==="
    grep "fs23-disk" "qemu-serial-fs23-r${r}.log"
  else
    echo "=== ROUND ${r}: TIMEOUT (no verify in 150s) ==="
    tail -3 "qemu-serial-fs23-r${r}.log" 2>/dev/null
  fi
  powershell -NoProfile -Command "Stop-Process -Name qemu-system-x86_64 -Force -ErrorAction SilentlyContinue" >/dev/null 2>&1
  sleep 3
done
echo "ALL ROUNDS DONE"
