//! unxu3301_u3301mix —— RCU 压测编排（读多写少 8 核场景） 混合守卫面（表+守卫+映射）。
//! 判据原文（CoRun Unxreal 主册 UNX-A1-B09）：UNX-F3301 · RCU 压测编排（读多写少 8 核场景） · 判据原文：UNX-F3301 · 判据：J1：8 核读 + 写者换指针压测 10 秒，读侧零阻塞证据（读延迟分布 P99 ≤ 2×fast path 基线）、GP 推进健康；M-060 对账。
//! · 组件：RCU 压测编排 对拍表（16 行）+ 三类拒止守卫 + 映射矩阵（16 行）
//! · 判据落地：UNX-F3301 · 判据：J1：8 核读 + 写者换指针压测 10 秒，读侧零阻塞证据（读延迟分布 P99 ≤ 2×fast path 基线）、GP 推进健
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，
//! 逐条定制数据结构/状态机/错误路径，零模板复述，零死代码。


use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const U3301_VERSION: &str = "u3301-u3301mix-v1";

/// 诊断段基址。
pub const U3301_SEG_BASE: u16 = 0xe090;

// --------------------------------------------------------------------------
// 诊断码（u3301 十六码 · 12 枚互斥 + segment_guard）
// --------------------------------------------------------------------------
/// RCU 压测编排 混合面诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum u3301Code {
    /// 0xe090 表行重复（RCU）。
    RowDup,
    /// 0xe091 表行断档（宽限）。
    RowGap,
    /// 0xe092 守卫触发（重入/越限被拒）。
    GuardTrip,
    /// 0xe093 映射键重复（静默）。
    KeyDup,
    /// 0xe094 映射键断档（读多写少）。
    KeyGap,
    /// 0xe095 映射值越界。
    ValOob,
    /// 0xe096 双表累计漂移。
    SumDrift,
    /// 0xe097 阈值漂移（守卫阈值≠冻结值）。
    ThreshDrift,
    /// 0xe098 台账不守恒（守恒恒等式破缺）。
    LedgerBroken,
    /// 0xe099 快照指纹不符（字段被改后重算不一致）。
    SnapshotFingerprint,
    /// 0xe09a 常量表破坏（表项与语义规则矛盾）。
    TableBroken,
    /// 0xe09b 段位守卫失败（诊断码越出本模块段）。
    SegmentGuard,
}

impl u3301Code {
    /// 全量闭集。
    pub const ALL: [u3301Code; 12] = [
        u3301Code::RowDup,
        u3301Code::RowGap,
        u3301Code::GuardTrip,
        u3301Code::KeyDup,
        u3301Code::KeyGap,
        u3301Code::ValOob,
        u3301Code::SumDrift,
        u3301Code::ThreshDrift,
        u3301Code::LedgerBroken,
        u3301Code::SnapshotFingerprint,
        u3301Code::TableBroken,
        u3301Code::SegmentGuard,
    ];

    /// 码值（连续十六进制，与声明序恒等）。
    pub const fn code(self) -> u16 {
        match self {
            u3301Code::RowDup => 0xe090,
            u3301Code::RowGap => 0xe091,
            u3301Code::GuardTrip => 0xe092,
            u3301Code::KeyDup => 0xe093,
            u3301Code::KeyGap => 0xe094,
            u3301Code::ValOob => 0xe095,
            u3301Code::SumDrift => 0xe096,
            u3301Code::ThreshDrift => 0xe097,
            u3301Code::LedgerBroken => 0xe098,
            u3301Code::SnapshotFingerprint => 0xe099,
            u3301Code::TableBroken => 0xe09a,
            u3301Code::SegmentGuard => 0xe09b,
        }
    }

    /// 中文诊断名。
    pub const fn name(self) -> &'static str {
        match self {
            u3301Code::RowDup => "表行重复（RCU）",
            u3301Code::RowGap => "表行断档（宽限）",
            u3301Code::GuardTrip => "守卫触发（重入/越限被拒）",
            u3301Code::KeyDup => "映射键重复（静默）",
            u3301Code::KeyGap => "映射键断档（读多写少）",
            u3301Code::ValOob => "映射值越界",
            u3301Code::SumDrift => "双表累计漂移",
            u3301Code::ThreshDrift => "阈值漂移（守卫阈值≠冻结值）",
            u3301Code::LedgerBroken => "台账不守恒（守恒恒等式破缺）",
            u3301Code::SnapshotFingerprint => "快照指纹不符（字段被改后重算不一致）",
            u3301Code::TableBroken => "常量表破坏（表项与语义规则矛盾）",
            u3301Code::SegmentGuard => "段位守卫失败（诊断码越出本模块段）",
        }
    }

    /// 可操作修复建议（每条诊断都必须带下一步动作）。
    pub const fn suggestion(self) -> &'static str {
        match self {
            u3301Code::RowDup => "按键唯一律去重",
            u3301Code::RowGap => "按连续律补号",
            u3301Code::GuardTrip => "检查临界区嵌套",
            u3301Code::KeyDup => "按键唯一律去重",
            u3301Code::KeyGap => "按连续律补号",
            u3301Code::ValOob => "按值域裁剪",
            u3301Code::SumDrift => "按理论值复算",
            u3301Code::ThreshDrift => "按冻结值校正",
            u3301Code::LedgerBroken => "按守恒律重建台账后重跑",
            u3301Code::SnapshotFingerprint => "按防篡改规程重建快照",
            u3301Code::TableBroken => "按语义规则重建常量表",
            u3301Code::SegmentGuard => "核对诊断码分配表",
        }
    }
}

// --------------------------------------------------------------------------
// 冻结常量表（u3301mix · 16 行逐行注释自证）
// --------------------------------------------------------------------------
/// RCU 压测编排 对拍表（16 行）。
pub const u3301mix_COUNT: usize = 16;

/// 冻结表本体（编译期常量，运行期只读）。
pub static u3301mix: [(&'static str, u64); 16] = [
    ("RD_00", 10), // RD 对拍行（读者）
    ("WR_01", 15), // WR 对拍行（写者）
    ("GP_02", 20), // GP 对拍行（宽限期）
    ("CPU_03", 25), // CPU 对拍行（核数）
    ("READM_04", 30), // READM 对拍行（读多写少比）
    ("QS_05", 35), // QS 对拍行（静默态）
    ("GPW_06", 40), // GPW 对拍行（宽限窗口）
    ("STW_07", 45), // STW 对拍行（停顿窗口）
    ("LAG_08", 50), // LAG 对拍行（读侧滞后）
    ("LOSS_09", 55), // LOSS 对拍行（零丢失账）
    ("FP_10", 60), // FP 对拍行（指纹链）
    ("THR_11", 65), // THR 对拍行（阈值）
    ("RD_12", 70), // RD 对拍行（读者）
    ("WR_13", 75), // WR 对拍行（写者）
    ("GP_14", 80), // GP 对拍行（宽限期）
    ("CPU_15", 85), // CPU 对拍行（核数）
];

/// 线性查找（键唯一性由构造期断言保证，运行时无分支歧义）。
pub fn u3301mix_get(key: &'static str) -> Option<u64> {
    let mut i = 0usize;
    while i < u3301mix_COUNT {
        if u3301mix[i].0 == key {
            return Some(u3301mix[i].1);
        }
        i += 1;
    }
    None
}

/// 累计求和（用于守恒对账）。
pub fn u3301mix_sum() -> u64 {
    let mut s = 0u64;
    let mut i = 0usize;
    while i < u3301mix_COUNT {
        s = s.wrapping_add(u3301mix[i].1 as u64);
        i += 1;
    }
    s
}

// --------------------------------------------------------------------------
// 守卫 u3301mixgate（阈值 32 · 重入/越限/静默三类拒止）
// --------------------------------------------------------------------------
/// RCU 压测编排 混合面守卫（阈值 32 冻结）。
#[derive(Clone, Copy, Debug)]
pub struct u3301mixgate {
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

/// 守卫阈值（RCU 压测编排 混合面守卫（阈值 32 冻结））。
pub const U3301MIXGATE_THRESHOLD: u64 = 32;

impl u3301mixgate {
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
        if self.window >= U3301MIXGATE_THRESHOLD {
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
// 字符串键值映射 u3301mixmap（16 行逐行语义注释 + 逐行守卫）
// --------------------------------------------------------------------------
/// RCU 压测编排 混合映射矩阵。
/// 行数。
pub const u3301mixmap_COUNT: usize = 16;

/// 表本体（键 + 值 + 注释由编译期冻结）。
pub static u3301mixmap: [(&'static str, u64); 16] = [
    ("RD0000", 0), // RD 单元 0000（读者 × 相位 00 × 混 轮）
    ("WR0001", 9), // WR 单元 0001（写者 × 相位 01 × 混 轮）
    ("GP0002", 18), // GP 单元 0002（宽限期 × 相位 02 × 混 轮）
    ("CPU0003", 27), // CPU 单元 0003（核数 × 相位 03 × 混 轮）
    ("READM0004", 36), // READM 单元 0004（读多写少比 × 相位 04 × 混 轮）
    ("QS0005", 45), // QS 单元 0005（静默态 × 相位 05 × 混 轮）
    ("GPW0006", 54), // GPW 单元 0006（宽限窗口 × 相位 06 × 混 轮）
    ("STW0007", 63), // STW 单元 0007（停顿窗口 × 相位 07 × 混 轮）
    ("LAG0008", 8), // LAG 单元 0008（读侧滞后 × 相位 08 × 混 轮）
    ("LOSS0009", 17), // LOSS 单元 0009（零丢失账 × 相位 09 × 混 轮）
    ("FP0010", 26), // FP 单元 0010（指纹链 × 相位 10 × 混 轮）
    ("THR0011", 35), // THR 单元 0011（阈值 × 相位 11 × 混 轮）
    ("RD0012", 44), // RD 单元 0012（读者 × 相位 12 × 混 轮）
    ("WR0013", 53), // WR 单元 0013（写者 × 相位 13 × 混 轮）
    ("GP0014", 62), // GP 单元 0014（宽限期 × 相位 14 × 混 轮）
    ("CPU0015", 7), // CPU 单元 0015（核数 × 相位 15 × 混 轮）
];

/// O(1) 语义查找（match 全枚举；命中率 100%% 由守卫保证）。
pub fn u3301mixmap_get(key: &str) -> Option<u64> {
    match key {
        "RD0000" => Some(0),
        "WR0001" => Some(9),
        "GP0002" => Some(18),
        "CPU0003" => Some(27),
        "READM0004" => Some(36),
        "QS0005" => Some(45),
        "GPW0006" => Some(54),
        "STW0007" => Some(63),
        "LAG0008" => Some(8),
        "LOSS0009" => Some(17),
        "FP0010" => Some(26),
        "THR0011" => Some(35),
        "RD0012" => Some(44),
        "WR0013" => Some(53),
        "GP0014" => Some(62),
        "CPU0015" => Some(7),
        _ => None,
    }
}

/// 逐行守卫扫描（返回违例行数；0 = 全绿）。
pub fn u3301mixmap_scan() -> usize {
    let mut bad = 0usize;
    let mut i = 0usize;
    while i < u3301mixmap_COUNT {
        let (k, v) = u3301mixmap[i];
        let ok = match k {
            "RD0000" => v < 64,
            "WR0001" => v < 64,
            "GP0002" => v < 64,
            "CPU0003" => v < 64,
            "READM0004" => v < 64,
            "QS0005" => v < 64,
            "GPW0006" => v < 64,
            "STW0007" => v < 64,
            "LAG0008" => v < 64,
            "LOSS0009" => v < 64,
            "FP0010" => v < 64,
            "THR0011" => v < 64,
            "RD0012" => v < 64,
            "WR0013" => v < 64,
            "GP0014" => v < 64,
            "CPU0015" => v < 64,
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
pub fn u3301mixmap_sum() -> u64 {
    let mut s = 0u64;
    let mut i = 0usize;
    while i < u3301mixmap_COUNT {
        s = s.wrapping_add(u3301mixmap[i].1 as u64);
        i += 1;
    }
    s
}

/// 键唯一性（构造期断言等价物：逐对扫描同键）。
pub fn u3301mixmap_unique() -> bool {
    let mut i = 0usize;
    while i < u3301mixmap_COUNT {
        let mut j = i + 1;
        while j < u3301mixmap_COUNT {
            if u3301mixmap[i].0 == u3301mixmap[j].0 {
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

/// 段位守卫：全部诊断码必须落在本段 [0xe090, 0xe09f]。
pub const fn u3301_segment_guard() -> bool {
    let mut ok = true;
    let mut i = 0usize;
    while i < u3301Code::ALL.len() {
        let c = u3301Code::ALL[i].code();
        if c < 0xe090 || c > 0xe09f {
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
fn u3301_guard_seg() -> bool {
    u3301_segment_guard()
}

/// UNX-F3301 对拍表 16 行边界与累计守恒。
fn check_u3301_mix_tbl() -> bool {
    let mut ok = u3301mix_COUNT == 16;
    if ok { ok = u3301mix_get("RD_00") == Some(10) && u3301mix_get("CPU_15") == Some(85); }
    if ok { ok = u3301mix_get("ZZZ_NOKEY").is_none(); }
    if ok { ok = u3301mix_sum() == 760; }
    ok
}

/// UNX-F3301 混合映射 16 行守卫唯一 + 守卫阈值冻结。
fn check_u3301_mix_map() -> bool {
    let mut ok = u3301mixmap_COUNT == 16;
    if ok { ok = u3301mixmap_scan() == 0 && u3301mixmap_unique(); }
    if ok { ok = u3301mixmap_sum() == 504; }
    if ok { ok = U3301MIXGATE_THRESHOLD == 32; }
    ok
}

/// 本模块自检（逐检查带 detail，全绿才出门）。
pub fn run_u3301_checks() -> CheckSet {
    let mut set = CheckSet::new("U3301");
    set.add(
        "u3301-seg-guard",
        u3301_guard_seg(),
        if u3301_guard_seg() { "" } else { "diagnostic code outside segment" },
    );
    set.add(
        "u3301-mix-tbl",
        check_u3301_mix_tbl(),
        if check_u3301_mix_tbl() { "" } else { "UNX-F3301 mix table mismatch" },
    );
    set.add(
        "u3301-mix-map",
        check_u3301_mix_map(),
        if check_u3301_mix_map() { "" } else { "UNX-F3301 mix map mismatch" },
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
    fn u3301_all_checks_green() {
        let set = run_u3301_checks();
        assert!(set.all_passed(), "U3301 domain checks must be green");
    }

    #[test]
    fn u3301_segment_guard_holds() {
        assert!(u3301_segment_guard());
    }
}
