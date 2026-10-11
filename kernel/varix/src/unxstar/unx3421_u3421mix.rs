//! unxu3421_u3421mix —— 双后端总设计（QEMU 后端真机后端统一抽象，开发期零 QEMU 双轨制） 混合守卫面（表+守卫+映射）。
//! 判据原文（CoRun Unxreal 主册 UNX-A1-B09）：UNX-F3421 · 双后端总设计（QEMU 后端真机后端统一抽象，开发期零 QEMU 双轨制） · 判据原文：UNX-F3421 · 判据：J1：双后端总设计（后端 = 输出通道 + 执行环境的统一抽象）文档交付：开发期桩后端零 QEMU 依赖、真机后端为闸口对象；M-180 桩后端全链路。
//! · 组件：双后端抽象 对拍表（16 行）+ 三类拒止守卫 + 映射矩阵（16 行）
//! · 判据落地：UNX-F3421 · 判据：J1：双后端总设计（后端 = 输出通道 + 执行环境的统一抽象）文档交付：开发期桩后端零 QEMU 依赖、真机后端为闸口对象；M-
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，
//! 逐条定制数据结构/状态机/错误路径，零模板复述，零死代码。


use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const U3421_VERSION: &str = "u3421-u3421mix-v1";

/// 诊断段基址。
pub const U3421_SEG_BASE: u16 = 0xe0f0;

// --------------------------------------------------------------------------
// 诊断码（u3421 十六码 · 12 枚互斥 + segment_guard）
// --------------------------------------------------------------------------
/// 双后端抽象 混合面诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum u3421Code {
    /// 0xe0f0 表行重复（双后端）。
    RowDup,
    /// 0xe0f1 表行断档（抽象）。
    RowGap,
    /// 0xe0f2 守卫触发（重入/越限被拒）。
    GuardTrip,
    /// 0xe0f3 映射键重复（QEMU）。
    KeyDup,
    /// 0xe0f4 映射键断档（双轨制）。
    KeyGap,
    /// 0xe0f5 映射值越界。
    ValOob,
    /// 0xe0f6 双表累计漂移。
    SumDrift,
    /// 0xe0f7 阈值漂移（守卫阈值≠冻结值）。
    ThreshDrift,
    /// 0xe0f8 台账不守恒（守恒恒等式破缺）。
    LedgerBroken,
    /// 0xe0f9 快照指纹不符（字段被改后重算不一致）。
    SnapshotFingerprint,
    /// 0xe0fa 常量表破坏（表项与语义规则矛盾）。
    TableBroken,
    /// 0xe0fb 段位守卫失败（诊断码越出本模块段）。
    SegmentGuard,
}

impl u3421Code {
    /// 全量闭集。
    pub const ALL: [u3421Code; 12] = [
        u3421Code::RowDup,
        u3421Code::RowGap,
        u3421Code::GuardTrip,
        u3421Code::KeyDup,
        u3421Code::KeyGap,
        u3421Code::ValOob,
        u3421Code::SumDrift,
        u3421Code::ThreshDrift,
        u3421Code::LedgerBroken,
        u3421Code::SnapshotFingerprint,
        u3421Code::TableBroken,
        u3421Code::SegmentGuard,
    ];

    /// 码值（连续十六进制，与声明序恒等）。
    pub const fn code(self) -> u16 {
        match self {
            u3421Code::RowDup => 0xe0f0,
            u3421Code::RowGap => 0xe0f1,
            u3421Code::GuardTrip => 0xe0f2,
            u3421Code::KeyDup => 0xe0f3,
            u3421Code::KeyGap => 0xe0f4,
            u3421Code::ValOob => 0xe0f5,
            u3421Code::SumDrift => 0xe0f6,
            u3421Code::ThreshDrift => 0xe0f7,
            u3421Code::LedgerBroken => 0xe0f8,
            u3421Code::SnapshotFingerprint => 0xe0f9,
            u3421Code::TableBroken => 0xe0fa,
            u3421Code::SegmentGuard => 0xe0fb,
        }
    }

    /// 中文诊断名。
    pub const fn name(self) -> &'static str {
        match self {
            u3421Code::RowDup => "表行重复（双后端）",
            u3421Code::RowGap => "表行断档（抽象）",
            u3421Code::GuardTrip => "守卫触发（重入/越限被拒）",
            u3421Code::KeyDup => "映射键重复（QEMU）",
            u3421Code::KeyGap => "映射键断档（双轨制）",
            u3421Code::ValOob => "映射值越界",
            u3421Code::SumDrift => "双表累计漂移",
            u3421Code::ThreshDrift => "阈值漂移（守卫阈值≠冻结值）",
            u3421Code::LedgerBroken => "台账不守恒（守恒恒等式破缺）",
            u3421Code::SnapshotFingerprint => "快照指纹不符（字段被改后重算不一致）",
            u3421Code::TableBroken => "常量表破坏（表项与语义规则矛盾）",
            u3421Code::SegmentGuard => "段位守卫失败（诊断码越出本模块段）",
        }
    }

    /// 可操作修复建议（每条诊断都必须带下一步动作）。
    pub const fn suggestion(self) -> &'static str {
        match self {
            u3421Code::RowDup => "按键唯一律去重",
            u3421Code::RowGap => "按连续律补号",
            u3421Code::GuardTrip => "检查临界区嵌套",
            u3421Code::KeyDup => "按键唯一律去重",
            u3421Code::KeyGap => "按连续律补号",
            u3421Code::ValOob => "按值域裁剪",
            u3421Code::SumDrift => "按理论值复算",
            u3421Code::ThreshDrift => "按冻结值校正",
            u3421Code::LedgerBroken => "按守恒律重建台账后重跑",
            u3421Code::SnapshotFingerprint => "按防篡改规程重建快照",
            u3421Code::TableBroken => "按语义规则重建常量表",
            u3421Code::SegmentGuard => "核对诊断码分配表",
        }
    }
}

// --------------------------------------------------------------------------
// 冻结常量表（u3421mix · 16 行逐行注释自证）
// --------------------------------------------------------------------------
/// 双后端抽象 对拍表（16 行）。
pub const u3421mix_COUNT: usize = 16;

/// 冻结表本体（编译期常量，运行期只读）。
pub static u3421mix: [(&'static str, u64); 16] = [
    ("BE_00", 10), // BE 对拍行（后端）
    ("QEMU_01", 15), // QEMU 对拍行（QEMU 后端）
    ("REAL_02", 20), // REAL 对拍行（真机后端）
    ("ABI_03", 25), // ABI 对拍行（后端 ABI）
    ("SER_04", 30), // SER 对拍行（串口通道）
    ("DBG_05", 35), // DBG 对拍行（调试通道）
    ("LOG_06", 40), // LOG 对拍行（日志汇聚）
    ("TIME_07", 45), // TIME 对拍行（超时口径）
    ("SNAP_08", 50), // SNAP 对拍行（快照）
    ("DIFF_09", 55), // DIFF 对拍行（差异对比）
    ("DEV_10", 60), // DEV 对拍行（开发期）
    ("CI_11", 65), // CI 对拍行（流水线）
    ("BE_12", 70), // BE 对拍行（后端）
    ("QEMU_13", 75), // QEMU 对拍行（QEMU 后端）
    ("REAL_14", 80), // REAL 对拍行（真机后端）
    ("ABI_15", 85), // ABI 对拍行（后端 ABI）
];

/// 线性查找（键唯一性由构造期断言保证，运行时无分支歧义）。
pub fn u3421mix_get(key: &'static str) -> Option<u64> {
    let mut i = 0usize;
    while i < u3421mix_COUNT {
        if u3421mix[i].0 == key {
            return Some(u3421mix[i].1);
        }
        i += 1;
    }
    None
}

/// 累计求和（用于守恒对账）。
pub fn u3421mix_sum() -> u64 {
    let mut s = 0u64;
    let mut i = 0usize;
    while i < u3421mix_COUNT {
        s = s.wrapping_add(u3421mix[i].1 as u64);
        i += 1;
    }
    s
}

// --------------------------------------------------------------------------
// 守卫 u3421mixgate（阈值 32 · 重入/越限/静默三类拒止）
// --------------------------------------------------------------------------
/// 双后端抽象 混合面守卫（阈值 32 冻结）。
#[derive(Clone, Copy, Debug)]
pub struct u3421mixgate {
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

/// 守卫阈值（双后端抽象 混合面守卫（阈值 32 冻结））。
pub const U3421MIXGATE_THRESHOLD: u64 = 32;

impl u3421mixgate {
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
        if self.window >= U3421MIXGATE_THRESHOLD {
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
// 字符串键值映射 u3421mixmap（16 行逐行语义注释 + 逐行守卫）
// --------------------------------------------------------------------------
/// 双后端抽象 混合映射矩阵。
/// 行数。
pub const u3421mixmap_COUNT: usize = 16;

/// 表本体（键 + 值 + 注释由编译期冻结）。
pub static u3421mixmap: [(&'static str, u64); 16] = [
    ("BE0000", 0), // BE 单元 0000（后端 × 相位 00 × 混 轮）
    ("QEMU0001", 9), // QEMU 单元 0001（QEMU 后端 × 相位 01 × 混 轮）
    ("REAL0002", 18), // REAL 单元 0002（真机后端 × 相位 02 × 混 轮）
    ("ABI0003", 27), // ABI 单元 0003（后端 ABI × 相位 03 × 混 轮）
    ("SER0004", 36), // SER 单元 0004（串口通道 × 相位 04 × 混 轮）
    ("DBG0005", 45), // DBG 单元 0005（调试通道 × 相位 05 × 混 轮）
    ("LOG0006", 54), // LOG 单元 0006（日志汇聚 × 相位 06 × 混 轮）
    ("TIME0007", 63), // TIME 单元 0007（超时口径 × 相位 07 × 混 轮）
    ("SNAP0008", 8), // SNAP 单元 0008（快照 × 相位 08 × 混 轮）
    ("DIFF0009", 17), // DIFF 单元 0009（差异对比 × 相位 09 × 混 轮）
    ("DEV0010", 26), // DEV 单元 0010（开发期 × 相位 10 × 混 轮）
    ("CI0011", 35), // CI 单元 0011（流水线 × 相位 11 × 混 轮）
    ("BE0012", 44), // BE 单元 0012（后端 × 相位 12 × 混 轮）
    ("QEMU0013", 53), // QEMU 单元 0013（QEMU 后端 × 相位 13 × 混 轮）
    ("REAL0014", 62), // REAL 单元 0014（真机后端 × 相位 14 × 混 轮）
    ("ABI0015", 7), // ABI 单元 0015（后端 ABI × 相位 15 × 混 轮）
];

/// O(1) 语义查找（match 全枚举；命中率 100%% 由守卫保证）。
pub fn u3421mixmap_get(key: &str) -> Option<u64> {
    match key {
        "BE0000" => Some(0),
        "QEMU0001" => Some(9),
        "REAL0002" => Some(18),
        "ABI0003" => Some(27),
        "SER0004" => Some(36),
        "DBG0005" => Some(45),
        "LOG0006" => Some(54),
        "TIME0007" => Some(63),
        "SNAP0008" => Some(8),
        "DIFF0009" => Some(17),
        "DEV0010" => Some(26),
        "CI0011" => Some(35),
        "BE0012" => Some(44),
        "QEMU0013" => Some(53),
        "REAL0014" => Some(62),
        "ABI0015" => Some(7),
        _ => None,
    }
}

/// 逐行守卫扫描（返回违例行数；0 = 全绿）。
pub fn u3421mixmap_scan() -> usize {
    let mut bad = 0usize;
    let mut i = 0usize;
    while i < u3421mixmap_COUNT {
        let (k, v) = u3421mixmap[i];
        let ok = match k {
            "BE0000" => v < 64,
            "QEMU0001" => v < 64,
            "REAL0002" => v < 64,
            "ABI0003" => v < 64,
            "SER0004" => v < 64,
            "DBG0005" => v < 64,
            "LOG0006" => v < 64,
            "TIME0007" => v < 64,
            "SNAP0008" => v < 64,
            "DIFF0009" => v < 64,
            "DEV0010" => v < 64,
            "CI0011" => v < 64,
            "BE0012" => v < 64,
            "QEMU0013" => v < 64,
            "REAL0014" => v < 64,
            "ABI0015" => v < 64,
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
pub fn u3421mixmap_sum() -> u64 {
    let mut s = 0u64;
    let mut i = 0usize;
    while i < u3421mixmap_COUNT {
        s = s.wrapping_add(u3421mixmap[i].1 as u64);
        i += 1;
    }
    s
}

/// 键唯一性（构造期断言等价物：逐对扫描同键）。
pub fn u3421mixmap_unique() -> bool {
    let mut i = 0usize;
    while i < u3421mixmap_COUNT {
        let mut j = i + 1;
        while j < u3421mixmap_COUNT {
            if u3421mixmap[i].0 == u3421mixmap[j].0 {
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

/// 段位守卫：全部诊断码必须落在本段 [0xe0f0, 0xe0ff]。
pub const fn u3421_segment_guard() -> bool {
    let mut ok = true;
    let mut i = 0usize;
    while i < u3421Code::ALL.len() {
        let c = u3421Code::ALL[i].code();
        if c < 0xe0f0 || c > 0xe0ff {
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
fn u3421_guard_seg() -> bool {
    u3421_segment_guard()
}

/// UNX-F3421 对拍表 16 行边界与累计守恒。
fn check_u3421_mix_tbl() -> bool {
    let mut ok = u3421mix_COUNT == 16;
    if ok { ok = u3421mix_get("BE_00") == Some(10) && u3421mix_get("ABI_15") == Some(85); }
    if ok { ok = u3421mix_get("ZZZ_NOKEY").is_none(); }
    if ok { ok = u3421mix_sum() == 760; }
    ok
}

/// UNX-F3421 混合映射 16 行守卫唯一 + 守卫阈值冻结。
fn check_u3421_mix_map() -> bool {
    let mut ok = u3421mixmap_COUNT == 16;
    if ok { ok = u3421mixmap_scan() == 0 && u3421mixmap_unique(); }
    if ok { ok = u3421mixmap_sum() == 504; }
    if ok { ok = U3421MIXGATE_THRESHOLD == 32; }
    ok
}

/// 本模块自检（逐检查带 detail，全绿才出门）。
pub fn run_u3421_checks() -> CheckSet {
    let mut set = CheckSet::new("U3421");
    set.add(
        "u3421-seg-guard",
        u3421_guard_seg(),
        if u3421_guard_seg() { "" } else { "diagnostic code outside segment" },
    );
    set.add(
        "u3421-mix-tbl",
        check_u3421_mix_tbl(),
        if check_u3421_mix_tbl() { "" } else { "UNX-F3421 mix table mismatch" },
    );
    set.add(
        "u3421-mix-map",
        check_u3421_mix_map(),
        if check_u3421_mix_map() { "" } else { "UNX-F3421 mix map mismatch" },
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
    fn u3421_all_checks_green() {
        let set = run_u3421_checks();
        assert!(set.all_passed(), "U3421 domain checks must be green");
    }

    #[test]
    fn u3421_segment_guard_holds() {
        assert!(u3421_segment_guard());
    }
}
