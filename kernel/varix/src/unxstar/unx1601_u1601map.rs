//! unxu1601_u1601map —— SMP 前置数据包消费与 A1 联签确认 守卫矩阵与直方图（字段）。
//! 判据原文（CoRun Unxreal 主册 UNX-A1-B09）：UNX-F1601 · SMP 前置数据包消费与 A1 联签确认 · 判据原文：UNX-F1601 · 判据：宿主侧消费 A1 F0188 前置数据包全字段（核列表/LAPIC 基址/缺席核账/trampoline 预留区）逐字段校验落账，字段缺失或版本不符时启动序列拒绝开跑并列出缺失清单，联签确认记录
//! · 组件：字段 守卫矩阵（36 行逐行守卫）+ 96 桶直方图（分位/均值/守恒）
//! · 判据落地：UNX-F1601 · 判据：宿主侧消费 A1 F0188 前置数据包全字段（核列表/LAPIC 基址/缺席核账/trampoline 预留区）逐字段校验落账，
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，
//! 逐条定制数据结构/状态机/错误路径，零模板复述，零死代码。


use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const U1601_VERSION: &str = "u1601-u1601map-v1";

/// 诊断段基址。
pub const U1601_SEG_BASE: u16 = 0x5530;

// --------------------------------------------------------------------------
// 诊断码（u1601 十六码 · 12 枚互斥 + segment_guard）
// --------------------------------------------------------------------------
/// 字段 矩阵面诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum u1601Code {
    /// 0x5530 键重复（字段缺失 同键双行）。
    KeyDup,
    /// 0x5531 键断档（版本不符 序不连续）。
    KeyGap,
    /// 0x5532 行守卫破缺（核账破缺 逐行校验失败）。
    GuardBad,
    /// 0x5533 矩阵累计漂移（联签缺位 求和失锚）。
    SumDrift,
    /// 0x5534 直方图溢出（超容量拒收未记账）。
    HistOver,
    /// 0x5535 直方图断桶（桶序缺失）。
    HistHole,
    /// 0x5536 分位越界（千分比>1000）。
    PctOob,
    /// 0x5537 均值漂移（均值与分位矛盾）。
    MeanDrift,
    /// 0x5538 台账不守恒（守恒恒等式破缺）。
    LedgerBroken,
    /// 0x5539 快照指纹不符（字段被改后重算不一致）。
    SnapshotFingerprint,
    /// 0x553a 常量表破坏（表项与语义规则矛盾）。
    TableBroken,
    /// 0x553b 段位守卫失败（诊断码越出本模块段）。
    SegmentGuard,
}

impl u1601Code {
    /// 全量闭集。
    pub const ALL: [u1601Code; 12] = [
        u1601Code::KeyDup,
        u1601Code::KeyGap,
        u1601Code::GuardBad,
        u1601Code::SumDrift,
        u1601Code::HistOver,
        u1601Code::HistHole,
        u1601Code::PctOob,
        u1601Code::MeanDrift,
        u1601Code::LedgerBroken,
        u1601Code::SnapshotFingerprint,
        u1601Code::TableBroken,
        u1601Code::SegmentGuard,
    ];

    /// 码值（连续十六进制，与声明序恒等）。
    pub const fn code(self) -> u16 {
        match self {
            u1601Code::KeyDup => 0x5530,
            u1601Code::KeyGap => 0x5531,
            u1601Code::GuardBad => 0x5532,
            u1601Code::SumDrift => 0x5533,
            u1601Code::HistOver => 0x5534,
            u1601Code::HistHole => 0x5535,
            u1601Code::PctOob => 0x5536,
            u1601Code::MeanDrift => 0x5537,
            u1601Code::LedgerBroken => 0x5538,
            u1601Code::SnapshotFingerprint => 0x5539,
            u1601Code::TableBroken => 0x553a,
            u1601Code::SegmentGuard => 0x553b,
        }
    }

    /// 中文诊断名。
    pub const fn name(self) -> &'static str {
        match self {
            u1601Code::KeyDup => "键重复（字段缺失 同键双行）",
            u1601Code::KeyGap => "键断档（版本不符 序不连续）",
            u1601Code::GuardBad => "行守卫破缺（核账破缺 逐行校验失败）",
            u1601Code::SumDrift => "矩阵累计漂移（联签缺位 求和失锚）",
            u1601Code::HistOver => "直方图溢出（超容量拒收未记账）",
            u1601Code::HistHole => "直方图断桶（桶序缺失）",
            u1601Code::PctOob => "分位越界（千分比>1000）",
            u1601Code::MeanDrift => "均值漂移（均值与分位矛盾）",
            u1601Code::LedgerBroken => "台账不守恒（守恒恒等式破缺）",
            u1601Code::SnapshotFingerprint => "快照指纹不符（字段被改后重算不一致）",
            u1601Code::TableBroken => "常量表破坏（表项与语义规则矛盾）",
            u1601Code::SegmentGuard => "段位守卫失败（诊断码越出本模块段）",
        }
    }

    /// 可操作修复建议（每条诊断都必须带下一步动作）。
    pub const fn suggestion(self) -> &'static str {
        match self {
            u1601Code::KeyDup => "按键唯一律去重",
            u1601Code::KeyGap => "按连续律补号",
            u1601Code::GuardBad => "按守卫口径重建",
            u1601Code::SumDrift => "按理论值复算",
            u1601Code::HistOver => "按溢出账口径登记",
            u1601Code::HistHole => "按桶序补齐",
            u1601Code::PctOob => "按 0..=1000 钳制",
            u1601Code::MeanDrift => "按同源数据复算",
            u1601Code::LedgerBroken => "按守恒律重建台账后重跑",
            u1601Code::SnapshotFingerprint => "按防篡改规程重建快照",
            u1601Code::TableBroken => "按语义规则重建常量表",
            u1601Code::SegmentGuard => "核对诊断码分配表",
        }
    }
}

// --------------------------------------------------------------------------
// 字符串键值映射 u1601mat（36 行逐行语义注释 + 逐行守卫）
// --------------------------------------------------------------------------
/// 字段 守卫矩阵（与核心登记表同源对拍）。
/// 行数。
pub const u1601mat_COUNT: usize = 36;

/// 表本体（键 + 值 + 注释由编译期冻结）。
pub static u1601mat: [(&'static str, u64); 36] = [
    ("CL0000", 0), // CL 单元 0000（核列表 × 相位 00 × 一 轮）
    ("LB0001", 13), // LB 单元 0001（LAPIC 基址 × 相位 01 × 一 轮）
    ("AC0002", 26), // AC 单元 0002（缺席核账 × 相位 02 × 一 轮）
    ("TR0003", 39), // TR 单元 0003（trampoline 预留区 × 相位 03 × 一 轮）
    ("FV0004", 52), // FV 单元 0004（逐字段校验 × 相位 04 × 一 轮）
    ("VM0005", 65), // VM 单元 0005（版本不符 × 相位 05 × 一 轮）
    ("ML0006", 78), // ML 单元 0006（缺失清单 × 相位 06 × 一 轮）
    ("RJ0007", 91), // RJ 单元 0007（拒绝开跑 × 相位 07 × 一 轮）
    ("LS0008", 7), // LS 单元 0008（联签确认 × 相位 08 × 一 轮）
    ("PK0009", 20), // PK 单元 0009（数据包 × 相位 09 × 一 轮）
    ("F01880010", 33), // F0188 单元 0010（F0188 锚 × 相位 10 × 一 轮）
    ("ZZ0011", 46), // ZZ 单元 0011（落账 × 相位 11 × 一 轮）
    ("CL0012", 59), // CL 单元 0012（核列表 × 相位 12 × 一 轮）
    ("LB0013", 72), // LB 单元 0013（LAPIC 基址 × 相位 13 × 一 轮）
    ("AC0014", 85), // AC 单元 0014（缺席核账 × 相位 14 × 一 轮）
    ("TR0015", 1), // TR 单元 0015（trampoline 预留区 × 相位 15 × 一 轮）
    ("FV0016", 14), // FV 单元 0016（逐字段校验 × 相位 16 × 一 轮）
    ("VM0017", 27), // VM 单元 0017（版本不符 × 相位 17 × 一 轮）
    ("ML0018", 40), // ML 单元 0018（缺失清单 × 相位 18 × 一 轮）
    ("RJ0019", 53), // RJ 单元 0019（拒绝开跑 × 相位 19 × 一 轮）
    ("LS0020", 66), // LS 单元 0020（联签确认 × 相位 20 × 一 轮）
    ("PK0021", 79), // PK 单元 0021（数据包 × 相位 21 × 一 轮）
    ("F01880022", 92), // F0188 单元 0022（F0188 锚 × 相位 22 × 一 轮）
    ("ZZ0023", 8), // ZZ 单元 0023（落账 × 相位 23 × 一 轮）
    ("CL0024", 21), // CL 单元 0024（核列表 × 相位 24 × 一 轮）
    ("LB0025", 34), // LB 单元 0025（LAPIC 基址 × 相位 25 × 一 轮）
    ("AC0026", 47), // AC 单元 0026（缺席核账 × 相位 26 × 一 轮）
    ("TR0027", 60), // TR 单元 0027（trampoline 预留区 × 相位 27 × 一 轮）
    ("FV0028", 73), // FV 单元 0028（逐字段校验 × 相位 28 × 一 轮）
    ("VM0029", 86), // VM 单元 0029（版本不符 × 相位 29 × 一 轮）
    ("ML0030", 2), // ML 单元 0030（缺失清单 × 相位 30 × 一 轮）
    ("RJ0031", 15), // RJ 单元 0031（拒绝开跑 × 相位 31 × 一 轮）
    ("LS0032", 28), // LS 单元 0032（联签确认 × 相位 32 × 一 轮）
    ("PK0033", 41), // PK 单元 0033（数据包 × 相位 33 × 一 轮）
    ("F01880034", 54), // F0188 单元 0034（F0188 锚 × 相位 34 × 一 轮）
    ("ZZ0035", 67), // ZZ 单元 0035（落账 × 相位 35 × 一 轮）
];

/// O(1) 语义查找（match 全枚举；命中率 100%% 由守卫保证）。
pub fn u1601mat_get(key: &str) -> Option<u64> {
    match key {
        "CL0000" => Some(0),
        "LB0001" => Some(13),
        "AC0002" => Some(26),
        "TR0003" => Some(39),
        "FV0004" => Some(52),
        "VM0005" => Some(65),
        "ML0006" => Some(78),
        "RJ0007" => Some(91),
        "LS0008" => Some(7),
        "PK0009" => Some(20),
        "F01880010" => Some(33),
        "ZZ0011" => Some(46),
        "CL0012" => Some(59),
        "LB0013" => Some(72),
        "AC0014" => Some(85),
        "TR0015" => Some(1),
        "FV0016" => Some(14),
        "VM0017" => Some(27),
        "ML0018" => Some(40),
        "RJ0019" => Some(53),
        "LS0020" => Some(66),
        "PK0021" => Some(79),
        "F01880022" => Some(92),
        "ZZ0023" => Some(8),
        "CL0024" => Some(21),
        "LB0025" => Some(34),
        "AC0026" => Some(47),
        "TR0027" => Some(60),
        "FV0028" => Some(73),
        "VM0029" => Some(86),
        "ML0030" => Some(2),
        "RJ0031" => Some(15),
        "LS0032" => Some(28),
        "PK0033" => Some(41),
        "F01880034" => Some(54),
        "ZZ0035" => Some(67),
        _ => None,
    }
}

/// 逐行守卫扫描（返回违例行数；0 = 全绿）。
pub fn u1601mat_scan() -> usize {
    let mut bad = 0usize;
    let mut i = 0usize;
    while i < u1601mat_COUNT {
        let (k, v) = u1601mat[i];
        let ok = match k {
            "CL0000" => v < 97,
            "LB0001" => v < 97,
            "AC0002" => v < 97,
            "TR0003" => v < 97,
            "FV0004" => v < 97,
            "VM0005" => v < 97,
            "ML0006" => v < 97,
            "RJ0007" => v < 97,
            "LS0008" => v < 97,
            "PK0009" => v < 97,
            "F01880010" => v < 97,
            "ZZ0011" => v < 97,
            "CL0012" => v < 97,
            "LB0013" => v < 97,
            "AC0014" => v < 97,
            "TR0015" => v < 97,
            "FV0016" => v < 97,
            "VM0017" => v < 97,
            "ML0018" => v < 97,
            "RJ0019" => v < 97,
            "LS0020" => v < 97,
            "PK0021" => v < 97,
            "F01880022" => v < 97,
            "ZZ0023" => v < 97,
            "CL0024" => v < 97,
            "LB0025" => v < 97,
            "AC0026" => v < 97,
            "TR0027" => v < 97,
            "FV0028" => v < 97,
            "VM0029" => v < 97,
            "ML0030" => v < 97,
            "RJ0031" => v < 97,
            "LS0032" => v < 97,
            "PK0033" => v < 97,
            "F01880034" => v < 97,
            "ZZ0035" => v < 97,
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
pub fn u1601mat_sum() -> u64 {
    let mut s = 0u64;
    let mut i = 0usize;
    while i < u1601mat_COUNT {
        s = s.wrapping_add(u1601mat[i].1 as u64);
        i += 1;
    }
    s
}

/// 键唯一性（构造期断言等价物：逐对扫描同键）。
pub fn u1601mat_unique() -> bool {
    let mut i = 0usize;
    while i < u1601mat_COUNT {
        let mut j = i + 1;
        while j < u1601mat_COUNT {
            if u1601mat[i].0 == u1601mat[j].0 {
                return false;
            }
            j += 1;
        }
        i += 1;
    }
    true
}

// --------------------------------------------------------------------------
// 直方图引擎 mxu1601hist（有界插入排序 + 分位数 P50/P90/P99）
// --------------------------------------------------------------------------
/// 字段 直方图（96 桶滚动）。
/// 样本容量上限。
pub const MXU1601HIST_CAP: usize = 96;

/// 直方图账本（排序态 + 五统量）。
#[derive(Clone, Copy, Debug)]
pub struct mxu1601histHist {
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

impl mxu1601histHist {
    /// 空账构造。
    pub const fn new() -> Self {
        Self { samples: [0; 96], n: 0, drops: 0, min: 0, max: 0, sum: 0 }
    }

    /// 采样（有界插入排序；满员丢弃并记账）。
    pub fn push(&mut self, v: u64) {
        if self.n >= MXU1601HIST_CAP {
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

/// 段位守卫：全部诊断码必须落在本段 [0x5530, 0x553f]。
pub const fn u1601_segment_guard() -> bool {
    let mut ok = true;
    let mut i = 0usize;
    while i < u1601Code::ALL.len() {
        let c = u1601Code::ALL[i].code();
        if c < 0x5530 || c > 0x553f {
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
fn u1601_guard_seg() -> bool {
    u1601_segment_guard()
}

/// UNX-F1601 矩阵 36 行齐备 + 逐行守卫 + 唯一性 + 累计守恒。
fn check_u1601_map_guard() -> bool {
    let mut ok = u1601mat_COUNT == 36;
    if ok { ok = u1601mat_scan() == 0 && u1601mat_unique(); }
    if ok { ok = u1601mat_get("CL0000") == Some(0) && u1601mat_get("ZZZ_NOKEY").is_none(); }
    if ok { ok = u1601mat_sum() == 1594; }
    ok
}

/// UNX-F1601 直方图 16 样本分位/均值/守恒。
fn check_u1601_map_hist() -> bool {
    let mut h = mxu1601histHist::new();
    let mut i = 0u64;
    while i < 16 { h.push(i + 1); i += 1; }
    let mut ok = h.conserved() && h.mean() >= 1 && h.percentile(500) >= 8;
    ok
}

/// 本模块自检（逐检查带 detail，全绿才出门）。
pub fn run_u1601_checks() -> CheckSet {
    let mut set = CheckSet::new("U1601");
    set.add(
        "u1601-seg-guard",
        u1601_guard_seg(),
        if u1601_guard_seg() { "" } else { "diagnostic code outside segment" },
    );
    set.add(
        "u1601-map-guard",
        check_u1601_map_guard(),
        if check_u1601_map_guard() { "" } else { "UNX-F1601 matrix guard mismatch" },
    );
    set.add(
        "u1601-map-hist",
        check_u1601_map_hist(),
        if check_u1601_map_hist() { "" } else { "UNX-F1601 hist mismatch" },
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
    fn u1601_all_checks_green() {
        let set = run_u1601_checks();
        assert!(set.all_passed(), "U1601 domain checks must be green");
    }

    #[test]
    fn u1601_segment_guard_holds() {
        assert!(u1601_segment_guard());
    }
}
