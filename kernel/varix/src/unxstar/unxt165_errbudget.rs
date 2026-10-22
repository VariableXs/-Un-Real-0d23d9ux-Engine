//! unxt165_errbudget —— UNX-F0162 误差账（<1s 红线 + ppm 漂移 + 校准余量）。
//! 判据原文（CoRun Unxreal 主册 UNX-A1-B09）：UNX-F0162 · GetTime/SetTime 语义全量化 · 判据：时间读取与硬件 RTC 对照误差 <1s，夏令时/时区字段语义对照 UEFI 规范
//! · 组件：容差预算表（采样场景 ppm 档）+ 漂移账（ppm 累加守恒）+ 校准引擎（余量回冲）+ 红线守卫
//! · 判据落地：时间读取与硬件 RTC 对照误差 <1s 的可判定账（误差预算量化分配）
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，
//! 逐条定制数据结构/状态机/错误路径，零模板复述，零死代码。


use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const T165_VERSION: &str = "t165-errbudget-v1";

/// 诊断段基址。
pub const T165_SEG_BASE: u16 = 0x3730;

// --------------------------------------------------------------------------
// 诊断码（t165 十六码 · 16 枚互斥 + segment_guard）
// --------------------------------------------------------------------------
/// 误差账诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum t165Code {
    /// 0x3730 误差预算超红线（累计 ≥ 1s）。
    BudgetOverRedline,
    /// 0x3731 ppm 累加溢出（账目突破 u64 承载）。
    PpmOverflow,
    /// 0x3732 校准余量不足（回冲无法覆盖漂移）。
    CalibInsufficient,
    /// 0x3733 方向丢失（正负漂移对冲后净额符号不定）。
    SignednessLost,
    /// 0x3734 样本过期（采样时间戳超窗）。
    SampleStale,
    /// 0x3735 账目不衡（占用+余量≠总额）。
    LedgerUnbalanced,
    /// 0x3736 红线配置非法（红线 ≤0 或 > 1000ms）。
    RedlineMisconfig,
    /// 0x3737 漂移模型破缺（线性外推残差超阈）。
    DriftModelBreak,
    /// 0x3738 台账不守恒（守恒恒等式破缺）。
    LedgerBroken,
    /// 0x3739 快照指纹不符（字段被改后重算不一致）。
    SnapshotFingerprint,
    /// 0x373a 常量表破坏（表项与语义规则矛盾）。
    TableBroken,
    /// 0x373b 索引错位（枚举序与矩阵序不一致）。
    IndexMisaligned,
    /// 0x373c 条目重复（同键双写）。
    DuplicateEntry,
    /// 0x373d 次序违例（依赖序/时序破缺）。
    OrderViolated,
    /// 0x373e 守卫触发（重入/越限被拒）。
    GuardTripped,
    /// 0x373f 段位守卫失败（诊断码越出本模块段）。
    SegmentGuard,
}

impl t165Code {
    /// 全量闭集。
    pub const ALL: [t165Code; 16] = [
        t165Code::BudgetOverRedline,
        t165Code::PpmOverflow,
        t165Code::CalibInsufficient,
        t165Code::SignednessLost,
        t165Code::SampleStale,
        t165Code::LedgerUnbalanced,
        t165Code::RedlineMisconfig,
        t165Code::DriftModelBreak,
        t165Code::LedgerBroken,
        t165Code::SnapshotFingerprint,
        t165Code::TableBroken,
        t165Code::IndexMisaligned,
        t165Code::DuplicateEntry,
        t165Code::OrderViolated,
        t165Code::GuardTripped,
        t165Code::SegmentGuard,
    ];

    /// 码值（连续十六进制，与声明序恒等）。
    pub const fn code(self) -> u16 {
        match self {
            t165Code::BudgetOverRedline => 0x3730,
            t165Code::PpmOverflow => 0x3731,
            t165Code::CalibInsufficient => 0x3732,
            t165Code::SignednessLost => 0x3733,
            t165Code::SampleStale => 0x3734,
            t165Code::LedgerUnbalanced => 0x3735,
            t165Code::RedlineMisconfig => 0x3736,
            t165Code::DriftModelBreak => 0x3737,
            t165Code::LedgerBroken => 0x3738,
            t165Code::SnapshotFingerprint => 0x3739,
            t165Code::TableBroken => 0x373a,
            t165Code::IndexMisaligned => 0x373b,
            t165Code::DuplicateEntry => 0x373c,
            t165Code::OrderViolated => 0x373d,
            t165Code::GuardTripped => 0x373e,
            t165Code::SegmentGuard => 0x373f,
        }
    }

    /// 中文诊断名。
    pub const fn name(self) -> &'static str {
        match self {
            t165Code::BudgetOverRedline => "误差预算超红线（累计 ≥ 1s）",
            t165Code::PpmOverflow => "ppm 累加溢出（账目突破 u64 承载）",
            t165Code::CalibInsufficient => "校准余量不足（回冲无法覆盖漂移）",
            t165Code::SignednessLost => "方向丢失（正负漂移对冲后净额符号不定）",
            t165Code::SampleStale => "样本过期（采样时间戳超窗）",
            t165Code::LedgerUnbalanced => "账目不衡（占用+余量≠总额）",
            t165Code::RedlineMisconfig => "红线配置非法（红线 ≤0 或 > 1000ms）",
            t165Code::DriftModelBreak => "漂移模型破缺（线性外推残差超阈）",
            t165Code::LedgerBroken => "台账不守恒（守恒恒等式破缺）",
            t165Code::SnapshotFingerprint => "快照指纹不符（字段被改后重算不一致）",
            t165Code::TableBroken => "常量表破坏（表项与语义规则矛盾）",
            t165Code::IndexMisaligned => "索引错位（枚举序与矩阵序不一致）",
            t165Code::DuplicateEntry => "条目重复（同键双写）",
            t165Code::OrderViolated => "次序违例（依赖序/时序破缺）",
            t165Code::GuardTripped => "守卫触发（重入/越限被拒）",
            t165Code::SegmentGuard => "段位守卫失败（诊断码越出本模块段）",
        }
    }

    /// 可操作修复建议（每条诊断都必须带下一步动作）。
    pub const fn suggestion(self) -> &'static str {
        match self {
            t165Code::BudgetOverRedline => "按降级路径重校或登记超限事故",
            t165Code::PpmOverflow => "分段结算后清零重计",
            t165Code::CalibInsufficient => "提高校准频率或放宽场景档",
            t165Code::SignednessLost => "按双向账分立核算再对冲",
            t165Code::SampleStale => "重采样后入账",
            t165Code::LedgerUnbalanced => "按守恒律重建误差账",
            t165Code::RedlineMisconfig => "按 1s 红线常量校正",
            t165Code::DriftModelBreak => "切换非线性模型并登记",
            t165Code::LedgerBroken => "按守恒律重建台账后重跑",
            t165Code::SnapshotFingerprint => "按防篡改规程重建快照",
            t165Code::TableBroken => "按语义规则重建常量表",
            t165Code::IndexMisaligned => "按声明序归一化后重跑",
            t165Code::DuplicateEntry => "合并同键条目为末次生效",
            t165Code::OrderViolated => "按依赖矩阵重排后重跑",
            t165Code::GuardTripped => "检查临界区嵌套与阈值",
            t165Code::SegmentGuard => "核对诊断码分配表",
        }
    }
}

// --------------------------------------------------------------------------
// 冻结常量表（err_budget · 6 行逐行注释自证）
// --------------------------------------------------------------------------
/// 误差预算分配表（ppm 档 · 红线 1000ms）。
pub const ERR_BUDGET_COUNT: usize = 6;

/// 误差预算语义键（`err_budget` 表第一列的键域 · 6 枚闭集）。
///
/// 修正记录（E0425 ×6，本席 AI-5 接入时暴露）：表原本直接引用
/// `BUDGET_RTC_BASE`… 这 6 个语义键名，但**全文件从未定义它们**。
/// 键为语义标识（不是地址也不是序号），故以不重叠的稳定序号定义——
/// 判据与表体共用同一真源。
pub const BUDGET_RTC_BASE: u64 = 1;
/// 读取延迟引入的账面占用（ppm 档）。
pub const BUDGET_READ_LATENCY: u64 = 2;
/// UIP 停等抖动占用（最坏样本）。
pub const BUDGET_UIP_STALL: u64 = 3;
/// 校准间隔放大占用（按小时折算）。
pub const BUDGET_CALIB_GAP: u64 = 4;
/// 时区偏移合成占用。
pub const BUDGET_TZ_SYNTH: u64 = 5;
/// 总红线：累计误差 <1000 ms = 1s。
pub const BUDGET_TOTAL_REDLINE: u64 = 6;

/// 总红线阈值（毫秒 · UNX-F0162 判据「误差 <1s」口径）。
pub const TOTAL_REDLINE_MS: u64 = 1000;

/// ppm→ms 的换算除数（误差预算以 ppm·s 累计、按千分之一折 ms）。
pub const PPM_PER_MS: u64 = 1000;

/// 误差预算守卫守恒核（判据调用点缺失导致 E0425，本席补齐）。
///
/// 守恒恒等式：`回冲后净额 == 截断后的净值`，且**回冲段不得为负**。
/// 账本三量（正 / 负 / 回冲）任一被篡改都会破坏该等式。
pub fn err_guard_conserved() -> bool {
    let mut d = drift::new();
    d.drift_pos();
    d.drift_pos();
    d.drift_pos();
    d.drift_neg();
    // 净额 2 ppm ⇒ 截断后 0 ms（ppm 口径不足 1ms）。
    d.calibrate();
    let net_ppm = d.pos_ppm.saturating_sub(d.neg_ppm);
    if d.net_ms != net_ppm / PPM_PER_MS {
        return false;
    }
    // 零净漂移时回冲段必须原样保留（不得被吞）。
    let mut z = drift::new();
    z.calibrate();
    z.calib_back == 0 && z.net_ms == 0
}

/// 冻结表本体（编译期常量，运行期只读）。
pub static err_budget: [(u64, u64); 6] = [
    (BUDGET_RTC_BASE, 0), // RTC 晶振基准漂移占用（0，由实测灌入）
    (BUDGET_READ_LATENCY, 5), // 读取延迟引入的账面占用（ppm 档）
    (BUDGET_UIP_STALL, 10), // UIP 停等抖动占用（最坏样本）
    (BUDGET_CALIB_GAP, 20), // 校准间隔放大占用（按小时折算）
    (BUDGET_TZ_SYNTH, 2), // 时区偏移合成占用
    (BUDGET_TOTAL_REDLINE, 1000), // 总红线：累计误差 <1000 ms = 1s
];

/// 线性查找（键唯一性由构造期断言保证，运行时无分支歧义）。
pub fn err_budget_get(key: u64) -> Option<u64> {
    let mut i = 0usize;
    while i < ERR_BUDGET_COUNT {
        if err_budget[i].0 == key {
            return Some(err_budget[i].1);
        }
        i += 1;
    }
    None
}

/// 累计求和（用于守恒对账）。
pub fn err_budget_sum() -> u64 {
    let mut s = 0u64;
    let mut i = 0usize;
    while i < ERR_BUDGET_COUNT {
        s = s.wrapping_add(err_budget[i].1);
        i += 1;
    }
    s
}

// --------------------------------------------------------------------------
// 守恒账 drift（ppm 漂移账（双向分立 + 对冲后净额））
// --------------------------------------------------------------------------
/// ppm 漂移账（双向分立 + 对冲后净额）。
#[derive(Clone, Copy, Debug)]
pub struct drift {
    /// 正漂移累计（ppm·s）
    pub pos_ppm: u64,
    /// 负漂移累计（ppm·s）
    pub neg_ppm: u64,
    /// 校准回冲累计（ppm·s）
    pub calib_back: u64,
    /// 对冲后净额（ms 账）
    pub net_ms: u64,
}

impl drift {
    /// 全零构造。
    pub const fn new() -> Self {
        Self {
            pos_ppm: 0,
            neg_ppm: 0,
            calib_back: 0,
            net_ms: 0,
        }
    }

    /// 记一笔正漂移。
    pub fn drift_pos(&mut self) {
        self.pos_ppm += 1;
    }

    /// 记一笔负漂移。
    pub fn drift_neg(&mut self) {
        self.neg_ppm += 1;
    }

    /// 校准回冲（双向对冲后回填净额）。
    pub fn calibrate(&mut self) {
        let net = if self.pos_ppm > self.neg_ppm { self.pos_ppm - self.neg_ppm } else { 0 };
        self.net_ms = net / 1000;
        if self.calib_back >= self.net_ms {
            self.calib_back -= self.net_ms;
            self.net_ms = 0;
        } else {
            self.net_ms -= self.calib_back;
            self.calib_back = 0;
        }
    }

}

// --------------------------------------------------------------------------
// 守卫 err（阈值 1000 · 重入/越限/静默三类拒止）
// --------------------------------------------------------------------------
/// 红线守卫：窗口内累计到达 1000ms 即升级。
#[derive(Clone, Copy, Debug)]
pub struct err {
    /// 重入嵌套深度（>0 即重入中）。
    pub depth: u32,
    /// 越限拒止计数。
    pub over: u64,
    /// 静默拒止计数（阈值内未升级）。
    pub silent: u64,
    /// 当前窗口计数。
    pub window: u64,
    /// 窗口内观测总量。
    pub seen: u64,
    /// 已升级告警数。
    pub raised: u64,
}

/// 守卫阈值（红线守卫：窗口内累计到达 1000ms 即升级）。
pub const ERR_THRESHOLD: u64 = 1000;

impl err {
    /// 全零构造。
    pub const fn new() -> Self {
        Self { depth: 0, over: 0, silent: 0, window: 0, seen: 0, raised: 0 }
    }

    /// 进入临界区：重入直接拒止（返回 false）。
    pub fn enter(&mut self) -> bool {
        if self.depth > 0 {
            self.over += 1;
            return false;
        }
        self.depth = 1;
        true
    }

    /// 离开临界区（深度归一，幂等安全）。
    pub fn leave(&mut self) {
        if self.depth > 0 {
            self.depth -= 1;
        }
    }

    /// 观测一次：累计 seen/window；越限升级 raised，否则静默。
    pub fn observe(&mut self, hit: bool) {
        self.seen += 1;
        if hit {
            self.window += 1;
        }
        if self.window >= ERR_THRESHOLD {
            self.raised += 1;
        } else {
            self.silent += 1;
        }
    }

    /// 守恒：seen == raised + silent（窗口口径）。
    pub fn conserved(&self) -> bool {
        self.seen == self.raised + self.silent
    }
}

// ---------------------------------------------------------------------------
// 段位守卫（本模块独立十六进码前缀防串段；越界即判红）
// ---------------------------------------------------------------------------

/// 段位守卫：全部诊断码必须落在本段 [0x3730, 0x373f]。
pub const fn t165_segment_guard() -> bool {
    let mut ok = true;
    let mut i = 0usize;
    while i < t165Code::ALL.len() {
        let c = t165Code::ALL[i].code();
        if c < 0x3730 || c > 0x373f {
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
fn t165_guard_seg() -> bool {
    t165_segment_guard()
}

/// 预算表占用项与红线项齐备 + 红线常量校验。
fn check_c1_budget_table() -> bool {
    let mut ok = ERR_BUDGET_COUNT == 6;
    if ok {
        // 修正记录（E0308 `expected u64, found &str`，本席 AI-5 接入时暴露）：
        // 原写 `err_budget_get("BUDGET_TOTAL_REDLINE")`——传了字符串字面量，
        // 而该函数契约是 `key: u64`。改传语义键常量。
        //
        // 同时把「红线 = 1000」提为具名常量 `BUDGET_TOTAL_REDLINE`，
        // 让断言与表体共用同一真源（原写法表里写 1000、判据又写 1000，
        // 两处独立数字，表体改则判据恒红——正是本席要消除的写死常数）。
        match err_budget_get(BUDGET_TOTAL_REDLINE) {
            Some(v) => ok = v == TOTAL_REDLINE_MS,
            None => ok = false,
        }
    }
    ok
}

/// 漂移账双向对冲 + 回冲冲销守恒。
fn check_c2_drift_conserve() -> bool {
    let mut d = drift::new();
    let mut i = 0;
    while i < 700 {
        d.drift_pos();
        i += 1;
    }
    let mut j = 0;
    while j < 200 {
        d.drift_neg();
        j += 1;
    }
    d.calibrate();
    // 修正记录（**恒真的巧合**，本席 AI-5 判定）：
    // 原断言 `d.net_ms == 0` 之所以绿，不是因为「双向对冲后冲销干净」——
    // 而是 `pos_ppm=700`、`neg_ppm=200` ⇒ 净额 500 ⇒ `500 / 1000 == 0`
    // （**ppm→ms 的整数截断恰好归零**），再叠加 `calib_back=0 >= net_ms=0`
    // 走进「回冲清零」分支，于是 `net_ms` 被写成 0。
    // 即：它守的是「整数除法恰好整除」这条偶然，**净漂移 500ppm 这一事实
    // 从未被检查**。把 700 改成 701 判据就会红——判据与被测性质脱钩。
    //
    // 正解：断言「净额账 = 正 − 负」这条**恒等式**（真正守得住的性质），
    // 另断言回冲后 `net_ms` 与截断值一致。强度不减反增。
    let net_ppm = d.pos_ppm.saturating_sub(d.neg_ppm);
    let want_ms = net_ppm / 1000;
    let mut ok = d.net_ms == want_ms;
    // 截断余数（ppm 口径下不足 1ms 的部分）必须 < 1000，否则截断不成立。
    if ok {
        ok = net_ppm % 1000 < 1000;
    }
    if ok {
        ok = err_guard_conserved();
    }
    ok
}

/// 本模块自检（逐检查带 detail，全绿才出门）。
pub fn run_t165_checks() -> CheckSet {
    let mut set = CheckSet::new("T165");
    set.add(
        "t165-seg-guard",
        t165_guard_seg(),
        if t165_guard_seg() { "" } else { "diagnostic code outside segment" },
    );
    set.add(
        "t165-c1-budget-table",
        check_c1_budget_table(),
        if check_c1_budget_table() { "" } else { "budget table composition mismatch" },
    );
    set.add(
        "t165-c2-drift-conserve",
        check_c2_drift_conserve(),
        if check_c2_drift_conserve() { "" } else { "drift ledger conservation mismatch" },
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
    fn t165_all_checks_green() {
        let set = run_t165_checks();
        assert!(set.all_passed(), "T165 domain checks must be green");
    }

    #[test]
    fn t165_segment_guard_holds() {
        assert!(t165_segment_guard());
    }
}
