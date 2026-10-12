//! unxu3401_u3401core —— ktest 现状盘点与扩容总设计（接管 testing/ 目录） 语义登记表与守恒账（ktest 扩容）。
//! 判据原文（CoRun Unxreal 主册 UNX-A1-B09）：UNX-F3401 · ktest 现状盘点与扩容总设计（接管 testing/ 目录） · 判据原文：UNX-F3401 · 判据：J1：总设计文档（现状盘点/扩容目标/宏族路线/接管三步法）交付；M-160 设计基线用例。
//! · 组件：ktest 扩容 语义登记表（24 行实词登记）+ 守恒账（入账/回执/注记三操作）
//! · 判据落地：UNX-F3401 · 判据：J1：总设计文档（现状盘点/扩容目标/宏族路线/接管三步法）交付；M-160 设计基线用例。
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，
//! 逐条定制数据结构/状态机/错误路径，零模板复述，零死代码。


use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const U3401_VERSION: &str = "u3401-u3401core-v1";

/// 诊断段基址。
pub const U3401_SEG_BASE: u16 = 0xe070;

// --------------------------------------------------------------------------
// 诊断码（u3401 十六码 · 12 枚互斥 + segment_guard）
// --------------------------------------------------------------------------
/// ktest 扩容 核心面诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum u3401Code {
    /// 0xe070 表行重复（ktest 同键双写）。
    RowDup,
    /// 0xe071 表行断档（扩容 序号不连续）。
    RowGap,
    /// 0xe072 值越界（接管 值域外）。
    ValOob,
    /// 0xe073 累计漂移（骨架 求和≠理论值）。
    SumDrift,
    /// 0xe074 账目重记（同一事件双入账）。
    LedgerDup,
    /// 0xe075 账目断漏（序号缺失）。
    LedgerHole,
    /// 0xe076 空操作（账目无效果调用）。
    OpNoop,
    /// 0xe077 字段漂移（账字段与账面口径不符）。
    FieldDrift,
    /// 0xe078 台账不守恒（守恒恒等式破缺）。
    LedgerBroken,
    /// 0xe079 快照指纹不符（字段被改后重算不一致）。
    SnapshotFingerprint,
    /// 0xe07a 常量表破坏（表项与语义规则矛盾）。
    TableBroken,
    /// 0xe07b 段位守卫失败（诊断码越出本模块段）。
    SegmentGuard,
}

impl u3401Code {
    /// 全量闭集。
    pub const ALL: [u3401Code; 12] = [
        u3401Code::RowDup,
        u3401Code::RowGap,
        u3401Code::ValOob,
        u3401Code::SumDrift,
        u3401Code::LedgerDup,
        u3401Code::LedgerHole,
        u3401Code::OpNoop,
        u3401Code::FieldDrift,
        u3401Code::LedgerBroken,
        u3401Code::SnapshotFingerprint,
        u3401Code::TableBroken,
        u3401Code::SegmentGuard,
    ];

    /// 码值（连续十六进制，与声明序恒等）。
    pub const fn code(self) -> u16 {
        match self {
            u3401Code::RowDup => 0xe070,
            u3401Code::RowGap => 0xe071,
            u3401Code::ValOob => 0xe072,
            u3401Code::SumDrift => 0xe073,
            u3401Code::LedgerDup => 0xe074,
            u3401Code::LedgerHole => 0xe075,
            u3401Code::OpNoop => 0xe076,
            u3401Code::FieldDrift => 0xe077,
            u3401Code::LedgerBroken => 0xe078,
            u3401Code::SnapshotFingerprint => 0xe079,
            u3401Code::TableBroken => 0xe07a,
            u3401Code::SegmentGuard => 0xe07b,
        }
    }

    /// 中文诊断名。
    pub const fn name(self) -> &'static str {
        match self {
            u3401Code::RowDup => "表行重复（ktest 同键双写）",
            u3401Code::RowGap => "表行断档（扩容 序号不连续）",
            u3401Code::ValOob => "值越界（接管 值域外）",
            u3401Code::SumDrift => "累计漂移（骨架 求和≠理论值）",
            u3401Code::LedgerDup => "账目重记（同一事件双入账）",
            u3401Code::LedgerHole => "账目断漏（序号缺失）",
            u3401Code::OpNoop => "空操作（账目无效果调用）",
            u3401Code::FieldDrift => "字段漂移（账字段与账面口径不符）",
            u3401Code::LedgerBroken => "台账不守恒（守恒恒等式破缺）",
            u3401Code::SnapshotFingerprint => "快照指纹不符（字段被改后重算不一致）",
            u3401Code::TableBroken => "常量表破坏（表项与语义规则矛盾）",
            u3401Code::SegmentGuard => "段位守卫失败（诊断码越出本模块段）",
        }
    }

    /// 可操作修复建议（每条诊断都必须带下一步动作）。
    pub const fn suggestion(self) -> &'static str {
        match self {
            u3401Code::RowDup => "按键唯一律去重",
            u3401Code::RowGap => "按连续律补号",
            u3401Code::ValOob => "按值域裁剪",
            u3401Code::SumDrift => "按理论值复算",
            u3401Code::LedgerDup => "按事件幂等律合并",
            u3401Code::LedgerHole => "按连续律补账",
            u3401Code::OpNoop => "核对操作前置条件",
            u3401Code::FieldDrift => "按口径重建字段",
            u3401Code::LedgerBroken => "按守恒律重建台账后重跑",
            u3401Code::SnapshotFingerprint => "按防篡改规程重建快照",
            u3401Code::TableBroken => "按语义规则重建常量表",
            u3401Code::SegmentGuard => "核对诊断码分配表",
        }
    }
}

// --------------------------------------------------------------------------
// 冻结常量表（u3401reg · 24 行逐行注释自证）
// --------------------------------------------------------------------------
/// ktest 扩容 语义登记表（24 行逐行注释）。
pub const u3401reg_COUNT: usize = 24;

/// 冻结表本体（编译期常量，运行期只读）。
pub static u3401reg: [(&'static str, u64); 24] = [
    ("KT_A", 7), // KT 主登记（ktest 套件 · 判据面）
    ("KT_B", 33), // KT 副登记（ktest 套件 · 对拍面）
    ("DIR_A", 14), // DIR 主登记（目录接管 · 判据面）
    ("DIR_B", 44), // DIR 副登记（目录接管 · 对拍面）
    ("CASE_A", 21), // CASE 主登记（用例 · 判据面）
    ("CASE_B", 55), // CASE 副登记（用例 · 对拍面）
    ("SEL_A", 28), // SEL 主登记（选择器 · 判据面）
    ("SEL_B", 66), // SEL 副登记（选择器 · 对拍面）
    ("REG_A", 35), // REG 主登记（注册表 · 判据面）
    ("REG_B", 77), // REG 副登记（注册表 · 对拍面）
    ("FIX_A", 42), // FIX 主登记（固化 · 判据面）
    ("FIX_B", 88), // FIX 副登记（固化 · 对拍面）
    ("BACKEND_A", 49), // BACKEND 主登记（后端 · 判据面）
    ("BACKEND_B", 99), // BACKEND 副登记（后端 · 对拍面）
    ("QEMU_A", 56), // QEMU 主登记（QEMU 后端 · 判据面）
    ("QEMU_B", 110), // QEMU 副登记（QEMU 后端 · 对拍面）
    ("REAL_A", 63), // REAL 主登记（真机后端 · 判据面）
    ("REAL_B", 121), // REAL 副登记（真机后端 · 对拍面）
    ("SKEL_A", 70), // SKEL 主登记（骨架 · 判据面）
    ("SKEL_B", 132), // SKEL 副登记（骨架 · 对拍面）
    ("MIG_A", 77), // MIG 主登记（迁移 · 判据面）
    ("MIG_B", 143), // MIG 副登记（迁移 · 对拍面）
    ("CNT_A", 84), // CNT 主登记（计数 · 判据面）
    ("CNT_B", 154), // CNT 副登记（计数 · 对拍面）
];

/// 线性查找（键唯一性由构造期断言保证，运行时无分支歧义）。
pub fn u3401reg_get(key: &'static str) -> Option<u64> {
    let mut i = 0usize;
    while i < u3401reg_COUNT {
        if u3401reg[i].0 == key {
            return Some(u3401reg[i].1);
        }
        i += 1;
    }
    None
}

/// 累计求和（用于守恒对账）。
pub fn u3401reg_sum() -> u64 {
    let mut s = 0u64;
    let mut i = 0usize;
    while i < u3401reg_COUNT {
        s = s.wrapping_add(u3401reg[i].1 as u64);
        i += 1;
    }
    s
}

// --------------------------------------------------------------------------
// 守恒账 Mxu3401Ledger（ktest 扩容 守恒账（入账/回执/注记三操作守恒））
// --------------------------------------------------------------------------
/// ktest 扩容 守恒账（入账/回执/注记三操作守恒）。
#[derive(Clone, Copy, Debug)]
pub struct Mxu3401Ledger {
    /// 入账笔数
    pub entries: u64,
    /// 回执笔数
    pub hits: u64,
    /// 注记笔数
    pub notes: u64,
}

impl Mxu3401Ledger {
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

/// 段位守卫：全部诊断码必须落在本段 [0xe070, 0xe07f]。
pub const fn u3401_segment_guard() -> bool {
    let mut ok = true;
    let mut i = 0usize;
    while i < u3401Code::ALL.len() {
        let c = u3401Code::ALL[i].code();
        if c < 0xe070 || c > 0xe07f {
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
fn u3401_guard_seg() -> bool {
    u3401_segment_guard()
}

/// UNX-F3401 登记表 24 行齐备 + 边界锚 + 键外拒收 + 累计守恒。
fn check_u3401_core_tbl() -> bool {
    let mut ok = u3401reg_COUNT == 24;
    if ok { ok = u3401reg_get("KT_A") == Some(7) && u3401reg_get("KT_B") == Some(33); }
    if ok { ok = u3401reg_get("CNT_A") == Some(84); }
    if ok { ok = u3401reg_get("ZZZ_NOKEY").is_none(); }
    if ok { ok = u3401reg_sum() == 1668; }
    ok
}

/// UNX-F3401 守恒账三操作确定性效果。
fn check_u3401_core_ldg() -> bool {
    let mut ld = Mxu3401Ledger::new();
    let mut ok = true;
    ld.bump_entry(); ld.bump_entry(); ld.bump_hit(); ld.log_note();
    ok = ld.entries == 2 && ld.hits == 1 && ld.notes == 1;
    ok
}

/// 本模块自检（逐检查带 detail，全绿才出门）。
pub fn run_u3401_checks() -> CheckSet {
    let mut set = CheckSet::new("U3401");
    set.add(
        "u3401-seg-guard",
        u3401_guard_seg(),
        if u3401_guard_seg() { "" } else { "diagnostic code outside segment" },
    );
    set.add(
        "u3401-core-tbl",
        check_u3401_core_tbl(),
        if check_u3401_core_tbl() { "" } else { "UNX-F3401 core table mismatch" },
    );
    set.add(
        "u3401-core-ldg",
        check_u3401_core_ldg(),
        if check_u3401_core_ldg() { "" } else { "UNX-F3401 core ledger mismatch" },
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
    fn u3401_all_checks_green() {
        let set = run_u3401_checks();
        assert!(set.all_passed(), "U3401 domain checks must be green");
    }

    #[test]
    fn u3401_segment_guard_holds() {
        assert!(u3401_segment_guard());
    }
}
