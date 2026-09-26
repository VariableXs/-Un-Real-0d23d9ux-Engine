# S4.1 · xHCI HID（真机 USB 键鼠）评估与设计 —— AI-5 内核基建长线

> **步骤卡**：S4.1 xHCI HID（M1 寄存器 → M2 环 → M3 枚举 → M4 HID 解析；纯轮询 IMOD=0）。
> **状态**：评估完成，开工。本文先于代码落盘（纪律：每件立项前先出评估再动工）。

---

## 1. 现状盘点（代码级证据）

| 事实 | 证据 |
|---|---|
| 内核已有真驱动范式：接口冻结层 + 宿主模拟器全链可测 + 目标态薄实现 | `drivers/blk.rs`（BlockDevice trait 冻结）、`drivers/nvme.rs`（BarAccess+DmaMem 双 trait、寄存器模拟器、`init_with_recovery` 恢复路径、QEMU 探针链） |
| PCI ECAM 扫描已是宿主可测真实现，但目前只识别 NVMe（class 0108） | `drivers/pci.rs`（McfgSegment/EcamAccess/scan_nvme_all） |
| 输入单汇点已存在：`InputService::pump()` 是唯一硬件泵，菜单（`menu_poll_key/menu_poll_mouse`）与 shim 桌面（`drain_to_shim`）都从这走 | `inputsvc.rs`（L491-533 pump、L578-598 menu 源、L643 drain_to_shim） |
| `InputEvent::Key(ps2::Key)` 是键事件唯一词汇；鼠标 = dx/dy/buttons 三元组（bit0 左/bit1 右/bit2 中） | `inputsvc.rs` L28-35 |
| **PS/2 鼠标 Y 语义：正 = 向上**（QEMU ps2.c `s->mouse_dy -= evt->rel.value`，向下移动产生负 dy） | `_attic/qemu-ps2-ref.c` L711/814 |
| **USB HID 鼠标 Y 语义：正 = 向下**（QEMU hid.c `e->ydy += evt->rel.value`，直接进报告） | `_attic/qemu-hidcore-ref.c` L206/399 |
| MMIO 映射走 PML4[510] 窗口惰性 map_mmio（PCD\|PWT），DMA 走 PMM 帧 + HHDM 访问 | `drivers/nvme.rs` target（BarMmio/DmaBuckets）、`mem/pfh.rs` |
| 引导菜单在 `mem::init` **之前**运行（main.rs 菜单 ~L134，mem::init ~L275） | `main.rs` |

## 2. 缺口结论

- 真机外接 USB 键鼠目前**不可达**：内核只有 i8042 PS/2 通道，无任何 USB 主机控制器栈。需求「任意鼠标和键盘」只有半边。
- `drivers/usb.rs` 是 AI-31 登记体（确定性模型，启动链零调用），不承担真驱动职责。

## 3. 方案（最小路径 1 控制器 × 1 设备 × 1 中断端点）

```
PCI ECAM 扫描(class 0x0C03 prog-if 0x30) → BAR 映射(PML4[510] 惰性 16 页)
  → HCRST 复位 → CRCR/DCBAA/CONFIG/ERSTSZ/ERSTBA/ERDP → RS=1
  → 端口扫描(CCS→PR→PRC/PED+speed) → Enable Slot → Address Device(BSR=0)
  → GET_DESCRIPTOR(8B→18B) → 接口类 3/1/1=键 3/1/2=鼠
  → SET_PROTOCOL(boot) → SET_IDLE(0) → SET_CONFIGURATION(1) → Configure Endpoint(EP1 IN)
  → EP1 常备一个 Normal TRB(IOC) → 轮询事件环 → HID boot 报告解码
  → 合成 PS/2 同构字节 → feed_key_byte / feed_mouse_byte（既有公开汇点，零改动接入）
```

设计要点（与 NVMe 同一纪律）：

1. **控制流泛型** `XhciCtrl<B: BarAccess, M: DmaMem>`——复用 nvme 的两个 trait；宿主用寄存器+设备模拟器全链测试，目标态只换 BarMmio/DMA 池两层薄实现。
2. **纯轮询**：IMAN.IE=0、IMOD=0，不注册任何中断；命令/传输完成靠事件环轮询。
3. **事件环单段 64 条**，ERST 表与事件环同帧（ERST@+0、环@+64）；EP/命令环 64 条 + Link TRB(TC=1) 支持 wrap 与 cycle 翻转。
4. **门铃写序**同 NVMe：TRB 落 DMA 内存 → Release fence → 写门铃；事件读全 → Acquire fence → 推进 ERDP。
5. **增量不替代**：PS/2 通道零改动；HID 事件经「HID→PS/2 同构字节」从 `feed_key_byte/feed_mouse_byte` 汇入，全部既有消费者（菜单/ushell/桌面）自动受益。鼠标 **Y 轴翻转**（HID 正=下 → PS/2 正=上）是本设计的显式契约，有测试锁定。
6. **QEMU 实证对照**：TRB/上下文/端口位布局逐字段对照 QEMU `hcd-xhci.c/h`（参考件归档 `_attic/qemu-hcd-xhci-ref.{c,h}` 等），模拟器与 QEMU 行为同构。

## 4. 红线与安全闸（AI-5 禁区逐条对照）

- **内置盘 NVMe 永不初始化**：本驱动只枚举 xHCI 控制器（class 0x0C03），对 NVMe/AHCI 域零操作；S4.2 才涉及存储，写路径三道闸届时另立。
- **PS/2 增量不替代**：`ps2.rs` 零改动；无 xHCI 时 pump 钩子零开销 no-op，既有 QEMU 配置（无 `-device nec-usb-xhci`）行为逐字节不变。
- **优雅跳过**：无 xHCI 控制器 / BAR 映射失败 / 初始化超时，一律 kwarn + 跳过，绝不 panic、绝不卡引导。

## 5. 已识别的跨辖区缺口（登记，不自行绕过）

**引导菜单 USB 键盘依赖引导序重排**：bootselect 菜单在 `mem::init` 之前运行，而 xHCI 初始化需要 PMM DMA 帧 + 页表映射（`pfh::map_mmio`）——当前引导序下菜单阶段内存域未上线，USB 键盘物理上无法在菜单前就绪。本批交付：驱动全栈 + 桌面/shim 路径全通（`drain_to_shim` 路径 USB 键鼠可用）；菜单路径 = **登记待办**：把菜单挪到内存域之后（或菜单期引入最小 DMA 通道）属引导链改造（AI-1 辖区），等拍板后另步施工。

## 6. 明确不做（如实声明）

- 集线器级联、USB3 速率协商、流协议（MaxPStreams）、scratchpad（HCSPARAMS2.MaxSpBuf=0 直读校验）、64 字节上下文（CSZ 位直读支持但 QEMU 为 32B）。
- HID report descriptor 解析——只用 boot 协议（SET_PROTOCOL(0)），不做通用 HID 描述符扫描。
- 游戏手柄/消费控制类 HID 设备；键盘修饰键只映射 Shift（`ps2::Key` 词汇表无 Ctrl/Alt，如实丢弃并计数）。
- 滚轮（QEMU usb-mouse 报告第 4 字节 dz）：现有 MouseDelta 无滚轮词汇，本批只取前 3 字节。

## 7. 验收口径（本批 DoD）

1. 宿主模拟器全链用例全绿（复位→枚举→控制传输→HID 解码→环 wrap→超时恢复）。
2. `cargo ktest --lib` 全绿（基线 3068 + 新增用例）；`cargo kcheck` 0 错。
3. **QEMU 实证**：`-device nec-usb-xhci -device usb-kbd -device usb-mouse` 下探针证据行齐全（控制器/枚举/协议/配置/键鼠事件），HMP sendkey/mouse_move 产生对应 HID 事件证据行，PS/2 既有路径无回归。
4. 验收记录归档 `docs/acceptance/`；QEMU 走查脚本归档 `_attic/`。
