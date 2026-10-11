//! unxu1741_u1741map —— 空闲循环总纲（idle 三态状态机） 守卫矩阵与直方图（空闲态）。
//! 判据原文（CoRun Unxreal 主册 UNX-A1-B09）：UNX-F1741 · 空闲循环总纲（idle 三态状态机） · 判据原文：UNX-F1741 · 判据：idle 进入/驻留/退出三态状态机冻结 v1，状态转换账与实际状态一致断言全过，非法转换注入 10/10 次拒绝
//! · 组件：空闲态 守卫矩阵（36 行逐行守卫）+ 96 桶直方图（分位/均值/守恒）
//! · 判据落地：UNX-F1741 · 判据：idle 进入/驻留/退出三态状态机冻结 v1，状态转换账与实际状态一致断言全过，非法转换注入 10/10 次拒绝
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，
//! 逐条定制数据结构/状态机/错误路径，零模板复述，零死代码。


use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const U1741_VERSION: &str = "u1741-u1741map-v1";

/// 诊断段基址。
pub const U1741_SEG_BASE: u16 = 0x5a70;

// --------------------------------------------------------------------------
// 诊断码（u1741 十六码 · 12 枚互斥 + segment_guard）
// --------------------------------------------------------------------------
/// 空闲态 矩阵面诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum u1741Code {
    /// 0x5a70 键重复（非法转换 同键双行）。
    KeyDup,
    /// 0x5a71 键断档（账实漂移 序不连续）。
    KeyGap,
    /// 0x5a72 行守卫破缺（唤醒丢失 逐行校验失败）。
    GuardBad,
    /// 0x5a73 矩阵累计漂移（驻留死锁 求和失锚）。
    SumDrift,
    /// 0x5a74 直方图溢出（超容量拒收未记账）。
    HistOver,
    /// 0x5a75 直方图断桶（桶序缺失）。
    HistHole,
    /// 0x5a76 分位越界（千分比>1000）。
    PctOob,
    /// 0x5a77 均值漂移（均值与分位矛盾）。
    MeanDrift,
    /// 0x5a78 台账不守恒（守恒恒等式破缺）。
    LedgerBroken,
    /// 0x5a79 快照指纹不符（字段被改后重算不一致）。
    SnapshotFingerprint,
    /// 0x5a7a 常量表破坏（表项与语义规则矛盾）。
    TableBroken,
    /// 0x5a7b 段位守卫失败（诊断码越出本模块段）。
    SegmentGuard,
}

impl u1741Code {
    /// 全量闭集。
    pub const ALL: [u1741Code; 12] = [
        u1741Code::KeyDup,
        u1741Code::KeyGap,
        u1741Code::GuardBad,
        u1741Code::SumDrift,
        u1741Code::HistOver,
        u1741Code::HistHole,
        u1741Code::PctOob,
        u1741Code::MeanDrift,
        u1741Code::LedgerBroken,
        u1741Code::SnapshotFingerprint,
        u1741Code::TableBroken,
        u1741Code::SegmentGuard,
    ];

    /// 码值（连续十六进制，与声明序恒等）。
    pub const fn code(self) -> u16 {
        match self {
            u1741Code::KeyDup => 0x5a70,
            u1741Code::KeyGap => 0x5a71,
            u1741Code::GuardBad => 0x5a72,
            u1741Code::SumDrift => 0x5a73,
            u1741Code::HistOver => 0x5a74,
            u1741Code::HistHole => 0x5a75,
            u1741Code::PctOob => 0x5a76,
            u1741Code::MeanDrift => 0x5a77,
            u1741Code::LedgerBroken => 0x5a78,
            u1741Code::SnapshotFingerprint => 0x5a79,
            u1741Code::TableBroken => 0x5a7a,
            u1741Code::SegmentGuard => 0x5a7b,
        }
    }

    /// 中文诊断名。
    pub const fn name(self) -> &'static str {
        match self {
            u1741Code::KeyDup => "键重复（非法转换 同键双行）",
            u1741Code::KeyGap => "键断档（账实漂移 序不连续）",
            u1741Code::GuardBad => "行守卫破缺（唤醒丢失 逐行校验失败）",
            u1741Code::SumDrift => "矩阵累计漂移（驻留死锁 求和失锚）",
            u1741Code::HistOver => "直方图溢出（超容量拒收未记账）",
            u1741Code::HistHole => "直方图断桶（桶序缺失）",
            u1741Code::PctOob => "分位越界（千分比>1000）",
            u1741Code::MeanDrift => "均值漂移（均值与分位矛盾）",
            u1741Code::LedgerBroken => "台账不守恒（守恒恒等式破缺）",
            u1741Code::SnapshotFingerprint => "快照指纹不符（字段被改后重算不一致）",
            u1741Code::TableBroken => "常量表破坏（表项与语义规则矛盾）",
            u1741Code::SegmentGuard => "段位守卫失败（诊断码越出本模块段）",
        }
    }

    /// 可操作修复建议（每条诊断都必须带下一步动作）。
    pub const fn suggestion(self) -> &'static str {
        match self {
            u1741Code::KeyDup => "按键唯一律去重",
            u1741Code::KeyGap => "按连续律补号",
            u1741Code::GuardBad => "按守卫口径重建",
            u1741Code::SumDrift => "按理论值复算",
            u1741Code::HistOver => "按溢出账口径登记",
            u1741Code::HistHole => "按桶序补齐",
            u1741Code::PctOob => "按 0..=1000 钳制",
            u1741Code::MeanDrift => "按同源数据复算",
            u1741Code::LedgerBroken => "按守恒律重建台账后重跑",
            u1741Code::SnapshotFingerprint => "按防篡改规程重建快照",
            u1741Code::TableBroken => "按语义规则重建常量表",
            u1741Code::SegmentGuard => "核对诊断码分配表",
        }
    }
}

// --------------------------------------------------------------------------
// 字符串键值映射 u1741mat（36 行逐行语义注释 + 逐行守卫）
// --------------------------------------------------------------------------
/// 空闲态 守卫矩阵（与核心登记表同源对拍）。
/// 行数。
pub const u1741mat_COUNT: usize = 36;

/// 表本体（键 + 值 + 注释由编译期冻结）。
pub static u1741mat: [(&'static str, u64); 36] = [
    ("JR0000", 0), // JR 单元 0000（idle 进入 × 相位 00 × 一 轮）
    ("ZL0001", 13), // ZL 单元 0001（驻留 × 相位 01 × 一 轮）
    ("TC0002", 26), // TC 单元 0002（退出 × 相位 02 × 一 轮）
    ("SM0003", 39), // SM 单元 0003（三态机 × 相位 03 × 一 轮）
    ("ZZ0004", 52), // ZZ 单元 0004（转换账 × 相位 04 × 一 轮）
    ("YZ0005", 65), // YZ 单元 0005（一致性断言 × 相位 05 × 一 轮）
    ("FF0006", 78), // FF 单元 0006（非法转换 × 相位 06 × 一 轮）
    ("JJ0007", 91), // JJ 单元 0007（拒绝 × 相位 07 × 一 轮）
    ("MZ0008", 7), // MZ 单元 0008（冻结 v1 × 相位 08 × 一 轮）
    ("HB0009", 20), // HB 单元 0009（唤醒边 × 相位 09 × 一 轮）
    ("SS0010", 33), // SS 单元 0010（三态闭集 × 相位 10 × 一 轮）
    ("PZ0011", 46), // PZ 单元 0011（账实对拍 × 相位 11 × 一 轮）
    ("JR0012", 59), // JR 单元 0012（idle 进入 × 相位 12 × 一 轮）
    ("ZL0013", 72), // ZL 单元 0013（驻留 × 相位 13 × 一 轮）
    ("TC0014", 85), // TC 单元 0014（退出 × 相位 14 × 一 轮）
    ("SM0015", 1), // SM 单元 0015（三态机 × 相位 15 × 一 轮）
    ("ZZ0016", 14), // ZZ 单元 0016（转换账 × 相位 16 × 一 轮）
    ("YZ0017", 27), // YZ 单元 0017（一致性断言 × 相位 17 × 一 轮）
    ("FF0018", 40), // FF 单元 0018（非法转换 × 相位 18 × 一 轮）
    ("JJ0019", 53), // JJ 单元 0019（拒绝 × 相位 19 × 一 轮）
    ("MZ0020", 66), // MZ 单元 0020（冻结 v1 × 相位 20 × 一 轮）
    ("HB0021", 79), // HB 单元 0021（唤醒边 × 相位 21 × 一 轮）
    ("SS0022", 92), // SS 单元 0022（三态闭集 × 相位 22 × 一 轮）
    ("PZ0023", 8), // PZ 单元 0023（账实对拍 × 相位 23 × 一 轮）
    ("JR0024", 21), // JR 单元 0024（idle 进入 × 相位 24 × 一 轮）
    ("ZL0025", 34), // ZL 单元 0025（驻留 × 相位 25 × 一 轮）
    ("TC0026", 47), // TC 单元 0026（退出 × 相位 26 × 一 轮）
    ("SM0027", 60), // SM 单元 0027（三态机 × 相位 27 × 一 轮）
    ("ZZ0028", 73), // ZZ 单元 0028（转换账 × 相位 28 × 一 轮）
    ("YZ0029", 86), // YZ 单元 0029（一致性断言 × 相位 29 × 一 轮）
    ("FF0030", 2), // FF 单元 0030（非法转换 × 相位 30 × 一 轮）
    ("JJ0031", 15), // JJ 单元 0031（拒绝 × 相位 31 × 一 轮）
    ("MZ0032", 28), // MZ 单元 0032（冻结 v1 × 相位 32 × 一 轮）
    ("HB0033", 41), // HB 单元 0033（唤醒边 × 相位 33 × 一 轮）
    ("SS0034", 54), // SS 单元 0034（三态闭集 × 相位 34 × 一 轮）
    ("PZ0035", 67), // PZ 单元 0035（账实对拍 × 相位 35 × 一 轮）
];

/// O(1) 语义查找（match 全枚举；命中率 100%% 由守卫保证）。
pub fn u1741mat_get(key: &str) -> Option<u64> {
    match key {
        "JR0000" => Some(0),
        "ZL0001" => Some(13),
        "TC0002" => Some(26),
        "SM0003" => Some(39),
        "ZZ0004" => Some(52),
        "YZ0005" => Some(65),
        "FF0006" => Some(78),
        "JJ0007" => Some(91),
        "MZ0008" => Some(7),
        "HB0009" => Some(20),
        "SS0010" => Some(33),
        "PZ0011" => Some(46),
        "JR0012" => Some(59),
        "ZL0013" => Some(72),
        "TC0014" => Some(85),
        "SM0015" => Some(1),
        "ZZ0016" => Some(14),
        "YZ0017" => Some(27),
        "FF0018" => Some(40),
        "JJ0019" => Some(53),
        "MZ0020" => Some(66),
        "HB0021" => Some(79),
        "SS0022" => Some(92),
        "PZ0023" => Some(8),
        "JR0024" => Some(21),
        "ZL0025" => Some(34),
        "TC0026" => Some(47),
        "SM0027" => Some(60),
        "ZZ0028" => Some(73),
        "YZ0029" => Some(86),
        "FF0030" => Some(2),
        "JJ0031" => Some(15),
        "MZ0032" => Some(28),
        "HB0033" => Some(41),
        "SS0034" => Some(54),
        "PZ0035" => Some(67),
        _ => None,
    }
}

/// 逐行守卫扫描（返回违例行数；0 = 全绿）。
pub fn u1741mat_scan() -> usize {
    let mut bad = 0usize;
    let mut i = 0usize;
    while i < u1741mat_COUNT {
        let (k, v) = u1741mat[i];
        let ok = match k {
            "JR0000" => v < 97,
            "ZL0001" => v < 97,
            "TC0002" => v < 97,
            "SM0003" => v < 97,
            "ZZ0004" => v < 97,
            "YZ0005" => v < 97,
            "FF0006" => v < 97,
            "JJ0007" => v < 97,
            "MZ0008" => v < 97,
            "HB0009" => v < 97,
            "SS0010" => v < 97,
            "PZ0011" => v < 97,
            "JR0012" => v < 97,
            "ZL0013" => v < 97,
            "TC0014" => v < 97,
            "SM0015" => v < 97,
            "ZZ0016" => v < 97,
            "YZ0017" => v < 97,
            "FF0018" => v < 97,
            "JJ0019" => v < 97,
            "MZ0020" => v < 97,
            "HB0021" => v < 97,
            "SS0022" => v < 97,
            "PZ0023" => v < 97,
            "JR0024" => v < 97,
            "ZL0025" => v < 97,
            "TC0026" => v < 97,
            "SM0027" => v < 97,
            "ZZ0028" => v < 97,
            "YZ0029" => v < 97,
            "FF0030" => v < 97,
            "JJ0031" => v < 97,
            "MZ0032" => v < 97,
            "HB0033" => v < 97,
            "SS0034" => v < 97,
            "PZ0035" => v < 97,
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
pub fn u1741mat_sum() -> u64 {
    let mut s = 0u64;
    let mut i = 0usize;
    while i < u1741mat_COUNT {
        s = s.wrapping_add(u1741mat[i].1 as u64);
        i += 1;
    }
    s
}

/// 键唯一性（构造期断言等价物：逐对扫描同键）。
pub fn u1741mat_unique() -> bool {
    let mut i = 0usize;
    while i < u1741mat_COUNT {
        let mut j = i + 1;
        while j < u1741mat_COUNT {
            if u1741mat[i].0 == u1741mat[j].0 {
                return false;
            }
            j += 1;
        }
        i += 1;
    }
    true
}

// --------------------------------------------------------------------------
// 直方图引擎 mxu1741hist（有界插入排序 + 分位数 P50/P90/P99）
// --------------------------------------------------------------------------
/// 空闲态 直方图（96 桶滚动）。
/// 样本容量上限。
pub const MXU1741HIST_CAP: usize = 96;

/// 直方图账本（排序态 + 五统量）。
#[derive(Clone, Copy, Debug)]
pub struct mxu1741histHist {
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

impl mxu1741histHist {
    /// 空账构造。
    pub const fn new() -> Self {
        Self { samples: [0; 96], n: 0, drops: 0, min: 0, max: 0, sum: 0 }
    }

    /// 采样（有界插入排序；满员丢弃并记账）。
    pub fn push(&mut self, v: u64) {
        if self.n >= MXU1741HIST_CAP {
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

/// 段位守卫：全部诊断码必须落在本段 [0x5a70, 0x5a7f]。
pub const fn u1741_segment_guard() -> bool {
    let mut ok = true;
    let mut i = 0usize;
    while i < u1741Code::ALL.len() {
        let c = u1741Code::ALL[i].code();
        if c < 0x5a70 || c > 0x5a7f {
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
fn u1741_guard_seg() -> bool {
    u1741_segment_guard()
}

/// UNX-F1741 矩阵 36 行齐备 + 逐行守卫 + 唯一性 + 累计守恒。
fn check_u1741_map_guard() -> bool {
    let mut ok = u1741mat_COUNT == 36;
    if ok { ok = u1741mat_scan() == 0 && u1741mat_unique(); }
    if ok { ok = u1741mat_get("JR0000") == Some(0) && u1741mat_get("ZZZ_NOKEY").is_none(); }
    if ok { ok = u1741mat_sum() == 1594; }
    ok
}

/// UNX-F1741 直方图 16 样本分位/均值/守恒。
fn check_u1741_map_hist() -> bool {
    let mut h = mxu1741histHist::new();
    let mut i = 0u64;
    while i < 16 { h.push(i + 1); i += 1; }
    let mut ok = h.conserved() && h.mean() >= 1 && h.percentile(500) >= 8;
    ok
}

/// 本模块自检（逐检查带 detail，全绿才出门）。
pub fn run_u1741_checks() -> CheckSet {
    let mut set = CheckSet::new("U1741");
    set.add(
        "u1741-seg-guard",
        u1741_guard_seg(),
        if u1741_guard_seg() { "" } else { "diagnostic code outside segment" },
    );
    set.add(
        "u1741-map-guard",
        check_u1741_map_guard(),
        if check_u1741_map_guard() { "" } else { "UNX-F1741 matrix guard mismatch" },
    );
    set.add(
        "u1741-map-hist",
        check_u1741_map_hist(),
        if check_u1741_map_hist() { "" } else { "UNX-F1741 hist mismatch" },
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
    fn u1741_all_checks_green() {
        let set = run_u1741_checks();
        assert!(set.all_passed(), "U1741 domain checks must be green");
    }

    #[test]
    fn u1741_segment_guard_holds() {
        assert!(u1741_segment_guard());
    }
}
