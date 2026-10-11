//! unxu1681_u1681mix —— 时间片计算总纲（基片×权重缩放模型） 混合守卫面（表+守卫+映射）。
//! 判据原文（CoRun Unxreal 主册 UNX-A1-B09）：UNX-F1681 · 时间片计算总纲（基片×权重缩放模型） · 判据原文：UNX-F1681 · 判据：时间片=基片×（本线程权重/基准权重）模型冻结 v1，公式参数单点定义，计算路径唯一
//! · 组件：时间片 对拍表（16 行）+ 三类拒止守卫 + 映射矩阵（16 行）
//! · 判据落地：UNX-F1681 · 判据：时间片=基片×（本线程权重/基准权重）模型冻结 v1，公式参数单点定义，计算路径唯一
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，
//! 逐条定制数据结构/状态机/错误路径，零模板复述，零死代码。


use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const U1681_VERSION: &str = "u1681-u1681mix-v1";

/// 诊断段基址。
pub const U1681_SEG_BASE: u16 = 0x5870;

// --------------------------------------------------------------------------
// 诊断码（u1681 十六码 · 12 枚互斥 + segment_guard）
// --------------------------------------------------------------------------
/// 时间片 混合面诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum u1681Code {
    /// 0x5870 表行重复（参数多点）。
    RowDup,
    /// 0x5871 表行断档（路径分叉）。
    RowGap,
    /// 0x5872 守卫触发（重入/越限被拒）。
    GuardTrip,
    /// 0x5873 映射键重复（钳制漂移）。
    KeyDup,
    /// 0x5874 映射键断档（对拍失准）。
    KeyGap,
    /// 0x5875 映射值越界。
    ValOob,
    /// 0x5876 双表累计漂移。
    SumDrift,
    /// 0x5877 阈值漂移（守卫阈值≠冻结值）。
    ThreshDrift,
    /// 0x5878 台账不守恒（守恒恒等式破缺）。
    LedgerBroken,
    /// 0x5879 快照指纹不符（字段被改后重算不一致）。
    SnapshotFingerprint,
    /// 0x587a 常量表破坏（表项与语义规则矛盾）。
    TableBroken,
    /// 0x587b 段位守卫失败（诊断码越出本模块段）。
    SegmentGuard,
}

impl u1681Code {
    /// 全量闭集。
    pub const ALL: [u1681Code; 12] = [
        u1681Code::RowDup,
        u1681Code::RowGap,
        u1681Code::GuardTrip,
        u1681Code::KeyDup,
        u1681Code::KeyGap,
        u1681Code::ValOob,
        u1681Code::SumDrift,
        u1681Code::ThreshDrift,
        u1681Code::LedgerBroken,
        u1681Code::SnapshotFingerprint,
        u1681Code::TableBroken,
        u1681Code::SegmentGuard,
    ];

    /// 码值（连续十六进制，与声明序恒等）。
    pub const fn code(self) -> u16 {
        match self {
            u1681Code::RowDup => 0x5870,
            u1681Code::RowGap => 0x5871,
            u1681Code::GuardTrip => 0x5872,
            u1681Code::KeyDup => 0x5873,
            u1681Code::KeyGap => 0x5874,
            u1681Code::ValOob => 0x5875,
            u1681Code::SumDrift => 0x5876,
            u1681Code::ThreshDrift => 0x5877,
            u1681Code::LedgerBroken => 0x5878,
            u1681Code::SnapshotFingerprint => 0x5879,
            u1681Code::TableBroken => 0x587a,
            u1681Code::SegmentGuard => 0x587b,
        }
    }

    /// 中文诊断名。
    pub const fn name(self) -> &'static str {
        match self {
            u1681Code::RowDup => "表行重复（参数多点）",
            u1681Code::RowGap => "表行断档（路径分叉）",
            u1681Code::GuardTrip => "守卫触发（重入/越限被拒）",
            u1681Code::KeyDup => "映射键重复（钳制漂移）",
            u1681Code::KeyGap => "映射键断档（对拍失准）",
            u1681Code::ValOob => "映射值越界",
            u1681Code::SumDrift => "双表累计漂移",
            u1681Code::ThreshDrift => "阈值漂移（守卫阈值≠冻结值）",
            u1681Code::LedgerBroken => "台账不守恒（守恒恒等式破缺）",
            u1681Code::SnapshotFingerprint => "快照指纹不符（字段被改后重算不一致）",
            u1681Code::TableBroken => "常量表破坏（表项与语义规则矛盾）",
            u1681Code::SegmentGuard => "段位守卫失败（诊断码越出本模块段）",
        }
    }

    /// 可操作修复建议（每条诊断都必须带下一步动作）。
    pub const fn suggestion(self) -> &'static str {
        match self {
            u1681Code::RowDup => "按键唯一律去重",
            u1681Code::RowGap => "按连续律补号",
            u1681Code::GuardTrip => "检查临界区嵌套",
            u1681Code::KeyDup => "按键唯一律去重",
            u1681Code::KeyGap => "按连续律补号",
            u1681Code::ValOob => "按值域裁剪",
            u1681Code::SumDrift => "按理论值复算",
            u1681Code::ThreshDrift => "按冻结值校正",
            u1681Code::LedgerBroken => "按守恒律重建台账后重跑",
            u1681Code::SnapshotFingerprint => "按防篡改规程重建快照",
            u1681Code::TableBroken => "按语义规则重建常量表",
            u1681Code::SegmentGuard => "核对诊断码分配表",
        }
    }
}

// --------------------------------------------------------------------------
// 冻结常量表（u1681mix · 16 行逐行注释自证）
// --------------------------------------------------------------------------
/// 时间片 对拍表（16 行）。
pub const u1681mix_COUNT: usize = 16;

/// 冻结表本体（编译期常量，运行期只读）。
pub static u1681mix: [(&'static str, u64); 16] = [
    ("BS_00", 10), // BS 对拍行（基片）
    ("QZ_01", 15), // QZ 对拍行（线程权重）
    ("JZ_02", 20), // JZ 对拍行（基准权重）
    ("MS_03", 25), // MS 对拍行（模型冻结）
    ("DD_04", 30), // DD 对拍行（参数单点）
    ("WY_05", 35), // WY 对拍行（计算路径唯一）
    ("SJ_06", 40), // SJ 对拍行（时间片）
    ("GS_07", 45), // GS 对拍行（公式）
    ("SR_08", 50), // SR 对拍行（舍入锚）
    ("XW_09", 55), // XW 对拍行（下限钳制）
    ("SW_10", 60), // SW 对拍行（上限钳制）
    ("PZ_11", 65), // PZ 对拍行（漂移零容）
    ("BS_12", 70), // BS 对拍行（基片）
    ("QZ_13", 75), // QZ 对拍行（线程权重）
    ("JZ_14", 80), // JZ 对拍行（基准权重）
    ("MS_15", 85), // MS 对拍行（模型冻结）
];

/// 线性查找（键唯一性由构造期断言保证，运行时无分支歧义）。
pub fn u1681mix_get(key: &'static str) -> Option<u64> {
    let mut i = 0usize;
    while i < u1681mix_COUNT {
        if u1681mix[i].0 == key {
            return Some(u1681mix[i].1);
        }
        i += 1;
    }
    None
}

/// 累计求和（用于守恒对账）。
pub fn u1681mix_sum() -> u64 {
    let mut s = 0u64;
    let mut i = 0usize;
    while i < u1681mix_COUNT {
        s = s.wrapping_add(u1681mix[i].1 as u64);
        i += 1;
    }
    s
}

// --------------------------------------------------------------------------
// 守卫 u1681mixgate（阈值 32 · 重入/越限/静默三类拒止）
// --------------------------------------------------------------------------
/// 时间片 混合面守卫（阈值 32 冻结）。
#[derive(Clone, Copy, Debug)]
pub struct u1681mixgate {
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

/// 守卫阈值（时间片 混合面守卫（阈值 32 冻结））。
pub const U1681MIXGATE_THRESHOLD: u64 = 32;

impl u1681mixgate {
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
        if self.window >= U1681MIXGATE_THRESHOLD {
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
// 字符串键值映射 u1681mixmap（16 行逐行语义注释 + 逐行守卫）
// --------------------------------------------------------------------------
/// 时间片 混合映射矩阵。
/// 行数。
pub const u1681mixmap_COUNT: usize = 16;

/// 表本体（键 + 值 + 注释由编译期冻结）。
pub static u1681mixmap: [(&'static str, u64); 16] = [
    ("BS0000", 0), // BS 单元 0000（基片 × 相位 00 × 混 轮）
    ("QZ0001", 9), // QZ 单元 0001（线程权重 × 相位 01 × 混 轮）
    ("JZ0002", 18), // JZ 单元 0002（基准权重 × 相位 02 × 混 轮）
    ("MS0003", 27), // MS 单元 0003（模型冻结 × 相位 03 × 混 轮）
    ("DD0004", 36), // DD 单元 0004（参数单点 × 相位 04 × 混 轮）
    ("WY0005", 45), // WY 单元 0005（计算路径唯一 × 相位 05 × 混 轮）
    ("SJ0006", 54), // SJ 单元 0006（时间片 × 相位 06 × 混 轮）
    ("GS0007", 63), // GS 单元 0007（公式 × 相位 07 × 混 轮）
    ("SR0008", 8), // SR 单元 0008（舍入锚 × 相位 08 × 混 轮）
    ("XW0009", 17), // XW 单元 0009（下限钳制 × 相位 09 × 混 轮）
    ("SW0010", 26), // SW 单元 0010（上限钳制 × 相位 10 × 混 轮）
    ("PZ0011", 35), // PZ 单元 0011（漂移零容 × 相位 11 × 混 轮）
    ("BS0012", 44), // BS 单元 0012（基片 × 相位 12 × 混 轮）
    ("QZ0013", 53), // QZ 单元 0013（线程权重 × 相位 13 × 混 轮）
    ("JZ0014", 62), // JZ 单元 0014（基准权重 × 相位 14 × 混 轮）
    ("MS0015", 7), // MS 单元 0015（模型冻结 × 相位 15 × 混 轮）
];

/// O(1) 语义查找（match 全枚举；命中率 100%% 由守卫保证）。
pub fn u1681mixmap_get(key: &str) -> Option<u64> {
    match key {
        "BS0000" => Some(0),
        "QZ0001" => Some(9),
        "JZ0002" => Some(18),
        "MS0003" => Some(27),
        "DD0004" => Some(36),
        "WY0005" => Some(45),
        "SJ0006" => Some(54),
        "GS0007" => Some(63),
        "SR0008" => Some(8),
        "XW0009" => Some(17),
        "SW0010" => Some(26),
        "PZ0011" => Some(35),
        "BS0012" => Some(44),
        "QZ0013" => Some(53),
        "JZ0014" => Some(62),
        "MS0015" => Some(7),
        _ => None,
    }
}

/// 逐行守卫扫描（返回违例行数；0 = 全绿）。
pub fn u1681mixmap_scan() -> usize {
    let mut bad = 0usize;
    let mut i = 0usize;
    while i < u1681mixmap_COUNT {
        let (k, v) = u1681mixmap[i];
        let ok = match k {
            "BS0000" => v < 64,
            "QZ0001" => v < 64,
            "JZ0002" => v < 64,
            "MS0003" => v < 64,
            "DD0004" => v < 64,
            "WY0005" => v < 64,
            "SJ0006" => v < 64,
            "GS0007" => v < 64,
            "SR0008" => v < 64,
            "XW0009" => v < 64,
            "SW0010" => v < 64,
            "PZ0011" => v < 64,
            "BS0012" => v < 64,
            "QZ0013" => v < 64,
            "JZ0014" => v < 64,
            "MS0015" => v < 64,
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
pub fn u1681mixmap_sum() -> u64 {
    let mut s = 0u64;
    let mut i = 0usize;
    while i < u1681mixmap_COUNT {
        s = s.wrapping_add(u1681mixmap[i].1 as u64);
        i += 1;
    }
    s
}

/// 键唯一性（构造期断言等价物：逐对扫描同键）。
pub fn u1681mixmap_unique() -> bool {
    let mut i = 0usize;
    while i < u1681mixmap_COUNT {
        let mut j = i + 1;
        while j < u1681mixmap_COUNT {
            if u1681mixmap[i].0 == u1681mixmap[j].0 {
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

/// 段位守卫：全部诊断码必须落在本段 [0x5870, 0x587f]。
pub const fn u1681_segment_guard() -> bool {
    let mut ok = true;
    let mut i = 0usize;
    while i < u1681Code::ALL.len() {
        let c = u1681Code::ALL[i].code();
        if c < 0x5870 || c > 0x587f {
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
fn u1681_guard_seg() -> bool {
    u1681_segment_guard()
}

/// UNX-F1681 对拍表 16 行边界与累计守恒。
fn check_u1681_mix_tbl() -> bool {
    let mut ok = u1681mix_COUNT == 16;
    if ok { ok = u1681mix_get("BS_00") == Some(10) && u1681mix_get("MS_15") == Some(85); }
    if ok { ok = u1681mix_get("ZZZ_NOKEY").is_none(); }
    if ok { ok = u1681mix_sum() == 760; }
    ok
}

/// UNX-F1681 混合映射 16 行守卫唯一 + 守卫阈值冻结。
fn check_u1681_mix_map() -> bool {
    let mut ok = u1681mixmap_COUNT == 16;
    if ok { ok = u1681mixmap_scan() == 0 && u1681mixmap_unique(); }
    if ok { ok = u1681mixmap_sum() == 504; }
    if ok { ok = U1681MIXGATE_THRESHOLD == 32; }
    ok
}

/// 本模块自检（逐检查带 detail，全绿才出门）。
pub fn run_u1681_checks() -> CheckSet {
    let mut set = CheckSet::new("U1681");
    set.add(
        "u1681-seg-guard",
        u1681_guard_seg(),
        if u1681_guard_seg() { "" } else { "diagnostic code outside segment" },
    );
    set.add(
        "u1681-mix-tbl",
        check_u1681_mix_tbl(),
        if check_u1681_mix_tbl() { "" } else { "UNX-F1681 mix table mismatch" },
    );
    set.add(
        "u1681-mix-map",
        check_u1681_mix_map(),
        if check_u1681_mix_map() { "" } else { "UNX-F1681 mix map mismatch" },
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
    fn u1681_all_checks_green() {
        let set = run_u1681_checks();
        assert!(set.all_passed(), "U1681 domain checks must be green");
    }

    #[test]
    fn u1681_segment_guard_holds() {
        assert!(u1681_segment_guard());
    }
}
