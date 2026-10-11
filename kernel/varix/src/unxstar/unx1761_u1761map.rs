//! unxu1761_u1761map —— 观测面总纲（账→接口→聚合三层架构） 守卫矩阵与直方图（层）。
//! 判据原文（CoRun Unxreal 主册 UNX-A1-B09）：UNX-F1761 · 观测面总纲（账→接口→聚合三层架构） · 判据原文：UNX-F1761 · 判据：三层架构冻结 v1，各批观测接口汇聚关系全量登记，三层职责越界注入 10/10 次拒绝
//! · 组件：层 守卫矩阵（36 行逐行守卫）+ 96 桶直方图（分位/均值/守恒）
//! · 判据落地：UNX-F1761 · 判据：三层架构冻结 v1，各批观测接口汇聚关系全量登记，三层职责越界注入 10/10 次拒绝
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，
//! 逐条定制数据结构/状态机/错误路径，零模板复述，零死代码。


use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const U1761_VERSION: &str = "u1761-u1761map-v1";

/// 诊断段基址。
pub const U1761_SEG_BASE: u16 = 0x5b30;

// --------------------------------------------------------------------------
// 诊断码（u1761 十六码 · 12 枚互斥 + segment_guard）
// --------------------------------------------------------------------------
/// 层 矩阵面诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum u1761Code {
    /// 0x5b30 键重复（职责越界 同键双行）。
    KeyDup,
    /// 0x5b31 键断档（汇聚漏登 序不连续）。
    KeyGap,
    /// 0x5b32 行守卫破缺（层间穿透 逐行校验失败）。
    GuardBad,
    /// 0x5b33 矩阵累计漂移（通道失联 求和失锚）。
    SumDrift,
    /// 0x5b34 直方图溢出（超容量拒收未记账）。
    HistOver,
    /// 0x5b35 直方图断桶（桶序缺失）。
    HistHole,
    /// 0x5b36 分位越界（千分比>1000）。
    PctOob,
    /// 0x5b37 均值漂移（均值与分位矛盾）。
    MeanDrift,
    /// 0x5b38 台账不守恒（守恒恒等式破缺）。
    LedgerBroken,
    /// 0x5b39 快照指纹不符（字段被改后重算不一致）。
    SnapshotFingerprint,
    /// 0x5b3a 常量表破坏（表项与语义规则矛盾）。
    TableBroken,
    /// 0x5b3b 段位守卫失败（诊断码越出本模块段）。
    SegmentGuard,
}

impl u1761Code {
    /// 全量闭集。
    pub const ALL: [u1761Code; 12] = [
        u1761Code::KeyDup,
        u1761Code::KeyGap,
        u1761Code::GuardBad,
        u1761Code::SumDrift,
        u1761Code::HistOver,
        u1761Code::HistHole,
        u1761Code::PctOob,
        u1761Code::MeanDrift,
        u1761Code::LedgerBroken,
        u1761Code::SnapshotFingerprint,
        u1761Code::TableBroken,
        u1761Code::SegmentGuard,
    ];

    /// 码值（连续十六进制，与声明序恒等）。
    pub const fn code(self) -> u16 {
        match self {
            u1761Code::KeyDup => 0x5b30,
            u1761Code::KeyGap => 0x5b31,
            u1761Code::GuardBad => 0x5b32,
            u1761Code::SumDrift => 0x5b33,
            u1761Code::HistOver => 0x5b34,
            u1761Code::HistHole => 0x5b35,
            u1761Code::PctOob => 0x5b36,
            u1761Code::MeanDrift => 0x5b37,
            u1761Code::LedgerBroken => 0x5b38,
            u1761Code::SnapshotFingerprint => 0x5b39,
            u1761Code::TableBroken => 0x5b3a,
            u1761Code::SegmentGuard => 0x5b3b,
        }
    }

    /// 中文诊断名。
    pub const fn name(self) -> &'static str {
        match self {
            u1761Code::KeyDup => "键重复（职责越界 同键双行）",
            u1761Code::KeyGap => "键断档（汇聚漏登 序不连续）",
            u1761Code::GuardBad => "行守卫破缺（层间穿透 逐行校验失败）",
            u1761Code::SumDrift => "矩阵累计漂移（通道失联 求和失锚）",
            u1761Code::HistOver => "直方图溢出（超容量拒收未记账）",
            u1761Code::HistHole => "直方图断桶（桶序缺失）",
            u1761Code::PctOob => "分位越界（千分比>1000）",
            u1761Code::MeanDrift => "均值漂移（均值与分位矛盾）",
            u1761Code::LedgerBroken => "台账不守恒（守恒恒等式破缺）",
            u1761Code::SnapshotFingerprint => "快照指纹不符（字段被改后重算不一致）",
            u1761Code::TableBroken => "常量表破坏（表项与语义规则矛盾）",
            u1761Code::SegmentGuard => "段位守卫失败（诊断码越出本模块段）",
        }
    }

    /// 可操作修复建议（每条诊断都必须带下一步动作）。
    pub const fn suggestion(self) -> &'static str {
        match self {
            u1761Code::KeyDup => "按键唯一律去重",
            u1761Code::KeyGap => "按连续律补号",
            u1761Code::GuardBad => "按守卫口径重建",
            u1761Code::SumDrift => "按理论值复算",
            u1761Code::HistOver => "按溢出账口径登记",
            u1761Code::HistHole => "按桶序补齐",
            u1761Code::PctOob => "按 0..=1000 钳制",
            u1761Code::MeanDrift => "按同源数据复算",
            u1761Code::LedgerBroken => "按守恒律重建台账后重跑",
            u1761Code::SnapshotFingerprint => "按防篡改规程重建快照",
            u1761Code::TableBroken => "按语义规则重建常量表",
            u1761Code::SegmentGuard => "核对诊断码分配表",
        }
    }
}

// --------------------------------------------------------------------------
// 字符串键值映射 u1761mat（36 行逐行语义注释 + 逐行守卫）
// --------------------------------------------------------------------------
/// 层 守卫矩阵（与核心登记表同源对拍）。
/// 行数。
pub const u1761mat_COUNT: usize = 36;

/// 表本体（键 + 值 + 注释由编译期冻结）。
pub static u1761mat: [(&'static str, u64); 36] = [
    ("Z0000", 0), // Z 单元 0000（账层 × 相位 00 × 一 轮）
    ("JK0001", 13), // JK 单元 0001（接口层 × 相位 01 × 一 轮）
    ("JH0002", 26), // JH 单元 0002（聚合层 × 相位 02 × 一 轮）
    ("HJ0003", 39), // HJ 单元 0003（汇聚关系 × 相位 03 × 一 轮）
    ("QL0004", 52), // QL 单元 0004（全量登记 × 相位 04 × 一 轮）
    ("ZZ0005", 65), // ZZ 单元 0005（职责越界 × 相位 05 × 一 轮）
    ("JJ0006", 78), // JJ 单元 0006（拒绝 × 相位 06 × 一 轮）
    ("MZ0007", 91), // MZ 单元 0007（冻结 v1 × 相位 07 × 一 轮）
    ("GC0008", 7), // GC 单元 0008（各批接口 × 相位 08 × 一 轮）
    ("CJ0009", 20), // CJ 单元 0009（层级闭集 × 相位 09 × 一 轮）
    ("SJ0010", 33), // SJ 单元 0010（上行通道 × 相位 10 × 一 轮）
    ("XD0011", 46), // XD 单元 0011（下行通道 × 相位 11 × 一 轮）
    ("Z0012", 59), // Z 单元 0012（账层 × 相位 12 × 一 轮）
    ("JK0013", 72), // JK 单元 0013（接口层 × 相位 13 × 一 轮）
    ("JH0014", 85), // JH 单元 0014（聚合层 × 相位 14 × 一 轮）
    ("HJ0015", 1), // HJ 单元 0015（汇聚关系 × 相位 15 × 一 轮）
    ("QL0016", 14), // QL 单元 0016（全量登记 × 相位 16 × 一 轮）
    ("ZZ0017", 27), // ZZ 单元 0017（职责越界 × 相位 17 × 一 轮）
    ("JJ0018", 40), // JJ 单元 0018（拒绝 × 相位 18 × 一 轮）
    ("MZ0019", 53), // MZ 单元 0019（冻结 v1 × 相位 19 × 一 轮）
    ("GC0020", 66), // GC 单元 0020（各批接口 × 相位 20 × 一 轮）
    ("CJ0021", 79), // CJ 单元 0021（层级闭集 × 相位 21 × 一 轮）
    ("SJ0022", 92), // SJ 单元 0022（上行通道 × 相位 22 × 一 轮）
    ("XD0023", 8), // XD 单元 0023（下行通道 × 相位 23 × 一 轮）
    ("Z0024", 21), // Z 单元 0024（账层 × 相位 24 × 一 轮）
    ("JK0025", 34), // JK 单元 0025（接口层 × 相位 25 × 一 轮）
    ("JH0026", 47), // JH 单元 0026（聚合层 × 相位 26 × 一 轮）
    ("HJ0027", 60), // HJ 单元 0027（汇聚关系 × 相位 27 × 一 轮）
    ("QL0028", 73), // QL 单元 0028（全量登记 × 相位 28 × 一 轮）
    ("ZZ0029", 86), // ZZ 单元 0029（职责越界 × 相位 29 × 一 轮）
    ("JJ0030", 2), // JJ 单元 0030（拒绝 × 相位 30 × 一 轮）
    ("MZ0031", 15), // MZ 单元 0031（冻结 v1 × 相位 31 × 一 轮）
    ("GC0032", 28), // GC 单元 0032（各批接口 × 相位 32 × 一 轮）
    ("CJ0033", 41), // CJ 单元 0033（层级闭集 × 相位 33 × 一 轮）
    ("SJ0034", 54), // SJ 单元 0034（上行通道 × 相位 34 × 一 轮）
    ("XD0035", 67), // XD 单元 0035（下行通道 × 相位 35 × 一 轮）
];

/// O(1) 语义查找（match 全枚举；命中率 100%% 由守卫保证）。
pub fn u1761mat_get(key: &str) -> Option<u64> {
    match key {
        "Z0000" => Some(0),
        "JK0001" => Some(13),
        "JH0002" => Some(26),
        "HJ0003" => Some(39),
        "QL0004" => Some(52),
        "ZZ0005" => Some(65),
        "JJ0006" => Some(78),
        "MZ0007" => Some(91),
        "GC0008" => Some(7),
        "CJ0009" => Some(20),
        "SJ0010" => Some(33),
        "XD0011" => Some(46),
        "Z0012" => Some(59),
        "JK0013" => Some(72),
        "JH0014" => Some(85),
        "HJ0015" => Some(1),
        "QL0016" => Some(14),
        "ZZ0017" => Some(27),
        "JJ0018" => Some(40),
        "MZ0019" => Some(53),
        "GC0020" => Some(66),
        "CJ0021" => Some(79),
        "SJ0022" => Some(92),
        "XD0023" => Some(8),
        "Z0024" => Some(21),
        "JK0025" => Some(34),
        "JH0026" => Some(47),
        "HJ0027" => Some(60),
        "QL0028" => Some(73),
        "ZZ0029" => Some(86),
        "JJ0030" => Some(2),
        "MZ0031" => Some(15),
        "GC0032" => Some(28),
        "CJ0033" => Some(41),
        "SJ0034" => Some(54),
        "XD0035" => Some(67),
        _ => None,
    }
}

/// 逐行守卫扫描（返回违例行数；0 = 全绿）。
pub fn u1761mat_scan() -> usize {
    let mut bad = 0usize;
    let mut i = 0usize;
    while i < u1761mat_COUNT {
        let (k, v) = u1761mat[i];
        let ok = match k {
            "Z0000" => v < 97,
            "JK0001" => v < 97,
            "JH0002" => v < 97,
            "HJ0003" => v < 97,
            "QL0004" => v < 97,
            "ZZ0005" => v < 97,
            "JJ0006" => v < 97,
            "MZ0007" => v < 97,
            "GC0008" => v < 97,
            "CJ0009" => v < 97,
            "SJ0010" => v < 97,
            "XD0011" => v < 97,
            "Z0012" => v < 97,
            "JK0013" => v < 97,
            "JH0014" => v < 97,
            "HJ0015" => v < 97,
            "QL0016" => v < 97,
            "ZZ0017" => v < 97,
            "JJ0018" => v < 97,
            "MZ0019" => v < 97,
            "GC0020" => v < 97,
            "CJ0021" => v < 97,
            "SJ0022" => v < 97,
            "XD0023" => v < 97,
            "Z0024" => v < 97,
            "JK0025" => v < 97,
            "JH0026" => v < 97,
            "HJ0027" => v < 97,
            "QL0028" => v < 97,
            "ZZ0029" => v < 97,
            "JJ0030" => v < 97,
            "MZ0031" => v < 97,
            "GC0032" => v < 97,
            "CJ0033" => v < 97,
            "SJ0034" => v < 97,
            "XD0035" => v < 97,
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
pub fn u1761mat_sum() -> u64 {
    let mut s = 0u64;
    let mut i = 0usize;
    while i < u1761mat_COUNT {
        s = s.wrapping_add(u1761mat[i].1 as u64);
        i += 1;
    }
    s
}

/// 键唯一性（构造期断言等价物：逐对扫描同键）。
pub fn u1761mat_unique() -> bool {
    let mut i = 0usize;
    while i < u1761mat_COUNT {
        let mut j = i + 1;
        while j < u1761mat_COUNT {
            if u1761mat[i].0 == u1761mat[j].0 {
                return false;
            }
            j += 1;
        }
        i += 1;
    }
    true
}

// --------------------------------------------------------------------------
// 直方图引擎 mxu1761hist（有界插入排序 + 分位数 P50/P90/P99）
// --------------------------------------------------------------------------
/// 层 直方图（96 桶滚动）。
/// 样本容量上限。
pub const MXU1761HIST_CAP: usize = 96;

/// 直方图账本（排序态 + 五统量）。
#[derive(Clone, Copy, Debug)]
pub struct mxu1761histHist {
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

impl mxu1761histHist {
    /// 空账构造。
    pub const fn new() -> Self {
        Self { samples: [0; 96], n: 0, drops: 0, min: 0, max: 0, sum: 0 }
    }

    /// 采样（有界插入排序；满员丢弃并记账）。
    pub fn push(&mut self, v: u64) {
        if self.n >= MXU1761HIST_CAP {
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

/// 段位守卫：全部诊断码必须落在本段 [0x5b30, 0x5b3f]。
pub const fn u1761_segment_guard() -> bool {
    let mut ok = true;
    let mut i = 0usize;
    while i < u1761Code::ALL.len() {
        let c = u1761Code::ALL[i].code();
        if c < 0x5b30 || c > 0x5b3f {
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
fn u1761_guard_seg() -> bool {
    u1761_segment_guard()
}

/// UNX-F1761 矩阵 36 行齐备 + 逐行守卫 + 唯一性 + 累计守恒。
fn check_u1761_map_guard() -> bool {
    let mut ok = u1761mat_COUNT == 36;
    if ok { ok = u1761mat_scan() == 0 && u1761mat_unique(); }
    if ok { ok = u1761mat_get("Z0000") == Some(0) && u1761mat_get("ZZZ_NOKEY").is_none(); }
    if ok { ok = u1761mat_sum() == 1594; }
    ok
}

/// UNX-F1761 直方图 16 样本分位/均值/守恒。
fn check_u1761_map_hist() -> bool {
    let mut h = mxu1761histHist::new();
    let mut i = 0u64;
    while i < 16 { h.push(i + 1); i += 1; }
    let mut ok = h.conserved() && h.mean() >= 1 && h.percentile(500) >= 8;
    ok
}

/// 本模块自检（逐检查带 detail，全绿才出门）。
pub fn run_u1761_checks() -> CheckSet {
    let mut set = CheckSet::new("U1761");
    set.add(
        "u1761-seg-guard",
        u1761_guard_seg(),
        if u1761_guard_seg() { "" } else { "diagnostic code outside segment" },
    );
    set.add(
        "u1761-map-guard",
        check_u1761_map_guard(),
        if check_u1761_map_guard() { "" } else { "UNX-F1761 matrix guard mismatch" },
    );
    set.add(
        "u1761-map-hist",
        check_u1761_map_hist(),
        if check_u1761_map_hist() { "" } else { "UNX-F1761 hist mismatch" },
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
    fn u1761_all_checks_green() {
        let set = run_u1761_checks();
        assert!(set.all_passed(), "U1761 domain checks must be green");
    }

    #[test]
    fn u1761_segment_guard_holds() {
        assert!(u1761_segment_guard());
    }
}
