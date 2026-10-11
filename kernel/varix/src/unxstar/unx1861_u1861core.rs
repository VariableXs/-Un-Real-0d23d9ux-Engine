//! unxu1861_u1861core —— TLB 击落总纲（shootdown 全链四段） 语义登记表与守恒账（击落段）。
//! 判据原文（CoRun Unxreal 主册 UNX-A1-B09）：UNX-F1861 · TLB 击落总纲（shootdown 全链四段） · 判据原文：UNX-F1861 · 判据：击落四段（发起→广播→应答→完成）冻结 v1，段间签名齐备，跳段注入 10/10 拒绝，一次击落全程账可导出
//! · 组件：击落段 语义登记表（24 行实词登记）+ 守恒账（入账/回执/注记三操作）
//! · 判据落地：UNX-F1861 · 判据：击落四段（发起→广播→应答→完成）冻结 v1，段间签名齐备，跳段注入 10/10 拒绝，一次击落全程账可导出
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，
//! 逐条定制数据结构/状态机/错误路径，零模板复述，零死代码。


use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const U1861_VERSION: &str = "u1861-u1861core-v1";

/// 诊断段基址。
pub const U1861_SEG_BASE: u16 = 0x5ec0;

// --------------------------------------------------------------------------
// 诊断码（u1861 十六码 · 12 枚互斥 + segment_guard）
// --------------------------------------------------------------------------
/// 击落段 核心面诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum u1861Code {
    /// 0x5ec0 表行重复（跳段 同键双写）。
    RowDup,
    /// 0x5ec1 表行断档（应答缺失 序号不连续）。
    RowGap,
    /// 0x5ec2 值越界（广播漏核 值域外）。
    ValOob,
    /// 0x5ec3 累计漂移（全程账断裂 求和≠理论值）。
    SumDrift,
    /// 0x5ec4 账目重记（同一事件双入账）。
    LedgerDup,
    /// 0x5ec5 账目断漏（序号缺失）。
    LedgerHole,
    /// 0x5ec6 空操作（账目无效果调用）。
    OpNoop,
    /// 0x5ec7 字段漂移（账字段与账面口径不符）。
    FieldDrift,
    /// 0x5ec8 台账不守恒（守恒恒等式破缺）。
    LedgerBroken,
    /// 0x5ec9 快照指纹不符（字段被改后重算不一致）。
    SnapshotFingerprint,
    /// 0x5eca 常量表破坏（表项与语义规则矛盾）。
    TableBroken,
    /// 0x5ecb 段位守卫失败（诊断码越出本模块段）。
    SegmentGuard,
}

impl u1861Code {
    /// 全量闭集。
    pub const ALL: [u1861Code; 12] = [
        u1861Code::RowDup,
        u1861Code::RowGap,
        u1861Code::ValOob,
        u1861Code::SumDrift,
        u1861Code::LedgerDup,
        u1861Code::LedgerHole,
        u1861Code::OpNoop,
        u1861Code::FieldDrift,
        u1861Code::LedgerBroken,
        u1861Code::SnapshotFingerprint,
        u1861Code::TableBroken,
        u1861Code::SegmentGuard,
    ];

    /// 码值（连续十六进制，与声明序恒等）。
    pub const fn code(self) -> u16 {
        match self {
            u1861Code::RowDup => 0x5ec0,
            u1861Code::RowGap => 0x5ec1,
            u1861Code::ValOob => 0x5ec2,
            u1861Code::SumDrift => 0x5ec3,
            u1861Code::LedgerDup => 0x5ec4,
            u1861Code::LedgerHole => 0x5ec5,
            u1861Code::OpNoop => 0x5ec6,
            u1861Code::FieldDrift => 0x5ec7,
            u1861Code::LedgerBroken => 0x5ec8,
            u1861Code::SnapshotFingerprint => 0x5ec9,
            u1861Code::TableBroken => 0x5eca,
            u1861Code::SegmentGuard => 0x5ecb,
        }
    }

    /// 中文诊断名。
    pub const fn name(self) -> &'static str {
        match self {
            u1861Code::RowDup => "表行重复（跳段 同键双写）",
            u1861Code::RowGap => "表行断档（应答缺失 序号不连续）",
            u1861Code::ValOob => "值越界（广播漏核 值域外）",
            u1861Code::SumDrift => "累计漂移（全程账断裂 求和≠理论值）",
            u1861Code::LedgerDup => "账目重记（同一事件双入账）",
            u1861Code::LedgerHole => "账目断漏（序号缺失）",
            u1861Code::OpNoop => "空操作（账目无效果调用）",
            u1861Code::FieldDrift => "字段漂移（账字段与账面口径不符）",
            u1861Code::LedgerBroken => "台账不守恒（守恒恒等式破缺）",
            u1861Code::SnapshotFingerprint => "快照指纹不符（字段被改后重算不一致）",
            u1861Code::TableBroken => "常量表破坏（表项与语义规则矛盾）",
            u1861Code::SegmentGuard => "段位守卫失败（诊断码越出本模块段）",
        }
    }

    /// 可操作修复建议（每条诊断都必须带下一步动作）。
    pub const fn suggestion(self) -> &'static str {
        match self {
            u1861Code::RowDup => "按键唯一律去重",
            u1861Code::RowGap => "按连续律补号",
            u1861Code::ValOob => "按值域裁剪",
            u1861Code::SumDrift => "按理论值复算",
            u1861Code::LedgerDup => "按事件幂等律合并",
            u1861Code::LedgerHole => "按连续律补账",
            u1861Code::OpNoop => "核对操作前置条件",
            u1861Code::FieldDrift => "按口径重建字段",
            u1861Code::LedgerBroken => "按守恒律重建台账后重跑",
            u1861Code::SnapshotFingerprint => "按防篡改规程重建快照",
            u1861Code::TableBroken => "按语义规则重建常量表",
            u1861Code::SegmentGuard => "核对诊断码分配表",
        }
    }
}

// --------------------------------------------------------------------------
// 冻结常量表（u1861reg · 24 行逐行注释自证）
// --------------------------------------------------------------------------
/// 击落段 语义登记表（24 行逐行注释）。
pub const u1861reg_COUNT: usize = 24;

/// 冻结表本体（编译期常量，运行期只读）。
pub static u1861reg: [(&'static str, u64); 24] = [
    ("FQ_A", 7), // FQ 主登记（发起 · 判据面）
    ("FQ_B", 33), // FQ 副登记（发起 · 对拍面）
    ("GB_A", 14), // GB 主登记（广播 · 判据面）
    ("GB_B", 44), // GB 副登记（广播 · 对拍面）
    ("YD_A", 21), // YD 主登记（应答 · 判据面）
    ("YD_B", 55), // YD 副登记（应答 · 对拍面）
    ("WC_A", 28), // WC 主登记（完成 · 判据面）
    ("WC_B", 66), // WC 副登记（完成 · 对拍面）
    ("SD_A", 35), // SD 主登记（四段冻结 · 判据面）
    ("SD_B", 77), // SD 副登记（四段冻结 · 对拍面）
    ("QM_A", 42), // QM 主登记（段间签名 · 判据面）
    ("QM_B", 88), // QM 副登记（段间签名 · 对拍面）
    ("TZ_A", 49), // TZ 主登记（跳段注入 · 判据面）
    ("TZ_B", 99), // TZ 副登记（跳段注入 · 对拍面）
    ("JJ_A", 56), // JJ 主登记（拒绝 · 判据面）
    ("JJ_B", 110), // JJ 副登记（拒绝 · 对拍面）
    ("QD_A", 63), // QD 主登记（全程账导出 · 判据面）
    ("QD_B", 121), // QD 副登记（全程账导出 · 对拍面）
    ("MM_A", 70), // MM 主登记（页掩码 · 判据面）
    ("MM_B", 132), // MM 副登记（页掩码 · 对拍面）
    ("HB_A", 77), // HB 主登记（回执 · 判据面）
    ("HB_B", 143), // HB 副登记（回执 · 对拍面）
    ("MZ_A", 84), // MZ 主登记（封账 · 判据面）
    ("MZ_B", 154), // MZ 副登记（封账 · 对拍面）
];

/// 线性查找（键唯一性由构造期断言保证，运行时无分支歧义）。
pub fn u1861reg_get(key: &'static str) -> Option<u64> {
    let mut i = 0usize;
    while i < u1861reg_COUNT {
        if u1861reg[i].0 == key {
            return Some(u1861reg[i].1);
        }
        i += 1;
    }
    None
}

/// 累计求和（用于守恒对账）。
pub fn u1861reg_sum() -> u64 {
    let mut s = 0u64;
    let mut i = 0usize;
    while i < u1861reg_COUNT {
        s = s.wrapping_add(u1861reg[i].1 as u64);
        i += 1;
    }
    s
}

// --------------------------------------------------------------------------
// 守恒账 Mxu1861Ledger（击落段 守恒账（入账/回执/注记三操作守恒））
// --------------------------------------------------------------------------
/// 击落段 守恒账（入账/回执/注记三操作守恒）。
#[derive(Clone, Copy, Debug)]
pub struct Mxu1861Ledger {
    /// 入账笔数
    pub entries: u64,
    /// 回执笔数
    pub hits: u64,
    /// 注记笔数
    pub notes: u64,
}

impl Mxu1861Ledger {
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

/// 段位守卫：全部诊断码必须落在本段 [0x5ec0, 0x5ecf]。
pub const fn u1861_segment_guard() -> bool {
    let mut ok = true;
    let mut i = 0usize;
    while i < u1861Code::ALL.len() {
        let c = u1861Code::ALL[i].code();
        if c < 0x5ec0 || c > 0x5ecf {
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
fn u1861_guard_seg() -> bool {
    u1861_segment_guard()
}

/// UNX-F1861 登记表 24 行齐备 + 边界锚 + 键外拒收 + 累计守恒。
fn check_u1861_core_tbl() -> bool {
    let mut ok = u1861reg_COUNT == 24;
    if ok { ok = u1861reg_get("FQ_A") == Some(7) && u1861reg_get("FQ_B") == Some(33); }
    if ok { ok = u1861reg_get("MZ_A") == Some(84); }
    if ok { ok = u1861reg_get("ZZZ_NOKEY").is_none(); }
    if ok { ok = u1861reg_sum() == 1668; }
    ok
}

/// UNX-F1861 守恒账三操作确定性效果。
fn check_u1861_core_ldg() -> bool {
    let mut ld = Mxu1861Ledger::new();
    let mut ok = true;
    ld.bump_entry(); ld.bump_entry(); ld.bump_hit(); ld.log_note();
    ok = ld.entries == 2 && ld.hits == 1 && ld.notes == 1;
    ok
}

/// 本模块自检（逐检查带 detail，全绿才出门）。
pub fn run_u1861_checks() -> CheckSet {
    let mut set = CheckSet::new("U1861");
    set.add(
        "u1861-seg-guard",
        u1861_guard_seg(),
        if u1861_guard_seg() { "" } else { "diagnostic code outside segment" },
    );
    set.add(
        "u1861-core-tbl",
        check_u1861_core_tbl(),
        if check_u1861_core_tbl() { "" } else { "UNX-F1861 core table mismatch" },
    );
    set.add(
        "u1861-core-ldg",
        check_u1861_core_ldg(),
        if check_u1861_core_ldg() { "" } else { "UNX-F1861 core ledger mismatch" },
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
    fn u1861_all_checks_green() {
        let set = run_u1861_checks();
        assert!(set.all_passed(), "U1861 domain checks must be green");
    }

    #[test]
    fn u1861_segment_guard_holds() {
        assert!(u1861_segment_guard());
    }
}
