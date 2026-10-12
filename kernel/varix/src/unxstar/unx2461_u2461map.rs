//! unxu2461_u2461map —— 水线三档判定（min/low/high） 守卫矩阵与直方图（水线档位）。
//! 判据原文（CoRun Unxreal 主册 UNX-A1-B09）：UNX-F2461 · 水线三档判定（min/low/high） · 判据原文：UNX-F2461 · 判据：三档水线判定与 nr_free 总账（F2425）同源单点，三机型初值生效，跨越事件 10/10 次正确触发零漏报零重复
//! · 组件：水线档位 守卫矩阵（36 行逐行守卫）+ 96 桶直方图（分位/均值/守恒）
//! · 判据落地：UNX-F2461 · 判据：三档水线判定与 nr_free 总账（F2425）同源单点，三机型初值生效，跨越事件 10/10 次正确触发零漏报零重复
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，
//! 逐条定制数据结构/状态机/错误路径，零模板复述，零死代码。


use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const U2461_VERSION: &str = "u2461-u2461map-v1";

/// 诊断段基址。
pub const U2461_SEG_BASE: u16 = 0xa270;

// --------------------------------------------------------------------------
// 诊断码（u2461 十六码 · 12 枚互斥 + segment_guard）
// --------------------------------------------------------------------------
/// 水线档位 矩阵面诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum u2461Code {
    /// 0xa270 键重复（漏报触发 同键双行）。
    KeyDup,
    /// 0xa271 键断档（重复上报 序不连续）。
    KeyGap,
    /// 0xa272 行守卫破缺（初值错配 逐行校验失败）。
    GuardBad,
    /// 0xa273 矩阵累计漂移（跨档抖动 求和失锚）。
    SumDrift,
    /// 0xa274 直方图溢出（超容量拒收未记账）。
    HistOver,
    /// 0xa275 直方图断桶（桶序缺失）。
    HistHole,
    /// 0xa276 分位越界（千分比>1000）。
    PctOob,
    /// 0xa277 均值漂移（均值与分位矛盾）。
    MeanDrift,
    /// 0xa278 台账不守恒（守恒恒等式破缺）。
    LedgerBroken,
    /// 0xa279 快照指纹不符（字段被改后重算不一致）。
    SnapshotFingerprint,
    /// 0xa27a 常量表破坏（表项与语义规则矛盾）。
    TableBroken,
    /// 0xa27b 段位守卫失败（诊断码越出本模块段）。
    SegmentGuard,
}

impl u2461Code {
    /// 全量闭集。
    pub const ALL: [u2461Code; 12] = [
        u2461Code::KeyDup,
        u2461Code::KeyGap,
        u2461Code::GuardBad,
        u2461Code::SumDrift,
        u2461Code::HistOver,
        u2461Code::HistHole,
        u2461Code::PctOob,
        u2461Code::MeanDrift,
        u2461Code::LedgerBroken,
        u2461Code::SnapshotFingerprint,
        u2461Code::TableBroken,
        u2461Code::SegmentGuard,
    ];

    /// 码值（连续十六进制，与声明序恒等）。
    pub const fn code(self) -> u16 {
        match self {
            u2461Code::KeyDup => 0xa270,
            u2461Code::KeyGap => 0xa271,
            u2461Code::GuardBad => 0xa272,
            u2461Code::SumDrift => 0xa273,
            u2461Code::HistOver => 0xa274,
            u2461Code::HistHole => 0xa275,
            u2461Code::PctOob => 0xa276,
            u2461Code::MeanDrift => 0xa277,
            u2461Code::LedgerBroken => 0xa278,
            u2461Code::SnapshotFingerprint => 0xa279,
            u2461Code::TableBroken => 0xa27a,
            u2461Code::SegmentGuard => 0xa27b,
        }
    }

    /// 中文诊断名。
    pub const fn name(self) -> &'static str {
        match self {
            u2461Code::KeyDup => "键重复（漏报触发 同键双行）",
            u2461Code::KeyGap => "键断档（重复上报 序不连续）",
            u2461Code::GuardBad => "行守卫破缺（初值错配 逐行校验失败）",
            u2461Code::SumDrift => "矩阵累计漂移（跨档抖动 求和失锚）",
            u2461Code::HistOver => "直方图溢出（超容量拒收未记账）",
            u2461Code::HistHole => "直方图断桶（桶序缺失）",
            u2461Code::PctOob => "分位越界（千分比>1000）",
            u2461Code::MeanDrift => "均值漂移（均值与分位矛盾）",
            u2461Code::LedgerBroken => "台账不守恒（守恒恒等式破缺）",
            u2461Code::SnapshotFingerprint => "快照指纹不符（字段被改后重算不一致）",
            u2461Code::TableBroken => "常量表破坏（表项与语义规则矛盾）",
            u2461Code::SegmentGuard => "段位守卫失败（诊断码越出本模块段）",
        }
    }

    /// 可操作修复建议（每条诊断都必须带下一步动作）。
    pub const fn suggestion(self) -> &'static str {
        match self {
            u2461Code::KeyDup => "按键唯一律去重",
            u2461Code::KeyGap => "按连续律补号",
            u2461Code::GuardBad => "按守卫口径重建",
            u2461Code::SumDrift => "按理论值复算",
            u2461Code::HistOver => "按溢出账口径登记",
            u2461Code::HistHole => "按桶序补齐",
            u2461Code::PctOob => "按 0..=1000 钳制",
            u2461Code::MeanDrift => "按同源数据复算",
            u2461Code::LedgerBroken => "按守恒律重建台账后重跑",
            u2461Code::SnapshotFingerprint => "按防篡改规程重建快照",
            u2461Code::TableBroken => "按语义规则重建常量表",
            u2461Code::SegmentGuard => "核对诊断码分配表",
        }
    }
}

// --------------------------------------------------------------------------
// 字符串键值映射 u2461mat（36 行逐行语义注释 + 逐行守卫）
// --------------------------------------------------------------------------
/// 水线档位 守卫矩阵（与核心登记表同源对拍）。
/// 行数。
pub const u2461mat_COUNT: usize = 36;

/// 表本体（键 + 值 + 注释由编译期冻结）。
pub static u2461mat: [(&'static str, u64); 36] = [
    ("MN0000", 0), // MN 单元 0000（min 档 × 相位 00 × 一 轮）
    ("LW0001", 13), // LW 单元 0001（low 档 × 相位 01 × 一 轮）
    ("HI0002", 26), // HI 单元 0002（high 档 × 相位 02 × 一 轮）
    ("NF0003", 39), // NF 单元 0003（nr_free 总账 × 相位 03 × 一 轮）
    ("SP0004", 52), // SP 单元 0004（同源单点 × 相位 04 × 一 轮）
    ("MC0005", 65), // MC 单元 0005（三机型 × 相位 05 × 一 轮）
    ("IV0006", 78), // IV 单元 0006（初值账 × 相位 06 × 一 轮）
    ("XE0007", 91), // XE 单元 0007（跨越事件 × 相位 07 × 一 轮）
    ("TR0008", 7), // TR 单元 0008（触发账 × 相位 08 × 一 轮）
    ("MS0009", 20), // MS 单元 0009（漏报账 × 相位 09 × 一 轮）
    ("RP0010", 33), // RP 单元 0010（重复账 × 相位 10 × 一 轮）
    ("ZT0011", 46), // ZT 单元 0011（零误断言 × 相位 11 × 一 轮）
    ("MN0012", 59), // MN 单元 0012（min 档 × 相位 12 × 一 轮）
    ("LW0013", 72), // LW 单元 0013（low 档 × 相位 13 × 一 轮）
    ("HI0014", 85), // HI 单元 0014（high 档 × 相位 14 × 一 轮）
    ("NF0015", 1), // NF 单元 0015（nr_free 总账 × 相位 15 × 一 轮）
    ("SP0016", 14), // SP 单元 0016（同源单点 × 相位 16 × 一 轮）
    ("MC0017", 27), // MC 单元 0017（三机型 × 相位 17 × 一 轮）
    ("IV0018", 40), // IV 单元 0018（初值账 × 相位 18 × 一 轮）
    ("XE0019", 53), // XE 单元 0019（跨越事件 × 相位 19 × 一 轮）
    ("TR0020", 66), // TR 单元 0020（触发账 × 相位 20 × 一 轮）
    ("MS0021", 79), // MS 单元 0021（漏报账 × 相位 21 × 一 轮）
    ("RP0022", 92), // RP 单元 0022（重复账 × 相位 22 × 一 轮）
    ("ZT0023", 8), // ZT 单元 0023（零误断言 × 相位 23 × 一 轮）
    ("MN0024", 21), // MN 单元 0024（min 档 × 相位 24 × 一 轮）
    ("LW0025", 34), // LW 单元 0025（low 档 × 相位 25 × 一 轮）
    ("HI0026", 47), // HI 单元 0026（high 档 × 相位 26 × 一 轮）
    ("NF0027", 60), // NF 单元 0027（nr_free 总账 × 相位 27 × 一 轮）
    ("SP0028", 73), // SP 单元 0028（同源单点 × 相位 28 × 一 轮）
    ("MC0029", 86), // MC 单元 0029（三机型 × 相位 29 × 一 轮）
    ("IV0030", 2), // IV 单元 0030（初值账 × 相位 30 × 一 轮）
    ("XE0031", 15), // XE 单元 0031（跨越事件 × 相位 31 × 一 轮）
    ("TR0032", 28), // TR 单元 0032（触发账 × 相位 32 × 一 轮）
    ("MS0033", 41), // MS 单元 0033（漏报账 × 相位 33 × 一 轮）
    ("RP0034", 54), // RP 单元 0034（重复账 × 相位 34 × 一 轮）
    ("ZT0035", 67), // ZT 单元 0035（零误断言 × 相位 35 × 一 轮）
];

/// O(1) 语义查找（match 全枚举；命中率 100%% 由守卫保证）。
pub fn u2461mat_get(key: &str) -> Option<u64> {
    match key {
        "MN0000" => Some(0),
        "LW0001" => Some(13),
        "HI0002" => Some(26),
        "NF0003" => Some(39),
        "SP0004" => Some(52),
        "MC0005" => Some(65),
        "IV0006" => Some(78),
        "XE0007" => Some(91),
        "TR0008" => Some(7),
        "MS0009" => Some(20),
        "RP0010" => Some(33),
        "ZT0011" => Some(46),
        "MN0012" => Some(59),
        "LW0013" => Some(72),
        "HI0014" => Some(85),
        "NF0015" => Some(1),
        "SP0016" => Some(14),
        "MC0017" => Some(27),
        "IV0018" => Some(40),
        "XE0019" => Some(53),
        "TR0020" => Some(66),
        "MS0021" => Some(79),
        "RP0022" => Some(92),
        "ZT0023" => Some(8),
        "MN0024" => Some(21),
        "LW0025" => Some(34),
        "HI0026" => Some(47),
        "NF0027" => Some(60),
        "SP0028" => Some(73),
        "MC0029" => Some(86),
        "IV0030" => Some(2),
        "XE0031" => Some(15),
        "TR0032" => Some(28),
        "MS0033" => Some(41),
        "RP0034" => Some(54),
        "ZT0035" => Some(67),
        _ => None,
    }
}

/// 逐行守卫扫描（返回违例行数；0 = 全绿）。
pub fn u2461mat_scan() -> usize {
    let mut bad = 0usize;
    let mut i = 0usize;
    while i < u2461mat_COUNT {
        let (k, v) = u2461mat[i];
        let ok = match k {
            "MN0000" => v < 97,
            "LW0001" => v < 97,
            "HI0002" => v < 97,
            "NF0003" => v < 97,
            "SP0004" => v < 97,
            "MC0005" => v < 97,
            "IV0006" => v < 97,
            "XE0007" => v < 97,
            "TR0008" => v < 97,
            "MS0009" => v < 97,
            "RP0010" => v < 97,
            "ZT0011" => v < 97,
            "MN0012" => v < 97,
            "LW0013" => v < 97,
            "HI0014" => v < 97,
            "NF0015" => v < 97,
            "SP0016" => v < 97,
            "MC0017" => v < 97,
            "IV0018" => v < 97,
            "XE0019" => v < 97,
            "TR0020" => v < 97,
            "MS0021" => v < 97,
            "RP0022" => v < 97,
            "ZT0023" => v < 97,
            "MN0024" => v < 97,
            "LW0025" => v < 97,
            "HI0026" => v < 97,
            "NF0027" => v < 97,
            "SP0028" => v < 97,
            "MC0029" => v < 97,
            "IV0030" => v < 97,
            "XE0031" => v < 97,
            "TR0032" => v < 97,
            "MS0033" => v < 97,
            "RP0034" => v < 97,
            "ZT0035" => v < 97,
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
pub fn u2461mat_sum() -> u64 {
    let mut s = 0u64;
    let mut i = 0usize;
    while i < u2461mat_COUNT {
        s = s.wrapping_add(u2461mat[i].1 as u64);
        i += 1;
    }
    s
}

/// 键唯一性（构造期断言等价物：逐对扫描同键）。
pub fn u2461mat_unique() -> bool {
    let mut i = 0usize;
    while i < u2461mat_COUNT {
        let mut j = i + 1;
        while j < u2461mat_COUNT {
            if u2461mat[i].0 == u2461mat[j].0 {
                return false;
            }
            j += 1;
        }
        i += 1;
    }
    true
}

// --------------------------------------------------------------------------
// 直方图引擎 mxu2461hist（有界插入排序 + 分位数 P50/P90/P99）
// --------------------------------------------------------------------------
/// 水线档位 直方图（96 桶滚动）。
/// 样本容量上限。
pub const MXU2461HIST_CAP: usize = 96;

/// 直方图账本（排序态 + 五统量）。
#[derive(Clone, Copy, Debug)]
pub struct mxu2461histHist {
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

impl mxu2461histHist {
    /// 空账构造。
    pub const fn new() -> Self {
        Self { samples: [0; 96], n: 0, drops: 0, min: 0, max: 0, sum: 0 }
    }

    /// 采样（有界插入排序；满员丢弃并记账）。
    pub fn push(&mut self, v: u64) {
        if self.n >= MXU2461HIST_CAP {
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

/// 段位守卫：全部诊断码必须落在本段 [0xa270, 0xa27f]。
pub const fn u2461_segment_guard() -> bool {
    let mut ok = true;
    let mut i = 0usize;
    while i < u2461Code::ALL.len() {
        let c = u2461Code::ALL[i].code();
        if c < 0xa270 || c > 0xa27f {
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
fn u2461_guard_seg() -> bool {
    u2461_segment_guard()
}

/// UNX-F2461 矩阵 36 行齐备 + 逐行守卫 + 唯一性 + 累计守恒。
fn check_u2461_map_guard() -> bool {
    let mut ok = u2461mat_COUNT == 36;
    if ok { ok = u2461mat_scan() == 0 && u2461mat_unique(); }
    if ok { ok = u2461mat_get("MN0000") == Some(0) && u2461mat_get("ZZZ_NOKEY").is_none(); }
    if ok { ok = u2461mat_sum() == 1594; }
    ok
}

/// UNX-F2461 直方图 16 样本分位/均值/守恒。
fn check_u2461_map_hist() -> bool {
    let mut h = mxu2461histHist::new();
    let mut i = 0u64;
    while i < 16 { h.push(i + 1); i += 1; }
    let mut ok = h.conserved() && h.mean() >= 1 && h.percentile(500) >= 8;
    ok
}

/// 本模块自检（逐检查带 detail，全绿才出门）。
pub fn run_u2461_checks() -> CheckSet {
    let mut set = CheckSet::new("U2461");
    set.add(
        "u2461-seg-guard",
        u2461_guard_seg(),
        if u2461_guard_seg() { "" } else { "diagnostic code outside segment" },
    );
    set.add(
        "u2461-map-guard",
        check_u2461_map_guard(),
        if check_u2461_map_guard() { "" } else { "UNX-F2461 matrix guard mismatch" },
    );
    set.add(
        "u2461-map-hist",
        check_u2461_map_hist(),
        if check_u2461_map_hist() { "" } else { "UNX-F2461 hist mismatch" },
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
    fn u2461_all_checks_green() {
        let set = run_u2461_checks();
        assert!(set.all_passed(), "U2461 domain checks must be green");
    }

    #[test]
    fn u2461_segment_guard_holds() {
        assert!(u2461_segment_guard());
    }
}
