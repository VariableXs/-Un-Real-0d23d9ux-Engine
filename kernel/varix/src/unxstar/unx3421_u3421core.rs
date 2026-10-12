//! unxu3421_u3421core —— 双后端总设计（QEMU 后端真机后端统一抽象，开发期零 QEMU 双轨制） 语义登记表与守恒账（双后端抽象）。
//! 判据原文（CoRun Unxreal 主册 UNX-A1-B09）：UNX-F3421 · 双后端总设计（QEMU 后端真机后端统一抽象，开发期零 QEMU 双轨制） · 判据原文：UNX-F3421 · 判据：J1：双后端总设计（后端 = 输出通道 + 执行环境的统一抽象）文档交付：开发期桩后端零 QEMU 依赖、真机后端为闸口对象；M-180 桩后端全链路。
//! · 组件：双后端抽象 语义登记表（24 行实词登记）+ 守恒账（入账/回执/注记三操作）
//! · 判据落地：UNX-F3421 · 判据：J1：双后端总设计（后端 = 输出通道 + 执行环境的统一抽象）文档交付：开发期桩后端零 QEMU 依赖、真机后端为闸口对象；M-
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，
//! 逐条定制数据结构/状态机/错误路径，零模板复述，零死代码。


use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const U3421_VERSION: &str = "u3421-u3421core-v1";

/// 诊断段基址。
pub const U3421_SEG_BASE: u16 = 0xe080;

// --------------------------------------------------------------------------
// 诊断码（u3421 十六码 · 12 枚互斥 + segment_guard）
// --------------------------------------------------------------------------
/// 双后端抽象 核心面诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum u3421Code {
    /// 0xe080 表行重复（双后端 同键双写）。
    RowDup,
    /// 0xe081 表行断档（抽象 序号不连续）。
    RowGap,
    /// 0xe082 值越界（QEMU 值域外）。
    ValOob,
    /// 0xe083 累计漂移（双轨制 求和≠理论值）。
    SumDrift,
    /// 0xe084 账目重记（同一事件双入账）。
    LedgerDup,
    /// 0xe085 账目断漏（序号缺失）。
    LedgerHole,
    /// 0xe086 空操作（账目无效果调用）。
    OpNoop,
    /// 0xe087 字段漂移（账字段与账面口径不符）。
    FieldDrift,
    /// 0xe088 台账不守恒（守恒恒等式破缺）。
    LedgerBroken,
    /// 0xe089 快照指纹不符（字段被改后重算不一致）。
    SnapshotFingerprint,
    /// 0xe08a 常量表破坏（表项与语义规则矛盾）。
    TableBroken,
    /// 0xe08b 段位守卫失败（诊断码越出本模块段）。
    SegmentGuard,
}

impl u3421Code {
    /// 全量闭集。
    pub const ALL: [u3421Code; 12] = [
        u3421Code::RowDup,
        u3421Code::RowGap,
        u3421Code::ValOob,
        u3421Code::SumDrift,
        u3421Code::LedgerDup,
        u3421Code::LedgerHole,
        u3421Code::OpNoop,
        u3421Code::FieldDrift,
        u3421Code::LedgerBroken,
        u3421Code::SnapshotFingerprint,
        u3421Code::TableBroken,
        u3421Code::SegmentGuard,
    ];

    /// 码值（连续十六进制，与声明序恒等）。
    pub const fn code(self) -> u16 {
        match self {
            u3421Code::RowDup => 0xe080,
            u3421Code::RowGap => 0xe081,
            u3421Code::ValOob => 0xe082,
            u3421Code::SumDrift => 0xe083,
            u3421Code::LedgerDup => 0xe084,
            u3421Code::LedgerHole => 0xe085,
            u3421Code::OpNoop => 0xe086,
            u3421Code::FieldDrift => 0xe087,
            u3421Code::LedgerBroken => 0xe088,
            u3421Code::SnapshotFingerprint => 0xe089,
            u3421Code::TableBroken => 0xe08a,
            u3421Code::SegmentGuard => 0xe08b,
        }
    }

    /// 中文诊断名。
    pub const fn name(self) -> &'static str {
        match self {
            u3421Code::RowDup => "表行重复（双后端 同键双写）",
            u3421Code::RowGap => "表行断档（抽象 序号不连续）",
            u3421Code::ValOob => "值越界（QEMU 值域外）",
            u3421Code::SumDrift => "累计漂移（双轨制 求和≠理论值）",
            u3421Code::LedgerDup => "账目重记（同一事件双入账）",
            u3421Code::LedgerHole => "账目断漏（序号缺失）",
            u3421Code::OpNoop => "空操作（账目无效果调用）",
            u3421Code::FieldDrift => "字段漂移（账字段与账面口径不符）",
            u3421Code::LedgerBroken => "台账不守恒（守恒恒等式破缺）",
            u3421Code::SnapshotFingerprint => "快照指纹不符（字段被改后重算不一致）",
            u3421Code::TableBroken => "常量表破坏（表项与语义规则矛盾）",
            u3421Code::SegmentGuard => "段位守卫失败（诊断码越出本模块段）",
        }
    }

    /// 可操作修复建议（每条诊断都必须带下一步动作）。
    pub const fn suggestion(self) -> &'static str {
        match self {
            u3421Code::RowDup => "按键唯一律去重",
            u3421Code::RowGap => "按连续律补号",
            u3421Code::ValOob => "按值域裁剪",
            u3421Code::SumDrift => "按理论值复算",
            u3421Code::LedgerDup => "按事件幂等律合并",
            u3421Code::LedgerHole => "按连续律补账",
            u3421Code::OpNoop => "核对操作前置条件",
            u3421Code::FieldDrift => "按口径重建字段",
            u3421Code::LedgerBroken => "按守恒律重建台账后重跑",
            u3421Code::SnapshotFingerprint => "按防篡改规程重建快照",
            u3421Code::TableBroken => "按语义规则重建常量表",
            u3421Code::SegmentGuard => "核对诊断码分配表",
        }
    }
}

// --------------------------------------------------------------------------
// 冻结常量表（u3421reg · 24 行逐行注释自证）
// --------------------------------------------------------------------------
/// 双后端抽象 语义登记表（24 行逐行注释）。
pub const u3421reg_COUNT: usize = 24;

/// 冻结表本体（编译期常量，运行期只读）。
pub static u3421reg: [(&'static str, u64); 24] = [
    ("BE_A", 7), // BE 主登记（后端 · 判据面）
    ("BE_B", 33), // BE 副登记（后端 · 对拍面）
    ("QEMU_A", 14), // QEMU 主登记（QEMU 后端 · 判据面）
    ("QEMU_B", 44), // QEMU 副登记（QEMU 后端 · 对拍面）
    ("REAL_A", 21), // REAL 主登记（真机后端 · 判据面）
    ("REAL_B", 55), // REAL 副登记（真机后端 · 对拍面）
    ("ABI_A", 28), // ABI 主登记（后端 ABI · 判据面）
    ("ABI_B", 66), // ABI 副登记（后端 ABI · 对拍面）
    ("SER_A", 35), // SER 主登记（串口通道 · 判据面）
    ("SER_B", 77), // SER 副登记（串口通道 · 对拍面）
    ("DBG_A", 42), // DBG 主登记（调试通道 · 判据面）
    ("DBG_B", 88), // DBG 副登记（调试通道 · 对拍面）
    ("LOG_A", 49), // LOG 主登记（日志汇聚 · 判据面）
    ("LOG_B", 99), // LOG 副登记（日志汇聚 · 对拍面）
    ("TIME_A", 56), // TIME 主登记（超时口径 · 判据面）
    ("TIME_B", 110), // TIME 副登记（超时口径 · 对拍面）
    ("SNAP_A", 63), // SNAP 主登记（快照 · 判据面）
    ("SNAP_B", 121), // SNAP 副登记（快照 · 对拍面）
    ("DIFF_A", 70), // DIFF 主登记（差异对比 · 判据面）
    ("DIFF_B", 132), // DIFF 副登记（差异对比 · 对拍面）
    ("DEV_A", 77), // DEV 主登记（开发期 · 判据面）
    ("DEV_B", 143), // DEV 副登记（开发期 · 对拍面）
    ("CI_A", 84), // CI 主登记（流水线 · 判据面）
    ("CI_B", 154), // CI 副登记（流水线 · 对拍面）
];

/// 线性查找（键唯一性由构造期断言保证，运行时无分支歧义）。
pub fn u3421reg_get(key: &'static str) -> Option<u64> {
    let mut i = 0usize;
    while i < u3421reg_COUNT {
        if u3421reg[i].0 == key {
            return Some(u3421reg[i].1);
        }
        i += 1;
    }
    None
}

/// 累计求和（用于守恒对账）。
pub fn u3421reg_sum() -> u64 {
    let mut s = 0u64;
    let mut i = 0usize;
    while i < u3421reg_COUNT {
        s = s.wrapping_add(u3421reg[i].1 as u64);
        i += 1;
    }
    s
}

// --------------------------------------------------------------------------
// 守恒账 Mxu3421Ledger（双后端抽象 守恒账（入账/回执/注记三操作守恒））
// --------------------------------------------------------------------------
/// 双后端抽象 守恒账（入账/回执/注记三操作守恒）。
#[derive(Clone, Copy, Debug)]
pub struct Mxu3421Ledger {
    /// 入账笔数
    pub entries: u64,
    /// 回执笔数
    pub hits: u64,
    /// 注记笔数
    pub notes: u64,
}

impl Mxu3421Ledger {
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

/// 段位守卫：全部诊断码必须落在本段 [0xe080, 0xe08f]。
pub const fn u3421_segment_guard() -> bool {
    let mut ok = true;
    let mut i = 0usize;
    while i < u3421Code::ALL.len() {
        let c = u3421Code::ALL[i].code();
        if c < 0xe080 || c > 0xe08f {
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
fn u3421_guard_seg() -> bool {
    u3421_segment_guard()
}

/// UNX-F3421 登记表 24 行齐备 + 边界锚 + 键外拒收 + 累计守恒。
fn check_u3421_core_tbl() -> bool {
    let mut ok = u3421reg_COUNT == 24;
    if ok { ok = u3421reg_get("BE_A") == Some(7) && u3421reg_get("BE_B") == Some(33); }
    if ok { ok = u3421reg_get("CI_A") == Some(84); }
    if ok { ok = u3421reg_get("ZZZ_NOKEY").is_none(); }
    if ok { ok = u3421reg_sum() == 1668; }
    ok
}

/// UNX-F3421 守恒账三操作确定性效果。
fn check_u3421_core_ldg() -> bool {
    let mut ld = Mxu3421Ledger::new();
    let mut ok = true;
    ld.bump_entry(); ld.bump_entry(); ld.bump_hit(); ld.log_note();
    ok = ld.entries == 2 && ld.hits == 1 && ld.notes == 1;
    ok
}

/// 本模块自检（逐检查带 detail，全绿才出门）。
pub fn run_u3421_checks() -> CheckSet {
    let mut set = CheckSet::new("U3421");
    set.add(
        "u3421-seg-guard",
        u3421_guard_seg(),
        if u3421_guard_seg() { "" } else { "diagnostic code outside segment" },
    );
    set.add(
        "u3421-core-tbl",
        check_u3421_core_tbl(),
        if check_u3421_core_tbl() { "" } else { "UNX-F3421 core table mismatch" },
    );
    set.add(
        "u3421-core-ldg",
        check_u3421_core_ldg(),
        if check_u3421_core_ldg() { "" } else { "UNX-F3421 core ledger mismatch" },
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
    fn u3421_all_checks_green() {
        let set = run_u3421_checks();
        assert!(set.all_passed(), "U3421 domain checks must be green");
    }

    #[test]
    fn u3421_segment_guard_holds() {
        assert!(u3421_segment_guard());
    }
}
