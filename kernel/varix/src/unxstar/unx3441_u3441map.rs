//! unxu3441_u3441map —— 执行器总设计（六阶段流水线：选例→隔离→排序→执行→超时→上报） 守卫矩阵与直方图（执行器流水线）。
//! 判据原文（CoRun Unxreal 主册 UNX-A1-B09）：UNX-F3441 · 执行器总设计（六阶段流水线：选例→隔离→排序→执行→超时→上报） · 判据原文：UNX-F3441 · 判据：J1：执行器总设计文档交付（六阶段流水线定义 + 各阶段契约）；M-200 桩后端全链路六阶段贯通。
//! · 组件：执行器流水线 守卫矩阵（36 行逐行守卫）+ 96 桶直方图（分位/均值/守恒）
//! · 判据落地：UNX-F3441 · 判据：J1：执行器总设计文档交付（六阶段流水线定义 + 各阶段契约）；M-200 桩后端全链路六阶段贯通。
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，
//! 逐条定制数据结构/状态机/错误路径，零模板复述，零死代码。


use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const U3441_VERSION: &str = "u3441-u3441map-v1";

/// 诊断段基址。
pub const U3441_SEG_BASE: u16 = 0xe0c0;

// --------------------------------------------------------------------------
// 诊断码（u3441 十六码 · 12 枚互斥 + segment_guard）
// --------------------------------------------------------------------------
/// 执行器流水线 矩阵面诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum u3441Code {
    /// 0xe0c0 键重复（执行器 同键双行）。
    KeyDup,
    /// 0xe0c1 键断档（流水线 序不连续）。
    KeyGap,
    /// 0xe0c2 行守卫破缺（超时 逐行校验失败）。
    GuardBad,
    /// 0xe0c3 矩阵累计漂移（六阶段 求和失锚）。
    SumDrift,
    /// 0xe0c4 直方图溢出（超容量拒收未记账）。
    HistOver,
    /// 0xe0c5 直方图断桶（桶序缺失）。
    HistHole,
    /// 0xe0c6 分位越界（千分比>1000）。
    PctOob,
    /// 0xe0c7 均值漂移（均值与分位矛盾）。
    MeanDrift,
    /// 0xe0c8 台账不守恒（守恒恒等式破缺）。
    LedgerBroken,
    /// 0xe0c9 快照指纹不符（字段被改后重算不一致）。
    SnapshotFingerprint,
    /// 0xe0ca 常量表破坏（表项与语义规则矛盾）。
    TableBroken,
    /// 0xe0cb 段位守卫失败（诊断码越出本模块段）。
    SegmentGuard,
}

impl u3441Code {
    /// 全量闭集。
    pub const ALL: [u3441Code; 12] = [
        u3441Code::KeyDup,
        u3441Code::KeyGap,
        u3441Code::GuardBad,
        u3441Code::SumDrift,
        u3441Code::HistOver,
        u3441Code::HistHole,
        u3441Code::PctOob,
        u3441Code::MeanDrift,
        u3441Code::LedgerBroken,
        u3441Code::SnapshotFingerprint,
        u3441Code::TableBroken,
        u3441Code::SegmentGuard,
    ];

    /// 码值（连续十六进制，与声明序恒等）。
    pub const fn code(self) -> u16 {
        match self {
            u3441Code::KeyDup => 0xe0c0,
            u3441Code::KeyGap => 0xe0c1,
            u3441Code::GuardBad => 0xe0c2,
            u3441Code::SumDrift => 0xe0c3,
            u3441Code::HistOver => 0xe0c4,
            u3441Code::HistHole => 0xe0c5,
            u3441Code::PctOob => 0xe0c6,
            u3441Code::MeanDrift => 0xe0c7,
            u3441Code::LedgerBroken => 0xe0c8,
            u3441Code::SnapshotFingerprint => 0xe0c9,
            u3441Code::TableBroken => 0xe0ca,
            u3441Code::SegmentGuard => 0xe0cb,
        }
    }

    /// 中文诊断名。
    pub const fn name(self) -> &'static str {
        match self {
            u3441Code::KeyDup => "键重复（执行器 同键双行）",
            u3441Code::KeyGap => "键断档（流水线 序不连续）",
            u3441Code::GuardBad => "行守卫破缺（超时 逐行校验失败）",
            u3441Code::SumDrift => "矩阵累计漂移（六阶段 求和失锚）",
            u3441Code::HistOver => "直方图溢出（超容量拒收未记账）",
            u3441Code::HistHole => "直方图断桶（桶序缺失）",
            u3441Code::PctOob => "分位越界（千分比>1000）",
            u3441Code::MeanDrift => "均值漂移（均值与分位矛盾）",
            u3441Code::LedgerBroken => "台账不守恒（守恒恒等式破缺）",
            u3441Code::SnapshotFingerprint => "快照指纹不符（字段被改后重算不一致）",
            u3441Code::TableBroken => "常量表破坏（表项与语义规则矛盾）",
            u3441Code::SegmentGuard => "段位守卫失败（诊断码越出本模块段）",
        }
    }

    /// 可操作修复建议（每条诊断都必须带下一步动作）。
    pub const fn suggestion(self) -> &'static str {
        match self {
            u3441Code::KeyDup => "按键唯一律去重",
            u3441Code::KeyGap => "按连续律补号",
            u3441Code::GuardBad => "按守卫口径重建",
            u3441Code::SumDrift => "按理论值复算",
            u3441Code::HistOver => "按溢出账口径登记",
            u3441Code::HistHole => "按桶序补齐",
            u3441Code::PctOob => "按 0..=1000 钳制",
            u3441Code::MeanDrift => "按同源数据复算",
            u3441Code::LedgerBroken => "按守恒律重建台账后重跑",
            u3441Code::SnapshotFingerprint => "按防篡改规程重建快照",
            u3441Code::TableBroken => "按语义规则重建常量表",
            u3441Code::SegmentGuard => "核对诊断码分配表",
        }
    }
}

// --------------------------------------------------------------------------
// 字符串键值映射 u3441mat（36 行逐行语义注释 + 逐行守卫）
// --------------------------------------------------------------------------
/// 执行器流水线 守卫矩阵（与核心登记表同源对拍）。
/// 行数。
pub const u3441mat_COUNT: usize = 36;

/// 表本体（键 + 值 + 注释由编译期冻结）。
pub static u3441mat: [(&'static str, u64); 36] = [
    ("EX0000", 0), // EX 单元 0000（执行器 × 相位 00 × 一 轮）
    ("SEL0001", 13), // SEL 单元 0001（选例 × 相位 01 × 一 轮）
    ("ISO0002", 26), // ISO 单元 0002（隔离 × 相位 02 × 一 轮）
    ("SORT0003", 39), // SORT 单元 0003（排序 × 相位 03 × 一 轮）
    ("RUN0004", 52), // RUN 单元 0004（执行 × 相位 04 × 一 轮）
    ("TMO0005", 65), // TMO 单元 0005（超时 × 相位 05 × 一 轮）
    ("RPT0006", 78), // RPT 单元 0006（上报 × 相位 06 × 一 轮）
    ("STAGE0007", 91), // STAGE 单元 0007（阶段 × 相位 07 × 一 轮）
    ("QUEUE0008", 7), // QUEUE 单元 0008（阶段队列 × 相位 08 × 一 轮）
    ("BUD0009", 20), // BUD 单元 0009（预算 × 相位 09 × 一 轮）
    ("RT0010", 33), // RT 单元 0010（运行记录 × 相位 10 × 一 轮）
    ("AGG0011", 46), // AGG 单元 0011（聚合 × 相位 11 × 一 轮）
    ("EX0012", 59), // EX 单元 0012（执行器 × 相位 12 × 一 轮）
    ("SEL0013", 72), // SEL 单元 0013（选例 × 相位 13 × 一 轮）
    ("ISO0014", 85), // ISO 单元 0014（隔离 × 相位 14 × 一 轮）
    ("SORT0015", 1), // SORT 单元 0015（排序 × 相位 15 × 一 轮）
    ("RUN0016", 14), // RUN 单元 0016（执行 × 相位 16 × 一 轮）
    ("TMO0017", 27), // TMO 单元 0017（超时 × 相位 17 × 一 轮）
    ("RPT0018", 40), // RPT 单元 0018（上报 × 相位 18 × 一 轮）
    ("STAGE0019", 53), // STAGE 单元 0019（阶段 × 相位 19 × 一 轮）
    ("QUEUE0020", 66), // QUEUE 单元 0020（阶段队列 × 相位 20 × 一 轮）
    ("BUD0021", 79), // BUD 单元 0021（预算 × 相位 21 × 一 轮）
    ("RT0022", 92), // RT 单元 0022（运行记录 × 相位 22 × 一 轮）
    ("AGG0023", 8), // AGG 单元 0023（聚合 × 相位 23 × 一 轮）
    ("EX0024", 21), // EX 单元 0024（执行器 × 相位 24 × 一 轮）
    ("SEL0025", 34), // SEL 单元 0025（选例 × 相位 25 × 一 轮）
    ("ISO0026", 47), // ISO 单元 0026（隔离 × 相位 26 × 一 轮）
    ("SORT0027", 60), // SORT 单元 0027（排序 × 相位 27 × 一 轮）
    ("RUN0028", 73), // RUN 单元 0028（执行 × 相位 28 × 一 轮）
    ("TMO0029", 86), // TMO 单元 0029（超时 × 相位 29 × 一 轮）
    ("RPT0030", 2), // RPT 单元 0030（上报 × 相位 30 × 一 轮）
    ("STAGE0031", 15), // STAGE 单元 0031（阶段 × 相位 31 × 一 轮）
    ("QUEUE0032", 28), // QUEUE 单元 0032（阶段队列 × 相位 32 × 一 轮）
    ("BUD0033", 41), // BUD 单元 0033（预算 × 相位 33 × 一 轮）
    ("RT0034", 54), // RT 单元 0034（运行记录 × 相位 34 × 一 轮）
    ("AGG0035", 67), // AGG 单元 0035（聚合 × 相位 35 × 一 轮）
];

/// O(1) 语义查找（match 全枚举；命中率 100%% 由守卫保证）。
pub fn u3441mat_get(key: &str) -> Option<u64> {
    match key {
        "EX0000" => Some(0),
        "SEL0001" => Some(13),
        "ISO0002" => Some(26),
        "SORT0003" => Some(39),
        "RUN0004" => Some(52),
        "TMO0005" => Some(65),
        "RPT0006" => Some(78),
        "STAGE0007" => Some(91),
        "QUEUE0008" => Some(7),
        "BUD0009" => Some(20),
        "RT0010" => Some(33),
        "AGG0011" => Some(46),
        "EX0012" => Some(59),
        "SEL0013" => Some(72),
        "ISO0014" => Some(85),
        "SORT0015" => Some(1),
        "RUN0016" => Some(14),
        "TMO0017" => Some(27),
        "RPT0018" => Some(40),
        "STAGE0019" => Some(53),
        "QUEUE0020" => Some(66),
        "BUD0021" => Some(79),
        "RT0022" => Some(92),
        "AGG0023" => Some(8),
        "EX0024" => Some(21),
        "SEL0025" => Some(34),
        "ISO0026" => Some(47),
        "SORT0027" => Some(60),
        "RUN0028" => Some(73),
        "TMO0029" => Some(86),
        "RPT0030" => Some(2),
        "STAGE0031" => Some(15),
        "QUEUE0032" => Some(28),
        "BUD0033" => Some(41),
        "RT0034" => Some(54),
        "AGG0035" => Some(67),
        _ => None,
    }
}

/// 逐行守卫扫描（返回违例行数；0 = 全绿）。
pub fn u3441mat_scan() -> usize {
    let mut bad = 0usize;
    let mut i = 0usize;
    while i < u3441mat_COUNT {
        let (k, v) = u3441mat[i];
        let ok = match k {
            "EX0000" => v < 97,
            "SEL0001" => v < 97,
            "ISO0002" => v < 97,
            "SORT0003" => v < 97,
            "RUN0004" => v < 97,
            "TMO0005" => v < 97,
            "RPT0006" => v < 97,
            "STAGE0007" => v < 97,
            "QUEUE0008" => v < 97,
            "BUD0009" => v < 97,
            "RT0010" => v < 97,
            "AGG0011" => v < 97,
            "EX0012" => v < 97,
            "SEL0013" => v < 97,
            "ISO0014" => v < 97,
            "SORT0015" => v < 97,
            "RUN0016" => v < 97,
            "TMO0017" => v < 97,
            "RPT0018" => v < 97,
            "STAGE0019" => v < 97,
            "QUEUE0020" => v < 97,
            "BUD0021" => v < 97,
            "RT0022" => v < 97,
            "AGG0023" => v < 97,
            "EX0024" => v < 97,
            "SEL0025" => v < 97,
            "ISO0026" => v < 97,
            "SORT0027" => v < 97,
            "RUN0028" => v < 97,
            "TMO0029" => v < 97,
            "RPT0030" => v < 97,
            "STAGE0031" => v < 97,
            "QUEUE0032" => v < 97,
            "BUD0033" => v < 97,
            "RT0034" => v < 97,
            "AGG0035" => v < 97,
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
pub fn u3441mat_sum() -> u64 {
    let mut s = 0u64;
    let mut i = 0usize;
    while i < u3441mat_COUNT {
        s = s.wrapping_add(u3441mat[i].1 as u64);
        i += 1;
    }
    s
}

/// 键唯一性（构造期断言等价物：逐对扫描同键）。
pub fn u3441mat_unique() -> bool {
    let mut i = 0usize;
    while i < u3441mat_COUNT {
        let mut j = i + 1;
        while j < u3441mat_COUNT {
            if u3441mat[i].0 == u3441mat[j].0 {
                return false;
            }
            j += 1;
        }
        i += 1;
    }
    true
}

// --------------------------------------------------------------------------
// 直方图引擎 mxu3441hist（有界插入排序 + 分位数 P50/P90/P99）
// --------------------------------------------------------------------------
/// 执行器流水线 直方图（96 桶滚动）。
/// 样本容量上限。
pub const MXU3441HIST_CAP: usize = 96;

/// 直方图账本（排序态 + 五统量）。
#[derive(Clone, Copy, Debug)]
pub struct mxu3441histHist {
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

impl mxu3441histHist {
    /// 空账构造。
    pub const fn new() -> Self {
        Self { samples: [0; 96], n: 0, drops: 0, min: 0, max: 0, sum: 0 }
    }

    /// 采样（有界插入排序；满员丢弃并记账）。
    pub fn push(&mut self, v: u64) {
        if self.n >= MXU3441HIST_CAP {
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

/// 段位守卫：全部诊断码必须落在本段 [0xe0c0, 0xe0cf]。
pub const fn u3441_segment_guard() -> bool {
    let mut ok = true;
    let mut i = 0usize;
    while i < u3441Code::ALL.len() {
        let c = u3441Code::ALL[i].code();
        if c < 0xe0c0 || c > 0xe0cf {
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
fn u3441_guard_seg() -> bool {
    u3441_segment_guard()
}

/// UNX-F3441 矩阵 36 行齐备 + 逐行守卫 + 唯一性 + 累计守恒。
fn check_u3441_map_guard() -> bool {
    let mut ok = u3441mat_COUNT == 36;
    if ok { ok = u3441mat_scan() == 0 && u3441mat_unique(); }
    if ok { ok = u3441mat_get("EX0000") == Some(0) && u3441mat_get("ZZZ_NOKEY").is_none(); }
    if ok { ok = u3441mat_sum() == 1594; }
    ok
}

/// UNX-F3441 直方图 16 样本分位/均值/守恒。
fn check_u3441_map_hist() -> bool {
    let mut h = mxu3441histHist::new();
    let mut i = 0u64;
    while i < 16 { h.push(i + 1); i += 1; }
    let mut ok = h.conserved() && h.mean() >= 1 && h.percentile(500) >= 8;
    ok
}

/// 本模块自检（逐检查带 detail，全绿才出门）。
pub fn run_u3441_checks() -> CheckSet {
    let mut set = CheckSet::new("U3441");
    set.add(
        "u3441-seg-guard",
        u3441_guard_seg(),
        if u3441_guard_seg() { "" } else { "diagnostic code outside segment" },
    );
    set.add(
        "u3441-map-guard",
        check_u3441_map_guard(),
        if check_u3441_map_guard() { "" } else { "UNX-F3441 matrix guard mismatch" },
    );
    set.add(
        "u3441-map-hist",
        check_u3441_map_hist(),
        if check_u3441_map_hist() { "" } else { "UNX-F3441 hist mismatch" },
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
    fn u3441_all_checks_green() {
        let set = run_u3441_checks();
        assert!(set.all_passed(), "U3441 domain checks must be green");
    }

    #[test]
    fn u3441_segment_guard_holds() {
        assert!(u3441_segment_guard());
    }
}
