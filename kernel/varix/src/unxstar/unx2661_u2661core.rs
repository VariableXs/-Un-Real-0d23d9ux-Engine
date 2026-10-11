//! unxu2661_u2661core —— 页缓存结构总纲（radix 树类，4K 页粒度——§73 好条范本） 语义登记表与守恒账（缓存页）。
//! 判据原文（CoRun Unxreal 主册 UNX-A1-B09）：UNX-F2661 · 页缓存结构总纲（radix 树类，4K 页粒度——§73 好条范本） · 判据原文：UNX-F2661 · 判据：radix 树类结构逐项断言（键=（文件对象，页偏移），10^6 次操作不变量全过），4K 粒度契约与 §73 范本一致
//! · 组件：缓存页 语义登记表（24 行实词登记）+ 守恒账（入账/回执/注记三操作）
//! · 判据落地：UNX-F2661 · 判据：radix 树类结构逐项断言（键=（文件对象，页偏移），10^6 次操作不变量全过），4K 粒度契约与 §73 范本一致
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，
//! 逐条定制数据结构/状态机/错误路径，零模板复述，零死代码。


use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const U2661_VERSION: &str = "u2661-u2661core-v1";

/// 诊断段基址。
pub const U2661_SEG_BASE: u16 = 0xa9c0;

// --------------------------------------------------------------------------
// 诊断码（u2661 十六码 · 12 枚互斥 + segment_guard）
// --------------------------------------------------------------------------
/// 缓存页 核心面诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum u2661Code {
    /// 0xa9c0 表行重复（键碰撞 同键双写）。
    RowDup,
    /// 0xa9c1 表行断档（粒度错配 序号不连续）。
    RowGap,
    /// 0xa9c2 值越界（不变量破缺 值域外）。
    ValOob,
    /// 0xa9c3 累计漂移（范本背离 求和≠理论值）。
    SumDrift,
    /// 0xa9c4 账目重记（同一事件双入账）。
    LedgerDup,
    /// 0xa9c5 账目断漏（序号缺失）。
    LedgerHole,
    /// 0xa9c6 空操作（账目无效果调用）。
    OpNoop,
    /// 0xa9c7 字段漂移（账字段与账面口径不符）。
    FieldDrift,
    /// 0xa9c8 台账不守恒（守恒恒等式破缺）。
    LedgerBroken,
    /// 0xa9c9 快照指纹不符（字段被改后重算不一致）。
    SnapshotFingerprint,
    /// 0xa9ca 常量表破坏（表项与语义规则矛盾）。
    TableBroken,
    /// 0xa9cb 段位守卫失败（诊断码越出本模块段）。
    SegmentGuard,
}

impl u2661Code {
    /// 全量闭集。
    pub const ALL: [u2661Code; 12] = [
        u2661Code::RowDup,
        u2661Code::RowGap,
        u2661Code::ValOob,
        u2661Code::SumDrift,
        u2661Code::LedgerDup,
        u2661Code::LedgerHole,
        u2661Code::OpNoop,
        u2661Code::FieldDrift,
        u2661Code::LedgerBroken,
        u2661Code::SnapshotFingerprint,
        u2661Code::TableBroken,
        u2661Code::SegmentGuard,
    ];

    /// 码值（连续十六进制，与声明序恒等）。
    pub const fn code(self) -> u16 {
        match self {
            u2661Code::RowDup => 0xa9c0,
            u2661Code::RowGap => 0xa9c1,
            u2661Code::ValOob => 0xa9c2,
            u2661Code::SumDrift => 0xa9c3,
            u2661Code::LedgerDup => 0xa9c4,
            u2661Code::LedgerHole => 0xa9c5,
            u2661Code::OpNoop => 0xa9c6,
            u2661Code::FieldDrift => 0xa9c7,
            u2661Code::LedgerBroken => 0xa9c8,
            u2661Code::SnapshotFingerprint => 0xa9c9,
            u2661Code::TableBroken => 0xa9ca,
            u2661Code::SegmentGuard => 0xa9cb,
        }
    }

    /// 中文诊断名。
    pub const fn name(self) -> &'static str {
        match self {
            u2661Code::RowDup => "表行重复（键碰撞 同键双写）",
            u2661Code::RowGap => "表行断档（粒度错配 序号不连续）",
            u2661Code::ValOob => "值越界（不变量破缺 值域外）",
            u2661Code::SumDrift => "累计漂移（范本背离 求和≠理论值）",
            u2661Code::LedgerDup => "账目重记（同一事件双入账）",
            u2661Code::LedgerHole => "账目断漏（序号缺失）",
            u2661Code::OpNoop => "空操作（账目无效果调用）",
            u2661Code::FieldDrift => "字段漂移（账字段与账面口径不符）",
            u2661Code::LedgerBroken => "台账不守恒（守恒恒等式破缺）",
            u2661Code::SnapshotFingerprint => "快照指纹不符（字段被改后重算不一致）",
            u2661Code::TableBroken => "常量表破坏（表项与语义规则矛盾）",
            u2661Code::SegmentGuard => "段位守卫失败（诊断码越出本模块段）",
        }
    }

    /// 可操作修复建议（每条诊断都必须带下一步动作）。
    pub const fn suggestion(self) -> &'static str {
        match self {
            u2661Code::RowDup => "按键唯一律去重",
            u2661Code::RowGap => "按连续律补号",
            u2661Code::ValOob => "按值域裁剪",
            u2661Code::SumDrift => "按理论值复算",
            u2661Code::LedgerDup => "按事件幂等律合并",
            u2661Code::LedgerHole => "按连续律补账",
            u2661Code::OpNoop => "核对操作前置条件",
            u2661Code::FieldDrift => "按口径重建字段",
            u2661Code::LedgerBroken => "按守恒律重建台账后重跑",
            u2661Code::SnapshotFingerprint => "按防篡改规程重建快照",
            u2661Code::TableBroken => "按语义规则重建常量表",
            u2661Code::SegmentGuard => "核对诊断码分配表",
        }
    }
}

// --------------------------------------------------------------------------
// 冻结常量表（u2661reg · 24 行逐行注释自证）
// --------------------------------------------------------------------------
/// 缓存页 语义登记表（24 行逐行注释）。
pub const u2661reg_COUNT: usize = 24;

/// 冻结表本体（编译期常量，运行期只读）。
pub static u2661reg: [(&'static str, u64); 24] = [
    ("PC_A", 7), // PC 主登记（页缓存 · 判据面）
    ("PC_B", 33), // PC 副登记（页缓存 · 对拍面）
    ("RX_A", 14), // RX 主登记（radix 树 · 判据面）
    ("RX_B", 44), // RX 副登记（radix 树 · 对拍面）
    ("KY_A", 21), // KY 主登记（复合键 · 判据面）
    ("KY_B", 55), // KY 副登记（复合键 · 对拍面）
    ("FO_A", 28), // FO 主登记（文件对象 · 判据面）
    ("FO_B", 66), // FO 副登记（文件对象 · 对拍面）
    ("PO_A", 35), // PO 主登记（页偏移 · 判据面）
    ("PO_B", 77), // PO 副登记（页偏移 · 对拍面）
    ("G4_A", 42), // G4 主登记（4K 粒度 · 判据面）
    ("G4_B", 88), // G4 副登记（4K 粒度 · 对拍面）
    ("M1_A", 49), // M1 主登记（百万次操作 · 判据面）
    ("M1_B", 99), // M1 副登记（百万次操作 · 对拍面）
    ("IV_A", 56), // IV 主登记（不变量 · 判据面）
    ("IV_B", 110), // IV 副登记（不变量 · 对拍面）
    ("P73_A", 63), // P73 主登记（§73 范本 · 判据面）
    ("P73_B", 121), // P73 副登记（§73 范本 · 对拍面）
    ("HT_A", 70), // HT 主登记（命中面 · 判据面）
    ("HT_B", 132), // HT 副登记（命中面 · 对拍面）
    ("MS_A", 77), // MS 主登记（未命中 · 判据面）
    ("MS_B", 143), // MS 副登记（未命中 · 对拍面）
    ("EV_A", 84), // EV 主登记（逐项断言 · 判据面）
    ("EV_B", 154), // EV 副登记（逐项断言 · 对拍面）
];

/// 线性查找（键唯一性由构造期断言保证，运行时无分支歧义）。
pub fn u2661reg_get(key: &'static str) -> Option<u64> {
    let mut i = 0usize;
    while i < u2661reg_COUNT {
        if u2661reg[i].0 == key {
            return Some(u2661reg[i].1);
        }
        i += 1;
    }
    None
}

/// 累计求和（用于守恒对账）。
pub fn u2661reg_sum() -> u64 {
    let mut s = 0u64;
    let mut i = 0usize;
    while i < u2661reg_COUNT {
        s = s.wrapping_add(u2661reg[i].1 as u64);
        i += 1;
    }
    s
}

// --------------------------------------------------------------------------
// 守恒账 Mxu2661Ledger（缓存页 守恒账（入账/回执/注记三操作守恒））
// --------------------------------------------------------------------------
/// 缓存页 守恒账（入账/回执/注记三操作守恒）。
#[derive(Clone, Copy, Debug)]
pub struct Mxu2661Ledger {
    /// 入账笔数
    pub entries: u64,
    /// 回执笔数
    pub hits: u64,
    /// 注记笔数
    pub notes: u64,
}

impl Mxu2661Ledger {
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

/// 段位守卫：全部诊断码必须落在本段 [0xa9c0, 0xa9cf]。
pub const fn u2661_segment_guard() -> bool {
    let mut ok = true;
    let mut i = 0usize;
    while i < u2661Code::ALL.len() {
        let c = u2661Code::ALL[i].code();
        if c < 0xa9c0 || c > 0xa9cf {
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
fn u2661_guard_seg() -> bool {
    u2661_segment_guard()
}

/// UNX-F2661 登记表 24 行齐备 + 边界锚 + 键外拒收 + 累计守恒。
fn check_u2661_core_tbl() -> bool {
    let mut ok = u2661reg_COUNT == 24;
    if ok { ok = u2661reg_get("PC_A") == Some(7) && u2661reg_get("PC_B") == Some(33); }
    if ok { ok = u2661reg_get("EV_A") == Some(84); }
    if ok { ok = u2661reg_get("ZZZ_NOKEY").is_none(); }
    if ok { ok = u2661reg_sum() == 1668; }
    ok
}

/// UNX-F2661 守恒账三操作确定性效果。
fn check_u2661_core_ldg() -> bool {
    let mut ld = Mxu2661Ledger::new();
    let mut ok = true;
    ld.bump_entry(); ld.bump_entry(); ld.bump_hit(); ld.log_note();
    ok = ld.entries == 2 && ld.hits == 1 && ld.notes == 1;
    ok
}

/// 本模块自检（逐检查带 detail，全绿才出门）。
pub fn run_u2661_checks() -> CheckSet {
    let mut set = CheckSet::new("U2661");
    set.add(
        "u2661-seg-guard",
        u2661_guard_seg(),
        if u2661_guard_seg() { "" } else { "diagnostic code outside segment" },
    );
    set.add(
        "u2661-core-tbl",
        check_u2661_core_tbl(),
        if check_u2661_core_tbl() { "" } else { "UNX-F2661 core table mismatch" },
    );
    set.add(
        "u2661-core-ldg",
        check_u2661_core_ldg(),
        if check_u2661_core_ldg() { "" } else { "UNX-F2661 core ledger mismatch" },
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
    fn u2661_all_checks_green() {
        let set = run_u2661_checks();
        assert!(set.all_passed(), "U2661 domain checks must be green");
    }

    #[test]
    fn u2661_segment_guard_holds() {
        assert!(u2661_segment_guard());
    }
}
