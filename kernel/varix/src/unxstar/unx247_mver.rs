//! unxt247_mver —— UNX-F0241 版本冻结与收官（manifest 版本台账）。
//! 判据原文（CoRun Unxreal 主册 UNX-A1-B09）：UNX-F0241 · BootManifest 全景结构（十二步总纲） · 判据：十二步节结构定义齐备（固件/内存/CPU/随机化/参数/健康/时间/日志/上次引导/订阅/完整性/时序），总 CRC 链校验过，结构黄金比对一致
//! · 组件：版本台账（schema 版本/结构版本/CRC 版本三账）+ 冻结状态机 + 收官印
//! · 判据落地：结构封版的版本面
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，
//! 逐条定制数据结构/状态机/错误路径，零模板复述，零死代码。


use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const T247_VERSION: &str = "t247-mver-v1";

/// 诊断段基址。
pub const T247_SEG_BASE: u16 = 0x3960;

// --------------------------------------------------------------------------
// 诊断码（t247 十六码 · 16 枚互斥 + segment_guard）
// --------------------------------------------------------------------------
/// 版本冻结诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum t247Code {
    /// 0x3960 版本重复（同号双版）。
    VersionDup,
    /// 0x3961 版本缺失（未登记版次）。
    VersionMissing,
    /// 0x3962 提前冻结（校验未过即冻）。
    FreezeEarly,
    /// 0x3963 冻结后变更（改版被拒）。
    FrozenMutate,
    /// 0x3964 版本回退（降到旧版）。
    RollbackVersion,
    /// 0x3965 版本账破缺（三账不守恒）。
    LedgerBreak,
    /// 0x3966 版本报告陈旧。
    ReportStale,
    /// 0x3967 收官印缺失。
    SealMissing,
    /// 0x3968 台账不守恒（守恒恒等式破缺）。
    LedgerBroken,
    /// 0x3969 快照指纹不符（字段被改后重算不一致）。
    SnapshotFingerprint,
    /// 0x396a 常量表破坏（表项与语义规则矛盾）。
    TableBroken,
    /// 0x396b 索引错位（枚举序与矩阵序不一致）。
    IndexMisaligned,
    /// 0x396c 条目重复（同键双写）。
    DuplicateEntry,
    /// 0x396d 次序违例（依赖序/时序破缺）。
    OrderViolated,
    /// 0x396e 守卫触发（重入/越限被拒）。
    GuardTripped,
    /// 0x396f 段位守卫失败（诊断码越出本模块段）。
    SegmentGuard,
}

impl t247Code {
    /// 全量闭集。
    pub const ALL: [t247Code; 16] = [
        t247Code::VersionDup,
        t247Code::VersionMissing,
        t247Code::FreezeEarly,
        t247Code::FrozenMutate,
        t247Code::RollbackVersion,
        t247Code::LedgerBreak,
        t247Code::ReportStale,
        t247Code::SealMissing,
        t247Code::LedgerBroken,
        t247Code::SnapshotFingerprint,
        t247Code::TableBroken,
        t247Code::IndexMisaligned,
        t247Code::DuplicateEntry,
        t247Code::OrderViolated,
        t247Code::GuardTripped,
        t247Code::SegmentGuard,
    ];

    /// 码值（连续十六进制，与声明序恒等）。
    pub const fn code(self) -> u16 {
        match self {
            t247Code::VersionDup => 0x3960,
            t247Code::VersionMissing => 0x3961,
            t247Code::FreezeEarly => 0x3962,
            t247Code::FrozenMutate => 0x3963,
            t247Code::RollbackVersion => 0x3964,
            t247Code::LedgerBreak => 0x3965,
            t247Code::ReportStale => 0x3966,
            t247Code::SealMissing => 0x3967,
            t247Code::LedgerBroken => 0x3968,
            t247Code::SnapshotFingerprint => 0x3969,
            t247Code::TableBroken => 0x396a,
            t247Code::IndexMisaligned => 0x396b,
            t247Code::DuplicateEntry => 0x396c,
            t247Code::OrderViolated => 0x396d,
            t247Code::GuardTripped => 0x396e,
            t247Code::SegmentGuard => 0x396f,
        }
    }

    /// 中文诊断名。
    pub const fn name(self) -> &'static str {
        match self {
            t247Code::VersionDup => "版本重复（同号双版）",
            t247Code::VersionMissing => "版本缺失（未登记版次）",
            t247Code::FreezeEarly => "提前冻结（校验未过即冻）",
            t247Code::FrozenMutate => "冻结后变更（改版被拒）",
            t247Code::RollbackVersion => "版本回退（降到旧版）",
            t247Code::LedgerBreak => "版本账破缺（三账不守恒）",
            t247Code::ReportStale => "版本报告陈旧",
            t247Code::SealMissing => "收官印缺失",
            t247Code::LedgerBroken => "台账不守恒（守恒恒等式破缺）",
            t247Code::SnapshotFingerprint => "快照指纹不符（字段被改后重算不一致）",
            t247Code::TableBroken => "常量表破坏（表项与语义规则矛盾）",
            t247Code::IndexMisaligned => "索引错位（枚举序与矩阵序不一致）",
            t247Code::DuplicateEntry => "条目重复（同键双写）",
            t247Code::OrderViolated => "次序违例（依赖序/时序破缺）",
            t247Code::GuardTripped => "守卫触发（重入/越限被拒）",
            t247Code::SegmentGuard => "段位守卫失败（诊断码越出本模块段）",
        }
    }

    /// 可操作修复建议（每条诊断都必须带下一步动作）。
    pub const fn suggestion(self) -> &'static str {
        match self {
            t247Code::VersionDup => "按末次生效合并",
            t247Code::VersionMissing => "补登记后入账",
            t247Code::FreezeEarly => "按冻结前置条件重排",
            t247Code::FrozenMutate => "按只读规程拒绝",
            t247Code::RollbackVersion => "按回退协议登记",
            t247Code::LedgerBreak => "按守恒重建",
            t247Code::ReportStale => "重跑后刷新",
            t247Code::SealMissing => "补收官印",
            t247Code::LedgerBroken => "按守恒律重建台账后重跑",
            t247Code::SnapshotFingerprint => "按防篡改规程重建快照",
            t247Code::TableBroken => "按语义规则重建常量表",
            t247Code::IndexMisaligned => "按声明序归一化后重跑",
            t247Code::DuplicateEntry => "合并同键条目为末次生效",
            t247Code::OrderViolated => "按依赖矩阵重排后重跑",
            t247Code::GuardTripped => "检查临界区嵌套与阈值",
            t247Code::SegmentGuard => "核对诊断码分配表",
        }
    }
}

// --------------------------------------------------------------------------
// 字符串键值映射 mver（4 行逐行语义注释 + 逐行守卫）
// --------------------------------------------------------------------------
/// manifest 版本台账（四版本逐行守卫）。
/// 行数。
pub const mver_COUNT: usize = 4;

/// 表本体（键 + 值 + 注释由编译期冻结）。
pub static mver: [(&'static str, u64); 4] = [
    ("VER_SCHEMA", 1), // schema 版本（十二步结构定义版本）
    ("VER_STRUCT", 1), // 结构版本（步字段矩阵版本）
    ("VER_CRC", 1), // CRC 链版本（链算法版本）
    ("VER_SEAL", 1), // 封印版本（封印算法版本）
];

/// O(1) 语义查找（match 全枚举；命中率 100%% 由守卫保证）。
pub fn mver_get(key: &str) -> Option<u64> {
    match key {
        "VER_SCHEMA" => Some(1),
        "VER_STRUCT" => Some(1),
        "VER_CRC" => Some(1),
        "VER_SEAL" => Some(1),
        _ => None,
    }
}

/// 逐行守卫扫描（返回违例行数；0 = 全绿）。
pub fn mver_scan() -> usize {
    let mut bad = 0usize;
    let mut i = 0usize;
    while i < mver_COUNT {
        let (k, v) = mver[i];
        let ok = match k {
            "VER_SCHEMA" => v >= 1,
            "VER_STRUCT" => v >= 1,
            "VER_CRC" => v >= 1,
            "VER_SEAL" => v >= 1,
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
pub fn mver_sum() -> u64 {
    let mut s = 0u64;
    let mut i = 0usize;
    while i < mver_COUNT {
        s = s.wrapping_add(mver[i].1 as u64);
        i += 1;
    }
    s
}

/// 键唯一性（构造期断言等价物：逐对扫描同键）。
pub fn mver_unique() -> bool {
    let mut i = 0usize;
    while i < mver_COUNT {
        let mut j = i + 1;
        while j < mver_COUNT {
            if mver[i].0 == mver[j].0 {
                return false;
            }
            j += 1;
        }
        i += 1;
    }
    true
}

// --------------------------------------------------------------------------
// 状态机 frz（3 态 × 3 事件 · 迁移 4 条显式声明）
// --------------------------------------------------------------------------
/// 版本冻结状态机状态枚举（闭集，索引恒等）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum frzState {
    Mutable,
    Frozen,
    Sealed,
}

impl frzState {
    /// 全量态闭集。
    pub const ALL: [frzState; 3] = [
        frzState::Mutable,
        frzState::Frozen,
        frzState::Sealed,
    ];

    /// 索引（与声明序恒等，对拍用）。
    pub const fn index(self) -> usize {
        match self {
            frzState::Mutable => 0,
            frzState::Frozen => 1,
            frzState::Sealed => 2,
        }
    }
}

/// frz事件枚举（闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum frzEvent {
    Freeze,
    Seal,
    Mutate,
}

impl frzEvent {
    /// 全量事件闭集。
    pub const ALL: [frzEvent; 3] = [
        frzEvent::Freeze,
        frzEvent::Seal,
        frzEvent::Mutate,
    ];
}

/// 合法迁移表（显式穷举；表外组合一律非法）。
pub const FRZ_TRANSITIONS: [(frzState, frzEvent, frzState); 4] = [
    (frzState::Mutable, frzEvent::Freeze, frzState::Frozen), // 冻结
    (frzState::Frozen, frzEvent::Seal, frzState::Sealed), // 封版
    (frzState::Frozen, frzEvent::Mutate, frzState::Frozen), // 拒绝变更保持
    (frzState::Sealed, frzEvent::Mutate, frzState::Sealed), // 拒绝变更保持
];

/// 迁移函数：合法则迁移并返回新态，非法返回 None（调用方记账）。
pub fn frz_next(state: frzState, ev: frzEvent) -> Option<frzState> {
    let mut i = 0usize;
    while i < 4 {
        let t = FRZ_TRANSITIONS[i];
        if t.0 == state && t.1 == ev {
            return Some(t.2);
        }
        i += 1;
    }
    None
}

/// 非法迁移检测（对拍：逐态逐事件全组合扫描表外项）。
pub fn frz_illegal_pairs() -> usize {
    let mut bad = 0usize;
    let mut si = 0usize;
    while si < frzState::ALL.len() {
        let mut ei = 0usize;
        while ei < frzEvent::ALL.len() {
            let legal = frz_next(frzState::ALL[si], frzEvent::ALL[ei]).is_some();
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
pub fn frz_terminal(st: frzState) -> bool {
    let mut i = 0usize;
    while i < 4 {
        if FRZ_TRANSITIONS[i].0 == st {
            return false;
        }
        i += 1;
    }
    true
}

/// 迁移账：合法/非法双计数 + 守恒（total = legal + illegal）。
#[derive(Clone, Copy, Debug)]
pub struct frzLedger {
    /// 合法迁移计数。
    pub legal: u64,
    /// 非法迁移拒止计数。
    pub illegal: u64,
    /// 当前态。
    pub cur: frzState,
}

impl frzLedger {
    /// 构造（初态由设计指定）。
    pub const fn new(initial: frzState) -> Self {
        Self { legal: 0, illegal: 0, cur: initial }
    }

    /// 施加事件：合法迁移双账之一，非法拒止另一账。
    pub fn apply(&mut self, ev: frzEvent) -> bool {
        match frz_next(self.cur, ev) {
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

/// 段位守卫：全部诊断码必须落在本段 [0x3960, 0x396f]。
pub const fn t247_segment_guard() -> bool {
    let mut ok = true;
    let mut i = 0usize;
    while i < t247Code::ALL.len() {
        let c = t247Code::ALL[i].code();
        if c < 0x3960 || c > 0x396f {
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
fn t247_guard_seg() -> bool {
    t247_segment_guard()
}

/// 表 mver 逐行守卫全绿（行数/守卫/唯一性）。
fn check_t247_mver_guard() -> bool {
    let mut ok = mver_COUNT == 4;
    if ok {
        ok = mver_scan() == 0;
    }
    if ok {
        ok = mver_unique();
    }
    ok
}

/// 冻结状态机（冻结-封版 + 冻结期变更保持）。
fn check_c2_freeze_sm() -> bool {
    let mut led = frzLedger::new(frzState::Mutable);
    let mut ok = led.apply(frzEvent::Freeze);
    ok = ok && led.apply(frzEvent::Seal);
    if ok {
        ok = led.cur == frzState::Sealed;
    }
    if ok {
        // 封版后变更按保持型合法迁移处理（仍驻 Sealed）。
        ok = led.apply(frzEvent::Mutate) && led.cur == frzState::Sealed;
    }
    if ok {
        // 已封版不可再冻结（非法迁移拒止）。
        ok = !led.apply(frzEvent::Freeze) && led.illegal == 1;
    }
    ok
}

/// 本模块自检（逐检查带 detail，全绿才出门）。
pub fn run_t247_checks() -> CheckSet {
    let mut set = CheckSet::new("T247");
    set.add(
        "t247-seg-guard",
        t247_guard_seg(),
        if t247_guard_seg() { "" } else { "diagnostic code outside segment" },
    );
    set.add(
        "t247-c1-ver-guard",
        check_t247_mver_guard(),
        if check_t247_mver_guard() { "" } else { "table mver guard mismatch" },
    );
    set.add(
        "t247-c2-freeze-sm",
        check_c2_freeze_sm(),
        if check_c2_freeze_sm() { "" } else { "freeze state machine mismatch" },
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
    fn t247_all_checks_green() {
        let set = run_t247_checks();
        assert!(set.all_passed(), "T247 domain checks must be green");
    }

    #[test]
    fn t247_segment_guard_holds() {
        assert!(t247_segment_guard());
    }
}
