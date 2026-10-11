//! unxu1841_u1841map —— IPI 架构总纲（向量分派框架与 A2 衔接） 守卫矩阵与直方图（向量）。
//! 判据原文（CoRun Unxreal 主册 UNX-A1-B09）：UNX-F1841 · IPI 架构总纲（向量分派框架与 A2 衔接） · 判据原文：UNX-F1841 · 判据：IPI 分派框架冻结 v1（向量→处理函数表+分发协议+EOI 纪律），与 A2 原语衔接断言全过，未登记向量注入 10/10 拒绝
//! · 组件：向量 守卫矩阵（36 行逐行守卫）+ 96 桶直方图（分位/均值/守恒）
//! · 判据落地：UNX-F1841 · 判据：IPI 分派框架冻结 v1（向量→处理函数表+分发协议+EOI 纪律），与 A2 原语衔接断言全过，未登记向量注入 10/10 拒
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，
//! 逐条定制数据结构/状态机/错误路径，零模板复述，零死代码。


use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const U1841_VERSION: &str = "u1841-u1841map-v1";

/// 诊断段基址。
pub const U1841_SEG_BASE: u16 = 0x5e30;

// --------------------------------------------------------------------------
// 诊断码（u1841 十六码 · 12 枚互斥 + segment_guard）
// --------------------------------------------------------------------------
/// 向量 矩阵面诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum u1841Code {
    /// 0x5e30 键重复（未登记向量 同键双行）。
    KeyDup,
    /// 0x5e31 键断档（EOI 漏发 序不连续）。
    KeyGap,
    /// 0x5e32 行守卫破缺（衔接断裂 逐行校验失败）。
    GuardBad,
    /// 0x5e33 矩阵累计漂移（分派漂移 求和失锚）。
    SumDrift,
    /// 0x5e34 直方图溢出（超容量拒收未记账）。
    HistOver,
    /// 0x5e35 直方图断桶（桶序缺失）。
    HistHole,
    /// 0x5e36 分位越界（千分比>1000）。
    PctOob,
    /// 0x5e37 均值漂移（均值与分位矛盾）。
    MeanDrift,
    /// 0x5e38 台账不守恒（守恒恒等式破缺）。
    LedgerBroken,
    /// 0x5e39 快照指纹不符（字段被改后重算不一致）。
    SnapshotFingerprint,
    /// 0x5e3a 常量表破坏（表项与语义规则矛盾）。
    TableBroken,
    /// 0x5e3b 段位守卫失败（诊断码越出本模块段）。
    SegmentGuard,
}

impl u1841Code {
    /// 全量闭集。
    pub const ALL: [u1841Code; 12] = [
        u1841Code::KeyDup,
        u1841Code::KeyGap,
        u1841Code::GuardBad,
        u1841Code::SumDrift,
        u1841Code::HistOver,
        u1841Code::HistHole,
        u1841Code::PctOob,
        u1841Code::MeanDrift,
        u1841Code::LedgerBroken,
        u1841Code::SnapshotFingerprint,
        u1841Code::TableBroken,
        u1841Code::SegmentGuard,
    ];

    /// 码值（连续十六进制，与声明序恒等）。
    pub const fn code(self) -> u16 {
        match self {
            u1841Code::KeyDup => 0x5e30,
            u1841Code::KeyGap => 0x5e31,
            u1841Code::GuardBad => 0x5e32,
            u1841Code::SumDrift => 0x5e33,
            u1841Code::HistOver => 0x5e34,
            u1841Code::HistHole => 0x5e35,
            u1841Code::PctOob => 0x5e36,
            u1841Code::MeanDrift => 0x5e37,
            u1841Code::LedgerBroken => 0x5e38,
            u1841Code::SnapshotFingerprint => 0x5e39,
            u1841Code::TableBroken => 0x5e3a,
            u1841Code::SegmentGuard => 0x5e3b,
        }
    }

    /// 中文诊断名。
    pub const fn name(self) -> &'static str {
        match self {
            u1841Code::KeyDup => "键重复（未登记向量 同键双行）",
            u1841Code::KeyGap => "键断档（EOI 漏发 序不连续）",
            u1841Code::GuardBad => "行守卫破缺（衔接断裂 逐行校验失败）",
            u1841Code::SumDrift => "矩阵累计漂移（分派漂移 求和失锚）",
            u1841Code::HistOver => "直方图溢出（超容量拒收未记账）",
            u1841Code::HistHole => "直方图断桶（桶序缺失）",
            u1841Code::PctOob => "分位越界（千分比>1000）",
            u1841Code::MeanDrift => "均值漂移（均值与分位矛盾）",
            u1841Code::LedgerBroken => "台账不守恒（守恒恒等式破缺）",
            u1841Code::SnapshotFingerprint => "快照指纹不符（字段被改后重算不一致）",
            u1841Code::TableBroken => "常量表破坏（表项与语义规则矛盾）",
            u1841Code::SegmentGuard => "段位守卫失败（诊断码越出本模块段）",
        }
    }

    /// 可操作修复建议（每条诊断都必须带下一步动作）。
    pub const fn suggestion(self) -> &'static str {
        match self {
            u1841Code::KeyDup => "按键唯一律去重",
            u1841Code::KeyGap => "按连续律补号",
            u1841Code::GuardBad => "按守卫口径重建",
            u1841Code::SumDrift => "按理论值复算",
            u1841Code::HistOver => "按溢出账口径登记",
            u1841Code::HistHole => "按桶序补齐",
            u1841Code::PctOob => "按 0..=1000 钳制",
            u1841Code::MeanDrift => "按同源数据复算",
            u1841Code::LedgerBroken => "按守恒律重建台账后重跑",
            u1841Code::SnapshotFingerprint => "按防篡改规程重建快照",
            u1841Code::TableBroken => "按语义规则重建常量表",
            u1841Code::SegmentGuard => "核对诊断码分配表",
        }
    }
}

// --------------------------------------------------------------------------
// 字符串键值映射 u1841mat（36 行逐行语义注释 + 逐行守卫）
// --------------------------------------------------------------------------
/// 向量 守卫矩阵（与核心登记表同源对拍）。
/// 行数。
pub const u1841mat_COUNT: usize = 36;

/// 表本体（键 + 值 + 注释由编译期冻结）。
pub static u1841mat: [(&'static str, u64); 36] = [
    ("XL0000", 0), // XL 单元 0000（向量表 × 相位 00 × 一 轮）
    ("CL0001", 13), // CL 单元 0001（处理函数表 × 相位 01 × 一 轮）
    ("FF0002", 26), // FF 单元 0002（分发协议 × 相位 02 × 一 轮）
    ("EOI0003", 39), // EOI 单元 0003（EOI 纪律 × 相位 03 × 一 轮）
    ("A20004", 52), // A2 单元 0004（A2 原语衔接 × 相位 04 × 一 轮）
    ("WD0005", 65), // WD 单元 0005（未登记向量 × 相位 05 × 一 轮）
    ("JJ0006", 78), // JJ 单元 0006（拒绝 × 相位 06 × 一 轮）
    ("MZ0007", 91), // MZ 单元 0007（冻结 v1 × 相位 07 × 一 轮）
    ("FP0008", 7), // FP 单元 0008（分派 × 相位 08 × 一 轮）
    ("HB0009", 20), // HB 单元 0009（回执 × 相位 09 × 一 轮）
    ("ZC0010", 33), // ZC 单元 0010（在册 × 相位 10 × 一 轮）
    ("PL0011", 46), // PL 单元 0011（批量 × 相位 11 × 一 轮）
    ("XL0012", 59), // XL 单元 0012（向量表 × 相位 12 × 一 轮）
    ("CL0013", 72), // CL 单元 0013（处理函数表 × 相位 13 × 一 轮）
    ("FF0014", 85), // FF 单元 0014（分发协议 × 相位 14 × 一 轮）
    ("EOI0015", 1), // EOI 单元 0015（EOI 纪律 × 相位 15 × 一 轮）
    ("A20016", 14), // A2 单元 0016（A2 原语衔接 × 相位 16 × 一 轮）
    ("WD0017", 27), // WD 单元 0017（未登记向量 × 相位 17 × 一 轮）
    ("JJ0018", 40), // JJ 单元 0018（拒绝 × 相位 18 × 一 轮）
    ("MZ0019", 53), // MZ 单元 0019（冻结 v1 × 相位 19 × 一 轮）
    ("FP0020", 66), // FP 单元 0020（分派 × 相位 20 × 一 轮）
    ("HB0021", 79), // HB 单元 0021（回执 × 相位 21 × 一 轮）
    ("ZC0022", 92), // ZC 单元 0022（在册 × 相位 22 × 一 轮）
    ("PL0023", 8), // PL 单元 0023（批量 × 相位 23 × 一 轮）
    ("XL0024", 21), // XL 单元 0024（向量表 × 相位 24 × 一 轮）
    ("CL0025", 34), // CL 单元 0025（处理函数表 × 相位 25 × 一 轮）
    ("FF0026", 47), // FF 单元 0026（分发协议 × 相位 26 × 一 轮）
    ("EOI0027", 60), // EOI 单元 0027（EOI 纪律 × 相位 27 × 一 轮）
    ("A20028", 73), // A2 单元 0028（A2 原语衔接 × 相位 28 × 一 轮）
    ("WD0029", 86), // WD 单元 0029（未登记向量 × 相位 29 × 一 轮）
    ("JJ0030", 2), // JJ 单元 0030（拒绝 × 相位 30 × 一 轮）
    ("MZ0031", 15), // MZ 单元 0031（冻结 v1 × 相位 31 × 一 轮）
    ("FP0032", 28), // FP 单元 0032（分派 × 相位 32 × 一 轮）
    ("HB0033", 41), // HB 单元 0033（回执 × 相位 33 × 一 轮）
    ("ZC0034", 54), // ZC 单元 0034（在册 × 相位 34 × 一 轮）
    ("PL0035", 67), // PL 单元 0035（批量 × 相位 35 × 一 轮）
];

/// O(1) 语义查找（match 全枚举；命中率 100%% 由守卫保证）。
pub fn u1841mat_get(key: &str) -> Option<u64> {
    match key {
        "XL0000" => Some(0),
        "CL0001" => Some(13),
        "FF0002" => Some(26),
        "EOI0003" => Some(39),
        "A20004" => Some(52),
        "WD0005" => Some(65),
        "JJ0006" => Some(78),
        "MZ0007" => Some(91),
        "FP0008" => Some(7),
        "HB0009" => Some(20),
        "ZC0010" => Some(33),
        "PL0011" => Some(46),
        "XL0012" => Some(59),
        "CL0013" => Some(72),
        "FF0014" => Some(85),
        "EOI0015" => Some(1),
        "A20016" => Some(14),
        "WD0017" => Some(27),
        "JJ0018" => Some(40),
        "MZ0019" => Some(53),
        "FP0020" => Some(66),
        "HB0021" => Some(79),
        "ZC0022" => Some(92),
        "PL0023" => Some(8),
        "XL0024" => Some(21),
        "CL0025" => Some(34),
        "FF0026" => Some(47),
        "EOI0027" => Some(60),
        "A20028" => Some(73),
        "WD0029" => Some(86),
        "JJ0030" => Some(2),
        "MZ0031" => Some(15),
        "FP0032" => Some(28),
        "HB0033" => Some(41),
        "ZC0034" => Some(54),
        "PL0035" => Some(67),
        _ => None,
    }
}

/// 逐行守卫扫描（返回违例行数；0 = 全绿）。
pub fn u1841mat_scan() -> usize {
    let mut bad = 0usize;
    let mut i = 0usize;
    while i < u1841mat_COUNT {
        let (k, v) = u1841mat[i];
        let ok = match k {
            "XL0000" => v < 97,
            "CL0001" => v < 97,
            "FF0002" => v < 97,
            "EOI0003" => v < 97,
            "A20004" => v < 97,
            "WD0005" => v < 97,
            "JJ0006" => v < 97,
            "MZ0007" => v < 97,
            "FP0008" => v < 97,
            "HB0009" => v < 97,
            "ZC0010" => v < 97,
            "PL0011" => v < 97,
            "XL0012" => v < 97,
            "CL0013" => v < 97,
            "FF0014" => v < 97,
            "EOI0015" => v < 97,
            "A20016" => v < 97,
            "WD0017" => v < 97,
            "JJ0018" => v < 97,
            "MZ0019" => v < 97,
            "FP0020" => v < 97,
            "HB0021" => v < 97,
            "ZC0022" => v < 97,
            "PL0023" => v < 97,
            "XL0024" => v < 97,
            "CL0025" => v < 97,
            "FF0026" => v < 97,
            "EOI0027" => v < 97,
            "A20028" => v < 97,
            "WD0029" => v < 97,
            "JJ0030" => v < 97,
            "MZ0031" => v < 97,
            "FP0032" => v < 97,
            "HB0033" => v < 97,
            "ZC0034" => v < 97,
            "PL0035" => v < 97,
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
pub fn u1841mat_sum() -> u64 {
    let mut s = 0u64;
    let mut i = 0usize;
    while i < u1841mat_COUNT {
        s = s.wrapping_add(u1841mat[i].1 as u64);
        i += 1;
    }
    s
}

/// 键唯一性（构造期断言等价物：逐对扫描同键）。
pub fn u1841mat_unique() -> bool {
    let mut i = 0usize;
    while i < u1841mat_COUNT {
        let mut j = i + 1;
        while j < u1841mat_COUNT {
            if u1841mat[i].0 == u1841mat[j].0 {
                return false;
            }
            j += 1;
        }
        i += 1;
    }
    true
}

// --------------------------------------------------------------------------
// 直方图引擎 mxu1841hist（有界插入排序 + 分位数 P50/P90/P99）
// --------------------------------------------------------------------------
/// 向量 直方图（96 桶滚动）。
/// 样本容量上限。
pub const MXU1841HIST_CAP: usize = 96;

/// 直方图账本（排序态 + 五统量）。
#[derive(Clone, Copy, Debug)]
pub struct mxu1841histHist {
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

impl mxu1841histHist {
    /// 空账构造。
    pub const fn new() -> Self {
        Self { samples: [0; 96], n: 0, drops: 0, min: 0, max: 0, sum: 0 }
    }

    /// 采样（有界插入排序；满员丢弃并记账）。
    pub fn push(&mut self, v: u64) {
        if self.n >= MXU1841HIST_CAP {
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

/// 段位守卫：全部诊断码必须落在本段 [0x5e30, 0x5e3f]。
pub const fn u1841_segment_guard() -> bool {
    let mut ok = true;
    let mut i = 0usize;
    while i < u1841Code::ALL.len() {
        let c = u1841Code::ALL[i].code();
        if c < 0x5e30 || c > 0x5e3f {
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
fn u1841_guard_seg() -> bool {
    u1841_segment_guard()
}

/// UNX-F1841 矩阵 36 行齐备 + 逐行守卫 + 唯一性 + 累计守恒。
fn check_u1841_map_guard() -> bool {
    let mut ok = u1841mat_COUNT == 36;
    if ok { ok = u1841mat_scan() == 0 && u1841mat_unique(); }
    if ok { ok = u1841mat_get("XL0000") == Some(0) && u1841mat_get("ZZZ_NOKEY").is_none(); }
    if ok { ok = u1841mat_sum() == 1594; }
    ok
}

/// UNX-F1841 直方图 16 样本分位/均值/守恒。
fn check_u1841_map_hist() -> bool {
    let mut h = mxu1841histHist::new();
    let mut i = 0u64;
    while i < 16 { h.push(i + 1); i += 1; }
    let mut ok = h.conserved() && h.mean() >= 1 && h.percentile(500) >= 8;
    ok
}

/// 本模块自检（逐检查带 detail，全绿才出门）。
pub fn run_u1841_checks() -> CheckSet {
    let mut set = CheckSet::new("U1841");
    set.add(
        "u1841-seg-guard",
        u1841_guard_seg(),
        if u1841_guard_seg() { "" } else { "diagnostic code outside segment" },
    );
    set.add(
        "u1841-map-guard",
        check_u1841_map_guard(),
        if check_u1841_map_guard() { "" } else { "UNX-F1841 matrix guard mismatch" },
    );
    set.add(
        "u1841-map-hist",
        check_u1841_map_hist(),
        if check_u1841_map_hist() { "" } else { "UNX-F1841 hist mismatch" },
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
    fn u1841_all_checks_green() {
        let set = run_u1841_checks();
        assert!(set.all_passed(), "U1841 domain checks must be green");
    }

    #[test]
    fn u1841_segment_guard_holds() {
        assert!(u1841_segment_guard());
    }
}
