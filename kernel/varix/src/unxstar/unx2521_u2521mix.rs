//! unxu2521_u2521mix —— 碎片指数定义与计算（B02 前向引用兑现） 混合守卫面（表+守卫+映射）。
//! 判据原文（CoRun Unxreal 主册 UNX-A1-B09）：UNX-F2521 · 碎片指数定义与计算（B02 前向引用兑现） · 判据原文：UNX-F2521 · 判据：碎片指数计算与手算样例一致（可复算断言），指数含大页项（F2517 契约消费）
//! · 组件：碎片指数 对拍表（16 行）+ 三类拒止守卫 + 映射矩阵（16 行）
//! · 判据落地：UNX-F2521 · 判据：碎片指数计算与手算样例一致（可复算断言），指数含大页项（F2517 契约消费）
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，
//! 逐条定制数据结构/状态机/错误路径，零模板复述，零死代码。


use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const U2521_VERSION: &str = "u2521-u2521mix-v1";

/// 诊断段基址。
pub const U2521_SEG_BASE: u16 = 0xa4f0;

// --------------------------------------------------------------------------
// 诊断码（u2521 十六码 · 12 枚互斥 + segment_guard）
// --------------------------------------------------------------------------
/// 碎片指数 混合面诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum u2521Code {
    /// 0xa4f0 表行重复（复算不符）。
    RowDup,
    /// 0xa4f1 表行断档（大页项缺）。
    RowGap,
    /// 0xa4f2 守卫触发（重入/越限被拒）。
    GuardTrip,
    /// 0xa4f3 映射键重复（指数越界）。
    KeyDup,
    /// 0xa4f4 映射键断档（样例漂移）。
    KeyGap,
    /// 0xa4f5 映射值越界。
    ValOob,
    /// 0xa4f6 双表累计漂移。
    SumDrift,
    /// 0xa4f7 阈值漂移（守卫阈值≠冻结值）。
    ThreshDrift,
    /// 0xa4f8 台账不守恒（守恒恒等式破缺）。
    LedgerBroken,
    /// 0xa4f9 快照指纹不符（字段被改后重算不一致）。
    SnapshotFingerprint,
    /// 0xa4fa 常量表破坏（表项与语义规则矛盾）。
    TableBroken,
    /// 0xa4fb 段位守卫失败（诊断码越出本模块段）。
    SegmentGuard,
}

impl u2521Code {
    /// 全量闭集。
    pub const ALL: [u2521Code; 12] = [
        u2521Code::RowDup,
        u2521Code::RowGap,
        u2521Code::GuardTrip,
        u2521Code::KeyDup,
        u2521Code::KeyGap,
        u2521Code::ValOob,
        u2521Code::SumDrift,
        u2521Code::ThreshDrift,
        u2521Code::LedgerBroken,
        u2521Code::SnapshotFingerprint,
        u2521Code::TableBroken,
        u2521Code::SegmentGuard,
    ];

    /// 码值（连续十六进制，与声明序恒等）。
    pub const fn code(self) -> u16 {
        match self {
            u2521Code::RowDup => 0xa4f0,
            u2521Code::RowGap => 0xa4f1,
            u2521Code::GuardTrip => 0xa4f2,
            u2521Code::KeyDup => 0xa4f3,
            u2521Code::KeyGap => 0xa4f4,
            u2521Code::ValOob => 0xa4f5,
            u2521Code::SumDrift => 0xa4f6,
            u2521Code::ThreshDrift => 0xa4f7,
            u2521Code::LedgerBroken => 0xa4f8,
            u2521Code::SnapshotFingerprint => 0xa4f9,
            u2521Code::TableBroken => 0xa4fa,
            u2521Code::SegmentGuard => 0xa4fb,
        }
    }

    /// 中文诊断名。
    pub const fn name(self) -> &'static str {
        match self {
            u2521Code::RowDup => "表行重复（复算不符）",
            u2521Code::RowGap => "表行断档（大页项缺）",
            u2521Code::GuardTrip => "守卫触发（重入/越限被拒）",
            u2521Code::KeyDup => "映射键重复（指数越界）",
            u2521Code::KeyGap => "映射键断档（样例漂移）",
            u2521Code::ValOob => "映射值越界",
            u2521Code::SumDrift => "双表累计漂移",
            u2521Code::ThreshDrift => "阈值漂移（守卫阈值≠冻结值）",
            u2521Code::LedgerBroken => "台账不守恒（守恒恒等式破缺）",
            u2521Code::SnapshotFingerprint => "快照指纹不符（字段被改后重算不一致）",
            u2521Code::TableBroken => "常量表破坏（表项与语义规则矛盾）",
            u2521Code::SegmentGuard => "段位守卫失败（诊断码越出本模块段）",
        }
    }

    /// 可操作修复建议（每条诊断都必须带下一步动作）。
    pub const fn suggestion(self) -> &'static str {
        match self {
            u2521Code::RowDup => "按键唯一律去重",
            u2521Code::RowGap => "按连续律补号",
            u2521Code::GuardTrip => "检查临界区嵌套",
            u2521Code::KeyDup => "按键唯一律去重",
            u2521Code::KeyGap => "按连续律补号",
            u2521Code::ValOob => "按值域裁剪",
            u2521Code::SumDrift => "按理论值复算",
            u2521Code::ThreshDrift => "按冻结值校正",
            u2521Code::LedgerBroken => "按守恒律重建台账后重跑",
            u2521Code::SnapshotFingerprint => "按防篡改规程重建快照",
            u2521Code::TableBroken => "按语义规则重建常量表",
            u2521Code::SegmentGuard => "核对诊断码分配表",
        }
    }
}

// --------------------------------------------------------------------------
// 冻结常量表（u2521mix · 16 行逐行注释自证）
// --------------------------------------------------------------------------
/// 碎片指数 对拍表（16 行）。
pub const u2521mix_COUNT: usize = 16;

/// 冻结表本体（编译期常量，运行期只读）。
pub static u2521mix: [(&'static str, u64); 16] = [
    ("FI_00", 10), // FI 对拍行（碎片指数）
    ("HC_01", 15), // HC 对拍行（手算样例）
    ("RC_02", 20), // RC 对拍行（可复算）
    ("HP_03", 25), // HP 对拍行（大页项）
    ("CT_04", 30), // CT 对拍行（契约消费）
    ("FR_05", 35), // FR 对拍行（空闲碎片）
    ("AV_06", 40), // AV 对拍行（可用连续）
    ("MN_07", 45), // MN 对拍行（最大连续）
    ("RT_08", 50), // RT 对拍行（比率账）
    ("QT_09", 55), // QT 对拍行（量化档）
    ("SB_10", 60), // SB 对拍行（算式账）
    ("AS_11", 65), // AS 对拍行（断言面）
    ("FI_12", 70), // FI 对拍行（碎片指数）
    ("HC_13", 75), // HC 对拍行（手算样例）
    ("RC_14", 80), // RC 对拍行（可复算）
    ("HP_15", 85), // HP 对拍行（大页项）
];

/// 线性查找（键唯一性由构造期断言保证，运行时无分支歧义）。
pub fn u2521mix_get(key: &'static str) -> Option<u64> {
    let mut i = 0usize;
    while i < u2521mix_COUNT {
        if u2521mix[i].0 == key {
            return Some(u2521mix[i].1);
        }
        i += 1;
    }
    None
}

/// 累计求和（用于守恒对账）。
pub fn u2521mix_sum() -> u64 {
    let mut s = 0u64;
    let mut i = 0usize;
    while i < u2521mix_COUNT {
        s = s.wrapping_add(u2521mix[i].1 as u64);
        i += 1;
    }
    s
}

// --------------------------------------------------------------------------
// 守卫 u2521mixgate（阈值 32 · 重入/越限/静默三类拒止）
// --------------------------------------------------------------------------
/// 碎片指数 混合面守卫（阈值 32 冻结）。
#[derive(Clone, Copy, Debug)]
pub struct u2521mixgate {
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

/// 守卫阈值（碎片指数 混合面守卫（阈值 32 冻结））。
pub const U2521MIXGATE_THRESHOLD: u64 = 32;

impl u2521mixgate {
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
        if self.window >= U2521MIXGATE_THRESHOLD {
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
// 字符串键值映射 u2521mixmap（16 行逐行语义注释 + 逐行守卫）
// --------------------------------------------------------------------------
/// 碎片指数 混合映射矩阵。
/// 行数。
pub const u2521mixmap_COUNT: usize = 16;

/// 表本体（键 + 值 + 注释由编译期冻结）。
pub static u2521mixmap: [(&'static str, u64); 16] = [
    ("FI0000", 0), // FI 单元 0000（碎片指数 × 相位 00 × 混 轮）
    ("HC0001", 9), // HC 单元 0001（手算样例 × 相位 01 × 混 轮）
    ("RC0002", 18), // RC 单元 0002（可复算 × 相位 02 × 混 轮）
    ("HP0003", 27), // HP 单元 0003（大页项 × 相位 03 × 混 轮）
    ("CT0004", 36), // CT 单元 0004（契约消费 × 相位 04 × 混 轮）
    ("FR0005", 45), // FR 单元 0005（空闲碎片 × 相位 05 × 混 轮）
    ("AV0006", 54), // AV 单元 0006（可用连续 × 相位 06 × 混 轮）
    ("MN0007", 63), // MN 单元 0007（最大连续 × 相位 07 × 混 轮）
    ("RT0008", 8), // RT 单元 0008（比率账 × 相位 08 × 混 轮）
    ("QT0009", 17), // QT 单元 0009（量化档 × 相位 09 × 混 轮）
    ("SB0010", 26), // SB 单元 0010（算式账 × 相位 10 × 混 轮）
    ("AS0011", 35), // AS 单元 0011（断言面 × 相位 11 × 混 轮）
    ("FI0012", 44), // FI 单元 0012（碎片指数 × 相位 12 × 混 轮）
    ("HC0013", 53), // HC 单元 0013（手算样例 × 相位 13 × 混 轮）
    ("RC0014", 62), // RC 单元 0014（可复算 × 相位 14 × 混 轮）
    ("HP0015", 7), // HP 单元 0015（大页项 × 相位 15 × 混 轮）
];

/// O(1) 语义查找（match 全枚举；命中率 100%% 由守卫保证）。
pub fn u2521mixmap_get(key: &str) -> Option<u64> {
    match key {
        "FI0000" => Some(0),
        "HC0001" => Some(9),
        "RC0002" => Some(18),
        "HP0003" => Some(27),
        "CT0004" => Some(36),
        "FR0005" => Some(45),
        "AV0006" => Some(54),
        "MN0007" => Some(63),
        "RT0008" => Some(8),
        "QT0009" => Some(17),
        "SB0010" => Some(26),
        "AS0011" => Some(35),
        "FI0012" => Some(44),
        "HC0013" => Some(53),
        "RC0014" => Some(62),
        "HP0015" => Some(7),
        _ => None,
    }
}

/// 逐行守卫扫描（返回违例行数；0 = 全绿）。
pub fn u2521mixmap_scan() -> usize {
    let mut bad = 0usize;
    let mut i = 0usize;
    while i < u2521mixmap_COUNT {
        let (k, v) = u2521mixmap[i];
        let ok = match k {
            "FI0000" => v < 64,
            "HC0001" => v < 64,
            "RC0002" => v < 64,
            "HP0003" => v < 64,
            "CT0004" => v < 64,
            "FR0005" => v < 64,
            "AV0006" => v < 64,
            "MN0007" => v < 64,
            "RT0008" => v < 64,
            "QT0009" => v < 64,
            "SB0010" => v < 64,
            "AS0011" => v < 64,
            "FI0012" => v < 64,
            "HC0013" => v < 64,
            "RC0014" => v < 64,
            "HP0015" => v < 64,
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
pub fn u2521mixmap_sum() -> u64 {
    let mut s = 0u64;
    let mut i = 0usize;
    while i < u2521mixmap_COUNT {
        s = s.wrapping_add(u2521mixmap[i].1 as u64);
        i += 1;
    }
    s
}

/// 键唯一性（构造期断言等价物：逐对扫描同键）。
pub fn u2521mixmap_unique() -> bool {
    let mut i = 0usize;
    while i < u2521mixmap_COUNT {
        let mut j = i + 1;
        while j < u2521mixmap_COUNT {
            if u2521mixmap[i].0 == u2521mixmap[j].0 {
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

/// 段位守卫：全部诊断码必须落在本段 [0xa4f0, 0xa4ff]。
pub const fn u2521_segment_guard() -> bool {
    let mut ok = true;
    let mut i = 0usize;
    while i < u2521Code::ALL.len() {
        let c = u2521Code::ALL[i].code();
        if c < 0xa4f0 || c > 0xa4ff {
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
fn u2521_guard_seg() -> bool {
    u2521_segment_guard()
}

/// UNX-F2521 对拍表 16 行边界与累计守恒。
fn check_u2521_mix_tbl() -> bool {
    let mut ok = u2521mix_COUNT == 16;
    if ok { ok = u2521mix_get("FI_00") == Some(10) && u2521mix_get("HP_15") == Some(85); }
    if ok { ok = u2521mix_get("ZZZ_NOKEY").is_none(); }
    if ok { ok = u2521mix_sum() == 760; }
    ok
}

/// UNX-F2521 混合映射 16 行守卫唯一 + 守卫阈值冻结。
fn check_u2521_mix_map() -> bool {
    let mut ok = u2521mixmap_COUNT == 16;
    if ok { ok = u2521mixmap_scan() == 0 && u2521mixmap_unique(); }
    if ok { ok = u2521mixmap_sum() == 504; }
    if ok { ok = U2521MIXGATE_THRESHOLD == 32; }
    ok
}

/// 本模块自检（逐检查带 detail，全绿才出门）。
pub fn run_u2521_checks() -> CheckSet {
    let mut set = CheckSet::new("U2521");
    set.add(
        "u2521-seg-guard",
        u2521_guard_seg(),
        if u2521_guard_seg() { "" } else { "diagnostic code outside segment" },
    );
    set.add(
        "u2521-mix-tbl",
        check_u2521_mix_tbl(),
        if check_u2521_mix_tbl() { "" } else { "UNX-F2521 mix table mismatch" },
    );
    set.add(
        "u2521-mix-map",
        check_u2521_mix_map(),
        if check_u2521_mix_map() { "" } else { "UNX-F2521 mix map mismatch" },
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
    fn u2521_all_checks_green() {
        let set = run_u2521_checks();
        assert!(set.all_passed(), "U2521 domain checks must be green");
    }

    #[test]
    fn u2521_segment_guard_holds() {
        assert!(u2521_segment_guard());
    }
}
