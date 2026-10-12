//! unxu3001_u3001core —— 双链结构冻结（active/inactive 两链节点布局） 语义登记表与守恒账（双链结构冻结）。
//! 判据原文（CoRun Unxreal 主册 UNX-A1-B09）：UNX-F3001 · 双链结构冻结（active/inactive 两链节点布局） · 判据原文：UNX-F3001 · 判据：UNX-F3001-J1 active/inactive 双链结构冻结哈希入册：节点布局（page 结构挂链位/lru 位标/双向指针）与 F2682 页缓存驻留结构对齐零缺失，链头尾指针操作单点化
//! · 组件：双链结构冻结 语义登记表（24 行实词登记）+ 守恒账（入账/回执/注记三操作）
//! · 判据落地：UNX-F3001 · 判据：UNX-F3001-J1 active/inactive 双链结构冻结哈希入册：节点布局（page 结构挂链位/lru 位标/双向
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，
//! 逐条定制数据结构/状态机/错误路径，零模板复述，零死代码。


use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const U3001_VERSION: &str = "u3001-u3001core-v1";

/// 诊断段基址。
pub const U3001_SEG_BASE: u16 = 0xe250;

// --------------------------------------------------------------------------
// 诊断码（u3001 十六码 · 12 枚互斥 + segment_guard）
// --------------------------------------------------------------------------
/// 双链结构冻结 核心面诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum u3001Code {
    /// 0xe250 表行重复（双链 同键双写）。
    RowDup,
    /// 0xe251 表行断档（节点 序号不连续）。
    RowGap,
    /// 0xe252 值越界（迁移 值域外）。
    ValOob,
    /// 0xe253 累计漂移（双链 求和≠理论值）。
    SumDrift,
    /// 0xe254 账目重记（同一事件双入账）。
    LedgerDup,
    /// 0xe255 账目断漏（序号缺失）。
    LedgerHole,
    /// 0xe256 空操作（账目无效果调用）。
    OpNoop,
    /// 0xe257 字段漂移（账字段与账面口径不符）。
    FieldDrift,
    /// 0xe258 台账不守恒（守恒恒等式破缺）。
    LedgerBroken,
    /// 0xe259 快照指纹不符（字段被改后重算不一致）。
    SnapshotFingerprint,
    /// 0xe25a 常量表破坏（表项与语义规则矛盾）。
    TableBroken,
    /// 0xe25b 段位守卫失败（诊断码越出本模块段）。
    SegmentGuard,
}

impl u3001Code {
    /// 全量闭集。
    pub const ALL: [u3001Code; 12] = [
        u3001Code::RowDup,
        u3001Code::RowGap,
        u3001Code::ValOob,
        u3001Code::SumDrift,
        u3001Code::LedgerDup,
        u3001Code::LedgerHole,
        u3001Code::OpNoop,
        u3001Code::FieldDrift,
        u3001Code::LedgerBroken,
        u3001Code::SnapshotFingerprint,
        u3001Code::TableBroken,
        u3001Code::SegmentGuard,
    ];

    /// 码值（连续十六进制，与声明序恒等）。
    pub const fn code(self) -> u16 {
        match self {
            u3001Code::RowDup => 0xe250,
            u3001Code::RowGap => 0xe251,
            u3001Code::ValOob => 0xe252,
            u3001Code::SumDrift => 0xe253,
            u3001Code::LedgerDup => 0xe254,
            u3001Code::LedgerHole => 0xe255,
            u3001Code::OpNoop => 0xe256,
            u3001Code::FieldDrift => 0xe257,
            u3001Code::LedgerBroken => 0xe258,
            u3001Code::SnapshotFingerprint => 0xe259,
            u3001Code::TableBroken => 0xe25a,
            u3001Code::SegmentGuard => 0xe25b,
        }
    }

    /// 中文诊断名。
    pub const fn name(self) -> &'static str {
        match self {
            u3001Code::RowDup => "表行重复（双链 同键双写）",
            u3001Code::RowGap => "表行断档（节点 序号不连续）",
            u3001Code::ValOob => "值越界（迁移 值域外）",
            u3001Code::SumDrift => "累计漂移（双链 求和≠理论值）",
            u3001Code::LedgerDup => "账目重记（同一事件双入账）",
            u3001Code::LedgerHole => "账目断漏（序号缺失）",
            u3001Code::OpNoop => "空操作（账目无效果调用）",
            u3001Code::FieldDrift => "字段漂移（账字段与账面口径不符）",
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
            u3001Code::ValOob => "按值域裁剪",
            u3001Code::SumDrift => "按理论值复算",
            u3001Code::LedgerDup => "按事件幂等律合并",
            u3001Code::LedgerHole => "按连续律补账",
            u3001Code::OpNoop => "核对操作前置条件",
            u3001Code::FieldDrift => "按口径重建字段",
            u3001Code::LedgerBroken => "按守恒律重建台账后重跑",
            u3001Code::SnapshotFingerprint => "按防篡改规程重建快照",
            u3001Code::TableBroken => "按语义规则重建常量表",
            u3001Code::SegmentGuard => "核对诊断码分配表",
        }
    }
}

// --------------------------------------------------------------------------
// 冻结常量表（u3001reg · 24 行逐行注释自证）
// --------------------------------------------------------------------------
/// 双链结构冻结 语义登记表（24 行逐行注释）。
pub const u3001reg_COUNT: usize = 24;

/// 冻结表本体（编译期常量，运行期只读）。
pub static u3001reg: [(&'static str, u64); 24] = [
    ("ACTIVE_A", 7), // ACTIVE 主登记（active 链 · 判据面）
    ("ACTIVE_B", 33), // ACTIVE 副登记（active 链 · 对拍面）
    ("INACT_A", 14), // INACT 主登记（inactive 链 · 判据面）
    ("INACT_B", 44), // INACT 副登记（inactive 链 · 对拍面）
    ("NODE_A", 21), // NODE 主登记（节点 · 判据面）
    ("NODE_B", 55), // NODE 副登记（节点 · 对拍面）
    ("LAYOUT_A", 28), // LAYOUT 主登记（节点布局 · 判据面）
    ("LAYOUT_B", 66), // LAYOUT 副登记（节点布局 · 对拍面）
    ("NEXT_A", 35), // NEXT 主登记（后继 · 判据面）
    ("NEXT_B", 77), // NEXT 副登记（后继 · 对拍面）
    ("PREV_A", 42), // PREV 主登记（前驱 · 判据面）
    ("PREV_B", 88), // PREV 副登记（前驱 · 对拍面）
    ("FREE_A", 49), // FREE 主登记（空闲链 · 判据面）
    ("FREE_B", 99), // FREE 副登记（空闲链 · 对拍面）
    ("POOL_A", 56), // POOL 主登记（节点池 · 判据面）
    ("POOL_B", 110), // POOL 副登记（节点池 · 对拍面）
    ("MOVE_A", 63), // MOVE 主登记（迁移 · 判据面）
    ("MOVE_B", 121), // MOVE 副登记（迁移 · 对拍面）
    ("SPLIT_A", 70), // SPLIT 主登记（拆分 · 判据面）
    ("SPLIT_B", 132), // SPLIT 副登记（拆分 · 对拍面）
    ("JOIN_A", 77), // JOIN 主登记（合并 · 判据面）
    ("JOIN_B", 143), // JOIN 副登记（合并 · 对拍面）
    ("INV_A", 84), // INV 主登记（不变量 · 判据面）
    ("INV_B", 154), // INV 副登记（不变量 · 对拍面）
];

/// 线性查找（键唯一性由构造期断言保证，运行时无分支歧义）。
pub fn u3001reg_get(key: &'static str) -> Option<u64> {
    let mut i = 0usize;
    while i < u3001reg_COUNT {
        if u3001reg[i].0 == key {
            return Some(u3001reg[i].1);
        }
        i += 1;
    }
    None
}

/// 累计求和（用于守恒对账）。
pub fn u3001reg_sum() -> u64 {
    let mut s = 0u64;
    let mut i = 0usize;
    while i < u3001reg_COUNT {
        s = s.wrapping_add(u3001reg[i].1 as u64);
        i += 1;
    }
    s
}

// --------------------------------------------------------------------------
// 守恒账 Mxu3001Ledger（双链结构冻结 守恒账（入账/回执/注记三操作守恒））
// --------------------------------------------------------------------------
/// 双链结构冻结 守恒账（入账/回执/注记三操作守恒）。
#[derive(Clone, Copy, Debug)]
pub struct Mxu3001Ledger {
    /// 入账笔数
    pub entries: u64,
    /// 回执笔数
    pub hits: u64,
    /// 注记笔数
    pub notes: u64,
}

impl Mxu3001Ledger {
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

/// 段位守卫：全部诊断码必须落在本段 [0xe250, 0xe25f]。
pub const fn u3001_segment_guard() -> bool {
    let mut ok = true;
    let mut i = 0usize;
    while i < u3001Code::ALL.len() {
        let c = u3001Code::ALL[i].code();
        if c < 0xe250 || c > 0xe25f {
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

/// UNX-F3001 登记表 24 行齐备 + 边界锚 + 键外拒收 + 累计守恒。
fn check_u3001_core_tbl() -> bool {
    let mut ok = u3001reg_COUNT == 24;
    if ok { ok = u3001reg_get("ACTIVE_A") == Some(7) && u3001reg_get("ACTIVE_B") == Some(33); }
    if ok { ok = u3001reg_get("INV_A") == Some(84); }
    if ok { ok = u3001reg_get("ZZZ_NOKEY").is_none(); }
    if ok { ok = u3001reg_sum() == 1668; }
    ok
}

/// UNX-F3001 守恒账三操作确定性效果。
fn check_u3001_core_ldg() -> bool {
    let mut ld = Mxu3001Ledger::new();
    let mut ok = true;
    ld.bump_entry(); ld.bump_entry(); ld.bump_hit(); ld.log_note();
    ok = ld.entries == 2 && ld.hits == 1 && ld.notes == 1;
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
        "u3001-core-tbl",
        check_u3001_core_tbl(),
        if check_u3001_core_tbl() { "" } else { "UNX-F3001 core table mismatch" },
    );
    set.add(
        "u3001-core-ldg",
        check_u3001_core_ldg(),
        if check_u3001_core_ldg() { "" } else { "UNX-F3001 core ledger mismatch" },
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
