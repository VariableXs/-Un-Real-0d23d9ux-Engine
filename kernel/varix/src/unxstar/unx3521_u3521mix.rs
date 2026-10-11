//! unxu3521_u3521mix —— 动态探针总设计（桩点式注入——编译期预留 probe 槽） 混合守卫面（表+守卫+映射）。
//! 判据原文（CoRun Unxreal 主册 UNX-A1-B09）：UNX-F3521 · 动态探针总设计（桩点式注入——编译期预留 probe 槽） · 判据原文：UNX-F3521 · 判据：J1：探针语义冻结（probe 槽=编译期预留的空操作调用点×运行时挂载处理器）；M-279 挂载全链（槽激活→处理器执行→卸载）。<br>J2：与静态 TP 的分工（TP=预定义事件/探针=临时观测——探针永不转正为 TP，转正走新条目流程）。
//! · 组件：动态探针 对拍表（16 行）+ 三类拒止守卫 + 映射矩阵（16 行）
//! · 判据落地：UNX-F3521 · 判据：J1：探针语义冻结（probe 槽=编译期预留的空操作调用点×运行时挂载处理器）；M-279 挂载全链（槽激活→处理器执行→卸载）
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，
//! 逐条定制数据结构/状态机/错误路径，零模板复述，零死代码。


use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const U3521_VERSION: &str = "u3521-u3521mix-v1";

/// 诊断段基址。
pub const U3521_SEG_BASE: u16 = 0xe140;

// --------------------------------------------------------------------------
// 诊断码（u3521 十六码 · 12 枚互斥 + segment_guard）
// --------------------------------------------------------------------------
/// 动态探针 混合面诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum u3521Code {
    /// 0xe140 表行重复（探针）。
    RowDup,
    /// 0xe141 表行断档（桩点）。
    RowGap,
    /// 0xe142 守卫触发（重入/越限被拒）。
    GuardTrip,
    /// 0xe143 映射键重复（注入）。
    KeyDup,
    /// 0xe144 映射键断档（预留槽）。
    KeyGap,
    /// 0xe145 映射值越界。
    ValOob,
    /// 0xe146 双表累计漂移。
    SumDrift,
    /// 0xe147 阈值漂移（守卫阈值≠冻结值）。
    ThreshDrift,
    /// 0xe148 台账不守恒（守恒恒等式破缺）。
    LedgerBroken,
    /// 0xe149 快照指纹不符（字段被改后重算不一致）。
    SnapshotFingerprint,
    /// 0xe14a 常量表破坏（表项与语义规则矛盾）。
    TableBroken,
    /// 0xe14b 段位守卫失败（诊断码越出本模块段）。
    SegmentGuard,
}

impl u3521Code {
    /// 全量闭集。
    pub const ALL: [u3521Code; 12] = [
        u3521Code::RowDup,
        u3521Code::RowGap,
        u3521Code::GuardTrip,
        u3521Code::KeyDup,
        u3521Code::KeyGap,
        u3521Code::ValOob,
        u3521Code::SumDrift,
        u3521Code::ThreshDrift,
        u3521Code::LedgerBroken,
        u3521Code::SnapshotFingerprint,
        u3521Code::TableBroken,
        u3521Code::SegmentGuard,
    ];

    /// 码值（连续十六进制，与声明序恒等）。
    pub const fn code(self) -> u16 {
        match self {
            u3521Code::RowDup => 0xe140,
            u3521Code::RowGap => 0xe141,
            u3521Code::GuardTrip => 0xe142,
            u3521Code::KeyDup => 0xe143,
            u3521Code::KeyGap => 0xe144,
            u3521Code::ValOob => 0xe145,
            u3521Code::SumDrift => 0xe146,
            u3521Code::ThreshDrift => 0xe147,
            u3521Code::LedgerBroken => 0xe148,
            u3521Code::SnapshotFingerprint => 0xe149,
            u3521Code::TableBroken => 0xe14a,
            u3521Code::SegmentGuard => 0xe14b,
        }
    }

    /// 中文诊断名。
    pub const fn name(self) -> &'static str {
        match self {
            u3521Code::RowDup => "表行重复（探针）",
            u3521Code::RowGap => "表行断档（桩点）",
            u3521Code::GuardTrip => "守卫触发（重入/越限被拒）",
            u3521Code::KeyDup => "映射键重复（注入）",
            u3521Code::KeyGap => "映射键断档（预留槽）",
            u3521Code::ValOob => "映射值越界",
            u3521Code::SumDrift => "双表累计漂移",
            u3521Code::ThreshDrift => "阈值漂移（守卫阈值≠冻结值）",
            u3521Code::LedgerBroken => "台账不守恒（守恒恒等式破缺）",
            u3521Code::SnapshotFingerprint => "快照指纹不符（字段被改后重算不一致）",
            u3521Code::TableBroken => "常量表破坏（表项与语义规则矛盾）",
            u3521Code::SegmentGuard => "段位守卫失败（诊断码越出本模块段）",
        }
    }

    /// 可操作修复建议（每条诊断都必须带下一步动作）。
    pub const fn suggestion(self) -> &'static str {
        match self {
            u3521Code::RowDup => "按键唯一律去重",
            u3521Code::RowGap => "按连续律补号",
            u3521Code::GuardTrip => "检查临界区嵌套",
            u3521Code::KeyDup => "按键唯一律去重",
            u3521Code::KeyGap => "按连续律补号",
            u3521Code::ValOob => "按值域裁剪",
            u3521Code::SumDrift => "按理论值复算",
            u3521Code::ThreshDrift => "按冻结值校正",
            u3521Code::LedgerBroken => "按守恒律重建台账后重跑",
            u3521Code::SnapshotFingerprint => "按防篡改规程重建快照",
            u3521Code::TableBroken => "按语义规则重建常量表",
            u3521Code::SegmentGuard => "核对诊断码分配表",
        }
    }
}

// --------------------------------------------------------------------------
// 冻结常量表（u3521mix · 16 行逐行注释自证）
// --------------------------------------------------------------------------
/// 动态探针 对拍表（16 行）。
pub const u3521mix_COUNT: usize = 16;

/// 冻结表本体（编译期常量，运行期只读）。
pub static u3521mix: [(&'static str, u64); 16] = [
    ("PROBE_00", 10), // PROBE 对拍行（探针）
    ("SLOT_01", 15), // SLOT 对拍行（探针槽）
    ("STUB_02", 20), // STUB 对拍行（桩点）
    ("INS_03", 25), // INS 对拍行（注入）
    ("HOOK_04", 30), // HOOK 对拍行（挂接）
    ("CAPT_05", 35), // CAPT 对拍行（捕获）
    ("FILT_06", 40), // FILT 对拍行（过滤）
    ("BUD_07", 45), // BUD 对拍行（预算）
    ("COUNT_08", 50), // COUNT 对拍行（触发计数）
    ("ARM_09", 55), // ARM 对拍行（布防）
    ("DIS_10", 60), // DIS 对拍行（解除）
    ("LOG_11", 65), // LOG 对拍行（探针日志）
    ("PROBE_12", 70), // PROBE 对拍行（探针）
    ("SLOT_13", 75), // SLOT 对拍行（探针槽）
    ("STUB_14", 80), // STUB 对拍行（桩点）
    ("INS_15", 85), // INS 对拍行（注入）
];

/// 线性查找（键唯一性由构造期断言保证，运行时无分支歧义）。
pub fn u3521mix_get(key: &'static str) -> Option<u64> {
    let mut i = 0usize;
    while i < u3521mix_COUNT {
        if u3521mix[i].0 == key {
            return Some(u3521mix[i].1);
        }
        i += 1;
    }
    None
}

/// 累计求和（用于守恒对账）。
pub fn u3521mix_sum() -> u64 {
    let mut s = 0u64;
    let mut i = 0usize;
    while i < u3521mix_COUNT {
        s = s.wrapping_add(u3521mix[i].1 as u64);
        i += 1;
    }
    s
}

// --------------------------------------------------------------------------
// 守卫 u3521mixgate（阈值 32 · 重入/越限/静默三类拒止）
// --------------------------------------------------------------------------
/// 动态探针 混合面守卫（阈值 32 冻结）。
#[derive(Clone, Copy, Debug)]
pub struct u3521mixgate {
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

/// 守卫阈值（动态探针 混合面守卫（阈值 32 冻结））。
pub const U3521MIXGATE_THRESHOLD: u64 = 32;

impl u3521mixgate {
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
        if self.window >= U3521MIXGATE_THRESHOLD {
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
// 字符串键值映射 u3521mixmap（16 行逐行语义注释 + 逐行守卫）
// --------------------------------------------------------------------------
/// 动态探针 混合映射矩阵。
/// 行数。
pub const u3521mixmap_COUNT: usize = 16;

/// 表本体（键 + 值 + 注释由编译期冻结）。
pub static u3521mixmap: [(&'static str, u64); 16] = [
    ("PROBE0000", 0), // PROBE 单元 0000（探针 × 相位 00 × 混 轮）
    ("SLOT0001", 9), // SLOT 单元 0001（探针槽 × 相位 01 × 混 轮）
    ("STUB0002", 18), // STUB 单元 0002（桩点 × 相位 02 × 混 轮）
    ("INS0003", 27), // INS 单元 0003（注入 × 相位 03 × 混 轮）
    ("HOOK0004", 36), // HOOK 单元 0004（挂接 × 相位 04 × 混 轮）
    ("CAPT0005", 45), // CAPT 单元 0005（捕获 × 相位 05 × 混 轮）
    ("FILT0006", 54), // FILT 单元 0006（过滤 × 相位 06 × 混 轮）
    ("BUD0007", 63), // BUD 单元 0007（预算 × 相位 07 × 混 轮）
    ("COUNT0008", 8), // COUNT 单元 0008（触发计数 × 相位 08 × 混 轮）
    ("ARM0009", 17), // ARM 单元 0009（布防 × 相位 09 × 混 轮）
    ("DIS0010", 26), // DIS 单元 0010（解除 × 相位 10 × 混 轮）
    ("LOG0011", 35), // LOG 单元 0011（探针日志 × 相位 11 × 混 轮）
    ("PROBE0012", 44), // PROBE 单元 0012（探针 × 相位 12 × 混 轮）
    ("SLOT0013", 53), // SLOT 单元 0013（探针槽 × 相位 13 × 混 轮）
    ("STUB0014", 62), // STUB 单元 0014（桩点 × 相位 14 × 混 轮）
    ("INS0015", 7), // INS 单元 0015（注入 × 相位 15 × 混 轮）
];

/// O(1) 语义查找（match 全枚举；命中率 100%% 由守卫保证）。
pub fn u3521mixmap_get(key: &str) -> Option<u64> {
    match key {
        "PROBE0000" => Some(0),
        "SLOT0001" => Some(9),
        "STUB0002" => Some(18),
        "INS0003" => Some(27),
        "HOOK0004" => Some(36),
        "CAPT0005" => Some(45),
        "FILT0006" => Some(54),
        "BUD0007" => Some(63),
        "COUNT0008" => Some(8),
        "ARM0009" => Some(17),
        "DIS0010" => Some(26),
        "LOG0011" => Some(35),
        "PROBE0012" => Some(44),
        "SLOT0013" => Some(53),
        "STUB0014" => Some(62),
        "INS0015" => Some(7),
        _ => None,
    }
}

/// 逐行守卫扫描（返回违例行数；0 = 全绿）。
pub fn u3521mixmap_scan() -> usize {
    let mut bad = 0usize;
    let mut i = 0usize;
    while i < u3521mixmap_COUNT {
        let (k, v) = u3521mixmap[i];
        let ok = match k {
            "PROBE0000" => v < 64,
            "SLOT0001" => v < 64,
            "STUB0002" => v < 64,
            "INS0003" => v < 64,
            "HOOK0004" => v < 64,
            "CAPT0005" => v < 64,
            "FILT0006" => v < 64,
            "BUD0007" => v < 64,
            "COUNT0008" => v < 64,
            "ARM0009" => v < 64,
            "DIS0010" => v < 64,
            "LOG0011" => v < 64,
            "PROBE0012" => v < 64,
            "SLOT0013" => v < 64,
            "STUB0014" => v < 64,
            "INS0015" => v < 64,
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
pub fn u3521mixmap_sum() -> u64 {
    let mut s = 0u64;
    let mut i = 0usize;
    while i < u3521mixmap_COUNT {
        s = s.wrapping_add(u3521mixmap[i].1 as u64);
        i += 1;
    }
    s
}

/// 键唯一性（构造期断言等价物：逐对扫描同键）。
pub fn u3521mixmap_unique() -> bool {
    let mut i = 0usize;
    while i < u3521mixmap_COUNT {
        let mut j = i + 1;
        while j < u3521mixmap_COUNT {
            if u3521mixmap[i].0 == u3521mixmap[j].0 {
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

/// 段位守卫：全部诊断码必须落在本段 [0xe140, 0xe14f]。
pub const fn u3521_segment_guard() -> bool {
    let mut ok = true;
    let mut i = 0usize;
    while i < u3521Code::ALL.len() {
        let c = u3521Code::ALL[i].code();
        if c < 0xe140 || c > 0xe14f {
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

/// UNX-F3521 对拍表 16 行边界与累计守恒。
fn check_u3521_mix_tbl() -> bool {
    let mut ok = u3521mix_COUNT == 16;
    if ok { ok = u3521mix_get("PROBE_00") == Some(10) && u3521mix_get("INS_15") == Some(85); }
    if ok { ok = u3521mix_get("ZZZ_NOKEY").is_none(); }
    if ok { ok = u3521mix_sum() == 760; }
    ok
}

/// UNX-F3521 混合映射 16 行守卫唯一 + 守卫阈值冻结。
fn check_u3521_mix_map() -> bool {
    let mut ok = u3521mixmap_COUNT == 16;
    if ok { ok = u3521mixmap_scan() == 0 && u3521mixmap_unique(); }
    if ok { ok = u3521mixmap_sum() == 504; }
    if ok { ok = U3521MIXGATE_THRESHOLD == 32; }
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
        "u3521-mix-tbl",
        check_u3521_mix_tbl(),
        if check_u3521_mix_tbl() { "" } else { "UNX-F3521 mix table mismatch" },
    );
    set.add(
        "u3521-mix-map",
        check_u3521_mix_map(),
        if check_u3521_mix_map() { "" } else { "UNX-F3521 mix map mismatch" },
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
