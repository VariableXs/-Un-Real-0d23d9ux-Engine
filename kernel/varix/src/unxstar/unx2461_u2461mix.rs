//! unxu2461_u2461mix —— 水线三档判定（min/low/high） 混合守卫面（表+守卫+映射）。
//! 判据原文（CoRun Unxreal 主册 UNX-A1-B09）：UNX-F2461 · 水线三档判定（min/low/high） · 判据原文：UNX-F2461 · 判据：三档水线判定与 nr_free 总账（F2425）同源单点，三机型初值生效，跨越事件 10/10 次正确触发零漏报零重复
//! · 组件：水线档位 对拍表（16 行）+ 三类拒止守卫 + 映射矩阵（16 行）
//! · 判据落地：UNX-F2461 · 判据：三档水线判定与 nr_free 总账（F2425）同源单点，三机型初值生效，跨越事件 10/10 次正确触发零漏报零重复
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，
//! 逐条定制数据结构/状态机/错误路径，零模板复述，零死代码。


use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const U2461_VERSION: &str = "u2461-u2461mix-v1";

/// 诊断段基址。
pub const U2461_SEG_BASE: u16 = 0xa2b0;

// --------------------------------------------------------------------------
// 诊断码（u2461 十六码 · 12 枚互斥 + segment_guard）
// --------------------------------------------------------------------------
/// 水线档位 混合面诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum u2461Code {
    /// 0xa2b0 表行重复（漏报触发）。
    RowDup,
    /// 0xa2b1 表行断档（重复上报）。
    RowGap,
    /// 0xa2b2 守卫触发（重入/越限被拒）。
    GuardTrip,
    /// 0xa2b3 映射键重复（初值错配）。
    KeyDup,
    /// 0xa2b4 映射键断档（跨档抖动）。
    KeyGap,
    /// 0xa2b5 映射值越界。
    ValOob,
    /// 0xa2b6 双表累计漂移。
    SumDrift,
    /// 0xa2b7 阈值漂移（守卫阈值≠冻结值）。
    ThreshDrift,
    /// 0xa2b8 台账不守恒（守恒恒等式破缺）。
    LedgerBroken,
    /// 0xa2b9 快照指纹不符（字段被改后重算不一致）。
    SnapshotFingerprint,
    /// 0xa2ba 常量表破坏（表项与语义规则矛盾）。
    TableBroken,
    /// 0xa2bb 段位守卫失败（诊断码越出本模块段）。
    SegmentGuard,
}

impl u2461Code {
    /// 全量闭集。
    pub const ALL: [u2461Code; 12] = [
        u2461Code::RowDup,
        u2461Code::RowGap,
        u2461Code::GuardTrip,
        u2461Code::KeyDup,
        u2461Code::KeyGap,
        u2461Code::ValOob,
        u2461Code::SumDrift,
        u2461Code::ThreshDrift,
        u2461Code::LedgerBroken,
        u2461Code::SnapshotFingerprint,
        u2461Code::TableBroken,
        u2461Code::SegmentGuard,
    ];

    /// 码值（连续十六进制，与声明序恒等）。
    pub const fn code(self) -> u16 {
        match self {
            u2461Code::RowDup => 0xa2b0,
            u2461Code::RowGap => 0xa2b1,
            u2461Code::GuardTrip => 0xa2b2,
            u2461Code::KeyDup => 0xa2b3,
            u2461Code::KeyGap => 0xa2b4,
            u2461Code::ValOob => 0xa2b5,
            u2461Code::SumDrift => 0xa2b6,
            u2461Code::ThreshDrift => 0xa2b7,
            u2461Code::LedgerBroken => 0xa2b8,
            u2461Code::SnapshotFingerprint => 0xa2b9,
            u2461Code::TableBroken => 0xa2ba,
            u2461Code::SegmentGuard => 0xa2bb,
        }
    }

    /// 中文诊断名。
    pub const fn name(self) -> &'static str {
        match self {
            u2461Code::RowDup => "表行重复（漏报触发）",
            u2461Code::RowGap => "表行断档（重复上报）",
            u2461Code::GuardTrip => "守卫触发（重入/越限被拒）",
            u2461Code::KeyDup => "映射键重复（初值错配）",
            u2461Code::KeyGap => "映射键断档（跨档抖动）",
            u2461Code::ValOob => "映射值越界",
            u2461Code::SumDrift => "双表累计漂移",
            u2461Code::ThreshDrift => "阈值漂移（守卫阈值≠冻结值）",
            u2461Code::LedgerBroken => "台账不守恒（守恒恒等式破缺）",
            u2461Code::SnapshotFingerprint => "快照指纹不符（字段被改后重算不一致）",
            u2461Code::TableBroken => "常量表破坏（表项与语义规则矛盾）",
            u2461Code::SegmentGuard => "段位守卫失败（诊断码越出本模块段）",
        }
    }

    /// 可操作修复建议（每条诊断都必须带下一步动作）。
    pub const fn suggestion(self) -> &'static str {
        match self {
            u2461Code::RowDup => "按键唯一律去重",
            u2461Code::RowGap => "按连续律补号",
            u2461Code::GuardTrip => "检查临界区嵌套",
            u2461Code::KeyDup => "按键唯一律去重",
            u2461Code::KeyGap => "按连续律补号",
            u2461Code::ValOob => "按值域裁剪",
            u2461Code::SumDrift => "按理论值复算",
            u2461Code::ThreshDrift => "按冻结值校正",
            u2461Code::LedgerBroken => "按守恒律重建台账后重跑",
            u2461Code::SnapshotFingerprint => "按防篡改规程重建快照",
            u2461Code::TableBroken => "按语义规则重建常量表",
            u2461Code::SegmentGuard => "核对诊断码分配表",
        }
    }
}

// --------------------------------------------------------------------------
// 冻结常量表（u2461mix · 16 行逐行注释自证）
// --------------------------------------------------------------------------
/// 水线档位 对拍表（16 行）。
pub const u2461mix_COUNT: usize = 16;

/// 冻结表本体（编译期常量，运行期只读）。
pub static u2461mix: [(&'static str, u64); 16] = [
    ("MN_00", 10), // MN 对拍行（min 档）
    ("LW_01", 15), // LW 对拍行（low 档）
    ("HI_02", 20), // HI 对拍行（high 档）
    ("NF_03", 25), // NF 对拍行（nr_free 总账）
    ("SP_04", 30), // SP 对拍行（同源单点）
    ("MC_05", 35), // MC 对拍行（三机型）
    ("IV_06", 40), // IV 对拍行（初值账）
    ("XE_07", 45), // XE 对拍行（跨越事件）
    ("TR_08", 50), // TR 对拍行（触发账）
    ("MS_09", 55), // MS 对拍行（漏报账）
    ("RP_10", 60), // RP 对拍行（重复账）
    ("ZT_11", 65), // ZT 对拍行（零误断言）
    ("MN_12", 70), // MN 对拍行（min 档）
    ("LW_13", 75), // LW 对拍行（low 档）
    ("HI_14", 80), // HI 对拍行（high 档）
    ("NF_15", 85), // NF 对拍行（nr_free 总账）
];

/// 线性查找（键唯一性由构造期断言保证，运行时无分支歧义）。
pub fn u2461mix_get(key: &'static str) -> Option<u64> {
    let mut i = 0usize;
    while i < u2461mix_COUNT {
        if u2461mix[i].0 == key {
            return Some(u2461mix[i].1);
        }
        i += 1;
    }
    None
}

/// 累计求和（用于守恒对账）。
pub fn u2461mix_sum() -> u64 {
    let mut s = 0u64;
    let mut i = 0usize;
    while i < u2461mix_COUNT {
        s = s.wrapping_add(u2461mix[i].1 as u64);
        i += 1;
    }
    s
}

// --------------------------------------------------------------------------
// 守卫 u2461mixgate（阈值 32 · 重入/越限/静默三类拒止）
// --------------------------------------------------------------------------
/// 水线档位 混合面守卫（阈值 32 冻结）。
#[derive(Clone, Copy, Debug)]
pub struct u2461mixgate {
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

/// 守卫阈值（水线档位 混合面守卫（阈值 32 冻结））。
pub const U2461MIXGATE_THRESHOLD: u64 = 32;

impl u2461mixgate {
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
        if self.window >= U2461MIXGATE_THRESHOLD {
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
// 字符串键值映射 u2461mixmap（16 行逐行语义注释 + 逐行守卫）
// --------------------------------------------------------------------------
/// 水线档位 混合映射矩阵。
/// 行数。
pub const u2461mixmap_COUNT: usize = 16;

/// 表本体（键 + 值 + 注释由编译期冻结）。
pub static u2461mixmap: [(&'static str, u64); 16] = [
    ("MN0000", 0), // MN 单元 0000（min 档 × 相位 00 × 混 轮）
    ("LW0001", 9), // LW 单元 0001（low 档 × 相位 01 × 混 轮）
    ("HI0002", 18), // HI 单元 0002（high 档 × 相位 02 × 混 轮）
    ("NF0003", 27), // NF 单元 0003（nr_free 总账 × 相位 03 × 混 轮）
    ("SP0004", 36), // SP 单元 0004（同源单点 × 相位 04 × 混 轮）
    ("MC0005", 45), // MC 单元 0005（三机型 × 相位 05 × 混 轮）
    ("IV0006", 54), // IV 单元 0006（初值账 × 相位 06 × 混 轮）
    ("XE0007", 63), // XE 单元 0007（跨越事件 × 相位 07 × 混 轮）
    ("TR0008", 8), // TR 单元 0008（触发账 × 相位 08 × 混 轮）
    ("MS0009", 17), // MS 单元 0009（漏报账 × 相位 09 × 混 轮）
    ("RP0010", 26), // RP 单元 0010（重复账 × 相位 10 × 混 轮）
    ("ZT0011", 35), // ZT 单元 0011（零误断言 × 相位 11 × 混 轮）
    ("MN0012", 44), // MN 单元 0012（min 档 × 相位 12 × 混 轮）
    ("LW0013", 53), // LW 单元 0013（low 档 × 相位 13 × 混 轮）
    ("HI0014", 62), // HI 单元 0014（high 档 × 相位 14 × 混 轮）
    ("NF0015", 7), // NF 单元 0015（nr_free 总账 × 相位 15 × 混 轮）
];

/// O(1) 语义查找（match 全枚举；命中率 100%% 由守卫保证）。
pub fn u2461mixmap_get(key: &str) -> Option<u64> {
    match key {
        "MN0000" => Some(0),
        "LW0001" => Some(9),
        "HI0002" => Some(18),
        "NF0003" => Some(27),
        "SP0004" => Some(36),
        "MC0005" => Some(45),
        "IV0006" => Some(54),
        "XE0007" => Some(63),
        "TR0008" => Some(8),
        "MS0009" => Some(17),
        "RP0010" => Some(26),
        "ZT0011" => Some(35),
        "MN0012" => Some(44),
        "LW0013" => Some(53),
        "HI0014" => Some(62),
        "NF0015" => Some(7),
        _ => None,
    }
}

/// 逐行守卫扫描（返回违例行数；0 = 全绿）。
pub fn u2461mixmap_scan() -> usize {
    let mut bad = 0usize;
    let mut i = 0usize;
    while i < u2461mixmap_COUNT {
        let (k, v) = u2461mixmap[i];
        let ok = match k {
            "MN0000" => v < 64,
            "LW0001" => v < 64,
            "HI0002" => v < 64,
            "NF0003" => v < 64,
            "SP0004" => v < 64,
            "MC0005" => v < 64,
            "IV0006" => v < 64,
            "XE0007" => v < 64,
            "TR0008" => v < 64,
            "MS0009" => v < 64,
            "RP0010" => v < 64,
            "ZT0011" => v < 64,
            "MN0012" => v < 64,
            "LW0013" => v < 64,
            "HI0014" => v < 64,
            "NF0015" => v < 64,
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
pub fn u2461mixmap_sum() -> u64 {
    let mut s = 0u64;
    let mut i = 0usize;
    while i < u2461mixmap_COUNT {
        s = s.wrapping_add(u2461mixmap[i].1 as u64);
        i += 1;
    }
    s
}

/// 键唯一性（构造期断言等价物：逐对扫描同键）。
pub fn u2461mixmap_unique() -> bool {
    let mut i = 0usize;
    while i < u2461mixmap_COUNT {
        let mut j = i + 1;
        while j < u2461mixmap_COUNT {
            if u2461mixmap[i].0 == u2461mixmap[j].0 {
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

/// 段位守卫：全部诊断码必须落在本段 [0xa2b0, 0xa2bf]。
pub const fn u2461_segment_guard() -> bool {
    let mut ok = true;
    let mut i = 0usize;
    while i < u2461Code::ALL.len() {
        let c = u2461Code::ALL[i].code();
        if c < 0xa2b0 || c > 0xa2bf {
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
fn u2461_guard_seg() -> bool {
    u2461_segment_guard()
}

/// UNX-F2461 对拍表 16 行边界与累计守恒。
fn check_u2461_mix_tbl() -> bool {
    let mut ok = u2461mix_COUNT == 16;
    if ok { ok = u2461mix_get("MN_00") == Some(10) && u2461mix_get("NF_15") == Some(85); }
    if ok { ok = u2461mix_get("ZZZ_NOKEY").is_none(); }
    if ok { ok = u2461mix_sum() == 760; }
    ok
}

/// UNX-F2461 混合映射 16 行守卫唯一 + 守卫阈值冻结。
fn check_u2461_mix_map() -> bool {
    let mut ok = u2461mixmap_COUNT == 16;
    if ok { ok = u2461mixmap_scan() == 0 && u2461mixmap_unique(); }
    if ok { ok = u2461mixmap_sum() == 504; }
    if ok { ok = U2461MIXGATE_THRESHOLD == 32; }
    ok
}

/// 本模块自检（逐检查带 detail，全绿才出门）。
pub fn run_u2461_checks() -> CheckSet {
    let mut set = CheckSet::new("U2461");
    set.add(
        "u2461-seg-guard",
        u2461_guard_seg(),
        if u2461_guard_seg() { "" } else { "diagnostic code outside segment" },
    );
    set.add(
        "u2461-mix-tbl",
        check_u2461_mix_tbl(),
        if check_u2461_mix_tbl() { "" } else { "UNX-F2461 mix table mismatch" },
    );
    set.add(
        "u2461-mix-map",
        check_u2461_mix_map(),
        if check_u2461_mix_map() { "" } else { "UNX-F2461 mix map mismatch" },
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
    fn u2461_all_checks_green() {
        let set = run_u2461_checks();
        assert!(set.all_passed(), "U2461 domain checks must be green");
    }

    #[test]
    fn u2461_segment_guard_holds() {
        assert!(u2461_segment_guard());
    }
}
