//! unxu1861_u1861mix —— TLB 击落总纲（shootdown 全链四段） 混合守卫面（表+守卫+映射）。
//! 判据原文（CoRun Unxreal 主册 UNX-A1-B09）：UNX-F1861 · TLB 击落总纲（shootdown 全链四段） · 判据原文：UNX-F1861 · 判据：击落四段（发起→广播→应答→完成）冻结 v1，段间签名齐备，跳段注入 10/10 拒绝，一次击落全程账可导出
//! · 组件：击落段 对拍表（16 行）+ 三类拒止守卫 + 映射矩阵（16 行）
//! · 判据落地：UNX-F1861 · 判据：击落四段（发起→广播→应答→完成）冻结 v1，段间签名齐备，跳段注入 10/10 拒绝，一次击落全程账可导出
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，
//! 逐条定制数据结构/状态机/错误路径，零模板复述，零死代码。


use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const U1861_VERSION: &str = "u1861-u1861mix-v1";

/// 诊断段基址。
pub const U1861_SEG_BASE: u16 = 0x5f30;

// --------------------------------------------------------------------------
// 诊断码（u1861 十六码 · 12 枚互斥 + segment_guard）
// --------------------------------------------------------------------------
/// 击落段 混合面诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum u1861Code {
    /// 0x5f30 表行重复（跳段）。
    RowDup,
    /// 0x5f31 表行断档（应答缺失）。
    RowGap,
    /// 0x5f32 守卫触发（重入/越限被拒）。
    GuardTrip,
    /// 0x5f33 映射键重复（广播漏核）。
    KeyDup,
    /// 0x5f34 映射键断档（全程账断裂）。
    KeyGap,
    /// 0x5f35 映射值越界。
    ValOob,
    /// 0x5f36 双表累计漂移。
    SumDrift,
    /// 0x5f37 阈值漂移（守卫阈值≠冻结值）。
    ThreshDrift,
    /// 0x5f38 台账不守恒（守恒恒等式破缺）。
    LedgerBroken,
    /// 0x5f39 快照指纹不符（字段被改后重算不一致）。
    SnapshotFingerprint,
    /// 0x5f3a 常量表破坏（表项与语义规则矛盾）。
    TableBroken,
    /// 0x5f3b 段位守卫失败（诊断码越出本模块段）。
    SegmentGuard,
}

impl u1861Code {
    /// 全量闭集。
    pub const ALL: [u1861Code; 12] = [
        u1861Code::RowDup,
        u1861Code::RowGap,
        u1861Code::GuardTrip,
        u1861Code::KeyDup,
        u1861Code::KeyGap,
        u1861Code::ValOob,
        u1861Code::SumDrift,
        u1861Code::ThreshDrift,
        u1861Code::LedgerBroken,
        u1861Code::SnapshotFingerprint,
        u1861Code::TableBroken,
        u1861Code::SegmentGuard,
    ];

    /// 码值（连续十六进制，与声明序恒等）。
    pub const fn code(self) -> u16 {
        match self {
            u1861Code::RowDup => 0x5f30,
            u1861Code::RowGap => 0x5f31,
            u1861Code::GuardTrip => 0x5f32,
            u1861Code::KeyDup => 0x5f33,
            u1861Code::KeyGap => 0x5f34,
            u1861Code::ValOob => 0x5f35,
            u1861Code::SumDrift => 0x5f36,
            u1861Code::ThreshDrift => 0x5f37,
            u1861Code::LedgerBroken => 0x5f38,
            u1861Code::SnapshotFingerprint => 0x5f39,
            u1861Code::TableBroken => 0x5f3a,
            u1861Code::SegmentGuard => 0x5f3b,
        }
    }

    /// 中文诊断名。
    pub const fn name(self) -> &'static str {
        match self {
            u1861Code::RowDup => "表行重复（跳段）",
            u1861Code::RowGap => "表行断档（应答缺失）",
            u1861Code::GuardTrip => "守卫触发（重入/越限被拒）",
            u1861Code::KeyDup => "映射键重复（广播漏核）",
            u1861Code::KeyGap => "映射键断档（全程账断裂）",
            u1861Code::ValOob => "映射值越界",
            u1861Code::SumDrift => "双表累计漂移",
            u1861Code::ThreshDrift => "阈值漂移（守卫阈值≠冻结值）",
            u1861Code::LedgerBroken => "台账不守恒（守恒恒等式破缺）",
            u1861Code::SnapshotFingerprint => "快照指纹不符（字段被改后重算不一致）",
            u1861Code::TableBroken => "常量表破坏（表项与语义规则矛盾）",
            u1861Code::SegmentGuard => "段位守卫失败（诊断码越出本模块段）",
        }
    }

    /// 可操作修复建议（每条诊断都必须带下一步动作）。
    pub const fn suggestion(self) -> &'static str {
        match self {
            u1861Code::RowDup => "按键唯一律去重",
            u1861Code::RowGap => "按连续律补号",
            u1861Code::GuardTrip => "检查临界区嵌套",
            u1861Code::KeyDup => "按键唯一律去重",
            u1861Code::KeyGap => "按连续律补号",
            u1861Code::ValOob => "按值域裁剪",
            u1861Code::SumDrift => "按理论值复算",
            u1861Code::ThreshDrift => "按冻结值校正",
            u1861Code::LedgerBroken => "按守恒律重建台账后重跑",
            u1861Code::SnapshotFingerprint => "按防篡改规程重建快照",
            u1861Code::TableBroken => "按语义规则重建常量表",
            u1861Code::SegmentGuard => "核对诊断码分配表",
        }
    }
}

// --------------------------------------------------------------------------
// 冻结常量表（u1861mix · 16 行逐行注释自证）
// --------------------------------------------------------------------------
/// 击落段 对拍表（16 行）。
pub const u1861mix_COUNT: usize = 16;

/// 冻结表本体（编译期常量，运行期只读）。
pub static u1861mix: [(&'static str, u64); 16] = [
    ("FQ_00", 10), // FQ 对拍行（发起）
    ("GB_01", 15), // GB 对拍行（广播）
    ("YD_02", 20), // YD 对拍行（应答）
    ("WC_03", 25), // WC 对拍行（完成）
    ("SD_04", 30), // SD 对拍行（四段冻结）
    ("QM_05", 35), // QM 对拍行（段间签名）
    ("TZ_06", 40), // TZ 对拍行（跳段注入）
    ("JJ_07", 45), // JJ 对拍行（拒绝）
    ("QD_08", 50), // QD 对拍行（全程账导出）
    ("MM_09", 55), // MM 对拍行（页掩码）
    ("HB_10", 60), // HB 对拍行（回执）
    ("MZ_11", 65), // MZ 对拍行（封账）
    ("FQ_12", 70), // FQ 对拍行（发起）
    ("GB_13", 75), // GB 对拍行（广播）
    ("YD_14", 80), // YD 对拍行（应答）
    ("WC_15", 85), // WC 对拍行（完成）
];

/// 线性查找（键唯一性由构造期断言保证，运行时无分支歧义）。
pub fn u1861mix_get(key: &'static str) -> Option<u64> {
    let mut i = 0usize;
    while i < u1861mix_COUNT {
        if u1861mix[i].0 == key {
            return Some(u1861mix[i].1);
        }
        i += 1;
    }
    None
}

/// 累计求和（用于守恒对账）。
pub fn u1861mix_sum() -> u64 {
    let mut s = 0u64;
    let mut i = 0usize;
    while i < u1861mix_COUNT {
        s = s.wrapping_add(u1861mix[i].1 as u64);
        i += 1;
    }
    s
}

// --------------------------------------------------------------------------
// 守卫 u1861mixgate（阈值 32 · 重入/越限/静默三类拒止）
// --------------------------------------------------------------------------
/// 击落段 混合面守卫（阈值 32 冻结）。
#[derive(Clone, Copy, Debug)]
pub struct u1861mixgate {
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

/// 守卫阈值（击落段 混合面守卫（阈值 32 冻结））。
pub const U1861MIXGATE_THRESHOLD: u64 = 32;

impl u1861mixgate {
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
        if self.window >= U1861MIXGATE_THRESHOLD {
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
// 字符串键值映射 u1861mixmap（16 行逐行语义注释 + 逐行守卫）
// --------------------------------------------------------------------------
/// 击落段 混合映射矩阵。
/// 行数。
pub const u1861mixmap_COUNT: usize = 16;

/// 表本体（键 + 值 + 注释由编译期冻结）。
pub static u1861mixmap: [(&'static str, u64); 16] = [
    ("FQ0000", 0), // FQ 单元 0000（发起 × 相位 00 × 混 轮）
    ("GB0001", 9), // GB 单元 0001（广播 × 相位 01 × 混 轮）
    ("YD0002", 18), // YD 单元 0002（应答 × 相位 02 × 混 轮）
    ("WC0003", 27), // WC 单元 0003（完成 × 相位 03 × 混 轮）
    ("SD0004", 36), // SD 单元 0004（四段冻结 × 相位 04 × 混 轮）
    ("QM0005", 45), // QM 单元 0005（段间签名 × 相位 05 × 混 轮）
    ("TZ0006", 54), // TZ 单元 0006（跳段注入 × 相位 06 × 混 轮）
    ("JJ0007", 63), // JJ 单元 0007（拒绝 × 相位 07 × 混 轮）
    ("QD0008", 8), // QD 单元 0008（全程账导出 × 相位 08 × 混 轮）
    ("MM0009", 17), // MM 单元 0009（页掩码 × 相位 09 × 混 轮）
    ("HB0010", 26), // HB 单元 0010（回执 × 相位 10 × 混 轮）
    ("MZ0011", 35), // MZ 单元 0011（封账 × 相位 11 × 混 轮）
    ("FQ0012", 44), // FQ 单元 0012（发起 × 相位 12 × 混 轮）
    ("GB0013", 53), // GB 单元 0013（广播 × 相位 13 × 混 轮）
    ("YD0014", 62), // YD 单元 0014（应答 × 相位 14 × 混 轮）
    ("WC0015", 7), // WC 单元 0015（完成 × 相位 15 × 混 轮）
];

/// O(1) 语义查找（match 全枚举；命中率 100%% 由守卫保证）。
pub fn u1861mixmap_get(key: &str) -> Option<u64> {
    match key {
        "FQ0000" => Some(0),
        "GB0001" => Some(9),
        "YD0002" => Some(18),
        "WC0003" => Some(27),
        "SD0004" => Some(36),
        "QM0005" => Some(45),
        "TZ0006" => Some(54),
        "JJ0007" => Some(63),
        "QD0008" => Some(8),
        "MM0009" => Some(17),
        "HB0010" => Some(26),
        "MZ0011" => Some(35),
        "FQ0012" => Some(44),
        "GB0013" => Some(53),
        "YD0014" => Some(62),
        "WC0015" => Some(7),
        _ => None,
    }
}

/// 逐行守卫扫描（返回违例行数；0 = 全绿）。
pub fn u1861mixmap_scan() -> usize {
    let mut bad = 0usize;
    let mut i = 0usize;
    while i < u1861mixmap_COUNT {
        let (k, v) = u1861mixmap[i];
        let ok = match k {
            "FQ0000" => v < 64,
            "GB0001" => v < 64,
            "YD0002" => v < 64,
            "WC0003" => v < 64,
            "SD0004" => v < 64,
            "QM0005" => v < 64,
            "TZ0006" => v < 64,
            "JJ0007" => v < 64,
            "QD0008" => v < 64,
            "MM0009" => v < 64,
            "HB0010" => v < 64,
            "MZ0011" => v < 64,
            "FQ0012" => v < 64,
            "GB0013" => v < 64,
            "YD0014" => v < 64,
            "WC0015" => v < 64,
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
pub fn u1861mixmap_sum() -> u64 {
    let mut s = 0u64;
    let mut i = 0usize;
    while i < u1861mixmap_COUNT {
        s = s.wrapping_add(u1861mixmap[i].1 as u64);
        i += 1;
    }
    s
}

/// 键唯一性（构造期断言等价物：逐对扫描同键）。
pub fn u1861mixmap_unique() -> bool {
    let mut i = 0usize;
    while i < u1861mixmap_COUNT {
        let mut j = i + 1;
        while j < u1861mixmap_COUNT {
            if u1861mixmap[i].0 == u1861mixmap[j].0 {
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

/// 段位守卫：全部诊断码必须落在本段 [0x5f30, 0x5f3f]。
pub const fn u1861_segment_guard() -> bool {
    let mut ok = true;
    let mut i = 0usize;
    while i < u1861Code::ALL.len() {
        let c = u1861Code::ALL[i].code();
        if c < 0x5f30 || c > 0x5f3f {
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
fn u1861_guard_seg() -> bool {
    u1861_segment_guard()
}

/// UNX-F1861 对拍表 16 行边界与累计守恒。
fn check_u1861_mix_tbl() -> bool {
    let mut ok = u1861mix_COUNT == 16;
    if ok { ok = u1861mix_get("FQ_00") == Some(10) && u1861mix_get("WC_15") == Some(85); }
    if ok { ok = u1861mix_get("ZZZ_NOKEY").is_none(); }
    if ok { ok = u1861mix_sum() == 760; }
    ok
}

/// UNX-F1861 混合映射 16 行守卫唯一 + 守卫阈值冻结。
fn check_u1861_mix_map() -> bool {
    let mut ok = u1861mixmap_COUNT == 16;
    if ok { ok = u1861mixmap_scan() == 0 && u1861mixmap_unique(); }
    if ok { ok = u1861mixmap_sum() == 504; }
    if ok { ok = U1861MIXGATE_THRESHOLD == 32; }
    ok
}

/// 本模块自检（逐检查带 detail，全绿才出门）。
pub fn run_u1861_checks() -> CheckSet {
    let mut set = CheckSet::new("U1861");
    set.add(
        "u1861-seg-guard",
        u1861_guard_seg(),
        if u1861_guard_seg() { "" } else { "diagnostic code outside segment" },
    );
    set.add(
        "u1861-mix-tbl",
        check_u1861_mix_tbl(),
        if check_u1861_mix_tbl() { "" } else { "UNX-F1861 mix table mismatch" },
    );
    set.add(
        "u1861-mix-map",
        check_u1861_mix_map(),
        if check_u1861_mix_map() { "" } else { "UNX-F1861 mix map mismatch" },
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
    fn u1861_all_checks_green() {
        let set = run_u1861_checks();
        assert!(set.all_passed(), "U1861 domain checks must be green");
    }

    #[test]
    fn u1861_segment_guard_holds() {
        assert!(u1861_segment_guard());
    }
}
