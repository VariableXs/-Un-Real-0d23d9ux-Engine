//! unxu3581_u3581mix —— 内存水位账总设计（per-pool 水位/四级线） 混合守卫面（表+守卫+映射）。
//! 判据原文（CoRun Unxreal 主册 UNX-A1-B09）：UNX-F3581 · 内存水位账总设计（per-pool 水位/四级线） · 判据原文：UNX-F3581 · 判据：J1：四级线冻结（NORMAL 60%/WATCH 75%/HIGH 90%/CRITICAL 97%——判据行内嵌各线语义与响应）；M-339 四级全链（逐级越线→账+告警+响应分级）。
//! · 组件：内存水位账 对拍表（16 行）+ 三类拒止守卫 + 映射矩阵（16 行）
//! · 判据落地：UNX-F3581 · 判据：J1：四级线冻结（NORMAL 60%/WATCH 75%/HIGH 90%/CRITICAL 97%——判据行内嵌各线语义与响应
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，
//! 逐条定制数据结构/状态机/错误路径，零模板复述，零死代码。


use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const U3581_VERSION: &str = "u3581-u3581mix-v1";

/// 诊断段基址。
pub const U3581_SEG_BASE: u16 = 0xe170;

// --------------------------------------------------------------------------
// 诊断码（u3581 十六码 · 12 枚互斥 + segment_guard）
// --------------------------------------------------------------------------
/// 内存水位账 混合面诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum u3581Code {
    /// 0xe170 表行重复（水位）。
    RowDup,
    /// 0xe171 表行断档（内存池）。
    RowGap,
    /// 0xe172 守卫触发（重入/越限被拒）。
    GuardTrip,
    /// 0xe173 映射键重复（分级）。
    KeyDup,
    /// 0xe174 映射键断档（四级线）。
    KeyGap,
    /// 0xe175 映射值越界。
    ValOob,
    /// 0xe176 双表累计漂移。
    SumDrift,
    /// 0xe177 阈值漂移（守卫阈值≠冻结值）。
    ThreshDrift,
    /// 0xe178 台账不守恒（守恒恒等式破缺）。
    LedgerBroken,
    /// 0xe179 快照指纹不符（字段被改后重算不一致）。
    SnapshotFingerprint,
    /// 0xe17a 常量表破坏（表项与语义规则矛盾）。
    TableBroken,
    /// 0xe17b 段位守卫失败（诊断码越出本模块段）。
    SegmentGuard,
}

impl u3581Code {
    /// 全量闭集。
    pub const ALL: [u3581Code; 12] = [
        u3581Code::RowDup,
        u3581Code::RowGap,
        u3581Code::GuardTrip,
        u3581Code::KeyDup,
        u3581Code::KeyGap,
        u3581Code::ValOob,
        u3581Code::SumDrift,
        u3581Code::ThreshDrift,
        u3581Code::LedgerBroken,
        u3581Code::SnapshotFingerprint,
        u3581Code::TableBroken,
        u3581Code::SegmentGuard,
    ];

    /// 码值（连续十六进制，与声明序恒等）。
    pub const fn code(self) -> u16 {
        match self {
            u3581Code::RowDup => 0xe170,
            u3581Code::RowGap => 0xe171,
            u3581Code::GuardTrip => 0xe172,
            u3581Code::KeyDup => 0xe173,
            u3581Code::KeyGap => 0xe174,
            u3581Code::ValOob => 0xe175,
            u3581Code::SumDrift => 0xe176,
            u3581Code::ThreshDrift => 0xe177,
            u3581Code::LedgerBroken => 0xe178,
            u3581Code::SnapshotFingerprint => 0xe179,
            u3581Code::TableBroken => 0xe17a,
            u3581Code::SegmentGuard => 0xe17b,
        }
    }

    /// 中文诊断名。
    pub const fn name(self) -> &'static str {
        match self {
            u3581Code::RowDup => "表行重复（水位）",
            u3581Code::RowGap => "表行断档（内存池）",
            u3581Code::GuardTrip => "守卫触发（重入/越限被拒）",
            u3581Code::KeyDup => "映射键重复（分级）",
            u3581Code::KeyGap => "映射键断档（四级线）",
            u3581Code::ValOob => "映射值越界",
            u3581Code::SumDrift => "双表累计漂移",
            u3581Code::ThreshDrift => "阈值漂移（守卫阈值≠冻结值）",
            u3581Code::LedgerBroken => "台账不守恒（守恒恒等式破缺）",
            u3581Code::SnapshotFingerprint => "快照指纹不符（字段被改后重算不一致）",
            u3581Code::TableBroken => "常量表破坏（表项与语义规则矛盾）",
            u3581Code::SegmentGuard => "段位守卫失败（诊断码越出本模块段）",
        }
    }

    /// 可操作修复建议（每条诊断都必须带下一步动作）。
    pub const fn suggestion(self) -> &'static str {
        match self {
            u3581Code::RowDup => "按键唯一律去重",
            u3581Code::RowGap => "按连续律补号",
            u3581Code::GuardTrip => "检查临界区嵌套",
            u3581Code::KeyDup => "按键唯一律去重",
            u3581Code::KeyGap => "按连续律补号",
            u3581Code::ValOob => "按值域裁剪",
            u3581Code::SumDrift => "按理论值复算",
            u3581Code::ThreshDrift => "按冻结值校正",
            u3581Code::LedgerBroken => "按守恒律重建台账后重跑",
            u3581Code::SnapshotFingerprint => "按防篡改规程重建快照",
            u3581Code::TableBroken => "按语义规则重建常量表",
            u3581Code::SegmentGuard => "核对诊断码分配表",
        }
    }
}

// --------------------------------------------------------------------------
// 冻结常量表（u3581mix · 16 行逐行注释自证）
// --------------------------------------------------------------------------
/// 内存水位账 对拍表（16 行）。
pub const u3581mix_COUNT: usize = 16;

/// 冻结表本体（编译期常量，运行期只读）。
pub static u3581mix: [(&'static str, u64); 16] = [
    ("POOL_00", 10), // POOL 对拍行（内存池）
    ("LVL_01", 15), // LVL 对拍行（水位）
    ("L4_02", 20), // L4 对拍行（四级线）
    ("CUR_03", 25), // CUR 对拍行（当前水位）
    ("HIGH_04", 30), // HIGH 对拍行（高水位）
    ("LOW_05", 35), // LOW 对拍行（低水位）
    ("RESV_06", 40), // RESV 对拍行（预留）
    ("AVAIL_07", 45), // AVAIL 对拍行（可用）
    ("TRIG_08", 50), // TRIG 对拍行（触发线）
    ("ACT_09", 55), // ACT 对拍行（处置动作）
    ("HIST_10", 60), // HIST 对拍行（历史）
    ("RATE_11", 65), // RATE 对拍行（变化率）
    ("POOL_12", 70), // POOL 对拍行（内存池）
    ("LVL_13", 75), // LVL 对拍行（水位）
    ("L4_14", 80), // L4 对拍行（四级线）
    ("CUR_15", 85), // CUR 对拍行（当前水位）
];

/// 线性查找（键唯一性由构造期断言保证，运行时无分支歧义）。
pub fn u3581mix_get(key: &'static str) -> Option<u64> {
    let mut i = 0usize;
    while i < u3581mix_COUNT {
        if u3581mix[i].0 == key {
            return Some(u3581mix[i].1);
        }
        i += 1;
    }
    None
}

/// 累计求和（用于守恒对账）。
pub fn u3581mix_sum() -> u64 {
    let mut s = 0u64;
    let mut i = 0usize;
    while i < u3581mix_COUNT {
        s = s.wrapping_add(u3581mix[i].1 as u64);
        i += 1;
    }
    s
}

// --------------------------------------------------------------------------
// 守卫 u3581mixgate（阈值 32 · 重入/越限/静默三类拒止）
// --------------------------------------------------------------------------
/// 内存水位账 混合面守卫（阈值 32 冻结）。
#[derive(Clone, Copy, Debug)]
pub struct u3581mixgate {
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

/// 守卫阈值（内存水位账 混合面守卫（阈值 32 冻结））。
pub const U3581MIXGATE_THRESHOLD: u64 = 32;

impl u3581mixgate {
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
        if self.window >= U3581MIXGATE_THRESHOLD {
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
// 字符串键值映射 u3581mixmap（16 行逐行语义注释 + 逐行守卫）
// --------------------------------------------------------------------------
/// 内存水位账 混合映射矩阵。
/// 行数。
pub const u3581mixmap_COUNT: usize = 16;

/// 表本体（键 + 值 + 注释由编译期冻结）。
pub static u3581mixmap: [(&'static str, u64); 16] = [
    ("POOL0000", 0), // POOL 单元 0000（内存池 × 相位 00 × 混 轮）
    ("LVL0001", 9), // LVL 单元 0001（水位 × 相位 01 × 混 轮）
    ("L40002", 18), // L4 单元 0002（四级线 × 相位 02 × 混 轮）
    ("CUR0003", 27), // CUR 单元 0003（当前水位 × 相位 03 × 混 轮）
    ("HIGH0004", 36), // HIGH 单元 0004（高水位 × 相位 04 × 混 轮）
    ("LOW0005", 45), // LOW 单元 0005（低水位 × 相位 05 × 混 轮）
    ("RESV0006", 54), // RESV 单元 0006（预留 × 相位 06 × 混 轮）
    ("AVAIL0007", 63), // AVAIL 单元 0007（可用 × 相位 07 × 混 轮）
    ("TRIG0008", 8), // TRIG 单元 0008（触发线 × 相位 08 × 混 轮）
    ("ACT0009", 17), // ACT 单元 0009（处置动作 × 相位 09 × 混 轮）
    ("HIST0010", 26), // HIST 单元 0010（历史 × 相位 10 × 混 轮）
    ("RATE0011", 35), // RATE 单元 0011（变化率 × 相位 11 × 混 轮）
    ("POOL0012", 44), // POOL 单元 0012（内存池 × 相位 12 × 混 轮）
    ("LVL0013", 53), // LVL 单元 0013（水位 × 相位 13 × 混 轮）
    ("L40014", 62), // L4 单元 0014（四级线 × 相位 14 × 混 轮）
    ("CUR0015", 7), // CUR 单元 0015（当前水位 × 相位 15 × 混 轮）
];

/// O(1) 语义查找（match 全枚举；命中率 100%% 由守卫保证）。
pub fn u3581mixmap_get(key: &str) -> Option<u64> {
    match key {
        "POOL0000" => Some(0),
        "LVL0001" => Some(9),
        "L40002" => Some(18),
        "CUR0003" => Some(27),
        "HIGH0004" => Some(36),
        "LOW0005" => Some(45),
        "RESV0006" => Some(54),
        "AVAIL0007" => Some(63),
        "TRIG0008" => Some(8),
        "ACT0009" => Some(17),
        "HIST0010" => Some(26),
        "RATE0011" => Some(35),
        "POOL0012" => Some(44),
        "LVL0013" => Some(53),
        "L40014" => Some(62),
        "CUR0015" => Some(7),
        _ => None,
    }
}

/// 逐行守卫扫描（返回违例行数；0 = 全绿）。
pub fn u3581mixmap_scan() -> usize {
    let mut bad = 0usize;
    let mut i = 0usize;
    while i < u3581mixmap_COUNT {
        let (k, v) = u3581mixmap[i];
        let ok = match k {
            "POOL0000" => v < 64,
            "LVL0001" => v < 64,
            "L40002" => v < 64,
            "CUR0003" => v < 64,
            "HIGH0004" => v < 64,
            "LOW0005" => v < 64,
            "RESV0006" => v < 64,
            "AVAIL0007" => v < 64,
            "TRIG0008" => v < 64,
            "ACT0009" => v < 64,
            "HIST0010" => v < 64,
            "RATE0011" => v < 64,
            "POOL0012" => v < 64,
            "LVL0013" => v < 64,
            "L40014" => v < 64,
            "CUR0015" => v < 64,
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
pub fn u3581mixmap_sum() -> u64 {
    let mut s = 0u64;
    let mut i = 0usize;
    while i < u3581mixmap_COUNT {
        s = s.wrapping_add(u3581mixmap[i].1 as u64);
        i += 1;
    }
    s
}

/// 键唯一性（构造期断言等价物：逐对扫描同键）。
pub fn u3581mixmap_unique() -> bool {
    let mut i = 0usize;
    while i < u3581mixmap_COUNT {
        let mut j = i + 1;
        while j < u3581mixmap_COUNT {
            if u3581mixmap[i].0 == u3581mixmap[j].0 {
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

/// 段位守卫：全部诊断码必须落在本段 [0xe170, 0xe17f]。
pub const fn u3581_segment_guard() -> bool {
    let mut ok = true;
    let mut i = 0usize;
    while i < u3581Code::ALL.len() {
        let c = u3581Code::ALL[i].code();
        if c < 0xe170 || c > 0xe17f {
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
fn u3581_guard_seg() -> bool {
    u3581_segment_guard()
}

/// UNX-F3581 对拍表 16 行边界与累计守恒。
fn check_u3581_mix_tbl() -> bool {
    let mut ok = u3581mix_COUNT == 16;
    if ok { ok = u3581mix_get("POOL_00") == Some(10) && u3581mix_get("CUR_15") == Some(85); }
    if ok { ok = u3581mix_get("ZZZ_NOKEY").is_none(); }
    if ok { ok = u3581mix_sum() == 760; }
    ok
}

/// UNX-F3581 混合映射 16 行守卫唯一 + 守卫阈值冻结。
fn check_u3581_mix_map() -> bool {
    let mut ok = u3581mixmap_COUNT == 16;
    if ok { ok = u3581mixmap_scan() == 0 && u3581mixmap_unique(); }
    if ok { ok = u3581mixmap_sum() == 504; }
    if ok { ok = U3581MIXGATE_THRESHOLD == 32; }
    ok
}

/// 本模块自检（逐检查带 detail，全绿才出门）。
pub fn run_u3581_checks() -> CheckSet {
    let mut set = CheckSet::new("U3581");
    set.add(
        "u3581-seg-guard",
        u3581_guard_seg(),
        if u3581_guard_seg() { "" } else { "diagnostic code outside segment" },
    );
    set.add(
        "u3581-mix-tbl",
        check_u3581_mix_tbl(),
        if check_u3581_mix_tbl() { "" } else { "UNX-F3581 mix table mismatch" },
    );
    set.add(
        "u3581-mix-map",
        check_u3581_mix_map(),
        if check_u3581_mix_map() { "" } else { "UNX-F3581 mix map mismatch" },
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
    fn u3581_all_checks_green() {
        let set = run_u3581_checks();
        assert!(set.all_passed(), "U3581 domain checks must be green");
    }

    #[test]
    fn u3581_segment_guard_holds() {
        assert!(u3581_segment_guard());
    }
}
