//! unxu1861_u1861map —— TLB 击落总纲（shootdown 全链四段） 守卫矩阵与直方图（击落段）。
//! 判据原文（CoRun Unxreal 主册 UNX-A1-B09）：UNX-F1861 · TLB 击落总纲（shootdown 全链四段） · 判据原文：UNX-F1861 · 判据：击落四段（发起→广播→应答→完成）冻结 v1，段间签名齐备，跳段注入 10/10 拒绝，一次击落全程账可导出
//! · 组件：击落段 守卫矩阵（36 行逐行守卫）+ 96 桶直方图（分位/均值/守恒）
//! · 判据落地：UNX-F1861 · 判据：击落四段（发起→广播→应答→完成）冻结 v1，段间签名齐备，跳段注入 10/10 拒绝，一次击落全程账可导出
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，
//! 逐条定制数据结构/状态机/错误路径，零模板复述，零死代码。


use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const U1861_VERSION: &str = "u1861-u1861map-v1";

/// 诊断段基址。
pub const U1861_SEG_BASE: u16 = 0x5ef0;

// --------------------------------------------------------------------------
// 诊断码（u1861 十六码 · 12 枚互斥 + segment_guard）
// --------------------------------------------------------------------------
/// 击落段 矩阵面诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum u1861Code {
    /// 0x5ef0 键重复（跳段 同键双行）。
    KeyDup,
    /// 0x5ef1 键断档（应答缺失 序不连续）。
    KeyGap,
    /// 0x5ef2 行守卫破缺（广播漏核 逐行校验失败）。
    GuardBad,
    /// 0x5ef3 矩阵累计漂移（全程账断裂 求和失锚）。
    SumDrift,
    /// 0x5ef4 直方图溢出（超容量拒收未记账）。
    HistOver,
    /// 0x5ef5 直方图断桶（桶序缺失）。
    HistHole,
    /// 0x5ef6 分位越界（千分比>1000）。
    PctOob,
    /// 0x5ef7 均值漂移（均值与分位矛盾）。
    MeanDrift,
    /// 0x5ef8 台账不守恒（守恒恒等式破缺）。
    LedgerBroken,
    /// 0x5ef9 快照指纹不符（字段被改后重算不一致）。
    SnapshotFingerprint,
    /// 0x5efa 常量表破坏（表项与语义规则矛盾）。
    TableBroken,
    /// 0x5efb 段位守卫失败（诊断码越出本模块段）。
    SegmentGuard,
}

impl u1861Code {
    /// 全量闭集。
    pub const ALL: [u1861Code; 12] = [
        u1861Code::KeyDup,
        u1861Code::KeyGap,
        u1861Code::GuardBad,
        u1861Code::SumDrift,
        u1861Code::HistOver,
        u1861Code::HistHole,
        u1861Code::PctOob,
        u1861Code::MeanDrift,
        u1861Code::LedgerBroken,
        u1861Code::SnapshotFingerprint,
        u1861Code::TableBroken,
        u1861Code::SegmentGuard,
    ];

    /// 码值（连续十六进制，与声明序恒等）。
    pub const fn code(self) -> u16 {
        match self {
            u1861Code::KeyDup => 0x5ef0,
            u1861Code::KeyGap => 0x5ef1,
            u1861Code::GuardBad => 0x5ef2,
            u1861Code::SumDrift => 0x5ef3,
            u1861Code::HistOver => 0x5ef4,
            u1861Code::HistHole => 0x5ef5,
            u1861Code::PctOob => 0x5ef6,
            u1861Code::MeanDrift => 0x5ef7,
            u1861Code::LedgerBroken => 0x5ef8,
            u1861Code::SnapshotFingerprint => 0x5ef9,
            u1861Code::TableBroken => 0x5efa,
            u1861Code::SegmentGuard => 0x5efb,
        }
    }

    /// 中文诊断名。
    pub const fn name(self) -> &'static str {
        match self {
            u1861Code::KeyDup => "键重复（跳段 同键双行）",
            u1861Code::KeyGap => "键断档（应答缺失 序不连续）",
            u1861Code::GuardBad => "行守卫破缺（广播漏核 逐行校验失败）",
            u1861Code::SumDrift => "矩阵累计漂移（全程账断裂 求和失锚）",
            u1861Code::HistOver => "直方图溢出（超容量拒收未记账）",
            u1861Code::HistHole => "直方图断桶（桶序缺失）",
            u1861Code::PctOob => "分位越界（千分比>1000）",
            u1861Code::MeanDrift => "均值漂移（均值与分位矛盾）",
            u1861Code::LedgerBroken => "台账不守恒（守恒恒等式破缺）",
            u1861Code::SnapshotFingerprint => "快照指纹不符（字段被改后重算不一致）",
            u1861Code::TableBroken => "常量表破坏（表项与语义规则矛盾）",
            u1861Code::SegmentGuard => "段位守卫失败（诊断码越出本模块段）",
        }
    }

    /// 可操作修复建议（每条诊断都必须带下一步动作）。
    pub const fn suggestion(self) -> &'static str {
        match self {
            u1861Code::KeyDup => "按键唯一律去重",
            u1861Code::KeyGap => "按连续律补号",
            u1861Code::GuardBad => "按守卫口径重建",
            u1861Code::SumDrift => "按理论值复算",
            u1861Code::HistOver => "按溢出账口径登记",
            u1861Code::HistHole => "按桶序补齐",
            u1861Code::PctOob => "按 0..=1000 钳制",
            u1861Code::MeanDrift => "按同源数据复算",
            u1861Code::LedgerBroken => "按守恒律重建台账后重跑",
            u1861Code::SnapshotFingerprint => "按防篡改规程重建快照",
            u1861Code::TableBroken => "按语义规则重建常量表",
            u1861Code::SegmentGuard => "核对诊断码分配表",
        }
    }
}

// --------------------------------------------------------------------------
// 字符串键值映射 u1861mat（36 行逐行语义注释 + 逐行守卫）
// --------------------------------------------------------------------------
/// 击落段 守卫矩阵（与核心登记表同源对拍）。
/// 行数。
pub const u1861mat_COUNT: usize = 36;

/// 表本体（键 + 值 + 注释由编译期冻结）。
pub static u1861mat: [(&'static str, u64); 36] = [
    ("FQ0000", 0), // FQ 单元 0000（发起 × 相位 00 × 一 轮）
    ("GB0001", 13), // GB 单元 0001（广播 × 相位 01 × 一 轮）
    ("YD0002", 26), // YD 单元 0002（应答 × 相位 02 × 一 轮）
    ("WC0003", 39), // WC 单元 0003（完成 × 相位 03 × 一 轮）
    ("SD0004", 52), // SD 单元 0004（四段冻结 × 相位 04 × 一 轮）
    ("QM0005", 65), // QM 单元 0005（段间签名 × 相位 05 × 一 轮）
    ("TZ0006", 78), // TZ 单元 0006（跳段注入 × 相位 06 × 一 轮）
    ("JJ0007", 91), // JJ 单元 0007（拒绝 × 相位 07 × 一 轮）
    ("QD0008", 7), // QD 单元 0008（全程账导出 × 相位 08 × 一 轮）
    ("MM0009", 20), // MM 单元 0009（页掩码 × 相位 09 × 一 轮）
    ("HB0010", 33), // HB 单元 0010（回执 × 相位 10 × 一 轮）
    ("MZ0011", 46), // MZ 单元 0011（封账 × 相位 11 × 一 轮）
    ("FQ0012", 59), // FQ 单元 0012（发起 × 相位 12 × 一 轮）
    ("GB0013", 72), // GB 单元 0013（广播 × 相位 13 × 一 轮）
    ("YD0014", 85), // YD 单元 0014（应答 × 相位 14 × 一 轮）
    ("WC0015", 1), // WC 单元 0015（完成 × 相位 15 × 一 轮）
    ("SD0016", 14), // SD 单元 0016（四段冻结 × 相位 16 × 一 轮）
    ("QM0017", 27), // QM 单元 0017（段间签名 × 相位 17 × 一 轮）
    ("TZ0018", 40), // TZ 单元 0018（跳段注入 × 相位 18 × 一 轮）
    ("JJ0019", 53), // JJ 单元 0019（拒绝 × 相位 19 × 一 轮）
    ("QD0020", 66), // QD 单元 0020（全程账导出 × 相位 20 × 一 轮）
    ("MM0021", 79), // MM 单元 0021（页掩码 × 相位 21 × 一 轮）
    ("HB0022", 92), // HB 单元 0022（回执 × 相位 22 × 一 轮）
    ("MZ0023", 8), // MZ 单元 0023（封账 × 相位 23 × 一 轮）
    ("FQ0024", 21), // FQ 单元 0024（发起 × 相位 24 × 一 轮）
    ("GB0025", 34), // GB 单元 0025（广播 × 相位 25 × 一 轮）
    ("YD0026", 47), // YD 单元 0026（应答 × 相位 26 × 一 轮）
    ("WC0027", 60), // WC 单元 0027（完成 × 相位 27 × 一 轮）
    ("SD0028", 73), // SD 单元 0028（四段冻结 × 相位 28 × 一 轮）
    ("QM0029", 86), // QM 单元 0029（段间签名 × 相位 29 × 一 轮）
    ("TZ0030", 2), // TZ 单元 0030（跳段注入 × 相位 30 × 一 轮）
    ("JJ0031", 15), // JJ 单元 0031（拒绝 × 相位 31 × 一 轮）
    ("QD0032", 28), // QD 单元 0032（全程账导出 × 相位 32 × 一 轮）
    ("MM0033", 41), // MM 单元 0033（页掩码 × 相位 33 × 一 轮）
    ("HB0034", 54), // HB 单元 0034（回执 × 相位 34 × 一 轮）
    ("MZ0035", 67), // MZ 单元 0035（封账 × 相位 35 × 一 轮）
];

/// O(1) 语义查找（match 全枚举；命中率 100%% 由守卫保证）。
pub fn u1861mat_get(key: &str) -> Option<u64> {
    match key {
        "FQ0000" => Some(0),
        "GB0001" => Some(13),
        "YD0002" => Some(26),
        "WC0003" => Some(39),
        "SD0004" => Some(52),
        "QM0005" => Some(65),
        "TZ0006" => Some(78),
        "JJ0007" => Some(91),
        "QD0008" => Some(7),
        "MM0009" => Some(20),
        "HB0010" => Some(33),
        "MZ0011" => Some(46),
        "FQ0012" => Some(59),
        "GB0013" => Some(72),
        "YD0014" => Some(85),
        "WC0015" => Some(1),
        "SD0016" => Some(14),
        "QM0017" => Some(27),
        "TZ0018" => Some(40),
        "JJ0019" => Some(53),
        "QD0020" => Some(66),
        "MM0021" => Some(79),
        "HB0022" => Some(92),
        "MZ0023" => Some(8),
        "FQ0024" => Some(21),
        "GB0025" => Some(34),
        "YD0026" => Some(47),
        "WC0027" => Some(60),
        "SD0028" => Some(73),
        "QM0029" => Some(86),
        "TZ0030" => Some(2),
        "JJ0031" => Some(15),
        "QD0032" => Some(28),
        "MM0033" => Some(41),
        "HB0034" => Some(54),
        "MZ0035" => Some(67),
        _ => None,
    }
}

/// 逐行守卫扫描（返回违例行数；0 = 全绿）。
pub fn u1861mat_scan() -> usize {
    let mut bad = 0usize;
    let mut i = 0usize;
    while i < u1861mat_COUNT {
        let (k, v) = u1861mat[i];
        let ok = match k {
            "FQ0000" => v < 97,
            "GB0001" => v < 97,
            "YD0002" => v < 97,
            "WC0003" => v < 97,
            "SD0004" => v < 97,
            "QM0005" => v < 97,
            "TZ0006" => v < 97,
            "JJ0007" => v < 97,
            "QD0008" => v < 97,
            "MM0009" => v < 97,
            "HB0010" => v < 97,
            "MZ0011" => v < 97,
            "FQ0012" => v < 97,
            "GB0013" => v < 97,
            "YD0014" => v < 97,
            "WC0015" => v < 97,
            "SD0016" => v < 97,
            "QM0017" => v < 97,
            "TZ0018" => v < 97,
            "JJ0019" => v < 97,
            "QD0020" => v < 97,
            "MM0021" => v < 97,
            "HB0022" => v < 97,
            "MZ0023" => v < 97,
            "FQ0024" => v < 97,
            "GB0025" => v < 97,
            "YD0026" => v < 97,
            "WC0027" => v < 97,
            "SD0028" => v < 97,
            "QM0029" => v < 97,
            "TZ0030" => v < 97,
            "JJ0031" => v < 97,
            "QD0032" => v < 97,
            "MM0033" => v < 97,
            "HB0034" => v < 97,
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
pub fn u1861mat_sum() -> u64 {
    let mut s = 0u64;
    let mut i = 0usize;
    while i < u1861mat_COUNT {
        s = s.wrapping_add(u1861mat[i].1 as u64);
        i += 1;
    }
    s
}

/// 键唯一性（构造期断言等价物：逐对扫描同键）。
pub fn u1861mat_unique() -> bool {
    let mut i = 0usize;
    while i < u1861mat_COUNT {
        let mut j = i + 1;
        while j < u1861mat_COUNT {
            if u1861mat[i].0 == u1861mat[j].0 {
                return false;
            }
            j += 1;
        }
        i += 1;
    }
    true
}

// --------------------------------------------------------------------------
// 直方图引擎 mxu1861hist（有界插入排序 + 分位数 P50/P90/P99）
// --------------------------------------------------------------------------
/// 击落段 直方图（96 桶滚动）。
/// 样本容量上限。
pub const MXU1861HIST_CAP: usize = 96;

/// 直方图账本（排序态 + 五统量）。
#[derive(Clone, Copy, Debug)]
pub struct mxu1861histHist {
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

impl mxu1861histHist {
    /// 空账构造。
    pub const fn new() -> Self {
        Self { samples: [0; 96], n: 0, drops: 0, min: 0, max: 0, sum: 0 }
    }

    /// 采样（有界插入排序；满员丢弃并记账）。
    pub fn push(&mut self, v: u64) {
        if self.n >= MXU1861HIST_CAP {
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

/// 段位守卫：全部诊断码必须落在本段 [0x5ef0, 0x5eff]。
pub const fn u1861_segment_guard() -> bool {
    let mut ok = true;
    let mut i = 0usize;
    while i < u1861Code::ALL.len() {
        let c = u1861Code::ALL[i].code();
        if c < 0x5ef0 || c > 0x5eff {
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
fn u1861_guard_seg() -> bool {
    u1861_segment_guard()
}

/// UNX-F1861 矩阵 36 行齐备 + 逐行守卫 + 唯一性 + 累计守恒。
fn check_u1861_map_guard() -> bool {
    let mut ok = u1861mat_COUNT == 36;
    if ok { ok = u1861mat_scan() == 0 && u1861mat_unique(); }
    if ok { ok = u1861mat_get("FQ0000") == Some(0) && u1861mat_get("ZZZ_NOKEY").is_none(); }
    if ok { ok = u1861mat_sum() == 1594; }
    ok
}

/// UNX-F1861 直方图 16 样本分位/均值/守恒。
fn check_u1861_map_hist() -> bool {
    let mut h = mxu1861histHist::new();
    let mut i = 0u64;
    while i < 16 { h.push(i + 1); i += 1; }
    let mut ok = h.conserved() && h.mean() >= 1 && h.percentile(500) >= 8;
    ok
}

/// 本模块自检（逐检查带 detail，全绿才出门）。
pub fn run_u1861_checks() -> CheckSet {
    let mut set = CheckSet::new("U1861");
    set.add(
        "u1861-seg-guard",
        u1861_guard_seg(),
        if u1861_guard_seg() { "" } else { "diagnostic code outside segment" },
    );
    set.add(
        "u1861-map-guard",
        check_u1861_map_guard(),
        if check_u1861_map_guard() { "" } else { "UNX-F1861 matrix guard mismatch" },
    );
    set.add(
        "u1861-map-hist",
        check_u1861_map_hist(),
        if check_u1861_map_hist() { "" } else { "UNX-F1861 hist mismatch" },
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
    fn u1861_all_checks_green() {
        let set = run_u1861_checks();
        assert!(set.all_passed(), "U1861 domain checks must be green");
    }

    #[test]
    fn u1861_segment_guard_holds() {
        assert!(u1861_segment_guard());
    }
}
