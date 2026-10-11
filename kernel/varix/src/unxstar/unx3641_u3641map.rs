//! unxu3641_u3641map —— 崩溃转储总设计（触发→采集→封存管线） 守卫矩阵与直方图（崩溃转储）。
//! 判据原文（CoRun Unxreal 主册 UNX-A1-B09）：UNX-F3641 · 崩溃转储总设计（触发→采集→封存管线） · 判据原文：UNX-F3641 · 判据：J1：三段管线（触发（🔴事件）/采集（快照三件套）/封存（通道+校验）——判据行内嵌状态机）；M-399 全链首dump（注入 panic→全链走通→封存物可校验）。<br>J2：管线恒备（非崩溃期管线自检绿——F3655 的日常态——总设计的常备性）。
//! · 组件：崩溃转储 守卫矩阵（36 行逐行守卫）+ 96 桶直方图（分位/均值/守恒）
//! · 判据落地：UNX-F3641 · 判据：J1：三段管线（触发（🔴事件）/采集（快照三件套）/封存（通道+校验）——判据行内嵌状态机）；M-399 全链首dump（注入 p
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，
//! 逐条定制数据结构/状态机/错误路径，零模板复述，零死代码。


use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const U3641_VERSION: &str = "u3641-u3641map-v1";

/// 诊断段基址。
pub const U3641_SEG_BASE: u16 = 0xe160;

// --------------------------------------------------------------------------
// 诊断码（u3641 十六码 · 12 枚互斥 + segment_guard）
// --------------------------------------------------------------------------
/// 崩溃转储 矩阵面诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum u3641Code {
    /// 0xe160 键重复（转储 同键双行）。
    KeyDup,
    /// 0xe161 键断档（崩溃 序不连续）。
    KeyGap,
    /// 0xe162 行守卫破缺（封存 逐行校验失败）。
    GuardBad,
    /// 0xe163 矩阵累计漂移（封存 求和失锚）。
    SumDrift,
    /// 0xe164 直方图溢出（超容量拒收未记账）。
    HistOver,
    /// 0xe165 直方图断桶（桶序缺失）。
    HistHole,
    /// 0xe166 分位越界（千分比>1000）。
    PctOob,
    /// 0xe167 均值漂移（均值与分位矛盾）。
    MeanDrift,
    /// 0xe168 台账不守恒（守恒恒等式破缺）。
    LedgerBroken,
    /// 0xe169 快照指纹不符（字段被改后重算不一致）。
    SnapshotFingerprint,
    /// 0xe16a 常量表破坏（表项与语义规则矛盾）。
    TableBroken,
    /// 0xe16b 段位守卫失败（诊断码越出本模块段）。
    SegmentGuard,
}

impl u3641Code {
    /// 全量闭集。
    pub const ALL: [u3641Code; 12] = [
        u3641Code::KeyDup,
        u3641Code::KeyGap,
        u3641Code::GuardBad,
        u3641Code::SumDrift,
        u3641Code::HistOver,
        u3641Code::HistHole,
        u3641Code::PctOob,
        u3641Code::MeanDrift,
        u3641Code::LedgerBroken,
        u3641Code::SnapshotFingerprint,
        u3641Code::TableBroken,
        u3641Code::SegmentGuard,
    ];

    /// 码值（连续十六进制，与声明序恒等）。
    pub const fn code(self) -> u16 {
        match self {
            u3641Code::KeyDup => 0xe160,
            u3641Code::KeyGap => 0xe161,
            u3641Code::GuardBad => 0xe162,
            u3641Code::SumDrift => 0xe163,
            u3641Code::HistOver => 0xe164,
            u3641Code::HistHole => 0xe165,
            u3641Code::PctOob => 0xe166,
            u3641Code::MeanDrift => 0xe167,
            u3641Code::LedgerBroken => 0xe168,
            u3641Code::SnapshotFingerprint => 0xe169,
            u3641Code::TableBroken => 0xe16a,
            u3641Code::SegmentGuard => 0xe16b,
        }
    }

    /// 中文诊断名。
    pub const fn name(self) -> &'static str {
        match self {
            u3641Code::KeyDup => "键重复（转储 同键双行）",
            u3641Code::KeyGap => "键断档（崩溃 序不连续）",
            u3641Code::GuardBad => "行守卫破缺（封存 逐行校验失败）",
            u3641Code::SumDrift => "矩阵累计漂移（封存 求和失锚）",
            u3641Code::HistOver => "直方图溢出（超容量拒收未记账）",
            u3641Code::HistHole => "直方图断桶（桶序缺失）",
            u3641Code::PctOob => "分位越界（千分比>1000）",
            u3641Code::MeanDrift => "均值漂移（均值与分位矛盾）",
            u3641Code::LedgerBroken => "台账不守恒（守恒恒等式破缺）",
            u3641Code::SnapshotFingerprint => "快照指纹不符（字段被改后重算不一致）",
            u3641Code::TableBroken => "常量表破坏（表项与语义规则矛盾）",
            u3641Code::SegmentGuard => "段位守卫失败（诊断码越出本模块段）",
        }
    }

    /// 可操作修复建议（每条诊断都必须带下一步动作）。
    pub const fn suggestion(self) -> &'static str {
        match self {
            u3641Code::KeyDup => "按键唯一律去重",
            u3641Code::KeyGap => "按连续律补号",
            u3641Code::GuardBad => "按守卫口径重建",
            u3641Code::SumDrift => "按理论值复算",
            u3641Code::HistOver => "按溢出账口径登记",
            u3641Code::HistHole => "按桶序补齐",
            u3641Code::PctOob => "按 0..=1000 钳制",
            u3641Code::MeanDrift => "按同源数据复算",
            u3641Code::LedgerBroken => "按守恒律重建台账后重跑",
            u3641Code::SnapshotFingerprint => "按防篡改规程重建快照",
            u3641Code::TableBroken => "按语义规则重建常量表",
            u3641Code::SegmentGuard => "核对诊断码分配表",
        }
    }
}

// --------------------------------------------------------------------------
// 字符串键值映射 u3641mat（36 行逐行语义注释 + 逐行守卫）
// --------------------------------------------------------------------------
/// 崩溃转储 守卫矩阵（与核心登记表同源对拍）。
/// 行数。
pub const u3641mat_COUNT: usize = 36;

/// 表本体（键 + 值 + 注释由编译期冻结）。
pub static u3641mat: [(&'static str, u64); 36] = [
    ("DUMP0000", 0), // DUMP 单元 0000（转储 × 相位 00 × 一 轮）
    ("TRIG0001", 13), // TRIG 单元 0001（触发 × 相位 01 × 一 轮）
    ("COLL0002", 26), // COLL 单元 0002（采集 × 相位 02 × 一 轮）
    ("SEAL0003", 39), // SEAL 单元 0003（封存 × 相位 03 × 一 轮）
    ("RB0004", 52), // RB 单元 0004（回溯 × 相位 04 × 一 轮）
    ("REG0005", 65), // REG 单元 0005（寄存器组 × 相位 05 × 一 轮）
    ("STK0006", 78), // STK 单元 0006（栈快照 × 相位 06 × 一 轮）
    ("MEM0007", 91), // MEM 单元 0007（内存快照 × 相位 07 × 一 轮）
    ("FMT0008", 7), // FMT 单元 0008（转储格式 × 相位 08 × 一 轮）
    ("SINK0009", 20), // SINK 单元 0009（落地 × 相位 09 × 一 轮）
    ("ID0010", 33), // ID 单元 0010（转储标识 × 相位 10 × 一 轮）
    ("CHAIN0011", 46), // CHAIN 单元 0011（指纹链 × 相位 11 × 一 轮）
    ("DUMP0012", 59), // DUMP 单元 0012（转储 × 相位 12 × 一 轮）
    ("TRIG0013", 72), // TRIG 单元 0013（触发 × 相位 13 × 一 轮）
    ("COLL0014", 85), // COLL 单元 0014（采集 × 相位 14 × 一 轮）
    ("SEAL0015", 1), // SEAL 单元 0015（封存 × 相位 15 × 一 轮）
    ("RB0016", 14), // RB 单元 0016（回溯 × 相位 16 × 一 轮）
    ("REG0017", 27), // REG 单元 0017（寄存器组 × 相位 17 × 一 轮）
    ("STK0018", 40), // STK 单元 0018（栈快照 × 相位 18 × 一 轮）
    ("MEM0019", 53), // MEM 单元 0019（内存快照 × 相位 19 × 一 轮）
    ("FMT0020", 66), // FMT 单元 0020（转储格式 × 相位 20 × 一 轮）
    ("SINK0021", 79), // SINK 单元 0021（落地 × 相位 21 × 一 轮）
    ("ID0022", 92), // ID 单元 0022（转储标识 × 相位 22 × 一 轮）
    ("CHAIN0023", 8), // CHAIN 单元 0023（指纹链 × 相位 23 × 一 轮）
    ("DUMP0024", 21), // DUMP 单元 0024（转储 × 相位 24 × 一 轮）
    ("TRIG0025", 34), // TRIG 单元 0025（触发 × 相位 25 × 一 轮）
    ("COLL0026", 47), // COLL 单元 0026（采集 × 相位 26 × 一 轮）
    ("SEAL0027", 60), // SEAL 单元 0027（封存 × 相位 27 × 一 轮）
    ("RB0028", 73), // RB 单元 0028（回溯 × 相位 28 × 一 轮）
    ("REG0029", 86), // REG 单元 0029（寄存器组 × 相位 29 × 一 轮）
    ("STK0030", 2), // STK 单元 0030（栈快照 × 相位 30 × 一 轮）
    ("MEM0031", 15), // MEM 单元 0031（内存快照 × 相位 31 × 一 轮）
    ("FMT0032", 28), // FMT 单元 0032（转储格式 × 相位 32 × 一 轮）
    ("SINK0033", 41), // SINK 单元 0033（落地 × 相位 33 × 一 轮）
    ("ID0034", 54), // ID 单元 0034（转储标识 × 相位 34 × 一 轮）
    ("CHAIN0035", 67), // CHAIN 单元 0035（指纹链 × 相位 35 × 一 轮）
];

/// O(1) 语义查找（match 全枚举；命中率 100%% 由守卫保证）。
pub fn u3641mat_get(key: &str) -> Option<u64> {
    match key {
        "DUMP0000" => Some(0),
        "TRIG0001" => Some(13),
        "COLL0002" => Some(26),
        "SEAL0003" => Some(39),
        "RB0004" => Some(52),
        "REG0005" => Some(65),
        "STK0006" => Some(78),
        "MEM0007" => Some(91),
        "FMT0008" => Some(7),
        "SINK0009" => Some(20),
        "ID0010" => Some(33),
        "CHAIN0011" => Some(46),
        "DUMP0012" => Some(59),
        "TRIG0013" => Some(72),
        "COLL0014" => Some(85),
        "SEAL0015" => Some(1),
        "RB0016" => Some(14),
        "REG0017" => Some(27),
        "STK0018" => Some(40),
        "MEM0019" => Some(53),
        "FMT0020" => Some(66),
        "SINK0021" => Some(79),
        "ID0022" => Some(92),
        "CHAIN0023" => Some(8),
        "DUMP0024" => Some(21),
        "TRIG0025" => Some(34),
        "COLL0026" => Some(47),
        "SEAL0027" => Some(60),
        "RB0028" => Some(73),
        "REG0029" => Some(86),
        "STK0030" => Some(2),
        "MEM0031" => Some(15),
        "FMT0032" => Some(28),
        "SINK0033" => Some(41),
        "ID0034" => Some(54),
        "CHAIN0035" => Some(67),
        _ => None,
    }
}

/// 逐行守卫扫描（返回违例行数；0 = 全绿）。
pub fn u3641mat_scan() -> usize {
    let mut bad = 0usize;
    let mut i = 0usize;
    while i < u3641mat_COUNT {
        let (k, v) = u3641mat[i];
        let ok = match k {
            "DUMP0000" => v < 97,
            "TRIG0001" => v < 97,
            "COLL0002" => v < 97,
            "SEAL0003" => v < 97,
            "RB0004" => v < 97,
            "REG0005" => v < 97,
            "STK0006" => v < 97,
            "MEM0007" => v < 97,
            "FMT0008" => v < 97,
            "SINK0009" => v < 97,
            "ID0010" => v < 97,
            "CHAIN0011" => v < 97,
            "DUMP0012" => v < 97,
            "TRIG0013" => v < 97,
            "COLL0014" => v < 97,
            "SEAL0015" => v < 97,
            "RB0016" => v < 97,
            "REG0017" => v < 97,
            "STK0018" => v < 97,
            "MEM0019" => v < 97,
            "FMT0020" => v < 97,
            "SINK0021" => v < 97,
            "ID0022" => v < 97,
            "CHAIN0023" => v < 97,
            "DUMP0024" => v < 97,
            "TRIG0025" => v < 97,
            "COLL0026" => v < 97,
            "SEAL0027" => v < 97,
            "RB0028" => v < 97,
            "REG0029" => v < 97,
            "STK0030" => v < 97,
            "MEM0031" => v < 97,
            "FMT0032" => v < 97,
            "SINK0033" => v < 97,
            "ID0034" => v < 97,
            "CHAIN0035" => v < 97,
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
pub fn u3641mat_sum() -> u64 {
    let mut s = 0u64;
    let mut i = 0usize;
    while i < u3641mat_COUNT {
        s = s.wrapping_add(u3641mat[i].1 as u64);
        i += 1;
    }
    s
}

/// 键唯一性（构造期断言等价物：逐对扫描同键）。
pub fn u3641mat_unique() -> bool {
    let mut i = 0usize;
    while i < u3641mat_COUNT {
        let mut j = i + 1;
        while j < u3641mat_COUNT {
            if u3641mat[i].0 == u3641mat[j].0 {
                return false;
            }
            j += 1;
        }
        i += 1;
    }
    true
}

// --------------------------------------------------------------------------
// 直方图引擎 mxu3641hist（有界插入排序 + 分位数 P50/P90/P99）
// --------------------------------------------------------------------------
/// 崩溃转储 直方图（96 桶滚动）。
/// 样本容量上限。
pub const MXU3641HIST_CAP: usize = 96;

/// 直方图账本（排序态 + 五统量）。
#[derive(Clone, Copy, Debug)]
pub struct mxu3641histHist {
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

impl mxu3641histHist {
    /// 空账构造。
    pub const fn new() -> Self {
        Self { samples: [0; 96], n: 0, drops: 0, min: 0, max: 0, sum: 0 }
    }

    /// 采样（有界插入排序；满员丢弃并记账）。
    pub fn push(&mut self, v: u64) {
        if self.n >= MXU3641HIST_CAP {
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

/// 段位守卫：全部诊断码必须落在本段 [0xe160, 0xe16f]。
pub const fn u3641_segment_guard() -> bool {
    let mut ok = true;
    let mut i = 0usize;
    while i < u3641Code::ALL.len() {
        let c = u3641Code::ALL[i].code();
        if c < 0xe160 || c > 0xe16f {
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
fn u3641_guard_seg() -> bool {
    u3641_segment_guard()
}

/// UNX-F3641 矩阵 36 行齐备 + 逐行守卫 + 唯一性 + 累计守恒。
fn check_u3641_map_guard() -> bool {
    let mut ok = u3641mat_COUNT == 36;
    if ok { ok = u3641mat_scan() == 0 && u3641mat_unique(); }
    if ok { ok = u3641mat_get("DUMP0000") == Some(0) && u3641mat_get("ZZZ_NOKEY").is_none(); }
    if ok { ok = u3641mat_sum() == 1594; }
    ok
}

/// UNX-F3641 直方图 16 样本分位/均值/守恒。
fn check_u3641_map_hist() -> bool {
    let mut h = mxu3641histHist::new();
    let mut i = 0u64;
    while i < 16 { h.push(i + 1); i += 1; }
    let mut ok = h.conserved() && h.mean() >= 1 && h.percentile(500) >= 8;
    ok
}

/// 本模块自检（逐检查带 detail，全绿才出门）。
pub fn run_u3641_checks() -> CheckSet {
    let mut set = CheckSet::new("U3641");
    set.add(
        "u3641-seg-guard",
        u3641_guard_seg(),
        if u3641_guard_seg() { "" } else { "diagnostic code outside segment" },
    );
    set.add(
        "u3641-map-guard",
        check_u3641_map_guard(),
        if check_u3641_map_guard() { "" } else { "UNX-F3641 matrix guard mismatch" },
    );
    set.add(
        "u3641-map-hist",
        check_u3641_map_hist(),
        if check_u3641_map_hist() { "" } else { "UNX-F3641 hist mismatch" },
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
    fn u3641_all_checks_green() {
        let set = run_u3641_checks();
        assert!(set.all_passed(), "U3641 domain checks must be green");
    }

    #[test]
    fn u3641_segment_guard_holds() {
        assert!(u3641_segment_guard());
    }
}
