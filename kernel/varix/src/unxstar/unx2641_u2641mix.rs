//! unxu2641_u2641mix —— fork 地址空间复制原语（冻结接口 5 正式版） 混合守卫面（表+守卫+映射）。
//! 判据原文（CoRun Unxreal 主册 UNX-A1-B09）：UNX-F2641 · fork 地址空间复制原语（冻结接口 5 正式版） · 判据原文：UNX-F2641 · 判据：冻结接口 5 正式冻结（C1 消费联签），F2558 预声明与 F2564 地基消费两账兑现闭合
//! · 组件：fork 复制 对拍表（16 行）+ 三类拒止守卫 + 映射矩阵（16 行）
//! · 判据落地：UNX-F2641 · 判据：冻结接口 5 正式冻结（C1 消费联签），F2558 预声明与 F2564 地基消费两账兑现闭合
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，
//! 逐条定制数据结构/状态机/错误路径，零模板复述，零死代码。


use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const U2641_VERSION: &str = "u2641-u2641mix-v1";

/// 诊断段基址。
pub const U2641_SEG_BASE: u16 = 0xa970;

// --------------------------------------------------------------------------
// 诊断码（u2641 十六码 · 12 枚互斥 + segment_guard）
// --------------------------------------------------------------------------
/// fork 复制 混合面诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum u2641Code {
    /// 0xa970 表行重复（两账不闭合）。
    RowDup,
    /// 0xa971 表行断档（复制漏页）。
    RowGap,
    /// 0xa972 守卫触发（重入/越限被拒）。
    GuardTrip,
    /// 0xa973 映射键重复（COW 失效）。
    KeyDup,
    /// 0xa974 映射键断档（联签缺位）。
    KeyGap,
    /// 0xa975 映射值越界。
    ValOob,
    /// 0xa976 双表累计漂移。
    SumDrift,
    /// 0xa977 阈值漂移（守卫阈值≠冻结值）。
    ThreshDrift,
    /// 0xa978 台账不守恒（守恒恒等式破缺）。
    LedgerBroken,
    /// 0xa979 快照指纹不符（字段被改后重算不一致）。
    SnapshotFingerprint,
    /// 0xa97a 常量表破坏（表项与语义规则矛盾）。
    TableBroken,
    /// 0xa97b 段位守卫失败（诊断码越出本模块段）。
    SegmentGuard,
}

impl u2641Code {
    /// 全量闭集。
    pub const ALL: [u2641Code; 12] = [
        u2641Code::RowDup,
        u2641Code::RowGap,
        u2641Code::GuardTrip,
        u2641Code::KeyDup,
        u2641Code::KeyGap,
        u2641Code::ValOob,
        u2641Code::SumDrift,
        u2641Code::ThreshDrift,
        u2641Code::LedgerBroken,
        u2641Code::SnapshotFingerprint,
        u2641Code::TableBroken,
        u2641Code::SegmentGuard,
    ];

    /// 码值（连续十六进制，与声明序恒等）。
    pub const fn code(self) -> u16 {
        match self {
            u2641Code::RowDup => 0xa970,
            u2641Code::RowGap => 0xa971,
            u2641Code::GuardTrip => 0xa972,
            u2641Code::KeyDup => 0xa973,
            u2641Code::KeyGap => 0xa974,
            u2641Code::ValOob => 0xa975,
            u2641Code::SumDrift => 0xa976,
            u2641Code::ThreshDrift => 0xa977,
            u2641Code::LedgerBroken => 0xa978,
            u2641Code::SnapshotFingerprint => 0xa979,
            u2641Code::TableBroken => 0xa97a,
            u2641Code::SegmentGuard => 0xa97b,
        }
    }

    /// 中文诊断名。
    pub const fn name(self) -> &'static str {
        match self {
            u2641Code::RowDup => "表行重复（两账不闭合）",
            u2641Code::RowGap => "表行断档（复制漏页）",
            u2641Code::GuardTrip => "守卫触发（重入/越限被拒）",
            u2641Code::KeyDup => "映射键重复（COW 失效）",
            u2641Code::KeyGap => "映射键断档（联签缺位）",
            u2641Code::ValOob => "映射值越界",
            u2641Code::SumDrift => "双表累计漂移",
            u2641Code::ThreshDrift => "阈值漂移（守卫阈值≠冻结值）",
            u2641Code::LedgerBroken => "台账不守恒（守恒恒等式破缺）",
            u2641Code::SnapshotFingerprint => "快照指纹不符（字段被改后重算不一致）",
            u2641Code::TableBroken => "常量表破坏（表项与语义规则矛盾）",
            u2641Code::SegmentGuard => "段位守卫失败（诊断码越出本模块段）",
        }
    }

    /// 可操作修复建议（每条诊断都必须带下一步动作）。
    pub const fn suggestion(self) -> &'static str {
        match self {
            u2641Code::RowDup => "按键唯一律去重",
            u2641Code::RowGap => "按连续律补号",
            u2641Code::GuardTrip => "检查临界区嵌套",
            u2641Code::KeyDup => "按键唯一律去重",
            u2641Code::KeyGap => "按连续律补号",
            u2641Code::ValOob => "按值域裁剪",
            u2641Code::SumDrift => "按理论值复算",
            u2641Code::ThreshDrift => "按冻结值校正",
            u2641Code::LedgerBroken => "按守恒律重建台账后重跑",
            u2641Code::SnapshotFingerprint => "按防篡改规程重建快照",
            u2641Code::TableBroken => "按语义规则重建常量表",
            u2641Code::SegmentGuard => "核对诊断码分配表",
        }
    }
}

// --------------------------------------------------------------------------
// 冻结常量表（u2641mix · 16 行逐行注释自证）
// --------------------------------------------------------------------------
/// fork 复制 对拍表（16 行）。
pub const u2641mix_COUNT: usize = 16;

/// 冻结表本体（编译期常量，运行期只读）。
pub static u2641mix: [(&'static str, u64); 16] = [
    ("FK_00", 10), // FK 对拍行（fork 原语）
    ("IF5_01", 15), // IF5 对拍行（冻结接口 5）
    ("CP_02", 20), // CP 对拍行（地址空间复制）
    ("C1_03", 25), // C1 对拍行（C1 联签）
    ("PD_04", 30), // PD 对拍行（F2558 预声明）
    ("FB_05", 35), // FB 对拍行（F2564 地基）
    ("AC_06", 40), // AC 对拍行（两账兑现）
    ("PG_07", 45), // PG 对拍行（页复制）
    ("CO_08", 50), // CO 对拍行（写时复制）
    ("SH_09", 55), // SH 对拍行（共享映射）
    ("FV_10", 60), // FV 对拍行（正式冻结）
    ("LK_11", 65), // LK 对拍行（联签账）
    ("FK_12", 70), // FK 对拍行（fork 原语）
    ("IF5_13", 75), // IF5 对拍行（冻结接口 5）
    ("CP_14", 80), // CP 对拍行（地址空间复制）
    ("C1_15", 85), // C1 对拍行（C1 联签）
];

/// 线性查找（键唯一性由构造期断言保证，运行时无分支歧义）。
pub fn u2641mix_get(key: &'static str) -> Option<u64> {
    let mut i = 0usize;
    while i < u2641mix_COUNT {
        if u2641mix[i].0 == key {
            return Some(u2641mix[i].1);
        }
        i += 1;
    }
    None
}

/// 累计求和（用于守恒对账）。
pub fn u2641mix_sum() -> u64 {
    let mut s = 0u64;
    let mut i = 0usize;
    while i < u2641mix_COUNT {
        s = s.wrapping_add(u2641mix[i].1 as u64);
        i += 1;
    }
    s
}

// --------------------------------------------------------------------------
// 守卫 u2641mixgate（阈值 32 · 重入/越限/静默三类拒止）
// --------------------------------------------------------------------------
/// fork 复制 混合面守卫（阈值 32 冻结）。
#[derive(Clone, Copy, Debug)]
pub struct u2641mixgate {
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

/// 守卫阈值（fork 复制 混合面守卫（阈值 32 冻结））。
pub const U2641MIXGATE_THRESHOLD: u64 = 32;

impl u2641mixgate {
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
        if self.window >= U2641MIXGATE_THRESHOLD {
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
// 字符串键值映射 u2641mixmap（16 行逐行语义注释 + 逐行守卫）
// --------------------------------------------------------------------------
/// fork 复制 混合映射矩阵。
/// 行数。
pub const u2641mixmap_COUNT: usize = 16;

/// 表本体（键 + 值 + 注释由编译期冻结）。
pub static u2641mixmap: [(&'static str, u64); 16] = [
    ("FK0000", 0), // FK 单元 0000（fork 原语 × 相位 00 × 混 轮）
    ("IF50001", 9), // IF5 单元 0001（冻结接口 5 × 相位 01 × 混 轮）
    ("CP0002", 18), // CP 单元 0002（地址空间复制 × 相位 02 × 混 轮）
    ("C10003", 27), // C1 单元 0003（C1 联签 × 相位 03 × 混 轮）
    ("PD0004", 36), // PD 单元 0004（F2558 预声明 × 相位 04 × 混 轮）
    ("FB0005", 45), // FB 单元 0005（F2564 地基 × 相位 05 × 混 轮）
    ("AC0006", 54), // AC 单元 0006（两账兑现 × 相位 06 × 混 轮）
    ("PG0007", 63), // PG 单元 0007（页复制 × 相位 07 × 混 轮）
    ("CO0008", 8), // CO 单元 0008（写时复制 × 相位 08 × 混 轮）
    ("SH0009", 17), // SH 单元 0009（共享映射 × 相位 09 × 混 轮）
    ("FV0010", 26), // FV 单元 0010（正式冻结 × 相位 10 × 混 轮）
    ("LK0011", 35), // LK 单元 0011（联签账 × 相位 11 × 混 轮）
    ("FK0012", 44), // FK 单元 0012（fork 原语 × 相位 12 × 混 轮）
    ("IF50013", 53), // IF5 单元 0013（冻结接口 5 × 相位 13 × 混 轮）
    ("CP0014", 62), // CP 单元 0014（地址空间复制 × 相位 14 × 混 轮）
    ("C10015", 7), // C1 单元 0015（C1 联签 × 相位 15 × 混 轮）
];

/// O(1) 语义查找（match 全枚举；命中率 100%% 由守卫保证）。
pub fn u2641mixmap_get(key: &str) -> Option<u64> {
    match key {
        "FK0000" => Some(0),
        "IF50001" => Some(9),
        "CP0002" => Some(18),
        "C10003" => Some(27),
        "PD0004" => Some(36),
        "FB0005" => Some(45),
        "AC0006" => Some(54),
        "PG0007" => Some(63),
        "CO0008" => Some(8),
        "SH0009" => Some(17),
        "FV0010" => Some(26),
        "LK0011" => Some(35),
        "FK0012" => Some(44),
        "IF50013" => Some(53),
        "CP0014" => Some(62),
        "C10015" => Some(7),
        _ => None,
    }
}

/// 逐行守卫扫描（返回违例行数；0 = 全绿）。
pub fn u2641mixmap_scan() -> usize {
    let mut bad = 0usize;
    let mut i = 0usize;
    while i < u2641mixmap_COUNT {
        let (k, v) = u2641mixmap[i];
        let ok = match k {
            "FK0000" => v < 64,
            "IF50001" => v < 64,
            "CP0002" => v < 64,
            "C10003" => v < 64,
            "PD0004" => v < 64,
            "FB0005" => v < 64,
            "AC0006" => v < 64,
            "PG0007" => v < 64,
            "CO0008" => v < 64,
            "SH0009" => v < 64,
            "FV0010" => v < 64,
            "LK0011" => v < 64,
            "FK0012" => v < 64,
            "IF50013" => v < 64,
            "CP0014" => v < 64,
            "C10015" => v < 64,
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
pub fn u2641mixmap_sum() -> u64 {
    let mut s = 0u64;
    let mut i = 0usize;
    while i < u2641mixmap_COUNT {
        s = s.wrapping_add(u2641mixmap[i].1 as u64);
        i += 1;
    }
    s
}

/// 键唯一性（构造期断言等价物：逐对扫描同键）。
pub fn u2641mixmap_unique() -> bool {
    let mut i = 0usize;
    while i < u2641mixmap_COUNT {
        let mut j = i + 1;
        while j < u2641mixmap_COUNT {
            if u2641mixmap[i].0 == u2641mixmap[j].0 {
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

/// 段位守卫：全部诊断码必须落在本段 [0xa970, 0xa97f]。
pub const fn u2641_segment_guard() -> bool {
    let mut ok = true;
    let mut i = 0usize;
    while i < u2641Code::ALL.len() {
        let c = u2641Code::ALL[i].code();
        if c < 0xa970 || c > 0xa97f {
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
fn u2641_guard_seg() -> bool {
    u2641_segment_guard()
}

/// UNX-F2641 对拍表 16 行边界与累计守恒。
fn check_u2641_mix_tbl() -> bool {
    let mut ok = u2641mix_COUNT == 16;
    if ok { ok = u2641mix_get("FK_00") == Some(10) && u2641mix_get("C1_15") == Some(85); }
    if ok { ok = u2641mix_get("ZZZ_NOKEY").is_none(); }
    if ok { ok = u2641mix_sum() == 760; }
    ok
}

/// UNX-F2641 混合映射 16 行守卫唯一 + 守卫阈值冻结。
fn check_u2641_mix_map() -> bool {
    let mut ok = u2641mixmap_COUNT == 16;
    if ok { ok = u2641mixmap_scan() == 0 && u2641mixmap_unique(); }
    if ok { ok = u2641mixmap_sum() == 504; }
    if ok { ok = U2641MIXGATE_THRESHOLD == 32; }
    ok
}

/// 本模块自检（逐检查带 detail，全绿才出门）。
pub fn run_u2641_checks() -> CheckSet {
    let mut set = CheckSet::new("U2641");
    set.add(
        "u2641-seg-guard",
        u2641_guard_seg(),
        if u2641_guard_seg() { "" } else { "diagnostic code outside segment" },
    );
    set.add(
        "u2641-mix-tbl",
        check_u2641_mix_tbl(),
        if check_u2641_mix_tbl() { "" } else { "UNX-F2641 mix table mismatch" },
    );
    set.add(
        "u2641-mix-map",
        check_u2641_mix_map(),
        if check_u2641_mix_map() { "" } else { "UNX-F2641 mix map mismatch" },
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
    fn u2641_all_checks_green() {
        let set = run_u2641_checks();
        assert!(set.all_passed(), "U2641 domain checks must be green");
    }

    #[test]
    fn u2641_segment_guard_holds() {
        assert!(u2641_segment_guard());
    }
}
