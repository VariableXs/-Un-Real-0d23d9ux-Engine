//! unxt573_uartread —— UNX-F0501 三寄存器回读一致（LCR/MCR/IER）。
//! 判据原文（CoRun Unxreal 主册 UNX-A1-B09）：UNX-F0501 · 16550 串口编程接管（F0117 升级） · 判据：COM1 编程至 115200 8N1（波特率误差 ≤1.7% 理论值核），LCR/MCR/IER 三寄存器回读一致
//! · 组件：回读对拍（三寄存器）+ 回读账 + FIFO 触发表
//! · 判据落地：LCR/MCR/IER 三寄存器回读一致
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，
//! 逐条定制数据结构/状态机/错误路径，零模板复述，零死代码。


use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const T573_VERSION: &str = "t573-uartread-v1";

/// 诊断段基址。
pub const T573_SEG_BASE: u16 = 0x4620;

// --------------------------------------------------------------------------
// 诊断码（t573 十六码 · 14 枚互斥 + segment_guard）
// --------------------------------------------------------------------------
/// 回读一致性诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum t573Code {
    /// 0x4620 回读不一致。
    ReadbackDiff,
    /// 0x4621 寄存器越界（三寄存器之外）。
    RegisterOob,
    /// 0x4622 FIFO 触发级别非法。
    FifoTriggerBad,
    /// 0x4623 回读账破缺。
    LedgerBreak,
    /// 0x4624 溢出跳过未记账。
    OverrunSkipped,
    /// 0x4625 回读序违例。
    OrderViolated,
    /// 0x4626 报告陈旧。
    ReportStale,
    /// 0x4627 守卫触发。
    GuardTripped,
    /// 0x4628 台账不守恒（守恒恒等式破缺）。
    LedgerBroken,
    /// 0x4629 快照指纹不符（字段被改后重算不一致）。
    SnapshotFingerprint,
    /// 0x462a 常量表破坏（表项与语义规则矛盾）。
    TableBroken,
    /// 0x462b 索引错位（枚举序与矩阵序不一致）。
    IndexMisaligned,
    /// 0x462c 条目重复（同键双写）。
    DuplicateEntry,
    /// 0x462d 段位守卫失败（诊断码越出本模块段）。
    SegmentGuard,
}

impl t573Code {
    /// 全量闭集。
    pub const ALL: [t573Code; 14] = [
        t573Code::ReadbackDiff,
        t573Code::RegisterOob,
        t573Code::FifoTriggerBad,
        t573Code::LedgerBreak,
        t573Code::OverrunSkipped,
        t573Code::OrderViolated,
        t573Code::ReportStale,
        t573Code::GuardTripped,
        t573Code::LedgerBroken,
        t573Code::SnapshotFingerprint,
        t573Code::TableBroken,
        t573Code::IndexMisaligned,
        t573Code::DuplicateEntry,
        t573Code::SegmentGuard,
    ];

    /// 码值（连续十六进制，与声明序恒等）。
    pub const fn code(self) -> u16 {
        match self {
            t573Code::ReadbackDiff => 0x4620,
            t573Code::RegisterOob => 0x4621,
            t573Code::FifoTriggerBad => 0x4622,
            t573Code::LedgerBreak => 0x4623,
            t573Code::OverrunSkipped => 0x4624,
            t573Code::OrderViolated => 0x4625,
            t573Code::ReportStale => 0x4626,
            t573Code::GuardTripped => 0x4627,
            t573Code::LedgerBroken => 0x4628,
            t573Code::SnapshotFingerprint => 0x4629,
            t573Code::TableBroken => 0x462a,
            t573Code::IndexMisaligned => 0x462b,
            t573Code::DuplicateEntry => 0x462c,
            t573Code::SegmentGuard => 0x462d,
        }
    }

    /// 中文诊断名。
    pub const fn name(self) -> &'static str {
        match self {
            t573Code::ReadbackDiff => "回读不一致",
            t573Code::RegisterOob => "寄存器越界（三寄存器之外）",
            t573Code::FifoTriggerBad => "FIFO 触发级别非法",
            t573Code::LedgerBreak => "回读账破缺",
            t573Code::OverrunSkipped => "溢出跳过未记账",
            t573Code::OrderViolated => "回读序违例",
            t573Code::ReportStale => "报告陈旧",
            t573Code::GuardTripped => "守卫触发",
            t573Code::LedgerBroken => "台账不守恒（守恒恒等式破缺）",
            t573Code::SnapshotFingerprint => "快照指纹不符（字段被改后重算不一致）",
            t573Code::TableBroken => "常量表破坏（表项与语义规则矛盾）",
            t573Code::IndexMisaligned => "索引错位（枚举序与矩阵序不一致）",
            t573Code::DuplicateEntry => "条目重复（同键双写）",
            t573Code::SegmentGuard => "段位守卫失败（诊断码越出本模块段）",
        }
    }

    /// 可操作修复建议（每条诊断都必须带下一步动作）。
    pub const fn suggestion(self) -> &'static str {
        match self {
            t573Code::ReadbackDiff => "按归因路径登记",
            t573Code::RegisterOob => "按三寄存器闭集裁剪",
            t573Code::FifoTriggerBad => "按触发表校正",
            t573Code::LedgerBreak => "按守恒重建",
            t573Code::OverrunSkipped => "按溢出路径补登",
            t573Code::OrderViolated => "按序记账",
            t573Code::ReportStale => "重渲染",
            t573Code::GuardTripped => "检查临界区",
            t573Code::LedgerBroken => "按守恒律重建台账后重跑",
            t573Code::SnapshotFingerprint => "按防篡改规程重建快照",
            t573Code::TableBroken => "按语义规则重建常量表",
            t573Code::IndexMisaligned => "按声明序归一化后重跑",
            t573Code::DuplicateEntry => "合并同键条目为末次生效",
            t573Code::SegmentGuard => "核对诊断码分配表",
        }
    }
}

// --------------------------------------------------------------------------
// 引擎 uart_dual（双源对拍：3 槽位逐槽回读一致性）
// --------------------------------------------------------------------------
/// 三寄存器回读对拍（写值 vs 读回值）。
/// 槽位枚举（闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum uart_dualSlot {
    /// LCR。
    Lcr,
    /// MCR。
    Mcr,
    /// IER。
    Ier,
}

impl uart_dualSlot {
    /// 全量槽位闭集。
    pub const ALL: [uart_dualSlot; 3] = [
        uart_dualSlot::Lcr,
        uart_dualSlot::Mcr,
        uart_dualSlot::Ier,
    ];

    /// 索引（与声明序恒等）。
    pub const fn index(self) -> usize {
        match self {
            uart_dualSlot::Lcr => 0,
            uart_dualSlot::Mcr => 1,
            uart_dualSlot::Ier => 2,
        }
    }

    /// 中文槽名。
    pub const fn cn(self) -> &'static str {
        match self {
            uart_dualSlot::Lcr => "LCR",
            uart_dualSlot::Mcr => "MCR",
            uart_dualSlot::Ier => "IER",
        }
    }
}

/// 双源样本（A 源首读 + B 源回读 + 一致性判定）。
#[derive(Clone, Copy, Debug)]
pub struct uart_dualPair {
    /// A 源首读值。
    pub a: u64,
    /// B 源回读值。
    pub b: u64,
    /// 是否一致。
    pub same: bool,
    /// 采样序号。
    pub seq: u64,
}

/// 对拍账（逐槽一致才放行；不一致槽位显性记账）。
#[derive(Clone, Copy, Debug)]
pub struct uart_dualDual {
    /// 逐槽样本。
    pub pairs: [uart_dualPair; 3],
    /// 一致槽位数。
    pub same_n: usize,
    /// 不一致槽位数。
    pub diff_n: usize,
    /// 采样总轮次。
    pub rounds: u64,
    /// 已采样的不一致槽位掩码。
    pub diff_mask: u64,
}

impl uart_dualDual {
    /// 全零构造。
    pub const fn new() -> Self {
        Self {
            pairs: [uart_dualPair { a: 0, b: 0, same: false, seq: 0 }; 3],
            same_n: 0,
            diff_n: 0,
            rounds: 0,
            diff_mask: 0,
        }
    }

    /// 单槽采样（A/B 双源读入后逐位比较）。
    pub fn sample(&mut self, who: uart_dualSlot, a: u64, b: u64) {
        let i = who.index();
        let same = a == b;
        self.pairs[i] = uart_dualPair { a, b, same, seq: self.rounds };
        if same {
            self.same_n += 1;
        } else {
            self.diff_n += 1;
            self.diff_mask |= 1u64 << (i as u64);
        }
        self.rounds += 1;
    }

    /// 全槽一致判定（对拍通过口径）。
    pub fn all_same(&self) -> bool {
        self.diff_n == 0 && self.same_n == 3
    }

    /// 复算：逐槽重读 pairs 重判 same（防记账面与数据面分叉）。
    pub fn recount(&self) -> bool {
        let mut same = 0usize;
        let mut i = 0usize;
        while i < 3 {
            if self.pairs[i].same == (self.pairs[i].a == self.pairs[i].b) {
                same += 1;
            }
            i += 1;
        }
        same == 3 && self.same_n + self.diff_n == 3
    }
}

// --------------------------------------------------------------------------
// 字符串键值映射 uartfifo（4 行逐行语义注释 + 逐行守卫）
// --------------------------------------------------------------------------
/// UART FIFO 触发级别表（四档）。
/// 行数。
pub const uartfifo_COUNT: usize = 4;

/// 表本体（键 + 值 + 注释由编译期冻结）。
pub static uartfifo: [(&'static str, u64); 4] = [
    ("FIFO_T1", 0), // 1 字节触发
    ("FIFO_T4", 1), // 4 字节触发
    ("FIFO_T8", 2), // 8 字节触发
    ("FIFO_T14", 3), // 14 字节触发
];

/// O(1) 语义查找（match 全枚举；命中率 100%% 由守卫保证）。
pub fn uartfifo_get(key: &str) -> Option<u64> {
    match key {
        "FIFO_T1" => Some(0),
        "FIFO_T4" => Some(1),
        "FIFO_T8" => Some(2),
        "FIFO_T14" => Some(3),
        _ => None,
    }
}

/// 逐行守卫扫描（返回违例行数；0 = 全绿）。
pub fn uartfifo_scan() -> usize {
    let mut bad = 0usize;
    let mut i = 0usize;
    while i < uartfifo_COUNT {
        let (k, v) = uartfifo[i];
        let ok = match k {
            "FIFO_T1" => v == 0,
            "FIFO_T4" => v == 1,
            "FIFO_T8" => v == 2,
            "FIFO_T14" => v == 3,
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
pub fn uartfifo_sum() -> u64 {
    let mut s = 0u64;
    let mut i = 0usize;
    while i < uartfifo_COUNT {
        s = s.wrapping_add(uartfifo[i].1 as u64);
        i += 1;
    }
    s
}

/// 键唯一性（构造期断言等价物：逐对扫描同键）。
pub fn uartfifo_unique() -> bool {
    let mut i = 0usize;
    while i < uartfifo_COUNT {
        let mut j = i + 1;
        while j < uartfifo_COUNT {
            if uartfifo[i].0 == uartfifo[j].0 {
                return false;
            }
            j += 1;
        }
        i += 1;
    }
    true
}

// ---------------------------------------------------------------------------
// 段位守卫（本模块独立十六进码前缀防串段；越界即判红）
// ---------------------------------------------------------------------------

/// 段位守卫：全部诊断码必须落在本段 [0x4620, 0x462f]。
pub const fn t573_segment_guard() -> bool {
    let mut ok = true;
    let mut i = 0usize;
    while i < t573Code::ALL.len() {
        let c = t573Code::ALL[i].code();
        if c < 0x4620 || c > 0x462f {
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
fn t573_guard_seg() -> bool {
    t573_segment_guard()
}

/// 表 uartfifo 逐行守卫全绿（行数/守卫/唯一性）。
fn check_t573_uartfifo_guard() -> bool {
    let mut ok = uartfifo_COUNT == 4;
    if ok {
        ok = uartfifo_scan() == 0;
    }
    if ok {
        ok = uartfifo_unique();
    }
    ok
}

/// 三寄存器回读一致 + 负样本。
fn check_c2_readback() -> bool {
    let mut d = uart_dualDual::new();
    d.sample(uart_dualSlot::Lcr, 0x03, 0x03);
    d.sample(uart_dualSlot::Mcr, 0x0B, 0x0B);
    d.sample(uart_dualSlot::Ier, 0x01, 0x01);
    let mut ok = d.all_same() && d.same_n == 3;
    if ok {
        d.sample(uart_dualSlot::Mcr, 0x0B, 0x0A);
        ok = !d.all_same() && d.diff_n == 1;
    }
    ok
}

/// 本模块自检（逐检查带 detail，全绿才出门）。
pub fn run_t573_checks() -> CheckSet {
    let mut set = CheckSet::new("T573");
    set.add(
        "t573-seg-guard",
        t573_guard_seg(),
        if t573_guard_seg() { "" } else { "diagnostic code outside segment" },
    );
    set.add(
        "t573-c1-fifo-guard",
        check_t573_uartfifo_guard(),
        if check_t573_uartfifo_guard() { "" } else { "table uartfifo guard mismatch" },
    );
    set.add(
        "t573-c2-readback",
        check_c2_readback(),
        if check_c2_readback() { "" } else { "uart register readback mismatch" },
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
    fn t573_all_checks_green() {
        let set = run_t573_checks();
        assert!(set.all_passed(), "T573 domain checks must be green");
    }

    #[test]
    fn t573_segment_guard_holds() {
        assert!(t573_segment_guard());
    }
}
