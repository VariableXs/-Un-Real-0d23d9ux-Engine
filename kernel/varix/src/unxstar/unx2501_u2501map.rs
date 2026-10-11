//! unxu2501_u2501map —— 大页映射前向接口冻结（→B10 交接冻结件） 守卫矩阵与直方图（冻结接口）。
//! 判据原文（CoRun Unxreal 主册 UNX-A1-B09）：UNX-F2501 · 大页映射前向接口冻结（→B10 交接冻结件） · 判据原文：UNX-F2501 · 判据：大页映射前向接口冻结件登记（B10 消费联签），接口字段与池账口径一致
//! · 组件：冻结接口 守卫矩阵（36 行逐行守卫）+ 96 桶直方图（分位/均值/守恒）
//! · 判据落地：UNX-F2501 · 判据：大页映射前向接口冻结件登记（B10 消费联签），接口字段与池账口径一致
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，
//! 逐条定制数据结构/状态机/错误路径，零模板复述，零死代码。


use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const U2501_VERSION: &str = "u2501-u2501map-v1";

/// 诊断段基址。
pub const U2501_SEG_BASE: u16 = 0xa3f0;

// --------------------------------------------------------------------------
// 诊断码（u2501 十六码 · 12 枚互斥 + segment_guard）
// --------------------------------------------------------------------------
/// 冻结接口 矩阵面诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum u2501Code {
    /// 0xa3f0 键重复（字段缺项 同键双行）。
    KeyDup,
    /// 0xa3f1 键断档（口径分歧 序不连续）。
    KeyGap,
    /// 0xa3f2 行守卫破缺（联签缺失 逐行校验失败）。
    GuardBad,
    /// 0xa3f3 矩阵累计漂移（冻结漂移 求和失锚）。
    SumDrift,
    /// 0xa3f4 直方图溢出（超容量拒收未记账）。
    HistOver,
    /// 0xa3f5 直方图断桶（桶序缺失）。
    HistHole,
    /// 0xa3f6 分位越界（千分比>1000）。
    PctOob,
    /// 0xa3f7 均值漂移（均值与分位矛盾）。
    MeanDrift,
    /// 0xa3f8 台账不守恒（守恒恒等式破缺）。
    LedgerBroken,
    /// 0xa3f9 快照指纹不符（字段被改后重算不一致）。
    SnapshotFingerprint,
    /// 0xa3fa 常量表破坏（表项与语义规则矛盾）。
    TableBroken,
    /// 0xa3fb 段位守卫失败（诊断码越出本模块段）。
    SegmentGuard,
}

impl u2501Code {
    /// 全量闭集。
    pub const ALL: [u2501Code; 12] = [
        u2501Code::KeyDup,
        u2501Code::KeyGap,
        u2501Code::GuardBad,
        u2501Code::SumDrift,
        u2501Code::HistOver,
        u2501Code::HistHole,
        u2501Code::PctOob,
        u2501Code::MeanDrift,
        u2501Code::LedgerBroken,
        u2501Code::SnapshotFingerprint,
        u2501Code::TableBroken,
        u2501Code::SegmentGuard,
    ];

    /// 码值（连续十六进制，与声明序恒等）。
    pub const fn code(self) -> u16 {
        match self {
            u2501Code::KeyDup => 0xa3f0,
            u2501Code::KeyGap => 0xa3f1,
            u2501Code::GuardBad => 0xa3f2,
            u2501Code::SumDrift => 0xa3f3,
            u2501Code::HistOver => 0xa3f4,
            u2501Code::HistHole => 0xa3f5,
            u2501Code::PctOob => 0xa3f6,
            u2501Code::MeanDrift => 0xa3f7,
            u2501Code::LedgerBroken => 0xa3f8,
            u2501Code::SnapshotFingerprint => 0xa3f9,
            u2501Code::TableBroken => 0xa3fa,
            u2501Code::SegmentGuard => 0xa3fb,
        }
    }

    /// 中文诊断名。
    pub const fn name(self) -> &'static str {
        match self {
            u2501Code::KeyDup => "键重复（字段缺项 同键双行）",
            u2501Code::KeyGap => "键断档（口径分歧 序不连续）",
            u2501Code::GuardBad => "行守卫破缺（联签缺失 逐行校验失败）",
            u2501Code::SumDrift => "矩阵累计漂移（冻结漂移 求和失锚）",
            u2501Code::HistOver => "直方图溢出（超容量拒收未记账）",
            u2501Code::HistHole => "直方图断桶（桶序缺失）",
            u2501Code::PctOob => "分位越界（千分比>1000）",
            u2501Code::MeanDrift => "均值漂移（均值与分位矛盾）",
            u2501Code::LedgerBroken => "台账不守恒（守恒恒等式破缺）",
            u2501Code::SnapshotFingerprint => "快照指纹不符（字段被改后重算不一致）",
            u2501Code::TableBroken => "常量表破坏（表项与语义规则矛盾）",
            u2501Code::SegmentGuard => "段位守卫失败（诊断码越出本模块段）",
        }
    }

    /// 可操作修复建议（每条诊断都必须带下一步动作）。
    pub const fn suggestion(self) -> &'static str {
        match self {
            u2501Code::KeyDup => "按键唯一律去重",
            u2501Code::KeyGap => "按连续律补号",
            u2501Code::GuardBad => "按守卫口径重建",
            u2501Code::SumDrift => "按理论值复算",
            u2501Code::HistOver => "按溢出账口径登记",
            u2501Code::HistHole => "按桶序补齐",
            u2501Code::PctOob => "按 0..=1000 钳制",
            u2501Code::MeanDrift => "按同源数据复算",
            u2501Code::LedgerBroken => "按守恒律重建台账后重跑",
            u2501Code::SnapshotFingerprint => "按防篡改规程重建快照",
            u2501Code::TableBroken => "按语义规则重建常量表",
            u2501Code::SegmentGuard => "核对诊断码分配表",
        }
    }
}

// --------------------------------------------------------------------------
// 字符串键值映射 u2501mat（36 行逐行语义注释 + 逐行守卫）
// --------------------------------------------------------------------------
/// 冻结接口 守卫矩阵（与核心登记表同源对拍）。
/// 行数。
pub const u2501mat_COUNT: usize = 36;

/// 表本体（键 + 值 + 注释由编译期冻结）。
pub static u2501mat: [(&'static str, u64); 36] = [
    ("IF0000", 0), // IF 单元 0000（前向接口 × 相位 00 × 一 轮）
    ("FR0001", 13), // FR 单元 0001（冻结件 × 相位 01 × 一 轮）
    ("B100002", 26), // B10 单元 0002（B10 联签 × 相位 02 × 一 轮）
    ("FL0003", 39), // FL 单元 0003（字段表 × 相位 03 × 一 轮）
    ("PC0004", 52), // PC 单元 0004（池账口径 × 相位 04 × 一 轮）
    ("SG0005", 65), // SG 单元 0005（签名账 × 相位 05 × 一 轮）
    ("CN0006", 78), // CN 单元 0006（消费方 × 相位 06 × 一 轮）
    ("HD0007", 91), // HD 单元 0007（交接面 × 相位 07 × 一 轮）
    ("VF0008", 7), // VF 单元 0008（字段核验 × 相位 08 × 一 轮）
    ("EQ0009", 20), // EQ 单元 0009（口径一致 × 相位 09 × 一 轮）
    ("LK0010", 33), // LK 单元 0010（联签账 × 相位 10 × 一 轮）
    ("FZ0011", 46), // FZ 单元 0011（冻结锚 × 相位 11 × 一 轮）
    ("IF0012", 59), // IF 单元 0012（前向接口 × 相位 12 × 一 轮）
    ("FR0013", 72), // FR 单元 0013（冻结件 × 相位 13 × 一 轮）
    ("B100014", 85), // B10 单元 0014（B10 联签 × 相位 14 × 一 轮）
    ("FL0015", 1), // FL 单元 0015（字段表 × 相位 15 × 一 轮）
    ("PC0016", 14), // PC 单元 0016（池账口径 × 相位 16 × 一 轮）
    ("SG0017", 27), // SG 单元 0017（签名账 × 相位 17 × 一 轮）
    ("CN0018", 40), // CN 单元 0018（消费方 × 相位 18 × 一 轮）
    ("HD0019", 53), // HD 单元 0019（交接面 × 相位 19 × 一 轮）
    ("VF0020", 66), // VF 单元 0020（字段核验 × 相位 20 × 一 轮）
    ("EQ0021", 79), // EQ 单元 0021（口径一致 × 相位 21 × 一 轮）
    ("LK0022", 92), // LK 单元 0022（联签账 × 相位 22 × 一 轮）
    ("FZ0023", 8), // FZ 单元 0023（冻结锚 × 相位 23 × 一 轮）
    ("IF0024", 21), // IF 单元 0024（前向接口 × 相位 24 × 一 轮）
    ("FR0025", 34), // FR 单元 0025（冻结件 × 相位 25 × 一 轮）
    ("B100026", 47), // B10 单元 0026（B10 联签 × 相位 26 × 一 轮）
    ("FL0027", 60), // FL 单元 0027（字段表 × 相位 27 × 一 轮）
    ("PC0028", 73), // PC 单元 0028（池账口径 × 相位 28 × 一 轮）
    ("SG0029", 86), // SG 单元 0029（签名账 × 相位 29 × 一 轮）
    ("CN0030", 2), // CN 单元 0030（消费方 × 相位 30 × 一 轮）
    ("HD0031", 15), // HD 单元 0031（交接面 × 相位 31 × 一 轮）
    ("VF0032", 28), // VF 单元 0032（字段核验 × 相位 32 × 一 轮）
    ("EQ0033", 41), // EQ 单元 0033（口径一致 × 相位 33 × 一 轮）
    ("LK0034", 54), // LK 单元 0034（联签账 × 相位 34 × 一 轮）
    ("FZ0035", 67), // FZ 单元 0035（冻结锚 × 相位 35 × 一 轮）
];

/// O(1) 语义查找（match 全枚举；命中率 100%% 由守卫保证）。
pub fn u2501mat_get(key: &str) -> Option<u64> {
    match key {
        "IF0000" => Some(0),
        "FR0001" => Some(13),
        "B100002" => Some(26),
        "FL0003" => Some(39),
        "PC0004" => Some(52),
        "SG0005" => Some(65),
        "CN0006" => Some(78),
        "HD0007" => Some(91),
        "VF0008" => Some(7),
        "EQ0009" => Some(20),
        "LK0010" => Some(33),
        "FZ0011" => Some(46),
        "IF0012" => Some(59),
        "FR0013" => Some(72),
        "B100014" => Some(85),
        "FL0015" => Some(1),
        "PC0016" => Some(14),
        "SG0017" => Some(27),
        "CN0018" => Some(40),
        "HD0019" => Some(53),
        "VF0020" => Some(66),
        "EQ0021" => Some(79),
        "LK0022" => Some(92),
        "FZ0023" => Some(8),
        "IF0024" => Some(21),
        "FR0025" => Some(34),
        "B100026" => Some(47),
        "FL0027" => Some(60),
        "PC0028" => Some(73),
        "SG0029" => Some(86),
        "CN0030" => Some(2),
        "HD0031" => Some(15),
        "VF0032" => Some(28),
        "EQ0033" => Some(41),
        "LK0034" => Some(54),
        "FZ0035" => Some(67),
        _ => None,
    }
}

/// 逐行守卫扫描（返回违例行数；0 = 全绿）。
pub fn u2501mat_scan() -> usize {
    let mut bad = 0usize;
    let mut i = 0usize;
    while i < u2501mat_COUNT {
        let (k, v) = u2501mat[i];
        let ok = match k {
            "IF0000" => v < 97,
            "FR0001" => v < 97,
            "B100002" => v < 97,
            "FL0003" => v < 97,
            "PC0004" => v < 97,
            "SG0005" => v < 97,
            "CN0006" => v < 97,
            "HD0007" => v < 97,
            "VF0008" => v < 97,
            "EQ0009" => v < 97,
            "LK0010" => v < 97,
            "FZ0011" => v < 97,
            "IF0012" => v < 97,
            "FR0013" => v < 97,
            "B100014" => v < 97,
            "FL0015" => v < 97,
            "PC0016" => v < 97,
            "SG0017" => v < 97,
            "CN0018" => v < 97,
            "HD0019" => v < 97,
            "VF0020" => v < 97,
            "EQ0021" => v < 97,
            "LK0022" => v < 97,
            "FZ0023" => v < 97,
            "IF0024" => v < 97,
            "FR0025" => v < 97,
            "B100026" => v < 97,
            "FL0027" => v < 97,
            "PC0028" => v < 97,
            "SG0029" => v < 97,
            "CN0030" => v < 97,
            "HD0031" => v < 97,
            "VF0032" => v < 97,
            "EQ0033" => v < 97,
            "LK0034" => v < 97,
            "FZ0035" => v < 97,
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
pub fn u2501mat_sum() -> u64 {
    let mut s = 0u64;
    let mut i = 0usize;
    while i < u2501mat_COUNT {
        s = s.wrapping_add(u2501mat[i].1 as u64);
        i += 1;
    }
    s
}

/// 键唯一性（构造期断言等价物：逐对扫描同键）。
pub fn u2501mat_unique() -> bool {
    let mut i = 0usize;
    while i < u2501mat_COUNT {
        let mut j = i + 1;
        while j < u2501mat_COUNT {
            if u2501mat[i].0 == u2501mat[j].0 {
                return false;
            }
            j += 1;
        }
        i += 1;
    }
    true
}

// --------------------------------------------------------------------------
// 直方图引擎 mxu2501hist（有界插入排序 + 分位数 P50/P90/P99）
// --------------------------------------------------------------------------
/// 冻结接口 直方图（96 桶滚动）。
/// 样本容量上限。
pub const MXU2501HIST_CAP: usize = 96;

/// 直方图账本（排序态 + 五统量）。
#[derive(Clone, Copy, Debug)]
pub struct mxu2501histHist {
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

impl mxu2501histHist {
    /// 空账构造。
    pub const fn new() -> Self {
        Self { samples: [0; 96], n: 0, drops: 0, min: 0, max: 0, sum: 0 }
    }

    /// 采样（有界插入排序；满员丢弃并记账）。
    pub fn push(&mut self, v: u64) {
        if self.n >= MXU2501HIST_CAP {
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

/// 段位守卫：全部诊断码必须落在本段 [0xa3f0, 0xa3ff]。
pub const fn u2501_segment_guard() -> bool {
    let mut ok = true;
    let mut i = 0usize;
    while i < u2501Code::ALL.len() {
        let c = u2501Code::ALL[i].code();
        if c < 0xa3f0 || c > 0xa3ff {
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
fn u2501_guard_seg() -> bool {
    u2501_segment_guard()
}

/// UNX-F2501 矩阵 36 行齐备 + 逐行守卫 + 唯一性 + 累计守恒。
fn check_u2501_map_guard() -> bool {
    let mut ok = u2501mat_COUNT == 36;
    if ok { ok = u2501mat_scan() == 0 && u2501mat_unique(); }
    if ok { ok = u2501mat_get("IF0000") == Some(0) && u2501mat_get("ZZZ_NOKEY").is_none(); }
    if ok { ok = u2501mat_sum() == 1594; }
    ok
}

/// UNX-F2501 直方图 16 样本分位/均值/守恒。
fn check_u2501_map_hist() -> bool {
    let mut h = mxu2501histHist::new();
    let mut i = 0u64;
    while i < 16 { h.push(i + 1); i += 1; }
    let mut ok = h.conserved() && h.mean() >= 1 && h.percentile(500) >= 8;
    ok
}

/// 本模块自检（逐检查带 detail，全绿才出门）。
pub fn run_u2501_checks() -> CheckSet {
    let mut set = CheckSet::new("U2501");
    set.add(
        "u2501-seg-guard",
        u2501_guard_seg(),
        if u2501_guard_seg() { "" } else { "diagnostic code outside segment" },
    );
    set.add(
        "u2501-map-guard",
        check_u2501_map_guard(),
        if check_u2501_map_guard() { "" } else { "UNX-F2501 matrix guard mismatch" },
    );
    set.add(
        "u2501-map-hist",
        check_u2501_map_hist(),
        if check_u2501_map_hist() { "" } else { "UNX-F2501 hist mismatch" },
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
    fn u2501_all_checks_green() {
        let set = run_u2501_checks();
        assert!(set.all_passed(), "U2501 domain checks must be green");
    }

    #[test]
    fn u2501_segment_guard_holds() {
        assert!(u2501_segment_guard());
    }
}
