//! F183 存储健康监测 · 批次六深化（secstar · G-G-13）。
//!
//! 批次六功能面（达成率 52%——最大缺口之一。与 b3/b4/b5 互补，本批管
//! 「全量表与扇区账」）：
//! - [`AttrTable`]：全量 SMART 属性表——多属性帧序列逐条入表/按键查
//!   （批次三管单帧，本批管整表：一张表看全盘属性）；
//! - [`SelfTestLog`]：SMART 自测日志解析——离线/短测/长测三型记录
//!   （厂商自测结果也是健康证据）；
//! - [`TempCurve`]：温度曲线段分析——爬升/平台/回落三段识别
//!   （瞬时读数之上看形状：持续爬升=散热在恶化）；
//! - [`SectorsAccount`]：扇区四数账——总/好/重分配/待映射守恒
//!   （总=好+重分配+待映射——账不守恒=读数在撒谎）。
//!
//! 零堆纪律：定长表 + 定长曲线窗，无 alloc。

use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// 全量 SMART 属性表
// ---------------------------------------------------------------------------

/// 表容量（常见盘 30-50 属性，表定 32 槽）。
pub const TABLE_CAP: usize = 32;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AttrEntry {
    pub id: u8,
    pub value: u8,
    pub raw_low: u32, // 原始值低 32 位（多数关键属性在此）
}

#[derive(Clone, Copy)]
pub struct AttrTable {
    entries: [Option<AttrEntry>; TABLE_CAP],
    pub n: usize,
}

impl AttrTable {
    pub const fn new() -> AttrTable {
        AttrTable { entries: [const { None }; TABLE_CAP], n: 0 }
    }

    /// 入表（id 已在 → 更新现值；新 id → 追加；满容诚实拒）。
    pub fn upsert(&mut self, id: u8, value: u8, raw_low: u32) -> bool {
        for e in self.entries[..self.n].iter_mut().flatten() {
            if e.id == id {
                e.value = value;
                e.raw_low = raw_low;
                return true;
            }
        }
        if self.n >= TABLE_CAP {
            return false;
        }
        self.entries[self.n] = Some(AttrEntry { id, value, raw_low });
        self.n += 1;
        true
    }

    pub fn get(&self, id: u8) -> Option<AttrEntry> {
        self.entries[..self.n].iter().flatten().copied().find(|e| e.id == id)
    }

    /// 表内最低 value（最弱属性——短板决定健康叙事）。
    pub fn weakest(&self) -> Option<AttrEntry> {
        self.entries[..self.n].iter().flatten().copied().min_by_key(|e| e.value)
    }
}

// ---------------------------------------------------------------------------
// SMART 自测日志
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SelfTestType {
    Offline,
    Short,
    Long,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SelfTestResult {
    Completed,
    Aborted,
    Failed,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SelfTestRec {
    pub kind: SelfTestType,
    pub result: SelfTestResult,
    pub hours_at_test: u16,
}

/// 自测日志环（21 条——ATA 标准容量）。
pub const SELFTEST_LOG_CAP: usize = 21;

pub struct SelfTestLog {
    ring: [Option<SelfTestRec>; SELFTEST_LOG_CAP],
    head: usize,
    pub n: usize,
}

impl SelfTestLog {
    pub const fn new() -> SelfTestLog {
        SelfTestLog { ring: [const { None }; SELFTEST_LOG_CAP], head: 0, n: 0 }
    }

    pub fn push(&mut self, rec: SelfTestRec) {
        if self.n < SELFTEST_LOG_CAP {
            self.n += 1;
        }
        self.ring[self.head] = Some(rec);
        self.head = (self.head + 1) % SELFTEST_LOG_CAP;
    }

    /// 最近一次 Long 测试结果（长测是最全面的体检——它的结论优先）。
    pub fn last_long(&self) -> Option<SelfTestResult> {
        (0..self.n)
            .filter_map(|i| self.ring[(self.head + SELFTEST_LOG_CAP - 1 - i) % SELFTEST_LOG_CAP])
            .find(|r| r.kind == SelfTestType::Long)
            .map(|r| r.result)
    }

    /// 失败计数（日志内 Failed 总数）。
    pub fn failed_count(&self) -> usize {
        self.ring.iter().flatten().filter(|r| r.result == SelfTestResult::Failed).count()
    }
}

// ---------------------------------------------------------------------------
// 温度曲线段分析
// ---------------------------------------------------------------------------

/// 曲线窗容量。
pub const CURVE_WINDOW: usize = 12;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CurvePhase {
    Rising,
    Plateau,
    Falling,
}

pub struct TempCurve {
    win: [i16; CURVE_WINDOW],
    n: usize,
    head: usize,
}

impl TempCurve {
    pub const fn new() -> TempCurve {
        TempCurve { win: [0; CURVE_WINDOW], n: 0, head: 0 }
    }

    pub fn push(&mut self, c: i16) {
        self.win[self.head] = c;
        self.head = (self.head + 1) % CURVE_WINDOW;
        if self.n < CURVE_WINDOW {
            self.n += 1;
        }
    }

    fn ordered(&self) -> [i16; CURVE_WINDOW] {
        let mut out = [0i16; CURVE_WINDOW];
        for i in 0..self.n {
            out[i] = self.win[(self.head + CURVE_WINDOW - self.n + i) % CURVE_WINDOW];
        }
        out
    }

    /// 段判定：末值-首值 > 5 = 爬升；< -5 = 回落；之间 = 平台。
    pub fn phase(&self) -> Option<CurvePhase> {
        if self.n < 2 {
            return None;
        }
        let o = self.ordered();
        let delta = o[self.n - 1] - o[0];
        if delta > 5 {
            Some(CurvePhase::Rising)
        } else if delta < -5 {
            Some(CurvePhase::Falling)
        } else {
            Some(CurvePhase::Plateau)
        }
    }

    /// 窗内峰值。
    pub fn peak(&self) -> Option<i16> {
        (0..self.n).map(|i| self.win[i]).max()
    }
}

// ---------------------------------------------------------------------------
// 扇区四数账
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SectorsAccount {
    pub total: u64,
    pub good: u64,
    pub realloc: u64,
    pub pending: u64,
}

impl SectorsAccount {
    /// 守恒判定：total == good + realloc + pending（四数账自洽）。
    pub fn conserves(&self) -> bool {
        self.total == self.good + self.realloc + self.pending
    }

    /// 健康率 ‰（好/总）。
    pub fn health_permille(&self) -> u32 {
        if self.total == 0 {
            return 0;
        }
        (self.good * 1_000 / self.total) as u32
    }
}

/// 守恒修复器：以 total/realloc/pending 反推 good（数据源缺失时的诚实插值）。
pub fn reconcile_good(total: u64, realloc: u64, pending: u64) -> Option<u64> {
    if realloc + pending > total {
        return None; // 坏比总数多 = 读数在撒谎，不修
    }
    Some(total - realloc - pending)
}

// ---------------------------------------------------------------------------
// 批次六自检
// ---------------------------------------------------------------------------

#[inline(never)]
pub fn run_diskhealth_b6_checks() -> CheckSet {
    let mut cs = CheckSet::new("F183-b6");

    // 1) 属性表 upsert：新 id 追加、旧 id 更新（表语义两面）。
    let mut t = AttrTable::new();
    t.upsert(5, 90, 16);
    t.upsert(197, 100, 0);
    t.upsert(5, 88, 24); // 更新
    cs.add(
        "attr_upsert",
        t.n == 2 && t.get(5).unwrap().value == 88 && t.get(5).unwrap().raw_low == 24 && t.get(197).unwrap().value == 100,
        "",
    );

    // 2) 最弱属性：表内 value 最小者出列（短板叙事面）。
    t.upsert(10, 50, 0);
    cs.add("attr_weakest", t.weakest().unwrap().id == 10, "");

    // 3) 表满容诚实拒：32 满后第 33 属性拒。
    let mut t2 = AttrTable::new();
    let mut all = true;
    for id in 0..TABLE_CAP as u8 {
        all &= t2.upsert(id + 1, 100, 0);
    }
    cs.add("attr_cap", all && !t2.upsert(200, 100, 0) && t2.n == TABLE_CAP, "");

    // 4) 自测日志：三型入环/最近 Long 检索（长测结论优先）。
    let mut l = SelfTestLog::new();
    l.push(SelfTestRec { kind: SelfTestType::Short, result: SelfTestResult::Completed, hours_at_test: 100 });
    l.push(SelfTestRec { kind: SelfTestType::Long, result: SelfTestResult::Completed, hours_at_test: 200 });
    l.push(SelfTestRec { kind: SelfTestType::Short, result: SelfTestResult::Failed, hours_at_test: 300 });
    cs.add(
        "selftest_last_long",
        l.last_long() == Some(SelfTestResult::Completed) && l.failed_count() == 1 && l.n == 3,
        "",
    );

    // 5) 自测回卷：21 满后最老滚出（环账纪律）。
    let mut l2 = SelfTestLog::new();
    for i in 0..(SELFTEST_LOG_CAP + 3) as u16 {
        l2.push(SelfTestRec { kind: SelfTestType::Long, result: SelfTestResult::Completed, hours_at_test: i });
    }
    cs.add("selftest_wraps", l2.n == SELFTEST_LOG_CAP && l2.last_long() == Some(SelfTestResult::Completed), "");

    // 6) 温度曲线三段：50→62 爬升、60±2 平台、62→48 回落（形状面）。
    let mut c = TempCurve::new();
    for v in [50i16, 53, 56, 58, 60, 62] {
        c.push(v);
    }
    let rising = c.phase() == Some(CurvePhase::Rising);
    let mut c2 = TempCurve::new();
    for v in [60i16, 61, 60, 61, 60, 61] {
        c2.push(v);
    }
    let plateau = c2.phase() == Some(CurvePhase::Plateau);
    let mut c3 = TempCurve::new();
    for v in [62i16, 58, 55, 52, 50, 48] {
        c3.push(v);
    }
    cs.add(
        "curve_three_phases",
        rising && plateau && c3.phase() == Some(CurvePhase::Falling),
        "",
    );

    // 7) 曲线峰值：62（爬升窗的顶点）。
    cs.add("curve_peak", c.peak() == Some(62), "");

    // 8) 扇区账守恒：total=好+重分配+待映射（账自洽）。
    let ok = SectorsAccount { total: 1_000, good: 990, realloc: 8, pending: 2 };
    let bad = SectorsAccount { total: 1_000, good: 995, realloc: 8, pending: 2 };
    cs.add("sectors_conserves", ok.conserves() && !bad.conserves(), "");

    // 9) 健康率：990/1000 = 990‰（好占比面）。
    cs.add("sectors_health", ok.health_permille() == 990 && SectorsAccount { total: 0, good: 0, realloc: 0, pending: 0 }.health_permille() == 0, "");

    // 10) 守恒修复器：反推 good 成功 / 坏>总诚实拒（插值两面）。
    cs.add(
        "reconcile_good",
        reconcile_good(1_000, 8, 2) == Some(990) && reconcile_good(100, 90, 20).is_none(),
        "",
    );

    cs
}

// ---------------------------------------------------------------------------
// 宿主单测（批次六）
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests_b6 {
    use super::*;

    #[test]
    fn attr_table_lookup_after_many() {
        // 大表乱序 upsert 后按键查全对（键查不受顺序影响）。
        let mut t = AttrTable::new();
        for id in [9u8, 1, 194, 5, 12] {
            t.upsert(id, id, id as u32);
        }
        assert_eq!(t.get(194).unwrap().id, 194);
        assert_eq!(t.get(1).unwrap().raw_low, 1);
        assert!(t.get(99).is_none());
    }

    #[test]
    fn curve_wraps_and_keeps_recent() {
        // 曲线窗回卷：12 窗塞 20 值只看最近 12（段判定按窗内）。
        let mut c = TempCurve::new();
        for i in 0..20i16 {
            c.push(40 + i);
        }
        // 窗内 = 48..59 → 升 11 → 爬升。
        assert_eq!(c.phase(), Some(CurvePhase::Rising));
        assert_eq!(c.peak(), Some(59));
    }

    #[test]
    fn selftest_zero_log_honest() {
        // 空日志：无 Long 记录 → None（不编造结论）。
        let l = SelfTestLog::new();
        assert!(l.last_long().is_none());
        assert_eq!(l.failed_count(), 0);
    }
}
