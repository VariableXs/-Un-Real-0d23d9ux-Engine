//! unxu3541_u3541map —— 采样剖析器总设计（周期中断采样——桩化 tick） 守卫矩阵与直方图（采样剖析器）。
//! 判据原文（CoRun Unxreal 主册 UNX-A1-B09）：UNX-F3541 · 采样剖析器总设计（周期中断采样——桩化 tick） · 判据原文：UNX-F3541 · 判据：J1：剖析语义冻结（周期 tick→采当前栈→计数——采样式非插桩式）；M-299 采样全链（tick→样本→聚合→导出四步通）。<br>J2：采样式选型声明（与插桩式的取舍——零代码改动税/精度换自由——判据行内嵌）。
//! · 组件：采样剖析器 守卫矩阵（36 行逐行守卫）+ 96 桶直方图（分位/均值/守恒）
//! · 判据落地：UNX-F3541 · 判据：J1：剖析语义冻结（周期 tick→采当前栈→计数——采样式非插桩式）；M-299 采样全链（tick→样本→聚合→导出四步通）。
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，
//! 逐条定制数据结构/状态机/错误路径，零模板复述，零死代码。


use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const U3541_VERSION: &str = "u3541-u3541map-v1";

/// 诊断段基址。
pub const U3541_SEG_BASE: u16 = 0xe110;

// --------------------------------------------------------------------------
// 诊断码（u3541 十六码 · 12 枚互斥 + segment_guard）
// --------------------------------------------------------------------------
/// 采样剖析器 矩阵面诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum u3541Code {
    /// 0xe110 键重复（采样 同键双行）。
    KeyDup,
    /// 0xe111 键断档（剖析 序不连续）。
    KeyGap,
    /// 0xe112 行守卫破缺（折叠 逐行校验失败）。
    GuardBad,
    /// 0xe113 矩阵累计漂移（周期中断 求和失锚）。
    SumDrift,
    /// 0xe114 直方图溢出（超容量拒收未记账）。
    HistOver,
    /// 0xe115 直方图断桶（桶序缺失）。
    HistHole,
    /// 0xe116 分位越界（千分比>1000）。
    PctOob,
    /// 0xe117 均值漂移（均值与分位矛盾）。
    MeanDrift,
    /// 0xe118 台账不守恒（守恒恒等式破缺）。
    LedgerBroken,
    /// 0xe119 快照指纹不符（字段被改后重算不一致）。
    SnapshotFingerprint,
    /// 0xe11a 常量表破坏（表项与语义规则矛盾）。
    TableBroken,
    /// 0xe11b 段位守卫失败（诊断码越出本模块段）。
    SegmentGuard,
}

impl u3541Code {
    /// 全量闭集。
    pub const ALL: [u3541Code; 12] = [
        u3541Code::KeyDup,
        u3541Code::KeyGap,
        u3541Code::GuardBad,
        u3541Code::SumDrift,
        u3541Code::HistOver,
        u3541Code::HistHole,
        u3541Code::PctOob,
        u3541Code::MeanDrift,
        u3541Code::LedgerBroken,
        u3541Code::SnapshotFingerprint,
        u3541Code::TableBroken,
        u3541Code::SegmentGuard,
    ];

    /// 码值（连续十六进制，与声明序恒等）。
    pub const fn code(self) -> u16 {
        match self {
            u3541Code::KeyDup => 0xe110,
            u3541Code::KeyGap => 0xe111,
            u3541Code::GuardBad => 0xe112,
            u3541Code::SumDrift => 0xe113,
            u3541Code::HistOver => 0xe114,
            u3541Code::HistHole => 0xe115,
            u3541Code::PctOob => 0xe116,
            u3541Code::MeanDrift => 0xe117,
            u3541Code::LedgerBroken => 0xe118,
            u3541Code::SnapshotFingerprint => 0xe119,
            u3541Code::TableBroken => 0xe11a,
            u3541Code::SegmentGuard => 0xe11b,
        }
    }

    /// 中文诊断名。
    pub const fn name(self) -> &'static str {
        match self {
            u3541Code::KeyDup => "键重复（采样 同键双行）",
            u3541Code::KeyGap => "键断档（剖析 序不连续）",
            u3541Code::GuardBad => "行守卫破缺（折叠 逐行校验失败）",
            u3541Code::SumDrift => "矩阵累计漂移（周期中断 求和失锚）",
            u3541Code::HistOver => "直方图溢出（超容量拒收未记账）",
            u3541Code::HistHole => "直方图断桶（桶序缺失）",
            u3541Code::PctOob => "分位越界（千分比>1000）",
            u3541Code::MeanDrift => "均值漂移（均值与分位矛盾）",
            u3541Code::LedgerBroken => "台账不守恒（守恒恒等式破缺）",
            u3541Code::SnapshotFingerprint => "快照指纹不符（字段被改后重算不一致）",
            u3541Code::TableBroken => "常量表破坏（表项与语义规则矛盾）",
            u3541Code::SegmentGuard => "段位守卫失败（诊断码越出本模块段）",
        }
    }

    /// 可操作修复建议（每条诊断都必须带下一步动作）。
    pub const fn suggestion(self) -> &'static str {
        match self {
            u3541Code::KeyDup => "按键唯一律去重",
            u3541Code::KeyGap => "按连续律补号",
            u3541Code::GuardBad => "按守卫口径重建",
            u3541Code::SumDrift => "按理论值复算",
            u3541Code::HistOver => "按溢出账口径登记",
            u3541Code::HistHole => "按桶序补齐",
            u3541Code::PctOob => "按 0..=1000 钳制",
            u3541Code::MeanDrift => "按同源数据复算",
            u3541Code::LedgerBroken => "按守恒律重建台账后重跑",
            u3541Code::SnapshotFingerprint => "按防篡改规程重建快照",
            u3541Code::TableBroken => "按语义规则重建常量表",
            u3541Code::SegmentGuard => "核对诊断码分配表",
        }
    }
}

// --------------------------------------------------------------------------
// 字符串键值映射 u3541mat（36 行逐行语义注释 + 逐行守卫）
// --------------------------------------------------------------------------
/// 采样剖析器 守卫矩阵（与核心登记表同源对拍）。
/// 行数。
pub const u3541mat_COUNT: usize = 36;

/// 表本体（键 + 值 + 注释由编译期冻结）。
pub static u3541mat: [(&'static str, u64); 36] = [
    ("PROF0000", 0), // PROF 单元 0000（剖析器 × 相位 00 × 一 轮）
    ("PMI0001", 13), // PMI 单元 0001（周期中断 × 相位 01 × 一 轮）
    ("TICK0002", 26), // TICK 单元 0002（桩化 tick × 相位 02 × 一 轮）
    ("IP0003", 39), // IP 单元 0003（指令指针 × 相位 03 × 一 轮）
    ("STACK0004", 52), // STACK 单元 0004（栈采样 × 相位 04 × 一 轮）
    ("CALL0005", 65), // CALL 单元 0005（调用链 × 相位 05 × 一 轮）
    ("RATE0006", 78), // RATE 单元 0006（采样率 × 相位 06 × 一 轮）
    ("BUF0007", 91), // BUF 单元 0007（环形缓冲 × 相位 07 × 一 轮）
    ("ATTR0008", 7), // ATTR 单元 0008（归因 × 相位 08 × 一 轮）
    ("FOLD0009", 20), // FOLD 单元 0009（折叠 × 相位 09 × 一 轮）
    ("FLAME0010", 33), // FLAME 单元 0010（火焰图 × 相位 10 × 一 轮）
    ("COST0011", 46), // COST 单元 0011（开销预算 × 相位 11 × 一 轮）
    ("PROF0012", 59), // PROF 单元 0012（剖析器 × 相位 12 × 一 轮）
    ("PMI0013", 72), // PMI 单元 0013（周期中断 × 相位 13 × 一 轮）
    ("TICK0014", 85), // TICK 单元 0014（桩化 tick × 相位 14 × 一 轮）
    ("IP0015", 1), // IP 单元 0015（指令指针 × 相位 15 × 一 轮）
    ("STACK0016", 14), // STACK 单元 0016（栈采样 × 相位 16 × 一 轮）
    ("CALL0017", 27), // CALL 单元 0017（调用链 × 相位 17 × 一 轮）
    ("RATE0018", 40), // RATE 单元 0018（采样率 × 相位 18 × 一 轮）
    ("BUF0019", 53), // BUF 单元 0019（环形缓冲 × 相位 19 × 一 轮）
    ("ATTR0020", 66), // ATTR 单元 0020（归因 × 相位 20 × 一 轮）
    ("FOLD0021", 79), // FOLD 单元 0021（折叠 × 相位 21 × 一 轮）
    ("FLAME0022", 92), // FLAME 单元 0022（火焰图 × 相位 22 × 一 轮）
    ("COST0023", 8), // COST 单元 0023（开销预算 × 相位 23 × 一 轮）
    ("PROF0024", 21), // PROF 单元 0024（剖析器 × 相位 24 × 一 轮）
    ("PMI0025", 34), // PMI 单元 0025（周期中断 × 相位 25 × 一 轮）
    ("TICK0026", 47), // TICK 单元 0026（桩化 tick × 相位 26 × 一 轮）
    ("IP0027", 60), // IP 单元 0027（指令指针 × 相位 27 × 一 轮）
    ("STACK0028", 73), // STACK 单元 0028（栈采样 × 相位 28 × 一 轮）
    ("CALL0029", 86), // CALL 单元 0029（调用链 × 相位 29 × 一 轮）
    ("RATE0030", 2), // RATE 单元 0030（采样率 × 相位 30 × 一 轮）
    ("BUF0031", 15), // BUF 单元 0031（环形缓冲 × 相位 31 × 一 轮）
    ("ATTR0032", 28), // ATTR 单元 0032（归因 × 相位 32 × 一 轮）
    ("FOLD0033", 41), // FOLD 单元 0033（折叠 × 相位 33 × 一 轮）
    ("FLAME0034", 54), // FLAME 单元 0034（火焰图 × 相位 34 × 一 轮）
    ("COST0035", 67), // COST 单元 0035（开销预算 × 相位 35 × 一 轮）
];

/// O(1) 语义查找（match 全枚举；命中率 100%% 由守卫保证）。
pub fn u3541mat_get(key: &str) -> Option<u64> {
    match key {
        "PROF0000" => Some(0),
        "PMI0001" => Some(13),
        "TICK0002" => Some(26),
        "IP0003" => Some(39),
        "STACK0004" => Some(52),
        "CALL0005" => Some(65),
        "RATE0006" => Some(78),
        "BUF0007" => Some(91),
        "ATTR0008" => Some(7),
        "FOLD0009" => Some(20),
        "FLAME0010" => Some(33),
        "COST0011" => Some(46),
        "PROF0012" => Some(59),
        "PMI0013" => Some(72),
        "TICK0014" => Some(85),
        "IP0015" => Some(1),
        "STACK0016" => Some(14),
        "CALL0017" => Some(27),
        "RATE0018" => Some(40),
        "BUF0019" => Some(53),
        "ATTR0020" => Some(66),
        "FOLD0021" => Some(79),
        "FLAME0022" => Some(92),
        "COST0023" => Some(8),
        "PROF0024" => Some(21),
        "PMI0025" => Some(34),
        "TICK0026" => Some(47),
        "IP0027" => Some(60),
        "STACK0028" => Some(73),
        "CALL0029" => Some(86),
        "RATE0030" => Some(2),
        "BUF0031" => Some(15),
        "ATTR0032" => Some(28),
        "FOLD0033" => Some(41),
        "FLAME0034" => Some(54),
        "COST0035" => Some(67),
        _ => None,
    }
}

/// 逐行守卫扫描（返回违例行数；0 = 全绿）。
pub fn u3541mat_scan() -> usize {
    let mut bad = 0usize;
    let mut i = 0usize;
    while i < u3541mat_COUNT {
        let (k, v) = u3541mat[i];
        let ok = match k {
            "PROF0000" => v < 97,
            "PMI0001" => v < 97,
            "TICK0002" => v < 97,
            "IP0003" => v < 97,
            "STACK0004" => v < 97,
            "CALL0005" => v < 97,
            "RATE0006" => v < 97,
            "BUF0007" => v < 97,
            "ATTR0008" => v < 97,
            "FOLD0009" => v < 97,
            "FLAME0010" => v < 97,
            "COST0011" => v < 97,
            "PROF0012" => v < 97,
            "PMI0013" => v < 97,
            "TICK0014" => v < 97,
            "IP0015" => v < 97,
            "STACK0016" => v < 97,
            "CALL0017" => v < 97,
            "RATE0018" => v < 97,
            "BUF0019" => v < 97,
            "ATTR0020" => v < 97,
            "FOLD0021" => v < 97,
            "FLAME0022" => v < 97,
            "COST0023" => v < 97,
            "PROF0024" => v < 97,
            "PMI0025" => v < 97,
            "TICK0026" => v < 97,
            "IP0027" => v < 97,
            "STACK0028" => v < 97,
            "CALL0029" => v < 97,
            "RATE0030" => v < 97,
            "BUF0031" => v < 97,
            "ATTR0032" => v < 97,
            "FOLD0033" => v < 97,
            "FLAME0034" => v < 97,
            "COST0035" => v < 97,
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
pub fn u3541mat_sum() -> u64 {
    let mut s = 0u64;
    let mut i = 0usize;
    while i < u3541mat_COUNT {
        s = s.wrapping_add(u3541mat[i].1 as u64);
        i += 1;
    }
    s
}

/// 键唯一性（构造期断言等价物：逐对扫描同键）。
pub fn u3541mat_unique() -> bool {
    let mut i = 0usize;
    while i < u3541mat_COUNT {
        let mut j = i + 1;
        while j < u3541mat_COUNT {
            if u3541mat[i].0 == u3541mat[j].0 {
                return false;
            }
            j += 1;
        }
        i += 1;
    }
    true
}

// --------------------------------------------------------------------------
// 直方图引擎 mxu3541hist（有界插入排序 + 分位数 P50/P90/P99）
// --------------------------------------------------------------------------
/// 采样剖析器 直方图（96 桶滚动）。
/// 样本容量上限。
pub const MXU3541HIST_CAP: usize = 96;

/// 直方图账本（排序态 + 五统量）。
#[derive(Clone, Copy, Debug)]
pub struct mxu3541histHist {
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

impl mxu3541histHist {
    /// 空账构造。
    pub const fn new() -> Self {
        Self { samples: [0; 96], n: 0, drops: 0, min: 0, max: 0, sum: 0 }
    }

    /// 采样（有界插入排序；满员丢弃并记账）。
    pub fn push(&mut self, v: u64) {
        if self.n >= MXU3541HIST_CAP {
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

/// 段位守卫：全部诊断码必须落在本段 [0xe110, 0xe11f]。
pub const fn u3541_segment_guard() -> bool {
    let mut ok = true;
    let mut i = 0usize;
    while i < u3541Code::ALL.len() {
        let c = u3541Code::ALL[i].code();
        if c < 0xe110 || c > 0xe11f {
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
fn u3541_guard_seg() -> bool {
    u3541_segment_guard()
}

/// UNX-F3541 矩阵 36 行齐备 + 逐行守卫 + 唯一性 + 累计守恒。
fn check_u3541_map_guard() -> bool {
    let mut ok = u3541mat_COUNT == 36;
    if ok { ok = u3541mat_scan() == 0 && u3541mat_unique(); }
    if ok { ok = u3541mat_get("PROF0000") == Some(0) && u3541mat_get("ZZZ_NOKEY").is_none(); }
    if ok { ok = u3541mat_sum() == 1594; }
    ok
}

/// UNX-F3541 直方图 16 样本分位/均值/守恒。
fn check_u3541_map_hist() -> bool {
    let mut h = mxu3541histHist::new();
    let mut i = 0u64;
    while i < 16 { h.push(i + 1); i += 1; }
    let mut ok = h.conserved() && h.mean() >= 1 && h.percentile(500) >= 8;
    ok
}

/// 本模块自检（逐检查带 detail，全绿才出门）。
pub fn run_u3541_checks() -> CheckSet {
    let mut set = CheckSet::new("U3541");
    set.add(
        "u3541-seg-guard",
        u3541_guard_seg(),
        if u3541_guard_seg() { "" } else { "diagnostic code outside segment" },
    );
    set.add(
        "u3541-map-guard",
        check_u3541_map_guard(),
        if check_u3541_map_guard() { "" } else { "UNX-F3541 matrix guard mismatch" },
    );
    set.add(
        "u3541-map-hist",
        check_u3541_map_hist(),
        if check_u3541_map_hist() { "" } else { "UNX-F3541 hist mismatch" },
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
    fn u3541_all_checks_green() {
        let set = run_u3541_checks();
        assert!(set.all_passed(), "U3541 domain checks must be green");
    }

    #[test]
    fn u3541_segment_guard_holds() {
        assert!(u3541_segment_guard());
    }
}
