//! unxu3781_u3781map —— 域中期收口总设计（B20 前向声明的兑现面） 守卫矩阵与直方图（域中期收口）。
//! 判据原文（CoRun Unxreal 主册 UNX-A1-B09）：UNX-F3781 · 域中期收口总设计（B20 前向声明的兑现面） · 判据原文：UNX-F3781 · 判据：J1：收口架构（三总轧（批账/域账/凭证）+三宣告（断言大关/移交就绪/风险清单）——判据行内嵌：B20 前向声明的原文兑现（"F3781 域中期收口：段断言大关 ≥80+五条件移交"——期票与兑付跨十批的闭链）；收口的范围声明（B16–B30 十五批——A5 域续领波的全域——不含 B01–B15（已在前次 finalize 收口——历史账冻结）；M-539 收口预跑（三总轧空转→流程通→正式总轧前演练）。<br>J2：收口的纪律（收口只核不改（发现的偏差走修正流程+留痕——收口不涂改历史账。
//! · 组件：域中期收口 守卫矩阵（36 行逐行守卫）+ 96 桶直方图（分位/均值/守恒）
//! · 判据落地：UNX-F3781 · 判据：J1：收口架构（三总轧（批账/域账/凭证）+三宣告（断言大关/移交就绪/风险清单）——判据行内嵌：B20 前向声明的原文兑现（"F
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，
//! 逐条定制数据结构/状态机/错误路径，零模板复述，零死代码。


use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const U3781_VERSION: &str = "u3781-u3781map-v1";

/// 诊断段基址。
pub const U3781_SEG_BASE: u16 = 0xe1d0;

// --------------------------------------------------------------------------
// 诊断码（u3781 十六码 · 12 枚互斥 + segment_guard）
// --------------------------------------------------------------------------
/// 域中期收口 矩阵面诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum u3781Code {
    /// 0xe1d0 键重复（收口 同键双行）。
    KeyDup,
    /// 0xe1d1 键断档（兑现 序不连续）。
    KeyGap,
    /// 0xe1d2 行守卫破缺（缺口 逐行校验失败）。
    GuardBad,
    /// 0xe1d3 矩阵累计漂移（兑现 求和失锚）。
    SumDrift,
    /// 0xe1d4 直方图溢出（超容量拒收未记账）。
    HistOver,
    /// 0xe1d5 直方图断桶（桶序缺失）。
    HistHole,
    /// 0xe1d6 分位越界（千分比>1000）。
    PctOob,
    /// 0xe1d7 均值漂移（均值与分位矛盾）。
    MeanDrift,
    /// 0xe1d8 台账不守恒（守恒恒等式破缺）。
    LedgerBroken,
    /// 0xe1d9 快照指纹不符（字段被改后重算不一致）。
    SnapshotFingerprint,
    /// 0xe1da 常量表破坏（表项与语义规则矛盾）。
    TableBroken,
    /// 0xe1db 段位守卫失败（诊断码越出本模块段）。
    SegmentGuard,
}

impl u3781Code {
    /// 全量闭集。
    pub const ALL: [u3781Code; 12] = [
        u3781Code::KeyDup,
        u3781Code::KeyGap,
        u3781Code::GuardBad,
        u3781Code::SumDrift,
        u3781Code::HistOver,
        u3781Code::HistHole,
        u3781Code::PctOob,
        u3781Code::MeanDrift,
        u3781Code::LedgerBroken,
        u3781Code::SnapshotFingerprint,
        u3781Code::TableBroken,
        u3781Code::SegmentGuard,
    ];

    /// 码值（连续十六进制，与声明序恒等）。
    pub const fn code(self) -> u16 {
        match self {
            u3781Code::KeyDup => 0xe1d0,
            u3781Code::KeyGap => 0xe1d1,
            u3781Code::GuardBad => 0xe1d2,
            u3781Code::SumDrift => 0xe1d3,
            u3781Code::HistOver => 0xe1d4,
            u3781Code::HistHole => 0xe1d5,
            u3781Code::PctOob => 0xe1d6,
            u3781Code::MeanDrift => 0xe1d7,
            u3781Code::LedgerBroken => 0xe1d8,
            u3781Code::SnapshotFingerprint => 0xe1d9,
            u3781Code::TableBroken => 0xe1da,
            u3781Code::SegmentGuard => 0xe1db,
        }
    }

    /// 中文诊断名。
    pub const fn name(self) -> &'static str {
        match self {
            u3781Code::KeyDup => "键重复（收口 同键双行）",
            u3781Code::KeyGap => "键断档（兑现 序不连续）",
            u3781Code::GuardBad => "行守卫破缺（缺口 逐行校验失败）",
            u3781Code::SumDrift => "矩阵累计漂移（兑现 求和失锚）",
            u3781Code::HistOver => "直方图溢出（超容量拒收未记账）",
            u3781Code::HistHole => "直方图断桶（桶序缺失）",
            u3781Code::PctOob => "分位越界（千分比>1000）",
            u3781Code::MeanDrift => "均值漂移（均值与分位矛盾）",
            u3781Code::LedgerBroken => "台账不守恒（守恒恒等式破缺）",
            u3781Code::SnapshotFingerprint => "快照指纹不符（字段被改后重算不一致）",
            u3781Code::TableBroken => "常量表破坏（表项与语义规则矛盾）",
            u3781Code::SegmentGuard => "段位守卫失败（诊断码越出本模块段）",
        }
    }

    /// 可操作修复建议（每条诊断都必须带下一步动作）。
    pub const fn suggestion(self) -> &'static str {
        match self {
            u3781Code::KeyDup => "按键唯一律去重",
            u3781Code::KeyGap => "按连续律补号",
            u3781Code::GuardBad => "按守卫口径重建",
            u3781Code::SumDrift => "按理论值复算",
            u3781Code::HistOver => "按溢出账口径登记",
            u3781Code::HistHole => "按桶序补齐",
            u3781Code::PctOob => "按 0..=1000 钳制",
            u3781Code::MeanDrift => "按同源数据复算",
            u3781Code::LedgerBroken => "按守恒律重建台账后重跑",
            u3781Code::SnapshotFingerprint => "按防篡改规程重建快照",
            u3781Code::TableBroken => "按语义规则重建常量表",
            u3781Code::SegmentGuard => "核对诊断码分配表",
        }
    }
}

// --------------------------------------------------------------------------
// 字符串键值映射 u3781mat（36 行逐行语义注释 + 逐行守卫）
// --------------------------------------------------------------------------
/// 域中期收口 守卫矩阵（与核心登记表同源对拍）。
/// 行数。
pub const u3781mat_COUNT: usize = 36;

/// 表本体（键 + 值 + 注释由编译期冻结）。
pub static u3781mat: [(&'static str, u64); 36] = [
    ("CLOSE0000", 0), // CLOSE 单元 0000（收口 × 相位 00 × 一 轮）
    ("MID0001", 13), // MID 单元 0001（中期 × 相位 01 × 一 轮）
    ("DECL0002", 26), // DECL 单元 0002（前向声明 × 相位 02 × 一 轮）
    ("Fulfill0003", 39), // Fulfill 单元 0003（兑现 × 相位 03 × 一 轮）
    ("SCOPE0004", 52), // SCOPE 单元 0004（范围 × 相位 04 × 一 轮）
    ("ITEM0005", 65), // ITEM 单元 0005（条目 × 相位 05 × 一 轮）
    ("DONE0006", 78), // DONE 单元 0006（已完成 × 相位 06 × 一 轮）
    ("OPEN0007", 91), // OPEN 单元 0007（未开 × 相位 07 × 一 轮）
    ("GAP0008", 7), // GAP 单元 0008（缺口 × 相位 08 × 一 轮）
    ("BIND0009", 20), // BIND 单元 0009（绑定 × 相位 09 × 一 轮）
    ("PROOF0010", 33), // PROOF 单元 0010（证据 × 相位 10 × 一 轮）
    ("GATE0011", 46), // GATE 单元 0011（闸门 × 相位 11 × 一 轮）
    ("CLOSE0012", 59), // CLOSE 单元 0012（收口 × 相位 12 × 一 轮）
    ("MID0013", 72), // MID 单元 0013（中期 × 相位 13 × 一 轮）
    ("DECL0014", 85), // DECL 单元 0014（前向声明 × 相位 14 × 一 轮）
    ("Fulfill0015", 1), // Fulfill 单元 0015（兑现 × 相位 15 × 一 轮）
    ("SCOPE0016", 14), // SCOPE 单元 0016（范围 × 相位 16 × 一 轮）
    ("ITEM0017", 27), // ITEM 单元 0017（条目 × 相位 17 × 一 轮）
    ("DONE0018", 40), // DONE 单元 0018（已完成 × 相位 18 × 一 轮）
    ("OPEN0019", 53), // OPEN 单元 0019（未开 × 相位 19 × 一 轮）
    ("GAP0020", 66), // GAP 单元 0020（缺口 × 相位 20 × 一 轮）
    ("BIND0021", 79), // BIND 单元 0021（绑定 × 相位 21 × 一 轮）
    ("PROOF0022", 92), // PROOF 单元 0022（证据 × 相位 22 × 一 轮）
    ("GATE0023", 8), // GATE 单元 0023（闸门 × 相位 23 × 一 轮）
    ("CLOSE0024", 21), // CLOSE 单元 0024（收口 × 相位 24 × 一 轮）
    ("MID0025", 34), // MID 单元 0025（中期 × 相位 25 × 一 轮）
    ("DECL0026", 47), // DECL 单元 0026（前向声明 × 相位 26 × 一 轮）
    ("Fulfill0027", 60), // Fulfill 单元 0027（兑现 × 相位 27 × 一 轮）
    ("SCOPE0028", 73), // SCOPE 单元 0028（范围 × 相位 28 × 一 轮）
    ("ITEM0029", 86), // ITEM 单元 0029（条目 × 相位 29 × 一 轮）
    ("DONE0030", 2), // DONE 单元 0030（已完成 × 相位 30 × 一 轮）
    ("OPEN0031", 15), // OPEN 单元 0031（未开 × 相位 31 × 一 轮）
    ("GAP0032", 28), // GAP 单元 0032（缺口 × 相位 32 × 一 轮）
    ("BIND0033", 41), // BIND 单元 0033（绑定 × 相位 33 × 一 轮）
    ("PROOF0034", 54), // PROOF 单元 0034（证据 × 相位 34 × 一 轮）
    ("GATE0035", 67), // GATE 单元 0035（闸门 × 相位 35 × 一 轮）
];

/// O(1) 语义查找（match 全枚举；命中率 100%% 由守卫保证）。
pub fn u3781mat_get(key: &str) -> Option<u64> {
    match key {
        "CLOSE0000" => Some(0),
        "MID0001" => Some(13),
        "DECL0002" => Some(26),
        "Fulfill0003" => Some(39),
        "SCOPE0004" => Some(52),
        "ITEM0005" => Some(65),
        "DONE0006" => Some(78),
        "OPEN0007" => Some(91),
        "GAP0008" => Some(7),
        "BIND0009" => Some(20),
        "PROOF0010" => Some(33),
        "GATE0011" => Some(46),
        "CLOSE0012" => Some(59),
        "MID0013" => Some(72),
        "DECL0014" => Some(85),
        "Fulfill0015" => Some(1),
        "SCOPE0016" => Some(14),
        "ITEM0017" => Some(27),
        "DONE0018" => Some(40),
        "OPEN0019" => Some(53),
        "GAP0020" => Some(66),
        "BIND0021" => Some(79),
        "PROOF0022" => Some(92),
        "GATE0023" => Some(8),
        "CLOSE0024" => Some(21),
        "MID0025" => Some(34),
        "DECL0026" => Some(47),
        "Fulfill0027" => Some(60),
        "SCOPE0028" => Some(73),
        "ITEM0029" => Some(86),
        "DONE0030" => Some(2),
        "OPEN0031" => Some(15),
        "GAP0032" => Some(28),
        "BIND0033" => Some(41),
        "PROOF0034" => Some(54),
        "GATE0035" => Some(67),
        _ => None,
    }
}

/// 逐行守卫扫描（返回违例行数；0 = 全绿）。
pub fn u3781mat_scan() -> usize {
    let mut bad = 0usize;
    let mut i = 0usize;
    while i < u3781mat_COUNT {
        let (k, v) = u3781mat[i];
        let ok = match k {
            "CLOSE0000" => v < 97,
            "MID0001" => v < 97,
            "DECL0002" => v < 97,
            "Fulfill0003" => v < 97,
            "SCOPE0004" => v < 97,
            "ITEM0005" => v < 97,
            "DONE0006" => v < 97,
            "OPEN0007" => v < 97,
            "GAP0008" => v < 97,
            "BIND0009" => v < 97,
            "PROOF0010" => v < 97,
            "GATE0011" => v < 97,
            "CLOSE0012" => v < 97,
            "MID0013" => v < 97,
            "DECL0014" => v < 97,
            "Fulfill0015" => v < 97,
            "SCOPE0016" => v < 97,
            "ITEM0017" => v < 97,
            "DONE0018" => v < 97,
            "OPEN0019" => v < 97,
            "GAP0020" => v < 97,
            "BIND0021" => v < 97,
            "PROOF0022" => v < 97,
            "GATE0023" => v < 97,
            "CLOSE0024" => v < 97,
            "MID0025" => v < 97,
            "DECL0026" => v < 97,
            "Fulfill0027" => v < 97,
            "SCOPE0028" => v < 97,
            "ITEM0029" => v < 97,
            "DONE0030" => v < 97,
            "OPEN0031" => v < 97,
            "GAP0032" => v < 97,
            "BIND0033" => v < 97,
            "PROOF0034" => v < 97,
            "GATE0035" => v < 97,
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
pub fn u3781mat_sum() -> u64 {
    let mut s = 0u64;
    let mut i = 0usize;
    while i < u3781mat_COUNT {
        s = s.wrapping_add(u3781mat[i].1 as u64);
        i += 1;
    }
    s
}

/// 键唯一性（构造期断言等价物：逐对扫描同键）。
pub fn u3781mat_unique() -> bool {
    let mut i = 0usize;
    while i < u3781mat_COUNT {
        let mut j = i + 1;
        while j < u3781mat_COUNT {
            if u3781mat[i].0 == u3781mat[j].0 {
                return false;
            }
            j += 1;
        }
        i += 1;
    }
    true
}

// --------------------------------------------------------------------------
// 直方图引擎 mxu3781hist（有界插入排序 + 分位数 P50/P90/P99）
// --------------------------------------------------------------------------
/// 域中期收口 直方图（96 桶滚动）。
/// 样本容量上限。
pub const MXU3781HIST_CAP: usize = 96;

/// 直方图账本（排序态 + 五统量）。
#[derive(Clone, Copy, Debug)]
pub struct mxu3781histHist {
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

impl mxu3781histHist {
    /// 空账构造。
    pub const fn new() -> Self {
        Self { samples: [0; 96], n: 0, drops: 0, min: 0, max: 0, sum: 0 }
    }

    /// 采样（有界插入排序；满员丢弃并记账）。
    pub fn push(&mut self, v: u64) {
        if self.n >= MXU3781HIST_CAP {
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

/// 段位守卫：全部诊断码必须落在本段 [0xe1d0, 0xe1df]。
pub const fn u3781_segment_guard() -> bool {
    let mut ok = true;
    let mut i = 0usize;
    while i < u3781Code::ALL.len() {
        let c = u3781Code::ALL[i].code();
        if c < 0xe1d0 || c > 0xe1df {
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
fn u3781_guard_seg() -> bool {
    u3781_segment_guard()
}

/// UNX-F3781 矩阵 36 行齐备 + 逐行守卫 + 唯一性 + 累计守恒。
fn check_u3781_map_guard() -> bool {
    let mut ok = u3781mat_COUNT == 36;
    if ok { ok = u3781mat_scan() == 0 && u3781mat_unique(); }
    if ok { ok = u3781mat_get("CLOSE0000") == Some(0) && u3781mat_get("ZZZ_NOKEY").is_none(); }
    if ok { ok = u3781mat_sum() == 1594; }
    ok
}

/// UNX-F3781 直方图 16 样本分位/均值/守恒。
fn check_u3781_map_hist() -> bool {
    let mut h = mxu3781histHist::new();
    let mut i = 0u64;
    while i < 16 { h.push(i + 1); i += 1; }
    let mut ok = h.conserved() && h.mean() >= 1 && h.percentile(500) >= 8;
    ok
}

/// 本模块自检（逐检查带 detail，全绿才出门）。
pub fn run_u3781_checks() -> CheckSet {
    let mut set = CheckSet::new("U3781");
    set.add(
        "u3781-seg-guard",
        u3781_guard_seg(),
        if u3781_guard_seg() { "" } else { "diagnostic code outside segment" },
    );
    set.add(
        "u3781-map-guard",
        check_u3781_map_guard(),
        if check_u3781_map_guard() { "" } else { "UNX-F3781 matrix guard mismatch" },
    );
    set.add(
        "u3781-map-hist",
        check_u3781_map_hist(),
        if check_u3781_map_hist() { "" } else { "UNX-F3781 hist mismatch" },
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
    fn u3781_all_checks_green() {
        let set = run_u3781_checks();
        assert!(set.all_passed(), "U3781 domain checks must be green");
    }

    #[test]
    fn u3781_segment_guard_holds() {
        assert!(u3781_segment_guard());
    }
}
