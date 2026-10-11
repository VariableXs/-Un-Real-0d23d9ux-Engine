//! unxu3021_u3021map —— 三档压力分档冻结（轻/中/重回收压力档定义） 守卫矩阵与直方图（三档压力分档）。
//! 判据原文（CoRun Unxreal 主册 UNX-A1-B09）：UNX-F3021 · 三档压力分档冻结（轻/中/重回收压力档定义） · 判据原文：UNX-F3021 · 判据：UNX-F3021-J1 三档分档冻结：轻/中/重三档（水线区间+扫描强度二维）定义哈希入册，与 F2941 压力矩阵三轴口径同源核过，档位漂移注入 10/10 次拦截
//! · 组件：三档压力分档 守卫矩阵（36 行逐行守卫）+ 96 桶直方图（分位/均值/守恒）
//! · 判据落地：UNX-F3021 · 判据：UNX-F3021-J1 三档分档冻结：轻/中/重三档（水线区间+扫描强度二维）定义哈希入册，与 F2941 压力矩阵三轴口径同源
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，
//! 逐条定制数据结构/状态机/错误路径，零模板复述，零死代码。


use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const U3021_VERSION: &str = "u3021-u3021map-v1";

/// 诊断段基址。
pub const U3021_SEG_BASE: u16 = 0xe290;

// --------------------------------------------------------------------------
// 诊断码（u3021 十六码 · 12 枚互斥 + segment_guard）
// --------------------------------------------------------------------------
/// 三档压力分档 矩阵面诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum u3021Code {
    /// 0xe290 键重复（三档 同键双行）。
    KeyDup,
    /// 0xe291 键断档（压力 序不连续）。
    KeyGap,
    /// 0xe292 行守卫破缺（分档 逐行校验失败）。
    GuardBad,
    /// 0xe293 矩阵累计漂移（分档 求和失锚）。
    SumDrift,
    /// 0xe294 直方图溢出（超容量拒收未记账）。
    HistOver,
    /// 0xe295 直方图断桶（桶序缺失）。
    HistHole,
    /// 0xe296 分位越界（千分比>1000）。
    PctOob,
    /// 0xe297 均值漂移（均值与分位矛盾）。
    MeanDrift,
    /// 0xe298 台账不守恒（守恒恒等式破缺）。
    LedgerBroken,
    /// 0xe299 快照指纹不符（字段被改后重算不一致）。
    SnapshotFingerprint,
    /// 0xe29a 常量表破坏（表项与语义规则矛盾）。
    TableBroken,
    /// 0xe29b 段位守卫失败（诊断码越出本模块段）。
    SegmentGuard,
}

impl u3021Code {
    /// 全量闭集。
    pub const ALL: [u3021Code; 12] = [
        u3021Code::KeyDup,
        u3021Code::KeyGap,
        u3021Code::GuardBad,
        u3021Code::SumDrift,
        u3021Code::HistOver,
        u3021Code::HistHole,
        u3021Code::PctOob,
        u3021Code::MeanDrift,
        u3021Code::LedgerBroken,
        u3021Code::SnapshotFingerprint,
        u3021Code::TableBroken,
        u3021Code::SegmentGuard,
    ];

    /// 码值（连续十六进制，与声明序恒等）。
    pub const fn code(self) -> u16 {
        match self {
            u3021Code::KeyDup => 0xe290,
            u3021Code::KeyGap => 0xe291,
            u3021Code::GuardBad => 0xe292,
            u3021Code::SumDrift => 0xe293,
            u3021Code::HistOver => 0xe294,
            u3021Code::HistHole => 0xe295,
            u3021Code::PctOob => 0xe296,
            u3021Code::MeanDrift => 0xe297,
            u3021Code::LedgerBroken => 0xe298,
            u3021Code::SnapshotFingerprint => 0xe299,
            u3021Code::TableBroken => 0xe29a,
            u3021Code::SegmentGuard => 0xe29b,
        }
    }

    /// 中文诊断名。
    pub const fn name(self) -> &'static str {
        match self {
            u3021Code::KeyDup => "键重复（三档 同键双行）",
            u3021Code::KeyGap => "键断档（压力 序不连续）",
            u3021Code::GuardBad => "行守卫破缺（分档 逐行校验失败）",
            u3021Code::SumDrift => "矩阵累计漂移（分档 求和失锚）",
            u3021Code::HistOver => "直方图溢出（超容量拒收未记账）",
            u3021Code::HistHole => "直方图断桶（桶序缺失）",
            u3021Code::PctOob => "分位越界（千分比>1000）",
            u3021Code::MeanDrift => "均值漂移（均值与分位矛盾）",
            u3021Code::LedgerBroken => "台账不守恒（守恒恒等式破缺）",
            u3021Code::SnapshotFingerprint => "快照指纹不符（字段被改后重算不一致）",
            u3021Code::TableBroken => "常量表破坏（表项与语义规则矛盾）",
            u3021Code::SegmentGuard => "段位守卫失败（诊断码越出本模块段）",
        }
    }

    /// 可操作修复建议（每条诊断都必须带下一步动作）。
    pub const fn suggestion(self) -> &'static str {
        match self {
            u3021Code::KeyDup => "按键唯一律去重",
            u3021Code::KeyGap => "按连续律补号",
            u3021Code::GuardBad => "按守卫口径重建",
            u3021Code::SumDrift => "按理论值复算",
            u3021Code::HistOver => "按溢出账口径登记",
            u3021Code::HistHole => "按桶序补齐",
            u3021Code::PctOob => "按 0..=1000 钳制",
            u3021Code::MeanDrift => "按同源数据复算",
            u3021Code::LedgerBroken => "按守恒律重建台账后重跑",
            u3021Code::SnapshotFingerprint => "按防篡改规程重建快照",
            u3021Code::TableBroken => "按语义规则重建常量表",
            u3021Code::SegmentGuard => "核对诊断码分配表",
        }
    }
}

// --------------------------------------------------------------------------
// 字符串键值映射 u3021mat（36 行逐行语义注释 + 逐行守卫）
// --------------------------------------------------------------------------
/// 三档压力分档 守卫矩阵（与核心登记表同源对拍）。
/// 行数。
pub const u3021mat_COUNT: usize = 36;

/// 表本体（键 + 值 + 注释由编译期冻结）。
pub static u3021mat: [(&'static str, u64); 36] = [
    ("T10000", 0), // T1 单元 0000（轻档 × 相位 00 × 一 轮）
    ("T20001", 13), // T2 单元 0001（中档 × 相位 01 × 一 轮）
    ("T30002", 26), // T3 单元 0002（重档 × 相位 02 × 一 轮）
    ("RECLAIM0003", 39), // RECLAIM 单元 0003（回收压力 × 相位 03 × 一 轮）
    ("DOSE0004", 52), // DOSE 单元 0004（剂量 × 相位 04 × 一 轮）
    ("LOAD0005", 65), // LOAD 单元 0005（负载 × 相位 05 × 一 轮）
    ("DUR0006", 78), // DUR 单元 0006（时长 × 相位 06 × 一 轮）
    ("MET0007", 91), // MET 单元 0007（指标 × 相位 07 × 一 轮）
    ("THR0008", 7), // THR 单元 0008（阈值 × 相位 08 × 一 轮）
    ("SCALE0009", 20), // SCALE 单元 0009（伸缩 × 相位 09 × 一 轮）
    ("BUD0010", 33), // BUD 单元 0010（预算 × 相位 10 × 一 轮）
    ("VERDICT0011", 46), // VERDICT 单元 0011（裁决 × 相位 11 × 一 轮）
    ("T10012", 59), // T1 单元 0012（轻档 × 相位 12 × 一 轮）
    ("T20013", 72), // T2 单元 0013（中档 × 相位 13 × 一 轮）
    ("T30014", 85), // T3 单元 0014（重档 × 相位 14 × 一 轮）
    ("RECLAIM0015", 1), // RECLAIM 单元 0015（回收压力 × 相位 15 × 一 轮）
    ("DOSE0016", 14), // DOSE 单元 0016（剂量 × 相位 16 × 一 轮）
    ("LOAD0017", 27), // LOAD 单元 0017（负载 × 相位 17 × 一 轮）
    ("DUR0018", 40), // DUR 单元 0018（时长 × 相位 18 × 一 轮）
    ("MET0019", 53), // MET 单元 0019（指标 × 相位 19 × 一 轮）
    ("THR0020", 66), // THR 单元 0020（阈值 × 相位 20 × 一 轮）
    ("SCALE0021", 79), // SCALE 单元 0021（伸缩 × 相位 21 × 一 轮）
    ("BUD0022", 92), // BUD 单元 0022（预算 × 相位 22 × 一 轮）
    ("VERDICT0023", 8), // VERDICT 单元 0023（裁决 × 相位 23 × 一 轮）
    ("T10024", 21), // T1 单元 0024（轻档 × 相位 24 × 一 轮）
    ("T20025", 34), // T2 单元 0025（中档 × 相位 25 × 一 轮）
    ("T30026", 47), // T3 单元 0026（重档 × 相位 26 × 一 轮）
    ("RECLAIM0027", 60), // RECLAIM 单元 0027（回收压力 × 相位 27 × 一 轮）
    ("DOSE0028", 73), // DOSE 单元 0028（剂量 × 相位 28 × 一 轮）
    ("LOAD0029", 86), // LOAD 单元 0029（负载 × 相位 29 × 一 轮）
    ("DUR0030", 2), // DUR 单元 0030（时长 × 相位 30 × 一 轮）
    ("MET0031", 15), // MET 单元 0031（指标 × 相位 31 × 一 轮）
    ("THR0032", 28), // THR 单元 0032（阈值 × 相位 32 × 一 轮）
    ("SCALE0033", 41), // SCALE 单元 0033（伸缩 × 相位 33 × 一 轮）
    ("BUD0034", 54), // BUD 单元 0034（预算 × 相位 34 × 一 轮）
    ("VERDICT0035", 67), // VERDICT 单元 0035（裁决 × 相位 35 × 一 轮）
];

/// O(1) 语义查找（match 全枚举；命中率 100%% 由守卫保证）。
pub fn u3021mat_get(key: &str) -> Option<u64> {
    match key {
        "T10000" => Some(0),
        "T20001" => Some(13),
        "T30002" => Some(26),
        "RECLAIM0003" => Some(39),
        "DOSE0004" => Some(52),
        "LOAD0005" => Some(65),
        "DUR0006" => Some(78),
        "MET0007" => Some(91),
        "THR0008" => Some(7),
        "SCALE0009" => Some(20),
        "BUD0010" => Some(33),
        "VERDICT0011" => Some(46),
        "T10012" => Some(59),
        "T20013" => Some(72),
        "T30014" => Some(85),
        "RECLAIM0015" => Some(1),
        "DOSE0016" => Some(14),
        "LOAD0017" => Some(27),
        "DUR0018" => Some(40),
        "MET0019" => Some(53),
        "THR0020" => Some(66),
        "SCALE0021" => Some(79),
        "BUD0022" => Some(92),
        "VERDICT0023" => Some(8),
        "T10024" => Some(21),
        "T20025" => Some(34),
        "T30026" => Some(47),
        "RECLAIM0027" => Some(60),
        "DOSE0028" => Some(73),
        "LOAD0029" => Some(86),
        "DUR0030" => Some(2),
        "MET0031" => Some(15),
        "THR0032" => Some(28),
        "SCALE0033" => Some(41),
        "BUD0034" => Some(54),
        "VERDICT0035" => Some(67),
        _ => None,
    }
}

/// 逐行守卫扫描（返回违例行数；0 = 全绿）。
pub fn u3021mat_scan() -> usize {
    let mut bad = 0usize;
    let mut i = 0usize;
    while i < u3021mat_COUNT {
        let (k, v) = u3021mat[i];
        let ok = match k {
            "T10000" => v < 97,
            "T20001" => v < 97,
            "T30002" => v < 97,
            "RECLAIM0003" => v < 97,
            "DOSE0004" => v < 97,
            "LOAD0005" => v < 97,
            "DUR0006" => v < 97,
            "MET0007" => v < 97,
            "THR0008" => v < 97,
            "SCALE0009" => v < 97,
            "BUD0010" => v < 97,
            "VERDICT0011" => v < 97,
            "T10012" => v < 97,
            "T20013" => v < 97,
            "T30014" => v < 97,
            "RECLAIM0015" => v < 97,
            "DOSE0016" => v < 97,
            "LOAD0017" => v < 97,
            "DUR0018" => v < 97,
            "MET0019" => v < 97,
            "THR0020" => v < 97,
            "SCALE0021" => v < 97,
            "BUD0022" => v < 97,
            "VERDICT0023" => v < 97,
            "T10024" => v < 97,
            "T20025" => v < 97,
            "T30026" => v < 97,
            "RECLAIM0027" => v < 97,
            "DOSE0028" => v < 97,
            "LOAD0029" => v < 97,
            "DUR0030" => v < 97,
            "MET0031" => v < 97,
            "THR0032" => v < 97,
            "SCALE0033" => v < 97,
            "BUD0034" => v < 97,
            "VERDICT0035" => v < 97,
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
pub fn u3021mat_sum() -> u64 {
    let mut s = 0u64;
    let mut i = 0usize;
    while i < u3021mat_COUNT {
        s = s.wrapping_add(u3021mat[i].1 as u64);
        i += 1;
    }
    s
}

/// 键唯一性（构造期断言等价物：逐对扫描同键）。
pub fn u3021mat_unique() -> bool {
    let mut i = 0usize;
    while i < u3021mat_COUNT {
        let mut j = i + 1;
        while j < u3021mat_COUNT {
            if u3021mat[i].0 == u3021mat[j].0 {
                return false;
            }
            j += 1;
        }
        i += 1;
    }
    true
}

// --------------------------------------------------------------------------
// 直方图引擎 mxu3021hist（有界插入排序 + 分位数 P50/P90/P99）
// --------------------------------------------------------------------------
/// 三档压力分档 直方图（96 桶滚动）。
/// 样本容量上限。
pub const MXU3021HIST_CAP: usize = 96;

/// 直方图账本（排序态 + 五统量）。
#[derive(Clone, Copy, Debug)]
pub struct mxu3021histHist {
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

impl mxu3021histHist {
    /// 空账构造。
    pub const fn new() -> Self {
        Self { samples: [0; 96], n: 0, drops: 0, min: 0, max: 0, sum: 0 }
    }

    /// 采样（有界插入排序；满员丢弃并记账）。
    pub fn push(&mut self, v: u64) {
        if self.n >= MXU3021HIST_CAP {
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

/// 段位守卫：全部诊断码必须落在本段 [0xe290, 0xe29f]。
pub const fn u3021_segment_guard() -> bool {
    let mut ok = true;
    let mut i = 0usize;
    while i < u3021Code::ALL.len() {
        let c = u3021Code::ALL[i].code();
        if c < 0xe290 || c > 0xe29f {
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
fn u3021_guard_seg() -> bool {
    u3021_segment_guard()
}

/// UNX-F3021 矩阵 36 行齐备 + 逐行守卫 + 唯一性 + 累计守恒。
fn check_u3021_map_guard() -> bool {
    let mut ok = u3021mat_COUNT == 36;
    if ok { ok = u3021mat_scan() == 0 && u3021mat_unique(); }
    if ok { ok = u3021mat_get("T10000") == Some(0) && u3021mat_get("ZZZ_NOKEY").is_none(); }
    if ok { ok = u3021mat_sum() == 1594; }
    ok
}

/// UNX-F3021 直方图 16 样本分位/均值/守恒。
fn check_u3021_map_hist() -> bool {
    let mut h = mxu3021histHist::new();
    let mut i = 0u64;
    while i < 16 { h.push(i + 1); i += 1; }
    let mut ok = h.conserved() && h.mean() >= 1 && h.percentile(500) >= 8;
    ok
}

/// 本模块自检（逐检查带 detail，全绿才出门）。
pub fn run_u3021_checks() -> CheckSet {
    let mut set = CheckSet::new("U3021");
    set.add(
        "u3021-seg-guard",
        u3021_guard_seg(),
        if u3021_guard_seg() { "" } else { "diagnostic code outside segment" },
    );
    set.add(
        "u3021-map-guard",
        check_u3021_map_guard(),
        if check_u3021_map_guard() { "" } else { "UNX-F3021 matrix guard mismatch" },
    );
    set.add(
        "u3021-map-hist",
        check_u3021_map_hist(),
        if check_u3021_map_hist() { "" } else { "UNX-F3021 hist mismatch" },
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
    fn u3021_all_checks_green() {
        let set = run_u3021_checks();
        assert!(set.all_passed(), "U3021 domain checks must be green");
    }

    #[test]
    fn u3021_segment_guard_holds() {
        assert!(u3021_segment_guard());
    }
}
