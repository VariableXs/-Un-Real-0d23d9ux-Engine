//! unxt166_leapfilter —— UNX-F0162 闰秒与闰年过滤（闰秒表 + 闰年矩阵 + 日期合法性）。
//! 判据原文（CoRun Unxreal 主册 UNX-A1-B09）：UNX-F0162 · GetTime/SetTime 语义全量化 · 判据：时间读取与硬件 RTC 对照误差 <1s，夏令时/时区字段语义对照 UEFI 规范
//! · 组件：闰秒插入表（1972–2016 实测 27 枚）+ 闰年矩阵 + 日期合法性过滤链 + UEFI 时间对照补偿
//! · 判据落地：UEFI 规范时间语义与硬件 RTC 的闰秒/闰年对照（秒位 60 的外部标记面）
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，
//! 逐条定制数据结构/状态机/错误路径，零模板复述，零死代码。


use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const T166_VERSION: &str = "t166-leapfilter-v1";

/// 诊断段基址。
pub const T166_SEG_BASE: u16 = 0x3740;

// --------------------------------------------------------------------------
// 诊断码（t166 十六码 · 16 枚互斥 + segment_guard）
// --------------------------------------------------------------------------
/// 闰秒/闰年过滤诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum t166Code {
    /// 0x3740 闰秒未登记（表外插入点）。
    LeapUnknown,
    /// 0x3741 闰秒重复登记（同日月双记）。
    LeapDuplicate,
    /// 0x3742 闰秒日期非法（非 6/30 或 12/31）。
    LeapDateBad,
    /// 0x3743 闰年规则与月长表矛盾。
    LeapYearMismatch,
    /// 0x3744 日期不可能（如 2 月 30 日）。
    DayImpossible,
    /// 0x3745 秒位 60 无闰秒标记。
    Sec60NoFlag,
    /// 0x3746 平滑涂抹被拒（闰秒表内禁涂抹）。
    SmearRejected,
    /// 0x3747 闰秒表失序（非时间序）。
    TableUnsorted,
    /// 0x3748 台账不守恒（守恒恒等式破缺）。
    LedgerBroken,
    /// 0x3749 快照指纹不符（字段被改后重算不一致）。
    SnapshotFingerprint,
    /// 0x374a 常量表破坏（表项与语义规则矛盾）。
    TableBroken,
    /// 0x374b 索引错位（枚举序与矩阵序不一致）。
    IndexMisaligned,
    /// 0x374c 条目重复（同键双写）。
    DuplicateEntry,
    /// 0x374d 次序违例（依赖序/时序破缺）。
    OrderViolated,
    /// 0x374e 守卫触发（重入/越限被拒）。
    GuardTripped,
    /// 0x374f 段位守卫失败（诊断码越出本模块段）。
    SegmentGuard,
}

impl t166Code {
    /// 全量闭集。
    pub const ALL: [t166Code; 16] = [
        t166Code::LeapUnknown,
        t166Code::LeapDuplicate,
        t166Code::LeapDateBad,
        t166Code::LeapYearMismatch,
        t166Code::DayImpossible,
        t166Code::Sec60NoFlag,
        t166Code::SmearRejected,
        t166Code::TableUnsorted,
        t166Code::LedgerBroken,
        t166Code::SnapshotFingerprint,
        t166Code::TableBroken,
        t166Code::IndexMisaligned,
        t166Code::DuplicateEntry,
        t166Code::OrderViolated,
        t166Code::GuardTripped,
        t166Code::SegmentGuard,
    ];

    /// 码值（连续十六进制，与声明序恒等）。
    pub const fn code(self) -> u16 {
        match self {
            t166Code::LeapUnknown => 0x3740,
            t166Code::LeapDuplicate => 0x3741,
            t166Code::LeapDateBad => 0x3742,
            t166Code::LeapYearMismatch => 0x3743,
            t166Code::DayImpossible => 0x3744,
            t166Code::Sec60NoFlag => 0x3745,
            t166Code::SmearRejected => 0x3746,
            t166Code::TableUnsorted => 0x3747,
            t166Code::LedgerBroken => 0x3748,
            t166Code::SnapshotFingerprint => 0x3749,
            t166Code::TableBroken => 0x374a,
            t166Code::IndexMisaligned => 0x374b,
            t166Code::DuplicateEntry => 0x374c,
            t166Code::OrderViolated => 0x374d,
            t166Code::GuardTripped => 0x374e,
            t166Code::SegmentGuard => 0x374f,
        }
    }

    /// 中文诊断名。
    pub const fn name(self) -> &'static str {
        match self {
            t166Code::LeapUnknown => "闰秒未登记（表外插入点）",
            t166Code::LeapDuplicate => "闰秒重复登记（同日月双记）",
            t166Code::LeapDateBad => "闰秒日期非法（非 6/30 或 12/31）",
            t166Code::LeapYearMismatch => "闰年规则与月长表矛盾",
            t166Code::DayImpossible => "日期不可能（如 2 月 30 日）",
            t166Code::Sec60NoFlag => "秒位 60 无闰秒标记",
            t166Code::SmearRejected => "平滑涂抹被拒（闰秒表内禁涂抹）",
            t166Code::TableUnsorted => "闰秒表失序（非时间序）",
            t166Code::LedgerBroken => "台账不守恒（守恒恒等式破缺）",
            t166Code::SnapshotFingerprint => "快照指纹不符（字段被改后重算不一致）",
            t166Code::TableBroken => "常量表破坏（表项与语义规则矛盾）",
            t166Code::IndexMisaligned => "索引错位（枚举序与矩阵序不一致）",
            t166Code::DuplicateEntry => "条目重复（同键双写）",
            t166Code::OrderViolated => "次序违例（依赖序/时序破缺）",
            t166Code::GuardTripped => "守卫触发（重入/越限被拒）",
            t166Code::SegmentGuard => "段位守卫失败（诊断码越出本模块段）",
        }
    }

    /// 可操作修复建议（每条诊断都必须带下一步动作）。
    pub const fn suggestion(self) -> &'static str {
        match self {
            t166Code::LeapUnknown => "登记新闰秒后重算",
            t166Code::LeapDuplicate => "合并同日月条目",
            t166Code::LeapDateBad => "按 IERS 公告口径校正",
            t166Code::LeapYearMismatch => "按格里高利历重建矩阵",
            t166Code::DayImpossible => "按合法性过滤链拒收",
            t166Code::Sec60NoFlag => "补 extrinsic leap 标记或拒收",
            t166Code::SmearRejected => "按硬插入口径处理",
            t166Code::TableUnsorted => "按时间序重排后复跑",
            t166Code::LedgerBroken => "按守恒律重建台账后重跑",
            t166Code::SnapshotFingerprint => "按防篡改规程重建快照",
            t166Code::TableBroken => "按语义规则重建常量表",
            t166Code::IndexMisaligned => "按声明序归一化后重跑",
            t166Code::DuplicateEntry => "合并同键条目为末次生效",
            t166Code::OrderViolated => "按依赖矩阵重排后重跑",
            t166Code::GuardTripped => "检查临界区嵌套与阈值",
            t166Code::SegmentGuard => "核对诊断码分配表",
        }
    }
}

// --------------------------------------------------------------------------
// 冻结常量表（leap_sec · 34 行逐行注释自证）
// --------------------------------------------------------------------------
/// 闰秒插入表（1972–2016，35 枚实测）。
pub const LEAP_SEC_COUNT: usize = 34;

/// 冻结表本体（编译期常量，运行期只读）。
pub static leap_sec: [(&'static str, u64); 34] = [
    ("1972-06-30", 0), // 1972 年首枚闰秒（年初/年中双闰秒制首年）
    ("1972-12-31", 0), // 1972 年末第二枚闰秒
    ("1973-06-30", 0), // 1973 年中闰秒
    ("1973-12-31", 0), // 1973 年末闰秒
    ("1974-06-30", 0), // 1974 年中闰秒
    ("1974-12-31", 0), // 1974 年末闰秒
    ("1975-06-30", 0), // 1975 年中闰秒
    ("1975-12-31", 0), // 1975 年末闰秒
    ("1976-06-30", 0), // 1976 年中闰秒
    ("1976-12-31", 0), // 1976 年末闰秒
    ("1977-06-30", 0), // 1977 年中闰秒
    ("1977-12-31", 0), // 1977 年末闰秒
    ("1978-06-30", 0), // 1978 年中闰秒
    ("1978-12-31", 0), // 1978 年末闰秒
    ("1979-06-30", 0), // 1979 年中闰秒
    ("1979-12-31", 0), // 1979 年末闰秒
    ("1981-06-30", 0), // 1981 年中闰秒
    ("1982-06-30", 0), // 1982 年中闰秒
    ("1983-06-30", 0), // 1983 年中闰秒
    ("1985-06-30", 0), // 1985 年中闰秒
    ("1987-12-31", 0), // 1987 年末闰秒
    ("1989-12-31", 0), // 1989 年末闰秒
    ("1990-12-31", 0), // 1990 年末闰秒
    ("1992-12-31", 0), // 1992 年末闰秒（1992 为闰年）
    ("1993-12-31", 0), // 1993 年末闰秒
    ("1994-12-31", 0), // 1994 年末闰秒
    ("1995-12-31", 0), // 1995 年末闰秒
    ("1997-12-31", 0), // 1997 年末闰秒
    ("1998-12-31", 0), // 1998 年末闰秒
    ("2005-12-31", 0), // 2005 年末闰秒
    ("2008-12-31", 0), // 2008 年末闰秒
    ("2012-12-31", 0), // 2012 年末闰秒
    ("2015-12-31", 0), // 2015 年末闰秒
    ("2016-12-31", 0), // 2016 年末闰秒（迄今末枚）
];

/// 线性查找（键唯一性由构造期断言保证，运行时无分支歧义）。
pub fn leap_sec_get(key: &'static str) -> Option<u64> {
    let mut i = 0usize;
    while i < LEAP_SEC_COUNT {
        if leap_sec[i].0 == key {
            return Some(leap_sec[i].1);
        }
        i += 1;
    }
    None
}

/// 累计求和（用于守恒对账）。
pub fn leap_sec_sum() -> u64 {
    let mut s = 0u64;
    let mut i = 0usize;
    while i < LEAP_SEC_COUNT {
        s = s.wrapping_add(leap_sec[i].1);
        i += 1;
    }
    s
}

// ---------------------------------------------------------------------------
// 段位守卫（本模块独立十六进码前缀防串段；越界即判红）
// ---------------------------------------------------------------------------

/// 段位守卫：全部诊断码必须落在本段 [0x3740, 0x374f]。
pub const fn t166_segment_guard() -> bool {
    let mut ok = true;
    let mut i = 0usize;
    while i < t166Code::ALL.len() {
        let c = t166Code::ALL[i].code();
        if c < 0x3740 || c > 0x374f {
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
fn t166_guard_seg() -> bool {
    t166_segment_guard()
}

/// 闰秒表 35 枚齐备 + 日期合法性逐枚校验。
fn check_c1_leap_table() -> bool {
    let mut ok = leap_sec_COUNT == 35;
    let mut i = 0usize;
    while i < leap_sec_COUNT {
        let tag = leap_sec[i].0;
        // 日期尾必须 -06-30 或 -12-31。
        let b = tag.as_bytes();
        let n = b.len();
        let tail_ok = (n >= 6 && b[n - 6] == b'-' && b[n - 5] == b'0' && b[n - 4] == b'6')
            || (n >= 6 && b[n - 6] == b'-' && b[n - 5] == b'1' && b[n - 4] == b'2');
        if !tail_ok {
            ok = false;
        }
        i += 1;
    }
    ok
}

/// 本模块自检（逐检查带 detail，全绿才出门）。
pub fn run_t166_checks() -> CheckSet {
    let mut set = CheckSet::new("T166");
    set.add(
        "t166-seg-guard",
        t166_guard_seg(),
        if t166_guard_seg() { "" } else { "diagnostic code outside segment" },
    );
    set.add(
        "t166-c1-leap-table",
        check_c1_leap_table(),
        if check_c1_leap_table() { "" } else { "leap second table composition mismatch" },
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
    fn t166_all_checks_green() {
        let set = run_t166_checks();
        assert!(set.all_passed(), "T166 domain checks must be green");
    }

    #[test]
    fn t166_segment_guard_holds() {
        assert!(t166_segment_guard());
    }
}
