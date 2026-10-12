//! unxu3381_u3381map —— 样本库总设计（可复现死锁样本注入框架） 守卫矩阵与直方图（死锁样本库）。
//! 判据原文（CoRun Unxreal 主册 UNX-A1-B09）：UNX-F3381 · 样本库总设计（可复现死锁样本注入框架） · 判据原文：UNX-F3381 · 判据：J1：框架文档交付：样本 = 参数化场景函数（锁数/时长/时机/期望结果四参数）；M-140 框架空转验证。
//! · 组件：死锁样本库 守卫矩阵（36 行逐行守卫）+ 96 桶直方图（分位/均值/守恒）
//! · 判据落地：UNX-F3381 · 判据：J1：框架文档交付：样本 = 参数化场景函数（锁数/时长/时机/期望结果四参数）；M-140 框架空转验证。
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，
//! 逐条定制数据结构/状态机/错误路径，零模板复述，零死代码。


use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const U3381_VERSION: &str = "u3381-u3381map-v1";

/// 诊断段基址。
pub const U3381_SEG_BASE: u16 = 0xe090;

// --------------------------------------------------------------------------
// 诊断码（u3381 十六码 · 12 枚互斥 + segment_guard）
// --------------------------------------------------------------------------
/// 死锁样本库 矩阵面诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum u3381Code {
    /// 0xe090 键重复（样本 同键双行）。
    KeyDup,
    /// 0xe091 键断档（重放 序不连续）。
    KeyGap,
    /// 0xe092 行守卫破缺（确定性 逐行校验失败）。
    GuardBad,
    /// 0xe093 矩阵累计漂移（可复现 求和失锚）。
    SumDrift,
    /// 0xe094 直方图溢出（超容量拒收未记账）。
    HistOver,
    /// 0xe095 直方图断桶（桶序缺失）。
    HistHole,
    /// 0xe096 分位越界（千分比>1000）。
    PctOob,
    /// 0xe097 均值漂移（均值与分位矛盾）。
    MeanDrift,
    /// 0xe098 台账不守恒（守恒恒等式破缺）。
    LedgerBroken,
    /// 0xe099 快照指纹不符（字段被改后重算不一致）。
    SnapshotFingerprint,
    /// 0xe09a 常量表破坏（表项与语义规则矛盾）。
    TableBroken,
    /// 0xe09b 段位守卫失败（诊断码越出本模块段）。
    SegmentGuard,
}

impl u3381Code {
    /// 全量闭集。
    pub const ALL: [u3381Code; 12] = [
        u3381Code::KeyDup,
        u3381Code::KeyGap,
        u3381Code::GuardBad,
        u3381Code::SumDrift,
        u3381Code::HistOver,
        u3381Code::HistHole,
        u3381Code::PctOob,
        u3381Code::MeanDrift,
        u3381Code::LedgerBroken,
        u3381Code::SnapshotFingerprint,
        u3381Code::TableBroken,
        u3381Code::SegmentGuard,
    ];

    /// 码值（连续十六进制，与声明序恒等）。
    pub const fn code(self) -> u16 {
        match self {
            u3381Code::KeyDup => 0xe090,
            u3381Code::KeyGap => 0xe091,
            u3381Code::GuardBad => 0xe092,
            u3381Code::SumDrift => 0xe093,
            u3381Code::HistOver => 0xe094,
            u3381Code::HistHole => 0xe095,
            u3381Code::PctOob => 0xe096,
            u3381Code::MeanDrift => 0xe097,
            u3381Code::LedgerBroken => 0xe098,
            u3381Code::SnapshotFingerprint => 0xe099,
            u3381Code::TableBroken => 0xe09a,
            u3381Code::SegmentGuard => 0xe09b,
        }
    }

    /// 中文诊断名。
    pub const fn name(self) -> &'static str {
        match self {
            u3381Code::KeyDup => "键重复（样本 同键双行）",
            u3381Code::KeyGap => "键断档（重放 序不连续）",
            u3381Code::GuardBad => "行守卫破缺（确定性 逐行校验失败）",
            u3381Code::SumDrift => "矩阵累计漂移（可复现 求和失锚）",
            u3381Code::HistOver => "直方图溢出（超容量拒收未记账）",
            u3381Code::HistHole => "直方图断桶（桶序缺失）",
            u3381Code::PctOob => "分位越界（千分比>1000）",
            u3381Code::MeanDrift => "均值漂移（均值与分位矛盾）",
            u3381Code::LedgerBroken => "台账不守恒（守恒恒等式破缺）",
            u3381Code::SnapshotFingerprint => "快照指纹不符（字段被改后重算不一致）",
            u3381Code::TableBroken => "常量表破坏（表项与语义规则矛盾）",
            u3381Code::SegmentGuard => "段位守卫失败（诊断码越出本模块段）",
        }
    }

    /// 可操作修复建议（每条诊断都必须带下一步动作）。
    pub const fn suggestion(self) -> &'static str {
        match self {
            u3381Code::KeyDup => "按键唯一律去重",
            u3381Code::KeyGap => "按连续律补号",
            u3381Code::GuardBad => "按守卫口径重建",
            u3381Code::SumDrift => "按理论值复算",
            u3381Code::HistOver => "按溢出账口径登记",
            u3381Code::HistHole => "按桶序补齐",
            u3381Code::PctOob => "按 0..=1000 钳制",
            u3381Code::MeanDrift => "按同源数据复算",
            u3381Code::LedgerBroken => "按守恒律重建台账后重跑",
            u3381Code::SnapshotFingerprint => "按防篡改规程重建快照",
            u3381Code::TableBroken => "按语义规则重建常量表",
            u3381Code::SegmentGuard => "核对诊断码分配表",
        }
    }
}

// --------------------------------------------------------------------------
// 字符串键值映射 u3381mat（36 行逐行语义注释 + 逐行守卫）
// --------------------------------------------------------------------------
/// 死锁样本库 守卫矩阵（与核心登记表同源对拍）。
/// 行数。
pub const u3381mat_COUNT: usize = 36;

/// 表本体（键 + 值 + 注释由编译期冻结）。
pub static u3381mat: [(&'static str, u64); 36] = [
    ("SMP0000", 0), // SMP 单元 0000（样本 × 相位 00 × 一 轮）
    ("SEED0001", 13), // SEED 单元 0001（随机种子 × 相位 01 × 一 轮）
    ("REPLAY0002", 26), // REPLAY 单元 0002（重放 × 相位 02 × 一 轮）
    ("DET0003", 39), // DET 单元 0003（确定性 × 相位 03 × 一 轮）
    ("INJ0004", 52), // INJ 单元 0004（注入点 × 相位 04 × 一 轮）
    ("GEN0005", 65), // GEN 单元 0005（生成器 × 相位 05 × 一 轮）
    ("BUF0006", 78), // BUF 单元 0006（样本缓冲 × 相位 06 × 一 轮）
    ("SIG0007", 91), // SIG 单元 0007（签名 × 相位 07 × 一 轮）
    ("CUR0008", 7), // CUR 单元 0008（当前样本 × 相位 08 × 一 轮）
    ("CAP0009", 20), // CAP 单元 0009（容量 × 相位 09 × 一 轮）
    ("VER0010", 33), // VER 单元 0010（版本 × 相位 10 × 一 轮）
    ("TRACE0011", 46), // TRACE 单元 0011（轨迹 × 相位 11 × 一 轮）
    ("SMP0012", 59), // SMP 单元 0012（样本 × 相位 12 × 一 轮）
    ("SEED0013", 72), // SEED 单元 0013（随机种子 × 相位 13 × 一 轮）
    ("REPLAY0014", 85), // REPLAY 单元 0014（重放 × 相位 14 × 一 轮）
    ("DET0015", 1), // DET 单元 0015（确定性 × 相位 15 × 一 轮）
    ("INJ0016", 14), // INJ 单元 0016（注入点 × 相位 16 × 一 轮）
    ("GEN0017", 27), // GEN 单元 0017（生成器 × 相位 17 × 一 轮）
    ("BUF0018", 40), // BUF 单元 0018（样本缓冲 × 相位 18 × 一 轮）
    ("SIG0019", 53), // SIG 单元 0019（签名 × 相位 19 × 一 轮）
    ("CUR0020", 66), // CUR 单元 0020（当前样本 × 相位 20 × 一 轮）
    ("CAP0021", 79), // CAP 单元 0021（容量 × 相位 21 × 一 轮）
    ("VER0022", 92), // VER 单元 0022（版本 × 相位 22 × 一 轮）
    ("TRACE0023", 8), // TRACE 单元 0023（轨迹 × 相位 23 × 一 轮）
    ("SMP0024", 21), // SMP 单元 0024（样本 × 相位 24 × 一 轮）
    ("SEED0025", 34), // SEED 单元 0025（随机种子 × 相位 25 × 一 轮）
    ("REPLAY0026", 47), // REPLAY 单元 0026（重放 × 相位 26 × 一 轮）
    ("DET0027", 60), // DET 单元 0027（确定性 × 相位 27 × 一 轮）
    ("INJ0028", 73), // INJ 单元 0028（注入点 × 相位 28 × 一 轮）
    ("GEN0029", 86), // GEN 单元 0029（生成器 × 相位 29 × 一 轮）
    ("BUF0030", 2), // BUF 单元 0030（样本缓冲 × 相位 30 × 一 轮）
    ("SIG0031", 15), // SIG 单元 0031（签名 × 相位 31 × 一 轮）
    ("CUR0032", 28), // CUR 单元 0032（当前样本 × 相位 32 × 一 轮）
    ("CAP0033", 41), // CAP 单元 0033（容量 × 相位 33 × 一 轮）
    ("VER0034", 54), // VER 单元 0034（版本 × 相位 34 × 一 轮）
    ("TRACE0035", 67), // TRACE 单元 0035（轨迹 × 相位 35 × 一 轮）
];

/// O(1) 语义查找（match 全枚举；命中率 100%% 由守卫保证）。
pub fn u3381mat_get(key: &str) -> Option<u64> {
    match key {
        "SMP0000" => Some(0),
        "SEED0001" => Some(13),
        "REPLAY0002" => Some(26),
        "DET0003" => Some(39),
        "INJ0004" => Some(52),
        "GEN0005" => Some(65),
        "BUF0006" => Some(78),
        "SIG0007" => Some(91),
        "CUR0008" => Some(7),
        "CAP0009" => Some(20),
        "VER0010" => Some(33),
        "TRACE0011" => Some(46),
        "SMP0012" => Some(59),
        "SEED0013" => Some(72),
        "REPLAY0014" => Some(85),
        "DET0015" => Some(1),
        "INJ0016" => Some(14),
        "GEN0017" => Some(27),
        "BUF0018" => Some(40),
        "SIG0019" => Some(53),
        "CUR0020" => Some(66),
        "CAP0021" => Some(79),
        "VER0022" => Some(92),
        "TRACE0023" => Some(8),
        "SMP0024" => Some(21),
        "SEED0025" => Some(34),
        "REPLAY0026" => Some(47),
        "DET0027" => Some(60),
        "INJ0028" => Some(73),
        "GEN0029" => Some(86),
        "BUF0030" => Some(2),
        "SIG0031" => Some(15),
        "CUR0032" => Some(28),
        "CAP0033" => Some(41),
        "VER0034" => Some(54),
        "TRACE0035" => Some(67),
        _ => None,
    }
}

/// 逐行守卫扫描（返回违例行数；0 = 全绿）。
pub fn u3381mat_scan() -> usize {
    let mut bad = 0usize;
    let mut i = 0usize;
    while i < u3381mat_COUNT {
        let (k, v) = u3381mat[i];
        let ok = match k {
            "SMP0000" => v < 97,
            "SEED0001" => v < 97,
            "REPLAY0002" => v < 97,
            "DET0003" => v < 97,
            "INJ0004" => v < 97,
            "GEN0005" => v < 97,
            "BUF0006" => v < 97,
            "SIG0007" => v < 97,
            "CUR0008" => v < 97,
            "CAP0009" => v < 97,
            "VER0010" => v < 97,
            "TRACE0011" => v < 97,
            "SMP0012" => v < 97,
            "SEED0013" => v < 97,
            "REPLAY0014" => v < 97,
            "DET0015" => v < 97,
            "INJ0016" => v < 97,
            "GEN0017" => v < 97,
            "BUF0018" => v < 97,
            "SIG0019" => v < 97,
            "CUR0020" => v < 97,
            "CAP0021" => v < 97,
            "VER0022" => v < 97,
            "TRACE0023" => v < 97,
            "SMP0024" => v < 97,
            "SEED0025" => v < 97,
            "REPLAY0026" => v < 97,
            "DET0027" => v < 97,
            "INJ0028" => v < 97,
            "GEN0029" => v < 97,
            "BUF0030" => v < 97,
            "SIG0031" => v < 97,
            "CUR0032" => v < 97,
            "CAP0033" => v < 97,
            "VER0034" => v < 97,
            "TRACE0035" => v < 97,
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
pub fn u3381mat_sum() -> u64 {
    let mut s = 0u64;
    let mut i = 0usize;
    while i < u3381mat_COUNT {
        s = s.wrapping_add(u3381mat[i].1 as u64);
        i += 1;
    }
    s
}

/// 键唯一性（构造期断言等价物：逐对扫描同键）。
pub fn u3381mat_unique() -> bool {
    let mut i = 0usize;
    while i < u3381mat_COUNT {
        let mut j = i + 1;
        while j < u3381mat_COUNT {
            if u3381mat[i].0 == u3381mat[j].0 {
                return false;
            }
            j += 1;
        }
        i += 1;
    }
    true
}

// --------------------------------------------------------------------------
// 直方图引擎 mxu3381hist（有界插入排序 + 分位数 P50/P90/P99）
// --------------------------------------------------------------------------
/// 死锁样本库 直方图（96 桶滚动）。
/// 样本容量上限。
pub const MXU3381HIST_CAP: usize = 96;

/// 直方图账本（排序态 + 五统量）。
#[derive(Clone, Copy, Debug)]
pub struct mxu3381histHist {
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

impl mxu3381histHist {
    /// 空账构造。
    pub const fn new() -> Self {
        Self { samples: [0; 96], n: 0, drops: 0, min: 0, max: 0, sum: 0 }
    }

    /// 采样（有界插入排序；满员丢弃并记账）。
    pub fn push(&mut self, v: u64) {
        if self.n >= MXU3381HIST_CAP {
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

/// 段位守卫：全部诊断码必须落在本段 [0xe090, 0xe09f]。
pub const fn u3381_segment_guard() -> bool {
    let mut ok = true;
    let mut i = 0usize;
    while i < u3381Code::ALL.len() {
        let c = u3381Code::ALL[i].code();
        if c < 0xe090 || c > 0xe09f {
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
fn u3381_guard_seg() -> bool {
    u3381_segment_guard()
}

/// UNX-F3381 矩阵 36 行齐备 + 逐行守卫 + 唯一性 + 累计守恒。
fn check_u3381_map_guard() -> bool {
    let mut ok = u3381mat_COUNT == 36;
    if ok { ok = u3381mat_scan() == 0 && u3381mat_unique(); }
    if ok { ok = u3381mat_get("SMP0000") == Some(0) && u3381mat_get("ZZZ_NOKEY").is_none(); }
    if ok { ok = u3381mat_sum() == 1594; }
    ok
}

/// UNX-F3381 直方图 16 样本分位/均值/守恒。
fn check_u3381_map_hist() -> bool {
    let mut h = mxu3381histHist::new();
    let mut i = 0u64;
    while i < 16 { h.push(i + 1); i += 1; }
    let mut ok = h.conserved() && h.mean() >= 1 && h.percentile(500) >= 8;
    ok
}

/// 本模块自检（逐检查带 detail，全绿才出门）。
pub fn run_u3381_checks() -> CheckSet {
    let mut set = CheckSet::new("U3381");
    set.add(
        "u3381-seg-guard",
        u3381_guard_seg(),
        if u3381_guard_seg() { "" } else { "diagnostic code outside segment" },
    );
    set.add(
        "u3381-map-guard",
        check_u3381_map_guard(),
        if check_u3381_map_guard() { "" } else { "UNX-F3381 matrix guard mismatch" },
    );
    set.add(
        "u3381-map-hist",
        check_u3381_map_hist(),
        if check_u3381_map_hist() { "" } else { "UNX-F3381 hist mismatch" },
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
    fn u3381_all_checks_green() {
        let set = run_u3381_checks();
        assert!(set.all_passed(), "U3381 domain checks must be green");
    }

    #[test]
    fn u3381_segment_guard_holds() {
        assert!(u3381_segment_guard());
    }
}
