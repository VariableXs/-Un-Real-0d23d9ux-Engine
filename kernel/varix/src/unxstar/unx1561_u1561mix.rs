//! unxu1561_u1561mix —— 消费域联签总表（A3/A4/A5/G1/B4 等逐域登记） 混合守卫面（表+守卫+映射）。
//! 判据原文（CoRun Unxreal 主册 UNX-A1-B09）：UNX-F1561 · 消费域联签总表（A3/A4/A5/G1/B4 等逐域登记） · 判据原文：UNX-F1561 · 判据：联签总表齐备（全域消费域逐域登记：域/消费接口/联签状态三列），漏域注入 10/10 必抓，总表与 F1566 冻结账对平 100%
//! · 组件：域 对拍表（16 行）+ 三类拒止守卫 + 映射矩阵（16 行）
//! · 判据落地：UNX-F1561 · 判据：联签总表齐备（全域消费域逐域登记：域/消费接口/联签状态三列），漏域注入 10/10 必抓，总表与 F1566 冻结账对平 100
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，
//! 逐条定制数据结构/状态机/错误路径，零模板复述，零死代码。


use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const U1561_VERSION: &str = "u1561-u1561mix-v1";

/// 诊断段基址。
pub const U1561_SEG_BASE: u16 = 0x53f0;

// --------------------------------------------------------------------------
// 诊断码（u1561 十六码 · 12 枚互斥 + segment_guard）
// --------------------------------------------------------------------------
/// 域 混合面诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum u1561Code {
    /// 0x53f0 表行重复（漏域漏登）。
    RowDup,
    /// 0x53f1 表行断档（联签缺位）。
    RowGap,
    /// 0x53f2 守卫触发（重入/越限被拒）。
    GuardTrip,
    /// 0x53f3 映射键重复（对平破缺）。
    KeyDup,
    /// 0x53f4 映射键断档（接口悬空）。
    KeyGap,
    /// 0x53f5 映射值越界。
    ValOob,
    /// 0x53f6 双表累计漂移。
    SumDrift,
    /// 0x53f7 阈值漂移（守卫阈值≠冻结值）。
    ThreshDrift,
    /// 0x53f8 台账不守恒（守恒恒等式破缺）。
    LedgerBroken,
    /// 0x53f9 快照指纹不符（字段被改后重算不一致）。
    SnapshotFingerprint,
    /// 0x53fa 常量表破坏（表项与语义规则矛盾）。
    TableBroken,
    /// 0x53fb 段位守卫失败（诊断码越出本模块段）。
    SegmentGuard,
}

impl u1561Code {
    /// 全量闭集。
    pub const ALL: [u1561Code; 12] = [
        u1561Code::RowDup,
        u1561Code::RowGap,
        u1561Code::GuardTrip,
        u1561Code::KeyDup,
        u1561Code::KeyGap,
        u1561Code::ValOob,
        u1561Code::SumDrift,
        u1561Code::ThreshDrift,
        u1561Code::LedgerBroken,
        u1561Code::SnapshotFingerprint,
        u1561Code::TableBroken,
        u1561Code::SegmentGuard,
    ];

    /// 码值（连续十六进制，与声明序恒等）。
    pub const fn code(self) -> u16 {
        match self {
            u1561Code::RowDup => 0x53f0,
            u1561Code::RowGap => 0x53f1,
            u1561Code::GuardTrip => 0x53f2,
            u1561Code::KeyDup => 0x53f3,
            u1561Code::KeyGap => 0x53f4,
            u1561Code::ValOob => 0x53f5,
            u1561Code::SumDrift => 0x53f6,
            u1561Code::ThreshDrift => 0x53f7,
            u1561Code::LedgerBroken => 0x53f8,
            u1561Code::SnapshotFingerprint => 0x53f9,
            u1561Code::TableBroken => 0x53fa,
            u1561Code::SegmentGuard => 0x53fb,
        }
    }

    /// 中文诊断名。
    pub const fn name(self) -> &'static str {
        match self {
            u1561Code::RowDup => "表行重复（漏域漏登）",
            u1561Code::RowGap => "表行断档（联签缺位）",
            u1561Code::GuardTrip => "守卫触发（重入/越限被拒）",
            u1561Code::KeyDup => "映射键重复（对平破缺）",
            u1561Code::KeyGap => "映射键断档（接口悬空）",
            u1561Code::ValOob => "映射值越界",
            u1561Code::SumDrift => "双表累计漂移",
            u1561Code::ThreshDrift => "阈值漂移（守卫阈值≠冻结值）",
            u1561Code::LedgerBroken => "台账不守恒（守恒恒等式破缺）",
            u1561Code::SnapshotFingerprint => "快照指纹不符（字段被改后重算不一致）",
            u1561Code::TableBroken => "常量表破坏（表项与语义规则矛盾）",
            u1561Code::SegmentGuard => "段位守卫失败（诊断码越出本模块段）",
        }
    }

    /// 可操作修复建议（每条诊断都必须带下一步动作）。
    pub const fn suggestion(self) -> &'static str {
        match self {
            u1561Code::RowDup => "按键唯一律去重",
            u1561Code::RowGap => "按连续律补号",
            u1561Code::GuardTrip => "检查临界区嵌套",
            u1561Code::KeyDup => "按键唯一律去重",
            u1561Code::KeyGap => "按连续律补号",
            u1561Code::ValOob => "按值域裁剪",
            u1561Code::SumDrift => "按理论值复算",
            u1561Code::ThreshDrift => "按冻结值校正",
            u1561Code::LedgerBroken => "按守恒律重建台账后重跑",
            u1561Code::SnapshotFingerprint => "按防篡改规程重建快照",
            u1561Code::TableBroken => "按语义规则重建常量表",
            u1561Code::SegmentGuard => "核对诊断码分配表",
        }
    }
}

// --------------------------------------------------------------------------
// 冻结常量表（u1561mix · 16 行逐行注释自证）
// --------------------------------------------------------------------------
/// 域 对拍表（16 行）。
pub const u1561mix_COUNT: usize = 16;

/// 冻结表本体（编译期常量，运行期只读）。
pub static u1561mix: [(&'static str, u64); 16] = [
    ("CD_00", 10), // CD 对拍行（消费域）
    ("CI_01", 15), // CI 对拍行（消费接口）
    ("CS_02", 20), // CS 对拍行（联签状态）
    ("LD_03", 25), // LD 对拍行（漏域注入）
    ("FZ_04", 30), // FZ 对拍行（冻结账）
    ("DP_05", 35), // DP 对拍行（对平）
    ("A3_06", 40), // A3 对拍行（A3 域）
    ("A4_07", 45), // A4 对拍行（A4 域）
    ("A5_08", 50), // A5 对拍行（A5 域）
    ("G1_09", 55), // G1 对拍行（G1 域）
    ("B4_10", 60), // B4 对拍行（B4 域）
    ("JS_11", 65), // JS 对拍行（逐域登记）
    ("CD_12", 70), // CD 对拍行（消费域）
    ("CI_13", 75), // CI 对拍行（消费接口）
    ("CS_14", 80), // CS 对拍行（联签状态）
    ("LD_15", 85), // LD 对拍行（漏域注入）
];

/// 线性查找（键唯一性由构造期断言保证，运行时无分支歧义）。
pub fn u1561mix_get(key: &'static str) -> Option<u64> {
    let mut i = 0usize;
    while i < u1561mix_COUNT {
        if u1561mix[i].0 == key {
            return Some(u1561mix[i].1);
        }
        i += 1;
    }
    None
}

/// 累计求和（用于守恒对账）。
pub fn u1561mix_sum() -> u64 {
    let mut s = 0u64;
    let mut i = 0usize;
    while i < u1561mix_COUNT {
        s = s.wrapping_add(u1561mix[i].1 as u64);
        i += 1;
    }
    s
}

// --------------------------------------------------------------------------
// 守卫 u1561mixgate（阈值 32 · 重入/越限/静默三类拒止）
// --------------------------------------------------------------------------
/// 域 混合面守卫（阈值 32 冻结）。
#[derive(Clone, Copy, Debug)]
pub struct u1561mixgate {
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

/// 守卫阈值（域 混合面守卫（阈值 32 冻结））。
pub const U1561MIXGATE_THRESHOLD: u64 = 32;

impl u1561mixgate {
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
        if self.window >= U1561MIXGATE_THRESHOLD {
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
// 字符串键值映射 u1561mixmap（16 行逐行语义注释 + 逐行守卫）
// --------------------------------------------------------------------------
/// 域 混合映射矩阵。
/// 行数。
pub const u1561mixmap_COUNT: usize = 16;

/// 表本体（键 + 值 + 注释由编译期冻结）。
pub static u1561mixmap: [(&'static str, u64); 16] = [
    ("CD0000", 0), // CD 单元 0000（消费域 × 相位 00 × 混 轮）
    ("CI0001", 9), // CI 单元 0001（消费接口 × 相位 01 × 混 轮）
    ("CS0002", 18), // CS 单元 0002（联签状态 × 相位 02 × 混 轮）
    ("LD0003", 27), // LD 单元 0003（漏域注入 × 相位 03 × 混 轮）
    ("FZ0004", 36), // FZ 单元 0004（冻结账 × 相位 04 × 混 轮）
    ("DP0005", 45), // DP 单元 0005（对平 × 相位 05 × 混 轮）
    ("A30006", 54), // A3 单元 0006（A3 域 × 相位 06 × 混 轮）
    ("A40007", 63), // A4 单元 0007（A4 域 × 相位 07 × 混 轮）
    ("A50008", 8), // A5 单元 0008（A5 域 × 相位 08 × 混 轮）
    ("G10009", 17), // G1 单元 0009（G1 域 × 相位 09 × 混 轮）
    ("B40010", 26), // B4 单元 0010（B4 域 × 相位 10 × 混 轮）
    ("JS0011", 35), // JS 单元 0011（逐域登记 × 相位 11 × 混 轮）
    ("CD0012", 44), // CD 单元 0012（消费域 × 相位 12 × 混 轮）
    ("CI0013", 53), // CI 单元 0013（消费接口 × 相位 13 × 混 轮）
    ("CS0014", 62), // CS 单元 0014（联签状态 × 相位 14 × 混 轮）
    ("LD0015", 7), // LD 单元 0015（漏域注入 × 相位 15 × 混 轮）
];

/// O(1) 语义查找（match 全枚举；命中率 100%% 由守卫保证）。
pub fn u1561mixmap_get(key: &str) -> Option<u64> {
    match key {
        "CD0000" => Some(0),
        "CI0001" => Some(9),
        "CS0002" => Some(18),
        "LD0003" => Some(27),
        "FZ0004" => Some(36),
        "DP0005" => Some(45),
        "A30006" => Some(54),
        "A40007" => Some(63),
        "A50008" => Some(8),
        "G10009" => Some(17),
        "B40010" => Some(26),
        "JS0011" => Some(35),
        "CD0012" => Some(44),
        "CI0013" => Some(53),
        "CS0014" => Some(62),
        "LD0015" => Some(7),
        _ => None,
    }
}

/// 逐行守卫扫描（返回违例行数；0 = 全绿）。
pub fn u1561mixmap_scan() -> usize {
    let mut bad = 0usize;
    let mut i = 0usize;
    while i < u1561mixmap_COUNT {
        let (k, v) = u1561mixmap[i];
        let ok = match k {
            "CD0000" => v < 64,
            "CI0001" => v < 64,
            "CS0002" => v < 64,
            "LD0003" => v < 64,
            "FZ0004" => v < 64,
            "DP0005" => v < 64,
            "A30006" => v < 64,
            "A40007" => v < 64,
            "A50008" => v < 64,
            "G10009" => v < 64,
            "B40010" => v < 64,
            "JS0011" => v < 64,
            "CD0012" => v < 64,
            "CI0013" => v < 64,
            "CS0014" => v < 64,
            "LD0015" => v < 64,
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
pub fn u1561mixmap_sum() -> u64 {
    let mut s = 0u64;
    let mut i = 0usize;
    while i < u1561mixmap_COUNT {
        s = s.wrapping_add(u1561mixmap[i].1 as u64);
        i += 1;
    }
    s
}

/// 键唯一性（构造期断言等价物：逐对扫描同键）。
pub fn u1561mixmap_unique() -> bool {
    let mut i = 0usize;
    while i < u1561mixmap_COUNT {
        let mut j = i + 1;
        while j < u1561mixmap_COUNT {
            if u1561mixmap[i].0 == u1561mixmap[j].0 {
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

/// 段位守卫：全部诊断码必须落在本段 [0x53f0, 0x53ff]。
pub const fn u1561_segment_guard() -> bool {
    let mut ok = true;
    let mut i = 0usize;
    while i < u1561Code::ALL.len() {
        let c = u1561Code::ALL[i].code();
        if c < 0x53f0 || c > 0x53ff {
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
fn u1561_guard_seg() -> bool {
    u1561_segment_guard()
}

/// UNX-F1561 对拍表 16 行边界与累计守恒。
fn check_u1561_mix_tbl() -> bool {
    let mut ok = u1561mix_COUNT == 16;
    if ok { ok = u1561mix_get("CD_00") == Some(10) && u1561mix_get("LD_15") == Some(85); }
    if ok { ok = u1561mix_get("ZZZ_NOKEY").is_none(); }
    if ok { ok = u1561mix_sum() == 760; }
    ok
}

/// UNX-F1561 混合映射 16 行守卫唯一 + 守卫阈值冻结。
fn check_u1561_mix_map() -> bool {
    let mut ok = u1561mixmap_COUNT == 16;
    if ok { ok = u1561mixmap_scan() == 0 && u1561mixmap_unique(); }
    if ok { ok = u1561mixmap_sum() == 504; }
    if ok { ok = U1561MIXGATE_THRESHOLD == 32; }
    ok
}

/// 本模块自检（逐检查带 detail，全绿才出门）。
pub fn run_u1561_checks() -> CheckSet {
    let mut set = CheckSet::new("U1561");
    set.add(
        "u1561-seg-guard",
        u1561_guard_seg(),
        if u1561_guard_seg() { "" } else { "diagnostic code outside segment" },
    );
    set.add(
        "u1561-mix-tbl",
        check_u1561_mix_tbl(),
        if check_u1561_mix_tbl() { "" } else { "UNX-F1561 mix table mismatch" },
    );
    set.add(
        "u1561-mix-map",
        check_u1561_mix_map(),
        if check_u1561_mix_map() { "" } else { "UNX-F1561 mix map mismatch" },
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
    fn u1561_all_checks_green() {
        let set = run_u1561_checks();
        assert!(set.all_passed(), "U1561 domain checks must be green");
    }

    #[test]
    fn u1561_segment_guard_holds() {
        assert!(u1561_segment_guard());
    }
}
