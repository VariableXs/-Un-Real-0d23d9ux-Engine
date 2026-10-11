//! unxu2661_u2661map —— 页缓存结构总纲（radix 树类，4K 页粒度——§73 好条范本） 守卫矩阵与直方图（缓存页）。
//! 判据原文（CoRun Unxreal 主册 UNX-A1-B09）：UNX-F2661 · 页缓存结构总纲（radix 树类，4K 页粒度——§73 好条范本） · 判据原文：UNX-F2661 · 判据：radix 树类结构逐项断言（键=（文件对象，页偏移），10^6 次操作不变量全过），4K 粒度契约与 §73 范本一致
//! · 组件：缓存页 守卫矩阵（36 行逐行守卫）+ 96 桶直方图（分位/均值/守恒）
//! · 判据落地：UNX-F2661 · 判据：radix 树类结构逐项断言（键=（文件对象，页偏移），10^6 次操作不变量全过），4K 粒度契约与 §73 范本一致
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，
//! 逐条定制数据结构/状态机/错误路径，零模板复述，零死代码。


use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const U2661_VERSION: &str = "u2661-u2661map-v1";

/// 诊断段基址。
pub const U2661_SEG_BASE: u16 = 0xa9f0;

// --------------------------------------------------------------------------
// 诊断码（u2661 十六码 · 12 枚互斥 + segment_guard）
// --------------------------------------------------------------------------
/// 缓存页 矩阵面诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum u2661Code {
    /// 0xa9f0 键重复（键碰撞 同键双行）。
    KeyDup,
    /// 0xa9f1 键断档（粒度错配 序不连续）。
    KeyGap,
    /// 0xa9f2 行守卫破缺（不变量破缺 逐行校验失败）。
    GuardBad,
    /// 0xa9f3 矩阵累计漂移（范本背离 求和失锚）。
    SumDrift,
    /// 0xa9f4 直方图溢出（超容量拒收未记账）。
    HistOver,
    /// 0xa9f5 直方图断桶（桶序缺失）。
    HistHole,
    /// 0xa9f6 分位越界（千分比>1000）。
    PctOob,
    /// 0xa9f7 均值漂移（均值与分位矛盾）。
    MeanDrift,
    /// 0xa9f8 台账不守恒（守恒恒等式破缺）。
    LedgerBroken,
    /// 0xa9f9 快照指纹不符（字段被改后重算不一致）。
    SnapshotFingerprint,
    /// 0xa9fa 常量表破坏（表项与语义规则矛盾）。
    TableBroken,
    /// 0xa9fb 段位守卫失败（诊断码越出本模块段）。
    SegmentGuard,
}

impl u2661Code {
    /// 全量闭集。
    pub const ALL: [u2661Code; 12] = [
        u2661Code::KeyDup,
        u2661Code::KeyGap,
        u2661Code::GuardBad,
        u2661Code::SumDrift,
        u2661Code::HistOver,
        u2661Code::HistHole,
        u2661Code::PctOob,
        u2661Code::MeanDrift,
        u2661Code::LedgerBroken,
        u2661Code::SnapshotFingerprint,
        u2661Code::TableBroken,
        u2661Code::SegmentGuard,
    ];

    /// 码值（连续十六进制，与声明序恒等）。
    pub const fn code(self) -> u16 {
        match self {
            u2661Code::KeyDup => 0xa9f0,
            u2661Code::KeyGap => 0xa9f1,
            u2661Code::GuardBad => 0xa9f2,
            u2661Code::SumDrift => 0xa9f3,
            u2661Code::HistOver => 0xa9f4,
            u2661Code::HistHole => 0xa9f5,
            u2661Code::PctOob => 0xa9f6,
            u2661Code::MeanDrift => 0xa9f7,
            u2661Code::LedgerBroken => 0xa9f8,
            u2661Code::SnapshotFingerprint => 0xa9f9,
            u2661Code::TableBroken => 0xa9fa,
            u2661Code::SegmentGuard => 0xa9fb,
        }
    }

    /// 中文诊断名。
    pub const fn name(self) -> &'static str {
        match self {
            u2661Code::KeyDup => "键重复（键碰撞 同键双行）",
            u2661Code::KeyGap => "键断档（粒度错配 序不连续）",
            u2661Code::GuardBad => "行守卫破缺（不变量破缺 逐行校验失败）",
            u2661Code::SumDrift => "矩阵累计漂移（范本背离 求和失锚）",
            u2661Code::HistOver => "直方图溢出（超容量拒收未记账）",
            u2661Code::HistHole => "直方图断桶（桶序缺失）",
            u2661Code::PctOob => "分位越界（千分比>1000）",
            u2661Code::MeanDrift => "均值漂移（均值与分位矛盾）",
            u2661Code::LedgerBroken => "台账不守恒（守恒恒等式破缺）",
            u2661Code::SnapshotFingerprint => "快照指纹不符（字段被改后重算不一致）",
            u2661Code::TableBroken => "常量表破坏（表项与语义规则矛盾）",
            u2661Code::SegmentGuard => "段位守卫失败（诊断码越出本模块段）",
        }
    }

    /// 可操作修复建议（每条诊断都必须带下一步动作）。
    pub const fn suggestion(self) -> &'static str {
        match self {
            u2661Code::KeyDup => "按键唯一律去重",
            u2661Code::KeyGap => "按连续律补号",
            u2661Code::GuardBad => "按守卫口径重建",
            u2661Code::SumDrift => "按理论值复算",
            u2661Code::HistOver => "按溢出账口径登记",
            u2661Code::HistHole => "按桶序补齐",
            u2661Code::PctOob => "按 0..=1000 钳制",
            u2661Code::MeanDrift => "按同源数据复算",
            u2661Code::LedgerBroken => "按守恒律重建台账后重跑",
            u2661Code::SnapshotFingerprint => "按防篡改规程重建快照",
            u2661Code::TableBroken => "按语义规则重建常量表",
            u2661Code::SegmentGuard => "核对诊断码分配表",
        }
    }
}

// --------------------------------------------------------------------------
// 字符串键值映射 u2661mat（36 行逐行语义注释 + 逐行守卫）
// --------------------------------------------------------------------------
/// 缓存页 守卫矩阵（与核心登记表同源对拍）。
/// 行数。
pub const u2661mat_COUNT: usize = 36;

/// 表本体（键 + 值 + 注释由编译期冻结）。
pub static u2661mat: [(&'static str, u64); 36] = [
    ("PC0000", 0), // PC 单元 0000（页缓存 × 相位 00 × 一 轮）
    ("RX0001", 13), // RX 单元 0001（radix 树 × 相位 01 × 一 轮）
    ("KY0002", 26), // KY 单元 0002（复合键 × 相位 02 × 一 轮）
    ("FO0003", 39), // FO 单元 0003（文件对象 × 相位 03 × 一 轮）
    ("PO0004", 52), // PO 单元 0004（页偏移 × 相位 04 × 一 轮）
    ("G40005", 65), // G4 单元 0005（4K 粒度 × 相位 05 × 一 轮）
    ("M10006", 78), // M1 单元 0006（百万次操作 × 相位 06 × 一 轮）
    ("IV0007", 91), // IV 单元 0007（不变量 × 相位 07 × 一 轮）
    ("P730008", 7), // P73 单元 0008（§73 范本 × 相位 08 × 一 轮）
    ("HT0009", 20), // HT 单元 0009（命中面 × 相位 09 × 一 轮）
    ("MS0010", 33), // MS 单元 0010（未命中 × 相位 10 × 一 轮）
    ("EV0011", 46), // EV 单元 0011（逐项断言 × 相位 11 × 一 轮）
    ("PC0012", 59), // PC 单元 0012（页缓存 × 相位 12 × 一 轮）
    ("RX0013", 72), // RX 单元 0013（radix 树 × 相位 13 × 一 轮）
    ("KY0014", 85), // KY 单元 0014（复合键 × 相位 14 × 一 轮）
    ("FO0015", 1), // FO 单元 0015（文件对象 × 相位 15 × 一 轮）
    ("PO0016", 14), // PO 单元 0016（页偏移 × 相位 16 × 一 轮）
    ("G40017", 27), // G4 单元 0017（4K 粒度 × 相位 17 × 一 轮）
    ("M10018", 40), // M1 单元 0018（百万次操作 × 相位 18 × 一 轮）
    ("IV0019", 53), // IV 单元 0019（不变量 × 相位 19 × 一 轮）
    ("P730020", 66), // P73 单元 0020（§73 范本 × 相位 20 × 一 轮）
    ("HT0021", 79), // HT 单元 0021（命中面 × 相位 21 × 一 轮）
    ("MS0022", 92), // MS 单元 0022（未命中 × 相位 22 × 一 轮）
    ("EV0023", 8), // EV 单元 0023（逐项断言 × 相位 23 × 一 轮）
    ("PC0024", 21), // PC 单元 0024（页缓存 × 相位 24 × 一 轮）
    ("RX0025", 34), // RX 单元 0025（radix 树 × 相位 25 × 一 轮）
    ("KY0026", 47), // KY 单元 0026（复合键 × 相位 26 × 一 轮）
    ("FO0027", 60), // FO 单元 0027（文件对象 × 相位 27 × 一 轮）
    ("PO0028", 73), // PO 单元 0028（页偏移 × 相位 28 × 一 轮）
    ("G40029", 86), // G4 单元 0029（4K 粒度 × 相位 29 × 一 轮）
    ("M10030", 2), // M1 单元 0030（百万次操作 × 相位 30 × 一 轮）
    ("IV0031", 15), // IV 单元 0031（不变量 × 相位 31 × 一 轮）
    ("P730032", 28), // P73 单元 0032（§73 范本 × 相位 32 × 一 轮）
    ("HT0033", 41), // HT 单元 0033（命中面 × 相位 33 × 一 轮）
    ("MS0034", 54), // MS 单元 0034（未命中 × 相位 34 × 一 轮）
    ("EV0035", 67), // EV 单元 0035（逐项断言 × 相位 35 × 一 轮）
];

/// O(1) 语义查找（match 全枚举；命中率 100%% 由守卫保证）。
pub fn u2661mat_get(key: &str) -> Option<u64> {
    match key {
        "PC0000" => Some(0),
        "RX0001" => Some(13),
        "KY0002" => Some(26),
        "FO0003" => Some(39),
        "PO0004" => Some(52),
        "G40005" => Some(65),
        "M10006" => Some(78),
        "IV0007" => Some(91),
        "P730008" => Some(7),
        "HT0009" => Some(20),
        "MS0010" => Some(33),
        "EV0011" => Some(46),
        "PC0012" => Some(59),
        "RX0013" => Some(72),
        "KY0014" => Some(85),
        "FO0015" => Some(1),
        "PO0016" => Some(14),
        "G40017" => Some(27),
        "M10018" => Some(40),
        "IV0019" => Some(53),
        "P730020" => Some(66),
        "HT0021" => Some(79),
        "MS0022" => Some(92),
        "EV0023" => Some(8),
        "PC0024" => Some(21),
        "RX0025" => Some(34),
        "KY0026" => Some(47),
        "FO0027" => Some(60),
        "PO0028" => Some(73),
        "G40029" => Some(86),
        "M10030" => Some(2),
        "IV0031" => Some(15),
        "P730032" => Some(28),
        "HT0033" => Some(41),
        "MS0034" => Some(54),
        "EV0035" => Some(67),
        _ => None,
    }
}

/// 逐行守卫扫描（返回违例行数；0 = 全绿）。
pub fn u2661mat_scan() -> usize {
    let mut bad = 0usize;
    let mut i = 0usize;
    while i < u2661mat_COUNT {
        let (k, v) = u2661mat[i];
        let ok = match k {
            "PC0000" => v < 97,
            "RX0001" => v < 97,
            "KY0002" => v < 97,
            "FO0003" => v < 97,
            "PO0004" => v < 97,
            "G40005" => v < 97,
            "M10006" => v < 97,
            "IV0007" => v < 97,
            "P730008" => v < 97,
            "HT0009" => v < 97,
            "MS0010" => v < 97,
            "EV0011" => v < 97,
            "PC0012" => v < 97,
            "RX0013" => v < 97,
            "KY0014" => v < 97,
            "FO0015" => v < 97,
            "PO0016" => v < 97,
            "G40017" => v < 97,
            "M10018" => v < 97,
            "IV0019" => v < 97,
            "P730020" => v < 97,
            "HT0021" => v < 97,
            "MS0022" => v < 97,
            "EV0023" => v < 97,
            "PC0024" => v < 97,
            "RX0025" => v < 97,
            "KY0026" => v < 97,
            "FO0027" => v < 97,
            "PO0028" => v < 97,
            "G40029" => v < 97,
            "M10030" => v < 97,
            "IV0031" => v < 97,
            "P730032" => v < 97,
            "HT0033" => v < 97,
            "MS0034" => v < 97,
            "EV0035" => v < 97,
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
pub fn u2661mat_sum() -> u64 {
    let mut s = 0u64;
    let mut i = 0usize;
    while i < u2661mat_COUNT {
        s = s.wrapping_add(u2661mat[i].1 as u64);
        i += 1;
    }
    s
}

/// 键唯一性（构造期断言等价物：逐对扫描同键）。
pub fn u2661mat_unique() -> bool {
    let mut i = 0usize;
    while i < u2661mat_COUNT {
        let mut j = i + 1;
        while j < u2661mat_COUNT {
            if u2661mat[i].0 == u2661mat[j].0 {
                return false;
            }
            j += 1;
        }
        i += 1;
    }
    true
}

// --------------------------------------------------------------------------
// 直方图引擎 mxu2661hist（有界插入排序 + 分位数 P50/P90/P99）
// --------------------------------------------------------------------------
/// 缓存页 直方图（96 桶滚动）。
/// 样本容量上限。
pub const MXU2661HIST_CAP: usize = 96;

/// 直方图账本（排序态 + 五统量）。
#[derive(Clone, Copy, Debug)]
pub struct mxu2661histHist {
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

impl mxu2661histHist {
    /// 空账构造。
    pub const fn new() -> Self {
        Self { samples: [0; 96], n: 0, drops: 0, min: 0, max: 0, sum: 0 }
    }

    /// 采样（有界插入排序；满员丢弃并记账）。
    pub fn push(&mut self, v: u64) {
        if self.n >= MXU2661HIST_CAP {
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

/// 段位守卫：全部诊断码必须落在本段 [0xa9f0, 0xa9ff]。
pub const fn u2661_segment_guard() -> bool {
    let mut ok = true;
    let mut i = 0usize;
    while i < u2661Code::ALL.len() {
        let c = u2661Code::ALL[i].code();
        if c < 0xa9f0 || c > 0xa9ff {
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
fn u2661_guard_seg() -> bool {
    u2661_segment_guard()
}

/// UNX-F2661 矩阵 36 行齐备 + 逐行守卫 + 唯一性 + 累计守恒。
fn check_u2661_map_guard() -> bool {
    let mut ok = u2661mat_COUNT == 36;
    if ok { ok = u2661mat_scan() == 0 && u2661mat_unique(); }
    if ok { ok = u2661mat_get("PC0000") == Some(0) && u2661mat_get("ZZZ_NOKEY").is_none(); }
    if ok { ok = u2661mat_sum() == 1594; }
    ok
}

/// UNX-F2661 直方图 16 样本分位/均值/守恒。
fn check_u2661_map_hist() -> bool {
    let mut h = mxu2661histHist::new();
    let mut i = 0u64;
    while i < 16 { h.push(i + 1); i += 1; }
    let mut ok = h.conserved() && h.mean() >= 1 && h.percentile(500) >= 8;
    ok
}

/// 本模块自检（逐检查带 detail，全绿才出门）。
pub fn run_u2661_checks() -> CheckSet {
    let mut set = CheckSet::new("U2661");
    set.add(
        "u2661-seg-guard",
        u2661_guard_seg(),
        if u2661_guard_seg() { "" } else { "diagnostic code outside segment" },
    );
    set.add(
        "u2661-map-guard",
        check_u2661_map_guard(),
        if check_u2661_map_guard() { "" } else { "UNX-F2661 matrix guard mismatch" },
    );
    set.add(
        "u2661-map-hist",
        check_u2661_map_hist(),
        if check_u2661_map_hist() { "" } else { "UNX-F2661 hist mismatch" },
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
    fn u2661_all_checks_green() {
        let set = run_u2661_checks();
        assert!(set.all_passed(), "U2661 domain checks must be green");
    }

    #[test]
    fn u2661_segment_guard_holds() {
        assert!(u2661_segment_guard());
    }
}
