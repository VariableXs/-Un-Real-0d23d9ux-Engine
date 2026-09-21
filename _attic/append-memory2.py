# -*- coding: utf-8 -*-
"""追加 xp 定案记忆 + 更新长期记忆 MEMORY.md 的 AI-5 段。"""
from pathlib import Path

daily = Path(r"D:\2\14\-Un-Real-0d23d9ux-Engine-main\.workbuddy\memory\2026-09-21.md")
entry = """

## AI-5 · S4.1 缺口二 xp 取证定案（22 日凌晨，commits 341672b/1d4dbed/cb143ed 已推 main）
- **monitor `xp` 物理内存取证定案**：命令 TRB 双双在环（slot0/slot1 ctrl=0x2401）且总线地址正确，事件环内只有两个端口事件、完成事件从未写入——控制器从不取指命令环。**最终定案：开发版 QEMU（v11.1.0-12130）TCG 层 Doorbell-DMA 可见性异常**，非驱动问题（驱动三种访问路径×两种后备×冷热 boot 全排除；端口事件双向可见证明 ERST 投递与 HHDM 路径正确）。
- **配置链断裂教训**：walkthrough 的 build_iso() 与 make-iso --conf 互相覆盖后，limine.conf 丢 kernel_cmdline → guest 空 cmdline + 5s 菜单 + handoff=true 重置循环。**验证法：`grep -ac "handoff=0" ISO文件`**。修复=直接 printf 写干净 conf 再 make-iso。
- **Windows QEMU 取证工具链**：monitor 用 `-monitor tcp:127.0.0.1:PORT`（Windows Python 无 AF_UNIX）；`xp /4wx 物理地址` 直读 QEMU RAM；`_attic/xp2-forensic.py` = 一体化（启动→等超时→xp 双侧对照→quit）。**QEMU 进程绝不能放在会随命令结束被清理的后台子壳里**（父 bash 退出会连带杀掉 detached QEMU）。
- **S4.1 收官状态**：驱动 v3 终态（.bss 后备+HHDM 访问+translate 总线边界）+ 16 模拟器用例 + 全量门禁绿 + QEMU 层异常完整取证登记；剩余=真机 SOP 会话（权威验收）+ 换稳定版 QEMU 复测（可选）。S4.2-S4.5 评估报告已交付等拍板。
"""
with daily.open("a", encoding="utf-8") as f:
    f.write(entry)

mem = Path(r"D:\2\14\-Un-Real-0d23d9ux-Engine-main\.workbuddy\memory\MEMORY.md")
s = mem.read_text(encoding="utf-8")
anchor = "## 三体 S3 批部署引擎线（09-21 AI-2 代码级交付）"
add = """## 三体 S4.1 xHCI HID（09-21/22 AI-5 交付，8939f47+341672b+1d4dbed+cb143ed）
- **驱动终态**：drivers/xhci.rs v3 = .bss 驻留 DMA 池（免疫 PMM/initfs 重叠）+ HHDM 访问路径（先掩码取帧基再查表，v1 反查 miss #PF 教训）+ bus_addr 边界翻译（DmaMem 默认恒等，11 类站点：CRCR/DCBAA 寄存器+内容/ERSTBA/ERST 内容/ERDP/EP ctx dequeue/TRB 参数/Link 目标/事件匹配）。模拟器 16 用例+ktest 3113/0+kcheck 0。
- **QEMU 层未解异常**：开发版 QEMU（v11.1.0-12130）TCG 下 Doorbell 后控制器不取指命令环（xp 物理取证定案：TRB 在环 cycle=1、完成事件缺失），端口事件双向可见。候选：稳定版 QEMU/WHPX/真机 SOP。取证工具 _attic/xp2-forensic.py。
- **HID→PS/2 同构**：Y 轴翻转（HID 正=下/PS2 正=上，QEMU hid.c vs ps2.c 实证）+ make-only 差分；合成 3B 包过 MouseDecoder 回环测试锁定。
- **QEMU 取证戒律**：trace/serial file: 后端=追加+缓冲，强杀丢尾部——用 monitor quit 优雅退出或 socket tee；trace 文件用全新文件名并核对头尾同期；bash taskkill //F 静默失败→僵尸 QEMU 污染证据，清点用 tasklist|grep、击杀用 PS 工具；ISO conf 验证=`grep -ac handoff=0 ISO`。
"""
assert anchor in s, "anchor missing"
s = s.replace(anchor, add + "\n" + anchor)
mem.write_text(s, encoding="utf-8")
print("MEMORY DONE")
