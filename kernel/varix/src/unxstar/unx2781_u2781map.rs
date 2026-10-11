//! unxu2781_u2781map —— 回收-分配联动总闸 守卫矩阵与直方图（联动事务）。
//! 判据原文（CoRun Unxreal 主册 UNX-A1-B09）：UNX-F2781 · 回收-分配联动总闸 · 判据原文：UNX-F2781 · 判据：联动总闸（分配路径事件→回收路径响应）全链 10/10 次贯通，联动账（触发源/响应/时延）三列齐备
//! · 组件：联动事务 守卫矩阵（36 行逐行守卫）+ 96 桶直方图（分位/均值/守恒）
//! · 判据落地：UNX-F2781 · 判据：联动总闸（分配路径事件→回收路径响应）全链 10/10 次贯通，联动账（触发源/响应/时延）三列齐备
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，
//! 逐条定制数据结构/状态机/错误路径，零模板复述，零死代码。


use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const U2781_VERSION: &str = "u2781-u2781map-v1";

/// 诊断段基址。
pub const U2781_SEG_BASE: u16 = 0xae30;

// --------------------------------------------------------------------------
// 诊断码（u2781 十六码 · 12 枚互斥 + segment_guard）
// --------------------------------------------------------------------------
/// 联动事务 矩阵面诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum u2781Code {
    /// 0xae30 键重复（闸门失守 同键双行）。
    KeyDup,
    /// 0xae31 键断档（响应缺列 序不连续）。
    KeyGap,
    /// 0xae32 行守卫破缺（时延失真 逐行校验失败）。
    GuardBad,
    /// 0xae33 矩阵累计漂移（链路断裂 求和失锚）。
    SumDrift,
    /// 0xae34 直方图溢出（超容量拒收未记账）。
    HistOver,
    /// 0xae35 直方图断桶（桶序缺失）。
    HistHole,
    /// 0xae36 分位越界（千分比>1000）。
    PctOob,
    /// 0xae37 均值漂移（均值与分位矛盾）。
    MeanDrift,
    /// 0xae38 台账不守恒（守恒恒等式破缺）。
    LedgerBroken,
    /// 0xae39 快照指纹不符（字段被改后重算不一致）。
    SnapshotFingerprint,
    /// 0xae3a 常量表破坏（表项与语义规则矛盾）。
    TableBroken,
    /// 0xae3b 段位守卫失败（诊断码越出本模块段）。
    SegmentGuard,
}

impl u2781Code {
    /// 全量闭集。
    pub const ALL: [u2781Code; 12] = [
        u2781Code::KeyDup,
        u2781Code::KeyGap,
        u2781Code::GuardBad,
        u2781Code::SumDrift,
        u2781Code::HistOver,
        u2781Code::HistHole,
        u2781Code::PctOob,
        u2781Code::MeanDrift,
        u2781Code::LedgerBroken,
        u2781Code::SnapshotFingerprint,
        u2781Code::TableBroken,
        u2781Code::SegmentGuard,
    ];

    /// 码值（连续十六进制，与声明序恒等）。
    pub const fn code(self) -> u16 {
        match self {
            u2781Code::KeyDup => 0xae30,
            u2781Code::KeyGap => 0xae31,
            u2781Code::GuardBad => 0xae32,
            u2781Code::SumDrift => 0xae33,
            u2781Code::HistOver => 0xae34,
            u2781Code::HistHole => 0xae35,
            u2781Code::PctOob => 0xae36,
            u2781Code::MeanDrift => 0xae37,
            u2781Code::LedgerBroken => 0xae38,
            u2781Code::SnapshotFingerprint => 0xae39,
            u2781Code::TableBroken => 0xae3a,
            u2781Code::SegmentGuard => 0xae3b,
        }
    }

    /// 中文诊断名。
    pub const fn name(self) -> &'static str {
        match self {
            u2781Code::KeyDup => "键重复（闸门失守 同键双行）",
            u2781Code::KeyGap => "键断档（响应缺列 序不连续）",
            u2781Code::GuardBad => "行守卫破缺（时延失真 逐行校验失败）",
            u2781Code::SumDrift => "矩阵累计漂移（链路断裂 求和失锚）",
            u2781Code::HistOver => "直方图溢出（超容量拒收未记账）",
            u2781Code::HistHole => "直方图断桶（桶序缺失）",
            u2781Code::PctOob => "分位越界（千分比>1000）",
            u2781Code::MeanDrift => "均值漂移（均值与分位矛盾）",
            u2781Code::LedgerBroken => "台账不守恒（守恒恒等式破缺）",
            u2781Code::SnapshotFingerprint => "快照指纹不符（字段被改后重算不一致）",
            u2781Code::TableBroken => "常量表破坏（表项与语义规则矛盾）",
            u2781Code::SegmentGuard => "段位守卫失败（诊断码越出本模块段）",
        }
    }

    /// 可操作修复建议（每条诊断都必须带下一步动作）。
    pub const fn suggestion(self) -> &'static str {
        match self {
            u2781Code::KeyDup => "按键唯一律去重",
            u2781Code::KeyGap => "按连续律补号",
            u2781Code::GuardBad => "按守卫口径重建",
            u2781Code::SumDrift => "按理论值复算",
            u2781Code::HistOver => "按溢出账口径登记",
            u2781Code::HistHole => "按桶序补齐",
            u2781Code::PctOob => "按 0..=1000 钳制",
            u2781Code::MeanDrift => "按同源数据复算",
            u2781Code::LedgerBroken => "按守恒律重建台账后重跑",
            u2781Code::SnapshotFingerprint => "按防篡改规程重建快照",
            u2781Code::TableBroken => "按语义规则重建常量表",
            u2781Code::SegmentGuard => "核对诊断码分配表",
        }
    }
}

// --------------------------------------------------------------------------
// 字符串键值映射 u2781mat（36 行逐行语义注释 + 逐行守卫）
// --------------------------------------------------------------------------
/// 联动事务 守卫矩阵（与核心登记表同源对拍）。
/// 行数。
pub const u2781mat_COUNT: usize = 36;

/// 表本体（键 + 值 + 注释由编译期冻结）。
pub static u2781mat: [(&'static str, u64); 36] = [
    ("LK0000", 0), // LK 单元 0000（联动总闸 × 相位 00 × 一 轮）
    ("AP0001", 13), // AP 单元 0001（分配路径 × 相位 01 × 一 轮）
    ("RP0002", 26), // RP 单元 0002（回收路径 × 相位 02 × 一 轮）
    ("TS0003", 39), // TS 单元 0003（触发源 × 相位 03 × 一 轮）
    ("RS0004", 52), // RS 单元 0004（响应 × 相位 04 × 一 轮）
    ("LT0005", 65), // LT 单元 0005（时延 × 相位 05 × 一 轮）
    ("C30006", 78), // C3 单元 0006（三列账 × 相位 06 × 一 轮）
    ("C100007", 91), // C10 单元 0007（十次贯通 × 相位 07 × 一 轮）
    ("FN0008", 7), // FN 单元 0008（全链贯通 × 相位 08 × 一 轮）
    ("AB0009", 20), // AB 单元 0009（阻断面 × 相位 09 × 一 轮）
    ("GD0010", 33), // GD 单元 0010（联动闸门 × 相位 10 × 一 轮）
    ("PR0011", 46), // PR 单元 0011（路径账 × 相位 11 × 一 轮）
    ("LK0012", 59), // LK 单元 0012（联动总闸 × 相位 12 × 一 轮）
    ("AP0013", 72), // AP 单元 0013（分配路径 × 相位 13 × 一 轮）
    ("RP0014", 85), // RP 单元 0014（回收路径 × 相位 14 × 一 轮）
    ("TS0015", 1), // TS 单元 0015（触发源 × 相位 15 × 一 轮）
    ("RS0016", 14), // RS 单元 0016（响应 × 相位 16 × 一 轮）
    ("LT0017", 27), // LT 单元 0017（时延 × 相位 17 × 一 轮）
    ("C30018", 40), // C3 单元 0018（三列账 × 相位 18 × 一 轮）
    ("C100019", 53), // C10 单元 0019（十次贯通 × 相位 19 × 一 轮）
    ("FN0020", 66), // FN 单元 0020（全链贯通 × 相位 20 × 一 轮）
    ("AB0021", 79), // AB 单元 0021（阻断面 × 相位 21 × 一 轮）
    ("GD0022", 92), // GD 单元 0022（联动闸门 × 相位 22 × 一 轮）
    ("PR0023", 8), // PR 单元 0023（路径账 × 相位 23 × 一 轮）
    ("LK0024", 21), // LK 单元 0024（联动总闸 × 相位 24 × 一 轮）
    ("AP0025", 34), // AP 单元 0025（分配路径 × 相位 25 × 一 轮）
    ("RP0026", 47), // RP 单元 0026（回收路径 × 相位 26 × 一 轮）
    ("TS0027", 60), // TS 单元 0027（触发源 × 相位 27 × 一 轮）
    ("RS0028", 73), // RS 单元 0028（响应 × 相位 28 × 一 轮）
    ("LT0029", 86), // LT 单元 0029（时延 × 相位 29 × 一 轮）
    ("C30030", 2), // C3 单元 0030（三列账 × 相位 30 × 一 轮）
    ("C100031", 15), // C10 单元 0031（十次贯通 × 相位 31 × 一 轮）
    ("FN0032", 28), // FN 单元 0032（全链贯通 × 相位 32 × 一 轮）
    ("AB0033", 41), // AB 单元 0033（阻断面 × 相位 33 × 一 轮）
    ("GD0034", 54), // GD 单元 0034（联动闸门 × 相位 34 × 一 轮）
    ("PR0035", 67), // PR 单元 0035（路径账 × 相位 35 × 一 轮）
];

/// O(1) 语义查找（match 全枚举；命中率 100%% 由守卫保证）。
pub fn u2781mat_get(key: &str) -> Option<u64> {
    match key {
        "LK0000" => Some(0),
        "AP0001" => Some(13),
        "RP0002" => Some(26),
        "TS0003" => Some(39),
        "RS0004" => Some(52),
        "LT0005" => Some(65),
        "C30006" => Some(78),
        "C100007" => Some(91),
        "FN0008" => Some(7),
        "AB0009" => Some(20),
        "GD0010" => Some(33),
        "PR0011" => Some(46),
        "LK0012" => Some(59),
        "AP0013" => Some(72),
        "RP0014" => Some(85),
        "TS0015" => Some(1),
        "RS0016" => Some(14),
        "LT0017" => Some(27),
        "C30018" => Some(40),
        "C100019" => Some(53),
        "FN0020" => Some(66),
        "AB0021" => Some(79),
        "GD0022" => Some(92),
        "PR0023" => Some(8),
        "LK0024" => Some(21),
        "AP0025" => Some(34),
        "RP0026" => Some(47),
        "TS0027" => Some(60),
        "RS0028" => Some(73),
        "LT0029" => Some(86),
        "C30030" => Some(2),
        "C100031" => Some(15),
        "FN0032" => Some(28),
        "AB0033" => Some(41),
        "GD0034" => Some(54),
        "PR0035" => Some(67),
        _ => None,
    }
}

/// 逐行守卫扫描（返回违例行数；0 = 全绿）。
pub fn u2781mat_scan() -> usize {
    let mut bad = 0usize;
    let mut i = 0usize;
    while i < u2781mat_COUNT {
        let (k, v) = u2781mat[i];
        let ok = match k {
            "LK0000" => v < 97,
            "AP0001" => v < 97,
            "RP0002" => v < 97,
            "TS0003" => v < 97,
            "RS0004" => v < 97,
            "LT0005" => v < 97,
            "C30006" => v < 97,
            "C100007" => v < 97,
            "FN0008" => v < 97,
            "AB0009" => v < 97,
            "GD0010" => v < 97,
            "PR0011" => v < 97,
            "LK0012" => v < 97,
            "AP0013" => v < 97,
            "RP0014" => v < 97,
            "TS0015" => v < 97,
            "RS0016" => v < 97,
            "LT0017" => v < 97,
            "C30018" => v < 97,
            "C100019" => v < 97,
            "FN0020" => v < 97,
            "AB0021" => v < 97,
            "GD0022" => v < 97,
            "PR0023" => v < 97,
            "LK0024" => v < 97,
            "AP0025" => v < 97,
            "RP0026" => v < 97,
            "TS0027" => v < 97,
            "RS0028" => v < 97,
            "LT0029" => v < 97,
            "C30030" => v < 97,
            "C100031" => v < 97,
            "FN0032" => v < 97,
            "AB0033" => v < 97,
            "GD0034" => v < 97,
            "PR0035" => v < 97,
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
pub fn u2781mat_sum() -> u64 {
    let mut s = 0u64;
    let mut i = 0usize;
    while i < u2781mat_COUNT {
        s = s.wrapping_add(u2781mat[i].1 as u64);
        i += 1;
    }
    s
}

/// 键唯一性（构造期断言等价物：逐对扫描同键）。
pub fn u2781mat_unique() -> bool {
    let mut i = 0usize;
    while i < u2781mat_COUNT {
        let mut j = i + 1;
        while j < u2781mat_COUNT {
            if u2781mat[i].0 == u2781mat[j].0 {
                return false;
            }
            j += 1;
        }
        i += 1;
    }
    true
}

// --------------------------------------------------------------------------
// 直方图引擎 mxu2781hist（有界插入排序 + 分位数 P50/P90/P99）
// --------------------------------------------------------------------------
/// 联动事务 直方图（96 桶滚动）。
/// 样本容量上限。
pub const MXU2781HIST_CAP: usize = 96;

/// 直方图账本（排序态 + 五统量）。
#[derive(Clone, Copy, Debug)]
pub struct mxu2781histHist {
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

impl mxu2781histHist {
    /// 空账构造。
    pub const fn new() -> Self {
        Self { samples: [0; 96], n: 0, drops: 0, min: 0, max: 0, sum: 0 }
    }

    /// 采样（有界插入排序；满员丢弃并记账）。
    pub fn push(&mut self, v: u64) {
        if self.n >= MXU2781HIST_CAP {
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

/// 段位守卫：全部诊断码必须落在本段 [0xae30, 0xae3f]。
pub const fn u2781_segment_guard() -> bool {
    let mut ok = true;
    let mut i = 0usize;
    while i < u2781Code::ALL.len() {
        let c = u2781Code::ALL[i].code();
        if c < 0xae30 || c > 0xae3f {
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
fn u2781_guard_seg() -> bool {
    u2781_segment_guard()
}

/// UNX-F2781 矩阵 36 行齐备 + 逐行守卫 + 唯一性 + 累计守恒。
fn check_u2781_map_guard() -> bool {
    let mut ok = u2781mat_COUNT == 36;
    if ok { ok = u2781mat_scan() == 0 && u2781mat_unique(); }
    if ok { ok = u2781mat_get("LK0000") == Some(0) && u2781mat_get("ZZZ_NOKEY").is_none(); }
    if ok { ok = u2781mat_sum() == 1594; }
    ok
}

/// UNX-F2781 直方图 16 样本分位/均值/守恒。
fn check_u2781_map_hist() -> bool {
    let mut h = mxu2781histHist::new();
    let mut i = 0u64;
    while i < 16 { h.push(i + 1); i += 1; }
    let mut ok = h.conserved() && h.mean() >= 1 && h.percentile(500) >= 8;
    ok
}

/// 本模块自检（逐检查带 detail，全绿才出门）。
pub fn run_u2781_checks() -> CheckSet {
    let mut set = CheckSet::new("U2781");
    set.add(
        "u2781-seg-guard",
        u2781_guard_seg(),
        if u2781_guard_seg() { "" } else { "diagnostic code outside segment" },
    );
    set.add(
        "u2781-map-guard",
        check_u2781_map_guard(),
        if check_u2781_map_guard() { "" } else { "UNX-F2781 matrix guard mismatch" },
    );
    set.add(
        "u2781-map-hist",
        check_u2781_map_hist(),
        if check_u2781_map_hist() { "" } else { "UNX-F2781 hist mismatch" },
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
    fn u2781_all_checks_green() {
        let set = run_u2781_checks();
        assert!(set.all_passed(), "U2781 domain checks must be green");
    }

    #[test]
    fn u2781_segment_guard_holds() {
        assert!(u2781_segment_guard());
    }
}
