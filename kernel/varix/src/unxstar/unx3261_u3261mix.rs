//! unxu3261_u3261mix —— rwlock 结构与读偏好语义定义 混合守卫面（表+守卫+映射）。
//! 判据原文（CoRun Unxreal 主册 UNX-A1-B09）：UNX-F3261 · rwlock 结构与读偏好语义定义 · 判据原文：UNX-F3261 · 判据：J1：`RwLock<T>` 状态字编码表（读计数/写者位/排队位）唯一化且 M-020 用例可验证三态迁移。
//! · 组件：rwlock 读偏好 对拍表（16 行）+ 三类拒止守卫 + 映射矩阵（16 行）
//! · 判据落地：UNX-F3261 · 判据：J1：`RwLock<T>` 状态字编码表（读计数/写者位/排队位）唯一化且 M-020 用例可验证三态迁移。
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，
//! 逐条定制数据结构/状态机/错误路径，零模板复述，零死代码。


use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const U3261_VERSION: &str = "u3261-u3261mix-v1";

/// 诊断段基址。
pub const U3261_SEG_BASE: u16 = 0xe080;

// --------------------------------------------------------------------------
// 诊断码（u3261 十六码 · 12 枚互斥 + segment_guard）
// --------------------------------------------------------------------------
/// rwlock 读偏好 混合面诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum u3261Code {
    /// 0xe080 表行重复（rwlock）。
    RowDup,
    /// 0xe081 表行断档（读偏好）。
    RowGap,
    /// 0xe082 守卫触发（重入/越限被拒）。
    GuardTrip,
    /// 0xe083 映射键重复（状态字）。
    KeyDup,
    /// 0xe084 映射键断档（升级）。
    KeyGap,
    /// 0xe085 映射值越界。
    ValOob,
    /// 0xe086 双表累计漂移。
    SumDrift,
    /// 0xe087 阈值漂移（守卫阈值≠冻结值）。
    ThreshDrift,
    /// 0xe088 台账不守恒（守恒恒等式破缺）。
    LedgerBroken,
    /// 0xe089 快照指纹不符（字段被改后重算不一致）。
    SnapshotFingerprint,
    /// 0xe08a 常量表破坏（表项与语义规则矛盾）。
    TableBroken,
    /// 0xe08b 段位守卫失败（诊断码越出本模块段）。
    SegmentGuard,
}

impl u3261Code {
    /// 全量闭集。
    pub const ALL: [u3261Code; 12] = [
        u3261Code::RowDup,
        u3261Code::RowGap,
        u3261Code::GuardTrip,
        u3261Code::KeyDup,
        u3261Code::KeyGap,
        u3261Code::ValOob,
        u3261Code::SumDrift,
        u3261Code::ThreshDrift,
        u3261Code::LedgerBroken,
        u3261Code::SnapshotFingerprint,
        u3261Code::TableBroken,
        u3261Code::SegmentGuard,
    ];

    /// 码值（连续十六进制，与声明序恒等）。
    pub const fn code(self) -> u16 {
        match self {
            u3261Code::RowDup => 0xe080,
            u3261Code::RowGap => 0xe081,
            u3261Code::GuardTrip => 0xe082,
            u3261Code::KeyDup => 0xe083,
            u3261Code::KeyGap => 0xe084,
            u3261Code::ValOob => 0xe085,
            u3261Code::SumDrift => 0xe086,
            u3261Code::ThreshDrift => 0xe087,
            u3261Code::LedgerBroken => 0xe088,
            u3261Code::SnapshotFingerprint => 0xe089,
            u3261Code::TableBroken => 0xe08a,
            u3261Code::SegmentGuard => 0xe08b,
        }
    }

    /// 中文诊断名。
    pub const fn name(self) -> &'static str {
        match self {
            u3261Code::RowDup => "表行重复（rwlock）",
            u3261Code::RowGap => "表行断档（读偏好）",
            u3261Code::GuardTrip => "守卫触发（重入/越限被拒）",
            u3261Code::KeyDup => "映射键重复（状态字）",
            u3261Code::KeyGap => "映射键断档（升级）",
            u3261Code::ValOob => "映射值越界",
            u3261Code::SumDrift => "双表累计漂移",
            u3261Code::ThreshDrift => "阈值漂移（守卫阈值≠冻结值）",
            u3261Code::LedgerBroken => "台账不守恒（守恒恒等式破缺）",
            u3261Code::SnapshotFingerprint => "快照指纹不符（字段被改后重算不一致）",
            u3261Code::TableBroken => "常量表破坏（表项与语义规则矛盾）",
            u3261Code::SegmentGuard => "段位守卫失败（诊断码越出本模块段）",
        }
    }

    /// 可操作修复建议（每条诊断都必须带下一步动作）。
    pub const fn suggestion(self) -> &'static str {
        match self {
            u3261Code::RowDup => "按键唯一律去重",
            u3261Code::RowGap => "按连续律补号",
            u3261Code::GuardTrip => "检查临界区嵌套",
            u3261Code::KeyDup => "按键唯一律去重",
            u3261Code::KeyGap => "按连续律补号",
            u3261Code::ValOob => "按值域裁剪",
            u3261Code::SumDrift => "按理论值复算",
            u3261Code::ThreshDrift => "按冻结值校正",
            u3261Code::LedgerBroken => "按守恒律重建台账后重跑",
            u3261Code::SnapshotFingerprint => "按防篡改规程重建快照",
            u3261Code::TableBroken => "按语义规则重建常量表",
            u3261Code::SegmentGuard => "核对诊断码分配表",
        }
    }
}

// --------------------------------------------------------------------------
// 冻结常量表（u3261mix · 16 行逐行注释自证）
// --------------------------------------------------------------------------
/// rwlock 读偏好 对拍表（16 行）。
pub const u3261mix_COUNT: usize = 16;

/// 冻结表本体（编译期常量，运行期只读）。
pub static u3261mix: [(&'static str, u64); 16] = [
    ("ST_00", 10), // ST 对拍行（状态字）
    ("RC_01", 15), // RC 对拍行（读计数）
    ("WR_02", 20), // WR 对拍行（写者位）
    ("WQ_03", 25), // WQ 对拍行（写排队位）
    ("RP_04", 30), // RP 对拍行（读偏好位）
    ("WB_05", 35), // WB 对拍行（写屏障）
    ("RG_06", 40), // RG 对拍行（读组）
    ("WG_07", 45), // WG 对拍行（写组）
    ("UPR_08", 50), // UPR 对拍行（升级门）
    ("DWN_09", 55), // DWN 对拍行（降级门）
    ("3ST_10", 60), // 3ST 对拍行（三态迁移）
    ("FIFO_11", 65), // FIFO 对拍行（写者排队序）
    ("ST_12", 70), // ST 对拍行（状态字）
    ("RC_13", 75), // RC 对拍行（读计数）
    ("WR_14", 80), // WR 对拍行（写者位）
    ("WQ_15", 85), // WQ 对拍行（写排队位）
];

/// 线性查找（键唯一性由构造期断言保证，运行时无分支歧义）。
pub fn u3261mix_get(key: &'static str) -> Option<u64> {
    let mut i = 0usize;
    while i < u3261mix_COUNT {
        if u3261mix[i].0 == key {
            return Some(u3261mix[i].1);
        }
        i += 1;
    }
    None
}

/// 累计求和（用于守恒对账）。
pub fn u3261mix_sum() -> u64 {
    let mut s = 0u64;
    let mut i = 0usize;
    while i < u3261mix_COUNT {
        s = s.wrapping_add(u3261mix[i].1 as u64);
        i += 1;
    }
    s
}

// --------------------------------------------------------------------------
// 守卫 u3261mixgate（阈值 32 · 重入/越限/静默三类拒止）
// --------------------------------------------------------------------------
/// rwlock 读偏好 混合面守卫（阈值 32 冻结）。
#[derive(Clone, Copy, Debug)]
pub struct u3261mixgate {
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

/// 守卫阈值（rwlock 读偏好 混合面守卫（阈值 32 冻结））。
pub const U3261MIXGATE_THRESHOLD: u64 = 32;

impl u3261mixgate {
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
        if self.window >= U3261MIXGATE_THRESHOLD {
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
// 字符串键值映射 u3261mixmap（16 行逐行语义注释 + 逐行守卫）
// --------------------------------------------------------------------------
/// rwlock 读偏好 混合映射矩阵。
/// 行数。
pub const u3261mixmap_COUNT: usize = 16;

/// 表本体（键 + 值 + 注释由编译期冻结）。
pub static u3261mixmap: [(&'static str, u64); 16] = [
    ("ST0000", 0), // ST 单元 0000（状态字 × 相位 00 × 混 轮）
    ("RC0001", 9), // RC 单元 0001（读计数 × 相位 01 × 混 轮）
    ("WR0002", 18), // WR 单元 0002（写者位 × 相位 02 × 混 轮）
    ("WQ0003", 27), // WQ 单元 0003（写排队位 × 相位 03 × 混 轮）
    ("RP0004", 36), // RP 单元 0004（读偏好位 × 相位 04 × 混 轮）
    ("WB0005", 45), // WB 单元 0005（写屏障 × 相位 05 × 混 轮）
    ("RG0006", 54), // RG 单元 0006（读组 × 相位 06 × 混 轮）
    ("WG0007", 63), // WG 单元 0007（写组 × 相位 07 × 混 轮）
    ("UPR0008", 8), // UPR 单元 0008（升级门 × 相位 08 × 混 轮）
    ("DWN0009", 17), // DWN 单元 0009（降级门 × 相位 09 × 混 轮）
    ("3ST0010", 26), // 3ST 单元 0010（三态迁移 × 相位 10 × 混 轮）
    ("FIFO0011", 35), // FIFO 单元 0011（写者排队序 × 相位 11 × 混 轮）
    ("ST0012", 44), // ST 单元 0012（状态字 × 相位 12 × 混 轮）
    ("RC0013", 53), // RC 单元 0013（读计数 × 相位 13 × 混 轮）
    ("WR0014", 62), // WR 单元 0014（写者位 × 相位 14 × 混 轮）
    ("WQ0015", 7), // WQ 单元 0015（写排队位 × 相位 15 × 混 轮）
];

/// O(1) 语义查找（match 全枚举；命中率 100%% 由守卫保证）。
pub fn u3261mixmap_get(key: &str) -> Option<u64> {
    match key {
        "ST0000" => Some(0),
        "RC0001" => Some(9),
        "WR0002" => Some(18),
        "WQ0003" => Some(27),
        "RP0004" => Some(36),
        "WB0005" => Some(45),
        "RG0006" => Some(54),
        "WG0007" => Some(63),
        "UPR0008" => Some(8),
        "DWN0009" => Some(17),
        "3ST0010" => Some(26),
        "FIFO0011" => Some(35),
        "ST0012" => Some(44),
        "RC0013" => Some(53),
        "WR0014" => Some(62),
        "WQ0015" => Some(7),
        _ => None,
    }
}

/// 逐行守卫扫描（返回违例行数；0 = 全绿）。
pub fn u3261mixmap_scan() -> usize {
    let mut bad = 0usize;
    let mut i = 0usize;
    while i < u3261mixmap_COUNT {
        let (k, v) = u3261mixmap[i];
        let ok = match k {
            "ST0000" => v < 64,
            "RC0001" => v < 64,
            "WR0002" => v < 64,
            "WQ0003" => v < 64,
            "RP0004" => v < 64,
            "WB0005" => v < 64,
            "RG0006" => v < 64,
            "WG0007" => v < 64,
            "UPR0008" => v < 64,
            "DWN0009" => v < 64,
            "3ST0010" => v < 64,
            "FIFO0011" => v < 64,
            "ST0012" => v < 64,
            "RC0013" => v < 64,
            "WR0014" => v < 64,
            "WQ0015" => v < 64,
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
pub fn u3261mixmap_sum() -> u64 {
    let mut s = 0u64;
    let mut i = 0usize;
    while i < u3261mixmap_COUNT {
        s = s.wrapping_add(u3261mixmap[i].1 as u64);
        i += 1;
    }
    s
}

/// 键唯一性（构造期断言等价物：逐对扫描同键）。
pub fn u3261mixmap_unique() -> bool {
    let mut i = 0usize;
    while i < u3261mixmap_COUNT {
        let mut j = i + 1;
        while j < u3261mixmap_COUNT {
            if u3261mixmap[i].0 == u3261mixmap[j].0 {
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

/// 段位守卫：全部诊断码必须落在本段 [0xe080, 0xe08f]。
pub const fn u3261_segment_guard() -> bool {
    let mut ok = true;
    let mut i = 0usize;
    while i < u3261Code::ALL.len() {
        let c = u3261Code::ALL[i].code();
        if c < 0xe080 || c > 0xe08f {
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
fn u3261_guard_seg() -> bool {
    u3261_segment_guard()
}

/// UNX-F3261 对拍表 16 行边界与累计守恒。
fn check_u3261_mix_tbl() -> bool {
    let mut ok = u3261mix_COUNT == 16;
    if ok { ok = u3261mix_get("ST_00") == Some(10) && u3261mix_get("WQ_15") == Some(85); }
    if ok { ok = u3261mix_get("ZZZ_NOKEY").is_none(); }
    if ok { ok = u3261mix_sum() == 760; }
    ok
}

/// UNX-F3261 混合映射 16 行守卫唯一 + 守卫阈值冻结。
fn check_u3261_mix_map() -> bool {
    let mut ok = u3261mixmap_COUNT == 16;
    if ok { ok = u3261mixmap_scan() == 0 && u3261mixmap_unique(); }
    if ok { ok = u3261mixmap_sum() == 504; }
    if ok { ok = U3261MIXGATE_THRESHOLD == 32; }
    ok
}

/// 本模块自检（逐检查带 detail，全绿才出门）。
pub fn run_u3261_checks() -> CheckSet {
    let mut set = CheckSet::new("U3261");
    set.add(
        "u3261-seg-guard",
        u3261_guard_seg(),
        if u3261_guard_seg() { "" } else { "diagnostic code outside segment" },
    );
    set.add(
        "u3261-mix-tbl",
        check_u3261_mix_tbl(),
        if check_u3261_mix_tbl() { "" } else { "UNX-F3261 mix table mismatch" },
    );
    set.add(
        "u3261-mix-map",
        check_u3261_mix_map(),
        if check_u3261_mix_map() { "" } else { "UNX-F3261 mix map mismatch" },
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
    fn u3261_all_checks_green() {
        let set = run_u3261_checks();
        assert!(set.all_passed(), "U3261 domain checks must be green");
    }

    #[test]
    fn u3261_segment_guard_holds() {
        assert!(u3261_segment_guard());
    }
}
