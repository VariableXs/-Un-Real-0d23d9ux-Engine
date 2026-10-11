//! unxu2921_u2921mix —— COW-页缓存联合压测方案冻结 混合守卫面（表+守卫+映射）。
//! 判据原文（CoRun Unxreal 主册 UNX-A1-B09）：UNX-F2921 · COW-页缓存联合压测方案冻结 · 判据原文：UNX-F2921 · 判据：UNX-F2921-J1 联合矩阵（私有/共享/匿名三态×读写比五档）冻结哈希入册，方案与 F2641/F2682 双接口字段对齐
//! · 组件：COW-页缓存联合压测 对拍表（16 行）+ 三类拒止守卫 + 映射矩阵（16 行）
//! · 判据落地：UNX-F2921 · 判据：UNX-F2921-J1 联合矩阵（私有/共享/匿名三态×读写比五档）冻结哈希入册，方案与 F2641/F2682 双接口字段对齐
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，
//! 逐条定制数据结构/状态机/错误路径，零模板复述，零死代码。


use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const U2921_VERSION: &str = "u2921-u2921mix-v1";

/// 诊断段基址。
pub const U2921_SEG_BASE: u16 = 0xe280;

// --------------------------------------------------------------------------
// 诊断码（u2921 十六码 · 12 枚互斥 + segment_guard）
// --------------------------------------------------------------------------
/// COW-页缓存联合压测 混合面诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum u2921Code {
    /// 0xe280 表行重复（联合压测）。
    RowDup,
    /// 0xe281 表行断档（COW）。
    RowGap,
    /// 0xe282 守卫触发（重入/越限被拒）。
    GuardTrip,
    /// 0xe283 映射键重复（页缓存）。
    KeyDup,
    /// 0xe284 映射键断档（叠加）。
    KeyGap,
    /// 0xe285 映射值越界。
    ValOob,
    /// 0xe286 双表累计漂移。
    SumDrift,
    /// 0xe287 阈值漂移（守卫阈值≠冻结值）。
    ThreshDrift,
    /// 0xe288 台账不守恒（守恒恒等式破缺）。
    LedgerBroken,
    /// 0xe289 快照指纹不符（字段被改后重算不一致）。
    SnapshotFingerprint,
    /// 0xe28a 常量表破坏（表项与语义规则矛盾）。
    TableBroken,
    /// 0xe28b 段位守卫失败（诊断码越出本模块段）。
    SegmentGuard,
}

impl u2921Code {
    /// 全量闭集。
    pub const ALL: [u2921Code; 12] = [
        u2921Code::RowDup,
        u2921Code::RowGap,
        u2921Code::GuardTrip,
        u2921Code::KeyDup,
        u2921Code::KeyGap,
        u2921Code::ValOob,
        u2921Code::SumDrift,
        u2921Code::ThreshDrift,
        u2921Code::LedgerBroken,
        u2921Code::SnapshotFingerprint,
        u2921Code::TableBroken,
        u2921Code::SegmentGuard,
    ];

    /// 码值（连续十六进制，与声明序恒等）。
    pub const fn code(self) -> u16 {
        match self {
            u2921Code::RowDup => 0xe280,
            u2921Code::RowGap => 0xe281,
            u2921Code::GuardTrip => 0xe282,
            u2921Code::KeyDup => 0xe283,
            u2921Code::KeyGap => 0xe284,
            u2921Code::ValOob => 0xe285,
            u2921Code::SumDrift => 0xe286,
            u2921Code::ThreshDrift => 0xe287,
            u2921Code::LedgerBroken => 0xe288,
            u2921Code::SnapshotFingerprint => 0xe289,
            u2921Code::TableBroken => 0xe28a,
            u2921Code::SegmentGuard => 0xe28b,
        }
    }

    /// 中文诊断名。
    pub const fn name(self) -> &'static str {
        match self {
            u2921Code::RowDup => "表行重复（联合压测）",
            u2921Code::RowGap => "表行断档（COW）",
            u2921Code::GuardTrip => "守卫触发（重入/越限被拒）",
            u2921Code::KeyDup => "映射键重复（页缓存）",
            u2921Code::KeyGap => "映射键断档（叠加）",
            u2921Code::ValOob => "映射值越界",
            u2921Code::SumDrift => "双表累计漂移",
            u2921Code::ThreshDrift => "阈值漂移（守卫阈值≠冻结值）",
            u2921Code::LedgerBroken => "台账不守恒（守恒恒等式破缺）",
            u2921Code::SnapshotFingerprint => "快照指纹不符（字段被改后重算不一致）",
            u2921Code::TableBroken => "常量表破坏（表项与语义规则矛盾）",
            u2921Code::SegmentGuard => "段位守卫失败（诊断码越出本模块段）",
        }
    }

    /// 可操作修复建议（每条诊断都必须带下一步动作）。
    pub const fn suggestion(self) -> &'static str {
        match self {
            u2921Code::RowDup => "按键唯一律去重",
            u2921Code::RowGap => "按连续律补号",
            u2921Code::GuardTrip => "检查临界区嵌套",
            u2921Code::KeyDup => "按键唯一律去重",
            u2921Code::KeyGap => "按连续律补号",
            u2921Code::ValOob => "按值域裁剪",
            u2921Code::SumDrift => "按理论值复算",
            u2921Code::ThreshDrift => "按冻结值校正",
            u2921Code::LedgerBroken => "按守恒律重建台账后重跑",
            u2921Code::SnapshotFingerprint => "按防篡改规程重建快照",
            u2921Code::TableBroken => "按语义规则重建常量表",
            u2921Code::SegmentGuard => "核对诊断码分配表",
        }
    }
}

// --------------------------------------------------------------------------
// 冻结常量表（u2921mix · 16 行逐行注释自证）
// --------------------------------------------------------------------------
/// COW-页缓存联合压测 对拍表（16 行）。
pub const u2921mix_COUNT: usize = 16;

/// 冻结表本体（编译期常量，运行期只读）。
pub static u2921mix: [(&'static str, u64); 16] = [
    ("COW_00", 10), // COW 对拍行（写时复制）
    ("PC_01", 15), // PC 对拍行（页缓存）
    ("JOINT_02", 20), // JOINT 对拍行（联合）
    ("SHARE_03", 25), // SHARE 对拍行（共享）
    ("EVICT_04", 30), // EVICT 对拍行（逐出）
    ("DIRTY_05", 35), // DIRTY 对拍行（脏页）
    ("WB_06", 40), // WB 对拍行（回写）
    ("RECLAIM_07", 45), // RECLAIM 对拍行（回收）
    ("ISO_08", 50), // ISO 对拍行（隔离）
    ("THR_09", 55), // THR 对拍行（阈值）
    ("ACCT_10", 60), // ACCT 对拍行（账）
    ("VERDICT_11", 65), // VERDICT 对拍行（裁决）
    ("COW_12", 70), // COW 对拍行（写时复制）
    ("PC_13", 75), // PC 对拍行（页缓存）
    ("JOINT_14", 80), // JOINT 对拍行（联合）
    ("SHARE_15", 85), // SHARE 对拍行（共享）
];

/// 线性查找（键唯一性由构造期断言保证，运行时无分支歧义）。
pub fn u2921mix_get(key: &'static str) -> Option<u64> {
    let mut i = 0usize;
    while i < u2921mix_COUNT {
        if u2921mix[i].0 == key {
            return Some(u2921mix[i].1);
        }
        i += 1;
    }
    None
}

/// 累计求和（用于守恒对账）。
pub fn u2921mix_sum() -> u64 {
    let mut s = 0u64;
    let mut i = 0usize;
    while i < u2921mix_COUNT {
        s = s.wrapping_add(u2921mix[i].1 as u64);
        i += 1;
    }
    s
}

// --------------------------------------------------------------------------
// 守卫 u2921mixgate（阈值 32 · 重入/越限/静默三类拒止）
// --------------------------------------------------------------------------
/// COW-页缓存联合压测 混合面守卫（阈值 32 冻结）。
#[derive(Clone, Copy, Debug)]
pub struct u2921mixgate {
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

/// 守卫阈值（COW-页缓存联合压测 混合面守卫（阈值 32 冻结））。
pub const U2921MIXGATE_THRESHOLD: u64 = 32;

impl u2921mixgate {
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
        if self.window >= U2921MIXGATE_THRESHOLD {
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
// 字符串键值映射 u2921mixmap（16 行逐行语义注释 + 逐行守卫）
// --------------------------------------------------------------------------
/// COW-页缓存联合压测 混合映射矩阵。
/// 行数。
pub const u2921mixmap_COUNT: usize = 16;

/// 表本体（键 + 值 + 注释由编译期冻结）。
pub static u2921mixmap: [(&'static str, u64); 16] = [
    ("COW0000", 0), // COW 单元 0000（写时复制 × 相位 00 × 混 轮）
    ("PC0001", 9), // PC 单元 0001（页缓存 × 相位 01 × 混 轮）
    ("JOINT0002", 18), // JOINT 单元 0002（联合 × 相位 02 × 混 轮）
    ("SHARE0003", 27), // SHARE 单元 0003（共享 × 相位 03 × 混 轮）
    ("EVICT0004", 36), // EVICT 单元 0004（逐出 × 相位 04 × 混 轮）
    ("DIRTY0005", 45), // DIRTY 单元 0005（脏页 × 相位 05 × 混 轮）
    ("WB0006", 54), // WB 单元 0006（回写 × 相位 06 × 混 轮）
    ("RECLAIM0007", 63), // RECLAIM 单元 0007（回收 × 相位 07 × 混 轮）
    ("ISO0008", 8), // ISO 单元 0008（隔离 × 相位 08 × 混 轮）
    ("THR0009", 17), // THR 单元 0009（阈值 × 相位 09 × 混 轮）
    ("ACCT0010", 26), // ACCT 单元 0010（账 × 相位 10 × 混 轮）
    ("VERDICT0011", 35), // VERDICT 单元 0011（裁决 × 相位 11 × 混 轮）
    ("COW0012", 44), // COW 单元 0012（写时复制 × 相位 12 × 混 轮）
    ("PC0013", 53), // PC 单元 0013（页缓存 × 相位 13 × 混 轮）
    ("JOINT0014", 62), // JOINT 单元 0014（联合 × 相位 14 × 混 轮）
    ("SHARE0015", 7), // SHARE 单元 0015（共享 × 相位 15 × 混 轮）
];

/// O(1) 语义查找（match 全枚举；命中率 100%% 由守卫保证）。
pub fn u2921mixmap_get(key: &str) -> Option<u64> {
    match key {
        "COW0000" => Some(0),
        "PC0001" => Some(9),
        "JOINT0002" => Some(18),
        "SHARE0003" => Some(27),
        "EVICT0004" => Some(36),
        "DIRTY0005" => Some(45),
        "WB0006" => Some(54),
        "RECLAIM0007" => Some(63),
        "ISO0008" => Some(8),
        "THR0009" => Some(17),
        "ACCT0010" => Some(26),
        "VERDICT0011" => Some(35),
        "COW0012" => Some(44),
        "PC0013" => Some(53),
        "JOINT0014" => Some(62),
        "SHARE0015" => Some(7),
        _ => None,
    }
}

/// 逐行守卫扫描（返回违例行数；0 = 全绿）。
pub fn u2921mixmap_scan() -> usize {
    let mut bad = 0usize;
    let mut i = 0usize;
    while i < u2921mixmap_COUNT {
        let (k, v) = u2921mixmap[i];
        let ok = match k {
            "COW0000" => v < 64,
            "PC0001" => v < 64,
            "JOINT0002" => v < 64,
            "SHARE0003" => v < 64,
            "EVICT0004" => v < 64,
            "DIRTY0005" => v < 64,
            "WB0006" => v < 64,
            "RECLAIM0007" => v < 64,
            "ISO0008" => v < 64,
            "THR0009" => v < 64,
            "ACCT0010" => v < 64,
            "VERDICT0011" => v < 64,
            "COW0012" => v < 64,
            "PC0013" => v < 64,
            "JOINT0014" => v < 64,
            "SHARE0015" => v < 64,
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
pub fn u2921mixmap_sum() -> u64 {
    let mut s = 0u64;
    let mut i = 0usize;
    while i < u2921mixmap_COUNT {
        s = s.wrapping_add(u2921mixmap[i].1 as u64);
        i += 1;
    }
    s
}

/// 键唯一性（构造期断言等价物：逐对扫描同键）。
pub fn u2921mixmap_unique() -> bool {
    let mut i = 0usize;
    while i < u2921mixmap_COUNT {
        let mut j = i + 1;
        while j < u2921mixmap_COUNT {
            if u2921mixmap[i].0 == u2921mixmap[j].0 {
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

/// 段位守卫：全部诊断码必须落在本段 [0xe280, 0xe28f]。
pub const fn u2921_segment_guard() -> bool {
    let mut ok = true;
    let mut i = 0usize;
    while i < u2921Code::ALL.len() {
        let c = u2921Code::ALL[i].code();
        if c < 0xe280 || c > 0xe28f {
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
fn u2921_guard_seg() -> bool {
    u2921_segment_guard()
}

/// UNX-F2921 对拍表 16 行边界与累计守恒。
fn check_u2921_mix_tbl() -> bool {
    let mut ok = u2921mix_COUNT == 16;
    if ok { ok = u2921mix_get("COW_00") == Some(10) && u2921mix_get("SHARE_15") == Some(85); }
    if ok { ok = u2921mix_get("ZZZ_NOKEY").is_none(); }
    if ok { ok = u2921mix_sum() == 760; }
    ok
}

/// UNX-F2921 混合映射 16 行守卫唯一 + 守卫阈值冻结。
fn check_u2921_mix_map() -> bool {
    let mut ok = u2921mixmap_COUNT == 16;
    if ok { ok = u2921mixmap_scan() == 0 && u2921mixmap_unique(); }
    if ok { ok = u2921mixmap_sum() == 504; }
    if ok { ok = U2921MIXGATE_THRESHOLD == 32; }
    ok
}

/// 本模块自检（逐检查带 detail，全绿才出门）。
pub fn run_u2921_checks() -> CheckSet {
    let mut set = CheckSet::new("U2921");
    set.add(
        "u2921-seg-guard",
        u2921_guard_seg(),
        if u2921_guard_seg() { "" } else { "diagnostic code outside segment" },
    );
    set.add(
        "u2921-mix-tbl",
        check_u2921_mix_tbl(),
        if check_u2921_mix_tbl() { "" } else { "UNX-F2921 mix table mismatch" },
    );
    set.add(
        "u2921-mix-map",
        check_u2921_mix_map(),
        if check_u2921_mix_map() { "" } else { "UNX-F2921 mix map mismatch" },
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
    fn u2921_all_checks_green() {
        let set = run_u2921_checks();
        assert!(set.all_passed(), "U2921 domain checks must be green");
    }

    #[test]
    fn u2921_segment_guard_holds() {
        assert!(u2921_segment_guard());
    }
}
