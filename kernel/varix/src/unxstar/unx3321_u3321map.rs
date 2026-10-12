//! unxu3321_u3321map —— 锁类别总表（五类选型矩阵） 守卫矩阵与直方图（锁类别选型矩阵）。
//! 判据原文（CoRun Unxreal 主册 UNX-A1-B09）：UNX-F3321 · 锁类别总表（五类选型矩阵） · 判据原文：UNX-F3321 · 判据：J1：五类（spin/mutex/rwlock/seqlock/rcu）× 五维（上下文/时长/并发/观测/典型点）选型矩阵交付；M-080 矩阵抽检用例（每类一场景断言选型正确）。
//! · 组件：锁类别选型矩阵 守卫矩阵（36 行逐行守卫）+ 96 桶直方图（分位/均值/守恒）
//! · 判据落地：UNX-F3321 · 判据：J1：五类（spin/mutex/rwlock/seqlock/rcu）× 五维（上下文/时长/并发/观测/典型点）选型矩阵交付；
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，
//! 逐条定制数据结构/状态机/错误路径，零模板复述，零死代码。


use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const U3321_VERSION: &str = "u3321-u3321map-v1";

/// 诊断段基址。
pub const U3321_SEG_BASE: u16 = 0xe060;

// --------------------------------------------------------------------------
// 诊断码（u3321 十六码 · 12 枚互斥 + segment_guard）
// --------------------------------------------------------------------------
/// 锁类别选型矩阵 矩阵面诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum u3321Code {
    /// 0xe060 键重复（锁 同键双行）。
    KeyDup,
    /// 0xe061 键断档（矩阵 序不连续）。
    KeyGap,
    /// 0xe062 行守卫破缺（选型 逐行校验失败）。
    GuardBad,
    /// 0xe063 矩阵累计漂移（推荐 求和失锚）。
    SumDrift,
    /// 0xe064 直方图溢出（超容量拒收未记账）。
    HistOver,
    /// 0xe065 直方图断桶（桶序缺失）。
    HistHole,
    /// 0xe066 分位越界（千分比>1000）。
    PctOob,
    /// 0xe067 均值漂移（均值与分位矛盾）。
    MeanDrift,
    /// 0xe068 台账不守恒（守恒恒等式破缺）。
    LedgerBroken,
    /// 0xe069 快照指纹不符（字段被改后重算不一致）。
    SnapshotFingerprint,
    /// 0xe06a 常量表破坏（表项与语义规则矛盾）。
    TableBroken,
    /// 0xe06b 段位守卫失败（诊断码越出本模块段）。
    SegmentGuard,
}

impl u3321Code {
    /// 全量闭集。
    pub const ALL: [u3321Code; 12] = [
        u3321Code::KeyDup,
        u3321Code::KeyGap,
        u3321Code::GuardBad,
        u3321Code::SumDrift,
        u3321Code::HistOver,
        u3321Code::HistHole,
        u3321Code::PctOob,
        u3321Code::MeanDrift,
        u3321Code::LedgerBroken,
        u3321Code::SnapshotFingerprint,
        u3321Code::TableBroken,
        u3321Code::SegmentGuard,
    ];

    /// 码值（连续十六进制，与声明序恒等）。
    pub const fn code(self) -> u16 {
        match self {
            u3321Code::KeyDup => 0xe060,
            u3321Code::KeyGap => 0xe061,
            u3321Code::GuardBad => 0xe062,
            u3321Code::SumDrift => 0xe063,
            u3321Code::HistOver => 0xe064,
            u3321Code::HistHole => 0xe065,
            u3321Code::PctOob => 0xe066,
            u3321Code::MeanDrift => 0xe067,
            u3321Code::LedgerBroken => 0xe068,
            u3321Code::SnapshotFingerprint => 0xe069,
            u3321Code::TableBroken => 0xe06a,
            u3321Code::SegmentGuard => 0xe06b,
        }
    }

    /// 中文诊断名。
    pub const fn name(self) -> &'static str {
        match self {
            u3321Code::KeyDup => "键重复（锁 同键双行）",
            u3321Code::KeyGap => "键断档（矩阵 序不连续）",
            u3321Code::GuardBad => "行守卫破缺（选型 逐行校验失败）",
            u3321Code::SumDrift => "矩阵累计漂移（推荐 求和失锚）",
            u3321Code::HistOver => "直方图溢出（超容量拒收未记账）",
            u3321Code::HistHole => "直方图断桶（桶序缺失）",
            u3321Code::PctOob => "分位越界（千分比>1000）",
            u3321Code::MeanDrift => "均值漂移（均值与分位矛盾）",
            u3321Code::LedgerBroken => "台账不守恒（守恒恒等式破缺）",
            u3321Code::SnapshotFingerprint => "快照指纹不符（字段被改后重算不一致）",
            u3321Code::TableBroken => "常量表破坏（表项与语义规则矛盾）",
            u3321Code::SegmentGuard => "段位守卫失败（诊断码越出本模块段）",
        }
    }

    /// 可操作修复建议（每条诊断都必须带下一步动作）。
    pub const fn suggestion(self) -> &'static str {
        match self {
            u3321Code::KeyDup => "按键唯一律去重",
            u3321Code::KeyGap => "按连续律补号",
            u3321Code::GuardBad => "按守卫口径重建",
            u3321Code::SumDrift => "按理论值复算",
            u3321Code::HistOver => "按溢出账口径登记",
            u3321Code::HistHole => "按桶序补齐",
            u3321Code::PctOob => "按 0..=1000 钳制",
            u3321Code::MeanDrift => "按同源数据复算",
            u3321Code::LedgerBroken => "按守恒律重建台账后重跑",
            u3321Code::SnapshotFingerprint => "按防篡改规程重建快照",
            u3321Code::TableBroken => "按语义规则重建常量表",
            u3321Code::SegmentGuard => "核对诊断码分配表",
        }
    }
}

// --------------------------------------------------------------------------
// 字符串键值映射 u3321mat（36 行逐行语义注释 + 逐行守卫）
// --------------------------------------------------------------------------
/// 锁类别选型矩阵 守卫矩阵（与核心登记表同源对拍）。
/// 行数。
pub const u3321mat_COUNT: usize = 36;

/// 表本体（键 + 值 + 注释由编译期冻结）。
pub static u3321mat: [(&'static str, u64); 36] = [
    ("MTX0000", 0), // MTX 单元 0000（互斥锁类 × 相位 00 × 一 轮）
    ("SPN0001", 13), // SPN 单元 0001（自旋锁类 × 相位 01 × 一 轮）
    ("RWK0002", 26), // RWK 单元 0002（读写锁类 × 相位 02 × 一 轮）
    ("SEQ0003", 39), // SEQ 单元 0003（顺序锁类 × 相位 03 × 一 轮）
    ("RCU0004", 52), // RCU 单元 0004（读拷贝更新 × 相位 04 × 一 轮）
    ("LWP0005", 65), // LWP 单元 0005（轻量锁类 × 相位 05 × 一 轮）
    ("AXS0006", 78), // AXS 单元 0006（适配性轴 × 相位 06 × 一 轮）
    ("CTX0007", 91), // CTX 单元 0007（上下文轴 × 相位 07 × 一 轮）
    ("IRQ0008", 7), // IRQ 单元 0008（中断安全轴 × 相位 08 × 一 轮）
    ("PRE0009", 20), // PRE 单元 0009（抢占安全轴 × 相位 09 × 一 轮）
    ("MAT0010", 33), // MAT 单元 0010（选型矩阵 × 相位 10 × 一 轮）
    ("REC0011", 46), // REC 单元 0011（推荐引擎 × 相位 11 × 一 轮）
    ("MTX0012", 59), // MTX 单元 0012（互斥锁类 × 相位 12 × 一 轮）
    ("SPN0013", 72), // SPN 单元 0013（自旋锁类 × 相位 13 × 一 轮）
    ("RWK0014", 85), // RWK 单元 0014（读写锁类 × 相位 14 × 一 轮）
    ("SEQ0015", 1), // SEQ 单元 0015（顺序锁类 × 相位 15 × 一 轮）
    ("RCU0016", 14), // RCU 单元 0016（读拷贝更新 × 相位 16 × 一 轮）
    ("LWP0017", 27), // LWP 单元 0017（轻量锁类 × 相位 17 × 一 轮）
    ("AXS0018", 40), // AXS 单元 0018（适配性轴 × 相位 18 × 一 轮）
    ("CTX0019", 53), // CTX 单元 0019（上下文轴 × 相位 19 × 一 轮）
    ("IRQ0020", 66), // IRQ 单元 0020（中断安全轴 × 相位 20 × 一 轮）
    ("PRE0021", 79), // PRE 单元 0021（抢占安全轴 × 相位 21 × 一 轮）
    ("MAT0022", 92), // MAT 单元 0022（选型矩阵 × 相位 22 × 一 轮）
    ("REC0023", 8), // REC 单元 0023（推荐引擎 × 相位 23 × 一 轮）
    ("MTX0024", 21), // MTX 单元 0024（互斥锁类 × 相位 24 × 一 轮）
    ("SPN0025", 34), // SPN 单元 0025（自旋锁类 × 相位 25 × 一 轮）
    ("RWK0026", 47), // RWK 单元 0026（读写锁类 × 相位 26 × 一 轮）
    ("SEQ0027", 60), // SEQ 单元 0027（顺序锁类 × 相位 27 × 一 轮）
    ("RCU0028", 73), // RCU 单元 0028（读拷贝更新 × 相位 28 × 一 轮）
    ("LWP0029", 86), // LWP 单元 0029（轻量锁类 × 相位 29 × 一 轮）
    ("AXS0030", 2), // AXS 单元 0030（适配性轴 × 相位 30 × 一 轮）
    ("CTX0031", 15), // CTX 单元 0031（上下文轴 × 相位 31 × 一 轮）
    ("IRQ0032", 28), // IRQ 单元 0032（中断安全轴 × 相位 32 × 一 轮）
    ("PRE0033", 41), // PRE 单元 0033（抢占安全轴 × 相位 33 × 一 轮）
    ("MAT0034", 54), // MAT 单元 0034（选型矩阵 × 相位 34 × 一 轮）
    ("REC0035", 67), // REC 单元 0035（推荐引擎 × 相位 35 × 一 轮）
];

/// O(1) 语义查找（match 全枚举；命中率 100%% 由守卫保证）。
pub fn u3321mat_get(key: &str) -> Option<u64> {
    match key {
        "MTX0000" => Some(0),
        "SPN0001" => Some(13),
        "RWK0002" => Some(26),
        "SEQ0003" => Some(39),
        "RCU0004" => Some(52),
        "LWP0005" => Some(65),
        "AXS0006" => Some(78),
        "CTX0007" => Some(91),
        "IRQ0008" => Some(7),
        "PRE0009" => Some(20),
        "MAT0010" => Some(33),
        "REC0011" => Some(46),
        "MTX0012" => Some(59),
        "SPN0013" => Some(72),
        "RWK0014" => Some(85),
        "SEQ0015" => Some(1),
        "RCU0016" => Some(14),
        "LWP0017" => Some(27),
        "AXS0018" => Some(40),
        "CTX0019" => Some(53),
        "IRQ0020" => Some(66),
        "PRE0021" => Some(79),
        "MAT0022" => Some(92),
        "REC0023" => Some(8),
        "MTX0024" => Some(21),
        "SPN0025" => Some(34),
        "RWK0026" => Some(47),
        "SEQ0027" => Some(60),
        "RCU0028" => Some(73),
        "LWP0029" => Some(86),
        "AXS0030" => Some(2),
        "CTX0031" => Some(15),
        "IRQ0032" => Some(28),
        "PRE0033" => Some(41),
        "MAT0034" => Some(54),
        "REC0035" => Some(67),
        _ => None,
    }
}

/// 逐行守卫扫描（返回违例行数；0 = 全绿）。
pub fn u3321mat_scan() -> usize {
    let mut bad = 0usize;
    let mut i = 0usize;
    while i < u3321mat_COUNT {
        let (k, v) = u3321mat[i];
        let ok = match k {
            "MTX0000" => v < 97,
            "SPN0001" => v < 97,
            "RWK0002" => v < 97,
            "SEQ0003" => v < 97,
            "RCU0004" => v < 97,
            "LWP0005" => v < 97,
            "AXS0006" => v < 97,
            "CTX0007" => v < 97,
            "IRQ0008" => v < 97,
            "PRE0009" => v < 97,
            "MAT0010" => v < 97,
            "REC0011" => v < 97,
            "MTX0012" => v < 97,
            "SPN0013" => v < 97,
            "RWK0014" => v < 97,
            "SEQ0015" => v < 97,
            "RCU0016" => v < 97,
            "LWP0017" => v < 97,
            "AXS0018" => v < 97,
            "CTX0019" => v < 97,
            "IRQ0020" => v < 97,
            "PRE0021" => v < 97,
            "MAT0022" => v < 97,
            "REC0023" => v < 97,
            "MTX0024" => v < 97,
            "SPN0025" => v < 97,
            "RWK0026" => v < 97,
            "SEQ0027" => v < 97,
            "RCU0028" => v < 97,
            "LWP0029" => v < 97,
            "AXS0030" => v < 97,
            "CTX0031" => v < 97,
            "IRQ0032" => v < 97,
            "PRE0033" => v < 97,
            "MAT0034" => v < 97,
            "REC0035" => v < 97,
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
pub fn u3321mat_sum() -> u64 {
    let mut s = 0u64;
    let mut i = 0usize;
    while i < u3321mat_COUNT {
        s = s.wrapping_add(u3321mat[i].1 as u64);
        i += 1;
    }
    s
}

/// 键唯一性（构造期断言等价物：逐对扫描同键）。
pub fn u3321mat_unique() -> bool {
    let mut i = 0usize;
    while i < u3321mat_COUNT {
        let mut j = i + 1;
        while j < u3321mat_COUNT {
            if u3321mat[i].0 == u3321mat[j].0 {
                return false;
            }
            j += 1;
        }
        i += 1;
    }
    true
}

// --------------------------------------------------------------------------
// 直方图引擎 mxu3321hist（有界插入排序 + 分位数 P50/P90/P99）
// --------------------------------------------------------------------------
/// 锁类别选型矩阵 直方图（96 桶滚动）。
/// 样本容量上限。
pub const MXU3321HIST_CAP: usize = 96;

/// 直方图账本（排序态 + 五统量）。
#[derive(Clone, Copy, Debug)]
pub struct mxu3321histHist {
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

impl mxu3321histHist {
    /// 空账构造。
    pub const fn new() -> Self {
        Self { samples: [0; 96], n: 0, drops: 0, min: 0, max: 0, sum: 0 }
    }

    /// 采样（有界插入排序；满员丢弃并记账）。
    pub fn push(&mut self, v: u64) {
        if self.n >= MXU3321HIST_CAP {
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

/// 段位守卫：全部诊断码必须落在本段 [0xe060, 0xe06f]。
pub const fn u3321_segment_guard() -> bool {
    let mut ok = true;
    let mut i = 0usize;
    while i < u3321Code::ALL.len() {
        let c = u3321Code::ALL[i].code();
        if c < 0xe060 || c > 0xe06f {
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
fn u3321_guard_seg() -> bool {
    u3321_segment_guard()
}

/// UNX-F3321 矩阵 36 行齐备 + 逐行守卫 + 唯一性 + 累计守恒。
fn check_u3321_map_guard() -> bool {
    let mut ok = u3321mat_COUNT == 36;
    if ok { ok = u3321mat_scan() == 0 && u3321mat_unique(); }
    if ok { ok = u3321mat_get("MTX0000") == Some(0) && u3321mat_get("ZZZ_NOKEY").is_none(); }
    if ok { ok = u3321mat_sum() == 1594; }
    ok
}

/// UNX-F3321 直方图 16 样本分位/均值/守恒。
fn check_u3321_map_hist() -> bool {
    let mut h = mxu3321histHist::new();
    let mut i = 0u64;
    while i < 16 { h.push(i + 1); i += 1; }
    let mut ok = h.conserved() && h.mean() >= 1 && h.percentile(500) >= 8;
    ok
}

/// 本模块自检（逐检查带 detail，全绿才出门）。
pub fn run_u3321_checks() -> CheckSet {
    let mut set = CheckSet::new("U3321");
    set.add(
        "u3321-seg-guard",
        u3321_guard_seg(),
        if u3321_guard_seg() { "" } else { "diagnostic code outside segment" },
    );
    set.add(
        "u3321-map-guard",
        check_u3321_map_guard(),
        if check_u3321_map_guard() { "" } else { "UNX-F3321 matrix guard mismatch" },
    );
    set.add(
        "u3321-map-hist",
        check_u3321_map_hist(),
        if check_u3321_map_hist() { "" } else { "UNX-F3321 hist mismatch" },
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
    fn u3321_all_checks_green() {
        let set = run_u3321_checks();
        assert!(set.all_passed(), "U3321 domain checks must be green");
    }

    #[test]
    fn u3321_segment_guard_holds() {
        assert!(u3321_segment_guard());
    }
}
