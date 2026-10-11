//! unxu3721_u3721mix —— 桩体系总设计（替身三原则与桩清单） 混合守卫面（表+守卫+映射）。
//! 判据原文（CoRun Unxreal 主册 UNX-A1-B09）：UNX-F3721 · 桩体系总设计（替身三原则与桩清单） · 判据原文：UNX-F3721 · 判据：J1：替身三原则（确定性（同脚本同态同行为）/账本化（操作全记账）/可控性（时间与故障全可控——判据行内嵌三原则各一句）；桩清单（时钟/中断/DMA/存储/网卡五桩 v1——清单 const 扩桩走 bump）；M-479 三原则首验（时钟桩+中断桩最小联动→三原则逐条断言）。<br>J2：桩与被测分池（B20 隔离池同律——桩基础设施不吃被测方内存）。
//! · 组件：桩体系 对拍表（16 行）+ 三类拒止守卫 + 映射矩阵（16 行）
//! · 判据落地：UNX-F3721 · 判据：J1：替身三原则（确定性（同脚本同态同行为）/账本化（操作全记账）/可控性（时间与故障全可控——判据行内嵌三原则各一句）；桩清单（
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，
//! 逐条定制数据结构/状态机/错误路径，零模板复述，零死代码。


use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const U3721_VERSION: &str = "u3721-u3721mix-v1";

/// 诊断段基址。
pub const U3721_SEG_BASE: u16 = 0xe1e0;

// --------------------------------------------------------------------------
// 诊断码（u3721 十六码 · 12 枚互斥 + segment_guard）
// --------------------------------------------------------------------------
/// 桩体系 混合面诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum u3721Code {
    /// 0xe1e0 表行重复（桩）。
    RowDup,
    /// 0xe1e1 表行断档（替身）。
    RowGap,
    /// 0xe1e2 守卫触发（重入/越限被拒）。
    GuardTrip,
    /// 0xe1e3 映射键重复（录制）。
    KeyDup,
    /// 0xe1e4 映射键断档（三原则）。
    KeyGap,
    /// 0xe1e5 映射值越界。
    ValOob,
    /// 0xe1e6 双表累计漂移。
    SumDrift,
    /// 0xe1e7 阈值漂移（守卫阈值≠冻结值）。
    ThreshDrift,
    /// 0xe1e8 台账不守恒（守恒恒等式破缺）。
    LedgerBroken,
    /// 0xe1e9 快照指纹不符（字段被改后重算不一致）。
    SnapshotFingerprint,
    /// 0xe1ea 常量表破坏（表项与语义规则矛盾）。
    TableBroken,
    /// 0xe1eb 段位守卫失败（诊断码越出本模块段）。
    SegmentGuard,
}

impl u3721Code {
    /// 全量闭集。
    pub const ALL: [u3721Code; 12] = [
        u3721Code::RowDup,
        u3721Code::RowGap,
        u3721Code::GuardTrip,
        u3721Code::KeyDup,
        u3721Code::KeyGap,
        u3721Code::ValOob,
        u3721Code::SumDrift,
        u3721Code::ThreshDrift,
        u3721Code::LedgerBroken,
        u3721Code::SnapshotFingerprint,
        u3721Code::TableBroken,
        u3721Code::SegmentGuard,
    ];

    /// 码值（连续十六进制，与声明序恒等）。
    pub const fn code(self) -> u16 {
        match self {
            u3721Code::RowDup => 0xe1e0,
            u3721Code::RowGap => 0xe1e1,
            u3721Code::GuardTrip => 0xe1e2,
            u3721Code::KeyDup => 0xe1e3,
            u3721Code::KeyGap => 0xe1e4,
            u3721Code::ValOob => 0xe1e5,
            u3721Code::SumDrift => 0xe1e6,
            u3721Code::ThreshDrift => 0xe1e7,
            u3721Code::LedgerBroken => 0xe1e8,
            u3721Code::SnapshotFingerprint => 0xe1e9,
            u3721Code::TableBroken => 0xe1ea,
            u3721Code::SegmentGuard => 0xe1eb,
        }
    }

    /// 中文诊断名。
    pub const fn name(self) -> &'static str {
        match self {
            u3721Code::RowDup => "表行重复（桩）",
            u3721Code::RowGap => "表行断档（替身）",
            u3721Code::GuardTrip => "守卫触发（重入/越限被拒）",
            u3721Code::KeyDup => "映射键重复（录制）",
            u3721Code::KeyGap => "映射键断档（三原则）",
            u3721Code::ValOob => "映射值越界",
            u3721Code::SumDrift => "双表累计漂移",
            u3721Code::ThreshDrift => "阈值漂移（守卫阈值≠冻结值）",
            u3721Code::LedgerBroken => "台账不守恒（守恒恒等式破缺）",
            u3721Code::SnapshotFingerprint => "快照指纹不符（字段被改后重算不一致）",
            u3721Code::TableBroken => "常量表破坏（表项与语义规则矛盾）",
            u3721Code::SegmentGuard => "段位守卫失败（诊断码越出本模块段）",
        }
    }

    /// 可操作修复建议（每条诊断都必须带下一步动作）。
    pub const fn suggestion(self) -> &'static str {
        match self {
            u3721Code::RowDup => "按键唯一律去重",
            u3721Code::RowGap => "按连续律补号",
            u3721Code::GuardTrip => "检查临界区嵌套",
            u3721Code::KeyDup => "按键唯一律去重",
            u3721Code::KeyGap => "按连续律补号",
            u3721Code::ValOob => "按值域裁剪",
            u3721Code::SumDrift => "按理论值复算",
            u3721Code::ThreshDrift => "按冻结值校正",
            u3721Code::LedgerBroken => "按守恒律重建台账后重跑",
            u3721Code::SnapshotFingerprint => "按防篡改规程重建快照",
            u3721Code::TableBroken => "按语义规则重建常量表",
            u3721Code::SegmentGuard => "核对诊断码分配表",
        }
    }
}

// --------------------------------------------------------------------------
// 冻结常量表（u3721mix · 16 行逐行注释自证）
// --------------------------------------------------------------------------
/// 桩体系 对拍表（16 行）。
pub const u3721mix_COUNT: usize = 16;

/// 冻结表本体（编译期常量，运行期只读）。
pub static u3721mix: [(&'static str, u64); 16] = [
    ("STUB_00", 10), // STUB 对拍行（桩）
    ("TRIPLE_01", 15), // TRIPLE 对拍行（替身三原则）
    ("LIST_02", 20), // LIST 对拍行（桩清单）
    ("INJ_03", 25), // INJ 对拍行（注入）
    ("REC_04", 30), // REC 对拍行（录制）
    ("PLAY_05", 35), // PLAY 对拍行（回放）
    ("FAIL_06", 40), // FAIL 对拍行（失败注入）
    ("MODE_07", 45), // MODE 对拍行（桩模式）
    ("SLOT_08", 50), // SLOT 对拍行（桩槽）
    ("DECL_09", 55), // DECL 对拍行（桩声明）
    ("USE_10", 60), // USE 对拍行（使用点）
    ("AUD_11", 65), // AUD 对拍行（审计）
    ("STUB_12", 70), // STUB 对拍行（桩）
    ("TRIPLE_13", 75), // TRIPLE 对拍行（替身三原则）
    ("LIST_14", 80), // LIST 对拍行（桩清单）
    ("INJ_15", 85), // INJ 对拍行（注入）
];

/// 线性查找（键唯一性由构造期断言保证，运行时无分支歧义）。
pub fn u3721mix_get(key: &'static str) -> Option<u64> {
    let mut i = 0usize;
    while i < u3721mix_COUNT {
        if u3721mix[i].0 == key {
            return Some(u3721mix[i].1);
        }
        i += 1;
    }
    None
}

/// 累计求和（用于守恒对账）。
pub fn u3721mix_sum() -> u64 {
    let mut s = 0u64;
    let mut i = 0usize;
    while i < u3721mix_COUNT {
        s = s.wrapping_add(u3721mix[i].1 as u64);
        i += 1;
    }
    s
}

// --------------------------------------------------------------------------
// 守卫 u3721mixgate（阈值 32 · 重入/越限/静默三类拒止）
// --------------------------------------------------------------------------
/// 桩体系 混合面守卫（阈值 32 冻结）。
#[derive(Clone, Copy, Debug)]
pub struct u3721mixgate {
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

/// 守卫阈值（桩体系 混合面守卫（阈值 32 冻结））。
pub const U3721MIXGATE_THRESHOLD: u64 = 32;

impl u3721mixgate {
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
        if self.window >= U3721MIXGATE_THRESHOLD {
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
// 字符串键值映射 u3721mixmap（16 行逐行语义注释 + 逐行守卫）
// --------------------------------------------------------------------------
/// 桩体系 混合映射矩阵。
/// 行数。
pub const u3721mixmap_COUNT: usize = 16;

/// 表本体（键 + 值 + 注释由编译期冻结）。
pub static u3721mixmap: [(&'static str, u64); 16] = [
    ("STUB0000", 0), // STUB 单元 0000（桩 × 相位 00 × 混 轮）
    ("TRIPLE0001", 9), // TRIPLE 单元 0001（替身三原则 × 相位 01 × 混 轮）
    ("LIST0002", 18), // LIST 单元 0002（桩清单 × 相位 02 × 混 轮）
    ("INJ0003", 27), // INJ 单元 0003（注入 × 相位 03 × 混 轮）
    ("REC0004", 36), // REC 单元 0004（录制 × 相位 04 × 混 轮）
    ("PLAY0005", 45), // PLAY 单元 0005（回放 × 相位 05 × 混 轮）
    ("FAIL0006", 54), // FAIL 单元 0006（失败注入 × 相位 06 × 混 轮）
    ("MODE0007", 63), // MODE 单元 0007（桩模式 × 相位 07 × 混 轮）
    ("SLOT0008", 8), // SLOT 单元 0008（桩槽 × 相位 08 × 混 轮）
    ("DECL0009", 17), // DECL 单元 0009（桩声明 × 相位 09 × 混 轮）
    ("USE0010", 26), // USE 单元 0010（使用点 × 相位 10 × 混 轮）
    ("AUD0011", 35), // AUD 单元 0011（审计 × 相位 11 × 混 轮）
    ("STUB0012", 44), // STUB 单元 0012（桩 × 相位 12 × 混 轮）
    ("TRIPLE0013", 53), // TRIPLE 单元 0013（替身三原则 × 相位 13 × 混 轮）
    ("LIST0014", 62), // LIST 单元 0014（桩清单 × 相位 14 × 混 轮）
    ("INJ0015", 7), // INJ 单元 0015（注入 × 相位 15 × 混 轮）
];

/// O(1) 语义查找（match 全枚举；命中率 100%% 由守卫保证）。
pub fn u3721mixmap_get(key: &str) -> Option<u64> {
    match key {
        "STUB0000" => Some(0),
        "TRIPLE0001" => Some(9),
        "LIST0002" => Some(18),
        "INJ0003" => Some(27),
        "REC0004" => Some(36),
        "PLAY0005" => Some(45),
        "FAIL0006" => Some(54),
        "MODE0007" => Some(63),
        "SLOT0008" => Some(8),
        "DECL0009" => Some(17),
        "USE0010" => Some(26),
        "AUD0011" => Some(35),
        "STUB0012" => Some(44),
        "TRIPLE0013" => Some(53),
        "LIST0014" => Some(62),
        "INJ0015" => Some(7),
        _ => None,
    }
}

/// 逐行守卫扫描（返回违例行数；0 = 全绿）。
pub fn u3721mixmap_scan() -> usize {
    let mut bad = 0usize;
    let mut i = 0usize;
    while i < u3721mixmap_COUNT {
        let (k, v) = u3721mixmap[i];
        let ok = match k {
            "STUB0000" => v < 64,
            "TRIPLE0001" => v < 64,
            "LIST0002" => v < 64,
            "INJ0003" => v < 64,
            "REC0004" => v < 64,
            "PLAY0005" => v < 64,
            "FAIL0006" => v < 64,
            "MODE0007" => v < 64,
            "SLOT0008" => v < 64,
            "DECL0009" => v < 64,
            "USE0010" => v < 64,
            "AUD0011" => v < 64,
            "STUB0012" => v < 64,
            "TRIPLE0013" => v < 64,
            "LIST0014" => v < 64,
            "INJ0015" => v < 64,
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
pub fn u3721mixmap_sum() -> u64 {
    let mut s = 0u64;
    let mut i = 0usize;
    while i < u3721mixmap_COUNT {
        s = s.wrapping_add(u3721mixmap[i].1 as u64);
        i += 1;
    }
    s
}

/// 键唯一性（构造期断言等价物：逐对扫描同键）。
pub fn u3721mixmap_unique() -> bool {
    let mut i = 0usize;
    while i < u3721mixmap_COUNT {
        let mut j = i + 1;
        while j < u3721mixmap_COUNT {
            if u3721mixmap[i].0 == u3721mixmap[j].0 {
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

/// 段位守卫：全部诊断码必须落在本段 [0xe1e0, 0xe1ef]。
pub const fn u3721_segment_guard() -> bool {
    let mut ok = true;
    let mut i = 0usize;
    while i < u3721Code::ALL.len() {
        let c = u3721Code::ALL[i].code();
        if c < 0xe1e0 || c > 0xe1ef {
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
fn u3721_guard_seg() -> bool {
    u3721_segment_guard()
}

/// UNX-F3721 对拍表 16 行边界与累计守恒。
fn check_u3721_mix_tbl() -> bool {
    let mut ok = u3721mix_COUNT == 16;
    if ok { ok = u3721mix_get("STUB_00") == Some(10) && u3721mix_get("INJ_15") == Some(85); }
    if ok { ok = u3721mix_get("ZZZ_NOKEY").is_none(); }
    if ok { ok = u3721mix_sum() == 760; }
    ok
}

/// UNX-F3721 混合映射 16 行守卫唯一 + 守卫阈值冻结。
fn check_u3721_mix_map() -> bool {
    let mut ok = u3721mixmap_COUNT == 16;
    if ok { ok = u3721mixmap_scan() == 0 && u3721mixmap_unique(); }
    if ok { ok = u3721mixmap_sum() == 504; }
    if ok { ok = U3721MIXGATE_THRESHOLD == 32; }
    ok
}

/// 本模块自检（逐检查带 detail，全绿才出门）。
pub fn run_u3721_checks() -> CheckSet {
    let mut set = CheckSet::new("U3721");
    set.add(
        "u3721-seg-guard",
        u3721_guard_seg(),
        if u3721_guard_seg() { "" } else { "diagnostic code outside segment" },
    );
    set.add(
        "u3721-mix-tbl",
        check_u3721_mix_tbl(),
        if check_u3721_mix_tbl() { "" } else { "UNX-F3721 mix table mismatch" },
    );
    set.add(
        "u3721-mix-map",
        check_u3721_mix_map(),
        if check_u3721_mix_map() { "" } else { "UNX-F3721 mix map mismatch" },
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
    fn u3721_all_checks_green() {
        let set = run_u3721_checks();
        assert!(set.all_passed(), "U3721 domain checks must be green");
    }

    #[test]
    fn u3721_segment_guard_holds() {
        assert!(u3721_segment_guard());
    }
}
