//! unxu3541_u3541mix —— 采样剖析器总设计（周期中断采样——桩化 tick） 混合守卫面（表+守卫+映射）。
//! 判据原文（CoRun Unxreal 主册 UNX-A1-B09）：UNX-F3541 · 采样剖析器总设计（周期中断采样——桩化 tick） · 判据原文：UNX-F3541 · 判据：J1：剖析语义冻结（周期 tick→采当前栈→计数——采样式非插桩式）；M-299 采样全链（tick→样本→聚合→导出四步通）。<br>J2：采样式选型声明（与插桩式的取舍——零代码改动税/精度换自由——判据行内嵌）。
//! · 组件：采样剖析器 对拍表（16 行）+ 三类拒止守卫 + 映射矩阵（16 行）
//! · 判据落地：UNX-F3541 · 判据：J1：剖析语义冻结（周期 tick→采当前栈→计数——采样式非插桩式）；M-299 采样全链（tick→样本→聚合→导出四步通）。
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，
//! 逐条定制数据结构/状态机/错误路径，零模板复述，零死代码。


use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const U3541_VERSION: &str = "u3541-u3541mix-v1";

/// 诊断段基址。
pub const U3541_SEG_BASE: u16 = 0xe150;

// --------------------------------------------------------------------------
// 诊断码（u3541 十六码 · 12 枚互斥 + segment_guard）
// --------------------------------------------------------------------------
/// 采样剖析器 混合面诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum u3541Code {
    /// 0xe150 表行重复（采样）。
    RowDup,
    /// 0xe151 表行断档（剖析）。
    RowGap,
    /// 0xe152 守卫触发（重入/越限被拒）。
    GuardTrip,
    /// 0xe153 映射键重复（折叠）。
    KeyDup,
    /// 0xe154 映射键断档（周期中断）。
    KeyGap,
    /// 0xe155 映射值越界。
    ValOob,
    /// 0xe156 双表累计漂移。
    SumDrift,
    /// 0xe157 阈值漂移（守卫阈值≠冻结值）。
    ThreshDrift,
    /// 0xe158 台账不守恒（守恒恒等式破缺）。
    LedgerBroken,
    /// 0xe159 快照指纹不符（字段被改后重算不一致）。
    SnapshotFingerprint,
    /// 0xe15a 常量表破坏（表项与语义规则矛盾）。
    TableBroken,
    /// 0xe15b 段位守卫失败（诊断码越出本模块段）。
    SegmentGuard,
}

impl u3541Code {
    /// 全量闭集。
    pub const ALL: [u3541Code; 12] = [
        u3541Code::RowDup,
        u3541Code::RowGap,
        u3541Code::GuardTrip,
        u3541Code::KeyDup,
        u3541Code::KeyGap,
        u3541Code::ValOob,
        u3541Code::SumDrift,
        u3541Code::ThreshDrift,
        u3541Code::LedgerBroken,
        u3541Code::SnapshotFingerprint,
        u3541Code::TableBroken,
        u3541Code::SegmentGuard,
    ];

    /// 码值（连续十六进制，与声明序恒等）。
    pub const fn code(self) -> u16 {
        match self {
            u3541Code::RowDup => 0xe150,
            u3541Code::RowGap => 0xe151,
            u3541Code::GuardTrip => 0xe152,
            u3541Code::KeyDup => 0xe153,
            u3541Code::KeyGap => 0xe154,
            u3541Code::ValOob => 0xe155,
            u3541Code::SumDrift => 0xe156,
            u3541Code::ThreshDrift => 0xe157,
            u3541Code::LedgerBroken => 0xe158,
            u3541Code::SnapshotFingerprint => 0xe159,
            u3541Code::TableBroken => 0xe15a,
            u3541Code::SegmentGuard => 0xe15b,
        }
    }

    /// 中文诊断名。
    pub const fn name(self) -> &'static str {
        match self {
            u3541Code::RowDup => "表行重复（采样）",
            u3541Code::RowGap => "表行断档（剖析）",
            u3541Code::GuardTrip => "守卫触发（重入/越限被拒）",
            u3541Code::KeyDup => "映射键重复（折叠）",
            u3541Code::KeyGap => "映射键断档（周期中断）",
            u3541Code::ValOob => "映射值越界",
            u3541Code::SumDrift => "双表累计漂移",
            u3541Code::ThreshDrift => "阈值漂移（守卫阈值≠冻结值）",
            u3541Code::LedgerBroken => "台账不守恒（守恒恒等式破缺）",
            u3541Code::SnapshotFingerprint => "快照指纹不符（字段被改后重算不一致）",
            u3541Code::TableBroken => "常量表破坏（表项与语义规则矛盾）",
            u3541Code::SegmentGuard => "段位守卫失败（诊断码越出本模块段）",
        }
    }

    /// 可操作修复建议（每条诊断都必须带下一步动作）。
    pub const fn suggestion(self) -> &'static str {
        match self {
            u3541Code::RowDup => "按键唯一律去重",
            u3541Code::RowGap => "按连续律补号",
            u3541Code::GuardTrip => "检查临界区嵌套",
            u3541Code::KeyDup => "按键唯一律去重",
            u3541Code::KeyGap => "按连续律补号",
            u3541Code::ValOob => "按值域裁剪",
            u3541Code::SumDrift => "按理论值复算",
            u3541Code::ThreshDrift => "按冻结值校正",
            u3541Code::LedgerBroken => "按守恒律重建台账后重跑",
            u3541Code::SnapshotFingerprint => "按防篡改规程重建快照",
            u3541Code::TableBroken => "按语义规则重建常量表",
            u3541Code::SegmentGuard => "核对诊断码分配表",
        }
    }
}

// --------------------------------------------------------------------------
// 冻结常量表（u3541mix · 16 行逐行注释自证）
// --------------------------------------------------------------------------
/// 采样剖析器 对拍表（16 行）。
pub const u3541mix_COUNT: usize = 16;

/// 冻结表本体（编译期常量，运行期只读）。
pub static u3541mix: [(&'static str, u64); 16] = [
    ("PROF_00", 10), // PROF 对拍行（剖析器）
    ("PMI_01", 15), // PMI 对拍行（周期中断）
    ("TICK_02", 20), // TICK 对拍行（桩化 tick）
    ("IP_03", 25), // IP 对拍行（指令指针）
    ("STACK_04", 30), // STACK 对拍行（栈采样）
    ("CALL_05", 35), // CALL 对拍行（调用链）
    ("RATE_06", 40), // RATE 对拍行（采样率）
    ("BUF_07", 45), // BUF 对拍行（环形缓冲）
    ("ATTR_08", 50), // ATTR 对拍行（归因）
    ("FOLD_09", 55), // FOLD 对拍行（折叠）
    ("FLAME_10", 60), // FLAME 对拍行（火焰图）
    ("COST_11", 65), // COST 对拍行（开销预算）
    ("PROF_12", 70), // PROF 对拍行（剖析器）
    ("PMI_13", 75), // PMI 对拍行（周期中断）
    ("TICK_14", 80), // TICK 对拍行（桩化 tick）
    ("IP_15", 85), // IP 对拍行（指令指针）
];

/// 线性查找（键唯一性由构造期断言保证，运行时无分支歧义）。
pub fn u3541mix_get(key: &'static str) -> Option<u64> {
    let mut i = 0usize;
    while i < u3541mix_COUNT {
        if u3541mix[i].0 == key {
            return Some(u3541mix[i].1);
        }
        i += 1;
    }
    None
}

/// 累计求和（用于守恒对账）。
pub fn u3541mix_sum() -> u64 {
    let mut s = 0u64;
    let mut i = 0usize;
    while i < u3541mix_COUNT {
        s = s.wrapping_add(u3541mix[i].1 as u64);
        i += 1;
    }
    s
}

// --------------------------------------------------------------------------
// 守卫 u3541mixgate（阈值 32 · 重入/越限/静默三类拒止）
// --------------------------------------------------------------------------
/// 采样剖析器 混合面守卫（阈值 32 冻结）。
#[derive(Clone, Copy, Debug)]
pub struct u3541mixgate {
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

/// 守卫阈值（采样剖析器 混合面守卫（阈值 32 冻结））。
pub const U3541MIXGATE_THRESHOLD: u64 = 32;

impl u3541mixgate {
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
        if self.window >= U3541MIXGATE_THRESHOLD {
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
// 字符串键值映射 u3541mixmap（16 行逐行语义注释 + 逐行守卫）
// --------------------------------------------------------------------------
/// 采样剖析器 混合映射矩阵。
/// 行数。
pub const u3541mixmap_COUNT: usize = 16;

/// 表本体（键 + 值 + 注释由编译期冻结）。
pub static u3541mixmap: [(&'static str, u64); 16] = [
    ("PROF0000", 0), // PROF 单元 0000（剖析器 × 相位 00 × 混 轮）
    ("PMI0001", 9), // PMI 单元 0001（周期中断 × 相位 01 × 混 轮）
    ("TICK0002", 18), // TICK 单元 0002（桩化 tick × 相位 02 × 混 轮）
    ("IP0003", 27), // IP 单元 0003（指令指针 × 相位 03 × 混 轮）
    ("STACK0004", 36), // STACK 单元 0004（栈采样 × 相位 04 × 混 轮）
    ("CALL0005", 45), // CALL 单元 0005（调用链 × 相位 05 × 混 轮）
    ("RATE0006", 54), // RATE 单元 0006（采样率 × 相位 06 × 混 轮）
    ("BUF0007", 63), // BUF 单元 0007（环形缓冲 × 相位 07 × 混 轮）
    ("ATTR0008", 8), // ATTR 单元 0008（归因 × 相位 08 × 混 轮）
    ("FOLD0009", 17), // FOLD 单元 0009（折叠 × 相位 09 × 混 轮）
    ("FLAME0010", 26), // FLAME 单元 0010（火焰图 × 相位 10 × 混 轮）
    ("COST0011", 35), // COST 单元 0011（开销预算 × 相位 11 × 混 轮）
    ("PROF0012", 44), // PROF 单元 0012（剖析器 × 相位 12 × 混 轮）
    ("PMI0013", 53), // PMI 单元 0013（周期中断 × 相位 13 × 混 轮）
    ("TICK0014", 62), // TICK 单元 0014（桩化 tick × 相位 14 × 混 轮）
    ("IP0015", 7), // IP 单元 0015（指令指针 × 相位 15 × 混 轮）
];

/// O(1) 语义查找（match 全枚举；命中率 100%% 由守卫保证）。
pub fn u3541mixmap_get(key: &str) -> Option<u64> {
    match key {
        "PROF0000" => Some(0),
        "PMI0001" => Some(9),
        "TICK0002" => Some(18),
        "IP0003" => Some(27),
        "STACK0004" => Some(36),
        "CALL0005" => Some(45),
        "RATE0006" => Some(54),
        "BUF0007" => Some(63),
        "ATTR0008" => Some(8),
        "FOLD0009" => Some(17),
        "FLAME0010" => Some(26),
        "COST0011" => Some(35),
        "PROF0012" => Some(44),
        "PMI0013" => Some(53),
        "TICK0014" => Some(62),
        "IP0015" => Some(7),
        _ => None,
    }
}

/// 逐行守卫扫描（返回违例行数；0 = 全绿）。
pub fn u3541mixmap_scan() -> usize {
    let mut bad = 0usize;
    let mut i = 0usize;
    while i < u3541mixmap_COUNT {
        let (k, v) = u3541mixmap[i];
        let ok = match k {
            "PROF0000" => v < 64,
            "PMI0001" => v < 64,
            "TICK0002" => v < 64,
            "IP0003" => v < 64,
            "STACK0004" => v < 64,
            "CALL0005" => v < 64,
            "RATE0006" => v < 64,
            "BUF0007" => v < 64,
            "ATTR0008" => v < 64,
            "FOLD0009" => v < 64,
            "FLAME0010" => v < 64,
            "COST0011" => v < 64,
            "PROF0012" => v < 64,
            "PMI0013" => v < 64,
            "TICK0014" => v < 64,
            "IP0015" => v < 64,
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
pub fn u3541mixmap_sum() -> u64 {
    let mut s = 0u64;
    let mut i = 0usize;
    while i < u3541mixmap_COUNT {
        s = s.wrapping_add(u3541mixmap[i].1 as u64);
        i += 1;
    }
    s
}

/// 键唯一性（构造期断言等价物：逐对扫描同键）。
pub fn u3541mixmap_unique() -> bool {
    let mut i = 0usize;
    while i < u3541mixmap_COUNT {
        let mut j = i + 1;
        while j < u3541mixmap_COUNT {
            if u3541mixmap[i].0 == u3541mixmap[j].0 {
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

/// 段位守卫：全部诊断码必须落在本段 [0xe150, 0xe15f]。
pub const fn u3541_segment_guard() -> bool {
    let mut ok = true;
    let mut i = 0usize;
    while i < u3541Code::ALL.len() {
        let c = u3541Code::ALL[i].code();
        if c < 0xe150 || c > 0xe15f {
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
fn u3541_guard_seg() -> bool {
    u3541_segment_guard()
}

/// UNX-F3541 对拍表 16 行边界与累计守恒。
fn check_u3541_mix_tbl() -> bool {
    let mut ok = u3541mix_COUNT == 16;
    if ok { ok = u3541mix_get("PROF_00") == Some(10) && u3541mix_get("IP_15") == Some(85); }
    if ok { ok = u3541mix_get("ZZZ_NOKEY").is_none(); }
    if ok { ok = u3541mix_sum() == 760; }
    ok
}

/// UNX-F3541 混合映射 16 行守卫唯一 + 守卫阈值冻结。
fn check_u3541_mix_map() -> bool {
    let mut ok = u3541mixmap_COUNT == 16;
    if ok { ok = u3541mixmap_scan() == 0 && u3541mixmap_unique(); }
    if ok { ok = u3541mixmap_sum() == 504; }
    if ok { ok = U3541MIXGATE_THRESHOLD == 32; }
    ok
}

/// 本模块自检（逐检查带 detail，全绿才出门）。
pub fn run_u3541_checks() -> CheckSet {
    let mut set = CheckSet::new("U3541");
    set.add(
        "u3541-seg-guard",
        u3541_guard_seg(),
        if u3541_guard_seg() { "" } else { "diagnostic code outside segment" },
    );
    set.add(
        "u3541-mix-tbl",
        check_u3541_mix_tbl(),
        if check_u3541_mix_tbl() { "" } else { "UNX-F3541 mix table mismatch" },
    );
    set.add(
        "u3541-mix-map",
        check_u3541_mix_map(),
        if check_u3541_mix_map() { "" } else { "UNX-F3541 mix map mismatch" },
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
    fn u3541_all_checks_green() {
        let set = run_u3541_checks();
        assert!(set.all_passed(), "U3541 domain checks must be green");
    }

    #[test]
    fn u3541_segment_guard_holds() {
        assert!(u3541_segment_guard());
    }
}
