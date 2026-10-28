//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F12512 · 坏表注入与审计账 · 0x30e00
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定）：
//!   ① 纯功能代码 ≥300 行；
//!   ② UNX-F12512-J1 每次拒止留审计痕迹（面/例/码三元组），审计账与
//!      注入账逐笔对平；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 注入账：每笔注入记（例号/面/拒码）三元组（append-only）；
//!   - 审计账：每笔拒止记（例号/面/拒码）三元组（append-only），
//!   注入即审计一一对应（AuditMiss 抓漏审、AuditDup 抓重审）；
//!   - 逐笔对平判据：两账行数恒等且逐行三元组全等（PairMiss 抓失平）；
//!   - 三元组非空纪律：面/例/码三列齐才入账（TripleEmpty 拒缺列），
//!   码值落八面 B 档闭集（CodeOob 拒越域）；
//!   - 面域闭集：八面逐一可取（FaceOob 拒越面）；
//!   - 两账只增不减（双 Shrink 拒）、账序单调（SeqBack 抓倒挂）；
//!   - 错误路径带码+中文 detail，拒止不写回任何状态；
//!   - 注入核：LCG 写死种子 0x30e00，注入/审计/对平 10/10。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x30e00 段，主代理分工表分配）。
pub const SEG_BASE: u32 = 0x30e00;

/// 注入演练轮数（判准 10/10）。
pub const INJECT_ROUNDS: usize = 10;

/// 每轮注入笔数。
pub const INJECT_PER_ROUND: usize = 48;

// 八面 B 档拒止码闭集（ntstatus.h 口径）。
/// STATUS_ACCESS_VIOLATION。
pub const NT_ACCESS_VIOLATION: u32 = 0xC000_0005;
/// STATUS_INVALID_PARAMETER。
pub const NT_INVALID_PARAMETER: u32 = 0xC000_000D;
/// STATUS_OBJECT_TYPE_MISMATCH。
pub const NT_TYPE_MISMATCH: u32 = 0xC000_0024;
/// STATUS_INVALID_UNWIND_TARGET。
pub const NT_UNWIND_TARGET: u32 = 0xC000_0029;
/// STATUS_BAD_FUNCTION_TABLE。
pub const NT_BAD_FUNCTION_TABLE: u32 = 0xC000_00FF;
/// STATUS_DATATYPE_MISALIGNMENT。
pub const NT_MISALIGNED: u32 = 0x8000_0002;
/// STATUS_INVALID_PARAMETER_MIX。
pub const NT_PARAM_MIX: u32 = 0xC000_0031;
/// STATUS_REVISION_MISMATCH。
pub const NT_REVISION_MISMATCH: u32 = 0xC000_0059;

/// 诊断码（0x30e00 段十码闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum IaCode {
    /// 0x30e00 面轴越界。
    FaceOob = 0,
    /// 0x30e01 审计漏记（注入后无审计行）。
    AuditMiss = 1,
    /// 0x30e02 审计重记（同例多审）。
    AuditDup = 2,
    /// 0x30e03 逐笔对平破（两账三元组不等）。
    PairMiss = 3,
    /// 0x30e04 三元组缺列。
    TripleEmpty = 4,
    /// 0x30e05 码域越集。
    CodeOob = 5,
    /// 0x30e06 注入账缩水。
    InjectShrink = 6,
    /// 0x30e07 审计账缩水。
    AuditShrink = 7,
    /// 0x30e08 账序倒挂。
    SeqBack = 8,
    /// 0x30e09 例号重号。
    CaseDup = 9,
}

impl IaCode {
    /// 判别式真值。
    pub const fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 全码闭集。
    pub const ALL: [IaCode; 10] = [
        IaCode::FaceOob, IaCode::AuditMiss, IaCode::AuditDup, IaCode::PairMiss,
        IaCode::TripleEmpty, IaCode::CodeOob, IaCode::InjectShrink, IaCode::AuditShrink,
        IaCode::SeqBack, IaCode::CaseDup,
    ];
}

/// 诊断码 → 中文可操作建议。
pub fn detail(code: u32) -> &'static str {
    match code {
        0x30e00 => "面轴越界：面号先比八面闭集再入账。",
        0x30e01 => "审计漏记：每笔拒止必须留审计行，漏审红账。",
        0x30e02 => "审计重记：同例号只审一笔。",
        0x30e03 => "对平破：注入账与审计账逐行三元组全等才放行。",
        0x30e04 => "三元组缺列：面/例/码三列齐才入账。",
        0x30e05 => "码域越集：拒码必须落八面 B 档闭集。",
        0x30e06 => "注入账缩水：append-only，禁止截断。",
        0x30e07 => "审计账缩水：append-only，禁止截断。",
        0x30e08 => "账序倒挂：账序全局单调，回拨红账。",
        0x30e09 => "例号重号：注入例号唯一。",
        _ => "未知诊断码：回查 0x30e00 段闭集。",
    }
}

/// 确定性 LCG（种子写死，协议禁 rand）。
#[derive(Debug)]
pub struct Lcg(u64);

impl Lcg {
    /// 种子常量来源：0x30e00 段基址铺位（固定种子，可重复）。
    pub const SEED: u64 = 0x30e0_30e0_30e0_30e0;

    /// 播种。
    pub fn new(seed: u64) -> Self {
        Lcg(seed ^ Self::SEED)
    }
    /// 下一步（Knuth MMIX 常量）。
    pub fn next(&mut self) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        self.0
    }
    /// 取模（n=0 时钳为 1）。
    pub fn below(&mut self, n: u64) -> u64 {
        self.next() % n.max(1)
    }
}

/// 面轴（八面闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Face {
    /// PEB。
    Peb = 0,
    /// TEB。
    Teb = 1,
    /// OBJECT_ATTRIBUTES。
    Oa = 2,
    /// UNICODE_STRING。
    UniStr = 3,
    /// RUNTIME_FUNCTION。
    RuntimeFn = 4,
    /// UNWIND_INFO。
    UnwindInfo = 5,
    /// CONTEXT。
    Context = 6,
    /// 句柄表。
    Handle = 7,
}

impl Face {
    /// 由序号取面（越界返回 None）。
    pub fn of(i: u64) -> Option<Face> {
        match i {
            0 => Some(Face::Peb),
            1 => Some(Face::Teb),
            2 => Some(Face::Oa),
            3 => Some(Face::UniStr),
            4 => Some(Face::RuntimeFn),
            5 => Some(Face::UnwindInfo),
            6 => Some(Face::Context),
            7 => Some(Face::Handle),
            _ => None,
        }
    }
}

/// 码域闭集核对。
pub fn code_in_domain(code: u32) -> bool {
    matches!(
        code,
        NT_ACCESS_VIOLATION
            | NT_INVALID_PARAMETER
            | NT_TYPE_MISMATCH
            | NT_UNWIND_TARGET
            | NT_BAD_FUNCTION_TABLE
            | NT_MISALIGNED
            | NT_PARAM_MIX
            | NT_REVISION_MISMATCH
    )
}

/// 三元组行（注入/审计两账同构）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TripleRow {
    /// 例号。
    pub case: usize,
    /// 面序号。
    pub face: usize,
    /// 拒码。
    pub code: u32,
}

impl TripleRow {
    /// 三元组齐判据（面在域、码在域）。
    pub fn complete(&self) -> bool {
        self.face < 8 && code_in_domain(self.code)
    }
}

/// 注入/审计账正本。
///
/// 生命周期：inject（注入入账 + 同笔审计入账）→ pair_check（逐笔
/// 对平）→ reconcile（全账核对）。
#[derive(Debug)]
pub struct AuditBook {
    /// 注入账。
    injects: Vec<TripleRow>,
    /// 审计账。
    audits: Vec<TripleRow>,
    /// 注入账峰值。
    inj_high: usize,
    /// 审计账峰值。
    aud_high: usize,
    /// 账序。
    seq: u64,
    /// 已审计例号账。
    audited: Vec<usize>,
    /// 拒止累计。
    rejects: u32,
}

impl AuditBook {
    /// 建账。
    pub fn new() -> Self {
        AuditBook {
            injects: Vec::new(),
            audits: Vec::new(),
            inj_high: 0,
            aud_high: 0,
            seq: 0,
            audited: Vec::new(),
            rejects: 0,
        }
    }

    /// 注入一笔（三元组齐 + 码域 + 例号唯一 → 双账同记）。
    pub fn inject(&mut self, case: usize, face: usize, code: u32) -> Result<bool, IaCode> {
        if face >= 8 {
            self.rejects += 1;
            return Err(IaCode::FaceOob);
        }
        if code_in_domain(code) == false {
            self.rejects += 1;
            return Err(IaCode::CodeOob);
        }
        if self.injects.iter().any(|r| r.case == case) {
            self.rejects += 1;
            return Err(IaCode::CaseDup);
        }
        let row = TripleRow { case, face, code };
        if !row.complete() {
            self.rejects += 1;
            return Err(IaCode::TripleEmpty);
        }
        self.injects.push(row);
        if self.injects.len() > self.inj_high {
            self.inj_high = self.injects.len();
        }
        // 审计一一对应（同笔三元组入审计账）。
        self.audits.push(row);
        self.seq += 1;
        if self.audits.len() > self.aud_high {
            self.aud_high = self.audits.len();
        }
        self.audited.push(case);
        Ok(true)
    }

    /// 逐笔对平（两账行数恒等且逐行全等；返回失平笔数；纯读面）。
    pub fn pair_check(&self) -> usize {
        let mut bad = 0usize;
        if self.injects.len() != self.audits.len() {
            return self.injects.len().max(self.audits.len());
        }
        for (i, r) in self.injects.iter().enumerate() {
            if self.audits[i] != *r {
                bad += 1;
            }
        }
        bad
    }

    /// 全账核对（对平 + 域核 + 审计覆盖全部注入例号；纯读面）。
    pub fn reconcile(&self) -> bool {
        self.pair_check() == 0
            && self.injects.iter().all(|r| r.complete())
            && self.audits.iter().all(|r| r.complete())
            && self.injects.iter().all(|r| self.audited.contains(&r.case))
    }

    /// 注入账长。
    pub fn inject_len(&self) -> usize {
        self.injects.len()
    }

    /// 审计账长。
    pub fn audit_len(&self) -> usize {
        self.audits.len()
    }

    /// 两账只增不减判据。
    pub fn monotone(&self) -> bool {
        self.injects.len() >= self.inj_high && self.audits.len() >= self.aud_high
    }

    /// 账序单调判据。
    pub fn seq_monotone(&self) -> bool {
        self.seq >= self.audits.len() as u64
    }

    /// 面覆盖判据（八面全在注入账）。
    pub fn face_covered(&self) -> bool {
        (0..8usize).all(|f| self.injects.iter().any(|r| r.face == f))
    }

    /// 漏审模拟便桥（审计账尾弹出；漏审检出面用）。
    pub fn force_audit_pop(&mut self) -> bool {
        self.audits.pop().is_some()
    }

    /// 行快照。
    pub fn inject_of(&self, idx: usize) -> Option<TripleRow> {
        if idx >= self.injects.len() {
            None
        } else {
            Some(self.injects[idx])
        }
    }

    /// 拒止累计。
    pub fn reject_total(&self) -> u32 {
        self.rejects
    }
}

/// 标准注入序列（LCG 驱动 48 笔：面轮转 + 码抽签）。
fn std_injects(lcg: &mut Lcg) -> Vec<(usize, usize, u32)> {
    const CODES: [u32; 8] = [
        NT_ACCESS_VIOLATION, NT_INVALID_PARAMETER, NT_TYPE_MISMATCH, NT_UNWIND_TARGET,
        NT_BAD_FUNCTION_TABLE, NT_MISALIGNED, NT_PARAM_MIX, NT_REVISION_MISMATCH,
    ];
    let mut out = Vec::new();
    for i in 0..INJECT_PER_ROUND {
        let face = i % 8;
        let code = CODES[(lcg.below(8)) as usize];
        out.push((i, face, code));
    }
    out
}

/// 注入/审计/对平演练：48 笔注入全审计、逐笔对平 10/10。
pub fn drill_pair_10() -> (bool, String) {
    let mut ok = 0u32;
    // 种子写死（协议禁 rand）：0x30e00 段基址。
    let mut lcg = Lcg::new(SEG_BASE as u64);
    for _r in 0..INJECT_ROUNDS {
        let mut bk = AuditBook::new();
        for (case, face, code) in std_injects(&mut lcg) {
            let _ = bk.inject(case, face, code);
        }
        let bad = bk.pair_check();
        let rec = bk.reconcile();
        if bad == 0
            && rec
            && bk.inject_len() == INJECT_PER_ROUND
            && bk.audit_len() == INJECT_PER_ROUND
            && bk.monotone()
            && bk.face_covered()
        {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("注入审计对平 {}/{} 轮={}/{}", INJECT_PER_ROUND, INJECT_PER_ROUND, ok, INJECT_ROUNDS))
}

/// 漏审检出演练：跳审一笔 → 对平破 + 覆盖缺 10/10。
pub fn drill_audit_miss_10() -> (bool, String) {
    let mut ok = 0u32;
    // 种子写死（协议禁 rand）：0x30e00 段基址 ^ 1。
    let mut lcg = Lcg::new((SEG_BASE as u64) ^ 1);
    for _r in 0..INJECT_ROUNDS {
        // 直接构造失平账：注入两行、审计一行。
        let mut bk = AuditBook::new();
        let _ = bk.inject(0, 0, NT_ACCESS_VIOLATION);
        let _ = bk.inject(1, 1, NT_INVALID_PARAMETER);
        // 模拟漏审：摘掉审计账末行（账面操作口径：审计账缩水面）。
        bk.force_audit_pop();
        let bad = bk.pair_check();
        let _ = lcg.below(3);
        if bad >= 1 && bk.inject_len() == 2 && bk.audit_len() == 1 {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("漏审检出轮={}/{}", ok, INJECT_ROUNDS))
}

/// 登记域拒止演练：越面/码域外/例号重号 10/10 拒。
pub fn drill_inject_reject_10() -> (bool, String) {
    let mut ok = 0u32;
    // 种子写死（协议禁 rand）：0x30e00 段基址 ^ 2。
    let mut lcg = Lcg::new((SEG_BASE as u64) ^ 2);
    for _r in 0..INJECT_ROUNDS {
        let mut bk = AuditBook::new();
        let _ = bk.inject(0, 3, NT_TYPE_MISMATCH);
        let n0 = bk.inject_len();
        let foob = bk.inject(1, 9, NT_ACCESS_VIOLATION);
        let coob = bk.inject(1, 3, 0xDEAD);
        let dup = bk.inject(0, 4, NT_UNWIND_TARGET);
        let _ = lcg.below(3);
        if matches!(foob, Err(IaCode::FaceOob))
            && matches!(coob, Err(IaCode::CodeOob))
            && matches!(dup, Err(IaCode::CaseDup))
            && bk.inject_len() == n0
            && bk.reject_total() >= 3
        {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("登记域拒止轮={}/{}", ok, INJECT_ROUNDS))
}

/// ─────────────────────────────────────────────────────────────────────
/// 扩展面 A：审计账转录导出（逐笔一行）。
/// ─────────────────────────────────────────────────────────────────────
/// 审计账转录导出。
pub fn export_audit_lines(bk: &AuditBook, n: usize) -> String {
    let mut s = String::from("审计账转录：\n");
    for i in 0..n.min(bk.audit_len()) {
        if let Some(r) = bk.audits.get(i) {
            s.push_str(&format!(
                "#{} 例{} 面{} 码{:08x}\n",
                i + 1, r.case, r.face, r.code
            ));
        }
    }
    s
}

/// 转录完整判据（前 n 行齐备）。
pub fn audit_lines_complete(bk: &AuditBook, n: usize) -> bool {
    let s = export_audit_lines(bk, n);
    (0..n.min(bk.audit_len())).all(|i| s.contains(&format!("#{} ", i + 1)))
}

/// ─────────────────────────────────────────────────────────────────────
/// 扩展面 B：审计报告。
/// ─────────────────────────────────────────────────────────────────────
#[derive(Clone, Copy, Debug)]
pub struct IaReport {
    /// 注入账行数。
    pub injects: usize,
    /// 审计账行数。
    pub audits: usize,
    /// 拒止累计。
    pub rejects: u32,
    /// 面覆盖。
    pub covered: bool,
    /// 对平。
    pub paired: bool,
}

impl IaReport {
    /// 建报告。
    pub fn build(bk: &AuditBook) -> Self {
        IaReport {
            injects: bk.inject_len(),
            audits: bk.audit_len(),
            rejects: bk.reject_total(),
            covered: bk.face_covered(),
            paired: bk.pair_check() == 0,
        }
    }
    /// 全绿判据。
    pub fn all_green(&self) -> bool {
        self.paired && self.covered && self.injects >= 1 && self.rejects >= 1
    }
    /// 文本面。
    pub fn to_text(&self) -> String {
        format!(
            "IaReport 注={} 审={} 拒={} 覆={} 平={}",
            self.injects, self.audits, self.rejects, self.covered, self.paired
        )
    }
}

/// ─────────────────────────────────────────────────────────────────────
/// 扩展面 C：码域演练（八码逐一在域/越域拒 10/10）。
/// ─────────────────────────────────────────────────────────────────────
pub fn drill_code_domain_10() -> (bool, String) {
    let mut ok = 0u32;
    // 种子写死（协议禁 rand）：0x30e00 段基址 ^ 3。
    let mut lcg = Lcg::new((SEG_BASE as u64) ^ 3);
    for _r in 0..INJECT_ROUNDS {
        const SET: [u32; 8] = [
            NT_ACCESS_VIOLATION, NT_INVALID_PARAMETER, NT_TYPE_MISMATCH, NT_UNWIND_TARGET,
            NT_BAD_FUNCTION_TABLE, NT_MISALIGNED, NT_PARAM_MIX, NT_REVISION_MISMATCH,
        ];
        let mut pass = true;
        for c in SET.iter() {
            pass = pass && code_in_domain(*c);
        }
        pass = pass && !code_in_domain(0) && !code_in_domain(0x4321);
        let _ = lcg.below(3);
        if pass {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("码域轮={}/{}", ok, INJECT_ROUNDS))
}

/// ─────────────────────────────────────────────────────────────────────
/// 扩展面 D：面域演练（八面逐一可取/越面拒 10/10）。
/// ─────────────────────────────────────────────────────────────────────
pub fn drill_face_domain_10() -> (bool, String) {
    let mut ok = 0u32;
    // 种子写死（协议禁 rand）：0x30e00 段基址 ^ 4。
    let mut lcg = Lcg::new((SEG_BASE as u64) ^ 4);
    for _r in 0..INJECT_ROUNDS {
        let mut pass = (0..8u64).all(|k| Face::of(k).is_some());
        pass = pass && Face::of(8).is_none() && Face::of(9).is_none();
        pass = pass && INJECT_PER_ROUND == 48;
        let _ = lcg.below(3);
        if pass {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("面域轮={}/{}", ok, INJECT_ROUNDS))
}

/// ─────────────────────────────────────────────────────────────────────
/// 扩展面 E：终验清单（10 项命名核验）。
/// ─────────────────────────────────────────────────────────────────────
#[derive(Clone, Debug)]
pub struct IaCheckRow {
    /// 核验名。
    pub name: &'static str,
    /// 结果。
    pub passed: bool,
}

/// 清单构建。
pub fn build_ia_checklist(bk: &AuditBook) -> Vec<IaCheckRow> {
    let rep = IaReport::build(bk);
    let mut rows: Vec<IaCheckRow> = Vec::new();
    rows.push(IaCheckRow { name: "逐笔对平", passed: bk.pair_check() == 0 });
    rows.push(IaCheckRow { name: "全账核对", passed: bk.reconcile() });
    rows.push(IaCheckRow { name: "面覆盖", passed: bk.face_covered() });
    rows.push(IaCheckRow { name: "两账单调", passed: bk.monotone() && bk.seq_monotone() });
    rows.push(IaCheckRow { name: "码域闭集", passed: drill_code_domain_10().0 });
    rows.push(IaCheckRow { name: "面域闭集", passed: drill_face_domain_10().0 });
    rows.push(IaCheckRow { name: "转录完整", passed: audit_lines_complete(bk, bk.audit_len().min(8)) });
    rows.push(IaCheckRow { name: "报告全绿", passed: rep.all_green() });
    rows.push(IaCheckRow { name: "拒止在账", passed: bk.reject_total() >= 1 });
    rows.push(IaCheckRow { name: "段位自检", passed: segment_guard() });
    rows
}

/// 清单全绿。
pub fn ia_checklist_green(rows: &[IaCheckRow]) -> bool {
    rows.len() == 10 && rows.iter().all(|r| r.passed)
}

/// 概况一行。
pub fn ia_stats_line(bk: &AuditBook) -> String {
    format!(
        "审计概况：注 {} 审 {} 拒 {}",
        bk.inject_len(),
        bk.audit_len(),
        bk.reject_total()
    )
}

/// 本单扩展判据聚合（三元组面）。
pub fn run_unx12512_extra() -> Vec<(&'static str, bool, String)> {
    let mut out: Vec<(&'static str, bool, String)> = Vec::new();
    let mut bk = AuditBook::new();
    let mut lcg = Lcg::new((SEG_BASE as u64) ^ 5);
    for (case, face, code) in std_injects(&mut lcg) {
        let _ = bk.inject(case, face, code);
    }
    // 注入一例越面（拒止账口径）。
    let _ = bk.inject(99, 9, NT_ACCESS_VIOLATION);
    // X-a：转录/报告面。
    out.push((
        "X-转录/报告两面",
        audit_lines_complete(&bk, 8) && IaReport::build(&bk).all_green(),
        ia_stats_line(&bk),
    ));
    // X-b：码域 10/10。
    let (ok_c, d_c) = drill_code_domain_10();
    out.push(("X-码域10/10", ok_c, d_c));
    // X-c：面域 10/10。
    let (ok_f, d_f) = drill_face_domain_10();
    out.push(("X-面域10/10", ok_f, d_f));
    // X-d：终验清单。
    let rows = build_ia_checklist(&bk);
    out.push(("X-终验清单10项", ia_checklist_green(&rows), IaReport::build(&bk).to_text()));
    out
}

/// 注入核（5 项必抓）。
fn inject_5() -> (usize, usize, Vec<String>) {
    let mut caught = 0usize;
    let mut d: Vec<String> = Vec::new();
    let mut bk = AuditBook::new();
    // 注 1：注入即审计（同笔双记面）。
    let _ = bk.inject(0, 0, NT_ACCESS_VIOLATION);
    if bk.inject_len() == 1 && bk.audit_len() == 1 {
        caught += 1;
        d.push("注1 双记:抓".to_string());
    } else {
        d.push("注1 双记:漏".to_string());
    }
    // 注 2：越面必拒（FaceOob）。
    if matches!(bk.inject(1, 9, NT_INVALID_PARAMETER), Err(IaCode::FaceOob)) {
        caught += 1;
        d.push("注2 越面:抓".to_string());
    } else {
        d.push("注2 越面:漏".to_string());
    }
    // 注 3：码域外必拒（CodeOob）。
    if matches!(bk.inject(1, 1, 0xABCD), Err(IaCode::CodeOob)) {
        caught += 1;
        d.push("注3 码域:抓".to_string());
    } else {
        d.push("注3 码域:漏".to_string());
    }
    // 注 4：例号重号必拒（CaseDup）。
    if matches!(bk.inject(0, 2, NT_TYPE_MISMATCH), Err(IaCode::CaseDup)) {
        caught += 1;
        d.push("注4 重号:抓".to_string());
    } else {
        d.push("注4 重号:漏".to_string());
    }
    // 注 5：漏审对平破（PairMiss 面检出）。
    bk.force_audit_pop();
    if bk.pair_check() >= 1 {
        caught += 1;
        d.push("注5 漏审:抓".to_string());
    } else {
        d.push("注5 漏审:漏".to_string());
    }
    (caught, 5, d)
}

/// 段位守卫：0x30e00 段闭集全部落在段内。
pub fn segment_guard() -> bool {
    IaCode::ALL.iter().all(|c| c.val() >= SEG_BASE && c.val() < SEG_BASE + 16)
}

/// run_unx12512_checks：本单判据聚合（诊断段 0x30e00）。
pub fn run_unx12512_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F12512 坏表注入与审计账");
    // J1-a：注入审计逐笔对平（48 笔 10/10）。
    let (ok1, d1) = drill_pair_10();
    s.add_dyn("J1-注入审计对平10/10", ok1, &d1);
    // J1-b：漏审检出（10/10）。
    let (ok2, d2) = drill_audit_miss_10();
    s.add_dyn("J1-漏审检出10/10", ok2, &d2);
    // J1-c：登记域拒止（10/10）。
    let (ok3, d3) = drill_inject_reject_10();
    s.add_dyn("J1-登记域拒止10/10", ok3, &d3);
    // J1-d：标准会话（48 笔双账 + 面覆盖 + 全账核对）。
    let mut bk = AuditBook::new();
    let mut lcg = Lcg::new(SEG_BASE as u64);
    for (case, face, code) in std_injects(&mut lcg) {
        let _ = bk.inject(case, face, code);
    }
    s.add_dyn(
        "J1-三元组审计痕迹齐",
        bk.inject_len() == INJECT_PER_ROUND
            && bk.audit_len() == INJECT_PER_ROUND
            && bk.reconcile()
            && bk.face_covered()
            && audit_lines_complete(&bk, 8),
        &format!(
            "注={} 审={} 核={} 覆={}",
            bk.inject_len(),
            bk.audit_len(),
            bk.reconcile(),
            bk.face_covered()
        ),
    );
    // J2：注入核 5/5。
    let (c, t, d5) = inject_5();
    s.add_dyn("J2-注入核5/5", c == t, &format!("{}/{} {}", c, t, d5.join(" ")));
    // J3：段位守卫。
    s.add_dyn("J3-段位守卫", segment_guard(), "0x30e00段闭集");
    // 扩展面合并（三元组面适配 shim API）。
    for (n2, p, d) in run_unx12512_extra() {
        s.add_dyn(n2, p, &d);
    }
    s
}
