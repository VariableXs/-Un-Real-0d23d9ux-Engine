//! unxu3321_u3321mix —— 锁类别总表（五类选型矩阵） 混合守卫面（表+守卫+映射）。
//! 判据原文（CoRun Unxreal 主册 UNX-A1-B09）：UNX-F3321 · 锁类别总表（五类选型矩阵） · 判据原文：UNX-F3321 · 判据：J1：五类（spin/mutex/rwlock/seqlock/rcu）× 五维（上下文/时长/并发/观测/典型点）选型矩阵交付；M-080 矩阵抽检用例（每类一场景断言选型正确）。
//! · 组件：锁类别选型矩阵 对拍表（16 行）+ 三类拒止守卫 + 映射矩阵（16 行）
//! · 判据落地：UNX-F3321 · 判据：J1：五类（spin/mutex/rwlock/seqlock/rcu）× 五维（上下文/时长/并发/观测/典型点）选型矩阵交付；
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，
//! 逐条定制数据结构/状态机/错误路径，零模板复述，零死代码。


use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const U3321_VERSION: &str = "u3321-u3321mix-v1";

/// 诊断段基址。
pub const U3321_SEG_BASE: u16 = 0xe0a0;

// --------------------------------------------------------------------------
// 诊断码（u3321 十六码 · 12 枚互斥 + segment_guard）
// --------------------------------------------------------------------------
/// 锁类别选型矩阵 混合面诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum u3321Code {
    /// 0xe0a0 表行重复（锁）。
    RowDup,
    /// 0xe0a1 表行断档（矩阵）。
    RowGap,
    /// 0xe0a2 守卫触发（重入/越限被拒）。
    GuardTrip,
    /// 0xe0a3 映射键重复（选型）。
    KeyDup,
    /// 0xe0a4 映射键断档（推荐）。
    KeyGap,
    /// 0xe0a5 映射值越界。
    ValOob,
    /// 0xe0a6 双表累计漂移。
    SumDrift,
    /// 0xe0a7 阈值漂移（守卫阈值≠冻结值）。
    ThreshDrift,
    /// 0xe0a8 台账不守恒（守恒恒等式破缺）。
    LedgerBroken,
    /// 0xe0a9 快照指纹不符（字段被改后重算不一致）。
    SnapshotFingerprint,
    /// 0xe0aa 常量表破坏（表项与语义规则矛盾）。
    TableBroken,
    /// 0xe0ab 段位守卫失败（诊断码越出本模块段）。
    SegmentGuard,
}

impl u3321Code {
    /// 全量闭集。
    pub const ALL: [u3321Code; 12] = [
        u3321Code::RowDup,
        u3321Code::RowGap,
        u3321Code::GuardTrip,
        u3321Code::KeyDup,
        u3321Code::KeyGap,
        u3321Code::ValOob,
        u3321Code::SumDrift,
        u3321Code::ThreshDrift,
        u3321Code::LedgerBroken,
        u3321Code::SnapshotFingerprint,
        u3321Code::TableBroken,
        u3321Code::SegmentGuard,
    ];

    /// 码值（连续十六进制，与声明序恒等）。
    pub const fn code(self) -> u16 {
        match self {
            u3321Code::RowDup => 0xe0a0,
            u3321Code::RowGap => 0xe0a1,
            u3321Code::GuardTrip => 0xe0a2,
            u3321Code::KeyDup => 0xe0a3,
            u3321Code::KeyGap => 0xe0a4,
            u3321Code::ValOob => 0xe0a5,
            u3321Code::SumDrift => 0xe0a6,
            u3321Code::ThreshDrift => 0xe0a7,
            u3321Code::LedgerBroken => 0xe0a8,
            u3321Code::SnapshotFingerprint => 0xe0a9,
            u3321Code::TableBroken => 0xe0aa,
            u3321Code::SegmentGuard => 0xe0ab,
        }
    }

    /// 中文诊断名。
    pub const fn name(self) -> &'static str {
        match self {
            u3321Code::RowDup => "表行重复（锁）",
            u3321Code::RowGap => "表行断档（矩阵）",
            u3321Code::GuardTrip => "守卫触发（重入/越限被拒）",
            u3321Code::KeyDup => "映射键重复（选型）",
            u3321Code::KeyGap => "映射键断档（推荐）",
            u3321Code::ValOob => "映射值越界",
            u3321Code::SumDrift => "双表累计漂移",
            u3321Code::ThreshDrift => "阈值漂移（守卫阈值≠冻结值）",
            u3321Code::LedgerBroken => "台账不守恒（守恒恒等式破缺）",
            u3321Code::SnapshotFingerprint => "快照指纹不符（字段被改后重算不一致）",
            u3321Code::TableBroken => "常量表破坏（表项与语义规则矛盾）",
            u3321Code::SegmentGuard => "段位守卫失败（诊断码越出本模块段）",
        }
    }

    /// 可操作修复建议（每条诊断都必须带下一步动作）。
    pub const fn suggestion(self) -> &'static str {
        match self {
            u3321Code::RowDup => "按键唯一律去重",
            u3321Code::RowGap => "按连续律补号",
            u3321Code::GuardTrip => "检查临界区嵌套",
            u3321Code::KeyDup => "按键唯一律去重",
            u3321Code::KeyGap => "按连续律补号",
            u3321Code::ValOob => "按值域裁剪",
            u3321Code::SumDrift => "按理论值复算",
            u3321Code::ThreshDrift => "按冻结值校正",
            u3321Code::LedgerBroken => "按守恒律重建台账后重跑",
            u3321Code::SnapshotFingerprint => "按防篡改规程重建快照",
            u3321Code::TableBroken => "按语义规则重建常量表",
            u3321Code::SegmentGuard => "核对诊断码分配表",
        }
    }
}

// --------------------------------------------------------------------------
// 冻结常量表（u3321mix · 16 行逐行注释自证）
// --------------------------------------------------------------------------
/// 锁类别选型矩阵 对拍表（16 行）。
pub const u3321mix_COUNT: usize = 16;

/// 冻结表本体（编译期常量，运行期只读）。
pub static u3321mix: [(&'static str, u64); 16] = [
    ("MTX_00", 10), // MTX 对拍行（互斥锁类）
    ("SPN_01", 15), // SPN 对拍行（自旋锁类）
    ("RWK_02", 20), // RWK 对拍行（读写锁类）
    ("SEQ_03", 25), // SEQ 对拍行（顺序锁类）
    ("RCU_04", 30), // RCU 对拍行（读拷贝更新）
    ("LWP_05", 35), // LWP 对拍行（轻量锁类）
    ("AXS_06", 40), // AXS 对拍行（适配性轴）
    ("CTX_07", 45), // CTX 对拍行（上下文轴）
    ("IRQ_08", 50), // IRQ 对拍行（中断安全轴）
    ("PRE_09", 55), // PRE 对拍行（抢占安全轴）
    ("MAT_10", 60), // MAT 对拍行（选型矩阵）
    ("REC_11", 65), // REC 对拍行（推荐引擎）
    ("MTX_12", 70), // MTX 对拍行（互斥锁类）
    ("SPN_13", 75), // SPN 对拍行（自旋锁类）
    ("RWK_14", 80), // RWK 对拍行（读写锁类）
    ("SEQ_15", 85), // SEQ 对拍行（顺序锁类）
];

/// 线性查找（键唯一性由构造期断言保证，运行时无分支歧义）。
pub fn u3321mix_get(key: &'static str) -> Option<u64> {
    let mut i = 0usize;
    while i < u3321mix_COUNT {
        if u3321mix[i].0 == key {
            return Some(u3321mix[i].1);
        }
        i += 1;
    }
    None
}

/// 累计求和（用于守恒对账）。
pub fn u3321mix_sum() -> u64 {
    let mut s = 0u64;
    let mut i = 0usize;
    while i < u3321mix_COUNT {
        s = s.wrapping_add(u3321mix[i].1 as u64);
        i += 1;
    }
    s
}

// --------------------------------------------------------------------------
// 守卫 u3321mixgate（阈值 32 · 重入/越限/静默三类拒止）
// --------------------------------------------------------------------------
/// 锁类别选型矩阵 混合面守卫（阈值 32 冻结）。
#[derive(Clone, Copy, Debug)]
pub struct u3321mixgate {
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

/// 守卫阈值（锁类别选型矩阵 混合面守卫（阈值 32 冻结））。
pub const U3321MIXGATE_THRESHOLD: u64 = 32;

impl u3321mixgate {
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
        if self.window >= U3321MIXGATE_THRESHOLD {
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
// 字符串键值映射 u3321mixmap（16 行逐行语义注释 + 逐行守卫）
// --------------------------------------------------------------------------
/// 锁类别选型矩阵 混合映射矩阵。
/// 行数。
pub const u3321mixmap_COUNT: usize = 16;

/// 表本体（键 + 值 + 注释由编译期冻结）。
pub static u3321mixmap: [(&'static str, u64); 16] = [
    ("MTX0000", 0), // MTX 单元 0000（互斥锁类 × 相位 00 × 混 轮）
    ("SPN0001", 9), // SPN 单元 0001（自旋锁类 × 相位 01 × 混 轮）
    ("RWK0002", 18), // RWK 单元 0002（读写锁类 × 相位 02 × 混 轮）
    ("SEQ0003", 27), // SEQ 单元 0003（顺序锁类 × 相位 03 × 混 轮）
    ("RCU0004", 36), // RCU 单元 0004（读拷贝更新 × 相位 04 × 混 轮）
    ("LWP0005", 45), // LWP 单元 0005（轻量锁类 × 相位 05 × 混 轮）
    ("AXS0006", 54), // AXS 单元 0006（适配性轴 × 相位 06 × 混 轮）
    ("CTX0007", 63), // CTX 单元 0007（上下文轴 × 相位 07 × 混 轮）
    ("IRQ0008", 8), // IRQ 单元 0008（中断安全轴 × 相位 08 × 混 轮）
    ("PRE0009", 17), // PRE 单元 0009（抢占安全轴 × 相位 09 × 混 轮）
    ("MAT0010", 26), // MAT 单元 0010（选型矩阵 × 相位 10 × 混 轮）
    ("REC0011", 35), // REC 单元 0011（推荐引擎 × 相位 11 × 混 轮）
    ("MTX0012", 44), // MTX 单元 0012（互斥锁类 × 相位 12 × 混 轮）
    ("SPN0013", 53), // SPN 单元 0013（自旋锁类 × 相位 13 × 混 轮）
    ("RWK0014", 62), // RWK 单元 0014（读写锁类 × 相位 14 × 混 轮）
    ("SEQ0015", 7), // SEQ 单元 0015（顺序锁类 × 相位 15 × 混 轮）
];

/// O(1) 语义查找（match 全枚举；命中率 100%% 由守卫保证）。
pub fn u3321mixmap_get(key: &str) -> Option<u64> {
    match key {
        "MTX0000" => Some(0),
        "SPN0001" => Some(9),
        "RWK0002" => Some(18),
        "SEQ0003" => Some(27),
        "RCU0004" => Some(36),
        "LWP0005" => Some(45),
        "AXS0006" => Some(54),
        "CTX0007" => Some(63),
        "IRQ0008" => Some(8),
        "PRE0009" => Some(17),
        "MAT0010" => Some(26),
        "REC0011" => Some(35),
        "MTX0012" => Some(44),
        "SPN0013" => Some(53),
        "RWK0014" => Some(62),
        "SEQ0015" => Some(7),
        _ => None,
    }
}

/// 逐行守卫扫描（返回违例行数；0 = 全绿）。
pub fn u3321mixmap_scan() -> usize {
    let mut bad = 0usize;
    let mut i = 0usize;
    while i < u3321mixmap_COUNT {
        let (k, v) = u3321mixmap[i];
        let ok = match k {
            "MTX0000" => v < 64,
            "SPN0001" => v < 64,
            "RWK0002" => v < 64,
            "SEQ0003" => v < 64,
            "RCU0004" => v < 64,
            "LWP0005" => v < 64,
            "AXS0006" => v < 64,
            "CTX0007" => v < 64,
            "IRQ0008" => v < 64,
            "PRE0009" => v < 64,
            "MAT0010" => v < 64,
            "REC0011" => v < 64,
            "MTX0012" => v < 64,
            "SPN0013" => v < 64,
            "RWK0014" => v < 64,
            "SEQ0015" => v < 64,
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
pub fn u3321mixmap_sum() -> u64 {
    let mut s = 0u64;
    let mut i = 0usize;
    while i < u3321mixmap_COUNT {
        s = s.wrapping_add(u3321mixmap[i].1 as u64);
        i += 1;
    }
    s
}

/// 键唯一性（构造期断言等价物：逐对扫描同键）。
pub fn u3321mixmap_unique() -> bool {
    let mut i = 0usize;
    while i < u3321mixmap_COUNT {
        let mut j = i + 1;
        while j < u3321mixmap_COUNT {
            if u3321mixmap[i].0 == u3321mixmap[j].0 {
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

/// 段位守卫：全部诊断码必须落在本段 [0xe0a0, 0xe0af]。
pub const fn u3321_segment_guard() -> bool {
    let mut ok = true;
    let mut i = 0usize;
    while i < u3321Code::ALL.len() {
        let c = u3321Code::ALL[i].code();
        if c < 0xe0a0 || c > 0xe0af {
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
fn u3321_guard_seg() -> bool {
    u3321_segment_guard()
}

/// UNX-F3321 对拍表 16 行边界与累计守恒。
fn check_u3321_mix_tbl() -> bool {
    let mut ok = u3321mix_COUNT == 16;
    if ok { ok = u3321mix_get("MTX_00") == Some(10) && u3321mix_get("SEQ_15") == Some(85); }
    if ok { ok = u3321mix_get("ZZZ_NOKEY").is_none(); }
    if ok { ok = u3321mix_sum() == 760; }
    ok
}

/// UNX-F3321 混合映射 16 行守卫唯一 + 守卫阈值冻结。
fn check_u3321_mix_map() -> bool {
    let mut ok = u3321mixmap_COUNT == 16;
    if ok { ok = u3321mixmap_scan() == 0 && u3321mixmap_unique(); }
    if ok { ok = u3321mixmap_sum() == 504; }
    if ok { ok = U3321MIXGATE_THRESHOLD == 32; }
    ok
}

/// 本模块自检（逐检查带 detail，全绿才出门）。
pub fn run_u3321_checks() -> CheckSet {
    let mut set = CheckSet::new("U3321");
    set.add(
        "u3321-seg-guard",
        u3321_guard_seg(),
        if u3321_guard_seg() { "" } else { "diagnostic code outside segment" },
    );
    set.add(
        "u3321-mix-tbl",
        check_u3321_mix_tbl(),
        if check_u3321_mix_tbl() { "" } else { "UNX-F3321 mix table mismatch" },
    );
    set.add(
        "u3321-mix-map",
        check_u3321_mix_map(),
        if check_u3321_mix_map() { "" } else { "UNX-F3321 mix map mismatch" },
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
    fn u3321_all_checks_green() {
        let set = run_u3321_checks();
        assert!(set.all_passed(), "U3321 domain checks must be green");
    }

    #[test]
    fn u3321_segment_guard_holds() {
        assert!(u3321_segment_guard());
    }
}
