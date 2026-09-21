# -*- coding: utf-8 -*-
"""更新 S4.1 验收记录：晚间深挖结果（缺口二收窄）。"""
from pathlib import Path

p = Path(r"D:\2\14\-Un-Real-0d23d9ux-Engine-main\docs\acceptance\kernel-S41-xhci-hid验收记录-2026-09-21.md")
s = p.read_text(encoding="utf-8")

old = """### 缺口二：目标态 DMA 帧物理寻址（本批核心遗留）
**症状**：QEMU 下 pmm 帧运气决定成败——帧落 `0x3ffd****` 族（高区）时全链枚举 + HID 报告流动（usb_xhci trace 逐条在案）；落 `0x3eb0****` 族时命令 TRB 对控制器不可见（fetch 读到 cycle=0），驱动 kwarn 超时优雅跳过（**绝不卡引导、绝不 panic**）。
**已排除**：寄存器写入（trace 证明全命中）、命令环语义（模拟器 16 用例）、串口/trace 环境污染（僵尸 QEMU 进程曾干扰证据链，已全部清理并以干净单轮 run 复核）。
**假设**（下一轮验证起点）：PMM 帧与 Limine 装载的 initfs/内核页重叠（memmap 预登记缺口），内核自身活动双向踩踏 DMA 帧。
**尝试与回退**：.bss 驻留池 + 页表翻译方案在 `virt_of` 缺失路径引入 #PF 崩溃（不可接受的回归），**已回退 pmm 版**（优雅失败）。修复方向已定：.bss 驻留 + 寄存器编程边界处页表翻译（translate 返回真物理，vmap 仅存虚拟，杜绝 phys→virt 反查）。
**取证工具**：cmd 超时路径的 kwarn 全量 dump（evt_idx/evt_cycle/ERDP/cmd_phys/TRB 回读）已常驻，下次跑查即得现场。"""
new = """### 缺口二：QEMU/TCG 下命令环 DMA 可见性异常（本批核心遗留，**晚间会话深挖后大幅收窄**）
**症状（最终形态）**：QEMU 下 Doorbell 后控制器对命令 TRB 的 DMA 读返回旧值（cycle=0），完成事件不产生；**同页/同帧机制下端口事件 DMA 写（QEMU→guest）完全可见**（port_ev=2），ERST 表内容（guest→QEMU）投递正确（事件落在翻译后的正确物理），唯独命令环 guest→QEMU 方向在 RS=1 后失效。驱动 kwarn 超时优雅跳过（**绝不卡引导、绝不 panic**）。
**晚间会话已完成的修复与排除（三轮改型）**：
1. **v1（.bss+反查）#PF 崩溃根因已找到并修**：`virt_of` 按帧基精确匹配，而链表写入用「帧基+偏移」键 → miss → 兜底返回物理形态地址被当虚拟写。v3 改为「先掩码取帧基再查表」从结构上消灭该类反查。
2. **v2（key=虚拟地址 + bus_addr 边界翻译）**：`DmaMem` 增加 `bus_addr` 默认恒等方法（nvme 零改动），池式实现覆写为页表翻译真值；CRCR/DCBAAP/ERSTBA/ERST 内容/ERDP/EP 上下文 dequeue/TRB 参数/Link 目标/事件匹配共 11 类站点全部过边界翻译。**QEMU trace 实证：CRCR=0x3f5ba001 等全部按翻译值正确到达控制器**。
3. **v3（HHDM 访问路径）**：内存访问改走 `物理+HHDM`（历史成功运行 trace2-boot1 所用同款路径），双路径（内核虚拟/HHDM）症状一致。
**已排除**：bus_addr 翻译错误（trace 证明 CRCR/ERSTBA/ERDP 翻译值正确到达）；寄存器写序（HCRST→编程→RS 顺序 QEMU 侧 trace 确认）；命令环语义（模拟器 16 用例含回绕）；SeaBIOS 干扰（其 POST 枚举与内核 HCRST 后的重初始化在 trace 中时序分离）；访问路径（内核虚拟/HHDM 双路径同症）；PMM 重叠假设（.bss 后备同症）；环境污染（僵尸 QEMU 进程曾干扰证据链——bash `taskkill //F` 静默失败导致，已全部清理并改用 PS 工具清点）。
**当前假设**（下一轮验证起点）：本机 QEMU 为开发版构建（v11.1.0-12130-ge470268ff4，非发布版），TCG 对「运行中控制器 Doorbell 处理器内发起的 DMA 读」与 guest 此前 CPU 写的可见性存在异常；候选验证：换 QEMU 稳定发布版 / `-d unimp,guest_errors` / Windows Hypervisor Platform（本机未启用，需系统开关）。
**真机口径**：真实 xHCI 硬件无 TCG 语义，本缺口不影响真机 SOP 验收路径；1.8 SOP 上机前 7 项照常。"""
assert s.count(old) == 1, "gap2 section count=%d" % s.count(old)
s = s.replace(old, new)

old2 = """## 4. 下一步（拍板清单）

1. 缺口二修复（.bss + 边界翻译）——AI-5 内，建议下一会话首个动作；"""
new2 = """## 4. 下一步（拍板清单）

1. 缺口二收尾（候选：QEMU 稳定版复测 / WHPX / 真机 SOP 直接验证）——AI-5 内；"""
assert s.count(old2) == 1, "next-step count=%d" % s.count(old2)
s = s.replace(old2, new2)

p.write_text(s, encoding="utf-8")
print("WRITTEN")
