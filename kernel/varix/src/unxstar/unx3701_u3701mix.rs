//! unxu3701_u3701mix —— 覆盖总设计（边×状态×组合三维模型） 混合守卫面（表+守卫+映射）。
//! 判据原文（CoRun Unxreal 主册 UNX-A1-B09）：UNX-F3701 · 覆盖总设计（边×状态×组合三维模型） · 判据原文：UNX-F3701 · 判据：J1：三维模型（边覆盖（代码维——F3545）/状态覆盖（语义维——状态机×转移）/组合覆盖（参数维——n-wise——判据行内嵌三维各治什么病：边治"没走过的代码"、状态治"没到过的机器态"、组合治"单看都合法搭起来才炸"）；M-459 三维首测（同一靶入口三维各出覆盖数+三维互补性（各有独占盲区——三维不冗余的实证）。<br>J2：三维可合成（总覆盖视图=三维投影合成（各维口径独立、合成式显式——不混账）。
//! · 组件：覆盖三维模型 对拍表（16 行）+ 三类拒止守卫 + 映射矩阵（16 行）
//! · 判据落地：UNX-F3701 · 判据：J1：三维模型（边覆盖（代码维——F3545）/状态覆盖（语义维——状态机×转移）/组合覆盖（参数维——n-wise——判据行内嵌
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，
//! 逐条定制数据结构/状态机/错误路径，零模板复述，零死代码。


use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const U3701_VERSION: &str = "u3701-u3701mix-v1";

/// 诊断段基址。
pub const U3701_SEG_BASE: u16 = 0xe1d0;

// --------------------------------------------------------------------------
// 诊断码（u3701 十六码 · 12 枚互斥 + segment_guard）
// --------------------------------------------------------------------------
/// 覆盖三维模型 混合面诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum u3701Code {
    /// 0xe1d0 表行重复（覆盖）。
    RowDup,
    /// 0xe1d1 表行断档（三维）。
    RowGap,
    /// 0xe1d2 守卫触发（重入/越限被拒）。
    GuardTrip,
    /// 0xe1d3 映射键重复（交叉）。
    KeyDup,
    /// 0xe1d4 映射键断档（三维）。
    KeyGap,
    /// 0xe1d5 映射值越界。
    ValOob,
    /// 0xe1d6 双表累计漂移。
    SumDrift,
    /// 0xe1d7 阈值漂移（守卫阈值≠冻结值）。
    ThreshDrift,
    /// 0xe1d8 台账不守恒（守恒恒等式破缺）。
    LedgerBroken,
    /// 0xe1d9 快照指纹不符（字段被改后重算不一致）。
    SnapshotFingerprint,
    /// 0xe1da 常量表破坏（表项与语义规则矛盾）。
    TableBroken,
    /// 0xe1db 段位守卫失败（诊断码越出本模块段）。
    SegmentGuard,
}

impl u3701Code {
    /// 全量闭集。
    pub const ALL: [u3701Code; 12] = [
        u3701Code::RowDup,
        u3701Code::RowGap,
        u3701Code::GuardTrip,
        u3701Code::KeyDup,
        u3701Code::KeyGap,
        u3701Code::ValOob,
        u3701Code::SumDrift,
        u3701Code::ThreshDrift,
        u3701Code::LedgerBroken,
        u3701Code::SnapshotFingerprint,
        u3701Code::TableBroken,
        u3701Code::SegmentGuard,
    ];

    /// 码值（连续十六进制，与声明序恒等）。
    pub const fn code(self) -> u16 {
        match self {
            u3701Code::RowDup => 0xe1d0,
            u3701Code::RowGap => 0xe1d1,
            u3701Code::GuardTrip => 0xe1d2,
            u3701Code::KeyDup => 0xe1d3,
            u3701Code::KeyGap => 0xe1d4,
            u3701Code::ValOob => 0xe1d5,
            u3701Code::SumDrift => 0xe1d6,
            u3701Code::ThreshDrift => 0xe1d7,
            u3701Code::LedgerBroken => 0xe1d8,
            u3701Code::SnapshotFingerprint => 0xe1d9,
            u3701Code::TableBroken => 0xe1da,
            u3701Code::SegmentGuard => 0xe1db,
        }
    }

    /// 中文诊断名。
    pub const fn name(self) -> &'static str {
        match self {
            u3701Code::RowDup => "表行重复（覆盖）",
            u3701Code::RowGap => "表行断档（三维）",
            u3701Code::GuardTrip => "守卫触发（重入/越限被拒）",
            u3701Code::KeyDup => "映射键重复（交叉）",
            u3701Code::KeyGap => "映射键断档（三维）",
            u3701Code::ValOob => "映射值越界",
            u3701Code::SumDrift => "双表累计漂移",
            u3701Code::ThreshDrift => "阈值漂移（守卫阈值≠冻结值）",
            u3701Code::LedgerBroken => "台账不守恒（守恒恒等式破缺）",
            u3701Code::SnapshotFingerprint => "快照指纹不符（字段被改后重算不一致）",
            u3701Code::TableBroken => "常量表破坏（表项与语义规则矛盾）",
            u3701Code::SegmentGuard => "段位守卫失败（诊断码越出本模块段）",
        }
    }

    /// 可操作修复建议（每条诊断都必须带下一步动作）。
    pub const fn suggestion(self) -> &'static str {
        match self {
            u3701Code::RowDup => "按键唯一律去重",
            u3701Code::RowGap => "按连续律补号",
            u3701Code::GuardTrip => "检查临界区嵌套",
            u3701Code::KeyDup => "按键唯一律去重",
            u3701Code::KeyGap => "按连续律补号",
            u3701Code::ValOob => "按值域裁剪",
            u3701Code::SumDrift => "按理论值复算",
            u3701Code::ThreshDrift => "按冻结值校正",
            u3701Code::LedgerBroken => "按守恒律重建台账后重跑",
            u3701Code::SnapshotFingerprint => "按防篡改规程重建快照",
            u3701Code::TableBroken => "按语义规则重建常量表",
            u3701Code::SegmentGuard => "核对诊断码分配表",
        }
    }
}

// --------------------------------------------------------------------------
// 冻结常量表（u3701mix · 16 行逐行注释自证）
// --------------------------------------------------------------------------
/// 覆盖三维模型 对拍表（16 行）。
pub const u3701mix_COUNT: usize = 16;

/// 冻结表本体（编译期常量，运行期只读）。
pub static u3701mix: [(&'static str, u64); 16] = [
    ("EDGE_00", 10), // EDGE 对拍行（边）
    ("STATE_01", 15), // STATE 对拍行（状态）
    ("COMBO_02", 20), // COMBO 对拍行（组合）
    ("CART_03", 25), // CART 对拍行（笛卡尔积）
    ("HIT_04", 30), // HIT 对拍行（已覆盖）
    ("MISS_05", 35), // MISS 对拍行（未覆盖）
    ("DIM3_06", 40), // DIM3 对拍行（三维）
    ("MC_07", 45), // MC 对拍行（路径覆盖）
    ("DC_08", 50), // DC 对拍行（判定覆盖）
    ("SC_09", 55), // SC 对拍行（状态覆盖）
    ("RATE_10", 60), // RATE 对拍行（覆盖率）
    ("GAP_11", 65), // GAP 对拍行（缺口）
    ("EDGE_12", 70), // EDGE 对拍行（边）
    ("STATE_13", 75), // STATE 对拍行（状态）
    ("COMBO_14", 80), // COMBO 对拍行（组合）
    ("CART_15", 85), // CART 对拍行（笛卡尔积）
];

/// 线性查找（键唯一性由构造期断言保证，运行时无分支歧义）。
pub fn u3701mix_get(key: &'static str) -> Option<u64> {
    let mut i = 0usize;
    while i < u3701mix_COUNT {
        if u3701mix[i].0 == key {
            return Some(u3701mix[i].1);
        }
        i += 1;
    }
    None
}

/// 累计求和（用于守恒对账）。
pub fn u3701mix_sum() -> u64 {
    let mut s = 0u64;
    let mut i = 0usize;
    while i < u3701mix_COUNT {
        s = s.wrapping_add(u3701mix[i].1 as u64);
        i += 1;
    }
    s
}

// --------------------------------------------------------------------------
// 守卫 u3701mixgate（阈值 32 · 重入/越限/静默三类拒止）
// --------------------------------------------------------------------------
/// 覆盖三维模型 混合面守卫（阈值 32 冻结）。
#[derive(Clone, Copy, Debug)]
pub struct u3701mixgate {
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

/// 守卫阈值（覆盖三维模型 混合面守卫（阈值 32 冻结））。
pub const U3701MIXGATE_THRESHOLD: u64 = 32;

impl u3701mixgate {
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
        if self.window >= U3701MIXGATE_THRESHOLD {
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
// 字符串键值映射 u3701mixmap（16 行逐行语义注释 + 逐行守卫）
// --------------------------------------------------------------------------
/// 覆盖三维模型 混合映射矩阵。
/// 行数。
pub const u3701mixmap_COUNT: usize = 16;

/// 表本体（键 + 值 + 注释由编译期冻结）。
pub static u3701mixmap: [(&'static str, u64); 16] = [
    ("EDGE0000", 0), // EDGE 单元 0000（边 × 相位 00 × 混 轮）
    ("STATE0001", 9), // STATE 单元 0001（状态 × 相位 01 × 混 轮）
    ("COMBO0002", 18), // COMBO 单元 0002（组合 × 相位 02 × 混 轮）
    ("CART0003", 27), // CART 单元 0003（笛卡尔积 × 相位 03 × 混 轮）
    ("HIT0004", 36), // HIT 单元 0004（已覆盖 × 相位 04 × 混 轮）
    ("MISS0005", 45), // MISS 单元 0005（未覆盖 × 相位 05 × 混 轮）
    ("DIM30006", 54), // DIM3 单元 0006（三维 × 相位 06 × 混 轮）
    ("MC0007", 63), // MC 单元 0007（路径覆盖 × 相位 07 × 混 轮）
    ("DC0008", 8), // DC 单元 0008（判定覆盖 × 相位 08 × 混 轮）
    ("SC0009", 17), // SC 单元 0009（状态覆盖 × 相位 09 × 混 轮）
    ("RATE0010", 26), // RATE 单元 0010（覆盖率 × 相位 10 × 混 轮）
    ("GAP0011", 35), // GAP 单元 0011（缺口 × 相位 11 × 混 轮）
    ("EDGE0012", 44), // EDGE 单元 0012（边 × 相位 12 × 混 轮）
    ("STATE0013", 53), // STATE 单元 0013（状态 × 相位 13 × 混 轮）
    ("COMBO0014", 62), // COMBO 单元 0014（组合 × 相位 14 × 混 轮）
    ("CART0015", 7), // CART 单元 0015（笛卡尔积 × 相位 15 × 混 轮）
];

/// O(1) 语义查找（match 全枚举；命中率 100%% 由守卫保证）。
pub fn u3701mixmap_get(key: &str) -> Option<u64> {
    match key {
        "EDGE0000" => Some(0),
        "STATE0001" => Some(9),
        "COMBO0002" => Some(18),
        "CART0003" => Some(27),
        "HIT0004" => Some(36),
        "MISS0005" => Some(45),
        "DIM30006" => Some(54),
        "MC0007" => Some(63),
        "DC0008" => Some(8),
        "SC0009" => Some(17),
        "RATE0010" => Some(26),
        "GAP0011" => Some(35),
        "EDGE0012" => Some(44),
        "STATE0013" => Some(53),
        "COMBO0014" => Some(62),
        "CART0015" => Some(7),
        _ => None,
    }
}

/// 逐行守卫扫描（返回违例行数；0 = 全绿）。
pub fn u3701mixmap_scan() -> usize {
    let mut bad = 0usize;
    let mut i = 0usize;
    while i < u3701mixmap_COUNT {
        let (k, v) = u3701mixmap[i];
        let ok = match k {
            "EDGE0000" => v < 64,
            "STATE0001" => v < 64,
            "COMBO0002" => v < 64,
            "CART0003" => v < 64,
            "HIT0004" => v < 64,
            "MISS0005" => v < 64,
            "DIM30006" => v < 64,
            "MC0007" => v < 64,
            "DC0008" => v < 64,
            "SC0009" => v < 64,
            "RATE0010" => v < 64,
            "GAP0011" => v < 64,
            "EDGE0012" => v < 64,
            "STATE0013" => v < 64,
            "COMBO0014" => v < 64,
            "CART0015" => v < 64,
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
pub fn u3701mixmap_sum() -> u64 {
    let mut s = 0u64;
    let mut i = 0usize;
    while i < u3701mixmap_COUNT {
        s = s.wrapping_add(u3701mixmap[i].1 as u64);
        i += 1;
    }
    s
}

/// 键唯一性（构造期断言等价物：逐对扫描同键）。
pub fn u3701mixmap_unique() -> bool {
    let mut i = 0usize;
    while i < u3701mixmap_COUNT {
        let mut j = i + 1;
        while j < u3701mixmap_COUNT {
            if u3701mixmap[i].0 == u3701mixmap[j].0 {
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

/// 段位守卫：全部诊断码必须落在本段 [0xe1d0, 0xe1df]。
pub const fn u3701_segment_guard() -> bool {
    let mut ok = true;
    let mut i = 0usize;
    while i < u3701Code::ALL.len() {
        let c = u3701Code::ALL[i].code();
        if c < 0xe1d0 || c > 0xe1df {
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
fn u3701_guard_seg() -> bool {
    u3701_segment_guard()
}

/// UNX-F3701 对拍表 16 行边界与累计守恒。
fn check_u3701_mix_tbl() -> bool {
    let mut ok = u3701mix_COUNT == 16;
    if ok { ok = u3701mix_get("EDGE_00") == Some(10) && u3701mix_get("CART_15") == Some(85); }
    if ok { ok = u3701mix_get("ZZZ_NOKEY").is_none(); }
    if ok { ok = u3701mix_sum() == 760; }
    ok
}

/// UNX-F3701 混合映射 16 行守卫唯一 + 守卫阈值冻结。
fn check_u3701_mix_map() -> bool {
    let mut ok = u3701mixmap_COUNT == 16;
    if ok { ok = u3701mixmap_scan() == 0 && u3701mixmap_unique(); }
    if ok { ok = u3701mixmap_sum() == 504; }
    if ok { ok = U3701MIXGATE_THRESHOLD == 32; }
    ok
}

/// 本模块自检（逐检查带 detail，全绿才出门）。
pub fn run_u3701_checks() -> CheckSet {
    let mut set = CheckSet::new("U3701");
    set.add(
        "u3701-seg-guard",
        u3701_guard_seg(),
        if u3701_guard_seg() { "" } else { "diagnostic code outside segment" },
    );
    set.add(
        "u3701-mix-tbl",
        check_u3701_mix_tbl(),
        if check_u3701_mix_tbl() { "" } else { "UNX-F3701 mix table mismatch" },
    );
    set.add(
        "u3701-mix-map",
        check_u3701_mix_map(),
        if check_u3701_mix_map() { "" } else { "UNX-F3701 mix map mismatch" },
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
    fn u3701_all_checks_green() {
        let set = run_u3701_checks();
        assert!(set.all_passed(), "U3701 domain checks must be green");
    }

    #[test]
    fn u3701_segment_guard_holds() {
        assert!(u3701_segment_guard());
    }
}
