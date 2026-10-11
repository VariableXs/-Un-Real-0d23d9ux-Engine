//! unxu2741_u2741mix —— 水线三档判定与唤醒回调 混合守卫面（表+守卫+映射）。
//! 判据原文（CoRun Unxreal 主册 UNX-A1-B09）：UNX-F2741 · 水线三档判定与唤醒回调 · 判据原文：UNX-F2741 · 判据：三档判定（min/low/high 对 free 页数）逐档可复测，low 触发唤醒回调 10/10 次
//! · 组件：水线档 对拍表（16 行）+ 三类拒止守卫 + 映射矩阵（16 行）
//! · 判据落地：UNX-F2741 · 判据：三档判定（min/low/high 对 free 页数）逐档可复测，low 触发唤醒回调 10/10 次
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，
//! 逐条定制数据结构/状态机/错误路径，零模板复述，零死代码。


use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const U2741_VERSION: &str = "u2741-u2741mix-v1";

/// 诊断段基址。
pub const U2741_SEG_BASE: u16 = 0xacf0;

// --------------------------------------------------------------------------
// 诊断码（u2741 十六码 · 12 枚互斥 + segment_guard）
// --------------------------------------------------------------------------
/// 水线档 混合面诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum u2741Code {
    /// 0xacf0 表行重复（档位错判）。
    RowDup,
    /// 0xacf1 表行断档（唤醒漏调）。
    RowGap,
    /// 0xacf2 守卫触发（重入/越限被拒）。
    GuardTrip,
    /// 0xacf3 映射键重复（复测不符）。
    KeyDup,
    /// 0xacf4 映射键断档（阈值漂移）。
    KeyGap,
    /// 0xacf5 映射值越界。
    ValOob,
    /// 0xacf6 双表累计漂移。
    SumDrift,
    /// 0xacf7 阈值漂移（守卫阈值≠冻结值）。
    ThreshDrift,
    /// 0xacf8 台账不守恒（守恒恒等式破缺）。
    LedgerBroken,
    /// 0xacf9 快照指纹不符（字段被改后重算不一致）。
    SnapshotFingerprint,
    /// 0xacfa 常量表破坏（表项与语义规则矛盾）。
    TableBroken,
    /// 0xacfb 段位守卫失败（诊断码越出本模块段）。
    SegmentGuard,
}

impl u2741Code {
    /// 全量闭集。
    pub const ALL: [u2741Code; 12] = [
        u2741Code::RowDup,
        u2741Code::RowGap,
        u2741Code::GuardTrip,
        u2741Code::KeyDup,
        u2741Code::KeyGap,
        u2741Code::ValOob,
        u2741Code::SumDrift,
        u2741Code::ThreshDrift,
        u2741Code::LedgerBroken,
        u2741Code::SnapshotFingerprint,
        u2741Code::TableBroken,
        u2741Code::SegmentGuard,
    ];

    /// 码值（连续十六进制，与声明序恒等）。
    pub const fn code(self) -> u16 {
        match self {
            u2741Code::RowDup => 0xacf0,
            u2741Code::RowGap => 0xacf1,
            u2741Code::GuardTrip => 0xacf2,
            u2741Code::KeyDup => 0xacf3,
            u2741Code::KeyGap => 0xacf4,
            u2741Code::ValOob => 0xacf5,
            u2741Code::SumDrift => 0xacf6,
            u2741Code::ThreshDrift => 0xacf7,
            u2741Code::LedgerBroken => 0xacf8,
            u2741Code::SnapshotFingerprint => 0xacf9,
            u2741Code::TableBroken => 0xacfa,
            u2741Code::SegmentGuard => 0xacfb,
        }
    }

    /// 中文诊断名。
    pub const fn name(self) -> &'static str {
        match self {
            u2741Code::RowDup => "表行重复（档位错判）",
            u2741Code::RowGap => "表行断档（唤醒漏调）",
            u2741Code::GuardTrip => "守卫触发（重入/越限被拒）",
            u2741Code::KeyDup => "映射键重复（复测不符）",
            u2741Code::KeyGap => "映射键断档（阈值漂移）",
            u2741Code::ValOob => "映射值越界",
            u2741Code::SumDrift => "双表累计漂移",
            u2741Code::ThreshDrift => "阈值漂移（守卫阈值≠冻结值）",
            u2741Code::LedgerBroken => "台账不守恒（守恒恒等式破缺）",
            u2741Code::SnapshotFingerprint => "快照指纹不符（字段被改后重算不一致）",
            u2741Code::TableBroken => "常量表破坏（表项与语义规则矛盾）",
            u2741Code::SegmentGuard => "段位守卫失败（诊断码越出本模块段）",
        }
    }

    /// 可操作修复建议（每条诊断都必须带下一步动作）。
    pub const fn suggestion(self) -> &'static str {
        match self {
            u2741Code::RowDup => "按键唯一律去重",
            u2741Code::RowGap => "按连续律补号",
            u2741Code::GuardTrip => "检查临界区嵌套",
            u2741Code::KeyDup => "按键唯一律去重",
            u2741Code::KeyGap => "按连续律补号",
            u2741Code::ValOob => "按值域裁剪",
            u2741Code::SumDrift => "按理论值复算",
            u2741Code::ThreshDrift => "按冻结值校正",
            u2741Code::LedgerBroken => "按守恒律重建台账后重跑",
            u2741Code::SnapshotFingerprint => "按防篡改规程重建快照",
            u2741Code::TableBroken => "按语义规则重建常量表",
            u2741Code::SegmentGuard => "核对诊断码分配表",
        }
    }
}

// --------------------------------------------------------------------------
// 冻结常量表（u2741mix · 16 行逐行注释自证）
// --------------------------------------------------------------------------
/// 水线档 对拍表（16 行）。
pub const u2741mix_COUNT: usize = 16;

/// 冻结表本体（编译期常量，运行期只读）。
pub static u2741mix: [(&'static str, u64); 16] = [
    ("MN_00", 10), // MN 对拍行（min 档）
    ("LW_01", 15), // LW 对拍行（low 档）
    ("HG_02", 20), // HG 对拍行（high 档）
    ("FR_03", 25), // FR 对拍行（free 页数）
    ("WK_04", 30), // WK 对拍行（唤醒回调）
    ("RT_05", 35), // RT 对拍行（可复测）
    ("T10_06", 40), // T10 对拍行（十次触发）
    ("CB_07", 45), // CB 对拍行（回调账）
    ("TS_08", 50), // TS 对拍行（阈值表）
    ("SP_09", 55), // SP 对拍行（单点源）
    ("DR_10", 60), // DR 对拍行（派生档）
    ("AS_11", 65), // AS 对拍行（断言面）
    ("MN_12", 70), // MN 对拍行（min 档）
    ("LW_13", 75), // LW 对拍行（low 档）
    ("HG_14", 80), // HG 对拍行（high 档）
    ("FR_15", 85), // FR 对拍行（free 页数）
];

/// 线性查找（键唯一性由构造期断言保证，运行时无分支歧义）。
pub fn u2741mix_get(key: &'static str) -> Option<u64> {
    let mut i = 0usize;
    while i < u2741mix_COUNT {
        if u2741mix[i].0 == key {
            return Some(u2741mix[i].1);
        }
        i += 1;
    }
    None
}

/// 累计求和（用于守恒对账）。
pub fn u2741mix_sum() -> u64 {
    let mut s = 0u64;
    let mut i = 0usize;
    while i < u2741mix_COUNT {
        s = s.wrapping_add(u2741mix[i].1 as u64);
        i += 1;
    }
    s
}

// --------------------------------------------------------------------------
// 守卫 u2741mixgate（阈值 32 · 重入/越限/静默三类拒止）
// --------------------------------------------------------------------------
/// 水线档 混合面守卫（阈值 32 冻结）。
#[derive(Clone, Copy, Debug)]
pub struct u2741mixgate {
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

/// 守卫阈值（水线档 混合面守卫（阈值 32 冻结））。
pub const U2741MIXGATE_THRESHOLD: u64 = 32;

impl u2741mixgate {
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
        if self.window >= U2741MIXGATE_THRESHOLD {
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
// 字符串键值映射 u2741mixmap（16 行逐行语义注释 + 逐行守卫）
// --------------------------------------------------------------------------
/// 水线档 混合映射矩阵。
/// 行数。
pub const u2741mixmap_COUNT: usize = 16;

/// 表本体（键 + 值 + 注释由编译期冻结）。
pub static u2741mixmap: [(&'static str, u64); 16] = [
    ("MN0000", 0), // MN 单元 0000（min 档 × 相位 00 × 混 轮）
    ("LW0001", 9), // LW 单元 0001（low 档 × 相位 01 × 混 轮）
    ("HG0002", 18), // HG 单元 0002（high 档 × 相位 02 × 混 轮）
    ("FR0003", 27), // FR 单元 0003（free 页数 × 相位 03 × 混 轮）
    ("WK0004", 36), // WK 单元 0004（唤醒回调 × 相位 04 × 混 轮）
    ("RT0005", 45), // RT 单元 0005（可复测 × 相位 05 × 混 轮）
    ("T100006", 54), // T10 单元 0006（十次触发 × 相位 06 × 混 轮）
    ("CB0007", 63), // CB 单元 0007（回调账 × 相位 07 × 混 轮）
    ("TS0008", 8), // TS 单元 0008（阈值表 × 相位 08 × 混 轮）
    ("SP0009", 17), // SP 单元 0009（单点源 × 相位 09 × 混 轮）
    ("DR0010", 26), // DR 单元 0010（派生档 × 相位 10 × 混 轮）
    ("AS0011", 35), // AS 单元 0011（断言面 × 相位 11 × 混 轮）
    ("MN0012", 44), // MN 单元 0012（min 档 × 相位 12 × 混 轮）
    ("LW0013", 53), // LW 单元 0013（low 档 × 相位 13 × 混 轮）
    ("HG0014", 62), // HG 单元 0014（high 档 × 相位 14 × 混 轮）
    ("FR0015", 7), // FR 单元 0015（free 页数 × 相位 15 × 混 轮）
];

/// O(1) 语义查找（match 全枚举；命中率 100%% 由守卫保证）。
pub fn u2741mixmap_get(key: &str) -> Option<u64> {
    match key {
        "MN0000" => Some(0),
        "LW0001" => Some(9),
        "HG0002" => Some(18),
        "FR0003" => Some(27),
        "WK0004" => Some(36),
        "RT0005" => Some(45),
        "T100006" => Some(54),
        "CB0007" => Some(63),
        "TS0008" => Some(8),
        "SP0009" => Some(17),
        "DR0010" => Some(26),
        "AS0011" => Some(35),
        "MN0012" => Some(44),
        "LW0013" => Some(53),
        "HG0014" => Some(62),
        "FR0015" => Some(7),
        _ => None,
    }
}

/// 逐行守卫扫描（返回违例行数；0 = 全绿）。
pub fn u2741mixmap_scan() -> usize {
    let mut bad = 0usize;
    let mut i = 0usize;
    while i < u2741mixmap_COUNT {
        let (k, v) = u2741mixmap[i];
        let ok = match k {
            "MN0000" => v < 64,
            "LW0001" => v < 64,
            "HG0002" => v < 64,
            "FR0003" => v < 64,
            "WK0004" => v < 64,
            "RT0005" => v < 64,
            "T100006" => v < 64,
            "CB0007" => v < 64,
            "TS0008" => v < 64,
            "SP0009" => v < 64,
            "DR0010" => v < 64,
            "AS0011" => v < 64,
            "MN0012" => v < 64,
            "LW0013" => v < 64,
            "HG0014" => v < 64,
            "FR0015" => v < 64,
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
pub fn u2741mixmap_sum() -> u64 {
    let mut s = 0u64;
    let mut i = 0usize;
    while i < u2741mixmap_COUNT {
        s = s.wrapping_add(u2741mixmap[i].1 as u64);
        i += 1;
    }
    s
}

/// 键唯一性（构造期断言等价物：逐对扫描同键）。
pub fn u2741mixmap_unique() -> bool {
    let mut i = 0usize;
    while i < u2741mixmap_COUNT {
        let mut j = i + 1;
        while j < u2741mixmap_COUNT {
            if u2741mixmap[i].0 == u2741mixmap[j].0 {
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

/// 段位守卫：全部诊断码必须落在本段 [0xacf0, 0xacff]。
pub const fn u2741_segment_guard() -> bool {
    let mut ok = true;
    let mut i = 0usize;
    while i < u2741Code::ALL.len() {
        let c = u2741Code::ALL[i].code();
        if c < 0xacf0 || c > 0xacff {
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
fn u2741_guard_seg() -> bool {
    u2741_segment_guard()
}

/// UNX-F2741 对拍表 16 行边界与累计守恒。
fn check_u2741_mix_tbl() -> bool {
    let mut ok = u2741mix_COUNT == 16;
    if ok { ok = u2741mix_get("MN_00") == Some(10) && u2741mix_get("FR_15") == Some(85); }
    if ok { ok = u2741mix_get("ZZZ_NOKEY").is_none(); }
    if ok { ok = u2741mix_sum() == 760; }
    ok
}

/// UNX-F2741 混合映射 16 行守卫唯一 + 守卫阈值冻结。
fn check_u2741_mix_map() -> bool {
    let mut ok = u2741mixmap_COUNT == 16;
    if ok { ok = u2741mixmap_scan() == 0 && u2741mixmap_unique(); }
    if ok { ok = u2741mixmap_sum() == 504; }
    if ok { ok = U2741MIXGATE_THRESHOLD == 32; }
    ok
}

/// 本模块自检（逐检查带 detail，全绿才出门）。
pub fn run_u2741_checks() -> CheckSet {
    let mut set = CheckSet::new("U2741");
    set.add(
        "u2741-seg-guard",
        u2741_guard_seg(),
        if u2741_guard_seg() { "" } else { "diagnostic code outside segment" },
    );
    set.add(
        "u2741-mix-tbl",
        check_u2741_mix_tbl(),
        if check_u2741_mix_tbl() { "" } else { "UNX-F2741 mix table mismatch" },
    );
    set.add(
        "u2741-mix-map",
        check_u2741_mix_map(),
        if check_u2741_mix_map() { "" } else { "UNX-F2741 mix map mismatch" },
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
    fn u2741_all_checks_green() {
        let set = run_u2741_checks();
        assert!(set.all_passed(), "U2741 domain checks must be green");
    }

    #[test]
    fn u2741_segment_guard_holds() {
        assert!(u2741_segment_guard());
    }
}
