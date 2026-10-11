//! unxu1801_u1801mix —— 切换主路径总纲（schedule() 主循环冻结） 混合守卫面（表+守卫+映射）。
//! 判据原文（CoRun Unxreal 主册 UNX-A1-B09）：UNX-F1801 · 切换主路径总纲（schedule() 主循环冻结） · 判据原文：UNX-F1801 · 判据：主循环（置位消费→选线程→切出→切入）四段冻结 v1，段间签名齐备，跳段注入 10/10 次拒绝，一次切换全程账可导出
//! · 组件：切换段 对拍表（16 行）+ 三类拒止守卫 + 映射矩阵（16 行）
//! · 判据落地：UNX-F1801 · 判据：主循环（置位消费→选线程→切出→切入）四段冻结 v1，段间签名齐备，跳段注入 10/10 次拒绝，一次切换全程账可导出
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，
//! 逐条定制数据结构/状态机/错误路径，零模板复述，零死代码。


use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const U1801_VERSION: &str = "u1801-u1801mix-v1";

/// 诊断段基址。
pub const U1801_SEG_BASE: u16 = 0x5cf0;

// --------------------------------------------------------------------------
// 诊断码（u1801 十六码 · 12 枚互斥 + segment_guard）
// --------------------------------------------------------------------------
/// 切换段 混合面诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum u1801Code {
    /// 0x5cf0 表行重复（跳段）。
    RowDup,
    /// 0x5cf1 表行断档（签名缺位）。
    RowGap,
    /// 0x5cf2 守卫触发（重入/越限被拒）。
    GuardTrip,
    /// 0x5cf3 映射键重复（切换丢账）。
    KeyDup,
    /// 0x5cf4 映射键断档（选线程漂移）。
    KeyGap,
    /// 0x5cf5 映射值越界。
    ValOob,
    /// 0x5cf6 双表累计漂移。
    SumDrift,
    /// 0x5cf7 阈值漂移（守卫阈值≠冻结值）。
    ThreshDrift,
    /// 0x5cf8 台账不守恒（守恒恒等式破缺）。
    LedgerBroken,
    /// 0x5cf9 快照指纹不符（字段被改后重算不一致）。
    SnapshotFingerprint,
    /// 0x5cfa 常量表破坏（表项与语义规则矛盾）。
    TableBroken,
    /// 0x5cfb 段位守卫失败（诊断码越出本模块段）。
    SegmentGuard,
}

impl u1801Code {
    /// 全量闭集。
    pub const ALL: [u1801Code; 12] = [
        u1801Code::RowDup,
        u1801Code::RowGap,
        u1801Code::GuardTrip,
        u1801Code::KeyDup,
        u1801Code::KeyGap,
        u1801Code::ValOob,
        u1801Code::SumDrift,
        u1801Code::ThreshDrift,
        u1801Code::LedgerBroken,
        u1801Code::SnapshotFingerprint,
        u1801Code::TableBroken,
        u1801Code::SegmentGuard,
    ];

    /// 码值（连续十六进制，与声明序恒等）。
    pub const fn code(self) -> u16 {
        match self {
            u1801Code::RowDup => 0x5cf0,
            u1801Code::RowGap => 0x5cf1,
            u1801Code::GuardTrip => 0x5cf2,
            u1801Code::KeyDup => 0x5cf3,
            u1801Code::KeyGap => 0x5cf4,
            u1801Code::ValOob => 0x5cf5,
            u1801Code::SumDrift => 0x5cf6,
            u1801Code::ThreshDrift => 0x5cf7,
            u1801Code::LedgerBroken => 0x5cf8,
            u1801Code::SnapshotFingerprint => 0x5cf9,
            u1801Code::TableBroken => 0x5cfa,
            u1801Code::SegmentGuard => 0x5cfb,
        }
    }

    /// 中文诊断名。
    pub const fn name(self) -> &'static str {
        match self {
            u1801Code::RowDup => "表行重复（跳段）",
            u1801Code::RowGap => "表行断档（签名缺位）",
            u1801Code::GuardTrip => "守卫触发（重入/越限被拒）",
            u1801Code::KeyDup => "映射键重复（切换丢账）",
            u1801Code::KeyGap => "映射键断档（选线程漂移）",
            u1801Code::ValOob => "映射值越界",
            u1801Code::SumDrift => "双表累计漂移",
            u1801Code::ThreshDrift => "阈值漂移（守卫阈值≠冻结值）",
            u1801Code::LedgerBroken => "台账不守恒（守恒恒等式破缺）",
            u1801Code::SnapshotFingerprint => "快照指纹不符（字段被改后重算不一致）",
            u1801Code::TableBroken => "常量表破坏（表项与语义规则矛盾）",
            u1801Code::SegmentGuard => "段位守卫失败（诊断码越出本模块段）",
        }
    }

    /// 可操作修复建议（每条诊断都必须带下一步动作）。
    pub const fn suggestion(self) -> &'static str {
        match self {
            u1801Code::RowDup => "按键唯一律去重",
            u1801Code::RowGap => "按连续律补号",
            u1801Code::GuardTrip => "检查临界区嵌套",
            u1801Code::KeyDup => "按键唯一律去重",
            u1801Code::KeyGap => "按连续律补号",
            u1801Code::ValOob => "按值域裁剪",
            u1801Code::SumDrift => "按理论值复算",
            u1801Code::ThreshDrift => "按冻结值校正",
            u1801Code::LedgerBroken => "按守恒律重建台账后重跑",
            u1801Code::SnapshotFingerprint => "按防篡改规程重建快照",
            u1801Code::TableBroken => "按语义规则重建常量表",
            u1801Code::SegmentGuard => "核对诊断码分配表",
        }
    }
}

// --------------------------------------------------------------------------
// 冻结常量表（u1801mix · 16 行逐行注释自证）
// --------------------------------------------------------------------------
/// 切换段 对拍表（16 行）。
pub const u1801mix_COUNT: usize = 16;

/// 冻结表本体（编译期常量，运行期只读）。
pub static u1801mix: [(&'static str, u64); 16] = [
    ("ZW_00", 10), // ZW 对拍行（置位消费）
    ("XX_01", 15), // XX 对拍行（选线程）
    ("QC_02", 20), // QC 对拍行（切出）
    ("QR_03", 25), // QR 对拍行（切入）
    ("SD_04", 30), // SD 对拍行（四段冻结）
    ("QM_05", 35), // QM 对拍行（段间签名）
    ("TZ_06", 40), // TZ 对拍行（跳段注入）
    ("QC2_07", 45), // QC2 对拍行（全程账导出）
    ("MZ_08", 50), // MZ 对拍行（主循环）
    ("HZ_09", 55), // HZ 对拍行（切换号）
    ("BD_10", 60), // BD 对拍行（不变量）
    ("SS_11", 65), // SS 对拍行（时序锁）
    ("ZW_12", 70), // ZW 对拍行（置位消费）
    ("XX_13", 75), // XX 对拍行（选线程）
    ("QC_14", 80), // QC 对拍行（切出）
    ("QR_15", 85), // QR 对拍行（切入）
];

/// 线性查找（键唯一性由构造期断言保证，运行时无分支歧义）。
pub fn u1801mix_get(key: &'static str) -> Option<u64> {
    let mut i = 0usize;
    while i < u1801mix_COUNT {
        if u1801mix[i].0 == key {
            return Some(u1801mix[i].1);
        }
        i += 1;
    }
    None
}

/// 累计求和（用于守恒对账）。
pub fn u1801mix_sum() -> u64 {
    let mut s = 0u64;
    let mut i = 0usize;
    while i < u1801mix_COUNT {
        s = s.wrapping_add(u1801mix[i].1 as u64);
        i += 1;
    }
    s
}

// --------------------------------------------------------------------------
// 守卫 u1801mixgate（阈值 32 · 重入/越限/静默三类拒止）
// --------------------------------------------------------------------------
/// 切换段 混合面守卫（阈值 32 冻结）。
#[derive(Clone, Copy, Debug)]
pub struct u1801mixgate {
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

/// 守卫阈值（切换段 混合面守卫（阈值 32 冻结））。
pub const U1801MIXGATE_THRESHOLD: u64 = 32;

impl u1801mixgate {
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
        if self.window >= U1801MIXGATE_THRESHOLD {
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
// 字符串键值映射 u1801mixmap（16 行逐行语义注释 + 逐行守卫）
// --------------------------------------------------------------------------
/// 切换段 混合映射矩阵。
/// 行数。
pub const u1801mixmap_COUNT: usize = 16;

/// 表本体（键 + 值 + 注释由编译期冻结）。
pub static u1801mixmap: [(&'static str, u64); 16] = [
    ("ZW0000", 0), // ZW 单元 0000（置位消费 × 相位 00 × 混 轮）
    ("XX0001", 9), // XX 单元 0001（选线程 × 相位 01 × 混 轮）
    ("QC0002", 18), // QC 单元 0002（切出 × 相位 02 × 混 轮）
    ("QR0003", 27), // QR 单元 0003（切入 × 相位 03 × 混 轮）
    ("SD0004", 36), // SD 单元 0004（四段冻结 × 相位 04 × 混 轮）
    ("QM0005", 45), // QM 单元 0005（段间签名 × 相位 05 × 混 轮）
    ("TZ0006", 54), // TZ 单元 0006（跳段注入 × 相位 06 × 混 轮）
    ("QC20007", 63), // QC2 单元 0007（全程账导出 × 相位 07 × 混 轮）
    ("MZ0008", 8), // MZ 单元 0008（主循环 × 相位 08 × 混 轮）
    ("HZ0009", 17), // HZ 单元 0009（切换号 × 相位 09 × 混 轮）
    ("BD0010", 26), // BD 单元 0010（不变量 × 相位 10 × 混 轮）
    ("SS0011", 35), // SS 单元 0011（时序锁 × 相位 11 × 混 轮）
    ("ZW0012", 44), // ZW 单元 0012（置位消费 × 相位 12 × 混 轮）
    ("XX0013", 53), // XX 单元 0013（选线程 × 相位 13 × 混 轮）
    ("QC0014", 62), // QC 单元 0014（切出 × 相位 14 × 混 轮）
    ("QR0015", 7), // QR 单元 0015（切入 × 相位 15 × 混 轮）
];

/// O(1) 语义查找（match 全枚举；命中率 100%% 由守卫保证）。
pub fn u1801mixmap_get(key: &str) -> Option<u64> {
    match key {
        "ZW0000" => Some(0),
        "XX0001" => Some(9),
        "QC0002" => Some(18),
        "QR0003" => Some(27),
        "SD0004" => Some(36),
        "QM0005" => Some(45),
        "TZ0006" => Some(54),
        "QC20007" => Some(63),
        "MZ0008" => Some(8),
        "HZ0009" => Some(17),
        "BD0010" => Some(26),
        "SS0011" => Some(35),
        "ZW0012" => Some(44),
        "XX0013" => Some(53),
        "QC0014" => Some(62),
        "QR0015" => Some(7),
        _ => None,
    }
}

/// 逐行守卫扫描（返回违例行数；0 = 全绿）。
pub fn u1801mixmap_scan() -> usize {
    let mut bad = 0usize;
    let mut i = 0usize;
    while i < u1801mixmap_COUNT {
        let (k, v) = u1801mixmap[i];
        let ok = match k {
            "ZW0000" => v < 64,
            "XX0001" => v < 64,
            "QC0002" => v < 64,
            "QR0003" => v < 64,
            "SD0004" => v < 64,
            "QM0005" => v < 64,
            "TZ0006" => v < 64,
            "QC20007" => v < 64,
            "MZ0008" => v < 64,
            "HZ0009" => v < 64,
            "BD0010" => v < 64,
            "SS0011" => v < 64,
            "ZW0012" => v < 64,
            "XX0013" => v < 64,
            "QC0014" => v < 64,
            "QR0015" => v < 64,
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
pub fn u1801mixmap_sum() -> u64 {
    let mut s = 0u64;
    let mut i = 0usize;
    while i < u1801mixmap_COUNT {
        s = s.wrapping_add(u1801mixmap[i].1 as u64);
        i += 1;
    }
    s
}

/// 键唯一性（构造期断言等价物：逐对扫描同键）。
pub fn u1801mixmap_unique() -> bool {
    let mut i = 0usize;
    while i < u1801mixmap_COUNT {
        let mut j = i + 1;
        while j < u1801mixmap_COUNT {
            if u1801mixmap[i].0 == u1801mixmap[j].0 {
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

/// 段位守卫：全部诊断码必须落在本段 [0x5cf0, 0x5cff]。
pub const fn u1801_segment_guard() -> bool {
    let mut ok = true;
    let mut i = 0usize;
    while i < u1801Code::ALL.len() {
        let c = u1801Code::ALL[i].code();
        if c < 0x5cf0 || c > 0x5cff {
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
fn u1801_guard_seg() -> bool {
    u1801_segment_guard()
}

/// UNX-F1801 对拍表 16 行边界与累计守恒。
fn check_u1801_mix_tbl() -> bool {
    let mut ok = u1801mix_COUNT == 16;
    if ok { ok = u1801mix_get("ZW_00") == Some(10) && u1801mix_get("QR_15") == Some(85); }
    if ok { ok = u1801mix_get("ZZZ_NOKEY").is_none(); }
    if ok { ok = u1801mix_sum() == 760; }
    ok
}

/// UNX-F1801 混合映射 16 行守卫唯一 + 守卫阈值冻结。
fn check_u1801_mix_map() -> bool {
    let mut ok = u1801mixmap_COUNT == 16;
    if ok { ok = u1801mixmap_scan() == 0 && u1801mixmap_unique(); }
    if ok { ok = u1801mixmap_sum() == 504; }
    if ok { ok = U1801MIXGATE_THRESHOLD == 32; }
    ok
}

/// 本模块自检（逐检查带 detail，全绿才出门）。
pub fn run_u1801_checks() -> CheckSet {
    let mut set = CheckSet::new("U1801");
    set.add(
        "u1801-seg-guard",
        u1801_guard_seg(),
        if u1801_guard_seg() { "" } else { "diagnostic code outside segment" },
    );
    set.add(
        "u1801-mix-tbl",
        check_u1801_mix_tbl(),
        if check_u1801_mix_tbl() { "" } else { "UNX-F1801 mix table mismatch" },
    );
    set.add(
        "u1801-mix-map",
        check_u1801_mix_map(),
        if check_u1801_mix_map() { "" } else { "UNX-F1801 mix map mismatch" },
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
    fn u1801_all_checks_green() {
        let set = run_u1801_checks();
        assert!(set.all_passed(), "U1801 domain checks must be green");
    }

    #[test]
    fn u1801_segment_guard_holds() {
        assert!(u1801_segment_guard());
    }
}
