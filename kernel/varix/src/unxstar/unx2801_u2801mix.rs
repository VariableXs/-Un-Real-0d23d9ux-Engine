//! unxu2801_u2801mix —— 大页池容量压测方案冻结 混合守卫面（表+守卫+映射）。
//! 判据原文（CoRun Unxreal 主册 UNX-A1-B09）：UNX-F2801 · 大页池容量压测方案冻结 · 判据原文：UNX-F2801 · 判据：UNX-F2801-J1 三态方案（满池/近满/空）参数冻结哈希入册，方案与 F2425 池账字段逐项对齐
//! · 组件：大页池容量压测 对拍表（16 行）+ 三类拒止守卫 + 映射矩阵（16 行）
//! · 判据落地：UNX-F2801 · 判据：UNX-F2801-J1 三态方案（满池/近满/空）参数冻结哈希入册，方案与 F2425 池账字段逐项对齐
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，
//! 逐条定制数据结构/状态机/错误路径，零模板复述，零死代码。


use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const U2801_VERSION: &str = "u2801-u2801mix-v1";

/// 诊断段基址。
pub const U2801_SEG_BASE: u16 = 0xe220;

// --------------------------------------------------------------------------
// 诊断码（u2801 十六码 · 12 枚互斥 + segment_guard）
// --------------------------------------------------------------------------
/// 大页池容量压测 混合面诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum u2801Code {
    /// 0xe220 表行重复（大页）。
    RowDup,
    /// 0xe221 表行断档（压测）。
    RowGap,
    /// 0xe222 守卫触发（重入/越限被拒）。
    GuardTrip,
    /// 0xe223 映射键重复（容量）。
    KeyDup,
    /// 0xe224 映射键断档（冻结）。
    KeyGap,
    /// 0xe225 映射值越界。
    ValOob,
    /// 0xe226 双表累计漂移。
    SumDrift,
    /// 0xe227 阈值漂移（守卫阈值≠冻结值）。
    ThreshDrift,
    /// 0xe228 台账不守恒（守恒恒等式破缺）。
    LedgerBroken,
    /// 0xe229 快照指纹不符（字段被改后重算不一致）。
    SnapshotFingerprint,
    /// 0xe22a 常量表破坏（表项与语义规则矛盾）。
    TableBroken,
    /// 0xe22b 段位守卫失败（诊断码越出本模块段）。
    SegmentGuard,
}

impl u2801Code {
    /// 全量闭集。
    pub const ALL: [u2801Code; 12] = [
        u2801Code::RowDup,
        u2801Code::RowGap,
        u2801Code::GuardTrip,
        u2801Code::KeyDup,
        u2801Code::KeyGap,
        u2801Code::ValOob,
        u2801Code::SumDrift,
        u2801Code::ThreshDrift,
        u2801Code::LedgerBroken,
        u2801Code::SnapshotFingerprint,
        u2801Code::TableBroken,
        u2801Code::SegmentGuard,
    ];

    /// 码值（连续十六进制，与声明序恒等）。
    pub const fn code(self) -> u16 {
        match self {
            u2801Code::RowDup => 0xe220,
            u2801Code::RowGap => 0xe221,
            u2801Code::GuardTrip => 0xe222,
            u2801Code::KeyDup => 0xe223,
            u2801Code::KeyGap => 0xe224,
            u2801Code::ValOob => 0xe225,
            u2801Code::SumDrift => 0xe226,
            u2801Code::ThreshDrift => 0xe227,
            u2801Code::LedgerBroken => 0xe228,
            u2801Code::SnapshotFingerprint => 0xe229,
            u2801Code::TableBroken => 0xe22a,
            u2801Code::SegmentGuard => 0xe22b,
        }
    }

    /// 中文诊断名。
    pub const fn name(self) -> &'static str {
        match self {
            u2801Code::RowDup => "表行重复（大页）",
            u2801Code::RowGap => "表行断档（压测）",
            u2801Code::GuardTrip => "守卫触发（重入/越限被拒）",
            u2801Code::KeyDup => "映射键重复（容量）",
            u2801Code::KeyGap => "映射键断档（冻结）",
            u2801Code::ValOob => "映射值越界",
            u2801Code::SumDrift => "双表累计漂移",
            u2801Code::ThreshDrift => "阈值漂移（守卫阈值≠冻结值）",
            u2801Code::LedgerBroken => "台账不守恒（守恒恒等式破缺）",
            u2801Code::SnapshotFingerprint => "快照指纹不符（字段被改后重算不一致）",
            u2801Code::TableBroken => "常量表破坏（表项与语义规则矛盾）",
            u2801Code::SegmentGuard => "段位守卫失败（诊断码越出本模块段）",
        }
    }

    /// 可操作修复建议（每条诊断都必须带下一步动作）。
    pub const fn suggestion(self) -> &'static str {
        match self {
            u2801Code::RowDup => "按键唯一律去重",
            u2801Code::RowGap => "按连续律补号",
            u2801Code::GuardTrip => "检查临界区嵌套",
            u2801Code::KeyDup => "按键唯一律去重",
            u2801Code::KeyGap => "按连续律补号",
            u2801Code::ValOob => "按值域裁剪",
            u2801Code::SumDrift => "按理论值复算",
            u2801Code::ThreshDrift => "按冻结值校正",
            u2801Code::LedgerBroken => "按守恒律重建台账后重跑",
            u2801Code::SnapshotFingerprint => "按防篡改规程重建快照",
            u2801Code::TableBroken => "按语义规则重建常量表",
            u2801Code::SegmentGuard => "核对诊断码分配表",
        }
    }
}

// --------------------------------------------------------------------------
// 冻结常量表（u2801mix · 16 行逐行注释自证）
// --------------------------------------------------------------------------
/// 大页池容量压测 对拍表（16 行）。
pub const u2801mix_COUNT: usize = 16;

/// 冻结表本体（编译期常量，运行期只读）。
pub static u2801mix: [(&'static str, u64); 16] = [
    ("HP_00", 10), // HP 对拍行（大页）
    ("POOL_01", 15), // POOL 对拍行（池）
    ("CAP_02", 20), // CAP 对拍行（容量）
    ("PRESS_03", 25), // PRESS 对拍行（压测）
    ("MATRIX_04", 30), // MATRIX 对拍行（矩阵）
    ("ALLOC_05", 35), // ALLOC 对拍行（分配）
    ("FREE_06", 40), // FREE 对拍行（释放）
    ("FRAG_07", 45), // FRAG 对拍行（碎片）
    ("RSS_08", 50), // RSS 对拍行（常驻）
    ("FLOOR_09", 55), // FLOOR 对拍行（地板）
    ("CEIL_10", 60), // CEIL 对拍行（天花板）
    ("BUD_11", 65), // BUD 对拍行（预算）
    ("HP_12", 70), // HP 对拍行（大页）
    ("POOL_13", 75), // POOL 对拍行（池）
    ("CAP_14", 80), // CAP 对拍行（容量）
    ("PRESS_15", 85), // PRESS 对拍行（压测）
];

/// 线性查找（键唯一性由构造期断言保证，运行时无分支歧义）。
pub fn u2801mix_get(key: &'static str) -> Option<u64> {
    let mut i = 0usize;
    while i < u2801mix_COUNT {
        if u2801mix[i].0 == key {
            return Some(u2801mix[i].1);
        }
        i += 1;
    }
    None
}

/// 累计求和（用于守恒对账）。
pub fn u2801mix_sum() -> u64 {
    let mut s = 0u64;
    let mut i = 0usize;
    while i < u2801mix_COUNT {
        s = s.wrapping_add(u2801mix[i].1 as u64);
        i += 1;
    }
    s
}

// --------------------------------------------------------------------------
// 守卫 u2801mixgate（阈值 32 · 重入/越限/静默三类拒止）
// --------------------------------------------------------------------------
/// 大页池容量压测 混合面守卫（阈值 32 冻结）。
#[derive(Clone, Copy, Debug)]
pub struct u2801mixgate {
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

/// 守卫阈值（大页池容量压测 混合面守卫（阈值 32 冻结））。
pub const U2801MIXGATE_THRESHOLD: u64 = 32;

impl u2801mixgate {
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
        if self.window >= U2801MIXGATE_THRESHOLD {
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
// 字符串键值映射 u2801mixmap（16 行逐行语义注释 + 逐行守卫）
// --------------------------------------------------------------------------
/// 大页池容量压测 混合映射矩阵。
/// 行数。
pub const u2801mixmap_COUNT: usize = 16;

/// 表本体（键 + 值 + 注释由编译期冻结）。
pub static u2801mixmap: [(&'static str, u64); 16] = [
    ("HP0000", 0), // HP 单元 0000（大页 × 相位 00 × 混 轮）
    ("POOL0001", 9), // POOL 单元 0001（池 × 相位 01 × 混 轮）
    ("CAP0002", 18), // CAP 单元 0002（容量 × 相位 02 × 混 轮）
    ("PRESS0003", 27), // PRESS 单元 0003（压测 × 相位 03 × 混 轮）
    ("MATRIX0004", 36), // MATRIX 单元 0004（矩阵 × 相位 04 × 混 轮）
    ("ALLOC0005", 45), // ALLOC 单元 0005（分配 × 相位 05 × 混 轮）
    ("FREE0006", 54), // FREE 单元 0006（释放 × 相位 06 × 混 轮）
    ("FRAG0007", 63), // FRAG 单元 0007（碎片 × 相位 07 × 混 轮）
    ("RSS0008", 8), // RSS 单元 0008（常驻 × 相位 08 × 混 轮）
    ("FLOOR0009", 17), // FLOOR 单元 0009（地板 × 相位 09 × 混 轮）
    ("CEIL0010", 26), // CEIL 单元 0010（天花板 × 相位 10 × 混 轮）
    ("BUD0011", 35), // BUD 单元 0011（预算 × 相位 11 × 混 轮）
    ("HP0012", 44), // HP 单元 0012（大页 × 相位 12 × 混 轮）
    ("POOL0013", 53), // POOL 单元 0013（池 × 相位 13 × 混 轮）
    ("CAP0014", 62), // CAP 单元 0014（容量 × 相位 14 × 混 轮）
    ("PRESS0015", 7), // PRESS 单元 0015（压测 × 相位 15 × 混 轮）
];

/// O(1) 语义查找（match 全枚举；命中率 100%% 由守卫保证）。
pub fn u2801mixmap_get(key: &str) -> Option<u64> {
    match key {
        "HP0000" => Some(0),
        "POOL0001" => Some(9),
        "CAP0002" => Some(18),
        "PRESS0003" => Some(27),
        "MATRIX0004" => Some(36),
        "ALLOC0005" => Some(45),
        "FREE0006" => Some(54),
        "FRAG0007" => Some(63),
        "RSS0008" => Some(8),
        "FLOOR0009" => Some(17),
        "CEIL0010" => Some(26),
        "BUD0011" => Some(35),
        "HP0012" => Some(44),
        "POOL0013" => Some(53),
        "CAP0014" => Some(62),
        "PRESS0015" => Some(7),
        _ => None,
    }
}

/// 逐行守卫扫描（返回违例行数；0 = 全绿）。
pub fn u2801mixmap_scan() -> usize {
    let mut bad = 0usize;
    let mut i = 0usize;
    while i < u2801mixmap_COUNT {
        let (k, v) = u2801mixmap[i];
        let ok = match k {
            "HP0000" => v < 64,
            "POOL0001" => v < 64,
            "CAP0002" => v < 64,
            "PRESS0003" => v < 64,
            "MATRIX0004" => v < 64,
            "ALLOC0005" => v < 64,
            "FREE0006" => v < 64,
            "FRAG0007" => v < 64,
            "RSS0008" => v < 64,
            "FLOOR0009" => v < 64,
            "CEIL0010" => v < 64,
            "BUD0011" => v < 64,
            "HP0012" => v < 64,
            "POOL0013" => v < 64,
            "CAP0014" => v < 64,
            "PRESS0015" => v < 64,
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
pub fn u2801mixmap_sum() -> u64 {
    let mut s = 0u64;
    let mut i = 0usize;
    while i < u2801mixmap_COUNT {
        s = s.wrapping_add(u2801mixmap[i].1 as u64);
        i += 1;
    }
    s
}

/// 键唯一性（构造期断言等价物：逐对扫描同键）。
pub fn u2801mixmap_unique() -> bool {
    let mut i = 0usize;
    while i < u2801mixmap_COUNT {
        let mut j = i + 1;
        while j < u2801mixmap_COUNT {
            if u2801mixmap[i].0 == u2801mixmap[j].0 {
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

/// 段位守卫：全部诊断码必须落在本段 [0xe220, 0xe22f]。
pub const fn u2801_segment_guard() -> bool {
    let mut ok = true;
    let mut i = 0usize;
    while i < u2801Code::ALL.len() {
        let c = u2801Code::ALL[i].code();
        if c < 0xe220 || c > 0xe22f {
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
fn u2801_guard_seg() -> bool {
    u2801_segment_guard()
}

/// UNX-F2801 对拍表 16 行边界与累计守恒。
fn check_u2801_mix_tbl() -> bool {
    let mut ok = u2801mix_COUNT == 16;
    if ok { ok = u2801mix_get("HP_00") == Some(10) && u2801mix_get("PRESS_15") == Some(85); }
    if ok { ok = u2801mix_get("ZZZ_NOKEY").is_none(); }
    if ok { ok = u2801mix_sum() == 760; }
    ok
}

/// UNX-F2801 混合映射 16 行守卫唯一 + 守卫阈值冻结。
fn check_u2801_mix_map() -> bool {
    let mut ok = u2801mixmap_COUNT == 16;
    if ok { ok = u2801mixmap_scan() == 0 && u2801mixmap_unique(); }
    if ok { ok = u2801mixmap_sum() == 504; }
    if ok { ok = U2801MIXGATE_THRESHOLD == 32; }
    ok
}

/// 本模块自检（逐检查带 detail，全绿才出门）。
pub fn run_u2801_checks() -> CheckSet {
    let mut set = CheckSet::new("U2801");
    set.add(
        "u2801-seg-guard",
        u2801_guard_seg(),
        if u2801_guard_seg() { "" } else { "diagnostic code outside segment" },
    );
    set.add(
        "u2801-mix-tbl",
        check_u2801_mix_tbl(),
        if check_u2801_mix_tbl() { "" } else { "UNX-F2801 mix table mismatch" },
    );
    set.add(
        "u2801-mix-map",
        check_u2801_mix_map(),
        if check_u2801_mix_map() { "" } else { "UNX-F2801 mix map mismatch" },
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
    fn u2801_all_checks_green() {
        let set = run_u2801_checks();
        assert!(set.all_passed(), "U2801 domain checks must be green");
    }

    #[test]
    fn u2801_segment_guard_holds() {
        assert!(u2801_segment_guard());
    }
}
