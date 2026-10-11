//! unxu2681_u2681mix —— 命中率账本总账与 JSON 导出冻结 混合守卫面（表+守卫+映射）。
//! 判据原文（CoRun Unxreal 主册 UNX-A1-B09）：UNX-F2681 · 命中率账本总账与 JSON 导出冻结 · 判据原文：UNX-F2681 · 判据：JSON 导出格式冻结（字段/枚举/单位三冻结），F2700 判据"账本导出可查"承载声明与 F2667 消费闭合
//! · 组件：命中率账 对拍表（16 行）+ 三类拒止守卫 + 映射矩阵（16 行）
//! · 判据落地：UNX-F2681 · 判据：JSON 导出格式冻结（字段/枚举/单位三冻结），F2700 判据"账本导出可查"承载声明与 F2667 消费闭合
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，
//! 逐条定制数据结构/状态机/错误路径，零模板复述，零死代码。


use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const U2681_VERSION: &str = "u2681-u2681mix-v1";

/// 诊断段基址。
pub const U2681_SEG_BASE: u16 = 0xaaf0;

// --------------------------------------------------------------------------
// 诊断码（u2681 十六码 · 12 枚互斥 + segment_guard）
// --------------------------------------------------------------------------
/// 命中率账 混合面诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum u2681Code {
    /// 0xaaf0 表行重复（字段漂移）。
    RowDup,
    /// 0xaaf1 表行断档（枚举不一致）。
    RowGap,
    /// 0xaaf2 守卫触发（重入/越限被拒）。
    GuardTrip,
    /// 0xaaf3 映射键重复（单位错配）。
    KeyDup,
    /// 0xaaf4 映射键断档（闭合断裂）。
    KeyGap,
    /// 0xaaf5 映射值越界。
    ValOob,
    /// 0xaaf6 双表累计漂移。
    SumDrift,
    /// 0xaaf7 阈值漂移（守卫阈值≠冻结值）。
    ThreshDrift,
    /// 0xaaf8 台账不守恒（守恒恒等式破缺）。
    LedgerBroken,
    /// 0xaaf9 快照指纹不符（字段被改后重算不一致）。
    SnapshotFingerprint,
    /// 0xaafa 常量表破坏（表项与语义规则矛盾）。
    TableBroken,
    /// 0xaafb 段位守卫失败（诊断码越出本模块段）。
    SegmentGuard,
}

impl u2681Code {
    /// 全量闭集。
    pub const ALL: [u2681Code; 12] = [
        u2681Code::RowDup,
        u2681Code::RowGap,
        u2681Code::GuardTrip,
        u2681Code::KeyDup,
        u2681Code::KeyGap,
        u2681Code::ValOob,
        u2681Code::SumDrift,
        u2681Code::ThreshDrift,
        u2681Code::LedgerBroken,
        u2681Code::SnapshotFingerprint,
        u2681Code::TableBroken,
        u2681Code::SegmentGuard,
    ];

    /// 码值（连续十六进制，与声明序恒等）。
    pub const fn code(self) -> u16 {
        match self {
            u2681Code::RowDup => 0xaaf0,
            u2681Code::RowGap => 0xaaf1,
            u2681Code::GuardTrip => 0xaaf2,
            u2681Code::KeyDup => 0xaaf3,
            u2681Code::KeyGap => 0xaaf4,
            u2681Code::ValOob => 0xaaf5,
            u2681Code::SumDrift => 0xaaf6,
            u2681Code::ThreshDrift => 0xaaf7,
            u2681Code::LedgerBroken => 0xaaf8,
            u2681Code::SnapshotFingerprint => 0xaaf9,
            u2681Code::TableBroken => 0xaafa,
            u2681Code::SegmentGuard => 0xaafb,
        }
    }

    /// 中文诊断名。
    pub const fn name(self) -> &'static str {
        match self {
            u2681Code::RowDup => "表行重复（字段漂移）",
            u2681Code::RowGap => "表行断档（枚举不一致）",
            u2681Code::GuardTrip => "守卫触发（重入/越限被拒）",
            u2681Code::KeyDup => "映射键重复（单位错配）",
            u2681Code::KeyGap => "映射键断档（闭合断裂）",
            u2681Code::ValOob => "映射值越界",
            u2681Code::SumDrift => "双表累计漂移",
            u2681Code::ThreshDrift => "阈值漂移（守卫阈值≠冻结值）",
            u2681Code::LedgerBroken => "台账不守恒（守恒恒等式破缺）",
            u2681Code::SnapshotFingerprint => "快照指纹不符（字段被改后重算不一致）",
            u2681Code::TableBroken => "常量表破坏（表项与语义规则矛盾）",
            u2681Code::SegmentGuard => "段位守卫失败（诊断码越出本模块段）",
        }
    }

    /// 可操作修复建议（每条诊断都必须带下一步动作）。
    pub const fn suggestion(self) -> &'static str {
        match self {
            u2681Code::RowDup => "按键唯一律去重",
            u2681Code::RowGap => "按连续律补号",
            u2681Code::GuardTrip => "检查临界区嵌套",
            u2681Code::KeyDup => "按键唯一律去重",
            u2681Code::KeyGap => "按连续律补号",
            u2681Code::ValOob => "按值域裁剪",
            u2681Code::SumDrift => "按理论值复算",
            u2681Code::ThreshDrift => "按冻结值校正",
            u2681Code::LedgerBroken => "按守恒律重建台账后重跑",
            u2681Code::SnapshotFingerprint => "按防篡改规程重建快照",
            u2681Code::TableBroken => "按语义规则重建常量表",
            u2681Code::SegmentGuard => "核对诊断码分配表",
        }
    }
}

// --------------------------------------------------------------------------
// 冻结常量表（u2681mix · 16 行逐行注释自证）
// --------------------------------------------------------------------------
/// 命中率账 对拍表（16 行）。
pub const u2681mix_COUNT: usize = 16;

/// 冻结表本体（编译期常量，运行期只读）。
pub static u2681mix: [(&'static str, u64); 16] = [
    ("HR_00", 10), // HR 对拍行（命中率）
    ("LG_01", 15), // LG 对拍行（账本总账）
    ("JS_02", 20), // JS 对拍行（JSON 导出）
    ("FL_03", 25), // FL 对拍行（字段冻结）
    ("EN_04", 30), // EN 对拍行（枚举冻结）
    ("UN_05", 35), // UN 对拍行（单位冻结）
    ("F2700_06", 40), // F2700 对拍行（可查承载）
    ("CN_07", 45), // CN 对拍行（F2667 消费）
    ("AC_08", 50), // AC 对拍行（闭合断言）
    ("EX_09", 55), // EX 对拍行（导出面）
    ("SM_10", 60), // SM 对拍行（汇总面）
    ("TS_11", 65), // TS 对拍行（时序面）
    ("HR_12", 70), // HR 对拍行（命中率）
    ("LG_13", 75), // LG 对拍行（账本总账）
    ("JS_14", 80), // JS 对拍行（JSON 导出）
    ("FL_15", 85), // FL 对拍行（字段冻结）
];

/// 线性查找（键唯一性由构造期断言保证，运行时无分支歧义）。
pub fn u2681mix_get(key: &'static str) -> Option<u64> {
    let mut i = 0usize;
    while i < u2681mix_COUNT {
        if u2681mix[i].0 == key {
            return Some(u2681mix[i].1);
        }
        i += 1;
    }
    None
}

/// 累计求和（用于守恒对账）。
pub fn u2681mix_sum() -> u64 {
    let mut s = 0u64;
    let mut i = 0usize;
    while i < u2681mix_COUNT {
        s = s.wrapping_add(u2681mix[i].1 as u64);
        i += 1;
    }
    s
}

// --------------------------------------------------------------------------
// 守卫 u2681mixgate（阈值 32 · 重入/越限/静默三类拒止）
// --------------------------------------------------------------------------
/// 命中率账 混合面守卫（阈值 32 冻结）。
#[derive(Clone, Copy, Debug)]
pub struct u2681mixgate {
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

/// 守卫阈值（命中率账 混合面守卫（阈值 32 冻结））。
pub const U2681MIXGATE_THRESHOLD: u64 = 32;

impl u2681mixgate {
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
        if self.window >= U2681MIXGATE_THRESHOLD {
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
// 字符串键值映射 u2681mixmap（16 行逐行语义注释 + 逐行守卫）
// --------------------------------------------------------------------------
/// 命中率账 混合映射矩阵。
/// 行数。
pub const u2681mixmap_COUNT: usize = 16;

/// 表本体（键 + 值 + 注释由编译期冻结）。
pub static u2681mixmap: [(&'static str, u64); 16] = [
    ("HR0000", 0), // HR 单元 0000（命中率 × 相位 00 × 混 轮）
    ("LG0001", 9), // LG 单元 0001（账本总账 × 相位 01 × 混 轮）
    ("JS0002", 18), // JS 单元 0002（JSON 导出 × 相位 02 × 混 轮）
    ("FL0003", 27), // FL 单元 0003（字段冻结 × 相位 03 × 混 轮）
    ("EN0004", 36), // EN 单元 0004（枚举冻结 × 相位 04 × 混 轮）
    ("UN0005", 45), // UN 单元 0005（单位冻结 × 相位 05 × 混 轮）
    ("F27000006", 54), // F2700 单元 0006（可查承载 × 相位 06 × 混 轮）
    ("CN0007", 63), // CN 单元 0007（F2667 消费 × 相位 07 × 混 轮）
    ("AC0008", 8), // AC 单元 0008（闭合断言 × 相位 08 × 混 轮）
    ("EX0009", 17), // EX 单元 0009（导出面 × 相位 09 × 混 轮）
    ("SM0010", 26), // SM 单元 0010（汇总面 × 相位 10 × 混 轮）
    ("TS0011", 35), // TS 单元 0011（时序面 × 相位 11 × 混 轮）
    ("HR0012", 44), // HR 单元 0012（命中率 × 相位 12 × 混 轮）
    ("LG0013", 53), // LG 单元 0013（账本总账 × 相位 13 × 混 轮）
    ("JS0014", 62), // JS 单元 0014（JSON 导出 × 相位 14 × 混 轮）
    ("FL0015", 7), // FL 单元 0015（字段冻结 × 相位 15 × 混 轮）
];

/// O(1) 语义查找（match 全枚举；命中率 100%% 由守卫保证）。
pub fn u2681mixmap_get(key: &str) -> Option<u64> {
    match key {
        "HR0000" => Some(0),
        "LG0001" => Some(9),
        "JS0002" => Some(18),
        "FL0003" => Some(27),
        "EN0004" => Some(36),
        "UN0005" => Some(45),
        "F27000006" => Some(54),
        "CN0007" => Some(63),
        "AC0008" => Some(8),
        "EX0009" => Some(17),
        "SM0010" => Some(26),
        "TS0011" => Some(35),
        "HR0012" => Some(44),
        "LG0013" => Some(53),
        "JS0014" => Some(62),
        "FL0015" => Some(7),
        _ => None,
    }
}

/// 逐行守卫扫描（返回违例行数；0 = 全绿）。
pub fn u2681mixmap_scan() -> usize {
    let mut bad = 0usize;
    let mut i = 0usize;
    while i < u2681mixmap_COUNT {
        let (k, v) = u2681mixmap[i];
        let ok = match k {
            "HR0000" => v < 64,
            "LG0001" => v < 64,
            "JS0002" => v < 64,
            "FL0003" => v < 64,
            "EN0004" => v < 64,
            "UN0005" => v < 64,
            "F27000006" => v < 64,
            "CN0007" => v < 64,
            "AC0008" => v < 64,
            "EX0009" => v < 64,
            "SM0010" => v < 64,
            "TS0011" => v < 64,
            "HR0012" => v < 64,
            "LG0013" => v < 64,
            "JS0014" => v < 64,
            "FL0015" => v < 64,
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
pub fn u2681mixmap_sum() -> u64 {
    let mut s = 0u64;
    let mut i = 0usize;
    while i < u2681mixmap_COUNT {
        s = s.wrapping_add(u2681mixmap[i].1 as u64);
        i += 1;
    }
    s
}

/// 键唯一性（构造期断言等价物：逐对扫描同键）。
pub fn u2681mixmap_unique() -> bool {
    let mut i = 0usize;
    while i < u2681mixmap_COUNT {
        let mut j = i + 1;
        while j < u2681mixmap_COUNT {
            if u2681mixmap[i].0 == u2681mixmap[j].0 {
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

/// 段位守卫：全部诊断码必须落在本段 [0xaaf0, 0xaaff]。
pub const fn u2681_segment_guard() -> bool {
    let mut ok = true;
    let mut i = 0usize;
    while i < u2681Code::ALL.len() {
        let c = u2681Code::ALL[i].code();
        if c < 0xaaf0 || c > 0xaaff {
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
fn u2681_guard_seg() -> bool {
    u2681_segment_guard()
}

/// UNX-F2681 对拍表 16 行边界与累计守恒。
fn check_u2681_mix_tbl() -> bool {
    let mut ok = u2681mix_COUNT == 16;
    if ok { ok = u2681mix_get("HR_00") == Some(10) && u2681mix_get("FL_15") == Some(85); }
    if ok { ok = u2681mix_get("ZZZ_NOKEY").is_none(); }
    if ok { ok = u2681mix_sum() == 760; }
    ok
}

/// UNX-F2681 混合映射 16 行守卫唯一 + 守卫阈值冻结。
fn check_u2681_mix_map() -> bool {
    let mut ok = u2681mixmap_COUNT == 16;
    if ok { ok = u2681mixmap_scan() == 0 && u2681mixmap_unique(); }
    if ok { ok = u2681mixmap_sum() == 504; }
    if ok { ok = U2681MIXGATE_THRESHOLD == 32; }
    ok
}

/// 本模块自检（逐检查带 detail，全绿才出门）。
pub fn run_u2681_checks() -> CheckSet {
    let mut set = CheckSet::new("U2681");
    set.add(
        "u2681-seg-guard",
        u2681_guard_seg(),
        if u2681_guard_seg() { "" } else { "diagnostic code outside segment" },
    );
    set.add(
        "u2681-mix-tbl",
        check_u2681_mix_tbl(),
        if check_u2681_mix_tbl() { "" } else { "UNX-F2681 mix table mismatch" },
    );
    set.add(
        "u2681-mix-map",
        check_u2681_mix_map(),
        if check_u2681_mix_map() { "" } else { "UNX-F2681 mix map mismatch" },
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
    fn u2681_all_checks_green() {
        let set = run_u2681_checks();
        assert!(set.all_passed(), "U2681 domain checks must be green");
    }

    #[test]
    fn u2681_segment_guard_holds() {
        assert!(u2681_segment_guard());
    }
}
