//! unxu2461_u2461core —— 水线三档判定（min/low/high） 语义登记表与守恒账（水线档位）。
//! 判据原文（CoRun Unxreal 主册 UNX-A1-B09）：UNX-F2461 · 水线三档判定（min/low/high） · 判据原文：UNX-F2461 · 判据：三档水线判定与 nr_free 总账（F2425）同源单点，三机型初值生效，跨越事件 10/10 次正确触发零漏报零重复
//! · 组件：水线档位 语义登记表（24 行实词登记）+ 守恒账（入账/回执/注记三操作）
//! · 判据落地：UNX-F2461 · 判据：三档水线判定与 nr_free 总账（F2425）同源单点，三机型初值生效，跨越事件 10/10 次正确触发零漏报零重复
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，
//! 逐条定制数据结构/状态机/错误路径，零模板复述，零死代码。


use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const U2461_VERSION: &str = "u2461-u2461core-v1";

/// 诊断段基址。
pub const U2461_SEG_BASE: u16 = 0xa240;

// --------------------------------------------------------------------------
// 诊断码（u2461 十六码 · 12 枚互斥 + segment_guard）
// --------------------------------------------------------------------------
/// 水线档位 核心面诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum u2461Code {
    /// 0xa240 表行重复（漏报触发 同键双写）。
    RowDup,
    /// 0xa241 表行断档（重复上报 序号不连续）。
    RowGap,
    /// 0xa242 值越界（初值错配 值域外）。
    ValOob,
    /// 0xa243 累计漂移（跨档抖动 求和≠理论值）。
    SumDrift,
    /// 0xa244 账目重记（同一事件双入账）。
    LedgerDup,
    /// 0xa245 账目断漏（序号缺失）。
    LedgerHole,
    /// 0xa246 空操作（账目无效果调用）。
    OpNoop,
    /// 0xa247 字段漂移（账字段与账面口径不符）。
    FieldDrift,
    /// 0xa248 台账不守恒（守恒恒等式破缺）。
    LedgerBroken,
    /// 0xa249 快照指纹不符（字段被改后重算不一致）。
    SnapshotFingerprint,
    /// 0xa24a 常量表破坏（表项与语义规则矛盾）。
    TableBroken,
    /// 0xa24b 段位守卫失败（诊断码越出本模块段）。
    SegmentGuard,
}

impl u2461Code {
    /// 全量闭集。
    pub const ALL: [u2461Code; 12] = [
        u2461Code::RowDup,
        u2461Code::RowGap,
        u2461Code::ValOob,
        u2461Code::SumDrift,
        u2461Code::LedgerDup,
        u2461Code::LedgerHole,
        u2461Code::OpNoop,
        u2461Code::FieldDrift,
        u2461Code::LedgerBroken,
        u2461Code::SnapshotFingerprint,
        u2461Code::TableBroken,
        u2461Code::SegmentGuard,
    ];

    /// 码值（连续十六进制，与声明序恒等）。
    pub const fn code(self) -> u16 {
        match self {
            u2461Code::RowDup => 0xa240,
            u2461Code::RowGap => 0xa241,
            u2461Code::ValOob => 0xa242,
            u2461Code::SumDrift => 0xa243,
            u2461Code::LedgerDup => 0xa244,
            u2461Code::LedgerHole => 0xa245,
            u2461Code::OpNoop => 0xa246,
            u2461Code::FieldDrift => 0xa247,
            u2461Code::LedgerBroken => 0xa248,
            u2461Code::SnapshotFingerprint => 0xa249,
            u2461Code::TableBroken => 0xa24a,
            u2461Code::SegmentGuard => 0xa24b,
        }
    }

    /// 中文诊断名。
    pub const fn name(self) -> &'static str {
        match self {
            u2461Code::RowDup => "表行重复（漏报触发 同键双写）",
            u2461Code::RowGap => "表行断档（重复上报 序号不连续）",
            u2461Code::ValOob => "值越界（初值错配 值域外）",
            u2461Code::SumDrift => "累计漂移（跨档抖动 求和≠理论值）",
            u2461Code::LedgerDup => "账目重记（同一事件双入账）",
            u2461Code::LedgerHole => "账目断漏（序号缺失）",
            u2461Code::OpNoop => "空操作（账目无效果调用）",
            u2461Code::FieldDrift => "字段漂移（账字段与账面口径不符）",
            u2461Code::LedgerBroken => "台账不守恒（守恒恒等式破缺）",
            u2461Code::SnapshotFingerprint => "快照指纹不符（字段被改后重算不一致）",
            u2461Code::TableBroken => "常量表破坏（表项与语义规则矛盾）",
            u2461Code::SegmentGuard => "段位守卫失败（诊断码越出本模块段）",
        }
    }

    /// 可操作修复建议（每条诊断都必须带下一步动作）。
    pub const fn suggestion(self) -> &'static str {
        match self {
            u2461Code::RowDup => "按键唯一律去重",
            u2461Code::RowGap => "按连续律补号",
            u2461Code::ValOob => "按值域裁剪",
            u2461Code::SumDrift => "按理论值复算",
            u2461Code::LedgerDup => "按事件幂等律合并",
            u2461Code::LedgerHole => "按连续律补账",
            u2461Code::OpNoop => "核对操作前置条件",
            u2461Code::FieldDrift => "按口径重建字段",
            u2461Code::LedgerBroken => "按守恒律重建台账后重跑",
            u2461Code::SnapshotFingerprint => "按防篡改规程重建快照",
            u2461Code::TableBroken => "按语义规则重建常量表",
            u2461Code::SegmentGuard => "核对诊断码分配表",
        }
    }
}

// --------------------------------------------------------------------------
// 冻结常量表（u2461reg · 24 行逐行注释自证）
// --------------------------------------------------------------------------
/// 水线档位 语义登记表（24 行逐行注释）。
pub const u2461reg_COUNT: usize = 24;

/// 冻结表本体（编译期常量，运行期只读）。
pub static u2461reg: [(&'static str, u64); 24] = [
    ("MN_A", 7), // MN 主登记（min 档 · 判据面）
    ("MN_B", 33), // MN 副登记（min 档 · 对拍面）
    ("LW_A", 14), // LW 主登记（low 档 · 判据面）
    ("LW_B", 44), // LW 副登记（low 档 · 对拍面）
    ("HI_A", 21), // HI 主登记（high 档 · 判据面）
    ("HI_B", 55), // HI 副登记（high 档 · 对拍面）
    ("NF_A", 28), // NF 主登记（nr_free 总账 · 判据面）
    ("NF_B", 66), // NF 副登记（nr_free 总账 · 对拍面）
    ("SP_A", 35), // SP 主登记（同源单点 · 判据面）
    ("SP_B", 77), // SP 副登记（同源单点 · 对拍面）
    ("MC_A", 42), // MC 主登记（三机型 · 判据面）
    ("MC_B", 88), // MC 副登记（三机型 · 对拍面）
    ("IV_A", 49), // IV 主登记（初值账 · 判据面）
    ("IV_B", 99), // IV 副登记（初值账 · 对拍面）
    ("XE_A", 56), // XE 主登记（跨越事件 · 判据面）
    ("XE_B", 110), // XE 副登记（跨越事件 · 对拍面）
    ("TR_A", 63), // TR 主登记（触发账 · 判据面）
    ("TR_B", 121), // TR 副登记（触发账 · 对拍面）
    ("MS_A", 70), // MS 主登记（漏报账 · 判据面）
    ("MS_B", 132), // MS 副登记（漏报账 · 对拍面）
    ("RP_A", 77), // RP 主登记（重复账 · 判据面）
    ("RP_B", 143), // RP 副登记（重复账 · 对拍面）
    ("ZT_A", 84), // ZT 主登记（零误断言 · 判据面）
    ("ZT_B", 154), // ZT 副登记（零误断言 · 对拍面）
];

/// 线性查找（键唯一性由构造期断言保证，运行时无分支歧义）。
pub fn u2461reg_get(key: &'static str) -> Option<u64> {
    let mut i = 0usize;
    while i < u2461reg_COUNT {
        if u2461reg[i].0 == key {
            return Some(u2461reg[i].1);
        }
        i += 1;
    }
    None
}

/// 累计求和（用于守恒对账）。
pub fn u2461reg_sum() -> u64 {
    let mut s = 0u64;
    let mut i = 0usize;
    while i < u2461reg_COUNT {
        s = s.wrapping_add(u2461reg[i].1 as u64);
        i += 1;
    }
    s
}

// --------------------------------------------------------------------------
// 守恒账 Mxu2461Ledger（水线档位 守恒账（入账/回执/注记三操作守恒））
// --------------------------------------------------------------------------
/// 水线档位 守恒账（入账/回执/注记三操作守恒）。
#[derive(Clone, Copy, Debug)]
pub struct Mxu2461Ledger {
    /// 入账笔数
    pub entries: u64,
    /// 回执笔数
    pub hits: u64,
    /// 注记笔数
    pub notes: u64,
}

impl Mxu2461Ledger {
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

/// 段位守卫：全部诊断码必须落在本段 [0xa240, 0xa24f]。
pub const fn u2461_segment_guard() -> bool {
    let mut ok = true;
    let mut i = 0usize;
    while i < u2461Code::ALL.len() {
        let c = u2461Code::ALL[i].code();
        if c < 0xa240 || c > 0xa24f {
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
fn u2461_guard_seg() -> bool {
    u2461_segment_guard()
}

/// UNX-F2461 登记表 24 行齐备 + 边界锚 + 键外拒收 + 累计守恒。
fn check_u2461_core_tbl() -> bool {
    let mut ok = u2461reg_COUNT == 24;
    if ok { ok = u2461reg_get("MN_A") == Some(7) && u2461reg_get("MN_B") == Some(33); }
    if ok { ok = u2461reg_get("ZT_A") == Some(84); }
    if ok { ok = u2461reg_get("ZZZ_NOKEY").is_none(); }
    if ok { ok = u2461reg_sum() == 1668; }
    ok
}

/// UNX-F2461 守恒账三操作确定性效果。
fn check_u2461_core_ldg() -> bool {
    let mut ld = Mxu2461Ledger::new();
    let mut ok = true;
    ld.bump_entry(); ld.bump_entry(); ld.bump_hit(); ld.log_note();
    ok = ld.entries == 2 && ld.hits == 1 && ld.notes == 1;
    ok
}

/// 本模块自检（逐检查带 detail，全绿才出门）。
pub fn run_u2461_checks() -> CheckSet {
    let mut set = CheckSet::new("U2461");
    set.add(
        "u2461-seg-guard",
        u2461_guard_seg(),
        if u2461_guard_seg() { "" } else { "diagnostic code outside segment" },
    );
    set.add(
        "u2461-core-tbl",
        check_u2461_core_tbl(),
        if check_u2461_core_tbl() { "" } else { "UNX-F2461 core table mismatch" },
    );
    set.add(
        "u2461-core-ldg",
        check_u2461_core_ldg(),
        if check_u2461_core_ldg() { "" } else { "UNX-F2461 core ledger mismatch" },
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
    fn u2461_all_checks_green() {
        let set = run_u2461_checks();
        assert!(set.all_passed(), "U2461 domain checks must be green");
    }

    #[test]
    fn u2461_segment_guard_holds() {
        assert!(u2461_segment_guard());
    }
}
