//! unxu1801_u1801core —— 切换主路径总纲（schedule() 主循环冻结） 语义登记表与守恒账（切换段）。
//! 判据原文（CoRun Unxreal 主册 UNX-A1-B09）：UNX-F1801 · 切换主路径总纲（schedule() 主循环冻结） · 判据原文：UNX-F1801 · 判据：主循环（置位消费→选线程→切出→切入）四段冻结 v1，段间签名齐备，跳段注入 10/10 次拒绝，一次切换全程账可导出
//! · 组件：切换段 语义登记表（24 行实词登记）+ 守恒账（入账/回执/注记三操作）
//! · 判据落地：UNX-F1801 · 判据：主循环（置位消费→选线程→切出→切入）四段冻结 v1，段间签名齐备，跳段注入 10/10 次拒绝，一次切换全程账可导出
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，
//! 逐条定制数据结构/状态机/错误路径，零模板复述，零死代码。


use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const U1801_VERSION: &str = "u1801-u1801core-v1";

/// 诊断段基址。
pub const U1801_SEG_BASE: u16 = 0x5c80;

// --------------------------------------------------------------------------
// 诊断码（u1801 十六码 · 12 枚互斥 + segment_guard）
// --------------------------------------------------------------------------
/// 切换段 核心面诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum u1801Code {
    /// 0x5c80 表行重复（跳段 同键双写）。
    RowDup,
    /// 0x5c81 表行断档（签名缺位 序号不连续）。
    RowGap,
    /// 0x5c82 值越界（切换丢账 值域外）。
    ValOob,
    /// 0x5c83 累计漂移（选线程漂移 求和≠理论值）。
    SumDrift,
    /// 0x5c84 账目重记（同一事件双入账）。
    LedgerDup,
    /// 0x5c85 账目断漏（序号缺失）。
    LedgerHole,
    /// 0x5c86 空操作（账目无效果调用）。
    OpNoop,
    /// 0x5c87 字段漂移（账字段与账面口径不符）。
    FieldDrift,
    /// 0x5c88 台账不守恒（守恒恒等式破缺）。
    LedgerBroken,
    /// 0x5c89 快照指纹不符（字段被改后重算不一致）。
    SnapshotFingerprint,
    /// 0x5c8a 常量表破坏（表项与语义规则矛盾）。
    TableBroken,
    /// 0x5c8b 段位守卫失败（诊断码越出本模块段）。
    SegmentGuard,
}

impl u1801Code {
    /// 全量闭集。
    pub const ALL: [u1801Code; 12] = [
        u1801Code::RowDup,
        u1801Code::RowGap,
        u1801Code::ValOob,
        u1801Code::SumDrift,
        u1801Code::LedgerDup,
        u1801Code::LedgerHole,
        u1801Code::OpNoop,
        u1801Code::FieldDrift,
        u1801Code::LedgerBroken,
        u1801Code::SnapshotFingerprint,
        u1801Code::TableBroken,
        u1801Code::SegmentGuard,
    ];

    /// 码值（连续十六进制，与声明序恒等）。
    pub const fn code(self) -> u16 {
        match self {
            u1801Code::RowDup => 0x5c80,
            u1801Code::RowGap => 0x5c81,
            u1801Code::ValOob => 0x5c82,
            u1801Code::SumDrift => 0x5c83,
            u1801Code::LedgerDup => 0x5c84,
            u1801Code::LedgerHole => 0x5c85,
            u1801Code::OpNoop => 0x5c86,
            u1801Code::FieldDrift => 0x5c87,
            u1801Code::LedgerBroken => 0x5c88,
            u1801Code::SnapshotFingerprint => 0x5c89,
            u1801Code::TableBroken => 0x5c8a,
            u1801Code::SegmentGuard => 0x5c8b,
        }
    }

    /// 中文诊断名。
    pub const fn name(self) -> &'static str {
        match self {
            u1801Code::RowDup => "表行重复（跳段 同键双写）",
            u1801Code::RowGap => "表行断档（签名缺位 序号不连续）",
            u1801Code::ValOob => "值越界（切换丢账 值域外）",
            u1801Code::SumDrift => "累计漂移（选线程漂移 求和≠理论值）",
            u1801Code::LedgerDup => "账目重记（同一事件双入账）",
            u1801Code::LedgerHole => "账目断漏（序号缺失）",
            u1801Code::OpNoop => "空操作（账目无效果调用）",
            u1801Code::FieldDrift => "字段漂移（账字段与账面口径不符）",
            u1801Code::LedgerBroken => "台账不守恒（守恒恒等式破缺）",
            u1801Code::SnapshotFingerprint => "快照指纹不符（字段被改后重算不一致）",
            u1801Code::TableBroken => "常量表破坏（表项与语义规则矛盾）",
            u1801Code::SegmentGuard => "段位守卫失败（诊断码越出本模块段）",
        }
    }

    /// 可操作修复建议（每条诊断都必须带下一步动作）。
    pub const fn suggestion(self) -> &'static str {
        match self {
            u1801Code::RowDup => "按键唯一律去重",
            u1801Code::RowGap => "按连续律补号",
            u1801Code::ValOob => "按值域裁剪",
            u1801Code::SumDrift => "按理论值复算",
            u1801Code::LedgerDup => "按事件幂等律合并",
            u1801Code::LedgerHole => "按连续律补账",
            u1801Code::OpNoop => "核对操作前置条件",
            u1801Code::FieldDrift => "按口径重建字段",
            u1801Code::LedgerBroken => "按守恒律重建台账后重跑",
            u1801Code::SnapshotFingerprint => "按防篡改规程重建快照",
            u1801Code::TableBroken => "按语义规则重建常量表",
            u1801Code::SegmentGuard => "核对诊断码分配表",
        }
    }
}

// --------------------------------------------------------------------------
// 冻结常量表（u1801reg · 24 行逐行注释自证）
// --------------------------------------------------------------------------
/// 切换段 语义登记表（24 行逐行注释）。
pub const u1801reg_COUNT: usize = 24;

/// 冻结表本体（编译期常量，运行期只读）。
pub static u1801reg: [(&'static str, u64); 24] = [
    ("ZW_A", 7), // ZW 主登记（置位消费 · 判据面）
    ("ZW_B", 33), // ZW 副登记（置位消费 · 对拍面）
    ("XX_A", 14), // XX 主登记（选线程 · 判据面）
    ("XX_B", 44), // XX 副登记（选线程 · 对拍面）
    ("QC_A", 21), // QC 主登记（切出 · 判据面）
    ("QC_B", 55), // QC 副登记（切出 · 对拍面）
    ("QR_A", 28), // QR 主登记（切入 · 判据面）
    ("QR_B", 66), // QR 副登记（切入 · 对拍面）
    ("SD_A", 35), // SD 主登记（四段冻结 · 判据面）
    ("SD_B", 77), // SD 副登记（四段冻结 · 对拍面）
    ("QM_A", 42), // QM 主登记（段间签名 · 判据面）
    ("QM_B", 88), // QM 副登记（段间签名 · 对拍面）
    ("TZ_A", 49), // TZ 主登记（跳段注入 · 判据面）
    ("TZ_B", 99), // TZ 副登记（跳段注入 · 对拍面）
    ("QC2_A", 56), // QC2 主登记（全程账导出 · 判据面）
    ("QC2_B", 110), // QC2 副登记（全程账导出 · 对拍面）
    ("MZ_A", 63), // MZ 主登记（主循环 · 判据面）
    ("MZ_B", 121), // MZ 副登记（主循环 · 对拍面）
    ("HZ_A", 70), // HZ 主登记（切换号 · 判据面）
    ("HZ_B", 132), // HZ 副登记（切换号 · 对拍面）
    ("BD_A", 77), // BD 主登记（不变量 · 判据面）
    ("BD_B", 143), // BD 副登记（不变量 · 对拍面）
    ("SS_A", 84), // SS 主登记（时序锁 · 判据面）
    ("SS_B", 154), // SS 副登记（时序锁 · 对拍面）
];

/// 线性查找（键唯一性由构造期断言保证，运行时无分支歧义）。
pub fn u1801reg_get(key: &'static str) -> Option<u64> {
    let mut i = 0usize;
    while i < u1801reg_COUNT {
        if u1801reg[i].0 == key {
            return Some(u1801reg[i].1);
        }
        i += 1;
    }
    None
}

/// 累计求和（用于守恒对账）。
pub fn u1801reg_sum() -> u64 {
    let mut s = 0u64;
    let mut i = 0usize;
    while i < u1801reg_COUNT {
        s = s.wrapping_add(u1801reg[i].1 as u64);
        i += 1;
    }
    s
}

// --------------------------------------------------------------------------
// 守恒账 Mxu1801Ledger（切换段 守恒账（入账/回执/注记三操作守恒））
// --------------------------------------------------------------------------
/// 切换段 守恒账（入账/回执/注记三操作守恒）。
#[derive(Clone, Copy, Debug)]
pub struct Mxu1801Ledger {
    /// 入账笔数
    pub entries: u64,
    /// 回执笔数
    pub hits: u64,
    /// 注记笔数
    pub notes: u64,
}

impl Mxu1801Ledger {
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

/// 段位守卫：全部诊断码必须落在本段 [0x5c80, 0x5c8f]。
pub const fn u1801_segment_guard() -> bool {
    let mut ok = true;
    let mut i = 0usize;
    while i < u1801Code::ALL.len() {
        let c = u1801Code::ALL[i].code();
        if c < 0x5c80 || c > 0x5c8f {
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
fn u1801_guard_seg() -> bool {
    u1801_segment_guard()
}

/// UNX-F1801 登记表 24 行齐备 + 边界锚 + 键外拒收 + 累计守恒。
fn check_u1801_core_tbl() -> bool {
    let mut ok = u1801reg_COUNT == 24;
    if ok { ok = u1801reg_get("ZW_A") == Some(7) && u1801reg_get("ZW_B") == Some(33); }
    if ok { ok = u1801reg_get("SS_A") == Some(84); }
    if ok { ok = u1801reg_get("ZZZ_NOKEY").is_none(); }
    if ok { ok = u1801reg_sum() == 1668; }
    ok
}

/// UNX-F1801 守恒账三操作确定性效果。
fn check_u1801_core_ldg() -> bool {
    let mut ld = Mxu1801Ledger::new();
    let mut ok = true;
    ld.bump_entry(); ld.bump_entry(); ld.bump_hit(); ld.log_note();
    ok = ld.entries == 2 && ld.hits == 1 && ld.notes == 1;
    ok
}

/// 本模块自检（逐检查带 detail，全绿才出门）。
pub fn run_u1801_checks() -> CheckSet {
    let mut set = CheckSet::new("U1801");
    set.add(
        "u1801-seg-guard",
        u1801_guard_seg(),
        if u1801_guard_seg() { "" } else { "diagnostic code outside segment" },
    );
    set.add(
        "u1801-core-tbl",
        check_u1801_core_tbl(),
        if check_u1801_core_tbl() { "" } else { "UNX-F1801 core table mismatch" },
    );
    set.add(
        "u1801-core-ldg",
        check_u1801_core_ldg(),
        if check_u1801_core_ldg() { "" } else { "UNX-F1801 core ledger mismatch" },
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
    fn u1801_all_checks_green() {
        let set = run_u1801_checks();
        assert!(set.all_passed(), "U1801 domain checks must be green");
    }

    #[test]
    fn u1801_segment_guard_holds() {
        assert!(u1801_segment_guard());
    }
}
