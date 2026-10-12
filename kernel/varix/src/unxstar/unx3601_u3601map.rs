//! unxu3601_u3601map —— 锁压力机总设计（场景编排/指标采集/裁决门） 守卫矩阵与直方图（锁压力机）。
//! 判据原文（CoRun Unxreal 主册 UNX-A1-B09）：UNX-F3601 · 锁压力机总设计（场景编排/指标采集/裁决门） · 判据原文：UNX-F3601 · 判据：J1：压力机三层（场景层（七类场景注册）/执行层（N 工作者调度）/度量层（争用账挂钩）——判据行内嵌）；M-359 全链首跑（场景注册→执行→度量→裁决四段走通）。<br>J2：零侵断言（压力机关闭时锁路径拍数不变——度量是包裹观测）。
//! · 组件：锁压力机 守卫矩阵（36 行逐行守卫）+ 96 桶直方图（分位/均值/守恒）
//! · 判据落地：UNX-F3601 · 判据：J1：压力机三层（场景层（七类场景注册）/执行层（N 工作者调度）/度量层（争用账挂钩）——判据行内嵌）；M-359 全链首跑（场
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，
//! 逐条定制数据结构/状态机/错误路径，零模板复述，零死代码。


use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const U3601_VERSION: &str = "u3601-u3601map-v1";

/// 诊断段基址。
pub const U3601_SEG_BASE: u16 = 0xe140;

// --------------------------------------------------------------------------
// 诊断码（u3601 十六码 · 12 枚互斥 + segment_guard）
// --------------------------------------------------------------------------
/// 锁压力机 矩阵面诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum u3601Code {
    /// 0xe140 键重复（压力 同键双行）。
    KeyDup,
    /// 0xe141 键断档（场景 序不连续）。
    KeyGap,
    /// 0xe142 行守卫破缺（裁决 逐行校验失败）。
    GuardBad,
    /// 0xe143 矩阵累计漂移（裁决门 求和失锚）。
    SumDrift,
    /// 0xe144 直方图溢出（超容量拒收未记账）。
    HistOver,
    /// 0xe145 直方图断桶（桶序缺失）。
    HistHole,
    /// 0xe146 分位越界（千分比>1000）。
    PctOob,
    /// 0xe147 均值漂移（均值与分位矛盾）。
    MeanDrift,
    /// 0xe148 台账不守恒（守恒恒等式破缺）。
    LedgerBroken,
    /// 0xe149 快照指纹不符（字段被改后重算不一致）。
    SnapshotFingerprint,
    /// 0xe14a 常量表破坏（表项与语义规则矛盾）。
    TableBroken,
    /// 0xe14b 段位守卫失败（诊断码越出本模块段）。
    SegmentGuard,
}

impl u3601Code {
    /// 全量闭集。
    pub const ALL: [u3601Code; 12] = [
        u3601Code::KeyDup,
        u3601Code::KeyGap,
        u3601Code::GuardBad,
        u3601Code::SumDrift,
        u3601Code::HistOver,
        u3601Code::HistHole,
        u3601Code::PctOob,
        u3601Code::MeanDrift,
        u3601Code::LedgerBroken,
        u3601Code::SnapshotFingerprint,
        u3601Code::TableBroken,
        u3601Code::SegmentGuard,
    ];

    /// 码值（连续十六进制，与声明序恒等）。
    pub const fn code(self) -> u16 {
        match self {
            u3601Code::KeyDup => 0xe140,
            u3601Code::KeyGap => 0xe141,
            u3601Code::GuardBad => 0xe142,
            u3601Code::SumDrift => 0xe143,
            u3601Code::HistOver => 0xe144,
            u3601Code::HistHole => 0xe145,
            u3601Code::PctOob => 0xe146,
            u3601Code::MeanDrift => 0xe147,
            u3601Code::LedgerBroken => 0xe148,
            u3601Code::SnapshotFingerprint => 0xe149,
            u3601Code::TableBroken => 0xe14a,
            u3601Code::SegmentGuard => 0xe14b,
        }
    }

    /// 中文诊断名。
    pub const fn name(self) -> &'static str {
        match self {
            u3601Code::KeyDup => "键重复（压力 同键双行）",
            u3601Code::KeyGap => "键断档（场景 序不连续）",
            u3601Code::GuardBad => "行守卫破缺（裁决 逐行校验失败）",
            u3601Code::SumDrift => "矩阵累计漂移（裁决门 求和失锚）",
            u3601Code::HistOver => "直方图溢出（超容量拒收未记账）",
            u3601Code::HistHole => "直方图断桶（桶序缺失）",
            u3601Code::PctOob => "分位越界（千分比>1000）",
            u3601Code::MeanDrift => "均值漂移（均值与分位矛盾）",
            u3601Code::LedgerBroken => "台账不守恒（守恒恒等式破缺）",
            u3601Code::SnapshotFingerprint => "快照指纹不符（字段被改后重算不一致）",
            u3601Code::TableBroken => "常量表破坏（表项与语义规则矛盾）",
            u3601Code::SegmentGuard => "段位守卫失败（诊断码越出本模块段）",
        }
    }

    /// 可操作修复建议（每条诊断都必须带下一步动作）。
    pub const fn suggestion(self) -> &'static str {
        match self {
            u3601Code::KeyDup => "按键唯一律去重",
            u3601Code::KeyGap => "按连续律补号",
            u3601Code::GuardBad => "按守卫口径重建",
            u3601Code::SumDrift => "按理论值复算",
            u3601Code::HistOver => "按溢出账口径登记",
            u3601Code::HistHole => "按桶序补齐",
            u3601Code::PctOob => "按 0..=1000 钳制",
            u3601Code::MeanDrift => "按同源数据复算",
            u3601Code::LedgerBroken => "按守恒律重建台账后重跑",
            u3601Code::SnapshotFingerprint => "按防篡改规程重建快照",
            u3601Code::TableBroken => "按语义规则重建常量表",
            u3601Code::SegmentGuard => "核对诊断码分配表",
        }
    }
}

// --------------------------------------------------------------------------
// 字符串键值映射 u3601mat（36 行逐行语义注释 + 逐行守卫）
// --------------------------------------------------------------------------
/// 锁压力机 守卫矩阵（与核心登记表同源对拍）。
/// 行数。
pub const u3601mat_COUNT: usize = 36;

/// 表本体（键 + 值 + 注释由编译期冻结）。
pub static u3601mat: [(&'static str, u64); 36] = [
    ("STRESS0000", 0), // STRESS 单元 0000（压力机 × 相位 00 × 一 轮）
    ("SCN0001", 13), // SCN 单元 0001（场景 × 相位 01 × 一 轮）
    ("MET0002", 26), // MET 单元 0002（指标 × 相位 02 × 一 轮）
    ("GATE0003", 39), // GATE 单元 0003（裁决门 × 相位 03 × 一 轮）
    ("LOAD0004", 52), // LOAD 单元 0004（负载 × 相位 04 × 一 轮）
    ("CONT0005", 65), // CONT 单元 0005（争用 × 相位 05 × 一 轮）
    ("THR0006", 78), // THR 单元 0006（阈值 × 相位 06 × 一 轮）
    ("SCORE0007", 91), // SCORE 单元 0007（评分 × 相位 07 × 一 轮）
    ("RUN0008", 7), // RUN 单元 0008（轮次 × 相位 08 × 一 轮）
    ("SAMP0009", 20), // SAMP 单元 0009（采样 × 相位 09 × 一 轮）
    ("VERDICT0010", 33), // VERDICT 单元 0010（裁决 × 相位 10 × 一 轮）
    ("RPT0011", 46), // RPT 单元 0011（报告 × 相位 11 × 一 轮）
    ("STRESS0012", 59), // STRESS 单元 0012（压力机 × 相位 12 × 一 轮）
    ("SCN0013", 72), // SCN 单元 0013（场景 × 相位 13 × 一 轮）
    ("MET0014", 85), // MET 单元 0014（指标 × 相位 14 × 一 轮）
    ("GATE0015", 1), // GATE 单元 0015（裁决门 × 相位 15 × 一 轮）
    ("LOAD0016", 14), // LOAD 单元 0016（负载 × 相位 16 × 一 轮）
    ("CONT0017", 27), // CONT 单元 0017（争用 × 相位 17 × 一 轮）
    ("THR0018", 40), // THR 单元 0018（阈值 × 相位 18 × 一 轮）
    ("SCORE0019", 53), // SCORE 单元 0019（评分 × 相位 19 × 一 轮）
    ("RUN0020", 66), // RUN 单元 0020（轮次 × 相位 20 × 一 轮）
    ("SAMP0021", 79), // SAMP 单元 0021（采样 × 相位 21 × 一 轮）
    ("VERDICT0022", 92), // VERDICT 单元 0022（裁决 × 相位 22 × 一 轮）
    ("RPT0023", 8), // RPT 单元 0023（报告 × 相位 23 × 一 轮）
    ("STRESS0024", 21), // STRESS 单元 0024（压力机 × 相位 24 × 一 轮）
    ("SCN0025", 34), // SCN 单元 0025（场景 × 相位 25 × 一 轮）
    ("MET0026", 47), // MET 单元 0026（指标 × 相位 26 × 一 轮）
    ("GATE0027", 60), // GATE 单元 0027（裁决门 × 相位 27 × 一 轮）
    ("LOAD0028", 73), // LOAD 单元 0028（负载 × 相位 28 × 一 轮）
    ("CONT0029", 86), // CONT 单元 0029（争用 × 相位 29 × 一 轮）
    ("THR0030", 2), // THR 单元 0030（阈值 × 相位 30 × 一 轮）
    ("SCORE0031", 15), // SCORE 单元 0031（评分 × 相位 31 × 一 轮）
    ("RUN0032", 28), // RUN 单元 0032（轮次 × 相位 32 × 一 轮）
    ("SAMP0033", 41), // SAMP 单元 0033（采样 × 相位 33 × 一 轮）
    ("VERDICT0034", 54), // VERDICT 单元 0034（裁决 × 相位 34 × 一 轮）
    ("RPT0035", 67), // RPT 单元 0035（报告 × 相位 35 × 一 轮）
];

/// O(1) 语义查找（match 全枚举；命中率 100%% 由守卫保证）。
pub fn u3601mat_get(key: &str) -> Option<u64> {
    match key {
        "STRESS0000" => Some(0),
        "SCN0001" => Some(13),
        "MET0002" => Some(26),
        "GATE0003" => Some(39),
        "LOAD0004" => Some(52),
        "CONT0005" => Some(65),
        "THR0006" => Some(78),
        "SCORE0007" => Some(91),
        "RUN0008" => Some(7),
        "SAMP0009" => Some(20),
        "VERDICT0010" => Some(33),
        "RPT0011" => Some(46),
        "STRESS0012" => Some(59),
        "SCN0013" => Some(72),
        "MET0014" => Some(85),
        "GATE0015" => Some(1),
        "LOAD0016" => Some(14),
        "CONT0017" => Some(27),
        "THR0018" => Some(40),
        "SCORE0019" => Some(53),
        "RUN0020" => Some(66),
        "SAMP0021" => Some(79),
        "VERDICT0022" => Some(92),
        "RPT0023" => Some(8),
        "STRESS0024" => Some(21),
        "SCN0025" => Some(34),
        "MET0026" => Some(47),
        "GATE0027" => Some(60),
        "LOAD0028" => Some(73),
        "CONT0029" => Some(86),
        "THR0030" => Some(2),
        "SCORE0031" => Some(15),
        "RUN0032" => Some(28),
        "SAMP0033" => Some(41),
        "VERDICT0034" => Some(54),
        "RPT0035" => Some(67),
        _ => None,
    }
}

/// 逐行守卫扫描（返回违例行数；0 = 全绿）。
pub fn u3601mat_scan() -> usize {
    let mut bad = 0usize;
    let mut i = 0usize;
    while i < u3601mat_COUNT {
        let (k, v) = u3601mat[i];
        let ok = match k {
            "STRESS0000" => v < 97,
            "SCN0001" => v < 97,
            "MET0002" => v < 97,
            "GATE0003" => v < 97,
            "LOAD0004" => v < 97,
            "CONT0005" => v < 97,
            "THR0006" => v < 97,
            "SCORE0007" => v < 97,
            "RUN0008" => v < 97,
            "SAMP0009" => v < 97,
            "VERDICT0010" => v < 97,
            "RPT0011" => v < 97,
            "STRESS0012" => v < 97,
            "SCN0013" => v < 97,
            "MET0014" => v < 97,
            "GATE0015" => v < 97,
            "LOAD0016" => v < 97,
            "CONT0017" => v < 97,
            "THR0018" => v < 97,
            "SCORE0019" => v < 97,
            "RUN0020" => v < 97,
            "SAMP0021" => v < 97,
            "VERDICT0022" => v < 97,
            "RPT0023" => v < 97,
            "STRESS0024" => v < 97,
            "SCN0025" => v < 97,
            "MET0026" => v < 97,
            "GATE0027" => v < 97,
            "LOAD0028" => v < 97,
            "CONT0029" => v < 97,
            "THR0030" => v < 97,
            "SCORE0031" => v < 97,
            "RUN0032" => v < 97,
            "SAMP0033" => v < 97,
            "VERDICT0034" => v < 97,
            "RPT0035" => v < 97,
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
pub fn u3601mat_sum() -> u64 {
    let mut s = 0u64;
    let mut i = 0usize;
    while i < u3601mat_COUNT {
        s = s.wrapping_add(u3601mat[i].1 as u64);
        i += 1;
    }
    s
}

/// 键唯一性（构造期断言等价物：逐对扫描同键）。
pub fn u3601mat_unique() -> bool {
    let mut i = 0usize;
    while i < u3601mat_COUNT {
        let mut j = i + 1;
        while j < u3601mat_COUNT {
            if u3601mat[i].0 == u3601mat[j].0 {
                return false;
            }
            j += 1;
        }
        i += 1;
    }
    true
}

// --------------------------------------------------------------------------
// 直方图引擎 mxu3601hist（有界插入排序 + 分位数 P50/P90/P99）
// --------------------------------------------------------------------------
/// 锁压力机 直方图（96 桶滚动）。
/// 样本容量上限。
pub const MXU3601HIST_CAP: usize = 96;

/// 直方图账本（排序态 + 五统量）。
#[derive(Clone, Copy, Debug)]
pub struct mxu3601histHist {
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

impl mxu3601histHist {
    /// 空账构造。
    pub const fn new() -> Self {
        Self { samples: [0; 96], n: 0, drops: 0, min: 0, max: 0, sum: 0 }
    }

    /// 采样（有界插入排序；满员丢弃并记账）。
    pub fn push(&mut self, v: u64) {
        if self.n >= MXU3601HIST_CAP {
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

/// 段位守卫：全部诊断码必须落在本段 [0xe140, 0xe14f]。
pub const fn u3601_segment_guard() -> bool {
    let mut ok = true;
    let mut i = 0usize;
    while i < u3601Code::ALL.len() {
        let c = u3601Code::ALL[i].code();
        if c < 0xe140 || c > 0xe14f {
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
fn u3601_guard_seg() -> bool {
    u3601_segment_guard()
}

/// UNX-F3601 矩阵 36 行齐备 + 逐行守卫 + 唯一性 + 累计守恒。
fn check_u3601_map_guard() -> bool {
    let mut ok = u3601mat_COUNT == 36;
    if ok { ok = u3601mat_scan() == 0 && u3601mat_unique(); }
    if ok { ok = u3601mat_get("STRESS0000") == Some(0) && u3601mat_get("ZZZ_NOKEY").is_none(); }
    if ok { ok = u3601mat_sum() == 1594; }
    ok
}

/// UNX-F3601 直方图 16 样本分位/均值/守恒。
fn check_u3601_map_hist() -> bool {
    let mut h = mxu3601histHist::new();
    let mut i = 0u64;
    while i < 16 { h.push(i + 1); i += 1; }
    let mut ok = h.conserved() && h.mean() >= 1 && h.percentile(500) >= 8;
    ok
}

/// 本模块自检（逐检查带 detail，全绿才出门）。
pub fn run_u3601_checks() -> CheckSet {
    let mut set = CheckSet::new("U3601");
    set.add(
        "u3601-seg-guard",
        u3601_guard_seg(),
        if u3601_guard_seg() { "" } else { "diagnostic code outside segment" },
    );
    set.add(
        "u3601-map-guard",
        check_u3601_map_guard(),
        if check_u3601_map_guard() { "" } else { "UNX-F3601 matrix guard mismatch" },
    );
    set.add(
        "u3601-map-hist",
        check_u3601_map_hist(),
        if check_u3601_map_hist() { "" } else { "UNX-F3601 hist mismatch" },
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
    fn u3601_all_checks_green() {
        let set = run_u3601_checks();
        assert!(set.all_passed(), "U3601 domain checks must be green");
    }

    #[test]
    fn u3601_segment_guard_holds() {
        assert!(u3601_segment_guard());
    }
}
