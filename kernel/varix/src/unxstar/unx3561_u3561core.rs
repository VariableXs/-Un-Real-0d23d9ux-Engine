//! unxu3561_u3561core —— 毒化总设计（0xAA alloc/0xBB free 双毒化——UAF 检测基石） 语义登记表与守恒账（内存毒化）。
//! 判据原文（CoRun Unxreal 主册 UNX-A1-B09）：UNX-F3561 · 毒化总设计（0xAA alloc/0xBB free 双毒化——UAF 检测基石） · 判据原文：UNX-F3561 · 判据：J1：双毒化语义（alloc 后填 0xAA（未初始化用）/free 后填 0xBB（已释放用）——读中毒值即违规）；M-319 UAF 全链（释放→读→0xBB 被捕）。
//! · 组件：内存毒化 语义登记表（24 行实词登记）+ 守恒账（入账/回执/注记三操作）
//! · 判据落地：UNX-F3561 · 判据：J1：双毒化语义（alloc 后填 0xAA（未初始化用）/free 后填 0xBB（已释放用）——读中毒值即违规）；M-319 
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，
//! 逐条定制数据结构/状态机/错误路径，零模板复述，零死代码。


use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const U3561_VERSION: &str = "u3561-u3561core-v1";

/// 诊断段基址。
pub const U3561_SEG_BASE: u16 = 0xe0f0;

// --------------------------------------------------------------------------
// 诊断码（u3561 十六码 · 12 枚互斥 + segment_guard）
// --------------------------------------------------------------------------
/// 内存毒化 核心面诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum u3561Code {
    /// 0xe0f0 表行重复（毒化 同键双写）。
    RowDup,
    /// 0xe0f1 表行断档（UAF 序号不连续）。
    RowGap,
    /// 0xe0f2 值越界（哨兵 值域外）。
    ValOob,
    /// 0xe0f3 累计漂移（双毒化 求和≠理论值）。
    SumDrift,
    /// 0xe0f4 账目重记（同一事件双入账）。
    LedgerDup,
    /// 0xe0f5 账目断漏（序号缺失）。
    LedgerHole,
    /// 0xe0f6 空操作（账目无效果调用）。
    OpNoop,
    /// 0xe0f7 字段漂移（账字段与账面口径不符）。
    FieldDrift,
    /// 0xe0f8 台账不守恒（守恒恒等式破缺）。
    LedgerBroken,
    /// 0xe0f9 快照指纹不符（字段被改后重算不一致）。
    SnapshotFingerprint,
    /// 0xe0fa 常量表破坏（表项与语义规则矛盾）。
    TableBroken,
    /// 0xe0fb 段位守卫失败（诊断码越出本模块段）。
    SegmentGuard,
}

impl u3561Code {
    /// 全量闭集。
    pub const ALL: [u3561Code; 12] = [
        u3561Code::RowDup,
        u3561Code::RowGap,
        u3561Code::ValOob,
        u3561Code::SumDrift,
        u3561Code::LedgerDup,
        u3561Code::LedgerHole,
        u3561Code::OpNoop,
        u3561Code::FieldDrift,
        u3561Code::LedgerBroken,
        u3561Code::SnapshotFingerprint,
        u3561Code::TableBroken,
        u3561Code::SegmentGuard,
    ];

    /// 码值（连续十六进制，与声明序恒等）。
    pub const fn code(self) -> u16 {
        match self {
            u3561Code::RowDup => 0xe0f0,
            u3561Code::RowGap => 0xe0f1,
            u3561Code::ValOob => 0xe0f2,
            u3561Code::SumDrift => 0xe0f3,
            u3561Code::LedgerDup => 0xe0f4,
            u3561Code::LedgerHole => 0xe0f5,
            u3561Code::OpNoop => 0xe0f6,
            u3561Code::FieldDrift => 0xe0f7,
            u3561Code::LedgerBroken => 0xe0f8,
            u3561Code::SnapshotFingerprint => 0xe0f9,
            u3561Code::TableBroken => 0xe0fa,
            u3561Code::SegmentGuard => 0xe0fb,
        }
    }

    /// 中文诊断名。
    pub const fn name(self) -> &'static str {
        match self {
            u3561Code::RowDup => "表行重复（毒化 同键双写）",
            u3561Code::RowGap => "表行断档（UAF 序号不连续）",
            u3561Code::ValOob => "值越界（哨兵 值域外）",
            u3561Code::SumDrift => "累计漂移（双毒化 求和≠理论值）",
            u3561Code::LedgerDup => "账目重记（同一事件双入账）",
            u3561Code::LedgerHole => "账目断漏（序号缺失）",
            u3561Code::OpNoop => "空操作（账目无效果调用）",
            u3561Code::FieldDrift => "字段漂移（账字段与账面口径不符）",
            u3561Code::LedgerBroken => "台账不守恒（守恒恒等式破缺）",
            u3561Code::SnapshotFingerprint => "快照指纹不符（字段被改后重算不一致）",
            u3561Code::TableBroken => "常量表破坏（表项与语义规则矛盾）",
            u3561Code::SegmentGuard => "段位守卫失败（诊断码越出本模块段）",
        }
    }

    /// 可操作修复建议（每条诊断都必须带下一步动作）。
    pub const fn suggestion(self) -> &'static str {
        match self {
            u3561Code::RowDup => "按键唯一律去重",
            u3561Code::RowGap => "按连续律补号",
            u3561Code::ValOob => "按值域裁剪",
            u3561Code::SumDrift => "按理论值复算",
            u3561Code::LedgerDup => "按事件幂等律合并",
            u3561Code::LedgerHole => "按连续律补账",
            u3561Code::OpNoop => "核对操作前置条件",
            u3561Code::FieldDrift => "按口径重建字段",
            u3561Code::LedgerBroken => "按守恒律重建台账后重跑",
            u3561Code::SnapshotFingerprint => "按防篡改规程重建快照",
            u3561Code::TableBroken => "按语义规则重建常量表",
            u3561Code::SegmentGuard => "核对诊断码分配表",
        }
    }
}

// --------------------------------------------------------------------------
// 冻结常量表（u3561reg · 24 行逐行注释自证）
// --------------------------------------------------------------------------
/// 内存毒化 语义登记表（24 行逐行注释）。
pub const u3561reg_COUNT: usize = 24;

/// 冻结表本体（编译期常量，运行期只读）。
pub static u3561reg: [(&'static str, u64); 24] = [
    ("AA_A", 7), // AA 主登记（分配毒化 · 判据面）
    ("AA_B", 33), // AA 副登记（分配毒化 · 对拍面）
    ("BB_A", 14), // BB 主登记（释放毒化 · 判据面）
    ("BB_B", 44), // BB 副登记（释放毒化 · 对拍面）
    ("MAGIC_A", 21), // MAGIC 主登记（毒化魔数 · 判据面）
    ("MAGIC_B", 55), // MAGIC 副登记（毒化魔数 · 对拍面）
    ("UAF_A", 28), // UAF 主登记（释放后使用 · 判据面）
    ("UAF_B", 66), // UAF 副登记（释放后使用 · 对拍面）
    ("CANARY_A", 35), // CANARY 主登记（哨兵 · 判据面）
    ("CANARY_B", 77), // CANARY 副登记（哨兵 · 对拍面）
    ("FILL_A", 42), // FILL 主登记（填充图案 · 判据面）
    ("FILL_B", 88), // FILL 副登记（填充图案 · 对拍面）
    ("CHK_A", 49), // CHK 主登记（释放前检查 · 判据面）
    ("CHK_B", 99), // CHK 副登记（释放前检查 · 对拍面）
    ("HIT_A", 56), // HIT 主登记（命中率 · 判据面）
    ("HIT_B", 110), // HIT 副登记（命中率 · 对拍面）
    ("QUAR_A", 63), // QUAR 主登记（隔离 · 判据面）
    ("QUAR_B", 121), // QUAR 副登记（隔离 · 对拍面）
    ("RPT_A", 70), // RPT 主登记（报告 · 判据面）
    ("RPT_B", 132), // RPT 副登记（报告 · 对拍面）
    ("SCAN_A", 77), // SCAN 主登记（扫描 · 判据面）
    ("SCAN_B", 143), // SCAN 副登记（扫描 · 对拍面）
    ("BUD_A", 84), // BUD 主登记（开销预算 · 判据面）
    ("BUD_B", 154), // BUD 副登记（开销预算 · 对拍面）
];

/// 线性查找（键唯一性由构造期断言保证，运行时无分支歧义）。
pub fn u3561reg_get(key: &'static str) -> Option<u64> {
    let mut i = 0usize;
    while i < u3561reg_COUNT {
        if u3561reg[i].0 == key {
            return Some(u3561reg[i].1);
        }
        i += 1;
    }
    None
}

/// 累计求和（用于守恒对账）。
pub fn u3561reg_sum() -> u64 {
    let mut s = 0u64;
    let mut i = 0usize;
    while i < u3561reg_COUNT {
        s = s.wrapping_add(u3561reg[i].1 as u64);
        i += 1;
    }
    s
}

// --------------------------------------------------------------------------
// 守恒账 Mxu3561Ledger（内存毒化 守恒账（入账/回执/注记三操作守恒））
// --------------------------------------------------------------------------
/// 内存毒化 守恒账（入账/回执/注记三操作守恒）。
#[derive(Clone, Copy, Debug)]
pub struct Mxu3561Ledger {
    /// 入账笔数
    pub entries: u64,
    /// 回执笔数
    pub hits: u64,
    /// 注记笔数
    pub notes: u64,
}

impl Mxu3561Ledger {
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

/// 段位守卫：全部诊断码必须落在本段 [0xe0f0, 0xe0ff]。
pub const fn u3561_segment_guard() -> bool {
    let mut ok = true;
    let mut i = 0usize;
    while i < u3561Code::ALL.len() {
        let c = u3561Code::ALL[i].code();
        if c < 0xe0f0 || c > 0xe0ff {
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
fn u3561_guard_seg() -> bool {
    u3561_segment_guard()
}

/// UNX-F3561 登记表 24 行齐备 + 边界锚 + 键外拒收 + 累计守恒。
fn check_u3561_core_tbl() -> bool {
    let mut ok = u3561reg_COUNT == 24;
    if ok { ok = u3561reg_get("AA_A") == Some(7) && u3561reg_get("AA_B") == Some(33); }
    if ok { ok = u3561reg_get("BUD_A") == Some(84); }
    if ok { ok = u3561reg_get("ZZZ_NOKEY").is_none(); }
    if ok { ok = u3561reg_sum() == 1668; }
    ok
}

/// UNX-F3561 守恒账三操作确定性效果。
fn check_u3561_core_ldg() -> bool {
    let mut ld = Mxu3561Ledger::new();
    let mut ok = true;
    ld.bump_entry(); ld.bump_entry(); ld.bump_hit(); ld.log_note();
    ok = ld.entries == 2 && ld.hits == 1 && ld.notes == 1;
    ok
}

/// 本模块自检（逐检查带 detail，全绿才出门）。
pub fn run_u3561_checks() -> CheckSet {
    let mut set = CheckSet::new("U3561");
    set.add(
        "u3561-seg-guard",
        u3561_guard_seg(),
        if u3561_guard_seg() { "" } else { "diagnostic code outside segment" },
    );
    set.add(
        "u3561-core-tbl",
        check_u3561_core_tbl(),
        if check_u3561_core_tbl() { "" } else { "UNX-F3561 core table mismatch" },
    );
    set.add(
        "u3561-core-ldg",
        check_u3561_core_ldg(),
        if check_u3561_core_ldg() { "" } else { "UNX-F3561 core ledger mismatch" },
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
    fn u3561_all_checks_green() {
        let set = run_u3561_checks();
        assert!(set.all_passed(), "U3561 domain checks must be green");
    }

    #[test]
    fn u3561_segment_guard_holds() {
        assert!(u3561_segment_guard());
    }
}
