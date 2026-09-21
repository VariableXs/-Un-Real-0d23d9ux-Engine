# -*- coding: utf-8 -*-
"""验收记录补：xp 取证定案段。"""
from pathlib import Path

p = Path(r"D:\2\14\-Un-Real-0d23d9ux-Engine-main\docs\acceptance\kernel-S41-xhci-hid验收记录-2026-09-21.md")
s = p.read_text(encoding="utf-8")

old = """**当前假设**（下一轮验证起点）：本机 QEMU 为开发版构建（v11.1.0-12130-ge470268ff4，非发布版），TCG 对「运行中控制器 Doorbell 处理器内发起的 DMA 读」与 guest 此前 CPU 写的可见性存在异常；候选验证：换 QEMU 稳定发布版 / `-d unimp,guest_errors` / Windows Hypervisor Platform（本机未启用，需系统开关）。"""
new = """**当前假设（xp 物理内存取证定案，monitor `xp` 直接读 QEMU RAM）**：命令 TRB 双双在环（slot0/slot1 ctrl=0x2401，cycle=1）且总线地址正确（CRCR=0x3f467001 等已达控制器），事件环内**只有两个端口事件（idx0=port5、idx1=port6），完成事件从未写入**——控制器从未取指命令环。驱动侧三种访问路径（内核虚拟/HHDM）×两种后备（PMM/.bss）×冷/热 boot 全部排除；端口事件 DMA 双向可见证明 ERST 投递与 HHDM 访问路径本身正确。**最终定案：本机开发版 QEMU（v11.1.0-12130-ge470268ff4）TCG 层 Doorbell-DMA 可见性异常**（门铃到达后控制器未发起命令环 DMA 读）。候选验证：换 QEMU 稳定发布版复测 / Windows Hypervisor Platform（需系统开关）/ 真机 SOP 直接验证（真实 xHCI 无此层）。取证工具：`_attic/xp2-forensic.py`（monitor xp 双侧对照）已入库。"""
assert s.count(old) == 1, "count=%d" % s.count(old)
s = s.replace(old, new)
p.write_text(s, encoding="utf-8")
print("WRITTEN")
