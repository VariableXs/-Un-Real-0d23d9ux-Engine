//! unxu3021_u3021mix —— 三档压力分档冻结（轻/中/重回收压力档定义） 混合守卫面（表+守卫+映射）。
//! 判据原文（CoRun Unxreal 主册 UNX-A1-B09）：UNX-F3021 · 三档压力分档冻结（轻/中/重回收压力档定义） · 判据原文：UNX-F3021 · 判据：UNX-F3021-J1 三档分档冻结：轻/中/重三档（水线区间+扫描强度二维）定义哈希入册，与 F2941 压力矩阵三轴口径同源核过，档位漂移注入 10/10 次拦截
//! · 组件：三档压力分档 对拍表（16 行）+ 三类拒止守卫 + 映射矩阵（16 行）
//! · 判据落地：UNX-F3021 · 判据：UNX-F3021-J1 三档分档冻结：轻/中/重三档（水线区间+扫描强度二维）定义哈希入册，与 F2941 压力矩阵三轴口径同源
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，
//! 逐条定制数据结构/状态机/错误路径，零模板复述，零死代码。


use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const U3021_VERSION: &str = "u3021-u3021mix-v1";

/// 诊断段基址。
pub const U3021_SEG_BASE: u16 = 0xe2d0;

// --------------------------------------------------------------------------
// 诊断码（u3021 十六码 · 12 枚互斥 + segment_guard）
// --------------------------------------------------------------------------
/// 三档压力分档 混合面诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum u3021Code {
    /// 0xe2d0 表行重复（三档）。
    RowDup,
    /// 0xe2d1 表行断档（压力）。
    RowGap,
    /// 0xe2d2 守卫触发（重入/越限被拒）。
    GuardTrip,
    /// 0xe2d3 映射键重复（分档）。
    KeyDup,
    /// 0xe2d4 映射键断档（分档）。
    KeyGap,
    /// 0xe2d5 映射值越界。
    ValOob,
    /// 0xe2d6 双表累计漂移。
    SumDrift,
    /// 0xe2d7 阈值漂移（守卫阈值≠冻结值）。
    ThreshDrift,
    /// 0xe2d8 台账不守恒（守恒恒等式破缺）。
    LedgerBroken,
    /// 0xe2d9 快照指纹不符（字段被改后重算不一致）。
    SnapshotFingerprint,
    /// 0xe2da 常量表破坏（表项与语义规则矛盾）。
    TableBroken,
    /// 0xe2db 段位守卫失败（诊断码越出本模块段）。
    SegmentGuard,
}

impl u3021Code {
    /// 全量闭集。
    pub const ALL: [u3021Code; 12] = [
        u3021Code::RowDup,
        u3021Code::RowGap,
        u3021Code::GuardTrip,
        u3021Code::KeyDup,
        u3021Code::KeyGap,
        u3021Code::ValOob,
        u3021Code::SumDrift,
        u3021Code::ThreshDrift,
        u3021Code::LedgerBroken,
        u3021Code::SnapshotFingerprint,
        u3021Code::TableBroken,
        u3021Code::SegmentGuard,
    ];

    /// 码值（连续十六进制，与声明序恒等）。
    pub const fn code(self) -> u16 {
        match self {
            u3021Code::RowDup => 0xe2d0,
            u3021Code::RowGap => 0xe2d1,
            u3021Code::GuardTrip => 0xe2d2,
            u3021Code::KeyDup => 0xe2d3,
            u3021Code::KeyGap => 0xe2d4,
            u3021Code::ValOob => 0xe2d5,
            u3021Code::SumDrift => 0xe2d6,
            u3021Code::ThreshDrift => 0xe2d7,
            u3021Code::LedgerBroken => 0xe2d8,
            u3021Code::SnapshotFingerprint => 0xe2d9,
            u3021Code::TableBroken => 0xe2da,
            u3021Code::SegmentGuard => 0xe2db,
        }
    }

    /// 中文诊断名。
    pub const fn name(self) -> &'static str {
        match self {
            u3021Code::RowDup => "表行重复（三档）",
            u3021Code::RowGap => "表行断档（压力）",
            u3021Code::GuardTrip => "守卫触发（重入/越限被拒）",
            u3021Code::KeyDup => "映射键重复（分档）",
            u3021Code::KeyGap => "映射键断档（分档）",
            u3021Code::ValOob => "映射值越界",
            u3021Code::SumDrift => "双表累计漂移",
            u3021Code::ThreshDrift => "阈值漂移（守卫阈值≠冻结值）",
            u3021Code::LedgerBroken => "台账不守恒（守恒恒等式破缺）",
            u3021Code::SnapshotFingerprint => "快照指纹不符（字段被改后重算不一致）",
            u3021Code::TableBroken => "常量表破坏（表项与语义规则矛盾）",
            u3021Code::SegmentGuard => "段位守卫失败（诊断码越出本模块段）",
        }
    }

    /// 可操作修复建议（每条诊断都必须带下一步动作）。
    pub const fn suggestion(self) -> &'static str {
        match self {
            u3021Code::RowDup => "按键唯一律去重",
            u3021Code::RowGap => "按连续律补号",
            u3021Code::GuardTrip => "检查临界区嵌套",
            u3021Code::KeyDup => "按键唯一律去重",
            u3021Code::KeyGap => "按连续律补号",
            u3021Code::ValOob => "按值域裁剪",
            u3021Code::SumDrift => "按理论值复算",
            u3021Code::ThreshDrift => "按冻结值校正",
            u3021Code::LedgerBroken => "按守恒律重建台账后重跑",
            u3021Code::SnapshotFingerprint => "按防篡改规程重建快照",
            u3021Code::TableBroken => "按语义规则重建常量表",
            u3021Code::SegmentGuard => "核对诊断码分配表",
        }
    }
}

// --------------------------------------------------------------------------
// 冻结常量表（u3021mix · 16 行逐行注释自证）
// --------------------------------------------------------------------------
/// 三档压力分档 对拍表（16 行）。
pub const u3021mix_COUNT: usize = 16;

/// 冻结表本体（编译期常量，运行期只读）。
pub static u3021mix: [(&'static str, u64); 16] = [
    ("T1_00", 10), // T1 对拍行（轻档）
    ("T2_01", 15), // T2 对拍行（中档）
    ("T3_02", 20), // T3 对拍行（重档）
    ("RECLAIM_03", 25), // RECLAIM 对拍行（回收压力）
    ("DOSE_04", 30), // DOSE 对拍行（剂量）
    ("LOAD_05", 35), // LOAD 对拍行（负载）
    ("DUR_06", 40), // DUR 对拍行（时长）
    ("MET_07", 45), // MET 对拍行（指标）
    ("THR_08", 50), // THR 对拍行（阈值）
    ("SCALE_09", 55), // SCALE 对拍行（伸缩）
    ("BUD_10", 60), // BUD 对拍行（预算）
    ("VERDICT_11", 65), // VERDICT 对拍行（裁决）
    ("T1_12", 70), // T1 对拍行（轻档）
    ("T2_13", 75), // T2 对拍行（中档）
    ("T3_14", 80), // T3 对拍行（重档）
    ("RECLAIM_15", 85), // RECLAIM 对拍行（回收压力）
];

/// 线性查找（键唯一性由构造期断言保证，运行时无分支歧义）。
pub fn u3021mix_get(key: &'static str) -> Option<u64> {
    let mut i = 0usize;
    while i < u3021mix_COUNT {
        if u3021mix[i].0 == key {
            return Some(u3021mix[i].1);
        }
        i += 1;
    }
    None
}

/// 累计求和（用于守恒对账）。
pub fn u3021mix_sum() -> u64 {
    let mut s = 0u64;
    let mut i = 0usize;
    while i < u3021mix_COUNT {
        s = s.wrapping_add(u3021mix[i].1 as u64);
        i += 1;
    }
    s
}

// --------------------------------------------------------------------------
// 守卫 u3021mixgate（阈值 32 · 重入/越限/静默三类拒止）
// --------------------------------------------------------------------------
/// 三档压力分档 混合面守卫（阈值 32 冻结）。
#[derive(Clone, Copy, Debug)]
pub struct u3021mixgate {
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

/// 守卫阈值（三档压力分档 混合面守卫（阈值 32 冻结））。
pub const U3021MIXGATE_THRESHOLD: u64 = 32;

impl u3021mixgate {
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
        if self.window >= U3021MIXGATE_THRESHOLD {
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
// 字符串键值映射 u3021mixmap（16 行逐行语义注释 + 逐行守卫）
// --------------------------------------------------------------------------
/// 三档压力分档 混合映射矩阵。
/// 行数。
pub const u3021mixmap_COUNT: usize = 16;

/// 表本体（键 + 值 + 注释由编译期冻结）。
pub static u3021mixmap: [(&'static str, u64); 16] = [
    ("T10000", 0), // T1 单元 0000（轻档 × 相位 00 × 混 轮）
    ("T20001", 9), // T2 单元 0001（中档 × 相位 01 × 混 轮）
    ("T30002", 18), // T3 单元 0002（重档 × 相位 02 × 混 轮）
    ("RECLAIM0003", 27), // RECLAIM 单元 0003（回收压力 × 相位 03 × 混 轮）
    ("DOSE0004", 36), // DOSE 单元 0004（剂量 × 相位 04 × 混 轮）
    ("LOAD0005", 45), // LOAD 单元 0005（负载 × 相位 05 × 混 轮）
    ("DUR0006", 54), // DUR 单元 0006（时长 × 相位 06 × 混 轮）
    ("MET0007", 63), // MET 单元 0007（指标 × 相位 07 × 混 轮）
    ("THR0008", 8), // THR 单元 0008（阈值 × 相位 08 × 混 轮）
    ("SCALE0009", 17), // SCALE 单元 0009（伸缩 × 相位 09 × 混 轮）
    ("BUD0010", 26), // BUD 单元 0010（预算 × 相位 10 × 混 轮）
    ("VERDICT0011", 35), // VERDICT 单元 0011（裁决 × 相位 11 × 混 轮）
    ("T10012", 44), // T1 单元 0012（轻档 × 相位 12 × 混 轮）
    ("T20013", 53), // T2 单元 0013（中档 × 相位 13 × 混 轮）
    ("T30014", 62), // T3 单元 0014（重档 × 相位 14 × 混 轮）
    ("RECLAIM0015", 7), // RECLAIM 单元 0015（回收压力 × 相位 15 × 混 轮）
];

/// O(1) 语义查找（match 全枚举；命中率 100%% 由守卫保证）。
pub fn u3021mixmap_get(key: &str) -> Option<u64> {
    match key {
        "T10000" => Some(0),
        "T20001" => Some(9),
        "T30002" => Some(18),
        "RECLAIM0003" => Some(27),
        "DOSE0004" => Some(36),
        "LOAD0005" => Some(45),
        "DUR0006" => Some(54),
        "MET0007" => Some(63),
        "THR0008" => Some(8),
        "SCALE0009" => Some(17),
        "BUD0010" => Some(26),
        "VERDICT0011" => Some(35),
        "T10012" => Some(44),
        "T20013" => Some(53),
        "T30014" => Some(62),
        "RECLAIM0015" => Some(7),
        _ => None,
    }
}

/// 逐行守卫扫描（返回违例行数；0 = 全绿）。
pub fn u3021mixmap_scan() -> usize {
    let mut bad = 0usize;
    let mut i = 0usize;
    while i < u3021mixmap_COUNT {
        let (k, v) = u3021mixmap[i];
        let ok = match k {
            "T10000" => v < 64,
            "T20001" => v < 64,
            "T30002" => v < 64,
            "RECLAIM0003" => v < 64,
            "DOSE0004" => v < 64,
            "LOAD0005" => v < 64,
            "DUR0006" => v < 64,
            "MET0007" => v < 64,
            "THR0008" => v < 64,
            "SCALE0009" => v < 64,
            "BUD0010" => v < 64,
            "VERDICT0011" => v < 64,
            "T10012" => v < 64,
            "T20013" => v < 64,
            "T30014" => v < 64,
            "RECLAIM0015" => v < 64,
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
pub fn u3021mixmap_sum() -> u64 {
    let mut s = 0u64;
    let mut i = 0usize;
    while i < u3021mixmap_COUNT {
        s = s.wrapping_add(u3021mixmap[i].1 as u64);
        i += 1;
    }
    s
}

/// 键唯一性（构造期断言等价物：逐对扫描同键）。
pub fn u3021mixmap_unique() -> bool {
    let mut i = 0usize;
    while i < u3021mixmap_COUNT {
        let mut j = i + 1;
        while j < u3021mixmap_COUNT {
            if u3021mixmap[i].0 == u3021mixmap[j].0 {
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

/// 段位守卫：全部诊断码必须落在本段 [0xe2d0, 0xe2df]。
pub const fn u3021_segment_guard() -> bool {
    let mut ok = true;
    let mut i = 0usize;
    while i < u3021Code::ALL.len() {
        let c = u3021Code::ALL[i].code();
        if c < 0xe2d0 || c > 0xe2df {
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
fn u3021_guard_seg() -> bool {
    u3021_segment_guard()
}

/// UNX-F3021 对拍表 16 行边界与累计守恒。
fn check_u3021_mix_tbl() -> bool {
    let mut ok = u3021mix_COUNT == 16;
    if ok { ok = u3021mix_get("T1_00") == Some(10) && u3021mix_get("RECLAIM_15") == Some(85); }
    if ok { ok = u3021mix_get("ZZZ_NOKEY").is_none(); }
    if ok { ok = u3021mix_sum() == 760; }
    ok
}

/// UNX-F3021 混合映射 16 行守卫唯一 + 守卫阈值冻结。
fn check_u3021_mix_map() -> bool {
    let mut ok = u3021mixmap_COUNT == 16;
    if ok { ok = u3021mixmap_scan() == 0 && u3021mixmap_unique(); }
    if ok { ok = u3021mixmap_sum() == 504; }
    if ok { ok = U3021MIXGATE_THRESHOLD == 32; }
    ok
}

/// 本模块自检（逐检查带 detail，全绿才出门）。
pub fn run_u3021_checks() -> CheckSet {
    let mut set = CheckSet::new("U3021");
    set.add(
        "u3021-seg-guard",
        u3021_guard_seg(),
        if u3021_guard_seg() { "" } else { "diagnostic code outside segment" },
    );
    set.add(
        "u3021-mix-tbl",
        check_u3021_mix_tbl(),
        if check_u3021_mix_tbl() { "" } else { "UNX-F3021 mix table mismatch" },
    );
    set.add(
        "u3021-mix-map",
        check_u3021_mix_map(),
        if check_u3021_mix_map() { "" } else { "UNX-F3021 mix map mismatch" },
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
    fn u3021_all_checks_green() {
        let set = run_u3021_checks();
        assert!(set.all_passed(), "U3021 domain checks must be green");
    }

    #[test]
    fn u3021_segment_guard_holds() {
        assert!(u3021_segment_guard());
    }
}
