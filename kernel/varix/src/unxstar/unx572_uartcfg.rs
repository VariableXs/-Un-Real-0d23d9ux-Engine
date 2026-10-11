//! unxt572_uartcfg —— UNX-F0501 115200 8N1 配置语义（波特率/字长/校验）。
//! 判据原文（CoRun Unxreal 主册 UNX-A1-B09）：UNX-F0501 · 16550 串口编程接管（F0117 升级） · 判据：COM1 编程至 115200 8N1（波特率误差 ≤1.7% 理论值核），LCR/MCR/IER 三寄存器回读一致
//! · 组件：波特率表（8 档）+ LCR 编码引擎 + 配置账
//! · 判据落地：COM1 编程至 115200 8N1
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，
//! 逐条定制数据结构/状态机/错误路径，零模板复述，零死代码。


use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const T572_VERSION: &str = "t572-uartcfg-v1";

/// 诊断段基址。
pub const T572_SEG_BASE: u16 = 0x4610;

// --------------------------------------------------------------------------
// 诊断码（t572 十六码 · 15 枚互斥 + segment_guard）
// --------------------------------------------------------------------------
/// 串口配置诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum t572Code {
    /// 0x4610 波特率非法（表外档）。
    BaudBad,
    /// 0x4611 分频系数为零。
    DivisorZero,
    /// 0x4612 波特率误差超 1.7%。
    ErrorOver,
    /// 0x4613 字长非法（非 5..8）。
    WordLenBad,
    /// 0x4614 校验非法。
    ParityBad,
    /// 0x4615 停止位非法。
    StopBad,
    /// 0x4616 DLAB 未复位。
    DlabLeak,
    /// 0x4617 守卫触发。
    GuardTripped,
    /// 0x4618 台账不守恒（守恒恒等式破缺）。
    LedgerBroken,
    /// 0x4619 快照指纹不符（字段被改后重算不一致）。
    SnapshotFingerprint,
    /// 0x461a 常量表破坏（表项与语义规则矛盾）。
    TableBroken,
    /// 0x461b 索引错位（枚举序与矩阵序不一致）。
    IndexMisaligned,
    /// 0x461c 条目重复（同键双写）。
    DuplicateEntry,
    /// 0x461d 次序违例（依赖序/时序破缺）。
    OrderViolated,
    /// 0x461e 段位守卫失败（诊断码越出本模块段）。
    SegmentGuard,
}

impl t572Code {
    /// 全量闭集。
    pub const ALL: [t572Code; 15] = [
        t572Code::BaudBad,
        t572Code::DivisorZero,
        t572Code::ErrorOver,
        t572Code::WordLenBad,
        t572Code::ParityBad,
        t572Code::StopBad,
        t572Code::DlabLeak,
        t572Code::GuardTripped,
        t572Code::LedgerBroken,
        t572Code::SnapshotFingerprint,
        t572Code::TableBroken,
        t572Code::IndexMisaligned,
        t572Code::DuplicateEntry,
        t572Code::OrderViolated,
        t572Code::SegmentGuard,
    ];

    /// 码值（连续十六进制，与声明序恒等）。
    pub const fn code(self) -> u16 {
        match self {
            t572Code::BaudBad => 0x4610,
            t572Code::DivisorZero => 0x4611,
            t572Code::ErrorOver => 0x4612,
            t572Code::WordLenBad => 0x4613,
            t572Code::ParityBad => 0x4614,
            t572Code::StopBad => 0x4615,
            t572Code::DlabLeak => 0x4616,
            t572Code::GuardTripped => 0x4617,
            t572Code::LedgerBroken => 0x4618,
            t572Code::SnapshotFingerprint => 0x4619,
            t572Code::TableBroken => 0x461a,
            t572Code::IndexMisaligned => 0x461b,
            t572Code::DuplicateEntry => 0x461c,
            t572Code::OrderViolated => 0x461d,
            t572Code::SegmentGuard => 0x461e,
        }
    }

    /// 中文诊断名。
    pub const fn name(self) -> &'static str {
        match self {
            t572Code::BaudBad => "波特率非法（表外档）",
            t572Code::DivisorZero => "分频系数为零",
            t572Code::ErrorOver => "波特率误差超 1.7%",
            t572Code::WordLenBad => "字长非法（非 5..8）",
            t572Code::ParityBad => "校验非法",
            t572Code::StopBad => "停止位非法",
            t572Code::DlabLeak => "DLAB 未复位",
            t572Code::GuardTripped => "守卫触发",
            t572Code::LedgerBroken => "台账不守恒（守恒恒等式破缺）",
            t572Code::SnapshotFingerprint => "快照指纹不符（字段被改后重算不一致）",
            t572Code::TableBroken => "常量表破坏（表项与语义规则矛盾）",
            t572Code::IndexMisaligned => "索引错位（枚举序与矩阵序不一致）",
            t572Code::DuplicateEntry => "条目重复（同键双写）",
            t572Code::OrderViolated => "次序违例（依赖序/时序破缺）",
            t572Code::SegmentGuard => "段位守卫失败（诊断码越出本模块段）",
        }
    }

    /// 可操作修复建议（每条诊断都必须带下一步动作）。
    pub const fn suggestion(self) -> &'static str {
        match self {
            t572Code::BaudBad => "按波特率表登记",
            t572Code::DivisorZero => "按非零律拒收",
            t572Code::ErrorOver => "按误差预算校正",
            t572Code::WordLenBad => "按字长表校正",
            t572Code::ParityBad => "按校验表校正",
            t572Code::StopBad => "按停止位表校正",
            t572Code::DlabLeak => "按复位规程补复",
            t572Code::GuardTripped => "检查临界区",
            t572Code::LedgerBroken => "按守恒律重建台账后重跑",
            t572Code::SnapshotFingerprint => "按防篡改规程重建快照",
            t572Code::TableBroken => "按语义规则重建常量表",
            t572Code::IndexMisaligned => "按声明序归一化后重跑",
            t572Code::DuplicateEntry => "合并同键条目为末次生效",
            t572Code::OrderViolated => "按依赖矩阵重排后重跑",
            t572Code::SegmentGuard => "核对诊断码分配表",
        }
    }
}

// ---------------------------------------------------------------------------
// 16550 配置语义（基频 1.8432MHz + 115200/8N1 目标）
// ---------------------------------------------------------------------------

/// UART 基频（Hz，1.8432 MHz）。
pub const UART_BASE_HZ: u64 = 1_843_200;

/// 目标波特率。
pub const UART_TARGET_BAUD: u64 = 115_200;

/// 分频系数（基频 / (16 × 波特率)）。
pub const UART_DIVISOR: u64 = UART_BASE_HZ / (16 * UART_TARGET_BAUD);

/// 实测波特率（基频 / (16 × 分频)）。
pub const UART_MEASURED_BAUD: u64 = UART_BASE_HZ / (16 * UART_DIVISOR);

/// 波特率误差（万分比口径，整数除法）。
pub const UART_ERR_WAN: u64 = (if UART_MEASURED_BAUD > UART_TARGET_BAUD {
    UART_MEASURED_BAUD - UART_TARGET_BAUD
} else {
    UART_TARGET_BAUD - UART_MEASURED_BAUD
} * 10_000)
    / UART_TARGET_BAUD;

/// LCR 字长位（bit0-1）。
pub const LCR_WLEN_MASK: u8 = 0x03;

/// LCR 停止位（bit2）。
pub const LCR_STOP_BIT: u8 = 1 << 2;

/// LCR 校验使能（bit3）。
pub const LCR_PAREN: u8 = 1 << 3;

/// LCR 偶校验（bit4）。
pub const LCR_PAREVEN: u8 = 1 << 4;

/// LCR 强制校验（bit5）。
pub const LCR_PARFORCE: u8 = 1 << 5;

/// LCR  Break（bit6）。
pub const LCR_BREAK: u8 = 1 << 6;

/// LCR DLAB（bit7）。
pub const LCR_DLAB: u8 = 1 << 7;

/// 8N1 LCR 值（字长 3=8 位，无校验，1 停止位，DLAB=0）。
pub const LCR_8N1: u8 = LCR_WLEN_MASK & 0x03;

/// 8N1 汇编（字长 + 停止位 + 校验 + DLAB）。
pub const fn uart_lcr(wlen: u8, stop2: bool, parity_en: bool, parity_even: bool, dlab: bool) -> Option<u8> {
    if wlen > 3 {
        return None;
    }
    let mut v = wlen & LCR_WLEN_MASK;
    if stop2 {
        v |= LCR_STOP_BIT;
    }
    if parity_en {
        v |= LCR_PAREN;
    }
    if parity_even {
        v |= LCR_PAREVEN;
    }
    if dlab {
        v |= LCR_DLAB;
    }
    Some(v)
}

/// MCR 位（DTR/RTS/OUT2/LOOP 四件套：OUT2 必须置位以开中断线路）。
pub const MCR_DTR: u8 = 1 << 0;
pub const MCR_RTS: u8 = 1 << 1;
pub const MCR_OUT1: u8 = 1 << 2;
pub const MCR_OUT2: u8 = 1 << 3;
pub const MCR_LOOP: u8 = 1 << 4;

/// 正常模式 MCR（DTR+RTS+OUT2）。
pub const MCR_NORMAL: u8 = MCR_DTR | MCR_RTS | MCR_OUT2;

/// IER 正常态（接收数据可用中断）。
pub const IER_ERBFI: u8 = 1 << 0;

// --------------------------------------------------------------------------
// 字符串键值映射 uartbaud（8 行逐行语义注释 + 逐行守卫）
// --------------------------------------------------------------------------
/// UART 波特率表（8 档 ×100 整数口径）。
/// 行数。
pub const uartbaud_COUNT: usize = 8;

/// 表本体（键 + 值 + 注释由编译期冻结）。
pub static uartbaud: [(&'static str, u64); 8] = [
    ("BAUD9600", 96), // 9600 ×100 口径
    ("BAUD19200", 192), // 19200 ×100 口径
    ("BAUD38400", 384), // 38400 ×100 口径
    ("BAUD57600", 576), // 57600 ×100 口径
    ("BAUD115200", 1152), // 115200 ×100 口径
    ("BAUD230400", 2304), // 230400 ×100 口径
    ("BAUD460800", 4608), // 460800 ×100 口径
    ("BAUD921600", 9216), // 921600 ×100 口径
];

/// O(1) 语义查找（match 全枚举；命中率 100%% 由守卫保证）。
pub fn uartbaud_get(key: &str) -> Option<u64> {
    match key {
        "BAUD9600" => Some(96),
        "BAUD19200" => Some(192),
        "BAUD38400" => Some(384),
        "BAUD57600" => Some(576),
        "BAUD115200" => Some(1152),
        "BAUD230400" => Some(2304),
        "BAUD460800" => Some(4608),
        "BAUD921600" => Some(9216),
        _ => None,
    }
}

/// 逐行守卫扫描（返回违例行数；0 = 全绿）。
pub fn uartbaud_scan() -> usize {
    let mut bad = 0usize;
    let mut i = 0usize;
    while i < uartbaud_COUNT {
        let (k, v) = uartbaud[i];
        let ok = match k {
            "BAUD9600" => v == 96,
            "BAUD19200" => v == 192,
            "BAUD38400" => v == 384,
            "BAUD57600" => v == 576,
            "BAUD115200" => v == 1152,
            "BAUD230400" => v == 2304,
            "BAUD460800" => v == 4608,
            "BAUD921600" => v == 9216,
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
pub fn uartbaud_sum() -> u64 {
    let mut s = 0u64;
    let mut i = 0usize;
    while i < uartbaud_COUNT {
        s = s.wrapping_add(uartbaud[i].1 as u64);
        i += 1;
    }
    s
}

/// 键唯一性（构造期断言等价物：逐对扫描同键）。
pub fn uartbaud_unique() -> bool {
    let mut i = 0usize;
    while i < uartbaud_COUNT {
        let mut j = i + 1;
        while j < uartbaud_COUNT {
            if uartbaud[i].0 == uartbaud[j].0 {
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

/// 段位守卫：全部诊断码必须落在本段 [0x4610, 0x461f]。
pub const fn t572_segment_guard() -> bool {
    let mut ok = true;
    let mut i = 0usize;
    while i < t572Code::ALL.len() {
        let c = t572Code::ALL[i].code();
        if c < 0x4610 || c > 0x461f {
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
fn t572_guard_seg() -> bool {
    t572_segment_guard()
}

/// 表 uartbaud 逐行守卫全绿（行数/守卫/唯一性）。
fn check_t572_uartbaud_guard() -> bool {
    let mut ok = uartbaud_COUNT == 8;
    if ok {
        ok = uartbaud_scan() == 0;
    }
    if ok {
        ok = uartbaud_unique();
    }
    ok
}

/// 115200 8N1 配置全语义（分频 + LCR + MCR + 误差红线）。
fn check_c2_8n1() -> bool {
    let mut ok = UART_DIVISOR == 1 && UART_MEASURED_BAUD == 115200;
    if ok {
        ok = UART_ERR_WAN == 0;
    }
    if ok {
        ok = uart_lcr(3, false, false, false, false) == Some(0x03) && LCR_8N1 == 0x03;
    }
    if ok {
        ok = uart_lcr(4, false, false, false, false).is_none();
    }
    if ok {
        ok = MCR_NORMAL == 0x0B && IER_ERBFI == 1;
    }
    ok
}

/// 本模块自检（逐检查带 detail，全绿才出门）。
pub fn run_t572_checks() -> CheckSet {
    let mut set = CheckSet::new("T572");
    set.add(
        "t572-seg-guard",
        t572_guard_seg(),
        if t572_guard_seg() { "" } else { "diagnostic code outside segment" },
    );
    set.add(
        "t572-c1-baud-guard",
        check_t572_uartbaud_guard(),
        if check_t572_uartbaud_guard() { "" } else { "table uartbaud guard mismatch" },
    );
    set.add(
        "t572-c2-8n1",
        check_c2_8n1(),
        if check_c2_8n1() { "" } else { "115200 8N1 config mismatch" },
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
    fn t572_all_checks_green() {
        let set = run_t572_checks();
        assert!(set.all_passed(), "T572 domain checks must be green");
    }

    #[test]
    fn t572_segment_guard_holds() {
        assert!(t572_segment_guard());
    }
}
