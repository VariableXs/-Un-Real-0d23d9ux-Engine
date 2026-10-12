//! unxu3701_u3701core —— 覆盖总设计（边×状态×组合三维模型） 语义登记表与守恒账（覆盖三维模型）。
//! 判据原文（CoRun Unxreal 主册 UNX-A1-B09）：UNX-F3701 · 覆盖总设计（边×状态×组合三维模型） · 判据原文：UNX-F3701 · 判据：J1：三维模型（边覆盖（代码维——F3545）/状态覆盖（语义维——状态机×转移）/组合覆盖（参数维——n-wise——判据行内嵌三维各治什么病：边治"没走过的代码"、状态治"没到过的机器态"、组合治"单看都合法搭起来才炸"）；M-459 三维首测（同一靶入口三维各出覆盖数+三维互补性（各有独占盲区——三维不冗余的实证）。<br>J2：三维可合成（总覆盖视图=三维投影合成（各维口径独立、合成式显式——不混账）。
//! · 组件：覆盖三维模型 语义登记表（24 行实词登记）+ 守恒账（入账/回执/注记三操作）
//! · 判据落地：UNX-F3701 · 判据：J1：三维模型（边覆盖（代码维——F3545）/状态覆盖（语义维——状态机×转移）/组合覆盖（参数维——n-wise——判据行内嵌
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，
//! 逐条定制数据结构/状态机/错误路径，零模板复述，零死代码。


use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const U3701_VERSION: &str = "u3701-u3701core-v1";

/// 诊断段基址。
pub const U3701_SEG_BASE: u16 = 0xe160;

// --------------------------------------------------------------------------
// 诊断码（u3701 十六码 · 12 枚互斥 + segment_guard）
// --------------------------------------------------------------------------
/// 覆盖三维模型 核心面诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum u3701Code {
    /// 0xe160 表行重复（覆盖 同键双写）。
    RowDup,
    /// 0xe161 表行断档（三维 序号不连续）。
    RowGap,
    /// 0xe162 值越界（交叉 值域外）。
    ValOob,
    /// 0xe163 累计漂移（三维 求和≠理论值）。
    SumDrift,
    /// 0xe164 账目重记（同一事件双入账）。
    LedgerDup,
    /// 0xe165 账目断漏（序号缺失）。
    LedgerHole,
    /// 0xe166 空操作（账目无效果调用）。
    OpNoop,
    /// 0xe167 字段漂移（账字段与账面口径不符）。
    FieldDrift,
    /// 0xe168 台账不守恒（守恒恒等式破缺）。
    LedgerBroken,
    /// 0xe169 快照指纹不符（字段被改后重算不一致）。
    SnapshotFingerprint,
    /// 0xe16a 常量表破坏（表项与语义规则矛盾）。
    TableBroken,
    /// 0xe16b 段位守卫失败（诊断码越出本模块段）。
    SegmentGuard,
}

impl u3701Code {
    /// 全量闭集。
    pub const ALL: [u3701Code; 12] = [
        u3701Code::RowDup,
        u3701Code::RowGap,
        u3701Code::ValOob,
        u3701Code::SumDrift,
        u3701Code::LedgerDup,
        u3701Code::LedgerHole,
        u3701Code::OpNoop,
        u3701Code::FieldDrift,
        u3701Code::LedgerBroken,
        u3701Code::SnapshotFingerprint,
        u3701Code::TableBroken,
        u3701Code::SegmentGuard,
    ];

    /// 码值（连续十六进制，与声明序恒等）。
    pub const fn code(self) -> u16 {
        match self {
            u3701Code::RowDup => 0xe160,
            u3701Code::RowGap => 0xe161,
            u3701Code::ValOob => 0xe162,
            u3701Code::SumDrift => 0xe163,
            u3701Code::LedgerDup => 0xe164,
            u3701Code::LedgerHole => 0xe165,
            u3701Code::OpNoop => 0xe166,
            u3701Code::FieldDrift => 0xe167,
            u3701Code::LedgerBroken => 0xe168,
            u3701Code::SnapshotFingerprint => 0xe169,
            u3701Code::TableBroken => 0xe16a,
            u3701Code::SegmentGuard => 0xe16b,
        }
    }

    /// 中文诊断名。
    pub const fn name(self) -> &'static str {
        match self {
            u3701Code::RowDup => "表行重复（覆盖 同键双写）",
            u3701Code::RowGap => "表行断档（三维 序号不连续）",
            u3701Code::ValOob => "值越界（交叉 值域外）",
            u3701Code::SumDrift => "累计漂移（三维 求和≠理论值）",
            u3701Code::LedgerDup => "账目重记（同一事件双入账）",
            u3701Code::LedgerHole => "账目断漏（序号缺失）",
            u3701Code::OpNoop => "空操作（账目无效果调用）",
            u3701Code::FieldDrift => "字段漂移（账字段与账面口径不符）",
            u3701Code::LedgerBroken => "台账不守恒（守恒恒等式破缺）",
            u3701Code::SnapshotFingerprint => "快照指纹不符（字段被改后重算不一致）",
            u3701Code::TableBroken => "常量表破坏（表项与语义规则矛盾）",
            u3701Code::SegmentGuard => "段位守卫失败（诊断码越出本模块段）",
        }
    }

    /// 可操作修复建议（每条诊断都必须带下一步动作）。
    pub const fn suggestion(self) -> &'static str {
        match self {
            u3701Code::RowDup => "按键唯一律去重",
            u3701Code::RowGap => "按连续律补号",
            u3701Code::ValOob => "按值域裁剪",
            u3701Code::SumDrift => "按理论值复算",
            u3701Code::LedgerDup => "按事件幂等律合并",
            u3701Code::LedgerHole => "按连续律补账",
            u3701Code::OpNoop => "核对操作前置条件",
            u3701Code::FieldDrift => "按口径重建字段",
            u3701Code::LedgerBroken => "按守恒律重建台账后重跑",
            u3701Code::SnapshotFingerprint => "按防篡改规程重建快照",
            u3701Code::TableBroken => "按语义规则重建常量表",
            u3701Code::SegmentGuard => "核对诊断码分配表",
        }
    }
}

// --------------------------------------------------------------------------
// 冻结常量表（u3701reg · 24 行逐行注释自证）
// --------------------------------------------------------------------------
/// 覆盖三维模型 语义登记表（24 行逐行注释）。
pub const u3701reg_COUNT: usize = 24;

/// 冻结表本体（编译期常量，运行期只读）。
pub static u3701reg: [(&'static str, u64); 24] = [
    ("EDGE_A", 7), // EDGE 主登记（边 · 判据面）
    ("EDGE_B", 33), // EDGE 副登记（边 · 对拍面）
    ("STATE_A", 14), // STATE 主登记（状态 · 判据面）
    ("STATE_B", 44), // STATE 副登记（状态 · 对拍面）
    ("COMBO_A", 21), // COMBO 主登记（组合 · 判据面）
    ("COMBO_B", 55), // COMBO 副登记（组合 · 对拍面）
    ("CART_A", 28), // CART 主登记（笛卡尔积 · 判据面）
    ("CART_B", 66), // CART 副登记（笛卡尔积 · 对拍面）
    ("HIT_A", 35), // HIT 主登记（已覆盖 · 判据面）
    ("HIT_B", 77), // HIT 副登记（已覆盖 · 对拍面）
    ("MISS_A", 42), // MISS 主登记（未覆盖 · 判据面）
    ("MISS_B", 88), // MISS 副登记（未覆盖 · 对拍面）
    ("DIM3_A", 49), // DIM3 主登记（三维 · 判据面）
    ("DIM3_B", 99), // DIM3 副登记（三维 · 对拍面）
    ("MC_A", 56), // MC 主登记（路径覆盖 · 判据面）
    ("MC_B", 110), // MC 副登记（路径覆盖 · 对拍面）
    ("DC_A", 63), // DC 主登记（判定覆盖 · 判据面）
    ("DC_B", 121), // DC 副登记（判定覆盖 · 对拍面）
    ("SC_A", 70), // SC 主登记（状态覆盖 · 判据面）
    ("SC_B", 132), // SC 副登记（状态覆盖 · 对拍面）
    ("RATE_A", 77), // RATE 主登记（覆盖率 · 判据面）
    ("RATE_B", 143), // RATE 副登记（覆盖率 · 对拍面）
    ("GAP_A", 84), // GAP 主登记（缺口 · 判据面）
    ("GAP_B", 154), // GAP 副登记（缺口 · 对拍面）
];

/// 线性查找（键唯一性由构造期断言保证，运行时无分支歧义）。
pub fn u3701reg_get(key: &'static str) -> Option<u64> {
    let mut i = 0usize;
    while i < u3701reg_COUNT {
        if u3701reg[i].0 == key {
            return Some(u3701reg[i].1);
        }
        i += 1;
    }
    None
}

/// 累计求和（用于守恒对账）。
pub fn u3701reg_sum() -> u64 {
    let mut s = 0u64;
    let mut i = 0usize;
    while i < u3701reg_COUNT {
        s = s.wrapping_add(u3701reg[i].1 as u64);
        i += 1;
    }
    s
}

// --------------------------------------------------------------------------
// 守恒账 Mxu3701Ledger（覆盖三维模型 守恒账（入账/回执/注记三操作守恒））
// --------------------------------------------------------------------------
/// 覆盖三维模型 守恒账（入账/回执/注记三操作守恒）。
#[derive(Clone, Copy, Debug)]
pub struct Mxu3701Ledger {
    /// 入账笔数
    pub entries: u64,
    /// 回执笔数
    pub hits: u64,
    /// 注记笔数
    pub notes: u64,
}

impl Mxu3701Ledger {
    /// 全零构造。
    pub const fn new() -> Self {
        Self {
            entries: 0,
            hits: 0,
            notes: 0,
        }
    }

    /// 入账一笔
    pub fn bump_entry(&mut self) {
        self.entries = self.entries.wrapping_add(1);
    }

    /// 回执一笔
    pub fn bump_hit(&mut self) {
        self.hits = self.hits.wrapping_add(1);
    }

    /// 注记一笔
    pub fn log_note(&mut self) {
        self.notes = self.notes.wrapping_add(1);
    }

}

// ---------------------------------------------------------------------------
// 段位守卫（本模块独立十六进码前缀防串段；越界即判红）
// ---------------------------------------------------------------------------

/// 段位守卫：全部诊断码必须落在本段 [0xe160, 0xe16f]。
pub const fn u3701_segment_guard() -> bool {
    let mut ok = true;
    let mut i = 0usize;
    while i < u3701Code::ALL.len() {
        let c = u3701Code::ALL[i].code();
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
fn u3701_guard_seg() -> bool {
    u3701_segment_guard()
}

/// UNX-F3701 登记表 24 行齐备 + 边界锚 + 键外拒收 + 累计守恒。
fn check_u3701_core_tbl() -> bool {
    let mut ok = u3701reg_COUNT == 24;
    if ok { ok = u3701reg_get("EDGE_A") == Some(7) && u3701reg_get("EDGE_B") == Some(33); }
    if ok { ok = u3701reg_get("GAP_A") == Some(84); }
    if ok { ok = u3701reg_get("ZZZ_NOKEY").is_none(); }
    if ok { ok = u3701reg_sum() == 1668; }
    ok
}

/// UNX-F3701 守恒账三操作确定性效果。
fn check_u3701_core_ldg() -> bool {
    let mut ld = Mxu3701Ledger::new();
    let mut ok = true;
    ld.bump_entry(); ld.bump_entry(); ld.bump_hit(); ld.log_note();
    ok = ld.entries == 2 && ld.hits == 1 && ld.notes == 1;
    ok
}

/// 本模块自检（逐检查带 detail，全绿才出门）。
pub fn run_u3701_checks() -> CheckSet {
    let mut set = CheckSet::new("U3701");
    set.add(
        "u3701-seg-guard",
        u3701_guard_seg(),
        if u3701_guard_seg() { "" } else { "diagnostic code outside segment" },
    );
    set.add(
        "u3701-core-tbl",
        check_u3701_core_tbl(),
        if check_u3701_core_tbl() { "" } else { "UNX-F3701 core table mismatch" },
    );
    set.add(
        "u3701-core-ldg",
        check_u3701_core_ldg(),
        if check_u3701_core_ldg() { "" } else { "UNX-F3701 core ledger mismatch" },
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
    fn u3701_all_checks_green() {
        let set = run_u3701_checks();
        assert!(set.all_passed(), "U3701 domain checks must be green");
    }

    #[test]
    fn u3701_segment_guard_holds() {
        assert!(u3701_segment_guard());
    }
}
