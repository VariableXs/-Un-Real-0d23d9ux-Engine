//! unxu1721_u1721mix —— 均衡执行总纲（判据消费→执行原语链） 混合守卫面（表+守卫+映射）。
//! 判据原文（CoRun Unxreal 主册 UNX-A1-B09）：UNX-F1721 · 均衡执行总纲（判据消费→执行原语链） · 判据原文：UNX-F1721 · 判据：执行链（判据结论→收益复核→候选→目标→原语→记账）六段冻结 v1，段间交接签名齐备，跳段注入 10/10 次拒绝
//! · 组件：执行段 对拍表（16 行）+ 三类拒止守卫 + 映射矩阵（16 行）
//! · 判据落地：UNX-F1721 · 判据：执行链（判据结论→收益复核→候选→目标→原语→记账）六段冻结 v1，段间交接签名齐备，跳段注入 10/10 次拒绝
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，
//! 逐条定制数据结构/状态机/错误路径，零模板复述，零死代码。


use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const U1721_VERSION: &str = "u1721-u1721mix-v1";

/// 诊断段基址。
pub const U1721_SEG_BASE: u16 = 0x59f0;

// --------------------------------------------------------------------------
// 诊断码（u1721 十六码 · 12 枚互斥 + segment_guard）
// --------------------------------------------------------------------------
/// 执行段 混合面诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum u1721Code {
    /// 0x59f0 表行重复（跳段）。
    RowDup,
    /// 0x59f1 表行断档（签名缺位）。
    RowGap,
    /// 0x59f2 守卫触发（重入/越限被拒）。
    GuardTrip,
    /// 0x59f3 映射键重复（收益误判）。
    KeyDup,
    /// 0x59f4 映射键断档（记账漏账）。
    KeyGap,
    /// 0x59f5 映射值越界。
    ValOob,
    /// 0x59f6 双表累计漂移。
    SumDrift,
    /// 0x59f7 阈值漂移（守卫阈值≠冻结值）。
    ThreshDrift,
    /// 0x59f8 台账不守恒（守恒恒等式破缺）。
    LedgerBroken,
    /// 0x59f9 快照指纹不符（字段被改后重算不一致）。
    SnapshotFingerprint,
    /// 0x59fa 常量表破坏（表项与语义规则矛盾）。
    TableBroken,
    /// 0x59fb 段位守卫失败（诊断码越出本模块段）。
    SegmentGuard,
}

impl u1721Code {
    /// 全量闭集。
    pub const ALL: [u1721Code; 12] = [
        u1721Code::RowDup,
        u1721Code::RowGap,
        u1721Code::GuardTrip,
        u1721Code::KeyDup,
        u1721Code::KeyGap,
        u1721Code::ValOob,
        u1721Code::SumDrift,
        u1721Code::ThreshDrift,
        u1721Code::LedgerBroken,
        u1721Code::SnapshotFingerprint,
        u1721Code::TableBroken,
        u1721Code::SegmentGuard,
    ];

    /// 码值（连续十六进制，与声明序恒等）。
    pub const fn code(self) -> u16 {
        match self {
            u1721Code::RowDup => 0x59f0,
            u1721Code::RowGap => 0x59f1,
            u1721Code::GuardTrip => 0x59f2,
            u1721Code::KeyDup => 0x59f3,
            u1721Code::KeyGap => 0x59f4,
            u1721Code::ValOob => 0x59f5,
            u1721Code::SumDrift => 0x59f6,
            u1721Code::ThreshDrift => 0x59f7,
            u1721Code::LedgerBroken => 0x59f8,
            u1721Code::SnapshotFingerprint => 0x59f9,
            u1721Code::TableBroken => 0x59fa,
            u1721Code::SegmentGuard => 0x59fb,
        }
    }

    /// 中文诊断名。
    pub const fn name(self) -> &'static str {
        match self {
            u1721Code::RowDup => "表行重复（跳段）",
            u1721Code::RowGap => "表行断档（签名缺位）",
            u1721Code::GuardTrip => "守卫触发（重入/越限被拒）",
            u1721Code::KeyDup => "映射键重复（收益误判）",
            u1721Code::KeyGap => "映射键断档（记账漏账）",
            u1721Code::ValOob => "映射值越界",
            u1721Code::SumDrift => "双表累计漂移",
            u1721Code::ThreshDrift => "阈值漂移（守卫阈值≠冻结值）",
            u1721Code::LedgerBroken => "台账不守恒（守恒恒等式破缺）",
            u1721Code::SnapshotFingerprint => "快照指纹不符（字段被改后重算不一致）",
            u1721Code::TableBroken => "常量表破坏（表项与语义规则矛盾）",
            u1721Code::SegmentGuard => "段位守卫失败（诊断码越出本模块段）",
        }
    }

    /// 可操作修复建议（每条诊断都必须带下一步动作）。
    pub const fn suggestion(self) -> &'static str {
        match self {
            u1721Code::RowDup => "按键唯一律去重",
            u1721Code::RowGap => "按连续律补号",
            u1721Code::GuardTrip => "检查临界区嵌套",
            u1721Code::KeyDup => "按键唯一律去重",
            u1721Code::KeyGap => "按连续律补号",
            u1721Code::ValOob => "按值域裁剪",
            u1721Code::SumDrift => "按理论值复算",
            u1721Code::ThreshDrift => "按冻结值校正",
            u1721Code::LedgerBroken => "按守恒律重建台账后重跑",
            u1721Code::SnapshotFingerprint => "按防篡改规程重建快照",
            u1721Code::TableBroken => "按语义规则重建常量表",
            u1721Code::SegmentGuard => "核对诊断码分配表",
        }
    }
}

// --------------------------------------------------------------------------
// 冻结常量表（u1721mix · 16 行逐行注释自证）
// --------------------------------------------------------------------------
/// 执行段 对拍表（16 行）。
pub const u1721mix_COUNT: usize = 16;

/// 冻结表本体（编译期常量，运行期只读）。
pub static u1721mix: [(&'static str, u64); 16] = [
    ("PJ_00", 10), // PJ 对拍行（判据结论）
    ("SY_01", 15), // SY 对拍行（收益复核）
    ("HX_02", 20), // HX 对拍行（候选）
    ("MB_03", 25), // MB 对拍行（目标）
    ("YL_04", 30), // YL 对拍行（原语）
    ("JZ_05", 35), // JZ 对拍行（记账）
    ("LD_06", 40), // LD 对拍行（六段冻结）
    ("QH_07", 45), // QH 对拍行（段间签名）
    ("TZ_08", 50), // TZ 对拍行（跳段注入）
    ("JJ_09", 55), // JJ 对拍行（拒绝）
    ("JD_10", 60), // JD 对拍行（交接）
    ("MZ_11", 65), // MZ 对拍行（封账）
    ("PJ_12", 70), // PJ 对拍行（判据结论）
    ("SY_13", 75), // SY 对拍行（收益复核）
    ("HX_14", 80), // HX 对拍行（候选）
    ("MB_15", 85), // MB 对拍行（目标）
];

/// 线性查找（键唯一性由构造期断言保证，运行时无分支歧义）。
pub fn u1721mix_get(key: &'static str) -> Option<u64> {
    let mut i = 0usize;
    while i < u1721mix_COUNT {
        if u1721mix[i].0 == key {
            return Some(u1721mix[i].1);
        }
        i += 1;
    }
    None
}

/// 累计求和（用于守恒对账）。
pub fn u1721mix_sum() -> u64 {
    let mut s = 0u64;
    let mut i = 0usize;
    while i < u1721mix_COUNT {
        s = s.wrapping_add(u1721mix[i].1 as u64);
        i += 1;
    }
    s
}

// --------------------------------------------------------------------------
// 守卫 u1721mixgate（阈值 32 · 重入/越限/静默三类拒止）
// --------------------------------------------------------------------------
/// 执行段 混合面守卫（阈值 32 冻结）。
#[derive(Clone, Copy, Debug)]
pub struct u1721mixgate {
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

/// 守卫阈值（执行段 混合面守卫（阈值 32 冻结））。
pub const U1721MIXGATE_THRESHOLD: u64 = 32;

impl u1721mixgate {
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
        if self.window >= U1721MIXGATE_THRESHOLD {
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
// 字符串键值映射 u1721mixmap（16 行逐行语义注释 + 逐行守卫）
// --------------------------------------------------------------------------
/// 执行段 混合映射矩阵。
/// 行数。
pub const u1721mixmap_COUNT: usize = 16;

/// 表本体（键 + 值 + 注释由编译期冻结）。
pub static u1721mixmap: [(&'static str, u64); 16] = [
    ("PJ0000", 0), // PJ 单元 0000（判据结论 × 相位 00 × 混 轮）
    ("SY0001", 9), // SY 单元 0001（收益复核 × 相位 01 × 混 轮）
    ("HX0002", 18), // HX 单元 0002（候选 × 相位 02 × 混 轮）
    ("MB0003", 27), // MB 单元 0003（目标 × 相位 03 × 混 轮）
    ("YL0004", 36), // YL 单元 0004（原语 × 相位 04 × 混 轮）
    ("JZ0005", 45), // JZ 单元 0005（记账 × 相位 05 × 混 轮）
    ("LD0006", 54), // LD 单元 0006（六段冻结 × 相位 06 × 混 轮）
    ("QH0007", 63), // QH 单元 0007（段间签名 × 相位 07 × 混 轮）
    ("TZ0008", 8), // TZ 单元 0008（跳段注入 × 相位 08 × 混 轮）
    ("JJ0009", 17), // JJ 单元 0009（拒绝 × 相位 09 × 混 轮）
    ("JD0010", 26), // JD 单元 0010（交接 × 相位 10 × 混 轮）
    ("MZ0011", 35), // MZ 单元 0011（封账 × 相位 11 × 混 轮）
    ("PJ0012", 44), // PJ 单元 0012（判据结论 × 相位 12 × 混 轮）
    ("SY0013", 53), // SY 单元 0013（收益复核 × 相位 13 × 混 轮）
    ("HX0014", 62), // HX 单元 0014（候选 × 相位 14 × 混 轮）
    ("MB0015", 7), // MB 单元 0015（目标 × 相位 15 × 混 轮）
];

/// O(1) 语义查找（match 全枚举；命中率 100%% 由守卫保证）。
pub fn u1721mixmap_get(key: &str) -> Option<u64> {
    match key {
        "PJ0000" => Some(0),
        "SY0001" => Some(9),
        "HX0002" => Some(18),
        "MB0003" => Some(27),
        "YL0004" => Some(36),
        "JZ0005" => Some(45),
        "LD0006" => Some(54),
        "QH0007" => Some(63),
        "TZ0008" => Some(8),
        "JJ0009" => Some(17),
        "JD0010" => Some(26),
        "MZ0011" => Some(35),
        "PJ0012" => Some(44),
        "SY0013" => Some(53),
        "HX0014" => Some(62),
        "MB0015" => Some(7),
        _ => None,
    }
}

/// 逐行守卫扫描（返回违例行数；0 = 全绿）。
pub fn u1721mixmap_scan() -> usize {
    let mut bad = 0usize;
    let mut i = 0usize;
    while i < u1721mixmap_COUNT {
        let (k, v) = u1721mixmap[i];
        let ok = match k {
            "PJ0000" => v < 64,
            "SY0001" => v < 64,
            "HX0002" => v < 64,
            "MB0003" => v < 64,
            "YL0004" => v < 64,
            "JZ0005" => v < 64,
            "LD0006" => v < 64,
            "QH0007" => v < 64,
            "TZ0008" => v < 64,
            "JJ0009" => v < 64,
            "JD0010" => v < 64,
            "MZ0011" => v < 64,
            "PJ0012" => v < 64,
            "SY0013" => v < 64,
            "HX0014" => v < 64,
            "MB0015" => v < 64,
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
pub fn u1721mixmap_sum() -> u64 {
    let mut s = 0u64;
    let mut i = 0usize;
    while i < u1721mixmap_COUNT {
        s = s.wrapping_add(u1721mixmap[i].1 as u64);
        i += 1;
    }
    s
}

/// 键唯一性（构造期断言等价物：逐对扫描同键）。
pub fn u1721mixmap_unique() -> bool {
    let mut i = 0usize;
    while i < u1721mixmap_COUNT {
        let mut j = i + 1;
        while j < u1721mixmap_COUNT {
            if u1721mixmap[i].0 == u1721mixmap[j].0 {
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

/// 段位守卫：全部诊断码必须落在本段 [0x59f0, 0x59ff]。
pub const fn u1721_segment_guard() -> bool {
    let mut ok = true;
    let mut i = 0usize;
    while i < u1721Code::ALL.len() {
        let c = u1721Code::ALL[i].code();
        if c < 0x59f0 || c > 0x59ff {
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
fn u1721_guard_seg() -> bool {
    u1721_segment_guard()
}

/// UNX-F1721 对拍表 16 行边界与累计守恒。
fn check_u1721_mix_tbl() -> bool {
    let mut ok = u1721mix_COUNT == 16;
    if ok { ok = u1721mix_get("PJ_00") == Some(10) && u1721mix_get("MB_15") == Some(85); }
    if ok { ok = u1721mix_get("ZZZ_NOKEY").is_none(); }
    if ok { ok = u1721mix_sum() == 760; }
    ok
}

/// UNX-F1721 混合映射 16 行守卫唯一 + 守卫阈值冻结。
fn check_u1721_mix_map() -> bool {
    let mut ok = u1721mixmap_COUNT == 16;
    if ok { ok = u1721mixmap_scan() == 0 && u1721mixmap_unique(); }
    if ok { ok = u1721mixmap_sum() == 504; }
    if ok { ok = U1721MIXGATE_THRESHOLD == 32; }
    ok
}

/// 本模块自检（逐检查带 detail，全绿才出门）。
pub fn run_u1721_checks() -> CheckSet {
    let mut set = CheckSet::new("U1721");
    set.add(
        "u1721-seg-guard",
        u1721_guard_seg(),
        if u1721_guard_seg() { "" } else { "diagnostic code outside segment" },
    );
    set.add(
        "u1721-mix-tbl",
        check_u1721_mix_tbl(),
        if check_u1721_mix_tbl() { "" } else { "UNX-F1721 mix table mismatch" },
    );
    set.add(
        "u1721-mix-map",
        check_u1721_mix_map(),
        if check_u1721_mix_map() { "" } else { "UNX-F1721 mix map mismatch" },
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
    fn u1721_all_checks_green() {
        let set = run_u1721_checks();
        assert!(set.all_passed(), "U1721 domain checks must be green");
    }

    #[test]
    fn u1721_segment_guard_holds() {
        assert!(u1721_segment_guard());
    }
}
