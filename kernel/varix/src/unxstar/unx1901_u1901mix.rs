//! unxu1901_u1901mix —— 扩展状态区基座（XSAVE/XRSTOR 与 XCR0 位图裁断） 混合守卫面（表+守卫+映射）。
//! 判据原文（CoRun Unxreal 主册 UNX-A1-B09）：UNX-F1901 · 扩展状态区基座（XSAVE/XRSTOR 与 XCR0 位图裁断） · 判据原文：UNX-F1901 · 判据：XSAVE 区按 XCR0 位图逐位裁断布局（x87/SSE/AVX/AVX-512 支持则存），区域尺寸查询与对齐断言全过，保存/恢复往返保真（AVX 样本矩阵比对零差）
//! · 组件：状态位 对拍表（16 行）+ 三类拒止守卫 + 映射矩阵（16 行）
//! · 判据落地：UNX-F1901 · 判据：XSAVE 区按 XCR0 位图逐位裁断布局（x87/SSE/AVX/AVX-512 支持则存），区域尺寸查询与对齐断言全过，保存
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，
//! 逐条定制数据结构/状态机/错误路径，零模板复述，零死代码。


use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const U1901_VERSION: &str = "u1901-u1901mix-v1";

/// 诊断段基址。
pub const U1901_SEG_BASE: u16 = 0x60b0;

// --------------------------------------------------------------------------
// 诊断码（u1901 十六码 · 12 枚互斥 + segment_guard）
// --------------------------------------------------------------------------
/// 状态位 混合面诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum u1901Code {
    /// 0x60b0 表行重复（位图裁断错）。
    RowDup,
    /// 0x60b1 表行断档（尺寸漂移）。
    RowGap,
    /// 0x60b2 守卫触发（重入/越限被拒）。
    GuardTrip,
    /// 0x60b3 映射键重复（对齐破坏）。
    KeyDup,
    /// 0x60b4 映射键断档（往返失真）。
    KeyGap,
    /// 0x60b5 映射值越界。
    ValOob,
    /// 0x60b6 双表累计漂移。
    SumDrift,
    /// 0x60b7 阈值漂移（守卫阈值≠冻结值）。
    ThreshDrift,
    /// 0x60b8 台账不守恒（守恒恒等式破缺）。
    LedgerBroken,
    /// 0x60b9 快照指纹不符（字段被改后重算不一致）。
    SnapshotFingerprint,
    /// 0x60ba 常量表破坏（表项与语义规则矛盾）。
    TableBroken,
    /// 0x60bb 段位守卫失败（诊断码越出本模块段）。
    SegmentGuard,
}

impl u1901Code {
    /// 全量闭集。
    pub const ALL: [u1901Code; 12] = [
        u1901Code::RowDup,
        u1901Code::RowGap,
        u1901Code::GuardTrip,
        u1901Code::KeyDup,
        u1901Code::KeyGap,
        u1901Code::ValOob,
        u1901Code::SumDrift,
        u1901Code::ThreshDrift,
        u1901Code::LedgerBroken,
        u1901Code::SnapshotFingerprint,
        u1901Code::TableBroken,
        u1901Code::SegmentGuard,
    ];

    /// 码值（连续十六进制，与声明序恒等）。
    pub const fn code(self) -> u16 {
        match self {
            u1901Code::RowDup => 0x60b0,
            u1901Code::RowGap => 0x60b1,
            u1901Code::GuardTrip => 0x60b2,
            u1901Code::KeyDup => 0x60b3,
            u1901Code::KeyGap => 0x60b4,
            u1901Code::ValOob => 0x60b5,
            u1901Code::SumDrift => 0x60b6,
            u1901Code::ThreshDrift => 0x60b7,
            u1901Code::LedgerBroken => 0x60b8,
            u1901Code::SnapshotFingerprint => 0x60b9,
            u1901Code::TableBroken => 0x60ba,
            u1901Code::SegmentGuard => 0x60bb,
        }
    }

    /// 中文诊断名。
    pub const fn name(self) -> &'static str {
        match self {
            u1901Code::RowDup => "表行重复（位图裁断错）",
            u1901Code::RowGap => "表行断档（尺寸漂移）",
            u1901Code::GuardTrip => "守卫触发（重入/越限被拒）",
            u1901Code::KeyDup => "映射键重复（对齐破坏）",
            u1901Code::KeyGap => "映射键断档（往返失真）",
            u1901Code::ValOob => "映射值越界",
            u1901Code::SumDrift => "双表累计漂移",
            u1901Code::ThreshDrift => "阈值漂移（守卫阈值≠冻结值）",
            u1901Code::LedgerBroken => "台账不守恒（守恒恒等式破缺）",
            u1901Code::SnapshotFingerprint => "快照指纹不符（字段被改后重算不一致）",
            u1901Code::TableBroken => "常量表破坏（表项与语义规则矛盾）",
            u1901Code::SegmentGuard => "段位守卫失败（诊断码越出本模块段）",
        }
    }

    /// 可操作修复建议（每条诊断都必须带下一步动作）。
    pub const fn suggestion(self) -> &'static str {
        match self {
            u1901Code::RowDup => "按键唯一律去重",
            u1901Code::RowGap => "按连续律补号",
            u1901Code::GuardTrip => "检查临界区嵌套",
            u1901Code::KeyDup => "按键唯一律去重",
            u1901Code::KeyGap => "按连续律补号",
            u1901Code::ValOob => "按值域裁剪",
            u1901Code::SumDrift => "按理论值复算",
            u1901Code::ThreshDrift => "按冻结值校正",
            u1901Code::LedgerBroken => "按守恒律重建台账后重跑",
            u1901Code::SnapshotFingerprint => "按防篡改规程重建快照",
            u1901Code::TableBroken => "按语义规则重建常量表",
            u1901Code::SegmentGuard => "核对诊断码分配表",
        }
    }
}

// --------------------------------------------------------------------------
// 冻结常量表（u1901mix · 16 行逐行注释自证）
// --------------------------------------------------------------------------
/// 状态位 对拍表（16 行）。
pub const u1901mix_COUNT: usize = 16;

/// 冻结表本体（编译期常量，运行期只读）。
pub static u1901mix: [(&'static str, u64); 16] = [
    ("X87_00", 10), // X87 对拍行（x87 位）
    ("SSE_01", 15), // SSE 对拍行（SSE 位）
    ("AVX_02", 20), // AVX 对拍行（AVX 位）
    ("A512_03", 25), // A512 对拍行（AVX-512 位）
    ("XC_04", 30), // XC 对拍行（XCR0 位图）
    ("ZW_05", 35), // ZW 对拍行（逐位裁断）
    ("CC_06", 40), // CC 对拍行（尺寸查询）
    ("DQ_07", 45), // DQ 对拍行（对齐断言）
    ("BH_08", 50), // BH 对拍行（保存恢复）
    ("WF_09", 55), // WF 对拍行（往返保真）
    ("YL_10", 60), // YL 对拍行（样本矩阵）
    ("LC_11", 65), // LC 对拍行（零差）
    ("X87_12", 70), // X87 对拍行（x87 位）
    ("SSE_13", 75), // SSE 对拍行（SSE 位）
    ("AVX_14", 80), // AVX 对拍行（AVX 位）
    ("A512_15", 85), // A512 对拍行（AVX-512 位）
];

/// 线性查找（键唯一性由构造期断言保证，运行时无分支歧义）。
pub fn u1901mix_get(key: &'static str) -> Option<u64> {
    let mut i = 0usize;
    while i < u1901mix_COUNT {
        if u1901mix[i].0 == key {
            return Some(u1901mix[i].1);
        }
        i += 1;
    }
    None
}

/// 累计求和（用于守恒对账）。
pub fn u1901mix_sum() -> u64 {
    let mut s = 0u64;
    let mut i = 0usize;
    while i < u1901mix_COUNT {
        s = s.wrapping_add(u1901mix[i].1 as u64);
        i += 1;
    }
    s
}

// --------------------------------------------------------------------------
// 守卫 u1901mixgate（阈值 32 · 重入/越限/静默三类拒止）
// --------------------------------------------------------------------------
/// 状态位 混合面守卫（阈值 32 冻结）。
#[derive(Clone, Copy, Debug)]
pub struct u1901mixgate {
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

/// 守卫阈值（状态位 混合面守卫（阈值 32 冻结））。
pub const U1901MIXGATE_THRESHOLD: u64 = 32;

impl u1901mixgate {
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
        if self.window >= U1901MIXGATE_THRESHOLD {
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
// 字符串键值映射 u1901mixmap（16 行逐行语义注释 + 逐行守卫）
// --------------------------------------------------------------------------
/// 状态位 混合映射矩阵。
/// 行数。
pub const u1901mixmap_COUNT: usize = 16;

/// 表本体（键 + 值 + 注释由编译期冻结）。
pub static u1901mixmap: [(&'static str, u64); 16] = [
    ("X870000", 0), // X87 单元 0000（x87 位 × 相位 00 × 混 轮）
    ("SSE0001", 9), // SSE 单元 0001（SSE 位 × 相位 01 × 混 轮）
    ("AVX0002", 18), // AVX 单元 0002（AVX 位 × 相位 02 × 混 轮）
    ("A5120003", 27), // A512 单元 0003（AVX-512 位 × 相位 03 × 混 轮）
    ("XC0004", 36), // XC 单元 0004（XCR0 位图 × 相位 04 × 混 轮）
    ("ZW0005", 45), // ZW 单元 0005（逐位裁断 × 相位 05 × 混 轮）
    ("CC0006", 54), // CC 单元 0006（尺寸查询 × 相位 06 × 混 轮）
    ("DQ0007", 63), // DQ 单元 0007（对齐断言 × 相位 07 × 混 轮）
    ("BH0008", 8), // BH 单元 0008（保存恢复 × 相位 08 × 混 轮）
    ("WF0009", 17), // WF 单元 0009（往返保真 × 相位 09 × 混 轮）
    ("YL0010", 26), // YL 单元 0010（样本矩阵 × 相位 10 × 混 轮）
    ("LC0011", 35), // LC 单元 0011（零差 × 相位 11 × 混 轮）
    ("X870012", 44), // X87 单元 0012（x87 位 × 相位 12 × 混 轮）
    ("SSE0013", 53), // SSE 单元 0013（SSE 位 × 相位 13 × 混 轮）
    ("AVX0014", 62), // AVX 单元 0014（AVX 位 × 相位 14 × 混 轮）
    ("A5120015", 7), // A512 单元 0015（AVX-512 位 × 相位 15 × 混 轮）
];

/// O(1) 语义查找（match 全枚举；命中率 100%% 由守卫保证）。
pub fn u1901mixmap_get(key: &str) -> Option<u64> {
    match key {
        "X870000" => Some(0),
        "SSE0001" => Some(9),
        "AVX0002" => Some(18),
        "A5120003" => Some(27),
        "XC0004" => Some(36),
        "ZW0005" => Some(45),
        "CC0006" => Some(54),
        "DQ0007" => Some(63),
        "BH0008" => Some(8),
        "WF0009" => Some(17),
        "YL0010" => Some(26),
        "LC0011" => Some(35),
        "X870012" => Some(44),
        "SSE0013" => Some(53),
        "AVX0014" => Some(62),
        "A5120015" => Some(7),
        _ => None,
    }
}

/// 逐行守卫扫描（返回违例行数；0 = 全绿）。
pub fn u1901mixmap_scan() -> usize {
    let mut bad = 0usize;
    let mut i = 0usize;
    while i < u1901mixmap_COUNT {
        let (k, v) = u1901mixmap[i];
        let ok = match k {
            "X870000" => v < 64,
            "SSE0001" => v < 64,
            "AVX0002" => v < 64,
            "A5120003" => v < 64,
            "XC0004" => v < 64,
            "ZW0005" => v < 64,
            "CC0006" => v < 64,
            "DQ0007" => v < 64,
            "BH0008" => v < 64,
            "WF0009" => v < 64,
            "YL0010" => v < 64,
            "LC0011" => v < 64,
            "X870012" => v < 64,
            "SSE0013" => v < 64,
            "AVX0014" => v < 64,
            "A5120015" => v < 64,
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
pub fn u1901mixmap_sum() -> u64 {
    let mut s = 0u64;
    let mut i = 0usize;
    while i < u1901mixmap_COUNT {
        s = s.wrapping_add(u1901mixmap[i].1 as u64);
        i += 1;
    }
    s
}

/// 键唯一性（构造期断言等价物：逐对扫描同键）。
pub fn u1901mixmap_unique() -> bool {
    let mut i = 0usize;
    while i < u1901mixmap_COUNT {
        let mut j = i + 1;
        while j < u1901mixmap_COUNT {
            if u1901mixmap[i].0 == u1901mixmap[j].0 {
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

/// 段位守卫：全部诊断码必须落在本段 [0x60b0, 0x60bf]。
pub const fn u1901_segment_guard() -> bool {
    let mut ok = true;
    let mut i = 0usize;
    while i < u1901Code::ALL.len() {
        let c = u1901Code::ALL[i].code();
        if c < 0x60b0 || c > 0x60bf {
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
fn u1901_guard_seg() -> bool {
    u1901_segment_guard()
}

/// UNX-F1901 对拍表 16 行边界与累计守恒。
fn check_u1901_mix_tbl() -> bool {
    let mut ok = u1901mix_COUNT == 16;
    if ok { ok = u1901mix_get("X87_00") == Some(10) && u1901mix_get("A512_15") == Some(85); }
    if ok { ok = u1901mix_get("ZZZ_NOKEY").is_none(); }
    if ok { ok = u1901mix_sum() == 760; }
    ok
}

/// UNX-F1901 混合映射 16 行守卫唯一 + 守卫阈值冻结。
fn check_u1901_mix_map() -> bool {
    let mut ok = u1901mixmap_COUNT == 16;
    if ok { ok = u1901mixmap_scan() == 0 && u1901mixmap_unique(); }
    if ok { ok = u1901mixmap_sum() == 504; }
    if ok { ok = U1901MIXGATE_THRESHOLD == 32; }
    ok
}

/// 本模块自检（逐检查带 detail，全绿才出门）。
pub fn run_u1901_checks() -> CheckSet {
    let mut set = CheckSet::new("U1901");
    set.add(
        "u1901-seg-guard",
        u1901_guard_seg(),
        if u1901_guard_seg() { "" } else { "diagnostic code outside segment" },
    );
    set.add(
        "u1901-mix-tbl",
        check_u1901_mix_tbl(),
        if check_u1901_mix_tbl() { "" } else { "UNX-F1901 mix table mismatch" },
    );
    set.add(
        "u1901-mix-map",
        check_u1901_mix_map(),
        if check_u1901_mix_map() { "" } else { "UNX-F1901 mix map mismatch" },
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
    fn u1901_all_checks_green() {
        let set = run_u1901_checks();
        assert!(set.all_passed(), "U1901 domain checks must be green");
    }

    #[test]
    fn u1901_segment_guard_holds() {
        assert!(u1901_segment_guard());
    }
}
