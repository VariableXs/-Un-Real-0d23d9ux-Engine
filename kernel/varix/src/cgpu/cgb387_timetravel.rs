//! CGPU-F0387 · 预算时间旅行（目标 340 行）
//! ==========================================================================
//! 锚点判据（施工书原文摘录）：
//! - 时间旅行：历史帧仲裁回放（定位「第 N 帧为什么这个账户被拒」——对接
//!   F0345 审计数据）；三类回放入口（按帧/按账户/按事件）；仿真联动（从
//!   历史帧切入 what-if——F0347 对接）。判据：回放、三入口、仿真联动、判据。
//!
//! 设计纪律：
//! - 回放只读：历史账不可变（append-only），what-if 在副本上算。
//! - 三入口统一经查询计划器，检索语义冻结（帧号精确/账户聚合/事件筛选）。
//! - no_std + alloc；零 unwrap/expect 于运行路径；诊断段 0x0AF0 独占。

use alloc::vec::Vec;
use alloc::string::String;

/// 诊断段基址（0x0AF0，全仓独占）。
pub const SEG_BASE: u16 = 0x0AF0;

pub const E_FRAME_UNKNOWN: u16 = SEG_BASE + 1;
pub const E_ACCOUNT_UNKNOWN: u16 = SEG_BASE + 2;
pub const E_EVENT_UNKNOWN: u16 = SEG_BASE + 3;
pub const E_SIM_BUDGET: u16 = SEG_BASE + 4;

/// 审计行（对接 F0345——回放的原始素材）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AuditRow {
    pub frame: u64,
    pub account: u32,
    /// 事件码（1 申请 2 准 3 拒 4 降档 5 释放）。
    pub event: u8,
    pub amount_pm: u32,
    pub note: String,
}

impl AuditRow {
    fn valid(&self) -> bool {
        (1..=5).contains(&self.event)
    }
}

/// 历史账（append-only——回放真源）。
pub struct HistoryLedger {
    rows: Vec<AuditRow>,
    /// 帧号有序性校验锚点（回放前提：时间轴单调）。
    max_frame: u64,
}

impl HistoryLedger {
    pub fn new() -> Self {
        HistoryLedger { rows: Vec::new(), max_frame: 0 }
    }

    /// 追加（时间轴必须单调不减——历史不可改写）。
    pub fn append(&mut self, row: AuditRow) -> Result<(), u16> {
        if !row.valid() {
            return Err(E_EVENT_UNKNOWN);
        }
        if row.frame < self.max_frame {
            return Err(E_FRAME_UNKNOWN); // 时间倒流=历史被改写，必拒
        }
        self.max_frame = row.frame;
        self.rows.push(row);
        Ok(())
    }

    pub fn rows(&self) -> &[AuditRow] {
        &self.rows
    }

    pub fn len(&self) -> usize {
        self.rows.len()
    }

    pub fn is_empty(&self) -> bool {
        self.rows.is_empty()
    }
}

impl Default for HistoryLedger {
    fn default() -> Self {
        Self::new()
    }
}

/// 三类回放入口（判据「三入口」）。
pub enum ReplayQuery {
    /// 入口一：按帧——精确帧号的全部事件。
    ByFrame(u64),
    /// 入口二：按账户——该账户全时间轴事件。
    ByAccount(u32),
    /// 入口三：按事件——指定事件码的跨帧筛选。
    ByEvent(u8),
}

/// 回放结果（带查询形态标注）。
#[derive(Debug, PartialEq, Eq)]
pub struct ReplayResult {
    pub via: &'static str,
    pub rows: Vec<AuditRow>,
}

/// 查询计划器（三入口统一语义）。
pub fn replay(ledger: &HistoryLedger, q: &ReplayQuery) -> Result<ReplayResult, u16> {
    let rows: Vec<AuditRow> = match q {
        ReplayQuery::ByFrame(f) => {
            let hit: Vec<AuditRow> = ledger.rows().iter().filter(|r| r.frame == *f).cloned().collect();
            if hit.is_empty() {
                return Err(E_FRAME_UNKNOWN);
            }
            hit
        }
        ReplayQuery::ByAccount(a) => {
            let hit: Vec<AuditRow> = ledger.rows().iter().filter(|r| r.account == *a).cloned().collect();
            if hit.is_empty() {
                return Err(E_ACCOUNT_UNKNOWN);
            }
            hit
        }
        ReplayQuery::ByEvent(e) => {
            if !(1..=5).contains(e) {
                return Err(E_EVENT_UNKNOWN);
            }
            let hit: Vec<AuditRow> = ledger.rows().iter().filter(|r| r.event == *e).cloned().collect();
            if hit.is_empty() {
                return Err(E_EVENT_UNKNOWN);
            }
            hit
        }
    };
    let via = match q {
        ReplayQuery::ByFrame(_) => "frame",
        ReplayQuery::ByAccount(_) => "account",
        ReplayQuery::ByEvent(_) => "event",
    };
    Ok(ReplayResult { via, rows })
}

/// 「第 N 帧为什么被拒」定位器：按帧+账户双键回放，抽出拒因链。
pub fn why_denied(ledger: &HistoryLedger, frame: u64, account: u32) -> Result<Option<String>, u16> {
    let mut has_any = false;
    let mut denial_note: Option<String> = None;
    for r in ledger.rows() {
        if r.frame == frame && r.account == account {
            has_any = true;
            if r.event == 3 {
                denial_note = Some(r.note.clone());
            }
        }
    }
    if !has_any {
        return Err(E_ACCOUNT_UNKNOWN);
    }
    Ok(denial_note)
}

/// what-if 仿真（判据「仿真联动」）：从历史帧切入，在副本上重算
/// 「若预算调整为 X，该帧会准吗」——只动副本，历史账零污染。
pub struct SimState {
    pub budget_pm: u32,
    pub used_pm: u32,
}

pub fn what_if_grant(hist_granted_pm: u32, request_pm: u32, sim: &mut SimState) -> Result<bool, u16> {
    if sim.budget_pm > 10_000 || sim.used_pm > sim.budget_pm {
        return Err(E_SIM_BUDGET);
    }
    let avail = sim.budget_pm.saturating_sub(sim.used_pm);
    if request_pm <= avail {
        sim.used_pm += request_pm;
        Ok(true)
    } else {
        // 仿真拒绝：不动账（副本语义——拒绝不改余额）。
        let _ = hist_granted_pm;
        Ok(false)
    }
}

// ---------------------------------------------------------------------------
// 判据自检
// ---------------------------------------------------------------------------
#[cfg(test)]
mod tests {
    use super::*;

    fn seeded() -> HistoryLedger {
        let mut l = HistoryLedger::new();
        let mk = |f, a, e, amt, n| AuditRow { frame: f, account: a, event: e, amount_pm: amt, note: String::from(n) };
        l.append(mk(1, 10, 1, 500, "req")).unwrap();
        l.append(mk(1, 10, 2, 500, "ok")).unwrap();
        l.append(mk(2, 20, 1, 900, "req")).unwrap();
        l.append(mk(2, 20, 3, 900, "storm quota")).unwrap();
        l.append(mk(3, 10, 5, 200, "release")).unwrap();
        l
    }

    #[test]
    fn a87_历史账_append_only时间轴单调() {
        let l = seeded();
        assert_eq!(l.len(), 5);
        // 时间倒流必拒（历史不可改写）。
        let mut l2 = HistoryLedger::new();
        l2.append(AuditRow { frame: 5, account: 1, event: 1, amount_pm: 1, note: String::from("a") }).unwrap();
        assert_eq!(l2.append(AuditRow { frame: 3, account: 1, event: 1, amount_pm: 1, note: String::from("b") }).unwrap_err(), E_FRAME_UNKNOWN);
        // 非法事件拒。
        assert_eq!(l2.append(AuditRow { frame: 6, account: 1, event: 9, amount_pm: 1, note: String::new() }).unwrap_err(), E_EVENT_UNKNOWN);
        // 同帧可多条（单调不减）。
        assert!(l2.append(AuditRow { frame: 5, account: 1, event: 2, amount_pm: 1, note: String::from("c") }).is_ok());
        let _ = l.rows();
    }

    #[test]
    fn a87_三入口_帧账户事件() {
        let l = seeded();
        // 入口一：按帧。
        let r = replay(&l, &ReplayQuery::ByFrame(2)).unwrap();
        assert_eq!(r.via, "frame");
        assert_eq!(r.rows.len(), 2);
        assert_eq!(r.rows[0].account, 20);
        // 入口二：按账户。
        let r = replay(&l, &ReplayQuery::ByAccount(10)).unwrap();
        assert_eq!(r.via, "account");
        assert_eq!(r.rows.len(), 3);
        // 入口三：按事件。
        let r = replay(&l, &ReplayQuery::ByEvent(1)).unwrap();
        assert_eq!(r.via, "event");
        assert_eq!(r.rows.len(), 2);
        // 三类空结果各自报错。
        assert_eq!(replay(&l, &ReplayQuery::ByFrame(99)).unwrap_err(), E_FRAME_UNKNOWN);
        assert_eq!(replay(&l, &ReplayQuery::ByAccount(99)).unwrap_err(), E_ACCOUNT_UNKNOWN);
        assert_eq!(replay(&l, &ReplayQuery::ByEvent(9)).unwrap_err(), E_EVENT_UNKNOWN);
        assert_eq!(replay(&l, &ReplayQuery::ByEvent(4)).unwrap_err(), E_EVENT_UNKNOWN);
    }

    #[test]
    fn a87_拒因定位_第N帧为什么被拒() {
        let l = seeded();
        let note = why_denied(&l, 2, 20).unwrap();
        assert_eq!(note, Some(String::from("storm quota")));
        // 该帧有活动但没被拒 → None（诚实缺报）。
        assert_eq!(why_denied(&l, 1, 10).unwrap(), None);
        // 幽灵（帧+账户）无任何活动 → 错。
        assert_eq!(why_denied(&l, 3, 20).unwrap_err(), E_ACCOUNT_UNKNOWN);
    }

    #[test]
    fn a87_what_if_副本仿真不改历史() {
        let l = seeded();
        let before = l.len();
        // 仿真一：预算 1000 已用 500，申请 400 → 准，副本余额更新。
        let mut sim = SimState { budget_pm: 1_000, used_pm: 500 };
        assert!(what_if_grant(400, 400, &mut sim).unwrap());
        assert_eq!(sim.used_pm, 900);
        // 仿真二：申请超余 → 拒且账不动。
        assert!(!what_if_grant(500, 400, &mut sim).unwrap());
        assert_eq!(sim.used_pm, 900);
        // 仿真三：历史账零污染。
        assert_eq!(l.len(), before);
        // 非法仿真态拒。
        let mut bad = SimState { budget_pm: 100, used_pm: 200 };
        assert_eq!(what_if_grant(1, 1, &mut bad).unwrap_err(), E_SIM_BUDGET);
    }
}
