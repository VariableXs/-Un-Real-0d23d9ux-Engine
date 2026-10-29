//! unx48213_xcosign — UNX-F48213 Xwayland AI-27 联签执行账（XIM 转换边界登记）（诊断段 0xBB80）。
//!
//! 判据原文：
//!   J1 联签登记入册；判据锚定 §AI-61 §8。
//!   J2 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计纪律：
//!   1. 转换边界定义：Xwayland 合成器侧 AI-27 输入管线与 XIM 协议面的交界
//!      即转换边界；焦点切换、预编辑交接、候选刷新三类 IM 状态交接过界时
//!      必须双签，单签不得入册。
//!   2. 双签结构：出侧签由 AI-27 按 (boundary, seq) 以确定性整数混合函数
//!      出签；入侧签由 XIM 侧按同 (boundary, seq) 独立回签；两签逐位相等
//!      方可落入登记账。
//!   3. 登记账守恒：账条 (seq, boundary, sig, round) 逐条留痕，登记总量
//!      =双签成功次数；未签、单签、错签一律拒登且不落半账。
//!   4. 防伪三查：seq 边界内单调递增（防重放）、boundary 预先入册（防
//!      野签）、双签逐位相等（防篡改）；任一查失败即拒并带诊断码返回。
//!   5. 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。

use crate::alloc::vec::Vec;

pub const SEG_BASE: u16 = 0xBB80;

// ── 诊断码（SEG_BASE+1 起，≤0x1F 个）──────────────────────
const E_REPLAY: u16 = SEG_BASE + 1; // seq 重放
const E_WILD_BND: u16 = SEG_BASE + 2; // 边界未入册（野签）
const E_SIG_MISMATCH: u16 = SEG_BASE + 3; // 双签不等（篡改）
const E_NO_PENDING: u16 = SEG_BASE + 4; // 无出侧签可回签（单签）
const E_REG_BAD: u16 = SEG_BASE + 5; // 登记账守恒破
const E_STATE_BAD: u16 = SEG_BASE + 6; // 执行器状态非法
const E_EMPTY_SUM: u16 = SEG_BASE + 7; // 写回摘要为空

/// 边界执行器状态：Blank 空闲待签 → Issued 已有出侧签挂账 → Registered 本轮入册完毕。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CosignState {
    Blank,
    Issued,
    Registered,
}

/// 登记账一条：(seq, boundary, sig, round) 四元组逐位留痕。
#[derive(Clone, Copy, Debug)]
pub struct CosignEntry {
    pub seq: u64,
    pub boundary: u16,
    pub sig: u64,
    pub round: u32,
}

/// 出侧已签、待入侧回签的暂挂条。
#[derive(Clone, Copy, Debug)]
pub struct PendingSig {
    pub seq: u64,
    pub boundary: u16,
    pub sig: u64,
}

/// 出侧签名：确定性 splitmix 风格整数混合，同 (boundary, seq) 恒得同签。
pub fn sign_of(boundary: u16, seq: u64) -> u64 {
    let mut z: u64 = ((boundary as u64) << 48) ^ seq ^ 0x9E37_79B9_7F4A_7C15;
    z = z.wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z ^= z >> 29;
    z = z.wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^= z >> 32;
    z
}

/// AI-27 ↔ XIM 转换边界联签执行枢纽。
pub struct CosignExec {
    pub state: CosignState,
    bounds: Vec<u16>, // 已入册边界表（防野签）
    pending: Vec<PendingSig>,
    pub ledger: Vec<CosignEntry>,
    pub issued_total: u64,    // 出侧出签总次数
    pub registered_total: u32, // 双签成功入册条数
    pub rejected_total: u32,  // 回签被拒次数（错签/篡改）
    pub last_seq: u64,        // 边界内 seq 单调高位
}

impl CosignExec {
    pub fn new() -> CosignExec {
        CosignExec {
            state: CosignState::Blank,
            bounds: Vec::new(),
            pending: Vec::new(),
            ledger: Vec::new(),
            issued_total: 0,
            registered_total: 0,
            rejected_total: 0,
            last_seq: 0,
        }
    }

    /// 边界入册：转换边界须先登记身份，后续联签方可在其上执行。
    pub fn enroll_boundary(&mut self, b: u16) -> Result<(), u16> {
        if self.is_boundary(b) {
            return Err(E_REG_BAD); // 重复入册视为登记账违规
        }
        self.bounds.push(b);
        Ok(())
    }

    pub fn is_boundary(&self, b: u16) -> bool {
        let mut i = 0usize;
        while i < self.bounds.len() {
            if self.bounds[i] == b {
                return true;
            }
            i += 1;
        }
        false
    }

    pub fn boundary_count(&self) -> usize {
        self.bounds.len()
    }

    /// 出侧出签（AI-27 侧）：边界在册 + seq 单调，两查过后出签挂暂挂条。
    pub fn sign_out(&mut self, b: u16, seq: u64) -> Result<u64, u16> {
        if !self.is_boundary(b) {
            return Err(E_WILD_BND);
        }
        if seq <= self.last_seq {
            return Err(E_REPLAY);
        }
        let sig = sign_of(b, seq);
        self.pending.push(PendingSig { seq, boundary: b, sig });
        self.issued_total += 1;
        self.last_seq = seq;
        self.state = CosignState::Issued;
        Ok(sig)
    }

    /// 入侧回签（XIM 侧）：暂挂条须在册且双签逐位相等；相等即登记入册，
    /// 不等即拒（篡改嫌疑），暂挂条撤下不落半账。
    pub fn sign_in(&mut self, b: u16, seq: u64, sig_back: u64) -> Result<(), u16> {
        let mut found = None;
        let mut i = 0usize;
        while i < self.pending.len() {
            if self.pending[i].boundary == b && self.pending[i].seq == seq {
                found = Some(i);
                break;
            }
            i += 1;
        }
        match found {
            None => Err(E_NO_PENDING),
            Some(idx) => {
                let p = self.pending[idx];
                // 撤下暂挂条（无论成败，不留半账）。
                let mut kept: Vec<PendingSig> = Vec::new();
                let mut m = 0usize;
                while m < self.pending.len() {
                    if m != idx {
                        kept.push(self.pending[m]);
                    }
                    m += 1;
                }
                self.pending = kept;
                if p.sig != sig_back {
                    self.rejected_total += 1;
                    return Err(E_SIG_MISMATCH);
                }
                self.ledger.push(CosignEntry {
                    seq: p.seq,
                    boundary: p.boundary,
                    sig: p.sig,
                    round: self.registered_total,
                });
                self.registered_total += 1;
                self.state = CosignState::Registered;
                Ok(())
            }
        }
    }

    pub fn pending_count(&self) -> usize {
        self.pending.len()
    }

    /// 登记账守恒审计：账条数=入册总量、seq 全程严格单调、边界闭集、
    /// 签名重算一致（四重账实相符）。
    pub fn audit(&self) -> Result<(), u16> {
        if self.ledger.len() as u32 != self.registered_total {
            return Err(E_REG_BAD);
        }
        let mut prev: u64 = 0;
        let mut i = 0usize;
        while i < self.ledger.len() {
            let e = self.ledger[i];
            if !self.is_boundary(e.boundary) {
                return Err(E_REG_BAD);
            }
            if e.seq <= prev && i > 0 {
                return Err(E_REPLAY);
            }
            if e.sig != sign_of(e.boundary, e.seq) {
                return Err(E_SIG_MISMATCH);
            }
            prev = e.seq;
            i += 1;
        }
        if self.state != CosignState::Blank
            && self.state != CosignState::Issued
            && self.state != CosignState::Registered
        {
            return Err(E_STATE_BAD);
        }
        Ok(())
    }

    /// 联签覆盖核验：入册量须达 want。
    pub fn coverage(&self, want: u32) -> Result<(), u16> {
        if self.registered_total < want {
            return Err(E_REG_BAD);
        }
        Ok(())
    }

    /// 完成写回（判据 J2）：本轮入册态 + 非空摘要方放行。
    pub fn writeback(&self, done: bool, summary_len: u32) -> Result<(), u16> {
        if !done || self.state != CosignState::Registered {
            return Err(E_STATE_BAD);
        }
        if summary_len == 0 {
            return Err(E_EMPTY_SUM);
        }
        Ok(())
    }
}

/// 自例程判据：逐条落实 J1（联签登记入册），返回命中数。
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;

    // R1 主对齐：3 条转换边界入册，64 次状态交接轮转双签，全部登记入册；
    //    审计四重账实相符（账条数、seq 单调、边界闭集、签名重算）。
    let mut ce = CosignExec::new();
    let mut b = 0u16;
    while b < 3 {
        ce.enroll_boundary(0x2710 + b)?;
        b += 1;
    }
    if ce.boundary_count() != 3 {
        return Err(E_REG_BAD);
    }
    let mut seq = 1u64;
    while seq <= 64 {
        let bd = 0x2710 + ((seq % 3) as u16);
        let sig = ce.sign_out(bd, seq)?;
        ce.sign_in(bd, seq, sig)?;
        seq += 1;
    }
    ce.coverage(64)?;
    if ce.registered_total != 64 || ce.issued_total != 64 {
        return Err(E_REG_BAD);
    }
    if ce.pending_count() != 0 {
        return Err(E_REG_BAD); // 双签完毕暂挂条必须清空
    }
    ce.audit()?;
    hits += 1;

    // R2 双签相等才入册：回签被篡改（sig+1）必拒（E_SIG_MISMATCH），
    //    拒登不入账、暂挂条撤下、入册量不动；重新出签再回签可入册。
    let mut ce = CosignExec::new();
    ce.enroll_boundary(0x2720)?;
    let sig = ce.sign_out(0x2720, 101)?;
    if ce.sign_in(0x2720, 101, sig ^ 1).is_ok() {
        return Err(E_SIG_MISMATCH);
    }
    if ce.rejected_total != 1 || ce.registered_total != 0 || ce.pending_count() != 0 {
        return Err(E_REG_BAD);
    }
    let sig2 = ce.sign_out(0x2720, 102)?;
    ce.sign_in(0x2720, 102, sig2)?;
    if ce.registered_total != 1 {
        return Err(E_REG_BAD);
    }
    ce.audit()?;
    hits += 1;

    // R3 重放拒收：同 seq 二次出签必拒（E_REPLAY），不新增暂挂条。
    let mut ce = CosignExec::new();
    ce.enroll_boundary(0x2730)?;
    ce.sign_out(0x2730, 1)?;
    ce.sign_out(0x2730, 2)?;
    if ce.sign_out(0x2730, 1).is_ok() {
        return Err(E_REPLAY);
    }
    if ce.pending_count() != 2 || ce.issued_total != 2 {
        return Err(E_REG_BAD);
    }
    hits += 1;

    // R4 野签拒收：边界未入册直接出签必拒（E_WILD_BND），零出签。
    let mut ce = CosignExec::new();
    if ce.sign_out(0x9999, 1).is_ok() {
        return Err(E_WILD_BND);
    }
    if ce.issued_total != 0 {
        return Err(E_REG_BAD);
    }
    hits += 1;

    // R5 单签不得入册：只出签不回签 → 暂挂挂着、入册量恒 0；
    //    补上回签后立即入册（J1 主体：入册动作由双签齐触发）。
    let mut ce = CosignExec::new();
    ce.enroll_boundary(0x2740)?;
    let sig = ce.sign_out(0x2740, 7)?;
    if ce.registered_total != 0 {
        return Err(E_REG_BAD);
    }
    if ce.sign_in(0x2740, 8, sig).is_ok() {
        return Err(E_NO_PENDING); // seq=8 无出侧签，单边回签必拒
    }
    ce.sign_in(0x2740, 7, sig)?;
    if ce.registered_total != 1 || ce.ledger[0].seq != 7 {
        return Err(E_REG_BAD);
    }
    ce.audit()?;
    hits += 1;

    // R6 入册留痕：账条四元组逐条可溯（seq/boundary/sig/round 与执行轨迹一致）。
    let mut ce = CosignExec::new();
    ce.enroll_boundary(0x2750)?;
    ce.enroll_boundary(0x2751)?;
    let mut seq = 1u64;
    while seq <= 8 {
        let bd = if seq % 2 == 0 { 0x2750 } else { 0x2751 };
        let sig = ce.sign_out(bd, seq)?;
        ce.sign_in(bd, seq, sig)?;
        seq += 1;
    }
    let mut i = 0usize;
    while i < ce.ledger.len() {
        let e = ce.ledger[i];
        let want_bd = if e.seq % 2 == 0 { 0x2750 } else { 0x2751 };
        if e.boundary != want_bd || e.sig != sign_of(want_bd, e.seq) || e.round != i as u32 {
            return Err(E_REG_BAD);
        }
        i += 1;
    }
    ce.audit()?;
    hits += 1;

    // R7 写回（判据 J2）：未入册不写回、空摘要拒写回、入册+非空摘要放行。
    let mut ce = CosignExec::new();
    if ce.writeback(true, 0x10).is_ok() {
        return Err(E_STATE_BAD); // Blank 态不得写回
    }
    ce.enroll_boundary(0x2760)?;
    let sig = ce.sign_out(0x2760, 1)?;
    ce.sign_in(0x2760, 1, sig)?;
    if ce.writeback(true, 0).is_ok() {
        return Err(E_EMPTY_SUM);
    }
    ce.writeback(true, 0x20)?;
    hits += 1;

    Ok(hits)
}
