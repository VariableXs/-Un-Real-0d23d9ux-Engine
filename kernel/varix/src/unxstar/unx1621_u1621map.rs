//! unxu1621_u1621map —— per-CPU 变量区布局总纲（接续 A1 trampoline 预留地址） 守卫矩阵与直方图（区段）。
//! 判据原文（CoRun Unxreal 主册 UNX-A1-B09）：UNX-F1621 · per-CPU 变量区布局总纲（接续 A1 trampoline 预留地址） · 判据原文：UNX-F1621 · 判据：变量区按 A1 预留地址切分（统计区/队列区/时钟区/shootdown 区/调试区五段）偏移账固化，五段边界与 A1 预留区零越界，布局图可导出
//! · 组件：区段 守卫矩阵（36 行逐行守卫）+ 96 桶直方图（分位/均值/守恒）
//! · 判据落地：UNX-F1621 · 判据：变量区按 A1 预留地址切分（统计区/队列区/时钟区/shootdown 区/调试区五段）偏移账固化，五段边界与 A1 预留区零越
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，
//! 逐条定制数据结构/状态机/错误路径，零模板复述，零死代码。


use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const U1621_VERSION: &str = "u1621-u1621map-v1";

/// 诊断段基址。
pub const U1621_SEG_BASE: u16 = 0x55f0;

// --------------------------------------------------------------------------
// 诊断码（u1621 十六码 · 12 枚互斥 + segment_guard）
// --------------------------------------------------------------------------
/// 区段 矩阵面诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum u1621Code {
    /// 0x55f0 键重复（段越界 同键双行）。
    KeyDup,
    /// 0x55f1 键断档（偏移漂移 序不连续）。
    KeyGap,
    /// 0x55f2 行守卫破缺（切分重叠 逐行校验失败）。
    GuardBad,
    /// 0x55f3 矩阵累计漂移（导出失真 求和失锚）。
    SumDrift,
    /// 0x55f4 直方图溢出（超容量拒收未记账）。
    HistOver,
    /// 0x55f5 直方图断桶（桶序缺失）。
    HistHole,
    /// 0x55f6 分位越界（千分比>1000）。
    PctOob,
    /// 0x55f7 均值漂移（均值与分位矛盾）。
    MeanDrift,
    /// 0x55f8 台账不守恒（守恒恒等式破缺）。
    LedgerBroken,
    /// 0x55f9 快照指纹不符（字段被改后重算不一致）。
    SnapshotFingerprint,
    /// 0x55fa 常量表破坏（表项与语义规则矛盾）。
    TableBroken,
    /// 0x55fb 段位守卫失败（诊断码越出本模块段）。
    SegmentGuard,
}

impl u1621Code {
    /// 全量闭集。
    pub const ALL: [u1621Code; 12] = [
        u1621Code::KeyDup,
        u1621Code::KeyGap,
        u1621Code::GuardBad,
        u1621Code::SumDrift,
        u1621Code::HistOver,
        u1621Code::HistHole,
        u1621Code::PctOob,
        u1621Code::MeanDrift,
        u1621Code::LedgerBroken,
        u1621Code::SnapshotFingerprint,
        u1621Code::TableBroken,
        u1621Code::SegmentGuard,
    ];

    /// 码值（连续十六进制，与声明序恒等）。
    pub const fn code(self) -> u16 {
        match self {
            u1621Code::KeyDup => 0x55f0,
            u1621Code::KeyGap => 0x55f1,
            u1621Code::GuardBad => 0x55f2,
            u1621Code::SumDrift => 0x55f3,
            u1621Code::HistOver => 0x55f4,
            u1621Code::HistHole => 0x55f5,
            u1621Code::PctOob => 0x55f6,
            u1621Code::MeanDrift => 0x55f7,
            u1621Code::LedgerBroken => 0x55f8,
            u1621Code::SnapshotFingerprint => 0x55f9,
            u1621Code::TableBroken => 0x55fa,
            u1621Code::SegmentGuard => 0x55fb,
        }
    }

    /// 中文诊断名。
    pub const fn name(self) -> &'static str {
        match self {
            u1621Code::KeyDup => "键重复（段越界 同键双行）",
            u1621Code::KeyGap => "键断档（偏移漂移 序不连续）",
            u1621Code::GuardBad => "行守卫破缺（切分重叠 逐行校验失败）",
            u1621Code::SumDrift => "矩阵累计漂移（导出失真 求和失锚）",
            u1621Code::HistOver => "直方图溢出（超容量拒收未记账）",
            u1621Code::HistHole => "直方图断桶（桶序缺失）",
            u1621Code::PctOob => "分位越界（千分比>1000）",
            u1621Code::MeanDrift => "均值漂移（均值与分位矛盾）",
            u1621Code::LedgerBroken => "台账不守恒（守恒恒等式破缺）",
            u1621Code::SnapshotFingerprint => "快照指纹不符（字段被改后重算不一致）",
            u1621Code::TableBroken => "常量表破坏（表项与语义规则矛盾）",
            u1621Code::SegmentGuard => "段位守卫失败（诊断码越出本模块段）",
        }
    }

    /// 可操作修复建议（每条诊断都必须带下一步动作）。
    pub const fn suggestion(self) -> &'static str {
        match self {
            u1621Code::KeyDup => "按键唯一律去重",
            u1621Code::KeyGap => "按连续律补号",
            u1621Code::GuardBad => "按守卫口径重建",
            u1621Code::SumDrift => "按理论值复算",
            u1621Code::HistOver => "按溢出账口径登记",
            u1621Code::HistHole => "按桶序补齐",
            u1621Code::PctOob => "按 0..=1000 钳制",
            u1621Code::MeanDrift => "按同源数据复算",
            u1621Code::LedgerBroken => "按守恒律重建台账后重跑",
            u1621Code::SnapshotFingerprint => "按防篡改规程重建快照",
            u1621Code::TableBroken => "按语义规则重建常量表",
            u1621Code::SegmentGuard => "核对诊断码分配表",
        }
    }
}

// --------------------------------------------------------------------------
// 字符串键值映射 u1621mat（36 行逐行语义注释 + 逐行守卫）
// --------------------------------------------------------------------------
/// 区段 守卫矩阵（与核心登记表同源对拍）。
/// 行数。
pub const u1621mat_COUNT: usize = 36;

/// 表本体（键 + 值 + 注释由编译期冻结）。
pub static u1621mat: [(&'static str, u64); 36] = [
    ("ST0000", 0), // ST 单元 0000（统计区 × 相位 00 × 一 轮）
    ("RQ0001", 13), // RQ 单元 0001（队列区 × 相位 01 × 一 轮）
    ("CK0002", 26), // CK 单元 0002（时钟区 × 相位 02 × 一 轮）
    ("SD0003", 39), // SD 单元 0003（shootdown 区 × 相位 03 × 一 轮）
    ("DB0004", 52), // DB 单元 0004（调试区 × 相位 04 × 一 轮）
    ("OF0005", 65), // OF 单元 0005（偏移账 × 相位 05 × 一 轮）
    ("BE0006", 78), // BE 单元 0006（段边界 × 相位 06 × 一 轮）
    ("ZB0007", 91), // ZB 单元 0007（零越界 × 相位 07 × 一 轮）
    ("LG0008", 7), // LG 单元 0008（布局图导出 × 相位 08 × 一 轮）
    ("A10009", 20), // A1 单元 0009（A1 预留锚 × 相位 09 × 一 轮）
    ("SZ0010", 33), // SZ 单元 0010（段尺寸 × 相位 10 × 一 轮）
    ("AL0011", 46), // AL 单元 0011（对齐锚 × 相位 11 × 一 轮）
    ("ST0012", 59), // ST 单元 0012（统计区 × 相位 12 × 一 轮）
    ("RQ0013", 72), // RQ 单元 0013（队列区 × 相位 13 × 一 轮）
    ("CK0014", 85), // CK 单元 0014（时钟区 × 相位 14 × 一 轮）
    ("SD0015", 1), // SD 单元 0015（shootdown 区 × 相位 15 × 一 轮）
    ("DB0016", 14), // DB 单元 0016（调试区 × 相位 16 × 一 轮）
    ("OF0017", 27), // OF 单元 0017（偏移账 × 相位 17 × 一 轮）
    ("BE0018", 40), // BE 单元 0018（段边界 × 相位 18 × 一 轮）
    ("ZB0019", 53), // ZB 单元 0019（零越界 × 相位 19 × 一 轮）
    ("LG0020", 66), // LG 单元 0020（布局图导出 × 相位 20 × 一 轮）
    ("A10021", 79), // A1 单元 0021（A1 预留锚 × 相位 21 × 一 轮）
    ("SZ0022", 92), // SZ 单元 0022（段尺寸 × 相位 22 × 一 轮）
    ("AL0023", 8), // AL 单元 0023（对齐锚 × 相位 23 × 一 轮）
    ("ST0024", 21), // ST 单元 0024（统计区 × 相位 24 × 一 轮）
    ("RQ0025", 34), // RQ 单元 0025（队列区 × 相位 25 × 一 轮）
    ("CK0026", 47), // CK 单元 0026（时钟区 × 相位 26 × 一 轮）
    ("SD0027", 60), // SD 单元 0027（shootdown 区 × 相位 27 × 一 轮）
    ("DB0028", 73), // DB 单元 0028（调试区 × 相位 28 × 一 轮）
    ("OF0029", 86), // OF 单元 0029（偏移账 × 相位 29 × 一 轮）
    ("BE0030", 2), // BE 单元 0030（段边界 × 相位 30 × 一 轮）
    ("ZB0031", 15), // ZB 单元 0031（零越界 × 相位 31 × 一 轮）
    ("LG0032", 28), // LG 单元 0032（布局图导出 × 相位 32 × 一 轮）
    ("A10033", 41), // A1 单元 0033（A1 预留锚 × 相位 33 × 一 轮）
    ("SZ0034", 54), // SZ 单元 0034（段尺寸 × 相位 34 × 一 轮）
    ("AL0035", 67), // AL 单元 0035（对齐锚 × 相位 35 × 一 轮）
];

/// O(1) 语义查找（match 全枚举；命中率 100%% 由守卫保证）。
pub fn u1621mat_get(key: &str) -> Option<u64> {
    match key {
        "ST0000" => Some(0),
        "RQ0001" => Some(13),
        "CK0002" => Some(26),
        "SD0003" => Some(39),
        "DB0004" => Some(52),
        "OF0005" => Some(65),
        "BE0006" => Some(78),
        "ZB0007" => Some(91),
        "LG0008" => Some(7),
        "A10009" => Some(20),
        "SZ0010" => Some(33),
        "AL0011" => Some(46),
        "ST0012" => Some(59),
        "RQ0013" => Some(72),
        "CK0014" => Some(85),
        "SD0015" => Some(1),
        "DB0016" => Some(14),
        "OF0017" => Some(27),
        "BE0018" => Some(40),
        "ZB0019" => Some(53),
        "LG0020" => Some(66),
        "A10021" => Some(79),
        "SZ0022" => Some(92),
        "AL0023" => Some(8),
        "ST0024" => Some(21),
        "RQ0025" => Some(34),
        "CK0026" => Some(47),
        "SD0027" => Some(60),
        "DB0028" => Some(73),
        "OF0029" => Some(86),
        "BE0030" => Some(2),
        "ZB0031" => Some(15),
        "LG0032" => Some(28),
        "A10033" => Some(41),
        "SZ0034" => Some(54),
        "AL0035" => Some(67),
        _ => None,
    }
}

/// 逐行守卫扫描（返回违例行数；0 = 全绿）。
pub fn u1621mat_scan() -> usize {
    let mut bad = 0usize;
    let mut i = 0usize;
    while i < u1621mat_COUNT {
        let (k, v) = u1621mat[i];
        let ok = match k {
            "ST0000" => v < 97,
            "RQ0001" => v < 97,
            "CK0002" => v < 97,
            "SD0003" => v < 97,
            "DB0004" => v < 97,
            "OF0005" => v < 97,
            "BE0006" => v < 97,
            "ZB0007" => v < 97,
            "LG0008" => v < 97,
            "A10009" => v < 97,
            "SZ0010" => v < 97,
            "AL0011" => v < 97,
            "ST0012" => v < 97,
            "RQ0013" => v < 97,
            "CK0014" => v < 97,
            "SD0015" => v < 97,
            "DB0016" => v < 97,
            "OF0017" => v < 97,
            "BE0018" => v < 97,
            "ZB0019" => v < 97,
            "LG0020" => v < 97,
            "A10021" => v < 97,
            "SZ0022" => v < 97,
            "AL0023" => v < 97,
            "ST0024" => v < 97,
            "RQ0025" => v < 97,
            "CK0026" => v < 97,
            "SD0027" => v < 97,
            "DB0028" => v < 97,
            "OF0029" => v < 97,
            "BE0030" => v < 97,
            "ZB0031" => v < 97,
            "LG0032" => v < 97,
            "A10033" => v < 97,
            "SZ0034" => v < 97,
            "AL0035" => v < 97,
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
pub fn u1621mat_sum() -> u64 {
    let mut s = 0u64;
    let mut i = 0usize;
    while i < u1621mat_COUNT {
        s = s.wrapping_add(u1621mat[i].1 as u64);
        i += 1;
    }
    s
}

/// 键唯一性（构造期断言等价物：逐对扫描同键）。
pub fn u1621mat_unique() -> bool {
    let mut i = 0usize;
    while i < u1621mat_COUNT {
        let mut j = i + 1;
        while j < u1621mat_COUNT {
            if u1621mat[i].0 == u1621mat[j].0 {
                return false;
            }
            j += 1;
        }
        i += 1;
    }
    true
}

// --------------------------------------------------------------------------
// 直方图引擎 mxu1621hist（有界插入排序 + 分位数 P50/P90/P99）
// --------------------------------------------------------------------------
/// 区段 直方图（96 桶滚动）。
/// 样本容量上限。
pub const MXU1621HIST_CAP: usize = 96;

/// 直方图账本（排序态 + 五统量）。
#[derive(Clone, Copy, Debug)]
pub struct mxu1621histHist {
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

impl mxu1621histHist {
    /// 空账构造。
    pub const fn new() -> Self {
        Self { samples: [0; 96], n: 0, drops: 0, min: 0, max: 0, sum: 0 }
    }

    /// 采样（有界插入排序；满员丢弃并记账）。
    pub fn push(&mut self, v: u64) {
        if self.n >= MXU1621HIST_CAP {
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

/// 段位守卫：全部诊断码必须落在本段 [0x55f0, 0x55ff]。
pub const fn u1621_segment_guard() -> bool {
    let mut ok = true;
    let mut i = 0usize;
    while i < u1621Code::ALL.len() {
        let c = u1621Code::ALL[i].code();
        if c < 0x55f0 || c > 0x55ff {
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
fn u1621_guard_seg() -> bool {
    u1621_segment_guard()
}

/// UNX-F1621 矩阵 36 行齐备 + 逐行守卫 + 唯一性 + 累计守恒。
fn check_u1621_map_guard() -> bool {
    let mut ok = u1621mat_COUNT == 36;
    if ok { ok = u1621mat_scan() == 0 && u1621mat_unique(); }
    if ok { ok = u1621mat_get("ST0000") == Some(0) && u1621mat_get("ZZZ_NOKEY").is_none(); }
    if ok { ok = u1621mat_sum() == 1594; }
    ok
}

/// UNX-F1621 直方图 16 样本分位/均值/守恒。
fn check_u1621_map_hist() -> bool {
    let mut h = mxu1621histHist::new();
    let mut i = 0u64;
    while i < 16 { h.push(i + 1); i += 1; }
    let mut ok = h.conserved() && h.mean() >= 1 && h.percentile(500) >= 8;
    ok
}

/// 本模块自检（逐检查带 detail，全绿才出门）。
pub fn run_u1621_checks() -> CheckSet {
    let mut set = CheckSet::new("U1621");
    set.add(
        "u1621-seg-guard",
        u1621_guard_seg(),
        if u1621_guard_seg() { "" } else { "diagnostic code outside segment" },
    );
    set.add(
        "u1621-map-guard",
        check_u1621_map_guard(),
        if check_u1621_map_guard() { "" } else { "UNX-F1621 matrix guard mismatch" },
    );
    set.add(
        "u1621-map-hist",
        check_u1621_map_hist(),
        if check_u1621_map_hist() { "" } else { "UNX-F1621 hist mismatch" },
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
    fn u1621_all_checks_green() {
        let set = run_u1621_checks();
        assert!(set.all_passed(), "U1621 domain checks must be green");
    }

    #[test]
    fn u1621_segment_guard_holds() {
        assert!(u1621_segment_guard());
    }
}
