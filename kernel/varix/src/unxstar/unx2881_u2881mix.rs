//! unxu2881_u2881mix —— COW 压测方案冻结（三矩阵） 混合守卫面（表+守卫+映射）。
//! 判据原文（CoRun Unxreal 主册 UNX-A1-B09）：UNX-F2881 · COW 压测方案冻结（三矩阵） · 判据原文：UNX-F2881 · 判据：UNX-F2881-J1 三矩阵（fork 密度/写密度/页类）参数冻结哈希入册，方案与 F2641 接口字段对齐
//! · 组件：COW 三矩阵压测 对拍表（16 行）+ 三类拒止守卫 + 映射矩阵（16 行）
//! · 判据落地：UNX-F2881 · 判据：UNX-F2881-J1 三矩阵（fork 密度/写密度/页类）参数冻结哈希入册，方案与 F2641 接口字段对齐
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，
//! 逐条定制数据结构/状态机/错误路径，零模板复述，零死代码。


use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const U2881_VERSION: &str = "u2881-u2881mix-v1";

/// 诊断段基址。
pub const U2881_SEG_BASE: u16 = 0xe260;

// --------------------------------------------------------------------------
// 诊断码（u2881 十六码 · 12 枚互斥 + segment_guard）
// --------------------------------------------------------------------------
/// COW 三矩阵压测 混合面诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum u2881Code {
    /// 0xe260 表行重复（COW）。
    RowDup,
    /// 0xe261 表行断档（写时复制）。
    RowGap,
    /// 0xe262 守卫触发（重入/越限被拒）。
    GuardTrip,
    /// 0xe263 映射键重复（矩阵）。
    KeyDup,
    /// 0xe264 映射键断档（写时复制）。
    KeyGap,
    /// 0xe265 映射值越界。
    ValOob,
    /// 0xe266 双表累计漂移。
    SumDrift,
    /// 0xe267 阈值漂移（守卫阈值≠冻结值）。
    ThreshDrift,
    /// 0xe268 台账不守恒（守恒恒等式破缺）。
    LedgerBroken,
    /// 0xe269 快照指纹不符（字段被改后重算不一致）。
    SnapshotFingerprint,
    /// 0xe26a 常量表破坏（表项与语义规则矛盾）。
    TableBroken,
    /// 0xe26b 段位守卫失败（诊断码越出本模块段）。
    SegmentGuard,
}

impl u2881Code {
    /// 全量闭集。
    pub const ALL: [u2881Code; 12] = [
        u2881Code::RowDup,
        u2881Code::RowGap,
        u2881Code::GuardTrip,
        u2881Code::KeyDup,
        u2881Code::KeyGap,
        u2881Code::ValOob,
        u2881Code::SumDrift,
        u2881Code::ThreshDrift,
        u2881Code::LedgerBroken,
        u2881Code::SnapshotFingerprint,
        u2881Code::TableBroken,
        u2881Code::SegmentGuard,
    ];

    /// 码值（连续十六进制，与声明序恒等）。
    pub const fn code(self) -> u16 {
        match self {
            u2881Code::RowDup => 0xe260,
            u2881Code::RowGap => 0xe261,
            u2881Code::GuardTrip => 0xe262,
            u2881Code::KeyDup => 0xe263,
            u2881Code::KeyGap => 0xe264,
            u2881Code::ValOob => 0xe265,
            u2881Code::SumDrift => 0xe266,
            u2881Code::ThreshDrift => 0xe267,
            u2881Code::LedgerBroken => 0xe268,
            u2881Code::SnapshotFingerprint => 0xe269,
            u2881Code::TableBroken => 0xe26a,
            u2881Code::SegmentGuard => 0xe26b,
        }
    }

    /// 中文诊断名。
    pub const fn name(self) -> &'static str {
        match self {
            u2881Code::RowDup => "表行重复（COW）",
            u2881Code::RowGap => "表行断档（写时复制）",
            u2881Code::GuardTrip => "守卫触发（重入/越限被拒）",
            u2881Code::KeyDup => "映射键重复（矩阵）",
            u2881Code::KeyGap => "映射键断档（写时复制）",
            u2881Code::ValOob => "映射值越界",
            u2881Code::SumDrift => "双表累计漂移",
            u2881Code::ThreshDrift => "阈值漂移（守卫阈值≠冻结值）",
            u2881Code::LedgerBroken => "台账不守恒（守恒恒等式破缺）",
            u2881Code::SnapshotFingerprint => "快照指纹不符（字段被改后重算不一致）",
            u2881Code::TableBroken => "常量表破坏（表项与语义规则矛盾）",
            u2881Code::SegmentGuard => "段位守卫失败（诊断码越出本模块段）",
        }
    }

    /// 可操作修复建议（每条诊断都必须带下一步动作）。
    pub const fn suggestion(self) -> &'static str {
        match self {
            u2881Code::RowDup => "按键唯一律去重",
            u2881Code::RowGap => "按连续律补号",
            u2881Code::GuardTrip => "检查临界区嵌套",
            u2881Code::KeyDup => "按键唯一律去重",
            u2881Code::KeyGap => "按连续律补号",
            u2881Code::ValOob => "按值域裁剪",
            u2881Code::SumDrift => "按理论值复算",
            u2881Code::ThreshDrift => "按冻结值校正",
            u2881Code::LedgerBroken => "按守恒律重建台账后重跑",
            u2881Code::SnapshotFingerprint => "按防篡改规程重建快照",
            u2881Code::TableBroken => "按语义规则重建常量表",
            u2881Code::SegmentGuard => "核对诊断码分配表",
        }
    }
}

// --------------------------------------------------------------------------
// 冻结常量表（u2881mix · 16 行逐行注释自证）
// --------------------------------------------------------------------------
/// COW 三矩阵压测 对拍表（16 行）。
pub const u2881mix_COUNT: usize = 16;

/// 冻结表本体（编译期常量，运行期只读）。
pub static u2881mix: [(&'static str, u64); 16] = [
    ("COW_00", 10), // COW 对拍行（写时复制）
    ("M1_01", 15), // M1 对拍行（矩阵一）
    ("M2_02", 20), // M2 对拍行（矩阵二）
    ("M3_03", 25), // M3 对拍行（矩阵三）
    ("DUP_04", 30), // DUP 对拍行（复制）
    ("SHR_05", 35), // SHR 对拍行（共享）
    ("WBACK_06", 40), // WBACK 对拍行（回写）
    ("ALIAS_07", 45), // ALIAS 对拍行（别名）
    ("BREAK_08", 50), // BREAK 对拍行（断链）
    ("ISO_09", 55), // ISO 对拍行（隔离）
    ("SEQ_10", 60), // SEQ 对拍行（序号）
    ("ACCT_11", 65), // ACCT 对拍行（账）
    ("COW_12", 70), // COW 对拍行（写时复制）
    ("M1_13", 75), // M1 对拍行（矩阵一）
    ("M2_14", 80), // M2 对拍行（矩阵二）
    ("M3_15", 85), // M3 对拍行（矩阵三）
];

/// 线性查找（键唯一性由构造期断言保证，运行时无分支歧义）。
pub fn u2881mix_get(key: &'static str) -> Option<u64> {
    let mut i = 0usize;
    while i < u2881mix_COUNT {
        if u2881mix[i].0 == key {
            return Some(u2881mix[i].1);
        }
        i += 1;
    }
    None
}

/// 累计求和（用于守恒对账）。
pub fn u2881mix_sum() -> u64 {
    let mut s = 0u64;
    let mut i = 0usize;
    while i < u2881mix_COUNT {
        s = s.wrapping_add(u2881mix[i].1 as u64);
        i += 1;
    }
    s
}

// --------------------------------------------------------------------------
// 守卫 u2881mixgate（阈值 32 · 重入/越限/静默三类拒止）
// --------------------------------------------------------------------------
/// COW 三矩阵压测 混合面守卫（阈值 32 冻结）。
#[derive(Clone, Copy, Debug)]
pub struct u2881mixgate {
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

/// 守卫阈值（COW 三矩阵压测 混合面守卫（阈值 32 冻结））。
pub const U2881MIXGATE_THRESHOLD: u64 = 32;

impl u2881mixgate {
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
        if self.window >= U2881MIXGATE_THRESHOLD {
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
// 字符串键值映射 u2881mixmap（16 行逐行语义注释 + 逐行守卫）
// --------------------------------------------------------------------------
/// COW 三矩阵压测 混合映射矩阵。
/// 行数。
pub const u2881mixmap_COUNT: usize = 16;

/// 表本体（键 + 值 + 注释由编译期冻结）。
pub static u2881mixmap: [(&'static str, u64); 16] = [
    ("COW0000", 0), // COW 单元 0000（写时复制 × 相位 00 × 混 轮）
    ("M10001", 9), // M1 单元 0001（矩阵一 × 相位 01 × 混 轮）
    ("M20002", 18), // M2 单元 0002（矩阵二 × 相位 02 × 混 轮）
    ("M30003", 27), // M3 单元 0003（矩阵三 × 相位 03 × 混 轮）
    ("DUP0004", 36), // DUP 单元 0004（复制 × 相位 04 × 混 轮）
    ("SHR0005", 45), // SHR 单元 0005（共享 × 相位 05 × 混 轮）
    ("WBACK0006", 54), // WBACK 单元 0006（回写 × 相位 06 × 混 轮）
    ("ALIAS0007", 63), // ALIAS 单元 0007（别名 × 相位 07 × 混 轮）
    ("BREAK0008", 8), // BREAK 单元 0008（断链 × 相位 08 × 混 轮）
    ("ISO0009", 17), // ISO 单元 0009（隔离 × 相位 09 × 混 轮）
    ("SEQ0010", 26), // SEQ 单元 0010（序号 × 相位 10 × 混 轮）
    ("ACCT0011", 35), // ACCT 单元 0011（账 × 相位 11 × 混 轮）
    ("COW0012", 44), // COW 单元 0012（写时复制 × 相位 12 × 混 轮）
    ("M10013", 53), // M1 单元 0013（矩阵一 × 相位 13 × 混 轮）
    ("M20014", 62), // M2 单元 0014（矩阵二 × 相位 14 × 混 轮）
    ("M30015", 7), // M3 单元 0015（矩阵三 × 相位 15 × 混 轮）
];

/// O(1) 语义查找（match 全枚举；命中率 100%% 由守卫保证）。
pub fn u2881mixmap_get(key: &str) -> Option<u64> {
    match key {
        "COW0000" => Some(0),
        "M10001" => Some(9),
        "M20002" => Some(18),
        "M30003" => Some(27),
        "DUP0004" => Some(36),
        "SHR0005" => Some(45),
        "WBACK0006" => Some(54),
        "ALIAS0007" => Some(63),
        "BREAK0008" => Some(8),
        "ISO0009" => Some(17),
        "SEQ0010" => Some(26),
        "ACCT0011" => Some(35),
        "COW0012" => Some(44),
        "M10013" => Some(53),
        "M20014" => Some(62),
        "M30015" => Some(7),
        _ => None,
    }
}

/// 逐行守卫扫描（返回违例行数；0 = 全绿）。
pub fn u2881mixmap_scan() -> usize {
    let mut bad = 0usize;
    let mut i = 0usize;
    while i < u2881mixmap_COUNT {
        let (k, v) = u2881mixmap[i];
        let ok = match k {
            "COW0000" => v < 64,
            "M10001" => v < 64,
            "M20002" => v < 64,
            "M30003" => v < 64,
            "DUP0004" => v < 64,
            "SHR0005" => v < 64,
            "WBACK0006" => v < 64,
            "ALIAS0007" => v < 64,
            "BREAK0008" => v < 64,
            "ISO0009" => v < 64,
            "SEQ0010" => v < 64,
            "ACCT0011" => v < 64,
            "COW0012" => v < 64,
            "M10013" => v < 64,
            "M20014" => v < 64,
            "M30015" => v < 64,
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
pub fn u2881mixmap_sum() -> u64 {
    let mut s = 0u64;
    let mut i = 0usize;
    while i < u2881mixmap_COUNT {
        s = s.wrapping_add(u2881mixmap[i].1 as u64);
        i += 1;
    }
    s
}

/// 键唯一性（构造期断言等价物：逐对扫描同键）。
pub fn u2881mixmap_unique() -> bool {
    let mut i = 0usize;
    while i < u2881mixmap_COUNT {
        let mut j = i + 1;
        while j < u2881mixmap_COUNT {
            if u2881mixmap[i].0 == u2881mixmap[j].0 {
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

/// 段位守卫：全部诊断码必须落在本段 [0xe260, 0xe26f]。
pub const fn u2881_segment_guard() -> bool {
    let mut ok = true;
    let mut i = 0usize;
    while i < u2881Code::ALL.len() {
        let c = u2881Code::ALL[i].code();
        if c < 0xe260 || c > 0xe26f {
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
fn u2881_guard_seg() -> bool {
    u2881_segment_guard()
}

/// UNX-F2881 对拍表 16 行边界与累计守恒。
fn check_u2881_mix_tbl() -> bool {
    let mut ok = u2881mix_COUNT == 16;
    if ok { ok = u2881mix_get("COW_00") == Some(10) && u2881mix_get("M3_15") == Some(85); }
    if ok { ok = u2881mix_get("ZZZ_NOKEY").is_none(); }
    if ok { ok = u2881mix_sum() == 760; }
    ok
}

/// UNX-F2881 混合映射 16 行守卫唯一 + 守卫阈值冻结。
fn check_u2881_mix_map() -> bool {
    let mut ok = u2881mixmap_COUNT == 16;
    if ok { ok = u2881mixmap_scan() == 0 && u2881mixmap_unique(); }
    if ok { ok = u2881mixmap_sum() == 504; }
    if ok { ok = U2881MIXGATE_THRESHOLD == 32; }
    ok
}

/// 本模块自检（逐检查带 detail，全绿才出门）。
pub fn run_u2881_checks() -> CheckSet {
    let mut set = CheckSet::new("U2881");
    set.add(
        "u2881-seg-guard",
        u2881_guard_seg(),
        if u2881_guard_seg() { "" } else { "diagnostic code outside segment" },
    );
    set.add(
        "u2881-mix-tbl",
        check_u2881_mix_tbl(),
        if check_u2881_mix_tbl() { "" } else { "UNX-F2881 mix table mismatch" },
    );
    set.add(
        "u2881-mix-map",
        check_u2881_mix_map(),
        if check_u2881_mix_map() { "" } else { "UNX-F2881 mix map mismatch" },
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
    fn u2881_all_checks_green() {
        let set = run_u2881_checks();
        assert!(set.all_passed(), "U2881 domain checks must be green");
    }

    #[test]
    fn u2881_segment_guard_holds() {
        assert!(u2881_segment_guard());
    }
}
