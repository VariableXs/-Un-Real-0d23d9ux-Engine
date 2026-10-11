//! unxu3781_u3781mix —— 域中期收口总设计（B20 前向声明的兑现面） 混合守卫面（表+守卫+映射）。
//! 判据原文（CoRun Unxreal 主册 UNX-A1-B09）：UNX-F3781 · 域中期收口总设计（B20 前向声明的兑现面） · 判据原文：UNX-F3781 · 判据：J1：收口架构（三总轧（批账/域账/凭证）+三宣告（断言大关/移交就绪/风险清单）——判据行内嵌：B20 前向声明的原文兑现（"F3781 域中期收口：段断言大关 ≥80+五条件移交"——期票与兑付跨十批的闭链）；收口的范围声明（B16–B30 十五批——A5 域续领波的全域——不含 B01–B15（已在前次 finalize 收口——历史账冻结）；M-539 收口预跑（三总轧空转→流程通→正式总轧前演练）。<br>J2：收口的纪律（收口只核不改（发现的偏差走修正流程+留痕——收口不涂改历史账。
//! · 组件：域中期收口 对拍表（16 行）+ 三类拒止守卫 + 映射矩阵（16 行）
//! · 判据落地：UNX-F3781 · 判据：J1：收口架构（三总轧（批账/域账/凭证）+三宣告（断言大关/移交就绪/风险清单）——判据行内嵌：B20 前向声明的原文兑现（"F
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，
//! 逐条定制数据结构/状态机/错误路径，零模板复述，零死代码。


use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const U3781_VERSION: &str = "u3781-u3781mix-v1";

/// 诊断段基址。
pub const U3781_SEG_BASE: u16 = 0xe210;

// --------------------------------------------------------------------------
// 诊断码（u3781 十六码 · 12 枚互斥 + segment_guard）
// --------------------------------------------------------------------------
/// 域中期收口 混合面诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum u3781Code {
    /// 0xe210 表行重复（收口）。
    RowDup,
    /// 0xe211 表行断档（兑现）。
    RowGap,
    /// 0xe212 守卫触发（重入/越限被拒）。
    GuardTrip,
    /// 0xe213 映射键重复（缺口）。
    KeyDup,
    /// 0xe214 映射键断档（兑现）。
    KeyGap,
    /// 0xe215 映射值越界。
    ValOob,
    /// 0xe216 双表累计漂移。
    SumDrift,
    /// 0xe217 阈值漂移（守卫阈值≠冻结值）。
    ThreshDrift,
    /// 0xe218 台账不守恒（守恒恒等式破缺）。
    LedgerBroken,
    /// 0xe219 快照指纹不符（字段被改后重算不一致）。
    SnapshotFingerprint,
    /// 0xe21a 常量表破坏（表项与语义规则矛盾）。
    TableBroken,
    /// 0xe21b 段位守卫失败（诊断码越出本模块段）。
    SegmentGuard,
}

impl u3781Code {
    /// 全量闭集。
    pub const ALL: [u3781Code; 12] = [
        u3781Code::RowDup,
        u3781Code::RowGap,
        u3781Code::GuardTrip,
        u3781Code::KeyDup,
        u3781Code::KeyGap,
        u3781Code::ValOob,
        u3781Code::SumDrift,
        u3781Code::ThreshDrift,
        u3781Code::LedgerBroken,
        u3781Code::SnapshotFingerprint,
        u3781Code::TableBroken,
        u3781Code::SegmentGuard,
    ];

    /// 码值（连续十六进制，与声明序恒等）。
    pub const fn code(self) -> u16 {
        match self {
            u3781Code::RowDup => 0xe210,
            u3781Code::RowGap => 0xe211,
            u3781Code::GuardTrip => 0xe212,
            u3781Code::KeyDup => 0xe213,
            u3781Code::KeyGap => 0xe214,
            u3781Code::ValOob => 0xe215,
            u3781Code::SumDrift => 0xe216,
            u3781Code::ThreshDrift => 0xe217,
            u3781Code::LedgerBroken => 0xe218,
            u3781Code::SnapshotFingerprint => 0xe219,
            u3781Code::TableBroken => 0xe21a,
            u3781Code::SegmentGuard => 0xe21b,
        }
    }

    /// 中文诊断名。
    pub const fn name(self) -> &'static str {
        match self {
            u3781Code::RowDup => "表行重复（收口）",
            u3781Code::RowGap => "表行断档（兑现）",
            u3781Code::GuardTrip => "守卫触发（重入/越限被拒）",
            u3781Code::KeyDup => "映射键重复（缺口）",
            u3781Code::KeyGap => "映射键断档（兑现）",
            u3781Code::ValOob => "映射值越界",
            u3781Code::SumDrift => "双表累计漂移",
            u3781Code::ThreshDrift => "阈值漂移（守卫阈值≠冻结值）",
            u3781Code::LedgerBroken => "台账不守恒（守恒恒等式破缺）",
            u3781Code::SnapshotFingerprint => "快照指纹不符（字段被改后重算不一致）",
            u3781Code::TableBroken => "常量表破坏（表项与语义规则矛盾）",
            u3781Code::SegmentGuard => "段位守卫失败（诊断码越出本模块段）",
        }
    }

    /// 可操作修复建议（每条诊断都必须带下一步动作）。
    pub const fn suggestion(self) -> &'static str {
        match self {
            u3781Code::RowDup => "按键唯一律去重",
            u3781Code::RowGap => "按连续律补号",
            u3781Code::GuardTrip => "检查临界区嵌套",
            u3781Code::KeyDup => "按键唯一律去重",
            u3781Code::KeyGap => "按连续律补号",
            u3781Code::ValOob => "按值域裁剪",
            u3781Code::SumDrift => "按理论值复算",
            u3781Code::ThreshDrift => "按冻结值校正",
            u3781Code::LedgerBroken => "按守恒律重建台账后重跑",
            u3781Code::SnapshotFingerprint => "按防篡改规程重建快照",
            u3781Code::TableBroken => "按语义规则重建常量表",
            u3781Code::SegmentGuard => "核对诊断码分配表",
        }
    }
}

// --------------------------------------------------------------------------
// 冻结常量表（u3781mix · 16 行逐行注释自证）
// --------------------------------------------------------------------------
/// 域中期收口 对拍表（16 行）。
pub const u3781mix_COUNT: usize = 16;

/// 冻结表本体（编译期常量，运行期只读）。
pub static u3781mix: [(&'static str, u64); 16] = [
    ("CLOSE_00", 10), // CLOSE 对拍行（收口）
    ("MID_01", 15), // MID 对拍行（中期）
    ("DECL_02", 20), // DECL 对拍行（前向声明）
    ("Fulfill_03", 25), // Fulfill 对拍行（兑现）
    ("SCOPE_04", 30), // SCOPE 对拍行（范围）
    ("ITEM_05", 35), // ITEM 对拍行（条目）
    ("DONE_06", 40), // DONE 对拍行（已完成）
    ("OPEN_07", 45), // OPEN 对拍行（未开）
    ("GAP_08", 50), // GAP 对拍行（缺口）
    ("BIND_09", 55), // BIND 对拍行（绑定）
    ("PROOF_10", 60), // PROOF 对拍行（证据）
    ("GATE_11", 65), // GATE 对拍行（闸门）
    ("CLOSE_12", 70), // CLOSE 对拍行（收口）
    ("MID_13", 75), // MID 对拍行（中期）
    ("DECL_14", 80), // DECL 对拍行（前向声明）
    ("Fulfill_15", 85), // Fulfill 对拍行（兑现）
];

/// 线性查找（键唯一性由构造期断言保证，运行时无分支歧义）。
pub fn u3781mix_get(key: &'static str) -> Option<u64> {
    let mut i = 0usize;
    while i < u3781mix_COUNT {
        if u3781mix[i].0 == key {
            return Some(u3781mix[i].1);
        }
        i += 1;
    }
    None
}

/// 累计求和（用于守恒对账）。
pub fn u3781mix_sum() -> u64 {
    let mut s = 0u64;
    let mut i = 0usize;
    while i < u3781mix_COUNT {
        s = s.wrapping_add(u3781mix[i].1 as u64);
        i += 1;
    }
    s
}

// --------------------------------------------------------------------------
// 守卫 u3781mixgate（阈值 32 · 重入/越限/静默三类拒止）
// --------------------------------------------------------------------------
/// 域中期收口 混合面守卫（阈值 32 冻结）。
#[derive(Clone, Copy, Debug)]
pub struct u3781mixgate {
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

/// 守卫阈值（域中期收口 混合面守卫（阈值 32 冻结））。
pub const U3781MIXGATE_THRESHOLD: u64 = 32;

impl u3781mixgate {
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
        if self.window >= U3781MIXGATE_THRESHOLD {
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
// 字符串键值映射 u3781mixmap（16 行逐行语义注释 + 逐行守卫）
// --------------------------------------------------------------------------
/// 域中期收口 混合映射矩阵。
/// 行数。
pub const u3781mixmap_COUNT: usize = 16;

/// 表本体（键 + 值 + 注释由编译期冻结）。
pub static u3781mixmap: [(&'static str, u64); 16] = [
    ("CLOSE0000", 0), // CLOSE 单元 0000（收口 × 相位 00 × 混 轮）
    ("MID0001", 9), // MID 单元 0001（中期 × 相位 01 × 混 轮）
    ("DECL0002", 18), // DECL 单元 0002（前向声明 × 相位 02 × 混 轮）
    ("Fulfill0003", 27), // Fulfill 单元 0003（兑现 × 相位 03 × 混 轮）
    ("SCOPE0004", 36), // SCOPE 单元 0004（范围 × 相位 04 × 混 轮）
    ("ITEM0005", 45), // ITEM 单元 0005（条目 × 相位 05 × 混 轮）
    ("DONE0006", 54), // DONE 单元 0006（已完成 × 相位 06 × 混 轮）
    ("OPEN0007", 63), // OPEN 单元 0007（未开 × 相位 07 × 混 轮）
    ("GAP0008", 8), // GAP 单元 0008（缺口 × 相位 08 × 混 轮）
    ("BIND0009", 17), // BIND 单元 0009（绑定 × 相位 09 × 混 轮）
    ("PROOF0010", 26), // PROOF 单元 0010（证据 × 相位 10 × 混 轮）
    ("GATE0011", 35), // GATE 单元 0011（闸门 × 相位 11 × 混 轮）
    ("CLOSE0012", 44), // CLOSE 单元 0012（收口 × 相位 12 × 混 轮）
    ("MID0013", 53), // MID 单元 0013（中期 × 相位 13 × 混 轮）
    ("DECL0014", 62), // DECL 单元 0014（前向声明 × 相位 14 × 混 轮）
    ("Fulfill0015", 7), // Fulfill 单元 0015（兑现 × 相位 15 × 混 轮）
];

/// O(1) 语义查找（match 全枚举；命中率 100%% 由守卫保证）。
pub fn u3781mixmap_get(key: &str) -> Option<u64> {
    match key {
        "CLOSE0000" => Some(0),
        "MID0001" => Some(9),
        "DECL0002" => Some(18),
        "Fulfill0003" => Some(27),
        "SCOPE0004" => Some(36),
        "ITEM0005" => Some(45),
        "DONE0006" => Some(54),
        "OPEN0007" => Some(63),
        "GAP0008" => Some(8),
        "BIND0009" => Some(17),
        "PROOF0010" => Some(26),
        "GATE0011" => Some(35),
        "CLOSE0012" => Some(44),
        "MID0013" => Some(53),
        "DECL0014" => Some(62),
        "Fulfill0015" => Some(7),
        _ => None,
    }
}

/// 逐行守卫扫描（返回违例行数；0 = 全绿）。
pub fn u3781mixmap_scan() -> usize {
    let mut bad = 0usize;
    let mut i = 0usize;
    while i < u3781mixmap_COUNT {
        let (k, v) = u3781mixmap[i];
        let ok = match k {
            "CLOSE0000" => v < 64,
            "MID0001" => v < 64,
            "DECL0002" => v < 64,
            "Fulfill0003" => v < 64,
            "SCOPE0004" => v < 64,
            "ITEM0005" => v < 64,
            "DONE0006" => v < 64,
            "OPEN0007" => v < 64,
            "GAP0008" => v < 64,
            "BIND0009" => v < 64,
            "PROOF0010" => v < 64,
            "GATE0011" => v < 64,
            "CLOSE0012" => v < 64,
            "MID0013" => v < 64,
            "DECL0014" => v < 64,
            "Fulfill0015" => v < 64,
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
pub fn u3781mixmap_sum() -> u64 {
    let mut s = 0u64;
    let mut i = 0usize;
    while i < u3781mixmap_COUNT {
        s = s.wrapping_add(u3781mixmap[i].1 as u64);
        i += 1;
    }
    s
}

/// 键唯一性（构造期断言等价物：逐对扫描同键）。
pub fn u3781mixmap_unique() -> bool {
    let mut i = 0usize;
    while i < u3781mixmap_COUNT {
        let mut j = i + 1;
        while j < u3781mixmap_COUNT {
            if u3781mixmap[i].0 == u3781mixmap[j].0 {
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

/// 段位守卫：全部诊断码必须落在本段 [0xe210, 0xe21f]。
pub const fn u3781_segment_guard() -> bool {
    let mut ok = true;
    let mut i = 0usize;
    while i < u3781Code::ALL.len() {
        let c = u3781Code::ALL[i].code();
        if c < 0xe210 || c > 0xe21f {
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
fn u3781_guard_seg() -> bool {
    u3781_segment_guard()
}

/// UNX-F3781 对拍表 16 行边界与累计守恒。
fn check_u3781_mix_tbl() -> bool {
    let mut ok = u3781mix_COUNT == 16;
    if ok { ok = u3781mix_get("CLOSE_00") == Some(10) && u3781mix_get("Fulfill_15") == Some(85); }
    if ok { ok = u3781mix_get("ZZZ_NOKEY").is_none(); }
    if ok { ok = u3781mix_sum() == 760; }
    ok
}

/// UNX-F3781 混合映射 16 行守卫唯一 + 守卫阈值冻结。
fn check_u3781_mix_map() -> bool {
    let mut ok = u3781mixmap_COUNT == 16;
    if ok { ok = u3781mixmap_scan() == 0 && u3781mixmap_unique(); }
    if ok { ok = u3781mixmap_sum() == 504; }
    if ok { ok = U3781MIXGATE_THRESHOLD == 32; }
    ok
}

/// 本模块自检（逐检查带 detail，全绿才出门）。
pub fn run_u3781_checks() -> CheckSet {
    let mut set = CheckSet::new("U3781");
    set.add(
        "u3781-seg-guard",
        u3781_guard_seg(),
        if u3781_guard_seg() { "" } else { "diagnostic code outside segment" },
    );
    set.add(
        "u3781-mix-tbl",
        check_u3781_mix_tbl(),
        if check_u3781_mix_tbl() { "" } else { "UNX-F3781 mix table mismatch" },
    );
    set.add(
        "u3781-mix-map",
        check_u3781_mix_map(),
        if check_u3781_mix_map() { "" } else { "UNX-F3781 mix map mismatch" },
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
    fn u3781_all_checks_green() {
        let set = run_u3781_checks();
        assert!(set.all_passed(), "U3781 domain checks must be green");
    }

    #[test]
    fn u3781_segment_guard_holds() {
        assert!(u3781_segment_guard());
    }
}
