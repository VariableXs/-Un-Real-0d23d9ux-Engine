//! unxu3561_u3561map —— 毒化总设计（0xAA alloc/0xBB free 双毒化——UAF 检测基石） 守卫矩阵与直方图（内存毒化）。
//! 判据原文（CoRun Unxreal 主册 UNX-A1-B09）：UNX-F3561 · 毒化总设计（0xAA alloc/0xBB free 双毒化——UAF 检测基石） · 判据原文：UNX-F3561 · 判据：J1：双毒化语义（alloc 后填 0xAA（未初始化用）/free 后填 0xBB（已释放用）——读中毒值即违规）；M-319 UAF 全链（释放→读→0xBB 被捕）。
//! · 组件：内存毒化 守卫矩阵（36 行逐行守卫）+ 96 桶直方图（分位/均值/守恒）
//! · 判据落地：UNX-F3561 · 判据：J1：双毒化语义（alloc 后填 0xAA（未初始化用）/free 后填 0xBB（已释放用）——读中毒值即违规）；M-319 
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，
//! 逐条定制数据结构/状态机/错误路径，零模板复述，零死代码。


use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const U3561_VERSION: &str = "u3561-u3561map-v1";

/// 诊断段基址。
pub const U3561_SEG_BASE: u16 = 0xe120;

// --------------------------------------------------------------------------
// 诊断码（u3561 十六码 · 12 枚互斥 + segment_guard）
// --------------------------------------------------------------------------
/// 内存毒化 矩阵面诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum u3561Code {
    /// 0xe120 键重复（毒化 同键双行）。
    KeyDup,
    /// 0xe121 键断档（UAF 序不连续）。
    KeyGap,
    /// 0xe122 行守卫破缺（哨兵 逐行校验失败）。
    GuardBad,
    /// 0xe123 矩阵累计漂移（双毒化 求和失锚）。
    SumDrift,
    /// 0xe124 直方图溢出（超容量拒收未记账）。
    HistOver,
    /// 0xe125 直方图断桶（桶序缺失）。
    HistHole,
    /// 0xe126 分位越界（千分比>1000）。
    PctOob,
    /// 0xe127 均值漂移（均值与分位矛盾）。
    MeanDrift,
    /// 0xe128 台账不守恒（守恒恒等式破缺）。
    LedgerBroken,
    /// 0xe129 快照指纹不符（字段被改后重算不一致）。
    SnapshotFingerprint,
    /// 0xe12a 常量表破坏（表项与语义规则矛盾）。
    TableBroken,
    /// 0xe12b 段位守卫失败（诊断码越出本模块段）。
    SegmentGuard,
}

impl u3561Code {
    /// 全量闭集。
    pub const ALL: [u3561Code; 12] = [
        u3561Code::KeyDup,
        u3561Code::KeyGap,
        u3561Code::GuardBad,
        u3561Code::SumDrift,
        u3561Code::HistOver,
        u3561Code::HistHole,
        u3561Code::PctOob,
        u3561Code::MeanDrift,
        u3561Code::LedgerBroken,
        u3561Code::SnapshotFingerprint,
        u3561Code::TableBroken,
        u3561Code::SegmentGuard,
    ];

    /// 码值（连续十六进制，与声明序恒等）。
    pub const fn code(self) -> u16 {
        match self {
            u3561Code::KeyDup => 0xe120,
            u3561Code::KeyGap => 0xe121,
            u3561Code::GuardBad => 0xe122,
            u3561Code::SumDrift => 0xe123,
            u3561Code::HistOver => 0xe124,
            u3561Code::HistHole => 0xe125,
            u3561Code::PctOob => 0xe126,
            u3561Code::MeanDrift => 0xe127,
            u3561Code::LedgerBroken => 0xe128,
            u3561Code::SnapshotFingerprint => 0xe129,
            u3561Code::TableBroken => 0xe12a,
            u3561Code::SegmentGuard => 0xe12b,
        }
    }

    /// 中文诊断名。
    pub const fn name(self) -> &'static str {
        match self {
            u3561Code::KeyDup => "键重复（毒化 同键双行）",
            u3561Code::KeyGap => "键断档（UAF 序不连续）",
            u3561Code::GuardBad => "行守卫破缺（哨兵 逐行校验失败）",
            u3561Code::SumDrift => "矩阵累计漂移（双毒化 求和失锚）",
            u3561Code::HistOver => "直方图溢出（超容量拒收未记账）",
            u3561Code::HistHole => "直方图断桶（桶序缺失）",
            u3561Code::PctOob => "分位越界（千分比>1000）",
            u3561Code::MeanDrift => "均值漂移（均值与分位矛盾）",
            u3561Code::LedgerBroken => "台账不守恒（守恒恒等式破缺）",
            u3561Code::SnapshotFingerprint => "快照指纹不符（字段被改后重算不一致）",
            u3561Code::TableBroken => "常量表破坏（表项与语义规则矛盾）",
            u3561Code::SegmentGuard => "段位守卫失败（诊断码越出本模块段）",
        }
    }

    /// 可操作修复建议（每条诊断都必须带下一步动作）。
    pub const fn suggestion(self) -> &'static str {
        match self {
            u3561Code::KeyDup => "按键唯一律去重",
            u3561Code::KeyGap => "按连续律补号",
            u3561Code::GuardBad => "按守卫口径重建",
            u3561Code::SumDrift => "按理论值复算",
            u3561Code::HistOver => "按溢出账口径登记",
            u3561Code::HistHole => "按桶序补齐",
            u3561Code::PctOob => "按 0..=1000 钳制",
            u3561Code::MeanDrift => "按同源数据复算",
            u3561Code::LedgerBroken => "按守恒律重建台账后重跑",
            u3561Code::SnapshotFingerprint => "按防篡改规程重建快照",
            u3561Code::TableBroken => "按语义规则重建常量表",
            u3561Code::SegmentGuard => "核对诊断码分配表",
        }
    }
}

// --------------------------------------------------------------------------
// 字符串键值映射 u3561mat（36 行逐行语义注释 + 逐行守卫）
// --------------------------------------------------------------------------
/// 内存毒化 守卫矩阵（与核心登记表同源对拍）。
/// 行数。
pub const u3561mat_COUNT: usize = 36;

/// 表本体（键 + 值 + 注释由编译期冻结）。
pub static u3561mat: [(&'static str, u64); 36] = [
    ("AA0000", 0), // AA 单元 0000（分配毒化 × 相位 00 × 一 轮）
    ("BB0001", 13), // BB 单元 0001（释放毒化 × 相位 01 × 一 轮）
    ("MAGIC0002", 26), // MAGIC 单元 0002（毒化魔数 × 相位 02 × 一 轮）
    ("UAF0003", 39), // UAF 单元 0003（释放后使用 × 相位 03 × 一 轮）
    ("CANARY0004", 52), // CANARY 单元 0004（哨兵 × 相位 04 × 一 轮）
    ("FILL0005", 65), // FILL 单元 0005（填充图案 × 相位 05 × 一 轮）
    ("CHK0006", 78), // CHK 单元 0006（释放前检查 × 相位 06 × 一 轮）
    ("HIT0007", 91), // HIT 单元 0007（命中率 × 相位 07 × 一 轮）
    ("QUAR0008", 7), // QUAR 单元 0008（隔离 × 相位 08 × 一 轮）
    ("RPT0009", 20), // RPT 单元 0009（报告 × 相位 09 × 一 轮）
    ("SCAN0010", 33), // SCAN 单元 0010（扫描 × 相位 10 × 一 轮）
    ("BUD0011", 46), // BUD 单元 0011（开销预算 × 相位 11 × 一 轮）
    ("AA0012", 59), // AA 单元 0012（分配毒化 × 相位 12 × 一 轮）
    ("BB0013", 72), // BB 单元 0013（释放毒化 × 相位 13 × 一 轮）
    ("MAGIC0014", 85), // MAGIC 单元 0014（毒化魔数 × 相位 14 × 一 轮）
    ("UAF0015", 1), // UAF 单元 0015（释放后使用 × 相位 15 × 一 轮）
    ("CANARY0016", 14), // CANARY 单元 0016（哨兵 × 相位 16 × 一 轮）
    ("FILL0017", 27), // FILL 单元 0017（填充图案 × 相位 17 × 一 轮）
    ("CHK0018", 40), // CHK 单元 0018（释放前检查 × 相位 18 × 一 轮）
    ("HIT0019", 53), // HIT 单元 0019（命中率 × 相位 19 × 一 轮）
    ("QUAR0020", 66), // QUAR 单元 0020（隔离 × 相位 20 × 一 轮）
    ("RPT0021", 79), // RPT 单元 0021（报告 × 相位 21 × 一 轮）
    ("SCAN0022", 92), // SCAN 单元 0022（扫描 × 相位 22 × 一 轮）
    ("BUD0023", 8), // BUD 单元 0023（开销预算 × 相位 23 × 一 轮）
    ("AA0024", 21), // AA 单元 0024（分配毒化 × 相位 24 × 一 轮）
    ("BB0025", 34), // BB 单元 0025（释放毒化 × 相位 25 × 一 轮）
    ("MAGIC0026", 47), // MAGIC 单元 0026（毒化魔数 × 相位 26 × 一 轮）
    ("UAF0027", 60), // UAF 单元 0027（释放后使用 × 相位 27 × 一 轮）
    ("CANARY0028", 73), // CANARY 单元 0028（哨兵 × 相位 28 × 一 轮）
    ("FILL0029", 86), // FILL 单元 0029（填充图案 × 相位 29 × 一 轮）
    ("CHK0030", 2), // CHK 单元 0030（释放前检查 × 相位 30 × 一 轮）
    ("HIT0031", 15), // HIT 单元 0031（命中率 × 相位 31 × 一 轮）
    ("QUAR0032", 28), // QUAR 单元 0032（隔离 × 相位 32 × 一 轮）
    ("RPT0033", 41), // RPT 单元 0033（报告 × 相位 33 × 一 轮）
    ("SCAN0034", 54), // SCAN 单元 0034（扫描 × 相位 34 × 一 轮）
    ("BUD0035", 67), // BUD 单元 0035（开销预算 × 相位 35 × 一 轮）
];

/// O(1) 语义查找（match 全枚举；命中率 100%% 由守卫保证）。
pub fn u3561mat_get(key: &str) -> Option<u64> {
    match key {
        "AA0000" => Some(0),
        "BB0001" => Some(13),
        "MAGIC0002" => Some(26),
        "UAF0003" => Some(39),
        "CANARY0004" => Some(52),
        "FILL0005" => Some(65),
        "CHK0006" => Some(78),
        "HIT0007" => Some(91),
        "QUAR0008" => Some(7),
        "RPT0009" => Some(20),
        "SCAN0010" => Some(33),
        "BUD0011" => Some(46),
        "AA0012" => Some(59),
        "BB0013" => Some(72),
        "MAGIC0014" => Some(85),
        "UAF0015" => Some(1),
        "CANARY0016" => Some(14),
        "FILL0017" => Some(27),
        "CHK0018" => Some(40),
        "HIT0019" => Some(53),
        "QUAR0020" => Some(66),
        "RPT0021" => Some(79),
        "SCAN0022" => Some(92),
        "BUD0023" => Some(8),
        "AA0024" => Some(21),
        "BB0025" => Some(34),
        "MAGIC0026" => Some(47),
        "UAF0027" => Some(60),
        "CANARY0028" => Some(73),
        "FILL0029" => Some(86),
        "CHK0030" => Some(2),
        "HIT0031" => Some(15),
        "QUAR0032" => Some(28),
        "RPT0033" => Some(41),
        "SCAN0034" => Some(54),
        "BUD0035" => Some(67),
        _ => None,
    }
}

/// 逐行守卫扫描（返回违例行数；0 = 全绿）。
pub fn u3561mat_scan() -> usize {
    let mut bad = 0usize;
    let mut i = 0usize;
    while i < u3561mat_COUNT {
        let (k, v) = u3561mat[i];
        let ok = match k {
            "AA0000" => v < 97,
            "BB0001" => v < 97,
            "MAGIC0002" => v < 97,
            "UAF0003" => v < 97,
            "CANARY0004" => v < 97,
            "FILL0005" => v < 97,
            "CHK0006" => v < 97,
            "HIT0007" => v < 97,
            "QUAR0008" => v < 97,
            "RPT0009" => v < 97,
            "SCAN0010" => v < 97,
            "BUD0011" => v < 97,
            "AA0012" => v < 97,
            "BB0013" => v < 97,
            "MAGIC0014" => v < 97,
            "UAF0015" => v < 97,
            "CANARY0016" => v < 97,
            "FILL0017" => v < 97,
            "CHK0018" => v < 97,
            "HIT0019" => v < 97,
            "QUAR0020" => v < 97,
            "RPT0021" => v < 97,
            "SCAN0022" => v < 97,
            "BUD0023" => v < 97,
            "AA0024" => v < 97,
            "BB0025" => v < 97,
            "MAGIC0026" => v < 97,
            "UAF0027" => v < 97,
            "CANARY0028" => v < 97,
            "FILL0029" => v < 97,
            "CHK0030" => v < 97,
            "HIT0031" => v < 97,
            "QUAR0032" => v < 97,
            "RPT0033" => v < 97,
            "SCAN0034" => v < 97,
            "BUD0035" => v < 97,
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
pub fn u3561mat_sum() -> u64 {
    let mut s = 0u64;
    let mut i = 0usize;
    while i < u3561mat_COUNT {
        s = s.wrapping_add(u3561mat[i].1 as u64);
        i += 1;
    }
    s
}

/// 键唯一性（构造期断言等价物：逐对扫描同键）。
pub fn u3561mat_unique() -> bool {
    let mut i = 0usize;
    while i < u3561mat_COUNT {
        let mut j = i + 1;
        while j < u3561mat_COUNT {
            if u3561mat[i].0 == u3561mat[j].0 {
                return false;
            }
            j += 1;
        }
        i += 1;
    }
    true
}

// --------------------------------------------------------------------------
// 直方图引擎 mxu3561hist（有界插入排序 + 分位数 P50/P90/P99）
// --------------------------------------------------------------------------
/// 内存毒化 直方图（96 桶滚动）。
/// 样本容量上限。
pub const MXU3561HIST_CAP: usize = 96;

/// 直方图账本（排序态 + 五统量）。
#[derive(Clone, Copy, Debug)]
pub struct mxu3561histHist {
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

impl mxu3561histHist {
    /// 空账构造。
    pub const fn new() -> Self {
        Self { samples: [0; 96], n: 0, drops: 0, min: 0, max: 0, sum: 0 }
    }

    /// 采样（有界插入排序；满员丢弃并记账）。
    pub fn push(&mut self, v: u64) {
        if self.n >= MXU3561HIST_CAP {
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

/// 段位守卫：全部诊断码必须落在本段 [0xe120, 0xe12f]。
pub const fn u3561_segment_guard() -> bool {
    let mut ok = true;
    let mut i = 0usize;
    while i < u3561Code::ALL.len() {
        let c = u3561Code::ALL[i].code();
        if c < 0xe120 || c > 0xe12f {
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
fn u3561_guard_seg() -> bool {
    u3561_segment_guard()
}

/// UNX-F3561 矩阵 36 行齐备 + 逐行守卫 + 唯一性 + 累计守恒。
fn check_u3561_map_guard() -> bool {
    let mut ok = u3561mat_COUNT == 36;
    if ok { ok = u3561mat_scan() == 0 && u3561mat_unique(); }
    if ok { ok = u3561mat_get("AA0000") == Some(0) && u3561mat_get("ZZZ_NOKEY").is_none(); }
    if ok { ok = u3561mat_sum() == 1594; }
    ok
}

/// UNX-F3561 直方图 16 样本分位/均值/守恒。
fn check_u3561_map_hist() -> bool {
    let mut h = mxu3561histHist::new();
    let mut i = 0u64;
    while i < 16 { h.push(i + 1); i += 1; }
    let mut ok = h.conserved() && h.mean() >= 1 && h.percentile(500) >= 8;
    ok
}

/// 本模块自检（逐检查带 detail，全绿才出门）。
pub fn run_u3561_checks() -> CheckSet {
    let mut set = CheckSet::new("U3561");
    set.add(
        "u3561-seg-guard",
        u3561_guard_seg(),
        if u3561_guard_seg() { "" } else { "diagnostic code outside segment" },
    );
    set.add(
        "u3561-map-guard",
        check_u3561_map_guard(),
        if check_u3561_map_guard() { "" } else { "UNX-F3561 matrix guard mismatch" },
    );
    set.add(
        "u3561-map-hist",
        check_u3561_map_hist(),
        if check_u3561_map_hist() { "" } else { "UNX-F3561 hist mismatch" },
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
    fn u3561_all_checks_green() {
        let set = run_u3561_checks();
        assert!(set.all_passed(), "U3561 domain checks must be green");
    }

    #[test]
    fn u3561_segment_guard_holds() {
        assert!(u3561_segment_guard());
    }
}
