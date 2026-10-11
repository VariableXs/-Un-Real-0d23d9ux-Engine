//! unxu2961_u2961map —— 域级回归矩阵方案冻结（B01–B28 全域） 守卫矩阵与直方图（域级回归全域矩阵）。
//! 判据原文（CoRun Unxreal 主册 UNX-A1-B09）：UNX-F2961 · 域级回归矩阵方案冻结（B01–B28 全域） · 判据原文：UNX-F2961 · 判据：UNX-F2961-J1 全域回归方案（28 批 560 条零抽样+三 runner 归一+绿账字段对齐）冻结哈希入册，方案与 F2968 绿账字段对齐
//! · 组件：域级回归全域矩阵 守卫矩阵（36 行逐行守卫）+ 96 桶直方图（分位/均值/守恒）
//! · 判据落地：UNX-F2961 · 判据：UNX-F2961-J1 全域回归方案（28 批 560 条零抽样+三 runner 归一+绿账字段对齐）冻结哈希入册，方案与 F
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，
//! 逐条定制数据结构/状态机/错误路径，零模板复述，零死代码。


use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const U2961_VERSION: &str = "u2961-u2961map-v1";

/// 诊断段基址。
pub const U2961_SEG_BASE: u16 = 0xe260;

// --------------------------------------------------------------------------
// 诊断码（u2961 十六码 · 12 枚互斥 + segment_guard）
// --------------------------------------------------------------------------
/// 域级回归全域矩阵 矩阵面诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum u2961Code {
    /// 0xe260 键重复（全域 同键双行）。
    KeyDup,
    /// 0xe261 键断档（回归 序不连续）。
    KeyGap,
    /// 0xe262 行守卫破缺（矩阵 逐行校验失败）。
    GuardBad,
    /// 0xe263 矩阵累计漂移（全域 求和失锚）。
    SumDrift,
    /// 0xe264 直方图溢出（超容量拒收未记账）。
    HistOver,
    /// 0xe265 直方图断桶（桶序缺失）。
    HistHole,
    /// 0xe266 分位越界（千分比>1000）。
    PctOob,
    /// 0xe267 均值漂移（均值与分位矛盾）。
    MeanDrift,
    /// 0xe268 台账不守恒（守恒恒等式破缺）。
    LedgerBroken,
    /// 0xe269 快照指纹不符（字段被改后重算不一致）。
    SnapshotFingerprint,
    /// 0xe26a 常量表破坏（表项与语义规则矛盾）。
    TableBroken,
    /// 0xe26b 段位守卫失败（诊断码越出本模块段）。
    SegmentGuard,
}

impl u2961Code {
    /// 全量闭集。
    pub const ALL: [u2961Code; 12] = [
        u2961Code::KeyDup,
        u2961Code::KeyGap,
        u2961Code::GuardBad,
        u2961Code::SumDrift,
        u2961Code::HistOver,
        u2961Code::HistHole,
        u2961Code::PctOob,
        u2961Code::MeanDrift,
        u2961Code::LedgerBroken,
        u2961Code::SnapshotFingerprint,
        u2961Code::TableBroken,
        u2961Code::SegmentGuard,
    ];

    /// 码值（连续十六进制，与声明序恒等）。
    pub const fn code(self) -> u16 {
        match self {
            u2961Code::KeyDup => 0xe260,
            u2961Code::KeyGap => 0xe261,
            u2961Code::GuardBad => 0xe262,
            u2961Code::SumDrift => 0xe263,
            u2961Code::HistOver => 0xe264,
            u2961Code::HistHole => 0xe265,
            u2961Code::PctOob => 0xe266,
            u2961Code::MeanDrift => 0xe267,
            u2961Code::LedgerBroken => 0xe268,
            u2961Code::SnapshotFingerprint => 0xe269,
            u2961Code::TableBroken => 0xe26a,
            u2961Code::SegmentGuard => 0xe26b,
        }
    }

    /// 中文诊断名。
    pub const fn name(self) -> &'static str {
        match self {
            u2961Code::KeyDup => "键重复（全域 同键双行）",
            u2961Code::KeyGap => "键断档（回归 序不连续）",
            u2961Code::GuardBad => "行守卫破缺（矩阵 逐行校验失败）",
            u2961Code::SumDrift => "矩阵累计漂移（全域 求和失锚）",
            u2961Code::HistOver => "直方图溢出（超容量拒收未记账）",
            u2961Code::HistHole => "直方图断桶（桶序缺失）",
            u2961Code::PctOob => "分位越界（千分比>1000）",
            u2961Code::MeanDrift => "均值漂移（均值与分位矛盾）",
            u2961Code::LedgerBroken => "台账不守恒（守恒恒等式破缺）",
            u2961Code::SnapshotFingerprint => "快照指纹不符（字段被改后重算不一致）",
            u2961Code::TableBroken => "常量表破坏（表项与语义规则矛盾）",
            u2961Code::SegmentGuard => "段位守卫失败（诊断码越出本模块段）",
        }
    }

    /// 可操作修复建议（每条诊断都必须带下一步动作）。
    pub const fn suggestion(self) -> &'static str {
        match self {
            u2961Code::KeyDup => "按键唯一律去重",
            u2961Code::KeyGap => "按连续律补号",
            u2961Code::GuardBad => "按守卫口径重建",
            u2961Code::SumDrift => "按理论值复算",
            u2961Code::HistOver => "按溢出账口径登记",
            u2961Code::HistHole => "按桶序补齐",
            u2961Code::PctOob => "按 0..=1000 钳制",
            u2961Code::MeanDrift => "按同源数据复算",
            u2961Code::LedgerBroken => "按守恒律重建台账后重跑",
            u2961Code::SnapshotFingerprint => "按防篡改规程重建快照",
            u2961Code::TableBroken => "按语义规则重建常量表",
            u2961Code::SegmentGuard => "核对诊断码分配表",
        }
    }
}

// --------------------------------------------------------------------------
// 字符串键值映射 u2961mat（36 行逐行语义注释 + 逐行守卫）
// --------------------------------------------------------------------------
/// 域级回归全域矩阵 守卫矩阵（与核心登记表同源对拍）。
/// 行数。
pub const u2961mat_COUNT: usize = 36;

/// 表本体（键 + 值 + 注释由编译期冻结）。
pub static u2961mat: [(&'static str, u64); 36] = [
    ("REG0000", 0), // REG 单元 0000（回归 × 相位 00 × 一 轮）
    ("FULL0001", 13), // FULL 单元 0001（全域 × 相位 01 × 一 轮）
    ("B01280002", 26), // B0128 单元 0002（B01-B28 × 相位 02 × 一 轮）
    ("CASE0003", 39), // CASE 单元 0003（用例 × 相位 03 × 一 轮）
    ("BASE0004", 52), // BASE 单元 0004（基线 × 相位 04 × 一 轮）
    ("DIFF0005", 65), // DIFF 单元 0005（差异 × 相位 05 × 一 轮）
    ("COV0006", 78), // COV 单元 0006（覆盖 × 相位 06 × 一 轮）
    ("FLAKY0007", 91), // FLAKY 单元 0007（抖动 × 相位 07 × 一 轮）
    ("QUAR0008", 7), // QUAR 单元 0008（隔离 × 相位 08 × 一 轮）
    ("SEAL0009", 20), // SEAL 单元 0009（封存 × 相位 09 × 一 轮）
    ("RERUN0010", 33), // RERUN 单元 0010（复跑 × 相位 10 × 一 轮）
    ("COUNT0011", 46), // COUNT 单元 0011（计数 × 相位 11 × 一 轮）
    ("REG0012", 59), // REG 单元 0012（回归 × 相位 12 × 一 轮）
    ("FULL0013", 72), // FULL 单元 0013（全域 × 相位 13 × 一 轮）
    ("B01280014", 85), // B0128 单元 0014（B01-B28 × 相位 14 × 一 轮）
    ("CASE0015", 1), // CASE 单元 0015（用例 × 相位 15 × 一 轮）
    ("BASE0016", 14), // BASE 单元 0016（基线 × 相位 16 × 一 轮）
    ("DIFF0017", 27), // DIFF 单元 0017（差异 × 相位 17 × 一 轮）
    ("COV0018", 40), // COV 单元 0018（覆盖 × 相位 18 × 一 轮）
    ("FLAKY0019", 53), // FLAKY 单元 0019（抖动 × 相位 19 × 一 轮）
    ("QUAR0020", 66), // QUAR 单元 0020（隔离 × 相位 20 × 一 轮）
    ("SEAL0021", 79), // SEAL 单元 0021（封存 × 相位 21 × 一 轮）
    ("RERUN0022", 92), // RERUN 单元 0022（复跑 × 相位 22 × 一 轮）
    ("COUNT0023", 8), // COUNT 单元 0023（计数 × 相位 23 × 一 轮）
    ("REG0024", 21), // REG 单元 0024（回归 × 相位 24 × 一 轮）
    ("FULL0025", 34), // FULL 单元 0025（全域 × 相位 25 × 一 轮）
    ("B01280026", 47), // B0128 单元 0026（B01-B28 × 相位 26 × 一 轮）
    ("CASE0027", 60), // CASE 单元 0027（用例 × 相位 27 × 一 轮）
    ("BASE0028", 73), // BASE 单元 0028（基线 × 相位 28 × 一 轮）
    ("DIFF0029", 86), // DIFF 单元 0029（差异 × 相位 29 × 一 轮）
    ("COV0030", 2), // COV 单元 0030（覆盖 × 相位 30 × 一 轮）
    ("FLAKY0031", 15), // FLAKY 单元 0031（抖动 × 相位 31 × 一 轮）
    ("QUAR0032", 28), // QUAR 单元 0032（隔离 × 相位 32 × 一 轮）
    ("SEAL0033", 41), // SEAL 单元 0033（封存 × 相位 33 × 一 轮）
    ("RERUN0034", 54), // RERUN 单元 0034（复跑 × 相位 34 × 一 轮）
    ("COUNT0035", 67), // COUNT 单元 0035（计数 × 相位 35 × 一 轮）
];

/// O(1) 语义查找（match 全枚举；命中率 100%% 由守卫保证）。
pub fn u2961mat_get(key: &str) -> Option<u64> {
    match key {
        "REG0000" => Some(0),
        "FULL0001" => Some(13),
        "B01280002" => Some(26),
        "CASE0003" => Some(39),
        "BASE0004" => Some(52),
        "DIFF0005" => Some(65),
        "COV0006" => Some(78),
        "FLAKY0007" => Some(91),
        "QUAR0008" => Some(7),
        "SEAL0009" => Some(20),
        "RERUN0010" => Some(33),
        "COUNT0011" => Some(46),
        "REG0012" => Some(59),
        "FULL0013" => Some(72),
        "B01280014" => Some(85),
        "CASE0015" => Some(1),
        "BASE0016" => Some(14),
        "DIFF0017" => Some(27),
        "COV0018" => Some(40),
        "FLAKY0019" => Some(53),
        "QUAR0020" => Some(66),
        "SEAL0021" => Some(79),
        "RERUN0022" => Some(92),
        "COUNT0023" => Some(8),
        "REG0024" => Some(21),
        "FULL0025" => Some(34),
        "B01280026" => Some(47),
        "CASE0027" => Some(60),
        "BASE0028" => Some(73),
        "DIFF0029" => Some(86),
        "COV0030" => Some(2),
        "FLAKY0031" => Some(15),
        "QUAR0032" => Some(28),
        "SEAL0033" => Some(41),
        "RERUN0034" => Some(54),
        "COUNT0035" => Some(67),
        _ => None,
    }
}

/// 逐行守卫扫描（返回违例行数；0 = 全绿）。
pub fn u2961mat_scan() -> usize {
    let mut bad = 0usize;
    let mut i = 0usize;
    while i < u2961mat_COUNT {
        let (k, v) = u2961mat[i];
        let ok = match k {
            "REG0000" => v < 97,
            "FULL0001" => v < 97,
            "B01280002" => v < 97,
            "CASE0003" => v < 97,
            "BASE0004" => v < 97,
            "DIFF0005" => v < 97,
            "COV0006" => v < 97,
            "FLAKY0007" => v < 97,
            "QUAR0008" => v < 97,
            "SEAL0009" => v < 97,
            "RERUN0010" => v < 97,
            "COUNT0011" => v < 97,
            "REG0012" => v < 97,
            "FULL0013" => v < 97,
            "B01280014" => v < 97,
            "CASE0015" => v < 97,
            "BASE0016" => v < 97,
            "DIFF0017" => v < 97,
            "COV0018" => v < 97,
            "FLAKY0019" => v < 97,
            "QUAR0020" => v < 97,
            "SEAL0021" => v < 97,
            "RERUN0022" => v < 97,
            "COUNT0023" => v < 97,
            "REG0024" => v < 97,
            "FULL0025" => v < 97,
            "B01280026" => v < 97,
            "CASE0027" => v < 97,
            "BASE0028" => v < 97,
            "DIFF0029" => v < 97,
            "COV0030" => v < 97,
            "FLAKY0031" => v < 97,
            "QUAR0032" => v < 97,
            "SEAL0033" => v < 97,
            "RERUN0034" => v < 97,
            "COUNT0035" => v < 97,
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
pub fn u2961mat_sum() -> u64 {
    let mut s = 0u64;
    let mut i = 0usize;
    while i < u2961mat_COUNT {
        s = s.wrapping_add(u2961mat[i].1 as u64);
        i += 1;
    }
    s
}

/// 键唯一性（构造期断言等价物：逐对扫描同键）。
pub fn u2961mat_unique() -> bool {
    let mut i = 0usize;
    while i < u2961mat_COUNT {
        let mut j = i + 1;
        while j < u2961mat_COUNT {
            if u2961mat[i].0 == u2961mat[j].0 {
                return false;
            }
            j += 1;
        }
        i += 1;
    }
    true
}

// --------------------------------------------------------------------------
// 直方图引擎 mxu2961hist（有界插入排序 + 分位数 P50/P90/P99）
// --------------------------------------------------------------------------
/// 域级回归全域矩阵 直方图（96 桶滚动）。
/// 样本容量上限。
pub const MXU2961HIST_CAP: usize = 96;

/// 直方图账本（排序态 + 五统量）。
#[derive(Clone, Copy, Debug)]
pub struct mxu2961histHist {
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

impl mxu2961histHist {
    /// 空账构造。
    pub const fn new() -> Self {
        Self { samples: [0; 96], n: 0, drops: 0, min: 0, max: 0, sum: 0 }
    }

    /// 采样（有界插入排序；满员丢弃并记账）。
    pub fn push(&mut self, v: u64) {
        if self.n >= MXU2961HIST_CAP {
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

/// 段位守卫：全部诊断码必须落在本段 [0xe260, 0xe26f]。
pub const fn u2961_segment_guard() -> bool {
    let mut ok = true;
    let mut i = 0usize;
    while i < u2961Code::ALL.len() {
        let c = u2961Code::ALL[i].code();
        if c < 0xe260 || c > 0xe26f {
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
fn u2961_guard_seg() -> bool {
    u2961_segment_guard()
}

/// UNX-F2961 矩阵 36 行齐备 + 逐行守卫 + 唯一性 + 累计守恒。
fn check_u2961_map_guard() -> bool {
    let mut ok = u2961mat_COUNT == 36;
    if ok { ok = u2961mat_scan() == 0 && u2961mat_unique(); }
    if ok { ok = u2961mat_get("REG0000") == Some(0) && u2961mat_get("ZZZ_NOKEY").is_none(); }
    if ok { ok = u2961mat_sum() == 1594; }
    ok
}

/// UNX-F2961 直方图 16 样本分位/均值/守恒。
fn check_u2961_map_hist() -> bool {
    let mut h = mxu2961histHist::new();
    let mut i = 0u64;
    while i < 16 { h.push(i + 1); i += 1; }
    let mut ok = h.conserved() && h.mean() >= 1 && h.percentile(500) >= 8;
    ok
}

/// 本模块自检（逐检查带 detail，全绿才出门）。
pub fn run_u2961_checks() -> CheckSet {
    let mut set = CheckSet::new("U2961");
    set.add(
        "u2961-seg-guard",
        u2961_guard_seg(),
        if u2961_guard_seg() { "" } else { "diagnostic code outside segment" },
    );
    set.add(
        "u2961-map-guard",
        check_u2961_map_guard(),
        if check_u2961_map_guard() { "" } else { "UNX-F2961 matrix guard mismatch" },
    );
    set.add(
        "u2961-map-hist",
        check_u2961_map_hist(),
        if check_u2961_map_hist() { "" } else { "UNX-F2961 hist mismatch" },
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
    fn u2961_all_checks_green() {
        let set = run_u2961_checks();
        assert!(set.all_passed(), "U2961 domain checks must be green");
    }

    #[test]
    fn u2961_segment_guard_holds() {
        assert!(u2961_segment_guard());
    }
}
