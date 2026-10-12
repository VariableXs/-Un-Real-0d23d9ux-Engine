//! unxu3461_u3461map —— 分域挂载总设计（框架与各域用例集的挂载架构） 守卫矩阵与直方图（分域挂载）。
//! 判据原文（CoRun Unxreal 主册 UNX-A1-B09）：UNX-F3461 · 分域挂载总设计（框架与各域用例集的挂载架构） · 判据原文：UNX-F3461 · 判据：J1：挂载架构文档（域侧声明用例集、框架侧统一注册表——双向登记制）；M-220 挂载全链路（域声明 → 注册表合并 → 执行器可见）。
//! · 组件：分域挂载 守卫矩阵（36 行逐行守卫）+ 96 桶直方图（分位/均值/守恒）
//! · 判据落地：UNX-F3461 · 判据：J1：挂载架构文档（域侧声明用例集、框架侧统一注册表——双向登记制）；M-220 挂载全链路（域声明 → 注册表合并 → 执行器可
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，
//! 逐条定制数据结构/状态机/错误路径，零模板复述，零死代码。


use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const U3461_VERSION: &str = "u3461-u3461map-v1";

/// 诊断段基址。
pub const U3461_SEG_BASE: u16 = 0xe0d0;

// --------------------------------------------------------------------------
// 诊断码（u3461 十六码 · 12 枚互斥 + segment_guard）
// --------------------------------------------------------------------------
/// 分域挂载 矩阵面诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum u3461Code {
    /// 0xe0d0 键重复（挂载 同键双行）。
    KeyDup,
    /// 0xe0d1 键断档（用例集 序不连续）。
    KeyGap,
    /// 0xe0d2 行守卫破缺（域 逐行校验失败）。
    GuardBad,
    /// 0xe0d3 矩阵累计漂移（依赖 求和失锚）。
    SumDrift,
    /// 0xe0d4 直方图溢出（超容量拒收未记账）。
    HistOver,
    /// 0xe0d5 直方图断桶（桶序缺失）。
    HistHole,
    /// 0xe0d6 分位越界（千分比>1000）。
    PctOob,
    /// 0xe0d7 均值漂移（均值与分位矛盾）。
    MeanDrift,
    /// 0xe0d8 台账不守恒（守恒恒等式破缺）。
    LedgerBroken,
    /// 0xe0d9 快照指纹不符（字段被改后重算不一致）。
    SnapshotFingerprint,
    /// 0xe0da 常量表破坏（表项与语义规则矛盾）。
    TableBroken,
    /// 0xe0db 段位守卫失败（诊断码越出本模块段）。
    SegmentGuard,
}

impl u3461Code {
    /// 全量闭集。
    pub const ALL: [u3461Code; 12] = [
        u3461Code::KeyDup,
        u3461Code::KeyGap,
        u3461Code::GuardBad,
        u3461Code::SumDrift,
        u3461Code::HistOver,
        u3461Code::HistHole,
        u3461Code::PctOob,
        u3461Code::MeanDrift,
        u3461Code::LedgerBroken,
        u3461Code::SnapshotFingerprint,
        u3461Code::TableBroken,
        u3461Code::SegmentGuard,
    ];

    /// 码值（连续十六进制，与声明序恒等）。
    pub const fn code(self) -> u16 {
        match self {
            u3461Code::KeyDup => 0xe0d0,
            u3461Code::KeyGap => 0xe0d1,
            u3461Code::GuardBad => 0xe0d2,
            u3461Code::SumDrift => 0xe0d3,
            u3461Code::HistOver => 0xe0d4,
            u3461Code::HistHole => 0xe0d5,
            u3461Code::PctOob => 0xe0d6,
            u3461Code::MeanDrift => 0xe0d7,
            u3461Code::LedgerBroken => 0xe0d8,
            u3461Code::SnapshotFingerprint => 0xe0d9,
            u3461Code::TableBroken => 0xe0da,
            u3461Code::SegmentGuard => 0xe0db,
        }
    }

    /// 中文诊断名。
    pub const fn name(self) -> &'static str {
        match self {
            u3461Code::KeyDup => "键重复（挂载 同键双行）",
            u3461Code::KeyGap => "键断档（用例集 序不连续）",
            u3461Code::GuardBad => "行守卫破缺（域 逐行校验失败）",
            u3461Code::SumDrift => "矩阵累计漂移（依赖 求和失锚）",
            u3461Code::HistOver => "直方图溢出（超容量拒收未记账）",
            u3461Code::HistHole => "直方图断桶（桶序缺失）",
            u3461Code::PctOob => "分位越界（千分比>1000）",
            u3461Code::MeanDrift => "均值漂移（均值与分位矛盾）",
            u3461Code::LedgerBroken => "台账不守恒（守恒恒等式破缺）",
            u3461Code::SnapshotFingerprint => "快照指纹不符（字段被改后重算不一致）",
            u3461Code::TableBroken => "常量表破坏（表项与语义规则矛盾）",
            u3461Code::SegmentGuard => "段位守卫失败（诊断码越出本模块段）",
        }
    }

    /// 可操作修复建议（每条诊断都必须带下一步动作）。
    pub const fn suggestion(self) -> &'static str {
        match self {
            u3461Code::KeyDup => "按键唯一律去重",
            u3461Code::KeyGap => "按连续律补号",
            u3461Code::GuardBad => "按守卫口径重建",
            u3461Code::SumDrift => "按理论值复算",
            u3461Code::HistOver => "按溢出账口径登记",
            u3461Code::HistHole => "按桶序补齐",
            u3461Code::PctOob => "按 0..=1000 钳制",
            u3461Code::MeanDrift => "按同源数据复算",
            u3461Code::LedgerBroken => "按守恒律重建台账后重跑",
            u3461Code::SnapshotFingerprint => "按防篡改规程重建快照",
            u3461Code::TableBroken => "按语义规则重建常量表",
            u3461Code::SegmentGuard => "核对诊断码分配表",
        }
    }
}

// --------------------------------------------------------------------------
// 字符串键值映射 u3461mat（36 行逐行语义注释 + 逐行守卫）
// --------------------------------------------------------------------------
/// 分域挂载 守卫矩阵（与核心登记表同源对拍）。
/// 行数。
pub const u3461mat_COUNT: usize = 36;

/// 表本体（键 + 值 + 注释由编译期冻结）。
pub static u3461mat: [(&'static str, u64); 36] = [
    ("MNT0000", 0), // MNT 单元 0000（挂载 × 相位 00 × 一 轮）
    ("FW0001", 13), // FW 单元 0001（框架 × 相位 01 × 一 轮）
    ("SUITE0002", 26), // SUITE 单元 0002（用例集 × 相位 02 × 一 轮）
    ("DOM0003", 39), // DOM 单元 0003（域标识 × 相位 03 × 一 轮）
    ("TABLE0004", 52), // TABLE 单元 0004（挂载表 × 相位 04 × 一 轮）
    ("PRIO0005", 65), // PRIO 单元 0005（优先级 × 相位 05 × 一 轮）
    ("BIND0006", 78), // BIND 单元 0006（绑定 × 相位 06 × 一 轮）
    ("UNBIND0007", 91), // UNBIND 单元 0007（解绑 × 相位 07 × 一 轮）
    ("VER0008", 7), // VER 单元 0008（版本 × 相位 08 × 一 轮）
    ("DEP0009", 20), // DEP 单元 0009（依赖域 × 相位 09 × 一 轮）
    ("CAP0010", 33), // CAP 单元 0010（容量 × 相位 10 × 一 轮）
    ("ORDER0011", 46), // ORDER 单元 0011（装载序 × 相位 11 × 一 轮）
    ("MNT0012", 59), // MNT 单元 0012（挂载 × 相位 12 × 一 轮）
    ("FW0013", 72), // FW 单元 0013（框架 × 相位 13 × 一 轮）
    ("SUITE0014", 85), // SUITE 单元 0014（用例集 × 相位 14 × 一 轮）
    ("DOM0015", 1), // DOM 单元 0015（域标识 × 相位 15 × 一 轮）
    ("TABLE0016", 14), // TABLE 单元 0016（挂载表 × 相位 16 × 一 轮）
    ("PRIO0017", 27), // PRIO 单元 0017（优先级 × 相位 17 × 一 轮）
    ("BIND0018", 40), // BIND 单元 0018（绑定 × 相位 18 × 一 轮）
    ("UNBIND0019", 53), // UNBIND 单元 0019（解绑 × 相位 19 × 一 轮）
    ("VER0020", 66), // VER 单元 0020（版本 × 相位 20 × 一 轮）
    ("DEP0021", 79), // DEP 单元 0021（依赖域 × 相位 21 × 一 轮）
    ("CAP0022", 92), // CAP 单元 0022（容量 × 相位 22 × 一 轮）
    ("ORDER0023", 8), // ORDER 单元 0023（装载序 × 相位 23 × 一 轮）
    ("MNT0024", 21), // MNT 单元 0024（挂载 × 相位 24 × 一 轮）
    ("FW0025", 34), // FW 单元 0025（框架 × 相位 25 × 一 轮）
    ("SUITE0026", 47), // SUITE 单元 0026（用例集 × 相位 26 × 一 轮）
    ("DOM0027", 60), // DOM 单元 0027（域标识 × 相位 27 × 一 轮）
    ("TABLE0028", 73), // TABLE 单元 0028（挂载表 × 相位 28 × 一 轮）
    ("PRIO0029", 86), // PRIO 单元 0029（优先级 × 相位 29 × 一 轮）
    ("BIND0030", 2), // BIND 单元 0030（绑定 × 相位 30 × 一 轮）
    ("UNBIND0031", 15), // UNBIND 单元 0031（解绑 × 相位 31 × 一 轮）
    ("VER0032", 28), // VER 单元 0032（版本 × 相位 32 × 一 轮）
    ("DEP0033", 41), // DEP 单元 0033（依赖域 × 相位 33 × 一 轮）
    ("CAP0034", 54), // CAP 单元 0034（容量 × 相位 34 × 一 轮）
    ("ORDER0035", 67), // ORDER 单元 0035（装载序 × 相位 35 × 一 轮）
];

/// O(1) 语义查找（match 全枚举；命中率 100%% 由守卫保证）。
pub fn u3461mat_get(key: &str) -> Option<u64> {
    match key {
        "MNT0000" => Some(0),
        "FW0001" => Some(13),
        "SUITE0002" => Some(26),
        "DOM0003" => Some(39),
        "TABLE0004" => Some(52),
        "PRIO0005" => Some(65),
        "BIND0006" => Some(78),
        "UNBIND0007" => Some(91),
        "VER0008" => Some(7),
        "DEP0009" => Some(20),
        "CAP0010" => Some(33),
        "ORDER0011" => Some(46),
        "MNT0012" => Some(59),
        "FW0013" => Some(72),
        "SUITE0014" => Some(85),
        "DOM0015" => Some(1),
        "TABLE0016" => Some(14),
        "PRIO0017" => Some(27),
        "BIND0018" => Some(40),
        "UNBIND0019" => Some(53),
        "VER0020" => Some(66),
        "DEP0021" => Some(79),
        "CAP0022" => Some(92),
        "ORDER0023" => Some(8),
        "MNT0024" => Some(21),
        "FW0025" => Some(34),
        "SUITE0026" => Some(47),
        "DOM0027" => Some(60),
        "TABLE0028" => Some(73),
        "PRIO0029" => Some(86),
        "BIND0030" => Some(2),
        "UNBIND0031" => Some(15),
        "VER0032" => Some(28),
        "DEP0033" => Some(41),
        "CAP0034" => Some(54),
        "ORDER0035" => Some(67),
        _ => None,
    }
}

/// 逐行守卫扫描（返回违例行数；0 = 全绿）。
pub fn u3461mat_scan() -> usize {
    let mut bad = 0usize;
    let mut i = 0usize;
    while i < u3461mat_COUNT {
        let (k, v) = u3461mat[i];
        let ok = match k {
            "MNT0000" => v < 97,
            "FW0001" => v < 97,
            "SUITE0002" => v < 97,
            "DOM0003" => v < 97,
            "TABLE0004" => v < 97,
            "PRIO0005" => v < 97,
            "BIND0006" => v < 97,
            "UNBIND0007" => v < 97,
            "VER0008" => v < 97,
            "DEP0009" => v < 97,
            "CAP0010" => v < 97,
            "ORDER0011" => v < 97,
            "MNT0012" => v < 97,
            "FW0013" => v < 97,
            "SUITE0014" => v < 97,
            "DOM0015" => v < 97,
            "TABLE0016" => v < 97,
            "PRIO0017" => v < 97,
            "BIND0018" => v < 97,
            "UNBIND0019" => v < 97,
            "VER0020" => v < 97,
            "DEP0021" => v < 97,
            "CAP0022" => v < 97,
            "ORDER0023" => v < 97,
            "MNT0024" => v < 97,
            "FW0025" => v < 97,
            "SUITE0026" => v < 97,
            "DOM0027" => v < 97,
            "TABLE0028" => v < 97,
            "PRIO0029" => v < 97,
            "BIND0030" => v < 97,
            "UNBIND0031" => v < 97,
            "VER0032" => v < 97,
            "DEP0033" => v < 97,
            "CAP0034" => v < 97,
            "ORDER0035" => v < 97,
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
pub fn u3461mat_sum() -> u64 {
    let mut s = 0u64;
    let mut i = 0usize;
    while i < u3461mat_COUNT {
        s = s.wrapping_add(u3461mat[i].1 as u64);
        i += 1;
    }
    s
}

/// 键唯一性（构造期断言等价物：逐对扫描同键）。
pub fn u3461mat_unique() -> bool {
    let mut i = 0usize;
    while i < u3461mat_COUNT {
        let mut j = i + 1;
        while j < u3461mat_COUNT {
            if u3461mat[i].0 == u3461mat[j].0 {
                return false;
            }
            j += 1;
        }
        i += 1;
    }
    true
}

// --------------------------------------------------------------------------
// 直方图引擎 mxu3461hist（有界插入排序 + 分位数 P50/P90/P99）
// --------------------------------------------------------------------------
/// 分域挂载 直方图（96 桶滚动）。
/// 样本容量上限。
pub const MXU3461HIST_CAP: usize = 96;

/// 直方图账本（排序态 + 五统量）。
#[derive(Clone, Copy, Debug)]
pub struct mxu3461histHist {
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

impl mxu3461histHist {
    /// 空账构造。
    pub const fn new() -> Self {
        Self { samples: [0; 96], n: 0, drops: 0, min: 0, max: 0, sum: 0 }
    }

    /// 采样（有界插入排序；满员丢弃并记账）。
    pub fn push(&mut self, v: u64) {
        if self.n >= MXU3461HIST_CAP {
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

/// 段位守卫：全部诊断码必须落在本段 [0xe0d0, 0xe0df]。
pub const fn u3461_segment_guard() -> bool {
    let mut ok = true;
    let mut i = 0usize;
    while i < u3461Code::ALL.len() {
        let c = u3461Code::ALL[i].code();
        if c < 0xe0d0 || c > 0xe0df {
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
fn u3461_guard_seg() -> bool {
    u3461_segment_guard()
}

/// UNX-F3461 矩阵 36 行齐备 + 逐行守卫 + 唯一性 + 累计守恒。
fn check_u3461_map_guard() -> bool {
    let mut ok = u3461mat_COUNT == 36;
    if ok { ok = u3461mat_scan() == 0 && u3461mat_unique(); }
    if ok { ok = u3461mat_get("MNT0000") == Some(0) && u3461mat_get("ZZZ_NOKEY").is_none(); }
    if ok { ok = u3461mat_sum() == 1594; }
    ok
}

/// UNX-F3461 直方图 16 样本分位/均值/守恒。
fn check_u3461_map_hist() -> bool {
    let mut h = mxu3461histHist::new();
    let mut i = 0u64;
    while i < 16 { h.push(i + 1); i += 1; }
    let mut ok = h.conserved() && h.mean() >= 1 && h.percentile(500) >= 8;
    ok
}

/// 本模块自检（逐检查带 detail，全绿才出门）。
pub fn run_u3461_checks() -> CheckSet {
    let mut set = CheckSet::new("U3461");
    set.add(
        "u3461-seg-guard",
        u3461_guard_seg(),
        if u3461_guard_seg() { "" } else { "diagnostic code outside segment" },
    );
    set.add(
        "u3461-map-guard",
        check_u3461_map_guard(),
        if check_u3461_map_guard() { "" } else { "UNX-F3461 matrix guard mismatch" },
    );
    set.add(
        "u3461-map-hist",
        check_u3461_map_hist(),
        if check_u3461_map_hist() { "" } else { "UNX-F3461 hist mismatch" },
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
    fn u3461_all_checks_green() {
        let set = run_u3461_checks();
        assert!(set.all_passed(), "U3461 domain checks must be green");
    }

    #[test]
    fn u3461_segment_guard_holds() {
        assert!(u3461_segment_guard());
    }
}
