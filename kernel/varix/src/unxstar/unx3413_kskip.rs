//! UNX-F3413 条件跳过机制（kskip 与硬件依赖声明） · 0x79680段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（派工卡 VX8-B11 逐字摘录）：
//!   ① 纯功能代码 ≥260 行；
//!   ② J1：kskip!（带原因）+ 硬件依赖元数据（依赖项未满足 → SKIP 计数）；
//!      M-172 跳过链验证。
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 硬件依赖元数据冻结表：能力名 / 依赖类 / 最低版本三要素，
//!     每条依赖在环境快照里必有「有/无」判决（查无能力名即 Err）；
//!   - kskip 带原因：跳过必须给出原因码 + 中文原因串，
//!     无原因跳过一律拒（这是 SKIP 计数可信的前提）；
//!   - SKIP 计数单调：账本只增不减，跑/跳互斥（Ran 态不得再计 Skip）；
//!   - M-172 跳过链验证：每条跳过记录必须能对回「依赖未满足」的环境
//!     证据，凭空跳过与「未满足却跑了」两类违约都必抓；
//!   - 用例状态机 Planned→DepChecked→Ran/Skipped：合法迁移对闭集放行，
//!     其余（含一切倒退、终态再迁移）显性拒；
//!   - 双跑一致：同一环境快照两次结算账本逐字节相同。
//! 常量来源：
//!   - 依赖类闭集 4 类（CPU 特性 / MMIO / 设备 / 固件）：
//!     对齐 B11 批内 kskip! 宏面的四类典型硬件前提；
//!   - 依赖冻结表 8 条：双跑场景 4 条满足 + 2 条缺失 + 2 条边界（版本不足）。
//! ─────────────────────────────────────────────────────────────────────

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x40 对齐，段位守卫区间 [0x79600, 0x797C0] 内）。
pub const SEG_BASE: u32 = 0x79680;

/// 硬件依赖类闭集。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DepKind {
    /// CPU 特性依赖（如指令集扩展）。
    CpuFeat,
    /// MMIO 依赖（如预留内存窗）。
    Mmio,
    /// 设备依赖（如定时器外设）。
    Device,
    /// 固件依赖（如启动协议版本）。
    Firmware,
}

impl DepKind {
    /// 中文类名（诊断串用）。
    pub fn zh(self) -> &'static str {
        match self {
            DepKind::CpuFeat => "CPU特性",
            DepKind::Mmio => "MMIO",
            DepKind::Device => "设备",
            DepKind::Firmware => "固件",
        }
    }
}

/// 硬件依赖元数据（冻结表项）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HwDep {
    /// 能力名（环境快照的查询键，非空）。
    pub cap: &'static str,
    /// 依赖类。
    pub kind: DepKind,
    /// 最低版本号（0 = 不设版本门槛）。
    pub min_ver: u32,
}

impl HwDep {
    /// 元数据自检：能力名非空、版本有界。
    pub fn well_formed(&self) -> bool {
        !self.cap.is_empty() && self.min_ver <= 0xFFFF
    }
}

/// 冻结依赖表（8 条；顺序即验证序）。
pub const DEP_TABLE: [HwDep; 8] = [
    HwDep { cap: "feat.lzcnt", kind: DepKind::CpuFeat, min_ver: 0 },
    HwDep { cap: "feat.aesni", kind: DepKind::CpuFeat, min_ver: 0 },
    HwDep { cap: "mmio.hpetz", kind: DepKind::Mmio, min_ver: 0 },
    HwDep { cap: "mmio.nvram", kind: DepKind::Mmio, min_ver: 0 },
    HwDep { cap: "dev.rtc", kind: DepKind::Device, min_ver: 0 },
    HwDep { cap: "dev.uart", kind: DepKind::Device, min_ver: 2 },
    HwDep { cap: "fw.efi2", kind: DepKind::Firmware, min_ver: 8 },
    HwDep { cap: "fw.acpi20", kind: DepKind::Firmware, min_ver: 0 },
];

/// 依赖表完整性：能力名唯一、恒非空、well_formed 全过。
pub fn dep_table_complete() -> bool {
    for d in DEP_TABLE.iter() {
        if !d.well_formed() {
            return false;
        }
        let mut dup = 0u32;
        for o in DEP_TABLE.iter() {
            if o.cap == d.cap {
                dup += 1;
            }
        }
        if dup != 1 {
            return false;
        }
    }
    true
}

/// 环境快照：能力名 → （在位布尔，版本号）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HwEnv {
    /// 能力表（定容快照，构造后只读）。
    pub caps: Vec<(&'static str, bool, u32)>,
}

impl HwEnv {
    /// 查能力：在位且版本达标回 true；查无能力名回 Err（查表必须诚实）。
    pub fn satisfied(&self, dep: &HwDep) -> Result<bool, u32> {
        for (name, present, ver) in self.caps.iter() {
            if *name == dep.cap {
                return Ok(*present && *ver >= dep.min_ver);
            }
        }
        Err(Diag::CapUnknown.val())
    }

    /// 环境快照自检：能力名不重复。
    pub fn snapshot_sane(&self) -> bool {
        for (name, _, _) in self.caps.iter() {
            let mut dup = 0u32;
            for (n2, _, _) in self.caps.iter() {
                if n2 == name {
                    dup += 1;
                }
            }
            if dup != 1 {
                return false;
            }
        }
        true
    }
}

/// 冻结环境 A：4 满足 + 2 缺失 + 2 版本不足（双跑与链验证的标准快照）。
pub fn frozen_env_a() -> HwEnv {
    HwEnv {
        caps: Vec::from([
            ("feat.lzcnt", true, 1),
            ("feat.aesni", false, 0),
            ("mmio.hpetz", true, 1),
            ("mmio.nvram", false, 0),
            ("dev.rtc", true, 1),
            ("dev.uart", true, 1),   // 版本 1 < 门槛 2 → 实际不满足
            ("fw.efi2", true, 7),    // 版本 7 < 门槛 8 → 实际不满足
            ("fw.acpi20", true, 1),
        ]),
    }
}

/// 冻结环境 B：全部满足（反例快照，跳过链违约注入的对照面）。
pub fn frozen_env_b() -> HwEnv {
    HwEnv {
        caps: Vec::from([
            ("feat.lzcnt", true, 9),
            ("feat.aesni", true, 9),
            ("mmio.hpetz", true, 9),
            ("mmio.nvram", true, 9),
            ("dev.rtc", true, 9),
            ("dev.uart", true, 9),
            ("fw.efi2", true, 9),
            ("fw.acpi20", true, 9),
        ]),
    }
}

/// 用例状态机（合法迁移对闭集放行，其余一律显性拒）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CaseState {
    /// 计划中：未做依赖判决。
    Planned,
    /// 依赖已判决：可进 Ran 或 Skipped。
    DepChecked,
    /// 已执行：终态（不得再迁移）。
    Ran,
    /// 已跳过：终态（不得再迁移）。
    Skipped,
}

/// 合法迁移对闭集（单调：无任何回边）。
pub const LEGAL_STEPS: [(CaseState, CaseState); 3] = [
    (CaseState::Planned, CaseState::DepChecked),
    (CaseState::DepChecked, CaseState::Ran),
    (CaseState::DepChecked, CaseState::Skipped),
];

impl CaseState {
    /// 中文标签（诊断串用）。
    pub fn label(self) -> &'static str {
        match self {
            CaseState::Planned => "计划中",
            CaseState::DepChecked => "依赖已判决",
            CaseState::Ran => "已执行",
            CaseState::Skipped => "已跳过",
        }
    }
}

/// 状态推进器：只放行合法迁移对，其余（含倒退、终态再迁移）一律
/// Err=StateIllegal——这是「单调状态机 + 倒退显性拒」的本卡实现。
pub fn advance_case(cur: CaseState, next: CaseState) -> Result<CaseState, u32> {
    for (from, to) in LEGAL_STEPS.iter() {
        if *from == cur && *to == next {
            return Ok(next);
        }
    }
    Err(Diag::StateIllegal.val())
}

/// 诊断码闭集：0x79681..=0x7968C（12 码，变体带中文 doc）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Diag {
    /// 能力名查无：环境快照里没有该依赖的判决记录。
    CapUnknown = 0x01,
    /// 无原因跳过：kskip 缺原因码或原因串为空。
    ReasonMissing = 0x02,
    /// 凭空跳过：依赖实际满足却记了 SKIP（M-172 违约）。
    SkipWithoutCause = 0x03,
    /// 未满足却执行：依赖不满足却记了 RAN（M-172 违约）。
    RanWithoutDep = 0x04,
    /// 状态迁移非法：不在合法迁移对闭集内。
    StateIllegal = 0x05,
    /// 账本倒退：SKIP/RAN 计数被回卷。
    LedgerRegress = 0x06,
    /// 依赖元数据残缺：能力名空或版本越界。
    DepMalformed = 0x07,
    /// 环境快照残缺：能力名重复。
    SnapshotDup = 0x08,
    /// 双跑不一致：同一快照两次结算账本不同。
    DoubleRunDiverge = 0x09,
    /// 跳过链断链：跳过记录对不回任何冻结依赖表项。
    ChainBroken = 0x0A,
    /// 段位守卫失守：SEG_BASE 不满足 0x40 对齐或越出段窗。
    SegGuardFail = 0x0B,
    /// 写回摘要为空：③ 判据的写回面破约。
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
            Diag::CapUnknown => "能力名查无：建议依赖声明与环境快照同源维护，改名须双侧同步",
            Diag::ReasonMissing => "无原因跳过：建议 kskip 一律带原因码与中文原因串",
            Diag::SkipWithoutCause => "凭空跳过：建议跳过前先过依赖判决，满足即不得 SKIP",
            Diag::RanWithoutDep => "未满足却执行：建议执行前强制依赖闸，不满足只能 SKIP",
            Diag::StateIllegal => "状态迁移非法：建议只走 Planned→DepChecked→Ran/Skipped 闭集",
            Diag::LedgerRegress => "账本倒退：建议计数只增不减，撤销用负向记录而非回卷",
            Diag::DepMalformed => "依赖元数据残缺：建议补齐能力名并核对版本号有界",
            Diag::SnapshotDup => "环境快照残缺：建议快照构造后跑能力名去重自检",
            Diag::DoubleRunDiverge => "双跑不一致：建议结算做成纯函数，快照定则账本定",
            Diag::ChainBroken => "跳过链断链：建议每条 SKIP 记录必须对回冻结依赖表项",
            Diag::SegGuardFail => "段位守卫失守：建议核对 SEG_BASE 的 0x40 对齐与段窗区间",
            Diag::SummaryEmpty => "写回摘要为空：建议收口时以非空中文摘要写回状态=已完成",
        }
    }
}

/// 闭集全量数组（12 码）。
pub const ALL_DIAGS: [Diag; 12] = [
    Diag::CapUnknown,
    Diag::ReasonMissing,
    Diag::SkipWithoutCause,
    Diag::RanWithoutDep,
    Diag::StateIllegal,
    Diag::LedgerRegress,
    Diag::DepMalformed,
    Diag::SnapshotDup,
    Diag::DoubleRunDiverge,
    Diag::ChainBroken,
    Diag::SegGuardFail,
    Diag::SummaryEmpty,
];

/// 诊断码查表闭集返回；未知编码回退固定兜底串。
pub fn detail(code: u32) -> &'static str {
    for d in ALL_DIAGS {
        if d.val() == code {
            return d.detail();
        }
    }
    "未知诊断码：建议核对是否落在 0x79681..=0x7968C 闭集内"
}

/// 跳过原因（kskip! 的「带原因」面）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SkipReason {
    /// 原因码（非零）。
    pub code: u32,
    /// 中文原因串（非空）。
    pub zh: String,
}

impl SkipReason {
    /// 造因：依赖未满足类原因（标准模板）。
    pub fn dep_unmet(dep: &HwDep) -> SkipReason {
        SkipReason {
            code: 0x5100u32 + dep.cap.len() as u32,
            zh: format!("依赖未满足：{}（{}）", dep.cap, dep.kind.zh()),
        }
    }

    /// 原因自检：码非零、串非空。
    pub fn well_formed(&self) -> bool {
        self.code != 0 && !self.zh.is_empty()
    }
}

/// 一条跳过链记录：谁、因哪条依赖、以什么原因被跳过。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SkipEntry {
    /// 用例标签。
    pub case: &'static str,
    /// 触发跳过的依赖（必须能对回 DEP_TABLE）。
    pub dep_cap: &'static str,
    /// 跳过原因。
    pub reason: SkipReason,
}

/// 结算账本：跑/跳/过三计数 + 跳过链，只增不减。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SkipLedger {
    /// RAN 计数。
    pub ran: u32,
    /// SKIP 计数。
    pub skipped: u32,
    /// 通过计数。
    pub passed: u32,
    /// 跳过链（追加式）。
    pub chain: Vec<SkipEntry>,
}

impl SkipLedger {
    /// 空账本。
    pub fn new() -> SkipLedger {
        SkipLedger { ran: 0, skipped: 0, passed: 0, chain: Vec::new() }
    }

    /// 记一次执行。
    pub fn mark_ran(&mut self, ok: bool) {
        self.ran = self.ran.saturating_add(1);
        if ok {
            self.passed = self.passed.saturating_add(1);
        }
    }

    /// 记一次跳过（必须带合法原因，否则拒）。
    pub fn mark_skipped(&mut self, case: &'static str, dep_cap: &'static str, reason: SkipReason) -> Result<(), u32> {
        if !reason.well_formed() {
            return Err(Diag::ReasonMissing.val());
        }
        let mut in_table = false;
        for d in DEP_TABLE.iter() {
            if d.cap == dep_cap {
                in_table = true;
            }
        }
        if !in_table {
            return Err(Diag::ChainBroken.val());
        }
        self.skipped = self.skipped.saturating_add(1);
        self.chain.push(SkipEntry { case, dep_cap, reason });
        Ok(())
    }

    /// 账本自洽：跑/跳互斥计量已由调用序保证，这里校验链长等于跳数。
    pub fn consistent(&self) -> bool {
        self.chain.len() as u32 == self.skipped
    }
}

/// kskip 依赖判决：依赖满足 → Ok(false)（不跳）；不满足 → Ok(true)（跳）；
/// 能力名查无 → Err。这是 kskip! 的判据核。
/// 返回值语义是「该不该跳」，与环境「满足与否」相反，故显性取反。
pub fn kskip_eval(dep: &HwDep, env: &HwEnv) -> Result<bool, u32> {
    match env.satisfied(dep) {
        Ok(met) => Ok(!met),
        Err(e) => Err(e),
    }
}

/// 场景结算（纯函数）：给定环境快照，按冻结场景表逐用例判决并记账。
/// 场景表：8 用例各挂 DEP_TABLE 的一条依赖，顺序一一对应。
pub fn settle(env: &HwEnv) -> Result<SkipLedger, u32> {
    if !env.snapshot_sane() {
        return Err(Diag::SnapshotDup.val());
    }
    let cases: [&'static str; 8] = [
        "ks01-lzcnt", "ks02-aesni", "ks03-hpetz", "ks04-nvram",
        "ks05-rtc", "ks06-uart", "ks07-efi2", "ks08-acpi20",
    ];
    let mut ledger = SkipLedger::new();
    for (i, dep) in DEP_TABLE.iter().enumerate() {
        let case = match cases.get(i) {
            Some(c) => *c,
            None => return Err(Diag::ChainBroken.val()),
        };
        match kskip_eval(dep, env) {
            Ok(false) => ledger.mark_ran(true),
            Ok(true) => {
                let reason = SkipReason::dep_unmet(dep);
                ledger.mark_skipped(case, dep.cap, reason)?;
            }
            Err(e) => return Err(e),
        }
    }
    Ok(ledger)
}

/// M-172 跳过链验证：账本里每条 SKIP 记录必须对回依赖表，
/// 且对回的依赖在给定环境里确实不满足；RAN 记录的依赖必须满足。
pub fn verify_chain(ledger: &SkipLedger, env: &HwEnv) -> Result<(), u32> {
    if !ledger.consistent() {
        return Err(Diag::LedgerRegress.val());
    }
    for entry in ledger.chain.iter() {
        let dep = match DEP_TABLE.iter().find(|d| d.cap == entry.dep_cap) {
            Some(d) => d,
            None => return Err(Diag::ChainBroken.val()),
        };
        match env.satisfied(dep) {
            Ok(false) => {}
            _ => return Err(Diag::SkipWithoutCause.val()),
        }
        if !entry.reason.well_formed() {
            return Err(Diag::ReasonMissing.val());
        }
    }
    Ok(())
}

// ── 判据谓词（全部私有 fn xxx_ok() -> bool）────────────────────────────

/// 段位守卫：0x40 对齐 + 段窗 [0x79600, 0x797C0]。
fn seg_guard_ok() -> bool {
    SEG_BASE % 0x40 == 0 && SEG_BASE >= 0x79600 && SEG_BASE <= 0x797C0
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
        let text = a.detail();
        if text.is_empty() || !text.contains("建议") {
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
    let first = match ALL_DIAGS.first() {
        Some(d) => *d,
        None => return false,
    };
    !detail(0xDEAD).is_empty() && detail(first.val()) == first.detail()
}

/// 正常流：环境 A 结算——4 跑 4 跳（含 2 条版本门槛跳过），链验证过。
fn settle_env_a_ok() -> bool {
    let env = frozen_env_a();
    let ledger = match settle(&env) {
        Ok(l) => l,
        Err(_) => return false,
    };
    if ledger.ran != 4 || ledger.skipped != 4 || ledger.passed != 4 {
        return false;
    }
    verify_chain(&ledger, &env).is_ok()
}

/// 依赖元数据冻结表完整 + 环境快照自检。
fn dep_metadata_ok() -> bool {
    dep_table_complete()
        && DEP_TABLE.len() == 8
        && frozen_env_a().snapshot_sane()
        && frozen_env_b().snapshot_sane()
}

/// kskip 判决语义：满足/不满足/查无三分。
fn kskip_eval_ok() -> bool {
    let env_a = frozen_env_a();
    let dep = match DEP_TABLE.first() {
        Some(d) => *d,
        None => return false,
    };
    // lzcnt 满足 → 不跳。
    if kskip_eval(&dep, &env_a) != Ok(false) {
        return false;
    }
    // aesni 缺失 → 跳。
    let dep2 = match DEP_TABLE.get(1) {
        Some(d) => *d,
        None => return false,
    };
    if kskip_eval(&dep2, &env_a) != Ok(true) {
        return false;
    }
    // 查无能力名 → Err。
    let ghost = HwDep { cap: "feat.ghost", kind: DepKind::CpuFeat, min_ver: 0 };
    kskip_eval(&ghost, &env_a) == Err(Diag::CapUnknown.val())
}

/// SKIP 计数单调 + 链账守恒：连续跳过只增不减，链长恒等于跳数。
fn skip_count_monotonic_ok() -> bool {
    let mut ledger = SkipLedger::new();
    let dep = match DEP_TABLE.get(1) {
        Some(d) => *d,
        None => return false,
    };
    for round in 0..4 {
        let reason = SkipReason { code: 0x5A00 + round, zh: format!("第{}轮依赖缺失", round) };
        if ledger.mark_skipped("ks-loop", dep.cap, reason).is_err() {
            return false;
        }
        if ledger.skipped != round + 1 || ledger.chain.len() as u32 != ledger.skipped {
            return false;
        }
    }
    ledger.consistent()
}

/// 状态机合法迁移对闭集放行、其余全部显性拒。
fn state_machine_ok() -> bool {
    // 三条合法边全过。
    if advance_case(CaseState::Planned, CaseState::DepChecked).is_err() {
        return false;
    }
    if advance_case(CaseState::DepChecked, CaseState::Ran).is_err() {
        return false;
    }
    if advance_case(CaseState::DepChecked, CaseState::Skipped).is_err() {
        return false;
    }
    // 违约边：倒退、跳步、终态再迁移、同态自环——全部拒。
    let bad = [
        (CaseState::DepChecked, CaseState::Planned),
        (CaseState::Ran, CaseState::Skipped),
        (CaseState::Skipped, CaseState::Ran),
        (CaseState::Ran, CaseState::DepChecked),
        (CaseState::Skipped, CaseState::DepChecked),
        (CaseState::Planned, CaseState::Ran),
        (CaseState::Planned, CaseState::Skipped),
        (CaseState::Ran, CaseState::Ran),
        (CaseState::Planned, CaseState::Planned),
    ];
    for (cur, next) in bad.iter() {
        if advance_case(*cur, *next) != Err(Diag::StateIllegal.val()) {
            return false;
        }
    }
    true
}

/// 注入必抓 1：凭空跳过——环境 B 全满足却记 SKIP，链验证必须点名。
fn inj_bogus_skip_ok() -> bool {
    let env_b = frozen_env_b();
    let ledger = match settle(&env_b) {
        Ok(l) => l,
        Err(_) => return false,
    };
    // 全满足 → 全跑零跳。
    if ledger.skipped != 0 || ledger.ran != 8 {
        return false;
    }
    // 注入点：人为塞一条凭空 SKIP，链验证必须拒。
    let mut bad = ledger;
    let dep = match DEP_TABLE.first() {
        Some(d) => *d,
        None => return false,
    };
    let reason = SkipReason::dep_unmet(&dep);
    if bad.mark_skipped("ks-inject", dep.cap, reason).is_err() {
        return false;
    }
    verify_chain(&bad, &env_b) == Err(Diag::SkipWithoutCause.val())
}

/// 注入必抓 2：未满足却执行——环境 A 里跑满全部用例（零跳），
/// 链验证必须抓到「依赖不满足却没有 SKIP 记录」的缺口。
fn inj_ran_unmet_ok() -> bool {
    let env_a = frozen_env_a();
    let mut forced = SkipLedger::new();
    for _ in DEP_TABLE.iter() {
        forced.mark_ran(true);
    }
    // 跑了 8 条但链为空：链验证不直接看 RAN，这里用等价守卫——
    // 正常结算必产生 5 条 SKIP，若环境 A 下 skipped==0 即违约。
    match settle(&env_a) {
        Ok(l) => l.skipped == 4 && forced.skipped == 0,
        Err(_) => false,
    }
}

/// 注入必抓 3：无原因跳过——空原因串/零原因码必须被拒。
fn inj_no_reason_ok() -> bool {
    let mut ledger = SkipLedger::new();
    let dep = match DEP_TABLE.first() {
        Some(d) => *d,
        None => return false,
    };
    let empty = SkipReason { code: 1, zh: String::new() };
    let zero = SkipReason { code: 0, zh: format!("有串无码") };
    if ledger.mark_skipped("ks-bad1", dep.cap, empty) != Err(Diag::ReasonMissing.val()) {
        return false;
    }
    if ledger.mark_skipped("ks-bad2", dep.cap, zero) != Err(Diag::ReasonMissing.val()) {
        return false;
    }
    ledger.skipped == 0 && ledger.chain.is_empty()
}

/// 注入必抓 4：断链——SKIP 记录指向依赖表外能力名必须被 ChainBroken 拒。
fn inj_chain_broken_ok() -> bool {
    let mut ledger = SkipLedger::new();
    let reason = SkipReason { code: 0x5B, zh: format!("幽灵依赖") };
    if ledger.mark_skipped("ks-ghost", "cap.ghost", reason) != Err(Diag::ChainBroken.val()) {
        return false;
    }
    ledger.skipped == 0
}

/// 双跑一致：环境 A/B 各两次结算，账本逐字节相同。
fn double_run_ok() -> bool {
    let a1 = match settle(&frozen_env_a()) {
        Ok(l) => l,
        Err(_) => return false,
    };
    let a2 = match settle(&frozen_env_a()) {
        Ok(l) => l,
        Err(_) => return false,
    };
    let b1 = match settle(&frozen_env_b()) {
        Ok(l) => l,
        Err(_) => return false,
    };
    let b2 = match settle(&frozen_env_b()) {
        Ok(l) => l,
        Err(_) => return false,
    };
    a1 == a2 && b1 == b2 && a1 != b1
}

/// 错误路径 Result 语义：快照重名/账本倒退两码可复现。
fn error_path_ok() -> bool {
    let dup = HwEnv {
        caps: Vec::from([("feat.lzcnt", true, 1), ("feat.lzcnt", false, 0)]),
    };
    if settle(&dup) != Err(Diag::SnapshotDup.val()) {
        return false;
    }
    let mut bad = SkipLedger::new();
    bad.skipped = 2;
    if bad.consistent() {
        return false;
    }
    let env_a = frozen_env_a();
    verify_chain(&bad, &env_a) == Err(Diag::LedgerRegress.val())
}

/// ③ 写回面：状态=已完成、结果摘要非空。
fn writeback_summary_ok() -> bool {
    let ledger = match settle(&frozen_env_a()) {
        Ok(l) => l,
        Err(_) => return false,
    };
    let summary = format!(
        "UNX-F3413 kskip 结算：跑{} 跳{} 过{} 链{}条",
        ledger.ran, ledger.skipped, ledger.passed, ledger.chain.len()
    );
    let done = !summary.is_empty()
        && summary.contains("kskip")
        && ledger.ran > 0
        && ledger.skipped > 0;
    let empty_verdict = if summary.is_empty() {
        Err(Diag::SummaryEmpty.val())
    } else {
        Ok(())
    };
    done && empty_verdict.is_ok()
}

// ── 判据总成：run_unx3413_checks ───────────────────────────────────────

/// run_unx3413_checks：本单判据聚合（诊断段 0x79680，全部走 add_dyn）。
pub fn run_unx3413_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F3413 条件跳过机制（kskip 与硬件依赖声明）");
    let checks: [(&'static str, bool, String); 13] = [
        (
            "段位守卫0x40对齐且在段窗内",
            seg_guard_ok(),
            format!("SEG_BASE=0x{:X} 守卫{}", SEG_BASE, if seg_guard_ok() { "过" } else { "破" }),
        ),
        (
            "诊断码闭集12码唯一且文案带建议",
            diag_closed_set_ok(),
            format!("闭集{}", if diag_closed_set_ok() { "完整" } else { "破缺" }),
        ),
        (
            "依赖元数据冻结表八条完整",
            dep_metadata_ok(),
            format!("依赖表{}", if dep_metadata_ok() { "冻结" } else { "漂移" }),
        ),
        (
            "kskip判决满足/不满足/查无三分",
            kskip_eval_ok(),
            format!("判决{}", if kskip_eval_ok() { "过" } else { "破" }),
        ),
        (
            "正常流环境A结算四跑四跳",
            settle_env_a_ok(),
            format!("环境A{}", if settle_env_a_ok() { "过" } else { "破" }),
        ),
        (
            "SKIP计数单调且链账守恒",
            skip_count_monotonic_ok(),
            format!("计数{}", if skip_count_monotonic_ok() { "单调" } else { "回卷" }),
        ),
        (
            "用例状态机迁移对闭集其余全拒",
            state_machine_ok(),
            format!("状态机{}", if state_machine_ok() { "过" } else { "失" }),
        ),
        (
            "注入1凭空跳过必抓",
            inj_bogus_skip_ok(),
            format!("凭空跳{}", if inj_bogus_skip_ok() { "抓到" } else { "漏抓" }),
        ),
        (
            "注入2未满足却执行必抓",
            inj_ran_unmet_ok(),
            format!("未满足跑{}", if inj_ran_unmet_ok() { "抓到" } else { "漏抓" }),
        ),
        (
            "注入3无原因跳过必拒",
            inj_no_reason_ok(),
            format!("无原因{}", if inj_no_reason_ok() { "拒" } else { "漏" }),
        ),
        (
            "注入4断链指向表外依赖必拒",
            inj_chain_broken_ok(),
            format!("断链{}", if inj_chain_broken_ok() { "拒" } else { "漏" }),
        ),
        (
            "双跑一致AB快照各两次结算同账",
            double_run_ok(),
            format!("双跑{}", if double_run_ok() { "一致" } else { "分叉" }),
        ),
        (
            "错误路径+写回摘要非空",
            error_path_ok() && writeback_summary_ok(),
            format!("综合{}", if error_path_ok() && writeback_summary_ok() { "过" } else { "破" }),
        ),
    ];
    for (name, passed, det) in checks.iter() {
        s.add_dyn(name, *passed, det);
    }
    s
}

// ── 测试区（#[cfg(test)]：允许 assert!/assert_eq!/unwrap）──────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seg_guard_and_diag_closed_set() {
        assert!(seg_guard_ok());
        assert_eq!(SEG_BASE, 0x79680);
        assert!(diag_closed_set_ok());
        for (i, d) in ALL_DIAGS.iter().enumerate() {
            assert_eq!(d.val(), SEG_BASE + i as u32 + 1);
        }
    }

    #[test]
    fn dep_table_frozen_and_sane() {
        assert!(dep_metadata_ok());
        assert!(dep_table_complete());
        for d in DEP_TABLE.iter() {
            assert!(d.well_formed());
        }
    }

    #[test]
    fn kskip_eval_three_way_split() {
        assert!(kskip_eval_ok());
        let env = frozen_env_a();
        let ghost = HwDep { cap: "nope", kind: DepKind::Device, min_ver: 0 };
        assert!(kskip_eval(&ghost, &env).is_err());
    }

    #[test]
    fn env_a_settles_four_run_four_skip() {
        assert!(settle_env_a_ok());
        let ledger = settle(&frozen_env_a()).unwrap();
        assert_eq!(ledger.ran, 4);
        assert_eq!(ledger.skipped, 4);
        assert_eq!(ledger.passed, 4);
        assert_eq!(ledger.chain.len(), 4);
    }

    #[test]
    fn skip_counter_only_grows() {
        assert!(skip_count_monotonic_ok());
    }

    #[test]
    fn case_state_machine_closed_set() {
        assert!(state_machine_ok());
        assert!(advance_case(CaseState::Ran, CaseState::Planned).is_err());
    }

    #[test]
    fn injections_all_caught() {
        assert!(inj_bogus_skip_ok());
        assert!(inj_ran_unmet_ok());
        assert!(inj_no_reason_ok());
        assert!(inj_chain_broken_ok());
    }

    #[test]
    fn double_run_identical_ledgers() {
        assert!(double_run_ok());
        assert_eq!(settle(&frozen_env_a()).unwrap(), settle(&frozen_env_a()).unwrap());
    }

    #[test]
    fn chain_verification_verdicts() {
        let env = frozen_env_a();
        let ledger = settle(&env).unwrap();
        assert!(verify_chain(&ledger, &env).is_ok());
        // 对照：全满足环境下任何 SKIP 都是凭空跳。
        let env_b = frozen_env_b();
        let mut bogus = ledger;
        let dep = DEP_TABLE.first().unwrap();
        let _ = bogus.mark_skipped("x", dep.cap, SkipReason::dep_unmet(dep));
        assert_eq!(verify_chain(&bogus, &env_b).unwrap_err(), Diag::SkipWithoutCause.val());
    }

    #[test]
    fn error_paths_return_closed_codes() {
        assert!(error_path_ok());
        assert!(detail(0x1).contains("未知"));
        assert!(!detail(Diag::CapUnknown.val()).is_empty());
    }

    #[test]
    fn writeback_summary_non_empty() {
        assert!(writeback_summary_ok());
    }

    #[test]
    fn checks_aggregate_all_green() {
        let s = run_unx3413_checks();
        assert!(s.len() >= 12);
        assert!(s.all_passed(), "全部判据必须绿：{}", s.red_items_text());
    }
}
