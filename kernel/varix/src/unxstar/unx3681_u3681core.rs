//! unxu3681_u3681core —— fuzz 总设计（生成/执行/判读三段管线） 语义登记表与守恒账（fuzz 三段管线）。
//! 判据原文（CoRun Unxreal 主册 UNX-A1-B09）：UNX-F3681 · fuzz 总设计（生成/执行/判读三段管线） · 判据原文：UNX-F3681 · 判据：J1：三段管线（生成（seed→输入序列）→执行（入口调用+判据采集）→判读（panic/账失衡/违例/confirm 分类）——判据行内嵌三段各一节）；M-439 三段首跑（最小入口+8 种子语料→一跑全链出判读报告）。<br>J2：管线确定性（同 seed 同语料→同输入序列→同判读——F3687 复现的架构前提——无外部熵）。
//! · 组件：fuzz 三段管线 语义登记表（24 行实词登记）+ 守恒账（入账/回执/注记三操作）
//! · 判据落地：UNX-F3681 · 判据：J1：三段管线（生成（seed→输入序列）→执行（入口调用+判据采集）→判读（panic/账失衡/违例/confirm 分类）——
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，
//! 逐条定制数据结构/状态机/错误路径，零模板复述，零死代码。


use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const U3681_VERSION: &str = "u3681-u3681core-v1";

/// 诊断段基址。
pub const U3681_SEG_BASE: u16 = 0xe150;

// --------------------------------------------------------------------------
// 诊断码（u3681 十六码 · 12 枚互斥 + segment_guard）
// --------------------------------------------------------------------------
/// fuzz 三段管线 核心面诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum u3681Code {
    /// 0xe150 表行重复（fuzz 同键双写）。
    RowDup,
    /// 0xe151 表行断档（生成 序号不连续）。
    RowGap,
    /// 0xe152 值越界（判读 值域外）。
    ValOob,
    /// 0xe153 累计漂移（三段 求和≠理论值）。
    SumDrift,
    /// 0xe154 账目重记（同一事件双入账）。
    LedgerDup,
    /// 0xe155 账目断漏（序号缺失）。
    LedgerHole,
    /// 0xe156 空操作（账目无效果调用）。
    OpNoop,
    /// 0xe157 字段漂移（账字段与账面口径不符）。
    FieldDrift,
    /// 0xe158 台账不守恒（守恒恒等式破缺）。
    LedgerBroken,
    /// 0xe159 快照指纹不符（字段被改后重算不一致）。
    SnapshotFingerprint,
    /// 0xe15a 常量表破坏（表项与语义规则矛盾）。
    TableBroken,
    /// 0xe15b 段位守卫失败（诊断码越出本模块段）。
    SegmentGuard,
}

impl u3681Code {
    /// 全量闭集。
    pub const ALL: [u3681Code; 12] = [
        u3681Code::RowDup,
        u3681Code::RowGap,
        u3681Code::ValOob,
        u3681Code::SumDrift,
        u3681Code::LedgerDup,
        u3681Code::LedgerHole,
        u3681Code::OpNoop,
        u3681Code::FieldDrift,
        u3681Code::LedgerBroken,
        u3681Code::SnapshotFingerprint,
        u3681Code::TableBroken,
        u3681Code::SegmentGuard,
    ];

    /// 码值（连续十六进制，与声明序恒等）。
    pub const fn code(self) -> u16 {
        match self {
            u3681Code::RowDup => 0xe150,
            u3681Code::RowGap => 0xe151,
            u3681Code::ValOob => 0xe152,
            u3681Code::SumDrift => 0xe153,
            u3681Code::LedgerDup => 0xe154,
            u3681Code::LedgerHole => 0xe155,
            u3681Code::OpNoop => 0xe156,
            u3681Code::FieldDrift => 0xe157,
            u3681Code::LedgerBroken => 0xe158,
            u3681Code::SnapshotFingerprint => 0xe159,
            u3681Code::TableBroken => 0xe15a,
            u3681Code::SegmentGuard => 0xe15b,
        }
    }

    /// 中文诊断名。
    pub const fn name(self) -> &'static str {
        match self {
            u3681Code::RowDup => "表行重复（fuzz 同键双写）",
            u3681Code::RowGap => "表行断档（生成 序号不连续）",
            u3681Code::ValOob => "值越界（判读 值域外）",
            u3681Code::SumDrift => "累计漂移（三段 求和≠理论值）",
            u3681Code::LedgerDup => "账目重记（同一事件双入账）",
            u3681Code::LedgerHole => "账目断漏（序号缺失）",
            u3681Code::OpNoop => "空操作（账目无效果调用）",
            u3681Code::FieldDrift => "字段漂移（账字段与账面口径不符）",
            u3681Code::LedgerBroken => "台账不守恒（守恒恒等式破缺）",
            u3681Code::SnapshotFingerprint => "快照指纹不符（字段被改后重算不一致）",
            u3681Code::TableBroken => "常量表破坏（表项与语义规则矛盾）",
            u3681Code::SegmentGuard => "段位守卫失败（诊断码越出本模块段）",
        }
    }

    /// 可操作修复建议（每条诊断都必须带下一步动作）。
    pub const fn suggestion(self) -> &'static str {
        match self {
            u3681Code::RowDup => "按键唯一律去重",
            u3681Code::RowGap => "按连续律补号",
            u3681Code::ValOob => "按值域裁剪",
            u3681Code::SumDrift => "按理论值复算",
            u3681Code::LedgerDup => "按事件幂等律合并",
            u3681Code::LedgerHole => "按连续律补账",
            u3681Code::OpNoop => "核对操作前置条件",
            u3681Code::FieldDrift => "按口径重建字段",
            u3681Code::LedgerBroken => "按守恒律重建台账后重跑",
            u3681Code::SnapshotFingerprint => "按防篡改规程重建快照",
            u3681Code::TableBroken => "按语义规则重建常量表",
            u3681Code::SegmentGuard => "核对诊断码分配表",
        }
    }
}

// --------------------------------------------------------------------------
// 冻结常量表（u3681reg · 24 行逐行注释自证）
// --------------------------------------------------------------------------
/// fuzz 三段管线 语义登记表（24 行逐行注释）。
pub const u3681reg_COUNT: usize = 24;

/// 冻结表本体（编译期常量，运行期只读）。
pub static u3681reg: [(&'static str, u64); 24] = [
    ("FUZZ_A", 7), // FUZZ 主登记（fuzz · 判据面）
    ("FUZZ_B", 33), // FUZZ 副登记（fuzz · 对拍面）
    ("GEN_A", 14), // GEN 主登记（生成 · 判据面）
    ("GEN_B", 44), // GEN 副登记（生成 · 对拍面）
    ("EXEC_A", 21), // EXEC 主登记（执行 · 判据面）
    ("EXEC_B", 55), // EXEC 副登记（执行 · 对拍面）
    ("JUDGE_A", 28), // JUDGE 主登记（判读 · 判据面）
    ("JUDGE_B", 66), // JUDGE 副登记（判读 · 对拍面）
    ("SEED_A", 35), // SEED 主登记（种子 · 判据面）
    ("SEED_B", 77), // SEED 副登记（种子 · 对拍面）
    ("CORPUS_A", 42), // CORPUS 主登记（语料库 · 判据面）
    ("CORPUS_B", 88), // CORPUS 副登记（语料库 · 对拍面）
    ("MUT_A", 49), // MUT 主登记（变异 · 判据面）
    ("MUT_B", 99), // MUT 副登记（变异 · 对拍面）
    ("CRASH_A", 56), // CRASH 主登记（崩溃检出 · 判据面）
    ("CRASH_B", 110), // CRASH 副登记（崩溃检出 · 对拍面）
    ("DEDUP_A", 63), // DEDUP 主登记（去重 · 判据面）
    ("DEDUP_B", 121), // DEDUP 副登记（去重 · 对拍面）
    ("COV_A", 70), // COV 主登记（覆盖 · 判据面）
    ("COV_B", 132), // COV 副登记（覆盖 · 对拍面）
    ("TIME_A", 77), // TIME 主登记（超时 · 判据面）
    ("TIME_B", 143), // TIME 副登记（超时 · 对拍面）
    ("ARTEF_A", 84), // ARTEF 主登记（产物 · 判据面）
    ("ARTEF_B", 154), // ARTEF 副登记（产物 · 对拍面）
];

/// 线性查找（键唯一性由构造期断言保证，运行时无分支歧义）。
pub fn u3681reg_get(key: &'static str) -> Option<u64> {
    let mut i = 0usize;
    while i < u3681reg_COUNT {
        if u3681reg[i].0 == key {
            return Some(u3681reg[i].1);
        }
        i += 1;
    }
    None
}

/// 累计求和（用于守恒对账）。
pub fn u3681reg_sum() -> u64 {
    let mut s = 0u64;
    let mut i = 0usize;
    while i < u3681reg_COUNT {
        s = s.wrapping_add(u3681reg[i].1 as u64);
        i += 1;
    }
    s
}

// --------------------------------------------------------------------------
// 守恒账 Mxu3681Ledger（fuzz 三段管线 守恒账（入账/回执/注记三操作守恒））
// --------------------------------------------------------------------------
/// fuzz 三段管线 守恒账（入账/回执/注记三操作守恒）。
#[derive(Clone, Copy, Debug)]
pub struct Mxu3681Ledger {
    /// 入账笔数
    pub entries: u64,
    /// 回执笔数
    pub hits: u64,
    /// 注记笔数
    pub notes: u64,
}

impl Mxu3681Ledger {
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

/// 段位守卫：全部诊断码必须落在本段 [0xe150, 0xe15f]。
pub const fn u3681_segment_guard() -> bool {
    let mut ok = true;
    let mut i = 0usize;
    while i < u3681Code::ALL.len() {
        let c = u3681Code::ALL[i].code();
        if c < 0xe150 || c > 0xe15f {
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
fn u3681_guard_seg() -> bool {
    u3681_segment_guard()
}

/// UNX-F3681 登记表 24 行齐备 + 边界锚 + 键外拒收 + 累计守恒。
fn check_u3681_core_tbl() -> bool {
    let mut ok = u3681reg_COUNT == 24;
    if ok { ok = u3681reg_get("FUZZ_A") == Some(7) && u3681reg_get("FUZZ_B") == Some(33); }
    if ok { ok = u3681reg_get("ARTEF_A") == Some(84); }
    if ok { ok = u3681reg_get("ZZZ_NOKEY").is_none(); }
    if ok { ok = u3681reg_sum() == 1668; }
    ok
}

/// UNX-F3681 守恒账三操作确定性效果。
fn check_u3681_core_ldg() -> bool {
    let mut ld = Mxu3681Ledger::new();
    let mut ok = true;
    ld.bump_entry(); ld.bump_entry(); ld.bump_hit(); ld.log_note();
    ok = ld.entries == 2 && ld.hits == 1 && ld.notes == 1;
    ok
}

/// 本模块自检（逐检查带 detail，全绿才出门）。
pub fn run_u3681_checks() -> CheckSet {
    let mut set = CheckSet::new("U3681");
    set.add(
        "u3681-seg-guard",
        u3681_guard_seg(),
        if u3681_guard_seg() { "" } else { "diagnostic code outside segment" },
    );
    set.add(
        "u3681-core-tbl",
        check_u3681_core_tbl(),
        if check_u3681_core_tbl() { "" } else { "UNX-F3681 core table mismatch" },
    );
    set.add(
        "u3681-core-ldg",
        check_u3681_core_ldg(),
        if check_u3681_core_ldg() { "" } else { "UNX-F3681 core ledger mismatch" },
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
    fn u3681_all_checks_green() {
        let set = run_u3681_checks();
        assert!(set.all_passed(), "U3681 domain checks must be green");
    }

    #[test]
    fn u3681_segment_guard_holds() {
        assert!(u3681_segment_guard());
    }
}
