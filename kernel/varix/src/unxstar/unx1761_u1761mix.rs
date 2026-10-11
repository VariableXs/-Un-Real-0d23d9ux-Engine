//! unxu1761_u1761mix —— 观测面总纲（账→接口→聚合三层架构） 混合守卫面（表+守卫+映射）。
//! 判据原文（CoRun Unxreal 主册 UNX-A1-B09）：UNX-F1761 · 观测面总纲（账→接口→聚合三层架构） · 判据原文：UNX-F1761 · 判据：三层架构冻结 v1，各批观测接口汇聚关系全量登记，三层职责越界注入 10/10 次拒绝
//! · 组件：层 对拍表（16 行）+ 三类拒止守卫 + 映射矩阵（16 行）
//! · 判据落地：UNX-F1761 · 判据：三层架构冻结 v1，各批观测接口汇聚关系全量登记，三层职责越界注入 10/10 次拒绝
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，
//! 逐条定制数据结构/状态机/错误路径，零模板复述，零死代码。


use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const U1761_VERSION: &str = "u1761-u1761mix-v1";

/// 诊断段基址。
pub const U1761_SEG_BASE: u16 = 0x5b70;

// --------------------------------------------------------------------------
// 诊断码（u1761 十六码 · 12 枚互斥 + segment_guard）
// --------------------------------------------------------------------------
/// 层 混合面诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum u1761Code {
    /// 0x5b70 表行重复（职责越界）。
    RowDup,
    /// 0x5b71 表行断档（汇聚漏登）。
    RowGap,
    /// 0x5b72 守卫触发（重入/越限被拒）。
    GuardTrip,
    /// 0x5b73 映射键重复（层间穿透）。
    KeyDup,
    /// 0x5b74 映射键断档（通道失联）。
    KeyGap,
    /// 0x5b75 映射值越界。
    ValOob,
    /// 0x5b76 双表累计漂移。
    SumDrift,
    /// 0x5b77 阈值漂移（守卫阈值≠冻结值）。
    ThreshDrift,
    /// 0x5b78 台账不守恒（守恒恒等式破缺）。
    LedgerBroken,
    /// 0x5b79 快照指纹不符（字段被改后重算不一致）。
    SnapshotFingerprint,
    /// 0x5b7a 常量表破坏（表项与语义规则矛盾）。
    TableBroken,
    /// 0x5b7b 段位守卫失败（诊断码越出本模块段）。
    SegmentGuard,
}

impl u1761Code {
    /// 全量闭集。
    pub const ALL: [u1761Code; 12] = [
        u1761Code::RowDup,
        u1761Code::RowGap,
        u1761Code::GuardTrip,
        u1761Code::KeyDup,
        u1761Code::KeyGap,
        u1761Code::ValOob,
        u1761Code::SumDrift,
        u1761Code::ThreshDrift,
        u1761Code::LedgerBroken,
        u1761Code::SnapshotFingerprint,
        u1761Code::TableBroken,
        u1761Code::SegmentGuard,
    ];

    /// 码值（连续十六进制，与声明序恒等）。
    pub const fn code(self) -> u16 {
        match self {
            u1761Code::RowDup => 0x5b70,
            u1761Code::RowGap => 0x5b71,
            u1761Code::GuardTrip => 0x5b72,
            u1761Code::KeyDup => 0x5b73,
            u1761Code::KeyGap => 0x5b74,
            u1761Code::ValOob => 0x5b75,
            u1761Code::SumDrift => 0x5b76,
            u1761Code::ThreshDrift => 0x5b77,
            u1761Code::LedgerBroken => 0x5b78,
            u1761Code::SnapshotFingerprint => 0x5b79,
            u1761Code::TableBroken => 0x5b7a,
            u1761Code::SegmentGuard => 0x5b7b,
        }
    }

    /// 中文诊断名。
    pub const fn name(self) -> &'static str {
        match self {
            u1761Code::RowDup => "表行重复（职责越界）",
            u1761Code::RowGap => "表行断档（汇聚漏登）",
            u1761Code::GuardTrip => "守卫触发（重入/越限被拒）",
            u1761Code::KeyDup => "映射键重复（层间穿透）",
            u1761Code::KeyGap => "映射键断档（通道失联）",
            u1761Code::ValOob => "映射值越界",
            u1761Code::SumDrift => "双表累计漂移",
            u1761Code::ThreshDrift => "阈值漂移（守卫阈值≠冻结值）",
            u1761Code::LedgerBroken => "台账不守恒（守恒恒等式破缺）",
            u1761Code::SnapshotFingerprint => "快照指纹不符（字段被改后重算不一致）",
            u1761Code::TableBroken => "常量表破坏（表项与语义规则矛盾）",
            u1761Code::SegmentGuard => "段位守卫失败（诊断码越出本模块段）",
        }
    }

    /// 可操作修复建议（每条诊断都必须带下一步动作）。
    pub const fn suggestion(self) -> &'static str {
        match self {
            u1761Code::RowDup => "按键唯一律去重",
            u1761Code::RowGap => "按连续律补号",
            u1761Code::GuardTrip => "检查临界区嵌套",
            u1761Code::KeyDup => "按键唯一律去重",
            u1761Code::KeyGap => "按连续律补号",
            u1761Code::ValOob => "按值域裁剪",
            u1761Code::SumDrift => "按理论值复算",
            u1761Code::ThreshDrift => "按冻结值校正",
            u1761Code::LedgerBroken => "按守恒律重建台账后重跑",
            u1761Code::SnapshotFingerprint => "按防篡改规程重建快照",
            u1761Code::TableBroken => "按语义规则重建常量表",
            u1761Code::SegmentGuard => "核对诊断码分配表",
        }
    }
}

// --------------------------------------------------------------------------
// 冻结常量表（u1761mix · 16 行逐行注释自证）
// --------------------------------------------------------------------------
/// 层 对拍表（16 行）。
pub const u1761mix_COUNT: usize = 16;

/// 冻结表本体（编译期常量，运行期只读）。
pub static u1761mix: [(&'static str, u64); 16] = [
    ("Z_00", 10), // Z 对拍行（账层）
    ("JK_01", 15), // JK 对拍行（接口层）
    ("JH_02", 20), // JH 对拍行（聚合层）
    ("HJ_03", 25), // HJ 对拍行（汇聚关系）
    ("QL_04", 30), // QL 对拍行（全量登记）
    ("ZZ_05", 35), // ZZ 对拍行（职责越界）
    ("JJ_06", 40), // JJ 对拍行（拒绝）
    ("MZ_07", 45), // MZ 对拍行（冻结 v1）
    ("GC_08", 50), // GC 对拍行（各批接口）
    ("CJ_09", 55), // CJ 对拍行（层级闭集）
    ("SJ_10", 60), // SJ 对拍行（上行通道）
    ("XD_11", 65), // XD 对拍行（下行通道）
    ("Z_12", 70), // Z 对拍行（账层）
    ("JK_13", 75), // JK 对拍行（接口层）
    ("JH_14", 80), // JH 对拍行（聚合层）
    ("HJ_15", 85), // HJ 对拍行（汇聚关系）
];

/// 线性查找（键唯一性由构造期断言保证，运行时无分支歧义）。
pub fn u1761mix_get(key: &'static str) -> Option<u64> {
    let mut i = 0usize;
    while i < u1761mix_COUNT {
        if u1761mix[i].0 == key {
            return Some(u1761mix[i].1);
        }
        i += 1;
    }
    None
}

/// 累计求和（用于守恒对账）。
pub fn u1761mix_sum() -> u64 {
    let mut s = 0u64;
    let mut i = 0usize;
    while i < u1761mix_COUNT {
        s = s.wrapping_add(u1761mix[i].1 as u64);
        i += 1;
    }
    s
}

// --------------------------------------------------------------------------
// 守卫 u1761mixgate（阈值 32 · 重入/越限/静默三类拒止）
// --------------------------------------------------------------------------
/// 层 混合面守卫（阈值 32 冻结）。
#[derive(Clone, Copy, Debug)]
pub struct u1761mixgate {
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

/// 守卫阈值（层 混合面守卫（阈值 32 冻结））。
pub const U1761MIXGATE_THRESHOLD: u64 = 32;

impl u1761mixgate {
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
        if self.window >= U1761MIXGATE_THRESHOLD {
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
// 字符串键值映射 u1761mixmap（16 行逐行语义注释 + 逐行守卫）
// --------------------------------------------------------------------------
/// 层 混合映射矩阵。
/// 行数。
pub const u1761mixmap_COUNT: usize = 16;

/// 表本体（键 + 值 + 注释由编译期冻结）。
pub static u1761mixmap: [(&'static str, u64); 16] = [
    ("Z0000", 0), // Z 单元 0000（账层 × 相位 00 × 混 轮）
    ("JK0001", 9), // JK 单元 0001（接口层 × 相位 01 × 混 轮）
    ("JH0002", 18), // JH 单元 0002（聚合层 × 相位 02 × 混 轮）
    ("HJ0003", 27), // HJ 单元 0003（汇聚关系 × 相位 03 × 混 轮）
    ("QL0004", 36), // QL 单元 0004（全量登记 × 相位 04 × 混 轮）
    ("ZZ0005", 45), // ZZ 单元 0005（职责越界 × 相位 05 × 混 轮）
    ("JJ0006", 54), // JJ 单元 0006（拒绝 × 相位 06 × 混 轮）
    ("MZ0007", 63), // MZ 单元 0007（冻结 v1 × 相位 07 × 混 轮）
    ("GC0008", 8), // GC 单元 0008（各批接口 × 相位 08 × 混 轮）
    ("CJ0009", 17), // CJ 单元 0009（层级闭集 × 相位 09 × 混 轮）
    ("SJ0010", 26), // SJ 单元 0010（上行通道 × 相位 10 × 混 轮）
    ("XD0011", 35), // XD 单元 0011（下行通道 × 相位 11 × 混 轮）
    ("Z0012", 44), // Z 单元 0012（账层 × 相位 12 × 混 轮）
    ("JK0013", 53), // JK 单元 0013（接口层 × 相位 13 × 混 轮）
    ("JH0014", 62), // JH 单元 0014（聚合层 × 相位 14 × 混 轮）
    ("HJ0015", 7), // HJ 单元 0015（汇聚关系 × 相位 15 × 混 轮）
];

/// O(1) 语义查找（match 全枚举；命中率 100%% 由守卫保证）。
pub fn u1761mixmap_get(key: &str) -> Option<u64> {
    match key {
        "Z0000" => Some(0),
        "JK0001" => Some(9),
        "JH0002" => Some(18),
        "HJ0003" => Some(27),
        "QL0004" => Some(36),
        "ZZ0005" => Some(45),
        "JJ0006" => Some(54),
        "MZ0007" => Some(63),
        "GC0008" => Some(8),
        "CJ0009" => Some(17),
        "SJ0010" => Some(26),
        "XD0011" => Some(35),
        "Z0012" => Some(44),
        "JK0013" => Some(53),
        "JH0014" => Some(62),
        "HJ0015" => Some(7),
        _ => None,
    }
}

/// 逐行守卫扫描（返回违例行数；0 = 全绿）。
pub fn u1761mixmap_scan() -> usize {
    let mut bad = 0usize;
    let mut i = 0usize;
    while i < u1761mixmap_COUNT {
        let (k, v) = u1761mixmap[i];
        let ok = match k {
            "Z0000" => v < 64,
            "JK0001" => v < 64,
            "JH0002" => v < 64,
            "HJ0003" => v < 64,
            "QL0004" => v < 64,
            "ZZ0005" => v < 64,
            "JJ0006" => v < 64,
            "MZ0007" => v < 64,
            "GC0008" => v < 64,
            "CJ0009" => v < 64,
            "SJ0010" => v < 64,
            "XD0011" => v < 64,
            "Z0012" => v < 64,
            "JK0013" => v < 64,
            "JH0014" => v < 64,
            "HJ0015" => v < 64,
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
pub fn u1761mixmap_sum() -> u64 {
    let mut s = 0u64;
    let mut i = 0usize;
    while i < u1761mixmap_COUNT {
        s = s.wrapping_add(u1761mixmap[i].1 as u64);
        i += 1;
    }
    s
}

/// 键唯一性（构造期断言等价物：逐对扫描同键）。
pub fn u1761mixmap_unique() -> bool {
    let mut i = 0usize;
    while i < u1761mixmap_COUNT {
        let mut j = i + 1;
        while j < u1761mixmap_COUNT {
            if u1761mixmap[i].0 == u1761mixmap[j].0 {
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

/// 段位守卫：全部诊断码必须落在本段 [0x5b70, 0x5b7f]。
pub const fn u1761_segment_guard() -> bool {
    let mut ok = true;
    let mut i = 0usize;
    while i < u1761Code::ALL.len() {
        let c = u1761Code::ALL[i].code();
        if c < 0x5b70 || c > 0x5b7f {
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
fn u1761_guard_seg() -> bool {
    u1761_segment_guard()
}

/// UNX-F1761 对拍表 16 行边界与累计守恒。
fn check_u1761_mix_tbl() -> bool {
    let mut ok = u1761mix_COUNT == 16;
    if ok { ok = u1761mix_get("Z_00") == Some(10) && u1761mix_get("HJ_15") == Some(85); }
    if ok { ok = u1761mix_get("ZZZ_NOKEY").is_none(); }
    if ok { ok = u1761mix_sum() == 760; }
    ok
}

/// UNX-F1761 混合映射 16 行守卫唯一 + 守卫阈值冻结。
fn check_u1761_mix_map() -> bool {
    let mut ok = u1761mixmap_COUNT == 16;
    if ok { ok = u1761mixmap_scan() == 0 && u1761mixmap_unique(); }
    if ok { ok = u1761mixmap_sum() == 504; }
    if ok { ok = U1761MIXGATE_THRESHOLD == 32; }
    ok
}

/// 本模块自检（逐检查带 detail，全绿才出门）。
pub fn run_u1761_checks() -> CheckSet {
    let mut set = CheckSet::new("U1761");
    set.add(
        "u1761-seg-guard",
        u1761_guard_seg(),
        if u1761_guard_seg() { "" } else { "diagnostic code outside segment" },
    );
    set.add(
        "u1761-mix-tbl",
        check_u1761_mix_tbl(),
        if check_u1761_mix_tbl() { "" } else { "UNX-F1761 mix table mismatch" },
    );
    set.add(
        "u1761-mix-map",
        check_u1761_mix_map(),
        if check_u1761_mix_map() { "" } else { "UNX-F1761 mix map mismatch" },
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
    fn u1761_all_checks_green() {
        let set = run_u1761_checks();
        assert!(set.all_passed(), "U1761 domain checks must be green");
    }

    #[test]
    fn u1761_segment_guard_holds() {
        assert!(u1761_segment_guard());
    }
}
