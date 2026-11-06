//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F2264 · 销账路径规划（每笔欠账销法） · 0x72B80
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定，CoRun·UNX-A3-B34）：
//!   ① 纯功能代码 ≥300 行；
//!   ② 判据原文：UNX-F2264-J1 销账规划全过（每笔欠账带销账路径：
//!     销法/销账闸门/责任批三列——闸门挂接可解析），路径缺失欠账为
//!     零断言过，伪路径注入 10/10 拒绝；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! ─────────────────────────────────────────────────────────────────────
//! 设计要点：三列路径 = 每笔欠账挂接（销法, 销账闸门, 责任批）三元组；
//!   销法资格 = 四销法（全额冲销/分期摊销/对冲互抵/挂账转期）按欠账
//!   种类与金额资格判定，错配即伪路径拒；闸门 DSL = 可解析表达式
//!   AMT>=n / AMT<n / LOCK=0|1 / SEQ<=n 与 & 组合，不可解析或永闭
//!   （矛盾闸）即拒；责任批 = 批号闭集 B34-S0..S3 一种一批，未知或
//!   错配即拒；覆盖断言 = 24 笔逐笔恰一条路径；伪路径注入 10 路
//!   （销法错配/残式/矛盾闸/幽灵批/批种错配/账号越界/重挂/未知键/
//!   空式/幻号），核验器 10/10 拒绝。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x72B80，VX3-1103 销账规划窗口独占）。
pub const SEG_BASE: u32 = 0x72B80;
/// 欠账笔数（冻结 24）。
pub const DEBT_N: usize = 24;
/// 欠账种数（本金/利息/罚息/费用，冻结 4）。
pub const KIND_N: usize = 4;
/// 销法种数（全额冲销/分期摊销/对冲互抵/挂账转期，冻结 4）。
pub const METHOD_N: usize = 4;
/// 闸门单式最大条款数（冻结 4）。
pub const GATE_MAX_CLAUSES: usize = 4;
/// 欠账纪元（冻结 7）。
pub const EPOCH: u64 = 7;
/// 伪路径注入路数（判据 10/10，冻结）。
pub const FAKE_ROUNDS: u32 = 10;
/// 责任批前缀（B34 规划批）。
pub const BATCH_ROOT: &str = "B34";
/// 分期摊销金额阈值（本金 ≥1000 走分期，冻结）。
pub const INSTALLMENT_THRESHOLD: i64 = 1000;
/// 欠账金额下界（建账闭式，冻结 300）。
pub const AMOUNT_BASE: i64 = 300;
/// 欠账金额跨度（建账闭式，冻结 1800）。
pub const AMOUNT_SPAN: i64 = 1800;

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DebtCode {
    /// 销法不适格：挂接销法与欠账种类/金额资格规则不符
    MethodIneligible,
    /// 闸门不可解析：表达式残缺、未知键或超长条款
    GateUnparsable,
    /// 矛盾闸：闸门可解析但对该欠账永闭（死路径）
    GateContradiction,
    /// 责任批未知：批号不在 B34-S0..S3 登记闭集内
    BatchUnknown,
    /// 批种错配：责任批与欠账种类不对应
    BatchKindMismatch,
    /// 欠账未知/未覆盖：账号不在册或逐笔无路径挂接
    DebtUncovered,
    /// 路径重挂：同一欠账挂接多条路径
    PathDuplicate,
    /// 双跑不一致：复现红线，golden_run 两次应同值
    NotReproducible,
}

impl DebtCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            DebtCode::MethodIneligible => "销法不适格：按种类与金额重选销法（本金大额分期/利息成对对冲/罚息转期/小额冲销）",
            DebtCode::GateUnparsable => "闸门不可解析：只认 AMT>=n/AMT<n/LOCK=0|1/SEQ<=n 四式与 & 组合",
            DebtCode::GateContradiction => "矛盾闸：闸门对该欠账永闭，改阈值或换销法",
            DebtCode::BatchUnknown => "责任批未知：批号必须在 B34-S0..S3 登记闭集内",
            DebtCode::BatchKindMismatch => "批种错配：一种一批，按欠账种类改挂对应责任批",
            DebtCode::DebtUncovered => "欠账未知/未覆盖：账号 1..=24 逐笔必须恰挂一条路径",
            DebtCode::PathDuplicate => "路径重挂：同一欠账只许一条销账路径，去重后重挂",
            DebtCode::NotReproducible => "双跑不一致：复现红线，golden_run 两次应同值",
        }
    }
    pub const ALL: [DebtCode; 8] = [
        DebtCode::MethodIneligible,
        DebtCode::GateUnparsable,
        DebtCode::GateContradiction,
        DebtCode::BatchUnknown,
        DebtCode::BatchKindMismatch,
        DebtCode::DebtUncovered,
        DebtCode::PathDuplicate,
        DebtCode::NotReproducible,
    ];
}

/// 诊断码查表（段外码返回统一文案）。
pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < DebtCode::ALL.len() {
        if DebtCode::ALL[i].val() == code {
            return DebtCode::ALL[i].detail();
        }
        i += 1;
    }
    "未知诊断码：不在 0x72B80 段闭集内"
}

// ── 域模型：欠账种类与销法 ──────────────────────────────────────
/// 欠账种类（下标即 i%4 建账序）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DebtKind {
    /// 本金。
    Principal,
    /// 利息。
    Interest,
    /// 罚息。
    Penalty,
    /// 费用。
    Fee,
}

impl DebtKind {
    pub fn idx(self) -> usize {
        match self {
            DebtKind::Principal => 0,
            DebtKind::Interest => 1,
            DebtKind::Penalty => 2,
            DebtKind::Fee => 3,
        }
    }
    pub fn from_idx(i: usize) -> DebtKind {
        match i % KIND_N {
            1 => DebtKind::Interest,
            2 => DebtKind::Penalty,
            3 => DebtKind::Fee,
            _ => DebtKind::Principal,
        }
    }
    /// 中文名（诊断与摘要用）。
    pub fn label(self) -> &'static str {
        match self {
            DebtKind::Principal => "本金",
            DebtKind::Interest => "利息",
            DebtKind::Penalty => "罚息",
            DebtKind::Fee => "费用",
        }
    }
}

/// 销法（销账路径第一列）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PayMethod {
    /// 全额冲销：小额一次清。
    FullOffset,
    /// 分期摊销：大额本金分批销。
    Installment,
    /// 对冲互抵：成对利息与本金相抵。
    CrossNetting,
    /// 挂账转期：罚息转期缓销。
    HoldConvert,
}

impl PayMethod {
    pub fn idx(self) -> usize {
        match self {
            PayMethod::FullOffset => 0,
            PayMethod::Installment => 1,
            PayMethod::CrossNetting => 2,
            PayMethod::HoldConvert => 3,
        }
    }
    pub fn from_idx(i: usize) -> PayMethod {
        match i % METHOD_N {
            1 => PayMethod::Installment,
            2 => PayMethod::CrossNetting,
            3 => PayMethod::HoldConvert,
            _ => PayMethod::FullOffset,
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            PayMethod::FullOffset => "全额冲销",
            PayMethod::Installment => "分期摊销",
            PayMethod::CrossNetting => "对冲互抵",
            PayMethod::HoldConvert => "挂账转期",
        }
    }
}

/// 欠账：账号（1 起）、种类、金额、成对旗（利息配本金）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Debt {
    pub id: u64,
    pub kind: DebtKind,
    pub amount: i64,
    pub paired: bool,
}

/// 建欠账册（确定性闭式：账号=下标+1，种类=下标%4，金额=300+(下标×137 %1800)，
/// 成对旗=下标%6==1，恰命中利息位）。
pub fn build_debts() -> Vec<Debt> {
    let mut out: Vec<Debt> = Vec::new();
    for i in 0..DEBT_N {
        out.push(Debt {
            id: (i + 1) as u64,
            kind: DebtKind::from_idx(i),
            amount: AMOUNT_BASE + ((i * 137) as i64) % AMOUNT_SPAN,
            paired: i % 6 == 1,
        });
    }
    out
}

/// 销法资格规则（唯一定义点）：
///   本金 ≥1000 → 分期摊销，否则全额冲销；
///   利息成对 → 对冲互抵，否则全额冲销；
///   罚息 → 挂账转期；
///   费用 → 全额冲销。
pub fn choose_method(d: &Debt) -> PayMethod {
    match d.kind {
        DebtKind::Principal => {
            if d.amount >= INSTALLMENT_THRESHOLD {
                PayMethod::Installment
            } else {
                PayMethod::FullOffset
            }
        }
        DebtKind::Interest => {
            if d.paired {
                PayMethod::CrossNetting
            } else {
                PayMethod::FullOffset
            }
        }
        DebtKind::Penalty => PayMethod::HoldConvert,
        DebtKind::Fee => PayMethod::FullOffset,
    }
}

/// 锁旗（闸门 LOCK 域的取值来源）：成对利息锁开，其余按账号奇偶。
pub fn lock_of(d: &Debt) -> u32 {
    if d.kind == DebtKind::Interest {
        d.paired as u32
    } else {
        (d.id % 2) as u32
    }
}

// ── 闸门 DSL（销账路径第二列） ──────────────────────────────────
/// 闸门条款：四式闭集。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GateClause {
    /// AMT>=n：金额下限。
    AmtGe(i64),
    /// AMT<n：金额上限。
    AmtLt(i64),
    /// LOCK=0|1：锁旗等值。
    LockEq(u32),
    /// SEQ<=n：账号序上限。
    SeqLe(u64),
}

/// 闸门上下文：求值输入。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GateCtx {
    pub amount: i64,
    pub lock: u32,
    pub seq: u64,
}

/// 十进制 i64 解析（checked，防溢出，只认无符号 + 负号）。
pub fn parse_i64(s: &str) -> Option<i64> {
    if s.is_empty() {
        return None;
    }
    let (neg, body) = if let Some(rest) = s.strip_prefix('-') {
        (true, rest)
    } else {
        (false, s)
    };
    if body.is_empty() {
        return None;
    }
    let b = body.as_bytes();
    let mut v: i64 = 0;
    let mut i = 0usize;
    while i < b.len() {
        if b[i] < b'0' || b[i] > b'9' {
            return None;
        }
        v = v.checked_mul(10)?.checked_add((b[i] - b'0') as i64)?;
        i += 1;
    }
    if neg {
        v = v.checked_neg()?;
    }
    Some(v)
}

/// 解析单条款：AMT>=n / AMT<n / LOCK=0|1 / SEQ<=n，其余一律 None。
pub fn parse_clause(tok: &str) -> Option<GateClause> {
    if let Some(rest) = tok.strip_prefix("AMT>=") {
        return parse_i64(rest).map(GateClause::AmtGe);
    }
    if let Some(rest) = tok.strip_prefix("AMT<") {
        return parse_i64(rest).map(GateClause::AmtLt);
    }
    if let Some(rest) = tok.strip_prefix("LOCK=") {
        return match rest {
            "0" => Some(GateClause::LockEq(0)),
            "1" => Some(GateClause::LockEq(1)),
            _ => None,
        };
    }
    if let Some(rest) = tok.strip_prefix("SEQ<=") {
        return parse_i64(rest).and_then(|v| {
            if v >= 0 {
                Some(GateClause::SeqLe(v as u64))
            } else {
                None
            }
        });
    }
    None
}

/// 解析闸门（& 组合，条款数 ≤ GATE_MAX_CLAUSES，空式/未知键即败）。
pub fn parse_gate(s: &str) -> Option<Vec<GateClause>> {
    if s.is_empty() {
        return None;
    }
    let mut out: Vec<GateClause> = Vec::new();
    let mut rest = s;
    loop {
        let (tok, tail) = match rest.find('&') {
            Some(p) => {
                let (a, b) = rest.split_at(p);
                (a, &b[1..])
            }
            None => (rest, ""),
        };
        if tok.is_empty() || out.len() >= GATE_MAX_CLAUSES {
            return None;
        }
        out.push(parse_clause(tok)?);
        if tail.is_empty() {
            break;
        }
        rest = tail;
    }
    Some(out)
}

/// 闸门求值：全条款 AND；任一不满足即闭。
pub fn eval_gate(clauses: &[GateClause], ctx: &GateCtx) -> Result<bool, u32> {
    if clauses.is_empty() {
        return Err(DebtCode::GateUnparsable.val());
    }
    let mut open = true;
    for c in clauses {
        let ok = match c {
            GateClause::AmtGe(n) => ctx.amount >= *n,
            GateClause::AmtLt(n) => ctx.amount < *n,
            GateClause::LockEq(v) => ctx.lock == *v,
            GateClause::SeqLe(n) => ctx.seq <= *n,
        };
        if !ok {
            open = false;
        }
    }
    Ok(open)
}

/// 销账闸门构建（唯一定义点，与销法资格规则一一对应）：
///   全额冲销 → AMT>=300；分期摊销 → AMT>=1000；
///   对冲互抵 → LOCK=1；挂账转期 → AMT<3000。
pub fn build_gate(method: PayMethod) -> String {
    match method {
        PayMethod::FullOffset => String::from("AMT>=300"),
        PayMethod::Installment => String::from("AMT>=1000"),
        PayMethod::CrossNetting => String::from("LOCK=1"),
        PayMethod::HoldConvert => String::from("AMT<3000"),
    }
}

// ── 责任批登记（销账路径第三列） ────────────────────────────────
/// 责任批登记闭集：一种一批（B34-S0..S3）。
pub const RESP_BATCHES: [&str; KIND_N] = ["B34-S0", "B34-S1", "B34-S2", "B34-S3"];

/// 种类 → 责任批。
pub fn batch_for_kind(k: DebtKind) -> String {
    match RESP_BATCHES.get(k.idx()) {
        Some(b) => String::from(*b),
        None => String::new(),
    }
}

/// 批号在册判定。
pub fn batch_registered(b: &str) -> bool {
    let mut i = 0;
    while i < RESP_BATCHES.len() {
        if let Some(x) = RESP_BATCHES.get(i) {
            if *x == b {
                return true;
            }
        }
        i += 1;
    }
    false
}

// ── 销账路径（三列三元组） ──────────────────────────────────────
/// 销账路径：账号 + 销法 + 闸门（原式与解析式双存）+ 责任批。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PayPath {
    pub debt_id: u64,
    pub method: PayMethod,
    pub gate_raw: String,
    pub gate: Vec<GateClause>,
    pub batch_id: String,
}

/// 规划器：逐笔欠账生成三列路径；闸门必须可解析。
/// 返回路径表；任何一笔失败即整体拒（规划零缺失断言的前提）。
pub fn plan_paths(debts: &[Debt]) -> Result<Vec<PayPath>, u32> {
    let mut out: Vec<PayPath> = Vec::new();
    for d in debts {
        let method = choose_method(d);
        let gate_raw = build_gate(method);
        let gate = match parse_gate(&gate_raw) {
            Some(g) => g,
            None => return Err(DebtCode::GateUnparsable.val()),
        };
        out.push(PayPath {
            debt_id: d.id,
            method,
            gate_raw,
            gate,
            batch_id: batch_for_kind(d.kind),
        });
    }
    Ok(out)
}

/// 路径覆盖断言：欠账 1..=N 逐笔恰一条路径，缺一或重挂即拒。
pub fn coverage_check(debts: &[Debt], paths: &[PayPath]) -> Result<u32, u32> {
    if paths.len() != debts.len() {
        return Err(DebtCode::DebtUncovered.val());
    }
    let mut seen: Vec<bool> = Vec::new();
    for _ in 0..debts.len() + 1 {
        seen.push(false);
    }
    for p in paths {
        if p.debt_id == 0 || p.debt_id > debts.len() as u64 {
            return Err(DebtCode::DebtUncovered.val());
        }
        let slot = (p.debt_id - 1) as usize;
        let flag = match seen.get(slot) {
            Some(x) => *x,
            None => return Err(DebtCode::DebtUncovered.val()),
        };
        if flag {
            return Err(DebtCode::PathDuplicate.val());
        }
        if let Some(f) = seen.get_mut(slot) {
            *f = true;
        }
    }
    for (j, f) in seen.iter().enumerate() {
        if j < debts.len() && !*f {
            return Err(DebtCode::DebtUncovered.val());
        }
    }
    Ok(paths.len() as u32)
}

/// 单路径核验（伪路径拒绝核心）：
///   账号在册 → 销法适格 → 闸门可解析 → 闸门对该欠账开 → 批在册 → 批种对应。
pub fn validate_path(debts: &[Debt], p: &PayPath) -> Result<(), u32> {
    if p.debt_id == 0 || p.debt_id > debts.len() as u64 {
        return Err(DebtCode::DebtUncovered.val());
    }
    let d = match debts.get((p.debt_id - 1) as usize) {
        Some(x) => x,
        None => return Err(DebtCode::DebtUncovered.val()),
    };
    if p.method != choose_method(d) {
        return Err(DebtCode::MethodIneligible.val());
    }
    let gate = match parse_gate(&p.gate_raw) {
        Some(g) => g,
        None => return Err(DebtCode::GateUnparsable.val()),
    };
    let ctx = GateCtx { amount: d.amount, lock: lock_of(d), seq: d.id };
    match eval_gate(&gate, &ctx) {
        Ok(true) => {}
        Ok(false) => return Err(DebtCode::GateContradiction.val()),
        Err(e) => return Err(e),
    }
    if !batch_registered(&p.batch_id) {
        return Err(DebtCode::BatchUnknown.val());
    }
    let want = batch_for_kind(d.kind);
    if p.batch_id != want {
        return Err(DebtCode::BatchKindMismatch.val());
    }
    Ok(())
}

/// 全表核验：覆盖断言 + 逐路径核验 + 重挂检查。
pub fn validate_paths_all(debts: &[Debt], paths: &[PayPath]) -> Result<u32, u32> {
    coverage_check(debts, paths)?;
    for p in paths {
        validate_path(debts, p)?;
    }
    Ok(paths.len() as u32)
}

// ── 规划引擎（状态机：Open→Planned→Settled，单调推进） ──────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PlanState {
    /// 开账：欠账在册，未规划。
    LedgerOpen,
    /// 已规划：逐笔路径挂接完毕。
    Planned,
    /// 已销清：规划执行收官，再规划即拒。
    Settled,
}

impl PlanState {
    pub fn rank(self) -> u8 {
        match self {
            PlanState::LedgerOpen => 0,
            PlanState::Planned => 1,
            PlanState::Settled => 2,
        }
    }
    pub fn can_advance(self, to: PlanState) -> bool {
        to.rank() > self.rank()
    }
}

/// 规划引擎：状态机 + 规划/核验记账。
#[derive(Clone, Debug)]
pub struct PlanEngine {
    pub state: PlanState,
    pub epoch: u64,
    /// 已完成核验轮数。
    pub validations: u32,
    /// 累计拒绝的伪路径轮数。
    pub fakes_caught: u32,
}

impl PlanEngine {
    pub fn new(epoch: u64) -> PlanEngine {
        PlanEngine { state: PlanState::LedgerOpen, epoch, validations: 0, fakes_caught: 0 }
    }

    /// 状态推进公开口：倒退/平移显性拒。
    pub fn advance(&mut self, to: PlanState) -> Result<(), u32> {
        if !self.state.can_advance(to) {
            return Err(DebtCode::NotReproducible.val());
        }
        self.state = to;
        Ok(())
    }

    /// 规划口：Open/Planned 态可规划（幂等）；Settled 后规划即拒。
    pub fn plan(&mut self, debts: &[Debt]) -> Result<Vec<PayPath>, u32> {
        if self.state.rank() >= PlanState::Settled.rank() {
            return Err(DebtCode::DebtUncovered.val());
        }
        let paths = plan_paths(debts)?;
        coverage_check(debts, &paths)?;
        if self.state.rank() < PlanState::Planned.rank() {
            self.state = PlanState::Planned;
        }
        Ok(paths)
    }

    /// 销清口：仅 Planned 态可销清。
    pub fn settle(&mut self) -> Result<(), u32> {
        if self.state.rank() < PlanState::Planned.rank() {
            return Err(DebtCode::DebtUncovered.val());
        }
        self.advance(PlanState::Settled)
    }

    /// 伪路径核验记账。
    pub fn note_fake(&mut self, caught: bool) {
        self.validations += 1;
        if caught {
            self.fakes_caught += 1;
        }
    }
}

// ── 伪路径注入（10 路） ─────────────────────────────────────────
/// 对黄金路径表注入第 kind 路伪路径，返回伪表。
/// 十路语义：
///   0 销法错配；1 残式（AMT>>>）；2 矛盾闸；3 幽灵批；
///   4 批种错配；5 账号越界（0 号）；6 重挂（复制首条）；
///   7 未知键（GHOST=1）；8 空式；9 幻号（999 号）。
pub fn fake_paths(paths: &[PayPath], kind: usize) -> Vec<PayPath> {
    let mut out: Vec<PayPath> = Vec::new();
    for p in paths {
        out.push(p.clone());
    }
    let k = kind % 10;
    if k == 6 {
        // 重挂：复制首条路径（同账号双挂）。
        if let Some(p0) = paths.get(0) {
            out.push(p0.clone());
        }
        return out;
    }
    let slot = match k {
        5 | 9 => 5usize,
        _ => k,
    };
    if let Some(p) = out.get_mut(slot) {
        match k {
            0 => p.method = PayMethod::HoldConvert,
            1 => p.gate_raw = String::from("AMT>>>"),
            2 => p.gate_raw = String::from("AMT>=9999&AMT<1"),
            3 => p.batch_id = String::from("B99-Z9"),
            4 => p.batch_id = String::from("B34-S3"),
            5 => p.debt_id = 0,
            7 => p.gate_raw = String::from("GHOST=1"),
            8 => p.gate_raw = String::new(),
            _ => p.debt_id = 999,
        }
    }
    out
}

/// 注入轮：每轮先验黄金路径表（核验必须通过，防注入器污染基准），
/// 再注入一路伪路径，核验器必须拒绝。
pub fn fake_rounds(rounds: u32) -> Result<InjectLedger, u32> {
    let debts = build_debts();
    let paths = plan_paths(&debts)?;
    validate_paths_all(&debts, &paths)?;
    let mut out = InjectLedger::new();
    let mut r = 0u32;
    while r < rounds {
        let kind = (r % 10) as usize;
        let fake = fake_paths(&paths, kind);
        out.record(kind, validate_paths_all(&debts, &fake).is_err());
        r += 1;
    }
    Ok(out)
}

/// 伪路径检出账：tries/caught + 漏抓路号。
pub struct InjectLedger {
    pub tries: u32,
    pub caught: u32,
    /// 漏抓的伪路径路号（诊断用）。
    pub miss_kinds: Vec<usize>,
}

impl InjectLedger {
    pub fn new() -> InjectLedger {
        InjectLedger { tries: 0, caught: 0, miss_kinds: Vec::new() }
    }
    pub fn record(&mut self, kind: usize, hit: bool) {
        self.tries += 1;
        if hit {
            self.caught += 1;
        } else {
            self.miss_kinds.push(kind);
        }
    }
    pub fn all_caught(&self) -> bool {
        self.tries > 0 && self.caught == self.tries
    }
}

// ── 黄金全链 ────────────────────────────────────────────────────
/// 全链：建册→规划→覆盖断言→全表核验→销清→四拒探→伪路径 10/10。
/// 返回 (正旗数, 拒旗数, 路径数, 检出伪路径数)。
pub fn golden_run() -> Result<(u32, u32, u32, u32), u32> {
    let debts = build_debts();
    let mut eng = PlanEngine::new(EPOCH);
    let paths = eng.plan(&debts)?;
    let cov = coverage_check(&debts, &paths)?;
    let ok_cnt = validate_paths_all(&debts, &paths)?;
    eng.settle()?;
    // 四拒探：销清后再规划 / 状态倒退 / 残式伪路径 / 矛盾闸伪路径。
    let replan_rej = {
        let mut e2 = PlanEngine::new(EPOCH);
        let _ = e2.plan(&debts)?;
        let _ = e2.settle();
        e2.plan(&debts).is_err()
    };
    let back_rej = eng.advance(PlanState::Planned).is_err();
    let fake1 = fake_paths(&paths, 1);
    let broken_rej = validate_paths_all(&debts, &fake1).is_err();
    let fake2 = fake_paths(&paths, 2);
    let dead_rej = validate_paths_all(&debts, &fake2).is_err();
    // 伪路径 10 路全拒。
    let inj = fake_rounds(FAKE_ROUNDS)?;
    let inj_ok = inj.all_caught();
    let flags = (cov == DEBT_N as u32) as u32
        + (ok_cnt == DEBT_N as u32) as u32
        + (paths.len() == DEBT_N) as u32
        + inj_ok as u32;
    let rejects = replan_rej as u32 + back_rej as u32 + broken_rej as u32 + dead_rej as u32;
    Ok((flags, rejects, paths.len() as u32, inj.caught))
}

// ── 探针三序 ────────────────────────────────────────────────────
/// 序 A：黄金全链四正四拒全中。
pub fn probe_seq_a() -> Result<(), u32> {
    match golden_run() {
        Ok((4, 4, _, _)) => Ok(()),
        _ => Err(DebtCode::NotReproducible.val()),
    }
}

/// 序 B：伪路径注入 10 路全拒。
pub fn probe_seq_b() -> Result<(), u32> {
    match fake_rounds(FAKE_ROUNDS) {
        Ok(l) if l.all_caught() => Ok(()),
        _ => Err(DebtCode::MethodIneligible.val()),
    }
}

/// 序 C：冻结常量与段位窗口。
pub fn probe_seq_c() -> Result<(), u32> {
    let in_window = SEG_BASE == 0x72B80 && DebtCode::NotReproducible.val() < SEG_BASE + 0x40;
    // 责任批闭集自证：四批号互异且都带 B34 前缀。
    let mut batches_ok = RESP_BATCHES.len() == KIND_N;
    for a in 0..RESP_BATCHES.len() {
        let x = match RESP_BATCHES.get(a) { Some(s) => *s, None => break };
        if !x.starts_with(BATCH_ROOT) {
            batches_ok = false;
        }
        for b in (a + 1)..RESP_BATCHES.len() {
            if let Some(y) = RESP_BATCHES.get(b) {
                if x == *y {
                    batches_ok = false;
                }
            }
        }
    }
    let ok = DEBT_N == 24 && KIND_N == 4 && METHOD_N == 4 && GATE_MAX_CLAUSES == 4
        && EPOCH == 7 && FAKE_ROUNDS == 10 && batches_ok && in_window;
    if ok {
        Ok(())
    } else {
        Err(DebtCode::NotReproducible.val())
    }
}

// ── 双跑一致（复现红线） ────────────────────────────────────────
pub fn repro_check() -> Result<(), u32> {
    let a = golden_run()?;
    let b = golden_run()?;
    if a != b {
        return Err(DebtCode::NotReproducible.val());
    }
    Ok(())
}

// ── 段位守卫 ────────────────────────────────────────────────────
pub fn segment_guard() -> bool {
    let mut i = 0;
    while i < DebtCode::ALL.len() {
        let v = DebtCode::ALL[i].val();
        if v < SEG_BASE || v >= SEG_BASE + 0x40 {
            return false;
        }
        i += 1;
    }
    true
}

// ── 闸门 DSL 探针（建式→解析→求值往返） ────────────────────────
pub fn gate_roundtrip() -> bool {
    // 四式逐条：建式→解析→求值方向正确。
    let ctx_hi = GateCtx { amount: 3500, lock: 1, seq: 25 };
    let ctx_lo = GateCtx { amount: 300, lock: 0, seq: 24 };
    let a = match parse_gate("AMT>=1000") { Some(g) => matches!(eval_gate(&g, &ctx_hi), Ok(true)) && matches!(eval_gate(&g, &ctx_lo), Ok(false)), None => false };
    let b = match parse_gate("AMT<3000") { Some(g) => matches!(eval_gate(&g, &ctx_lo), Ok(true)) && matches!(eval_gate(&g, &ctx_hi), Ok(false)), None => false };
    let c = match parse_gate("LOCK=1") { Some(g) => matches!(eval_gate(&g, &ctx_hi), Ok(true)) && matches!(eval_gate(&g, &ctx_lo), Ok(false)), None => false };
    let d = match parse_gate("SEQ<=24") { Some(g) => matches!(eval_gate(&g, &ctx_lo), Ok(true)) && matches!(eval_gate(&g, &ctx_hi), Ok(false)), None => false };
    // 破损式四拒：残式 / 未知键 / 空式 / 超长条款。
    let bad = parse_gate("AMT>>>").is_none()
        && parse_gate("GHOST=1").is_none()
        && parse_gate("").is_none()
        && parse_gate("AMT>=1&AMT>=2&AMT>=3&AMT>=4&AMT>=5").is_none();
    a && b && c && d && bad
}

// ── 完成写回（判据③：状态=已完成、结果摘要非空） ────────────────
pub fn writeback_summary() -> Result<String, u32> {
    let debts = build_debts();
    let mut eng = PlanEngine::new(EPOCH);
    let paths = eng.plan(&debts)?;
    let cnt = validate_paths_all(&debts, &paths)?;
    eng.settle()?;
    let inj = fake_rounds(FAKE_ROUNDS)?;
    Ok(format!(
        "状态=已完成 paths={} covered={} fakes={}/{} state=Settled",
        cnt, DEBT_N, inj.caught, inj.tries
    ))
}

// ── 判据面 ──────────────────────────────────────────────────────
pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut v: Vec<(&'static str, bool, String)> = Vec::new();
    // J1 主判据：黄金全链（四正四拒）。
    let g = golden_run();
    let (gp, gd) = match &g {
        Ok((f, r, p, c)) => (*f == 4 && *r == 4, format!("flags={} rejects={} paths={} caught={}", f, r, p, c)),
        Err(e) => (false, format!("err=0x{:x} {}", e, detail(*e))),
    };
    v.push(("J1-销账规划全过", gp, gd));
    // 建册与黄金路径表（后续分项判据共用）。
    let debts = build_debts();
    let paths = match plan_paths(&debts) {
        Ok(p) => p,
        Err(e) => {
            v.push(("J1-三列路径挂接", false, format!("err=0x{:x}", e)));
            return v;
        }
    };
    // J1 三列路径挂接：24 笔逐笔销法/闸门/责任批齐备。
    let mut tri_ok = paths.len() == DEBT_N;
    for (i, d) in debts.iter().enumerate() {
        let p = match paths.get(i) { Some(x) => x, None => break };
        if p.debt_id != d.id || p.gate_raw.is_empty() || p.batch_id.is_empty() {
            tri_ok = false;
            break;
        }
    }
    v.push(("J1-三列路径挂接", tri_ok, format!("销法/闸门/责任批 三列 × {} 笔", DEBT_N)));
    // J1 路径缺失欠账为零（覆盖断言）。
    let cov = coverage_check(&debts, &paths);
    v.push(("J1-路径缺失为零", matches!(&cov, Ok(n) if *n == DEBT_N as u32), format!("covered={}/{}", DEBT_N, DEBT_N)));
    // J1 全表核验通过（含闸门逐笔开）。
    let ok_cnt = validate_paths_all(&debts, &paths);
    v.push(("J1-全表核验通过", matches!(&ok_cnt, Ok(n) if *n == DEBT_N as u32), String::from("销法适格+闸门开+批种对应 逐笔全过")));
    // J1 伪路径注入 10/10。
    let inj = fake_rounds(FAKE_ROUNDS);
    let (ip, id) = match &inj {
        Ok(l) => (l.all_caught(), format!("t={} c={} miss={:?}", l.tries, l.caught, l.miss_kinds)),
        Err(e) => (false, format!("err=0x{:x}", e)),
    };
    v.push(("J1-伪路径注入10/10", ip, id));
    // J1 闸门 DSL 往返。
    v.push(("J1-闸门DSL往返", gate_roundtrip(), String::from("四式建→析→求值；残式/未知键/空式/超长必败")));
    // J2 状态机单调守卫。
    let mut eng = PlanEngine::new(EPOCH);
    let m1 = eng.advance(PlanState::Planned).is_ok();
    let m2 = eng.advance(PlanState::Planned).is_err();
    let m3 = eng.advance(PlanState::LedgerOpen).is_err();
    let m4 = eng.advance(PlanState::Settled).is_ok();
    v.push(("J2-状态机单调守卫", m1 && m2 && m3 && m4, format!("进={} 平拒={} 倒拒={} 终进={}", m1, m2, m3, m4)));
    // J2 销清后再规划拒。
    let mut eng2 = PlanEngine::new(EPOCH);
    let _ = eng2.plan(&debts);
    let _ = eng2.settle();
    let replan_rej = eng2.plan(&debts).is_err();
    let settle_rej = {
        let mut e3 = PlanEngine::new(EPOCH);
        e3.settle().is_err()
    };
    v.push((
        "J2-销清守卫",
        replan_rej && settle_rej,
        format!("销清再规划拒={} 未规划销清拒={}", replan_rej, settle_rej),
    ));
    // J2 双跑一致。
    v.push(("J2-双跑一致", repro_check().is_ok(), String::from("golden_run 两跑同值")));
    // J3 写回摘要非空。
    let wb = writeback_summary();
    let (wp, wd) = match &wb {
        Ok(s) => (!s.is_empty(), s.clone()),
        Err(e) => (false, format!("err=0x{:x}", e)),
    };
    v.push(("J3-写回摘要", wp, wd));
    // J4 段位守卫。
    v.push(("J4-段位守卫", segment_guard(), String::from("0x72B80 窗口")));
    v
}

pub fn run_unx2264_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F2264 销账路径规划（每笔欠账销法）");
    for (n, p, d) in extra_checks() {
        s.add_dyn(n, p, &d);
    }
    s
}
