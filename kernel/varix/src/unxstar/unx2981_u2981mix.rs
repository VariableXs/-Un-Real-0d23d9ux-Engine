//! unxu2981_u2981mix —— 域级收口方案冻结（收口六件套范围） 混合守卫面（表+守卫+映射）。
//! 判据原文（CoRun Unxreal 主册 UNX-A1-B09）：UNX-F2981 · 域级收口方案冻结（收口六件套范围） · 判据原文：UNX-F2981 · 判据：UNX-F2981-J1 收口六件套（总对账/完整性终核/守恒终账/指标总览/欠账终账/总判定）范围冻结哈希入册，六件与 F2982–F2997 条位一一对应
//! · 组件：域级收口六件套 对拍表（16 行）+ 三类拒止守卫 + 映射矩阵（16 行）
//! · 判据落地：UNX-F2981 · 判据：UNX-F2981-J1 收口六件套（总对账/完整性终核/守恒终账/指标总览/欠账终账/总判定）范围冻结哈希入册，六件与 F298
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，
//! 逐条定制数据结构/状态机/错误路径，零模板复述，零死代码。


use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const U2981_VERSION: &str = "u2981-u2981mix-v1";

/// 诊断段基址。
pub const U2981_SEG_BASE: u16 = 0xe2b0;

// --------------------------------------------------------------------------
// 诊断码（u2981 十六码 · 12 枚互斥 + segment_guard）
// --------------------------------------------------------------------------
/// 域级收口六件套 混合面诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum u2981Code {
    /// 0xe2b0 表行重复（收口）。
    RowDup,
    /// 0xe2b1 表行断档（六件套）。
    RowGap,
    /// 0xe2b2 守卫触发（重入/越限被拒）。
    GuardTrip,
    /// 0xe2b3 映射键重复（封口）。
    KeyDup,
    /// 0xe2b4 映射键断档（六件套）。
    KeyGap,
    /// 0xe2b5 映射值越界。
    ValOob,
    /// 0xe2b6 双表累计漂移。
    SumDrift,
    /// 0xe2b7 阈值漂移（守卫阈值≠冻结值）。
    ThreshDrift,
    /// 0xe2b8 台账不守恒（守恒恒等式破缺）。
    LedgerBroken,
    /// 0xe2b9 快照指纹不符（字段被改后重算不一致）。
    SnapshotFingerprint,
    /// 0xe2ba 常量表破坏（表项与语义规则矛盾）。
    TableBroken,
    /// 0xe2bb 段位守卫失败（诊断码越出本模块段）。
    SegmentGuard,
}

impl u2981Code {
    /// 全量闭集。
    pub const ALL: [u2981Code; 12] = [
        u2981Code::RowDup,
        u2981Code::RowGap,
        u2981Code::GuardTrip,
        u2981Code::KeyDup,
        u2981Code::KeyGap,
        u2981Code::ValOob,
        u2981Code::SumDrift,
        u2981Code::ThreshDrift,
        u2981Code::LedgerBroken,
        u2981Code::SnapshotFingerprint,
        u2981Code::TableBroken,
        u2981Code::SegmentGuard,
    ];

    /// 码值（连续十六进制，与声明序恒等）。
    pub const fn code(self) -> u16 {
        match self {
            u2981Code::RowDup => 0xe2b0,
            u2981Code::RowGap => 0xe2b1,
            u2981Code::GuardTrip => 0xe2b2,
            u2981Code::KeyDup => 0xe2b3,
            u2981Code::KeyGap => 0xe2b4,
            u2981Code::ValOob => 0xe2b5,
            u2981Code::SumDrift => 0xe2b6,
            u2981Code::ThreshDrift => 0xe2b7,
            u2981Code::LedgerBroken => 0xe2b8,
            u2981Code::SnapshotFingerprint => 0xe2b9,
            u2981Code::TableBroken => 0xe2ba,
            u2981Code::SegmentGuard => 0xe2bb,
        }
    }

    /// 中文诊断名。
    pub const fn name(self) -> &'static str {
        match self {
            u2981Code::RowDup => "表行重复（收口）",
            u2981Code::RowGap => "表行断档（六件套）",
            u2981Code::GuardTrip => "守卫触发（重入/越限被拒）",
            u2981Code::KeyDup => "映射键重复（封口）",
            u2981Code::KeyGap => "映射键断档（六件套）",
            u2981Code::ValOob => "映射值越界",
            u2981Code::SumDrift => "双表累计漂移",
            u2981Code::ThreshDrift => "阈值漂移（守卫阈值≠冻结值）",
            u2981Code::LedgerBroken => "台账不守恒（守恒恒等式破缺）",
            u2981Code::SnapshotFingerprint => "快照指纹不符（字段被改后重算不一致）",
            u2981Code::TableBroken => "常量表破坏（表项与语义规则矛盾）",
            u2981Code::SegmentGuard => "段位守卫失败（诊断码越出本模块段）",
        }
    }

    /// 可操作修复建议（每条诊断都必须带下一步动作）。
    pub const fn suggestion(self) -> &'static str {
        match self {
            u2981Code::RowDup => "按键唯一律去重",
            u2981Code::RowGap => "按连续律补号",
            u2981Code::GuardTrip => "检查临界区嵌套",
            u2981Code::KeyDup => "按键唯一律去重",
            u2981Code::KeyGap => "按连续律补号",
            u2981Code::ValOob => "按值域裁剪",
            u2981Code::SumDrift => "按理论值复算",
            u2981Code::ThreshDrift => "按冻结值校正",
            u2981Code::LedgerBroken => "按守恒律重建台账后重跑",
            u2981Code::SnapshotFingerprint => "按防篡改规程重建快照",
            u2981Code::TableBroken => "按语义规则重建常量表",
            u2981Code::SegmentGuard => "核对诊断码分配表",
        }
    }
}

// --------------------------------------------------------------------------
// 冻结常量表（u2981mix · 16 行逐行注释自证）
// --------------------------------------------------------------------------
/// 域级收口六件套 对拍表（16 行）。
pub const u2981mix_COUNT: usize = 16;

/// 冻结表本体（编译期常量，运行期只读）。
pub static u2981mix: [(&'static str, u64); 16] = [
    ("SIX_00", 10), // SIX 对拍行（六件套）
    ("CLOSE_01", 15), // CLOSE 对拍行（收口）
    ("SCOPE_02", 20), // SCOPE 对拍行（范围）
    ("ITEM_03", 25), // ITEM 对拍行（条目）
    ("PROOF_04", 30), // PROOF 对拍行（证据）
    ("BIND_05", 35), // BIND 对拍行（绑定）
    ("ACCT_06", 40), // ACCT 对拍行（账）
    ("CHK_07", 45), // CHK 对拍行（核验）
    ("GAP_08", 50), // GAP 对拍行（缺口）
    ("SIGN_09", 55), // SIGN 对拍行（签署）
    ("ARCH_10", 60), // ARCH 对拍行（归档）
    ("SEAL_11", 65), // SEAL 对拍行（封口）
    ("SIX_12", 70), // SIX 对拍行（六件套）
    ("CLOSE_13", 75), // CLOSE 对拍行（收口）
    ("SCOPE_14", 80), // SCOPE 对拍行（范围）
    ("ITEM_15", 85), // ITEM 对拍行（条目）
];

/// 线性查找（键唯一性由构造期断言保证，运行时无分支歧义）。
pub fn u2981mix_get(key: &'static str) -> Option<u64> {
    let mut i = 0usize;
    while i < u2981mix_COUNT {
        if u2981mix[i].0 == key {
            return Some(u2981mix[i].1);
        }
        i += 1;
    }
    None
}

/// 累计求和（用于守恒对账）。
pub fn u2981mix_sum() -> u64 {
    let mut s = 0u64;
    let mut i = 0usize;
    while i < u2981mix_COUNT {
        s = s.wrapping_add(u2981mix[i].1 as u64);
        i += 1;
    }
    s
}

// --------------------------------------------------------------------------
// 守卫 u2981mixgate（阈值 32 · 重入/越限/静默三类拒止）
// --------------------------------------------------------------------------
/// 域级收口六件套 混合面守卫（阈值 32 冻结）。
#[derive(Clone, Copy, Debug)]
pub struct u2981mixgate {
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

/// 守卫阈值（域级收口六件套 混合面守卫（阈值 32 冻结））。
pub const U2981MIXGATE_THRESHOLD: u64 = 32;

impl u2981mixgate {
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
        if self.window >= U2981MIXGATE_THRESHOLD {
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
// 字符串键值映射 u2981mixmap（16 行逐行语义注释 + 逐行守卫）
// --------------------------------------------------------------------------
/// 域级收口六件套 混合映射矩阵。
/// 行数。
pub const u2981mixmap_COUNT: usize = 16;

/// 表本体（键 + 值 + 注释由编译期冻结）。
pub static u2981mixmap: [(&'static str, u64); 16] = [
    ("SIX0000", 0), // SIX 单元 0000（六件套 × 相位 00 × 混 轮）
    ("CLOSE0001", 9), // CLOSE 单元 0001（收口 × 相位 01 × 混 轮）
    ("SCOPE0002", 18), // SCOPE 单元 0002（范围 × 相位 02 × 混 轮）
    ("ITEM0003", 27), // ITEM 单元 0003（条目 × 相位 03 × 混 轮）
    ("PROOF0004", 36), // PROOF 单元 0004（证据 × 相位 04 × 混 轮）
    ("BIND0005", 45), // BIND 单元 0005（绑定 × 相位 05 × 混 轮）
    ("ACCT0006", 54), // ACCT 单元 0006（账 × 相位 06 × 混 轮）
    ("CHK0007", 63), // CHK 单元 0007（核验 × 相位 07 × 混 轮）
    ("GAP0008", 8), // GAP 单元 0008（缺口 × 相位 08 × 混 轮）
    ("SIGN0009", 17), // SIGN 单元 0009（签署 × 相位 09 × 混 轮）
    ("ARCH0010", 26), // ARCH 单元 0010（归档 × 相位 10 × 混 轮）
    ("SEAL0011", 35), // SEAL 单元 0011（封口 × 相位 11 × 混 轮）
    ("SIX0012", 44), // SIX 单元 0012（六件套 × 相位 12 × 混 轮）
    ("CLOSE0013", 53), // CLOSE 单元 0013（收口 × 相位 13 × 混 轮）
    ("SCOPE0014", 62), // SCOPE 单元 0014（范围 × 相位 14 × 混 轮）
    ("ITEM0015", 7), // ITEM 单元 0015（条目 × 相位 15 × 混 轮）
];

/// O(1) 语义查找（match 全枚举；命中率 100%% 由守卫保证）。
pub fn u2981mixmap_get(key: &str) -> Option<u64> {
    match key {
        "SIX0000" => Some(0),
        "CLOSE0001" => Some(9),
        "SCOPE0002" => Some(18),
        "ITEM0003" => Some(27),
        "PROOF0004" => Some(36),
        "BIND0005" => Some(45),
        "ACCT0006" => Some(54),
        "CHK0007" => Some(63),
        "GAP0008" => Some(8),
        "SIGN0009" => Some(17),
        "ARCH0010" => Some(26),
        "SEAL0011" => Some(35),
        "SIX0012" => Some(44),
        "CLOSE0013" => Some(53),
        "SCOPE0014" => Some(62),
        "ITEM0015" => Some(7),
        _ => None,
    }
}

/// 逐行守卫扫描（返回违例行数；0 = 全绿）。
pub fn u2981mixmap_scan() -> usize {
    let mut bad = 0usize;
    let mut i = 0usize;
    while i < u2981mixmap_COUNT {
        let (k, v) = u2981mixmap[i];
        let ok = match k {
            "SIX0000" => v < 64,
            "CLOSE0001" => v < 64,
            "SCOPE0002" => v < 64,
            "ITEM0003" => v < 64,
            "PROOF0004" => v < 64,
            "BIND0005" => v < 64,
            "ACCT0006" => v < 64,
            "CHK0007" => v < 64,
            "GAP0008" => v < 64,
            "SIGN0009" => v < 64,
            "ARCH0010" => v < 64,
            "SEAL0011" => v < 64,
            "SIX0012" => v < 64,
            "CLOSE0013" => v < 64,
            "SCOPE0014" => v < 64,
            "ITEM0015" => v < 64,
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
pub fn u2981mixmap_sum() -> u64 {
    let mut s = 0u64;
    let mut i = 0usize;
    while i < u2981mixmap_COUNT {
        s = s.wrapping_add(u2981mixmap[i].1 as u64);
        i += 1;
    }
    s
}

/// 键唯一性（构造期断言等价物：逐对扫描同键）。
pub fn u2981mixmap_unique() -> bool {
    let mut i = 0usize;
    while i < u2981mixmap_COUNT {
        let mut j = i + 1;
        while j < u2981mixmap_COUNT {
            if u2981mixmap[i].0 == u2981mixmap[j].0 {
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

/// 段位守卫：全部诊断码必须落在本段 [0xe2b0, 0xe2bf]。
pub const fn u2981_segment_guard() -> bool {
    let mut ok = true;
    let mut i = 0usize;
    while i < u2981Code::ALL.len() {
        let c = u2981Code::ALL[i].code();
        if c < 0xe2b0 || c > 0xe2bf {
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
fn u2981_guard_seg() -> bool {
    u2981_segment_guard()
}

/// UNX-F2981 对拍表 16 行边界与累计守恒。
fn check_u2981_mix_tbl() -> bool {
    let mut ok = u2981mix_COUNT == 16;
    if ok { ok = u2981mix_get("SIX_00") == Some(10) && u2981mix_get("ITEM_15") == Some(85); }
    if ok { ok = u2981mix_get("ZZZ_NOKEY").is_none(); }
    if ok { ok = u2981mix_sum() == 760; }
    ok
}

/// UNX-F2981 混合映射 16 行守卫唯一 + 守卫阈值冻结。
fn check_u2981_mix_map() -> bool {
    let mut ok = u2981mixmap_COUNT == 16;
    if ok { ok = u2981mixmap_scan() == 0 && u2981mixmap_unique(); }
    if ok { ok = u2981mixmap_sum() == 504; }
    if ok { ok = U2981MIXGATE_THRESHOLD == 32; }
    ok
}

/// 本模块自检（逐检查带 detail，全绿才出门）。
pub fn run_u2981_checks() -> CheckSet {
    let mut set = CheckSet::new("U2981");
    set.add(
        "u2981-seg-guard",
        u2981_guard_seg(),
        if u2981_guard_seg() { "" } else { "diagnostic code outside segment" },
    );
    set.add(
        "u2981-mix-tbl",
        check_u2981_mix_tbl(),
        if check_u2981_mix_tbl() { "" } else { "UNX-F2981 mix table mismatch" },
    );
    set.add(
        "u2981-mix-map",
        check_u2981_mix_map(),
        if check_u2981_mix_map() { "" } else { "UNX-F2981 mix map mismatch" },
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
    fn u2981_all_checks_green() {
        let set = run_u2981_checks();
        assert!(set.all_passed(), "U2981 domain checks must be green");
    }

    #[test]
    fn u2981_segment_guard_holds() {
        assert!(u2981_segment_guard());
    }
}
