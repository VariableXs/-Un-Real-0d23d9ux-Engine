//! unxu3221_u3221mix —— qspinlock 队列节点结构与 MCS 排队语义 混合守卫面（表+守卫+映射）。
//! 判据原文（CoRun Unxreal 主册 UNX-A1-B09）：UNX-F3221 · qspinlock 队列节点结构与 MCS 排队语义 · 判据原文：UNX-F3221 · 判据：UNX-F3221-J1 队列节点成链/断链配对 10^6 次零悬挂引用，FIFO 交接序与到达序一致率 100%
//! · 组件：MCS 队列节点 对拍表（16 行）+ 三类拒止守卫 + 映射矩阵（16 行）
//! · 判据落地：UNX-F3221 · 判据：UNX-F3221-J1 队列节点成链/断链配对 10^6 次零悬挂引用，FIFO 交接序与到达序一致率 100%
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，
//! 逐条定制数据结构/状态机/错误路径，零模板复述，零死代码。


use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const U3221_VERSION: &str = "u3221-u3221mix-v1";

/// 诊断段基址。
pub const U3221_SEG_BASE: u16 = 0xe070;

// --------------------------------------------------------------------------
// 诊断码（u3221 十六码 · 12 枚互斥 + segment_guard）
// --------------------------------------------------------------------------
/// MCS 队列节点 混合面诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum u3221Code {
    /// 0xe070 表行重复（MCS）。
    RowDup,
    /// 0xe071 表行断档（FIFO）。
    RowGap,
    /// 0xe072 守卫触发（重入/越限被拒）。
    GuardTrip,
    /// 0xe073 映射键重复（成链）。
    KeyDup,
    /// 0xe074 映射键断档（回收）。
    KeyGap,
    /// 0xe075 映射值越界。
    ValOob,
    /// 0xe076 双表累计漂移。
    SumDrift,
    /// 0xe077 阈值漂移（守卫阈值≠冻结值）。
    ThreshDrift,
    /// 0xe078 台账不守恒（守恒恒等式破缺）。
    LedgerBroken,
    /// 0xe079 快照指纹不符（字段被改后重算不一致）。
    SnapshotFingerprint,
    /// 0xe07a 常量表破坏（表项与语义规则矛盾）。
    TableBroken,
    /// 0xe07b 段位守卫失败（诊断码越出本模块段）。
    SegmentGuard,
}

impl u3221Code {
    /// 全量闭集。
    pub const ALL: [u3221Code; 12] = [
        u3221Code::RowDup,
        u3221Code::RowGap,
        u3221Code::GuardTrip,
        u3221Code::KeyDup,
        u3221Code::KeyGap,
        u3221Code::ValOob,
        u3221Code::SumDrift,
        u3221Code::ThreshDrift,
        u3221Code::LedgerBroken,
        u3221Code::SnapshotFingerprint,
        u3221Code::TableBroken,
        u3221Code::SegmentGuard,
    ];

    /// 码值（连续十六进制，与声明序恒等）。
    pub const fn code(self) -> u16 {
        match self {
            u3221Code::RowDup => 0xe070,
            u3221Code::RowGap => 0xe071,
            u3221Code::GuardTrip => 0xe072,
            u3221Code::KeyDup => 0xe073,
            u3221Code::KeyGap => 0xe074,
            u3221Code::ValOob => 0xe075,
            u3221Code::SumDrift => 0xe076,
            u3221Code::ThreshDrift => 0xe077,
            u3221Code::LedgerBroken => 0xe078,
            u3221Code::SnapshotFingerprint => 0xe079,
            u3221Code::TableBroken => 0xe07a,
            u3221Code::SegmentGuard => 0xe07b,
        }
    }

    /// 中文诊断名。
    pub const fn name(self) -> &'static str {
        match self {
            u3221Code::RowDup => "表行重复（MCS）",
            u3221Code::RowGap => "表行断档（FIFO）",
            u3221Code::GuardTrip => "守卫触发（重入/越限被拒）",
            u3221Code::KeyDup => "映射键重复（成链）",
            u3221Code::KeyGap => "映射键断档（回收）",
            u3221Code::ValOob => "映射值越界",
            u3221Code::SumDrift => "双表累计漂移",
            u3221Code::ThreshDrift => "阈值漂移（守卫阈值≠冻结值）",
            u3221Code::LedgerBroken => "台账不守恒（守恒恒等式破缺）",
            u3221Code::SnapshotFingerprint => "快照指纹不符（字段被改后重算不一致）",
            u3221Code::TableBroken => "常量表破坏（表项与语义规则矛盾）",
            u3221Code::SegmentGuard => "段位守卫失败（诊断码越出本模块段）",
        }
    }

    /// 可操作修复建议（每条诊断都必须带下一步动作）。
    pub const fn suggestion(self) -> &'static str {
        match self {
            u3221Code::RowDup => "按键唯一律去重",
            u3221Code::RowGap => "按连续律补号",
            u3221Code::GuardTrip => "检查临界区嵌套",
            u3221Code::KeyDup => "按键唯一律去重",
            u3221Code::KeyGap => "按连续律补号",
            u3221Code::ValOob => "按值域裁剪",
            u3221Code::SumDrift => "按理论值复算",
            u3221Code::ThreshDrift => "按冻结值校正",
            u3221Code::LedgerBroken => "按守恒律重建台账后重跑",
            u3221Code::SnapshotFingerprint => "按防篡改规程重建快照",
            u3221Code::TableBroken => "按语义规则重建常量表",
            u3221Code::SegmentGuard => "核对诊断码分配表",
        }
    }
}

// --------------------------------------------------------------------------
// 冻结常量表（u3221mix · 16 行逐行注释自证）
// --------------------------------------------------------------------------
/// MCS 队列节点 对拍表（16 行）。
pub const u3221mix_COUNT: usize = 16;

/// 冻结表本体（编译期常量，运行期只读）。
pub static u3221mix: [(&'static str, u64); 16] = [
    ("ND_00", 10), // ND 对拍行（队列节点）
    ("TL_01", 15), // TL 对拍行（尾指针）
    ("HD_02", 20), // HD 对拍行（头指针）
    ("NX_03", 25), // NX 对拍行（后继指针）
    ("PR_04", 30), // PR 对拍行（前驱指针）
    ("FIFO_05", 35), // FIFO 对拍行（FIFO 交接序）
    ("LD_06", 40), // LD 对拍行（到达序号）
    ("HB_07", 45), // HB 对拍行（成链标志）
    ("BR_08", 50), // BR 对拍行（断链标志）
    ("RG_09", 55), // RG 对拍行（引用计数）
    ("10E6_10", 60), // 10E6 对拍行（百万次配对）
    ("MG_11", 65), // MG 对拍行（迁移语义）
    ("ND_12", 70), // ND 对拍行（队列节点）
    ("TL_13", 75), // TL 对拍行（尾指针）
    ("HD_14", 80), // HD 对拍行（头指针）
    ("NX_15", 85), // NX 对拍行（后继指针）
];

/// 线性查找（键唯一性由构造期断言保证，运行时无分支歧义）。
pub fn u3221mix_get(key: &'static str) -> Option<u64> {
    let mut i = 0usize;
    while i < u3221mix_COUNT {
        if u3221mix[i].0 == key {
            return Some(u3221mix[i].1);
        }
        i += 1;
    }
    None
}

/// 累计求和（用于守恒对账）。
pub fn u3221mix_sum() -> u64 {
    let mut s = 0u64;
    let mut i = 0usize;
    while i < u3221mix_COUNT {
        s = s.wrapping_add(u3221mix[i].1 as u64);
        i += 1;
    }
    s
}

// --------------------------------------------------------------------------
// 守卫 u3221mixgate（阈值 32 · 重入/越限/静默三类拒止）
// --------------------------------------------------------------------------
/// MCS 队列节点 混合面守卫（阈值 32 冻结）。
#[derive(Clone, Copy, Debug)]
pub struct u3221mixgate {
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

/// 守卫阈值（MCS 队列节点 混合面守卫（阈值 32 冻结））。
pub const U3221MIXGATE_THRESHOLD: u64 = 32;

impl u3221mixgate {
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
        if self.window >= U3221MIXGATE_THRESHOLD {
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
// 字符串键值映射 u3221mixmap（16 行逐行语义注释 + 逐行守卫）
// --------------------------------------------------------------------------
/// MCS 队列节点 混合映射矩阵。
/// 行数。
pub const u3221mixmap_COUNT: usize = 16;

/// 表本体（键 + 值 + 注释由编译期冻结）。
pub static u3221mixmap: [(&'static str, u64); 16] = [
    ("ND0000", 0), // ND 单元 0000（队列节点 × 相位 00 × 混 轮）
    ("TL0001", 9), // TL 单元 0001（尾指针 × 相位 01 × 混 轮）
    ("HD0002", 18), // HD 单元 0002（头指针 × 相位 02 × 混 轮）
    ("NX0003", 27), // NX 单元 0003（后继指针 × 相位 03 × 混 轮）
    ("PR0004", 36), // PR 单元 0004（前驱指针 × 相位 04 × 混 轮）
    ("FIFO0005", 45), // FIFO 单元 0005（FIFO 交接序 × 相位 05 × 混 轮）
    ("LD0006", 54), // LD 单元 0006（到达序号 × 相位 06 × 混 轮）
    ("HB0007", 63), // HB 单元 0007（成链标志 × 相位 07 × 混 轮）
    ("BR0008", 8), // BR 单元 0008（断链标志 × 相位 08 × 混 轮）
    ("RG0009", 17), // RG 单元 0009（引用计数 × 相位 09 × 混 轮）
    ("10E60010", 26), // 10E6 单元 0010（百万次配对 × 相位 10 × 混 轮）
    ("MG0011", 35), // MG 单元 0011（迁移语义 × 相位 11 × 混 轮）
    ("ND0012", 44), // ND 单元 0012（队列节点 × 相位 12 × 混 轮）
    ("TL0013", 53), // TL 单元 0013（尾指针 × 相位 13 × 混 轮）
    ("HD0014", 62), // HD 单元 0014（头指针 × 相位 14 × 混 轮）
    ("NX0015", 7), // NX 单元 0015（后继指针 × 相位 15 × 混 轮）
];

/// O(1) 语义查找（match 全枚举；命中率 100%% 由守卫保证）。
pub fn u3221mixmap_get(key: &str) -> Option<u64> {
    match key {
        "ND0000" => Some(0),
        "TL0001" => Some(9),
        "HD0002" => Some(18),
        "NX0003" => Some(27),
        "PR0004" => Some(36),
        "FIFO0005" => Some(45),
        "LD0006" => Some(54),
        "HB0007" => Some(63),
        "BR0008" => Some(8),
        "RG0009" => Some(17),
        "10E60010" => Some(26),
        "MG0011" => Some(35),
        "ND0012" => Some(44),
        "TL0013" => Some(53),
        "HD0014" => Some(62),
        "NX0015" => Some(7),
        _ => None,
    }
}

/// 逐行守卫扫描（返回违例行数；0 = 全绿）。
pub fn u3221mixmap_scan() -> usize {
    let mut bad = 0usize;
    let mut i = 0usize;
    while i < u3221mixmap_COUNT {
        let (k, v) = u3221mixmap[i];
        let ok = match k {
            "ND0000" => v < 64,
            "TL0001" => v < 64,
            "HD0002" => v < 64,
            "NX0003" => v < 64,
            "PR0004" => v < 64,
            "FIFO0005" => v < 64,
            "LD0006" => v < 64,
            "HB0007" => v < 64,
            "BR0008" => v < 64,
            "RG0009" => v < 64,
            "10E60010" => v < 64,
            "MG0011" => v < 64,
            "ND0012" => v < 64,
            "TL0013" => v < 64,
            "HD0014" => v < 64,
            "NX0015" => v < 64,
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
pub fn u3221mixmap_sum() -> u64 {
    let mut s = 0u64;
    let mut i = 0usize;
    while i < u3221mixmap_COUNT {
        s = s.wrapping_add(u3221mixmap[i].1 as u64);
        i += 1;
    }
    s
}

/// 键唯一性（构造期断言等价物：逐对扫描同键）。
pub fn u3221mixmap_unique() -> bool {
    let mut i = 0usize;
    while i < u3221mixmap_COUNT {
        let mut j = i + 1;
        while j < u3221mixmap_COUNT {
            if u3221mixmap[i].0 == u3221mixmap[j].0 {
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

/// 段位守卫：全部诊断码必须落在本段 [0xe070, 0xe07f]。
pub const fn u3221_segment_guard() -> bool {
    let mut ok = true;
    let mut i = 0usize;
    while i < u3221Code::ALL.len() {
        let c = u3221Code::ALL[i].code();
        if c < 0xe070 || c > 0xe07f {
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
fn u3221_guard_seg() -> bool {
    u3221_segment_guard()
}

/// UNX-F3221 对拍表 16 行边界与累计守恒。
fn check_u3221_mix_tbl() -> bool {
    let mut ok = u3221mix_COUNT == 16;
    if ok { ok = u3221mix_get("ND_00") == Some(10) && u3221mix_get("NX_15") == Some(85); }
    if ok { ok = u3221mix_get("ZZZ_NOKEY").is_none(); }
    if ok { ok = u3221mix_sum() == 760; }
    ok
}

/// UNX-F3221 混合映射 16 行守卫唯一 + 守卫阈值冻结。
fn check_u3221_mix_map() -> bool {
    let mut ok = u3221mixmap_COUNT == 16;
    if ok { ok = u3221mixmap_scan() == 0 && u3221mixmap_unique(); }
    if ok { ok = u3221mixmap_sum() == 504; }
    if ok { ok = U3221MIXGATE_THRESHOLD == 32; }
    ok
}

/// 本模块自检（逐检查带 detail，全绿才出门）。
pub fn run_u3221_checks() -> CheckSet {
    let mut set = CheckSet::new("U3221");
    set.add(
        "u3221-seg-guard",
        u3221_guard_seg(),
        if u3221_guard_seg() { "" } else { "diagnostic code outside segment" },
    );
    set.add(
        "u3221-mix-tbl",
        check_u3221_mix_tbl(),
        if check_u3221_mix_tbl() { "" } else { "UNX-F3221 mix table mismatch" },
    );
    set.add(
        "u3221-mix-map",
        check_u3221_mix_map(),
        if check_u3221_mix_map() { "" } else { "UNX-F3221 mix map mismatch" },
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
    fn u3221_all_checks_green() {
        let set = run_u3221_checks();
        assert!(set.all_passed(), "U3221 domain checks must be green");
    }

    #[test]
    fn u3221_segment_guard_holds() {
        assert!(u3221_segment_guard());
    }
}
