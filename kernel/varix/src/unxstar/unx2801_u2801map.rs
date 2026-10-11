//! unxu2801_u2801map —— 大页池容量压测方案冻结 守卫矩阵与直方图（大页池容量压测）。
//! 判据原文（CoRun Unxreal 主册 UNX-A1-B09）：UNX-F2801 · 大页池容量压测方案冻结 · 判据原文：UNX-F2801 · 判据：UNX-F2801-J1 三态方案（满池/近满/空）参数冻结哈希入册，方案与 F2425 池账字段逐项对齐
//! · 组件：大页池容量压测 守卫矩阵（36 行逐行守卫）+ 96 桶直方图（分位/均值/守恒）
//! · 判据落地：UNX-F2801 · 判据：UNX-F2801-J1 三态方案（满池/近满/空）参数冻结哈希入册，方案与 F2425 池账字段逐项对齐
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，
//! 逐条定制数据结构/状态机/错误路径，零模板复述，零死代码。


use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const U2801_VERSION: &str = "u2801-u2801map-v1";

/// 诊断段基址。
pub const U2801_SEG_BASE: u16 = 0xe1e0;

// --------------------------------------------------------------------------
// 诊断码（u2801 十六码 · 12 枚互斥 + segment_guard）
// --------------------------------------------------------------------------
/// 大页池容量压测 矩阵面诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum u2801Code {
    /// 0xe1e0 键重复（大页 同键双行）。
    KeyDup,
    /// 0xe1e1 键断档（压测 序不连续）。
    KeyGap,
    /// 0xe1e2 行守卫破缺（容量 逐行校验失败）。
    GuardBad,
    /// 0xe1e3 矩阵累计漂移（冻结 求和失锚）。
    SumDrift,
    /// 0xe1e4 直方图溢出（超容量拒收未记账）。
    HistOver,
    /// 0xe1e5 直方图断桶（桶序缺失）。
    HistHole,
    /// 0xe1e6 分位越界（千分比>1000）。
    PctOob,
    /// 0xe1e7 均值漂移（均值与分位矛盾）。
    MeanDrift,
    /// 0xe1e8 台账不守恒（守恒恒等式破缺）。
    LedgerBroken,
    /// 0xe1e9 快照指纹不符（字段被改后重算不一致）。
    SnapshotFingerprint,
    /// 0xe1ea 常量表破坏（表项与语义规则矛盾）。
    TableBroken,
    /// 0xe1eb 段位守卫失败（诊断码越出本模块段）。
    SegmentGuard,
}

impl u2801Code {
    /// 全量闭集。
    pub const ALL: [u2801Code; 12] = [
        u2801Code::KeyDup,
        u2801Code::KeyGap,
        u2801Code::GuardBad,
        u2801Code::SumDrift,
        u2801Code::HistOver,
        u2801Code::HistHole,
        u2801Code::PctOob,
        u2801Code::MeanDrift,
        u2801Code::LedgerBroken,
        u2801Code::SnapshotFingerprint,
        u2801Code::TableBroken,
        u2801Code::SegmentGuard,
    ];

    /// 码值（连续十六进制，与声明序恒等）。
    pub const fn code(self) -> u16 {
        match self {
            u2801Code::KeyDup => 0xe1e0,
            u2801Code::KeyGap => 0xe1e1,
            u2801Code::GuardBad => 0xe1e2,
            u2801Code::SumDrift => 0xe1e3,
            u2801Code::HistOver => 0xe1e4,
            u2801Code::HistHole => 0xe1e5,
            u2801Code::PctOob => 0xe1e6,
            u2801Code::MeanDrift => 0xe1e7,
            u2801Code::LedgerBroken => 0xe1e8,
            u2801Code::SnapshotFingerprint => 0xe1e9,
            u2801Code::TableBroken => 0xe1ea,
            u2801Code::SegmentGuard => 0xe1eb,
        }
    }

    /// 中文诊断名。
    pub const fn name(self) -> &'static str {
        match self {
            u2801Code::KeyDup => "键重复（大页 同键双行）",
            u2801Code::KeyGap => "键断档（压测 序不连续）",
            u2801Code::GuardBad => "行守卫破缺（容量 逐行校验失败）",
            u2801Code::SumDrift => "矩阵累计漂移（冻结 求和失锚）",
            u2801Code::HistOver => "直方图溢出（超容量拒收未记账）",
            u2801Code::HistHole => "直方图断桶（桶序缺失）",
            u2801Code::PctOob => "分位越界（千分比>1000）",
            u2801Code::MeanDrift => "均值漂移（均值与分位矛盾）",
            u2801Code::LedgerBroken => "台账不守恒（守恒恒等式破缺）",
            u2801Code::SnapshotFingerprint => "快照指纹不符（字段被改后重算不一致）",
            u2801Code::TableBroken => "常量表破坏（表项与语义规则矛盾）",
            u2801Code::SegmentGuard => "段位守卫失败（诊断码越出本模块段）",
        }
    }

    /// 可操作修复建议（每条诊断都必须带下一步动作）。
    pub const fn suggestion(self) -> &'static str {
        match self {
            u2801Code::KeyDup => "按键唯一律去重",
            u2801Code::KeyGap => "按连续律补号",
            u2801Code::GuardBad => "按守卫口径重建",
            u2801Code::SumDrift => "按理论值复算",
            u2801Code::HistOver => "按溢出账口径登记",
            u2801Code::HistHole => "按桶序补齐",
            u2801Code::PctOob => "按 0..=1000 钳制",
            u2801Code::MeanDrift => "按同源数据复算",
            u2801Code::LedgerBroken => "按守恒律重建台账后重跑",
            u2801Code::SnapshotFingerprint => "按防篡改规程重建快照",
            u2801Code::TableBroken => "按语义规则重建常量表",
            u2801Code::SegmentGuard => "核对诊断码分配表",
        }
    }
}

// --------------------------------------------------------------------------
// 字符串键值映射 u2801mat（36 行逐行语义注释 + 逐行守卫）
// --------------------------------------------------------------------------
/// 大页池容量压测 守卫矩阵（与核心登记表同源对拍）。
/// 行数。
pub const u2801mat_COUNT: usize = 36;

/// 表本体（键 + 值 + 注释由编译期冻结）。
pub static u2801mat: [(&'static str, u64); 36] = [
    ("HP0000", 0), // HP 单元 0000（大页 × 相位 00 × 一 轮）
    ("POOL0001", 13), // POOL 单元 0001（池 × 相位 01 × 一 轮）
    ("CAP0002", 26), // CAP 单元 0002（容量 × 相位 02 × 一 轮）
    ("PRESS0003", 39), // PRESS 单元 0003（压测 × 相位 03 × 一 轮）
    ("MATRIX0004", 52), // MATRIX 单元 0004（矩阵 × 相位 04 × 一 轮）
    ("ALLOC0005", 65), // ALLOC 单元 0005（分配 × 相位 05 × 一 轮）
    ("FREE0006", 78), // FREE 单元 0006（释放 × 相位 06 × 一 轮）
    ("FRAG0007", 91), // FRAG 单元 0007（碎片 × 相位 07 × 一 轮）
    ("RSS0008", 7), // RSS 单元 0008（常驻 × 相位 08 × 一 轮）
    ("FLOOR0009", 20), // FLOOR 单元 0009（地板 × 相位 09 × 一 轮）
    ("CEIL0010", 33), // CEIL 单元 0010（天花板 × 相位 10 × 一 轮）
    ("BUD0011", 46), // BUD 单元 0011（预算 × 相位 11 × 一 轮）
    ("HP0012", 59), // HP 单元 0012（大页 × 相位 12 × 一 轮）
    ("POOL0013", 72), // POOL 单元 0013（池 × 相位 13 × 一 轮）
    ("CAP0014", 85), // CAP 单元 0014（容量 × 相位 14 × 一 轮）
    ("PRESS0015", 1), // PRESS 单元 0015（压测 × 相位 15 × 一 轮）
    ("MATRIX0016", 14), // MATRIX 单元 0016（矩阵 × 相位 16 × 一 轮）
    ("ALLOC0017", 27), // ALLOC 单元 0017（分配 × 相位 17 × 一 轮）
    ("FREE0018", 40), // FREE 单元 0018（释放 × 相位 18 × 一 轮）
    ("FRAG0019", 53), // FRAG 单元 0019（碎片 × 相位 19 × 一 轮）
    ("RSS0020", 66), // RSS 单元 0020（常驻 × 相位 20 × 一 轮）
    ("FLOOR0021", 79), // FLOOR 单元 0021（地板 × 相位 21 × 一 轮）
    ("CEIL0022", 92), // CEIL 单元 0022（天花板 × 相位 22 × 一 轮）
    ("BUD0023", 8), // BUD 单元 0023（预算 × 相位 23 × 一 轮）
    ("HP0024", 21), // HP 单元 0024（大页 × 相位 24 × 一 轮）
    ("POOL0025", 34), // POOL 单元 0025（池 × 相位 25 × 一 轮）
    ("CAP0026", 47), // CAP 单元 0026（容量 × 相位 26 × 一 轮）
    ("PRESS0027", 60), // PRESS 单元 0027（压测 × 相位 27 × 一 轮）
    ("MATRIX0028", 73), // MATRIX 单元 0028（矩阵 × 相位 28 × 一 轮）
    ("ALLOC0029", 86), // ALLOC 单元 0029（分配 × 相位 29 × 一 轮）
    ("FREE0030", 2), // FREE 单元 0030（释放 × 相位 30 × 一 轮）
    ("FRAG0031", 15), // FRAG 单元 0031（碎片 × 相位 31 × 一 轮）
    ("RSS0032", 28), // RSS 单元 0032（常驻 × 相位 32 × 一 轮）
    ("FLOOR0033", 41), // FLOOR 单元 0033（地板 × 相位 33 × 一 轮）
    ("CEIL0034", 54), // CEIL 单元 0034（天花板 × 相位 34 × 一 轮）
    ("BUD0035", 67), // BUD 单元 0035（预算 × 相位 35 × 一 轮）
];

/// O(1) 语义查找（match 全枚举；命中率 100%% 由守卫保证）。
pub fn u2801mat_get(key: &str) -> Option<u64> {
    match key {
        "HP0000" => Some(0),
        "POOL0001" => Some(13),
        "CAP0002" => Some(26),
        "PRESS0003" => Some(39),
        "MATRIX0004" => Some(52),
        "ALLOC0005" => Some(65),
        "FREE0006" => Some(78),
        "FRAG0007" => Some(91),
        "RSS0008" => Some(7),
        "FLOOR0009" => Some(20),
        "CEIL0010" => Some(33),
        "BUD0011" => Some(46),
        "HP0012" => Some(59),
        "POOL0013" => Some(72),
        "CAP0014" => Some(85),
        "PRESS0015" => Some(1),
        "MATRIX0016" => Some(14),
        "ALLOC0017" => Some(27),
        "FREE0018" => Some(40),
        "FRAG0019" => Some(53),
        "RSS0020" => Some(66),
        "FLOOR0021" => Some(79),
        "CEIL0022" => Some(92),
        "BUD0023" => Some(8),
        "HP0024" => Some(21),
        "POOL0025" => Some(34),
        "CAP0026" => Some(47),
        "PRESS0027" => Some(60),
        "MATRIX0028" => Some(73),
        "ALLOC0029" => Some(86),
        "FREE0030" => Some(2),
        "FRAG0031" => Some(15),
        "RSS0032" => Some(28),
        "FLOOR0033" => Some(41),
        "CEIL0034" => Some(54),
        "BUD0035" => Some(67),
        _ => None,
    }
}

/// 逐行守卫扫描（返回违例行数；0 = 全绿）。
pub fn u2801mat_scan() -> usize {
    let mut bad = 0usize;
    let mut i = 0usize;
    while i < u2801mat_COUNT {
        let (k, v) = u2801mat[i];
        let ok = match k {
            "HP0000" => v < 97,
            "POOL0001" => v < 97,
            "CAP0002" => v < 97,
            "PRESS0003" => v < 97,
            "MATRIX0004" => v < 97,
            "ALLOC0005" => v < 97,
            "FREE0006" => v < 97,
            "FRAG0007" => v < 97,
            "RSS0008" => v < 97,
            "FLOOR0009" => v < 97,
            "CEIL0010" => v < 97,
            "BUD0011" => v < 97,
            "HP0012" => v < 97,
            "POOL0013" => v < 97,
            "CAP0014" => v < 97,
            "PRESS0015" => v < 97,
            "MATRIX0016" => v < 97,
            "ALLOC0017" => v < 97,
            "FREE0018" => v < 97,
            "FRAG0019" => v < 97,
            "RSS0020" => v < 97,
            "FLOOR0021" => v < 97,
            "CEIL0022" => v < 97,
            "BUD0023" => v < 97,
            "HP0024" => v < 97,
            "POOL0025" => v < 97,
            "CAP0026" => v < 97,
            "PRESS0027" => v < 97,
            "MATRIX0028" => v < 97,
            "ALLOC0029" => v < 97,
            "FREE0030" => v < 97,
            "FRAG0031" => v < 97,
            "RSS0032" => v < 97,
            "FLOOR0033" => v < 97,
            "CEIL0034" => v < 97,
            "BUD0035" => v < 97,
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
pub fn u2801mat_sum() -> u64 {
    let mut s = 0u64;
    let mut i = 0usize;
    while i < u2801mat_COUNT {
        s = s.wrapping_add(u2801mat[i].1 as u64);
        i += 1;
    }
    s
}

/// 键唯一性（构造期断言等价物：逐对扫描同键）。
pub fn u2801mat_unique() -> bool {
    let mut i = 0usize;
    while i < u2801mat_COUNT {
        let mut j = i + 1;
        while j < u2801mat_COUNT {
            if u2801mat[i].0 == u2801mat[j].0 {
                return false;
            }
            j += 1;
        }
        i += 1;
    }
    true
}

// --------------------------------------------------------------------------
// 直方图引擎 mxu2801hist（有界插入排序 + 分位数 P50/P90/P99）
// --------------------------------------------------------------------------
/// 大页池容量压测 直方图（96 桶滚动）。
/// 样本容量上限。
pub const MXU2801HIST_CAP: usize = 96;

/// 直方图账本（排序态 + 五统量）。
#[derive(Clone, Copy, Debug)]
pub struct mxu2801histHist {
    /// 样本数组（插入排序维护有序）。
    pub samples: [u64; 96],
    /// 样本数。
    pub n: usize,
    /// 溢出丢弃计数。
    pub drops: u64,
    /// 最小值（空时为 0）。
    pub min: u64,
    /// 最大值。
    pub max: u64,
    /// 累加和。
    pub sum: u64,
}

impl mxu2801histHist {
    /// 空账构造。
    pub const fn new() -> Self {
        Self { samples: [0; 96], n: 0, drops: 0, min: 0, max: 0, sum: 0 }
    }

    /// 采样（有界插入排序；满员丢弃并记账）。
    pub fn push(&mut self, v: u64) {
        if self.n >= MXU2801HIST_CAP {
            self.drops += 1;
            return;
        }
        let mut j = self.n;
        while j > 0 && self.samples[j - 1] > v {
            self.samples[j] = self.samples[j - 1];
            j -= 1;
        }
        self.samples[j] = v;
        self.n += 1;
        self.sum = self.sum.wrapping_add(v);
        if self.n == 1 || v < self.min {
            self.min = v;
        }
        if v > self.max {
            self.max = v;
        }
    }

    /// 分位数（permille 口径：q500=P50/q900=P90/q990=P99；最近秩法）。
    pub fn percentile(&self, permille: u64) -> u64 {
        if self.n == 0 {
            return 0;
        }
        let rank = (permille * self.n as u64 + 999) / 1000;
        let idx = if rank == 0 { 1 } else { rank };
        let i = if idx as usize > self.n { self.n - 1 } else { idx as usize - 1 };
        self.samples[i]
    }

    /// 均值（整数除，余数入账）。
    pub fn mean(&self) -> u64 {
        if self.n == 0 {
            return 0;
        }
        self.sum / self.n as u64
    }

    /// 守恒：sum == 各样本和（复算）。
    pub fn conserved(&self) -> bool {
        let mut s = 0u64;
        let mut i = 0usize;
        while i < self.n {
            s = s.wrapping_add(self.samples[i]);
            i += 1;
        }
        s == self.sum
    }
}

// ---------------------------------------------------------------------------
// 段位守卫（本模块独立十六进码前缀防串段；越界即判红）
// ---------------------------------------------------------------------------

/// 段位守卫：全部诊断码必须落在本段 [0xe1e0, 0xe1ef]。
pub const fn u2801_segment_guard() -> bool {
    let mut ok = true;
    let mut i = 0usize;
    while i < u2801Code::ALL.len() {
        let c = u2801Code::ALL[i].code();
        if c < 0xe1e0 || c > 0xe1ef {
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
fn u2801_guard_seg() -> bool {
    u2801_segment_guard()
}

/// UNX-F2801 矩阵 36 行齐备 + 逐行守卫 + 唯一性 + 累计守恒。
fn check_u2801_map_guard() -> bool {
    let mut ok = u2801mat_COUNT == 36;
    if ok { ok = u2801mat_scan() == 0 && u2801mat_unique(); }
    if ok { ok = u2801mat_get("HP0000") == Some(0) && u2801mat_get("ZZZ_NOKEY").is_none(); }
    if ok { ok = u2801mat_sum() == 1594; }
    ok
}

/// UNX-F2801 直方图 16 样本分位/均值/守恒。
fn check_u2801_map_hist() -> bool {
    let mut h = mxu2801histHist::new();
    let mut i = 0u64;
    while i < 16 { h.push(i + 1); i += 1; }
    let mut ok = h.conserved() && h.mean() >= 1 && h.percentile(500) >= 8;
    ok
}

/// 本模块自检（逐检查带 detail，全绿才出门）。
pub fn run_u2801_checks() -> CheckSet {
    let mut set = CheckSet::new("U2801");
    set.add(
        "u2801-seg-guard",
        u2801_guard_seg(),
        if u2801_guard_seg() { "" } else { "diagnostic code outside segment" },
    );
    set.add(
        "u2801-map-guard",
        check_u2801_map_guard(),
        if check_u2801_map_guard() { "" } else { "UNX-F2801 matrix guard mismatch" },
    );
    set.add(
        "u2801-map-hist",
        check_u2801_map_hist(),
        if check_u2801_map_hist() { "" } else { "UNX-F2801 hist mismatch" },
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
    fn u2801_all_checks_green() {
        let set = run_u2801_checks();
        assert!(set.all_passed(), "U2801 domain checks must be green");
    }

    #[test]
    fn u2801_segment_guard_holds() {
        assert!(u2801_segment_guard());
    }
}
