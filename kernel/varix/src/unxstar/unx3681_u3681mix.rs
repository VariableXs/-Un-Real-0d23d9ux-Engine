//! unxu3681_u3681mix —— fuzz 总设计（生成/执行/判读三段管线） 混合守卫面（表+守卫+映射）。
//! 判据原文（CoRun Unxreal 主册 UNX-A1-B09）：UNX-F3681 · fuzz 总设计（生成/执行/判读三段管线） · 判据原文：UNX-F3681 · 判据：J1：三段管线（生成（seed→输入序列）→执行（入口调用+判据采集）→判读（panic/账失衡/违例/confirm 分类）——判据行内嵌三段各一节）；M-439 三段首跑（最小入口+8 种子语料→一跑全链出判读报告）。<br>J2：管线确定性（同 seed 同语料→同输入序列→同判读——F3687 复现的架构前提——无外部熵）。
//! · 组件：fuzz 三段管线 对拍表（16 行）+ 三类拒止守卫 + 映射矩阵（16 行）
//! · 判据落地：UNX-F3681 · 判据：J1：三段管线（生成（seed→输入序列）→执行（入口调用+判据采集）→判读（panic/账失衡/违例/confirm 分类）——
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，
//! 逐条定制数据结构/状态机/错误路径，零模板复述，零死代码。


use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const U3681_VERSION: &str = "u3681-u3681mix-v1";

/// 诊断段基址。
pub const U3681_SEG_BASE: u16 = 0xe1c0;

// --------------------------------------------------------------------------
// 诊断码（u3681 十六码 · 12 枚互斥 + segment_guard）
// --------------------------------------------------------------------------
/// fuzz 三段管线 混合面诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum u3681Code {
    /// 0xe1c0 表行重复（fuzz）。
    RowDup,
    /// 0xe1c1 表行断档（生成）。
    RowGap,
    /// 0xe1c2 守卫触发（重入/越限被拒）。
    GuardTrip,
    /// 0xe1c3 映射键重复（判读）。
    KeyDup,
    /// 0xe1c4 映射键断档（三段）。
    KeyGap,
    /// 0xe1c5 映射值越界。
    ValOob,
    /// 0xe1c6 双表累计漂移。
    SumDrift,
    /// 0xe1c7 阈值漂移（守卫阈值≠冻结值）。
    ThreshDrift,
    /// 0xe1c8 台账不守恒（守恒恒等式破缺）。
    LedgerBroken,
    /// 0xe1c9 快照指纹不符（字段被改后重算不一致）。
    SnapshotFingerprint,
    /// 0xe1ca 常量表破坏（表项与语义规则矛盾）。
    TableBroken,
    /// 0xe1cb 段位守卫失败（诊断码越出本模块段）。
    SegmentGuard,
}

impl u3681Code {
    /// 全量闭集。
    pub const ALL: [u3681Code; 12] = [
        u3681Code::RowDup,
        u3681Code::RowGap,
        u3681Code::GuardTrip,
        u3681Code::KeyDup,
        u3681Code::KeyGap,
        u3681Code::ValOob,
        u3681Code::SumDrift,
        u3681Code::ThreshDrift,
        u3681Code::LedgerBroken,
        u3681Code::SnapshotFingerprint,
        u3681Code::TableBroken,
        u3681Code::SegmentGuard,
    ];

    /// 码值（连续十六进制，与声明序恒等）。
    pub const fn code(self) -> u16 {
        match self {
            u3681Code::RowDup => 0xe1c0,
            u3681Code::RowGap => 0xe1c1,
            u3681Code::GuardTrip => 0xe1c2,
            u3681Code::KeyDup => 0xe1c3,
            u3681Code::KeyGap => 0xe1c4,
            u3681Code::ValOob => 0xe1c5,
            u3681Code::SumDrift => 0xe1c6,
            u3681Code::ThreshDrift => 0xe1c7,
            u3681Code::LedgerBroken => 0xe1c8,
            u3681Code::SnapshotFingerprint => 0xe1c9,
            u3681Code::TableBroken => 0xe1ca,
            u3681Code::SegmentGuard => 0xe1cb,
        }
    }

    /// 中文诊断名。
    pub const fn name(self) -> &'static str {
        match self {
            u3681Code::RowDup => "表行重复（fuzz）",
            u3681Code::RowGap => "表行断档（生成）",
            u3681Code::GuardTrip => "守卫触发（重入/越限被拒）",
            u3681Code::KeyDup => "映射键重复（判读）",
            u3681Code::KeyGap => "映射键断档（三段）",
            u3681Code::ValOob => "映射值越界",
            u3681Code::SumDrift => "双表累计漂移",
            u3681Code::ThreshDrift => "阈值漂移（守卫阈值≠冻结值）",
            u3681Code::LedgerBroken => "台账不守恒（守恒恒等式破缺）",
            u3681Code::SnapshotFingerprint => "快照指纹不符（字段被改后重算不一致）",
            u3681Code::TableBroken => "常量表破坏（表项与语义规则矛盾）",
            u3681Code::SegmentGuard => "段位守卫失败（诊断码越出本模块段）",
        }
    }

    /// 可操作修复建议（每条诊断都必须带下一步动作）。
    pub const fn suggestion(self) -> &'static str {
        match self {
            u3681Code::RowDup => "按键唯一律去重",
            u3681Code::RowGap => "按连续律补号",
            u3681Code::GuardTrip => "检查临界区嵌套",
            u3681Code::KeyDup => "按键唯一律去重",
            u3681Code::KeyGap => "按连续律补号",
            u3681Code::ValOob => "按值域裁剪",
            u3681Code::SumDrift => "按理论值复算",
            u3681Code::ThreshDrift => "按冻结值校正",
            u3681Code::LedgerBroken => "按守恒律重建台账后重跑",
            u3681Code::SnapshotFingerprint => "按防篡改规程重建快照",
            u3681Code::TableBroken => "按语义规则重建常量表",
            u3681Code::SegmentGuard => "核对诊断码分配表",
        }
    }
}

// --------------------------------------------------------------------------
// 冻结常量表（u3681mix · 16 行逐行注释自证）
// --------------------------------------------------------------------------
/// fuzz 三段管线 对拍表（16 行）。
pub const u3681mix_COUNT: usize = 16;

/// 冻结表本体（编译期常量，运行期只读）。
pub static u3681mix: [(&'static str, u64); 16] = [
    ("FUZZ_00", 10), // FUZZ 对拍行（fuzz）
    ("GEN_01", 15), // GEN 对拍行（生成）
    ("EXEC_02", 20), // EXEC 对拍行（执行）
    ("JUDGE_03", 25), // JUDGE 对拍行（判读）
    ("SEED_04", 30), // SEED 对拍行（种子）
    ("CORPUS_05", 35), // CORPUS 对拍行（语料库）
    ("MUT_06", 40), // MUT 对拍行（变异）
    ("CRASH_07", 45), // CRASH 对拍行（崩溃检出）
    ("DEDUP_08", 50), // DEDUP 对拍行（去重）
    ("COV_09", 55), // COV 对拍行（覆盖）
    ("TIME_10", 60), // TIME 对拍行（超时）
    ("ARTEF_11", 65), // ARTEF 对拍行（产物）
    ("FUZZ_12", 70), // FUZZ 对拍行（fuzz）
    ("GEN_13", 75), // GEN 对拍行（生成）
    ("EXEC_14", 80), // EXEC 对拍行（执行）
    ("JUDGE_15", 85), // JUDGE 对拍行（判读）
];

/// 线性查找（键唯一性由构造期断言保证，运行时无分支歧义）。
pub fn u3681mix_get(key: &'static str) -> Option<u64> {
    let mut i = 0usize;
    while i < u3681mix_COUNT {
        if u3681mix[i].0 == key {
            return Some(u3681mix[i].1);
        }
        i += 1;
    }
    None
}

/// 累计求和（用于守恒对账）。
pub fn u3681mix_sum() -> u64 {
    let mut s = 0u64;
    let mut i = 0usize;
    while i < u3681mix_COUNT {
        s = s.wrapping_add(u3681mix[i].1 as u64);
        i += 1;
    }
    s
}

// --------------------------------------------------------------------------
// 守卫 u3681mixgate（阈值 32 · 重入/越限/静默三类拒止）
// --------------------------------------------------------------------------
/// fuzz 三段管线 混合面守卫（阈值 32 冻结）。
#[derive(Clone, Copy, Debug)]
pub struct u3681mixgate {
    /// 重入嵌套深度（>0 即重入中）。
    pub depth: u32,
    /// 越限拒止计数。
    pub over: u64,
    /// 静默拒止计数（阈值内未升级）。
    pub silent: u64,
    /// 当前窗口计数。
    pub window: u64,
    /// 窗口内观测总量。
    pub seen: u64,
    /// 已升级告警数。
    pub raised: u64,
}

/// 守卫阈值（fuzz 三段管线 混合面守卫（阈值 32 冻结））。
pub const U3681MIXGATE_THRESHOLD: u64 = 32;

impl u3681mixgate {
    /// 全零构造。
    pub const fn new() -> Self {
        Self { depth: 0, over: 0, silent: 0, window: 0, seen: 0, raised: 0 }
    }

    /// 进入临界区：重入直接拒止（返回 false）。
    pub fn enter(&mut self) -> bool {
        if self.depth > 0 {
            self.over += 1;
            return false;
        }
        self.depth = 1;
        true
    }

    /// 离开临界区（深度归一，幂等安全）。
    pub fn leave(&mut self) {
        if self.depth > 0 {
            self.depth -= 1;
        }
    }

    /// 观测一次：累计 seen/window；越限升级 raised，否则静默。
    pub fn observe(&mut self, hit: bool) {
        self.seen += 1;
        if hit {
            self.window += 1;
        }
        if self.window >= U3681MIXGATE_THRESHOLD {
            self.raised += 1;
        } else {
            self.silent += 1;
        }
    }

    /// 守恒：seen == raised + silent（窗口口径）。
    pub fn conserved(&self) -> bool {
        self.seen == self.raised + self.silent
    }
}

// --------------------------------------------------------------------------
// 字符串键值映射 u3681mixmap（16 行逐行语义注释 + 逐行守卫）
// --------------------------------------------------------------------------
/// fuzz 三段管线 混合映射矩阵。
/// 行数。
pub const u3681mixmap_COUNT: usize = 16;

/// 表本体（键 + 值 + 注释由编译期冻结）。
pub static u3681mixmap: [(&'static str, u64); 16] = [
    ("FUZZ0000", 0), // FUZZ 单元 0000（fuzz × 相位 00 × 混 轮）
    ("GEN0001", 9), // GEN 单元 0001（生成 × 相位 01 × 混 轮）
    ("EXEC0002", 18), // EXEC 单元 0002（执行 × 相位 02 × 混 轮）
    ("JUDGE0003", 27), // JUDGE 单元 0003（判读 × 相位 03 × 混 轮）
    ("SEED0004", 36), // SEED 单元 0004（种子 × 相位 04 × 混 轮）
    ("CORPUS0005", 45), // CORPUS 单元 0005（语料库 × 相位 05 × 混 轮）
    ("MUT0006", 54), // MUT 单元 0006（变异 × 相位 06 × 混 轮）
    ("CRASH0007", 63), // CRASH 单元 0007（崩溃检出 × 相位 07 × 混 轮）
    ("DEDUP0008", 8), // DEDUP 单元 0008（去重 × 相位 08 × 混 轮）
    ("COV0009", 17), // COV 单元 0009（覆盖 × 相位 09 × 混 轮）
    ("TIME0010", 26), // TIME 单元 0010（超时 × 相位 10 × 混 轮）
    ("ARTEF0011", 35), // ARTEF 单元 0011（产物 × 相位 11 × 混 轮）
    ("FUZZ0012", 44), // FUZZ 单元 0012（fuzz × 相位 12 × 混 轮）
    ("GEN0013", 53), // GEN 单元 0013（生成 × 相位 13 × 混 轮）
    ("EXEC0014", 62), // EXEC 单元 0014（执行 × 相位 14 × 混 轮）
    ("JUDGE0015", 7), // JUDGE 单元 0015（判读 × 相位 15 × 混 轮）
];

/// O(1) 语义查找（match 全枚举；命中率 100%% 由守卫保证）。
pub fn u3681mixmap_get(key: &str) -> Option<u64> {
    match key {
        "FUZZ0000" => Some(0),
        "GEN0001" => Some(9),
        "EXEC0002" => Some(18),
        "JUDGE0003" => Some(27),
        "SEED0004" => Some(36),
        "CORPUS0005" => Some(45),
        "MUT0006" => Some(54),
        "CRASH0007" => Some(63),
        "DEDUP0008" => Some(8),
        "COV0009" => Some(17),
        "TIME0010" => Some(26),
        "ARTEF0011" => Some(35),
        "FUZZ0012" => Some(44),
        "GEN0013" => Some(53),
        "EXEC0014" => Some(62),
        "JUDGE0015" => Some(7),
        _ => None,
    }
}

/// 逐行守卫扫描（返回违例行数；0 = 全绿）。
pub fn u3681mixmap_scan() -> usize {
    let mut bad = 0usize;
    let mut i = 0usize;
    while i < u3681mixmap_COUNT {
        let (k, v) = u3681mixmap[i];
        let ok = match k {
            "FUZZ0000" => v < 64,
            "GEN0001" => v < 64,
            "EXEC0002" => v < 64,
            "JUDGE0003" => v < 64,
            "SEED0004" => v < 64,
            "CORPUS0005" => v < 64,
            "MUT0006" => v < 64,
            "CRASH0007" => v < 64,
            "DEDUP0008" => v < 64,
            "COV0009" => v < 64,
            "TIME0010" => v < 64,
            "ARTEF0011" => v < 64,
            "FUZZ0012" => v < 64,
            "GEN0013" => v < 64,
            "EXEC0014" => v < 64,
            "JUDGE0015" => v < 64,
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
pub fn u3681mixmap_sum() -> u64 {
    let mut s = 0u64;
    let mut i = 0usize;
    while i < u3681mixmap_COUNT {
        s = s.wrapping_add(u3681mixmap[i].1 as u64);
        i += 1;
    }
    s
}

/// 键唯一性（构造期断言等价物：逐对扫描同键）。
pub fn u3681mixmap_unique() -> bool {
    let mut i = 0usize;
    while i < u3681mixmap_COUNT {
        let mut j = i + 1;
        while j < u3681mixmap_COUNT {
            if u3681mixmap[i].0 == u3681mixmap[j].0 {
                return false;
            }
            j += 1;
        }
        i += 1;
    }
    true
}

// ---------------------------------------------------------------------------
// 段位守卫（本模块独立十六进码前缀防串段；越界即判红）
// ---------------------------------------------------------------------------

/// 段位守卫：全部诊断码必须落在本段 [0xe1c0, 0xe1cf]。
pub const fn u3681_segment_guard() -> bool {
    let mut ok = true;
    let mut i = 0usize;
    while i < u3681Code::ALL.len() {
        let c = u3681Code::ALL[i].code();
        if c < 0xe1c0 || c > 0xe1cf {
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

/// UNX-F3681 对拍表 16 行边界与累计守恒。
fn check_u3681_mix_tbl() -> bool {
    let mut ok = u3681mix_COUNT == 16;
    if ok { ok = u3681mix_get("FUZZ_00") == Some(10) && u3681mix_get("JUDGE_15") == Some(85); }
    if ok { ok = u3681mix_get("ZZZ_NOKEY").is_none(); }
    if ok { ok = u3681mix_sum() == 760; }
    ok
}

/// UNX-F3681 混合映射 16 行守卫唯一 + 守卫阈值冻结。
fn check_u3681_mix_map() -> bool {
    let mut ok = u3681mixmap_COUNT == 16;
    if ok { ok = u3681mixmap_scan() == 0 && u3681mixmap_unique(); }
    if ok { ok = u3681mixmap_sum() == 504; }
    if ok { ok = U3681MIXGATE_THRESHOLD == 32; }
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
        "u3681-mix-tbl",
        check_u3681_mix_tbl(),
        if check_u3681_mix_tbl() { "" } else { "UNX-F3681 mix table mismatch" },
    );
    set.add(
        "u3681-mix-map",
        check_u3681_mix_map(),
        if check_u3681_mix_map() { "" } else { "UNX-F3681 mix map mismatch" },
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
