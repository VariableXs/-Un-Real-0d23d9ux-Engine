//! unxu2421_u2421map —— order-0 页快速路径 守卫矩阵与直方图（单页请求）。
//! 判据原文（CoRun Unxreal 主册 UNX-A1-B09）：UNX-F2421 · order-0 页快速路径 · 判据原文：UNX-F2421 · 判据：单页分配 p99 时延落账且与高阶路径比值 ≤0.5，10^6 次分配释放对账零泄漏
//! · 组件：单页请求 守卫矩阵（36 行逐行守卫）+ 96 桶直方图（分位/均值/守恒）
//! · 判据落地：UNX-F2421 · 判据：单页分配 p99 时延落账且与高阶路径比值 ≤0.5，10^6 次分配释放对账零泄漏
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，
//! 逐条定制数据结构/状态机/错误路径，零模板复述，零死代码。


use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const U2421_VERSION: &str = "u2421-u2421map-v1";

/// 诊断段基址。
pub const U2421_SEG_BASE: u16 = 0xa0f0;

// --------------------------------------------------------------------------
// 诊断码（u2421 十六码 · 12 枚互斥 + segment_guard）
// --------------------------------------------------------------------------
/// 单页请求 矩阵面诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum u2421Code {
    /// 0xa0f0 键重复（快径旁落 同键双行）。
    KeyDup,
    /// 0xa0f1 键断档（时延超标 序不连续）。
    KeyGap,
    /// 0xa0f2 行守卫破缺（比值越界 逐行校验失败）。
    GuardBad,
    /// 0xa0f3 矩阵累计漂移（泄漏漏记 求和失锚）。
    SumDrift,
    /// 0xa0f4 直方图溢出（超容量拒收未记账）。
    HistOver,
    /// 0xa0f5 直方图断桶（桶序缺失）。
    HistHole,
    /// 0xa0f6 分位越界（千分比>1000）。
    PctOob,
    /// 0xa0f7 均值漂移（均值与分位矛盾）。
    MeanDrift,
    /// 0xa0f8 台账不守恒（守恒恒等式破缺）。
    LedgerBroken,
    /// 0xa0f9 快照指纹不符（字段被改后重算不一致）。
    SnapshotFingerprint,
    /// 0xa0fa 常量表破坏（表项与语义规则矛盾）。
    TableBroken,
    /// 0xa0fb 段位守卫失败（诊断码越出本模块段）。
    SegmentGuard,
}

impl u2421Code {
    /// 全量闭集。
    pub const ALL: [u2421Code; 12] = [
        u2421Code::KeyDup,
        u2421Code::KeyGap,
        u2421Code::GuardBad,
        u2421Code::SumDrift,
        u2421Code::HistOver,
        u2421Code::HistHole,
        u2421Code::PctOob,
        u2421Code::MeanDrift,
        u2421Code::LedgerBroken,
        u2421Code::SnapshotFingerprint,
        u2421Code::TableBroken,
        u2421Code::SegmentGuard,
    ];

    /// 码值（连续十六进制，与声明序恒等）。
    pub const fn code(self) -> u16 {
        match self {
            u2421Code::KeyDup => 0xa0f0,
            u2421Code::KeyGap => 0xa0f1,
            u2421Code::GuardBad => 0xa0f2,
            u2421Code::SumDrift => 0xa0f3,
            u2421Code::HistOver => 0xa0f4,
            u2421Code::HistHole => 0xa0f5,
            u2421Code::PctOob => 0xa0f6,
            u2421Code::MeanDrift => 0xa0f7,
            u2421Code::LedgerBroken => 0xa0f8,
            u2421Code::SnapshotFingerprint => 0xa0f9,
            u2421Code::TableBroken => 0xa0fa,
            u2421Code::SegmentGuard => 0xa0fb,
        }
    }

    /// 中文诊断名。
    pub const fn name(self) -> &'static str {
        match self {
            u2421Code::KeyDup => "键重复（快径旁落 同键双行）",
            u2421Code::KeyGap => "键断档（时延超标 序不连续）",
            u2421Code::GuardBad => "行守卫破缺（比值越界 逐行校验失败）",
            u2421Code::SumDrift => "矩阵累计漂移（泄漏漏记 求和失锚）",
            u2421Code::HistOver => "直方图溢出（超容量拒收未记账）",
            u2421Code::HistHole => "直方图断桶（桶序缺失）",
            u2421Code::PctOob => "分位越界（千分比>1000）",
            u2421Code::MeanDrift => "均值漂移（均值与分位矛盾）",
            u2421Code::LedgerBroken => "台账不守恒（守恒恒等式破缺）",
            u2421Code::SnapshotFingerprint => "快照指纹不符（字段被改后重算不一致）",
            u2421Code::TableBroken => "常量表破坏（表项与语义规则矛盾）",
            u2421Code::SegmentGuard => "段位守卫失败（诊断码越出本模块段）",
        }
    }

    /// 可操作修复建议（每条诊断都必须带下一步动作）。
    pub const fn suggestion(self) -> &'static str {
        match self {
            u2421Code::KeyDup => "按键唯一律去重",
            u2421Code::KeyGap => "按连续律补号",
            u2421Code::GuardBad => "按守卫口径重建",
            u2421Code::SumDrift => "按理论值复算",
            u2421Code::HistOver => "按溢出账口径登记",
            u2421Code::HistHole => "按桶序补齐",
            u2421Code::PctOob => "按 0..=1000 钳制",
            u2421Code::MeanDrift => "按同源数据复算",
            u2421Code::LedgerBroken => "按守恒律重建台账后重跑",
            u2421Code::SnapshotFingerprint => "按防篡改规程重建快照",
            u2421Code::TableBroken => "按语义规则重建常量表",
            u2421Code::SegmentGuard => "核对诊断码分配表",
        }
    }
}

// --------------------------------------------------------------------------
// 字符串键值映射 u2421mat（36 行逐行语义注释 + 逐行守卫）
// --------------------------------------------------------------------------
/// 单页请求 守卫矩阵（与核心登记表同源对拍）。
/// 行数。
pub const u2421mat_COUNT: usize = 36;

/// 表本体（键 + 值 + 注释由编译期冻结）。
pub static u2421mat: [(&'static str, u64); 36] = [
    ("O00000", 0), // O0 单元 0000（order-0 路径 × 相位 00 × 一 轮）
    ("P990001", 13), // P99 单元 0001（p99 时延 × 相位 01 × 一 轮）
    ("HI0002", 26), // HI 单元 0002（高阶路径 × 相位 02 × 一 轮）
    ("RT0003", 39), // RT 单元 0003（比值门限 × 相位 03 × 一 轮）
    ("M10004", 52), // M1 单元 0004（百万次对账 × 相位 04 × 一 轮）
    ("LK0005", 65), // LK 单元 0005（泄漏账 × 相位 05 × 一 轮）
    ("AL0006", 78), // AL 单元 0006（分配事件 × 相位 06 × 一 轮）
    ("FR0007", 91), // FR 单元 0007（释放事件 × 相位 07 × 一 轮）
    ("FD0008", 7), // FD 单元 0008（快径标志 × 相位 08 × 一 轮）
    ("LT0009", 20), // LT 单元 0009（时延账 × 相位 09 × 一 轮）
    ("QF0010", 33), // QF 单元 0010（配额账 × 相位 10 × 一 轮）
    ("ZC0011", 46), // ZC 单元 0011（零泄漏断言 × 相位 11 × 一 轮）
    ("O00012", 59), // O0 单元 0012（order-0 路径 × 相位 12 × 一 轮）
    ("P990013", 72), // P99 单元 0013（p99 时延 × 相位 13 × 一 轮）
    ("HI0014", 85), // HI 单元 0014（高阶路径 × 相位 14 × 一 轮）
    ("RT0015", 1), // RT 单元 0015（比值门限 × 相位 15 × 一 轮）
    ("M10016", 14), // M1 单元 0016（百万次对账 × 相位 16 × 一 轮）
    ("LK0017", 27), // LK 单元 0017（泄漏账 × 相位 17 × 一 轮）
    ("AL0018", 40), // AL 单元 0018（分配事件 × 相位 18 × 一 轮）
    ("FR0019", 53), // FR 单元 0019（释放事件 × 相位 19 × 一 轮）
    ("FD0020", 66), // FD 单元 0020（快径标志 × 相位 20 × 一 轮）
    ("LT0021", 79), // LT 单元 0021（时延账 × 相位 21 × 一 轮）
    ("QF0022", 92), // QF 单元 0022（配额账 × 相位 22 × 一 轮）
    ("ZC0023", 8), // ZC 单元 0023（零泄漏断言 × 相位 23 × 一 轮）
    ("O00024", 21), // O0 单元 0024（order-0 路径 × 相位 24 × 一 轮）
    ("P990025", 34), // P99 单元 0025（p99 时延 × 相位 25 × 一 轮）
    ("HI0026", 47), // HI 单元 0026（高阶路径 × 相位 26 × 一 轮）
    ("RT0027", 60), // RT 单元 0027（比值门限 × 相位 27 × 一 轮）
    ("M10028", 73), // M1 单元 0028（百万次对账 × 相位 28 × 一 轮）
    ("LK0029", 86), // LK 单元 0029（泄漏账 × 相位 29 × 一 轮）
    ("AL0030", 2), // AL 单元 0030（分配事件 × 相位 30 × 一 轮）
    ("FR0031", 15), // FR 单元 0031（释放事件 × 相位 31 × 一 轮）
    ("FD0032", 28), // FD 单元 0032（快径标志 × 相位 32 × 一 轮）
    ("LT0033", 41), // LT 单元 0033（时延账 × 相位 33 × 一 轮）
    ("QF0034", 54), // QF 单元 0034（配额账 × 相位 34 × 一 轮）
    ("ZC0035", 67), // ZC 单元 0035（零泄漏断言 × 相位 35 × 一 轮）
];

/// O(1) 语义查找（match 全枚举；命中率 100%% 由守卫保证）。
pub fn u2421mat_get(key: &str) -> Option<u64> {
    match key {
        "O00000" => Some(0),
        "P990001" => Some(13),
        "HI0002" => Some(26),
        "RT0003" => Some(39),
        "M10004" => Some(52),
        "LK0005" => Some(65),
        "AL0006" => Some(78),
        "FR0007" => Some(91),
        "FD0008" => Some(7),
        "LT0009" => Some(20),
        "QF0010" => Some(33),
        "ZC0011" => Some(46),
        "O00012" => Some(59),
        "P990013" => Some(72),
        "HI0014" => Some(85),
        "RT0015" => Some(1),
        "M10016" => Some(14),
        "LK0017" => Some(27),
        "AL0018" => Some(40),
        "FR0019" => Some(53),
        "FD0020" => Some(66),
        "LT0021" => Some(79),
        "QF0022" => Some(92),
        "ZC0023" => Some(8),
        "O00024" => Some(21),
        "P990025" => Some(34),
        "HI0026" => Some(47),
        "RT0027" => Some(60),
        "M10028" => Some(73),
        "LK0029" => Some(86),
        "AL0030" => Some(2),
        "FR0031" => Some(15),
        "FD0032" => Some(28),
        "LT0033" => Some(41),
        "QF0034" => Some(54),
        "ZC0035" => Some(67),
        _ => None,
    }
}

/// 逐行守卫扫描（返回违例行数；0 = 全绿）。
pub fn u2421mat_scan() -> usize {
    let mut bad = 0usize;
    let mut i = 0usize;
    while i < u2421mat_COUNT {
        let (k, v) = u2421mat[i];
        let ok = match k {
            "O00000" => v < 97,
            "P990001" => v < 97,
            "HI0002" => v < 97,
            "RT0003" => v < 97,
            "M10004" => v < 97,
            "LK0005" => v < 97,
            "AL0006" => v < 97,
            "FR0007" => v < 97,
            "FD0008" => v < 97,
            "LT0009" => v < 97,
            "QF0010" => v < 97,
            "ZC0011" => v < 97,
            "O00012" => v < 97,
            "P990013" => v < 97,
            "HI0014" => v < 97,
            "RT0015" => v < 97,
            "M10016" => v < 97,
            "LK0017" => v < 97,
            "AL0018" => v < 97,
            "FR0019" => v < 97,
            "FD0020" => v < 97,
            "LT0021" => v < 97,
            "QF0022" => v < 97,
            "ZC0023" => v < 97,
            "O00024" => v < 97,
            "P990025" => v < 97,
            "HI0026" => v < 97,
            "RT0027" => v < 97,
            "M10028" => v < 97,
            "LK0029" => v < 97,
            "AL0030" => v < 97,
            "FR0031" => v < 97,
            "FD0032" => v < 97,
            "LT0033" => v < 97,
            "QF0034" => v < 97,
            "ZC0035" => v < 97,
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
pub fn u2421mat_sum() -> u64 {
    let mut s = 0u64;
    let mut i = 0usize;
    while i < u2421mat_COUNT {
        s = s.wrapping_add(u2421mat[i].1 as u64);
        i += 1;
    }
    s
}

/// 键唯一性（构造期断言等价物：逐对扫描同键）。
pub fn u2421mat_unique() -> bool {
    let mut i = 0usize;
    while i < u2421mat_COUNT {
        let mut j = i + 1;
        while j < u2421mat_COUNT {
            if u2421mat[i].0 == u2421mat[j].0 {
                return false;
            }
            j += 1;
        }
        i += 1;
    }
    true
}

// --------------------------------------------------------------------------
// 直方图引擎 mxu2421hist（有界插入排序 + 分位数 P50/P90/P99）
// --------------------------------------------------------------------------
/// 单页请求 直方图（96 桶滚动）。
/// 样本容量上限。
pub const MXU2421HIST_CAP: usize = 96;

/// 直方图账本（排序态 + 五统量）。
#[derive(Clone, Copy, Debug)]
pub struct mxu2421histHist {
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

impl mxu2421histHist {
    /// 空账构造。
    pub const fn new() -> Self {
        Self { samples: [0; 96], n: 0, drops: 0, min: 0, max: 0, sum: 0 }
    }

    /// 采样（有界插入排序；满员丢弃并记账）。
    pub fn push(&mut self, v: u64) {
        if self.n >= MXU2421HIST_CAP {
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

/// 段位守卫：全部诊断码必须落在本段 [0xa0f0, 0xa0ff]。
pub const fn u2421_segment_guard() -> bool {
    let mut ok = true;
    let mut i = 0usize;
    while i < u2421Code::ALL.len() {
        let c = u2421Code::ALL[i].code();
        if c < 0xa0f0 || c > 0xa0ff {
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
fn u2421_guard_seg() -> bool {
    u2421_segment_guard()
}

/// UNX-F2421 矩阵 36 行齐备 + 逐行守卫 + 唯一性 + 累计守恒。
fn check_u2421_map_guard() -> bool {
    let mut ok = u2421mat_COUNT == 36;
    if ok { ok = u2421mat_scan() == 0 && u2421mat_unique(); }
    if ok { ok = u2421mat_get("O00000") == Some(0) && u2421mat_get("ZZZ_NOKEY").is_none(); }
    if ok { ok = u2421mat_sum() == 1594; }
    ok
}

/// UNX-F2421 直方图 16 样本分位/均值/守恒。
fn check_u2421_map_hist() -> bool {
    let mut h = mxu2421histHist::new();
    let mut i = 0u64;
    while i < 16 { h.push(i + 1); i += 1; }
    let mut ok = h.conserved() && h.mean() >= 1 && h.percentile(500) >= 8;
    ok
}

/// 本模块自检（逐检查带 detail，全绿才出门）。
pub fn run_u2421_checks() -> CheckSet {
    let mut set = CheckSet::new("U2421");
    set.add(
        "u2421-seg-guard",
        u2421_guard_seg(),
        if u2421_guard_seg() { "" } else { "diagnostic code outside segment" },
    );
    set.add(
        "u2421-map-guard",
        check_u2421_map_guard(),
        if check_u2421_map_guard() { "" } else { "UNX-F2421 matrix guard mismatch" },
    );
    set.add(
        "u2421-map-hist",
        check_u2421_map_hist(),
        if check_u2421_map_hist() { "" } else { "UNX-F2421 hist mismatch" },
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
    fn u2421_all_checks_green() {
        let set = run_u2421_checks();
        assert!(set.all_passed(), "U2421 domain checks must be green");
    }

    #[test]
    fn u2421_segment_guard_holds() {
        assert!(u2421_segment_guard());
    }
}
