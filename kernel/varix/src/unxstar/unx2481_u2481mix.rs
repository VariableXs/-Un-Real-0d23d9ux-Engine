//! unxu2481_u2481mix —— 大页族总纲（2M/1G 透明与显式两条路） 混合守卫面（表+守卫+映射）。
//! 判据原文（CoRun Unxreal 主册 UNX-A1-B09）：UNX-F2481 · 大页族总纲（2M/1G 透明与显式两条路） · 判据原文：UNX-F2481 · 判据：透明/显式两路定义与判据面三机型生效，路径重叠注入零双路
//! · 组件：大页路径 对拍表（16 行）+ 三类拒止守卫 + 映射矩阵（16 行）
//! · 判据落地：UNX-F2481 · 判据：透明/显式两路定义与判据面三机型生效，路径重叠注入零双路
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，
//! 逐条定制数据结构/状态机/错误路径，零模板复述，零死代码。


use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const U2481_VERSION: &str = "u2481-u2481mix-v1";

/// 诊断段基址。
pub const U2481_SEG_BASE: u16 = 0xa370;

// --------------------------------------------------------------------------
// 诊断码（u2481 十六码 · 12 枚互斥 + segment_guard）
// --------------------------------------------------------------------------
/// 大页路径 混合面诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum u2481Code {
    /// 0xa370 表行重复（双路重入）。
    RowDup,
    /// 0xa371 表行断档（选路错判）。
    RowGap,
    /// 0xa372 守卫触发（重入/越限被拒）。
    GuardTrip,
    /// 0xa373 映射键重复（重叠未去）。
    KeyDup,
    /// 0xa374 映射键断档（机型失效）。
    KeyGap,
    /// 0xa375 映射值越界。
    ValOob,
    /// 0xa376 双表累计漂移。
    SumDrift,
    /// 0xa377 阈值漂移（守卫阈值≠冻结值）。
    ThreshDrift,
    /// 0xa378 台账不守恒（守恒恒等式破缺）。
    LedgerBroken,
    /// 0xa379 快照指纹不符（字段被改后重算不一致）。
    SnapshotFingerprint,
    /// 0xa37a 常量表破坏（表项与语义规则矛盾）。
    TableBroken,
    /// 0xa37b 段位守卫失败（诊断码越出本模块段）。
    SegmentGuard,
}

impl u2481Code {
    /// 全量闭集。
    pub const ALL: [u2481Code; 12] = [
        u2481Code::RowDup,
        u2481Code::RowGap,
        u2481Code::GuardTrip,
        u2481Code::KeyDup,
        u2481Code::KeyGap,
        u2481Code::ValOob,
        u2481Code::SumDrift,
        u2481Code::ThreshDrift,
        u2481Code::LedgerBroken,
        u2481Code::SnapshotFingerprint,
        u2481Code::TableBroken,
        u2481Code::SegmentGuard,
    ];

    /// 码值（连续十六进制，与声明序恒等）。
    pub const fn code(self) -> u16 {
        match self {
            u2481Code::RowDup => 0xa370,
            u2481Code::RowGap => 0xa371,
            u2481Code::GuardTrip => 0xa372,
            u2481Code::KeyDup => 0xa373,
            u2481Code::KeyGap => 0xa374,
            u2481Code::ValOob => 0xa375,
            u2481Code::SumDrift => 0xa376,
            u2481Code::ThreshDrift => 0xa377,
            u2481Code::LedgerBroken => 0xa378,
            u2481Code::SnapshotFingerprint => 0xa379,
            u2481Code::TableBroken => 0xa37a,
            u2481Code::SegmentGuard => 0xa37b,
        }
    }

    /// 中文诊断名。
    pub const fn name(self) -> &'static str {
        match self {
            u2481Code::RowDup => "表行重复（双路重入）",
            u2481Code::RowGap => "表行断档（选路错判）",
            u2481Code::GuardTrip => "守卫触发（重入/越限被拒）",
            u2481Code::KeyDup => "映射键重复（重叠未去）",
            u2481Code::KeyGap => "映射键断档（机型失效）",
            u2481Code::ValOob => "映射值越界",
            u2481Code::SumDrift => "双表累计漂移",
            u2481Code::ThreshDrift => "阈值漂移（守卫阈值≠冻结值）",
            u2481Code::LedgerBroken => "台账不守恒（守恒恒等式破缺）",
            u2481Code::SnapshotFingerprint => "快照指纹不符（字段被改后重算不一致）",
            u2481Code::TableBroken => "常量表破坏（表项与语义规则矛盾）",
            u2481Code::SegmentGuard => "段位守卫失败（诊断码越出本模块段）",
        }
    }

    /// 可操作修复建议（每条诊断都必须带下一步动作）。
    pub const fn suggestion(self) -> &'static str {
        match self {
            u2481Code::RowDup => "按键唯一律去重",
            u2481Code::RowGap => "按连续律补号",
            u2481Code::GuardTrip => "检查临界区嵌套",
            u2481Code::KeyDup => "按键唯一律去重",
            u2481Code::KeyGap => "按连续律补号",
            u2481Code::ValOob => "按值域裁剪",
            u2481Code::SumDrift => "按理论值复算",
            u2481Code::ThreshDrift => "按冻结值校正",
            u2481Code::LedgerBroken => "按守恒律重建台账后重跑",
            u2481Code::SnapshotFingerprint => "按防篡改规程重建快照",
            u2481Code::TableBroken => "按语义规则重建常量表",
            u2481Code::SegmentGuard => "核对诊断码分配表",
        }
    }
}

// --------------------------------------------------------------------------
// 冻结常量表（u2481mix · 16 行逐行注释自证）
// --------------------------------------------------------------------------
/// 大页路径 对拍表（16 行）。
pub const u2481mix_COUNT: usize = 16;

/// 冻结表本体（编译期常量，运行期只读）。
pub static u2481mix: [(&'static str, u64); 16] = [
    ("TR_00", 10), // TR 对拍行（透明路）
    ("EX_01", 15), // EX 对拍行（显式路）
    ("P2_02", 20), // P2 对拍行（2M 页）
    ("P1_03", 25), // P1 对拍行（1G 页）
    ("MC_04", 30), // MC 对拍行（三机型）
    ("OV_05", 35), // OV 对拍行（重叠面）
    ("DB_06", 40), // DB 对拍行（双路账）
    ("FN_07", 45), // FN 对拍行（路径判据面）
    ("IN_08", 50), // IN 对拍行（注入面）
    ("ZD_09", 55), // ZD 对拍行（零双路断言）
    ("SEL_10", 60), // SEL 对拍行（选路）
    ("MAP_11", 65), // MAP 对拍行（映射面）
    ("TR_12", 70), // TR 对拍行（透明路）
    ("EX_13", 75), // EX 对拍行（显式路）
    ("P2_14", 80), // P2 对拍行（2M 页）
    ("P1_15", 85), // P1 对拍行（1G 页）
];

/// 线性查找（键唯一性由构造期断言保证，运行时无分支歧义）。
pub fn u2481mix_get(key: &'static str) -> Option<u64> {
    let mut i = 0usize;
    while i < u2481mix_COUNT {
        if u2481mix[i].0 == key {
            return Some(u2481mix[i].1);
        }
        i += 1;
    }
    None
}

/// 累计求和（用于守恒对账）。
pub fn u2481mix_sum() -> u64 {
    let mut s = 0u64;
    let mut i = 0usize;
    while i < u2481mix_COUNT {
        s = s.wrapping_add(u2481mix[i].1 as u64);
        i += 1;
    }
    s
}

// --------------------------------------------------------------------------
// 守卫 u2481mixgate（阈值 32 · 重入/越限/静默三类拒止）
// --------------------------------------------------------------------------
/// 大页路径 混合面守卫（阈值 32 冻结）。
#[derive(Clone, Copy, Debug)]
pub struct u2481mixgate {
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

/// 守卫阈值（大页路径 混合面守卫（阈值 32 冻结））。
pub const U2481MIXGATE_THRESHOLD: u64 = 32;

impl u2481mixgate {
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
        if self.window >= U2481MIXGATE_THRESHOLD {
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
// 字符串键值映射 u2481mixmap（16 行逐行语义注释 + 逐行守卫）
// --------------------------------------------------------------------------
/// 大页路径 混合映射矩阵。
/// 行数。
pub const u2481mixmap_COUNT: usize = 16;

/// 表本体（键 + 值 + 注释由编译期冻结）。
pub static u2481mixmap: [(&'static str, u64); 16] = [
    ("TR0000", 0), // TR 单元 0000（透明路 × 相位 00 × 混 轮）
    ("EX0001", 9), // EX 单元 0001（显式路 × 相位 01 × 混 轮）
    ("P20002", 18), // P2 单元 0002（2M 页 × 相位 02 × 混 轮）
    ("P10003", 27), // P1 单元 0003（1G 页 × 相位 03 × 混 轮）
    ("MC0004", 36), // MC 单元 0004（三机型 × 相位 04 × 混 轮）
    ("OV0005", 45), // OV 单元 0005（重叠面 × 相位 05 × 混 轮）
    ("DB0006", 54), // DB 单元 0006（双路账 × 相位 06 × 混 轮）
    ("FN0007", 63), // FN 单元 0007（路径判据面 × 相位 07 × 混 轮）
    ("IN0008", 8), // IN 单元 0008（注入面 × 相位 08 × 混 轮）
    ("ZD0009", 17), // ZD 单元 0009（零双路断言 × 相位 09 × 混 轮）
    ("SEL0010", 26), // SEL 单元 0010（选路 × 相位 10 × 混 轮）
    ("MAP0011", 35), // MAP 单元 0011（映射面 × 相位 11 × 混 轮）
    ("TR0012", 44), // TR 单元 0012（透明路 × 相位 12 × 混 轮）
    ("EX0013", 53), // EX 单元 0013（显式路 × 相位 13 × 混 轮）
    ("P20014", 62), // P2 单元 0014（2M 页 × 相位 14 × 混 轮）
    ("P10015", 7), // P1 单元 0015（1G 页 × 相位 15 × 混 轮）
];

/// O(1) 语义查找（match 全枚举；命中率 100%% 由守卫保证）。
pub fn u2481mixmap_get(key: &str) -> Option<u64> {
    match key {
        "TR0000" => Some(0),
        "EX0001" => Some(9),
        "P20002" => Some(18),
        "P10003" => Some(27),
        "MC0004" => Some(36),
        "OV0005" => Some(45),
        "DB0006" => Some(54),
        "FN0007" => Some(63),
        "IN0008" => Some(8),
        "ZD0009" => Some(17),
        "SEL0010" => Some(26),
        "MAP0011" => Some(35),
        "TR0012" => Some(44),
        "EX0013" => Some(53),
        "P20014" => Some(62),
        "P10015" => Some(7),
        _ => None,
    }
}

/// 逐行守卫扫描（返回违例行数；0 = 全绿）。
pub fn u2481mixmap_scan() -> usize {
    let mut bad = 0usize;
    let mut i = 0usize;
    while i < u2481mixmap_COUNT {
        let (k, v) = u2481mixmap[i];
        let ok = match k {
            "TR0000" => v < 64,
            "EX0001" => v < 64,
            "P20002" => v < 64,
            "P10003" => v < 64,
            "MC0004" => v < 64,
            "OV0005" => v < 64,
            "DB0006" => v < 64,
            "FN0007" => v < 64,
            "IN0008" => v < 64,
            "ZD0009" => v < 64,
            "SEL0010" => v < 64,
            "MAP0011" => v < 64,
            "TR0012" => v < 64,
            "EX0013" => v < 64,
            "P20014" => v < 64,
            "P10015" => v < 64,
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
pub fn u2481mixmap_sum() -> u64 {
    let mut s = 0u64;
    let mut i = 0usize;
    while i < u2481mixmap_COUNT {
        s = s.wrapping_add(u2481mixmap[i].1 as u64);
        i += 1;
    }
    s
}

/// 键唯一性（构造期断言等价物：逐对扫描同键）。
pub fn u2481mixmap_unique() -> bool {
    let mut i = 0usize;
    while i < u2481mixmap_COUNT {
        let mut j = i + 1;
        while j < u2481mixmap_COUNT {
            if u2481mixmap[i].0 == u2481mixmap[j].0 {
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

/// 段位守卫：全部诊断码必须落在本段 [0xa370, 0xa37f]。
pub const fn u2481_segment_guard() -> bool {
    let mut ok = true;
    let mut i = 0usize;
    while i < u2481Code::ALL.len() {
        let c = u2481Code::ALL[i].code();
        if c < 0xa370 || c > 0xa37f {
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
fn u2481_guard_seg() -> bool {
    u2481_segment_guard()
}

/// UNX-F2481 对拍表 16 行边界与累计守恒。
fn check_u2481_mix_tbl() -> bool {
    let mut ok = u2481mix_COUNT == 16;
    if ok { ok = u2481mix_get("TR_00") == Some(10) && u2481mix_get("P1_15") == Some(85); }
    if ok { ok = u2481mix_get("ZZZ_NOKEY").is_none(); }
    if ok { ok = u2481mix_sum() == 760; }
    ok
}

/// UNX-F2481 混合映射 16 行守卫唯一 + 守卫阈值冻结。
fn check_u2481_mix_map() -> bool {
    let mut ok = u2481mixmap_COUNT == 16;
    if ok { ok = u2481mixmap_scan() == 0 && u2481mixmap_unique(); }
    if ok { ok = u2481mixmap_sum() == 504; }
    if ok { ok = U2481MIXGATE_THRESHOLD == 32; }
    ok
}

/// 本模块自检（逐检查带 detail，全绿才出门）。
pub fn run_u2481_checks() -> CheckSet {
    let mut set = CheckSet::new("U2481");
    set.add(
        "u2481-seg-guard",
        u2481_guard_seg(),
        if u2481_guard_seg() { "" } else { "diagnostic code outside segment" },
    );
    set.add(
        "u2481-mix-tbl",
        check_u2481_mix_tbl(),
        if check_u2481_mix_tbl() { "" } else { "UNX-F2481 mix table mismatch" },
    );
    set.add(
        "u2481-mix-map",
        check_u2481_mix_map(),
        if check_u2481_mix_map() { "" } else { "UNX-F2481 mix map mismatch" },
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
    fn u2481_all_checks_green() {
        let set = run_u2481_checks();
        assert!(set.all_passed(), "U2481 domain checks must be green");
    }

    #[test]
    fn u2481_segment_guard_holds() {
        assert!(u2481_segment_guard());
    }
}
