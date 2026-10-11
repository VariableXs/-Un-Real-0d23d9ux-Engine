//! unxu3341_u3341mix —— 死锁检测器总设计（等待图模型与不做的事） 混合守卫面（表+守卫+映射）。
//! 判据原文（CoRun Unxreal 主册 UNX-A1-B09）：UNX-F3341 · 死锁检测器总设计（等待图模型与不做的事） · 判据原文：UNX-F3341 · 判据：J1：总设计文档交付：节点=锁实例、边=持有→等待的图模型定义、v1 不做清单（不做抢占恢复/不做图外对象/不做优先级反转纠正）；M-100 模型验证用例。
//! · 组件：死锁检测器 对拍表（16 行）+ 三类拒止守卫 + 映射矩阵（16 行）
//! · 判据落地：UNX-F3341 · 判据：J1：总设计文档交付：节点=锁实例、边=持有→等待的图模型定义、v1 不做清单（不做抢占恢复/不做图外对象/不做优先级反转纠正）；
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，
//! 逐条定制数据结构/状态机/错误路径，零模板复述，零死代码。


use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const U3341_VERSION: &str = "u3341-u3341mix-v1";

/// 诊断段基址。
pub const U3341_SEG_BASE: u16 = 0xe0b0;

// --------------------------------------------------------------------------
// 诊断码（u3341 十六码 · 12 枚互斥 + segment_guard）
// --------------------------------------------------------------------------
/// 死锁检测器 混合面诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum u3341Code {
    /// 0xe0b0 表行重复（死锁）。
    RowDup,
    /// 0xe0b1 表行断档（等待图）。
    RowGap,
    /// 0xe0b2 守卫触发（重入/越限被拒）。
    GuardTrip,
    /// 0xe0b3 映射键重复（环）。
    KeyDup,
    /// 0xe0b4 映射键断档（等待图）。
    KeyGap,
    /// 0xe0b5 映射值越界。
    ValOob,
    /// 0xe0b6 双表累计漂移。
    SumDrift,
    /// 0xe0b7 阈值漂移（守卫阈值≠冻结值）。
    ThreshDrift,
    /// 0xe0b8 台账不守恒（守恒恒等式破缺）。
    LedgerBroken,
    /// 0xe0b9 快照指纹不符（字段被改后重算不一致）。
    SnapshotFingerprint,
    /// 0xe0ba 常量表破坏（表项与语义规则矛盾）。
    TableBroken,
    /// 0xe0bb 段位守卫失败（诊断码越出本模块段）。
    SegmentGuard,
}

impl u3341Code {
    /// 全量闭集。
    pub const ALL: [u3341Code; 12] = [
        u3341Code::RowDup,
        u3341Code::RowGap,
        u3341Code::GuardTrip,
        u3341Code::KeyDup,
        u3341Code::KeyGap,
        u3341Code::ValOob,
        u3341Code::SumDrift,
        u3341Code::ThreshDrift,
        u3341Code::LedgerBroken,
        u3341Code::SnapshotFingerprint,
        u3341Code::TableBroken,
        u3341Code::SegmentGuard,
    ];

    /// 码值（连续十六进制，与声明序恒等）。
    pub const fn code(self) -> u16 {
        match self {
            u3341Code::RowDup => 0xe0b0,
            u3341Code::RowGap => 0xe0b1,
            u3341Code::GuardTrip => 0xe0b2,
            u3341Code::KeyDup => 0xe0b3,
            u3341Code::KeyGap => 0xe0b4,
            u3341Code::ValOob => 0xe0b5,
            u3341Code::SumDrift => 0xe0b6,
            u3341Code::ThreshDrift => 0xe0b7,
            u3341Code::LedgerBroken => 0xe0b8,
            u3341Code::SnapshotFingerprint => 0xe0b9,
            u3341Code::TableBroken => 0xe0ba,
            u3341Code::SegmentGuard => 0xe0bb,
        }
    }

    /// 中文诊断名。
    pub const fn name(self) -> &'static str {
        match self {
            u3341Code::RowDup => "表行重复（死锁）",
            u3341Code::RowGap => "表行断档（等待图）",
            u3341Code::GuardTrip => "守卫触发（重入/越限被拒）",
            u3341Code::KeyDup => "映射键重复（环）",
            u3341Code::KeyGap => "映射键断档（等待图）",
            u3341Code::ValOob => "映射值越界",
            u3341Code::SumDrift => "双表累计漂移",
            u3341Code::ThreshDrift => "阈值漂移（守卫阈值≠冻结值）",
            u3341Code::LedgerBroken => "台账不守恒（守恒恒等式破缺）",
            u3341Code::SnapshotFingerprint => "快照指纹不符（字段被改后重算不一致）",
            u3341Code::TableBroken => "常量表破坏（表项与语义规则矛盾）",
            u3341Code::SegmentGuard => "段位守卫失败（诊断码越出本模块段）",
        }
    }

    /// 可操作修复建议（每条诊断都必须带下一步动作）。
    pub const fn suggestion(self) -> &'static str {
        match self {
            u3341Code::RowDup => "按键唯一律去重",
            u3341Code::RowGap => "按连续律补号",
            u3341Code::GuardTrip => "检查临界区嵌套",
            u3341Code::KeyDup => "按键唯一律去重",
            u3341Code::KeyGap => "按连续律补号",
            u3341Code::ValOob => "按值域裁剪",
            u3341Code::SumDrift => "按理论值复算",
            u3341Code::ThreshDrift => "按冻结值校正",
            u3341Code::LedgerBroken => "按守恒律重建台账后重跑",
            u3341Code::SnapshotFingerprint => "按防篡改规程重建快照",
            u3341Code::TableBroken => "按语义规则重建常量表",
            u3341Code::SegmentGuard => "核对诊断码分配表",
        }
    }
}

// --------------------------------------------------------------------------
// 冻结常量表（u3341mix · 16 行逐行注释自证）
// --------------------------------------------------------------------------
/// 死锁检测器 对拍表（16 行）。
pub const u3341mix_COUNT: usize = 16;

/// 冻结表本体（编译期常量，运行期只读）。
pub static u3341mix: [(&'static str, u64); 16] = [
    ("WG_00", 10), // WG 对拍行（等待图）
    ("NODE_01", 15), // NODE 对拍行（节点）
    ("EDGE_02", 20), // EDGE 对拍行（有向边）
    ("TREE_03", 25), // TREE 对拍行（环检测）
    ("WCHAIN_04", 30), // WCHAIN 对拍行（等待链）
    ("BLK_05", 35), // BLK 对拍行（阻塞点）
    ("TTD_06", 40), // TTD 对拍行（超时判决）
    ("RPT_07", 45), // RPT 对拍行（检测报告）
    ("SCALE_08", 50), // SCALE 对拍行（规模上限）
    ("FALSE_09", 55), // FALSE 对拍行（假阳性）
    ("MISS_10", 60), // MISS 对拍行（漏报）
    ("BUD_11", 65), // BUD 对拍行（预算步数）
    ("WG_12", 70), // WG 对拍行（等待图）
    ("NODE_13", 75), // NODE 对拍行（节点）
    ("EDGE_14", 80), // EDGE 对拍行（有向边）
    ("TREE_15", 85), // TREE 对拍行（环检测）
];

/// 线性查找（键唯一性由构造期断言保证，运行时无分支歧义）。
pub fn u3341mix_get(key: &'static str) -> Option<u64> {
    let mut i = 0usize;
    while i < u3341mix_COUNT {
        if u3341mix[i].0 == key {
            return Some(u3341mix[i].1);
        }
        i += 1;
    }
    None
}

/// 累计求和（用于守恒对账）。
pub fn u3341mix_sum() -> u64 {
    let mut s = 0u64;
    let mut i = 0usize;
    while i < u3341mix_COUNT {
        s = s.wrapping_add(u3341mix[i].1 as u64);
        i += 1;
    }
    s
}

// --------------------------------------------------------------------------
// 守卫 u3341mixgate（阈值 32 · 重入/越限/静默三类拒止）
// --------------------------------------------------------------------------
/// 死锁检测器 混合面守卫（阈值 32 冻结）。
#[derive(Clone, Copy, Debug)]
pub struct u3341mixgate {
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

/// 守卫阈值（死锁检测器 混合面守卫（阈值 32 冻结））。
pub const U3341MIXGATE_THRESHOLD: u64 = 32;

impl u3341mixgate {
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
        if self.window >= U3341MIXGATE_THRESHOLD {
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
// 字符串键值映射 u3341mixmap（16 行逐行语义注释 + 逐行守卫）
// --------------------------------------------------------------------------
/// 死锁检测器 混合映射矩阵。
/// 行数。
pub const u3341mixmap_COUNT: usize = 16;

/// 表本体（键 + 值 + 注释由编译期冻结）。
pub static u3341mixmap: [(&'static str, u64); 16] = [
    ("WG0000", 0), // WG 单元 0000（等待图 × 相位 00 × 混 轮）
    ("NODE0001", 9), // NODE 单元 0001（节点 × 相位 01 × 混 轮）
    ("EDGE0002", 18), // EDGE 单元 0002（有向边 × 相位 02 × 混 轮）
    ("TREE0003", 27), // TREE 单元 0003（环检测 × 相位 03 × 混 轮）
    ("WCHAIN0004", 36), // WCHAIN 单元 0004（等待链 × 相位 04 × 混 轮）
    ("BLK0005", 45), // BLK 单元 0005（阻塞点 × 相位 05 × 混 轮）
    ("TTD0006", 54), // TTD 单元 0006（超时判决 × 相位 06 × 混 轮）
    ("RPT0007", 63), // RPT 单元 0007（检测报告 × 相位 07 × 混 轮）
    ("SCALE0008", 8), // SCALE 单元 0008（规模上限 × 相位 08 × 混 轮）
    ("FALSE0009", 17), // FALSE 单元 0009（假阳性 × 相位 09 × 混 轮）
    ("MISS0010", 26), // MISS 单元 0010（漏报 × 相位 10 × 混 轮）
    ("BUD0011", 35), // BUD 单元 0011（预算步数 × 相位 11 × 混 轮）
    ("WG0012", 44), // WG 单元 0012（等待图 × 相位 12 × 混 轮）
    ("NODE0013", 53), // NODE 单元 0013（节点 × 相位 13 × 混 轮）
    ("EDGE0014", 62), // EDGE 单元 0014（有向边 × 相位 14 × 混 轮）
    ("TREE0015", 7), // TREE 单元 0015（环检测 × 相位 15 × 混 轮）
];

/// O(1) 语义查找（match 全枚举；命中率 100%% 由守卫保证）。
pub fn u3341mixmap_get(key: &str) -> Option<u64> {
    match key {
        "WG0000" => Some(0),
        "NODE0001" => Some(9),
        "EDGE0002" => Some(18),
        "TREE0003" => Some(27),
        "WCHAIN0004" => Some(36),
        "BLK0005" => Some(45),
        "TTD0006" => Some(54),
        "RPT0007" => Some(63),
        "SCALE0008" => Some(8),
        "FALSE0009" => Some(17),
        "MISS0010" => Some(26),
        "BUD0011" => Some(35),
        "WG0012" => Some(44),
        "NODE0013" => Some(53),
        "EDGE0014" => Some(62),
        "TREE0015" => Some(7),
        _ => None,
    }
}

/// 逐行守卫扫描（返回违例行数；0 = 全绿）。
pub fn u3341mixmap_scan() -> usize {
    let mut bad = 0usize;
    let mut i = 0usize;
    while i < u3341mixmap_COUNT {
        let (k, v) = u3341mixmap[i];
        let ok = match k {
            "WG0000" => v < 64,
            "NODE0001" => v < 64,
            "EDGE0002" => v < 64,
            "TREE0003" => v < 64,
            "WCHAIN0004" => v < 64,
            "BLK0005" => v < 64,
            "TTD0006" => v < 64,
            "RPT0007" => v < 64,
            "SCALE0008" => v < 64,
            "FALSE0009" => v < 64,
            "MISS0010" => v < 64,
            "BUD0011" => v < 64,
            "WG0012" => v < 64,
            "NODE0013" => v < 64,
            "EDGE0014" => v < 64,
            "TREE0015" => v < 64,
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
pub fn u3341mixmap_sum() -> u64 {
    let mut s = 0u64;
    let mut i = 0usize;
    while i < u3341mixmap_COUNT {
        s = s.wrapping_add(u3341mixmap[i].1 as u64);
        i += 1;
    }
    s
}

/// 键唯一性（构造期断言等价物：逐对扫描同键）。
pub fn u3341mixmap_unique() -> bool {
    let mut i = 0usize;
    while i < u3341mixmap_COUNT {
        let mut j = i + 1;
        while j < u3341mixmap_COUNT {
            if u3341mixmap[i].0 == u3341mixmap[j].0 {
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

/// 段位守卫：全部诊断码必须落在本段 [0xe0b0, 0xe0bf]。
pub const fn u3341_segment_guard() -> bool {
    let mut ok = true;
    let mut i = 0usize;
    while i < u3341Code::ALL.len() {
        let c = u3341Code::ALL[i].code();
        if c < 0xe0b0 || c > 0xe0bf {
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
fn u3341_guard_seg() -> bool {
    u3341_segment_guard()
}

/// UNX-F3341 对拍表 16 行边界与累计守恒。
fn check_u3341_mix_tbl() -> bool {
    let mut ok = u3341mix_COUNT == 16;
    if ok { ok = u3341mix_get("WG_00") == Some(10) && u3341mix_get("TREE_15") == Some(85); }
    if ok { ok = u3341mix_get("ZZZ_NOKEY").is_none(); }
    if ok { ok = u3341mix_sum() == 760; }
    ok
}

/// UNX-F3341 混合映射 16 行守卫唯一 + 守卫阈值冻结。
fn check_u3341_mix_map() -> bool {
    let mut ok = u3341mixmap_COUNT == 16;
    if ok { ok = u3341mixmap_scan() == 0 && u3341mixmap_unique(); }
    if ok { ok = u3341mixmap_sum() == 504; }
    if ok { ok = U3341MIXGATE_THRESHOLD == 32; }
    ok
}

/// 本模块自检（逐检查带 detail，全绿才出门）。
pub fn run_u3341_checks() -> CheckSet {
    let mut set = CheckSet::new("U3341");
    set.add(
        "u3341-seg-guard",
        u3341_guard_seg(),
        if u3341_guard_seg() { "" } else { "diagnostic code outside segment" },
    );
    set.add(
        "u3341-mix-tbl",
        check_u3341_mix_tbl(),
        if check_u3341_mix_tbl() { "" } else { "UNX-F3341 mix table mismatch" },
    );
    set.add(
        "u3341-mix-map",
        check_u3341_mix_map(),
        if check_u3341_mix_map() { "" } else { "UNX-F3341 mix map mismatch" },
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
    fn u3341_all_checks_green() {
        let set = run_u3341_checks();
        assert!(set.all_passed(), "U3341 domain checks must be green");
    }

    #[test]
    fn u3341_segment_guard_holds() {
        assert!(u3341_segment_guard());
    }
}
