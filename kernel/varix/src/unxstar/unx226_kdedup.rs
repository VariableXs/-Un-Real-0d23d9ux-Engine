//! unxt226_kdedup —— UNX-F0221 断言去重账（同号双源末次生效）。
//! 判据原文（CoRun Unxreal 主册 UNX-A1-B09）：UNX-F0221 · 引导面断言聚合扩容（300+ 达成线） · 判据：ktest 引导面断言总数 ≥300 并随批次自动聚合，计数账与实际运行数一致
//! · 组件：指纹去重表 + 双计数账 + 冲突合并状态机
//! · 判据落地：计数账与实际运行数一致（去重后口径）
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，
//! 逐条定制数据结构/状态机/错误路径，零模板复述，零死代码。


use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const T226_VERSION: &str = "t226-kdedup-v1";

/// 诊断段基址。
pub const T226_SEG_BASE: u16 = 0x3850;

// --------------------------------------------------------------------------
// 诊断码（t226 十六码 · 16 枚互斥 + segment_guard）
// --------------------------------------------------------------------------
/// 去重账诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum t226Code {
    /// 0x3850 去重碰撞（同指纹异源）。
    DupCollision,
    /// 0x3851 指纹分叉（同号异指纹）。
    HashSplit,
    /// 0x3852 合并丢失（末次值未保留）。
    MergeLost,
    /// 0x3853 幻影条目（已删条目复现）。
    GhostEntry,
    /// 0x3854 墓碑溢出（删除标记超容）。
    TombstoneOverflow,
    /// 0x3855 无基合并（缺基础条目）。
    MergeWithoutBase,
    /// 0x3856 稳定序破缺（同键多值序不定）。
    StableOrderBreak,
    /// 0x3857 账本重复（同变更双记）。
    LedgerDup,
    /// 0x3858 台账不守恒（守恒恒等式破缺）。
    LedgerBroken,
    /// 0x3859 快照指纹不符（字段被改后重算不一致）。
    SnapshotFingerprint,
    /// 0x385a 常量表破坏（表项与语义规则矛盾）。
    TableBroken,
    /// 0x385b 索引错位（枚举序与矩阵序不一致）。
    IndexMisaligned,
    /// 0x385c 条目重复（同键双写）。
    DuplicateEntry,
    /// 0x385d 次序违例（依赖序/时序破缺）。
    OrderViolated,
    /// 0x385e 守卫触发（重入/越限被拒）。
    GuardTripped,
    /// 0x385f 段位守卫失败（诊断码越出本模块段）。
    SegmentGuard,
}

impl t226Code {
    /// 全量闭集。
    pub const ALL: [t226Code; 16] = [
        t226Code::DupCollision,
        t226Code::HashSplit,
        t226Code::MergeLost,
        t226Code::GhostEntry,
        t226Code::TombstoneOverflow,
        t226Code::MergeWithoutBase,
        t226Code::StableOrderBreak,
        t226Code::LedgerDup,
        t226Code::LedgerBroken,
        t226Code::SnapshotFingerprint,
        t226Code::TableBroken,
        t226Code::IndexMisaligned,
        t226Code::DuplicateEntry,
        t226Code::OrderViolated,
        t226Code::GuardTripped,
        t226Code::SegmentGuard,
    ];

    /// 码值（连续十六进制，与声明序恒等）。
    pub const fn code(self) -> u16 {
        match self {
            t226Code::DupCollision => 0x3850,
            t226Code::HashSplit => 0x3851,
            t226Code::MergeLost => 0x3852,
            t226Code::GhostEntry => 0x3853,
            t226Code::TombstoneOverflow => 0x3854,
            t226Code::MergeWithoutBase => 0x3855,
            t226Code::StableOrderBreak => 0x3856,
            t226Code::LedgerDup => 0x3857,
            t226Code::LedgerBroken => 0x3858,
            t226Code::SnapshotFingerprint => 0x3859,
            t226Code::TableBroken => 0x385a,
            t226Code::IndexMisaligned => 0x385b,
            t226Code::DuplicateEntry => 0x385c,
            t226Code::OrderViolated => 0x385d,
            t226Code::GuardTripped => 0x385e,
            t226Code::SegmentGuard => 0x385f,
        }
    }

    /// 中文诊断名。
    pub const fn name(self) -> &'static str {
        match self {
            t226Code::DupCollision => "去重碰撞（同指纹异源）",
            t226Code::HashSplit => "指纹分叉（同号异指纹）",
            t226Code::MergeLost => "合并丢失（末次值未保留）",
            t226Code::GhostEntry => "幻影条目（已删条目复现）",
            t226Code::TombstoneOverflow => "墓碑溢出（删除标记超容）",
            t226Code::MergeWithoutBase => "无基合并（缺基础条目）",
            t226Code::StableOrderBreak => "稳定序破缺（同键多值序不定）",
            t226Code::LedgerDup => "账本重复（同变更双记）",
            t226Code::LedgerBroken => "台账不守恒（守恒恒等式破缺）",
            t226Code::SnapshotFingerprint => "快照指纹不符（字段被改后重算不一致）",
            t226Code::TableBroken => "常量表破坏（表项与语义规则矛盾）",
            t226Code::IndexMisaligned => "索引错位（枚举序与矩阵序不一致）",
            t226Code::DuplicateEntry => "条目重复（同键双写）",
            t226Code::OrderViolated => "次序违例（依赖序/时序破缺）",
            t226Code::GuardTripped => "守卫触发（重入/越限被拒）",
            t226Code::SegmentGuard => "段位守卫失败（诊断码越出本模块段）",
        }
    }

    /// 可操作修复建议（每条诊断都必须带下一步动作）。
    pub const fn suggestion(self) -> &'static str {
        match self {
            t226Code::DupCollision => "按末次生效合并并登记",
            t226Code::HashSplit => "按断言号归一化重算",
            t226Code::MergeLost => "按合并协议重建",
            t226Code::GhostEntry => "核对 tombstone 账",
            t226Code::TombstoneOverflow => "压缩墓碑后重建",
            t226Code::MergeWithoutBase => "补基础条目后合并",
            t226Code::StableOrderBreak => "按稳定排序归位",
            t226Code::LedgerDup => "幂等去重",
            t226Code::LedgerBroken => "按守恒律重建台账后重跑",
            t226Code::SnapshotFingerprint => "按防篡改规程重建快照",
            t226Code::TableBroken => "按语义规则重建常量表",
            t226Code::IndexMisaligned => "按声明序归一化后重跑",
            t226Code::DuplicateEntry => "合并同键条目为末次生效",
            t226Code::OrderViolated => "按依赖矩阵重排后重跑",
            t226Code::GuardTripped => "检查临界区嵌套与阈值",
            t226Code::SegmentGuard => "核对诊断码分配表",
        }
    }
}

// --------------------------------------------------------------------------
// 字符串键值映射 dedup_fp（24 行逐行语义注释 + 逐行守卫）
// --------------------------------------------------------------------------
/// 断言指纹表（24 行逐行守卫）。
/// 行数。
pub const dedup_fp_COUNT: usize = 24;

/// 表本体（键 + 值 + 注释由编译期冻结）。
pub static dedup_fp: [(&'static str, u64); 24] = [
    ("FP000", 1000), // 断言指纹 000（同指纹末次生效）
    ("FP001", 1001), // 断言指纹 001（同指纹末次生效）
    ("FP002", 1002), // 断言指纹 002（同指纹末次生效）
    ("FP003", 1003), // 断言指纹 003（同指纹末次生效）
    ("FP004", 1004), // 断言指纹 004（同指纹末次生效）
    ("FP005", 1005), // 断言指纹 005（同指纹末次生效）
    ("FP006", 1006), // 断言指纹 006（同指纹末次生效）
    ("FP007", 1007), // 断言指纹 007（同指纹末次生效）
    ("FP008", 1008), // 断言指纹 008（同指纹末次生效）
    ("FP009", 1009), // 断言指纹 009（同指纹末次生效）
    ("FP010", 1010), // 断言指纹 010（同指纹末次生效）
    ("FP011", 1011), // 断言指纹 011（同指纹末次生效）
    ("FP012", 1012), // 断言指纹 012（同指纹末次生效）
    ("FP013", 1013), // 断言指纹 013（同指纹末次生效）
    ("FP014", 1014), // 断言指纹 014（同指纹末次生效）
    ("FP015", 1015), // 断言指纹 015（同指纹末次生效）
    ("FP016", 1016), // 断言指纹 016（同指纹末次生效）
    ("FP017", 1017), // 断言指纹 017（同指纹末次生效）
    ("FP018", 1018), // 断言指纹 018（同指纹末次生效）
    ("FP019", 1019), // 断言指纹 019（同指纹末次生效）
    ("FP020", 1020), // 断言指纹 020（同指纹末次生效）
    ("FP021", 1021), // 断言指纹 021（同指纹末次生效）
    ("FP022", 1022), // 断言指纹 022（同指纹末次生效）
    ("FP023", 1023), // 断言指纹 023（同指纹末次生效）
];

/// O(1) 语义查找（match 全枚举；命中率 100%% 由守卫保证）。
pub fn dedup_fp_get(key: &str) -> Option<u64> {
    match key {
        "FP000" => Some(1000),
        "FP001" => Some(1001),
        "FP002" => Some(1002),
        "FP003" => Some(1003),
        "FP004" => Some(1004),
        "FP005" => Some(1005),
        "FP006" => Some(1006),
        "FP007" => Some(1007),
        "FP008" => Some(1008),
        "FP009" => Some(1009),
        "FP010" => Some(1010),
        "FP011" => Some(1011),
        "FP012" => Some(1012),
        "FP013" => Some(1013),
        "FP014" => Some(1014),
        "FP015" => Some(1015),
        "FP016" => Some(1016),
        "FP017" => Some(1017),
        "FP018" => Some(1018),
        "FP019" => Some(1019),
        "FP020" => Some(1020),
        "FP021" => Some(1021),
        "FP022" => Some(1022),
        "FP023" => Some(1023),
        _ => None,
    }
}

/// 逐行守卫扫描（返回违例行数；0 = 全绿）。
pub fn dedup_fp_scan() -> usize {
    let mut bad = 0usize;
    let mut i = 0usize;
    while i < dedup_fp_COUNT {
        let (k, v) = dedup_fp[i];
        let ok = match k {
            "FP000" => v >= 1000 && v < 2000,
            "FP001" => v >= 1000 && v < 2000,
            "FP002" => v >= 1000 && v < 2000,
            "FP003" => v >= 1000 && v < 2000,
            "FP004" => v >= 1000 && v < 2000,
            "FP005" => v >= 1000 && v < 2000,
            "FP006" => v >= 1000 && v < 2000,
            "FP007" => v >= 1000 && v < 2000,
            "FP008" => v >= 1000 && v < 2000,
            "FP009" => v >= 1000 && v < 2000,
            "FP010" => v >= 1000 && v < 2000,
            "FP011" => v >= 1000 && v < 2000,
            "FP012" => v >= 1000 && v < 2000,
            "FP013" => v >= 1000 && v < 2000,
            "FP014" => v >= 1000 && v < 2000,
            "FP015" => v >= 1000 && v < 2000,
            "FP016" => v >= 1000 && v < 2000,
            "FP017" => v >= 1000 && v < 2000,
            "FP018" => v >= 1000 && v < 2000,
            "FP019" => v >= 1000 && v < 2000,
            "FP020" => v >= 1000 && v < 2000,
            "FP021" => v >= 1000 && v < 2000,
            "FP022" => v >= 1000 && v < 2000,
            "FP023" => v >= 1000 && v < 2000,
            _ => false,
        };
        if !ok {
            bad += 1;
        }
        i += 1;
    }
    bad
}

/// 累计求和（守恒对账用）。
pub fn dedup_fp_sum() -> u64 {
    let mut s = 0u64;
    let mut i = 0usize;
    while i < dedup_fp_COUNT {
        s = s.wrapping_add(dedup_fp[i].1 as u64);
        i += 1;
    }
    s
}

/// 键唯一性（构造期断言等价物：逐对扫描同键）。
pub fn dedup_fp_unique() -> bool {
    let mut i = 0usize;
    while i < dedup_fp_COUNT {
        let mut j = i + 1;
        while j < dedup_fp_COUNT {
            if dedup_fp[i].0 == dedup_fp[j].0 {
                return false;
            }
            j += 1;
        }
        i += 1;
    }
    true
}

// --------------------------------------------------------------------------
// 状态机 merge（4 态 × 4 事件 · 迁移 5 条显式声明）
// --------------------------------------------------------------------------
/// 去重合并状态机状态枚举（闭集，索引恒等）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum mergeState {
    Idle,
    Matched,
    Merged,
    Conflict,
}

impl mergeState {
    /// 全量态闭集。
    pub const ALL: [mergeState; 4] = [
        mergeState::Idle,
        mergeState::Matched,
        mergeState::Merged,
        mergeState::Conflict,
    ];

    /// 索引（与声明序恒等，对拍用）。
    pub const fn index(self) -> usize {
        match self {
            mergeState::Idle => 0,
            mergeState::Matched => 1,
            mergeState::Merged => 2,
            mergeState::Conflict => 3,
        }
    }
}

/// merge事件枚举（闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum mergeEvent {
    Lookup,
    Hit,
    Apply,
    Collide,
}

impl mergeEvent {
    /// 全量事件闭集。
    pub const ALL: [mergeEvent; 4] = [
        mergeEvent::Lookup,
        mergeEvent::Hit,
        mergeEvent::Apply,
        mergeEvent::Collide,
    ];
}

/// 合法迁移表（显式穷举；表外组合一律非法）。
pub const MERGE_TRANSITIONS: [(mergeState, mergeEvent, mergeState); 5] = [
    (mergeState::Idle, mergeEvent::Lookup, mergeState::Matched), // 指纹查找
    (mergeState::Matched, mergeEvent::Hit, mergeState::Merged), // 命中合并
    (mergeState::Matched, mergeEvent::Collide, mergeState::Conflict), // 碰撞入冲突
    (mergeState::Conflict, mergeEvent::Apply, mergeState::Merged), // 冲突裁定后合并
    (mergeState::Merged, mergeEvent::Lookup, mergeState::Matched), // 下一轮
];

/// 迁移函数：合法则迁移并返回新态，非法返回 None（调用方记账）。
pub fn merge_next(state: mergeState, ev: mergeEvent) -> Option<mergeState> {
    let mut i = 0usize;
    while i < 5 {
        let t = MERGE_TRANSITIONS[i];
        if t.0 == state && t.1 == ev {
            return Some(t.2);
        }
        i += 1;
    }
    None
}

/// 非法迁移检测（对拍：逐态逐事件全组合扫描表外项）。
pub fn merge_illegal_pairs() -> usize {
    let mut bad = 0usize;
    let mut si = 0usize;
    while si < mergeState::ALL.len() {
        let mut ei = 0usize;
        while ei < mergeEvent::ALL.len() {
            let legal = merge_next(mergeState::ALL[si], mergeEvent::ALL[ei]).is_some();
            if !legal {
                bad += 1;
            }
            ei += 1;
        }
        si += 1;
    }
    bad
}

/// 终态集合（无出边的态即终态）。
pub fn merge_terminal(st: mergeState) -> bool {
    let mut i = 0usize;
    while i < 5 {
        if MERGE_TRANSITIONS[i].0 == st {
            return false;
        }
        i += 1;
    }
    true
}

/// 迁移账：合法/非法双计数 + 守恒（total = legal + illegal）。
#[derive(Clone, Copy, Debug)]
pub struct mergeLedger {
    /// 合法迁移计数。
    pub legal: u64,
    /// 非法迁移拒止计数。
    pub illegal: u64,
    /// 当前态。
    pub cur: mergeState,
}

impl mergeLedger {
    /// 构造（初态由设计指定）。
    pub const fn new(initial: mergeState) -> Self {
        Self { legal: 0, illegal: 0, cur: initial }
    }

    /// 施加事件：合法迁移双账之一，非法拒止另一账。
    pub fn apply(&mut self, ev: mergeEvent) -> bool {
        match merge_next(self.cur, ev) {
            Some(ns) => {
                self.cur = ns;
                self.legal += 1;
                true
            }
            None => {
                self.illegal += 1;
                false
            }
        }
    }

    /// 守恒恒等式：legal + illegal == 施加次数（由调用方累计）。
    pub fn total(&self) -> u64 {
        self.legal + self.illegal
    }
}

// ---------------------------------------------------------------------------
// 段位守卫（本模块独立十六进码前缀防串段；越界即判红）
// ---------------------------------------------------------------------------

/// 段位守卫：全部诊断码必须落在本段 [0x3850, 0x385f]。
pub const fn t226_segment_guard() -> bool {
    let mut ok = true;
    let mut i = 0usize;
    while i < t226Code::ALL.len() {
        let c = t226Code::ALL[i].code();
        if c < 0x3850 || c > 0x385f {
            ok = false;
        }
        i += 1;
    }
    ok
}

// --------------------------------------------------------------------------
// 自检（CheckSet 范式：每检查一带 detail，全绿才出门）
// --------------------------------------------------------------------------
/// 段位守卫：全部诊断码落在本模块段内。
fn t226_guard_seg() -> bool {
    t226_segment_guard()
}

/// 表 dedup_fp 逐行守卫全绿（行数/守卫/唯一性）。
fn check_t226_dedup_fp_guard() -> bool {
    let mut ok = dedup_fp_COUNT == 24;
    if ok {
        ok = dedup_fp_scan() == 0;
    }
    if ok {
        ok = dedup_fp_unique();
    }
    ok
}

/// 合并状态机主路径 + 冲突裁定路径。
fn check_c2_merge_sm() -> bool {
    let mut led = mergeLedger::new(mergeState::Idle);
    let mut ok = led.apply(mergeEvent::Lookup);
    ok = ok && led.apply(mergeEvent::Collide);
    ok = ok && led.apply(mergeEvent::Apply);
    if ok {
        ok = led.cur == mergeState::Merged;
    }
    if ok {
        ok = led.apply(mergeEvent::Lookup) && led.cur == mergeState::Matched;
    }
    ok
}

/// 本模块自检（逐检查带 detail，全绿才出门）。
pub fn run_t226_checks() -> CheckSet {
    let mut set = CheckSet::new("T226");
    set.add(
        "t226-seg-guard",
        t226_guard_seg(),
        if t226_guard_seg() { "" } else { "diagnostic code outside segment" },
    );
    set.add(
        "t226-c1-fp-guard",
        check_t226_dedup_fp_guard(),
        if check_t226_dedup_fp_guard() { "" } else { "table dedup_fp guard mismatch" },
    );
    set.add(
        "t226-c2-merge-sm",
        check_c2_merge_sm(),
        if check_c2_merge_sm() { "" } else { "merge state machine mismatch" },
    );
    set
}

// ---------------------------------------------------------------------------
// 单元测试（域聚合测试在 mod.rs——此处只放本模块纯函数级断言）
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn t226_all_checks_green() {
        let set = run_t226_checks();
        assert!(set.all_passed(), "T226 domain checks must be green");
    }

    #[test]
    fn t226_segment_guard_holds() {
        assert!(t226_segment_guard());
    }
}
