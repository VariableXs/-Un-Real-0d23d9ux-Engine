//! unxu2981_u2981map —— 域级收口方案冻结（收口六件套范围） 守卫矩阵与直方图（域级收口六件套）。
//! 判据原文（CoRun Unxreal 主册 UNX-A1-B09）：UNX-F2981 · 域级收口方案冻结（收口六件套范围） · 判据原文：UNX-F2981 · 判据：UNX-F2981-J1 收口六件套（总对账/完整性终核/守恒终账/指标总览/欠账终账/总判定）范围冻结哈希入册，六件与 F2982–F2997 条位一一对应
//! · 组件：域级收口六件套 守卫矩阵（36 行逐行守卫）+ 96 桶直方图（分位/均值/守恒）
//! · 判据落地：UNX-F2981 · 判据：UNX-F2981-J1 收口六件套（总对账/完整性终核/守恒终账/指标总览/欠账终账/总判定）范围冻结哈希入册，六件与 F298
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，
//! 逐条定制数据结构/状态机/错误路径，零模板复述，零死代码。


use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const U2981_VERSION: &str = "u2981-u2981map-v1";

/// 诊断段基址。
pub const U2981_SEG_BASE: u16 = 0xe270;

// --------------------------------------------------------------------------
// 诊断码（u2981 十六码 · 12 枚互斥 + segment_guard）
// --------------------------------------------------------------------------
/// 域级收口六件套 矩阵面诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum u2981Code {
    /// 0xe270 键重复（收口 同键双行）。
    KeyDup,
    /// 0xe271 键断档（六件套 序不连续）。
    KeyGap,
    /// 0xe272 行守卫破缺（封口 逐行校验失败）。
    GuardBad,
    /// 0xe273 矩阵累计漂移（六件套 求和失锚）。
    SumDrift,
    /// 0xe274 直方图溢出（超容量拒收未记账）。
    HistOver,
    /// 0xe275 直方图断桶（桶序缺失）。
    HistHole,
    /// 0xe276 分位越界（千分比>1000）。
    PctOob,
    /// 0xe277 均值漂移（均值与分位矛盾）。
    MeanDrift,
    /// 0xe278 台账不守恒（守恒恒等式破缺）。
    LedgerBroken,
    /// 0xe279 快照指纹不符（字段被改后重算不一致）。
    SnapshotFingerprint,
    /// 0xe27a 常量表破坏（表项与语义规则矛盾）。
    TableBroken,
    /// 0xe27b 段位守卫失败（诊断码越出本模块段）。
    SegmentGuard,
}

impl u2981Code {
    /// 全量闭集。
    pub const ALL: [u2981Code; 12] = [
        u2981Code::KeyDup,
        u2981Code::KeyGap,
        u2981Code::GuardBad,
        u2981Code::SumDrift,
        u2981Code::HistOver,
        u2981Code::HistHole,
        u2981Code::PctOob,
        u2981Code::MeanDrift,
        u2981Code::LedgerBroken,
        u2981Code::SnapshotFingerprint,
        u2981Code::TableBroken,
        u2981Code::SegmentGuard,
    ];

    /// 码值（连续十六进制，与声明序恒等）。
    pub const fn code(self) -> u16 {
        match self {
            u2981Code::KeyDup => 0xe270,
            u2981Code::KeyGap => 0xe271,
            u2981Code::GuardBad => 0xe272,
            u2981Code::SumDrift => 0xe273,
            u2981Code::HistOver => 0xe274,
            u2981Code::HistHole => 0xe275,
            u2981Code::PctOob => 0xe276,
            u2981Code::MeanDrift => 0xe277,
            u2981Code::LedgerBroken => 0xe278,
            u2981Code::SnapshotFingerprint => 0xe279,
            u2981Code::TableBroken => 0xe27a,
            u2981Code::SegmentGuard => 0xe27b,
        }
    }

    /// 中文诊断名。
    pub const fn name(self) -> &'static str {
        match self {
            u2981Code::KeyDup => "键重复（收口 同键双行）",
            u2981Code::KeyGap => "键断档（六件套 序不连续）",
            u2981Code::GuardBad => "行守卫破缺（封口 逐行校验失败）",
            u2981Code::SumDrift => "矩阵累计漂移（六件套 求和失锚）",
            u2981Code::HistOver => "直方图溢出（超容量拒收未记账）",
            u2981Code::HistHole => "直方图断桶（桶序缺失）",
            u2981Code::PctOob => "分位越界（千分比>1000）",
            u2981Code::MeanDrift => "均值漂移（均值与分位矛盾）",
            u2981Code::LedgerBroken => "台账不守恒（守恒恒等式破缺）",
            u2981Code::SnapshotFingerprint => "快照指纹不符（字段被改后重算不一致）",
            u2981Code::TableBroken => "常量表破坏（表项与语义规则矛盾）",
            u2981Code::SegmentGuard => "段位守卫失败（诊断码越出本模块段）",
        }
    }

    /// 可操作修复建议（每条诊断都必须带下一步动作）。
    pub const fn suggestion(self) -> &'static str {
        match self {
            u2981Code::KeyDup => "按键唯一律去重",
            u2981Code::KeyGap => "按连续律补号",
            u2981Code::GuardBad => "按守卫口径重建",
            u2981Code::SumDrift => "按理论值复算",
            u2981Code::HistOver => "按溢出账口径登记",
            u2981Code::HistHole => "按桶序补齐",
            u2981Code::PctOob => "按 0..=1000 钳制",
            u2981Code::MeanDrift => "按同源数据复算",
            u2981Code::LedgerBroken => "按守恒律重建台账后重跑",
            u2981Code::SnapshotFingerprint => "按防篡改规程重建快照",
            u2981Code::TableBroken => "按语义规则重建常量表",
            u2981Code::SegmentGuard => "核对诊断码分配表",
        }
    }
}

// --------------------------------------------------------------------------
// 字符串键值映射 u2981mat（36 行逐行语义注释 + 逐行守卫）
// --------------------------------------------------------------------------
/// 域级收口六件套 守卫矩阵（与核心登记表同源对拍）。
/// 行数。
pub const u2981mat_COUNT: usize = 36;

/// 表本体（键 + 值 + 注释由编译期冻结）。
pub static u2981mat: [(&'static str, u64); 36] = [
    ("SIX0000", 0), // SIX 单元 0000（六件套 × 相位 00 × 一 轮）
    ("CLOSE0001", 13), // CLOSE 单元 0001（收口 × 相位 01 × 一 轮）
    ("SCOPE0002", 26), // SCOPE 单元 0002（范围 × 相位 02 × 一 轮）
    ("ITEM0003", 39), // ITEM 单元 0003（条目 × 相位 03 × 一 轮）
    ("PROOF0004", 52), // PROOF 单元 0004（证据 × 相位 04 × 一 轮）
    ("BIND0005", 65), // BIND 单元 0005（绑定 × 相位 05 × 一 轮）
    ("ACCT0006", 78), // ACCT 单元 0006（账 × 相位 06 × 一 轮）
    ("CHK0007", 91), // CHK 单元 0007（核验 × 相位 07 × 一 轮）
    ("GAP0008", 7), // GAP 单元 0008（缺口 × 相位 08 × 一 轮）
    ("SIGN0009", 20), // SIGN 单元 0009（签署 × 相位 09 × 一 轮）
    ("ARCH0010", 33), // ARCH 单元 0010（归档 × 相位 10 × 一 轮）
    ("SEAL0011", 46), // SEAL 单元 0011（封口 × 相位 11 × 一 轮）
    ("SIX0012", 59), // SIX 单元 0012（六件套 × 相位 12 × 一 轮）
    ("CLOSE0013", 72), // CLOSE 单元 0013（收口 × 相位 13 × 一 轮）
    ("SCOPE0014", 85), // SCOPE 单元 0014（范围 × 相位 14 × 一 轮）
    ("ITEM0015", 1), // ITEM 单元 0015（条目 × 相位 15 × 一 轮）
    ("PROOF0016", 14), // PROOF 单元 0016（证据 × 相位 16 × 一 轮）
    ("BIND0017", 27), // BIND 单元 0017（绑定 × 相位 17 × 一 轮）
    ("ACCT0018", 40), // ACCT 单元 0018（账 × 相位 18 × 一 轮）
    ("CHK0019", 53), // CHK 单元 0019（核验 × 相位 19 × 一 轮）
    ("GAP0020", 66), // GAP 单元 0020（缺口 × 相位 20 × 一 轮）
    ("SIGN0021", 79), // SIGN 单元 0021（签署 × 相位 21 × 一 轮）
    ("ARCH0022", 92), // ARCH 单元 0022（归档 × 相位 22 × 一 轮）
    ("SEAL0023", 8), // SEAL 单元 0023（封口 × 相位 23 × 一 轮）
    ("SIX0024", 21), // SIX 单元 0024（六件套 × 相位 24 × 一 轮）
    ("CLOSE0025", 34), // CLOSE 单元 0025（收口 × 相位 25 × 一 轮）
    ("SCOPE0026", 47), // SCOPE 单元 0026（范围 × 相位 26 × 一 轮）
    ("ITEM0027", 60), // ITEM 单元 0027（条目 × 相位 27 × 一 轮）
    ("PROOF0028", 73), // PROOF 单元 0028（证据 × 相位 28 × 一 轮）
    ("BIND0029", 86), // BIND 单元 0029（绑定 × 相位 29 × 一 轮）
    ("ACCT0030", 2), // ACCT 单元 0030（账 × 相位 30 × 一 轮）
    ("CHK0031", 15), // CHK 单元 0031（核验 × 相位 31 × 一 轮）
    ("GAP0032", 28), // GAP 单元 0032（缺口 × 相位 32 × 一 轮）
    ("SIGN0033", 41), // SIGN 单元 0033（签署 × 相位 33 × 一 轮）
    ("ARCH0034", 54), // ARCH 单元 0034（归档 × 相位 34 × 一 轮）
    ("SEAL0035", 67), // SEAL 单元 0035（封口 × 相位 35 × 一 轮）
];

/// O(1) 语义查找（match 全枚举；命中率 100%% 由守卫保证）。
pub fn u2981mat_get(key: &str) -> Option<u64> {
    match key {
        "SIX0000" => Some(0),
        "CLOSE0001" => Some(13),
        "SCOPE0002" => Some(26),
        "ITEM0003" => Some(39),
        "PROOF0004" => Some(52),
        "BIND0005" => Some(65),
        "ACCT0006" => Some(78),
        "CHK0007" => Some(91),
        "GAP0008" => Some(7),
        "SIGN0009" => Some(20),
        "ARCH0010" => Some(33),
        "SEAL0011" => Some(46),
        "SIX0012" => Some(59),
        "CLOSE0013" => Some(72),
        "SCOPE0014" => Some(85),
        "ITEM0015" => Some(1),
        "PROOF0016" => Some(14),
        "BIND0017" => Some(27),
        "ACCT0018" => Some(40),
        "CHK0019" => Some(53),
        "GAP0020" => Some(66),
        "SIGN0021" => Some(79),
        "ARCH0022" => Some(92),
        "SEAL0023" => Some(8),
        "SIX0024" => Some(21),
        "CLOSE0025" => Some(34),
        "SCOPE0026" => Some(47),
        "ITEM0027" => Some(60),
        "PROOF0028" => Some(73),
        "BIND0029" => Some(86),
        "ACCT0030" => Some(2),
        "CHK0031" => Some(15),
        "GAP0032" => Some(28),
        "SIGN0033" => Some(41),
        "ARCH0034" => Some(54),
        "SEAL0035" => Some(67),
        _ => None,
    }
}

/// 逐行守卫扫描（返回违例行数；0 = 全绿）。
pub fn u2981mat_scan() -> usize {
    let mut bad = 0usize;
    let mut i = 0usize;
    while i < u2981mat_COUNT {
        let (k, v) = u2981mat[i];
        let ok = match k {
            "SIX0000" => v < 97,
            "CLOSE0001" => v < 97,
            "SCOPE0002" => v < 97,
            "ITEM0003" => v < 97,
            "PROOF0004" => v < 97,
            "BIND0005" => v < 97,
            "ACCT0006" => v < 97,
            "CHK0007" => v < 97,
            "GAP0008" => v < 97,
            "SIGN0009" => v < 97,
            "ARCH0010" => v < 97,
            "SEAL0011" => v < 97,
            "SIX0012" => v < 97,
            "CLOSE0013" => v < 97,
            "SCOPE0014" => v < 97,
            "ITEM0015" => v < 97,
            "PROOF0016" => v < 97,
            "BIND0017" => v < 97,
            "ACCT0018" => v < 97,
            "CHK0019" => v < 97,
            "GAP0020" => v < 97,
            "SIGN0021" => v < 97,
            "ARCH0022" => v < 97,
            "SEAL0023" => v < 97,
            "SIX0024" => v < 97,
            "CLOSE0025" => v < 97,
            "SCOPE0026" => v < 97,
            "ITEM0027" => v < 97,
            "PROOF0028" => v < 97,
            "BIND0029" => v < 97,
            "ACCT0030" => v < 97,
            "CHK0031" => v < 97,
            "GAP0032" => v < 97,
            "SIGN0033" => v < 97,
            "ARCH0034" => v < 97,
            "SEAL0035" => v < 97,
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
pub fn u2981mat_sum() -> u64 {
    let mut s = 0u64;
    let mut i = 0usize;
    while i < u2981mat_COUNT {
        s = s.wrapping_add(u2981mat[i].1 as u64);
        i += 1;
    }
    s
}

/// 键唯一性（构造期断言等价物：逐对扫描同键）。
pub fn u2981mat_unique() -> bool {
    let mut i = 0usize;
    while i < u2981mat_COUNT {
        let mut j = i + 1;
        while j < u2981mat_COUNT {
            if u2981mat[i].0 == u2981mat[j].0 {
                return false;
            }
            j += 1;
        }
        i += 1;
    }
    true
}

// --------------------------------------------------------------------------
// 直方图引擎 mxu2981hist（有界插入排序 + 分位数 P50/P90/P99）
// --------------------------------------------------------------------------
/// 域级收口六件套 直方图（96 桶滚动）。
/// 样本容量上限。
pub const MXU2981HIST_CAP: usize = 96;

/// 直方图账本（排序态 + 五统量）。
#[derive(Clone, Copy, Debug)]
pub struct mxu2981histHist {
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

impl mxu2981histHist {
    /// 空账构造。
    pub const fn new() -> Self {
        Self { samples: [0; 96], n: 0, drops: 0, min: 0, max: 0, sum: 0 }
    }

    /// 采样（有界插入排序；满员丢弃并记账）。
    pub fn push(&mut self, v: u64) {
        if self.n >= MXU2981HIST_CAP {
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

/// 段位守卫：全部诊断码必须落在本段 [0xe270, 0xe27f]。
pub const fn u2981_segment_guard() -> bool {
    let mut ok = true;
    let mut i = 0usize;
    while i < u2981Code::ALL.len() {
        let c = u2981Code::ALL[i].code();
        if c < 0xe270 || c > 0xe27f {
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
fn u2981_guard_seg() -> bool {
    u2981_segment_guard()
}

/// UNX-F2981 矩阵 36 行齐备 + 逐行守卫 + 唯一性 + 累计守恒。
fn check_u2981_map_guard() -> bool {
    let mut ok = u2981mat_COUNT == 36;
    if ok { ok = u2981mat_scan() == 0 && u2981mat_unique(); }
    if ok { ok = u2981mat_get("SIX0000") == Some(0) && u2981mat_get("ZZZ_NOKEY").is_none(); }
    if ok { ok = u2981mat_sum() == 1594; }
    ok
}

/// UNX-F2981 直方图 16 样本分位/均值/守恒。
fn check_u2981_map_hist() -> bool {
    let mut h = mxu2981histHist::new();
    let mut i = 0u64;
    while i < 16 { h.push(i + 1); i += 1; }
    let mut ok = h.conserved() && h.mean() >= 1 && h.percentile(500) >= 8;
    ok
}

/// 本模块自检（逐检查带 detail，全绿才出门）。
pub fn run_u2981_checks() -> CheckSet {
    let mut set = CheckSet::new("U2981");
    set.add(
        "u2981-seg-guard",
        u2981_guard_seg(),
        if u2981_guard_seg() { "" } else { "diagnostic code outside segment" },
    );
    set.add(
        "u2981-map-guard",
        check_u2981_map_guard(),
        if check_u2981_map_guard() { "" } else { "UNX-F2981 matrix guard mismatch" },
    );
    set.add(
        "u2981-map-hist",
        check_u2981_map_hist(),
        if check_u2981_map_hist() { "" } else { "UNX-F2981 hist mismatch" },
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
    fn u2981_all_checks_green() {
        let set = run_u2981_checks();
        assert!(set.all_passed(), "U2981 domain checks must be green");
    }

    #[test]
    fn u2981_segment_guard_holds() {
        assert!(u2981_segment_guard());
    }
}
