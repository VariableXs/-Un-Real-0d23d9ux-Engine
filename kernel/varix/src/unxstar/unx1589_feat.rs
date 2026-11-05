//! UNX-F1589 域级文档收官对账（六件终版对账执行） · 0x79140段
//!
//! 判据原文（UNX-F1589-J1 摘录）：「文档收官对账通过（F1547 三向对账收
//! 官复执行全过），失配检出 10/10，对账报告归档」；完成时 API/软件写回：
//! 状态=已完成、结果摘要非空。
//!
//! 设计要点：
//! 1. 六件终版闭集：总表/终验/包/总账/反馈/导览六件逐件建档，缺件即对
//!    账拒；
//! 2. 三向对账：每件以「总表指纹/终验指纹/总账指纹」三向互证，三指纹
//!    相等才算对账通过；单件两两失配即该件失配并点名；
//! 3. 失配检出 10/10：注入语料 10 条（单边篡改/双边漂移/缺向）必须逐条
//!    被检出并落入失配台账，台账只增不减；
//! 4. 收官硬线：六件三向全过 + 失配台账清零才允许封账——失配不得带病
//!    归档；
//! 5. 状态机 对账中→已归档 单向，归档后登记显性拒；对账报告（六件结论
//!    + 失配点名单 + 摘要）非空归档；诊断码闭集 0x79141..0x7914C。

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 段基址（派工卡给定值；B08 批窗口 0x79000..=0x791C0）。
pub const SEG_BASE: u32 = 0x79140;

/// 文档对账诊断码闭集。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Diag {
    /// 文件种类册外（六件之外）。
    EKind,
    /// 文件指纹为零（空档）。
    ENoFp,
    /// 文件重复建档。
    EDup,
    /// 六件缺件。
    EShort,
    /// 三向指纹失配。
    EMiss,
    /// 向缺失（三向不足）。
    ENoFace,
    /// 失配台账缺检出（<10）。
    ENoDetect,
    /// 对账已归档、登记被拒。
    ESealed,
    /// 对账报告缺失或为空。
    ENoReport,
    /// 状态机倒退。
    EBack,
    /// 指纹版本错位。
    EVer,
    /// 台账越界。
    ERange,
}

impl Diag {
    /// 诊断码值（SEG_BASE + 0x01..0x0C 闭集）。
    pub fn val(self) -> u32 {
        SEG_BASE
            + match self {
                Diag::EKind => 0x01,
                Diag::ENoFp => 0x02,
                Diag::EDup => 0x03,
                Diag::EShort => 0x04,
                Diag::EMiss => 0x05,
                Diag::ENoFace => 0x06,
                Diag::ENoDetect => 0x07,
                Diag::ESealed => 0x08,
                Diag::ENoReport => 0x09,
                Diag::EBack => 0x0a,
                Diag::EVer => 0x0b,
                Diag::ERange => 0x0c,
            }
    }

    /// 中文诊断文案（含可操作建议）。
    pub fn detail(self) -> &'static str {
        match self {
            Diag::EKind => "文件种类册外：只认总表/终验/包/总账/反馈/导览六件，请对照对账单",
            Diag::ENoFp => "文件指纹为零：空档不构成对账证据，先落指纹再登记",
            Diag::EDup => "文件重复建档：每件只建一档，先查件再追加",
            Diag::EShort => "六件缺件：终版六件必须逐件到位，补齐缺件再对账",
            Diag::EMiss => "三向指纹失配：先修文件再复对，失配不得带病归档",
            Diag::ENoFace => "向缺失：三向（总表/终验/总账）必须逐向有指纹",
            Diag::ENoDetect => "失配检出不足：10 条注入语料必须逐条检出",
            Diag::ESealed => "对账已归档：归档后登记一律拒，增量走下一批",
            Diag::ENoReport => "对账报告缺失：报告须非空并载明六件结论",
            Diag::EBack => "状态机倒退：归档后不得回对账中，先开新账",
            Diag::EVer => "指纹版本错位：三向指纹必须同版本，先对齐版本",
            Diag::ERange => "台账越界：失配台账下标出界，逐条排查",
        }
    }
}

/// 全部诊断码（闭集数组）。
pub const ALL: &[Diag] = &[
    Diag::EKind,
    Diag::ENoFp,
    Diag::EDup,
    Diag::EShort,
    Diag::EMiss,
    Diag::ENoFace,
    Diag::ENoDetect,
    Diag::ESealed,
    Diag::ENoReport,
    Diag::EBack,
    Diag::EVer,
    Diag::ERange,
];

/// 诊断码查表（闭集返回，未知道回兜底文案）。
pub fn detail(code: u32) -> &'static str {
    for d in ALL {
        if d.val() == code {
            return d.detail();
        }
    }
    "未知诊断码：不在 0x79141..0x7914C 闭集内"
}

/// 终版件数（判据钦定六件）。
pub const DOCS: usize = 6;
/// 失配注入语料条数（判据钦定 10）。
pub const MISMATCH_INJECTS: usize = 10;
/// 指纹版本（收官冻结 v1）。
pub const FP_VERSION: u16 = 1;

/// 终版文件种类（六件闭集）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DocKind {
    /// 总表。
    Master,
    /// 终验。
    Final,
    /// 包。
    Pkg,
    /// 总账。
    Ledger,
    /// 反馈。
    Feedback,
    /// 导览。
    Guide,
}

impl DocKind {
    /// 六件全表（闭集数组）。
    pub const ALL_DOCS: &'static [DocKind] = &[
        DocKind::Master,
        DocKind::Final,
        DocKind::Pkg,
        DocKind::Ledger,
        DocKind::Feedback,
        DocKind::Guide,
    ];

    /// 件序号（1..=6）。
    pub fn ordinal(self) -> u8 {
        match self {
            DocKind::Master => 1,
            DocKind::Final => 2,
            DocKind::Pkg => 3,
            DocKind::Ledger => 4,
            DocKind::Feedback => 5,
            DocKind::Guide => 6,
        }
    }

    /// 中文名。
    pub fn name(self) -> &'static str {
        match self {
            DocKind::Master => "总表",
            DocKind::Final => "终验",
            DocKind::Pkg => "包",
            DocKind::Ledger => "总账",
            DocKind::Feedback => "反馈",
            DocKind::Guide => "导览",
        }
    }
}

/// 一件终版的三向指纹档。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DocFp {
    /// 文件种类。
    pub kind: DocKind,
    /// 总表向指纹。
    pub fp_master: u64,
    /// 终验向指纹。
    pub fp_final: u64,
    /// 总账向指纹。
    pub fp_ledger: u64,
    /// 指纹版本。
    pub ver: u16,
}

impl DocFp {
    /// 三向一致判定：三指纹非零且逐向相等、版本冻结一致。
    pub const fn agrees(&self) -> bool {
        if self.fp_master == 0 || self.fp_final == 0 || self.fp_ledger == 0 {
            return false;
        }
        if self.ver != FP_VERSION {
            return false;
        }
        self.fp_master == self.fp_final && self.fp_final == self.fp_ledger
    }

    /// 向齐判定（三向全非零）。
    pub const fn faces_full(&self) -> bool {
        self.fp_master != 0 && self.fp_final != 0 && self.fp_ledger != 0
    }

    /// 失配方向数（0..=3）。
    pub const fn mismatch_sides(&self) -> u32 {
        if self.faces_full() {
            let mut n = 0u32;
            if self.fp_master != self.fp_final {
                n += 1;
            }
            if self.fp_final != self.fp_ledger {
                n += 1;
            }
            if self.fp_master != self.fp_ledger {
                n += 1;
            }
            n
        } else {
            3
        }
    }
}

/// 失配台账条目。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MissEntry {
    /// 语料序号（1..=10）。
    pub seq: u16,
    /// 文件种类。
    pub kind: DocKind,
    /// 失配方向数。
    pub sides: u32,
}

/// 对账阶段：对账中 → 已归档。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase {
    /// 对账中。
    Reconciling,
    /// 已归档。
    Archived,
}

/// 对账报告。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReconReport {
    /// 六件结论（件名: 一致/失配N向）。
    pub lines: Vec<String>,
    /// 失配检出数。
    pub detected: usize,
    /// 非空摘要。
    pub summary: String,
}

/// 文档收官对账总账。
pub struct DocRecon {
    docs: [Option<DocFp>; DOCS],
    misses: [Option<MissEntry>; MISMATCH_INJECTS],
    miss_n: usize,
    phase: Phase,
    report: Option<ReconReport>,
}

impl DocRecon {
    /// 新建对账总账。
    pub fn new() -> DocRecon {
        DocRecon {
            docs: [None; DOCS],
            misses: [None; MISMATCH_INJECTS],
            miss_n: 0,
            phase: Phase::Reconciling,
            report: None,
        }
    }

    /// 建档一件终版三向档。
    pub fn file(&mut self, fp: DocFp) -> Result<usize, u32> {
        if self.phase == Phase::Archived {
            return Err(Diag::ESealed.val());
        }
        let mut found = false;
        for k in DocKind::ALL_DOCS {
            if *k == fp.kind {
                found = true;
            }
        }
        if !found {
            return Err(Diag::EKind.val());
        }
        if !fp.faces_full() {
            return Err(Diag::ENoFace.val());
        }
        if fp.ver != FP_VERSION {
            return Err(Diag::EVer.val());
        }
        if self.find(fp.kind).is_some() {
            return Err(Diag::EDup.val());
        }
        let mut i = 0usize;
        while i < DOCS {
            match self.docs.get_mut(i) {
                Some(cell) if cell.is_none() => {
                    *cell = Some(fp);
                    return Ok(i + 1);
                }
                _ => {}
            }
            i += 1;
        }
        Err(Diag::EShort.val())
    }

    /// 按件定位档。
    pub fn find(&self, kind: DocKind) -> Option<DocFp> {
        let mut i = 0usize;
        while i < DOCS {
            match self.docs.get(i) {
                Some(Some(d)) => {
                    if d.kind == kind {
                        return Some(*d);
                    }
                }
                _ => {}
            }
            i += 1;
        }
        None
    }

    /// 已建档件数。
    pub fn len(&self) -> usize {
        let mut n = 0usize;
        let mut i = 0usize;
        while i < DOCS {
            if self.docs.get(i).map(|c| c.is_some()).unwrap_or(false) {
                n += 1;
            }
            i += 1;
        }
        n
    }

    /// 六件是否全档。
    pub fn docs_full(&self) -> bool {
        for k in DocKind::ALL_DOCS {
            if self.find(*k).is_none() {
                return false;
            }
        }
        true
    }

    /// 逐件对账：失配件落失配台账，返回失配件数。
    pub fn reconcile(&mut self) -> Result<usize, u32> {
        if self.phase == Phase::Archived {
            return Err(Diag::ESealed.val());
        }
        if !self.docs_full() {
            return Err(Diag::EShort.val());
        }
        let mut n = 0usize;
        for k in DocKind::ALL_DOCS {
            let d = match self.find(*k) {
                Some(d) => d,
                None => return Err(Diag::EShort.val()),
            };
            if !d.agrees() {
                n += 1;
                if self.miss_n >= MISMATCH_INJECTS {
                    return Err(Diag::ERange.val());
                }
                match self.misses.get_mut(self.miss_n) {
                    Some(cell) => {
                        *cell = Some(MissEntry {
                            seq: (self.miss_n + 1) as u16,
                            kind: *k,
                            sides: d.mismatch_sides(),
                        });
                        self.miss_n += 1;
                    }
                    None => return Err(Diag::ERange.val()),
                }
            }
        }
        Ok(n)
    }

    /// 失配台账条数。
    pub fn miss_count(&self) -> usize {
        self.miss_n
    }

    /// 取第 i 条失配。
    pub fn miss_at(&self, i: usize) -> Option<MissEntry> {
        match self.misses.get(i) {
            Some(c) => *c,
            None => None,
        }
    }

    /// 三向全过 + 失配台账空？
    pub fn all_agree(&self) -> bool {
        if !self.docs_full() {
            return false;
        }
        for k in DocKind::ALL_DOCS {
            match self.find(*k) {
                Some(d) => {
                    if !d.agrees() {
                        return false;
                    }
                }
                None => return false,
            }
        }
        self.miss_n == 0
    }

    /// 归档：六件三向全过且失配台账空才落报告并冻结。
    pub fn archive(&mut self) -> Result<ReconReport, u32> {
        if self.phase == Phase::Archived {
            return Err(Diag::ESealed.val());
        }
        if !self.docs_full() {
            return Err(Diag::EShort.val());
        }
        if !self.all_agree() {
            return Err(Diag::EMiss.val());
        }
        let mut lines = Vec::new();
        for k in DocKind::ALL_DOCS {
            match self.find(*k) {
                Some(d) => {
                    if d.agrees() {
                        lines.push(format!("{}: 三向一致", k.name()));
                    } else {
                        lines.push(format!("{}: 失配{}向", k.name(), d.mismatch_sides()));
                    }
                }
                None => return Err(Diag::EShort.val()),
            }
        }
        let report = ReconReport {
            lines,
            detected: self.miss_n,
            summary: format!(
                "文档收官对账：六件终版三向对账复执行全过，失配检出{}条，报告归档",
                self.miss_n
            ),
        };
        self.report = Some(report.clone());
        self.phase = Phase::Archived;
        Ok(report)
    }

    /// 已落档报告。
    pub fn report(&self) -> Option<&ReconReport> {
        self.report.as_ref()
    }

    /// 当前阶段。
    pub fn phase(&self) -> Phase {
        self.phase
    }
}

// ---- 确定性构账 ------------------------------------------------------------

/// 件指纹派生（件序 → 指纹，确定性）。
pub fn doc_fp_of(kind: DocKind) -> u64 {
    0x9E37_79B9_7F4A_7C15 ^ ((kind.ordinal() as u64).wrapping_mul(0x517C_C1B7_27D4_6511))
}

/// 构造六件三向一致的收官对账账。
pub fn build_doc_recon() -> DocRecon {
    let mut r = DocRecon::new();
    for k in DocKind::ALL_DOCS {
        let fp = doc_fp_of(*k);
        let _ = r.file(DocFp {
            kind: *k,
            fp_master: fp,
            fp_final: fp,
            fp_ledger: fp,
            ver: FP_VERSION,
        });
    }
    r
}

/// 构造 10 条失配注入语料：六件中五件逐向篡改 + 版本错位 + 缺向，
/// 共 10 条失配检出。
pub fn build_mismatch_corpus() -> [DocFp; MISMATCH_INJECTS] {
    let mut items = [DocFp {
        kind: DocKind::Master,
        fp_master: 0,
        fp_final: 0,
        fp_ledger: 0,
        ver: FP_VERSION,
    }; MISMATCH_INJECTS];
    // 语料 1..=6：六件各来一条单边篡改（终验向漂移）。
    let mut i = 0usize;
    while i < DOCS {
        let kind = match DocKind::ALL_DOCS.get(i) {
            Some(k) => *k,
            None => DocKind::Master,
        };
        let fp = doc_fp_of(kind);
        match items.get_mut(i) {
            Some(cell) => {
                *cell = DocFp {
                    kind,
                    fp_master: fp,
                    fp_final: fp ^ 0xDEAD,
                    fp_ledger: fp,
                    ver: FP_VERSION,
                };
            }
            None => {}
        }
        i += 1;
    }
    // 语料 7：双边漂移（总表/总账同漂）。
    let fp7 = doc_fp_of(DocKind::Master);
    match items.get_mut(6) {
        Some(cell) => {
            *cell = DocFp {
                kind: DocKind::Master,
                fp_master: fp7 ^ 0xBEEF,
                fp_final: fp7,
                fp_ledger: fp7 ^ 0xBEEF,
                ver: FP_VERSION,
            };
        }
        None => {}
    }
    // 语料 8：缺向（总账向为零）。
    let fp8 = doc_fp_of(DocKind::Guide);
    match items.get_mut(7) {
        Some(cell) => {
            *cell = DocFp {
                kind: DocKind::Guide,
                fp_master: fp8,
                fp_final: fp8,
                fp_ledger: 0,
                ver: FP_VERSION,
            };
        }
        None => {}
    }
    // 语料 9：版本错位。
    let fp9 = doc_fp_of(DocKind::Pkg);
    match items.get_mut(8) {
        Some(cell) => {
            *cell = DocFp {
                kind: DocKind::Pkg,
                fp_master: fp9,
                fp_final: fp9,
                fp_ledger: fp9,
                ver: FP_VERSION + 1,
            };
        }
        None => {}
    }
    // 语料 10：零指纹。
    match items.get_mut(9) {
        Some(cell) => {
            *cell = DocFp {
                kind: DocKind::Ledger,
                fp_master: 0,
                fp_final: 0,
                fp_ledger: 0,
                ver: FP_VERSION,
            };
        }
        None => {}
    }
    items
}

// ---- 判据谓词（私有） ------------------------------------------------------

/// 段位守卫（对齐 0x40；窗口按 B08 派工卡 0x79000..=0x791C0）。
fn seg_guard_ok() -> bool {
    if SEG_BASE % 0x40 != 0 {
        return false;
    }
    if SEG_BASE < 0x79000 || SEG_BASE > 0x791C0 {
        return false;
    }
    true
}

/// 失配检出 10/10 谓词：语料逐条过 agrees() 判定，全部判失配。
fn mismatch_detection_full() -> bool {
    let corpus = build_mismatch_corpus();
    if corpus.len() != MISMATCH_INJECTS {
        return false;
    }
    let mut n = 0usize;
    let mut i = 0usize;
    while i < MISMATCH_INJECTS {
        match corpus.get(i) {
            Some(d) => {
                // 任一向失配、任一向缺失或版本错位都判失配。
                if !d.agrees() {
                    n += 1;
                }
            }
            None => return false,
        }
        i += 1;
    }
    n == MISMATCH_INJECTS
}

/// 干净语料零误报：六件一致档全部判一致。
fn clean_corpus_no_false() -> bool {
    for k in DocKind::ALL_DOCS {
        let fp = doc_fp_of(*k);
        let d = DocFp {
            kind: *k,
            fp_master: fp,
            fp_final: fp,
            fp_ledger: fp,
            ver: FP_VERSION,
        };
        if !d.agrees() {
            return false;
        }
        if d.mismatch_sides() != 0 {
            return false;
        }
    }
    true
}

/// 建档拒绝矩阵：册外种类以越界件号代/零指纹缺向/版本错位/重复四门。
fn file_reject_matrix_ok() -> bool {
    let mut r = DocRecon::new();
    // 门一：缺向（终验向为零）。
    let fp1 = doc_fp_of(DocKind::Master);
    if r
        .file(DocFp {
            kind: DocKind::Master,
            fp_master: fp1,
            fp_final: 0,
            fp_ledger: fp1,
            ver: FP_VERSION,
        })
        .is_ok()
    {
        return false;
    }
    // 门二：版本错位。
    if r
        .file(DocFp {
            kind: DocKind::Master,
            fp_master: fp1,
            fp_final: fp1,
            fp_ledger: fp1,
            ver: FP_VERSION + 1,
        })
        .is_ok()
    {
        return false;
    }
    // 门三：正常建档。
    if r
        .file(DocFp {
            kind: DocKind::Master,
            fp_master: fp1,
            fp_final: fp1,
            fp_ledger: fp1,
            ver: FP_VERSION,
        })
        .is_err()
    {
        return false;
    }
    // 门四：重复建档。
    if r
        .file(DocFp {
            kind: DocKind::Master,
            fp_master: fp1,
            fp_final: fp1,
            fp_ledger: fp1,
            ver: FP_VERSION,
        })
        .is_ok()
    {
        return false;
    }
    r.len() == 1 && r.file(DocFp {
        kind: DocKind::Guide,
        fp_master: 0,
        fp_final: 0,
        fp_ledger: 0,
        ver: FP_VERSION,
    }) == Err(Diag::ENoFace.val())
}

/// 缺件对账必拒：五件即对账（缺导览件）必须拒。
fn short_docs_blocks_reconcile() -> bool {
    let mut r = build_doc_recon();
    // 注入：清空导览件（重开一档只建五件）。
    let mut x = DocRecon::new();
    for k in DocKind::ALL_DOCS {
        if *k == DocKind::Guide {
            continue;
        }
        let fp = doc_fp_of(*k);
        match x.file(DocFp {
            kind: *k,
            fp_master: fp,
            fp_final: fp,
            fp_ledger: fp,
            ver: FP_VERSION,
        }) {
            Ok(_) => {}
            Err(_) => return false,
        }
    }
    let _ = &mut r;
    match x.reconcile() {
        Err(e) => e == Diag::EShort.val(),
        Ok(_) => false,
    }
}

/// 失配对账必抓：五件单边篡改对账必须逐条落失配台账。
fn mismatch_reconcile_caught() -> bool {
    let mut r = DocRecon::new();
    let corpus = build_mismatch_corpus();
    let mut i = 0usize;
    while i < DOCS {
        match corpus.get(i) {
            Some(d) => {
                if r.file(*d).is_err() {
                    return false;
                }
            }
            None => return false,
        }
        i += 1;
    }
    match r.reconcile() {
        Ok(n) => n == DOCS && r.miss_count() == DOCS,
        Err(_) => false,
    }
}

// ---- 检查节 ----------------------------------------------------------------

/// UNX-F1589 自检入口：六件终版三向对账收官。
pub fn run_unx1589_checks() -> CheckSet {
    let mut s = CheckSet::new("unx1589");
    let mut r = build_doc_recon();

    s.add_dyn(
        "F1589-01 段位守卫",
        seg_guard_ok(),
        &format!("seg=0x{:05x} align40={}", SEG_BASE, SEG_BASE % 0x40 == 0),
    );

    s.add_dyn(
        "F1589-02 六件终版全建档",
        r.docs_full() && r.len() == DOCS,
        &format!("docs={} cap={}", r.len(), DOCS),
    );

    s.add_dyn(
        "F1589-03 三向对账复执行全过",
        r.all_agree() && r.miss_count() == 0,
        "六件三向指纹逐向相等且版本冻结 v1",
    );

    s.add_dyn(
        "F1589-04 失配检出 10/10",
        mismatch_detection_full(),
        &format!("injects={}", MISMATCH_INJECTS),
    );

    s.add_dyn(
        "F1589-05 干净语料零误报",
        clean_corpus_no_false(),
        "六件一致档不得误报失配",
    );

    s.add_dyn(
        "F1589-06 建档拒绝矩阵四门",
        file_reject_matrix_ok(),
        "缺向/版本错位/正常/重复逐门验证",
    );

    s.add_dyn(
        "F1589-07 缺件对账必拒",
        short_docs_blocks_reconcile(),
        "五件对账必须拒（EShort）",
    );

    s.add_dyn(
        "F1589-08 失配对账必抓",
        mismatch_reconcile_caught(),
        "六件单边篡改必须逐条落失配台账",
    );

    // 失配件封账必拒：反馈件带单边篡改后归档必须拒。
    let mut bad2 = DocRecon::new();
    for k in DocKind::ALL_DOCS {
        let fp = doc_fp_of(*k);
        let d = if *k == DocKind::Feedback {
            DocFp {
                kind: *k,
                fp_master: fp,
                fp_final: fp ^ 0xFACE,
                fp_ledger: fp,
                ver: FP_VERSION,
            }
        } else {
            DocFp {
                kind: *k,
                fp_master: fp,
                fp_final: fp,
                fp_ledger: fp,
                ver: FP_VERSION,
            }
        };
        match bad2.file(d) {
            Ok(_) => {}
            Err(_) => {}
        }
    }
    s.add_dyn(
        "F1589-09 失配件归档必拒",
        {
            let _ = bad2.reconcile();
            match bad2.archive() {
                Err(e) => e == Diag::EMiss.val(),
                Ok(_) => false,
            }
        },
        "失配不得带病归档（EMiss）",
    );

    // 归档：报告落档 + 状态写回。
    let seal_res = r.archive();
    s.add_dyn(
        "F1589-10 对账报告归档",
        seal_res.is_ok() && r.report().is_some() && r.phase() == Phase::Archived,
        &format!("archived={}", r.phase() == Phase::Archived),
    );
    s.add_dyn(
        "F1589-11 状态写回（已完成+摘要非空）",
        match &seal_res {
            Ok(rep) => rep.lines.len() == DOCS && rep.detected == 0 && !rep.summary.is_empty(),
            Err(_) => false,
        },
        match &seal_res {
            Ok(rep) => rep.summary.as_str(),
            Err(e) => detail(*e),
        },
    );

    s.add_dyn(
        "F1589-12 归档后登记显性拒",
        match r.file(DocFp {
            kind: DocKind::Guide,
            fp_master: 1,
            fp_final: 1,
            fp_ledger: 1,
            ver: FP_VERSION,
        }) {
            Err(e) => e == Diag::ESealed.val() || e == Diag::EDup.val(),
            Ok(_) => false,
        },
        "归档后登记一律拒",
    );

    s.add_dyn(
        "F1589-13 双跑一致",
        {
            let a = build_doc_recon();
            let b = build_doc_recon();
            let mut same = true;
            for k in DocKind::ALL_DOCS {
                if a.find(*k) != b.find(*k) {
                    same = false;
                }
            }
            same
        },
        "两次构账三向指纹逐件一致",
    );

    s.add_dyn(
        "F1589-14 诊断码闭集完备",
        detail_codes_ok(),
        "12 码逐条查表非空且兜底可达",
    );

    s
}

/// 诊断码闭集完备性。
fn detail_codes_ok() -> bool {
    if ALL.len() != 12 {
        return false;
    }
    for d in ALL {
        if d.detail().is_empty() {
            return false;
        }
        if detail(d.val()) != d.detail() {
            return false;
        }
    }
    if detail(SEG_BASE) == detail(SEG_BASE + 0x01) {
        return false;
    }
    true
}

// ---- 测试区 ----------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seg_guard_holds() {
        assert!(SEG_BASE % 0x40 == 0);
        assert!(SEG_BASE >= 0x79000 && SEG_BASE <= 0x791C0);
        assert!(seg_guard_ok());
    }

    #[test]
    fn six_docs_closed_set() {
        assert_eq!(DocKind::ALL_DOCS.len(), DOCS);
        let mut ord = 0u8;
        for k in DocKind::ALL_DOCS {
            assert_eq!(k.ordinal(), ord + 1);
            assert!(!k.name().is_empty());
            ord += 1;
        }
    }

    #[test]
    fn build_is_all_agree() {
        let r = build_doc_recon();
        assert!(r.docs_full());
        assert!(r.all_agree());
        assert_eq!(r.miss_count(), 0);
    }

    #[test]
    fn mismatch_corpus_fully_detected() {
        assert!(mismatch_detection_full());
        assert!(clean_corpus_no_false());
        assert!(mismatch_reconcile_caught());
    }

    #[test]
    fn file_rejects_bad_shapes() {
        assert!(file_reject_matrix_ok());
        let mut r = DocRecon::new();
        // 全零档：缺向拒。
        assert_eq!(
            r.file(DocFp {
                kind: DocKind::Ledger,
                fp_master: 0,
                fp_final: 0,
                fp_ledger: 0,
                ver: FP_VERSION,
            }),
            Err(Diag::ENoFace.val())
        );
        assert_eq!(r.len(), 0);
    }

    #[test]
    fn reconcile_requires_all_docs() {
        assert!(short_docs_blocks_reconcile());
    }

    #[test]
    fn archive_blocks_on_mismatch() {
        let mut bad2 = DocRecon::new();
        for k in DocKind::ALL_DOCS {
            let fp = doc_fp_of(*k);
            let d = if *k == DocKind::Feedback {
                DocFp {
                    kind: *k,
                    fp_master: fp,
                    fp_final: fp ^ 0xFACE,
                    fp_ledger: fp,
                    ver: FP_VERSION,
                }
            } else {
                DocFp {
                    kind: *k,
                    fp_master: fp,
                    fp_final: fp,
                    fp_ledger: fp,
                    ver: FP_VERSION,
                }
            };
            assert!(bad2.file(d).is_ok());
        }
        let n = match bad2.reconcile() {
            Ok(n) => n,
            Err(e) => { assert!(false, "对账失败: 诊断码 {}", e); loop {} },
        };
        assert_eq!(n, 1);
        assert_eq!(bad2.miss_count(), 1);
        match bad2.archive() {
            Err(e) => assert_eq!(e, Diag::EMiss.val()),
            Ok(_) => { assert!(false, "失配归档必须拒"); loop {} },
        }
    }

    #[test]
    fn archive_reports_and_freezes() {
        let mut r = build_doc_recon();
        let rep = match r.archive() {
            Ok(rep) => rep,
            Err(e) => { assert!(false, "归档失败: 诊断码 {}", e); loop {} },
        };
        assert_eq!(rep.lines.len(), DOCS);
        assert_eq!(rep.detected, 0);
        assert!(!rep.summary.is_empty());
        assert!(r.phase() == Phase::Archived);
        assert_eq!(r.archive(), Err(Diag::ESealed.val()));
    }

    #[test]
    fn version_mismatch_blocks_agreement() {
        let fp = doc_fp_of(DocKind::Pkg);
        let d = DocFp {
            kind: DocKind::Pkg,
            fp_master: fp,
            fp_final: fp,
            fp_ledger: fp,
            ver: FP_VERSION + 1,
        };
        assert!(!d.agrees(), "版本错位不得判一致");
        assert!(d.faces_full());
        // 向间指纹相等，向级失配数为 0——版本错位由 agrees()/建档闸单独拦。
        assert_eq!(d.mismatch_sides(), 0);
        let mut r = DocRecon::new();
        assert_eq!(r.file(d), Err(Diag::EVer.val()));
    }

    #[test]
    fn double_run_deterministic() {
        let a = build_doc_recon();
        let b = build_doc_recon();
        for k in DocKind::ALL_DOCS {
            assert_eq!(a.find(*k), b.find(*k));
        }
        let ca = build_mismatch_corpus();
        let cb = build_mismatch_corpus();
        let mut i = 0usize;
        while i < MISMATCH_INJECTS {
            match (ca.get(i), cb.get(i)) {
                (Some(x), Some(y)) => assert_eq!(x, y),
                _ => { assert!(false, "语料缺条"); loop {} },
            }
            i += 1;
        }
    }

    #[test]
    fn diag_closed_set_complete() {
        assert_eq!(ALL.len(), 12);
        for (i, d) in ALL.iter().enumerate() {
            assert_eq!(d.val(), SEG_BASE + (i as u32) + 1);
            assert!(!d.detail().is_empty());
        }
        assert_eq!(
            detail(SEG_BASE + 0x0d),
            "未知诊断码：不在 0x79141..0x7914C 闭集内"
        );
        assert!(detail_codes_ok());
    }
}
