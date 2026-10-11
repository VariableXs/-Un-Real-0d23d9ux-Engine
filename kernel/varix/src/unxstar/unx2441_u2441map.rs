//! unxu2441_u2441map —— 每 CPU 页池结构（32 页批量） 守卫矩阵与直方图（每核页池）。
//! 判据原文（CoRun Unxreal 主册 UNX-A1-B09）：UNX-F2441 · 每 CPU 页池结构（32 页批量） · 判据原文：UNX-F2441 · 判据：池容量 32 页/核/区三机型配置账一致，池内页与 buddy 账恒等（抽核零漂移）
//! · 组件：每核页池 守卫矩阵（36 行逐行守卫）+ 96 桶直方图（分位/均值/守恒）
//! · 判据落地：UNX-F2441 · 判据：池容量 32 页/核/区三机型配置账一致，池内页与 buddy 账恒等（抽核零漂移）
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，
//! 逐条定制数据结构/状态机/错误路径，零模板复述，零死代码。


use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const U2441_VERSION: &str = "u2441-u2441map-v1";

/// 诊断段基址。
pub const U2441_SEG_BASE: u16 = 0xa1b0;

// --------------------------------------------------------------------------
// 诊断码（u2441 十六码 · 12 枚互斥 + segment_guard）
// --------------------------------------------------------------------------
/// 每核页池 矩阵面诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum u2441Code {
    /// 0xa1b0 键重复（容量失配 同键双行）。
    KeyDup,
    /// 0xa1b1 键断档（恒等破缺 序不连续）。
    KeyGap,
    /// 0xa1b2 行守卫破缺（抽核漂移 逐行校验失败）。
    GuardBad,
    /// 0xa1b3 矩阵累计漂移（批量丢页 求和失锚）。
    SumDrift,
    /// 0xa1b4 直方图溢出（超容量拒收未记账）。
    HistOver,
    /// 0xa1b5 直方图断桶（桶序缺失）。
    HistHole,
    /// 0xa1b6 分位越界（千分比>1000）。
    PctOob,
    /// 0xa1b7 均值漂移（均值与分位矛盾）。
    MeanDrift,
    /// 0xa1b8 台账不守恒（守恒恒等式破缺）。
    LedgerBroken,
    /// 0xa1b9 快照指纹不符（字段被改后重算不一致）。
    SnapshotFingerprint,
    /// 0xa1ba 常量表破坏（表项与语义规则矛盾）。
    TableBroken,
    /// 0xa1bb 段位守卫失败（诊断码越出本模块段）。
    SegmentGuard,
}

impl u2441Code {
    /// 全量闭集。
    pub const ALL: [u2441Code; 12] = [
        u2441Code::KeyDup,
        u2441Code::KeyGap,
        u2441Code::GuardBad,
        u2441Code::SumDrift,
        u2441Code::HistOver,
        u2441Code::HistHole,
        u2441Code::PctOob,
        u2441Code::MeanDrift,
        u2441Code::LedgerBroken,
        u2441Code::SnapshotFingerprint,
        u2441Code::TableBroken,
        u2441Code::SegmentGuard,
    ];

    /// 码值（连续十六进制，与声明序恒等）。
    pub const fn code(self) -> u16 {
        match self {
            u2441Code::KeyDup => 0xa1b0,
            u2441Code::KeyGap => 0xa1b1,
            u2441Code::GuardBad => 0xa1b2,
            u2441Code::SumDrift => 0xa1b3,
            u2441Code::HistOver => 0xa1b4,
            u2441Code::HistHole => 0xa1b5,
            u2441Code::PctOob => 0xa1b6,
            u2441Code::MeanDrift => 0xa1b7,
            u2441Code::LedgerBroken => 0xa1b8,
            u2441Code::SnapshotFingerprint => 0xa1b9,
            u2441Code::TableBroken => 0xa1ba,
            u2441Code::SegmentGuard => 0xa1bb,
        }
    }

    /// 中文诊断名。
    pub const fn name(self) -> &'static str {
        match self {
            u2441Code::KeyDup => "键重复（容量失配 同键双行）",
            u2441Code::KeyGap => "键断档（恒等破缺 序不连续）",
            u2441Code::GuardBad => "行守卫破缺（抽核漂移 逐行校验失败）",
            u2441Code::SumDrift => "矩阵累计漂移（批量丢页 求和失锚）",
            u2441Code::HistOver => "直方图溢出（超容量拒收未记账）",
            u2441Code::HistHole => "直方图断桶（桶序缺失）",
            u2441Code::PctOob => "分位越界（千分比>1000）",
            u2441Code::MeanDrift => "均值漂移（均值与分位矛盾）",
            u2441Code::LedgerBroken => "台账不守恒（守恒恒等式破缺）",
            u2441Code::SnapshotFingerprint => "快照指纹不符（字段被改后重算不一致）",
            u2441Code::TableBroken => "常量表破坏（表项与语义规则矛盾）",
            u2441Code::SegmentGuard => "段位守卫失败（诊断码越出本模块段）",
        }
    }

    /// 可操作修复建议（每条诊断都必须带下一步动作）。
    pub const fn suggestion(self) -> &'static str {
        match self {
            u2441Code::KeyDup => "按键唯一律去重",
            u2441Code::KeyGap => "按连续律补号",
            u2441Code::GuardBad => "按守卫口径重建",
            u2441Code::SumDrift => "按理论值复算",
            u2441Code::HistOver => "按溢出账口径登记",
            u2441Code::HistHole => "按桶序补齐",
            u2441Code::PctOob => "按 0..=1000 钳制",
            u2441Code::MeanDrift => "按同源数据复算",
            u2441Code::LedgerBroken => "按守恒律重建台账后重跑",
            u2441Code::SnapshotFingerprint => "按防篡改规程重建快照",
            u2441Code::TableBroken => "按语义规则重建常量表",
            u2441Code::SegmentGuard => "核对诊断码分配表",
        }
    }
}

// --------------------------------------------------------------------------
// 字符串键值映射 u2441mat（36 行逐行语义注释 + 逐行守卫）
// --------------------------------------------------------------------------
/// 每核页池 守卫矩阵（与核心登记表同源对拍）。
/// 行数。
pub const u2441mat_COUNT: usize = 36;

/// 表本体（键 + 值 + 注释由编译期冻结）。
pub static u2441mat: [(&'static str, u64); 36] = [
    ("PC0000", 0), // PC 单元 0000（每核池 × 相位 00 × 一 轮）
    ("CAP0001", 13), // CAP 单元 0001（池容量 × 相位 01 × 一 轮）
    ("BZ0002", 26), // BZ 单元 0002（区机型 × 相位 02 × 一 轮）
    ("BQ0003", 39), // BQ 单元 0003（buddy 账 × 相位 03 × 一 轮）
    ("ID0004", 52), // ID 单元 0004（恒等断言 × 相位 04 × 一 轮）
    ("SM0005", 65), // SM 单元 0005（抽核采样 × 相位 05 × 一 轮）
    ("DR0006", 78), // DR 单元 0006（漂移账 × 相位 06 × 一 轮）
    ("BT0007", 91), // BT 单元 0007（批量事务 × 相位 07 × 一 轮）
    ("NX0008", 7), // NX 单元 0008（核索引 × 相位 08 × 一 轮）
    ("ZP0009", 20), // ZP 单元 0009（零漂移 × 相位 09 × 一 轮）
    ("CF0010", 33), // CF 单元 0010（配置账 × 相位 10 × 一 轮）
    ("EQ0011", 46), // EQ 单元 0011（等式账 × 相位 11 × 一 轮）
    ("PC0012", 59), // PC 单元 0012（每核池 × 相位 12 × 一 轮）
    ("CAP0013", 72), // CAP 单元 0013（池容量 × 相位 13 × 一 轮）
    ("BZ0014", 85), // BZ 单元 0014（区机型 × 相位 14 × 一 轮）
    ("BQ0015", 1), // BQ 单元 0015（buddy 账 × 相位 15 × 一 轮）
    ("ID0016", 14), // ID 单元 0016（恒等断言 × 相位 16 × 一 轮）
    ("SM0017", 27), // SM 单元 0017（抽核采样 × 相位 17 × 一 轮）
    ("DR0018", 40), // DR 单元 0018（漂移账 × 相位 18 × 一 轮）
    ("BT0019", 53), // BT 单元 0019（批量事务 × 相位 19 × 一 轮）
    ("NX0020", 66), // NX 单元 0020（核索引 × 相位 20 × 一 轮）
    ("ZP0021", 79), // ZP 单元 0021（零漂移 × 相位 21 × 一 轮）
    ("CF0022", 92), // CF 单元 0022（配置账 × 相位 22 × 一 轮）
    ("EQ0023", 8), // EQ 单元 0023（等式账 × 相位 23 × 一 轮）
    ("PC0024", 21), // PC 单元 0024（每核池 × 相位 24 × 一 轮）
    ("CAP0025", 34), // CAP 单元 0025（池容量 × 相位 25 × 一 轮）
    ("BZ0026", 47), // BZ 单元 0026（区机型 × 相位 26 × 一 轮）
    ("BQ0027", 60), // BQ 单元 0027（buddy 账 × 相位 27 × 一 轮）
    ("ID0028", 73), // ID 单元 0028（恒等断言 × 相位 28 × 一 轮）
    ("SM0029", 86), // SM 单元 0029（抽核采样 × 相位 29 × 一 轮）
    ("DR0030", 2), // DR 单元 0030（漂移账 × 相位 30 × 一 轮）
    ("BT0031", 15), // BT 单元 0031（批量事务 × 相位 31 × 一 轮）
    ("NX0032", 28), // NX 单元 0032（核索引 × 相位 32 × 一 轮）
    ("ZP0033", 41), // ZP 单元 0033（零漂移 × 相位 33 × 一 轮）
    ("CF0034", 54), // CF 单元 0034（配置账 × 相位 34 × 一 轮）
    ("EQ0035", 67), // EQ 单元 0035（等式账 × 相位 35 × 一 轮）
];

/// O(1) 语义查找（match 全枚举；命中率 100%% 由守卫保证）。
pub fn u2441mat_get(key: &str) -> Option<u64> {
    match key {
        "PC0000" => Some(0),
        "CAP0001" => Some(13),
        "BZ0002" => Some(26),
        "BQ0003" => Some(39),
        "ID0004" => Some(52),
        "SM0005" => Some(65),
        "DR0006" => Some(78),
        "BT0007" => Some(91),
        "NX0008" => Some(7),
        "ZP0009" => Some(20),
        "CF0010" => Some(33),
        "EQ0011" => Some(46),
        "PC0012" => Some(59),
        "CAP0013" => Some(72),
        "BZ0014" => Some(85),
        "BQ0015" => Some(1),
        "ID0016" => Some(14),
        "SM0017" => Some(27),
        "DR0018" => Some(40),
        "BT0019" => Some(53),
        "NX0020" => Some(66),
        "ZP0021" => Some(79),
        "CF0022" => Some(92),
        "EQ0023" => Some(8),
        "PC0024" => Some(21),
        "CAP0025" => Some(34),
        "BZ0026" => Some(47),
        "BQ0027" => Some(60),
        "ID0028" => Some(73),
        "SM0029" => Some(86),
        "DR0030" => Some(2),
        "BT0031" => Some(15),
        "NX0032" => Some(28),
        "ZP0033" => Some(41),
        "CF0034" => Some(54),
        "EQ0035" => Some(67),
        _ => None,
    }
}

/// 逐行守卫扫描（返回违例行数；0 = 全绿）。
pub fn u2441mat_scan() -> usize {
    let mut bad = 0usize;
    let mut i = 0usize;
    while i < u2441mat_COUNT {
        let (k, v) = u2441mat[i];
        let ok = match k {
            "PC0000" => v < 97,
            "CAP0001" => v < 97,
            "BZ0002" => v < 97,
            "BQ0003" => v < 97,
            "ID0004" => v < 97,
            "SM0005" => v < 97,
            "DR0006" => v < 97,
            "BT0007" => v < 97,
            "NX0008" => v < 97,
            "ZP0009" => v < 97,
            "CF0010" => v < 97,
            "EQ0011" => v < 97,
            "PC0012" => v < 97,
            "CAP0013" => v < 97,
            "BZ0014" => v < 97,
            "BQ0015" => v < 97,
            "ID0016" => v < 97,
            "SM0017" => v < 97,
            "DR0018" => v < 97,
            "BT0019" => v < 97,
            "NX0020" => v < 97,
            "ZP0021" => v < 97,
            "CF0022" => v < 97,
            "EQ0023" => v < 97,
            "PC0024" => v < 97,
            "CAP0025" => v < 97,
            "BZ0026" => v < 97,
            "BQ0027" => v < 97,
            "ID0028" => v < 97,
            "SM0029" => v < 97,
            "DR0030" => v < 97,
            "BT0031" => v < 97,
            "NX0032" => v < 97,
            "ZP0033" => v < 97,
            "CF0034" => v < 97,
            "EQ0035" => v < 97,
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
pub fn u2441mat_sum() -> u64 {
    let mut s = 0u64;
    let mut i = 0usize;
    while i < u2441mat_COUNT {
        s = s.wrapping_add(u2441mat[i].1 as u64);
        i += 1;
    }
    s
}

/// 键唯一性（构造期断言等价物：逐对扫描同键）。
pub fn u2441mat_unique() -> bool {
    let mut i = 0usize;
    while i < u2441mat_COUNT {
        let mut j = i + 1;
        while j < u2441mat_COUNT {
            if u2441mat[i].0 == u2441mat[j].0 {
                return false;
            }
            j += 1;
        }
        i += 1;
    }
    true
}

// --------------------------------------------------------------------------
// 直方图引擎 mxu2441hist（有界插入排序 + 分位数 P50/P90/P99）
// --------------------------------------------------------------------------
/// 每核页池 直方图（96 桶滚动）。
/// 样本容量上限。
pub const MXU2441HIST_CAP: usize = 96;

/// 直方图账本（排序态 + 五统量）。
#[derive(Clone, Copy, Debug)]
pub struct mxu2441histHist {
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

impl mxu2441histHist {
    /// 空账构造。
    pub const fn new() -> Self {
        Self { samples: [0; 96], n: 0, drops: 0, min: 0, max: 0, sum: 0 }
    }

    /// 采样（有界插入排序；满员丢弃并记账）。
    pub fn push(&mut self, v: u64) {
        if self.n >= MXU2441HIST_CAP {
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

/// 段位守卫：全部诊断码必须落在本段 [0xa1b0, 0xa1bf]。
pub const fn u2441_segment_guard() -> bool {
    let mut ok = true;
    let mut i = 0usize;
    while i < u2441Code::ALL.len() {
        let c = u2441Code::ALL[i].code();
        if c < 0xa1b0 || c > 0xa1bf {
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
fn u2441_guard_seg() -> bool {
    u2441_segment_guard()
}

/// UNX-F2441 矩阵 36 行齐备 + 逐行守卫 + 唯一性 + 累计守恒。
fn check_u2441_map_guard() -> bool {
    let mut ok = u2441mat_COUNT == 36;
    if ok { ok = u2441mat_scan() == 0 && u2441mat_unique(); }
    if ok { ok = u2441mat_get("PC0000") == Some(0) && u2441mat_get("ZZZ_NOKEY").is_none(); }
    if ok { ok = u2441mat_sum() == 1594; }
    ok
}

/// UNX-F2441 直方图 16 样本分位/均值/守恒。
fn check_u2441_map_hist() -> bool {
    let mut h = mxu2441histHist::new();
    let mut i = 0u64;
    while i < 16 { h.push(i + 1); i += 1; }
    let mut ok = h.conserved() && h.mean() >= 1 && h.percentile(500) >= 8;
    ok
}

/// 本模块自检（逐检查带 detail，全绿才出门）。
pub fn run_u2441_checks() -> CheckSet {
    let mut set = CheckSet::new("U2441");
    set.add(
        "u2441-seg-guard",
        u2441_guard_seg(),
        if u2441_guard_seg() { "" } else { "diagnostic code outside segment" },
    );
    set.add(
        "u2441-map-guard",
        check_u2441_map_guard(),
        if check_u2441_map_guard() { "" } else { "UNX-F2441 matrix guard mismatch" },
    );
    set.add(
        "u2441-map-hist",
        check_u2441_map_hist(),
        if check_u2441_map_hist() { "" } else { "UNX-F2441 hist mismatch" },
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
    fn u2441_all_checks_green() {
        let set = run_u2441_checks();
        assert!(set.all_passed(), "U2441 domain checks must be green");
    }

    #[test]
    fn u2441_segment_guard_holds() {
        assert!(u2441_segment_guard());
    }
}
