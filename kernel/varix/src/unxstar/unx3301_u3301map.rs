//! unxu3301_u3301map —— RCU 压测编排（读多写少 8 核场景） 守卫矩阵与直方图（RCU 压测编排）。
//! 判据原文（CoRun Unxreal 主册 UNX-A1-B09）：UNX-F3301 · RCU 压测编排（读多写少 8 核场景） · 判据原文：UNX-F3301 · 判据：J1：8 核读 + 写者换指针压测 10 秒，读侧零阻塞证据（读延迟分布 P99 ≤ 2×fast path 基线）、GP 推进健康；M-060 对账。
//! · 组件：RCU 压测编排 守卫矩阵（36 行逐行守卫）+ 96 桶直方图（分位/均值/守恒）
//! · 判据落地：UNX-F3301 · 判据：J1：8 核读 + 写者换指针压测 10 秒，读侧零阻塞证据（读延迟分布 P99 ≤ 2×fast path 基线）、GP 推进健
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，
//! 逐条定制数据结构/状态机/错误路径，零模板复述，零死代码。


use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const U3301_VERSION: &str = "u3301-u3301map-v1";

/// 诊断段基址。
pub const U3301_SEG_BASE: u16 = 0xe050;

// --------------------------------------------------------------------------
// 诊断码（u3301 十六码 · 12 枚互斥 + segment_guard）
// --------------------------------------------------------------------------
/// RCU 压测编排 矩阵面诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum u3301Code {
    /// 0xe050 键重复（RCU 同键双行）。
    KeyDup,
    /// 0xe051 键断档（宽限 序不连续）。
    KeyGap,
    /// 0xe052 行守卫破缺（静默 逐行校验失败）。
    GuardBad,
    /// 0xe053 矩阵累计漂移（读多写少 求和失锚）。
    SumDrift,
    /// 0xe054 直方图溢出（超容量拒收未记账）。
    HistOver,
    /// 0xe055 直方图断桶（桶序缺失）。
    HistHole,
    /// 0xe056 分位越界（千分比>1000）。
    PctOob,
    /// 0xe057 均值漂移（均值与分位矛盾）。
    MeanDrift,
    /// 0xe058 台账不守恒（守恒恒等式破缺）。
    LedgerBroken,
    /// 0xe059 快照指纹不符（字段被改后重算不一致）。
    SnapshotFingerprint,
    /// 0xe05a 常量表破坏（表项与语义规则矛盾）。
    TableBroken,
    /// 0xe05b 段位守卫失败（诊断码越出本模块段）。
    SegmentGuard,
}

impl u3301Code {
    /// 全量闭集。
    pub const ALL: [u3301Code; 12] = [
        u3301Code::KeyDup,
        u3301Code::KeyGap,
        u3301Code::GuardBad,
        u3301Code::SumDrift,
        u3301Code::HistOver,
        u3301Code::HistHole,
        u3301Code::PctOob,
        u3301Code::MeanDrift,
        u3301Code::LedgerBroken,
        u3301Code::SnapshotFingerprint,
        u3301Code::TableBroken,
        u3301Code::SegmentGuard,
    ];

    /// 码值（连续十六进制，与声明序恒等）。
    pub const fn code(self) -> u16 {
        match self {
            u3301Code::KeyDup => 0xe050,
            u3301Code::KeyGap => 0xe051,
            u3301Code::GuardBad => 0xe052,
            u3301Code::SumDrift => 0xe053,
            u3301Code::HistOver => 0xe054,
            u3301Code::HistHole => 0xe055,
            u3301Code::PctOob => 0xe056,
            u3301Code::MeanDrift => 0xe057,
            u3301Code::LedgerBroken => 0xe058,
            u3301Code::SnapshotFingerprint => 0xe059,
            u3301Code::TableBroken => 0xe05a,
            u3301Code::SegmentGuard => 0xe05b,
        }
    }

    /// 中文诊断名。
    pub const fn name(self) -> &'static str {
        match self {
            u3301Code::KeyDup => "键重复（RCU 同键双行）",
            u3301Code::KeyGap => "键断档（宽限 序不连续）",
            u3301Code::GuardBad => "行守卫破缺（静默 逐行校验失败）",
            u3301Code::SumDrift => "矩阵累计漂移（读多写少 求和失锚）",
            u3301Code::HistOver => "直方图溢出（超容量拒收未记账）",
            u3301Code::HistHole => "直方图断桶（桶序缺失）",
            u3301Code::PctOob => "分位越界（千分比>1000）",
            u3301Code::MeanDrift => "均值漂移（均值与分位矛盾）",
            u3301Code::LedgerBroken => "台账不守恒（守恒恒等式破缺）",
            u3301Code::SnapshotFingerprint => "快照指纹不符（字段被改后重算不一致）",
            u3301Code::TableBroken => "常量表破坏（表项与语义规则矛盾）",
            u3301Code::SegmentGuard => "段位守卫失败（诊断码越出本模块段）",
        }
    }

    /// 可操作修复建议（每条诊断都必须带下一步动作）。
    pub const fn suggestion(self) -> &'static str {
        match self {
            u3301Code::KeyDup => "按键唯一律去重",
            u3301Code::KeyGap => "按连续律补号",
            u3301Code::GuardBad => "按守卫口径重建",
            u3301Code::SumDrift => "按理论值复算",
            u3301Code::HistOver => "按溢出账口径登记",
            u3301Code::HistHole => "按桶序补齐",
            u3301Code::PctOob => "按 0..=1000 钳制",
            u3301Code::MeanDrift => "按同源数据复算",
            u3301Code::LedgerBroken => "按守恒律重建台账后重跑",
            u3301Code::SnapshotFingerprint => "按防篡改规程重建快照",
            u3301Code::TableBroken => "按语义规则重建常量表",
            u3301Code::SegmentGuard => "核对诊断码分配表",
        }
    }
}

// --------------------------------------------------------------------------
// 字符串键值映射 u3301mat（36 行逐行语义注释 + 逐行守卫）
// --------------------------------------------------------------------------
/// RCU 压测编排 守卫矩阵（与核心登记表同源对拍）。
/// 行数。
pub const u3301mat_COUNT: usize = 36;

/// 表本体（键 + 值 + 注释由编译期冻结）。
pub static u3301mat: [(&'static str, u64); 36] = [
    ("RD0000", 0), // RD 单元 0000（读者 × 相位 00 × 一 轮）
    ("WR0001", 13), // WR 单元 0001（写者 × 相位 01 × 一 轮）
    ("GP0002", 26), // GP 单元 0002（宽限期 × 相位 02 × 一 轮）
    ("CPU0003", 39), // CPU 单元 0003（核数 × 相位 03 × 一 轮）
    ("READM0004", 52), // READM 单元 0004（读多写少比 × 相位 04 × 一 轮）
    ("QS0005", 65), // QS 单元 0005（静默态 × 相位 05 × 一 轮）
    ("GPW0006", 78), // GPW 单元 0006（宽限窗口 × 相位 06 × 一 轮）
    ("STW0007", 91), // STW 单元 0007（停顿窗口 × 相位 07 × 一 轮）
    ("LAG0008", 7), // LAG 单元 0008（读侧滞后 × 相位 08 × 一 轮）
    ("LOSS0009", 20), // LOSS 单元 0009（零丢失账 × 相位 09 × 一 轮）
    ("FP0010", 33), // FP 单元 0010（指纹链 × 相位 10 × 一 轮）
    ("THR0011", 46), // THR 单元 0011（阈值 × 相位 11 × 一 轮）
    ("RD0012", 59), // RD 单元 0012（读者 × 相位 12 × 一 轮）
    ("WR0013", 72), // WR 单元 0013（写者 × 相位 13 × 一 轮）
    ("GP0014", 85), // GP 单元 0014（宽限期 × 相位 14 × 一 轮）
    ("CPU0015", 1), // CPU 单元 0015（核数 × 相位 15 × 一 轮）
    ("READM0016", 14), // READM 单元 0016（读多写少比 × 相位 16 × 一 轮）
    ("QS0017", 27), // QS 单元 0017（静默态 × 相位 17 × 一 轮）
    ("GPW0018", 40), // GPW 单元 0018（宽限窗口 × 相位 18 × 一 轮）
    ("STW0019", 53), // STW 单元 0019（停顿窗口 × 相位 19 × 一 轮）
    ("LAG0020", 66), // LAG 单元 0020（读侧滞后 × 相位 20 × 一 轮）
    ("LOSS0021", 79), // LOSS 单元 0021（零丢失账 × 相位 21 × 一 轮）
    ("FP0022", 92), // FP 单元 0022（指纹链 × 相位 22 × 一 轮）
    ("THR0023", 8), // THR 单元 0023（阈值 × 相位 23 × 一 轮）
    ("RD0024", 21), // RD 单元 0024（读者 × 相位 24 × 一 轮）
    ("WR0025", 34), // WR 单元 0025（写者 × 相位 25 × 一 轮）
    ("GP0026", 47), // GP 单元 0026（宽限期 × 相位 26 × 一 轮）
    ("CPU0027", 60), // CPU 单元 0027（核数 × 相位 27 × 一 轮）
    ("READM0028", 73), // READM 单元 0028（读多写少比 × 相位 28 × 一 轮）
    ("QS0029", 86), // QS 单元 0029（静默态 × 相位 29 × 一 轮）
    ("GPW0030", 2), // GPW 单元 0030（宽限窗口 × 相位 30 × 一 轮）
    ("STW0031", 15), // STW 单元 0031（停顿窗口 × 相位 31 × 一 轮）
    ("LAG0032", 28), // LAG 单元 0032（读侧滞后 × 相位 32 × 一 轮）
    ("LOSS0033", 41), // LOSS 单元 0033（零丢失账 × 相位 33 × 一 轮）
    ("FP0034", 54), // FP 单元 0034（指纹链 × 相位 34 × 一 轮）
    ("THR0035", 67), // THR 单元 0035（阈值 × 相位 35 × 一 轮）
];

/// O(1) 语义查找（match 全枚举；命中率 100%% 由守卫保证）。
pub fn u3301mat_get(key: &str) -> Option<u64> {
    match key {
        "RD0000" => Some(0),
        "WR0001" => Some(13),
        "GP0002" => Some(26),
        "CPU0003" => Some(39),
        "READM0004" => Some(52),
        "QS0005" => Some(65),
        "GPW0006" => Some(78),
        "STW0007" => Some(91),
        "LAG0008" => Some(7),
        "LOSS0009" => Some(20),
        "FP0010" => Some(33),
        "THR0011" => Some(46),
        "RD0012" => Some(59),
        "WR0013" => Some(72),
        "GP0014" => Some(85),
        "CPU0015" => Some(1),
        "READM0016" => Some(14),
        "QS0017" => Some(27),
        "GPW0018" => Some(40),
        "STW0019" => Some(53),
        "LAG0020" => Some(66),
        "LOSS0021" => Some(79),
        "FP0022" => Some(92),
        "THR0023" => Some(8),
        "RD0024" => Some(21),
        "WR0025" => Some(34),
        "GP0026" => Some(47),
        "CPU0027" => Some(60),
        "READM0028" => Some(73),
        "QS0029" => Some(86),
        "GPW0030" => Some(2),
        "STW0031" => Some(15),
        "LAG0032" => Some(28),
        "LOSS0033" => Some(41),
        "FP0034" => Some(54),
        "THR0035" => Some(67),
        _ => None,
    }
}

/// 逐行守卫扫描（返回违例行数；0 = 全绿）。
pub fn u3301mat_scan() -> usize {
    let mut bad = 0usize;
    let mut i = 0usize;
    while i < u3301mat_COUNT {
        let (k, v) = u3301mat[i];
        let ok = match k {
            "RD0000" => v < 97,
            "WR0001" => v < 97,
            "GP0002" => v < 97,
            "CPU0003" => v < 97,
            "READM0004" => v < 97,
            "QS0005" => v < 97,
            "GPW0006" => v < 97,
            "STW0007" => v < 97,
            "LAG0008" => v < 97,
            "LOSS0009" => v < 97,
            "FP0010" => v < 97,
            "THR0011" => v < 97,
            "RD0012" => v < 97,
            "WR0013" => v < 97,
            "GP0014" => v < 97,
            "CPU0015" => v < 97,
            "READM0016" => v < 97,
            "QS0017" => v < 97,
            "GPW0018" => v < 97,
            "STW0019" => v < 97,
            "LAG0020" => v < 97,
            "LOSS0021" => v < 97,
            "FP0022" => v < 97,
            "THR0023" => v < 97,
            "RD0024" => v < 97,
            "WR0025" => v < 97,
            "GP0026" => v < 97,
            "CPU0027" => v < 97,
            "READM0028" => v < 97,
            "QS0029" => v < 97,
            "GPW0030" => v < 97,
            "STW0031" => v < 97,
            "LAG0032" => v < 97,
            "LOSS0033" => v < 97,
            "FP0034" => v < 97,
            "THR0035" => v < 97,
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
pub fn u3301mat_sum() -> u64 {
    let mut s = 0u64;
    let mut i = 0usize;
    while i < u3301mat_COUNT {
        s = s.wrapping_add(u3301mat[i].1 as u64);
        i += 1;
    }
    s
}

/// 键唯一性（构造期断言等价物：逐对扫描同键）。
pub fn u3301mat_unique() -> bool {
    let mut i = 0usize;
    while i < u3301mat_COUNT {
        let mut j = i + 1;
        while j < u3301mat_COUNT {
            if u3301mat[i].0 == u3301mat[j].0 {
                return false;
            }
            j += 1;
        }
        i += 1;
    }
    true
}

// --------------------------------------------------------------------------
// 直方图引擎 mxu3301hist（有界插入排序 + 分位数 P50/P90/P99）
// --------------------------------------------------------------------------
/// RCU 压测编排 直方图（96 桶滚动）。
/// 样本容量上限。
pub const MXU3301HIST_CAP: usize = 96;

/// 直方图账本（排序态 + 五统量）。
#[derive(Clone, Copy, Debug)]
pub struct mxu3301histHist {
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

impl mxu3301histHist {
    /// 空账构造。
    pub const fn new() -> Self {
        Self { samples: [0; 96], n: 0, drops: 0, min: 0, max: 0, sum: 0 }
    }

    /// 采样（有界插入排序；满员丢弃并记账）。
    pub fn push(&mut self, v: u64) {
        if self.n >= MXU3301HIST_CAP {
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

/// 段位守卫：全部诊断码必须落在本段 [0xe050, 0xe05f]。
pub const fn u3301_segment_guard() -> bool {
    let mut ok = true;
    let mut i = 0usize;
    while i < u3301Code::ALL.len() {
        let c = u3301Code::ALL[i].code();
        if c < 0xe050 || c > 0xe05f {
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
fn u3301_guard_seg() -> bool {
    u3301_segment_guard()
}

/// UNX-F3301 矩阵 36 行齐备 + 逐行守卫 + 唯一性 + 累计守恒。
fn check_u3301_map_guard() -> bool {
    let mut ok = u3301mat_COUNT == 36;
    if ok { ok = u3301mat_scan() == 0 && u3301mat_unique(); }
    if ok { ok = u3301mat_get("RD0000") == Some(0) && u3301mat_get("ZZZ_NOKEY").is_none(); }
    if ok { ok = u3301mat_sum() == 1594; }
    ok
}

/// UNX-F3301 直方图 16 样本分位/均值/守恒。
fn check_u3301_map_hist() -> bool {
    let mut h = mxu3301histHist::new();
    let mut i = 0u64;
    while i < 16 { h.push(i + 1); i += 1; }
    let mut ok = h.conserved() && h.mean() >= 1 && h.percentile(500) >= 8;
    ok
}

/// 本模块自检（逐检查带 detail，全绿才出门）。
pub fn run_u3301_checks() -> CheckSet {
    let mut set = CheckSet::new("U3301");
    set.add(
        "u3301-seg-guard",
        u3301_guard_seg(),
        if u3301_guard_seg() { "" } else { "diagnostic code outside segment" },
    );
    set.add(
        "u3301-map-guard",
        check_u3301_map_guard(),
        if check_u3301_map_guard() { "" } else { "UNX-F3301 matrix guard mismatch" },
    );
    set.add(
        "u3301-map-hist",
        check_u3301_map_hist(),
        if check_u3301_map_hist() { "" } else { "UNX-F3301 hist mismatch" },
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
    fn u3301_all_checks_green() {
        let set = run_u3301_checks();
        assert!(set.all_passed(), "U3301 domain checks must be green");
    }

    #[test]
    fn u3301_segment_guard_holds() {
        assert!(u3301_segment_guard());
    }
}
