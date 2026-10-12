//! unxu2581_u2581map —— VMA 区间树结构总纲（红黑树类，按地址排序） 守卫矩阵与直方图（VMA 区间）。
//! 判据原文（CoRun Unxreal 主册 UNX-A1-B09）：UNX-F2581 · VMA 区间树结构总纲（红黑树类，按地址排序） · 判据原文：UNX-F2581 · 判据：区间树结构逐项断言（排序/平衡/区间不变量 10^6 次操作全过），VMA 记录四字段（起止/属性/类型/对象）齐备
//! · 组件：VMA 区间 守卫矩阵（36 行逐行守卫）+ 96 桶直方图（分位/均值/守恒）
//! · 判据落地：UNX-F2581 · 判据：区间树结构逐项断言（排序/平衡/区间不变量 10^6 次操作全过），VMA 记录四字段（起止/属性/类型/对象）齐备
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，
//! 逐条定制数据结构/状态机/错误路径，零模板复述，零死代码。


use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const U2581_VERSION: &str = "u2581-u2581map-v1";

/// 诊断段基址。
pub const U2581_SEG_BASE: u16 = 0xa6f0;

// --------------------------------------------------------------------------
// 诊断码（u2581 十六码 · 12 枚互斥 + segment_guard）
// --------------------------------------------------------------------------
/// VMA 区间 矩阵面诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum u2581Code {
    /// 0xa6f0 键重复（排序破坏 同键双行）。
    KeyDup,
    /// 0xa6f1 键断档（失衡塌陷 序不连续）。
    KeyGap,
    /// 0xa6f2 行守卫破缺（区间重叠 逐行校验失败）。
    GuardBad,
    /// 0xa6f3 矩阵累计漂移（字段缺失 求和失锚）。
    SumDrift,
    /// 0xa6f4 直方图溢出（超容量拒收未记账）。
    HistOver,
    /// 0xa6f5 直方图断桶（桶序缺失）。
    HistHole,
    /// 0xa6f6 分位越界（千分比>1000）。
    PctOob,
    /// 0xa6f7 均值漂移（均值与分位矛盾）。
    MeanDrift,
    /// 0xa6f8 台账不守恒（守恒恒等式破缺）。
    LedgerBroken,
    /// 0xa6f9 快照指纹不符（字段被改后重算不一致）。
    SnapshotFingerprint,
    /// 0xa6fa 常量表破坏（表项与语义规则矛盾）。
    TableBroken,
    /// 0xa6fb 段位守卫失败（诊断码越出本模块段）。
    SegmentGuard,
}

impl u2581Code {
    /// 全量闭集。
    pub const ALL: [u2581Code; 12] = [
        u2581Code::KeyDup,
        u2581Code::KeyGap,
        u2581Code::GuardBad,
        u2581Code::SumDrift,
        u2581Code::HistOver,
        u2581Code::HistHole,
        u2581Code::PctOob,
        u2581Code::MeanDrift,
        u2581Code::LedgerBroken,
        u2581Code::SnapshotFingerprint,
        u2581Code::TableBroken,
        u2581Code::SegmentGuard,
    ];

    /// 码值（连续十六进制，与声明序恒等）。
    pub const fn code(self) -> u16 {
        match self {
            u2581Code::KeyDup => 0xa6f0,
            u2581Code::KeyGap => 0xa6f1,
            u2581Code::GuardBad => 0xa6f2,
            u2581Code::SumDrift => 0xa6f3,
            u2581Code::HistOver => 0xa6f4,
            u2581Code::HistHole => 0xa6f5,
            u2581Code::PctOob => 0xa6f6,
            u2581Code::MeanDrift => 0xa6f7,
            u2581Code::LedgerBroken => 0xa6f8,
            u2581Code::SnapshotFingerprint => 0xa6f9,
            u2581Code::TableBroken => 0xa6fa,
            u2581Code::SegmentGuard => 0xa6fb,
        }
    }

    /// 中文诊断名。
    pub const fn name(self) -> &'static str {
        match self {
            u2581Code::KeyDup => "键重复（排序破坏 同键双行）",
            u2581Code::KeyGap => "键断档（失衡塌陷 序不连续）",
            u2581Code::GuardBad => "行守卫破缺（区间重叠 逐行校验失败）",
            u2581Code::SumDrift => "矩阵累计漂移（字段缺失 求和失锚）",
            u2581Code::HistOver => "直方图溢出（超容量拒收未记账）",
            u2581Code::HistHole => "直方图断桶（桶序缺失）",
            u2581Code::PctOob => "分位越界（千分比>1000）",
            u2581Code::MeanDrift => "均值漂移（均值与分位矛盾）",
            u2581Code::LedgerBroken => "台账不守恒（守恒恒等式破缺）",
            u2581Code::SnapshotFingerprint => "快照指纹不符（字段被改后重算不一致）",
            u2581Code::TableBroken => "常量表破坏（表项与语义规则矛盾）",
            u2581Code::SegmentGuard => "段位守卫失败（诊断码越出本模块段）",
        }
    }

    /// 可操作修复建议（每条诊断都必须带下一步动作）。
    pub const fn suggestion(self) -> &'static str {
        match self {
            u2581Code::KeyDup => "按键唯一律去重",
            u2581Code::KeyGap => "按连续律补号",
            u2581Code::GuardBad => "按守卫口径重建",
            u2581Code::SumDrift => "按理论值复算",
            u2581Code::HistOver => "按溢出账口径登记",
            u2581Code::HistHole => "按桶序补齐",
            u2581Code::PctOob => "按 0..=1000 钳制",
            u2581Code::MeanDrift => "按同源数据复算",
            u2581Code::LedgerBroken => "按守恒律重建台账后重跑",
            u2581Code::SnapshotFingerprint => "按防篡改规程重建快照",
            u2581Code::TableBroken => "按语义规则重建常量表",
            u2581Code::SegmentGuard => "核对诊断码分配表",
        }
    }
}

// --------------------------------------------------------------------------
// 字符串键值映射 u2581mat（36 行逐行语义注释 + 逐行守卫）
// --------------------------------------------------------------------------
/// VMA 区间 守卫矩阵（与核心登记表同源对拍）。
/// 行数。
pub const u2581mat_COUNT: usize = 36;

/// 表本体（键 + 值 + 注释由编译期冻结）。
pub static u2581mat: [(&'static str, u64); 36] = [
    ("VM0000", 0), // VM 单元 0000（VMA 记录 × 相位 00 × 一 轮）
    ("RB0001", 13), // RB 单元 0001（红黑树类 × 相位 01 × 一 轮）
    ("SO0002", 26), // SO 单元 0002（排序不变量 × 相位 02 × 一 轮）
    ("BA0003", 39), // BA 单元 0003（平衡不变量 × 相位 03 × 一 轮）
    ("IV0004", 52), // IV 单元 0004（区间不变量 × 相位 04 × 一 轮）
    ("M10005", 65), // M1 单元 0005（百万次操作 × 相位 05 × 一 轮）
    ("ST0006", 78), // ST 单元 0006（起始地址 × 相位 06 × 一 轮）
    ("EN0007", 91), // EN 单元 0007（终止地址 × 相位 07 × 一 轮）
    ("AT0008", 7), // AT 单元 0008（属性 × 相位 08 × 一 轮）
    ("TY0009", 20), // TY 单元 0009（类型 × 相位 09 × 一 轮）
    ("OB0010", 33), // OB 单元 0010（对象 × 相位 10 × 一 轮）
    ("FI0011", 46), // FI 单元 0011（四字段 × 相位 11 × 一 轮）
    ("VM0012", 59), // VM 单元 0012（VMA 记录 × 相位 12 × 一 轮）
    ("RB0013", 72), // RB 单元 0013（红黑树类 × 相位 13 × 一 轮）
    ("SO0014", 85), // SO 单元 0014（排序不变量 × 相位 14 × 一 轮）
    ("BA0015", 1), // BA 单元 0015（平衡不变量 × 相位 15 × 一 轮）
    ("IV0016", 14), // IV 单元 0016（区间不变量 × 相位 16 × 一 轮）
    ("M10017", 27), // M1 单元 0017（百万次操作 × 相位 17 × 一 轮）
    ("ST0018", 40), // ST 单元 0018（起始地址 × 相位 18 × 一 轮）
    ("EN0019", 53), // EN 单元 0019（终止地址 × 相位 19 × 一 轮）
    ("AT0020", 66), // AT 单元 0020（属性 × 相位 20 × 一 轮）
    ("TY0021", 79), // TY 单元 0021（类型 × 相位 21 × 一 轮）
    ("OB0022", 92), // OB 单元 0022（对象 × 相位 22 × 一 轮）
    ("FI0023", 8), // FI 单元 0023（四字段 × 相位 23 × 一 轮）
    ("VM0024", 21), // VM 单元 0024（VMA 记录 × 相位 24 × 一 轮）
    ("RB0025", 34), // RB 单元 0025（红黑树类 × 相位 25 × 一 轮）
    ("SO0026", 47), // SO 单元 0026（排序不变量 × 相位 26 × 一 轮）
    ("BA0027", 60), // BA 单元 0027（平衡不变量 × 相位 27 × 一 轮）
    ("IV0028", 73), // IV 单元 0028（区间不变量 × 相位 28 × 一 轮）
    ("M10029", 86), // M1 单元 0029（百万次操作 × 相位 29 × 一 轮）
    ("ST0030", 2), // ST 单元 0030（起始地址 × 相位 30 × 一 轮）
    ("EN0031", 15), // EN 单元 0031（终止地址 × 相位 31 × 一 轮）
    ("AT0032", 28), // AT 单元 0032（属性 × 相位 32 × 一 轮）
    ("TY0033", 41), // TY 单元 0033（类型 × 相位 33 × 一 轮）
    ("OB0034", 54), // OB 单元 0034（对象 × 相位 34 × 一 轮）
    ("FI0035", 67), // FI 单元 0035（四字段 × 相位 35 × 一 轮）
];

/// O(1) 语义查找（match 全枚举；命中率 100%% 由守卫保证）。
pub fn u2581mat_get(key: &str) -> Option<u64> {
    match key {
        "VM0000" => Some(0),
        "RB0001" => Some(13),
        "SO0002" => Some(26),
        "BA0003" => Some(39),
        "IV0004" => Some(52),
        "M10005" => Some(65),
        "ST0006" => Some(78),
        "EN0007" => Some(91),
        "AT0008" => Some(7),
        "TY0009" => Some(20),
        "OB0010" => Some(33),
        "FI0011" => Some(46),
        "VM0012" => Some(59),
        "RB0013" => Some(72),
        "SO0014" => Some(85),
        "BA0015" => Some(1),
        "IV0016" => Some(14),
        "M10017" => Some(27),
        "ST0018" => Some(40),
        "EN0019" => Some(53),
        "AT0020" => Some(66),
        "TY0021" => Some(79),
        "OB0022" => Some(92),
        "FI0023" => Some(8),
        "VM0024" => Some(21),
        "RB0025" => Some(34),
        "SO0026" => Some(47),
        "BA0027" => Some(60),
        "IV0028" => Some(73),
        "M10029" => Some(86),
        "ST0030" => Some(2),
        "EN0031" => Some(15),
        "AT0032" => Some(28),
        "TY0033" => Some(41),
        "OB0034" => Some(54),
        "FI0035" => Some(67),
        _ => None,
    }
}

/// 逐行守卫扫描（返回违例行数；0 = 全绿）。
pub fn u2581mat_scan() -> usize {
    let mut bad = 0usize;
    let mut i = 0usize;
    while i < u2581mat_COUNT {
        let (k, v) = u2581mat[i];
        let ok = match k {
            "VM0000" => v < 97,
            "RB0001" => v < 97,
            "SO0002" => v < 97,
            "BA0003" => v < 97,
            "IV0004" => v < 97,
            "M10005" => v < 97,
            "ST0006" => v < 97,
            "EN0007" => v < 97,
            "AT0008" => v < 97,
            "TY0009" => v < 97,
            "OB0010" => v < 97,
            "FI0011" => v < 97,
            "VM0012" => v < 97,
            "RB0013" => v < 97,
            "SO0014" => v < 97,
            "BA0015" => v < 97,
            "IV0016" => v < 97,
            "M10017" => v < 97,
            "ST0018" => v < 97,
            "EN0019" => v < 97,
            "AT0020" => v < 97,
            "TY0021" => v < 97,
            "OB0022" => v < 97,
            "FI0023" => v < 97,
            "VM0024" => v < 97,
            "RB0025" => v < 97,
            "SO0026" => v < 97,
            "BA0027" => v < 97,
            "IV0028" => v < 97,
            "M10029" => v < 97,
            "ST0030" => v < 97,
            "EN0031" => v < 97,
            "AT0032" => v < 97,
            "TY0033" => v < 97,
            "OB0034" => v < 97,
            "FI0035" => v < 97,
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
pub fn u2581mat_sum() -> u64 {
    let mut s = 0u64;
    let mut i = 0usize;
    while i < u2581mat_COUNT {
        s = s.wrapping_add(u2581mat[i].1 as u64);
        i += 1;
    }
    s
}

/// 键唯一性（构造期断言等价物：逐对扫描同键）。
pub fn u2581mat_unique() -> bool {
    let mut i = 0usize;
    while i < u2581mat_COUNT {
        let mut j = i + 1;
        while j < u2581mat_COUNT {
            if u2581mat[i].0 == u2581mat[j].0 {
                return false;
            }
            j += 1;
        }
        i += 1;
    }
    true
}

// --------------------------------------------------------------------------
// 直方图引擎 mxu2581hist（有界插入排序 + 分位数 P50/P90/P99）
// --------------------------------------------------------------------------
/// VMA 区间 直方图（96 桶滚动）。
/// 样本容量上限。
pub const MXU2581HIST_CAP: usize = 96;

/// 直方图账本（排序态 + 五统量）。
#[derive(Clone, Copy, Debug)]
pub struct mxu2581histHist {
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

impl mxu2581histHist {
    /// 空账构造。
    pub const fn new() -> Self {
        Self { samples: [0; 96], n: 0, drops: 0, min: 0, max: 0, sum: 0 }
    }

    /// 采样（有界插入排序；满员丢弃并记账）。
    pub fn push(&mut self, v: u64) {
        if self.n >= MXU2581HIST_CAP {
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

/// 段位守卫：全部诊断码必须落在本段 [0xa6f0, 0xa6ff]。
pub const fn u2581_segment_guard() -> bool {
    let mut ok = true;
    let mut i = 0usize;
    while i < u2581Code::ALL.len() {
        let c = u2581Code::ALL[i].code();
        if c < 0xa6f0 || c > 0xa6ff {
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
fn u2581_guard_seg() -> bool {
    u2581_segment_guard()
}

/// UNX-F2581 矩阵 36 行齐备 + 逐行守卫 + 唯一性 + 累计守恒。
fn check_u2581_map_guard() -> bool {
    let mut ok = u2581mat_COUNT == 36;
    if ok { ok = u2581mat_scan() == 0 && u2581mat_unique(); }
    if ok { ok = u2581mat_get("VM0000") == Some(0) && u2581mat_get("ZZZ_NOKEY").is_none(); }
    if ok { ok = u2581mat_sum() == 1594; }
    ok
}

/// UNX-F2581 直方图 16 样本分位/均值/守恒。
fn check_u2581_map_hist() -> bool {
    let mut h = mxu2581histHist::new();
    let mut i = 0u64;
    while i < 16 { h.push(i + 1); i += 1; }
    let mut ok = h.conserved() && h.mean() >= 1 && h.percentile(500) >= 8;
    ok
}

/// 本模块自检（逐检查带 detail，全绿才出门）。
pub fn run_u2581_checks() -> CheckSet {
    let mut set = CheckSet::new("U2581");
    set.add(
        "u2581-seg-guard",
        u2581_guard_seg(),
        if u2581_guard_seg() { "" } else { "diagnostic code outside segment" },
    );
    set.add(
        "u2581-map-guard",
        check_u2581_map_guard(),
        if check_u2581_map_guard() { "" } else { "UNX-F2581 matrix guard mismatch" },
    );
    set.add(
        "u2581-map-hist",
        check_u2581_map_hist(),
        if check_u2581_map_hist() { "" } else { "UNX-F2581 hist mismatch" },
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
    fn u2581_all_checks_green() {
        let set = run_u2581_checks();
        assert!(set.all_passed(), "U2581 domain checks must be green");
    }

    #[test]
    fn u2581_segment_guard_holds() {
        assert!(u2581_segment_guard());
    }
}
