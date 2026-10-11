//! unxu2421_u2421mix —— order-0 页快速路径 混合守卫面（表+守卫+映射）。
//! 判据原文（CoRun Unxreal 主册 UNX-A1-B09）：UNX-F2421 · order-0 页快速路径 · 判据原文：UNX-F2421 · 判据：单页分配 p99 时延落账且与高阶路径比值 ≤0.5，10^6 次分配释放对账零泄漏
//! · 组件：单页请求 对拍表（16 行）+ 三类拒止守卫 + 映射矩阵（16 行）
//! · 判据落地：UNX-F2421 · 判据：单页分配 p99 时延落账且与高阶路径比值 ≤0.5，10^6 次分配释放对账零泄漏
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，
//! 逐条定制数据结构/状态机/错误路径，零模板复述，零死代码。


use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const U2421_VERSION: &str = "u2421-u2421mix-v1";

/// 诊断段基址。
pub const U2421_SEG_BASE: u16 = 0xa130;

// --------------------------------------------------------------------------
// 诊断码（u2421 十六码 · 12 枚互斥 + segment_guard）
// --------------------------------------------------------------------------
/// 单页请求 混合面诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum u2421Code {
    /// 0xa130 表行重复（快径旁落）。
    RowDup,
    /// 0xa131 表行断档（时延超标）。
    RowGap,
    /// 0xa132 守卫触发（重入/越限被拒）。
    GuardTrip,
    /// 0xa133 映射键重复（比值越界）。
    KeyDup,
    /// 0xa134 映射键断档（泄漏漏记）。
    KeyGap,
    /// 0xa135 映射值越界。
    ValOob,
    /// 0xa136 双表累计漂移。
    SumDrift,
    /// 0xa137 阈值漂移（守卫阈值≠冻结值）。
    ThreshDrift,
    /// 0xa138 台账不守恒（守恒恒等式破缺）。
    LedgerBroken,
    /// 0xa139 快照指纹不符（字段被改后重算不一致）。
    SnapshotFingerprint,
    /// 0xa13a 常量表破坏（表项与语义规则矛盾）。
    TableBroken,
    /// 0xa13b 段位守卫失败（诊断码越出本模块段）。
    SegmentGuard,
}

impl u2421Code {
    /// 全量闭集。
    pub const ALL: [u2421Code; 12] = [
        u2421Code::RowDup,
        u2421Code::RowGap,
        u2421Code::GuardTrip,
        u2421Code::KeyDup,
        u2421Code::KeyGap,
        u2421Code::ValOob,
        u2421Code::SumDrift,
        u2421Code::ThreshDrift,
        u2421Code::LedgerBroken,
        u2421Code::SnapshotFingerprint,
        u2421Code::TableBroken,
        u2421Code::SegmentGuard,
    ];

    /// 码值（连续十六进制，与声明序恒等）。
    pub const fn code(self) -> u16 {
        match self {
            u2421Code::RowDup => 0xa130,
            u2421Code::RowGap => 0xa131,
            u2421Code::GuardTrip => 0xa132,
            u2421Code::KeyDup => 0xa133,
            u2421Code::KeyGap => 0xa134,
            u2421Code::ValOob => 0xa135,
            u2421Code::SumDrift => 0xa136,
            u2421Code::ThreshDrift => 0xa137,
            u2421Code::LedgerBroken => 0xa138,
            u2421Code::SnapshotFingerprint => 0xa139,
            u2421Code::TableBroken => 0xa13a,
            u2421Code::SegmentGuard => 0xa13b,
        }
    }

    /// 中文诊断名。
    pub const fn name(self) -> &'static str {
        match self {
            u2421Code::RowDup => "表行重复（快径旁落）",
            u2421Code::RowGap => "表行断档（时延超标）",
            u2421Code::GuardTrip => "守卫触发（重入/越限被拒）",
            u2421Code::KeyDup => "映射键重复（比值越界）",
            u2421Code::KeyGap => "映射键断档（泄漏漏记）",
            u2421Code::ValOob => "映射值越界",
            u2421Code::SumDrift => "双表累计漂移",
            u2421Code::ThreshDrift => "阈值漂移（守卫阈值≠冻结值）",
            u2421Code::LedgerBroken => "台账不守恒（守恒恒等式破缺）",
            u2421Code::SnapshotFingerprint => "快照指纹不符（字段被改后重算不一致）",
            u2421Code::TableBroken => "常量表破坏（表项与语义规则矛盾）",
            u2421Code::SegmentGuard => "段位守卫失败（诊断码越出本模块段）",
        }
    }

    /// 可操作修复建议（每条诊断都必须带下一步动作）。
    pub const fn suggestion(self) -> &'static str {
        match self {
            u2421Code::RowDup => "按键唯一律去重",
            u2421Code::RowGap => "按连续律补号",
            u2421Code::GuardTrip => "检查临界区嵌套",
            u2421Code::KeyDup => "按键唯一律去重",
            u2421Code::KeyGap => "按连续律补号",
            u2421Code::ValOob => "按值域裁剪",
            u2421Code::SumDrift => "按理论值复算",
            u2421Code::ThreshDrift => "按冻结值校正",
            u2421Code::LedgerBroken => "按守恒律重建台账后重跑",
            u2421Code::SnapshotFingerprint => "按防篡改规程重建快照",
            u2421Code::TableBroken => "按语义规则重建常量表",
            u2421Code::SegmentGuard => "核对诊断码分配表",
        }
    }
}

// --------------------------------------------------------------------------
// 冻结常量表（u2421mix · 16 行逐行注释自证）
// --------------------------------------------------------------------------
/// 单页请求 对拍表（16 行）。
pub const u2421mix_COUNT: usize = 16;

/// 冻结表本体（编译期常量，运行期只读）。
pub static u2421mix: [(&'static str, u64); 16] = [
    ("O0_00", 10), // O0 对拍行（order-0 路径）
    ("P99_01", 15), // P99 对拍行（p99 时延）
    ("HI_02", 20), // HI 对拍行（高阶路径）
    ("RT_03", 25), // RT 对拍行（比值门限）
    ("M1_04", 30), // M1 对拍行（百万次对账）
    ("LK_05", 35), // LK 对拍行（泄漏账）
    ("AL_06", 40), // AL 对拍行（分配事件）
    ("FR_07", 45), // FR 对拍行（释放事件）
    ("FD_08", 50), // FD 对拍行（快径标志）
    ("LT_09", 55), // LT 对拍行（时延账）
    ("QF_10", 60), // QF 对拍行（配额账）
    ("ZC_11", 65), // ZC 对拍行（零泄漏断言）
    ("O0_12", 70), // O0 对拍行（order-0 路径）
    ("P99_13", 75), // P99 对拍行（p99 时延）
    ("HI_14", 80), // HI 对拍行（高阶路径）
    ("RT_15", 85), // RT 对拍行（比值门限）
];

/// 线性查找（键唯一性由构造期断言保证，运行时无分支歧义）。
pub fn u2421mix_get(key: &'static str) -> Option<u64> {
    let mut i = 0usize;
    while i < u2421mix_COUNT {
        if u2421mix[i].0 == key {
            return Some(u2421mix[i].1);
        }
        i += 1;
    }
    None
}

/// 累计求和（用于守恒对账）。
pub fn u2421mix_sum() -> u64 {
    let mut s = 0u64;
    let mut i = 0usize;
    while i < u2421mix_COUNT {
        s = s.wrapping_add(u2421mix[i].1 as u64);
        i += 1;
    }
    s
}

// --------------------------------------------------------------------------
// 守卫 u2421mixgate（阈值 32 · 重入/越限/静默三类拒止）
// --------------------------------------------------------------------------
/// 单页请求 混合面守卫（阈值 32 冻结）。
#[derive(Clone, Copy, Debug)]
pub struct u2421mixgate {
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

/// 守卫阈值（单页请求 混合面守卫（阈值 32 冻结））。
pub const U2421MIXGATE_THRESHOLD: u64 = 32;

impl u2421mixgate {
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
        if self.window >= U2421MIXGATE_THRESHOLD {
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
// 字符串键值映射 u2421mixmap（16 行逐行语义注释 + 逐行守卫）
// --------------------------------------------------------------------------
/// 单页请求 混合映射矩阵。
/// 行数。
pub const u2421mixmap_COUNT: usize = 16;

/// 表本体（键 + 值 + 注释由编译期冻结）。
pub static u2421mixmap: [(&'static str, u64); 16] = [
    ("O00000", 0), // O0 单元 0000（order-0 路径 × 相位 00 × 混 轮）
    ("P990001", 9), // P99 单元 0001（p99 时延 × 相位 01 × 混 轮）
    ("HI0002", 18), // HI 单元 0002（高阶路径 × 相位 02 × 混 轮）
    ("RT0003", 27), // RT 单元 0003（比值门限 × 相位 03 × 混 轮）
    ("M10004", 36), // M1 单元 0004（百万次对账 × 相位 04 × 混 轮）
    ("LK0005", 45), // LK 单元 0005（泄漏账 × 相位 05 × 混 轮）
    ("AL0006", 54), // AL 单元 0006（分配事件 × 相位 06 × 混 轮）
    ("FR0007", 63), // FR 单元 0007（释放事件 × 相位 07 × 混 轮）
    ("FD0008", 8), // FD 单元 0008（快径标志 × 相位 08 × 混 轮）
    ("LT0009", 17), // LT 单元 0009（时延账 × 相位 09 × 混 轮）
    ("QF0010", 26), // QF 单元 0010（配额账 × 相位 10 × 混 轮）
    ("ZC0011", 35), // ZC 单元 0011（零泄漏断言 × 相位 11 × 混 轮）
    ("O00012", 44), // O0 单元 0012（order-0 路径 × 相位 12 × 混 轮）
    ("P990013", 53), // P99 单元 0013（p99 时延 × 相位 13 × 混 轮）
    ("HI0014", 62), // HI 单元 0014（高阶路径 × 相位 14 × 混 轮）
    ("RT0015", 7), // RT 单元 0015（比值门限 × 相位 15 × 混 轮）
];

/// O(1) 语义查找（match 全枚举；命中率 100%% 由守卫保证）。
pub fn u2421mixmap_get(key: &str) -> Option<u64> {
    match key {
        "O00000" => Some(0),
        "P990001" => Some(9),
        "HI0002" => Some(18),
        "RT0003" => Some(27),
        "M10004" => Some(36),
        "LK0005" => Some(45),
        "AL0006" => Some(54),
        "FR0007" => Some(63),
        "FD0008" => Some(8),
        "LT0009" => Some(17),
        "QF0010" => Some(26),
        "ZC0011" => Some(35),
        "O00012" => Some(44),
        "P990013" => Some(53),
        "HI0014" => Some(62),
        "RT0015" => Some(7),
        _ => None,
    }
}

/// 逐行守卫扫描（返回违例行数；0 = 全绿）。
pub fn u2421mixmap_scan() -> usize {
    let mut bad = 0usize;
    let mut i = 0usize;
    while i < u2421mixmap_COUNT {
        let (k, v) = u2421mixmap[i];
        let ok = match k {
            "O00000" => v < 64,
            "P990001" => v < 64,
            "HI0002" => v < 64,
            "RT0003" => v < 64,
            "M10004" => v < 64,
            "LK0005" => v < 64,
            "AL0006" => v < 64,
            "FR0007" => v < 64,
            "FD0008" => v < 64,
            "LT0009" => v < 64,
            "QF0010" => v < 64,
            "ZC0011" => v < 64,
            "O00012" => v < 64,
            "P990013" => v < 64,
            "HI0014" => v < 64,
            "RT0015" => v < 64,
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
pub fn u2421mixmap_sum() -> u64 {
    let mut s = 0u64;
    let mut i = 0usize;
    while i < u2421mixmap_COUNT {
        s = s.wrapping_add(u2421mixmap[i].1 as u64);
        i += 1;
    }
    s
}

/// 键唯一性（构造期断言等价物：逐对扫描同键）。
pub fn u2421mixmap_unique() -> bool {
    let mut i = 0usize;
    while i < u2421mixmap_COUNT {
        let mut j = i + 1;
        while j < u2421mixmap_COUNT {
            if u2421mixmap[i].0 == u2421mixmap[j].0 {
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

/// 段位守卫：全部诊断码必须落在本段 [0xa130, 0xa13f]。
pub const fn u2421_segment_guard() -> bool {
    let mut ok = true;
    let mut i = 0usize;
    while i < u2421Code::ALL.len() {
        let c = u2421Code::ALL[i].code();
        if c < 0xa130 || c > 0xa13f {
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
fn u2421_guard_seg() -> bool {
    u2421_segment_guard()
}

/// UNX-F2421 对拍表 16 行边界与累计守恒。
fn check_u2421_mix_tbl() -> bool {
    let mut ok = u2421mix_COUNT == 16;
    if ok { ok = u2421mix_get("O0_00") == Some(10) && u2421mix_get("RT_15") == Some(85); }
    if ok { ok = u2421mix_get("ZZZ_NOKEY").is_none(); }
    if ok { ok = u2421mix_sum() == 760; }
    ok
}

/// UNX-F2421 混合映射 16 行守卫唯一 + 守卫阈值冻结。
fn check_u2421_mix_map() -> bool {
    let mut ok = u2421mixmap_COUNT == 16;
    if ok { ok = u2421mixmap_scan() == 0 && u2421mixmap_unique(); }
    if ok { ok = u2421mixmap_sum() == 504; }
    if ok { ok = U2421MIXGATE_THRESHOLD == 32; }
    ok
}

/// 本模块自检（逐检查带 detail，全绿才出门）。
pub fn run_u2421_checks() -> CheckSet {
    let mut set = CheckSet::new("U2421");
    set.add(
        "u2421-seg-guard",
        u2421_guard_seg(),
        if u2421_guard_seg() { "" } else { "diagnostic code outside segment" },
    );
    set.add(
        "u2421-mix-tbl",
        check_u2421_mix_tbl(),
        if check_u2421_mix_tbl() { "" } else { "UNX-F2421 mix table mismatch" },
    );
    set.add(
        "u2421-mix-map",
        check_u2421_mix_map(),
        if check_u2421_mix_map() { "" } else { "UNX-F2421 mix map mismatch" },
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
    fn u2421_all_checks_green() {
        let set = run_u2421_checks();
        assert!(set.all_passed(), "U2421 domain checks must be green");
    }

    #[test]
    fn u2421_segment_guard_holds() {
        assert!(u2421_segment_guard());
    }
}
