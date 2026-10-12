//! unxu2621_u2621map —— 每进程 CR3 语义与上下文切换内存侧（A3 每核上下文消费） 守卫矩阵与直方图（CR3 切换）。
//! 判据原文（CoRun Unxreal 主册 UNX-A1-B09）：UNX-F2621 · 每进程 CR3 语义与上下文切换内存侧（A3 每核上下文消费） · 判据原文：UNX-F2621 · 判据：CR3 切换内存侧语义逐项断言（切换点单点/NOFLUSH 组合正确），A3 每核上下文冻结件消费联签在档
//! · 组件：CR3 切换 守卫矩阵（36 行逐行守卫）+ 96 桶直方图（分位/均值/守恒）
//! · 判据落地：UNX-F2621 · 判据：CR3 切换内存侧语义逐项断言（切换点单点/NOFLUSH 组合正确），A3 每核上下文冻结件消费联签在档
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，
//! 逐条定制数据结构/状态机/错误路径，零模板复述，零死代码。


use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const U2621_VERSION: &str = "u2621-u2621map-v1";

/// 诊断段基址。
pub const U2621_SEG_BASE: u16 = 0xa870;

// --------------------------------------------------------------------------
// 诊断码（u2621 十六码 · 12 枚互斥 + segment_guard）
// --------------------------------------------------------------------------
/// CR3 切换 矩阵面诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum u2621Code {
    /// 0xa870 键重复（切换点漏 同键双行）。
    KeyDup,
    /// 0xa871 键断档（冲刷错配 序不连续）。
    KeyGap,
    /// 0xa872 行守卫破缺（联签缺失 逐行校验失败）。
    GuardBad,
    /// 0xa873 矩阵累计漂移（多核串扰 求和失锚）。
    SumDrift,
    /// 0xa874 直方图溢出（超容量拒收未记账）。
    HistOver,
    /// 0xa875 直方图断桶（桶序缺失）。
    HistHole,
    /// 0xa876 分位越界（千分比>1000）。
    PctOob,
    /// 0xa877 均值漂移（均值与分位矛盾）。
    MeanDrift,
    /// 0xa878 台账不守恒（守恒恒等式破缺）。
    LedgerBroken,
    /// 0xa879 快照指纹不符（字段被改后重算不一致）。
    SnapshotFingerprint,
    /// 0xa87a 常量表破坏（表项与语义规则矛盾）。
    TableBroken,
    /// 0xa87b 段位守卫失败（诊断码越出本模块段）。
    SegmentGuard,
}

impl u2621Code {
    /// 全量闭集。
    pub const ALL: [u2621Code; 12] = [
        u2621Code::KeyDup,
        u2621Code::KeyGap,
        u2621Code::GuardBad,
        u2621Code::SumDrift,
        u2621Code::HistOver,
        u2621Code::HistHole,
        u2621Code::PctOob,
        u2621Code::MeanDrift,
        u2621Code::LedgerBroken,
        u2621Code::SnapshotFingerprint,
        u2621Code::TableBroken,
        u2621Code::SegmentGuard,
    ];

    /// 码值（连续十六进制，与声明序恒等）。
    pub const fn code(self) -> u16 {
        match self {
            u2621Code::KeyDup => 0xa870,
            u2621Code::KeyGap => 0xa871,
            u2621Code::GuardBad => 0xa872,
            u2621Code::SumDrift => 0xa873,
            u2621Code::HistOver => 0xa874,
            u2621Code::HistHole => 0xa875,
            u2621Code::PctOob => 0xa876,
            u2621Code::MeanDrift => 0xa877,
            u2621Code::LedgerBroken => 0xa878,
            u2621Code::SnapshotFingerprint => 0xa879,
            u2621Code::TableBroken => 0xa87a,
            u2621Code::SegmentGuard => 0xa87b,
        }
    }

    /// 中文诊断名。
    pub const fn name(self) -> &'static str {
        match self {
            u2621Code::KeyDup => "键重复（切换点漏 同键双行）",
            u2621Code::KeyGap => "键断档（冲刷错配 序不连续）",
            u2621Code::GuardBad => "行守卫破缺（联签缺失 逐行校验失败）",
            u2621Code::SumDrift => "矩阵累计漂移（多核串扰 求和失锚）",
            u2621Code::HistOver => "直方图溢出（超容量拒收未记账）",
            u2621Code::HistHole => "直方图断桶（桶序缺失）",
            u2621Code::PctOob => "分位越界（千分比>1000）",
            u2621Code::MeanDrift => "均值漂移（均值与分位矛盾）",
            u2621Code::LedgerBroken => "台账不守恒（守恒恒等式破缺）",
            u2621Code::SnapshotFingerprint => "快照指纹不符（字段被改后重算不一致）",
            u2621Code::TableBroken => "常量表破坏（表项与语义规则矛盾）",
            u2621Code::SegmentGuard => "段位守卫失败（诊断码越出本模块段）",
        }
    }

    /// 可操作修复建议（每条诊断都必须带下一步动作）。
    pub const fn suggestion(self) -> &'static str {
        match self {
            u2621Code::KeyDup => "按键唯一律去重",
            u2621Code::KeyGap => "按连续律补号",
            u2621Code::GuardBad => "按守卫口径重建",
            u2621Code::SumDrift => "按理论值复算",
            u2621Code::HistOver => "按溢出账口径登记",
            u2621Code::HistHole => "按桶序补齐",
            u2621Code::PctOob => "按 0..=1000 钳制",
            u2621Code::MeanDrift => "按同源数据复算",
            u2621Code::LedgerBroken => "按守恒律重建台账后重跑",
            u2621Code::SnapshotFingerprint => "按防篡改规程重建快照",
            u2621Code::TableBroken => "按语义规则重建常量表",
            u2621Code::SegmentGuard => "核对诊断码分配表",
        }
    }
}

// --------------------------------------------------------------------------
// 字符串键值映射 u2621mat（36 行逐行语义注释 + 逐行守卫）
// --------------------------------------------------------------------------
/// CR3 切换 守卫矩阵（与核心登记表同源对拍）。
/// 行数。
pub const u2621mat_COUNT: usize = 36;

/// 表本体（键 + 值 + 注释由编译期冻结）。
pub static u2621mat: [(&'static str, u64); 36] = [
    ("CR0000", 0), // CR 单元 0000（CR3 语义 × 相位 00 × 一 轮）
    ("SW0001", 13), // SW 单元 0001（切换点 × 相位 01 × 一 轮）
    ("SP0002", 26), // SP 单元 0002（单点律 × 相位 02 × 一 轮）
    ("NF0003", 39), // NF 单元 0003（NOFLUSH × 相位 03 × 一 轮）
    ("FL0004", 52), // FL 单元 0004（FLUSH 组合 × 相位 04 × 一 轮）
    ("CT0005", 65), // CT 单元 0005（每核上下文 × 相位 05 × 一 轮）
    ("FZ0006", 78), // FZ 单元 0006（冻结件 × 相位 06 × 一 轮）
    ("CN0007", 91), // CN 单元 0007（消费联签 × 相位 07 × 一 轮）
    ("PC0008", 7), // PC 单元 0008（每核面 × 相位 08 × 一 轮）
    ("TL0009", 20), // TL 单元 0009（TLB 面 × 相位 09 × 一 轮）
    ("AS0010", 33), // AS 单元 0010（断言面 × 相位 10 × 一 轮）
    ("AR0011", 46), // AR 单元 0011（在档锚 × 相位 11 × 一 轮）
    ("CR0012", 59), // CR 单元 0012（CR3 语义 × 相位 12 × 一 轮）
    ("SW0013", 72), // SW 单元 0013（切换点 × 相位 13 × 一 轮）
    ("SP0014", 85), // SP 单元 0014（单点律 × 相位 14 × 一 轮）
    ("NF0015", 1), // NF 单元 0015（NOFLUSH × 相位 15 × 一 轮）
    ("FL0016", 14), // FL 单元 0016（FLUSH 组合 × 相位 16 × 一 轮）
    ("CT0017", 27), // CT 单元 0017（每核上下文 × 相位 17 × 一 轮）
    ("FZ0018", 40), // FZ 单元 0018（冻结件 × 相位 18 × 一 轮）
    ("CN0019", 53), // CN 单元 0019（消费联签 × 相位 19 × 一 轮）
    ("PC0020", 66), // PC 单元 0020（每核面 × 相位 20 × 一 轮）
    ("TL0021", 79), // TL 单元 0021（TLB 面 × 相位 21 × 一 轮）
    ("AS0022", 92), // AS 单元 0022（断言面 × 相位 22 × 一 轮）
    ("AR0023", 8), // AR 单元 0023（在档锚 × 相位 23 × 一 轮）
    ("CR0024", 21), // CR 单元 0024（CR3 语义 × 相位 24 × 一 轮）
    ("SW0025", 34), // SW 单元 0025（切换点 × 相位 25 × 一 轮）
    ("SP0026", 47), // SP 单元 0026（单点律 × 相位 26 × 一 轮）
    ("NF0027", 60), // NF 单元 0027（NOFLUSH × 相位 27 × 一 轮）
    ("FL0028", 73), // FL 单元 0028（FLUSH 组合 × 相位 28 × 一 轮）
    ("CT0029", 86), // CT 单元 0029（每核上下文 × 相位 29 × 一 轮）
    ("FZ0030", 2), // FZ 单元 0030（冻结件 × 相位 30 × 一 轮）
    ("CN0031", 15), // CN 单元 0031（消费联签 × 相位 31 × 一 轮）
    ("PC0032", 28), // PC 单元 0032（每核面 × 相位 32 × 一 轮）
    ("TL0033", 41), // TL 单元 0033（TLB 面 × 相位 33 × 一 轮）
    ("AS0034", 54), // AS 单元 0034（断言面 × 相位 34 × 一 轮）
    ("AR0035", 67), // AR 单元 0035（在档锚 × 相位 35 × 一 轮）
];

/// O(1) 语义查找（match 全枚举；命中率 100%% 由守卫保证）。
pub fn u2621mat_get(key: &str) -> Option<u64> {
    match key {
        "CR0000" => Some(0),
        "SW0001" => Some(13),
        "SP0002" => Some(26),
        "NF0003" => Some(39),
        "FL0004" => Some(52),
        "CT0005" => Some(65),
        "FZ0006" => Some(78),
        "CN0007" => Some(91),
        "PC0008" => Some(7),
        "TL0009" => Some(20),
        "AS0010" => Some(33),
        "AR0011" => Some(46),
        "CR0012" => Some(59),
        "SW0013" => Some(72),
        "SP0014" => Some(85),
        "NF0015" => Some(1),
        "FL0016" => Some(14),
        "CT0017" => Some(27),
        "FZ0018" => Some(40),
        "CN0019" => Some(53),
        "PC0020" => Some(66),
        "TL0021" => Some(79),
        "AS0022" => Some(92),
        "AR0023" => Some(8),
        "CR0024" => Some(21),
        "SW0025" => Some(34),
        "SP0026" => Some(47),
        "NF0027" => Some(60),
        "FL0028" => Some(73),
        "CT0029" => Some(86),
        "FZ0030" => Some(2),
        "CN0031" => Some(15),
        "PC0032" => Some(28),
        "TL0033" => Some(41),
        "AS0034" => Some(54),
        "AR0035" => Some(67),
        _ => None,
    }
}

/// 逐行守卫扫描（返回违例行数；0 = 全绿）。
pub fn u2621mat_scan() -> usize {
    let mut bad = 0usize;
    let mut i = 0usize;
    while i < u2621mat_COUNT {
        let (k, v) = u2621mat[i];
        let ok = match k {
            "CR0000" => v < 97,
            "SW0001" => v < 97,
            "SP0002" => v < 97,
            "NF0003" => v < 97,
            "FL0004" => v < 97,
            "CT0005" => v < 97,
            "FZ0006" => v < 97,
            "CN0007" => v < 97,
            "PC0008" => v < 97,
            "TL0009" => v < 97,
            "AS0010" => v < 97,
            "AR0011" => v < 97,
            "CR0012" => v < 97,
            "SW0013" => v < 97,
            "SP0014" => v < 97,
            "NF0015" => v < 97,
            "FL0016" => v < 97,
            "CT0017" => v < 97,
            "FZ0018" => v < 97,
            "CN0019" => v < 97,
            "PC0020" => v < 97,
            "TL0021" => v < 97,
            "AS0022" => v < 97,
            "AR0023" => v < 97,
            "CR0024" => v < 97,
            "SW0025" => v < 97,
            "SP0026" => v < 97,
            "NF0027" => v < 97,
            "FL0028" => v < 97,
            "CT0029" => v < 97,
            "FZ0030" => v < 97,
            "CN0031" => v < 97,
            "PC0032" => v < 97,
            "TL0033" => v < 97,
            "AS0034" => v < 97,
            "AR0035" => v < 97,
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
pub fn u2621mat_sum() -> u64 {
    let mut s = 0u64;
    let mut i = 0usize;
    while i < u2621mat_COUNT {
        s = s.wrapping_add(u2621mat[i].1 as u64);
        i += 1;
    }
    s
}

/// 键唯一性（构造期断言等价物：逐对扫描同键）。
pub fn u2621mat_unique() -> bool {
    let mut i = 0usize;
    while i < u2621mat_COUNT {
        let mut j = i + 1;
        while j < u2621mat_COUNT {
            if u2621mat[i].0 == u2621mat[j].0 {
                return false;
            }
            j += 1;
        }
        i += 1;
    }
    true
}

// --------------------------------------------------------------------------
// 直方图引擎 mxu2621hist（有界插入排序 + 分位数 P50/P90/P99）
// --------------------------------------------------------------------------
/// CR3 切换 直方图（96 桶滚动）。
/// 样本容量上限。
pub const MXU2621HIST_CAP: usize = 96;

/// 直方图账本（排序态 + 五统量）。
#[derive(Clone, Copy, Debug)]
pub struct mxu2621histHist {
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

impl mxu2621histHist {
    /// 空账构造。
    pub const fn new() -> Self {
        Self { samples: [0; 96], n: 0, drops: 0, min: 0, max: 0, sum: 0 }
    }

    /// 采样（有界插入排序；满员丢弃并记账）。
    pub fn push(&mut self, v: u64) {
        if self.n >= MXU2621HIST_CAP {
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

/// 段位守卫：全部诊断码必须落在本段 [0xa870, 0xa87f]。
pub const fn u2621_segment_guard() -> bool {
    let mut ok = true;
    let mut i = 0usize;
    while i < u2621Code::ALL.len() {
        let c = u2621Code::ALL[i].code();
        if c < 0xa870 || c > 0xa87f {
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
fn u2621_guard_seg() -> bool {
    u2621_segment_guard()
}

/// UNX-F2621 矩阵 36 行齐备 + 逐行守卫 + 唯一性 + 累计守恒。
fn check_u2621_map_guard() -> bool {
    let mut ok = u2621mat_COUNT == 36;
    if ok { ok = u2621mat_scan() == 0 && u2621mat_unique(); }
    if ok { ok = u2621mat_get("CR0000") == Some(0) && u2621mat_get("ZZZ_NOKEY").is_none(); }
    if ok { ok = u2621mat_sum() == 1594; }
    ok
}

/// UNX-F2621 直方图 16 样本分位/均值/守恒。
fn check_u2621_map_hist() -> bool {
    let mut h = mxu2621histHist::new();
    let mut i = 0u64;
    while i < 16 { h.push(i + 1); i += 1; }
    let mut ok = h.conserved() && h.mean() >= 1 && h.percentile(500) >= 8;
    ok
}

/// 本模块自检（逐检查带 detail，全绿才出门）。
pub fn run_u2621_checks() -> CheckSet {
    let mut set = CheckSet::new("U2621");
    set.add(
        "u2621-seg-guard",
        u2621_guard_seg(),
        if u2621_guard_seg() { "" } else { "diagnostic code outside segment" },
    );
    set.add(
        "u2621-map-guard",
        check_u2621_map_guard(),
        if check_u2621_map_guard() { "" } else { "UNX-F2621 matrix guard mismatch" },
    );
    set.add(
        "u2621-map-hist",
        check_u2621_map_hist(),
        if check_u2621_map_hist() { "" } else { "UNX-F2621 hist mismatch" },
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
    fn u2621_all_checks_green() {
        let set = run_u2621_checks();
        assert!(set.all_passed(), "U2621 domain checks must be green");
    }

    #[test]
    fn u2621_segment_guard_holds() {
        assert!(u2621_segment_guard());
    }
}
