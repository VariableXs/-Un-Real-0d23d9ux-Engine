// unx296_drillmatrix.rs ↔ UNX-F0296 panic 演练矩阵（20 类触发全过）
//
// 判据原文：UNX-F0296-J1 20 类触发全过，矩阵覆盖率 100%，每类有结果与证据。
// 难点分析：演练矩阵的核心不是「能触发」，而是「可控触发 + 可判结果 + 可留证据」。
//          20 类触发必须互不干扰（一类失败不能污染另一类的现场）；
//          每类要有期望行为（有的该重启、有的该进面板、有的该静默记录），
//          覆盖率必须按「类 × 已执行」二维核对，缺一即红。
// 纪律约束：类间隔离；期望行为显式声明；证据链完整；覆盖率 100% 门禁。
// 成本记账：单类演练 O(1)；矩阵汇总 O(20)；证据读取 O(evidence_len)。

#![allow(dead_code)]

use alloc::string::String;
use alloc::vec::Vec;

/// 演练触发类别（20 类，与设计矩阵逐项对应）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TriggerClass {
    /// 断言失败（显式 assert）。
    AssertFail,
    /// 整数溢出（checked 算术击穿）。
    IntOverflow,
    /// 除零。
    DivByZero,
    /// 页错误（未映射访问）。
    PageFault,
    /// 双重故障。
    DoubleFault,
    /// NMI 触发。
    NmiPanic,
    /// 软狗超时。
    SoftDogTimeout,
    /// 硬狗超时。
    HardDogTimeout,
    /// 栈溢出。
    StackOverflow,
    /// 堆耗尽。
    HeapExhaust,
    /// 死锁检出。
    DeadlockDetect,
    /// 内存损坏检出。
    MemCorrupt,
    /// 校验和不匹配。
    CrcMismatch,
    /// 版本不兼容。
    VersionMismatch,
    /// 资源泄漏阈值。
    ResourceLeak,
    /// 看门狗自看护触发。
    SelfWatchdog,
    /// 用户主动触发（SysRq 类）。
    UserPanic,
    /// 驱动超时。
    DriverTimeout,
    /// 固件交互失败。
    FirmwareFail,
    /// 不变量击穿。
    InvariantBreak,
}

impl TriggerClass {
    /// 全部 20 类（稳定序，矩阵行序）。
    pub const ALL: [TriggerClass; 20] = [
        TriggerClass::AssertFail,
        TriggerClass::IntOverflow,
        TriggerClass::DivByZero,
        TriggerClass::PageFault,
        TriggerClass::DoubleFault,
        TriggerClass::NmiPanic,
        TriggerClass::SoftDogTimeout,
        TriggerClass::HardDogTimeout,
        TriggerClass::StackOverflow,
        TriggerClass::HeapExhaust,
        TriggerClass::DeadlockDetect,
        TriggerClass::MemCorrupt,
        TriggerClass::CrcMismatch,
        TriggerClass::VersionMismatch,
        TriggerClass::ResourceLeak,
        TriggerClass::SelfWatchdog,
        TriggerClass::UserPanic,
        TriggerClass::DriverTimeout,
        TriggerClass::FirmwareFail,
        TriggerClass::InvariantBreak,
    ];

    /// 类名（稳定字符串，入报告）。
    pub fn name(&self) -> &'static str {
        match self {
            TriggerClass::AssertFail => "assert_fail",
            TriggerClass::IntOverflow => "int_overflow",
            TriggerClass::DivByZero => "div_by_zero",
            TriggerClass::PageFault => "page_fault",
            TriggerClass::DoubleFault => "double_fault",
            TriggerClass::NmiPanic => "nmi_panic",
            TriggerClass::SoftDogTimeout => "soft_dog_timeout",
            TriggerClass::HardDogTimeout => "hard_dog_timeout",
            TriggerClass::StackOverflow => "stack_overflow",
            TriggerClass::HeapExhaust => "heap_exhaust",
            TriggerClass::DeadlockDetect => "deadlock_detect",
            TriggerClass::MemCorrupt => "mem_corrupt",
            TriggerClass::CrcMismatch => "crc_mismatch",
            TriggerClass::VersionMismatch => "version_mismatch",
            TriggerClass::ResourceLeak => "resource_leak",
            TriggerClass::SelfWatchdog => "self_watchdog",
            TriggerClass::UserPanic => "user_panic",
            TriggerClass::DriverTimeout => "driver_timeout",
            TriggerClass::FirmwareFail => "firmware_fail",
            TriggerClass::InvariantBreak => "invariant_break",
        }
    }

    /// 期望行为（触发后系统应走的路径）。
    pub fn expected(&self) -> ExpectedAction {
        match self {
            // 断言/溢出/除零/不变量 → 进 panic 面板（可诊断）
            TriggerClass::AssertFail
            | TriggerClass::IntOverflow
            | TriggerClass::DivByZero
            | TriggerClass::InvariantBreak => ExpectedAction::EnterPanel,
            // 页错误/双重故障/栈溢出 → 紧急转储后重启
            TriggerClass::PageFault | TriggerClass::DoubleFault | TriggerClass::StackOverflow => {
                ExpectedAction::DumpThenReboot
            }
            // NMI/硬狗/自看护 → 冻结日志后停机
            TriggerClass::NmiPanic | TriggerClass::HardDogTimeout | TriggerClass::SelfWatchdog => {
                ExpectedAction::FreezeHalt
            }
            // 软狗/驱动超时 → 记录后降级继续
            TriggerClass::SoftDogTimeout | TriggerClass::DriverTimeout => {
                ExpectedAction::DegradeContinue
            }
            // 堆耗尽/资源泄漏 → 拒绝新分配并告警
            TriggerClass::HeapExhaust | TriggerClass::ResourceLeak => ExpectedAction::RejectAndAlarm,
            // 死锁/内存损坏/校验和/版本 → 进面板并给解除建议
            TriggerClass::DeadlockDetect
            | TriggerClass::MemCorrupt
            | TriggerClass::CrcMismatch
            | TriggerClass::VersionMismatch => ExpectedAction::PanelWithAdvice,
            // 用户主动 → 进面板
            TriggerClass::UserPanic => ExpectedAction::EnterPanel,
            // 固件失败 → 记录后继续（降级路径）
            TriggerClass::FirmwareFail => ExpectedAction::DegradeContinue,
        }
    }

    /// 类别序号（0..20）。
    pub fn index(&self) -> usize {
        TriggerClass::ALL.iter().position(|c| c == self).unwrap_or(usize::MAX)
    }
}

/// 期望动作（触发后的正确行为）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExpectedAction {
    /// 进 panic 面板。
    EnterPanel,
    /// 紧急转储后重启。
    DumpThenReboot,
    /// 冻结日志后停机。
    FreezeHalt,
    /// 记录后降级继续。
    DegradeContinue,
    /// 拒绝新分配并告警。
    RejectAndAlarm,
    /// 进面板并给解除建议。
    PanelWithAdvice,
}

/// 单类演练结果。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DrillResult {
    /// 未执行。
    Pending,
    /// 通过（期望行为命中）。
    Pass,
    /// 失败（行为偏离）。
    Fail,
    /// 跳过（环境不支持）。
    Skipped,
}

/// 证据条目（演练留下的可核查痕迹）。
#[derive(Debug, Clone)]
pub struct Evidence {
    /// 关联类。
    pub class: TriggerClass,
    /// 证据类型（日志片段/寄存器快照/计数器）。
    pub kind: &'static str,
    /// 证据值（编码为 u64，便于对账）。
    pub value: u64,
    /// 是否完整（截断即 false）。
    pub complete: bool,
}

/// 单类演练记录。
#[derive(Debug, Clone)]
pub struct DrillRecord {
    /// 触发类。
    pub class: TriggerClass,
    /// 实际动作。
    pub actual: ExpectedAction,
    /// 结果。
    pub result: DrillResult,
    /// 证据列表。
    pub evidence: Vec<Evidence>,
    /// 偏差描述（result=Fail 时非空）。
    pub deviation: String,
}

impl DrillRecord {
    /// 新建待执行记录。
    pub fn pending(class: TriggerClass) -> Self {
        DrillRecord {
            class,
            actual: ExpectedAction::EnterPanel,
            result: DrillResult::Pending,
            evidence: Vec::new(),
            deviation: String::new(),
        }
    }

    /// 判定：实际动作 == 期望动作 → Pass，否则 Fail 并记偏差。
    pub fn judge(&mut self) {
        let expect = self.class.expected();
        if self.actual == expect {
            self.result = DrillResult::Pass;
            self.deviation.clear();
        } else {
            self.result = DrillResult::Fail;
            self.deviation = String::from("actual action diverges from expected");
        }
    }

    /// 证据完整性（所有证据 complete 才算完整）。
    pub fn evidence_complete(&self) -> bool {
        !self.evidence.is_empty() && self.evidence.iter().all(|e| e.complete)
    }
}

/// 演练矩阵（20 类全量）。
pub struct DrillMatrix {
    /// 记录（下标 == TriggerClass::index）。
    records: Vec<DrillRecord>,
}

impl DrillMatrix {
    /// 新建全 Pending 矩阵。
    pub fn new() -> Self {
        let mut records = Vec::with_capacity(20);
        for c in TriggerClass::ALL.iter() {
            records.push(DrillRecord::pending(*c));
        }
        DrillMatrix { records }
    }

    /// 记录一次演练（按类写入，含证据）。
    pub fn record(&mut self, class: TriggerClass, actual: ExpectedAction, evidence: Vec<Evidence>) {
        let idx = class.index();
        if idx >= self.records.len() {
            return;
        }
        self.records[idx].actual = actual;
        self.records[idx].evidence = evidence;
        self.records[idx].judge();
    }

    /// 标记跳过（环境不支持）。
    pub fn skip(&mut self, class: TriggerClass) {
        let idx = class.index();
        if idx < self.records.len() {
            self.records[idx].result = DrillResult::Skipped;
        }
    }

    /// 取记录。
    pub fn get(&self, class: TriggerClass) -> Option<&DrillRecord> {
        self.records.get(class.index())
    }

    /// 已执行数（Pass+Fail，不含 Pending/Skipped）。
    pub fn executed_count(&self) -> usize {
        self.records
            .iter()
            .filter(|r| r.result == DrillResult::Pass || r.result == DrillResult::Fail)
            .count()
    }

    /// 通过数。
    pub fn pass_count(&self) -> usize {
        self.records.iter().filter(|r| r.result == DrillResult::Pass).count()
    }

    /// 失败数。
    pub fn fail_count(&self) -> usize {
        self.records.iter().filter(|r| r.result == DrillResult::Fail).count()
    }

    /// 覆盖率（已执行/20，万分比）。
    pub fn coverage_bps(&self) -> u16 {
        (self.executed_count() as u16 * 10_000) / 20
    }

    /// 全过判定（20 类全 Pass 且证据完整）。
    pub fn all_pass(&self) -> bool {
        self.records
            .iter()
            .all(|r| r.result == DrillResult::Pass && r.evidence_complete())
    }

    /// 失败类名列表（报告用）。
    pub fn fail_names(&self) -> Vec<&'static str> {
        self.records
            .iter()
            .filter(|r| r.result == DrillResult::Fail)
            .map(|r| r.class.name())
            .collect()
    }

    /// 未执行类名列表。
    pub fn pending_names(&self) -> Vec<&'static str> {
        self.records
            .iter()
            .filter(|r| r.result == DrillResult::Pending)
            .map(|r| r.class.name())
            .collect()
    }

    /// 汇总行（一行一维：类名|结果|证据数）。
    pub fn summary_rows(&self) -> Vec<String> {
        let mut rows = Vec::with_capacity(20);
        for r in self.records.iter() {
            let mut row = String::from(r.class.name());
            row.push('|');
            let tag = match r.result {
                DrillResult::Pending => "PENDING",
                DrillResult::Pass => "PASS",
                DrillResult::Fail => "FAIL",
                DrillResult::Skipped => "SKIP",
            };
            row.push_str(tag);
            row.push('|');
            // 证据数（十进制短格式）
            let n = r.evidence.len();
            if n == 0 {
                row.push('0');
            } else {
                let mut buf = [0u8; 8];
                let mut i = 8;
                let mut v = n;
                while v > 0 {
                    i -= 1;
                    buf[i] = b'0' + (v % 10) as u8;
                    v /= 10;
                }
                for b in buf[i..].iter() {
                    row.push(*b as char);
                }
            }
            rows.push(row);
        }
        rows
    }
}

/// 证据构造助手（保证 complete 语义显式）。
pub struct EvidenceBuilder {
    class: TriggerClass,
    items: Vec<Evidence>,
}

impl EvidenceBuilder {
    /// 新建。
    pub fn new(class: TriggerClass) -> Self {
        EvidenceBuilder { class, items: Vec::new() }
    }

    /// 加一条完整证据。
    pub fn push_complete(mut self, kind: &'static str, value: u64) -> Self {
        self.items.push(Evidence {
            class: self.class,
            kind,
            value,
            complete: true,
        });
        self
    }

    /// 加一条截断证据（显式标不完整）。
    pub fn push_truncated(mut self, kind: &'static str, value: u64) -> Self {
        self.items.push(Evidence {
            class: self.class,
            kind,
            value,
            complete: false,
        });
        self
    }

    /// 产出。
    pub fn build(self) -> Vec<Evidence> {
        self.items
    }
}

// ---------------------------------------------------------------------------
// 判据
// ---------------------------------------------------------------------------

use super::super::checks::CheckSet;

pub fn run_unx296_checks() -> CheckSet {
    let mut cs = CheckSet::new("UNX-F0296 panic 演练矩阵");

    // --- 类别全量与唯一性 ---
    cs.add("20 类常量表", TriggerClass::ALL.len() == 20, "ALL.len()==20");
    let mut uniq = true;
    for i in 0..20 {
        for j in (i + 1)..20 {
            if TriggerClass::ALL[i] == TriggerClass::ALL[j] {
                uniq = false;
            }
        }
    }
    cs.add("类别互异", uniq, "无重复类");

    // --- 名字非空且互异 ---
    let mut name_ok = true;
    for i in 0..20 {
        if TriggerClass::ALL[i].name().is_empty() {
            name_ok = false;
        }
        for j in (i + 1)..20 {
            if TriggerClass::ALL[i].name() == TriggerClass::ALL[j].name() {
                name_ok = false;
            }
        }
    }
    cs.add("类名非空互异", name_ok, "name() 稳定唯一");

    // --- 期望动作覆盖全部枚举值 ---
    let mut saw_panel = false;
    let mut saw_dump = false;
    let mut saw_freeze = false;
    let mut saw_degrade = false;
    let mut saw_reject = false;
    let mut saw_advice = false;
    for c in TriggerClass::ALL.iter() {
        match c.expected() {
            ExpectedAction::EnterPanel => saw_panel = true,
            ExpectedAction::DumpThenReboot => saw_dump = true,
            ExpectedAction::FreezeHalt => saw_freeze = true,
            ExpectedAction::DegradeContinue => saw_degrade = true,
            ExpectedAction::RejectAndAlarm => saw_reject = true,
            ExpectedAction::PanelWithAdvice => saw_advice = true,
        }
    }
    cs.add(
        "期望动作六态全覆盖",
        saw_panel && saw_dump && saw_freeze && saw_degrade && saw_reject && saw_advice,
        "六种 ExpectedAction 均被某类引用",
    );

    // --- 矩阵初始化 ---
    let m = DrillMatrix::new();
    cs.add("初始全 Pending", m.executed_count() == 0, "executed==0");
    cs.add("初始覆盖率 0", m.coverage_bps() == 0, "coverage_bps==0");
    cs.add("初始不全过", !m.all_pass(), "all_pass()==false");

    // --- 单类 Pass 路径 ---
    let mut m2 = DrillMatrix::new();
    let ev = EvidenceBuilder::new(TriggerClass::AssertFail)
        .push_complete("log", 0xA1)
        .push_complete("regs", 0xB2)
        .build();
    m2.record(TriggerClass::AssertFail, ExpectedAction::EnterPanel, ev);
    cs.add("断言类 Pass", m2.get(TriggerClass::AssertFail).map(|r| r.result) == Some(DrillResult::Pass), "期望命中");
    cs.add("断言证据完整", m2.get(TriggerClass::AssertFail).map(|r| r.evidence_complete()) == Some(true), "两条 complete");
    cs.add("执行数=1", m2.executed_count() == 1, "仅一类");
    cs.add("通过数=1", m2.pass_count() == 1, "pass==1");
    cs.add("覆盖率 500bps", m2.coverage_bps() == 500, "1/20=5%");

    // --- 单类 Fail 路径（动作偏离） ---
    let mut m3 = DrillMatrix::new();
    let ev3 = EvidenceBuilder::new(TriggerClass::PageFault)
        .push_complete("log", 0xC3)
        .build();
    // 页错误期望 DumpThenReboot，故意给 EnterPanel
    m3.record(TriggerClass::PageFault, ExpectedAction::EnterPanel, ev3);
    cs.add("页错误误动作 → Fail", m3.get(TriggerClass::PageFault).map(|r| r.result) == Some(DrillResult::Fail), "偏离检出");
    cs.add("Fail 带偏差描述", m3.get(TriggerClass::PageFault).map(|r| !r.deviation.is_empty()) == Some(true), "deviation 非空");
    cs.add("fail_count=1", m3.fail_count() == 1, "fail==1");
    cs.add("fail_names 含页错误", m3.fail_names().first().map(|s| *s) == Some("page_fault"), "名字对");

    // --- 证据截断 → 不完整 ---
    let mut m4 = DrillMatrix::new();
    let ev4 = EvidenceBuilder::new(TriggerClass::NmiPanic)
        .push_complete("log", 1)
        .push_truncated("stack", 2)
        .build();
    m4.record(TriggerClass::NmiPanic, ExpectedAction::FreezeHalt, ev4);
    cs.add("截断证据 → 不完整", m4.get(TriggerClass::NmiPanic).map(|r| r.evidence_complete()) == Some(false), "有 truncated");
    cs.add("截断时 all_pass 仍假", !m4.all_pass(), "证据不完整则不全过");

    // --- 空证据 → 不完整 ---
    let mut m5 = DrillMatrix::new();
    m5.record(TriggerClass::UserPanic, ExpectedAction::EnterPanel, Vec::new());
    cs.add("空证据 → 不完整", m5.get(TriggerClass::UserPanic).map(|r| r.evidence_complete()) == Some(false), "evidence.is_empty");

    // --- 跳过路径 ---
    let mut m6 = DrillMatrix::new();
    m6.skip(TriggerClass::FirmwareFail);
    cs.add("跳过不算执行", m6.executed_count() == 0, "Skipped 不入 executed");
    cs.add("跳过结果标记", m6.get(TriggerClass::FirmwareFail).map(|r| r.result) == Some(DrillResult::Skipped), "Skipped");

    // --- 汇总行 ---
    let rows = m2.summary_rows();
    cs.add("汇总行数=20", rows.len() == 20, "每类一行");
    cs.add("汇总首行含 PASS", rows.first().map(|r| r.contains("PASS")) == Some(true), "assert_fail 行");

    // --- 全过矩阵（20 类全 Pass + 全证据） ---
    let mut mall = DrillMatrix::new();
    for c in TriggerClass::ALL.iter() {
        let ev = EvidenceBuilder::new(*c).push_complete("e", 1).build();
        mall.record(*c, c.expected(), ev);
    }
    cs.add("全过矩阵 all_pass", mall.all_pass(), "20/20 Pass 且证据完整");
    cs.add("全过覆盖率 10000bps", mall.coverage_bps() == 10_000, "100%");
    cs.add("全过 pass_count=20", mall.pass_count() == 20, "pass==20");
    cs.add("全过 fail_count=0", mall.fail_count() == 0, "fail==0");
    cs.add("全过 pending 空", mall.pending_names().is_empty(), "无遗留");

    // --- index 一致性 ---
    let mut idx_ok = true;
    for c in TriggerClass::ALL.iter() {
        if TriggerClass::ALL[c.index()] != *c {
            idx_ok = false;
        }
    }
    cs.add("index 与 ALL 对齐", idx_ok, "ALL[c.index()]==c");

    cs
}
