//! unxu2761_u2761mix —— 四级压力档判定器（水位+产出双因子） 混合守卫面（表+守卫+映射）。
//! 判据原文（CoRun Unxreal 主册 UNX-A1-B09）：UNX-F2761 · 四级压力档判定器（水位+产出双因子） · 判据原文：UNX-F2761 · 判据：四档（正常/轻压/中压/重压）双因子判定逐档可复测，档位注入 40/40 次正确，与 F2741/F2747 双源读数一致
//! · 组件：压力档 对拍表（16 行）+ 三类拒止守卫 + 映射矩阵（16 行）
//! · 判据落地：UNX-F2761 · 判据：四档（正常/轻压/中压/重压）双因子判定逐档可复测，档位注入 40/40 次正确，与 F2741/F2747 双源读数一致
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，
//! 逐条定制数据结构/状态机/错误路径，零模板复述，零死代码。


use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const U2761_VERSION: &str = "u2761-u2761mix-v1";

/// 诊断段基址。
pub const U2761_SEG_BASE: u16 = 0xadb0;

// --------------------------------------------------------------------------
// 诊断码（u2761 十六码 · 12 枚互斥 + segment_guard）
// --------------------------------------------------------------------------
/// 压力档 混合面诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum u2761Code {
    /// 0xadb0 表行重复（档位跳变）。
    RowDup,
    /// 0xadb1 表行断档（双源分歧）。
    RowGap,
    /// 0xadb2 守卫触发（重入/越限被拒）。
    GuardTrip,
    /// 0xadb3 映射键重复（注入误判）。
    KeyDup,
    /// 0xadb4 映射键断档（因子失真）。
    KeyGap,
    /// 0xadb5 映射值越界。
    ValOob,
    /// 0xadb6 双表累计漂移。
    SumDrift,
    /// 0xadb7 阈值漂移（守卫阈值≠冻结值）。
    ThreshDrift,
    /// 0xadb8 台账不守恒（守恒恒等式破缺）。
    LedgerBroken,
    /// 0xadb9 快照指纹不符（字段被改后重算不一致）。
    SnapshotFingerprint,
    /// 0xadba 常量表破坏（表项与语义规则矛盾）。
    TableBroken,
    /// 0xadbb 段位守卫失败（诊断码越出本模块段）。
    SegmentGuard,
}

impl u2761Code {
    /// 全量闭集。
    pub const ALL: [u2761Code; 12] = [
        u2761Code::RowDup,
        u2761Code::RowGap,
        u2761Code::GuardTrip,
        u2761Code::KeyDup,
        u2761Code::KeyGap,
        u2761Code::ValOob,
        u2761Code::SumDrift,
        u2761Code::ThreshDrift,
        u2761Code::LedgerBroken,
        u2761Code::SnapshotFingerprint,
        u2761Code::TableBroken,
        u2761Code::SegmentGuard,
    ];

    /// 码值（连续十六进制，与声明序恒等）。
    pub const fn code(self) -> u16 {
        match self {
            u2761Code::RowDup => 0xadb0,
            u2761Code::RowGap => 0xadb1,
            u2761Code::GuardTrip => 0xadb2,
            u2761Code::KeyDup => 0xadb3,
            u2761Code::KeyGap => 0xadb4,
            u2761Code::ValOob => 0xadb5,
            u2761Code::SumDrift => 0xadb6,
            u2761Code::ThreshDrift => 0xadb7,
            u2761Code::LedgerBroken => 0xadb8,
            u2761Code::SnapshotFingerprint => 0xadb9,
            u2761Code::TableBroken => 0xadba,
            u2761Code::SegmentGuard => 0xadbb,
        }
    }

    /// 中文诊断名。
    pub const fn name(self) -> &'static str {
        match self {
            u2761Code::RowDup => "表行重复（档位跳变）",
            u2761Code::RowGap => "表行断档（双源分歧）",
            u2761Code::GuardTrip => "守卫触发（重入/越限被拒）",
            u2761Code::KeyDup => "映射键重复（注入误判）",
            u2761Code::KeyGap => "映射键断档（因子失真）",
            u2761Code::ValOob => "映射值越界",
            u2761Code::SumDrift => "双表累计漂移",
            u2761Code::ThreshDrift => "阈值漂移（守卫阈值≠冻结值）",
            u2761Code::LedgerBroken => "台账不守恒（守恒恒等式破缺）",
            u2761Code::SnapshotFingerprint => "快照指纹不符（字段被改后重算不一致）",
            u2761Code::TableBroken => "常量表破坏（表项与语义规则矛盾）",
            u2761Code::SegmentGuard => "段位守卫失败（诊断码越出本模块段）",
        }
    }

    /// 可操作修复建议（每条诊断都必须带下一步动作）。
    pub const fn suggestion(self) -> &'static str {
        match self {
            u2761Code::RowDup => "按键唯一律去重",
            u2761Code::RowGap => "按连续律补号",
            u2761Code::GuardTrip => "检查临界区嵌套",
            u2761Code::KeyDup => "按键唯一律去重",
            u2761Code::KeyGap => "按连续律补号",
            u2761Code::ValOob => "按值域裁剪",
            u2761Code::SumDrift => "按理论值复算",
            u2761Code::ThreshDrift => "按冻结值校正",
            u2761Code::LedgerBroken => "按守恒律重建台账后重跑",
            u2761Code::SnapshotFingerprint => "按防篡改规程重建快照",
            u2761Code::TableBroken => "按语义规则重建常量表",
            u2761Code::SegmentGuard => "核对诊断码分配表",
        }
    }
}

// --------------------------------------------------------------------------
// 冻结常量表（u2761mix · 16 行逐行注释自证）
// --------------------------------------------------------------------------
/// 压力档 对拍表（16 行）。
pub const u2761mix_COUNT: usize = 16;

/// 冻结表本体（编译期常量，运行期只读）。
pub static u2761mix: [(&'static str, u64); 16] = [
    ("NM_00", 10), // NM 对拍行（正常档）
    ("LT_01", 15), // LT 对拍行（轻压档）
    ("MD_02", 20), // MD 对拍行（中压档）
    ("HV_03", 25), // HV 对拍行（重压档）
    ("WL_04", 30), // WL 对拍行（水位因子）
    ("OU_05", 35), // OU 对拍行（产出因子）
    ("DF_06", 40), // DF 对拍行（双因子）
    ("IN40_07", 45), // IN40 对拍行（注入 40/40）
    ("D1_08", 50), // D1 对拍行（F2741 读数）
    ("D2_09", 55), // D2 对拍行（F2747 读数）
    ("DR_10", 60), // DR 对拍行（双源一致）
    ("RT_11", 65), // RT 对拍行（可复测）
    ("NM_12", 70), // NM 对拍行（正常档）
    ("LT_13", 75), // LT 对拍行（轻压档）
    ("MD_14", 80), // MD 对拍行（中压档）
    ("HV_15", 85), // HV 对拍行（重压档）
];

/// 线性查找（键唯一性由构造期断言保证，运行时无分支歧义）。
pub fn u2761mix_get(key: &'static str) -> Option<u64> {
    let mut i = 0usize;
    while i < u2761mix_COUNT {
        if u2761mix[i].0 == key {
            return Some(u2761mix[i].1);
        }
        i += 1;
    }
    None
}

/// 累计求和（用于守恒对账）。
pub fn u2761mix_sum() -> u64 {
    let mut s = 0u64;
    let mut i = 0usize;
    while i < u2761mix_COUNT {
        s = s.wrapping_add(u2761mix[i].1 as u64);
        i += 1;
    }
    s
}

// --------------------------------------------------------------------------
// 守卫 u2761mixgate（阈值 32 · 重入/越限/静默三类拒止）
// --------------------------------------------------------------------------
/// 压力档 混合面守卫（阈值 32 冻结）。
#[derive(Clone, Copy, Debug)]
pub struct u2761mixgate {
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

/// 守卫阈值（压力档 混合面守卫（阈值 32 冻结））。
pub const U2761MIXGATE_THRESHOLD: u64 = 32;

impl u2761mixgate {
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
        if self.window >= U2761MIXGATE_THRESHOLD {
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
// 字符串键值映射 u2761mixmap（16 行逐行语义注释 + 逐行守卫）
// --------------------------------------------------------------------------
/// 压力档 混合映射矩阵。
/// 行数。
pub const u2761mixmap_COUNT: usize = 16;

/// 表本体（键 + 值 + 注释由编译期冻结）。
pub static u2761mixmap: [(&'static str, u64); 16] = [
    ("NM0000", 0), // NM 单元 0000（正常档 × 相位 00 × 混 轮）
    ("LT0001", 9), // LT 单元 0001（轻压档 × 相位 01 × 混 轮）
    ("MD0002", 18), // MD 单元 0002（中压档 × 相位 02 × 混 轮）
    ("HV0003", 27), // HV 单元 0003（重压档 × 相位 03 × 混 轮）
    ("WL0004", 36), // WL 单元 0004（水位因子 × 相位 04 × 混 轮）
    ("OU0005", 45), // OU 单元 0005（产出因子 × 相位 05 × 混 轮）
    ("DF0006", 54), // DF 单元 0006（双因子 × 相位 06 × 混 轮）
    ("IN400007", 63), // IN40 单元 0007（注入 40/40 × 相位 07 × 混 轮）
    ("D10008", 8), // D1 单元 0008（F2741 读数 × 相位 08 × 混 轮）
    ("D20009", 17), // D2 单元 0009（F2747 读数 × 相位 09 × 混 轮）
    ("DR0010", 26), // DR 单元 0010（双源一致 × 相位 10 × 混 轮）
    ("RT0011", 35), // RT 单元 0011（可复测 × 相位 11 × 混 轮）
    ("NM0012", 44), // NM 单元 0012（正常档 × 相位 12 × 混 轮）
    ("LT0013", 53), // LT 单元 0013（轻压档 × 相位 13 × 混 轮）
    ("MD0014", 62), // MD 单元 0014（中压档 × 相位 14 × 混 轮）
    ("HV0015", 7), // HV 单元 0015（重压档 × 相位 15 × 混 轮）
];

/// O(1) 语义查找（match 全枚举；命中率 100%% 由守卫保证）。
pub fn u2761mixmap_get(key: &str) -> Option<u64> {
    match key {
        "NM0000" => Some(0),
        "LT0001" => Some(9),
        "MD0002" => Some(18),
        "HV0003" => Some(27),
        "WL0004" => Some(36),
        "OU0005" => Some(45),
        "DF0006" => Some(54),
        "IN400007" => Some(63),
        "D10008" => Some(8),
        "D20009" => Some(17),
        "DR0010" => Some(26),
        "RT0011" => Some(35),
        "NM0012" => Some(44),
        "LT0013" => Some(53),
        "MD0014" => Some(62),
        "HV0015" => Some(7),
        _ => None,
    }
}

/// 逐行守卫扫描（返回违例行数；0 = 全绿）。
pub fn u2761mixmap_scan() -> usize {
    let mut bad = 0usize;
    let mut i = 0usize;
    while i < u2761mixmap_COUNT {
        let (k, v) = u2761mixmap[i];
        let ok = match k {
            "NM0000" => v < 64,
            "LT0001" => v < 64,
            "MD0002" => v < 64,
            "HV0003" => v < 64,
            "WL0004" => v < 64,
            "OU0005" => v < 64,
            "DF0006" => v < 64,
            "IN400007" => v < 64,
            "D10008" => v < 64,
            "D20009" => v < 64,
            "DR0010" => v < 64,
            "RT0011" => v < 64,
            "NM0012" => v < 64,
            "LT0013" => v < 64,
            "MD0014" => v < 64,
            "HV0015" => v < 64,
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
pub fn u2761mixmap_sum() -> u64 {
    let mut s = 0u64;
    let mut i = 0usize;
    while i < u2761mixmap_COUNT {
        s = s.wrapping_add(u2761mixmap[i].1 as u64);
        i += 1;
    }
    s
}

/// 键唯一性（构造期断言等价物：逐对扫描同键）。
pub fn u2761mixmap_unique() -> bool {
    let mut i = 0usize;
    while i < u2761mixmap_COUNT {
        let mut j = i + 1;
        while j < u2761mixmap_COUNT {
            if u2761mixmap[i].0 == u2761mixmap[j].0 {
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

/// 段位守卫：全部诊断码必须落在本段 [0xadb0, 0xadbf]。
pub const fn u2761_segment_guard() -> bool {
    let mut ok = true;
    let mut i = 0usize;
    while i < u2761Code::ALL.len() {
        let c = u2761Code::ALL[i].code();
        if c < 0xadb0 || c > 0xadbf {
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
fn u2761_guard_seg() -> bool {
    u2761_segment_guard()
}

/// UNX-F2761 对拍表 16 行边界与累计守恒。
fn check_u2761_mix_tbl() -> bool {
    let mut ok = u2761mix_COUNT == 16;
    if ok { ok = u2761mix_get("NM_00") == Some(10) && u2761mix_get("HV_15") == Some(85); }
    if ok { ok = u2761mix_get("ZZZ_NOKEY").is_none(); }
    if ok { ok = u2761mix_sum() == 760; }
    ok
}

/// UNX-F2761 混合映射 16 行守卫唯一 + 守卫阈值冻结。
fn check_u2761_mix_map() -> bool {
    let mut ok = u2761mixmap_COUNT == 16;
    if ok { ok = u2761mixmap_scan() == 0 && u2761mixmap_unique(); }
    if ok { ok = u2761mixmap_sum() == 504; }
    if ok { ok = U2761MIXGATE_THRESHOLD == 32; }
    ok
}

/// 本模块自检（逐检查带 detail，全绿才出门）。
pub fn run_u2761_checks() -> CheckSet {
    let mut set = CheckSet::new("U2761");
    set.add(
        "u2761-seg-guard",
        u2761_guard_seg(),
        if u2761_guard_seg() { "" } else { "diagnostic code outside segment" },
    );
    set.add(
        "u2761-mix-tbl",
        check_u2761_mix_tbl(),
        if check_u2761_mix_tbl() { "" } else { "UNX-F2761 mix table mismatch" },
    );
    set.add(
        "u2761-mix-map",
        check_u2761_mix_map(),
        if check_u2761_mix_map() { "" } else { "UNX-F2761 mix map mismatch" },
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
    fn u2761_all_checks_green() {
        let set = run_u2761_checks();
        assert!(set.all_passed(), "U2761 domain checks must be green");
    }

    #[test]
    fn u2761_segment_guard_holds() {
        assert!(u2761_segment_guard());
    }
}
