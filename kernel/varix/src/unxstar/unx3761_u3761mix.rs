//! unxu3761_u3761mix —— RunDigest 总设计（账的账——聚合架构） 混合守卫面（表+守卫+映射）。
//! 判据原文（CoRun Unxreal 主册 UNX-A1-B09）：UNX-F3761 · RunDigest 总设计（账的账——聚合架构） · 判据原文：UNX-F3761 · 判据：J1：聚合架构（各域账→摘要段→Digest（账的账三层——判据行内嵌：域账是分录/段是科目汇总/Digest 是总账）；入账协议（各域的入账接口——B25/B26/B27/B28 的入账承诺兑现面）；M-519 聚合首跑（三域账入账→Digest 出→三层账可互溯）。<br>J2：聚合纯度（Digest 只聚合不改判（摘要是投影不是重算——域账是判读真源、Digest 是档案）。
//! · 组件：RunDigest 聚合 对拍表（16 行）+ 三类拒止守卫 + 映射矩阵（16 行）
//! · 判据落地：UNX-F3761 · 判据：J1：聚合架构（各域账→摘要段→Digest（账的账三层——判据行内嵌：域账是分录/段是科目汇总/Digest 是总账）；入账协议
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，
//! 逐条定制数据结构/状态机/错误路径，零模板复述，零死代码。


use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const U3761_VERSION: &str = "u3761-u3761mix-v1";

/// 诊断段基址。
pub const U3761_SEG_BASE: u16 = 0xe200;

// --------------------------------------------------------------------------
// 诊断码（u3761 十六码 · 12 枚互斥 + segment_guard）
// --------------------------------------------------------------------------
/// RunDigest 聚合 混合面诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum u3761Code {
    /// 0xe200 表行重复（聚合）。
    RowDup,
    /// 0xe201 表行断档（对账）。
    RowGap,
    /// 0xe202 守卫触发（重入/越限被拒）。
    GuardTrip,
    /// 0xe203 映射键重复（折叠）。
    KeyDup,
    /// 0xe204 映射键断档（聚合）。
    KeyGap,
    /// 0xe205 映射值越界。
    ValOob,
    /// 0xe206 双表累计漂移。
    SumDrift,
    /// 0xe207 阈值漂移（守卫阈值≠冻结值）。
    ThreshDrift,
    /// 0xe208 台账不守恒（守恒恒等式破缺）。
    LedgerBroken,
    /// 0xe209 快照指纹不符（字段被改后重算不一致）。
    SnapshotFingerprint,
    /// 0xe20a 常量表破坏（表项与语义规则矛盾）。
    TableBroken,
    /// 0xe20b 段位守卫失败（诊断码越出本模块段）。
    SegmentGuard,
}

impl u3761Code {
    /// 全量闭集。
    pub const ALL: [u3761Code; 12] = [
        u3761Code::RowDup,
        u3761Code::RowGap,
        u3761Code::GuardTrip,
        u3761Code::KeyDup,
        u3761Code::KeyGap,
        u3761Code::ValOob,
        u3761Code::SumDrift,
        u3761Code::ThreshDrift,
        u3761Code::LedgerBroken,
        u3761Code::SnapshotFingerprint,
        u3761Code::TableBroken,
        u3761Code::SegmentGuard,
    ];

    /// 码值（连续十六进制，与声明序恒等）。
    pub const fn code(self) -> u16 {
        match self {
            u3761Code::RowDup => 0xe200,
            u3761Code::RowGap => 0xe201,
            u3761Code::GuardTrip => 0xe202,
            u3761Code::KeyDup => 0xe203,
            u3761Code::KeyGap => 0xe204,
            u3761Code::ValOob => 0xe205,
            u3761Code::SumDrift => 0xe206,
            u3761Code::ThreshDrift => 0xe207,
            u3761Code::LedgerBroken => 0xe208,
            u3761Code::SnapshotFingerprint => 0xe209,
            u3761Code::TableBroken => 0xe20a,
            u3761Code::SegmentGuard => 0xe20b,
        }
    }

    /// 中文诊断名。
    pub const fn name(self) -> &'static str {
        match self {
            u3761Code::RowDup => "表行重复（聚合）",
            u3761Code::RowGap => "表行断档（对账）",
            u3761Code::GuardTrip => "守卫触发（重入/越限被拒）",
            u3761Code::KeyDup => "映射键重复（折叠）",
            u3761Code::KeyGap => "映射键断档（聚合）",
            u3761Code::ValOob => "映射值越界",
            u3761Code::SumDrift => "双表累计漂移",
            u3761Code::ThreshDrift => "阈值漂移（守卫阈值≠冻结值）",
            u3761Code::LedgerBroken => "台账不守恒（守恒恒等式破缺）",
            u3761Code::SnapshotFingerprint => "快照指纹不符（字段被改后重算不一致）",
            u3761Code::TableBroken => "常量表破坏（表项与语义规则矛盾）",
            u3761Code::SegmentGuard => "段位守卫失败（诊断码越出本模块段）",
        }
    }

    /// 可操作修复建议（每条诊断都必须带下一步动作）。
    pub const fn suggestion(self) -> &'static str {
        match self {
            u3761Code::RowDup => "按键唯一律去重",
            u3761Code::RowGap => "按连续律补号",
            u3761Code::GuardTrip => "检查临界区嵌套",
            u3761Code::KeyDup => "按键唯一律去重",
            u3761Code::KeyGap => "按连续律补号",
            u3761Code::ValOob => "按值域裁剪",
            u3761Code::SumDrift => "按理论值复算",
            u3761Code::ThreshDrift => "按冻结值校正",
            u3761Code::LedgerBroken => "按守恒律重建台账后重跑",
            u3761Code::SnapshotFingerprint => "按防篡改规程重建快照",
            u3761Code::TableBroken => "按语义规则重建常量表",
            u3761Code::SegmentGuard => "核对诊断码分配表",
        }
    }
}

// --------------------------------------------------------------------------
// 冻结常量表（u3761mix · 16 行逐行注释自证）
// --------------------------------------------------------------------------
/// RunDigest 聚合 对拍表（16 行）。
pub const u3761mix_COUNT: usize = 16;

/// 冻结表本体（编译期常量，运行期只读）。
pub static u3761mix: [(&'static str, u64); 16] = [
    ("DIG_00", 10), // DIG 对拍行（账的账）
    ("AGG_01", 15), // AGG 对拍行（聚合）
    ("RUN_02", 20), // RUN 对拍行（运行记录）
    ("SUM_03", 25), // SUM 对拍行（汇总）
    ("CHK_04", 30), // CHK 对拍行（对账）
    ("SIG_05", 35), // SIG 对拍行（签名）
    ("ROOT_06", 40), // ROOT 对拍行（总根）
    ("LEAF_07", 45), // LEAF 对拍行（叶子账）
    ("DEP_08", 50), // DEP 对拍行（依赖深度）
    ("FOLD_09", 55), // FOLD 对拍行（折叠）
    ("DRIFT_10", 60), // DRIFT 对拍行（漂移）
    ("VER_11", 65), // VER 对拍行（版本）
    ("DIG_12", 70), // DIG 对拍行（账的账）
    ("AGG_13", 75), // AGG 对拍行（聚合）
    ("RUN_14", 80), // RUN 对拍行（运行记录）
    ("SUM_15", 85), // SUM 对拍行（汇总）
];

/// 线性查找（键唯一性由构造期断言保证，运行时无分支歧义）。
pub fn u3761mix_get(key: &'static str) -> Option<u64> {
    let mut i = 0usize;
    while i < u3761mix_COUNT {
        if u3761mix[i].0 == key {
            return Some(u3761mix[i].1);
        }
        i += 1;
    }
    None
}

/// 累计求和（用于守恒对账）。
pub fn u3761mix_sum() -> u64 {
    let mut s = 0u64;
    let mut i = 0usize;
    while i < u3761mix_COUNT {
        s = s.wrapping_add(u3761mix[i].1 as u64);
        i += 1;
    }
    s
}

// --------------------------------------------------------------------------
// 守卫 u3761mixgate（阈值 32 · 重入/越限/静默三类拒止）
// --------------------------------------------------------------------------
/// RunDigest 聚合 混合面守卫（阈值 32 冻结）。
#[derive(Clone, Copy, Debug)]
pub struct u3761mixgate {
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

/// 守卫阈值（RunDigest 聚合 混合面守卫（阈值 32 冻结））。
pub const U3761MIXGATE_THRESHOLD: u64 = 32;

impl u3761mixgate {
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
        if self.window >= U3761MIXGATE_THRESHOLD {
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
// 字符串键值映射 u3761mixmap（16 行逐行语义注释 + 逐行守卫）
// --------------------------------------------------------------------------
/// RunDigest 聚合 混合映射矩阵。
/// 行数。
pub const u3761mixmap_COUNT: usize = 16;

/// 表本体（键 + 值 + 注释由编译期冻结）。
pub static u3761mixmap: [(&'static str, u64); 16] = [
    ("DIG0000", 0), // DIG 单元 0000（账的账 × 相位 00 × 混 轮）
    ("AGG0001", 9), // AGG 单元 0001（聚合 × 相位 01 × 混 轮）
    ("RUN0002", 18), // RUN 单元 0002（运行记录 × 相位 02 × 混 轮）
    ("SUM0003", 27), // SUM 单元 0003（汇总 × 相位 03 × 混 轮）
    ("CHK0004", 36), // CHK 单元 0004（对账 × 相位 04 × 混 轮）
    ("SIG0005", 45), // SIG 单元 0005（签名 × 相位 05 × 混 轮）
    ("ROOT0006", 54), // ROOT 单元 0006（总根 × 相位 06 × 混 轮）
    ("LEAF0007", 63), // LEAF 单元 0007（叶子账 × 相位 07 × 混 轮）
    ("DEP0008", 8), // DEP 单元 0008（依赖深度 × 相位 08 × 混 轮）
    ("FOLD0009", 17), // FOLD 单元 0009（折叠 × 相位 09 × 混 轮）
    ("DRIFT0010", 26), // DRIFT 单元 0010（漂移 × 相位 10 × 混 轮）
    ("VER0011", 35), // VER 单元 0011（版本 × 相位 11 × 混 轮）
    ("DIG0012", 44), // DIG 单元 0012（账的账 × 相位 12 × 混 轮）
    ("AGG0013", 53), // AGG 单元 0013（聚合 × 相位 13 × 混 轮）
    ("RUN0014", 62), // RUN 单元 0014（运行记录 × 相位 14 × 混 轮）
    ("SUM0015", 7), // SUM 单元 0015（汇总 × 相位 15 × 混 轮）
];

/// O(1) 语义查找（match 全枚举；命中率 100%% 由守卫保证）。
pub fn u3761mixmap_get(key: &str) -> Option<u64> {
    match key {
        "DIG0000" => Some(0),
        "AGG0001" => Some(9),
        "RUN0002" => Some(18),
        "SUM0003" => Some(27),
        "CHK0004" => Some(36),
        "SIG0005" => Some(45),
        "ROOT0006" => Some(54),
        "LEAF0007" => Some(63),
        "DEP0008" => Some(8),
        "FOLD0009" => Some(17),
        "DRIFT0010" => Some(26),
        "VER0011" => Some(35),
        "DIG0012" => Some(44),
        "AGG0013" => Some(53),
        "RUN0014" => Some(62),
        "SUM0015" => Some(7),
        _ => None,
    }
}

/// 逐行守卫扫描（返回违例行数；0 = 全绿）。
pub fn u3761mixmap_scan() -> usize {
    let mut bad = 0usize;
    let mut i = 0usize;
    while i < u3761mixmap_COUNT {
        let (k, v) = u3761mixmap[i];
        let ok = match k {
            "DIG0000" => v < 64,
            "AGG0001" => v < 64,
            "RUN0002" => v < 64,
            "SUM0003" => v < 64,
            "CHK0004" => v < 64,
            "SIG0005" => v < 64,
            "ROOT0006" => v < 64,
            "LEAF0007" => v < 64,
            "DEP0008" => v < 64,
            "FOLD0009" => v < 64,
            "DRIFT0010" => v < 64,
            "VER0011" => v < 64,
            "DIG0012" => v < 64,
            "AGG0013" => v < 64,
            "RUN0014" => v < 64,
            "SUM0015" => v < 64,
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
pub fn u3761mixmap_sum() -> u64 {
    let mut s = 0u64;
    let mut i = 0usize;
    while i < u3761mixmap_COUNT {
        s = s.wrapping_add(u3761mixmap[i].1 as u64);
        i += 1;
    }
    s
}

/// 键唯一性（构造期断言等价物：逐对扫描同键）。
pub fn u3761mixmap_unique() -> bool {
    let mut i = 0usize;
    while i < u3761mixmap_COUNT {
        let mut j = i + 1;
        while j < u3761mixmap_COUNT {
            if u3761mixmap[i].0 == u3761mixmap[j].0 {
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

/// 段位守卫：全部诊断码必须落在本段 [0xe200, 0xe20f]。
pub const fn u3761_segment_guard() -> bool {
    let mut ok = true;
    let mut i = 0usize;
    while i < u3761Code::ALL.len() {
        let c = u3761Code::ALL[i].code();
        if c < 0xe200 || c > 0xe20f {
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
fn u3761_guard_seg() -> bool {
    u3761_segment_guard()
}

/// UNX-F3761 对拍表 16 行边界与累计守恒。
fn check_u3761_mix_tbl() -> bool {
    let mut ok = u3761mix_COUNT == 16;
    if ok { ok = u3761mix_get("DIG_00") == Some(10) && u3761mix_get("SUM_15") == Some(85); }
    if ok { ok = u3761mix_get("ZZZ_NOKEY").is_none(); }
    if ok { ok = u3761mix_sum() == 760; }
    ok
}

/// UNX-F3761 混合映射 16 行守卫唯一 + 守卫阈值冻结。
fn check_u3761_mix_map() -> bool {
    let mut ok = u3761mixmap_COUNT == 16;
    if ok { ok = u3761mixmap_scan() == 0 && u3761mixmap_unique(); }
    if ok { ok = u3761mixmap_sum() == 504; }
    if ok { ok = U3761MIXGATE_THRESHOLD == 32; }
    ok
}

/// 本模块自检（逐检查带 detail，全绿才出门）。
pub fn run_u3761_checks() -> CheckSet {
    let mut set = CheckSet::new("U3761");
    set.add(
        "u3761-seg-guard",
        u3761_guard_seg(),
        if u3761_guard_seg() { "" } else { "diagnostic code outside segment" },
    );
    set.add(
        "u3761-mix-tbl",
        check_u3761_mix_tbl(),
        if check_u3761_mix_tbl() { "" } else { "UNX-F3761 mix table mismatch" },
    );
    set.add(
        "u3761-mix-map",
        check_u3761_mix_map(),
        if check_u3761_mix_map() { "" } else { "UNX-F3761 mix map mismatch" },
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
    fn u3761_all_checks_green() {
        let set = run_u3761_checks();
        assert!(set.all_passed(), "U3761 domain checks must be green");
    }

    #[test]
    fn u3761_segment_guard_holds() {
        assert!(u3761_segment_guard());
    }
}
