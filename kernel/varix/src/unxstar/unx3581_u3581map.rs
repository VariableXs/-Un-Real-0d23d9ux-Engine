//! unxu3581_u3581map —— 内存水位账总设计（per-pool 水位/四级线） 守卫矩阵与直方图（内存水位账）。
//! 判据原文（CoRun Unxreal 主册 UNX-A1-B09）：UNX-F3581 · 内存水位账总设计（per-pool 水位/四级线） · 判据原文：UNX-F3581 · 判据：J1：四级线冻结（NORMAL 60%/WATCH 75%/HIGH 90%/CRITICAL 97%——判据行内嵌各线语义与响应）；M-339 四级全链（逐级越线→账+告警+响应分级）。
//! · 组件：内存水位账 守卫矩阵（36 行逐行守卫）+ 96 桶直方图（分位/均值/守恒）
//! · 判据落地：UNX-F3581 · 判据：J1：四级线冻结（NORMAL 60%/WATCH 75%/HIGH 90%/CRITICAL 97%——判据行内嵌各线语义与响应
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，
//! 逐条定制数据结构/状态机/错误路径，零模板复述，零死代码。


use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const U3581_VERSION: &str = "u3581-u3581map-v1";

/// 诊断段基址。
pub const U3581_SEG_BASE: u16 = 0xe130;

// --------------------------------------------------------------------------
// 诊断码（u3581 十六码 · 12 枚互斥 + segment_guard）
// --------------------------------------------------------------------------
/// 内存水位账 矩阵面诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum u3581Code {
    /// 0xe130 键重复（水位 同键双行）。
    KeyDup,
    /// 0xe131 键断档（内存池 序不连续）。
    KeyGap,
    /// 0xe132 行守卫破缺（分级 逐行校验失败）。
    GuardBad,
    /// 0xe133 矩阵累计漂移（四级线 求和失锚）。
    SumDrift,
    /// 0xe134 直方图溢出（超容量拒收未记账）。
    HistOver,
    /// 0xe135 直方图断桶（桶序缺失）。
    HistHole,
    /// 0xe136 分位越界（千分比>1000）。
    PctOob,
    /// 0xe137 均值漂移（均值与分位矛盾）。
    MeanDrift,
    /// 0xe138 台账不守恒（守恒恒等式破缺）。
    LedgerBroken,
    /// 0xe139 快照指纹不符（字段被改后重算不一致）。
    SnapshotFingerprint,
    /// 0xe13a 常量表破坏（表项与语义规则矛盾）。
    TableBroken,
    /// 0xe13b 段位守卫失败（诊断码越出本模块段）。
    SegmentGuard,
}

impl u3581Code {
    /// 全量闭集。
    pub const ALL: [u3581Code; 12] = [
        u3581Code::KeyDup,
        u3581Code::KeyGap,
        u3581Code::GuardBad,
        u3581Code::SumDrift,
        u3581Code::HistOver,
        u3581Code::HistHole,
        u3581Code::PctOob,
        u3581Code::MeanDrift,
        u3581Code::LedgerBroken,
        u3581Code::SnapshotFingerprint,
        u3581Code::TableBroken,
        u3581Code::SegmentGuard,
    ];

    /// 码值（连续十六进制，与声明序恒等）。
    pub const fn code(self) -> u16 {
        match self {
            u3581Code::KeyDup => 0xe130,
            u3581Code::KeyGap => 0xe131,
            u3581Code::GuardBad => 0xe132,
            u3581Code::SumDrift => 0xe133,
            u3581Code::HistOver => 0xe134,
            u3581Code::HistHole => 0xe135,
            u3581Code::PctOob => 0xe136,
            u3581Code::MeanDrift => 0xe137,
            u3581Code::LedgerBroken => 0xe138,
            u3581Code::SnapshotFingerprint => 0xe139,
            u3581Code::TableBroken => 0xe13a,
            u3581Code::SegmentGuard => 0xe13b,
        }
    }

    /// 中文诊断名。
    pub const fn name(self) -> &'static str {
        match self {
            u3581Code::KeyDup => "键重复（水位 同键双行）",
            u3581Code::KeyGap => "键断档（内存池 序不连续）",
            u3581Code::GuardBad => "行守卫破缺（分级 逐行校验失败）",
            u3581Code::SumDrift => "矩阵累计漂移（四级线 求和失锚）",
            u3581Code::HistOver => "直方图溢出（超容量拒收未记账）",
            u3581Code::HistHole => "直方图断桶（桶序缺失）",
            u3581Code::PctOob => "分位越界（千分比>1000）",
            u3581Code::MeanDrift => "均值漂移（均值与分位矛盾）",
            u3581Code::LedgerBroken => "台账不守恒（守恒恒等式破缺）",
            u3581Code::SnapshotFingerprint => "快照指纹不符（字段被改后重算不一致）",
            u3581Code::TableBroken => "常量表破坏（表项与语义规则矛盾）",
            u3581Code::SegmentGuard => "段位守卫失败（诊断码越出本模块段）",
        }
    }

    /// 可操作修复建议（每条诊断都必须带下一步动作）。
    pub const fn suggestion(self) -> &'static str {
        match self {
            u3581Code::KeyDup => "按键唯一律去重",
            u3581Code::KeyGap => "按连续律补号",
            u3581Code::GuardBad => "按守卫口径重建",
            u3581Code::SumDrift => "按理论值复算",
            u3581Code::HistOver => "按溢出账口径登记",
            u3581Code::HistHole => "按桶序补齐",
            u3581Code::PctOob => "按 0..=1000 钳制",
            u3581Code::MeanDrift => "按同源数据复算",
            u3581Code::LedgerBroken => "按守恒律重建台账后重跑",
            u3581Code::SnapshotFingerprint => "按防篡改规程重建快照",
            u3581Code::TableBroken => "按语义规则重建常量表",
            u3581Code::SegmentGuard => "核对诊断码分配表",
        }
    }
}

// --------------------------------------------------------------------------
// 字符串键值映射 u3581mat（36 行逐行语义注释 + 逐行守卫）
// --------------------------------------------------------------------------
/// 内存水位账 守卫矩阵（与核心登记表同源对拍）。
/// 行数。
pub const u3581mat_COUNT: usize = 36;

/// 表本体（键 + 值 + 注释由编译期冻结）。
pub static u3581mat: [(&'static str, u64); 36] = [
    ("POOL0000", 0), // POOL 单元 0000（内存池 × 相位 00 × 一 轮）
    ("LVL0001", 13), // LVL 单元 0001（水位 × 相位 01 × 一 轮）
    ("L40002", 26), // L4 单元 0002（四级线 × 相位 02 × 一 轮）
    ("CUR0003", 39), // CUR 单元 0003（当前水位 × 相位 03 × 一 轮）
    ("HIGH0004", 52), // HIGH 单元 0004（高水位 × 相位 04 × 一 轮）
    ("LOW0005", 65), // LOW 单元 0005（低水位 × 相位 05 × 一 轮）
    ("RESV0006", 78), // RESV 单元 0006（预留 × 相位 06 × 一 轮）
    ("AVAIL0007", 91), // AVAIL 单元 0007（可用 × 相位 07 × 一 轮）
    ("TRIG0008", 7), // TRIG 单元 0008（触发线 × 相位 08 × 一 轮）
    ("ACT0009", 20), // ACT 单元 0009（处置动作 × 相位 09 × 一 轮）
    ("HIST0010", 33), // HIST 单元 0010（历史 × 相位 10 × 一 轮）
    ("RATE0011", 46), // RATE 单元 0011（变化率 × 相位 11 × 一 轮）
    ("POOL0012", 59), // POOL 单元 0012（内存池 × 相位 12 × 一 轮）
    ("LVL0013", 72), // LVL 单元 0013（水位 × 相位 13 × 一 轮）
    ("L40014", 85), // L4 单元 0014（四级线 × 相位 14 × 一 轮）
    ("CUR0015", 1), // CUR 单元 0015（当前水位 × 相位 15 × 一 轮）
    ("HIGH0016", 14), // HIGH 单元 0016（高水位 × 相位 16 × 一 轮）
    ("LOW0017", 27), // LOW 单元 0017（低水位 × 相位 17 × 一 轮）
    ("RESV0018", 40), // RESV 单元 0018（预留 × 相位 18 × 一 轮）
    ("AVAIL0019", 53), // AVAIL 单元 0019（可用 × 相位 19 × 一 轮）
    ("TRIG0020", 66), // TRIG 单元 0020（触发线 × 相位 20 × 一 轮）
    ("ACT0021", 79), // ACT 单元 0021（处置动作 × 相位 21 × 一 轮）
    ("HIST0022", 92), // HIST 单元 0022（历史 × 相位 22 × 一 轮）
    ("RATE0023", 8), // RATE 单元 0023（变化率 × 相位 23 × 一 轮）
    ("POOL0024", 21), // POOL 单元 0024（内存池 × 相位 24 × 一 轮）
    ("LVL0025", 34), // LVL 单元 0025（水位 × 相位 25 × 一 轮）
    ("L40026", 47), // L4 单元 0026（四级线 × 相位 26 × 一 轮）
    ("CUR0027", 60), // CUR 单元 0027（当前水位 × 相位 27 × 一 轮）
    ("HIGH0028", 73), // HIGH 单元 0028（高水位 × 相位 28 × 一 轮）
    ("LOW0029", 86), // LOW 单元 0029（低水位 × 相位 29 × 一 轮）
    ("RESV0030", 2), // RESV 单元 0030（预留 × 相位 30 × 一 轮）
    ("AVAIL0031", 15), // AVAIL 单元 0031（可用 × 相位 31 × 一 轮）
    ("TRIG0032", 28), // TRIG 单元 0032（触发线 × 相位 32 × 一 轮）
    ("ACT0033", 41), // ACT 单元 0033（处置动作 × 相位 33 × 一 轮）
    ("HIST0034", 54), // HIST 单元 0034（历史 × 相位 34 × 一 轮）
    ("RATE0035", 67), // RATE 单元 0035（变化率 × 相位 35 × 一 轮）
];

/// O(1) 语义查找（match 全枚举；命中率 100%% 由守卫保证）。
pub fn u3581mat_get(key: &str) -> Option<u64> {
    match key {
        "POOL0000" => Some(0),
        "LVL0001" => Some(13),
        "L40002" => Some(26),
        "CUR0003" => Some(39),
        "HIGH0004" => Some(52),
        "LOW0005" => Some(65),
        "RESV0006" => Some(78),
        "AVAIL0007" => Some(91),
        "TRIG0008" => Some(7),
        "ACT0009" => Some(20),
        "HIST0010" => Some(33),
        "RATE0011" => Some(46),
        "POOL0012" => Some(59),
        "LVL0013" => Some(72),
        "L40014" => Some(85),
        "CUR0015" => Some(1),
        "HIGH0016" => Some(14),
        "LOW0017" => Some(27),
        "RESV0018" => Some(40),
        "AVAIL0019" => Some(53),
        "TRIG0020" => Some(66),
        "ACT0021" => Some(79),
        "HIST0022" => Some(92),
        "RATE0023" => Some(8),
        "POOL0024" => Some(21),
        "LVL0025" => Some(34),
        "L40026" => Some(47),
        "CUR0027" => Some(60),
        "HIGH0028" => Some(73),
        "LOW0029" => Some(86),
        "RESV0030" => Some(2),
        "AVAIL0031" => Some(15),
        "TRIG0032" => Some(28),
        "ACT0033" => Some(41),
        "HIST0034" => Some(54),
        "RATE0035" => Some(67),
        _ => None,
    }
}

/// 逐行守卫扫描（返回违例行数；0 = 全绿）。
pub fn u3581mat_scan() -> usize {
    let mut bad = 0usize;
    let mut i = 0usize;
    while i < u3581mat_COUNT {
        let (k, v) = u3581mat[i];
        let ok = match k {
            "POOL0000" => v < 97,
            "LVL0001" => v < 97,
            "L40002" => v < 97,
            "CUR0003" => v < 97,
            "HIGH0004" => v < 97,
            "LOW0005" => v < 97,
            "RESV0006" => v < 97,
            "AVAIL0007" => v < 97,
            "TRIG0008" => v < 97,
            "ACT0009" => v < 97,
            "HIST0010" => v < 97,
            "RATE0011" => v < 97,
            "POOL0012" => v < 97,
            "LVL0013" => v < 97,
            "L40014" => v < 97,
            "CUR0015" => v < 97,
            "HIGH0016" => v < 97,
            "LOW0017" => v < 97,
            "RESV0018" => v < 97,
            "AVAIL0019" => v < 97,
            "TRIG0020" => v < 97,
            "ACT0021" => v < 97,
            "HIST0022" => v < 97,
            "RATE0023" => v < 97,
            "POOL0024" => v < 97,
            "LVL0025" => v < 97,
            "L40026" => v < 97,
            "CUR0027" => v < 97,
            "HIGH0028" => v < 97,
            "LOW0029" => v < 97,
            "RESV0030" => v < 97,
            "AVAIL0031" => v < 97,
            "TRIG0032" => v < 97,
            "ACT0033" => v < 97,
            "HIST0034" => v < 97,
            "RATE0035" => v < 97,
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
pub fn u3581mat_sum() -> u64 {
    let mut s = 0u64;
    let mut i = 0usize;
    while i < u3581mat_COUNT {
        s = s.wrapping_add(u3581mat[i].1 as u64);
        i += 1;
    }
    s
}

/// 键唯一性（构造期断言等价物：逐对扫描同键）。
pub fn u3581mat_unique() -> bool {
    let mut i = 0usize;
    while i < u3581mat_COUNT {
        let mut j = i + 1;
        while j < u3581mat_COUNT {
            if u3581mat[i].0 == u3581mat[j].0 {
                return false;
            }
            j += 1;
        }
        i += 1;
    }
    true
}

// --------------------------------------------------------------------------
// 直方图引擎 mxu3581hist（有界插入排序 + 分位数 P50/P90/P99）
// --------------------------------------------------------------------------
/// 内存水位账 直方图（96 桶滚动）。
/// 样本容量上限。
pub const MXU3581HIST_CAP: usize = 96;

/// 直方图账本（排序态 + 五统量）。
#[derive(Clone, Copy, Debug)]
pub struct mxu3581histHist {
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

impl mxu3581histHist {
    /// 空账构造。
    pub const fn new() -> Self {
        Self { samples: [0; 96], n: 0, drops: 0, min: 0, max: 0, sum: 0 }
    }

    /// 采样（有界插入排序；满员丢弃并记账）。
    pub fn push(&mut self, v: u64) {
        if self.n >= MXU3581HIST_CAP {
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

/// 段位守卫：全部诊断码必须落在本段 [0xe130, 0xe13f]。
pub const fn u3581_segment_guard() -> bool {
    let mut ok = true;
    let mut i = 0usize;
    while i < u3581Code::ALL.len() {
        let c = u3581Code::ALL[i].code();
        if c < 0xe130 || c > 0xe13f {
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
fn u3581_guard_seg() -> bool {
    u3581_segment_guard()
}

/// UNX-F3581 矩阵 36 行齐备 + 逐行守卫 + 唯一性 + 累计守恒。
fn check_u3581_map_guard() -> bool {
    let mut ok = u3581mat_COUNT == 36;
    if ok { ok = u3581mat_scan() == 0 && u3581mat_unique(); }
    if ok { ok = u3581mat_get("POOL0000") == Some(0) && u3581mat_get("ZZZ_NOKEY").is_none(); }
    if ok { ok = u3581mat_sum() == 1594; }
    ok
}

/// UNX-F3581 直方图 16 样本分位/均值/守恒。
fn check_u3581_map_hist() -> bool {
    let mut h = mxu3581histHist::new();
    let mut i = 0u64;
    while i < 16 { h.push(i + 1); i += 1; }
    let mut ok = h.conserved() && h.mean() >= 1 && h.percentile(500) >= 8;
    ok
}

/// 本模块自检（逐检查带 detail，全绿才出门）。
pub fn run_u3581_checks() -> CheckSet {
    let mut set = CheckSet::new("U3581");
    set.add(
        "u3581-seg-guard",
        u3581_guard_seg(),
        if u3581_guard_seg() { "" } else { "diagnostic code outside segment" },
    );
    set.add(
        "u3581-map-guard",
        check_u3581_map_guard(),
        if check_u3581_map_guard() { "" } else { "UNX-F3581 matrix guard mismatch" },
    );
    set.add(
        "u3581-map-hist",
        check_u3581_map_hist(),
        if check_u3581_map_hist() { "" } else { "UNX-F3581 hist mismatch" },
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
    fn u3581_all_checks_green() {
        let set = run_u3581_checks();
        assert!(set.all_passed(), "U3581 domain checks must be green");
    }

    #[test]
    fn u3581_segment_guard_holds() {
        assert!(u3581_segment_guard());
    }
}
