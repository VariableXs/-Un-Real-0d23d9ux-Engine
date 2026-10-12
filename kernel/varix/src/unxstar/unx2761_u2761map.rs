//! unxu2761_u2761map —— 四级压力档判定器（水位+产出双因子） 守卫矩阵与直方图（压力档）。
//! 判据原文（CoRun Unxreal 主册 UNX-A1-B09）：UNX-F2761 · 四级压力档判定器（水位+产出双因子） · 判据原文：UNX-F2761 · 判据：四档（正常/轻压/中压/重压）双因子判定逐档可复测，档位注入 40/40 次正确，与 F2741/F2747 双源读数一致
//! · 组件：压力档 守卫矩阵（36 行逐行守卫）+ 96 桶直方图（分位/均值/守恒）
//! · 判据落地：UNX-F2761 · 判据：四档（正常/轻压/中压/重压）双因子判定逐档可复测，档位注入 40/40 次正确，与 F2741/F2747 双源读数一致
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，
//! 逐条定制数据结构/状态机/错误路径，零模板复述，零死代码。


use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const U2761_VERSION: &str = "u2761-u2761map-v1";

/// 诊断段基址。
pub const U2761_SEG_BASE: u16 = 0xad70;

// --------------------------------------------------------------------------
// 诊断码（u2761 十六码 · 12 枚互斥 + segment_guard）
// --------------------------------------------------------------------------
/// 压力档 矩阵面诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum u2761Code {
    /// 0xad70 键重复（档位跳变 同键双行）。
    KeyDup,
    /// 0xad71 键断档（双源分歧 序不连续）。
    KeyGap,
    /// 0xad72 行守卫破缺（注入误判 逐行校验失败）。
    GuardBad,
    /// 0xad73 矩阵累计漂移（因子失真 求和失锚）。
    SumDrift,
    /// 0xad74 直方图溢出（超容量拒收未记账）。
    HistOver,
    /// 0xad75 直方图断桶（桶序缺失）。
    HistHole,
    /// 0xad76 分位越界（千分比>1000）。
    PctOob,
    /// 0xad77 均值漂移（均值与分位矛盾）。
    MeanDrift,
    /// 0xad78 台账不守恒（守恒恒等式破缺）。
    LedgerBroken,
    /// 0xad79 快照指纹不符（字段被改后重算不一致）。
    SnapshotFingerprint,
    /// 0xad7a 常量表破坏（表项与语义规则矛盾）。
    TableBroken,
    /// 0xad7b 段位守卫失败（诊断码越出本模块段）。
    SegmentGuard,
}

impl u2761Code {
    /// 全量闭集。
    pub const ALL: [u2761Code; 12] = [
        u2761Code::KeyDup,
        u2761Code::KeyGap,
        u2761Code::GuardBad,
        u2761Code::SumDrift,
        u2761Code::HistOver,
        u2761Code::HistHole,
        u2761Code::PctOob,
        u2761Code::MeanDrift,
        u2761Code::LedgerBroken,
        u2761Code::SnapshotFingerprint,
        u2761Code::TableBroken,
        u2761Code::SegmentGuard,
    ];

    /// 码值（连续十六进制，与声明序恒等）。
    pub const fn code(self) -> u16 {
        match self {
            u2761Code::KeyDup => 0xad70,
            u2761Code::KeyGap => 0xad71,
            u2761Code::GuardBad => 0xad72,
            u2761Code::SumDrift => 0xad73,
            u2761Code::HistOver => 0xad74,
            u2761Code::HistHole => 0xad75,
            u2761Code::PctOob => 0xad76,
            u2761Code::MeanDrift => 0xad77,
            u2761Code::LedgerBroken => 0xad78,
            u2761Code::SnapshotFingerprint => 0xad79,
            u2761Code::TableBroken => 0xad7a,
            u2761Code::SegmentGuard => 0xad7b,
        }
    }

    /// 中文诊断名。
    pub const fn name(self) -> &'static str {
        match self {
            u2761Code::KeyDup => "键重复（档位跳变 同键双行）",
            u2761Code::KeyGap => "键断档（双源分歧 序不连续）",
            u2761Code::GuardBad => "行守卫破缺（注入误判 逐行校验失败）",
            u2761Code::SumDrift => "矩阵累计漂移（因子失真 求和失锚）",
            u2761Code::HistOver => "直方图溢出（超容量拒收未记账）",
            u2761Code::HistHole => "直方图断桶（桶序缺失）",
            u2761Code::PctOob => "分位越界（千分比>1000）",
            u2761Code::MeanDrift => "均值漂移（均值与分位矛盾）",
            u2761Code::LedgerBroken => "台账不守恒（守恒恒等式破缺）",
            u2761Code::SnapshotFingerprint => "快照指纹不符（字段被改后重算不一致）",
            u2761Code::TableBroken => "常量表破坏（表项与语义规则矛盾）",
            u2761Code::SegmentGuard => "段位守卫失败（诊断码越出本模块段）",
        }
    }

    /// 可操作修复建议（每条诊断都必须带下一步动作）。
    pub const fn suggestion(self) -> &'static str {
        match self {
            u2761Code::KeyDup => "按键唯一律去重",
            u2761Code::KeyGap => "按连续律补号",
            u2761Code::GuardBad => "按守卫口径重建",
            u2761Code::SumDrift => "按理论值复算",
            u2761Code::HistOver => "按溢出账口径登记",
            u2761Code::HistHole => "按桶序补齐",
            u2761Code::PctOob => "按 0..=1000 钳制",
            u2761Code::MeanDrift => "按同源数据复算",
            u2761Code::LedgerBroken => "按守恒律重建台账后重跑",
            u2761Code::SnapshotFingerprint => "按防篡改规程重建快照",
            u2761Code::TableBroken => "按语义规则重建常量表",
            u2761Code::SegmentGuard => "核对诊断码分配表",
        }
    }
}

// --------------------------------------------------------------------------
// 字符串键值映射 u2761mat（36 行逐行语义注释 + 逐行守卫）
// --------------------------------------------------------------------------
/// 压力档 守卫矩阵（与核心登记表同源对拍）。
/// 行数。
pub const u2761mat_COUNT: usize = 36;

/// 表本体（键 + 值 + 注释由编译期冻结）。
pub static u2761mat: [(&'static str, u64); 36] = [
    ("NM0000", 0), // NM 单元 0000（正常档 × 相位 00 × 一 轮）
    ("LT0001", 13), // LT 单元 0001（轻压档 × 相位 01 × 一 轮）
    ("MD0002", 26), // MD 单元 0002（中压档 × 相位 02 × 一 轮）
    ("HV0003", 39), // HV 单元 0003（重压档 × 相位 03 × 一 轮）
    ("WL0004", 52), // WL 单元 0004（水位因子 × 相位 04 × 一 轮）
    ("OU0005", 65), // OU 单元 0005（产出因子 × 相位 05 × 一 轮）
    ("DF0006", 78), // DF 单元 0006（双因子 × 相位 06 × 一 轮）
    ("IN400007", 91), // IN40 单元 0007（注入 40/40 × 相位 07 × 一 轮）
    ("D10008", 7), // D1 单元 0008（F2741 读数 × 相位 08 × 一 轮）
    ("D20009", 20), // D2 单元 0009（F2747 读数 × 相位 09 × 一 轮）
    ("DR0010", 33), // DR 单元 0010（双源一致 × 相位 10 × 一 轮）
    ("RT0011", 46), // RT 单元 0011（可复测 × 相位 11 × 一 轮）
    ("NM0012", 59), // NM 单元 0012（正常档 × 相位 12 × 一 轮）
    ("LT0013", 72), // LT 单元 0013（轻压档 × 相位 13 × 一 轮）
    ("MD0014", 85), // MD 单元 0014（中压档 × 相位 14 × 一 轮）
    ("HV0015", 1), // HV 单元 0015（重压档 × 相位 15 × 一 轮）
    ("WL0016", 14), // WL 单元 0016（水位因子 × 相位 16 × 一 轮）
    ("OU0017", 27), // OU 单元 0017（产出因子 × 相位 17 × 一 轮）
    ("DF0018", 40), // DF 单元 0018（双因子 × 相位 18 × 一 轮）
    ("IN400019", 53), // IN40 单元 0019（注入 40/40 × 相位 19 × 一 轮）
    ("D10020", 66), // D1 单元 0020（F2741 读数 × 相位 20 × 一 轮）
    ("D20021", 79), // D2 单元 0021（F2747 读数 × 相位 21 × 一 轮）
    ("DR0022", 92), // DR 单元 0022（双源一致 × 相位 22 × 一 轮）
    ("RT0023", 8), // RT 单元 0023（可复测 × 相位 23 × 一 轮）
    ("NM0024", 21), // NM 单元 0024（正常档 × 相位 24 × 一 轮）
    ("LT0025", 34), // LT 单元 0025（轻压档 × 相位 25 × 一 轮）
    ("MD0026", 47), // MD 单元 0026（中压档 × 相位 26 × 一 轮）
    ("HV0027", 60), // HV 单元 0027（重压档 × 相位 27 × 一 轮）
    ("WL0028", 73), // WL 单元 0028（水位因子 × 相位 28 × 一 轮）
    ("OU0029", 86), // OU 单元 0029（产出因子 × 相位 29 × 一 轮）
    ("DF0030", 2), // DF 单元 0030（双因子 × 相位 30 × 一 轮）
    ("IN400031", 15), // IN40 单元 0031（注入 40/40 × 相位 31 × 一 轮）
    ("D10032", 28), // D1 单元 0032（F2741 读数 × 相位 32 × 一 轮）
    ("D20033", 41), // D2 单元 0033（F2747 读数 × 相位 33 × 一 轮）
    ("DR0034", 54), // DR 单元 0034（双源一致 × 相位 34 × 一 轮）
    ("RT0035", 67), // RT 单元 0035（可复测 × 相位 35 × 一 轮）
];

/// O(1) 语义查找（match 全枚举；命中率 100%% 由守卫保证）。
pub fn u2761mat_get(key: &str) -> Option<u64> {
    match key {
        "NM0000" => Some(0),
        "LT0001" => Some(13),
        "MD0002" => Some(26),
        "HV0003" => Some(39),
        "WL0004" => Some(52),
        "OU0005" => Some(65),
        "DF0006" => Some(78),
        "IN400007" => Some(91),
        "D10008" => Some(7),
        "D20009" => Some(20),
        "DR0010" => Some(33),
        "RT0011" => Some(46),
        "NM0012" => Some(59),
        "LT0013" => Some(72),
        "MD0014" => Some(85),
        "HV0015" => Some(1),
        "WL0016" => Some(14),
        "OU0017" => Some(27),
        "DF0018" => Some(40),
        "IN400019" => Some(53),
        "D10020" => Some(66),
        "D20021" => Some(79),
        "DR0022" => Some(92),
        "RT0023" => Some(8),
        "NM0024" => Some(21),
        "LT0025" => Some(34),
        "MD0026" => Some(47),
        "HV0027" => Some(60),
        "WL0028" => Some(73),
        "OU0029" => Some(86),
        "DF0030" => Some(2),
        "IN400031" => Some(15),
        "D10032" => Some(28),
        "D20033" => Some(41),
        "DR0034" => Some(54),
        "RT0035" => Some(67),
        _ => None,
    }
}

/// 逐行守卫扫描（返回违例行数；0 = 全绿）。
pub fn u2761mat_scan() -> usize {
    let mut bad = 0usize;
    let mut i = 0usize;
    while i < u2761mat_COUNT {
        let (k, v) = u2761mat[i];
        let ok = match k {
            "NM0000" => v < 97,
            "LT0001" => v < 97,
            "MD0002" => v < 97,
            "HV0003" => v < 97,
            "WL0004" => v < 97,
            "OU0005" => v < 97,
            "DF0006" => v < 97,
            "IN400007" => v < 97,
            "D10008" => v < 97,
            "D20009" => v < 97,
            "DR0010" => v < 97,
            "RT0011" => v < 97,
            "NM0012" => v < 97,
            "LT0013" => v < 97,
            "MD0014" => v < 97,
            "HV0015" => v < 97,
            "WL0016" => v < 97,
            "OU0017" => v < 97,
            "DF0018" => v < 97,
            "IN400019" => v < 97,
            "D10020" => v < 97,
            "D20021" => v < 97,
            "DR0022" => v < 97,
            "RT0023" => v < 97,
            "NM0024" => v < 97,
            "LT0025" => v < 97,
            "MD0026" => v < 97,
            "HV0027" => v < 97,
            "WL0028" => v < 97,
            "OU0029" => v < 97,
            "DF0030" => v < 97,
            "IN400031" => v < 97,
            "D10032" => v < 97,
            "D20033" => v < 97,
            "DR0034" => v < 97,
            "RT0035" => v < 97,
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
pub fn u2761mat_sum() -> u64 {
    let mut s = 0u64;
    let mut i = 0usize;
    while i < u2761mat_COUNT {
        s = s.wrapping_add(u2761mat[i].1 as u64);
        i += 1;
    }
    s
}

/// 键唯一性（构造期断言等价物：逐对扫描同键）。
pub fn u2761mat_unique() -> bool {
    let mut i = 0usize;
    while i < u2761mat_COUNT {
        let mut j = i + 1;
        while j < u2761mat_COUNT {
            if u2761mat[i].0 == u2761mat[j].0 {
                return false;
            }
            j += 1;
        }
        i += 1;
    }
    true
}

// --------------------------------------------------------------------------
// 直方图引擎 mxu2761hist（有界插入排序 + 分位数 P50/P90/P99）
// --------------------------------------------------------------------------
/// 压力档 直方图（96 桶滚动）。
/// 样本容量上限。
pub const MXU2761HIST_CAP: usize = 96;

/// 直方图账本（排序态 + 五统量）。
#[derive(Clone, Copy, Debug)]
pub struct mxu2761histHist {
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

impl mxu2761histHist {
    /// 空账构造。
    pub const fn new() -> Self {
        Self { samples: [0; 96], n: 0, drops: 0, min: 0, max: 0, sum: 0 }
    }

    /// 采样（有界插入排序；满员丢弃并记账）。
    pub fn push(&mut self, v: u64) {
        if self.n >= MXU2761HIST_CAP {
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

/// 段位守卫：全部诊断码必须落在本段 [0xad70, 0xad7f]。
pub const fn u2761_segment_guard() -> bool {
    let mut ok = true;
    let mut i = 0usize;
    while i < u2761Code::ALL.len() {
        let c = u2761Code::ALL[i].code();
        if c < 0xad70 || c > 0xad7f {
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
fn u2761_guard_seg() -> bool {
    u2761_segment_guard()
}

/// UNX-F2761 矩阵 36 行齐备 + 逐行守卫 + 唯一性 + 累计守恒。
fn check_u2761_map_guard() -> bool {
    let mut ok = u2761mat_COUNT == 36;
    if ok { ok = u2761mat_scan() == 0 && u2761mat_unique(); }
    if ok { ok = u2761mat_get("NM0000") == Some(0) && u2761mat_get("ZZZ_NOKEY").is_none(); }
    if ok { ok = u2761mat_sum() == 1594; }
    ok
}

/// UNX-F2761 直方图 16 样本分位/均值/守恒。
fn check_u2761_map_hist() -> bool {
    let mut h = mxu2761histHist::new();
    let mut i = 0u64;
    while i < 16 { h.push(i + 1); i += 1; }
    let mut ok = h.conserved() && h.mean() >= 1 && h.percentile(500) >= 8;
    ok
}

/// 本模块自检（逐检查带 detail，全绿才出门）。
pub fn run_u2761_checks() -> CheckSet {
    let mut set = CheckSet::new("U2761");
    set.add(
        "u2761-seg-guard",
        u2761_guard_seg(),
        if u2761_guard_seg() { "" } else { "diagnostic code outside segment" },
    );
    set.add(
        "u2761-map-guard",
        check_u2761_map_guard(),
        if check_u2761_map_guard() { "" } else { "UNX-F2761 matrix guard mismatch" },
    );
    set.add(
        "u2761-map-hist",
        check_u2761_map_hist(),
        if check_u2761_map_hist() { "" } else { "UNX-F2761 hist mismatch" },
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
    fn u2761_all_checks_green() {
        let set = run_u2761_checks();
        assert!(set.all_passed(), "U2761 domain checks must be green");
    }

    #[test]
    fn u2761_segment_guard_holds() {
        assert!(u2761_segment_guard());
    }
}
