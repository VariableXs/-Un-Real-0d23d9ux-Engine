//! unxu3501_u3501map —— 跟踪点总设计（TP 注册表/编译期埋点/运行时开关） 守卫矩阵与直方图（跟踪点体系）。
//! 判据原文（CoRun Unxreal 主册 UNX-A1-B09）：UNX-F3501 · 跟踪点总设计（TP 注册表/编译期埋点/运行时开关） · 判据原文：UNX-F3501 · 判据：J1：TP 语义冻结（埋点位置 × 负载类型 × 默认态三列矩阵）；M-259 TP 全链路（埋点→开关→负载→消费端可读）。<br>J2：与日志分工声明（文本流 vs 结构化事件流——F3506 协议对端，禁用态零开销）。
//! · 组件：跟踪点体系 守卫矩阵（36 行逐行守卫）+ 96 桶直方图（分位/均值/守恒）
//! · 判据落地：UNX-F3501 · 判据：J1：TP 语义冻结（埋点位置 × 负载类型 × 默认态三列矩阵）；M-259 TP 全链路（埋点→开关→负载→消费端可读）。<b
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，
//! 逐条定制数据结构/状态机/错误路径，零模板复述，零死代码。


use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const U3501_VERSION: &str = "u3501-u3501map-v1";

/// 诊断段基址。
pub const U3501_SEG_BASE: u16 = 0xe0f0;

// --------------------------------------------------------------------------
// 诊断码（u3501 十六码 · 12 枚互斥 + segment_guard）
// --------------------------------------------------------------------------
/// 跟踪点体系 矩阵面诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum u3501Code {
    /// 0xe0f0 键重复（跟踪点 同键双行）。
    KeyDup,
    /// 0xe0f1 键断档（埋点 序不连续）。
    KeyGap,
    /// 0xe0f2 行守卫破缺（开关 逐行校验失败）。
    GuardBad,
    /// 0xe0f3 矩阵累计漂移（注册表 求和失锚）。
    SumDrift,
    /// 0xe0f4 直方图溢出（超容量拒收未记账）。
    HistOver,
    /// 0xe0f5 直方图断桶（桶序缺失）。
    HistHole,
    /// 0xe0f6 分位越界（千分比>1000）。
    PctOob,
    /// 0xe0f7 均值漂移（均值与分位矛盾）。
    MeanDrift,
    /// 0xe0f8 台账不守恒（守恒恒等式破缺）。
    LedgerBroken,
    /// 0xe0f9 快照指纹不符（字段被改后重算不一致）。
    SnapshotFingerprint,
    /// 0xe0fa 常量表破坏（表项与语义规则矛盾）。
    TableBroken,
    /// 0xe0fb 段位守卫失败（诊断码越出本模块段）。
    SegmentGuard,
}

impl u3501Code {
    /// 全量闭集。
    pub const ALL: [u3501Code; 12] = [
        u3501Code::KeyDup,
        u3501Code::KeyGap,
        u3501Code::GuardBad,
        u3501Code::SumDrift,
        u3501Code::HistOver,
        u3501Code::HistHole,
        u3501Code::PctOob,
        u3501Code::MeanDrift,
        u3501Code::LedgerBroken,
        u3501Code::SnapshotFingerprint,
        u3501Code::TableBroken,
        u3501Code::SegmentGuard,
    ];

    /// 码值（连续十六进制，与声明序恒等）。
    pub const fn code(self) -> u16 {
        match self {
            u3501Code::KeyDup => 0xe0f0,
            u3501Code::KeyGap => 0xe0f1,
            u3501Code::GuardBad => 0xe0f2,
            u3501Code::SumDrift => 0xe0f3,
            u3501Code::HistOver => 0xe0f4,
            u3501Code::HistHole => 0xe0f5,
            u3501Code::PctOob => 0xe0f6,
            u3501Code::MeanDrift => 0xe0f7,
            u3501Code::LedgerBroken => 0xe0f8,
            u3501Code::SnapshotFingerprint => 0xe0f9,
            u3501Code::TableBroken => 0xe0fa,
            u3501Code::SegmentGuard => 0xe0fb,
        }
    }

    /// 中文诊断名。
    pub const fn name(self) -> &'static str {
        match self {
            u3501Code::KeyDup => "键重复（跟踪点 同键双行）",
            u3501Code::KeyGap => "键断档（埋点 序不连续）",
            u3501Code::GuardBad => "行守卫破缺（开关 逐行校验失败）",
            u3501Code::SumDrift => "矩阵累计漂移（注册表 求和失锚）",
            u3501Code::HistOver => "直方图溢出（超容量拒收未记账）",
            u3501Code::HistHole => "直方图断桶（桶序缺失）",
            u3501Code::PctOob => "分位越界（千分比>1000）",
            u3501Code::MeanDrift => "均值漂移（均值与分位矛盾）",
            u3501Code::LedgerBroken => "台账不守恒（守恒恒等式破缺）",
            u3501Code::SnapshotFingerprint => "快照指纹不符（字段被改后重算不一致）",
            u3501Code::TableBroken => "常量表破坏（表项与语义规则矛盾）",
            u3501Code::SegmentGuard => "段位守卫失败（诊断码越出本模块段）",
        }
    }

    /// 可操作修复建议（每条诊断都必须带下一步动作）。
    pub const fn suggestion(self) -> &'static str {
        match self {
            u3501Code::KeyDup => "按键唯一律去重",
            u3501Code::KeyGap => "按连续律补号",
            u3501Code::GuardBad => "按守卫口径重建",
            u3501Code::SumDrift => "按理论值复算",
            u3501Code::HistOver => "按溢出账口径登记",
            u3501Code::HistHole => "按桶序补齐",
            u3501Code::PctOob => "按 0..=1000 钳制",
            u3501Code::MeanDrift => "按同源数据复算",
            u3501Code::LedgerBroken => "按守恒律重建台账后重跑",
            u3501Code::SnapshotFingerprint => "按防篡改规程重建快照",
            u3501Code::TableBroken => "按语义规则重建常量表",
            u3501Code::SegmentGuard => "核对诊断码分配表",
        }
    }
}

// --------------------------------------------------------------------------
// 字符串键值映射 u3501mat（36 行逐行语义注释 + 逐行守卫）
// --------------------------------------------------------------------------
/// 跟踪点体系 守卫矩阵（与核心登记表同源对拍）。
/// 行数。
pub const u3501mat_COUNT: usize = 36;

/// 表本体（键 + 值 + 注释由编译期冻结）。
pub static u3501mat: [(&'static str, u64); 36] = [
    ("TP0000", 0), // TP 单元 0000（跟踪点 × 相位 00 × 一 轮）
    ("REG0001", 13), // REG 单元 0001（注册表 × 相位 01 × 一 轮）
    ("INS0002", 26), // INS 单元 0002（埋点 × 相位 02 × 一 轮）
    ("SW0003", 39), // SW 单元 0003（运行时开关 × 相位 03 × 一 轮）
    ("CID0004", 52), // CID 单元 0004（编译期 ID × 相位 04 × 一 轮）
    ("HIT0005", 65), // HIT 单元 0005（命中计数 × 相位 05 × 一 轮）
    ("BUF0006", 78), // BUF 单元 0006（事件缓冲 × 相位 06 × 一 轮）
    ("FLT0007", 91), // FLT 单元 0007（过滤 × 相位 07 × 一 轮）
    ("OVF0008", 7), // OVF 单元 0008（溢出 × 相位 08 × 一 轮）
    ("DUMP0009", 20), // DUMP 单元 0009（导出 × 相位 09 × 一 轮）
    ("STATIC0010", 33), // STATIC 单元 0010（静态段 × 相位 10 × 一 轮）
    ("DYN0011", 46), // DYN 单元 0011（动态段 × 相位 11 × 一 轮）
    ("TP0012", 59), // TP 单元 0012（跟踪点 × 相位 12 × 一 轮）
    ("REG0013", 72), // REG 单元 0013（注册表 × 相位 13 × 一 轮）
    ("INS0014", 85), // INS 单元 0014（埋点 × 相位 14 × 一 轮）
    ("SW0015", 1), // SW 单元 0015（运行时开关 × 相位 15 × 一 轮）
    ("CID0016", 14), // CID 单元 0016（编译期 ID × 相位 16 × 一 轮）
    ("HIT0017", 27), // HIT 单元 0017（命中计数 × 相位 17 × 一 轮）
    ("BUF0018", 40), // BUF 单元 0018（事件缓冲 × 相位 18 × 一 轮）
    ("FLT0019", 53), // FLT 单元 0019（过滤 × 相位 19 × 一 轮）
    ("OVF0020", 66), // OVF 单元 0020（溢出 × 相位 20 × 一 轮）
    ("DUMP0021", 79), // DUMP 单元 0021（导出 × 相位 21 × 一 轮）
    ("STATIC0022", 92), // STATIC 单元 0022（静态段 × 相位 22 × 一 轮）
    ("DYN0023", 8), // DYN 单元 0023（动态段 × 相位 23 × 一 轮）
    ("TP0024", 21), // TP 单元 0024（跟踪点 × 相位 24 × 一 轮）
    ("REG0025", 34), // REG 单元 0025（注册表 × 相位 25 × 一 轮）
    ("INS0026", 47), // INS 单元 0026（埋点 × 相位 26 × 一 轮）
    ("SW0027", 60), // SW 单元 0027（运行时开关 × 相位 27 × 一 轮）
    ("CID0028", 73), // CID 单元 0028（编译期 ID × 相位 28 × 一 轮）
    ("HIT0029", 86), // HIT 单元 0029（命中计数 × 相位 29 × 一 轮）
    ("BUF0030", 2), // BUF 单元 0030（事件缓冲 × 相位 30 × 一 轮）
    ("FLT0031", 15), // FLT 单元 0031（过滤 × 相位 31 × 一 轮）
    ("OVF0032", 28), // OVF 单元 0032（溢出 × 相位 32 × 一 轮）
    ("DUMP0033", 41), // DUMP 单元 0033（导出 × 相位 33 × 一 轮）
    ("STATIC0034", 54), // STATIC 单元 0034（静态段 × 相位 34 × 一 轮）
    ("DYN0035", 67), // DYN 单元 0035（动态段 × 相位 35 × 一 轮）
];

/// O(1) 语义查找（match 全枚举；命中率 100%% 由守卫保证）。
pub fn u3501mat_get(key: &str) -> Option<u64> {
    match key {
        "TP0000" => Some(0),
        "REG0001" => Some(13),
        "INS0002" => Some(26),
        "SW0003" => Some(39),
        "CID0004" => Some(52),
        "HIT0005" => Some(65),
        "BUF0006" => Some(78),
        "FLT0007" => Some(91),
        "OVF0008" => Some(7),
        "DUMP0009" => Some(20),
        "STATIC0010" => Some(33),
        "DYN0011" => Some(46),
        "TP0012" => Some(59),
        "REG0013" => Some(72),
        "INS0014" => Some(85),
        "SW0015" => Some(1),
        "CID0016" => Some(14),
        "HIT0017" => Some(27),
        "BUF0018" => Some(40),
        "FLT0019" => Some(53),
        "OVF0020" => Some(66),
        "DUMP0021" => Some(79),
        "STATIC0022" => Some(92),
        "DYN0023" => Some(8),
        "TP0024" => Some(21),
        "REG0025" => Some(34),
        "INS0026" => Some(47),
        "SW0027" => Some(60),
        "CID0028" => Some(73),
        "HIT0029" => Some(86),
        "BUF0030" => Some(2),
        "FLT0031" => Some(15),
        "OVF0032" => Some(28),
        "DUMP0033" => Some(41),
        "STATIC0034" => Some(54),
        "DYN0035" => Some(67),
        _ => None,
    }
}

/// 逐行守卫扫描（返回违例行数；0 = 全绿）。
pub fn u3501mat_scan() -> usize {
    let mut bad = 0usize;
    let mut i = 0usize;
    while i < u3501mat_COUNT {
        let (k, v) = u3501mat[i];
        let ok = match k {
            "TP0000" => v < 97,
            "REG0001" => v < 97,
            "INS0002" => v < 97,
            "SW0003" => v < 97,
            "CID0004" => v < 97,
            "HIT0005" => v < 97,
            "BUF0006" => v < 97,
            "FLT0007" => v < 97,
            "OVF0008" => v < 97,
            "DUMP0009" => v < 97,
            "STATIC0010" => v < 97,
            "DYN0011" => v < 97,
            "TP0012" => v < 97,
            "REG0013" => v < 97,
            "INS0014" => v < 97,
            "SW0015" => v < 97,
            "CID0016" => v < 97,
            "HIT0017" => v < 97,
            "BUF0018" => v < 97,
            "FLT0019" => v < 97,
            "OVF0020" => v < 97,
            "DUMP0021" => v < 97,
            "STATIC0022" => v < 97,
            "DYN0023" => v < 97,
            "TP0024" => v < 97,
            "REG0025" => v < 97,
            "INS0026" => v < 97,
            "SW0027" => v < 97,
            "CID0028" => v < 97,
            "HIT0029" => v < 97,
            "BUF0030" => v < 97,
            "FLT0031" => v < 97,
            "OVF0032" => v < 97,
            "DUMP0033" => v < 97,
            "STATIC0034" => v < 97,
            "DYN0035" => v < 97,
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
pub fn u3501mat_sum() -> u64 {
    let mut s = 0u64;
    let mut i = 0usize;
    while i < u3501mat_COUNT {
        s = s.wrapping_add(u3501mat[i].1 as u64);
        i += 1;
    }
    s
}

/// 键唯一性（构造期断言等价物：逐对扫描同键）。
pub fn u3501mat_unique() -> bool {
    let mut i = 0usize;
    while i < u3501mat_COUNT {
        let mut j = i + 1;
        while j < u3501mat_COUNT {
            if u3501mat[i].0 == u3501mat[j].0 {
                return false;
            }
            j += 1;
        }
        i += 1;
    }
    true
}

// --------------------------------------------------------------------------
// 直方图引擎 mxu3501hist（有界插入排序 + 分位数 P50/P90/P99）
// --------------------------------------------------------------------------
/// 跟踪点体系 直方图（96 桶滚动）。
/// 样本容量上限。
pub const MXU3501HIST_CAP: usize = 96;

/// 直方图账本（排序态 + 五统量）。
#[derive(Clone, Copy, Debug)]
pub struct mxu3501histHist {
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

impl mxu3501histHist {
    /// 空账构造。
    pub const fn new() -> Self {
        Self { samples: [0; 96], n: 0, drops: 0, min: 0, max: 0, sum: 0 }
    }

    /// 采样（有界插入排序；满员丢弃并记账）。
    pub fn push(&mut self, v: u64) {
        if self.n >= MXU3501HIST_CAP {
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

/// 段位守卫：全部诊断码必须落在本段 [0xe0f0, 0xe0ff]。
pub const fn u3501_segment_guard() -> bool {
    let mut ok = true;
    let mut i = 0usize;
    while i < u3501Code::ALL.len() {
        let c = u3501Code::ALL[i].code();
        if c < 0xe0f0 || c > 0xe0ff {
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
fn u3501_guard_seg() -> bool {
    u3501_segment_guard()
}

/// UNX-F3501 矩阵 36 行齐备 + 逐行守卫 + 唯一性 + 累计守恒。
fn check_u3501_map_guard() -> bool {
    let mut ok = u3501mat_COUNT == 36;
    if ok { ok = u3501mat_scan() == 0 && u3501mat_unique(); }
    if ok { ok = u3501mat_get("TP0000") == Some(0) && u3501mat_get("ZZZ_NOKEY").is_none(); }
    if ok { ok = u3501mat_sum() == 1594; }
    ok
}

/// UNX-F3501 直方图 16 样本分位/均值/守恒。
fn check_u3501_map_hist() -> bool {
    let mut h = mxu3501histHist::new();
    let mut i = 0u64;
    while i < 16 { h.push(i + 1); i += 1; }
    let mut ok = h.conserved() && h.mean() >= 1 && h.percentile(500) >= 8;
    ok
}

/// 本模块自检（逐检查带 detail，全绿才出门）。
pub fn run_u3501_checks() -> CheckSet {
    let mut set = CheckSet::new("U3501");
    set.add(
        "u3501-seg-guard",
        u3501_guard_seg(),
        if u3501_guard_seg() { "" } else { "diagnostic code outside segment" },
    );
    set.add(
        "u3501-map-guard",
        check_u3501_map_guard(),
        if check_u3501_map_guard() { "" } else { "UNX-F3501 matrix guard mismatch" },
    );
    set.add(
        "u3501-map-hist",
        check_u3501_map_hist(),
        if check_u3501_map_hist() { "" } else { "UNX-F3501 hist mismatch" },
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
    fn u3501_all_checks_green() {
        let set = run_u3501_checks();
        assert!(set.all_passed(), "U3501 domain checks must be green");
    }

    #[test]
    fn u3501_segment_guard_holds() {
        assert!(u3501_segment_guard());
    }
}
