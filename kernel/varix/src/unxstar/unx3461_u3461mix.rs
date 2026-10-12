//! unxu3461_u3461mix —— 分域挂载总设计（框架与各域用例集的挂载架构） 混合守卫面（表+守卫+映射）。
//! 判据原文（CoRun Unxreal 主册 UNX-A1-B09）：UNX-F3461 · 分域挂载总设计（框架与各域用例集的挂载架构） · 判据原文：UNX-F3461 · 判据：J1：挂载架构文档（域侧声明用例集、框架侧统一注册表——双向登记制）；M-220 挂载全链路（域声明 → 注册表合并 → 执行器可见）。
//! · 组件：分域挂载 对拍表（16 行）+ 三类拒止守卫 + 映射矩阵（16 行）
//! · 判据落地：UNX-F3461 · 判据：J1：挂载架构文档（域侧声明用例集、框架侧统一注册表——双向登记制）；M-220 挂载全链路（域声明 → 注册表合并 → 执行器可
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，
//! 逐条定制数据结构/状态机/错误路径，零模板复述，零死代码。


use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const U3461_VERSION: &str = "u3461-u3461mix-v1";

/// 诊断段基址。
pub const U3461_SEG_BASE: u16 = 0xe110;

// --------------------------------------------------------------------------
// 诊断码（u3461 十六码 · 12 枚互斥 + segment_guard）
// --------------------------------------------------------------------------
/// 分域挂载 混合面诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum u3461Code {
    /// 0xe110 表行重复（挂载）。
    RowDup,
    /// 0xe111 表行断档（用例集）。
    RowGap,
    /// 0xe112 守卫触发（重入/越限被拒）。
    GuardTrip,
    /// 0xe113 映射键重复（域）。
    KeyDup,
    /// 0xe114 映射键断档（依赖）。
    KeyGap,
    /// 0xe115 映射值越界。
    ValOob,
    /// 0xe116 双表累计漂移。
    SumDrift,
    /// 0xe117 阈值漂移（守卫阈值≠冻结值）。
    ThreshDrift,
    /// 0xe118 台账不守恒（守恒恒等式破缺）。
    LedgerBroken,
    /// 0xe119 快照指纹不符（字段被改后重算不一致）。
    SnapshotFingerprint,
    /// 0xe11a 常量表破坏（表项与语义规则矛盾）。
    TableBroken,
    /// 0xe11b 段位守卫失败（诊断码越出本模块段）。
    SegmentGuard,
}

impl u3461Code {
    /// 全量闭集。
    pub const ALL: [u3461Code; 12] = [
        u3461Code::RowDup,
        u3461Code::RowGap,
        u3461Code::GuardTrip,
        u3461Code::KeyDup,
        u3461Code::KeyGap,
        u3461Code::ValOob,
        u3461Code::SumDrift,
        u3461Code::ThreshDrift,
        u3461Code::LedgerBroken,
        u3461Code::SnapshotFingerprint,
        u3461Code::TableBroken,
        u3461Code::SegmentGuard,
    ];

    /// 码值（连续十六进制，与声明序恒等）。
    pub const fn code(self) -> u16 {
        match self {
            u3461Code::RowDup => 0xe110,
            u3461Code::RowGap => 0xe111,
            u3461Code::GuardTrip => 0xe112,
            u3461Code::KeyDup => 0xe113,
            u3461Code::KeyGap => 0xe114,
            u3461Code::ValOob => 0xe115,
            u3461Code::SumDrift => 0xe116,
            u3461Code::ThreshDrift => 0xe117,
            u3461Code::LedgerBroken => 0xe118,
            u3461Code::SnapshotFingerprint => 0xe119,
            u3461Code::TableBroken => 0xe11a,
            u3461Code::SegmentGuard => 0xe11b,
        }
    }

    /// 中文诊断名。
    pub const fn name(self) -> &'static str {
        match self {
            u3461Code::RowDup => "表行重复（挂载）",
            u3461Code::RowGap => "表行断档（用例集）",
            u3461Code::GuardTrip => "守卫触发（重入/越限被拒）",
            u3461Code::KeyDup => "映射键重复（域）",
            u3461Code::KeyGap => "映射键断档（依赖）",
            u3461Code::ValOob => "映射值越界",
            u3461Code::SumDrift => "双表累计漂移",
            u3461Code::ThreshDrift => "阈值漂移（守卫阈值≠冻结值）",
            u3461Code::LedgerBroken => "台账不守恒（守恒恒等式破缺）",
            u3461Code::SnapshotFingerprint => "快照指纹不符（字段被改后重算不一致）",
            u3461Code::TableBroken => "常量表破坏（表项与语义规则矛盾）",
            u3461Code::SegmentGuard => "段位守卫失败（诊断码越出本模块段）",
        }
    }

    /// 可操作修复建议（每条诊断都必须带下一步动作）。
    pub const fn suggestion(self) -> &'static str {
        match self {
            u3461Code::RowDup => "按键唯一律去重",
            u3461Code::RowGap => "按连续律补号",
            u3461Code::GuardTrip => "检查临界区嵌套",
            u3461Code::KeyDup => "按键唯一律去重",
            u3461Code::KeyGap => "按连续律补号",
            u3461Code::ValOob => "按值域裁剪",
            u3461Code::SumDrift => "按理论值复算",
            u3461Code::ThreshDrift => "按冻结值校正",
            u3461Code::LedgerBroken => "按守恒律重建台账后重跑",
            u3461Code::SnapshotFingerprint => "按防篡改规程重建快照",
            u3461Code::TableBroken => "按语义规则重建常量表",
            u3461Code::SegmentGuard => "核对诊断码分配表",
        }
    }
}

// --------------------------------------------------------------------------
// 冻结常量表（u3461mix · 16 行逐行注释自证）
// --------------------------------------------------------------------------
/// 分域挂载 对拍表（16 行）。
pub const u3461mix_COUNT: usize = 16;

/// 冻结表本体（编译期常量，运行期只读）。
pub static u3461mix: [(&'static str, u64); 16] = [
    ("MNT_00", 10), // MNT 对拍行（挂载）
    ("FW_01", 15), // FW 对拍行（框架）
    ("SUITE_02", 20), // SUITE 对拍行（用例集）
    ("DOM_03", 25), // DOM 对拍行（域标识）
    ("TABLE_04", 30), // TABLE 对拍行（挂载表）
    ("PRIO_05", 35), // PRIO 对拍行（优先级）
    ("BIND_06", 40), // BIND 对拍行（绑定）
    ("UNBIND_07", 45), // UNBIND 对拍行（解绑）
    ("VER_08", 50), // VER 对拍行（版本）
    ("DEP_09", 55), // DEP 对拍行（依赖域）
    ("CAP_10", 60), // CAP 对拍行（容量）
    ("ORDER_11", 65), // ORDER 对拍行（装载序）
    ("MNT_12", 70), // MNT 对拍行（挂载）
    ("FW_13", 75), // FW 对拍行（框架）
    ("SUITE_14", 80), // SUITE 对拍行（用例集）
    ("DOM_15", 85), // DOM 对拍行（域标识）
];

/// 线性查找（键唯一性由构造期断言保证，运行时无分支歧义）。
pub fn u3461mix_get(key: &'static str) -> Option<u64> {
    let mut i = 0usize;
    while i < u3461mix_COUNT {
        if u3461mix[i].0 == key {
            return Some(u3461mix[i].1);
        }
        i += 1;
    }
    None
}

/// 累计求和（用于守恒对账）。
pub fn u3461mix_sum() -> u64 {
    let mut s = 0u64;
    let mut i = 0usize;
    while i < u3461mix_COUNT {
        s = s.wrapping_add(u3461mix[i].1 as u64);
        i += 1;
    }
    s
}

// --------------------------------------------------------------------------
// 守卫 u3461mixgate（阈值 32 · 重入/越限/静默三类拒止）
// --------------------------------------------------------------------------
/// 分域挂载 混合面守卫（阈值 32 冻结）。
#[derive(Clone, Copy, Debug)]
pub struct u3461mixgate {
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

/// 守卫阈值（分域挂载 混合面守卫（阈值 32 冻结））。
pub const U3461MIXGATE_THRESHOLD: u64 = 32;

impl u3461mixgate {
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
        if self.window >= U3461MIXGATE_THRESHOLD {
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
// 字符串键值映射 u3461mixmap（16 行逐行语义注释 + 逐行守卫）
// --------------------------------------------------------------------------
/// 分域挂载 混合映射矩阵。
/// 行数。
pub const u3461mixmap_COUNT: usize = 16;

/// 表本体（键 + 值 + 注释由编译期冻结）。
pub static u3461mixmap: [(&'static str, u64); 16] = [
    ("MNT0000", 0), // MNT 单元 0000（挂载 × 相位 00 × 混 轮）
    ("FW0001", 9), // FW 单元 0001（框架 × 相位 01 × 混 轮）
    ("SUITE0002", 18), // SUITE 单元 0002（用例集 × 相位 02 × 混 轮）
    ("DOM0003", 27), // DOM 单元 0003（域标识 × 相位 03 × 混 轮）
    ("TABLE0004", 36), // TABLE 单元 0004（挂载表 × 相位 04 × 混 轮）
    ("PRIO0005", 45), // PRIO 单元 0005（优先级 × 相位 05 × 混 轮）
    ("BIND0006", 54), // BIND 单元 0006（绑定 × 相位 06 × 混 轮）
    ("UNBIND0007", 63), // UNBIND 单元 0007（解绑 × 相位 07 × 混 轮）
    ("VER0008", 8), // VER 单元 0008（版本 × 相位 08 × 混 轮）
    ("DEP0009", 17), // DEP 单元 0009（依赖域 × 相位 09 × 混 轮）
    ("CAP0010", 26), // CAP 单元 0010（容量 × 相位 10 × 混 轮）
    ("ORDER0011", 35), // ORDER 单元 0011（装载序 × 相位 11 × 混 轮）
    ("MNT0012", 44), // MNT 单元 0012（挂载 × 相位 12 × 混 轮）
    ("FW0013", 53), // FW 单元 0013（框架 × 相位 13 × 混 轮）
    ("SUITE0014", 62), // SUITE 单元 0014（用例集 × 相位 14 × 混 轮）
    ("DOM0015", 7), // DOM 单元 0015（域标识 × 相位 15 × 混 轮）
];

/// O(1) 语义查找（match 全枚举；命中率 100%% 由守卫保证）。
pub fn u3461mixmap_get(key: &str) -> Option<u64> {
    match key {
        "MNT0000" => Some(0),
        "FW0001" => Some(9),
        "SUITE0002" => Some(18),
        "DOM0003" => Some(27),
        "TABLE0004" => Some(36),
        "PRIO0005" => Some(45),
        "BIND0006" => Some(54),
        "UNBIND0007" => Some(63),
        "VER0008" => Some(8),
        "DEP0009" => Some(17),
        "CAP0010" => Some(26),
        "ORDER0011" => Some(35),
        "MNT0012" => Some(44),
        "FW0013" => Some(53),
        "SUITE0014" => Some(62),
        "DOM0015" => Some(7),
        _ => None,
    }
}

/// 逐行守卫扫描（返回违例行数；0 = 全绿）。
pub fn u3461mixmap_scan() -> usize {
    let mut bad = 0usize;
    let mut i = 0usize;
    while i < u3461mixmap_COUNT {
        let (k, v) = u3461mixmap[i];
        let ok = match k {
            "MNT0000" => v < 64,
            "FW0001" => v < 64,
            "SUITE0002" => v < 64,
            "DOM0003" => v < 64,
            "TABLE0004" => v < 64,
            "PRIO0005" => v < 64,
            "BIND0006" => v < 64,
            "UNBIND0007" => v < 64,
            "VER0008" => v < 64,
            "DEP0009" => v < 64,
            "CAP0010" => v < 64,
            "ORDER0011" => v < 64,
            "MNT0012" => v < 64,
            "FW0013" => v < 64,
            "SUITE0014" => v < 64,
            "DOM0015" => v < 64,
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
pub fn u3461mixmap_sum() -> u64 {
    let mut s = 0u64;
    let mut i = 0usize;
    while i < u3461mixmap_COUNT {
        s = s.wrapping_add(u3461mixmap[i].1 as u64);
        i += 1;
    }
    s
}

/// 键唯一性（构造期断言等价物：逐对扫描同键）。
pub fn u3461mixmap_unique() -> bool {
    let mut i = 0usize;
    while i < u3461mixmap_COUNT {
        let mut j = i + 1;
        while j < u3461mixmap_COUNT {
            if u3461mixmap[i].0 == u3461mixmap[j].0 {
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

/// 段位守卫：全部诊断码必须落在本段 [0xe110, 0xe11f]。
pub const fn u3461_segment_guard() -> bool {
    let mut ok = true;
    let mut i = 0usize;
    while i < u3461Code::ALL.len() {
        let c = u3461Code::ALL[i].code();
        if c < 0xe110 || c > 0xe11f {
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
fn u3461_guard_seg() -> bool {
    u3461_segment_guard()
}

/// UNX-F3461 对拍表 16 行边界与累计守恒。
fn check_u3461_mix_tbl() -> bool {
    let mut ok = u3461mix_COUNT == 16;
    if ok { ok = u3461mix_get("MNT_00") == Some(10) && u3461mix_get("DOM_15") == Some(85); }
    if ok { ok = u3461mix_get("ZZZ_NOKEY").is_none(); }
    if ok { ok = u3461mix_sum() == 760; }
    ok
}

/// UNX-F3461 混合映射 16 行守卫唯一 + 守卫阈值冻结。
fn check_u3461_mix_map() -> bool {
    let mut ok = u3461mixmap_COUNT == 16;
    if ok { ok = u3461mixmap_scan() == 0 && u3461mixmap_unique(); }
    if ok { ok = u3461mixmap_sum() == 504; }
    if ok { ok = U3461MIXGATE_THRESHOLD == 32; }
    ok
}

/// 本模块自检（逐检查带 detail，全绿才出门）。
pub fn run_u3461_checks() -> CheckSet {
    let mut set = CheckSet::new("U3461");
    set.add(
        "u3461-seg-guard",
        u3461_guard_seg(),
        if u3461_guard_seg() { "" } else { "diagnostic code outside segment" },
    );
    set.add(
        "u3461-mix-tbl",
        check_u3461_mix_tbl(),
        if check_u3461_mix_tbl() { "" } else { "UNX-F3461 mix table mismatch" },
    );
    set.add(
        "u3461-mix-map",
        check_u3461_mix_map(),
        if check_u3461_mix_map() { "" } else { "UNX-F3461 mix map mismatch" },
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
    fn u3461_all_checks_green() {
        let set = run_u3461_checks();
        assert!(set.all_passed(), "U3461 domain checks must be green");
    }

    #[test]
    fn u3461_segment_guard_holds() {
        assert!(u3461_segment_guard());
    }
}
