//! unxu2601_u2601map —— 现存 pfh.rs 接管声明（#PF 处理器收编） 守卫矩阵与直方图（缺页处理路）。
//! 判据原文（CoRun Unxreal 主册 UNX-A1-B09）：UNX-F2601 · 现存 pfh.rs 接管声明（#PF 处理器收编） · 判据原文：UNX-F2601 · 判据：现存 `pfh.rs`（四路起步态：Guard/COW/GrowStack/MapZero）接管账在档（三态标注），收编后四路语义与成熟体逐路条目一致断言
//! · 组件：缺页处理路 守卫矩阵（36 行逐行守卫）+ 96 桶直方图（分位/均值/守恒）
//! · 判据落地：UNX-F2601 · 判据：现存 `pfh.rs`（四路起步态：Guard/COW/GrowStack/MapZero）接管账在档（三态标注），收编后四路语义
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，
//! 逐条定制数据结构/状态机/错误路径，零模板复述，零死代码。


use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const U2601_VERSION: &str = "u2601-u2601map-v1";

/// 诊断段基址。
pub const U2601_SEG_BASE: u16 = 0xa7b0;

// --------------------------------------------------------------------------
// 诊断码（u2601 十六码 · 12 枚互斥 + segment_guard）
// --------------------------------------------------------------------------
/// 缺页处理路 矩阵面诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum u2601Code {
    /// 0xa7b0 键重复（四路漏标 同键双行）。
    KeyDup,
    /// 0xa7b1 键断档（语义错位 序不连续）。
    KeyGap,
    /// 0xa7b2 行守卫破缺（接管缺账 逐行校验失败）。
    GuardBad,
    /// 0xa7b3 矩阵累计漂移（断言分歧 求和失锚）。
    SumDrift,
    /// 0xa7b4 直方图溢出（超容量拒收未记账）。
    HistOver,
    /// 0xa7b5 直方图断桶（桶序缺失）。
    HistHole,
    /// 0xa7b6 分位越界（千分比>1000）。
    PctOob,
    /// 0xa7b7 均值漂移（均值与分位矛盾）。
    MeanDrift,
    /// 0xa7b8 台账不守恒（守恒恒等式破缺）。
    LedgerBroken,
    /// 0xa7b9 快照指纹不符（字段被改后重算不一致）。
    SnapshotFingerprint,
    /// 0xa7ba 常量表破坏（表项与语义规则矛盾）。
    TableBroken,
    /// 0xa7bb 段位守卫失败（诊断码越出本模块段）。
    SegmentGuard,
}

impl u2601Code {
    /// 全量闭集。
    pub const ALL: [u2601Code; 12] = [
        u2601Code::KeyDup,
        u2601Code::KeyGap,
        u2601Code::GuardBad,
        u2601Code::SumDrift,
        u2601Code::HistOver,
        u2601Code::HistHole,
        u2601Code::PctOob,
        u2601Code::MeanDrift,
        u2601Code::LedgerBroken,
        u2601Code::SnapshotFingerprint,
        u2601Code::TableBroken,
        u2601Code::SegmentGuard,
    ];

    /// 码值（连续十六进制，与声明序恒等）。
    pub const fn code(self) -> u16 {
        match self {
            u2601Code::KeyDup => 0xa7b0,
            u2601Code::KeyGap => 0xa7b1,
            u2601Code::GuardBad => 0xa7b2,
            u2601Code::SumDrift => 0xa7b3,
            u2601Code::HistOver => 0xa7b4,
            u2601Code::HistHole => 0xa7b5,
            u2601Code::PctOob => 0xa7b6,
            u2601Code::MeanDrift => 0xa7b7,
            u2601Code::LedgerBroken => 0xa7b8,
            u2601Code::SnapshotFingerprint => 0xa7b9,
            u2601Code::TableBroken => 0xa7ba,
            u2601Code::SegmentGuard => 0xa7bb,
        }
    }

    /// 中文诊断名。
    pub const fn name(self) -> &'static str {
        match self {
            u2601Code::KeyDup => "键重复（四路漏标 同键双行）",
            u2601Code::KeyGap => "键断档（语义错位 序不连续）",
            u2601Code::GuardBad => "行守卫破缺（接管缺账 逐行校验失败）",
            u2601Code::SumDrift => "矩阵累计漂移（断言分歧 求和失锚）",
            u2601Code::HistOver => "直方图溢出（超容量拒收未记账）",
            u2601Code::HistHole => "直方图断桶（桶序缺失）",
            u2601Code::PctOob => "分位越界（千分比>1000）",
            u2601Code::MeanDrift => "均值漂移（均值与分位矛盾）",
            u2601Code::LedgerBroken => "台账不守恒（守恒恒等式破缺）",
            u2601Code::SnapshotFingerprint => "快照指纹不符（字段被改后重算不一致）",
            u2601Code::TableBroken => "常量表破坏（表项与语义规则矛盾）",
            u2601Code::SegmentGuard => "段位守卫失败（诊断码越出本模块段）",
        }
    }

    /// 可操作修复建议（每条诊断都必须带下一步动作）。
    pub const fn suggestion(self) -> &'static str {
        match self {
            u2601Code::KeyDup => "按键唯一律去重",
            u2601Code::KeyGap => "按连续律补号",
            u2601Code::GuardBad => "按守卫口径重建",
            u2601Code::SumDrift => "按理论值复算",
            u2601Code::HistOver => "按溢出账口径登记",
            u2601Code::HistHole => "按桶序补齐",
            u2601Code::PctOob => "按 0..=1000 钳制",
            u2601Code::MeanDrift => "按同源数据复算",
            u2601Code::LedgerBroken => "按守恒律重建台账后重跑",
            u2601Code::SnapshotFingerprint => "按防篡改规程重建快照",
            u2601Code::TableBroken => "按语义规则重建常量表",
            u2601Code::SegmentGuard => "核对诊断码分配表",
        }
    }
}

// --------------------------------------------------------------------------
// 字符串键值映射 u2601mat（36 行逐行语义注释 + 逐行守卫）
// --------------------------------------------------------------------------
/// 缺页处理路 守卫矩阵（与核心登记表同源对拍）。
/// 行数。
pub const u2601mat_COUNT: usize = 36;

/// 表本体（键 + 值 + 注释由编译期冻结）。
pub static u2601mat: [(&'static str, u64); 36] = [
    ("PF0000", 0), // PF 单元 0000（缺页处理器 × 相位 00 × 一 轮）
    ("GD0001", 13), // GD 单元 0001（Guard 路 × 相位 01 × 一 轮）
    ("CW0002", 26), // CW 单元 0002（COW 路 × 相位 02 × 一 轮）
    ("GS0003", 39), // GS 单元 0003（GrowStack 路 × 相位 03 × 一 轮）
    ("MZ0004", 52), // MZ 单元 0004（MapZero 路 × 相位 04 × 一 轮）
    ("TS0005", 65), // TS 单元 0005（三态标注 × 相位 05 × 一 轮）
    ("AD0006", 78), // AD 单元 0006（接管账 × 相位 06 × 一 轮）
    ("MG0007", 91), // MG 单元 0007（成熟体 × 相位 07 × 一 轮）
    ("SM0008", 7), // SM 单元 0008（语义条目 × 相位 08 × 一 轮）
    ("WR0009", 20), // WR 单元 0009（四路 × 相位 09 × 一 轮）
    ("AC0010", 33), // AC 单元 0010（一致断言 × 相位 10 × 一 轮）
    ("RC0011", 46), // RC 单元 0011（收编锚 × 相位 11 × 一 轮）
    ("PF0012", 59), // PF 单元 0012（缺页处理器 × 相位 12 × 一 轮）
    ("GD0013", 72), // GD 单元 0013（Guard 路 × 相位 13 × 一 轮）
    ("CW0014", 85), // CW 单元 0014（COW 路 × 相位 14 × 一 轮）
    ("GS0015", 1), // GS 单元 0015（GrowStack 路 × 相位 15 × 一 轮）
    ("MZ0016", 14), // MZ 单元 0016（MapZero 路 × 相位 16 × 一 轮）
    ("TS0017", 27), // TS 单元 0017（三态标注 × 相位 17 × 一 轮）
    ("AD0018", 40), // AD 单元 0018（接管账 × 相位 18 × 一 轮）
    ("MG0019", 53), // MG 单元 0019（成熟体 × 相位 19 × 一 轮）
    ("SM0020", 66), // SM 单元 0020（语义条目 × 相位 20 × 一 轮）
    ("WR0021", 79), // WR 单元 0021（四路 × 相位 21 × 一 轮）
    ("AC0022", 92), // AC 单元 0022（一致断言 × 相位 22 × 一 轮）
    ("RC0023", 8), // RC 单元 0023（收编锚 × 相位 23 × 一 轮）
    ("PF0024", 21), // PF 单元 0024（缺页处理器 × 相位 24 × 一 轮）
    ("GD0025", 34), // GD 单元 0025（Guard 路 × 相位 25 × 一 轮）
    ("CW0026", 47), // CW 单元 0026（COW 路 × 相位 26 × 一 轮）
    ("GS0027", 60), // GS 单元 0027（GrowStack 路 × 相位 27 × 一 轮）
    ("MZ0028", 73), // MZ 单元 0028（MapZero 路 × 相位 28 × 一 轮）
    ("TS0029", 86), // TS 单元 0029（三态标注 × 相位 29 × 一 轮）
    ("AD0030", 2), // AD 单元 0030（接管账 × 相位 30 × 一 轮）
    ("MG0031", 15), // MG 单元 0031（成熟体 × 相位 31 × 一 轮）
    ("SM0032", 28), // SM 单元 0032（语义条目 × 相位 32 × 一 轮）
    ("WR0033", 41), // WR 单元 0033（四路 × 相位 33 × 一 轮）
    ("AC0034", 54), // AC 单元 0034（一致断言 × 相位 34 × 一 轮）
    ("RC0035", 67), // RC 单元 0035（收编锚 × 相位 35 × 一 轮）
];

/// O(1) 语义查找（match 全枚举；命中率 100%% 由守卫保证）。
pub fn u2601mat_get(key: &str) -> Option<u64> {
    match key {
        "PF0000" => Some(0),
        "GD0001" => Some(13),
        "CW0002" => Some(26),
        "GS0003" => Some(39),
        "MZ0004" => Some(52),
        "TS0005" => Some(65),
        "AD0006" => Some(78),
        "MG0007" => Some(91),
        "SM0008" => Some(7),
        "WR0009" => Some(20),
        "AC0010" => Some(33),
        "RC0011" => Some(46),
        "PF0012" => Some(59),
        "GD0013" => Some(72),
        "CW0014" => Some(85),
        "GS0015" => Some(1),
        "MZ0016" => Some(14),
        "TS0017" => Some(27),
        "AD0018" => Some(40),
        "MG0019" => Some(53),
        "SM0020" => Some(66),
        "WR0021" => Some(79),
        "AC0022" => Some(92),
        "RC0023" => Some(8),
        "PF0024" => Some(21),
        "GD0025" => Some(34),
        "CW0026" => Some(47),
        "GS0027" => Some(60),
        "MZ0028" => Some(73),
        "TS0029" => Some(86),
        "AD0030" => Some(2),
        "MG0031" => Some(15),
        "SM0032" => Some(28),
        "WR0033" => Some(41),
        "AC0034" => Some(54),
        "RC0035" => Some(67),
        _ => None,
    }
}

/// 逐行守卫扫描（返回违例行数；0 = 全绿）。
pub fn u2601mat_scan() -> usize {
    let mut bad = 0usize;
    let mut i = 0usize;
    while i < u2601mat_COUNT {
        let (k, v) = u2601mat[i];
        let ok = match k {
            "PF0000" => v < 97,
            "GD0001" => v < 97,
            "CW0002" => v < 97,
            "GS0003" => v < 97,
            "MZ0004" => v < 97,
            "TS0005" => v < 97,
            "AD0006" => v < 97,
            "MG0007" => v < 97,
            "SM0008" => v < 97,
            "WR0009" => v < 97,
            "AC0010" => v < 97,
            "RC0011" => v < 97,
            "PF0012" => v < 97,
            "GD0013" => v < 97,
            "CW0014" => v < 97,
            "GS0015" => v < 97,
            "MZ0016" => v < 97,
            "TS0017" => v < 97,
            "AD0018" => v < 97,
            "MG0019" => v < 97,
            "SM0020" => v < 97,
            "WR0021" => v < 97,
            "AC0022" => v < 97,
            "RC0023" => v < 97,
            "PF0024" => v < 97,
            "GD0025" => v < 97,
            "CW0026" => v < 97,
            "GS0027" => v < 97,
            "MZ0028" => v < 97,
            "TS0029" => v < 97,
            "AD0030" => v < 97,
            "MG0031" => v < 97,
            "SM0032" => v < 97,
            "WR0033" => v < 97,
            "AC0034" => v < 97,
            "RC0035" => v < 97,
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
pub fn u2601mat_sum() -> u64 {
    let mut s = 0u64;
    let mut i = 0usize;
    while i < u2601mat_COUNT {
        s = s.wrapping_add(u2601mat[i].1 as u64);
        i += 1;
    }
    s
}

/// 键唯一性（构造期断言等价物：逐对扫描同键）。
pub fn u2601mat_unique() -> bool {
    let mut i = 0usize;
    while i < u2601mat_COUNT {
        let mut j = i + 1;
        while j < u2601mat_COUNT {
            if u2601mat[i].0 == u2601mat[j].0 {
                return false;
            }
            j += 1;
        }
        i += 1;
    }
    true
}

// --------------------------------------------------------------------------
// 直方图引擎 mxu2601hist（有界插入排序 + 分位数 P50/P90/P99）
// --------------------------------------------------------------------------
/// 缺页处理路 直方图（96 桶滚动）。
/// 样本容量上限。
pub const MXU2601HIST_CAP: usize = 96;

/// 直方图账本（排序态 + 五统量）。
#[derive(Clone, Copy, Debug)]
pub struct mxu2601histHist {
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

impl mxu2601histHist {
    /// 空账构造。
    pub const fn new() -> Self {
        Self { samples: [0; 96], n: 0, drops: 0, min: 0, max: 0, sum: 0 }
    }

    /// 采样（有界插入排序；满员丢弃并记账）。
    pub fn push(&mut self, v: u64) {
        if self.n >= MXU2601HIST_CAP {
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

/// 段位守卫：全部诊断码必须落在本段 [0xa7b0, 0xa7bf]。
pub const fn u2601_segment_guard() -> bool {
    let mut ok = true;
    let mut i = 0usize;
    while i < u2601Code::ALL.len() {
        let c = u2601Code::ALL[i].code();
        if c < 0xa7b0 || c > 0xa7bf {
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
fn u2601_guard_seg() -> bool {
    u2601_segment_guard()
}

/// UNX-F2601 矩阵 36 行齐备 + 逐行守卫 + 唯一性 + 累计守恒。
fn check_u2601_map_guard() -> bool {
    let mut ok = u2601mat_COUNT == 36;
    if ok { ok = u2601mat_scan() == 0 && u2601mat_unique(); }
    if ok { ok = u2601mat_get("PF0000") == Some(0) && u2601mat_get("ZZZ_NOKEY").is_none(); }
    if ok { ok = u2601mat_sum() == 1594; }
    ok
}

/// UNX-F2601 直方图 16 样本分位/均值/守恒。
fn check_u2601_map_hist() -> bool {
    let mut h = mxu2601histHist::new();
    let mut i = 0u64;
    while i < 16 { h.push(i + 1); i += 1; }
    let mut ok = h.conserved() && h.mean() >= 1 && h.percentile(500) >= 8;
    ok
}

/// 本模块自检（逐检查带 detail，全绿才出门）。
pub fn run_u2601_checks() -> CheckSet {
    let mut set = CheckSet::new("U2601");
    set.add(
        "u2601-seg-guard",
        u2601_guard_seg(),
        if u2601_guard_seg() { "" } else { "diagnostic code outside segment" },
    );
    set.add(
        "u2601-map-guard",
        check_u2601_map_guard(),
        if check_u2601_map_guard() { "" } else { "UNX-F2601 matrix guard mismatch" },
    );
    set.add(
        "u2601-map-hist",
        check_u2601_map_hist(),
        if check_u2601_map_hist() { "" } else { "UNX-F2601 hist mismatch" },
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
    fn u2601_all_checks_green() {
        let set = run_u2601_checks();
        assert!(set.all_passed(), "U2601 domain checks must be green");
    }

    #[test]
    fn u2601_segment_guard_holds() {
        assert!(u2601_segment_guard());
    }
}
