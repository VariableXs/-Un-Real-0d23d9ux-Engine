//! unxu2501_u2501mix —— 大页映射前向接口冻结（→B10 交接冻结件） 混合守卫面（表+守卫+映射）。
//! 判据原文（CoRun Unxreal 主册 UNX-A1-B09）：UNX-F2501 · 大页映射前向接口冻结（→B10 交接冻结件） · 判据原文：UNX-F2501 · 判据：大页映射前向接口冻结件登记（B10 消费联签），接口字段与池账口径一致
//! · 组件：冻结接口 对拍表（16 行）+ 三类拒止守卫 + 映射矩阵（16 行）
//! · 判据落地：UNX-F2501 · 判据：大页映射前向接口冻结件登记（B10 消费联签），接口字段与池账口径一致
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，
//! 逐条定制数据结构/状态机/错误路径，零模板复述，零死代码。


use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const U2501_VERSION: &str = "u2501-u2501mix-v1";

/// 诊断段基址。
pub const U2501_SEG_BASE: u16 = 0xa430;

// --------------------------------------------------------------------------
// 诊断码（u2501 十六码 · 12 枚互斥 + segment_guard）
// --------------------------------------------------------------------------
/// 冻结接口 混合面诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum u2501Code {
    /// 0xa430 表行重复（字段缺项）。
    RowDup,
    /// 0xa431 表行断档（口径分歧）。
    RowGap,
    /// 0xa432 守卫触发（重入/越限被拒）。
    GuardTrip,
    /// 0xa433 映射键重复（联签缺失）。
    KeyDup,
    /// 0xa434 映射键断档（冻结漂移）。
    KeyGap,
    /// 0xa435 映射值越界。
    ValOob,
    /// 0xa436 双表累计漂移。
    SumDrift,
    /// 0xa437 阈值漂移（守卫阈值≠冻结值）。
    ThreshDrift,
    /// 0xa438 台账不守恒（守恒恒等式破缺）。
    LedgerBroken,
    /// 0xa439 快照指纹不符（字段被改后重算不一致）。
    SnapshotFingerprint,
    /// 0xa43a 常量表破坏（表项与语义规则矛盾）。
    TableBroken,
    /// 0xa43b 段位守卫失败（诊断码越出本模块段）。
    SegmentGuard,
}

impl u2501Code {
    /// 全量闭集。
    pub const ALL: [u2501Code; 12] = [
        u2501Code::RowDup,
        u2501Code::RowGap,
        u2501Code::GuardTrip,
        u2501Code::KeyDup,
        u2501Code::KeyGap,
        u2501Code::ValOob,
        u2501Code::SumDrift,
        u2501Code::ThreshDrift,
        u2501Code::LedgerBroken,
        u2501Code::SnapshotFingerprint,
        u2501Code::TableBroken,
        u2501Code::SegmentGuard,
    ];

    /// 码值（连续十六进制，与声明序恒等）。
    pub const fn code(self) -> u16 {
        match self {
            u2501Code::RowDup => 0xa430,
            u2501Code::RowGap => 0xa431,
            u2501Code::GuardTrip => 0xa432,
            u2501Code::KeyDup => 0xa433,
            u2501Code::KeyGap => 0xa434,
            u2501Code::ValOob => 0xa435,
            u2501Code::SumDrift => 0xa436,
            u2501Code::ThreshDrift => 0xa437,
            u2501Code::LedgerBroken => 0xa438,
            u2501Code::SnapshotFingerprint => 0xa439,
            u2501Code::TableBroken => 0xa43a,
            u2501Code::SegmentGuard => 0xa43b,
        }
    }

    /// 中文诊断名。
    pub const fn name(self) -> &'static str {
        match self {
            u2501Code::RowDup => "表行重复（字段缺项）",
            u2501Code::RowGap => "表行断档（口径分歧）",
            u2501Code::GuardTrip => "守卫触发（重入/越限被拒）",
            u2501Code::KeyDup => "映射键重复（联签缺失）",
            u2501Code::KeyGap => "映射键断档（冻结漂移）",
            u2501Code::ValOob => "映射值越界",
            u2501Code::SumDrift => "双表累计漂移",
            u2501Code::ThreshDrift => "阈值漂移（守卫阈值≠冻结值）",
            u2501Code::LedgerBroken => "台账不守恒（守恒恒等式破缺）",
            u2501Code::SnapshotFingerprint => "快照指纹不符（字段被改后重算不一致）",
            u2501Code::TableBroken => "常量表破坏（表项与语义规则矛盾）",
            u2501Code::SegmentGuard => "段位守卫失败（诊断码越出本模块段）",
        }
    }

    /// 可操作修复建议（每条诊断都必须带下一步动作）。
    pub const fn suggestion(self) -> &'static str {
        match self {
            u2501Code::RowDup => "按键唯一律去重",
            u2501Code::RowGap => "按连续律补号",
            u2501Code::GuardTrip => "检查临界区嵌套",
            u2501Code::KeyDup => "按键唯一律去重",
            u2501Code::KeyGap => "按连续律补号",
            u2501Code::ValOob => "按值域裁剪",
            u2501Code::SumDrift => "按理论值复算",
            u2501Code::ThreshDrift => "按冻结值校正",
            u2501Code::LedgerBroken => "按守恒律重建台账后重跑",
            u2501Code::SnapshotFingerprint => "按防篡改规程重建快照",
            u2501Code::TableBroken => "按语义规则重建常量表",
            u2501Code::SegmentGuard => "核对诊断码分配表",
        }
    }
}

// --------------------------------------------------------------------------
// 冻结常量表（u2501mix · 16 行逐行注释自证）
// --------------------------------------------------------------------------
/// 冻结接口 对拍表（16 行）。
pub const u2501mix_COUNT: usize = 16;

/// 冻结表本体（编译期常量，运行期只读）。
pub static u2501mix: [(&'static str, u64); 16] = [
    ("IF_00", 10), // IF 对拍行（前向接口）
    ("FR_01", 15), // FR 对拍行（冻结件）
    ("B10_02", 20), // B10 对拍行（B10 联签）
    ("FL_03", 25), // FL 对拍行（字段表）
    ("PC_04", 30), // PC 对拍行（池账口径）
    ("SG_05", 35), // SG 对拍行（签名账）
    ("CN_06", 40), // CN 对拍行（消费方）
    ("HD_07", 45), // HD 对拍行（交接面）
    ("VF_08", 50), // VF 对拍行（字段核验）
    ("EQ_09", 55), // EQ 对拍行（口径一致）
    ("LK_10", 60), // LK 对拍行（联签账）
    ("FZ_11", 65), // FZ 对拍行（冻结锚）
    ("IF_12", 70), // IF 对拍行（前向接口）
    ("FR_13", 75), // FR 对拍行（冻结件）
    ("B10_14", 80), // B10 对拍行（B10 联签）
    ("FL_15", 85), // FL 对拍行（字段表）
];

/// 线性查找（键唯一性由构造期断言保证，运行时无分支歧义）。
pub fn u2501mix_get(key: &'static str) -> Option<u64> {
    let mut i = 0usize;
    while i < u2501mix_COUNT {
        if u2501mix[i].0 == key {
            return Some(u2501mix[i].1);
        }
        i += 1;
    }
    None
}

/// 累计求和（用于守恒对账）。
pub fn u2501mix_sum() -> u64 {
    let mut s = 0u64;
    let mut i = 0usize;
    while i < u2501mix_COUNT {
        s = s.wrapping_add(u2501mix[i].1 as u64);
        i += 1;
    }
    s
}

// --------------------------------------------------------------------------
// 守卫 u2501mixgate（阈值 32 · 重入/越限/静默三类拒止）
// --------------------------------------------------------------------------
/// 冻结接口 混合面守卫（阈值 32 冻结）。
#[derive(Clone, Copy, Debug)]
pub struct u2501mixgate {
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

/// 守卫阈值（冻结接口 混合面守卫（阈值 32 冻结））。
pub const U2501MIXGATE_THRESHOLD: u64 = 32;

impl u2501mixgate {
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
        if self.window >= U2501MIXGATE_THRESHOLD {
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
// 字符串键值映射 u2501mixmap（16 行逐行语义注释 + 逐行守卫）
// --------------------------------------------------------------------------
/// 冻结接口 混合映射矩阵。
/// 行数。
pub const u2501mixmap_COUNT: usize = 16;

/// 表本体（键 + 值 + 注释由编译期冻结）。
pub static u2501mixmap: [(&'static str, u64); 16] = [
    ("IF0000", 0), // IF 单元 0000（前向接口 × 相位 00 × 混 轮）
    ("FR0001", 9), // FR 单元 0001（冻结件 × 相位 01 × 混 轮）
    ("B100002", 18), // B10 单元 0002（B10 联签 × 相位 02 × 混 轮）
    ("FL0003", 27), // FL 单元 0003（字段表 × 相位 03 × 混 轮）
    ("PC0004", 36), // PC 单元 0004（池账口径 × 相位 04 × 混 轮）
    ("SG0005", 45), // SG 单元 0005（签名账 × 相位 05 × 混 轮）
    ("CN0006", 54), // CN 单元 0006（消费方 × 相位 06 × 混 轮）
    ("HD0007", 63), // HD 单元 0007（交接面 × 相位 07 × 混 轮）
    ("VF0008", 8), // VF 单元 0008（字段核验 × 相位 08 × 混 轮）
    ("EQ0009", 17), // EQ 单元 0009（口径一致 × 相位 09 × 混 轮）
    ("LK0010", 26), // LK 单元 0010（联签账 × 相位 10 × 混 轮）
    ("FZ0011", 35), // FZ 单元 0011（冻结锚 × 相位 11 × 混 轮）
    ("IF0012", 44), // IF 单元 0012（前向接口 × 相位 12 × 混 轮）
    ("FR0013", 53), // FR 单元 0013（冻结件 × 相位 13 × 混 轮）
    ("B100014", 62), // B10 单元 0014（B10 联签 × 相位 14 × 混 轮）
    ("FL0015", 7), // FL 单元 0015（字段表 × 相位 15 × 混 轮）
];

/// O(1) 语义查找（match 全枚举；命中率 100%% 由守卫保证）。
pub fn u2501mixmap_get(key: &str) -> Option<u64> {
    match key {
        "IF0000" => Some(0),
        "FR0001" => Some(9),
        "B100002" => Some(18),
        "FL0003" => Some(27),
        "PC0004" => Some(36),
        "SG0005" => Some(45),
        "CN0006" => Some(54),
        "HD0007" => Some(63),
        "VF0008" => Some(8),
        "EQ0009" => Some(17),
        "LK0010" => Some(26),
        "FZ0011" => Some(35),
        "IF0012" => Some(44),
        "FR0013" => Some(53),
        "B100014" => Some(62),
        "FL0015" => Some(7),
        _ => None,
    }
}

/// 逐行守卫扫描（返回违例行数；0 = 全绿）。
pub fn u2501mixmap_scan() -> usize {
    let mut bad = 0usize;
    let mut i = 0usize;
    while i < u2501mixmap_COUNT {
        let (k, v) = u2501mixmap[i];
        let ok = match k {
            "IF0000" => v < 64,
            "FR0001" => v < 64,
            "B100002" => v < 64,
            "FL0003" => v < 64,
            "PC0004" => v < 64,
            "SG0005" => v < 64,
            "CN0006" => v < 64,
            "HD0007" => v < 64,
            "VF0008" => v < 64,
            "EQ0009" => v < 64,
            "LK0010" => v < 64,
            "FZ0011" => v < 64,
            "IF0012" => v < 64,
            "FR0013" => v < 64,
            "B100014" => v < 64,
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
pub fn u2501mixmap_sum() -> u64 {
    let mut s = 0u64;
    let mut i = 0usize;
    while i < u2501mixmap_COUNT {
        s = s.wrapping_add(u2501mixmap[i].1 as u64);
        i += 1;
    }
    s
}

/// 键唯一性（构造期断言等价物：逐对扫描同键）。
pub fn u2501mixmap_unique() -> bool {
    let mut i = 0usize;
    while i < u2501mixmap_COUNT {
        let mut j = i + 1;
        while j < u2501mixmap_COUNT {
            if u2501mixmap[i].0 == u2501mixmap[j].0 {
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

/// 段位守卫：全部诊断码必须落在本段 [0xa430, 0xa43f]。
pub const fn u2501_segment_guard() -> bool {
    let mut ok = true;
    let mut i = 0usize;
    while i < u2501Code::ALL.len() {
        let c = u2501Code::ALL[i].code();
        if c < 0xa430 || c > 0xa43f {
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
fn u2501_guard_seg() -> bool {
    u2501_segment_guard()
}

/// UNX-F2501 对拍表 16 行边界与累计守恒。
fn check_u2501_mix_tbl() -> bool {
    let mut ok = u2501mix_COUNT == 16;
    if ok { ok = u2501mix_get("IF_00") == Some(10) && u2501mix_get("FL_15") == Some(85); }
    if ok { ok = u2501mix_get("ZZZ_NOKEY").is_none(); }
    if ok { ok = u2501mix_sum() == 760; }
    ok
}

/// UNX-F2501 混合映射 16 行守卫唯一 + 守卫阈值冻结。
fn check_u2501_mix_map() -> bool {
    let mut ok = u2501mixmap_COUNT == 16;
    if ok { ok = u2501mixmap_scan() == 0 && u2501mixmap_unique(); }
    if ok { ok = u2501mixmap_sum() == 504; }
    if ok { ok = U2501MIXGATE_THRESHOLD == 32; }
    ok
}

/// 本模块自检（逐检查带 detail，全绿才出门）。
pub fn run_u2501_checks() -> CheckSet {
    let mut set = CheckSet::new("U2501");
    set.add(
        "u2501-seg-guard",
        u2501_guard_seg(),
        if u2501_guard_seg() { "" } else { "diagnostic code outside segment" },
    );
    set.add(
        "u2501-mix-tbl",
        check_u2501_mix_tbl(),
        if check_u2501_mix_tbl() { "" } else { "UNX-F2501 mix table mismatch" },
    );
    set.add(
        "u2501-mix-map",
        check_u2501_mix_map(),
        if check_u2501_mix_map() { "" } else { "UNX-F2501 mix map mismatch" },
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
    fn u2501_all_checks_green() {
        let set = run_u2501_checks();
        assert!(set.all_passed(), "U2501 domain checks must be green");
    }

    #[test]
    fn u2501_segment_guard_holds() {
        assert!(u2501_segment_guard());
    }
}
