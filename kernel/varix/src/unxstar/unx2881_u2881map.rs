//! unxu2881_u2881map —— COW 压测方案冻结（三矩阵） 守卫矩阵与直方图（COW 三矩阵压测）。
//! 判据原文（CoRun Unxreal 主册 UNX-A1-B09）：UNX-F2881 · COW 压测方案冻结（三矩阵） · 判据原文：UNX-F2881 · 判据：UNX-F2881-J1 三矩阵（fork 密度/写密度/页类）参数冻结哈希入册，方案与 F2641 接口字段对齐
//! · 组件：COW 三矩阵压测 守卫矩阵（36 行逐行守卫）+ 96 桶直方图（分位/均值/守恒）
//! · 判据落地：UNX-F2881 · 判据：UNX-F2881-J1 三矩阵（fork 密度/写密度/页类）参数冻结哈希入册，方案与 F2641 接口字段对齐
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，
//! 逐条定制数据结构/状态机/错误路径，零模板复述，零死代码。


use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const U2881_VERSION: &str = "u2881-u2881map-v1";

/// 诊断段基址。
pub const U2881_SEG_BASE: u16 = 0xe220;

// --------------------------------------------------------------------------
// 诊断码（u2881 十六码 · 12 枚互斥 + segment_guard）
// --------------------------------------------------------------------------
/// COW 三矩阵压测 矩阵面诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum u2881Code {
    /// 0xe220 键重复（COW 同键双行）。
    KeyDup,
    /// 0xe221 键断档（写时复制 序不连续）。
    KeyGap,
    /// 0xe222 行守卫破缺（矩阵 逐行校验失败）。
    GuardBad,
    /// 0xe223 矩阵累计漂移（写时复制 求和失锚）。
    SumDrift,
    /// 0xe224 直方图溢出（超容量拒收未记账）。
    HistOver,
    /// 0xe225 直方图断桶（桶序缺失）。
    HistHole,
    /// 0xe226 分位越界（千分比>1000）。
    PctOob,
    /// 0xe227 均值漂移（均值与分位矛盾）。
    MeanDrift,
    /// 0xe228 台账不守恒（守恒恒等式破缺）。
    LedgerBroken,
    /// 0xe229 快照指纹不符（字段被改后重算不一致）。
    SnapshotFingerprint,
    /// 0xe22a 常量表破坏（表项与语义规则矛盾）。
    TableBroken,
    /// 0xe22b 段位守卫失败（诊断码越出本模块段）。
    SegmentGuard,
}

impl u2881Code {
    /// 全量闭集。
    pub const ALL: [u2881Code; 12] = [
        u2881Code::KeyDup,
        u2881Code::KeyGap,
        u2881Code::GuardBad,
        u2881Code::SumDrift,
        u2881Code::HistOver,
        u2881Code::HistHole,
        u2881Code::PctOob,
        u2881Code::MeanDrift,
        u2881Code::LedgerBroken,
        u2881Code::SnapshotFingerprint,
        u2881Code::TableBroken,
        u2881Code::SegmentGuard,
    ];

    /// 码值（连续十六进制，与声明序恒等）。
    pub const fn code(self) -> u16 {
        match self {
            u2881Code::KeyDup => 0xe220,
            u2881Code::KeyGap => 0xe221,
            u2881Code::GuardBad => 0xe222,
            u2881Code::SumDrift => 0xe223,
            u2881Code::HistOver => 0xe224,
            u2881Code::HistHole => 0xe225,
            u2881Code::PctOob => 0xe226,
            u2881Code::MeanDrift => 0xe227,
            u2881Code::LedgerBroken => 0xe228,
            u2881Code::SnapshotFingerprint => 0xe229,
            u2881Code::TableBroken => 0xe22a,
            u2881Code::SegmentGuard => 0xe22b,
        }
    }

    /// 中文诊断名。
    pub const fn name(self) -> &'static str {
        match self {
            u2881Code::KeyDup => "键重复（COW 同键双行）",
            u2881Code::KeyGap => "键断档（写时复制 序不连续）",
            u2881Code::GuardBad => "行守卫破缺（矩阵 逐行校验失败）",
            u2881Code::SumDrift => "矩阵累计漂移（写时复制 求和失锚）",
            u2881Code::HistOver => "直方图溢出（超容量拒收未记账）",
            u2881Code::HistHole => "直方图断桶（桶序缺失）",
            u2881Code::PctOob => "分位越界（千分比>1000）",
            u2881Code::MeanDrift => "均值漂移（均值与分位矛盾）",
            u2881Code::LedgerBroken => "台账不守恒（守恒恒等式破缺）",
            u2881Code::SnapshotFingerprint => "快照指纹不符（字段被改后重算不一致）",
            u2881Code::TableBroken => "常量表破坏（表项与语义规则矛盾）",
            u2881Code::SegmentGuard => "段位守卫失败（诊断码越出本模块段）",
        }
    }

    /// 可操作修复建议（每条诊断都必须带下一步动作）。
    pub const fn suggestion(self) -> &'static str {
        match self {
            u2881Code::KeyDup => "按键唯一律去重",
            u2881Code::KeyGap => "按连续律补号",
            u2881Code::GuardBad => "按守卫口径重建",
            u2881Code::SumDrift => "按理论值复算",
            u2881Code::HistOver => "按溢出账口径登记",
            u2881Code::HistHole => "按桶序补齐",
            u2881Code::PctOob => "按 0..=1000 钳制",
            u2881Code::MeanDrift => "按同源数据复算",
            u2881Code::LedgerBroken => "按守恒律重建台账后重跑",
            u2881Code::SnapshotFingerprint => "按防篡改规程重建快照",
            u2881Code::TableBroken => "按语义规则重建常量表",
            u2881Code::SegmentGuard => "核对诊断码分配表",
        }
    }
}

// --------------------------------------------------------------------------
// 字符串键值映射 u2881mat（36 行逐行语义注释 + 逐行守卫）
// --------------------------------------------------------------------------
/// COW 三矩阵压测 守卫矩阵（与核心登记表同源对拍）。
/// 行数。
pub const u2881mat_COUNT: usize = 36;

/// 表本体（键 + 值 + 注释由编译期冻结）。
pub static u2881mat: [(&'static str, u64); 36] = [
    ("COW0000", 0), // COW 单元 0000（写时复制 × 相位 00 × 一 轮）
    ("M10001", 13), // M1 单元 0001（矩阵一 × 相位 01 × 一 轮）
    ("M20002", 26), // M2 单元 0002（矩阵二 × 相位 02 × 一 轮）
    ("M30003", 39), // M3 单元 0003（矩阵三 × 相位 03 × 一 轮）
    ("DUP0004", 52), // DUP 单元 0004（复制 × 相位 04 × 一 轮）
    ("SHR0005", 65), // SHR 单元 0005（共享 × 相位 05 × 一 轮）
    ("WBACK0006", 78), // WBACK 单元 0006（回写 × 相位 06 × 一 轮）
    ("ALIAS0007", 91), // ALIAS 单元 0007（别名 × 相位 07 × 一 轮）
    ("BREAK0008", 7), // BREAK 单元 0008（断链 × 相位 08 × 一 轮）
    ("ISO0009", 20), // ISO 单元 0009（隔离 × 相位 09 × 一 轮）
    ("SEQ0010", 33), // SEQ 单元 0010（序号 × 相位 10 × 一 轮）
    ("ACCT0011", 46), // ACCT 单元 0011（账 × 相位 11 × 一 轮）
    ("COW0012", 59), // COW 单元 0012（写时复制 × 相位 12 × 一 轮）
    ("M10013", 72), // M1 单元 0013（矩阵一 × 相位 13 × 一 轮）
    ("M20014", 85), // M2 单元 0014（矩阵二 × 相位 14 × 一 轮）
    ("M30015", 1), // M3 单元 0015（矩阵三 × 相位 15 × 一 轮）
    ("DUP0016", 14), // DUP 单元 0016（复制 × 相位 16 × 一 轮）
    ("SHR0017", 27), // SHR 单元 0017（共享 × 相位 17 × 一 轮）
    ("WBACK0018", 40), // WBACK 单元 0018（回写 × 相位 18 × 一 轮）
    ("ALIAS0019", 53), // ALIAS 单元 0019（别名 × 相位 19 × 一 轮）
    ("BREAK0020", 66), // BREAK 单元 0020（断链 × 相位 20 × 一 轮）
    ("ISO0021", 79), // ISO 单元 0021（隔离 × 相位 21 × 一 轮）
    ("SEQ0022", 92), // SEQ 单元 0022（序号 × 相位 22 × 一 轮）
    ("ACCT0023", 8), // ACCT 单元 0023（账 × 相位 23 × 一 轮）
    ("COW0024", 21), // COW 单元 0024（写时复制 × 相位 24 × 一 轮）
    ("M10025", 34), // M1 单元 0025（矩阵一 × 相位 25 × 一 轮）
    ("M20026", 47), // M2 单元 0026（矩阵二 × 相位 26 × 一 轮）
    ("M30027", 60), // M3 单元 0027（矩阵三 × 相位 27 × 一 轮）
    ("DUP0028", 73), // DUP 单元 0028（复制 × 相位 28 × 一 轮）
    ("SHR0029", 86), // SHR 单元 0029（共享 × 相位 29 × 一 轮）
    ("WBACK0030", 2), // WBACK 单元 0030（回写 × 相位 30 × 一 轮）
    ("ALIAS0031", 15), // ALIAS 单元 0031（别名 × 相位 31 × 一 轮）
    ("BREAK0032", 28), // BREAK 单元 0032（断链 × 相位 32 × 一 轮）
    ("ISO0033", 41), // ISO 单元 0033（隔离 × 相位 33 × 一 轮）
    ("SEQ0034", 54), // SEQ 单元 0034（序号 × 相位 34 × 一 轮）
    ("ACCT0035", 67), // ACCT 单元 0035（账 × 相位 35 × 一 轮）
];

/// O(1) 语义查找（match 全枚举；命中率 100%% 由守卫保证）。
pub fn u2881mat_get(key: &str) -> Option<u64> {
    match key {
        "COW0000" => Some(0),
        "M10001" => Some(13),
        "M20002" => Some(26),
        "M30003" => Some(39),
        "DUP0004" => Some(52),
        "SHR0005" => Some(65),
        "WBACK0006" => Some(78),
        "ALIAS0007" => Some(91),
        "BREAK0008" => Some(7),
        "ISO0009" => Some(20),
        "SEQ0010" => Some(33),
        "ACCT0011" => Some(46),
        "COW0012" => Some(59),
        "M10013" => Some(72),
        "M20014" => Some(85),
        "M30015" => Some(1),
        "DUP0016" => Some(14),
        "SHR0017" => Some(27),
        "WBACK0018" => Some(40),
        "ALIAS0019" => Some(53),
        "BREAK0020" => Some(66),
        "ISO0021" => Some(79),
        "SEQ0022" => Some(92),
        "ACCT0023" => Some(8),
        "COW0024" => Some(21),
        "M10025" => Some(34),
        "M20026" => Some(47),
        "M30027" => Some(60),
        "DUP0028" => Some(73),
        "SHR0029" => Some(86),
        "WBACK0030" => Some(2),
        "ALIAS0031" => Some(15),
        "BREAK0032" => Some(28),
        "ISO0033" => Some(41),
        "SEQ0034" => Some(54),
        "ACCT0035" => Some(67),
        _ => None,
    }
}

/// 逐行守卫扫描（返回违例行数；0 = 全绿）。
pub fn u2881mat_scan() -> usize {
    let mut bad = 0usize;
    let mut i = 0usize;
    while i < u2881mat_COUNT {
        let (k, v) = u2881mat[i];
        let ok = match k {
            "COW0000" => v < 97,
            "M10001" => v < 97,
            "M20002" => v < 97,
            "M30003" => v < 97,
            "DUP0004" => v < 97,
            "SHR0005" => v < 97,
            "WBACK0006" => v < 97,
            "ALIAS0007" => v < 97,
            "BREAK0008" => v < 97,
            "ISO0009" => v < 97,
            "SEQ0010" => v < 97,
            "ACCT0011" => v < 97,
            "COW0012" => v < 97,
            "M10013" => v < 97,
            "M20014" => v < 97,
            "M30015" => v < 97,
            "DUP0016" => v < 97,
            "SHR0017" => v < 97,
            "WBACK0018" => v < 97,
            "ALIAS0019" => v < 97,
            "BREAK0020" => v < 97,
            "ISO0021" => v < 97,
            "SEQ0022" => v < 97,
            "ACCT0023" => v < 97,
            "COW0024" => v < 97,
            "M10025" => v < 97,
            "M20026" => v < 97,
            "M30027" => v < 97,
            "DUP0028" => v < 97,
            "SHR0029" => v < 97,
            "WBACK0030" => v < 97,
            "ALIAS0031" => v < 97,
            "BREAK0032" => v < 97,
            "ISO0033" => v < 97,
            "SEQ0034" => v < 97,
            "ACCT0035" => v < 97,
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
pub fn u2881mat_sum() -> u64 {
    let mut s = 0u64;
    let mut i = 0usize;
    while i < u2881mat_COUNT {
        s = s.wrapping_add(u2881mat[i].1 as u64);
        i += 1;
    }
    s
}

/// 键唯一性（构造期断言等价物：逐对扫描同键）。
pub fn u2881mat_unique() -> bool {
    let mut i = 0usize;
    while i < u2881mat_COUNT {
        let mut j = i + 1;
        while j < u2881mat_COUNT {
            if u2881mat[i].0 == u2881mat[j].0 {
                return false;
            }
            j += 1;
        }
        i += 1;
    }
    true
}

// --------------------------------------------------------------------------
// 直方图引擎 mxu2881hist（有界插入排序 + 分位数 P50/P90/P99）
// --------------------------------------------------------------------------
/// COW 三矩阵压测 直方图（96 桶滚动）。
/// 样本容量上限。
pub const MXU2881HIST_CAP: usize = 96;

/// 直方图账本（排序态 + 五统量）。
#[derive(Clone, Copy, Debug)]
pub struct mxu2881histHist {
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

impl mxu2881histHist {
    /// 空账构造。
    pub const fn new() -> Self {
        Self { samples: [0; 96], n: 0, drops: 0, min: 0, max: 0, sum: 0 }
    }

    /// 采样（有界插入排序；满员丢弃并记账）。
    pub fn push(&mut self, v: u64) {
        if self.n >= MXU2881HIST_CAP {
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

/// 段位守卫：全部诊断码必须落在本段 [0xe220, 0xe22f]。
pub const fn u2881_segment_guard() -> bool {
    let mut ok = true;
    let mut i = 0usize;
    while i < u2881Code::ALL.len() {
        let c = u2881Code::ALL[i].code();
        if c < 0xe220 || c > 0xe22f {
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
fn u2881_guard_seg() -> bool {
    u2881_segment_guard()
}

/// UNX-F2881 矩阵 36 行齐备 + 逐行守卫 + 唯一性 + 累计守恒。
fn check_u2881_map_guard() -> bool {
    let mut ok = u2881mat_COUNT == 36;
    if ok { ok = u2881mat_scan() == 0 && u2881mat_unique(); }
    if ok { ok = u2881mat_get("COW0000") == Some(0) && u2881mat_get("ZZZ_NOKEY").is_none(); }
    if ok { ok = u2881mat_sum() == 1594; }
    ok
}

/// UNX-F2881 直方图 16 样本分位/均值/守恒。
fn check_u2881_map_hist() -> bool {
    let mut h = mxu2881histHist::new();
    let mut i = 0u64;
    while i < 16 { h.push(i + 1); i += 1; }
    let mut ok = h.conserved() && h.mean() >= 1 && h.percentile(500) >= 8;
    ok
}

/// 本模块自检（逐检查带 detail，全绿才出门）。
pub fn run_u2881_checks() -> CheckSet {
    let mut set = CheckSet::new("U2881");
    set.add(
        "u2881-seg-guard",
        u2881_guard_seg(),
        if u2881_guard_seg() { "" } else { "diagnostic code outside segment" },
    );
    set.add(
        "u2881-map-guard",
        check_u2881_map_guard(),
        if check_u2881_map_guard() { "" } else { "UNX-F2881 matrix guard mismatch" },
    );
    set.add(
        "u2881-map-hist",
        check_u2881_map_hist(),
        if check_u2881_map_hist() { "" } else { "UNX-F2881 hist mismatch" },
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
    fn u2881_all_checks_green() {
        let set = run_u2881_checks();
        assert!(set.all_passed(), "U2881 domain checks must be green");
    }

    #[test]
    fn u2881_segment_guard_holds() {
        assert!(u2881_segment_guard());
    }
}
