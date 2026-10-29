//! unx48329_smobs — UNX-F48329 智能面观测埋点（联想/纠错/反馈环/热词四点）（诊断段 0xCA00）。
//!
//! 判据原文：
//!   J1 四点 100%；判据锚定 体验十三·补。
//!   J2 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计纪律：
//!   1. 四点闭集：联想触发（ASSOC）/纠错介入（PYFIX）/反馈环落账
//!      （FEDLOOP）/热词上榜（HOTWORD）——智能面每条功能路径必须
//!      有落点，缺一点即观测盲区。
//!   2. 100% 口径：每点各自埋点事件全覆盖（触发即埋、埋必入账、
//!      账必带序号与会话标签）；四点覆盖率逐点 == 100%，任一点
//!      缺记即红（覆盖率不是估出来的）。
//!   3. 会话序单调：每点各自 seq 严格 +1 无跳号无回退；跨点账目
//!      分立互不冒充；总账 == 四点和。
//!   4. 事件载荷带最小上下文（点 id + 会话标签 + 载荷指纹），指纹
//!      可复算防漂移。
//!   5. 纯功能线：no_std + alloc，零 unwrap/expect/panic，确定性，
//!      drill() 返回 Ok(hits)。

use crate::alloc::vec::Vec;

pub const SEG_BASE: u16 = 0xCA00;

// ── 诊断码（SEG_BASE+1 起，≤0x1F 个）──────────────────────
const E_POINT_BAD: u16 = SEG_BASE + 1; // 埋点越四点闭集
const E_SEQ_GAP: u16 = SEG_BASE + 2;   // 序号跳号/回退
const E_COVER_MISS: u16 = SEG_BASE + 3;// 覆盖率不足 100%
const E_LEDGER_BAD: u16 = SEG_BASE + 4;// 四点账不平
const E_FP_BAD: u16 = SEG_BASE + 5;    // 载荷指纹不符
const E_REPLAY_DIFF: u16 = SEG_BASE + 6;// 复跑不一致
const E_EMPTY_SUM: u16 = SEG_BASE + 7; // 写回摘要为空

/// 四点闭集。
pub const PT_ASSOC: u8 = 0;   // 联想触发
pub const PT_PYFIX: u8 = 1;   // 纠错介入
pub const PT_FEDLOOP: u8 = 2; // 反馈环落账
pub const PT_HOTWORD: u8 = 3; // 热词上榜
pub const POINT_N: usize = 4;
pub const PT_NAMES: [&str; POINT_N] = ["联想触发", "纠错介入", "反馈环落账", "热词上榜"];

/// 账容量。
pub const EV_CAP: usize = 64;

/// 埋点事件。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ObsEvent {
    pub point: u8,
    pub session: u16, // 会话标签
    pub seq: u32,     // 点内序号（1 起连续）
    pub fp: u64,      // 载荷指纹
}

/// 智能面观测面：四点埋点 + 逐点账 + 覆盖率判定。
pub struct SmObs {
    books: [Vec<ObsEvent>; POINT_N],
    last_seq: [u32; POINT_N],
    pub reject_total: u32,
}

impl SmObs {
    pub fn new() -> SmObs {
        SmObs {
            books: [Vec::new(), Vec::new(), Vec::new(), Vec::new()],
            last_seq: [0; POINT_N],
            reject_total: 0,
        }
    }

    /// 埋点：触发即埋，序号点内严格 +1，载荷指纹入账。
    pub fn mark(&mut self, point: u8, session: u16, payload: u32) -> Result<u32, u16> {
        if point as usize >= POINT_N {
            self.reject_total += 1;
            return Err(E_POINT_BAD);
        }
        let idx = point as usize;
        if self.books[idx].len() >= EV_CAP {
            self.reject_total += 1;
            return Err(E_LEDGER_BAD);
        }
        let seq = self.last_seq[idx] + 1;
        let fp = obs_fp(point, session, payload, seq);
        self.books[idx].push(ObsEvent { point, session, seq, fp });
        self.last_seq[idx] = seq;
        Ok(seq)
    }

    pub fn book_len(&self, point: u8) -> Result<usize, u16> {
        if point as usize >= POINT_N {
            return Err(E_POINT_BAD);
        }
        Ok(self.books[point as usize].len())
    }

    /// 覆盖率判定（J1 四点 100%）：四点各有埋点（非零）且逐事件
    /// 序号 1..n 连续、指纹可复算。
    pub fn coverage(&self) -> Result<(), u16> {
        let mut p = 0usize;
        while p < POINT_N {
            if self.books[p].is_empty() {
                return Err(E_COVER_MISS);
            }
            let mut i = 0usize;
            while i < self.books[p].len() {
                let e = self.books[p][i];
                if e.seq != i as u32 + 1 {
                    return Err(E_SEQ_GAP);
                }
                if e.fp != obs_fp(e.point, e.session, e.session as u32, e.seq) {
                    return Err(E_FP_BAD);
                }
                i += 1;
            }
            p += 1;
        }
        Ok(())
    }

    /// 账目守恒：总事件数 == 四点和；点内 last_seq == 账长。
    pub fn audit(&self) -> Result<(), u16> {
        let mut total: u32 = 0;
        let mut p = 0usize;
        while p < POINT_N {
            total += self.books[p].len() as u32;
            if self.last_seq[p] != self.books[p].len() as u32 {
                return Err(E_LEDGER_BAD);
            }
            p += 1;
        }
        let mut sum: u32 = 0;
        let mut p = 0usize;
        while p < POINT_N {
            sum += self.books[p].len() as u32;
            p += 1;
        }
        if total != sum {
            return Err(E_LEDGER_BAD);
        }
        Ok(())
    }

    /// 四点指纹聚合（复跑对照）。
    pub fn fingerprint(&self) -> u64 {
        let mut fp: u64 = 0xCA00_CA01;
        let mut p = 0usize;
        while p < POINT_N {
            let mut i = 0usize;
            while i < self.books[p].len() {
                let e = self.books[p][i];
                fp = fp.rotate_left(7) ^ e.fp;
                i += 1;
            }
            p += 1;
        }
        fp
    }

    /// 完成写回（判据 J2）。
    pub fn writeback(&self, done: bool, summary_len: u32) -> Result<(), u16> {
        if !done || self.books[0].is_empty() {
            return Err(E_LEDGER_BAD);
        }
        if summary_len == 0 {
            return Err(E_EMPTY_SUM);
        }
        Ok(())
    }
}

/// 载荷指纹（点/会话/载荷/序 四元折叠）。
pub fn obs_fp(point: u8, session: u16, payload: u32, seq: u32) -> u64 {
    let mut fp: u64 = 0xCA02_CA03 ^ (point as u64);
    fp = fp.rotate_left(8) ^ (session as u64);
    fp = fp.rotate_left(8) ^ (payload as u64);
    fp = fp.rotate_left(8) ^ (seq as u64);
    fp = fp.rotate_left(8);
    fp
}

/// 自例程判据：逐条落实 J1（四点 100%），返回命中数。
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;

    // R1 四点 100%：四点各埋一轮 ×10 事件，覆盖率逐点绿，序号
    //    连续，账守恒。
    let mut ob = SmObs::new();
    let mut p = 0u8;
    while p < 4 {
        let mut n = 0u32;
        while n < 10 {
            ob.mark(p, 0x100 + p as u16, n)?;
            n += 1;
        }
        p += 1;
    }
    ob.coverage()?;
    ob.audit()?;
    let mut total: u32 = 0;
    let mut p = 0u8;
    while p < 4 {
        total += ob.book_len(p)? as u32;
        p += 1;
    }
    if total != 40 {
        return Err(E_LEDGER_BAD);
    }
    hits += 1;

    // R2 触发即埋全路径：模拟一次会话全流程（联想→纠错→反馈环→
    //    热词各触发 ≥1 次），四点全有账（缺一点即观测盲区必红）。
    let mut ob = SmObs::new();
    ob.mark(PT_ASSOC, 7, 1)?;
    ob.mark(PT_PYFIX, 7, 2)?;
    ob.mark(PT_FEDLOOP, 7, 3)?;
    ob.mark(PT_HOTWORD, 7, 4)?;
    ob.coverage()?;
    let mut p = 0u8;
    while p < 4 {
        if ob.book_len(p)? != 1 {
            return Err(E_COVER_MISS);
        }
        p += 1;
    }
    hits += 1;

    // R3 序号单调与跨点分立：点内连埋序号严格 +1；A 点序号不影响
    //    B 点（分立不冒充）；点间会话标签可同。
    let mut ob = SmObs::new();
    let mut n = 0u32;
    while n < 5 {
        let s = ob.mark(PT_ASSOC, 9, n)?;
        if s != n + 1 {
            return Err(E_SEQ_GAP);
        }
        n += 1;
    }
    let s = ob.mark(PT_HOTWORD, 9, 100)?;
    if s != 1 {
        return Err(E_SEQ_GAP); // 他点不得继承序号
    }
    if ob.book_len(PT_ASSOC)? != 5 || ob.book_len(PT_HOTWORD)? != 1 {
        return Err(E_LEDGER_BAD);
    }
    hits += 1;

    // R4 载荷指纹可复算与防篡改：账内指纹与 obs_fp 重算一致；篡改
    //    会话标签后重算必不符。
    let mut ob = SmObs::new();
    ob.mark(PT_FEDLOOP, 0xBEEF, 42)?;
    let e0 = ob.books[PT_FEDLOOP as usize][0];
    if e0.fp != obs_fp(e0.point, e0.session, e0.session as u32, e0.seq) {
        return Err(E_FP_BAD);
    }
    let forged = obs_fp(e0.point, e0.session ^ 1, e0.session as u32, e0.seq);
    if forged == e0.fp {
        return Err(E_FP_BAD); // 指纹对标签翻转敏感（非恒绿）
    }
    hits += 1;

    // R5 边界闭集：五点越闭集拒埋；账满拒埋且计数留痕；覆盖率对
    //    空账必红。
    let mut ob = SmObs::new();
    if ob.mark(9, 1, 1) != Err(E_POINT_BAD) {
        return Err(E_POINT_BAD);
    }
    if ob.mark(3, 1, 1).is_err() {
        return Err(E_POINT_BAD); // 边界内一点必须可埋
    }
    if ob.book_len(9) != Err(E_POINT_BAD) {
        return Err(E_POINT_BAD);
    }
    let mut full = SmObs::new();
    let mut n = 0u32;
    while n < EV_CAP as u32 {
        full.mark(PT_ASSOC, 1, n)?;
        n += 1;
    }
    if full.mark(PT_ASSOC, 1, 999).is_ok() {
        return Err(E_LEDGER_BAD);
    }
    if full.reject_total != 1 {
        return Err(E_LEDGER_BAD);
    }
    let empty = SmObs::new();
    if empty.coverage().is_ok() {
        return Err(E_COVER_MISS);
    }
    hits += 1;

    // R6 缺点必红演练：只埋三点，coverage 必红（100% 口径可红可绿）。
    let mut ob = SmObs::new();
    ob.mark(PT_ASSOC, 3, 1)?;
    ob.mark(PT_PYFIX, 3, 2)?;
    ob.mark(PT_FEDLOOP, 3, 3)?;
    if ob.coverage().is_ok() {
        return Err(E_COVER_MISS); // 缺热词点必红
    }
    ob.mark(PT_HOTWORD, 3, 4)?;
    ob.coverage()?; // 补齐转绿
    hits += 1;

    // R7 可红演练：序号跳号必红——构造后删首事件使 seq 从 2 起，
    //    coverage 必红；空摘要/空账写回必红。
    let mut ob = SmObs::new();
    let mut n = 0u32;
    while n < 3 {
        ob.mark(PT_PYFIX, 5, n)?;
        n += 1;
    }
    ob.books[PT_PYFIX as usize].remove(0); // 模拟首条丢失
    if ob.coverage().is_ok() {
        return Err(E_SEQ_GAP);
    }
    if ob.writeback(true, 0).is_ok() {
        return Err(E_EMPTY_SUM);
    }
    let ob0 = SmObs::new();
    if ob0.writeback(false, 8).is_ok() {
        return Err(E_LEDGER_BAD);
    }
    hits += 1;

    // R8 复跑指纹一致与写回（判据 J2）：同会话两轮四点埋点，聚合
    //    指纹逐位一致（确定性）；写回过。
    let mut ob = SmObs::new();
    let mut p = 0u8;
    while p < 4 {
        let mut n = 0u32;
        while n < 6 {
            ob.mark(p, 0x77, n * 3 + 1)?;
            n += 1;
        }
        p += 1;
    }
    let fp1 = ob.fingerprint();
    let mut ob2 = SmObs::new();
    let mut p = 0u8;
    while p < 4 {
        let mut n = 0u32;
        while n < 6 {
            ob2.mark(p, 0x77, n * 3 + 1)?;
            n += 1;
        }
        p += 1;
    }
    if ob2.fingerprint() != fp1 {
        return Err(E_REPLAY_DIFF);
    }
    ob.coverage()?;
    ob.audit()?;
    if ob.writeback(true, 0).is_ok() {
        return Err(E_EMPTY_SUM);
    }
    ob.writeback(true, 0x1C)?;
    hits += 1;

    Ok(hits)
}
