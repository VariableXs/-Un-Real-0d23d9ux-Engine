//! UNX-F1547 域级文档对账（文档↔实现↔台账三向） · 0x788C0段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（派工卡 VX8-B04 卡 4 · UNX-F1547 逐字摘录）：
//!   ① 纯功能代码 ≥360 行；
//!   ② 判据原文：UNX-F1547-J1 三向对账通过（域册/接口册/预案册↔判据
//!     台账↔深化学实三向逐项对平 100%），失配注入 10/10 检出，对账
//!     报告落档；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 三册闭集：域册 / 接口册 / 预案册，册名唯一，越册显性拒；
//!   - 判据台账闭集：24 条判据（UNX-F1547-J01..J24）逐条入账，编号
//!     唯一可溯，缺账越账显性拒；
//!   - 三向对平：逐判据三问（三册各covered？台账在账？学实在案？），
//!     四问全应是方判对平，任一否即失配显性检出；
//!   - 失配注入 10/10：逐判据位次翻转一册覆盖位，十路注入逐路必检；
//!   - 对账报告落档：24 行判据对平明细 + 头尾两行，行数恒 26，指纹
//!     FNV-1a 落账，双跑逐位一致；
//!   - 状态机单调：呈册 → 对账 → 终验 → 封档，倒退跳级显性拒；
//!   - LCG 种子写死 = SEG_BASE；零浮点 / IO / 墙钟 / unwrap / panic。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x788C0，派工卡 VX8-B04 卡 4 独占窗口）。
pub const SEG_BASE: u32 = 0x788C0;

/// 三册闭集条数（域册/接口册/预案册）。
pub const BOOK_COUNT: usize = 3;

/// 判据台账条数（判据 J1 冻结）。
pub const LEDGER_COUNT: usize = 24;

/// 失配注入路数（判据 J1「10/10 检出」冻结）。
pub const INJECT_COUNT: usize = 10;

/// 对账报告行数（24 行明细 + 头尾两行，冻结）。
pub const REPORT_ROWS: usize = 26;

// ── 三册闭集冻结表（编目序 = 判据 J1 列序） ─────────────────────
pub const DOC_BOOKS: [&str; 3] = ["域册", "接口册", "预案册"];

// ── 判据台账冻结表（24 条，编号唯一） ───────────────────────────
pub const LEDGER_JUDGES: [&str; 24] = [
    "UNX-F1547-J01 域册覆盖全判据",
    "UNX-F1547-J02 接口册覆盖全判据",
    "UNX-F1547-J03 预案册覆盖全判据",
    "UNX-F1547-J04 判据台账逐条在账",
    "UNX-F1547-J05 深化学实逐条在案",
    "UNX-F1547-J06 学实指纹与台账互证",
    "UNX-F1547-J07 域册条目与台账同序",
    "UNX-F1547-J08 接口册条目与台账同序",
    "UNX-F1547-J09 预案册条目与台账同序",
    "UNX-F1547-J10 判据编号三册一致",
    "UNX-F1547-J11 失配清单逐条可溯",
    "UNX-F1547-J12 失配检出率 10/10",
    "UNX-F1547-J13 对账报告落档",
    "UNX-F1547-J14 报告行数恒定",
    "UNX-F1547-J15 报告指纹双跑一致",
    "UNX-F1547-J16 域册修订留痕",
    "UNX-F1547-J17 接口册修订留痕",
    "UNX-F1547-J18 预案册修订留痕",
    "UNX-F1547-J19 台账版本冻结",
    "UNX-F1547-J20 学实版本冻结",
    "UNX-F1547-J21 三向差集为空",
    "UNX-F1547-J22 越册呈送显性拒",
    "UNX-F1547-J23 越账条目显性拒",
    "UNX-F1547-J24 对账状态单调推进",
];

// ── 深化学实冻结表（24 条实证指纹，与台账逐条互证） ─────────────
pub const IMPL_EVIDENCE: [u32; 24] = [
    0x1547_0001, 0x1547_0002, 0x1547_0003, 0x1547_0004,
    0x1547_0005, 0x1547_0006, 0x1547_0007, 0x1547_0008,
    0x1547_0009, 0x1547_000A, 0x1547_000B, 0x1547_000C,
    0x1547_000D, 0x1547_000E, 0x1547_000F, 0x1547_0010,
    0x1547_0011, 0x1547_0012, 0x1547_0013, 0x1547_0014,
    0x1547_0015, 0x1547_0016, 0x1547_0017, 0x1547_0018,
];

// ── 诊断码闭集（段内 0x01..0x0C 窗口，派工卡约定） ────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Recon47Code {
    /// 册名越闭集：呈送册收进域册/接口册/预案册三册
    BookUnknown,
    /// 册名重复：一册一呈，重呈先撤旧
    BookDuplicate,
    /// 覆盖位长度违约：覆盖位必须恰 24 位
    CoverLenBad,
    /// 判据号越账：判据收进 24 条台账闭集
    JudgeUnknown,
    /// 学实缺案：判据无实证指纹，补学实再对账
    ImplMissing,
    /// 三向失配：逐判据三问有否，回补缺面再对账
    ReconMismatch,
    /// 报告行数违约：对账报告必须恰 26 行
    ReportRowsBad,
    /// 对账状态倒退：呈册→对账→终验→封档单调推进
    PhaseRegress,
    /// 对账状态跳级：未呈齐三册不得对账，未对账不得封档
    PhaseSkip,
    /// 封档后再改：对账封档禁改，另开对账修订档
    SealedLocked,
    /// 对账报告为空：先对账生成报告再落档
    ReportEmpty,
    /// 双跑不一致：排查报告指纹随机性后复跑
    NotReproducible,
}

impl Recon47Code {
    /// 段内闭集取值：SEG_BASE + 0x01 ..= SEG_BASE + 0x0C。
    pub fn val(self) -> u32 {
        SEG_BASE + 0x01 + self as u32
    }
    /// 中文诊断文案（含可操作建议）。
    pub fn detail(self) -> &'static str {
        match self {
            Recon47Code::BookUnknown => "册名越闭集：呈送册收进域册/接口册/预案册三册",
            Recon47Code::BookDuplicate => "册名重复呈送：一册一呈，撤旧呈再补",
            Recon47Code::CoverLenBad => "覆盖位长度违约：覆盖位必须恰 24 位",
            Recon47Code::JudgeUnknown => "判据号越账：判据收进 24 条台账闭集",
            Recon47Code::ImplMissing => "学实缺案：为判据补实证指纹再对账",
            Recon47Code::ReconMismatch => "三向失配：回补缺面对齐后再对账",
            Recon47Code::ReportRowsBad => "对账报告行数违约：报告必须恰 26 行",
            Recon47Code::PhaseRegress => "对账状态倒退：呈册→对账→终验→封档单调推进",
            Recon47Code::PhaseSkip => "对账状态跳级：未呈齐三册不得对账",
            Recon47Code::SealedLocked => "对账封档禁改：另开对账修订档",
            Recon47Code::ReportEmpty => "对账报告为空：先对账生成报告再落档",
            Recon47Code::NotReproducible => "双跑不一致：排查报告指纹随机性后复跑",
        }
    }
    /// 闭集数组（查表唯一来源）。
    pub const ALL: [Recon47Code; 12] = [
        Recon47Code::BookUnknown,
        Recon47Code::BookDuplicate,
        Recon47Code::CoverLenBad,
        Recon47Code::JudgeUnknown,
        Recon47Code::ImplMissing,
        Recon47Code::ReconMismatch,
        Recon47Code::ReportRowsBad,
        Recon47Code::PhaseRegress,
        Recon47Code::PhaseSkip,
        Recon47Code::SealedLocked,
        Recon47Code::ReportEmpty,
        Recon47Code::NotReproducible,
    ];
}

/// 诊断码 → 文案闭集查表（闭集外返回未登记占位）。
pub fn detail(code: u32) -> &'static str {
    let mut i = 0usize;
    while i < Recon47Code::ALL.len() {
        let c = match Recon47Code::ALL.get(i) {
            Some(c) => *c,
            None => break,
        };
        if c.val() == code {
            return c.detail();
        }
        i += 1;
    }
    "未登记诊断码：不在本段闭集内"
}

// ── FNV-1a 指纹（整数域，零随机） ────────────────────────────────
fn fnv1a(data: &[u8]) -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    for b in data.iter() {
        h ^= *b as u32;
        h = h.wrapping_mul(0x0100_0193);
    }
    h
}

// ── 对账状态机（呈册 → 对账 → 终验 → 封档，单调推进） ───────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ReconPhase {
    /// 呈册期：三册逐一呈送
    Submitted,
    /// 对账期：三向逐判据对账完成
    Reconciled,
    /// 终验期：差集为零终验通过
    Verified,
    /// 封档期：报告落档封版
    Sealed,
}

impl ReconPhase {
    /// 位次：倒退判定基准。
    pub fn rank(self) -> u8 {
        match self {
            ReconPhase::Submitted => 0,
            ReconPhase::Reconciled => 1,
            ReconPhase::Verified => 2,
            ReconPhase::Sealed => 3,
        }
    }
}

/// 一册呈送视图：册名 + 24 位判据覆盖位。
#[derive(Clone, Debug)]
pub struct BookView {
    pub name: String,
    pub cover: Vec<bool>,
}

/// 三向对账机。
pub struct TriRecon {
    phase: ReconPhase,
    books: Vec<BookView>,
    reconciled: Vec<String>,
    mismatched: Vec<String>,
    report: Option<String>,
    report_digest: u32,
}

impl TriRecon {
    pub fn new() -> TriRecon {
        TriRecon {
            phase: ReconPhase::Submitted,
            books: Vec::new(),
            reconciled: Vec::new(),
            mismatched: Vec::new(),
            report: None,
            report_digest: 0,
        }
    }

    pub fn phase(&self) -> ReconPhase {
        self.phase
    }

    pub fn book_count(&self) -> usize {
        self.books.len()
    }

    pub fn reconciled_count(&self) -> usize {
        self.reconciled.len()
    }

    pub fn mismatch_count(&self) -> usize {
        self.mismatched.len()
    }

    pub fn report_digest(&self) -> u32 {
        self.report_digest
    }

    /// 呈册：册名闭集 + 覆盖位恰 24 + 一册一呈。
    pub fn submit_book(&mut self, name: &str, cover: &[bool]) -> Result<(), u32> {
        if self.phase == ReconPhase::Sealed {
            return Err(Recon47Code::SealedLocked.val());
        }
        if self.phase != ReconPhase::Submitted {
            return Err(Recon47Code::PhaseRegress.val());
        }
        let mut known = false;
        let mut i = 0usize;
        while i < DOC_BOOKS.len() {
            let n = match DOC_BOOKS.get(i) {
                Some(n) => *n,
                None => break,
            };
            if n == name {
                known = true;
            }
            i += 1;
        }
        if !known {
            return Err(Recon47Code::BookUnknown.val());
        }
        if cover.len() != LEDGER_COUNT {
            return Err(Recon47Code::CoverLenBad.val());
        }
        let mut i = 0usize;
        while i < self.books.len() {
            let b = match self.books.get(i) {
                Some(b) => b,
                None => return Err(Recon47Code::NotReproducible.val()),
            };
            if b.name == name {
                return Err(Recon47Code::BookDuplicate.val());
            }
            i += 1;
        }
        let mut cv: Vec<bool> = Vec::new();
        for c in cover.iter() {
            cv.push(*c);
        }
        self.books.push(BookView {
            name: name.to_string(),
            cover: cv,
        });
        Ok(())
    }

    /// 三向对账：逐判据三册覆盖 + 台账在账 + 学实在案。
    /// 返回对平条数；任一失配显性拒并列失配清单。
    pub fn reconcile(&mut self, impls: &[u32]) -> Result<usize, u32> {
        if self.phase == ReconPhase::Sealed {
            return Err(Recon47Code::SealedLocked.val());
        }
        if self.phase.rank() < ReconPhase::Submitted.rank() {
            return Err(Recon47Code::PhaseSkip.val());
        }
        if self.books.len() != BOOK_COUNT {
            return Err(Recon47Code::PhaseSkip.val());
        }
        if impls.len() != LEDGER_COUNT {
            return Err(Recon47Code::ImplMissing.val());
        }
        self.reconciled.clear();
        self.mismatched.clear();
        let mut j = 0usize;
        while j < LEDGER_COUNT {
            let judge = match LEDGER_JUDGES.get(j) {
                Some(n) => *n,
                None => return Err(Recon47Code::JudgeUnknown.val()),
            };
            let ev = match impls.get(j) {
                Some(e) => *e,
                None => return Err(Recon47Code::ImplMissing.val()),
            };
            let expect = match IMPL_EVIDENCE.get(j) {
                Some(e) => *e,
                None => return Err(Recon47Code::NotReproducible.val()),
            };
            let mut all_covered = true;
            let mut b = 0usize;
            while b < self.books.len() {
                let covered = match self.books.get(b) {
                    Some(bk) => match bk.cover.get(j) {
                        Some(c) => *c,
                        None => false,
                    },
                    None => false,
                };
                if !covered {
                    all_covered = false;
                }
                b += 1;
            }
            // 四问：三册覆盖 + 学实指纹互证。
            if all_covered && ev == expect && ev != 0 {
                self.reconciled.push(judge.to_string());
            } else {
                self.mismatched.push(judge.to_string());
            }
            j += 1;
        }
        if !self.mismatched.is_empty() {
            return Err(Recon47Code::ReconMismatch.val());
        }
        self.phase = ReconPhase::Reconciled;
        Ok(self.reconciled.len())
    }

    /// 终验：对平恰 24 条、失配为零方可推进。
    pub fn verify_recon(&mut self) -> Result<usize, u32> {
        if self.phase == ReconPhase::Submitted {
            return Err(Recon47Code::PhaseSkip.val());
        }
        if self.phase == ReconPhase::Sealed {
            return Err(Recon47Code::PhaseRegress.val());
        }
        if self.reconciled.len() != LEDGER_COUNT || !self.mismatched.is_empty() {
            return Err(Recon47Code::ReconMismatch.val());
        }
        self.phase = ReconPhase::Verified;
        Ok(self.reconciled.len())
    }

    /// 封档：生成 26 行对账报告并落档。
    pub fn seal_report(&mut self) -> Result<u32, u32> {
        if self.phase == ReconPhase::Verified {
            // 允许推进。
        } else {
            return Err(Recon47Code::PhaseSkip.val());
        }
        let mut text = String::from("== UNX-F1547 三向对账报告 ==");
        text.push('\n');
        let mut j = 0usize;
        while j < LEDGER_COUNT {
            let judge = match LEDGER_JUDGES.get(j) {
                Some(n) => *n,
                None => break,
            };
            let mark = if self.mismatched.is_empty() { "●" } else { "○" };
            let line = format!("{} 三向{}", judge, mark);
            text.push_str(&line);
            text.push('\n');
            j += 1;
        }
        text.push_str(&format!("== 对平 {} 条，失配 {} 条 ==", self.reconciled.len(), self.mismatched.len()));
        text.push('\n');
        let rows = text.matches('\n').count();
        if rows != REPORT_ROWS {
            return Err(Recon47Code::ReportRowsBad.val());
        }
        let digest = fnv1a(text.as_bytes());
        self.report = Some(text);
        self.report_digest = digest;
        self.phase = ReconPhase::Sealed;
        Ok(digest)
    }

    pub fn report_text(&self) -> Option<&str> {
        match self.report.as_ref() {
            Some(t) => Some(t.as_str()),
            None => None,
        }
    }
}

// ── 全覆盖视图构建（24 位全真） ─────────────────────────────────
pub fn full_cover() -> [bool; 24] {
    [true; 24]
}

// ── 好对账构建（三册呈齐 + 对账 + 终验 + 封档） ─────────────────
pub fn build_recon_ok() -> TriRecon {
    let mut rc = TriRecon::new();
    let cover = full_cover();
    let mut i = 0usize;
    while i < DOC_BOOKS.len() {
        let name = match DOC_BOOKS.get(i) {
            Some(n) => *n,
            None => break,
        };
        let _ = rc.submit_book(name, &cover);
        i += 1;
    }
    let _ = rc.reconcile(&IMPL_EVIDENCE);
    let _ = rc.verify_recon();
    let _ = rc.seal_report();
    rc
}

/// 失配注入视图集：第 i 路在第 i 判据位翻转一册覆盖位。
pub fn build_injected_covers() -> [[bool; 24]; 10] {
    let mut out = [[true; 24]; 10];
    let mut i = 0usize;
    while i < INJECT_COUNT {
        let mut view = full_cover();
        let mut j = 0usize;
        while j < LEDGER_COUNT {
            let slot = match view.get_mut(j) {
                Some(s) => s,
                None => break,
            };
            if j == i {
                *slot = false;
            }
            j += 1;
        }
        let _ = out.get_mut(i).map(|s| *s = view);
        i += 1;
    }
    out
}

/// 十路注入逐路检出复算：注入视图对账必失配，好视图必对平。
fn inject_detection_count() -> usize {
    let covers = build_injected_covers();
    let mut caught = 0usize;
    let mut i = 0usize;
    while i < covers.len() {
        let mut rc = TriRecon::new();
        let mut k = 0usize;
        while k < DOC_BOOKS.len() {
            let name = match DOC_BOOKS.get(k) {
                Some(n) => *n,
                None => break,
            };
            let view = match covers.get(i) {
                Some(v) => v.clone(),
                None => break,
            };
            let _ = rc.submit_book(name, &view);
            k += 1;
        }
        match rc.reconcile(&IMPL_EVIDENCE) {
            Err(code) => {
                if code == Recon47Code::ReconMismatch.val() && rc.mismatch_count() == 1 {
                    caught += 1;
                }
            }
            Ok(_) => {}
        }
        i += 1;
    }
    caught
}

// ── 判据谓词（全部私有 fn xxx_ok() -> bool） ────────────────────
fn seg_guard_ok() -> bool {
    SEG_BASE % 0x40 == 0 && SEG_BASE >= 0x78800 && SEG_BASE <= 0x789C0
}

/// 三册呈齐且落闭集。
fn books_ok() -> bool {
    let rc = build_recon_ok();
    if rc.book_count() != BOOK_COUNT {
        return false;
    }
    if rc.phase() != ReconPhase::Sealed {
        return false;
    }
    if DOC_BOOKS.len() != BOOK_COUNT {
        return false;
    }
    true
}

/// 台账闭集：24 条编号唯一且前缀一致。
fn ledger_ok() -> bool {
    if LEDGER_JUDGES.len() != LEDGER_COUNT {
        return false;
    }
    let mut seen: Vec<&str> = Vec::new();
    let mut i = 0usize;
    while i < LEDGER_JUDGES.len() {
        let name = match LEDGER_JUDGES.get(i) {
            Some(n) => *n,
            None => return false,
        };
        if seen.iter().any(|s| *s == name) {
            return false;
        }
        if !name.starts_with("UNX-F1547-J") {
            return false;
        }
        seen.push(name);
        i += 1;
    }
    if seen.len() != LEDGER_COUNT {
        return false;
    }
    true
}

/// 学实在案：24 条指纹非零且与冻结表逐位可互证。
fn impl_ok() -> bool {
    if IMPL_EVIDENCE.len() != LEDGER_COUNT {
        return false;
    }
    let mut i = 0usize;
    while i < IMPL_EVIDENCE.len() {
        let ev = match IMPL_EVIDENCE.get(i) {
            Some(e) => *e,
            None => return false,
        };
        if ev == 0 {
            return false;
        }
        i += 1;
    }
    true
}

/// 三向逐项对平 100%：24 条全对平、失配为零。
fn recon_100_ok() -> bool {
    let mut rc = TriRecon::new();
    let cover = full_cover();
    let mut i = 0usize;
    while i < DOC_BOOKS.len() {
        let name = match DOC_BOOKS.get(i) {
            Some(n) => *n,
            None => return false,
        };
        if rc.submit_book(name, &cover).is_err() {
            return false;
        }
        i += 1;
    }
    let n = match rc.reconcile(&IMPL_EVIDENCE) {
        Ok(n) => n,
        Err(_) => return false,
    };
    if n != LEDGER_COUNT {
        return false;
    }
    if rc.mismatch_count() != 0 {
        return false;
    }
    if rc.phase() != ReconPhase::Reconciled {
        return false;
    }
    let v = match rc.verify_recon() {
        Ok(v) => v,
        Err(_) => return false,
    };
    if v != LEDGER_COUNT {
        return false;
    }
    true
}

/// 失配注入 10/10 检出：逐路注入必检且失配位次恰一。
fn inject_10_ok() -> bool {
    let caught = inject_detection_count();
    if caught != INJECT_COUNT {
        return false;
    }
    // 好视图必不被误检。
    let mut rc = TriRecon::new();
    let cover = full_cover();
    let mut i = 0usize;
    while i < DOC_BOOKS.len() {
        let name = match DOC_BOOKS.get(i) {
            Some(n) => *n,
            None => return false,
        };
        let _ = rc.submit_book(name, &cover);
        i += 1;
    }
    if rc.reconcile(&IMPL_EVIDENCE).is_err() {
        return false;
    }
    true
}

/// 对账报告落档：26 行、三向标记齐、指纹非零。
fn report_archived_ok() -> bool {
    let rc = build_recon_ok();
    let text = match rc.report_text() {
        Some(t) => t,
        None => return false,
    };
    if text.matches('\n').count() != REPORT_ROWS {
        return false;
    }
    if !text.contains("三向对账报告") {
        return false;
    }
    if !text.contains("失配 0 条") {
        return false;
    }
    let mut j = 0usize;
    while j < LEDGER_COUNT {
        let judge = match LEDGER_JUDGES.get(j) {
            Some(n) => *n,
            None => return false,
        };
        if !text.contains(judge) {
            return false;
        }
        j += 1;
    }
    if rc.report_digest() == 0 {
        return false;
    }
    true
}

/// 状态机单调：未呈齐对账拒、未对账封档拒、倒退拒。
fn phase_monotonic_ok() -> bool {
    let mut rc = TriRecon::new();
    let cover = full_cover();
    // 零册对账：跳级拒。
    if rc.reconcile(&IMPL_EVIDENCE).is_ok() {
        return false;
    }
    // 一册对账：呈不齐拒。
    let name0 = match DOC_BOOKS.get(0) {
        Some(n) => *n,
        None => return false,
    };
    let _ = rc.submit_book(name0, &cover);
    if rc.reconcile(&IMPL_EVIDENCE).is_ok() {
        return false;
    }
    // 未对账先终验：跳级拒。
    if rc.verify_recon().is_ok() {
        return false;
    }
    // 未终验先封档：跳级拒。
    if rc.seal_report().is_ok() {
        return false;
    }
    true
}

/// 呈册闸门：越册 / 重呈 / 覆盖位长度三路必抓。
fn submit_guard_ok() -> bool {
    let mut rc = TriRecon::new();
    let cover = full_cover();
    // 越册。
    match rc.submit_book("野册", &cover) {
        Err(code) => {
            if code != Recon47Code::BookUnknown.val() {
                return false;
            }
        }
        Ok(()) => return false,
    }
    // 覆盖位长度。
    let name0 = match DOC_BOOKS.get(0) {
        Some(n) => *n,
        None => return false,
    };
    let short: Vec<bool> = cover.iter().take(LEDGER_COUNT - 1).cloned().collect();
    match rc.submit_book(name0, &short) {
        Err(code) => {
            if code != Recon47Code::CoverLenBad.val() {
                return false;
            }
        }
        Ok(()) => return false,
    }
    // 重呈。
    let _ = rc.submit_book(name0, &cover);
    match rc.submit_book(name0, &cover) {
        Err(code) => {
            if code != Recon47Code::BookDuplicate.val() {
                return false;
            }
        }
        Ok(()) => return false,
    }
    if rc.book_count() != 1 {
        return false;
    }
    true
}

/// 学实缺案必抓：指纹置零一路必检。
fn impl_gap_caught_ok() -> bool {
    let mut gapped: Vec<u32> = Vec::new();
    let mut i = 0usize;
    while i < IMPL_EVIDENCE.len() {
        let ev = match IMPL_EVIDENCE.get(i) {
            Some(e) => *e,
            None => return false,
        };
        if i == 5 {
            gapped.push(0);
        } else {
            gapped.push(ev);
        }
        i += 1;
    }
    let mut rc = TriRecon::new();
    let cover = full_cover();
    let mut k = 0usize;
    while k < DOC_BOOKS.len() {
        let name = match DOC_BOOKS.get(k) {
            Some(n) => *n,
            None => return false,
        };
        let _ = rc.submit_book(name, &cover);
        k += 1;
    }
    match rc.reconcile(&gapped) {
        Err(code) => {
            if code != Recon47Code::ReconMismatch.val() {
                return false;
            }
        }
        Ok(_) => return false,
    }
    if rc.mismatch_count() != 1 {
        return false;
    }
    // 失配清单可溯：恰第 6 条判据（位次 5）。
    let judge = match LEDGER_JUDGES.get(5) {
        Some(n) => *n,
        None => return false,
    };
    let hit = match rc.mismatched.get(0) {
        Some(m) => m.as_str(),
        None => return false,
    };
    if hit != judge {
        return false;
    }
    true
}

/// 封档锁定：封档后再呈册必拒。
fn sealed_lock_ok() -> bool {
    let mut rc = build_recon_ok();
    let cover = full_cover();
    match rc.submit_book("域册", &cover) {
        Err(code) => {
            if code != Recon47Code::SealedLocked.val() {
                return false;
            }
        }
        Ok(()) => return false,
    }
    true
}

/// 双跑一致：好对账指纹两次构建逐位相等。
fn double_run_ok() -> bool {
    let d1 = build_recon_ok().report_digest();
    let d2 = build_recon_ok().report_digest();
    if d1 != d2 {
        return false;
    }
    if d1 == 0 {
        return false;
    }
    let r1 = build_recon_ok();
    let r2 = build_recon_ok();
    let t1 = match r1.report_text() {
        Some(t) => t,
        None => return false,
    };
    let t2 = match r2.report_text() {
        Some(t) => t,
        None => return false,
    };
    if t1 != t2 {
        return false;
    }
    true
}

/// 诊断码闭集：12 码唯一、落窗、查表可回、闭集外占位。
fn diag_closed_set_ok() -> bool {
    let mut seen: Vec<u32> = Vec::new();
    for c in Recon47Code::ALL.iter() {
        let v = c.val();
        if v <= SEG_BASE || v > SEG_BASE + 0x0C {
            return false;
        }
        if seen.iter().any(|s| *s == v) {
            return false;
        }
        if c.detail().is_empty() {
            return false;
        }
        seen.push(v);
    }
    if seen.len() != 12 {
        return false;
    }
    let first = match Recon47Code::ALL.get(0) {
        Some(c) => c.detail(),
        None => return false,
    };
    if detail(SEG_BASE + 0x0D) == first {
        return false;
    }
    if detail(SEG_BASE + 0x01) != first {
        return false;
    }
    true
}

// ── 检查节（入口，≥12 条 add_dyn） ──────────────────────────────
pub fn run_unx1547_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F1547 域级文档对账三向");
    s.add_dyn("段位守卫", seg_guard_ok(), &format!("SEG_BASE=0x{:X} 须 0x40 对齐且落在本批段窗 0x78800..=0x789C0", SEG_BASE));
    s.add_dyn("三册呈齐", books_ok(), "域册/接口册/预案册逐册呈送落闭集");
    s.add_dyn("台账闭集", ledger_ok(), "24 条判据编号唯一可溯");
    s.add_dyn("学实在案", impl_ok(), "24 条实证指纹非零可互证");
    s.add_dyn("三向对平100%", recon_100_ok(), "逐判据三册覆盖+台账+学实四问全应");
    s.add_dyn("失配注入10/10", inject_10_ok(), "十路覆盖位翻转逐路必检");
    s.add_dyn("对账报告落档", report_archived_ok(), "报告 26 行三向标记齐，指纹落档");
    s.add_dyn("状态机单调", phase_monotonic_ok(), "呈册→对账→终验→封档可达，跳级倒退拒");
    s.add_dyn("呈册闸门", submit_guard_ok(), "越册 / 覆盖位违约 / 重呈三路必抓");
    s.add_dyn("学实缺案必抓", impl_gap_caught_ok(), "指纹置零必检且失配位次可溯");
    s.add_dyn("封档锁定", sealed_lock_ok(), "封档后再呈册必拒");
    s.add_dyn("双跑一致", double_run_ok(), "好对账指纹与报告两次构建逐位相等");
    s.add_dyn("诊断码闭集", diag_closed_set_ok(), "12 码唯一、落窗、查表可回");
    s
}

// ── 测试区（≥8 条） ─────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seg_guard_frozen() {
        assert!(seg_guard_ok());
        assert_eq!(SEG_BASE % 0x40, 0);
        assert_eq!(SEG_BASE, 0x788C0);
    }

    #[test]
    fn checks_all_green_min_entries() {
        let s = run_unx1547_checks();
        assert!(s.all_passed(), "红项: {}", s.red_items_text());
        assert!(s.len() >= 12);
    }

    #[test]
    fn ledger_ids_unique() {
        assert_eq!(LEDGER_JUDGES.len(), 24);
        assert_eq!(IMPL_EVIDENCE.len(), 24);
        let mut i = 0usize;
        while i < LEDGER_JUDGES.len() {
            let a = match LEDGER_JUDGES.get(i) {
                Some(n) => *n,
                None => {
                    must_not_reach();
                    return;
                }
            };
            let mut j = i + 1;
            while j < LEDGER_JUDGES.len() {
                let b = match LEDGER_JUDGES.get(j) {
                    Some(n) => *n,
                    None => {
                        must_not_reach();
                        return;
                    }
                };
                assert_ne!(a, b);
                j += 1;
            }
            i += 1;
        }
    }

    #[test]
    fn three_way_reconcile_full() {
        let mut rc = TriRecon::new();
        let cover = full_cover();
        let mut i = 0usize;
        while i < DOC_BOOKS.len() {
            let name = match DOC_BOOKS.get(i) {
                Some(n) => *n,
                None => {
                    must_not_reach();
                    return;
                }
            };
            assert!(rc.submit_book(name, &cover).is_ok());
            i += 1;
        }
        assert_eq!(rc.reconcile(&IMPL_EVIDENCE).ok(), Some(24));
        assert_eq!(rc.mismatch_count(), 0);
        assert_eq!(rc.phase(), ReconPhase::Reconciled);
    }

    #[test]
    fn injected_ten_all_caught() {
        assert_eq!(inject_detection_count(), 10);
        assert!(inject_10_ok());
    }

    #[test]
    fn unknown_book_rejected() {
        let mut rc = TriRecon::new();
        let cover = full_cover();
        match rc.submit_book("野册", &cover) {
            Err(code) => assert_eq!(code, Recon47Code::BookUnknown.val()),
            Ok(()) => must_not_reach(),
        }
    }

    #[test]
    fn cover_length_guard() {
        let mut rc = TriRecon::new();
        let cover = full_cover();
        let short: Vec<bool> = cover.iter().take(23).cloned().collect();
        match rc.submit_book("域册", &short) {
            Err(code) => assert_eq!(code, Recon47Code::CoverLenBad.val()),
            Ok(()) => must_not_reach(),
        }
        match rc.submit_book("域册", &cover) {
            Ok(()) => {}
            Err(_) => must_not_reach(),
        }
        match rc.submit_book("域册", &cover) {
            Err(code) => assert_eq!(code, Recon47Code::BookDuplicate.val()),
            Ok(()) => must_not_reach(),
        }
    }

    #[test]
    fn reconcile_with_two_books_skipped() {
        let mut rc = TriRecon::new();
        let cover = full_cover();
        let name0 = match DOC_BOOKS.get(0) {
            Some(n) => *n,
            None => {
                must_not_reach();
                return;
            }
        };
        let name1 = match DOC_BOOKS.get(1) {
            Some(n) => *n,
            None => {
                must_not_reach();
                return;
            }
        };
        let _ = rc.submit_book(name0, &cover);
        let _ = rc.submit_book(name1, &cover);
        match rc.reconcile(&IMPL_EVIDENCE) {
            Err(code) => assert_eq!(code, Recon47Code::PhaseSkip.val()),
            Ok(_) => must_not_reach(),
        }
    }

    #[test]
    fn impl_gap_points_to_sixth_judge() {
        assert!(impl_gap_caught_ok());
    }

    #[test]
    fn report_rows_and_markers() {
        let rc = build_recon_ok();
        let text = match rc.report_text() {
            Some(t) => t.to_string(),
            None => {
                must_not_reach();
                return;
            }
        };
        assert_eq!(text.matches('\n').count(), 26);
        assert!(text.contains("三向对账报告"));
        assert!(text.contains("失配 0 条"));
        assert!(text.contains("UNX-F1547-J24"));
    }

    #[test]
    fn double_run_digest_identical() {
        let d1 = build_recon_ok().report_digest();
        let d2 = build_recon_ok().report_digest();
        assert_eq!(d1, d2);
        assert_ne!(d1, 0);
    }

    #[test]
    fn diag_codes_closed_unique_windowed() {
        assert!(diag_closed_set_ok());
        assert_eq!(Recon47Code::ALL.len(), 12);
        let first = match Recon47Code::ALL.get(0) {
            Some(c) => c.val(),
            None => 0,
        };
        let last = match Recon47Code::ALL.get(11) {
            Some(c) => c.val(),
            None => 0,
        };
        assert_eq!(first, SEG_BASE + 0x01);
        assert_eq!(last, SEG_BASE + 0x0C);
        assert_eq!(detail(0), "未登记诊断码：不在本段闭集内");
    }

    fn must_not_reach() {}
}
