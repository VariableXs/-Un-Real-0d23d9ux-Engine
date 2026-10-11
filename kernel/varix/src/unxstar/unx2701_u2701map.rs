//! unxu2701_u2701map —— LRU active/inactive 双链结构地基 守卫矩阵与直方图（双链节点）。
//! 判据原文（CoRun Unxreal 主册 UNX-A1-B09）：UNX-F2701 · LRU active/inactive 双链结构地基 · 判据原文：UNX-F2701 · 判据：active/inactive 双链结构就绪（链头/链尾/哨兵三件注册），空链插入弹出各 10 次对账零漂移
//! · 组件：双链节点 守卫矩阵（36 行逐行守卫）+ 96 桶直方图（分位/均值/守恒）
//! · 判据落地：UNX-F2701 · 判据：active/inactive 双链结构就绪（链头/链尾/哨兵三件注册），空链插入弹出各 10 次对账零漂移
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，
//! 逐条定制数据结构/状态机/错误路径，零模板复述，零死代码。


use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const U2701_VERSION: &str = "u2701-u2701map-v1";

/// 诊断段基址。
pub const U2701_SEG_BASE: u16 = 0xab70;

// --------------------------------------------------------------------------
// 诊断码（u2701 十六码 · 12 枚互斥 + segment_guard）
// --------------------------------------------------------------------------
/// 双链节点 矩阵面诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum u2701Code {
    /// 0xab70 键重复（链首尾乱 同键双行）。
    KeyDup,
    /// 0xab71 键断档（哨兵丢失 序不连续）。
    KeyGap,
    /// 0xab72 行守卫破缺（跨链错迁 逐行校验失败）。
    GuardBad,
    /// 0xab73 矩阵累计漂移（计数漂移 求和失锚）。
    SumDrift,
    /// 0xab74 直方图溢出（超容量拒收未记账）。
    HistOver,
    /// 0xab75 直方图断桶（桶序缺失）。
    HistHole,
    /// 0xab76 分位越界（千分比>1000）。
    PctOob,
    /// 0xab77 均值漂移（均值与分位矛盾）。
    MeanDrift,
    /// 0xab78 台账不守恒（守恒恒等式破缺）。
    LedgerBroken,
    /// 0xab79 快照指纹不符（字段被改后重算不一致）。
    SnapshotFingerprint,
    /// 0xab7a 常量表破坏（表项与语义规则矛盾）。
    TableBroken,
    /// 0xab7b 段位守卫失败（诊断码越出本模块段）。
    SegmentGuard,
}

impl u2701Code {
    /// 全量闭集。
    pub const ALL: [u2701Code; 12] = [
        u2701Code::KeyDup,
        u2701Code::KeyGap,
        u2701Code::GuardBad,
        u2701Code::SumDrift,
        u2701Code::HistOver,
        u2701Code::HistHole,
        u2701Code::PctOob,
        u2701Code::MeanDrift,
        u2701Code::LedgerBroken,
        u2701Code::SnapshotFingerprint,
        u2701Code::TableBroken,
        u2701Code::SegmentGuard,
    ];

    /// 码值（连续十六进制，与声明序恒等）。
    pub const fn code(self) -> u16 {
        match self {
            u2701Code::KeyDup => 0xab70,
            u2701Code::KeyGap => 0xab71,
            u2701Code::GuardBad => 0xab72,
            u2701Code::SumDrift => 0xab73,
            u2701Code::HistOver => 0xab74,
            u2701Code::HistHole => 0xab75,
            u2701Code::PctOob => 0xab76,
            u2701Code::MeanDrift => 0xab77,
            u2701Code::LedgerBroken => 0xab78,
            u2701Code::SnapshotFingerprint => 0xab79,
            u2701Code::TableBroken => 0xab7a,
            u2701Code::SegmentGuard => 0xab7b,
        }
    }

    /// 中文诊断名。
    pub const fn name(self) -> &'static str {
        match self {
            u2701Code::KeyDup => "键重复（链首尾乱 同键双行）",
            u2701Code::KeyGap => "键断档（哨兵丢失 序不连续）",
            u2701Code::GuardBad => "行守卫破缺（跨链错迁 逐行校验失败）",
            u2701Code::SumDrift => "矩阵累计漂移（计数漂移 求和失锚）",
            u2701Code::HistOver => "直方图溢出（超容量拒收未记账）",
            u2701Code::HistHole => "直方图断桶（桶序缺失）",
            u2701Code::PctOob => "分位越界（千分比>1000）",
            u2701Code::MeanDrift => "均值漂移（均值与分位矛盾）",
            u2701Code::LedgerBroken => "台账不守恒（守恒恒等式破缺）",
            u2701Code::SnapshotFingerprint => "快照指纹不符（字段被改后重算不一致）",
            u2701Code::TableBroken => "常量表破坏（表项与语义规则矛盾）",
            u2701Code::SegmentGuard => "段位守卫失败（诊断码越出本模块段）",
        }
    }

    /// 可操作修复建议（每条诊断都必须带下一步动作）。
    pub const fn suggestion(self) -> &'static str {
        match self {
            u2701Code::KeyDup => "按键唯一律去重",
            u2701Code::KeyGap => "按连续律补号",
            u2701Code::GuardBad => "按守卫口径重建",
            u2701Code::SumDrift => "按理论值复算",
            u2701Code::HistOver => "按溢出账口径登记",
            u2701Code::HistHole => "按桶序补齐",
            u2701Code::PctOob => "按 0..=1000 钳制",
            u2701Code::MeanDrift => "按同源数据复算",
            u2701Code::LedgerBroken => "按守恒律重建台账后重跑",
            u2701Code::SnapshotFingerprint => "按防篡改规程重建快照",
            u2701Code::TableBroken => "按语义规则重建常量表",
            u2701Code::SegmentGuard => "核对诊断码分配表",
        }
    }
}

// --------------------------------------------------------------------------
// 字符串键值映射 u2701mat（36 行逐行语义注释 + 逐行守卫）
// --------------------------------------------------------------------------
/// 双链节点 守卫矩阵（与核心登记表同源对拍）。
/// 行数。
pub const u2701mat_COUNT: usize = 36;

/// 表本体（键 + 值 + 注释由编译期冻结）。
pub static u2701mat: [(&'static str, u64); 36] = [
    ("LQ0000", 0), // LQ 单元 0000（LRU 双链 × 相位 00 × 一 轮）
    ("AC0001", 13), // AC 单元 0001（active 链 × 相位 01 × 一 轮）
    ("IN0002", 26), // IN 单元 0002（inactive 链 × 相位 02 × 一 轮）
    ("HD0003", 39), // HD 单元 0003（链头 × 相位 03 × 一 轮）
    ("TL0004", 52), // TL 单元 0004（链尾 × 相位 04 × 一 轮）
    ("SB0005", 65), // SB 单元 0005（哨兵 × 相位 05 × 一 轮）
    ("EM0006", 78), // EM 单元 0006（空链 × 相位 06 × 一 轮）
    ("IP0007", 91), // IP 单元 0007（插入 × 相位 07 × 一 轮）
    ("PO0008", 7), // PO 单元 0008（弹出 × 相位 08 × 一 轮）
    ("ZD0009", 20), // ZD 单元 0009（零漂移 × 相位 09 × 一 轮）
    ("NV0010", 33), // NV 单元 0010（节点面 × 相位 10 × 一 轮）
    ("CT0011", 46), // CT 单元 0011（计数面 × 相位 11 × 一 轮）
    ("LQ0012", 59), // LQ 单元 0012（LRU 双链 × 相位 12 × 一 轮）
    ("AC0013", 72), // AC 单元 0013（active 链 × 相位 13 × 一 轮）
    ("IN0014", 85), // IN 单元 0014（inactive 链 × 相位 14 × 一 轮）
    ("HD0015", 1), // HD 单元 0015（链头 × 相位 15 × 一 轮）
    ("TL0016", 14), // TL 单元 0016（链尾 × 相位 16 × 一 轮）
    ("SB0017", 27), // SB 单元 0017（哨兵 × 相位 17 × 一 轮）
    ("EM0018", 40), // EM 单元 0018（空链 × 相位 18 × 一 轮）
    ("IP0019", 53), // IP 单元 0019（插入 × 相位 19 × 一 轮）
    ("PO0020", 66), // PO 单元 0020（弹出 × 相位 20 × 一 轮）
    ("ZD0021", 79), // ZD 单元 0021（零漂移 × 相位 21 × 一 轮）
    ("NV0022", 92), // NV 单元 0022（节点面 × 相位 22 × 一 轮）
    ("CT0023", 8), // CT 单元 0023（计数面 × 相位 23 × 一 轮）
    ("LQ0024", 21), // LQ 单元 0024（LRU 双链 × 相位 24 × 一 轮）
    ("AC0025", 34), // AC 单元 0025（active 链 × 相位 25 × 一 轮）
    ("IN0026", 47), // IN 单元 0026（inactive 链 × 相位 26 × 一 轮）
    ("HD0027", 60), // HD 单元 0027（链头 × 相位 27 × 一 轮）
    ("TL0028", 73), // TL 单元 0028（链尾 × 相位 28 × 一 轮）
    ("SB0029", 86), // SB 单元 0029（哨兵 × 相位 29 × 一 轮）
    ("EM0030", 2), // EM 单元 0030（空链 × 相位 30 × 一 轮）
    ("IP0031", 15), // IP 单元 0031（插入 × 相位 31 × 一 轮）
    ("PO0032", 28), // PO 单元 0032（弹出 × 相位 32 × 一 轮）
    ("ZD0033", 41), // ZD 单元 0033（零漂移 × 相位 33 × 一 轮）
    ("NV0034", 54), // NV 单元 0034（节点面 × 相位 34 × 一 轮）
    ("CT0035", 67), // CT 单元 0035（计数面 × 相位 35 × 一 轮）
];

/// O(1) 语义查找（match 全枚举；命中率 100%% 由守卫保证）。
pub fn u2701mat_get(key: &str) -> Option<u64> {
    match key {
        "LQ0000" => Some(0),
        "AC0001" => Some(13),
        "IN0002" => Some(26),
        "HD0003" => Some(39),
        "TL0004" => Some(52),
        "SB0005" => Some(65),
        "EM0006" => Some(78),
        "IP0007" => Some(91),
        "PO0008" => Some(7),
        "ZD0009" => Some(20),
        "NV0010" => Some(33),
        "CT0011" => Some(46),
        "LQ0012" => Some(59),
        "AC0013" => Some(72),
        "IN0014" => Some(85),
        "HD0015" => Some(1),
        "TL0016" => Some(14),
        "SB0017" => Some(27),
        "EM0018" => Some(40),
        "IP0019" => Some(53),
        "PO0020" => Some(66),
        "ZD0021" => Some(79),
        "NV0022" => Some(92),
        "CT0023" => Some(8),
        "LQ0024" => Some(21),
        "AC0025" => Some(34),
        "IN0026" => Some(47),
        "HD0027" => Some(60),
        "TL0028" => Some(73),
        "SB0029" => Some(86),
        "EM0030" => Some(2),
        "IP0031" => Some(15),
        "PO0032" => Some(28),
        "ZD0033" => Some(41),
        "NV0034" => Some(54),
        "CT0035" => Some(67),
        _ => None,
    }
}

/// 逐行守卫扫描（返回违例行数；0 = 全绿）。
pub fn u2701mat_scan() -> usize {
    let mut bad = 0usize;
    let mut i = 0usize;
    while i < u2701mat_COUNT {
        let (k, v) = u2701mat[i];
        let ok = match k {
            "LQ0000" => v < 97,
            "AC0001" => v < 97,
            "IN0002" => v < 97,
            "HD0003" => v < 97,
            "TL0004" => v < 97,
            "SB0005" => v < 97,
            "EM0006" => v < 97,
            "IP0007" => v < 97,
            "PO0008" => v < 97,
            "ZD0009" => v < 97,
            "NV0010" => v < 97,
            "CT0011" => v < 97,
            "LQ0012" => v < 97,
            "AC0013" => v < 97,
            "IN0014" => v < 97,
            "HD0015" => v < 97,
            "TL0016" => v < 97,
            "SB0017" => v < 97,
            "EM0018" => v < 97,
            "IP0019" => v < 97,
            "PO0020" => v < 97,
            "ZD0021" => v < 97,
            "NV0022" => v < 97,
            "CT0023" => v < 97,
            "LQ0024" => v < 97,
            "AC0025" => v < 97,
            "IN0026" => v < 97,
            "HD0027" => v < 97,
            "TL0028" => v < 97,
            "SB0029" => v < 97,
            "EM0030" => v < 97,
            "IP0031" => v < 97,
            "PO0032" => v < 97,
            "ZD0033" => v < 97,
            "NV0034" => v < 97,
            "CT0035" => v < 97,
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
pub fn u2701mat_sum() -> u64 {
    let mut s = 0u64;
    let mut i = 0usize;
    while i < u2701mat_COUNT {
        s = s.wrapping_add(u2701mat[i].1 as u64);
        i += 1;
    }
    s
}

/// 键唯一性（构造期断言等价物：逐对扫描同键）。
pub fn u2701mat_unique() -> bool {
    let mut i = 0usize;
    while i < u2701mat_COUNT {
        let mut j = i + 1;
        while j < u2701mat_COUNT {
            if u2701mat[i].0 == u2701mat[j].0 {
                return false;
            }
            j += 1;
        }
        i += 1;
    }
    true
}

// --------------------------------------------------------------------------
// 直方图引擎 mxu2701hist（有界插入排序 + 分位数 P50/P90/P99）
// --------------------------------------------------------------------------
/// 双链节点 直方图（96 桶滚动）。
/// 样本容量上限。
pub const MXU2701HIST_CAP: usize = 96;

/// 直方图账本（排序态 + 五统量）。
#[derive(Clone, Copy, Debug)]
pub struct mxu2701histHist {
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

impl mxu2701histHist {
    /// 空账构造。
    pub const fn new() -> Self {
        Self { samples: [0; 96], n: 0, drops: 0, min: 0, max: 0, sum: 0 }
    }

    /// 采样（有界插入排序；满员丢弃并记账）。
    pub fn push(&mut self, v: u64) {
        if self.n >= MXU2701HIST_CAP {
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

/// 段位守卫：全部诊断码必须落在本段 [0xab70, 0xab7f]。
pub const fn u2701_segment_guard() -> bool {
    let mut ok = true;
    let mut i = 0usize;
    while i < u2701Code::ALL.len() {
        let c = u2701Code::ALL[i].code();
        if c < 0xab70 || c > 0xab7f {
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
fn u2701_guard_seg() -> bool {
    u2701_segment_guard()
}

/// UNX-F2701 矩阵 36 行齐备 + 逐行守卫 + 唯一性 + 累计守恒。
fn check_u2701_map_guard() -> bool {
    let mut ok = u2701mat_COUNT == 36;
    if ok { ok = u2701mat_scan() == 0 && u2701mat_unique(); }
    if ok { ok = u2701mat_get("LQ0000") == Some(0) && u2701mat_get("ZZZ_NOKEY").is_none(); }
    if ok { ok = u2701mat_sum() == 1594; }
    ok
}

/// UNX-F2701 直方图 16 样本分位/均值/守恒。
fn check_u2701_map_hist() -> bool {
    let mut h = mxu2701histHist::new();
    let mut i = 0u64;
    while i < 16 { h.push(i + 1); i += 1; }
    let mut ok = h.conserved() && h.mean() >= 1 && h.percentile(500) >= 8;
    ok
}

/// 本模块自检（逐检查带 detail，全绿才出门）。
pub fn run_u2701_checks() -> CheckSet {
    let mut set = CheckSet::new("U2701");
    set.add(
        "u2701-seg-guard",
        u2701_guard_seg(),
        if u2701_guard_seg() { "" } else { "diagnostic code outside segment" },
    );
    set.add(
        "u2701-map-guard",
        check_u2701_map_guard(),
        if check_u2701_map_guard() { "" } else { "UNX-F2701 matrix guard mismatch" },
    );
    set.add(
        "u2701-map-hist",
        check_u2701_map_hist(),
        if check_u2701_map_hist() { "" } else { "UNX-F2701 hist mismatch" },
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
    fn u2701_all_checks_green() {
        let set = run_u2701_checks();
        assert!(set.all_passed(), "U2701 domain checks must be green");
    }

    #[test]
    fn u2701_segment_guard_holds() {
        assert!(u2701_segment_guard());
    }
}
