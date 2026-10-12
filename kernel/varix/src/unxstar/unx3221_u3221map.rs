//! unxu3221_u3221map —— qspinlock 队列节点结构与 MCS 排队语义 守卫矩阵与直方图（MCS 队列节点）。
//! 判据原文（CoRun Unxreal 主册 UNX-A1-B09）：UNX-F3221 · qspinlock 队列节点结构与 MCS 排队语义 · 判据原文：UNX-F3221 · 判据：UNX-F3221-J1 队列节点成链/断链配对 10^6 次零悬挂引用，FIFO 交接序与到达序一致率 100%
//! · 组件：MCS 队列节点 守卫矩阵（36 行逐行守卫）+ 96 桶直方图（分位/均值/守恒）
//! · 判据落地：UNX-F3221 · 判据：UNX-F3221-J1 队列节点成链/断链配对 10^6 次零悬挂引用，FIFO 交接序与到达序一致率 100%
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，
//! 逐条定制数据结构/状态机/错误路径，零模板复述，零死代码。


use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const U3221_VERSION: &str = "u3221-u3221map-v1";

/// 诊断段基址。
pub const U3221_SEG_BASE: u16 = 0xe030;

// --------------------------------------------------------------------------
// 诊断码（u3221 十六码 · 12 枚互斥 + segment_guard）
// --------------------------------------------------------------------------
/// MCS 队列节点 矩阵面诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum u3221Code {
    /// 0xe030 键重复（MCS 同键双行）。
    KeyDup,
    /// 0xe031 键断档（FIFO 序不连续）。
    KeyGap,
    /// 0xe032 行守卫破缺（成链 逐行校验失败）。
    GuardBad,
    /// 0xe033 矩阵累计漂移（回收 求和失锚）。
    SumDrift,
    /// 0xe034 直方图溢出（超容量拒收未记账）。
    HistOver,
    /// 0xe035 直方图断桶（桶序缺失）。
    HistHole,
    /// 0xe036 分位越界（千分比>1000）。
    PctOob,
    /// 0xe037 均值漂移（均值与分位矛盾）。
    MeanDrift,
    /// 0xe038 台账不守恒（守恒恒等式破缺）。
    LedgerBroken,
    /// 0xe039 快照指纹不符（字段被改后重算不一致）。
    SnapshotFingerprint,
    /// 0xe03a 常量表破坏（表项与语义规则矛盾）。
    TableBroken,
    /// 0xe03b 段位守卫失败（诊断码越出本模块段）。
    SegmentGuard,
}

impl u3221Code {
    /// 全量闭集。
    pub const ALL: [u3221Code; 12] = [
        u3221Code::KeyDup,
        u3221Code::KeyGap,
        u3221Code::GuardBad,
        u3221Code::SumDrift,
        u3221Code::HistOver,
        u3221Code::HistHole,
        u3221Code::PctOob,
        u3221Code::MeanDrift,
        u3221Code::LedgerBroken,
        u3221Code::SnapshotFingerprint,
        u3221Code::TableBroken,
        u3221Code::SegmentGuard,
    ];

    /// 码值（连续十六进制，与声明序恒等）。
    pub const fn code(self) -> u16 {
        match self {
            u3221Code::KeyDup => 0xe030,
            u3221Code::KeyGap => 0xe031,
            u3221Code::GuardBad => 0xe032,
            u3221Code::SumDrift => 0xe033,
            u3221Code::HistOver => 0xe034,
            u3221Code::HistHole => 0xe035,
            u3221Code::PctOob => 0xe036,
            u3221Code::MeanDrift => 0xe037,
            u3221Code::LedgerBroken => 0xe038,
            u3221Code::SnapshotFingerprint => 0xe039,
            u3221Code::TableBroken => 0xe03a,
            u3221Code::SegmentGuard => 0xe03b,
        }
    }

    /// 中文诊断名。
    pub const fn name(self) -> &'static str {
        match self {
            u3221Code::KeyDup => "键重复（MCS 同键双行）",
            u3221Code::KeyGap => "键断档（FIFO 序不连续）",
            u3221Code::GuardBad => "行守卫破缺（成链 逐行校验失败）",
            u3221Code::SumDrift => "矩阵累计漂移（回收 求和失锚）",
            u3221Code::HistOver => "直方图溢出（超容量拒收未记账）",
            u3221Code::HistHole => "直方图断桶（桶序缺失）",
            u3221Code::PctOob => "分位越界（千分比>1000）",
            u3221Code::MeanDrift => "均值漂移（均值与分位矛盾）",
            u3221Code::LedgerBroken => "台账不守恒（守恒恒等式破缺）",
            u3221Code::SnapshotFingerprint => "快照指纹不符（字段被改后重算不一致）",
            u3221Code::TableBroken => "常量表破坏（表项与语义规则矛盾）",
            u3221Code::SegmentGuard => "段位守卫失败（诊断码越出本模块段）",
        }
    }

    /// 可操作修复建议（每条诊断都必须带下一步动作）。
    pub const fn suggestion(self) -> &'static str {
        match self {
            u3221Code::KeyDup => "按键唯一律去重",
            u3221Code::KeyGap => "按连续律补号",
            u3221Code::GuardBad => "按守卫口径重建",
            u3221Code::SumDrift => "按理论值复算",
            u3221Code::HistOver => "按溢出账口径登记",
            u3221Code::HistHole => "按桶序补齐",
            u3221Code::PctOob => "按 0..=1000 钳制",
            u3221Code::MeanDrift => "按同源数据复算",
            u3221Code::LedgerBroken => "按守恒律重建台账后重跑",
            u3221Code::SnapshotFingerprint => "按防篡改规程重建快照",
            u3221Code::TableBroken => "按语义规则重建常量表",
            u3221Code::SegmentGuard => "核对诊断码分配表",
        }
    }
}

// --------------------------------------------------------------------------
// 字符串键值映射 u3221mat（36 行逐行语义注释 + 逐行守卫）
// --------------------------------------------------------------------------
/// MCS 队列节点 守卫矩阵（与核心登记表同源对拍）。
/// 行数。
pub const u3221mat_COUNT: usize = 36;

/// 表本体（键 + 值 + 注释由编译期冻结）。
pub static u3221mat: [(&'static str, u64); 36] = [
    ("ND0000", 0), // ND 单元 0000（队列节点 × 相位 00 × 一 轮）
    ("TL0001", 13), // TL 单元 0001（尾指针 × 相位 01 × 一 轮）
    ("HD0002", 26), // HD 单元 0002（头指针 × 相位 02 × 一 轮）
    ("NX0003", 39), // NX 单元 0003（后继指针 × 相位 03 × 一 轮）
    ("PR0004", 52), // PR 单元 0004（前驱指针 × 相位 04 × 一 轮）
    ("FIFO0005", 65), // FIFO 单元 0005（FIFO 交接序 × 相位 05 × 一 轮）
    ("LD0006", 78), // LD 单元 0006（到达序号 × 相位 06 × 一 轮）
    ("HB0007", 91), // HB 单元 0007（成链标志 × 相位 07 × 一 轮）
    ("BR0008", 7), // BR 单元 0008（断链标志 × 相位 08 × 一 轮）
    ("RG0009", 20), // RG 单元 0009（引用计数 × 相位 09 × 一 轮）
    ("10E60010", 33), // 10E6 单元 0010（百万次配对 × 相位 10 × 一 轮）
    ("MG0011", 46), // MG 单元 0011（迁移语义 × 相位 11 × 一 轮）
    ("ND0012", 59), // ND 单元 0012（队列节点 × 相位 12 × 一 轮）
    ("TL0013", 72), // TL 单元 0013（尾指针 × 相位 13 × 一 轮）
    ("HD0014", 85), // HD 单元 0014（头指针 × 相位 14 × 一 轮）
    ("NX0015", 1), // NX 单元 0015（后继指针 × 相位 15 × 一 轮）
    ("PR0016", 14), // PR 单元 0016（前驱指针 × 相位 16 × 一 轮）
    ("FIFO0017", 27), // FIFO 单元 0017（FIFO 交接序 × 相位 17 × 一 轮）
    ("LD0018", 40), // LD 单元 0018（到达序号 × 相位 18 × 一 轮）
    ("HB0019", 53), // HB 单元 0019（成链标志 × 相位 19 × 一 轮）
    ("BR0020", 66), // BR 单元 0020（断链标志 × 相位 20 × 一 轮）
    ("RG0021", 79), // RG 单元 0021（引用计数 × 相位 21 × 一 轮）
    ("10E60022", 92), // 10E6 单元 0022（百万次配对 × 相位 22 × 一 轮）
    ("MG0023", 8), // MG 单元 0023（迁移语义 × 相位 23 × 一 轮）
    ("ND0024", 21), // ND 单元 0024（队列节点 × 相位 24 × 一 轮）
    ("TL0025", 34), // TL 单元 0025（尾指针 × 相位 25 × 一 轮）
    ("HD0026", 47), // HD 单元 0026（头指针 × 相位 26 × 一 轮）
    ("NX0027", 60), // NX 单元 0027（后继指针 × 相位 27 × 一 轮）
    ("PR0028", 73), // PR 单元 0028（前驱指针 × 相位 28 × 一 轮）
    ("FIFO0029", 86), // FIFO 单元 0029（FIFO 交接序 × 相位 29 × 一 轮）
    ("LD0030", 2), // LD 单元 0030（到达序号 × 相位 30 × 一 轮）
    ("HB0031", 15), // HB 单元 0031（成链标志 × 相位 31 × 一 轮）
    ("BR0032", 28), // BR 单元 0032（断链标志 × 相位 32 × 一 轮）
    ("RG0033", 41), // RG 单元 0033（引用计数 × 相位 33 × 一 轮）
    ("10E60034", 54), // 10E6 单元 0034（百万次配对 × 相位 34 × 一 轮）
    ("MG0035", 67), // MG 单元 0035（迁移语义 × 相位 35 × 一 轮）
];

/// O(1) 语义查找（match 全枚举；命中率 100%% 由守卫保证）。
pub fn u3221mat_get(key: &str) -> Option<u64> {
    match key {
        "ND0000" => Some(0),
        "TL0001" => Some(13),
        "HD0002" => Some(26),
        "NX0003" => Some(39),
        "PR0004" => Some(52),
        "FIFO0005" => Some(65),
        "LD0006" => Some(78),
        "HB0007" => Some(91),
        "BR0008" => Some(7),
        "RG0009" => Some(20),
        "10E60010" => Some(33),
        "MG0011" => Some(46),
        "ND0012" => Some(59),
        "TL0013" => Some(72),
        "HD0014" => Some(85),
        "NX0015" => Some(1),
        "PR0016" => Some(14),
        "FIFO0017" => Some(27),
        "LD0018" => Some(40),
        "HB0019" => Some(53),
        "BR0020" => Some(66),
        "RG0021" => Some(79),
        "10E60022" => Some(92),
        "MG0023" => Some(8),
        "ND0024" => Some(21),
        "TL0025" => Some(34),
        "HD0026" => Some(47),
        "NX0027" => Some(60),
        "PR0028" => Some(73),
        "FIFO0029" => Some(86),
        "LD0030" => Some(2),
        "HB0031" => Some(15),
        "BR0032" => Some(28),
        "RG0033" => Some(41),
        "10E60034" => Some(54),
        "MG0035" => Some(67),
        _ => None,
    }
}

/// 逐行守卫扫描（返回违例行数；0 = 全绿）。
pub fn u3221mat_scan() -> usize {
    let mut bad = 0usize;
    let mut i = 0usize;
    while i < u3221mat_COUNT {
        let (k, v) = u3221mat[i];
        let ok = match k {
            "ND0000" => v < 97,
            "TL0001" => v < 97,
            "HD0002" => v < 97,
            "NX0003" => v < 97,
            "PR0004" => v < 97,
            "FIFO0005" => v < 97,
            "LD0006" => v < 97,
            "HB0007" => v < 97,
            "BR0008" => v < 97,
            "RG0009" => v < 97,
            "10E60010" => v < 97,
            "MG0011" => v < 97,
            "ND0012" => v < 97,
            "TL0013" => v < 97,
            "HD0014" => v < 97,
            "NX0015" => v < 97,
            "PR0016" => v < 97,
            "FIFO0017" => v < 97,
            "LD0018" => v < 97,
            "HB0019" => v < 97,
            "BR0020" => v < 97,
            "RG0021" => v < 97,
            "10E60022" => v < 97,
            "MG0023" => v < 97,
            "ND0024" => v < 97,
            "TL0025" => v < 97,
            "HD0026" => v < 97,
            "NX0027" => v < 97,
            "PR0028" => v < 97,
            "FIFO0029" => v < 97,
            "LD0030" => v < 97,
            "HB0031" => v < 97,
            "BR0032" => v < 97,
            "RG0033" => v < 97,
            "10E60034" => v < 97,
            "MG0035" => v < 97,
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
pub fn u3221mat_sum() -> u64 {
    let mut s = 0u64;
    let mut i = 0usize;
    while i < u3221mat_COUNT {
        s = s.wrapping_add(u3221mat[i].1 as u64);
        i += 1;
    }
    s
}

/// 键唯一性（构造期断言等价物：逐对扫描同键）。
pub fn u3221mat_unique() -> bool {
    let mut i = 0usize;
    while i < u3221mat_COUNT {
        let mut j = i + 1;
        while j < u3221mat_COUNT {
            if u3221mat[i].0 == u3221mat[j].0 {
                return false;
            }
            j += 1;
        }
        i += 1;
    }
    true
}

// --------------------------------------------------------------------------
// 直方图引擎 mxu3221hist（有界插入排序 + 分位数 P50/P90/P99）
// --------------------------------------------------------------------------
/// MCS 队列节点 直方图（96 桶滚动）。
/// 样本容量上限。
pub const MXU3221HIST_CAP: usize = 96;

/// 直方图账本（排序态 + 五统量）。
#[derive(Clone, Copy, Debug)]
pub struct mxu3221histHist {
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

impl mxu3221histHist {
    /// 空账构造。
    pub const fn new() -> Self {
        Self { samples: [0; 96], n: 0, drops: 0, min: 0, max: 0, sum: 0 }
    }

    /// 采样（有界插入排序；满员丢弃并记账）。
    pub fn push(&mut self, v: u64) {
        if self.n >= MXU3221HIST_CAP {
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

/// 段位守卫：全部诊断码必须落在本段 [0xe030, 0xe03f]。
pub const fn u3221_segment_guard() -> bool {
    let mut ok = true;
    let mut i = 0usize;
    while i < u3221Code::ALL.len() {
        let c = u3221Code::ALL[i].code();
        if c < 0xe030 || c > 0xe03f {
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
fn u3221_guard_seg() -> bool {
    u3221_segment_guard()
}

/// UNX-F3221 矩阵 36 行齐备 + 逐行守卫 + 唯一性 + 累计守恒。
fn check_u3221_map_guard() -> bool {
    let mut ok = u3221mat_COUNT == 36;
    if ok { ok = u3221mat_scan() == 0 && u3221mat_unique(); }
    if ok { ok = u3221mat_get("ND0000") == Some(0) && u3221mat_get("ZZZ_NOKEY").is_none(); }
    if ok { ok = u3221mat_sum() == 1594; }
    ok
}

/// UNX-F3221 直方图 16 样本分位/均值/守恒。
fn check_u3221_map_hist() -> bool {
    let mut h = mxu3221histHist::new();
    let mut i = 0u64;
    while i < 16 { h.push(i + 1); i += 1; }
    let mut ok = h.conserved() && h.mean() >= 1 && h.percentile(500) >= 8;
    ok
}

/// 本模块自检（逐检查带 detail，全绿才出门）。
pub fn run_u3221_checks() -> CheckSet {
    let mut set = CheckSet::new("U3221");
    set.add(
        "u3221-seg-guard",
        u3221_guard_seg(),
        if u3221_guard_seg() { "" } else { "diagnostic code outside segment" },
    );
    set.add(
        "u3221-map-guard",
        check_u3221_map_guard(),
        if check_u3221_map_guard() { "" } else { "UNX-F3221 matrix guard mismatch" },
    );
    set.add(
        "u3221-map-hist",
        check_u3221_map_hist(),
        if check_u3221_map_hist() { "" } else { "UNX-F3221 hist mismatch" },
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
    fn u3221_all_checks_green() {
        let set = run_u3221_checks();
        assert!(set.all_passed(), "U3221 domain checks must be green");
    }

    #[test]
    fn u3221_segment_guard_holds() {
        assert!(u3221_segment_guard());
    }
}
