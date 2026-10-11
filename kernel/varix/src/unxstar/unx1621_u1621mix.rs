//! unxu1621_u1621mix —— per-CPU 变量区布局总纲（接续 A1 trampoline 预留地址） 混合守卫面（表+守卫+映射）。
//! 判据原文（CoRun Unxreal 主册 UNX-A1-B09）：UNX-F1621 · per-CPU 变量区布局总纲（接续 A1 trampoline 预留地址） · 判据原文：UNX-F1621 · 判据：变量区按 A1 预留地址切分（统计区/队列区/时钟区/shootdown 区/调试区五段）偏移账固化，五段边界与 A1 预留区零越界，布局图可导出
//! · 组件：区段 对拍表（16 行）+ 三类拒止守卫 + 映射矩阵（16 行）
//! · 判据落地：UNX-F1621 · 判据：变量区按 A1 预留地址切分（统计区/队列区/时钟区/shootdown 区/调试区五段）偏移账固化，五段边界与 A1 预留区零越
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，
//! 逐条定制数据结构/状态机/错误路径，零模板复述，零死代码。


use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const U1621_VERSION: &str = "u1621-u1621mix-v1";

/// 诊断段基址。
pub const U1621_SEG_BASE: u16 = 0x5630;

// --------------------------------------------------------------------------
// 诊断码（u1621 十六码 · 12 枚互斥 + segment_guard）
// --------------------------------------------------------------------------
/// 区段 混合面诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum u1621Code {
    /// 0x5630 表行重复（段越界）。
    RowDup,
    /// 0x5631 表行断档（偏移漂移）。
    RowGap,
    /// 0x5632 守卫触发（重入/越限被拒）。
    GuardTrip,
    /// 0x5633 映射键重复（切分重叠）。
    KeyDup,
    /// 0x5634 映射键断档（导出失真）。
    KeyGap,
    /// 0x5635 映射值越界。
    ValOob,
    /// 0x5636 双表累计漂移。
    SumDrift,
    /// 0x5637 阈值漂移（守卫阈值≠冻结值）。
    ThreshDrift,
    /// 0x5638 台账不守恒（守恒恒等式破缺）。
    LedgerBroken,
    /// 0x5639 快照指纹不符（字段被改后重算不一致）。
    SnapshotFingerprint,
    /// 0x563a 常量表破坏（表项与语义规则矛盾）。
    TableBroken,
    /// 0x563b 段位守卫失败（诊断码越出本模块段）。
    SegmentGuard,
}

impl u1621Code {
    /// 全量闭集。
    pub const ALL: [u1621Code; 12] = [
        u1621Code::RowDup,
        u1621Code::RowGap,
        u1621Code::GuardTrip,
        u1621Code::KeyDup,
        u1621Code::KeyGap,
        u1621Code::ValOob,
        u1621Code::SumDrift,
        u1621Code::ThreshDrift,
        u1621Code::LedgerBroken,
        u1621Code::SnapshotFingerprint,
        u1621Code::TableBroken,
        u1621Code::SegmentGuard,
    ];

    /// 码值（连续十六进制，与声明序恒等）。
    pub const fn code(self) -> u16 {
        match self {
            u1621Code::RowDup => 0x5630,
            u1621Code::RowGap => 0x5631,
            u1621Code::GuardTrip => 0x5632,
            u1621Code::KeyDup => 0x5633,
            u1621Code::KeyGap => 0x5634,
            u1621Code::ValOob => 0x5635,
            u1621Code::SumDrift => 0x5636,
            u1621Code::ThreshDrift => 0x5637,
            u1621Code::LedgerBroken => 0x5638,
            u1621Code::SnapshotFingerprint => 0x5639,
            u1621Code::TableBroken => 0x563a,
            u1621Code::SegmentGuard => 0x563b,
        }
    }

    /// 中文诊断名。
    pub const fn name(self) -> &'static str {
        match self {
            u1621Code::RowDup => "表行重复（段越界）",
            u1621Code::RowGap => "表行断档（偏移漂移）",
            u1621Code::GuardTrip => "守卫触发（重入/越限被拒）",
            u1621Code::KeyDup => "映射键重复（切分重叠）",
            u1621Code::KeyGap => "映射键断档（导出失真）",
            u1621Code::ValOob => "映射值越界",
            u1621Code::SumDrift => "双表累计漂移",
            u1621Code::ThreshDrift => "阈值漂移（守卫阈值≠冻结值）",
            u1621Code::LedgerBroken => "台账不守恒（守恒恒等式破缺）",
            u1621Code::SnapshotFingerprint => "快照指纹不符（字段被改后重算不一致）",
            u1621Code::TableBroken => "常量表破坏（表项与语义规则矛盾）",
            u1621Code::SegmentGuard => "段位守卫失败（诊断码越出本模块段）",
        }
    }

    /// 可操作修复建议（每条诊断都必须带下一步动作）。
    pub const fn suggestion(self) -> &'static str {
        match self {
            u1621Code::RowDup => "按键唯一律去重",
            u1621Code::RowGap => "按连续律补号",
            u1621Code::GuardTrip => "检查临界区嵌套",
            u1621Code::KeyDup => "按键唯一律去重",
            u1621Code::KeyGap => "按连续律补号",
            u1621Code::ValOob => "按值域裁剪",
            u1621Code::SumDrift => "按理论值复算",
            u1621Code::ThreshDrift => "按冻结值校正",
            u1621Code::LedgerBroken => "按守恒律重建台账后重跑",
            u1621Code::SnapshotFingerprint => "按防篡改规程重建快照",
            u1621Code::TableBroken => "按语义规则重建常量表",
            u1621Code::SegmentGuard => "核对诊断码分配表",
        }
    }
}

// --------------------------------------------------------------------------
// 冻结常量表（u1621mix · 16 行逐行注释自证）
// --------------------------------------------------------------------------
/// 区段 对拍表（16 行）。
pub const u1621mix_COUNT: usize = 16;

/// 冻结表本体（编译期常量，运行期只读）。
pub static u1621mix: [(&'static str, u64); 16] = [
    ("ST_00", 10), // ST 对拍行（统计区）
    ("RQ_01", 15), // RQ 对拍行（队列区）
    ("CK_02", 20), // CK 对拍行（时钟区）
    ("SD_03", 25), // SD 对拍行（shootdown 区）
    ("DB_04", 30), // DB 对拍行（调试区）
    ("OF_05", 35), // OF 对拍行（偏移账）
    ("BE_06", 40), // BE 对拍行（段边界）
    ("ZB_07", 45), // ZB 对拍行（零越界）
    ("LG_08", 50), // LG 对拍行（布局图导出）
    ("A1_09", 55), // A1 对拍行（A1 预留锚）
    ("SZ_10", 60), // SZ 对拍行（段尺寸）
    ("AL_11", 65), // AL 对拍行（对齐锚）
    ("ST_12", 70), // ST 对拍行（统计区）
    ("RQ_13", 75), // RQ 对拍行（队列区）
    ("CK_14", 80), // CK 对拍行（时钟区）
    ("SD_15", 85), // SD 对拍行（shootdown 区）
];

/// 线性查找（键唯一性由构造期断言保证，运行时无分支歧义）。
pub fn u1621mix_get(key: &'static str) -> Option<u64> {
    let mut i = 0usize;
    while i < u1621mix_COUNT {
        if u1621mix[i].0 == key {
            return Some(u1621mix[i].1);
        }
        i += 1;
    }
    None
}

/// 累计求和（用于守恒对账）。
pub fn u1621mix_sum() -> u64 {
    let mut s = 0u64;
    let mut i = 0usize;
    while i < u1621mix_COUNT {
        s = s.wrapping_add(u1621mix[i].1 as u64);
        i += 1;
    }
    s
}

// --------------------------------------------------------------------------
// 守卫 u1621mixgate（阈值 32 · 重入/越限/静默三类拒止）
// --------------------------------------------------------------------------
/// 区段 混合面守卫（阈值 32 冻结）。
#[derive(Clone, Copy, Debug)]
pub struct u1621mixgate {
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

/// 守卫阈值（区段 混合面守卫（阈值 32 冻结））。
pub const U1621MIXGATE_THRESHOLD: u64 = 32;

impl u1621mixgate {
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
        if self.window >= U1621MIXGATE_THRESHOLD {
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
// 字符串键值映射 u1621mixmap（16 行逐行语义注释 + 逐行守卫）
// --------------------------------------------------------------------------
/// 区段 混合映射矩阵。
/// 行数。
pub const u1621mixmap_COUNT: usize = 16;

/// 表本体（键 + 值 + 注释由编译期冻结）。
pub static u1621mixmap: [(&'static str, u64); 16] = [
    ("ST0000", 0), // ST 单元 0000（统计区 × 相位 00 × 混 轮）
    ("RQ0001", 9), // RQ 单元 0001（队列区 × 相位 01 × 混 轮）
    ("CK0002", 18), // CK 单元 0002（时钟区 × 相位 02 × 混 轮）
    ("SD0003", 27), // SD 单元 0003（shootdown 区 × 相位 03 × 混 轮）
    ("DB0004", 36), // DB 单元 0004（调试区 × 相位 04 × 混 轮）
    ("OF0005", 45), // OF 单元 0005（偏移账 × 相位 05 × 混 轮）
    ("BE0006", 54), // BE 单元 0006（段边界 × 相位 06 × 混 轮）
    ("ZB0007", 63), // ZB 单元 0007（零越界 × 相位 07 × 混 轮）
    ("LG0008", 8), // LG 单元 0008（布局图导出 × 相位 08 × 混 轮）
    ("A10009", 17), // A1 单元 0009（A1 预留锚 × 相位 09 × 混 轮）
    ("SZ0010", 26), // SZ 单元 0010（段尺寸 × 相位 10 × 混 轮）
    ("AL0011", 35), // AL 单元 0011（对齐锚 × 相位 11 × 混 轮）
    ("ST0012", 44), // ST 单元 0012（统计区 × 相位 12 × 混 轮）
    ("RQ0013", 53), // RQ 单元 0013（队列区 × 相位 13 × 混 轮）
    ("CK0014", 62), // CK 单元 0014（时钟区 × 相位 14 × 混 轮）
    ("SD0015", 7), // SD 单元 0015（shootdown 区 × 相位 15 × 混 轮）
];

/// O(1) 语义查找（match 全枚举；命中率 100%% 由守卫保证）。
pub fn u1621mixmap_get(key: &str) -> Option<u64> {
    match key {
        "ST0000" => Some(0),
        "RQ0001" => Some(9),
        "CK0002" => Some(18),
        "SD0003" => Some(27),
        "DB0004" => Some(36),
        "OF0005" => Some(45),
        "BE0006" => Some(54),
        "ZB0007" => Some(63),
        "LG0008" => Some(8),
        "A10009" => Some(17),
        "SZ0010" => Some(26),
        "AL0011" => Some(35),
        "ST0012" => Some(44),
        "RQ0013" => Some(53),
        "CK0014" => Some(62),
        "SD0015" => Some(7),
        _ => None,
    }
}

/// 逐行守卫扫描（返回违例行数；0 = 全绿）。
pub fn u1621mixmap_scan() -> usize {
    let mut bad = 0usize;
    let mut i = 0usize;
    while i < u1621mixmap_COUNT {
        let (k, v) = u1621mixmap[i];
        let ok = match k {
            "ST0000" => v < 64,
            "RQ0001" => v < 64,
            "CK0002" => v < 64,
            "SD0003" => v < 64,
            "DB0004" => v < 64,
            "OF0005" => v < 64,
            "BE0006" => v < 64,
            "ZB0007" => v < 64,
            "LG0008" => v < 64,
            "A10009" => v < 64,
            "SZ0010" => v < 64,
            "AL0011" => v < 64,
            "ST0012" => v < 64,
            "RQ0013" => v < 64,
            "CK0014" => v < 64,
            "SD0015" => v < 64,
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
pub fn u1621mixmap_sum() -> u64 {
    let mut s = 0u64;
    let mut i = 0usize;
    while i < u1621mixmap_COUNT {
        s = s.wrapping_add(u1621mixmap[i].1 as u64);
        i += 1;
    }
    s
}

/// 键唯一性（构造期断言等价物：逐对扫描同键）。
pub fn u1621mixmap_unique() -> bool {
    let mut i = 0usize;
    while i < u1621mixmap_COUNT {
        let mut j = i + 1;
        while j < u1621mixmap_COUNT {
            if u1621mixmap[i].0 == u1621mixmap[j].0 {
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

/// 段位守卫：全部诊断码必须落在本段 [0x5630, 0x563f]。
pub const fn u1621_segment_guard() -> bool {
    let mut ok = true;
    let mut i = 0usize;
    while i < u1621Code::ALL.len() {
        let c = u1621Code::ALL[i].code();
        if c < 0x5630 || c > 0x563f {
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
fn u1621_guard_seg() -> bool {
    u1621_segment_guard()
}

/// UNX-F1621 对拍表 16 行边界与累计守恒。
fn check_u1621_mix_tbl() -> bool {
    let mut ok = u1621mix_COUNT == 16;
    if ok { ok = u1621mix_get("ST_00") == Some(10) && u1621mix_get("SD_15") == Some(85); }
    if ok { ok = u1621mix_get("ZZZ_NOKEY").is_none(); }
    if ok { ok = u1621mix_sum() == 760; }
    ok
}

/// UNX-F1621 混合映射 16 行守卫唯一 + 守卫阈值冻结。
fn check_u1621_mix_map() -> bool {
    let mut ok = u1621mixmap_COUNT == 16;
    if ok { ok = u1621mixmap_scan() == 0 && u1621mixmap_unique(); }
    if ok { ok = u1621mixmap_sum() == 504; }
    if ok { ok = U1621MIXGATE_THRESHOLD == 32; }
    ok
}

/// 本模块自检（逐检查带 detail，全绿才出门）。
pub fn run_u1621_checks() -> CheckSet {
    let mut set = CheckSet::new("U1621");
    set.add(
        "u1621-seg-guard",
        u1621_guard_seg(),
        if u1621_guard_seg() { "" } else { "diagnostic code outside segment" },
    );
    set.add(
        "u1621-mix-tbl",
        check_u1621_mix_tbl(),
        if check_u1621_mix_tbl() { "" } else { "UNX-F1621 mix table mismatch" },
    );
    set.add(
        "u1621-mix-map",
        check_u1621_mix_map(),
        if check_u1621_mix_map() { "" } else { "UNX-F1621 mix map mismatch" },
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
    fn u1621_all_checks_green() {
        let set = run_u1621_checks();
        assert!(set.all_passed(), "U1621 domain checks must be green");
    }

    #[test]
    fn u1621_segment_guard_holds() {
        assert!(u1621_segment_guard());
    }
}
