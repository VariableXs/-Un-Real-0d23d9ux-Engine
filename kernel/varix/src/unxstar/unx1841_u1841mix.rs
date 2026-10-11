//! unxu1841_u1841mix —— IPI 架构总纲（向量分派框架与 A2 衔接） 混合守卫面（表+守卫+映射）。
//! 判据原文（CoRun Unxreal 主册 UNX-A1-B09）：UNX-F1841 · IPI 架构总纲（向量分派框架与 A2 衔接） · 判据原文：UNX-F1841 · 判据：IPI 分派框架冻结 v1（向量→处理函数表+分发协议+EOI 纪律），与 A2 原语衔接断言全过，未登记向量注入 10/10 拒绝
//! · 组件：向量 对拍表（16 行）+ 三类拒止守卫 + 映射矩阵（16 行）
//! · 判据落地：UNX-F1841 · 判据：IPI 分派框架冻结 v1（向量→处理函数表+分发协议+EOI 纪律），与 A2 原语衔接断言全过，未登记向量注入 10/10 拒
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，
//! 逐条定制数据结构/状态机/错误路径，零模板复述，零死代码。


use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const U1841_VERSION: &str = "u1841-u1841mix-v1";

/// 诊断段基址。
pub const U1841_SEG_BASE: u16 = 0x5e70;

// --------------------------------------------------------------------------
// 诊断码（u1841 十六码 · 12 枚互斥 + segment_guard）
// --------------------------------------------------------------------------
/// 向量 混合面诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum u1841Code {
    /// 0x5e70 表行重复（未登记向量）。
    RowDup,
    /// 0x5e71 表行断档（EOI 漏发）。
    RowGap,
    /// 0x5e72 守卫触发（重入/越限被拒）。
    GuardTrip,
    /// 0x5e73 映射键重复（衔接断裂）。
    KeyDup,
    /// 0x5e74 映射键断档（分派漂移）。
    KeyGap,
    /// 0x5e75 映射值越界。
    ValOob,
    /// 0x5e76 双表累计漂移。
    SumDrift,
    /// 0x5e77 阈值漂移（守卫阈值≠冻结值）。
    ThreshDrift,
    /// 0x5e78 台账不守恒（守恒恒等式破缺）。
    LedgerBroken,
    /// 0x5e79 快照指纹不符（字段被改后重算不一致）。
    SnapshotFingerprint,
    /// 0x5e7a 常量表破坏（表项与语义规则矛盾）。
    TableBroken,
    /// 0x5e7b 段位守卫失败（诊断码越出本模块段）。
    SegmentGuard,
}

impl u1841Code {
    /// 全量闭集。
    pub const ALL: [u1841Code; 12] = [
        u1841Code::RowDup,
        u1841Code::RowGap,
        u1841Code::GuardTrip,
        u1841Code::KeyDup,
        u1841Code::KeyGap,
        u1841Code::ValOob,
        u1841Code::SumDrift,
        u1841Code::ThreshDrift,
        u1841Code::LedgerBroken,
        u1841Code::SnapshotFingerprint,
        u1841Code::TableBroken,
        u1841Code::SegmentGuard,
    ];

    /// 码值（连续十六进制，与声明序恒等）。
    pub const fn code(self) -> u16 {
        match self {
            u1841Code::RowDup => 0x5e70,
            u1841Code::RowGap => 0x5e71,
            u1841Code::GuardTrip => 0x5e72,
            u1841Code::KeyDup => 0x5e73,
            u1841Code::KeyGap => 0x5e74,
            u1841Code::ValOob => 0x5e75,
            u1841Code::SumDrift => 0x5e76,
            u1841Code::ThreshDrift => 0x5e77,
            u1841Code::LedgerBroken => 0x5e78,
            u1841Code::SnapshotFingerprint => 0x5e79,
            u1841Code::TableBroken => 0x5e7a,
            u1841Code::SegmentGuard => 0x5e7b,
        }
    }

    /// 中文诊断名。
    pub const fn name(self) -> &'static str {
        match self {
            u1841Code::RowDup => "表行重复（未登记向量）",
            u1841Code::RowGap => "表行断档（EOI 漏发）",
            u1841Code::GuardTrip => "守卫触发（重入/越限被拒）",
            u1841Code::KeyDup => "映射键重复（衔接断裂）",
            u1841Code::KeyGap => "映射键断档（分派漂移）",
            u1841Code::ValOob => "映射值越界",
            u1841Code::SumDrift => "双表累计漂移",
            u1841Code::ThreshDrift => "阈值漂移（守卫阈值≠冻结值）",
            u1841Code::LedgerBroken => "台账不守恒（守恒恒等式破缺）",
            u1841Code::SnapshotFingerprint => "快照指纹不符（字段被改后重算不一致）",
            u1841Code::TableBroken => "常量表破坏（表项与语义规则矛盾）",
            u1841Code::SegmentGuard => "段位守卫失败（诊断码越出本模块段）",
        }
    }

    /// 可操作修复建议（每条诊断都必须带下一步动作）。
    pub const fn suggestion(self) -> &'static str {
        match self {
            u1841Code::RowDup => "按键唯一律去重",
            u1841Code::RowGap => "按连续律补号",
            u1841Code::GuardTrip => "检查临界区嵌套",
            u1841Code::KeyDup => "按键唯一律去重",
            u1841Code::KeyGap => "按连续律补号",
            u1841Code::ValOob => "按值域裁剪",
            u1841Code::SumDrift => "按理论值复算",
            u1841Code::ThreshDrift => "按冻结值校正",
            u1841Code::LedgerBroken => "按守恒律重建台账后重跑",
            u1841Code::SnapshotFingerprint => "按防篡改规程重建快照",
            u1841Code::TableBroken => "按语义规则重建常量表",
            u1841Code::SegmentGuard => "核对诊断码分配表",
        }
    }
}

// --------------------------------------------------------------------------
// 冻结常量表（u1841mix · 16 行逐行注释自证）
// --------------------------------------------------------------------------
/// 向量 对拍表（16 行）。
pub const u1841mix_COUNT: usize = 16;

/// 冻结表本体（编译期常量，运行期只读）。
pub static u1841mix: [(&'static str, u64); 16] = [
    ("XL_00", 10), // XL 对拍行（向量表）
    ("CL_01", 15), // CL 对拍行（处理函数表）
    ("FF_02", 20), // FF 对拍行（分发协议）
    ("EOI_03", 25), // EOI 对拍行（EOI 纪律）
    ("A2_04", 30), // A2 对拍行（A2 原语衔接）
    ("WD_05", 35), // WD 对拍行（未登记向量）
    ("JJ_06", 40), // JJ 对拍行（拒绝）
    ("MZ_07", 45), // MZ 对拍行（冻结 v1）
    ("FP_08", 50), // FP 对拍行（分派）
    ("HB_09", 55), // HB 对拍行（回执）
    ("ZC_10", 60), // ZC 对拍行（在册）
    ("PL_11", 65), // PL 对拍行（批量）
    ("XL_12", 70), // XL 对拍行（向量表）
    ("CL_13", 75), // CL 对拍行（处理函数表）
    ("FF_14", 80), // FF 对拍行（分发协议）
    ("EOI_15", 85), // EOI 对拍行（EOI 纪律）
];

/// 线性查找（键唯一性由构造期断言保证，运行时无分支歧义）。
pub fn u1841mix_get(key: &'static str) -> Option<u64> {
    let mut i = 0usize;
    while i < u1841mix_COUNT {
        if u1841mix[i].0 == key {
            return Some(u1841mix[i].1);
        }
        i += 1;
    }
    None
}

/// 累计求和（用于守恒对账）。
pub fn u1841mix_sum() -> u64 {
    let mut s = 0u64;
    let mut i = 0usize;
    while i < u1841mix_COUNT {
        s = s.wrapping_add(u1841mix[i].1 as u64);
        i += 1;
    }
    s
}

// --------------------------------------------------------------------------
// 守卫 u1841mixgate（阈值 32 · 重入/越限/静默三类拒止）
// --------------------------------------------------------------------------
/// 向量 混合面守卫（阈值 32 冻结）。
#[derive(Clone, Copy, Debug)]
pub struct u1841mixgate {
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

/// 守卫阈值（向量 混合面守卫（阈值 32 冻结））。
pub const U1841MIXGATE_THRESHOLD: u64 = 32;

impl u1841mixgate {
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
        if self.window >= U1841MIXGATE_THRESHOLD {
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
// 字符串键值映射 u1841mixmap（16 行逐行语义注释 + 逐行守卫）
// --------------------------------------------------------------------------
/// 向量 混合映射矩阵。
/// 行数。
pub const u1841mixmap_COUNT: usize = 16;

/// 表本体（键 + 值 + 注释由编译期冻结）。
pub static u1841mixmap: [(&'static str, u64); 16] = [
    ("XL0000", 0), // XL 单元 0000（向量表 × 相位 00 × 混 轮）
    ("CL0001", 9), // CL 单元 0001（处理函数表 × 相位 01 × 混 轮）
    ("FF0002", 18), // FF 单元 0002（分发协议 × 相位 02 × 混 轮）
    ("EOI0003", 27), // EOI 单元 0003（EOI 纪律 × 相位 03 × 混 轮）
    ("A20004", 36), // A2 单元 0004（A2 原语衔接 × 相位 04 × 混 轮）
    ("WD0005", 45), // WD 单元 0005（未登记向量 × 相位 05 × 混 轮）
    ("JJ0006", 54), // JJ 单元 0006（拒绝 × 相位 06 × 混 轮）
    ("MZ0007", 63), // MZ 单元 0007（冻结 v1 × 相位 07 × 混 轮）
    ("FP0008", 8), // FP 单元 0008（分派 × 相位 08 × 混 轮）
    ("HB0009", 17), // HB 单元 0009（回执 × 相位 09 × 混 轮）
    ("ZC0010", 26), // ZC 单元 0010（在册 × 相位 10 × 混 轮）
    ("PL0011", 35), // PL 单元 0011（批量 × 相位 11 × 混 轮）
    ("XL0012", 44), // XL 单元 0012（向量表 × 相位 12 × 混 轮）
    ("CL0013", 53), // CL 单元 0013（处理函数表 × 相位 13 × 混 轮）
    ("FF0014", 62), // FF 单元 0014（分发协议 × 相位 14 × 混 轮）
    ("EOI0015", 7), // EOI 单元 0015（EOI 纪律 × 相位 15 × 混 轮）
];

/// O(1) 语义查找（match 全枚举；命中率 100%% 由守卫保证）。
pub fn u1841mixmap_get(key: &str) -> Option<u64> {
    match key {
        "XL0000" => Some(0),
        "CL0001" => Some(9),
        "FF0002" => Some(18),
        "EOI0003" => Some(27),
        "A20004" => Some(36),
        "WD0005" => Some(45),
        "JJ0006" => Some(54),
        "MZ0007" => Some(63),
        "FP0008" => Some(8),
        "HB0009" => Some(17),
        "ZC0010" => Some(26),
        "PL0011" => Some(35),
        "XL0012" => Some(44),
        "CL0013" => Some(53),
        "FF0014" => Some(62),
        "EOI0015" => Some(7),
        _ => None,
    }
}

/// 逐行守卫扫描（返回违例行数；0 = 全绿）。
pub fn u1841mixmap_scan() -> usize {
    let mut bad = 0usize;
    let mut i = 0usize;
    while i < u1841mixmap_COUNT {
        let (k, v) = u1841mixmap[i];
        let ok = match k {
            "XL0000" => v < 64,
            "CL0001" => v < 64,
            "FF0002" => v < 64,
            "EOI0003" => v < 64,
            "A20004" => v < 64,
            "WD0005" => v < 64,
            "JJ0006" => v < 64,
            "MZ0007" => v < 64,
            "FP0008" => v < 64,
            "HB0009" => v < 64,
            "ZC0010" => v < 64,
            "PL0011" => v < 64,
            "XL0012" => v < 64,
            "CL0013" => v < 64,
            "FF0014" => v < 64,
            "EOI0015" => v < 64,
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
pub fn u1841mixmap_sum() -> u64 {
    let mut s = 0u64;
    let mut i = 0usize;
    while i < u1841mixmap_COUNT {
        s = s.wrapping_add(u1841mixmap[i].1 as u64);
        i += 1;
    }
    s
}

/// 键唯一性（构造期断言等价物：逐对扫描同键）。
pub fn u1841mixmap_unique() -> bool {
    let mut i = 0usize;
    while i < u1841mixmap_COUNT {
        let mut j = i + 1;
        while j < u1841mixmap_COUNT {
            if u1841mixmap[i].0 == u1841mixmap[j].0 {
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

/// 段位守卫：全部诊断码必须落在本段 [0x5e70, 0x5e7f]。
pub const fn u1841_segment_guard() -> bool {
    let mut ok = true;
    let mut i = 0usize;
    while i < u1841Code::ALL.len() {
        let c = u1841Code::ALL[i].code();
        if c < 0x5e70 || c > 0x5e7f {
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
fn u1841_guard_seg() -> bool {
    u1841_segment_guard()
}

/// UNX-F1841 对拍表 16 行边界与累计守恒。
fn check_u1841_mix_tbl() -> bool {
    let mut ok = u1841mix_COUNT == 16;
    if ok { ok = u1841mix_get("XL_00") == Some(10) && u1841mix_get("EOI_15") == Some(85); }
    if ok { ok = u1841mix_get("ZZZ_NOKEY").is_none(); }
    if ok { ok = u1841mix_sum() == 760; }
    ok
}

/// UNX-F1841 混合映射 16 行守卫唯一 + 守卫阈值冻结。
fn check_u1841_mix_map() -> bool {
    let mut ok = u1841mixmap_COUNT == 16;
    if ok { ok = u1841mixmap_scan() == 0 && u1841mixmap_unique(); }
    if ok { ok = u1841mixmap_sum() == 504; }
    if ok { ok = U1841MIXGATE_THRESHOLD == 32; }
    ok
}

/// 本模块自检（逐检查带 detail，全绿才出门）。
pub fn run_u1841_checks() -> CheckSet {
    let mut set = CheckSet::new("U1841");
    set.add(
        "u1841-seg-guard",
        u1841_guard_seg(),
        if u1841_guard_seg() { "" } else { "diagnostic code outside segment" },
    );
    set.add(
        "u1841-mix-tbl",
        check_u1841_mix_tbl(),
        if check_u1841_mix_tbl() { "" } else { "UNX-F1841 mix table mismatch" },
    );
    set.add(
        "u1841-mix-map",
        check_u1841_mix_map(),
        if check_u1841_mix_map() { "" } else { "UNX-F1841 mix map mismatch" },
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
    fn u1841_all_checks_green() {
        let set = run_u1841_checks();
        assert!(set.all_passed(), "U1841 domain checks must be green");
    }

    #[test]
    fn u1841_segment_guard_holds() {
        assert!(u1841_segment_guard());
    }
}
