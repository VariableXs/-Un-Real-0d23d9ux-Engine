# -*- coding: utf-8 -*-
"""验收记录终版补丁：xp2 定案 + 容错措施说明。"""
from pathlib import Path

p = Path(r"D:\2\14\-Un-Real-0d23d9ux-Engine-main\docs\acceptance\kernel-S41-xhci-hid验收记录-2026-09-21.md")
s = p.read_text(encoding="utf-8")

old = """**最终定案：本机开发版 QEMU（v11.1.0-12130-ge470268ff4）TCG 层 Doorbell-DMA 可见性异常**（门铃到达后控制器未发起命令环 DMA 读）。候选验证：换 QEMU 稳定发布版复测 / Windows Hypervisor Platform（需系统开关）/ 真机 SOP 直接验证（真实 xHCI 无此层）。取证工具：`_attic/xp2-forensic.py`（monitor xp 双侧对照）已入库。"""
new = """**最终定案（monitor `xp` 物理内存取证 + 全槽扫描容错后仍复现）**：命令 TRB 双双在环（slot0/slot1 ctrl=0x2401，cycle=1）且总线地址正确（CRCR=0x3f467001 等已达控制器），但控制器 Doorbell 后的命令环 DMA 读返回旧值（fetch 读到 cycle=0），完成事件从未写入——**TCG 层 guest CPU 写到 QEMU DMA 读的可见性延迟异常**（非确定性，非驱动问题：驱动三种访问路径 × 两种后备 × 冷/热 boot 全排除；端口事件 DMA 双向可见证明 ERST 投递与 HHDM 路径正确）。
**驱动侧已加两层工程容错（sim 16 用例仍全绿）**：①事件环全槽扫描 `evt_scan_for`（类型+TRB 指针双匹配，防写序/游标错位）；②命令等待过半重振铃一次（防门铃丢失）。两者对真机无害（真驱动对多段事件环本就全段扫描）。
**环境备注**：本机 QEMU（weilnetz 20260811 快照）与开发版同为 v11.1.0-12130-ge470268ff4，无可对照版本；Windows Hypervisor Platform 未启用（需系统开关）。**权威验收环境 = 真机 SOP 会话**（真实 xHCI 硅片无 TCG 语义）。取证工具：`_attic/xp2-forensic.py`（monitor xp 双侧对照）、`_attic/patch-scan-rerun.py` 已入库。"""
assert s.count(old) == 1, "count=%d" % s.count(old)
s = s.replace(old, new)
p.write_text(s, encoding="utf-8")
print("WRITTEN")
