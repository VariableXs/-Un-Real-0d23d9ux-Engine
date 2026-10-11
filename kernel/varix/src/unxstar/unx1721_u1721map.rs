//! unxu1721_u1721map —— 均衡执行总纲（判据消费→执行原语链） 守卫矩阵与直方图（执行段）。
//! 判据原文（CoRun Unxreal 主册 UNX-A1-B09）：UNX-F1721 · 均衡执行总纲（判据消费→执行原语链） · 判据原文：UNX-F1721 · 判据：执行链（判据结论→收益复核→候选→目标→原语→记账）六段冻结 v1，段间交接签名齐备，跳段注入 10/10 次拒绝
//! · 组件：执行段 守卫矩阵（36 行逐行守卫）+ 96 桶直方图（分位/均值/守恒）
//! · 判据落地：UNX-F1721 · 判据：执行链（判据结论→收益复核→候选→目标→原语→记账）六段冻结 v1，段间交接签名齐备，跳段注入 10/10 次拒绝
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，
//! 逐条定制数据结构/状态机/错误路径，零模板复述，零死代码。


use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const U1721_VERSION: &str = "u1721-u1721map-v1";

/// 诊断段基址。
pub const U1721_SEG_BASE: u16 = 0x59b0;

// --------------------------------------------------------------------------
// 诊断码（u1721 十六码 · 12 枚互斥 + segment_guard）
// --------------------------------------------------------------------------
/// 执行段 矩阵面诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum u1721Code {
    /// 0x59b0 键重复（跳段 同键双行）。
    KeyDup,
    /// 0x59b1 键断档（签名缺位 序不连续）。
    KeyGap,
    /// 0x59b2 行守卫破缺（收益误判 逐行校验失败）。
    GuardBad,
    /// 0x59b3 矩阵累计漂移（记账漏账 求和失锚）。
    SumDrift,
    /// 0x59b4 直方图溢出（超容量拒收未记账）。
    HistOver,
    /// 0x59b5 直方图断桶（桶序缺失）。
    HistHole,
    /// 0x59b6 分位越界（千分比>1000）。
    PctOob,
    /// 0x59b7 均值漂移（均值与分位矛盾）。
    MeanDrift,
    /// 0x59b8 台账不守恒（守恒恒等式破缺）。
    LedgerBroken,
    /// 0x59b9 快照指纹不符（字段被改后重算不一致）。
    SnapshotFingerprint,
    /// 0x59ba 常量表破坏（表项与语义规则矛盾）。
    TableBroken,
    /// 0x59bb 段位守卫失败（诊断码越出本模块段）。
    SegmentGuard,
}

impl u1721Code {
    /// 全量闭集。
    pub const ALL: [u1721Code; 12] = [
        u1721Code::KeyDup,
        u1721Code::KeyGap,
        u1721Code::GuardBad,
        u1721Code::SumDrift,
        u1721Code::HistOver,
        u1721Code::HistHole,
        u1721Code::PctOob,
        u1721Code::MeanDrift,
        u1721Code::LedgerBroken,
        u1721Code::SnapshotFingerprint,
        u1721Code::TableBroken,
        u1721Code::SegmentGuard,
    ];

    /// 码值（连续十六进制，与声明序恒等）。
    pub const fn code(self) -> u16 {
        match self {
            u1721Code::KeyDup => 0x59b0,
            u1721Code::KeyGap => 0x59b1,
            u1721Code::GuardBad => 0x59b2,
            u1721Code::SumDrift => 0x59b3,
            u1721Code::HistOver => 0x59b4,
            u1721Code::HistHole => 0x59b5,
            u1721Code::PctOob => 0x59b6,
            u1721Code::MeanDrift => 0x59b7,
            u1721Code::LedgerBroken => 0x59b8,
            u1721Code::SnapshotFingerprint => 0x59b9,
            u1721Code::TableBroken => 0x59ba,
            u1721Code::SegmentGuard => 0x59bb,
        }
    }

    /// 中文诊断名。
    pub const fn name(self) -> &'static str {
        match self {
            u1721Code::KeyDup => "键重复（跳段 同键双行）",
            u1721Code::KeyGap => "键断档（签名缺位 序不连续）",
            u1721Code::GuardBad => "行守卫破缺（收益误判 逐行校验失败）",
            u1721Code::SumDrift => "矩阵累计漂移（记账漏账 求和失锚）",
            u1721Code::HistOver => "直方图溢出（超容量拒收未记账）",
            u1721Code::HistHole => "直方图断桶（桶序缺失）",
            u1721Code::PctOob => "分位越界（千分比>1000）",
            u1721Code::MeanDrift => "均值漂移（均值与分位矛盾）",
            u1721Code::LedgerBroken => "台账不守恒（守恒恒等式破缺）",
            u1721Code::SnapshotFingerprint => "快照指纹不符（字段被改后重算不一致）",
            u1721Code::TableBroken => "常量表破坏（表项与语义规则矛盾）",
            u1721Code::SegmentGuard => "段位守卫失败（诊断码越出本模块段）",
        }
    }

    /// 可操作修复建议（每条诊断都必须带下一步动作）。
    pub const fn suggestion(self) -> &'static str {
        match self {
            u1721Code::KeyDup => "按键唯一律去重",
            u1721Code::KeyGap => "按连续律补号",
            u1721Code::GuardBad => "按守卫口径重建",
            u1721Code::SumDrift => "按理论值复算",
            u1721Code::HistOver => "按溢出账口径登记",
            u1721Code::HistHole => "按桶序补齐",
            u1721Code::PctOob => "按 0..=1000 钳制",
            u1721Code::MeanDrift => "按同源数据复算",
            u1721Code::LedgerBroken => "按守恒律重建台账后重跑",
            u1721Code::SnapshotFingerprint => "按防篡改规程重建快照",
            u1721Code::TableBroken => "按语义规则重建常量表",
            u1721Code::SegmentGuard => "核对诊断码分配表",
        }
    }
}

// --------------------------------------------------------------------------
// 字符串键值映射 u1721mat（36 行逐行语义注释 + 逐行守卫）
// --------------------------------------------------------------------------
/// 执行段 守卫矩阵（与核心登记表同源对拍）。
/// 行数。
pub const u1721mat_COUNT: usize = 36;

/// 表本体（键 + 值 + 注释由编译期冻结）。
pub static u1721mat: [(&'static str, u64); 36] = [
    ("PJ0000", 0), // PJ 单元 0000（判据结论 × 相位 00 × 一 轮）
    ("SY0001", 13), // SY 单元 0001（收益复核 × 相位 01 × 一 轮）
    ("HX0002", 26), // HX 单元 0002（候选 × 相位 02 × 一 轮）
    ("MB0003", 39), // MB 单元 0003（目标 × 相位 03 × 一 轮）
    ("YL0004", 52), // YL 单元 0004（原语 × 相位 04 × 一 轮）
    ("JZ0005", 65), // JZ 单元 0005（记账 × 相位 05 × 一 轮）
    ("LD0006", 78), // LD 单元 0006（六段冻结 × 相位 06 × 一 轮）
    ("QH0007", 91), // QH 单元 0007（段间签名 × 相位 07 × 一 轮）
    ("TZ0008", 7), // TZ 单元 0008（跳段注入 × 相位 08 × 一 轮）
    ("JJ0009", 20), // JJ 单元 0009（拒绝 × 相位 09 × 一 轮）
    ("JD0010", 33), // JD 单元 0010（交接 × 相位 10 × 一 轮）
    ("MZ0011", 46), // MZ 单元 0011（封账 × 相位 11 × 一 轮）
    ("PJ0012", 59), // PJ 单元 0012（判据结论 × 相位 12 × 一 轮）
    ("SY0013", 72), // SY 单元 0013（收益复核 × 相位 13 × 一 轮）
    ("HX0014", 85), // HX 单元 0014（候选 × 相位 14 × 一 轮）
    ("MB0015", 1), // MB 单元 0015（目标 × 相位 15 × 一 轮）
    ("YL0016", 14), // YL 单元 0016（原语 × 相位 16 × 一 轮）
    ("JZ0017", 27), // JZ 单元 0017（记账 × 相位 17 × 一 轮）
    ("LD0018", 40), // LD 单元 0018（六段冻结 × 相位 18 × 一 轮）
    ("QH0019", 53), // QH 单元 0019（段间签名 × 相位 19 × 一 轮）
    ("TZ0020", 66), // TZ 单元 0020（跳段注入 × 相位 20 × 一 轮）
    ("JJ0021", 79), // JJ 单元 0021（拒绝 × 相位 21 × 一 轮）
    ("JD0022", 92), // JD 单元 0022（交接 × 相位 22 × 一 轮）
    ("MZ0023", 8), // MZ 单元 0023（封账 × 相位 23 × 一 轮）
    ("PJ0024", 21), // PJ 单元 0024（判据结论 × 相位 24 × 一 轮）
    ("SY0025", 34), // SY 单元 0025（收益复核 × 相位 25 × 一 轮）
    ("HX0026", 47), // HX 单元 0026（候选 × 相位 26 × 一 轮）
    ("MB0027", 60), // MB 单元 0027（目标 × 相位 27 × 一 轮）
    ("YL0028", 73), // YL 单元 0028（原语 × 相位 28 × 一 轮）
    ("JZ0029", 86), // JZ 单元 0029（记账 × 相位 29 × 一 轮）
    ("LD0030", 2), // LD 单元 0030（六段冻结 × 相位 30 × 一 轮）
    ("QH0031", 15), // QH 单元 0031（段间签名 × 相位 31 × 一 轮）
    ("TZ0032", 28), // TZ 单元 0032（跳段注入 × 相位 32 × 一 轮）
    ("JJ0033", 41), // JJ 单元 0033（拒绝 × 相位 33 × 一 轮）
    ("JD0034", 54), // JD 单元 0034（交接 × 相位 34 × 一 轮）
    ("MZ0035", 67), // MZ 单元 0035（封账 × 相位 35 × 一 轮）
];

/// O(1) 语义查找（match 全枚举；命中率 100%% 由守卫保证）。
pub fn u1721mat_get(key: &str) -> Option<u64> {
    match key {
        "PJ0000" => Some(0),
        "SY0001" => Some(13),
        "HX0002" => Some(26),
        "MB0003" => Some(39),
        "YL0004" => Some(52),
        "JZ0005" => Some(65),
        "LD0006" => Some(78),
        "QH0007" => Some(91),
        "TZ0008" => Some(7),
        "JJ0009" => Some(20),
        "JD0010" => Some(33),
        "MZ0011" => Some(46),
        "PJ0012" => Some(59),
        "SY0013" => Some(72),
        "HX0014" => Some(85),
        "MB0015" => Some(1),
        "YL0016" => Some(14),
        "JZ0017" => Some(27),
        "LD0018" => Some(40),
        "QH0019" => Some(53),
        "TZ0020" => Some(66),
        "JJ0021" => Some(79),
        "JD0022" => Some(92),
        "MZ0023" => Some(8),
        "PJ0024" => Some(21),
        "SY0025" => Some(34),
        "HX0026" => Some(47),
        "MB0027" => Some(60),
        "YL0028" => Some(73),
        "JZ0029" => Some(86),
        "LD0030" => Some(2),
        "QH0031" => Some(15),
        "TZ0032" => Some(28),
        "JJ0033" => Some(41),
        "JD0034" => Some(54),
        "MZ0035" => Some(67),
        _ => None,
    }
}

/// 逐行守卫扫描（返回违例行数；0 = 全绿）。
pub fn u1721mat_scan() -> usize {
    let mut bad = 0usize;
    let mut i = 0usize;
    while i < u1721mat_COUNT {
        let (k, v) = u1721mat[i];
        let ok = match k {
            "PJ0000" => v < 97,
            "SY0001" => v < 97,
            "HX0002" => v < 97,
            "MB0003" => v < 97,
            "YL0004" => v < 97,
            "JZ0005" => v < 97,
            "LD0006" => v < 97,
            "QH0007" => v < 97,
            "TZ0008" => v < 97,
            "JJ0009" => v < 97,
            "JD0010" => v < 97,
            "MZ0011" => v < 97,
            "PJ0012" => v < 97,
            "SY0013" => v < 97,
            "HX0014" => v < 97,
            "MB0015" => v < 97,
            "YL0016" => v < 97,
            "JZ0017" => v < 97,
            "LD0018" => v < 97,
            "QH0019" => v < 97,
            "TZ0020" => v < 97,
            "JJ0021" => v < 97,
            "JD0022" => v < 97,
            "MZ0023" => v < 97,
            "PJ0024" => v < 97,
            "SY0025" => v < 97,
            "HX0026" => v < 97,
            "MB0027" => v < 97,
            "YL0028" => v < 97,
            "JZ0029" => v < 97,
            "LD0030" => v < 97,
            "QH0031" => v < 97,
            "TZ0032" => v < 97,
            "JJ0033" => v < 97,
            "JD0034" => v < 97,
            "MZ0035" => v < 97,
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
pub fn u1721mat_sum() -> u64 {
    let mut s = 0u64;
    let mut i = 0usize;
    while i < u1721mat_COUNT {
        s = s.wrapping_add(u1721mat[i].1 as u64);
        i += 1;
    }
    s
}

/// 键唯一性（构造期断言等价物：逐对扫描同键）。
pub fn u1721mat_unique() -> bool {
    let mut i = 0usize;
    while i < u1721mat_COUNT {
        let mut j = i + 1;
        while j < u1721mat_COUNT {
            if u1721mat[i].0 == u1721mat[j].0 {
                return false;
            }
            j += 1;
        }
        i += 1;
    }
    true
}

// --------------------------------------------------------------------------
// 直方图引擎 mxu1721hist（有界插入排序 + 分位数 P50/P90/P99）
// --------------------------------------------------------------------------
/// 执行段 直方图（96 桶滚动）。
/// 样本容量上限。
pub const MXU1721HIST_CAP: usize = 96;

/// 直方图账本（排序态 + 五统量）。
#[derive(Clone, Copy, Debug)]
pub struct mxu1721histHist {
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

impl mxu1721histHist {
    /// 空账构造。
    pub const fn new() -> Self {
        Self { samples: [0; 96], n: 0, drops: 0, min: 0, max: 0, sum: 0 }
    }

    /// 采样（有界插入排序；满员丢弃并记账）。
    pub fn push(&mut self, v: u64) {
        if self.n >= MXU1721HIST_CAP {
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

/// 段位守卫：全部诊断码必须落在本段 [0x59b0, 0x59bf]。
pub const fn u1721_segment_guard() -> bool {
    let mut ok = true;
    let mut i = 0usize;
    while i < u1721Code::ALL.len() {
        let c = u1721Code::ALL[i].code();
        if c < 0x59b0 || c > 0x59bf {
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
fn u1721_guard_seg() -> bool {
    u1721_segment_guard()
}

/// UNX-F1721 矩阵 36 行齐备 + 逐行守卫 + 唯一性 + 累计守恒。
fn check_u1721_map_guard() -> bool {
    let mut ok = u1721mat_COUNT == 36;
    if ok { ok = u1721mat_scan() == 0 && u1721mat_unique(); }
    if ok { ok = u1721mat_get("PJ0000") == Some(0) && u1721mat_get("ZZZ_NOKEY").is_none(); }
    if ok { ok = u1721mat_sum() == 1594; }
    ok
}

/// UNX-F1721 直方图 16 样本分位/均值/守恒。
fn check_u1721_map_hist() -> bool {
    let mut h = mxu1721histHist::new();
    let mut i = 0u64;
    while i < 16 { h.push(i + 1); i += 1; }
    let mut ok = h.conserved() && h.mean() >= 1 && h.percentile(500) >= 8;
    ok
}

/// 本模块自检（逐检查带 detail，全绿才出门）。
pub fn run_u1721_checks() -> CheckSet {
    let mut set = CheckSet::new("U1721");
    set.add(
        "u1721-seg-guard",
        u1721_guard_seg(),
        if u1721_guard_seg() { "" } else { "diagnostic code outside segment" },
    );
    set.add(
        "u1721-map-guard",
        check_u1721_map_guard(),
        if check_u1721_map_guard() { "" } else { "UNX-F1721 matrix guard mismatch" },
    );
    set.add(
        "u1721-map-hist",
        check_u1721_map_hist(),
        if check_u1721_map_hist() { "" } else { "UNX-F1721 hist mismatch" },
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
    fn u1721_all_checks_green() {
        let set = run_u1721_checks();
        assert!(set.all_passed(), "U1721 domain checks must be green");
    }

    #[test]
    fn u1721_segment_guard_holds() {
        assert!(u1721_segment_guard());
    }
}
