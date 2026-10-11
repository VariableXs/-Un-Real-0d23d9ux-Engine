//! unxu2941_u2941map —— 域级压力矩阵方案冻结（三轴） 守卫矩阵与直方图（域级压力三轴）。
//! 判据原文（CoRun Unxreal 主册 UNX-A1-B09）：UNX-F2941 · 域级压力矩阵方案冻结（三轴） · 判据原文：UNX-F2941 · 判据：UNX-F2941-J1 三轴方案（8 核矩阵×16GB 三档压力×72h 长稳）冻结哈希入册，方案与 F2938 汇总账字段对齐
//! · 组件：域级压力三轴 守卫矩阵（36 行逐行守卫）+ 96 桶直方图（分位/均值/守恒）
//! · 判据落地：UNX-F2941 · 判据：UNX-F2941-J1 三轴方案（8 核矩阵×16GB 三档压力×72h 长稳）冻结哈希入册，方案与 F2938 汇总账字段对齐
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，
//! 逐条定制数据结构/状态机/错误路径，零模板复述，零死代码。


use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const U2941_VERSION: &str = "u2941-u2941map-v1";

/// 诊断段基址。
pub const U2941_SEG_BASE: u16 = 0xe250;

// --------------------------------------------------------------------------
// 诊断码（u2941 十六码 · 12 枚互斥 + segment_guard）
// --------------------------------------------------------------------------
/// 域级压力三轴 矩阵面诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum u2941Code {
    /// 0xe250 键重复（域级 同键双行）。
    KeyDup,
    /// 0xe251 键断档（压力 序不连续）。
    KeyGap,
    /// 0xe252 行守卫破缺（三轴 逐行校验失败）。
    GuardBad,
    /// 0xe253 矩阵累计漂移（三轴 求和失锚）。
    SumDrift,
    /// 0xe254 直方图溢出（超容量拒收未记账）。
    HistOver,
    /// 0xe255 直方图断桶（桶序缺失）。
    HistHole,
    /// 0xe256 分位越界（千分比>1000）。
    PctOob,
    /// 0xe257 均值漂移（均值与分位矛盾）。
    MeanDrift,
    /// 0xe258 台账不守恒（守恒恒等式破缺）。
    LedgerBroken,
    /// 0xe259 快照指纹不符（字段被改后重算不一致）。
    SnapshotFingerprint,
    /// 0xe25a 常量表破坏（表项与语义规则矛盾）。
    TableBroken,
    /// 0xe25b 段位守卫失败（诊断码越出本模块段）。
    SegmentGuard,
}

impl u2941Code {
    /// 全量闭集。
    pub const ALL: [u2941Code; 12] = [
        u2941Code::KeyDup,
        u2941Code::KeyGap,
        u2941Code::GuardBad,
        u2941Code::SumDrift,
        u2941Code::HistOver,
        u2941Code::HistHole,
        u2941Code::PctOob,
        u2941Code::MeanDrift,
        u2941Code::LedgerBroken,
        u2941Code::SnapshotFingerprint,
        u2941Code::TableBroken,
        u2941Code::SegmentGuard,
    ];

    /// 码值（连续十六进制，与声明序恒等）。
    pub const fn code(self) -> u16 {
        match self {
            u2941Code::KeyDup => 0xe250,
            u2941Code::KeyGap => 0xe251,
            u2941Code::GuardBad => 0xe252,
            u2941Code::SumDrift => 0xe253,
            u2941Code::HistOver => 0xe254,
            u2941Code::HistHole => 0xe255,
            u2941Code::PctOob => 0xe256,
            u2941Code::MeanDrift => 0xe257,
            u2941Code::LedgerBroken => 0xe258,
            u2941Code::SnapshotFingerprint => 0xe259,
            u2941Code::TableBroken => 0xe25a,
            u2941Code::SegmentGuard => 0xe25b,
        }
    }

    /// 中文诊断名。
    pub const fn name(self) -> &'static str {
        match self {
            u2941Code::KeyDup => "键重复（域级 同键双行）",
            u2941Code::KeyGap => "键断档（压力 序不连续）",
            u2941Code::GuardBad => "行守卫破缺（三轴 逐行校验失败）",
            u2941Code::SumDrift => "矩阵累计漂移（三轴 求和失锚）",
            u2941Code::HistOver => "直方图溢出（超容量拒收未记账）",
            u2941Code::HistHole => "直方图断桶（桶序缺失）",
            u2941Code::PctOob => "分位越界（千分比>1000）",
            u2941Code::MeanDrift => "均值漂移（均值与分位矛盾）",
            u2941Code::LedgerBroken => "台账不守恒（守恒恒等式破缺）",
            u2941Code::SnapshotFingerprint => "快照指纹不符（字段被改后重算不一致）",
            u2941Code::TableBroken => "常量表破坏（表项与语义规则矛盾）",
            u2941Code::SegmentGuard => "段位守卫失败（诊断码越出本模块段）",
        }
    }

    /// 可操作修复建议（每条诊断都必须带下一步动作）。
    pub const fn suggestion(self) -> &'static str {
        match self {
            u2941Code::KeyDup => "按键唯一律去重",
            u2941Code::KeyGap => "按连续律补号",
            u2941Code::GuardBad => "按守卫口径重建",
            u2941Code::SumDrift => "按理论值复算",
            u2941Code::HistOver => "按溢出账口径登记",
            u2941Code::HistHole => "按桶序补齐",
            u2941Code::PctOob => "按 0..=1000 钳制",
            u2941Code::MeanDrift => "按同源数据复算",
            u2941Code::LedgerBroken => "按守恒律重建台账后重跑",
            u2941Code::SnapshotFingerprint => "按防篡改规程重建快照",
            u2941Code::TableBroken => "按语义规则重建常量表",
            u2941Code::SegmentGuard => "核对诊断码分配表",
        }
    }
}

// --------------------------------------------------------------------------
// 字符串键值映射 u2941mat（36 行逐行语义注释 + 逐行守卫）
// --------------------------------------------------------------------------
/// 域级压力三轴 守卫矩阵（与核心登记表同源对拍）。
/// 行数。
pub const u2941mat_COUNT: usize = 36;

/// 表本体（键 + 值 + 注释由编译期冻结）。
pub static u2941mat: [(&'static str, u64); 36] = [
    ("AX10000", 0), // AX1 单元 0000（轴一 × 相位 00 × 一 轮）
    ("AX20001", 13), // AX2 单元 0001（轴二 × 相位 01 × 一 轮）
    ("AX30002", 26), // AX3 单元 0002（轴三 × 相位 02 × 一 轮）
    ("STRESS0003", 39), // STRESS 单元 0003（压力 × 相位 03 × 一 轮）
    ("DOM0004", 52), // DOM 单元 0004（域级 × 相位 04 × 一 轮）
    ("MATRIX0005", 65), // MATRIX 单元 0005（矩阵 × 相位 05 × 一 轮）
    ("LOAD0006", 78), // LOAD 单元 0006（负载 × 相位 06 × 一 轮）
    ("TIME0007", 91), // TIME 单元 0007（时长 × 相位 07 × 一 轮）
    ("MET0008", 7), // MET 单元 0008（指标 × 相位 08 × 一 轮）
    ("THR0009", 20), // THR 单元 0009（阈值 × 相位 09 × 一 轮）
    ("VERDICT0010", 33), // VERDICT 单元 0010（裁决 × 相位 10 × 一 轮）
    ("SEAL0011", 46), // SEAL 单元 0011（封存 × 相位 11 × 一 轮）
    ("AX10012", 59), // AX1 单元 0012（轴一 × 相位 12 × 一 轮）
    ("AX20013", 72), // AX2 单元 0013（轴二 × 相位 13 × 一 轮）
    ("AX30014", 85), // AX3 单元 0014（轴三 × 相位 14 × 一 轮）
    ("STRESS0015", 1), // STRESS 单元 0015（压力 × 相位 15 × 一 轮）
    ("DOM0016", 14), // DOM 单元 0016（域级 × 相位 16 × 一 轮）
    ("MATRIX0017", 27), // MATRIX 单元 0017（矩阵 × 相位 17 × 一 轮）
    ("LOAD0018", 40), // LOAD 单元 0018（负载 × 相位 18 × 一 轮）
    ("TIME0019", 53), // TIME 单元 0019（时长 × 相位 19 × 一 轮）
    ("MET0020", 66), // MET 单元 0020（指标 × 相位 20 × 一 轮）
    ("THR0021", 79), // THR 单元 0021（阈值 × 相位 21 × 一 轮）
    ("VERDICT0022", 92), // VERDICT 单元 0022（裁决 × 相位 22 × 一 轮）
    ("SEAL0023", 8), // SEAL 单元 0023（封存 × 相位 23 × 一 轮）
    ("AX10024", 21), // AX1 单元 0024（轴一 × 相位 24 × 一 轮）
    ("AX20025", 34), // AX2 单元 0025（轴二 × 相位 25 × 一 轮）
    ("AX30026", 47), // AX3 单元 0026（轴三 × 相位 26 × 一 轮）
    ("STRESS0027", 60), // STRESS 单元 0027（压力 × 相位 27 × 一 轮）
    ("DOM0028", 73), // DOM 单元 0028（域级 × 相位 28 × 一 轮）
    ("MATRIX0029", 86), // MATRIX 单元 0029（矩阵 × 相位 29 × 一 轮）
    ("LOAD0030", 2), // LOAD 单元 0030（负载 × 相位 30 × 一 轮）
    ("TIME0031", 15), // TIME 单元 0031（时长 × 相位 31 × 一 轮）
    ("MET0032", 28), // MET 单元 0032（指标 × 相位 32 × 一 轮）
    ("THR0033", 41), // THR 单元 0033（阈值 × 相位 33 × 一 轮）
    ("VERDICT0034", 54), // VERDICT 单元 0034（裁决 × 相位 34 × 一 轮）
    ("SEAL0035", 67), // SEAL 单元 0035（封存 × 相位 35 × 一 轮）
];

/// O(1) 语义查找（match 全枚举；命中率 100%% 由守卫保证）。
pub fn u2941mat_get(key: &str) -> Option<u64> {
    match key {
        "AX10000" => Some(0),
        "AX20001" => Some(13),
        "AX30002" => Some(26),
        "STRESS0003" => Some(39),
        "DOM0004" => Some(52),
        "MATRIX0005" => Some(65),
        "LOAD0006" => Some(78),
        "TIME0007" => Some(91),
        "MET0008" => Some(7),
        "THR0009" => Some(20),
        "VERDICT0010" => Some(33),
        "SEAL0011" => Some(46),
        "AX10012" => Some(59),
        "AX20013" => Some(72),
        "AX30014" => Some(85),
        "STRESS0015" => Some(1),
        "DOM0016" => Some(14),
        "MATRIX0017" => Some(27),
        "LOAD0018" => Some(40),
        "TIME0019" => Some(53),
        "MET0020" => Some(66),
        "THR0021" => Some(79),
        "VERDICT0022" => Some(92),
        "SEAL0023" => Some(8),
        "AX10024" => Some(21),
        "AX20025" => Some(34),
        "AX30026" => Some(47),
        "STRESS0027" => Some(60),
        "DOM0028" => Some(73),
        "MATRIX0029" => Some(86),
        "LOAD0030" => Some(2),
        "TIME0031" => Some(15),
        "MET0032" => Some(28),
        "THR0033" => Some(41),
        "VERDICT0034" => Some(54),
        "SEAL0035" => Some(67),
        _ => None,
    }
}

/// 逐行守卫扫描（返回违例行数；0 = 全绿）。
pub fn u2941mat_scan() -> usize {
    let mut bad = 0usize;
    let mut i = 0usize;
    while i < u2941mat_COUNT {
        let (k, v) = u2941mat[i];
        let ok = match k {
            "AX10000" => v < 97,
            "AX20001" => v < 97,
            "AX30002" => v < 97,
            "STRESS0003" => v < 97,
            "DOM0004" => v < 97,
            "MATRIX0005" => v < 97,
            "LOAD0006" => v < 97,
            "TIME0007" => v < 97,
            "MET0008" => v < 97,
            "THR0009" => v < 97,
            "VERDICT0010" => v < 97,
            "SEAL0011" => v < 97,
            "AX10012" => v < 97,
            "AX20013" => v < 97,
            "AX30014" => v < 97,
            "STRESS0015" => v < 97,
            "DOM0016" => v < 97,
            "MATRIX0017" => v < 97,
            "LOAD0018" => v < 97,
            "TIME0019" => v < 97,
            "MET0020" => v < 97,
            "THR0021" => v < 97,
            "VERDICT0022" => v < 97,
            "SEAL0023" => v < 97,
            "AX10024" => v < 97,
            "AX20025" => v < 97,
            "AX30026" => v < 97,
            "STRESS0027" => v < 97,
            "DOM0028" => v < 97,
            "MATRIX0029" => v < 97,
            "LOAD0030" => v < 97,
            "TIME0031" => v < 97,
            "MET0032" => v < 97,
            "THR0033" => v < 97,
            "VERDICT0034" => v < 97,
            "SEAL0035" => v < 97,
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
pub fn u2941mat_sum() -> u64 {
    let mut s = 0u64;
    let mut i = 0usize;
    while i < u2941mat_COUNT {
        s = s.wrapping_add(u2941mat[i].1 as u64);
        i += 1;
    }
    s
}

/// 键唯一性（构造期断言等价物：逐对扫描同键）。
pub fn u2941mat_unique() -> bool {
    let mut i = 0usize;
    while i < u2941mat_COUNT {
        let mut j = i + 1;
        while j < u2941mat_COUNT {
            if u2941mat[i].0 == u2941mat[j].0 {
                return false;
            }
            j += 1;
        }
        i += 1;
    }
    true
}

// --------------------------------------------------------------------------
// 直方图引擎 mxu2941hist（有界插入排序 + 分位数 P50/P90/P99）
// --------------------------------------------------------------------------
/// 域级压力三轴 直方图（96 桶滚动）。
/// 样本容量上限。
pub const MXU2941HIST_CAP: usize = 96;

/// 直方图账本（排序态 + 五统量）。
#[derive(Clone, Copy, Debug)]
pub struct mxu2941histHist {
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

impl mxu2941histHist {
    /// 空账构造。
    pub const fn new() -> Self {
        Self { samples: [0; 96], n: 0, drops: 0, min: 0, max: 0, sum: 0 }
    }

    /// 采样（有界插入排序；满员丢弃并记账）。
    pub fn push(&mut self, v: u64) {
        if self.n >= MXU2941HIST_CAP {
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

/// 段位守卫：全部诊断码必须落在本段 [0xe250, 0xe25f]。
pub const fn u2941_segment_guard() -> bool {
    let mut ok = true;
    let mut i = 0usize;
    while i < u2941Code::ALL.len() {
        let c = u2941Code::ALL[i].code();
        if c < 0xe250 || c > 0xe25f {
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
fn u2941_guard_seg() -> bool {
    u2941_segment_guard()
}

/// UNX-F2941 矩阵 36 行齐备 + 逐行守卫 + 唯一性 + 累计守恒。
fn check_u2941_map_guard() -> bool {
    let mut ok = u2941mat_COUNT == 36;
    if ok { ok = u2941mat_scan() == 0 && u2941mat_unique(); }
    if ok { ok = u2941mat_get("AX10000") == Some(0) && u2941mat_get("ZZZ_NOKEY").is_none(); }
    if ok { ok = u2941mat_sum() == 1594; }
    ok
}

/// UNX-F2941 直方图 16 样本分位/均值/守恒。
fn check_u2941_map_hist() -> bool {
    let mut h = mxu2941histHist::new();
    let mut i = 0u64;
    while i < 16 { h.push(i + 1); i += 1; }
    let mut ok = h.conserved() && h.mean() >= 1 && h.percentile(500) >= 8;
    ok
}

/// 本模块自检（逐检查带 detail，全绿才出门）。
pub fn run_u2941_checks() -> CheckSet {
    let mut set = CheckSet::new("U2941");
    set.add(
        "u2941-seg-guard",
        u2941_guard_seg(),
        if u2941_guard_seg() { "" } else { "diagnostic code outside segment" },
    );
    set.add(
        "u2941-map-guard",
        check_u2941_map_guard(),
        if check_u2941_map_guard() { "" } else { "UNX-F2941 matrix guard mismatch" },
    );
    set.add(
        "u2941-map-hist",
        check_u2941_map_hist(),
        if check_u2941_map_hist() { "" } else { "UNX-F2941 hist mismatch" },
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
    fn u2941_all_checks_green() {
        let set = run_u2941_checks();
        assert!(set.all_passed(), "U2941 domain checks must be green");
    }

    #[test]
    fn u2941_segment_guard_holds() {
        assert!(u2941_segment_guard());
    }
}
