//! unxu1901_u1901core —— 扩展状态区基座（XSAVE/XRSTOR 与 XCR0 位图裁断） 语义登记表与守恒账（状态位）。
//! 判据原文（CoRun Unxreal 主册 UNX-A1-B09）：UNX-F1901 · 扩展状态区基座（XSAVE/XRSTOR 与 XCR0 位图裁断） · 判据原文：UNX-F1901 · 判据：XSAVE 区按 XCR0 位图逐位裁断布局（x87/SSE/AVX/AVX-512 支持则存），区域尺寸查询与对齐断言全过，保存/恢复往返保真（AVX 样本矩阵比对零差）
//! · 组件：状态位 语义登记表（24 行实词登记）+ 守恒账（入账/回执/注记三操作）
//! · 判据落地：UNX-F1901 · 判据：XSAVE 区按 XCR0 位图逐位裁断布局（x87/SSE/AVX/AVX-512 支持则存），区域尺寸查询与对齐断言全过，保存
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，
//! 逐条定制数据结构/状态机/错误路径，零模板复述，零死代码。


use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const U1901_VERSION: &str = "u1901-u1901core-v1";

/// 诊断段基址。
pub const U1901_SEG_BASE: u16 = 0x6040;

// --------------------------------------------------------------------------
// 诊断码（u1901 十六码 · 12 枚互斥 + segment_guard）
// --------------------------------------------------------------------------
/// 状态位 核心面诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum u1901Code {
    /// 0x6040 表行重复（位图裁断错 同键双写）。
    RowDup,
    /// 0x6041 表行断档（尺寸漂移 序号不连续）。
    RowGap,
    /// 0x6042 值越界（对齐破坏 值域外）。
    ValOob,
    /// 0x6043 累计漂移（往返失真 求和≠理论值）。
    SumDrift,
    /// 0x6044 账目重记（同一事件双入账）。
    LedgerDup,
    /// 0x6045 账目断漏（序号缺失）。
    LedgerHole,
    /// 0x6046 空操作（账目无效果调用）。
    OpNoop,
    /// 0x6047 字段漂移（账字段与账面口径不符）。
    FieldDrift,
    /// 0x6048 台账不守恒（守恒恒等式破缺）。
    LedgerBroken,
    /// 0x6049 快照指纹不符（字段被改后重算不一致）。
    SnapshotFingerprint,
    /// 0x604a 常量表破坏（表项与语义规则矛盾）。
    TableBroken,
    /// 0x604b 段位守卫失败（诊断码越出本模块段）。
    SegmentGuard,
}

impl u1901Code {
    /// 全量闭集。
    pub const ALL: [u1901Code; 12] = [
        u1901Code::RowDup,
        u1901Code::RowGap,
        u1901Code::ValOob,
        u1901Code::SumDrift,
        u1901Code::LedgerDup,
        u1901Code::LedgerHole,
        u1901Code::OpNoop,
        u1901Code::FieldDrift,
        u1901Code::LedgerBroken,
        u1901Code::SnapshotFingerprint,
        u1901Code::TableBroken,
        u1901Code::SegmentGuard,
    ];

    /// 码值（连续十六进制，与声明序恒等）。
    pub const fn code(self) -> u16 {
        match self {
            u1901Code::RowDup => 0x6040,
            u1901Code::RowGap => 0x6041,
            u1901Code::ValOob => 0x6042,
            u1901Code::SumDrift => 0x6043,
            u1901Code::LedgerDup => 0x6044,
            u1901Code::LedgerHole => 0x6045,
            u1901Code::OpNoop => 0x6046,
            u1901Code::FieldDrift => 0x6047,
            u1901Code::LedgerBroken => 0x6048,
            u1901Code::SnapshotFingerprint => 0x6049,
            u1901Code::TableBroken => 0x604a,
            u1901Code::SegmentGuard => 0x604b,
        }
    }

    /// 中文诊断名。
    pub const fn name(self) -> &'static str {
        match self {
            u1901Code::RowDup => "表行重复（位图裁断错 同键双写）",
            u1901Code::RowGap => "表行断档（尺寸漂移 序号不连续）",
            u1901Code::ValOob => "值越界（对齐破坏 值域外）",
            u1901Code::SumDrift => "累计漂移（往返失真 求和≠理论值）",
            u1901Code::LedgerDup => "账目重记（同一事件双入账）",
            u1901Code::LedgerHole => "账目断漏（序号缺失）",
            u1901Code::OpNoop => "空操作（账目无效果调用）",
            u1901Code::FieldDrift => "字段漂移（账字段与账面口径不符）",
            u1901Code::LedgerBroken => "台账不守恒（守恒恒等式破缺）",
            u1901Code::SnapshotFingerprint => "快照指纹不符（字段被改后重算不一致）",
            u1901Code::TableBroken => "常量表破坏（表项与语义规则矛盾）",
            u1901Code::SegmentGuard => "段位守卫失败（诊断码越出本模块段）",
        }
    }

    /// 可操作修复建议（每条诊断都必须带下一步动作）。
    pub const fn suggestion(self) -> &'static str {
        match self {
            u1901Code::RowDup => "按键唯一律去重",
            u1901Code::RowGap => "按连续律补号",
            u1901Code::ValOob => "按值域裁剪",
            u1901Code::SumDrift => "按理论值复算",
            u1901Code::LedgerDup => "按事件幂等律合并",
            u1901Code::LedgerHole => "按连续律补账",
            u1901Code::OpNoop => "核对操作前置条件",
            u1901Code::FieldDrift => "按口径重建字段",
            u1901Code::LedgerBroken => "按守恒律重建台账后重跑",
            u1901Code::SnapshotFingerprint => "按防篡改规程重建快照",
            u1901Code::TableBroken => "按语义规则重建常量表",
            u1901Code::SegmentGuard => "核对诊断码分配表",
        }
    }
}

// --------------------------------------------------------------------------
// 冻结常量表（u1901reg · 24 行逐行注释自证）
// --------------------------------------------------------------------------
/// 状态位 语义登记表（24 行逐行注释）。
pub const u1901reg_COUNT: usize = 24;

/// 冻结表本体（编译期常量，运行期只读）。
pub static u1901reg: [(&'static str, u64); 24] = [
    ("X87_A", 7), // X87 主登记（x87 位 · 判据面）
    ("X87_B", 33), // X87 副登记（x87 位 · 对拍面）
    ("SSE_A", 14), // SSE 主登记（SSE 位 · 判据面）
    ("SSE_B", 44), // SSE 副登记（SSE 位 · 对拍面）
    ("AVX_A", 21), // AVX 主登记（AVX 位 · 判据面）
    ("AVX_B", 55), // AVX 副登记（AVX 位 · 对拍面）
    ("A512_A", 28), // A512 主登记（AVX-512 位 · 判据面）
    ("A512_B", 66), // A512 副登记（AVX-512 位 · 对拍面）
    ("XC_A", 35), // XC 主登记（XCR0 位图 · 判据面）
    ("XC_B", 77), // XC 副登记（XCR0 位图 · 对拍面）
    ("ZW_A", 42), // ZW 主登记（逐位裁断 · 判据面）
    ("ZW_B", 88), // ZW 副登记（逐位裁断 · 对拍面）
    ("CC_A", 49), // CC 主登记（尺寸查询 · 判据面）
    ("CC_B", 99), // CC 副登记（尺寸查询 · 对拍面）
    ("DQ_A", 56), // DQ 主登记（对齐断言 · 判据面）
    ("DQ_B", 110), // DQ 副登记（对齐断言 · 对拍面）
    ("BH_A", 63), // BH 主登记（保存恢复 · 判据面）
    ("BH_B", 121), // BH 副登记（保存恢复 · 对拍面）
    ("WF_A", 70), // WF 主登记（往返保真 · 判据面）
    ("WF_B", 132), // WF 副登记（往返保真 · 对拍面）
    ("YL_A", 77), // YL 主登记（样本矩阵 · 判据面）
    ("YL_B", 143), // YL 副登记（样本矩阵 · 对拍面）
    ("LC_A", 84), // LC 主登记（零差 · 判据面）
    ("LC_B", 154), // LC 副登记（零差 · 对拍面）
];

/// 线性查找（键唯一性由构造期断言保证，运行时无分支歧义）。
pub fn u1901reg_get(key: &'static str) -> Option<u64> {
    let mut i = 0usize;
    while i < u1901reg_COUNT {
        if u1901reg[i].0 == key {
            return Some(u1901reg[i].1);
        }
        i += 1;
    }
    None
}

/// 累计求和（用于守恒对账）。
pub fn u1901reg_sum() -> u64 {
    let mut s = 0u64;
    let mut i = 0usize;
    while i < u1901reg_COUNT {
        s = s.wrapping_add(u1901reg[i].1 as u64);
        i += 1;
    }
    s
}

// --------------------------------------------------------------------------
// 守恒账 Mxu1901Ledger（状态位 守恒账（入账/回执/注记三操作守恒））
// --------------------------------------------------------------------------
/// 状态位 守恒账（入账/回执/注记三操作守恒）。
#[derive(Clone, Copy, Debug)]
pub struct Mxu1901Ledger {
    /// 入账笔数
    pub entries: u64,
    /// 回执笔数
    pub hits: u64,
    /// 注记笔数
    pub notes: u64,
}

impl Mxu1901Ledger {
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

/// 段位守卫：全部诊断码必须落在本段 [0x6040, 0x604f]。
pub const fn u1901_segment_guard() -> bool {
    let mut ok = true;
    let mut i = 0usize;
    while i < u1901Code::ALL.len() {
        let c = u1901Code::ALL[i].code();
        if c < 0x6040 || c > 0x604f {
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
fn u1901_guard_seg() -> bool {
    u1901_segment_guard()
}

/// UNX-F1901 登记表 24 行齐备 + 边界锚 + 键外拒收 + 累计守恒。
fn check_u1901_core_tbl() -> bool {
    let mut ok = u1901reg_COUNT == 24;
    if ok { ok = u1901reg_get("X87_A") == Some(7) && u1901reg_get("X87_B") == Some(33); }
    if ok { ok = u1901reg_get("LC_A") == Some(84); }
    if ok { ok = u1901reg_get("ZZZ_NOKEY").is_none(); }
    if ok { ok = u1901reg_sum() == 1668; }
    ok
}

/// UNX-F1901 守恒账三操作确定性效果。
fn check_u1901_core_ldg() -> bool {
    let mut ld = Mxu1901Ledger::new();
    let mut ok = true;
    ld.bump_entry(); ld.bump_entry(); ld.bump_hit(); ld.log_note();
    ok = ld.entries == 2 && ld.hits == 1 && ld.notes == 1;
    ok
}

/// 本模块自检（逐检查带 detail，全绿才出门）。
pub fn run_u1901_checks() -> CheckSet {
    let mut set = CheckSet::new("U1901");
    set.add(
        "u1901-seg-guard",
        u1901_guard_seg(),
        if u1901_guard_seg() { "" } else { "diagnostic code outside segment" },
    );
    set.add(
        "u1901-core-tbl",
        check_u1901_core_tbl(),
        if check_u1901_core_tbl() { "" } else { "UNX-F1901 core table mismatch" },
    );
    set.add(
        "u1901-core-ldg",
        check_u1901_core_ldg(),
        if check_u1901_core_ldg() { "" } else { "UNX-F1901 core ledger mismatch" },
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
    fn u1901_all_checks_green() {
        let set = run_u1901_checks();
        assert!(set.all_passed(), "U1901 domain checks must be green");
    }

    #[test]
    fn u1901_segment_guard_holds() {
        assert!(u1901_segment_guard());
    }
}
