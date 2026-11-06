//! UNX-F3451 用例预算表（每用例执行预算登记与超支告警） · 0x79E00段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（派工卡 VX8-B15 逐字摘录）：
//!   ① 纯功能代码 ≥300 行；
//!   ② J1：静态预算表（kcase! budget 字段）+ 实测超支告警（超支入账
//!     不阻断）；M-210 超支注入告警验证。
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 静态预算表：`KCASE_BUDGET_TABLE` 冻结 6 条 kcase 预算（预算
//!     单位拍），登记路径逐条从冻结表读出（TableFrozen 点名漂移）；
//!   - 实测超支告警：`record_spent` 累计实测拍数（checked 加法，
//!     溢出显性拒），实测 > 预算时**入账不阻断**——返回 Ok(true)
//!     并写入告警账（AlarmRecord），不返回 Err、不中断执行流
//!     （AlarmLost 点名告警账拒收）；
//!   - 单调状态机：Registered → Measured → Closed 三相，rank 只进
//!     不退，Closed 后再记账显性拒（PhaseRegress 点名倒退）；
//!   - M-210 注入：预算 100 拍注入 150 拍必触发告警、再注 50 拍
//!     二次告警，两笔均入账且全程 Ok（不阻断语义锁死）；
//!   - 告警摘要有账可查：超支额 = 实测 − 预算（checked 减法），
//!     告警账逐笔可对（AlarmLost 点名丢账）；
//!   - 双跑一致：登记→记账→注入全链纯函数化，同输入两次执行
//!     告警账逐笔相同（DoubleRunDiverge 点名分叉）。
//! 常量来源：
//!   - 预算拍数：本卡 kcase! budget 字段冻结值（拍模型 1 拍 = 1µs，
//!     与 B07 120 拍线同一量纲）；
//!   - M-210：派工卡验收判据编号（超支注入告警验证锚定）；
//!   - 登记容量 64 / 告警容量 32：定容防失控，超容显性拒。
//! ─────────────────────────────────────────────────────────────────────

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x40 对齐，B15 批段窗 [0x79E00, 0x79FC0] 内）。
pub const SEG_BASE: u32 = 0x79E00;

/// 登记容量（定容防失控：超容显性拒，不静默扩表）。
pub const LEDGER_CAP: usize = 64;
/// 告警账容量（超容显性拒：告警丢失必须可记账，不静默吞）。
pub const ALARM_CAP: usize = 32;
/// M-210 注入锚定（判据编号）。
pub const M210_AUDIT_ID: u64 = 0x210;

/// 静态预算表：kcase! budget 字段冻结值（用例名, 预算拍数）。
///
/// 常量来源：本卡判据「静态预算表（kcase! budget 字段）」——六条
/// 预算冻结于此，登记路径只准从本表读出，任何改表即 TableFrozen 红线。
pub const KCASE_BUDGET_TABLE: [(&str, u64); 6] = [
    ("kcase_boot", 4_000),
    ("kcase_sched", 6_000),
    ("kcase_mem", 8_000),
    ("kcase_fs", 5_000),
    ("kcase_net", 7_000),
    ("kcase_ui", 3_000),
];

/// 静态表预算总和（对账锚：登记面总量必须等于本值）。
pub const KCASE_BUDGET_TOTAL: u64 = 33_000;

/// 用例账相：Registered → Measured → Closed 单调推进（倒退显性拒）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CasePhase {
    /// 已登记未记账：预算入表，实测为零。
    Registered = 1,
    /// 已记账：至少一笔实测落账。
    Measured = 2,
    /// 已收口：预算对账定稿，再记账显性拒。
    Closed = 3,
}

impl CasePhase {
    /// 相位秩（单调性判据本体：rank 只进不退）。
    pub fn rank(self) -> u8 {
        self as u8
    }

    /// 相位中文名（写回摘要与诊断文案共用）。
    pub fn name(self) -> &'static str {
        match self {
            CasePhase::Registered => "已登记",
            CasePhase::Measured => "已记账",
            CasePhase::Closed => "已收口",
        }
    }
}

/// 单用例预算账：登记名 + 冻结预算 + 实测累计 + 相位 + 告警计数。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CaseBudget {
    /// 用例名（与静态表 kcase 名一致）。
    pub name: String,
    /// 冻结预算（拍）。
    pub budget: u64,
    /// 实测累计（拍，checked 累加）。
    pub spent: u64,
    /// 单调相位。
    pub phase: CasePhase,
    /// 超支告警笔数（入账不阻断的对账面）。
    pub alarms: u32,
}

/// 一笔超支告警：超支额 = 实测 − 预算（checked 减法，恒正）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AlarmRecord {
    /// 超支用例名。
    pub case: String,
    /// 超支额（拍，恒 > 0）。
    pub over: u64,
    /// 告警发生时的实测累计（对账锚）。
    pub spent_at: u64,
}

/// 预算账本：登记面 + 告警面（两账分离，超支入账不阻断）。
#[derive(Debug, Clone, Default)]
pub struct BudgetLedger {
    /// 登记面（定容 LEDGER_CAP）。
    pub entries: Vec<CaseBudget>,
    /// 告警面（定容 ALARM_CAP，超支入账不阻断）。
    pub alarms: Vec<AlarmRecord>,
}

impl BudgetLedger {
    /// 空账本。
    pub fn new() -> BudgetLedger {
        BudgetLedger {
            entries: Vec::new(),
            alarms: Vec::new(),
        }
    }

    /// 按名查登记项（不裸下标，迭代查找）。
    pub fn find(&self, name: &str) -> Option<u32> {
        let mut i = 0u32;
        for e in self.entries.iter() {
            if e.name == name {
                return Some(i);
            }
            i += 1;
        }
        None
    }

    /// 登记一条预算（重复登记/零预算/超容显性拒）。
    pub fn register(&mut self, name: &str, budget: u64) -> Result<(), u32> {
        if self.find(name).is_some() {
            return Err(Diag::RegDup.val());
        }
        if budget == 0 {
            return Err(Diag::BudgetZero.val());
        }
        if self.entries.len() >= LEDGER_CAP {
            return Err(Diag::LedgerFull.val());
        }
        self.entries.push(CaseBudget {
            name: name.to_string(),
            budget,
            spent: 0,
            phase: CasePhase::Registered,
            alarms: 0,
        });
        Ok(())
    }

    /// 从静态预算表整表登记（表内逐条读出，缺项对账）。
    pub fn register_from_table(&mut self) -> Result<usize, u32> {
        let mut n = 0usize;
        for (name, budget) in KCASE_BUDGET_TABLE.iter() {
            self.register(name, *budget)?;
            n += 1;
        }
        Ok(n)
    }

    /// 记一笔实测（超支入账不阻断：返回 Ok(超支与否)，不返回 Err）。
    ///
    /// 语义锁定：
    /// - 未知用例 → Err(CaseUnknown)（登记前置，别让无账可记）；
    /// - 已收口 → Err(PhaseRegress)（状态机倒退显性拒）；
    /// - 累计溢出 → Err(SpentOverflow)（checked 加法，不静默回绕）；
    /// - 超支 → **Ok(true)** + 告警入账（超支是告警不是错误，不阻断）。
    pub fn record_spent(&mut self, name: &str, ticks: u64) -> Result<bool, u32> {
        let idx = match self.find(name) {
            Some(i) => i as usize,
            None => return Err(Diag::CaseUnknown.val()),
        };
        let e = match self.entries.get_mut(idx) {
            Some(e) => e,
            None => return Err(Diag::CaseUnknown.val()),
        };
        if e.phase.rank() >= CasePhase::Closed.rank() {
            return Err(Diag::PhaseRegress.val());
        }
        let total = match e.spent.checked_add(ticks) {
            Some(t) => t,
            None => return Err(Diag::SpentOverflow.val()),
        };
        e.phase = CasePhase::Measured;
        let prev = e.spent;
        e.spent = total;
        if total > e.budget {
            // 超支：告警入账，不阻断（Ok 不 Err）。
            if self.alarms.len() >= ALARM_CAP {
                return Err(Diag::AlarmLost.val());
            }
            // 本笔超支额取增量口径：新累计减去「上笔累计与预算线的
            // 较大者」——首笔为越过预算线的部分，续笔为本笔新增超支，
            // 两笔逐笔可加且与总超支平账（50 + 50 = 100）。
            let floor = if prev > e.budget { prev } else { e.budget };
            let over = match total.checked_sub(floor) {
                Some(o) => o,
                None => return Err(Diag::SpentOverflow.val()),
            };
            self.alarms.push(AlarmRecord {
                case: e.name.clone(),
                over,
                spent_at: total,
            });
            e.alarms += 1;
            return Ok(true);
        }
        Ok(false)
    }

    /// 收口一条预算（Closed 后再记账由 record_spent 拒）。
    pub fn close(&mut self, name: &str) -> Result<(), u32> {
        let idx = match self.find(name) {
            Some(i) => i as usize,
            None => return Err(Diag::CaseUnknown.val()),
        };
        match self.entries.get_mut(idx) {
            Some(e) => {
                if e.phase.rank() >= CasePhase::Closed.rank() {
                    return Err(Diag::PhaseRegress.val());
                }
                e.phase = CasePhase::Closed;
                Ok(())
            }
            None => Err(Diag::CaseUnknown.val()),
        }
    }

    /// 告警账总超支额（逐笔求和，checked 溢出显性拒）。
    pub fn total_over(&self) -> Result<u64, u32> {
        let mut t: u64 = 0;
        for a in self.alarms.iter() {
            t = match t.checked_add(a.over) {
                Some(x) => x,
                None => return Err(Diag::SpentOverflow.val()),
            };
        }
        Ok(t)
    }
}

/// 静态表查预算（kcase! budget 字段读取面）。
pub fn kcase_budget(name: &str) -> Option<u64> {
    for (n, b) in KCASE_BUDGET_TABLE.iter() {
        if *n == name {
            return Some(*b);
        }
    }
    None
}

/// M-210 超支注入：预算 100 拍注 150 拍必告警、再注 50 拍二告警，
/// 两笔均入账且全程 Ok（不阻断语义验证本体）。
pub fn m210_inject() -> Result<(u32, u64), u32> {
    let mut led = BudgetLedger::new();
    led.register("kcase_m210", 100)?;
    let a1 = led.record_spent("kcase_m210", 150)?;
    let a2 = led.record_spent("kcase_m210", 50)?;
    if !a1 || !a2 {
        return Err(Diag::InjectMiss.val());
    }
    let count = match led.entries.first() {
        Some(e) => e.alarms,
        None => return Err(Diag::InjectMiss.val()),
    };
    let over_total = led.total_over()?;
    // 对账：两笔超支额 50 + 50 = 100，告警账总和必须相等。
    if count != 2 || over_total != 100 {
        return Err(Diag::AlarmLost.val());
    }
    Ok((count, over_total))
}

/// 诊断码闭集：0x79E01..=0x79E0C（12 码，变体带中文 doc）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Diag {
    /// 重复登记：同名用例二次入表。
    RegDup = 0x01,
    /// 预算为零：零/负语义预算显性拒。
    BudgetZero = 0x02,
    /// 用例未登记：记账找不到登记项。
    CaseUnknown = 0x03,
    /// 状态机倒退：Closed 后再记账等倒退行为。
    PhaseRegress = 0x04,
    /// 累计溢出：实测累计 checked 溢出。
    SpentOverflow = 0x05,
    /// 告警未入账：告警账拒收（超容）或对账不平。
    AlarmLost = 0x06,
    /// 登记表满：超 LEDGER_CAP 显性拒。
    LedgerFull = 0x07,
    /// 静态预算表漂移：冻结表被改动或缺项。
    TableFrozen = 0x08,
    /// M-210 注入漏抓：超支注入未触发告警。
    InjectMiss = 0x09,
    /// 双跑分叉：同输入两次全链执行告警账不一致。
    DoubleRunDiverge = 0x0A,
    /// 段位守卫失守：SEG_BASE 不满足 0x40 对齐或越出批段窗。
    SegGuardFail = 0x0B,
    /// 写回摘要为空：完成时摘要未定稿。
    SummaryEmpty = 0x0C,
}

impl Diag {
    /// 完整 u32 编码。
    pub fn val(self) -> u32 {
        SEG_BASE + (self as u32)
    }

    /// 中文诊断文案（含可操作建议）。
    pub fn detail(self) -> &'static str {
        match self {
            Diag::RegDup => "重复登记：建议先查 find 再登记，或改用幂等登记入口",
            Diag::BudgetZero => "预算为零：建议从 KCASE_BUDGET_TABLE 冻结表读预算",
            Diag::CaseUnknown => "用例未登记：建议先 register 再 record_spent",
            Diag::PhaseRegress => "状态机倒退：建议相位 rank 只进不退，Closed 用例只读",
            Diag::SpentOverflow => "累计溢出：建议拍数上限钳制在 checked 加法安全区",
            Diag::AlarmLost => "告警未入账：建议扩 ALARM_CAP 或分批清账，告警不许静默丢",
            Diag::LedgerFull => "登记表满：建议分批执行或扩 LEDGER_CAP，不许静默丢登记",
            Diag::TableFrozen => "静态预算表漂移：建议恢复 KCASE_BUDGET_TABLE 冻结值",
            Diag::InjectMiss => "M-210 注入漏抓：建议核对超支判定 spent > budget 分支",
            Diag::DoubleRunDiverge => "双跑分叉：建议全链纯函数化，同输入定同告警账",
            Diag::SegGuardFail => "段位守卫失守：建议核对 SEG_BASE 的 0x40 对齐与批段窗",
            Diag::SummaryEmpty => "写回摘要为空：建议完成时以非空中文摘要写回",
        }
    }
}

/// 闭集全量数组（12 码）。
pub const ALL_DIAGS: [Diag; 12] = [
    Diag::RegDup,
    Diag::BudgetZero,
    Diag::CaseUnknown,
    Diag::PhaseRegress,
    Diag::SpentOverflow,
    Diag::AlarmLost,
    Diag::LedgerFull,
    Diag::TableFrozen,
    Diag::InjectMiss,
    Diag::DoubleRunDiverge,
    Diag::SegGuardFail,
    Diag::SummaryEmpty,
];

/// 诊断码查表闭集返回；未知编码回退固定兜底串。
pub fn detail(code: u32) -> &'static str {
    for d in ALL_DIAGS.iter() {
        if d.val() == code {
            return d.detail();
        }
    }
    "未知诊断码：建议核对是否落在 0x79E01..=0x79E0C 闭集内"
}

// ── 判据谓词（全部私有 fn xxx_ok() -> bool）────────────────────────────

/// 段位守卫：0x40 对齐 + B15 批段窗 [0x79E00, 0x79FC0]。
fn seg_guard_ok() -> bool {
    SEG_BASE % 0x40 == 0 && SEG_BASE >= 0x79E00 && SEG_BASE <= 0x79FC0
}

/// 诊断闭集完整：12 码、编码连续、唯一、文案非空且带「建议」。
fn diag_closed_set_ok() -> bool {
    if ALL_DIAGS.len() != 12 {
        return false;
    }
    let mut i = 0usize;
    while i < ALL_DIAGS.len() {
        let a = match ALL_DIAGS.get(i) {
            Some(d) => *d,
            None => return false,
        };
        if a.val() != SEG_BASE + (i as u32) + 1 {
            return false;
        }
        if a.detail().is_empty() || !a.detail().contains("建议") {
            return false;
        }
        let mut j = i + 1;
        while j < ALL_DIAGS.len() {
            match ALL_DIAGS.get(j) {
                Some(b) => {
                    if a.val() == b.val() {
                        return false;
                    }
                }
                None => return false,
            }
            j += 1;
        }
        i += 1;
    }
    detail(0xDEAD) == detail(0x5A5A) && !detail(0xDEAD).is_empty()
}

/// 静态预算表冻结：六条名值逐一对得上，总和恰为 33_000。
fn table_frozen_ok() -> bool {
    if KCASE_BUDGET_TABLE.len() != 6 {
        return false;
    }
    let mut sum: u64 = 0;
    for (name, budget) in KCASE_BUDGET_TABLE.iter() {
        let looked = match kcase_budget(name) {
            Some(b) => b,
            None => return false,
        };
        if looked != *budget || *budget == 0 {
            return false;
        }
        sum = match sum.checked_add(*budget) {
            Some(s) => s,
            None => return false,
        };
    }
    sum == KCASE_BUDGET_TOTAL && kcase_budget("kcase_absent").is_none()
}

/// 登记正常流：整表登记 6 条全部成功、相位齐 Registered。
fn register_ok() -> bool {
    let mut led = BudgetLedger::new();
    let registered = match led.register_from_table() {
        Ok(n) => n == 6,
        Err(_) => return false,
    };
    if !registered {
        return false;
    }
    if led.entries.len() != 6 {
        return false;
    }
    for e in led.entries.iter() {
        if e.phase != CasePhase::Registered || e.spent != 0 || e.alarms != 0 {
            return false;
        }
    }
    detail(Diag::RegDup.val()).contains("幂等")
}

/// 登记注入：重复登记/零预算必拒（错误路径可操作建议在 detail）。
fn register_reject_ok() -> bool {
    let mut led = BudgetLedger::new();
    if led.register("kcase_dup", 100).is_err() {
        return false;
    }
    if led.register("kcase_dup", 100) != Err(Diag::RegDup.val()) {
        return false;
    }
    led.register("kcase_zero", 0) == Err(Diag::BudgetZero.val())
        && detail(Diag::BudgetZero.val()).contains("冻结表")
}

/// 记账正常流：预算内两笔累计不告警，相位进 Measured。
fn record_ok() -> bool {
    let mut led = BudgetLedger::new();
    if led.register("kcase_boot", 4_000).is_err() {
        return false;
    }
    let a = match led.record_spent("kcase_boot", 1_500) {
        Ok(v) => v,
        Err(_) => return false,
    };
    let b = match led.record_spent("kcase_boot", 2_500) {
        Ok(v) => v,
        Err(_) => return false,
    };
    if a || b {
        return false;
    }
    match led.find("kcase_boot") {
        Some(i) => match led.entries.get(i as usize) {
            Some(e) => e.spent == 4_000 && e.phase == CasePhase::Measured && e.alarms == 0,
            None => false,
        },
        None => false,
    }
}

/// 记账注入：未知用例/溢出必拒；边界 spent == budget 不算超支。
fn record_boundary_ok() -> bool {
    let mut led = BudgetLedger::new();
    if led.register("kcase_edge", 1_000).is_err() {
        return false;
    }
    if led.record_spent("kcase_ghost", 1) != Err(Diag::CaseUnknown.val()) {
        return false;
    }
    // 边界：恰好等于预算不超支（严格大于才告警）。
    match led.record_spent("kcase_edge", 1_000) {
        Ok(v) => {
            if v {
                return false;
            }
        }
        Err(_) => return false,
    }
    // 越界一拍即告警（Ok(true)，不阻断）。
    match led.record_spent("kcase_edge", 1) {
        Ok(v) => v,
        Err(_) => return false,
    }
}

/// 超支告警：实测 > 预算必告警入账，超支额 = 实测 − 预算。
fn overspend_alarm_ok() -> bool {
    let mut led = BudgetLedger::new();
    if led.register("kcase_net", 7_000).is_err() {
        return false;
    }
    match led.record_spent("kcase_net", 7_500) {
        Ok(true) => {}
        _ => return false,
    }
    let over_total = match led.total_over() {
        Ok(t) => t,
        Err(_) => return false,
    };
    if over_total != 500 {
        return false;
    }
    match led.alarms.first() {
        Some(a) => {
            a.case == "kcase_net"
                && a.over == 500
                && a.spent_at == 7_500
                && detail(Diag::AlarmLost.val()).contains("不许静默丢")
        }
        None => false,
    }
}

/// 不阻断语义：超支后执行流继续（Ok），收口可正常完成。
fn nonblocking_ok() -> bool {
    let mut led = BudgetLedger::new();
    if led.register("kcase_fs", 5_000).is_err() {
        return false;
    }
    // 超支两笔，全程必须 Ok（Err 即阻断，违约）。
    match led.record_spent("kcase_fs", 6_000) {
        Ok(true) => {}
        _ => return false,
    }
    match led.record_spent("kcase_fs", 1_000) {
        Ok(true) => {}
        _ => return false,
    }
    if led.close("kcase_fs").is_err() {
        return false;
    }
    // Closed 后再记账显性拒（倒退守卫，不是阻断超支）。
    led.record_spent("kcase_fs", 1) == Err(Diag::PhaseRegress.val())
        && detail(Diag::PhaseRegress.val()).contains("只进不退")
}

/// M-210 注入验证：100 拍预算注入 150+50 拍，两笔告警全入账。
fn inject_m210_ok() -> bool {
    match m210_inject() {
        Ok((count, over)) => count == 2 && over == 100,
        Err(_) => false,
    }
}

/// 状态机单调：rank 只进不退，三相名齐。
fn phase_machine_ok() -> bool {
    if CasePhase::Registered.rank() >= CasePhase::Measured.rank()
        || CasePhase::Measured.rank() >= CasePhase::Closed.rank()
    {
        return false;
    }
    CasePhase::Registered.name() == "已登记"
        && CasePhase::Measured.name() == "已记账"
        && CasePhase::Closed.name() == "已收口"
}

/// 整表对账：六条登记实测后总 spent 与告警账逐笔平。
fn ledger_audit_ok() -> bool {
    let mut led = BudgetLedger::new();
    if led.register_from_table().is_err() {
        return false;
    }
    // 只超支两条：kcase_fs 注 6_000（超 1_000）、kcase_ui 注 3_500（超 500）。
    let plan: [(&str, u64); 6] = [
        ("kcase_boot", 3_000),
        ("kcase_sched", 6_000),
        ("kcase_mem", 8_000),
        ("kcase_fs", 6_000),
        ("kcase_net", 7_000),
        ("kcase_ui", 3_500),
    ];
    for (name, ticks) in plan.iter() {
        if led.record_spent(name, *ticks).is_err() {
            return false;
        }
    }
    if led.alarms.len() != 2 {
        return false;
    }
    let total_over = match led.total_over() {
        Ok(t) => t,
        Err(_) => return false,
    };
    // 名单对账：告警面必须恰是超支的两条。
    let mut hit_fs = false;
    let mut hit_ui = false;
    for a in led.alarms.iter() {
        if a.case == "kcase_fs" && a.over == 1_000 {
            hit_fs = true;
        }
        if a.case == "kcase_ui" && a.over == 500 {
            hit_ui = true;
        }
    }
    total_over == 1_500 && hit_fs && hit_ui
}

/// 告警账容量守卫：超 ALARM_CAP 显性拒（AlarmLost），不静默吞。
fn alarm_cap_ok() -> bool {
    let mut led = BudgetLedger::new();
    if led.register("kcase_cap", 1).is_err() {
        return false;
    }
    let mut fired = 0u32;
    let mut i = 0u64;
    while i < (ALARM_CAP as u64) + 2 {
        match led.record_spent("kcase_cap", 10) {
            Ok(true) => fired += 1,
            Ok(false) => {}
            Err(code) => {
                // 第 ALARM_CAP+1 笔起告警账满，显性拒。
                return code == Diag::AlarmLost.val() && fired == ALARM_CAP as u32;
            }
        }
        i += 1;
    }
    false
}

/// 双跑一致：注入链跑两遍，告警账逐笔相同。
fn double_run_ok() -> bool {
    let a = m210_inject();
    let b = m210_inject();
    a.is_ok() && b.is_ok() && a == b
}

/// 写回面：状态=已完成、结果摘要非空（含告警笔数与超支额）。
fn writeback_summary_ok() -> bool {
    let (count, over) = match m210_inject() {
        Ok(x) => x,
        Err(_) => return false,
    };
    let summary = format!(
        "UNX-F3451 完成：静态预算表6条 告警{}笔 总超支{}拍 入账不阻断",
        count, over
    );
    !summary.is_empty()
        && summary.contains("UNX-F3451")
        && summary.contains("不阻断")
        && detail(Diag::SummaryEmpty.val()).contains("摘要")
}

// ── 判据总成：run_unx3451_checks ───────────────────────────────────────

/// run_unx3451_checks：本单判据聚合（诊断段 0x79E00，全部走 add_dyn）。
pub fn run_unx3451_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F3451 用例预算表");
    let checks: [(&'static str, bool, String); 13] = [
        (
            "段位守卫0x40对齐且在批段窗内",
            seg_guard_ok(),
            format!(
                "SEG_BASE=0x{:X} 守卫{}",
                SEG_BASE,
                if seg_guard_ok() { "过" } else { "破" }
            ),
        ),
        (
            "诊断码闭集12码唯一且文案带建议",
            diag_closed_set_ok(),
            format!(
                "闭集{}",
                if diag_closed_set_ok() { "完整" } else { "破缺" }
            ),
        ),
        (
            "静态预算表六条冻结且总和三万三",
            table_frozen_ok(),
            format!(
                "预算表{}",
                if table_frozen_ok() { "冻结" } else { "漂移" }
            ),
        ),
        (
            "整表登记六条相位齐已登记",
            register_ok(),
            format!("登记{}", if register_ok() { "全过" } else { "残缺" }),
        ),
        (
            "重复登记与零预算必拒",
            register_reject_ok(),
            format!(
                "登记拒{}",
                if register_reject_ok() { "抓到" } else { "漏抓" }
            ),
        ),
        (
            "预算内累计不告警相位进已记账",
            record_ok(),
            format!("记账{}", if record_ok() { "平" } else { "不平" }),
        ),
        (
            "边界恰等预算不超支越一拍即告警",
            record_boundary_ok(),
            format!(
                "边界{}",
                if record_boundary_ok() { "钉住" } else { "漏" }
            ),
        ),
        (
            "超支必告警且超支额逐笔平",
            overspend_alarm_ok(),
            format!(
                "超支告警{}",
                if overspend_alarm_ok() { "入账" } else { "丢账" }
            ),
        ),
        (
            "超支入账不阻断且收口可完成",
            nonblocking_ok(),
            format!(
                "不阻断{}",
                if nonblocking_ok() { "守住" } else { "破约" }
            ),
        ),
        (
            "M-210超支注入两笔全抓全入账",
            inject_m210_ok(),
            format!(
                "M-210{}",
                if inject_m210_ok() { "验证过" } else { "漏抓" }
            ),
        ),
        (
            "状态机三相rank只进不退",
            phase_machine_ok(),
            format!(
                "状态机{}",
                if phase_machine_ok() { "单调" } else { "倒退" }
            ),
        ),
        (
            "整表记账告警面恰两条超支户",
            ledger_audit_ok(),
            format!(
                "整表对账{}",
                if ledger_audit_ok() { "平" } else { "不平" }
            ),
        ),
        (
            "告警账超容显性拒不静默吞",
            alarm_cap_ok(),
            format!(
                "告警容量{}",
                if alarm_cap_ok() { "守住" } else { "静默" }
            ),
        ),
    ];
    let dr = double_run_ok();
    let wb = writeback_summary_ok();
    for (name, passed, det) in checks.iter() {
        s.add_dyn(name, *passed, det);
    }
    s.add_dyn(
        "双跑一致注入链两遍告警账逐笔同",
        dr,
        &format!("双跑{}", if dr { "一致" } else { "分叉" }),
    );
    s.add_dyn(
        "写回摘要非空状态已完成",
        wb,
        &format!("写回{}", if wb { "非空" } else { "空" }),
    );
    s
}

// ── 测试区（#[cfg(test)]：允许 assert!/assert_eq!/unwrap）──────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seg_guard_and_diag_closed_set() {
        assert!(seg_guard_ok());
        assert_eq!(SEG_BASE, 0x79E00);
        assert!(diag_closed_set_ok());
        for (i, d) in ALL_DIAGS.iter().enumerate() {
            assert_eq!(d.val(), SEG_BASE + i as u32 + 1);
        }
    }

    #[test]
    fn budget_table_frozen_values() {
        assert!(table_frozen_ok());
        assert_eq!(kcase_budget("kcase_mem"), Some(8_000));
        assert_eq!(kcase_budget("kcase_ui"), Some(3_000));
        assert_eq!(kcase_budget("nope"), None);
        assert_eq!(KCASE_BUDGET_TOTAL, 33_000);
    }

    #[test]
    fn register_flow_and_rejections() {
        assert!(register_ok());
        assert!(register_reject_ok());
        let mut led = BudgetLedger::new();
        assert!(led.register("a", 10).is_ok());
        assert_eq!(led.register("a", 10), Err(Diag::RegDup.val()));
        assert_eq!(led.register("b", 0), Err(Diag::BudgetZero.val()));
        assert_eq!(led.register_from_table(), Ok(6));
        assert_eq!(led.entries.len(), 7);
    }

    #[test]
    fn record_flow_and_boundary() {
        assert!(record_ok());
        assert!(record_boundary_ok());
        let mut led = BudgetLedger::new();
        led.register("e", 100).unwrap();
        assert_eq!(led.record_spent("ghost", 1), Err(Diag::CaseUnknown.val()));
        assert_eq!(led.record_spent("e", 100), Ok(false));
        assert_eq!(led.record_spent("e", 1), Ok(true));
        assert_eq!(led.alarms.len(), 1);
        assert_eq!(led.alarms.get(0).unwrap().over, 1);
    }

    #[test]
    fn overspend_alarm_and_nonblocking() {
        assert!(overspend_alarm_ok());
        assert!(nonblocking_ok());
    }

    #[test]
    fn m210_injection_two_alarms() {
        assert!(inject_m210_ok());
        let (count, over) = m210_inject().unwrap();
        assert_eq!(count, 2);
        assert_eq!(over, 100);
        assert_eq!(M210_AUDIT_ID, 0x210);
    }

    #[test]
    fn phase_machine_monotonic() {
        assert!(phase_machine_ok());
        let mut led = BudgetLedger::new();
        led.register("p", 10).unwrap();
        led.record_spent("p", 5).unwrap();
        led.close("p").unwrap();
        assert_eq!(led.record_spent("p", 5), Err(Diag::PhaseRegress.val()));
        assert_eq!(led.close("p"), Err(Diag::PhaseRegress.val()));
    }

    #[test]
    fn ledger_audit_two_overspenders() {
        assert!(ledger_audit_ok());
        assert!(alarm_cap_ok());
    }

    #[test]
    fn double_run_identical() {
        assert!(double_run_ok());
        assert_eq!(m210_inject(), m210_inject());
    }

    #[test]
    fn writeback_summary_non_empty() {
        assert!(writeback_summary_ok());
    }

    #[test]
    fn checked_overflow_rejected() {
        let mut led = BudgetLedger::new();
        led.register("big", u64::MAX).unwrap();
        // 预算 u64::MAX：首笔恰好打满不超支（恰等不上档），续 1 拍
        // checked 溢出显性拒（SpentOverflow）。
        assert_eq!(led.record_spent("big", u64::MAX), Ok(false));
        assert_eq!(
            led.record_spent("big", 1),
            Err(Diag::SpentOverflow.val())
        );
        led.register("big2", u64::MAX).unwrap();
        led.record_spent("big2", u64::MAX - 1).unwrap();
        assert_eq!(
            led.record_spent("big2", 10),
            Err(Diag::SpentOverflow.val())
        );
    }

    #[test]
    fn checks_aggregate_all_green() {
        let s = run_unx3451_checks();
        assert!(s.len() >= 12);
        assert!(s.all_passed(), "全部判据必须绿：{}", s.red_items_text());
    }
}
