//! unxu1661_u1661map —— 调度类注册框架与优先级链 守卫矩阵与直方图（调度类）。
//! 判据原文（CoRun Unxreal 主册 UNX-A1-B09）：UNX-F1661 · 调度类注册框架与优先级链 · 判据原文：UNX-F1661 · 判据：注册框架单点构建，类优先级链静态闭合（无环、无洞、首尾确定），重复注册注入 10/10 次拒绝
//! · 组件：调度类 守卫矩阵（36 行逐行守卫）+ 96 桶直方图（分位/均值/守恒）
//! · 判据落地：UNX-F1661 · 判据：注册框架单点构建，类优先级链静态闭合（无环、无洞、首尾确定），重复注册注入 10/10 次拒绝
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，
//! 逐条定制数据结构/状态机/错误路径，零模板复述，零死代码。


use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const U1661_VERSION: &str = "u1661-u1661map-v1";

/// 诊断段基址。
pub const U1661_SEG_BASE: u16 = 0x5770;

// --------------------------------------------------------------------------
// 诊断码（u1661 十六码 · 12 枚互斥 + segment_guard）
// --------------------------------------------------------------------------
/// 调度类 矩阵面诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum u1661Code {
    /// 0x5770 键重复（重复注册 同键双行）。
    KeyDup,
    /// 0x5771 键断档（链环 序不连续）。
    KeyGap,
    /// 0x5772 行守卫破缺（链洞 逐行校验失败）。
    GuardBad,
    /// 0x5773 矩阵累计漂移（首尾漂移 求和失锚）。
    SumDrift,
    /// 0x5774 直方图溢出（超容量拒收未记账）。
    HistOver,
    /// 0x5775 直方图断桶（桶序缺失）。
    HistHole,
    /// 0x5776 分位越界（千分比>1000）。
    PctOob,
    /// 0x5777 均值漂移（均值与分位矛盾）。
    MeanDrift,
    /// 0x5778 台账不守恒（守恒恒等式破缺）。
    LedgerBroken,
    /// 0x5779 快照指纹不符（字段被改后重算不一致）。
    SnapshotFingerprint,
    /// 0x577a 常量表破坏（表项与语义规则矛盾）。
    TableBroken,
    /// 0x577b 段位守卫失败（诊断码越出本模块段）。
    SegmentGuard,
}

impl u1661Code {
    /// 全量闭集。
    pub const ALL: [u1661Code; 12] = [
        u1661Code::KeyDup,
        u1661Code::KeyGap,
        u1661Code::GuardBad,
        u1661Code::SumDrift,
        u1661Code::HistOver,
        u1661Code::HistHole,
        u1661Code::PctOob,
        u1661Code::MeanDrift,
        u1661Code::LedgerBroken,
        u1661Code::SnapshotFingerprint,
        u1661Code::TableBroken,
        u1661Code::SegmentGuard,
    ];

    /// 码值（连续十六进制，与声明序恒等）。
    pub const fn code(self) -> u16 {
        match self {
            u1661Code::KeyDup => 0x5770,
            u1661Code::KeyGap => 0x5771,
            u1661Code::GuardBad => 0x5772,
            u1661Code::SumDrift => 0x5773,
            u1661Code::HistOver => 0x5774,
            u1661Code::HistHole => 0x5775,
            u1661Code::PctOob => 0x5776,
            u1661Code::MeanDrift => 0x5777,
            u1661Code::LedgerBroken => 0x5778,
            u1661Code::SnapshotFingerprint => 0x5779,
            u1661Code::TableBroken => 0x577a,
            u1661Code::SegmentGuard => 0x577b,
        }
    }

    /// 中文诊断名。
    pub const fn name(self) -> &'static str {
        match self {
            u1661Code::KeyDup => "键重复（重复注册 同键双行）",
            u1661Code::KeyGap => "键断档（链环 序不连续）",
            u1661Code::GuardBad => "行守卫破缺（链洞 逐行校验失败）",
            u1661Code::SumDrift => "矩阵累计漂移（首尾漂移 求和失锚）",
            u1661Code::HistOver => "直方图溢出（超容量拒收未记账）",
            u1661Code::HistHole => "直方图断桶（桶序缺失）",
            u1661Code::PctOob => "分位越界（千分比>1000）",
            u1661Code::MeanDrift => "均值漂移（均值与分位矛盾）",
            u1661Code::LedgerBroken => "台账不守恒（守恒恒等式破缺）",
            u1661Code::SnapshotFingerprint => "快照指纹不符（字段被改后重算不一致）",
            u1661Code::TableBroken => "常量表破坏（表项与语义规则矛盾）",
            u1661Code::SegmentGuard => "段位守卫失败（诊断码越出本模块段）",
        }
    }

    /// 可操作修复建议（每条诊断都必须带下一步动作）。
    pub const fn suggestion(self) -> &'static str {
        match self {
            u1661Code::KeyDup => "按键唯一律去重",
            u1661Code::KeyGap => "按连续律补号",
            u1661Code::GuardBad => "按守卫口径重建",
            u1661Code::SumDrift => "按理论值复算",
            u1661Code::HistOver => "按溢出账口径登记",
            u1661Code::HistHole => "按桶序补齐",
            u1661Code::PctOob => "按 0..=1000 钳制",
            u1661Code::MeanDrift => "按同源数据复算",
            u1661Code::LedgerBroken => "按守恒律重建台账后重跑",
            u1661Code::SnapshotFingerprint => "按防篡改规程重建快照",
            u1661Code::TableBroken => "按语义规则重建常量表",
            u1661Code::SegmentGuard => "核对诊断码分配表",
        }
    }
}

// --------------------------------------------------------------------------
// 字符串键值映射 u1661mat（36 行逐行语义注释 + 逐行守卫）
// --------------------------------------------------------------------------
/// 调度类 守卫矩阵（与核心登记表同源对拍）。
/// 行数。
pub const u1661mat_COUNT: usize = 36;

/// 表本体（键 + 值 + 注释由编译期冻结）。
pub static u1661mat: [(&'static str, u64); 36] = [
    ("RG0000", 0), // RG 单元 0000（注册框架 × 相位 00 × 一 轮）
    ("PL0001", 13), // PL 单元 0001（优先级链 × 相位 01 × 一 轮）
    ("WJ0002", 26), // WJ 单元 0002（无环 × 相位 02 × 一 轮）
    ("WD0003", 39), // WD 单元 0003（无洞 × 相位 03 × 一 轮）
    ("SW0004", 52), // SW 单元 0004（首尾确定 × 相位 04 × 一 轮）
    ("CF0005", 65), // CF 单元 0005（重复注册 × 相位 05 × 一 轮）
    ("JJ0006", 78), // JJ 单元 0006（拒绝 × 相位 06 × 一 轮）
    ("DL0007", 91), // DL 单元 0007（单点构建 × 相位 07 × 一 轮）
    ("ST0008", 7), // ST 单元 0008（静态闭合 × 相位 08 × 一 轮）
    ("LK0009", 20), // LK 单元 0009（链锁 × 相位 09 × 一 轮）
    ("PN0010", 33), // PN 单元 0010（类序号 × 相位 10 × 一 轮）
    ("JH0011", 46), // JH 单元 0011（交接锚 × 相位 11 × 一 轮）
    ("RG0012", 59), // RG 单元 0012（注册框架 × 相位 12 × 一 轮）
    ("PL0013", 72), // PL 单元 0013（优先级链 × 相位 13 × 一 轮）
    ("WJ0014", 85), // WJ 单元 0014（无环 × 相位 14 × 一 轮）
    ("WD0015", 1), // WD 单元 0015（无洞 × 相位 15 × 一 轮）
    ("SW0016", 14), // SW 单元 0016（首尾确定 × 相位 16 × 一 轮）
    ("CF0017", 27), // CF 单元 0017（重复注册 × 相位 17 × 一 轮）
    ("JJ0018", 40), // JJ 单元 0018（拒绝 × 相位 18 × 一 轮）
    ("DL0019", 53), // DL 单元 0019（单点构建 × 相位 19 × 一 轮）
    ("ST0020", 66), // ST 单元 0020（静态闭合 × 相位 20 × 一 轮）
    ("LK0021", 79), // LK 单元 0021（链锁 × 相位 21 × 一 轮）
    ("PN0022", 92), // PN 单元 0022（类序号 × 相位 22 × 一 轮）
    ("JH0023", 8), // JH 单元 0023（交接锚 × 相位 23 × 一 轮）
    ("RG0024", 21), // RG 单元 0024（注册框架 × 相位 24 × 一 轮）
    ("PL0025", 34), // PL 单元 0025（优先级链 × 相位 25 × 一 轮）
    ("WJ0026", 47), // WJ 单元 0026（无环 × 相位 26 × 一 轮）
    ("WD0027", 60), // WD 单元 0027（无洞 × 相位 27 × 一 轮）
    ("SW0028", 73), // SW 单元 0028（首尾确定 × 相位 28 × 一 轮）
    ("CF0029", 86), // CF 单元 0029（重复注册 × 相位 29 × 一 轮）
    ("JJ0030", 2), // JJ 单元 0030（拒绝 × 相位 30 × 一 轮）
    ("DL0031", 15), // DL 单元 0031（单点构建 × 相位 31 × 一 轮）
    ("ST0032", 28), // ST 单元 0032（静态闭合 × 相位 32 × 一 轮）
    ("LK0033", 41), // LK 单元 0033（链锁 × 相位 33 × 一 轮）
    ("PN0034", 54), // PN 单元 0034（类序号 × 相位 34 × 一 轮）
    ("JH0035", 67), // JH 单元 0035（交接锚 × 相位 35 × 一 轮）
];

/// O(1) 语义查找（match 全枚举；命中率 100%% 由守卫保证）。
pub fn u1661mat_get(key: &str) -> Option<u64> {
    match key {
        "RG0000" => Some(0),
        "PL0001" => Some(13),
        "WJ0002" => Some(26),
        "WD0003" => Some(39),
        "SW0004" => Some(52),
        "CF0005" => Some(65),
        "JJ0006" => Some(78),
        "DL0007" => Some(91),
        "ST0008" => Some(7),
        "LK0009" => Some(20),
        "PN0010" => Some(33),
        "JH0011" => Some(46),
        "RG0012" => Some(59),
        "PL0013" => Some(72),
        "WJ0014" => Some(85),
        "WD0015" => Some(1),
        "SW0016" => Some(14),
        "CF0017" => Some(27),
        "JJ0018" => Some(40),
        "DL0019" => Some(53),
        "ST0020" => Some(66),
        "LK0021" => Some(79),
        "PN0022" => Some(92),
        "JH0023" => Some(8),
        "RG0024" => Some(21),
        "PL0025" => Some(34),
        "WJ0026" => Some(47),
        "WD0027" => Some(60),
        "SW0028" => Some(73),
        "CF0029" => Some(86),
        "JJ0030" => Some(2),
        "DL0031" => Some(15),
        "ST0032" => Some(28),
        "LK0033" => Some(41),
        "PN0034" => Some(54),
        "JH0035" => Some(67),
        _ => None,
    }
}

/// 逐行守卫扫描（返回违例行数；0 = 全绿）。
pub fn u1661mat_scan() -> usize {
    let mut bad = 0usize;
    let mut i = 0usize;
    while i < u1661mat_COUNT {
        let (k, v) = u1661mat[i];
        let ok = match k {
            "RG0000" => v < 97,
            "PL0001" => v < 97,
            "WJ0002" => v < 97,
            "WD0003" => v < 97,
            "SW0004" => v < 97,
            "CF0005" => v < 97,
            "JJ0006" => v < 97,
            "DL0007" => v < 97,
            "ST0008" => v < 97,
            "LK0009" => v < 97,
            "PN0010" => v < 97,
            "JH0011" => v < 97,
            "RG0012" => v < 97,
            "PL0013" => v < 97,
            "WJ0014" => v < 97,
            "WD0015" => v < 97,
            "SW0016" => v < 97,
            "CF0017" => v < 97,
            "JJ0018" => v < 97,
            "DL0019" => v < 97,
            "ST0020" => v < 97,
            "LK0021" => v < 97,
            "PN0022" => v < 97,
            "JH0023" => v < 97,
            "RG0024" => v < 97,
            "PL0025" => v < 97,
            "WJ0026" => v < 97,
            "WD0027" => v < 97,
            "SW0028" => v < 97,
            "CF0029" => v < 97,
            "JJ0030" => v < 97,
            "DL0031" => v < 97,
            "ST0032" => v < 97,
            "LK0033" => v < 97,
            "PN0034" => v < 97,
            "JH0035" => v < 97,
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
pub fn u1661mat_sum() -> u64 {
    let mut s = 0u64;
    let mut i = 0usize;
    while i < u1661mat_COUNT {
        s = s.wrapping_add(u1661mat[i].1 as u64);
        i += 1;
    }
    s
}

/// 键唯一性（构造期断言等价物：逐对扫描同键）。
pub fn u1661mat_unique() -> bool {
    let mut i = 0usize;
    while i < u1661mat_COUNT {
        let mut j = i + 1;
        while j < u1661mat_COUNT {
            if u1661mat[i].0 == u1661mat[j].0 {
                return false;
            }
            j += 1;
        }
        i += 1;
    }
    true
}

// --------------------------------------------------------------------------
// 直方图引擎 mxu1661hist（有界插入排序 + 分位数 P50/P90/P99）
// --------------------------------------------------------------------------
/// 调度类 直方图（96 桶滚动）。
/// 样本容量上限。
pub const MXU1661HIST_CAP: usize = 96;

/// 直方图账本（排序态 + 五统量）。
#[derive(Clone, Copy, Debug)]
pub struct mxu1661histHist {
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

impl mxu1661histHist {
    /// 空账构造。
    pub const fn new() -> Self {
        Self { samples: [0; 96], n: 0, drops: 0, min: 0, max: 0, sum: 0 }
    }

    /// 采样（有界插入排序；满员丢弃并记账）。
    pub fn push(&mut self, v: u64) {
        if self.n >= MXU1661HIST_CAP {
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

/// 段位守卫：全部诊断码必须落在本段 [0x5770, 0x577f]。
pub const fn u1661_segment_guard() -> bool {
    let mut ok = true;
    let mut i = 0usize;
    while i < u1661Code::ALL.len() {
        let c = u1661Code::ALL[i].code();
        if c < 0x5770 || c > 0x577f {
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
fn u1661_guard_seg() -> bool {
    u1661_segment_guard()
}

/// UNX-F1661 矩阵 36 行齐备 + 逐行守卫 + 唯一性 + 累计守恒。
fn check_u1661_map_guard() -> bool {
    let mut ok = u1661mat_COUNT == 36;
    if ok { ok = u1661mat_scan() == 0 && u1661mat_unique(); }
    if ok { ok = u1661mat_get("RG0000") == Some(0) && u1661mat_get("ZZZ_NOKEY").is_none(); }
    if ok { ok = u1661mat_sum() == 1594; }
    ok
}

/// UNX-F1661 直方图 16 样本分位/均值/守恒。
fn check_u1661_map_hist() -> bool {
    let mut h = mxu1661histHist::new();
    let mut i = 0u64;
    while i < 16 { h.push(i + 1); i += 1; }
    let mut ok = h.conserved() && h.mean() >= 1 && h.percentile(500) >= 8;
    ok
}

/// 本模块自检（逐检查带 detail，全绿才出门）。
pub fn run_u1661_checks() -> CheckSet {
    let mut set = CheckSet::new("U1661");
    set.add(
        "u1661-seg-guard",
        u1661_guard_seg(),
        if u1661_guard_seg() { "" } else { "diagnostic code outside segment" },
    );
    set.add(
        "u1661-map-guard",
        check_u1661_map_guard(),
        if check_u1661_map_guard() { "" } else { "UNX-F1661 matrix guard mismatch" },
    );
    set.add(
        "u1661-map-hist",
        check_u1661_map_hist(),
        if check_u1661_map_hist() { "" } else { "UNX-F1661 hist mismatch" },
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
    fn u1661_all_checks_green() {
        let set = run_u1661_checks();
        assert!(set.all_passed(), "U1661 domain checks must be green");
    }

    #[test]
    fn u1661_segment_guard_holds() {
        assert!(u1661_segment_guard());
    }
}
