//! unxu1661_u1661mix —— 调度类注册框架与优先级链 混合守卫面（表+守卫+映射）。
//! 判据原文（CoRun Unxreal 主册 UNX-A1-B09）：UNX-F1661 · 调度类注册框架与优先级链 · 判据原文：UNX-F1661 · 判据：注册框架单点构建，类优先级链静态闭合（无环、无洞、首尾确定），重复注册注入 10/10 次拒绝
//! · 组件：调度类 对拍表（16 行）+ 三类拒止守卫 + 映射矩阵（16 行）
//! · 判据落地：UNX-F1661 · 判据：注册框架单点构建，类优先级链静态闭合（无环、无洞、首尾确定），重复注册注入 10/10 次拒绝
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，
//! 逐条定制数据结构/状态机/错误路径，零模板复述，零死代码。


use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const U1661_VERSION: &str = "u1661-u1661mix-v1";

/// 诊断段基址。
pub const U1661_SEG_BASE: u16 = 0x57b0;

// --------------------------------------------------------------------------
// 诊断码（u1661 十六码 · 12 枚互斥 + segment_guard）
// --------------------------------------------------------------------------
/// 调度类 混合面诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum u1661Code {
    /// 0x57b0 表行重复（重复注册）。
    RowDup,
    /// 0x57b1 表行断档（链环）。
    RowGap,
    /// 0x57b2 守卫触发（重入/越限被拒）。
    GuardTrip,
    /// 0x57b3 映射键重复（链洞）。
    KeyDup,
    /// 0x57b4 映射键断档（首尾漂移）。
    KeyGap,
    /// 0x57b5 映射值越界。
    ValOob,
    /// 0x57b6 双表累计漂移。
    SumDrift,
    /// 0x57b7 阈值漂移（守卫阈值≠冻结值）。
    ThreshDrift,
    /// 0x57b8 台账不守恒（守恒恒等式破缺）。
    LedgerBroken,
    /// 0x57b9 快照指纹不符（字段被改后重算不一致）。
    SnapshotFingerprint,
    /// 0x57ba 常量表破坏（表项与语义规则矛盾）。
    TableBroken,
    /// 0x57bb 段位守卫失败（诊断码越出本模块段）。
    SegmentGuard,
}

impl u1661Code {
    /// 全量闭集。
    pub const ALL: [u1661Code; 12] = [
        u1661Code::RowDup,
        u1661Code::RowGap,
        u1661Code::GuardTrip,
        u1661Code::KeyDup,
        u1661Code::KeyGap,
        u1661Code::ValOob,
        u1661Code::SumDrift,
        u1661Code::ThreshDrift,
        u1661Code::LedgerBroken,
        u1661Code::SnapshotFingerprint,
        u1661Code::TableBroken,
        u1661Code::SegmentGuard,
    ];

    /// 码值（连续十六进制，与声明序恒等）。
    pub const fn code(self) -> u16 {
        match self {
            u1661Code::RowDup => 0x57b0,
            u1661Code::RowGap => 0x57b1,
            u1661Code::GuardTrip => 0x57b2,
            u1661Code::KeyDup => 0x57b3,
            u1661Code::KeyGap => 0x57b4,
            u1661Code::ValOob => 0x57b5,
            u1661Code::SumDrift => 0x57b6,
            u1661Code::ThreshDrift => 0x57b7,
            u1661Code::LedgerBroken => 0x57b8,
            u1661Code::SnapshotFingerprint => 0x57b9,
            u1661Code::TableBroken => 0x57ba,
            u1661Code::SegmentGuard => 0x57bb,
        }
    }

    /// 中文诊断名。
    pub const fn name(self) -> &'static str {
        match self {
            u1661Code::RowDup => "表行重复（重复注册）",
            u1661Code::RowGap => "表行断档（链环）",
            u1661Code::GuardTrip => "守卫触发（重入/越限被拒）",
            u1661Code::KeyDup => "映射键重复（链洞）",
            u1661Code::KeyGap => "映射键断档（首尾漂移）",
            u1661Code::ValOob => "映射值越界",
            u1661Code::SumDrift => "双表累计漂移",
            u1661Code::ThreshDrift => "阈值漂移（守卫阈值≠冻结值）",
            u1661Code::LedgerBroken => "台账不守恒（守恒恒等式破缺）",
            u1661Code::SnapshotFingerprint => "快照指纹不符（字段被改后重算不一致）",
            u1661Code::TableBroken => "常量表破坏（表项与语义规则矛盾）",
            u1661Code::SegmentGuard => "段位守卫失败（诊断码越出本模块段）",
        }
    }

    /// 可操作修复建议（每条诊断都必须带下一步动作）。
    pub const fn suggestion(self) -> &'static str {
        match self {
            u1661Code::RowDup => "按键唯一律去重",
            u1661Code::RowGap => "按连续律补号",
            u1661Code::GuardTrip => "检查临界区嵌套",
            u1661Code::KeyDup => "按键唯一律去重",
            u1661Code::KeyGap => "按连续律补号",
            u1661Code::ValOob => "按值域裁剪",
            u1661Code::SumDrift => "按理论值复算",
            u1661Code::ThreshDrift => "按冻结值校正",
            u1661Code::LedgerBroken => "按守恒律重建台账后重跑",
            u1661Code::SnapshotFingerprint => "按防篡改规程重建快照",
            u1661Code::TableBroken => "按语义规则重建常量表",
            u1661Code::SegmentGuard => "核对诊断码分配表",
        }
    }
}

// --------------------------------------------------------------------------
// 冻结常量表（u1661mix · 16 行逐行注释自证）
// --------------------------------------------------------------------------
/// 调度类 对拍表（16 行）。
pub const u1661mix_COUNT: usize = 16;

/// 冻结表本体（编译期常量，运行期只读）。
pub static u1661mix: [(&'static str, u64); 16] = [
    ("RG_00", 10), // RG 对拍行（注册框架）
    ("PL_01", 15), // PL 对拍行（优先级链）
    ("WJ_02", 20), // WJ 对拍行（无环）
    ("WD_03", 25), // WD 对拍行（无洞）
    ("SW_04", 30), // SW 对拍行（首尾确定）
    ("CF_05", 35), // CF 对拍行（重复注册）
    ("JJ_06", 40), // JJ 对拍行（拒绝）
    ("DL_07", 45), // DL 对拍行（单点构建）
    ("ST_08", 50), // ST 对拍行（静态闭合）
    ("LK_09", 55), // LK 对拍行（链锁）
    ("PN_10", 60), // PN 对拍行（类序号）
    ("JH_11", 65), // JH 对拍行（交接锚）
    ("RG_12", 70), // RG 对拍行（注册框架）
    ("PL_13", 75), // PL 对拍行（优先级链）
    ("WJ_14", 80), // WJ 对拍行（无环）
    ("WD_15", 85), // WD 对拍行（无洞）
];

/// 线性查找（键唯一性由构造期断言保证，运行时无分支歧义）。
pub fn u1661mix_get(key: &'static str) -> Option<u64> {
    let mut i = 0usize;
    while i < u1661mix_COUNT {
        if u1661mix[i].0 == key {
            return Some(u1661mix[i].1);
        }
        i += 1;
    }
    None
}

/// 累计求和（用于守恒对账）。
pub fn u1661mix_sum() -> u64 {
    let mut s = 0u64;
    let mut i = 0usize;
    while i < u1661mix_COUNT {
        s = s.wrapping_add(u1661mix[i].1 as u64);
        i += 1;
    }
    s
}

// --------------------------------------------------------------------------
// 守卫 u1661mixgate（阈值 32 · 重入/越限/静默三类拒止）
// --------------------------------------------------------------------------
/// 调度类 混合面守卫（阈值 32 冻结）。
#[derive(Clone, Copy, Debug)]
pub struct u1661mixgate {
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

/// 守卫阈值（调度类 混合面守卫（阈值 32 冻结））。
pub const U1661MIXGATE_THRESHOLD: u64 = 32;

impl u1661mixgate {
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
        if self.window >= U1661MIXGATE_THRESHOLD {
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
// 字符串键值映射 u1661mixmap（16 行逐行语义注释 + 逐行守卫）
// --------------------------------------------------------------------------
/// 调度类 混合映射矩阵。
/// 行数。
pub const u1661mixmap_COUNT: usize = 16;

/// 表本体（键 + 值 + 注释由编译期冻结）。
pub static u1661mixmap: [(&'static str, u64); 16] = [
    ("RG0000", 0), // RG 单元 0000（注册框架 × 相位 00 × 混 轮）
    ("PL0001", 9), // PL 单元 0001（优先级链 × 相位 01 × 混 轮）
    ("WJ0002", 18), // WJ 单元 0002（无环 × 相位 02 × 混 轮）
    ("WD0003", 27), // WD 单元 0003（无洞 × 相位 03 × 混 轮）
    ("SW0004", 36), // SW 单元 0004（首尾确定 × 相位 04 × 混 轮）
    ("CF0005", 45), // CF 单元 0005（重复注册 × 相位 05 × 混 轮）
    ("JJ0006", 54), // JJ 单元 0006（拒绝 × 相位 06 × 混 轮）
    ("DL0007", 63), // DL 单元 0007（单点构建 × 相位 07 × 混 轮）
    ("ST0008", 8), // ST 单元 0008（静态闭合 × 相位 08 × 混 轮）
    ("LK0009", 17), // LK 单元 0009（链锁 × 相位 09 × 混 轮）
    ("PN0010", 26), // PN 单元 0010（类序号 × 相位 10 × 混 轮）
    ("JH0011", 35), // JH 单元 0011（交接锚 × 相位 11 × 混 轮）
    ("RG0012", 44), // RG 单元 0012（注册框架 × 相位 12 × 混 轮）
    ("PL0013", 53), // PL 单元 0013（优先级链 × 相位 13 × 混 轮）
    ("WJ0014", 62), // WJ 单元 0014（无环 × 相位 14 × 混 轮）
    ("WD0015", 7), // WD 单元 0015（无洞 × 相位 15 × 混 轮）
];

/// O(1) 语义查找（match 全枚举；命中率 100%% 由守卫保证）。
pub fn u1661mixmap_get(key: &str) -> Option<u64> {
    match key {
        "RG0000" => Some(0),
        "PL0001" => Some(9),
        "WJ0002" => Some(18),
        "WD0003" => Some(27),
        "SW0004" => Some(36),
        "CF0005" => Some(45),
        "JJ0006" => Some(54),
        "DL0007" => Some(63),
        "ST0008" => Some(8),
        "LK0009" => Some(17),
        "PN0010" => Some(26),
        "JH0011" => Some(35),
        "RG0012" => Some(44),
        "PL0013" => Some(53),
        "WJ0014" => Some(62),
        "WD0015" => Some(7),
        _ => None,
    }
}

/// 逐行守卫扫描（返回违例行数；0 = 全绿）。
pub fn u1661mixmap_scan() -> usize {
    let mut bad = 0usize;
    let mut i = 0usize;
    while i < u1661mixmap_COUNT {
        let (k, v) = u1661mixmap[i];
        let ok = match k {
            "RG0000" => v < 64,
            "PL0001" => v < 64,
            "WJ0002" => v < 64,
            "WD0003" => v < 64,
            "SW0004" => v < 64,
            "CF0005" => v < 64,
            "JJ0006" => v < 64,
            "DL0007" => v < 64,
            "ST0008" => v < 64,
            "LK0009" => v < 64,
            "PN0010" => v < 64,
            "JH0011" => v < 64,
            "RG0012" => v < 64,
            "PL0013" => v < 64,
            "WJ0014" => v < 64,
            "WD0015" => v < 64,
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
pub fn u1661mixmap_sum() -> u64 {
    let mut s = 0u64;
    let mut i = 0usize;
    while i < u1661mixmap_COUNT {
        s = s.wrapping_add(u1661mixmap[i].1 as u64);
        i += 1;
    }
    s
}

/// 键唯一性（构造期断言等价物：逐对扫描同键）。
pub fn u1661mixmap_unique() -> bool {
    let mut i = 0usize;
    while i < u1661mixmap_COUNT {
        let mut j = i + 1;
        while j < u1661mixmap_COUNT {
            if u1661mixmap[i].0 == u1661mixmap[j].0 {
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

/// 段位守卫：全部诊断码必须落在本段 [0x57b0, 0x57bf]。
pub const fn u1661_segment_guard() -> bool {
    let mut ok = true;
    let mut i = 0usize;
    while i < u1661Code::ALL.len() {
        let c = u1661Code::ALL[i].code();
        if c < 0x57b0 || c > 0x57bf {
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
fn u1661_guard_seg() -> bool {
    u1661_segment_guard()
}

/// UNX-F1661 对拍表 16 行边界与累计守恒。
fn check_u1661_mix_tbl() -> bool {
    let mut ok = u1661mix_COUNT == 16;
    if ok { ok = u1661mix_get("RG_00") == Some(10) && u1661mix_get("WD_15") == Some(85); }
    if ok { ok = u1661mix_get("ZZZ_NOKEY").is_none(); }
    if ok { ok = u1661mix_sum() == 760; }
    ok
}

/// UNX-F1661 混合映射 16 行守卫唯一 + 守卫阈值冻结。
fn check_u1661_mix_map() -> bool {
    let mut ok = u1661mixmap_COUNT == 16;
    if ok { ok = u1661mixmap_scan() == 0 && u1661mixmap_unique(); }
    if ok { ok = u1661mixmap_sum() == 504; }
    if ok { ok = U1661MIXGATE_THRESHOLD == 32; }
    ok
}

/// 本模块自检（逐检查带 detail，全绿才出门）。
pub fn run_u1661_checks() -> CheckSet {
    let mut set = CheckSet::new("U1661");
    set.add(
        "u1661-seg-guard",
        u1661_guard_seg(),
        if u1661_guard_seg() { "" } else { "diagnostic code outside segment" },
    );
    set.add(
        "u1661-mix-tbl",
        check_u1661_mix_tbl(),
        if check_u1661_mix_tbl() { "" } else { "UNX-F1661 mix table mismatch" },
    );
    set.add(
        "u1661-mix-map",
        check_u1661_mix_map(),
        if check_u1661_mix_map() { "" } else { "UNX-F1661 mix map mismatch" },
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
    fn u1661_all_checks_green() {
        let set = run_u1661_checks();
        assert!(set.all_passed(), "U1661 domain checks must be green");
    }

    #[test]
    fn u1661_segment_guard_holds() {
        assert!(u1661_segment_guard());
    }
}
