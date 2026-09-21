# S4.1 · xHCI HID 验收记录 —— AI-5 内核基建长线（2026-09-21）

> **交付物**：`kernel/varix/src/drivers/xhci.rs`（xHCI 最小栈 + HID boot 协议 + 宿主模拟器 16 用例）；
> `drivers/pci.rs`（xHCI 识别 + scan_xhci_all）；`drivers/mod.rs`（挂载）；
> `inputsvc.rs` pump 钩子（+8 行）；`main.rs` 探针（+3 行）；
> 评估设计/评估报告 ×5；QEMU 走查脚本 + QEMU 源参考件（_attic）。

---

## 1. 已证明（证据在案）

| 项 | 证据 |
|---|---|
| 驱动逻辑全链（宿主模拟器与 QEMU hcd-xhci.c 行为同构） | `cargo ktest --lib xhci` **16/16 PASS**：复位→枚举→控制传输→HID 键鼠解码→EP1 环回绕零丢失→超时恢复（重试 1 次→DeviceReset）→无设备优雅跳过→槽位回收 |
| 四线门禁不回归 | `cargo ktest --lib` **3113 passed / 0 failed**（基线 3068 + xhci 16 + 并行会话新增）；`cargo kcheck` 0 错；`cargo kbuild` release 通过 |
| PCI 层 | `PciKind::Xhci`（0x0C03/0x30）+ `scan_xhci_all`，QEMU 实测 BDF 00:03.0、BAR0=0xfebd4000 命中 |
| QEMU 实机证据（usb_xhci trace 逐条） | HCRST/CRCR(0x18)/DCBAAP/ERSTSZ/ERSTBA/ERDP 写入全部命中；`CR_ENABLE_SLOT`→`ER_COMMAND_COMPLETE SUCCESS`；`slot_address slotid 1 port 1 / slotid 2 port 2`；SETUP/DATA/STATUS 控制传输链 8 次 SUCCESS；**8 次 TR_NORMAL 中断 IN（8B 键盘报/4B 鼠标报）= HID 报告真实流动** |
| 关键 bug 修复（QEMU trace 实证驱动） | ①CRCR 在操作段 **0x18**（原按记忆写 0x10，QEMU `case 0x18` 铁证纠正）；②RTSOFF/DBOFF 是 32 位寄存器（`&0xFF` 会把 0x1000 掐成 0）；③cmd_submit 超时清在途标记（防级联失败） |

## 2. 已登记缺口（不静默，附证据与修复方向）

### 缺口一：引导菜单阶段 USB 键盘不可达（跨辖区）
bootselect 菜单在 `mem::init` 之前运行，xHCI 初始化需要 DMA 帧 + 页表——引导序重排归 AI-1 辖区，等拍板。

### 缺口二：目标态 DMA 帧物理寻址（本批核心遗留）
**症状**：QEMU 下 pmm 帧运气决定成败——帧落 `0x3ffd****` 族（高区）时全链枚举 + HID 报告流动（usb_xhci trace 逐条在案）；落 `0x3eb0****` 族时命令 TRB 对控制器不可见（fetch 读到 cycle=0），驱动 kwarn 超时优雅跳过（**绝不卡引导、绝不 panic**）。
**已排除**：寄存器写入（trace 证明全命中）、命令环语义（模拟器 16 用例）、串口/trace 环境污染（僵尸 QEMU 进程曾干扰证据链，已全部清理并以干净单轮 run 复核）。
**假设**（下一轮验证起点）：PMM 帧与 Limine 装载的 initfs/内核页重叠（memmap 预登记缺口），内核自身活动双向踩踏 DMA 帧。
**尝试与回退**：.bss 驻留池 + 页表翻译方案在 `virt_of` 缺失路径引入 #PF 崩溃（不可接受的回归），**已回退 pmm 版**（优雅失败）。修复方向已定：.bss 驻留 + 寄存器编程边界处页表翻译（translate 返回真物理，vmap 仅存虚拟，杜绝 phys→virt 反查）。
**取证工具**：cmd 超时路径的 kwarn 全量 dump（evt_idx/evt_cycle/ERDP/cmd_phys/TRB 回读）已常驻，下次跑查即得现场。

### 缺口三：热复位后重初始化
handoff 复位（QEMU 内第二遍 boot）后重初始化失败（trace：第一遍全链成功）。单轮 boot 路径（handoff=0）不受影响；真机语义=交接去 Windows，回内核走冷启动，影响面有限，随缺口二一并解决。

## 3. 自检六问对照

1. **需求对照**：S4.1 步骤卡=M1 寄存器✓ M2 环✓ M3 枚举✓（模拟器级 + QEMU trace 级）M4 HID✓（解码器+合成通道全测）；纯轮询 IMOD=0✓；真机验收=待 1.8 SOP 会话（硬件会话，非本批代码交付范畴）。
2. **回读验证**：所有关键改动 grep/回读复核（本会话再次实证 Edit 工具批量静默丢补丁的风险，全部改为 Python 脚本落盘 + 回读断言）。
3. **门禁**：ktest 3113/0、kcheck 0 错、kbuild 通过（见 §1）。
4. **边界/负向**：无设备、超时恢复、槽位回收、环回绕、SHORT_PACKET、ErrorRollOver、词汇表外 usage——模拟器用例全覆盖。
5. **体验五子项**：用户可见部分=输入通道（菜单/桌面），USB 通道与 PS/2 同构汇入；方向契约（Y 翻转）有测试与日志证据行。
6. **归档**：本记录 + 评估报告 ×5 + 走查脚本与参考件（_attic）✓。

## 4. 下一步（拍板清单）

1. 缺口二修复（.bss + 边界翻译）——AI-5 内，建议下一会话首个动作；
2. 引导序重排（缺口一）——AI-1 辖区，等拍板；
3. memmap 预登记排查（initfs/模块区）——登记给 boot-reservation 属主线；
4. 真机 SOP 会话（Legion F12 → VARIX，外接 USB 键鼠走查）——等用户排期。
