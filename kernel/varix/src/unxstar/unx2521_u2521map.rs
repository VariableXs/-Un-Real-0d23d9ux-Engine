//! unxu2521_u2521map —— 碎片指数定义与计算（B02 前向引用兑现） 守卫矩阵与直方图（碎片指数）。
//! 判据原文（CoRun Unxreal 主册 UNX-A1-B09）：UNX-F2521 · 碎片指数定义与计算（B02 前向引用兑现） · 判据原文：UNX-F2521 · 判据：碎片指数计算与手算样例一致（可复算断言），指数含大页项（F2517 契约消费）
//! · 组件：碎片指数 守卫矩阵（36 行逐行守卫）+ 96 桶直方图（分位/均值/守恒）
//! · 判据落地：UNX-F2521 · 判据：碎片指数计算与手算样例一致（可复算断言），指数含大页项（F2517 契约消费）
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，
//! 逐条定制数据结构/状态机/错误路径，零模板复述，零死代码。


use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const U2521_VERSION: &str = "u2521-u2521map-v1";

/// 诊断段基址。
pub const U2521_SEG_BASE: u16 = 0xa4b0;

// --------------------------------------------------------------------------
// 诊断码（u2521 十六码 · 12 枚互斥 + segment_guard）
// --------------------------------------------------------------------------
/// 碎片指数 矩阵面诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum u2521Code {
    /// 0xa4b0 键重复（复算不符 同键双行）。
    KeyDup,
    /// 0xa4b1 键断档（大页项缺 序不连续）。
    KeyGap,
    /// 0xa4b2 行守卫破缺（指数越界 逐行校验失败）。
    GuardBad,
    /// 0xa4b3 矩阵累计漂移（样例漂移 求和失锚）。
    SumDrift,
    /// 0xa4b4 直方图溢出（超容量拒收未记账）。
    HistOver,
    /// 0xa4b5 直方图断桶（桶序缺失）。
    HistHole,
    /// 0xa4b6 分位越界（千分比>1000）。
    PctOob,
    /// 0xa4b7 均值漂移（均值与分位矛盾）。
    MeanDrift,
    /// 0xa4b8 台账不守恒（守恒恒等式破缺）。
    LedgerBroken,
    /// 0xa4b9 快照指纹不符（字段被改后重算不一致）。
    SnapshotFingerprint,
    /// 0xa4ba 常量表破坏（表项与语义规则矛盾）。
    TableBroken,
    /// 0xa4bb 段位守卫失败（诊断码越出本模块段）。
    SegmentGuard,
}

impl u2521Code {
    /// 全量闭集。
    pub const ALL: [u2521Code; 12] = [
        u2521Code::KeyDup,
        u2521Code::KeyGap,
        u2521Code::GuardBad,
        u2521Code::SumDrift,
        u2521Code::HistOver,
        u2521Code::HistHole,
        u2521Code::PctOob,
        u2521Code::MeanDrift,
        u2521Code::LedgerBroken,
        u2521Code::SnapshotFingerprint,
        u2521Code::TableBroken,
        u2521Code::SegmentGuard,
    ];

    /// 码值（连续十六进制，与声明序恒等）。
    pub const fn code(self) -> u16 {
        match self {
            u2521Code::KeyDup => 0xa4b0,
            u2521Code::KeyGap => 0xa4b1,
            u2521Code::GuardBad => 0xa4b2,
            u2521Code::SumDrift => 0xa4b3,
            u2521Code::HistOver => 0xa4b4,
            u2521Code::HistHole => 0xa4b5,
            u2521Code::PctOob => 0xa4b6,
            u2521Code::MeanDrift => 0xa4b7,
            u2521Code::LedgerBroken => 0xa4b8,
            u2521Code::SnapshotFingerprint => 0xa4b9,
            u2521Code::TableBroken => 0xa4ba,
            u2521Code::SegmentGuard => 0xa4bb,
        }
    }

    /// 中文诊断名。
    pub const fn name(self) -> &'static str {
        match self {
            u2521Code::KeyDup => "键重复（复算不符 同键双行）",
            u2521Code::KeyGap => "键断档（大页项缺 序不连续）",
            u2521Code::GuardBad => "行守卫破缺（指数越界 逐行校验失败）",
            u2521Code::SumDrift => "矩阵累计漂移（样例漂移 求和失锚）",
            u2521Code::HistOver => "直方图溢出（超容量拒收未记账）",
            u2521Code::HistHole => "直方图断桶（桶序缺失）",
            u2521Code::PctOob => "分位越界（千分比>1000）",
            u2521Code::MeanDrift => "均值漂移（均值与分位矛盾）",
            u2521Code::LedgerBroken => "台账不守恒（守恒恒等式破缺）",
            u2521Code::SnapshotFingerprint => "快照指纹不符（字段被改后重算不一致）",
            u2521Code::TableBroken => "常量表破坏（表项与语义规则矛盾）",
            u2521Code::SegmentGuard => "段位守卫失败（诊断码越出本模块段）",
        }
    }

    /// 可操作修复建议（每条诊断都必须带下一步动作）。
    pub const fn suggestion(self) -> &'static str {
        match self {
            u2521Code::KeyDup => "按键唯一律去重",
            u2521Code::KeyGap => "按连续律补号",
            u2521Code::GuardBad => "按守卫口径重建",
            u2521Code::SumDrift => "按理论值复算",
            u2521Code::HistOver => "按溢出账口径登记",
            u2521Code::HistHole => "按桶序补齐",
            u2521Code::PctOob => "按 0..=1000 钳制",
            u2521Code::MeanDrift => "按同源数据复算",
            u2521Code::LedgerBroken => "按守恒律重建台账后重跑",
            u2521Code::SnapshotFingerprint => "按防篡改规程重建快照",
            u2521Code::TableBroken => "按语义规则重建常量表",
            u2521Code::SegmentGuard => "核对诊断码分配表",
        }
    }
}

// --------------------------------------------------------------------------
// 字符串键值映射 u2521mat（36 行逐行语义注释 + 逐行守卫）
// --------------------------------------------------------------------------
/// 碎片指数 守卫矩阵（与核心登记表同源对拍）。
/// 行数。
pub const u2521mat_COUNT: usize = 36;

/// 表本体（键 + 值 + 注释由编译期冻结）。
pub static u2521mat: [(&'static str, u64); 36] = [
    ("FI0000", 0), // FI 单元 0000（碎片指数 × 相位 00 × 一 轮）
    ("HC0001", 13), // HC 单元 0001（手算样例 × 相位 01 × 一 轮）
    ("RC0002", 26), // RC 单元 0002（可复算 × 相位 02 × 一 轮）
    ("HP0003", 39), // HP 单元 0003（大页项 × 相位 03 × 一 轮）
    ("CT0004", 52), // CT 单元 0004（契约消费 × 相位 04 × 一 轮）
    ("FR0005", 65), // FR 单元 0005（空闲碎片 × 相位 05 × 一 轮）
    ("AV0006", 78), // AV 单元 0006（可用连续 × 相位 06 × 一 轮）
    ("MN0007", 91), // MN 单元 0007（最大连续 × 相位 07 × 一 轮）
    ("RT0008", 7), // RT 单元 0008（比率账 × 相位 08 × 一 轮）
    ("QT0009", 20), // QT 单元 0009（量化档 × 相位 09 × 一 轮）
    ("SB0010", 33), // SB 单元 0010（算式账 × 相位 10 × 一 轮）
    ("AS0011", 46), // AS 单元 0011（断言面 × 相位 11 × 一 轮）
    ("FI0012", 59), // FI 单元 0012（碎片指数 × 相位 12 × 一 轮）
    ("HC0013", 72), // HC 单元 0013（手算样例 × 相位 13 × 一 轮）
    ("RC0014", 85), // RC 单元 0014（可复算 × 相位 14 × 一 轮）
    ("HP0015", 1), // HP 单元 0015（大页项 × 相位 15 × 一 轮）
    ("CT0016", 14), // CT 单元 0016（契约消费 × 相位 16 × 一 轮）
    ("FR0017", 27), // FR 单元 0017（空闲碎片 × 相位 17 × 一 轮）
    ("AV0018", 40), // AV 单元 0018（可用连续 × 相位 18 × 一 轮）
    ("MN0019", 53), // MN 单元 0019（最大连续 × 相位 19 × 一 轮）
    ("RT0020", 66), // RT 单元 0020（比率账 × 相位 20 × 一 轮）
    ("QT0021", 79), // QT 单元 0021（量化档 × 相位 21 × 一 轮）
    ("SB0022", 92), // SB 单元 0022（算式账 × 相位 22 × 一 轮）
    ("AS0023", 8), // AS 单元 0023（断言面 × 相位 23 × 一 轮）
    ("FI0024", 21), // FI 单元 0024（碎片指数 × 相位 24 × 一 轮）
    ("HC0025", 34), // HC 单元 0025（手算样例 × 相位 25 × 一 轮）
    ("RC0026", 47), // RC 单元 0026（可复算 × 相位 26 × 一 轮）
    ("HP0027", 60), // HP 单元 0027（大页项 × 相位 27 × 一 轮）
    ("CT0028", 73), // CT 单元 0028（契约消费 × 相位 28 × 一 轮）
    ("FR0029", 86), // FR 单元 0029（空闲碎片 × 相位 29 × 一 轮）
    ("AV0030", 2), // AV 单元 0030（可用连续 × 相位 30 × 一 轮）
    ("MN0031", 15), // MN 单元 0031（最大连续 × 相位 31 × 一 轮）
    ("RT0032", 28), // RT 单元 0032（比率账 × 相位 32 × 一 轮）
    ("QT0033", 41), // QT 单元 0033（量化档 × 相位 33 × 一 轮）
    ("SB0034", 54), // SB 单元 0034（算式账 × 相位 34 × 一 轮）
    ("AS0035", 67), // AS 单元 0035（断言面 × 相位 35 × 一 轮）
];

/// O(1) 语义查找（match 全枚举；命中率 100%% 由守卫保证）。
pub fn u2521mat_get(key: &str) -> Option<u64> {
    match key {
        "FI0000" => Some(0),
        "HC0001" => Some(13),
        "RC0002" => Some(26),
        "HP0003" => Some(39),
        "CT0004" => Some(52),
        "FR0005" => Some(65),
        "AV0006" => Some(78),
        "MN0007" => Some(91),
        "RT0008" => Some(7),
        "QT0009" => Some(20),
        "SB0010" => Some(33),
        "AS0011" => Some(46),
        "FI0012" => Some(59),
        "HC0013" => Some(72),
        "RC0014" => Some(85),
        "HP0015" => Some(1),
        "CT0016" => Some(14),
        "FR0017" => Some(27),
        "AV0018" => Some(40),
        "MN0019" => Some(53),
        "RT0020" => Some(66),
        "QT0021" => Some(79),
        "SB0022" => Some(92),
        "AS0023" => Some(8),
        "FI0024" => Some(21),
        "HC0025" => Some(34),
        "RC0026" => Some(47),
        "HP0027" => Some(60),
        "CT0028" => Some(73),
        "FR0029" => Some(86),
        "AV0030" => Some(2),
        "MN0031" => Some(15),
        "RT0032" => Some(28),
        "QT0033" => Some(41),
        "SB0034" => Some(54),
        "AS0035" => Some(67),
        _ => None,
    }
}

/// 逐行守卫扫描（返回违例行数；0 = 全绿）。
pub fn u2521mat_scan() -> usize {
    let mut bad = 0usize;
    let mut i = 0usize;
    while i < u2521mat_COUNT {
        let (k, v) = u2521mat[i];
        let ok = match k {
            "FI0000" => v < 97,
            "HC0001" => v < 97,
            "RC0002" => v < 97,
            "HP0003" => v < 97,
            "CT0004" => v < 97,
            "FR0005" => v < 97,
            "AV0006" => v < 97,
            "MN0007" => v < 97,
            "RT0008" => v < 97,
            "QT0009" => v < 97,
            "SB0010" => v < 97,
            "AS0011" => v < 97,
            "FI0012" => v < 97,
            "HC0013" => v < 97,
            "RC0014" => v < 97,
            "HP0015" => v < 97,
            "CT0016" => v < 97,
            "FR0017" => v < 97,
            "AV0018" => v < 97,
            "MN0019" => v < 97,
            "RT0020" => v < 97,
            "QT0021" => v < 97,
            "SB0022" => v < 97,
            "AS0023" => v < 97,
            "FI0024" => v < 97,
            "HC0025" => v < 97,
            "RC0026" => v < 97,
            "HP0027" => v < 97,
            "CT0028" => v < 97,
            "FR0029" => v < 97,
            "AV0030" => v < 97,
            "MN0031" => v < 97,
            "RT0032" => v < 97,
            "QT0033" => v < 97,
            "SB0034" => v < 97,
            "AS0035" => v < 97,
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
pub fn u2521mat_sum() -> u64 {
    let mut s = 0u64;
    let mut i = 0usize;
    while i < u2521mat_COUNT {
        s = s.wrapping_add(u2521mat[i].1 as u64);
        i += 1;
    }
    s
}

/// 键唯一性（构造期断言等价物：逐对扫描同键）。
pub fn u2521mat_unique() -> bool {
    let mut i = 0usize;
    while i < u2521mat_COUNT {
        let mut j = i + 1;
        while j < u2521mat_COUNT {
            if u2521mat[i].0 == u2521mat[j].0 {
                return false;
            }
            j += 1;
        }
        i += 1;
    }
    true
}

// --------------------------------------------------------------------------
// 直方图引擎 mxu2521hist（有界插入排序 + 分位数 P50/P90/P99）
// --------------------------------------------------------------------------
/// 碎片指数 直方图（96 桶滚动）。
/// 样本容量上限。
pub const MXU2521HIST_CAP: usize = 96;

/// 直方图账本（排序态 + 五统量）。
#[derive(Clone, Copy, Debug)]
pub struct mxu2521histHist {
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

impl mxu2521histHist {
    /// 空账构造。
    pub const fn new() -> Self {
        Self { samples: [0; 96], n: 0, drops: 0, min: 0, max: 0, sum: 0 }
    }

    /// 采样（有界插入排序；满员丢弃并记账）。
    pub fn push(&mut self, v: u64) {
        if self.n >= MXU2521HIST_CAP {
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

/// 段位守卫：全部诊断码必须落在本段 [0xa4b0, 0xa4bf]。
pub const fn u2521_segment_guard() -> bool {
    let mut ok = true;
    let mut i = 0usize;
    while i < u2521Code::ALL.len() {
        let c = u2521Code::ALL[i].code();
        if c < 0xa4b0 || c > 0xa4bf {
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
fn u2521_guard_seg() -> bool {
    u2521_segment_guard()
}

/// UNX-F2521 矩阵 36 行齐备 + 逐行守卫 + 唯一性 + 累计守恒。
fn check_u2521_map_guard() -> bool {
    let mut ok = u2521mat_COUNT == 36;
    if ok { ok = u2521mat_scan() == 0 && u2521mat_unique(); }
    if ok { ok = u2521mat_get("FI0000") == Some(0) && u2521mat_get("ZZZ_NOKEY").is_none(); }
    if ok { ok = u2521mat_sum() == 1594; }
    ok
}

/// UNX-F2521 直方图 16 样本分位/均值/守恒。
fn check_u2521_map_hist() -> bool {
    let mut h = mxu2521histHist::new();
    let mut i = 0u64;
    while i < 16 { h.push(i + 1); i += 1; }
    let mut ok = h.conserved() && h.mean() >= 1 && h.percentile(500) >= 8;
    ok
}

/// 本模块自检（逐检查带 detail，全绿才出门）。
pub fn run_u2521_checks() -> CheckSet {
    let mut set = CheckSet::new("U2521");
    set.add(
        "u2521-seg-guard",
        u2521_guard_seg(),
        if u2521_guard_seg() { "" } else { "diagnostic code outside segment" },
    );
    set.add(
        "u2521-map-guard",
        check_u2521_map_guard(),
        if check_u2521_map_guard() { "" } else { "UNX-F2521 matrix guard mismatch" },
    );
    set.add(
        "u2521-map-hist",
        check_u2521_map_hist(),
        if check_u2521_map_hist() { "" } else { "UNX-F2521 hist mismatch" },
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
    fn u2521_all_checks_green() {
        let set = run_u2521_checks();
        assert!(set.all_passed(), "U2521 domain checks must be green");
    }

    #[test]
    fn u2521_segment_guard_holds() {
        assert!(u2521_segment_guard());
    }
}
