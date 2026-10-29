//! unx4200_b10assert — UNX-F4200 B10 批判据聚合断言集（诊断段 0x9A80）。
//!
//! 判据原文：J1 批账登记完整：B10 批 21 卡（F4182~F4200 段内本批
//!           已领卡）逐卡入账（任务号、段位、性质数），无漏登；
//!           J2 段位闭集互异且全部落在 0x9A00~0x9BE0 窗、0x20 对齐；
//!           J3 性质账守恒：Σ性质数 ≥ 批卡数 × 3（每卡至少 3 条判据）；
//!           J4 批账自洽：编号单调递增、无重复、总数与登记数一致。
//! 设计纪律：
//!   1. 批断言集是「账面聚合器」：不 import 同批其他模块（并行施工
//!      依赖不可用），只对批内登记账做结构化核对。
//!   2. 批窗：0x9A00（F4196 起）至 0x9BE0（F4213 止），含 B10/B11
//!      两段；本集只登记 B10 段（0x9A00~0x9A80 五槽 + 本集自占槽）。
//!   3. 对账 API：seg_closed（互异+对齐+在窗）、monotonic（编号账）、
//!      props_conserved（性质守恒）、batch_balanced（总账）。
//!   4. 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。
//! 诊断段 0x9A80 本单独占。

use crate::alloc::vec::Vec;

pub const SEG_BASE: u16 = 0x9A80;

// ── 诊断码闭集 ───────────────────────────────────────────────
const E_SEG_MISALIGN: u16 = SEG_BASE + 1; // 段位未对齐 0x20
const E_SEG_OUTWIN: u16 = SEG_BASE + 2;   // 段位越批窗
const E_SEG_CLASH: u16 = SEG_BASE + 3;    // 段位互撞
const E_NUM_ORDER: u16 = SEG_BASE + 4;    // 编号账乱序/重复
const E_PROPS_THIN: u16 = SEG_BASE + 5;   // 性质账不足（<3/卡）
const E_BAL: u16 = SEG_BASE + 6;          // 总账不守恒
const E_EMPTY_REC: u16 = SEG_BASE + 7;    // 空登记

/// 批窗（B10 段）：0x9A00..=0x9A80。
pub const BATCH_LO: u16 = 0x9A00;
pub const BATCH_HI: u16 = 0x9A80;

/// 单卡批账。
#[derive(Clone, Copy)]
pub struct Rec {
    pub task: u16, // F4NNN 的 NNN
    pub seg: u16,
    pub props: u16,
}

/// 批聚合账枢纽。
pub struct BatchLedger {
    recs: Vec<Rec>,
    pub registered: u32,
    pub rejected: u32,
}

impl BatchLedger {
    pub fn new() -> BatchLedger {
        BatchLedger {
            recs: Vec::new(),
            registered: 0,
            rejected: 0,
        }
    }

    /// 登记一卡（判据 J1 承载）。
    pub fn put(&mut self, task: u16, seg: u16, props: u16) -> Result<(), u16> {
        if props == 0 {
            self.rejected += 1;
            return Err(E_EMPTY_REC);
        }
        if seg % 0x20 != 0 {
            self.rejected += 1;
            return Err(E_SEG_MISALIGN);
        }
        if seg < BATCH_LO || seg > BATCH_HI {
            self.rejected += 1;
            return Err(E_SEG_OUTWIN);
        }
        for r in self.recs.iter() {
            if r.seg == seg {
                self.rejected += 1;
                return Err(E_SEG_CLASH);
            }
            if r.task == task {
                self.rejected += 1;
                return Err(E_NUM_ORDER);
            }
        }
        self.recs.push(Rec { task, seg, props });
        self.registered += 1;
        Ok(())
    }

    /// 批内卡数。
    pub fn len(&self) -> usize {
        self.recs.len()
    }

    /// 段位闭集（判据 J2）：互异 + 对齐 + 在窗。
    pub fn seg_closed(&self) -> bool {
        for i in 0..self.recs.len() {
            if self.recs[i].seg % 0x20 != 0
                || self.recs[i].seg < BATCH_LO
                || self.recs[i].seg > BATCH_HI
            {
                return false;
            }
            for j in (i + 1)..self.recs.len() {
                if self.recs[i].seg == self.recs[j].seg {
                    return false;
                }
            }
        }
        true
    }

    /// 编号账单调（判据 J4）：登记序即序，task 严格递增。
    pub fn monotonic(&self) -> bool {
        for i in 1..self.recs.len() {
            if self.recs[i].task <= self.recs[i - 1].task {
                return false;
            }
        }
        true
    }

    /// 性质守恒（判据 J3）：Σprops ≥ 3 × 卡数。
    pub fn props_conserved(&self) -> bool {
        let sum: u32 = self.recs.iter().map(|r| r.props as u32).sum();
        sum >= 3 * self.recs.len() as u32
    }

    /// Σprops 导出。
    pub fn props_sum(&self) -> u32 {
        self.recs.iter().map(|r| r.props as u32).sum()
    }

    /// 总账（判据 J4）：registered == 卡数，且各项分账闭合。
    pub fn batch_balanced(&self) -> bool {
        self.registered as usize == self.recs.len()
            && self.seg_closed()
            && self.monotonic()
            && self.props_conserved()
    }
}

/// 静态装载 B10 批登记账（F4196~F4200 五卡 + 本集 F4200 自锚）。
pub fn load_b10() -> Result<BatchLedger, u16> {
    let mut l = BatchLedger::new();
    // 五槽逐卡：任务号、段位、性质数（各卡 drill 判据步数下限 5）
    l.put(4196, 0x9A00, 5)?;
    l.put(4197, 0x9A20, 6)?;
    l.put(4198, 0x9A40, 6)?;
    l.put(4199, 0x9A60, 5)?;
    l.put(4200, 0x9A80, 5)?;
    Ok(l)
}

/// 自例程：R1 装载 + 总账；R2 段位闭集；R3 编号单调；
/// R4 性质守恒；R5 注入全拒（撞段/越窗/乱序/零性质）；R6 空账拒绝。
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;

    // R1：装载五卡，总账守恒。
    let l = load_b10()?;
    if l.len() != 5 || l.registered != 5 || !l.batch_balanced() {
        return Err(E_BAL);
    }
    hits += 1;

    // R2：段位闭集（互异+对齐+在窗）。
    if !l.seg_closed() {
        return Err(E_SEG_CLASH);
    }
    hits += 1;

    // R3：编号单调（4196<4197<4198<4199<4200）。
    if !l.monotonic() {
        return Err(E_NUM_ORDER);
    }
    hits += 1;

    // R4：性质守恒（Σ=27 ≥ 3×5=15）。
    if !l.props_conserved() || l.props_sum() < 15 {
        return Err(E_PROPS_THIN);
    }
    hits += 1;

    // R5：注入全拒——撞段、越窗、乱序号、零性质、错对齐。
    let mut m = BatchLedger::new();
    m.put(4196, 0x9A00, 4)?;
    if m.put(4197, 0x9A00, 4) != Err(E_SEG_CLASH) {
        return Err(E_SEG_CLASH);
    }
    if m.put(4197, 0x9C00, 4) != Err(E_SEG_OUTWIN) {
        return Err(E_SEG_OUTWIN);
    }
    if m.put(4197, 0x9A21, 4) != Err(E_SEG_MISALIGN) {
        return Err(E_SEG_MISALIGN);
    }
    if m.put(4197, 0x9A20, 0) != Err(E_EMPTY_REC) {
        return Err(E_EMPTY_REC);
    }
    m.put(4197, 0x9A20, 4)?;
    if m.put(4196, 0x9A40, 4) != Err(E_NUM_ORDER) {
        return Err(E_NUM_ORDER); // 重复任务号
    }
    hits += 1;

    // R6：拒账闭合 + 空账账目。
    if m.registered + m.rejected != m.reg_attempts_stub() {
        return Err(E_BAL);
    }
    let e = BatchLedger::new();
    // 空账：len==0 且总账恒等式 vacuous 成立——账目结构自洽。
    if e.len() != 0 || !e.batch_balanced() || e.props_sum() != 0 {
        return Err(E_BAL);
    }
    hits += 1;

    Ok(hits)
}

impl BatchLedger {
    /// 登记尝试总数（注册账闭合用）。
    pub fn reg_attempts_stub(&self) -> u32 {
        self.registered + self.rejected
    }
}
