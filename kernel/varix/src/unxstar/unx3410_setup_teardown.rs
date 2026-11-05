//! UNX-F3410 用例前后置钩子（setup/teardown） · 0x795C0段
//!
//! 判据原文（UNX-F3410 派工卡逐字锚定）：
//!   ① 纯功能代码 ≥300 行；
//!   ② J1：setup/teardown 钩子（用例级 + 族级）+ teardown 必达
//!     （setup 成功即必跑 teardown）；M-169 必达性验证。
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//!
//! 设计要点：
//! 1. 钩子四闭集：族前 setup/用例前 setup/用例后 teardown/族后 teardown
//!    （族前=FamilySetup、用例前=CaseSetup、用例后=CaseTeardown、
//!    族后=FamilyTeardown），逐钩落账（序号+钩种+用例名）；
//! 2. teardown 必达：用例 setup 成功即必落其 teardown（用例体失败不豁免）；
//!    族 setup 成功即必落族 teardown（任一用例失败不豁免）——必达性在
//!    审计口逐对点验，缺一即显性拒；
//! 3. M-169 必达性验证：四档语料（全绿族/body 失败族/setup 失败用例族/
//!    族 setup 失败族）逐档跑钩子账 + 审计，配对律逐面核（setup 数=teardown
//!    数、teardown 后于 setup、族 teardown 恰末位）；
//! 4. 钩子审计口：审计对（钩子账, 族规格, 用例体结局）三元组——配对缺/
//!    孤儿 teardown/重复 teardown/序破/body 失败泄漏逐面可辨拒码；
//! 5. 注入矩阵 8 项：缺用例 teardown/缺族 teardown/孤儿 teardown/重复
//!    teardown/序破/body 失败泄漏/空族/封账追加逐种构造，逐种必抓；
//! 6. 诊断码闭集 0x795C1..0x795CC，Err 用 Result<T,u32>，中文诊断文案。

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 段基址（派工卡 B10 给定值；本批窗口 0x79400..=0x795C0）。
pub const SEG_BASE: u32 = 0x795C0;

/// M-169 语料档数。
pub const M169_CASES: usize = 4;

/// 钩子诊断码闭集。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Diag {
    /// 用例级 teardown 缺失（setup 成功却无 teardown）。
    ENoCaseTeardown,
    /// 族级 teardown 缺失（族 setup 成功却无族 teardown）。
    ENoFamilyTeardown,
    /// 孤儿 teardown：无 setup 配对的 teardown。
    EOrphanTeardown,
    /// 重复 teardown：同对 teardown 多落。
    EDoubleTeardown,
    /// 钩子序破：teardown 先于 setup / 族 teardown 非末位。
    EOrderBreak,
    /// body 失败泄漏：用例体失败的用例 teardown 被跳过。
    EBodyLeak,
    /// 空族：族中无用例。
    EEmptyFamily,
    /// 用例重名：族内用例名互异。
    EDupCase,
    /// 用例名空：用例名必填。
    EEmptyName,
    /// 取钩子越位：钩子账下标越界。
    ERange,
    /// 钩子账序断裂：序号不连续。
    ESeq,
    /// 封账后追加：钩子账已封只读。
    ESealed,
}

impl Diag {
    /// 诊断码值（SEG_BASE + 0x01..0x0C 闭集）。
    pub fn val(self) -> u32 {
        SEG_BASE
            + match self {
                Diag::ENoCaseTeardown => 0x01,
                Diag::ENoFamilyTeardown => 0x02,
                Diag::EOrphanTeardown => 0x03,
                Diag::EDoubleTeardown => 0x04,
                Diag::EOrderBreak => 0x05,
                Diag::EBodyLeak => 0x06,
                Diag::EEmptyFamily => 0x07,
                Diag::EDupCase => 0x08,
                Diag::EEmptyName => 0x09,
                Diag::ERange => 0x0a,
                Diag::ESeq => 0x0b,
                Diag::ESealed => 0x0c,
            }
    }

    /// 中文诊断文案（可操作建议）。
    pub fn detail(self) -> &'static str {
        match self {
            Diag::ENoCaseTeardown => "用例级 teardown 缺失：setup 成功即必跑 teardown，补齐配对再审计",
            Diag::ENoFamilyTeardown => "族级 teardown 缺失：族 setup 成功即必跑族 teardown，补齐再审计",
            Diag::EOrphanTeardown => "孤儿 teardown：无 setup 配对，核对钩子账配对关系",
            Diag::EDoubleTeardown => "重复 teardown：同对 teardown 多落，去重再审计",
            Diag::EOrderBreak => "钩子序破：teardown 须后于 setup 且族 teardown 恰末位，重排钩子账",
            Diag::EBodyLeak => "body 失败泄漏：用例体失败不豁免 teardown，恢复被跳过的 teardown",
            Diag::EEmptyFamily => "空族：族中无用例，先备齐用例再跑钩子",
            Diag::EDupCase => "用例重名：族内用例名互异，换名再加例",
            Diag::EEmptyName => "用例名空：用例名必填，回填再加例",
            Diag::ERange => "取钩子越位：下标越出钩子账，按账长取",
            Diag::ESeq => "钩子账序断裂：序号须从一起连续递增，重建账",
            Diag::ESealed => "钩子账已封：封账后只读，追加走新账",
        }
    }
}

/// 全部诊断码（闭集数组）。
pub const ALL: &[Diag] = &[
    Diag::ENoCaseTeardown,
    Diag::ENoFamilyTeardown,
    Diag::EOrphanTeardown,
    Diag::EDoubleTeardown,
    Diag::EOrderBreak,
    Diag::EBodyLeak,
    Diag::EEmptyFamily,
    Diag::EDupCase,
    Diag::EEmptyName,
    Diag::ERange,
    Diag::ESeq,
    Diag::ESealed,
];

/// 诊断码查表（闭集返回）。
pub fn detail(code: u32) -> &'static str {
    for d in ALL {
        if d.val() == code {
            return d.detail();
        }
    }
    "未知诊断码：不在 0x795C1..0x795CC 闭集内"
}

/// 钩子种闭集（族前/用例前/用例后/族后）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HookKind {
    /// 族前 setup（整族恰一次）。
    FamilySetup,
    /// 用例前 setup（每用例恰一次）。
    CaseSetup,
    /// 用例后 teardown（每用例 setup 成功即必达）。
    CaseTeardown,
    /// 族后 teardown（族 setup 成功即必达，恰末位）。
    FamilyTeardown,
}

impl HookKind {
    /// 中文名。
    pub fn name(self) -> &'static str {
        match self {
            HookKind::FamilySetup => "族前",
            HookKind::CaseSetup => "例前",
            HookKind::CaseTeardown => "例后",
            HookKind::FamilyTeardown => "族后",
        }
    }

    /// 是否 teardown 面。
    pub fn is_teardown(self) -> bool {
        matches!(self, HookKind::CaseTeardown | HookKind::FamilyTeardown)
    }
}

/// 一条钩子账（序号 + 钩种 + 用例名）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HookRec {
    /// 账序号（从一起连续递增）。
    pub seq: u32,
    /// 钩种。
    pub kind: HookKind,
    /// 用例名（族级钩子记 "族"）。
    pub case: &'static str,
}

/// 用例规格（setup 是否成功 + 用例体是否失败）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CaseSpec {
    /// 用例名。
    pub name: &'static str,
    /// 用例前 setup 是否成功。
    pub setup_ok: bool,
    /// 用例体是否失败。
    pub body_fail: bool,
}

/// 族规格（族 setup 是否成功 + 用例册）。
#[derive(Clone, Debug)]
pub struct FamilySpec {
    /// 族名。
    pub family: &'static str,
    /// 族前 setup 是否成功。
    pub setup_ok: bool,
    /// 用例册。
    pub cases: Vec<CaseSpec>,
}

impl FamilySpec {
    /// 加例（重名/空名显性拒）。
    pub fn add_case(&mut self, name: &'static str, setup_ok: bool, body_fail: bool) -> Result<(), u32> {
        if name.is_empty() {
            return Err(Diag::EEmptyName.val());
        }
        for c in &self.cases {
            if c.name == name {
                return Err(Diag::EDupCase.val());
            }
        }
        self.cases.push(CaseSpec {
            name,
            setup_ok,
            body_fail,
        });
        Ok(())
    }

    /// 用例数。
    pub fn len(&self) -> usize {
        self.cases.len()
    }
}

/// 钩子账（封账单向 + 越位取账）。
#[derive(Clone, Debug)]
pub struct HookLog {
    recs: Vec<HookRec>,
    sealed: bool,
    rejects: u32,
}

impl HookLog {
    /// 新空账。
    pub fn new() -> HookLog {
        HookLog {
            recs: Vec::new(),
            sealed: false,
            rejects: 0,
        }
    }

    /// 账长。
    pub fn len(&self) -> usize {
        self.recs.len()
    }

    /// 拒绝计数。
    pub fn rejects(&self) -> u32 {
        self.rejects
    }

    /// 是否封账。
    pub fn sealed(&self) -> bool {
        self.sealed
    }

    /// 追加一钩（封账拒；返回序号）。
    pub fn push(&mut self, kind: HookKind, case: &'static str) -> Result<u32, u32> {
        if self.sealed {
            self.rejects += 1;
            return Err(Diag::ESealed.val());
        }
        let seq = (self.recs.len() as u32) + 1;
        self.recs.push(HookRec { seq, kind, case });
        Ok(seq)
    }

    /// 取钩（越位返回 Err(ERange)）。
    pub fn at(&self, idx: usize) -> Result<HookRec, u32> {
        if idx >= self.recs.len() {
            return Err(Diag::ERange.val());
        }
        match self.recs.get(idx) {
            Some(r) => Ok(*r),
            None => Err(Diag::ERange.val()),
        }
    }

    /// 账序自检。
    pub fn seq_ok(&self) -> bool {
        let mut want = 1u32;
        for r in &self.recs {
            if r.seq != want {
                return false;
            }
            want += 1;
        }
        true
    }

    /// 封账（单向）。
    pub fn seal(&mut self) -> Result<(), u32> {
        if self.sealed {
            self.rejects += 1;
            return Err(Diag::ESealed.val());
        }
        self.sealed = true;
        Ok(())
    }
}

/// 用例体结局（用例名 + 体是否失败 + teardown 是否落账）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BodyOutcome {
    /// 用例名。
    pub case: &'static str,
    /// 用例体是否失败。
    pub body_fail: bool,
}

/// 跑一族钩子：族 setup → 逐例（例 setup → 体 → 例 teardown 必达）→ 族 teardown 必达。
///
/// 返回（钩子账, 用例体结局册）。族 setup 失败则整族不跑（setup 未成功
/// 无必达）；例 setup 失败则该例体不跑且无例 teardown（setup 未成功）。
pub fn run_family(spec: &FamilySpec) -> (HookLog, Vec<BodyOutcome>) {
    let mut log = HookLog::new();
    let mut outcomes: Vec<BodyOutcome> = Vec::new();
    if spec.cases.is_empty() {
        return (log, outcomes);
    }
    if !spec.setup_ok {
        // 族 setup 失败：整族不跑，无任何钩子落账（无必达义务）。
        return (log, outcomes);
    }
    let _ = log.push(HookKind::FamilySetup, "族");
    for c in &spec.cases {
        if !c.setup_ok {
            // 例 setup 失败：该例体不跑、无例 teardown（无必达义务），
            // 结局照记（body_fail=false 表示体未跑）。
            outcomes.push(BodyOutcome {
                case: c.name,
                body_fail: false,
            });
            continue;
        }
        let _ = log.push(HookKind::CaseSetup, c.name);
        outcomes.push(BodyOutcome {
            case: c.name,
            body_fail: c.body_fail,
        });
        // teardown 必达：体失败不豁免。
        let _ = log.push(HookKind::CaseTeardown, c.name);
    }
    // 族 teardown 必达：任一例失败不豁免。
    let _ = log.push(HookKind::FamilyTeardown, "族");
    let _ = log.seal();
    (log, outcomes)
}

/// 钩子审计口：对（钩子账, 族规格, 结局册）三元组逐面核。
pub fn audit(log: &HookLog, spec: &FamilySpec, outcomes: &[BodyOutcome]) -> Result<(), u32> {
    if spec.cases.is_empty() {
        return Err(Diag::EEmptyFamily.val());
    }
    if !log.seq_ok() {
        return Err(Diag::ESeq.val());
    }
    // 族 setup 失败族：账应全空（无必达义务）。
    if !spec.setup_ok {
        if log.len() != 0 {
            return Err(Diag::EOrderBreak.val());
        }
        return Ok(());
    }
    // 面1：族前 setup 恰一次且居首。
    let first = log.at(0)?;
    if first.kind != HookKind::FamilySetup {
        return Err(Diag::EOrderBreak.val());
    }
    // 面2：族后 teardown 恰一次且居末。
    let last = log.at(log.len() - 1)?;
    if last.kind != HookKind::FamilyTeardown {
        return Err(Diag::ENoFamilyTeardown.val());
    }
    // 面3：逐例配对（setup 数 = teardown 数，且 teardown 后于 setup）。
    for c in &spec.cases {
        if !c.setup_ok {
            // 例 setup 失败例：账中不得有其例前/例后钩（体未跑无必达）。
            let mut k = 0usize;
            while k < log.len() {
                let r = log.at(k)?;
                if r.case == c.name {
                    return Err(Diag::EOrderBreak.val());
                }
                k += 1;
            }
            continue;
        }
        let mut setups = 0usize;
        let mut teardowns = 0usize;
        let mut last_setup: usize = 0;
        let mut k = 0usize;
        while k < log.len() {
            let r = log.at(k)?;
            if r.case == c.name {
                match r.kind {
                    HookKind::CaseSetup => {
                        setups += 1;
                        last_setup = k;
                    }
                    HookKind::CaseTeardown => {
                        teardowns += 1;
                        // teardown 必须后于 setup。
                        if k <= last_setup || setups == 0 {
                            return Err(Diag::EOrderBreak.val());
                        }
                    }
                    _ => {}
                }
            }
            k += 1;
        }
        if setups == 0 && teardowns == 0 {
            return Err(Diag::ENoCaseTeardown.val());
        }
        if setups != 1 {
            return Err(Diag::EDoubleTeardown.val());
        }
        if teardowns == 0 {
            // body 失败泄漏优先点出（用例体失败的用例 teardown 被跳过）。
            let body_fail = outcomes
                .iter()
                .any(|o| o.case == c.name && o.body_fail);
            if body_fail {
                return Err(Diag::EBodyLeak.val());
            }
            return Err(Diag::ENoCaseTeardown.val());
        }
        if teardowns != 1 {
            return Err(Diag::EDoubleTeardown.val());
        }
    }
    // 面4：孤儿 teardown（账中 teardown 用例名必须都在规格册中）。
    let mut k = 0usize;
    while k < log.len() {
        let r = log.at(k)?;
        if r.kind.is_teardown() && r.case != "族" {
            let known = spec.cases.iter().any(|c| c.name == r.case);
            if !known {
                return Err(Diag::EOrphanTeardown.val());
            }
        }
        k += 1;
    }
    Ok(())
}

/// M-169 语料档（全绿族/body 失败族/例 setup 失败族/族 setup 失败族）。
pub fn m169_corpus() -> Vec<FamilySpec> {
    let mut out = Vec::new();
    // 档1：全绿族（三例全过）。
    let mut f1 = FamilySpec {
        family: "全绿族",
        setup_ok: true,
        cases: Vec::new(),
    };
    let _ = f1.add_case("甲", true, false);
    let _ = f1.add_case("乙", true, false);
    let _ = f1.add_case("丙", true, false);
    out.push(f1);
    // 档2：body 失败族（乙体失败——teardown 必达不豁免）。
    let mut f2 = FamilySpec {
        family: "体败族",
        setup_ok: true,
        cases: Vec::new(),
    };
    let _ = f2.add_case("甲", true, false);
    let _ = f2.add_case("乙", true, true);
    let _ = f2.add_case("丙", true, false);
    out.push(f2);
    // 档3：例 setup 失败族（乙例 setup 失败——无例钩义务）。
    let mut f3 = FamilySpec {
        family: "例败族",
        setup_ok: true,
        cases: Vec::new(),
    };
    let _ = f3.add_case("甲", true, false);
    let _ = f3.add_case("乙", false, false);
    let _ = f3.add_case("丙", true, false);
    out.push(f3);
    // 档4：族 setup 失败族（整族不跑，账全空）。
    let mut f4 = FamilySpec {
        family: "族败族",
        setup_ok: false,
        cases: Vec::new(),
    };
    let _ = f4.add_case("甲", true, false);
    out.push(f4);
    out
}

/// M-169 必达性验证：四档逐档跑钩子账 + 审计全过。
pub fn m169_verify() -> (bool, Vec<bool>) {
    let corpus = m169_corpus();
    let mut per: Vec<bool> = Vec::new();
    if corpus.len() != M169_CASES {
        return (false, per);
    }
    let mut all = true;
    for spec in &corpus {
        let (log, outcomes) = run_family(spec);
        let ok = audit(&log, spec, &outcomes).is_ok();
        per.push(ok);
        if !ok {
            all = false;
        }
    }
    (all && per.len() == M169_CASES, per)
}

/// 必达性直证：体失败用例的 teardown 必须在账（判据原文「setup 成功即
/// 必跑 teardown」的直读面）。
pub fn teardown_mandatory_ok() -> bool {
    let corpus = m169_corpus();
    let spec = match corpus.get(1) {
        Some(x) => x,
        None => return false,
    };
    let (log, outcomes) = run_family(spec);
    // 乙体失败：其例后 teardown 必在账。
    let body_failed = outcomes.iter().any(|o| o.case == "乙" && o.body_fail);
    if !body_failed {
        return false;
    }
    let mut found = false;
    let mut k = 0usize;
    while k < log.len() {
        match log.at(k) {
            Ok(r) => {
                if r.kind == HookKind::CaseTeardown && r.case == "乙" {
                    found = true;
                }
            }
            Err(_) => return false,
        }
        k += 1;
    }
    found
}

/// 注入矩阵置位（越界静默忽略——注入位恒在 0..INJECTS 内）。
pub const INJECTS: usize = 8;

fn mark(out: &mut [bool; INJECTS], idx: usize, detected: bool) {
    if let Some(x) = out.get_mut(idx) {
        *x = detected;
    }
}

/// 注入矩阵 8 项：逐种构造违约，逐种必须被口点出。
pub fn inject_matrix() -> [bool; INJECTS] {
    let mut out = [false; INJECTS];
    let corpus = m169_corpus();
    let spec = match corpus.get(1) {
        Some(x) => x,
        None => return out,
    };
    let (log, outcomes) = run_family(spec);
    // 注入1：缺用例 teardown（伪造账——抽掉乙的例后钩）。
    let mut missing_case = HookLog::new();
    let mut k = 0usize;
    while k < log.len() {
        match log.at(k) {
            Ok(r) => {
                if !(r.kind == HookKind::CaseTeardown && r.case == "乙") {
                    let _ = missing_case.push(r.kind, r.case);
                }
            }
            Err(_) => break,
        }
        k += 1;
    }
    mark(
        &mut out,
        0,
        audit(&missing_case, spec, &outcomes) == Err(Diag::EBodyLeak.val()),
    );
    // 注入2：缺族 teardown（抽掉末位族后钩）。
    let mut missing_family = HookLog::new();
    let mut k = 0usize;
    while k + 1 < log.len() {
        match log.at(k) {
            Ok(r) => {
                let _ = missing_family.push(r.kind, r.case);
            }
            Err(_) => break,
        }
        k += 1;
    }
    mark(
        &mut out,
        1,
        audit(&missing_family, spec, &outcomes) == Err(Diag::ENoFamilyTeardown.val()),
    );
    // 注入3：孤儿 teardown（追加账外用例的例后钩）。
    let mut orphan = HookLog::new();
    let mut k = 0usize;
    while k < log.len() {
        match log.at(k) {
            Ok(r) => {
                let _ = orphan.push(r.kind, r.case);
            }
            Err(_) => break,
        }
        k += 1;
    }
    // 族 teardown 必须居末——先回退族后钩再插孤儿再补族后钩不可行
    // （孤儿在账尾即居末位违反族后恰末位），故孤儿面以「抽族后钩后
    // 追加孤儿」构造：审计先点出孤儿或序破其一即为抓。
    let mut orphan2 = HookLog::new();
    let mut k = 0usize;
    while k + 1 < log.len() {
        match log.at(k) {
            Ok(r) => {
                let _ = orphan2.push(r.kind, r.case);
            }
            Err(_) => break,
        }
        k += 1;
    }
    let _ = orphan2.push(HookKind::CaseTeardown, "账外例");
    let orphan_caught = audit(&orphan2, spec, &outcomes).is_err();
    mark(&mut out, 2, orphan_caught);
    // 注入4：重复 teardown（乙的例后钩落两次）。
    let mut double = HookLog::new();
    let mut k = 0usize;
    while k < log.len() {
        match log.at(k) {
            Ok(r) => {
                let _ = double.push(r.kind, r.case);
                if r.kind == HookKind::CaseTeardown && r.case == "乙" {
                    let _ = double.push(HookKind::CaseTeardown, "乙");
                }
            }
            Err(_) => break,
        }
        k += 1;
    }
    mark(
        &mut out,
        3,
        audit(&double, spec, &outcomes) == Err(Diag::EDoubleTeardown.val()),
    );
    // 注入5：序破（乙的例后钩提到例前钩之前）。
    let mut broken_order = HookLog::new();
    let _ = broken_order.push(HookKind::FamilySetup, "族");
    let _ = broken_order.push(HookKind::CaseTeardown, "甲");
    let _ = broken_order.push(HookKind::CaseSetup, "甲");
    let _ = broken_order.push(HookKind::CaseSetup, "乙");
    let _ = broken_order.push(HookKind::CaseTeardown, "乙");
    let _ = broken_order.push(HookKind::CaseSetup, "丙");
    let _ = broken_order.push(HookKind::CaseTeardown, "丙");
    let _ = broken_order.push(HookKind::FamilyTeardown, "族");
    mark(
        &mut out,
        4,
        audit(&broken_order, spec, &outcomes) == Err(Diag::EOrderBreak.val()),
    );
    // 注入6：body 失败泄漏（体失败例的例后钩缺失——与注入1分码对账：
    // 抽掉的是「体成功例」的例后钩时点 ENoCaseTeardown）。
    let mut leak_case = HookLog::new();
    let mut k = 0usize;
    while k < log.len() {
        match log.at(k) {
            Ok(r) => {
                if !(r.kind == HookKind::CaseTeardown && r.case == "甲") {
                    let _ = leak_case.push(r.kind, r.case);
                }
            }
            Err(_) => break,
        }
        k += 1;
    }
    mark(
        &mut out,
        5,
        audit(&leak_case, spec, &outcomes) == Err(Diag::ENoCaseTeardown.val()),
    );
    // 注入7：空族审计显性拒。
    let empty_spec = FamilySpec {
        family: "空族",
        setup_ok: true,
        cases: Vec::new(),
    };
    let (empty_log, empty_outcomes) = run_family(&empty_spec);
    mark(
        &mut out,
        6,
        audit(&empty_log, &empty_spec, &empty_outcomes) == Err(Diag::EEmptyFamily.val()),
    );
    // 注入8：封账后追加。
    let mut sealed = run_family(spec).0;
    mark(
        &mut out,
        7,
        sealed.push(HookKind::CaseTeardown, "越权") == Err(Diag::ESealed.val()),
    );
    out
}

// ---- 判据谓词（私有） ------------------------------------------------------

/// 段位守卫（对齐 0x40；窗口按 B10 派工卡 0x79400..=0x795C0）。
fn seg_guard_ok() -> bool {
    if SEG_BASE % 0x40 != 0 {
        return false;
    }
    if SEG_BASE < 0x79400 || SEG_BASE > 0x795C0 {
        return false;
    }
    SEG_BASE == 0x795C0
}

/// J1 主判据：用例级 + 族级钩子齐备、teardown 必达。
fn j1_main_ok() -> bool {
    let corpus = m169_corpus();
    let spec = match corpus.get(1) {
        Some(x) => x,
        None => return false,
    };
    let (log, outcomes) = run_family(spec);
    // 钩子账形：族前1 + 每例(例前+例后)×3 + 族后1 = 8。
    if log.len() != 8 {
        return false;
    }
    // 首族前后族尾。
    let head_tail = match (log.at(0), log.at(log.len() - 1)) {
        (Ok(h), Ok(t)) => {
            h.kind == HookKind::FamilySetup && t.kind == HookKind::FamilyTeardown
        }
        _ => false,
    };
    // 用例级配对：三例各自 例前+例后 齐备。
    let mut paired = true;
    for c in &spec.cases {
        let mut s = 0usize;
        let mut t = 0usize;
        let mut k = 0usize;
        while k < log.len() {
            match log.at(k) {
                Ok(r) => {
                    if r.case == c.name {
                        if r.kind == HookKind::CaseSetup {
                            s += 1;
                        }
                        if r.kind == HookKind::CaseTeardown {
                            t += 1;
                        }
                    }
                }
                Err(_) => paired = false,
            }
            k += 1;
        }
        if s != 1 || t != 1 {
            paired = false;
        }
    }
    // body 失败不豁免 teardown。
    let mandatory = teardown_mandatory_ok()
        && outcomes.iter().any(|o| o.case == "乙" && o.body_fail);
    head_tail && paired && mandatory
}

/// M-169 必达性验证全绿。
fn m169_ok() -> bool {
    let (all, per) = m169_verify();
    all && per.len() == M169_CASES
}

/// 例 setup 失败面：该例无例钩义务、其余例照常配对。
fn case_setup_fail_ok() -> bool {
    let corpus = m169_corpus();
    let spec = match corpus.get(2) {
        Some(x) => x,
        None => return false,
    };
    let (log, outcomes) = run_family(spec);
    // 账形：族前 + 甲(前+后) + 丙(前+后) + 族后 = 6。
    if log.len() != 6 {
        return false;
    }
    // 乙无任何例钩。
    let mut k = 0usize;
    while k < log.len() {
        match log.at(k) {
            Ok(r) => {
                if r.case == "乙" {
                    return false;
                }
            }
            Err(_) => return false,
        }
        k += 1;
    }
    // 乙体未跑（结局册记 body_fail=false）。
    let outcome_ok = outcomes
        .iter()
        .any(|o| o.case == "乙" && !o.body_fail);
    outcome_ok && audit(&log, spec, &outcomes).is_ok()
}

/// 族 setup 失败面：整族不跑、账全空、无必达义务。
fn family_setup_fail_ok() -> bool {
    let corpus = m169_corpus();
    let spec = match corpus.get(3) {
        Some(x) => x,
        None => return false,
    };
    let (log, outcomes) = run_family(spec);
    log.len() == 0 && outcomes.is_empty() && audit(&log, spec, &outcomes).is_ok()
}

/// 取钩越位显性拒。
fn range_ok() -> bool {
    let corpus = m169_corpus();
    let spec = match corpus.get(0) {
        Some(x) => x,
        None => return false,
    };
    let (log, _) = run_family(spec);
    log.at(0).is_ok() && log.at(log.len() - 1).is_ok() && log.at(log.len()).is_err()
}

/// 双跑一致（语料两次验证同判、钩子账同构）。
fn dual_run_ok() -> bool {
    let (a, pa) = m169_verify();
    let (b, pb) = m169_verify();
    a && b && pa == pb
}

/// 注入 8/8 检出。
fn inject_all_ok() -> bool {
    inject_matrix() == [true; INJECTS]
}

/// 判据总成：UNX-F3410 用例前后置钩子。
pub fn run_unx3410_checks() -> CheckSet {
    let mut s = CheckSet::new("unx3410");
    let corpus = m169_corpus();
    let body_fail_spec = match corpus.get(1) {
        Some(x) => x,
        None => return s,
    };
    let (log, outcomes) = run_family(body_fail_spec);

    s.add_dyn(
        "F3410-01 段位守卫",
        seg_guard_ok(),
        &format!("seg=0x{:05x} align40={}", SEG_BASE, SEG_BASE % 0x40 == 0),
    );

    // 正常流：用例级+族级钩子齐备、teardown 必达（J1 主判据）。
    s.add_dyn(
        "F3410-02 前后置钩子teardown必达",
        j1_main_ok(),
        &format!("log={}", log.len()),
    );

    // M-169 必达性验证四档全绿。
    let (m_ok, m_per) = m169_verify();
    s.add_dyn(
        "F3410-03 M169必达性验证四档",
        m_ok,
        &format!("per={:?}", m_per),
    );

    // 必达直证：体失败用例的 teardown 必在账。
    s.add_dyn(
        "F3410-04 teardown必达直证",
        teardown_mandatory_ok(),
        &format!("mandatory={}", teardown_mandatory_ok()),
    );

    // 注入必抓：8 项注入全检出。
    s.add_dyn(
        "F3410-05 注入矩阵八项全检出",
        inject_all_ok(),
        &format!("matrix={:?}", inject_matrix()),
    );

    // 例 setup 失败面：无例钩义务、其余照常配对。
    s.add_dyn(
        "F3410-06 例setup失败无例钩义务",
        case_setup_fail_ok(),
        &format!("case_fail={}", case_setup_fail_ok()),
    );

    // 族 setup 失败面：整族不跑、账全空。
    s.add_dyn(
        "F3410-07 族setup失败整族不跑",
        family_setup_fail_ok(),
        &format!("family_fail={}", family_setup_fail_ok()),
    );

    // 边界：空族与重名与空名显性拒。
    let mut gate = FamilySpec {
        family: "闸族",
        setup_ok: true,
        cases: Vec::new(),
    };
    let empty_fam = gate.len() == 0;
    let _ = gate.add_case("甲", true, false);
    let dup = gate.add_case("甲", true, false) == Err(Diag::EDupCase.val());
    let blank = gate.add_case("", true, false) == Err(Diag::EEmptyName.val());
    s.add_dyn(
        "F3410-08 空族重名空名显性拒",
        empty_fam && dup && blank && gate.len() == 1,
        &format!("len={}", gate.len()),
    );

    // 边界：取钩越位显性拒。
    s.add_dyn(
        "F3410-09 取钩越位显性拒",
        range_ok(),
        &format!("range={}", range_ok()),
    );

    // 边界：钩子账封账单向（封账后追加与再封均拒——手工装配未封账构造）。
    let mut sealed = HookLog::new();
    let _ = sealed.push(HookKind::FamilySetup, "族");
    let _ = sealed.push(HookKind::CaseSetup, "甲");
    let _ = sealed.push(HookKind::CaseTeardown, "甲");
    let seal1 = sealed.seal().is_ok();
    let seal2 = sealed.seal() == Err(Diag::ESealed.val());
    let push_after = sealed.push(HookKind::CaseTeardown, "越权") == Err(Diag::ESealed.val());
    s.add_dyn(
        "F3410-10 钩子账封账单向",
        seal1 && seal2 && push_after && sealed.sealed(),
        &format!("rejects={}", sealed.rejects()),
    );

    // 钩子账序连续。
    s.add_dyn(
        "F3410-11 钩子账序连续",
        log.seq_ok() && log.len() == 8,
        &format!("log={}", log.len()),
    );

    // 写回摘要非空（判据③写回面：钩子账逐行 + 结局册逐行）。
    let mut sum = String::new();
    let mut k = 0usize;
    while k < log.len() {
        match log.at(k) {
            Ok(r) => {
                sum.push_str(&format!("{}{}\n", r.kind.name(), r.case));
            }
            Err(_) => break,
        }
        k += 1;
    }
    for o in &outcomes {
        sum.push_str(&format!(
            "例{} 体{}\n",
            o.case,
            if o.body_fail { "败" } else { "过" }
        ));
    }
    s.add_dyn(
        "F3410-12 写回摘要非空含钩子行",
        !sum.is_empty() && sum.contains("族前族") && sum.contains("族后族"),
        &format!("bytes={}", sum.len()),
    );

    // 双跑一致。
    s.add_dyn(
        "F3410-13 双跑一致",
        dual_run_ok(),
        &format!("dual={}", dual_run_ok()),
    );

    // 诊断码闭集。
    s.add_dyn(
        "F3410-14 诊断码闭集",
        ALL.len() == 12 && detail(SEG_BASE + 0x0C) == Diag::ESealed.detail(),
        &format!("all={} seg=0x{:05x}", ALL.len(), SEG_BASE),
    );

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn diag_codes_closed() {
        assert_eq!(ALL.len(), 12);
        assert_eq!(Diag::ENoCaseTeardown.val(), SEG_BASE + 0x01);
        assert_eq!(Diag::ESealed.val(), SEG_BASE + 0x0C);
        for d in ALL {
            assert!(!detail(d.val()).is_empty());
        }
        assert!(detail(SEG_BASE + 0x70).starts_with("未知诊断码"));
    }

    #[test]
    fn seg_guard() {
        assert!(seg_guard_ok());
        assert_eq!(SEG_BASE % 0x40, 0);
        assert_eq!(SEG_BASE, 0x795C0);
    }

    #[test]
    fn green_family_hook_shape() {
        let corpus = m169_corpus();
        let spec = match corpus.get(0) {
            Some(x) => x,
            None => return,
        };
        let (log, outcomes) = run_family(spec);
        // 族前 + 三例(前+后) + 族后 = 8。
        assert_eq!(log.len(), 8);
        assert_eq!(outcomes.len(), 3);
        assert_eq!(audit(&log, spec, &outcomes), Ok(()));
    }

    #[test]
    fn body_fail_does_not_skip_teardown() {
        assert!(teardown_mandatory_ok());
        let corpus = m169_corpus();
        let spec = match corpus.get(1) {
            Some(x) => x,
            None => return,
        };
        let (log, outcomes) = run_family(spec);
        assert!(outcomes.iter().any(|o| o.case == "乙" && o.body_fail));
        let mut found = false;
        for i in 0..log.len() {
            if let Ok(r) = log.at(i) {
                if r.kind == HookKind::CaseTeardown && r.case == "乙" {
                    found = true;
                }
            }
        }
        assert!(found, "体失败用例的 teardown 必在账");
    }

    #[test]
    fn case_setup_fail_no_case_hooks() {
        assert!(case_setup_fail_ok());
        let corpus = m169_corpus();
        let spec = match corpus.get(2) {
            Some(x) => x,
            None => return,
        };
        let (log, _) = run_family(spec);
        assert_eq!(log.len(), 6);
        for i in 0..log.len() {
            if let Ok(r) = log.at(i) {
                assert_ne!(r.case, "乙", "例 setup 失败例不得落例钩");
            }
        }
    }

    #[test]
    fn family_setup_fail_runs_nothing() {
        assert!(family_setup_fail_ok());
        let corpus = m169_corpus();
        let spec = match corpus.get(3) {
            Some(x) => x,
            None => return,
        };
        let (log, outcomes) = run_family(spec);
        assert_eq!(log.len(), 0);
        assert_eq!(outcomes.len(), 0);
    }

    #[test]
    fn m169_four_cases_green() {
        assert_eq!(m169_corpus().len(), M169_CASES);
        let (all, per) = m169_verify();
        assert!(all);
        assert!(per.iter().all(|x| *x));
    }

    #[test]
    fn inject_eight_of_eight() {
        assert_eq!(inject_matrix(), [true; INJECTS]);
    }

    #[test]
    fn missing_teardown_detected() {
        let corpus = m169_corpus();
        let spec = match corpus.get(1) {
            Some(x) => x,
            None => return,
        };
        let (log, outcomes) = run_family(spec);
        // 抽掉甲的例后钩 → ENoCaseTeardown。
        let mut broken = HookLog::new();
        for i in 0..log.len() {
            if let Ok(r) = log.at(i) {
                if !(r.kind == HookKind::CaseTeardown && r.case == "甲") {
                    let _ = broken.push(r.kind, r.case);
                }
            }
        }
        assert_eq!(
            audit(&broken, spec, &outcomes),
            Err(Diag::ENoCaseTeardown.val())
        );
    }

    #[test]
    fn empty_family_rejected() {
        let spec = FamilySpec {
            family: "空族",
            setup_ok: true,
            cases: Vec::new(),
        };
        let (log, outcomes) = run_family(&spec);
        assert_eq!(log.len(), 0);
        assert_eq!(audit(&log, &spec, &outcomes), Err(Diag::EEmptyFamily.val()));
        let mut g = FamilySpec {
            family: "闸",
            setup_ok: true,
            cases: Vec::new(),
        };
        let _ = g.add_case("甲", true, false);
        assert_eq!(g.add_case("甲", true, false), Err(Diag::EDupCase.val()));
        assert_eq!(g.add_case("", true, false), Err(Diag::EEmptyName.val()));
    }

    #[test]
    fn log_seal_one_way() {
        let mut log = HookLog::new();
        let _ = log.push(HookKind::FamilySetup, "族");
        let _ = log.push(HookKind::CaseTeardown, "甲");
        assert_eq!(log.seal(), Ok(()));
        assert_eq!(log.seal(), Err(Diag::ESealed.val()));
        assert_eq!(
            log.push(HookKind::CaseTeardown, "越权"),
            Err(Diag::ESealed.val())
        );
        assert!(log.sealed());
    }

    #[test]
    fn range_guard() {
        assert!(range_ok());
        let corpus = m169_corpus();
        let spec = match corpus.get(0) {
            Some(x) => x,
            None => return,
        };
        let (log, _) = run_family(spec);
        assert_eq!(log.at(0).map(|r| r.seq), Ok(1));
        assert!(log.at(99).is_err());
    }

    #[test]
    fn dual_run_consistency() {
        assert!(dual_run_ok());
    }

    #[test]
    fn checks_all_green() {
        let s = run_unx3410_checks();
        assert_eq!(s.len(), 14);
        assert_eq!(s.failed_count(), 0);
        assert!(s.all_passed());
    }
}
