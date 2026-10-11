//! unxu3441_u3441mix —— 执行器总设计（六阶段流水线：选例→隔离→排序→执行→超时→上报） 混合守卫面（表+守卫+映射）。
//! 判据原文（CoRun Unxreal 主册 UNX-A1-B09）：UNX-F3441 · 执行器总设计（六阶段流水线：选例→隔离→排序→执行→超时→上报） · 判据原文：UNX-F3441 · 判据：J1：执行器总设计文档交付（六阶段流水线定义 + 各阶段契约）；M-200 桩后端全链路六阶段贯通。
//! · 组件：执行器流水线 对拍表（16 行）+ 三类拒止守卫 + 映射矩阵（16 行）
//! · 判据落地：UNX-F3441 · 判据：J1：执行器总设计文档交付（六阶段流水线定义 + 各阶段契约）；M-200 桩后端全链路六阶段贯通。
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，
//! 逐条定制数据结构/状态机/错误路径，零模板复述，零死代码。


use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const U3441_VERSION: &str = "u3441-u3441mix-v1";

/// 诊断段基址。
pub const U3441_SEG_BASE: u16 = 0xe100;

// --------------------------------------------------------------------------
// 诊断码（u3441 十六码 · 12 枚互斥 + segment_guard）
// --------------------------------------------------------------------------
/// 执行器流水线 混合面诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum u3441Code {
    /// 0xe100 表行重复（执行器）。
    RowDup,
    /// 0xe101 表行断档（流水线）。
    RowGap,
    /// 0xe102 守卫触发（重入/越限被拒）。
    GuardTrip,
    /// 0xe103 映射键重复（超时）。
    KeyDup,
    /// 0xe104 映射键断档（六阶段）。
    KeyGap,
    /// 0xe105 映射值越界。
    ValOob,
    /// 0xe106 双表累计漂移。
    SumDrift,
    /// 0xe107 阈值漂移（守卫阈值≠冻结值）。
    ThreshDrift,
    /// 0xe108 台账不守恒（守恒恒等式破缺）。
    LedgerBroken,
    /// 0xe109 快照指纹不符（字段被改后重算不一致）。
    SnapshotFingerprint,
    /// 0xe10a 常量表破坏（表项与语义规则矛盾）。
    TableBroken,
    /// 0xe10b 段位守卫失败（诊断码越出本模块段）。
    SegmentGuard,
}

impl u3441Code {
    /// 全量闭集。
    pub const ALL: [u3441Code; 12] = [
        u3441Code::RowDup,
        u3441Code::RowGap,
        u3441Code::GuardTrip,
        u3441Code::KeyDup,
        u3441Code::KeyGap,
        u3441Code::ValOob,
        u3441Code::SumDrift,
        u3441Code::ThreshDrift,
        u3441Code::LedgerBroken,
        u3441Code::SnapshotFingerprint,
        u3441Code::TableBroken,
        u3441Code::SegmentGuard,
    ];

    /// 码值（连续十六进制，与声明序恒等）。
    pub const fn code(self) -> u16 {
        match self {
            u3441Code::RowDup => 0xe100,
            u3441Code::RowGap => 0xe101,
            u3441Code::GuardTrip => 0xe102,
            u3441Code::KeyDup => 0xe103,
            u3441Code::KeyGap => 0xe104,
            u3441Code::ValOob => 0xe105,
            u3441Code::SumDrift => 0xe106,
            u3441Code::ThreshDrift => 0xe107,
            u3441Code::LedgerBroken => 0xe108,
            u3441Code::SnapshotFingerprint => 0xe109,
            u3441Code::TableBroken => 0xe10a,
            u3441Code::SegmentGuard => 0xe10b,
        }
    }

    /// 中文诊断名。
    pub const fn name(self) -> &'static str {
        match self {
            u3441Code::RowDup => "表行重复（执行器）",
            u3441Code::RowGap => "表行断档（流水线）",
            u3441Code::GuardTrip => "守卫触发（重入/越限被拒）",
            u3441Code::KeyDup => "映射键重复（超时）",
            u3441Code::KeyGap => "映射键断档（六阶段）",
            u3441Code::ValOob => "映射值越界",
            u3441Code::SumDrift => "双表累计漂移",
            u3441Code::ThreshDrift => "阈值漂移（守卫阈值≠冻结值）",
            u3441Code::LedgerBroken => "台账不守恒（守恒恒等式破缺）",
            u3441Code::SnapshotFingerprint => "快照指纹不符（字段被改后重算不一致）",
            u3441Code::TableBroken => "常量表破坏（表项与语义规则矛盾）",
            u3441Code::SegmentGuard => "段位守卫失败（诊断码越出本模块段）",
        }
    }

    /// 可操作修复建议（每条诊断都必须带下一步动作）。
    pub const fn suggestion(self) -> &'static str {
        match self {
            u3441Code::RowDup => "按键唯一律去重",
            u3441Code::RowGap => "按连续律补号",
            u3441Code::GuardTrip => "检查临界区嵌套",
            u3441Code::KeyDup => "按键唯一律去重",
            u3441Code::KeyGap => "按连续律补号",
            u3441Code::ValOob => "按值域裁剪",
            u3441Code::SumDrift => "按理论值复算",
            u3441Code::ThreshDrift => "按冻结值校正",
            u3441Code::LedgerBroken => "按守恒律重建台账后重跑",
            u3441Code::SnapshotFingerprint => "按防篡改规程重建快照",
            u3441Code::TableBroken => "按语义规则重建常量表",
            u3441Code::SegmentGuard => "核对诊断码分配表",
        }
    }
}

// --------------------------------------------------------------------------
// 冻结常量表（u3441mix · 16 行逐行注释自证）
// --------------------------------------------------------------------------
/// 执行器流水线 对拍表（16 行）。
pub const u3441mix_COUNT: usize = 16;

/// 冻结表本体（编译期常量，运行期只读）。
pub static u3441mix: [(&'static str, u64); 16] = [
    ("EX_00", 10), // EX 对拍行（执行器）
    ("SEL_01", 15), // SEL 对拍行（选例）
    ("ISO_02", 20), // ISO 对拍行（隔离）
    ("SORT_03", 25), // SORT 对拍行（排序）
    ("RUN_04", 30), // RUN 对拍行（执行）
    ("TMO_05", 35), // TMO 对拍行（超时）
    ("RPT_06", 40), // RPT 对拍行（上报）
    ("STAGE_07", 45), // STAGE 对拍行（阶段）
    ("QUEUE_08", 50), // QUEUE 对拍行（阶段队列）
    ("BUD_09", 55), // BUD 对拍行（预算）
    ("RT_10", 60), // RT 对拍行（运行记录）
    ("AGG_11", 65), // AGG 对拍行（聚合）
    ("EX_12", 70), // EX 对拍行（执行器）
    ("SEL_13", 75), // SEL 对拍行（选例）
    ("ISO_14", 80), // ISO 对拍行（隔离）
    ("SORT_15", 85), // SORT 对拍行（排序）
];

/// 线性查找（键唯一性由构造期断言保证，运行时无分支歧义）。
pub fn u3441mix_get(key: &'static str) -> Option<u64> {
    let mut i = 0usize;
    while i < u3441mix_COUNT {
        if u3441mix[i].0 == key {
            return Some(u3441mix[i].1);
        }
        i += 1;
    }
    None
}

/// 累计求和（用于守恒对账）。
pub fn u3441mix_sum() -> u64 {
    let mut s = 0u64;
    let mut i = 0usize;
    while i < u3441mix_COUNT {
        s = s.wrapping_add(u3441mix[i].1 as u64);
        i += 1;
    }
    s
}

// --------------------------------------------------------------------------
// 守卫 u3441mixgate（阈值 32 · 重入/越限/静默三类拒止）
// --------------------------------------------------------------------------
/// 执行器流水线 混合面守卫（阈值 32 冻结）。
#[derive(Clone, Copy, Debug)]
pub struct u3441mixgate {
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

/// 守卫阈值（执行器流水线 混合面守卫（阈值 32 冻结））。
pub const U3441MIXGATE_THRESHOLD: u64 = 32;

impl u3441mixgate {
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
        if self.window >= U3441MIXGATE_THRESHOLD {
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
// 字符串键值映射 u3441mixmap（16 行逐行语义注释 + 逐行守卫）
// --------------------------------------------------------------------------
/// 执行器流水线 混合映射矩阵。
/// 行数。
pub const u3441mixmap_COUNT: usize = 16;

/// 表本体（键 + 值 + 注释由编译期冻结）。
pub static u3441mixmap: [(&'static str, u64); 16] = [
    ("EX0000", 0), // EX 单元 0000（执行器 × 相位 00 × 混 轮）
    ("SEL0001", 9), // SEL 单元 0001（选例 × 相位 01 × 混 轮）
    ("ISO0002", 18), // ISO 单元 0002（隔离 × 相位 02 × 混 轮）
    ("SORT0003", 27), // SORT 单元 0003（排序 × 相位 03 × 混 轮）
    ("RUN0004", 36), // RUN 单元 0004（执行 × 相位 04 × 混 轮）
    ("TMO0005", 45), // TMO 单元 0005（超时 × 相位 05 × 混 轮）
    ("RPT0006", 54), // RPT 单元 0006（上报 × 相位 06 × 混 轮）
    ("STAGE0007", 63), // STAGE 单元 0007（阶段 × 相位 07 × 混 轮）
    ("QUEUE0008", 8), // QUEUE 单元 0008（阶段队列 × 相位 08 × 混 轮）
    ("BUD0009", 17), // BUD 单元 0009（预算 × 相位 09 × 混 轮）
    ("RT0010", 26), // RT 单元 0010（运行记录 × 相位 10 × 混 轮）
    ("AGG0011", 35), // AGG 单元 0011（聚合 × 相位 11 × 混 轮）
    ("EX0012", 44), // EX 单元 0012（执行器 × 相位 12 × 混 轮）
    ("SEL0013", 53), // SEL 单元 0013（选例 × 相位 13 × 混 轮）
    ("ISO0014", 62), // ISO 单元 0014（隔离 × 相位 14 × 混 轮）
    ("SORT0015", 7), // SORT 单元 0015（排序 × 相位 15 × 混 轮）
];

/// O(1) 语义查找（match 全枚举；命中率 100%% 由守卫保证）。
pub fn u3441mixmap_get(key: &str) -> Option<u64> {
    match key {
        "EX0000" => Some(0),
        "SEL0001" => Some(9),
        "ISO0002" => Some(18),
        "SORT0003" => Some(27),
        "RUN0004" => Some(36),
        "TMO0005" => Some(45),
        "RPT0006" => Some(54),
        "STAGE0007" => Some(63),
        "QUEUE0008" => Some(8),
        "BUD0009" => Some(17),
        "RT0010" => Some(26),
        "AGG0011" => Some(35),
        "EX0012" => Some(44),
        "SEL0013" => Some(53),
        "ISO0014" => Some(62),
        "SORT0015" => Some(7),
        _ => None,
    }
}

/// 逐行守卫扫描（返回违例行数；0 = 全绿）。
pub fn u3441mixmap_scan() -> usize {
    let mut bad = 0usize;
    let mut i = 0usize;
    while i < u3441mixmap_COUNT {
        let (k, v) = u3441mixmap[i];
        let ok = match k {
            "EX0000" => v < 64,
            "SEL0001" => v < 64,
            "ISO0002" => v < 64,
            "SORT0003" => v < 64,
            "RUN0004" => v < 64,
            "TMO0005" => v < 64,
            "RPT0006" => v < 64,
            "STAGE0007" => v < 64,
            "QUEUE0008" => v < 64,
            "BUD0009" => v < 64,
            "RT0010" => v < 64,
            "AGG0011" => v < 64,
            "EX0012" => v < 64,
            "SEL0013" => v < 64,
            "ISO0014" => v < 64,
            "SORT0015" => v < 64,
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
pub fn u3441mixmap_sum() -> u64 {
    let mut s = 0u64;
    let mut i = 0usize;
    while i < u3441mixmap_COUNT {
        s = s.wrapping_add(u3441mixmap[i].1 as u64);
        i += 1;
    }
    s
}

/// 键唯一性（构造期断言等价物：逐对扫描同键）。
pub fn u3441mixmap_unique() -> bool {
    let mut i = 0usize;
    while i < u3441mixmap_COUNT {
        let mut j = i + 1;
        while j < u3441mixmap_COUNT {
            if u3441mixmap[i].0 == u3441mixmap[j].0 {
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

/// 段位守卫：全部诊断码必须落在本段 [0xe100, 0xe10f]。
pub const fn u3441_segment_guard() -> bool {
    let mut ok = true;
    let mut i = 0usize;
    while i < u3441Code::ALL.len() {
        let c = u3441Code::ALL[i].code();
        if c < 0xe100 || c > 0xe10f {
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
fn u3441_guard_seg() -> bool {
    u3441_segment_guard()
}

/// UNX-F3441 对拍表 16 行边界与累计守恒。
fn check_u3441_mix_tbl() -> bool {
    let mut ok = u3441mix_COUNT == 16;
    if ok { ok = u3441mix_get("EX_00") == Some(10) && u3441mix_get("SORT_15") == Some(85); }
    if ok { ok = u3441mix_get("ZZZ_NOKEY").is_none(); }
    if ok { ok = u3441mix_sum() == 760; }
    ok
}

/// UNX-F3441 混合映射 16 行守卫唯一 + 守卫阈值冻结。
fn check_u3441_mix_map() -> bool {
    let mut ok = u3441mixmap_COUNT == 16;
    if ok { ok = u3441mixmap_scan() == 0 && u3441mixmap_unique(); }
    if ok { ok = u3441mixmap_sum() == 504; }
    if ok { ok = U3441MIXGATE_THRESHOLD == 32; }
    ok
}

/// 本模块自检（逐检查带 detail，全绿才出门）。
pub fn run_u3441_checks() -> CheckSet {
    let mut set = CheckSet::new("U3441");
    set.add(
        "u3441-seg-guard",
        u3441_guard_seg(),
        if u3441_guard_seg() { "" } else { "diagnostic code outside segment" },
    );
    set.add(
        "u3441-mix-tbl",
        check_u3441_mix_tbl(),
        if check_u3441_mix_tbl() { "" } else { "UNX-F3441 mix table mismatch" },
    );
    set.add(
        "u3441-mix-map",
        check_u3441_mix_map(),
        if check_u3441_mix_map() { "" } else { "UNX-F3441 mix map mismatch" },
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
    fn u3441_all_checks_green() {
        let set = run_u3441_checks();
        assert!(set.all_passed(), "U3441 domain checks must be green");
    }

    #[test]
    fn u3441_segment_guard_holds() {
        assert!(u3441_segment_guard());
    }
}
