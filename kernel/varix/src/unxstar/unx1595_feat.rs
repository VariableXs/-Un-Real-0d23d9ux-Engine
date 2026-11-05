//! UNX-F1595 域经沉淀最终条（全域收官级沉淀三条款） · 0x792C0段
//!
//! 判据原文（UNX-F1595-J1 摘录）：「收官级沉淀三条款齐备（每条：条款/全域
//! 实证/传承建议三列），条款可溯 10/10，随沉淀账 v1.1 留痕」；完成时 API/
//! 软件写回：状态=已完成、结果摘要非空。
//!
//! 设计要点：
//! 1. 三列闭面：每条最终条必带 条款/全域实证/传承建议 三列，任一列空在
//!    入账口即拒——空列条款不得沉淀入账；
//! 2. 条款可溯 10/10：十条最终条逐条可溯（条款号落在域判据号 F0801..=F1600
//!    且三列齐备），逐条必须通过可溯校验，断链条款零容忍；
//! 3. 沉淀账 v1.1 留痕：账面版本冻结为 v1.1，落账/封账留痕逐行带版本号，
//!    版本不符显性拒；账为追加式（append-only），封账后追加全拒；
//! 4. 相位机 追加中→已封账 单向；封账前置：三条款齐备且逐条可溯；
//! 5. 诊断码闭集 0x792C1..0x792CC，Err 用 Result<T,u32>，中文诊断文案带
//!    可操作建议。

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 段基址（派工卡 B09 给定值；本批窗口 0x79200..=0x793C0）。
pub const SEG_BASE: u32 = 0x792C0;

/// 收官级最终条数（判据原文三条款为封账下限，账面钦定十条全谱）。
pub const CLAUSE_COUNT: usize = 10;

/// 封账下限条款数（收官级沉淀三条款）。
pub const SEAL_MIN_CLAUSES: usize = 3;

/// 条款可溯探测种数（判据原文 10/10）。
pub const TRACE_PROBES: usize = 10;

/// 沉淀账版本（判据原文 v1.1，冻结值）。
pub const SEDIMENT_VERSION: &str = "v1.1";

/// 域判据号下沿（条款号可溯区间）。
pub const CLAUSE_NO_MIN: u32 = 801;

/// 域判据号上沿（条款号可溯区间）。
pub const CLAUSE_NO_MAX: u32 = 1600;

/// 留痕指纹折叠种子（FNV-1a 偏移基）。
pub const DIGEST_SEED: u64 = 0xCBF2_9CE4_8422_2325;

/// 留痕指纹折叠质数（FNV-1a 质数）。
pub const DIGEST_PRIME: u64 = 0x100_0000_01B3;

/// 沉淀最终条诊断码闭集。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Diag {
    /// 封账后追加拒。
    ESealed,
    /// 条款号重复（同号已沉淀）。
    EDupNo,
    /// 条款号出域（不在 F0801..=F1600）。
    ERange,
    /// 条款列空。
    EClause,
    /// 全域实证列空。
    EEvidence,
    /// 传承建议列空。
    ELegacy,
    /// 三条款未齐先封账。
    ESeal,
    /// 不可溯条款入账（可溯校验未过）。
    ENoTrace,
    /// 沉淀账版本不符。
    EVersion,
    /// 空账封账拒。
    EEmpty,
    /// 未封账先出终证。
    EOrder,
    /// 条款号跳序（账内号必须按域序递增）。
    ESeq,
}

impl Diag {
    /// 诊断码值（SEG_BASE + 0x01..0x0C 闭集）。
    pub fn val(self) -> u32 {
        SEG_BASE
            + match self {
                Diag::ESealed => 0x01,
                Diag::EDupNo => 0x02,
                Diag::ERange => 0x03,
                Diag::EClause => 0x04,
                Diag::EEvidence => 0x05,
                Diag::ELegacy => 0x06,
                Diag::ESeal => 0x07,
                Diag::ENoTrace => 0x08,
                Diag::EVersion => 0x09,
                Diag::EEmpty => 0x0a,
                Diag::EOrder => 0x0b,
                Diag::ESeq => 0x0c,
            }
    }

    /// 中文诊断文案（可操作建议）。
    pub fn detail(self) -> &'static str {
        match self {
            Diag::ESealed => "封账后追加拒：沉淀账已封，新条款走下一版沉淀账",
            Diag::EDupNo => "条款号重复：同号已沉淀，先查账面再入账",
            Diag::ERange => "条款号出域：条款号须落在 F0801..=F1600，先核域判据号",
            Diag::EClause => "条款列空：无条款文字不成条，先补条款列再入账",
            Diag::EEvidence => "全域实证列空：实证列须引全域判据实绩，先补实证",
            Diag::ELegacy => "传承建议列空：传承列须给后续批可操作建议，先补建议",
            Diag::ESeal => "三条款未齐：封账下限三条款，先补齐再封",
            Diag::ENoTrace => "不可溯条款：条款号或三列断链，逐列修复再入账",
            Diag::EVersion => "沉淀账版本不符：账面冻结 v1.1，版本错写即拒",
            Diag::EEmpty => "空账封账拒：无条不成账，先沉淀条款",
            Diag::EOrder => "未封账先出终证：终证在封账后生成，先封账再取",
            Diag::ESeq => "条款号跳序：账内号按域序递增，先修号序再入账",
        }
    }
}

/// 全部诊断码（闭集数组）。
pub const ALL: &[Diag] = &[
    Diag::ESealed,
    Diag::EDupNo,
    Diag::ERange,
    Diag::EClause,
    Diag::EEvidence,
    Diag::ELegacy,
    Diag::ESeal,
    Diag::ENoTrace,
    Diag::EVersion,
    Diag::EEmpty,
    Diag::EOrder,
    Diag::ESeq,
];

/// 诊断码查表（闭集返回）。
pub fn detail(code: u32) -> &'static str {
    for d in ALL {
        if d.val() == code {
            return d.detail();
        }
    }
    "未知诊断码：不在 0x792C1..0x792CC 闭集内"
}

/// 沉淀账相位（追加中→已封账 单向）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SedPhase {
    /// 追加中：条款逐条入账。
    Appending,
    /// 已封账：三条款下限过且逐条可溯，账面封存。
    Sealed,
}

impl SedPhase {
    /// 序数（倒退检测用）。
    pub fn ord(self) -> u8 {
        match self {
            SedPhase::Appending => 0,
            SedPhase::Sealed => 1,
        }
    }

    /// 中文名。
    pub fn name(self) -> &'static str {
        match self {
            SedPhase::Appending => "追加中",
            SedPhase::Sealed => "已封账",
        }
    }
}

/// 一条收官级最终条（三列：条款/全域实证/传承建议）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FinalClause {
    /// 条款号（域判据号，落在 F0801..=F1600）。
    pub no: u32,
    /// 条款列。
    pub clause: &'static str,
    /// 全域实证列。
    pub evidence: &'static str,
    /// 传承建议列。
    pub legacy: &'static str,
}

impl FinalClause {
    /// 三列齐备校验（任一列空即不可溯）。
    pub fn columns_ok(&self) -> bool {
        !self.clause.is_empty() && !self.evidence.is_empty() && !self.legacy.is_empty()
    }

    /// 条款可溯校验（号在域内 + 三列齐备）。
    pub fn traceable(&self) -> bool {
        if self.no < CLAUSE_NO_MIN || self.no > CLAUSE_NO_MAX {
            return false;
        }
        self.columns_ok()
    }
}

/// 沉淀账（追加式收官级最终条账面）。
#[derive(Clone, Debug)]
pub struct SedimentLedger {
    version: &'static str,
    entries: Vec<FinalClause>,
    phase: SedPhase,
    /// 结构性拒绝计数。
    rejects: u32,
}

impl SedimentLedger {
    /// 新沉淀账（版本冻结 v1.1）。
    pub fn new() -> SedimentLedger {
        SedimentLedger {
            version: SEDIMENT_VERSION,
            entries: Vec::new(),
            phase: SedPhase::Appending,
            rejects: 0,
        }
    }

    /// 账面版本。
    pub fn version(&self) -> &'static str {
        self.version
    }

    /// 相位。
    pub fn phase(&self) -> SedPhase {
        self.phase
    }

    /// 拒绝计数。
    pub fn rejects(&self) -> u32 {
        self.rejects
    }

    /// 已沉淀条数。
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// 按条款号取条。
    pub fn clause_of(&self, no: u32) -> Option<&FinalClause> {
        self.entries.iter().find(|c| c.no == no)
    }

    /// 入账（追加式：号唯一、号在域内、三列齐备、号序递增；封账后全拒）。
    pub fn append(&mut self, clause: FinalClause) -> Result<(), u32> {
        if self.phase == SedPhase::Sealed {
            self.rejects += 1;
            return Err(Diag::ESealed.val());
        }
        if self.version != SEDIMENT_VERSION {
            self.rejects += 1;
            return Err(Diag::EVersion.val());
        }
        if clause.no < CLAUSE_NO_MIN || clause.no > CLAUSE_NO_MAX {
            self.rejects += 1;
            return Err(Diag::ERange.val());
        }
        if self.entries.iter().any(|c| c.no == clause.no) {
            self.rejects += 1;
            return Err(Diag::EDupNo.val());
        }
        if let Some(last) = self.entries.last() {
            if clause.no <= last.no {
                self.rejects += 1;
                return Err(Diag::ESeq.val());
            }
        }
        if clause.clause.is_empty() {
            self.rejects += 1;
            return Err(Diag::EClause.val());
        }
        if clause.evidence.is_empty() {
            self.rejects += 1;
            return Err(Diag::EEvidence.val());
        }
        if clause.legacy.is_empty() {
            self.rejects += 1;
            return Err(Diag::ELegacy.val());
        }
        if !clause.traceable() {
            self.rejects += 1;
            return Err(Diag::ENoTrace.val());
        }
        self.entries.push(clause);
        Ok(())
    }

    /// 封账（三条款下限 + 逐条可溯才封；单向，倒退拒）。
    pub fn seal(&mut self) -> Result<(), u32> {
        if self.phase != SedPhase::Appending {
            self.rejects += 1;
            return Err(Diag::ESealed.val());
        }
        if self.entries.is_empty() {
            return Err(Diag::EEmpty.val());
        }
        if self.entries.len() < SEAL_MIN_CLAUSES {
            return Err(Diag::ESeal.val());
        }
        for c in &self.entries {
            if !c.traceable() {
                return Err(Diag::ENoTrace.val());
            }
        }
        self.phase = SedPhase::Sealed;
        Ok(())
    }

    /// 终证（封账后生成：版本留痕 + 逐条三列一行）。
    pub fn final_proof(&self) -> Result<String, u32> {
        if self.phase != SedPhase::Sealed {
            return Err(Diag::EOrder.val());
        }
        let mut out = format!("沉淀账{} 终证 条数{}\n", self.version, self.entries.len());
        for c in &self.entries {
            out.push_str(&format!(
                "条{} 【{}】实证【{}】传承【{}】\n",
                c.no, c.clause, c.evidence, c.legacy
            ));
        }
        Ok(out)
    }
}

/// 留痕指纹（FNV-1a 整数折叠：三列混折）。
pub fn clause_digest(clause: &FinalClause) -> u64 {
    let mut h = DIGEST_SEED;
    for b in clause.clause.as_bytes() {
        h ^= *b as u64;
        h = h.wrapping_mul(DIGEST_PRIME);
    }
    for b in clause.evidence.as_bytes() {
        h ^= *b as u64;
        h = h.wrapping_mul(DIGEST_PRIME);
    }
    for b in clause.legacy.as_bytes() {
        h ^= *b as u64;
        h = h.wrapping_mul(DIGEST_PRIME);
    }
    h
}

/// 十条钦定最终条（条款号递增落域内，三列齐备）。
pub const FINAL_CLAUSES: [FinalClause; CLAUSE_COUNT] = [
    FinalClause { no: 1501, clause: "收官总链一串定真伪", evidence: "F1592 主键非零且复验步数预算内", legacy: "后续批收官先折主键再动链" },
    FinalClause { no: 1510, clause: "风险清账悬空零容忍", evidence: "F1593 悬空十笔全点名且清账零悬空", legacy: "新险先立账再归置，三态逐项落账" },
    FinalClause { no: 1520, clause: "五证据齐备才收官", evidence: "F1594 缺项宣告拦截十种全拦", legacy: "宣告前逐证核见证数，缺一不宣告" },
    FinalClause { no: 1530, clause: "沉淀三列缺一不入账", evidence: "本账十条逐条三列齐备", legacy: "沉淀必带实证与传承，空列不入账" },
    FinalClause { no: 1540, clause: "体验信号闭环才算收官", evidence: "F1596 闭环率百分之百", legacy: "新信号先入流再闭环，收官前清零" },
    FinalClause { no: 1550, clause: "移交四件齐备才宣告", evidence: "F1597 宣告不可撤销且记录归档", legacy: "交接前核包链时点签收四件" },
    FinalClause { no: 1560, clause: "域账封账五证明齐备", evidence: "F1598 封账后写拦截十种全拦", legacy: "封账前核五证明与行数守恒" },
    FinalClause { no: 1570, clause: "闭账恒等式三向实核", evidence: "F1599 恒等破坏注入十种全检出", legacy: "改账先核条数行数判据号三向恒等" },
    FinalClause { no: 1580, clause: "观测面无盲区才收官", evidence: "全域观测账逐面留痕非空", legacy: "新面先入观测账再收官" },
    FinalClause { no: 1590, clause: "传承建议可操作", evidence: "十条传承列逐条非空且带动作", legacy: "建议写成动作句，不写口号" },
];

/// 标准沉淀账：十条齐备（追加中相位，未封账）。
pub fn build_ledger() -> SedimentLedger {
    let mut ledger = SedimentLedger::new();
    for c in FINAL_CLAUSES.iter() {
        let _ = ledger.append(*c);
    }
    ledger
}

/// 条款可溯 10/10：十条钦定条款逐条必须通过可溯校验与账面取回。
/// 返回十布尔（true=该条可溯且账面可取回）。
pub fn trace_matrix() -> [bool; TRACE_PROBES] {
    let ledger = build_ledger();
    let mut out = [false; TRACE_PROBES];
    let mut i = 0usize;
    while i < TRACE_PROBES {
        let c = match FINAL_CLAUSES.get(i) {
            Some(x) => *x,
            None => break,
        };
        let fetched_ok = match ledger.clause_of(c.no) {
            Some(got) => got == &c,
            None => false,
        };
        if let Some(cell) = out.get_mut(i) {
            *cell = c.traceable() && fetched_ok;
        }
        i += 1;
    }
    out
}

/// 断链注入 10/10：十种断链注入（号出域上沿外/下沿外/条款列空/实证列空/
/// 传承列空/重号/跳序/封账后追加/版本错写/空账封账）逐种必须被入账口或
/// 封账口点出。返回十布尔（true=该注入被拒）。
pub fn break_matrix() -> [bool; TRACE_PROBES] {
    let mut out = [false; TRACE_PROBES];
    // 注入1：号出域（上沿外 1601）。
    let mut l = SedimentLedger::new();
    let rejected = l.append(FinalClause { no: 1601, clause: "甲", evidence: "乙", legacy: "丙" }) == Err(Diag::ERange.val());
    if let Some(cell) = out.get_mut(0) {
        *cell = rejected;
    }
    // 注入2：号出域（下沿外 800）。
    let mut l = SedimentLedger::new();
    let rejected = l.append(FinalClause { no: 800, clause: "甲", evidence: "乙", legacy: "丙" }) == Err(Diag::ERange.val());
    if let Some(cell) = out.get_mut(1) {
        *cell = rejected;
    }
    // 注入3：条款列空。
    let mut l = SedimentLedger::new();
    let rejected = l.append(FinalClause { no: 1501, clause: "", evidence: "乙", legacy: "丙" }) == Err(Diag::EClause.val());
    if let Some(cell) = out.get_mut(2) {
        *cell = rejected;
    }
    // 注入4：实证列空。
    let mut l = SedimentLedger::new();
    let rejected = l.append(FinalClause { no: 1501, clause: "甲", evidence: "", legacy: "丙" }) == Err(Diag::EEvidence.val());
    if let Some(cell) = out.get_mut(3) {
        *cell = rejected;
    }
    // 注入5：传承列空。
    let mut l = SedimentLedger::new();
    let rejected = l.append(FinalClause { no: 1501, clause: "甲", evidence: "乙", legacy: "" }) == Err(Diag::ELegacy.val());
    if let Some(cell) = out.get_mut(4) {
        *cell = rejected;
    }
    // 注入6：重号。
    let mut l = build_ledger();
    let head = match FINAL_CLAUSES.get(0) {
        Some(x) => *x,
        None => FinalClause { no: 1501, clause: "甲", evidence: "乙", legacy: "丙" },
    };
    let rejected = l.append(head) == Err(Diag::EDupNo.val());
    if let Some(cell) = out.get_mut(5) {
        *cell = rejected;
    }
    // 注入7：跳序（号小于尾条）。
    let mut l = build_ledger();
    let rejected = l.append(FinalClause { no: 1502, clause: "甲", evidence: "乙", legacy: "丙" }) == Err(Diag::ESeq.val());
    if let Some(cell) = out.get_mut(6) {
        *cell = rejected;
    }
    // 注入8：封账后追加。
    let mut l = build_ledger();
    let _ = l.seal();
    let rejected = l.append(FinalClause { no: 1599, clause: "甲", evidence: "乙", legacy: "丙" }) == Err(Diag::ESealed.val());
    if let Some(cell) = out.get_mut(7) {
        *cell = rejected;
    }
    // 注入9：三条款未齐先封账。
    let mut l = SedimentLedger::new();
    let _ = l.append(FinalClause { no: 1501, clause: "甲", evidence: "乙", legacy: "丙" });
    let _ = l.append(FinalClause { no: 1502, clause: "甲", evidence: "乙", legacy: "丙" });
    let rejected = l.seal() == Err(Diag::ESeal.val());
    if let Some(cell) = out.get_mut(8) {
        *cell = rejected;
    }
    // 注入10：空账封账。
    let mut l = SedimentLedger::new();
    let rejected = l.seal() == Err(Diag::EEmpty.val());
    if let Some(cell) = out.get_mut(9) {
        *cell = rejected;
    }
    out
}

// ---- 判据谓词（私有） ------------------------------------------------------

/// 段位守卫（对齐 0x40；窗口按 B09 派工卡 0x79200..=0x793C0）。
fn seg_guard_ok() -> bool {
    if SEG_BASE % 0x40 != 0 {
        return false;
    }
    if SEG_BASE < 0x79200 || SEG_BASE > 0x793C0 {
        return false;
    }
    SEG_BASE == 0x792C0
}

/// 三条款齐备（账面条数足封账下限且逐条三列齐备）。
fn three_clauses_ok(ledger: &SedimentLedger) -> bool {
    if ledger.len() < SEAL_MIN_CLAUSES {
        return false;
    }
    for i in 0..ledger.len() {
        let c = match ledger.clause_of(match FINAL_CLAUSES.get(i) {
            Some(x) => x.no,
            None => 0,
        }) {
            Some(c) => c,
            None => return false,
        };
        if !c.columns_ok() {
            return false;
        }
    }
    true
}

/// 条款可溯 10/10。
fn trace_all_ok() -> bool {
    trace_matrix() == [true; TRACE_PROBES]
}

/// 断链注入 10/10 检出。
fn break_all_ok() -> bool {
    break_matrix() == [true; TRACE_PROBES]
}

/// 版本留痕（账面版本冻结 v1.1 且终证带版本号）。
fn version_ok(ledger: &SedimentLedger) -> bool {
    if ledger.version() != SEDIMENT_VERSION {
        return false;
    }
    match ledger.final_proof() {
        Ok(t) => t.contains(SEDIMENT_VERSION),
        Err(_) => false,
    }
}

/// 双跑一致（同构输入两次构建终证一致且非空）。
fn dual_run_ok() -> bool {
    let mut a = build_ledger();
    let mut b = build_ledger();
    let _ = a.seal();
    let _ = b.seal();
    match (a.final_proof(), b.final_proof()) {
        (Ok(x), Ok(y)) => x == y && !x.is_empty(),
        _ => false,
    }
}

/// 沉淀最终条判据：UNX-F1595 域经沉淀最终条。
pub fn run_unx1595_checks() -> CheckSet {
    let mut s = CheckSet::new("unx1595");
    let mut ledger = build_ledger();

    s.add_dyn(
        "F1595-01 段位守卫",
        seg_guard_ok(),
        &format!("seg=0x{:05x} align40={}", SEG_BASE, SEG_BASE % 0x40 == 0),
    );

    // 版本留痕：账面版本冻结 v1.1。
    s.add_dyn(
        "F1595-02 沉淀账版本冻结 v1.1",
        ledger.version() == SEDIMENT_VERSION && SEDIMENT_VERSION == "v1.1",
        &format!("version={}", ledger.version()),
    );

    // 正常流：十条齐备且三条款下限过。
    s.add_dyn(
        "F1595-03 十条齐备三条款下限过",
        ledger.len() == CLAUSE_COUNT && three_clauses_ok(&ledger),
        &format!("entries={}/{}", ledger.len(), CLAUSE_COUNT),
    );

    // 三列闭面：三列名与入账三拒对齐（空列逐列拒）。
    let mut cols = SedimentLedger::new();
    let c1 = cols.append(FinalClause { no: 1501, clause: "", evidence: "乙", legacy: "丙" }) == Err(Diag::EClause.val());
    let c2 = cols.append(FinalClause { no: 1501, clause: "甲", evidence: "", legacy: "丙" }) == Err(Diag::EEvidence.val());
    let c3 = cols.append(FinalClause { no: 1501, clause: "甲", evidence: "乙", legacy: "" }) == Err(Diag::ELegacy.val());
    s.add_dyn(
        "F1595-04 三列空列逐列拒",
        c1 && c2 && c3,
        &format!("rejects={}", cols.rejects()),
    );

    // 条款可溯 10/10。
    s.add_dyn(
        "F1595-05 条款可溯十条全过",
        trace_all_ok(),
        &format!("matrix={:?}", trace_matrix()),
    );

    // 断链注入 10/10。
    s.add_dyn(
        "F1595-06 断链注入十种全检出",
        break_all_ok(),
        &format!("matrix={:?}", break_matrix()),
    );

    // 号域闭面：号必须落在 F0801..=F1600。
    let mut range = SedimentLedger::new();
    let under = range.append(FinalClause { no: CLAUSE_NO_MIN - 1, clause: "甲", evidence: "乙", legacy: "丙" }) == Err(Diag::ERange.val());
    let over = range.append(FinalClause { no: CLAUSE_NO_MAX + 1, clause: "甲", evidence: "乙", legacy: "丙" }) == Err(Diag::ERange.val());
    let edge_lo = range.append(FinalClause { no: CLAUSE_NO_MIN, clause: "甲", evidence: "乙", legacy: "丙" }) == Ok(());
    s.add_dyn(
        "F1595-07 条款号域边界",
        under && over && edge_lo,
        &format!("range={}-{}", CLAUSE_NO_MIN, CLAUSE_NO_MAX),
    );

    // 号序递增：跳序显性拒、重号显性拒。
    let mut seq = build_ledger();
    let head = match FINAL_CLAUSES.get(0) {
        Some(x) => *x,
        None => FinalClause { no: 1501, clause: "甲", evidence: "乙", legacy: "丙" },
    };
    let jump = seq.append(FinalClause { no: 1502, clause: "甲", evidence: "乙", legacy: "丙" }) == Err(Diag::ESeq.val());
    let dup = seq.append(head) == Err(Diag::EDupNo.val());
    s.add_dyn(
        "F1595-08 号序递增与重号拒",
        jump && dup,
        &format!("rejects={}", seq.rejects()),
    );

    // 封账：封账后追加显性拒、重复封账显性拒。
    let seal_ok = ledger.seal() == Ok(());
    let after_append = ledger.append(FinalClause { no: 1599, clause: "甲", evidence: "乙", legacy: "丙" }) == Err(Diag::ESealed.val());
    let re_seal = ledger.seal() == Err(Diag::ESealed.val());
    s.add_dyn(
        "F1595-09 封账后追加与重封全拒",
        seal_ok && after_append && re_seal && ledger.phase().ord() == 1,
        &format!("phase={}", ledger.phase().name()),
    );

    // 终证：封账后生成非空且带版本留痕；未封账终证显性拒。
    let mut early = SedimentLedger::new();
    let head = match FINAL_CLAUSES.get(0) {
        Some(x) => *x,
        None => FinalClause { no: 1501, clause: "甲", evidence: "乙", legacy: "丙" },
    };
    let _ = early.append(head);
    let early_rej = early.final_proof() == Err(Diag::EOrder.val());
    s.add_dyn(
        "F1595-10 未封账终证显性拒",
        early_rej && version_ok(&ledger),
        &format!("version_trace={}", version_ok(&ledger)),
    );

    // 终证逐条三列全名落档。
    let proof = ledger.final_proof();
    let proof_ok = match &proof {
        Ok(t) => t.contains("条1501") && t.contains("条1590") && t.contains("实证") && t.contains("传承"),
        Err(_) => false,
    };
    s.add_dyn(
        "F1595-11 终证三列逐条落档",
        proof_ok,
        &format!("bytes={}", proof.as_ref().map(|t| t.len()).unwrap_or(0)),
    );

    // 双跑一致与诊断码闭集。
    s.add_dyn(
        "F1595-12 双跑一致与诊断码闭集",
        dual_run_ok() && ALL.len() == 12 && detail(SEG_BASE + 0x0C) == Diag::ESeq.detail(),
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
        assert_eq!(Diag::ESealed.val(), SEG_BASE + 0x01);
        assert_eq!(Diag::ESeq.val(), SEG_BASE + 0x0C);
        for d in ALL {
            assert!(!detail(d.val()).is_empty());
        }
        assert!(detail(SEG_BASE + 0x70).starts_with("未知诊断码"));
    }

    #[test]
    fn seg_guard() {
        assert!(seg_guard_ok());
        assert_eq!(SEG_BASE % 0x40, 0);
        assert_eq!(SEG_BASE, 0x792C0);
    }

    #[test]
    fn version_frozen() {
        assert_eq!(SEDIMENT_VERSION, "v1.1");
        let l = SedimentLedger::new();
        assert_eq!(l.version(), "v1.1");
        assert_eq!(l.phase(), SedPhase::Appending);
    }

    #[test]
    fn columns_gates() {
        let mut l = SedimentLedger::new();
        assert_eq!(
            l.append(FinalClause { no: 1501, clause: "", evidence: "乙", legacy: "丙" }),
            Err(Diag::EClause.val())
        );
        assert_eq!(
            l.append(FinalClause { no: 1501, clause: "甲", evidence: "", legacy: "丙" }),
            Err(Diag::EEvidence.val())
        );
        assert_eq!(
            l.append(FinalClause { no: 1501, clause: "甲", evidence: "乙", legacy: "" }),
            Err(Diag::ELegacy.val())
        );
        assert_eq!(l.len(), 0);
    }

    #[test]
    fn no_range_and_order() {
        let mut l = SedimentLedger::new();
        assert_eq!(
            l.append(FinalClause { no: 800, clause: "甲", evidence: "乙", legacy: "丙" }),
            Err(Diag::ERange.val())
        );
        assert_eq!(
            l.append(FinalClause { no: 1601, clause: "甲", evidence: "乙", legacy: "丙" }),
            Err(Diag::ERange.val())
        );
        assert_eq!(
            l.append(FinalClause { no: 1502, clause: "甲", evidence: "乙", legacy: "丙" }),
            Ok(())
        );
        assert_eq!(
            l.append(FinalClause { no: 1502, clause: "再", evidence: "乙", legacy: "丙" }),
            Err(Diag::EDupNo.val())
        );
        assert_eq!(
            l.append(FinalClause { no: 1501, clause: "回", evidence: "乙", legacy: "丙" }),
            Err(Diag::ESeq.val())
        );
    }

    #[test]
    fn trace_ten_of_ten() {
        assert_eq!(trace_matrix(), [true; TRACE_PROBES]);
        for c in FINAL_CLAUSES.iter() {
            assert!(c.traceable(), "条{} 必须可溯", c.no);
            assert!(c.columns_ok());
        }
    }

    #[test]
    fn break_ten_of_ten() {
        assert_eq!(break_matrix(), [true; TRACE_PROBES]);
    }

    #[test]
    fn seal_needs_three_clauses() {
        let mut l = SedimentLedger::new();
        assert_eq!(l.seal(), Err(Diag::EEmpty.val()));
        let _ = l.append(FinalClause { no: 1501, clause: "甲", evidence: "乙", legacy: "丙" });
        let _ = l.append(FinalClause { no: 1502, clause: "甲", evidence: "乙", legacy: "丙" });
        assert_eq!(l.seal(), Err(Diag::ESeal.val()));
        let _ = l.append(FinalClause { no: 1503, clause: "甲", evidence: "乙", legacy: "丙" });
        assert_eq!(l.seal(), Ok(()));
        assert_eq!(l.phase(), SedPhase::Sealed);
    }

    #[test]
    fn sealed_rejects_append_and_reseal() {
        let mut l = build_ledger();
        assert_eq!(l.seal(), Ok(()));
        assert_eq!(
            l.append(FinalClause { no: 1599, clause: "甲", evidence: "乙", legacy: "丙" }),
            Err(Diag::ESealed.val())
        );
        assert_eq!(l.seal(), Err(Diag::ESealed.val()));
        assert_eq!(l.phase().ord(), 1);
    }

    #[test]
    fn final_proof_order_gate() {
        let mut l = build_ledger();
        assert_eq!(l.final_proof(), Err(Diag::EOrder.val()));
        let _ = l.seal();
        let proof = l.final_proof();
        assert!(proof.is_ok());
        let t = proof.unwrap_or_default();
        assert!(t.contains(SEDIMENT_VERSION));
        assert!(t.contains("条1501"));
        assert!(t.contains("传承"));
    }

    #[test]
    fn clause_digest_stable_and_distinct() {
        let a = clause_digest(&FINAL_CLAUSES.get(0).copied().unwrap_or(FinalClause { no: 0, clause: "", evidence: "", legacy: "" }));
        let b = clause_digest(&FINAL_CLAUSES.get(0).copied().unwrap_or(FinalClause { no: 0, clause: "", evidence: "", legacy: "" }));
        let c = clause_digest(&FINAL_CLAUSES.get(1).copied().unwrap_or(FinalClause { no: 0, clause: "", evidence: "", legacy: "" }));
        assert_eq!(a, b);
        assert_ne!(a, c);
        assert_ne!(a, 0);
    }

    #[test]
    fn dual_run_consistency() {
        assert!(dual_run_ok());
    }

    #[test]
    fn checks_all_green() {
        let s = run_unx1595_checks();
        assert_eq!(s.len(), 12);
        assert_eq!(s.failed_count(), 0);
        assert!(s.all_passed());
    }
}
