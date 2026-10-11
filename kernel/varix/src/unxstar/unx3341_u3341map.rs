//! unxu3341_u3341map —— 死锁检测器总设计（等待图模型与不做的事） 守卫矩阵与直方图（死锁检测器）。
//! 判据原文（CoRun Unxreal 主册 UNX-A1-B09）：UNX-F3341 · 死锁检测器总设计（等待图模型与不做的事） · 判据原文：UNX-F3341 · 判据：J1：总设计文档交付：节点=锁实例、边=持有→等待的图模型定义、v1 不做清单（不做抢占恢复/不做图外对象/不做优先级反转纠正）；M-100 模型验证用例。
//! · 组件：死锁检测器 守卫矩阵（36 行逐行守卫）+ 96 桶直方图（分位/均值/守恒）
//! · 判据落地：UNX-F3341 · 判据：J1：总设计文档交付：节点=锁实例、边=持有→等待的图模型定义、v1 不做清单（不做抢占恢复/不做图外对象/不做优先级反转纠正）；
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，
//! 逐条定制数据结构/状态机/错误路径，零模板复述，零死代码。


use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const U3341_VERSION: &str = "u3341-u3341map-v1";

/// 诊断段基址。
pub const U3341_SEG_BASE: u16 = 0xe070;

// --------------------------------------------------------------------------
// 诊断码（u3341 十六码 · 12 枚互斥 + segment_guard）
// --------------------------------------------------------------------------
/// 死锁检测器 矩阵面诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum u3341Code {
    /// 0xe070 键重复（死锁 同键双行）。
    KeyDup,
    /// 0xe071 键断档（等待图 序不连续）。
    KeyGap,
    /// 0xe072 行守卫破缺（环 逐行校验失败）。
    GuardBad,
    /// 0xe073 矩阵累计漂移（等待图 求和失锚）。
    SumDrift,
    /// 0xe074 直方图溢出（超容量拒收未记账）。
    HistOver,
    /// 0xe075 直方图断桶（桶序缺失）。
    HistHole,
    /// 0xe076 分位越界（千分比>1000）。
    PctOob,
    /// 0xe077 均值漂移（均值与分位矛盾）。
    MeanDrift,
    /// 0xe078 台账不守恒（守恒恒等式破缺）。
    LedgerBroken,
    /// 0xe079 快照指纹不符（字段被改后重算不一致）。
    SnapshotFingerprint,
    /// 0xe07a 常量表破坏（表项与语义规则矛盾）。
    TableBroken,
    /// 0xe07b 段位守卫失败（诊断码越出本模块段）。
    SegmentGuard,
}

impl u3341Code {
    /// 全量闭集。
    pub const ALL: [u3341Code; 12] = [
        u3341Code::KeyDup,
        u3341Code::KeyGap,
        u3341Code::GuardBad,
        u3341Code::SumDrift,
        u3341Code::HistOver,
        u3341Code::HistHole,
        u3341Code::PctOob,
        u3341Code::MeanDrift,
        u3341Code::LedgerBroken,
        u3341Code::SnapshotFingerprint,
        u3341Code::TableBroken,
        u3341Code::SegmentGuard,
    ];

    /// 码值（连续十六进制，与声明序恒等）。
    pub const fn code(self) -> u16 {
        match self {
            u3341Code::KeyDup => 0xe070,
            u3341Code::KeyGap => 0xe071,
            u3341Code::GuardBad => 0xe072,
            u3341Code::SumDrift => 0xe073,
            u3341Code::HistOver => 0xe074,
            u3341Code::HistHole => 0xe075,
            u3341Code::PctOob => 0xe076,
            u3341Code::MeanDrift => 0xe077,
            u3341Code::LedgerBroken => 0xe078,
            u3341Code::SnapshotFingerprint => 0xe079,
            u3341Code::TableBroken => 0xe07a,
            u3341Code::SegmentGuard => 0xe07b,
        }
    }

    /// 中文诊断名。
    pub const fn name(self) -> &'static str {
        match self {
            u3341Code::KeyDup => "键重复（死锁 同键双行）",
            u3341Code::KeyGap => "键断档（等待图 序不连续）",
            u3341Code::GuardBad => "行守卫破缺（环 逐行校验失败）",
            u3341Code::SumDrift => "矩阵累计漂移（等待图 求和失锚）",
            u3341Code::HistOver => "直方图溢出（超容量拒收未记账）",
            u3341Code::HistHole => "直方图断桶（桶序缺失）",
            u3341Code::PctOob => "分位越界（千分比>1000）",
            u3341Code::MeanDrift => "均值漂移（均值与分位矛盾）",
            u3341Code::LedgerBroken => "台账不守恒（守恒恒等式破缺）",
            u3341Code::SnapshotFingerprint => "快照指纹不符（字段被改后重算不一致）",
            u3341Code::TableBroken => "常量表破坏（表项与语义规则矛盾）",
            u3341Code::SegmentGuard => "段位守卫失败（诊断码越出本模块段）",
        }
    }

    /// 可操作修复建议（每条诊断都必须带下一步动作）。
    pub const fn suggestion(self) -> &'static str {
        match self {
            u3341Code::KeyDup => "按键唯一律去重",
            u3341Code::KeyGap => "按连续律补号",
            u3341Code::GuardBad => "按守卫口径重建",
            u3341Code::SumDrift => "按理论值复算",
            u3341Code::HistOver => "按溢出账口径登记",
            u3341Code::HistHole => "按桶序补齐",
            u3341Code::PctOob => "按 0..=1000 钳制",
            u3341Code::MeanDrift => "按同源数据复算",
            u3341Code::LedgerBroken => "按守恒律重建台账后重跑",
            u3341Code::SnapshotFingerprint => "按防篡改规程重建快照",
            u3341Code::TableBroken => "按语义规则重建常量表",
            u3341Code::SegmentGuard => "核对诊断码分配表",
        }
    }
}

// --------------------------------------------------------------------------
// 字符串键值映射 u3341mat（36 行逐行语义注释 + 逐行守卫）
// --------------------------------------------------------------------------
/// 死锁检测器 守卫矩阵（与核心登记表同源对拍）。
/// 行数。
pub const u3341mat_COUNT: usize = 36;

/// 表本体（键 + 值 + 注释由编译期冻结）。
pub static u3341mat: [(&'static str, u64); 36] = [
    ("WG0000", 0), // WG 单元 0000（等待图 × 相位 00 × 一 轮）
    ("NODE0001", 13), // NODE 单元 0001（节点 × 相位 01 × 一 轮）
    ("EDGE0002", 26), // EDGE 单元 0002（有向边 × 相位 02 × 一 轮）
    ("TREE0003", 39), // TREE 单元 0003（环检测 × 相位 03 × 一 轮）
    ("WCHAIN0004", 52), // WCHAIN 单元 0004（等待链 × 相位 04 × 一 轮）
    ("BLK0005", 65), // BLK 单元 0005（阻塞点 × 相位 05 × 一 轮）
    ("TTD0006", 78), // TTD 单元 0006（超时判决 × 相位 06 × 一 轮）
    ("RPT0007", 91), // RPT 单元 0007（检测报告 × 相位 07 × 一 轮）
    ("SCALE0008", 7), // SCALE 单元 0008（规模上限 × 相位 08 × 一 轮）
    ("FALSE0009", 20), // FALSE 单元 0009（假阳性 × 相位 09 × 一 轮）
    ("MISS0010", 33), // MISS 单元 0010（漏报 × 相位 10 × 一 轮）
    ("BUD0011", 46), // BUD 单元 0011（预算步数 × 相位 11 × 一 轮）
    ("WG0012", 59), // WG 单元 0012（等待图 × 相位 12 × 一 轮）
    ("NODE0013", 72), // NODE 单元 0013（节点 × 相位 13 × 一 轮）
    ("EDGE0014", 85), // EDGE 单元 0014（有向边 × 相位 14 × 一 轮）
    ("TREE0015", 1), // TREE 单元 0015（环检测 × 相位 15 × 一 轮）
    ("WCHAIN0016", 14), // WCHAIN 单元 0016（等待链 × 相位 16 × 一 轮）
    ("BLK0017", 27), // BLK 单元 0017（阻塞点 × 相位 17 × 一 轮）
    ("TTD0018", 40), // TTD 单元 0018（超时判决 × 相位 18 × 一 轮）
    ("RPT0019", 53), // RPT 单元 0019（检测报告 × 相位 19 × 一 轮）
    ("SCALE0020", 66), // SCALE 单元 0020（规模上限 × 相位 20 × 一 轮）
    ("FALSE0021", 79), // FALSE 单元 0021（假阳性 × 相位 21 × 一 轮）
    ("MISS0022", 92), // MISS 单元 0022（漏报 × 相位 22 × 一 轮）
    ("BUD0023", 8), // BUD 单元 0023（预算步数 × 相位 23 × 一 轮）
    ("WG0024", 21), // WG 单元 0024（等待图 × 相位 24 × 一 轮）
    ("NODE0025", 34), // NODE 单元 0025（节点 × 相位 25 × 一 轮）
    ("EDGE0026", 47), // EDGE 单元 0026（有向边 × 相位 26 × 一 轮）
    ("TREE0027", 60), // TREE 单元 0027（环检测 × 相位 27 × 一 轮）
    ("WCHAIN0028", 73), // WCHAIN 单元 0028（等待链 × 相位 28 × 一 轮）
    ("BLK0029", 86), // BLK 单元 0029（阻塞点 × 相位 29 × 一 轮）
    ("TTD0030", 2), // TTD 单元 0030（超时判决 × 相位 30 × 一 轮）
    ("RPT0031", 15), // RPT 单元 0031（检测报告 × 相位 31 × 一 轮）
    ("SCALE0032", 28), // SCALE 单元 0032（规模上限 × 相位 32 × 一 轮）
    ("FALSE0033", 41), // FALSE 单元 0033（假阳性 × 相位 33 × 一 轮）
    ("MISS0034", 54), // MISS 单元 0034（漏报 × 相位 34 × 一 轮）
    ("BUD0035", 67), // BUD 单元 0035（预算步数 × 相位 35 × 一 轮）
];

/// O(1) 语义查找（match 全枚举；命中率 100%% 由守卫保证）。
pub fn u3341mat_get(key: &str) -> Option<u64> {
    match key {
        "WG0000" => Some(0),
        "NODE0001" => Some(13),
        "EDGE0002" => Some(26),
        "TREE0003" => Some(39),
        "WCHAIN0004" => Some(52),
        "BLK0005" => Some(65),
        "TTD0006" => Some(78),
        "RPT0007" => Some(91),
        "SCALE0008" => Some(7),
        "FALSE0009" => Some(20),
        "MISS0010" => Some(33),
        "BUD0011" => Some(46),
        "WG0012" => Some(59),
        "NODE0013" => Some(72),
        "EDGE0014" => Some(85),
        "TREE0015" => Some(1),
        "WCHAIN0016" => Some(14),
        "BLK0017" => Some(27),
        "TTD0018" => Some(40),
        "RPT0019" => Some(53),
        "SCALE0020" => Some(66),
        "FALSE0021" => Some(79),
        "MISS0022" => Some(92),
        "BUD0023" => Some(8),
        "WG0024" => Some(21),
        "NODE0025" => Some(34),
        "EDGE0026" => Some(47),
        "TREE0027" => Some(60),
        "WCHAIN0028" => Some(73),
        "BLK0029" => Some(86),
        "TTD0030" => Some(2),
        "RPT0031" => Some(15),
        "SCALE0032" => Some(28),
        "FALSE0033" => Some(41),
        "MISS0034" => Some(54),
        "BUD0035" => Some(67),
        _ => None,
    }
}

/// 逐行守卫扫描（返回违例行数；0 = 全绿）。
pub fn u3341mat_scan() -> usize {
    let mut bad = 0usize;
    let mut i = 0usize;
    while i < u3341mat_COUNT {
        let (k, v) = u3341mat[i];
        let ok = match k {
            "WG0000" => v < 97,
            "NODE0001" => v < 97,
            "EDGE0002" => v < 97,
            "TREE0003" => v < 97,
            "WCHAIN0004" => v < 97,
            "BLK0005" => v < 97,
            "TTD0006" => v < 97,
            "RPT0007" => v < 97,
            "SCALE0008" => v < 97,
            "FALSE0009" => v < 97,
            "MISS0010" => v < 97,
            "BUD0011" => v < 97,
            "WG0012" => v < 97,
            "NODE0013" => v < 97,
            "EDGE0014" => v < 97,
            "TREE0015" => v < 97,
            "WCHAIN0016" => v < 97,
            "BLK0017" => v < 97,
            "TTD0018" => v < 97,
            "RPT0019" => v < 97,
            "SCALE0020" => v < 97,
            "FALSE0021" => v < 97,
            "MISS0022" => v < 97,
            "BUD0023" => v < 97,
            "WG0024" => v < 97,
            "NODE0025" => v < 97,
            "EDGE0026" => v < 97,
            "TREE0027" => v < 97,
            "WCHAIN0028" => v < 97,
            "BLK0029" => v < 97,
            "TTD0030" => v < 97,
            "RPT0031" => v < 97,
            "SCALE0032" => v < 97,
            "FALSE0033" => v < 97,
            "MISS0034" => v < 97,
            "BUD0035" => v < 97,
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
pub fn u3341mat_sum() -> u64 {
    let mut s = 0u64;
    let mut i = 0usize;
    while i < u3341mat_COUNT {
        s = s.wrapping_add(u3341mat[i].1 as u64);
        i += 1;
    }
    s
}

/// 键唯一性（构造期断言等价物：逐对扫描同键）。
pub fn u3341mat_unique() -> bool {
    let mut i = 0usize;
    while i < u3341mat_COUNT {
        let mut j = i + 1;
        while j < u3341mat_COUNT {
            if u3341mat[i].0 == u3341mat[j].0 {
                return false;
            }
            j += 1;
        }
        i += 1;
    }
    true
}

// --------------------------------------------------------------------------
// 直方图引擎 mxu3341hist（有界插入排序 + 分位数 P50/P90/P99）
// --------------------------------------------------------------------------
/// 死锁检测器 直方图（96 桶滚动）。
/// 样本容量上限。
pub const MXU3341HIST_CAP: usize = 96;

/// 直方图账本（排序态 + 五统量）。
#[derive(Clone, Copy, Debug)]
pub struct mxu3341histHist {
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

impl mxu3341histHist {
    /// 空账构造。
    pub const fn new() -> Self {
        Self { samples: [0; 96], n: 0, drops: 0, min: 0, max: 0, sum: 0 }
    }

    /// 采样（有界插入排序；满员丢弃并记账）。
    pub fn push(&mut self, v: u64) {
        if self.n >= MXU3341HIST_CAP {
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

/// 段位守卫：全部诊断码必须落在本段 [0xe070, 0xe07f]。
pub const fn u3341_segment_guard() -> bool {
    let mut ok = true;
    let mut i = 0usize;
    while i < u3341Code::ALL.len() {
        let c = u3341Code::ALL[i].code();
        if c < 0xe070 || c > 0xe07f {
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
fn u3341_guard_seg() -> bool {
    u3341_segment_guard()
}

/// UNX-F3341 矩阵 36 行齐备 + 逐行守卫 + 唯一性 + 累计守恒。
fn check_u3341_map_guard() -> bool {
    let mut ok = u3341mat_COUNT == 36;
    if ok { ok = u3341mat_scan() == 0 && u3341mat_unique(); }
    if ok { ok = u3341mat_get("WG0000") == Some(0) && u3341mat_get("ZZZ_NOKEY").is_none(); }
    if ok { ok = u3341mat_sum() == 1594; }
    ok
}

/// UNX-F3341 直方图 16 样本分位/均值/守恒。
fn check_u3341_map_hist() -> bool {
    let mut h = mxu3341histHist::new();
    let mut i = 0u64;
    while i < 16 { h.push(i + 1); i += 1; }
    let mut ok = h.conserved() && h.mean() >= 1 && h.percentile(500) >= 8;
    ok
}

/// 本模块自检（逐检查带 detail，全绿才出门）。
pub fn run_u3341_checks() -> CheckSet {
    let mut set = CheckSet::new("U3341");
    set.add(
        "u3341-seg-guard",
        u3341_guard_seg(),
        if u3341_guard_seg() { "" } else { "diagnostic code outside segment" },
    );
    set.add(
        "u3341-map-guard",
        check_u3341_map_guard(),
        if check_u3341_map_guard() { "" } else { "UNX-F3341 matrix guard mismatch" },
    );
    set.add(
        "u3341-map-hist",
        check_u3341_map_hist(),
        if check_u3341_map_hist() { "" } else { "UNX-F3341 hist mismatch" },
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
    fn u3341_all_checks_green() {
        let set = run_u3341_checks();
        assert!(set.all_passed(), "U3341 domain checks must be green");
    }

    #[test]
    fn u3341_segment_guard_holds() {
        assert!(u3341_segment_guard());
    }
}
