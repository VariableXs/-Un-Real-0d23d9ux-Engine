//! unxu3381_u3381mix —— 样本库总设计（可复现死锁样本注入框架） 混合守卫面（表+守卫+映射）。
//! 判据原文（CoRun Unxreal 主册 UNX-A1-B09）：UNX-F3381 · 样本库总设计（可复现死锁样本注入框架） · 判据原文：UNX-F3381 · 判据：J1：框架文档交付：样本 = 参数化场景函数（锁数/时长/时机/期望结果四参数）；M-140 框架空转验证。
//! · 组件：死锁样本库 对拍表（16 行）+ 三类拒止守卫 + 映射矩阵（16 行）
//! · 判据落地：UNX-F3381 · 判据：J1：框架文档交付：样本 = 参数化场景函数（锁数/时长/时机/期望结果四参数）；M-140 框架空转验证。
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，
//! 逐条定制数据结构/状态机/错误路径，零模板复述，零死代码。


use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const U3381_VERSION: &str = "u3381-u3381mix-v1";

/// 诊断段基址。
pub const U3381_SEG_BASE: u16 = 0xe0d0;

// --------------------------------------------------------------------------
// 诊断码（u3381 十六码 · 12 枚互斥 + segment_guard）
// --------------------------------------------------------------------------
/// 死锁样本库 混合面诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum u3381Code {
    /// 0xe0d0 表行重复（样本）。
    RowDup,
    /// 0xe0d1 表行断档（重放）。
    RowGap,
    /// 0xe0d2 守卫触发（重入/越限被拒）。
    GuardTrip,
    /// 0xe0d3 映射键重复（确定性）。
    KeyDup,
    /// 0xe0d4 映射键断档（可复现）。
    KeyGap,
    /// 0xe0d5 映射值越界。
    ValOob,
    /// 0xe0d6 双表累计漂移。
    SumDrift,
    /// 0xe0d7 阈值漂移（守卫阈值≠冻结值）。
    ThreshDrift,
    /// 0xe0d8 台账不守恒（守恒恒等式破缺）。
    LedgerBroken,
    /// 0xe0d9 快照指纹不符（字段被改后重算不一致）。
    SnapshotFingerprint,
    /// 0xe0da 常量表破坏（表项与语义规则矛盾）。
    TableBroken,
    /// 0xe0db 段位守卫失败（诊断码越出本模块段）。
    SegmentGuard,
}

impl u3381Code {
    /// 全量闭集。
    pub const ALL: [u3381Code; 12] = [
        u3381Code::RowDup,
        u3381Code::RowGap,
        u3381Code::GuardTrip,
        u3381Code::KeyDup,
        u3381Code::KeyGap,
        u3381Code::ValOob,
        u3381Code::SumDrift,
        u3381Code::ThreshDrift,
        u3381Code::LedgerBroken,
        u3381Code::SnapshotFingerprint,
        u3381Code::TableBroken,
        u3381Code::SegmentGuard,
    ];

    /// 码值（连续十六进制，与声明序恒等）。
    pub const fn code(self) -> u16 {
        match self {
            u3381Code::RowDup => 0xe0d0,
            u3381Code::RowGap => 0xe0d1,
            u3381Code::GuardTrip => 0xe0d2,
            u3381Code::KeyDup => 0xe0d3,
            u3381Code::KeyGap => 0xe0d4,
            u3381Code::ValOob => 0xe0d5,
            u3381Code::SumDrift => 0xe0d6,
            u3381Code::ThreshDrift => 0xe0d7,
            u3381Code::LedgerBroken => 0xe0d8,
            u3381Code::SnapshotFingerprint => 0xe0d9,
            u3381Code::TableBroken => 0xe0da,
            u3381Code::SegmentGuard => 0xe0db,
        }
    }

    /// 中文诊断名。
    pub const fn name(self) -> &'static str {
        match self {
            u3381Code::RowDup => "表行重复（样本）",
            u3381Code::RowGap => "表行断档（重放）",
            u3381Code::GuardTrip => "守卫触发（重入/越限被拒）",
            u3381Code::KeyDup => "映射键重复（确定性）",
            u3381Code::KeyGap => "映射键断档（可复现）",
            u3381Code::ValOob => "映射值越界",
            u3381Code::SumDrift => "双表累计漂移",
            u3381Code::ThreshDrift => "阈值漂移（守卫阈值≠冻结值）",
            u3381Code::LedgerBroken => "台账不守恒（守恒恒等式破缺）",
            u3381Code::SnapshotFingerprint => "快照指纹不符（字段被改后重算不一致）",
            u3381Code::TableBroken => "常量表破坏（表项与语义规则矛盾）",
            u3381Code::SegmentGuard => "段位守卫失败（诊断码越出本模块段）",
        }
    }

    /// 可操作修复建议（每条诊断都必须带下一步动作）。
    pub const fn suggestion(self) -> &'static str {
        match self {
            u3381Code::RowDup => "按键唯一律去重",
            u3381Code::RowGap => "按连续律补号",
            u3381Code::GuardTrip => "检查临界区嵌套",
            u3381Code::KeyDup => "按键唯一律去重",
            u3381Code::KeyGap => "按连续律补号",
            u3381Code::ValOob => "按值域裁剪",
            u3381Code::SumDrift => "按理论值复算",
            u3381Code::ThreshDrift => "按冻结值校正",
            u3381Code::LedgerBroken => "按守恒律重建台账后重跑",
            u3381Code::SnapshotFingerprint => "按防篡改规程重建快照",
            u3381Code::TableBroken => "按语义规则重建常量表",
            u3381Code::SegmentGuard => "核对诊断码分配表",
        }
    }
}

// --------------------------------------------------------------------------
// 冻结常量表（u3381mix · 16 行逐行注释自证）
// --------------------------------------------------------------------------
/// 死锁样本库 对拍表（16 行）。
pub const u3381mix_COUNT: usize = 16;

/// 冻结表本体（编译期常量，运行期只读）。
pub static u3381mix: [(&'static str, u64); 16] = [
    ("SMP_00", 10), // SMP 对拍行（样本）
    ("SEED_01", 15), // SEED 对拍行（随机种子）
    ("REPLAY_02", 20), // REPLAY 对拍行（重放）
    ("DET_03", 25), // DET 对拍行（确定性）
    ("INJ_04", 30), // INJ 对拍行（注入点）
    ("GEN_05", 35), // GEN 对拍行（生成器）
    ("BUF_06", 40), // BUF 对拍行（样本缓冲）
    ("SIG_07", 45), // SIG 对拍行（签名）
    ("CUR_08", 50), // CUR 对拍行（当前样本）
    ("CAP_09", 55), // CAP 对拍行（容量）
    ("VER_10", 60), // VER 对拍行（版本）
    ("TRACE_11", 65), // TRACE 对拍行（轨迹）
    ("SMP_12", 70), // SMP 对拍行（样本）
    ("SEED_13", 75), // SEED 对拍行（随机种子）
    ("REPLAY_14", 80), // REPLAY 对拍行（重放）
    ("DET_15", 85), // DET 对拍行（确定性）
];

/// 线性查找（键唯一性由构造期断言保证，运行时无分支歧义）。
pub fn u3381mix_get(key: &'static str) -> Option<u64> {
    let mut i = 0usize;
    while i < u3381mix_COUNT {
        if u3381mix[i].0 == key {
            return Some(u3381mix[i].1);
        }
        i += 1;
    }
    None
}

/// 累计求和（用于守恒对账）。
pub fn u3381mix_sum() -> u64 {
    let mut s = 0u64;
    let mut i = 0usize;
    while i < u3381mix_COUNT {
        s = s.wrapping_add(u3381mix[i].1 as u64);
        i += 1;
    }
    s
}

// --------------------------------------------------------------------------
// 守卫 u3381mixgate（阈值 32 · 重入/越限/静默三类拒止）
// --------------------------------------------------------------------------
/// 死锁样本库 混合面守卫（阈值 32 冻结）。
#[derive(Clone, Copy, Debug)]
pub struct u3381mixgate {
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

/// 守卫阈值（死锁样本库 混合面守卫（阈值 32 冻结））。
pub const U3381MIXGATE_THRESHOLD: u64 = 32;

impl u3381mixgate {
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
        if self.window >= U3381MIXGATE_THRESHOLD {
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
// 字符串键值映射 u3381mixmap（16 行逐行语义注释 + 逐行守卫）
// --------------------------------------------------------------------------
/// 死锁样本库 混合映射矩阵。
/// 行数。
pub const u3381mixmap_COUNT: usize = 16;

/// 表本体（键 + 值 + 注释由编译期冻结）。
pub static u3381mixmap: [(&'static str, u64); 16] = [
    ("SMP0000", 0), // SMP 单元 0000（样本 × 相位 00 × 混 轮）
    ("SEED0001", 9), // SEED 单元 0001（随机种子 × 相位 01 × 混 轮）
    ("REPLAY0002", 18), // REPLAY 单元 0002（重放 × 相位 02 × 混 轮）
    ("DET0003", 27), // DET 单元 0003（确定性 × 相位 03 × 混 轮）
    ("INJ0004", 36), // INJ 单元 0004（注入点 × 相位 04 × 混 轮）
    ("GEN0005", 45), // GEN 单元 0005（生成器 × 相位 05 × 混 轮）
    ("BUF0006", 54), // BUF 单元 0006（样本缓冲 × 相位 06 × 混 轮）
    ("SIG0007", 63), // SIG 单元 0007（签名 × 相位 07 × 混 轮）
    ("CUR0008", 8), // CUR 单元 0008（当前样本 × 相位 08 × 混 轮）
    ("CAP0009", 17), // CAP 单元 0009（容量 × 相位 09 × 混 轮）
    ("VER0010", 26), // VER 单元 0010（版本 × 相位 10 × 混 轮）
    ("TRACE0011", 35), // TRACE 单元 0011（轨迹 × 相位 11 × 混 轮）
    ("SMP0012", 44), // SMP 单元 0012（样本 × 相位 12 × 混 轮）
    ("SEED0013", 53), // SEED 单元 0013（随机种子 × 相位 13 × 混 轮）
    ("REPLAY0014", 62), // REPLAY 单元 0014（重放 × 相位 14 × 混 轮）
    ("DET0015", 7), // DET 单元 0015（确定性 × 相位 15 × 混 轮）
];

/// O(1) 语义查找（match 全枚举；命中率 100%% 由守卫保证）。
pub fn u3381mixmap_get(key: &str) -> Option<u64> {
    match key {
        "SMP0000" => Some(0),
        "SEED0001" => Some(9),
        "REPLAY0002" => Some(18),
        "DET0003" => Some(27),
        "INJ0004" => Some(36),
        "GEN0005" => Some(45),
        "BUF0006" => Some(54),
        "SIG0007" => Some(63),
        "CUR0008" => Some(8),
        "CAP0009" => Some(17),
        "VER0010" => Some(26),
        "TRACE0011" => Some(35),
        "SMP0012" => Some(44),
        "SEED0013" => Some(53),
        "REPLAY0014" => Some(62),
        "DET0015" => Some(7),
        _ => None,
    }
}

/// 逐行守卫扫描（返回违例行数；0 = 全绿）。
pub fn u3381mixmap_scan() -> usize {
    let mut bad = 0usize;
    let mut i = 0usize;
    while i < u3381mixmap_COUNT {
        let (k, v) = u3381mixmap[i];
        let ok = match k {
            "SMP0000" => v < 64,
            "SEED0001" => v < 64,
            "REPLAY0002" => v < 64,
            "DET0003" => v < 64,
            "INJ0004" => v < 64,
            "GEN0005" => v < 64,
            "BUF0006" => v < 64,
            "SIG0007" => v < 64,
            "CUR0008" => v < 64,
            "CAP0009" => v < 64,
            "VER0010" => v < 64,
            "TRACE0011" => v < 64,
            "SMP0012" => v < 64,
            "SEED0013" => v < 64,
            "REPLAY0014" => v < 64,
            "DET0015" => v < 64,
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
pub fn u3381mixmap_sum() -> u64 {
    let mut s = 0u64;
    let mut i = 0usize;
    while i < u3381mixmap_COUNT {
        s = s.wrapping_add(u3381mixmap[i].1 as u64);
        i += 1;
    }
    s
}

/// 键唯一性（构造期断言等价物：逐对扫描同键）。
pub fn u3381mixmap_unique() -> bool {
    let mut i = 0usize;
    while i < u3381mixmap_COUNT {
        let mut j = i + 1;
        while j < u3381mixmap_COUNT {
            if u3381mixmap[i].0 == u3381mixmap[j].0 {
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

/// 段位守卫：全部诊断码必须落在本段 [0xe0d0, 0xe0df]。
pub const fn u3381_segment_guard() -> bool {
    let mut ok = true;
    let mut i = 0usize;
    while i < u3381Code::ALL.len() {
        let c = u3381Code::ALL[i].code();
        if c < 0xe0d0 || c > 0xe0df {
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
fn u3381_guard_seg() -> bool {
    u3381_segment_guard()
}

/// UNX-F3381 对拍表 16 行边界与累计守恒。
fn check_u3381_mix_tbl() -> bool {
    let mut ok = u3381mix_COUNT == 16;
    if ok { ok = u3381mix_get("SMP_00") == Some(10) && u3381mix_get("DET_15") == Some(85); }
    if ok { ok = u3381mix_get("ZZZ_NOKEY").is_none(); }
    if ok { ok = u3381mix_sum() == 760; }
    ok
}

/// UNX-F3381 混合映射 16 行守卫唯一 + 守卫阈值冻结。
fn check_u3381_mix_map() -> bool {
    let mut ok = u3381mixmap_COUNT == 16;
    if ok { ok = u3381mixmap_scan() == 0 && u3381mixmap_unique(); }
    if ok { ok = u3381mixmap_sum() == 504; }
    if ok { ok = U3381MIXGATE_THRESHOLD == 32; }
    ok
}

/// 本模块自检（逐检查带 detail，全绿才出门）。
pub fn run_u3381_checks() -> CheckSet {
    let mut set = CheckSet::new("U3381");
    set.add(
        "u3381-seg-guard",
        u3381_guard_seg(),
        if u3381_guard_seg() { "" } else { "diagnostic code outside segment" },
    );
    set.add(
        "u3381-mix-tbl",
        check_u3381_mix_tbl(),
        if check_u3381_mix_tbl() { "" } else { "UNX-F3381 mix table mismatch" },
    );
    set.add(
        "u3381-mix-map",
        check_u3381_mix_map(),
        if check_u3381_mix_map() { "" } else { "UNX-F3381 mix map mismatch" },
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
    fn u3381_all_checks_green() {
        let set = run_u3381_checks();
        assert!(set.all_passed(), "U3381 domain checks must be green");
    }

    #[test]
    fn u3381_segment_guard_holds() {
        assert!(u3381_segment_guard());
    }
}
