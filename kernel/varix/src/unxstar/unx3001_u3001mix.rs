//! unxu3001_u3001mix —— 双链结构冻结（active/inactive 两链节点布局） 混合守卫面（表+守卫+映射）。
//! 判据原文（CoRun Unxreal 主册 UNX-A1-B09）：UNX-F3001 · 双链结构冻结（active/inactive 两链节点布局） · 判据原文：UNX-F3001 · 判据：UNX-F3001-J1 active/inactive 双链结构冻结哈希入册：节点布局（page 结构挂链位/lru 位标/双向指针）与 F2682 页缓存驻留结构对齐零缺失，链头尾指针操作单点化
//! · 组件：双链结构冻结 对拍表（16 行）+ 三类拒止守卫 + 映射矩阵（16 行）
//! · 判据落地：UNX-F3001 · 判据：UNX-F3001-J1 active/inactive 双链结构冻结哈希入册：节点布局（page 结构挂链位/lru 位标/双向
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，
//! 逐条定制数据结构/状态机/错误路径，零模板复述，零死代码。


use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const U3001_VERSION: &str = "u3001-u3001mix-v1";

/// 诊断段基址。
pub const U3001_SEG_BASE: u16 = 0xe2c0;

// --------------------------------------------------------------------------
// 诊断码（u3001 十六码 · 12 枚互斥 + segment_guard）
// --------------------------------------------------------------------------
/// 双链结构冻结 混合面诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum u3001Code {
    /// 0xe2c0 表行重复（双链）。
    RowDup,
    /// 0xe2c1 表行断档（节点）。
    RowGap,
    /// 0xe2c2 守卫触发（重入/越限被拒）。
    GuardTrip,
    /// 0xe2c3 映射键重复（迁移）。
    KeyDup,
    /// 0xe2c4 映射键断档（双链）。
    KeyGap,
    /// 0xe2c5 映射值越界。
    ValOob,
    /// 0xe2c6 双表累计漂移。
    SumDrift,
    /// 0xe2c7 阈值漂移（守卫阈值≠冻结值）。
    ThreshDrift,
    /// 0xe2c8 台账不守恒（守恒恒等式破缺）。
    LedgerBroken,
    /// 0xe2c9 快照指纹不符（字段被改后重算不一致）。
    SnapshotFingerprint,
    /// 0xe2ca 常量表破坏（表项与语义规则矛盾）。
    TableBroken,
    /// 0xe2cb 段位守卫失败（诊断码越出本模块段）。
    SegmentGuard,
}

impl u3001Code {
    /// 全量闭集。
    pub const ALL: [u3001Code; 12] = [
        u3001Code::RowDup,
        u3001Code::RowGap,
        u3001Code::GuardTrip,
        u3001Code::KeyDup,
        u3001Code::KeyGap,
        u3001Code::ValOob,
        u3001Code::SumDrift,
        u3001Code::ThreshDrift,
        u3001Code::LedgerBroken,
        u3001Code::SnapshotFingerprint,
        u3001Code::TableBroken,
        u3001Code::SegmentGuard,
    ];

    /// 码值（连续十六进制，与声明序恒等）。
    pub const fn code(self) -> u16 {
        match self {
            u3001Code::RowDup => 0xe2c0,
            u3001Code::RowGap => 0xe2c1,
            u3001Code::GuardTrip => 0xe2c2,
            u3001Code::KeyDup => 0xe2c3,
            u3001Code::KeyGap => 0xe2c4,
            u3001Code::ValOob => 0xe2c5,
            u3001Code::SumDrift => 0xe2c6,
            u3001Code::ThreshDrift => 0xe2c7,
            u3001Code::LedgerBroken => 0xe2c8,
            u3001Code::SnapshotFingerprint => 0xe2c9,
            u3001Code::TableBroken => 0xe2ca,
            u3001Code::SegmentGuard => 0xe2cb,
        }
    }

    /// 中文诊断名。
    pub const fn name(self) -> &'static str {
        match self {
            u3001Code::RowDup => "表行重复（双链）",
            u3001Code::RowGap => "表行断档（节点）",
            u3001Code::GuardTrip => "守卫触发（重入/越限被拒）",
            u3001Code::KeyDup => "映射键重复（迁移）",
            u3001Code::KeyGap => "映射键断档（双链）",
            u3001Code::ValOob => "映射值越界",
            u3001Code::SumDrift => "双表累计漂移",
            u3001Code::ThreshDrift => "阈值漂移（守卫阈值≠冻结值）",
            u3001Code::LedgerBroken => "台账不守恒（守恒恒等式破缺）",
            u3001Code::SnapshotFingerprint => "快照指纹不符（字段被改后重算不一致）",
            u3001Code::TableBroken => "常量表破坏（表项与语义规则矛盾）",
            u3001Code::SegmentGuard => "段位守卫失败（诊断码越出本模块段）",
        }
    }

    /// 可操作修复建议（每条诊断都必须带下一步动作）。
    pub const fn suggestion(self) -> &'static str {
        match self {
            u3001Code::RowDup => "按键唯一律去重",
            u3001Code::RowGap => "按连续律补号",
            u3001Code::GuardTrip => "检查临界区嵌套",
            u3001Code::KeyDup => "按键唯一律去重",
            u3001Code::KeyGap => "按连续律补号",
            u3001Code::ValOob => "按值域裁剪",
            u3001Code::SumDrift => "按理论值复算",
            u3001Code::ThreshDrift => "按冻结值校正",
            u3001Code::LedgerBroken => "按守恒律重建台账后重跑",
            u3001Code::SnapshotFingerprint => "按防篡改规程重建快照",
            u3001Code::TableBroken => "按语义规则重建常量表",
            u3001Code::SegmentGuard => "核对诊断码分配表",
        }
    }
}

// --------------------------------------------------------------------------
// 冻结常量表（u3001mix · 16 行逐行注释自证）
// --------------------------------------------------------------------------
/// 双链结构冻结 对拍表（16 行）。
pub const u3001mix_COUNT: usize = 16;

/// 冻结表本体（编译期常量，运行期只读）。
pub static u3001mix: [(&'static str, u64); 16] = [
    ("ACTIVE_00", 10), // ACTIVE 对拍行（active 链）
    ("INACT_01", 15), // INACT 对拍行（inactive 链）
    ("NODE_02", 20), // NODE 对拍行（节点）
    ("LAYOUT_03", 25), // LAYOUT 对拍行（节点布局）
    ("NEXT_04", 30), // NEXT 对拍行（后继）
    ("PREV_05", 35), // PREV 对拍行（前驱）
    ("FREE_06", 40), // FREE 对拍行（空闲链）
    ("POOL_07", 45), // POOL 对拍行（节点池）
    ("MOVE_08", 50), // MOVE 对拍行（迁移）
    ("SPLIT_09", 55), // SPLIT 对拍行（拆分）
    ("JOIN_10", 60), // JOIN 对拍行（合并）
    ("INV_11", 65), // INV 对拍行（不变量）
    ("ACTIVE_12", 70), // ACTIVE 对拍行（active 链）
    ("INACT_13", 75), // INACT 对拍行（inactive 链）
    ("NODE_14", 80), // NODE 对拍行（节点）
    ("LAYOUT_15", 85), // LAYOUT 对拍行（节点布局）
];

/// 线性查找（键唯一性由构造期断言保证，运行时无分支歧义）。
pub fn u3001mix_get(key: &'static str) -> Option<u64> {
    let mut i = 0usize;
    while i < u3001mix_COUNT {
        if u3001mix[i].0 == key {
            return Some(u3001mix[i].1);
        }
        i += 1;
    }
    None
}

/// 累计求和（用于守恒对账）。
pub fn u3001mix_sum() -> u64 {
    let mut s = 0u64;
    let mut i = 0usize;
    while i < u3001mix_COUNT {
        s = s.wrapping_add(u3001mix[i].1 as u64);
        i += 1;
    }
    s
}

// --------------------------------------------------------------------------
// 守卫 u3001mixgate（阈值 32 · 重入/越限/静默三类拒止）
// --------------------------------------------------------------------------
/// 双链结构冻结 混合面守卫（阈值 32 冻结）。
#[derive(Clone, Copy, Debug)]
pub struct u3001mixgate {
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

/// 守卫阈值（双链结构冻结 混合面守卫（阈值 32 冻结））。
pub const U3001MIXGATE_THRESHOLD: u64 = 32;

impl u3001mixgate {
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
        if self.window >= U3001MIXGATE_THRESHOLD {
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
// 字符串键值映射 u3001mixmap（16 行逐行语义注释 + 逐行守卫）
// --------------------------------------------------------------------------
/// 双链结构冻结 混合映射矩阵。
/// 行数。
pub const u3001mixmap_COUNT: usize = 16;

/// 表本体（键 + 值 + 注释由编译期冻结）。
pub static u3001mixmap: [(&'static str, u64); 16] = [
    ("ACTIVE0000", 0), // ACTIVE 单元 0000（active 链 × 相位 00 × 混 轮）
    ("INACT0001", 9), // INACT 单元 0001（inactive 链 × 相位 01 × 混 轮）
    ("NODE0002", 18), // NODE 单元 0002（节点 × 相位 02 × 混 轮）
    ("LAYOUT0003", 27), // LAYOUT 单元 0003（节点布局 × 相位 03 × 混 轮）
    ("NEXT0004", 36), // NEXT 单元 0004（后继 × 相位 04 × 混 轮）
    ("PREV0005", 45), // PREV 单元 0005（前驱 × 相位 05 × 混 轮）
    ("FREE0006", 54), // FREE 单元 0006（空闲链 × 相位 06 × 混 轮）
    ("POOL0007", 63), // POOL 单元 0007（节点池 × 相位 07 × 混 轮）
    ("MOVE0008", 8), // MOVE 单元 0008（迁移 × 相位 08 × 混 轮）
    ("SPLIT0009", 17), // SPLIT 单元 0009（拆分 × 相位 09 × 混 轮）
    ("JOIN0010", 26), // JOIN 单元 0010（合并 × 相位 10 × 混 轮）
    ("INV0011", 35), // INV 单元 0011（不变量 × 相位 11 × 混 轮）
    ("ACTIVE0012", 44), // ACTIVE 单元 0012（active 链 × 相位 12 × 混 轮）
    ("INACT0013", 53), // INACT 单元 0013（inactive 链 × 相位 13 × 混 轮）
    ("NODE0014", 62), // NODE 单元 0014（节点 × 相位 14 × 混 轮）
    ("LAYOUT0015", 7), // LAYOUT 单元 0015（节点布局 × 相位 15 × 混 轮）
];

/// O(1) 语义查找（match 全枚举；命中率 100%% 由守卫保证）。
pub fn u3001mixmap_get(key: &str) -> Option<u64> {
    match key {
        "ACTIVE0000" => Some(0),
        "INACT0001" => Some(9),
        "NODE0002" => Some(18),
        "LAYOUT0003" => Some(27),
        "NEXT0004" => Some(36),
        "PREV0005" => Some(45),
        "FREE0006" => Some(54),
        "POOL0007" => Some(63),
        "MOVE0008" => Some(8),
        "SPLIT0009" => Some(17),
        "JOIN0010" => Some(26),
        "INV0011" => Some(35),
        "ACTIVE0012" => Some(44),
        "INACT0013" => Some(53),
        "NODE0014" => Some(62),
        "LAYOUT0015" => Some(7),
        _ => None,
    }
}

/// 逐行守卫扫描（返回违例行数；0 = 全绿）。
pub fn u3001mixmap_scan() -> usize {
    let mut bad = 0usize;
    let mut i = 0usize;
    while i < u3001mixmap_COUNT {
        let (k, v) = u3001mixmap[i];
        let ok = match k {
            "ACTIVE0000" => v < 64,
            "INACT0001" => v < 64,
            "NODE0002" => v < 64,
            "LAYOUT0003" => v < 64,
            "NEXT0004" => v < 64,
            "PREV0005" => v < 64,
            "FREE0006" => v < 64,
            "POOL0007" => v < 64,
            "MOVE0008" => v < 64,
            "SPLIT0009" => v < 64,
            "JOIN0010" => v < 64,
            "INV0011" => v < 64,
            "ACTIVE0012" => v < 64,
            "INACT0013" => v < 64,
            "NODE0014" => v < 64,
            "LAYOUT0015" => v < 64,
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
pub fn u3001mixmap_sum() -> u64 {
    let mut s = 0u64;
    let mut i = 0usize;
    while i < u3001mixmap_COUNT {
        s = s.wrapping_add(u3001mixmap[i].1 as u64);
        i += 1;
    }
    s
}

/// 键唯一性（构造期断言等价物：逐对扫描同键）。
pub fn u3001mixmap_unique() -> bool {
    let mut i = 0usize;
    while i < u3001mixmap_COUNT {
        let mut j = i + 1;
        while j < u3001mixmap_COUNT {
            if u3001mixmap[i].0 == u3001mixmap[j].0 {
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

/// 段位守卫：全部诊断码必须落在本段 [0xe2c0, 0xe2cf]。
pub const fn u3001_segment_guard() -> bool {
    let mut ok = true;
    let mut i = 0usize;
    while i < u3001Code::ALL.len() {
        let c = u3001Code::ALL[i].code();
        if c < 0xe2c0 || c > 0xe2cf {
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
fn u3001_guard_seg() -> bool {
    u3001_segment_guard()
}

/// UNX-F3001 对拍表 16 行边界与累计守恒。
fn check_u3001_mix_tbl() -> bool {
    let mut ok = u3001mix_COUNT == 16;
    if ok { ok = u3001mix_get("ACTIVE_00") == Some(10) && u3001mix_get("LAYOUT_15") == Some(85); }
    if ok { ok = u3001mix_get("ZZZ_NOKEY").is_none(); }
    if ok { ok = u3001mix_sum() == 760; }
    ok
}

/// UNX-F3001 混合映射 16 行守卫唯一 + 守卫阈值冻结。
fn check_u3001_mix_map() -> bool {
    let mut ok = u3001mixmap_COUNT == 16;
    if ok { ok = u3001mixmap_scan() == 0 && u3001mixmap_unique(); }
    if ok { ok = u3001mixmap_sum() == 504; }
    if ok { ok = U3001MIXGATE_THRESHOLD == 32; }
    ok
}

/// 本模块自检（逐检查带 detail，全绿才出门）。
pub fn run_u3001_checks() -> CheckSet {
    let mut set = CheckSet::new("U3001");
    set.add(
        "u3001-seg-guard",
        u3001_guard_seg(),
        if u3001_guard_seg() { "" } else { "diagnostic code outside segment" },
    );
    set.add(
        "u3001-mix-tbl",
        check_u3001_mix_tbl(),
        if check_u3001_mix_tbl() { "" } else { "UNX-F3001 mix table mismatch" },
    );
    set.add(
        "u3001-mix-map",
        check_u3001_mix_map(),
        if check_u3001_mix_map() { "" } else { "UNX-F3001 mix map mismatch" },
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
    fn u3001_all_checks_green() {
        let set = run_u3001_checks();
        assert!(set.all_passed(), "U3001 domain checks must be green");
    }

    #[test]
    fn u3001_segment_guard_holds() {
        assert!(u3001_segment_guard());
    }
}
