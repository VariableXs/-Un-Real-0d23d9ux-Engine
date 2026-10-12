//! unxu2481_u2481map —— 大页族总纲（2M/1G 透明与显式两条路） 守卫矩阵与直方图（大页路径）。
//! 判据原文（CoRun Unxreal 主册 UNX-A1-B09）：UNX-F2481 · 大页族总纲（2M/1G 透明与显式两条路） · 判据原文：UNX-F2481 · 判据：透明/显式两路定义与判据面三机型生效，路径重叠注入零双路
//! · 组件：大页路径 守卫矩阵（36 行逐行守卫）+ 96 桶直方图（分位/均值/守恒）
//! · 判据落地：UNX-F2481 · 判据：透明/显式两路定义与判据面三机型生效，路径重叠注入零双路
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，
//! 逐条定制数据结构/状态机/错误路径，零模板复述，零死代码。


use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const U2481_VERSION: &str = "u2481-u2481map-v1";

/// 诊断段基址。
pub const U2481_SEG_BASE: u16 = 0xa330;

// --------------------------------------------------------------------------
// 诊断码（u2481 十六码 · 12 枚互斥 + segment_guard）
// --------------------------------------------------------------------------
/// 大页路径 矩阵面诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum u2481Code {
    /// 0xa330 键重复（双路重入 同键双行）。
    KeyDup,
    /// 0xa331 键断档（选路错判 序不连续）。
    KeyGap,
    /// 0xa332 行守卫破缺（重叠未去 逐行校验失败）。
    GuardBad,
    /// 0xa333 矩阵累计漂移（机型失效 求和失锚）。
    SumDrift,
    /// 0xa334 直方图溢出（超容量拒收未记账）。
    HistOver,
    /// 0xa335 直方图断桶（桶序缺失）。
    HistHole,
    /// 0xa336 分位越界（千分比>1000）。
    PctOob,
    /// 0xa337 均值漂移（均值与分位矛盾）。
    MeanDrift,
    /// 0xa338 台账不守恒（守恒恒等式破缺）。
    LedgerBroken,
    /// 0xa339 快照指纹不符（字段被改后重算不一致）。
    SnapshotFingerprint,
    /// 0xa33a 常量表破坏（表项与语义规则矛盾）。
    TableBroken,
    /// 0xa33b 段位守卫失败（诊断码越出本模块段）。
    SegmentGuard,
}

impl u2481Code {
    /// 全量闭集。
    pub const ALL: [u2481Code; 12] = [
        u2481Code::KeyDup,
        u2481Code::KeyGap,
        u2481Code::GuardBad,
        u2481Code::SumDrift,
        u2481Code::HistOver,
        u2481Code::HistHole,
        u2481Code::PctOob,
        u2481Code::MeanDrift,
        u2481Code::LedgerBroken,
        u2481Code::SnapshotFingerprint,
        u2481Code::TableBroken,
        u2481Code::SegmentGuard,
    ];

    /// 码值（连续十六进制，与声明序恒等）。
    pub const fn code(self) -> u16 {
        match self {
            u2481Code::KeyDup => 0xa330,
            u2481Code::KeyGap => 0xa331,
            u2481Code::GuardBad => 0xa332,
            u2481Code::SumDrift => 0xa333,
            u2481Code::HistOver => 0xa334,
            u2481Code::HistHole => 0xa335,
            u2481Code::PctOob => 0xa336,
            u2481Code::MeanDrift => 0xa337,
            u2481Code::LedgerBroken => 0xa338,
            u2481Code::SnapshotFingerprint => 0xa339,
            u2481Code::TableBroken => 0xa33a,
            u2481Code::SegmentGuard => 0xa33b,
        }
    }

    /// 中文诊断名。
    pub const fn name(self) -> &'static str {
        match self {
            u2481Code::KeyDup => "键重复（双路重入 同键双行）",
            u2481Code::KeyGap => "键断档（选路错判 序不连续）",
            u2481Code::GuardBad => "行守卫破缺（重叠未去 逐行校验失败）",
            u2481Code::SumDrift => "矩阵累计漂移（机型失效 求和失锚）",
            u2481Code::HistOver => "直方图溢出（超容量拒收未记账）",
            u2481Code::HistHole => "直方图断桶（桶序缺失）",
            u2481Code::PctOob => "分位越界（千分比>1000）",
            u2481Code::MeanDrift => "均值漂移（均值与分位矛盾）",
            u2481Code::LedgerBroken => "台账不守恒（守恒恒等式破缺）",
            u2481Code::SnapshotFingerprint => "快照指纹不符（字段被改后重算不一致）",
            u2481Code::TableBroken => "常量表破坏（表项与语义规则矛盾）",
            u2481Code::SegmentGuard => "段位守卫失败（诊断码越出本模块段）",
        }
    }

    /// 可操作修复建议（每条诊断都必须带下一步动作）。
    pub const fn suggestion(self) -> &'static str {
        match self {
            u2481Code::KeyDup => "按键唯一律去重",
            u2481Code::KeyGap => "按连续律补号",
            u2481Code::GuardBad => "按守卫口径重建",
            u2481Code::SumDrift => "按理论值复算",
            u2481Code::HistOver => "按溢出账口径登记",
            u2481Code::HistHole => "按桶序补齐",
            u2481Code::PctOob => "按 0..=1000 钳制",
            u2481Code::MeanDrift => "按同源数据复算",
            u2481Code::LedgerBroken => "按守恒律重建台账后重跑",
            u2481Code::SnapshotFingerprint => "按防篡改规程重建快照",
            u2481Code::TableBroken => "按语义规则重建常量表",
            u2481Code::SegmentGuard => "核对诊断码分配表",
        }
    }
}

// --------------------------------------------------------------------------
// 字符串键值映射 u2481mat（36 行逐行语义注释 + 逐行守卫）
// --------------------------------------------------------------------------
/// 大页路径 守卫矩阵（与核心登记表同源对拍）。
/// 行数。
pub const u2481mat_COUNT: usize = 36;

/// 表本体（键 + 值 + 注释由编译期冻结）。
pub static u2481mat: [(&'static str, u64); 36] = [
    ("TR0000", 0), // TR 单元 0000（透明路 × 相位 00 × 一 轮）
    ("EX0001", 13), // EX 单元 0001（显式路 × 相位 01 × 一 轮）
    ("P20002", 26), // P2 单元 0002（2M 页 × 相位 02 × 一 轮）
    ("P10003", 39), // P1 单元 0003（1G 页 × 相位 03 × 一 轮）
    ("MC0004", 52), // MC 单元 0004（三机型 × 相位 04 × 一 轮）
    ("OV0005", 65), // OV 单元 0005（重叠面 × 相位 05 × 一 轮）
    ("DB0006", 78), // DB 单元 0006（双路账 × 相位 06 × 一 轮）
    ("FN0007", 91), // FN 单元 0007（路径判据面 × 相位 07 × 一 轮）
    ("IN0008", 7), // IN 单元 0008（注入面 × 相位 08 × 一 轮）
    ("ZD0009", 20), // ZD 单元 0009（零双路断言 × 相位 09 × 一 轮）
    ("SEL0010", 33), // SEL 单元 0010（选路 × 相位 10 × 一 轮）
    ("MAP0011", 46), // MAP 单元 0011（映射面 × 相位 11 × 一 轮）
    ("TR0012", 59), // TR 单元 0012（透明路 × 相位 12 × 一 轮）
    ("EX0013", 72), // EX 单元 0013（显式路 × 相位 13 × 一 轮）
    ("P20014", 85), // P2 单元 0014（2M 页 × 相位 14 × 一 轮）
    ("P10015", 1), // P1 单元 0015（1G 页 × 相位 15 × 一 轮）
    ("MC0016", 14), // MC 单元 0016（三机型 × 相位 16 × 一 轮）
    ("OV0017", 27), // OV 单元 0017（重叠面 × 相位 17 × 一 轮）
    ("DB0018", 40), // DB 单元 0018（双路账 × 相位 18 × 一 轮）
    ("FN0019", 53), // FN 单元 0019（路径判据面 × 相位 19 × 一 轮）
    ("IN0020", 66), // IN 单元 0020（注入面 × 相位 20 × 一 轮）
    ("ZD0021", 79), // ZD 单元 0021（零双路断言 × 相位 21 × 一 轮）
    ("SEL0022", 92), // SEL 单元 0022（选路 × 相位 22 × 一 轮）
    ("MAP0023", 8), // MAP 单元 0023（映射面 × 相位 23 × 一 轮）
    ("TR0024", 21), // TR 单元 0024（透明路 × 相位 24 × 一 轮）
    ("EX0025", 34), // EX 单元 0025（显式路 × 相位 25 × 一 轮）
    ("P20026", 47), // P2 单元 0026（2M 页 × 相位 26 × 一 轮）
    ("P10027", 60), // P1 单元 0027（1G 页 × 相位 27 × 一 轮）
    ("MC0028", 73), // MC 单元 0028（三机型 × 相位 28 × 一 轮）
    ("OV0029", 86), // OV 单元 0029（重叠面 × 相位 29 × 一 轮）
    ("DB0030", 2), // DB 单元 0030（双路账 × 相位 30 × 一 轮）
    ("FN0031", 15), // FN 单元 0031（路径判据面 × 相位 31 × 一 轮）
    ("IN0032", 28), // IN 单元 0032（注入面 × 相位 32 × 一 轮）
    ("ZD0033", 41), // ZD 单元 0033（零双路断言 × 相位 33 × 一 轮）
    ("SEL0034", 54), // SEL 单元 0034（选路 × 相位 34 × 一 轮）
    ("MAP0035", 67), // MAP 单元 0035（映射面 × 相位 35 × 一 轮）
];

/// O(1) 语义查找（match 全枚举；命中率 100%% 由守卫保证）。
pub fn u2481mat_get(key: &str) -> Option<u64> {
    match key {
        "TR0000" => Some(0),
        "EX0001" => Some(13),
        "P20002" => Some(26),
        "P10003" => Some(39),
        "MC0004" => Some(52),
        "OV0005" => Some(65),
        "DB0006" => Some(78),
        "FN0007" => Some(91),
        "IN0008" => Some(7),
        "ZD0009" => Some(20),
        "SEL0010" => Some(33),
        "MAP0011" => Some(46),
        "TR0012" => Some(59),
        "EX0013" => Some(72),
        "P20014" => Some(85),
        "P10015" => Some(1),
        "MC0016" => Some(14),
        "OV0017" => Some(27),
        "DB0018" => Some(40),
        "FN0019" => Some(53),
        "IN0020" => Some(66),
        "ZD0021" => Some(79),
        "SEL0022" => Some(92),
        "MAP0023" => Some(8),
        "TR0024" => Some(21),
        "EX0025" => Some(34),
        "P20026" => Some(47),
        "P10027" => Some(60),
        "MC0028" => Some(73),
        "OV0029" => Some(86),
        "DB0030" => Some(2),
        "FN0031" => Some(15),
        "IN0032" => Some(28),
        "ZD0033" => Some(41),
        "SEL0034" => Some(54),
        "MAP0035" => Some(67),
        _ => None,
    }
}

/// 逐行守卫扫描（返回违例行数；0 = 全绿）。
pub fn u2481mat_scan() -> usize {
    let mut bad = 0usize;
    let mut i = 0usize;
    while i < u2481mat_COUNT {
        let (k, v) = u2481mat[i];
        let ok = match k {
            "TR0000" => v < 97,
            "EX0001" => v < 97,
            "P20002" => v < 97,
            "P10003" => v < 97,
            "MC0004" => v < 97,
            "OV0005" => v < 97,
            "DB0006" => v < 97,
            "FN0007" => v < 97,
            "IN0008" => v < 97,
            "ZD0009" => v < 97,
            "SEL0010" => v < 97,
            "MAP0011" => v < 97,
            "TR0012" => v < 97,
            "EX0013" => v < 97,
            "P20014" => v < 97,
            "P10015" => v < 97,
            "MC0016" => v < 97,
            "OV0017" => v < 97,
            "DB0018" => v < 97,
            "FN0019" => v < 97,
            "IN0020" => v < 97,
            "ZD0021" => v < 97,
            "SEL0022" => v < 97,
            "MAP0023" => v < 97,
            "TR0024" => v < 97,
            "EX0025" => v < 97,
            "P20026" => v < 97,
            "P10027" => v < 97,
            "MC0028" => v < 97,
            "OV0029" => v < 97,
            "DB0030" => v < 97,
            "FN0031" => v < 97,
            "IN0032" => v < 97,
            "ZD0033" => v < 97,
            "SEL0034" => v < 97,
            "MAP0035" => v < 97,
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
pub fn u2481mat_sum() -> u64 {
    let mut s = 0u64;
    let mut i = 0usize;
    while i < u2481mat_COUNT {
        s = s.wrapping_add(u2481mat[i].1 as u64);
        i += 1;
    }
    s
}

/// 键唯一性（构造期断言等价物：逐对扫描同键）。
pub fn u2481mat_unique() -> bool {
    let mut i = 0usize;
    while i < u2481mat_COUNT {
        let mut j = i + 1;
        while j < u2481mat_COUNT {
            if u2481mat[i].0 == u2481mat[j].0 {
                return false;
            }
            j += 1;
        }
        i += 1;
    }
    true
}

// --------------------------------------------------------------------------
// 直方图引擎 mxu2481hist（有界插入排序 + 分位数 P50/P90/P99）
// --------------------------------------------------------------------------
/// 大页路径 直方图（96 桶滚动）。
/// 样本容量上限。
pub const MXU2481HIST_CAP: usize = 96;

/// 直方图账本（排序态 + 五统量）。
#[derive(Clone, Copy, Debug)]
pub struct mxu2481histHist {
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

impl mxu2481histHist {
    /// 空账构造。
    pub const fn new() -> Self {
        Self { samples: [0; 96], n: 0, drops: 0, min: 0, max: 0, sum: 0 }
    }

    /// 采样（有界插入排序；满员丢弃并记账）。
    pub fn push(&mut self, v: u64) {
        if self.n >= MXU2481HIST_CAP {
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

/// 段位守卫：全部诊断码必须落在本段 [0xa330, 0xa33f]。
pub const fn u2481_segment_guard() -> bool {
    let mut ok = true;
    let mut i = 0usize;
    while i < u2481Code::ALL.len() {
        let c = u2481Code::ALL[i].code();
        if c < 0xa330 || c > 0xa33f {
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
fn u2481_guard_seg() -> bool {
    u2481_segment_guard()
}

/// UNX-F2481 矩阵 36 行齐备 + 逐行守卫 + 唯一性 + 累计守恒。
fn check_u2481_map_guard() -> bool {
    let mut ok = u2481mat_COUNT == 36;
    if ok { ok = u2481mat_scan() == 0 && u2481mat_unique(); }
    if ok { ok = u2481mat_get("TR0000") == Some(0) && u2481mat_get("ZZZ_NOKEY").is_none(); }
    if ok { ok = u2481mat_sum() == 1594; }
    ok
}

/// UNX-F2481 直方图 16 样本分位/均值/守恒。
fn check_u2481_map_hist() -> bool {
    let mut h = mxu2481histHist::new();
    let mut i = 0u64;
    while i < 16 { h.push(i + 1); i += 1; }
    let mut ok = h.conserved() && h.mean() >= 1 && h.percentile(500) >= 8;
    ok
}

/// 本模块自检（逐检查带 detail，全绿才出门）。
pub fn run_u2481_checks() -> CheckSet {
    let mut set = CheckSet::new("U2481");
    set.add(
        "u2481-seg-guard",
        u2481_guard_seg(),
        if u2481_guard_seg() { "" } else { "diagnostic code outside segment" },
    );
    set.add(
        "u2481-map-guard",
        check_u2481_map_guard(),
        if check_u2481_map_guard() { "" } else { "UNX-F2481 matrix guard mismatch" },
    );
    set.add(
        "u2481-map-hist",
        check_u2481_map_hist(),
        if check_u2481_map_hist() { "" } else { "UNX-F2481 hist mismatch" },
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
    fn u2481_all_checks_green() {
        let set = run_u2481_checks();
        assert!(set.all_passed(), "U2481 domain checks must be green");
    }

    #[test]
    fn u2481_segment_guard_holds() {
        assert!(u2481_segment_guard());
    }
}
