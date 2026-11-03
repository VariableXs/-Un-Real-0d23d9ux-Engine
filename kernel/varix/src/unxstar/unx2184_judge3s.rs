//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F2184 · 判据评估器域级扩容（三态判定） · 0x72A80
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定，CoRun·UNX-A3-B30）：
//!   ① 纯功能代码 ≥300 行；
//!   ② 判据原文：UNX-F2184-J1 评估器扩容全过（通过判据引用全域冻结值/
//!     三态判定/失败定位三件），谎报注入 10/10 检出，与 F1893 收口面扩
//!     容一致；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：全域冻结值表 16 格确定性派生，Draft→Sealed 单调封域（封后
//!   改域即冻结突变拒）；判据按 cid 派生算子（Le/Ge/Eq/Band）与域引用，
//!   出域引用落 UNKW；三态判定 PASS/FAIL/UNKW，FAIL 必带定位三件（判据
//!   号+观测+参考）；报告行五列 FNV 尾校验；谎报注入 10 路（校验和如实
//!   重算的伪造行 + 结构破坏行）审计器 10/10 检出。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x72A80，VX3-1103 评估器扩容窗口独占）。
pub const SEG_BASE: u32 = 0x72A80;
/// 全域冻结值格数（冻结 16）。
pub const DOMAIN_N: usize = 16;
/// 判据条数（冻结 12）。
pub const CRIT_N: usize = 12;
/// 评估器纪元（冻结 6）。
pub const EPOCH: u64 = 6;
/// 谎报注入轮数（判据 10/10 语义，冻结）。
pub const JUDGE_ROUNDS: u32 = 10;
/// 失败定位三件（判据号/观测值/域参考值，冻结 3）。
pub const LOCUS_N: usize = 3;
/// 出域判据号（第 9 号判据引用出域，冻结）。
pub const OUT_REF_CID: usize = 9;

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum JudgeCode {
    /// 域未冻结：域表 Draft 态即受理评估，越权评估拒
    DomainUnfrozen,
    /// 引用出域：判据引用下标越过域表边界（合法时落 UNKW 态）
    RefOut,
    /// 算子未知：比较算子只在 Le/Ge/Eq/Band 闭集内
    OpUnknown,
    /// 三态字非法：只认 PASS/FAIL/UNKW 三态字
    VerdictBad,
    /// 定位缺失：FAIL 无定位三件，或非 FAIL 却带定位
    LocusMissing,
    /// 谎报检出：报告行与冻结域重推期望不符
    FalseReport,
    /// 冻结突变：封域后改域值，冻结红线
    FrozenMutate,
    /// 双跑不一致：golden_run 两次应同值
    NotReproducible,
}

impl JudgeCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            JudgeCode::DomainUnfrozen => "域未冻结：先封域再评估，Draft 态拒评",
            JudgeCode::RefOut => "引用出域：判据引用下标越界，落 UNKW 并记录定位",
            JudgeCode::OpUnknown => "算子未知：比较算子必须落在 Le/Ge/Eq/Band 闭集",
            JudgeCode::VerdictBad => "三态字非法：只认 PASS/FAIL/UNKW，查态字拼写",
            JudgeCode::LocusMissing => "定位缺失：FAIL 必带判据号+观测+参考三件",
            JudgeCode::FalseReport => "谎报检出：报告行与冻结域重推期望不符，逐列定位",
            JudgeCode::FrozenMutate => "冻结突变：封域后改域值属冻结红线，重开新域",
            JudgeCode::NotReproducible => "双跑不一致：复现红线，golden_run 两次应同值",
        }
    }
}

/// 诊断码闭集全表（查表/段位守卫共用）。
pub const ALL_CODES: [JudgeCode; 8] = [
    JudgeCode::DomainUnfrozen, JudgeCode::RefOut, JudgeCode::OpUnknown,
    JudgeCode::VerdictBad, JudgeCode::LocusMissing, JudgeCode::FalseReport,
    JudgeCode::FrozenMutate, JudgeCode::NotReproducible,
];

/// 诊断码查表（段外码返回统一文案）。
pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < ALL_CODES.len() {
        if ALL_CODES[i].val() == code {
            return ALL_CODES[i].detail();
        }
        i += 1;
    }
    "未知诊断码：不在 0x72A80 段闭集内"
}

/// 段位守卫（全部诊断码落 0x72A80 窗口）。
pub fn segment_guard() -> bool {
    let mut i = 0;
    while i < ALL_CODES.len() {
        let v = ALL_CODES[i].val();
        if v < SEG_BASE || v >= SEG_BASE + 0x40 {
            return false;
        }
        i += 1;
    }
    true
}

// ── 评估器状态机（Draft→Armed→Sealed，单调推进） ────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum EvalPhase {
    /// 起草：域表可改，评估拒。
    Draft,
    /// 待发：判据挂载完毕，域已封，评估受理。
    Armed,
    /// 已封账：报告定版，再评估按冻结拒。
    Sealed,
}

impl EvalPhase {
    /// 单调序值：Draft=0 < Armed=1 < Sealed=2；只许前进，平移倒退拒。
    pub fn can_advance(self, to: EvalPhase) -> bool {
        (match self { EvalPhase::Draft => 0, EvalPhase::Armed => 1, EvalPhase::Sealed => 2 })
            < (match to { EvalPhase::Draft => 0, EvalPhase::Armed => 1, EvalPhase::Sealed => 2 })
    }
}

// ── 三态判定闭集 ────────────────────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Verdict {
    /// 通过：比较算子在观测值与域参考值上成立。
    Pass,
    /// 失败：比较不成立，必带定位三件。
    Fail,
    /// 未知：判据引用出域，不得带定位。
    Unkw,
}

impl Verdict {
    /// 三态字（报告行第 4 列，冻结拼写）。
    pub fn word(self) -> &'static str {
        match self { Verdict::Pass => "PASS", Verdict::Fail => "FAIL", Verdict::Unkw => "UNKW" }
    }
    /// 字面解析：闭集外返回 None。
    pub fn parse(w: &str) -> Option<Verdict> {
        match w {
            "PASS" => Some(Verdict::Pass),
            "FAIL" => Some(Verdict::Fail),
            "UNKW" => Some(Verdict::Unkw),
            _ => None,
        }
    }
}

// ── 比较算子闭集（Le/Ge/Eq/Band） ───────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CmpOp {
    /// Le：观测 ≤ 参考。
    Le,
    /// Ge：观测 ≥ 参考。
    Ge,
    /// Eq：观测 == 参考。
    Eq,
    /// Band：观测按位与参考仍含参考全位。
    Band,
}

impl CmpOp {
    /// 算子判定：成立 true。
    pub fn eval(self, obs: u64, refe: u64) -> bool {
        match self {
            CmpOp::Le => obs <= refe,
            CmpOp::Ge => obs >= refe,
            CmpOp::Eq => obs == refe,
            CmpOp::Band => (obs & refe) == refe,
        }
    }
    /// 按判据号确定性派生算子（cid%4，可复算）。
    pub fn by_cid(cid: usize) -> CmpOp {
        match cid % 4 {
            0 => CmpOp::Le,
            1 => CmpOp::Ge,
            2 => CmpOp::Eq,
            _ => CmpOp::Band,
        }
    }
}

// ── 全域冻结值表 ────────────────────────────────────────────────
/// 域表：纪元 + 16 格域值 + 封域旗；封域后改值即冻结突变拒。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DomainTable {
    pub epoch: u64,
    pub vals: Vec<u64>,
    /// true=已封域（封域后 vals 冻结）。
    pub sealed: bool,
}

impl DomainTable {
    /// 确定性建域：vals[i] = (i*17+5)%100 + 1，可复算可对拍。
    pub fn build(epoch: u64) -> DomainTable {
        let vals: Vec<u64> = (0..DOMAIN_N).map(|i| ((i * 17 + 5) % 100 + 1) as u64).collect();
        DomainTable { epoch, vals, sealed: false }
    }
    /// 封域：Draft→Sealed 单调；重复封域拒。
    pub fn seal(&mut self) -> Result<(), u32> {
        if self.sealed {
            return Err(JudgeCode::FrozenMutate.val());
        }
        self.sealed = true;
        Ok(())
    }
    /// 改域值：封域后拒（冻结红线）；下标越界拒。
    pub fn set(&mut self, idx: usize, v: u64) -> Result<(), u32> {
        if self.sealed {
            return Err(JudgeCode::FrozenMutate.val());
        }
        if idx >= self.vals.len() {
            return Err(JudgeCode::RefOut.val());
        }
        self.vals[idx] = v;
        Ok(())
    }
    /// 取域值：下标越界返回 None（判据引用出域 → UNKW）。
    pub fn get(&self, idx: usize) -> Option<u64> {
        self.vals.get(idx).copied()
    }
    /// 逐格对拍：与确定性重建一致（防暗改）。
    pub fn matches_derived(&self) -> bool {
        if self.vals.len() != DOMAIN_N {
            return false;
        }
        for (i, v) in self.vals.iter().enumerate() {
            if *v != ((i * 17 + 5) % 100 + 1) as u64 {
                return false;
            }
        }
        true
    }
}

// ── 判据与三态判定 ──────────────────────────────────────────────
/// 判据：号 + 算子 + 域引用下标 + 观测值（全确定性派生）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Criterion {
    /// 判据号（0..=11）。
    pub cid: usize,
    /// 比较算子。
    pub op: CmpOp,
    /// 域引用下标（出域判据恒 OUT_REF_CID 号引用 DOMAIN_N）。
    pub ref_idx: usize,
    /// 观测值（确定性派生；出域判据恒 0）。
    pub sample: u64,
}

/// 失败定位三件：判据号 + 观测值 + 域参考值。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Locus {
    pub cid: usize,
    pub obs: u64,
    pub refe: u64,
}

/// 确定性建判据：算子=cid%4，引用=(cid*3+1)%DOMAIN_N，出域判据单列。
pub fn build_crits() -> Vec<Criterion> {
    let mut v: Vec<Criterion> = Vec::new();
    for cid in 0..CRIT_N {
        let ref_idx = if cid == OUT_REF_CID { DOMAIN_N } else { (cid * 3 + 1) % DOMAIN_N };
        v.push(Criterion { cid, op: CmpOp::by_cid(cid), ref_idx, sample: 0 });
    }
    v
}

/// 观测值派生：命判据满足样（同参考）或违约样（按算子越界一位）。
/// cid%3==1 走违约样（造 FAIL），其余走满足样（造 PASS）。
pub fn derive_sample(op: CmpOp, refe: u64, violate: bool) -> u64 {
    if !violate {
        return refe;
    }
    match op {
        CmpOp::Le | CmpOp::Eq => refe.saturating_add(1),
        CmpOp::Ge | CmpOp::Band => refe.saturating_sub(1),
    }
}

/// 挂观测：逐判据回填派生观测值（出域判据观测恒 0）。
pub fn attach_samples(crits: &mut Vec<Criterion>, dom: &DomainTable) {
    for i in 0..crits.len() {
        if let Some(c) = crits.get_mut(i) {
            let refe = dom.get(c.ref_idx).unwrap_or(0);
            c.sample = if c.ref_idx >= DOMAIN_N { 0 } else { derive_sample(c.op, refe, i % 3 == 1) };
        }
    }
}

/// 三态判定：引用出域 → UNKW；比较成立 → PASS；不成立 → FAIL 带定位。
pub fn evaluate(c: &Criterion, dom: &DomainTable) -> Result<(Verdict, Option<Locus>), u32> {
    match dom.get(c.ref_idx) {
        None => Ok((Verdict::Unkw, None)),
        Some(refe) => {
            if c.op.eval(c.sample, refe) {
                Ok((Verdict::Pass, None))
            } else {
                Ok((Verdict::Fail, Some(Locus { cid: c.cid, obs: c.sample, refe })))
            }
        }
    }
}

// ── FNV-1a 32 位校验和（整数实现，报告行尾校验共用） ────────────
pub fn fnv1a(bytes: &[u8]) -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    for b in bytes.iter() {
        h = (h ^ (*b as u32)).wrapping_mul(0x0100_0193);
    }
    h
}

// ── 报告行（五列：判据号|观测值|域参考值|三态|校验和） ──────────
/// 单行报告：判据号隐含算子（cid%4 确定性派生），尾校验覆盖前四列。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReportRow {
    pub cid: usize,
    pub obs: u64,
    pub refe: u64,
    pub verdict: Verdict,
}

/// 渲染单行：判据号|观测值|域参考值|三态|校验和;。
pub fn render_row(r: &ReportRow) -> String {
    let prefix = format!("{}|{}|{}|{}|", r.cid, r.obs, r.refe, r.verdict.word());
    format!("{}{:08X};", prefix, fnv1a(prefix.as_bytes()))
}

/// 伪造行（谎报注入器专用）：指定观测/参考/三态，校验和如实重算
///（保证谎报行能过解析关，只能被审计关抓到）。
pub fn forge_row(cid: usize, obs: u64, refe: u64, verdict: Verdict) -> String {
    let prefix = format!("{}|{}|{}|{}|", cid, obs, refe, verdict.word());
    format!("{}{:08X};", prefix, fnv1a(prefix.as_bytes()))
}

/// 字节查找（ASCII 正文专用）。
pub fn find_byte(h: &[u8], from: usize, needle: u8) -> Option<usize> {
    h.iter().skip(from).position(|c| *c == needle).map(|p| p + from)
}

/// 子串查找（ASCII 专用）。
pub fn find_sub(h: &[u8], from: usize, pat: &[u8]) -> Option<usize> {
    if pat.is_empty() || h.len() < pat.len() {
        return None;
    }
    h.windows(pat.len()).skip(from).position(|w| w == pat).map(|p| p + from)
}

/// 十进制无符号解析（checked，防溢出）。
pub fn parse_dec(s: &str) -> Option<u64> {
    if s.is_empty() {
        return None;
    }
    let mut v: u64 = 0;
    for c in s.as_bytes().iter() {
        if *c < b'0' || *c > b'9' {
            return None;
        }
        v = v.checked_mul(10)?.checked_add((*c - b'0') as u64)?;
    }
    Some(v)
}

/// 8 位十六进制解析（校验和域专用）。
pub fn parse_hex8(s: &str) -> Option<u32> {
    if s.len() != 8 {
        return None;
    }
    let b = s.as_bytes();
    let mut v: u32 = 0;
    for i in 0..8 {
        let d = match b[i] {
            c @ b'0'..=b'9' => (c - b'0') as u32,
            c @ b'A'..=b'F' => (c - b'A') as u32 + 10,
            _ => return None,
        };
        v = (v << 4) | d;
    }
    Some(v)
}

/// 解析单行五列：判据号|观测值|域参考值|三态|校验和;。
pub fn parse_row(line: &str) -> Result<ReportRow, u32> {
    let b = line.as_bytes();
    if b.is_empty() || b[b.len() - 1] != b';' {
        return Err(JudgeCode::VerdictBad.val());
    }
    let body = &line[..b.len() - 1];
    let mut cuts: Vec<usize> = Vec::new();
    let mut i = 0usize;
    while i < body.len() {
        if let Some(p) = find_byte(body.as_bytes(), i, b'|') {
            cuts.push(p);
            i = p + 1;
        } else {
            break;
        }
    }
    if cuts.len() != 4 {
        return Err(JudgeCode::VerdictBad.val());
    }
    let (c0, c1, c2, c3) = (cuts[0], cuts[1], cuts[2], cuts[3]);
    if c0 == 0 || c1 == c0 + 1 {
        return Err(JudgeCode::VerdictBad.val());
    }
    let cid = parse_dec(&body[..c0]).ok_or(JudgeCode::VerdictBad.val())? as usize;
    if cid >= CRIT_N {
        return Err(JudgeCode::OpUnknown.val());
    }
    let obs = parse_dec(&body[c0 + 1..c1]).ok_or(JudgeCode::VerdictBad.val())?;
    let refe = parse_dec(&body[c1 + 1..c2]).ok_or(JudgeCode::VerdictBad.val())?;
    let verdict = Verdict::parse(&body[c2 + 1..c3]).ok_or(JudgeCode::VerdictBad.val())?;
    // UNKW 行不得带定位值（0/0 强制）；非 UNKW 行参考值不得为 0。
    match verdict {
        Verdict::Unkw if obs != 0 || refe != 0 => return Err(JudgeCode::LocusMissing.val()),
        Verdict::Unkw => {}
        _ if refe == 0 => return Err(JudgeCode::LocusMissing.val()),
        _ => {}
    }
    let sum = parse_hex8(&body[c3 + 1..]).ok_or(JudgeCode::VerdictBad.val())?;
    let prefix = format!("{}|{}|{}|{}|", cid, obs, refe, verdict.word());
    if fnv1a(prefix.as_bytes()) != sum {
        return Err(JudgeCode::FalseReport.val());
    }
    Ok(ReportRow { cid, obs, refe, verdict })
}

// ── 评估器（相位机 + 报告账） ───────────────────────────────────
/// 评估器：域表 + 判据表 + 相位机；Draft 态拒评，封账后拒再评。
#[derive(Clone, Debug)]
pub struct Evaluator {
    pub dom: DomainTable,
    pub crits: Vec<Criterion>,
    pub phase: EvalPhase,
    /// 报告账行表（三态落账）。
    pub rows: Vec<ReportRow>,
    /// 定位三件账（仅 FAIL）。
    pub loci: Vec<Locus>,
    /// 评估计数。
    pub evals: u32,
}

impl Evaluator {
    pub fn new(epoch: u64) -> Evaluator {
        Evaluator {
            dom: DomainTable::build(epoch),
            crits: build_crits(),
            phase: EvalPhase::Draft,
            rows: Vec::new(),
            loci: Vec::new(),
            evals: 0,
        }
    }
    /// 相位推进公开口：倒退/平移显性拒。
    pub fn advance_phase(&mut self, to: EvalPhase) -> Result<(), u32> {
        if !self.phase.can_advance(to) {
            return Err(JudgeCode::FrozenMutate.val());
        }
        self.phase = to;
        Ok(())
    }
    /// 封域：域表封 + 相位 Draft→Armed（挂观测先行）。
    pub fn arm(&mut self) -> Result<(), u32> {
        attach_samples(&mut self.crits, &self.dom);
        self.dom.seal()?;
        self.advance_phase(EvalPhase::Armed)
    }
    /// 全量评估：域未封拒；逐判据三态落账 + 定位三件落账。
    pub fn eval_all(&mut self) -> Result<(), u32> {
        if self.phase != EvalPhase::Armed {
            return Err(JudgeCode::DomainUnfrozen.val());
        }
        let dom = self.dom.clone();
        for k in 0..self.crits.len() {
            let c = match self.crits.get(k) {
                Some(c) => *c,
                None => return Err(JudgeCode::RefOut.val()),
            };
            let (verdict, locus) = evaluate(&c, &dom)?;
            let row = match (verdict, locus) {
                (Verdict::Fail, Some(l)) => {
                    self.loci.push(l);
                    ReportRow { cid: c.cid, obs: c.sample, refe: l.refe, verdict }
                }
                (Verdict::Unkw, None) => ReportRow { cid: c.cid, obs: 0, refe: 0, verdict },
                (Verdict::Pass, None) => {
                    let refe = dom.get(c.ref_idx).unwrap_or(0);
                    ReportRow { cid: c.cid, obs: c.sample, refe, verdict }
                }
                _ => return Err(JudgeCode::LocusMissing.val()),
            };
            self.rows.push(row);
            self.evals += 1;
        }
        self.advance_phase(EvalPhase::Sealed)
    }
    /// 三态账（pass, fail, unkw）。
    pub fn tri_cover(&self) -> (u32, u32, u32) {
        let mut n = [0u32; 3];
        for r in &self.rows {
            n[match r.verdict { Verdict::Pass => 0, Verdict::Fail => 1, Verdict::Unkw => 2 }] += 1;
        }
        (n[0], n[1], n[2])
    }
    /// 全账渲染。
    pub fn render_all(&self) -> String {
        let mut out = String::new();
        for r in &self.rows {
            out.push_str(&render_row(r));
            out.push('\n');
        }
        out
    }
}

// ── 谎报审计器（从冻结域重推期望，逐列对账） ────────────────────
/// 审计账文：逐行解析 + 判据序 + 重推三态/观测/参考对账 + 定位账。
pub fn audit_text(text: &str, dom: &DomainTable) -> Result<(u32, u32, u32), u32> {
    if text.is_empty() {
        return Err(JudgeCode::FalseReport.val());
    }
    if !dom.sealed || !dom.matches_derived() {
        return Err(JudgeCode::DomainUnfrozen.val());
    }
    let mut crits = build_crits();
    attach_samples(&mut crits, dom);
    let mut rows: Vec<ReportRow> = Vec::new();
    let bts = text.as_bytes();
    let mut i = 0usize;
    while i < bts.len() {
        let end = match find_byte(bts, i, b'\n') {
            Some(p) => p,
            None => bts.len(),
        };
        let line = &text[i..end];
        if !line.is_empty() {
            rows.push(parse_row(line)?);
        }
        i = end.saturating_add(1);
    }
    if rows.len() != CRIT_N {
        return Err(JudgeCode::FalseReport.val());
    }
    let mut n = [0u32; 3];
    for (k, pair) in rows.iter().zip(crits.iter()).enumerate() {
        let (r, c) = pair;
        if r.cid != k {
            return Err(JudgeCode::FalseReport.val());
        }
        let (want_v, want_locus) = evaluate(c, dom)?;
        if r.verdict != want_v {
            return Err(JudgeCode::FalseReport.val());
        }
        let slot = match want_v {
            Verdict::Unkw => {
                if r.obs != 0 || r.refe != 0 {
                    return Err(JudgeCode::LocusMissing.val());
                }
                2
            }
            Verdict::Pass => {
                let refe_exp = dom.get(c.ref_idx).ok_or(JudgeCode::RefOut.val())?;
                if r.obs != c.sample || r.refe != refe_exp {
                    return Err(JudgeCode::FalseReport.val());
                }
                0
            }
            Verdict::Fail => {
                // 定位三件：判据号 + 观测值 + 域参考值逐件对账。
                let l = match want_locus {
                    Some(l) => l,
                    None => return Err(JudgeCode::LocusMissing.val()),
                };
                if r.obs != l.obs || r.refe != l.refe || l.cid != k {
                    return Err(JudgeCode::LocusMissing.val());
                }
                1
            }
        };
        n[slot] += 1;
    }
    Ok((n[0], n[1], n[2]))
}

/// 审计账本本体：渲染→解析→三态账与本体对拍 + 定位账核。
pub fn audit_ledger(ev: &Evaluator) -> Result<(), u32> {
    let (p, f, u) = audit_text(&ev.render_all(), &ev.dom)?;
    let (pp, ff, uu) = ev.tri_cover();
    if p != pp || f != ff || u != uu {
        return Err(JudgeCode::FalseReport.val());
    }
    // 定位账：FAIL 数 == 定位数，且逐件判据号/观测/参考与行一致。
    if ev.loci.len() != ff as usize {
        return Err(JudgeCode::LocusMissing.val());
    }
    for l in &ev.loci {
        let r = match ev.rows.get(l.cid) {
            Some(r) => r,
            None => return Err(JudgeCode::LocusMissing.val()),
        };
        if r.verdict != Verdict::Fail || r.obs != l.obs || r.refe != l.refe {
            return Err(JudgeCode::LocusMissing.val());
        }
    }
    if ev.phase != EvalPhase::Sealed || ev.evals != CRIT_N as u32 {
        return Err(JudgeCode::DomainUnfrozen.val());
    }
    Ok(())
}

// ── 谎报注入（10 路，谎报行校验和如实重算，只能被审计关抓） ─────
/// 黄金账文行区间 [lo, hi)（hi 为行尾换行下标）。
fn row_span(text: &str, k: usize) -> (usize, usize) {
    let b = text.as_bytes();
    let mut lo = 0usize;
    let mut idx = 0usize;
    let mut i = 0usize;
    while i < b.len() {
        if b[i] == b'\n' {
            if idx == k {
                return (lo, i);
            }
            idx += 1;
            lo = i + 1;
        }
        i += 1;
    }
    (lo, b.len())
}

/// 行替换：删第 k 行原文，原位落伪造行（带换行）。
fn replace_row(text: &str, k: usize, forged: &str) -> String {
    let (lo, hi) = row_span(text, k);
    let mut out = String::from(&text[..lo]);
    out.push_str(forged);
    out.push('\n');
    out.push_str(&text[hi + 1..]);
    out
}

/// 行内单字节改写：第 k 行定位 pat，其后第 off 字节改写。
fn patch_in_row(text: &str, k: usize, pat: &[u8], off: usize, newc: u8) -> String {
    let (lo, hi) = row_span(text, k);
    let seg = &text[lo..hi];
    let tgt = match find_sub(seg.as_bytes(), 0, pat) {
        Some(p) => p + off,
        None => usize::MAX,
    };
    let mut out = String::from(&text[..lo]);
    let mut i = 0usize;
    while i < seg.len() {
        if i == tgt {
            out.push(newc as char);
        } else {
            out.push_str(&seg[i..i + 1]);
        }
        i += 1;
    }
    out.push_str(&text[hi..]);
    out
}

/// 行对调：第 a/b 行互换（判据序破坏注入）。
fn swap_rows(text: &str, a: usize, b2: usize) -> String {
    let (a0, a1) = row_span(text, a);
    let (b0, b1) = row_span(text, b2);
    let mut out = String::new();
    out.push_str(&text[b0..b1 + 1]);
    out.push_str(&text[a0..a1 + 1]);
    out.push_str(&text[b1 + 1..]);
    out
}

/// 行复制：第 k 行复制一份（判据序重复注入）。
fn dup_row(text: &str, k: usize) -> String {
    let (lo, hi) = row_span(text, k);
    let mut out = String::from(&text[..hi + 1]);
    out.push_str(&text[lo..hi + 1]);
    out.push_str(&text[hi + 1..]);
    out
}

/// 删行：第 k 行删除（行数不足注入）。
fn drop_row(text: &str, k: usize) -> String {
    let (lo, hi) = row_span(text, k);
    let mut out = String::from(&text[..lo]);
    out.push_str(&text[hi + 1..]);
    out
}

/// 对黄金报告账文注入第 kind 路谎报，返回坏账文（全 ASCII，安全）。
pub fn distort(text: &str, kind: usize) -> String {
    // 黄金三态位：行 0=PASS(obs=23,ref=23)，行 1=FAIL(73,74)，行 9=UNKW。
    match kind % 10 {
        0 => replace_row(text, 1, &forge_row(1, 73, 74, Verdict::Pass)), // 败判充过。
        1 => replace_row(text, 0, &forge_row(0, 23, 23, Verdict::Fail)), // 过判充败。
        2 => replace_row(text, 9, &forge_row(9, 0, 0, Verdict::Pass)), // 出域充过。
        3 => replace_row(text, 0, &forge_row(0, 24, 23, Verdict::Pass)), // 改观测。
        4 => replace_row(text, 2, &forge_row(2, 25, 26, Verdict::Pass)), // 改参考。
        5 => replace_row(text, 9, &forge_row(9, 1, 2, Verdict::Unkw)), // 出域带值。
        6 => drop_row(text, 5), // 删行：行数不足必抓。
        7 => swap_rows(text, 0, 1), // 换序：判据序被破坏必抓。
        8 => dup_row(text, 3), // 复制行：判据序重复必抓。
        _ => patch_in_row(text, 4, b"|FAIL|", 5, b'X'), // 野态字：FAILX 闭外必抓。
    }
}

// ── 注入检出账 ──────────────────────────────────────────────────
pub struct InjectLedger {
    pub tries: u32,
    pub caught: u32,
    /// 漏抓的谎报路号（诊断用）。
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

/// 谎报注入轮：每轮注入一路谎报，审计器必须检出（黄金文先审计通过）。
pub fn inject_rounds(rounds: u32) -> Result<InjectLedger, u32> {
    let mut acc = InjectLedger::new();
    for r in 0..rounds {
        let mut ev = Evaluator::new(EPOCH + r as u64);
        ev.arm()?;
        ev.eval_all()?;
        let text = ev.render_all();
        audit_text(&text, &ev.dom)?;
        let kind = (r % 10) as usize;
        acc.record(kind, audit_text(&distort(&text, kind), &ev.dom).is_err());
    }
    Ok(acc)
}

// ── 黄金全链 ────────────────────────────────────────────────────
/// 全链：建域封域 → 三态评估 → 审计对账 → 定位三件 → 四拒探。
/// 返回 (正旗数, 拒旗数, 三态账, 定位数)。
pub fn golden_run() -> Result<(u32, u32, (u32, u32, u32), usize), u32> {
    let ev = golden_eval()?;
    audit_ledger(&ev)?;
    let tri = ev.tri_cover();
    // 三态账冻结：(7 过, 4 败, 1 出域)。
    let tri_ok = tri == (7, 4, 1);
    // 定位三件：FAIL 逐件带账（判据号+观测+参考）。
    let locus_ok = ev.loci.len() == tri.1 as usize
        && ev.loci.iter().all(|l| l.obs != l.refe || l.refe == 0);
    // 单行谎报探针：伪造关可过、审计关必抓。
    let forge_ok = match ev.loci.first() {
        Some(l) => {
            let bad = forge_row(l.cid, l.obs, l.refe, Verdict::Pass);
            parse_row(&bad).is_ok() && audit_text(&format!("{}\n", bad), &ev.dom).is_err()
        }
        None => false,
    };
    // 四拒探：Draft 态评估 / 封域改值 / 出域充过 / 野态字。
    let mut ev2 = Evaluator::new(9);
    let draft_rej = ev2.eval_all().is_err();
    let mut ev3 = Evaluator::new(9);
    ev3.arm()?;
    let mutate_rej = ev3.dom.set(3, 99).is_err();
    let out_rej = audit_text(&format!("{}\n", forge_row(9, 0, 0, Verdict::Pass)), &ev3.dom).is_err();
    let word_rej = Verdict::parse("PASSX").is_none();
    let flags = tri_ok as u32 + locus_ok as u32 + forge_ok as u32 + ev.dom.matches_derived() as u32;
    let rejects = draft_rej as u32 + mutate_rej as u32 + out_rej as u32 + word_rej as u32;
    Ok((flags, rejects, tri, ev.loci.len()))
}

/// 黄金评估器装填（建域封域 + 评估，供全链/判据面复用）。
pub fn golden_eval() -> Result<Evaluator, u32> {
    let mut ev = Evaluator::new(EPOCH);
    ev.arm()?;
    ev.eval_all()?;
    Ok(ev)
}

// ── 双跑一致（复现红线） ────────────────────────────────────────
pub fn repro_check() -> Result<(), u32> {
    let a = golden_run()?;
    let b = golden_run()?;
    if a != b {
        return Err(JudgeCode::NotReproducible.val());
    }
    Ok(())
}

// ── 相位机与算子守卫探针 ────────────────────────────────────────
pub fn phase_guard() -> bool {
    let mut ev = Evaluator::new(EPOCH);
    let m1 = ev.advance_phase(EvalPhase::Armed).is_ok();
    let m2 = ev.advance_phase(EvalPhase::Draft).is_err();
    let m3 = ev.advance_phase(EvalPhase::Armed).is_err();
    let m4 = ev.advance_phase(EvalPhase::Sealed).is_ok();
    // 算子四路：Le/Ge/Eq/Band 逐路判定。
    let o1 = CmpOp::Le.eval(5, 6) && !CmpOp::Le.eval(7, 6);
    let o2 = CmpOp::Ge.eval(7, 6) && !CmpOp::Ge.eval(5, 6);
    let o3 = CmpOp::Eq.eval(6, 6) && !CmpOp::Eq.eval(7, 6);
    let o4 = CmpOp::Band.eval(7, 6) && !CmpOp::Band.eval(5, 6);
    // 算子派生守卫：by_cid 闭集内确定性。
    let d1 = CmpOp::by_cid(0) == CmpOp::Le && CmpOp::by_cid(11) == CmpOp::Band;
    m1 && m2 && m3 && m4 && o1 && o2 && o3 && o4 && d1
}

// ── 完成写回（判据③：状态=已完成、结果摘要非空） ────────────────
pub fn writeback_summary() -> Result<String, u32> {
    let ev = golden_eval()?;
    audit_ledger(&ev)?;
    let (p, f, u) = ev.tri_cover();
    Ok(format!(
        "状态=已完成 domain={} crits={} pass={} fail={} unkw={} loci={} evals={}",
        DOMAIN_N, CRIT_N, p, f, u, ev.loci.len(), ev.evals
    ))
}

// ── 判据面 ──────────────────────────────────────────────────────
pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut v: Vec<(&'static str, bool, String)> = Vec::new();
    // J1 主判据：黄金全链（四正四拒 + 三态账 (7,4,1)）。
    let g = golden_run();
    let (gp, gd) = match &g {
        Ok((f, r, tri, l)) => (*f == 4 && *r == 4, format!("flags={} rejects={} tri={:?} loci={}", f, r, tri, l)),
        Err(e) => (false, format!("err=0x{:x} {}", e, detail(*e))),
    };
    v.push(("J1-评估器扩容全过", gp, gd));
    // J1 全域冻结值引用：域已封且逐格与派生口径一致。
    let ev = match golden_eval() {
        Ok(e) => e,
        Err(e) => {
            v.push(("J1-全域冻结值引用", false, format!("err=0x{:x}", e)));
            return v;
        }
    };
    v.push(("J1-全域冻结值引用", ev.dom.sealed && ev.dom.matches_derived(),
        format!("epoch={} vals={} sealed={}", ev.dom.epoch, ev.dom.vals.len(), ev.dom.sealed)));
    // J1 三态判定：三态齐且账面冻结 (7,4,1)。
    let tri = ev.tri_cover();
    v.push(("J1-三态判定", tri == (7, 4, 1), format!("pass={} fail={} unkw={}", tri.0, tri.1, tri.2)));
    // J1 失败定位三件：FAIL 逐件带账。
    let locus_ok = ev.loci.len() == tri.1 as usize
        && ev.loci.iter().all(|l| l.obs != l.refe || l.refe == 0);
    v.push(("J1-失败定位三件", locus_ok, format!("loci={} fail={}", ev.loci.len(), tri.1)));
    // J1 谎报注入 10/10 检出。
    let inj = inject_rounds(JUDGE_ROUNDS);
    let (ip, id) = match &inj {
        Ok(l) => (l.all_caught(), format!("t={} c={} miss={:?}", l.tries, l.caught, l.miss_kinds)),
        Err(e) => (false, format!("err=0x{:x}", e)),
    };
    v.push(("J1-谎报注入10/10", ip, id));
    // J2 单调守卫：相位机四探 + 封域改值拒 + 封域重封拒 + 封账再评拒。
    let mut ev2 = Evaluator::new(EPOCH);
    let a1 = ev2.advance_phase(EvalPhase::Armed).is_ok();
    let _ = ev2.arm();
    let a2 = ev2.dom.set(0, 1).is_err();
    let a3 = ev2.dom.seal().is_err();
    let _ = ev2.eval_all();
    let a4 = ev2.eval_all().is_err();
    v.push(("J2-单调守卫", a1 && a2 && a3 && a4,
        format!("相位进={a1} 封域改值拒={a2} 重封拒={a3} 封账再评拒={a4}")));
    // J2 相位/算子守卫。
    v.push(("J2-相位算子守卫", phase_guard(), String::from("相位单调四探 + 算子四路判定")));
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
    v.push(("J4-段位守卫", segment_guard(), String::from("0x72A80 窗口")));
    v
}

pub fn run_unx2184_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F2184 判据评估器域级扩容（三态判定）");
    for (n, p, d) in extra_checks() {
        s.add_dyn(n, p, &d);
    }
    s
}
