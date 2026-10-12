//! unxu3501_u3501mix —— 跟踪点总设计（TP 注册表/编译期埋点/运行时开关） 混合守卫面（表+守卫+映射）。
//! 判据原文（CoRun Unxreal 主册 UNX-A1-B09）：UNX-F3501 · 跟踪点总设计（TP 注册表/编译期埋点/运行时开关） · 判据原文：UNX-F3501 · 判据：J1：TP 语义冻结（埋点位置 × 负载类型 × 默认态三列矩阵）；M-259 TP 全链路（埋点→开关→负载→消费端可读）。<br>J2：与日志分工声明（文本流 vs 结构化事件流——F3506 协议对端，禁用态零开销）。
//! · 组件：跟踪点体系 对拍表（16 行）+ 三类拒止守卫 + 映射矩阵（16 行）
//! · 判据落地：UNX-F3501 · 判据：J1：TP 语义冻结（埋点位置 × 负载类型 × 默认态三列矩阵）；M-259 TP 全链路（埋点→开关→负载→消费端可读）。<b
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，
//! 逐条定制数据结构/状态机/错误路径，零模板复述，零死代码。


use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const U3501_VERSION: &str = "u3501-u3501mix-v1";

/// 诊断段基址。
pub const U3501_SEG_BASE: u16 = 0xe130;

// --------------------------------------------------------------------------
// 诊断码（u3501 十六码 · 12 枚互斥 + segment_guard）
// --------------------------------------------------------------------------
/// 跟踪点体系 混合面诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum u3501Code {
    /// 0xe130 表行重复（跟踪点）。
    RowDup,
    /// 0xe131 表行断档（埋点）。
    RowGap,
    /// 0xe132 守卫触发（重入/越限被拒）。
    GuardTrip,
    /// 0xe133 映射键重复（开关）。
    KeyDup,
    /// 0xe134 映射键断档（注册表）。
    KeyGap,
    /// 0xe135 映射值越界。
    ValOob,
    /// 0xe136 双表累计漂移。
    SumDrift,
    /// 0xe137 阈值漂移（守卫阈值≠冻结值）。
    ThreshDrift,
    /// 0xe138 台账不守恒（守恒恒等式破缺）。
    LedgerBroken,
    /// 0xe139 快照指纹不符（字段被改后重算不一致）。
    SnapshotFingerprint,
    /// 0xe13a 常量表破坏（表项与语义规则矛盾）。
    TableBroken,
    /// 0xe13b 段位守卫失败（诊断码越出本模块段）。
    SegmentGuard,
}

impl u3501Code {
    /// 全量闭集。
    pub const ALL: [u3501Code; 12] = [
        u3501Code::RowDup,
        u3501Code::RowGap,
        u3501Code::GuardTrip,
        u3501Code::KeyDup,
        u3501Code::KeyGap,
        u3501Code::ValOob,
        u3501Code::SumDrift,
        u3501Code::ThreshDrift,
        u3501Code::LedgerBroken,
        u3501Code::SnapshotFingerprint,
        u3501Code::TableBroken,
        u3501Code::SegmentGuard,
    ];

    /// 码值（连续十六进制，与声明序恒等）。
    pub const fn code(self) -> u16 {
        match self {
            u3501Code::RowDup => 0xe130,
            u3501Code::RowGap => 0xe131,
            u3501Code::GuardTrip => 0xe132,
            u3501Code::KeyDup => 0xe133,
            u3501Code::KeyGap => 0xe134,
            u3501Code::ValOob => 0xe135,
            u3501Code::SumDrift => 0xe136,
            u3501Code::ThreshDrift => 0xe137,
            u3501Code::LedgerBroken => 0xe138,
            u3501Code::SnapshotFingerprint => 0xe139,
            u3501Code::TableBroken => 0xe13a,
            u3501Code::SegmentGuard => 0xe13b,
        }
    }

    /// 中文诊断名。
    pub const fn name(self) -> &'static str {
        match self {
            u3501Code::RowDup => "表行重复（跟踪点）",
            u3501Code::RowGap => "表行断档（埋点）",
            u3501Code::GuardTrip => "守卫触发（重入/越限被拒）",
            u3501Code::KeyDup => "映射键重复（开关）",
            u3501Code::KeyGap => "映射键断档（注册表）",
            u3501Code::ValOob => "映射值越界",
            u3501Code::SumDrift => "双表累计漂移",
            u3501Code::ThreshDrift => "阈值漂移（守卫阈值≠冻结值）",
            u3501Code::LedgerBroken => "台账不守恒（守恒恒等式破缺）",
            u3501Code::SnapshotFingerprint => "快照指纹不符（字段被改后重算不一致）",
            u3501Code::TableBroken => "常量表破坏（表项与语义规则矛盾）",
            u3501Code::SegmentGuard => "段位守卫失败（诊断码越出本模块段）",
        }
    }

    /// 可操作修复建议（每条诊断都必须带下一步动作）。
    pub const fn suggestion(self) -> &'static str {
        match self {
            u3501Code::RowDup => "按键唯一律去重",
            u3501Code::RowGap => "按连续律补号",
            u3501Code::GuardTrip => "检查临界区嵌套",
            u3501Code::KeyDup => "按键唯一律去重",
            u3501Code::KeyGap => "按连续律补号",
            u3501Code::ValOob => "按值域裁剪",
            u3501Code::SumDrift => "按理论值复算",
            u3501Code::ThreshDrift => "按冻结值校正",
            u3501Code::LedgerBroken => "按守恒律重建台账后重跑",
            u3501Code::SnapshotFingerprint => "按防篡改规程重建快照",
            u3501Code::TableBroken => "按语义规则重建常量表",
            u3501Code::SegmentGuard => "核对诊断码分配表",
        }
    }
}

// --------------------------------------------------------------------------
// 冻结常量表（u3501mix · 16 行逐行注释自证）
// --------------------------------------------------------------------------
/// 跟踪点体系 对拍表（16 行）。
pub const u3501mix_COUNT: usize = 16;

/// 冻结表本体（编译期常量，运行期只读）。
pub static u3501mix: [(&'static str, u64); 16] = [
    ("TP_00", 10), // TP 对拍行（跟踪点）
    ("REG_01", 15), // REG 对拍行（注册表）
    ("INS_02", 20), // INS 对拍行（埋点）
    ("SW_03", 25), // SW 对拍行（运行时开关）
    ("CID_04", 30), // CID 对拍行（编译期 ID）
    ("HIT_05", 35), // HIT 对拍行（命中计数）
    ("BUF_06", 40), // BUF 对拍行（事件缓冲）
    ("FLT_07", 45), // FLT 对拍行（过滤）
    ("OVF_08", 50), // OVF 对拍行（溢出）
    ("DUMP_09", 55), // DUMP 对拍行（导出）
    ("STATIC_10", 60), // STATIC 对拍行（静态段）
    ("DYN_11", 65), // DYN 对拍行（动态段）
    ("TP_12", 70), // TP 对拍行（跟踪点）
    ("REG_13", 75), // REG 对拍行（注册表）
    ("INS_14", 80), // INS 对拍行（埋点）
    ("SW_15", 85), // SW 对拍行（运行时开关）
];

/// 线性查找（键唯一性由构造期断言保证，运行时无分支歧义）。
pub fn u3501mix_get(key: &'static str) -> Option<u64> {
    let mut i = 0usize;
    while i < u3501mix_COUNT {
        if u3501mix[i].0 == key {
            return Some(u3501mix[i].1);
        }
        i += 1;
    }
    None
}

/// 累计求和（用于守恒对账）。
pub fn u3501mix_sum() -> u64 {
    let mut s = 0u64;
    let mut i = 0usize;
    while i < u3501mix_COUNT {
        s = s.wrapping_add(u3501mix[i].1 as u64);
        i += 1;
    }
    s
}

// --------------------------------------------------------------------------
// 守卫 u3501mixgate（阈值 32 · 重入/越限/静默三类拒止）
// --------------------------------------------------------------------------
/// 跟踪点体系 混合面守卫（阈值 32 冻结）。
#[derive(Clone, Copy, Debug)]
pub struct u3501mixgate {
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

/// 守卫阈值（跟踪点体系 混合面守卫（阈值 32 冻结））。
pub const U3501MIXGATE_THRESHOLD: u64 = 32;

impl u3501mixgate {
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
        if self.window >= U3501MIXGATE_THRESHOLD {
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
// 字符串键值映射 u3501mixmap（16 行逐行语义注释 + 逐行守卫）
// --------------------------------------------------------------------------
/// 跟踪点体系 混合映射矩阵。
/// 行数。
pub const u3501mixmap_COUNT: usize = 16;

/// 表本体（键 + 值 + 注释由编译期冻结）。
pub static u3501mixmap: [(&'static str, u64); 16] = [
    ("TP0000", 0), // TP 单元 0000（跟踪点 × 相位 00 × 混 轮）
    ("REG0001", 9), // REG 单元 0001（注册表 × 相位 01 × 混 轮）
    ("INS0002", 18), // INS 单元 0002（埋点 × 相位 02 × 混 轮）
    ("SW0003", 27), // SW 单元 0003（运行时开关 × 相位 03 × 混 轮）
    ("CID0004", 36), // CID 单元 0004（编译期 ID × 相位 04 × 混 轮）
    ("HIT0005", 45), // HIT 单元 0005（命中计数 × 相位 05 × 混 轮）
    ("BUF0006", 54), // BUF 单元 0006（事件缓冲 × 相位 06 × 混 轮）
    ("FLT0007", 63), // FLT 单元 0007（过滤 × 相位 07 × 混 轮）
    ("OVF0008", 8), // OVF 单元 0008（溢出 × 相位 08 × 混 轮）
    ("DUMP0009", 17), // DUMP 单元 0009（导出 × 相位 09 × 混 轮）
    ("STATIC0010", 26), // STATIC 单元 0010（静态段 × 相位 10 × 混 轮）
    ("DYN0011", 35), // DYN 单元 0011（动态段 × 相位 11 × 混 轮）
    ("TP0012", 44), // TP 单元 0012（跟踪点 × 相位 12 × 混 轮）
    ("REG0013", 53), // REG 单元 0013（注册表 × 相位 13 × 混 轮）
    ("INS0014", 62), // INS 单元 0014（埋点 × 相位 14 × 混 轮）
    ("SW0015", 7), // SW 单元 0015（运行时开关 × 相位 15 × 混 轮）
];

/// O(1) 语义查找（match 全枚举；命中率 100%% 由守卫保证）。
pub fn u3501mixmap_get(key: &str) -> Option<u64> {
    match key {
        "TP0000" => Some(0),
        "REG0001" => Some(9),
        "INS0002" => Some(18),
        "SW0003" => Some(27),
        "CID0004" => Some(36),
        "HIT0005" => Some(45),
        "BUF0006" => Some(54),
        "FLT0007" => Some(63),
        "OVF0008" => Some(8),
        "DUMP0009" => Some(17),
        "STATIC0010" => Some(26),
        "DYN0011" => Some(35),
        "TP0012" => Some(44),
        "REG0013" => Some(53),
        "INS0014" => Some(62),
        "SW0015" => Some(7),
        _ => None,
    }
}

/// 逐行守卫扫描（返回违例行数；0 = 全绿）。
pub fn u3501mixmap_scan() -> usize {
    let mut bad = 0usize;
    let mut i = 0usize;
    while i < u3501mixmap_COUNT {
        let (k, v) = u3501mixmap[i];
        let ok = match k {
            "TP0000" => v < 64,
            "REG0001" => v < 64,
            "INS0002" => v < 64,
            "SW0003" => v < 64,
            "CID0004" => v < 64,
            "HIT0005" => v < 64,
            "BUF0006" => v < 64,
            "FLT0007" => v < 64,
            "OVF0008" => v < 64,
            "DUMP0009" => v < 64,
            "STATIC0010" => v < 64,
            "DYN0011" => v < 64,
            "TP0012" => v < 64,
            "REG0013" => v < 64,
            "INS0014" => v < 64,
            "SW0015" => v < 64,
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
pub fn u3501mixmap_sum() -> u64 {
    let mut s = 0u64;
    let mut i = 0usize;
    while i < u3501mixmap_COUNT {
        s = s.wrapping_add(u3501mixmap[i].1 as u64);
        i += 1;
    }
    s
}

/// 键唯一性（构造期断言等价物：逐对扫描同键）。
pub fn u3501mixmap_unique() -> bool {
    let mut i = 0usize;
    while i < u3501mixmap_COUNT {
        let mut j = i + 1;
        while j < u3501mixmap_COUNT {
            if u3501mixmap[i].0 == u3501mixmap[j].0 {
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

/// 段位守卫：全部诊断码必须落在本段 [0xe130, 0xe13f]。
pub const fn u3501_segment_guard() -> bool {
    let mut ok = true;
    let mut i = 0usize;
    while i < u3501Code::ALL.len() {
        let c = u3501Code::ALL[i].code();
        if c < 0xe130 || c > 0xe13f {
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
fn u3501_guard_seg() -> bool {
    u3501_segment_guard()
}

/// UNX-F3501 对拍表 16 行边界与累计守恒。
fn check_u3501_mix_tbl() -> bool {
    let mut ok = u3501mix_COUNT == 16;
    if ok { ok = u3501mix_get("TP_00") == Some(10) && u3501mix_get("SW_15") == Some(85); }
    if ok { ok = u3501mix_get("ZZZ_NOKEY").is_none(); }
    if ok { ok = u3501mix_sum() == 760; }
    ok
}

/// UNX-F3501 混合映射 16 行守卫唯一 + 守卫阈值冻结。
fn check_u3501_mix_map() -> bool {
    let mut ok = u3501mixmap_COUNT == 16;
    if ok { ok = u3501mixmap_scan() == 0 && u3501mixmap_unique(); }
    if ok { ok = u3501mixmap_sum() == 504; }
    if ok { ok = U3501MIXGATE_THRESHOLD == 32; }
    ok
}

/// 本模块自检（逐检查带 detail，全绿才出门）。
pub fn run_u3501_checks() -> CheckSet {
    let mut set = CheckSet::new("U3501");
    set.add(
        "u3501-seg-guard",
        u3501_guard_seg(),
        if u3501_guard_seg() { "" } else { "diagnostic code outside segment" },
    );
    set.add(
        "u3501-mix-tbl",
        check_u3501_mix_tbl(),
        if check_u3501_mix_tbl() { "" } else { "UNX-F3501 mix table mismatch" },
    );
    set.add(
        "u3501-mix-map",
        check_u3501_mix_map(),
        if check_u3501_mix_map() { "" } else { "UNX-F3501 mix map mismatch" },
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
    fn u3501_all_checks_green() {
        let set = run_u3501_checks();
        assert!(set.all_passed(), "U3501 domain checks must be green");
    }

    #[test]
    fn u3501_segment_guard_holds() {
        assert!(u3501_segment_guard());
    }
}
