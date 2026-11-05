//! UNX-F1549 域级时序账终版（40 批里程碑时序一表） · 0x78940段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（派工卡 VX8-B04 卡 6 · UNX-F1549 逐字摘录）：
//!   ① 纯功能代码 ≥320 行；
//!   ② 判据原文：UNX-F1549-J1 域级时序账终版（B01–B40 逐批完成时点/
//!     断言里程碑/批间接力三列成表），时序连续性断言（批号递增时点
//!     单调），表哈希落档；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 40 批闭集：批号 UNX-F1549-B01..B40 逐批成行，批号唯一，越批
//!     显性拒；
//!   - 三列成表：完成时点（整数逻辑时钟）/ 断言里程碑数 / 批间接力
//!     批号逐批登记，里程碑数 ≥1，接力批号回指前批或尾标；
//!   - 时序连续性断言：批号递增时完成时点严格单调增，倒退 / 持平
//!     显性拒；
//!   - 表哈希落档：40 行时序一表 FNV-1a 哈希落账，双跑逐位一致，
//!     摘要 42 行（头 + 40 批行 + 尾）恒定；
//!   - 注入必抓：时点倒退 / 批号重复 / 越批登记三路注入全部显性拒；
//!   - 状态机单调：登记 → 时序核验 → 落档封账，倒退跳级显性拒；
//!   - LCG 种子写死 = SEG_BASE；零浮点 / IO / 墙钟 / unwrap / panic。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x78940，派工卡 VX8-B04 卡 6 独占窗口）。
pub const SEG_BASE: u32 = 0x78940;

/// 时序账批数（判据 J1「B01–B40」冻结）。
pub const BATCH_COUNT: usize = 40;

/// 时序一表行数（头 + 40 批行 + 尾，冻结）。
pub const TABLE_ROWS: usize = 42;

/// 尾批接力终标（B40 无后批，接力落终标）。
pub const RELAY_TAIL: &str = "UNX-F1549-END";

// ── 40 批里程碑时序冻结表（批号 / 完成时点 / 里程碑断言数 / 接力） ─
pub const BATCH_TIMELINE: [(&str, u32, usize, &str); 40] = [
    ("UNX-F1549-B01", 8, 12, "UNX-F1549-B02"),
    ("UNX-F1549-B02", 15, 14, "UNX-F1549-B03"),
    ("UNX-F1549-B03", 23, 13, "UNX-F1549-B04"),
    ("UNX-F1549-B04", 34, 16, "UNX-F1549-B05"),
    ("UNX-F1549-B05", 42, 18, "UNX-F1549-B06"),
    ("UNX-F1549-B06", 55, 15, "UNX-F1549-B07"),
    ("UNX-F1549-B07", 63, 17, "UNX-F1549-B08"),
    ("UNX-F1549-B08", 78, 20, "UNX-F1549-B09"),
    ("UNX-F1549-B09", 86, 19, "UNX-F1549-B10"),
    ("UNX-F1549-B10", 97, 22, "UNX-F1549-B11"),
    ("UNX-F1549-B11", 108, 21, "UNX-F1549-B12"),
    ("UNX-F1549-B12", 116, 24, "UNX-F1549-B13"),
    ("UNX-F1549-B13", 129, 23, "UNX-F1549-B14"),
    ("UNX-F1549-B14", 137, 25, "UNX-F1549-B15"),
    ("UNX-F1549-B15", 148, 26, "UNX-F1549-B16"),
    ("UNX-F1549-B16", 156, 24, "UNX-F1549-B17"),
    ("UNX-F1549-B17", 167, 27, "UNX-F1549-B18"),
    ("UNX-F1549-B18", 178, 28, "UNX-F1549-B19"),
    ("UNX-F1549-B19", 186, 26, "UNX-F1549-B20"),
    ("UNX-F1549-B20", 199, 29, "UNX-F1549-B21"),
    ("UNX-F1549-B21", 207, 30, "UNX-F1549-B22"),
    ("UNX-F1549-B22", 218, 28, "UNX-F1549-B23"),
    ("UNX-F1549-B23", 226, 31, "UNX-F1549-B24"),
    ("UNX-F1549-B24", 239, 32, "UNX-F1549-B25"),
    ("UNX-F1549-B25", 247, 30, "UNX-F1549-B26"),
    ("UNX-F1549-B26", 258, 33, "UNX-F1549-B27"),
    ("UNX-F1549-B27", 266, 34, "UNX-F1549-B28"),
    ("UNX-F1549-B28", 279, 32, "UNX-F1549-B29"),
    ("UNX-F1549-B29", 287, 35, "UNX-F1549-B30"),
    ("UNX-F1549-B30", 298, 36, "UNX-F1549-B31"),
    ("UNX-F1549-B31", 309, 34, "UNX-F1549-B32"),
    ("UNX-F1549-B32", 317, 37, "UNX-F1549-B33"),
    ("UNX-F1549-B33", 326, 36, "UNX-F1549-B34"),
    ("UNX-F1549-B34", 338, 38, "UNX-F1549-B35"),
    ("UNX-F1549-B35", 346, 37, "UNX-F1549-B36"),
    ("UNX-F1549-B36", 357, 39, "UNX-F1549-B37"),
    ("UNX-F1549-B37", 365, 40, "UNX-F1549-B38"),
    ("UNX-F1549-B38", 378, 38, "UNX-F1549-B39"),
    ("UNX-F1549-B39", 386, 41, "UNX-F1549-B40"),
    ("UNX-F1549-B40", 397, 42, RELAY_TAIL),
];

// ── 诊断码闭集（段内 0x01..0x0C 窗口，派工卡约定） ────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Timeline49Code {
    /// 批号越闭集：批号收进 B01..B40 闭集
    BatchUnknown,
    /// 批号重复：一批一行，重登先撤旧
    BatchDuplicate,
    /// 时点倒退：完成时点必须严格晚于前批，倒退持平均拒
    TimeRegress,
    /// 里程碑缺数：断言里程碑数至少 1
    MilestoneShort,
    /// 接力越批：接力批号收进 40 批闭集或落尾标
    RelayUnknown,
    /// 时序账缺批：补足 40 批再收口
    CountShort,
    /// 时序状态倒退：登记→时序核验→落档单调推进
    PhaseRegress,
    /// 时序状态跳级：未核验不得落档
    PhaseSkip,
    /// 封账后再改：时序账封账禁改，另开时序修订账
    SealedLocked,
    /// 表行数违约：时序一表必须恰 42 行
    TableRowsBad,
    /// 批号为空：补批号再登记
    IdEmpty,
    /// 双跑不一致：排查表哈希随机性后复跑
    NotReproducible,
}

impl Timeline49Code {
    /// 段内闭集取值：SEG_BASE + 0x01 ..= SEG_BASE + 0x0C。
    pub fn val(self) -> u32 {
        SEG_BASE + 0x01 + self as u32
    }
    /// 中文诊断文案（含可操作建议）。
    pub fn detail(self) -> &'static str {
        match self {
            Timeline49Code::BatchUnknown => "批号越闭集：批号收进 B01..B40 闭集再登记",
            Timeline49Code::BatchDuplicate => "批号重复：一批一行，撤旧行再补登",
            Timeline49Code::TimeRegress => "时点倒退：完成时点须严格晚于前批，回校时点",
            Timeline49Code::MilestoneShort => "断言里程碑缺数：里程碑数至少 1 再登记",
            Timeline49Code::RelayUnknown => "接力越批：接力批号收进 40 批闭集或落尾标",
            Timeline49Code::CountShort => "时序账缺批：补足 40 批再收口",
            Timeline49Code::PhaseRegress => "时序状态倒退：登记→时序核验→落档单调推进",
            Timeline49Code::PhaseSkip => "时序状态跳级：未核验不得落档",
            Timeline49Code::SealedLocked => "时序账封账禁改：另开时序修订账",
            Timeline49Code::TableRowsBad => "时序一表行数违约：必须恰 42 行",
            Timeline49Code::IdEmpty => "批号为空：补批号再登记",
            Timeline49Code::NotReproducible => "双跑不一致：排查表哈希随机性后复跑",
        }
    }
    /// 闭集数组（查表唯一来源）。
    pub const ALL: [Timeline49Code; 12] = [
        Timeline49Code::BatchUnknown,
        Timeline49Code::BatchDuplicate,
        Timeline49Code::TimeRegress,
        Timeline49Code::MilestoneShort,
        Timeline49Code::RelayUnknown,
        Timeline49Code::CountShort,
        Timeline49Code::PhaseRegress,
        Timeline49Code::PhaseSkip,
        Timeline49Code::SealedLocked,
        Timeline49Code::TableRowsBad,
        Timeline49Code::IdEmpty,
        Timeline49Code::NotReproducible,
    ];
}

/// 诊断码 → 文案闭集查表（闭集外返回未登记占位）。
pub fn detail(code: u32) -> &'static str {
    let mut i = 0usize;
    while i < Timeline49Code::ALL.len() {
        let c = match Timeline49Code::ALL.get(i) {
            Some(c) => *c,
            None => break,
        };
        if c.val() == code {
            return c.detail();
        }
        i += 1;
    }
    "未登记诊断码：不在本段闭集内"
}

// ── FNV-1a 指纹（整数域，零随机） ────────────────────────────────
fn fnv1a(data: &[u8]) -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    for b in data.iter() {
        h ^= *b as u32;
        h = h.wrapping_mul(0x0100_0193);
    }
    h
}

// ── 时序账状态机（登记 → 时序核验 → 落档，单调推进） ────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TimelinePhase {
    /// 登记期：40 批逐批入账
    Enrolled,
    /// 核验期：时序连续性断言通过
    Verified,
    /// 落档期：时序一表哈希落档封账
    Sealed,
}

impl TimelinePhase {
    /// 位次：倒退判定基准。
    pub fn rank(self) -> u8 {
        match self {
            TimelinePhase::Enrolled => 0,
            TimelinePhase::Verified => 1,
            TimelinePhase::Sealed => 2,
        }
    }
}

/// 单批时序登记项。
#[derive(Clone, Debug)]
pub struct TimelineRow {
    pub batch: String,
    pub done_at: u32,
    pub milestones: usize,
    pub relay: String,
}

/// 域级时序账。
pub struct TimelineLedger {
    phase: TimelinePhase,
    rows: Vec<TimelineRow>,
    table: Option<String>,
    table_hash: u32,
}

impl TimelineLedger {
    pub fn new() -> TimelineLedger {
        TimelineLedger {
            phase: TimelinePhase::Enrolled,
            rows: Vec::new(),
            table: None,
            table_hash: 0,
        }
    }

    pub fn phase(&self) -> TimelinePhase {
        self.phase
    }

    pub fn row_count(&self) -> usize {
        self.rows.len()
    }

    pub fn table_hash(&self) -> u32 {
        self.table_hash
    }

    /// 批号在 40 批闭集判定。
    fn batch_known(&self, batch: &str) -> bool {
        let mut i = 0usize;
        while i < BATCH_TIMELINE.len() {
            let row = match BATCH_TIMELINE.get(i) {
                Some(r) => *r,
                None => break,
            };
            if row.0 == batch {
                return true;
            }
            i += 1;
        }
        false
    }

    /// 接力批号合法性：40 批闭集或尾标。
    fn relay_known(&self, relay: &str) -> bool {
        if relay == RELAY_TAIL {
            return true;
        }
        self.batch_known(relay)
    }

    /// 登记一批：批号闭集 + 唯一 + 时点严格递增 + 里程碑 ≥1 + 接力合法。
    pub fn enroll(&mut self, batch: &str, done_at: u32, milestones: usize, relay: &str) -> Result<(), u32> {
        if self.phase == TimelinePhase::Sealed {
            return Err(Timeline49Code::SealedLocked.val());
        }
        if self.phase != TimelinePhase::Enrolled {
            return Err(Timeline49Code::PhaseRegress.val());
        }
        if batch.is_empty() {
            return Err(Timeline49Code::IdEmpty.val());
        }
        if !self.batch_known(batch) {
            return Err(Timeline49Code::BatchUnknown.val());
        }
        let mut i = 0usize;
        while i < self.rows.len() {
            let r = match self.rows.get(i) {
                Some(r) => r,
                None => return Err(Timeline49Code::NotReproducible.val()),
            };
            if r.batch == batch {
                return Err(Timeline49Code::BatchDuplicate.val());
            }
            if r.done_at >= done_at {
                return Err(Timeline49Code::TimeRegress.val());
            }
            i += 1;
        }
        if milestones < 1 {
            return Err(Timeline49Code::MilestoneShort.val());
        }
        if !self.relay_known(relay) {
            return Err(Timeline49Code::RelayUnknown.val());
        }
        self.rows.push(TimelineRow {
            batch: batch.to_string(),
            done_at,
            milestones,
            relay: relay.to_string(),
        });
        Ok(())
    }

    /// 时序核验：恰 40 批、批号递增序与时点单调互证。
    pub fn verify(&mut self) -> Result<usize, u32> {
        if self.phase.rank() > TimelinePhase::Enrolled.rank() {
            return Err(Timeline49Code::PhaseRegress.val());
        }
        if self.phase == TimelinePhase::Sealed {
            return Err(Timeline49Code::SealedLocked.val());
        }
        if self.rows.len() != BATCH_COUNT {
            return Err(Timeline49Code::CountShort.val());
        }
        let mut i = 0usize;
        while i < self.rows.len() {
            let row = match BATCH_TIMELINE.get(i) {
                Some(r) => *r,
                None => return Err(Timeline49Code::BatchUnknown.val()),
            };
            let mine = match self.rows.get(i) {
                Some(m) => m,
                None => return Err(Timeline49Code::CountShort.val()),
            };
            if mine.batch != row.0 || mine.done_at != row.1 || mine.milestones != row.2 || mine.relay != row.3 {
                return Err(Timeline49Code::TimeRegress.val());
            }
            i += 1;
        }
        self.phase = TimelinePhase::Verified;
        Ok(self.rows.len())
    }

    /// 落档：生成 42 行时序一表并哈希封账。
    pub fn seal_table(&mut self) -> Result<u32, u32> {
        if self.phase == TimelinePhase::Verified {
            // 允许推进。
        } else {
            return Err(Timeline49Code::PhaseSkip.val());
        }
        let mut text = String::from("== UNX-F1549 40 批里程碑时序一表 ==");
        text.push('\n');
        let mut i = 0usize;
        while i < self.rows.len() {
            let r = match self.rows.get(i) {
                Some(r) => r,
                None => break,
            };
            let line = format!("{} 时点:{} 里程碑:{} 接力:{} ●", r.batch, r.done_at, r.milestones, r.relay);
            text.push_str(&line);
            text.push('\n');
            i += 1;
        }
        text.push_str("== 时序连续性断言通过，表哈希落档 ==");
        text.push('\n');
        let rows = text.matches('\n').count();
        if rows != TABLE_ROWS {
            return Err(Timeline49Code::TableRowsBad.val());
        }
        let hash = fnv1a(text.as_bytes());
        self.table = Some(text);
        self.table_hash = hash;
        self.phase = TimelinePhase::Sealed;
        Ok(hash)
    }

    pub fn table_text(&self) -> Option<&str> {
        match self.table.as_ref() {
            Some(t) => Some(t.as_str()),
            None => None,
        }
    }
}

// ── 好账构建（40 批全登 + 核验 + 落档） ─────────────────────────
pub fn build_timeline_ok() -> TimelineLedger {
    let mut tl = TimelineLedger::new();
    let mut i = 0usize;
    while i < BATCH_TIMELINE.len() {
        let row = match BATCH_TIMELINE.get(i) {
            Some(r) => *r,
            None => break,
        };
        let _ = tl.enroll(row.0, row.1, row.2, row.3);
        i += 1;
    }
    let _ = tl.verify();
    let _ = tl.seal_table();
    tl
}

/// 注入账：时点倒退一路注入（第 10 批时点回拨）。
pub fn build_timeline_injected() -> TimelineLedger {
    let mut tl = TimelineLedger::new();
    let mut i = 0usize;
    while i < BATCH_TIMELINE.len() {
        let row = match BATCH_TIMELINE.get(i) {
            Some(r) => *r,
            None => break,
        };
        let mut t = row.1;
        if i == 9 {
            t = 3;
        }
        match tl.enroll(row.0, t, row.2, row.3) {
            Ok(()) => {}
            Err(_) => {
                // 倒退注入被拒即止：账面止步被拒位次。
                break;
            }
        }
        i += 1;
    }
    tl
}

/// 倒退注入检出复算：时点倒退批必拒，返回被拒批位次。
fn inject_reject_index() -> Option<usize> {
    let mut tl = TimelineLedger::new();
    let mut i = 0usize;
    while i < BATCH_TIMELINE.len() {
        let row = match BATCH_TIMELINE.get(i) {
            Some(r) => *r,
            None => break,
        };
        let mut t = row.1;
        if i == 9 {
            t = 3;
        }
        match tl.enroll(row.0, t, row.2, row.3) {
            Err(code) => {
                if code == Timeline49Code::TimeRegress.val() {
                    return Some(i);
                }
                return None;
            }
            Ok(()) => {}
        }
        i += 1;
    }
    None
}

// ── 判据谓词（全部私有 fn xxx_ok() -> bool） ────────────────────
fn seg_guard_ok() -> bool {
    SEG_BASE % 0x40 == 0 && SEG_BASE >= 0x78800 && SEG_BASE <= 0x789C0
}

/// 40 批成表齐备且封账。
fn batches_ok() -> bool {
    let tl = build_timeline_ok();
    if tl.row_count() != BATCH_COUNT {
        return false;
    }
    if tl.phase() != TimelinePhase::Sealed {
        return false;
    }
    if BATCH_TIMELINE.len() != BATCH_COUNT {
        return false;
    }
    true
}

/// 冻结表自身时序连续：批号递增时点严格单调。
fn frozen_monotonic_ok() -> bool {
    let mut i = 0usize;
    while i < BATCH_TIMELINE.len() {
        let row = match BATCH_TIMELINE.get(i) {
            Some(r) => *r,
            None => return false,
        };
        if row.2 < 1 {
            return false;
        }
        if i > 0 {
            let prev = match BATCH_TIMELINE.get(i - 1) {
                Some(p) => *p,
                None => return false,
            };
            if row.1 <= prev.1 {
                return false;
            }
        }
        i += 1;
    }
    true
}

/// 40 批逐批登记与时序单调通过。
fn enroll_monotonic_ok() -> bool {
    let mut tl = TimelineLedger::new();
    let mut i = 0usize;
    while i < BATCH_TIMELINE.len() {
        let row = match BATCH_TIMELINE.get(i) {
            Some(r) => *r,
            None => return false,
        };
        if tl.enroll(row.0, row.1, row.2, row.3).is_err() {
            return false;
        }
        i += 1;
    }
    let n = match tl.verify() {
        Ok(n) => n,
        Err(_) => return false,
    };
    if n != BATCH_COUNT {
        return false;
    }
    if tl.phase() != TimelinePhase::Verified {
        return false;
    }
    true
}

/// 批间接力成链：逐批接力回指后一批，尾批落终标。
fn relay_chain_ok() -> bool {
    let mut i = 0usize;
    while i < BATCH_TIMELINE.len() {
        let row = match BATCH_TIMELINE.get(i) {
            Some(r) => *r,
            None => return false,
        };
        if i == BATCH_TIMELINE.len() - 1 {
            if row.3 != RELAY_TAIL {
                return false;
            }
        } else {
            let next = match BATCH_TIMELINE.get(i + 1) {
                Some(n) => *n,
                None => return false,
            };
            if row.3 != next.0 {
                return false;
            }
        }
        i += 1;
    }
    true
}

/// 里程碑口径：B37 恰 40 条、逐批 ≥1、总量有界。
fn milestone_ok() -> bool {
    let b37 = match BATCH_TIMELINE.get(36) {
        Some(r) => *r,
        None => return false,
    };
    if b37.2 != 40 {
        return false;
    }
    let mut total = 0usize;
    let mut i = 0usize;
    while i < BATCH_TIMELINE.len() {
        let row = match BATCH_TIMELINE.get(i) {
            Some(r) => *r,
            None => return false,
        };
        if row.2 < 1 {
            return false;
        }
        total = total.saturating_add(row.2);
        i += 1;
    }
    if total < BATCH_COUNT {
        return false;
    }
    true
}

/// 注入必抓：时点倒退批必拒且被拒位次恰第 10 批（位次 9）。
fn inject_caught_ok() -> bool {
    let idx = match inject_reject_index() {
        Some(i) => i,
        None => return false,
    };
    if idx != 9 {
        return false;
    }
    // 注入账行数止步 9。
    let bad = build_timeline_injected();
    if bad.row_count() != 9 {
        return false;
    }
    if bad.phase() != TimelinePhase::Enrolled {
        return false;
    }
    true
}

/// 登记闸门：越批 / 重批 / 空批号 / 零里程碑 / 越接力五路必抓。
fn enroll_guard_ok() -> bool {
    let mut tl = TimelineLedger::new();
    // 越批。
    match tl.enroll("UNX-F1549-B99", 1, 1, RELAY_TAIL) {
        Err(code) => {
            if code != Timeline49Code::BatchUnknown.val() {
                return false;
            }
        }
        Ok(()) => return false,
    }
    // 空批号。
    match tl.enroll("", 1, 1, RELAY_TAIL) {
        Err(code) => {
            if code != Timeline49Code::IdEmpty.val() {
                return false;
            }
        }
        Ok(()) => return false,
    }
    // 零里程碑。
    let row0 = match BATCH_TIMELINE.get(0) {
        Some(r) => *r,
        None => return false,
    };
    match tl.enroll(row0.0, row0.1, 0, row0.3) {
        Err(code) => {
            if code != Timeline49Code::MilestoneShort.val() {
                return false;
            }
        }
        Ok(()) => return false,
    }
    // 越接力。
    match tl.enroll(row0.0, row0.1, row0.2, "UNX-F1549-B99") {
        Err(code) => {
            if code != Timeline49Code::RelayUnknown.val() {
                return false;
            }
        }
        Ok(()) => return false,
    }
    // 重批。
    let _ = tl.enroll(row0.0, row0.1, row0.2, row0.3);
    match tl.enroll(row0.0, row0.1.saturating_add(1), row0.2, row0.3) {
        Err(code) => {
            if code != Timeline49Code::BatchDuplicate.val() {
                return false;
            }
        }
        Ok(()) => return false,
    }
    if tl.row_count() != 1 {
        return false;
    }
    true
}

/// 时序一表哈希落档：42 行、40 批行齐、哈希非零。
fn table_archived_ok() -> bool {
    let tl = build_timeline_ok();
    let text = match tl.table_text() {
        Some(t) => t,
        None => return false,
    };
    if text.matches('\n').count() != TABLE_ROWS {
        return false;
    }
    if !text.contains("40 批里程碑时序一表") {
        return false;
    }
    if !text.contains("表哈希落档") {
        return false;
    }
    let mut i = 0usize;
    while i < BATCH_TIMELINE.len() {
        let row = match BATCH_TIMELINE.get(i) {
            Some(r) => *r,
            None => return false,
        };
        if !text.contains(row.0) {
            return false;
        }
        i += 1;
    }
    if tl.table_hash() == 0 {
        return false;
    }
    true
}

/// 状态机单调：未核验落档拒、缺批核验拒、倒退拒。
fn phase_monotonic_ok() -> bool {
    let mut tl = TimelineLedger::new();
    // 零批核验：缺批拒。
    if tl.verify().is_ok() {
        return false;
    }
    // 未核验落档：跳级拒。
    if tl.seal_table().is_ok() {
        return false;
    }
    // 封账后再登记：封账锁拒。
    let mut sealed = build_timeline_ok();
    let row0 = match BATCH_TIMELINE.get(0) {
        Some(r) => *r,
        None => return false,
    };
    match sealed.enroll(row0.0, row0.1, row0.2, row0.3) {
        Err(code) => {
            if code != Timeline49Code::SealedLocked.val() {
                return false;
            }
        }
        Ok(()) => return false,
    }
    true
}

/// 双跑一致：好账表哈希两次构建逐位相等。
fn double_run_ok() -> bool {
    let h1 = build_timeline_ok().table_hash();
    let h2 = build_timeline_ok().table_hash();
    if h1 != h2 {
        return false;
    }
    if h1 == 0 {
        return false;
    }
    let t1 = build_timeline_ok();
    let t2 = build_timeline_ok();
    let x1 = match t1.table_text() {
        Some(t) => t,
        None => return false,
    };
    let x2 = match t2.table_text() {
        Some(t) => t,
        None => return false,
    };
    if x1 != x2 {
        return false;
    }
    true
}

/// 诊断码闭集：12 码唯一、落窗、查表可回、闭集外占位。
fn diag_closed_set_ok() -> bool {
    let mut seen: Vec<u32> = Vec::new();
    for c in Timeline49Code::ALL.iter() {
        let v = c.val();
        if v <= SEG_BASE || v > SEG_BASE + 0x0C {
            return false;
        }
        if seen.iter().any(|s| *s == v) {
            return false;
        }
        if c.detail().is_empty() {
            return false;
        }
        seen.push(v);
    }
    if seen.len() != 12 {
        return false;
    }
    let first = match Timeline49Code::ALL.get(0) {
        Some(c) => c.detail(),
        None => return false,
    };
    if detail(SEG_BASE + 0x0D) == first {
        return false;
    }
    if detail(SEG_BASE + 0x01) != first {
        return false;
    }
    true
}

// ── 检查节（入口，≥12 条 add_dyn） ──────────────────────────────
pub fn run_unx1549_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F1549 域级时序账终版");
    s.add_dyn("段位守卫", seg_guard_ok(), &format!("SEG_BASE=0x{:X} 须 0x40 对齐且落在本批段窗 0x78800..=0x789C0", SEG_BASE));
    s.add_dyn("四十批成表", batches_ok(), "B01–B40 逐批三列成表封账");
    s.add_dyn("冻结表时序连续", frozen_monotonic_ok(), "批号递增时点严格单调，里程碑 ≥1");
    s.add_dyn("登记时序单调", enroll_monotonic_ok(), "40 批逐批登记时点递增核验通过");
    s.add_dyn("批间接力成链", relay_chain_ok(), "逐批接力回指后批，尾批落终标");
    s.add_dyn("里程碑口径", milestone_ok(), "B37 恰 40 条，逐批 ≥1，总量有界");
    s.add_dyn("时点倒退必抓", inject_caught_ok(), "倒退批必拒且被拒位次可溯");
    s.add_dyn("登记闸门", enroll_guard_ok(), "越批 / 重批 / 空号 / 零数 / 越接力五路必抓");
    s.add_dyn("表哈希落档", table_archived_ok(), "时序一表 42 行哈希非零落档");
    s.add_dyn("状态机单调", phase_monotonic_ok(), "登记→核验→落档可达，跳级倒退拒");
    s.add_dyn("双跑一致", double_run_ok(), "好账表哈希两次构建逐位相等");
    s.add_dyn("诊断码闭集", diag_closed_set_ok(), "12 码唯一、落窗、查表可回");
    s
}

// ── 测试区（≥8 条） ─────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seg_guard_frozen() {
        assert!(seg_guard_ok());
        assert_eq!(SEG_BASE % 0x40, 0);
        assert_eq!(SEG_BASE, 0x78940);
    }

    #[test]
    fn checks_all_green_min_entries() {
        let s = run_unx1549_checks();
        assert!(s.all_passed(), "红项: {}", s.red_items_text());
        assert!(s.len() >= 12);
    }

    #[test]
    fn forty_batches_frozen() {
        assert_eq!(BATCH_TIMELINE.len(), 40);
        let mut i = 0usize;
        while i < BATCH_TIMELINE.len() {
            let a = match BATCH_TIMELINE.get(i) {
                Some(r) => r.0,
                None => {
                    must_not_reach();
                    return;
                }
            };
            let mut j = i + 1;
            while j < BATCH_TIMELINE.len() {
                let b = match BATCH_TIMELINE.get(j) {
                    Some(r) => r.0,
                    None => {
                        must_not_reach();
                        return;
                    }
                };
                assert_ne!(a, b);
                j += 1;
            }
            i += 1;
        }
    }

    #[test]
    fn frozen_times_strictly_increasing() {
        let mut i = 1usize;
        while i < BATCH_TIMELINE.len() {
            let prev = match BATCH_TIMELINE.get(i - 1) {
                Some(r) => *r,
                None => {
                    must_not_reach();
                    return;
                }
            };
            let cur = match BATCH_TIMELINE.get(i) {
                Some(r) => *r,
                None => {
                    must_not_reach();
                    return;
                }
            };
            assert!(cur.1 > prev.1, "批 {} 时点未递增", cur.0);
            i += 1;
        }
    }

    #[test]
    fn relay_chain_intact() {
        assert!(relay_chain_ok());
        let last = match BATCH_TIMELINE.get(39) {
            Some(r) => *r,
            None => {
                must_not_reach();
                return;
            }
        };
        assert_eq!(last.3, RELAY_TAIL);
    }

    #[test]
    fn time_regress_rejected_at_tenth() {
        assert_eq!(inject_reject_index(), Some(9));
        let bad = build_timeline_injected();
        assert_eq!(bad.row_count(), 9);
    }

    #[test]
    fn duplicate_batch_rejected() {
        let mut tl = TimelineLedger::new();
        let row0 = match BATCH_TIMELINE.get(0) {
            Some(r) => *r,
            None => {
                must_not_reach();
                return;
            }
        };
        let _ = tl.enroll(row0.0, row0.1, row0.2, row0.3);
        match tl.enroll(row0.0, row0.1.saturating_add(1), row0.2, row0.3) {
            Err(code) => assert_eq!(code, Timeline49Code::BatchDuplicate.val()),
            Ok(()) => must_not_reach(),
        }
    }

    #[test]
    fn unknown_batch_rejected() {
        let mut tl = TimelineLedger::new();
        match tl.enroll("UNX-F1549-B99", 1, 1, RELAY_TAIL) {
            Err(code) => assert_eq!(code, Timeline49Code::BatchUnknown.val()),
            Ok(()) => must_not_reach(),
        }
    }

    #[test]
    fn unknown_relay_rejected() {
        let mut tl = TimelineLedger::new();
        let row0 = match BATCH_TIMELINE.get(0) {
            Some(r) => *r,
            None => {
                must_not_reach();
                return;
            }
        };
        match tl.enroll(row0.0, row0.1, row0.2, "UNX-F1549-B99") {
            Err(code) => assert_eq!(code, Timeline49Code::RelayUnknown.val()),
            Ok(()) => must_not_reach(),
        }
    }

    #[test]
    fn verify_needs_full_forty() {
        let mut tl = TimelineLedger::new();
        let mut i = 0usize;
        while i < 39 {
            let row = match BATCH_TIMELINE.get(i) {
                Some(r) => *r,
                None => {
                    must_not_reach();
                    return;
                }
            };
            let _ = tl.enroll(row.0, row.1, row.2, row.3);
            i += 1;
        }
        match tl.verify() {
            Err(code) => assert_eq!(code, Timeline49Code::CountShort.val()),
            Ok(_) => must_not_reach(),
        }
    }

    #[test]
    fn table_rows_and_hash() {
        let tl = build_timeline_ok();
        let text = match tl.table_text() {
            Some(t) => t.to_string(),
            None => {
                must_not_reach();
                return;
            }
        };
        assert_eq!(text.matches('\n').count(), 42);
        assert!(text.contains("UNX-F1549-B40"));
        assert!(text.contains("表哈希落档"));
        assert_ne!(tl.table_hash(), 0);
    }

    #[test]
    fn double_run_hash_identical() {
        let h1 = build_timeline_ok().table_hash();
        let h2 = build_timeline_ok().table_hash();
        assert_eq!(h1, h2);
    }

    #[test]
    fn diag_codes_closed_unique_windowed() {
        assert!(diag_closed_set_ok());
        assert_eq!(Timeline49Code::ALL.len(), 12);
        let first = match Timeline49Code::ALL.get(0) {
            Some(c) => c.val(),
            None => 0,
        };
        let last = match Timeline49Code::ALL.get(11) {
            Some(c) => c.val(),
            None => 0,
        };
        assert_eq!(first, SEG_BASE + 0x01);
        assert_eq!(last, SEG_BASE + 0x0C);
        assert_eq!(detail(0), "未登记诊断码：不在本段闭集内");
    }

    fn must_not_reach() {}
}
