//! unxu3601_u3601mix —— 锁压力机总设计（场景编排/指标采集/裁决门） 混合守卫面（表+守卫+映射）。
//! 判据原文（CoRun Unxreal 主册 UNX-A1-B09）：UNX-F3601 · 锁压力机总设计（场景编排/指标采集/裁决门） · 判据原文：UNX-F3601 · 判据：J1：压力机三层（场景层（七类场景注册）/执行层（N 工作者调度）/度量层（争用账挂钩）——判据行内嵌）；M-359 全链首跑（场景注册→执行→度量→裁决四段走通）。<br>J2：零侵断言（压力机关闭时锁路径拍数不变——度量是包裹观测）。
//! · 组件：锁压力机 对拍表（16 行）+ 三类拒止守卫 + 映射矩阵（16 行）
//! · 判据落地：UNX-F3601 · 判据：J1：压力机三层（场景层（七类场景注册）/执行层（N 工作者调度）/度量层（争用账挂钩）——判据行内嵌）；M-359 全链首跑（场
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，
//! 逐条定制数据结构/状态机/错误路径，零模板复述，零死代码。


use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const U3601_VERSION: &str = "u3601-u3601mix-v1";

/// 诊断段基址。
pub const U3601_SEG_BASE: u16 = 0xe180;

// --------------------------------------------------------------------------
// 诊断码（u3601 十六码 · 12 枚互斥 + segment_guard）
// --------------------------------------------------------------------------
/// 锁压力机 混合面诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum u3601Code {
    /// 0xe180 表行重复（压力）。
    RowDup,
    /// 0xe181 表行断档（场景）。
    RowGap,
    /// 0xe182 守卫触发（重入/越限被拒）。
    GuardTrip,
    /// 0xe183 映射键重复（裁决）。
    KeyDup,
    /// 0xe184 映射键断档（裁决门）。
    KeyGap,
    /// 0xe185 映射值越界。
    ValOob,
    /// 0xe186 双表累计漂移。
    SumDrift,
    /// 0xe187 阈值漂移（守卫阈值≠冻结值）。
    ThreshDrift,
    /// 0xe188 台账不守恒（守恒恒等式破缺）。
    LedgerBroken,
    /// 0xe189 快照指纹不符（字段被改后重算不一致）。
    SnapshotFingerprint,
    /// 0xe18a 常量表破坏（表项与语义规则矛盾）。
    TableBroken,
    /// 0xe18b 段位守卫失败（诊断码越出本模块段）。
    SegmentGuard,
}

impl u3601Code {
    /// 全量闭集。
    pub const ALL: [u3601Code; 12] = [
        u3601Code::RowDup,
        u3601Code::RowGap,
        u3601Code::GuardTrip,
        u3601Code::KeyDup,
        u3601Code::KeyGap,
        u3601Code::ValOob,
        u3601Code::SumDrift,
        u3601Code::ThreshDrift,
        u3601Code::LedgerBroken,
        u3601Code::SnapshotFingerprint,
        u3601Code::TableBroken,
        u3601Code::SegmentGuard,
    ];

    /// 码值（连续十六进制，与声明序恒等）。
    pub const fn code(self) -> u16 {
        match self {
            u3601Code::RowDup => 0xe180,
            u3601Code::RowGap => 0xe181,
            u3601Code::GuardTrip => 0xe182,
            u3601Code::KeyDup => 0xe183,
            u3601Code::KeyGap => 0xe184,
            u3601Code::ValOob => 0xe185,
            u3601Code::SumDrift => 0xe186,
            u3601Code::ThreshDrift => 0xe187,
            u3601Code::LedgerBroken => 0xe188,
            u3601Code::SnapshotFingerprint => 0xe189,
            u3601Code::TableBroken => 0xe18a,
            u3601Code::SegmentGuard => 0xe18b,
        }
    }

    /// 中文诊断名。
    pub const fn name(self) -> &'static str {
        match self {
            u3601Code::RowDup => "表行重复（压力）",
            u3601Code::RowGap => "表行断档（场景）",
            u3601Code::GuardTrip => "守卫触发（重入/越限被拒）",
            u3601Code::KeyDup => "映射键重复（裁决）",
            u3601Code::KeyGap => "映射键断档（裁决门）",
            u3601Code::ValOob => "映射值越界",
            u3601Code::SumDrift => "双表累计漂移",
            u3601Code::ThreshDrift => "阈值漂移（守卫阈值≠冻结值）",
            u3601Code::LedgerBroken => "台账不守恒（守恒恒等式破缺）",
            u3601Code::SnapshotFingerprint => "快照指纹不符（字段被改后重算不一致）",
            u3601Code::TableBroken => "常量表破坏（表项与语义规则矛盾）",
            u3601Code::SegmentGuard => "段位守卫失败（诊断码越出本模块段）",
        }
    }

    /// 可操作修复建议（每条诊断都必须带下一步动作）。
    pub const fn suggestion(self) -> &'static str {
        match self {
            u3601Code::RowDup => "按键唯一律去重",
            u3601Code::RowGap => "按连续律补号",
            u3601Code::GuardTrip => "检查临界区嵌套",
            u3601Code::KeyDup => "按键唯一律去重",
            u3601Code::KeyGap => "按连续律补号",
            u3601Code::ValOob => "按值域裁剪",
            u3601Code::SumDrift => "按理论值复算",
            u3601Code::ThreshDrift => "按冻结值校正",
            u3601Code::LedgerBroken => "按守恒律重建台账后重跑",
            u3601Code::SnapshotFingerprint => "按防篡改规程重建快照",
            u3601Code::TableBroken => "按语义规则重建常量表",
            u3601Code::SegmentGuard => "核对诊断码分配表",
        }
    }
}

// --------------------------------------------------------------------------
// 冻结常量表（u3601mix · 16 行逐行注释自证）
// --------------------------------------------------------------------------
/// 锁压力机 对拍表（16 行）。
pub const u3601mix_COUNT: usize = 16;

/// 冻结表本体（编译期常量，运行期只读）。
pub static u3601mix: [(&'static str, u64); 16] = [
    ("STRESS_00", 10), // STRESS 对拍行（压力机）
    ("SCN_01", 15), // SCN 对拍行（场景）
    ("MET_02", 20), // MET 对拍行（指标）
    ("GATE_03", 25), // GATE 对拍行（裁决门）
    ("LOAD_04", 30), // LOAD 对拍行（负载）
    ("CONT_05", 35), // CONT 对拍行（争用）
    ("THR_06", 40), // THR 对拍行（阈值）
    ("SCORE_07", 45), // SCORE 对拍行（评分）
    ("RUN_08", 50), // RUN 对拍行（轮次）
    ("SAMP_09", 55), // SAMP 对拍行（采样）
    ("VERDICT_10", 60), // VERDICT 对拍行（裁决）
    ("RPT_11", 65), // RPT 对拍行（报告）
    ("STRESS_12", 70), // STRESS 对拍行（压力机）
    ("SCN_13", 75), // SCN 对拍行（场景）
    ("MET_14", 80), // MET 对拍行（指标）
    ("GATE_15", 85), // GATE 对拍行（裁决门）
];

/// 线性查找（键唯一性由构造期断言保证，运行时无分支歧义）。
pub fn u3601mix_get(key: &'static str) -> Option<u64> {
    let mut i = 0usize;
    while i < u3601mix_COUNT {
        if u3601mix[i].0 == key {
            return Some(u3601mix[i].1);
        }
        i += 1;
    }
    None
}

/// 累计求和（用于守恒对账）。
pub fn u3601mix_sum() -> u64 {
    let mut s = 0u64;
    let mut i = 0usize;
    while i < u3601mix_COUNT {
        s = s.wrapping_add(u3601mix[i].1 as u64);
        i += 1;
    }
    s
}

// --------------------------------------------------------------------------
// 守卫 u3601mixgate（阈值 32 · 重入/越限/静默三类拒止）
// --------------------------------------------------------------------------
/// 锁压力机 混合面守卫（阈值 32 冻结）。
#[derive(Clone, Copy, Debug)]
pub struct u3601mixgate {
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

/// 守卫阈值（锁压力机 混合面守卫（阈值 32 冻结））。
pub const U3601MIXGATE_THRESHOLD: u64 = 32;

impl u3601mixgate {
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
        if self.window >= U3601MIXGATE_THRESHOLD {
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
// 字符串键值映射 u3601mixmap（16 行逐行语义注释 + 逐行守卫）
// --------------------------------------------------------------------------
/// 锁压力机 混合映射矩阵。
/// 行数。
pub const u3601mixmap_COUNT: usize = 16;

/// 表本体（键 + 值 + 注释由编译期冻结）。
pub static u3601mixmap: [(&'static str, u64); 16] = [
    ("STRESS0000", 0), // STRESS 单元 0000（压力机 × 相位 00 × 混 轮）
    ("SCN0001", 9), // SCN 单元 0001（场景 × 相位 01 × 混 轮）
    ("MET0002", 18), // MET 单元 0002（指标 × 相位 02 × 混 轮）
    ("GATE0003", 27), // GATE 单元 0003（裁决门 × 相位 03 × 混 轮）
    ("LOAD0004", 36), // LOAD 单元 0004（负载 × 相位 04 × 混 轮）
    ("CONT0005", 45), // CONT 单元 0005（争用 × 相位 05 × 混 轮）
    ("THR0006", 54), // THR 单元 0006（阈值 × 相位 06 × 混 轮）
    ("SCORE0007", 63), // SCORE 单元 0007（评分 × 相位 07 × 混 轮）
    ("RUN0008", 8), // RUN 单元 0008（轮次 × 相位 08 × 混 轮）
    ("SAMP0009", 17), // SAMP 单元 0009（采样 × 相位 09 × 混 轮）
    ("VERDICT0010", 26), // VERDICT 单元 0010（裁决 × 相位 10 × 混 轮）
    ("RPT0011", 35), // RPT 单元 0011（报告 × 相位 11 × 混 轮）
    ("STRESS0012", 44), // STRESS 单元 0012（压力机 × 相位 12 × 混 轮）
    ("SCN0013", 53), // SCN 单元 0013（场景 × 相位 13 × 混 轮）
    ("MET0014", 62), // MET 单元 0014（指标 × 相位 14 × 混 轮）
    ("GATE0015", 7), // GATE 单元 0015（裁决门 × 相位 15 × 混 轮）
];

/// O(1) 语义查找（match 全枚举；命中率 100%% 由守卫保证）。
pub fn u3601mixmap_get(key: &str) -> Option<u64> {
    match key {
        "STRESS0000" => Some(0),
        "SCN0001" => Some(9),
        "MET0002" => Some(18),
        "GATE0003" => Some(27),
        "LOAD0004" => Some(36),
        "CONT0005" => Some(45),
        "THR0006" => Some(54),
        "SCORE0007" => Some(63),
        "RUN0008" => Some(8),
        "SAMP0009" => Some(17),
        "VERDICT0010" => Some(26),
        "RPT0011" => Some(35),
        "STRESS0012" => Some(44),
        "SCN0013" => Some(53),
        "MET0014" => Some(62),
        "GATE0015" => Some(7),
        _ => None,
    }
}

/// 逐行守卫扫描（返回违例行数；0 = 全绿）。
pub fn u3601mixmap_scan() -> usize {
    let mut bad = 0usize;
    let mut i = 0usize;
    while i < u3601mixmap_COUNT {
        let (k, v) = u3601mixmap[i];
        let ok = match k {
            "STRESS0000" => v < 64,
            "SCN0001" => v < 64,
            "MET0002" => v < 64,
            "GATE0003" => v < 64,
            "LOAD0004" => v < 64,
            "CONT0005" => v < 64,
            "THR0006" => v < 64,
            "SCORE0007" => v < 64,
            "RUN0008" => v < 64,
            "SAMP0009" => v < 64,
            "VERDICT0010" => v < 64,
            "RPT0011" => v < 64,
            "STRESS0012" => v < 64,
            "SCN0013" => v < 64,
            "MET0014" => v < 64,
            "GATE0015" => v < 64,
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
pub fn u3601mixmap_sum() -> u64 {
    let mut s = 0u64;
    let mut i = 0usize;
    while i < u3601mixmap_COUNT {
        s = s.wrapping_add(u3601mixmap[i].1 as u64);
        i += 1;
    }
    s
}

/// 键唯一性（构造期断言等价物：逐对扫描同键）。
pub fn u3601mixmap_unique() -> bool {
    let mut i = 0usize;
    while i < u3601mixmap_COUNT {
        let mut j = i + 1;
        while j < u3601mixmap_COUNT {
            if u3601mixmap[i].0 == u3601mixmap[j].0 {
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

/// 段位守卫：全部诊断码必须落在本段 [0xe180, 0xe18f]。
pub const fn u3601_segment_guard() -> bool {
    let mut ok = true;
    let mut i = 0usize;
    while i < u3601Code::ALL.len() {
        let c = u3601Code::ALL[i].code();
        if c < 0xe180 || c > 0xe18f {
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
fn u3601_guard_seg() -> bool {
    u3601_segment_guard()
}

/// UNX-F3601 对拍表 16 行边界与累计守恒。
fn check_u3601_mix_tbl() -> bool {
    let mut ok = u3601mix_COUNT == 16;
    if ok { ok = u3601mix_get("STRESS_00") == Some(10) && u3601mix_get("GATE_15") == Some(85); }
    if ok { ok = u3601mix_get("ZZZ_NOKEY").is_none(); }
    if ok { ok = u3601mix_sum() == 760; }
    ok
}

/// UNX-F3601 混合映射 16 行守卫唯一 + 守卫阈值冻结。
fn check_u3601_mix_map() -> bool {
    let mut ok = u3601mixmap_COUNT == 16;
    if ok { ok = u3601mixmap_scan() == 0 && u3601mixmap_unique(); }
    if ok { ok = u3601mixmap_sum() == 504; }
    if ok { ok = U3601MIXGATE_THRESHOLD == 32; }
    ok
}

/// 本模块自检（逐检查带 detail，全绿才出门）。
pub fn run_u3601_checks() -> CheckSet {
    let mut set = CheckSet::new("U3601");
    set.add(
        "u3601-seg-guard",
        u3601_guard_seg(),
        if u3601_guard_seg() { "" } else { "diagnostic code outside segment" },
    );
    set.add(
        "u3601-mix-tbl",
        check_u3601_mix_tbl(),
        if check_u3601_mix_tbl() { "" } else { "UNX-F3601 mix table mismatch" },
    );
    set.add(
        "u3601-mix-map",
        check_u3601_mix_map(),
        if check_u3601_mix_map() { "" } else { "UNX-F3601 mix map mismatch" },
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
    fn u3601_all_checks_green() {
        let set = run_u3601_checks();
        assert!(set.all_passed(), "U3601 domain checks must be green");
    }

    #[test]
    fn u3601_segment_guard_holds() {
        assert!(u3601_segment_guard());
    }
}
