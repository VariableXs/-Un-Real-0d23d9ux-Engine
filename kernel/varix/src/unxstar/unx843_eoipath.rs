//! unxf843_unx843_eoipath —— UNX-F0843 MSI/MSI-X 向量 EOI 路径与终结点裁决。
//! 判据原文（CoRun Unxreal 主册 UNX-A1-B09）：UNX-F0843 · MSI/MSI-X 向量 EOI 路径与
//! 终结点裁决 · 判据 J1：MSI-X 中断 EOI 路径与 IRQ 类一致（10 万事件行为矩阵一致率
//! 100%），终结点裁决账正确。
//! · 组件：MSI/MSI-X 向量账（32/2048 语义、地址/数据对纯函数、多向量使能位图）
//!   + EOI 路径五相流水状态机（事件入→路由→服务→EOI 写→终结点裁决，与 IRQ 类
//!   路径同构逐相对拍）+ 终结点裁决闭集（四态+依据账注）+ Pending 位图与重发
//!   语义 + 向量泄漏账 + 异常账（三类显性）+ 滚动窗守恒 + 行为矩阵（LCG 闭式
//!   生成、分批 1 万×10 对拍）+ kv 导出。
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 逻辑 tick，全确定性
//! （LCG 固定种子），零浮点，EOI 全语义模型（禁 IO/真实 LAPIC 写），零死代码。
//! 诊断码独占 0xB5 段（全目录 grep 确认空闲）。

use alloc::{vec, vec::Vec, string::{String, ToString}, format};

/// 模块版本。
pub const F843_VERSION: &str = "f843-unx843_eoipath-v1";

/// 诊断段基址（0xB5 段，独占）。
pub const F843_SEG_BASE: u16 = 0xB500;
// ---------------------------------------------------------------------------
// 一、诊断码闭集（独占 0xB5 段，17 码互斥 + 段位守卫）
// ---------------------------------------------------------------------------

/// F0843 诊断码。独占 `0xB5xx` 段。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EoiCode {
    /// 0xB501 未注册向量 EOI（事件/EOI 打在未注册向量上）。
    VecNotRegistered,
    /// 0xB502 重复 EOI（同一次投递被终结两次）。
    EoiDuplicate,
    /// 0xB503 乱序 EOI（EOI 序号回退或早到）。
    EoiOutOfOrder,
    /// 0xB504 向量越界（超向量域语义界）。
    VecOverflow,
    /// 0xB505 MSI 多向量超 32 上限。
    MsiVecLimit,
    /// 0xB506 MSI-X 超 2048 表项上限。
    MsixVecLimit,
    /// 0xB507 地址/数据对构造校验失败（回读不符）。
    AddrDataPair,
    /// 0xB508 Pending 位图损坏（位态矛盾或越界）。
    PendingBitmapCorrupt,
    /// 0xB509 in-flight 不配平（向量泄漏）。
    InflightUnbalanced,
    /// 0xB50A 终态越出闭集（非四态）。
    VerdictOutside,
    /// 0xB50B 行为矩阵逐事件不一致（双路对拍失配）。
    MatrixMismatch,
    /// 0xB50C 批次序破缺（批次号/批内序不连续）。
    BatchSeqBroken,
    /// 0xB50D LCG 种子为零（确定性破坏）。
    LcgSeedZero,
    /// 0xB50E 滚动窗守恒破缺（入账≠终态账）。
    WindowConserve,
    /// 0xB50F 五相流水相位迁移非法。
    PhaseInvalid,
    /// 0xB510 kv 导出账不平（字段缺失或合计不符）。
    KvExportBad,
    /// 0xB511 段位守卫失败（诊断码越出本模块段）。
    SegmentGuard,
}

impl EoiCode {
    /// 全量闭集（声明序与码值严格递增对应）。
    pub const ALL: [EoiCode; 17] = [
        EoiCode::VecNotRegistered, EoiCode::EoiDuplicate, EoiCode::EoiOutOfOrder,
        EoiCode::VecOverflow, EoiCode::MsiVecLimit, EoiCode::MsixVecLimit,
        EoiCode::AddrDataPair, EoiCode::PendingBitmapCorrupt, EoiCode::InflightUnbalanced,
        EoiCode::VerdictOutside, EoiCode::MatrixMismatch, EoiCode::BatchSeqBroken,
        EoiCode::LcgSeedZero, EoiCode::WindowConserve, EoiCode::PhaseInvalid,
        EoiCode::KvExportBad, EoiCode::SegmentGuard,
    ];

    /// wire 码值（连续十六进制，与声明序恒等）。
    pub const fn code(self) -> u16 {
        match self {
            EoiCode::VecNotRegistered => 0xB501,
            EoiCode::EoiDuplicate => 0xB502,
            EoiCode::EoiOutOfOrder => 0xB503,
            EoiCode::VecOverflow => 0xB504,
            EoiCode::MsiVecLimit => 0xB505,
            EoiCode::MsixVecLimit => 0xB506,
            EoiCode::AddrDataPair => 0xB507,
            EoiCode::PendingBitmapCorrupt => 0xB508,
            EoiCode::InflightUnbalanced => 0xB509,
            EoiCode::VerdictOutside => 0xB50A,
            EoiCode::MatrixMismatch => 0xB50B,
            EoiCode::BatchSeqBroken => 0xB50C,
            EoiCode::LcgSeedZero => 0xB50D,
            EoiCode::WindowConserve => 0xB50E,
            EoiCode::PhaseInvalid => 0xB50F,
            EoiCode::KvExportBad => 0xB510,
            EoiCode::SegmentGuard => 0xB511,
        }
    }

    /// 中文诊断名。
    pub const fn name(self) -> &'static str {
        match self {
            EoiCode::VecNotRegistered => "未注册向量 EOI（事件/EOI 打在未注册向量上）",
            EoiCode::EoiDuplicate => "重复 EOI（同一次投递被终结两次）",
            EoiCode::EoiOutOfOrder => "乱序 EOI（EOI 序号回退或早到）",
            EoiCode::VecOverflow => "向量越界（超向量域语义界）",
            EoiCode::MsiVecLimit => "MSI 多向量超 32 上限",
            EoiCode::MsixVecLimit => "MSI-X 超 2048 表项上限",
            EoiCode::AddrDataPair => "地址/数据对构造校验失败（回读不符）",
            EoiCode::PendingBitmapCorrupt => "Pending 位图损坏（位态矛盾或越界）",
            EoiCode::InflightUnbalanced => "in-flight 不配平（向量泄漏）",
            EoiCode::VerdictOutside => "终态越出闭集（非四态）",
            EoiCode::MatrixMismatch => "行为矩阵逐事件不一致（双路对拍失配）",
            EoiCode::BatchSeqBroken => "批次序破缺（批次号/批内序不连续）",
            EoiCode::LcgSeedZero => "LCG 种子为零（确定性破坏）",
            EoiCode::WindowConserve => "滚动窗守恒破缺（入账≠终态账）",
            EoiCode::PhaseInvalid => "五相流水相位迁移非法",
            EoiCode::KvExportBad => "kv 导出账不平（字段缺失或合计不符）",
            EoiCode::SegmentGuard => "段位守卫失败（诊断码越出本模块段）",
        }
    }

    /// 可操作修复建议（每条诊断必须带下一步动作）。
    pub const fn suggestion(self) -> &'static str {
        match self {
            EoiCode::VecNotRegistered => "先注册向量再入事件，按路由表补登",
            EoiCode::EoiDuplicate => "按一次投递一次 EOI 去重账归一",
            EoiCode::EoiOutOfOrder => "按投递序重排 EOI 后重跑",
            EoiCode::VecOverflow => "按 32/2048 语义界收紧向量域",
            EoiCode::MsiVecLimit => "按 MSI 32 向量上限分桶",
            EoiCode::MsixVecLimit => "按 MSI-X 2048 表项上限截断",
            EoiCode::AddrDataPair => "按地址/数据对纯函数重建并回读校验",
            EoiCode::PendingBitmapCorrupt => "按位图不变量重建 Pending 账",
            EoiCode::InflightUnbalanced => "按 in-flight 入出配平律补账",
            EoiCode::VerdictOutside => "按四态闭集收敛终态",
            EoiCode::MatrixMismatch => "按逐相对拍定位失配相位并归因",
            EoiCode::BatchSeqBroken => "按批次序闭式重建后重跑",
            EoiCode::LcgSeedZero => "换非零固定种子重跑",
            EoiCode::WindowConserve => "按滚动窗守恒律重建账本",
            EoiCode::PhaseInvalid => "按五相顺序重放事务",
            EoiCode::KvExportBad => "按导出不变量补齐字段",
            EoiCode::SegmentGuard => "核对诊断码分配表，收回越段码",
        }
    }

    /// 人话原因（分配式诊断，供 kv 与报告引用）。
    pub fn reason(self) -> String {
        let mut s = String::from("UNX-F0843/");
        s.push_str(self.name());
        s.push_str("；建议：");
        s.push_str(self.suggestion());
        s
    }
}

/// 段位守卫：全部诊断码必须落在 0xB500..=0xB5FF 且严格递增。
pub fn seg_guard() -> Result<(), EoiCode> {
    let lo = F843_SEG_BASE;
    let hi = F843_SEG_BASE + 0xFF;
    let mut prev: u16 = 0;
    for c in EoiCode::ALL.iter() {
        let v = c.code();
        if v < lo || v > hi {
            return Err(EoiCode::SegmentGuard);
        }
        if prev != 0 && v <= prev {
            return Err(EoiCode::SegmentGuard);
        }
        prev = v;
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// 二、IRQ 类与向量域常量（MSI 32 / MSI-X 2048 语义界）
// ---------------------------------------------------------------------------

/// IRQ 类（EOI 路径按类同构；判据对拍对象为 MSI-X 类与 IRQ 线类）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IrqClass {
    /// MSI（多消息，最多 32 向量）。
    Msi,
    /// MSI-X（每设备 2048 表项）。
    Msix,
    /// IRQ 线类（IO APIC 路由线）。
    IrqLine,
}

impl IrqClass {
    /// 类名（kv 用）。
    pub const fn as_str(self) -> &'static str {
        match self {
            IrqClass::Msi => "msi",
            IrqClass::Msix => "msix",
            IrqClass::IrqLine => "irq_line",
        }
    }

    /// 类向量域上限（语义界）。
    pub const fn cap(self) -> u16 {
        match self {
            IrqClass::Msi => MSI_CAP,
            IrqClass::Msix => MSIX_CAP,
            IrqClass::IrqLine => MSIX_CAP,
        }
    }
}

/// MSI 多向量上限（每设备 32）。
pub const MSI_CAP: u16 = 32;
/// MSI-X 表项上限（每函数 2048）。
pub const MSIX_CAP: u16 = 2048;
/// 向量域下界（0..32 为 CPU 保留/异常向量，MSI 不得占用）。
pub const REG_LO: u16 = 32;
/// 注册向量上界（含）——语义模型中 32..=255 为已注册向量。
pub const REG_HI: u16 = 255;
/// 语义模型 CPU 数（路由目标域）。
pub const NR_CPU: u64 = 8;

/// 判定向量是否已注册（MSI-X/IRQ 线域：32..=255 闭区间）。
pub fn is_registered(vector: u16) -> bool {
    vector >= REG_LO && vector <= REG_HI
}

/// 类注册窗（MSI 域为消息向量空间 0..=31；MSI-X/IRQ 线域为 32..=255）。
pub fn reg_window(class: IrqClass) -> (u16, u16) {
    match class {
        IrqClass::Msi => (0, MSI_CAP - 1),
        _ => (REG_LO, REG_HI),
    }
}

/// 判定向量是否落在类注册窗内。
pub fn in_reg_window(class: IrqClass, vector: u16) -> bool {
    let (lo, hi) = reg_window(class);
    vector >= lo && vector <= hi
}

/// 规范路由（闭式：cpu = (v*7+3) mod 8）——两路同源，对拍一致的基础。
pub fn canonical_cpu(vector: u16) -> u8 {
    ((vector as u64 * 7 + 3) % NR_CPU) as u8
}

// ---------------------------------------------------------------------------
// 三、MSI/MSI-X 地址-数据对构造纯函数（含回读校验）
// ---------------------------------------------------------------------------

/// MSI/MSI-X 消息地址-数据对（语义模型，禁真实 MMIO）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MsiAddrData {
    /// 消息地址（FSB 上送目的 APIC 编码）。
    pub addr: u64,
    /// 消息数据（低字节向量 + 投递/触发模式位）。
    pub data: u32,
}

impl MsiAddrData {
    /// 从地址回读目的 APIC（bits 12..19）。
    pub fn dest_apic(&self) -> u8 {
        ((self.addr >> 12) & 0xFF) as u8
    }

    /// 从数据回读向量（bits 0..7）。
    pub fn vector(&self) -> u16 {
        (self.data & 0xFF) as u16
    }

    /// 触发模式位（bit 14：0 边沿 / 1 电平）。
    pub fn level_trigger(&self) -> bool {
        (self.data & (1 << 14)) != 0
    }
}

/// MSI 地址-数据对构造（纯函数）。规则：① 向量 < 32（CPU 保留域）拒绝；
/// ② 向量 > 255 拒绝；③ 地址 = 0xFEE00000 基 + dest<<12 + 逻辑目标模式位
/// (bit2) + 重定向提示位(bit3)；④ 数据 = 向量 | 电平触发位(bit14) | 电平
/// 断言位(bit15)；⑤ 构造后回读校验，不符即诊断 AddrDataPair。
pub fn msi_addr_data(
    dest_apic: u8,
    logical_mode: bool,
    level_trigger: bool,
    vector: u16,
) -> Result<MsiAddrData, EoiCode> {
    if vector < REG_LO {
        return Err(EoiCode::AddrDataPair);
    }
    if vector > 0xFF {
        return Err(EoiCode::VecOverflow);
    }
    let mut addr: u64 = 0xFEE0_0000;
    addr |= (dest_apic as u64) << 12;
    if logical_mode {
        addr |= 1 << 2;
    }
    addr |= 1 << 3; // redirect hint：MSI 走 FSB 重定向
    let mut data: u32 = vector as u32;
    if level_trigger {
        data |= 1 << 14;
        data |= 1 << 15; // 初始断言电平
    }
    let pair = MsiAddrData { addr, data };
    // 回读校验（构造正确性闭环）
    if pair.vector() != vector || pair.dest_apic() != dest_apic {
        return Err(EoiCode::AddrDataPair);
    }
    if level_trigger != pair.level_trigger() {
        return Err(EoiCode::AddrDataPair);
    }
    Ok(pair)
}

/// MSI-X 表项地址-数据对构造（纯函数；表项 16 字节步进，msg addr 在 +0）。
/// 规则：① index >= 2048 拒绝；② 向量取语义基 32 + index；③ 地址编码
/// dest APIC 与表页指纹；④ 数据低字节 = 语义向量；⑤ 回读校验闭环。
pub fn msix_msg_pair(table_base: u64, index: u16, dest_apic: u8) -> Result<MsiAddrData, EoiCode> {
    if index >= MSIX_CAP {
        return Err(EoiCode::MsixVecLimit);
    }
    let vector = REG_LO + index;
    let mut addr: u64 = 0xFEE0_0000;
    addr |= (dest_apic as u64) << 12;
    addr |= (table_base & 0xFFF) << 20; // 语义模型：表页低 12 位指纹进高位段
    let data: u32 = vector as u32;
    let pair = MsiAddrData { addr, data };
    if pair.vector() != vector || pair.dest_apic() != dest_apic {
        return Err(EoiCode::AddrDataPair);
    }
    Ok(pair)
}

/// MSI 多向量使能位数校验（1..=32）。
pub fn msi_multi_count_ok(n: u32) -> Result<(), EoiCode> {
    if n == 0 || n > MSI_CAP as u32 {
        return Err(EoiCode::MsiVecLimit);
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// 四、使能位图与 Pending 位图（多向量使能 / 重发语义）
// ---------------------------------------------------------------------------

/// 通用位图（u64 字阵列，越界显性报错）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Bitmap {
    words: Vec<u64>,
    bits: u16,
}

impl Bitmap {
    /// 构造（全清零）。
    pub fn new(bits: u16) -> Result<Bitmap, EoiCode> {
        if bits == 0 {
            return Err(EoiCode::VecOverflow);
        }
        let words = ((bits as usize) + 63) / 64;
        Ok(Bitmap {
            words: vec![0u64; words],
            bits,
        })
    }

    /// 位数。
    pub fn bits(&self) -> u16 {
        self.bits
    }

    /// 读位（越界 → PendingBitmapCorrupt）。
    pub fn test(&self, idx: u16) -> Result<bool, EoiCode> {
        if idx >= self.bits {
            return Err(EoiCode::PendingBitmapCorrupt);
        }
        let w = (idx as usize) / 64;
        let b = (idx as usize) % 64;
        match self.words.get(w) {
            Some(word) => Ok((word >> b) & 1 == 1),
            None => Err(EoiCode::PendingBitmapCorrupt),
        }
    }

    /// 写位原语（on=置位 / off=清位；越界 → PendingBitmapCorrupt）。
    fn bit_write(&mut self, idx: u16, on: bool) -> Result<(), EoiCode> {
        if idx >= self.bits {
            return Err(EoiCode::PendingBitmapCorrupt);
        }
        let w = (idx as usize) / 64;
        let b = (idx as usize) % 64;
        match self.words.get_mut(w) {
            Some(word) => {
                if on {
                    *word |= 1u64 << b;
                } else {
                    *word &= !(1u64 << b);
                }
                Ok(())
            }
            None => Err(EoiCode::PendingBitmapCorrupt),
        }
    }

    /// 置位（越界 → PendingBitmapCorrupt）。
    pub fn set(&mut self, idx: u16) -> Result<(), EoiCode> {
        self.bit_write(idx, true)
    }

    /// 清位（越界 → PendingBitmapCorrupt）。
    pub fn clear(&mut self, idx: u16) -> Result<(), EoiCode> {
        self.bit_write(idx, false)
    }

    /// 置位计数（账面用）。
    pub fn popcount(&self) -> u64 {
        let mut n = 0u64;
        for w in self.words.iter() {
            let mut x = *w;
            while x != 0 {
                x &= x.wrapping_sub(1);
                n += 1;
            }
        }
        n
    }

    /// 前 n 位连续置一（多向量使能掩码语义）。
    pub fn fill_low(n: u16) -> Result<Bitmap, EoiCode> {
        let mut bm = Bitmap::new(n)?;
        for i in 0..n {
            bm.set(i)?;
        }
        Ok(bm)
    }
}

/// 多向量使能位图构造（MSI n∈1..=32；MSI-X n∈1..=2048）。
pub fn enable_bitmap(class: IrqClass, n: u16) -> Result<Bitmap, EoiCode> {
    let cap = class.cap();
    if n == 0 || n > cap {
        return match class {
            IrqClass::Msi => Err(EoiCode::MsiVecLimit),
            _ => Err(EoiCode::MsixVecLimit),
        };
    }
    Bitmap::fill_low(n)
}

// ---------------------------------------------------------------------------
// 五、向量单元账与向量泄漏账（in-flight 入出配平）
// ---------------------------------------------------------------------------

/// 向量服务相位（单元级状态机：Idle→InService→AwaitingEoi→Idle）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CellPhase {
    /// 空闲（无在途投递）。
    Idle,
    /// 服务中（handler 执行窗）。
    InService,
    /// 服务完毕，等待 EOI 写。
    AwaitingEoi,
}

/// 单向量账元（ arrivals/EOI/in-flight/重发/末次序号全记账）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VecCell {
    /// 当前相位。
    pub phase: CellPhase,
    /// 在途投递数（入出配平对象）。
    pub inflight: u32,
    /// 累计受理投递数。
    pub arrivals: u64,
    /// 累计有效 EOI 数。
    pub eois: u64,
    /// 挂起（pending）计数。
    pub pending_n: u32,
    /// 重发次数（EOI 时 pending 落地）。
    pub resends: u64,
    /// 改道投递次数（busy 重路由）。
    pub rerouted: u64,
    /// 末次有效 EOI 序号（0=从未）。
    pub last_eoi_seq: u64,
    /// 末次服务结束逻辑 tick。
    pub last_tick: u64,
}

impl VecCell {
    /// 空白元。
    pub const fn new() -> VecCell {
        VecCell {
            phase: CellPhase::Idle,
            inflight: 0,
            arrivals: 0,
            eois: 0,
            pending_n: 0,
            resends: 0,
            rerouted: 0,
            last_eoi_seq: 0,
            last_tick: 0,
        }
    }
}

/// 向量泄漏账（in-flight 开/闭配平 + 峰值 + 终扫零）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LeakLedger {
    /// in-flight 开账累计（受理投递 + 重发改道开账）。
    pub opened: u64,
    /// in-flight 闭账累计（有效 EOI + 改道闭账）。
    pub closed: u64,
    /// 峰值在途（语义观测）。
    pub peak: u64,
}

impl LeakLedger {
    /// 空账。
    pub const fn new() -> LeakLedger {
        LeakLedger {
            opened: 0,
            closed: 0,
            peak: 0,
        }
    }

    /// 配平判据：开账=闭账。
    pub fn balanced(&self) -> bool {
        self.opened == self.closed
    }
}

// ---------------------------------------------------------------------------
// 六、异常账（三类显性：未注册 EOI / 重复 EOI / 乱序 EOI）
// ---------------------------------------------------------------------------

/// 异常账（EOI 路径三类显性异常，含首次出现序号）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AnomalyLedger {
    /// 未注册向量 EOI 计数。
    pub unreg: u64,
    /// 重复 EOI 计数（含无投递可终结的空 EOI）。
    pub dup: u64,
    /// 乱序 EOI 计数（序号回退或早到）。
    pub order: u64,
    /// 首次未注册异常事件序号（0=未出现）。
    pub first_unreg_seq: u64,
    /// 首次重复异常事件序号（0=未出现）。
    pub first_dup_seq: u64,
    /// 首次乱序异常事件序号（0=未出现）。
    pub first_order_seq: u64,
}

impl AnomalyLedger {
    /// 空账。
    pub const fn new() -> AnomalyLedger {
        AnomalyLedger {
            unreg: 0,
            dup: 0,
            order: 0,
            first_unreg_seq: 0,
            first_dup_seq: 0,
            first_order_seq: 0,
        }
    }

    /// 记一类异常（首次出现序号落账）。
    pub fn record(&mut self, which: u8, seq: u64) {
        let (cnt, first) = match which {
            0 => (&mut self.unreg, &mut self.first_unreg_seq),
            1 => (&mut self.dup, &mut self.first_dup_seq),
            _ => (&mut self.order, &mut self.first_order_seq),
        };
        *cnt += 1;
        if *first == 0 {
            *first = seq;
        }
    }

    /// 三类合计。
    pub fn total(&self) -> u64 {
        self.unreg + self.dup + self.order
    }
}

// ---------------------------------------------------------------------------
// 七、终结点裁决闭集与裁决依据账注
// ---------------------------------------------------------------------------

/// 终结点裁决闭集（EOI 写后事件终态四态，判据强制闭集）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    /// 正常终结：受理→服务→EOI 写→完成。
    Completed,
    /// 虚假终结：无投递可终结（空 EOI / 重复 EOI）。
    Spurious,
    /// 改道终结：busy 重路由 / pending 重发 / 路由改写。
    Rerouted,
    /// 丢弃终结：未注册 / 未使能 / 乱序 EOI 无效。
    Dropped,
}

impl Verdict {
    /// wire 码（1..4，Evt5 落账用）。
    pub const fn code(self) -> u8 {
        match self {
            Verdict::Completed => 1,
            Verdict::Spurious => 2,
            Verdict::Rerouted => 3,
            Verdict::Dropped => 4,
        }
    }

    /// 由 wire 码还原（越界 → None，由调用方转诊断）。
    pub fn from_code(c: u8) -> Option<Verdict> {
        match c {
            1 => Some(Verdict::Completed),
            2 => Some(Verdict::Spurious),
            3 => Some(Verdict::Rerouted),
            4 => Some(Verdict::Dropped),
            _ => None,
        }
    }

    /// 态名。
    pub const fn as_str(self) -> &'static str {
        match self {
            Verdict::Completed => "Completed",
            Verdict::Spurious => "Spurious",
            Verdict::Rerouted => "Rerouted",
            Verdict::Dropped => "Dropped",
        }
    }

    /// 闭集下标（0..4，账列索引用）。
    pub const fn idx(self) -> usize {
        match self {
            Verdict::Completed => 0,
            Verdict::Spurious => 1,
            Verdict::Rerouted => 2,
            Verdict::Dropped => 3,
        }
    }
}

/// 裁决依据闭集（九种依据，账注到（终态×依据）矩阵）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Rationale {
    /// 正常完成。
    NormalComplete,
    /// 无投递可终结（空 EOI）。
    NoDelivery,
    /// 重复 EOI。
    DupEoi,
    /// 乱序 EOI 无效。
    OrderViolation,
    /// 向量未注册。
    NotRegistered,
    /// 向量未使能。
    NotEnabled,
    /// busy 重路由（服务重叠改道）。
    BusyReroute,
    /// pending 重发落地。
    PendingResend,
    /// 路由改写（改道投递）。
    RouteRewrite,
}

impl Rationale {
    /// 依据下标（0..9，账注矩阵列）。
    pub const fn idx(self) -> usize {
        match self {
            Rationale::NormalComplete => 0,
            Rationale::NoDelivery => 1,
            Rationale::DupEoi => 2,
            Rationale::OrderViolation => 3,
            Rationale::NotRegistered => 4,
            Rationale::NotEnabled => 5,
            Rationale::BusyReroute => 6,
            Rationale::PendingResend => 7,
            Rationale::RouteRewrite => 8,
        }
    }

    /// 依据人话账注（裁决依据账注判据要求）。
    pub const fn note(self) -> &'static str {
        match self {
            Rationale::NormalComplete => "受理→服务→EOI 写链路完整，正常终结",
            Rationale::NoDelivery => "向量空闲无在途投递，EOI 落空判虚假",
            Rationale::DupEoi => "同投递二次 EOI，按去重账判虚假",
            Rationale::OrderViolation => "EOI 序号回退或早到，按序账判无效丢弃",
            Rationale::NotRegistered => "向量不在注册域 32..=255，入口即弃",
            Rationale::NotEnabled => "向量未使能（使能位图清位），入口即弃",
            Rationale::BusyReroute => "服务窗重叠，改道备用 CPU 即时终结",
            Rationale::PendingResend => "服务中再入挂起，EOI 后重发落地改道终结",
            Rationale::RouteRewrite => "路由改写生效，按改道路径终结",
        }
    }
}

/// 终结点裁决账（终态直方图 × 依据账注矩阵 + 守恒校验）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VerdictLedger {
    /// 终态直方图（Completed/Spurious/Rerouted/Dropped）。
    pub hist: [u64; 4],
    /// 依据账注矩阵 basis[终态][依据]。
    pub basis: [[u64; 9]; 4],
}

impl VerdictLedger {
    /// 空账。
    pub const fn new() -> VerdictLedger {
        VerdictLedger {
            hist: [0; 4],
            basis: [[0; 9]; 4],
        }
    }

    /// 记一笔（终态+依据）。
    pub fn record(&mut self, v: Verdict, r: Rationale) {
        self.hist[v.idx()] += 1;
        self.basis[v.idx()][r.idx()] += 1;
    }

    /// 终态合计。
    pub fn total(&self) -> u64 {
        let mut t = 0u64;
        for i in 0..4 {
            t += self.hist[i];
        }
        t
    }

    /// 守恒：每终态行的依据合计 == 终态计数（裁决账正确性判据）。
    pub fn conserved(&self) -> bool {
        for i in 0..4 {
            let row: u64 = self.basis[i].iter().sum();
            if row != self.hist[i] {
                return false;
            }
        }
        true
    }
}

// ---------------------------------------------------------------------------
// 八、事件五元组、LCG 与 10 万事件闭式生成
// ---------------------------------------------------------------------------

/// 事件种类闭集（七类，注入确定性异常场景）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EvtKind {
    /// 正常投递（受理→服务→EOI→裁决）。
    NormalFire,
    /// 服务中再入挂起→EOI 后重发落地。
    ResendAfterPend,
    /// 路由改写投递（改道终结）。
    RerouteReq,
    /// 虚假 EOI（无投递可终结）。
    SpuriousEoi,
    /// 重复 EOI（同投递终结两次）。
    DupEoi,
    /// 乱序 EOI（序号回退探针后按序补写）。
    OutOfOrderEoi,
    /// 未注册向量投递。
    UnregFire,
}

impl EvtKind {
    /// wire 码（Evt5 类型字段，1..7）。
    pub const fn code(self) -> u8 {
        match self {
            EvtKind::NormalFire => 1,
            EvtKind::ResendAfterPend => 2,
            EvtKind::RerouteReq => 3,
            EvtKind::SpuriousEoi => 4,
            EvtKind::DupEoi => 5,
            EvtKind::OutOfOrderEoi => 6,
            EvtKind::UnregFire => 7,
        }
    }

    /// 由闭式选择值还原（模 100 分桶，确定性）。
    pub fn pick(sel: u64) -> EvtKind {
        let m = sel % 100;
        if m < 60 {
            EvtKind::NormalFire
        } else if m < 70 {
            EvtKind::ResendAfterPend
        } else if m < 75 {
            EvtKind::RerouteReq
        } else if m < 80 {
            EvtKind::SpuriousEoi
        } else if m < 84 {
            EvtKind::DupEoi
        } else if m < 88 {
            EvtKind::OutOfOrderEoi
        } else {
            EvtKind::UnregFire
        }
    }
}

/// 事件五元组（类型/向量/tick/服务时长/裁决结果）——行为矩阵对拍对象。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Evt5 {
    /// 类型（EvtKind wire 码）。
    pub kind: u8,
    /// 向量。
    pub vector: u16,
    /// 逻辑 tick（禁墙钟，纯 u64）。
    pub tick: u64,
    /// 服务时长（tick 数）。
    pub svc_len: u32,
    /// 裁决结果（Verdict wire 码）。
    pub verdict: u8,
}

/// EOI 事务输入事件（LCG 闭式生成）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EoiEvent {
    /// 全局单调序号（1 起）。
    pub seq: u64,
    /// 事件种类。
    pub kind: EvtKind,
    /// 目标向量。
    pub vector: u16,
    /// 到达逻辑 tick。
    pub tick: u64,
    /// 服务时长（tick）。
    pub svc_len: u32,
}

/// 五相流水相迹（逐相对拍对象：每相一个语义观测量）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PhaseTrace {
    /// 五相观测量：入相/路由/服务/EOI 写/裁决。
    pub datum: [u32; 5],
}

/// LCG 确定性发生器（64 位 Knuth 乘加；种子非零强制）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Lcg64 {
    state: u64,
}

impl Lcg64 {
    /// 乘子。
    pub const MUL: u64 = 6_364_136_223_846_793_005;
    /// 增量。
    pub const ADD: u64 = 1_442_695_040_888_963_407;

    /// 构造（种子为零 → LcgSeedZero，确定性红线）。
    pub fn new(seed: u64) -> Result<Lcg64, EoiCode> {
        if seed == 0 {
            return Err(EoiCode::LcgSeedZero);
        }
        Ok(Lcg64 { state: seed })
    }

    /// 下一步（闭式确定）。
    pub fn next(&mut self) -> u64 {
        self.state = self
            .state
            .wrapping_mul(Lcg64::MUL)
            .wrapping_add(Lcg64::ADD);
        self.state
    }
}

/// 行为矩阵事件总量（判据：10 万）。
pub const TOTAL_EVENTS: u64 = 100_000;
/// 批事件数（分批对拍：1 万/批）。
pub const BATCH_EVENTS: u64 = 10_000;
/// 批数。
pub const BATCHES: u64 = 10;
/// 滚动窗宽（守恒窗）。
pub const WINDOW: u64 = 1_000;
/// 行为矩阵主种子（固定非零，全确定性）。
pub const F843_SEED: u64 = 0xB5_5EED_4643_0001;
/// MSI 独立场景种子（固定非零）。
pub const MSI_SEED: u64 = 0xB5_5EED_4643_0002;
/// MSI 独立场景事件数。
pub const MSI_EVENTS: u64 = 1_000;

/// 10 万事件闭式生成（LCG 固定种子；向量域按种类分桶）。规则：① 种子零
/// → LcgSeedZero；② 序号 1..=100000 单调；③ tick 单调递增（步距 1..12）；
/// ④ 服务时长 1..8 tick；⑤ UnregFire 取 256..=2047 未注册桶，其余取
/// 32..=255 注册桶。
pub fn gen_events() -> Result<Vec<EoiEvent>, EoiCode> {
    let mut lcg = Lcg64::new(F843_SEED)?;
    let mut out: Vec<EoiEvent> = Vec::with_capacity(TOTAL_EVENTS as usize);
    let mut tick: u64 = 0;
    let unreg_span = MSIX_CAP as u64 - REG_HI as u64 - 1; // 1792（256..=2047）
    let reg_span = REG_HI as u64 - REG_LO as u64 + 1; // 224
    for i in 0..TOTAL_EVENTS {
        let s1 = lcg.next();
        let s2 = lcg.next();
        let kind = EvtKind::pick(s1 >> 7);
        let vector: u16 = match kind {
            EvtKind::UnregFire => (REG_HI as u64 + 1 + s2 % unreg_span) as u16,
            _ => (REG_LO as u64 + s2 % reg_span) as u16,
        };
        tick += 1 + (s1 >> 13) % 12;
        let svc_len = 1 + ((s2 >> 9) % 8) as u32;
        out.push(EoiEvent { seq: i + 1, kind, vector, tick, svc_len });
    }
    Ok(out)
}

// ---------------------------------------------------------------------------
// 九、EOI 路径五相流水状态机（事件入→路由→服务→EOI 写→终结点裁决）
// ---------------------------------------------------------------------------

/// 相位数（五相）。
pub const PHASE_N: usize = 5;
/// 相名（kv/报告用）。
pub const PHASE_NAMES: [&str; PHASE_N] = ["事件入", "路由", "服务", "EOI 写", "终结点裁决"];

// 入相观测量
const ARR_NONE: u32 = 0;
const ARR_ACCEPTED: u32 = 1;
const ARR_DISABLED: u32 = 2;
const ARR_BUSY: u32 = 3;
const ARR_UNREG: u32 = 4;
// EOI 写观测量
const EOI_NONE: u32 = 0;
const EOI_ACCEPTED: u32 = 1;
const EOI_RESEND: u32 = 2;
const EOI_DUP: u32 = 3;
const EOI_ORDER: u32 = 4;
const EOI_IDLE: u32 = 5;
const EOI_ORDER_MARK: u32 = 6;

/// 滚动窗计数（入账/终态账守恒对象）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WindowCnt {
    /// 窗内入账事件数。
    pub win_in: u64,
    /// 窗内终态账数。
    pub win_out: u64,
    /// 窗起始序号。
    pub lo_seq: u64,
    /// 窗结束序号。
    pub hi_seq: u64,
}

/// EOI 路径核心（按 IRQ 类实例化；MSI-X 类与 IRQ 线类同构对拍）。
#[derive(Debug, Clone)]
pub struct EoiCore {
    /// IRQ 类。
    pub class: IrqClass,
    /// 向量域上限。
    pub cap: u16,
    /// 使能位图。
    pub enabled: Bitmap,
    /// Pending 位图（重发语义载体）。
    pub pending: Bitmap,
    /// 路由表（IRQ 线类直查；-1 未注册）。
    pub route: Vec<i8>,
    /// 向量账元。
    pub cells: Vec<VecCell>,
    /// 裁决账。
    pub vledger: VerdictLedger,
    /// 异常账。
    pub anom: AnomalyLedger,
    /// 泄漏账。
    pub leak: LeakLedger,
    /// 滚动窗账。
    pub windows: Vec<WindowCnt>,
    /// 事务指纹（滚动混合，双路对拍辅助）。
    pub fingerprint: u64,
    /// 已处理事件数。
    pub events_seen: u64,
    /// 已终结事件数。
    pub terminal: u64,
    /// 受理投递总数（含改道）。
    pub delivered: u64,
    /// 重发开账数。
    pub resend_open: u64,
    /// 重发闭账数。
    pub resend_close: u64,
    /// 改道开账数。
    pub reroute_open: u64,
    /// 改道闭账数。
    pub reroute_close: u64,
    /// 最新逻辑 tick。
    pub last_tick: u64,
    /// 窗内累计（滚动）。
    win_in: u64,
    win_out: u64,
    win_lo: u64,
}

impl EoiCore {
    /// 构造（按类建域；注册域 32..=255 全使能，路由闭式同源）。
    pub fn new(class: IrqClass) -> Result<EoiCore, EoiCode> {
        let cap = class.cap();
        if cap == 0 {
            return Err(EoiCode::VecOverflow);
        }
        let mut enabled = Bitmap::new(cap)?;
        let mut route: Vec<i8> = Vec::with_capacity(cap as usize);
        let mut cells: Vec<VecCell> = Vec::with_capacity(cap as usize);
        for v in 0..cap {
            if in_reg_window(class, v) {
                enabled.set(v)?;
                route.push(canonical_cpu(v) as i8);
            } else {
                route.push(-1);
            }
            cells.push(VecCell::new());
        }
        Ok(EoiCore {
            class,
            cap,
            enabled,
            pending: Bitmap::new(cap)?,
            route,
            cells,
            vledger: VerdictLedger::new(),
            anom: AnomalyLedger::new(),
            leak: LeakLedger::new(),
            windows: Vec::new(),
            fingerprint: 0x9E37_79B9_7F4A_7C15,
            events_seen: 0,
            terminal: 0,
            delivered: 0,
            resend_open: 0,
            resend_close: 0,
            reroute_open: 0,
            reroute_close: 0,
            last_tick: 0,
            win_in: 0,
            win_out: 0,
            win_lo: 0,
        })
    }

    /// 注册域内向量数。
    pub fn registered_count(&self) -> u64 {
        let mut n = 0u64;
        for v in 0..self.cap {
            if is_registered(v) {
                n += 1;
            }
        }
        n
    }

    /// 路由相：MSI-X 类经地址/数据对编解码，IRQ 线类直查路由表——两表示
    /// 同源规范映射（对拍判据的结构基础）。
    fn route_of(&self, vector: u16) -> Result<(u8, bool), EoiCode> {
        if vector >= self.cap {
            return Err(EoiCode::VecOverflow);
        }
        let reg = in_reg_window(self.class, vector);
        if !reg {
            return Ok((0, false));
        }
        let cpu = match self.class {
            // 地址/数据对表示：构造→回读解码（编解码闭环参与对拍）
            IrqClass::Msix => {
                let pair = msix_msg_pair(0x1000, vector - REG_LO, canonical_cpu(vector))?;
                if pair.vector() != vector {
                    return Err(EoiCode::AddrDataPair);
                }
                pair.dest_apic()
            }
            _ => match self.route.get(vector as usize) {
                Some(&c) if c >= 0 => c as u8,
                Some(_) => return Ok((0, false)),
                None => return Err(EoiCode::VecOverflow),
            },
        };
        if cpu as u64 >= NR_CPU {
            return Err(EoiCode::AddrDataPair);
        }
        Ok((cpu, true))
    }

    /// 相一：事件入（受理 / 挂起改道 / 未使能 / 未注册 / 空转分桶）。
    fn ph_arrival(&mut self, ev: &EoiEvent) -> Result<u32, EoiCode> {
        if ev.vector >= self.cap {
            return Err(EoiCode::VecOverflow);
        }
        self.events_seen += 1;
        if self.win_in == 0 {
            self.win_lo = ev.seq;
        }
        self.win_in += 1;
        match ev.kind {
            EvtKind::SpuriousEoi => return Ok(ARR_NONE),
            _ => {}
        }
        if !in_reg_window(self.class, ev.vector) {
            self.anom.record(0, ev.seq);
            return Ok(ARR_UNREG);
        }
        let idx = ev.vector as usize;
        let enabled = self.enabled.test(ev.vector)?;
        if !enabled {
            return Ok(ARR_DISABLED);
        }
        // busy 判据：服务窗重叠（ev.tick 落在上一次服务结束 tick 之前）
        let cell = match self.cells.get_mut(idx) {
            Some(c) => c,
            None => return Err(EoiCode::VecOverflow),
        };
        let busy = cell.last_tick > 0 && ev.tick < cell.last_tick;
        if busy {
            // 改道投递：备用 CPU 即时终结（开账即闭账，语义配平）
            self.delivered += 1;
            self.reroute_open += 1;
            self.reroute_close += 1;
            self.leak.opened += 1;
            self.leak.closed += 1;
            cell.rerouted += 1;
            return Ok(ARR_BUSY);
        }        self.delivered += 1;
        self.leak.opened += 1;
        cell.phase = CellPhase::InService;
        cell.arrivals += 1;
        cell.inflight += 1;
        if cell.inflight as u64 > self.leak.peak {
            self.leak.peak = cell.inflight as u64;
        }
        Ok(ARR_ACCEPTED)
    }

    /// 相二：路由（目的 CPU / 注册位 / 改写位编码进相迹）。
    fn ph_route(&mut self, ev: &EoiEvent, arrival: u32) -> Result<u32, EoiCode> {
        let (mut dest, reg) = self.route_of(ev.vector)?;
        let mut rewritten = false;
        if arrival == ARR_ACCEPTED && ev.kind == EvtKind::RerouteReq {
            dest = (dest + 1) % NR_CPU as u8;
            rewritten = true;
        }
        let mut datum: u32 = dest as u32;
        if reg {
            datum |= 1 << 8;
        }
        if rewritten {
            datum |= 1 << 9;
        }
        Ok(datum)
    }

    /// 相三：服务（受理路径推进服务窗并置 AwaitingEoi；ResendAfterPend
    /// 在此挂 pending 位——重发语义载体）。
    fn ph_service(&mut self, ev: &EoiEvent, arrival: u32) -> Result<u32, EoiCode> {
        match arrival {
            ARR_NONE | ARR_UNREG | ARR_DISABLED | ARR_BUSY => return Ok(arrival),
            _ => {}
        }
        let idx = ev.vector as usize;
        let cell = match self.cells.get_mut(idx) {
            Some(c) => c,
            None => return Err(EoiCode::VecOverflow),
        };
        cell.phase = CellPhase::AwaitingEoi;
        cell.last_tick = ev.tick + ev.svc_len as u64;
        if cell.last_tick > self.last_tick {
            self.last_tick = cell.last_tick;
        }
        if ev.kind == EvtKind::ResendAfterPend {
            self.pending.set(ev.vector)?;
            cell.pending_n += 1;
        }
        Ok(ARR_ACCEPTED)
    }

    /// EOI 写原语（序号账：seq <= last 即重复/乱序；配平闭账）。
    fn try_eoi(&mut self, seq: u64, vector: u16) -> Result<u32, EoiCode> {
        let idx = vector as usize;
        let cell = match self.cells.get_mut(idx) {
            Some(c) => c,
            None => return Err(EoiCode::VecOverflow),
        };
        match cell.phase {
            CellPhase::Idle => {
                if seq != 0 && seq == cell.last_eoi_seq {
                    return Ok(EOI_DUP);
                }
                Ok(EOI_IDLE)
            }
            CellPhase::InService | CellPhase::AwaitingEoi => {
                if seq <= cell.last_eoi_seq {
                    if seq == cell.last_eoi_seq {
                        return Ok(EOI_DUP);
                    }
                    return Ok(EOI_ORDER);
                }
                if cell.inflight == 0 {
                    return Err(EoiCode::InflightUnbalanced);
                }
                cell.inflight -= 1;
                cell.eois += 1;
                cell.last_eoi_seq = seq;
                cell.phase = CellPhase::Idle;
                self.leak.closed += 1;
                // Pending 落地 → 重发语义（开账即闭账，同事务配平）
                let mut resend = false;
                if self.pending.test(vector)? {
                    self.pending.clear(vector)?;
                    if cell.pending_n > 0 {
                        cell.pending_n -= 1;
                    }
                    cell.resends += 1;
                    self.resend_open += 1;
                    self.resend_close += 1;
                    self.leak.opened += 1;
                    self.leak.closed += 1;
                    resend = true;
                }
                Ok(if resend { EOI_RESEND } else { EOI_ACCEPTED })
            }
        }
    }

    /// 相四：EOI 写（按事件种类分路径：正常/重复/乱序探针/虚假）。
    fn ph_eoi(&mut self, ev: &EoiEvent, arrival: u32) -> Result<u32, EoiCode> {
        match ev.kind {
            EvtKind::SpuriousEoi => {
                // 虚假 EOI：向量若在服务窗重叠 → 乱序类；否则空 EOI → 重复类
                let idx = ev.vector as usize;
                let busy = match self.cells.get(idx) {
                    Some(c) => c.last_tick > 0 && ev.tick < c.last_tick,
                    None => return Err(EoiCode::VecOverflow),
                };
                if busy {
                    self.anom.record(2, ev.seq);
                    return Ok(EOI_ORDER);
                }
                self.anom.record(1, ev.seq);
                return Ok(EOI_IDLE);
            }
            _ => {}
        }
        if arrival == ARR_UNREG || arrival == ARR_DISABLED || arrival == ARR_NONE {
            return Ok(EOI_NONE);
        }
        if arrival == ARR_BUSY {
            // 改道投递的 EOI 已在备用 CPU 语义完成
            return Ok(EOI_ACCEPTED);
        }
        match ev.kind {
            EvtKind::DupEoi => {
                let first = self.try_eoi(ev.seq, ev.vector)?;
                if first != EOI_ACCEPTED {
                    return Ok(first);
                }
                // 二次 EOI：同序号重放 → 重复账
                let second = self.try_eoi(ev.seq, ev.vector)?;
                if second == EOI_DUP {
                    self.anom.record(1, ev.seq);
                }
                Ok(second)
            }
            EvtKind::OutOfOrderEoi => {
                // 乱序探针：取向量账面末次 EOI 序号的历史方向值写一次（必被
                // 拒：last-1 落乱序账，last/0 落重复账），再按序补写配平
                let probe_seq = {
                    let last = match self.cells.get(ev.vector as usize) {
                        Some(c) => c.last_eoi_seq,
                        None => return Err(EoiCode::VecOverflow),
                    };
                    if last > 1 {
                        last - 1
                    } else {
                        0
                    }
                };
                let probe = self.try_eoi(probe_seq, ev.vector)?;
                if probe == EOI_ACCEPTED || probe == EOI_RESEND {
                    return Err(EoiCode::PhaseInvalid);
                }
                if probe == EOI_DUP {
                    self.anom.record(1, ev.seq);
                } else if probe == EOI_ORDER {
                    self.anom.record(2, ev.seq);
                }
                let real = self.try_eoi(ev.seq, ev.vector)?;
                if real != EOI_ACCEPTED {
                    return Err(EoiCode::PhaseInvalid);
                }
                Ok(EOI_ORDER_MARK)
            }
            _ => self.try_eoi(ev.seq, ev.vector),
        }
    }

    /// 相五：终结点裁决（闭集四态 + 依据账注；输入=五相事实）。
    fn adjudicate(
        &mut self,
        ev: &EoiEvent,
        arrival: u32,
        route_datum: u32,
        eoi_datum: u32,
    ) -> Result<(Verdict, Rationale), EoiCode> {
        let registered = (route_datum >> 8) & 1 == 1;
        let rewritten = (route_datum >> 9) & 1 == 1;
        let (v, r) = if arrival == ARR_UNREG || !registered {
            (Verdict::Dropped, Rationale::NotRegistered)
        } else if arrival == ARR_DISABLED {
            (Verdict::Dropped, Rationale::NotEnabled)
        } else {
            match eoi_datum {
                EOI_DUP => (Verdict::Spurious, Rationale::DupEoi),
                EOI_IDLE => (Verdict::Spurious, Rationale::NoDelivery),
                EOI_ORDER | EOI_ORDER_MARK => (Verdict::Dropped, Rationale::OrderViolation),
                EOI_RESEND => (Verdict::Rerouted, Rationale::PendingResend),
                EOI_ACCEPTED => {
                    if arrival == ARR_BUSY {
                        (Verdict::Rerouted, Rationale::BusyReroute)
                    } else if rewritten {
                        (Verdict::Rerouted, Rationale::RouteRewrite)
                    } else {
                        (Verdict::Completed, Rationale::NormalComplete)
                    }
                }
                _ => return Err(EoiCode::VerdictOutside),
            }
        };
        self.vledger.record(v, r);
        self.terminal += 1;
        self.win_out += 1;
        // 事务指纹滚动混合（双路对拍辅助）
        let mix = (ev.seq as u64)
            ^ ((v.code() as u64) << 8)
            ^ ((ev.vector as u64) << 16)
            ^ (ev.tick << 24)
            ^ ((ev.svc_len as u64) << 40);
        self.fingerprint = self
            .fingerprint
            .wrapping_mul(0x100_0000_01B3)
            .wrapping_add(mix);
        // 滚动窗守恒落账
        if self.win_in >= WINDOW && self.win_out >= WINDOW {
            self.windows.push(WindowCnt {
                win_in: self.win_in,
                win_out: self.win_out,
                lo_seq: self.win_lo,
                hi_seq: ev.seq,
            });
            self.win_in = 0;
            self.win_out = 0;
            self.win_lo = 0;
        }
        Ok((v, r))
    }

    /// 单事件全事务：五相流水（事件入→路由→服务→EOI 写→终结点裁决）。
    /// 返回行为五元组 + 五相相迹（逐相对拍对象）。任一相结构违例即 Err。
    pub fn process(&mut self, ev: &EoiEvent) -> Result<(Evt5, PhaseTrace), EoiCode> {
        if ev.seq == 0 {
            return Err(EoiCode::BatchSeqBroken);
        }
        if ev.vector >= self.cap {
            return Err(EoiCode::VecOverflow);
        }
        let mut tr = PhaseTrace { datum: [0; PHASE_N] };
        // 五相流水：事件入 → 路由 → 服务 → EOI 写 → 终结点裁决
        let arrival = self.ph_arrival(ev)?;
        tr.datum[0] = arrival;
        let route_datum = self.ph_route(ev, arrival)?;
        tr.datum[1] = route_datum;
        let service_datum = self.ph_service(ev, arrival)?;
        tr.datum[2] = service_datum;
        let eoi_datum = self.ph_eoi(ev, arrival)?;
        tr.datum[3] = eoi_datum;
        let (v, _r) = self.adjudicate(ev, arrival, route_datum, eoi_datum)?;
        tr.datum[4] = v.code() as u32;
        Ok((
            Evt5 {
                kind: ev.kind.code(),
                vector: ev.vector,
                tick: ev.tick,
                svc_len: ev.svc_len,
                verdict: v.code(),
            },
            tr,
        ))
    }

    /// 泄漏账终扫：全部单元 Idle 且 in-flight 归零，开账=闭账。
    pub fn leak_clean(&self) -> bool {
        if !self.leak.balanced() {
            return false;
        }
        for c in self.cells.iter() {
            if c.phase != CellPhase::Idle || c.inflight != 0 {
                return false;
            }
        }
        true
    }

    /// 滚动窗守恒校验：每窗入账==终态账。
    pub fn windows_conserved(&self) -> bool {
        for w in self.windows.iter() {
            if w.win_in != w.win_out || w.win_in != WINDOW {
                return false;
            }
        }
        self.win_in == self.win_out
    }
}

// ---------------------------------------------------------------------------
// 十、行为矩阵（分批 1 万×10 对拍账；MSI-X 路 × IRQ 线路逐事件+逐相对拍）
// ---------------------------------------------------------------------------

/// 批对拍账。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BatchLedger {
    /// 批号（0..10）。
    pub batch: u64,
    /// 批内事件数。
    pub events: u64,
    /// 五元组逐事件一致数。
    pub evt_matched: u64,
    /// 五相逐相一致数。
    pub phase_matched: u64,
    /// 失配数。
    pub mismatches: u64,
}

impl BatchLedger {
    /// 批内全一致。
    pub fn perfect(&self) -> bool {
        self.events == self.evt_matched
            && self.events == self.phase_matched
            && self.mismatches == 0
    }
}

/// 失配样本（首个失配点归因）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MismatchSample {
    /// 事件序号。
    pub seq: u64,
    /// MSI-X 路裁决码。
    pub verdict_mx: u8,
    /// IRQ 线路裁决码。
    pub verdict_irq: u8,
}

/// 滚动窗对拍账（双路窗账逐窗比对 + 守恒）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WindowRec {
    /// 窗序。
    pub idx: u64,
    /// 窗内入账（两路同源，必须相等）。
    pub win_in: u64,
    /// 窗内终态账（两路相等且==入账）。
    pub win_out: u64,
    /// 守恒与双路一致判。
    pub ok: bool,
}

/// 行为矩阵报告（判据 J1 落账载体）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MatrixReport {
    /// 事件总量。
    pub total: u64,
    /// 五元组一致数。
    pub matched: u64,
    /// 五相相迹一致数。
    pub phase_matched: u64,
    /// 一致率（基点：matched*10000/total，零浮点）。
    pub agree_bp: u64,
    /// 批账（1 万×10）。
    pub batches: Vec<BatchLedger>,
    /// 滚动窗账。
    pub windows: Vec<WindowRec>,
    /// 失配样本（至多 8 条）。
    pub mismatch_samples: Vec<MismatchSample>,
    /// 双路指纹一致。
    pub fingerprint_eq: bool,
    /// 双路裁决账逐态一致。
    pub verdict_hist_eq: bool,
    /// 双路异常账一致。
    pub anomaly_eq: bool,
    /// 双路泄漏账配平。
    pub leak_balanced: bool,
    /// 滚动窗守恒（两路）。
    pub windows_ok: bool,
    /// 裁决账守恒（两路依据矩阵守恒）。
    pub verdict_conserved: bool,
}

impl MatrixReport {
    /// 判据 J1 闸门：一致率 100% 且各账全平。
    pub fn gate(&self) -> Result<(), EoiCode> {
        if self.total != TOTAL_EVENTS {
            return Err(EoiCode::BatchSeqBroken);
        }
        if self.matched != self.total || self.phase_matched != self.total {
            return Err(EoiCode::MatrixMismatch);
        }
        if self.agree_bp != 10_000 {
            return Err(EoiCode::MatrixMismatch);
        }
        if !self.fingerprint_eq || !self.verdict_hist_eq || !self.anomaly_eq {
            return Err(EoiCode::MatrixMismatch);
        }
        if !self.leak_balanced || !self.windows_ok || !self.verdict_conserved {
            return Err(EoiCode::WindowConserve);
        }
        Ok(())
    }
}

/// 矩阵跑批产物（报告 + 双路核心终态账，供快照与 kv 复用，免二次重放）。
pub struct MatrixRun {
    /// 对拍报告。
    pub report: MatrixReport,
    /// MSI-X 路核心（跑毕终态账）。
    pub mx: EoiCore,
    /// IRQ 线路核心（跑毕终态账）。
    pub irq: EoiCore,
}

/// 跑行为矩阵：同一条 10 万事件闭式流喂两路同构五相流水，逐事件五元组
/// 对拍 + 逐相相迹对拍，分批 1 万×10 落账，滚动窗守恒双路核账。
pub fn run_matrix() -> Result<MatrixRun, EoiCode> {
    let events = gen_events()?;
    if events.len() as u64 != TOTAL_EVENTS {
        return Err(EoiCode::BatchSeqBroken);
    }
    let mut mx = EoiCore::new(IrqClass::Msix)?;
    let mut irq = EoiCore::new(IrqClass::IrqLine)?;
    let mut batches: Vec<BatchLedger> = Vec::new();
    let mut mismatch_samples: Vec<MismatchSample> = Vec::new();
    let mut total = 0u64;
    let mut matched = 0u64;
    let mut phase_matched = 0u64;
    for b in 0..BATCHES {
        let mut bm = 0u64;
        let mut bp = 0u64;
        let mut mism = 0u64;
        for i in 0..BATCH_EVENTS {
            let flat = b * BATCH_EVENTS + i;
            let ev = match events.get(flat as usize) {
                Some(e) => e,
                None => return Err(EoiCode::BatchSeqBroken),
            };
            let (a, ta) = mx.process(ev)?;
            let (c, tc) = irq.process(ev)?;
            total += 1;
            if a == c {
                matched += 1;
                bm += 1;
            } else {
                mism += 1;
                if mismatch_samples.len() < 8 {
                    mismatch_samples.push(MismatchSample {
                        seq: ev.seq, verdict_mx: a.verdict, verdict_irq: c.verdict,
                    });
                }
            }
            if ta == tc {
                phase_matched += 1;
                bp += 1;
            }
        }
        batches.push(BatchLedger {
            batch: b,
            events: BATCH_EVENTS,
            evt_matched: bm,
            phase_matched: bp,
            mismatches: mism,
        });
    }
    // 滚动窗对拍（两路窗账逐窗比对 + 守恒）
    if mx.windows.len() != irq.windows.len() {
        return Err(EoiCode::WindowConserve);
    }
    let mut windows: Vec<WindowRec> = Vec::new();
    for i in 0..mx.windows.len() {
        let a = match mx.windows.get(i) { Some(w) => *w, None => return Err(EoiCode::WindowConserve) };
        let c = match irq.windows.get(i) { Some(w) => *w, None => return Err(EoiCode::WindowConserve) };
        let ok = a.win_in == c.win_in && a.win_out == c.win_out && a.win_in == a.win_out;
        windows.push(WindowRec { idx: i as u64, win_in: a.win_in, win_out: a.win_out, ok });
    }
    let windows_ok = {
        let mut ok = mx.windows_conserved() && irq.windows_conserved();
        for w in windows.iter() {
            if !w.ok {
                ok = false;
            }
        }
        ok
    };
    let agree_bp = if total > 0 {
        matched * 10_000 / total
    } else {
        0
    };
    let report = MatrixReport {
        total, matched, phase_matched, agree_bp, batches, windows, mismatch_samples,
        fingerprint_eq: mx.fingerprint == irq.fingerprint,
        verdict_hist_eq: mx.vledger.hist == irq.vledger.hist,
        anomaly_eq: mx.anom == irq.anom,
        leak_balanced: mx.leak_clean() && irq.leak_clean(),
        windows_ok,
        verdict_conserved: mx.vledger.conserved() && irq.vledger.conserved(),
    };
    Ok(MatrixRun { report, mx, irq })
}

// ---------------------------------------------------------------------------
// 十一、MSI 32 向量独立场景（多向量使能位图 + 32 语义界独立核账）
// ---------------------------------------------------------------------------

/// MSI 场景报告（32 向量域独立守恒，不参与 10 万矩阵对拍）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MsiReport {
    /// 事件数。
    pub events: u64,
    /// 多向量使能位数。
    pub multi_vec_n: u16,
    /// 使能位图置位数（== multi_vec_n 才平）。
    pub enabled_bits: u64,
    /// 终态合计（== events 才守恒）。
    pub terminal: u64,
    /// 受理投递数。
    pub delivered: u64,
    /// 裁决直方图。
    pub hist: [u64; 4],
    /// 异常合计。
    pub anomalies: u64,
    /// 泄漏账配平。
    pub leak_balanced: bool,
    /// 滚动窗守恒。
    pub windows_ok: bool,
    /// 裁决账守恒。
    pub verdict_conserved: bool,
}

impl MsiReport {
    /// 场景闸门。
    pub fn gate(&self) -> Result<(), EoiCode> {
        if self.events != MSI_EVENTS {
            return Err(EoiCode::BatchSeqBroken);
        }
        if self.enabled_bits != self.multi_vec_n as u64 {
            return Err(EoiCode::MsiVecLimit);
        }
        if self.terminal != self.events {
            return Err(EoiCode::WindowConserve);
        }
        if !self.leak_balanced || !self.windows_ok || !self.verdict_conserved {
            return Err(EoiCode::InflightUnbalanced);
        }
        Ok(())
    }
}

/// 跑 MSI 32 向量独立场景：多向量使能位图（n=8）、地址/数据对构造回读
/// （IDT 向量域 32..39）、1000 事件全事务（消息向量域 0..=31）、守恒与
/// 配平核账。MSI 类注册窗为消息向量空间 0..=31（reg_window 特判）。
pub fn run_msi_scenario() -> Result<MsiReport, EoiCode> {
    // 多向量使能位图（MSI n=8）
    let bitmap = enable_bitmap(IrqClass::Msi, 8)?;
    let enabled_bits = bitmap.popcount();
    // 地址/数据对构造回读（真实 MSI 语义要求 IDT 向量 >= 32，独立闭环）
    for i in 0..8u16 {
        let pair = msi_addr_data(2 + i as u8, false, false, REG_LO + i)?;
        if pair.vector() != REG_LO + i {
            return Err(EoiCode::AddrDataPair);
        }
    }
    // 1000 事件语义事务（消息向量域 0..=31 全注册）
    let mut lcg = Lcg64::new(MSI_SEED)?;
    let mut core = EoiCore::new(IrqClass::Msi)?;
    let mut tick: u64 = 0;
    for i in 0..MSI_EVENTS {
        let s1 = lcg.next();
        let s2 = lcg.next();
        let kind = EvtKind::pick(s1 >> 7);
        let vector: u16 = (s2 % MSI_CAP as u64) as u16;
        tick += 1 + (s1 >> 13) % 10;
        let svc_len = 1 + ((s2 >> 9) % 8) as u32;
        let ev = EoiEvent { seq: i + 1, kind, vector, tick, svc_len };
        let _ = core.process(&ev)?;
    }
    let hist = core.vledger.hist;
    let terminal = core.vledger.total();
    Ok(MsiReport {
        events: MSI_EVENTS,
        multi_vec_n: 8,
        enabled_bits,
        terminal,
        delivered: core.delivered,
        hist,
        anomalies: core.anom.total(),
        leak_balanced: core.leak_clean(),
        windows_ok: core.windows_conserved(),
        verdict_conserved: core.vledger.conserved(),
    })
}

// ---------------------------------------------------------------------------
// 十二、kv 导出与域快照
// ---------------------------------------------------------------------------

/// 单条 kv。
pub type Kv = (String, String);

/// 数值转字符串（alloc ToString）。
fn num_str<T: ToString>(v: T) -> String {
    v.to_string()
}

/// 裁决依据 kv 键闭集（与 Rationale::idx 序恒等）。
const RAT_KEYS: [&str; 9] = [
    "normal",
    "no_delivery",
    "dup_eoi",
    "order_violation",
    "not_registered",
    "not_enabled",
    "busy_reroute",
    "pending_resend",
    "route_rewrite",
];

/// 基点一致率格式化（bp → "100.00%" 字符串，零浮点）。
fn bp_str(bp: u64) -> String {
    let whole = bp / 100;
    let frac = bp % 100;
    format!("{}.{:02}%", whole, frac)
}

/// 行为矩阵报告 kv 导出（字段闭集；账不平 → KvExportBad）。
pub fn matrix_kv(rep: &MatrixReport) -> Result<Vec<Kv>, EoiCode> {
    let mut kv: Vec<Kv> = Vec::new();
    kv.push(("f843.version".into(), F843_VERSION.into()));
    kv.push(("f843.seg_base".into(), format!("0x{:04X}", F843_SEG_BASE)));
    kv.push(("matrix.total".into(), num_str(rep.total)));
    kv.push(("matrix.matched".into(), num_str(rep.matched)));
    kv.push(("matrix.phase_matched".into(), num_str(rep.phase_matched)));
    kv.push(("matrix.agree".into(), bp_str(rep.agree_bp)));
    kv.push(("matrix.fingerprint_eq".into(), num_str(rep.fingerprint_eq as u64)));
    kv.push(("matrix.verdict_hist_eq".into(), num_str(rep.verdict_hist_eq as u64)));
    kv.push(("matrix.anomaly_eq".into(), num_str(rep.anomaly_eq as u64)));
    kv.push(("matrix.leak_balanced".into(), num_str(rep.leak_balanced as u64)));
    kv.push(("matrix.windows_ok".into(), num_str(rep.windows_ok as u64)));
    kv.push(("matrix.verdict_conserved".into(), num_str(rep.verdict_conserved as u64)));
    kv.push(("matrix.batch_count".into(), num_str(rep.batches.len() as u64)));
    kv.push(("matrix.phases".into(), PHASE_NAMES.join("/")));
    for b in rep.batches.iter() {
        kv.push((format!("matrix.batch{}.evt_matched", b.batch), num_str(b.evt_matched)));
        kv.push((format!("matrix.batch{}.phase_matched", b.batch), num_str(b.phase_matched)));
        kv.push((format!("matrix.batch{}.mismatches", b.batch), num_str(b.mismatches)));
    }
    kv.push(("matrix.window_count".into(), num_str(rep.windows.len() as u64)));
    // 守恒闸：total == matched 且窗账一致
    if rep.total != rep.matched && rep.total != 0 {
        return Err(EoiCode::KvExportBad);
    }
    for w in rep.windows.iter() {
        if !w.ok {
            return Err(EoiCode::WindowConserve);
        }
    }
    Ok(kv)
}

/// 裁决账 kv 导出（终态直方图 × 依据账注）。
pub fn verdict_kv(core_tag: &str, vl: &VerdictLedger) -> Result<Vec<Kv>, EoiCode> {
    if !vl.conserved() {
        return Err(EoiCode::KvExportBad);
    }
    let names = [
        Verdict::Completed.as_str(),
        Verdict::Spurious.as_str(),
        Verdict::Rerouted.as_str(),
        Verdict::Dropped.as_str(),
    ];
    let mut kv: Vec<Kv> = Vec::new();
    for i in 0..4 {
        kv.push((format!("{}.verdict.{}", core_tag, names[i]), num_str(vl.hist[i])));
        for j in 0..9 {
            if vl.basis[i][j] > 0 {
                kv.push((
                    format!("{}.basis.{}.{}", core_tag, names[i], RAT_KEYS[j]),
                    num_str(vl.basis[i][j]),
                ));
            }
        }
    }
    Ok(kv)
}

/// 域快照（F0843 全域聚合：矩阵 + MSI 场景 + 守卫 + kv 总账）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct F843Snapshot {
    /// 版本。
    pub version: String,
    /// 诊断段基址。
    pub seg_base: u16,
    /// 段位守卫通过。
    pub seg_ok: bool,
    /// 矩阵闸门通过（J1 主体）。
    pub matrix_ok: bool,
    /// 一致率（基点）。
    pub agree_bp: u64,
    /// 事件总量。
    pub total_events: u64,
    /// MSI 场景闸门通过。
    pub msi_ok: bool,
    /// 双路裁决直方图（msix 路）。
    pub hist: [u64; 4],
    /// 双路异常合计（两路同账）。
    pub anomalies: u64,
    /// 泄漏账配平（两路）。
    pub leak_balanced: bool,
    /// 滚动窗守恒（两路）。
    pub windows_ok: bool,
    /// 裁决账守恒（两路）。
    pub verdict_conserved: bool,
    /// 快照指纹（字段滚动混合）。
    pub fingerprint: u64,
}

impl F843Snapshot {
    /// 判据 J1 总闸：段卫 + 矩阵 100% + MSI 场景 + 守恒全平。
    pub fn gate(&self) -> Result<(), EoiCode> {
        if !self.seg_ok {
            return Err(EoiCode::SegmentGuard);
        }
        if !self.matrix_ok || self.agree_bp != 10_000 || self.total_events != TOTAL_EVENTS {
            return Err(EoiCode::MatrixMismatch);
        }
        if !self.msi_ok {
            return Err(EoiCode::MsiVecLimit);
        }
        if !self.leak_balanced || !self.windows_ok || !self.verdict_conserved {
            return Err(EoiCode::WindowConserve);
        }
        Ok(())
    }

    /// 快照 kv 导出。
    pub fn to_kv(&self) -> Vec<Kv> {
        let mut kv: Vec<Kv> = Vec::new();
        kv.push(("f843.version".into(), self.version.clone()));
        kv.push(("f843.seg_base".into(), format!("0x{:04X}", self.seg_base)));
        kv.push(("f843.seg_ok".into(), num_str(self.seg_ok as u64)));
        kv.push(("f843.matrix_ok".into(), num_str(self.matrix_ok as u64)));
        kv.push(("f843.agree".into(), bp_str(self.agree_bp)));
        kv.push(("f843.total_events".into(), num_str(self.total_events)));
        kv.push(("f843.msi_ok".into(), num_str(self.msi_ok as u64)));
        kv.push(("f843.verdict.completed".into(), num_str(self.hist[0])));
        kv.push(("f843.verdict.spurious".into(), num_str(self.hist[1])));
        kv.push(("f843.verdict.rerouted".into(), num_str(self.hist[2])));
        kv.push(("f843.verdict.dropped".into(), num_str(self.hist[3])));
        kv.push(("f843.anomalies".into(), num_str(self.anomalies)));
        kv.push(("f843.leak_balanced".into(), num_str(self.leak_balanced as u64)));
        kv.push(("f843.windows_ok".into(), num_str(self.windows_ok as u64)));
        kv.push(("f843.verdict_conserved".into(), num_str(self.verdict_conserved as u64)));
        kv.push(("f843.fingerprint".into(), format!("0x{:016X}", self.fingerprint)));
        kv
    }
}

/// 全域跑批：段卫 → 行为矩阵（10 万×双路对拍）→ MSI 场景 → 快照聚合。
pub fn domain_run() -> Result<F843Snapshot, EoiCode> {
    seg_guard()?;
    let run = run_matrix()?;
    run.report.gate()?;
    let msi = run_msi_scenario()?;
    msi.gate()?;
    // 双路裁决账注 kv 落账（守恒不平在此显性报错）
    let _ = verdict_kv("msix", &run.mx.vledger)?;
    let _ = verdict_kv("irq_line", &run.irq.vledger)?;
    let hist = run.mx.vledger.hist;
    let mut fp: u64 = 0x243F_6A88_85A3_08D3;
    for (i, h) in hist.iter().enumerate() {
        fp = fp.wrapping_mul(0x100_0000_01B3).wrapping_add(*h ^ (i as u64));
    }
    fp = fp.wrapping_mul(0x100_0000_01B3).wrapping_add(run.mx.fingerprint as u64);
    fp = fp.wrapping_mul(0x100_0000_01B3).wrapping_add(run.irq.fingerprint as u64);
    Ok(F843Snapshot {
        version: F843_VERSION.into(),
        seg_base: F843_SEG_BASE,
        seg_ok: true,
        matrix_ok: true,
        agree_bp: run.report.agree_bp,
        total_events: run.report.total,
        msi_ok: true,
        hist,
        anomalies: run.mx.anom.total(),
        leak_balanced: run.report.leak_balanced,
        windows_ok: run.report.windows_ok,
        verdict_conserved: run.report.verdict_conserved,
        fingerprint: fp,
    })
}
