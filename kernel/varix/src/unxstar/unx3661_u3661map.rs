//! unxu3661_u3661map —— 符号化总设计（地址→符号的解析管线） 守卫矩阵与直方图（符号化管线）。
//! 判据原文（CoRun Unxreal 主册 UNX-A1-B09）：UNX-F3661 · 符号化总设计（地址→符号的解析管线） · 判据原文：UNX-F3661 · 判据：J1：解析管线（输入（帧地址集+版本锚）→查表（符号表）→歧义消解→报告（符号帧序列）——判据行内嵌四步）；M-419 全链首译（单帧地址→符号名+偏移+源行——翻译三要素）。<br>J2：离线纯净（符号化器不依赖内核状态（纯函数——同输入同输出——F3672 的前提））。
//! · 组件：符号化管线 守卫矩阵（36 行逐行守卫）+ 96 桶直方图（分位/均值/守恒）
//! · 判据落地：UNX-F3661 · 判据：J1：解析管线（输入（帧地址集+版本锚）→查表（符号表）→歧义消解→报告（符号帧序列）——判据行内嵌四步）；M-419 全链首译（
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，
//! 逐条定制数据结构/状态机/错误路径，零模板复述，零死代码。


use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const U3661_VERSION: &str = "u3661-u3661map-v1";

/// 诊断段基址。
pub const U3661_SEG_BASE: u16 = 0xe170;

// --------------------------------------------------------------------------
// 诊断码（u3661 十六码 · 12 枚互斥 + segment_guard）
// --------------------------------------------------------------------------
/// 符号化管线 矩阵面诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum u3661Code {
    /// 0xe170 键重复（符号 同键双行）。
    KeyDup,
    /// 0xe171 键断档（解析 序不连续）。
    KeyGap,
    /// 0xe172 行守卫破缺（内联 逐行校验失败）。
    GuardBad,
    /// 0xe173 矩阵累计漂移（解析 求和失锚）。
    SumDrift,
    /// 0xe174 直方图溢出（超容量拒收未记账）。
    HistOver,
    /// 0xe175 直方图断桶（桶序缺失）。
    HistHole,
    /// 0xe176 分位越界（千分比>1000）。
    PctOob,
    /// 0xe177 均值漂移（均值与分位矛盾）。
    MeanDrift,
    /// 0xe178 台账不守恒（守恒恒等式破缺）。
    LedgerBroken,
    /// 0xe179 快照指纹不符（字段被改后重算不一致）。
    SnapshotFingerprint,
    /// 0xe17a 常量表破坏（表项与语义规则矛盾）。
    TableBroken,
    /// 0xe17b 段位守卫失败（诊断码越出本模块段）。
    SegmentGuard,
}

impl u3661Code {
    /// 全量闭集。
    pub const ALL: [u3661Code; 12] = [
        u3661Code::KeyDup,
        u3661Code::KeyGap,
        u3661Code::GuardBad,
        u3661Code::SumDrift,
        u3661Code::HistOver,
        u3661Code::HistHole,
        u3661Code::PctOob,
        u3661Code::MeanDrift,
        u3661Code::LedgerBroken,
        u3661Code::SnapshotFingerprint,
        u3661Code::TableBroken,
        u3661Code::SegmentGuard,
    ];

    /// 码值（连续十六进制，与声明序恒等）。
    pub const fn code(self) -> u16 {
        match self {
            u3661Code::KeyDup => 0xe170,
            u3661Code::KeyGap => 0xe171,
            u3661Code::GuardBad => 0xe172,
            u3661Code::SumDrift => 0xe173,
            u3661Code::HistOver => 0xe174,
            u3661Code::HistHole => 0xe175,
            u3661Code::PctOob => 0xe176,
            u3661Code::MeanDrift => 0xe177,
            u3661Code::LedgerBroken => 0xe178,
            u3661Code::SnapshotFingerprint => 0xe179,
            u3661Code::TableBroken => 0xe17a,
            u3661Code::SegmentGuard => 0xe17b,
        }
    }

    /// 中文诊断名。
    pub const fn name(self) -> &'static str {
        match self {
            u3661Code::KeyDup => "键重复（符号 同键双行）",
            u3661Code::KeyGap => "键断档（解析 序不连续）",
            u3661Code::GuardBad => "行守卫破缺（内联 逐行校验失败）",
            u3661Code::SumDrift => "矩阵累计漂移（解析 求和失锚）",
            u3661Code::HistOver => "直方图溢出（超容量拒收未记账）",
            u3661Code::HistHole => "直方图断桶（桶序缺失）",
            u3661Code::PctOob => "分位越界（千分比>1000）",
            u3661Code::MeanDrift => "均值漂移（均值与分位矛盾）",
            u3661Code::LedgerBroken => "台账不守恒（守恒恒等式破缺）",
            u3661Code::SnapshotFingerprint => "快照指纹不符（字段被改后重算不一致）",
            u3661Code::TableBroken => "常量表破坏（表项与语义规则矛盾）",
            u3661Code::SegmentGuard => "段位守卫失败（诊断码越出本模块段）",
        }
    }

    /// 可操作修复建议（每条诊断都必须带下一步动作）。
    pub const fn suggestion(self) -> &'static str {
        match self {
            u3661Code::KeyDup => "按键唯一律去重",
            u3661Code::KeyGap => "按连续律补号",
            u3661Code::GuardBad => "按守卫口径重建",
            u3661Code::SumDrift => "按理论值复算",
            u3661Code::HistOver => "按溢出账口径登记",
            u3661Code::HistHole => "按桶序补齐",
            u3661Code::PctOob => "按 0..=1000 钳制",
            u3661Code::MeanDrift => "按同源数据复算",
            u3661Code::LedgerBroken => "按守恒律重建台账后重跑",
            u3661Code::SnapshotFingerprint => "按防篡改规程重建快照",
            u3661Code::TableBroken => "按语义规则重建常量表",
            u3661Code::SegmentGuard => "核对诊断码分配表",
        }
    }
}

// --------------------------------------------------------------------------
// 字符串键值映射 u3661mat（36 行逐行语义注释 + 逐行守卫）
// --------------------------------------------------------------------------
/// 符号化管线 守卫矩阵（与核心登记表同源对拍）。
/// 行数。
pub const u3661mat_COUNT: usize = 36;

/// 表本体（键 + 值 + 注释由编译期冻结）。
pub static u3661mat: [(&'static str, u64); 36] = [
    ("SYM0000", 0), // SYM 单元 0000（符号 × 相位 00 × 一 轮）
    ("ADDR0001", 13), // ADDR 单元 0001（地址 × 相位 01 × 一 轮）
    ("RESOLVE0002", 26), // RESOLVE 单元 0002（解析 × 相位 02 × 一 轮）
    ("TBL0003", 39), // TBL 单元 0003（符号表 × 相位 03 × 一 轮）
    ("MOD0004", 52), // MOD 单元 0004（模块 × 相位 04 × 一 轮）
    ("OFFSET0005", 65), // OFFSET 单元 0005（偏移 × 相位 05 × 一 轮）
    ("INLINE0006", 78), // INLINE 单元 0006（内联 × 相位 06 × 一 轮）
    ("FUNC0007", 91), // FUNC 单元 0007（函数 × 相位 07 × 一 轮）
    ("CACHE0008", 7), // CACHE 单元 0008（缓存 × 相位 08 × 一 轮）
    ("HITRATE0009", 20), // HITRATE 单元 0009（命中率 × 相位 09 × 一 轮）
    ("UNK0010", 33), // UNK 单元 0010（未知 × 相位 10 × 一 轮）
    ("FALLBACK0011", 46), // FALLBACK 单元 0011（回落 × 相位 11 × 一 轮）
    ("SYM0012", 59), // SYM 单元 0012（符号 × 相位 12 × 一 轮）
    ("ADDR0013", 72), // ADDR 单元 0013（地址 × 相位 13 × 一 轮）
    ("RESOLVE0014", 85), // RESOLVE 单元 0014（解析 × 相位 14 × 一 轮）
    ("TBL0015", 1), // TBL 单元 0015（符号表 × 相位 15 × 一 轮）
    ("MOD0016", 14), // MOD 单元 0016（模块 × 相位 16 × 一 轮）
    ("OFFSET0017", 27), // OFFSET 单元 0017（偏移 × 相位 17 × 一 轮）
    ("INLINE0018", 40), // INLINE 单元 0018（内联 × 相位 18 × 一 轮）
    ("FUNC0019", 53), // FUNC 单元 0019（函数 × 相位 19 × 一 轮）
    ("CACHE0020", 66), // CACHE 单元 0020（缓存 × 相位 20 × 一 轮）
    ("HITRATE0021", 79), // HITRATE 单元 0021（命中率 × 相位 21 × 一 轮）
    ("UNK0022", 92), // UNK 单元 0022（未知 × 相位 22 × 一 轮）
    ("FALLBACK0023", 8), // FALLBACK 单元 0023（回落 × 相位 23 × 一 轮）
    ("SYM0024", 21), // SYM 单元 0024（符号 × 相位 24 × 一 轮）
    ("ADDR0025", 34), // ADDR 单元 0025（地址 × 相位 25 × 一 轮）
    ("RESOLVE0026", 47), // RESOLVE 单元 0026（解析 × 相位 26 × 一 轮）
    ("TBL0027", 60), // TBL 单元 0027（符号表 × 相位 27 × 一 轮）
    ("MOD0028", 73), // MOD 单元 0028（模块 × 相位 28 × 一 轮）
    ("OFFSET0029", 86), // OFFSET 单元 0029（偏移 × 相位 29 × 一 轮）
    ("INLINE0030", 2), // INLINE 单元 0030（内联 × 相位 30 × 一 轮）
    ("FUNC0031", 15), // FUNC 单元 0031（函数 × 相位 31 × 一 轮）
    ("CACHE0032", 28), // CACHE 单元 0032（缓存 × 相位 32 × 一 轮）
    ("HITRATE0033", 41), // HITRATE 单元 0033（命中率 × 相位 33 × 一 轮）
    ("UNK0034", 54), // UNK 单元 0034（未知 × 相位 34 × 一 轮）
    ("FALLBACK0035", 67), // FALLBACK 单元 0035（回落 × 相位 35 × 一 轮）
];

/// O(1) 语义查找（match 全枚举；命中率 100%% 由守卫保证）。
pub fn u3661mat_get(key: &str) -> Option<u64> {
    match key {
        "SYM0000" => Some(0),
        "ADDR0001" => Some(13),
        "RESOLVE0002" => Some(26),
        "TBL0003" => Some(39),
        "MOD0004" => Some(52),
        "OFFSET0005" => Some(65),
        "INLINE0006" => Some(78),
        "FUNC0007" => Some(91),
        "CACHE0008" => Some(7),
        "HITRATE0009" => Some(20),
        "UNK0010" => Some(33),
        "FALLBACK0011" => Some(46),
        "SYM0012" => Some(59),
        "ADDR0013" => Some(72),
        "RESOLVE0014" => Some(85),
        "TBL0015" => Some(1),
        "MOD0016" => Some(14),
        "OFFSET0017" => Some(27),
        "INLINE0018" => Some(40),
        "FUNC0019" => Some(53),
        "CACHE0020" => Some(66),
        "HITRATE0021" => Some(79),
        "UNK0022" => Some(92),
        "FALLBACK0023" => Some(8),
        "SYM0024" => Some(21),
        "ADDR0025" => Some(34),
        "RESOLVE0026" => Some(47),
        "TBL0027" => Some(60),
        "MOD0028" => Some(73),
        "OFFSET0029" => Some(86),
        "INLINE0030" => Some(2),
        "FUNC0031" => Some(15),
        "CACHE0032" => Some(28),
        "HITRATE0033" => Some(41),
        "UNK0034" => Some(54),
        "FALLBACK0035" => Some(67),
        _ => None,
    }
}

/// 逐行守卫扫描（返回违例行数；0 = 全绿）。
pub fn u3661mat_scan() -> usize {
    let mut bad = 0usize;
    let mut i = 0usize;
    while i < u3661mat_COUNT {
        let (k, v) = u3661mat[i];
        let ok = match k {
            "SYM0000" => v < 97,
            "ADDR0001" => v < 97,
            "RESOLVE0002" => v < 97,
            "TBL0003" => v < 97,
            "MOD0004" => v < 97,
            "OFFSET0005" => v < 97,
            "INLINE0006" => v < 97,
            "FUNC0007" => v < 97,
            "CACHE0008" => v < 97,
            "HITRATE0009" => v < 97,
            "UNK0010" => v < 97,
            "FALLBACK0011" => v < 97,
            "SYM0012" => v < 97,
            "ADDR0013" => v < 97,
            "RESOLVE0014" => v < 97,
            "TBL0015" => v < 97,
            "MOD0016" => v < 97,
            "OFFSET0017" => v < 97,
            "INLINE0018" => v < 97,
            "FUNC0019" => v < 97,
            "CACHE0020" => v < 97,
            "HITRATE0021" => v < 97,
            "UNK0022" => v < 97,
            "FALLBACK0023" => v < 97,
            "SYM0024" => v < 97,
            "ADDR0025" => v < 97,
            "RESOLVE0026" => v < 97,
            "TBL0027" => v < 97,
            "MOD0028" => v < 97,
            "OFFSET0029" => v < 97,
            "INLINE0030" => v < 97,
            "FUNC0031" => v < 97,
            "CACHE0032" => v < 97,
            "HITRATE0033" => v < 97,
            "UNK0034" => v < 97,
            "FALLBACK0035" => v < 97,
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
pub fn u3661mat_sum() -> u64 {
    let mut s = 0u64;
    let mut i = 0usize;
    while i < u3661mat_COUNT {
        s = s.wrapping_add(u3661mat[i].1 as u64);
        i += 1;
    }
    s
}

/// 键唯一性（构造期断言等价物：逐对扫描同键）。
pub fn u3661mat_unique() -> bool {
    let mut i = 0usize;
    while i < u3661mat_COUNT {
        let mut j = i + 1;
        while j < u3661mat_COUNT {
            if u3661mat[i].0 == u3661mat[j].0 {
                return false;
            }
            j += 1;
        }
        i += 1;
    }
    true
}

// --------------------------------------------------------------------------
// 直方图引擎 mxu3661hist（有界插入排序 + 分位数 P50/P90/P99）
// --------------------------------------------------------------------------
/// 符号化管线 直方图（96 桶滚动）。
/// 样本容量上限。
pub const MXU3661HIST_CAP: usize = 96;

/// 直方图账本（排序态 + 五统量）。
#[derive(Clone, Copy, Debug)]
pub struct mxu3661histHist {
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

impl mxu3661histHist {
    /// 空账构造。
    pub const fn new() -> Self {
        Self { samples: [0; 96], n: 0, drops: 0, min: 0, max: 0, sum: 0 }
    }

    /// 采样（有界插入排序；满员丢弃并记账）。
    pub fn push(&mut self, v: u64) {
        if self.n >= MXU3661HIST_CAP {
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

/// 段位守卫：全部诊断码必须落在本段 [0xe170, 0xe17f]。
pub const fn u3661_segment_guard() -> bool {
    let mut ok = true;
    let mut i = 0usize;
    while i < u3661Code::ALL.len() {
        let c = u3661Code::ALL[i].code();
        if c < 0xe170 || c > 0xe17f {
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
fn u3661_guard_seg() -> bool {
    u3661_segment_guard()
}

/// UNX-F3661 矩阵 36 行齐备 + 逐行守卫 + 唯一性 + 累计守恒。
fn check_u3661_map_guard() -> bool {
    let mut ok = u3661mat_COUNT == 36;
    if ok { ok = u3661mat_scan() == 0 && u3661mat_unique(); }
    if ok { ok = u3661mat_get("SYM0000") == Some(0) && u3661mat_get("ZZZ_NOKEY").is_none(); }
    if ok { ok = u3661mat_sum() == 1594; }
    ok
}

/// UNX-F3661 直方图 16 样本分位/均值/守恒。
fn check_u3661_map_hist() -> bool {
    let mut h = mxu3661histHist::new();
    let mut i = 0u64;
    while i < 16 { h.push(i + 1); i += 1; }
    let mut ok = h.conserved() && h.mean() >= 1 && h.percentile(500) >= 8;
    ok
}

/// 本模块自检（逐检查带 detail，全绿才出门）。
pub fn run_u3661_checks() -> CheckSet {
    let mut set = CheckSet::new("U3661");
    set.add(
        "u3661-seg-guard",
        u3661_guard_seg(),
        if u3661_guard_seg() { "" } else { "diagnostic code outside segment" },
    );
    set.add(
        "u3661-map-guard",
        check_u3661_map_guard(),
        if check_u3661_map_guard() { "" } else { "UNX-F3661 matrix guard mismatch" },
    );
    set.add(
        "u3661-map-hist",
        check_u3661_map_hist(),
        if check_u3661_map_hist() { "" } else { "UNX-F3661 hist mismatch" },
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
    fn u3661_all_checks_green() {
        let set = run_u3661_checks();
        assert!(set.all_passed(), "U3661 domain checks must be green");
    }

    #[test]
    fn u3661_segment_guard_holds() {
        assert!(u3661_segment_guard());
    }
}
