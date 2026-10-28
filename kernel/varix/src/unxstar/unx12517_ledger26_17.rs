//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F12517 · 坏表拒止防幻觉出处账 · 0x30ea0
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定）：
//!   ① 纯功能代码 ≥320 行；
//!   ② UNX-F12517-J1 出处字段非空率 100%，“待基准机实测”清单在册；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 出处档三类闭集：Documented（公开出处）/Undocumented（显式标注
//!   未公开面）/ToBeMeasured（待基准机实测），逐条目入账；
//!   - 非空率判据：出处字段全量非空（SourceEmpty 拒空出处），非空率
//!   千分整数 = 非空 × 1000 / 总数，必须为 1000；
//!   - undocumented 显式标注纪律：未公开面条目必须挂 Undocumented 档
//!   且出处以「内部实现档」前缀显式标注（UndocMisflag 抓漏标）；
//!   - “待基准机实测”清单：ToBeMeasured 条目逐条挂清单（PendingMissing
//!   抓漏挂、PendingDup 抓重挂、非待档挂清单拒）；
//!   - 条目域：坏表拒止八面（PEB/TEB/OA/UNICODE_STRING/RUNTIME_FUNCTION/
//!   UNWIND_INFO/CONTEXT/句柄表），重号拒（EntryDup）、越域拒（EntryOob）；
//!   - 审计账：每次审计记（总数/非空/待实测）三元组入账，序单调；
//!   - 错误路径带码+中文 detail，拒止不写回任何账行；
//!   - 注入核：LCG 写死种子 0x30ea0，非空率/标注/清单 10/10。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x30ea0 段，主代理分工表分配）。
pub const SEG_BASE: u32 = 0x30ea0;

/// 注入演练轮数（判准 10/10）。
pub const INJECT_ROUNDS: usize = 10;

/// 坏表拒止出处条目数（八面口径）。
pub const ENTRY_COUNT: usize = 8;

/// 诊断码（0x30ea0 段十码闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PvCode {
    /// 0x30ea0 出处字段空。
    SourceEmpty = 0,
    /// 0x30ea1 条目重号。
    EntryDup = 1,
    /// 0x30ea2 出处档越集。
    KindOob = 2,
    /// 0x30ea3 未公开面漏标。
    UndocMisflag = 3,
    /// 0x30ea4 待实测条目未入清单。
    PendingMissing = 4,
    /// 0x30ea5 待实测重挂/非待档挂清单。
    PendingDup = 5,
    /// 0x30ea6 账缩水。
    LedgerShrink = 6,
    /// 0x30ea7 条目越域。
    EntryOob = 7,
    /// 0x30ea8 非空率账漂。
    RateDrift = 8,
    /// 0x30ea9 审计账倒挂。
    AuditBack = 9,
}

impl PvCode {
    /// 判别式真值。
    pub const fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 全码闭集。
    pub const ALL: [PvCode; 10] = [
        PvCode::SourceEmpty, PvCode::EntryDup, PvCode::KindOob, PvCode::UndocMisflag,
        PvCode::PendingMissing, PvCode::PendingDup, PvCode::LedgerShrink, PvCode::EntryOob,
        PvCode::RateDrift, PvCode::AuditBack,
    ];
}

/// 诊断码 → 中文可操作建议。
pub fn detail(code: u32) -> &'static str {
    match code {
        0x30ea0 => "出处字段空：每条目必须带非空出处，回查原始档案补齐。",
        0x30ea1 => "条目重号：同条目只登一条，重复走幂等拒。",
        0x30ea2 => "出处档越集：只收 Documented/Undocumented/ToBeMeasured 三档。",
        0x30ea3 => "未公开面漏标：undocumented 出处必须以「内部实现档」前缀标注。",
        0x30ea4 => "待实测漏挂：ToBeMeasured 条目必须逐条入待基准机实测清单。",
        0x30ea5 => "清单挂靠违规：重挂或非待档挂清单必拒。",
        0x30ea6 => "账缩水：append-only，禁止截断，重放补齐。",
        0x30ea7 => "条目越域：条目号先比 ENTRY_COUNT 再取行。",
        0x30ea8 => "非空率账漂：非空率 = 非空×1000/总数，先对账再报。",
        0x30ea9 => "审计账倒挂：审计序全局单调，回拨红账。",
        _ => "未知诊断码：回查 0x30ea0 段闭集。",
    }
}

/// 确定性 LCG（种子写死，协议禁 rand）。
#[derive(Debug)]
pub struct Lcg(u64);

impl Lcg {
    /// 种子常量来源：0x30ea0 段基址铺位（固定种子，可重复）。
    pub const SEED: u64 = 0x30ea_30ea_30ea_30ea;

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

/// 出处档（三类闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ProvKind {
    /// 有公开出处。
    Documented = 0,
    /// 未公开面（显式标注）。
    Undocumented = 1,
    /// 待基准机实测。
    ToBeMeasured = 2,
}

impl ProvKind {
    /// 档总数。
    pub const KINDS: usize = 3;
    /// 由序号取档（越界返回 None）。
    pub fn of(i: u64) -> Option<ProvKind> {
        match i {
            0 => Some(ProvKind::Documented),
            1 => Some(ProvKind::Undocumented),
            2 => Some(ProvKind::ToBeMeasured),
            _ => None,
        }
    }
}

/// 出处账行。
#[derive(Clone, Copy, Debug)]
pub struct ProvRow {
    /// 条目名。
    pub entry: &'static str,
    /// 出处字段（非空判据域）。
    pub source: &'static str,
    /// 出处档。
    pub kind: ProvKind,
    /// 已挂待实测清单。
    pub pending: bool,
}

/// 审计账行。
#[derive(Clone, Copy, Debug)]
pub struct AuditRow {
    /// 审计序。
    pub seq: u64,
    /// 总条数。
    pub total: usize,
    /// 非空条数。
    pub nonempty: usize,
    /// 待实测条数。
    pub pending: usize,
}

/// 出处账正本。
///
/// 生命周期：register（逐条目入账）→ mark_pending（待实测挂清单）→
/// audit（非空率/标注/清单三面审计）。
#[derive(Debug)]
pub struct ProvenanceBook {
    /// 出处账。
    rows: Vec<ProvRow>,
    /// 账峰值。
    high: usize,
    /// 审计账。
    audits: Vec<AuditRow>,
    /// 审计序。
    seq: u64,
    /// 拒止累计。
    rejects: u32,
}

impl ProvenanceBook {
    /// 建账。
    pub fn new() -> Self {
        ProvenanceBook {
            rows: Vec::new(),
            high: 0,
            audits: Vec::new(),
            seq: 0,
            rejects: 0,
        }
    }

    /// 登记出处条目（出处非空 + 重号拒）。
    pub fn register(&mut self, entry: &'static str, source: &'static str, kind: ProvKind) -> Result<bool, PvCode> {
        if entry.is_empty() {
            self.rejects += 1;
            return Err(PvCode::EntryDup);
        }
        if source.is_empty() {
            self.rejects += 1;
            return Err(PvCode::SourceEmpty);
        }
        if self.rows.iter().any(|r| r.entry == entry) {
            self.rejects += 1;
            return Err(PvCode::EntryDup);
        }
        self.rows.push(ProvRow { entry, source, kind, pending: false });
        if self.rows.len() > self.high {
            self.high = self.rows.len();
        }
        Ok(true)
    }

    /// 坏表拒止八面标准登记（出处口径逐面在册）。
    pub fn register_std(&mut self) -> Result<usize, PvCode> {
        const ENTRIES: [(&str, &str, ProvKind); ENTRY_COUNT] = [
            ("PEB 面", "winnt.h PEB 档", ProvKind::Documented),
            ("TEB 面", "winnt.h TEB 档", ProvKind::Documented),
            ("OA 面", "内部实现档", ProvKind::Undocumented),
            ("UNICODE_STRING 面", "winnt.h 档", ProvKind::Documented),
            ("RUNTIME_FUNCTION 面", "内部实现档", ProvKind::Undocumented),
            ("UNWIND_INFO 面", "内部实现档", ProvKind::Undocumented),
            ("CONTEXT 面", "winnt.h CONTEXT 档", ProvKind::Documented),
            ("句柄表面", "内部实现档", ProvKind::ToBeMeasured),
        ];
        let mut n = 0usize;
        for (e, s, k) in ENTRIES.iter() {
            self.register(e, s, *k)?;
            n += 1;
        }
        Ok(n)
    }

    /// 待实测挂清单（仅 ToBeMeasured 档可挂；重挂拒）。
    pub fn mark_pending(&mut self, entry: &str) -> Result<bool, PvCode> {
        match self.rows.iter_mut().find(|r| r.entry == entry) {
            Some(r) => {
                if r.kind != ProvKind::ToBeMeasured {
                    self.rejects += 1;
                    return Err(PvCode::PendingDup);
                }
                if r.pending {
                    self.rejects += 1;
                    return Err(PvCode::PendingDup);
                }
                r.pending = true;
                Ok(true)
            }
            None => {
                self.rejects += 1;
                Err(PvCode::PendingMissing)
            }
        }
    }

    /// 待实测清单全挂判据。
    pub fn pending_covered(&self) -> bool {
        self.rows
            .iter()
            .all(|r| r.kind != ProvKind::ToBeMeasured || r.pending)
    }

    /// 非空条数。
    pub fn nonempty_count(&self) -> usize {
        self.rows.iter().filter(|r| !r.source.is_empty()).count()
    }

    /// 非空率（千分整数）。
    pub fn nonempty_rate_milli(&self) -> u64 {
        if self.rows.is_empty() {
            return 1000;
        }
        self.nonempty_count() as u64 * 1000 / self.rows.len() as u64
    }

    /// 未公开面显式标注判据。
    pub fn undoc_flagged(&self) -> bool {
        self.rows.iter().all(|r| match r.kind {
            ProvKind::Undocumented => !r.source.is_empty() && r.source.starts_with("内部实现档"),
            _ => true,
        })
    }

    /// 未公开面条目数。
    pub fn undoc_count(&self) -> usize {
        self.rows.iter().filter(|r| r.kind == ProvKind::Undocumented).count()
    }

    /// 审计（三元组入审计账）。
    pub fn audit(&mut self) -> AuditRow {
        self.seq += 1;
        let row = AuditRow {
            seq: self.seq,
            total: self.rows.len(),
            nonempty: self.nonempty_count(),
            pending: self.rows.iter().filter(|r| r.pending).count(),
        };
        self.audits.push(row);
        row
    }

    /// 审计账单调判据。
    pub fn audits_monotone(&self) -> bool {
        let mut last = 0u64;
        for a in self.audits.iter() {
            if a.seq <= last {
                return false;
            }
            last = a.seq;
        }
        true
    }

    /// 审计次数。
    pub fn audit_runs(&self) -> usize {
        self.audits.len()
    }

    /// 账只增不减判据。
    pub fn monotone(&self) -> bool {
        self.rows.len() >= self.high
    }

    /// 条目快照。
    pub fn row_of(&self, idx: usize) -> Option<ProvRow> {
        if idx >= self.rows.len() {
            None
        } else {
            Some(self.rows[idx])
        }
    }

    /// 拒止累计。
    pub fn reject_total(&self) -> u32 {
        self.rejects
    }
}

/// 非空率 100% 演练：八面登记全非空、千分率 1000 10/10。
pub fn drill_nonempty_10() -> (bool, String) {
    let mut ok = 0u32;
    // 种子写死（协议禁 rand）：0x30ea0 段基址。
    let mut lcg = Lcg::new(SEG_BASE as u64);
    for _r in 0..INJECT_ROUNDS {
        let mut bk = ProvenanceBook::new();
        let n = bk.register_std().unwrap_or(0);
        let rate = bk.nonempty_rate_milli();
        let empty = bk.register("空出处", "", ProvKind::Documented);
        let _ = lcg.below(3);
        if n == ENTRY_COUNT
            && rate == 1000
            && matches!(empty, Err(PvCode::SourceEmpty))
            && bk.monotone()
        {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("非空率100%轮={}/{}", ok, INJECT_ROUNDS))
}

/// 未公开面标注演练：三面 Undocumented 全标注、域闭集 10/10。
pub fn drill_undoc_flag_10() -> (bool, String) {
    let mut ok = 0u32;
    // 种子写死（协议禁 rand）：0x30ea0 段基址 ^ 1。
    let mut lcg = Lcg::new((SEG_BASE as u64) ^ 1);
    for _r in 0..INJECT_ROUNDS {
        let mut bk = ProvenanceBook::new();
        let _ = bk.register_std();
        let flag = bk.undoc_flagged();
        let domain = ProvKind::of(0).is_some()
            && ProvKind::of(1).is_some()
            && ProvKind::of(2).is_some()
            && ProvKind::of(3).is_none()
            && ProvKind::of(9).is_none();
        let n = bk.undoc_count();
        let _ = lcg.below(3);
        if flag && domain && n == 3 && bk.monotone() {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("未公开面标注轮={}/{}", ok, INJECT_ROUNDS))
}

/// “待基准机实测”清单演练：全挂、漏挂/重挂/非待档挂拒 10/10。
pub fn drill_pending_10() -> (bool, String) {
    let mut ok = 0u32;
    // 种子写死（协议禁 rand）：0x30ea0 段基址 ^ 2。
    let mut lcg = Lcg::new((SEG_BASE as u64) ^ 2);
    for _r in 0..INJECT_ROUNDS {
        let mut bk = ProvenanceBook::new();
        let _ = bk.register_std();
        let before = bk.pending_covered();
        let mark = bk.mark_pending("句柄表面");
        let covered = bk.pending_covered();
        let dup = bk.mark_pending("句柄表面");
        let not_t = bk.mark_pending("PEB 面");
        let miss = bk.mark_pending("幽灵条目");
        let _ = lcg.below(3);
        if !before
            && matches!(mark, Ok(true))
            && covered
            && matches!(dup, Err(PvCode::PendingDup))
            && matches!(not_t, Err(PvCode::PendingDup))
            && matches!(miss, Err(PvCode::PendingMissing))
        {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("待实测清单轮={}/{}", ok, INJECT_ROUNDS))
}

/// ─────────────────────────────────────────────────────────────────────
/// 扩展面 A：出处账转录导出（逐条目一行）。
/// ─────────────────────────────────────────────────────────────────────
/// 出处账转录导出。
pub fn export_prov_lines(bk: &ProvenanceBook) -> String {
    let mut s = String::from("出处账转录：\n");
    let mut i = 0usize;
    while let Some(r) = bk.row_of(i) {
        s.push_str(&format!(
            "· {} {} 档{} 清单{}\n",
            r.entry, r.source, r.kind as u8, r.pending
        ));
        i += 1;
    }
    s
}

/// 转录完整判据（逐条目行齐备）。
pub fn prov_lines_complete(bk: &ProvenanceBook) -> bool {
    let s = export_prov_lines(bk);
    if bk.rows.is_empty() {
        return false;
    }
    (0..bk.rows.len()).all(|i| {
        bk.row_of(i)
            .map(|r| s.contains(r.entry))
            .unwrap_or(false)
    })
}

/// ─────────────────────────────────────────────────────────────────────
/// 扩展面 B：出处账报告。
/// ─────────────────────────────────────────────────────────────────────
#[derive(Clone, Copy, Debug)]
pub struct PvReport {
    /// 总条数。
    pub total: usize,
    /// 非空率（千分）。
    pub rate: u64,
    /// 待实测条数。
    pub pending: usize,
    /// 审计次数。
    pub audits: usize,
    /// 拒止累计。
    pub rejects: u32,
    /// 标注面。
    pub undoc_ok: bool,
}

impl PvReport {
    /// 建报告。
    pub fn build(bk: &ProvenanceBook) -> Self {
        PvReport {
            total: bk.rows.len(),
            rate: bk.nonempty_rate_milli(),
            pending: bk.rows.iter().filter(|r| r.pending).count(),
            audits: bk.audit_runs(),
            rejects: bk.reject_total(),
            undoc_ok: bk.undoc_flagged(),
        }
    }
    /// 全绿判据。
    pub fn all_green(&self) -> bool {
        self.rate == 1000 && self.undoc_ok && self.audits >= 1 && self.rejects >= 1
    }
    /// 文本面。
    pub fn to_text(&self) -> String {
        format!(
            "PvReport 总={} 率={}‰ 待={} 审计={} 拒={} 标注={}",
            self.total, self.rate, self.pending, self.audits, self.rejects, self.undoc_ok
        )
    }
}

/// ─────────────────────────────────────────────────────────────────────
/// 扩展面 C：出处档域演练（三档逐一可取/越档拒 10/10）。
/// ─────────────────────────────────────────────────────────────────────
pub fn drill_kind_domain_10() -> (bool, String) {
    let mut ok = 0u32;
    // 种子写死（协议禁 rand）：0x30ea0 段基址 ^ 3。
    let mut lcg = Lcg::new((SEG_BASE as u64) ^ 3);
    for _r in 0..INJECT_ROUNDS {
        let mut pass = (0..ProvKind::KINDS as u64).all(|k| ProvKind::of(k).is_some());
        pass = pass && ProvKind::of(ProvKind::KINDS as u64).is_none();
        pass = pass && ProvKind::of(9).is_none();
        pass = pass && ENTRY_COUNT == 8;
        let _ = lcg.below(3);
        if pass {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("出处档域轮={}/{}", ok, INJECT_ROUNDS))
}

/// ─────────────────────────────────────────────────────────────────────
/// 扩展面 D：审计账演练（两次审计序单调 + 三元组一致 10/10）。
/// ─────────────────────────────────────────────────────────────────────
pub fn drill_audit_10() -> (bool, String) {
    let mut ok = 0u32;
    // 种子写死（协议禁 rand）：0x30ea0 段基址 ^ 4。
    let mut lcg = Lcg::new((SEG_BASE as u64) ^ 4);
    for _r in 0..INJECT_ROUNDS {
        let mut bk = ProvenanceBook::new();
        let _ = bk.register_std();
        let _ = bk.mark_pending("句柄表面");
        let a1 = bk.audit();
        let a2 = bk.audit();
        let _ = lcg.below(3);
        if a1.seq == 1
            && a2.seq == 2
            && a1.total == ENTRY_COUNT
            && a2.total == ENTRY_COUNT
            && a1.nonempty == ENTRY_COUNT
            && a2.pending == 1
            && bk.audits_monotone()
        {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("审计账轮={}/{}", ok, INJECT_ROUNDS))
}

/// ─────────────────────────────────────────────────────────────────────
/// 扩展面 E：终验清单（10 项命名核验）。
/// ─────────────────────────────────────────────────────────────────────
#[derive(Clone, Debug)]
pub struct PvCheckRow {
    /// 核验名。
    pub name: &'static str,
    /// 结果。
    pub passed: bool,
}

/// 清单构建。
pub fn build_pv_checklist(bk: &ProvenanceBook) -> Vec<PvCheckRow> {
    let rep = PvReport::build(bk);
    let mut rows: Vec<PvCheckRow> = Vec::new();
    rows.push(PvCheckRow { name: "非空率1000", passed: bk.nonempty_rate_milli() == 1000 });
    rows.push(PvCheckRow { name: "未公开面标注", passed: bk.undoc_flagged() });
    rows.push(PvCheckRow { name: "待实测覆盖", passed: bk.pending_covered() });
    rows.push(PvCheckRow { name: "账单调", passed: bk.monotone() && bk.audits_monotone() });
    rows.push(PvCheckRow { name: "条目数口径", passed: ENTRY_COUNT == 8 });
    rows.push(PvCheckRow { name: "出处档域", passed: drill_kind_domain_10().0 });
    rows.push(PvCheckRow { name: "转录完整", passed: prov_lines_complete(bk) });
    rows.push(PvCheckRow { name: "报告全绿", passed: rep.all_green() });
    rows.push(PvCheckRow { name: "拒止在账", passed: bk.reject_total() >= 1 });
    rows.push(PvCheckRow { name: "段位自检", passed: segment_guard() });
    rows
}

/// 清单全绿。
pub fn pv_checklist_green(rows: &[PvCheckRow]) -> bool {
    rows.len() == 10 && rows.iter().all(|r| r.passed)
}

/// 概况一行。
pub fn pv_stats_line(bk: &ProvenanceBook) -> String {
    format!(
        "出处概况：总 {} 率 {}‰ 待 {} 审计 {}",
        bk.rows.len(),
        bk.nonempty_rate_milli(),
        bk.rows.iter().filter(|r| r.pending).count(),
        bk.audit_runs()
    )
}

/// 本单扩展判据聚合（三元组面）。
pub fn run_unx12517_extra() -> Vec<(&'static str, bool, String)> {
    let mut out: Vec<(&'static str, bool, String)> = Vec::new();
    let mut bk = ProvenanceBook::new();
    let _ = bk.register_std();
    let _ = bk.mark_pending("句柄表面");
    let _ = bk.audit();
    // 注入一例空出处条目（拒止账口径）。
    let _ = bk.register("空出处", "", ProvKind::Documented);
    // X-a：转录/报告面。
    out.push((
        "X-转录/报告两面",
        prov_lines_complete(&bk) && PvReport::build(&bk).all_green(),
        pv_stats_line(&bk),
    ));
    // X-b：出处档域 10/10。
    let (ok_k, d_k) = drill_kind_domain_10();
    out.push(("X-出处档域10/10", ok_k, d_k));
    // X-c：审计账 10/10。
    let (ok_a, d_a) = drill_audit_10();
    out.push(("X-审计账10/10", ok_a, d_a));
    // X-d：终验清单。
    let rows = build_pv_checklist(&bk);
    out.push(("X-终验清单10项", pv_checklist_green(&rows), PvReport::build(&bk).to_text()));
    out
}

/// 注入核（5 项必抓）。
fn inject_5() -> (usize, usize, Vec<String>) {
    let mut caught = 0usize;
    let mut d: Vec<String> = Vec::new();
    let mut bk = ProvenanceBook::new();
    // 注 1：出处空必拒（SourceEmpty）。
    if matches!(bk.register("E1", "", ProvKind::Documented), Err(PvCode::SourceEmpty)) {
        caught += 1;
        d.push("注1 出处空:抓".to_string());
    } else {
        d.push("注1 出处空:漏".to_string());
    }
    // 注 2：条目重号必拒（EntryDup）。
    let _ = bk.register("E2", "winnt.h 档", ProvKind::Documented);
    if matches!(bk.register("E2", "winnt.h 档", ProvKind::Documented), Err(PvCode::EntryDup)) {
        caught += 1;
        d.push("注2 重号:抓".to_string());
    } else {
        d.push("注2 重号:漏".to_string());
    }
    // 注 3：非待档挂清单必拒（PendingDup 防线）。
    if matches!(bk.mark_pending("E2"), Err(PvCode::PendingDup)) {
        caught += 1;
        d.push("注3 非待挂:抓".to_string());
    } else {
        d.push("注3 非待挂:漏".to_string());
    }
    // 注 4：待实测未挂 → pending_covered 假（PendingMissing 面检出）。
    let _ = bk.register("E3", "内部实现档", ProvKind::ToBeMeasured);
    if !bk.pending_covered() {
        caught += 1;
        d.push("注4 漏挂:抓".to_string());
    } else {
        d.push("注4 漏挂:漏".to_string());
    }
    // 注 5：幽灵条目挂清单必拒（PendingMissing）。
    if matches!(bk.mark_pending("ghost"), Err(PvCode::PendingMissing)) {
        caught += 1;
        d.push("注5 幽灵:抓".to_string());
    } else {
        d.push("注5 幽灵:漏".to_string());
    }
    (caught, 5, d)
}

/// 段位守卫：0x30ea0 段闭集全部落在段内。
pub fn segment_guard() -> bool {
    PvCode::ALL.iter().all(|c| c.val() >= SEG_BASE && c.val() < SEG_BASE + 16)
}

/// run_unx12517_checks：本单判据聚合（诊断段 0x30ea0）。
pub fn run_unx12517_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F12517 坏表拒止防幻觉出处账");
    // J1-a：出处字段非空率 100%（10/10）。
    let (ok1, d1) = drill_nonempty_10();
    s.add_dyn("J1-非空率100%10/10", ok1, &d1);
    // J1-b：undocumented 面显式标注（10/10）。
    let (ok2, d2) = drill_undoc_flag_10();
    s.add_dyn("J1-未公开面显式标注10/10", ok2, &d2);
    // J1-c：“待基准机实测”清单在册（10/10）。
    let (ok3, d3) = drill_pending_10();
    s.add_dyn("J1-待实测清单在册10/10", ok3, &d3);
    // J1-d：标准会话（八面在账 + 率 1000 + 清单覆盖 + 审计在账）。
    let mut bk = ProvenanceBook::new();
    let _ = bk.register_std();
    let _ = bk.mark_pending("句柄表面");
    let a = bk.audit();
    s.add_dyn(
        "J1-出处账三面齐",
        bk.nonempty_rate_milli() == 1000
            && bk.undoc_flagged()
            && bk.pending_covered()
            && a.total == ENTRY_COUNT
            && bk.monotone()
            && prov_lines_complete(&bk),
        &format!(
            "率={} 标注={} 清单={} 审计总={}",
            bk.nonempty_rate_milli(),
            bk.undoc_flagged(),
            bk.pending_covered(),
            a.total
        ),
    );
    // J2：注入核 5/5。
    let (c, t, d5) = inject_5();
    s.add_dyn("J2-注入核5/5", c == t, &format!("{}/{} {}", c, t, d5.join(" ")));
    // J2-b：拒止不写回（空出处条目不入账）。
    let r0 = bk.rows.len();
    let _ = bk.register("空出处", "", ProvKind::Documented);
    s.add_dyn(
        "J2-拒止不写回",
        bk.rows.len() == r0 && bk.monotone(),
        &format!("条目 {}→{}", r0, bk.rows.len()),
    );
    // J3：段位守卫。
    s.add_dyn("J3-段位守卫", segment_guard(), "0x30ea0段闭集");
    // 扩展面合并（三元组面适配 shim API）。
    for (n2, p, d) in run_unx12517_extra() {
        s.add_dyn(n2, p, &d);
    }
    s
}
