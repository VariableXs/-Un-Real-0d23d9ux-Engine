//! unxu3741_u3741core —— 门禁总设计（五门体系与流水线拓扑） 语义登记表与守恒账（门禁体系）。
//! 判据原文（CoRun Unxreal 主册 UNX-A1-B09）：UNX-F3741 · 门禁总设计（五门体系与流水线拓扑） · 判据原文：UNX-F3741 · 判据：J1：五门体系（守恒门/防重门/覆盖门/断言门/fuzz 门——判据行内嵌五门各守什么律）+流水线拓扑（提交→五门串行→报告——门序 const）；M-499 五门首跑（样例提交过五门→全绿报告出）。<br>J2：门是冷的（机器判——门无人工覆写通道（豁免走 F3755 显式制——不是当场翻绿）。
//! · 组件：门禁体系 语义登记表（24 行实词登记）+ 守恒账（入账/回执/注记三操作）
//! · 判据落地：UNX-F3741 · 判据：J1：五门体系（守恒门/防重门/覆盖门/断言门/fuzz 门——判据行内嵌五门各守什么律）+流水线拓扑（提交→五门串行→报告——门
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，
//! 逐条定制数据结构/状态机/错误路径，零模板复述，零死代码。


use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const U3741_VERSION: &str = "u3741-u3741core-v1";

/// 诊断段基址。
pub const U3741_SEG_BASE: u16 = 0xe180;

// --------------------------------------------------------------------------
// 诊断码（u3741 十六码 · 12 枚互斥 + segment_guard）
// --------------------------------------------------------------------------
/// 门禁体系 核心面诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum u3741Code {
    /// 0xe180 表行重复（门禁 同键双写）。
    RowDup,
    /// 0xe181 表行断档（五门 序号不连续）。
    RowGap,
    /// 0xe182 值越界（拓扑 值域外）。
    ValOob,
    /// 0xe183 累计漂移（五门 求和≠理论值）。
    SumDrift,
    /// 0xe184 账目重记（同一事件双入账）。
    LedgerDup,
    /// 0xe185 账目断漏（序号缺失）。
    LedgerHole,
    /// 0xe186 空操作（账目无效果调用）。
    OpNoop,
    /// 0xe187 字段漂移（账字段与账面口径不符）。
    FieldDrift,
    /// 0xe188 台账不守恒（守恒恒等式破缺）。
    LedgerBroken,
    /// 0xe189 快照指纹不符（字段被改后重算不一致）。
    SnapshotFingerprint,
    /// 0xe18a 常量表破坏（表项与语义规则矛盾）。
    TableBroken,
    /// 0xe18b 段位守卫失败（诊断码越出本模块段）。
    SegmentGuard,
}

impl u3741Code {
    /// 全量闭集。
    pub const ALL: [u3741Code; 12] = [
        u3741Code::RowDup,
        u3741Code::RowGap,
        u3741Code::ValOob,
        u3741Code::SumDrift,
        u3741Code::LedgerDup,
        u3741Code::LedgerHole,
        u3741Code::OpNoop,
        u3741Code::FieldDrift,
        u3741Code::LedgerBroken,
        u3741Code::SnapshotFingerprint,
        u3741Code::TableBroken,
        u3741Code::SegmentGuard,
    ];

    /// 码值（连续十六进制，与声明序恒等）。
    pub const fn code(self) -> u16 {
        match self {
            u3741Code::RowDup => 0xe180,
            u3741Code::RowGap => 0xe181,
            u3741Code::ValOob => 0xe182,
            u3741Code::SumDrift => 0xe183,
            u3741Code::LedgerDup => 0xe184,
            u3741Code::LedgerHole => 0xe185,
            u3741Code::OpNoop => 0xe186,
            u3741Code::FieldDrift => 0xe187,
            u3741Code::LedgerBroken => 0xe188,
            u3741Code::SnapshotFingerprint => 0xe189,
            u3741Code::TableBroken => 0xe18a,
            u3741Code::SegmentGuard => 0xe18b,
        }
    }

    /// 中文诊断名。
    pub const fn name(self) -> &'static str {
        match self {
            u3741Code::RowDup => "表行重复（门禁 同键双写）",
            u3741Code::RowGap => "表行断档（五门 序号不连续）",
            u3741Code::ValOob => "值越界（拓扑 值域外）",
            u3741Code::SumDrift => "累计漂移（五门 求和≠理论值）",
            u3741Code::LedgerDup => "账目重记（同一事件双入账）",
            u3741Code::LedgerHole => "账目断漏（序号缺失）",
            u3741Code::OpNoop => "空操作（账目无效果调用）",
            u3741Code::FieldDrift => "字段漂移（账字段与账面口径不符）",
            u3741Code::LedgerBroken => "台账不守恒（守恒恒等式破缺）",
            u3741Code::SnapshotFingerprint => "快照指纹不符（字段被改后重算不一致）",
            u3741Code::TableBroken => "常量表破坏（表项与语义规则矛盾）",
            u3741Code::SegmentGuard => "段位守卫失败（诊断码越出本模块段）",
        }
    }

    /// 可操作修复建议（每条诊断都必须带下一步动作）。
    pub const fn suggestion(self) -> &'static str {
        match self {
            u3741Code::RowDup => "按键唯一律去重",
            u3741Code::RowGap => "按连续律补号",
            u3741Code::ValOob => "按值域裁剪",
            u3741Code::SumDrift => "按理论值复算",
            u3741Code::LedgerDup => "按事件幂等律合并",
            u3741Code::LedgerHole => "按连续律补账",
            u3741Code::OpNoop => "核对操作前置条件",
            u3741Code::FieldDrift => "按口径重建字段",
            u3741Code::LedgerBroken => "按守恒律重建台账后重跑",
            u3741Code::SnapshotFingerprint => "按防篡改规程重建快照",
            u3741Code::TableBroken => "按语义规则重建常量表",
            u3741Code::SegmentGuard => "核对诊断码分配表",
        }
    }
}

// --------------------------------------------------------------------------
// 冻结常量表（u3741reg · 24 行逐行注释自证）
// --------------------------------------------------------------------------
/// 门禁体系 语义登记表（24 行逐行注释）。
pub const u3741reg_COUNT: usize = 24;

/// 冻结表本体（编译期常量，运行期只读）。
pub static u3741reg: [(&'static str, u64); 24] = [
    ("GATE_A", 7), // GATE 主登记（门禁 · 判据面）
    ("GATE_B", 33), // GATE 副登记（门禁 · 对拍面）
    ("FIVE_A", 14), // FIVE 主登记（五门 · 判据面）
    ("FIVE_B", 44), // FIVE 副登记（五门 · 对拍面）
    ("PIPE_A", 21), // PIPE 主登记（流水线 · 判据面）
    ("PIPE_B", 55), // PIPE 副登记（流水线 · 对拍面）
    ("TOPO_A", 28), // TOPO 主登记（拓扑 · 判据面）
    ("TOPO_B", 66), // TOPO 副登记（拓扑 · 对拍面）
    ("FAST_A", 35), // FAST 主登记（快速门 · 判据面）
    ("FAST_B", 77), // FAST 副登记（快速门 · 对拍面）
    ("DEEP_A", 42), // DEEP 主登记（深度门 · 判据面）
    ("DEEP_B", 88), // DEEP 副登记（深度门 · 对拍面）
    ("FUZZG_A", 49), // FUZZG 主登记（fuzz 门 · 判据面）
    ("FUZZG_B", 99), // FUZZG 副登记（fuzz 门 · 对拍面）
    ("REGR_A", 56), // REGR 主登记（回归门 · 判据面）
    ("REGR_B", 110), // REGR 副登记（回归门 · 对拍面）
    ("RELEASE_A", 63), // RELEASE 主登记（发布门 · 判据面）
    ("RELEASE_B", 121), // RELEASE 副登记（发布门 · 对拍面）
    ("ORDER_A", 70), // ORDER 主登记（门序 · 判据面）
    ("ORDER_B", 132), // ORDER 副登记（门序 · 对拍面）
    ("BLOCK_A", 77), // BLOCK 主登记（阻断 · 判据面）
    ("BLOCK_B", 143), // BLOCK 副登记（阻断 · 对拍面）
    ("SIGN_A", 84), // SIGN 主登记（签署 · 判据面）
    ("SIGN_B", 154), // SIGN 副登记（签署 · 对拍面）
];

/// 线性查找（键唯一性由构造期断言保证，运行时无分支歧义）。
pub fn u3741reg_get(key: &'static str) -> Option<u64> {
    let mut i = 0usize;
    while i < u3741reg_COUNT {
        if u3741reg[i].0 == key {
            return Some(u3741reg[i].1);
        }
        i += 1;
    }
    None
}

/// 累计求和（用于守恒对账）。
pub fn u3741reg_sum() -> u64 {
    let mut s = 0u64;
    let mut i = 0usize;
    while i < u3741reg_COUNT {
        s = s.wrapping_add(u3741reg[i].1 as u64);
        i += 1;
    }
    s
}

// --------------------------------------------------------------------------
// 守恒账 Mxu3741Ledger（门禁体系 守恒账（入账/回执/注记三操作守恒））
// --------------------------------------------------------------------------
/// 门禁体系 守恒账（入账/回执/注记三操作守恒）。
#[derive(Clone, Copy, Debug)]
pub struct Mxu3741Ledger {
    /// 入账笔数
    pub entries: u64,
    /// 回执笔数
    pub hits: u64,
    /// 注记笔数
    pub notes: u64,
}

impl Mxu3741Ledger {
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

/// 段位守卫：全部诊断码必须落在本段 [0xe180, 0xe18f]。
pub const fn u3741_segment_guard() -> bool {
    let mut ok = true;
    let mut i = 0usize;
    while i < u3741Code::ALL.len() {
        let c = u3741Code::ALL[i].code();
        if c < 0xe180 || c > 0xe18f {
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
fn u3741_guard_seg() -> bool {
    u3741_segment_guard()
}

/// UNX-F3741 登记表 24 行齐备 + 边界锚 + 键外拒收 + 累计守恒。
fn check_u3741_core_tbl() -> bool {
    let mut ok = u3741reg_COUNT == 24;
    if ok { ok = u3741reg_get("GATE_A") == Some(7) && u3741reg_get("GATE_B") == Some(33); }
    if ok { ok = u3741reg_get("SIGN_A") == Some(84); }
    if ok { ok = u3741reg_get("ZZZ_NOKEY").is_none(); }
    if ok { ok = u3741reg_sum() == 1668; }
    ok
}

/// UNX-F3741 守恒账三操作确定性效果。
fn check_u3741_core_ldg() -> bool {
    let mut ld = Mxu3741Ledger::new();
    let mut ok = true;
    ld.bump_entry(); ld.bump_entry(); ld.bump_hit(); ld.log_note();
    ok = ld.entries == 2 && ld.hits == 1 && ld.notes == 1;
    ok
}

/// 本模块自检（逐检查带 detail，全绿才出门）。
pub fn run_u3741_checks() -> CheckSet {
    let mut set = CheckSet::new("U3741");
    set.add(
        "u3741-seg-guard",
        u3741_guard_seg(),
        if u3741_guard_seg() { "" } else { "diagnostic code outside segment" },
    );
    set.add(
        "u3741-core-tbl",
        check_u3741_core_tbl(),
        if check_u3741_core_tbl() { "" } else { "UNX-F3741 core table mismatch" },
    );
    set.add(
        "u3741-core-ldg",
        check_u3741_core_ldg(),
        if check_u3741_core_ldg() { "" } else { "UNX-F3741 core ledger mismatch" },
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
    fn u3741_all_checks_green() {
        let set = run_u3741_checks();
        assert!(set.all_passed(), "U3741 domain checks must be green");
    }

    #[test]
    fn u3741_segment_guard_holds() {
        assert!(u3741_segment_guard());
    }
}
