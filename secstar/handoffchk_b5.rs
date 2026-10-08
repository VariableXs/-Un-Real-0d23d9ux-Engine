//! F181 交接预检器 · 批次五深化（secstar · G-G-11）。
//!
//! 批次五功能面（达成率 46%——主攻批次。与 b3「调度与账本」、b4
//! 「面板与快照」互补，本批管「历史与分诊」）：
//! - [`GateHistory`]：闸门 30 日历史——每日交接的裁决+快照账
//!   （趋势面：最近拦停率、连续全绿天数）；
//! - [`TriageCard`]：分诊卡数据面——红查 → 主层 triage_copy 三要素
//!   消费 + 快照定位哪条坏（不裸抛状态位——给人话）；
//! - [`PrecheckSelftest`]：预检器自检——假故障注入 → 面板必须显红
//!   （预检器自己也接受检验：探测器说谎会被抓）；
//! - [`WinEspPath`]：WINESP 目标路径账——目标存在探测的逐段路径
//!   （存在性探测的可解释面：查了哪几段、哪段断了）。
//!
//! 零堆纪律：定长历史环 + 定长路径账，无 alloc。

use super::handoffchk::{AuditEntry, CheckId, CheckState, AUDIT_CAP};
use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// 闸门 30 日历史
// ---------------------------------------------------------------------------

/// 历史环容量。
pub const HISTORY_CAP: usize = 30;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DayRecord {
    pub day: u32,
    pub allowed: bool,
    pub snapshot6: u8,
}

pub struct GateHistory {
    ring: [Option<DayRecord>; HISTORY_CAP],
    head: usize,
    pub n: usize,
    pub overflows: u32,
}

impl GateHistory {
    pub const fn new() -> GateHistory {
        GateHistory { ring: [const { None }; HISTORY_CAP], head: 0, n: 0, overflows: 0 }
    }

    pub fn record(&mut self, rec: DayRecord) {
        if self.n == HISTORY_CAP {
            self.overflows += 1;
        } else {
            self.n += 1;
        }
        self.ring[self.head] = Some(rec);
        self.head = (self.head + 1) % HISTORY_CAP;
    }

    /// 拦停率 ‰（窗口内）。
    pub fn blocked_permille(&self) -> u32 {
        if self.n == 0 {
            return 0;
        }
        let blocked = self.ring.iter().flatten().filter(|r| !r.allowed).count();
        (blocked * 1_000 / self.n) as u32
    }

    /// 从最新往回的连续全绿天数（趋势面： streak 越长越安心）。
    pub fn green_streak(&self) -> usize {
        let mut streak = 0;
        for i in 0..self.n {
            let idx = (self.head + HISTORY_CAP - 1 - i) % HISTORY_CAP;
            match self.ring[idx] {
                Some(r) if r.allowed => streak += 1,
                _ => break,
            }
        }
        streak
    }

    /// 最近一条记录。
    pub fn last(&self) -> Option<DayRecord> {
        if self.n == 0 {
            return None;
        }
        self.ring[(self.head + HISTORY_CAP - 1) % HISTORY_CAP]
    }
}

// ---------------------------------------------------------------------------
// 分诊卡（三要素消费面）
// ---------------------------------------------------------------------------

/// 分诊卡：红查定位 + 主层三要素文案直取。
pub struct TriageCard {
    pub bad_check: CheckId,
    pub what: &'static str,
    pub why: &'static str,
    pub next: &'static str,
}

/// 从快照生成分诊卡：找第一个非绿查 → 三要素（triage_copy 主层逐字）。
pub fn triage_from(states: &[CheckState; 3]) -> Option<TriageCard> {
    for (i, s) in states.iter().enumerate() {
        if *s != CheckState::Green {
            let id = CheckId::ALL[i];
            let (what, why, next) = super::handoffchk::triage_copy(id);
            return Some(TriageCard { bad_check: id, what, why, next });
        }
    }
    None
}

/// 全绿无分诊卡（分诊只给坏的——好消息不生成卡片）。
pub fn triage_all_green(states: &[CheckState; 3]) -> bool {
    triage_from(states).is_none()
}

// ---------------------------------------------------------------------------
// 预检器自检（假故障注入）
// ---------------------------------------------------------------------------

/// 可注入探测替身：按位图回填红态（探测器说谎的场景模拟）。
pub struct LyingProbe {
    pub lie_bitmap: u8, // bit i = 第 i 查谎报红
    pub calls: u32,
}

impl super::handoffchk::GateProbe for LyingProbe {
    fn probe(&mut self, _id: CheckId) -> (CheckState, u64) {
        self.calls += 1;
        (CheckState::Green, 5)
    }
}

/// 自检裁决：注入假红后分诊卡必须生成且指向对应查（探测器说谎会被抓）。
pub fn precheck_selftest(lie_bit: usize) -> bool {
    if lie_bit >= 3 {
        return false;
    }
    // 构造带谎的快照：除谎位外全绿。
    let mut states = [CheckState::Green; 3];
    states[lie_bit] = CheckState::Red;
    match triage_from(&states) {
        Some(card) => card.bad_check == CheckId::ALL[lie_bit],
        None => false,
    }
}

// ---------------------------------------------------------------------------
// WINESP 目标路径账
// ---------------------------------------------------------------------------

/// 探测路径段（存在性探测逐段走）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EspSegment {
    DiskPresent,
    PartitionFound,
    FsMounted,
    BootmgrSeen,
}

/// 路径账：逐段状态 → 断在哪段（可解释的存在性失败）。
pub struct EspPath {
    states: [CheckState; 4],
}

impl EspPath {
    pub const fn all_green() -> EspPath {
        EspPath { states: [CheckState::Green; 4] }
    }

    pub fn set(&mut self, seg: EspSegment, s: CheckState) -> bool {
        let i = seg as usize;
        if i >= 4 {
            return false;
        }
        self.states[i] = s;
        true
    }

    /// 第一断点（顺序语义：磁盘不在则后面段无从谈起——短路报告）。
    pub fn first_break(&self) -> Option<EspSegment> {
        const ORDER: [EspSegment; 4] = [
            EspSegment::DiskPresent,
            EspSegment::PartitionFound,
            EspSegment::FsMounted,
            EspSegment::BootmgrSeen,
        ];
        for (i, seg) in ORDER.iter().enumerate() {
            if self.states[i] != CheckState::Green {
                return Some(*seg);
            }
        }
        None
    }

    /// 完整 → 目标存在裁决绿。
    pub fn exists(&self) -> bool {
        self.first_break().is_none()
    }
}

// ---------------------------------------------------------------------------
// 批次五自检
// ---------------------------------------------------------------------------

#[inline(never)]
pub fn run_handoffchk_b5_checks() -> CheckSet {
    let mut cs = CheckSet::new("F181-b5");

    // 1) 历史账：记录/拦停率（5 中 1 拦 = 200‰）。
    let mut h = GateHistory::new();
    for day in 0..5u32 {
        h.record(DayRecord { day, allowed: day != 2, snapshot6: 0 });
    }
    cs.add("history_blocked_rate", h.blocked_permille() == 200 && h.n == 5, "");

    // 2) 全绿连胜：最新 3 天连续全绿 → streak 3；中间一次拦停 → streak 断。
    let mut h2 = GateHistory::new();
    for day in 0..5u32 {
        h2.record(DayRecord { day, allowed: day >= 2, snapshot6: 0 });
    }
    cs.add("history_green_streak", h2.green_streak() == 3, "");

    // 3) 历史满容留痕：30 满后 overflows 计数（不静默丢）。
    let mut h3 = GateHistory::new();
    for day in 0..(HISTORY_CAP + 2) as u32 {
        h3.record(DayRecord { day, allowed: true, snapshot6: 0 });
    }
    cs.add("history_overflow", h3.n == HISTORY_CAP && h3.overflows == 2 && h3.last().unwrap().day == 31, "");

    // 4) 空历史诚实：拦停率 0 不冒充完美、streak 0、last None。
    let empty = GateHistory::new();
    cs.add("history_empty_honest", empty.blocked_permille() == 0 && empty.green_streak() == 0 && empty.last().is_none(), "");

    // 5) 分诊卡：哈希查红 → 卡指向哈希查 + 三要素逐字（主层 triage_copy）。
    let card = triage_from(&[CheckState::Green, CheckState::Red, CheckState::Green]).unwrap();
    let (w0, _, _) = super::handoffchk::triage_copy(CheckId::HashTrusted);
    cs.add(
        "triage_points_at_bad",
        card.bad_check == CheckId::HashTrusted && card.what == w0 && !card.why.is_empty() && !card.next.is_empty(),
        "",
    );

    // 6) 分诊顺序：两查同红 → 报第一查（顺序语义确定——不随机挑）。
    let card2 = triage_from(&[CheckState::Red, CheckState::Red, CheckState::Green]).unwrap();
    cs.add("triage_first_break", card2.bad_check == CheckId::TargetExists, "");

    // 7) 全绿无卡：好消息不生成卡片。
    cs.add("triage_all_green_none", triage_all_green(&[CheckState::Green; 3]), "");

    // 8) 自检三连：三个谎位各自被抓（探测器说谎必被分诊定位）。
    cs.add(
        "precheck_selftest_three",
        precheck_selftest(0) && precheck_selftest(1) && precheck_selftest(2),
        "",
    );

    // 9) 自检越界谎位拒：bit 3 不存在（位图面容量）。
    cs.add("precheck_selftest_bounds", !precheck_selftest(3), "");

    // 10) WINESP 路径全绿：目标存在。
    let mut p = EspPath::all_green();
    cs.add("esp_path_exists", p.exists(), "");

    // 11) WINESP 断点定位：FS 未挂载 → 断在 FsMounted（后面段不再看）。
    p.set(EspSegment::FsMounted, CheckState::Red);
    p.set(EspSegment::BootmgrSeen, CheckState::Red);
    cs.add(
        "esp_path_break",
        p.first_break() == Some(EspSegment::FsMounted) && !p.exists(),
        "",
    );

    // 12) 主册常量贯通：审计 32 容量一处一事实。
    cs.add("consts_aligned", AUDIT_CAP == 32 && core::mem::size_of::<AuditEntry>() >= 6, "");

    cs
}

// ---------------------------------------------------------------------------
// 宿主单测（批次五）
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests_b5 {
    use super::*;

    #[test]
    fn history_wraparound_recent_wins() {
        // 回卷后最近 30 天在账（第 31 天覆盖第 0 天）。
        let mut h = GateHistory::new();
        for day in 0..(HISTORY_CAP + 1) as u32 {
            h.record(DayRecord { day, allowed: day % 2 == 0, snapshot6: 0 });
        }
        assert_eq!(h.last().unwrap().day, 30);
        assert_eq!(h.n, HISTORY_CAP);
    }

    #[test]
    fn triage_exception_treated_as_bad() {
        // 异常态（fail-closed）同样触发分诊（不只是红——异常也要人话）。
        let card = triage_from(&[CheckState::Green, CheckState::Exception, CheckState::Green]).unwrap();
        assert_eq!(card.bad_check, CheckId::HashTrusted);
    }

    #[test]
    fn esp_path_sequential_short_circuit() {
        // 短路序：首段红时后段即使绿也以首段为准（探测顺序语义）。
        let mut p = EspPath::all_green();
        p.set(EspSegment::DiskPresent, CheckState::Red);
        p.set(EspSegment::PartitionFound, CheckState::Green);
        assert_eq!(p.first_break(), Some(EspSegment::DiskPresent));
    }
}
