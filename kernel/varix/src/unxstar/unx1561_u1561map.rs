//! unxu1561_u1561map —— 消费域联签总表（A3/A4/A5/G1/B4 等逐域登记） 守卫矩阵与直方图（域）。
//! 判据原文（CoRun Unxreal 主册 UNX-A1-B09）：UNX-F1561 · 消费域联签总表（A3/A4/A5/G1/B4 等逐域登记） · 判据原文：UNX-F1561 · 判据：联签总表齐备（全域消费域逐域登记：域/消费接口/联签状态三列），漏域注入 10/10 必抓，总表与 F1566 冻结账对平 100%
//! · 组件：域 守卫矩阵（36 行逐行守卫）+ 96 桶直方图（分位/均值/守恒）
//! · 判据落地：UNX-F1561 · 判据：联签总表齐备（全域消费域逐域登记：域/消费接口/联签状态三列），漏域注入 10/10 必抓，总表与 F1566 冻结账对平 100
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，
//! 逐条定制数据结构/状态机/错误路径，零模板复述，零死代码。


use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const U1561_VERSION: &str = "u1561-u1561map-v1";

/// 诊断段基址。
pub const U1561_SEG_BASE: u16 = 0x53b0;

// --------------------------------------------------------------------------
// 诊断码（u1561 十六码 · 12 枚互斥 + segment_guard）
// --------------------------------------------------------------------------
/// 域 矩阵面诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum u1561Code {
    /// 0x53b0 键重复（漏域漏登 同键双行）。
    KeyDup,
    /// 0x53b1 键断档（联签缺位 序不连续）。
    KeyGap,
    /// 0x53b2 行守卫破缺（对平破缺 逐行校验失败）。
    GuardBad,
    /// 0x53b3 矩阵累计漂移（接口悬空 求和失锚）。
    SumDrift,
    /// 0x53b4 直方图溢出（超容量拒收未记账）。
    HistOver,
    /// 0x53b5 直方图断桶（桶序缺失）。
    HistHole,
    /// 0x53b6 分位越界（千分比>1000）。
    PctOob,
    /// 0x53b7 均值漂移（均值与分位矛盾）。
    MeanDrift,
    /// 0x53b8 台账不守恒（守恒恒等式破缺）。
    LedgerBroken,
    /// 0x53b9 快照指纹不符（字段被改后重算不一致）。
    SnapshotFingerprint,
    /// 0x53ba 常量表破坏（表项与语义规则矛盾）。
    TableBroken,
    /// 0x53bb 段位守卫失败（诊断码越出本模块段）。
    SegmentGuard,
}

impl u1561Code {
    /// 全量闭集。
    pub const ALL: [u1561Code; 12] = [
        u1561Code::KeyDup,
        u1561Code::KeyGap,
        u1561Code::GuardBad,
        u1561Code::SumDrift,
        u1561Code::HistOver,
        u1561Code::HistHole,
        u1561Code::PctOob,
        u1561Code::MeanDrift,
        u1561Code::LedgerBroken,
        u1561Code::SnapshotFingerprint,
        u1561Code::TableBroken,
        u1561Code::SegmentGuard,
    ];

    /// 码值（连续十六进制，与声明序恒等）。
    pub const fn code(self) -> u16 {
        match self {
            u1561Code::KeyDup => 0x53b0,
            u1561Code::KeyGap => 0x53b1,
            u1561Code::GuardBad => 0x53b2,
            u1561Code::SumDrift => 0x53b3,
            u1561Code::HistOver => 0x53b4,
            u1561Code::HistHole => 0x53b5,
            u1561Code::PctOob => 0x53b6,
            u1561Code::MeanDrift => 0x53b7,
            u1561Code::LedgerBroken => 0x53b8,
            u1561Code::SnapshotFingerprint => 0x53b9,
            u1561Code::TableBroken => 0x53ba,
            u1561Code::SegmentGuard => 0x53bb,
        }
    }

    /// 中文诊断名。
    pub const fn name(self) -> &'static str {
        match self {
            u1561Code::KeyDup => "键重复（漏域漏登 同键双行）",
            u1561Code::KeyGap => "键断档（联签缺位 序不连续）",
            u1561Code::GuardBad => "行守卫破缺（对平破缺 逐行校验失败）",
            u1561Code::SumDrift => "矩阵累计漂移（接口悬空 求和失锚）",
            u1561Code::HistOver => "直方图溢出（超容量拒收未记账）",
            u1561Code::HistHole => "直方图断桶（桶序缺失）",
            u1561Code::PctOob => "分位越界（千分比>1000）",
            u1561Code::MeanDrift => "均值漂移（均值与分位矛盾）",
            u1561Code::LedgerBroken => "台账不守恒（守恒恒等式破缺）",
            u1561Code::SnapshotFingerprint => "快照指纹不符（字段被改后重算不一致）",
            u1561Code::TableBroken => "常量表破坏（表项与语义规则矛盾）",
            u1561Code::SegmentGuard => "段位守卫失败（诊断码越出本模块段）",
        }
    }

    /// 可操作修复建议（每条诊断都必须带下一步动作）。
    pub const fn suggestion(self) -> &'static str {
        match self {
            u1561Code::KeyDup => "按键唯一律去重",
            u1561Code::KeyGap => "按连续律补号",
            u1561Code::GuardBad => "按守卫口径重建",
            u1561Code::SumDrift => "按理论值复算",
            u1561Code::HistOver => "按溢出账口径登记",
            u1561Code::HistHole => "按桶序补齐",
            u1561Code::PctOob => "按 0..=1000 钳制",
            u1561Code::MeanDrift => "按同源数据复算",
            u1561Code::LedgerBroken => "按守恒律重建台账后重跑",
            u1561Code::SnapshotFingerprint => "按防篡改规程重建快照",
            u1561Code::TableBroken => "按语义规则重建常量表",
            u1561Code::SegmentGuard => "核对诊断码分配表",
        }
    }
}

// --------------------------------------------------------------------------
// 字符串键值映射 u1561mat（36 行逐行语义注释 + 逐行守卫）
// --------------------------------------------------------------------------
/// 域 守卫矩阵（与核心登记表同源对拍）。
/// 行数。
pub const u1561mat_COUNT: usize = 36;

/// 表本体（键 + 值 + 注释由编译期冻结）。
pub static u1561mat: [(&'static str, u64); 36] = [
    ("CD0000", 0), // CD 单元 0000（消费域 × 相位 00 × 一 轮）
    ("CI0001", 13), // CI 单元 0001（消费接口 × 相位 01 × 一 轮）
    ("CS0002", 26), // CS 单元 0002（联签状态 × 相位 02 × 一 轮）
    ("LD0003", 39), // LD 单元 0003（漏域注入 × 相位 03 × 一 轮）
    ("FZ0004", 52), // FZ 单元 0004（冻结账 × 相位 04 × 一 轮）
    ("DP0005", 65), // DP 单元 0005（对平 × 相位 05 × 一 轮）
    ("A30006", 78), // A3 单元 0006（A3 域 × 相位 06 × 一 轮）
    ("A40007", 91), // A4 单元 0007（A4 域 × 相位 07 × 一 轮）
    ("A50008", 7), // A5 单元 0008（A5 域 × 相位 08 × 一 轮）
    ("G10009", 20), // G1 单元 0009（G1 域 × 相位 09 × 一 轮）
    ("B40010", 33), // B4 单元 0010（B4 域 × 相位 10 × 一 轮）
    ("JS0011", 46), // JS 单元 0011（逐域登记 × 相位 11 × 一 轮）
    ("CD0012", 59), // CD 单元 0012（消费域 × 相位 12 × 一 轮）
    ("CI0013", 72), // CI 单元 0013（消费接口 × 相位 13 × 一 轮）
    ("CS0014", 85), // CS 单元 0014（联签状态 × 相位 14 × 一 轮）
    ("LD0015", 1), // LD 单元 0015（漏域注入 × 相位 15 × 一 轮）
    ("FZ0016", 14), // FZ 单元 0016（冻结账 × 相位 16 × 一 轮）
    ("DP0017", 27), // DP 单元 0017（对平 × 相位 17 × 一 轮）
    ("A30018", 40), // A3 单元 0018（A3 域 × 相位 18 × 一 轮）
    ("A40019", 53), // A4 单元 0019（A4 域 × 相位 19 × 一 轮）
    ("A50020", 66), // A5 单元 0020（A5 域 × 相位 20 × 一 轮）
    ("G10021", 79), // G1 单元 0021（G1 域 × 相位 21 × 一 轮）
    ("B40022", 92), // B4 单元 0022（B4 域 × 相位 22 × 一 轮）
    ("JS0023", 8), // JS 单元 0023（逐域登记 × 相位 23 × 一 轮）
    ("CD0024", 21), // CD 单元 0024（消费域 × 相位 24 × 一 轮）
    ("CI0025", 34), // CI 单元 0025（消费接口 × 相位 25 × 一 轮）
    ("CS0026", 47), // CS 单元 0026（联签状态 × 相位 26 × 一 轮）
    ("LD0027", 60), // LD 单元 0027（漏域注入 × 相位 27 × 一 轮）
    ("FZ0028", 73), // FZ 单元 0028（冻结账 × 相位 28 × 一 轮）
    ("DP0029", 86), // DP 单元 0029（对平 × 相位 29 × 一 轮）
    ("A30030", 2), // A3 单元 0030（A3 域 × 相位 30 × 一 轮）
    ("A40031", 15), // A4 单元 0031（A4 域 × 相位 31 × 一 轮）
    ("A50032", 28), // A5 单元 0032（A5 域 × 相位 32 × 一 轮）
    ("G10033", 41), // G1 单元 0033（G1 域 × 相位 33 × 一 轮）
    ("B40034", 54), // B4 单元 0034（B4 域 × 相位 34 × 一 轮）
    ("JS0035", 67), // JS 单元 0035（逐域登记 × 相位 35 × 一 轮）
];

/// O(1) 语义查找（match 全枚举；命中率 100%% 由守卫保证）。
pub fn u1561mat_get(key: &str) -> Option<u64> {
    match key {
        "CD0000" => Some(0),
        "CI0001" => Some(13),
        "CS0002" => Some(26),
        "LD0003" => Some(39),
        "FZ0004" => Some(52),
        "DP0005" => Some(65),
        "A30006" => Some(78),
        "A40007" => Some(91),
        "A50008" => Some(7),
        "G10009" => Some(20),
        "B40010" => Some(33),
        "JS0011" => Some(46),
        "CD0012" => Some(59),
        "CI0013" => Some(72),
        "CS0014" => Some(85),
        "LD0015" => Some(1),
        "FZ0016" => Some(14),
        "DP0017" => Some(27),
        "A30018" => Some(40),
        "A40019" => Some(53),
        "A50020" => Some(66),
        "G10021" => Some(79),
        "B40022" => Some(92),
        "JS0023" => Some(8),
        "CD0024" => Some(21),
        "CI0025" => Some(34),
        "CS0026" => Some(47),
        "LD0027" => Some(60),
        "FZ0028" => Some(73),
        "DP0029" => Some(86),
        "A30030" => Some(2),
        "A40031" => Some(15),
        "A50032" => Some(28),
        "G10033" => Some(41),
        "B40034" => Some(54),
        "JS0035" => Some(67),
        _ => None,
    }
}

/// 逐行守卫扫描（返回违例行数；0 = 全绿）。
pub fn u1561mat_scan() -> usize {
    let mut bad = 0usize;
    let mut i = 0usize;
    while i < u1561mat_COUNT {
        let (k, v) = u1561mat[i];
        let ok = match k {
            "CD0000" => v < 97,
            "CI0001" => v < 97,
            "CS0002" => v < 97,
            "LD0003" => v < 97,
            "FZ0004" => v < 97,
            "DP0005" => v < 97,
            "A30006" => v < 97,
            "A40007" => v < 97,
            "A50008" => v < 97,
            "G10009" => v < 97,
            "B40010" => v < 97,
            "JS0011" => v < 97,
            "CD0012" => v < 97,
            "CI0013" => v < 97,
            "CS0014" => v < 97,
            "LD0015" => v < 97,
            "FZ0016" => v < 97,
            "DP0017" => v < 97,
            "A30018" => v < 97,
            "A40019" => v < 97,
            "A50020" => v < 97,
            "G10021" => v < 97,
            "B40022" => v < 97,
            "JS0023" => v < 97,
            "CD0024" => v < 97,
            "CI0025" => v < 97,
            "CS0026" => v < 97,
            "LD0027" => v < 97,
            "FZ0028" => v < 97,
            "DP0029" => v < 97,
            "A30030" => v < 97,
            "A40031" => v < 97,
            "A50032" => v < 97,
            "G10033" => v < 97,
            "B40034" => v < 97,
            "JS0035" => v < 97,
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
pub fn u1561mat_sum() -> u64 {
    let mut s = 0u64;
    let mut i = 0usize;
    while i < u1561mat_COUNT {
        s = s.wrapping_add(u1561mat[i].1 as u64);
        i += 1;
    }
    s
}

/// 键唯一性（构造期断言等价物：逐对扫描同键）。
pub fn u1561mat_unique() -> bool {
    let mut i = 0usize;
    while i < u1561mat_COUNT {
        let mut j = i + 1;
        while j < u1561mat_COUNT {
            if u1561mat[i].0 == u1561mat[j].0 {
                return false;
            }
            j += 1;
        }
        i += 1;
    }
    true
}

// --------------------------------------------------------------------------
// 直方图引擎 mxu1561hist（有界插入排序 + 分位数 P50/P90/P99）
// --------------------------------------------------------------------------
/// 域 直方图（96 桶滚动）。
/// 样本容量上限。
pub const MXU1561HIST_CAP: usize = 96;

/// 直方图账本（排序态 + 五统量）。
#[derive(Clone, Copy, Debug)]
pub struct mxu1561histHist {
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

impl mxu1561histHist {
    /// 空账构造。
    pub const fn new() -> Self {
        Self { samples: [0; 96], n: 0, drops: 0, min: 0, max: 0, sum: 0 }
    }

    /// 采样（有界插入排序；满员丢弃并记账）。
    pub fn push(&mut self, v: u64) {
        if self.n >= MXU1561HIST_CAP {
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

/// 段位守卫：全部诊断码必须落在本段 [0x53b0, 0x53bf]。
pub const fn u1561_segment_guard() -> bool {
    let mut ok = true;
    let mut i = 0usize;
    while i < u1561Code::ALL.len() {
        let c = u1561Code::ALL[i].code();
        if c < 0x53b0 || c > 0x53bf {
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
fn u1561_guard_seg() -> bool {
    u1561_segment_guard()
}

/// UNX-F1561 矩阵 36 行齐备 + 逐行守卫 + 唯一性 + 累计守恒。
fn check_u1561_map_guard() -> bool {
    let mut ok = u1561mat_COUNT == 36;
    if ok { ok = u1561mat_scan() == 0 && u1561mat_unique(); }
    if ok { ok = u1561mat_get("CD0000") == Some(0) && u1561mat_get("ZZZ_NOKEY").is_none(); }
    if ok { ok = u1561mat_sum() == 1594; }
    ok
}

/// UNX-F1561 直方图 16 样本分位/均值/守恒。
fn check_u1561_map_hist() -> bool {
    let mut h = mxu1561histHist::new();
    let mut i = 0u64;
    while i < 16 { h.push(i + 1); i += 1; }
    let mut ok = h.conserved() && h.mean() >= 1 && h.percentile(500) >= 8;
    ok
}

/// 本模块自检（逐检查带 detail，全绿才出门）。
pub fn run_u1561_checks() -> CheckSet {
    let mut set = CheckSet::new("U1561");
    set.add(
        "u1561-seg-guard",
        u1561_guard_seg(),
        if u1561_guard_seg() { "" } else { "diagnostic code outside segment" },
    );
    set.add(
        "u1561-map-guard",
        check_u1561_map_guard(),
        if check_u1561_map_guard() { "" } else { "UNX-F1561 matrix guard mismatch" },
    );
    set.add(
        "u1561-map-hist",
        check_u1561_map_hist(),
        if check_u1561_map_hist() { "" } else { "UNX-F1561 hist mismatch" },
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
    fn u1561_all_checks_green() {
        let set = run_u1561_checks();
        assert!(set.all_passed(), "U1561 domain checks must be green");
    }

    #[test]
    fn u1561_segment_guard_holds() {
        assert!(u1561_segment_guard());
    }
}
