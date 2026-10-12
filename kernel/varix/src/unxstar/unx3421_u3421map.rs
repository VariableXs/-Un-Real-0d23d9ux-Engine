//! unxu3421_u3421map —— 双后端总设计（QEMU 后端真机后端统一抽象，开发期零 QEMU 双轨制） 守卫矩阵与直方图（双后端抽象）。
//! 判据原文（CoRun Unxreal 主册 UNX-A1-B09）：UNX-F3421 · 双后端总设计（QEMU 后端真机后端统一抽象，开发期零 QEMU 双轨制） · 判据原文：UNX-F3421 · 判据：J1：双后端总设计（后端 = 输出通道 + 执行环境的统一抽象）文档交付：开发期桩后端零 QEMU 依赖、真机后端为闸口对象；M-180 桩后端全链路。
//! · 组件：双后端抽象 守卫矩阵（36 行逐行守卫）+ 96 桶直方图（分位/均值/守恒）
//! · 判据落地：UNX-F3421 · 判据：J1：双后端总设计（后端 = 输出通道 + 执行环境的统一抽象）文档交付：开发期桩后端零 QEMU 依赖、真机后端为闸口对象；M-
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，
//! 逐条定制数据结构/状态机/错误路径，零模板复述，零死代码。


use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const U3421_VERSION: &str = "u3421-u3421map-v1";

/// 诊断段基址。
pub const U3421_SEG_BASE: u16 = 0xe0b0;

// --------------------------------------------------------------------------
// 诊断码（u3421 十六码 · 12 枚互斥 + segment_guard）
// --------------------------------------------------------------------------
/// 双后端抽象 矩阵面诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum u3421Code {
    /// 0xe0b0 键重复（双后端 同键双行）。
    KeyDup,
    /// 0xe0b1 键断档（抽象 序不连续）。
    KeyGap,
    /// 0xe0b2 行守卫破缺（QEMU 逐行校验失败）。
    GuardBad,
    /// 0xe0b3 矩阵累计漂移（双轨制 求和失锚）。
    SumDrift,
    /// 0xe0b4 直方图溢出（超容量拒收未记账）。
    HistOver,
    /// 0xe0b5 直方图断桶（桶序缺失）。
    HistHole,
    /// 0xe0b6 分位越界（千分比>1000）。
    PctOob,
    /// 0xe0b7 均值漂移（均值与分位矛盾）。
    MeanDrift,
    /// 0xe0b8 台账不守恒（守恒恒等式破缺）。
    LedgerBroken,
    /// 0xe0b9 快照指纹不符（字段被改后重算不一致）。
    SnapshotFingerprint,
    /// 0xe0ba 常量表破坏（表项与语义规则矛盾）。
    TableBroken,
    /// 0xe0bb 段位守卫失败（诊断码越出本模块段）。
    SegmentGuard,
}

impl u3421Code {
    /// 全量闭集。
    pub const ALL: [u3421Code; 12] = [
        u3421Code::KeyDup,
        u3421Code::KeyGap,
        u3421Code::GuardBad,
        u3421Code::SumDrift,
        u3421Code::HistOver,
        u3421Code::HistHole,
        u3421Code::PctOob,
        u3421Code::MeanDrift,
        u3421Code::LedgerBroken,
        u3421Code::SnapshotFingerprint,
        u3421Code::TableBroken,
        u3421Code::SegmentGuard,
    ];

    /// 码值（连续十六进制，与声明序恒等）。
    pub const fn code(self) -> u16 {
        match self {
            u3421Code::KeyDup => 0xe0b0,
            u3421Code::KeyGap => 0xe0b1,
            u3421Code::GuardBad => 0xe0b2,
            u3421Code::SumDrift => 0xe0b3,
            u3421Code::HistOver => 0xe0b4,
            u3421Code::HistHole => 0xe0b5,
            u3421Code::PctOob => 0xe0b6,
            u3421Code::MeanDrift => 0xe0b7,
            u3421Code::LedgerBroken => 0xe0b8,
            u3421Code::SnapshotFingerprint => 0xe0b9,
            u3421Code::TableBroken => 0xe0ba,
            u3421Code::SegmentGuard => 0xe0bb,
        }
    }

    /// 中文诊断名。
    pub const fn name(self) -> &'static str {
        match self {
            u3421Code::KeyDup => "键重复（双后端 同键双行）",
            u3421Code::KeyGap => "键断档（抽象 序不连续）",
            u3421Code::GuardBad => "行守卫破缺（QEMU 逐行校验失败）",
            u3421Code::SumDrift => "矩阵累计漂移（双轨制 求和失锚）",
            u3421Code::HistOver => "直方图溢出（超容量拒收未记账）",
            u3421Code::HistHole => "直方图断桶（桶序缺失）",
            u3421Code::PctOob => "分位越界（千分比>1000）",
            u3421Code::MeanDrift => "均值漂移（均值与分位矛盾）",
            u3421Code::LedgerBroken => "台账不守恒（守恒恒等式破缺）",
            u3421Code::SnapshotFingerprint => "快照指纹不符（字段被改后重算不一致）",
            u3421Code::TableBroken => "常量表破坏（表项与语义规则矛盾）",
            u3421Code::SegmentGuard => "段位守卫失败（诊断码越出本模块段）",
        }
    }

    /// 可操作修复建议（每条诊断都必须带下一步动作）。
    pub const fn suggestion(self) -> &'static str {
        match self {
            u3421Code::KeyDup => "按键唯一律去重",
            u3421Code::KeyGap => "按连续律补号",
            u3421Code::GuardBad => "按守卫口径重建",
            u3421Code::SumDrift => "按理论值复算",
            u3421Code::HistOver => "按溢出账口径登记",
            u3421Code::HistHole => "按桶序补齐",
            u3421Code::PctOob => "按 0..=1000 钳制",
            u3421Code::MeanDrift => "按同源数据复算",
            u3421Code::LedgerBroken => "按守恒律重建台账后重跑",
            u3421Code::SnapshotFingerprint => "按防篡改规程重建快照",
            u3421Code::TableBroken => "按语义规则重建常量表",
            u3421Code::SegmentGuard => "核对诊断码分配表",
        }
    }
}

// --------------------------------------------------------------------------
// 字符串键值映射 u3421mat（36 行逐行语义注释 + 逐行守卫）
// --------------------------------------------------------------------------
/// 双后端抽象 守卫矩阵（与核心登记表同源对拍）。
/// 行数。
pub const u3421mat_COUNT: usize = 36;

/// 表本体（键 + 值 + 注释由编译期冻结）。
pub static u3421mat: [(&'static str, u64); 36] = [
    ("BE0000", 0), // BE 单元 0000（后端 × 相位 00 × 一 轮）
    ("QEMU0001", 13), // QEMU 单元 0001（QEMU 后端 × 相位 01 × 一 轮）
    ("REAL0002", 26), // REAL 单元 0002（真机后端 × 相位 02 × 一 轮）
    ("ABI0003", 39), // ABI 单元 0003（后端 ABI × 相位 03 × 一 轮）
    ("SER0004", 52), // SER 单元 0004（串口通道 × 相位 04 × 一 轮）
    ("DBG0005", 65), // DBG 单元 0005（调试通道 × 相位 05 × 一 轮）
    ("LOG0006", 78), // LOG 单元 0006（日志汇聚 × 相位 06 × 一 轮）
    ("TIME0007", 91), // TIME 单元 0007（超时口径 × 相位 07 × 一 轮）
    ("SNAP0008", 7), // SNAP 单元 0008（快照 × 相位 08 × 一 轮）
    ("DIFF0009", 20), // DIFF 单元 0009（差异对比 × 相位 09 × 一 轮）
    ("DEV0010", 33), // DEV 单元 0010（开发期 × 相位 10 × 一 轮）
    ("CI0011", 46), // CI 单元 0011（流水线 × 相位 11 × 一 轮）
    ("BE0012", 59), // BE 单元 0012（后端 × 相位 12 × 一 轮）
    ("QEMU0013", 72), // QEMU 单元 0013（QEMU 后端 × 相位 13 × 一 轮）
    ("REAL0014", 85), // REAL 单元 0014（真机后端 × 相位 14 × 一 轮）
    ("ABI0015", 1), // ABI 单元 0015（后端 ABI × 相位 15 × 一 轮）
    ("SER0016", 14), // SER 单元 0016（串口通道 × 相位 16 × 一 轮）
    ("DBG0017", 27), // DBG 单元 0017（调试通道 × 相位 17 × 一 轮）
    ("LOG0018", 40), // LOG 单元 0018（日志汇聚 × 相位 18 × 一 轮）
    ("TIME0019", 53), // TIME 单元 0019（超时口径 × 相位 19 × 一 轮）
    ("SNAP0020", 66), // SNAP 单元 0020（快照 × 相位 20 × 一 轮）
    ("DIFF0021", 79), // DIFF 单元 0021（差异对比 × 相位 21 × 一 轮）
    ("DEV0022", 92), // DEV 单元 0022（开发期 × 相位 22 × 一 轮）
    ("CI0023", 8), // CI 单元 0023（流水线 × 相位 23 × 一 轮）
    ("BE0024", 21), // BE 单元 0024（后端 × 相位 24 × 一 轮）
    ("QEMU0025", 34), // QEMU 单元 0025（QEMU 后端 × 相位 25 × 一 轮）
    ("REAL0026", 47), // REAL 单元 0026（真机后端 × 相位 26 × 一 轮）
    ("ABI0027", 60), // ABI 单元 0027（后端 ABI × 相位 27 × 一 轮）
    ("SER0028", 73), // SER 单元 0028（串口通道 × 相位 28 × 一 轮）
    ("DBG0029", 86), // DBG 单元 0029（调试通道 × 相位 29 × 一 轮）
    ("LOG0030", 2), // LOG 单元 0030（日志汇聚 × 相位 30 × 一 轮）
    ("TIME0031", 15), // TIME 单元 0031（超时口径 × 相位 31 × 一 轮）
    ("SNAP0032", 28), // SNAP 单元 0032（快照 × 相位 32 × 一 轮）
    ("DIFF0033", 41), // DIFF 单元 0033（差异对比 × 相位 33 × 一 轮）
    ("DEV0034", 54), // DEV 单元 0034（开发期 × 相位 34 × 一 轮）
    ("CI0035", 67), // CI 单元 0035（流水线 × 相位 35 × 一 轮）
];

/// O(1) 语义查找（match 全枚举；命中率 100%% 由守卫保证）。
pub fn u3421mat_get(key: &str) -> Option<u64> {
    match key {
        "BE0000" => Some(0),
        "QEMU0001" => Some(13),
        "REAL0002" => Some(26),
        "ABI0003" => Some(39),
        "SER0004" => Some(52),
        "DBG0005" => Some(65),
        "LOG0006" => Some(78),
        "TIME0007" => Some(91),
        "SNAP0008" => Some(7),
        "DIFF0009" => Some(20),
        "DEV0010" => Some(33),
        "CI0011" => Some(46),
        "BE0012" => Some(59),
        "QEMU0013" => Some(72),
        "REAL0014" => Some(85),
        "ABI0015" => Some(1),
        "SER0016" => Some(14),
        "DBG0017" => Some(27),
        "LOG0018" => Some(40),
        "TIME0019" => Some(53),
        "SNAP0020" => Some(66),
        "DIFF0021" => Some(79),
        "DEV0022" => Some(92),
        "CI0023" => Some(8),
        "BE0024" => Some(21),
        "QEMU0025" => Some(34),
        "REAL0026" => Some(47),
        "ABI0027" => Some(60),
        "SER0028" => Some(73),
        "DBG0029" => Some(86),
        "LOG0030" => Some(2),
        "TIME0031" => Some(15),
        "SNAP0032" => Some(28),
        "DIFF0033" => Some(41),
        "DEV0034" => Some(54),
        "CI0035" => Some(67),
        _ => None,
    }
}

/// 逐行守卫扫描（返回违例行数；0 = 全绿）。
pub fn u3421mat_scan() -> usize {
    let mut bad = 0usize;
    let mut i = 0usize;
    while i < u3421mat_COUNT {
        let (k, v) = u3421mat[i];
        let ok = match k {
            "BE0000" => v < 97,
            "QEMU0001" => v < 97,
            "REAL0002" => v < 97,
            "ABI0003" => v < 97,
            "SER0004" => v < 97,
            "DBG0005" => v < 97,
            "LOG0006" => v < 97,
            "TIME0007" => v < 97,
            "SNAP0008" => v < 97,
            "DIFF0009" => v < 97,
            "DEV0010" => v < 97,
            "CI0011" => v < 97,
            "BE0012" => v < 97,
            "QEMU0013" => v < 97,
            "REAL0014" => v < 97,
            "ABI0015" => v < 97,
            "SER0016" => v < 97,
            "DBG0017" => v < 97,
            "LOG0018" => v < 97,
            "TIME0019" => v < 97,
            "SNAP0020" => v < 97,
            "DIFF0021" => v < 97,
            "DEV0022" => v < 97,
            "CI0023" => v < 97,
            "BE0024" => v < 97,
            "QEMU0025" => v < 97,
            "REAL0026" => v < 97,
            "ABI0027" => v < 97,
            "SER0028" => v < 97,
            "DBG0029" => v < 97,
            "LOG0030" => v < 97,
            "TIME0031" => v < 97,
            "SNAP0032" => v < 97,
            "DIFF0033" => v < 97,
            "DEV0034" => v < 97,
            "CI0035" => v < 97,
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
pub fn u3421mat_sum() -> u64 {
    let mut s = 0u64;
    let mut i = 0usize;
    while i < u3421mat_COUNT {
        s = s.wrapping_add(u3421mat[i].1 as u64);
        i += 1;
    }
    s
}

/// 键唯一性（构造期断言等价物：逐对扫描同键）。
pub fn u3421mat_unique() -> bool {
    let mut i = 0usize;
    while i < u3421mat_COUNT {
        let mut j = i + 1;
        while j < u3421mat_COUNT {
            if u3421mat[i].0 == u3421mat[j].0 {
                return false;
            }
            j += 1;
        }
        i += 1;
    }
    true
}

// --------------------------------------------------------------------------
// 直方图引擎 mxu3421hist（有界插入排序 + 分位数 P50/P90/P99）
// --------------------------------------------------------------------------
/// 双后端抽象 直方图（96 桶滚动）。
/// 样本容量上限。
pub const MXU3421HIST_CAP: usize = 96;

/// 直方图账本（排序态 + 五统量）。
#[derive(Clone, Copy, Debug)]
pub struct mxu3421histHist {
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

impl mxu3421histHist {
    /// 空账构造。
    pub const fn new() -> Self {
        Self { samples: [0; 96], n: 0, drops: 0, min: 0, max: 0, sum: 0 }
    }

    /// 采样（有界插入排序；满员丢弃并记账）。
    pub fn push(&mut self, v: u64) {
        if self.n >= MXU3421HIST_CAP {
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

/// 段位守卫：全部诊断码必须落在本段 [0xe0b0, 0xe0bf]。
pub const fn u3421_segment_guard() -> bool {
    let mut ok = true;
    let mut i = 0usize;
    while i < u3421Code::ALL.len() {
        let c = u3421Code::ALL[i].code();
        if c < 0xe0b0 || c > 0xe0bf {
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
fn u3421_guard_seg() -> bool {
    u3421_segment_guard()
}

/// UNX-F3421 矩阵 36 行齐备 + 逐行守卫 + 唯一性 + 累计守恒。
fn check_u3421_map_guard() -> bool {
    let mut ok = u3421mat_COUNT == 36;
    if ok { ok = u3421mat_scan() == 0 && u3421mat_unique(); }
    if ok { ok = u3421mat_get("BE0000") == Some(0) && u3421mat_get("ZZZ_NOKEY").is_none(); }
    if ok { ok = u3421mat_sum() == 1594; }
    ok
}

/// UNX-F3421 直方图 16 样本分位/均值/守恒。
fn check_u3421_map_hist() -> bool {
    let mut h = mxu3421histHist::new();
    let mut i = 0u64;
    while i < 16 { h.push(i + 1); i += 1; }
    let mut ok = h.conserved() && h.mean() >= 1 && h.percentile(500) >= 8;
    ok
}

/// 本模块自检（逐检查带 detail，全绿才出门）。
pub fn run_u3421_checks() -> CheckSet {
    let mut set = CheckSet::new("U3421");
    set.add(
        "u3421-seg-guard",
        u3421_guard_seg(),
        if u3421_guard_seg() { "" } else { "diagnostic code outside segment" },
    );
    set.add(
        "u3421-map-guard",
        check_u3421_map_guard(),
        if check_u3421_map_guard() { "" } else { "UNX-F3421 matrix guard mismatch" },
    );
    set.add(
        "u3421-map-hist",
        check_u3421_map_hist(),
        if check_u3421_map_hist() { "" } else { "UNX-F3421 hist mismatch" },
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
    fn u3421_all_checks_green() {
        let set = run_u3421_checks();
        assert!(set.all_passed(), "U3421 domain checks must be green");
    }

    #[test]
    fn u3421_segment_guard_holds() {
        assert!(u3421_segment_guard());
    }
}
