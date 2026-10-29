//! unx48267_upsla — UNX-F48267 fcitx5 上游跟随 SLA 登记（重大变更 7 日内评估，AI-94 台账）（诊断段 0xC240）。
//!
//! 判据原文：
//!   J1 SLA 登记入 AI-94 台账留痕；判据锚定 §AI-61 §2 专题四、§8 治理线。
//!   J2 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计纪律：
//!   1. 台账三账分立：上游事件账（AI-94 主账）、评估账（SLA 闭环）、
//!      违例账（超期告警）；三账互查闭合，不许单账孤证。
//!   2. SLA 口径：重大变更（kind=major）自上报日起 7 日内必须完成评估；
//!      评估日-上报日 > 7 即超期，超期必须落违例账留痕（不掩盖、不回改）。
//!   3. 留痕不可变：台账只追加不改写——事件 seq 一经入账不可重用
//!      （E_SEQ_DUP），评估记录落账后不可撤销；审计以账对账。
//!   4. 闭环口径：每条 major 事件必有评估记录（未评即 E_EVAL_MISS），
//!      每条评估必有事件对应（孤评即 E_LEDGER_BAD）；minor/patch 只
//!      登记不强评（治理线只锁重大变更）。
//!   5. 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。

use crate::alloc::vec::Vec;

pub const SEG_BASE: u16 = 0xC240;

// ── 诊断码（SEG_BASE+1 起，≤0x1F 个）──────────────────────
const E_SLA_OVERDUE: u16 = SEG_BASE + 1; // 重大变更评估超期
const E_SEQ_DUP: u16 = SEG_BASE + 2; // 事件 seq 重用
const E_KIND_BAD: u16 = SEG_BASE + 3; // 变更级别越界
const E_LEDGER_BAD: u16 = SEG_BASE + 4; // 台账互查不闭合
const E_EVAL_MISS: u16 = SEG_BASE + 5; // 重大变更未评估
const E_STATE_BAD: u16 = SEG_BASE + 6; // 台账状态非法
const E_EMPTY_SUM: u16 = SEG_BASE + 7; // 写回摘要为空

/// SLA 口径：重大变更 7 日内评估。
pub const SLA_DAYS: u32 = 7;

/// 变更级别闭集。
pub const KIND_MAJOR: u8 = 0;
pub const KIND_MINOR: u8 = 1;
pub const KIND_PATCH: u8 = 2;

/// AI-94 台账事件行。
#[derive(Clone, Copy, Debug)]
pub struct UpEvent {
    pub seq: u64,
    pub kind: u8,
    pub day: u32,
    pub digest: u64, // 上游变更摘要（确定性内容指纹）
}

/// 评估记录。
#[derive(Clone, Copy, Debug)]
pub struct EvalRow {
    pub seq: u64,
    pub eval_day: u32,
    pub sla_days: u32,
    pub overdue: bool,
}

/// 违例告警行（超期留痕，只追加）。
#[derive(Clone, Copy, Debug)]
pub struct ViolationRow {
    pub seq: u64,
    pub eval_day: u32,
    pub over_by: u32, // 超期天数
}

/// AI-94 台账。
pub struct SlaLedger {
    pub events: Vec<UpEvent>,
    pub evals: Vec<EvalRow>,
    pub violations: Vec<ViolationRow>,
    pub last_seq: u64,
}

impl SlaLedger {
    pub fn new() -> SlaLedger {
        SlaLedger { events: Vec::new(), evals: Vec::new(), violations: Vec::new(), last_seq: 0 }
    }

    /// 事件登记入账（AI-94 主账留痕）：seq 单调不重用、级别闭集。
    pub fn register(&mut self, seq: u64, kind: u8, day: u32, digest: u64) -> Result<(), u16> {
        if kind > KIND_PATCH {
            return Err(E_KIND_BAD);
        }
        if seq <= self.last_seq {
            return Err(E_SEQ_DUP);
        }
        self.events.push(UpEvent { seq, kind, day, digest });
        self.last_seq = seq;
        Ok(())
    }

    /// 评估落账：major 事件按 7 日口径判定，超期即落违例账（留痕不掩盖），
    /// 评估记录照常闭合（overdue 标记随身）。
    pub fn evaluate(&mut self, seq: u64, eval_day: u32) -> Result<u32, u16> {
        // 事件必须在册。
        let mut found: Option<UpEvent> = None;
        let mut i = 0usize;
        while i < self.events.len() {
            if self.events[i].seq == seq {
                found = Some(self.events[i]);
                break;
            }
            i += 1;
        }
        let ev = match found {
            Some(e) => e,
            None => return Err(E_LEDGER_BAD), // 孤评（无对应事件）
        };
        // 重复评估拒收（留痕不可变）。
        i = 0;
        while i < self.evals.len() {
            if self.evals[i].seq == seq {
                return Err(E_STATE_BAD);
            }
            i += 1;
        }
        if eval_day < ev.day {
            return Err(E_STATE_BAD); // 评估早于上报，时序倒流
        }
        let sla = eval_day - ev.day;
        let overdue = ev.kind == KIND_MAJOR && sla > SLA_DAYS;
        if overdue {
            self.violations.push(ViolationRow { seq, eval_day, over_by: sla - SLA_DAYS });
        }
        self.evals.push(EvalRow { seq, eval_day, sla_days: sla, overdue });
        Ok(sla)
    }

    /// 台账互查审计（纪律 4）：三账闭合——每条 major 有评估、每条评估
    /// 有事件、违例账与评估账 overdue 标记逐条互证。
    pub fn audit(&self) -> Result<(), u16> {
        // 每条评估必有事件。
        let mut i = 0usize;
        while i < self.evals.len() {
            let e = self.evals[i];
            let mut hit = false;
            let mut j = 0usize;
            while j < self.events.len() {
                if self.events[j].seq == e.seq {
                    hit = true;
                    break;
                }
                j += 1;
            }
            if !hit {
                return Err(E_LEDGER_BAD);
            }
            i += 1;
        }
        // 每条 major 必有评估（未评即缺）。
        i = 0;
        while i < self.events.len() {
            if self.events[i].kind == KIND_MAJOR {
                let mut hit = false;
                let mut j = 0usize;
                while j < self.evals.len() {
                    if self.evals[j].seq == self.events[i].seq {
                        hit = true;
                        break;
                    }
                    j += 1;
                }
                if !hit {
                    return Err(E_EVAL_MISS);
                }
            }
            i += 1;
        }
        // 违例账与评估账互证：条数一致、seq 集一致、超期量一致。
        if self.violations.len() != self.overdue_count() as usize {
            return Err(E_LEDGER_BAD);
        }
        i = 0;
        while i < self.violations.len() {
            let v = self.violations[i];
            let mut hit = false;
            let mut j = 0usize;
            while j < self.evals.len() {
                if self.evals[j].seq == v.seq && self.evals[j].overdue {
                    if self.evals[j].sla_days - SLA_DAYS != v.over_by {
                        return Err(E_LEDGER_BAD);
                    }
                    hit = true;
                    break;
                }
                j += 1;
            }
            if !hit {
                return Err(E_LEDGER_BAD);
            }
            i += 1;
        }
        Ok(())
    }

    pub fn overdue_count(&self) -> u32 {
        let mut n = 0u32;
        let mut i = 0usize;
        while i < self.evals.len() {
            if self.evals[i].overdue {
                n += 1;
            }
            i += 1;
        }
        n
    }

    pub fn event_count(&self) -> usize {
        self.events.len()
    }
    pub fn eval_count(&self) -> usize {
        self.evals.len()
    }

    /// 完成写回（判据 J2）：台账有痕 + 非空摘要方放行。
    pub fn writeback(&self, done: bool, summary_len: u32) -> Result<(), u16> {
        if !done || self.event_count() == 0 {
            return Err(E_STATE_BAD);
        }
        if summary_len == 0 {
            return Err(E_EMPTY_SUM);
        }
        Ok(())
    }
}

/// 自例程判据：逐条落实 J1（SLA 登记入 AI-94 台账留痕），返回命中数。
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;

    // R1 主留痕闭环：5 条上游事件（3 major + 1 minor + 1 patch）入账；
    //    3 条 major 分别 +3/+7/+2 日完成评估（SLA 内），audit 三账闭合。
    let mut lg = SlaLedger::new();
    lg.register(1, KIND_MAJOR, 100, 0xA1)?;
    lg.register(2, KIND_MAJOR, 110, 0xA2)?;
    lg.register(3, KIND_MAJOR, 120, 0xA3)?;
    lg.register(4, KIND_MINOR, 125, 0xA4)?;
    lg.register(5, KIND_PATCH, 130, 0xA5)?;
    if lg.event_count() != 5 {
        return Err(E_LEDGER_BAD);
    }
    lg.evaluate(1, 103)?; // 3 日
    lg.evaluate(2, 117)?; // 7 日（恰在口径线上，不超期）
    lg.evaluate(3, 122)?; // 2 日
    if lg.eval_count() != 3 {
        return Err(E_LEDGER_BAD);
    }
    if lg.overdue_count() != 0 {
        return Err(E_SLA_OVERDUE);
    }
    lg.audit()?;
    hits += 1;

    // R2 SLA 口径边界：major 恰 7 日不违、8 日即违——违例落账留痕
    //    （over_by=1），台账依旧闭合（留痕不掩盖）。
    let mut lg2 = SlaLedger::new();
    lg2.register(1, KIND_MAJOR, 200, 0xB1)?;
    if lg2.evaluate(1, 207)? != 7 {
        return Err(E_SLA_OVERDUE);
    }
    lg2.register(2, KIND_MAJOR, 300, 0xB2)?;
    if lg2.evaluate(2, 308).is_ok() {
        return Err(E_SLA_OVERDUE); // 8 日必须报违
    }
    if lg2.overdue_count() != 1 {
        return Err(E_LEDGER_BAD);
    }
    if lg2.violations[0].over_by != 1 {
        return Err(E_LEDGER_BAD);
    }
    lg2.audit()?;
    hits += 1;

    // R3 留痕不可变：seq 重用拒（E_SEQ_DUP）、重复评估拒（E_STATE_BAD）、
    //    评估早于上报拒（时序倒流）。
    if lg2.register(2, KIND_MINOR, 310, 0xB3).is_ok() {
        return Err(E_SEQ_DUP);
    }
    if lg2.evaluate(1, 210).is_ok() {
        return Err(E_STATE_BAD);
    }
    lg2.register(3, KIND_MAJOR, 400, 0xB4)?;
    if lg2.evaluate(3, 399).is_ok() {
        return Err(E_STATE_BAD);
    }
    hits += 1;

    // R4 治理线只锁重大：minor/patch 不评估亦过审（非 major 无强评口径）。
    let mut lg3 = SlaLedger::new();
    lg3.register(1, KIND_MINOR, 1, 0xC1)?;
    lg3.register(2, KIND_PATCH, 2, 0xC2)?;
    lg3.audit()?; // 未评估不缺
    lg3.register(3, KIND_MAJOR, 3, 0xC3)?;
    if lg3.audit().is_ok() {
        return Err(E_EVAL_MISS); // major 未评必报缺
    }
    hits += 1;

    // R5 孤评拒收：评估不存在的 seq → E_LEDGER_BAD。
    if lg3.evaluate(99, 10).is_ok() {
        return Err(E_LEDGER_BAD);
    }
    hits += 1;

    // R6 级别闭集：kind=3 → E_KIND_BAD，不入账。
    let mut lg4 = SlaLedger::new();
    if lg4.register(1, 3, 1, 0xD1).is_ok() {
        return Err(E_KIND_BAD);
    }
    if lg4.event_count() != 0 {
        return Err(E_LEDGER_BAD);
    }
    hits += 1;

    // R7 违例账互证负例：人工塞一条无评估对应的违例行 → audit 必报
    //    E_LEDGER_BAD（违例必须能溯到评估记录）。
    let mut lg5 = SlaLedger::new();
    lg5.register(1, KIND_MAJOR, 1, 0xE1)?;
    lg5.evaluate(1, 20)?; // 19 日超期，违例 +1
    lg5.violations.push(ViolationRow { seq: 42, eval_day: 50, over_by: 1 });
    if lg5.audit().is_ok() {
        return Err(E_LEDGER_BAD);
    }
    hits += 1;

    // R8 写回（判据 J2）：空台账不写回、空摘要拒、有痕+非空摘要放行。
    let mut lg6 = SlaLedger::new();
    if lg6.writeback(true, 0x10).is_ok() {
        return Err(E_STATE_BAD);
    }
    lg6.register(1, KIND_MAJOR, 1, 0xF1)?;
    lg6.evaluate(1, 5)?;
    if lg6.writeback(true, 0).is_ok() {
        return Err(E_EMPTY_SUM);
    }
    lg6.writeback(true, 0x20)?;
    hits += 1;

    Ok(hits)
}
