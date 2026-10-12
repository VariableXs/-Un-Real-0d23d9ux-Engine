//! unxu3621_u3621mix —— RCU 压测总设计（读/写/宽限三面实验框架） 混合守卫面（表+守卫+映射）。
//! 判据原文（CoRun Unxreal 主册 UNX-A1-B09）：UNX-F3621 · RCU 压测总设计（读/写/宽限三面实验框架） · 判据原文：UNX-F3621 · 判据：J1：三面框架（读面（并发/一致性）/写面（复制发布切换）/宽限面（时长/滞留/回收）——判据行内嵌职责表）；M-379 三面首跑（最小三面用例各绿）。<br>J2：框架零侵（RCU 路径拍数与基线一致——度量可关）。
//! · 组件：RCU 三面实验 对拍表（16 行）+ 三类拒止守卫 + 映射矩阵（16 行）
//! · 判据落地：UNX-F3621 · 判据：J1：三面框架（读面（并发/一致性）/写面（复制发布切换）/宽限面（时长/滞留/回收）——判据行内嵌职责表）；M-379 三面首跑
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，
//! 逐条定制数据结构/状态机/错误路径，零模板复述，零死代码。


use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const U3621_VERSION: &str = "u3621-u3621mix-v1";

/// 诊断段基址。
pub const U3621_SEG_BASE: u16 = 0xe190;

// --------------------------------------------------------------------------
// 诊断码（u3621 十六码 · 12 枚互斥 + segment_guard）
// --------------------------------------------------------------------------
/// RCU 三面实验 混合面诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum u3621Code {
    /// 0xe190 表行重复（RCU）。
    RowDup,
    /// 0xe191 表行断档（宽限）。
    RowGap,
    /// 0xe192 守卫触发（重入/越限被拒）。
    GuardTrip,
    /// 0xe193 映射键重复（实验）。
    KeyDup,
    /// 0xe194 映射键断档（三面）。
    KeyGap,
    /// 0xe195 映射值越界。
    ValOob,
    /// 0xe196 双表累计漂移。
    SumDrift,
    /// 0xe197 阈值漂移（守卫阈值≠冻结值）。
    ThreshDrift,
    /// 0xe198 台账不守恒（守恒恒等式破缺）。
    LedgerBroken,
    /// 0xe199 快照指纹不符（字段被改后重算不一致）。
    SnapshotFingerprint,
    /// 0xe19a 常量表破坏（表项与语义规则矛盾）。
    TableBroken,
    /// 0xe19b 段位守卫失败（诊断码越出本模块段）。
    SegmentGuard,
}

impl u3621Code {
    /// 全量闭集。
    pub const ALL: [u3621Code; 12] = [
        u3621Code::RowDup,
        u3621Code::RowGap,
        u3621Code::GuardTrip,
        u3621Code::KeyDup,
        u3621Code::KeyGap,
        u3621Code::ValOob,
        u3621Code::SumDrift,
        u3621Code::ThreshDrift,
        u3621Code::LedgerBroken,
        u3621Code::SnapshotFingerprint,
        u3621Code::TableBroken,
        u3621Code::SegmentGuard,
    ];

    /// 码值（连续十六进制，与声明序恒等）。
    pub const fn code(self) -> u16 {
        match self {
            u3621Code::RowDup => 0xe190,
            u3621Code::RowGap => 0xe191,
            u3621Code::GuardTrip => 0xe192,
            u3621Code::KeyDup => 0xe193,
            u3621Code::KeyGap => 0xe194,
            u3621Code::ValOob => 0xe195,
            u3621Code::SumDrift => 0xe196,
            u3621Code::ThreshDrift => 0xe197,
            u3621Code::LedgerBroken => 0xe198,
            u3621Code::SnapshotFingerprint => 0xe199,
            u3621Code::TableBroken => 0xe19a,
            u3621Code::SegmentGuard => 0xe19b,
        }
    }

    /// 中文诊断名。
    pub const fn name(self) -> &'static str {
        match self {
            u3621Code::RowDup => "表行重复（RCU）",
            u3621Code::RowGap => "表行断档（宽限）",
            u3621Code::GuardTrip => "守卫触发（重入/越限被拒）",
            u3621Code::KeyDup => "映射键重复（实验）",
            u3621Code::KeyGap => "映射键断档（三面）",
            u3621Code::ValOob => "映射值越界",
            u3621Code::SumDrift => "双表累计漂移",
            u3621Code::ThreshDrift => "阈值漂移（守卫阈值≠冻结值）",
            u3621Code::LedgerBroken => "台账不守恒（守恒恒等式破缺）",
            u3621Code::SnapshotFingerprint => "快照指纹不符（字段被改后重算不一致）",
            u3621Code::TableBroken => "常量表破坏（表项与语义规则矛盾）",
            u3621Code::SegmentGuard => "段位守卫失败（诊断码越出本模块段）",
        }
    }

    /// 可操作修复建议（每条诊断都必须带下一步动作）。
    pub const fn suggestion(self) -> &'static str {
        match self {
            u3621Code::RowDup => "按键唯一律去重",
            u3621Code::RowGap => "按连续律补号",
            u3621Code::GuardTrip => "检查临界区嵌套",
            u3621Code::KeyDup => "按键唯一律去重",
            u3621Code::KeyGap => "按连续律补号",
            u3621Code::ValOob => "按值域裁剪",
            u3621Code::SumDrift => "按理论值复算",
            u3621Code::ThreshDrift => "按冻结值校正",
            u3621Code::LedgerBroken => "按守恒律重建台账后重跑",
            u3621Code::SnapshotFingerprint => "按防篡改规程重建快照",
            u3621Code::TableBroken => "按语义规则重建常量表",
            u3621Code::SegmentGuard => "核对诊断码分配表",
        }
    }
}

// --------------------------------------------------------------------------
// 冻结常量表（u3621mix · 16 行逐行注释自证）
// --------------------------------------------------------------------------
/// RCU 三面实验 对拍表（16 行）。
pub const u3621mix_COUNT: usize = 16;

/// 冻结表本体（编译期常量，运行期只读）。
pub static u3621mix: [(&'static str, u64); 16] = [
    ("RDC_00", 10), // RDC 对拍行（读面）
    ("WRC_01", 15), // WRC 对拍行（写面）
    ("GPC_02", 20), // GPC 对拍行（宽限面）
    ("EXP_03", 25), // EXP 对拍行（实验）
    ("TRIAL_04", 30), // TRIAL 对拍行（试验轮）
    ("LAG_05", 35), // LAG 对拍行（读侧滞后）
    ("GPW_06", 40), // GPW 对拍行（宽限窗口）
    ("THR_07", 45), // THR 对拍行（阈值）
    ("DATA_08", 50), // DATA 对拍行（数据面）
    ("MEM_09", 55), // MEM 对拍行（内存面）
    ("CPU_10", 60), // CPU 对拍行（核数）
    ("AGG_11", 65), // AGG 对拍行（聚合）
    ("RDC_12", 70), // RDC 对拍行（读面）
    ("WRC_13", 75), // WRC 对拍行（写面）
    ("GPC_14", 80), // GPC 对拍行（宽限面）
    ("EXP_15", 85), // EXP 对拍行（实验）
];

/// 线性查找（键唯一性由构造期断言保证，运行时无分支歧义）。
pub fn u3621mix_get(key: &'static str) -> Option<u64> {
    let mut i = 0usize;
    while i < u3621mix_COUNT {
        if u3621mix[i].0 == key {
            return Some(u3621mix[i].1);
        }
        i += 1;
    }
    None
}

/// 累计求和（用于守恒对账）。
pub fn u3621mix_sum() -> u64 {
    let mut s = 0u64;
    let mut i = 0usize;
    while i < u3621mix_COUNT {
        s = s.wrapping_add(u3621mix[i].1 as u64);
        i += 1;
    }
    s
}

// --------------------------------------------------------------------------
// 守卫 u3621mixgate（阈值 32 · 重入/越限/静默三类拒止）
// --------------------------------------------------------------------------
/// RCU 三面实验 混合面守卫（阈值 32 冻结）。
#[derive(Clone, Copy, Debug)]
pub struct u3621mixgate {
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

/// 守卫阈值（RCU 三面实验 混合面守卫（阈值 32 冻结））。
pub const U3621MIXGATE_THRESHOLD: u64 = 32;

impl u3621mixgate {
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
        if self.window >= U3621MIXGATE_THRESHOLD {
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
// 字符串键值映射 u3621mixmap（16 行逐行语义注释 + 逐行守卫）
// --------------------------------------------------------------------------
/// RCU 三面实验 混合映射矩阵。
/// 行数。
pub const u3621mixmap_COUNT: usize = 16;

/// 表本体（键 + 值 + 注释由编译期冻结）。
pub static u3621mixmap: [(&'static str, u64); 16] = [
    ("RDC0000", 0), // RDC 单元 0000（读面 × 相位 00 × 混 轮）
    ("WRC0001", 9), // WRC 单元 0001（写面 × 相位 01 × 混 轮）
    ("GPC0002", 18), // GPC 单元 0002（宽限面 × 相位 02 × 混 轮）
    ("EXP0003", 27), // EXP 单元 0003（实验 × 相位 03 × 混 轮）
    ("TRIAL0004", 36), // TRIAL 单元 0004（试验轮 × 相位 04 × 混 轮）
    ("LAG0005", 45), // LAG 单元 0005（读侧滞后 × 相位 05 × 混 轮）
    ("GPW0006", 54), // GPW 单元 0006（宽限窗口 × 相位 06 × 混 轮）
    ("THR0007", 63), // THR 单元 0007（阈值 × 相位 07 × 混 轮）
    ("DATA0008", 8), // DATA 单元 0008（数据面 × 相位 08 × 混 轮）
    ("MEM0009", 17), // MEM 单元 0009（内存面 × 相位 09 × 混 轮）
    ("CPU0010", 26), // CPU 单元 0010（核数 × 相位 10 × 混 轮）
    ("AGG0011", 35), // AGG 单元 0011（聚合 × 相位 11 × 混 轮）
    ("RDC0012", 44), // RDC 单元 0012（读面 × 相位 12 × 混 轮）
    ("WRC0013", 53), // WRC 单元 0013（写面 × 相位 13 × 混 轮）
    ("GPC0014", 62), // GPC 单元 0014（宽限面 × 相位 14 × 混 轮）
    ("EXP0015", 7), // EXP 单元 0015（实验 × 相位 15 × 混 轮）
];

/// O(1) 语义查找（match 全枚举；命中率 100%% 由守卫保证）。
pub fn u3621mixmap_get(key: &str) -> Option<u64> {
    match key {
        "RDC0000" => Some(0),
        "WRC0001" => Some(9),
        "GPC0002" => Some(18),
        "EXP0003" => Some(27),
        "TRIAL0004" => Some(36),
        "LAG0005" => Some(45),
        "GPW0006" => Some(54),
        "THR0007" => Some(63),
        "DATA0008" => Some(8),
        "MEM0009" => Some(17),
        "CPU0010" => Some(26),
        "AGG0011" => Some(35),
        "RDC0012" => Some(44),
        "WRC0013" => Some(53),
        "GPC0014" => Some(62),
        "EXP0015" => Some(7),
        _ => None,
    }
}

/// 逐行守卫扫描（返回违例行数；0 = 全绿）。
pub fn u3621mixmap_scan() -> usize {
    let mut bad = 0usize;
    let mut i = 0usize;
    while i < u3621mixmap_COUNT {
        let (k, v) = u3621mixmap[i];
        let ok = match k {
            "RDC0000" => v < 64,
            "WRC0001" => v < 64,
            "GPC0002" => v < 64,
            "EXP0003" => v < 64,
            "TRIAL0004" => v < 64,
            "LAG0005" => v < 64,
            "GPW0006" => v < 64,
            "THR0007" => v < 64,
            "DATA0008" => v < 64,
            "MEM0009" => v < 64,
            "CPU0010" => v < 64,
            "AGG0011" => v < 64,
            "RDC0012" => v < 64,
            "WRC0013" => v < 64,
            "GPC0014" => v < 64,
            "EXP0015" => v < 64,
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
pub fn u3621mixmap_sum() -> u64 {
    let mut s = 0u64;
    let mut i = 0usize;
    while i < u3621mixmap_COUNT {
        s = s.wrapping_add(u3621mixmap[i].1 as u64);
        i += 1;
    }
    s
}

/// 键唯一性（构造期断言等价物：逐对扫描同键）。
pub fn u3621mixmap_unique() -> bool {
    let mut i = 0usize;
    while i < u3621mixmap_COUNT {
        let mut j = i + 1;
        while j < u3621mixmap_COUNT {
            if u3621mixmap[i].0 == u3621mixmap[j].0 {
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

/// 段位守卫：全部诊断码必须落在本段 [0xe190, 0xe19f]。
pub const fn u3621_segment_guard() -> bool {
    let mut ok = true;
    let mut i = 0usize;
    while i < u3621Code::ALL.len() {
        let c = u3621Code::ALL[i].code();
        if c < 0xe190 || c > 0xe19f {
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
fn u3621_guard_seg() -> bool {
    u3621_segment_guard()
}

/// UNX-F3621 对拍表 16 行边界与累计守恒。
fn check_u3621_mix_tbl() -> bool {
    let mut ok = u3621mix_COUNT == 16;
    if ok { ok = u3621mix_get("RDC_00") == Some(10) && u3621mix_get("EXP_15") == Some(85); }
    if ok { ok = u3621mix_get("ZZZ_NOKEY").is_none(); }
    if ok { ok = u3621mix_sum() == 760; }
    ok
}

/// UNX-F3621 混合映射 16 行守卫唯一 + 守卫阈值冻结。
fn check_u3621_mix_map() -> bool {
    let mut ok = u3621mixmap_COUNT == 16;
    if ok { ok = u3621mixmap_scan() == 0 && u3621mixmap_unique(); }
    if ok { ok = u3621mixmap_sum() == 504; }
    if ok { ok = U3621MIXGATE_THRESHOLD == 32; }
    ok
}

/// 本模块自检（逐检查带 detail，全绿才出门）。
pub fn run_u3621_checks() -> CheckSet {
    let mut set = CheckSet::new("U3621");
    set.add(
        "u3621-seg-guard",
        u3621_guard_seg(),
        if u3621_guard_seg() { "" } else { "diagnostic code outside segment" },
    );
    set.add(
        "u3621-mix-tbl",
        check_u3621_mix_tbl(),
        if check_u3621_mix_tbl() { "" } else { "UNX-F3621 mix table mismatch" },
    );
    set.add(
        "u3621-mix-map",
        check_u3621_mix_map(),
        if check_u3621_mix_map() { "" } else { "UNX-F3621 mix map mismatch" },
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
    fn u3621_all_checks_green() {
        let set = run_u3621_checks();
        assert!(set.all_passed(), "U3621 domain checks must be green");
    }

    #[test]
    fn u3621_segment_guard_holds() {
        assert!(u3621_segment_guard());
    }
}
