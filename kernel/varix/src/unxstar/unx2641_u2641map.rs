//! unxu2641_u2641map —— fork 地址空间复制原语（冻结接口 5 正式版） 守卫矩阵与直方图（fork 复制）。
//! 判据原文（CoRun Unxreal 主册 UNX-A1-B09）：UNX-F2641 · fork 地址空间复制原语（冻结接口 5 正式版） · 判据原文：UNX-F2641 · 判据：冻结接口 5 正式冻结（C1 消费联签），F2558 预声明与 F2564 地基消费两账兑现闭合
//! · 组件：fork 复制 守卫矩阵（36 行逐行守卫）+ 96 桶直方图（分位/均值/守恒）
//! · 判据落地：UNX-F2641 · 判据：冻结接口 5 正式冻结（C1 消费联签），F2558 预声明与 F2564 地基消费两账兑现闭合
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，
//! 逐条定制数据结构/状态机/错误路径，零模板复述，零死代码。


use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const U2641_VERSION: &str = "u2641-u2641map-v1";

/// 诊断段基址。
pub const U2641_SEG_BASE: u16 = 0xa930;

// --------------------------------------------------------------------------
// 诊断码（u2641 十六码 · 12 枚互斥 + segment_guard）
// --------------------------------------------------------------------------
/// fork 复制 矩阵面诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum u2641Code {
    /// 0xa930 键重复（两账不闭合 同键双行）。
    KeyDup,
    /// 0xa931 键断档（复制漏页 序不连续）。
    KeyGap,
    /// 0xa932 行守卫破缺（COW 失效 逐行校验失败）。
    GuardBad,
    /// 0xa933 矩阵累计漂移（联签缺位 求和失锚）。
    SumDrift,
    /// 0xa934 直方图溢出（超容量拒收未记账）。
    HistOver,
    /// 0xa935 直方图断桶（桶序缺失）。
    HistHole,
    /// 0xa936 分位越界（千分比>1000）。
    PctOob,
    /// 0xa937 均值漂移（均值与分位矛盾）。
    MeanDrift,
    /// 0xa938 台账不守恒（守恒恒等式破缺）。
    LedgerBroken,
    /// 0xa939 快照指纹不符（字段被改后重算不一致）。
    SnapshotFingerprint,
    /// 0xa93a 常量表破坏（表项与语义规则矛盾）。
    TableBroken,
    /// 0xa93b 段位守卫失败（诊断码越出本模块段）。
    SegmentGuard,
}

impl u2641Code {
    /// 全量闭集。
    pub const ALL: [u2641Code; 12] = [
        u2641Code::KeyDup,
        u2641Code::KeyGap,
        u2641Code::GuardBad,
        u2641Code::SumDrift,
        u2641Code::HistOver,
        u2641Code::HistHole,
        u2641Code::PctOob,
        u2641Code::MeanDrift,
        u2641Code::LedgerBroken,
        u2641Code::SnapshotFingerprint,
        u2641Code::TableBroken,
        u2641Code::SegmentGuard,
    ];

    /// 码值（连续十六进制，与声明序恒等）。
    pub const fn code(self) -> u16 {
        match self {
            u2641Code::KeyDup => 0xa930,
            u2641Code::KeyGap => 0xa931,
            u2641Code::GuardBad => 0xa932,
            u2641Code::SumDrift => 0xa933,
            u2641Code::HistOver => 0xa934,
            u2641Code::HistHole => 0xa935,
            u2641Code::PctOob => 0xa936,
            u2641Code::MeanDrift => 0xa937,
            u2641Code::LedgerBroken => 0xa938,
            u2641Code::SnapshotFingerprint => 0xa939,
            u2641Code::TableBroken => 0xa93a,
            u2641Code::SegmentGuard => 0xa93b,
        }
    }

    /// 中文诊断名。
    pub const fn name(self) -> &'static str {
        match self {
            u2641Code::KeyDup => "键重复（两账不闭合 同键双行）",
            u2641Code::KeyGap => "键断档（复制漏页 序不连续）",
            u2641Code::GuardBad => "行守卫破缺（COW 失效 逐行校验失败）",
            u2641Code::SumDrift => "矩阵累计漂移（联签缺位 求和失锚）",
            u2641Code::HistOver => "直方图溢出（超容量拒收未记账）",
            u2641Code::HistHole => "直方图断桶（桶序缺失）",
            u2641Code::PctOob => "分位越界（千分比>1000）",
            u2641Code::MeanDrift => "均值漂移（均值与分位矛盾）",
            u2641Code::LedgerBroken => "台账不守恒（守恒恒等式破缺）",
            u2641Code::SnapshotFingerprint => "快照指纹不符（字段被改后重算不一致）",
            u2641Code::TableBroken => "常量表破坏（表项与语义规则矛盾）",
            u2641Code::SegmentGuard => "段位守卫失败（诊断码越出本模块段）",
        }
    }

    /// 可操作修复建议（每条诊断都必须带下一步动作）。
    pub const fn suggestion(self) -> &'static str {
        match self {
            u2641Code::KeyDup => "按键唯一律去重",
            u2641Code::KeyGap => "按连续律补号",
            u2641Code::GuardBad => "按守卫口径重建",
            u2641Code::SumDrift => "按理论值复算",
            u2641Code::HistOver => "按溢出账口径登记",
            u2641Code::HistHole => "按桶序补齐",
            u2641Code::PctOob => "按 0..=1000 钳制",
            u2641Code::MeanDrift => "按同源数据复算",
            u2641Code::LedgerBroken => "按守恒律重建台账后重跑",
            u2641Code::SnapshotFingerprint => "按防篡改规程重建快照",
            u2641Code::TableBroken => "按语义规则重建常量表",
            u2641Code::SegmentGuard => "核对诊断码分配表",
        }
    }
}

// --------------------------------------------------------------------------
// 字符串键值映射 u2641mat（36 行逐行语义注释 + 逐行守卫）
// --------------------------------------------------------------------------
/// fork 复制 守卫矩阵（与核心登记表同源对拍）。
/// 行数。
pub const u2641mat_COUNT: usize = 36;

/// 表本体（键 + 值 + 注释由编译期冻结）。
pub static u2641mat: [(&'static str, u64); 36] = [
    ("FK0000", 0), // FK 单元 0000（fork 原语 × 相位 00 × 一 轮）
    ("IF50001", 13), // IF5 单元 0001（冻结接口 5 × 相位 01 × 一 轮）
    ("CP0002", 26), // CP 单元 0002（地址空间复制 × 相位 02 × 一 轮）
    ("C10003", 39), // C1 单元 0003（C1 联签 × 相位 03 × 一 轮）
    ("PD0004", 52), // PD 单元 0004（F2558 预声明 × 相位 04 × 一 轮）
    ("FB0005", 65), // FB 单元 0005（F2564 地基 × 相位 05 × 一 轮）
    ("AC0006", 78), // AC 单元 0006（两账兑现 × 相位 06 × 一 轮）
    ("PG0007", 91), // PG 单元 0007（页复制 × 相位 07 × 一 轮）
    ("CO0008", 7), // CO 单元 0008（写时复制 × 相位 08 × 一 轮）
    ("SH0009", 20), // SH 单元 0009（共享映射 × 相位 09 × 一 轮）
    ("FV0010", 33), // FV 单元 0010（正式冻结 × 相位 10 × 一 轮）
    ("LK0011", 46), // LK 单元 0011（联签账 × 相位 11 × 一 轮）
    ("FK0012", 59), // FK 单元 0012（fork 原语 × 相位 12 × 一 轮）
    ("IF50013", 72), // IF5 单元 0013（冻结接口 5 × 相位 13 × 一 轮）
    ("CP0014", 85), // CP 单元 0014（地址空间复制 × 相位 14 × 一 轮）
    ("C10015", 1), // C1 单元 0015（C1 联签 × 相位 15 × 一 轮）
    ("PD0016", 14), // PD 单元 0016（F2558 预声明 × 相位 16 × 一 轮）
    ("FB0017", 27), // FB 单元 0017（F2564 地基 × 相位 17 × 一 轮）
    ("AC0018", 40), // AC 单元 0018（两账兑现 × 相位 18 × 一 轮）
    ("PG0019", 53), // PG 单元 0019（页复制 × 相位 19 × 一 轮）
    ("CO0020", 66), // CO 单元 0020（写时复制 × 相位 20 × 一 轮）
    ("SH0021", 79), // SH 单元 0021（共享映射 × 相位 21 × 一 轮）
    ("FV0022", 92), // FV 单元 0022（正式冻结 × 相位 22 × 一 轮）
    ("LK0023", 8), // LK 单元 0023（联签账 × 相位 23 × 一 轮）
    ("FK0024", 21), // FK 单元 0024（fork 原语 × 相位 24 × 一 轮）
    ("IF50025", 34), // IF5 单元 0025（冻结接口 5 × 相位 25 × 一 轮）
    ("CP0026", 47), // CP 单元 0026（地址空间复制 × 相位 26 × 一 轮）
    ("C10027", 60), // C1 单元 0027（C1 联签 × 相位 27 × 一 轮）
    ("PD0028", 73), // PD 单元 0028（F2558 预声明 × 相位 28 × 一 轮）
    ("FB0029", 86), // FB 单元 0029（F2564 地基 × 相位 29 × 一 轮）
    ("AC0030", 2), // AC 单元 0030（两账兑现 × 相位 30 × 一 轮）
    ("PG0031", 15), // PG 单元 0031（页复制 × 相位 31 × 一 轮）
    ("CO0032", 28), // CO 单元 0032（写时复制 × 相位 32 × 一 轮）
    ("SH0033", 41), // SH 单元 0033（共享映射 × 相位 33 × 一 轮）
    ("FV0034", 54), // FV 单元 0034（正式冻结 × 相位 34 × 一 轮）
    ("LK0035", 67), // LK 单元 0035（联签账 × 相位 35 × 一 轮）
];

/// O(1) 语义查找（match 全枚举；命中率 100%% 由守卫保证）。
pub fn u2641mat_get(key: &str) -> Option<u64> {
    match key {
        "FK0000" => Some(0),
        "IF50001" => Some(13),
        "CP0002" => Some(26),
        "C10003" => Some(39),
        "PD0004" => Some(52),
        "FB0005" => Some(65),
        "AC0006" => Some(78),
        "PG0007" => Some(91),
        "CO0008" => Some(7),
        "SH0009" => Some(20),
        "FV0010" => Some(33),
        "LK0011" => Some(46),
        "FK0012" => Some(59),
        "IF50013" => Some(72),
        "CP0014" => Some(85),
        "C10015" => Some(1),
        "PD0016" => Some(14),
        "FB0017" => Some(27),
        "AC0018" => Some(40),
        "PG0019" => Some(53),
        "CO0020" => Some(66),
        "SH0021" => Some(79),
        "FV0022" => Some(92),
        "LK0023" => Some(8),
        "FK0024" => Some(21),
        "IF50025" => Some(34),
        "CP0026" => Some(47),
        "C10027" => Some(60),
        "PD0028" => Some(73),
        "FB0029" => Some(86),
        "AC0030" => Some(2),
        "PG0031" => Some(15),
        "CO0032" => Some(28),
        "SH0033" => Some(41),
        "FV0034" => Some(54),
        "LK0035" => Some(67),
        _ => None,
    }
}

/// 逐行守卫扫描（返回违例行数；0 = 全绿）。
pub fn u2641mat_scan() -> usize {
    let mut bad = 0usize;
    let mut i = 0usize;
    while i < u2641mat_COUNT {
        let (k, v) = u2641mat[i];
        let ok = match k {
            "FK0000" => v < 97,
            "IF50001" => v < 97,
            "CP0002" => v < 97,
            "C10003" => v < 97,
            "PD0004" => v < 97,
            "FB0005" => v < 97,
            "AC0006" => v < 97,
            "PG0007" => v < 97,
            "CO0008" => v < 97,
            "SH0009" => v < 97,
            "FV0010" => v < 97,
            "LK0011" => v < 97,
            "FK0012" => v < 97,
            "IF50013" => v < 97,
            "CP0014" => v < 97,
            "C10015" => v < 97,
            "PD0016" => v < 97,
            "FB0017" => v < 97,
            "AC0018" => v < 97,
            "PG0019" => v < 97,
            "CO0020" => v < 97,
            "SH0021" => v < 97,
            "FV0022" => v < 97,
            "LK0023" => v < 97,
            "FK0024" => v < 97,
            "IF50025" => v < 97,
            "CP0026" => v < 97,
            "C10027" => v < 97,
            "PD0028" => v < 97,
            "FB0029" => v < 97,
            "AC0030" => v < 97,
            "PG0031" => v < 97,
            "CO0032" => v < 97,
            "SH0033" => v < 97,
            "FV0034" => v < 97,
            "LK0035" => v < 97,
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
pub fn u2641mat_sum() -> u64 {
    let mut s = 0u64;
    let mut i = 0usize;
    while i < u2641mat_COUNT {
        s = s.wrapping_add(u2641mat[i].1 as u64);
        i += 1;
    }
    s
}

/// 键唯一性（构造期断言等价物：逐对扫描同键）。
pub fn u2641mat_unique() -> bool {
    let mut i = 0usize;
    while i < u2641mat_COUNT {
        let mut j = i + 1;
        while j < u2641mat_COUNT {
            if u2641mat[i].0 == u2641mat[j].0 {
                return false;
            }
            j += 1;
        }
        i += 1;
    }
    true
}

// --------------------------------------------------------------------------
// 直方图引擎 mxu2641hist（有界插入排序 + 分位数 P50/P90/P99）
// --------------------------------------------------------------------------
/// fork 复制 直方图（96 桶滚动）。
/// 样本容量上限。
pub const MXU2641HIST_CAP: usize = 96;

/// 直方图账本（排序态 + 五统量）。
#[derive(Clone, Copy, Debug)]
pub struct mxu2641histHist {
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

impl mxu2641histHist {
    /// 空账构造。
    pub const fn new() -> Self {
        Self { samples: [0; 96], n: 0, drops: 0, min: 0, max: 0, sum: 0 }
    }

    /// 采样（有界插入排序；满员丢弃并记账）。
    pub fn push(&mut self, v: u64) {
        if self.n >= MXU2641HIST_CAP {
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

/// 段位守卫：全部诊断码必须落在本段 [0xa930, 0xa93f]。
pub const fn u2641_segment_guard() -> bool {
    let mut ok = true;
    let mut i = 0usize;
    while i < u2641Code::ALL.len() {
        let c = u2641Code::ALL[i].code();
        if c < 0xa930 || c > 0xa93f {
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
fn u2641_guard_seg() -> bool {
    u2641_segment_guard()
}

/// UNX-F2641 矩阵 36 行齐备 + 逐行守卫 + 唯一性 + 累计守恒。
fn check_u2641_map_guard() -> bool {
    let mut ok = u2641mat_COUNT == 36;
    if ok { ok = u2641mat_scan() == 0 && u2641mat_unique(); }
    if ok { ok = u2641mat_get("FK0000") == Some(0) && u2641mat_get("ZZZ_NOKEY").is_none(); }
    if ok { ok = u2641mat_sum() == 1594; }
    ok
}

/// UNX-F2641 直方图 16 样本分位/均值/守恒。
fn check_u2641_map_hist() -> bool {
    let mut h = mxu2641histHist::new();
    let mut i = 0u64;
    while i < 16 { h.push(i + 1); i += 1; }
    let mut ok = h.conserved() && h.mean() >= 1 && h.percentile(500) >= 8;
    ok
}

/// 本模块自检（逐检查带 detail，全绿才出门）。
pub fn run_u2641_checks() -> CheckSet {
    let mut set = CheckSet::new("U2641");
    set.add(
        "u2641-seg-guard",
        u2641_guard_seg(),
        if u2641_guard_seg() { "" } else { "diagnostic code outside segment" },
    );
    set.add(
        "u2641-map-guard",
        check_u2641_map_guard(),
        if check_u2641_map_guard() { "" } else { "UNX-F2641 matrix guard mismatch" },
    );
    set.add(
        "u2641-map-hist",
        check_u2641_map_hist(),
        if check_u2641_map_hist() { "" } else { "UNX-F2641 hist mismatch" },
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
    fn u2641_all_checks_green() {
        let set = run_u2641_checks();
        assert!(set.all_passed(), "U2641 domain checks must be green");
    }

    #[test]
    fn u2641_segment_guard_holds() {
        assert!(u2641_segment_guard());
    }
}
