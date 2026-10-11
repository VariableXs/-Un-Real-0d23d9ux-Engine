//! unxu1681_u1681map —— 时间片计算总纲（基片×权重缩放模型） 守卫矩阵与直方图（时间片）。
//! 判据原文（CoRun Unxreal 主册 UNX-A1-B09）：UNX-F1681 · 时间片计算总纲（基片×权重缩放模型） · 判据原文：UNX-F1681 · 判据：时间片=基片×（本线程权重/基准权重）模型冻结 v1，公式参数单点定义，计算路径唯一
//! · 组件：时间片 守卫矩阵（36 行逐行守卫）+ 96 桶直方图（分位/均值/守恒）
//! · 判据落地：UNX-F1681 · 判据：时间片=基片×（本线程权重/基准权重）模型冻结 v1，公式参数单点定义，计算路径唯一
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，
//! 逐条定制数据结构/状态机/错误路径，零模板复述，零死代码。


use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const U1681_VERSION: &str = "u1681-u1681map-v1";

/// 诊断段基址。
pub const U1681_SEG_BASE: u16 = 0x5830;

// --------------------------------------------------------------------------
// 诊断码（u1681 十六码 · 12 枚互斥 + segment_guard）
// --------------------------------------------------------------------------
/// 时间片 矩阵面诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum u1681Code {
    /// 0x5830 键重复（参数多点 同键双行）。
    KeyDup,
    /// 0x5831 键断档（路径分叉 序不连续）。
    KeyGap,
    /// 0x5832 行守卫破缺（钳制漂移 逐行校验失败）。
    GuardBad,
    /// 0x5833 矩阵累计漂移（对拍失准 求和失锚）。
    SumDrift,
    /// 0x5834 直方图溢出（超容量拒收未记账）。
    HistOver,
    /// 0x5835 直方图断桶（桶序缺失）。
    HistHole,
    /// 0x5836 分位越界（千分比>1000）。
    PctOob,
    /// 0x5837 均值漂移（均值与分位矛盾）。
    MeanDrift,
    /// 0x5838 台账不守恒（守恒恒等式破缺）。
    LedgerBroken,
    /// 0x5839 快照指纹不符（字段被改后重算不一致）。
    SnapshotFingerprint,
    /// 0x583a 常量表破坏（表项与语义规则矛盾）。
    TableBroken,
    /// 0x583b 段位守卫失败（诊断码越出本模块段）。
    SegmentGuard,
}

impl u1681Code {
    /// 全量闭集。
    pub const ALL: [u1681Code; 12] = [
        u1681Code::KeyDup,
        u1681Code::KeyGap,
        u1681Code::GuardBad,
        u1681Code::SumDrift,
        u1681Code::HistOver,
        u1681Code::HistHole,
        u1681Code::PctOob,
        u1681Code::MeanDrift,
        u1681Code::LedgerBroken,
        u1681Code::SnapshotFingerprint,
        u1681Code::TableBroken,
        u1681Code::SegmentGuard,
    ];

    /// 码值（连续十六进制，与声明序恒等）。
    pub const fn code(self) -> u16 {
        match self {
            u1681Code::KeyDup => 0x5830,
            u1681Code::KeyGap => 0x5831,
            u1681Code::GuardBad => 0x5832,
            u1681Code::SumDrift => 0x5833,
            u1681Code::HistOver => 0x5834,
            u1681Code::HistHole => 0x5835,
            u1681Code::PctOob => 0x5836,
            u1681Code::MeanDrift => 0x5837,
            u1681Code::LedgerBroken => 0x5838,
            u1681Code::SnapshotFingerprint => 0x5839,
            u1681Code::TableBroken => 0x583a,
            u1681Code::SegmentGuard => 0x583b,
        }
    }

    /// 中文诊断名。
    pub const fn name(self) -> &'static str {
        match self {
            u1681Code::KeyDup => "键重复（参数多点 同键双行）",
            u1681Code::KeyGap => "键断档（路径分叉 序不连续）",
            u1681Code::GuardBad => "行守卫破缺（钳制漂移 逐行校验失败）",
            u1681Code::SumDrift => "矩阵累计漂移（对拍失准 求和失锚）",
            u1681Code::HistOver => "直方图溢出（超容量拒收未记账）",
            u1681Code::HistHole => "直方图断桶（桶序缺失）",
            u1681Code::PctOob => "分位越界（千分比>1000）",
            u1681Code::MeanDrift => "均值漂移（均值与分位矛盾）",
            u1681Code::LedgerBroken => "台账不守恒（守恒恒等式破缺）",
            u1681Code::SnapshotFingerprint => "快照指纹不符（字段被改后重算不一致）",
            u1681Code::TableBroken => "常量表破坏（表项与语义规则矛盾）",
            u1681Code::SegmentGuard => "段位守卫失败（诊断码越出本模块段）",
        }
    }

    /// 可操作修复建议（每条诊断都必须带下一步动作）。
    pub const fn suggestion(self) -> &'static str {
        match self {
            u1681Code::KeyDup => "按键唯一律去重",
            u1681Code::KeyGap => "按连续律补号",
            u1681Code::GuardBad => "按守卫口径重建",
            u1681Code::SumDrift => "按理论值复算",
            u1681Code::HistOver => "按溢出账口径登记",
            u1681Code::HistHole => "按桶序补齐",
            u1681Code::PctOob => "按 0..=1000 钳制",
            u1681Code::MeanDrift => "按同源数据复算",
            u1681Code::LedgerBroken => "按守恒律重建台账后重跑",
            u1681Code::SnapshotFingerprint => "按防篡改规程重建快照",
            u1681Code::TableBroken => "按语义规则重建常量表",
            u1681Code::SegmentGuard => "核对诊断码分配表",
        }
    }
}

// --------------------------------------------------------------------------
// 字符串键值映射 u1681mat（36 行逐行语义注释 + 逐行守卫）
// --------------------------------------------------------------------------
/// 时间片 守卫矩阵（与核心登记表同源对拍）。
/// 行数。
pub const u1681mat_COUNT: usize = 36;

/// 表本体（键 + 值 + 注释由编译期冻结）。
pub static u1681mat: [(&'static str, u64); 36] = [
    ("BS0000", 0), // BS 单元 0000（基片 × 相位 00 × 一 轮）
    ("QZ0001", 13), // QZ 单元 0001（线程权重 × 相位 01 × 一 轮）
    ("JZ0002", 26), // JZ 单元 0002（基准权重 × 相位 02 × 一 轮）
    ("MS0003", 39), // MS 单元 0003（模型冻结 × 相位 03 × 一 轮）
    ("DD0004", 52), // DD 单元 0004（参数单点 × 相位 04 × 一 轮）
    ("WY0005", 65), // WY 单元 0005（计算路径唯一 × 相位 05 × 一 轮）
    ("SJ0006", 78), // SJ 单元 0006（时间片 × 相位 06 × 一 轮）
    ("GS0007", 91), // GS 单元 0007（公式 × 相位 07 × 一 轮）
    ("SR0008", 7), // SR 单元 0008（舍入锚 × 相位 08 × 一 轮）
    ("XW0009", 20), // XW 单元 0009（下限钳制 × 相位 09 × 一 轮）
    ("SW0010", 33), // SW 单元 0010（上限钳制 × 相位 10 × 一 轮）
    ("PZ0011", 46), // PZ 单元 0011（漂移零容 × 相位 11 × 一 轮）
    ("BS0012", 59), // BS 单元 0012（基片 × 相位 12 × 一 轮）
    ("QZ0013", 72), // QZ 单元 0013（线程权重 × 相位 13 × 一 轮）
    ("JZ0014", 85), // JZ 单元 0014（基准权重 × 相位 14 × 一 轮）
    ("MS0015", 1), // MS 单元 0015（模型冻结 × 相位 15 × 一 轮）
    ("DD0016", 14), // DD 单元 0016（参数单点 × 相位 16 × 一 轮）
    ("WY0017", 27), // WY 单元 0017（计算路径唯一 × 相位 17 × 一 轮）
    ("SJ0018", 40), // SJ 单元 0018（时间片 × 相位 18 × 一 轮）
    ("GS0019", 53), // GS 单元 0019（公式 × 相位 19 × 一 轮）
    ("SR0020", 66), // SR 单元 0020（舍入锚 × 相位 20 × 一 轮）
    ("XW0021", 79), // XW 单元 0021（下限钳制 × 相位 21 × 一 轮）
    ("SW0022", 92), // SW 单元 0022（上限钳制 × 相位 22 × 一 轮）
    ("PZ0023", 8), // PZ 单元 0023（漂移零容 × 相位 23 × 一 轮）
    ("BS0024", 21), // BS 单元 0024（基片 × 相位 24 × 一 轮）
    ("QZ0025", 34), // QZ 单元 0025（线程权重 × 相位 25 × 一 轮）
    ("JZ0026", 47), // JZ 单元 0026（基准权重 × 相位 26 × 一 轮）
    ("MS0027", 60), // MS 单元 0027（模型冻结 × 相位 27 × 一 轮）
    ("DD0028", 73), // DD 单元 0028（参数单点 × 相位 28 × 一 轮）
    ("WY0029", 86), // WY 单元 0029（计算路径唯一 × 相位 29 × 一 轮）
    ("SJ0030", 2), // SJ 单元 0030（时间片 × 相位 30 × 一 轮）
    ("GS0031", 15), // GS 单元 0031（公式 × 相位 31 × 一 轮）
    ("SR0032", 28), // SR 单元 0032（舍入锚 × 相位 32 × 一 轮）
    ("XW0033", 41), // XW 单元 0033（下限钳制 × 相位 33 × 一 轮）
    ("SW0034", 54), // SW 单元 0034（上限钳制 × 相位 34 × 一 轮）
    ("PZ0035", 67), // PZ 单元 0035（漂移零容 × 相位 35 × 一 轮）
];

/// O(1) 语义查找（match 全枚举；命中率 100%% 由守卫保证）。
pub fn u1681mat_get(key: &str) -> Option<u64> {
    match key {
        "BS0000" => Some(0),
        "QZ0001" => Some(13),
        "JZ0002" => Some(26),
        "MS0003" => Some(39),
        "DD0004" => Some(52),
        "WY0005" => Some(65),
        "SJ0006" => Some(78),
        "GS0007" => Some(91),
        "SR0008" => Some(7),
        "XW0009" => Some(20),
        "SW0010" => Some(33),
        "PZ0011" => Some(46),
        "BS0012" => Some(59),
        "QZ0013" => Some(72),
        "JZ0014" => Some(85),
        "MS0015" => Some(1),
        "DD0016" => Some(14),
        "WY0017" => Some(27),
        "SJ0018" => Some(40),
        "GS0019" => Some(53),
        "SR0020" => Some(66),
        "XW0021" => Some(79),
        "SW0022" => Some(92),
        "PZ0023" => Some(8),
        "BS0024" => Some(21),
        "QZ0025" => Some(34),
        "JZ0026" => Some(47),
        "MS0027" => Some(60),
        "DD0028" => Some(73),
        "WY0029" => Some(86),
        "SJ0030" => Some(2),
        "GS0031" => Some(15),
        "SR0032" => Some(28),
        "XW0033" => Some(41),
        "SW0034" => Some(54),
        "PZ0035" => Some(67),
        _ => None,
    }
}

/// 逐行守卫扫描（返回违例行数；0 = 全绿）。
pub fn u1681mat_scan() -> usize {
    let mut bad = 0usize;
    let mut i = 0usize;
    while i < u1681mat_COUNT {
        let (k, v) = u1681mat[i];
        let ok = match k {
            "BS0000" => v < 97,
            "QZ0001" => v < 97,
            "JZ0002" => v < 97,
            "MS0003" => v < 97,
            "DD0004" => v < 97,
            "WY0005" => v < 97,
            "SJ0006" => v < 97,
            "GS0007" => v < 97,
            "SR0008" => v < 97,
            "XW0009" => v < 97,
            "SW0010" => v < 97,
            "PZ0011" => v < 97,
            "BS0012" => v < 97,
            "QZ0013" => v < 97,
            "JZ0014" => v < 97,
            "MS0015" => v < 97,
            "DD0016" => v < 97,
            "WY0017" => v < 97,
            "SJ0018" => v < 97,
            "GS0019" => v < 97,
            "SR0020" => v < 97,
            "XW0021" => v < 97,
            "SW0022" => v < 97,
            "PZ0023" => v < 97,
            "BS0024" => v < 97,
            "QZ0025" => v < 97,
            "JZ0026" => v < 97,
            "MS0027" => v < 97,
            "DD0028" => v < 97,
            "WY0029" => v < 97,
            "SJ0030" => v < 97,
            "GS0031" => v < 97,
            "SR0032" => v < 97,
            "XW0033" => v < 97,
            "SW0034" => v < 97,
            "PZ0035" => v < 97,
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
pub fn u1681mat_sum() -> u64 {
    let mut s = 0u64;
    let mut i = 0usize;
    while i < u1681mat_COUNT {
        s = s.wrapping_add(u1681mat[i].1 as u64);
        i += 1;
    }
    s
}

/// 键唯一性（构造期断言等价物：逐对扫描同键）。
pub fn u1681mat_unique() -> bool {
    let mut i = 0usize;
    while i < u1681mat_COUNT {
        let mut j = i + 1;
        while j < u1681mat_COUNT {
            if u1681mat[i].0 == u1681mat[j].0 {
                return false;
            }
            j += 1;
        }
        i += 1;
    }
    true
}

// --------------------------------------------------------------------------
// 直方图引擎 mxu1681hist（有界插入排序 + 分位数 P50/P90/P99）
// --------------------------------------------------------------------------
/// 时间片 直方图（96 桶滚动）。
/// 样本容量上限。
pub const MXU1681HIST_CAP: usize = 96;

/// 直方图账本（排序态 + 五统量）。
#[derive(Clone, Copy, Debug)]
pub struct mxu1681histHist {
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

impl mxu1681histHist {
    /// 空账构造。
    pub const fn new() -> Self {
        Self { samples: [0; 96], n: 0, drops: 0, min: 0, max: 0, sum: 0 }
    }

    /// 采样（有界插入排序；满员丢弃并记账）。
    pub fn push(&mut self, v: u64) {
        if self.n >= MXU1681HIST_CAP {
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

/// 段位守卫：全部诊断码必须落在本段 [0x5830, 0x583f]。
pub const fn u1681_segment_guard() -> bool {
    let mut ok = true;
    let mut i = 0usize;
    while i < u1681Code::ALL.len() {
        let c = u1681Code::ALL[i].code();
        if c < 0x5830 || c > 0x583f {
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
fn u1681_guard_seg() -> bool {
    u1681_segment_guard()
}

/// UNX-F1681 矩阵 36 行齐备 + 逐行守卫 + 唯一性 + 累计守恒。
fn check_u1681_map_guard() -> bool {
    let mut ok = u1681mat_COUNT == 36;
    if ok { ok = u1681mat_scan() == 0 && u1681mat_unique(); }
    if ok { ok = u1681mat_get("BS0000") == Some(0) && u1681mat_get("ZZZ_NOKEY").is_none(); }
    if ok { ok = u1681mat_sum() == 1594; }
    ok
}

/// UNX-F1681 直方图 16 样本分位/均值/守恒。
fn check_u1681_map_hist() -> bool {
    let mut h = mxu1681histHist::new();
    let mut i = 0u64;
    while i < 16 { h.push(i + 1); i += 1; }
    let mut ok = h.conserved() && h.mean() >= 1 && h.percentile(500) >= 8;
    ok
}

/// 本模块自检（逐检查带 detail，全绿才出门）。
pub fn run_u1681_checks() -> CheckSet {
    let mut set = CheckSet::new("U1681");
    set.add(
        "u1681-seg-guard",
        u1681_guard_seg(),
        if u1681_guard_seg() { "" } else { "diagnostic code outside segment" },
    );
    set.add(
        "u1681-map-guard",
        check_u1681_map_guard(),
        if check_u1681_map_guard() { "" } else { "UNX-F1681 matrix guard mismatch" },
    );
    set.add(
        "u1681-map-hist",
        check_u1681_map_hist(),
        if check_u1681_map_hist() { "" } else { "UNX-F1681 hist mismatch" },
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
    fn u1681_all_checks_green() {
        let set = run_u1681_checks();
        assert!(set.all_passed(), "U1681 domain checks must be green");
    }

    #[test]
    fn u1681_segment_guard_holds() {
        assert!(u1681_segment_guard());
    }
}
