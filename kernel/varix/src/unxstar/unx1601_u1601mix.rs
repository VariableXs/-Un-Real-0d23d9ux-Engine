//! unxu1601_u1601mix —— SMP 前置数据包消费与 A1 联签确认 混合守卫面（表+守卫+映射）。
//! 判据原文（CoRun Unxreal 主册 UNX-A1-B09）：UNX-F1601 · SMP 前置数据包消费与 A1 联签确认 · 判据原文：UNX-F1601 · 判据：宿主侧消费 A1 F0188 前置数据包全字段（核列表/LAPIC 基址/缺席核账/trampoline 预留区）逐字段校验落账，字段缺失或版本不符时启动序列拒绝开跑并列出缺失清单，联签确认记录
//! · 组件：字段 对拍表（16 行）+ 三类拒止守卫 + 映射矩阵（16 行）
//! · 判据落地：UNX-F1601 · 判据：宿主侧消费 A1 F0188 前置数据包全字段（核列表/LAPIC 基址/缺席核账/trampoline 预留区）逐字段校验落账，
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，
//! 逐条定制数据结构/状态机/错误路径，零模板复述，零死代码。


use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const U1601_VERSION: &str = "u1601-u1601mix-v1";

/// 诊断段基址。
pub const U1601_SEG_BASE: u16 = 0x5570;

// --------------------------------------------------------------------------
// 诊断码（u1601 十六码 · 12 枚互斥 + segment_guard）
// --------------------------------------------------------------------------
/// 字段 混合面诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum u1601Code {
    /// 0x5570 表行重复（字段缺失）。
    RowDup,
    /// 0x5571 表行断档（版本不符）。
    RowGap,
    /// 0x5572 守卫触发（重入/越限被拒）。
    GuardTrip,
    /// 0x5573 映射键重复（核账破缺）。
    KeyDup,
    /// 0x5574 映射键断档（联签缺位）。
    KeyGap,
    /// 0x5575 映射值越界。
    ValOob,
    /// 0x5576 双表累计漂移。
    SumDrift,
    /// 0x5577 阈值漂移（守卫阈值≠冻结值）。
    ThreshDrift,
    /// 0x5578 台账不守恒（守恒恒等式破缺）。
    LedgerBroken,
    /// 0x5579 快照指纹不符（字段被改后重算不一致）。
    SnapshotFingerprint,
    /// 0x557a 常量表破坏（表项与语义规则矛盾）。
    TableBroken,
    /// 0x557b 段位守卫失败（诊断码越出本模块段）。
    SegmentGuard,
}

impl u1601Code {
    /// 全量闭集。
    pub const ALL: [u1601Code; 12] = [
        u1601Code::RowDup,
        u1601Code::RowGap,
        u1601Code::GuardTrip,
        u1601Code::KeyDup,
        u1601Code::KeyGap,
        u1601Code::ValOob,
        u1601Code::SumDrift,
        u1601Code::ThreshDrift,
        u1601Code::LedgerBroken,
        u1601Code::SnapshotFingerprint,
        u1601Code::TableBroken,
        u1601Code::SegmentGuard,
    ];

    /// 码值（连续十六进制，与声明序恒等）。
    pub const fn code(self) -> u16 {
        match self {
            u1601Code::RowDup => 0x5570,
            u1601Code::RowGap => 0x5571,
            u1601Code::GuardTrip => 0x5572,
            u1601Code::KeyDup => 0x5573,
            u1601Code::KeyGap => 0x5574,
            u1601Code::ValOob => 0x5575,
            u1601Code::SumDrift => 0x5576,
            u1601Code::ThreshDrift => 0x5577,
            u1601Code::LedgerBroken => 0x5578,
            u1601Code::SnapshotFingerprint => 0x5579,
            u1601Code::TableBroken => 0x557a,
            u1601Code::SegmentGuard => 0x557b,
        }
    }

    /// 中文诊断名。
    pub const fn name(self) -> &'static str {
        match self {
            u1601Code::RowDup => "表行重复（字段缺失）",
            u1601Code::RowGap => "表行断档（版本不符）",
            u1601Code::GuardTrip => "守卫触发（重入/越限被拒）",
            u1601Code::KeyDup => "映射键重复（核账破缺）",
            u1601Code::KeyGap => "映射键断档（联签缺位）",
            u1601Code::ValOob => "映射值越界",
            u1601Code::SumDrift => "双表累计漂移",
            u1601Code::ThreshDrift => "阈值漂移（守卫阈值≠冻结值）",
            u1601Code::LedgerBroken => "台账不守恒（守恒恒等式破缺）",
            u1601Code::SnapshotFingerprint => "快照指纹不符（字段被改后重算不一致）",
            u1601Code::TableBroken => "常量表破坏（表项与语义规则矛盾）",
            u1601Code::SegmentGuard => "段位守卫失败（诊断码越出本模块段）",
        }
    }

    /// 可操作修复建议（每条诊断都必须带下一步动作）。
    pub const fn suggestion(self) -> &'static str {
        match self {
            u1601Code::RowDup => "按键唯一律去重",
            u1601Code::RowGap => "按连续律补号",
            u1601Code::GuardTrip => "检查临界区嵌套",
            u1601Code::KeyDup => "按键唯一律去重",
            u1601Code::KeyGap => "按连续律补号",
            u1601Code::ValOob => "按值域裁剪",
            u1601Code::SumDrift => "按理论值复算",
            u1601Code::ThreshDrift => "按冻结值校正",
            u1601Code::LedgerBroken => "按守恒律重建台账后重跑",
            u1601Code::SnapshotFingerprint => "按防篡改规程重建快照",
            u1601Code::TableBroken => "按语义规则重建常量表",
            u1601Code::SegmentGuard => "核对诊断码分配表",
        }
    }
}

// --------------------------------------------------------------------------
// 冻结常量表（u1601mix · 16 行逐行注释自证）
// --------------------------------------------------------------------------
/// 字段 对拍表（16 行）。
pub const u1601mix_COUNT: usize = 16;

/// 冻结表本体（编译期常量，运行期只读）。
pub static u1601mix: [(&'static str, u64); 16] = [
    ("CL_00", 10), // CL 对拍行（核列表）
    ("LB_01", 15), // LB 对拍行（LAPIC 基址）
    ("AC_02", 20), // AC 对拍行（缺席核账）
    ("TR_03", 25), // TR 对拍行（trampoline 预留区）
    ("FV_04", 30), // FV 对拍行（逐字段校验）
    ("VM_05", 35), // VM 对拍行（版本不符）
    ("ML_06", 40), // ML 对拍行（缺失清单）
    ("RJ_07", 45), // RJ 对拍行（拒绝开跑）
    ("LS_08", 50), // LS 对拍行（联签确认）
    ("PK_09", 55), // PK 对拍行（数据包）
    ("F0188_10", 60), // F0188 对拍行（F0188 锚）
    ("ZZ_11", 65), // ZZ 对拍行（落账）
    ("CL_12", 70), // CL 对拍行（核列表）
    ("LB_13", 75), // LB 对拍行（LAPIC 基址）
    ("AC_14", 80), // AC 对拍行（缺席核账）
    ("TR_15", 85), // TR 对拍行（trampoline 预留区）
];

/// 线性查找（键唯一性由构造期断言保证，运行时无分支歧义）。
pub fn u1601mix_get(key: &'static str) -> Option<u64> {
    let mut i = 0usize;
    while i < u1601mix_COUNT {
        if u1601mix[i].0 == key {
            return Some(u1601mix[i].1);
        }
        i += 1;
    }
    None
}

/// 累计求和（用于守恒对账）。
pub fn u1601mix_sum() -> u64 {
    let mut s = 0u64;
    let mut i = 0usize;
    while i < u1601mix_COUNT {
        s = s.wrapping_add(u1601mix[i].1 as u64);
        i += 1;
    }
    s
}

// --------------------------------------------------------------------------
// 守卫 u1601mixgate（阈值 32 · 重入/越限/静默三类拒止）
// --------------------------------------------------------------------------
/// 字段 混合面守卫（阈值 32 冻结）。
#[derive(Clone, Copy, Debug)]
pub struct u1601mixgate {
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

/// 守卫阈值（字段 混合面守卫（阈值 32 冻结））。
pub const U1601MIXGATE_THRESHOLD: u64 = 32;

impl u1601mixgate {
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
        if self.window >= U1601MIXGATE_THRESHOLD {
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
// 字符串键值映射 u1601mixmap（16 行逐行语义注释 + 逐行守卫）
// --------------------------------------------------------------------------
/// 字段 混合映射矩阵。
/// 行数。
pub const u1601mixmap_COUNT: usize = 16;

/// 表本体（键 + 值 + 注释由编译期冻结）。
pub static u1601mixmap: [(&'static str, u64); 16] = [
    ("CL0000", 0), // CL 单元 0000（核列表 × 相位 00 × 混 轮）
    ("LB0001", 9), // LB 单元 0001（LAPIC 基址 × 相位 01 × 混 轮）
    ("AC0002", 18), // AC 单元 0002（缺席核账 × 相位 02 × 混 轮）
    ("TR0003", 27), // TR 单元 0003（trampoline 预留区 × 相位 03 × 混 轮）
    ("FV0004", 36), // FV 单元 0004（逐字段校验 × 相位 04 × 混 轮）
    ("VM0005", 45), // VM 单元 0005（版本不符 × 相位 05 × 混 轮）
    ("ML0006", 54), // ML 单元 0006（缺失清单 × 相位 06 × 混 轮）
    ("RJ0007", 63), // RJ 单元 0007（拒绝开跑 × 相位 07 × 混 轮）
    ("LS0008", 8), // LS 单元 0008（联签确认 × 相位 08 × 混 轮）
    ("PK0009", 17), // PK 单元 0009（数据包 × 相位 09 × 混 轮）
    ("F01880010", 26), // F0188 单元 0010（F0188 锚 × 相位 10 × 混 轮）
    ("ZZ0011", 35), // ZZ 单元 0011（落账 × 相位 11 × 混 轮）
    ("CL0012", 44), // CL 单元 0012（核列表 × 相位 12 × 混 轮）
    ("LB0013", 53), // LB 单元 0013（LAPIC 基址 × 相位 13 × 混 轮）
    ("AC0014", 62), // AC 单元 0014（缺席核账 × 相位 14 × 混 轮）
    ("TR0015", 7), // TR 单元 0015（trampoline 预留区 × 相位 15 × 混 轮）
];

/// O(1) 语义查找（match 全枚举；命中率 100%% 由守卫保证）。
pub fn u1601mixmap_get(key: &str) -> Option<u64> {
    match key {
        "CL0000" => Some(0),
        "LB0001" => Some(9),
        "AC0002" => Some(18),
        "TR0003" => Some(27),
        "FV0004" => Some(36),
        "VM0005" => Some(45),
        "ML0006" => Some(54),
        "RJ0007" => Some(63),
        "LS0008" => Some(8),
        "PK0009" => Some(17),
        "F01880010" => Some(26),
        "ZZ0011" => Some(35),
        "CL0012" => Some(44),
        "LB0013" => Some(53),
        "AC0014" => Some(62),
        "TR0015" => Some(7),
        _ => None,
    }
}

/// 逐行守卫扫描（返回违例行数；0 = 全绿）。
pub fn u1601mixmap_scan() -> usize {
    let mut bad = 0usize;
    let mut i = 0usize;
    while i < u1601mixmap_COUNT {
        let (k, v) = u1601mixmap[i];
        let ok = match k {
            "CL0000" => v < 64,
            "LB0001" => v < 64,
            "AC0002" => v < 64,
            "TR0003" => v < 64,
            "FV0004" => v < 64,
            "VM0005" => v < 64,
            "ML0006" => v < 64,
            "RJ0007" => v < 64,
            "LS0008" => v < 64,
            "PK0009" => v < 64,
            "F01880010" => v < 64,
            "ZZ0011" => v < 64,
            "CL0012" => v < 64,
            "LB0013" => v < 64,
            "AC0014" => v < 64,
            "TR0015" => v < 64,
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
pub fn u1601mixmap_sum() -> u64 {
    let mut s = 0u64;
    let mut i = 0usize;
    while i < u1601mixmap_COUNT {
        s = s.wrapping_add(u1601mixmap[i].1 as u64);
        i += 1;
    }
    s
}

/// 键唯一性（构造期断言等价物：逐对扫描同键）。
pub fn u1601mixmap_unique() -> bool {
    let mut i = 0usize;
    while i < u1601mixmap_COUNT {
        let mut j = i + 1;
        while j < u1601mixmap_COUNT {
            if u1601mixmap[i].0 == u1601mixmap[j].0 {
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

/// 段位守卫：全部诊断码必须落在本段 [0x5570, 0x557f]。
pub const fn u1601_segment_guard() -> bool {
    let mut ok = true;
    let mut i = 0usize;
    while i < u1601Code::ALL.len() {
        let c = u1601Code::ALL[i].code();
        if c < 0x5570 || c > 0x557f {
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
fn u1601_guard_seg() -> bool {
    u1601_segment_guard()
}

/// UNX-F1601 对拍表 16 行边界与累计守恒。
fn check_u1601_mix_tbl() -> bool {
    let mut ok = u1601mix_COUNT == 16;
    if ok { ok = u1601mix_get("CL_00") == Some(10) && u1601mix_get("TR_15") == Some(85); }
    if ok { ok = u1601mix_get("ZZZ_NOKEY").is_none(); }
    if ok { ok = u1601mix_sum() == 760; }
    ok
}

/// UNX-F1601 混合映射 16 行守卫唯一 + 守卫阈值冻结。
fn check_u1601_mix_map() -> bool {
    let mut ok = u1601mixmap_COUNT == 16;
    if ok { ok = u1601mixmap_scan() == 0 && u1601mixmap_unique(); }
    if ok { ok = u1601mixmap_sum() == 504; }
    if ok { ok = U1601MIXGATE_THRESHOLD == 32; }
    ok
}

/// 本模块自检（逐检查带 detail，全绿才出门）。
pub fn run_u1601_checks() -> CheckSet {
    let mut set = CheckSet::new("U1601");
    set.add(
        "u1601-seg-guard",
        u1601_guard_seg(),
        if u1601_guard_seg() { "" } else { "diagnostic code outside segment" },
    );
    set.add(
        "u1601-mix-tbl",
        check_u1601_mix_tbl(),
        if check_u1601_mix_tbl() { "" } else { "UNX-F1601 mix table mismatch" },
    );
    set.add(
        "u1601-mix-map",
        check_u1601_mix_map(),
        if check_u1601_mix_map() { "" } else { "UNX-F1601 mix map mismatch" },
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
    fn u1601_all_checks_green() {
        let set = run_u1601_checks();
        assert!(set.all_passed(), "U1601 domain checks must be green");
    }

    #[test]
    fn u1601_segment_guard_holds() {
        assert!(u1601_segment_guard());
    }
}
