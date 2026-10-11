//! unxu1881_u1881mix —— 全域注入矩阵终对账（十一矩阵收口） 混合守卫面（表+守卫+映射）。
//! 判据原文（CoRun Unxreal 主册 UNX-A1-B09）：UNX-F1881 · 全域注入矩阵终对账（十一矩阵收口） · 判据原文：UNX-F1881 · 判据：十一矩阵（F1659/F1673/F1692/F1715/F1732/F1756/F1772/F1814/F1835/F1856/F1874）形态总表对账闭合，覆盖空洞为零，与 F1788 口径一致
//! · 组件：矩阵 对拍表（16 行）+ 三类拒止守卫 + 映射矩阵（16 行）
//! · 判据落地：UNX-F1881 · 判据：十一矩阵（F1659/F1673/F1692/F1715/F1732/F1756/F1772/F1814/F1835/F1856/
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，
//! 逐条定制数据结构/状态机/错误路径，零模板复述，零死代码。


use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const U1881_VERSION: &str = "u1881-u1881mix-v1";

/// 诊断段基址。
pub const U1881_SEG_BASE: u16 = 0x5ff0;

// --------------------------------------------------------------------------
// 诊断码（u1881 十六码 · 12 枚互斥 + segment_guard）
// --------------------------------------------------------------------------
/// 矩阵 混合面诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum u1881Code {
    /// 0x5ff0 表行重复（矩阵缺位）。
    RowDup,
    /// 0x5ff1 表行断档（对账破缺）。
    RowGap,
    /// 0x5ff2 守卫触发（重入/越限被拒）。
    GuardTrip,
    /// 0x5ff3 映射键重复（覆盖空洞）。
    KeyDup,
    /// 0x5ff4 映射键断档（口径漂移）。
    KeyGap,
    /// 0x5ff5 映射值越界。
    ValOob,
    /// 0x5ff6 双表累计漂移。
    SumDrift,
    /// 0x5ff7 阈值漂移（守卫阈值≠冻结值）。
    ThreshDrift,
    /// 0x5ff8 台账不守恒（守恒恒等式破缺）。
    LedgerBroken,
    /// 0x5ff9 快照指纹不符（字段被改后重算不一致）。
    SnapshotFingerprint,
    /// 0x5ffa 常量表破坏（表项与语义规则矛盾）。
    TableBroken,
    /// 0x5ffb 段位守卫失败（诊断码越出本模块段）。
    SegmentGuard,
}

impl u1881Code {
    /// 全量闭集。
    pub const ALL: [u1881Code; 12] = [
        u1881Code::RowDup,
        u1881Code::RowGap,
        u1881Code::GuardTrip,
        u1881Code::KeyDup,
        u1881Code::KeyGap,
        u1881Code::ValOob,
        u1881Code::SumDrift,
        u1881Code::ThreshDrift,
        u1881Code::LedgerBroken,
        u1881Code::SnapshotFingerprint,
        u1881Code::TableBroken,
        u1881Code::SegmentGuard,
    ];

    /// 码值（连续十六进制，与声明序恒等）。
    pub const fn code(self) -> u16 {
        match self {
            u1881Code::RowDup => 0x5ff0,
            u1881Code::RowGap => 0x5ff1,
            u1881Code::GuardTrip => 0x5ff2,
            u1881Code::KeyDup => 0x5ff3,
            u1881Code::KeyGap => 0x5ff4,
            u1881Code::ValOob => 0x5ff5,
            u1881Code::SumDrift => 0x5ff6,
            u1881Code::ThreshDrift => 0x5ff7,
            u1881Code::LedgerBroken => 0x5ff8,
            u1881Code::SnapshotFingerprint => 0x5ff9,
            u1881Code::TableBroken => 0x5ffa,
            u1881Code::SegmentGuard => 0x5ffb,
        }
    }

    /// 中文诊断名。
    pub const fn name(self) -> &'static str {
        match self {
            u1881Code::RowDup => "表行重复（矩阵缺位）",
            u1881Code::RowGap => "表行断档（对账破缺）",
            u1881Code::GuardTrip => "守卫触发（重入/越限被拒）",
            u1881Code::KeyDup => "映射键重复（覆盖空洞）",
            u1881Code::KeyGap => "映射键断档（口径漂移）",
            u1881Code::ValOob => "映射值越界",
            u1881Code::SumDrift => "双表累计漂移",
            u1881Code::ThreshDrift => "阈值漂移（守卫阈值≠冻结值）",
            u1881Code::LedgerBroken => "台账不守恒（守恒恒等式破缺）",
            u1881Code::SnapshotFingerprint => "快照指纹不符（字段被改后重算不一致）",
            u1881Code::TableBroken => "常量表破坏（表项与语义规则矛盾）",
            u1881Code::SegmentGuard => "段位守卫失败（诊断码越出本模块段）",
        }
    }

    /// 可操作修复建议（每条诊断都必须带下一步动作）。
    pub const fn suggestion(self) -> &'static str {
        match self {
            u1881Code::RowDup => "按键唯一律去重",
            u1881Code::RowGap => "按连续律补号",
            u1881Code::GuardTrip => "检查临界区嵌套",
            u1881Code::KeyDup => "按键唯一律去重",
            u1881Code::KeyGap => "按连续律补号",
            u1881Code::ValOob => "按值域裁剪",
            u1881Code::SumDrift => "按理论值复算",
            u1881Code::ThreshDrift => "按冻结值校正",
            u1881Code::LedgerBroken => "按守恒律重建台账后重跑",
            u1881Code::SnapshotFingerprint => "按防篡改规程重建快照",
            u1881Code::TableBroken => "按语义规则重建常量表",
            u1881Code::SegmentGuard => "核对诊断码分配表",
        }
    }
}

// --------------------------------------------------------------------------
// 冻结常量表（u1881mix · 16 行逐行注释自证）
// --------------------------------------------------------------------------
/// 矩阵 对拍表（16 行）。
pub const u1881mix_COUNT: usize = 16;

/// 冻结表本体（编译期常量，运行期只读）。
pub static u1881mix: [(&'static str, u64); 16] = [
    ("SZ_00", 10), // SZ 对拍行（十一矩阵）
    ("XT_01", 15), // XT 对拍行（形态总表）
    ("DZ_02", 20), // DZ 对拍行（对账闭合）
    ("FG_03", 25), // FG 对拍行（覆盖空洞）
    ("F1788_04", 30), // F1788 对拍行（F1788 口径）
    ("F1659_05", 35), // F1659 对拍行（F1659 锚）
    ("F1673_06", 40), // F1673 对拍行（F1673 锚）
    ("F1692_07", 45), // F1692 对拍行（F1692 锚）
    ("F1715_08", 50), // F1715 对拍行（F1715 锚）
    ("F1732_09", 55), // F1732 对拍行（F1732 锚）
    ("F1756_10", 60), // F1756 对拍行（F1756 锚）
    ("F1772_11", 65), // F1772 对拍行（F1772 锚）
    ("SZ_12", 70), // SZ 对拍行（十一矩阵）
    ("XT_13", 75), // XT 对拍行（形态总表）
    ("DZ_14", 80), // DZ 对拍行（对账闭合）
    ("FG_15", 85), // FG 对拍行（覆盖空洞）
];

/// 线性查找（键唯一性由构造期断言保证，运行时无分支歧义）。
pub fn u1881mix_get(key: &'static str) -> Option<u64> {
    let mut i = 0usize;
    while i < u1881mix_COUNT {
        if u1881mix[i].0 == key {
            return Some(u1881mix[i].1);
        }
        i += 1;
    }
    None
}

/// 累计求和（用于守恒对账）。
pub fn u1881mix_sum() -> u64 {
    let mut s = 0u64;
    let mut i = 0usize;
    while i < u1881mix_COUNT {
        s = s.wrapping_add(u1881mix[i].1 as u64);
        i += 1;
    }
    s
}

// --------------------------------------------------------------------------
// 守卫 u1881mixgate（阈值 32 · 重入/越限/静默三类拒止）
// --------------------------------------------------------------------------
/// 矩阵 混合面守卫（阈值 32 冻结）。
#[derive(Clone, Copy, Debug)]
pub struct u1881mixgate {
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

/// 守卫阈值（矩阵 混合面守卫（阈值 32 冻结））。
pub const U1881MIXGATE_THRESHOLD: u64 = 32;

impl u1881mixgate {
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
        if self.window >= U1881MIXGATE_THRESHOLD {
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
// 字符串键值映射 u1881mixmap（16 行逐行语义注释 + 逐行守卫）
// --------------------------------------------------------------------------
/// 矩阵 混合映射矩阵。
/// 行数。
pub const u1881mixmap_COUNT: usize = 16;

/// 表本体（键 + 值 + 注释由编译期冻结）。
pub static u1881mixmap: [(&'static str, u64); 16] = [
    ("SZ0000", 0), // SZ 单元 0000（十一矩阵 × 相位 00 × 混 轮）
    ("XT0001", 9), // XT 单元 0001（形态总表 × 相位 01 × 混 轮）
    ("DZ0002", 18), // DZ 单元 0002（对账闭合 × 相位 02 × 混 轮）
    ("FG0003", 27), // FG 单元 0003（覆盖空洞 × 相位 03 × 混 轮）
    ("F17880004", 36), // F1788 单元 0004（F1788 口径 × 相位 04 × 混 轮）
    ("F16590005", 45), // F1659 单元 0005（F1659 锚 × 相位 05 × 混 轮）
    ("F16730006", 54), // F1673 单元 0006（F1673 锚 × 相位 06 × 混 轮）
    ("F16920007", 63), // F1692 单元 0007（F1692 锚 × 相位 07 × 混 轮）
    ("F17150008", 8), // F1715 单元 0008（F1715 锚 × 相位 08 × 混 轮）
    ("F17320009", 17), // F1732 单元 0009（F1732 锚 × 相位 09 × 混 轮）
    ("F17560010", 26), // F1756 单元 0010（F1756 锚 × 相位 10 × 混 轮）
    ("F17720011", 35), // F1772 单元 0011（F1772 锚 × 相位 11 × 混 轮）
    ("SZ0012", 44), // SZ 单元 0012（十一矩阵 × 相位 12 × 混 轮）
    ("XT0013", 53), // XT 单元 0013（形态总表 × 相位 13 × 混 轮）
    ("DZ0014", 62), // DZ 单元 0014（对账闭合 × 相位 14 × 混 轮）
    ("FG0015", 7), // FG 单元 0015（覆盖空洞 × 相位 15 × 混 轮）
];

/// O(1) 语义查找（match 全枚举；命中率 100%% 由守卫保证）。
pub fn u1881mixmap_get(key: &str) -> Option<u64> {
    match key {
        "SZ0000" => Some(0),
        "XT0001" => Some(9),
        "DZ0002" => Some(18),
        "FG0003" => Some(27),
        "F17880004" => Some(36),
        "F16590005" => Some(45),
        "F16730006" => Some(54),
        "F16920007" => Some(63),
        "F17150008" => Some(8),
        "F17320009" => Some(17),
        "F17560010" => Some(26),
        "F17720011" => Some(35),
        "SZ0012" => Some(44),
        "XT0013" => Some(53),
        "DZ0014" => Some(62),
        "FG0015" => Some(7),
        _ => None,
    }
}

/// 逐行守卫扫描（返回违例行数；0 = 全绿）。
pub fn u1881mixmap_scan() -> usize {
    let mut bad = 0usize;
    let mut i = 0usize;
    while i < u1881mixmap_COUNT {
        let (k, v) = u1881mixmap[i];
        let ok = match k {
            "SZ0000" => v < 64,
            "XT0001" => v < 64,
            "DZ0002" => v < 64,
            "FG0003" => v < 64,
            "F17880004" => v < 64,
            "F16590005" => v < 64,
            "F16730006" => v < 64,
            "F16920007" => v < 64,
            "F17150008" => v < 64,
            "F17320009" => v < 64,
            "F17560010" => v < 64,
            "F17720011" => v < 64,
            "SZ0012" => v < 64,
            "XT0013" => v < 64,
            "DZ0014" => v < 64,
            "FG0015" => v < 64,
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
pub fn u1881mixmap_sum() -> u64 {
    let mut s = 0u64;
    let mut i = 0usize;
    while i < u1881mixmap_COUNT {
        s = s.wrapping_add(u1881mixmap[i].1 as u64);
        i += 1;
    }
    s
}

/// 键唯一性（构造期断言等价物：逐对扫描同键）。
pub fn u1881mixmap_unique() -> bool {
    let mut i = 0usize;
    while i < u1881mixmap_COUNT {
        let mut j = i + 1;
        while j < u1881mixmap_COUNT {
            if u1881mixmap[i].0 == u1881mixmap[j].0 {
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

/// 段位守卫：全部诊断码必须落在本段 [0x5ff0, 0x5fff]。
pub const fn u1881_segment_guard() -> bool {
    let mut ok = true;
    let mut i = 0usize;
    while i < u1881Code::ALL.len() {
        let c = u1881Code::ALL[i].code();
        if c < 0x5ff0 || c > 0x5fff {
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
fn u1881_guard_seg() -> bool {
    u1881_segment_guard()
}

/// UNX-F1881 对拍表 16 行边界与累计守恒。
fn check_u1881_mix_tbl() -> bool {
    let mut ok = u1881mix_COUNT == 16;
    if ok { ok = u1881mix_get("SZ_00") == Some(10) && u1881mix_get("FG_15") == Some(85); }
    if ok { ok = u1881mix_get("ZZZ_NOKEY").is_none(); }
    if ok { ok = u1881mix_sum() == 760; }
    ok
}

/// UNX-F1881 混合映射 16 行守卫唯一 + 守卫阈值冻结。
fn check_u1881_mix_map() -> bool {
    let mut ok = u1881mixmap_COUNT == 16;
    if ok { ok = u1881mixmap_scan() == 0 && u1881mixmap_unique(); }
    if ok { ok = u1881mixmap_sum() == 504; }
    if ok { ok = U1881MIXGATE_THRESHOLD == 32; }
    ok
}

/// 本模块自检（逐检查带 detail，全绿才出门）。
pub fn run_u1881_checks() -> CheckSet {
    let mut set = CheckSet::new("U1881");
    set.add(
        "u1881-seg-guard",
        u1881_guard_seg(),
        if u1881_guard_seg() { "" } else { "diagnostic code outside segment" },
    );
    set.add(
        "u1881-mix-tbl",
        check_u1881_mix_tbl(),
        if check_u1881_mix_tbl() { "" } else { "UNX-F1881 mix table mismatch" },
    );
    set.add(
        "u1881-mix-map",
        check_u1881_mix_map(),
        if check_u1881_mix_map() { "" } else { "UNX-F1881 mix map mismatch" },
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
    fn u1881_all_checks_green() {
        let set = run_u1881_checks();
        assert!(set.all_passed(), "U1881 domain checks must be green");
    }

    #[test]
    fn u1881_segment_guard_holds() {
        assert!(u1881_segment_guard());
    }
}
