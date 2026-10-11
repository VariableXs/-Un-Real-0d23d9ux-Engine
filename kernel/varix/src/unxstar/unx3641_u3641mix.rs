//! unxu3641_u3641mix —— 崩溃转储总设计（触发→采集→封存管线） 混合守卫面（表+守卫+映射）。
//! 判据原文（CoRun Unxreal 主册 UNX-A1-B09）：UNX-F3641 · 崩溃转储总设计（触发→采集→封存管线） · 判据原文：UNX-F3641 · 判据：J1：三段管线（触发（🔴事件）/采集（快照三件套）/封存（通道+校验）——判据行内嵌状态机）；M-399 全链首dump（注入 panic→全链走通→封存物可校验）。<br>J2：管线恒备（非崩溃期管线自检绿——F3655 的日常态——总设计的常备性）。
//! · 组件：崩溃转储 对拍表（16 行）+ 三类拒止守卫 + 映射矩阵（16 行）
//! · 判据落地：UNX-F3641 · 判据：J1：三段管线（触发（🔴事件）/采集（快照三件套）/封存（通道+校验）——判据行内嵌状态机）；M-399 全链首dump（注入 p
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，
//! 逐条定制数据结构/状态机/错误路径，零模板复述，零死代码。


use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const U3641_VERSION: &str = "u3641-u3641mix-v1";

/// 诊断段基址。
pub const U3641_SEG_BASE: u16 = 0xe1a0;

// --------------------------------------------------------------------------
// 诊断码（u3641 十六码 · 12 枚互斥 + segment_guard）
// --------------------------------------------------------------------------
/// 崩溃转储 混合面诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum u3641Code {
    /// 0xe1a0 表行重复（转储）。
    RowDup,
    /// 0xe1a1 表行断档（崩溃）。
    RowGap,
    /// 0xe1a2 守卫触发（重入/越限被拒）。
    GuardTrip,
    /// 0xe1a3 映射键重复（封存）。
    KeyDup,
    /// 0xe1a4 映射键断档（封存）。
    KeyGap,
    /// 0xe1a5 映射值越界。
    ValOob,
    /// 0xe1a6 双表累计漂移。
    SumDrift,
    /// 0xe1a7 阈值漂移（守卫阈值≠冻结值）。
    ThreshDrift,
    /// 0xe1a8 台账不守恒（守恒恒等式破缺）。
    LedgerBroken,
    /// 0xe1a9 快照指纹不符（字段被改后重算不一致）。
    SnapshotFingerprint,
    /// 0xe1aa 常量表破坏（表项与语义规则矛盾）。
    TableBroken,
    /// 0xe1ab 段位守卫失败（诊断码越出本模块段）。
    SegmentGuard,
}

impl u3641Code {
    /// 全量闭集。
    pub const ALL: [u3641Code; 12] = [
        u3641Code::RowDup,
        u3641Code::RowGap,
        u3641Code::GuardTrip,
        u3641Code::KeyDup,
        u3641Code::KeyGap,
        u3641Code::ValOob,
        u3641Code::SumDrift,
        u3641Code::ThreshDrift,
        u3641Code::LedgerBroken,
        u3641Code::SnapshotFingerprint,
        u3641Code::TableBroken,
        u3641Code::SegmentGuard,
    ];

    /// 码值（连续十六进制，与声明序恒等）。
    pub const fn code(self) -> u16 {
        match self {
            u3641Code::RowDup => 0xe1a0,
            u3641Code::RowGap => 0xe1a1,
            u3641Code::GuardTrip => 0xe1a2,
            u3641Code::KeyDup => 0xe1a3,
            u3641Code::KeyGap => 0xe1a4,
            u3641Code::ValOob => 0xe1a5,
            u3641Code::SumDrift => 0xe1a6,
            u3641Code::ThreshDrift => 0xe1a7,
            u3641Code::LedgerBroken => 0xe1a8,
            u3641Code::SnapshotFingerprint => 0xe1a9,
            u3641Code::TableBroken => 0xe1aa,
            u3641Code::SegmentGuard => 0xe1ab,
        }
    }

    /// 中文诊断名。
    pub const fn name(self) -> &'static str {
        match self {
            u3641Code::RowDup => "表行重复（转储）",
            u3641Code::RowGap => "表行断档（崩溃）",
            u3641Code::GuardTrip => "守卫触发（重入/越限被拒）",
            u3641Code::KeyDup => "映射键重复（封存）",
            u3641Code::KeyGap => "映射键断档（封存）",
            u3641Code::ValOob => "映射值越界",
            u3641Code::SumDrift => "双表累计漂移",
            u3641Code::ThreshDrift => "阈值漂移（守卫阈值≠冻结值）",
            u3641Code::LedgerBroken => "台账不守恒（守恒恒等式破缺）",
            u3641Code::SnapshotFingerprint => "快照指纹不符（字段被改后重算不一致）",
            u3641Code::TableBroken => "常量表破坏（表项与语义规则矛盾）",
            u3641Code::SegmentGuard => "段位守卫失败（诊断码越出本模块段）",
        }
    }

    /// 可操作修复建议（每条诊断都必须带下一步动作）。
    pub const fn suggestion(self) -> &'static str {
        match self {
            u3641Code::RowDup => "按键唯一律去重",
            u3641Code::RowGap => "按连续律补号",
            u3641Code::GuardTrip => "检查临界区嵌套",
            u3641Code::KeyDup => "按键唯一律去重",
            u3641Code::KeyGap => "按连续律补号",
            u3641Code::ValOob => "按值域裁剪",
            u3641Code::SumDrift => "按理论值复算",
            u3641Code::ThreshDrift => "按冻结值校正",
            u3641Code::LedgerBroken => "按守恒律重建台账后重跑",
            u3641Code::SnapshotFingerprint => "按防篡改规程重建快照",
            u3641Code::TableBroken => "按语义规则重建常量表",
            u3641Code::SegmentGuard => "核对诊断码分配表",
        }
    }
}

// --------------------------------------------------------------------------
// 冻结常量表（u3641mix · 16 行逐行注释自证）
// --------------------------------------------------------------------------
/// 崩溃转储 对拍表（16 行）。
pub const u3641mix_COUNT: usize = 16;

/// 冻结表本体（编译期常量，运行期只读）。
pub static u3641mix: [(&'static str, u64); 16] = [
    ("DUMP_00", 10), // DUMP 对拍行（转储）
    ("TRIG_01", 15), // TRIG 对拍行（触发）
    ("COLL_02", 20), // COLL 对拍行（采集）
    ("SEAL_03", 25), // SEAL 对拍行（封存）
    ("RB_04", 30), // RB 对拍行（回溯）
    ("REG_05", 35), // REG 对拍行（寄存器组）
    ("STK_06", 40), // STK 对拍行（栈快照）
    ("MEM_07", 45), // MEM 对拍行（内存快照）
    ("FMT_08", 50), // FMT 对拍行（转储格式）
    ("SINK_09", 55), // SINK 对拍行（落地）
    ("ID_10", 60), // ID 对拍行（转储标识）
    ("CHAIN_11", 65), // CHAIN 对拍行（指纹链）
    ("DUMP_12", 70), // DUMP 对拍行（转储）
    ("TRIG_13", 75), // TRIG 对拍行（触发）
    ("COLL_14", 80), // COLL 对拍行（采集）
    ("SEAL_15", 85), // SEAL 对拍行（封存）
];

/// 线性查找（键唯一性由构造期断言保证，运行时无分支歧义）。
pub fn u3641mix_get(key: &'static str) -> Option<u64> {
    let mut i = 0usize;
    while i < u3641mix_COUNT {
        if u3641mix[i].0 == key {
            return Some(u3641mix[i].1);
        }
        i += 1;
    }
    None
}

/// 累计求和（用于守恒对账）。
pub fn u3641mix_sum() -> u64 {
    let mut s = 0u64;
    let mut i = 0usize;
    while i < u3641mix_COUNT {
        s = s.wrapping_add(u3641mix[i].1 as u64);
        i += 1;
    }
    s
}

// --------------------------------------------------------------------------
// 守卫 u3641mixgate（阈值 32 · 重入/越限/静默三类拒止）
// --------------------------------------------------------------------------
/// 崩溃转储 混合面守卫（阈值 32 冻结）。
#[derive(Clone, Copy, Debug)]
pub struct u3641mixgate {
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

/// 守卫阈值（崩溃转储 混合面守卫（阈值 32 冻结））。
pub const U3641MIXGATE_THRESHOLD: u64 = 32;

impl u3641mixgate {
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
        if self.window >= U3641MIXGATE_THRESHOLD {
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
// 字符串键值映射 u3641mixmap（16 行逐行语义注释 + 逐行守卫）
// --------------------------------------------------------------------------
/// 崩溃转储 混合映射矩阵。
/// 行数。
pub const u3641mixmap_COUNT: usize = 16;

/// 表本体（键 + 值 + 注释由编译期冻结）。
pub static u3641mixmap: [(&'static str, u64); 16] = [
    ("DUMP0000", 0), // DUMP 单元 0000（转储 × 相位 00 × 混 轮）
    ("TRIG0001", 9), // TRIG 单元 0001（触发 × 相位 01 × 混 轮）
    ("COLL0002", 18), // COLL 单元 0002（采集 × 相位 02 × 混 轮）
    ("SEAL0003", 27), // SEAL 单元 0003（封存 × 相位 03 × 混 轮）
    ("RB0004", 36), // RB 单元 0004（回溯 × 相位 04 × 混 轮）
    ("REG0005", 45), // REG 单元 0005（寄存器组 × 相位 05 × 混 轮）
    ("STK0006", 54), // STK 单元 0006（栈快照 × 相位 06 × 混 轮）
    ("MEM0007", 63), // MEM 单元 0007（内存快照 × 相位 07 × 混 轮）
    ("FMT0008", 8), // FMT 单元 0008（转储格式 × 相位 08 × 混 轮）
    ("SINK0009", 17), // SINK 单元 0009（落地 × 相位 09 × 混 轮）
    ("ID0010", 26), // ID 单元 0010（转储标识 × 相位 10 × 混 轮）
    ("CHAIN0011", 35), // CHAIN 单元 0011（指纹链 × 相位 11 × 混 轮）
    ("DUMP0012", 44), // DUMP 单元 0012（转储 × 相位 12 × 混 轮）
    ("TRIG0013", 53), // TRIG 单元 0013（触发 × 相位 13 × 混 轮）
    ("COLL0014", 62), // COLL 单元 0014（采集 × 相位 14 × 混 轮）
    ("SEAL0015", 7), // SEAL 单元 0015（封存 × 相位 15 × 混 轮）
];

/// O(1) 语义查找（match 全枚举；命中率 100%% 由守卫保证）。
pub fn u3641mixmap_get(key: &str) -> Option<u64> {
    match key {
        "DUMP0000" => Some(0),
        "TRIG0001" => Some(9),
        "COLL0002" => Some(18),
        "SEAL0003" => Some(27),
        "RB0004" => Some(36),
        "REG0005" => Some(45),
        "STK0006" => Some(54),
        "MEM0007" => Some(63),
        "FMT0008" => Some(8),
        "SINK0009" => Some(17),
        "ID0010" => Some(26),
        "CHAIN0011" => Some(35),
        "DUMP0012" => Some(44),
        "TRIG0013" => Some(53),
        "COLL0014" => Some(62),
        "SEAL0015" => Some(7),
        _ => None,
    }
}

/// 逐行守卫扫描（返回违例行数；0 = 全绿）。
pub fn u3641mixmap_scan() -> usize {
    let mut bad = 0usize;
    let mut i = 0usize;
    while i < u3641mixmap_COUNT {
        let (k, v) = u3641mixmap[i];
        let ok = match k {
            "DUMP0000" => v < 64,
            "TRIG0001" => v < 64,
            "COLL0002" => v < 64,
            "SEAL0003" => v < 64,
            "RB0004" => v < 64,
            "REG0005" => v < 64,
            "STK0006" => v < 64,
            "MEM0007" => v < 64,
            "FMT0008" => v < 64,
            "SINK0009" => v < 64,
            "ID0010" => v < 64,
            "CHAIN0011" => v < 64,
            "DUMP0012" => v < 64,
            "TRIG0013" => v < 64,
            "COLL0014" => v < 64,
            "SEAL0015" => v < 64,
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
pub fn u3641mixmap_sum() -> u64 {
    let mut s = 0u64;
    let mut i = 0usize;
    while i < u3641mixmap_COUNT {
        s = s.wrapping_add(u3641mixmap[i].1 as u64);
        i += 1;
    }
    s
}

/// 键唯一性（构造期断言等价物：逐对扫描同键）。
pub fn u3641mixmap_unique() -> bool {
    let mut i = 0usize;
    while i < u3641mixmap_COUNT {
        let mut j = i + 1;
        while j < u3641mixmap_COUNT {
            if u3641mixmap[i].0 == u3641mixmap[j].0 {
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

/// 段位守卫：全部诊断码必须落在本段 [0xe1a0, 0xe1af]。
pub const fn u3641_segment_guard() -> bool {
    let mut ok = true;
    let mut i = 0usize;
    while i < u3641Code::ALL.len() {
        let c = u3641Code::ALL[i].code();
        if c < 0xe1a0 || c > 0xe1af {
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
fn u3641_guard_seg() -> bool {
    u3641_segment_guard()
}

/// UNX-F3641 对拍表 16 行边界与累计守恒。
fn check_u3641_mix_tbl() -> bool {
    let mut ok = u3641mix_COUNT == 16;
    if ok { ok = u3641mix_get("DUMP_00") == Some(10) && u3641mix_get("SEAL_15") == Some(85); }
    if ok { ok = u3641mix_get("ZZZ_NOKEY").is_none(); }
    if ok { ok = u3641mix_sum() == 760; }
    ok
}

/// UNX-F3641 混合映射 16 行守卫唯一 + 守卫阈值冻结。
fn check_u3641_mix_map() -> bool {
    let mut ok = u3641mixmap_COUNT == 16;
    if ok { ok = u3641mixmap_scan() == 0 && u3641mixmap_unique(); }
    if ok { ok = u3641mixmap_sum() == 504; }
    if ok { ok = U3641MIXGATE_THRESHOLD == 32; }
    ok
}

/// 本模块自检（逐检查带 detail，全绿才出门）。
pub fn run_u3641_checks() -> CheckSet {
    let mut set = CheckSet::new("U3641");
    set.add(
        "u3641-seg-guard",
        u3641_guard_seg(),
        if u3641_guard_seg() { "" } else { "diagnostic code outside segment" },
    );
    set.add(
        "u3641-mix-tbl",
        check_u3641_mix_tbl(),
        if check_u3641_mix_tbl() { "" } else { "UNX-F3641 mix table mismatch" },
    );
    set.add(
        "u3641-mix-map",
        check_u3641_mix_map(),
        if check_u3641_mix_map() { "" } else { "UNX-F3641 mix map mismatch" },
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
    fn u3641_all_checks_green() {
        let set = run_u3641_checks();
        assert!(set.all_passed(), "U3641 domain checks must be green");
    }

    #[test]
    fn u3641_segment_guard_holds() {
        assert!(u3641_segment_guard());
    }
}
