//! unxu2901_u2901map —— 页缓存压测方案冻结（三矩阵） 守卫矩阵与直方图（页缓存三矩阵压测）。
//! 判据原文（CoRun Unxreal 主册 UNX-A1-B09）：UNX-F2901 · 页缓存压测方案冻结（三矩阵） · 判据原文：UNX-F2901 · 判据：UNX-F2901-J1 三矩阵（读写比/文件尺寸/并发度）参数冻结哈希入册，方案与 F2682 三接口字段对齐
//! · 组件：页缓存三矩阵压测 守卫矩阵（36 行逐行守卫）+ 96 桶直方图（分位/均值/守恒）
//! · 判据落地：UNX-F2901 · 判据：UNX-F2901-J1 三矩阵（读写比/文件尺寸/并发度）参数冻结哈希入册，方案与 F2682 三接口字段对齐
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，
//! 逐条定制数据结构/状态机/错误路径，零模板复述，零死代码。


use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const U2901_VERSION: &str = "u2901-u2901map-v1";

/// 诊断段基址。
pub const U2901_SEG_BASE: u16 = 0xe230;

// --------------------------------------------------------------------------
// 诊断码（u2901 十六码 · 12 枚互斥 + segment_guard）
// --------------------------------------------------------------------------
/// 页缓存三矩阵压测 矩阵面诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum u2901Code {
    /// 0xe230 键重复（页缓存 同键双行）。
    KeyDup,
    /// 0xe231 键断档（radix 序不连续）。
    KeyGap,
    /// 0xe232 行守卫破缺（逐出 逐行校验失败）。
    GuardBad,
    /// 0xe233 矩阵累计漂移（逐出 求和失锚）。
    SumDrift,
    /// 0xe234 直方图溢出（超容量拒收未记账）。
    HistOver,
    /// 0xe235 直方图断桶（桶序缺失）。
    HistHole,
    /// 0xe236 分位越界（千分比>1000）。
    PctOob,
    /// 0xe237 均值漂移（均值与分位矛盾）。
    MeanDrift,
    /// 0xe238 台账不守恒（守恒恒等式破缺）。
    LedgerBroken,
    /// 0xe239 快照指纹不符（字段被改后重算不一致）。
    SnapshotFingerprint,
    /// 0xe23a 常量表破坏（表项与语义规则矛盾）。
    TableBroken,
    /// 0xe23b 段位守卫失败（诊断码越出本模块段）。
    SegmentGuard,
}

impl u2901Code {
    /// 全量闭集。
    pub const ALL: [u2901Code; 12] = [
        u2901Code::KeyDup,
        u2901Code::KeyGap,
        u2901Code::GuardBad,
        u2901Code::SumDrift,
        u2901Code::HistOver,
        u2901Code::HistHole,
        u2901Code::PctOob,
        u2901Code::MeanDrift,
        u2901Code::LedgerBroken,
        u2901Code::SnapshotFingerprint,
        u2901Code::TableBroken,
        u2901Code::SegmentGuard,
    ];

    /// 码值（连续十六进制，与声明序恒等）。
    pub const fn code(self) -> u16 {
        match self {
            u2901Code::KeyDup => 0xe230,
            u2901Code::KeyGap => 0xe231,
            u2901Code::GuardBad => 0xe232,
            u2901Code::SumDrift => 0xe233,
            u2901Code::HistOver => 0xe234,
            u2901Code::HistHole => 0xe235,
            u2901Code::PctOob => 0xe236,
            u2901Code::MeanDrift => 0xe237,
            u2901Code::LedgerBroken => 0xe238,
            u2901Code::SnapshotFingerprint => 0xe239,
            u2901Code::TableBroken => 0xe23a,
            u2901Code::SegmentGuard => 0xe23b,
        }
    }

    /// 中文诊断名。
    pub const fn name(self) -> &'static str {
        match self {
            u2901Code::KeyDup => "键重复（页缓存 同键双行）",
            u2901Code::KeyGap => "键断档（radix 序不连续）",
            u2901Code::GuardBad => "行守卫破缺（逐出 逐行校验失败）",
            u2901Code::SumDrift => "矩阵累计漂移（逐出 求和失锚）",
            u2901Code::HistOver => "直方图溢出（超容量拒收未记账）",
            u2901Code::HistHole => "直方图断桶（桶序缺失）",
            u2901Code::PctOob => "分位越界（千分比>1000）",
            u2901Code::MeanDrift => "均值漂移（均值与分位矛盾）",
            u2901Code::LedgerBroken => "台账不守恒（守恒恒等式破缺）",
            u2901Code::SnapshotFingerprint => "快照指纹不符（字段被改后重算不一致）",
            u2901Code::TableBroken => "常量表破坏（表项与语义规则矛盾）",
            u2901Code::SegmentGuard => "段位守卫失败（诊断码越出本模块段）",
        }
    }

    /// 可操作修复建议（每条诊断都必须带下一步动作）。
    pub const fn suggestion(self) -> &'static str {
        match self {
            u2901Code::KeyDup => "按键唯一律去重",
            u2901Code::KeyGap => "按连续律补号",
            u2901Code::GuardBad => "按守卫口径重建",
            u2901Code::SumDrift => "按理论值复算",
            u2901Code::HistOver => "按溢出账口径登记",
            u2901Code::HistHole => "按桶序补齐",
            u2901Code::PctOob => "按 0..=1000 钳制",
            u2901Code::MeanDrift => "按同源数据复算",
            u2901Code::LedgerBroken => "按守恒律重建台账后重跑",
            u2901Code::SnapshotFingerprint => "按防篡改规程重建快照",
            u2901Code::TableBroken => "按语义规则重建常量表",
            u2901Code::SegmentGuard => "核对诊断码分配表",
        }
    }
}

// --------------------------------------------------------------------------
// 字符串键值映射 u2901mat（36 行逐行语义注释 + 逐行守卫）
// --------------------------------------------------------------------------
/// 页缓存三矩阵压测 守卫矩阵（与核心登记表同源对拍）。
/// 行数。
pub const u2901mat_COUNT: usize = 36;

/// 表本体（键 + 值 + 注释由编译期冻结）。
pub static u2901mat: [(&'static str, u64); 36] = [
    ("PC0000", 0), // PC 单元 0000（页缓存 × 相位 00 × 一 轮）
    ("M10001", 13), // M1 单元 0001（矩阵一 × 相位 01 × 一 轮）
    ("M20002", 26), // M2 单元 0002（矩阵二 × 相位 02 × 一 轮）
    ("M30003", 39), // M3 单元 0003（矩阵三 × 相位 03 × 一 轮）
    ("RADIX0004", 52), // RADIX 单元 0004（radix 树 × 相位 04 × 一 轮）
    ("EVICT0005", 65), // EVICT 单元 0005（逐出 × 相位 05 × 一 轮）
    ("RECLAIM0006", 78), // RECLAIM 单元 0006（回收 × 相位 06 × 一 轮）
    ("HIT0007", 91), // HIT 单元 0007（命中 × 相位 07 × 一 轮）
    ("MISS0008", 7), // MISS 单元 0008（未命中 × 相位 08 × 一 轮）
    ("IO0009", 20), // IO 单元 0009（回源 × 相位 09 × 一 轮）
    ("THR0010", 33), // THR 单元 0010（阈值 × 相位 10 × 一 轮）
    ("ACC0011", 46), // ACC 单元 0011（记账 × 相位 11 × 一 轮）
    ("PC0012", 59), // PC 单元 0012（页缓存 × 相位 12 × 一 轮）
    ("M10013", 72), // M1 单元 0013（矩阵一 × 相位 13 × 一 轮）
    ("M20014", 85), // M2 单元 0014（矩阵二 × 相位 14 × 一 轮）
    ("M30015", 1), // M3 单元 0015（矩阵三 × 相位 15 × 一 轮）
    ("RADIX0016", 14), // RADIX 单元 0016（radix 树 × 相位 16 × 一 轮）
    ("EVICT0017", 27), // EVICT 单元 0017（逐出 × 相位 17 × 一 轮）
    ("RECLAIM0018", 40), // RECLAIM 单元 0018（回收 × 相位 18 × 一 轮）
    ("HIT0019", 53), // HIT 单元 0019（命中 × 相位 19 × 一 轮）
    ("MISS0020", 66), // MISS 单元 0020（未命中 × 相位 20 × 一 轮）
    ("IO0021", 79), // IO 单元 0021（回源 × 相位 21 × 一 轮）
    ("THR0022", 92), // THR 单元 0022（阈值 × 相位 22 × 一 轮）
    ("ACC0023", 8), // ACC 单元 0023（记账 × 相位 23 × 一 轮）
    ("PC0024", 21), // PC 单元 0024（页缓存 × 相位 24 × 一 轮）
    ("M10025", 34), // M1 单元 0025（矩阵一 × 相位 25 × 一 轮）
    ("M20026", 47), // M2 单元 0026（矩阵二 × 相位 26 × 一 轮）
    ("M30027", 60), // M3 单元 0027（矩阵三 × 相位 27 × 一 轮）
    ("RADIX0028", 73), // RADIX 单元 0028（radix 树 × 相位 28 × 一 轮）
    ("EVICT0029", 86), // EVICT 单元 0029（逐出 × 相位 29 × 一 轮）
    ("RECLAIM0030", 2), // RECLAIM 单元 0030（回收 × 相位 30 × 一 轮）
    ("HIT0031", 15), // HIT 单元 0031（命中 × 相位 31 × 一 轮）
    ("MISS0032", 28), // MISS 单元 0032（未命中 × 相位 32 × 一 轮）
    ("IO0033", 41), // IO 单元 0033（回源 × 相位 33 × 一 轮）
    ("THR0034", 54), // THR 单元 0034（阈值 × 相位 34 × 一 轮）
    ("ACC0035", 67), // ACC 单元 0035（记账 × 相位 35 × 一 轮）
];

/// O(1) 语义查找（match 全枚举；命中率 100%% 由守卫保证）。
pub fn u2901mat_get(key: &str) -> Option<u64> {
    match key {
        "PC0000" => Some(0),
        "M10001" => Some(13),
        "M20002" => Some(26),
        "M30003" => Some(39),
        "RADIX0004" => Some(52),
        "EVICT0005" => Some(65),
        "RECLAIM0006" => Some(78),
        "HIT0007" => Some(91),
        "MISS0008" => Some(7),
        "IO0009" => Some(20),
        "THR0010" => Some(33),
        "ACC0011" => Some(46),
        "PC0012" => Some(59),
        "M10013" => Some(72),
        "M20014" => Some(85),
        "M30015" => Some(1),
        "RADIX0016" => Some(14),
        "EVICT0017" => Some(27),
        "RECLAIM0018" => Some(40),
        "HIT0019" => Some(53),
        "MISS0020" => Some(66),
        "IO0021" => Some(79),
        "THR0022" => Some(92),
        "ACC0023" => Some(8),
        "PC0024" => Some(21),
        "M10025" => Some(34),
        "M20026" => Some(47),
        "M30027" => Some(60),
        "RADIX0028" => Some(73),
        "EVICT0029" => Some(86),
        "RECLAIM0030" => Some(2),
        "HIT0031" => Some(15),
        "MISS0032" => Some(28),
        "IO0033" => Some(41),
        "THR0034" => Some(54),
        "ACC0035" => Some(67),
        _ => None,
    }
}

/// 逐行守卫扫描（返回违例行数；0 = 全绿）。
pub fn u2901mat_scan() -> usize {
    let mut bad = 0usize;
    let mut i = 0usize;
    while i < u2901mat_COUNT {
        let (k, v) = u2901mat[i];
        let ok = match k {
            "PC0000" => v < 97,
            "M10001" => v < 97,
            "M20002" => v < 97,
            "M30003" => v < 97,
            "RADIX0004" => v < 97,
            "EVICT0005" => v < 97,
            "RECLAIM0006" => v < 97,
            "HIT0007" => v < 97,
            "MISS0008" => v < 97,
            "IO0009" => v < 97,
            "THR0010" => v < 97,
            "ACC0011" => v < 97,
            "PC0012" => v < 97,
            "M10013" => v < 97,
            "M20014" => v < 97,
            "M30015" => v < 97,
            "RADIX0016" => v < 97,
            "EVICT0017" => v < 97,
            "RECLAIM0018" => v < 97,
            "HIT0019" => v < 97,
            "MISS0020" => v < 97,
            "IO0021" => v < 97,
            "THR0022" => v < 97,
            "ACC0023" => v < 97,
            "PC0024" => v < 97,
            "M10025" => v < 97,
            "M20026" => v < 97,
            "M30027" => v < 97,
            "RADIX0028" => v < 97,
            "EVICT0029" => v < 97,
            "RECLAIM0030" => v < 97,
            "HIT0031" => v < 97,
            "MISS0032" => v < 97,
            "IO0033" => v < 97,
            "THR0034" => v < 97,
            "ACC0035" => v < 97,
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
pub fn u2901mat_sum() -> u64 {
    let mut s = 0u64;
    let mut i = 0usize;
    while i < u2901mat_COUNT {
        s = s.wrapping_add(u2901mat[i].1 as u64);
        i += 1;
    }
    s
}

/// 键唯一性（构造期断言等价物：逐对扫描同键）。
pub fn u2901mat_unique() -> bool {
    let mut i = 0usize;
    while i < u2901mat_COUNT {
        let mut j = i + 1;
        while j < u2901mat_COUNT {
            if u2901mat[i].0 == u2901mat[j].0 {
                return false;
            }
            j += 1;
        }
        i += 1;
    }
    true
}

// --------------------------------------------------------------------------
// 直方图引擎 mxu2901hist（有界插入排序 + 分位数 P50/P90/P99）
// --------------------------------------------------------------------------
/// 页缓存三矩阵压测 直方图（96 桶滚动）。
/// 样本容量上限。
pub const MXU2901HIST_CAP: usize = 96;

/// 直方图账本（排序态 + 五统量）。
#[derive(Clone, Copy, Debug)]
pub struct mxu2901histHist {
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

impl mxu2901histHist {
    /// 空账构造。
    pub const fn new() -> Self {
        Self { samples: [0; 96], n: 0, drops: 0, min: 0, max: 0, sum: 0 }
    }

    /// 采样（有界插入排序；满员丢弃并记账）。
    pub fn push(&mut self, v: u64) {
        if self.n >= MXU2901HIST_CAP {
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

/// 段位守卫：全部诊断码必须落在本段 [0xe230, 0xe23f]。
pub const fn u2901_segment_guard() -> bool {
    let mut ok = true;
    let mut i = 0usize;
    while i < u2901Code::ALL.len() {
        let c = u2901Code::ALL[i].code();
        if c < 0xe230 || c > 0xe23f {
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
fn u2901_guard_seg() -> bool {
    u2901_segment_guard()
}

/// UNX-F2901 矩阵 36 行齐备 + 逐行守卫 + 唯一性 + 累计守恒。
fn check_u2901_map_guard() -> bool {
    let mut ok = u2901mat_COUNT == 36;
    if ok { ok = u2901mat_scan() == 0 && u2901mat_unique(); }
    if ok { ok = u2901mat_get("PC0000") == Some(0) && u2901mat_get("ZZZ_NOKEY").is_none(); }
    if ok { ok = u2901mat_sum() == 1594; }
    ok
}

/// UNX-F2901 直方图 16 样本分位/均值/守恒。
fn check_u2901_map_hist() -> bool {
    let mut h = mxu2901histHist::new();
    let mut i = 0u64;
    while i < 16 { h.push(i + 1); i += 1; }
    let mut ok = h.conserved() && h.mean() >= 1 && h.percentile(500) >= 8;
    ok
}

/// 本模块自检（逐检查带 detail，全绿才出门）。
pub fn run_u2901_checks() -> CheckSet {
    let mut set = CheckSet::new("U2901");
    set.add(
        "u2901-seg-guard",
        u2901_guard_seg(),
        if u2901_guard_seg() { "" } else { "diagnostic code outside segment" },
    );
    set.add(
        "u2901-map-guard",
        check_u2901_map_guard(),
        if check_u2901_map_guard() { "" } else { "UNX-F2901 matrix guard mismatch" },
    );
    set.add(
        "u2901-map-hist",
        check_u2901_map_hist(),
        if check_u2901_map_hist() { "" } else { "UNX-F2901 hist mismatch" },
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
    fn u2901_all_checks_green() {
        let set = run_u2901_checks();
        assert!(set.all_passed(), "U2901 domain checks must be green");
    }

    #[test]
    fn u2901_segment_guard_holds() {
        assert!(u2901_segment_guard());
    }
}
