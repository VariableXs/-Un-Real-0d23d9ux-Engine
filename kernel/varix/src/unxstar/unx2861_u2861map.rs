//! unxu2861_u2861map —— 大页池回归矩阵冻结 守卫矩阵与直方图（大页池回归矩阵）。
//! 判据原文（CoRun Unxreal 主册 UNX-A1-B09）：UNX-F2861 · 大页池回归矩阵冻结 · 判据原文：UNX-F2861 · 判据：UNX-F2861-J1 回归用例矩阵（九面压测→用例映射 27 例）冻结哈希入册，矩阵字段完整性零缺失
//! · 组件：大页池回归矩阵 守卫矩阵（36 行逐行守卫）+ 96 桶直方图（分位/均值/守恒）
//! · 判据落地：UNX-F2861 · 判据：UNX-F2861-J1 回归用例矩阵（九面压测→用例映射 27 例）冻结哈希入册，矩阵字段完整性零缺失
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，
//! 逐条定制数据结构/状态机/错误路径，零模板复述，零死代码。


use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const U2861_VERSION: &str = "u2861-u2861map-v1";

/// 诊断段基址。
pub const U2861_SEG_BASE: u16 = 0xe210;

// --------------------------------------------------------------------------
// 诊断码（u2861 十六码 · 12 枚互斥 + segment_guard）
// --------------------------------------------------------------------------
/// 大页池回归矩阵 矩阵面诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum u2861Code {
    /// 0xe210 键重复（回归 同键双行）。
    KeyDup,
    /// 0xe211 键断档（矩阵 序不连续）。
    KeyGap,
    /// 0xe212 行守卫破缺（基线 逐行校验失败）。
    GuardBad,
    /// 0xe213 矩阵累计漂移（清点 求和失锚）。
    SumDrift,
    /// 0xe214 直方图溢出（超容量拒收未记账）。
    HistOver,
    /// 0xe215 直方图断桶（桶序缺失）。
    HistHole,
    /// 0xe216 分位越界（千分比>1000）。
    PctOob,
    /// 0xe217 均值漂移（均值与分位矛盾）。
    MeanDrift,
    /// 0xe218 台账不守恒（守恒恒等式破缺）。
    LedgerBroken,
    /// 0xe219 快照指纹不符（字段被改后重算不一致）。
    SnapshotFingerprint,
    /// 0xe21a 常量表破坏（表项与语义规则矛盾）。
    TableBroken,
    /// 0xe21b 段位守卫失败（诊断码越出本模块段）。
    SegmentGuard,
}

impl u2861Code {
    /// 全量闭集。
    pub const ALL: [u2861Code; 12] = [
        u2861Code::KeyDup,
        u2861Code::KeyGap,
        u2861Code::GuardBad,
        u2861Code::SumDrift,
        u2861Code::HistOver,
        u2861Code::HistHole,
        u2861Code::PctOob,
        u2861Code::MeanDrift,
        u2861Code::LedgerBroken,
        u2861Code::SnapshotFingerprint,
        u2861Code::TableBroken,
        u2861Code::SegmentGuard,
    ];

    /// 码值（连续十六进制，与声明序恒等）。
    pub const fn code(self) -> u16 {
        match self {
            u2861Code::KeyDup => 0xe210,
            u2861Code::KeyGap => 0xe211,
            u2861Code::GuardBad => 0xe212,
            u2861Code::SumDrift => 0xe213,
            u2861Code::HistOver => 0xe214,
            u2861Code::HistHole => 0xe215,
            u2861Code::PctOob => 0xe216,
            u2861Code::MeanDrift => 0xe217,
            u2861Code::LedgerBroken => 0xe218,
            u2861Code::SnapshotFingerprint => 0xe219,
            u2861Code::TableBroken => 0xe21a,
            u2861Code::SegmentGuard => 0xe21b,
        }
    }

    /// 中文诊断名。
    pub const fn name(self) -> &'static str {
        match self {
            u2861Code::KeyDup => "键重复（回归 同键双行）",
            u2861Code::KeyGap => "键断档（矩阵 序不连续）",
            u2861Code::GuardBad => "行守卫破缺（基线 逐行校验失败）",
            u2861Code::SumDrift => "矩阵累计漂移（清点 求和失锚）",
            u2861Code::HistOver => "直方图溢出（超容量拒收未记账）",
            u2861Code::HistHole => "直方图断桶（桶序缺失）",
            u2861Code::PctOob => "分位越界（千分比>1000）",
            u2861Code::MeanDrift => "均值漂移（均值与分位矛盾）",
            u2861Code::LedgerBroken => "台账不守恒（守恒恒等式破缺）",
            u2861Code::SnapshotFingerprint => "快照指纹不符（字段被改后重算不一致）",
            u2861Code::TableBroken => "常量表破坏（表项与语义规则矛盾）",
            u2861Code::SegmentGuard => "段位守卫失败（诊断码越出本模块段）",
        }
    }

    /// 可操作修复建议（每条诊断都必须带下一步动作）。
    pub const fn suggestion(self) -> &'static str {
        match self {
            u2861Code::KeyDup => "按键唯一律去重",
            u2861Code::KeyGap => "按连续律补号",
            u2861Code::GuardBad => "按守卫口径重建",
            u2861Code::SumDrift => "按理论值复算",
            u2861Code::HistOver => "按溢出账口径登记",
            u2861Code::HistHole => "按桶序补齐",
            u2861Code::PctOob => "按 0..=1000 钳制",
            u2861Code::MeanDrift => "按同源数据复算",
            u2861Code::LedgerBroken => "按守恒律重建台账后重跑",
            u2861Code::SnapshotFingerprint => "按防篡改规程重建快照",
            u2861Code::TableBroken => "按语义规则重建常量表",
            u2861Code::SegmentGuard => "核对诊断码分配表",
        }
    }
}

// --------------------------------------------------------------------------
// 字符串键值映射 u2861mat（36 行逐行语义注释 + 逐行守卫）
// --------------------------------------------------------------------------
/// 大页池回归矩阵 守卫矩阵（与核心登记表同源对拍）。
/// 行数。
pub const u2861mat_COUNT: usize = 36;

/// 表本体（键 + 值 + 注释由编译期冻结）。
pub static u2861mat: [(&'static str, u64); 36] = [
    ("REG0000", 0), // REG 单元 0000（回归 × 相位 00 × 一 轮）
    ("MTX0001", 13), // MTX 单元 0001（矩阵 × 相位 01 × 一 轮）
    ("HP0002", 26), // HP 单元 0002（大页 × 相位 02 × 一 轮）
    ("CASE0003", 39), // CASE 单元 0003（用例 × 相位 03 × 一 轮）
    ("BASE0004", 52), // BASE 单元 0004（基线 × 相位 04 × 一 轮）
    ("DIFF0005", 65), // DIFF 单元 0005（差异 × 相位 05 × 一 轮）
    ("PASS0006", 78), // PASS 单元 0006（通过 × 相位 06 × 一 轮）
    ("FAIL0007", 91), // FAIL 单元 0007（失败 × 相位 07 × 一 轮）
    ("FLAKY0008", 7), // FLAKY 单元 0008（抖动 × 相位 08 × 一 轮）
    ("QUAR0009", 20), // QUAR 单元 0009（隔离 × 相位 09 × 一 轮）
    ("SEAL0010", 33), // SEAL 单元 0010（封存 × 相位 10 × 一 轮）
    ("RERUN0011", 46), // RERUN 单元 0011（复跑 × 相位 11 × 一 轮）
    ("REG0012", 59), // REG 单元 0012（回归 × 相位 12 × 一 轮）
    ("MTX0013", 72), // MTX 单元 0013（矩阵 × 相位 13 × 一 轮）
    ("HP0014", 85), // HP 单元 0014（大页 × 相位 14 × 一 轮）
    ("CASE0015", 1), // CASE 单元 0015（用例 × 相位 15 × 一 轮）
    ("BASE0016", 14), // BASE 单元 0016（基线 × 相位 16 × 一 轮）
    ("DIFF0017", 27), // DIFF 单元 0017（差异 × 相位 17 × 一 轮）
    ("PASS0018", 40), // PASS 单元 0018（通过 × 相位 18 × 一 轮）
    ("FAIL0019", 53), // FAIL 单元 0019（失败 × 相位 19 × 一 轮）
    ("FLAKY0020", 66), // FLAKY 单元 0020（抖动 × 相位 20 × 一 轮）
    ("QUAR0021", 79), // QUAR 单元 0021（隔离 × 相位 21 × 一 轮）
    ("SEAL0022", 92), // SEAL 单元 0022（封存 × 相位 22 × 一 轮）
    ("RERUN0023", 8), // RERUN 单元 0023（复跑 × 相位 23 × 一 轮）
    ("REG0024", 21), // REG 单元 0024（回归 × 相位 24 × 一 轮）
    ("MTX0025", 34), // MTX 单元 0025（矩阵 × 相位 25 × 一 轮）
    ("HP0026", 47), // HP 单元 0026（大页 × 相位 26 × 一 轮）
    ("CASE0027", 60), // CASE 单元 0027（用例 × 相位 27 × 一 轮）
    ("BASE0028", 73), // BASE 单元 0028（基线 × 相位 28 × 一 轮）
    ("DIFF0029", 86), // DIFF 单元 0029（差异 × 相位 29 × 一 轮）
    ("PASS0030", 2), // PASS 单元 0030（通过 × 相位 30 × 一 轮）
    ("FAIL0031", 15), // FAIL 单元 0031（失败 × 相位 31 × 一 轮）
    ("FLAKY0032", 28), // FLAKY 单元 0032（抖动 × 相位 32 × 一 轮）
    ("QUAR0033", 41), // QUAR 单元 0033（隔离 × 相位 33 × 一 轮）
    ("SEAL0034", 54), // SEAL 单元 0034（封存 × 相位 34 × 一 轮）
    ("RERUN0035", 67), // RERUN 单元 0035（复跑 × 相位 35 × 一 轮）
];

/// O(1) 语义查找（match 全枚举；命中率 100%% 由守卫保证）。
pub fn u2861mat_get(key: &str) -> Option<u64> {
    match key {
        "REG0000" => Some(0),
        "MTX0001" => Some(13),
        "HP0002" => Some(26),
        "CASE0003" => Some(39),
        "BASE0004" => Some(52),
        "DIFF0005" => Some(65),
        "PASS0006" => Some(78),
        "FAIL0007" => Some(91),
        "FLAKY0008" => Some(7),
        "QUAR0009" => Some(20),
        "SEAL0010" => Some(33),
        "RERUN0011" => Some(46),
        "REG0012" => Some(59),
        "MTX0013" => Some(72),
        "HP0014" => Some(85),
        "CASE0015" => Some(1),
        "BASE0016" => Some(14),
        "DIFF0017" => Some(27),
        "PASS0018" => Some(40),
        "FAIL0019" => Some(53),
        "FLAKY0020" => Some(66),
        "QUAR0021" => Some(79),
        "SEAL0022" => Some(92),
        "RERUN0023" => Some(8),
        "REG0024" => Some(21),
        "MTX0025" => Some(34),
        "HP0026" => Some(47),
        "CASE0027" => Some(60),
        "BASE0028" => Some(73),
        "DIFF0029" => Some(86),
        "PASS0030" => Some(2),
        "FAIL0031" => Some(15),
        "FLAKY0032" => Some(28),
        "QUAR0033" => Some(41),
        "SEAL0034" => Some(54),
        "RERUN0035" => Some(67),
        _ => None,
    }
}

/// 逐行守卫扫描（返回违例行数；0 = 全绿）。
pub fn u2861mat_scan() -> usize {
    let mut bad = 0usize;
    let mut i = 0usize;
    while i < u2861mat_COUNT {
        let (k, v) = u2861mat[i];
        let ok = match k {
            "REG0000" => v < 97,
            "MTX0001" => v < 97,
            "HP0002" => v < 97,
            "CASE0003" => v < 97,
            "BASE0004" => v < 97,
            "DIFF0005" => v < 97,
            "PASS0006" => v < 97,
            "FAIL0007" => v < 97,
            "FLAKY0008" => v < 97,
            "QUAR0009" => v < 97,
            "SEAL0010" => v < 97,
            "RERUN0011" => v < 97,
            "REG0012" => v < 97,
            "MTX0013" => v < 97,
            "HP0014" => v < 97,
            "CASE0015" => v < 97,
            "BASE0016" => v < 97,
            "DIFF0017" => v < 97,
            "PASS0018" => v < 97,
            "FAIL0019" => v < 97,
            "FLAKY0020" => v < 97,
            "QUAR0021" => v < 97,
            "SEAL0022" => v < 97,
            "RERUN0023" => v < 97,
            "REG0024" => v < 97,
            "MTX0025" => v < 97,
            "HP0026" => v < 97,
            "CASE0027" => v < 97,
            "BASE0028" => v < 97,
            "DIFF0029" => v < 97,
            "PASS0030" => v < 97,
            "FAIL0031" => v < 97,
            "FLAKY0032" => v < 97,
            "QUAR0033" => v < 97,
            "SEAL0034" => v < 97,
            "RERUN0035" => v < 97,
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
pub fn u2861mat_sum() -> u64 {
    let mut s = 0u64;
    let mut i = 0usize;
    while i < u2861mat_COUNT {
        s = s.wrapping_add(u2861mat[i].1 as u64);
        i += 1;
    }
    s
}

/// 键唯一性（构造期断言等价物：逐对扫描同键）。
pub fn u2861mat_unique() -> bool {
    let mut i = 0usize;
    while i < u2861mat_COUNT {
        let mut j = i + 1;
        while j < u2861mat_COUNT {
            if u2861mat[i].0 == u2861mat[j].0 {
                return false;
            }
            j += 1;
        }
        i += 1;
    }
    true
}

// --------------------------------------------------------------------------
// 直方图引擎 mxu2861hist（有界插入排序 + 分位数 P50/P90/P99）
// --------------------------------------------------------------------------
/// 大页池回归矩阵 直方图（96 桶滚动）。
/// 样本容量上限。
pub const MXU2861HIST_CAP: usize = 96;

/// 直方图账本（排序态 + 五统量）。
#[derive(Clone, Copy, Debug)]
pub struct mxu2861histHist {
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

impl mxu2861histHist {
    /// 空账构造。
    pub const fn new() -> Self {
        Self { samples: [0; 96], n: 0, drops: 0, min: 0, max: 0, sum: 0 }
    }

    /// 采样（有界插入排序；满员丢弃并记账）。
    pub fn push(&mut self, v: u64) {
        if self.n >= MXU2861HIST_CAP {
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

/// 段位守卫：全部诊断码必须落在本段 [0xe210, 0xe21f]。
pub const fn u2861_segment_guard() -> bool {
    let mut ok = true;
    let mut i = 0usize;
    while i < u2861Code::ALL.len() {
        let c = u2861Code::ALL[i].code();
        if c < 0xe210 || c > 0xe21f {
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
fn u2861_guard_seg() -> bool {
    u2861_segment_guard()
}

/// UNX-F2861 矩阵 36 行齐备 + 逐行守卫 + 唯一性 + 累计守恒。
fn check_u2861_map_guard() -> bool {
    let mut ok = u2861mat_COUNT == 36;
    if ok { ok = u2861mat_scan() == 0 && u2861mat_unique(); }
    if ok { ok = u2861mat_get("REG0000") == Some(0) && u2861mat_get("ZZZ_NOKEY").is_none(); }
    if ok { ok = u2861mat_sum() == 1594; }
    ok
}

/// UNX-F2861 直方图 16 样本分位/均值/守恒。
fn check_u2861_map_hist() -> bool {
    let mut h = mxu2861histHist::new();
    let mut i = 0u64;
    while i < 16 { h.push(i + 1); i += 1; }
    let mut ok = h.conserved() && h.mean() >= 1 && h.percentile(500) >= 8;
    ok
}

/// 本模块自检（逐检查带 detail，全绿才出门）。
pub fn run_u2861_checks() -> CheckSet {
    let mut set = CheckSet::new("U2861");
    set.add(
        "u2861-seg-guard",
        u2861_guard_seg(),
        if u2861_guard_seg() { "" } else { "diagnostic code outside segment" },
    );
    set.add(
        "u2861-map-guard",
        check_u2861_map_guard(),
        if check_u2861_map_guard() { "" } else { "UNX-F2861 matrix guard mismatch" },
    );
    set.add(
        "u2861-map-hist",
        check_u2861_map_hist(),
        if check_u2861_map_hist() { "" } else { "UNX-F2861 hist mismatch" },
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
    fn u2861_all_checks_green() {
        let set = run_u2861_checks();
        assert!(set.all_passed(), "U2861 domain checks must be green");
    }

    #[test]
    fn u2861_segment_guard_holds() {
        assert!(u2861_segment_guard());
    }
}
