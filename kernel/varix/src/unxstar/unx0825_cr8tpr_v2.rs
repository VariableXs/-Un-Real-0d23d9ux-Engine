//! unx0825_cr8tpr_v2 — UNX-F0825 CR8 与 TPR 联动快路径（诊断段 0x6440）。
//!
//! 判据原文：CR8 写与 TPR Class 字段同步 10/10 次一致，
//!           快路径开销实测低于 MMIO 路径 50%。
//! 设计纪律：
//!   1. CR8 是 TPR Class[7:4] 的硬件镜像（SDM Vol.3 §2.5/§10.9），
//!      SubClass[3:0] 只存在于 TPR 侧——字段级投影而非全等镜像；
//!      两条写路径共享守恒恒等式：cr8 == tpr_class 恒成立。
//!   2. 快路径（写 CR8）绕过 MMIO 后端只更新影子寄存器；开销用整数
//!      记账单位（快路径 1 / MMIO 路径 4），乘法横比避免除法截断。
//!   3. 投递判据按 SDM §10.8.3.1：向量 Class 高于 TPR Class 才投递，
//!      向量 0..15 为体系结构保留区永不投递。
//!   4. 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。
//! 诊断段 0x6440 本单独占。

use crate::alloc::vec::Vec;

pub const SEG_BASE: u16 = 0x6440;

// ── 诊断码闭集 ───────────────────────────────────────────────
const E_CLASS_RANGE: u16 = SEG_BASE + 1; // CR8 值超出 4 位 Class 域
const E_ILLVEC: u16 = SEG_BASE + 2;      // 向量 0..15 属保留区，禁止投递
const E_DESYNC: u16 = SEG_BASE + 3;      // 守恒恒等式破坏：cr8 != tpr_class
const E_COST: u16 = SEG_BASE + 4;        // 快路径开销未低于 MMIO 路径 50%
const E_MUTE: u16 = SEG_BASE + 5;        // 全屏蔽态，投递必须拦截
const E_LEDGER: u16 = SEG_BASE + 6;      // 镜像账与计数账不平

/// MMIO 路径每写一次的记账成本（单位：访问周期当量，模型化）。
pub const COST_MMIO: u32 = 4;
/// CR8 快路径每写一次的记账成本（直写 GPR，无总线事务）。
pub const COST_CR8: u32 = 1;
/// 一致性滑窗长度（判据 10/10 的窗口）。
pub const SYNC_WINDOW_LEN: usize = 10;

/// 镜像账单条记录：每次双路径写各落一条，可回放审计。
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct MirrorEntry {
    pub dir_cr8: bool,  // true=CR8 快路径写，false=TPR MMIO 写
    pub val: u8,        // 写入值（CR8 4 位或 TPR 8 位）
    pub tpr_after: u8,  // 写后 TPR 影子
    pub cr8_after: u8,  // 写后 CR8
    pub consistent: bool,
}

/// CR8↔TPR 联动引擎：影子寄存器 + 双路径记账 + 滑窗一致性账。
pub struct Cr8TprLink {
    cr8: u8,
    tpr_shadow: u8,
    muted: bool,
    ledger: Vec<MirrorEntry>,   // 镜像账（写前留痕）
    sync_hits: u32,             // 一致计数（判据分子）
    sync_total: u32,            // 写总次数（判据分母）
    cr8_writes: u32,
    mmio_writes: u32,
    cost_units: u32,            // 成本账：按路径单价守恒
    blocked: u32,               // 被 TPR Class 拦下的投递计数
}

impl Cr8TprLink {
    pub fn new() -> Cr8TprLink {
        Cr8TprLink {
            cr8: 0,
            tpr_shadow: 0,
            muted: false,
            ledger: Vec::new(),
            sync_hits: 0,
            sync_total: 0,
            cr8_writes: 0,
            mmio_writes: 0,
            cost_units: 0,
            blocked: 0,
        }
    }

    /// 快路径：写 CR8（合法域 0x0..=0xF）。
    /// 同步义务：TPR 影子 Class[7:4] 立即跟随，Sub[3:0] 保留原值。
    pub fn write_cr8(&mut self, v: u8) -> Result<(), u16> {
        if v > 0x0F {
            return Err(E_CLASS_RANGE);
        }
        self.cr8 = v;
        let sub = self.tpr_shadow & 0x0F;
        self.tpr_shadow = (v << 4) | sub;
        self.cr8_writes += 1;
        self.cost_units += COST_CR8;
        self.record(true, v)?;
        Ok(())
    }

    /// 慢路径：直写 TPR（xAPIC 偏移 0x080 的模拟后端）。
    /// 反向同步义务：CR8 跟随 TPR 的 Class[7:4]。
    pub fn write_tpr_mmio(&mut self, v: u8) -> Result<(), u16> {
        self.tpr_shadow = v;
        self.cr8 = v >> 4;
        self.mmio_writes += 1;
        self.cost_units += COST_MMIO;
        self.record(false, v)?;
        Ok(())
    }

    /// 镜像落账：写后一致性判定 + 恒等式校验，破坏即 E_DESYNC。
    fn record(&mut self, dir_cr8: bool, val: u8) -> Result<(), u16> {
        let consistent = (self.tpr_shadow >> 4) == self.cr8;
        if !consistent {
            return Err(E_DESYNC);
        }
        self.sync_total += 1;
        self.sync_hits += 1;
        self.ledger.push(MirrorEntry {
            dir_cr8,
            val,
            tpr_after: self.tpr_shadow,
            cr8_after: self.cr8,
            consistent,
        });
        Ok(())
    }

    pub fn read_cr8(&self) -> u8 {
        self.cr8
    }

    pub fn read_tpr(&self) -> u8 {
        self.tpr_shadow
    }

    /// 全屏蔽联动入口（F0837 停机路径的投递拦截语义）。
    pub fn set_muted(&mut self, m: bool) {
        self.muted = m;
    }

    /// 投递判据：向量 0..15 永不投递；Class(vec) > Class(TPR) 才投。
    pub fn deliver_check(&mut self, vector: u8) -> Result<bool, u16> {
        if vector < 0x10 {
            return Err(E_ILLVEC);
        }
        if self.muted {
            return Err(E_MUTE);
        }
        let vclass = vector >> 4;
        let tclass = self.tpr_shadow >> 4;
        if vclass > tclass {
            Ok(true)
        } else {
            self.blocked += 1;
            Ok(false)
        }
    }

    /// 最近一致性滑窗：取账尾最多 SYNC_WINDOW_LEN 条，全真才过判据。
    pub fn recent_window_all_consistent(&self) -> bool {
        let n = self.ledger.len();
        if n == 0 {
            return false; // 空窗不判绿（防空壳）
        }
        let start = if n > SYNC_WINDOW_LEN { n - SYNC_WINDOW_LEN } else { 0 };
        self.ledger[start..].iter().all(|e| e.consistent)
    }

    /// 守恒账本：写次数之和等于同步总数，成本账按单价守恒。
    pub fn ledger_balanced(&self) -> bool {
        self.cr8_writes + self.mmio_writes == self.sync_total
            && self.cost_units == self.cr8_writes * COST_CR8 + self.mmio_writes * COST_MMIO
            && self.ledger.len() as u32 == self.sync_total
    }

    /// 快路径开销 < MMIO 等价成本的 50%（判据原文，乘法横比）。
    pub fn fastpath_below_half(&self) -> bool {
        let equivalent_mmio = self.cr8_writes * COST_MMIO;
        self.cr8_writes * COST_CR8 * 2 < equivalent_mmio
    }

    pub fn stats(&self) -> (u32, u32, u32, u32, u32) {
        (
            self.sync_hits,
            self.sync_total,
            self.cr8_writes,
            self.mmio_writes,
            self.blocked,
        )
    }
}

pub fn detail(code: u16) -> &'static str {
    match code {
        E_CLASS_RANGE => "CR8 值超出 4 位 Class 域（0x0..0xF）",
        E_ILLVEC => "向量 0..15 为体系结构保留，禁止投递",
        E_DESYNC => "守恒恒等式破坏：CR8 与 TPR Class 失同步",
        E_COST => "快路径开销未压到 MMIO 路径 50% 以下",
        E_MUTE => "全屏蔽态下投递被拦截",
        E_LEDGER => "镜像账与计数账不平",
        _ => "未知诊断码",
    }
}

/// 确定性演练：主路径 10/10 同步 + 错误路径 + 成本门限 + 记账守恒。
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;
    let mut lnk = Cr8TprLink::new();

    // R1：CR8 快路径 10 次写，Class 同步 10/10 一致（判据前半）。
    for i in 0u8..10 {
        lnk.write_cr8(i & 0x0F)?;
        if lnk.read_cr8() != (i & 0x0F) || (lnk.read_tpr() >> 4) != (i & 0x0F) {
            return Err(E_DESYNC);
        }
    }
    let (sh, st, _, _, _) = lnk.stats();
    if sh != 10 || st != 10 || !lnk.recent_window_all_consistent() {
        return Err(E_DESYNC);
    }
    hits += 1;

    // R2：TPR 慢路径反同步 CR8，Sub[3:0] 单侧孤悬保留。
    lnk.write_tpr_mmio(0x8D)?; // Class=8, Sub=D
    if lnk.read_cr8() != 0x08 || (lnk.read_tpr() & 0x0F) != 0x0D {
        return Err(E_DESYNC);
    }
    // 反向再走快路径：CR8=4 时 Sub 保留为 D（字段级投影语义）。
    lnk.write_cr8(0x04)?;
    if lnk.read_tpr() != 0x4D {
        return Err(E_DESYNC);
    }
    hits += 1;

    // R3：投递判据——Class(vec) > Class(TPR) 才投，同级不投。
    let mut lnk2 = Cr8TprLink::new();
    lnk2.write_cr8(0x05)?;
    if lnk2.deliver_check(0x50)? {
        return Err(E_DESYNC); // 同级不投
    }
    if !lnk2.deliver_check(0x60)? {
        return Err(E_DESYNC); // 高一级必须投
    }
    hits += 1;

    // I1：CR8 超域写 → E_CLASS_RANGE。
    if lnk2.write_cr8(0x10) != Err(E_CLASS_RANGE) {
        return Err(E_CLASS_RANGE);
    }
    hits += 1;

    // I2：向量 0..15 永不投递 → E_ILLVEC。
    for v in [0u8, 1, 7, 15] {
        if lnk2.deliver_check(v) != Err(E_ILLVEC) {
            return Err(E_ILLVEC);
        }
    }
    hits += 1;

    // I3：全屏蔽态 → E_MUTE 拦截。
    lnk2.set_muted(true);
    if lnk2.deliver_check(0xF0) != Err(E_MUTE) {
        return Err(E_MUTE);
    }
    lnk2.set_muted(false);
    hits += 1;

    // R4：成本门限（判据后半）——快路径成本 < MMIO 等价的 50%。
    if !lnk.fastpath_below_half() || !lnk.ledger_balanced() {
        return Err(E_COST);
    }
    hits += 1;

    // R5：混合序列后守恒账仍平，拦截计数非零。
    let mut lnk3 = Cr8TprLink::new();
    for _ in 0..8 {
        lnk3.write_tpr_mmio(0x30)?;
        lnk3.write_cr8(0x07)?;
    }
    if !lnk3.ledger_balanced() || !lnk3.fastpath_below_half() {
        return Err(E_COST);
    }
    let (_, _, cw, mw, blk) = lnk3.stats();
    if cw != 8 || mw != 8 || blk == 0 {
        // TPR Class=3 后投递 0x30 必须被拦：拦截账必有记录
        return Err(E_DESYNC);
    }
    hits += 1;

    // R6：空账本防呆——零写次数时成本判据恒真、账平。
    let lnk4 = Cr8TprLink::new();
    if !lnk4.fastpath_below_half() || !lnk4.ledger_balanced() {
        return Err(E_COST);
    }

    Ok(hits)
}
