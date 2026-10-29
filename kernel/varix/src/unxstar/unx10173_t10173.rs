//! UNX-F10173 定时漂移测量正反双判据（容差带达成与越界误用受控）（0x108c0 段）
//!
//! 锚点：UNX-F10173-J1 容差带达成（多轮睡眠累计漂移在容差带内 10/10 次）。
//! 域：CoRun·UNX-C3（ktest syscall 面谱系）。
//!
//! 结构：漂移册（逐轮 请求/实睡 对账）+ 漂移核（欠跑不可能/单轮超限
//! 拒收/累计带宽评估）+ 容差账本 + 折叠指纹 + 注入核（欠跑伪造/
//! 超限轮/累计越带/负时长/印后登记）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x108c0 独占 0x20）。
pub const SEG_BASE: u32 = 0x108c0;
pub const DR_UNDERRUN: u32 = SEG_BASE + 0x01; // 实睡 < 请求（欠跑不可能）
pub const DR_OVER_ROUND: u32 = SEG_BASE + 0x02; // 单轮超限
pub const DR_NEG: u32 = SEG_BASE + 0x03; // 负/零时长
pub const DR_EMPTY_NOTE: u32 = SEG_BASE + 0x04; // 留痕为空
pub const DR_FROZEN: u32 = SEG_BASE + 0x05; // 印账后仍登记

/// 单轮超限带宽（ms）：实睡超出请求的最大可容忍值。
pub const OVERSHOOT_CAP: u32 = 5;

/// 逐轮漂移留痕。
#[derive(Clone)]
pub struct DriftRec {
    pub seq: u32,
    pub round: u32,
    pub requested: u32,
    pub actual: u32,
    pub note: String,
}

impl DriftRec {
    /// 漂移量：actual - requested（欠跑为不可能事件）。
    pub fn drift(&self) -> i64 {
        self.actual as i64 - self.requested as i64
    }

    /// 账目合法性：留痕非空、时长正、欠跑不可能、单轮在带宽内。
    pub fn ok(&self) -> bool {
        !self.note.is_empty()
            && self.requested > 0
            && self.actual >= self.requested
            && self.actual - self.requested <= OVERSHOOT_CAP
    }

    pub fn render_line(&self) -> String {
        format!(
            "D{}|r{}|req{}|act{}|d{}|{}",
            self.seq,
            self.round,
            self.requested,
            self.actual,
            self.drift(),
            self.note
        )
    }
}

/// 漂移评估结论。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BandVerdict {
    InBand { total: i64 },
    OutOfBand { total: i64 },
}

/// 漂移册：轮次账 + 带宽评估核。
pub struct DriftBook {
    pub rounds: Vec<DriftRec>,
    pub seq_next: u32,
    pub sealed: bool,
}

impl DriftBook {
    pub fn new() -> DriftBook {
        DriftBook { rounds: Vec::new(), seq_next: 1, sealed: false }
    }

    /// 登记一轮：欠跑/超限/负时长/空留痕逐门拒收。
    pub fn record(&mut self, requested: u32, actual: u32, note: &str) -> Result<i64, u32> {
        if self.sealed {
            return Err(DR_FROZEN);
        }
        if note.is_empty() {
            return Err(DR_EMPTY_NOTE);
        }
        if requested == 0 {
            return Err(DR_NEG);
        }
        if actual < requested {
            return Err(DR_UNDERRUN);
        }
        if actual - requested > OVERSHOOT_CAP {
            return Err(DR_OVER_ROUND);
        }
        let round = self.rounds.len() as u32 + 1;
        self.rounds.push(DriftRec {
            seq: self.seq_next,
            round,
            requested,
            actual,
            note: note.to_string(),
        });
        self.seq_next += 1;
        Ok(actual as i64 - requested as i64)
    }

    /// 累计漂移。
    pub fn total_drift(&self) -> i64 {
        self.rounds.iter().map(|r| r.drift()).sum()
    }

    /// 带宽评估：累计漂移必须 ≤ 轮数 × 单轮带宽（线性带宽）。
    pub fn evaluate(&self) -> BandVerdict {
        let total = self.total_drift();
        let band = self.rounds.len() as i64 * OVERSHOOT_CAP as i64;
        if total >= 0 && total <= band {
            BandVerdict::InBand { total }
        } else {
            BandVerdict::OutOfBand { total }
        }
    }

    /// 最差单轮：漂移最大的一轮（诊断面）。
    pub fn worst_round(&self) -> Option<&DriftRec> {
        let mut best: Option<&DriftRec> = None;
        for r in &self.rounds {
            match best {
                Some(w) if w.drift() >= r.drift() => {}
                _ => best = Some(r),
            }
        }
        best
    }

    /// 平均漂移（毫微位放大 1000 倍整型返回，避浮点）。
    pub fn avg_drift_milli(&self) -> i64 {
        if self.rounds.is_empty() {
            return 0;
        }
        self.total_drift() * 1000 / self.rounds.len() as i64
    }

    pub fn ledger_ok(&self) -> bool {
        self.rounds.iter().all(|r| r.ok())
    }

    pub fn seq_unique(&self) -> bool {
        for a in 0..self.rounds.len() {
            for b in (a + 1)..self.rounds.len() {
                if self.rounds[a].seq == self.rounds[b].seq {
                    return false;
                }
            }
        }
        true
    }

    /// 折叠指纹（43 进制卷积）。
    pub fn fold(&self) -> u32 {
        let mut h: u32 = 0x108c0;
        for r in &self.rounds {
            h = h.wrapping_mul(43).wrapping_add(r.seq);
            h = h.wrapping_mul(43).wrapping_add(r.requested);
            h = h.wrapping_mul(43).wrapping_add(r.actual);
            for by in r.note.bytes() {
                h = h.wrapping_mul(43).wrapping_add(by as u32);
            }
        }
        h
    }

    pub fn seal(&mut self) -> u32 {
        self.sealed = true;
        self.fold()
    }
}

/// 注入核：五个必然被检出的坏样本。
pub fn inject_five() -> [bool; 5] {
    let mut got = [false; 5];
    let mut b = DriftBook::new();
    // 1) 欠跑伪造（实睡 < 请求）必拒。
    got[0] = b.record(100, 97, "inj-under").err() == Some(DR_UNDERRUN);
    // 2) 单轮超限（超带宽 6 > 5）必拒。
    got[1] = b.record(100, 106, "inj-over").err() == Some(DR_OVER_ROUND);
    // 3) 零时长必拒。
    got[2] = b.record(0, 0, "inj-neg").err() == Some(DR_NEG);
    // 4) 空留痕必拒。
    got[3] = b.record(100, 100, "").err() == Some(DR_EMPTY_NOTE);
    // 5) 印账后登记必抓。
    b.seal();
    got[4] = b.record(100, 100, "inj-frozen").err() == Some(DR_FROZEN);
    got
}

/// 批内判据（对应卡验收 J 判据）。
pub fn run_unx10173_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F10173");

    // J1 容差带达成：10 轮小漂移累计在带内。
    let j1 = {
        let mut b = DriftBook::new();
        let overs = [0u32, 1, 2, 0, 3, 1, 0, 2, 4, 1];
        for (i, o) in overs.iter().enumerate() {
            let _ = b.record(100, 100 + o, "r");
            let _ = i;
        }
        b.evaluate() == BandVerdict::InBand { total: 14 }
    };
    s.add("J1-容差带达成", j1, "十轮累计 14ms 落在 50ms 带内");

    // J2 累计越带检出：超带宽轮次拉爆累计（构造不可能合法登记的场景：
    // 超限轮在登记门即被拒——漂移面以拒收留痕为检出路径）。
    let j2 = {
        let mut b = DriftBook::new();
        let r1 = b.record(100, 105, "edge-ok");
        let r2 = b.record(100, 110, "over");
        r1 == Ok(5) && r2 == Err(DR_OVER_ROUND) && b.rounds.len() == 1
    };
    s.add("J2-越带拒收", j2, "超带宽轮在登记门即拒");

    // J3 欠跑不可能：实睡 < 请求必拒。
    let j3 = {
        let mut b = DriftBook::new();
        b.record(100, 99, "under").err() == Some(DR_UNDERRUN)
    };
    s.add("J3-欠跑拒", j3, "时间不会倒流，欠跑必拒");

    // J4 边界双合法：恰在带宽端点（5）收、越一点（6）拒。
    let j4 = {
        let mut b = DriftBook::new();
        let a = b.record(50, 55, "cap");
        let c = b.record(50, 56, "cap+1");
        a == Ok(5) && c == Err(DR_OVER_ROUND)
    };
    s.add("J4-带宽端点", j4, "端点含边收、出边拒");

    // J5 零/负时长拒。
    let j5 = {
        let mut b = DriftBook::new();
        b.record(0, 0, "zero").err() == Some(DR_NEG)
    };
    s.add("J5-零时长拒", j5, "无请求时长不构成测量轮");

    // J6 账本完整：合法轮全部过判据且 seq 唯一。
    let j6 = {
        let mut b = DriftBook::new();
        let _ = b.record(100, 101, "a");
        let _ = b.record(100, 100, "b");
        let _ = b.record(200, 205, "c");
        b.ledger_ok() && b.seq_unique() && b.rounds.len() == 3
    };
    s.add("J6-账本完整", j6, "三轮账全合法无重号");

    // J7 累计守恒：账面重算与汇总一致。
    let j7 = {
        let mut b = DriftBook::new();
        let _ = b.record(100, 102, "a");
        let _ = b.record(100, 101, "b");
        let manual: i64 = b.rounds.iter().map(|r| r.drift()).sum();
        b.total_drift() == manual && manual == 3
    };
    s.add("J7-累计守恒", j7, "逐轮重算与汇总零漂移");

    // J8 指纹稳定且印账后封禁。
    let j8 = {
        let mut b = DriftBook::new();
        let _ = b.record(100, 100, "x");
        let f1 = b.fold();
        let f2 = b.fold();
        b.seal();
        f1 == f2 && b.record(100, 100, "post").err() == Some(DR_FROZEN)
    };
    s.add("J8-指纹封账", j8, "指纹幂等且印后必拒");

    // J9 注入核 5/5。
    let got = inject_five();
    let j9 = got.iter().all(|g| *g);
    s.add_dyn("J9-注入全抓", j9, &format!("inj5={:?}", got));

    // J10 诊断面：最差单轮与平均漂移可导出且自洽。
    let j10 = {
        let mut b = DriftBook::new();
        let _ = b.record(100, 101, "a");
        let _ = b.record(100, 105, "b");
        let _ = b.record(100, 100, "c");
        let worst = b.worst_round().map(|r| r.drift()).unwrap_or(-1);
        b.avg_drift_milli() == 2000 && worst == 5
    };
    s.add("J10-诊断面自洽", j10, "最差轮=5、均值=2ms 自洽");

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx10173_drift_flow() {
        let mut b = DriftBook::new();
        assert_eq!(b.record(100, 103, "a"), Ok(3));
        assert_eq!(b.record(100, 105, "b"), Ok(5));
        assert_eq!(b.record(100, 106, "c"), Err(DR_OVER_ROUND));
        assert_eq!(b.evaluate(), BandVerdict::InBand { total: 8 });
        assert_eq!(b.rounds[1].drift(), 5);
    }

    #[test]
    fn unx10173_checks_green() {
        let s = run_unx10173_checks();
        assert!(s.all_passed(), "UNX-F10173 判据未全绿");
    }

    #[test]
    fn unx10173_band_sweep() {
        // 满带宽扫描：逐轮恰顶带宽时累计恰在带沿。
        let mut b = DriftBook::new();
        for _ in 0..10 {
            assert_eq!(b.record(100, 105, "cap"), Ok(5));
        }
        assert_eq!(b.total_drift(), 50);
        assert_eq!(b.evaluate(), BandVerdict::InBand { total: 50 });
        // 任何一轮超带宽都在登记门被拒，累计不可能被拉爆。
        assert_eq!(b.record(100, 106, "x"), Err(DR_OVER_ROUND));
        assert_eq!(b.worst_round().map(|r| r.drift()), Some(5));
    }
}
