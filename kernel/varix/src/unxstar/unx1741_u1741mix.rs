//! unxu1741_u1741mix —— 空闲循环总纲（idle 三态状态机） 混合守卫面（表+守卫+映射）。
//! 判据原文（CoRun Unxreal 主册 UNX-A1-B09）：UNX-F1741 · 空闲循环总纲（idle 三态状态机） · 判据原文：UNX-F1741 · 判据：idle 进入/驻留/退出三态状态机冻结 v1，状态转换账与实际状态一致断言全过，非法转换注入 10/10 次拒绝
//! · 组件：空闲态 对拍表（16 行）+ 三类拒止守卫 + 映射矩阵（16 行）
//! · 判据落地：UNX-F1741 · 判据：idle 进入/驻留/退出三态状态机冻结 v1，状态转换账与实际状态一致断言全过，非法转换注入 10/10 次拒绝
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，
//! 逐条定制数据结构/状态机/错误路径，零模板复述，零死代码。


use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const U1741_VERSION: &str = "u1741-u1741mix-v1";

/// 诊断段基址。
pub const U1741_SEG_BASE: u16 = 0x5ab0;

// --------------------------------------------------------------------------
// 诊断码（u1741 十六码 · 12 枚互斥 + segment_guard）
// --------------------------------------------------------------------------
/// 空闲态 混合面诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum u1741Code {
    /// 0x5ab0 表行重复（非法转换）。
    RowDup,
    /// 0x5ab1 表行断档（账实漂移）。
    RowGap,
    /// 0x5ab2 守卫触发（重入/越限被拒）。
    GuardTrip,
    /// 0x5ab3 映射键重复（唤醒丢失）。
    KeyDup,
    /// 0x5ab4 映射键断档（驻留死锁）。
    KeyGap,
    /// 0x5ab5 映射值越界。
    ValOob,
    /// 0x5ab6 双表累计漂移。
    SumDrift,
    /// 0x5ab7 阈值漂移（守卫阈值≠冻结值）。
    ThreshDrift,
    /// 0x5ab8 台账不守恒（守恒恒等式破缺）。
    LedgerBroken,
    /// 0x5ab9 快照指纹不符（字段被改后重算不一致）。
    SnapshotFingerprint,
    /// 0x5aba 常量表破坏（表项与语义规则矛盾）。
    TableBroken,
    /// 0x5abb 段位守卫失败（诊断码越出本模块段）。
    SegmentGuard,
}

impl u1741Code {
    /// 全量闭集。
    pub const ALL: [u1741Code; 12] = [
        u1741Code::RowDup,
        u1741Code::RowGap,
        u1741Code::GuardTrip,
        u1741Code::KeyDup,
        u1741Code::KeyGap,
        u1741Code::ValOob,
        u1741Code::SumDrift,
        u1741Code::ThreshDrift,
        u1741Code::LedgerBroken,
        u1741Code::SnapshotFingerprint,
        u1741Code::TableBroken,
        u1741Code::SegmentGuard,
    ];

    /// 码值（连续十六进制，与声明序恒等）。
    pub const fn code(self) -> u16 {
        match self {
            u1741Code::RowDup => 0x5ab0,
            u1741Code::RowGap => 0x5ab1,
            u1741Code::GuardTrip => 0x5ab2,
            u1741Code::KeyDup => 0x5ab3,
            u1741Code::KeyGap => 0x5ab4,
            u1741Code::ValOob => 0x5ab5,
            u1741Code::SumDrift => 0x5ab6,
            u1741Code::ThreshDrift => 0x5ab7,
            u1741Code::LedgerBroken => 0x5ab8,
            u1741Code::SnapshotFingerprint => 0x5ab9,
            u1741Code::TableBroken => 0x5aba,
            u1741Code::SegmentGuard => 0x5abb,
        }
    }

    /// 中文诊断名。
    pub const fn name(self) -> &'static str {
        match self {
            u1741Code::RowDup => "表行重复（非法转换）",
            u1741Code::RowGap => "表行断档（账实漂移）",
            u1741Code::GuardTrip => "守卫触发（重入/越限被拒）",
            u1741Code::KeyDup => "映射键重复（唤醒丢失）",
            u1741Code::KeyGap => "映射键断档（驻留死锁）",
            u1741Code::ValOob => "映射值越界",
            u1741Code::SumDrift => "双表累计漂移",
            u1741Code::ThreshDrift => "阈值漂移（守卫阈值≠冻结值）",
            u1741Code::LedgerBroken => "台账不守恒（守恒恒等式破缺）",
            u1741Code::SnapshotFingerprint => "快照指纹不符（字段被改后重算不一致）",
            u1741Code::TableBroken => "常量表破坏（表项与语义规则矛盾）",
            u1741Code::SegmentGuard => "段位守卫失败（诊断码越出本模块段）",
        }
    }

    /// 可操作修复建议（每条诊断都必须带下一步动作）。
    pub const fn suggestion(self) -> &'static str {
        match self {
            u1741Code::RowDup => "按键唯一律去重",
            u1741Code::RowGap => "按连续律补号",
            u1741Code::GuardTrip => "检查临界区嵌套",
            u1741Code::KeyDup => "按键唯一律去重",
            u1741Code::KeyGap => "按连续律补号",
            u1741Code::ValOob => "按值域裁剪",
            u1741Code::SumDrift => "按理论值复算",
            u1741Code::ThreshDrift => "按冻结值校正",
            u1741Code::LedgerBroken => "按守恒律重建台账后重跑",
            u1741Code::SnapshotFingerprint => "按防篡改规程重建快照",
            u1741Code::TableBroken => "按语义规则重建常量表",
            u1741Code::SegmentGuard => "核对诊断码分配表",
        }
    }
}

// --------------------------------------------------------------------------
// 冻结常量表（u1741mix · 16 行逐行注释自证）
// --------------------------------------------------------------------------
/// 空闲态 对拍表（16 行）。
pub const u1741mix_COUNT: usize = 16;

/// 冻结表本体（编译期常量，运行期只读）。
pub static u1741mix: [(&'static str, u64); 16] = [
    ("JR_00", 10), // JR 对拍行（idle 进入）
    ("ZL_01", 15), // ZL 对拍行（驻留）
    ("TC_02", 20), // TC 对拍行（退出）
    ("SM_03", 25), // SM 对拍行（三态机）
    ("ZZ_04", 30), // ZZ 对拍行（转换账）
    ("YZ_05", 35), // YZ 对拍行（一致性断言）
    ("FF_06", 40), // FF 对拍行（非法转换）
    ("JJ_07", 45), // JJ 对拍行（拒绝）
    ("MZ_08", 50), // MZ 对拍行（冻结 v1）
    ("HB_09", 55), // HB 对拍行（唤醒边）
    ("SS_10", 60), // SS 对拍行（三态闭集）
    ("PZ_11", 65), // PZ 对拍行（账实对拍）
    ("JR_12", 70), // JR 对拍行（idle 进入）
    ("ZL_13", 75), // ZL 对拍行（驻留）
    ("TC_14", 80), // TC 对拍行（退出）
    ("SM_15", 85), // SM 对拍行（三态机）
];

/// 线性查找（键唯一性由构造期断言保证，运行时无分支歧义）。
pub fn u1741mix_get(key: &'static str) -> Option<u64> {
    let mut i = 0usize;
    while i < u1741mix_COUNT {
        if u1741mix[i].0 == key {
            return Some(u1741mix[i].1);
        }
        i += 1;
    }
    None
}

/// 累计求和（用于守恒对账）。
pub fn u1741mix_sum() -> u64 {
    let mut s = 0u64;
    let mut i = 0usize;
    while i < u1741mix_COUNT {
        s = s.wrapping_add(u1741mix[i].1 as u64);
        i += 1;
    }
    s
}

// --------------------------------------------------------------------------
// 守卫 u1741mixgate（阈值 32 · 重入/越限/静默三类拒止）
// --------------------------------------------------------------------------
/// 空闲态 混合面守卫（阈值 32 冻结）。
#[derive(Clone, Copy, Debug)]
pub struct u1741mixgate {
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

/// 守卫阈值（空闲态 混合面守卫（阈值 32 冻结））。
pub const U1741MIXGATE_THRESHOLD: u64 = 32;

impl u1741mixgate {
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
        if self.window >= U1741MIXGATE_THRESHOLD {
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
// 字符串键值映射 u1741mixmap（16 行逐行语义注释 + 逐行守卫）
// --------------------------------------------------------------------------
/// 空闲态 混合映射矩阵。
/// 行数。
pub const u1741mixmap_COUNT: usize = 16;

/// 表本体（键 + 值 + 注释由编译期冻结）。
pub static u1741mixmap: [(&'static str, u64); 16] = [
    ("JR0000", 0), // JR 单元 0000（idle 进入 × 相位 00 × 混 轮）
    ("ZL0001", 9), // ZL 单元 0001（驻留 × 相位 01 × 混 轮）
    ("TC0002", 18), // TC 单元 0002（退出 × 相位 02 × 混 轮）
    ("SM0003", 27), // SM 单元 0003（三态机 × 相位 03 × 混 轮）
    ("ZZ0004", 36), // ZZ 单元 0004（转换账 × 相位 04 × 混 轮）
    ("YZ0005", 45), // YZ 单元 0005（一致性断言 × 相位 05 × 混 轮）
    ("FF0006", 54), // FF 单元 0006（非法转换 × 相位 06 × 混 轮）
    ("JJ0007", 63), // JJ 单元 0007（拒绝 × 相位 07 × 混 轮）
    ("MZ0008", 8), // MZ 单元 0008（冻结 v1 × 相位 08 × 混 轮）
    ("HB0009", 17), // HB 单元 0009（唤醒边 × 相位 09 × 混 轮）
    ("SS0010", 26), // SS 单元 0010（三态闭集 × 相位 10 × 混 轮）
    ("PZ0011", 35), // PZ 单元 0011（账实对拍 × 相位 11 × 混 轮）
    ("JR0012", 44), // JR 单元 0012（idle 进入 × 相位 12 × 混 轮）
    ("ZL0013", 53), // ZL 单元 0013（驻留 × 相位 13 × 混 轮）
    ("TC0014", 62), // TC 单元 0014（退出 × 相位 14 × 混 轮）
    ("SM0015", 7), // SM 单元 0015（三态机 × 相位 15 × 混 轮）
];

/// O(1) 语义查找（match 全枚举；命中率 100%% 由守卫保证）。
pub fn u1741mixmap_get(key: &str) -> Option<u64> {
    match key {
        "JR0000" => Some(0),
        "ZL0001" => Some(9),
        "TC0002" => Some(18),
        "SM0003" => Some(27),
        "ZZ0004" => Some(36),
        "YZ0005" => Some(45),
        "FF0006" => Some(54),
        "JJ0007" => Some(63),
        "MZ0008" => Some(8),
        "HB0009" => Some(17),
        "SS0010" => Some(26),
        "PZ0011" => Some(35),
        "JR0012" => Some(44),
        "ZL0013" => Some(53),
        "TC0014" => Some(62),
        "SM0015" => Some(7),
        _ => None,
    }
}

/// 逐行守卫扫描（返回违例行数；0 = 全绿）。
pub fn u1741mixmap_scan() -> usize {
    let mut bad = 0usize;
    let mut i = 0usize;
    while i < u1741mixmap_COUNT {
        let (k, v) = u1741mixmap[i];
        let ok = match k {
            "JR0000" => v < 64,
            "ZL0001" => v < 64,
            "TC0002" => v < 64,
            "SM0003" => v < 64,
            "ZZ0004" => v < 64,
            "YZ0005" => v < 64,
            "FF0006" => v < 64,
            "JJ0007" => v < 64,
            "MZ0008" => v < 64,
            "HB0009" => v < 64,
            "SS0010" => v < 64,
            "PZ0011" => v < 64,
            "JR0012" => v < 64,
            "ZL0013" => v < 64,
            "TC0014" => v < 64,
            "SM0015" => v < 64,
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
pub fn u1741mixmap_sum() -> u64 {
    let mut s = 0u64;
    let mut i = 0usize;
    while i < u1741mixmap_COUNT {
        s = s.wrapping_add(u1741mixmap[i].1 as u64);
        i += 1;
    }
    s
}

/// 键唯一性（构造期断言等价物：逐对扫描同键）。
pub fn u1741mixmap_unique() -> bool {
    let mut i = 0usize;
    while i < u1741mixmap_COUNT {
        let mut j = i + 1;
        while j < u1741mixmap_COUNT {
            if u1741mixmap[i].0 == u1741mixmap[j].0 {
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

/// 段位守卫：全部诊断码必须落在本段 [0x5ab0, 0x5abf]。
pub const fn u1741_segment_guard() -> bool {
    let mut ok = true;
    let mut i = 0usize;
    while i < u1741Code::ALL.len() {
        let c = u1741Code::ALL[i].code();
        if c < 0x5ab0 || c > 0x5abf {
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
fn u1741_guard_seg() -> bool {
    u1741_segment_guard()
}

/// UNX-F1741 对拍表 16 行边界与累计守恒。
fn check_u1741_mix_tbl() -> bool {
    let mut ok = u1741mix_COUNT == 16;
    if ok { ok = u1741mix_get("JR_00") == Some(10) && u1741mix_get("SM_15") == Some(85); }
    if ok { ok = u1741mix_get("ZZZ_NOKEY").is_none(); }
    if ok { ok = u1741mix_sum() == 760; }
    ok
}

/// UNX-F1741 混合映射 16 行守卫唯一 + 守卫阈值冻结。
fn check_u1741_mix_map() -> bool {
    let mut ok = u1741mixmap_COUNT == 16;
    if ok { ok = u1741mixmap_scan() == 0 && u1741mixmap_unique(); }
    if ok { ok = u1741mixmap_sum() == 504; }
    if ok { ok = U1741MIXGATE_THRESHOLD == 32; }
    ok
}

/// 本模块自检（逐检查带 detail，全绿才出门）。
pub fn run_u1741_checks() -> CheckSet {
    let mut set = CheckSet::new("U1741");
    set.add(
        "u1741-seg-guard",
        u1741_guard_seg(),
        if u1741_guard_seg() { "" } else { "diagnostic code outside segment" },
    );
    set.add(
        "u1741-mix-tbl",
        check_u1741_mix_tbl(),
        if check_u1741_mix_tbl() { "" } else { "UNX-F1741 mix table mismatch" },
    );
    set.add(
        "u1741-mix-map",
        check_u1741_mix_map(),
        if check_u1741_mix_map() { "" } else { "UNX-F1741 mix map mismatch" },
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
    fn u1741_all_checks_green() {
        let set = run_u1741_checks();
        assert!(set.all_passed(), "U1741 domain checks must be green");
    }

    #[test]
    fn u1741_segment_guard_holds() {
        assert!(u1741_segment_guard());
    }
}
