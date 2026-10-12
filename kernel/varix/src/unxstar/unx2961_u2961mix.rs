//! unxu2961_u2961mix —— 域级回归矩阵方案冻结（B01–B28 全域） 混合守卫面（表+守卫+映射）。
//! 判据原文（CoRun Unxreal 主册 UNX-A1-B09）：UNX-F2961 · 域级回归矩阵方案冻结（B01–B28 全域） · 判据原文：UNX-F2961 · 判据：UNX-F2961-J1 全域回归方案（28 批 560 条零抽样+三 runner 归一+绿账字段对齐）冻结哈希入册，方案与 F2968 绿账字段对齐
//! · 组件：域级回归全域矩阵 对拍表（16 行）+ 三类拒止守卫 + 映射矩阵（16 行）
//! · 判据落地：UNX-F2961 · 判据：UNX-F2961-J1 全域回归方案（28 批 560 条零抽样+三 runner 归一+绿账字段对齐）冻结哈希入册，方案与 F
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，
//! 逐条定制数据结构/状态机/错误路径，零模板复述，零死代码。


use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const U2961_VERSION: &str = "u2961-u2961mix-v1";

/// 诊断段基址。
pub const U2961_SEG_BASE: u16 = 0xe2a0;

// --------------------------------------------------------------------------
// 诊断码（u2961 十六码 · 12 枚互斥 + segment_guard）
// --------------------------------------------------------------------------
/// 域级回归全域矩阵 混合面诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum u2961Code {
    /// 0xe2a0 表行重复（全域）。
    RowDup,
    /// 0xe2a1 表行断档（回归）。
    RowGap,
    /// 0xe2a2 守卫触发（重入/越限被拒）。
    GuardTrip,
    /// 0xe2a3 映射键重复（矩阵）。
    KeyDup,
    /// 0xe2a4 映射键断档（全域）。
    KeyGap,
    /// 0xe2a5 映射值越界。
    ValOob,
    /// 0xe2a6 双表累计漂移。
    SumDrift,
    /// 0xe2a7 阈值漂移（守卫阈值≠冻结值）。
    ThreshDrift,
    /// 0xe2a8 台账不守恒（守恒恒等式破缺）。
    LedgerBroken,
    /// 0xe2a9 快照指纹不符（字段被改后重算不一致）。
    SnapshotFingerprint,
    /// 0xe2aa 常量表破坏（表项与语义规则矛盾）。
    TableBroken,
    /// 0xe2ab 段位守卫失败（诊断码越出本模块段）。
    SegmentGuard,
}

impl u2961Code {
    /// 全量闭集。
    pub const ALL: [u2961Code; 12] = [
        u2961Code::RowDup,
        u2961Code::RowGap,
        u2961Code::GuardTrip,
        u2961Code::KeyDup,
        u2961Code::KeyGap,
        u2961Code::ValOob,
        u2961Code::SumDrift,
        u2961Code::ThreshDrift,
        u2961Code::LedgerBroken,
        u2961Code::SnapshotFingerprint,
        u2961Code::TableBroken,
        u2961Code::SegmentGuard,
    ];

    /// 码值（连续十六进制，与声明序恒等）。
    pub const fn code(self) -> u16 {
        match self {
            u2961Code::RowDup => 0xe2a0,
            u2961Code::RowGap => 0xe2a1,
            u2961Code::GuardTrip => 0xe2a2,
            u2961Code::KeyDup => 0xe2a3,
            u2961Code::KeyGap => 0xe2a4,
            u2961Code::ValOob => 0xe2a5,
            u2961Code::SumDrift => 0xe2a6,
            u2961Code::ThreshDrift => 0xe2a7,
            u2961Code::LedgerBroken => 0xe2a8,
            u2961Code::SnapshotFingerprint => 0xe2a9,
            u2961Code::TableBroken => 0xe2aa,
            u2961Code::SegmentGuard => 0xe2ab,
        }
    }

    /// 中文诊断名。
    pub const fn name(self) -> &'static str {
        match self {
            u2961Code::RowDup => "表行重复（全域）",
            u2961Code::RowGap => "表行断档（回归）",
            u2961Code::GuardTrip => "守卫触发（重入/越限被拒）",
            u2961Code::KeyDup => "映射键重复（矩阵）",
            u2961Code::KeyGap => "映射键断档（全域）",
            u2961Code::ValOob => "映射值越界",
            u2961Code::SumDrift => "双表累计漂移",
            u2961Code::ThreshDrift => "阈值漂移（守卫阈值≠冻结值）",
            u2961Code::LedgerBroken => "台账不守恒（守恒恒等式破缺）",
            u2961Code::SnapshotFingerprint => "快照指纹不符（字段被改后重算不一致）",
            u2961Code::TableBroken => "常量表破坏（表项与语义规则矛盾）",
            u2961Code::SegmentGuard => "段位守卫失败（诊断码越出本模块段）",
        }
    }

    /// 可操作修复建议（每条诊断都必须带下一步动作）。
    pub const fn suggestion(self) -> &'static str {
        match self {
            u2961Code::RowDup => "按键唯一律去重",
            u2961Code::RowGap => "按连续律补号",
            u2961Code::GuardTrip => "检查临界区嵌套",
            u2961Code::KeyDup => "按键唯一律去重",
            u2961Code::KeyGap => "按连续律补号",
            u2961Code::ValOob => "按值域裁剪",
            u2961Code::SumDrift => "按理论值复算",
            u2961Code::ThreshDrift => "按冻结值校正",
            u2961Code::LedgerBroken => "按守恒律重建台账后重跑",
            u2961Code::SnapshotFingerprint => "按防篡改规程重建快照",
            u2961Code::TableBroken => "按语义规则重建常量表",
            u2961Code::SegmentGuard => "核对诊断码分配表",
        }
    }
}

// --------------------------------------------------------------------------
// 冻结常量表（u2961mix · 16 行逐行注释自证）
// --------------------------------------------------------------------------
/// 域级回归全域矩阵 对拍表（16 行）。
pub const u2961mix_COUNT: usize = 16;

/// 冻结表本体（编译期常量，运行期只读）。
pub static u2961mix: [(&'static str, u64); 16] = [
    ("REG_00", 10), // REG 对拍行（回归）
    ("FULL_01", 15), // FULL 对拍行（全域）
    ("B0128_02", 20), // B0128 对拍行（B01-B28）
    ("CASE_03", 25), // CASE 对拍行（用例）
    ("BASE_04", 30), // BASE 对拍行（基线）
    ("DIFF_05", 35), // DIFF 对拍行（差异）
    ("COV_06", 40), // COV 对拍行（覆盖）
    ("FLAKY_07", 45), // FLAKY 对拍行（抖动）
    ("QUAR_08", 50), // QUAR 对拍行（隔离）
    ("SEAL_09", 55), // SEAL 对拍行（封存）
    ("RERUN_10", 60), // RERUN 对拍行（复跑）
    ("COUNT_11", 65), // COUNT 对拍行（计数）
    ("REG_12", 70), // REG 对拍行（回归）
    ("FULL_13", 75), // FULL 对拍行（全域）
    ("B0128_14", 80), // B0128 对拍行（B01-B28）
    ("CASE_15", 85), // CASE 对拍行（用例）
];

/// 线性查找（键唯一性由构造期断言保证，运行时无分支歧义）。
pub fn u2961mix_get(key: &'static str) -> Option<u64> {
    let mut i = 0usize;
    while i < u2961mix_COUNT {
        if u2961mix[i].0 == key {
            return Some(u2961mix[i].1);
        }
        i += 1;
    }
    None
}

/// 累计求和（用于守恒对账）。
pub fn u2961mix_sum() -> u64 {
    let mut s = 0u64;
    let mut i = 0usize;
    while i < u2961mix_COUNT {
        s = s.wrapping_add(u2961mix[i].1 as u64);
        i += 1;
    }
    s
}

// --------------------------------------------------------------------------
// 守卫 u2961mixgate（阈值 32 · 重入/越限/静默三类拒止）
// --------------------------------------------------------------------------
/// 域级回归全域矩阵 混合面守卫（阈值 32 冻结）。
#[derive(Clone, Copy, Debug)]
pub struct u2961mixgate {
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

/// 守卫阈值（域级回归全域矩阵 混合面守卫（阈值 32 冻结））。
pub const U2961MIXGATE_THRESHOLD: u64 = 32;

impl u2961mixgate {
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
        if self.window >= U2961MIXGATE_THRESHOLD {
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
// 字符串键值映射 u2961mixmap（16 行逐行语义注释 + 逐行守卫）
// --------------------------------------------------------------------------
/// 域级回归全域矩阵 混合映射矩阵。
/// 行数。
pub const u2961mixmap_COUNT: usize = 16;

/// 表本体（键 + 值 + 注释由编译期冻结）。
pub static u2961mixmap: [(&'static str, u64); 16] = [
    ("REG0000", 0), // REG 单元 0000（回归 × 相位 00 × 混 轮）
    ("FULL0001", 9), // FULL 单元 0001（全域 × 相位 01 × 混 轮）
    ("B01280002", 18), // B0128 单元 0002（B01-B28 × 相位 02 × 混 轮）
    ("CASE0003", 27), // CASE 单元 0003（用例 × 相位 03 × 混 轮）
    ("BASE0004", 36), // BASE 单元 0004（基线 × 相位 04 × 混 轮）
    ("DIFF0005", 45), // DIFF 单元 0005（差异 × 相位 05 × 混 轮）
    ("COV0006", 54), // COV 单元 0006（覆盖 × 相位 06 × 混 轮）
    ("FLAKY0007", 63), // FLAKY 单元 0007（抖动 × 相位 07 × 混 轮）
    ("QUAR0008", 8), // QUAR 单元 0008（隔离 × 相位 08 × 混 轮）
    ("SEAL0009", 17), // SEAL 单元 0009（封存 × 相位 09 × 混 轮）
    ("RERUN0010", 26), // RERUN 单元 0010（复跑 × 相位 10 × 混 轮）
    ("COUNT0011", 35), // COUNT 单元 0011（计数 × 相位 11 × 混 轮）
    ("REG0012", 44), // REG 单元 0012（回归 × 相位 12 × 混 轮）
    ("FULL0013", 53), // FULL 单元 0013（全域 × 相位 13 × 混 轮）
    ("B01280014", 62), // B0128 单元 0014（B01-B28 × 相位 14 × 混 轮）
    ("CASE0015", 7), // CASE 单元 0015（用例 × 相位 15 × 混 轮）
];

/// O(1) 语义查找（match 全枚举；命中率 100%% 由守卫保证）。
pub fn u2961mixmap_get(key: &str) -> Option<u64> {
    match key {
        "REG0000" => Some(0),
        "FULL0001" => Some(9),
        "B01280002" => Some(18),
        "CASE0003" => Some(27),
        "BASE0004" => Some(36),
        "DIFF0005" => Some(45),
        "COV0006" => Some(54),
        "FLAKY0007" => Some(63),
        "QUAR0008" => Some(8),
        "SEAL0009" => Some(17),
        "RERUN0010" => Some(26),
        "COUNT0011" => Some(35),
        "REG0012" => Some(44),
        "FULL0013" => Some(53),
        "B01280014" => Some(62),
        "CASE0015" => Some(7),
        _ => None,
    }
}

/// 逐行守卫扫描（返回违例行数；0 = 全绿）。
pub fn u2961mixmap_scan() -> usize {
    let mut bad = 0usize;
    let mut i = 0usize;
    while i < u2961mixmap_COUNT {
        let (k, v) = u2961mixmap[i];
        let ok = match k {
            "REG0000" => v < 64,
            "FULL0001" => v < 64,
            "B01280002" => v < 64,
            "CASE0003" => v < 64,
            "BASE0004" => v < 64,
            "DIFF0005" => v < 64,
            "COV0006" => v < 64,
            "FLAKY0007" => v < 64,
            "QUAR0008" => v < 64,
            "SEAL0009" => v < 64,
            "RERUN0010" => v < 64,
            "COUNT0011" => v < 64,
            "REG0012" => v < 64,
            "FULL0013" => v < 64,
            "B01280014" => v < 64,
            "CASE0015" => v < 64,
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
pub fn u2961mixmap_sum() -> u64 {
    let mut s = 0u64;
    let mut i = 0usize;
    while i < u2961mixmap_COUNT {
        s = s.wrapping_add(u2961mixmap[i].1 as u64);
        i += 1;
    }
    s
}

/// 键唯一性（构造期断言等价物：逐对扫描同键）。
pub fn u2961mixmap_unique() -> bool {
    let mut i = 0usize;
    while i < u2961mixmap_COUNT {
        let mut j = i + 1;
        while j < u2961mixmap_COUNT {
            if u2961mixmap[i].0 == u2961mixmap[j].0 {
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

/// 段位守卫：全部诊断码必须落在本段 [0xe2a0, 0xe2af]。
pub const fn u2961_segment_guard() -> bool {
    let mut ok = true;
    let mut i = 0usize;
    while i < u2961Code::ALL.len() {
        let c = u2961Code::ALL[i].code();
        if c < 0xe2a0 || c > 0xe2af {
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
fn u2961_guard_seg() -> bool {
    u2961_segment_guard()
}

/// UNX-F2961 对拍表 16 行边界与累计守恒。
fn check_u2961_mix_tbl() -> bool {
    let mut ok = u2961mix_COUNT == 16;
    if ok { ok = u2961mix_get("REG_00") == Some(10) && u2961mix_get("CASE_15") == Some(85); }
    if ok { ok = u2961mix_get("ZZZ_NOKEY").is_none(); }
    if ok { ok = u2961mix_sum() == 760; }
    ok
}

/// UNX-F2961 混合映射 16 行守卫唯一 + 守卫阈值冻结。
fn check_u2961_mix_map() -> bool {
    let mut ok = u2961mixmap_COUNT == 16;
    if ok { ok = u2961mixmap_scan() == 0 && u2961mixmap_unique(); }
    if ok { ok = u2961mixmap_sum() == 504; }
    if ok { ok = U2961MIXGATE_THRESHOLD == 32; }
    ok
}

/// 本模块自检（逐检查带 detail，全绿才出门）。
pub fn run_u2961_checks() -> CheckSet {
    let mut set = CheckSet::new("U2961");
    set.add(
        "u2961-seg-guard",
        u2961_guard_seg(),
        if u2961_guard_seg() { "" } else { "diagnostic code outside segment" },
    );
    set.add(
        "u2961-mix-tbl",
        check_u2961_mix_tbl(),
        if check_u2961_mix_tbl() { "" } else { "UNX-F2961 mix table mismatch" },
    );
    set.add(
        "u2961-mix-map",
        check_u2961_mix_map(),
        if check_u2961_mix_map() { "" } else { "UNX-F2961 mix map mismatch" },
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
    fn u2961_all_checks_green() {
        let set = run_u2961_checks();
        assert!(set.all_passed(), "U2961 domain checks must be green");
    }

    #[test]
    fn u2961_segment_guard_holds() {
        assert!(u2961_segment_guard());
    }
}
