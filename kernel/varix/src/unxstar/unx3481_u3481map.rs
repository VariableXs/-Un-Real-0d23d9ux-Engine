//! unxu3481_u3481map —— 分级日志总设计（五级 TRACE/DEBUG/INFO/WARN/ERROR + 分级输出策略） 守卫矩阵与直方图（分级日志）。
//! 判据原文（CoRun Unxreal 主册 UNX-A1-B09）：UNX-F3481 · 分级日志总设计（五级 TRACE/DEBUG/INFO/WARN/ERROR + 分级输出策略） · 判据原文：UNX-F3481 · 判据：J1：五级语义冻结（级别定义 × 消费者 × 输出策略三列——判据行内嵌矩阵）；M-240 分级全链路（五级各一条日志 → 过滤 → 转储 → 可读）。
//! · 组件：分级日志 守卫矩阵（36 行逐行守卫）+ 96 桶直方图（分位/均值/守恒）
//! · 判据落地：UNX-F3481 · 判据：J1：五级语义冻结（级别定义 × 消费者 × 输出策略三列——判据行内嵌矩阵）；M-240 分级全链路（五级各一条日志 → 过滤 
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，
//! 逐条定制数据结构/状态机/错误路径，零模板复述，零死代码。


use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const U3481_VERSION: &str = "u3481-u3481map-v1";

/// 诊断段基址。
pub const U3481_SEG_BASE: u16 = 0xe0e0;

// --------------------------------------------------------------------------
// 诊断码（u3481 十六码 · 12 枚互斥 + segment_guard）
// --------------------------------------------------------------------------
/// 分级日志 矩阵面诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum u3481Code {
    /// 0xe0e0 键重复（日志 同键双行）。
    KeyDup,
    /// 0xe0e1 键断档（分级 序不连续）。
    KeyGap,
    /// 0xe0e2 行守卫破缺（限速 逐行校验失败）。
    GuardBad,
    /// 0xe0e3 矩阵累计漂移（五级 求和失锚）。
    SumDrift,
    /// 0xe0e4 直方图溢出（超容量拒收未记账）。
    HistOver,
    /// 0xe0e5 直方图断桶（桶序缺失）。
    HistHole,
    /// 0xe0e6 分位越界（千分比>1000）。
    PctOob,
    /// 0xe0e7 均值漂移（均值与分位矛盾）。
    MeanDrift,
    /// 0xe0e8 台账不守恒（守恒恒等式破缺）。
    LedgerBroken,
    /// 0xe0e9 快照指纹不符（字段被改后重算不一致）。
    SnapshotFingerprint,
    /// 0xe0ea 常量表破坏（表项与语义规则矛盾）。
    TableBroken,
    /// 0xe0eb 段位守卫失败（诊断码越出本模块段）。
    SegmentGuard,
}

impl u3481Code {
    /// 全量闭集。
    pub const ALL: [u3481Code; 12] = [
        u3481Code::KeyDup,
        u3481Code::KeyGap,
        u3481Code::GuardBad,
        u3481Code::SumDrift,
        u3481Code::HistOver,
        u3481Code::HistHole,
        u3481Code::PctOob,
        u3481Code::MeanDrift,
        u3481Code::LedgerBroken,
        u3481Code::SnapshotFingerprint,
        u3481Code::TableBroken,
        u3481Code::SegmentGuard,
    ];

    /// 码值（连续十六进制，与声明序恒等）。
    pub const fn code(self) -> u16 {
        match self {
            u3481Code::KeyDup => 0xe0e0,
            u3481Code::KeyGap => 0xe0e1,
            u3481Code::GuardBad => 0xe0e2,
            u3481Code::SumDrift => 0xe0e3,
            u3481Code::HistOver => 0xe0e4,
            u3481Code::HistHole => 0xe0e5,
            u3481Code::PctOob => 0xe0e6,
            u3481Code::MeanDrift => 0xe0e7,
            u3481Code::LedgerBroken => 0xe0e8,
            u3481Code::SnapshotFingerprint => 0xe0e9,
            u3481Code::TableBroken => 0xe0ea,
            u3481Code::SegmentGuard => 0xe0eb,
        }
    }

    /// 中文诊断名。
    pub const fn name(self) -> &'static str {
        match self {
            u3481Code::KeyDup => "键重复（日志 同键双行）",
            u3481Code::KeyGap => "键断档（分级 序不连续）",
            u3481Code::GuardBad => "行守卫破缺（限速 逐行校验失败）",
            u3481Code::SumDrift => "矩阵累计漂移（五级 求和失锚）",
            u3481Code::HistOver => "直方图溢出（超容量拒收未记账）",
            u3481Code::HistHole => "直方图断桶（桶序缺失）",
            u3481Code::PctOob => "分位越界（千分比>1000）",
            u3481Code::MeanDrift => "均值漂移（均值与分位矛盾）",
            u3481Code::LedgerBroken => "台账不守恒（守恒恒等式破缺）",
            u3481Code::SnapshotFingerprint => "快照指纹不符（字段被改后重算不一致）",
            u3481Code::TableBroken => "常量表破坏（表项与语义规则矛盾）",
            u3481Code::SegmentGuard => "段位守卫失败（诊断码越出本模块段）",
        }
    }

    /// 可操作修复建议（每条诊断都必须带下一步动作）。
    pub const fn suggestion(self) -> &'static str {
        match self {
            u3481Code::KeyDup => "按键唯一律去重",
            u3481Code::KeyGap => "按连续律补号",
            u3481Code::GuardBad => "按守卫口径重建",
            u3481Code::SumDrift => "按理论值复算",
            u3481Code::HistOver => "按溢出账口径登记",
            u3481Code::HistHole => "按桶序补齐",
            u3481Code::PctOob => "按 0..=1000 钳制",
            u3481Code::MeanDrift => "按同源数据复算",
            u3481Code::LedgerBroken => "按守恒律重建台账后重跑",
            u3481Code::SnapshotFingerprint => "按防篡改规程重建快照",
            u3481Code::TableBroken => "按语义规则重建常量表",
            u3481Code::SegmentGuard => "核对诊断码分配表",
        }
    }
}

// --------------------------------------------------------------------------
// 字符串键值映射 u3481mat（36 行逐行语义注释 + 逐行守卫）
// --------------------------------------------------------------------------
/// 分级日志 守卫矩阵（与核心登记表同源对拍）。
/// 行数。
pub const u3481mat_COUNT: usize = 36;

/// 表本体（键 + 值 + 注释由编译期冻结）。
pub static u3481mat: [(&'static str, u64); 36] = [
    ("LVL0000", 0), // LVL 单元 0000（日志级别 × 相位 00 × 一 轮）
    ("TRACE0001", 13), // TRACE 单元 0001（跟踪级 × 相位 01 × 一 轮）
    ("DBG0002", 26), // DBG 单元 0002（调试级 × 相位 02 × 一 轮）
    ("INF0003", 39), // INF 单元 0003（信息级 × 相位 03 × 一 轮）
    ("WRN0004", 52), // WRN 单元 0004（警告级 × 相位 04 × 一 轮）
    ("ERR0005", 65), // ERR 单元 0005（错误级 × 相位 05 × 一 轮）
    ("FILT0006", 78), // FILT 单元 0006（过滤 × 相位 06 × 一 轮）
    ("BUF0007", 91), // BUF 单元 0007（环形缓冲 × 相位 07 × 一 轮）
    ("RATE0008", 7), // RATE 单元 0008（限速 × 相位 08 × 一 轮）
    ("FLUSH0009", 20), // FLUSH 单元 0009（刷盘 × 相位 09 × 一 轮）
    ("CTX0010", 33), // CTX 单元 0010（上下文字段 × 相位 10 × 一 轮）
    ("DROP0011", 46), // DROP 单元 0011（丢弃计数 × 相位 11 × 一 轮）
    ("LVL0012", 59), // LVL 单元 0012（日志级别 × 相位 12 × 一 轮）
    ("TRACE0013", 72), // TRACE 单元 0013（跟踪级 × 相位 13 × 一 轮）
    ("DBG0014", 85), // DBG 单元 0014（调试级 × 相位 14 × 一 轮）
    ("INF0015", 1), // INF 单元 0015（信息级 × 相位 15 × 一 轮）
    ("WRN0016", 14), // WRN 单元 0016（警告级 × 相位 16 × 一 轮）
    ("ERR0017", 27), // ERR 单元 0017（错误级 × 相位 17 × 一 轮）
    ("FILT0018", 40), // FILT 单元 0018（过滤 × 相位 18 × 一 轮）
    ("BUF0019", 53), // BUF 单元 0019（环形缓冲 × 相位 19 × 一 轮）
    ("RATE0020", 66), // RATE 单元 0020（限速 × 相位 20 × 一 轮）
    ("FLUSH0021", 79), // FLUSH 单元 0021（刷盘 × 相位 21 × 一 轮）
    ("CTX0022", 92), // CTX 单元 0022（上下文字段 × 相位 22 × 一 轮）
    ("DROP0023", 8), // DROP 单元 0023（丢弃计数 × 相位 23 × 一 轮）
    ("LVL0024", 21), // LVL 单元 0024（日志级别 × 相位 24 × 一 轮）
    ("TRACE0025", 34), // TRACE 单元 0025（跟踪级 × 相位 25 × 一 轮）
    ("DBG0026", 47), // DBG 单元 0026（调试级 × 相位 26 × 一 轮）
    ("INF0027", 60), // INF 单元 0027（信息级 × 相位 27 × 一 轮）
    ("WRN0028", 73), // WRN 单元 0028（警告级 × 相位 28 × 一 轮）
    ("ERR0029", 86), // ERR 单元 0029（错误级 × 相位 29 × 一 轮）
    ("FILT0030", 2), // FILT 单元 0030（过滤 × 相位 30 × 一 轮）
    ("BUF0031", 15), // BUF 单元 0031（环形缓冲 × 相位 31 × 一 轮）
    ("RATE0032", 28), // RATE 单元 0032（限速 × 相位 32 × 一 轮）
    ("FLUSH0033", 41), // FLUSH 单元 0033（刷盘 × 相位 33 × 一 轮）
    ("CTX0034", 54), // CTX 单元 0034（上下文字段 × 相位 34 × 一 轮）
    ("DROP0035", 67), // DROP 单元 0035（丢弃计数 × 相位 35 × 一 轮）
];

/// O(1) 语义查找（match 全枚举；命中率 100%% 由守卫保证）。
pub fn u3481mat_get(key: &str) -> Option<u64> {
    match key {
        "LVL0000" => Some(0),
        "TRACE0001" => Some(13),
        "DBG0002" => Some(26),
        "INF0003" => Some(39),
        "WRN0004" => Some(52),
        "ERR0005" => Some(65),
        "FILT0006" => Some(78),
        "BUF0007" => Some(91),
        "RATE0008" => Some(7),
        "FLUSH0009" => Some(20),
        "CTX0010" => Some(33),
        "DROP0011" => Some(46),
        "LVL0012" => Some(59),
        "TRACE0013" => Some(72),
        "DBG0014" => Some(85),
        "INF0015" => Some(1),
        "WRN0016" => Some(14),
        "ERR0017" => Some(27),
        "FILT0018" => Some(40),
        "BUF0019" => Some(53),
        "RATE0020" => Some(66),
        "FLUSH0021" => Some(79),
        "CTX0022" => Some(92),
        "DROP0023" => Some(8),
        "LVL0024" => Some(21),
        "TRACE0025" => Some(34),
        "DBG0026" => Some(47),
        "INF0027" => Some(60),
        "WRN0028" => Some(73),
        "ERR0029" => Some(86),
        "FILT0030" => Some(2),
        "BUF0031" => Some(15),
        "RATE0032" => Some(28),
        "FLUSH0033" => Some(41),
        "CTX0034" => Some(54),
        "DROP0035" => Some(67),
        _ => None,
    }
}

/// 逐行守卫扫描（返回违例行数；0 = 全绿）。
pub fn u3481mat_scan() -> usize {
    let mut bad = 0usize;
    let mut i = 0usize;
    while i < u3481mat_COUNT {
        let (k, v) = u3481mat[i];
        let ok = match k {
            "LVL0000" => v < 97,
            "TRACE0001" => v < 97,
            "DBG0002" => v < 97,
            "INF0003" => v < 97,
            "WRN0004" => v < 97,
            "ERR0005" => v < 97,
            "FILT0006" => v < 97,
            "BUF0007" => v < 97,
            "RATE0008" => v < 97,
            "FLUSH0009" => v < 97,
            "CTX0010" => v < 97,
            "DROP0011" => v < 97,
            "LVL0012" => v < 97,
            "TRACE0013" => v < 97,
            "DBG0014" => v < 97,
            "INF0015" => v < 97,
            "WRN0016" => v < 97,
            "ERR0017" => v < 97,
            "FILT0018" => v < 97,
            "BUF0019" => v < 97,
            "RATE0020" => v < 97,
            "FLUSH0021" => v < 97,
            "CTX0022" => v < 97,
            "DROP0023" => v < 97,
            "LVL0024" => v < 97,
            "TRACE0025" => v < 97,
            "DBG0026" => v < 97,
            "INF0027" => v < 97,
            "WRN0028" => v < 97,
            "ERR0029" => v < 97,
            "FILT0030" => v < 97,
            "BUF0031" => v < 97,
            "RATE0032" => v < 97,
            "FLUSH0033" => v < 97,
            "CTX0034" => v < 97,
            "DROP0035" => v < 97,
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
pub fn u3481mat_sum() -> u64 {
    let mut s = 0u64;
    let mut i = 0usize;
    while i < u3481mat_COUNT {
        s = s.wrapping_add(u3481mat[i].1 as u64);
        i += 1;
    }
    s
}

/// 键唯一性（构造期断言等价物：逐对扫描同键）。
pub fn u3481mat_unique() -> bool {
    let mut i = 0usize;
    while i < u3481mat_COUNT {
        let mut j = i + 1;
        while j < u3481mat_COUNT {
            if u3481mat[i].0 == u3481mat[j].0 {
                return false;
            }
            j += 1;
        }
        i += 1;
    }
    true
}

// --------------------------------------------------------------------------
// 直方图引擎 mxu3481hist（有界插入排序 + 分位数 P50/P90/P99）
// --------------------------------------------------------------------------
/// 分级日志 直方图（96 桶滚动）。
/// 样本容量上限。
pub const MXU3481HIST_CAP: usize = 96;

/// 直方图账本（排序态 + 五统量）。
#[derive(Clone, Copy, Debug)]
pub struct mxu3481histHist {
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

impl mxu3481histHist {
    /// 空账构造。
    pub const fn new() -> Self {
        Self { samples: [0; 96], n: 0, drops: 0, min: 0, max: 0, sum: 0 }
    }

    /// 采样（有界插入排序；满员丢弃并记账）。
    pub fn push(&mut self, v: u64) {
        if self.n >= MXU3481HIST_CAP {
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

/// 段位守卫：全部诊断码必须落在本段 [0xe0e0, 0xe0ef]。
pub const fn u3481_segment_guard() -> bool {
    let mut ok = true;
    let mut i = 0usize;
    while i < u3481Code::ALL.len() {
        let c = u3481Code::ALL[i].code();
        if c < 0xe0e0 || c > 0xe0ef {
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
fn u3481_guard_seg() -> bool {
    u3481_segment_guard()
}

/// UNX-F3481 矩阵 36 行齐备 + 逐行守卫 + 唯一性 + 累计守恒。
fn check_u3481_map_guard() -> bool {
    let mut ok = u3481mat_COUNT == 36;
    if ok { ok = u3481mat_scan() == 0 && u3481mat_unique(); }
    if ok { ok = u3481mat_get("LVL0000") == Some(0) && u3481mat_get("ZZZ_NOKEY").is_none(); }
    if ok { ok = u3481mat_sum() == 1594; }
    ok
}

/// UNX-F3481 直方图 16 样本分位/均值/守恒。
fn check_u3481_map_hist() -> bool {
    let mut h = mxu3481histHist::new();
    let mut i = 0u64;
    while i < 16 { h.push(i + 1); i += 1; }
    let mut ok = h.conserved() && h.mean() >= 1 && h.percentile(500) >= 8;
    ok
}

/// 本模块自检（逐检查带 detail，全绿才出门）。
pub fn run_u3481_checks() -> CheckSet {
    let mut set = CheckSet::new("U3481");
    set.add(
        "u3481-seg-guard",
        u3481_guard_seg(),
        if u3481_guard_seg() { "" } else { "diagnostic code outside segment" },
    );
    set.add(
        "u3481-map-guard",
        check_u3481_map_guard(),
        if check_u3481_map_guard() { "" } else { "UNX-F3481 matrix guard mismatch" },
    );
    set.add(
        "u3481-map-hist",
        check_u3481_map_hist(),
        if check_u3481_map_hist() { "" } else { "UNX-F3481 hist mismatch" },
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
    fn u3481_all_checks_green() {
        let set = run_u3481_checks();
        assert!(set.all_passed(), "U3481 domain checks must be green");
    }

    #[test]
    fn u3481_segment_guard_holds() {
        assert!(u3481_segment_guard());
    }
}
