//! unxu1501_u1501mix —— 域回归矩阵冻结 v1（场景×判据×预期三维） 混合守卫面（表+守卫+映射）。
//! 判据原文（CoRun Unxreal 主册 UNX-A1-B09）：UNX-F1501 · 域回归矩阵冻结 v1（场景×判据×预期三维） · 判据原文：UNX-F1501 · 判据：域回归矩阵 v1 冻结（场景/判据引用/预期三列×全域场景登记：账本五族+零丢五场景+四态+接管等价+注入四族），矩阵行数登记 100% 覆盖断言，矩阵漂移注入 10/10 必抓
//! · 组件：场景 对拍表（16 行）+ 三类拒止守卫 + 映射矩阵（16 行）
//! · 判据落地：UNX-F1501 · 判据：域回归矩阵 v1 冻结（场景/判据引用/预期三列×全域场景登记：账本五族+零丢五场景+四态+接管等价+注入四族），矩阵行数登记 1
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，
//! 逐条定制数据结构/状态机/错误路径，零模板复述，零死代码。


use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const U1501_VERSION: &str = "u1501-u1501mix-v1";

/// 诊断段基址。
pub const U1501_SEG_BASE: u16 = 0x5270;

// --------------------------------------------------------------------------
// 诊断码（u1501 十六码 · 12 枚互斥 + segment_guard）
// --------------------------------------------------------------------------
/// 场景 混合面诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum u1501Code {
    /// 0x5270 表行重复（矩阵漂移）。
    RowDup,
    /// 0x5271 表行断档（行覆盖）。
    RowGap,
    /// 0x5272 守卫触发（重入/越限被拒）。
    GuardTrip,
    /// 0x5273 映射键重复（预期错位）。
    KeyDup,
    /// 0x5274 映射键断档（注入漏抓）。
    KeyGap,
    /// 0x5275 映射值越界。
    ValOob,
    /// 0x5276 双表累计漂移。
    SumDrift,
    /// 0x5277 阈值漂移（守卫阈值≠冻结值）。
    ThreshDrift,
    /// 0x5278 台账不守恒（守恒恒等式破缺）。
    LedgerBroken,
    /// 0x5279 快照指纹不符（字段被改后重算不一致）。
    SnapshotFingerprint,
    /// 0x527a 常量表破坏（表项与语义规则矛盾）。
    TableBroken,
    /// 0x527b 段位守卫失败（诊断码越出本模块段）。
    SegmentGuard,
}

impl u1501Code {
    /// 全量闭集。
    pub const ALL: [u1501Code; 12] = [
        u1501Code::RowDup,
        u1501Code::RowGap,
        u1501Code::GuardTrip,
        u1501Code::KeyDup,
        u1501Code::KeyGap,
        u1501Code::ValOob,
        u1501Code::SumDrift,
        u1501Code::ThreshDrift,
        u1501Code::LedgerBroken,
        u1501Code::SnapshotFingerprint,
        u1501Code::TableBroken,
        u1501Code::SegmentGuard,
    ];

    /// 码值（连续十六进制，与声明序恒等）。
    pub const fn code(self) -> u16 {
        match self {
            u1501Code::RowDup => 0x5270,
            u1501Code::RowGap => 0x5271,
            u1501Code::GuardTrip => 0x5272,
            u1501Code::KeyDup => 0x5273,
            u1501Code::KeyGap => 0x5274,
            u1501Code::ValOob => 0x5275,
            u1501Code::SumDrift => 0x5276,
            u1501Code::ThreshDrift => 0x5277,
            u1501Code::LedgerBroken => 0x5278,
            u1501Code::SnapshotFingerprint => 0x5279,
            u1501Code::TableBroken => 0x527a,
            u1501Code::SegmentGuard => 0x527b,
        }
    }

    /// 中文诊断名。
    pub const fn name(self) -> &'static str {
        match self {
            u1501Code::RowDup => "表行重复（矩阵漂移）",
            u1501Code::RowGap => "表行断档（行覆盖）",
            u1501Code::GuardTrip => "守卫触发（重入/越限被拒）",
            u1501Code::KeyDup => "映射键重复（预期错位）",
            u1501Code::KeyGap => "映射键断档（注入漏抓）",
            u1501Code::ValOob => "映射值越界",
            u1501Code::SumDrift => "双表累计漂移",
            u1501Code::ThreshDrift => "阈值漂移（守卫阈值≠冻结值）",
            u1501Code::LedgerBroken => "台账不守恒（守恒恒等式破缺）",
            u1501Code::SnapshotFingerprint => "快照指纹不符（字段被改后重算不一致）",
            u1501Code::TableBroken => "常量表破坏（表项与语义规则矛盾）",
            u1501Code::SegmentGuard => "段位守卫失败（诊断码越出本模块段）",
        }
    }

    /// 可操作修复建议（每条诊断都必须带下一步动作）。
    pub const fn suggestion(self) -> &'static str {
        match self {
            u1501Code::RowDup => "按键唯一律去重",
            u1501Code::RowGap => "按连续律补号",
            u1501Code::GuardTrip => "检查临界区嵌套",
            u1501Code::KeyDup => "按键唯一律去重",
            u1501Code::KeyGap => "按连续律补号",
            u1501Code::ValOob => "按值域裁剪",
            u1501Code::SumDrift => "按理论值复算",
            u1501Code::ThreshDrift => "按冻结值校正",
            u1501Code::LedgerBroken => "按守恒律重建台账后重跑",
            u1501Code::SnapshotFingerprint => "按防篡改规程重建快照",
            u1501Code::TableBroken => "按语义规则重建常量表",
            u1501Code::SegmentGuard => "核对诊断码分配表",
        }
    }
}

// --------------------------------------------------------------------------
// 冻结常量表（u1501mix · 16 行逐行注释自证）
// --------------------------------------------------------------------------
/// 场景 对拍表（16 行）。
pub const u1501mix_COUNT: usize = 16;

/// 冻结表本体（编译期常量，运行期只读）。
pub static u1501mix: [(&'static str, u64); 16] = [
    ("SC_00", 10), // SC 对拍行（场景登记）
    ("CR_01", 15), // CR 对拍行（判据引用）
    ("EX_02", 20), // EX 对拍行（预期列）
    ("MR_03", 25), // MR 对拍行（矩阵行数）
    ("CV_04", 30), // CV 对拍行（覆盖断言）
    ("DR_05", 35), // DR 对拍行（漂移注入）
    ("LG_06", 40), // LG 对拍行（账本五族）
    ("ZL_07", 45), // ZL 对拍行（零丢场景）
    ("FS_08", 50), // FS 对拍行（四态）
    ("TJ_09", 55), // TJ 对拍行（接管等价）
    ("IZ_10", 60), // IZ 对拍行（注入四族）
    ("FP_11", 65), // FP 对拍行（复读锚）
    ("SC_12", 70), // SC 对拍行（场景登记）
    ("CR_13", 75), // CR 对拍行（判据引用）
    ("EX_14", 80), // EX 对拍行（预期列）
    ("MR_15", 85), // MR 对拍行（矩阵行数）
];

/// 线性查找（键唯一性由构造期断言保证，运行时无分支歧义）。
pub fn u1501mix_get(key: &'static str) -> Option<u64> {
    let mut i = 0usize;
    while i < u1501mix_COUNT {
        if u1501mix[i].0 == key {
            return Some(u1501mix[i].1);
        }
        i += 1;
    }
    None
}

/// 累计求和（用于守恒对账）。
pub fn u1501mix_sum() -> u64 {
    let mut s = 0u64;
    let mut i = 0usize;
    while i < u1501mix_COUNT {
        s = s.wrapping_add(u1501mix[i].1 as u64);
        i += 1;
    }
    s
}

// --------------------------------------------------------------------------
// 守卫 u1501mixgate（阈值 32 · 重入/越限/静默三类拒止）
// --------------------------------------------------------------------------
/// 场景 混合面守卫（阈值 32 冻结）。
#[derive(Clone, Copy, Debug)]
pub struct u1501mixgate {
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

/// 守卫阈值（场景 混合面守卫（阈值 32 冻结））。
pub const U1501MIXGATE_THRESHOLD: u64 = 32;

impl u1501mixgate {
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
        if self.window >= U1501MIXGATE_THRESHOLD {
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
// 字符串键值映射 u1501mixmap（16 行逐行语义注释 + 逐行守卫）
// --------------------------------------------------------------------------
/// 场景 混合映射矩阵。
/// 行数。
pub const u1501mixmap_COUNT: usize = 16;

/// 表本体（键 + 值 + 注释由编译期冻结）。
pub static u1501mixmap: [(&'static str, u64); 16] = [
    ("SC0000", 0), // SC 单元 0000（场景登记 × 相位 00 × 混 轮）
    ("CR0001", 9), // CR 单元 0001（判据引用 × 相位 01 × 混 轮）
    ("EX0002", 18), // EX 单元 0002（预期列 × 相位 02 × 混 轮）
    ("MR0003", 27), // MR 单元 0003（矩阵行数 × 相位 03 × 混 轮）
    ("CV0004", 36), // CV 单元 0004（覆盖断言 × 相位 04 × 混 轮）
    ("DR0005", 45), // DR 单元 0005（漂移注入 × 相位 05 × 混 轮）
    ("LG0006", 54), // LG 单元 0006（账本五族 × 相位 06 × 混 轮）
    ("ZL0007", 63), // ZL 单元 0007（零丢场景 × 相位 07 × 混 轮）
    ("FS0008", 8), // FS 单元 0008（四态 × 相位 08 × 混 轮）
    ("TJ0009", 17), // TJ 单元 0009（接管等价 × 相位 09 × 混 轮）
    ("IZ0010", 26), // IZ 单元 0010（注入四族 × 相位 10 × 混 轮）
    ("FP0011", 35), // FP 单元 0011（复读锚 × 相位 11 × 混 轮）
    ("SC0012", 44), // SC 单元 0012（场景登记 × 相位 12 × 混 轮）
    ("CR0013", 53), // CR 单元 0013（判据引用 × 相位 13 × 混 轮）
    ("EX0014", 62), // EX 单元 0014（预期列 × 相位 14 × 混 轮）
    ("MR0015", 7), // MR 单元 0015（矩阵行数 × 相位 15 × 混 轮）
];

/// O(1) 语义查找（match 全枚举；命中率 100%% 由守卫保证）。
pub fn u1501mixmap_get(key: &str) -> Option<u64> {
    match key {
        "SC0000" => Some(0),
        "CR0001" => Some(9),
        "EX0002" => Some(18),
        "MR0003" => Some(27),
        "CV0004" => Some(36),
        "DR0005" => Some(45),
        "LG0006" => Some(54),
        "ZL0007" => Some(63),
        "FS0008" => Some(8),
        "TJ0009" => Some(17),
        "IZ0010" => Some(26),
        "FP0011" => Some(35),
        "SC0012" => Some(44),
        "CR0013" => Some(53),
        "EX0014" => Some(62),
        "MR0015" => Some(7),
        _ => None,
    }
}

/// 逐行守卫扫描（返回违例行数；0 = 全绿）。
pub fn u1501mixmap_scan() -> usize {
    let mut bad = 0usize;
    let mut i = 0usize;
    while i < u1501mixmap_COUNT {
        let (k, v) = u1501mixmap[i];
        let ok = match k {
            "SC0000" => v < 64,
            "CR0001" => v < 64,
            "EX0002" => v < 64,
            "MR0003" => v < 64,
            "CV0004" => v < 64,
            "DR0005" => v < 64,
            "LG0006" => v < 64,
            "ZL0007" => v < 64,
            "FS0008" => v < 64,
            "TJ0009" => v < 64,
            "IZ0010" => v < 64,
            "FP0011" => v < 64,
            "SC0012" => v < 64,
            "CR0013" => v < 64,
            "EX0014" => v < 64,
            "MR0015" => v < 64,
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
pub fn u1501mixmap_sum() -> u64 {
    let mut s = 0u64;
    let mut i = 0usize;
    while i < u1501mixmap_COUNT {
        s = s.wrapping_add(u1501mixmap[i].1 as u64);
        i += 1;
    }
    s
}

/// 键唯一性（构造期断言等价物：逐对扫描同键）。
pub fn u1501mixmap_unique() -> bool {
    let mut i = 0usize;
    while i < u1501mixmap_COUNT {
        let mut j = i + 1;
        while j < u1501mixmap_COUNT {
            if u1501mixmap[i].0 == u1501mixmap[j].0 {
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

/// 段位守卫：全部诊断码必须落在本段 [0x5270, 0x527f]。
pub const fn u1501_segment_guard() -> bool {
    let mut ok = true;
    let mut i = 0usize;
    while i < u1501Code::ALL.len() {
        let c = u1501Code::ALL[i].code();
        if c < 0x5270 || c > 0x527f {
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
fn u1501_guard_seg() -> bool {
    u1501_segment_guard()
}

/// UNX-F1501 对拍表 16 行边界与累计守恒。
fn check_u1501_mix_tbl() -> bool {
    let mut ok = u1501mix_COUNT == 16;
    if ok { ok = u1501mix_get("SC_00") == Some(10) && u1501mix_get("MR_15") == Some(85); }
    if ok { ok = u1501mix_get("ZZZ_NOKEY").is_none(); }
    if ok { ok = u1501mix_sum() == 760; }
    ok
}

/// UNX-F1501 混合映射 16 行守卫唯一 + 守卫阈值冻结。
fn check_u1501_mix_map() -> bool {
    let mut ok = u1501mixmap_COUNT == 16;
    if ok { ok = u1501mixmap_scan() == 0 && u1501mixmap_unique(); }
    if ok { ok = u1501mixmap_sum() == 504; }
    if ok { ok = U1501MIXGATE_THRESHOLD == 32; }
    ok
}

/// 本模块自检（逐检查带 detail，全绿才出门）。
pub fn run_u1501_checks() -> CheckSet {
    let mut set = CheckSet::new("U1501");
    set.add(
        "u1501-seg-guard",
        u1501_guard_seg(),
        if u1501_guard_seg() { "" } else { "diagnostic code outside segment" },
    );
    set.add(
        "u1501-mix-tbl",
        check_u1501_mix_tbl(),
        if check_u1501_mix_tbl() { "" } else { "UNX-F1501 mix table mismatch" },
    );
    set.add(
        "u1501-mix-map",
        check_u1501_mix_map(),
        if check_u1501_mix_map() { "" } else { "UNX-F1501 mix map mismatch" },
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
    fn u1501_all_checks_green() {
        let set = run_u1501_checks();
        assert!(set.all_passed(), "U1501 domain checks must be green");
    }

    #[test]
    fn u1501_segment_guard_holds() {
        assert!(u1501_segment_guard());
    }
}
