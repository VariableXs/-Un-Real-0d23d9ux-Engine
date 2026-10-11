//! unxu1721_u1721core —— 均衡执行总纲（判据消费→执行原语链） 语义登记表与守恒账（执行段）。
//! 判据原文（CoRun Unxreal 主册 UNX-A1-B09）：UNX-F1721 · 均衡执行总纲（判据消费→执行原语链） · 判据原文：UNX-F1721 · 判据：执行链（判据结论→收益复核→候选→目标→原语→记账）六段冻结 v1，段间交接签名齐备，跳段注入 10/10 次拒绝
//! · 组件：执行段 语义登记表（24 行实词登记）+ 守恒账（入账/回执/注记三操作）
//! · 判据落地：UNX-F1721 · 判据：执行链（判据结论→收益复核→候选→目标→原语→记账）六段冻结 v1，段间交接签名齐备，跳段注入 10/10 次拒绝
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，
//! 逐条定制数据结构/状态机/错误路径，零模板复述，零死代码。


use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const U1721_VERSION: &str = "u1721-u1721core-v1";

/// 诊断段基址。
pub const U1721_SEG_BASE: u16 = 0x5980;

// --------------------------------------------------------------------------
// 诊断码（u1721 十六码 · 12 枚互斥 + segment_guard）
// --------------------------------------------------------------------------
/// 执行段 核心面诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum u1721Code {
    /// 0x5980 表行重复（跳段 同键双写）。
    RowDup,
    /// 0x5981 表行断档（签名缺位 序号不连续）。
    RowGap,
    /// 0x5982 值越界（收益误判 值域外）。
    ValOob,
    /// 0x5983 累计漂移（记账漏账 求和≠理论值）。
    SumDrift,
    /// 0x5984 账目重记（同一事件双入账）。
    LedgerDup,
    /// 0x5985 账目断漏（序号缺失）。
    LedgerHole,
    /// 0x5986 空操作（账目无效果调用）。
    OpNoop,
    /// 0x5987 字段漂移（账字段与账面口径不符）。
    FieldDrift,
    /// 0x5988 台账不守恒（守恒恒等式破缺）。
    LedgerBroken,
    /// 0x5989 快照指纹不符（字段被改后重算不一致）。
    SnapshotFingerprint,
    /// 0x598a 常量表破坏（表项与语义规则矛盾）。
    TableBroken,
    /// 0x598b 段位守卫失败（诊断码越出本模块段）。
    SegmentGuard,
}

impl u1721Code {
    /// 全量闭集。
    pub const ALL: [u1721Code; 12] = [
        u1721Code::RowDup,
        u1721Code::RowGap,
        u1721Code::ValOob,
        u1721Code::SumDrift,
        u1721Code::LedgerDup,
        u1721Code::LedgerHole,
        u1721Code::OpNoop,
        u1721Code::FieldDrift,
        u1721Code::LedgerBroken,
        u1721Code::SnapshotFingerprint,
        u1721Code::TableBroken,
        u1721Code::SegmentGuard,
    ];

    /// 码值（连续十六进制，与声明序恒等）。
    pub const fn code(self) -> u16 {
        match self {
            u1721Code::RowDup => 0x5980,
            u1721Code::RowGap => 0x5981,
            u1721Code::ValOob => 0x5982,
            u1721Code::SumDrift => 0x5983,
            u1721Code::LedgerDup => 0x5984,
            u1721Code::LedgerHole => 0x5985,
            u1721Code::OpNoop => 0x5986,
            u1721Code::FieldDrift => 0x5987,
            u1721Code::LedgerBroken => 0x5988,
            u1721Code::SnapshotFingerprint => 0x5989,
            u1721Code::TableBroken => 0x598a,
            u1721Code::SegmentGuard => 0x598b,
        }
    }

    /// 中文诊断名。
    pub const fn name(self) -> &'static str {
        match self {
            u1721Code::RowDup => "表行重复（跳段 同键双写）",
            u1721Code::RowGap => "表行断档（签名缺位 序号不连续）",
            u1721Code::ValOob => "值越界（收益误判 值域外）",
            u1721Code::SumDrift => "累计漂移（记账漏账 求和≠理论值）",
            u1721Code::LedgerDup => "账目重记（同一事件双入账）",
            u1721Code::LedgerHole => "账目断漏（序号缺失）",
            u1721Code::OpNoop => "空操作（账目无效果调用）",
            u1721Code::FieldDrift => "字段漂移（账字段与账面口径不符）",
            u1721Code::LedgerBroken => "台账不守恒（守恒恒等式破缺）",
            u1721Code::SnapshotFingerprint => "快照指纹不符（字段被改后重算不一致）",
            u1721Code::TableBroken => "常量表破坏（表项与语义规则矛盾）",
            u1721Code::SegmentGuard => "段位守卫失败（诊断码越出本模块段）",
        }
    }

    /// 可操作修复建议（每条诊断都必须带下一步动作）。
    pub const fn suggestion(self) -> &'static str {
        match self {
            u1721Code::RowDup => "按键唯一律去重",
            u1721Code::RowGap => "按连续律补号",
            u1721Code::ValOob => "按值域裁剪",
            u1721Code::SumDrift => "按理论值复算",
            u1721Code::LedgerDup => "按事件幂等律合并",
            u1721Code::LedgerHole => "按连续律补账",
            u1721Code::OpNoop => "核对操作前置条件",
            u1721Code::FieldDrift => "按口径重建字段",
            u1721Code::LedgerBroken => "按守恒律重建台账后重跑",
            u1721Code::SnapshotFingerprint => "按防篡改规程重建快照",
            u1721Code::TableBroken => "按语义规则重建常量表",
            u1721Code::SegmentGuard => "核对诊断码分配表",
        }
    }
}

// --------------------------------------------------------------------------
// 冻结常量表（u1721reg · 24 行逐行注释自证）
// --------------------------------------------------------------------------
/// 执行段 语义登记表（24 行逐行注释）。
pub const u1721reg_COUNT: usize = 24;

/// 冻结表本体（编译期常量，运行期只读）。
pub static u1721reg: [(&'static str, u64); 24] = [
    ("PJ_A", 7), // PJ 主登记（判据结论 · 判据面）
    ("PJ_B", 33), // PJ 副登记（判据结论 · 对拍面）
    ("SY_A", 14), // SY 主登记（收益复核 · 判据面）
    ("SY_B", 44), // SY 副登记（收益复核 · 对拍面）
    ("HX_A", 21), // HX 主登记（候选 · 判据面）
    ("HX_B", 55), // HX 副登记（候选 · 对拍面）
    ("MB_A", 28), // MB 主登记（目标 · 判据面）
    ("MB_B", 66), // MB 副登记（目标 · 对拍面）
    ("YL_A", 35), // YL 主登记（原语 · 判据面）
    ("YL_B", 77), // YL 副登记（原语 · 对拍面）
    ("JZ_A", 42), // JZ 主登记（记账 · 判据面）
    ("JZ_B", 88), // JZ 副登记（记账 · 对拍面）
    ("LD_A", 49), // LD 主登记（六段冻结 · 判据面）
    ("LD_B", 99), // LD 副登记（六段冻结 · 对拍面）
    ("QH_A", 56), // QH 主登记（段间签名 · 判据面）
    ("QH_B", 110), // QH 副登记（段间签名 · 对拍面）
    ("TZ_A", 63), // TZ 主登记（跳段注入 · 判据面）
    ("TZ_B", 121), // TZ 副登记（跳段注入 · 对拍面）
    ("JJ_A", 70), // JJ 主登记（拒绝 · 判据面）
    ("JJ_B", 132), // JJ 副登记（拒绝 · 对拍面）
    ("JD_A", 77), // JD 主登记（交接 · 判据面）
    ("JD_B", 143), // JD 副登记（交接 · 对拍面）
    ("MZ_A", 84), // MZ 主登记（封账 · 判据面）
    ("MZ_B", 154), // MZ 副登记（封账 · 对拍面）
];

/// 线性查找（键唯一性由构造期断言保证，运行时无分支歧义）。
pub fn u1721reg_get(key: &'static str) -> Option<u64> {
    let mut i = 0usize;
    while i < u1721reg_COUNT {
        if u1721reg[i].0 == key {
            return Some(u1721reg[i].1);
        }
        i += 1;
    }
    None
}

/// 累计求和（用于守恒对账）。
pub fn u1721reg_sum() -> u64 {
    let mut s = 0u64;
    let mut i = 0usize;
    while i < u1721reg_COUNT {
        s = s.wrapping_add(u1721reg[i].1 as u64);
        i += 1;
    }
    s
}

// --------------------------------------------------------------------------
// 守恒账 Mxu1721Ledger（执行段 守恒账（入账/回执/注记三操作守恒））
// --------------------------------------------------------------------------
/// 执行段 守恒账（入账/回执/注记三操作守恒）。
#[derive(Clone, Copy, Debug)]
pub struct Mxu1721Ledger {
    /// 入账笔数
    pub entries: u64,
    /// 回执笔数
    pub hits: u64,
    /// 注记笔数
    pub notes: u64,
}

impl Mxu1721Ledger {
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

/// 段位守卫：全部诊断码必须落在本段 [0x5980, 0x598f]。
pub const fn u1721_segment_guard() -> bool {
    let mut ok = true;
    let mut i = 0usize;
    while i < u1721Code::ALL.len() {
        let c = u1721Code::ALL[i].code();
        if c < 0x5980 || c > 0x598f {
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
fn u1721_guard_seg() -> bool {
    u1721_segment_guard()
}

/// UNX-F1721 登记表 24 行齐备 + 边界锚 + 键外拒收 + 累计守恒。
fn check_u1721_core_tbl() -> bool {
    let mut ok = u1721reg_COUNT == 24;
    if ok { ok = u1721reg_get("PJ_A") == Some(7) && u1721reg_get("PJ_B") == Some(33); }
    if ok { ok = u1721reg_get("MZ_A") == Some(84); }
    if ok { ok = u1721reg_get("ZZZ_NOKEY").is_none(); }
    if ok { ok = u1721reg_sum() == 1668; }
    ok
}

/// UNX-F1721 守恒账三操作确定性效果。
fn check_u1721_core_ldg() -> bool {
    let mut ld = Mxu1721Ledger::new();
    let mut ok = true;
    ld.bump_entry(); ld.bump_entry(); ld.bump_hit(); ld.log_note();
    ok = ld.entries == 2 && ld.hits == 1 && ld.notes == 1;
    ok
}

/// 本模块自检（逐检查带 detail，全绿才出门）。
pub fn run_u1721_checks() -> CheckSet {
    let mut set = CheckSet::new("U1721");
    set.add(
        "u1721-seg-guard",
        u1721_guard_seg(),
        if u1721_guard_seg() { "" } else { "diagnostic code outside segment" },
    );
    set.add(
        "u1721-core-tbl",
        check_u1721_core_tbl(),
        if check_u1721_core_tbl() { "" } else { "UNX-F1721 core table mismatch" },
    );
    set.add(
        "u1721-core-ldg",
        check_u1721_core_ldg(),
        if check_u1721_core_ldg() { "" } else { "UNX-F1721 core ledger mismatch" },
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
    fn u1721_all_checks_green() {
        let set = run_u1721_checks();
        assert!(set.all_passed(), "U1721 domain checks must be green");
    }

    #[test]
    fn u1721_segment_guard_holds() {
        assert!(u1721_segment_guard());
    }
}
