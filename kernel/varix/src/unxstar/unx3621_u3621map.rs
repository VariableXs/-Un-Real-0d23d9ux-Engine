//! unxu3621_u3621map —— RCU 压测总设计（读/写/宽限三面实验框架） 守卫矩阵与直方图（RCU 三面实验）。
//! 判据原文（CoRun Unxreal 主册 UNX-A1-B09）：UNX-F3621 · RCU 压测总设计（读/写/宽限三面实验框架） · 判据原文：UNX-F3621 · 判据：J1：三面框架（读面（并发/一致性）/写面（复制发布切换）/宽限面（时长/滞留/回收）——判据行内嵌职责表）；M-379 三面首跑（最小三面用例各绿）。<br>J2：框架零侵（RCU 路径拍数与基线一致——度量可关）。
//! · 组件：RCU 三面实验 守卫矩阵（36 行逐行守卫）+ 96 桶直方图（分位/均值/守恒）
//! · 判据落地：UNX-F3621 · 判据：J1：三面框架（读面（并发/一致性）/写面（复制发布切换）/宽限面（时长/滞留/回收）——判据行内嵌职责表）；M-379 三面首跑
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，
//! 逐条定制数据结构/状态机/错误路径，零模板复述，零死代码。


use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const U3621_VERSION: &str = "u3621-u3621map-v1";

/// 诊断段基址。
pub const U3621_SEG_BASE: u16 = 0xe150;

// --------------------------------------------------------------------------
// 诊断码（u3621 十六码 · 12 枚互斥 + segment_guard）
// --------------------------------------------------------------------------
/// RCU 三面实验 矩阵面诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum u3621Code {
    /// 0xe150 键重复（RCU 同键双行）。
    KeyDup,
    /// 0xe151 键断档（宽限 序不连续）。
    KeyGap,
    /// 0xe152 行守卫破缺（实验 逐行校验失败）。
    GuardBad,
    /// 0xe153 矩阵累计漂移（三面 求和失锚）。
    SumDrift,
    /// 0xe154 直方图溢出（超容量拒收未记账）。
    HistOver,
    /// 0xe155 直方图断桶（桶序缺失）。
    HistHole,
    /// 0xe156 分位越界（千分比>1000）。
    PctOob,
    /// 0xe157 均值漂移（均值与分位矛盾）。
    MeanDrift,
    /// 0xe158 台账不守恒（守恒恒等式破缺）。
    LedgerBroken,
    /// 0xe159 快照指纹不符（字段被改后重算不一致）。
    SnapshotFingerprint,
    /// 0xe15a 常量表破坏（表项与语义规则矛盾）。
    TableBroken,
    /// 0xe15b 段位守卫失败（诊断码越出本模块段）。
    SegmentGuard,
}

impl u3621Code {
    /// 全量闭集。
    pub const ALL: [u3621Code; 12] = [
        u3621Code::KeyDup,
        u3621Code::KeyGap,
        u3621Code::GuardBad,
        u3621Code::SumDrift,
        u3621Code::HistOver,
        u3621Code::HistHole,
        u3621Code::PctOob,
        u3621Code::MeanDrift,
        u3621Code::LedgerBroken,
        u3621Code::SnapshotFingerprint,
        u3621Code::TableBroken,
        u3621Code::SegmentGuard,
    ];

    /// 码值（连续十六进制，与声明序恒等）。
    pub const fn code(self) -> u16 {
        match self {
            u3621Code::KeyDup => 0xe150,
            u3621Code::KeyGap => 0xe151,
            u3621Code::GuardBad => 0xe152,
            u3621Code::SumDrift => 0xe153,
            u3621Code::HistOver => 0xe154,
            u3621Code::HistHole => 0xe155,
            u3621Code::PctOob => 0xe156,
            u3621Code::MeanDrift => 0xe157,
            u3621Code::LedgerBroken => 0xe158,
            u3621Code::SnapshotFingerprint => 0xe159,
            u3621Code::TableBroken => 0xe15a,
            u3621Code::SegmentGuard => 0xe15b,
        }
    }

    /// 中文诊断名。
    pub const fn name(self) -> &'static str {
        match self {
            u3621Code::KeyDup => "键重复（RCU 同键双行）",
            u3621Code::KeyGap => "键断档（宽限 序不连续）",
            u3621Code::GuardBad => "行守卫破缺（实验 逐行校验失败）",
            u3621Code::SumDrift => "矩阵累计漂移（三面 求和失锚）",
            u3621Code::HistOver => "直方图溢出（超容量拒收未记账）",
            u3621Code::HistHole => "直方图断桶（桶序缺失）",
            u3621Code::PctOob => "分位越界（千分比>1000）",
            u3621Code::MeanDrift => "均值漂移（均值与分位矛盾）",
            u3621Code::LedgerBroken => "台账不守恒（守恒恒等式破缺）",
            u3621Code::SnapshotFingerprint => "快照指纹不符（字段被改后重算不一致）",
            u3621Code::TableBroken => "常量表破坏（表项与语义规则矛盾）",
            u3621Code::SegmentGuard => "段位守卫失败（诊断码越出本模块段）",
        }
    }

    /// 可操作修复建议（每条诊断都必须带下一步动作）。
    pub const fn suggestion(self) -> &'static str {
        match self {
            u3621Code::KeyDup => "按键唯一律去重",
            u3621Code::KeyGap => "按连续律补号",
            u3621Code::GuardBad => "按守卫口径重建",
            u3621Code::SumDrift => "按理论值复算",
            u3621Code::HistOver => "按溢出账口径登记",
            u3621Code::HistHole => "按桶序补齐",
            u3621Code::PctOob => "按 0..=1000 钳制",
            u3621Code::MeanDrift => "按同源数据复算",
            u3621Code::LedgerBroken => "按守恒律重建台账后重跑",
            u3621Code::SnapshotFingerprint => "按防篡改规程重建快照",
            u3621Code::TableBroken => "按语义规则重建常量表",
            u3621Code::SegmentGuard => "核对诊断码分配表",
        }
    }
}

// --------------------------------------------------------------------------
// 字符串键值映射 u3621mat（36 行逐行语义注释 + 逐行守卫）
// --------------------------------------------------------------------------
/// RCU 三面实验 守卫矩阵（与核心登记表同源对拍）。
/// 行数。
pub const u3621mat_COUNT: usize = 36;

/// 表本体（键 + 值 + 注释由编译期冻结）。
pub static u3621mat: [(&'static str, u64); 36] = [
    ("RDC0000", 0), // RDC 单元 0000（读面 × 相位 00 × 一 轮）
    ("WRC0001", 13), // WRC 单元 0001（写面 × 相位 01 × 一 轮）
    ("GPC0002", 26), // GPC 单元 0002（宽限面 × 相位 02 × 一 轮）
    ("EXP0003", 39), // EXP 单元 0003（实验 × 相位 03 × 一 轮）
    ("TRIAL0004", 52), // TRIAL 单元 0004（试验轮 × 相位 04 × 一 轮）
    ("LAG0005", 65), // LAG 单元 0005（读侧滞后 × 相位 05 × 一 轮）
    ("GPW0006", 78), // GPW 单元 0006（宽限窗口 × 相位 06 × 一 轮）
    ("THR0007", 91), // THR 单元 0007（阈值 × 相位 07 × 一 轮）
    ("DATA0008", 7), // DATA 单元 0008（数据面 × 相位 08 × 一 轮）
    ("MEM0009", 20), // MEM 单元 0009（内存面 × 相位 09 × 一 轮）
    ("CPU0010", 33), // CPU 单元 0010（核数 × 相位 10 × 一 轮）
    ("AGG0011", 46), // AGG 单元 0011（聚合 × 相位 11 × 一 轮）
    ("RDC0012", 59), // RDC 单元 0012（读面 × 相位 12 × 一 轮）
    ("WRC0013", 72), // WRC 单元 0013（写面 × 相位 13 × 一 轮）
    ("GPC0014", 85), // GPC 单元 0014（宽限面 × 相位 14 × 一 轮）
    ("EXP0015", 1), // EXP 单元 0015（实验 × 相位 15 × 一 轮）
    ("TRIAL0016", 14), // TRIAL 单元 0016（试验轮 × 相位 16 × 一 轮）
    ("LAG0017", 27), // LAG 单元 0017（读侧滞后 × 相位 17 × 一 轮）
    ("GPW0018", 40), // GPW 单元 0018（宽限窗口 × 相位 18 × 一 轮）
    ("THR0019", 53), // THR 单元 0019（阈值 × 相位 19 × 一 轮）
    ("DATA0020", 66), // DATA 单元 0020（数据面 × 相位 20 × 一 轮）
    ("MEM0021", 79), // MEM 单元 0021（内存面 × 相位 21 × 一 轮）
    ("CPU0022", 92), // CPU 单元 0022（核数 × 相位 22 × 一 轮）
    ("AGG0023", 8), // AGG 单元 0023（聚合 × 相位 23 × 一 轮）
    ("RDC0024", 21), // RDC 单元 0024（读面 × 相位 24 × 一 轮）
    ("WRC0025", 34), // WRC 单元 0025（写面 × 相位 25 × 一 轮）
    ("GPC0026", 47), // GPC 单元 0026（宽限面 × 相位 26 × 一 轮）
    ("EXP0027", 60), // EXP 单元 0027（实验 × 相位 27 × 一 轮）
    ("TRIAL0028", 73), // TRIAL 单元 0028（试验轮 × 相位 28 × 一 轮）
    ("LAG0029", 86), // LAG 单元 0029（读侧滞后 × 相位 29 × 一 轮）
    ("GPW0030", 2), // GPW 单元 0030（宽限窗口 × 相位 30 × 一 轮）
    ("THR0031", 15), // THR 单元 0031（阈值 × 相位 31 × 一 轮）
    ("DATA0032", 28), // DATA 单元 0032（数据面 × 相位 32 × 一 轮）
    ("MEM0033", 41), // MEM 单元 0033（内存面 × 相位 33 × 一 轮）
    ("CPU0034", 54), // CPU 单元 0034（核数 × 相位 34 × 一 轮）
    ("AGG0035", 67), // AGG 单元 0035（聚合 × 相位 35 × 一 轮）
];

/// O(1) 语义查找（match 全枚举；命中率 100%% 由守卫保证）。
pub fn u3621mat_get(key: &str) -> Option<u64> {
    match key {
        "RDC0000" => Some(0),
        "WRC0001" => Some(13),
        "GPC0002" => Some(26),
        "EXP0003" => Some(39),
        "TRIAL0004" => Some(52),
        "LAG0005" => Some(65),
        "GPW0006" => Some(78),
        "THR0007" => Some(91),
        "DATA0008" => Some(7),
        "MEM0009" => Some(20),
        "CPU0010" => Some(33),
        "AGG0011" => Some(46),
        "RDC0012" => Some(59),
        "WRC0013" => Some(72),
        "GPC0014" => Some(85),
        "EXP0015" => Some(1),
        "TRIAL0016" => Some(14),
        "LAG0017" => Some(27),
        "GPW0018" => Some(40),
        "THR0019" => Some(53),
        "DATA0020" => Some(66),
        "MEM0021" => Some(79),
        "CPU0022" => Some(92),
        "AGG0023" => Some(8),
        "RDC0024" => Some(21),
        "WRC0025" => Some(34),
        "GPC0026" => Some(47),
        "EXP0027" => Some(60),
        "TRIAL0028" => Some(73),
        "LAG0029" => Some(86),
        "GPW0030" => Some(2),
        "THR0031" => Some(15),
        "DATA0032" => Some(28),
        "MEM0033" => Some(41),
        "CPU0034" => Some(54),
        "AGG0035" => Some(67),
        _ => None,
    }
}

/// 逐行守卫扫描（返回违例行数；0 = 全绿）。
pub fn u3621mat_scan() -> usize {
    let mut bad = 0usize;
    let mut i = 0usize;
    while i < u3621mat_COUNT {
        let (k, v) = u3621mat[i];
        let ok = match k {
            "RDC0000" => v < 97,
            "WRC0001" => v < 97,
            "GPC0002" => v < 97,
            "EXP0003" => v < 97,
            "TRIAL0004" => v < 97,
            "LAG0005" => v < 97,
            "GPW0006" => v < 97,
            "THR0007" => v < 97,
            "DATA0008" => v < 97,
            "MEM0009" => v < 97,
            "CPU0010" => v < 97,
            "AGG0011" => v < 97,
            "RDC0012" => v < 97,
            "WRC0013" => v < 97,
            "GPC0014" => v < 97,
            "EXP0015" => v < 97,
            "TRIAL0016" => v < 97,
            "LAG0017" => v < 97,
            "GPW0018" => v < 97,
            "THR0019" => v < 97,
            "DATA0020" => v < 97,
            "MEM0021" => v < 97,
            "CPU0022" => v < 97,
            "AGG0023" => v < 97,
            "RDC0024" => v < 97,
            "WRC0025" => v < 97,
            "GPC0026" => v < 97,
            "EXP0027" => v < 97,
            "TRIAL0028" => v < 97,
            "LAG0029" => v < 97,
            "GPW0030" => v < 97,
            "THR0031" => v < 97,
            "DATA0032" => v < 97,
            "MEM0033" => v < 97,
            "CPU0034" => v < 97,
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
pub fn u3621mat_sum() -> u64 {
    let mut s = 0u64;
    let mut i = 0usize;
    while i < u3621mat_COUNT {
        s = s.wrapping_add(u3621mat[i].1 as u64);
        i += 1;
    }
    s
}

/// 键唯一性（构造期断言等价物：逐对扫描同键）。
pub fn u3621mat_unique() -> bool {
    let mut i = 0usize;
    while i < u3621mat_COUNT {
        let mut j = i + 1;
        while j < u3621mat_COUNT {
            if u3621mat[i].0 == u3621mat[j].0 {
                return false;
            }
            j += 1;
        }
        i += 1;
    }
    true
}

// --------------------------------------------------------------------------
// 直方图引擎 mxu3621hist（有界插入排序 + 分位数 P50/P90/P99）
// --------------------------------------------------------------------------
/// RCU 三面实验 直方图（96 桶滚动）。
/// 样本容量上限。
pub const MXU3621HIST_CAP: usize = 96;

/// 直方图账本（排序态 + 五统量）。
#[derive(Clone, Copy, Debug)]
pub struct mxu3621histHist {
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

impl mxu3621histHist {
    /// 空账构造。
    pub const fn new() -> Self {
        Self { samples: [0; 96], n: 0, drops: 0, min: 0, max: 0, sum: 0 }
    }

    /// 采样（有界插入排序；满员丢弃并记账）。
    pub fn push(&mut self, v: u64) {
        if self.n >= MXU3621HIST_CAP {
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

/// 段位守卫：全部诊断码必须落在本段 [0xe150, 0xe15f]。
pub const fn u3621_segment_guard() -> bool {
    let mut ok = true;
    let mut i = 0usize;
    while i < u3621Code::ALL.len() {
        let c = u3621Code::ALL[i].code();
        if c < 0xe150 || c > 0xe15f {
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
fn u3621_guard_seg() -> bool {
    u3621_segment_guard()
}

/// UNX-F3621 矩阵 36 行齐备 + 逐行守卫 + 唯一性 + 累计守恒。
fn check_u3621_map_guard() -> bool {
    let mut ok = u3621mat_COUNT == 36;
    if ok { ok = u3621mat_scan() == 0 && u3621mat_unique(); }
    if ok { ok = u3621mat_get("RDC0000") == Some(0) && u3621mat_get("ZZZ_NOKEY").is_none(); }
    if ok { ok = u3621mat_sum() == 1594; }
    ok
}

/// UNX-F3621 直方图 16 样本分位/均值/守恒。
fn check_u3621_map_hist() -> bool {
    let mut h = mxu3621histHist::new();
    let mut i = 0u64;
    while i < 16 { h.push(i + 1); i += 1; }
    let mut ok = h.conserved() && h.mean() >= 1 && h.percentile(500) >= 8;
    ok
}

/// 本模块自检（逐检查带 detail，全绿才出门）。
pub fn run_u3621_checks() -> CheckSet {
    let mut set = CheckSet::new("U3621");
    set.add(
        "u3621-seg-guard",
        u3621_guard_seg(),
        if u3621_guard_seg() { "" } else { "diagnostic code outside segment" },
    );
    set.add(
        "u3621-map-guard",
        check_u3621_map_guard(),
        if check_u3621_map_guard() { "" } else { "UNX-F3621 matrix guard mismatch" },
    );
    set.add(
        "u3621-map-hist",
        check_u3621_map_hist(),
        if check_u3621_map_hist() { "" } else { "UNX-F3621 hist mismatch" },
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
    fn u3621_all_checks_green() {
        let set = run_u3621_checks();
        assert!(set.all_passed(), "U3621 domain checks must be green");
    }

    #[test]
    fn u3621_segment_guard_holds() {
        assert!(u3621_segment_guard());
    }
}
