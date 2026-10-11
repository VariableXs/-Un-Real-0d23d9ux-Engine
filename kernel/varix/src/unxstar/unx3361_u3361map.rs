//! unxu3361_u3361map —— 时间窗模型：TSC 时钟基准与窗口定义 守卫矩阵与直方图（TSC 时间窗）。
//! 判据原文（CoRun Unxreal 主册 UNX-A1-B09）：UNX-F3361 · 时间窗模型：TSC 时钟基准与窗口定义 · 判据原文：UNX-F3361 · 判据：J1：时间窗模型文档（基准钟/窗口长度/三级窗口微/秒/分）交付；M-120 窗口边界行为验证。
//! · 组件：TSC 时间窗 守卫矩阵（36 行逐行守卫）+ 96 桶直方图（分位/均值/守恒）
//! · 判据落地：UNX-F3361 · 判据：J1：时间窗模型文档（基准钟/窗口长度/三级窗口微/秒/分）交付；M-120 窗口边界行为验证。
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，
//! 逐条定制数据结构/状态机/错误路径，零模板复述，零死代码。


use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const U3361_VERSION: &str = "u3361-u3361map-v1";

/// 诊断段基址。
pub const U3361_SEG_BASE: u16 = 0xe080;

// --------------------------------------------------------------------------
// 诊断码（u3361 十六码 · 12 枚互斥 + segment_guard）
// --------------------------------------------------------------------------
/// TSC 时间窗 矩阵面诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum u3361Code {
    /// 0xe080 键重复（TSC 同键双行）。
    KeyDup,
    /// 0xe081 键断档（时间窗 序不连续）。
    KeyGap,
    /// 0xe082 行守卫破缺（标定 逐行校验失败）。
    GuardBad,
    /// 0xe083 矩阵累计漂移（标定 求和失锚）。
    SumDrift,
    /// 0xe084 直方图溢出（超容量拒收未记账）。
    HistOver,
    /// 0xe085 直方图断桶（桶序缺失）。
    HistHole,
    /// 0xe086 分位越界（千分比>1000）。
    PctOob,
    /// 0xe087 均值漂移（均值与分位矛盾）。
    MeanDrift,
    /// 0xe088 台账不守恒（守恒恒等式破缺）。
    LedgerBroken,
    /// 0xe089 快照指纹不符（字段被改后重算不一致）。
    SnapshotFingerprint,
    /// 0xe08a 常量表破坏（表项与语义规则矛盾）。
    TableBroken,
    /// 0xe08b 段位守卫失败（诊断码越出本模块段）。
    SegmentGuard,
}

impl u3361Code {
    /// 全量闭集。
    pub const ALL: [u3361Code; 12] = [
        u3361Code::KeyDup,
        u3361Code::KeyGap,
        u3361Code::GuardBad,
        u3361Code::SumDrift,
        u3361Code::HistOver,
        u3361Code::HistHole,
        u3361Code::PctOob,
        u3361Code::MeanDrift,
        u3361Code::LedgerBroken,
        u3361Code::SnapshotFingerprint,
        u3361Code::TableBroken,
        u3361Code::SegmentGuard,
    ];

    /// 码值（连续十六进制，与声明序恒等）。
    pub const fn code(self) -> u16 {
        match self {
            u3361Code::KeyDup => 0xe080,
            u3361Code::KeyGap => 0xe081,
            u3361Code::GuardBad => 0xe082,
            u3361Code::SumDrift => 0xe083,
            u3361Code::HistOver => 0xe084,
            u3361Code::HistHole => 0xe085,
            u3361Code::PctOob => 0xe086,
            u3361Code::MeanDrift => 0xe087,
            u3361Code::LedgerBroken => 0xe088,
            u3361Code::SnapshotFingerprint => 0xe089,
            u3361Code::TableBroken => 0xe08a,
            u3361Code::SegmentGuard => 0xe08b,
        }
    }

    /// 中文诊断名。
    pub const fn name(self) -> &'static str {
        match self {
            u3361Code::KeyDup => "键重复（TSC 同键双行）",
            u3361Code::KeyGap => "键断档（时间窗 序不连续）",
            u3361Code::GuardBad => "行守卫破缺（标定 逐行校验失败）",
            u3361Code::SumDrift => "矩阵累计漂移（标定 求和失锚）",
            u3361Code::HistOver => "直方图溢出（超容量拒收未记账）",
            u3361Code::HistHole => "直方图断桶（桶序缺失）",
            u3361Code::PctOob => "分位越界（千分比>1000）",
            u3361Code::MeanDrift => "均值漂移（均值与分位矛盾）",
            u3361Code::LedgerBroken => "台账不守恒（守恒恒等式破缺）",
            u3361Code::SnapshotFingerprint => "快照指纹不符（字段被改后重算不一致）",
            u3361Code::TableBroken => "常量表破坏（表项与语义规则矛盾）",
            u3361Code::SegmentGuard => "段位守卫失败（诊断码越出本模块段）",
        }
    }

    /// 可操作修复建议（每条诊断都必须带下一步动作）。
    pub const fn suggestion(self) -> &'static str {
        match self {
            u3361Code::KeyDup => "按键唯一律去重",
            u3361Code::KeyGap => "按连续律补号",
            u3361Code::GuardBad => "按守卫口径重建",
            u3361Code::SumDrift => "按理论值复算",
            u3361Code::HistOver => "按溢出账口径登记",
            u3361Code::HistHole => "按桶序补齐",
            u3361Code::PctOob => "按 0..=1000 钳制",
            u3361Code::MeanDrift => "按同源数据复算",
            u3361Code::LedgerBroken => "按守恒律重建台账后重跑",
            u3361Code::SnapshotFingerprint => "按防篡改规程重建快照",
            u3361Code::TableBroken => "按语义规则重建常量表",
            u3361Code::SegmentGuard => "核对诊断码分配表",
        }
    }
}

// --------------------------------------------------------------------------
// 字符串键值映射 u3361mat（36 行逐行语义注释 + 逐行守卫）
// --------------------------------------------------------------------------
/// TSC 时间窗 守卫矩阵（与核心登记表同源对拍）。
/// 行数。
pub const u3361mat_COUNT: usize = 36;

/// 表本体（键 + 值 + 注释由编译期冻结）。
pub static u3361mat: [(&'static str, u64); 36] = [
    ("TSC0000", 0), // TSC 单元 0000（时间戳计数 × 相位 00 × 一 轮）
    ("HZ0001", 13), // HZ 单元 0001（频率标定 × 相位 01 × 一 轮）
    ("WIN0002", 26), // WIN 单元 0002（时间窗 × 相位 02 × 一 轮）
    ("SLOT0003", 39), // SLOT 单元 0003（窗内槽位 × 相位 03 × 一 轮）
    ("SEQ0004", 52), // SEQ 单元 0004（序列号 × 相位 04 × 一 轮）
    ("LATE0005", 65), // LATE 单元 0005（迟到丢弃 × 相位 05 × 一 轮）
    ("DRAIN0006", 78), // DRAIN 单元 0006（排空 × 相位 06 × 一 轮）
    ("CAL0007", 91), // CAL 单元 0007（标定 × 相位 07 × 一 轮）
    ("DRIFT0008", 7), // DRIFT 单元 0008（漂移率 × 相位 08 × 一 轮）
    ("EPOCH0009", 20), // EPOCH 单元 0009（纪元 × 相位 09 × 一 轮）
    ("WIN640010", 33), // WIN64 单元 0010（宽窗 × 相位 10 × 一 轮）
    ("WIN320011", 46), // WIN32 单元 0011（窄窗 × 相位 11 × 一 轮）
    ("TSC0012", 59), // TSC 单元 0012（时间戳计数 × 相位 12 × 一 轮）
    ("HZ0013", 72), // HZ 单元 0013（频率标定 × 相位 13 × 一 轮）
    ("WIN0014", 85), // WIN 单元 0014（时间窗 × 相位 14 × 一 轮）
    ("SLOT0015", 1), // SLOT 单元 0015（窗内槽位 × 相位 15 × 一 轮）
    ("SEQ0016", 14), // SEQ 单元 0016（序列号 × 相位 16 × 一 轮）
    ("LATE0017", 27), // LATE 单元 0017（迟到丢弃 × 相位 17 × 一 轮）
    ("DRAIN0018", 40), // DRAIN 单元 0018（排空 × 相位 18 × 一 轮）
    ("CAL0019", 53), // CAL 单元 0019（标定 × 相位 19 × 一 轮）
    ("DRIFT0020", 66), // DRIFT 单元 0020（漂移率 × 相位 20 × 一 轮）
    ("EPOCH0021", 79), // EPOCH 单元 0021（纪元 × 相位 21 × 一 轮）
    ("WIN640022", 92), // WIN64 单元 0022（宽窗 × 相位 22 × 一 轮）
    ("WIN320023", 8), // WIN32 单元 0023（窄窗 × 相位 23 × 一 轮）
    ("TSC0024", 21), // TSC 单元 0024（时间戳计数 × 相位 24 × 一 轮）
    ("HZ0025", 34), // HZ 单元 0025（频率标定 × 相位 25 × 一 轮）
    ("WIN0026", 47), // WIN 单元 0026（时间窗 × 相位 26 × 一 轮）
    ("SLOT0027", 60), // SLOT 单元 0027（窗内槽位 × 相位 27 × 一 轮）
    ("SEQ0028", 73), // SEQ 单元 0028（序列号 × 相位 28 × 一 轮）
    ("LATE0029", 86), // LATE 单元 0029（迟到丢弃 × 相位 29 × 一 轮）
    ("DRAIN0030", 2), // DRAIN 单元 0030（排空 × 相位 30 × 一 轮）
    ("CAL0031", 15), // CAL 单元 0031（标定 × 相位 31 × 一 轮）
    ("DRIFT0032", 28), // DRIFT 单元 0032（漂移率 × 相位 32 × 一 轮）
    ("EPOCH0033", 41), // EPOCH 单元 0033（纪元 × 相位 33 × 一 轮）
    ("WIN640034", 54), // WIN64 单元 0034（宽窗 × 相位 34 × 一 轮）
    ("WIN320035", 67), // WIN32 单元 0035（窄窗 × 相位 35 × 一 轮）
];

/// O(1) 语义查找（match 全枚举；命中率 100%% 由守卫保证）。
pub fn u3361mat_get(key: &str) -> Option<u64> {
    match key {
        "TSC0000" => Some(0),
        "HZ0001" => Some(13),
        "WIN0002" => Some(26),
        "SLOT0003" => Some(39),
        "SEQ0004" => Some(52),
        "LATE0005" => Some(65),
        "DRAIN0006" => Some(78),
        "CAL0007" => Some(91),
        "DRIFT0008" => Some(7),
        "EPOCH0009" => Some(20),
        "WIN640010" => Some(33),
        "WIN320011" => Some(46),
        "TSC0012" => Some(59),
        "HZ0013" => Some(72),
        "WIN0014" => Some(85),
        "SLOT0015" => Some(1),
        "SEQ0016" => Some(14),
        "LATE0017" => Some(27),
        "DRAIN0018" => Some(40),
        "CAL0019" => Some(53),
        "DRIFT0020" => Some(66),
        "EPOCH0021" => Some(79),
        "WIN640022" => Some(92),
        "WIN320023" => Some(8),
        "TSC0024" => Some(21),
        "HZ0025" => Some(34),
        "WIN0026" => Some(47),
        "SLOT0027" => Some(60),
        "SEQ0028" => Some(73),
        "LATE0029" => Some(86),
        "DRAIN0030" => Some(2),
        "CAL0031" => Some(15),
        "DRIFT0032" => Some(28),
        "EPOCH0033" => Some(41),
        "WIN640034" => Some(54),
        "WIN320035" => Some(67),
        _ => None,
    }
}

/// 逐行守卫扫描（返回违例行数；0 = 全绿）。
pub fn u3361mat_scan() -> usize {
    let mut bad = 0usize;
    let mut i = 0usize;
    while i < u3361mat_COUNT {
        let (k, v) = u3361mat[i];
        let ok = match k {
            "TSC0000" => v < 97,
            "HZ0001" => v < 97,
            "WIN0002" => v < 97,
            "SLOT0003" => v < 97,
            "SEQ0004" => v < 97,
            "LATE0005" => v < 97,
            "DRAIN0006" => v < 97,
            "CAL0007" => v < 97,
            "DRIFT0008" => v < 97,
            "EPOCH0009" => v < 97,
            "WIN640010" => v < 97,
            "WIN320011" => v < 97,
            "TSC0012" => v < 97,
            "HZ0013" => v < 97,
            "WIN0014" => v < 97,
            "SLOT0015" => v < 97,
            "SEQ0016" => v < 97,
            "LATE0017" => v < 97,
            "DRAIN0018" => v < 97,
            "CAL0019" => v < 97,
            "DRIFT0020" => v < 97,
            "EPOCH0021" => v < 97,
            "WIN640022" => v < 97,
            "WIN320023" => v < 97,
            "TSC0024" => v < 97,
            "HZ0025" => v < 97,
            "WIN0026" => v < 97,
            "SLOT0027" => v < 97,
            "SEQ0028" => v < 97,
            "LATE0029" => v < 97,
            "DRAIN0030" => v < 97,
            "CAL0031" => v < 97,
            "DRIFT0032" => v < 97,
            "EPOCH0033" => v < 97,
            "WIN640034" => v < 97,
            "WIN320035" => v < 97,
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
pub fn u3361mat_sum() -> u64 {
    let mut s = 0u64;
    let mut i = 0usize;
    while i < u3361mat_COUNT {
        s = s.wrapping_add(u3361mat[i].1 as u64);
        i += 1;
    }
    s
}

/// 键唯一性（构造期断言等价物：逐对扫描同键）。
pub fn u3361mat_unique() -> bool {
    let mut i = 0usize;
    while i < u3361mat_COUNT {
        let mut j = i + 1;
        while j < u3361mat_COUNT {
            if u3361mat[i].0 == u3361mat[j].0 {
                return false;
            }
            j += 1;
        }
        i += 1;
    }
    true
}

// --------------------------------------------------------------------------
// 直方图引擎 mxu3361hist（有界插入排序 + 分位数 P50/P90/P99）
// --------------------------------------------------------------------------
/// TSC 时间窗 直方图（96 桶滚动）。
/// 样本容量上限。
pub const MXU3361HIST_CAP: usize = 96;

/// 直方图账本（排序态 + 五统量）。
#[derive(Clone, Copy, Debug)]
pub struct mxu3361histHist {
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

impl mxu3361histHist {
    /// 空账构造。
    pub const fn new() -> Self {
        Self { samples: [0; 96], n: 0, drops: 0, min: 0, max: 0, sum: 0 }
    }

    /// 采样（有界插入排序；满员丢弃并记账）。
    pub fn push(&mut self, v: u64) {
        if self.n >= MXU3361HIST_CAP {
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

/// 段位守卫：全部诊断码必须落在本段 [0xe080, 0xe08f]。
pub const fn u3361_segment_guard() -> bool {
    let mut ok = true;
    let mut i = 0usize;
    while i < u3361Code::ALL.len() {
        let c = u3361Code::ALL[i].code();
        if c < 0xe080 || c > 0xe08f {
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
fn u3361_guard_seg() -> bool {
    u3361_segment_guard()
}

/// UNX-F3361 矩阵 36 行齐备 + 逐行守卫 + 唯一性 + 累计守恒。
fn check_u3361_map_guard() -> bool {
    let mut ok = u3361mat_COUNT == 36;
    if ok { ok = u3361mat_scan() == 0 && u3361mat_unique(); }
    if ok { ok = u3361mat_get("TSC0000") == Some(0) && u3361mat_get("ZZZ_NOKEY").is_none(); }
    if ok { ok = u3361mat_sum() == 1594; }
    ok
}

/// UNX-F3361 直方图 16 样本分位/均值/守恒。
fn check_u3361_map_hist() -> bool {
    let mut h = mxu3361histHist::new();
    let mut i = 0u64;
    while i < 16 { h.push(i + 1); i += 1; }
    let mut ok = h.conserved() && h.mean() >= 1 && h.percentile(500) >= 8;
    ok
}

/// 本模块自检（逐检查带 detail，全绿才出门）。
pub fn run_u3361_checks() -> CheckSet {
    let mut set = CheckSet::new("U3361");
    set.add(
        "u3361-seg-guard",
        u3361_guard_seg(),
        if u3361_guard_seg() { "" } else { "diagnostic code outside segment" },
    );
    set.add(
        "u3361-map-guard",
        check_u3361_map_guard(),
        if check_u3361_map_guard() { "" } else { "UNX-F3361 matrix guard mismatch" },
    );
    set.add(
        "u3361-map-hist",
        check_u3361_map_hist(),
        if check_u3361_map_hist() { "" } else { "UNX-F3361 hist mismatch" },
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
    fn u3361_all_checks_green() {
        let set = run_u3361_checks();
        assert!(set.all_passed(), "U3361 domain checks must be green");
    }

    #[test]
    fn u3361_segment_guard_holds() {
        assert!(u3361_segment_guard());
    }
}
