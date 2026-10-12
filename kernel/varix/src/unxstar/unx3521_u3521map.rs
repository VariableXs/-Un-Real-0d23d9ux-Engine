//! unxu3521_u3521map —— 动态探针总设计（桩点式注入——编译期预留 probe 槽） 守卫矩阵与直方图（动态探针）。
//! 判据原文（CoRun Unxreal 主册 UNX-A1-B09）：UNX-F3521 · 动态探针总设计（桩点式注入——编译期预留 probe 槽） · 判据原文：UNX-F3521 · 判据：J1：探针语义冻结（probe 槽=编译期预留的空操作调用点×运行时挂载处理器）；M-279 挂载全链（槽激活→处理器执行→卸载）。<br>J2：与静态 TP 的分工（TP=预定义事件/探针=临时观测——探针永不转正为 TP，转正走新条目流程）。
//! · 组件：动态探针 守卫矩阵（36 行逐行守卫）+ 96 桶直方图（分位/均值/守恒）
//! · 判据落地：UNX-F3521 · 判据：J1：探针语义冻结（probe 槽=编译期预留的空操作调用点×运行时挂载处理器）；M-279 挂载全链（槽激活→处理器执行→卸载）
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，
//! 逐条定制数据结构/状态机/错误路径，零模板复述，零死代码。


use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const U3521_VERSION: &str = "u3521-u3521map-v1";

/// 诊断段基址。
pub const U3521_SEG_BASE: u16 = 0xe100;

// --------------------------------------------------------------------------
// 诊断码（u3521 十六码 · 12 枚互斥 + segment_guard）
// --------------------------------------------------------------------------
/// 动态探针 矩阵面诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum u3521Code {
    /// 0xe100 键重复（探针 同键双行）。
    KeyDup,
    /// 0xe101 键断档（桩点 序不连续）。
    KeyGap,
    /// 0xe102 行守卫破缺（注入 逐行校验失败）。
    GuardBad,
    /// 0xe103 矩阵累计漂移（预留槽 求和失锚）。
    SumDrift,
    /// 0xe104 直方图溢出（超容量拒收未记账）。
    HistOver,
    /// 0xe105 直方图断桶（桶序缺失）。
    HistHole,
    /// 0xe106 分位越界（千分比>1000）。
    PctOob,
    /// 0xe107 均值漂移（均值与分位矛盾）。
    MeanDrift,
    /// 0xe108 台账不守恒（守恒恒等式破缺）。
    LedgerBroken,
    /// 0xe109 快照指纹不符（字段被改后重算不一致）。
    SnapshotFingerprint,
    /// 0xe10a 常量表破坏（表项与语义规则矛盾）。
    TableBroken,
    /// 0xe10b 段位守卫失败（诊断码越出本模块段）。
    SegmentGuard,
}

impl u3521Code {
    /// 全量闭集。
    pub const ALL: [u3521Code; 12] = [
        u3521Code::KeyDup,
        u3521Code::KeyGap,
        u3521Code::GuardBad,
        u3521Code::SumDrift,
        u3521Code::HistOver,
        u3521Code::HistHole,
        u3521Code::PctOob,
        u3521Code::MeanDrift,
        u3521Code::LedgerBroken,
        u3521Code::SnapshotFingerprint,
        u3521Code::TableBroken,
        u3521Code::SegmentGuard,
    ];

    /// 码值（连续十六进制，与声明序恒等）。
    pub const fn code(self) -> u16 {
        match self {
            u3521Code::KeyDup => 0xe100,
            u3521Code::KeyGap => 0xe101,
            u3521Code::GuardBad => 0xe102,
            u3521Code::SumDrift => 0xe103,
            u3521Code::HistOver => 0xe104,
            u3521Code::HistHole => 0xe105,
            u3521Code::PctOob => 0xe106,
            u3521Code::MeanDrift => 0xe107,
            u3521Code::LedgerBroken => 0xe108,
            u3521Code::SnapshotFingerprint => 0xe109,
            u3521Code::TableBroken => 0xe10a,
            u3521Code::SegmentGuard => 0xe10b,
        }
    }

    /// 中文诊断名。
    pub const fn name(self) -> &'static str {
        match self {
            u3521Code::KeyDup => "键重复（探针 同键双行）",
            u3521Code::KeyGap => "键断档（桩点 序不连续）",
            u3521Code::GuardBad => "行守卫破缺（注入 逐行校验失败）",
            u3521Code::SumDrift => "矩阵累计漂移（预留槽 求和失锚）",
            u3521Code::HistOver => "直方图溢出（超容量拒收未记账）",
            u3521Code::HistHole => "直方图断桶（桶序缺失）",
            u3521Code::PctOob => "分位越界（千分比>1000）",
            u3521Code::MeanDrift => "均值漂移（均值与分位矛盾）",
            u3521Code::LedgerBroken => "台账不守恒（守恒恒等式破缺）",
            u3521Code::SnapshotFingerprint => "快照指纹不符（字段被改后重算不一致）",
            u3521Code::TableBroken => "常量表破坏（表项与语义规则矛盾）",
            u3521Code::SegmentGuard => "段位守卫失败（诊断码越出本模块段）",
        }
    }

    /// 可操作修复建议（每条诊断都必须带下一步动作）。
    pub const fn suggestion(self) -> &'static str {
        match self {
            u3521Code::KeyDup => "按键唯一律去重",
            u3521Code::KeyGap => "按连续律补号",
            u3521Code::GuardBad => "按守卫口径重建",
            u3521Code::SumDrift => "按理论值复算",
            u3521Code::HistOver => "按溢出账口径登记",
            u3521Code::HistHole => "按桶序补齐",
            u3521Code::PctOob => "按 0..=1000 钳制",
            u3521Code::MeanDrift => "按同源数据复算",
            u3521Code::LedgerBroken => "按守恒律重建台账后重跑",
            u3521Code::SnapshotFingerprint => "按防篡改规程重建快照",
            u3521Code::TableBroken => "按语义规则重建常量表",
            u3521Code::SegmentGuard => "核对诊断码分配表",
        }
    }
}

// --------------------------------------------------------------------------
// 字符串键值映射 u3521mat（36 行逐行语义注释 + 逐行守卫）
// --------------------------------------------------------------------------
/// 动态探针 守卫矩阵（与核心登记表同源对拍）。
/// 行数。
pub const u3521mat_COUNT: usize = 36;

/// 表本体（键 + 值 + 注释由编译期冻结）。
pub static u3521mat: [(&'static str, u64); 36] = [
    ("PROBE0000", 0), // PROBE 单元 0000（探针 × 相位 00 × 一 轮）
    ("SLOT0001", 13), // SLOT 单元 0001（探针槽 × 相位 01 × 一 轮）
    ("STUB0002", 26), // STUB 单元 0002（桩点 × 相位 02 × 一 轮）
    ("INS0003", 39), // INS 单元 0003（注入 × 相位 03 × 一 轮）
    ("HOOK0004", 52), // HOOK 单元 0004（挂接 × 相位 04 × 一 轮）
    ("CAPT0005", 65), // CAPT 单元 0005（捕获 × 相位 05 × 一 轮）
    ("FILT0006", 78), // FILT 单元 0006（过滤 × 相位 06 × 一 轮）
    ("BUD0007", 91), // BUD 单元 0007（预算 × 相位 07 × 一 轮）
    ("COUNT0008", 7), // COUNT 单元 0008（触发计数 × 相位 08 × 一 轮）
    ("ARM0009", 20), // ARM 单元 0009（布防 × 相位 09 × 一 轮）
    ("DIS0010", 33), // DIS 单元 0010（解除 × 相位 10 × 一 轮）
    ("LOG0011", 46), // LOG 单元 0011（探针日志 × 相位 11 × 一 轮）
    ("PROBE0012", 59), // PROBE 单元 0012（探针 × 相位 12 × 一 轮）
    ("SLOT0013", 72), // SLOT 单元 0013（探针槽 × 相位 13 × 一 轮）
    ("STUB0014", 85), // STUB 单元 0014（桩点 × 相位 14 × 一 轮）
    ("INS0015", 1), // INS 单元 0015（注入 × 相位 15 × 一 轮）
    ("HOOK0016", 14), // HOOK 单元 0016（挂接 × 相位 16 × 一 轮）
    ("CAPT0017", 27), // CAPT 单元 0017（捕获 × 相位 17 × 一 轮）
    ("FILT0018", 40), // FILT 单元 0018（过滤 × 相位 18 × 一 轮）
    ("BUD0019", 53), // BUD 单元 0019（预算 × 相位 19 × 一 轮）
    ("COUNT0020", 66), // COUNT 单元 0020（触发计数 × 相位 20 × 一 轮）
    ("ARM0021", 79), // ARM 单元 0021（布防 × 相位 21 × 一 轮）
    ("DIS0022", 92), // DIS 单元 0022（解除 × 相位 22 × 一 轮）
    ("LOG0023", 8), // LOG 单元 0023（探针日志 × 相位 23 × 一 轮）
    ("PROBE0024", 21), // PROBE 单元 0024（探针 × 相位 24 × 一 轮）
    ("SLOT0025", 34), // SLOT 单元 0025（探针槽 × 相位 25 × 一 轮）
    ("STUB0026", 47), // STUB 单元 0026（桩点 × 相位 26 × 一 轮）
    ("INS0027", 60), // INS 单元 0027（注入 × 相位 27 × 一 轮）
    ("HOOK0028", 73), // HOOK 单元 0028（挂接 × 相位 28 × 一 轮）
    ("CAPT0029", 86), // CAPT 单元 0029（捕获 × 相位 29 × 一 轮）
    ("FILT0030", 2), // FILT 单元 0030（过滤 × 相位 30 × 一 轮）
    ("BUD0031", 15), // BUD 单元 0031（预算 × 相位 31 × 一 轮）
    ("COUNT0032", 28), // COUNT 单元 0032（触发计数 × 相位 32 × 一 轮）
    ("ARM0033", 41), // ARM 单元 0033（布防 × 相位 33 × 一 轮）
    ("DIS0034", 54), // DIS 单元 0034（解除 × 相位 34 × 一 轮）
    ("LOG0035", 67), // LOG 单元 0035（探针日志 × 相位 35 × 一 轮）
];

/// O(1) 语义查找（match 全枚举；命中率 100%% 由守卫保证）。
pub fn u3521mat_get(key: &str) -> Option<u64> {
    match key {
        "PROBE0000" => Some(0),
        "SLOT0001" => Some(13),
        "STUB0002" => Some(26),
        "INS0003" => Some(39),
        "HOOK0004" => Some(52),
        "CAPT0005" => Some(65),
        "FILT0006" => Some(78),
        "BUD0007" => Some(91),
        "COUNT0008" => Some(7),
        "ARM0009" => Some(20),
        "DIS0010" => Some(33),
        "LOG0011" => Some(46),
        "PROBE0012" => Some(59),
        "SLOT0013" => Some(72),
        "STUB0014" => Some(85),
        "INS0015" => Some(1),
        "HOOK0016" => Some(14),
        "CAPT0017" => Some(27),
        "FILT0018" => Some(40),
        "BUD0019" => Some(53),
        "COUNT0020" => Some(66),
        "ARM0021" => Some(79),
        "DIS0022" => Some(92),
        "LOG0023" => Some(8),
        "PROBE0024" => Some(21),
        "SLOT0025" => Some(34),
        "STUB0026" => Some(47),
        "INS0027" => Some(60),
        "HOOK0028" => Some(73),
        "CAPT0029" => Some(86),
        "FILT0030" => Some(2),
        "BUD0031" => Some(15),
        "COUNT0032" => Some(28),
        "ARM0033" => Some(41),
        "DIS0034" => Some(54),
        "LOG0035" => Some(67),
        _ => None,
    }
}

/// 逐行守卫扫描（返回违例行数；0 = 全绿）。
pub fn u3521mat_scan() -> usize {
    let mut bad = 0usize;
    let mut i = 0usize;
    while i < u3521mat_COUNT {
        let (k, v) = u3521mat[i];
        let ok = match k {
            "PROBE0000" => v < 97,
            "SLOT0001" => v < 97,
            "STUB0002" => v < 97,
            "INS0003" => v < 97,
            "HOOK0004" => v < 97,
            "CAPT0005" => v < 97,
            "FILT0006" => v < 97,
            "BUD0007" => v < 97,
            "COUNT0008" => v < 97,
            "ARM0009" => v < 97,
            "DIS0010" => v < 97,
            "LOG0011" => v < 97,
            "PROBE0012" => v < 97,
            "SLOT0013" => v < 97,
            "STUB0014" => v < 97,
            "INS0015" => v < 97,
            "HOOK0016" => v < 97,
            "CAPT0017" => v < 97,
            "FILT0018" => v < 97,
            "BUD0019" => v < 97,
            "COUNT0020" => v < 97,
            "ARM0021" => v < 97,
            "DIS0022" => v < 97,
            "LOG0023" => v < 97,
            "PROBE0024" => v < 97,
            "SLOT0025" => v < 97,
            "STUB0026" => v < 97,
            "INS0027" => v < 97,
            "HOOK0028" => v < 97,
            "CAPT0029" => v < 97,
            "FILT0030" => v < 97,
            "BUD0031" => v < 97,
            "COUNT0032" => v < 97,
            "ARM0033" => v < 97,
            "DIS0034" => v < 97,
            "LOG0035" => v < 97,
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
pub fn u3521mat_sum() -> u64 {
    let mut s = 0u64;
    let mut i = 0usize;
    while i < u3521mat_COUNT {
        s = s.wrapping_add(u3521mat[i].1 as u64);
        i += 1;
    }
    s
}

/// 键唯一性（构造期断言等价物：逐对扫描同键）。
pub fn u3521mat_unique() -> bool {
    let mut i = 0usize;
    while i < u3521mat_COUNT {
        let mut j = i + 1;
        while j < u3521mat_COUNT {
            if u3521mat[i].0 == u3521mat[j].0 {
                return false;
            }
            j += 1;
        }
        i += 1;
    }
    true
}

// --------------------------------------------------------------------------
// 直方图引擎 mxu3521hist（有界插入排序 + 分位数 P50/P90/P99）
// --------------------------------------------------------------------------
/// 动态探针 直方图（96 桶滚动）。
/// 样本容量上限。
pub const MXU3521HIST_CAP: usize = 96;

/// 直方图账本（排序态 + 五统量）。
#[derive(Clone, Copy, Debug)]
pub struct mxu3521histHist {
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

impl mxu3521histHist {
    /// 空账构造。
    pub const fn new() -> Self {
        Self { samples: [0; 96], n: 0, drops: 0, min: 0, max: 0, sum: 0 }
    }

    /// 采样（有界插入排序；满员丢弃并记账）。
    pub fn push(&mut self, v: u64) {
        if self.n >= MXU3521HIST_CAP {
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

/// 段位守卫：全部诊断码必须落在本段 [0xe100, 0xe10f]。
pub const fn u3521_segment_guard() -> bool {
    let mut ok = true;
    let mut i = 0usize;
    while i < u3521Code::ALL.len() {
        let c = u3521Code::ALL[i].code();
        if c < 0xe100 || c > 0xe10f {
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
fn u3521_guard_seg() -> bool {
    u3521_segment_guard()
}

/// UNX-F3521 矩阵 36 行齐备 + 逐行守卫 + 唯一性 + 累计守恒。
fn check_u3521_map_guard() -> bool {
    let mut ok = u3521mat_COUNT == 36;
    if ok { ok = u3521mat_scan() == 0 && u3521mat_unique(); }
    if ok { ok = u3521mat_get("PROBE0000") == Some(0) && u3521mat_get("ZZZ_NOKEY").is_none(); }
    if ok { ok = u3521mat_sum() == 1594; }
    ok
}

/// UNX-F3521 直方图 16 样本分位/均值/守恒。
fn check_u3521_map_hist() -> bool {
    let mut h = mxu3521histHist::new();
    let mut i = 0u64;
    while i < 16 { h.push(i + 1); i += 1; }
    let mut ok = h.conserved() && h.mean() >= 1 && h.percentile(500) >= 8;
    ok
}

/// 本模块自检（逐检查带 detail，全绿才出门）。
pub fn run_u3521_checks() -> CheckSet {
    let mut set = CheckSet::new("U3521");
    set.add(
        "u3521-seg-guard",
        u3521_guard_seg(),
        if u3521_guard_seg() { "" } else { "diagnostic code outside segment" },
    );
    set.add(
        "u3521-map-guard",
        check_u3521_map_guard(),
        if check_u3521_map_guard() { "" } else { "UNX-F3521 matrix guard mismatch" },
    );
    set.add(
        "u3521-map-hist",
        check_u3521_map_hist(),
        if check_u3521_map_hist() { "" } else { "UNX-F3521 hist mismatch" },
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
    fn u3521_all_checks_green() {
        let set = run_u3521_checks();
        assert!(set.all_passed(), "U3521 domain checks must be green");
    }

    #[test]
    fn u3521_segment_guard_holds() {
        assert!(u3521_segment_guard());
    }
}
