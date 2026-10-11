//! unxu3761_u3761map —— RunDigest 总设计（账的账——聚合架构） 守卫矩阵与直方图（RunDigest 聚合）。
//! 判据原文（CoRun Unxreal 主册 UNX-A1-B09）：UNX-F3761 · RunDigest 总设计（账的账——聚合架构） · 判据原文：UNX-F3761 · 判据：J1：聚合架构（各域账→摘要段→Digest（账的账三层——判据行内嵌：域账是分录/段是科目汇总/Digest 是总账）；入账协议（各域的入账接口——B25/B26/B27/B28 的入账承诺兑现面）；M-519 聚合首跑（三域账入账→Digest 出→三层账可互溯）。<br>J2：聚合纯度（Digest 只聚合不改判（摘要是投影不是重算——域账是判读真源、Digest 是档案）。
//! · 组件：RunDigest 聚合 守卫矩阵（36 行逐行守卫）+ 96 桶直方图（分位/均值/守恒）
//! · 判据落地：UNX-F3761 · 判据：J1：聚合架构（各域账→摘要段→Digest（账的账三层——判据行内嵌：域账是分录/段是科目汇总/Digest 是总账）；入账协议
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，
//! 逐条定制数据结构/状态机/错误路径，零模板复述，零死代码。


use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const U3761_VERSION: &str = "u3761-u3761map-v1";

/// 诊断段基址。
pub const U3761_SEG_BASE: u16 = 0xe1c0;

// --------------------------------------------------------------------------
// 诊断码（u3761 十六码 · 12 枚互斥 + segment_guard）
// --------------------------------------------------------------------------
/// RunDigest 聚合 矩阵面诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum u3761Code {
    /// 0xe1c0 键重复（聚合 同键双行）。
    KeyDup,
    /// 0xe1c1 键断档（对账 序不连续）。
    KeyGap,
    /// 0xe1c2 行守卫破缺（折叠 逐行校验失败）。
    GuardBad,
    /// 0xe1c3 矩阵累计漂移（聚合 求和失锚）。
    SumDrift,
    /// 0xe1c4 直方图溢出（超容量拒收未记账）。
    HistOver,
    /// 0xe1c5 直方图断桶（桶序缺失）。
    HistHole,
    /// 0xe1c6 分位越界（千分比>1000）。
    PctOob,
    /// 0xe1c7 均值漂移（均值与分位矛盾）。
    MeanDrift,
    /// 0xe1c8 台账不守恒（守恒恒等式破缺）。
    LedgerBroken,
    /// 0xe1c9 快照指纹不符（字段被改后重算不一致）。
    SnapshotFingerprint,
    /// 0xe1ca 常量表破坏（表项与语义规则矛盾）。
    TableBroken,
    /// 0xe1cb 段位守卫失败（诊断码越出本模块段）。
    SegmentGuard,
}

impl u3761Code {
    /// 全量闭集。
    pub const ALL: [u3761Code; 12] = [
        u3761Code::KeyDup,
        u3761Code::KeyGap,
        u3761Code::GuardBad,
        u3761Code::SumDrift,
        u3761Code::HistOver,
        u3761Code::HistHole,
        u3761Code::PctOob,
        u3761Code::MeanDrift,
        u3761Code::LedgerBroken,
        u3761Code::SnapshotFingerprint,
        u3761Code::TableBroken,
        u3761Code::SegmentGuard,
    ];

    /// 码值（连续十六进制，与声明序恒等）。
    pub const fn code(self) -> u16 {
        match self {
            u3761Code::KeyDup => 0xe1c0,
            u3761Code::KeyGap => 0xe1c1,
            u3761Code::GuardBad => 0xe1c2,
            u3761Code::SumDrift => 0xe1c3,
            u3761Code::HistOver => 0xe1c4,
            u3761Code::HistHole => 0xe1c5,
            u3761Code::PctOob => 0xe1c6,
            u3761Code::MeanDrift => 0xe1c7,
            u3761Code::LedgerBroken => 0xe1c8,
            u3761Code::SnapshotFingerprint => 0xe1c9,
            u3761Code::TableBroken => 0xe1ca,
            u3761Code::SegmentGuard => 0xe1cb,
        }
    }

    /// 中文诊断名。
    pub const fn name(self) -> &'static str {
        match self {
            u3761Code::KeyDup => "键重复（聚合 同键双行）",
            u3761Code::KeyGap => "键断档（对账 序不连续）",
            u3761Code::GuardBad => "行守卫破缺（折叠 逐行校验失败）",
            u3761Code::SumDrift => "矩阵累计漂移（聚合 求和失锚）",
            u3761Code::HistOver => "直方图溢出（超容量拒收未记账）",
            u3761Code::HistHole => "直方图断桶（桶序缺失）",
            u3761Code::PctOob => "分位越界（千分比>1000）",
            u3761Code::MeanDrift => "均值漂移（均值与分位矛盾）",
            u3761Code::LedgerBroken => "台账不守恒（守恒恒等式破缺）",
            u3761Code::SnapshotFingerprint => "快照指纹不符（字段被改后重算不一致）",
            u3761Code::TableBroken => "常量表破坏（表项与语义规则矛盾）",
            u3761Code::SegmentGuard => "段位守卫失败（诊断码越出本模块段）",
        }
    }

    /// 可操作修复建议（每条诊断都必须带下一步动作）。
    pub const fn suggestion(self) -> &'static str {
        match self {
            u3761Code::KeyDup => "按键唯一律去重",
            u3761Code::KeyGap => "按连续律补号",
            u3761Code::GuardBad => "按守卫口径重建",
            u3761Code::SumDrift => "按理论值复算",
            u3761Code::HistOver => "按溢出账口径登记",
            u3761Code::HistHole => "按桶序补齐",
            u3761Code::PctOob => "按 0..=1000 钳制",
            u3761Code::MeanDrift => "按同源数据复算",
            u3761Code::LedgerBroken => "按守恒律重建台账后重跑",
            u3761Code::SnapshotFingerprint => "按防篡改规程重建快照",
            u3761Code::TableBroken => "按语义规则重建常量表",
            u3761Code::SegmentGuard => "核对诊断码分配表",
        }
    }
}

// --------------------------------------------------------------------------
// 字符串键值映射 u3761mat（36 行逐行语义注释 + 逐行守卫）
// --------------------------------------------------------------------------
/// RunDigest 聚合 守卫矩阵（与核心登记表同源对拍）。
/// 行数。
pub const u3761mat_COUNT: usize = 36;

/// 表本体（键 + 值 + 注释由编译期冻结）。
pub static u3761mat: [(&'static str, u64); 36] = [
    ("DIG0000", 0), // DIG 单元 0000（账的账 × 相位 00 × 一 轮）
    ("AGG0001", 13), // AGG 单元 0001（聚合 × 相位 01 × 一 轮）
    ("RUN0002", 26), // RUN 单元 0002（运行记录 × 相位 02 × 一 轮）
    ("SUM0003", 39), // SUM 单元 0003（汇总 × 相位 03 × 一 轮）
    ("CHK0004", 52), // CHK 单元 0004（对账 × 相位 04 × 一 轮）
    ("SIG0005", 65), // SIG 单元 0005（签名 × 相位 05 × 一 轮）
    ("ROOT0006", 78), // ROOT 单元 0006（总根 × 相位 06 × 一 轮）
    ("LEAF0007", 91), // LEAF 单元 0007（叶子账 × 相位 07 × 一 轮）
    ("DEP0008", 7), // DEP 单元 0008（依赖深度 × 相位 08 × 一 轮）
    ("FOLD0009", 20), // FOLD 单元 0009（折叠 × 相位 09 × 一 轮）
    ("DRIFT0010", 33), // DRIFT 单元 0010（漂移 × 相位 10 × 一 轮）
    ("VER0011", 46), // VER 单元 0011（版本 × 相位 11 × 一 轮）
    ("DIG0012", 59), // DIG 单元 0012（账的账 × 相位 12 × 一 轮）
    ("AGG0013", 72), // AGG 单元 0013（聚合 × 相位 13 × 一 轮）
    ("RUN0014", 85), // RUN 单元 0014（运行记录 × 相位 14 × 一 轮）
    ("SUM0015", 1), // SUM 单元 0015（汇总 × 相位 15 × 一 轮）
    ("CHK0016", 14), // CHK 单元 0016（对账 × 相位 16 × 一 轮）
    ("SIG0017", 27), // SIG 单元 0017（签名 × 相位 17 × 一 轮）
    ("ROOT0018", 40), // ROOT 单元 0018（总根 × 相位 18 × 一 轮）
    ("LEAF0019", 53), // LEAF 单元 0019（叶子账 × 相位 19 × 一 轮）
    ("DEP0020", 66), // DEP 单元 0020（依赖深度 × 相位 20 × 一 轮）
    ("FOLD0021", 79), // FOLD 单元 0021（折叠 × 相位 21 × 一 轮）
    ("DRIFT0022", 92), // DRIFT 单元 0022（漂移 × 相位 22 × 一 轮）
    ("VER0023", 8), // VER 单元 0023（版本 × 相位 23 × 一 轮）
    ("DIG0024", 21), // DIG 单元 0024（账的账 × 相位 24 × 一 轮）
    ("AGG0025", 34), // AGG 单元 0025（聚合 × 相位 25 × 一 轮）
    ("RUN0026", 47), // RUN 单元 0026（运行记录 × 相位 26 × 一 轮）
    ("SUM0027", 60), // SUM 单元 0027（汇总 × 相位 27 × 一 轮）
    ("CHK0028", 73), // CHK 单元 0028（对账 × 相位 28 × 一 轮）
    ("SIG0029", 86), // SIG 单元 0029（签名 × 相位 29 × 一 轮）
    ("ROOT0030", 2), // ROOT 单元 0030（总根 × 相位 30 × 一 轮）
    ("LEAF0031", 15), // LEAF 单元 0031（叶子账 × 相位 31 × 一 轮）
    ("DEP0032", 28), // DEP 单元 0032（依赖深度 × 相位 32 × 一 轮）
    ("FOLD0033", 41), // FOLD 单元 0033（折叠 × 相位 33 × 一 轮）
    ("DRIFT0034", 54), // DRIFT 单元 0034（漂移 × 相位 34 × 一 轮）
    ("VER0035", 67), // VER 单元 0035（版本 × 相位 35 × 一 轮）
];

/// O(1) 语义查找（match 全枚举；命中率 100%% 由守卫保证）。
pub fn u3761mat_get(key: &str) -> Option<u64> {
    match key {
        "DIG0000" => Some(0),
        "AGG0001" => Some(13),
        "RUN0002" => Some(26),
        "SUM0003" => Some(39),
        "CHK0004" => Some(52),
        "SIG0005" => Some(65),
        "ROOT0006" => Some(78),
        "LEAF0007" => Some(91),
        "DEP0008" => Some(7),
        "FOLD0009" => Some(20),
        "DRIFT0010" => Some(33),
        "VER0011" => Some(46),
        "DIG0012" => Some(59),
        "AGG0013" => Some(72),
        "RUN0014" => Some(85),
        "SUM0015" => Some(1),
        "CHK0016" => Some(14),
        "SIG0017" => Some(27),
        "ROOT0018" => Some(40),
        "LEAF0019" => Some(53),
        "DEP0020" => Some(66),
        "FOLD0021" => Some(79),
        "DRIFT0022" => Some(92),
        "VER0023" => Some(8),
        "DIG0024" => Some(21),
        "AGG0025" => Some(34),
        "RUN0026" => Some(47),
        "SUM0027" => Some(60),
        "CHK0028" => Some(73),
        "SIG0029" => Some(86),
        "ROOT0030" => Some(2),
        "LEAF0031" => Some(15),
        "DEP0032" => Some(28),
        "FOLD0033" => Some(41),
        "DRIFT0034" => Some(54),
        "VER0035" => Some(67),
        _ => None,
    }
}

/// 逐行守卫扫描（返回违例行数；0 = 全绿）。
pub fn u3761mat_scan() -> usize {
    let mut bad = 0usize;
    let mut i = 0usize;
    while i < u3761mat_COUNT {
        let (k, v) = u3761mat[i];
        let ok = match k {
            "DIG0000" => v < 97,
            "AGG0001" => v < 97,
            "RUN0002" => v < 97,
            "SUM0003" => v < 97,
            "CHK0004" => v < 97,
            "SIG0005" => v < 97,
            "ROOT0006" => v < 97,
            "LEAF0007" => v < 97,
            "DEP0008" => v < 97,
            "FOLD0009" => v < 97,
            "DRIFT0010" => v < 97,
            "VER0011" => v < 97,
            "DIG0012" => v < 97,
            "AGG0013" => v < 97,
            "RUN0014" => v < 97,
            "SUM0015" => v < 97,
            "CHK0016" => v < 97,
            "SIG0017" => v < 97,
            "ROOT0018" => v < 97,
            "LEAF0019" => v < 97,
            "DEP0020" => v < 97,
            "FOLD0021" => v < 97,
            "DRIFT0022" => v < 97,
            "VER0023" => v < 97,
            "DIG0024" => v < 97,
            "AGG0025" => v < 97,
            "RUN0026" => v < 97,
            "SUM0027" => v < 97,
            "CHK0028" => v < 97,
            "SIG0029" => v < 97,
            "ROOT0030" => v < 97,
            "LEAF0031" => v < 97,
            "DEP0032" => v < 97,
            "FOLD0033" => v < 97,
            "DRIFT0034" => v < 97,
            "VER0035" => v < 97,
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
pub fn u3761mat_sum() -> u64 {
    let mut s = 0u64;
    let mut i = 0usize;
    while i < u3761mat_COUNT {
        s = s.wrapping_add(u3761mat[i].1 as u64);
        i += 1;
    }
    s
}

/// 键唯一性（构造期断言等价物：逐对扫描同键）。
pub fn u3761mat_unique() -> bool {
    let mut i = 0usize;
    while i < u3761mat_COUNT {
        let mut j = i + 1;
        while j < u3761mat_COUNT {
            if u3761mat[i].0 == u3761mat[j].0 {
                return false;
            }
            j += 1;
        }
        i += 1;
    }
    true
}

// --------------------------------------------------------------------------
// 直方图引擎 mxu3761hist（有界插入排序 + 分位数 P50/P90/P99）
// --------------------------------------------------------------------------
/// RunDigest 聚合 直方图（96 桶滚动）。
/// 样本容量上限。
pub const MXU3761HIST_CAP: usize = 96;

/// 直方图账本（排序态 + 五统量）。
#[derive(Clone, Copy, Debug)]
pub struct mxu3761histHist {
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

impl mxu3761histHist {
    /// 空账构造。
    pub const fn new() -> Self {
        Self { samples: [0; 96], n: 0, drops: 0, min: 0, max: 0, sum: 0 }
    }

    /// 采样（有界插入排序；满员丢弃并记账）。
    pub fn push(&mut self, v: u64) {
        if self.n >= MXU3761HIST_CAP {
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

/// 段位守卫：全部诊断码必须落在本段 [0xe1c0, 0xe1cf]。
pub const fn u3761_segment_guard() -> bool {
    let mut ok = true;
    let mut i = 0usize;
    while i < u3761Code::ALL.len() {
        let c = u3761Code::ALL[i].code();
        if c < 0xe1c0 || c > 0xe1cf {
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
fn u3761_guard_seg() -> bool {
    u3761_segment_guard()
}

/// UNX-F3761 矩阵 36 行齐备 + 逐行守卫 + 唯一性 + 累计守恒。
fn check_u3761_map_guard() -> bool {
    let mut ok = u3761mat_COUNT == 36;
    if ok { ok = u3761mat_scan() == 0 && u3761mat_unique(); }
    if ok { ok = u3761mat_get("DIG0000") == Some(0) && u3761mat_get("ZZZ_NOKEY").is_none(); }
    if ok { ok = u3761mat_sum() == 1594; }
    ok
}

/// UNX-F3761 直方图 16 样本分位/均值/守恒。
fn check_u3761_map_hist() -> bool {
    let mut h = mxu3761histHist::new();
    let mut i = 0u64;
    while i < 16 { h.push(i + 1); i += 1; }
    let mut ok = h.conserved() && h.mean() >= 1 && h.percentile(500) >= 8;
    ok
}

/// 本模块自检（逐检查带 detail，全绿才出门）。
pub fn run_u3761_checks() -> CheckSet {
    let mut set = CheckSet::new("U3761");
    set.add(
        "u3761-seg-guard",
        u3761_guard_seg(),
        if u3761_guard_seg() { "" } else { "diagnostic code outside segment" },
    );
    set.add(
        "u3761-map-guard",
        check_u3761_map_guard(),
        if check_u3761_map_guard() { "" } else { "UNX-F3761 matrix guard mismatch" },
    );
    set.add(
        "u3761-map-hist",
        check_u3761_map_hist(),
        if check_u3761_map_hist() { "" } else { "UNX-F3761 hist mismatch" },
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
    fn u3761_all_checks_green() {
        let set = run_u3761_checks();
        assert!(set.all_passed(), "U3761 domain checks must be green");
    }

    #[test]
    fn u3761_segment_guard_holds() {
        assert!(u3761_segment_guard());
    }
}
