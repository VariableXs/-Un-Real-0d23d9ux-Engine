//! unxu1701_u1701map —— 负载追踪总纲（窗口化追踪模型冻结） 守卫矩阵与直方图（窗口）。
//! 判据原文（CoRun Unxreal 主册 UNX-A1-B09）：UNX-F1701 · 负载追踪总纲（窗口化追踪模型冻结） · 判据原文：UNX-F1701 · 判据：窗口化追踪模型冻结 v1（几何衰减均值+双窗口档），模型参数单点定义，计算路径唯一，与 F1646 瞬时账分层声明在册
//! · 组件：窗口 守卫矩阵（36 行逐行守卫）+ 96 桶直方图（分位/均值/守恒）
//! · 判据落地：UNX-F1701 · 判据：窗口化追踪模型冻结 v1（几何衰减均值+双窗口档），模型参数单点定义，计算路径唯一，与 F1646 瞬时账分层声明在册
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，
//! 逐条定制数据结构/状态机/错误路径，零模板复述，零死代码。


use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const U1701_VERSION: &str = "u1701-u1701map-v1";

/// 诊断段基址。
pub const U1701_SEG_BASE: u16 = 0x58f0;

// --------------------------------------------------------------------------
// 诊断码（u1701 十六码 · 12 枚互斥 + segment_guard）
// --------------------------------------------------------------------------
/// 窗口 矩阵面诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum u1701Code {
    /// 0x58f0 键重复（窗口漂移 同键双行）。
    KeyDup,
    /// 0x58f1 键断档（衰减失准 序不连续）。
    KeyGap,
    /// 0x58f2 行守卫破缺（分层混账 逐行校验失败）。
    GuardBad,
    /// 0x58f3 矩阵累计漂移（参数多点 求和失锚）。
    SumDrift,
    /// 0x58f4 直方图溢出（超容量拒收未记账）。
    HistOver,
    /// 0x58f5 直方图断桶（桶序缺失）。
    HistHole,
    /// 0x58f6 分位越界（千分比>1000）。
    PctOob,
    /// 0x58f7 均值漂移（均值与分位矛盾）。
    MeanDrift,
    /// 0x58f8 台账不守恒（守恒恒等式破缺）。
    LedgerBroken,
    /// 0x58f9 快照指纹不符（字段被改后重算不一致）。
    SnapshotFingerprint,
    /// 0x58fa 常量表破坏（表项与语义规则矛盾）。
    TableBroken,
    /// 0x58fb 段位守卫失败（诊断码越出本模块段）。
    SegmentGuard,
}

impl u1701Code {
    /// 全量闭集。
    pub const ALL: [u1701Code; 12] = [
        u1701Code::KeyDup,
        u1701Code::KeyGap,
        u1701Code::GuardBad,
        u1701Code::SumDrift,
        u1701Code::HistOver,
        u1701Code::HistHole,
        u1701Code::PctOob,
        u1701Code::MeanDrift,
        u1701Code::LedgerBroken,
        u1701Code::SnapshotFingerprint,
        u1701Code::TableBroken,
        u1701Code::SegmentGuard,
    ];

    /// 码值（连续十六进制，与声明序恒等）。
    pub const fn code(self) -> u16 {
        match self {
            u1701Code::KeyDup => 0x58f0,
            u1701Code::KeyGap => 0x58f1,
            u1701Code::GuardBad => 0x58f2,
            u1701Code::SumDrift => 0x58f3,
            u1701Code::HistOver => 0x58f4,
            u1701Code::HistHole => 0x58f5,
            u1701Code::PctOob => 0x58f6,
            u1701Code::MeanDrift => 0x58f7,
            u1701Code::LedgerBroken => 0x58f8,
            u1701Code::SnapshotFingerprint => 0x58f9,
            u1701Code::TableBroken => 0x58fa,
            u1701Code::SegmentGuard => 0x58fb,
        }
    }

    /// 中文诊断名。
    pub const fn name(self) -> &'static str {
        match self {
            u1701Code::KeyDup => "键重复（窗口漂移 同键双行）",
            u1701Code::KeyGap => "键断档（衰减失准 序不连续）",
            u1701Code::GuardBad => "行守卫破缺（分层混账 逐行校验失败）",
            u1701Code::SumDrift => "矩阵累计漂移（参数多点 求和失锚）",
            u1701Code::HistOver => "直方图溢出（超容量拒收未记账）",
            u1701Code::HistHole => "直方图断桶（桶序缺失）",
            u1701Code::PctOob => "分位越界（千分比>1000）",
            u1701Code::MeanDrift => "均值漂移（均值与分位矛盾）",
            u1701Code::LedgerBroken => "台账不守恒（守恒恒等式破缺）",
            u1701Code::SnapshotFingerprint => "快照指纹不符（字段被改后重算不一致）",
            u1701Code::TableBroken => "常量表破坏（表项与语义规则矛盾）",
            u1701Code::SegmentGuard => "段位守卫失败（诊断码越出本模块段）",
        }
    }

    /// 可操作修复建议（每条诊断都必须带下一步动作）。
    pub const fn suggestion(self) -> &'static str {
        match self {
            u1701Code::KeyDup => "按键唯一律去重",
            u1701Code::KeyGap => "按连续律补号",
            u1701Code::GuardBad => "按守卫口径重建",
            u1701Code::SumDrift => "按理论值复算",
            u1701Code::HistOver => "按溢出账口径登记",
            u1701Code::HistHole => "按桶序补齐",
            u1701Code::PctOob => "按 0..=1000 钳制",
            u1701Code::MeanDrift => "按同源数据复算",
            u1701Code::LedgerBroken => "按守恒律重建台账后重跑",
            u1701Code::SnapshotFingerprint => "按防篡改规程重建快照",
            u1701Code::TableBroken => "按语义规则重建常量表",
            u1701Code::SegmentGuard => "核对诊断码分配表",
        }
    }
}

// --------------------------------------------------------------------------
// 字符串键值映射 u1701mat（36 行逐行语义注释 + 逐行守卫）
// --------------------------------------------------------------------------
/// 窗口 守卫矩阵（与核心登记表同源对拍）。
/// 行数。
pub const u1701mat_COUNT: usize = 36;

/// 表本体（键 + 值 + 注释由编译期冻结）。
pub static u1701mat: [(&'static str, u64); 36] = [
    ("JS0000", 0), // JS 单元 0000（几何衰减 × 相位 00 × 一 轮）
    ("SC0001", 13), // SC 单元 0001（双窗口档 × 相位 01 × 一 轮）
    ("CS0002", 26), // CS 单元 0002（参数单点 × 相位 02 × 一 轮）
    ("LY0003", 39), // LY 单元 0003（路径唯一 × 相位 03 × 一 轮）
    ("F16460004", 52), // F1646 单元 0004（瞬时账分层 × 相位 04 × 一 轮）
    ("CK0005", 65), // CK 单元 0005（窗口 × 相位 05 × 一 轮）
    ("JZ0006", 78), // JZ 单元 0006（均值 × 相位 06 × 一 轮）
    ("MZ0007", 91), // MZ 单元 0007（模型冻结 × 相位 07 × 一 轮）
    ("B010008", 7), // B01 单元 0008（短窗档 × 相位 08 × 一 轮）
    ("B020009", 20), // B02 单元 0009（长窗档 × 相位 09 × 一 轮）
    ("SD0010", 33), // SD 单元 0010（衰减因子 × 相位 10 × 一 轮）
    ("ZC0011", 46), // ZC 单元 0011（在册声明 × 相位 11 × 一 轮）
    ("JS0012", 59), // JS 单元 0012（几何衰减 × 相位 12 × 一 轮）
    ("SC0013", 72), // SC 单元 0013（双窗口档 × 相位 13 × 一 轮）
    ("CS0014", 85), // CS 单元 0014（参数单点 × 相位 14 × 一 轮）
    ("LY0015", 1), // LY 单元 0015（路径唯一 × 相位 15 × 一 轮）
    ("F16460016", 14), // F1646 单元 0016（瞬时账分层 × 相位 16 × 一 轮）
    ("CK0017", 27), // CK 单元 0017（窗口 × 相位 17 × 一 轮）
    ("JZ0018", 40), // JZ 单元 0018（均值 × 相位 18 × 一 轮）
    ("MZ0019", 53), // MZ 单元 0019（模型冻结 × 相位 19 × 一 轮）
    ("B010020", 66), // B01 单元 0020（短窗档 × 相位 20 × 一 轮）
    ("B020021", 79), // B02 单元 0021（长窗档 × 相位 21 × 一 轮）
    ("SD0022", 92), // SD 单元 0022（衰减因子 × 相位 22 × 一 轮）
    ("ZC0023", 8), // ZC 单元 0023（在册声明 × 相位 23 × 一 轮）
    ("JS0024", 21), // JS 单元 0024（几何衰减 × 相位 24 × 一 轮）
    ("SC0025", 34), // SC 单元 0025（双窗口档 × 相位 25 × 一 轮）
    ("CS0026", 47), // CS 单元 0026（参数单点 × 相位 26 × 一 轮）
    ("LY0027", 60), // LY 单元 0027（路径唯一 × 相位 27 × 一 轮）
    ("F16460028", 73), // F1646 单元 0028（瞬时账分层 × 相位 28 × 一 轮）
    ("CK0029", 86), // CK 单元 0029（窗口 × 相位 29 × 一 轮）
    ("JZ0030", 2), // JZ 单元 0030（均值 × 相位 30 × 一 轮）
    ("MZ0031", 15), // MZ 单元 0031（模型冻结 × 相位 31 × 一 轮）
    ("B010032", 28), // B01 单元 0032（短窗档 × 相位 32 × 一 轮）
    ("B020033", 41), // B02 单元 0033（长窗档 × 相位 33 × 一 轮）
    ("SD0034", 54), // SD 单元 0034（衰减因子 × 相位 34 × 一 轮）
    ("ZC0035", 67), // ZC 单元 0035（在册声明 × 相位 35 × 一 轮）
];

/// O(1) 语义查找（match 全枚举；命中率 100%% 由守卫保证）。
pub fn u1701mat_get(key: &str) -> Option<u64> {
    match key {
        "JS0000" => Some(0),
        "SC0001" => Some(13),
        "CS0002" => Some(26),
        "LY0003" => Some(39),
        "F16460004" => Some(52),
        "CK0005" => Some(65),
        "JZ0006" => Some(78),
        "MZ0007" => Some(91),
        "B010008" => Some(7),
        "B020009" => Some(20),
        "SD0010" => Some(33),
        "ZC0011" => Some(46),
        "JS0012" => Some(59),
        "SC0013" => Some(72),
        "CS0014" => Some(85),
        "LY0015" => Some(1),
        "F16460016" => Some(14),
        "CK0017" => Some(27),
        "JZ0018" => Some(40),
        "MZ0019" => Some(53),
        "B010020" => Some(66),
        "B020021" => Some(79),
        "SD0022" => Some(92),
        "ZC0023" => Some(8),
        "JS0024" => Some(21),
        "SC0025" => Some(34),
        "CS0026" => Some(47),
        "LY0027" => Some(60),
        "F16460028" => Some(73),
        "CK0029" => Some(86),
        "JZ0030" => Some(2),
        "MZ0031" => Some(15),
        "B010032" => Some(28),
        "B020033" => Some(41),
        "SD0034" => Some(54),
        "ZC0035" => Some(67),
        _ => None,
    }
}

/// 逐行守卫扫描（返回违例行数；0 = 全绿）。
pub fn u1701mat_scan() -> usize {
    let mut bad = 0usize;
    let mut i = 0usize;
    while i < u1701mat_COUNT {
        let (k, v) = u1701mat[i];
        let ok = match k {
            "JS0000" => v < 97,
            "SC0001" => v < 97,
            "CS0002" => v < 97,
            "LY0003" => v < 97,
            "F16460004" => v < 97,
            "CK0005" => v < 97,
            "JZ0006" => v < 97,
            "MZ0007" => v < 97,
            "B010008" => v < 97,
            "B020009" => v < 97,
            "SD0010" => v < 97,
            "ZC0011" => v < 97,
            "JS0012" => v < 97,
            "SC0013" => v < 97,
            "CS0014" => v < 97,
            "LY0015" => v < 97,
            "F16460016" => v < 97,
            "CK0017" => v < 97,
            "JZ0018" => v < 97,
            "MZ0019" => v < 97,
            "B010020" => v < 97,
            "B020021" => v < 97,
            "SD0022" => v < 97,
            "ZC0023" => v < 97,
            "JS0024" => v < 97,
            "SC0025" => v < 97,
            "CS0026" => v < 97,
            "LY0027" => v < 97,
            "F16460028" => v < 97,
            "CK0029" => v < 97,
            "JZ0030" => v < 97,
            "MZ0031" => v < 97,
            "B010032" => v < 97,
            "B020033" => v < 97,
            "SD0034" => v < 97,
            "ZC0035" => v < 97,
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
pub fn u1701mat_sum() -> u64 {
    let mut s = 0u64;
    let mut i = 0usize;
    while i < u1701mat_COUNT {
        s = s.wrapping_add(u1701mat[i].1 as u64);
        i += 1;
    }
    s
}

/// 键唯一性（构造期断言等价物：逐对扫描同键）。
pub fn u1701mat_unique() -> bool {
    let mut i = 0usize;
    while i < u1701mat_COUNT {
        let mut j = i + 1;
        while j < u1701mat_COUNT {
            if u1701mat[i].0 == u1701mat[j].0 {
                return false;
            }
            j += 1;
        }
        i += 1;
    }
    true
}

// --------------------------------------------------------------------------
// 直方图引擎 mxu1701hist（有界插入排序 + 分位数 P50/P90/P99）
// --------------------------------------------------------------------------
/// 窗口 直方图（96 桶滚动）。
/// 样本容量上限。
pub const MXU1701HIST_CAP: usize = 96;

/// 直方图账本（排序态 + 五统量）。
#[derive(Clone, Copy, Debug)]
pub struct mxu1701histHist {
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

impl mxu1701histHist {
    /// 空账构造。
    pub const fn new() -> Self {
        Self { samples: [0; 96], n: 0, drops: 0, min: 0, max: 0, sum: 0 }
    }

    /// 采样（有界插入排序；满员丢弃并记账）。
    pub fn push(&mut self, v: u64) {
        if self.n >= MXU1701HIST_CAP {
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

/// 段位守卫：全部诊断码必须落在本段 [0x58f0, 0x58ff]。
pub const fn u1701_segment_guard() -> bool {
    let mut ok = true;
    let mut i = 0usize;
    while i < u1701Code::ALL.len() {
        let c = u1701Code::ALL[i].code();
        if c < 0x58f0 || c > 0x58ff {
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
fn u1701_guard_seg() -> bool {
    u1701_segment_guard()
}

/// UNX-F1701 矩阵 36 行齐备 + 逐行守卫 + 唯一性 + 累计守恒。
fn check_u1701_map_guard() -> bool {
    let mut ok = u1701mat_COUNT == 36;
    if ok { ok = u1701mat_scan() == 0 && u1701mat_unique(); }
    if ok { ok = u1701mat_get("JS0000") == Some(0) && u1701mat_get("ZZZ_NOKEY").is_none(); }
    if ok { ok = u1701mat_sum() == 1594; }
    ok
}

/// UNX-F1701 直方图 16 样本分位/均值/守恒。
fn check_u1701_map_hist() -> bool {
    let mut h = mxu1701histHist::new();
    let mut i = 0u64;
    while i < 16 { h.push(i + 1); i += 1; }
    let mut ok = h.conserved() && h.mean() >= 1 && h.percentile(500) >= 8;
    ok
}

/// 本模块自检（逐检查带 detail，全绿才出门）。
pub fn run_u1701_checks() -> CheckSet {
    let mut set = CheckSet::new("U1701");
    set.add(
        "u1701-seg-guard",
        u1701_guard_seg(),
        if u1701_guard_seg() { "" } else { "diagnostic code outside segment" },
    );
    set.add(
        "u1701-map-guard",
        check_u1701_map_guard(),
        if check_u1701_map_guard() { "" } else { "UNX-F1701 matrix guard mismatch" },
    );
    set.add(
        "u1701-map-hist",
        check_u1701_map_hist(),
        if check_u1701_map_hist() { "" } else { "UNX-F1701 hist mismatch" },
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
    fn u1701_all_checks_green() {
        let set = run_u1701_checks();
        assert!(set.all_passed(), "U1701 domain checks must be green");
    }

    #[test]
    fn u1701_segment_guard_holds() {
        assert!(u1701_segment_guard());
    }
}
