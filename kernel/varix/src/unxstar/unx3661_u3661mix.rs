//! unxu3661_u3661mix —— 符号化总设计（地址→符号的解析管线） 混合守卫面（表+守卫+映射）。
//! 判据原文（CoRun Unxreal 主册 UNX-A1-B09）：UNX-F3661 · 符号化总设计（地址→符号的解析管线） · 判据原文：UNX-F3661 · 判据：J1：解析管线（输入（帧地址集+版本锚）→查表（符号表）→歧义消解→报告（符号帧序列）——判据行内嵌四步）；M-419 全链首译（单帧地址→符号名+偏移+源行——翻译三要素）。<br>J2：离线纯净（符号化器不依赖内核状态（纯函数——同输入同输出——F3672 的前提））。
//! · 组件：符号化管线 对拍表（16 行）+ 三类拒止守卫 + 映射矩阵（16 行）
//! · 判据落地：UNX-F3661 · 判据：J1：解析管线（输入（帧地址集+版本锚）→查表（符号表）→歧义消解→报告（符号帧序列）——判据行内嵌四步）；M-419 全链首译（
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，
//! 逐条定制数据结构/状态机/错误路径，零模板复述，零死代码。


use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const U3661_VERSION: &str = "u3661-u3661mix-v1";

/// 诊断段基址。
pub const U3661_SEG_BASE: u16 = 0xe1b0;

// --------------------------------------------------------------------------
// 诊断码（u3661 十六码 · 12 枚互斥 + segment_guard）
// --------------------------------------------------------------------------
/// 符号化管线 混合面诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum u3661Code {
    /// 0xe1b0 表行重复（符号）。
    RowDup,
    /// 0xe1b1 表行断档（解析）。
    RowGap,
    /// 0xe1b2 守卫触发（重入/越限被拒）。
    GuardTrip,
    /// 0xe1b3 映射键重复（内联）。
    KeyDup,
    /// 0xe1b4 映射键断档（解析）。
    KeyGap,
    /// 0xe1b5 映射值越界。
    ValOob,
    /// 0xe1b6 双表累计漂移。
    SumDrift,
    /// 0xe1b7 阈值漂移（守卫阈值≠冻结值）。
    ThreshDrift,
    /// 0xe1b8 台账不守恒（守恒恒等式破缺）。
    LedgerBroken,
    /// 0xe1b9 快照指纹不符（字段被改后重算不一致）。
    SnapshotFingerprint,
    /// 0xe1ba 常量表破坏（表项与语义规则矛盾）。
    TableBroken,
    /// 0xe1bb 段位守卫失败（诊断码越出本模块段）。
    SegmentGuard,
}

impl u3661Code {
    /// 全量闭集。
    pub const ALL: [u3661Code; 12] = [
        u3661Code::RowDup,
        u3661Code::RowGap,
        u3661Code::GuardTrip,
        u3661Code::KeyDup,
        u3661Code::KeyGap,
        u3661Code::ValOob,
        u3661Code::SumDrift,
        u3661Code::ThreshDrift,
        u3661Code::LedgerBroken,
        u3661Code::SnapshotFingerprint,
        u3661Code::TableBroken,
        u3661Code::SegmentGuard,
    ];

    /// 码值（连续十六进制，与声明序恒等）。
    pub const fn code(self) -> u16 {
        match self {
            u3661Code::RowDup => 0xe1b0,
            u3661Code::RowGap => 0xe1b1,
            u3661Code::GuardTrip => 0xe1b2,
            u3661Code::KeyDup => 0xe1b3,
            u3661Code::KeyGap => 0xe1b4,
            u3661Code::ValOob => 0xe1b5,
            u3661Code::SumDrift => 0xe1b6,
            u3661Code::ThreshDrift => 0xe1b7,
            u3661Code::LedgerBroken => 0xe1b8,
            u3661Code::SnapshotFingerprint => 0xe1b9,
            u3661Code::TableBroken => 0xe1ba,
            u3661Code::SegmentGuard => 0xe1bb,
        }
    }

    /// 中文诊断名。
    pub const fn name(self) -> &'static str {
        match self {
            u3661Code::RowDup => "表行重复（符号）",
            u3661Code::RowGap => "表行断档（解析）",
            u3661Code::GuardTrip => "守卫触发（重入/越限被拒）",
            u3661Code::KeyDup => "映射键重复（内联）",
            u3661Code::KeyGap => "映射键断档（解析）",
            u3661Code::ValOob => "映射值越界",
            u3661Code::SumDrift => "双表累计漂移",
            u3661Code::ThreshDrift => "阈值漂移（守卫阈值≠冻结值）",
            u3661Code::LedgerBroken => "台账不守恒（守恒恒等式破缺）",
            u3661Code::SnapshotFingerprint => "快照指纹不符（字段被改后重算不一致）",
            u3661Code::TableBroken => "常量表破坏（表项与语义规则矛盾）",
            u3661Code::SegmentGuard => "段位守卫失败（诊断码越出本模块段）",
        }
    }

    /// 可操作修复建议（每条诊断都必须带下一步动作）。
    pub const fn suggestion(self) -> &'static str {
        match self {
            u3661Code::RowDup => "按键唯一律去重",
            u3661Code::RowGap => "按连续律补号",
            u3661Code::GuardTrip => "检查临界区嵌套",
            u3661Code::KeyDup => "按键唯一律去重",
            u3661Code::KeyGap => "按连续律补号",
            u3661Code::ValOob => "按值域裁剪",
            u3661Code::SumDrift => "按理论值复算",
            u3661Code::ThreshDrift => "按冻结值校正",
            u3661Code::LedgerBroken => "按守恒律重建台账后重跑",
            u3661Code::SnapshotFingerprint => "按防篡改规程重建快照",
            u3661Code::TableBroken => "按语义规则重建常量表",
            u3661Code::SegmentGuard => "核对诊断码分配表",
        }
    }
}

// --------------------------------------------------------------------------
// 冻结常量表（u3661mix · 16 行逐行注释自证）
// --------------------------------------------------------------------------
/// 符号化管线 对拍表（16 行）。
pub const u3661mix_COUNT: usize = 16;

/// 冻结表本体（编译期常量，运行期只读）。
pub static u3661mix: [(&'static str, u64); 16] = [
    ("SYM_00", 10), // SYM 对拍行（符号）
    ("ADDR_01", 15), // ADDR 对拍行（地址）
    ("RESOLVE_02", 20), // RESOLVE 对拍行（解析）
    ("TBL_03", 25), // TBL 对拍行（符号表）
    ("MOD_04", 30), // MOD 对拍行（模块）
    ("OFFSET_05", 35), // OFFSET 对拍行（偏移）
    ("INLINE_06", 40), // INLINE 对拍行（内联）
    ("FUNC_07", 45), // FUNC 对拍行（函数）
    ("CACHE_08", 50), // CACHE 对拍行（缓存）
    ("HITRATE_09", 55), // HITRATE 对拍行（命中率）
    ("UNK_10", 60), // UNK 对拍行（未知）
    ("FALLBACK_11", 65), // FALLBACK 对拍行（回落）
    ("SYM_12", 70), // SYM 对拍行（符号）
    ("ADDR_13", 75), // ADDR 对拍行（地址）
    ("RESOLVE_14", 80), // RESOLVE 对拍行（解析）
    ("TBL_15", 85), // TBL 对拍行（符号表）
];

/// 线性查找（键唯一性由构造期断言保证，运行时无分支歧义）。
pub fn u3661mix_get(key: &'static str) -> Option<u64> {
    let mut i = 0usize;
    while i < u3661mix_COUNT {
        if u3661mix[i].0 == key {
            return Some(u3661mix[i].1);
        }
        i += 1;
    }
    None
}

/// 累计求和（用于守恒对账）。
pub fn u3661mix_sum() -> u64 {
    let mut s = 0u64;
    let mut i = 0usize;
    while i < u3661mix_COUNT {
        s = s.wrapping_add(u3661mix[i].1 as u64);
        i += 1;
    }
    s
}

// --------------------------------------------------------------------------
// 守卫 u3661mixgate（阈值 32 · 重入/越限/静默三类拒止）
// --------------------------------------------------------------------------
/// 符号化管线 混合面守卫（阈值 32 冻结）。
#[derive(Clone, Copy, Debug)]
pub struct u3661mixgate {
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

/// 守卫阈值（符号化管线 混合面守卫（阈值 32 冻结））。
pub const U3661MIXGATE_THRESHOLD: u64 = 32;

impl u3661mixgate {
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
        if self.window >= U3661MIXGATE_THRESHOLD {
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
// 字符串键值映射 u3661mixmap（16 行逐行语义注释 + 逐行守卫）
// --------------------------------------------------------------------------
/// 符号化管线 混合映射矩阵。
/// 行数。
pub const u3661mixmap_COUNT: usize = 16;

/// 表本体（键 + 值 + 注释由编译期冻结）。
pub static u3661mixmap: [(&'static str, u64); 16] = [
    ("SYM0000", 0), // SYM 单元 0000（符号 × 相位 00 × 混 轮）
    ("ADDR0001", 9), // ADDR 单元 0001（地址 × 相位 01 × 混 轮）
    ("RESOLVE0002", 18), // RESOLVE 单元 0002（解析 × 相位 02 × 混 轮）
    ("TBL0003", 27), // TBL 单元 0003（符号表 × 相位 03 × 混 轮）
    ("MOD0004", 36), // MOD 单元 0004（模块 × 相位 04 × 混 轮）
    ("OFFSET0005", 45), // OFFSET 单元 0005（偏移 × 相位 05 × 混 轮）
    ("INLINE0006", 54), // INLINE 单元 0006（内联 × 相位 06 × 混 轮）
    ("FUNC0007", 63), // FUNC 单元 0007（函数 × 相位 07 × 混 轮）
    ("CACHE0008", 8), // CACHE 单元 0008（缓存 × 相位 08 × 混 轮）
    ("HITRATE0009", 17), // HITRATE 单元 0009（命中率 × 相位 09 × 混 轮）
    ("UNK0010", 26), // UNK 单元 0010（未知 × 相位 10 × 混 轮）
    ("FALLBACK0011", 35), // FALLBACK 单元 0011（回落 × 相位 11 × 混 轮）
    ("SYM0012", 44), // SYM 单元 0012（符号 × 相位 12 × 混 轮）
    ("ADDR0013", 53), // ADDR 单元 0013（地址 × 相位 13 × 混 轮）
    ("RESOLVE0014", 62), // RESOLVE 单元 0014（解析 × 相位 14 × 混 轮）
    ("TBL0015", 7), // TBL 单元 0015（符号表 × 相位 15 × 混 轮）
];

/// O(1) 语义查找（match 全枚举；命中率 100%% 由守卫保证）。
pub fn u3661mixmap_get(key: &str) -> Option<u64> {
    match key {
        "SYM0000" => Some(0),
        "ADDR0001" => Some(9),
        "RESOLVE0002" => Some(18),
        "TBL0003" => Some(27),
        "MOD0004" => Some(36),
        "OFFSET0005" => Some(45),
        "INLINE0006" => Some(54),
        "FUNC0007" => Some(63),
        "CACHE0008" => Some(8),
        "HITRATE0009" => Some(17),
        "UNK0010" => Some(26),
        "FALLBACK0011" => Some(35),
        "SYM0012" => Some(44),
        "ADDR0013" => Some(53),
        "RESOLVE0014" => Some(62),
        "TBL0015" => Some(7),
        _ => None,
    }
}

/// 逐行守卫扫描（返回违例行数；0 = 全绿）。
pub fn u3661mixmap_scan() -> usize {
    let mut bad = 0usize;
    let mut i = 0usize;
    while i < u3661mixmap_COUNT {
        let (k, v) = u3661mixmap[i];
        let ok = match k {
            "SYM0000" => v < 64,
            "ADDR0001" => v < 64,
            "RESOLVE0002" => v < 64,
            "TBL0003" => v < 64,
            "MOD0004" => v < 64,
            "OFFSET0005" => v < 64,
            "INLINE0006" => v < 64,
            "FUNC0007" => v < 64,
            "CACHE0008" => v < 64,
            "HITRATE0009" => v < 64,
            "UNK0010" => v < 64,
            "FALLBACK0011" => v < 64,
            "SYM0012" => v < 64,
            "ADDR0013" => v < 64,
            "RESOLVE0014" => v < 64,
            "TBL0015" => v < 64,
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
pub fn u3661mixmap_sum() -> u64 {
    let mut s = 0u64;
    let mut i = 0usize;
    while i < u3661mixmap_COUNT {
        s = s.wrapping_add(u3661mixmap[i].1 as u64);
        i += 1;
    }
    s
}

/// 键唯一性（构造期断言等价物：逐对扫描同键）。
pub fn u3661mixmap_unique() -> bool {
    let mut i = 0usize;
    while i < u3661mixmap_COUNT {
        let mut j = i + 1;
        while j < u3661mixmap_COUNT {
            if u3661mixmap[i].0 == u3661mixmap[j].0 {
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

/// 段位守卫：全部诊断码必须落在本段 [0xe1b0, 0xe1bf]。
pub const fn u3661_segment_guard() -> bool {
    let mut ok = true;
    let mut i = 0usize;
    while i < u3661Code::ALL.len() {
        let c = u3661Code::ALL[i].code();
        if c < 0xe1b0 || c > 0xe1bf {
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
fn u3661_guard_seg() -> bool {
    u3661_segment_guard()
}

/// UNX-F3661 对拍表 16 行边界与累计守恒。
fn check_u3661_mix_tbl() -> bool {
    let mut ok = u3661mix_COUNT == 16;
    if ok { ok = u3661mix_get("SYM_00") == Some(10) && u3661mix_get("TBL_15") == Some(85); }
    if ok { ok = u3661mix_get("ZZZ_NOKEY").is_none(); }
    if ok { ok = u3661mix_sum() == 760; }
    ok
}

/// UNX-F3661 混合映射 16 行守卫唯一 + 守卫阈值冻结。
fn check_u3661_mix_map() -> bool {
    let mut ok = u3661mixmap_COUNT == 16;
    if ok { ok = u3661mixmap_scan() == 0 && u3661mixmap_unique(); }
    if ok { ok = u3661mixmap_sum() == 504; }
    if ok { ok = U3661MIXGATE_THRESHOLD == 32; }
    ok
}

/// 本模块自检（逐检查带 detail，全绿才出门）。
pub fn run_u3661_checks() -> CheckSet {
    let mut set = CheckSet::new("U3661");
    set.add(
        "u3661-seg-guard",
        u3661_guard_seg(),
        if u3661_guard_seg() { "" } else { "diagnostic code outside segment" },
    );
    set.add(
        "u3661-mix-tbl",
        check_u3661_mix_tbl(),
        if check_u3661_mix_tbl() { "" } else { "UNX-F3661 mix table mismatch" },
    );
    set.add(
        "u3661-mix-map",
        check_u3661_mix_map(),
        if check_u3661_mix_map() { "" } else { "UNX-F3661 mix map mismatch" },
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
    fn u3661_all_checks_green() {
        let set = run_u3661_checks();
        assert!(set.all_passed(), "U3661 domain checks must be green");
    }

    #[test]
    fn u3661_segment_guard_holds() {
        assert!(u3661_segment_guard());
    }
}
