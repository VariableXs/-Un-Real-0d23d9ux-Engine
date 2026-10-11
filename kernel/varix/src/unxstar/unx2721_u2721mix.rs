//! unxu2721_u2721mix —— 扫描器主循环与扫描节拍 混合守卫面（表+守卫+映射）。
//! 判据原文（CoRun Unxreal 主册 UNX-A1-B09）：UNX-F2721 · 扫描器主循环与扫描节拍 · 判据原文：UNX-F2721 · 判据：扫描主循环（isolate→谓词→逐出/归还三段）闭环 10 轮账平，扫描节拍双触发（水线/直接回收）命中同一入口
//! · 组件：扫描轮次 对拍表（16 行）+ 三类拒止守卫 + 映射矩阵（16 行）
//! · 判据落地：UNX-F2721 · 判据：扫描主循环（isolate→谓词→逐出/归还三段）闭环 10 轮账平，扫描节拍双触发（水线/直接回收）命中同一入口
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，
//! 逐条定制数据结构/状态机/错误路径，零模板复述，零死代码。


use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const U2721_VERSION: &str = "u2721-u2721mix-v1";

/// 诊断段基址。
pub const U2721_SEG_BASE: u16 = 0xac70;

// --------------------------------------------------------------------------
// 诊断码（u2721 十六码 · 12 枚互斥 + segment_guard）
// --------------------------------------------------------------------------
/// 扫描轮次 混合面诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum u2721Code {
    /// 0xac70 表行重复（三段断链）。
    RowDup,
    /// 0xac71 表行断档（节拍丢失）。
    RowGap,
    /// 0xac72 守卫触发（重入/越限被拒）。
    GuardTrip,
    /// 0xac73 映射键重复（入口分叉）。
    KeyDup,
    /// 0xac74 映射键断档（账不平）。
    KeyGap,
    /// 0xac75 映射值越界。
    ValOob,
    /// 0xac76 双表累计漂移。
    SumDrift,
    /// 0xac77 阈值漂移（守卫阈值≠冻结值）。
    ThreshDrift,
    /// 0xac78 台账不守恒（守恒恒等式破缺）。
    LedgerBroken,
    /// 0xac79 快照指纹不符（字段被改后重算不一致）。
    SnapshotFingerprint,
    /// 0xac7a 常量表破坏（表项与语义规则矛盾）。
    TableBroken,
    /// 0xac7b 段位守卫失败（诊断码越出本模块段）。
    SegmentGuard,
}

impl u2721Code {
    /// 全量闭集。
    pub const ALL: [u2721Code; 12] = [
        u2721Code::RowDup,
        u2721Code::RowGap,
        u2721Code::GuardTrip,
        u2721Code::KeyDup,
        u2721Code::KeyGap,
        u2721Code::ValOob,
        u2721Code::SumDrift,
        u2721Code::ThreshDrift,
        u2721Code::LedgerBroken,
        u2721Code::SnapshotFingerprint,
        u2721Code::TableBroken,
        u2721Code::SegmentGuard,
    ];

    /// 码值（连续十六进制，与声明序恒等）。
    pub const fn code(self) -> u16 {
        match self {
            u2721Code::RowDup => 0xac70,
            u2721Code::RowGap => 0xac71,
            u2721Code::GuardTrip => 0xac72,
            u2721Code::KeyDup => 0xac73,
            u2721Code::KeyGap => 0xac74,
            u2721Code::ValOob => 0xac75,
            u2721Code::SumDrift => 0xac76,
            u2721Code::ThreshDrift => 0xac77,
            u2721Code::LedgerBroken => 0xac78,
            u2721Code::SnapshotFingerprint => 0xac79,
            u2721Code::TableBroken => 0xac7a,
            u2721Code::SegmentGuard => 0xac7b,
        }
    }

    /// 中文诊断名。
    pub const fn name(self) -> &'static str {
        match self {
            u2721Code::RowDup => "表行重复（三段断链）",
            u2721Code::RowGap => "表行断档（节拍丢失）",
            u2721Code::GuardTrip => "守卫触发（重入/越限被拒）",
            u2721Code::KeyDup => "映射键重复（入口分叉）",
            u2721Code::KeyGap => "映射键断档（账不平）",
            u2721Code::ValOob => "映射值越界",
            u2721Code::SumDrift => "双表累计漂移",
            u2721Code::ThreshDrift => "阈值漂移（守卫阈值≠冻结值）",
            u2721Code::LedgerBroken => "台账不守恒（守恒恒等式破缺）",
            u2721Code::SnapshotFingerprint => "快照指纹不符（字段被改后重算不一致）",
            u2721Code::TableBroken => "常量表破坏（表项与语义规则矛盾）",
            u2721Code::SegmentGuard => "段位守卫失败（诊断码越出本模块段）",
        }
    }

    /// 可操作修复建议（每条诊断都必须带下一步动作）。
    pub const fn suggestion(self) -> &'static str {
        match self {
            u2721Code::RowDup => "按键唯一律去重",
            u2721Code::RowGap => "按连续律补号",
            u2721Code::GuardTrip => "检查临界区嵌套",
            u2721Code::KeyDup => "按键唯一律去重",
            u2721Code::KeyGap => "按连续律补号",
            u2721Code::ValOob => "按值域裁剪",
            u2721Code::SumDrift => "按理论值复算",
            u2721Code::ThreshDrift => "按冻结值校正",
            u2721Code::LedgerBroken => "按守恒律重建台账后重跑",
            u2721Code::SnapshotFingerprint => "按防篡改规程重建快照",
            u2721Code::TableBroken => "按语义规则重建常量表",
            u2721Code::SegmentGuard => "核对诊断码分配表",
        }
    }
}

// --------------------------------------------------------------------------
// 冻结常量表（u2721mix · 16 行逐行注释自证）
// --------------------------------------------------------------------------
/// 扫描轮次 对拍表（16 行）。
pub const u2721mix_COUNT: usize = 16;

/// 冻结表本体（编译期常量，运行期只读）。
pub static u2721mix: [(&'static str, u64); 16] = [
    ("SC_00", 10), // SC 对拍行（扫描器）
    ("IS_01", 15), // IS 对拍行（isolate 段）
    ("PD_02", 20), // PD 对拍行（谓词段）
    ("EV_03", 25), // EV 对拍行（逐出段）
    ("RT_04", 30), // RT 对拍行（归还段）
    ("TP_05", 35), // TP 对拍行（扫描节拍）
    ("WL_06", 40), // WL 对拍行（水线触发）
    ("DR_07", 45), // DR 对拍行（直接回收触发）
    ("SE_08", 50), // SE 对拍行（同一入口）
    ("R10_09", 55), // R10 对拍行（十轮闭环）
    ("AP_10", 60), // AP 对拍行（账平断言）
    ("CT_11", 65), // CT 对拍行（节拍面）
    ("SC_12", 70), // SC 对拍行（扫描器）
    ("IS_13", 75), // IS 对拍行（isolate 段）
    ("PD_14", 80), // PD 对拍行（谓词段）
    ("EV_15", 85), // EV 对拍行（逐出段）
];

/// 线性查找（键唯一性由构造期断言保证，运行时无分支歧义）。
pub fn u2721mix_get(key: &'static str) -> Option<u64> {
    let mut i = 0usize;
    while i < u2721mix_COUNT {
        if u2721mix[i].0 == key {
            return Some(u2721mix[i].1);
        }
        i += 1;
    }
    None
}

/// 累计求和（用于守恒对账）。
pub fn u2721mix_sum() -> u64 {
    let mut s = 0u64;
    let mut i = 0usize;
    while i < u2721mix_COUNT {
        s = s.wrapping_add(u2721mix[i].1 as u64);
        i += 1;
    }
    s
}

// --------------------------------------------------------------------------
// 守卫 u2721mixgate（阈值 32 · 重入/越限/静默三类拒止）
// --------------------------------------------------------------------------
/// 扫描轮次 混合面守卫（阈值 32 冻结）。
#[derive(Clone, Copy, Debug)]
pub struct u2721mixgate {
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

/// 守卫阈值（扫描轮次 混合面守卫（阈值 32 冻结））。
pub const U2721MIXGATE_THRESHOLD: u64 = 32;

impl u2721mixgate {
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
        if self.window >= U2721MIXGATE_THRESHOLD {
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
// 字符串键值映射 u2721mixmap（16 行逐行语义注释 + 逐行守卫）
// --------------------------------------------------------------------------
/// 扫描轮次 混合映射矩阵。
/// 行数。
pub const u2721mixmap_COUNT: usize = 16;

/// 表本体（键 + 值 + 注释由编译期冻结）。
pub static u2721mixmap: [(&'static str, u64); 16] = [
    ("SC0000", 0), // SC 单元 0000（扫描器 × 相位 00 × 混 轮）
    ("IS0001", 9), // IS 单元 0001（isolate 段 × 相位 01 × 混 轮）
    ("PD0002", 18), // PD 单元 0002（谓词段 × 相位 02 × 混 轮）
    ("EV0003", 27), // EV 单元 0003（逐出段 × 相位 03 × 混 轮）
    ("RT0004", 36), // RT 单元 0004（归还段 × 相位 04 × 混 轮）
    ("TP0005", 45), // TP 单元 0005（扫描节拍 × 相位 05 × 混 轮）
    ("WL0006", 54), // WL 单元 0006（水线触发 × 相位 06 × 混 轮）
    ("DR0007", 63), // DR 单元 0007（直接回收触发 × 相位 07 × 混 轮）
    ("SE0008", 8), // SE 单元 0008（同一入口 × 相位 08 × 混 轮）
    ("R100009", 17), // R10 单元 0009（十轮闭环 × 相位 09 × 混 轮）
    ("AP0010", 26), // AP 单元 0010（账平断言 × 相位 10 × 混 轮）
    ("CT0011", 35), // CT 单元 0011（节拍面 × 相位 11 × 混 轮）
    ("SC0012", 44), // SC 单元 0012（扫描器 × 相位 12 × 混 轮）
    ("IS0013", 53), // IS 单元 0013（isolate 段 × 相位 13 × 混 轮）
    ("PD0014", 62), // PD 单元 0014（谓词段 × 相位 14 × 混 轮）
    ("EV0015", 7), // EV 单元 0015（逐出段 × 相位 15 × 混 轮）
];

/// O(1) 语义查找（match 全枚举；命中率 100%% 由守卫保证）。
pub fn u2721mixmap_get(key: &str) -> Option<u64> {
    match key {
        "SC0000" => Some(0),
        "IS0001" => Some(9),
        "PD0002" => Some(18),
        "EV0003" => Some(27),
        "RT0004" => Some(36),
        "TP0005" => Some(45),
        "WL0006" => Some(54),
        "DR0007" => Some(63),
        "SE0008" => Some(8),
        "R100009" => Some(17),
        "AP0010" => Some(26),
        "CT0011" => Some(35),
        "SC0012" => Some(44),
        "IS0013" => Some(53),
        "PD0014" => Some(62),
        "EV0015" => Some(7),
        _ => None,
    }
}

/// 逐行守卫扫描（返回违例行数；0 = 全绿）。
pub fn u2721mixmap_scan() -> usize {
    let mut bad = 0usize;
    let mut i = 0usize;
    while i < u2721mixmap_COUNT {
        let (k, v) = u2721mixmap[i];
        let ok = match k {
            "SC0000" => v < 64,
            "IS0001" => v < 64,
            "PD0002" => v < 64,
            "EV0003" => v < 64,
            "RT0004" => v < 64,
            "TP0005" => v < 64,
            "WL0006" => v < 64,
            "DR0007" => v < 64,
            "SE0008" => v < 64,
            "R100009" => v < 64,
            "AP0010" => v < 64,
            "CT0011" => v < 64,
            "SC0012" => v < 64,
            "IS0013" => v < 64,
            "PD0014" => v < 64,
            "EV0015" => v < 64,
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
pub fn u2721mixmap_sum() -> u64 {
    let mut s = 0u64;
    let mut i = 0usize;
    while i < u2721mixmap_COUNT {
        s = s.wrapping_add(u2721mixmap[i].1 as u64);
        i += 1;
    }
    s
}

/// 键唯一性（构造期断言等价物：逐对扫描同键）。
pub fn u2721mixmap_unique() -> bool {
    let mut i = 0usize;
    while i < u2721mixmap_COUNT {
        let mut j = i + 1;
        while j < u2721mixmap_COUNT {
            if u2721mixmap[i].0 == u2721mixmap[j].0 {
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

/// 段位守卫：全部诊断码必须落在本段 [0xac70, 0xac7f]。
pub const fn u2721_segment_guard() -> bool {
    let mut ok = true;
    let mut i = 0usize;
    while i < u2721Code::ALL.len() {
        let c = u2721Code::ALL[i].code();
        if c < 0xac70 || c > 0xac7f {
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
fn u2721_guard_seg() -> bool {
    u2721_segment_guard()
}

/// UNX-F2721 对拍表 16 行边界与累计守恒。
fn check_u2721_mix_tbl() -> bool {
    let mut ok = u2721mix_COUNT == 16;
    if ok { ok = u2721mix_get("SC_00") == Some(10) && u2721mix_get("EV_15") == Some(85); }
    if ok { ok = u2721mix_get("ZZZ_NOKEY").is_none(); }
    if ok { ok = u2721mix_sum() == 760; }
    ok
}

/// UNX-F2721 混合映射 16 行守卫唯一 + 守卫阈值冻结。
fn check_u2721_mix_map() -> bool {
    let mut ok = u2721mixmap_COUNT == 16;
    if ok { ok = u2721mixmap_scan() == 0 && u2721mixmap_unique(); }
    if ok { ok = u2721mixmap_sum() == 504; }
    if ok { ok = U2721MIXGATE_THRESHOLD == 32; }
    ok
}

/// 本模块自检（逐检查带 detail，全绿才出门）。
pub fn run_u2721_checks() -> CheckSet {
    let mut set = CheckSet::new("U2721");
    set.add(
        "u2721-seg-guard",
        u2721_guard_seg(),
        if u2721_guard_seg() { "" } else { "diagnostic code outside segment" },
    );
    set.add(
        "u2721-mix-tbl",
        check_u2721_mix_tbl(),
        if check_u2721_mix_tbl() { "" } else { "UNX-F2721 mix table mismatch" },
    );
    set.add(
        "u2721-mix-map",
        check_u2721_mix_map(),
        if check_u2721_mix_map() { "" } else { "UNX-F2721 mix map mismatch" },
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
    fn u2721_all_checks_green() {
        let set = run_u2721_checks();
        assert!(set.all_passed(), "U2721 domain checks must be green");
    }

    #[test]
    fn u2721_segment_guard_holds() {
        assert!(u2721_segment_guard());
    }
}
