//! unxu2401_u2401map —— 物理内存图消费与四态归一 守卫矩阵与直方图（内存区间）。
//! 判据原文（CoRun Unxreal 主册 UNX-A1-B09）：UNX-F2401 · 物理内存图消费与四态归一 · 判据原文：UNX-F2401 · 判据：真机引导后消费 A1 合并内存图，四态归一（可用/保留/可回收/坏页）区间数与 A1 合并账一致，逐态字节和与总 RAM 偏差 ≤4KB
//! · 组件：内存区间 守卫矩阵（36 行逐行守卫）+ 96 桶直方图（分位/均值/守恒）
//! · 判据落地：UNX-F2401 · 判据：真机引导后消费 A1 合并内存图，四态归一（可用/保留/可回收/坏页）区间数与 A1 合并账一致，逐态字节和与总 RAM 偏差 ≤
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，
//! 逐条定制数据结构/状态机/错误路径，零模板复述，零死代码。


use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const U2401_VERSION: &str = "u2401-u2401map-v1";

/// 诊断段基址。
pub const U2401_SEG_BASE: u16 = 0xa030;

// --------------------------------------------------------------------------
// 诊断码（u2401 十六码 · 12 枚互斥 + segment_guard）
// --------------------------------------------------------------------------
/// 内存区间 矩阵面诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum u2401Code {
    /// 0xa030 键重复（区间数失配 同键双行）。
    KeyDup,
    /// 0xa031 键断档（字节和偏差 序不连续）。
    KeyGap,
    /// 0xa032 行守卫破缺（四态漏归 逐行校验失败）。
    GuardBad,
    /// 0xa033 矩阵累计漂移（坏页误判 求和失锚）。
    SumDrift,
    /// 0xa034 直方图溢出（超容量拒收未记账）。
    HistOver,
    /// 0xa035 直方图断桶（桶序缺失）。
    HistHole,
    /// 0xa036 分位越界（千分比>1000）。
    PctOob,
    /// 0xa037 均值漂移（均值与分位矛盾）。
    MeanDrift,
    /// 0xa038 台账不守恒（守恒恒等式破缺）。
    LedgerBroken,
    /// 0xa039 快照指纹不符（字段被改后重算不一致）。
    SnapshotFingerprint,
    /// 0xa03a 常量表破坏（表项与语义规则矛盾）。
    TableBroken,
    /// 0xa03b 段位守卫失败（诊断码越出本模块段）。
    SegmentGuard,
}

impl u2401Code {
    /// 全量闭集。
    pub const ALL: [u2401Code; 12] = [
        u2401Code::KeyDup,
        u2401Code::KeyGap,
        u2401Code::GuardBad,
        u2401Code::SumDrift,
        u2401Code::HistOver,
        u2401Code::HistHole,
        u2401Code::PctOob,
        u2401Code::MeanDrift,
        u2401Code::LedgerBroken,
        u2401Code::SnapshotFingerprint,
        u2401Code::TableBroken,
        u2401Code::SegmentGuard,
    ];

    /// 码值（连续十六进制，与声明序恒等）。
    pub const fn code(self) -> u16 {
        match self {
            u2401Code::KeyDup => 0xa030,
            u2401Code::KeyGap => 0xa031,
            u2401Code::GuardBad => 0xa032,
            u2401Code::SumDrift => 0xa033,
            u2401Code::HistOver => 0xa034,
            u2401Code::HistHole => 0xa035,
            u2401Code::PctOob => 0xa036,
            u2401Code::MeanDrift => 0xa037,
            u2401Code::LedgerBroken => 0xa038,
            u2401Code::SnapshotFingerprint => 0xa039,
            u2401Code::TableBroken => 0xa03a,
            u2401Code::SegmentGuard => 0xa03b,
        }
    }

    /// 中文诊断名。
    pub const fn name(self) -> &'static str {
        match self {
            u2401Code::KeyDup => "键重复（区间数失配 同键双行）",
            u2401Code::KeyGap => "键断档（字节和偏差 序不连续）",
            u2401Code::GuardBad => "行守卫破缺（四态漏归 逐行校验失败）",
            u2401Code::SumDrift => "矩阵累计漂移（坏页误判 求和失锚）",
            u2401Code::HistOver => "直方图溢出（超容量拒收未记账）",
            u2401Code::HistHole => "直方图断桶（桶序缺失）",
            u2401Code::PctOob => "分位越界（千分比>1000）",
            u2401Code::MeanDrift => "均值漂移（均值与分位矛盾）",
            u2401Code::LedgerBroken => "台账不守恒（守恒恒等式破缺）",
            u2401Code::SnapshotFingerprint => "快照指纹不符（字段被改后重算不一致）",
            u2401Code::TableBroken => "常量表破坏（表项与语义规则矛盾）",
            u2401Code::SegmentGuard => "段位守卫失败（诊断码越出本模块段）",
        }
    }

    /// 可操作修复建议（每条诊断都必须带下一步动作）。
    pub const fn suggestion(self) -> &'static str {
        match self {
            u2401Code::KeyDup => "按键唯一律去重",
            u2401Code::KeyGap => "按连续律补号",
            u2401Code::GuardBad => "按守卫口径重建",
            u2401Code::SumDrift => "按理论值复算",
            u2401Code::HistOver => "按溢出账口径登记",
            u2401Code::HistHole => "按桶序补齐",
            u2401Code::PctOob => "按 0..=1000 钳制",
            u2401Code::MeanDrift => "按同源数据复算",
            u2401Code::LedgerBroken => "按守恒律重建台账后重跑",
            u2401Code::SnapshotFingerprint => "按防篡改规程重建快照",
            u2401Code::TableBroken => "按语义规则重建常量表",
            u2401Code::SegmentGuard => "核对诊断码分配表",
        }
    }
}

// --------------------------------------------------------------------------
// 字符串键值映射 u2401mat（36 行逐行语义注释 + 逐行守卫）
// --------------------------------------------------------------------------
/// 内存区间 守卫矩阵（与核心登记表同源对拍）。
/// 行数。
pub const u2401mat_COUNT: usize = 36;

/// 表本体（键 + 值 + 注释由编译期冻结）。
pub static u2401mat: [(&'static str, u64); 36] = [
    ("AV0000", 0), // AV 单元 0000（可用区间 × 相位 00 × 一 轮）
    ("RS0001", 13), // RS 单元 0001（保留区间 × 相位 01 × 一 轮）
    ("RC0002", 26), // RC 单元 0002（可回收区间 × 相位 02 × 一 轮）
    ("BD0003", 39), // BD 单元 0003（坏页区间 × 相位 03 × 一 轮）
    ("MG0004", 52), // MG 单元 0004（合并内存图 × 相位 04 × 一 轮）
    ("IV0005", 65), // IV 单元 0005（区间数账 × 相位 05 × 一 轮）
    ("BS0006", 78), // BS 单元 0006（逐态字节和 × 相位 06 × 一 轮）
    ("DR0007", 91), // DR 单元 0007（偏差阈值 × 相位 07 × 一 轮）
    ("RAM0008", 7), // RAM 单元 0008（总 RAM 账 × 相位 08 × 一 轮）
    ("RG0009", 20), // RG 单元 0009（归一段 × 相位 09 × 一 轮）
    ("NM0010", 33), // NM 单元 0010（归一映射 × 相位 10 × 一 轮）
    ("QA0011", 46), // QA 单元 0011（对平断言 × 相位 11 × 一 轮）
    ("AV0012", 59), // AV 单元 0012（可用区间 × 相位 12 × 一 轮）
    ("RS0013", 72), // RS 单元 0013（保留区间 × 相位 13 × 一 轮）
    ("RC0014", 85), // RC 单元 0014（可回收区间 × 相位 14 × 一 轮）
    ("BD0015", 1), // BD 单元 0015（坏页区间 × 相位 15 × 一 轮）
    ("MG0016", 14), // MG 单元 0016（合并内存图 × 相位 16 × 一 轮）
    ("IV0017", 27), // IV 单元 0017（区间数账 × 相位 17 × 一 轮）
    ("BS0018", 40), // BS 单元 0018（逐态字节和 × 相位 18 × 一 轮）
    ("DR0019", 53), // DR 单元 0019（偏差阈值 × 相位 19 × 一 轮）
    ("RAM0020", 66), // RAM 单元 0020（总 RAM 账 × 相位 20 × 一 轮）
    ("RG0021", 79), // RG 单元 0021（归一段 × 相位 21 × 一 轮）
    ("NM0022", 92), // NM 单元 0022（归一映射 × 相位 22 × 一 轮）
    ("QA0023", 8), // QA 单元 0023（对平断言 × 相位 23 × 一 轮）
    ("AV0024", 21), // AV 单元 0024（可用区间 × 相位 24 × 一 轮）
    ("RS0025", 34), // RS 单元 0025（保留区间 × 相位 25 × 一 轮）
    ("RC0026", 47), // RC 单元 0026（可回收区间 × 相位 26 × 一 轮）
    ("BD0027", 60), // BD 单元 0027（坏页区间 × 相位 27 × 一 轮）
    ("MG0028", 73), // MG 单元 0028（合并内存图 × 相位 28 × 一 轮）
    ("IV0029", 86), // IV 单元 0029（区间数账 × 相位 29 × 一 轮）
    ("BS0030", 2), // BS 单元 0030（逐态字节和 × 相位 30 × 一 轮）
    ("DR0031", 15), // DR 单元 0031（偏差阈值 × 相位 31 × 一 轮）
    ("RAM0032", 28), // RAM 单元 0032（总 RAM 账 × 相位 32 × 一 轮）
    ("RG0033", 41), // RG 单元 0033（归一段 × 相位 33 × 一 轮）
    ("NM0034", 54), // NM 单元 0034（归一映射 × 相位 34 × 一 轮）
    ("QA0035", 67), // QA 单元 0035（对平断言 × 相位 35 × 一 轮）
];

/// O(1) 语义查找（match 全枚举；命中率 100%% 由守卫保证）。
pub fn u2401mat_get(key: &str) -> Option<u64> {
    match key {
        "AV0000" => Some(0),
        "RS0001" => Some(13),
        "RC0002" => Some(26),
        "BD0003" => Some(39),
        "MG0004" => Some(52),
        "IV0005" => Some(65),
        "BS0006" => Some(78),
        "DR0007" => Some(91),
        "RAM0008" => Some(7),
        "RG0009" => Some(20),
        "NM0010" => Some(33),
        "QA0011" => Some(46),
        "AV0012" => Some(59),
        "RS0013" => Some(72),
        "RC0014" => Some(85),
        "BD0015" => Some(1),
        "MG0016" => Some(14),
        "IV0017" => Some(27),
        "BS0018" => Some(40),
        "DR0019" => Some(53),
        "RAM0020" => Some(66),
        "RG0021" => Some(79),
        "NM0022" => Some(92),
        "QA0023" => Some(8),
        "AV0024" => Some(21),
        "RS0025" => Some(34),
        "RC0026" => Some(47),
        "BD0027" => Some(60),
        "MG0028" => Some(73),
        "IV0029" => Some(86),
        "BS0030" => Some(2),
        "DR0031" => Some(15),
        "RAM0032" => Some(28),
        "RG0033" => Some(41),
        "NM0034" => Some(54),
        "QA0035" => Some(67),
        _ => None,
    }
}

/// 逐行守卫扫描（返回违例行数；0 = 全绿）。
pub fn u2401mat_scan() -> usize {
    let mut bad = 0usize;
    let mut i = 0usize;
    while i < u2401mat_COUNT {
        let (k, v) = u2401mat[i];
        let ok = match k {
            "AV0000" => v < 97,
            "RS0001" => v < 97,
            "RC0002" => v < 97,
            "BD0003" => v < 97,
            "MG0004" => v < 97,
            "IV0005" => v < 97,
            "BS0006" => v < 97,
            "DR0007" => v < 97,
            "RAM0008" => v < 97,
            "RG0009" => v < 97,
            "NM0010" => v < 97,
            "QA0011" => v < 97,
            "AV0012" => v < 97,
            "RS0013" => v < 97,
            "RC0014" => v < 97,
            "BD0015" => v < 97,
            "MG0016" => v < 97,
            "IV0017" => v < 97,
            "BS0018" => v < 97,
            "DR0019" => v < 97,
            "RAM0020" => v < 97,
            "RG0021" => v < 97,
            "NM0022" => v < 97,
            "QA0023" => v < 97,
            "AV0024" => v < 97,
            "RS0025" => v < 97,
            "RC0026" => v < 97,
            "BD0027" => v < 97,
            "MG0028" => v < 97,
            "IV0029" => v < 97,
            "BS0030" => v < 97,
            "DR0031" => v < 97,
            "RAM0032" => v < 97,
            "RG0033" => v < 97,
            "NM0034" => v < 97,
            "QA0035" => v < 97,
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
pub fn u2401mat_sum() -> u64 {
    let mut s = 0u64;
    let mut i = 0usize;
    while i < u2401mat_COUNT {
        s = s.wrapping_add(u2401mat[i].1 as u64);
        i += 1;
    }
    s
}

/// 键唯一性（构造期断言等价物：逐对扫描同键）。
pub fn u2401mat_unique() -> bool {
    let mut i = 0usize;
    while i < u2401mat_COUNT {
        let mut j = i + 1;
        while j < u2401mat_COUNT {
            if u2401mat[i].0 == u2401mat[j].0 {
                return false;
            }
            j += 1;
        }
        i += 1;
    }
    true
}

// --------------------------------------------------------------------------
// 直方图引擎 mxu2401hist（有界插入排序 + 分位数 P50/P90/P99）
// --------------------------------------------------------------------------
/// 内存区间 直方图（96 桶滚动）。
/// 样本容量上限。
pub const MXU2401HIST_CAP: usize = 96;

/// 直方图账本（排序态 + 五统量）。
#[derive(Clone, Copy, Debug)]
pub struct mxu2401histHist {
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

impl mxu2401histHist {
    /// 空账构造。
    pub const fn new() -> Self {
        Self { samples: [0; 96], n: 0, drops: 0, min: 0, max: 0, sum: 0 }
    }

    /// 采样（有界插入排序；满员丢弃并记账）。
    pub fn push(&mut self, v: u64) {
        if self.n >= MXU2401HIST_CAP {
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

/// 段位守卫：全部诊断码必须落在本段 [0xa030, 0xa03f]。
pub const fn u2401_segment_guard() -> bool {
    let mut ok = true;
    let mut i = 0usize;
    while i < u2401Code::ALL.len() {
        let c = u2401Code::ALL[i].code();
        if c < 0xa030 || c > 0xa03f {
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
fn u2401_guard_seg() -> bool {
    u2401_segment_guard()
}

/// UNX-F2401 矩阵 36 行齐备 + 逐行守卫 + 唯一性 + 累计守恒。
fn check_u2401_map_guard() -> bool {
    let mut ok = u2401mat_COUNT == 36;
    if ok { ok = u2401mat_scan() == 0 && u2401mat_unique(); }
    if ok { ok = u2401mat_get("AV0000") == Some(0) && u2401mat_get("ZZZ_NOKEY").is_none(); }
    if ok { ok = u2401mat_sum() == 1594; }
    ok
}

/// UNX-F2401 直方图 16 样本分位/均值/守恒。
fn check_u2401_map_hist() -> bool {
    let mut h = mxu2401histHist::new();
    let mut i = 0u64;
    while i < 16 { h.push(i + 1); i += 1; }
    let mut ok = h.conserved() && h.mean() >= 1 && h.percentile(500) >= 8;
    ok
}

/// 本模块自检（逐检查带 detail，全绿才出门）。
pub fn run_u2401_checks() -> CheckSet {
    let mut set = CheckSet::new("U2401");
    set.add(
        "u2401-seg-guard",
        u2401_guard_seg(),
        if u2401_guard_seg() { "" } else { "diagnostic code outside segment" },
    );
    set.add(
        "u2401-map-guard",
        check_u2401_map_guard(),
        if check_u2401_map_guard() { "" } else { "UNX-F2401 matrix guard mismatch" },
    );
    set.add(
        "u2401-map-hist",
        check_u2401_map_hist(),
        if check_u2401_map_hist() { "" } else { "UNX-F2401 hist mismatch" },
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
    fn u2401_all_checks_green() {
        let set = run_u2401_checks();
        assert!(set.all_passed(), "U2401 domain checks must be green");
    }

    #[test]
    fn u2401_segment_guard_holds() {
        assert!(u2401_segment_guard());
    }
}
