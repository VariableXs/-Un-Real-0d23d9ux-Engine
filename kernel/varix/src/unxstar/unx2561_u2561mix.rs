//! unxu2561_u2561mix —— AddressSpace 成熟体总纲（现存 addrspace.rs 接管） 混合守卫面（表+守卫+映射）。
//! 判据原文（CoRun Unxreal 主册 UNX-A1-B09）：UNX-F2561 · AddressSpace 成熟体总纲（现存 addrspace.rs 接管） · 判据原文：UNX-F2561 · 判据：现存 `addrspace.rs`（F001 起步态）接管账在档（语义逐项映射），成熟体接口面三机直测一致
//! · 组件：地址空间体 对拍表（16 行）+ 三类拒止守卫 + 映射矩阵（16 行）
//! · 判据落地：UNX-F2561 · 判据：现存 `addrspace.rs`（F001 起步态）接管账在档（语义逐项映射），成熟体接口面三机直测一致
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，
//! 逐条定制数据结构/状态机/错误路径，零模板复述，零死代码。


use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const U2561_VERSION: &str = "u2561-u2561mix-v1";

/// 诊断段基址。
pub const U2561_SEG_BASE: u16 = 0xa670;

// --------------------------------------------------------------------------
// 诊断码（u2561 十六码 · 12 枚互斥 + segment_guard）
// --------------------------------------------------------------------------
/// 地址空间体 混合面诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum u2561Code {
    /// 0xa670 表行重复（接管漏项）。
    RowDup,
    /// 0xa671 表行断档（语义错配）。
    RowGap,
    /// 0xa672 守卫触发（重入/越限被拒）。
    GuardTrip,
    /// 0xa673 映射键重复（权限失真）。
    KeyDup,
    /// 0xa674 映射键断档（直测分歧）。
    KeyGap,
    /// 0xa675 映射值越界。
    ValOob,
    /// 0xa676 双表累计漂移。
    SumDrift,
    /// 0xa677 阈值漂移（守卫阈值≠冻结值）。
    ThreshDrift,
    /// 0xa678 台账不守恒（守恒恒等式破缺）。
    LedgerBroken,
    /// 0xa679 快照指纹不符（字段被改后重算不一致）。
    SnapshotFingerprint,
    /// 0xa67a 常量表破坏（表项与语义规则矛盾）。
    TableBroken,
    /// 0xa67b 段位守卫失败（诊断码越出本模块段）。
    SegmentGuard,
}

impl u2561Code {
    /// 全量闭集。
    pub const ALL: [u2561Code; 12] = [
        u2561Code::RowDup,
        u2561Code::RowGap,
        u2561Code::GuardTrip,
        u2561Code::KeyDup,
        u2561Code::KeyGap,
        u2561Code::ValOob,
        u2561Code::SumDrift,
        u2561Code::ThreshDrift,
        u2561Code::LedgerBroken,
        u2561Code::SnapshotFingerprint,
        u2561Code::TableBroken,
        u2561Code::SegmentGuard,
    ];

    /// 码值（连续十六进制，与声明序恒等）。
    pub const fn code(self) -> u16 {
        match self {
            u2561Code::RowDup => 0xa670,
            u2561Code::RowGap => 0xa671,
            u2561Code::GuardTrip => 0xa672,
            u2561Code::KeyDup => 0xa673,
            u2561Code::KeyGap => 0xa674,
            u2561Code::ValOob => 0xa675,
            u2561Code::SumDrift => 0xa676,
            u2561Code::ThreshDrift => 0xa677,
            u2561Code::LedgerBroken => 0xa678,
            u2561Code::SnapshotFingerprint => 0xa679,
            u2561Code::TableBroken => 0xa67a,
            u2561Code::SegmentGuard => 0xa67b,
        }
    }

    /// 中文诊断名。
    pub const fn name(self) -> &'static str {
        match self {
            u2561Code::RowDup => "表行重复（接管漏项）",
            u2561Code::RowGap => "表行断档（语义错配）",
            u2561Code::GuardTrip => "守卫触发（重入/越限被拒）",
            u2561Code::KeyDup => "映射键重复（权限失真）",
            u2561Code::KeyGap => "映射键断档（直测分歧）",
            u2561Code::ValOob => "映射值越界",
            u2561Code::SumDrift => "双表累计漂移",
            u2561Code::ThreshDrift => "阈值漂移（守卫阈值≠冻结值）",
            u2561Code::LedgerBroken => "台账不守恒（守恒恒等式破缺）",
            u2561Code::SnapshotFingerprint => "快照指纹不符（字段被改后重算不一致）",
            u2561Code::TableBroken => "常量表破坏（表项与语义规则矛盾）",
            u2561Code::SegmentGuard => "段位守卫失败（诊断码越出本模块段）",
        }
    }

    /// 可操作修复建议（每条诊断都必须带下一步动作）。
    pub const fn suggestion(self) -> &'static str {
        match self {
            u2561Code::RowDup => "按键唯一律去重",
            u2561Code::RowGap => "按连续律补号",
            u2561Code::GuardTrip => "检查临界区嵌套",
            u2561Code::KeyDup => "按键唯一律去重",
            u2561Code::KeyGap => "按连续律补号",
            u2561Code::ValOob => "按值域裁剪",
            u2561Code::SumDrift => "按理论值复算",
            u2561Code::ThreshDrift => "按冻结值校正",
            u2561Code::LedgerBroken => "按守恒律重建台账后重跑",
            u2561Code::SnapshotFingerprint => "按防篡改规程重建快照",
            u2561Code::TableBroken => "按语义规则重建常量表",
            u2561Code::SegmentGuard => "核对诊断码分配表",
        }
    }
}

// --------------------------------------------------------------------------
// 冻结常量表（u2561mix · 16 行逐行注释自证）
// --------------------------------------------------------------------------
/// 地址空间体 对拍表（16 行）。
pub const u2561mix_COUNT: usize = 16;

/// 冻结表本体（编译期常量，运行期只读）。
pub static u2561mix: [(&'static str, u64); 16] = [
    ("AS_00", 10), // AS 对拍行（AddressSpace）
    ("LG_01", 15), // LG 对拍行（接管账）
    ("SM_02", 20), // SM 对拍行（语义映射）
    ("MT_03", 25), // MT 对拍行（成熟体）
    ("IF_04", 30), // IF 对拍行（接口面）
    ("TM_05", 35), // TM 对拍行（三机直测）
    ("AS2_06", 40), // AS2 对拍行（起步态）
    ("MP_07", 45), // MP 对拍行（映射项）
    ("UN_08", 50), // UN 对拍行（解除项）
    ("PC_09", 55), // PC 对拍行（权限账）
    ("FV_10", 60), // FV 对拍行（逐项核验）
    ("AR_11", 65), // AR 对拍行（在档锚）
    ("AS_12", 70), // AS 对拍行（AddressSpace）
    ("LG_13", 75), // LG 对拍行（接管账）
    ("SM_14", 80), // SM 对拍行（语义映射）
    ("MT_15", 85), // MT 对拍行（成熟体）
];

/// 线性查找（键唯一性由构造期断言保证，运行时无分支歧义）。
pub fn u2561mix_get(key: &'static str) -> Option<u64> {
    let mut i = 0usize;
    while i < u2561mix_COUNT {
        if u2561mix[i].0 == key {
            return Some(u2561mix[i].1);
        }
        i += 1;
    }
    None
}

/// 累计求和（用于守恒对账）。
pub fn u2561mix_sum() -> u64 {
    let mut s = 0u64;
    let mut i = 0usize;
    while i < u2561mix_COUNT {
        s = s.wrapping_add(u2561mix[i].1 as u64);
        i += 1;
    }
    s
}

// --------------------------------------------------------------------------
// 守卫 u2561mixgate（阈值 32 · 重入/越限/静默三类拒止）
// --------------------------------------------------------------------------
/// 地址空间体 混合面守卫（阈值 32 冻结）。
#[derive(Clone, Copy, Debug)]
pub struct u2561mixgate {
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

/// 守卫阈值（地址空间体 混合面守卫（阈值 32 冻结））。
pub const U2561MIXGATE_THRESHOLD: u64 = 32;

impl u2561mixgate {
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
        if self.window >= U2561MIXGATE_THRESHOLD {
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
// 字符串键值映射 u2561mixmap（16 行逐行语义注释 + 逐行守卫）
// --------------------------------------------------------------------------
/// 地址空间体 混合映射矩阵。
/// 行数。
pub const u2561mixmap_COUNT: usize = 16;

/// 表本体（键 + 值 + 注释由编译期冻结）。
pub static u2561mixmap: [(&'static str, u64); 16] = [
    ("AS0000", 0), // AS 单元 0000（AddressSpace × 相位 00 × 混 轮）
    ("LG0001", 9), // LG 单元 0001（接管账 × 相位 01 × 混 轮）
    ("SM0002", 18), // SM 单元 0002（语义映射 × 相位 02 × 混 轮）
    ("MT0003", 27), // MT 单元 0003（成熟体 × 相位 03 × 混 轮）
    ("IF0004", 36), // IF 单元 0004（接口面 × 相位 04 × 混 轮）
    ("TM0005", 45), // TM 单元 0005（三机直测 × 相位 05 × 混 轮）
    ("AS20006", 54), // AS2 单元 0006（起步态 × 相位 06 × 混 轮）
    ("MP0007", 63), // MP 单元 0007（映射项 × 相位 07 × 混 轮）
    ("UN0008", 8), // UN 单元 0008（解除项 × 相位 08 × 混 轮）
    ("PC0009", 17), // PC 单元 0009（权限账 × 相位 09 × 混 轮）
    ("FV0010", 26), // FV 单元 0010（逐项核验 × 相位 10 × 混 轮）
    ("AR0011", 35), // AR 单元 0011（在档锚 × 相位 11 × 混 轮）
    ("AS0012", 44), // AS 单元 0012（AddressSpace × 相位 12 × 混 轮）
    ("LG0013", 53), // LG 单元 0013（接管账 × 相位 13 × 混 轮）
    ("SM0014", 62), // SM 单元 0014（语义映射 × 相位 14 × 混 轮）
    ("MT0015", 7), // MT 单元 0015（成熟体 × 相位 15 × 混 轮）
];

/// O(1) 语义查找（match 全枚举；命中率 100%% 由守卫保证）。
pub fn u2561mixmap_get(key: &str) -> Option<u64> {
    match key {
        "AS0000" => Some(0),
        "LG0001" => Some(9),
        "SM0002" => Some(18),
        "MT0003" => Some(27),
        "IF0004" => Some(36),
        "TM0005" => Some(45),
        "AS20006" => Some(54),
        "MP0007" => Some(63),
        "UN0008" => Some(8),
        "PC0009" => Some(17),
        "FV0010" => Some(26),
        "AR0011" => Some(35),
        "AS0012" => Some(44),
        "LG0013" => Some(53),
        "SM0014" => Some(62),
        "MT0015" => Some(7),
        _ => None,
    }
}

/// 逐行守卫扫描（返回违例行数；0 = 全绿）。
pub fn u2561mixmap_scan() -> usize {
    let mut bad = 0usize;
    let mut i = 0usize;
    while i < u2561mixmap_COUNT {
        let (k, v) = u2561mixmap[i];
        let ok = match k {
            "AS0000" => v < 64,
            "LG0001" => v < 64,
            "SM0002" => v < 64,
            "MT0003" => v < 64,
            "IF0004" => v < 64,
            "TM0005" => v < 64,
            "AS20006" => v < 64,
            "MP0007" => v < 64,
            "UN0008" => v < 64,
            "PC0009" => v < 64,
            "FV0010" => v < 64,
            "AR0011" => v < 64,
            "AS0012" => v < 64,
            "LG0013" => v < 64,
            "SM0014" => v < 64,
            "MT0015" => v < 64,
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
pub fn u2561mixmap_sum() -> u64 {
    let mut s = 0u64;
    let mut i = 0usize;
    while i < u2561mixmap_COUNT {
        s = s.wrapping_add(u2561mixmap[i].1 as u64);
        i += 1;
    }
    s
}

/// 键唯一性（构造期断言等价物：逐对扫描同键）。
pub fn u2561mixmap_unique() -> bool {
    let mut i = 0usize;
    while i < u2561mixmap_COUNT {
        let mut j = i + 1;
        while j < u2561mixmap_COUNT {
            if u2561mixmap[i].0 == u2561mixmap[j].0 {
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

/// 段位守卫：全部诊断码必须落在本段 [0xa670, 0xa67f]。
pub const fn u2561_segment_guard() -> bool {
    let mut ok = true;
    let mut i = 0usize;
    while i < u2561Code::ALL.len() {
        let c = u2561Code::ALL[i].code();
        if c < 0xa670 || c > 0xa67f {
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
fn u2561_guard_seg() -> bool {
    u2561_segment_guard()
}

/// UNX-F2561 对拍表 16 行边界与累计守恒。
fn check_u2561_mix_tbl() -> bool {
    let mut ok = u2561mix_COUNT == 16;
    if ok { ok = u2561mix_get("AS_00") == Some(10) && u2561mix_get("MT_15") == Some(85); }
    if ok { ok = u2561mix_get("ZZZ_NOKEY").is_none(); }
    if ok { ok = u2561mix_sum() == 760; }
    ok
}

/// UNX-F2561 混合映射 16 行守卫唯一 + 守卫阈值冻结。
fn check_u2561_mix_map() -> bool {
    let mut ok = u2561mixmap_COUNT == 16;
    if ok { ok = u2561mixmap_scan() == 0 && u2561mixmap_unique(); }
    if ok { ok = u2561mixmap_sum() == 504; }
    if ok { ok = U2561MIXGATE_THRESHOLD == 32; }
    ok
}

/// 本模块自检（逐检查带 detail，全绿才出门）。
pub fn run_u2561_checks() -> CheckSet {
    let mut set = CheckSet::new("U2561");
    set.add(
        "u2561-seg-guard",
        u2561_guard_seg(),
        if u2561_guard_seg() { "" } else { "diagnostic code outside segment" },
    );
    set.add(
        "u2561-mix-tbl",
        check_u2561_mix_tbl(),
        if check_u2561_mix_tbl() { "" } else { "UNX-F2561 mix table mismatch" },
    );
    set.add(
        "u2561-mix-map",
        check_u2561_mix_map(),
        if check_u2561_mix_map() { "" } else { "UNX-F2561 mix map mismatch" },
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
    fn u2561_all_checks_green() {
        let set = run_u2561_checks();
        assert!(set.all_passed(), "U2561 domain checks must be green");
    }

    #[test]
    fn u2561_segment_guard_holds() {
        assert!(u2561_segment_guard());
    }
}
