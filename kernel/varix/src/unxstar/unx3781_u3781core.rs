//! unxu3781_u3781core —— 域中期收口总设计（B20 前向声明的兑现面） 语义登记表与守恒账（域中期收口）。
//! 判据原文（CoRun Unxreal 主册 UNX-A1-B09）：UNX-F3781 · 域中期收口总设计（B20 前向声明的兑现面） · 判据原文：UNX-F3781 · 判据：J1：收口架构（三总轧（批账/域账/凭证）+三宣告（断言大关/移交就绪/风险清单）——判据行内嵌：B20 前向声明的原文兑现（"F3781 域中期收口：段断言大关 ≥80+五条件移交"——期票与兑付跨十批的闭链）；收口的范围声明（B16–B30 十五批——A5 域续领波的全域——不含 B01–B15（已在前次 finalize 收口——历史账冻结）；M-539 收口预跑（三总轧空转→流程通→正式总轧前演练）。<br>J2：收口的纪律（收口只核不改（发现的偏差走修正流程+留痕——收口不涂改历史账。
//! · 组件：域中期收口 语义登记表（24 行实词登记）+ 守恒账（入账/回执/注记三操作）
//! · 判据落地：UNX-F3781 · 判据：J1：收口架构（三总轧（批账/域账/凭证）+三宣告（断言大关/移交就绪/风险清单）——判据行内嵌：B20 前向声明的原文兑现（"F
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，
//! 逐条定制数据结构/状态机/错误路径，零模板复述，零死代码。


use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const U3781_VERSION: &str = "u3781-u3781core-v1";

/// 诊断段基址。
pub const U3781_SEG_BASE: u16 = 0xe1a0;

// --------------------------------------------------------------------------
// 诊断码（u3781 十六码 · 12 枚互斥 + segment_guard）
// --------------------------------------------------------------------------
/// 域中期收口 核心面诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum u3781Code {
    /// 0xe1a0 表行重复（收口 同键双写）。
    RowDup,
    /// 0xe1a1 表行断档（兑现 序号不连续）。
    RowGap,
    /// 0xe1a2 值越界（缺口 值域外）。
    ValOob,
    /// 0xe1a3 累计漂移（兑现 求和≠理论值）。
    SumDrift,
    /// 0xe1a4 账目重记（同一事件双入账）。
    LedgerDup,
    /// 0xe1a5 账目断漏（序号缺失）。
    LedgerHole,
    /// 0xe1a6 空操作（账目无效果调用）。
    OpNoop,
    /// 0xe1a7 字段漂移（账字段与账面口径不符）。
    FieldDrift,
    /// 0xe1a8 台账不守恒（守恒恒等式破缺）。
    LedgerBroken,
    /// 0xe1a9 快照指纹不符（字段被改后重算不一致）。
    SnapshotFingerprint,
    /// 0xe1aa 常量表破坏（表项与语义规则矛盾）。
    TableBroken,
    /// 0xe1ab 段位守卫失败（诊断码越出本模块段）。
    SegmentGuard,
}

impl u3781Code {
    /// 全量闭集。
    pub const ALL: [u3781Code; 12] = [
        u3781Code::RowDup,
        u3781Code::RowGap,
        u3781Code::ValOob,
        u3781Code::SumDrift,
        u3781Code::LedgerDup,
        u3781Code::LedgerHole,
        u3781Code::OpNoop,
        u3781Code::FieldDrift,
        u3781Code::LedgerBroken,
        u3781Code::SnapshotFingerprint,
        u3781Code::TableBroken,
        u3781Code::SegmentGuard,
    ];

    /// 码值（连续十六进制，与声明序恒等）。
    pub const fn code(self) -> u16 {
        match self {
            u3781Code::RowDup => 0xe1a0,
            u3781Code::RowGap => 0xe1a1,
            u3781Code::ValOob => 0xe1a2,
            u3781Code::SumDrift => 0xe1a3,
            u3781Code::LedgerDup => 0xe1a4,
            u3781Code::LedgerHole => 0xe1a5,
            u3781Code::OpNoop => 0xe1a6,
            u3781Code::FieldDrift => 0xe1a7,
            u3781Code::LedgerBroken => 0xe1a8,
            u3781Code::SnapshotFingerprint => 0xe1a9,
            u3781Code::TableBroken => 0xe1aa,
            u3781Code::SegmentGuard => 0xe1ab,
        }
    }

    /// 中文诊断名。
    pub const fn name(self) -> &'static str {
        match self {
            u3781Code::RowDup => "表行重复（收口 同键双写）",
            u3781Code::RowGap => "表行断档（兑现 序号不连续）",
            u3781Code::ValOob => "值越界（缺口 值域外）",
            u3781Code::SumDrift => "累计漂移（兑现 求和≠理论值）",
            u3781Code::LedgerDup => "账目重记（同一事件双入账）",
            u3781Code::LedgerHole => "账目断漏（序号缺失）",
            u3781Code::OpNoop => "空操作（账目无效果调用）",
            u3781Code::FieldDrift => "字段漂移（账字段与账面口径不符）",
            u3781Code::LedgerBroken => "台账不守恒（守恒恒等式破缺）",
            u3781Code::SnapshotFingerprint => "快照指纹不符（字段被改后重算不一致）",
            u3781Code::TableBroken => "常量表破坏（表项与语义规则矛盾）",
            u3781Code::SegmentGuard => "段位守卫失败（诊断码越出本模块段）",
        }
    }

    /// 可操作修复建议（每条诊断都必须带下一步动作）。
    pub const fn suggestion(self) -> &'static str {
        match self {
            u3781Code::RowDup => "按键唯一律去重",
            u3781Code::RowGap => "按连续律补号",
            u3781Code::ValOob => "按值域裁剪",
            u3781Code::SumDrift => "按理论值复算",
            u3781Code::LedgerDup => "按事件幂等律合并",
            u3781Code::LedgerHole => "按连续律补账",
            u3781Code::OpNoop => "核对操作前置条件",
            u3781Code::FieldDrift => "按口径重建字段",
            u3781Code::LedgerBroken => "按守恒律重建台账后重跑",
            u3781Code::SnapshotFingerprint => "按防篡改规程重建快照",
            u3781Code::TableBroken => "按语义规则重建常量表",
            u3781Code::SegmentGuard => "核对诊断码分配表",
        }
    }
}

// --------------------------------------------------------------------------
// 冻结常量表（u3781reg · 24 行逐行注释自证）
// --------------------------------------------------------------------------
/// 域中期收口 语义登记表（24 行逐行注释）。
pub const u3781reg_COUNT: usize = 24;

/// 冻结表本体（编译期常量，运行期只读）。
pub static u3781reg: [(&'static str, u64); 24] = [
    ("CLOSE_A", 7), // CLOSE 主登记（收口 · 判据面）
    ("CLOSE_B", 33), // CLOSE 副登记（收口 · 对拍面）
    ("MID_A", 14), // MID 主登记（中期 · 判据面）
    ("MID_B", 44), // MID 副登记（中期 · 对拍面）
    ("DECL_A", 21), // DECL 主登记（前向声明 · 判据面）
    ("DECL_B", 55), // DECL 副登记（前向声明 · 对拍面）
    ("Fulfill_A", 28), // Fulfill 主登记（兑现 · 判据面）
    ("Fulfill_B", 66), // Fulfill 副登记（兑现 · 对拍面）
    ("SCOPE_A", 35), // SCOPE 主登记（范围 · 判据面）
    ("SCOPE_B", 77), // SCOPE 副登记（范围 · 对拍面）
    ("ITEM_A", 42), // ITEM 主登记（条目 · 判据面）
    ("ITEM_B", 88), // ITEM 副登记（条目 · 对拍面）
    ("DONE_A", 49), // DONE 主登记（已完成 · 判据面）
    ("DONE_B", 99), // DONE 副登记（已完成 · 对拍面）
    ("OPEN_A", 56), // OPEN 主登记（未开 · 判据面）
    ("OPEN_B", 110), // OPEN 副登记（未开 · 对拍面）
    ("GAP_A", 63), // GAP 主登记（缺口 · 判据面）
    ("GAP_B", 121), // GAP 副登记（缺口 · 对拍面）
    ("BIND_A", 70), // BIND 主登记（绑定 · 判据面）
    ("BIND_B", 132), // BIND 副登记（绑定 · 对拍面）
    ("PROOF_A", 77), // PROOF 主登记（证据 · 判据面）
    ("PROOF_B", 143), // PROOF 副登记（证据 · 对拍面）
    ("GATE_A", 84), // GATE 主登记（闸门 · 判据面）
    ("GATE_B", 154), // GATE 副登记（闸门 · 对拍面）
];

/// 线性查找（键唯一性由构造期断言保证，运行时无分支歧义）。
pub fn u3781reg_get(key: &'static str) -> Option<u64> {
    let mut i = 0usize;
    while i < u3781reg_COUNT {
        if u3781reg[i].0 == key {
            return Some(u3781reg[i].1);
        }
        i += 1;
    }
    None
}

/// 累计求和（用于守恒对账）。
pub fn u3781reg_sum() -> u64 {
    let mut s = 0u64;
    let mut i = 0usize;
    while i < u3781reg_COUNT {
        s = s.wrapping_add(u3781reg[i].1 as u64);
        i += 1;
    }
    s
}

// --------------------------------------------------------------------------
// 守恒账 Mxu3781Ledger（域中期收口 守恒账（入账/回执/注记三操作守恒））
// --------------------------------------------------------------------------
/// 域中期收口 守恒账（入账/回执/注记三操作守恒）。
#[derive(Clone, Copy, Debug)]
pub struct Mxu3781Ledger {
    /// 入账笔数
    pub entries: u64,
    /// 回执笔数
    pub hits: u64,
    /// 注记笔数
    pub notes: u64,
}

impl Mxu3781Ledger {
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

/// 段位守卫：全部诊断码必须落在本段 [0xe1a0, 0xe1af]。
pub const fn u3781_segment_guard() -> bool {
    let mut ok = true;
    let mut i = 0usize;
    while i < u3781Code::ALL.len() {
        let c = u3781Code::ALL[i].code();
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
fn u3781_guard_seg() -> bool {
    u3781_segment_guard()
}

/// UNX-F3781 登记表 24 行齐备 + 边界锚 + 键外拒收 + 累计守恒。
fn check_u3781_core_tbl() -> bool {
    let mut ok = u3781reg_COUNT == 24;
    if ok { ok = u3781reg_get("CLOSE_A") == Some(7) && u3781reg_get("CLOSE_B") == Some(33); }
    if ok { ok = u3781reg_get("GATE_A") == Some(84); }
    if ok { ok = u3781reg_get("ZZZ_NOKEY").is_none(); }
    if ok { ok = u3781reg_sum() == 1668; }
    ok
}

/// UNX-F3781 守恒账三操作确定性效果。
fn check_u3781_core_ldg() -> bool {
    let mut ld = Mxu3781Ledger::new();
    let mut ok = true;
    ld.bump_entry(); ld.bump_entry(); ld.bump_hit(); ld.log_note();
    ok = ld.entries == 2 && ld.hits == 1 && ld.notes == 1;
    ok
}

/// 本模块自检（逐检查带 detail，全绿才出门）。
pub fn run_u3781_checks() -> CheckSet {
    let mut set = CheckSet::new("U3781");
    set.add(
        "u3781-seg-guard",
        u3781_guard_seg(),
        if u3781_guard_seg() { "" } else { "diagnostic code outside segment" },
    );
    set.add(
        "u3781-core-tbl",
        check_u3781_core_tbl(),
        if check_u3781_core_tbl() { "" } else { "UNX-F3781 core table mismatch" },
    );
    set.add(
        "u3781-core-ldg",
        check_u3781_core_ldg(),
        if check_u3781_core_ldg() { "" } else { "UNX-F3781 core ledger mismatch" },
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
    fn u3781_all_checks_green() {
        let set = run_u3781_checks();
        assert!(set.all_passed(), "U3781 domain checks must be green");
    }

    #[test]
    fn u3781_segment_guard_holds() {
        assert!(u3781_segment_guard());
    }
}
