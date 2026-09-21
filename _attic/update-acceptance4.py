# -*- coding: utf-8 -*-
"""验收记录：补 drop-condition 首要假设与惰性 ERDP 修复候选。"""
from pathlib import Path

p = Path(r"D:\2\14\-Un-Real-0d23d9ux-Engine-main\docs\acceptance\kernel-S41-xhci-hid验收记录-2026-09-21.md")
s = p.read_text(encoding="utf-8")

old = """**当前假设（xp 物理内存取证定案，monitor `xp` 直接读 QEMU RAM）**：命令 TRB 双双在环（slot0/slot1 ctrl=0x2401，cycle=1）且总线地址正确（CRCR=0x3f467001 等已达控制器），事件环内**只有两个端口事件（idx0=port5、idx1=port6），完成事件从未写入**——控制器从未取指命令环。"""
new = """**当前假设（xp 物理内存取证定案，monitor `xp` 直接读 QEMU RAM）**：命令 TRB 双双在环（slot0/slot1 ctrl=0x2401，cycle=1）且总线地址正确（CRCR=0x3f467001 等已达控制器），事件环内**只有两个端口事件（idx0=port5、idx1=port6），完成事件从未写入**。**首要候选机制 = QEMU `xhci_event()` 的满环丢弃分支**：`(er_ep_idx+1)%size == dp_idx` 时静默丢弃事件（源码 _attic/qemu-hcd-xhci-ref.c ~L672 "ER full, drop event"）——本症事件序列中驱动 ERDP 推进与 QEMU er_ep_idx 的相对位置恰可命中该分支（端口事件占位 + 完成事件被丢 + 端口事件覆写同槽，三现象同源）。"""
assert s.count(old) == 1, "count=%d" % s.count(old)
s = s.replace(old, new)

old2 = """**环境备注**：本机 QEMU（weilnetz 20260811 快照）与开发版同为 v11.1.0-12130-ge470268ff4，无可对照版本；Windows Hypervisor Platform 未启用（需系统开关）。**权威验收环境 = 真机 SOP 会话**（真实 xHCI 硅片无 TCG 语义）。取证工具：`_attic/xp2-forensic.py`（monitor xp 双侧对照）、`_attic/patch-scan-rerun.py` 已入库。"""
new2 = """**环境备注**：本机 QEMU（weilnetz 20260811 快照）与开发版同为 v11.1.0-12130-ge470268ff4，无可对照版本；Windows Hypervisor Platform 未启用（需系统开关）。**权威验收环境 = 真机 SOP 会话**（真实 xHCI 硅片无 TCG 语义）。取证工具：`_attic/xp2-forensic.py`（monitor xp 双侧对照）、`_attic/patch-scan-rerun.py` 已入库。
**修复候选（下一会话首项，已备案）**：ERDP 惰性推进——消费事件后不立即前推 ERDP（保持「落后一个事件」），仅当事件环过半时批量推进——使 QEMU 的满环丢弃分支永不命中（dp_idx 始终落后 er_ep_idx ≥2），并为 SeaBIOS 式老固件兼容留出缓冲。改动局限 evt_step/advance 一处，宿主模拟器可加「惰性推进下完成事件不丢」回归用例。"""
assert s.count(old2) == 1, "count2=%d" % s.count(old2)
s = s.replace(old2, new2)
p.write_text(s, encoding="utf-8")
print("WRITTEN")
