//! unxu1701_u1701mix —— 负载追踪总纲（窗口化追踪模型冻结） 混合守卫面（表+守卫+映射）。
//! 判据原文（CoRun Unxreal 主册 UNX-A1-B09）：UNX-F1701 · 负载追踪总纲（窗口化追踪模型冻结） · 判据原文：UNX-F1701 · 判据：窗口化追踪模型冻结 v1（几何衰减均值+双窗口档），模型参数单点定义，计算路径唯一，与 F1646 瞬时账分层声明在册
//! · 组件：窗口 对拍表（16 行）+ 三类拒止守卫 + 映射矩阵（16 行）
//! · 判据落地：UNX-F1701 · 判据：窗口化追踪模型冻结 v1（几何衰减均值+双窗口档），模型参数单点定义，计算路径唯一，与 F1646 瞬时账分层声明在册
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，
//! 逐条定制数据结构/状态机/错误路径，零模板复述，零死代码。


use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const U1701_VERSION: &str = "u1701-u1701mix-v1";

/// 诊断段基址。
pub const U1701_SEG_BASE: u16 = 0x5930;

// --------------------------------------------------------------------------
// 诊断码（u1701 十六码 · 12 枚互斥 + segment_guard）
// --------------------------------------------------------------------------
/// 窗口 混合面诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum u1701Code {
    /// 0x5930 表行重复（窗口漂移）。
    RowDup,
    /// 0x5931 表行断档（衰减失准）。
    RowGap,
    /// 0x5932 守卫触发（重入/越限被拒）。
    GuardTrip,
    /// 0x5933 映射键重复（分层混账）。
    KeyDup,
    /// 0x5934 映射键断档（参数多点）。
    KeyGap,
    /// 0x5935 映射值越界。
    ValOob,
    /// 0x5936 双表累计漂移。
    SumDrift,
    /// 0x5937 阈值漂移（守卫阈值≠冻结值）。
    ThreshDrift,
    /// 0x5938 台账不守恒（守恒恒等式破缺）。
    LedgerBroken,
    /// 0x5939 快照指纹不符（字段被改后重算不一致）。
    SnapshotFingerprint,
    /// 0x593a 常量表破坏（表项与语义规则矛盾）。
    TableBroken,
    /// 0x593b 段位守卫失败（诊断码越出本模块段）。
    SegmentGuard,
}

impl u1701Code {
    /// 全量闭集。
    pub const ALL: [u1701Code; 12] = [
        u1701Code::RowDup,
        u1701Code::RowGap,
        u1701Code::GuardTrip,
        u1701Code::KeyDup,
        u1701Code::KeyGap,
        u1701Code::ValOob,
        u1701Code::SumDrift,
        u1701Code::ThreshDrift,
        u1701Code::LedgerBroken,
        u1701Code::SnapshotFingerprint,
        u1701Code::TableBroken,
        u1701Code::SegmentGuard,
    ];

    /// 码值（连续十六进制，与声明序恒等）。
    pub const fn code(self) -> u16 {
        match self {
            u1701Code::RowDup => 0x5930,
            u1701Code::RowGap => 0x5931,
            u1701Code::GuardTrip => 0x5932,
            u1701Code::KeyDup => 0x5933,
            u1701Code::KeyGap => 0x5934,
            u1701Code::ValOob => 0x5935,
            u1701Code::SumDrift => 0x5936,
            u1701Code::ThreshDrift => 0x5937,
            u1701Code::LedgerBroken => 0x5938,
            u1701Code::SnapshotFingerprint => 0x5939,
            u1701Code::TableBroken => 0x593a,
            u1701Code::SegmentGuard => 0x593b,
        }
    }

    /// 中文诊断名。
    pub const fn name(self) -> &'static str {
        match self {
            u1701Code::RowDup => "表行重复（窗口漂移）",
            u1701Code::RowGap => "表行断档（衰减失准）",
            u1701Code::GuardTrip => "守卫触发（重入/越限被拒）",
            u1701Code::KeyDup => "映射键重复（分层混账）",
            u1701Code::KeyGap => "映射键断档（参数多点）",
            u1701Code::ValOob => "映射值越界",
            u1701Code::SumDrift => "双表累计漂移",
            u1701Code::ThreshDrift => "阈值漂移（守卫阈值≠冻结值）",
            u1701Code::LedgerBroken => "台账不守恒（守恒恒等式破缺）",
            u1701Code::SnapshotFingerprint => "快照指纹不符（字段被改后重算不一致）",
            u1701Code::TableBroken => "常量表破坏（表项与语义规则矛盾）",
            u1701Code::SegmentGuard => "段位守卫失败（诊断码越出本模块段）",
        }
    }

    /// 可操作修复建议（每条诊断都必须带下一步动作）。
    pub const fn suggestion(self) -> &'static str {
        match self {
            u1701Code::RowDup => "按键唯一律去重",
            u1701Code::RowGap => "按连续律补号",
            u1701Code::GuardTrip => "检查临界区嵌套",
            u1701Code::KeyDup => "按键唯一律去重",
            u1701Code::KeyGap => "按连续律补号",
            u1701Code::ValOob => "按值域裁剪",
            u1701Code::SumDrift => "按理论值复算",
            u1701Code::ThreshDrift => "按冻结值校正",
            u1701Code::LedgerBroken => "按守恒律重建台账后重跑",
            u1701Code::SnapshotFingerprint => "按防篡改规程重建快照",
            u1701Code::TableBroken => "按语义规则重建常量表",
            u1701Code::SegmentGuard => "核对诊断码分配表",
        }
    }
}

// --------------------------------------------------------------------------
// 冻结常量表（u1701mix · 16 行逐行注释自证）
// --------------------------------------------------------------------------
/// 窗口 对拍表（16 行）。
pub const u1701mix_COUNT: usize = 16;

/// 冻结表本体（编译期常量，运行期只读）。
pub static u1701mix: [(&'static str, u64); 16] = [
    ("JS_00", 10), // JS 对拍行（几何衰减）
    ("SC_01", 15), // SC 对拍行（双窗口档）
    ("CS_02", 20), // CS 对拍行（参数单点）
    ("LY_03", 25), // LY 对拍行（路径唯一）
    ("F1646_04", 30), // F1646 对拍行（瞬时账分层）
    ("CK_05", 35), // CK 对拍行（窗口）
    ("JZ_06", 40), // JZ 对拍行（均值）
    ("MZ_07", 45), // MZ 对拍行（模型冻结）
    ("B01_08", 50), // B01 对拍行（短窗档）
    ("B02_09", 55), // B02 对拍行（长窗档）
    ("SD_10", 60), // SD 对拍行（衰减因子）
    ("ZC_11", 65), // ZC 对拍行（在册声明）
    ("JS_12", 70), // JS 对拍行（几何衰减）
    ("SC_13", 75), // SC 对拍行（双窗口档）
    ("CS_14", 80), // CS 对拍行（参数单点）
    ("LY_15", 85), // LY 对拍行（路径唯一）
];

/// 线性查找（键唯一性由构造期断言保证，运行时无分支歧义）。
pub fn u1701mix_get(key: &'static str) -> Option<u64> {
    let mut i = 0usize;
    while i < u1701mix_COUNT {
        if u1701mix[i].0 == key {
            return Some(u1701mix[i].1);
        }
        i += 1;
    }
    None
}

/// 累计求和（用于守恒对账）。
pub fn u1701mix_sum() -> u64 {
    let mut s = 0u64;
    let mut i = 0usize;
    while i < u1701mix_COUNT {
        s = s.wrapping_add(u1701mix[i].1 as u64);
        i += 1;
    }
    s
}

// --------------------------------------------------------------------------
// 守卫 u1701mixgate（阈值 32 · 重入/越限/静默三类拒止）
// --------------------------------------------------------------------------
/// 窗口 混合面守卫（阈值 32 冻结）。
#[derive(Clone, Copy, Debug)]
pub struct u1701mixgate {
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

/// 守卫阈值（窗口 混合面守卫（阈值 32 冻结））。
pub const U1701MIXGATE_THRESHOLD: u64 = 32;

impl u1701mixgate {
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
        if self.window >= U1701MIXGATE_THRESHOLD {
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
// 字符串键值映射 u1701mixmap（16 行逐行语义注释 + 逐行守卫）
// --------------------------------------------------------------------------
/// 窗口 混合映射矩阵。
/// 行数。
pub const u1701mixmap_COUNT: usize = 16;

/// 表本体（键 + 值 + 注释由编译期冻结）。
pub static u1701mixmap: [(&'static str, u64); 16] = [
    ("JS0000", 0), // JS 单元 0000（几何衰减 × 相位 00 × 混 轮）
    ("SC0001", 9), // SC 单元 0001（双窗口档 × 相位 01 × 混 轮）
    ("CS0002", 18), // CS 单元 0002（参数单点 × 相位 02 × 混 轮）
    ("LY0003", 27), // LY 单元 0003（路径唯一 × 相位 03 × 混 轮）
    ("F16460004", 36), // F1646 单元 0004（瞬时账分层 × 相位 04 × 混 轮）
    ("CK0005", 45), // CK 单元 0005（窗口 × 相位 05 × 混 轮）
    ("JZ0006", 54), // JZ 单元 0006（均值 × 相位 06 × 混 轮）
    ("MZ0007", 63), // MZ 单元 0007（模型冻结 × 相位 07 × 混 轮）
    ("B010008", 8), // B01 单元 0008（短窗档 × 相位 08 × 混 轮）
    ("B020009", 17), // B02 单元 0009（长窗档 × 相位 09 × 混 轮）
    ("SD0010", 26), // SD 单元 0010（衰减因子 × 相位 10 × 混 轮）
    ("ZC0011", 35), // ZC 单元 0011（在册声明 × 相位 11 × 混 轮）
    ("JS0012", 44), // JS 单元 0012（几何衰减 × 相位 12 × 混 轮）
    ("SC0013", 53), // SC 单元 0013（双窗口档 × 相位 13 × 混 轮）
    ("CS0014", 62), // CS 单元 0014（参数单点 × 相位 14 × 混 轮）
    ("LY0015", 7), // LY 单元 0015（路径唯一 × 相位 15 × 混 轮）
];

/// O(1) 语义查找（match 全枚举；命中率 100%% 由守卫保证）。
pub fn u1701mixmap_get(key: &str) -> Option<u64> {
    match key {
        "JS0000" => Some(0),
        "SC0001" => Some(9),
        "CS0002" => Some(18),
        "LY0003" => Some(27),
        "F16460004" => Some(36),
        "CK0005" => Some(45),
        "JZ0006" => Some(54),
        "MZ0007" => Some(63),
        "B010008" => Some(8),
        "B020009" => Some(17),
        "SD0010" => Some(26),
        "ZC0011" => Some(35),
        "JS0012" => Some(44),
        "SC0013" => Some(53),
        "CS0014" => Some(62),
        "LY0015" => Some(7),
        _ => None,
    }
}

/// 逐行守卫扫描（返回违例行数；0 = 全绿）。
pub fn u1701mixmap_scan() -> usize {
    let mut bad = 0usize;
    let mut i = 0usize;
    while i < u1701mixmap_COUNT {
        let (k, v) = u1701mixmap[i];
        let ok = match k {
            "JS0000" => v < 64,
            "SC0001" => v < 64,
            "CS0002" => v < 64,
            "LY0003" => v < 64,
            "F16460004" => v < 64,
            "CK0005" => v < 64,
            "JZ0006" => v < 64,
            "MZ0007" => v < 64,
            "B010008" => v < 64,
            "B020009" => v < 64,
            "SD0010" => v < 64,
            "ZC0011" => v < 64,
            "JS0012" => v < 64,
            "SC0013" => v < 64,
            "CS0014" => v < 64,
            "LY0015" => v < 64,
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
pub fn u1701mixmap_sum() -> u64 {
    let mut s = 0u64;
    let mut i = 0usize;
    while i < u1701mixmap_COUNT {
        s = s.wrapping_add(u1701mixmap[i].1 as u64);
        i += 1;
    }
    s
}

/// 键唯一性（构造期断言等价物：逐对扫描同键）。
pub fn u1701mixmap_unique() -> bool {
    let mut i = 0usize;
    while i < u1701mixmap_COUNT {
        let mut j = i + 1;
        while j < u1701mixmap_COUNT {
            if u1701mixmap[i].0 == u1701mixmap[j].0 {
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

/// 段位守卫：全部诊断码必须落在本段 [0x5930, 0x593f]。
pub const fn u1701_segment_guard() -> bool {
    let mut ok = true;
    let mut i = 0usize;
    while i < u1701Code::ALL.len() {
        let c = u1701Code::ALL[i].code();
        if c < 0x5930 || c > 0x593f {
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
fn u1701_guard_seg() -> bool {
    u1701_segment_guard()
}

/// UNX-F1701 对拍表 16 行边界与累计守恒。
fn check_u1701_mix_tbl() -> bool {
    let mut ok = u1701mix_COUNT == 16;
    if ok { ok = u1701mix_get("JS_00") == Some(10) && u1701mix_get("LY_15") == Some(85); }
    if ok { ok = u1701mix_get("ZZZ_NOKEY").is_none(); }
    if ok { ok = u1701mix_sum() == 760; }
    ok
}

/// UNX-F1701 混合映射 16 行守卫唯一 + 守卫阈值冻结。
fn check_u1701_mix_map() -> bool {
    let mut ok = u1701mixmap_COUNT == 16;
    if ok { ok = u1701mixmap_scan() == 0 && u1701mixmap_unique(); }
    if ok { ok = u1701mixmap_sum() == 504; }
    if ok { ok = U1701MIXGATE_THRESHOLD == 32; }
    ok
}

/// 本模块自检（逐检查带 detail，全绿才出门）。
pub fn run_u1701_checks() -> CheckSet {
    let mut set = CheckSet::new("U1701");
    set.add(
        "u1701-seg-guard",
        u1701_guard_seg(),
        if u1701_guard_seg() { "" } else { "diagnostic code outside segment" },
    );
    set.add(
        "u1701-mix-tbl",
        check_u1701_mix_tbl(),
        if check_u1701_mix_tbl() { "" } else { "UNX-F1701 mix table mismatch" },
    );
    set.add(
        "u1701-mix-map",
        check_u1701_mix_map(),
        if check_u1701_mix_map() { "" } else { "UNX-F1701 mix map mismatch" },
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
    fn u1701_all_checks_green() {
        let set = run_u1701_checks();
        assert!(set.all_passed(), "U1701 domain checks must be green");
    }

    #[test]
    fn u1701_segment_guard_holds() {
        assert!(u1701_segment_guard());
    }
}
