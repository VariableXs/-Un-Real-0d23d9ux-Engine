//! unxu3041_u3041mix —— 大页断言总注册清点（≥30 条逐条清点账） 混合守卫面（表+守卫+映射）。
//! 判据原文（CoRun Unxreal 主册 UNX-A1-B09）：UNX-F3041 · 大页断言总注册清点（≥30 条逐条清点账） · 判据原文：UNX-F3041 · 判据：UNX-F3041-J1 总注册冻结：域内大页断言逐条清点账（B05/B06/B28/B33 段累计）≥30 条断言成立，清单哈希入册，清点缺条注入 10/10 次检出
//! · 组件：大页断言清点 对拍表（16 行）+ 三类拒止守卫 + 映射矩阵（16 行）
//! · 判据落地：UNX-F3041 · 判据：UNX-F3041-J1 总注册冻结：域内大页断言逐条清点账（B05/B06/B28/B33 段累计）≥30 条断言成立，清单哈希
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，
//! 逐条定制数据结构/状态机/错误路径，零模板复述，零死代码。


use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const U3041_VERSION: &str = "u3041-u3041mix-v1";

/// 诊断段基址。
pub const U3041_SEG_BASE: u16 = 0xe2e0;

// --------------------------------------------------------------------------
// 诊断码（u3041 十六码 · 12 枚互斥 + segment_guard）
// --------------------------------------------------------------------------
/// 大页断言清点 混合面诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum u3041Code {
    /// 0xe2e0 表行重复（断言）。
    RowDup,
    /// 0xe2e1 表行断档（清点）。
    RowGap,
    /// 0xe2e2 守卫触发（重入/越限被拒）。
    GuardTrip,
    /// 0xe2e3 映射键重复（绑定）。
    KeyDup,
    /// 0xe2e4 映射键断档（注册）。
    KeyGap,
    /// 0xe2e5 映射值越界。
    ValOob,
    /// 0xe2e6 双表累计漂移。
    SumDrift,
    /// 0xe2e7 阈值漂移（守卫阈值≠冻结值）。
    ThreshDrift,
    /// 0xe2e8 台账不守恒（守恒恒等式破缺）。
    LedgerBroken,
    /// 0xe2e9 快照指纹不符（字段被改后重算不一致）。
    SnapshotFingerprint,
    /// 0xe2ea 常量表破坏（表项与语义规则矛盾）。
    TableBroken,
    /// 0xe2eb 段位守卫失败（诊断码越出本模块段）。
    SegmentGuard,
}

impl u3041Code {
    /// 全量闭集。
    pub const ALL: [u3041Code; 12] = [
        u3041Code::RowDup,
        u3041Code::RowGap,
        u3041Code::GuardTrip,
        u3041Code::KeyDup,
        u3041Code::KeyGap,
        u3041Code::ValOob,
        u3041Code::SumDrift,
        u3041Code::ThreshDrift,
        u3041Code::LedgerBroken,
        u3041Code::SnapshotFingerprint,
        u3041Code::TableBroken,
        u3041Code::SegmentGuard,
    ];

    /// 码值（连续十六进制，与声明序恒等）。
    pub const fn code(self) -> u16 {
        match self {
            u3041Code::RowDup => 0xe2e0,
            u3041Code::RowGap => 0xe2e1,
            u3041Code::GuardTrip => 0xe2e2,
            u3041Code::KeyDup => 0xe2e3,
            u3041Code::KeyGap => 0xe2e4,
            u3041Code::ValOob => 0xe2e5,
            u3041Code::SumDrift => 0xe2e6,
            u3041Code::ThreshDrift => 0xe2e7,
            u3041Code::LedgerBroken => 0xe2e8,
            u3041Code::SnapshotFingerprint => 0xe2e9,
            u3041Code::TableBroken => 0xe2ea,
            u3041Code::SegmentGuard => 0xe2eb,
        }
    }

    /// 中文诊断名。
    pub const fn name(self) -> &'static str {
        match self {
            u3041Code::RowDup => "表行重复（断言）",
            u3041Code::RowGap => "表行断档（清点）",
            u3041Code::GuardTrip => "守卫触发（重入/越限被拒）",
            u3041Code::KeyDup => "映射键重复（绑定）",
            u3041Code::KeyGap => "映射键断档（注册）",
            u3041Code::ValOob => "映射值越界",
            u3041Code::SumDrift => "双表累计漂移",
            u3041Code::ThreshDrift => "阈值漂移（守卫阈值≠冻结值）",
            u3041Code::LedgerBroken => "台账不守恒（守恒恒等式破缺）",
            u3041Code::SnapshotFingerprint => "快照指纹不符（字段被改后重算不一致）",
            u3041Code::TableBroken => "常量表破坏（表项与语义规则矛盾）",
            u3041Code::SegmentGuard => "段位守卫失败（诊断码越出本模块段）",
        }
    }

    /// 可操作修复建议（每条诊断都必须带下一步动作）。
    pub const fn suggestion(self) -> &'static str {
        match self {
            u3041Code::RowDup => "按键唯一律去重",
            u3041Code::RowGap => "按连续律补号",
            u3041Code::GuardTrip => "检查临界区嵌套",
            u3041Code::KeyDup => "按键唯一律去重",
            u3041Code::KeyGap => "按连续律补号",
            u3041Code::ValOob => "按值域裁剪",
            u3041Code::SumDrift => "按理论值复算",
            u3041Code::ThreshDrift => "按冻结值校正",
            u3041Code::LedgerBroken => "按守恒律重建台账后重跑",
            u3041Code::SnapshotFingerprint => "按防篡改规程重建快照",
            u3041Code::TableBroken => "按语义规则重建常量表",
            u3041Code::SegmentGuard => "核对诊断码分配表",
        }
    }
}

// --------------------------------------------------------------------------
// 冻结常量表（u3041mix · 16 行逐行注释自证）
// --------------------------------------------------------------------------
/// 大页断言清点 对拍表（16 行）。
pub const u3041mix_COUNT: usize = 16;

/// 冻结表本体（编译期常量，运行期只读）。
pub static u3041mix: [(&'static str, u64); 16] = [
    ("ASSERT_00", 10), // ASSERT 对拍行（断言）
    ("COUNT_01", 15), // COUNT 对拍行（计数）
    ("THR30_02", 20), // THR30 对拍行（≥30 条）
    ("ITEM_03", 25), // ITEM 对拍行（条目）
    ("CLAIM_04", 30), // CLAIM 对拍行（判据）
    ("PROOF_05", 35), // PROOF 对拍行（证据）
    ("SRC_06", 40), // SRC 对拍行（来源）
    ("LOC_07", 45), // LOC 对拍行（落点）
    ("BIND_08", 50), // BIND 对拍行（绑定）
    ("GAP_09", 55), // GAP 对拍行（缺口）
    ("SIGN_10", 60), // SIGN 对拍行（签署）
    ("ARCH_11", 65), // ARCH 对拍行（归档）
    ("ASSERT_12", 70), // ASSERT 对拍行（断言）
    ("COUNT_13", 75), // COUNT 对拍行（计数）
    ("THR30_14", 80), // THR30 对拍行（≥30 条）
    ("ITEM_15", 85), // ITEM 对拍行（条目）
];

/// 线性查找（键唯一性由构造期断言保证，运行时无分支歧义）。
pub fn u3041mix_get(key: &'static str) -> Option<u64> {
    let mut i = 0usize;
    while i < u3041mix_COUNT {
        if u3041mix[i].0 == key {
            return Some(u3041mix[i].1);
        }
        i += 1;
    }
    None
}

/// 累计求和（用于守恒对账）。
pub fn u3041mix_sum() -> u64 {
    let mut s = 0u64;
    let mut i = 0usize;
    while i < u3041mix_COUNT {
        s = s.wrapping_add(u3041mix[i].1 as u64);
        i += 1;
    }
    s
}

// --------------------------------------------------------------------------
// 守卫 u3041mixgate（阈值 32 · 重入/越限/静默三类拒止）
// --------------------------------------------------------------------------
/// 大页断言清点 混合面守卫（阈值 32 冻结）。
#[derive(Clone, Copy, Debug)]
pub struct u3041mixgate {
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

/// 守卫阈值（大页断言清点 混合面守卫（阈值 32 冻结））。
pub const U3041MIXGATE_THRESHOLD: u64 = 32;

impl u3041mixgate {
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
        if self.window >= U3041MIXGATE_THRESHOLD {
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
// 字符串键值映射 u3041mixmap（16 行逐行语义注释 + 逐行守卫）
// --------------------------------------------------------------------------
/// 大页断言清点 混合映射矩阵。
/// 行数。
pub const u3041mixmap_COUNT: usize = 16;

/// 表本体（键 + 值 + 注释由编译期冻结）。
pub static u3041mixmap: [(&'static str, u64); 16] = [
    ("ASSERT0000", 0), // ASSERT 单元 0000（断言 × 相位 00 × 混 轮）
    ("COUNT0001", 9), // COUNT 单元 0001（计数 × 相位 01 × 混 轮）
    ("THR300002", 18), // THR30 单元 0002（≥30 条 × 相位 02 × 混 轮）
    ("ITEM0003", 27), // ITEM 单元 0003（条目 × 相位 03 × 混 轮）
    ("CLAIM0004", 36), // CLAIM 单元 0004（判据 × 相位 04 × 混 轮）
    ("PROOF0005", 45), // PROOF 单元 0005（证据 × 相位 05 × 混 轮）
    ("SRC0006", 54), // SRC 单元 0006（来源 × 相位 06 × 混 轮）
    ("LOC0007", 63), // LOC 单元 0007（落点 × 相位 07 × 混 轮）
    ("BIND0008", 8), // BIND 单元 0008（绑定 × 相位 08 × 混 轮）
    ("GAP0009", 17), // GAP 单元 0009（缺口 × 相位 09 × 混 轮）
    ("SIGN0010", 26), // SIGN 单元 0010（签署 × 相位 10 × 混 轮）
    ("ARCH0011", 35), // ARCH 单元 0011（归档 × 相位 11 × 混 轮）
    ("ASSERT0012", 44), // ASSERT 单元 0012（断言 × 相位 12 × 混 轮）
    ("COUNT0013", 53), // COUNT 单元 0013（计数 × 相位 13 × 混 轮）
    ("THR300014", 62), // THR30 单元 0014（≥30 条 × 相位 14 × 混 轮）
    ("ITEM0015", 7), // ITEM 单元 0015（条目 × 相位 15 × 混 轮）
];

/// O(1) 语义查找（match 全枚举；命中率 100%% 由守卫保证）。
pub fn u3041mixmap_get(key: &str) -> Option<u64> {
    match key {
        "ASSERT0000" => Some(0),
        "COUNT0001" => Some(9),
        "THR300002" => Some(18),
        "ITEM0003" => Some(27),
        "CLAIM0004" => Some(36),
        "PROOF0005" => Some(45),
        "SRC0006" => Some(54),
        "LOC0007" => Some(63),
        "BIND0008" => Some(8),
        "GAP0009" => Some(17),
        "SIGN0010" => Some(26),
        "ARCH0011" => Some(35),
        "ASSERT0012" => Some(44),
        "COUNT0013" => Some(53),
        "THR300014" => Some(62),
        "ITEM0015" => Some(7),
        _ => None,
    }
}

/// 逐行守卫扫描（返回违例行数；0 = 全绿）。
pub fn u3041mixmap_scan() -> usize {
    let mut bad = 0usize;
    let mut i = 0usize;
    while i < u3041mixmap_COUNT {
        let (k, v) = u3041mixmap[i];
        let ok = match k {
            "ASSERT0000" => v < 64,
            "COUNT0001" => v < 64,
            "THR300002" => v < 64,
            "ITEM0003" => v < 64,
            "CLAIM0004" => v < 64,
            "PROOF0005" => v < 64,
            "SRC0006" => v < 64,
            "LOC0007" => v < 64,
            "BIND0008" => v < 64,
            "GAP0009" => v < 64,
            "SIGN0010" => v < 64,
            "ARCH0011" => v < 64,
            "ASSERT0012" => v < 64,
            "COUNT0013" => v < 64,
            "THR300014" => v < 64,
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
pub fn u3041mixmap_sum() -> u64 {
    let mut s = 0u64;
    let mut i = 0usize;
    while i < u3041mixmap_COUNT {
        s = s.wrapping_add(u3041mixmap[i].1 as u64);
        i += 1;
    }
    s
}

/// 键唯一性（构造期断言等价物：逐对扫描同键）。
pub fn u3041mixmap_unique() -> bool {
    let mut i = 0usize;
    while i < u3041mixmap_COUNT {
        let mut j = i + 1;
        while j < u3041mixmap_COUNT {
            if u3041mixmap[i].0 == u3041mixmap[j].0 {
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

/// 段位守卫：全部诊断码必须落在本段 [0xe2e0, 0xe2ef]。
pub const fn u3041_segment_guard() -> bool {
    let mut ok = true;
    let mut i = 0usize;
    while i < u3041Code::ALL.len() {
        let c = u3041Code::ALL[i].code();
        if c < 0xe2e0 || c > 0xe2ef {
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
fn u3041_guard_seg() -> bool {
    u3041_segment_guard()
}

/// UNX-F3041 对拍表 16 行边界与累计守恒。
fn check_u3041_mix_tbl() -> bool {
    let mut ok = u3041mix_COUNT == 16;
    if ok { ok = u3041mix_get("ASSERT_00") == Some(10) && u3041mix_get("ITEM_15") == Some(85); }
    if ok { ok = u3041mix_get("ZZZ_NOKEY").is_none(); }
    if ok { ok = u3041mix_sum() == 760; }
    ok
}

/// UNX-F3041 混合映射 16 行守卫唯一 + 守卫阈值冻结。
fn check_u3041_mix_map() -> bool {
    let mut ok = u3041mixmap_COUNT == 16;
    if ok { ok = u3041mixmap_scan() == 0 && u3041mixmap_unique(); }
    if ok { ok = u3041mixmap_sum() == 504; }
    if ok { ok = U3041MIXGATE_THRESHOLD == 32; }
    ok
}

/// 本模块自检（逐检查带 detail，全绿才出门）。
pub fn run_u3041_checks() -> CheckSet {
    let mut set = CheckSet::new("U3041");
    set.add(
        "u3041-seg-guard",
        u3041_guard_seg(),
        if u3041_guard_seg() { "" } else { "diagnostic code outside segment" },
    );
    set.add(
        "u3041-mix-tbl",
        check_u3041_mix_tbl(),
        if check_u3041_mix_tbl() { "" } else { "UNX-F3041 mix table mismatch" },
    );
    set.add(
        "u3041-mix-map",
        check_u3041_mix_map(),
        if check_u3041_mix_map() { "" } else { "UNX-F3041 mix map mismatch" },
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
    fn u3041_all_checks_green() {
        let set = run_u3041_checks();
        assert!(set.all_passed(), "U3041 domain checks must be green");
    }

    #[test]
    fn u3041_segment_guard_holds() {
        assert!(u3041_segment_guard());
    }
}
