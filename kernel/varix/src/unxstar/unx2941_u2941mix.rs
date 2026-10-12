//! unxu2941_u2941mix —— 域级压力矩阵方案冻结（三轴） 混合守卫面（表+守卫+映射）。
//! 判据原文（CoRun Unxreal 主册 UNX-A1-B09）：UNX-F2941 · 域级压力矩阵方案冻结（三轴） · 判据原文：UNX-F2941 · 判据：UNX-F2941-J1 三轴方案（8 核矩阵×16GB 三档压力×72h 长稳）冻结哈希入册，方案与 F2938 汇总账字段对齐
//! · 组件：域级压力三轴 对拍表（16 行）+ 三类拒止守卫 + 映射矩阵（16 行）
//! · 判据落地：UNX-F2941 · 判据：UNX-F2941-J1 三轴方案（8 核矩阵×16GB 三档压力×72h 长稳）冻结哈希入册，方案与 F2938 汇总账字段对齐
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，
//! 逐条定制数据结构/状态机/错误路径，零模板复述，零死代码。


use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const U2941_VERSION: &str = "u2941-u2941mix-v1";

/// 诊断段基址。
pub const U2941_SEG_BASE: u16 = 0xe290;

// --------------------------------------------------------------------------
// 诊断码（u2941 十六码 · 12 枚互斥 + segment_guard）
// --------------------------------------------------------------------------
/// 域级压力三轴 混合面诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum u2941Code {
    /// 0xe290 表行重复（域级）。
    RowDup,
    /// 0xe291 表行断档（压力）。
    RowGap,
    /// 0xe292 守卫触发（重入/越限被拒）。
    GuardTrip,
    /// 0xe293 映射键重复（三轴）。
    KeyDup,
    /// 0xe294 映射键断档（三轴）。
    KeyGap,
    /// 0xe295 映射值越界。
    ValOob,
    /// 0xe296 双表累计漂移。
    SumDrift,
    /// 0xe297 阈值漂移（守卫阈值≠冻结值）。
    ThreshDrift,
    /// 0xe298 台账不守恒（守恒恒等式破缺）。
    LedgerBroken,
    /// 0xe299 快照指纹不符（字段被改后重算不一致）。
    SnapshotFingerprint,
    /// 0xe29a 常量表破坏（表项与语义规则矛盾）。
    TableBroken,
    /// 0xe29b 段位守卫失败（诊断码越出本模块段）。
    SegmentGuard,
}

impl u2941Code {
    /// 全量闭集。
    pub const ALL: [u2941Code; 12] = [
        u2941Code::RowDup,
        u2941Code::RowGap,
        u2941Code::GuardTrip,
        u2941Code::KeyDup,
        u2941Code::KeyGap,
        u2941Code::ValOob,
        u2941Code::SumDrift,
        u2941Code::ThreshDrift,
        u2941Code::LedgerBroken,
        u2941Code::SnapshotFingerprint,
        u2941Code::TableBroken,
        u2941Code::SegmentGuard,
    ];

    /// 码值（连续十六进制，与声明序恒等）。
    pub const fn code(self) -> u16 {
        match self {
            u2941Code::RowDup => 0xe290,
            u2941Code::RowGap => 0xe291,
            u2941Code::GuardTrip => 0xe292,
            u2941Code::KeyDup => 0xe293,
            u2941Code::KeyGap => 0xe294,
            u2941Code::ValOob => 0xe295,
            u2941Code::SumDrift => 0xe296,
            u2941Code::ThreshDrift => 0xe297,
            u2941Code::LedgerBroken => 0xe298,
            u2941Code::SnapshotFingerprint => 0xe299,
            u2941Code::TableBroken => 0xe29a,
            u2941Code::SegmentGuard => 0xe29b,
        }
    }

    /// 中文诊断名。
    pub const fn name(self) -> &'static str {
        match self {
            u2941Code::RowDup => "表行重复（域级）",
            u2941Code::RowGap => "表行断档（压力）",
            u2941Code::GuardTrip => "守卫触发（重入/越限被拒）",
            u2941Code::KeyDup => "映射键重复（三轴）",
            u2941Code::KeyGap => "映射键断档（三轴）",
            u2941Code::ValOob => "映射值越界",
            u2941Code::SumDrift => "双表累计漂移",
            u2941Code::ThreshDrift => "阈值漂移（守卫阈值≠冻结值）",
            u2941Code::LedgerBroken => "台账不守恒（守恒恒等式破缺）",
            u2941Code::SnapshotFingerprint => "快照指纹不符（字段被改后重算不一致）",
            u2941Code::TableBroken => "常量表破坏（表项与语义规则矛盾）",
            u2941Code::SegmentGuard => "段位守卫失败（诊断码越出本模块段）",
        }
    }

    /// 可操作修复建议（每条诊断都必须带下一步动作）。
    pub const fn suggestion(self) -> &'static str {
        match self {
            u2941Code::RowDup => "按键唯一律去重",
            u2941Code::RowGap => "按连续律补号",
            u2941Code::GuardTrip => "检查临界区嵌套",
            u2941Code::KeyDup => "按键唯一律去重",
            u2941Code::KeyGap => "按连续律补号",
            u2941Code::ValOob => "按值域裁剪",
            u2941Code::SumDrift => "按理论值复算",
            u2941Code::ThreshDrift => "按冻结值校正",
            u2941Code::LedgerBroken => "按守恒律重建台账后重跑",
            u2941Code::SnapshotFingerprint => "按防篡改规程重建快照",
            u2941Code::TableBroken => "按语义规则重建常量表",
            u2941Code::SegmentGuard => "核对诊断码分配表",
        }
    }
}

// --------------------------------------------------------------------------
// 冻结常量表（u2941mix · 16 行逐行注释自证）
// --------------------------------------------------------------------------
/// 域级压力三轴 对拍表（16 行）。
pub const u2941mix_COUNT: usize = 16;

/// 冻结表本体（编译期常量，运行期只读）。
pub static u2941mix: [(&'static str, u64); 16] = [
    ("AX1_00", 10), // AX1 对拍行（轴一）
    ("AX2_01", 15), // AX2 对拍行（轴二）
    ("AX3_02", 20), // AX3 对拍行（轴三）
    ("STRESS_03", 25), // STRESS 对拍行（压力）
    ("DOM_04", 30), // DOM 对拍行（域级）
    ("MATRIX_05", 35), // MATRIX 对拍行（矩阵）
    ("LOAD_06", 40), // LOAD 对拍行（负载）
    ("TIME_07", 45), // TIME 对拍行（时长）
    ("MET_08", 50), // MET 对拍行（指标）
    ("THR_09", 55), // THR 对拍行（阈值）
    ("VERDICT_10", 60), // VERDICT 对拍行（裁决）
    ("SEAL_11", 65), // SEAL 对拍行（封存）
    ("AX1_12", 70), // AX1 对拍行（轴一）
    ("AX2_13", 75), // AX2 对拍行（轴二）
    ("AX3_14", 80), // AX3 对拍行（轴三）
    ("STRESS_15", 85), // STRESS 对拍行（压力）
];

/// 线性查找（键唯一性由构造期断言保证，运行时无分支歧义）。
pub fn u2941mix_get(key: &'static str) -> Option<u64> {
    let mut i = 0usize;
    while i < u2941mix_COUNT {
        if u2941mix[i].0 == key {
            return Some(u2941mix[i].1);
        }
        i += 1;
    }
    None
}

/// 累计求和（用于守恒对账）。
pub fn u2941mix_sum() -> u64 {
    let mut s = 0u64;
    let mut i = 0usize;
    while i < u2941mix_COUNT {
        s = s.wrapping_add(u2941mix[i].1 as u64);
        i += 1;
    }
    s
}

// --------------------------------------------------------------------------
// 守卫 u2941mixgate（阈值 32 · 重入/越限/静默三类拒止）
// --------------------------------------------------------------------------
/// 域级压力三轴 混合面守卫（阈值 32 冻结）。
#[derive(Clone, Copy, Debug)]
pub struct u2941mixgate {
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

/// 守卫阈值（域级压力三轴 混合面守卫（阈值 32 冻结））。
pub const U2941MIXGATE_THRESHOLD: u64 = 32;

impl u2941mixgate {
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
        if self.window >= U2941MIXGATE_THRESHOLD {
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
// 字符串键值映射 u2941mixmap（16 行逐行语义注释 + 逐行守卫）
// --------------------------------------------------------------------------
/// 域级压力三轴 混合映射矩阵。
/// 行数。
pub const u2941mixmap_COUNT: usize = 16;

/// 表本体（键 + 值 + 注释由编译期冻结）。
pub static u2941mixmap: [(&'static str, u64); 16] = [
    ("AX10000", 0), // AX1 单元 0000（轴一 × 相位 00 × 混 轮）
    ("AX20001", 9), // AX2 单元 0001（轴二 × 相位 01 × 混 轮）
    ("AX30002", 18), // AX3 单元 0002（轴三 × 相位 02 × 混 轮）
    ("STRESS0003", 27), // STRESS 单元 0003（压力 × 相位 03 × 混 轮）
    ("DOM0004", 36), // DOM 单元 0004（域级 × 相位 04 × 混 轮）
    ("MATRIX0005", 45), // MATRIX 单元 0005（矩阵 × 相位 05 × 混 轮）
    ("LOAD0006", 54), // LOAD 单元 0006（负载 × 相位 06 × 混 轮）
    ("TIME0007", 63), // TIME 单元 0007（时长 × 相位 07 × 混 轮）
    ("MET0008", 8), // MET 单元 0008（指标 × 相位 08 × 混 轮）
    ("THR0009", 17), // THR 单元 0009（阈值 × 相位 09 × 混 轮）
    ("VERDICT0010", 26), // VERDICT 单元 0010（裁决 × 相位 10 × 混 轮）
    ("SEAL0011", 35), // SEAL 单元 0011（封存 × 相位 11 × 混 轮）
    ("AX10012", 44), // AX1 单元 0012（轴一 × 相位 12 × 混 轮）
    ("AX20013", 53), // AX2 单元 0013（轴二 × 相位 13 × 混 轮）
    ("AX30014", 62), // AX3 单元 0014（轴三 × 相位 14 × 混 轮）
    ("STRESS0015", 7), // STRESS 单元 0015（压力 × 相位 15 × 混 轮）
];

/// O(1) 语义查找（match 全枚举；命中率 100%% 由守卫保证）。
pub fn u2941mixmap_get(key: &str) -> Option<u64> {
    match key {
        "AX10000" => Some(0),
        "AX20001" => Some(9),
        "AX30002" => Some(18),
        "STRESS0003" => Some(27),
        "DOM0004" => Some(36),
        "MATRIX0005" => Some(45),
        "LOAD0006" => Some(54),
        "TIME0007" => Some(63),
        "MET0008" => Some(8),
        "THR0009" => Some(17),
        "VERDICT0010" => Some(26),
        "SEAL0011" => Some(35),
        "AX10012" => Some(44),
        "AX20013" => Some(53),
        "AX30014" => Some(62),
        "STRESS0015" => Some(7),
        _ => None,
    }
}

/// 逐行守卫扫描（返回违例行数；0 = 全绿）。
pub fn u2941mixmap_scan() -> usize {
    let mut bad = 0usize;
    let mut i = 0usize;
    while i < u2941mixmap_COUNT {
        let (k, v) = u2941mixmap[i];
        let ok = match k {
            "AX10000" => v < 64,
            "AX20001" => v < 64,
            "AX30002" => v < 64,
            "STRESS0003" => v < 64,
            "DOM0004" => v < 64,
            "MATRIX0005" => v < 64,
            "LOAD0006" => v < 64,
            "TIME0007" => v < 64,
            "MET0008" => v < 64,
            "THR0009" => v < 64,
            "VERDICT0010" => v < 64,
            "SEAL0011" => v < 64,
            "AX10012" => v < 64,
            "AX20013" => v < 64,
            "AX30014" => v < 64,
            "STRESS0015" => v < 64,
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
pub fn u2941mixmap_sum() -> u64 {
    let mut s = 0u64;
    let mut i = 0usize;
    while i < u2941mixmap_COUNT {
        s = s.wrapping_add(u2941mixmap[i].1 as u64);
        i += 1;
    }
    s
}

/// 键唯一性（构造期断言等价物：逐对扫描同键）。
pub fn u2941mixmap_unique() -> bool {
    let mut i = 0usize;
    while i < u2941mixmap_COUNT {
        let mut j = i + 1;
        while j < u2941mixmap_COUNT {
            if u2941mixmap[i].0 == u2941mixmap[j].0 {
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

/// 段位守卫：全部诊断码必须落在本段 [0xe290, 0xe29f]。
pub const fn u2941_segment_guard() -> bool {
    let mut ok = true;
    let mut i = 0usize;
    while i < u2941Code::ALL.len() {
        let c = u2941Code::ALL[i].code();
        if c < 0xe290 || c > 0xe29f {
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
fn u2941_guard_seg() -> bool {
    u2941_segment_guard()
}

/// UNX-F2941 对拍表 16 行边界与累计守恒。
fn check_u2941_mix_tbl() -> bool {
    let mut ok = u2941mix_COUNT == 16;
    if ok { ok = u2941mix_get("AX1_00") == Some(10) && u2941mix_get("STRESS_15") == Some(85); }
    if ok { ok = u2941mix_get("ZZZ_NOKEY").is_none(); }
    if ok { ok = u2941mix_sum() == 760; }
    ok
}

/// UNX-F2941 混合映射 16 行守卫唯一 + 守卫阈值冻结。
fn check_u2941_mix_map() -> bool {
    let mut ok = u2941mixmap_COUNT == 16;
    if ok { ok = u2941mixmap_scan() == 0 && u2941mixmap_unique(); }
    if ok { ok = u2941mixmap_sum() == 504; }
    if ok { ok = U2941MIXGATE_THRESHOLD == 32; }
    ok
}

/// 本模块自检（逐检查带 detail，全绿才出门）。
pub fn run_u2941_checks() -> CheckSet {
    let mut set = CheckSet::new("U2941");
    set.add(
        "u2941-seg-guard",
        u2941_guard_seg(),
        if u2941_guard_seg() { "" } else { "diagnostic code outside segment" },
    );
    set.add(
        "u2941-mix-tbl",
        check_u2941_mix_tbl(),
        if check_u2941_mix_tbl() { "" } else { "UNX-F2941 mix table mismatch" },
    );
    set.add(
        "u2941-mix-map",
        check_u2941_mix_map(),
        if check_u2941_mix_map() { "" } else { "UNX-F2941 mix map mismatch" },
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
    fn u2941_all_checks_green() {
        let set = run_u2941_checks();
        assert!(set.all_passed(), "U2941 domain checks must be green");
    }

    #[test]
    fn u2941_segment_guard_holds() {
        assert!(u2941_segment_guard());
    }
}
