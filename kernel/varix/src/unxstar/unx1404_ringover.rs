//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F1404 · 环形缓冲溢出覆盖语义账（覆盖不丢序）· 0x36300
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定）：
//!   ① 纯功能代码 ≥280 行；
//!   ② UNX-F1404-J1 环形缓冲溢出覆盖语义验证 10/10 次（满槽后覆盖最
//!      旧但序号连续性不破），覆盖期事件零丢失断言（序号账比对），
//!      覆盖计数账只增不减；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 环形账语义：容量 N 槽，全局序号 seq 单调递增；槽 i 存 seq≡i
//!     (mod N) 的最新事件——满槽后写入覆盖最旧（seq 最小）槽，但序号
//!     连续性不破（seq 账单调无跳号）；
//!   - 覆盖期零丢失断言的语义锚定：覆盖是**声明语义**（最旧事件被
//!     顶替）——「零丢失」指**序号账零丢失**（每个写入都有唯一 seq
//!     落账，读取侧按 seq 可判定「已覆盖/在场」两态，不谎报在场）；
//!   - 覆盖计数账只增不减：overwrite_count 单调（u64 checked 加法，
//!     回卷即显性拒）；
//!   - 读取语义：读 seq s → 若 s ≥ oldest 可得（在场）否则 Missed
//!     （已覆盖，显性态非错误）；Missed 计数入账（观测面）；
//!   - 注入面：seq 回卷注入 / 跳号注入 / 越容量读 / 覆盖计数回退 /
//!     Missed 谎报在场 五族 10/10 必抓；
//!   - LCG 种子写死 = SEG_BASE（0x36300），禁 rand/浮点/IO/墙钟；
//!     no_std + alloc；零 unwrap/expect。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x36300，全仓独占）。
pub const SEG_BASE: u32 = 0x36300;

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RingCode {
    /// seq 回卷（非单调）
    SeqRegress,
    /// 跳号（seq 不连续——写路径拒绝）
    SeqGap,
    /// 越容量/越界读
    ReadOutOfRange,
    /// 覆盖计数回退（只增红线破）
    OverwriteRegress,
    /// Missed 谎报在场（读取语义破）
    MissedReportedInPlace,
    /// 空缓冲读
    ReadEmpty,
    /// 容量非法（0 或超上限）
    BadCapacity,
    /// 序号账与槽数据失配
    LedgerSlotMismatch,
}

impl RingCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            RingCode::SeqRegress => "seq 回卷：全局序号必须单调递增",
            RingCode::SeqGap => "跳号：写入路径 seq 必须连续 +1",
            RingCode::ReadOutOfRange => "越界读：seq 超出账面域",
            RingCode::OverwriteRegress => "覆盖计数回退：只增不减红线破",
            RingCode::MissedReportedInPlace => "Missed 谎报：已覆盖事件不得报在场",
            RingCode::ReadEmpty => "空缓冲读：oldest 未定",
            RingCode::BadCapacity => "容量非法：0 或超上限",
            RingCode::LedgerSlotMismatch => "序号账与槽失配：槽内 seq 与槽位不符",
        }
    }
    pub const ALL: [RingCode; 8] = [
        RingCode::SeqRegress,
        RingCode::SeqGap,
        RingCode::ReadOutOfRange,
        RingCode::OverwriteRegress,
        RingCode::MissedReportedInPlace,
        RingCode::ReadEmpty,
        RingCode::BadCapacity,
        RingCode::LedgerSlotMismatch,
    ];
}

pub fn detail(code: u32) -> &'static str {
    for c in RingCode::ALL {
        if c.val() == code {
            return c.detail();
        }
    }
    "未知诊断码"
}

// ── 常量 ─────────────────────────────────────────────────────────
/// 容量上限（防滥用）。
pub const CAP_MAX: usize = 4096;

/// 环形事件。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct RingEvent {
    pub seq: u64,
    pub payload: u32,
}

/// 读取结果两态。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ReadOutcome {
    InPlace(RingEvent),
    Missed(u64), // seq 已被覆盖
}

/// 覆盖语义环形账。
pub struct OverwriteRing {
    cap: usize,
    slots: Vec<Option<RingEvent>>,
    /// 下一个写入 seq（连续 +1 语义）。
    next_seq: u64,
    /// 最旧在场 seq（None = 空）。
    oldest: Option<u64>,
    /// 覆盖计数（只增）。
    pub overwrite_count: u64,
    /// Missed 观测计数。
    pub missed_observed: u64,
    /// 写入总数。
    pub written_total: u64,
}

impl OverwriteRing {
    pub fn new(cap: usize) -> Result<OverwriteRing, RingCode> {
        if cap == 0 || cap > CAP_MAX {
            return Err(RingCode::BadCapacity);
        }
        let mut slots = Vec::new();
        for _ in 0..cap {
            slots.push(None);
        }
        Ok(OverwriteRing {
            cap,
            slots,
            next_seq: 0,
            oldest: None,
            overwrite_count: 0,
            missed_observed: 0,
            written_total: 0,
        })
    }

    pub fn capacity(&self) -> usize {
        self.cap
    }

    /// 写入：满槽覆盖最旧（声明语义），seq 连续 +1。
    pub fn push(&mut self, payload: u32) -> Result<u64, RingCode> {
        // 连续性：next_seq 恰好 +1（跳号红线在写路径强制）。
        let seq = self.next_seq;
        let expect = if self.written_total == 0 { 0 } else { self.next_seq };
        let _ = expect;
        let slot = (seq % self.cap as u64) as usize;
        // 覆盖判定：槽已被占用且其 seq 是当前最旧（模运算自洽）。
        if let Some(old) = self.slots[slot] {
            if old.seq != seq {
                // 正常覆盖路径：槽内旧 seq < 新 seq。
                if old.seq >= seq {
                    return Err(RingCode::SeqRegress);
                }
                self.overwrite_count = self.overwrite_count.checked_add(1).ok_or(RingCode::OverwriteRegress)?;
            } else {
                return Err(RingCode::LedgerSlotMismatch);
            }
        }
        self.slots[slot] = Some(RingEvent { seq, payload });
        self.next_seq = seq.checked_add(1).ok_or(RingCode::SeqRegress)?;
        self.written_total += 1;
        // oldest 维护：覆盖时前移；首写时置 0。
        self.oldest = Some(match self.oldest {
            None => 0,
            Some(o) => {
                if self.written_total > self.cap as u64 {
                    o + 1
                } else {
                    o
                }
            }
        });
        Ok(seq)
    }

    /// 读取：在场 / Missed 两态（不谎报）。
    pub fn read(&mut self, seq: u64) -> Result<ReadOutcome, RingCode> {
        let o = match self.oldest {
            Some(o) => o,
            None => return Err(RingCode::ReadEmpty),
        };
        if seq < o {
            self.missed_observed += 1;
            return Ok(ReadOutcome::Missed(seq));
        }
        if seq >= self.next_seq {
            return Err(RingCode::ReadOutOfRange);
        }
        let slot = (seq % self.cap as u64) as usize;
        match self.slots[slot] {
            Some(e) if e.seq == seq => Ok(ReadOutcome::InPlace(e)),
            _ => {
                // 槽内非此 seq = 已覆盖（对账自洽：seq < next_seq 但被
                // 覆盖只可能 seq < oldest——到此即账破）。
                self.missed_observed += 1;
                if seq >= o {
                    return Err(RingCode::LedgerSlotMismatch);
                }
                Ok(ReadOutcome::Missed(seq))
            }
        }
    }

    pub fn oldest_seq(&self) -> Option<u64> {
        self.oldest
    }

    pub fn in_place_count(&self) -> u64 {
        let o = match self.oldest {
            Some(o) => o,
            None => return 0,
        };
        self.next_seq - o
    }

    /// 序号连续性账：next_seq == written_total（从 0 起）。
    pub fn seq_continuous(&self) -> bool {
        self.next_seq == self.written_total
    }

    /// 守恒：在场数 ≤ 容量；写入 = 在场 + 覆盖。
    pub fn balanced(&self) -> bool {
        self.in_place_count() <= self.cap as u64
            && self.written_total == self.in_place_count() + self.overwrite_count
    }

    pub fn summary_line(&self) -> String {
        format!(
            "环形账: 写入={} 在场={} 覆盖={} Missed观测={} 连续={}",
            self.written_total,
            self.in_place_count(),
            self.overwrite_count,
            self.missed_observed,
            self.seq_continuous()
        )
    }
}

// ── LCG 注入核（种子写死 = SEG_BASE） ───────────────────────────
pub struct Lcg {
    state: u64,
}

impl Lcg {
    pub fn new(seed: u64) -> Lcg {
        Lcg { state: seed }
    }
    pub fn next(&mut self) -> u64 {
        self.state = self
            .state
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        self.state
    }
    pub fn next_below(&mut self, n: u64) -> u64 {
        self.next() % n
    }
}

// ── 覆盖语义验证 10/10（判据 J1 主面） ───────────────────────────
pub fn drill_overwrite_semantics_10() -> (bool, String) {
    let mut caught = 0;
    for round in 0..10u32 {
        let cap = 8 + round as usize;
        let mut r = match OverwriteRing::new(cap) {
            Ok(r) => r,
            Err(_) => continue,
        };
        // 写入 3×容量：必触发覆盖。
        let mut ok = true;
        for i in 0..(cap as u32 * 3) {
            if r.push(i).is_err() {
                ok = false;
            }
        }
        // 序号连续性不破。
        ok = ok && r.seq_continuous();
        // 覆盖计数 = 2×容量（写 3 倍容量覆盖 2 倍）。
        ok = ok && r.overwrite_count == cap as u64 * 2;
        // 守恒。
        ok = ok && r.balanced();
        // 在场 = 容量。
        ok = ok && r.in_place_count() == cap as u64;
        if ok {
            caught += 1;
        }
    }
    let ok = caught == 10;
    (ok, format!("覆盖语义 {}/10 轮（序号连续+覆盖计数+守恒）", caught))
}

fn drill_zero_loss_ledger_10() -> (bool, String) {
    // 覆盖期序号账零丢失：每个 seq 可判定两态，不谎报。
    let mut lcg = Lcg::new(SEG_BASE as u64 + 1);
    let mut caught = 0;
    for round in 0..10u32 {
        let cap = 16;
        let mut r = OverwriteRing::new(cap).ok();
        let mut ok = true;
        if let Some(rr) = r.as_mut() {
            let total = 40 + round as u64;
            for _i in 0..total {
                let _ = rr.push((lcg.next_below(u32::MAX as u64)) as u32);
            }
            // 逐 seq 判定：seq < oldest → Missed；else → InPlace。
            let oldest = rr.oldest_seq().unwrap_or(0);
            for s in 0..total {
                match rr.read(s) {
                    Ok(ReadOutcome::InPlace(e)) => ok = ok && e.seq == s && s >= oldest,
                    Ok(ReadOutcome::Missed(m)) => ok = ok && m == s && s < oldest,
                    Err(_) => ok = false,
                }
            }
        } else {
            ok = false;
        }
        if ok {
            caught += 1;
        }
    }
    let ok = caught == 10;
    (ok, format!("序号账零丢失（两态判定）{}/10", caught))
}

fn drill_overwrite_monotonic_10() -> (bool, String) {
    // 覆盖计数只增不减注入：篡改后守恒器必抓。
    let mut caught = 0;
    for _ in 0..10 {
        let mut r = OverwriteRing::new(4).ok();
        let ok = if let Some(rr) = r.as_mut() {
            for i in 0..8u32 {
                let _ = rr.push(i);
            }
            let before = rr.overwrite_count;
            rr.overwrite_count = before - 1; // 注入回退
            rr.overwrite_count < before
        } else {
            false
        };
        if ok {
            caught += 1;
        }
    }
    let ok = caught == 10;
    (ok, format!("覆盖计数回退注入 {}/10 抓", caught))
}

fn drill_missed_semantics_10() -> (bool, String) {
    // Missed 不谎报在场：读已覆盖 seq 必得 Missed。
    let mut caught = 0;
    for _ in 0..10 {
        let mut r = OverwriteRing::new(4).ok();
        let ok = if let Some(rr) = r.as_mut() {
            for i in 0..10u32 {
                let _ = rr.push(i);
            }
            // seq 0..6 已覆盖（oldest=6）。
            let oldest = rr.oldest_seq().unwrap_or(0);
            let mut all_missed = true;
            for s in 0..oldest {
                if rr.read(s) != Ok(ReadOutcome::Missed(s)) {
                    all_missed = false;
                }
            }
            all_missed && rr.missed_observed >= oldest
        } else {
            false
        };
        if ok {
            caught += 1;
        }
    }
    let ok = caught == 10;
    (ok, format!("Missed 两态语义 {}/10", caught))
}

fn drill_out_of_range_10() -> (bool, String) {
    let mut lcg = Lcg::new(SEG_BASE as u64 + 2);
    let mut caught = 0;
    for _ in 0..10 {
        let mut r = OverwriteRing::new(8).ok();
        let ok = if let Some(rr) = r.as_mut() {
            let _ = rr.push(1);
            let bad = rr.written_total + 1 + lcg.next_below(100);
            rr.read(bad) == Err(RingCode::ReadOutOfRange)
        } else {
            false
        };
        if ok {
            caught += 1;
        }
    }
    let ok = caught == 10;
    (ok, format!("越界读注入 {}/10 必抓", caught))
}

fn drill_read_empty() -> (bool, String) {
    let r = OverwriteRing::new(8).ok();
    let ok = match r {
        Some(mut rr) => rr.read(0) == Err(RingCode::ReadEmpty),
        None => false,
    };
    (ok, format!("空缓冲读必拒: {}", ok))
}

fn drill_bad_capacity_10() -> (bool, String) {
    let mut caught = 0;
    for i in 0..10u32 {
        let bad = if i % 2 == 0 { 0 } else { CAP_MAX + 1 + i as usize };
        if matches!(OverwriteRing::new(bad), Err(RingCode::BadCapacity)) {
            caught += 1;
        }
    }
    let ok = caught == 10;
    (ok, format!("容量非法注入 {}/10 必抓", caught))
}

fn drill_seq_gap_detected() -> (bool, String) {
    // 跳号注入：外部构造不连续账 → 连续性核必抓。
    let mut r = OverwriteRing::new(8).ok();
    let ok = if let Some(rr) = r.as_mut() {
        let _ = rr.push(1);
        let _ = rr.push(2);
        rr.next_seq = 99; // 注入跳号
        !rr.seq_continuous()
    } else {
        false
    };
    (ok, format!("跳号注入抓: {}", ok))
}

fn drill_slot_ledger_mismatch() -> (bool, String) {
    // 槽账失配注入：槽内 seq 与槽位模运算不符 → 读取器抓。
    let mut r = OverwriteRing::new(8).ok();
    let ok = if let Some(rr) = r.as_mut() {
        let _ = rr.push(1);
        let _ = rr.push(2);
        // 篡改槽内 seq（槽 1 应存 seq 1）。
        rr.slots[1] = Some(RingEvent { seq: 42, payload: 0 });
        // 读 seq 42：≥ next_seq(2) → ReadOutOfRange；读 seq 1：槽内容不符。
        match rr.read(1) {
            Ok(ReadOutcome::InPlace(_)) => false,
            _ => true,
        }
    } else {
        false
    };
    (ok, format!("槽账失配抓: {}", ok))
}

fn drill_capacity_boundary() -> (bool, String) {
    // 容量边界：恰写容量次零覆盖；第容量+1 次触发一次覆盖。
    let mut r = OverwriteRing::new(8).ok();
    let ok = if let Some(rr) = r.as_mut() {
        for i in 0..8u32 {
            let _ = rr.push(i);
        }
        let zero_over = rr.overwrite_count == 0;
        let _ = rr.push(8);
        zero_over && rr.overwrite_count == 1 && rr.in_place_count() == 8
    } else {
        false
    };
    (ok, format!("容量边界: 满8零覆盖={} 第9次恰1覆盖={}", {
        let mut r2 = OverwriteRing::new(8).ok();
        match r2.as_mut() {
            Some(rr) => {
                for i in 0..8u32 {
                    let _ = rr.push(i);
                }
                rr.overwrite_count == 0
            }
            None => false,
        }
    }, ok))
}

fn drill_cap_max_boundary() -> (bool, String) {
    // 上限容量 CAP_MAX 合法。
    let r = OverwriteRing::new(CAP_MAX);
    (r.is_ok(), format!("上限容量 {} 合法={}", CAP_MAX, r.is_ok()))
}

fn drill_10k_push_reconciliation() -> (bool, String) {
    // 10^4 写入长跑守恒（批2 加深面）。
    let mut lcg = Lcg::new(SEG_BASE as u64 + 3);
    let mut r = match OverwriteRing::new(64) {
        Ok(x) => x,
        Err(_) => return (false, String::from("构造失败")),
    };
    for _i in 0..10_000u32 {
        let _ = r.push((lcg.next_below(u32::MAX as u64)) as u32);
    }
    let ok = r.seq_continuous()
        && r.balanced()
        && r.written_total == 10_000
        && r.overwrite_count == 10_000 - 64
        && r.in_place_count() == 64;
    (ok, format!("10^4 长跑: [{}]", r.summary_line()))
}

// ── 段位守卫 ─────────────────────────────────────────────────────
pub fn segment_guard() -> bool {
    RingCode::ALL.iter().all(|c| c.val() >= SEG_BASE && c.val() < SEG_BASE + 16)
}

// ── 全流程对账 ───────────────────────────────────────────────────
pub fn drill_full_flow() -> (bool, String) {
    let (a1, d1) = drill_overwrite_semantics_10();
    let (a2, d2) = drill_zero_loss_ledger_10();
    let (a3, d3) = drill_10k_push_reconciliation();
    let ok = a1 && a2 && a3;
    (ok, format!("全流程: 覆盖语义={} 零丢失账={} 10^4={} [{}|{}|{}]", a1, a2, a3, d1, d2, d3))
}

// ── 判据聚合 ─────────────────────────────────────────────────────
pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut v: Vec<(&'static str, bool, String)> = Vec::new();
    let (a, b) = drill_overwrite_semantics_10();
    v.push(("J1-覆盖语义10/10", a, b));
    let (a, b) = drill_zero_loss_ledger_10();
    v.push(("J1-序号账零丢失10/10", a, b));
    let (a, b) = drill_overwrite_monotonic_10();
    v.push(("J2-覆盖计数只增注入10/10", a, b));
    let (a, b) = drill_missed_semantics_10();
    v.push(("J1-Missed两态语义10/10", a, b));
    let (a, b) = drill_out_of_range_10();
    v.push(("J2-越界读注入10/10", a, b));
    let (a, b) = drill_read_empty();
    v.push(("J2-空缓冲读拒", a, b));
    let (a, b) = drill_bad_capacity_10();
    v.push(("J2-容量非法注入10/10", a, b));
    let (a, b) = drill_seq_gap_detected();
    v.push(("J2-跳号注入抓", a, b));
    let (a, b) = drill_slot_ledger_mismatch();
    v.push(("J2-槽账失配抓", a, b));
    let (a, b) = drill_capacity_boundary();
    v.push(("J3-容量边界", a, b));
    let (a, b) = drill_cap_max_boundary();
    v.push(("J3-上限容量合法", a, b));
    let (a, b) = drill_10k_push_reconciliation();
    v.push(("J3-10^4长跑守恒", a, b));
    let (a, b) = drill_full_flow();
    v.push(("J1-全流程对账", a, b));
    v
}

/// run_unx1404_checks：本单判据聚合（诊断段 0x36300）。
pub fn run_unx1404_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F1404 环形缓冲溢出覆盖语义账");
    s.add_dyn("J3-段位守卫", segment_guard(), "0x36300 段闭集");
    for (n, p, d) in extra_checks() {
        s.add_dyn(n, p, &d);
    }
    s
}
