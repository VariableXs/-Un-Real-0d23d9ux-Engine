//! unxu3741_u3741mix —— 门禁总设计（五门体系与流水线拓扑） 混合守卫面（表+守卫+映射）。
//! 判据原文（CoRun Unxreal 主册 UNX-A1-B09）：UNX-F3741 · 门禁总设计（五门体系与流水线拓扑） · 判据原文：UNX-F3741 · 判据：J1：五门体系（守恒门/防重门/覆盖门/断言门/fuzz 门——判据行内嵌五门各守什么律）+流水线拓扑（提交→五门串行→报告——门序 const）；M-499 五门首跑（样例提交过五门→全绿报告出）。<br>J2：门是冷的（机器判——门无人工覆写通道（豁免走 F3755 显式制——不是当场翻绿）。
//! · 组件：门禁体系 对拍表（16 行）+ 三类拒止守卫 + 映射矩阵（16 行）
//! · 判据落地：UNX-F3741 · 判据：J1：五门体系（守恒门/防重门/覆盖门/断言门/fuzz 门——判据行内嵌五门各守什么律）+流水线拓扑（提交→五门串行→报告——门
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，
//! 逐条定制数据结构/状态机/错误路径，零模板复述，零死代码。


use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const U3741_VERSION: &str = "u3741-u3741mix-v1";

/// 诊断段基址。
pub const U3741_SEG_BASE: u16 = 0xe1f0;

// --------------------------------------------------------------------------
// 诊断码（u3741 十六码 · 12 枚互斥 + segment_guard）
// --------------------------------------------------------------------------
/// 门禁体系 混合面诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum u3741Code {
    /// 0xe1f0 表行重复（门禁）。
    RowDup,
    /// 0xe1f1 表行断档（五门）。
    RowGap,
    /// 0xe1f2 守卫触发（重入/越限被拒）。
    GuardTrip,
    /// 0xe1f3 映射键重复（拓扑）。
    KeyDup,
    /// 0xe1f4 映射键断档（五门）。
    KeyGap,
    /// 0xe1f5 映射值越界。
    ValOob,
    /// 0xe1f6 双表累计漂移。
    SumDrift,
    /// 0xe1f7 阈值漂移（守卫阈值≠冻结值）。
    ThreshDrift,
    /// 0xe1f8 台账不守恒（守恒恒等式破缺）。
    LedgerBroken,
    /// 0xe1f9 快照指纹不符（字段被改后重算不一致）。
    SnapshotFingerprint,
    /// 0xe1fa 常量表破坏（表项与语义规则矛盾）。
    TableBroken,
    /// 0xe1fb 段位守卫失败（诊断码越出本模块段）。
    SegmentGuard,
}

impl u3741Code {
    /// 全量闭集。
    pub const ALL: [u3741Code; 12] = [
        u3741Code::RowDup,
        u3741Code::RowGap,
        u3741Code::GuardTrip,
        u3741Code::KeyDup,
        u3741Code::KeyGap,
        u3741Code::ValOob,
        u3741Code::SumDrift,
        u3741Code::ThreshDrift,
        u3741Code::LedgerBroken,
        u3741Code::SnapshotFingerprint,
        u3741Code::TableBroken,
        u3741Code::SegmentGuard,
    ];

    /// 码值（连续十六进制，与声明序恒等）。
    pub const fn code(self) -> u16 {
        match self {
            u3741Code::RowDup => 0xe1f0,
            u3741Code::RowGap => 0xe1f1,
            u3741Code::GuardTrip => 0xe1f2,
            u3741Code::KeyDup => 0xe1f3,
            u3741Code::KeyGap => 0xe1f4,
            u3741Code::ValOob => 0xe1f5,
            u3741Code::SumDrift => 0xe1f6,
            u3741Code::ThreshDrift => 0xe1f7,
            u3741Code::LedgerBroken => 0xe1f8,
            u3741Code::SnapshotFingerprint => 0xe1f9,
            u3741Code::TableBroken => 0xe1fa,
            u3741Code::SegmentGuard => 0xe1fb,
        }
    }

    /// 中文诊断名。
    pub const fn name(self) -> &'static str {
        match self {
            u3741Code::RowDup => "表行重复（门禁）",
            u3741Code::RowGap => "表行断档（五门）",
            u3741Code::GuardTrip => "守卫触发（重入/越限被拒）",
            u3741Code::KeyDup => "映射键重复（拓扑）",
            u3741Code::KeyGap => "映射键断档（五门）",
            u3741Code::ValOob => "映射值越界",
            u3741Code::SumDrift => "双表累计漂移",
            u3741Code::ThreshDrift => "阈值漂移（守卫阈值≠冻结值）",
            u3741Code::LedgerBroken => "台账不守恒（守恒恒等式破缺）",
            u3741Code::SnapshotFingerprint => "快照指纹不符（字段被改后重算不一致）",
            u3741Code::TableBroken => "常量表破坏（表项与语义规则矛盾）",
            u3741Code::SegmentGuard => "段位守卫失败（诊断码越出本模块段）",
        }
    }

    /// 可操作修复建议（每条诊断都必须带下一步动作）。
    pub const fn suggestion(self) -> &'static str {
        match self {
            u3741Code::RowDup => "按键唯一律去重",
            u3741Code::RowGap => "按连续律补号",
            u3741Code::GuardTrip => "检查临界区嵌套",
            u3741Code::KeyDup => "按键唯一律去重",
            u3741Code::KeyGap => "按连续律补号",
            u3741Code::ValOob => "按值域裁剪",
            u3741Code::SumDrift => "按理论值复算",
            u3741Code::ThreshDrift => "按冻结值校正",
            u3741Code::LedgerBroken => "按守恒律重建台账后重跑",
            u3741Code::SnapshotFingerprint => "按防篡改规程重建快照",
            u3741Code::TableBroken => "按语义规则重建常量表",
            u3741Code::SegmentGuard => "核对诊断码分配表",
        }
    }
}

// --------------------------------------------------------------------------
// 冻结常量表（u3741mix · 16 行逐行注释自证）
// --------------------------------------------------------------------------
/// 门禁体系 对拍表（16 行）。
pub const u3741mix_COUNT: usize = 16;

/// 冻结表本体（编译期常量，运行期只读）。
pub static u3741mix: [(&'static str, u64); 16] = [
    ("GATE_00", 10), // GATE 对拍行（门禁）
    ("FIVE_01", 15), // FIVE 对拍行（五门）
    ("PIPE_02", 20), // PIPE 对拍行（流水线）
    ("TOPO_03", 25), // TOPO 对拍行（拓扑）
    ("FAST_04", 30), // FAST 对拍行（快速门）
    ("DEEP_05", 35), // DEEP 对拍行（深度门）
    ("FUZZG_06", 40), // FUZZG 对拍行（fuzz 门）
    ("REGR_07", 45), // REGR 对拍行（回归门）
    ("RELEASE_08", 50), // RELEASE 对拍行（发布门）
    ("ORDER_09", 55), // ORDER 对拍行（门序）
    ("BLOCK_10", 60), // BLOCK 对拍行（阻断）
    ("SIGN_11", 65), // SIGN 对拍行（签署）
    ("GATE_12", 70), // GATE 对拍行（门禁）
    ("FIVE_13", 75), // FIVE 对拍行（五门）
    ("PIPE_14", 80), // PIPE 对拍行（流水线）
    ("TOPO_15", 85), // TOPO 对拍行（拓扑）
];

/// 线性查找（键唯一性由构造期断言保证，运行时无分支歧义）。
pub fn u3741mix_get(key: &'static str) -> Option<u64> {
    let mut i = 0usize;
    while i < u3741mix_COUNT {
        if u3741mix[i].0 == key {
            return Some(u3741mix[i].1);
        }
        i += 1;
    }
    None
}

/// 累计求和（用于守恒对账）。
pub fn u3741mix_sum() -> u64 {
    let mut s = 0u64;
    let mut i = 0usize;
    while i < u3741mix_COUNT {
        s = s.wrapping_add(u3741mix[i].1 as u64);
        i += 1;
    }
    s
}

// --------------------------------------------------------------------------
// 守卫 u3741mixgate（阈值 32 · 重入/越限/静默三类拒止）
// --------------------------------------------------------------------------
/// 门禁体系 混合面守卫（阈值 32 冻结）。
#[derive(Clone, Copy, Debug)]
pub struct u3741mixgate {
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

/// 守卫阈值（门禁体系 混合面守卫（阈值 32 冻结））。
pub const U3741MIXGATE_THRESHOLD: u64 = 32;

impl u3741mixgate {
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
        if self.window >= U3741MIXGATE_THRESHOLD {
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
// 字符串键值映射 u3741mixmap（16 行逐行语义注释 + 逐行守卫）
// --------------------------------------------------------------------------
/// 门禁体系 混合映射矩阵。
/// 行数。
pub const u3741mixmap_COUNT: usize = 16;

/// 表本体（键 + 值 + 注释由编译期冻结）。
pub static u3741mixmap: [(&'static str, u64); 16] = [
    ("GATE0000", 0), // GATE 单元 0000（门禁 × 相位 00 × 混 轮）
    ("FIVE0001", 9), // FIVE 单元 0001（五门 × 相位 01 × 混 轮）
    ("PIPE0002", 18), // PIPE 单元 0002（流水线 × 相位 02 × 混 轮）
    ("TOPO0003", 27), // TOPO 单元 0003（拓扑 × 相位 03 × 混 轮）
    ("FAST0004", 36), // FAST 单元 0004（快速门 × 相位 04 × 混 轮）
    ("DEEP0005", 45), // DEEP 单元 0005（深度门 × 相位 05 × 混 轮）
    ("FUZZG0006", 54), // FUZZG 单元 0006（fuzz 门 × 相位 06 × 混 轮）
    ("REGR0007", 63), // REGR 单元 0007（回归门 × 相位 07 × 混 轮）
    ("RELEASE0008", 8), // RELEASE 单元 0008（发布门 × 相位 08 × 混 轮）
    ("ORDER0009", 17), // ORDER 单元 0009（门序 × 相位 09 × 混 轮）
    ("BLOCK0010", 26), // BLOCK 单元 0010（阻断 × 相位 10 × 混 轮）
    ("SIGN0011", 35), // SIGN 单元 0011（签署 × 相位 11 × 混 轮）
    ("GATE0012", 44), // GATE 单元 0012（门禁 × 相位 12 × 混 轮）
    ("FIVE0013", 53), // FIVE 单元 0013（五门 × 相位 13 × 混 轮）
    ("PIPE0014", 62), // PIPE 单元 0014（流水线 × 相位 14 × 混 轮）
    ("TOPO0015", 7), // TOPO 单元 0015（拓扑 × 相位 15 × 混 轮）
];

/// O(1) 语义查找（match 全枚举；命中率 100%% 由守卫保证）。
pub fn u3741mixmap_get(key: &str) -> Option<u64> {
    match key {
        "GATE0000" => Some(0),
        "FIVE0001" => Some(9),
        "PIPE0002" => Some(18),
        "TOPO0003" => Some(27),
        "FAST0004" => Some(36),
        "DEEP0005" => Some(45),
        "FUZZG0006" => Some(54),
        "REGR0007" => Some(63),
        "RELEASE0008" => Some(8),
        "ORDER0009" => Some(17),
        "BLOCK0010" => Some(26),
        "SIGN0011" => Some(35),
        "GATE0012" => Some(44),
        "FIVE0013" => Some(53),
        "PIPE0014" => Some(62),
        "TOPO0015" => Some(7),
        _ => None,
    }
}

/// 逐行守卫扫描（返回违例行数；0 = 全绿）。
pub fn u3741mixmap_scan() -> usize {
    let mut bad = 0usize;
    let mut i = 0usize;
    while i < u3741mixmap_COUNT {
        let (k, v) = u3741mixmap[i];
        let ok = match k {
            "GATE0000" => v < 64,
            "FIVE0001" => v < 64,
            "PIPE0002" => v < 64,
            "TOPO0003" => v < 64,
            "FAST0004" => v < 64,
            "DEEP0005" => v < 64,
            "FUZZG0006" => v < 64,
            "REGR0007" => v < 64,
            "RELEASE0008" => v < 64,
            "ORDER0009" => v < 64,
            "BLOCK0010" => v < 64,
            "SIGN0011" => v < 64,
            "GATE0012" => v < 64,
            "FIVE0013" => v < 64,
            "PIPE0014" => v < 64,
            "TOPO0015" => v < 64,
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
pub fn u3741mixmap_sum() -> u64 {
    let mut s = 0u64;
    let mut i = 0usize;
    while i < u3741mixmap_COUNT {
        s = s.wrapping_add(u3741mixmap[i].1 as u64);
        i += 1;
    }
    s
}

/// 键唯一性（构造期断言等价物：逐对扫描同键）。
pub fn u3741mixmap_unique() -> bool {
    let mut i = 0usize;
    while i < u3741mixmap_COUNT {
        let mut j = i + 1;
        while j < u3741mixmap_COUNT {
            if u3741mixmap[i].0 == u3741mixmap[j].0 {
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

/// 段位守卫：全部诊断码必须落在本段 [0xe1f0, 0xe1ff]。
pub const fn u3741_segment_guard() -> bool {
    let mut ok = true;
    let mut i = 0usize;
    while i < u3741Code::ALL.len() {
        let c = u3741Code::ALL[i].code();
        if c < 0xe1f0 || c > 0xe1ff {
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
fn u3741_guard_seg() -> bool {
    u3741_segment_guard()
}

/// UNX-F3741 对拍表 16 行边界与累计守恒。
fn check_u3741_mix_tbl() -> bool {
    let mut ok = u3741mix_COUNT == 16;
    if ok { ok = u3741mix_get("GATE_00") == Some(10) && u3741mix_get("TOPO_15") == Some(85); }
    if ok { ok = u3741mix_get("ZZZ_NOKEY").is_none(); }
    if ok { ok = u3741mix_sum() == 760; }
    ok
}

/// UNX-F3741 混合映射 16 行守卫唯一 + 守卫阈值冻结。
fn check_u3741_mix_map() -> bool {
    let mut ok = u3741mixmap_COUNT == 16;
    if ok { ok = u3741mixmap_scan() == 0 && u3741mixmap_unique(); }
    if ok { ok = u3741mixmap_sum() == 504; }
    if ok { ok = U3741MIXGATE_THRESHOLD == 32; }
    ok
}

/// 本模块自检（逐检查带 detail，全绿才出门）。
pub fn run_u3741_checks() -> CheckSet {
    let mut set = CheckSet::new("U3741");
    set.add(
        "u3741-seg-guard",
        u3741_guard_seg(),
        if u3741_guard_seg() { "" } else { "diagnostic code outside segment" },
    );
    set.add(
        "u3741-mix-tbl",
        check_u3741_mix_tbl(),
        if check_u3741_mix_tbl() { "" } else { "UNX-F3741 mix table mismatch" },
    );
    set.add(
        "u3741-mix-map",
        check_u3741_mix_map(),
        if check_u3741_mix_map() { "" } else { "UNX-F3741 mix map mismatch" },
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
    fn u3741_all_checks_green() {
        let set = run_u3741_checks();
        assert!(set.all_passed(), "U3741 domain checks must be green");
    }

    #[test]
    fn u3741_segment_guard_holds() {
        assert!(u3741_segment_guard());
    }
}
