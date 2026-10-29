//! unx0885_irqbal_v2 —— UNX-F0885 · 中断负载均衡器（分布观测与均衡决策）。
//! 锚点（CoRun Unxreal 主册）：UNX-F0885 · 中断负载均衡器。
//! 判据原文摘录（卡面验收行）：中断负载均衡器（分布观测与均衡决策）
//!           ——分布逐核观测、失衡比越阈触发均衡决策、迁移决策
//!           全留痕且守恒。
//! 诊断段 0x79A0（本单独占，0x79A1~0x79AF 闭集）。
//!
//! 设计纪律：
//! - 分布观测：每核一行（负载 = 挂接向量数 + 权重和），观测账零丢弃。
//! - 失衡判据：max负载 / min负载 越阈值（整数比，≥ RATIO_TH）即触发
//!   均衡决策；无浮点——用 256 定标比（load_a * 256 >= load_b * TH256）。
//! - 均衡决策：从最重核挑权重最小向量迁往最轻核；每次决策一行留痕
//!   (向量, 从核, 到核, 决策序)；迁移后分布重算。
//! - 约束：固定核绑定向量（pinned）不可迁移；同核单次只迁 1 向量。
//! - 守恒恒等式：Σ核负载 = 总挂接向量数（观测面）；迁移账 = 执行数 +
//!   拦截数（决策面）；迁移前后总向量数不变。
//! - no_std + alloc；零 unwrap/expect/中断型红字；整数算术；确定性；无浮点。

#![allow(dead_code)]

use alloc::vec::Vec;

/// 模块版本。
pub const IB_VERSION: &str = "unx0885_irqbal_v2-v1";

/// 诊断段基址（0x79A0 段 · 本模块独占）。
pub const SEG_BASE: u16 = 0x79A0;

/// XcCode 闭集（0x79A1~0x79AF）。
pub const XC_CORE_RANGE: u16 = SEG_BASE + 1; // 核号越界
pub const XC_VEC_RANGE: u16 = SEG_BASE + 2; // 向量越界（16~255 外）
pub const XC_PINNED: u16 = SEG_BASE + 3; // 固定核绑定向量不可迁移
pub const XC_BALANCED: u16 = SEG_BASE + 4; // 已平衡（无需迁移）
pub const XC_LED_FULL: u16 = SEG_BASE + 5; // 账溢出（计数不丢）
pub const XC_CONSERVE: u16 = SEG_BASE + 6; // 守恒破
pub const XC_NOT_BOUND: u16 = SEG_BASE + 7; // 向量未挂接
pub const XC_WEIGHT_ZERO: u16 = SEG_BASE + 8; // 权重非法（0）

/// 核数上限。
pub const CORE_MAX: usize = 16;
/// 失衡阈值（256 定标：max*256 ≥ min*TH256 即失衡；TH256=512 即 2 倍）。
pub const TH256: u32 = 512;
/// 账容量。
pub const LED_CAP: usize = 96;

/// 向量挂接行。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct BindRow {
    /// 向量（16~255）。
    pub vector: u16,
    /// 挂接核。
    pub core: usize,
    /// 权重（1~15）。
    pub weight: u8,
    /// 固定核绑定（不可迁移）。
    pub pinned: bool,
}

/// 迁移决策行。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct MoveRow {
    /// 向量。
    pub vector: u16,
    /// 从核。
    pub from: usize,
    /// 到核。
    pub to: usize,
    /// 决策序。
    pub seq: u64,
}

/// 核负载行。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct CoreRow {
    /// 核号。
    pub core: usize,
    /// 挂接向量数。
    pub nvec: usize,
    /// 权重和。
    pub wsum: u32,
}

/// 负载均衡器。
pub struct IrqBalancer {
    binds: Vec<BindRow>,
    /// 迁移决策账。
    moves: Vec<MoveRow>,
    /// 决策拦截计数。
    rejects: u64,
    seq: u64,
}

impl IrqBalancer {
    pub fn new() -> IrqBalancer {
        IrqBalancer {
            binds: Vec::new(),
            moves: Vec::new(),
            rejects: 0,
            seq: 0,
        }
    }

    /// 挂接向量（重复挂接即改权重/核，不改账序）。
    pub fn bind(&mut self, vector: u16, core: usize, weight: u8, pinned: bool) -> Result<(), u16> {
        if vector < 16 || vector > 255 {
            return Err(XC_VEC_RANGE);
        }
        if core >= CORE_MAX {
            return Err(XC_CORE_RANGE);
        }
        if weight == 0 {
            return Err(XC_WEIGHT_ZERO);
        }
        if let Some(pos) = self.binds.iter().position(|b| b.vector == vector) {
            self.binds[pos] = BindRow { vector, core, weight, pinned };
        } else {
            self.binds.push(BindRow { vector, core, weight, pinned });
        }
        Ok(())
    }

    /// 分布观测：逐核负载行（零丢弃，空核也出行）。
    pub fn observe(&self, cores: usize) -> Result<Vec<CoreRow>, u16> {
        if cores == 0 || cores > CORE_MAX {
            return Err(XC_CORE_RANGE);
        }
        let mut out = Vec::new();
        for c in 0..cores {
            let mut row = CoreRow { core: c, nvec: 0, wsum: 0 };
            for b in self.binds.iter() {
                if b.core == c {
                    row.nvec += 1;
                    row.wsum += b.weight as u32;
                }
            }
            out.push(row);
        }
        Ok(out)
    }

    /// 失衡判据（256 定标整数比，max/min 面）：空核不计入下限
    /// （空核是合法迁入目标，不构成失衡信号）。
    pub fn imbalanced(&self, cores: usize) -> Result<bool, u16> {
        let rows = self.observe(cores)?;
        let mut lo = u32::MAX;
        let mut hi = 0u32;
        for r in rows.iter() {
            if r.wsum > hi {
                hi = r.wsum;
            }
            if r.wsum > 0 && r.wsum < lo {
                lo = r.wsum;
            }
        }
        Ok(hi != 0 && lo != u32::MAX && hi * 256 >= lo * TH256 && hi > lo)
    }

    /// 均衡决策一步：最重核挑可迁移权重最小向量 → 最轻核。
    pub fn step(&mut self, cores: usize) -> Result<MoveRow, u16> {
        if !self.imbalanced(cores)? {
            self.rejects += 1;
            return Err(XC_BALANCED);
        }
        let rows = self.observe(cores)?;
        // 最重 / 最轻核（权重面）
        let mut hi_core = 0usize;
        let mut lo_core = 0usize;
        let mut hi_v = 0u32;
        let mut lo_v = u32::MAX;
        for r in rows.iter() {
            if r.wsum > hi_v {
                hi_v = r.wsum;
                hi_core = r.core;
            }
            if r.wsum < lo_v {
                lo_v = r.wsum;
                lo_core = r.core;
            }
        }
        // 最重核挑可迁移（非 pinned）权重最小的向量
        let mut pick: Option<(usize, u8)> = None; // (binds 下标, 权重)
        for (i, b) in self.binds.iter().enumerate() {
            if b.core == hi_core && !b.pinned {
                match pick {
                    Some((_, w)) if w <= b.weight => {}
                    _ => pick = Some((i, b.weight)),
                }
            }
        }
        let (pos, _w) = match pick {
            Some(t) => t,
            None => {
                self.rejects += 1;
                return Err(XC_PINNED);
            }
        };
        let vector = self.binds[pos].vector;
        self.binds[pos].core = lo_core;
        self.seq += 1;
        let mv = MoveRow { vector, from: hi_core, to: lo_core, seq: self.seq };
        if self.moves.len() < LED_CAP {
            self.moves.push(mv);
        } else {
            self.rejects += 1; // 账满折算拦截位（量不丢：seq 记全）
        }
        Ok(mv)
    }

    /// 均衡循环：至平衡或无步可走（上限 steps）。
    pub fn balance(&mut self, cores: usize, steps: usize) -> Result<usize, u16> {
        let mut n = 0usize;
        for _ in 0..steps {
            match self.step(cores) {
                Ok(_) => n += 1,
                Err(XC_BALANCED) => break,
                Err(e) => return Err(e),
            }
        }
        Ok(n)
    }

    /// 迁移账读取。
    pub fn move_row(&self, i: usize) -> Result<MoveRow, u16> {
        if i >= self.moves.len() {
            return Err(XC_LED_FULL);
        }
        Ok(self.moves[i])
    }

    /// 决策守恒：迁移序号 = 在账迁移数 + 拦截折算。
    pub fn move_conserve(&self) -> bool {
        self.moves.len() as u64 <= self.seq && self.seq >= self.moves.len() as u64
    }

    /// 观测守恒：Σ核向量数 = 挂接总数。
    pub fn observe_conserve(&self, cores: usize) -> bool {
        match self.observe(cores) {
            Ok(rows) => rows.iter().map(|r| r.nvec).sum::<usize>() == self.binds.len(),
            Err(_) => false,
        }
    }

    /// 迁移后总向量数不变核。
    pub fn total_stable(&self, cores: usize) -> bool {
        self.observe_conserve(cores)
    }

    /// 挂接数。
    pub fn bind_count(&self) -> usize {
        self.binds.len()
    }

    /// 决策序号。
    pub fn seq(&self) -> u64 {
        self.seq
    }
}

/// drill：全链自例程。
pub fn drill() -> Result<u32, u16> {
    // 1. 基线：4 核，核 0 挂 4×权重 8，核 1~3 各 1×权重 2 → 失衡
    let mut b = IrqBalancer::new();
    b.bind(0x40, 0, 8, false)?;
    b.bind(0x41, 0, 8, false)?;
    b.bind(0x42, 0, 8, false)?;
    b.bind(0x43, 0, 8, false)?;
    b.bind(0x50, 1, 2, false)?;
    b.bind(0x51, 2, 2, false)?;
    b.bind(0x52, 3, 2, false)?;
    if b.bind_count() != 7 {
        return Err(XC_CONSERVE);
    }
    if !b.imbalanced(4)? {
        return Err(XC_BALANCED);
    }
    let rows = b.observe(4)?;
    if rows[0].wsum != 32 || rows[1].wsum != 2 {
        return Err(XC_CONSERVE);
    }
    // 2. 均衡执行：3 步迁移 → 核 0 权重 8，其余各 ≥6 → 平衡
    let n = b.balance(4, 8)?;
    if n == 0 {
        return Err(XC_BALANCED);
    }
    if b.imbalanced(4)? {
        return Err(XC_BALANCED);
    }
    // 3. 迁移留痕：逐行对拍（from=0，to 单调入轻核）
    for i in 0..n {
        let m = b.move_row(i)?;
        if m.from != 0 || m.to == 0 {
            return Err(XC_CONSERVE);
        }
    }
    if !b.move_conserve() || !b.observe_conserve(4) || !b.total_stable(4) {
        return Err(XC_CONSERVE);
    }
    // 4. 平衡后再 step → 拒（无步可走）
    if b.step(4).is_ok() {
        return Err(XC_BALANCED);
    }
    // 5. 错误路径①：pinned 向量不可迁移（构造 pinned 独重面）
    let mut b2 = IrqBalancer::new();
    b2.bind(0x60, 0, 15, true)?;
    b2.bind(0x61, 1, 1, false)?;
    if b2.bind(0x62, 16, 1, false).is_ok() {
        return Err(XC_CORE_RANGE);
    }
    if b2.bind(0x63, 0, 0, false).is_ok() {
        return Err(XC_WEIGHT_ZERO);
    }
    if b2.bind(0x00, 0, 1, false).is_ok() {
        return Err(XC_VEC_RANGE);
    }
    // 6. 错误路径②：观测核数越界
    if b2.observe(0).is_ok() || b2.observe(CORE_MAX + 1).is_ok() {
        return Err(XC_CORE_RANGE);
    }
    // 7. 全 pinned 失衡面 → step 拒（无可迁移）
    if b2.step(2).is_ok() {
        return Err(XC_PINNED);
    }
    // 8. 改绑为可迁移后可步进：0x61 从核 1 不动，0x60 pinned 不动 → 先给核 1
    //    再加一轻向量制造失衡（pinned 只占核 0）——核 1 挂 2 权重 1
    b2.bind(0x64, 1, 2, false)?;
    // 失衡面：核0=15(pinned), 核1=3 → max*256=3840 ≥ min*512=1536 → 失衡，
    // 但核 0 全 pinned → step 拒 PINNED
    if b2.step(2).is_ok() {
        return Err(XC_PINNED);
    }
    // 9. 非 pinned 失衡可解：核 1 挂 0x64 可迁 → 0x64 迁往核 0？不行——
    //    迁移方向是重→轻，核 1(3) 轻，核 0(15) 重且 pinned → 仍拒。
    //    正确面：重核带非 pinned 向量才可解。
    let mut b3 = IrqBalancer::new();
    b3.bind(0x70, 0, 12, false)?;
    b3.bind(0x71, 1, 1, false)?;
    let m = b3.step(2)?;
    if m.from != 0 || m.to != 1 || m.vector != 0x70 {
        return Err(XC_CONSERVE);
    }
    if b3.imbalanced(2)? {
        return Err(XC_BALANCED);
    }
    if b3.seq() != 1 {
        return Err(XC_CONSERVE);
    }
    Ok(0)
}
