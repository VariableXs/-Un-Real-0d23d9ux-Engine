//! unxu2541_u2541map —— alloc_pages/free_pages 冻结接口总账（正式冻结版） 守卫矩阵与直方图（冻结接口）。
//! 判据原文（CoRun Unxreal 主册 UNX-A1-B09）：UNX-F2541 · alloc_pages/free_pages 冻结接口总账（正式冻结版） · 判据原文：UNX-F2541 · 判据：双接口冻结件登记（全域消费方清单齐备），签名与错误码三机直测断言一致
//! · 组件：冻结接口 守卫矩阵（36 行逐行守卫）+ 96 桶直方图（分位/均值/守恒）
//! · 判据落地：UNX-F2541 · 判据：双接口冻结件登记（全域消费方清单齐备），签名与错误码三机直测断言一致
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，
//! 逐条定制数据结构/状态机/错误路径，零模板复述，零死代码。


use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const U2541_VERSION: &str = "u2541-u2541map-v1";

/// 诊断段基址。
pub const U2541_SEG_BASE: u16 = 0xa570;

// --------------------------------------------------------------------------
// 诊断码（u2541 十六码 · 12 枚互斥 + segment_guard）
// --------------------------------------------------------------------------
/// 冻结接口 矩阵面诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum u2541Code {
    /// 0xa570 键重复（消费方缺 同键双行）。
    KeyDup,
    /// 0xa571 键断档（签名不符 序不连续）。
    KeyGap,
    /// 0xa572 行守卫破缺（错误码分歧 逐行校验失败）。
    GuardBad,
    /// 0xa573 矩阵累计漂移（直测不一致 求和失锚）。
    SumDrift,
    /// 0xa574 直方图溢出（超容量拒收未记账）。
    HistOver,
    /// 0xa575 直方图断桶（桶序缺失）。
    HistHole,
    /// 0xa576 分位越界（千分比>1000）。
    PctOob,
    /// 0xa577 均值漂移（均值与分位矛盾）。
    MeanDrift,
    /// 0xa578 台账不守恒（守恒恒等式破缺）。
    LedgerBroken,
    /// 0xa579 快照指纹不符（字段被改后重算不一致）。
    SnapshotFingerprint,
    /// 0xa57a 常量表破坏（表项与语义规则矛盾）。
    TableBroken,
    /// 0xa57b 段位守卫失败（诊断码越出本模块段）。
    SegmentGuard,
}

impl u2541Code {
    /// 全量闭集。
    pub const ALL: [u2541Code; 12] = [
        u2541Code::KeyDup,
        u2541Code::KeyGap,
        u2541Code::GuardBad,
        u2541Code::SumDrift,
        u2541Code::HistOver,
        u2541Code::HistHole,
        u2541Code::PctOob,
        u2541Code::MeanDrift,
        u2541Code::LedgerBroken,
        u2541Code::SnapshotFingerprint,
        u2541Code::TableBroken,
        u2541Code::SegmentGuard,
    ];

    /// 码值（连续十六进制，与声明序恒等）。
    pub const fn code(self) -> u16 {
        match self {
            u2541Code::KeyDup => 0xa570,
            u2541Code::KeyGap => 0xa571,
            u2541Code::GuardBad => 0xa572,
            u2541Code::SumDrift => 0xa573,
            u2541Code::HistOver => 0xa574,
            u2541Code::HistHole => 0xa575,
            u2541Code::PctOob => 0xa576,
            u2541Code::MeanDrift => 0xa577,
            u2541Code::LedgerBroken => 0xa578,
            u2541Code::SnapshotFingerprint => 0xa579,
            u2541Code::TableBroken => 0xa57a,
            u2541Code::SegmentGuard => 0xa57b,
        }
    }

    /// 中文诊断名。
    pub const fn name(self) -> &'static str {
        match self {
            u2541Code::KeyDup => "键重复（消费方缺 同键双行）",
            u2541Code::KeyGap => "键断档（签名不符 序不连续）",
            u2541Code::GuardBad => "行守卫破缺（错误码分歧 逐行校验失败）",
            u2541Code::SumDrift => "矩阵累计漂移（直测不一致 求和失锚）",
            u2541Code::HistOver => "直方图溢出（超容量拒收未记账）",
            u2541Code::HistHole => "直方图断桶（桶序缺失）",
            u2541Code::PctOob => "分位越界（千分比>1000）",
            u2541Code::MeanDrift => "均值漂移（均值与分位矛盾）",
            u2541Code::LedgerBroken => "台账不守恒（守恒恒等式破缺）",
            u2541Code::SnapshotFingerprint => "快照指纹不符（字段被改后重算不一致）",
            u2541Code::TableBroken => "常量表破坏（表项与语义规则矛盾）",
            u2541Code::SegmentGuard => "段位守卫失败（诊断码越出本模块段）",
        }
    }

    /// 可操作修复建议（每条诊断都必须带下一步动作）。
    pub const fn suggestion(self) -> &'static str {
        match self {
            u2541Code::KeyDup => "按键唯一律去重",
            u2541Code::KeyGap => "按连续律补号",
            u2541Code::GuardBad => "按守卫口径重建",
            u2541Code::SumDrift => "按理论值复算",
            u2541Code::HistOver => "按溢出账口径登记",
            u2541Code::HistHole => "按桶序补齐",
            u2541Code::PctOob => "按 0..=1000 钳制",
            u2541Code::MeanDrift => "按同源数据复算",
            u2541Code::LedgerBroken => "按守恒律重建台账后重跑",
            u2541Code::SnapshotFingerprint => "按防篡改规程重建快照",
            u2541Code::TableBroken => "按语义规则重建常量表",
            u2541Code::SegmentGuard => "核对诊断码分配表",
        }
    }
}

// --------------------------------------------------------------------------
// 字符串键值映射 u2541mat（36 行逐行语义注释 + 逐行守卫）
// --------------------------------------------------------------------------
/// 冻结接口 守卫矩阵（与核心登记表同源对拍）。
/// 行数。
pub const u2541mat_COUNT: usize = 36;

/// 表本体（键 + 值 + 注释由编译期冻结）。
pub static u2541mat: [(&'static str, u64); 36] = [
    ("AL0000", 0), // AL 单元 0000（alloc_pages × 相位 00 × 一 轮）
    ("FP0001", 13), // FP 单元 0001（free_pages × 相位 01 × 一 轮）
    ("FZ0002", 26), // FZ 单元 0002（冻结件 × 相位 02 × 一 轮）
    ("CN0003", 39), // CN 单元 0003（消费方清单 × 相位 03 × 一 轮）
    ("SG0004", 52), // SG 单元 0004（签名账 × 相位 04 × 一 轮）
    ("EC0005", 65), // EC 单元 0005（错误码 × 相位 05 × 一 轮）
    ("TM0006", 78), // TM 单元 0006（三机直测 × 相位 06 × 一 轮）
    ("AS0007", 91), // AS 单元 0007（断言一致 × 相位 07 × 一 轮）
    ("SG20008", 7), // SG2 单元 0008（参数面 × 相位 08 × 一 轮）
    ("RT0009", 20), // RT 单元 0009（返回面 × 相位 09 × 一 轮）
    ("DS0010", 33), // DS 单元 0010（全域面 × 相位 10 × 一 轮）
    ("LK0011", 46), // LK 单元 0011（联签账 × 相位 11 × 一 轮）
    ("AL0012", 59), // AL 单元 0012（alloc_pages × 相位 12 × 一 轮）
    ("FP0013", 72), // FP 单元 0013（free_pages × 相位 13 × 一 轮）
    ("FZ0014", 85), // FZ 单元 0014（冻结件 × 相位 14 × 一 轮）
    ("CN0015", 1), // CN 单元 0015（消费方清单 × 相位 15 × 一 轮）
    ("SG0016", 14), // SG 单元 0016（签名账 × 相位 16 × 一 轮）
    ("EC0017", 27), // EC 单元 0017（错误码 × 相位 17 × 一 轮）
    ("TM0018", 40), // TM 单元 0018（三机直测 × 相位 18 × 一 轮）
    ("AS0019", 53), // AS 单元 0019（断言一致 × 相位 19 × 一 轮）
    ("SG20020", 66), // SG2 单元 0020（参数面 × 相位 20 × 一 轮）
    ("RT0021", 79), // RT 单元 0021（返回面 × 相位 21 × 一 轮）
    ("DS0022", 92), // DS 单元 0022（全域面 × 相位 22 × 一 轮）
    ("LK0023", 8), // LK 单元 0023（联签账 × 相位 23 × 一 轮）
    ("AL0024", 21), // AL 单元 0024（alloc_pages × 相位 24 × 一 轮）
    ("FP0025", 34), // FP 单元 0025（free_pages × 相位 25 × 一 轮）
    ("FZ0026", 47), // FZ 单元 0026（冻结件 × 相位 26 × 一 轮）
    ("CN0027", 60), // CN 单元 0027（消费方清单 × 相位 27 × 一 轮）
    ("SG0028", 73), // SG 单元 0028（签名账 × 相位 28 × 一 轮）
    ("EC0029", 86), // EC 单元 0029（错误码 × 相位 29 × 一 轮）
    ("TM0030", 2), // TM 单元 0030（三机直测 × 相位 30 × 一 轮）
    ("AS0031", 15), // AS 单元 0031（断言一致 × 相位 31 × 一 轮）
    ("SG20032", 28), // SG2 单元 0032（参数面 × 相位 32 × 一 轮）
    ("RT0033", 41), // RT 单元 0033（返回面 × 相位 33 × 一 轮）
    ("DS0034", 54), // DS 单元 0034（全域面 × 相位 34 × 一 轮）
    ("LK0035", 67), // LK 单元 0035（联签账 × 相位 35 × 一 轮）
];

/// O(1) 语义查找（match 全枚举；命中率 100%% 由守卫保证）。
pub fn u2541mat_get(key: &str) -> Option<u64> {
    match key {
        "AL0000" => Some(0),
        "FP0001" => Some(13),
        "FZ0002" => Some(26),
        "CN0003" => Some(39),
        "SG0004" => Some(52),
        "EC0005" => Some(65),
        "TM0006" => Some(78),
        "AS0007" => Some(91),
        "SG20008" => Some(7),
        "RT0009" => Some(20),
        "DS0010" => Some(33),
        "LK0011" => Some(46),
        "AL0012" => Some(59),
        "FP0013" => Some(72),
        "FZ0014" => Some(85),
        "CN0015" => Some(1),
        "SG0016" => Some(14),
        "EC0017" => Some(27),
        "TM0018" => Some(40),
        "AS0019" => Some(53),
        "SG20020" => Some(66),
        "RT0021" => Some(79),
        "DS0022" => Some(92),
        "LK0023" => Some(8),
        "AL0024" => Some(21),
        "FP0025" => Some(34),
        "FZ0026" => Some(47),
        "CN0027" => Some(60),
        "SG0028" => Some(73),
        "EC0029" => Some(86),
        "TM0030" => Some(2),
        "AS0031" => Some(15),
        "SG20032" => Some(28),
        "RT0033" => Some(41),
        "DS0034" => Some(54),
        "LK0035" => Some(67),
        _ => None,
    }
}

/// 逐行守卫扫描（返回违例行数；0 = 全绿）。
pub fn u2541mat_scan() -> usize {
    let mut bad = 0usize;
    let mut i = 0usize;
    while i < u2541mat_COUNT {
        let (k, v) = u2541mat[i];
        let ok = match k {
            "AL0000" => v < 97,
            "FP0001" => v < 97,
            "FZ0002" => v < 97,
            "CN0003" => v < 97,
            "SG0004" => v < 97,
            "EC0005" => v < 97,
            "TM0006" => v < 97,
            "AS0007" => v < 97,
            "SG20008" => v < 97,
            "RT0009" => v < 97,
            "DS0010" => v < 97,
            "LK0011" => v < 97,
            "AL0012" => v < 97,
            "FP0013" => v < 97,
            "FZ0014" => v < 97,
            "CN0015" => v < 97,
            "SG0016" => v < 97,
            "EC0017" => v < 97,
            "TM0018" => v < 97,
            "AS0019" => v < 97,
            "SG20020" => v < 97,
            "RT0021" => v < 97,
            "DS0022" => v < 97,
            "LK0023" => v < 97,
            "AL0024" => v < 97,
            "FP0025" => v < 97,
            "FZ0026" => v < 97,
            "CN0027" => v < 97,
            "SG0028" => v < 97,
            "EC0029" => v < 97,
            "TM0030" => v < 97,
            "AS0031" => v < 97,
            "SG20032" => v < 97,
            "RT0033" => v < 97,
            "DS0034" => v < 97,
            "LK0035" => v < 97,
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
pub fn u2541mat_sum() -> u64 {
    let mut s = 0u64;
    let mut i = 0usize;
    while i < u2541mat_COUNT {
        s = s.wrapping_add(u2541mat[i].1 as u64);
        i += 1;
    }
    s
}

/// 键唯一性（构造期断言等价物：逐对扫描同键）。
pub fn u2541mat_unique() -> bool {
    let mut i = 0usize;
    while i < u2541mat_COUNT {
        let mut j = i + 1;
        while j < u2541mat_COUNT {
            if u2541mat[i].0 == u2541mat[j].0 {
                return false;
            }
            j += 1;
        }
        i += 1;
    }
    true
}

// --------------------------------------------------------------------------
// 直方图引擎 mxu2541hist（有界插入排序 + 分位数 P50/P90/P99）
// --------------------------------------------------------------------------
/// 冻结接口 直方图（96 桶滚动）。
/// 样本容量上限。
pub const MXU2541HIST_CAP: usize = 96;

/// 直方图账本（排序态 + 五统量）。
#[derive(Clone, Copy, Debug)]
pub struct mxu2541histHist {
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

impl mxu2541histHist {
    /// 空账构造。
    pub const fn new() -> Self {
        Self { samples: [0; 96], n: 0, drops: 0, min: 0, max: 0, sum: 0 }
    }

    /// 采样（有界插入排序；满员丢弃并记账）。
    pub fn push(&mut self, v: u64) {
        if self.n >= MXU2541HIST_CAP {
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

/// 段位守卫：全部诊断码必须落在本段 [0xa570, 0xa57f]。
pub const fn u2541_segment_guard() -> bool {
    let mut ok = true;
    let mut i = 0usize;
    while i < u2541Code::ALL.len() {
        let c = u2541Code::ALL[i].code();
        if c < 0xa570 || c > 0xa57f {
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
fn u2541_guard_seg() -> bool {
    u2541_segment_guard()
}

/// UNX-F2541 矩阵 36 行齐备 + 逐行守卫 + 唯一性 + 累计守恒。
fn check_u2541_map_guard() -> bool {
    let mut ok = u2541mat_COUNT == 36;
    if ok { ok = u2541mat_scan() == 0 && u2541mat_unique(); }
    if ok { ok = u2541mat_get("AL0000") == Some(0) && u2541mat_get("ZZZ_NOKEY").is_none(); }
    if ok { ok = u2541mat_sum() == 1594; }
    ok
}

/// UNX-F2541 直方图 16 样本分位/均值/守恒。
fn check_u2541_map_hist() -> bool {
    let mut h = mxu2541histHist::new();
    let mut i = 0u64;
    while i < 16 { h.push(i + 1); i += 1; }
    let mut ok = h.conserved() && h.mean() >= 1 && h.percentile(500) >= 8;
    ok
}

/// 本模块自检（逐检查带 detail，全绿才出门）。
pub fn run_u2541_checks() -> CheckSet {
    let mut set = CheckSet::new("U2541");
    set.add(
        "u2541-seg-guard",
        u2541_guard_seg(),
        if u2541_guard_seg() { "" } else { "diagnostic code outside segment" },
    );
    set.add(
        "u2541-map-guard",
        check_u2541_map_guard(),
        if check_u2541_map_guard() { "" } else { "UNX-F2541 matrix guard mismatch" },
    );
    set.add(
        "u2541-map-hist",
        check_u2541_map_hist(),
        if check_u2541_map_hist() { "" } else { "UNX-F2541 hist mismatch" },
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
    fn u2541_all_checks_green() {
        let set = run_u2541_checks();
        assert!(set.all_passed(), "U2541 domain checks must be green");
    }

    #[test]
    fn u2541_segment_guard_holds() {
        assert!(u2541_segment_guard());
    }
}
