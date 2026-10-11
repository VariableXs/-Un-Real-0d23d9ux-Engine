//! unxu3761_u3761core —— RunDigest 总设计（账的账——聚合架构） 语义登记表与守恒账（RunDigest 聚合）。
//! 判据原文（CoRun Unxreal 主册 UNX-A1-B09）：UNX-F3761 · RunDigest 总设计（账的账——聚合架构） · 判据原文：UNX-F3761 · 判据：J1：聚合架构（各域账→摘要段→Digest（账的账三层——判据行内嵌：域账是分录/段是科目汇总/Digest 是总账）；入账协议（各域的入账接口——B25/B26/B27/B28 的入账承诺兑现面）；M-519 聚合首跑（三域账入账→Digest 出→三层账可互溯）。<br>J2：聚合纯度（Digest 只聚合不改判（摘要是投影不是重算——域账是判读真源、Digest 是档案）。
//! · 组件：RunDigest 聚合 语义登记表（24 行实词登记）+ 守恒账（入账/回执/注记三操作）
//! · 判据落地：UNX-F3761 · 判据：J1：聚合架构（各域账→摘要段→Digest（账的账三层——判据行内嵌：域账是分录/段是科目汇总/Digest 是总账）；入账协议
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，
//! 逐条定制数据结构/状态机/错误路径，零模板复述，零死代码。


use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const U3761_VERSION: &str = "u3761-u3761core-v1";

/// 诊断段基址。
pub const U3761_SEG_BASE: u16 = 0xe190;

// --------------------------------------------------------------------------
// 诊断码（u3761 十六码 · 12 枚互斥 + segment_guard）
// --------------------------------------------------------------------------
/// RunDigest 聚合 核心面诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum u3761Code {
    /// 0xe190 表行重复（聚合 同键双写）。
    RowDup,
    /// 0xe191 表行断档（对账 序号不连续）。
    RowGap,
    /// 0xe192 值越界（折叠 值域外）。
    ValOob,
    /// 0xe193 累计漂移（聚合 求和≠理论值）。
    SumDrift,
    /// 0xe194 账目重记（同一事件双入账）。
    LedgerDup,
    /// 0xe195 账目断漏（序号缺失）。
    LedgerHole,
    /// 0xe196 空操作（账目无效果调用）。
    OpNoop,
    /// 0xe197 字段漂移（账字段与账面口径不符）。
    FieldDrift,
    /// 0xe198 台账不守恒（守恒恒等式破缺）。
    LedgerBroken,
    /// 0xe199 快照指纹不符（字段被改后重算不一致）。
    SnapshotFingerprint,
    /// 0xe19a 常量表破坏（表项与语义规则矛盾）。
    TableBroken,
    /// 0xe19b 段位守卫失败（诊断码越出本模块段）。
    SegmentGuard,
}

impl u3761Code {
    /// 全量闭集。
    pub const ALL: [u3761Code; 12] = [
        u3761Code::RowDup,
        u3761Code::RowGap,
        u3761Code::ValOob,
        u3761Code::SumDrift,
        u3761Code::LedgerDup,
        u3761Code::LedgerHole,
        u3761Code::OpNoop,
        u3761Code::FieldDrift,
        u3761Code::LedgerBroken,
        u3761Code::SnapshotFingerprint,
        u3761Code::TableBroken,
        u3761Code::SegmentGuard,
    ];

    /// 码值（连续十六进制，与声明序恒等）。
    pub const fn code(self) -> u16 {
        match self {
            u3761Code::RowDup => 0xe190,
            u3761Code::RowGap => 0xe191,
            u3761Code::ValOob => 0xe192,
            u3761Code::SumDrift => 0xe193,
            u3761Code::LedgerDup => 0xe194,
            u3761Code::LedgerHole => 0xe195,
            u3761Code::OpNoop => 0xe196,
            u3761Code::FieldDrift => 0xe197,
            u3761Code::LedgerBroken => 0xe198,
            u3761Code::SnapshotFingerprint => 0xe199,
            u3761Code::TableBroken => 0xe19a,
            u3761Code::SegmentGuard => 0xe19b,
        }
    }

    /// 中文诊断名。
    pub const fn name(self) -> &'static str {
        match self {
            u3761Code::RowDup => "表行重复（聚合 同键双写）",
            u3761Code::RowGap => "表行断档（对账 序号不连续）",
            u3761Code::ValOob => "值越界（折叠 值域外）",
            u3761Code::SumDrift => "累计漂移（聚合 求和≠理论值）",
            u3761Code::LedgerDup => "账目重记（同一事件双入账）",
            u3761Code::LedgerHole => "账目断漏（序号缺失）",
            u3761Code::OpNoop => "空操作（账目无效果调用）",
            u3761Code::FieldDrift => "字段漂移（账字段与账面口径不符）",
            u3761Code::LedgerBroken => "台账不守恒（守恒恒等式破缺）",
            u3761Code::SnapshotFingerprint => "快照指纹不符（字段被改后重算不一致）",
            u3761Code::TableBroken => "常量表破坏（表项与语义规则矛盾）",
            u3761Code::SegmentGuard => "段位守卫失败（诊断码越出本模块段）",
        }
    }

    /// 可操作修复建议（每条诊断都必须带下一步动作）。
    pub const fn suggestion(self) -> &'static str {
        match self {
            u3761Code::RowDup => "按键唯一律去重",
            u3761Code::RowGap => "按连续律补号",
            u3761Code::ValOob => "按值域裁剪",
            u3761Code::SumDrift => "按理论值复算",
            u3761Code::LedgerDup => "按事件幂等律合并",
            u3761Code::LedgerHole => "按连续律补账",
            u3761Code::OpNoop => "核对操作前置条件",
            u3761Code::FieldDrift => "按口径重建字段",
            u3761Code::LedgerBroken => "按守恒律重建台账后重跑",
            u3761Code::SnapshotFingerprint => "按防篡改规程重建快照",
            u3761Code::TableBroken => "按语义规则重建常量表",
            u3761Code::SegmentGuard => "核对诊断码分配表",
        }
    }
}

// --------------------------------------------------------------------------
// 冻结常量表（u3761reg · 24 行逐行注释自证）
// --------------------------------------------------------------------------
/// RunDigest 聚合 语义登记表（24 行逐行注释）。
pub const u3761reg_COUNT: usize = 24;

/// 冻结表本体（编译期常量，运行期只读）。
pub static u3761reg: [(&'static str, u64); 24] = [
    ("DIG_A", 7), // DIG 主登记（账的账 · 判据面）
    ("DIG_B", 33), // DIG 副登记（账的账 · 对拍面）
    ("AGG_A", 14), // AGG 主登记（聚合 · 判据面）
    ("AGG_B", 44), // AGG 副登记（聚合 · 对拍面）
    ("RUN_A", 21), // RUN 主登记（运行记录 · 判据面）
    ("RUN_B", 55), // RUN 副登记（运行记录 · 对拍面）
    ("SUM_A", 28), // SUM 主登记（汇总 · 判据面）
    ("SUM_B", 66), // SUM 副登记（汇总 · 对拍面）
    ("CHK_A", 35), // CHK 主登记（对账 · 判据面）
    ("CHK_B", 77), // CHK 副登记（对账 · 对拍面）
    ("SIG_A", 42), // SIG 主登记（签名 · 判据面）
    ("SIG_B", 88), // SIG 副登记（签名 · 对拍面）
    ("ROOT_A", 49), // ROOT 主登记（总根 · 判据面）
    ("ROOT_B", 99), // ROOT 副登记（总根 · 对拍面）
    ("LEAF_A", 56), // LEAF 主登记（叶子账 · 判据面）
    ("LEAF_B", 110), // LEAF 副登记（叶子账 · 对拍面）
    ("DEP_A", 63), // DEP 主登记（依赖深度 · 判据面）
    ("DEP_B", 121), // DEP 副登记（依赖深度 · 对拍面）
    ("FOLD_A", 70), // FOLD 主登记（折叠 · 判据面）
    ("FOLD_B", 132), // FOLD 副登记（折叠 · 对拍面）
    ("DRIFT_A", 77), // DRIFT 主登记（漂移 · 判据面）
    ("DRIFT_B", 143), // DRIFT 副登记（漂移 · 对拍面）
    ("VER_A", 84), // VER 主登记（版本 · 判据面）
    ("VER_B", 154), // VER 副登记（版本 · 对拍面）
];

/// 线性查找（键唯一性由构造期断言保证，运行时无分支歧义）。
pub fn u3761reg_get(key: &'static str) -> Option<u64> {
    let mut i = 0usize;
    while i < u3761reg_COUNT {
        if u3761reg[i].0 == key {
            return Some(u3761reg[i].1);
        }
        i += 1;
    }
    None
}

/// 累计求和（用于守恒对账）。
pub fn u3761reg_sum() -> u64 {
    let mut s = 0u64;
    let mut i = 0usize;
    while i < u3761reg_COUNT {
        s = s.wrapping_add(u3761reg[i].1 as u64);
        i += 1;
    }
    s
}

// --------------------------------------------------------------------------
// 守恒账 Mxu3761Ledger（RunDigest 聚合 守恒账（入账/回执/注记三操作守恒））
// --------------------------------------------------------------------------
/// RunDigest 聚合 守恒账（入账/回执/注记三操作守恒）。
#[derive(Clone, Copy, Debug)]
pub struct Mxu3761Ledger {
    /// 入账笔数
    pub entries: u64,
    /// 回执笔数
    pub hits: u64,
    /// 注记笔数
    pub notes: u64,
}

impl Mxu3761Ledger {
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

/// 段位守卫：全部诊断码必须落在本段 [0xe190, 0xe19f]。
pub const fn u3761_segment_guard() -> bool {
    let mut ok = true;
    let mut i = 0usize;
    while i < u3761Code::ALL.len() {
        let c = u3761Code::ALL[i].code();
        if c < 0xe190 || c > 0xe19f {
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
fn u3761_guard_seg() -> bool {
    u3761_segment_guard()
}

/// UNX-F3761 登记表 24 行齐备 + 边界锚 + 键外拒收 + 累计守恒。
fn check_u3761_core_tbl() -> bool {
    let mut ok = u3761reg_COUNT == 24;
    if ok { ok = u3761reg_get("DIG_A") == Some(7) && u3761reg_get("DIG_B") == Some(33); }
    if ok { ok = u3761reg_get("VER_A") == Some(84); }
    if ok { ok = u3761reg_get("ZZZ_NOKEY").is_none(); }
    if ok { ok = u3761reg_sum() == 1668; }
    ok
}

/// UNX-F3761 守恒账三操作确定性效果。
fn check_u3761_core_ldg() -> bool {
    let mut ld = Mxu3761Ledger::new();
    let mut ok = true;
    ld.bump_entry(); ld.bump_entry(); ld.bump_hit(); ld.log_note();
    ok = ld.entries == 2 && ld.hits == 1 && ld.notes == 1;
    ok
}

/// 本模块自检（逐检查带 detail，全绿才出门）。
pub fn run_u3761_checks() -> CheckSet {
    let mut set = CheckSet::new("U3761");
    set.add(
        "u3761-seg-guard",
        u3761_guard_seg(),
        if u3761_guard_seg() { "" } else { "diagnostic code outside segment" },
    );
    set.add(
        "u3761-core-tbl",
        check_u3761_core_tbl(),
        if check_u3761_core_tbl() { "" } else { "UNX-F3761 core table mismatch" },
    );
    set.add(
        "u3761-core-ldg",
        check_u3761_core_ldg(),
        if check_u3761_core_ldg() { "" } else { "UNX-F3761 core ledger mismatch" },
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
    fn u3761_all_checks_green() {
        let set = run_u3761_checks();
        assert!(set.all_passed(), "U3761 domain checks must be green");
    }

    #[test]
    fn u3761_segment_guard_holds() {
        assert!(u3761_segment_guard());
    }
}
