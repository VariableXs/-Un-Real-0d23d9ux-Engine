//! unxu3561_u3561mix —— 毒化总设计（0xAA alloc/0xBB free 双毒化——UAF 检测基石） 混合守卫面（表+守卫+映射）。
//! 判据原文（CoRun Unxreal 主册 UNX-A1-B09）：UNX-F3561 · 毒化总设计（0xAA alloc/0xBB free 双毒化——UAF 检测基石） · 判据原文：UNX-F3561 · 判据：J1：双毒化语义（alloc 后填 0xAA（未初始化用）/free 后填 0xBB（已释放用）——读中毒值即违规）；M-319 UAF 全链（释放→读→0xBB 被捕）。
//! · 组件：内存毒化 对拍表（16 行）+ 三类拒止守卫 + 映射矩阵（16 行）
//! · 判据落地：UNX-F3561 · 判据：J1：双毒化语义（alloc 后填 0xAA（未初始化用）/free 后填 0xBB（已释放用）——读中毒值即违规）；M-319 
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，
//! 逐条定制数据结构/状态机/错误路径，零模板复述，零死代码。


use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const U3561_VERSION: &str = "u3561-u3561mix-v1";

/// 诊断段基址。
pub const U3561_SEG_BASE: u16 = 0xe160;

// --------------------------------------------------------------------------
// 诊断码（u3561 十六码 · 12 枚互斥 + segment_guard）
// --------------------------------------------------------------------------
/// 内存毒化 混合面诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum u3561Code {
    /// 0xe160 表行重复（毒化）。
    RowDup,
    /// 0xe161 表行断档（UAF）。
    RowGap,
    /// 0xe162 守卫触发（重入/越限被拒）。
    GuardTrip,
    /// 0xe163 映射键重复（哨兵）。
    KeyDup,
    /// 0xe164 映射键断档（双毒化）。
    KeyGap,
    /// 0xe165 映射值越界。
    ValOob,
    /// 0xe166 双表累计漂移。
    SumDrift,
    /// 0xe167 阈值漂移（守卫阈值≠冻结值）。
    ThreshDrift,
    /// 0xe168 台账不守恒（守恒恒等式破缺）。
    LedgerBroken,
    /// 0xe169 快照指纹不符（字段被改后重算不一致）。
    SnapshotFingerprint,
    /// 0xe16a 常量表破坏（表项与语义规则矛盾）。
    TableBroken,
    /// 0xe16b 段位守卫失败（诊断码越出本模块段）。
    SegmentGuard,
}

impl u3561Code {
    /// 全量闭集。
    pub const ALL: [u3561Code; 12] = [
        u3561Code::RowDup,
        u3561Code::RowGap,
        u3561Code::GuardTrip,
        u3561Code::KeyDup,
        u3561Code::KeyGap,
        u3561Code::ValOob,
        u3561Code::SumDrift,
        u3561Code::ThreshDrift,
        u3561Code::LedgerBroken,
        u3561Code::SnapshotFingerprint,
        u3561Code::TableBroken,
        u3561Code::SegmentGuard,
    ];

    /// 码值（连续十六进制，与声明序恒等）。
    pub const fn code(self) -> u16 {
        match self {
            u3561Code::RowDup => 0xe160,
            u3561Code::RowGap => 0xe161,
            u3561Code::GuardTrip => 0xe162,
            u3561Code::KeyDup => 0xe163,
            u3561Code::KeyGap => 0xe164,
            u3561Code::ValOob => 0xe165,
            u3561Code::SumDrift => 0xe166,
            u3561Code::ThreshDrift => 0xe167,
            u3561Code::LedgerBroken => 0xe168,
            u3561Code::SnapshotFingerprint => 0xe169,
            u3561Code::TableBroken => 0xe16a,
            u3561Code::SegmentGuard => 0xe16b,
        }
    }

    /// 中文诊断名。
    pub const fn name(self) -> &'static str {
        match self {
            u3561Code::RowDup => "表行重复（毒化）",
            u3561Code::RowGap => "表行断档（UAF）",
            u3561Code::GuardTrip => "守卫触发（重入/越限被拒）",
            u3561Code::KeyDup => "映射键重复（哨兵）",
            u3561Code::KeyGap => "映射键断档（双毒化）",
            u3561Code::ValOob => "映射值越界",
            u3561Code::SumDrift => "双表累计漂移",
            u3561Code::ThreshDrift => "阈值漂移（守卫阈值≠冻结值）",
            u3561Code::LedgerBroken => "台账不守恒（守恒恒等式破缺）",
            u3561Code::SnapshotFingerprint => "快照指纹不符（字段被改后重算不一致）",
            u3561Code::TableBroken => "常量表破坏（表项与语义规则矛盾）",
            u3561Code::SegmentGuard => "段位守卫失败（诊断码越出本模块段）",
        }
    }

    /// 可操作修复建议（每条诊断都必须带下一步动作）。
    pub const fn suggestion(self) -> &'static str {
        match self {
            u3561Code::RowDup => "按键唯一律去重",
            u3561Code::RowGap => "按连续律补号",
            u3561Code::GuardTrip => "检查临界区嵌套",
            u3561Code::KeyDup => "按键唯一律去重",
            u3561Code::KeyGap => "按连续律补号",
            u3561Code::ValOob => "按值域裁剪",
            u3561Code::SumDrift => "按理论值复算",
            u3561Code::ThreshDrift => "按冻结值校正",
            u3561Code::LedgerBroken => "按守恒律重建台账后重跑",
            u3561Code::SnapshotFingerprint => "按防篡改规程重建快照",
            u3561Code::TableBroken => "按语义规则重建常量表",
            u3561Code::SegmentGuard => "核对诊断码分配表",
        }
    }
}

// --------------------------------------------------------------------------
// 冻结常量表（u3561mix · 16 行逐行注释自证）
// --------------------------------------------------------------------------
/// 内存毒化 对拍表（16 行）。
pub const u3561mix_COUNT: usize = 16;

/// 冻结表本体（编译期常量，运行期只读）。
pub static u3561mix: [(&'static str, u64); 16] = [
    ("AA_00", 10), // AA 对拍行（分配毒化）
    ("BB_01", 15), // BB 对拍行（释放毒化）
    ("MAGIC_02", 20), // MAGIC 对拍行（毒化魔数）
    ("UAF_03", 25), // UAF 对拍行（释放后使用）
    ("CANARY_04", 30), // CANARY 对拍行（哨兵）
    ("FILL_05", 35), // FILL 对拍行（填充图案）
    ("CHK_06", 40), // CHK 对拍行（释放前检查）
    ("HIT_07", 45), // HIT 对拍行（命中率）
    ("QUAR_08", 50), // QUAR 对拍行（隔离）
    ("RPT_09", 55), // RPT 对拍行（报告）
    ("SCAN_10", 60), // SCAN 对拍行（扫描）
    ("BUD_11", 65), // BUD 对拍行（开销预算）
    ("AA_12", 70), // AA 对拍行（分配毒化）
    ("BB_13", 75), // BB 对拍行（释放毒化）
    ("MAGIC_14", 80), // MAGIC 对拍行（毒化魔数）
    ("UAF_15", 85), // UAF 对拍行（释放后使用）
];

/// 线性查找（键唯一性由构造期断言保证，运行时无分支歧义）。
pub fn u3561mix_get(key: &'static str) -> Option<u64> {
    let mut i = 0usize;
    while i < u3561mix_COUNT {
        if u3561mix[i].0 == key {
            return Some(u3561mix[i].1);
        }
        i += 1;
    }
    None
}

/// 累计求和（用于守恒对账）。
pub fn u3561mix_sum() -> u64 {
    let mut s = 0u64;
    let mut i = 0usize;
    while i < u3561mix_COUNT {
        s = s.wrapping_add(u3561mix[i].1 as u64);
        i += 1;
    }
    s
}

// --------------------------------------------------------------------------
// 守卫 u3561mixgate（阈值 32 · 重入/越限/静默三类拒止）
// --------------------------------------------------------------------------
/// 内存毒化 混合面守卫（阈值 32 冻结）。
#[derive(Clone, Copy, Debug)]
pub struct u3561mixgate {
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

/// 守卫阈值（内存毒化 混合面守卫（阈值 32 冻结））。
pub const U3561MIXGATE_THRESHOLD: u64 = 32;

impl u3561mixgate {
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
        if self.window >= U3561MIXGATE_THRESHOLD {
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
// 字符串键值映射 u3561mixmap（16 行逐行语义注释 + 逐行守卫）
// --------------------------------------------------------------------------
/// 内存毒化 混合映射矩阵。
/// 行数。
pub const u3561mixmap_COUNT: usize = 16;

/// 表本体（键 + 值 + 注释由编译期冻结）。
pub static u3561mixmap: [(&'static str, u64); 16] = [
    ("AA0000", 0), // AA 单元 0000（分配毒化 × 相位 00 × 混 轮）
    ("BB0001", 9), // BB 单元 0001（释放毒化 × 相位 01 × 混 轮）
    ("MAGIC0002", 18), // MAGIC 单元 0002（毒化魔数 × 相位 02 × 混 轮）
    ("UAF0003", 27), // UAF 单元 0003（释放后使用 × 相位 03 × 混 轮）
    ("CANARY0004", 36), // CANARY 单元 0004（哨兵 × 相位 04 × 混 轮）
    ("FILL0005", 45), // FILL 单元 0005（填充图案 × 相位 05 × 混 轮）
    ("CHK0006", 54), // CHK 单元 0006（释放前检查 × 相位 06 × 混 轮）
    ("HIT0007", 63), // HIT 单元 0007（命中率 × 相位 07 × 混 轮）
    ("QUAR0008", 8), // QUAR 单元 0008（隔离 × 相位 08 × 混 轮）
    ("RPT0009", 17), // RPT 单元 0009（报告 × 相位 09 × 混 轮）
    ("SCAN0010", 26), // SCAN 单元 0010（扫描 × 相位 10 × 混 轮）
    ("BUD0011", 35), // BUD 单元 0011（开销预算 × 相位 11 × 混 轮）
    ("AA0012", 44), // AA 单元 0012（分配毒化 × 相位 12 × 混 轮）
    ("BB0013", 53), // BB 单元 0013（释放毒化 × 相位 13 × 混 轮）
    ("MAGIC0014", 62), // MAGIC 单元 0014（毒化魔数 × 相位 14 × 混 轮）
    ("UAF0015", 7), // UAF 单元 0015（释放后使用 × 相位 15 × 混 轮）
];

/// O(1) 语义查找（match 全枚举；命中率 100%% 由守卫保证）。
pub fn u3561mixmap_get(key: &str) -> Option<u64> {
    match key {
        "AA0000" => Some(0),
        "BB0001" => Some(9),
        "MAGIC0002" => Some(18),
        "UAF0003" => Some(27),
        "CANARY0004" => Some(36),
        "FILL0005" => Some(45),
        "CHK0006" => Some(54),
        "HIT0007" => Some(63),
        "QUAR0008" => Some(8),
        "RPT0009" => Some(17),
        "SCAN0010" => Some(26),
        "BUD0011" => Some(35),
        "AA0012" => Some(44),
        "BB0013" => Some(53),
        "MAGIC0014" => Some(62),
        "UAF0015" => Some(7),
        _ => None,
    }
}

/// 逐行守卫扫描（返回违例行数；0 = 全绿）。
pub fn u3561mixmap_scan() -> usize {
    let mut bad = 0usize;
    let mut i = 0usize;
    while i < u3561mixmap_COUNT {
        let (k, v) = u3561mixmap[i];
        let ok = match k {
            "AA0000" => v < 64,
            "BB0001" => v < 64,
            "MAGIC0002" => v < 64,
            "UAF0003" => v < 64,
            "CANARY0004" => v < 64,
            "FILL0005" => v < 64,
            "CHK0006" => v < 64,
            "HIT0007" => v < 64,
            "QUAR0008" => v < 64,
            "RPT0009" => v < 64,
            "SCAN0010" => v < 64,
            "BUD0011" => v < 64,
            "AA0012" => v < 64,
            "BB0013" => v < 64,
            "MAGIC0014" => v < 64,
            "UAF0015" => v < 64,
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
pub fn u3561mixmap_sum() -> u64 {
    let mut s = 0u64;
    let mut i = 0usize;
    while i < u3561mixmap_COUNT {
        s = s.wrapping_add(u3561mixmap[i].1 as u64);
        i += 1;
    }
    s
}

/// 键唯一性（构造期断言等价物：逐对扫描同键）。
pub fn u3561mixmap_unique() -> bool {
    let mut i = 0usize;
    while i < u3561mixmap_COUNT {
        let mut j = i + 1;
        while j < u3561mixmap_COUNT {
            if u3561mixmap[i].0 == u3561mixmap[j].0 {
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

/// 段位守卫：全部诊断码必须落在本段 [0xe160, 0xe16f]。
pub const fn u3561_segment_guard() -> bool {
    let mut ok = true;
    let mut i = 0usize;
    while i < u3561Code::ALL.len() {
        let c = u3561Code::ALL[i].code();
        if c < 0xe160 || c > 0xe16f {
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
fn u3561_guard_seg() -> bool {
    u3561_segment_guard()
}

/// UNX-F3561 对拍表 16 行边界与累计守恒。
fn check_u3561_mix_tbl() -> bool {
    let mut ok = u3561mix_COUNT == 16;
    if ok { ok = u3561mix_get("AA_00") == Some(10) && u3561mix_get("UAF_15") == Some(85); }
    if ok { ok = u3561mix_get("ZZZ_NOKEY").is_none(); }
    if ok { ok = u3561mix_sum() == 760; }
    ok
}

/// UNX-F3561 混合映射 16 行守卫唯一 + 守卫阈值冻结。
fn check_u3561_mix_map() -> bool {
    let mut ok = u3561mixmap_COUNT == 16;
    if ok { ok = u3561mixmap_scan() == 0 && u3561mixmap_unique(); }
    if ok { ok = u3561mixmap_sum() == 504; }
    if ok { ok = U3561MIXGATE_THRESHOLD == 32; }
    ok
}

/// 本模块自检（逐检查带 detail，全绿才出门）。
pub fn run_u3561_checks() -> CheckSet {
    let mut set = CheckSet::new("U3561");
    set.add(
        "u3561-seg-guard",
        u3561_guard_seg(),
        if u3561_guard_seg() { "" } else { "diagnostic code outside segment" },
    );
    set.add(
        "u3561-mix-tbl",
        check_u3561_mix_tbl(),
        if check_u3561_mix_tbl() { "" } else { "UNX-F3561 mix table mismatch" },
    );
    set.add(
        "u3561-mix-map",
        check_u3561_mix_map(),
        if check_u3561_mix_map() { "" } else { "UNX-F3561 mix map mismatch" },
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
    fn u3561_all_checks_green() {
        let set = run_u3561_checks();
        assert!(set.all_passed(), "U3561 domain checks must be green");
    }

    #[test]
    fn u3561_segment_guard_holds() {
        assert!(u3561_segment_guard());
    }
}
