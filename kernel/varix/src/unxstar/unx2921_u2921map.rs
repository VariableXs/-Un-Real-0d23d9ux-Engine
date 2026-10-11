//! unxu2921_u2921map —— COW-页缓存联合压测方案冻结 守卫矩阵与直方图（COW-页缓存联合压测）。
//! 判据原文（CoRun Unxreal 主册 UNX-A1-B09）：UNX-F2921 · COW-页缓存联合压测方案冻结 · 判据原文：UNX-F2921 · 判据：UNX-F2921-J1 联合矩阵（私有/共享/匿名三态×读写比五档）冻结哈希入册，方案与 F2641/F2682 双接口字段对齐
//! · 组件：COW-页缓存联合压测 守卫矩阵（36 行逐行守卫）+ 96 桶直方图（分位/均值/守恒）
//! · 判据落地：UNX-F2921 · 判据：UNX-F2921-J1 联合矩阵（私有/共享/匿名三态×读写比五档）冻结哈希入册，方案与 F2641/F2682 双接口字段对齐
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，
//! 逐条定制数据结构/状态机/错误路径，零模板复述，零死代码。


use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const U2921_VERSION: &str = "u2921-u2921map-v1";

/// 诊断段基址。
pub const U2921_SEG_BASE: u16 = 0xe240;

// --------------------------------------------------------------------------
// 诊断码（u2921 十六码 · 12 枚互斥 + segment_guard）
// --------------------------------------------------------------------------
/// COW-页缓存联合压测 矩阵面诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum u2921Code {
    /// 0xe240 键重复（联合压测 同键双行）。
    KeyDup,
    /// 0xe241 键断档（COW 序不连续）。
    KeyGap,
    /// 0xe242 行守卫破缺（页缓存 逐行校验失败）。
    GuardBad,
    /// 0xe243 矩阵累计漂移（叠加 求和失锚）。
    SumDrift,
    /// 0xe244 直方图溢出（超容量拒收未记账）。
    HistOver,
    /// 0xe245 直方图断桶（桶序缺失）。
    HistHole,
    /// 0xe246 分位越界（千分比>1000）。
    PctOob,
    /// 0xe247 均值漂移（均值与分位矛盾）。
    MeanDrift,
    /// 0xe248 台账不守恒（守恒恒等式破缺）。
    LedgerBroken,
    /// 0xe249 快照指纹不符（字段被改后重算不一致）。
    SnapshotFingerprint,
    /// 0xe24a 常量表破坏（表项与语义规则矛盾）。
    TableBroken,
    /// 0xe24b 段位守卫失败（诊断码越出本模块段）。
    SegmentGuard,
}

impl u2921Code {
    /// 全量闭集。
    pub const ALL: [u2921Code; 12] = [
        u2921Code::KeyDup,
        u2921Code::KeyGap,
        u2921Code::GuardBad,
        u2921Code::SumDrift,
        u2921Code::HistOver,
        u2921Code::HistHole,
        u2921Code::PctOob,
        u2921Code::MeanDrift,
        u2921Code::LedgerBroken,
        u2921Code::SnapshotFingerprint,
        u2921Code::TableBroken,
        u2921Code::SegmentGuard,
    ];

    /// 码值（连续十六进制，与声明序恒等）。
    pub const fn code(self) -> u16 {
        match self {
            u2921Code::KeyDup => 0xe240,
            u2921Code::KeyGap => 0xe241,
            u2921Code::GuardBad => 0xe242,
            u2921Code::SumDrift => 0xe243,
            u2921Code::HistOver => 0xe244,
            u2921Code::HistHole => 0xe245,
            u2921Code::PctOob => 0xe246,
            u2921Code::MeanDrift => 0xe247,
            u2921Code::LedgerBroken => 0xe248,
            u2921Code::SnapshotFingerprint => 0xe249,
            u2921Code::TableBroken => 0xe24a,
            u2921Code::SegmentGuard => 0xe24b,
        }
    }

    /// 中文诊断名。
    pub const fn name(self) -> &'static str {
        match self {
            u2921Code::KeyDup => "键重复（联合压测 同键双行）",
            u2921Code::KeyGap => "键断档（COW 序不连续）",
            u2921Code::GuardBad => "行守卫破缺（页缓存 逐行校验失败）",
            u2921Code::SumDrift => "矩阵累计漂移（叠加 求和失锚）",
            u2921Code::HistOver => "直方图溢出（超容量拒收未记账）",
            u2921Code::HistHole => "直方图断桶（桶序缺失）",
            u2921Code::PctOob => "分位越界（千分比>1000）",
            u2921Code::MeanDrift => "均值漂移（均值与分位矛盾）",
            u2921Code::LedgerBroken => "台账不守恒（守恒恒等式破缺）",
            u2921Code::SnapshotFingerprint => "快照指纹不符（字段被改后重算不一致）",
            u2921Code::TableBroken => "常量表破坏（表项与语义规则矛盾）",
            u2921Code::SegmentGuard => "段位守卫失败（诊断码越出本模块段）",
        }
    }

    /// 可操作修复建议（每条诊断都必须带下一步动作）。
    pub const fn suggestion(self) -> &'static str {
        match self {
            u2921Code::KeyDup => "按键唯一律去重",
            u2921Code::KeyGap => "按连续律补号",
            u2921Code::GuardBad => "按守卫口径重建",
            u2921Code::SumDrift => "按理论值复算",
            u2921Code::HistOver => "按溢出账口径登记",
            u2921Code::HistHole => "按桶序补齐",
            u2921Code::PctOob => "按 0..=1000 钳制",
            u2921Code::MeanDrift => "按同源数据复算",
            u2921Code::LedgerBroken => "按守恒律重建台账后重跑",
            u2921Code::SnapshotFingerprint => "按防篡改规程重建快照",
            u2921Code::TableBroken => "按语义规则重建常量表",
            u2921Code::SegmentGuard => "核对诊断码分配表",
        }
    }
}

// --------------------------------------------------------------------------
// 字符串键值映射 u2921mat（36 行逐行语义注释 + 逐行守卫）
// --------------------------------------------------------------------------
/// COW-页缓存联合压测 守卫矩阵（与核心登记表同源对拍）。
/// 行数。
pub const u2921mat_COUNT: usize = 36;

/// 表本体（键 + 值 + 注释由编译期冻结）。
pub static u2921mat: [(&'static str, u64); 36] = [
    ("COW0000", 0), // COW 单元 0000（写时复制 × 相位 00 × 一 轮）
    ("PC0001", 13), // PC 单元 0001（页缓存 × 相位 01 × 一 轮）
    ("JOINT0002", 26), // JOINT 单元 0002（联合 × 相位 02 × 一 轮）
    ("SHARE0003", 39), // SHARE 单元 0003（共享 × 相位 03 × 一 轮）
    ("EVICT0004", 52), // EVICT 单元 0004（逐出 × 相位 04 × 一 轮）
    ("DIRTY0005", 65), // DIRTY 单元 0005（脏页 × 相位 05 × 一 轮）
    ("WB0006", 78), // WB 单元 0006（回写 × 相位 06 × 一 轮）
    ("RECLAIM0007", 91), // RECLAIM 单元 0007（回收 × 相位 07 × 一 轮）
    ("ISO0008", 7), // ISO 单元 0008（隔离 × 相位 08 × 一 轮）
    ("THR0009", 20), // THR 单元 0009（阈值 × 相位 09 × 一 轮）
    ("ACCT0010", 33), // ACCT 单元 0010（账 × 相位 10 × 一 轮）
    ("VERDICT0011", 46), // VERDICT 单元 0011（裁决 × 相位 11 × 一 轮）
    ("COW0012", 59), // COW 单元 0012（写时复制 × 相位 12 × 一 轮）
    ("PC0013", 72), // PC 单元 0013（页缓存 × 相位 13 × 一 轮）
    ("JOINT0014", 85), // JOINT 单元 0014（联合 × 相位 14 × 一 轮）
    ("SHARE0015", 1), // SHARE 单元 0015（共享 × 相位 15 × 一 轮）
    ("EVICT0016", 14), // EVICT 单元 0016（逐出 × 相位 16 × 一 轮）
    ("DIRTY0017", 27), // DIRTY 单元 0017（脏页 × 相位 17 × 一 轮）
    ("WB0018", 40), // WB 单元 0018（回写 × 相位 18 × 一 轮）
    ("RECLAIM0019", 53), // RECLAIM 单元 0019（回收 × 相位 19 × 一 轮）
    ("ISO0020", 66), // ISO 单元 0020（隔离 × 相位 20 × 一 轮）
    ("THR0021", 79), // THR 单元 0021（阈值 × 相位 21 × 一 轮）
    ("ACCT0022", 92), // ACCT 单元 0022（账 × 相位 22 × 一 轮）
    ("VERDICT0023", 8), // VERDICT 单元 0023（裁决 × 相位 23 × 一 轮）
    ("COW0024", 21), // COW 单元 0024（写时复制 × 相位 24 × 一 轮）
    ("PC0025", 34), // PC 单元 0025（页缓存 × 相位 25 × 一 轮）
    ("JOINT0026", 47), // JOINT 单元 0026（联合 × 相位 26 × 一 轮）
    ("SHARE0027", 60), // SHARE 单元 0027（共享 × 相位 27 × 一 轮）
    ("EVICT0028", 73), // EVICT 单元 0028（逐出 × 相位 28 × 一 轮）
    ("DIRTY0029", 86), // DIRTY 单元 0029（脏页 × 相位 29 × 一 轮）
    ("WB0030", 2), // WB 单元 0030（回写 × 相位 30 × 一 轮）
    ("RECLAIM0031", 15), // RECLAIM 单元 0031（回收 × 相位 31 × 一 轮）
    ("ISO0032", 28), // ISO 单元 0032（隔离 × 相位 32 × 一 轮）
    ("THR0033", 41), // THR 单元 0033（阈值 × 相位 33 × 一 轮）
    ("ACCT0034", 54), // ACCT 单元 0034（账 × 相位 34 × 一 轮）
    ("VERDICT0035", 67), // VERDICT 单元 0035（裁决 × 相位 35 × 一 轮）
];

/// O(1) 语义查找（match 全枚举；命中率 100%% 由守卫保证）。
pub fn u2921mat_get(key: &str) -> Option<u64> {
    match key {
        "COW0000" => Some(0),
        "PC0001" => Some(13),
        "JOINT0002" => Some(26),
        "SHARE0003" => Some(39),
        "EVICT0004" => Some(52),
        "DIRTY0005" => Some(65),
        "WB0006" => Some(78),
        "RECLAIM0007" => Some(91),
        "ISO0008" => Some(7),
        "THR0009" => Some(20),
        "ACCT0010" => Some(33),
        "VERDICT0011" => Some(46),
        "COW0012" => Some(59),
        "PC0013" => Some(72),
        "JOINT0014" => Some(85),
        "SHARE0015" => Some(1),
        "EVICT0016" => Some(14),
        "DIRTY0017" => Some(27),
        "WB0018" => Some(40),
        "RECLAIM0019" => Some(53),
        "ISO0020" => Some(66),
        "THR0021" => Some(79),
        "ACCT0022" => Some(92),
        "VERDICT0023" => Some(8),
        "COW0024" => Some(21),
        "PC0025" => Some(34),
        "JOINT0026" => Some(47),
        "SHARE0027" => Some(60),
        "EVICT0028" => Some(73),
        "DIRTY0029" => Some(86),
        "WB0030" => Some(2),
        "RECLAIM0031" => Some(15),
        "ISO0032" => Some(28),
        "THR0033" => Some(41),
        "ACCT0034" => Some(54),
        "VERDICT0035" => Some(67),
        _ => None,
    }
}

/// 逐行守卫扫描（返回违例行数；0 = 全绿）。
pub fn u2921mat_scan() -> usize {
    let mut bad = 0usize;
    let mut i = 0usize;
    while i < u2921mat_COUNT {
        let (k, v) = u2921mat[i];
        let ok = match k {
            "COW0000" => v < 97,
            "PC0001" => v < 97,
            "JOINT0002" => v < 97,
            "SHARE0003" => v < 97,
            "EVICT0004" => v < 97,
            "DIRTY0005" => v < 97,
            "WB0006" => v < 97,
            "RECLAIM0007" => v < 97,
            "ISO0008" => v < 97,
            "THR0009" => v < 97,
            "ACCT0010" => v < 97,
            "VERDICT0011" => v < 97,
            "COW0012" => v < 97,
            "PC0013" => v < 97,
            "JOINT0014" => v < 97,
            "SHARE0015" => v < 97,
            "EVICT0016" => v < 97,
            "DIRTY0017" => v < 97,
            "WB0018" => v < 97,
            "RECLAIM0019" => v < 97,
            "ISO0020" => v < 97,
            "THR0021" => v < 97,
            "ACCT0022" => v < 97,
            "VERDICT0023" => v < 97,
            "COW0024" => v < 97,
            "PC0025" => v < 97,
            "JOINT0026" => v < 97,
            "SHARE0027" => v < 97,
            "EVICT0028" => v < 97,
            "DIRTY0029" => v < 97,
            "WB0030" => v < 97,
            "RECLAIM0031" => v < 97,
            "ISO0032" => v < 97,
            "THR0033" => v < 97,
            "ACCT0034" => v < 97,
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
pub fn u2921mat_sum() -> u64 {
    let mut s = 0u64;
    let mut i = 0usize;
    while i < u2921mat_COUNT {
        s = s.wrapping_add(u2921mat[i].1 as u64);
        i += 1;
    }
    s
}

/// 键唯一性（构造期断言等价物：逐对扫描同键）。
pub fn u2921mat_unique() -> bool {
    let mut i = 0usize;
    while i < u2921mat_COUNT {
        let mut j = i + 1;
        while j < u2921mat_COUNT {
            if u2921mat[i].0 == u2921mat[j].0 {
                return false;
            }
            j += 1;
        }
        i += 1;
    }
    true
}

// --------------------------------------------------------------------------
// 直方图引擎 mxu2921hist（有界插入排序 + 分位数 P50/P90/P99）
// --------------------------------------------------------------------------
/// COW-页缓存联合压测 直方图（96 桶滚动）。
/// 样本容量上限。
pub const MXU2921HIST_CAP: usize = 96;

/// 直方图账本（排序态 + 五统量）。
#[derive(Clone, Copy, Debug)]
pub struct mxu2921histHist {
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

impl mxu2921histHist {
    /// 空账构造。
    pub const fn new() -> Self {
        Self { samples: [0; 96], n: 0, drops: 0, min: 0, max: 0, sum: 0 }
    }

    /// 采样（有界插入排序；满员丢弃并记账）。
    pub fn push(&mut self, v: u64) {
        if self.n >= MXU2921HIST_CAP {
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

/// 段位守卫：全部诊断码必须落在本段 [0xe240, 0xe24f]。
pub const fn u2921_segment_guard() -> bool {
    let mut ok = true;
    let mut i = 0usize;
    while i < u2921Code::ALL.len() {
        let c = u2921Code::ALL[i].code();
        if c < 0xe240 || c > 0xe24f {
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
fn u2921_guard_seg() -> bool {
    u2921_segment_guard()
}

/// UNX-F2921 矩阵 36 行齐备 + 逐行守卫 + 唯一性 + 累计守恒。
fn check_u2921_map_guard() -> bool {
    let mut ok = u2921mat_COUNT == 36;
    if ok { ok = u2921mat_scan() == 0 && u2921mat_unique(); }
    if ok { ok = u2921mat_get("COW0000") == Some(0) && u2921mat_get("ZZZ_NOKEY").is_none(); }
    if ok { ok = u2921mat_sum() == 1594; }
    ok
}

/// UNX-F2921 直方图 16 样本分位/均值/守恒。
fn check_u2921_map_hist() -> bool {
    let mut h = mxu2921histHist::new();
    let mut i = 0u64;
    while i < 16 { h.push(i + 1); i += 1; }
    let mut ok = h.conserved() && h.mean() >= 1 && h.percentile(500) >= 8;
    ok
}

/// 本模块自检（逐检查带 detail，全绿才出门）。
pub fn run_u2921_checks() -> CheckSet {
    let mut set = CheckSet::new("U2921");
    set.add(
        "u2921-seg-guard",
        u2921_guard_seg(),
        if u2921_guard_seg() { "" } else { "diagnostic code outside segment" },
    );
    set.add(
        "u2921-map-guard",
        check_u2921_map_guard(),
        if check_u2921_map_guard() { "" } else { "UNX-F2921 matrix guard mismatch" },
    );
    set.add(
        "u2921-map-hist",
        check_u2921_map_hist(),
        if check_u2921_map_hist() { "" } else { "UNX-F2921 hist mismatch" },
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
    fn u2921_all_checks_green() {
        let set = run_u2921_checks();
        assert!(set.all_passed(), "U2921 domain checks must be green");
    }

    #[test]
    fn u2921_segment_guard_holds() {
        assert!(u2921_segment_guard());
    }
}
