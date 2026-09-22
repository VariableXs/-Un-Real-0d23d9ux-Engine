# AI-5 · S4.2 MSC + exFAT 受限直写 + last_boot 写回 + AHCI 验收记录

> **定位**：M5 基建批核心件的代码级 + 模拟器级 + QEMU 级验收记录（2026-09-22）。
> 真机（物理整机引导）验收项保持独立记账，见 §6 与《AI分工完成图》更新。

---

## 1. S4.2-A · xHCI MSC 传输层（BOT+SCSI）

- **交付**：`kernel/varix/src/drivers/msc.rs`（纯协议层，`BulkPipe` 泛型）+
  `drivers/xhci.rs` bulk 端点扩展（EP1 OUT/IN 上下文、bulk 传输环、
  传输事件等待含全槽扫描容错与半程重振铃、MSC 设备表 `MAX_MSC=2`）。
- **SCSI 词汇表**：TEST_UNIT_READY / INQUIRY / READ CAPACITY(10) / READ(10) /
  WRITE(10) / REQUEST_SENSE（仅 FAILED 取证）。**提前到达的 CSW 识别**：
  设备失败时可跳过数据段直接回 CSW（真实硬件=数据端点 stall），协议层
  首块识别 tag 配对的 13B CSW 并按其状态收尾。
- **块语义**：几何来自 READ CAPACITY（不假设 512）；`BlockDevice` 实现；
  越界/非整块显式拒绝。
- **模拟器**：BOT 设备行为学模型（CBW→数据→CSW 逐字节、tag 错配/相位
  错误/介质错误/短包注入）+ xHCI 模拟器扩展（bulk OUT 端点 + MSC 虚拟盘
  挂 DMA/TRB 层）。**12 项测试全绿**。
- **xHCI 模拟器新增**：MSC 枚举状态 / 读写回环 / bulk 环回绕（80 次写
  多轮回绕零丢失）/ 与 HID 键鼠共存。xhci 全套 20/20。

## 2. S4.2-B · exFAT 受限直写层（`fs/exfat_rw.rs`）

- **两条原语，其余写请求不存在**：
  1. `rewrite_same_size`——既有文件同尺寸就地改写：只写文件自身数据簇
     起始扇区（内容按 512B 扇区零填充），绝不碰 FAT/位图/目录项/尺寸。
  2. `create_file_root`——根目录新建小文件（≤15 字符 ASCII 名、数据
     ≤4KiB）：落盘顺序 数据→FAT→位图→目录项（提交点）。
- **扇区粒度硬纪律**：真实 SHARED（560GiB exFAT）簇可达 128KiB，内核堆
  单块 ≤4KiB——**从不物化整簇**：目录扫描按扇区流式（条目集跨扇区由
  ≤576B 携带缓冲拼接），数据/位图/FAT/目录项全部 1–2 扇区读-改-写。
- **断电不变式（ktest）**：create 写序列 4 扇区逐一掉电注入 → 卷可挂载、
  文件「不可见或完整」二态；rewrite 掉电 → 目录项/位图/FAT 完好。
  **8 项测试全绿**。
- **构建器**：`_attic/mkexfat.py` 增加 `/boot-select.json` 种子
  （last_boot 初值=windows，模拟 Windows 侧已写现场）。

## 3. last_boot 写回闭环（方案2 根解，AI-1 依赖改挂 S4.2）

- **写回点**：main.rs 引导流在 NVMe/xHCI/AHCI 探针之后——走到那里的唯一
  路径 = 三卡选中 varix（或倒计时默认）→ `record_last_boot("variable")`。
- **通道选择**：MSC（真机 U 盘直写）优先 → QEMU 第二 NVMe（usrshell 全局
  挂载同一块设备）兜底；均失败 = kwarn 记账，**绝不阻塞引导**。
- **文本级拼接**：`bootcfg::splice_last_boot` 只改 `"last_boot":"…"` 值，
  其余字节原样；windows↔variable 同 8 字符 = 等长改写天然满足
  `rewrite_same_size` 契约；键缺失（Windows 侧从未写过）= 跳过不造键。
- **Windows 侧**：Variable/dualboot 启动写 `last_boot=windows`（既有链路，
  SHARED 真相源单一事实源不变）。
- **QEMU ×5 交替演练**：`_attic/t7-t8-qemu-campaign.py` P2——Windows 侧
  raw patch（windows）→ 冷启动内核写回（variable）→ 镜像 raw 断言，×5。

## 4. AHCI 最小栈（`drivers/ahci.rs`）

- **范围**：单控制器 × 单端口（首个 DET=3）× 命令槽 0 串行 × LBA48；
  IDENTIFY / READ DMA EXT / WRITE DMA EXT / FLUSH CACHE EXT；纯轮询；
  init 超时 → 整段重初始化一次 → DeviceReset（与 NVMe/xHCI 同口径）。
- **安全设计**：目标态探针**默认只读**（IDENTIFY + 读 LBA0）；写回环仅当
  cmdline `ahci_selftest=1`（QEMU 刮擦盘；真机 SATA 盘可能承载用户数据，
  绝不自动写）。`pci.rs` classify 新增 0x0106/0x01 → `PciKind::Ahci` +
  `scan_ahci`（本机内置 NVMe 0x0108 零交集）。
- **模拟器**：HBA 行为模型（PxCI 写入同步处理、CFIS 解码、PRDT DMA、
  RFIS status、HR 自清语义）**6 项测试全绿**。

## 5. QEMU 战役（`_attic/t7-t8-qemu-campaign.py`，2026-09-22 终判）

| 阶段 | 内容 | 结果 |
|---|---|---|
| P1 | 壁纸桌面 + last_boot 内核写回取证 + screendump | **PASS**（desktop-ready / 内核写回 marker / 镜像 raw 断言 variable / rc=0 四项全过；壁纸视觉证据 `_attic/acceptance-t7t8/p1-desktop-wallpaper.png`） |
| P2 | last_boot ×5 交替（重建镜像 windows → 冷启动内核写回 variable → raw 断言） | **PASS ×5**（每轮 desktop=True / write=True / img=variable / rc=0） |
| P3 | 冷启动 ×10（全新 QEMU 进程，desktop-ready + rc=0） | **PASS ×10**（每轮 101-102s，零失败） |
| P4 | handoff 防自锁拒绝路径 ×2（OVMF pflash） | **FAIL（环境级）**——edk2 固件 + Limine UEFI 链无串口输出（黑屏），防自锁闸门本体有宿主测试 + S0.1 QEMU 实测背书；OVMF+Limine 链路调试列为独立缺口（非本批回归） |

**战役总判**：P1/P2/P3 全绿；证据链 `_attic/t7-campaign-result.log` + `_attic/t7-serial.log`。
壁纸桌面视觉：ushell 桌面与 Windows 侧 WE 壁纸同源（FILES/SETTINGS/ABOUT 图标 +
START 任务栏 + UP 时钟照常），与用户 2026-09-20 目标截图对照成立。

**战役脚本戒律（新增）**：①共享镜像 VBR 带 0xAA55 → SeaBIOS 视为可引导硬盘抢先
→ `-boot order=d`；②内核 QEMU 流程需要双 NVMe（#1 测试盘 + #2 SHARED，单盘=
无 SHARED 全局挂载=写回静默 false）；③孤儿 QEMU 占端口必须按进程名强杀
（TaskStop 只杀 python）；④`python -u` 再 tee（否则缓冲吞进度）。

附录（战役终判）见文末「战役结果」小节——由战役脚本输出回填。

## 6. 真机验收项（未完，如实登记）

- 真机 F12 → VARIX → ushell 真壁纸桌面（物理整机，需用户在场；SOP 同
  S0.3/S1.4 步骤卡）。**U 盘 ESP 部署已备**：`esp-deploy-switch.ps1` 新增
  壁纸模块部署步骤（哈希校验+备份），isoroot 已含 wallpaper.rgb565。
- 真机 U 盘 MSC 读写回环（内核写 → Windows 读）：代码就绪，随真机引导
  会话执行；Windows 侧校验 + `chkdsk W:` 收尾。
- xHCI MSC 真机验收：开发版 QEMU TCG 写可见性异常（S4.1 定案）同样适用
  于 MSC 数据段——模拟器级全绿 + 真机 SOP 权威。

## 7. 戒律沉淀（本批新坑）

1. **Limine internal module 子目录路径挂死**：`internal_modules` 请求
   `../boot/<file>`（子目录）在 ISO9660 上使引导黑屏挂死（串口零输出）；
   **根级路径**（`../<file>`）正常。资产模块一律放卷根。
2. **1.8MiB include_bytes 进 .rodata** 使内核 RW 段整体后移，引导期 .bss
   写落只读页 fatal #PF（`_attic/t7-crash-serial.log`）——大资产走模块
   通道（.bss 装载实测安全），不走 include_bytes。
3. MSC 数据段「设备失败提前回 CSW」必须在协议层首块识别，否则 stall
   语义被误判为相位破坏。

——AI-5 交付 / AI-6 执法签发（2026-09-22）
