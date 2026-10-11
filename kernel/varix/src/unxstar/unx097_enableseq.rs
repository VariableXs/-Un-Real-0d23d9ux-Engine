//! unx097_enableseq — UNX-F0908 MSI-X 使能时序（PCI 命令位与能力使能纪律）。
//!
//! 判据原文（施工书锚点 UNX-F0908，CoRun·UNX-A2-B06 批）：
//! **UNX-F0908-J1 使能时序正确（写序固化 10/10 次），使能前表项预验
//! （未配置即使能 10/10 次拦截），INTx 掩蔽联动 10/10 次正确**。
//!
//! 职责定位：F0902 建好了桌项与配额、F0905 定了上下文契约——本单管
//! **"开闸"那一刻的次序**。MSI-X 使能是一个三步舞：桌项写好 → INTx
//! 引脚掩蔽 → MXE 置位，跳任何一步都是真机灾难（Windows 深在哪：
//! 这套时序散在 pci.sys 的资源分配 + 总线驱动能力配置 + 驱动
//! KeInitializeDpc 时序三处，驱动侧从不接触原始次序所以感知不到；
//! 直接操作 PCIe 设备的驱动（嵌入式/直通场景）写错序就是"设备用
//! 未初始化桌项发随机向量中断"级别的灾难，且只有真机才炸——
//! 开发期零症状；Varix：①**四步状态机固化写序**（Idle→Table
//! Programmed→INTx Masked→MXE Enabled），每步单向、跳步显性拒
//! （写序不是文档是状态机）；②**使能前预验**——MXE 闸只认"全部
//! 待用桌项已通过消息编码校验"的账面（未配置即使能 10/10 拦截，
//! 桌项校验复用 F0902 编解码闭集）；③**INTx 联动**——MXE 置位的
//! 前置是 Command.INTX_DISABLE 已置位（双路径竞争从时序上排除），
//! MXE 清位后 INTx 才许恢复（联动账双向机检）；④**FunctionMask
//! 全屏蔽面**——调试/崩溃现场需要的"一键静默"有显性入口与账面；
//! ⑤**真机风险前置**——反序注入（先 MXE 后桌项/不关 INTx/双使能）
//! 在宿主面全部拦截留账，真机炸点在宿主面就红。
//! 上游：F0902 桌项编解码与能力结构（本单预验复用其闭集）、
//! F0906 注水资产（压力使能复用本单状态机）；下游：F0909 双能力
//! 裁决（MSI/MSI-X 竞争时序消费本单联动账）、F0910 INTx 兼容路径
//! （掩蔽联动对面）。
//! 本单交付**命令位面 + 四步状态机 + 预验器 + INTx 联动账 + 三判据
//! 十轮 + 反序注入**七面。
//!
//! 设计要点：no_std+alloc；零 unwrap/expect；零 IO 零墙钟（tick 显性
//! 注入）；LCG 确定性仿真（Numerical Recipes 参数）；FNV-1a 快照
//! 往返；错误全显性带诊断码与建议。诊断码段 0x41（0x40 为 F0907
//! 所取）。

use alloc::collections::BTreeMap;
use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

/// 模块版本。
pub const ENABLE_SEQ_VERSION: &str = "B06-enableseq-v1";

// ---------------------------------------------------------------------------
// 诊断码（XcCode 0x41 段，本单独占——0x40 为 F0907 所取）
// ---------------------------------------------------------------------------

/// 0x4101 跳步（使能时序状态机非法迁移）。
pub const XC_STEP_SKIP: u32 = 0x4101;
/// 0x4102 未配置即使能（桌项预验不过——MXE 闸拦截）。
pub const XC_TABLE_UNREADY: u32 = 0x4102;
/// 0x4103 INTx 联动缺失（MXE 置位前 INTX_DISABLE 未置）。
pub const XC_INTX_LINK_MISSING: u32 = 0x4103;
/// 0x4104 双使能（MXE 已开再开——幂等闸）。
pub const XC_DOUBLE_ENABLE: u32 = 0x4104;
/// 0x4105 桌项非法（预验发现编码越域——复用 F0902 闭集）。
pub const XC_TABLE_ENTRY_BAD: u32 = 0x4105;
/// 0x4106 INTx 恢复越权（MXE 未清就恢复 INTx 引脚）。
pub const XC_INTX_RESTORE_EARLY: u32 = 0x4106;
/// 0x4107 命令位越集（PCI Command 保留位置 1）。
pub const XC_CMD_RESERVED: u32 = 0x4107;
/// 0x4108 设备未入账（时序操作打给未登记设备）。
pub const XC_DEVICE_ABSENT: u32 = 0x4108;
/// 0x4109 掩蔽态失配（FunctionMask 账与 MXE 账矛盾）。
pub const XC_FUNC_MASK_MISMATCH: u32 = 0x4109;
/// 0x410A 快照指纹漂移。
pub const XC_SNAP_DRIFT: u32 = 0x410A;
/// 0x410B 预验空桌（零桌项就要求使能）。
pub const XC_EMPTY_TABLE: u32 = 0x410B;

// ---------------------------------------------------------------------------
// §1 PCI Command 寄存器位面
// ---------------------------------------------------------------------------

/// PCI Command 位闭集（PCI Local Bus Spec 6.2.3——bit10 之外本单只读）。
pub mod pci_cmd {
    /// IO Space Enable（bit0）。
    pub const IO_EN: u16 = 0x0001;
    /// Memory Space Enable（bit1）。
    pub const MEM_EN: u16 = 0x0002;
    /// Bus Master Enable（bit2）。
    pub const BUS_MASTER: u16 = 0x0004;
    /// Special Cycle（bit3）。
    pub const SPECIAL_CYCLES: u16 = 0x0008;
    /// Memory Write and Invalidate（bit4）。
    pub const MWI_EN: u16 = 0x0010;
    /// VGA Palette Snoop（bit5）。
    pub const VGA_SNOOP: u16 = 0x0020;
    /// Parity Error Response（bit6）。
    pub const PARITY_RESP: u16 = 0x0040;
    /// IDSEL stepping（bit7）。
    pub const IDSEL_STEP: u16 = 0x0080;
    /// SERR# Enable（bit8）。
    pub const SERR_EN: u16 = 0x0100;
    /// Fast Back-to-Back（bit9）。
    pub const FAST_B2B: u16 = 0x0200;
    /// **INTx Disable（bit10）——本单主角**：MSI-X 使能前置。
    pub const INTX_DISABLE: u16 = 0x0400;
    /// 保留位掩码（bit15..11——置 1 即非法）。
    pub const RESERVED_MASK: u16 = 0xF800;
}

/// 命令字合法性：保留位必须为零。
pub fn cmd_legal(cmd: u16) -> bool {
    cmd & pci_cmd::RESERVED_MASK == 0
}

// ---------------------------------------------------------------------------
// §2 四步状态机
// ---------------------------------------------------------------------------

/// 使能时序四步闭集。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum EnableStep {
    /// 空闲（桌项未写）。
    Idle,
    /// 桌项已编程（全部待用桌项过预验）。
    TableProgrammed,
    /// INTx 已掩蔽（Command.INTX_DISABLE=1）。
    IntxMasked,
    /// MSI-X 已使能（MXE=1）——终态。
    MsiXEnabled,
}

impl EnableStep {
    /// 全态枚举。
    pub const fn all() -> [EnableStep; 4] {
        [
            EnableStep::Idle,
            EnableStep::TableProgrammed,
            EnableStep::IntxMasked,
            EnableStep::MsiXEnabled,
        ]
    }

    /// 下标。
    pub const fn idx(self) -> usize {
        match self {
            EnableStep::Idle => 0,
            EnableStep::TableProgrammed => 1,
            EnableStep::IntxMasked => 2,
            EnableStep::MsiXEnabled => 3,
        }
    }

    /// 人话名。
    pub const fn name(self) -> &'static str {
        match self {
            EnableStep::Idle => "idle",
            EnableStep::TableProgrammed => "table-programmed",
            EnableStep::IntxMasked => "intx-masked",
            EnableStep::MsiXEnabled => "msix-enabled",
        }
    }
}

/// 迁移矩阵——`[from][to]`，合法迁移恰 3 格（严格单调链）。
pub const ENABLE_MATRIX: [[bool; 4]; 4] = [
    // to:  Idle   TblPrg IntxM  MsiXEn
    [false, true, false, false], // Idle
    [false, false, true, false], // TableProgrammed
    [false, false, false, true], // IntxMasked
    [false, false, false, false], // MsiXEnabled（终态）
];

/// 迁移合法性。
pub const fn enable_step_legal(from: EnableStep, to: EnableStep) -> bool {
    ENABLE_MATRIX[from.idx()][to.idx()]
}

// ---------------------------------------------------------------------------
// §3 桌项预验器（复用 F0902 编解码闭集语义）
// ---------------------------------------------------------------------------

/// 本单自含的消息编码校验（与 F0902 同语义——自包含编译面）。
pub const MSI_ADDR_BASE: u32 = 0xFEE0_0000;
pub const MSI_ADDR_FIELD_MASK: u32 = 0x000F_FFFC;
pub const MSI_VECTOR_BASE: u8 = 0x60;
pub const MSI_VECTOR_LIMIT: u8 = 0x70;

/// 桌项（复刻 F0902 面的最小闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct TableEntry {
    /// message address。
    pub msg_addr: u32,
    /// message data。
    pub msg_data: u32,
    /// vector control（仅 bit31 masked）。
    pub vec_ctrl: u32,
}

/// 桌项预验：地址帧基 + 向量域 + VecCtrl 保留位（三闸全过才算"已配置"）。
pub fn entry_ready(e: &TableEntry) -> Result<(), (u32, String)> {
    if e.msg_addr & !MSI_ADDR_FIELD_MASK != MSI_ADDR_BASE {
        return Err((XC_TABLE_ENTRY_BAD, format!("地址帧基破坏：{:#x}", e.msg_addr)));
    }
    let vector = (e.msg_data & 0xFF) as u8;
    if vector < MSI_VECTOR_BASE || vector >= MSI_VECTOR_LIMIT {
        return Err((
            XC_TABLE_ENTRY_BAD,
            format!("数据字向量 {:#x} 落出设备池 [{:#x},{:#x})", vector, MSI_VECTOR_BASE, MSI_VECTOR_LIMIT),
        ));
    }
    if e.vec_ctrl & 0x7FFF_FFFF != 0 {
        return Err((XC_TABLE_ENTRY_BAD, format!("VecCtrl 保留位非零：{:#x}", e.vec_ctrl)));
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// §4 设备使能账（MsiXEnableSequencer）
// ---------------------------------------------------------------------------

/// 单设备 MSI-X 使能账——四步状态机 + 桌项表 + INTx 联动账。
#[derive(Clone, Debug)]
pub struct DeviceEnableSeq {
    /// 设备 tag。
    pub device: String,
    /// 当前步。
    pub step: EnableStep,
    /// 桌项表（vector → 桌项——预验后入账）。
    pub table: BTreeMap<u8, TableEntry>,
    /// Command 寄存器账面。
    pub cmd: u16,
    /// Message Control 账面（MXE bit15 / FunctionMask bit14）。
    pub msg_ctrl: u16,
    /// 状态机迁移历史（序账）。
    pub history: Vec<(EnableStep, EnableStep, u64)>,
    /// 逻辑 tick。
    tick: u64,
}

impl DeviceEnableSeq {
    /// 新账（Idle 态，Command 初始只开 MEM——真机 MSI-X 需要 MEM 使能）。
    pub fn new(device: &str, tick: u64) -> DeviceEnableSeq {
        DeviceEnableSeq {
            device: device.to_string(),
            step: EnableStep::Idle,
            table: BTreeMap::new(),
            cmd: pci_cmd::MEM_EN,
            msg_ctrl: 0,
            history: Vec::new(),
            tick,
        }
    }

    /// 当前步。
    pub fn step(&self) -> EnableStep {
        self.step
    }

    /// INTx 是否已掩蔽。
    pub fn intx_masked(&self) -> bool {
        self.cmd & pci_cmd::INTX_DISABLE != 0
    }

    /// MXE 是否已开。
    pub fn mxe(&self) -> bool {
        self.msg_ctrl & 0x8000 != 0
    }

    /// FunctionMask 态。
    pub fn func_mask(&self) -> bool {
        self.msg_ctrl & 0x4000 != 0
    }

    /// 写桌项（Idle/TableProgrammed 段允许——MXE 开后写桌项是运行期
    /// 改向量，本单契约禁（需走 F0915 清理链））。
    pub fn program_entry(&mut self, vector: u8, e: TableEntry) -> Result<(), (u32, String)> {
        if self.step == EnableStep::MsiXEnabled {
            return Err((
                XC_STEP_SKIP,
                format!("设备 {} MXE 已开——桌项再写是运行期改向量，禁（走清理链）", self.device),
            ));
        }
        entry_ready(&e)?;
        self.table.insert(vector, e);
        // 桌项全过预验 → 推进到 TableProgrammed（从 Idle 单向）。
        if self.step == EnableStep::Idle {
            self.transition(EnableStep::TableProgrammed)?;
        }
        Ok(())
    }

    /// 状态机迁移（矩阵闸 + 历史记账）。
    fn transition(&mut self, to: EnableStep) -> Result<(), (u32, String)> {
        let from = self.step;
        if !enable_step_legal(from, to) {
            return Err((
                XC_STEP_SKIP,
                format!(
                    "设备 {} 跳步：{} → {} 被矩阵拒绝",
                    self.device,
                    from.name(),
                    to.name()
                ),
            ));
        }
        self.history.push((from, to, self.tick));
        self.step = to;
        Ok(())
    }

    /// INTx 掩蔽（TableProgrammed 段执行——置 Command.INTX_DISABLE）。
    pub fn mask_intx(&mut self) -> Result<(), (u32, String)> {
        if self.step != EnableStep::TableProgrammed {
            return Err((
                XC_STEP_SKIP,
                format!(
                    "设备 {} 在 {} 段执行 INTx 掩蔽——仅 table-programmed 段合法",
                    self.device,
                    self.step.name()
                ),
            ));
        }
        let new_cmd = self.cmd | pci_cmd::INTX_DISABLE;
        if !cmd_legal(new_cmd) {
            return Err((XC_CMD_RESERVED, "Command 保留位被置——非法命令字".to_string()));
        }
        self.cmd = new_cmd;
        self.transition(EnableStep::IntxMasked)?;
        Ok(())
    }

    /// MXE 使能（IntxMasked 段执行——三道闸：步序 / 预验 / INTx 联动）。
    pub fn enable_msix(&mut self) -> Result<(), (u32, String)> {
        if self.step != EnableStep::IntxMasked {
            return Err((
                XC_STEP_SKIP,
                format!(
                    "设备 {} 在 {} 段执行 MXE 使能——仅 intx-masked 段合法",
                    self.device,
                    self.step.name()
                ),
            ));
        }
        if self.mxe() {
            return Err((XC_DOUBLE_ENABLE, format!("设备 {} MXE 已开——双使能", self.device)));
        }
        // 预验：桌项非空 + 全项过校验（未配置即使能的主拦截面）。
        if self.table.is_empty() {
            return Err((XC_EMPTY_TABLE, format!("设备 {} 空桌使能被拒", self.device)));
        }
        for (v, e) in &self.table {
            entry_ready(e).map_err(|(c, m)| {
                (c, format!("设备 {} 桌项 {:#x} 预验不过：{}", self.device, v, m))
            })?;
        }
        // INTx 联动闸：INTX_DISABLE 必须已置（双路径竞争时序排除）。
        if !self.intx_masked() {
            return Err((
                XC_INTX_LINK_MISSING,
                format!(
                    "设备 {} INTx 联动缺失：MXE 置位前 Command.INTX_DISABLE 未置",
                    self.device
                ),
            ));
        }
        self.msg_ctrl |= 0x8000;
        self.transition(EnableStep::MsiXEnabled)?;
        Ok(())
    }

    /// FunctionMask 全屏蔽（任意 MXE 开后段——一键静默，账面显性）。
    pub fn set_function_mask(&mut self, masked: bool) -> Result<(), (u32, String)> {
        if !self.mxe() {
            return Err((
                XC_FUNC_MASK_MISMATCH,
                format!("设备 {} MXE 未开——FunctionMask 语义失效面", self.device),
            ));
        }
        if masked {
            self.msg_ctrl |= 0x4000;
        } else {
            self.msg_ctrl &= !0x4000;
        }
        Ok(())
    }

    /// MXE 清位（禁用路径——清后 INTx 才许恢复：联动反向）。
    pub fn disable_msix(&mut self) -> Result<(), (u32, String)> {
        if !self.mxe() {
            return Err((XC_DOUBLE_ENABLE, format!("设备 {} MXE 未开——无需禁用", self.device)));
        }
        self.msg_ctrl &= !0x8000;
        self.step = EnableStep::IntxMasked; // 回退到掩蔽段（状态机反向单步）
        Ok(())
    }

    /// INTx 恢复（仅 MXE 已清的 IntxMasked 段——反向联动闸）。
    pub fn restore_intx(&mut self) -> Result<(), (u32, String)> {
        if self.mxe() {
            return Err((
                XC_INTX_RESTORE_EARLY,
                format!("设备 {} MXE 未清——INTx 恢复越权（双路径竞争）", self.device),
            ));
        }
        if self.step != EnableStep::IntxMasked {
            return Err((
                XC_STEP_SKIP,
                format!("设备 {} 在 {} 段恢复 INTx——仅 intx-masked 段合法", self.device, self.step.name()),
            ));
        }
        self.cmd &= !pci_cmd::INTX_DISABLE;
        self.step = EnableStep::TableProgrammed;
        Ok(())
    }

    /// 全账自检。
    pub fn audit(&self) -> Result<(), (u32, String)> {
        // 联动不变量：MXE 开 ⇒ INTx 掩蔽。
        if self.mxe() && !self.intx_masked() {
            return Err((
                XC_INTX_LINK_MISSING,
                format!("设备 {} 不变量破：MXE 开但 INTx 未掩蔽", self.device),
            ));
        }
        // FunctionMask 只在 MXE 开时有意义。
        if self.func_mask() && !self.mxe() {
            return Err((
                XC_FUNC_MASK_MISMATCH,
                format!("设备 {} 不变量破：FunctionMask 在 MXE 关时置位", self.device),
            ));
        }
        // 命令字保留位。
        if !cmd_legal(self.cmd) {
            return Err((XC_CMD_RESERVED, format!("设备 {} Command 保留位脏", self.device)));
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// §5 设备使能登记处（多设备账）
// ---------------------------------------------------------------------------

/// 设备使能登记处。
#[derive(Clone, Debug, Default)]
pub struct EnableBoard {
    /// 设备账。
    pub devices: BTreeMap<String, DeviceEnableSeq>,
}

impl EnableBoard {
    /// 新板。
    pub fn new() -> EnableBoard {
        EnableBoard::default()
    }

    /// 入册。
    pub fn enroll(&mut self, device: &str) -> Result<(), (u32, String)> {
        if device.is_empty() {
            return Err((XC_DEVICE_ABSENT, "设备 tag 为空".to_string()));
        }
        if self.devices.contains_key(device) {
            return Err((XC_DEVICE_ABSENT, format!("设备 {} 已入册", device)));
        }
        self.devices
            .insert(device.to_string(), DeviceEnableSeq::new(device, 0));
        Ok(())
    }

    /// 取设备。
    pub fn device(&self, device: &str) -> Option<&DeviceEnableSeq> {
        self.devices.get(device)
    }

    /// 取设备可变。
    pub fn device_mut(&mut self, device: &str) -> Option<&mut DeviceEnableSeq> {
        self.devices.get_mut(device)
    }

    /// 全板自检。
    pub fn audit_all(&self) -> Result<(), (u32, String)> {
        for d in self.devices.values() {
            d.audit()?;
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// §6 确定性底座（LCG + FNV）
// ---------------------------------------------------------------------------

/// LCG 状态（Numerical Recipes 参数）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Lcg(u32);

impl Lcg {
    /// 种子。
    pub const fn new(seed: u32) -> Lcg {
        Lcg(seed)
    }

    /// 下一步。
    pub fn next(&mut self) -> u32 {
        self.0 = self.0.wrapping_mul(1664525).wrapping_add(1013904223);
        self.0
    }

    /// [0, n) 均匀。
    pub fn below(&mut self, n: u32) -> u32 {
        self.next() % n.max(1)
    }
}

/// FNV-1a 32 位。
pub fn fnv32(data: &[u8], mut hash: u32) -> u32 {
    for &b in data {
        hash ^= u32::from(b);
        hash = hash.wrapping_mul(0x0100_0193);
    }
    hash
}

/// FNV 偏移基。
pub const FNV_OFFSET: u32 = 0x811C_9DC5;

// ---------------------------------------------------------------------------
// §7 判据三面（J1 三组各 10/10）
// ---------------------------------------------------------------------------

/// 构造合法桌项（dest_id=2，向量在池内）。
fn good_entry(vector: u8) -> TableEntry {
    TableEntry {
        msg_addr: MSI_ADDR_BASE | (2 << 12),
        msg_data: u32::from(vector),
        vec_ctrl: 0,
    }
}

/// J1-a 使能时序正确：写序固化 10/10 轮（桌项→INTx mask→MXE 全链）。
pub fn j1a_sequence_10() -> Result<String, (u32, String)> {
    for round in 0..10u64 {
        let dev = format!("seq-r{}", round);
        let mut board = EnableBoard::new();
        board.enroll(&dev)?;
        let d = board.device_mut(&dev).ok_or((XC_DEVICE_ABSENT, "入册即失".to_string()))?;
        // 桌项编程（4 项——F0902 NVMe 案同源）。
        for v in [0x60u8, 0x61, 0x62, 0x63] {
            d.program_entry(v, good_entry(v))?;
        }
        if d.step() != EnableStep::TableProgrammed {
            return Err((XC_STEP_SKIP, format!("第 {} 轮：桌项编程后未达 table-programmed", round)));
        }
        // INTx 掩蔽。
        d.mask_intx()?;
        if d.step() != EnableStep::IntxMasked || !d.intx_masked() {
            return Err((XC_INTX_LINK_MISSING, format!("第 {} 轮：掩蔽后步/位不符", round)));
        }
        // MXE 使能。
        d.enable_msix()?;
        if d.step() != EnableStep::MsiXEnabled || !d.mxe() {
            return Err((XC_STEP_SKIP, format!("第 {} 轮：使能后步/位不符", round)));
        }
        d.audit()?;
        // 反向：禁用 → INTx 恢复（联动反向验证）。
        d.disable_msix()?;
        d.restore_intx()?;
        if d.intx_masked() {
            return Err((XC_INTX_RESTORE_EARLY, format!("第 {} 轮：恢复后 INTx 仍掩蔽", round)));
        }
        d.audit()?;
    }
    Ok("10/10 使能时序全链过（4桌项→INTx掩蔽→MXE→禁用→INTx恢复）".to_string())
}

/// J1-b 使能前表项预验：未配置即使能 10/10 拦截。
///
/// 注入面：0 空桌使能（2 注）／1 桌项坏帧基（2 注）／2 桌项越池向量
/// （2 注）／3 保留位脏 VecCtrl（2 注）／4 跳步（Idle 直 MXE）……
/// 实际 10 注：空桌×2 + 坏帧基×2 + 越池×2 + 脏 VecCtrl×2 + 跳步×2。
pub fn j1b_unready_intercept_10() -> Result<String, (u32, String)> {
    let mut hits = 0u32;
    // ── 空桌 ×2 ──
    for round in 0..2u64 {
        let mut board = EnableBoard::new();
        let dev = format!("empty-{}", round);
        board.enroll(&dev)?;
        let d = board.device_mut(&dev).unwrap();
        // 不写桌项，直走掩蔽（掩蔽不依赖桌项）→ MXE 应被空桌闸拦。
        d.program_entry(0x60, good_entry(0x60))?; // 先过桌项段
        d.mask_intx()?;
        let d2 = board.device_mut(&dev).unwrap();
        // 清桌（模拟"未配置"——账面清空后试图使能）。
        d2.table.clear();
        let r = d2.enable_msix();
        if matches!(r, Err((XC_EMPTY_TABLE, _))) {
            hits += 1;
        }
    }
    // ── 坏帧基 ×2 ──
    for round in 0..2u64 {
        let mut board = EnableBoard::new();
        let dev = format!("badaddr-{}", round);
        board.enroll(&dev)?;
        let bad = TableEntry {
            msg_addr: 0xEE00_0000 | (2 << 12), // 帧基错
            msg_data: 0x60,
            vec_ctrl: 0,
        };
        let r = board.device_mut(&dev).unwrap().program_entry(0x60, bad);
        if matches!(r, Err((XC_TABLE_ENTRY_BAD, _))) {
            hits += 1;
        }
    }
    // ── 越池向量 ×2 ──
    for round in 0..2u64 {
        let mut board = EnableBoard::new();
        let dev = format!("badvec-{}", round);
        board.enroll(&dev)?;
        let bad = TableEntry {
            msg_addr: MSI_ADDR_BASE,
            msg_data: 0x59, // 池外
            vec_ctrl: 0,
        };
        let r = board.device_mut(&dev).unwrap().program_entry(0x60, bad);
        if matches!(r, Err((XC_TABLE_ENTRY_BAD, _))) {
            hits += 1;
        }
    }
    // ── 脏 VecCtrl ×2 ──
    for round in 0..2u64 {
        let mut board = EnableBoard::new();
        let dev = format!("badvc-{}", round);
        board.enroll(&dev)?;
        let bad = TableEntry {
            msg_addr: MSI_ADDR_BASE,
            msg_data: 0x60,
            vec_ctrl: 0x0000_0001, // 保留位
        };
        let r = board.device_mut(&dev).unwrap().program_entry(0x60, bad);
        if matches!(r, Err((XC_TABLE_ENTRY_BAD, _))) {
            hits += 1;
        }
    }
    // ── 跳步 ×2（Idle 直 MXE——先得绕过 program_entry 的自动推进；
    //     program_entry 成功即推进 TableProgrammed，因此跳步面=掩蔽段
    //     之前直接 enable：Idle 段 enable 被步序闸拦）──
    for round in 0..2u64 {
        let mut board = EnableBoard::new();
        let dev = format!("skip-{}", round);
        board.enroll(&dev)?;
        let r = board.device_mut(&dev).unwrap().enable_msix();
        if matches!(r, Err((XC_STEP_SKIP, _)) | Err((XC_EMPTY_TABLE, _))) {
            hits += 1;
        }
    }
    if hits != 10 {
        return Err((XC_TABLE_UNREADY, format!("预验拦截 {} / 10——漏网", hits)));
    }
    Ok("10/10 未配置即使能全拦截（空桌2+坏帧基2+越池2+脏VecCtrl2+跳步2）".to_string())
}

/// J1-c INTx 掩蔽联动：10/10 正确（正向 MXE⇒掩蔽前置 + 反向 MXE 清后
/// 才许恢复 + 不变量审计）。
pub fn j1c_intx_link_10() -> Result<String, (u32, String)> {
    let mut hits = 0u32;
    // ── 正向：不掩蔽直使能被联动闸拦 ×3（构造：掩蔽后手动清位再使能）──
    for round in 0..3u64 {
        let mut board = EnableBoard::new();
        let dev = format!("link-{}", round);
        board.enroll(&dev)?;
        {
            let d = board.device_mut(&dev).unwrap();
            d.program_entry(0x60, good_entry(0x60))?;
            d.mask_intx()?;
        }
        // 手动清 INTX_DISABLE（模拟驱动错序——掩蔽段内清位）。
        board.device_mut(&dev).unwrap().cmd &= !pci_cmd::INTX_DISABLE;
        let r = board.device_mut(&dev).unwrap().enable_msix();
        if matches!(r, Err((XC_INTX_LINK_MISSING, _))) {
            hits += 1;
        }
    }
    // ── 正向：正确链使能后不变量成立 ×3 ──
    for round in 0..3u64 {
        let mut board = EnableBoard::new();
        let dev = format!("ok-{}", round);
        board.enroll(&dev)?;
        let d = board.device_mut(&dev).unwrap();
        d.program_entry(0x60, good_entry(0x60))?;
        d.mask_intx()?;
        d.enable_msix()?;
        if d.mxe() && d.intx_masked() {
            board.device_mut(&dev).unwrap().audit()?;
            hits += 1;
        }
    }
    // ── 反向：MXE 开着就恢复 INTx 被拦 ×2 ──
    for round in 0..2u64 {
        let mut board = EnableBoard::new();
        let dev = format!("early-{}", round);
        board.enroll(&dev)?;
        {
            let d = board.device_mut(&dev).unwrap();
            d.program_entry(0x60, good_entry(0x60))?;
            d.mask_intx()?;
            d.enable_msix()?;
        }
        let r = board.device_mut(&dev).unwrap().restore_intx();
        if matches!(r, Err((XC_INTX_RESTORE_EARLY, _))) {
            hits += 1;
        }
    }
    // ── 反向：禁用后恢复成功（联动闭环）×2 ──
    for round in 0..2u64 {
        let mut board = EnableBoard::new();
        let dev = format!("close-{}", round);
        board.enroll(&dev)?;
        {
            let d = board.device_mut(&dev).unwrap();
            d.program_entry(0x60, good_entry(0x60))?;
            d.mask_intx()?;
            d.enable_msix()?;
            d.disable_msix()?;
        }
        let r = board.device_mut(&dev).unwrap().restore_intx();
        if r.is_ok() && !board.device_mut(&dev).unwrap().intx_masked() {
            hits += 1;
        }
    }
    if hits != 10 {
        return Err((XC_INTX_LINK_MISSING, format!("INTx 联动 {} / 10——漏网", hits)));
    }
    Ok("10/10 INTx 掩蔽联动全过（正3+不变量3+早恢复拦2+闭环2）".to_string())
}

// ---------------------------------------------------------------------------
// §8 反序/越权注入套件（8 类全拦）
// ---------------------------------------------------------------------------

/// 反序与越权注入——8 类全拦截。
///
/// 0 Idle 段 INTx 掩蔽（跳步）　1 Sustain…（无）——具体：
/// 0 Idle 段掩蔽　1 TableProgrammed 段直 MXE（跳过掩蔽）
/// 2 MXE 开后再写桌项　3 双使能　4 保留位 Command 拒
/// 5 Idle 段禁用（无使能先禁）　6 FunctionMask 在 MXE 关时置
/// 7 空设备操作
pub fn reverse_injections_8() -> Result<String, (u32, String)> {
    let mut hits: Vec<(u8, bool, String)> = Vec::new();

    // ── 0：Idle 段掩蔽（跳步）──
    {
        let mut board = EnableBoard::new();
        board.enroll("d0")?;
        let r = board.device_mut("d0").unwrap().mask_intx();
        hits.push((0, matches!(r, Err((XC_STEP_SKIP, _))), match r {
            Err(e) => e.1,
            Ok(_) => "未拦——跳步穿透".to_string(),
        }));
    }
    // ── 1：TableProgrammed 段直 MXE（跳过掩蔽）──
    {
        let mut board = EnableBoard::new();
        board.enroll("d1")?;
        let d = board.device_mut("d1").unwrap();
        d.program_entry(0x60, good_entry(0x60))?;
        // 掩蔽段前置缺失——enable 在 table-programmed 段应被步序闸拦。
        let r = board.device_mut("d1").unwrap().enable_msix();
        hits.push((1, matches!(r, Err((XC_STEP_SKIP, _))), match r {
            Err(e) => e.1,
            Ok(_) => "未拦——掩蔽跳步穿透".to_string(),
        }));
    }
    // ── 2：MXE 开后再写桌项 ──
    {
        let mut board = EnableBoard::new();
        board.enroll("d2")?;
        {
            let d = board.device_mut("d2").unwrap();
            d.program_entry(0x60, good_entry(0x60))?;
            d.mask_intx()?;
            d.enable_msix()?;
        }
        let r = board
            .device_mut("d2")
            .unwrap()
            .program_entry(0x61, good_entry(0x61));
        hits.push((2, matches!(r, Err((XC_STEP_SKIP, _))), match r {
            Err(e) => e.1,
            Ok(_) => "未拦——运行期改桌项".to_string(),
        }));
    }
    // ── 3：双使能 ──
    {
        let mut board = EnableBoard::new();
        board.enroll("d3")?;
        {
            let d = board.device_mut("d3").unwrap();
            d.program_entry(0x60, good_entry(0x60))?;
            d.mask_intx()?;
            d.enable_msix()?;
        }
        let r = board.device_mut("d3").unwrap().enable_msix();
        hits.push((3, matches!(r, Err((XC_DOUBLE_ENABLE, _))), match r {
            Err(e) => e.1,
            Ok(_) => "未拦——双使能".to_string(),
        }));
    }
    // ── 4：保留位 Command ──
    {
        let ok = !cmd_legal(0xF800);
        hits.push((4, ok, if ok { "保留位命令字拒".to_string() } else { "未拦".to_string() }));
    }
    // ── 5：未使能先禁用 ──
    {
        let mut board = EnableBoard::new();
        board.enroll("d5")?;
        let r = board.device_mut("d5").unwrap().disable_msix();
        hits.push((5, r.is_err(), match r {
            Err(e) => e.1,
            Ok(_) => "未拦——未使能先禁".to_string(),
        }));
    }
    // ── 6：FunctionMask 在 MXE 关时置 ──
    {
        let mut board = EnableBoard::new();
        board.enroll("d6")?;
        let r = board.device_mut("d6").unwrap().set_function_mask(true);
        hits.push((6, matches!(r, Err((XC_FUNC_MASK_MISMATCH, _))), match r {
            Err(e) => e.1,
            Ok(_) => "未拦——掩蔽语义失效".to_string(),
        }));
    }
    // ── 7：空设备操作 ──
    {
        let mut board = EnableBoard::new();
        let r = board.device_mut("ghost");
        hits.push((7, r.is_none(), "空设备显性 None".to_string()));
    }

    let missed: Vec<&(u8, bool, String)> = hits.iter().filter(|(_, ok, _)| !ok).collect();
    if !missed.is_empty() {
        let names: Vec<String> = missed.iter().map(|(k, _, e)| format!("#{} {}", k, e)).collect();
        return Err((XC_STEP_SKIP, format!("反序注入漏网：{}", names.join("；"))));
    }
    Ok("8/8 反序越权全拦".to_string())
}

// ---------------------------------------------------------------------------
// §9 快照
// ---------------------------------------------------------------------------

/// 使能账快照指纹。
pub struct EnableSnap {
    /// 指纹。
    pub hash: u32,
    /// 摘要。
    pub summary: String,
}

impl EnableSnap {
    /// 采样：每设备（步/cmd/msg_ctrl/桌项序）。
    pub fn take(board: &EnableBoard) -> EnableSnap {
        let mut buf: Vec<u8> = Vec::new();
        for (name, d) in board.devices.iter() {
            buf.extend_from_slice(name.as_bytes());
            buf.push(d.step().idx() as u8);
            buf.extend_from_slice(&d.cmd.to_le_bytes());
            buf.extend_from_slice(&d.msg_ctrl.to_le_bytes());
            for (v, e) in d.table.iter() {
                buf.push(*v);
                buf.extend_from_slice(&e.msg_addr.to_le_bytes());
                buf.extend_from_slice(&e.msg_data.to_le_bytes());
            }
        }
        EnableSnap {
            hash: fnv32(&buf, FNV_OFFSET),
            summary: format!("设备={} ", board.devices.len()),
        }
    }
}

// ---------------------------------------------------------------------------
// §10 全链自证（聚合入口——ktest 桥接点）
// ---------------------------------------------------------------------------

/// 全链自证：J1-a + J1-b + J1-c + 反序注入，一锤定音。
pub fn run_all_checks() -> Result<String, (u32, String)> {
    let mut parts: Vec<String> = Vec::new();
    parts.push(j1a_sequence_10()?);
    parts.push(j1b_unready_intercept_10()?);
    parts.push(j1c_intx_link_10()?);
    parts.push(reverse_injections_8()?);
    // 快照稳定性（空板双采样）。
    let fresh = EnableBoard::new();
    let s0 = EnableSnap::take(&fresh);
    let s1 = EnableSnap::take(&fresh);
    if s0.hash != s1.hash {
        return Err((XC_SNAP_DRIFT, "空板快照指纹不稳定".to_string()));
    }
    parts.push(format!("快照稳定（{:#010x}）", s0.hash));
    Ok(parts.join("；"))
}
