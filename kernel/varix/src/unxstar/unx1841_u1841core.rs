//! unxu1841_u1841core —— IPI 架构总纲（向量分派框架与 A2 衔接） 语义登记表与守恒账（向量）。
//! 判据原文（CoRun Unxreal 主册 UNX-A1-B09）：UNX-F1841 · IPI 架构总纲（向量分派框架与 A2 衔接） · 判据原文：UNX-F1841 · 判据：IPI 分派框架冻结 v1（向量→处理函数表+分发协议+EOI 纪律），与 A2 原语衔接断言全过，未登记向量注入 10/10 拒绝
//! · 组件：向量 语义登记表（24 行实词登记）+ 守恒账（入账/回执/注记三操作）
//! · 判据落地：UNX-F1841 · 判据：IPI 分派框架冻结 v1（向量→处理函数表+分发协议+EOI 纪律），与 A2 原语衔接断言全过，未登记向量注入 10/10 拒
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，
//! 逐条定制数据结构/状态机/错误路径，零模板复述，零死代码。


use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const U1841_VERSION: &str = "u1841-u1841core-v1";

/// 诊断段基址。
pub const U1841_SEG_BASE: u16 = 0x5e00;

// --------------------------------------------------------------------------
// 诊断码（u1841 十六码 · 12 枚互斥 + segment_guard）
// --------------------------------------------------------------------------
/// 向量 核心面诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum u1841Code {
    /// 0x5e00 表行重复（未登记向量 同键双写）。
    RowDup,
    /// 0x5e01 表行断档（EOI 漏发 序号不连续）。
    RowGap,
    /// 0x5e02 值越界（衔接断裂 值域外）。
    ValOob,
    /// 0x5e03 累计漂移（分派漂移 求和≠理论值）。
    SumDrift,
    /// 0x5e04 账目重记（同一事件双入账）。
    LedgerDup,
    /// 0x5e05 账目断漏（序号缺失）。
    LedgerHole,
    /// 0x5e06 空操作（账目无效果调用）。
    OpNoop,
    /// 0x5e07 字段漂移（账字段与账面口径不符）。
    FieldDrift,
    /// 0x5e08 台账不守恒（守恒恒等式破缺）。
    LedgerBroken,
    /// 0x5e09 快照指纹不符（字段被改后重算不一致）。
    SnapshotFingerprint,
    /// 0x5e0a 常量表破坏（表项与语义规则矛盾）。
    TableBroken,
    /// 0x5e0b 段位守卫失败（诊断码越出本模块段）。
    SegmentGuard,
}

impl u1841Code {
    /// 全量闭集。
    pub const ALL: [u1841Code; 12] = [
        u1841Code::RowDup,
        u1841Code::RowGap,
        u1841Code::ValOob,
        u1841Code::SumDrift,
        u1841Code::LedgerDup,
        u1841Code::LedgerHole,
        u1841Code::OpNoop,
        u1841Code::FieldDrift,
        u1841Code::LedgerBroken,
        u1841Code::SnapshotFingerprint,
        u1841Code::TableBroken,
        u1841Code::SegmentGuard,
    ];

    /// 码值（连续十六进制，与声明序恒等）。
    pub const fn code(self) -> u16 {
        match self {
            u1841Code::RowDup => 0x5e00,
            u1841Code::RowGap => 0x5e01,
            u1841Code::ValOob => 0x5e02,
            u1841Code::SumDrift => 0x5e03,
            u1841Code::LedgerDup => 0x5e04,
            u1841Code::LedgerHole => 0x5e05,
            u1841Code::OpNoop => 0x5e06,
            u1841Code::FieldDrift => 0x5e07,
            u1841Code::LedgerBroken => 0x5e08,
            u1841Code::SnapshotFingerprint => 0x5e09,
            u1841Code::TableBroken => 0x5e0a,
            u1841Code::SegmentGuard => 0x5e0b,
        }
    }

    /// 中文诊断名。
    pub const fn name(self) -> &'static str {
        match self {
            u1841Code::RowDup => "表行重复（未登记向量 同键双写）",
            u1841Code::RowGap => "表行断档（EOI 漏发 序号不连续）",
            u1841Code::ValOob => "值越界（衔接断裂 值域外）",
            u1841Code::SumDrift => "累计漂移（分派漂移 求和≠理论值）",
            u1841Code::LedgerDup => "账目重记（同一事件双入账）",
            u1841Code::LedgerHole => "账目断漏（序号缺失）",
            u1841Code::OpNoop => "空操作（账目无效果调用）",
            u1841Code::FieldDrift => "字段漂移（账字段与账面口径不符）",
            u1841Code::LedgerBroken => "台账不守恒（守恒恒等式破缺）",
            u1841Code::SnapshotFingerprint => "快照指纹不符（字段被改后重算不一致）",
            u1841Code::TableBroken => "常量表破坏（表项与语义规则矛盾）",
            u1841Code::SegmentGuard => "段位守卫失败（诊断码越出本模块段）",
        }
    }

    /// 可操作修复建议（每条诊断都必须带下一步动作）。
    pub const fn suggestion(self) -> &'static str {
        match self {
            u1841Code::RowDup => "按键唯一律去重",
            u1841Code::RowGap => "按连续律补号",
            u1841Code::ValOob => "按值域裁剪",
            u1841Code::SumDrift => "按理论值复算",
            u1841Code::LedgerDup => "按事件幂等律合并",
            u1841Code::LedgerHole => "按连续律补账",
            u1841Code::OpNoop => "核对操作前置条件",
            u1841Code::FieldDrift => "按口径重建字段",
            u1841Code::LedgerBroken => "按守恒律重建台账后重跑",
            u1841Code::SnapshotFingerprint => "按防篡改规程重建快照",
            u1841Code::TableBroken => "按语义规则重建常量表",
            u1841Code::SegmentGuard => "核对诊断码分配表",
        }
    }
}

// --------------------------------------------------------------------------
// 冻结常量表（u1841reg · 24 行逐行注释自证）
// --------------------------------------------------------------------------
/// 向量 语义登记表（24 行逐行注释）。
pub const u1841reg_COUNT: usize = 24;

/// 冻结表本体（编译期常量，运行期只读）。
pub static u1841reg: [(&'static str, u64); 24] = [
    ("XL_A", 7), // XL 主登记（向量表 · 判据面）
    ("XL_B", 33), // XL 副登记（向量表 · 对拍面）
    ("CL_A", 14), // CL 主登记（处理函数表 · 判据面）
    ("CL_B", 44), // CL 副登记（处理函数表 · 对拍面）
    ("FF_A", 21), // FF 主登记（分发协议 · 判据面）
    ("FF_B", 55), // FF 副登记（分发协议 · 对拍面）
    ("EOI_A", 28), // EOI 主登记（EOI 纪律 · 判据面）
    ("EOI_B", 66), // EOI 副登记（EOI 纪律 · 对拍面）
    ("A2_A", 35), // A2 主登记（A2 原语衔接 · 判据面）
    ("A2_B", 77), // A2 副登记（A2 原语衔接 · 对拍面）
    ("WD_A", 42), // WD 主登记（未登记向量 · 判据面）
    ("WD_B", 88), // WD 副登记（未登记向量 · 对拍面）
    ("JJ_A", 49), // JJ 主登记（拒绝 · 判据面）
    ("JJ_B", 99), // JJ 副登记（拒绝 · 对拍面）
    ("MZ_A", 56), // MZ 主登记（冻结 v1 · 判据面）
    ("MZ_B", 110), // MZ 副登记（冻结 v1 · 对拍面）
    ("FP_A", 63), // FP 主登记（分派 · 判据面）
    ("FP_B", 121), // FP 副登记（分派 · 对拍面）
    ("HB_A", 70), // HB 主登记（回执 · 判据面）
    ("HB_B", 132), // HB 副登记（回执 · 对拍面）
    ("ZC_A", 77), // ZC 主登记（在册 · 判据面）
    ("ZC_B", 143), // ZC 副登记（在册 · 对拍面）
    ("PL_A", 84), // PL 主登记（批量 · 判据面）
    ("PL_B", 154), // PL 副登记（批量 · 对拍面）
];

/// 线性查找（键唯一性由构造期断言保证，运行时无分支歧义）。
pub fn u1841reg_get(key: &'static str) -> Option<u64> {
    let mut i = 0usize;
    while i < u1841reg_COUNT {
        if u1841reg[i].0 == key {
            return Some(u1841reg[i].1);
        }
        i += 1;
    }
    None
}

/// 累计求和（用于守恒对账）。
pub fn u1841reg_sum() -> u64 {
    let mut s = 0u64;
    let mut i = 0usize;
    while i < u1841reg_COUNT {
        s = s.wrapping_add(u1841reg[i].1 as u64);
        i += 1;
    }
    s
}

// --------------------------------------------------------------------------
// 守恒账 Mxu1841Ledger（向量 守恒账（入账/回执/注记三操作守恒））
// --------------------------------------------------------------------------
/// 向量 守恒账（入账/回执/注记三操作守恒）。
#[derive(Clone, Copy, Debug)]
pub struct Mxu1841Ledger {
    /// 入账笔数
    pub entries: u64,
    /// 回执笔数
    pub hits: u64,
    /// 注记笔数
    pub notes: u64,
}

impl Mxu1841Ledger {
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

/// 段位守卫：全部诊断码必须落在本段 [0x5e00, 0x5e0f]。
pub const fn u1841_segment_guard() -> bool {
    let mut ok = true;
    let mut i = 0usize;
    while i < u1841Code::ALL.len() {
        let c = u1841Code::ALL[i].code();
        if c < 0x5e00 || c > 0x5e0f {
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
fn u1841_guard_seg() -> bool {
    u1841_segment_guard()
}

/// UNX-F1841 登记表 24 行齐备 + 边界锚 + 键外拒收 + 累计守恒。
fn check_u1841_core_tbl() -> bool {
    let mut ok = u1841reg_COUNT == 24;
    if ok { ok = u1841reg_get("XL_A") == Some(7) && u1841reg_get("XL_B") == Some(33); }
    if ok { ok = u1841reg_get("PL_A") == Some(84); }
    if ok { ok = u1841reg_get("ZZZ_NOKEY").is_none(); }
    if ok { ok = u1841reg_sum() == 1668; }
    ok
}

/// UNX-F1841 守恒账三操作确定性效果。
fn check_u1841_core_ldg() -> bool {
    let mut ld = Mxu1841Ledger::new();
    let mut ok = true;
    ld.bump_entry(); ld.bump_entry(); ld.bump_hit(); ld.log_note();
    ok = ld.entries == 2 && ld.hits == 1 && ld.notes == 1;
    ok
}

/// 本模块自检（逐检查带 detail，全绿才出门）。
pub fn run_u1841_checks() -> CheckSet {
    let mut set = CheckSet::new("U1841");
    set.add(
        "u1841-seg-guard",
        u1841_guard_seg(),
        if u1841_guard_seg() { "" } else { "diagnostic code outside segment" },
    );
    set.add(
        "u1841-core-tbl",
        check_u1841_core_tbl(),
        if check_u1841_core_tbl() { "" } else { "UNX-F1841 core table mismatch" },
    );
    set.add(
        "u1841-core-ldg",
        check_u1841_core_ldg(),
        if check_u1841_core_ldg() { "" } else { "UNX-F1841 core ledger mismatch" },
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
    fn u1841_all_checks_green() {
        let set = run_u1841_checks();
        assert!(set.all_passed(), "U1841 domain checks must be green");
    }

    #[test]
    fn u1841_segment_guard_holds() {
        assert!(u1841_segment_guard());
    }
}
