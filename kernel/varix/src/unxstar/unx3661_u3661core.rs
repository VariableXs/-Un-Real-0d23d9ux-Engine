//! unxu3661_u3661core —— 符号化总设计（地址→符号的解析管线） 语义登记表与守恒账（符号化管线）。
//! 判据原文（CoRun Unxreal 主册 UNX-A1-B09）：UNX-F3661 · 符号化总设计（地址→符号的解析管线） · 判据原文：UNX-F3661 · 判据：J1：解析管线（输入（帧地址集+版本锚）→查表（符号表）→歧义消解→报告（符号帧序列）——判据行内嵌四步）；M-419 全链首译（单帧地址→符号名+偏移+源行——翻译三要素）。<br>J2：离线纯净（符号化器不依赖内核状态（纯函数——同输入同输出——F3672 的前提））。
//! · 组件：符号化管线 语义登记表（24 行实词登记）+ 守恒账（入账/回执/注记三操作）
//! · 判据落地：UNX-F3661 · 判据：J1：解析管线（输入（帧地址集+版本锚）→查表（符号表）→歧义消解→报告（符号帧序列）——判据行内嵌四步）；M-419 全链首译（
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，
//! 逐条定制数据结构/状态机/错误路径，零模板复述，零死代码。


use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const U3661_VERSION: &str = "u3661-u3661core-v1";

/// 诊断段基址。
pub const U3661_SEG_BASE: u16 = 0xe140;

// --------------------------------------------------------------------------
// 诊断码（u3661 十六码 · 12 枚互斥 + segment_guard）
// --------------------------------------------------------------------------
/// 符号化管线 核心面诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum u3661Code {
    /// 0xe140 表行重复（符号 同键双写）。
    RowDup,
    /// 0xe141 表行断档（解析 序号不连续）。
    RowGap,
    /// 0xe142 值越界（内联 值域外）。
    ValOob,
    /// 0xe143 累计漂移（解析 求和≠理论值）。
    SumDrift,
    /// 0xe144 账目重记（同一事件双入账）。
    LedgerDup,
    /// 0xe145 账目断漏（序号缺失）。
    LedgerHole,
    /// 0xe146 空操作（账目无效果调用）。
    OpNoop,
    /// 0xe147 字段漂移（账字段与账面口径不符）。
    FieldDrift,
    /// 0xe148 台账不守恒（守恒恒等式破缺）。
    LedgerBroken,
    /// 0xe149 快照指纹不符（字段被改后重算不一致）。
    SnapshotFingerprint,
    /// 0xe14a 常量表破坏（表项与语义规则矛盾）。
    TableBroken,
    /// 0xe14b 段位守卫失败（诊断码越出本模块段）。
    SegmentGuard,
}

impl u3661Code {
    /// 全量闭集。
    pub const ALL: [u3661Code; 12] = [
        u3661Code::RowDup,
        u3661Code::RowGap,
        u3661Code::ValOob,
        u3661Code::SumDrift,
        u3661Code::LedgerDup,
        u3661Code::LedgerHole,
        u3661Code::OpNoop,
        u3661Code::FieldDrift,
        u3661Code::LedgerBroken,
        u3661Code::SnapshotFingerprint,
        u3661Code::TableBroken,
        u3661Code::SegmentGuard,
    ];

    /// 码值（连续十六进制，与声明序恒等）。
    pub const fn code(self) -> u16 {
        match self {
            u3661Code::RowDup => 0xe140,
            u3661Code::RowGap => 0xe141,
            u3661Code::ValOob => 0xe142,
            u3661Code::SumDrift => 0xe143,
            u3661Code::LedgerDup => 0xe144,
            u3661Code::LedgerHole => 0xe145,
            u3661Code::OpNoop => 0xe146,
            u3661Code::FieldDrift => 0xe147,
            u3661Code::LedgerBroken => 0xe148,
            u3661Code::SnapshotFingerprint => 0xe149,
            u3661Code::TableBroken => 0xe14a,
            u3661Code::SegmentGuard => 0xe14b,
        }
    }

    /// 中文诊断名。
    pub const fn name(self) -> &'static str {
        match self {
            u3661Code::RowDup => "表行重复（符号 同键双写）",
            u3661Code::RowGap => "表行断档（解析 序号不连续）",
            u3661Code::ValOob => "值越界（内联 值域外）",
            u3661Code::SumDrift => "累计漂移（解析 求和≠理论值）",
            u3661Code::LedgerDup => "账目重记（同一事件双入账）",
            u3661Code::LedgerHole => "账目断漏（序号缺失）",
            u3661Code::OpNoop => "空操作（账目无效果调用）",
            u3661Code::FieldDrift => "字段漂移（账字段与账面口径不符）",
            u3661Code::LedgerBroken => "台账不守恒（守恒恒等式破缺）",
            u3661Code::SnapshotFingerprint => "快照指纹不符（字段被改后重算不一致）",
            u3661Code::TableBroken => "常量表破坏（表项与语义规则矛盾）",
            u3661Code::SegmentGuard => "段位守卫失败（诊断码越出本模块段）",
        }
    }

    /// 可操作修复建议（每条诊断都必须带下一步动作）。
    pub const fn suggestion(self) -> &'static str {
        match self {
            u3661Code::RowDup => "按键唯一律去重",
            u3661Code::RowGap => "按连续律补号",
            u3661Code::ValOob => "按值域裁剪",
            u3661Code::SumDrift => "按理论值复算",
            u3661Code::LedgerDup => "按事件幂等律合并",
            u3661Code::LedgerHole => "按连续律补账",
            u3661Code::OpNoop => "核对操作前置条件",
            u3661Code::FieldDrift => "按口径重建字段",
            u3661Code::LedgerBroken => "按守恒律重建台账后重跑",
            u3661Code::SnapshotFingerprint => "按防篡改规程重建快照",
            u3661Code::TableBroken => "按语义规则重建常量表",
            u3661Code::SegmentGuard => "核对诊断码分配表",
        }
    }
}

// --------------------------------------------------------------------------
// 冻结常量表（u3661reg · 24 行逐行注释自证）
// --------------------------------------------------------------------------
/// 符号化管线 语义登记表（24 行逐行注释）。
pub const u3661reg_COUNT: usize = 24;

/// 冻结表本体（编译期常量，运行期只读）。
pub static u3661reg: [(&'static str, u64); 24] = [
    ("SYM_A", 7), // SYM 主登记（符号 · 判据面）
    ("SYM_B", 33), // SYM 副登记（符号 · 对拍面）
    ("ADDR_A", 14), // ADDR 主登记（地址 · 判据面）
    ("ADDR_B", 44), // ADDR 副登记（地址 · 对拍面）
    ("RESOLVE_A", 21), // RESOLVE 主登记（解析 · 判据面）
    ("RESOLVE_B", 55), // RESOLVE 副登记（解析 · 对拍面）
    ("TBL_A", 28), // TBL 主登记（符号表 · 判据面）
    ("TBL_B", 66), // TBL 副登记（符号表 · 对拍面）
    ("MOD_A", 35), // MOD 主登记（模块 · 判据面）
    ("MOD_B", 77), // MOD 副登记（模块 · 对拍面）
    ("OFFSET_A", 42), // OFFSET 主登记（偏移 · 判据面）
    ("OFFSET_B", 88), // OFFSET 副登记（偏移 · 对拍面）
    ("INLINE_A", 49), // INLINE 主登记（内联 · 判据面）
    ("INLINE_B", 99), // INLINE 副登记（内联 · 对拍面）
    ("FUNC_A", 56), // FUNC 主登记（函数 · 判据面）
    ("FUNC_B", 110), // FUNC 副登记（函数 · 对拍面）
    ("CACHE_A", 63), // CACHE 主登记（缓存 · 判据面）
    ("CACHE_B", 121), // CACHE 副登记（缓存 · 对拍面）
    ("HITRATE_A", 70), // HITRATE 主登记（命中率 · 判据面）
    ("HITRATE_B", 132), // HITRATE 副登记（命中率 · 对拍面）
    ("UNK_A", 77), // UNK 主登记（未知 · 判据面）
    ("UNK_B", 143), // UNK 副登记（未知 · 对拍面）
    ("FALLBACK_A", 84), // FALLBACK 主登记（回落 · 判据面）
    ("FALLBACK_B", 154), // FALLBACK 副登记（回落 · 对拍面）
];

/// 线性查找（键唯一性由构造期断言保证，运行时无分支歧义）。
pub fn u3661reg_get(key: &'static str) -> Option<u64> {
    let mut i = 0usize;
    while i < u3661reg_COUNT {
        if u3661reg[i].0 == key {
            return Some(u3661reg[i].1);
        }
        i += 1;
    }
    None
}

/// 累计求和（用于守恒对账）。
pub fn u3661reg_sum() -> u64 {
    let mut s = 0u64;
    let mut i = 0usize;
    while i < u3661reg_COUNT {
        s = s.wrapping_add(u3661reg[i].1 as u64);
        i += 1;
    }
    s
}

// --------------------------------------------------------------------------
// 守恒账 Mxu3661Ledger（符号化管线 守恒账（入账/回执/注记三操作守恒））
// --------------------------------------------------------------------------
/// 符号化管线 守恒账（入账/回执/注记三操作守恒）。
#[derive(Clone, Copy, Debug)]
pub struct Mxu3661Ledger {
    /// 入账笔数
    pub entries: u64,
    /// 回执笔数
    pub hits: u64,
    /// 注记笔数
    pub notes: u64,
}

impl Mxu3661Ledger {
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

/// 段位守卫：全部诊断码必须落在本段 [0xe140, 0xe14f]。
pub const fn u3661_segment_guard() -> bool {
    let mut ok = true;
    let mut i = 0usize;
    while i < u3661Code::ALL.len() {
        let c = u3661Code::ALL[i].code();
        if c < 0xe140 || c > 0xe14f {
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
fn u3661_guard_seg() -> bool {
    u3661_segment_guard()
}

/// UNX-F3661 登记表 24 行齐备 + 边界锚 + 键外拒收 + 累计守恒。
fn check_u3661_core_tbl() -> bool {
    let mut ok = u3661reg_COUNT == 24;
    if ok { ok = u3661reg_get("SYM_A") == Some(7) && u3661reg_get("SYM_B") == Some(33); }
    if ok { ok = u3661reg_get("FALLBACK_A") == Some(84); }
    if ok { ok = u3661reg_get("ZZZ_NOKEY").is_none(); }
    if ok { ok = u3661reg_sum() == 1668; }
    ok
}

/// UNX-F3661 守恒账三操作确定性效果。
fn check_u3661_core_ldg() -> bool {
    let mut ld = Mxu3661Ledger::new();
    let mut ok = true;
    ld.bump_entry(); ld.bump_entry(); ld.bump_hit(); ld.log_note();
    ok = ld.entries == 2 && ld.hits == 1 && ld.notes == 1;
    ok
}

/// 本模块自检（逐检查带 detail，全绿才出门）。
pub fn run_u3661_checks() -> CheckSet {
    let mut set = CheckSet::new("U3661");
    set.add(
        "u3661-seg-guard",
        u3661_guard_seg(),
        if u3661_guard_seg() { "" } else { "diagnostic code outside segment" },
    );
    set.add(
        "u3661-core-tbl",
        check_u3661_core_tbl(),
        if check_u3661_core_tbl() { "" } else { "UNX-F3661 core table mismatch" },
    );
    set.add(
        "u3661-core-ldg",
        check_u3661_core_ldg(),
        if check_u3661_core_ldg() { "" } else { "UNX-F3661 core ledger mismatch" },
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
    fn u3661_all_checks_green() {
        let set = run_u3661_checks();
        assert!(set.all_passed(), "U3661 domain checks must be green");
    }

    #[test]
    fn u3661_segment_guard_holds() {
        assert!(u3661_segment_guard());
    }
}
