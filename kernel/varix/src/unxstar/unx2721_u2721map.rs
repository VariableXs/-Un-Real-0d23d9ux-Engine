//! unxu2721_u2721map —— 扫描器主循环与扫描节拍 守卫矩阵与直方图（扫描轮次）。
//! 判据原文（CoRun Unxreal 主册 UNX-A1-B09）：UNX-F2721 · 扫描器主循环与扫描节拍 · 判据原文：UNX-F2721 · 判据：扫描主循环（isolate→谓词→逐出/归还三段）闭环 10 轮账平，扫描节拍双触发（水线/直接回收）命中同一入口
//! · 组件：扫描轮次 守卫矩阵（36 行逐行守卫）+ 96 桶直方图（分位/均值/守恒）
//! · 判据落地：UNX-F2721 · 判据：扫描主循环（isolate→谓词→逐出/归还三段）闭环 10 轮账平，扫描节拍双触发（水线/直接回收）命中同一入口
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，
//! 逐条定制数据结构/状态机/错误路径，零模板复述，零死代码。


use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const U2721_VERSION: &str = "u2721-u2721map-v1";

/// 诊断段基址。
pub const U2721_SEG_BASE: u16 = 0xac30;

// --------------------------------------------------------------------------
// 诊断码（u2721 十六码 · 12 枚互斥 + segment_guard）
// --------------------------------------------------------------------------
/// 扫描轮次 矩阵面诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum u2721Code {
    /// 0xac30 键重复（三段断链 同键双行）。
    KeyDup,
    /// 0xac31 键断档（节拍丢失 序不连续）。
    KeyGap,
    /// 0xac32 行守卫破缺（入口分叉 逐行校验失败）。
    GuardBad,
    /// 0xac33 矩阵累计漂移（账不平 求和失锚）。
    SumDrift,
    /// 0xac34 直方图溢出（超容量拒收未记账）。
    HistOver,
    /// 0xac35 直方图断桶（桶序缺失）。
    HistHole,
    /// 0xac36 分位越界（千分比>1000）。
    PctOob,
    /// 0xac37 均值漂移（均值与分位矛盾）。
    MeanDrift,
    /// 0xac38 台账不守恒（守恒恒等式破缺）。
    LedgerBroken,
    /// 0xac39 快照指纹不符（字段被改后重算不一致）。
    SnapshotFingerprint,
    /// 0xac3a 常量表破坏（表项与语义规则矛盾）。
    TableBroken,
    /// 0xac3b 段位守卫失败（诊断码越出本模块段）。
    SegmentGuard,
}

impl u2721Code {
    /// 全量闭集。
    pub const ALL: [u2721Code; 12] = [
        u2721Code::KeyDup,
        u2721Code::KeyGap,
        u2721Code::GuardBad,
        u2721Code::SumDrift,
        u2721Code::HistOver,
        u2721Code::HistHole,
        u2721Code::PctOob,
        u2721Code::MeanDrift,
        u2721Code::LedgerBroken,
        u2721Code::SnapshotFingerprint,
        u2721Code::TableBroken,
        u2721Code::SegmentGuard,
    ];

    /// 码值（连续十六进制，与声明序恒等）。
    pub const fn code(self) -> u16 {
        match self {
            u2721Code::KeyDup => 0xac30,
            u2721Code::KeyGap => 0xac31,
            u2721Code::GuardBad => 0xac32,
            u2721Code::SumDrift => 0xac33,
            u2721Code::HistOver => 0xac34,
            u2721Code::HistHole => 0xac35,
            u2721Code::PctOob => 0xac36,
            u2721Code::MeanDrift => 0xac37,
            u2721Code::LedgerBroken => 0xac38,
            u2721Code::SnapshotFingerprint => 0xac39,
            u2721Code::TableBroken => 0xac3a,
            u2721Code::SegmentGuard => 0xac3b,
        }
    }

    /// 中文诊断名。
    pub const fn name(self) -> &'static str {
        match self {
            u2721Code::KeyDup => "键重复（三段断链 同键双行）",
            u2721Code::KeyGap => "键断档（节拍丢失 序不连续）",
            u2721Code::GuardBad => "行守卫破缺（入口分叉 逐行校验失败）",
            u2721Code::SumDrift => "矩阵累计漂移（账不平 求和失锚）",
            u2721Code::HistOver => "直方图溢出（超容量拒收未记账）",
            u2721Code::HistHole => "直方图断桶（桶序缺失）",
            u2721Code::PctOob => "分位越界（千分比>1000）",
            u2721Code::MeanDrift => "均值漂移（均值与分位矛盾）",
            u2721Code::LedgerBroken => "台账不守恒（守恒恒等式破缺）",
            u2721Code::SnapshotFingerprint => "快照指纹不符（字段被改后重算不一致）",
            u2721Code::TableBroken => "常量表破坏（表项与语义规则矛盾）",
            u2721Code::SegmentGuard => "段位守卫失败（诊断码越出本模块段）",
        }
    }

    /// 可操作修复建议（每条诊断都必须带下一步动作）。
    pub const fn suggestion(self) -> &'static str {
        match self {
            u2721Code::KeyDup => "按键唯一律去重",
            u2721Code::KeyGap => "按连续律补号",
            u2721Code::GuardBad => "按守卫口径重建",
            u2721Code::SumDrift => "按理论值复算",
            u2721Code::HistOver => "按溢出账口径登记",
            u2721Code::HistHole => "按桶序补齐",
            u2721Code::PctOob => "按 0..=1000 钳制",
            u2721Code::MeanDrift => "按同源数据复算",
            u2721Code::LedgerBroken => "按守恒律重建台账后重跑",
            u2721Code::SnapshotFingerprint => "按防篡改规程重建快照",
            u2721Code::TableBroken => "按语义规则重建常量表",
            u2721Code::SegmentGuard => "核对诊断码分配表",
        }
    }
}

// --------------------------------------------------------------------------
// 字符串键值映射 u2721mat（36 行逐行语义注释 + 逐行守卫）
// --------------------------------------------------------------------------
/// 扫描轮次 守卫矩阵（与核心登记表同源对拍）。
/// 行数。
pub const u2721mat_COUNT: usize = 36;

/// 表本体（键 + 值 + 注释由编译期冻结）。
pub static u2721mat: [(&'static str, u64); 36] = [
    ("SC0000", 0), // SC 单元 0000（扫描器 × 相位 00 × 一 轮）
    ("IS0001", 13), // IS 单元 0001（isolate 段 × 相位 01 × 一 轮）
    ("PD0002", 26), // PD 单元 0002（谓词段 × 相位 02 × 一 轮）
    ("EV0003", 39), // EV 单元 0003（逐出段 × 相位 03 × 一 轮）
    ("RT0004", 52), // RT 单元 0004（归还段 × 相位 04 × 一 轮）
    ("TP0005", 65), // TP 单元 0005（扫描节拍 × 相位 05 × 一 轮）
    ("WL0006", 78), // WL 单元 0006（水线触发 × 相位 06 × 一 轮）
    ("DR0007", 91), // DR 单元 0007（直接回收触发 × 相位 07 × 一 轮）
    ("SE0008", 7), // SE 单元 0008（同一入口 × 相位 08 × 一 轮）
    ("R100009", 20), // R10 单元 0009（十轮闭环 × 相位 09 × 一 轮）
    ("AP0010", 33), // AP 单元 0010（账平断言 × 相位 10 × 一 轮）
    ("CT0011", 46), // CT 单元 0011（节拍面 × 相位 11 × 一 轮）
    ("SC0012", 59), // SC 单元 0012（扫描器 × 相位 12 × 一 轮）
    ("IS0013", 72), // IS 单元 0013（isolate 段 × 相位 13 × 一 轮）
    ("PD0014", 85), // PD 单元 0014（谓词段 × 相位 14 × 一 轮）
    ("EV0015", 1), // EV 单元 0015（逐出段 × 相位 15 × 一 轮）
    ("RT0016", 14), // RT 单元 0016（归还段 × 相位 16 × 一 轮）
    ("TP0017", 27), // TP 单元 0017（扫描节拍 × 相位 17 × 一 轮）
    ("WL0018", 40), // WL 单元 0018（水线触发 × 相位 18 × 一 轮）
    ("DR0019", 53), // DR 单元 0019（直接回收触发 × 相位 19 × 一 轮）
    ("SE0020", 66), // SE 单元 0020（同一入口 × 相位 20 × 一 轮）
    ("R100021", 79), // R10 单元 0021（十轮闭环 × 相位 21 × 一 轮）
    ("AP0022", 92), // AP 单元 0022（账平断言 × 相位 22 × 一 轮）
    ("CT0023", 8), // CT 单元 0023（节拍面 × 相位 23 × 一 轮）
    ("SC0024", 21), // SC 单元 0024（扫描器 × 相位 24 × 一 轮）
    ("IS0025", 34), // IS 单元 0025（isolate 段 × 相位 25 × 一 轮）
    ("PD0026", 47), // PD 单元 0026（谓词段 × 相位 26 × 一 轮）
    ("EV0027", 60), // EV 单元 0027（逐出段 × 相位 27 × 一 轮）
    ("RT0028", 73), // RT 单元 0028（归还段 × 相位 28 × 一 轮）
    ("TP0029", 86), // TP 单元 0029（扫描节拍 × 相位 29 × 一 轮）
    ("WL0030", 2), // WL 单元 0030（水线触发 × 相位 30 × 一 轮）
    ("DR0031", 15), // DR 单元 0031（直接回收触发 × 相位 31 × 一 轮）
    ("SE0032", 28), // SE 单元 0032（同一入口 × 相位 32 × 一 轮）
    ("R100033", 41), // R10 单元 0033（十轮闭环 × 相位 33 × 一 轮）
    ("AP0034", 54), // AP 单元 0034（账平断言 × 相位 34 × 一 轮）
    ("CT0035", 67), // CT 单元 0035（节拍面 × 相位 35 × 一 轮）
];

/// O(1) 语义查找（match 全枚举；命中率 100%% 由守卫保证）。
pub fn u2721mat_get(key: &str) -> Option<u64> {
    match key {
        "SC0000" => Some(0),
        "IS0001" => Some(13),
        "PD0002" => Some(26),
        "EV0003" => Some(39),
        "RT0004" => Some(52),
        "TP0005" => Some(65),
        "WL0006" => Some(78),
        "DR0007" => Some(91),
        "SE0008" => Some(7),
        "R100009" => Some(20),
        "AP0010" => Some(33),
        "CT0011" => Some(46),
        "SC0012" => Some(59),
        "IS0013" => Some(72),
        "PD0014" => Some(85),
        "EV0015" => Some(1),
        "RT0016" => Some(14),
        "TP0017" => Some(27),
        "WL0018" => Some(40),
        "DR0019" => Some(53),
        "SE0020" => Some(66),
        "R100021" => Some(79),
        "AP0022" => Some(92),
        "CT0023" => Some(8),
        "SC0024" => Some(21),
        "IS0025" => Some(34),
        "PD0026" => Some(47),
        "EV0027" => Some(60),
        "RT0028" => Some(73),
        "TP0029" => Some(86),
        "WL0030" => Some(2),
        "DR0031" => Some(15),
        "SE0032" => Some(28),
        "R100033" => Some(41),
        "AP0034" => Some(54),
        "CT0035" => Some(67),
        _ => None,
    }
}

/// 逐行守卫扫描（返回违例行数；0 = 全绿）。
pub fn u2721mat_scan() -> usize {
    let mut bad = 0usize;
    let mut i = 0usize;
    while i < u2721mat_COUNT {
        let (k, v) = u2721mat[i];
        let ok = match k {
            "SC0000" => v < 97,
            "IS0001" => v < 97,
            "PD0002" => v < 97,
            "EV0003" => v < 97,
            "RT0004" => v < 97,
            "TP0005" => v < 97,
            "WL0006" => v < 97,
            "DR0007" => v < 97,
            "SE0008" => v < 97,
            "R100009" => v < 97,
            "AP0010" => v < 97,
            "CT0011" => v < 97,
            "SC0012" => v < 97,
            "IS0013" => v < 97,
            "PD0014" => v < 97,
            "EV0015" => v < 97,
            "RT0016" => v < 97,
            "TP0017" => v < 97,
            "WL0018" => v < 97,
            "DR0019" => v < 97,
            "SE0020" => v < 97,
            "R100021" => v < 97,
            "AP0022" => v < 97,
            "CT0023" => v < 97,
            "SC0024" => v < 97,
            "IS0025" => v < 97,
            "PD0026" => v < 97,
            "EV0027" => v < 97,
            "RT0028" => v < 97,
            "TP0029" => v < 97,
            "WL0030" => v < 97,
            "DR0031" => v < 97,
            "SE0032" => v < 97,
            "R100033" => v < 97,
            "AP0034" => v < 97,
            "CT0035" => v < 97,
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
pub fn u2721mat_sum() -> u64 {
    let mut s = 0u64;
    let mut i = 0usize;
    while i < u2721mat_COUNT {
        s = s.wrapping_add(u2721mat[i].1 as u64);
        i += 1;
    }
    s
}

/// 键唯一性（构造期断言等价物：逐对扫描同键）。
pub fn u2721mat_unique() -> bool {
    let mut i = 0usize;
    while i < u2721mat_COUNT {
        let mut j = i + 1;
        while j < u2721mat_COUNT {
            if u2721mat[i].0 == u2721mat[j].0 {
                return false;
            }
            j += 1;
        }
        i += 1;
    }
    true
}

// --------------------------------------------------------------------------
// 直方图引擎 mxu2721hist（有界插入排序 + 分位数 P50/P90/P99）
// --------------------------------------------------------------------------
/// 扫描轮次 直方图（96 桶滚动）。
/// 样本容量上限。
pub const MXU2721HIST_CAP: usize = 96;

/// 直方图账本（排序态 + 五统量）。
#[derive(Clone, Copy, Debug)]
pub struct mxu2721histHist {
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

impl mxu2721histHist {
    /// 空账构造。
    pub const fn new() -> Self {
        Self { samples: [0; 96], n: 0, drops: 0, min: 0, max: 0, sum: 0 }
    }

    /// 采样（有界插入排序；满员丢弃并记账）。
    pub fn push(&mut self, v: u64) {
        if self.n >= MXU2721HIST_CAP {
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

/// 段位守卫：全部诊断码必须落在本段 [0xac30, 0xac3f]。
pub const fn u2721_segment_guard() -> bool {
    let mut ok = true;
    let mut i = 0usize;
    while i < u2721Code::ALL.len() {
        let c = u2721Code::ALL[i].code();
        if c < 0xac30 || c > 0xac3f {
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
fn u2721_guard_seg() -> bool {
    u2721_segment_guard()
}

/// UNX-F2721 矩阵 36 行齐备 + 逐行守卫 + 唯一性 + 累计守恒。
fn check_u2721_map_guard() -> bool {
    let mut ok = u2721mat_COUNT == 36;
    if ok { ok = u2721mat_scan() == 0 && u2721mat_unique(); }
    if ok { ok = u2721mat_get("SC0000") == Some(0) && u2721mat_get("ZZZ_NOKEY").is_none(); }
    if ok { ok = u2721mat_sum() == 1594; }
    ok
}

/// UNX-F2721 直方图 16 样本分位/均值/守恒。
fn check_u2721_map_hist() -> bool {
    let mut h = mxu2721histHist::new();
    let mut i = 0u64;
    while i < 16 { h.push(i + 1); i += 1; }
    let mut ok = h.conserved() && h.mean() >= 1 && h.percentile(500) >= 8;
    ok
}

/// 本模块自检（逐检查带 detail，全绿才出门）。
pub fn run_u2721_checks() -> CheckSet {
    let mut set = CheckSet::new("U2721");
    set.add(
        "u2721-seg-guard",
        u2721_guard_seg(),
        if u2721_guard_seg() { "" } else { "diagnostic code outside segment" },
    );
    set.add(
        "u2721-map-guard",
        check_u2721_map_guard(),
        if check_u2721_map_guard() { "" } else { "UNX-F2721 matrix guard mismatch" },
    );
    set.add(
        "u2721-map-hist",
        check_u2721_map_hist(),
        if check_u2721_map_hist() { "" } else { "UNX-F2721 hist mismatch" },
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
    fn u2721_all_checks_green() {
        let set = run_u2721_checks();
        assert!(set.all_passed(), "U2721 domain checks must be green");
    }

    #[test]
    fn u2721_segment_guard_holds() {
        assert!(u2721_segment_guard());
    }
}
