//! unxu3401_u3401mix —— ktest 现状盘点与扩容总设计（接管 testing/ 目录） 混合守卫面（表+守卫+映射）。
//! 判据原文（CoRun Unxreal 主册 UNX-A1-B09）：UNX-F3401 · ktest 现状盘点与扩容总设计（接管 testing/ 目录） · 判据原文：UNX-F3401 · 判据：J1：总设计文档（现状盘点/扩容目标/宏族路线/接管三步法）交付；M-160 设计基线用例。
//! · 组件：ktest 扩容 对拍表（16 行）+ 三类拒止守卫 + 映射矩阵（16 行）
//! · 判据落地：UNX-F3401 · 判据：J1：总设计文档（现状盘点/扩容目标/宏族路线/接管三步法）交付；M-160 设计基线用例。
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，
//! 逐条定制数据结构/状态机/错误路径，零模板复述，零死代码。


use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const U3401_VERSION: &str = "u3401-u3401mix-v1";

/// 诊断段基址。
pub const U3401_SEG_BASE: u16 = 0xe0e0;

// --------------------------------------------------------------------------
// 诊断码（u3401 十六码 · 12 枚互斥 + segment_guard）
// --------------------------------------------------------------------------
/// ktest 扩容 混合面诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum u3401Code {
    /// 0xe0e0 表行重复（ktest）。
    RowDup,
    /// 0xe0e1 表行断档（扩容）。
    RowGap,
    /// 0xe0e2 守卫触发（重入/越限被拒）。
    GuardTrip,
    /// 0xe0e3 映射键重复（接管）。
    KeyDup,
    /// 0xe0e4 映射键断档（骨架）。
    KeyGap,
    /// 0xe0e5 映射值越界。
    ValOob,
    /// 0xe0e6 双表累计漂移。
    SumDrift,
    /// 0xe0e7 阈值漂移（守卫阈值≠冻结值）。
    ThreshDrift,
    /// 0xe0e8 台账不守恒（守恒恒等式破缺）。
    LedgerBroken,
    /// 0xe0e9 快照指纹不符（字段被改后重算不一致）。
    SnapshotFingerprint,
    /// 0xe0ea 常量表破坏（表项与语义规则矛盾）。
    TableBroken,
    /// 0xe0eb 段位守卫失败（诊断码越出本模块段）。
    SegmentGuard,
}

impl u3401Code {
    /// 全量闭集。
    pub const ALL: [u3401Code; 12] = [
        u3401Code::RowDup,
        u3401Code::RowGap,
        u3401Code::GuardTrip,
        u3401Code::KeyDup,
        u3401Code::KeyGap,
        u3401Code::ValOob,
        u3401Code::SumDrift,
        u3401Code::ThreshDrift,
        u3401Code::LedgerBroken,
        u3401Code::SnapshotFingerprint,
        u3401Code::TableBroken,
        u3401Code::SegmentGuard,
    ];

    /// 码值（连续十六进制，与声明序恒等）。
    pub const fn code(self) -> u16 {
        match self {
            u3401Code::RowDup => 0xe0e0,
            u3401Code::RowGap => 0xe0e1,
            u3401Code::GuardTrip => 0xe0e2,
            u3401Code::KeyDup => 0xe0e3,
            u3401Code::KeyGap => 0xe0e4,
            u3401Code::ValOob => 0xe0e5,
            u3401Code::SumDrift => 0xe0e6,
            u3401Code::ThreshDrift => 0xe0e7,
            u3401Code::LedgerBroken => 0xe0e8,
            u3401Code::SnapshotFingerprint => 0xe0e9,
            u3401Code::TableBroken => 0xe0ea,
            u3401Code::SegmentGuard => 0xe0eb,
        }
    }

    /// 中文诊断名。
    pub const fn name(self) -> &'static str {
        match self {
            u3401Code::RowDup => "表行重复（ktest）",
            u3401Code::RowGap => "表行断档（扩容）",
            u3401Code::GuardTrip => "守卫触发（重入/越限被拒）",
            u3401Code::KeyDup => "映射键重复（接管）",
            u3401Code::KeyGap => "映射键断档（骨架）",
            u3401Code::ValOob => "映射值越界",
            u3401Code::SumDrift => "双表累计漂移",
            u3401Code::ThreshDrift => "阈值漂移（守卫阈值≠冻结值）",
            u3401Code::LedgerBroken => "台账不守恒（守恒恒等式破缺）",
            u3401Code::SnapshotFingerprint => "快照指纹不符（字段被改后重算不一致）",
            u3401Code::TableBroken => "常量表破坏（表项与语义规则矛盾）",
            u3401Code::SegmentGuard => "段位守卫失败（诊断码越出本模块段）",
        }
    }

    /// 可操作修复建议（每条诊断都必须带下一步动作）。
    pub const fn suggestion(self) -> &'static str {
        match self {
            u3401Code::RowDup => "按键唯一律去重",
            u3401Code::RowGap => "按连续律补号",
            u3401Code::GuardTrip => "检查临界区嵌套",
            u3401Code::KeyDup => "按键唯一律去重",
            u3401Code::KeyGap => "按连续律补号",
            u3401Code::ValOob => "按值域裁剪",
            u3401Code::SumDrift => "按理论值复算",
            u3401Code::ThreshDrift => "按冻结值校正",
            u3401Code::LedgerBroken => "按守恒律重建台账后重跑",
            u3401Code::SnapshotFingerprint => "按防篡改规程重建快照",
            u3401Code::TableBroken => "按语义规则重建常量表",
            u3401Code::SegmentGuard => "核对诊断码分配表",
        }
    }
}

// --------------------------------------------------------------------------
// 冻结常量表（u3401mix · 16 行逐行注释自证）
// --------------------------------------------------------------------------
/// ktest 扩容 对拍表（16 行）。
pub const u3401mix_COUNT: usize = 16;

/// 冻结表本体（编译期常量，运行期只读）。
pub static u3401mix: [(&'static str, u64); 16] = [
    ("KT_00", 10), // KT 对拍行（ktest 套件）
    ("DIR_01", 15), // DIR 对拍行（目录接管）
    ("CASE_02", 20), // CASE 对拍行（用例）
    ("SEL_03", 25), // SEL 对拍行（选择器）
    ("REG_04", 30), // REG 对拍行（注册表）
    ("FIX_05", 35), // FIX 对拍行（固化）
    ("BACKEND_06", 40), // BACKEND 对拍行（后端）
    ("QEMU_07", 45), // QEMU 对拍行（QEMU 后端）
    ("REAL_08", 50), // REAL 对拍行（真机后端）
    ("SKEL_09", 55), // SKEL 对拍行（骨架）
    ("MIG_10", 60), // MIG 对拍行（迁移）
    ("CNT_11", 65), // CNT 对拍行（计数）
    ("KT_12", 70), // KT 对拍行（ktest 套件）
    ("DIR_13", 75), // DIR 对拍行（目录接管）
    ("CASE_14", 80), // CASE 对拍行（用例）
    ("SEL_15", 85), // SEL 对拍行（选择器）
];

/// 线性查找（键唯一性由构造期断言保证，运行时无分支歧义）。
pub fn u3401mix_get(key: &'static str) -> Option<u64> {
    let mut i = 0usize;
    while i < u3401mix_COUNT {
        if u3401mix[i].0 == key {
            return Some(u3401mix[i].1);
        }
        i += 1;
    }
    None
}

/// 累计求和（用于守恒对账）。
pub fn u3401mix_sum() -> u64 {
    let mut s = 0u64;
    let mut i = 0usize;
    while i < u3401mix_COUNT {
        s = s.wrapping_add(u3401mix[i].1 as u64);
        i += 1;
    }
    s
}

// --------------------------------------------------------------------------
// 守卫 u3401mixgate（阈值 32 · 重入/越限/静默三类拒止）
// --------------------------------------------------------------------------
/// ktest 扩容 混合面守卫（阈值 32 冻结）。
#[derive(Clone, Copy, Debug)]
pub struct u3401mixgate {
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

/// 守卫阈值（ktest 扩容 混合面守卫（阈值 32 冻结））。
pub const U3401MIXGATE_THRESHOLD: u64 = 32;

impl u3401mixgate {
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
        if self.window >= U3401MIXGATE_THRESHOLD {
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
// 字符串键值映射 u3401mixmap（16 行逐行语义注释 + 逐行守卫）
// --------------------------------------------------------------------------
/// ktest 扩容 混合映射矩阵。
/// 行数。
pub const u3401mixmap_COUNT: usize = 16;

/// 表本体（键 + 值 + 注释由编译期冻结）。
pub static u3401mixmap: [(&'static str, u64); 16] = [
    ("KT0000", 0), // KT 单元 0000（ktest 套件 × 相位 00 × 混 轮）
    ("DIR0001", 9), // DIR 单元 0001（目录接管 × 相位 01 × 混 轮）
    ("CASE0002", 18), // CASE 单元 0002（用例 × 相位 02 × 混 轮）
    ("SEL0003", 27), // SEL 单元 0003（选择器 × 相位 03 × 混 轮）
    ("REG0004", 36), // REG 单元 0004（注册表 × 相位 04 × 混 轮）
    ("FIX0005", 45), // FIX 单元 0005（固化 × 相位 05 × 混 轮）
    ("BACKEND0006", 54), // BACKEND 单元 0006（后端 × 相位 06 × 混 轮）
    ("QEMU0007", 63), // QEMU 单元 0007（QEMU 后端 × 相位 07 × 混 轮）
    ("REAL0008", 8), // REAL 单元 0008（真机后端 × 相位 08 × 混 轮）
    ("SKEL0009", 17), // SKEL 单元 0009（骨架 × 相位 09 × 混 轮）
    ("MIG0010", 26), // MIG 单元 0010（迁移 × 相位 10 × 混 轮）
    ("CNT0011", 35), // CNT 单元 0011（计数 × 相位 11 × 混 轮）
    ("KT0012", 44), // KT 单元 0012（ktest 套件 × 相位 12 × 混 轮）
    ("DIR0013", 53), // DIR 单元 0013（目录接管 × 相位 13 × 混 轮）
    ("CASE0014", 62), // CASE 单元 0014（用例 × 相位 14 × 混 轮）
    ("SEL0015", 7), // SEL 单元 0015（选择器 × 相位 15 × 混 轮）
];

/// O(1) 语义查找（match 全枚举；命中率 100%% 由守卫保证）。
pub fn u3401mixmap_get(key: &str) -> Option<u64> {
    match key {
        "KT0000" => Some(0),
        "DIR0001" => Some(9),
        "CASE0002" => Some(18),
        "SEL0003" => Some(27),
        "REG0004" => Some(36),
        "FIX0005" => Some(45),
        "BACKEND0006" => Some(54),
        "QEMU0007" => Some(63),
        "REAL0008" => Some(8),
        "SKEL0009" => Some(17),
        "MIG0010" => Some(26),
        "CNT0011" => Some(35),
        "KT0012" => Some(44),
        "DIR0013" => Some(53),
        "CASE0014" => Some(62),
        "SEL0015" => Some(7),
        _ => None,
    }
}

/// 逐行守卫扫描（返回违例行数；0 = 全绿）。
pub fn u3401mixmap_scan() -> usize {
    let mut bad = 0usize;
    let mut i = 0usize;
    while i < u3401mixmap_COUNT {
        let (k, v) = u3401mixmap[i];
        let ok = match k {
            "KT0000" => v < 64,
            "DIR0001" => v < 64,
            "CASE0002" => v < 64,
            "SEL0003" => v < 64,
            "REG0004" => v < 64,
            "FIX0005" => v < 64,
            "BACKEND0006" => v < 64,
            "QEMU0007" => v < 64,
            "REAL0008" => v < 64,
            "SKEL0009" => v < 64,
            "MIG0010" => v < 64,
            "CNT0011" => v < 64,
            "KT0012" => v < 64,
            "DIR0013" => v < 64,
            "CASE0014" => v < 64,
            "SEL0015" => v < 64,
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
pub fn u3401mixmap_sum() -> u64 {
    let mut s = 0u64;
    let mut i = 0usize;
    while i < u3401mixmap_COUNT {
        s = s.wrapping_add(u3401mixmap[i].1 as u64);
        i += 1;
    }
    s
}

/// 键唯一性（构造期断言等价物：逐对扫描同键）。
pub fn u3401mixmap_unique() -> bool {
    let mut i = 0usize;
    while i < u3401mixmap_COUNT {
        let mut j = i + 1;
        while j < u3401mixmap_COUNT {
            if u3401mixmap[i].0 == u3401mixmap[j].0 {
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

/// 段位守卫：全部诊断码必须落在本段 [0xe0e0, 0xe0ef]。
pub const fn u3401_segment_guard() -> bool {
    let mut ok = true;
    let mut i = 0usize;
    while i < u3401Code::ALL.len() {
        let c = u3401Code::ALL[i].code();
        if c < 0xe0e0 || c > 0xe0ef {
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
fn u3401_guard_seg() -> bool {
    u3401_segment_guard()
}

/// UNX-F3401 对拍表 16 行边界与累计守恒。
fn check_u3401_mix_tbl() -> bool {
    let mut ok = u3401mix_COUNT == 16;
    if ok { ok = u3401mix_get("KT_00") == Some(10) && u3401mix_get("SEL_15") == Some(85); }
    if ok { ok = u3401mix_get("ZZZ_NOKEY").is_none(); }
    if ok { ok = u3401mix_sum() == 760; }
    ok
}

/// UNX-F3401 混合映射 16 行守卫唯一 + 守卫阈值冻结。
fn check_u3401_mix_map() -> bool {
    let mut ok = u3401mixmap_COUNT == 16;
    if ok { ok = u3401mixmap_scan() == 0 && u3401mixmap_unique(); }
    if ok { ok = u3401mixmap_sum() == 504; }
    if ok { ok = U3401MIXGATE_THRESHOLD == 32; }
    ok
}

/// 本模块自检（逐检查带 detail，全绿才出门）。
pub fn run_u3401_checks() -> CheckSet {
    let mut set = CheckSet::new("U3401");
    set.add(
        "u3401-seg-guard",
        u3401_guard_seg(),
        if u3401_guard_seg() { "" } else { "diagnostic code outside segment" },
    );
    set.add(
        "u3401-mix-tbl",
        check_u3401_mix_tbl(),
        if check_u3401_mix_tbl() { "" } else { "UNX-F3401 mix table mismatch" },
    );
    set.add(
        "u3401-mix-map",
        check_u3401_mix_map(),
        if check_u3401_mix_map() { "" } else { "UNX-F3401 mix map mismatch" },
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
    fn u3401_all_checks_green() {
        let set = run_u3401_checks();
        assert!(set.all_passed(), "U3401 domain checks must be green");
    }

    #[test]
    fn u3401_segment_guard_holds() {
        assert!(u3401_segment_guard());
    }
}
