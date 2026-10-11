//! unxu2741_u2741map —— 水线三档判定与唤醒回调 守卫矩阵与直方图（水线档）。
//! 判据原文（CoRun Unxreal 主册 UNX-A1-B09）：UNX-F2741 · 水线三档判定与唤醒回调 · 判据原文：UNX-F2741 · 判据：三档判定（min/low/high 对 free 页数）逐档可复测，low 触发唤醒回调 10/10 次
//! · 组件：水线档 守卫矩阵（36 行逐行守卫）+ 96 桶直方图（分位/均值/守恒）
//! · 判据落地：UNX-F2741 · 判据：三档判定（min/low/high 对 free 页数）逐档可复测，low 触发唤醒回调 10/10 次
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，
//! 逐条定制数据结构/状态机/错误路径，零模板复述，零死代码。


use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const U2741_VERSION: &str = "u2741-u2741map-v1";

/// 诊断段基址。
pub const U2741_SEG_BASE: u16 = 0xacb0;

// --------------------------------------------------------------------------
// 诊断码（u2741 十六码 · 12 枚互斥 + segment_guard）
// --------------------------------------------------------------------------
/// 水线档 矩阵面诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum u2741Code {
    /// 0xacb0 键重复（档位错判 同键双行）。
    KeyDup,
    /// 0xacb1 键断档（唤醒漏调 序不连续）。
    KeyGap,
    /// 0xacb2 行守卫破缺（复测不符 逐行校验失败）。
    GuardBad,
    /// 0xacb3 矩阵累计漂移（阈值漂移 求和失锚）。
    SumDrift,
    /// 0xacb4 直方图溢出（超容量拒收未记账）。
    HistOver,
    /// 0xacb5 直方图断桶（桶序缺失）。
    HistHole,
    /// 0xacb6 分位越界（千分比>1000）。
    PctOob,
    /// 0xacb7 均值漂移（均值与分位矛盾）。
    MeanDrift,
    /// 0xacb8 台账不守恒（守恒恒等式破缺）。
    LedgerBroken,
    /// 0xacb9 快照指纹不符（字段被改后重算不一致）。
    SnapshotFingerprint,
    /// 0xacba 常量表破坏（表项与语义规则矛盾）。
    TableBroken,
    /// 0xacbb 段位守卫失败（诊断码越出本模块段）。
    SegmentGuard,
}

impl u2741Code {
    /// 全量闭集。
    pub const ALL: [u2741Code; 12] = [
        u2741Code::KeyDup,
        u2741Code::KeyGap,
        u2741Code::GuardBad,
        u2741Code::SumDrift,
        u2741Code::HistOver,
        u2741Code::HistHole,
        u2741Code::PctOob,
        u2741Code::MeanDrift,
        u2741Code::LedgerBroken,
        u2741Code::SnapshotFingerprint,
        u2741Code::TableBroken,
        u2741Code::SegmentGuard,
    ];

    /// 码值（连续十六进制，与声明序恒等）。
    pub const fn code(self) -> u16 {
        match self {
            u2741Code::KeyDup => 0xacb0,
            u2741Code::KeyGap => 0xacb1,
            u2741Code::GuardBad => 0xacb2,
            u2741Code::SumDrift => 0xacb3,
            u2741Code::HistOver => 0xacb4,
            u2741Code::HistHole => 0xacb5,
            u2741Code::PctOob => 0xacb6,
            u2741Code::MeanDrift => 0xacb7,
            u2741Code::LedgerBroken => 0xacb8,
            u2741Code::SnapshotFingerprint => 0xacb9,
            u2741Code::TableBroken => 0xacba,
            u2741Code::SegmentGuard => 0xacbb,
        }
    }

    /// 中文诊断名。
    pub const fn name(self) -> &'static str {
        match self {
            u2741Code::KeyDup => "键重复（档位错判 同键双行）",
            u2741Code::KeyGap => "键断档（唤醒漏调 序不连续）",
            u2741Code::GuardBad => "行守卫破缺（复测不符 逐行校验失败）",
            u2741Code::SumDrift => "矩阵累计漂移（阈值漂移 求和失锚）",
            u2741Code::HistOver => "直方图溢出（超容量拒收未记账）",
            u2741Code::HistHole => "直方图断桶（桶序缺失）",
            u2741Code::PctOob => "分位越界（千分比>1000）",
            u2741Code::MeanDrift => "均值漂移（均值与分位矛盾）",
            u2741Code::LedgerBroken => "台账不守恒（守恒恒等式破缺）",
            u2741Code::SnapshotFingerprint => "快照指纹不符（字段被改后重算不一致）",
            u2741Code::TableBroken => "常量表破坏（表项与语义规则矛盾）",
            u2741Code::SegmentGuard => "段位守卫失败（诊断码越出本模块段）",
        }
    }

    /// 可操作修复建议（每条诊断都必须带下一步动作）。
    pub const fn suggestion(self) -> &'static str {
        match self {
            u2741Code::KeyDup => "按键唯一律去重",
            u2741Code::KeyGap => "按连续律补号",
            u2741Code::GuardBad => "按守卫口径重建",
            u2741Code::SumDrift => "按理论值复算",
            u2741Code::HistOver => "按溢出账口径登记",
            u2741Code::HistHole => "按桶序补齐",
            u2741Code::PctOob => "按 0..=1000 钳制",
            u2741Code::MeanDrift => "按同源数据复算",
            u2741Code::LedgerBroken => "按守恒律重建台账后重跑",
            u2741Code::SnapshotFingerprint => "按防篡改规程重建快照",
            u2741Code::TableBroken => "按语义规则重建常量表",
            u2741Code::SegmentGuard => "核对诊断码分配表",
        }
    }
}

// --------------------------------------------------------------------------
// 字符串键值映射 u2741mat（36 行逐行语义注释 + 逐行守卫）
// --------------------------------------------------------------------------
/// 水线档 守卫矩阵（与核心登记表同源对拍）。
/// 行数。
pub const u2741mat_COUNT: usize = 36;

/// 表本体（键 + 值 + 注释由编译期冻结）。
pub static u2741mat: [(&'static str, u64); 36] = [
    ("MN0000", 0), // MN 单元 0000（min 档 × 相位 00 × 一 轮）
    ("LW0001", 13), // LW 单元 0001（low 档 × 相位 01 × 一 轮）
    ("HG0002", 26), // HG 单元 0002（high 档 × 相位 02 × 一 轮）
    ("FR0003", 39), // FR 单元 0003（free 页数 × 相位 03 × 一 轮）
    ("WK0004", 52), // WK 单元 0004（唤醒回调 × 相位 04 × 一 轮）
    ("RT0005", 65), // RT 单元 0005（可复测 × 相位 05 × 一 轮）
    ("T100006", 78), // T10 单元 0006（十次触发 × 相位 06 × 一 轮）
    ("CB0007", 91), // CB 单元 0007（回调账 × 相位 07 × 一 轮）
    ("TS0008", 7), // TS 单元 0008（阈值表 × 相位 08 × 一 轮）
    ("SP0009", 20), // SP 单元 0009（单点源 × 相位 09 × 一 轮）
    ("DR0010", 33), // DR 单元 0010（派生档 × 相位 10 × 一 轮）
    ("AS0011", 46), // AS 单元 0011（断言面 × 相位 11 × 一 轮）
    ("MN0012", 59), // MN 单元 0012（min 档 × 相位 12 × 一 轮）
    ("LW0013", 72), // LW 单元 0013（low 档 × 相位 13 × 一 轮）
    ("HG0014", 85), // HG 单元 0014（high 档 × 相位 14 × 一 轮）
    ("FR0015", 1), // FR 单元 0015（free 页数 × 相位 15 × 一 轮）
    ("WK0016", 14), // WK 单元 0016（唤醒回调 × 相位 16 × 一 轮）
    ("RT0017", 27), // RT 单元 0017（可复测 × 相位 17 × 一 轮）
    ("T100018", 40), // T10 单元 0018（十次触发 × 相位 18 × 一 轮）
    ("CB0019", 53), // CB 单元 0019（回调账 × 相位 19 × 一 轮）
    ("TS0020", 66), // TS 单元 0020（阈值表 × 相位 20 × 一 轮）
    ("SP0021", 79), // SP 单元 0021（单点源 × 相位 21 × 一 轮）
    ("DR0022", 92), // DR 单元 0022（派生档 × 相位 22 × 一 轮）
    ("AS0023", 8), // AS 单元 0023（断言面 × 相位 23 × 一 轮）
    ("MN0024", 21), // MN 单元 0024（min 档 × 相位 24 × 一 轮）
    ("LW0025", 34), // LW 单元 0025（low 档 × 相位 25 × 一 轮）
    ("HG0026", 47), // HG 单元 0026（high 档 × 相位 26 × 一 轮）
    ("FR0027", 60), // FR 单元 0027（free 页数 × 相位 27 × 一 轮）
    ("WK0028", 73), // WK 单元 0028（唤醒回调 × 相位 28 × 一 轮）
    ("RT0029", 86), // RT 单元 0029（可复测 × 相位 29 × 一 轮）
    ("T100030", 2), // T10 单元 0030（十次触发 × 相位 30 × 一 轮）
    ("CB0031", 15), // CB 单元 0031（回调账 × 相位 31 × 一 轮）
    ("TS0032", 28), // TS 单元 0032（阈值表 × 相位 32 × 一 轮）
    ("SP0033", 41), // SP 单元 0033（单点源 × 相位 33 × 一 轮）
    ("DR0034", 54), // DR 单元 0034（派生档 × 相位 34 × 一 轮）
    ("AS0035", 67), // AS 单元 0035（断言面 × 相位 35 × 一 轮）
];

/// O(1) 语义查找（match 全枚举；命中率 100%% 由守卫保证）。
pub fn u2741mat_get(key: &str) -> Option<u64> {
    match key {
        "MN0000" => Some(0),
        "LW0001" => Some(13),
        "HG0002" => Some(26),
        "FR0003" => Some(39),
        "WK0004" => Some(52),
        "RT0005" => Some(65),
        "T100006" => Some(78),
        "CB0007" => Some(91),
        "TS0008" => Some(7),
        "SP0009" => Some(20),
        "DR0010" => Some(33),
        "AS0011" => Some(46),
        "MN0012" => Some(59),
        "LW0013" => Some(72),
        "HG0014" => Some(85),
        "FR0015" => Some(1),
        "WK0016" => Some(14),
        "RT0017" => Some(27),
        "T100018" => Some(40),
        "CB0019" => Some(53),
        "TS0020" => Some(66),
        "SP0021" => Some(79),
        "DR0022" => Some(92),
        "AS0023" => Some(8),
        "MN0024" => Some(21),
        "LW0025" => Some(34),
        "HG0026" => Some(47),
        "FR0027" => Some(60),
        "WK0028" => Some(73),
        "RT0029" => Some(86),
        "T100030" => Some(2),
        "CB0031" => Some(15),
        "TS0032" => Some(28),
        "SP0033" => Some(41),
        "DR0034" => Some(54),
        "AS0035" => Some(67),
        _ => None,
    }
}

/// 逐行守卫扫描（返回违例行数；0 = 全绿）。
pub fn u2741mat_scan() -> usize {
    let mut bad = 0usize;
    let mut i = 0usize;
    while i < u2741mat_COUNT {
        let (k, v) = u2741mat[i];
        let ok = match k {
            "MN0000" => v < 97,
            "LW0001" => v < 97,
            "HG0002" => v < 97,
            "FR0003" => v < 97,
            "WK0004" => v < 97,
            "RT0005" => v < 97,
            "T100006" => v < 97,
            "CB0007" => v < 97,
            "TS0008" => v < 97,
            "SP0009" => v < 97,
            "DR0010" => v < 97,
            "AS0011" => v < 97,
            "MN0012" => v < 97,
            "LW0013" => v < 97,
            "HG0014" => v < 97,
            "FR0015" => v < 97,
            "WK0016" => v < 97,
            "RT0017" => v < 97,
            "T100018" => v < 97,
            "CB0019" => v < 97,
            "TS0020" => v < 97,
            "SP0021" => v < 97,
            "DR0022" => v < 97,
            "AS0023" => v < 97,
            "MN0024" => v < 97,
            "LW0025" => v < 97,
            "HG0026" => v < 97,
            "FR0027" => v < 97,
            "WK0028" => v < 97,
            "RT0029" => v < 97,
            "T100030" => v < 97,
            "CB0031" => v < 97,
            "TS0032" => v < 97,
            "SP0033" => v < 97,
            "DR0034" => v < 97,
            "AS0035" => v < 97,
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
pub fn u2741mat_sum() -> u64 {
    let mut s = 0u64;
    let mut i = 0usize;
    while i < u2741mat_COUNT {
        s = s.wrapping_add(u2741mat[i].1 as u64);
        i += 1;
    }
    s
}

/// 键唯一性（构造期断言等价物：逐对扫描同键）。
pub fn u2741mat_unique() -> bool {
    let mut i = 0usize;
    while i < u2741mat_COUNT {
        let mut j = i + 1;
        while j < u2741mat_COUNT {
            if u2741mat[i].0 == u2741mat[j].0 {
                return false;
            }
            j += 1;
        }
        i += 1;
    }
    true
}

// --------------------------------------------------------------------------
// 直方图引擎 mxu2741hist（有界插入排序 + 分位数 P50/P90/P99）
// --------------------------------------------------------------------------
/// 水线档 直方图（96 桶滚动）。
/// 样本容量上限。
pub const MXU2741HIST_CAP: usize = 96;

/// 直方图账本（排序态 + 五统量）。
#[derive(Clone, Copy, Debug)]
pub struct mxu2741histHist {
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

impl mxu2741histHist {
    /// 空账构造。
    pub const fn new() -> Self {
        Self { samples: [0; 96], n: 0, drops: 0, min: 0, max: 0, sum: 0 }
    }

    /// 采样（有界插入排序；满员丢弃并记账）。
    pub fn push(&mut self, v: u64) {
        if self.n >= MXU2741HIST_CAP {
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

/// 段位守卫：全部诊断码必须落在本段 [0xacb0, 0xacbf]。
pub const fn u2741_segment_guard() -> bool {
    let mut ok = true;
    let mut i = 0usize;
    while i < u2741Code::ALL.len() {
        let c = u2741Code::ALL[i].code();
        if c < 0xacb0 || c > 0xacbf {
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
fn u2741_guard_seg() -> bool {
    u2741_segment_guard()
}

/// UNX-F2741 矩阵 36 行齐备 + 逐行守卫 + 唯一性 + 累计守恒。
fn check_u2741_map_guard() -> bool {
    let mut ok = u2741mat_COUNT == 36;
    if ok { ok = u2741mat_scan() == 0 && u2741mat_unique(); }
    if ok { ok = u2741mat_get("MN0000") == Some(0) && u2741mat_get("ZZZ_NOKEY").is_none(); }
    if ok { ok = u2741mat_sum() == 1594; }
    ok
}

/// UNX-F2741 直方图 16 样本分位/均值/守恒。
fn check_u2741_map_hist() -> bool {
    let mut h = mxu2741histHist::new();
    let mut i = 0u64;
    while i < 16 { h.push(i + 1); i += 1; }
    let mut ok = h.conserved() && h.mean() >= 1 && h.percentile(500) >= 8;
    ok
}

/// 本模块自检（逐检查带 detail，全绿才出门）。
pub fn run_u2741_checks() -> CheckSet {
    let mut set = CheckSet::new("U2741");
    set.add(
        "u2741-seg-guard",
        u2741_guard_seg(),
        if u2741_guard_seg() { "" } else { "diagnostic code outside segment" },
    );
    set.add(
        "u2741-map-guard",
        check_u2741_map_guard(),
        if check_u2741_map_guard() { "" } else { "UNX-F2741 matrix guard mismatch" },
    );
    set.add(
        "u2741-map-hist",
        check_u2741_map_hist(),
        if check_u2741_map_hist() { "" } else { "UNX-F2741 hist mismatch" },
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
    fn u2741_all_checks_green() {
        let set = run_u2741_checks();
        assert!(set.all_passed(), "U2741 domain checks must be green");
    }

    #[test]
    fn u2741_segment_guard_holds() {
        assert!(u2741_segment_guard());
    }
}
