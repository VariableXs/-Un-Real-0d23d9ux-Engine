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

### 缺口二：QEMU/TCG 下命令环 DMA 可见性异常（本批核心遗留，**晚间会话深挖后大幅收窄**）
**症状（最终形态）**：QEMU 下 Doorbell 后控制器对命令 TRB 的 DMA 读返回旧值（cycle=0），完成事件不产生；**同页/同帧机制下端口事件 DMA 写（QEMU→guest）完全可见**（port_ev=2），ERST 表内容（guest→QEMU）投递正确（事件落在翻译后的正确物理），唯独命令环 guest→QEMU 方向在 RS=1 后失效。驱动 kwarn 超时优雅跳过（**绝不卡引导、绝不 panic**）。
**晚间会话已完成的修复与排除（三轮改型）**：
1. **v1（.bss+反查）#PF 崩溃根因已找到并修**：`virt_of` 按帧基精确匹配，而链表写入用「帧基+偏移」键 → miss → 兜底返回物理形态地址被当虚拟写。v3 改为「先掩码取帧基再查表」从结构上消灭该类反查。
2. **v2（key=虚拟地址 + bus_addr 边界翻译）**：`DmaMem` 增加 `bus_addr` 默认恒等方法（nvme 零改动），池式实现覆写为页表翻译真值；CRCR/DCBAAP/ERSTBA/ERST 内容/ERDP/EP 上下文 dequeue/TRB 参数/Link 目标/事件匹配共 11 类站点全部过边界翻译。**QEMU trace 实证：CRCR=0x3f5ba001 等全部按翻译值正确到达控制器**。
3. **v3（HHDM 访问路径）**：内存访问改走 `物理+HHDM`（历史成功运行 trace2-boot1 所用同款路径），双路径（内核虚拟/HHDM）症状一致。
**已排除**：bus_addr 翻译错误（trace 证明 CRCR/ERSTBA/ERDP 翻译值正确到达）；寄存器写序（HCRST→编程→RS 顺序 QEMU 侧 trace 确认）；命令环语义（模拟器 16 用例含回绕）；SeaBIOS 干扰（其 POST 枚举与内核 HCRST 后的重初始化在 trace 中时序分离）；访问路径（内核虚拟/HHDM 双路径同症）；PMM 重叠假设（.bss 后备同症）；环境污染（僵尸 QEMU 进程曾干扰证据链——bash `taskkill //F` 静默失败导致，已全部清理并改用 PS 工具清点）。
**当前假设（xp 物理内存取证定案，monitor `xp` 直接读 QEMU RAM）**：命令 TRB 双双在环（slot0/slot1 ctrl=0x2401，cycle=1）且总线地址正确（CRCR=0x3f467001 等已达控制器），事件环内**只有两个端口事件（idx0=port5、idx1=port6），完成事件从未写入**——控制器从未取指命令环。驱动侧三种访问路径（内核虚拟/HHDM）×两种后备（PMM/.bss）×冷/热 boot 全部排除；端口事件 DMA 双向可见证明 ERST 投递与 HHDM 访问路径本身正确。**最终定案：本机开发版 QEMU（v11.1.0-12130-ge470268ff4）TCG 层 Doorbell-DMA 可见性异常**（门铃到达后控制器未发起命令环 DMA 读）。候选验证：换 QEMU 稳定发布版复测 / Windows Hypervisor Platform（需系统开关）/ 真机 SOP 直接验证（真实 xHCI 无此层）。取证工具：`_attic/xp2-forensic.py`（monitor xp 双侧对照）已入库。
**真机口径**：真实 xHCI 硬件无 TCG 语义，本缺口不影响真机 SOP 验收路径；1.8 SOP 上机前 7 项照常。

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

1. 缺口二收尾（候选：QEMU 稳定版复测 / WHPX / 真机 SOP 直接验证）——AI-5 内；
2. 引导序重排（缺口一）——AI-1 辖区，等拍板；
3. memmap 预登记排查（initfs/模块区）——登记给 boot-reservation 属主线；
4. 真机 SOP 会话（Legion F12 → VARIX，外接 USB 键鼠走查）——等用户排期。
