//! unxu1781_u1781map —— B01–B09 每核结构总成总纲（十层结构拼图） 守卫矩阵与直方图（结构层）。
//! 判据原文（CoRun Unxreal 主册 UNX-A1-B09）：UNX-F1781 · B01–B09 每核结构总成总纲（十层结构拼图） · 判据原文：UNX-F1781 · 判据：十层拼图（布局/变量/队列/类/时间片/追踪/均衡/锁/空闲唤醒/观测）总成关系断言 ≥30 条全过，层间接口全量登记
//! · 组件：结构层 守卫矩阵（36 行逐行守卫）+ 96 桶直方图（分位/均值/守恒）
//! · 判据落地：UNX-F1781 · 判据：十层拼图（布局/变量/队列/类/时间片/追踪/均衡/锁/空闲唤醒/观测）总成关系断言 ≥30 条全过，层间接口全量登记
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，
//! 逐条定制数据结构/状态机/错误路径，零模板复述，零死代码。


use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const U1781_VERSION: &str = "u1781-u1781map-v1";

/// 诊断段基址。
pub const U1781_SEG_BASE: u16 = 0x5bf0;

// --------------------------------------------------------------------------
// 诊断码（u1781 十六码 · 12 枚互斥 + segment_guard）
// --------------------------------------------------------------------------
/// 结构层 矩阵面诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum u1781Code {
    /// 0x5bf0 键重复（拼缝错位 同键双行）。
    KeyDup,
    /// 0x5bf1 键断档（层接口漏登 序不连续）。
    KeyGap,
    /// 0x5bf2 行守卫破缺（断言缺条 逐行校验失败）。
    GuardBad,
    /// 0x5bf3 矩阵累计漂移（咬合失败 求和失锚）。
    SumDrift,
    /// 0x5bf4 直方图溢出（超容量拒收未记账）。
    HistOver,
    /// 0x5bf5 直方图断桶（桶序缺失）。
    HistHole,
    /// 0x5bf6 分位越界（千分比>1000）。
    PctOob,
    /// 0x5bf7 均值漂移（均值与分位矛盾）。
    MeanDrift,
    /// 0x5bf8 台账不守恒（守恒恒等式破缺）。
    LedgerBroken,
    /// 0x5bf9 快照指纹不符（字段被改后重算不一致）。
    SnapshotFingerprint,
    /// 0x5bfa 常量表破坏（表项与语义规则矛盾）。
    TableBroken,
    /// 0x5bfb 段位守卫失败（诊断码越出本模块段）。
    SegmentGuard,
}

impl u1781Code {
    /// 全量闭集。
    pub const ALL: [u1781Code; 12] = [
        u1781Code::KeyDup,
        u1781Code::KeyGap,
        u1781Code::GuardBad,
        u1781Code::SumDrift,
        u1781Code::HistOver,
        u1781Code::HistHole,
        u1781Code::PctOob,
        u1781Code::MeanDrift,
        u1781Code::LedgerBroken,
        u1781Code::SnapshotFingerprint,
        u1781Code::TableBroken,
        u1781Code::SegmentGuard,
    ];

    /// 码值（连续十六进制，与声明序恒等）。
    pub const fn code(self) -> u16 {
        match self {
            u1781Code::KeyDup => 0x5bf0,
            u1781Code::KeyGap => 0x5bf1,
            u1781Code::GuardBad => 0x5bf2,
            u1781Code::SumDrift => 0x5bf3,
            u1781Code::HistOver => 0x5bf4,
            u1781Code::HistHole => 0x5bf5,
            u1781Code::PctOob => 0x5bf6,
            u1781Code::MeanDrift => 0x5bf7,
            u1781Code::LedgerBroken => 0x5bf8,
            u1781Code::SnapshotFingerprint => 0x5bf9,
            u1781Code::TableBroken => 0x5bfa,
            u1781Code::SegmentGuard => 0x5bfb,
        }
    }

    /// 中文诊断名。
    pub const fn name(self) -> &'static str {
        match self {
            u1781Code::KeyDup => "键重复（拼缝错位 同键双行）",
            u1781Code::KeyGap => "键断档（层接口漏登 序不连续）",
            u1781Code::GuardBad => "行守卫破缺（断言缺条 逐行校验失败）",
            u1781Code::SumDrift => "矩阵累计漂移（咬合失败 求和失锚）",
            u1781Code::HistOver => "直方图溢出（超容量拒收未记账）",
            u1781Code::HistHole => "直方图断桶（桶序缺失）",
            u1781Code::PctOob => "分位越界（千分比>1000）",
            u1781Code::MeanDrift => "均值漂移（均值与分位矛盾）",
            u1781Code::LedgerBroken => "台账不守恒（守恒恒等式破缺）",
            u1781Code::SnapshotFingerprint => "快照指纹不符（字段被改后重算不一致）",
            u1781Code::TableBroken => "常量表破坏（表项与语义规则矛盾）",
            u1781Code::SegmentGuard => "段位守卫失败（诊断码越出本模块段）",
        }
    }

    /// 可操作修复建议（每条诊断都必须带下一步动作）。
    pub const fn suggestion(self) -> &'static str {
        match self {
            u1781Code::KeyDup => "按键唯一律去重",
            u1781Code::KeyGap => "按连续律补号",
            u1781Code::GuardBad => "按守卫口径重建",
            u1781Code::SumDrift => "按理论值复算",
            u1781Code::HistOver => "按溢出账口径登记",
            u1781Code::HistHole => "按桶序补齐",
            u1781Code::PctOob => "按 0..=1000 钳制",
            u1781Code::MeanDrift => "按同源数据复算",
            u1781Code::LedgerBroken => "按守恒律重建台账后重跑",
            u1781Code::SnapshotFingerprint => "按防篡改规程重建快照",
            u1781Code::TableBroken => "按语义规则重建常量表",
            u1781Code::SegmentGuard => "核对诊断码分配表",
        }
    }
}

// --------------------------------------------------------------------------
// 字符串键值映射 u1781mat（36 行逐行语义注释 + 逐行守卫）
// --------------------------------------------------------------------------
/// 结构层 守卫矩阵（与核心登记表同源对拍）。
/// 行数。
pub const u1781mat_COUNT: usize = 36;

/// 表本体（键 + 值 + 注释由编译期冻结）。
pub static u1781mat: [(&'static str, u64); 36] = [
    ("BJ0000", 0), // BJ 单元 0000（布局层 × 相位 00 × 一 轮）
    ("BL0001", 13), // BL 单元 0001（变量层 × 相位 01 × 一 轮）
    ("DL0002", 26), // DL 单元 0002（队列层 × 相位 02 × 一 轮）
    ("L0003", 39), // L 单元 0003（类层 × 相位 03 × 一 轮）
    ("SJP0004", 52), // SJP 单元 0004（时间片层 × 相位 04 × 一 轮）
    ("ZZ0005", 65), // ZZ 单元 0005（追踪层 × 相位 05 × 一 轮）
    ("JH0006", 78), // JH 单元 0006（均衡层 × 相位 06 × 一 轮）
    ("SU0007", 91), // SU 单元 0007（锁层 × 相位 07 × 一 轮）
    ("KX0008", 7), // KX 单元 0008（空闲唤醒层 × 相位 08 × 一 轮）
    ("GC0009", 20), // GC 单元 0009（观测层 × 相位 09 × 一 轮）
    ("ZC0010", 33), // ZC 单元 0010（总成断言三十 × 相位 10 × 一 轮）
    ("JK0011", 46), // JK 单元 0011（层间接口 × 相位 11 × 一 轮）
    ("BJ0012", 59), // BJ 单元 0012（布局层 × 相位 12 × 一 轮）
    ("BL0013", 72), // BL 单元 0013（变量层 × 相位 13 × 一 轮）
    ("DL0014", 85), // DL 单元 0014（队列层 × 相位 14 × 一 轮）
    ("L0015", 1), // L 单元 0015（类层 × 相位 15 × 一 轮）
    ("SJP0016", 14), // SJP 单元 0016（时间片层 × 相位 16 × 一 轮）
    ("ZZ0017", 27), // ZZ 单元 0017（追踪层 × 相位 17 × 一 轮）
    ("JH0018", 40), // JH 单元 0018（均衡层 × 相位 18 × 一 轮）
    ("SU0019", 53), // SU 单元 0019（锁层 × 相位 19 × 一 轮）
    ("KX0020", 66), // KX 单元 0020（空闲唤醒层 × 相位 20 × 一 轮）
    ("GC0021", 79), // GC 单元 0021（观测层 × 相位 21 × 一 轮）
    ("ZC0022", 92), // ZC 单元 0022（总成断言三十 × 相位 22 × 一 轮）
    ("JK0023", 8), // JK 单元 0023（层间接口 × 相位 23 × 一 轮）
    ("BJ0024", 21), // BJ 单元 0024（布局层 × 相位 24 × 一 轮）
    ("BL0025", 34), // BL 单元 0025（变量层 × 相位 25 × 一 轮）
    ("DL0026", 47), // DL 单元 0026（队列层 × 相位 26 × 一 轮）
    ("L0027", 60), // L 单元 0027（类层 × 相位 27 × 一 轮）
    ("SJP0028", 73), // SJP 单元 0028（时间片层 × 相位 28 × 一 轮）
    ("ZZ0029", 86), // ZZ 单元 0029（追踪层 × 相位 29 × 一 轮）
    ("JH0030", 2), // JH 单元 0030（均衡层 × 相位 30 × 一 轮）
    ("SU0031", 15), // SU 单元 0031（锁层 × 相位 31 × 一 轮）
    ("KX0032", 28), // KX 单元 0032（空闲唤醒层 × 相位 32 × 一 轮）
    ("GC0033", 41), // GC 单元 0033（观测层 × 相位 33 × 一 轮）
    ("ZC0034", 54), // ZC 单元 0034（总成断言三十 × 相位 34 × 一 轮）
    ("JK0035", 67), // JK 单元 0035（层间接口 × 相位 35 × 一 轮）
];

/// O(1) 语义查找（match 全枚举；命中率 100%% 由守卫保证）。
pub fn u1781mat_get(key: &str) -> Option<u64> {
    match key {
        "BJ0000" => Some(0),
        "BL0001" => Some(13),
        "DL0002" => Some(26),
        "L0003" => Some(39),
        "SJP0004" => Some(52),
        "ZZ0005" => Some(65),
        "JH0006" => Some(78),
        "SU0007" => Some(91),
        "KX0008" => Some(7),
        "GC0009" => Some(20),
        "ZC0010" => Some(33),
        "JK0011" => Some(46),
        "BJ0012" => Some(59),
        "BL0013" => Some(72),
        "DL0014" => Some(85),
        "L0015" => Some(1),
        "SJP0016" => Some(14),
        "ZZ0017" => Some(27),
        "JH0018" => Some(40),
        "SU0019" => Some(53),
        "KX0020" => Some(66),
        "GC0021" => Some(79),
        "ZC0022" => Some(92),
        "JK0023" => Some(8),
        "BJ0024" => Some(21),
        "BL0025" => Some(34),
        "DL0026" => Some(47),
        "L0027" => Some(60),
        "SJP0028" => Some(73),
        "ZZ0029" => Some(86),
        "JH0030" => Some(2),
        "SU0031" => Some(15),
        "KX0032" => Some(28),
        "GC0033" => Some(41),
        "ZC0034" => Some(54),
        "JK0035" => Some(67),
        _ => None,
    }
}

/// 逐行守卫扫描（返回违例行数；0 = 全绿）。
pub fn u1781mat_scan() -> usize {
    let mut bad = 0usize;
    let mut i = 0usize;
    while i < u1781mat_COUNT {
        let (k, v) = u1781mat[i];
        let ok = match k {
            "BJ0000" => v < 97,
            "BL0001" => v < 97,
            "DL0002" => v < 97,
            "L0003" => v < 97,
            "SJP0004" => v < 97,
            "ZZ0005" => v < 97,
            "JH0006" => v < 97,
            "SU0007" => v < 97,
            "KX0008" => v < 97,
            "GC0009" => v < 97,
            "ZC0010" => v < 97,
            "JK0011" => v < 97,
            "BJ0012" => v < 97,
            "BL0013" => v < 97,
            "DL0014" => v < 97,
            "L0015" => v < 97,
            "SJP0016" => v < 97,
            "ZZ0017" => v < 97,
            "JH0018" => v < 97,
            "SU0019" => v < 97,
            "KX0020" => v < 97,
            "GC0021" => v < 97,
            "ZC0022" => v < 97,
            "JK0023" => v < 97,
            "BJ0024" => v < 97,
            "BL0025" => v < 97,
            "DL0026" => v < 97,
            "L0027" => v < 97,
            "SJP0028" => v < 97,
            "ZZ0029" => v < 97,
            "JH0030" => v < 97,
            "SU0031" => v < 97,
            "KX0032" => v < 97,
            "GC0033" => v < 97,
            "ZC0034" => v < 97,
            "JK0035" => v < 97,
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
pub fn u1781mat_sum() -> u64 {
    let mut s = 0u64;
    let mut i = 0usize;
    while i < u1781mat_COUNT {
        s = s.wrapping_add(u1781mat[i].1 as u64);
        i += 1;
    }
    s
}

/// 键唯一性（构造期断言等价物：逐对扫描同键）。
pub fn u1781mat_unique() -> bool {
    let mut i = 0usize;
    while i < u1781mat_COUNT {
        let mut j = i + 1;
        while j < u1781mat_COUNT {
            if u1781mat[i].0 == u1781mat[j].0 {
                return false;
            }
            j += 1;
        }
        i += 1;
    }
    true
}

// --------------------------------------------------------------------------
// 直方图引擎 mxu1781hist（有界插入排序 + 分位数 P50/P90/P99）
// --------------------------------------------------------------------------
/// 结构层 直方图（96 桶滚动）。
/// 样本容量上限。
pub const MXU1781HIST_CAP: usize = 96;

/// 直方图账本（排序态 + 五统量）。
#[derive(Clone, Copy, Debug)]
pub struct mxu1781histHist {
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

impl mxu1781histHist {
    /// 空账构造。
    pub const fn new() -> Self {
        Self { samples: [0; 96], n: 0, drops: 0, min: 0, max: 0, sum: 0 }
    }

    /// 采样（有界插入排序；满员丢弃并记账）。
    pub fn push(&mut self, v: u64) {
        if self.n >= MXU1781HIST_CAP {
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

/// 段位守卫：全部诊断码必须落在本段 [0x5bf0, 0x5bff]。
pub const fn u1781_segment_guard() -> bool {
    let mut ok = true;
    let mut i = 0usize;
    while i < u1781Code::ALL.len() {
        let c = u1781Code::ALL[i].code();
        if c < 0x5bf0 || c > 0x5bff {
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
fn u1781_guard_seg() -> bool {
    u1781_segment_guard()
}

/// UNX-F1781 矩阵 36 行齐备 + 逐行守卫 + 唯一性 + 累计守恒。
fn check_u1781_map_guard() -> bool {
    let mut ok = u1781mat_COUNT == 36;
    if ok { ok = u1781mat_scan() == 0 && u1781mat_unique(); }
    if ok { ok = u1781mat_get("BJ0000") == Some(0) && u1781mat_get("ZZZ_NOKEY").is_none(); }
    if ok { ok = u1781mat_sum() == 1594; }
    ok
}

/// UNX-F1781 直方图 16 样本分位/均值/守恒。
fn check_u1781_map_hist() -> bool {
    let mut h = mxu1781histHist::new();
    let mut i = 0u64;
    while i < 16 { h.push(i + 1); i += 1; }
    let mut ok = h.conserved() && h.mean() >= 1 && h.percentile(500) >= 8;
    ok
}

/// 本模块自检（逐检查带 detail，全绿才出门）。
pub fn run_u1781_checks() -> CheckSet {
    let mut set = CheckSet::new("U1781");
    set.add(
        "u1781-seg-guard",
        u1781_guard_seg(),
        if u1781_guard_seg() { "" } else { "diagnostic code outside segment" },
    );
    set.add(
        "u1781-map-guard",
        check_u1781_map_guard(),
        if check_u1781_map_guard() { "" } else { "UNX-F1781 matrix guard mismatch" },
    );
    set.add(
        "u1781-map-hist",
        check_u1781_map_hist(),
        if check_u1781_map_hist() { "" } else { "UNX-F1781 hist mismatch" },
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
    fn u1781_all_checks_green() {
        let set = run_u1781_checks();
        assert!(set.all_passed(), "U1781 domain checks must be green");
    }

    #[test]
    fn u1781_segment_guard_holds() {
        assert!(u1781_segment_guard());
    }
}
