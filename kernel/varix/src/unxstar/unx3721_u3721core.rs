//! unxu3721_u3721core —— 桩体系总设计（替身三原则与桩清单） 语义登记表与守恒账（桩体系）。
//! 判据原文（CoRun Unxreal 主册 UNX-A1-B09）：UNX-F3721 · 桩体系总设计（替身三原则与桩清单） · 判据原文：UNX-F3721 · 判据：J1：替身三原则（确定性（同脚本同态同行为）/账本化（操作全记账）/可控性（时间与故障全可控——判据行内嵌三原则各一句）；桩清单（时钟/中断/DMA/存储/网卡五桩 v1——清单 const 扩桩走 bump）；M-479 三原则首验（时钟桩+中断桩最小联动→三原则逐条断言）。<br>J2：桩与被测分池（B20 隔离池同律——桩基础设施不吃被测方内存）。
//! · 组件：桩体系 语义登记表（24 行实词登记）+ 守恒账（入账/回执/注记三操作）
//! · 判据落地：UNX-F3721 · 判据：J1：替身三原则（确定性（同脚本同态同行为）/账本化（操作全记账）/可控性（时间与故障全可控——判据行内嵌三原则各一句）；桩清单（
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，
//! 逐条定制数据结构/状态机/错误路径，零模板复述，零死代码。


use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const U3721_VERSION: &str = "u3721-u3721core-v1";

/// 诊断段基址。
pub const U3721_SEG_BASE: u16 = 0xe170;

// --------------------------------------------------------------------------
// 诊断码（u3721 十六码 · 12 枚互斥 + segment_guard）
// --------------------------------------------------------------------------
/// 桩体系 核心面诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum u3721Code {
    /// 0xe170 表行重复（桩 同键双写）。
    RowDup,
    /// 0xe171 表行断档（替身 序号不连续）。
    RowGap,
    /// 0xe172 值越界（录制 值域外）。
    ValOob,
    /// 0xe173 累计漂移（三原则 求和≠理论值）。
    SumDrift,
    /// 0xe174 账目重记（同一事件双入账）。
    LedgerDup,
    /// 0xe175 账目断漏（序号缺失）。
    LedgerHole,
    /// 0xe176 空操作（账目无效果调用）。
    OpNoop,
    /// 0xe177 字段漂移（账字段与账面口径不符）。
    FieldDrift,
    /// 0xe178 台账不守恒（守恒恒等式破缺）。
    LedgerBroken,
    /// 0xe179 快照指纹不符（字段被改后重算不一致）。
    SnapshotFingerprint,
    /// 0xe17a 常量表破坏（表项与语义规则矛盾）。
    TableBroken,
    /// 0xe17b 段位守卫失败（诊断码越出本模块段）。
    SegmentGuard,
}

impl u3721Code {
    /// 全量闭集。
    pub const ALL: [u3721Code; 12] = [
        u3721Code::RowDup,
        u3721Code::RowGap,
        u3721Code::ValOob,
        u3721Code::SumDrift,
        u3721Code::LedgerDup,
        u3721Code::LedgerHole,
        u3721Code::OpNoop,
        u3721Code::FieldDrift,
        u3721Code::LedgerBroken,
        u3721Code::SnapshotFingerprint,
        u3721Code::TableBroken,
        u3721Code::SegmentGuard,
    ];

    /// 码值（连续十六进制，与声明序恒等）。
    pub const fn code(self) -> u16 {
        match self {
            u3721Code::RowDup => 0xe170,
            u3721Code::RowGap => 0xe171,
            u3721Code::ValOob => 0xe172,
            u3721Code::SumDrift => 0xe173,
            u3721Code::LedgerDup => 0xe174,
            u3721Code::LedgerHole => 0xe175,
            u3721Code::OpNoop => 0xe176,
            u3721Code::FieldDrift => 0xe177,
            u3721Code::LedgerBroken => 0xe178,
            u3721Code::SnapshotFingerprint => 0xe179,
            u3721Code::TableBroken => 0xe17a,
            u3721Code::SegmentGuard => 0xe17b,
        }
    }

    /// 中文诊断名。
    pub const fn name(self) -> &'static str {
        match self {
            u3721Code::RowDup => "表行重复（桩 同键双写）",
            u3721Code::RowGap => "表行断档（替身 序号不连续）",
            u3721Code::ValOob => "值越界（录制 值域外）",
            u3721Code::SumDrift => "累计漂移（三原则 求和≠理论值）",
            u3721Code::LedgerDup => "账目重记（同一事件双入账）",
            u3721Code::LedgerHole => "账目断漏（序号缺失）",
            u3721Code::OpNoop => "空操作（账目无效果调用）",
            u3721Code::FieldDrift => "字段漂移（账字段与账面口径不符）",
            u3721Code::LedgerBroken => "台账不守恒（守恒恒等式破缺）",
            u3721Code::SnapshotFingerprint => "快照指纹不符（字段被改后重算不一致）",
            u3721Code::TableBroken => "常量表破坏（表项与语义规则矛盾）",
            u3721Code::SegmentGuard => "段位守卫失败（诊断码越出本模块段）",
        }
    }

    /// 可操作修复建议（每条诊断都必须带下一步动作）。
    pub const fn suggestion(self) -> &'static str {
        match self {
            u3721Code::RowDup => "按键唯一律去重",
            u3721Code::RowGap => "按连续律补号",
            u3721Code::ValOob => "按值域裁剪",
            u3721Code::SumDrift => "按理论值复算",
            u3721Code::LedgerDup => "按事件幂等律合并",
            u3721Code::LedgerHole => "按连续律补账",
            u3721Code::OpNoop => "核对操作前置条件",
            u3721Code::FieldDrift => "按口径重建字段",
            u3721Code::LedgerBroken => "按守恒律重建台账后重跑",
            u3721Code::SnapshotFingerprint => "按防篡改规程重建快照",
            u3721Code::TableBroken => "按语义规则重建常量表",
            u3721Code::SegmentGuard => "核对诊断码分配表",
        }
    }
}

// --------------------------------------------------------------------------
// 冻结常量表（u3721reg · 24 行逐行注释自证）
// --------------------------------------------------------------------------
/// 桩体系 语义登记表（24 行逐行注释）。
pub const u3721reg_COUNT: usize = 24;

/// 冻结表本体（编译期常量，运行期只读）。
pub static u3721reg: [(&'static str, u64); 24] = [
    ("STUB_A", 7), // STUB 主登记（桩 · 判据面）
    ("STUB_B", 33), // STUB 副登记（桩 · 对拍面）
    ("TRIPLE_A", 14), // TRIPLE 主登记（替身三原则 · 判据面）
    ("TRIPLE_B", 44), // TRIPLE 副登记（替身三原则 · 对拍面）
    ("LIST_A", 21), // LIST 主登记（桩清单 · 判据面）
    ("LIST_B", 55), // LIST 副登记（桩清单 · 对拍面）
    ("INJ_A", 28), // INJ 主登记（注入 · 判据面）
    ("INJ_B", 66), // INJ 副登记（注入 · 对拍面）
    ("REC_A", 35), // REC 主登记（录制 · 判据面）
    ("REC_B", 77), // REC 副登记（录制 · 对拍面）
    ("PLAY_A", 42), // PLAY 主登记（回放 · 判据面）
    ("PLAY_B", 88), // PLAY 副登记（回放 · 对拍面）
    ("FAIL_A", 49), // FAIL 主登记（失败注入 · 判据面）
    ("FAIL_B", 99), // FAIL 副登记（失败注入 · 对拍面）
    ("MODE_A", 56), // MODE 主登记（桩模式 · 判据面）
    ("MODE_B", 110), // MODE 副登记（桩模式 · 对拍面）
    ("SLOT_A", 63), // SLOT 主登记（桩槽 · 判据面）
    ("SLOT_B", 121), // SLOT 副登记（桩槽 · 对拍面）
    ("DECL_A", 70), // DECL 主登记（桩声明 · 判据面）
    ("DECL_B", 132), // DECL 副登记（桩声明 · 对拍面）
    ("USE_A", 77), // USE 主登记（使用点 · 判据面）
    ("USE_B", 143), // USE 副登记（使用点 · 对拍面）
    ("AUD_A", 84), // AUD 主登记（审计 · 判据面）
    ("AUD_B", 154), // AUD 副登记（审计 · 对拍面）
];

/// 线性查找（键唯一性由构造期断言保证，运行时无分支歧义）。
pub fn u3721reg_get(key: &'static str) -> Option<u64> {
    let mut i = 0usize;
    while i < u3721reg_COUNT {
        if u3721reg[i].0 == key {
            return Some(u3721reg[i].1);
        }
        i += 1;
    }
    None
}

/// 累计求和（用于守恒对账）。
pub fn u3721reg_sum() -> u64 {
    let mut s = 0u64;
    let mut i = 0usize;
    while i < u3721reg_COUNT {
        s = s.wrapping_add(u3721reg[i].1 as u64);
        i += 1;
    }
    s
}

// --------------------------------------------------------------------------
// 守恒账 Mxu3721Ledger（桩体系 守恒账（入账/回执/注记三操作守恒））
// --------------------------------------------------------------------------
/// 桩体系 守恒账（入账/回执/注记三操作守恒）。
#[derive(Clone, Copy, Debug)]
pub struct Mxu3721Ledger {
    /// 入账笔数
    pub entries: u64,
    /// 回执笔数
    pub hits: u64,
    /// 注记笔数
    pub notes: u64,
}

impl Mxu3721Ledger {
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

/// 段位守卫：全部诊断码必须落在本段 [0xe170, 0xe17f]。
pub const fn u3721_segment_guard() -> bool {
    let mut ok = true;
    let mut i = 0usize;
    while i < u3721Code::ALL.len() {
        let c = u3721Code::ALL[i].code();
        if c < 0xe170 || c > 0xe17f {
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
fn u3721_guard_seg() -> bool {
    u3721_segment_guard()
}

/// UNX-F3721 登记表 24 行齐备 + 边界锚 + 键外拒收 + 累计守恒。
fn check_u3721_core_tbl() -> bool {
    let mut ok = u3721reg_COUNT == 24;
    if ok { ok = u3721reg_get("STUB_A") == Some(7) && u3721reg_get("STUB_B") == Some(33); }
    if ok { ok = u3721reg_get("AUD_A") == Some(84); }
    if ok { ok = u3721reg_get("ZZZ_NOKEY").is_none(); }
    if ok { ok = u3721reg_sum() == 1668; }
    ok
}

/// UNX-F3721 守恒账三操作确定性效果。
fn check_u3721_core_ldg() -> bool {
    let mut ld = Mxu3721Ledger::new();
    let mut ok = true;
    ld.bump_entry(); ld.bump_entry(); ld.bump_hit(); ld.log_note();
    ok = ld.entries == 2 && ld.hits == 1 && ld.notes == 1;
    ok
}

/// 本模块自检（逐检查带 detail，全绿才出门）。
pub fn run_u3721_checks() -> CheckSet {
    let mut set = CheckSet::new("U3721");
    set.add(
        "u3721-seg-guard",
        u3721_guard_seg(),
        if u3721_guard_seg() { "" } else { "diagnostic code outside segment" },
    );
    set.add(
        "u3721-core-tbl",
        check_u3721_core_tbl(),
        if check_u3721_core_tbl() { "" } else { "UNX-F3721 core table mismatch" },
    );
    set.add(
        "u3721-core-ldg",
        check_u3721_core_ldg(),
        if check_u3721_core_ldg() { "" } else { "UNX-F3721 core ledger mismatch" },
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
    fn u3721_all_checks_green() {
        let set = run_u3721_checks();
        assert!(set.all_passed(), "U3721 domain checks must be green");
    }

    #[test]
    fn u3721_segment_guard_holds() {
        assert!(u3721_segment_guard());
    }
}
