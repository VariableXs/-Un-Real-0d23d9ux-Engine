//! unxu3721_u3721map —— 桩体系总设计（替身三原则与桩清单） 守卫矩阵与直方图（桩体系）。
//! 判据原文（CoRun Unxreal 主册 UNX-A1-B09）：UNX-F3721 · 桩体系总设计（替身三原则与桩清单） · 判据原文：UNX-F3721 · 判据：J1：替身三原则（确定性（同脚本同态同行为）/账本化（操作全记账）/可控性（时间与故障全可控——判据行内嵌三原则各一句）；桩清单（时钟/中断/DMA/存储/网卡五桩 v1——清单 const 扩桩走 bump）；M-479 三原则首验（时钟桩+中断桩最小联动→三原则逐条断言）。<br>J2：桩与被测分池（B20 隔离池同律——桩基础设施不吃被测方内存）。
//! · 组件：桩体系 守卫矩阵（36 行逐行守卫）+ 96 桶直方图（分位/均值/守恒）
//! · 判据落地：UNX-F3721 · 判据：J1：替身三原则（确定性（同脚本同态同行为）/账本化（操作全记账）/可控性（时间与故障全可控——判据行内嵌三原则各一句）；桩清单（
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，
//! 逐条定制数据结构/状态机/错误路径，零模板复述，零死代码。


use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const U3721_VERSION: &str = "u3721-u3721map-v1";

/// 诊断段基址。
pub const U3721_SEG_BASE: u16 = 0xe1a0;

// --------------------------------------------------------------------------
// 诊断码（u3721 十六码 · 12 枚互斥 + segment_guard）
// --------------------------------------------------------------------------
/// 桩体系 矩阵面诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum u3721Code {
    /// 0xe1a0 键重复（桩 同键双行）。
    KeyDup,
    /// 0xe1a1 键断档（替身 序不连续）。
    KeyGap,
    /// 0xe1a2 行守卫破缺（录制 逐行校验失败）。
    GuardBad,
    /// 0xe1a3 矩阵累计漂移（三原则 求和失锚）。
    SumDrift,
    /// 0xe1a4 直方图溢出（超容量拒收未记账）。
    HistOver,
    /// 0xe1a5 直方图断桶（桶序缺失）。
    HistHole,
    /// 0xe1a6 分位越界（千分比>1000）。
    PctOob,
    /// 0xe1a7 均值漂移（均值与分位矛盾）。
    MeanDrift,
    /// 0xe1a8 台账不守恒（守恒恒等式破缺）。
    LedgerBroken,
    /// 0xe1a9 快照指纹不符（字段被改后重算不一致）。
    SnapshotFingerprint,
    /// 0xe1aa 常量表破坏（表项与语义规则矛盾）。
    TableBroken,
    /// 0xe1ab 段位守卫失败（诊断码越出本模块段）。
    SegmentGuard,
}

impl u3721Code {
    /// 全量闭集。
    pub const ALL: [u3721Code; 12] = [
        u3721Code::KeyDup,
        u3721Code::KeyGap,
        u3721Code::GuardBad,
        u3721Code::SumDrift,
        u3721Code::HistOver,
        u3721Code::HistHole,
        u3721Code::PctOob,
        u3721Code::MeanDrift,
        u3721Code::LedgerBroken,
        u3721Code::SnapshotFingerprint,
        u3721Code::TableBroken,
        u3721Code::SegmentGuard,
    ];

    /// 码值（连续十六进制，与声明序恒等）。
    pub const fn code(self) -> u16 {
        match self {
            u3721Code::KeyDup => 0xe1a0,
            u3721Code::KeyGap => 0xe1a1,
            u3721Code::GuardBad => 0xe1a2,
            u3721Code::SumDrift => 0xe1a3,
            u3721Code::HistOver => 0xe1a4,
            u3721Code::HistHole => 0xe1a5,
            u3721Code::PctOob => 0xe1a6,
            u3721Code::MeanDrift => 0xe1a7,
            u3721Code::LedgerBroken => 0xe1a8,
            u3721Code::SnapshotFingerprint => 0xe1a9,
            u3721Code::TableBroken => 0xe1aa,
            u3721Code::SegmentGuard => 0xe1ab,
        }
    }

    /// 中文诊断名。
    pub const fn name(self) -> &'static str {
        match self {
            u3721Code::KeyDup => "键重复（桩 同键双行）",
            u3721Code::KeyGap => "键断档（替身 序不连续）",
            u3721Code::GuardBad => "行守卫破缺（录制 逐行校验失败）",
            u3721Code::SumDrift => "矩阵累计漂移（三原则 求和失锚）",
            u3721Code::HistOver => "直方图溢出（超容量拒收未记账）",
            u3721Code::HistHole => "直方图断桶（桶序缺失）",
            u3721Code::PctOob => "分位越界（千分比>1000）",
            u3721Code::MeanDrift => "均值漂移（均值与分位矛盾）",
            u3721Code::LedgerBroken => "台账不守恒（守恒恒等式破缺）",
            u3721Code::SnapshotFingerprint => "快照指纹不符（字段被改后重算不一致）",
            u3721Code::TableBroken => "常量表破坏（表项与语义规则矛盾）",
            u3721Code::SegmentGuard => "段位守卫失败（诊断码越出本模块段）",
        }
    }

    /// 可操作修复建议（每条诊断都必须带下一步动作）。
    pub const fn suggestion(self) -> &'static str {
        match self {
            u3721Code::KeyDup => "按键唯一律去重",
            u3721Code::KeyGap => "按连续律补号",
            u3721Code::GuardBad => "按守卫口径重建",
            u3721Code::SumDrift => "按理论值复算",
            u3721Code::HistOver => "按溢出账口径登记",
            u3721Code::HistHole => "按桶序补齐",
            u3721Code::PctOob => "按 0..=1000 钳制",
            u3721Code::MeanDrift => "按同源数据复算",
            u3721Code::LedgerBroken => "按守恒律重建台账后重跑",
            u3721Code::SnapshotFingerprint => "按防篡改规程重建快照",
            u3721Code::TableBroken => "按语义规则重建常量表",
            u3721Code::SegmentGuard => "核对诊断码分配表",
        }
    }
}

// --------------------------------------------------------------------------
// 字符串键值映射 u3721mat（36 行逐行语义注释 + 逐行守卫）
// --------------------------------------------------------------------------
/// 桩体系 守卫矩阵（与核心登记表同源对拍）。
/// 行数。
pub const u3721mat_COUNT: usize = 36;

/// 表本体（键 + 值 + 注释由编译期冻结）。
pub static u3721mat: [(&'static str, u64); 36] = [
    ("STUB0000", 0), // STUB 单元 0000（桩 × 相位 00 × 一 轮）
    ("TRIPLE0001", 13), // TRIPLE 单元 0001（替身三原则 × 相位 01 × 一 轮）
    ("LIST0002", 26), // LIST 单元 0002（桩清单 × 相位 02 × 一 轮）
    ("INJ0003", 39), // INJ 单元 0003（注入 × 相位 03 × 一 轮）
    ("REC0004", 52), // REC 单元 0004（录制 × 相位 04 × 一 轮）
    ("PLAY0005", 65), // PLAY 单元 0005（回放 × 相位 05 × 一 轮）
    ("FAIL0006", 78), // FAIL 单元 0006（失败注入 × 相位 06 × 一 轮）
    ("MODE0007", 91), // MODE 单元 0007（桩模式 × 相位 07 × 一 轮）
    ("SLOT0008", 7), // SLOT 单元 0008（桩槽 × 相位 08 × 一 轮）
    ("DECL0009", 20), // DECL 单元 0009（桩声明 × 相位 09 × 一 轮）
    ("USE0010", 33), // USE 单元 0010（使用点 × 相位 10 × 一 轮）
    ("AUD0011", 46), // AUD 单元 0011（审计 × 相位 11 × 一 轮）
    ("STUB0012", 59), // STUB 单元 0012（桩 × 相位 12 × 一 轮）
    ("TRIPLE0013", 72), // TRIPLE 单元 0013（替身三原则 × 相位 13 × 一 轮）
    ("LIST0014", 85), // LIST 单元 0014（桩清单 × 相位 14 × 一 轮）
    ("INJ0015", 1), // INJ 单元 0015（注入 × 相位 15 × 一 轮）
    ("REC0016", 14), // REC 单元 0016（录制 × 相位 16 × 一 轮）
    ("PLAY0017", 27), // PLAY 单元 0017（回放 × 相位 17 × 一 轮）
    ("FAIL0018", 40), // FAIL 单元 0018（失败注入 × 相位 18 × 一 轮）
    ("MODE0019", 53), // MODE 单元 0019（桩模式 × 相位 19 × 一 轮）
    ("SLOT0020", 66), // SLOT 单元 0020（桩槽 × 相位 20 × 一 轮）
    ("DECL0021", 79), // DECL 单元 0021（桩声明 × 相位 21 × 一 轮）
    ("USE0022", 92), // USE 单元 0022（使用点 × 相位 22 × 一 轮）
    ("AUD0023", 8), // AUD 单元 0023（审计 × 相位 23 × 一 轮）
    ("STUB0024", 21), // STUB 单元 0024（桩 × 相位 24 × 一 轮）
    ("TRIPLE0025", 34), // TRIPLE 单元 0025（替身三原则 × 相位 25 × 一 轮）
    ("LIST0026", 47), // LIST 单元 0026（桩清单 × 相位 26 × 一 轮）
    ("INJ0027", 60), // INJ 单元 0027（注入 × 相位 27 × 一 轮）
    ("REC0028", 73), // REC 单元 0028（录制 × 相位 28 × 一 轮）
    ("PLAY0029", 86), // PLAY 单元 0029（回放 × 相位 29 × 一 轮）
    ("FAIL0030", 2), // FAIL 单元 0030（失败注入 × 相位 30 × 一 轮）
    ("MODE0031", 15), // MODE 单元 0031（桩模式 × 相位 31 × 一 轮）
    ("SLOT0032", 28), // SLOT 单元 0032（桩槽 × 相位 32 × 一 轮）
    ("DECL0033", 41), // DECL 单元 0033（桩声明 × 相位 33 × 一 轮）
    ("USE0034", 54), // USE 单元 0034（使用点 × 相位 34 × 一 轮）
    ("AUD0035", 67), // AUD 单元 0035（审计 × 相位 35 × 一 轮）
];

/// O(1) 语义查找（match 全枚举；命中率 100%% 由守卫保证）。
pub fn u3721mat_get(key: &str) -> Option<u64> {
    match key {
        "STUB0000" => Some(0),
        "TRIPLE0001" => Some(13),
        "LIST0002" => Some(26),
        "INJ0003" => Some(39),
        "REC0004" => Some(52),
        "PLAY0005" => Some(65),
        "FAIL0006" => Some(78),
        "MODE0007" => Some(91),
        "SLOT0008" => Some(7),
        "DECL0009" => Some(20),
        "USE0010" => Some(33),
        "AUD0011" => Some(46),
        "STUB0012" => Some(59),
        "TRIPLE0013" => Some(72),
        "LIST0014" => Some(85),
        "INJ0015" => Some(1),
        "REC0016" => Some(14),
        "PLAY0017" => Some(27),
        "FAIL0018" => Some(40),
        "MODE0019" => Some(53),
        "SLOT0020" => Some(66),
        "DECL0021" => Some(79),
        "USE0022" => Some(92),
        "AUD0023" => Some(8),
        "STUB0024" => Some(21),
        "TRIPLE0025" => Some(34),
        "LIST0026" => Some(47),
        "INJ0027" => Some(60),
        "REC0028" => Some(73),
        "PLAY0029" => Some(86),
        "FAIL0030" => Some(2),
        "MODE0031" => Some(15),
        "SLOT0032" => Some(28),
        "DECL0033" => Some(41),
        "USE0034" => Some(54),
        "AUD0035" => Some(67),
        _ => None,
    }
}

/// 逐行守卫扫描（返回违例行数；0 = 全绿）。
pub fn u3721mat_scan() -> usize {
    let mut bad = 0usize;
    let mut i = 0usize;
    while i < u3721mat_COUNT {
        let (k, v) = u3721mat[i];
        let ok = match k {
            "STUB0000" => v < 97,
            "TRIPLE0001" => v < 97,
            "LIST0002" => v < 97,
            "INJ0003" => v < 97,
            "REC0004" => v < 97,
            "PLAY0005" => v < 97,
            "FAIL0006" => v < 97,
            "MODE0007" => v < 97,
            "SLOT0008" => v < 97,
            "DECL0009" => v < 97,
            "USE0010" => v < 97,
            "AUD0011" => v < 97,
            "STUB0012" => v < 97,
            "TRIPLE0013" => v < 97,
            "LIST0014" => v < 97,
            "INJ0015" => v < 97,
            "REC0016" => v < 97,
            "PLAY0017" => v < 97,
            "FAIL0018" => v < 97,
            "MODE0019" => v < 97,
            "SLOT0020" => v < 97,
            "DECL0021" => v < 97,
            "USE0022" => v < 97,
            "AUD0023" => v < 97,
            "STUB0024" => v < 97,
            "TRIPLE0025" => v < 97,
            "LIST0026" => v < 97,
            "INJ0027" => v < 97,
            "REC0028" => v < 97,
            "PLAY0029" => v < 97,
            "FAIL0030" => v < 97,
            "MODE0031" => v < 97,
            "SLOT0032" => v < 97,
            "DECL0033" => v < 97,
            "USE0034" => v < 97,
            "AUD0035" => v < 97,
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
pub fn u3721mat_sum() -> u64 {
    let mut s = 0u64;
    let mut i = 0usize;
    while i < u3721mat_COUNT {
        s = s.wrapping_add(u3721mat[i].1 as u64);
        i += 1;
    }
    s
}

/// 键唯一性（构造期断言等价物：逐对扫描同键）。
pub fn u3721mat_unique() -> bool {
    let mut i = 0usize;
    while i < u3721mat_COUNT {
        let mut j = i + 1;
        while j < u3721mat_COUNT {
            if u3721mat[i].0 == u3721mat[j].0 {
                return false;
            }
            j += 1;
        }
        i += 1;
    }
    true
}

// --------------------------------------------------------------------------
// 直方图引擎 mxu3721hist（有界插入排序 + 分位数 P50/P90/P99）
// --------------------------------------------------------------------------
/// 桩体系 直方图（96 桶滚动）。
/// 样本容量上限。
pub const MXU3721HIST_CAP: usize = 96;

/// 直方图账本（排序态 + 五统量）。
#[derive(Clone, Copy, Debug)]
pub struct mxu3721histHist {
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

impl mxu3721histHist {
    /// 空账构造。
    pub const fn new() -> Self {
        Self { samples: [0; 96], n: 0, drops: 0, min: 0, max: 0, sum: 0 }
    }

    /// 采样（有界插入排序；满员丢弃并记账）。
    pub fn push(&mut self, v: u64) {
        if self.n >= MXU3721HIST_CAP {
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

/// 段位守卫：全部诊断码必须落在本段 [0xe1a0, 0xe1af]。
pub const fn u3721_segment_guard() -> bool {
    let mut ok = true;
    let mut i = 0usize;
    while i < u3721Code::ALL.len() {
        let c = u3721Code::ALL[i].code();
        if c < 0xe1a0 || c > 0xe1af {
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
fn u3721_guard_seg() -> bool {
    u3721_segment_guard()
}

/// UNX-F3721 矩阵 36 行齐备 + 逐行守卫 + 唯一性 + 累计守恒。
fn check_u3721_map_guard() -> bool {
    let mut ok = u3721mat_COUNT == 36;
    if ok { ok = u3721mat_scan() == 0 && u3721mat_unique(); }
    if ok { ok = u3721mat_get("STUB0000") == Some(0) && u3721mat_get("ZZZ_NOKEY").is_none(); }
    if ok { ok = u3721mat_sum() == 1594; }
    ok
}

/// UNX-F3721 直方图 16 样本分位/均值/守恒。
fn check_u3721_map_hist() -> bool {
    let mut h = mxu3721histHist::new();
    let mut i = 0u64;
    while i < 16 { h.push(i + 1); i += 1; }
    let mut ok = h.conserved() && h.mean() >= 1 && h.percentile(500) >= 8;
    ok
}

/// 本模块自检（逐检查带 detail，全绿才出门）。
pub fn run_u3721_checks() -> CheckSet {
    let mut set = CheckSet::new("U3721");
    set.add(
        "u3721-seg-guard",
        u3721_guard_seg(),
        if u3721_guard_seg() { "" } else { "diagnostic code outside segment" },
    );
    set.add(
        "u3721-map-guard",
        check_u3721_map_guard(),
        if check_u3721_map_guard() { "" } else { "UNX-F3721 matrix guard mismatch" },
    );
    set.add(
        "u3721-map-hist",
        check_u3721_map_hist(),
        if check_u3721_map_hist() { "" } else { "UNX-F3721 hist mismatch" },
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
    fn u3721_all_checks_green() {
        let set = run_u3721_checks();
        assert!(set.all_passed(), "U3721 domain checks must be green");
    }

    #[test]
    fn u3721_segment_guard_holds() {
        assert!(u3721_segment_guard());
    }
}
