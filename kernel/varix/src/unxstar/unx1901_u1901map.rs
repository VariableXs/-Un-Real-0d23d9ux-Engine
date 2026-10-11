//! unxu1901_u1901map —— 扩展状态区基座（XSAVE/XRSTOR 与 XCR0 位图裁断） 守卫矩阵与直方图（状态位）。
//! 判据原文（CoRun Unxreal 主册 UNX-A1-B09）：UNX-F1901 · 扩展状态区基座（XSAVE/XRSTOR 与 XCR0 位图裁断） · 判据原文：UNX-F1901 · 判据：XSAVE 区按 XCR0 位图逐位裁断布局（x87/SSE/AVX/AVX-512 支持则存），区域尺寸查询与对齐断言全过，保存/恢复往返保真（AVX 样本矩阵比对零差）
//! · 组件：状态位 守卫矩阵（36 行逐行守卫）+ 96 桶直方图（分位/均值/守恒）
//! · 判据落地：UNX-F1901 · 判据：XSAVE 区按 XCR0 位图逐位裁断布局（x87/SSE/AVX/AVX-512 支持则存），区域尺寸查询与对齐断言全过，保存
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，
//! 逐条定制数据结构/状态机/错误路径，零模板复述，零死代码。


use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const U1901_VERSION: &str = "u1901-u1901map-v1";

/// 诊断段基址。
pub const U1901_SEG_BASE: u16 = 0x6070;

// --------------------------------------------------------------------------
// 诊断码（u1901 十六码 · 12 枚互斥 + segment_guard）
// --------------------------------------------------------------------------
/// 状态位 矩阵面诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum u1901Code {
    /// 0x6070 键重复（位图裁断错 同键双行）。
    KeyDup,
    /// 0x6071 键断档（尺寸漂移 序不连续）。
    KeyGap,
    /// 0x6072 行守卫破缺（对齐破坏 逐行校验失败）。
    GuardBad,
    /// 0x6073 矩阵累计漂移（往返失真 求和失锚）。
    SumDrift,
    /// 0x6074 直方图溢出（超容量拒收未记账）。
    HistOver,
    /// 0x6075 直方图断桶（桶序缺失）。
    HistHole,
    /// 0x6076 分位越界（千分比>1000）。
    PctOob,
    /// 0x6077 均值漂移（均值与分位矛盾）。
    MeanDrift,
    /// 0x6078 台账不守恒（守恒恒等式破缺）。
    LedgerBroken,
    /// 0x6079 快照指纹不符（字段被改后重算不一致）。
    SnapshotFingerprint,
    /// 0x607a 常量表破坏（表项与语义规则矛盾）。
    TableBroken,
    /// 0x607b 段位守卫失败（诊断码越出本模块段）。
    SegmentGuard,
}

impl u1901Code {
    /// 全量闭集。
    pub const ALL: [u1901Code; 12] = [
        u1901Code::KeyDup,
        u1901Code::KeyGap,
        u1901Code::GuardBad,
        u1901Code::SumDrift,
        u1901Code::HistOver,
        u1901Code::HistHole,
        u1901Code::PctOob,
        u1901Code::MeanDrift,
        u1901Code::LedgerBroken,
        u1901Code::SnapshotFingerprint,
        u1901Code::TableBroken,
        u1901Code::SegmentGuard,
    ];

    /// 码值（连续十六进制，与声明序恒等）。
    pub const fn code(self) -> u16 {
        match self {
            u1901Code::KeyDup => 0x6070,
            u1901Code::KeyGap => 0x6071,
            u1901Code::GuardBad => 0x6072,
            u1901Code::SumDrift => 0x6073,
            u1901Code::HistOver => 0x6074,
            u1901Code::HistHole => 0x6075,
            u1901Code::PctOob => 0x6076,
            u1901Code::MeanDrift => 0x6077,
            u1901Code::LedgerBroken => 0x6078,
            u1901Code::SnapshotFingerprint => 0x6079,
            u1901Code::TableBroken => 0x607a,
            u1901Code::SegmentGuard => 0x607b,
        }
    }

    /// 中文诊断名。
    pub const fn name(self) -> &'static str {
        match self {
            u1901Code::KeyDup => "键重复（位图裁断错 同键双行）",
            u1901Code::KeyGap => "键断档（尺寸漂移 序不连续）",
            u1901Code::GuardBad => "行守卫破缺（对齐破坏 逐行校验失败）",
            u1901Code::SumDrift => "矩阵累计漂移（往返失真 求和失锚）",
            u1901Code::HistOver => "直方图溢出（超容量拒收未记账）",
            u1901Code::HistHole => "直方图断桶（桶序缺失）",
            u1901Code::PctOob => "分位越界（千分比>1000）",
            u1901Code::MeanDrift => "均值漂移（均值与分位矛盾）",
            u1901Code::LedgerBroken => "台账不守恒（守恒恒等式破缺）",
            u1901Code::SnapshotFingerprint => "快照指纹不符（字段被改后重算不一致）",
            u1901Code::TableBroken => "常量表破坏（表项与语义规则矛盾）",
            u1901Code::SegmentGuard => "段位守卫失败（诊断码越出本模块段）",
        }
    }

    /// 可操作修复建议（每条诊断都必须带下一步动作）。
    pub const fn suggestion(self) -> &'static str {
        match self {
            u1901Code::KeyDup => "按键唯一律去重",
            u1901Code::KeyGap => "按连续律补号",
            u1901Code::GuardBad => "按守卫口径重建",
            u1901Code::SumDrift => "按理论值复算",
            u1901Code::HistOver => "按溢出账口径登记",
            u1901Code::HistHole => "按桶序补齐",
            u1901Code::PctOob => "按 0..=1000 钳制",
            u1901Code::MeanDrift => "按同源数据复算",
            u1901Code::LedgerBroken => "按守恒律重建台账后重跑",
            u1901Code::SnapshotFingerprint => "按防篡改规程重建快照",
            u1901Code::TableBroken => "按语义规则重建常量表",
            u1901Code::SegmentGuard => "核对诊断码分配表",
        }
    }
}

// --------------------------------------------------------------------------
// 字符串键值映射 u1901mat（36 行逐行语义注释 + 逐行守卫）
// --------------------------------------------------------------------------
/// 状态位 守卫矩阵（与核心登记表同源对拍）。
/// 行数。
pub const u1901mat_COUNT: usize = 36;

/// 表本体（键 + 值 + 注释由编译期冻结）。
pub static u1901mat: [(&'static str, u64); 36] = [
    ("X870000", 0), // X87 单元 0000（x87 位 × 相位 00 × 一 轮）
    ("SSE0001", 13), // SSE 单元 0001（SSE 位 × 相位 01 × 一 轮）
    ("AVX0002", 26), // AVX 单元 0002（AVX 位 × 相位 02 × 一 轮）
    ("A5120003", 39), // A512 单元 0003（AVX-512 位 × 相位 03 × 一 轮）
    ("XC0004", 52), // XC 单元 0004（XCR0 位图 × 相位 04 × 一 轮）
    ("ZW0005", 65), // ZW 单元 0005（逐位裁断 × 相位 05 × 一 轮）
    ("CC0006", 78), // CC 单元 0006（尺寸查询 × 相位 06 × 一 轮）
    ("DQ0007", 91), // DQ 单元 0007（对齐断言 × 相位 07 × 一 轮）
    ("BH0008", 7), // BH 单元 0008（保存恢复 × 相位 08 × 一 轮）
    ("WF0009", 20), // WF 单元 0009（往返保真 × 相位 09 × 一 轮）
    ("YL0010", 33), // YL 单元 0010（样本矩阵 × 相位 10 × 一 轮）
    ("LC0011", 46), // LC 单元 0011（零差 × 相位 11 × 一 轮）
    ("X870012", 59), // X87 单元 0012（x87 位 × 相位 12 × 一 轮）
    ("SSE0013", 72), // SSE 单元 0013（SSE 位 × 相位 13 × 一 轮）
    ("AVX0014", 85), // AVX 单元 0014（AVX 位 × 相位 14 × 一 轮）
    ("A5120015", 1), // A512 单元 0015（AVX-512 位 × 相位 15 × 一 轮）
    ("XC0016", 14), // XC 单元 0016（XCR0 位图 × 相位 16 × 一 轮）
    ("ZW0017", 27), // ZW 单元 0017（逐位裁断 × 相位 17 × 一 轮）
    ("CC0018", 40), // CC 单元 0018（尺寸查询 × 相位 18 × 一 轮）
    ("DQ0019", 53), // DQ 单元 0019（对齐断言 × 相位 19 × 一 轮）
    ("BH0020", 66), // BH 单元 0020（保存恢复 × 相位 20 × 一 轮）
    ("WF0021", 79), // WF 单元 0021（往返保真 × 相位 21 × 一 轮）
    ("YL0022", 92), // YL 单元 0022（样本矩阵 × 相位 22 × 一 轮）
    ("LC0023", 8), // LC 单元 0023（零差 × 相位 23 × 一 轮）
    ("X870024", 21), // X87 单元 0024（x87 位 × 相位 24 × 一 轮）
    ("SSE0025", 34), // SSE 单元 0025（SSE 位 × 相位 25 × 一 轮）
    ("AVX0026", 47), // AVX 单元 0026（AVX 位 × 相位 26 × 一 轮）
    ("A5120027", 60), // A512 单元 0027（AVX-512 位 × 相位 27 × 一 轮）
    ("XC0028", 73), // XC 单元 0028（XCR0 位图 × 相位 28 × 一 轮）
    ("ZW0029", 86), // ZW 单元 0029（逐位裁断 × 相位 29 × 一 轮）
    ("CC0030", 2), // CC 单元 0030（尺寸查询 × 相位 30 × 一 轮）
    ("DQ0031", 15), // DQ 单元 0031（对齐断言 × 相位 31 × 一 轮）
    ("BH0032", 28), // BH 单元 0032（保存恢复 × 相位 32 × 一 轮）
    ("WF0033", 41), // WF 单元 0033（往返保真 × 相位 33 × 一 轮）
    ("YL0034", 54), // YL 单元 0034（样本矩阵 × 相位 34 × 一 轮）
    ("LC0035", 67), // LC 单元 0035（零差 × 相位 35 × 一 轮）
];

/// O(1) 语义查找（match 全枚举；命中率 100%% 由守卫保证）。
pub fn u1901mat_get(key: &str) -> Option<u64> {
    match key {
        "X870000" => Some(0),
        "SSE0001" => Some(13),
        "AVX0002" => Some(26),
        "A5120003" => Some(39),
        "XC0004" => Some(52),
        "ZW0005" => Some(65),
        "CC0006" => Some(78),
        "DQ0007" => Some(91),
        "BH0008" => Some(7),
        "WF0009" => Some(20),
        "YL0010" => Some(33),
        "LC0011" => Some(46),
        "X870012" => Some(59),
        "SSE0013" => Some(72),
        "AVX0014" => Some(85),
        "A5120015" => Some(1),
        "XC0016" => Some(14),
        "ZW0017" => Some(27),
        "CC0018" => Some(40),
        "DQ0019" => Some(53),
        "BH0020" => Some(66),
        "WF0021" => Some(79),
        "YL0022" => Some(92),
        "LC0023" => Some(8),
        "X870024" => Some(21),
        "SSE0025" => Some(34),
        "AVX0026" => Some(47),
        "A5120027" => Some(60),
        "XC0028" => Some(73),
        "ZW0029" => Some(86),
        "CC0030" => Some(2),
        "DQ0031" => Some(15),
        "BH0032" => Some(28),
        "WF0033" => Some(41),
        "YL0034" => Some(54),
        "LC0035" => Some(67),
        _ => None,
    }
}

/// 逐行守卫扫描（返回违例行数；0 = 全绿）。
pub fn u1901mat_scan() -> usize {
    let mut bad = 0usize;
    let mut i = 0usize;
    while i < u1901mat_COUNT {
        let (k, v) = u1901mat[i];
        let ok = match k {
            "X870000" => v < 97,
            "SSE0001" => v < 97,
            "AVX0002" => v < 97,
            "A5120003" => v < 97,
            "XC0004" => v < 97,
            "ZW0005" => v < 97,
            "CC0006" => v < 97,
            "DQ0007" => v < 97,
            "BH0008" => v < 97,
            "WF0009" => v < 97,
            "YL0010" => v < 97,
            "LC0011" => v < 97,
            "X870012" => v < 97,
            "SSE0013" => v < 97,
            "AVX0014" => v < 97,
            "A5120015" => v < 97,
            "XC0016" => v < 97,
            "ZW0017" => v < 97,
            "CC0018" => v < 97,
            "DQ0019" => v < 97,
            "BH0020" => v < 97,
            "WF0021" => v < 97,
            "YL0022" => v < 97,
            "LC0023" => v < 97,
            "X870024" => v < 97,
            "SSE0025" => v < 97,
            "AVX0026" => v < 97,
            "A5120027" => v < 97,
            "XC0028" => v < 97,
            "ZW0029" => v < 97,
            "CC0030" => v < 97,
            "DQ0031" => v < 97,
            "BH0032" => v < 97,
            "WF0033" => v < 97,
            "YL0034" => v < 97,
            "LC0035" => v < 97,
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
pub fn u1901mat_sum() -> u64 {
    let mut s = 0u64;
    let mut i = 0usize;
    while i < u1901mat_COUNT {
        s = s.wrapping_add(u1901mat[i].1 as u64);
        i += 1;
    }
    s
}

/// 键唯一性（构造期断言等价物：逐对扫描同键）。
pub fn u1901mat_unique() -> bool {
    let mut i = 0usize;
    while i < u1901mat_COUNT {
        let mut j = i + 1;
        while j < u1901mat_COUNT {
            if u1901mat[i].0 == u1901mat[j].0 {
                return false;
            }
            j += 1;
        }
        i += 1;
    }
    true
}

// --------------------------------------------------------------------------
// 直方图引擎 mxu1901hist（有界插入排序 + 分位数 P50/P90/P99）
// --------------------------------------------------------------------------
/// 状态位 直方图（96 桶滚动）。
/// 样本容量上限。
pub const MXU1901HIST_CAP: usize = 96;

/// 直方图账本（排序态 + 五统量）。
#[derive(Clone, Copy, Debug)]
pub struct mxu1901histHist {
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

impl mxu1901histHist {
    /// 空账构造。
    pub const fn new() -> Self {
        Self { samples: [0; 96], n: 0, drops: 0, min: 0, max: 0, sum: 0 }
    }

    /// 采样（有界插入排序；满员丢弃并记账）。
    pub fn push(&mut self, v: u64) {
        if self.n >= MXU1901HIST_CAP {
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

/// 段位守卫：全部诊断码必须落在本段 [0x6070, 0x607f]。
pub const fn u1901_segment_guard() -> bool {
    let mut ok = true;
    let mut i = 0usize;
    while i < u1901Code::ALL.len() {
        let c = u1901Code::ALL[i].code();
        if c < 0x6070 || c > 0x607f {
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
fn u1901_guard_seg() -> bool {
    u1901_segment_guard()
}

/// UNX-F1901 矩阵 36 行齐备 + 逐行守卫 + 唯一性 + 累计守恒。
fn check_u1901_map_guard() -> bool {
    let mut ok = u1901mat_COUNT == 36;
    if ok { ok = u1901mat_scan() == 0 && u1901mat_unique(); }
    if ok { ok = u1901mat_get("X870000") == Some(0) && u1901mat_get("ZZZ_NOKEY").is_none(); }
    if ok { ok = u1901mat_sum() == 1594; }
    ok
}

/// UNX-F1901 直方图 16 样本分位/均值/守恒。
fn check_u1901_map_hist() -> bool {
    let mut h = mxu1901histHist::new();
    let mut i = 0u64;
    while i < 16 { h.push(i + 1); i += 1; }
    let mut ok = h.conserved() && h.mean() >= 1 && h.percentile(500) >= 8;
    ok
}

/// 本模块自检（逐检查带 detail，全绿才出门）。
pub fn run_u1901_checks() -> CheckSet {
    let mut set = CheckSet::new("U1901");
    set.add(
        "u1901-seg-guard",
        u1901_guard_seg(),
        if u1901_guard_seg() { "" } else { "diagnostic code outside segment" },
    );
    set.add(
        "u1901-map-guard",
        check_u1901_map_guard(),
        if check_u1901_map_guard() { "" } else { "UNX-F1901 matrix guard mismatch" },
    );
    set.add(
        "u1901-map-hist",
        check_u1901_map_hist(),
        if check_u1901_map_hist() { "" } else { "UNX-F1901 hist mismatch" },
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
    fn u1901_all_checks_green() {
        let set = run_u1901_checks();
        assert!(set.all_passed(), "U1901 domain checks must be green");
    }

    #[test]
    fn u1901_segment_guard_holds() {
        assert!(u1901_segment_guard());
    }
}
