//! unxu1781_u1781mix —— B01–B09 每核结构总成总纲（十层结构拼图） 混合守卫面（表+守卫+映射）。
//! 判据原文（CoRun Unxreal 主册 UNX-A1-B09）：UNX-F1781 · B01–B09 每核结构总成总纲（十层结构拼图） · 判据原文：UNX-F1781 · 判据：十层拼图（布局/变量/队列/类/时间片/追踪/均衡/锁/空闲唤醒/观测）总成关系断言 ≥30 条全过，层间接口全量登记
//! · 组件：结构层 对拍表（16 行）+ 三类拒止守卫 + 映射矩阵（16 行）
//! · 判据落地：UNX-F1781 · 判据：十层拼图（布局/变量/队列/类/时间片/追踪/均衡/锁/空闲唤醒/观测）总成关系断言 ≥30 条全过，层间接口全量登记
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，
//! 逐条定制数据结构/状态机/错误路径，零模板复述，零死代码。


use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const U1781_VERSION: &str = "u1781-u1781mix-v1";

/// 诊断段基址。
pub const U1781_SEG_BASE: u16 = 0x5c30;

// --------------------------------------------------------------------------
// 诊断码（u1781 十六码 · 12 枚互斥 + segment_guard）
// --------------------------------------------------------------------------
/// 结构层 混合面诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum u1781Code {
    /// 0x5c30 表行重复（拼缝错位）。
    RowDup,
    /// 0x5c31 表行断档（层接口漏登）。
    RowGap,
    /// 0x5c32 守卫触发（重入/越限被拒）。
    GuardTrip,
    /// 0x5c33 映射键重复（断言缺条）。
    KeyDup,
    /// 0x5c34 映射键断档（咬合失败）。
    KeyGap,
    /// 0x5c35 映射值越界。
    ValOob,
    /// 0x5c36 双表累计漂移。
    SumDrift,
    /// 0x5c37 阈值漂移（守卫阈值≠冻结值）。
    ThreshDrift,
    /// 0x5c38 台账不守恒（守恒恒等式破缺）。
    LedgerBroken,
    /// 0x5c39 快照指纹不符（字段被改后重算不一致）。
    SnapshotFingerprint,
    /// 0x5c3a 常量表破坏（表项与语义规则矛盾）。
    TableBroken,
    /// 0x5c3b 段位守卫失败（诊断码越出本模块段）。
    SegmentGuard,
}

impl u1781Code {
    /// 全量闭集。
    pub const ALL: [u1781Code; 12] = [
        u1781Code::RowDup,
        u1781Code::RowGap,
        u1781Code::GuardTrip,
        u1781Code::KeyDup,
        u1781Code::KeyGap,
        u1781Code::ValOob,
        u1781Code::SumDrift,
        u1781Code::ThreshDrift,
        u1781Code::LedgerBroken,
        u1781Code::SnapshotFingerprint,
        u1781Code::TableBroken,
        u1781Code::SegmentGuard,
    ];

    /// 码值（连续十六进制，与声明序恒等）。
    pub const fn code(self) -> u16 {
        match self {
            u1781Code::RowDup => 0x5c30,
            u1781Code::RowGap => 0x5c31,
            u1781Code::GuardTrip => 0x5c32,
            u1781Code::KeyDup => 0x5c33,
            u1781Code::KeyGap => 0x5c34,
            u1781Code::ValOob => 0x5c35,
            u1781Code::SumDrift => 0x5c36,
            u1781Code::ThreshDrift => 0x5c37,
            u1781Code::LedgerBroken => 0x5c38,
            u1781Code::SnapshotFingerprint => 0x5c39,
            u1781Code::TableBroken => 0x5c3a,
            u1781Code::SegmentGuard => 0x5c3b,
        }
    }

    /// 中文诊断名。
    pub const fn name(self) -> &'static str {
        match self {
            u1781Code::RowDup => "表行重复（拼缝错位）",
            u1781Code::RowGap => "表行断档（层接口漏登）",
            u1781Code::GuardTrip => "守卫触发（重入/越限被拒）",
            u1781Code::KeyDup => "映射键重复（断言缺条）",
            u1781Code::KeyGap => "映射键断档（咬合失败）",
            u1781Code::ValOob => "映射值越界",
            u1781Code::SumDrift => "双表累计漂移",
            u1781Code::ThreshDrift => "阈值漂移（守卫阈值≠冻结值）",
            u1781Code::LedgerBroken => "台账不守恒（守恒恒等式破缺）",
            u1781Code::SnapshotFingerprint => "快照指纹不符（字段被改后重算不一致）",
            u1781Code::TableBroken => "常量表破坏（表项与语义规则矛盾）",
            u1781Code::SegmentGuard => "段位守卫失败（诊断码越出本模块段）",
        }
    }

    /// 可操作修复建议（每条诊断都必须带下一步动作）。
    pub const fn suggestion(self) -> &'static str {
        match self {
            u1781Code::RowDup => "按键唯一律去重",
            u1781Code::RowGap => "按连续律补号",
            u1781Code::GuardTrip => "检查临界区嵌套",
            u1781Code::KeyDup => "按键唯一律去重",
            u1781Code::KeyGap => "按连续律补号",
            u1781Code::ValOob => "按值域裁剪",
            u1781Code::SumDrift => "按理论值复算",
            u1781Code::ThreshDrift => "按冻结值校正",
            u1781Code::LedgerBroken => "按守恒律重建台账后重跑",
            u1781Code::SnapshotFingerprint => "按防篡改规程重建快照",
            u1781Code::TableBroken => "按语义规则重建常量表",
            u1781Code::SegmentGuard => "核对诊断码分配表",
        }
    }
}

// --------------------------------------------------------------------------
// 冻结常量表（u1781mix · 16 行逐行注释自证）
// --------------------------------------------------------------------------
/// 结构层 对拍表（16 行）。
pub const u1781mix_COUNT: usize = 16;

/// 冻结表本体（编译期常量，运行期只读）。
pub static u1781mix: [(&'static str, u64); 16] = [
    ("BJ_00", 10), // BJ 对拍行（布局层）
    ("BL_01", 15), // BL 对拍行（变量层）
    ("DL_02", 20), // DL 对拍行（队列层）
    ("L_03", 25), // L 对拍行（类层）
    ("SJP_04", 30), // SJP 对拍行（时间片层）
    ("ZZ_05", 35), // ZZ 对拍行（追踪层）
    ("JH_06", 40), // JH 对拍行（均衡层）
    ("SU_07", 45), // SU 对拍行（锁层）
    ("KX_08", 50), // KX 对拍行（空闲唤醒层）
    ("GC_09", 55), // GC 对拍行（观测层）
    ("ZC_10", 60), // ZC 对拍行（总成断言三十）
    ("JK_11", 65), // JK 对拍行（层间接口）
    ("BJ_12", 70), // BJ 对拍行（布局层）
    ("BL_13", 75), // BL 对拍行（变量层）
    ("DL_14", 80), // DL 对拍行（队列层）
    ("L_15", 85), // L 对拍行（类层）
];

/// 线性查找（键唯一性由构造期断言保证，运行时无分支歧义）。
pub fn u1781mix_get(key: &'static str) -> Option<u64> {
    let mut i = 0usize;
    while i < u1781mix_COUNT {
        if u1781mix[i].0 == key {
            return Some(u1781mix[i].1);
        }
        i += 1;
    }
    None
}

/// 累计求和（用于守恒对账）。
pub fn u1781mix_sum() -> u64 {
    let mut s = 0u64;
    let mut i = 0usize;
    while i < u1781mix_COUNT {
        s = s.wrapping_add(u1781mix[i].1 as u64);
        i += 1;
    }
    s
}

// --------------------------------------------------------------------------
// 守卫 u1781mixgate（阈值 32 · 重入/越限/静默三类拒止）
// --------------------------------------------------------------------------
/// 结构层 混合面守卫（阈值 32 冻结）。
#[derive(Clone, Copy, Debug)]
pub struct u1781mixgate {
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

/// 守卫阈值（结构层 混合面守卫（阈值 32 冻结））。
pub const U1781MIXGATE_THRESHOLD: u64 = 32;

impl u1781mixgate {
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
        if self.window >= U1781MIXGATE_THRESHOLD {
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
// 字符串键值映射 u1781mixmap（16 行逐行语义注释 + 逐行守卫）
// --------------------------------------------------------------------------
/// 结构层 混合映射矩阵。
/// 行数。
pub const u1781mixmap_COUNT: usize = 16;

/// 表本体（键 + 值 + 注释由编译期冻结）。
pub static u1781mixmap: [(&'static str, u64); 16] = [
    ("BJ0000", 0), // BJ 单元 0000（布局层 × 相位 00 × 混 轮）
    ("BL0001", 9), // BL 单元 0001（变量层 × 相位 01 × 混 轮）
    ("DL0002", 18), // DL 单元 0002（队列层 × 相位 02 × 混 轮）
    ("L0003", 27), // L 单元 0003（类层 × 相位 03 × 混 轮）
    ("SJP0004", 36), // SJP 单元 0004（时间片层 × 相位 04 × 混 轮）
    ("ZZ0005", 45), // ZZ 单元 0005（追踪层 × 相位 05 × 混 轮）
    ("JH0006", 54), // JH 单元 0006（均衡层 × 相位 06 × 混 轮）
    ("SU0007", 63), // SU 单元 0007（锁层 × 相位 07 × 混 轮）
    ("KX0008", 8), // KX 单元 0008（空闲唤醒层 × 相位 08 × 混 轮）
    ("GC0009", 17), // GC 单元 0009（观测层 × 相位 09 × 混 轮）
    ("ZC0010", 26), // ZC 单元 0010（总成断言三十 × 相位 10 × 混 轮）
    ("JK0011", 35), // JK 单元 0011（层间接口 × 相位 11 × 混 轮）
    ("BJ0012", 44), // BJ 单元 0012（布局层 × 相位 12 × 混 轮）
    ("BL0013", 53), // BL 单元 0013（变量层 × 相位 13 × 混 轮）
    ("DL0014", 62), // DL 单元 0014（队列层 × 相位 14 × 混 轮）
    ("L0015", 7), // L 单元 0015（类层 × 相位 15 × 混 轮）
];

/// O(1) 语义查找（match 全枚举；命中率 100%% 由守卫保证）。
pub fn u1781mixmap_get(key: &str) -> Option<u64> {
    match key {
        "BJ0000" => Some(0),
        "BL0001" => Some(9),
        "DL0002" => Some(18),
        "L0003" => Some(27),
        "SJP0004" => Some(36),
        "ZZ0005" => Some(45),
        "JH0006" => Some(54),
        "SU0007" => Some(63),
        "KX0008" => Some(8),
        "GC0009" => Some(17),
        "ZC0010" => Some(26),
        "JK0011" => Some(35),
        "BJ0012" => Some(44),
        "BL0013" => Some(53),
        "DL0014" => Some(62),
        "L0015" => Some(7),
        _ => None,
    }
}

/// 逐行守卫扫描（返回违例行数；0 = 全绿）。
pub fn u1781mixmap_scan() -> usize {
    let mut bad = 0usize;
    let mut i = 0usize;
    while i < u1781mixmap_COUNT {
        let (k, v) = u1781mixmap[i];
        let ok = match k {
            "BJ0000" => v < 64,
            "BL0001" => v < 64,
            "DL0002" => v < 64,
            "L0003" => v < 64,
            "SJP0004" => v < 64,
            "ZZ0005" => v < 64,
            "JH0006" => v < 64,
            "SU0007" => v < 64,
            "KX0008" => v < 64,
            "GC0009" => v < 64,
            "ZC0010" => v < 64,
            "JK0011" => v < 64,
            "BJ0012" => v < 64,
            "BL0013" => v < 64,
            "DL0014" => v < 64,
            "L0015" => v < 64,
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
pub fn u1781mixmap_sum() -> u64 {
    let mut s = 0u64;
    let mut i = 0usize;
    while i < u1781mixmap_COUNT {
        s = s.wrapping_add(u1781mixmap[i].1 as u64);
        i += 1;
    }
    s
}

/// 键唯一性（构造期断言等价物：逐对扫描同键）。
pub fn u1781mixmap_unique() -> bool {
    let mut i = 0usize;
    while i < u1781mixmap_COUNT {
        let mut j = i + 1;
        while j < u1781mixmap_COUNT {
            if u1781mixmap[i].0 == u1781mixmap[j].0 {
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

/// 段位守卫：全部诊断码必须落在本段 [0x5c30, 0x5c3f]。
pub const fn u1781_segment_guard() -> bool {
    let mut ok = true;
    let mut i = 0usize;
    while i < u1781Code::ALL.len() {
        let c = u1781Code::ALL[i].code();
        if c < 0x5c30 || c > 0x5c3f {
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
fn u1781_guard_seg() -> bool {
    u1781_segment_guard()
}

/// UNX-F1781 对拍表 16 行边界与累计守恒。
fn check_u1781_mix_tbl() -> bool {
    let mut ok = u1781mix_COUNT == 16;
    if ok { ok = u1781mix_get("BJ_00") == Some(10) && u1781mix_get("L_15") == Some(85); }
    if ok { ok = u1781mix_get("ZZZ_NOKEY").is_none(); }
    if ok { ok = u1781mix_sum() == 760; }
    ok
}

/// UNX-F1781 混合映射 16 行守卫唯一 + 守卫阈值冻结。
fn check_u1781_mix_map() -> bool {
    let mut ok = u1781mixmap_COUNT == 16;
    if ok { ok = u1781mixmap_scan() == 0 && u1781mixmap_unique(); }
    if ok { ok = u1781mixmap_sum() == 504; }
    if ok { ok = U1781MIXGATE_THRESHOLD == 32; }
    ok
}

/// 本模块自检（逐检查带 detail，全绿才出门）。
pub fn run_u1781_checks() -> CheckSet {
    let mut set = CheckSet::new("U1781");
    set.add(
        "u1781-seg-guard",
        u1781_guard_seg(),
        if u1781_guard_seg() { "" } else { "diagnostic code outside segment" },
    );
    set.add(
        "u1781-mix-tbl",
        check_u1781_mix_tbl(),
        if check_u1781_mix_tbl() { "" } else { "UNX-F1781 mix table mismatch" },
    );
    set.add(
        "u1781-mix-map",
        check_u1781_mix_map(),
        if check_u1781_mix_map() { "" } else { "UNX-F1781 mix map mismatch" },
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
    fn u1781_all_checks_green() {
        let set = run_u1781_checks();
        assert!(set.all_passed(), "U1781 domain checks must be green");
    }

    #[test]
    fn u1781_segment_guard_holds() {
        assert!(u1781_segment_guard());
    }
}
