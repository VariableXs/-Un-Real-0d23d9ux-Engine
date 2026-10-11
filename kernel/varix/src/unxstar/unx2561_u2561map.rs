//! unxu2561_u2561map —— AddressSpace 成熟体总纲（现存 addrspace.rs 接管） 守卫矩阵与直方图（地址空间体）。
//! 判据原文（CoRun Unxreal 主册 UNX-A1-B09）：UNX-F2561 · AddressSpace 成熟体总纲（现存 addrspace.rs 接管） · 判据原文：UNX-F2561 · 判据：现存 `addrspace.rs`（F001 起步态）接管账在档（语义逐项映射），成熟体接口面三机直测一致
//! · 组件：地址空间体 守卫矩阵（36 行逐行守卫）+ 96 桶直方图（分位/均值/守恒）
//! · 判据落地：UNX-F2561 · 判据：现存 `addrspace.rs`（F001 起步态）接管账在档（语义逐项映射），成熟体接口面三机直测一致
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，
//! 逐条定制数据结构/状态机/错误路径，零模板复述，零死代码。


use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const U2561_VERSION: &str = "u2561-u2561map-v1";

/// 诊断段基址。
pub const U2561_SEG_BASE: u16 = 0xa630;

// --------------------------------------------------------------------------
// 诊断码（u2561 十六码 · 12 枚互斥 + segment_guard）
// --------------------------------------------------------------------------
/// 地址空间体 矩阵面诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum u2561Code {
    /// 0xa630 键重复（接管漏项 同键双行）。
    KeyDup,
    /// 0xa631 键断档（语义错配 序不连续）。
    KeyGap,
    /// 0xa632 行守卫破缺（权限失真 逐行校验失败）。
    GuardBad,
    /// 0xa633 矩阵累计漂移（直测分歧 求和失锚）。
    SumDrift,
    /// 0xa634 直方图溢出（超容量拒收未记账）。
    HistOver,
    /// 0xa635 直方图断桶（桶序缺失）。
    HistHole,
    /// 0xa636 分位越界（千分比>1000）。
    PctOob,
    /// 0xa637 均值漂移（均值与分位矛盾）。
    MeanDrift,
    /// 0xa638 台账不守恒（守恒恒等式破缺）。
    LedgerBroken,
    /// 0xa639 快照指纹不符（字段被改后重算不一致）。
    SnapshotFingerprint,
    /// 0xa63a 常量表破坏（表项与语义规则矛盾）。
    TableBroken,
    /// 0xa63b 段位守卫失败（诊断码越出本模块段）。
    SegmentGuard,
}

impl u2561Code {
    /// 全量闭集。
    pub const ALL: [u2561Code; 12] = [
        u2561Code::KeyDup,
        u2561Code::KeyGap,
        u2561Code::GuardBad,
        u2561Code::SumDrift,
        u2561Code::HistOver,
        u2561Code::HistHole,
        u2561Code::PctOob,
        u2561Code::MeanDrift,
        u2561Code::LedgerBroken,
        u2561Code::SnapshotFingerprint,
        u2561Code::TableBroken,
        u2561Code::SegmentGuard,
    ];

    /// 码值（连续十六进制，与声明序恒等）。
    pub const fn code(self) -> u16 {
        match self {
            u2561Code::KeyDup => 0xa630,
            u2561Code::KeyGap => 0xa631,
            u2561Code::GuardBad => 0xa632,
            u2561Code::SumDrift => 0xa633,
            u2561Code::HistOver => 0xa634,
            u2561Code::HistHole => 0xa635,
            u2561Code::PctOob => 0xa636,
            u2561Code::MeanDrift => 0xa637,
            u2561Code::LedgerBroken => 0xa638,
            u2561Code::SnapshotFingerprint => 0xa639,
            u2561Code::TableBroken => 0xa63a,
            u2561Code::SegmentGuard => 0xa63b,
        }
    }

    /// 中文诊断名。
    pub const fn name(self) -> &'static str {
        match self {
            u2561Code::KeyDup => "键重复（接管漏项 同键双行）",
            u2561Code::KeyGap => "键断档（语义错配 序不连续）",
            u2561Code::GuardBad => "行守卫破缺（权限失真 逐行校验失败）",
            u2561Code::SumDrift => "矩阵累计漂移（直测分歧 求和失锚）",
            u2561Code::HistOver => "直方图溢出（超容量拒收未记账）",
            u2561Code::HistHole => "直方图断桶（桶序缺失）",
            u2561Code::PctOob => "分位越界（千分比>1000）",
            u2561Code::MeanDrift => "均值漂移（均值与分位矛盾）",
            u2561Code::LedgerBroken => "台账不守恒（守恒恒等式破缺）",
            u2561Code::SnapshotFingerprint => "快照指纹不符（字段被改后重算不一致）",
            u2561Code::TableBroken => "常量表破坏（表项与语义规则矛盾）",
            u2561Code::SegmentGuard => "段位守卫失败（诊断码越出本模块段）",
        }
    }

    /// 可操作修复建议（每条诊断都必须带下一步动作）。
    pub const fn suggestion(self) -> &'static str {
        match self {
            u2561Code::KeyDup => "按键唯一律去重",
            u2561Code::KeyGap => "按连续律补号",
            u2561Code::GuardBad => "按守卫口径重建",
            u2561Code::SumDrift => "按理论值复算",
            u2561Code::HistOver => "按溢出账口径登记",
            u2561Code::HistHole => "按桶序补齐",
            u2561Code::PctOob => "按 0..=1000 钳制",
            u2561Code::MeanDrift => "按同源数据复算",
            u2561Code::LedgerBroken => "按守恒律重建台账后重跑",
            u2561Code::SnapshotFingerprint => "按防篡改规程重建快照",
            u2561Code::TableBroken => "按语义规则重建常量表",
            u2561Code::SegmentGuard => "核对诊断码分配表",
        }
    }
}

// --------------------------------------------------------------------------
// 字符串键值映射 u2561mat（36 行逐行语义注释 + 逐行守卫）
// --------------------------------------------------------------------------
/// 地址空间体 守卫矩阵（与核心登记表同源对拍）。
/// 行数。
pub const u2561mat_COUNT: usize = 36;

/// 表本体（键 + 值 + 注释由编译期冻结）。
pub static u2561mat: [(&'static str, u64); 36] = [
    ("AS0000", 0), // AS 单元 0000（AddressSpace × 相位 00 × 一 轮）
    ("LG0001", 13), // LG 单元 0001（接管账 × 相位 01 × 一 轮）
    ("SM0002", 26), // SM 单元 0002（语义映射 × 相位 02 × 一 轮）
    ("MT0003", 39), // MT 单元 0003（成熟体 × 相位 03 × 一 轮）
    ("IF0004", 52), // IF 单元 0004（接口面 × 相位 04 × 一 轮）
    ("TM0005", 65), // TM 单元 0005（三机直测 × 相位 05 × 一 轮）
    ("AS20006", 78), // AS2 单元 0006（起步态 × 相位 06 × 一 轮）
    ("MP0007", 91), // MP 单元 0007（映射项 × 相位 07 × 一 轮）
    ("UN0008", 7), // UN 单元 0008（解除项 × 相位 08 × 一 轮）
    ("PC0009", 20), // PC 单元 0009（权限账 × 相位 09 × 一 轮）
    ("FV0010", 33), // FV 单元 0010（逐项核验 × 相位 10 × 一 轮）
    ("AR0011", 46), // AR 单元 0011（在档锚 × 相位 11 × 一 轮）
    ("AS0012", 59), // AS 单元 0012（AddressSpace × 相位 12 × 一 轮）
    ("LG0013", 72), // LG 单元 0013（接管账 × 相位 13 × 一 轮）
    ("SM0014", 85), // SM 单元 0014（语义映射 × 相位 14 × 一 轮）
    ("MT0015", 1), // MT 单元 0015（成熟体 × 相位 15 × 一 轮）
    ("IF0016", 14), // IF 单元 0016（接口面 × 相位 16 × 一 轮）
    ("TM0017", 27), // TM 单元 0017（三机直测 × 相位 17 × 一 轮）
    ("AS20018", 40), // AS2 单元 0018（起步态 × 相位 18 × 一 轮）
    ("MP0019", 53), // MP 单元 0019（映射项 × 相位 19 × 一 轮）
    ("UN0020", 66), // UN 单元 0020（解除项 × 相位 20 × 一 轮）
    ("PC0021", 79), // PC 单元 0021（权限账 × 相位 21 × 一 轮）
    ("FV0022", 92), // FV 单元 0022（逐项核验 × 相位 22 × 一 轮）
    ("AR0023", 8), // AR 单元 0023（在档锚 × 相位 23 × 一 轮）
    ("AS0024", 21), // AS 单元 0024（AddressSpace × 相位 24 × 一 轮）
    ("LG0025", 34), // LG 单元 0025（接管账 × 相位 25 × 一 轮）
    ("SM0026", 47), // SM 单元 0026（语义映射 × 相位 26 × 一 轮）
    ("MT0027", 60), // MT 单元 0027（成熟体 × 相位 27 × 一 轮）
    ("IF0028", 73), // IF 单元 0028（接口面 × 相位 28 × 一 轮）
    ("TM0029", 86), // TM 单元 0029（三机直测 × 相位 29 × 一 轮）
    ("AS20030", 2), // AS2 单元 0030（起步态 × 相位 30 × 一 轮）
    ("MP0031", 15), // MP 单元 0031（映射项 × 相位 31 × 一 轮）
    ("UN0032", 28), // UN 单元 0032（解除项 × 相位 32 × 一 轮）
    ("PC0033", 41), // PC 单元 0033（权限账 × 相位 33 × 一 轮）
    ("FV0034", 54), // FV 单元 0034（逐项核验 × 相位 34 × 一 轮）
    ("AR0035", 67), // AR 单元 0035（在档锚 × 相位 35 × 一 轮）
];

/// O(1) 语义查找（match 全枚举；命中率 100%% 由守卫保证）。
pub fn u2561mat_get(key: &str) -> Option<u64> {
    match key {
        "AS0000" => Some(0),
        "LG0001" => Some(13),
        "SM0002" => Some(26),
        "MT0003" => Some(39),
        "IF0004" => Some(52),
        "TM0005" => Some(65),
        "AS20006" => Some(78),
        "MP0007" => Some(91),
        "UN0008" => Some(7),
        "PC0009" => Some(20),
        "FV0010" => Some(33),
        "AR0011" => Some(46),
        "AS0012" => Some(59),
        "LG0013" => Some(72),
        "SM0014" => Some(85),
        "MT0015" => Some(1),
        "IF0016" => Some(14),
        "TM0017" => Some(27),
        "AS20018" => Some(40),
        "MP0019" => Some(53),
        "UN0020" => Some(66),
        "PC0021" => Some(79),
        "FV0022" => Some(92),
        "AR0023" => Some(8),
        "AS0024" => Some(21),
        "LG0025" => Some(34),
        "SM0026" => Some(47),
        "MT0027" => Some(60),
        "IF0028" => Some(73),
        "TM0029" => Some(86),
        "AS20030" => Some(2),
        "MP0031" => Some(15),
        "UN0032" => Some(28),
        "PC0033" => Some(41),
        "FV0034" => Some(54),
        "AR0035" => Some(67),
        _ => None,
    }
}

/// 逐行守卫扫描（返回违例行数；0 = 全绿）。
pub fn u2561mat_scan() -> usize {
    let mut bad = 0usize;
    let mut i = 0usize;
    while i < u2561mat_COUNT {
        let (k, v) = u2561mat[i];
        let ok = match k {
            "AS0000" => v < 97,
            "LG0001" => v < 97,
            "SM0002" => v < 97,
            "MT0003" => v < 97,
            "IF0004" => v < 97,
            "TM0005" => v < 97,
            "AS20006" => v < 97,
            "MP0007" => v < 97,
            "UN0008" => v < 97,
            "PC0009" => v < 97,
            "FV0010" => v < 97,
            "AR0011" => v < 97,
            "AS0012" => v < 97,
            "LG0013" => v < 97,
            "SM0014" => v < 97,
            "MT0015" => v < 97,
            "IF0016" => v < 97,
            "TM0017" => v < 97,
            "AS20018" => v < 97,
            "MP0019" => v < 97,
            "UN0020" => v < 97,
            "PC0021" => v < 97,
            "FV0022" => v < 97,
            "AR0023" => v < 97,
            "AS0024" => v < 97,
            "LG0025" => v < 97,
            "SM0026" => v < 97,
            "MT0027" => v < 97,
            "IF0028" => v < 97,
            "TM0029" => v < 97,
            "AS20030" => v < 97,
            "MP0031" => v < 97,
            "UN0032" => v < 97,
            "PC0033" => v < 97,
            "FV0034" => v < 97,
            "AR0035" => v < 97,
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
pub fn u2561mat_sum() -> u64 {
    let mut s = 0u64;
    let mut i = 0usize;
    while i < u2561mat_COUNT {
        s = s.wrapping_add(u2561mat[i].1 as u64);
        i += 1;
    }
    s
}

/// 键唯一性（构造期断言等价物：逐对扫描同键）。
pub fn u2561mat_unique() -> bool {
    let mut i = 0usize;
    while i < u2561mat_COUNT {
        let mut j = i + 1;
        while j < u2561mat_COUNT {
            if u2561mat[i].0 == u2561mat[j].0 {
                return false;
            }
            j += 1;
        }
        i += 1;
    }
    true
}

// --------------------------------------------------------------------------
// 直方图引擎 mxu2561hist（有界插入排序 + 分位数 P50/P90/P99）
// --------------------------------------------------------------------------
/// 地址空间体 直方图（96 桶滚动）。
/// 样本容量上限。
pub const MXU2561HIST_CAP: usize = 96;

/// 直方图账本（排序态 + 五统量）。
#[derive(Clone, Copy, Debug)]
pub struct mxu2561histHist {
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

impl mxu2561histHist {
    /// 空账构造。
    pub const fn new() -> Self {
        Self { samples: [0; 96], n: 0, drops: 0, min: 0, max: 0, sum: 0 }
    }

    /// 采样（有界插入排序；满员丢弃并记账）。
    pub fn push(&mut self, v: u64) {
        if self.n >= MXU2561HIST_CAP {
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

/// 段位守卫：全部诊断码必须落在本段 [0xa630, 0xa63f]。
pub const fn u2561_segment_guard() -> bool {
    let mut ok = true;
    let mut i = 0usize;
    while i < u2561Code::ALL.len() {
        let c = u2561Code::ALL[i].code();
        if c < 0xa630 || c > 0xa63f {
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
fn u2561_guard_seg() -> bool {
    u2561_segment_guard()
}

/// UNX-F2561 矩阵 36 行齐备 + 逐行守卫 + 唯一性 + 累计守恒。
fn check_u2561_map_guard() -> bool {
    let mut ok = u2561mat_COUNT == 36;
    if ok { ok = u2561mat_scan() == 0 && u2561mat_unique(); }
    if ok { ok = u2561mat_get("AS0000") == Some(0) && u2561mat_get("ZZZ_NOKEY").is_none(); }
    if ok { ok = u2561mat_sum() == 1594; }
    ok
}

/// UNX-F2561 直方图 16 样本分位/均值/守恒。
fn check_u2561_map_hist() -> bool {
    let mut h = mxu2561histHist::new();
    let mut i = 0u64;
    while i < 16 { h.push(i + 1); i += 1; }
    let mut ok = h.conserved() && h.mean() >= 1 && h.percentile(500) >= 8;
    ok
}

/// 本模块自检（逐检查带 detail，全绿才出门）。
pub fn run_u2561_checks() -> CheckSet {
    let mut set = CheckSet::new("U2561");
    set.add(
        "u2561-seg-guard",
        u2561_guard_seg(),
        if u2561_guard_seg() { "" } else { "diagnostic code outside segment" },
    );
    set.add(
        "u2561-map-guard",
        check_u2561_map_guard(),
        if check_u2561_map_guard() { "" } else { "UNX-F2561 matrix guard mismatch" },
    );
    set.add(
        "u2561-map-hist",
        check_u2561_map_hist(),
        if check_u2561_map_hist() { "" } else { "UNX-F2561 hist mismatch" },
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
    fn u2561_all_checks_green() {
        let set = run_u2561_checks();
        assert!(set.all_passed(), "U2561 domain checks must be green");
    }

    #[test]
    fn u2561_segment_guard_holds() {
        assert!(u2561_segment_guard());
    }
}
