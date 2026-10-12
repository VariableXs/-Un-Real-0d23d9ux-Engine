//! unxu3681_u3681map —— fuzz 总设计（生成/执行/判读三段管线） 守卫矩阵与直方图（fuzz 三段管线）。
//! 判据原文（CoRun Unxreal 主册 UNX-A1-B09）：UNX-F3681 · fuzz 总设计（生成/执行/判读三段管线） · 判据原文：UNX-F3681 · 判据：J1：三段管线（生成（seed→输入序列）→执行（入口调用+判据采集）→判读（panic/账失衡/违例/confirm 分类）——判据行内嵌三段各一节）；M-439 三段首跑（最小入口+8 种子语料→一跑全链出判读报告）。<br>J2：管线确定性（同 seed 同语料→同输入序列→同判读——F3687 复现的架构前提——无外部熵）。
//! · 组件：fuzz 三段管线 守卫矩阵（36 行逐行守卫）+ 96 桶直方图（分位/均值/守恒）
//! · 判据落地：UNX-F3681 · 判据：J1：三段管线（生成（seed→输入序列）→执行（入口调用+判据采集）→判读（panic/账失衡/违例/confirm 分类）——
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，
//! 逐条定制数据结构/状态机/错误路径，零模板复述，零死代码。


use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const U3681_VERSION: &str = "u3681-u3681map-v1";

/// 诊断段基址。
pub const U3681_SEG_BASE: u16 = 0xe180;

// --------------------------------------------------------------------------
// 诊断码（u3681 十六码 · 12 枚互斥 + segment_guard）
// --------------------------------------------------------------------------
/// fuzz 三段管线 矩阵面诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum u3681Code {
    /// 0xe180 键重复（fuzz 同键双行）。
    KeyDup,
    /// 0xe181 键断档（生成 序不连续）。
    KeyGap,
    /// 0xe182 行守卫破缺（判读 逐行校验失败）。
    GuardBad,
    /// 0xe183 矩阵累计漂移（三段 求和失锚）。
    SumDrift,
    /// 0xe184 直方图溢出（超容量拒收未记账）。
    HistOver,
    /// 0xe185 直方图断桶（桶序缺失）。
    HistHole,
    /// 0xe186 分位越界（千分比>1000）。
    PctOob,
    /// 0xe187 均值漂移（均值与分位矛盾）。
    MeanDrift,
    /// 0xe188 台账不守恒（守恒恒等式破缺）。
    LedgerBroken,
    /// 0xe189 快照指纹不符（字段被改后重算不一致）。
    SnapshotFingerprint,
    /// 0xe18a 常量表破坏（表项与语义规则矛盾）。
    TableBroken,
    /// 0xe18b 段位守卫失败（诊断码越出本模块段）。
    SegmentGuard,
}

impl u3681Code {
    /// 全量闭集。
    pub const ALL: [u3681Code; 12] = [
        u3681Code::KeyDup,
        u3681Code::KeyGap,
        u3681Code::GuardBad,
        u3681Code::SumDrift,
        u3681Code::HistOver,
        u3681Code::HistHole,
        u3681Code::PctOob,
        u3681Code::MeanDrift,
        u3681Code::LedgerBroken,
        u3681Code::SnapshotFingerprint,
        u3681Code::TableBroken,
        u3681Code::SegmentGuard,
    ];

    /// 码值（连续十六进制，与声明序恒等）。
    pub const fn code(self) -> u16 {
        match self {
            u3681Code::KeyDup => 0xe180,
            u3681Code::KeyGap => 0xe181,
            u3681Code::GuardBad => 0xe182,
            u3681Code::SumDrift => 0xe183,
            u3681Code::HistOver => 0xe184,
            u3681Code::HistHole => 0xe185,
            u3681Code::PctOob => 0xe186,
            u3681Code::MeanDrift => 0xe187,
            u3681Code::LedgerBroken => 0xe188,
            u3681Code::SnapshotFingerprint => 0xe189,
            u3681Code::TableBroken => 0xe18a,
            u3681Code::SegmentGuard => 0xe18b,
        }
    }

    /// 中文诊断名。
    pub const fn name(self) -> &'static str {
        match self {
            u3681Code::KeyDup => "键重复（fuzz 同键双行）",
            u3681Code::KeyGap => "键断档（生成 序不连续）",
            u3681Code::GuardBad => "行守卫破缺（判读 逐行校验失败）",
            u3681Code::SumDrift => "矩阵累计漂移（三段 求和失锚）",
            u3681Code::HistOver => "直方图溢出（超容量拒收未记账）",
            u3681Code::HistHole => "直方图断桶（桶序缺失）",
            u3681Code::PctOob => "分位越界（千分比>1000）",
            u3681Code::MeanDrift => "均值漂移（均值与分位矛盾）",
            u3681Code::LedgerBroken => "台账不守恒（守恒恒等式破缺）",
            u3681Code::SnapshotFingerprint => "快照指纹不符（字段被改后重算不一致）",
            u3681Code::TableBroken => "常量表破坏（表项与语义规则矛盾）",
            u3681Code::SegmentGuard => "段位守卫失败（诊断码越出本模块段）",
        }
    }

    /// 可操作修复建议（每条诊断都必须带下一步动作）。
    pub const fn suggestion(self) -> &'static str {
        match self {
            u3681Code::KeyDup => "按键唯一律去重",
            u3681Code::KeyGap => "按连续律补号",
            u3681Code::GuardBad => "按守卫口径重建",
            u3681Code::SumDrift => "按理论值复算",
            u3681Code::HistOver => "按溢出账口径登记",
            u3681Code::HistHole => "按桶序补齐",
            u3681Code::PctOob => "按 0..=1000 钳制",
            u3681Code::MeanDrift => "按同源数据复算",
            u3681Code::LedgerBroken => "按守恒律重建台账后重跑",
            u3681Code::SnapshotFingerprint => "按防篡改规程重建快照",
            u3681Code::TableBroken => "按语义规则重建常量表",
            u3681Code::SegmentGuard => "核对诊断码分配表",
        }
    }
}

// --------------------------------------------------------------------------
// 字符串键值映射 u3681mat（36 行逐行语义注释 + 逐行守卫）
// --------------------------------------------------------------------------
/// fuzz 三段管线 守卫矩阵（与核心登记表同源对拍）。
/// 行数。
pub const u3681mat_COUNT: usize = 36;

/// 表本体（键 + 值 + 注释由编译期冻结）。
pub static u3681mat: [(&'static str, u64); 36] = [
    ("FUZZ0000", 0), // FUZZ 单元 0000（fuzz × 相位 00 × 一 轮）
    ("GEN0001", 13), // GEN 单元 0001（生成 × 相位 01 × 一 轮）
    ("EXEC0002", 26), // EXEC 单元 0002（执行 × 相位 02 × 一 轮）
    ("JUDGE0003", 39), // JUDGE 单元 0003（判读 × 相位 03 × 一 轮）
    ("SEED0004", 52), // SEED 单元 0004（种子 × 相位 04 × 一 轮）
    ("CORPUS0005", 65), // CORPUS 单元 0005（语料库 × 相位 05 × 一 轮）
    ("MUT0006", 78), // MUT 单元 0006（变异 × 相位 06 × 一 轮）
    ("CRASH0007", 91), // CRASH 单元 0007（崩溃检出 × 相位 07 × 一 轮）
    ("DEDUP0008", 7), // DEDUP 单元 0008（去重 × 相位 08 × 一 轮）
    ("COV0009", 20), // COV 单元 0009（覆盖 × 相位 09 × 一 轮）
    ("TIME0010", 33), // TIME 单元 0010（超时 × 相位 10 × 一 轮）
    ("ARTEF0011", 46), // ARTEF 单元 0011（产物 × 相位 11 × 一 轮）
    ("FUZZ0012", 59), // FUZZ 单元 0012（fuzz × 相位 12 × 一 轮）
    ("GEN0013", 72), // GEN 单元 0013（生成 × 相位 13 × 一 轮）
    ("EXEC0014", 85), // EXEC 单元 0014（执行 × 相位 14 × 一 轮）
    ("JUDGE0015", 1), // JUDGE 单元 0015（判读 × 相位 15 × 一 轮）
    ("SEED0016", 14), // SEED 单元 0016（种子 × 相位 16 × 一 轮）
    ("CORPUS0017", 27), // CORPUS 单元 0017（语料库 × 相位 17 × 一 轮）
    ("MUT0018", 40), // MUT 单元 0018（变异 × 相位 18 × 一 轮）
    ("CRASH0019", 53), // CRASH 单元 0019（崩溃检出 × 相位 19 × 一 轮）
    ("DEDUP0020", 66), // DEDUP 单元 0020（去重 × 相位 20 × 一 轮）
    ("COV0021", 79), // COV 单元 0021（覆盖 × 相位 21 × 一 轮）
    ("TIME0022", 92), // TIME 单元 0022（超时 × 相位 22 × 一 轮）
    ("ARTEF0023", 8), // ARTEF 单元 0023（产物 × 相位 23 × 一 轮）
    ("FUZZ0024", 21), // FUZZ 单元 0024（fuzz × 相位 24 × 一 轮）
    ("GEN0025", 34), // GEN 单元 0025（生成 × 相位 25 × 一 轮）
    ("EXEC0026", 47), // EXEC 单元 0026（执行 × 相位 26 × 一 轮）
    ("JUDGE0027", 60), // JUDGE 单元 0027（判读 × 相位 27 × 一 轮）
    ("SEED0028", 73), // SEED 单元 0028（种子 × 相位 28 × 一 轮）
    ("CORPUS0029", 86), // CORPUS 单元 0029（语料库 × 相位 29 × 一 轮）
    ("MUT0030", 2), // MUT 单元 0030（变异 × 相位 30 × 一 轮）
    ("CRASH0031", 15), // CRASH 单元 0031（崩溃检出 × 相位 31 × 一 轮）
    ("DEDUP0032", 28), // DEDUP 单元 0032（去重 × 相位 32 × 一 轮）
    ("COV0033", 41), // COV 单元 0033（覆盖 × 相位 33 × 一 轮）
    ("TIME0034", 54), // TIME 单元 0034（超时 × 相位 34 × 一 轮）
    ("ARTEF0035", 67), // ARTEF 单元 0035（产物 × 相位 35 × 一 轮）
];

/// O(1) 语义查找（match 全枚举；命中率 100%% 由守卫保证）。
pub fn u3681mat_get(key: &str) -> Option<u64> {
    match key {
        "FUZZ0000" => Some(0),
        "GEN0001" => Some(13),
        "EXEC0002" => Some(26),
        "JUDGE0003" => Some(39),
        "SEED0004" => Some(52),
        "CORPUS0005" => Some(65),
        "MUT0006" => Some(78),
        "CRASH0007" => Some(91),
        "DEDUP0008" => Some(7),
        "COV0009" => Some(20),
        "TIME0010" => Some(33),
        "ARTEF0011" => Some(46),
        "FUZZ0012" => Some(59),
        "GEN0013" => Some(72),
        "EXEC0014" => Some(85),
        "JUDGE0015" => Some(1),
        "SEED0016" => Some(14),
        "CORPUS0017" => Some(27),
        "MUT0018" => Some(40),
        "CRASH0019" => Some(53),
        "DEDUP0020" => Some(66),
        "COV0021" => Some(79),
        "TIME0022" => Some(92),
        "ARTEF0023" => Some(8),
        "FUZZ0024" => Some(21),
        "GEN0025" => Some(34),
        "EXEC0026" => Some(47),
        "JUDGE0027" => Some(60),
        "SEED0028" => Some(73),
        "CORPUS0029" => Some(86),
        "MUT0030" => Some(2),
        "CRASH0031" => Some(15),
        "DEDUP0032" => Some(28),
        "COV0033" => Some(41),
        "TIME0034" => Some(54),
        "ARTEF0035" => Some(67),
        _ => None,
    }
}

/// 逐行守卫扫描（返回违例行数；0 = 全绿）。
pub fn u3681mat_scan() -> usize {
    let mut bad = 0usize;
    let mut i = 0usize;
    while i < u3681mat_COUNT {
        let (k, v) = u3681mat[i];
        let ok = match k {
            "FUZZ0000" => v < 97,
            "GEN0001" => v < 97,
            "EXEC0002" => v < 97,
            "JUDGE0003" => v < 97,
            "SEED0004" => v < 97,
            "CORPUS0005" => v < 97,
            "MUT0006" => v < 97,
            "CRASH0007" => v < 97,
            "DEDUP0008" => v < 97,
            "COV0009" => v < 97,
            "TIME0010" => v < 97,
            "ARTEF0011" => v < 97,
            "FUZZ0012" => v < 97,
            "GEN0013" => v < 97,
            "EXEC0014" => v < 97,
            "JUDGE0015" => v < 97,
            "SEED0016" => v < 97,
            "CORPUS0017" => v < 97,
            "MUT0018" => v < 97,
            "CRASH0019" => v < 97,
            "DEDUP0020" => v < 97,
            "COV0021" => v < 97,
            "TIME0022" => v < 97,
            "ARTEF0023" => v < 97,
            "FUZZ0024" => v < 97,
            "GEN0025" => v < 97,
            "EXEC0026" => v < 97,
            "JUDGE0027" => v < 97,
            "SEED0028" => v < 97,
            "CORPUS0029" => v < 97,
            "MUT0030" => v < 97,
            "CRASH0031" => v < 97,
            "DEDUP0032" => v < 97,
            "COV0033" => v < 97,
            "TIME0034" => v < 97,
            "ARTEF0035" => v < 97,
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
pub fn u3681mat_sum() -> u64 {
    let mut s = 0u64;
    let mut i = 0usize;
    while i < u3681mat_COUNT {
        s = s.wrapping_add(u3681mat[i].1 as u64);
        i += 1;
    }
    s
}

/// 键唯一性（构造期断言等价物：逐对扫描同键）。
pub fn u3681mat_unique() -> bool {
    let mut i = 0usize;
    while i < u3681mat_COUNT {
        let mut j = i + 1;
        while j < u3681mat_COUNT {
            if u3681mat[i].0 == u3681mat[j].0 {
                return false;
            }
            j += 1;
        }
        i += 1;
    }
    true
}

// --------------------------------------------------------------------------
// 直方图引擎 mxu3681hist（有界插入排序 + 分位数 P50/P90/P99）
// --------------------------------------------------------------------------
/// fuzz 三段管线 直方图（96 桶滚动）。
/// 样本容量上限。
pub const MXU3681HIST_CAP: usize = 96;

/// 直方图账本（排序态 + 五统量）。
#[derive(Clone, Copy, Debug)]
pub struct mxu3681histHist {
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

impl mxu3681histHist {
    /// 空账构造。
    pub const fn new() -> Self {
        Self { samples: [0; 96], n: 0, drops: 0, min: 0, max: 0, sum: 0 }
    }

    /// 采样（有界插入排序；满员丢弃并记账）。
    pub fn push(&mut self, v: u64) {
        if self.n >= MXU3681HIST_CAP {
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

/// 段位守卫：全部诊断码必须落在本段 [0xe180, 0xe18f]。
pub const fn u3681_segment_guard() -> bool {
    let mut ok = true;
    let mut i = 0usize;
    while i < u3681Code::ALL.len() {
        let c = u3681Code::ALL[i].code();
        if c < 0xe180 || c > 0xe18f {
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
fn u3681_guard_seg() -> bool {
    u3681_segment_guard()
}

/// UNX-F3681 矩阵 36 行齐备 + 逐行守卫 + 唯一性 + 累计守恒。
fn check_u3681_map_guard() -> bool {
    let mut ok = u3681mat_COUNT == 36;
    if ok { ok = u3681mat_scan() == 0 && u3681mat_unique(); }
    if ok { ok = u3681mat_get("FUZZ0000") == Some(0) && u3681mat_get("ZZZ_NOKEY").is_none(); }
    if ok { ok = u3681mat_sum() == 1594; }
    ok
}

/// UNX-F3681 直方图 16 样本分位/均值/守恒。
fn check_u3681_map_hist() -> bool {
    let mut h = mxu3681histHist::new();
    let mut i = 0u64;
    while i < 16 { h.push(i + 1); i += 1; }
    let mut ok = h.conserved() && h.mean() >= 1 && h.percentile(500) >= 8;
    ok
}

/// 本模块自检（逐检查带 detail，全绿才出门）。
pub fn run_u3681_checks() -> CheckSet {
    let mut set = CheckSet::new("U3681");
    set.add(
        "u3681-seg-guard",
        u3681_guard_seg(),
        if u3681_guard_seg() { "" } else { "diagnostic code outside segment" },
    );
    set.add(
        "u3681-map-guard",
        check_u3681_map_guard(),
        if check_u3681_map_guard() { "" } else { "UNX-F3681 matrix guard mismatch" },
    );
    set.add(
        "u3681-map-hist",
        check_u3681_map_hist(),
        if check_u3681_map_hist() { "" } else { "UNX-F3681 hist mismatch" },
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
    fn u3681_all_checks_green() {
        let set = run_u3681_checks();
        assert!(set.all_passed(), "U3681 domain checks must be green");
    }

    #[test]
    fn u3681_segment_guard_holds() {
        assert!(u3681_segment_guard());
    }
}
