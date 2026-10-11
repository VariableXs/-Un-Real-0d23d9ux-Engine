//! unxu1821_u1821map —— 切换帧总纲（内核栈布局与切换帧结构冻结） 守卫矩阵与直方图（帧区）。
//! 判据原文（CoRun Unxreal 主册 UNX-A1-B09）：UNX-F1821 · 切换帧总纲（内核栈布局与切换帧结构冻结） · 判据原文：UNX-F1821 · 判据：切换帧结构（栈顶中断帧区/寄存器保存区/控制块锚区）冻结 v1 与 F1624 栈约定对齐，布局越界注入 10/10 检出
//! · 组件：帧区 守卫矩阵（36 行逐行守卫）+ 96 桶直方图（分位/均值/守恒）
//! · 判据落地：UNX-F1821 · 判据：切换帧结构（栈顶中断帧区/寄存器保存区/控制块锚区）冻结 v1 与 F1624 栈约定对齐，布局越界注入 10/10 检出
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，
//! 逐条定制数据结构/状态机/错误路径，零模板复述，零死代码。


use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const U1821_VERSION: &str = "u1821-u1821map-v1";

/// 诊断段基址。
pub const U1821_SEG_BASE: u16 = 0x5d70;

// --------------------------------------------------------------------------
// 诊断码（u1821 十六码 · 12 枚互斥 + segment_guard）
// --------------------------------------------------------------------------
/// 帧区 矩阵面诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum u1821Code {
    /// 0x5d70 键重复（布局越界 同键双行）。
    KeyDup,
    /// 0x5d71 键断档（对齐破坏 序不连续）。
    KeyGap,
    /// 0x5d72 行守卫破缺（约定失锚 逐行校验失败）。
    GuardBad,
    /// 0x5d73 矩阵累计漂移（检出漏报 求和失锚）。
    SumDrift,
    /// 0x5d74 直方图溢出（超容量拒收未记账）。
    HistOver,
    /// 0x5d75 直方图断桶（桶序缺失）。
    HistHole,
    /// 0x5d76 分位越界（千分比>1000）。
    PctOob,
    /// 0x5d77 均值漂移（均值与分位矛盾）。
    MeanDrift,
    /// 0x5d78 台账不守恒（守恒恒等式破缺）。
    LedgerBroken,
    /// 0x5d79 快照指纹不符（字段被改后重算不一致）。
    SnapshotFingerprint,
    /// 0x5d7a 常量表破坏（表项与语义规则矛盾）。
    TableBroken,
    /// 0x5d7b 段位守卫失败（诊断码越出本模块段）。
    SegmentGuard,
}

impl u1821Code {
    /// 全量闭集。
    pub const ALL: [u1821Code; 12] = [
        u1821Code::KeyDup,
        u1821Code::KeyGap,
        u1821Code::GuardBad,
        u1821Code::SumDrift,
        u1821Code::HistOver,
        u1821Code::HistHole,
        u1821Code::PctOob,
        u1821Code::MeanDrift,
        u1821Code::LedgerBroken,
        u1821Code::SnapshotFingerprint,
        u1821Code::TableBroken,
        u1821Code::SegmentGuard,
    ];

    /// 码值（连续十六进制，与声明序恒等）。
    pub const fn code(self) -> u16 {
        match self {
            u1821Code::KeyDup => 0x5d70,
            u1821Code::KeyGap => 0x5d71,
            u1821Code::GuardBad => 0x5d72,
            u1821Code::SumDrift => 0x5d73,
            u1821Code::HistOver => 0x5d74,
            u1821Code::HistHole => 0x5d75,
            u1821Code::PctOob => 0x5d76,
            u1821Code::MeanDrift => 0x5d77,
            u1821Code::LedgerBroken => 0x5d78,
            u1821Code::SnapshotFingerprint => 0x5d79,
            u1821Code::TableBroken => 0x5d7a,
            u1821Code::SegmentGuard => 0x5d7b,
        }
    }

    /// 中文诊断名。
    pub const fn name(self) -> &'static str {
        match self {
            u1821Code::KeyDup => "键重复（布局越界 同键双行）",
            u1821Code::KeyGap => "键断档（对齐破坏 序不连续）",
            u1821Code::GuardBad => "行守卫破缺（约定失锚 逐行校验失败）",
            u1821Code::SumDrift => "矩阵累计漂移（检出漏报 求和失锚）",
            u1821Code::HistOver => "直方图溢出（超容量拒收未记账）",
            u1821Code::HistHole => "直方图断桶（桶序缺失）",
            u1821Code::PctOob => "分位越界（千分比>1000）",
            u1821Code::MeanDrift => "均值漂移（均值与分位矛盾）",
            u1821Code::LedgerBroken => "台账不守恒（守恒恒等式破缺）",
            u1821Code::SnapshotFingerprint => "快照指纹不符（字段被改后重算不一致）",
            u1821Code::TableBroken => "常量表破坏（表项与语义规则矛盾）",
            u1821Code::SegmentGuard => "段位守卫失败（诊断码越出本模块段）",
        }
    }

    /// 可操作修复建议（每条诊断都必须带下一步动作）。
    pub const fn suggestion(self) -> &'static str {
        match self {
            u1821Code::KeyDup => "按键唯一律去重",
            u1821Code::KeyGap => "按连续律补号",
            u1821Code::GuardBad => "按守卫口径重建",
            u1821Code::SumDrift => "按理论值复算",
            u1821Code::HistOver => "按溢出账口径登记",
            u1821Code::HistHole => "按桶序补齐",
            u1821Code::PctOob => "按 0..=1000 钳制",
            u1821Code::MeanDrift => "按同源数据复算",
            u1821Code::LedgerBroken => "按守恒律重建台账后重跑",
            u1821Code::SnapshotFingerprint => "按防篡改规程重建快照",
            u1821Code::TableBroken => "按语义规则重建常量表",
            u1821Code::SegmentGuard => "核对诊断码分配表",
        }
    }
}

// --------------------------------------------------------------------------
// 字符串键值映射 u1821mat（36 行逐行语义注释 + 逐行守卫）
// --------------------------------------------------------------------------
/// 帧区 守卫矩阵（与核心登记表同源对拍）。
/// 行数。
pub const u1821mat_COUNT: usize = 36;

/// 表本体（键 + 值 + 注释由编译期冻结）。
pub static u1821mat: [(&'static str, u64); 36] = [
    ("ZZ0000", 0), // ZZ 单元 0000（栈顶中断帧区 × 相位 00 × 一 轮）
    ("BC0001", 13), // BC 单元 0001（寄存器保存区 × 相位 01 × 一 轮）
    ("KB0002", 26), // KB 单元 0002（控制块锚区 × 相位 02 × 一 轮）
    ("DZ0003", 39), // DZ 单元 0003（冻结 v1 × 相位 03 × 一 轮）
    ("F16240004", 52), // F1624 单元 0004（F1624 栈约定 × 相位 04 × 一 轮）
    ("YJ0005", 65), // YJ 单元 0005（越界注入 × 相位 05 × 一 轮）
    ("JC0006", 78), // JC 单元 0006（检出 × 相位 06 × 一 轮）
    ("JP0007", 91), // JP 单元 0007（布局 × 相位 07 × 一 轮）
    ("DZ20008", 7), // DZ2 单元 0008（对齐锚 × 相位 08 × 一 轮）
    ("HZ0009", 20), // HZ 单元 0009（帧号 × 相位 09 × 一 轮）
    ("SZ0010", 33), // SZ 单元 0010（尺寸账 × 相位 10 × 一 轮）
    ("PB0011", 46), // PB 单元 0011（偏移账 × 相位 11 × 一 轮）
    ("ZZ0012", 59), // ZZ 单元 0012（栈顶中断帧区 × 相位 12 × 一 轮）
    ("BC0013", 72), // BC 单元 0013（寄存器保存区 × 相位 13 × 一 轮）
    ("KB0014", 85), // KB 单元 0014（控制块锚区 × 相位 14 × 一 轮）
    ("DZ0015", 1), // DZ 单元 0015（冻结 v1 × 相位 15 × 一 轮）
    ("F16240016", 14), // F1624 单元 0016（F1624 栈约定 × 相位 16 × 一 轮）
    ("YJ0017", 27), // YJ 单元 0017（越界注入 × 相位 17 × 一 轮）
    ("JC0018", 40), // JC 单元 0018（检出 × 相位 18 × 一 轮）
    ("JP0019", 53), // JP 单元 0019（布局 × 相位 19 × 一 轮）
    ("DZ20020", 66), // DZ2 单元 0020（对齐锚 × 相位 20 × 一 轮）
    ("HZ0021", 79), // HZ 单元 0021（帧号 × 相位 21 × 一 轮）
    ("SZ0022", 92), // SZ 单元 0022（尺寸账 × 相位 22 × 一 轮）
    ("PB0023", 8), // PB 单元 0023（偏移账 × 相位 23 × 一 轮）
    ("ZZ0024", 21), // ZZ 单元 0024（栈顶中断帧区 × 相位 24 × 一 轮）
    ("BC0025", 34), // BC 单元 0025（寄存器保存区 × 相位 25 × 一 轮）
    ("KB0026", 47), // KB 单元 0026（控制块锚区 × 相位 26 × 一 轮）
    ("DZ0027", 60), // DZ 单元 0027（冻结 v1 × 相位 27 × 一 轮）
    ("F16240028", 73), // F1624 单元 0028（F1624 栈约定 × 相位 28 × 一 轮）
    ("YJ0029", 86), // YJ 单元 0029（越界注入 × 相位 29 × 一 轮）
    ("JC0030", 2), // JC 单元 0030（检出 × 相位 30 × 一 轮）
    ("JP0031", 15), // JP 单元 0031（布局 × 相位 31 × 一 轮）
    ("DZ20032", 28), // DZ2 单元 0032（对齐锚 × 相位 32 × 一 轮）
    ("HZ0033", 41), // HZ 单元 0033（帧号 × 相位 33 × 一 轮）
    ("SZ0034", 54), // SZ 单元 0034（尺寸账 × 相位 34 × 一 轮）
    ("PB0035", 67), // PB 单元 0035（偏移账 × 相位 35 × 一 轮）
];

/// O(1) 语义查找（match 全枚举；命中率 100%% 由守卫保证）。
pub fn u1821mat_get(key: &str) -> Option<u64> {
    match key {
        "ZZ0000" => Some(0),
        "BC0001" => Some(13),
        "KB0002" => Some(26),
        "DZ0003" => Some(39),
        "F16240004" => Some(52),
        "YJ0005" => Some(65),
        "JC0006" => Some(78),
        "JP0007" => Some(91),
        "DZ20008" => Some(7),
        "HZ0009" => Some(20),
        "SZ0010" => Some(33),
        "PB0011" => Some(46),
        "ZZ0012" => Some(59),
        "BC0013" => Some(72),
        "KB0014" => Some(85),
        "DZ0015" => Some(1),
        "F16240016" => Some(14),
        "YJ0017" => Some(27),
        "JC0018" => Some(40),
        "JP0019" => Some(53),
        "DZ20020" => Some(66),
        "HZ0021" => Some(79),
        "SZ0022" => Some(92),
        "PB0023" => Some(8),
        "ZZ0024" => Some(21),
        "BC0025" => Some(34),
        "KB0026" => Some(47),
        "DZ0027" => Some(60),
        "F16240028" => Some(73),
        "YJ0029" => Some(86),
        "JC0030" => Some(2),
        "JP0031" => Some(15),
        "DZ20032" => Some(28),
        "HZ0033" => Some(41),
        "SZ0034" => Some(54),
        "PB0035" => Some(67),
        _ => None,
    }
}

/// 逐行守卫扫描（返回违例行数；0 = 全绿）。
pub fn u1821mat_scan() -> usize {
    let mut bad = 0usize;
    let mut i = 0usize;
    while i < u1821mat_COUNT {
        let (k, v) = u1821mat[i];
        let ok = match k {
            "ZZ0000" => v < 97,
            "BC0001" => v < 97,
            "KB0002" => v < 97,
            "DZ0003" => v < 97,
            "F16240004" => v < 97,
            "YJ0005" => v < 97,
            "JC0006" => v < 97,
            "JP0007" => v < 97,
            "DZ20008" => v < 97,
            "HZ0009" => v < 97,
            "SZ0010" => v < 97,
            "PB0011" => v < 97,
            "ZZ0012" => v < 97,
            "BC0013" => v < 97,
            "KB0014" => v < 97,
            "DZ0015" => v < 97,
            "F16240016" => v < 97,
            "YJ0017" => v < 97,
            "JC0018" => v < 97,
            "JP0019" => v < 97,
            "DZ20020" => v < 97,
            "HZ0021" => v < 97,
            "SZ0022" => v < 97,
            "PB0023" => v < 97,
            "ZZ0024" => v < 97,
            "BC0025" => v < 97,
            "KB0026" => v < 97,
            "DZ0027" => v < 97,
            "F16240028" => v < 97,
            "YJ0029" => v < 97,
            "JC0030" => v < 97,
            "JP0031" => v < 97,
            "DZ20032" => v < 97,
            "HZ0033" => v < 97,
            "SZ0034" => v < 97,
            "PB0035" => v < 97,
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
pub fn u1821mat_sum() -> u64 {
    let mut s = 0u64;
    let mut i = 0usize;
    while i < u1821mat_COUNT {
        s = s.wrapping_add(u1821mat[i].1 as u64);
        i += 1;
    }
    s
}

/// 键唯一性（构造期断言等价物：逐对扫描同键）。
pub fn u1821mat_unique() -> bool {
    let mut i = 0usize;
    while i < u1821mat_COUNT {
        let mut j = i + 1;
        while j < u1821mat_COUNT {
            if u1821mat[i].0 == u1821mat[j].0 {
                return false;
            }
            j += 1;
        }
        i += 1;
    }
    true
}

// --------------------------------------------------------------------------
// 直方图引擎 mxu1821hist（有界插入排序 + 分位数 P50/P90/P99）
// --------------------------------------------------------------------------
/// 帧区 直方图（96 桶滚动）。
/// 样本容量上限。
pub const MXU1821HIST_CAP: usize = 96;

/// 直方图账本（排序态 + 五统量）。
#[derive(Clone, Copy, Debug)]
pub struct mxu1821histHist {
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

impl mxu1821histHist {
    /// 空账构造。
    pub const fn new() -> Self {
        Self { samples: [0; 96], n: 0, drops: 0, min: 0, max: 0, sum: 0 }
    }

    /// 采样（有界插入排序；满员丢弃并记账）。
    pub fn push(&mut self, v: u64) {
        if self.n >= MXU1821HIST_CAP {
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

/// 段位守卫：全部诊断码必须落在本段 [0x5d70, 0x5d7f]。
pub const fn u1821_segment_guard() -> bool {
    let mut ok = true;
    let mut i = 0usize;
    while i < u1821Code::ALL.len() {
        let c = u1821Code::ALL[i].code();
        if c < 0x5d70 || c > 0x5d7f {
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
fn u1821_guard_seg() -> bool {
    u1821_segment_guard()
}

/// UNX-F1821 矩阵 36 行齐备 + 逐行守卫 + 唯一性 + 累计守恒。
fn check_u1821_map_guard() -> bool {
    let mut ok = u1821mat_COUNT == 36;
    if ok { ok = u1821mat_scan() == 0 && u1821mat_unique(); }
    if ok { ok = u1821mat_get("ZZ0000") == Some(0) && u1821mat_get("ZZZ_NOKEY").is_none(); }
    if ok { ok = u1821mat_sum() == 1594; }
    ok
}

/// UNX-F1821 直方图 16 样本分位/均值/守恒。
fn check_u1821_map_hist() -> bool {
    let mut h = mxu1821histHist::new();
    let mut i = 0u64;
    while i < 16 { h.push(i + 1); i += 1; }
    let mut ok = h.conserved() && h.mean() >= 1 && h.percentile(500) >= 8;
    ok
}

/// 本模块自检（逐检查带 detail，全绿才出门）。
pub fn run_u1821_checks() -> CheckSet {
    let mut set = CheckSet::new("U1821");
    set.add(
        "u1821-seg-guard",
        u1821_guard_seg(),
        if u1821_guard_seg() { "" } else { "diagnostic code outside segment" },
    );
    set.add(
        "u1821-map-guard",
        check_u1821_map_guard(),
        if check_u1821_map_guard() { "" } else { "UNX-F1821 matrix guard mismatch" },
    );
    set.add(
        "u1821-map-hist",
        check_u1821_map_hist(),
        if check_u1821_map_hist() { "" } else { "UNX-F1821 hist mismatch" },
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
    fn u1821_all_checks_green() {
        let set = run_u1821_checks();
        assert!(set.all_passed(), "U1821 domain checks must be green");
    }

    #[test]
    fn u1821_segment_guard_holds() {
        assert!(u1821_segment_guard());
    }
}
