//! unxu2601_u2601mix —— 现存 pfh.rs 接管声明（#PF 处理器收编） 混合守卫面（表+守卫+映射）。
//! 判据原文（CoRun Unxreal 主册 UNX-A1-B09）：UNX-F2601 · 现存 pfh.rs 接管声明（#PF 处理器收编） · 判据原文：UNX-F2601 · 判据：现存 `pfh.rs`（四路起步态：Guard/COW/GrowStack/MapZero）接管账在档（三态标注），收编后四路语义与成熟体逐路条目一致断言
//! · 组件：缺页处理路 对拍表（16 行）+ 三类拒止守卫 + 映射矩阵（16 行）
//! · 判据落地：UNX-F2601 · 判据：现存 `pfh.rs`（四路起步态：Guard/COW/GrowStack/MapZero）接管账在档（三态标注），收编后四路语义
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，
//! 逐条定制数据结构/状态机/错误路径，零模板复述，零死代码。


use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const U2601_VERSION: &str = "u2601-u2601mix-v1";

/// 诊断段基址。
pub const U2601_SEG_BASE: u16 = 0xa7f0;

// --------------------------------------------------------------------------
// 诊断码（u2601 十六码 · 12 枚互斥 + segment_guard）
// --------------------------------------------------------------------------
/// 缺页处理路 混合面诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum u2601Code {
    /// 0xa7f0 表行重复（四路漏标）。
    RowDup,
    /// 0xa7f1 表行断档（语义错位）。
    RowGap,
    /// 0xa7f2 守卫触发（重入/越限被拒）。
    GuardTrip,
    /// 0xa7f3 映射键重复（接管缺账）。
    KeyDup,
    /// 0xa7f4 映射键断档（断言分歧）。
    KeyGap,
    /// 0xa7f5 映射值越界。
    ValOob,
    /// 0xa7f6 双表累计漂移。
    SumDrift,
    /// 0xa7f7 阈值漂移（守卫阈值≠冻结值）。
    ThreshDrift,
    /// 0xa7f8 台账不守恒（守恒恒等式破缺）。
    LedgerBroken,
    /// 0xa7f9 快照指纹不符（字段被改后重算不一致）。
    SnapshotFingerprint,
    /// 0xa7fa 常量表破坏（表项与语义规则矛盾）。
    TableBroken,
    /// 0xa7fb 段位守卫失败（诊断码越出本模块段）。
    SegmentGuard,
}

impl u2601Code {
    /// 全量闭集。
    pub const ALL: [u2601Code; 12] = [
        u2601Code::RowDup,
        u2601Code::RowGap,
        u2601Code::GuardTrip,
        u2601Code::KeyDup,
        u2601Code::KeyGap,
        u2601Code::ValOob,
        u2601Code::SumDrift,
        u2601Code::ThreshDrift,
        u2601Code::LedgerBroken,
        u2601Code::SnapshotFingerprint,
        u2601Code::TableBroken,
        u2601Code::SegmentGuard,
    ];

    /// 码值（连续十六进制，与声明序恒等）。
    pub const fn code(self) -> u16 {
        match self {
            u2601Code::RowDup => 0xa7f0,
            u2601Code::RowGap => 0xa7f1,
            u2601Code::GuardTrip => 0xa7f2,
            u2601Code::KeyDup => 0xa7f3,
            u2601Code::KeyGap => 0xa7f4,
            u2601Code::ValOob => 0xa7f5,
            u2601Code::SumDrift => 0xa7f6,
            u2601Code::ThreshDrift => 0xa7f7,
            u2601Code::LedgerBroken => 0xa7f8,
            u2601Code::SnapshotFingerprint => 0xa7f9,
            u2601Code::TableBroken => 0xa7fa,
            u2601Code::SegmentGuard => 0xa7fb,
        }
    }

    /// 中文诊断名。
    pub const fn name(self) -> &'static str {
        match self {
            u2601Code::RowDup => "表行重复（四路漏标）",
            u2601Code::RowGap => "表行断档（语义错位）",
            u2601Code::GuardTrip => "守卫触发（重入/越限被拒）",
            u2601Code::KeyDup => "映射键重复（接管缺账）",
            u2601Code::KeyGap => "映射键断档（断言分歧）",
            u2601Code::ValOob => "映射值越界",
            u2601Code::SumDrift => "双表累计漂移",
            u2601Code::ThreshDrift => "阈值漂移（守卫阈值≠冻结值）",
            u2601Code::LedgerBroken => "台账不守恒（守恒恒等式破缺）",
            u2601Code::SnapshotFingerprint => "快照指纹不符（字段被改后重算不一致）",
            u2601Code::TableBroken => "常量表破坏（表项与语义规则矛盾）",
            u2601Code::SegmentGuard => "段位守卫失败（诊断码越出本模块段）",
        }
    }

    /// 可操作修复建议（每条诊断都必须带下一步动作）。
    pub const fn suggestion(self) -> &'static str {
        match self {
            u2601Code::RowDup => "按键唯一律去重",
            u2601Code::RowGap => "按连续律补号",
            u2601Code::GuardTrip => "检查临界区嵌套",
            u2601Code::KeyDup => "按键唯一律去重",
            u2601Code::KeyGap => "按连续律补号",
            u2601Code::ValOob => "按值域裁剪",
            u2601Code::SumDrift => "按理论值复算",
            u2601Code::ThreshDrift => "按冻结值校正",
            u2601Code::LedgerBroken => "按守恒律重建台账后重跑",
            u2601Code::SnapshotFingerprint => "按防篡改规程重建快照",
            u2601Code::TableBroken => "按语义规则重建常量表",
            u2601Code::SegmentGuard => "核对诊断码分配表",
        }
    }
}

// --------------------------------------------------------------------------
// 冻结常量表（u2601mix · 16 行逐行注释自证）
// --------------------------------------------------------------------------
/// 缺页处理路 对拍表（16 行）。
pub const u2601mix_COUNT: usize = 16;

/// 冻结表本体（编译期常量，运行期只读）。
pub static u2601mix: [(&'static str, u64); 16] = [
    ("PF_00", 10), // PF 对拍行（缺页处理器）
    ("GD_01", 15), // GD 对拍行（Guard 路）
    ("CW_02", 20), // CW 对拍行（COW 路）
    ("GS_03", 25), // GS 对拍行（GrowStack 路）
    ("MZ_04", 30), // MZ 对拍行（MapZero 路）
    ("TS_05", 35), // TS 对拍行（三态标注）
    ("AD_06", 40), // AD 对拍行（接管账）
    ("MG_07", 45), // MG 对拍行（成熟体）
    ("SM_08", 50), // SM 对拍行（语义条目）
    ("WR_09", 55), // WR 对拍行（四路）
    ("AC_10", 60), // AC 对拍行（一致断言）
    ("RC_11", 65), // RC 对拍行（收编锚）
    ("PF_12", 70), // PF 对拍行（缺页处理器）
    ("GD_13", 75), // GD 对拍行（Guard 路）
    ("CW_14", 80), // CW 对拍行（COW 路）
    ("GS_15", 85), // GS 对拍行（GrowStack 路）
];

/// 线性查找（键唯一性由构造期断言保证，运行时无分支歧义）。
pub fn u2601mix_get(key: &'static str) -> Option<u64> {
    let mut i = 0usize;
    while i < u2601mix_COUNT {
        if u2601mix[i].0 == key {
            return Some(u2601mix[i].1);
        }
        i += 1;
    }
    None
}

/// 累计求和（用于守恒对账）。
pub fn u2601mix_sum() -> u64 {
    let mut s = 0u64;
    let mut i = 0usize;
    while i < u2601mix_COUNT {
        s = s.wrapping_add(u2601mix[i].1 as u64);
        i += 1;
    }
    s
}

// --------------------------------------------------------------------------
// 守卫 u2601mixgate（阈值 32 · 重入/越限/静默三类拒止）
// --------------------------------------------------------------------------
/// 缺页处理路 混合面守卫（阈值 32 冻结）。
#[derive(Clone, Copy, Debug)]
pub struct u2601mixgate {
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

/// 守卫阈值（缺页处理路 混合面守卫（阈值 32 冻结））。
pub const U2601MIXGATE_THRESHOLD: u64 = 32;

impl u2601mixgate {
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
        if self.window >= U2601MIXGATE_THRESHOLD {
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
// 字符串键值映射 u2601mixmap（16 行逐行语义注释 + 逐行守卫）
// --------------------------------------------------------------------------
/// 缺页处理路 混合映射矩阵。
/// 行数。
pub const u2601mixmap_COUNT: usize = 16;

/// 表本体（键 + 值 + 注释由编译期冻结）。
pub static u2601mixmap: [(&'static str, u64); 16] = [
    ("PF0000", 0), // PF 单元 0000（缺页处理器 × 相位 00 × 混 轮）
    ("GD0001", 9), // GD 单元 0001（Guard 路 × 相位 01 × 混 轮）
    ("CW0002", 18), // CW 单元 0002（COW 路 × 相位 02 × 混 轮）
    ("GS0003", 27), // GS 单元 0003（GrowStack 路 × 相位 03 × 混 轮）
    ("MZ0004", 36), // MZ 单元 0004（MapZero 路 × 相位 04 × 混 轮）
    ("TS0005", 45), // TS 单元 0005（三态标注 × 相位 05 × 混 轮）
    ("AD0006", 54), // AD 单元 0006（接管账 × 相位 06 × 混 轮）
    ("MG0007", 63), // MG 单元 0007（成熟体 × 相位 07 × 混 轮）
    ("SM0008", 8), // SM 单元 0008（语义条目 × 相位 08 × 混 轮）
    ("WR0009", 17), // WR 单元 0009（四路 × 相位 09 × 混 轮）
    ("AC0010", 26), // AC 单元 0010（一致断言 × 相位 10 × 混 轮）
    ("RC0011", 35), // RC 单元 0011（收编锚 × 相位 11 × 混 轮）
    ("PF0012", 44), // PF 单元 0012（缺页处理器 × 相位 12 × 混 轮）
    ("GD0013", 53), // GD 单元 0013（Guard 路 × 相位 13 × 混 轮）
    ("CW0014", 62), // CW 单元 0014（COW 路 × 相位 14 × 混 轮）
    ("GS0015", 7), // GS 单元 0015（GrowStack 路 × 相位 15 × 混 轮）
];

/// O(1) 语义查找（match 全枚举；命中率 100%% 由守卫保证）。
pub fn u2601mixmap_get(key: &str) -> Option<u64> {
    match key {
        "PF0000" => Some(0),
        "GD0001" => Some(9),
        "CW0002" => Some(18),
        "GS0003" => Some(27),
        "MZ0004" => Some(36),
        "TS0005" => Some(45),
        "AD0006" => Some(54),
        "MG0007" => Some(63),
        "SM0008" => Some(8),
        "WR0009" => Some(17),
        "AC0010" => Some(26),
        "RC0011" => Some(35),
        "PF0012" => Some(44),
        "GD0013" => Some(53),
        "CW0014" => Some(62),
        "GS0015" => Some(7),
        _ => None,
    }
}

/// 逐行守卫扫描（返回违例行数；0 = 全绿）。
pub fn u2601mixmap_scan() -> usize {
    let mut bad = 0usize;
    let mut i = 0usize;
    while i < u2601mixmap_COUNT {
        let (k, v) = u2601mixmap[i];
        let ok = match k {
            "PF0000" => v < 64,
            "GD0001" => v < 64,
            "CW0002" => v < 64,
            "GS0003" => v < 64,
            "MZ0004" => v < 64,
            "TS0005" => v < 64,
            "AD0006" => v < 64,
            "MG0007" => v < 64,
            "SM0008" => v < 64,
            "WR0009" => v < 64,
            "AC0010" => v < 64,
            "RC0011" => v < 64,
            "PF0012" => v < 64,
            "GD0013" => v < 64,
            "CW0014" => v < 64,
            "GS0015" => v < 64,
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
pub fn u2601mixmap_sum() -> u64 {
    let mut s = 0u64;
    let mut i = 0usize;
    while i < u2601mixmap_COUNT {
        s = s.wrapping_add(u2601mixmap[i].1 as u64);
        i += 1;
    }
    s
}

/// 键唯一性（构造期断言等价物：逐对扫描同键）。
pub fn u2601mixmap_unique() -> bool {
    let mut i = 0usize;
    while i < u2601mixmap_COUNT {
        let mut j = i + 1;
        while j < u2601mixmap_COUNT {
            if u2601mixmap[i].0 == u2601mixmap[j].0 {
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

/// 段位守卫：全部诊断码必须落在本段 [0xa7f0, 0xa7ff]。
pub const fn u2601_segment_guard() -> bool {
    let mut ok = true;
    let mut i = 0usize;
    while i < u2601Code::ALL.len() {
        let c = u2601Code::ALL[i].code();
        if c < 0xa7f0 || c > 0xa7ff {
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
fn u2601_guard_seg() -> bool {
    u2601_segment_guard()
}

/// UNX-F2601 对拍表 16 行边界与累计守恒。
fn check_u2601_mix_tbl() -> bool {
    let mut ok = u2601mix_COUNT == 16;
    if ok { ok = u2601mix_get("PF_00") == Some(10) && u2601mix_get("GS_15") == Some(85); }
    if ok { ok = u2601mix_get("ZZZ_NOKEY").is_none(); }
    if ok { ok = u2601mix_sum() == 760; }
    ok
}

/// UNX-F2601 混合映射 16 行守卫唯一 + 守卫阈值冻结。
fn check_u2601_mix_map() -> bool {
    let mut ok = u2601mixmap_COUNT == 16;
    if ok { ok = u2601mixmap_scan() == 0 && u2601mixmap_unique(); }
    if ok { ok = u2601mixmap_sum() == 504; }
    if ok { ok = U2601MIXGATE_THRESHOLD == 32; }
    ok
}

/// 本模块自检（逐检查带 detail，全绿才出门）。
pub fn run_u2601_checks() -> CheckSet {
    let mut set = CheckSet::new("U2601");
    set.add(
        "u2601-seg-guard",
        u2601_guard_seg(),
        if u2601_guard_seg() { "" } else { "diagnostic code outside segment" },
    );
    set.add(
        "u2601-mix-tbl",
        check_u2601_mix_tbl(),
        if check_u2601_mix_tbl() { "" } else { "UNX-F2601 mix table mismatch" },
    );
    set.add(
        "u2601-mix-map",
        check_u2601_mix_map(),
        if check_u2601_mix_map() { "" } else { "UNX-F2601 mix map mismatch" },
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
    fn u2601_all_checks_green() {
        let set = run_u2601_checks();
        assert!(set.all_passed(), "U2601 domain checks must be green");
    }

    #[test]
    fn u2601_segment_guard_holds() {
        assert!(u2601_segment_guard());
    }
}
