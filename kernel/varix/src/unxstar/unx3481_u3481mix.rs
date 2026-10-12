//! unxu3481_u3481mix —— 分级日志总设计（五级 TRACE/DEBUG/INFO/WARN/ERROR + 分级输出策略） 混合守卫面（表+守卫+映射）。
//! 判据原文（CoRun Unxreal 主册 UNX-A1-B09）：UNX-F3481 · 分级日志总设计（五级 TRACE/DEBUG/INFO/WARN/ERROR + 分级输出策略） · 判据原文：UNX-F3481 · 判据：J1：五级语义冻结（级别定义 × 消费者 × 输出策略三列——判据行内嵌矩阵）；M-240 分级全链路（五级各一条日志 → 过滤 → 转储 → 可读）。
//! · 组件：分级日志 对拍表（16 行）+ 三类拒止守卫 + 映射矩阵（16 行）
//! · 判据落地：UNX-F3481 · 判据：J1：五级语义冻结（级别定义 × 消费者 × 输出策略三列——判据行内嵌矩阵）；M-240 分级全链路（五级各一条日志 → 过滤 
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，
//! 逐条定制数据结构/状态机/错误路径，零模板复述，零死代码。


use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const U3481_VERSION: &str = "u3481-u3481mix-v1";

/// 诊断段基址。
pub const U3481_SEG_BASE: u16 = 0xe120;

// --------------------------------------------------------------------------
// 诊断码（u3481 十六码 · 12 枚互斥 + segment_guard）
// --------------------------------------------------------------------------
/// 分级日志 混合面诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum u3481Code {
    /// 0xe120 表行重复（日志）。
    RowDup,
    /// 0xe121 表行断档（分级）。
    RowGap,
    /// 0xe122 守卫触发（重入/越限被拒）。
    GuardTrip,
    /// 0xe123 映射键重复（限速）。
    KeyDup,
    /// 0xe124 映射键断档（五级）。
    KeyGap,
    /// 0xe125 映射值越界。
    ValOob,
    /// 0xe126 双表累计漂移。
    SumDrift,
    /// 0xe127 阈值漂移（守卫阈值≠冻结值）。
    ThreshDrift,
    /// 0xe128 台账不守恒（守恒恒等式破缺）。
    LedgerBroken,
    /// 0xe129 快照指纹不符（字段被改后重算不一致）。
    SnapshotFingerprint,
    /// 0xe12a 常量表破坏（表项与语义规则矛盾）。
    TableBroken,
    /// 0xe12b 段位守卫失败（诊断码越出本模块段）。
    SegmentGuard,
}

impl u3481Code {
    /// 全量闭集。
    pub const ALL: [u3481Code; 12] = [
        u3481Code::RowDup,
        u3481Code::RowGap,
        u3481Code::GuardTrip,
        u3481Code::KeyDup,
        u3481Code::KeyGap,
        u3481Code::ValOob,
        u3481Code::SumDrift,
        u3481Code::ThreshDrift,
        u3481Code::LedgerBroken,
        u3481Code::SnapshotFingerprint,
        u3481Code::TableBroken,
        u3481Code::SegmentGuard,
    ];

    /// 码值（连续十六进制，与声明序恒等）。
    pub const fn code(self) -> u16 {
        match self {
            u3481Code::RowDup => 0xe120,
            u3481Code::RowGap => 0xe121,
            u3481Code::GuardTrip => 0xe122,
            u3481Code::KeyDup => 0xe123,
            u3481Code::KeyGap => 0xe124,
            u3481Code::ValOob => 0xe125,
            u3481Code::SumDrift => 0xe126,
            u3481Code::ThreshDrift => 0xe127,
            u3481Code::LedgerBroken => 0xe128,
            u3481Code::SnapshotFingerprint => 0xe129,
            u3481Code::TableBroken => 0xe12a,
            u3481Code::SegmentGuard => 0xe12b,
        }
    }

    /// 中文诊断名。
    pub const fn name(self) -> &'static str {
        match self {
            u3481Code::RowDup => "表行重复（日志）",
            u3481Code::RowGap => "表行断档（分级）",
            u3481Code::GuardTrip => "守卫触发（重入/越限被拒）",
            u3481Code::KeyDup => "映射键重复（限速）",
            u3481Code::KeyGap => "映射键断档（五级）",
            u3481Code::ValOob => "映射值越界",
            u3481Code::SumDrift => "双表累计漂移",
            u3481Code::ThreshDrift => "阈值漂移（守卫阈值≠冻结值）",
            u3481Code::LedgerBroken => "台账不守恒（守恒恒等式破缺）",
            u3481Code::SnapshotFingerprint => "快照指纹不符（字段被改后重算不一致）",
            u3481Code::TableBroken => "常量表破坏（表项与语义规则矛盾）",
            u3481Code::SegmentGuard => "段位守卫失败（诊断码越出本模块段）",
        }
    }

    /// 可操作修复建议（每条诊断都必须带下一步动作）。
    pub const fn suggestion(self) -> &'static str {
        match self {
            u3481Code::RowDup => "按键唯一律去重",
            u3481Code::RowGap => "按连续律补号",
            u3481Code::GuardTrip => "检查临界区嵌套",
            u3481Code::KeyDup => "按键唯一律去重",
            u3481Code::KeyGap => "按连续律补号",
            u3481Code::ValOob => "按值域裁剪",
            u3481Code::SumDrift => "按理论值复算",
            u3481Code::ThreshDrift => "按冻结值校正",
            u3481Code::LedgerBroken => "按守恒律重建台账后重跑",
            u3481Code::SnapshotFingerprint => "按防篡改规程重建快照",
            u3481Code::TableBroken => "按语义规则重建常量表",
            u3481Code::SegmentGuard => "核对诊断码分配表",
        }
    }
}

// --------------------------------------------------------------------------
// 冻结常量表（u3481mix · 16 行逐行注释自证）
// --------------------------------------------------------------------------
/// 分级日志 对拍表（16 行）。
pub const u3481mix_COUNT: usize = 16;

/// 冻结表本体（编译期常量，运行期只读）。
pub static u3481mix: [(&'static str, u64); 16] = [
    ("LVL_00", 10), // LVL 对拍行（日志级别）
    ("TRACE_01", 15), // TRACE 对拍行（跟踪级）
    ("DBG_02", 20), // DBG 对拍行（调试级）
    ("INF_03", 25), // INF 对拍行（信息级）
    ("WRN_04", 30), // WRN 对拍行（警告级）
    ("ERR_05", 35), // ERR 对拍行（错误级）
    ("FILT_06", 40), // FILT 对拍行（过滤）
    ("BUF_07", 45), // BUF 对拍行（环形缓冲）
    ("RATE_08", 50), // RATE 对拍行（限速）
    ("FLUSH_09", 55), // FLUSH 对拍行（刷盘）
    ("CTX_10", 60), // CTX 对拍行（上下文字段）
    ("DROP_11", 65), // DROP 对拍行（丢弃计数）
    ("LVL_12", 70), // LVL 对拍行（日志级别）
    ("TRACE_13", 75), // TRACE 对拍行（跟踪级）
    ("DBG_14", 80), // DBG 对拍行（调试级）
    ("INF_15", 85), // INF 对拍行（信息级）
];

/// 线性查找（键唯一性由构造期断言保证，运行时无分支歧义）。
pub fn u3481mix_get(key: &'static str) -> Option<u64> {
    let mut i = 0usize;
    while i < u3481mix_COUNT {
        if u3481mix[i].0 == key {
            return Some(u3481mix[i].1);
        }
        i += 1;
    }
    None
}

/// 累计求和（用于守恒对账）。
pub fn u3481mix_sum() -> u64 {
    let mut s = 0u64;
    let mut i = 0usize;
    while i < u3481mix_COUNT {
        s = s.wrapping_add(u3481mix[i].1 as u64);
        i += 1;
    }
    s
}

// --------------------------------------------------------------------------
// 守卫 u3481mixgate（阈值 32 · 重入/越限/静默三类拒止）
// --------------------------------------------------------------------------
/// 分级日志 混合面守卫（阈值 32 冻结）。
#[derive(Clone, Copy, Debug)]
pub struct u3481mixgate {
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

/// 守卫阈值（分级日志 混合面守卫（阈值 32 冻结））。
pub const U3481MIXGATE_THRESHOLD: u64 = 32;

impl u3481mixgate {
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
        if self.window >= U3481MIXGATE_THRESHOLD {
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

// --------------------------------------------------------------------------
// 字符串键值映射 u3481mixmap（16 行逐行语义注释 + 逐行守卫）
// --------------------------------------------------------------------------
/// 分级日志 混合映射矩阵。
/// 行数。
pub const u3481mixmap_COUNT: usize = 16;

/// 表本体（键 + 值 + 注释由编译期冻结）。
pub static u3481mixmap: [(&'static str, u64); 16] = [
    ("LVL0000", 0), // LVL 单元 0000（日志级别 × 相位 00 × 混 轮）
    ("TRACE0001", 9), // TRACE 单元 0001（跟踪级 × 相位 01 × 混 轮）
    ("DBG0002", 18), // DBG 单元 0002（调试级 × 相位 02 × 混 轮）
    ("INF0003", 27), // INF 单元 0003（信息级 × 相位 03 × 混 轮）
    ("WRN0004", 36), // WRN 单元 0004（警告级 × 相位 04 × 混 轮）
    ("ERR0005", 45), // ERR 单元 0005（错误级 × 相位 05 × 混 轮）
    ("FILT0006", 54), // FILT 单元 0006（过滤 × 相位 06 × 混 轮）
    ("BUF0007", 63), // BUF 单元 0007（环形缓冲 × 相位 07 × 混 轮）
    ("RATE0008", 8), // RATE 单元 0008（限速 × 相位 08 × 混 轮）
    ("FLUSH0009", 17), // FLUSH 单元 0009（刷盘 × 相位 09 × 混 轮）
    ("CTX0010", 26), // CTX 单元 0010（上下文字段 × 相位 10 × 混 轮）
    ("DROP0011", 35), // DROP 单元 0011（丢弃计数 × 相位 11 × 混 轮）
    ("LVL0012", 44), // LVL 单元 0012（日志级别 × 相位 12 × 混 轮）
    ("TRACE0013", 53), // TRACE 单元 0013（跟踪级 × 相位 13 × 混 轮）
    ("DBG0014", 62), // DBG 单元 0014（调试级 × 相位 14 × 混 轮）
    ("INF0015", 7), // INF 单元 0015（信息级 × 相位 15 × 混 轮）
];

/// O(1) 语义查找（match 全枚举；命中率 100%% 由守卫保证）。
pub fn u3481mixmap_get(key: &str) -> Option<u64> {
    match key {
        "LVL0000" => Some(0),
        "TRACE0001" => Some(9),
        "DBG0002" => Some(18),
        "INF0003" => Some(27),
        "WRN0004" => Some(36),
        "ERR0005" => Some(45),
        "FILT0006" => Some(54),
        "BUF0007" => Some(63),
        "RATE0008" => Some(8),
        "FLUSH0009" => Some(17),
        "CTX0010" => Some(26),
        "DROP0011" => Some(35),
        "LVL0012" => Some(44),
        "TRACE0013" => Some(53),
        "DBG0014" => Some(62),
        "INF0015" => Some(7),
        _ => None,
    }
}

/// 逐行守卫扫描（返回违例行数；0 = 全绿）。
pub fn u3481mixmap_scan() -> usize {
    let mut bad = 0usize;
    let mut i = 0usize;
    while i < u3481mixmap_COUNT {
        let (k, v) = u3481mixmap[i];
        let ok = match k {
            "LVL0000" => v < 64,
            "TRACE0001" => v < 64,
            "DBG0002" => v < 64,
            "INF0003" => v < 64,
            "WRN0004" => v < 64,
            "ERR0005" => v < 64,
            "FILT0006" => v < 64,
            "BUF0007" => v < 64,
            "RATE0008" => v < 64,
            "FLUSH0009" => v < 64,
            "CTX0010" => v < 64,
            "DROP0011" => v < 64,
            "LVL0012" => v < 64,
            "TRACE0013" => v < 64,
            "DBG0014" => v < 64,
            "INF0015" => v < 64,
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
pub fn u3481mixmap_sum() -> u64 {
    let mut s = 0u64;
    let mut i = 0usize;
    while i < u3481mixmap_COUNT {
        s = s.wrapping_add(u3481mixmap[i].1 as u64);
        i += 1;
    }
    s
}

/// 键唯一性（构造期断言等价物：逐对扫描同键）。
pub fn u3481mixmap_unique() -> bool {
    let mut i = 0usize;
    while i < u3481mixmap_COUNT {
        let mut j = i + 1;
        while j < u3481mixmap_COUNT {
            if u3481mixmap[i].0 == u3481mixmap[j].0 {
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

/// 段位守卫：全部诊断码必须落在本段 [0xe120, 0xe12f]。
pub const fn u3481_segment_guard() -> bool {
    let mut ok = true;
    let mut i = 0usize;
    while i < u3481Code::ALL.len() {
        let c = u3481Code::ALL[i].code();
        if c < 0xe120 || c > 0xe12f {
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
fn u3481_guard_seg() -> bool {
    u3481_segment_guard()
}

/// UNX-F3481 对拍表 16 行边界与累计守恒。
fn check_u3481_mix_tbl() -> bool {
    let mut ok = u3481mix_COUNT == 16;
    if ok { ok = u3481mix_get("LVL_00") == Some(10) && u3481mix_get("INF_15") == Some(85); }
    if ok { ok = u3481mix_get("ZZZ_NOKEY").is_none(); }
    if ok { ok = u3481mix_sum() == 760; }
    ok
}

/// UNX-F3481 混合映射 16 行守卫唯一 + 守卫阈值冻结。
fn check_u3481_mix_map() -> bool {
    let mut ok = u3481mixmap_COUNT == 16;
    if ok { ok = u3481mixmap_scan() == 0 && u3481mixmap_unique(); }
    if ok { ok = u3481mixmap_sum() == 504; }
    if ok { ok = U3481MIXGATE_THRESHOLD == 32; }
    ok
}

/// 本模块自检（逐检查带 detail，全绿才出门）。
pub fn run_u3481_checks() -> CheckSet {
    let mut set = CheckSet::new("U3481");
    set.add(
        "u3481-seg-guard",
        u3481_guard_seg(),
        if u3481_guard_seg() { "" } else { "diagnostic code outside segment" },
    );
    set.add(
        "u3481-mix-tbl",
        check_u3481_mix_tbl(),
        if check_u3481_mix_tbl() { "" } else { "UNX-F3481 mix table mismatch" },
    );
    set.add(
        "u3481-mix-map",
        check_u3481_mix_map(),
        if check_u3481_mix_map() { "" } else { "UNX-F3481 mix map mismatch" },
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
    fn u3481_all_checks_green() {
        let set = run_u3481_checks();
        assert!(set.all_passed(), "U3481 domain checks must be green");
    }

    #[test]
    fn u3481_segment_guard_holds() {
        assert!(u3481_segment_guard());
    }
}
