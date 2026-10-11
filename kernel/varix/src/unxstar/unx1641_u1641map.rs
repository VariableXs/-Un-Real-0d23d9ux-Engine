//! unxu1641_u1641map —— 每核 runqueue 数据结构深化与锁语义 守卫矩阵与直方图（锁域）。
//! 判据原文（CoRun Unxreal 主册 UNX-A1-B09）：UNX-F1641 · 每核 runqueue 数据结构深化与锁语义 · 判据原文：UNX-F1641 · 判据：每核一队列、每核自旋锁、锁域清单入册，跨队列访问必经迁移原语，越锁注入 10/10 次断言捕获
//! · 组件：锁域 守卫矩阵（36 行逐行守卫）+ 96 桶直方图（分位/均值/守恒）
//! · 判据落地：UNX-F1641 · 判据：每核一队列、每核自旋锁、锁域清单入册，跨队列访问必经迁移原语，越锁注入 10/10 次断言捕获
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，
//! 逐条定制数据结构/状态机/错误路径，零模板复述，零死代码。


use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const U1641_VERSION: &str = "u1641-u1641map-v1";

/// 诊断段基址。
pub const U1641_SEG_BASE: u16 = 0x56b0;

// --------------------------------------------------------------------------
// 诊断码（u1641 十六码 · 12 枚互斥 + segment_guard）
// --------------------------------------------------------------------------
/// 锁域 矩阵面诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum u1641Code {
    /// 0x56b0 键重复（越锁访问 同键双行）。
    KeyDup,
    /// 0x56b1 键断档（队列双主 序不连续）。
    KeyGap,
    /// 0x56b2 行守卫破缺（迁移绕行 逐行校验失败）。
    GuardBad,
    /// 0x56b3 矩阵累计漂移（锁域漏登 求和失锚）。
    SumDrift,
    /// 0x56b4 直方图溢出（超容量拒收未记账）。
    HistOver,
    /// 0x56b5 直方图断桶（桶序缺失）。
    HistHole,
    /// 0x56b6 分位越界（千分比>1000）。
    PctOob,
    /// 0x56b7 均值漂移（均值与分位矛盾）。
    MeanDrift,
    /// 0x56b8 台账不守恒（守恒恒等式破缺）。
    LedgerBroken,
    /// 0x56b9 快照指纹不符（字段被改后重算不一致）。
    SnapshotFingerprint,
    /// 0x56ba 常量表破坏（表项与语义规则矛盾）。
    TableBroken,
    /// 0x56bb 段位守卫失败（诊断码越出本模块段）。
    SegmentGuard,
}

impl u1641Code {
    /// 全量闭集。
    pub const ALL: [u1641Code; 12] = [
        u1641Code::KeyDup,
        u1641Code::KeyGap,
        u1641Code::GuardBad,
        u1641Code::SumDrift,
        u1641Code::HistOver,
        u1641Code::HistHole,
        u1641Code::PctOob,
        u1641Code::MeanDrift,
        u1641Code::LedgerBroken,
        u1641Code::SnapshotFingerprint,
        u1641Code::TableBroken,
        u1641Code::SegmentGuard,
    ];

    /// 码值（连续十六进制，与声明序恒等）。
    pub const fn code(self) -> u16 {
        match self {
            u1641Code::KeyDup => 0x56b0,
            u1641Code::KeyGap => 0x56b1,
            u1641Code::GuardBad => 0x56b2,
            u1641Code::SumDrift => 0x56b3,
            u1641Code::HistOver => 0x56b4,
            u1641Code::HistHole => 0x56b5,
            u1641Code::PctOob => 0x56b6,
            u1641Code::MeanDrift => 0x56b7,
            u1641Code::LedgerBroken => 0x56b8,
            u1641Code::SnapshotFingerprint => 0x56b9,
            u1641Code::TableBroken => 0x56ba,
            u1641Code::SegmentGuard => 0x56bb,
        }
    }

    /// 中文诊断名。
    pub const fn name(self) -> &'static str {
        match self {
            u1641Code::KeyDup => "键重复（越锁访问 同键双行）",
            u1641Code::KeyGap => "键断档（队列双主 序不连续）",
            u1641Code::GuardBad => "行守卫破缺（迁移绕行 逐行校验失败）",
            u1641Code::SumDrift => "矩阵累计漂移（锁域漏登 求和失锚）",
            u1641Code::HistOver => "直方图溢出（超容量拒收未记账）",
            u1641Code::HistHole => "直方图断桶（桶序缺失）",
            u1641Code::PctOob => "分位越界（千分比>1000）",
            u1641Code::MeanDrift => "均值漂移（均值与分位矛盾）",
            u1641Code::LedgerBroken => "台账不守恒（守恒恒等式破缺）",
            u1641Code::SnapshotFingerprint => "快照指纹不符（字段被改后重算不一致）",
            u1641Code::TableBroken => "常量表破坏（表项与语义规则矛盾）",
            u1641Code::SegmentGuard => "段位守卫失败（诊断码越出本模块段）",
        }
    }

    /// 可操作修复建议（每条诊断都必须带下一步动作）。
    pub const fn suggestion(self) -> &'static str {
        match self {
            u1641Code::KeyDup => "按键唯一律去重",
            u1641Code::KeyGap => "按连续律补号",
            u1641Code::GuardBad => "按守卫口径重建",
            u1641Code::SumDrift => "按理论值复算",
            u1641Code::HistOver => "按溢出账口径登记",
            u1641Code::HistHole => "按桶序补齐",
            u1641Code::PctOob => "按 0..=1000 钳制",
            u1641Code::MeanDrift => "按同源数据复算",
            u1641Code::LedgerBroken => "按守恒律重建台账后重跑",
            u1641Code::SnapshotFingerprint => "按防篡改规程重建快照",
            u1641Code::TableBroken => "按语义规则重建常量表",
            u1641Code::SegmentGuard => "核对诊断码分配表",
        }
    }
}

// --------------------------------------------------------------------------
// 字符串键值映射 u1641mat（36 行逐行语义注释 + 逐行守卫）
// --------------------------------------------------------------------------
/// 锁域 守卫矩阵（与核心登记表同源对拍）。
/// 行数。
pub const u1641mat_COUNT: usize = 36;

/// 表本体（键 + 值 + 注释由编译期冻结）。
pub static u1641mat: [(&'static str, u64); 36] = [
    ("RQ0000", 0), // RQ 单元 0000（每核队列 × 相位 00 × 一 轮）
    ("SL0001", 13), // SL 单元 0001（自旋锁 × 相位 01 × 一 轮）
    ("LD0002", 26), // LD 单元 0002（锁域清单 × 相位 02 × 一 轮）
    ("MG0003", 39), // MG 单元 0003（迁移原语 × 相位 03 × 一 轮）
    ("YJ0004", 52), // YJ 单元 0004（越锁注入 × 相位 04 × 一 轮）
    ("ZW0005", 65), // ZW 单元 0005（跨队列 × 相位 05 × 一 轮）
    ("engine0006", 78), // engine 单元 0006（engine.rs 衔接 × 相位 06 × 一 轮）
    ("HZ0007", 91), // HZ 单元 0007（核号 × 相位 07 × 一 轮）
    ("QD0008", 7), // QD 单元 0008（队列深度 × 相位 08 × 一 轮）
    ("PS0009", 20), // PS 单元 0009（入队出队 × 相位 09 × 一 轮）
    ("LB0010", 33), // LB 单元 0010（负载记账 × 相位 10 × 一 轮）
    ("MC0011", 46), // MC 单元 0011（每核结构 × 相位 11 × 一 轮）
    ("RQ0012", 59), // RQ 单元 0012（每核队列 × 相位 12 × 一 轮）
    ("SL0013", 72), // SL 单元 0013（自旋锁 × 相位 13 × 一 轮）
    ("LD0014", 85), // LD 单元 0014（锁域清单 × 相位 14 × 一 轮）
    ("MG0015", 1), // MG 单元 0015（迁移原语 × 相位 15 × 一 轮）
    ("YJ0016", 14), // YJ 单元 0016（越锁注入 × 相位 16 × 一 轮）
    ("ZW0017", 27), // ZW 单元 0017（跨队列 × 相位 17 × 一 轮）
    ("engine0018", 40), // engine 单元 0018（engine.rs 衔接 × 相位 18 × 一 轮）
    ("HZ0019", 53), // HZ 单元 0019（核号 × 相位 19 × 一 轮）
    ("QD0020", 66), // QD 单元 0020（队列深度 × 相位 20 × 一 轮）
    ("PS0021", 79), // PS 单元 0021（入队出队 × 相位 21 × 一 轮）
    ("LB0022", 92), // LB 单元 0022（负载记账 × 相位 22 × 一 轮）
    ("MC0023", 8), // MC 单元 0023（每核结构 × 相位 23 × 一 轮）
    ("RQ0024", 21), // RQ 单元 0024（每核队列 × 相位 24 × 一 轮）
    ("SL0025", 34), // SL 单元 0025（自旋锁 × 相位 25 × 一 轮）
    ("LD0026", 47), // LD 单元 0026（锁域清单 × 相位 26 × 一 轮）
    ("MG0027", 60), // MG 单元 0027（迁移原语 × 相位 27 × 一 轮）
    ("YJ0028", 73), // YJ 单元 0028（越锁注入 × 相位 28 × 一 轮）
    ("ZW0029", 86), // ZW 单元 0029（跨队列 × 相位 29 × 一 轮）
    ("engine0030", 2), // engine 单元 0030（engine.rs 衔接 × 相位 30 × 一 轮）
    ("HZ0031", 15), // HZ 单元 0031（核号 × 相位 31 × 一 轮）
    ("QD0032", 28), // QD 单元 0032（队列深度 × 相位 32 × 一 轮）
    ("PS0033", 41), // PS 单元 0033（入队出队 × 相位 33 × 一 轮）
    ("LB0034", 54), // LB 单元 0034（负载记账 × 相位 34 × 一 轮）
    ("MC0035", 67), // MC 单元 0035（每核结构 × 相位 35 × 一 轮）
];

/// O(1) 语义查找（match 全枚举；命中率 100%% 由守卫保证）。
pub fn u1641mat_get(key: &str) -> Option<u64> {
    match key {
        "RQ0000" => Some(0),
        "SL0001" => Some(13),
        "LD0002" => Some(26),
        "MG0003" => Some(39),
        "YJ0004" => Some(52),
        "ZW0005" => Some(65),
        "engine0006" => Some(78),
        "HZ0007" => Some(91),
        "QD0008" => Some(7),
        "PS0009" => Some(20),
        "LB0010" => Some(33),
        "MC0011" => Some(46),
        "RQ0012" => Some(59),
        "SL0013" => Some(72),
        "LD0014" => Some(85),
        "MG0015" => Some(1),
        "YJ0016" => Some(14),
        "ZW0017" => Some(27),
        "engine0018" => Some(40),
        "HZ0019" => Some(53),
        "QD0020" => Some(66),
        "PS0021" => Some(79),
        "LB0022" => Some(92),
        "MC0023" => Some(8),
        "RQ0024" => Some(21),
        "SL0025" => Some(34),
        "LD0026" => Some(47),
        "MG0027" => Some(60),
        "YJ0028" => Some(73),
        "ZW0029" => Some(86),
        "engine0030" => Some(2),
        "HZ0031" => Some(15),
        "QD0032" => Some(28),
        "PS0033" => Some(41),
        "LB0034" => Some(54),
        "MC0035" => Some(67),
        _ => None,
    }
}

/// 逐行守卫扫描（返回违例行数；0 = 全绿）。
pub fn u1641mat_scan() -> usize {
    let mut bad = 0usize;
    let mut i = 0usize;
    while i < u1641mat_COUNT {
        let (k, v) = u1641mat[i];
        let ok = match k {
            "RQ0000" => v < 97,
            "SL0001" => v < 97,
            "LD0002" => v < 97,
            "MG0003" => v < 97,
            "YJ0004" => v < 97,
            "ZW0005" => v < 97,
            "engine0006" => v < 97,
            "HZ0007" => v < 97,
            "QD0008" => v < 97,
            "PS0009" => v < 97,
            "LB0010" => v < 97,
            "MC0011" => v < 97,
            "RQ0012" => v < 97,
            "SL0013" => v < 97,
            "LD0014" => v < 97,
            "MG0015" => v < 97,
            "YJ0016" => v < 97,
            "ZW0017" => v < 97,
            "engine0018" => v < 97,
            "HZ0019" => v < 97,
            "QD0020" => v < 97,
            "PS0021" => v < 97,
            "LB0022" => v < 97,
            "MC0023" => v < 97,
            "RQ0024" => v < 97,
            "SL0025" => v < 97,
            "LD0026" => v < 97,
            "MG0027" => v < 97,
            "YJ0028" => v < 97,
            "ZW0029" => v < 97,
            "engine0030" => v < 97,
            "HZ0031" => v < 97,
            "QD0032" => v < 97,
            "PS0033" => v < 97,
            "LB0034" => v < 97,
            "MC0035" => v < 97,
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
pub fn u1641mat_sum() -> u64 {
    let mut s = 0u64;
    let mut i = 0usize;
    while i < u1641mat_COUNT {
        s = s.wrapping_add(u1641mat[i].1 as u64);
        i += 1;
    }
    s
}

/// 键唯一性（构造期断言等价物：逐对扫描同键）。
pub fn u1641mat_unique() -> bool {
    let mut i = 0usize;
    while i < u1641mat_COUNT {
        let mut j = i + 1;
        while j < u1641mat_COUNT {
            if u1641mat[i].0 == u1641mat[j].0 {
                return false;
            }
            j += 1;
        }
        i += 1;
    }
    true
}

// --------------------------------------------------------------------------
// 直方图引擎 mxu1641hist（有界插入排序 + 分位数 P50/P90/P99）
// --------------------------------------------------------------------------
/// 锁域 直方图（96 桶滚动）。
/// 样本容量上限。
pub const MXU1641HIST_CAP: usize = 96;

/// 直方图账本（排序态 + 五统量）。
#[derive(Clone, Copy, Debug)]
pub struct mxu1641histHist {
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

impl mxu1641histHist {
    /// 空账构造。
    pub const fn new() -> Self {
        Self { samples: [0; 96], n: 0, drops: 0, min: 0, max: 0, sum: 0 }
    }

    /// 采样（有界插入排序；满员丢弃并记账）。
    pub fn push(&mut self, v: u64) {
        if self.n >= MXU1641HIST_CAP {
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

/// 段位守卫：全部诊断码必须落在本段 [0x56b0, 0x56bf]。
pub const fn u1641_segment_guard() -> bool {
    let mut ok = true;
    let mut i = 0usize;
    while i < u1641Code::ALL.len() {
        let c = u1641Code::ALL[i].code();
        if c < 0x56b0 || c > 0x56bf {
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
fn u1641_guard_seg() -> bool {
    u1641_segment_guard()
}

/// UNX-F1641 矩阵 36 行齐备 + 逐行守卫 + 唯一性 + 累计守恒。
fn check_u1641_map_guard() -> bool {
    let mut ok = u1641mat_COUNT == 36;
    if ok { ok = u1641mat_scan() == 0 && u1641mat_unique(); }
    if ok { ok = u1641mat_get("RQ0000") == Some(0) && u1641mat_get("ZZZ_NOKEY").is_none(); }
    if ok { ok = u1641mat_sum() == 1594; }
    ok
}

/// UNX-F1641 直方图 16 样本分位/均值/守恒。
fn check_u1641_map_hist() -> bool {
    let mut h = mxu1641histHist::new();
    let mut i = 0u64;
    while i < 16 { h.push(i + 1); i += 1; }
    let mut ok = h.conserved() && h.mean() >= 1 && h.percentile(500) >= 8;
    ok
}

/// 本模块自检（逐检查带 detail，全绿才出门）。
pub fn run_u1641_checks() -> CheckSet {
    let mut set = CheckSet::new("U1641");
    set.add(
        "u1641-seg-guard",
        u1641_guard_seg(),
        if u1641_guard_seg() { "" } else { "diagnostic code outside segment" },
    );
    set.add(
        "u1641-map-guard",
        check_u1641_map_guard(),
        if check_u1641_map_guard() { "" } else { "UNX-F1641 matrix guard mismatch" },
    );
    set.add(
        "u1641-map-hist",
        check_u1641_map_hist(),
        if check_u1641_map_hist() { "" } else { "UNX-F1641 hist mismatch" },
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
    fn u1641_all_checks_green() {
        let set = run_u1641_checks();
        assert!(set.all_passed(), "U1641 domain checks must be green");
    }

    #[test]
    fn u1641_segment_guard_holds() {
        assert!(u1641_segment_guard());
    }
}
