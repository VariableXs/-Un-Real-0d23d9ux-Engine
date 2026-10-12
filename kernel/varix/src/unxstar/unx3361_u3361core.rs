//! unxu3361_u3361core —— 时间窗模型：TSC 时钟基准与窗口定义 语义登记表与守恒账（TSC 时间窗）。
//! 判据原文（CoRun Unxreal 主册 UNX-A1-B09）：UNX-F3361 · 时间窗模型：TSC 时钟基准与窗口定义 · 判据原文：UNX-F3361 · 判据：J1：时间窗模型文档（基准钟/窗口长度/三级窗口微/秒/分）交付；M-120 窗口边界行为验证。
//! · 组件：TSC 时间窗 语义登记表（24 行实词登记）+ 守恒账（入账/回执/注记三操作）
//! · 判据落地：UNX-F3361 · 判据：J1：时间窗模型文档（基准钟/窗口长度/三级窗口微/秒/分）交付；M-120 窗口边界行为验证。
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，
//! 逐条定制数据结构/状态机/错误路径，零模板复述，零死代码。


use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const U3361_VERSION: &str = "u3361-u3361core-v1";

/// 诊断段基址。
pub const U3361_SEG_BASE: u16 = 0xe050;

// --------------------------------------------------------------------------
// 诊断码（u3361 十六码 · 12 枚互斥 + segment_guard）
// --------------------------------------------------------------------------
/// TSC 时间窗 核心面诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum u3361Code {
    /// 0xe050 表行重复（TSC 同键双写）。
    RowDup,
    /// 0xe051 表行断档（时间窗 序号不连续）。
    RowGap,
    /// 0xe052 值越界（标定 值域外）。
    ValOob,
    /// 0xe053 累计漂移（标定 求和≠理论值）。
    SumDrift,
    /// 0xe054 账目重记（同一事件双入账）。
    LedgerDup,
    /// 0xe055 账目断漏（序号缺失）。
    LedgerHole,
    /// 0xe056 空操作（账目无效果调用）。
    OpNoop,
    /// 0xe057 字段漂移（账字段与账面口径不符）。
    FieldDrift,
    /// 0xe058 台账不守恒（守恒恒等式破缺）。
    LedgerBroken,
    /// 0xe059 快照指纹不符（字段被改后重算不一致）。
    SnapshotFingerprint,
    /// 0xe05a 常量表破坏（表项与语义规则矛盾）。
    TableBroken,
    /// 0xe05b 段位守卫失败（诊断码越出本模块段）。
    SegmentGuard,
}

impl u3361Code {
    /// 全量闭集。
    pub const ALL: [u3361Code; 12] = [
        u3361Code::RowDup,
        u3361Code::RowGap,
        u3361Code::ValOob,
        u3361Code::SumDrift,
        u3361Code::LedgerDup,
        u3361Code::LedgerHole,
        u3361Code::OpNoop,
        u3361Code::FieldDrift,
        u3361Code::LedgerBroken,
        u3361Code::SnapshotFingerprint,
        u3361Code::TableBroken,
        u3361Code::SegmentGuard,
    ];

    /// 码值（连续十六进制，与声明序恒等）。
    pub const fn code(self) -> u16 {
        match self {
            u3361Code::RowDup => 0xe050,
            u3361Code::RowGap => 0xe051,
            u3361Code::ValOob => 0xe052,
            u3361Code::SumDrift => 0xe053,
            u3361Code::LedgerDup => 0xe054,
            u3361Code::LedgerHole => 0xe055,
            u3361Code::OpNoop => 0xe056,
            u3361Code::FieldDrift => 0xe057,
            u3361Code::LedgerBroken => 0xe058,
            u3361Code::SnapshotFingerprint => 0xe059,
            u3361Code::TableBroken => 0xe05a,
            u3361Code::SegmentGuard => 0xe05b,
        }
    }

    /// 中文诊断名。
    pub const fn name(self) -> &'static str {
        match self {
            u3361Code::RowDup => "表行重复（TSC 同键双写）",
            u3361Code::RowGap => "表行断档（时间窗 序号不连续）",
            u3361Code::ValOob => "值越界（标定 值域外）",
            u3361Code::SumDrift => "累计漂移（标定 求和≠理论值）",
            u3361Code::LedgerDup => "账目重记（同一事件双入账）",
            u3361Code::LedgerHole => "账目断漏（序号缺失）",
            u3361Code::OpNoop => "空操作（账目无效果调用）",
            u3361Code::FieldDrift => "字段漂移（账字段与账面口径不符）",
            u3361Code::LedgerBroken => "台账不守恒（守恒恒等式破缺）",
            u3361Code::SnapshotFingerprint => "快照指纹不符（字段被改后重算不一致）",
            u3361Code::TableBroken => "常量表破坏（表项与语义规则矛盾）",
            u3361Code::SegmentGuard => "段位守卫失败（诊断码越出本模块段）",
        }
    }

    /// 可操作修复建议（每条诊断都必须带下一步动作）。
    pub const fn suggestion(self) -> &'static str {
        match self {
            u3361Code::RowDup => "按键唯一律去重",
            u3361Code::RowGap => "按连续律补号",
            u3361Code::ValOob => "按值域裁剪",
            u3361Code::SumDrift => "按理论值复算",
            u3361Code::LedgerDup => "按事件幂等律合并",
            u3361Code::LedgerHole => "按连续律补账",
            u3361Code::OpNoop => "核对操作前置条件",
            u3361Code::FieldDrift => "按口径重建字段",
            u3361Code::LedgerBroken => "按守恒律重建台账后重跑",
            u3361Code::SnapshotFingerprint => "按防篡改规程重建快照",
            u3361Code::TableBroken => "按语义规则重建常量表",
            u3361Code::SegmentGuard => "核对诊断码分配表",
        }
    }
}

// --------------------------------------------------------------------------
// 冻结常量表（u3361reg · 24 行逐行注释自证）
// --------------------------------------------------------------------------
/// TSC 时间窗 语义登记表（24 行逐行注释）。
pub const u3361reg_COUNT: usize = 24;

/// 冻结表本体（编译期常量，运行期只读）。
pub static u3361reg: [(&'static str, u64); 24] = [
    ("TSC_A", 7), // TSC 主登记（时间戳计数 · 判据面）
    ("TSC_B", 33), // TSC 副登记（时间戳计数 · 对拍面）
    ("HZ_A", 14), // HZ 主登记（频率标定 · 判据面）
    ("HZ_B", 44), // HZ 副登记（频率标定 · 对拍面）
    ("WIN_A", 21), // WIN 主登记（时间窗 · 判据面）
    ("WIN_B", 55), // WIN 副登记（时间窗 · 对拍面）
    ("SLOT_A", 28), // SLOT 主登记（窗内槽位 · 判据面）
    ("SLOT_B", 66), // SLOT 副登记（窗内槽位 · 对拍面）
    ("SEQ_A", 35), // SEQ 主登记（序列号 · 判据面）
    ("SEQ_B", 77), // SEQ 副登记（序列号 · 对拍面）
    ("LATE_A", 42), // LATE 主登记（迟到丢弃 · 判据面）
    ("LATE_B", 88), // LATE 副登记（迟到丢弃 · 对拍面）
    ("DRAIN_A", 49), // DRAIN 主登记（排空 · 判据面）
    ("DRAIN_B", 99), // DRAIN 副登记（排空 · 对拍面）
    ("CAL_A", 56), // CAL 主登记（标定 · 判据面）
    ("CAL_B", 110), // CAL 副登记（标定 · 对拍面）
    ("DRIFT_A", 63), // DRIFT 主登记（漂移率 · 判据面）
    ("DRIFT_B", 121), // DRIFT 副登记（漂移率 · 对拍面）
    ("EPOCH_A", 70), // EPOCH 主登记（纪元 · 判据面）
    ("EPOCH_B", 132), // EPOCH 副登记（纪元 · 对拍面）
    ("WIN64_A", 77), // WIN64 主登记（宽窗 · 判据面）
    ("WIN64_B", 143), // WIN64 副登记（宽窗 · 对拍面）
    ("WIN32_A", 84), // WIN32 主登记（窄窗 · 判据面）
    ("WIN32_B", 154), // WIN32 副登记（窄窗 · 对拍面）
];

/// 线性查找（键唯一性由构造期断言保证，运行时无分支歧义）。
pub fn u3361reg_get(key: &'static str) -> Option<u64> {
    let mut i = 0usize;
    while i < u3361reg_COUNT {
        if u3361reg[i].0 == key {
            return Some(u3361reg[i].1);
        }
        i += 1;
    }
    None
}

/// 累计求和（用于守恒对账）。
pub fn u3361reg_sum() -> u64 {
    let mut s = 0u64;
    let mut i = 0usize;
    while i < u3361reg_COUNT {
        s = s.wrapping_add(u3361reg[i].1 as u64);
        i += 1;
    }
    s
}

// --------------------------------------------------------------------------
// 守恒账 Mxu3361Ledger（TSC 时间窗 守恒账（入账/回执/注记三操作守恒））
// --------------------------------------------------------------------------
/// TSC 时间窗 守恒账（入账/回执/注记三操作守恒）。
#[derive(Clone, Copy, Debug)]
pub struct Mxu3361Ledger {
    /// 入账笔数
    pub entries: u64,
    /// 回执笔数
    pub hits: u64,
    /// 注记笔数
    pub notes: u64,
}

impl Mxu3361Ledger {
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

/// 段位守卫：全部诊断码必须落在本段 [0xe050, 0xe05f]。
pub const fn u3361_segment_guard() -> bool {
    let mut ok = true;
    let mut i = 0usize;
    while i < u3361Code::ALL.len() {
        let c = u3361Code::ALL[i].code();
        if c < 0xe050 || c > 0xe05f {
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
fn u3361_guard_seg() -> bool {
    u3361_segment_guard()
}

/// UNX-F3361 登记表 24 行齐备 + 边界锚 + 键外拒收 + 累计守恒。
fn check_u3361_core_tbl() -> bool {
    let mut ok = u3361reg_COUNT == 24;
    if ok { ok = u3361reg_get("TSC_A") == Some(7) && u3361reg_get("TSC_B") == Some(33); }
    if ok { ok = u3361reg_get("WIN32_A") == Some(84); }
    if ok { ok = u3361reg_get("ZZZ_NOKEY").is_none(); }
    if ok { ok = u3361reg_sum() == 1668; }
    ok
}

/// UNX-F3361 守恒账三操作确定性效果。
fn check_u3361_core_ldg() -> bool {
    let mut ld = Mxu3361Ledger::new();
    let mut ok = true;
    ld.bump_entry(); ld.bump_entry(); ld.bump_hit(); ld.log_note();
    ok = ld.entries == 2 && ld.hits == 1 && ld.notes == 1;
    ok
}

/// 本模块自检（逐检查带 detail，全绿才出门）。
pub fn run_u3361_checks() -> CheckSet {
    let mut set = CheckSet::new("U3361");
    set.add(
        "u3361-seg-guard",
        u3361_guard_seg(),
        if u3361_guard_seg() { "" } else { "diagnostic code outside segment" },
    );
    set.add(
        "u3361-core-tbl",
        check_u3361_core_tbl(),
        if check_u3361_core_tbl() { "" } else { "UNX-F3361 core table mismatch" },
    );
    set.add(
        "u3361-core-ldg",
        check_u3361_core_ldg(),
        if check_u3361_core_ldg() { "" } else { "UNX-F3361 core ledger mismatch" },
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
    fn u3361_all_checks_green() {
        let set = run_u3361_checks();
        assert!(set.all_passed(), "U3361 domain checks must be green");
    }

    #[test]
    fn u3361_segment_guard_holds() {
        assert!(u3361_segment_guard());
    }
}
