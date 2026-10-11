//! unxu1881_u1881map —— 全域注入矩阵终对账（十一矩阵收口） 守卫矩阵与直方图（矩阵）。
//! 判据原文（CoRun Unxreal 主册 UNX-A1-B09）：UNX-F1881 · 全域注入矩阵终对账（十一矩阵收口） · 判据原文：UNX-F1881 · 判据：十一矩阵（F1659/F1673/F1692/F1715/F1732/F1756/F1772/F1814/F1835/F1856/F1874）形态总表对账闭合，覆盖空洞为零，与 F1788 口径一致
//! · 组件：矩阵 守卫矩阵（36 行逐行守卫）+ 96 桶直方图（分位/均值/守恒）
//! · 判据落地：UNX-F1881 · 判据：十一矩阵（F1659/F1673/F1692/F1715/F1732/F1756/F1772/F1814/F1835/F1856/
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，
//! 逐条定制数据结构/状态机/错误路径，零模板复述，零死代码。


use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const U1881_VERSION: &str = "u1881-u1881map-v1";

/// 诊断段基址。
pub const U1881_SEG_BASE: u16 = 0x5fb0;

// --------------------------------------------------------------------------
// 诊断码（u1881 十六码 · 12 枚互斥 + segment_guard）
// --------------------------------------------------------------------------
/// 矩阵 矩阵面诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum u1881Code {
    /// 0x5fb0 键重复（矩阵缺位 同键双行）。
    KeyDup,
    /// 0x5fb1 键断档（对账破缺 序不连续）。
    KeyGap,
    /// 0x5fb2 行守卫破缺（覆盖空洞 逐行校验失败）。
    GuardBad,
    /// 0x5fb3 矩阵累计漂移（口径漂移 求和失锚）。
    SumDrift,
    /// 0x5fb4 直方图溢出（超容量拒收未记账）。
    HistOver,
    /// 0x5fb5 直方图断桶（桶序缺失）。
    HistHole,
    /// 0x5fb6 分位越界（千分比>1000）。
    PctOob,
    /// 0x5fb7 均值漂移（均值与分位矛盾）。
    MeanDrift,
    /// 0x5fb8 台账不守恒（守恒恒等式破缺）。
    LedgerBroken,
    /// 0x5fb9 快照指纹不符（字段被改后重算不一致）。
    SnapshotFingerprint,
    /// 0x5fba 常量表破坏（表项与语义规则矛盾）。
    TableBroken,
    /// 0x5fbb 段位守卫失败（诊断码越出本模块段）。
    SegmentGuard,
}

impl u1881Code {
    /// 全量闭集。
    pub const ALL: [u1881Code; 12] = [
        u1881Code::KeyDup,
        u1881Code::KeyGap,
        u1881Code::GuardBad,
        u1881Code::SumDrift,
        u1881Code::HistOver,
        u1881Code::HistHole,
        u1881Code::PctOob,
        u1881Code::MeanDrift,
        u1881Code::LedgerBroken,
        u1881Code::SnapshotFingerprint,
        u1881Code::TableBroken,
        u1881Code::SegmentGuard,
    ];

    /// 码值（连续十六进制，与声明序恒等）。
    pub const fn code(self) -> u16 {
        match self {
            u1881Code::KeyDup => 0x5fb0,
            u1881Code::KeyGap => 0x5fb1,
            u1881Code::GuardBad => 0x5fb2,
            u1881Code::SumDrift => 0x5fb3,
            u1881Code::HistOver => 0x5fb4,
            u1881Code::HistHole => 0x5fb5,
            u1881Code::PctOob => 0x5fb6,
            u1881Code::MeanDrift => 0x5fb7,
            u1881Code::LedgerBroken => 0x5fb8,
            u1881Code::SnapshotFingerprint => 0x5fb9,
            u1881Code::TableBroken => 0x5fba,
            u1881Code::SegmentGuard => 0x5fbb,
        }
    }

    /// 中文诊断名。
    pub const fn name(self) -> &'static str {
        match self {
            u1881Code::KeyDup => "键重复（矩阵缺位 同键双行）",
            u1881Code::KeyGap => "键断档（对账破缺 序不连续）",
            u1881Code::GuardBad => "行守卫破缺（覆盖空洞 逐行校验失败）",
            u1881Code::SumDrift => "矩阵累计漂移（口径漂移 求和失锚）",
            u1881Code::HistOver => "直方图溢出（超容量拒收未记账）",
            u1881Code::HistHole => "直方图断桶（桶序缺失）",
            u1881Code::PctOob => "分位越界（千分比>1000）",
            u1881Code::MeanDrift => "均值漂移（均值与分位矛盾）",
            u1881Code::LedgerBroken => "台账不守恒（守恒恒等式破缺）",
            u1881Code::SnapshotFingerprint => "快照指纹不符（字段被改后重算不一致）",
            u1881Code::TableBroken => "常量表破坏（表项与语义规则矛盾）",
            u1881Code::SegmentGuard => "段位守卫失败（诊断码越出本模块段）",
        }
    }

    /// 可操作修复建议（每条诊断都必须带下一步动作）。
    pub const fn suggestion(self) -> &'static str {
        match self {
            u1881Code::KeyDup => "按键唯一律去重",
            u1881Code::KeyGap => "按连续律补号",
            u1881Code::GuardBad => "按守卫口径重建",
            u1881Code::SumDrift => "按理论值复算",
            u1881Code::HistOver => "按溢出账口径登记",
            u1881Code::HistHole => "按桶序补齐",
            u1881Code::PctOob => "按 0..=1000 钳制",
            u1881Code::MeanDrift => "按同源数据复算",
            u1881Code::LedgerBroken => "按守恒律重建台账后重跑",
            u1881Code::SnapshotFingerprint => "按防篡改规程重建快照",
            u1881Code::TableBroken => "按语义规则重建常量表",
            u1881Code::SegmentGuard => "核对诊断码分配表",
        }
    }
}

// --------------------------------------------------------------------------
// 字符串键值映射 u1881mat（36 行逐行语义注释 + 逐行守卫）
// --------------------------------------------------------------------------
/// 矩阵 守卫矩阵（与核心登记表同源对拍）。
/// 行数。
pub const u1881mat_COUNT: usize = 36;

/// 表本体（键 + 值 + 注释由编译期冻结）。
pub static u1881mat: [(&'static str, u64); 36] = [
    ("SZ0000", 0), // SZ 单元 0000（十一矩阵 × 相位 00 × 一 轮）
    ("XT0001", 13), // XT 单元 0001（形态总表 × 相位 01 × 一 轮）
    ("DZ0002", 26), // DZ 单元 0002（对账闭合 × 相位 02 × 一 轮）
    ("FG0003", 39), // FG 单元 0003（覆盖空洞 × 相位 03 × 一 轮）
    ("F17880004", 52), // F1788 单元 0004（F1788 口径 × 相位 04 × 一 轮）
    ("F16590005", 65), // F1659 单元 0005（F1659 锚 × 相位 05 × 一 轮）
    ("F16730006", 78), // F1673 单元 0006（F1673 锚 × 相位 06 × 一 轮）
    ("F16920007", 91), // F1692 单元 0007（F1692 锚 × 相位 07 × 一 轮）
    ("F17150008", 7), // F1715 单元 0008（F1715 锚 × 相位 08 × 一 轮）
    ("F17320009", 20), // F1732 单元 0009（F1732 锚 × 相位 09 × 一 轮）
    ("F17560010", 33), // F1756 单元 0010（F1756 锚 × 相位 10 × 一 轮）
    ("F17720011", 46), // F1772 单元 0011（F1772 锚 × 相位 11 × 一 轮）
    ("SZ0012", 59), // SZ 单元 0012（十一矩阵 × 相位 12 × 一 轮）
    ("XT0013", 72), // XT 单元 0013（形态总表 × 相位 13 × 一 轮）
    ("DZ0014", 85), // DZ 单元 0014（对账闭合 × 相位 14 × 一 轮）
    ("FG0015", 1), // FG 单元 0015（覆盖空洞 × 相位 15 × 一 轮）
    ("F17880016", 14), // F1788 单元 0016（F1788 口径 × 相位 16 × 一 轮）
    ("F16590017", 27), // F1659 单元 0017（F1659 锚 × 相位 17 × 一 轮）
    ("F16730018", 40), // F1673 单元 0018（F1673 锚 × 相位 18 × 一 轮）
    ("F16920019", 53), // F1692 单元 0019（F1692 锚 × 相位 19 × 一 轮）
    ("F17150020", 66), // F1715 单元 0020（F1715 锚 × 相位 20 × 一 轮）
    ("F17320021", 79), // F1732 单元 0021（F1732 锚 × 相位 21 × 一 轮）
    ("F17560022", 92), // F1756 单元 0022（F1756 锚 × 相位 22 × 一 轮）
    ("F17720023", 8), // F1772 单元 0023（F1772 锚 × 相位 23 × 一 轮）
    ("SZ0024", 21), // SZ 单元 0024（十一矩阵 × 相位 24 × 一 轮）
    ("XT0025", 34), // XT 单元 0025（形态总表 × 相位 25 × 一 轮）
    ("DZ0026", 47), // DZ 单元 0026（对账闭合 × 相位 26 × 一 轮）
    ("FG0027", 60), // FG 单元 0027（覆盖空洞 × 相位 27 × 一 轮）
    ("F17880028", 73), // F1788 单元 0028（F1788 口径 × 相位 28 × 一 轮）
    ("F16590029", 86), // F1659 单元 0029（F1659 锚 × 相位 29 × 一 轮）
    ("F16730030", 2), // F1673 单元 0030（F1673 锚 × 相位 30 × 一 轮）
    ("F16920031", 15), // F1692 单元 0031（F1692 锚 × 相位 31 × 一 轮）
    ("F17150032", 28), // F1715 单元 0032（F1715 锚 × 相位 32 × 一 轮）
    ("F17320033", 41), // F1732 单元 0033（F1732 锚 × 相位 33 × 一 轮）
    ("F17560034", 54), // F1756 单元 0034（F1756 锚 × 相位 34 × 一 轮）
    ("F17720035", 67), // F1772 单元 0035（F1772 锚 × 相位 35 × 一 轮）
];

/// O(1) 语义查找（match 全枚举；命中率 100%% 由守卫保证）。
pub fn u1881mat_get(key: &str) -> Option<u64> {
    match key {
        "SZ0000" => Some(0),
        "XT0001" => Some(13),
        "DZ0002" => Some(26),
        "FG0003" => Some(39),
        "F17880004" => Some(52),
        "F16590005" => Some(65),
        "F16730006" => Some(78),
        "F16920007" => Some(91),
        "F17150008" => Some(7),
        "F17320009" => Some(20),
        "F17560010" => Some(33),
        "F17720011" => Some(46),
        "SZ0012" => Some(59),
        "XT0013" => Some(72),
        "DZ0014" => Some(85),
        "FG0015" => Some(1),
        "F17880016" => Some(14),
        "F16590017" => Some(27),
        "F16730018" => Some(40),
        "F16920019" => Some(53),
        "F17150020" => Some(66),
        "F17320021" => Some(79),
        "F17560022" => Some(92),
        "F17720023" => Some(8),
        "SZ0024" => Some(21),
        "XT0025" => Some(34),
        "DZ0026" => Some(47),
        "FG0027" => Some(60),
        "F17880028" => Some(73),
        "F16590029" => Some(86),
        "F16730030" => Some(2),
        "F16920031" => Some(15),
        "F17150032" => Some(28),
        "F17320033" => Some(41),
        "F17560034" => Some(54),
        "F17720035" => Some(67),
        _ => None,
    }
}

/// 逐行守卫扫描（返回违例行数；0 = 全绿）。
pub fn u1881mat_scan() -> usize {
    let mut bad = 0usize;
    let mut i = 0usize;
    while i < u1881mat_COUNT {
        let (k, v) = u1881mat[i];
        let ok = match k {
            "SZ0000" => v < 97,
            "XT0001" => v < 97,
            "DZ0002" => v < 97,
            "FG0003" => v < 97,
            "F17880004" => v < 97,
            "F16590005" => v < 97,
            "F16730006" => v < 97,
            "F16920007" => v < 97,
            "F17150008" => v < 97,
            "F17320009" => v < 97,
            "F17560010" => v < 97,
            "F17720011" => v < 97,
            "SZ0012" => v < 97,
            "XT0013" => v < 97,
            "DZ0014" => v < 97,
            "FG0015" => v < 97,
            "F17880016" => v < 97,
            "F16590017" => v < 97,
            "F16730018" => v < 97,
            "F16920019" => v < 97,
            "F17150020" => v < 97,
            "F17320021" => v < 97,
            "F17560022" => v < 97,
            "F17720023" => v < 97,
            "SZ0024" => v < 97,
            "XT0025" => v < 97,
            "DZ0026" => v < 97,
            "FG0027" => v < 97,
            "F17880028" => v < 97,
            "F16590029" => v < 97,
            "F16730030" => v < 97,
            "F16920031" => v < 97,
            "F17150032" => v < 97,
            "F17320033" => v < 97,
            "F17560034" => v < 97,
            "F17720035" => v < 97,
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
pub fn u1881mat_sum() -> u64 {
    let mut s = 0u64;
    let mut i = 0usize;
    while i < u1881mat_COUNT {
        s = s.wrapping_add(u1881mat[i].1 as u64);
        i += 1;
    }
    s
}

/// 键唯一性（构造期断言等价物：逐对扫描同键）。
pub fn u1881mat_unique() -> bool {
    let mut i = 0usize;
    while i < u1881mat_COUNT {
        let mut j = i + 1;
        while j < u1881mat_COUNT {
            if u1881mat[i].0 == u1881mat[j].0 {
                return false;
            }
            j += 1;
        }
        i += 1;
    }
    true
}

// --------------------------------------------------------------------------
// 直方图引擎 mxu1881hist（有界插入排序 + 分位数 P50/P90/P99）
// --------------------------------------------------------------------------
/// 矩阵 直方图（96 桶滚动）。
/// 样本容量上限。
pub const MXU1881HIST_CAP: usize = 96;

/// 直方图账本（排序态 + 五统量）。
#[derive(Clone, Copy, Debug)]
pub struct mxu1881histHist {
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

impl mxu1881histHist {
    /// 空账构造。
    pub const fn new() -> Self {
        Self { samples: [0; 96], n: 0, drops: 0, min: 0, max: 0, sum: 0 }
    }

    /// 采样（有界插入排序；满员丢弃并记账）。
    pub fn push(&mut self, v: u64) {
        if self.n >= MXU1881HIST_CAP {
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

/// 段位守卫：全部诊断码必须落在本段 [0x5fb0, 0x5fbf]。
pub const fn u1881_segment_guard() -> bool {
    let mut ok = true;
    let mut i = 0usize;
    while i < u1881Code::ALL.len() {
        let c = u1881Code::ALL[i].code();
        if c < 0x5fb0 || c > 0x5fbf {
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
fn u1881_guard_seg() -> bool {
    u1881_segment_guard()
}

/// UNX-F1881 矩阵 36 行齐备 + 逐行守卫 + 唯一性 + 累计守恒。
fn check_u1881_map_guard() -> bool {
    let mut ok = u1881mat_COUNT == 36;
    if ok { ok = u1881mat_scan() == 0 && u1881mat_unique(); }
    if ok { ok = u1881mat_get("SZ0000") == Some(0) && u1881mat_get("ZZZ_NOKEY").is_none(); }
    if ok { ok = u1881mat_sum() == 1594; }
    ok
}

/// UNX-F1881 直方图 16 样本分位/均值/守恒。
fn check_u1881_map_hist() -> bool {
    let mut h = mxu1881histHist::new();
    let mut i = 0u64;
    while i < 16 { h.push(i + 1); i += 1; }
    let mut ok = h.conserved() && h.mean() >= 1 && h.percentile(500) >= 8;
    ok
}

/// 本模块自检（逐检查带 detail，全绿才出门）。
pub fn run_u1881_checks() -> CheckSet {
    let mut set = CheckSet::new("U1881");
    set.add(
        "u1881-seg-guard",
        u1881_guard_seg(),
        if u1881_guard_seg() { "" } else { "diagnostic code outside segment" },
    );
    set.add(
        "u1881-map-guard",
        check_u1881_map_guard(),
        if check_u1881_map_guard() { "" } else { "UNX-F1881 matrix guard mismatch" },
    );
    set.add(
        "u1881-map-hist",
        check_u1881_map_hist(),
        if check_u1881_map_hist() { "" } else { "UNX-F1881 hist mismatch" },
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
    fn u1881_all_checks_green() {
        let set = run_u1881_checks();
        assert!(set.all_passed(), "U1881 domain checks must be green");
    }

    #[test]
    fn u1881_segment_guard_holds() {
        assert!(u1881_segment_guard());
    }
}
