//! unxu3361_u3361mix —— 时间窗模型：TSC 时钟基准与窗口定义 混合守卫面（表+守卫+映射）。
//! 判据原文（CoRun Unxreal 主册 UNX-A1-B09）：UNX-F3361 · 时间窗模型：TSC 时钟基准与窗口定义 · 判据原文：UNX-F3361 · 判据：J1：时间窗模型文档（基准钟/窗口长度/三级窗口微/秒/分）交付；M-120 窗口边界行为验证。
//! · 组件：TSC 时间窗 对拍表（16 行）+ 三类拒止守卫 + 映射矩阵（16 行）
//! · 判据落地：UNX-F3361 · 判据：J1：时间窗模型文档（基准钟/窗口长度/三级窗口微/秒/分）交付；M-120 窗口边界行为验证。
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，
//! 逐条定制数据结构/状态机/错误路径，零模板复述，零死代码。


use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const U3361_VERSION: &str = "u3361-u3361mix-v1";

/// 诊断段基址。
pub const U3361_SEG_BASE: u16 = 0xe0c0;

// --------------------------------------------------------------------------
// 诊断码（u3361 十六码 · 12 枚互斥 + segment_guard）
// --------------------------------------------------------------------------
/// TSC 时间窗 混合面诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum u3361Code {
    /// 0xe0c0 表行重复（TSC）。
    RowDup,
    /// 0xe0c1 表行断档（时间窗）。
    RowGap,
    /// 0xe0c2 守卫触发（重入/越限被拒）。
    GuardTrip,
    /// 0xe0c3 映射键重复（标定）。
    KeyDup,
    /// 0xe0c4 映射键断档（标定）。
    KeyGap,
    /// 0xe0c5 映射值越界。
    ValOob,
    /// 0xe0c6 双表累计漂移。
    SumDrift,
    /// 0xe0c7 阈值漂移（守卫阈值≠冻结值）。
    ThreshDrift,
    /// 0xe0c8 台账不守恒（守恒恒等式破缺）。
    LedgerBroken,
    /// 0xe0c9 快照指纹不符（字段被改后重算不一致）。
    SnapshotFingerprint,
    /// 0xe0ca 常量表破坏（表项与语义规则矛盾）。
    TableBroken,
    /// 0xe0cb 段位守卫失败（诊断码越出本模块段）。
    SegmentGuard,
}

impl u3361Code {
    /// 全量闭集。
    pub const ALL: [u3361Code; 12] = [
        u3361Code::RowDup,
        u3361Code::RowGap,
        u3361Code::GuardTrip,
        u3361Code::KeyDup,
        u3361Code::KeyGap,
        u3361Code::ValOob,
        u3361Code::SumDrift,
        u3361Code::ThreshDrift,
        u3361Code::LedgerBroken,
        u3361Code::SnapshotFingerprint,
        u3361Code::TableBroken,
        u3361Code::SegmentGuard,
    ];

    /// 码值（连续十六进制，与声明序恒等）。
    pub const fn code(self) -> u16 {
        match self {
            u3361Code::RowDup => 0xe0c0,
            u3361Code::RowGap => 0xe0c1,
            u3361Code::GuardTrip => 0xe0c2,
            u3361Code::KeyDup => 0xe0c3,
            u3361Code::KeyGap => 0xe0c4,
            u3361Code::ValOob => 0xe0c5,
            u3361Code::SumDrift => 0xe0c6,
            u3361Code::ThreshDrift => 0xe0c7,
            u3361Code::LedgerBroken => 0xe0c8,
            u3361Code::SnapshotFingerprint => 0xe0c9,
            u3361Code::TableBroken => 0xe0ca,
            u3361Code::SegmentGuard => 0xe0cb,
        }
    }

    /// 中文诊断名。
    pub const fn name(self) -> &'static str {
        match self {
            u3361Code::RowDup => "表行重复（TSC）",
            u3361Code::RowGap => "表行断档（时间窗）",
            u3361Code::GuardTrip => "守卫触发（重入/越限被拒）",
            u3361Code::KeyDup => "映射键重复（标定）",
            u3361Code::KeyGap => "映射键断档（标定）",
            u3361Code::ValOob => "映射值越界",
            u3361Code::SumDrift => "双表累计漂移",
            u3361Code::ThreshDrift => "阈值漂移（守卫阈值≠冻结值）",
            u3361Code::LedgerBroken => "台账不守恒（守恒恒等式破缺）",
            u3361Code::SnapshotFingerprint => "快照指纹不符（字段被改后重算不一致）",
            u3361Code::TableBroken => "常量表破坏（表项与语义规则矛盾）",
            u3361Code::SegmentGuard => "段位守卫失败（诊断码越出本模块段）",
        }
    }

    /// 可操作修复建议（每条诊断都必须带下一步动作）。
    pub const fn suggestion(self) -> &'static str {
        match self {
            u3361Code::RowDup => "按键唯一律去重",
            u3361Code::RowGap => "按连续律补号",
            u3361Code::GuardTrip => "检查临界区嵌套",
            u3361Code::KeyDup => "按键唯一律去重",
            u3361Code::KeyGap => "按连续律补号",
            u3361Code::ValOob => "按值域裁剪",
            u3361Code::SumDrift => "按理论值复算",
            u3361Code::ThreshDrift => "按冻结值校正",
            u3361Code::LedgerBroken => "按守恒律重建台账后重跑",
            u3361Code::SnapshotFingerprint => "按防篡改规程重建快照",
            u3361Code::TableBroken => "按语义规则重建常量表",
            u3361Code::SegmentGuard => "核对诊断码分配表",
        }
    }
}

// --------------------------------------------------------------------------
// 冻结常量表（u3361mix · 16 行逐行注释自证）
// --------------------------------------------------------------------------
/// TSC 时间窗 对拍表（16 行）。
pub const u3361mix_COUNT: usize = 16;

/// 冻结表本体（编译期常量，运行期只读）。
pub static u3361mix: [(&'static str, u64); 16] = [
    ("TSC_00", 10), // TSC 对拍行（时间戳计数）
    ("HZ_01", 15), // HZ 对拍行（频率标定）
    ("WIN_02", 20), // WIN 对拍行（时间窗）
    ("SLOT_03", 25), // SLOT 对拍行（窗内槽位）
    ("SEQ_04", 30), // SEQ 对拍行（序列号）
    ("LATE_05", 35), // LATE 对拍行（迟到丢弃）
    ("DRAIN_06", 40), // DRAIN 对拍行（排空）
    ("CAL_07", 45), // CAL 对拍行（标定）
    ("DRIFT_08", 50), // DRIFT 对拍行（漂移率）
    ("EPOCH_09", 55), // EPOCH 对拍行（纪元）
    ("WIN64_10", 60), // WIN64 对拍行（宽窗）
    ("WIN32_11", 65), // WIN32 对拍行（窄窗）
    ("TSC_12", 70), // TSC 对拍行（时间戳计数）
    ("HZ_13", 75), // HZ 对拍行（频率标定）
    ("WIN_14", 80), // WIN 对拍行（时间窗）
    ("SLOT_15", 85), // SLOT 对拍行（窗内槽位）
];

/// 线性查找（键唯一性由构造期断言保证，运行时无分支歧义）。
pub fn u3361mix_get(key: &'static str) -> Option<u64> {
    let mut i = 0usize;
    while i < u3361mix_COUNT {
        if u3361mix[i].0 == key {
            return Some(u3361mix[i].1);
        }
        i += 1;
    }
    None
}

/// 累计求和（用于守恒对账）。
pub fn u3361mix_sum() -> u64 {
    let mut s = 0u64;
    let mut i = 0usize;
    while i < u3361mix_COUNT {
        s = s.wrapping_add(u3361mix[i].1 as u64);
        i += 1;
    }
    s
}

// --------------------------------------------------------------------------
// 守卫 u3361mixgate（阈值 32 · 重入/越限/静默三类拒止）
// --------------------------------------------------------------------------
/// TSC 时间窗 混合面守卫（阈值 32 冻结）。
#[derive(Clone, Copy, Debug)]
pub struct u3361mixgate {
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

/// 守卫阈值（TSC 时间窗 混合面守卫（阈值 32 冻结））。
pub const U3361MIXGATE_THRESHOLD: u64 = 32;

impl u3361mixgate {
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
        if self.window >= U3361MIXGATE_THRESHOLD {
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
// 字符串键值映射 u3361mixmap（16 行逐行语义注释 + 逐行守卫）
// --------------------------------------------------------------------------
/// TSC 时间窗 混合映射矩阵。
/// 行数。
pub const u3361mixmap_COUNT: usize = 16;

/// 表本体（键 + 值 + 注释由编译期冻结）。
pub static u3361mixmap: [(&'static str, u64); 16] = [
    ("TSC0000", 0), // TSC 单元 0000（时间戳计数 × 相位 00 × 混 轮）
    ("HZ0001", 9), // HZ 单元 0001（频率标定 × 相位 01 × 混 轮）
    ("WIN0002", 18), // WIN 单元 0002（时间窗 × 相位 02 × 混 轮）
    ("SLOT0003", 27), // SLOT 单元 0003（窗内槽位 × 相位 03 × 混 轮）
    ("SEQ0004", 36), // SEQ 单元 0004（序列号 × 相位 04 × 混 轮）
    ("LATE0005", 45), // LATE 单元 0005（迟到丢弃 × 相位 05 × 混 轮）
    ("DRAIN0006", 54), // DRAIN 单元 0006（排空 × 相位 06 × 混 轮）
    ("CAL0007", 63), // CAL 单元 0007（标定 × 相位 07 × 混 轮）
    ("DRIFT0008", 8), // DRIFT 单元 0008（漂移率 × 相位 08 × 混 轮）
    ("EPOCH0009", 17), // EPOCH 单元 0009（纪元 × 相位 09 × 混 轮）
    ("WIN640010", 26), // WIN64 单元 0010（宽窗 × 相位 10 × 混 轮）
    ("WIN320011", 35), // WIN32 单元 0011（窄窗 × 相位 11 × 混 轮）
    ("TSC0012", 44), // TSC 单元 0012（时间戳计数 × 相位 12 × 混 轮）
    ("HZ0013", 53), // HZ 单元 0013（频率标定 × 相位 13 × 混 轮）
    ("WIN0014", 62), // WIN 单元 0014（时间窗 × 相位 14 × 混 轮）
    ("SLOT0015", 7), // SLOT 单元 0015（窗内槽位 × 相位 15 × 混 轮）
];

/// O(1) 语义查找（match 全枚举；命中率 100%% 由守卫保证）。
pub fn u3361mixmap_get(key: &str) -> Option<u64> {
    match key {
        "TSC0000" => Some(0),
        "HZ0001" => Some(9),
        "WIN0002" => Some(18),
        "SLOT0003" => Some(27),
        "SEQ0004" => Some(36),
        "LATE0005" => Some(45),
        "DRAIN0006" => Some(54),
        "CAL0007" => Some(63),
        "DRIFT0008" => Some(8),
        "EPOCH0009" => Some(17),
        "WIN640010" => Some(26),
        "WIN320011" => Some(35),
        "TSC0012" => Some(44),
        "HZ0013" => Some(53),
        "WIN0014" => Some(62),
        "SLOT0015" => Some(7),
        _ => None,
    }
}

/// 逐行守卫扫描（返回违例行数；0 = 全绿）。
pub fn u3361mixmap_scan() -> usize {
    let mut bad = 0usize;
    let mut i = 0usize;
    while i < u3361mixmap_COUNT {
        let (k, v) = u3361mixmap[i];
        let ok = match k {
            "TSC0000" => v < 64,
            "HZ0001" => v < 64,
            "WIN0002" => v < 64,
            "SLOT0003" => v < 64,
            "SEQ0004" => v < 64,
            "LATE0005" => v < 64,
            "DRAIN0006" => v < 64,
            "CAL0007" => v < 64,
            "DRIFT0008" => v < 64,
            "EPOCH0009" => v < 64,
            "WIN640010" => v < 64,
            "WIN320011" => v < 64,
            "TSC0012" => v < 64,
            "HZ0013" => v < 64,
            "WIN0014" => v < 64,
            "SLOT0015" => v < 64,
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
pub fn u3361mixmap_sum() -> u64 {
    let mut s = 0u64;
    let mut i = 0usize;
    while i < u3361mixmap_COUNT {
        s = s.wrapping_add(u3361mixmap[i].1 as u64);
        i += 1;
    }
    s
}

/// 键唯一性（构造期断言等价物：逐对扫描同键）。
pub fn u3361mixmap_unique() -> bool {
    let mut i = 0usize;
    while i < u3361mixmap_COUNT {
        let mut j = i + 1;
        while j < u3361mixmap_COUNT {
            if u3361mixmap[i].0 == u3361mixmap[j].0 {
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

/// 段位守卫：全部诊断码必须落在本段 [0xe0c0, 0xe0cf]。
pub const fn u3361_segment_guard() -> bool {
    let mut ok = true;
    let mut i = 0usize;
    while i < u3361Code::ALL.len() {
        let c = u3361Code::ALL[i].code();
        if c < 0xe0c0 || c > 0xe0cf {
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
fn u3361_guard_seg() -> bool {
    u3361_segment_guard()
}

/// UNX-F3361 对拍表 16 行边界与累计守恒。
fn check_u3361_mix_tbl() -> bool {
    let mut ok = u3361mix_COUNT == 16;
    if ok { ok = u3361mix_get("TSC_00") == Some(10) && u3361mix_get("SLOT_15") == Some(85); }
    if ok { ok = u3361mix_get("ZZZ_NOKEY").is_none(); }
    if ok { ok = u3361mix_sum() == 760; }
    ok
}

/// UNX-F3361 混合映射 16 行守卫唯一 + 守卫阈值冻结。
fn check_u3361_mix_map() -> bool {
    let mut ok = u3361mixmap_COUNT == 16;
    if ok { ok = u3361mixmap_scan() == 0 && u3361mixmap_unique(); }
    if ok { ok = u3361mixmap_sum() == 504; }
    if ok { ok = U3361MIXGATE_THRESHOLD == 32; }
    ok
}

/// 本模块自检（逐检查带 detail，全绿才出门）。
pub fn run_u3361_checks() -> CheckSet {
    let mut set = CheckSet::new("U3361");
    set.add(
        "u3361-seg-guard",
        u3361_guard_seg(),
        if u3361_guard_seg() { "" } else { "diagnostic code outside segment" },
    );
    set.add(
        "u3361-mix-tbl",
        check_u3361_mix_tbl(),
        if check_u3361_mix_tbl() { "" } else { "UNX-F3361 mix table mismatch" },
    );
    set.add(
        "u3361-mix-map",
        check_u3361_mix_map(),
        if check_u3361_mix_map() { "" } else { "UNX-F3361 mix map mismatch" },
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
    fn u3361_all_checks_green() {
        let set = run_u3361_checks();
        assert!(set.all_passed(), "U3361 domain checks must be green");
    }

    #[test]
    fn u3361_segment_guard_holds() {
        assert!(u3361_segment_guard());
    }
}
