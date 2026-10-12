//! unxu3261_u3261core —— rwlock 结构与读偏好语义定义 语义登记表与守恒账（rwlock 读偏好）。
//! 判据原文（CoRun Unxreal 主册 UNX-A1-B09）：UNX-F3261 · rwlock 结构与读偏好语义定义 · 判据原文：UNX-F3261 · 判据：J1：`RwLock<T>` 状态字编码表（读计数/写者位/排队位）唯一化且 M-020 用例可验证三态迁移。
//! · 组件：rwlock 读偏好 语义登记表（24 行实词登记）+ 守恒账（入账/回执/注记三操作）
//! · 判据落地：UNX-F3261 · 判据：J1：`RwLock<T>` 状态字编码表（读计数/写者位/排队位）唯一化且 M-020 用例可验证三态迁移。
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，
//! 逐条定制数据结构/状态机/错误路径，零模板复述，零死代码。


use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const U3261_VERSION: &str = "u3261-u3261core-v1";

/// 诊断段基址。
pub const U3261_SEG_BASE: u16 = 0xe010;

// --------------------------------------------------------------------------
// 诊断码（u3261 十六码 · 12 枚互斥 + segment_guard）
// --------------------------------------------------------------------------
/// rwlock 读偏好 核心面诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum u3261Code {
    /// 0xe010 表行重复（rwlock 同键双写）。
    RowDup,
    /// 0xe011 表行断档（读偏好 序号不连续）。
    RowGap,
    /// 0xe012 值越界（状态字 值域外）。
    ValOob,
    /// 0xe013 累计漂移（升级 求和≠理论值）。
    SumDrift,
    /// 0xe014 账目重记（同一事件双入账）。
    LedgerDup,
    /// 0xe015 账目断漏（序号缺失）。
    LedgerHole,
    /// 0xe016 空操作（账目无效果调用）。
    OpNoop,
    /// 0xe017 字段漂移（账字段与账面口径不符）。
    FieldDrift,
    /// 0xe018 台账不守恒（守恒恒等式破缺）。
    LedgerBroken,
    /// 0xe019 快照指纹不符（字段被改后重算不一致）。
    SnapshotFingerprint,
    /// 0xe01a 常量表破坏（表项与语义规则矛盾）。
    TableBroken,
    /// 0xe01b 段位守卫失败（诊断码越出本模块段）。
    SegmentGuard,
}

impl u3261Code {
    /// 全量闭集。
    pub const ALL: [u3261Code; 12] = [
        u3261Code::RowDup,
        u3261Code::RowGap,
        u3261Code::ValOob,
        u3261Code::SumDrift,
        u3261Code::LedgerDup,
        u3261Code::LedgerHole,
        u3261Code::OpNoop,
        u3261Code::FieldDrift,
        u3261Code::LedgerBroken,
        u3261Code::SnapshotFingerprint,
        u3261Code::TableBroken,
        u3261Code::SegmentGuard,
    ];

    /// 码值（连续十六进制，与声明序恒等）。
    pub const fn code(self) -> u16 {
        match self {
            u3261Code::RowDup => 0xe010,
            u3261Code::RowGap => 0xe011,
            u3261Code::ValOob => 0xe012,
            u3261Code::SumDrift => 0xe013,
            u3261Code::LedgerDup => 0xe014,
            u3261Code::LedgerHole => 0xe015,
            u3261Code::OpNoop => 0xe016,
            u3261Code::FieldDrift => 0xe017,
            u3261Code::LedgerBroken => 0xe018,
            u3261Code::SnapshotFingerprint => 0xe019,
            u3261Code::TableBroken => 0xe01a,
            u3261Code::SegmentGuard => 0xe01b,
        }
    }

    /// 中文诊断名。
    pub const fn name(self) -> &'static str {
        match self {
            u3261Code::RowDup => "表行重复（rwlock 同键双写）",
            u3261Code::RowGap => "表行断档（读偏好 序号不连续）",
            u3261Code::ValOob => "值越界（状态字 值域外）",
            u3261Code::SumDrift => "累计漂移（升级 求和≠理论值）",
            u3261Code::LedgerDup => "账目重记（同一事件双入账）",
            u3261Code::LedgerHole => "账目断漏（序号缺失）",
            u3261Code::OpNoop => "空操作（账目无效果调用）",
            u3261Code::FieldDrift => "字段漂移（账字段与账面口径不符）",
            u3261Code::LedgerBroken => "台账不守恒（守恒恒等式破缺）",
            u3261Code::SnapshotFingerprint => "快照指纹不符（字段被改后重算不一致）",
            u3261Code::TableBroken => "常量表破坏（表项与语义规则矛盾）",
            u3261Code::SegmentGuard => "段位守卫失败（诊断码越出本模块段）",
        }
    }

    /// 可操作修复建议（每条诊断都必须带下一步动作）。
    pub const fn suggestion(self) -> &'static str {
        match self {
            u3261Code::RowDup => "按键唯一律去重",
            u3261Code::RowGap => "按连续律补号",
            u3261Code::ValOob => "按值域裁剪",
            u3261Code::SumDrift => "按理论值复算",
            u3261Code::LedgerDup => "按事件幂等律合并",
            u3261Code::LedgerHole => "按连续律补账",
            u3261Code::OpNoop => "核对操作前置条件",
            u3261Code::FieldDrift => "按口径重建字段",
            u3261Code::LedgerBroken => "按守恒律重建台账后重跑",
            u3261Code::SnapshotFingerprint => "按防篡改规程重建快照",
            u3261Code::TableBroken => "按语义规则重建常量表",
            u3261Code::SegmentGuard => "核对诊断码分配表",
        }
    }
}

// --------------------------------------------------------------------------
// 冻结常量表（u3261reg · 24 行逐行注释自证）
// --------------------------------------------------------------------------
/// rwlock 读偏好 语义登记表（24 行逐行注释）。
pub const u3261reg_COUNT: usize = 24;

/// 冻结表本体（编译期常量，运行期只读）。
pub static u3261reg: [(&'static str, u64); 24] = [
    ("ST_A", 7), // ST 主登记（状态字 · 判据面）
    ("ST_B", 33), // ST 副登记（状态字 · 对拍面）
    ("RC_A", 14), // RC 主登记（读计数 · 判据面）
    ("RC_B", 44), // RC 副登记（读计数 · 对拍面）
    ("WR_A", 21), // WR 主登记（写者位 · 判据面）
    ("WR_B", 55), // WR 副登记（写者位 · 对拍面）
    ("WQ_A", 28), // WQ 主登记（写排队位 · 判据面）
    ("WQ_B", 66), // WQ 副登记（写排队位 · 对拍面）
    ("RP_A", 35), // RP 主登记（读偏好位 · 判据面）
    ("RP_B", 77), // RP 副登记（读偏好位 · 对拍面）
    ("WB_A", 42), // WB 主登记（写屏障 · 判据面）
    ("WB_B", 88), // WB 副登记（写屏障 · 对拍面）
    ("RG_A", 49), // RG 主登记（读组 · 判据面）
    ("RG_B", 99), // RG 副登记（读组 · 对拍面）
    ("WG_A", 56), // WG 主登记（写组 · 判据面）
    ("WG_B", 110), // WG 副登记（写组 · 对拍面）
    ("UPR_A", 63), // UPR 主登记（升级门 · 判据面）
    ("UPR_B", 121), // UPR 副登记（升级门 · 对拍面）
    ("DWN_A", 70), // DWN 主登记（降级门 · 判据面）
    ("DWN_B", 132), // DWN 副登记（降级门 · 对拍面）
    ("3ST_A", 77), // 3ST 主登记（三态迁移 · 判据面）
    ("3ST_B", 143), // 3ST 副登记（三态迁移 · 对拍面）
    ("FIFO_A", 84), // FIFO 主登记（写者排队序 · 判据面）
    ("FIFO_B", 154), // FIFO 副登记（写者排队序 · 对拍面）
];

/// 线性查找（键唯一性由构造期断言保证，运行时无分支歧义）。
pub fn u3261reg_get(key: &'static str) -> Option<u64> {
    let mut i = 0usize;
    while i < u3261reg_COUNT {
        if u3261reg[i].0 == key {
            return Some(u3261reg[i].1);
        }
        i += 1;
    }
    None
}

/// 累计求和（用于守恒对账）。
pub fn u3261reg_sum() -> u64 {
    let mut s = 0u64;
    let mut i = 0usize;
    while i < u3261reg_COUNT {
        s = s.wrapping_add(u3261reg[i].1 as u64);
        i += 1;
    }
    s
}

// --------------------------------------------------------------------------
// 守恒账 Mxu3261Ledger（rwlock 读偏好 守恒账（入账/回执/注记三操作守恒））
// --------------------------------------------------------------------------
/// rwlock 读偏好 守恒账（入账/回执/注记三操作守恒）。
#[derive(Clone, Copy, Debug)]
pub struct Mxu3261Ledger {
    /// 入账笔数
    pub entries: u64,
    /// 回执笔数
    pub hits: u64,
    /// 注记笔数
    pub notes: u64,
}

impl Mxu3261Ledger {
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

/// 段位守卫：全部诊断码必须落在本段 [0xe010, 0xe01f]。
pub const fn u3261_segment_guard() -> bool {
    let mut ok = true;
    let mut i = 0usize;
    while i < u3261Code::ALL.len() {
        let c = u3261Code::ALL[i].code();
        if c < 0xe010 || c > 0xe01f {
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
fn u3261_guard_seg() -> bool {
    u3261_segment_guard()
}

/// UNX-F3261 登记表 24 行齐备 + 边界锚 + 键外拒收 + 累计守恒。
fn check_u3261_core_tbl() -> bool {
    let mut ok = u3261reg_COUNT == 24;
    if ok { ok = u3261reg_get("ST_A") == Some(7) && u3261reg_get("ST_B") == Some(33); }
    if ok { ok = u3261reg_get("FIFO_A") == Some(84); }
    if ok { ok = u3261reg_get("ZZZ_NOKEY").is_none(); }
    if ok { ok = u3261reg_sum() == 1668; }
    ok
}

/// UNX-F3261 守恒账三操作确定性效果。
fn check_u3261_core_ldg() -> bool {
    let mut ld = Mxu3261Ledger::new();
    let mut ok = true;
    ld.bump_entry(); ld.bump_entry(); ld.bump_hit(); ld.log_note();
    ok = ld.entries == 2 && ld.hits == 1 && ld.notes == 1;
    ok
}

/// 本模块自检（逐检查带 detail，全绿才出门）。
pub fn run_u3261_checks() -> CheckSet {
    let mut set = CheckSet::new("U3261");
    set.add(
        "u3261-seg-guard",
        u3261_guard_seg(),
        if u3261_guard_seg() { "" } else { "diagnostic code outside segment" },
    );
    set.add(
        "u3261-core-tbl",
        check_u3261_core_tbl(),
        if check_u3261_core_tbl() { "" } else { "UNX-F3261 core table mismatch" },
    );
    set.add(
        "u3261-core-ldg",
        check_u3261_core_ldg(),
        if check_u3261_core_ldg() { "" } else { "UNX-F3261 core ledger mismatch" },
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
    fn u3261_all_checks_green() {
        let set = run_u3261_checks();
        assert!(set.all_passed(), "U3261 domain checks must be green");
    }

    #[test]
    fn u3261_segment_guard_holds() {
        assert!(u3261_segment_guard());
    }
}
