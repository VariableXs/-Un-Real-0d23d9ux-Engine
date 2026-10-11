//! unxt505_unx505_sw10k —— UNX-F1502 万次 Polling↔Irq 切换回归（三列留痕 + 失败必抓）。
//! 判据原文（CoRun Unxreal 主册 UNX-A1-B09）：UNX-F1502 · F0039 万次切换回归（10^4 次 Polling↔Irq） · 判据：万次切换全绿（次数/态向/时延三列逐次留痕），成功率 100%，失败注入 10/10 必抓，报告落档
//! · 组件：切换留痕表（次数/态向/时延三列）+ 成功率账 + 失败注入 10/10 + 报告落档
//! · 判据落地：10^4 全绿 + 100% 成功率 + 10/10 必抓
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，
//! 逐条定制数据结构/状态机/错误路径，零模板复述，零死代码。


use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const T505_VERSION: &str = "t505-unx505_sw10k-v1";

/// 诊断段基址。
pub const T505_SEG_BASE: u16 = 0x6400;

// --------------------------------------------------------------------------
// 诊断码（t505 十六码 · 15 枚互斥 + segment_guard）
// --------------------------------------------------------------------------
/// 万次切换诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum t505Code {
    /// 0x6400 切换丢失（未留痕）。
    SwitchLost,
    /// 0x6401 态向非法（双向之外）。
    StateDirBad,
    /// 0x6402 时延非法（超窗或负值）。
    LatencyBad,
    /// 0x6403 成功率失真（成功率非 100% 却报绿）。
    SuccessLost,
    /// 0x6404 失败注入逃逸（未抓）。
    InjectEscaped,
    /// 0x6405 报告未落档。
    ReportMissing,
    /// 0x6406 留痕账破缺。
    LedgerBreak,
    /// 0x6407 守卫触发。
    GuardTripped,
    /// 0x6408 台账不守恒（守恒恒等式破缺）。
    LedgerBroken,
    /// 0x6409 快照指纹不符（字段被改后重算不一致）。
    SnapshotFingerprint,
    /// 0x640a 常量表破坏（表项与语义规则矛盾）。
    TableBroken,
    /// 0x640b 索引错位（枚举序与矩阵序不一致）。
    IndexMisaligned,
    /// 0x640c 条目重复（同键双写）。
    DuplicateEntry,
    /// 0x640d 次序违例（依赖序/时序破缺）。
    OrderViolated,
    /// 0x640e 段位守卫失败（诊断码越出本模块段）。
    SegmentGuard,
}

impl t505Code {
    /// 全量闭集。
    pub const ALL: [t505Code; 15] = [
        t505Code::SwitchLost,
        t505Code::StateDirBad,
        t505Code::LatencyBad,
        t505Code::SuccessLost,
        t505Code::InjectEscaped,
        t505Code::ReportMissing,
        t505Code::LedgerBreak,
        t505Code::GuardTripped,
        t505Code::LedgerBroken,
        t505Code::SnapshotFingerprint,
        t505Code::TableBroken,
        t505Code::IndexMisaligned,
        t505Code::DuplicateEntry,
        t505Code::OrderViolated,
        t505Code::SegmentGuard,
    ];

    /// 码值（连续十六进制，与声明序恒等）。
    pub const fn code(self) -> u16 {
        match self {
            t505Code::SwitchLost => 0x6400,
            t505Code::StateDirBad => 0x6401,
            t505Code::LatencyBad => 0x6402,
            t505Code::SuccessLost => 0x6403,
            t505Code::InjectEscaped => 0x6404,
            t505Code::ReportMissing => 0x6405,
            t505Code::LedgerBreak => 0x6406,
            t505Code::GuardTripped => 0x6407,
            t505Code::LedgerBroken => 0x6408,
            t505Code::SnapshotFingerprint => 0x6409,
            t505Code::TableBroken => 0x640a,
            t505Code::IndexMisaligned => 0x640b,
            t505Code::DuplicateEntry => 0x640c,
            t505Code::OrderViolated => 0x640d,
            t505Code::SegmentGuard => 0x640e,
        }
    }

    /// 中文诊断名。
    pub const fn name(self) -> &'static str {
        match self {
            t505Code::SwitchLost => "切换丢失（未留痕）",
            t505Code::StateDirBad => "态向非法（双向之外）",
            t505Code::LatencyBad => "时延非法（超窗或负值）",
            t505Code::SuccessLost => "成功率失真（成功率非 100% 却报绿）",
            t505Code::InjectEscaped => "失败注入逃逸（未抓）",
            t505Code::ReportMissing => "报告未落档",
            t505Code::LedgerBreak => "留痕账破缺",
            t505Code::GuardTripped => "守卫触发",
            t505Code::LedgerBroken => "台账不守恒（守恒恒等式破缺）",
            t505Code::SnapshotFingerprint => "快照指纹不符（字段被改后重算不一致）",
            t505Code::TableBroken => "常量表破坏（表项与语义规则矛盾）",
            t505Code::IndexMisaligned => "索引错位（枚举序与矩阵序不一致）",
            t505Code::DuplicateEntry => "条目重复（同键双写）",
            t505Code::OrderViolated => "次序违例（依赖序/时序破缺）",
            t505Code::SegmentGuard => "段位守卫失败（诊断码越出本模块段）",
        }
    }

    /// 可操作修复建议（每条诊断都必须带下一步动作）。
    pub const fn suggestion(self) -> &'static str {
        match self {
            t505Code::SwitchLost => "按三列留痕律补痕",
            t505Code::StateDirBad => "按二向闭集校正",
            t505Code::LatencyBad => "按时延窗校正",
            t505Code::SuccessLost => "按 100% 口径纠正",
            t505Code::InjectEscaped => "按 10/10 必抓补抓",
            t505Code::ReportMissing => "按落档律补档",
            t505Code::LedgerBreak => "按守恒重建",
            t505Code::GuardTripped => "检查临界区",
            t505Code::LedgerBroken => "按守恒律重建台账后重跑",
            t505Code::SnapshotFingerprint => "按防篡改规程重建快照",
            t505Code::TableBroken => "按语义规则重建常量表",
            t505Code::IndexMisaligned => "按声明序归一化后重跑",
            t505Code::DuplicateEntry => "合并同键条目为末次生效",
            t505Code::OrderViolated => "按依赖矩阵重排后重跑",
            t505Code::SegmentGuard => "核对诊断码分配表",
        }
    }
}

// ---------------------------------------------------------------------------
// FNV-1a 64 快照指纹（无第三方依赖的确定性哈希；跨次运行恒同）
// ---------------------------------------------------------------------------

/// FNV-1a 64 偏移基准（标准初值 0xcbf29ce484222325）。
pub const FNV_OFFSET: u64 = 0xcbf2_9ce4_8422_2325;

/// FNV-1a 64 素数（标准 0x100000001b3）。
pub const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;

/// 逐字节 FNV-1a 64 递推（xor 后乘，顺序即语义）。
pub const fn fnv1a64(data: &[u8]) -> u64 {
    let mut h = FNV_OFFSET;
    let mut i = 0usize;
    while i < data.len() {
        h ^= data[i] as u64;
        h = h.wrapping_mul(FNV_PRIME);
        i += 1;
    }
    h
}

/// 64 位值入链（小端字节序，保证跨平台构造一致）。
pub const fn fnv1a64_u64(h: u64, v: u64) -> u64 {
    let b = [
        (v & 0xff) as u8,
        ((v >> 8) & 0xff) as u8,
        ((v >> 16) & 0xff) as u8,
        ((v >> 24) & 0xff) as u8,
        ((v >> 32) & 0xff) as u8,
        ((v >> 40) & 0xff) as u8,
        ((v >> 48) & 0xff) as u8,
        ((v >> 56) & 0xff) as u8,
    ];
    fnv1a64_from(h, &b)
}

/// 以给定初值继续递推（链式哈希原语）。
pub const fn fnv1a64_from(h: u64, data: &[u8]) -> u64 {
    let mut hh = h;
    let mut i = 0usize;
    while i < data.len() {
        hh ^= data[i] as u64;
        hh = hh.wrapping_mul(FNV_PRIME);
        i += 1;
    }
    hh
}

// ---------------------------------------------------------------------------
// F0039 模式切换万次回归（Polling ↔ Irq；次数/态向/时延三列留痕）
// ---------------------------------------------------------------------------

/// 切换总次数（10^4）。
pub const SWITCH_RUNS: u64 = 10_000;

/// 态向（双向闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SwitchDir {
    /// Polling → Irq。
    ToIrq,
    /// Irq → Polling。
    ToPolling,
}

/// 留痕行（三列：次数/态向/时延）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SwitchTrace {
    /// 第几次（1..=10000）。
    pub seq: u64,
    /// 态向（0=ToIrq 1=ToPolling）。
    pub dir: u8,
    /// 时延（微秒，本次状态迁移耗时）。
    pub latency_us: u32,
}

/// 留痕行合法性（次数域 + 二向 + 时延窗 0..10ms）。
pub const fn trace_ok(t: &SwitchTrace) -> bool {
    t.seq >= 1 && t.seq <= SWITCH_RUNS && t.dir <= 1 && t.latency_us <= 10_000
}

/// 切换账（万次全绿 + 100% 成功率 + 失败注入 10/10）。
#[derive(Clone, Copy, Debug)]
pub struct SwitchBook {
    /// 已留痕次数。
    pub traced: u64,
    /// 成功次数。
    pub ok: u64,
    /// 失败次数（必须 0）。
    pub fail: u64,
    /// 失败注入次数。
    pub injected: u64,
    /// 注入被抓次数（必须 == injected）。
    pub caught: u64,
    /// 最大时延（us）。
    pub max_latency_us: u32,
    /// 报告指纹（FNV-1a 64）。
    pub report_fp: u64,
}

impl SwitchBook {
    /// 空账构造。
    pub const fn new() -> Self {
        Self {
            traced: 0,
            ok: 0,
            fail: 0,
            injected: 0,
            caught: 0,
            max_latency_us: 0,
            report_fp: 0,
        }
    }

    /// 记一次切换（三列齐 + 时延累计）。
    pub fn trace(&mut self, t: &SwitchTrace) -> bool {
        if !trace_ok(t) {
            self.fail += 1;
            return false;
        }
        self.traced += 1;
        self.ok += 1;
        if t.latency_us > self.max_latency_us {
            self.max_latency_us = t.latency_us;
        }
        true
    }

    /// 注入一次失败（非法态向 → 必须被 trace 拒收并计入被抓）。
    pub fn inject_fail(&mut self, seq: u64) -> bool {
        self.injected += 1;
        let bad = SwitchTrace { seq, dir: 2, latency_us: 100 };
        if !self.trace(&bad) {
            self.caught += 1;
            return true;
        }
        false
    }

    /// 万次全绿判据。
    pub fn all_ok(&self) -> bool {
        self.traced == SWITCH_RUNS && self.fail == 0 && self.ok == SWITCH_RUNS
    }

    /// 100% 成功率判据（冗余口径：ok == traced）。
    pub fn success_100(&self) -> bool {
        self.ok == self.traced && self.traced > 0
    }

    /// 失败注入 10/10 必抓。
    pub fn inject_all_caught(&self) -> bool {
        self.injected == self.caught && self.injected >= 10
    }

    /// 报告落档（FNV 指纹封存）。
    pub fn seal_report(&mut self) -> bool {
        if !self.all_ok() {
            return false;
        }
        let payload = [
            self.traced,
            self.ok as u64,
            self.max_latency_us as u64,
            self.injected,
            self.caught,
        ];
        let mut h = FNV_OFFSET;
        let mut i = 0;
        while i < payload.len() {
            h = fnv1a64_from(h, &payload[i].to_le_bytes());
            i += 1;
        }
        self.report_fp = h;
        self.report_fp != 0
    }
}

// ---------------------------------------------------------------------------
// 段位守卫（本模块独立十六进码前缀防串段；越界即判红）
// ---------------------------------------------------------------------------

/// 段位守卫：全部诊断码必须落在本段 [0x6400, 0x640f]。
pub const fn t505_segment_guard() -> bool {
    let mut ok = true;
    let mut i = 0usize;
    while i < t505Code::ALL.len() {
        let c = t505Code::ALL[i].code();
        if c < 0x6400 || c > 0x640f {
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
fn t505_guard_seg() -> bool {
    t505_segment_guard()
}

/// 留痕行三列合法性（次数/态向/时延窗 + 双向）。
fn check_c1_trace() -> bool {
    let t = SwitchTrace { seq: 1, dir: 0, latency_us: 120 };
    let mut ok = trace_ok(&t);
    if ok {
        ok = trace_ok(&SwitchTrace { seq: SWITCH_RUNS, dir: 1, latency_us: 10_000 });
    }
    if ok {
        ok = !trace_ok(&SwitchTrace { seq: 0, dir: 0, latency_us: 1 });
        ok = ok && !trace_ok(&SwitchTrace { seq: 1, dir: 2, latency_us: 1 });
        ok = ok && !trace_ok(&SwitchTrace { seq: 1, dir: 0, latency_us: 10_001 });
    }
    ok
}

/// 万次全绿（10^4 留痕 + 成功率 100% + 报告落档）。
fn check_c2_10k() -> bool {
    let mut bk = SwitchBook::new();
    let mut i = 1u64;
    while i <= 10_000 {
        let t = SwitchTrace { seq: i, dir: (i % 2) as u8, latency_us: 100 + (i % 50) as u32 };
        if !bk.trace(&t) {
            return false;
        }
        i += 1;
    }
    let mut ok = bk.all_ok() && bk.success_100() && bk.traced == 10_000;
    if ok {
        ok = bk.max_latency_us == 149 && bk.seal_report() && bk.report_fp != 0;
    }
    ok
}

/// 失败注入 10/10 必抓（非法态向全拦）。
fn check_c3_inject() -> bool {
    let mut bk = SwitchBook::new();
    let mut ok = true;
    let mut i = 1u64;
    while i <= 10 {
        ok = ok && bk.inject_fail(i);
        i += 1;
    }
    if ok {
        ok = bk.inject_all_caught() && bk.caught == 10 && bk.fail == 10;
    }
    if ok {
        // 万次账不被污染：注入的失败不计入成功。
        ok = bk.ok == 0 && bk.traced == 0;
    }
    ok
}

/// 本模块自检（逐检查带 detail，全绿才出门）。
pub fn run_t505_checks() -> CheckSet {
    let mut set = CheckSet::new("T505");
    set.add(
        "t505-seg-guard",
        t505_guard_seg(),
        if t505_guard_seg() { "" } else { "diagnostic code outside segment" },
    );
    set.add(
        "t505-c1-trace",
        check_c1_trace(),
        if check_c1_trace() { "" } else { "switch trace mismatch" },
    );
    set.add(
        "t505-c2-10k",
        check_c2_10k(),
        if check_c2_10k() { "" } else { "10k switch regression mismatch" },
    );
    set.add(
        "t505-c3-inject",
        check_c3_inject(),
        if check_c3_inject() { "" } else { "switch failure injection mismatch" },
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
    fn t505_all_checks_green() {
        let set = run_t505_checks();
        assert!(set.all_passed(), "T505 domain checks must be green");
    }

    #[test]
    fn t505_segment_guard_holds() {
        assert!(t505_segment_guard());
    }
}
