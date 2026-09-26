# S4.2 · xHCI MSC（USB 存储直读写）评估报告 —— AI-5 内核基建长线

> **步骤卡**：S4.2 xHCI MSC（BOT+SCSI，读为主写限 SHARED，新立项评估后动工）。
> **状态**：评估完成，**动工待拍板**（本报告即立项材料）。
> **前置**：S4.1 xHCI HID 已落地（控制器初始化/命令环/事件环/枚举框架/纯轮询模型全部在位）——本件在 S4.1 之上做协议层增量，控制器层零重做。

---

## 1. 现状盘点

| 事实 | 证据 |
|---|---|
| xHCI 控制器栈已在位：复位/命令环/事件环/端口枚举/中断端点轮询，宿主模拟器 16 用例全绿 + QEMU 实证 | `drivers/xhci.rs`（S4.1 交付） |
| 块设备接口已冻结：NVMe/AHCI/未来设备同接口，上层（fs23_journal/kvsrv/vfsguard/exfat_ro）只认识 BlockDevice | `drivers/blk.rs`（trait 冻结契约） |
| exFAT 只读挂载已有完整链路（NVMe 第二控制器 → exfat_ro → usrshell 全局挂载） | `drivers/nvme.rs` target `probe_and_selftest`、`fs/exfat_ro` |
| 写路径三道闸的既有组件：vfsguard 白名单、SnapshotArea 快照、fs23_journal 断电注入探针（×10/×11 零丢失实测） | `vfsguard.rs`、`milestone.rs`、`fs/fs23_disk.rs` |
| S0.2 配置桥当前依赖 ESP 副本（boot-select.json 引导期读不到的阻断缺口，候选方案 A） | 《实施总步骤图》S0.2 |

## 2. 方案评估

### 2.1 协议层（BOT = Bulk-Only Transport + SCSI 命令集）

- **端点形态**：每 MSC 设备一对 Bulk 端点（EPx OUT / EPx IN），xHCI 侧是 S4.1 已验证的 Normal TRB 环模型 ×2（OUT 环新增，IN 环复用现模型）。控制枚举阶段多一个 GET_DESCRIPTOR(0x06, String 可跳) + 类判定（接口类 0x08，协议 0x50=BOT）+ Get Max LUN 类请求（可选，超时报错容忍）。
- **SCSI 最小命令集**（如实冻结，其余 Unsupported 口径）：
  - `TEST UNIT READY (0x00)` / `REQUEST SENSE (0x03)`——状态轮询与错误明细；
  - `INQUIRY (0x12)`——厂商/型号证据行；
  - `READ CAPACITY(10) (0x25)`——容量与块长；
  - `READ(10) (0x28)` / `WRITE(10) (0x2A)`——LBA 读写（U 盘 ≤2TB 内 32 位 LBA 足够，READ(16) 不做）。
- **BOT 封包**：CBW（31 字节，签名 USBC）→ 数据阶段（可选）→ CSW（13 字节，签名 USBS）。dCBWTag 软件自增配对；CSW 状态 0/1/2 三态 + Residue 处理；CSW 意义失配（签名/Tag 错）→ 端点 Clear Feature Halt 恢复流程（Reset Recovery）。
- **BlockDevice 实现**：`block_size/capacity_blocks/read_blocks/write_blocks/flush`（flush = SYNCHRONIZE CACHE(10) 0x35，U 盘多不支持 → 如实报告 Unsupported 并回退 TEST UNIT READY 轮询确认，绝不假装 flush 成功）。

### 2.2 与三体架构的衔接

- **U 盘 MSC 控制器 = 现有 NVMe 控制器路径的并列后端**：`probe_and_selftest` 枚举到 MSC LUN 后构造 `XhciMscCtrl`（impl BlockDevice）→ 交给 `fs::exfat_ro` 挂 SHARED → `proc::usrshell::mount::install` 同一入口。上层零改动（接口冻结的红利）。
- **QEMU 侧**：`-device usb-storage,bus=xhci.0,drive=shd`（QEMU 原生 BOT+SCSI 模拟）→ 宿主模拟器（SCSI 应答模型）+ QEMU 双层验证，与 S4.1 同一方法论。
- **ESP 副本退役（S0.2 候选 A 的解药）**：MSC 直读落地后，内核引导期即可从 SHARED 读 boot-select.json 真身——ESP 副本+module_path 方案光荣退役，bootselect/handoff 的配置源回归单一事实源。**这是 S4.2 对 AI-1 线的最大外溢价值**。

### 2.3 写路径三道闸（红线级，缺一不开工）

| 闸 | 机制 | 复用 |
|---|---|---|
| ①vfsguard 白名单 | 写请求先过白名单裁决，越权显式拒绝并记审计账本 | `vfsguard.rs` 现成 |
| ②SnapshotArea 快照 | 写前对目标区间快照，可回滚 | 既有快照组件 |
| ③断电注入 ×10 | 外部脚本 kill QEMU 模拟掉电，journal+快照零丢失复测 | fs23_powercut_probe 范式 |

## 3. 红线与禁区（AI-5 禁区逐条对照）

- **内置盘 NVMe 永不初始化**：MSC 枚举白名单只收 xHCI 域下的存储设备；xHCI 控制器本身经 ECAM 扫描（MCFG），NVMe/AHCI 分类结果不被本件触碰。附加保险：把「控制器 BDF 白名单」做成探测期登记（QEMU = 00:xx.x 实测值落 boot 日志），真机上如出现非预期第二控制器 → kwarn + 拒绝挂载。
- **SHARED 用户数据只增不删**（红线三）：删除类操作先快照；写路径绝不绕过 vfsguard。
- **不碰 AI-1 辖区**：ESP 副本退役的切换动作由 AI-1 在其会话执行，本件只交付「直读能力 + 退役可行性结论」。
- **exFAT 写实现不在本件范围**：exfat_ro 是只读实现；「内核直写 SHARED」= MSC 块层写能力 + exFAT 写驱动 = 两件工程。本件交付前者并跑通回环探针（块级）；exFAT 写驱动按既有 exfat_ro 的结构增量立项（评估随本报告附：exFAT 写 = 目录项定位/簇链分配/journal 联动，工程量 ≈ exfat_ro 的 1.5 倍，建议作为 S4.2b 单独拍板）。

## 4. 工作量与验收口径（预估 4-5 天，与 S4.1 同粒度）

- M1 BOT 封包器 + SCSI 命令构造器（纯函数，宿主可测，1 天）；
- M2 OUT 端点环 + CSW 配对状态机（宿主模拟器全链，1.5 天）；
- M3 MSC 枚举 + READ CAPACITY/READ(10) 通（QEMU usb-storage 实证，1 天）；
- M4 WRITE(10) + 三道闸接线 + 断电注入 ×10（1.5 天）。
- 验收：①块级回环 ×1000（loopback_probe 现成范式）②QEMU usb-storage 全链证据行 ③断电 ×10 零损坏 ④EXTRA：向 AI-1 交付「ESP 副本退役」可行性确认单。

## 5. 结论

**可行，前置全部就绪，风险集中在 CSW 异常恢复与 exFAT 写驱动两个点**——前者有 Reset Recovery 成熟流程兜底，后者建议拆 S4.2b 单独拍板。建议拍板顺序：先批 S4.2 本体（块级直读写），exFAT 写驱动随下一批决策。
