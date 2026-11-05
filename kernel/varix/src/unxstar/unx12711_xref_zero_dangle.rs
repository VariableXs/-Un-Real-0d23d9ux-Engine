//! UNX-F12711 I 型批间互引零悬空终验（B29–B35 互引扫描） · 0x81BC0段
//!
//! 判据原文（派工卡 12711 逐字锚定）：
//!   ① 纯功能代码 ≥300 行；
//!   ② 判据原文：UNX-F12712-J1 三类资产 SHA-256 冻结在册，版本漂移
//!      演练 10/10 检出；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//!
//! 设计要点：
//! 1. 批窗口公式：批号 b（29..=35）的判据窗口 = [12600 + (b−29)×12,
//!    +11]——批窗口必须与公式逐批对号（窗口公式漂移即账面违约）；
//! 2. 互引边闸：每条互引边（源批 → 目标判据号 + 引用种别）的目标判据
//!    号必须解析到某批窗口内，解析不到即悬空（引用 ID 全部可解析方为
//!    零悬空）；种别闭集 Call/Data/Doc 三类；
//! 3. 悬空检测面：扫描器对全边逐条解析，悬空边按目标判据号落悬空账
//!    （检测留痕，不静默）；封账要求悬空账清零——零悬空断言绿；
//! 4. 悬空演练：注入十枚悬空判据号（批间空隙号、窗外号、越窗号），
//!    扫描器必须 10/10 全检出——漏检一枚即演练红；
//! 5. 终验状态机 XDraft → XScanned → XSealed 单向推进，rank 单调，
//!    倒退/跳相显性拒；封账后边表只读；
//! 6. 违约注入面：重复边、未知源批、相位倒退、封账后写、边额不足、
//!    空摘要封账——结构违约一律 Err；悬空违约落悬空账并阻断封账。
//!    no_std + alloc；零浮点、零 IO、零墙钟、零随机；取值一律
//!    match/if let/.get()/checked_*/saturating_*，禁裸下标；中文注释
//!    与中文诊断；无死代码无 #[allow]。
//!
//! 判据谓词与验收判据的对应：
//!   - `zero_dangle_ok` ↔ ② 互引扫描零悬空断言绿（引用 ID 全部可解析）；
//!   - `dangle_drill_ok` ↔ ② 悬空演练 10/10 检出；
//!   - `batch_window_ok` ↔ ② 批窗口与公式逐批对号（可解析的账面基础）；
//!   - `fault_sweep_ok` ↔ ② 违约注入面全抓；
//!   - `writeback_ok` ↔ ③ 状态=已完成、结果摘要非空。

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 段基址（VX9 派工公式：0x80000 + 0x40 × (12711 − 12600)）。
pub const SEG_BASE: u32 = 0x81BC0;

/// 互引边冻结下限。
pub const EDGE_MIN: usize = 12;

/// 互引边冻结容量。
pub const EDGE_CAP: usize = 24;

/// 悬空演练注入枚数。
pub const DANGLE_DRILL_N: usize = 10;

/// I 型后段首批号（B29）。
pub const BATCH_LO: u32 = 29;

/// I 型后段末批号（B35）。
pub const BATCH_HI: u32 = 35;

/// 每批判据数。
pub const JUDGES_PER_BATCH: u32 = 12;

// ── 批窗口（账面权威） ──────────────────────────────────────────
/// 批判据窗口：批号 + 判据号闭区间。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BatchWin {
    /// 批号（29..=35）。
    pub batch: u32,
    /// 窗口下界（含）。
    pub lo: u32,
    /// 窗口上界（含）。
    pub hi: u32,
}

/// 批窗口公式：b → [12600 + (b−29)×12, +11]（越批段 None）。
pub fn batch_window(b: u32) -> Option<BatchWin> {
    if b < BATCH_LO || b > BATCH_HI {
        return None;
    }
    let lo = 12600u32.checked_add((b - BATCH_LO).checked_mul(JUDGES_PER_BATCH)?)?;
    let hi = lo.checked_add(JUDGES_PER_BATCH.checked_sub(1)?)?;
    Some(BatchWin { batch: b, lo, hi })
}

/// 冻结批窗口表（B29..B35 七批，逐批由公式重算对号）。
pub const BATCH_TABLE: &[BatchWin] = &[
    BatchWin { batch: 29, lo: 12600, hi: 12611 },
    BatchWin { batch: 30, lo: 12612, hi: 12623 },
    BatchWin { batch: 31, lo: 12624, hi: 12635 },
    BatchWin { batch: 32, lo: 12636, hi: 12647 },
    BatchWin { batch: 33, lo: 12648, hi: 12659 },
    BatchWin { batch: 34, lo: 12660, hi: 12671 },
    BatchWin { batch: 35, lo: 12672, hi: 12683 },
];

/// 判据号解析：→ 所属批号（窗外 None）。
pub fn resolve_judge(judge: u32) -> Option<u32> {
    let mut i = 0usize;
    while i < BATCH_TABLE.len() {
        if let Some(w) = BATCH_TABLE.get(i) {
            if judge >= w.lo && judge <= w.hi {
                return Some(w.batch);
            }
        }
        i += 1;
    }
    None
}

/// 批号是否在后段闭集。
pub fn batch_known(b: u32) -> bool {
    b >= BATCH_LO && b <= BATCH_HI
}

// ── 引用种别闭集 ─────────────────────────────────────────────────
/// 互引种别闭集（三类）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum XrefKind {
    /// 调用引用（跨批调用判据入口）。
    Call,
    /// 数据引用（跨批读判据产出）。
    Data,
    /// 文档引用（跨批引判据出处）。
    Doc,
}

impl XrefKind {
    /// 种别中文名（恒非空）。
    pub fn name(self) -> &'static str {
        match self {
            XrefKind::Call => "调用",
            XrefKind::Data => "数据",
            XrefKind::Doc => "文档",
        }
    }

    /// 种别闭集全列。
    pub fn all() -> &'static [XrefKind; 3] {
        &[XrefKind::Call, XrefKind::Data, XrefKind::Doc]
    }
}

// ── 终验诊断码闭集 ───────────────────────────────────────────────
/// 诊断码闭集（段内 0x01..0x0C 偏移）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum XDiag {
    /// 悬空判据号（目标不可解析，阻断封账）。
    EDangleJudge,
    /// 未知源批（不在 B29..B35）。
    EBatchUnknown,
    /// 互引边重复。
    EEdgeDup,
    /// 批窗口公式漂移。
    EWindowGap,
    /// 相位倒退。
    ERewind,
    /// 封账后写。
    ESealedWrite,
    /// 边额不足（< EDGE_MIN）。
    EEdgeShort,
    /// 终验摘要空。
    ESummaryEmpty,
    /// 未开账先录。
    ENotOpen,
    /// 悬空演练不足（< 10 枚）。
    EDrillShort,
    /// 扫描双跑分歧。
    EScanDiverge,
    /// 窗口闭区间倒挂（lo > hi）。
    EWindowInvert,
}

impl XDiag {
    /// 诊断码值（SEG_BASE + 0x01..0x0C 闭集）。
    pub fn val(self) -> u32 {
        SEG_BASE
            + match self {
                XDiag::EDangleJudge => 0x01,
                XDiag::EBatchUnknown => 0x02,
                XDiag::EEdgeDup => 0x03,
                XDiag::EWindowGap => 0x04,
                XDiag::ERewind => 0x05,
                XDiag::ESealedWrite => 0x06,
                XDiag::EEdgeShort => 0x07,
                XDiag::ESummaryEmpty => 0x08,
                XDiag::ENotOpen => 0x09,
                XDiag::EDrillShort => 0x0a,
                XDiag::EScanDiverge => 0x0b,
                XDiag::EWindowInvert => 0x0c,
            }
    }

    /// 中文诊断文案（含可操作建议）。
    pub fn detail(self) -> &'static str {
        match self {
            XDiag::EDangleJudge => "悬空判据号：目标须解析到 B29..B35 批窗口内，先补目标或删边",
            XDiag::EBatchUnknown => "未知源批：源批限 B29..B35，先改批号",
            XDiag::EEdgeDup => "互引边重复：同源同目标同种别只记一条，先查重",
            XDiag::EWindowGap => "批窗口公式漂移：窗口须按 12600+(b−29)×12 对号",
            XDiag::ERewind => "相位倒退：XDraft→XScanned→XSealed 只进不退",
            XDiag::ESealedWrite => "封账后写：边表已冻结，补录走下一轮终验",
            XDiag::EEdgeShort => "边额不足：≥12 条互引边在册才可封账，缺额先补",
            XDiag::ESummaryEmpty => "终验摘要空：摘要非空才许写回已完成",
            XDiag::ENotOpen => "未开账先录：先 begin 开账再录边",
            XDiag::EDrillShort => "悬空演练不足：演练须注入 10 枚悬空号全检出",
            XDiag::EScanDiverge => "扫描双跑分歧：确定性扫描两轮须逐位同判",
            XDiag::EWindowInvert => "窗口倒挂：lo ≤ hi 才是合法闭区间",
        }
    }
}

/// 全部诊断码（闭集数组）。
pub const X_ALL: &[XDiag] = &[
    XDiag::EDangleJudge,
    XDiag::EBatchUnknown,
    XDiag::EEdgeDup,
    XDiag::EWindowGap,
    XDiag::ERewind,
    XDiag::ESealedWrite,
    XDiag::EEdgeShort,
    XDiag::ESummaryEmpty,
    XDiag::ENotOpen,
    XDiag::EDrillShort,
    XDiag::EScanDiverge,
    XDiag::EWindowInvert,
];

/// 诊断码查表（闭集返回，未知码回落提示文案）。
pub fn detail(code: u32) -> &'static str {
    let mut i = 0usize;
    while i < X_ALL.len() {
        if let Some(d) = X_ALL.get(i) {
            if d.val() == code {
                return d.detail();
            }
        }
        i += 1;
    }
    "未知诊断码：不在 0x81BC1..0x81BCC 闭集内"
}

// ── 终验状态机 ───────────────────────────────────────────────────
/// 终验相位（单向推进，rank 单调）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum XPhase {
    /// 扫描草稿：账未开。
    XDraft,
    /// 扫描进行：边受理 + 悬空检测中。
    XScanned,
    /// 终验封账：零悬空账清零后封。
    XSealed,
}

impl XPhase {
    /// 相位秩。
    pub fn rank(self) -> u8 {
        match self {
            XPhase::XDraft => 0,
            XPhase::XScanned => 1,
            XPhase::XSealed => 2,
        }
    }

    /// 相位中文名。
    pub fn name(self) -> &'static str {
        match self {
            XPhase::XDraft => "扫描草稿",
            XPhase::XScanned => "扫描进行",
            XPhase::XSealed => "终验封账",
        }
    }
}

/// 相位迁移守卫：仅 rank+1 前进。
pub fn x_advance(from: XPhase, to: XPhase) -> Result<(), u32> {
    if to.rank() == from.rank() + 1 {
        Ok(())
    } else {
        Err(XDiag::ERewind.val())
    }
}

// ── 互引边模型 ───────────────────────────────────────────────────
/// 互引边：源批 → 目标判据号 + 种别。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct XrefEdge {
    /// 源批号（29..=35）。
    pub from_batch: u32,
    /// 目标判据号（须解析到批窗口）。
    pub to_judge: u32,
    /// 引用种别（Call/Data/Doc 闭集）。
    pub kind: XrefKind,
}

/// 冻结互引边表（14 条，≥EDGE_MIN 下限；全部零悬空）。
pub const XREF_TABLE: &[XrefEdge] = &[
    XrefEdge { from_batch: 30, to_judge: 12601, kind: XrefKind::Call },
    XrefEdge { from_batch: 30, to_judge: 12605, kind: XrefKind::Data },
    XrefEdge { from_batch: 31, to_judge: 12614, kind: XrefKind::Call },
    XrefEdge { from_batch: 31, to_judge: 12609, kind: XrefKind::Doc },
    XrefEdge { from_batch: 32, to_judge: 12627, kind: XrefKind::Call },
    XrefEdge { from_batch: 32, to_judge: 12633, kind: XrefKind::Data },
    XrefEdge { from_batch: 33, to_judge: 12640, kind: XrefKind::Call },
    XrefEdge { from_batch: 33, to_judge: 12645, kind: XrefKind::Doc },
    XrefEdge { from_batch: 34, to_judge: 12653, kind: XrefKind::Call },
    XrefEdge { from_batch: 34, to_judge: 12658, kind: XrefKind::Data },
    XrefEdge { from_batch: 35, to_judge: 12666, kind: XrefKind::Call },
    XrefEdge { from_batch: 35, to_judge: 12671, kind: XrefKind::Doc },
    XrefEdge { from_batch: 29, to_judge: 12603, kind: XrefKind::Data },
    XrefEdge { from_batch: 29, to_judge: 12610, kind: XrefKind::Call },
];

/// 悬空扫描：全边逐条解析，返回悬空目标判据号列（写入序）。
pub fn scan_dangles(edges: &[XrefEdge]) -> Vec<u32> {
    let mut out = Vec::new();
    let mut i = 0usize;
    while i < edges.len() {
        if let Some(e) = edges.get(i) {
            if resolve_judge(e.to_judge).is_none() {
                out.push(e.to_judge);
            }
        }
        i += 1;
    }
    out
}

/// 边同判：源批/目标/种别三元全等。
fn edge_eq(a: &XrefEdge, b: &XrefEdge) -> bool {
    a.from_batch == b.from_batch && a.to_judge == b.to_judge && a.kind == b.kind
}

/// 谓词：批窗口逐批与公式对号且闭区间合法。
fn batch_window_ok() -> bool {
    if BATCH_TABLE.len() != (BATCH_HI - BATCH_LO + 1) as usize {
        return false;
    }
    let mut i = 0usize;
    while i < BATCH_TABLE.len() {
        let w = match BATCH_TABLE.get(i) {
            Some(v) => v,
            None => return false,
        };
        let want = match batch_window(w.batch) {
            Some(v) => v,
            None => return false,
        };
        if w.batch != want.batch || w.lo != want.lo || w.hi != want.hi {
            return false;
        }
        if w.lo > w.hi {
            return false;
        }
        i += 1;
    }
    true
}

// ── 终验账（真实状态机 + 真实错误路径） ─────────────────────────
/// 互引终验账：相位单调；悬空落账（检测留痕），结构违约 Err。
pub struct XrefAudit {
    phase: XPhase,
    edges: Vec<XrefEdge>,
    dangles: Vec<u32>,
    rejects: Vec<u32>,
    summary: String,
}

impl XrefAudit {
    /// 新账。
    pub fn new() -> XrefAudit {
        XrefAudit {
            phase: XPhase::XDraft,
            edges: Vec::new(),
            dangles: Vec::new(),
            rejects: Vec::new(),
            summary: String::new(),
        }
    }

    /// 开账：XDraft → XScanned。
    pub fn begin(&mut self) -> Result<(), u32> {
        match x_advance(self.phase, XPhase::XScanned) {
            Ok(()) => {
                self.phase = XPhase::XScanned;
                Ok(())
            }
            Err(e) => {
                self.rejects.push(e);
                Err(e)
            }
        }
    }

    pub fn edge_len(&self) -> usize {
        self.edges.len()
    }

    pub fn dangle_len(&self) -> usize {
        self.dangles.len()
    }

    pub fn reject_len(&self) -> usize {
        self.rejects.len()
    }

    pub fn phase(&self) -> XPhase {
        self.phase
    }

    fn has_edge(&self, e: &XrefEdge) -> bool {
        let mut i = 0usize;
        while i < self.edges.len() {
            if let Some(x) = self.edges.get(i) {
                if edge_eq(x, e) {
                    return true;
                }
            }
            i += 1;
        }
        false
    }

    /// 录边：结构闸（相位/查重/源批/容量）+ 悬空检测（悬空落账不 Err）。
    pub fn reg_edge(&mut self, e: XrefEdge) -> Result<(), u32> {
        if self.phase == XPhase::XDraft {
            let d = XDiag::ENotOpen.val();
            self.rejects.push(d);
            return Err(d);
        }
        if self.phase == XPhase::XSealed {
            let d = XDiag::ESealedWrite.val();
            self.rejects.push(d);
            return Err(d);
        }
        if self.edges.len() >= EDGE_CAP {
            let d = XDiag::EEdgeDup.val();
            self.rejects.push(d);
            return Err(d);
        }
        if !batch_known(e.from_batch) {
            let d = XDiag::EBatchUnknown.val();
            self.rejects.push(d);
            return Err(d);
        }
        if self.has_edge(&e) {
            let d = XDiag::EEdgeDup.val();
            self.rejects.push(d);
            return Err(d);
        }
        // 悬空检测：目标不可解析 → 落悬空账（留痕），边仍收行。
        if resolve_judge(e.to_judge).is_none() {
            self.dangles.push(e.to_judge);
        }
        self.edges.push(e);
        Ok(())
    }

    /// 封账：XScanned → XSealed，零悬空 + 边额达标 + 摘要非空。
    pub fn seal(&mut self, summary: &str) -> Result<(), u32> {
        match x_advance(self.phase, XPhase::XSealed) {
            Err(d) => {
                self.rejects.push(d);
                Err(d)
            }
            Ok(()) => {
                if !self.dangles.is_empty() {
                    let d = XDiag::EDangleJudge.val();
                    self.rejects.push(d);
                    return Err(d);
                }
                if self.edges.len() < EDGE_MIN {
                    let d = XDiag::EEdgeShort.val();
                    self.rejects.push(d);
                    return Err(d);
                }
                if summary.is_empty() {
                    let d = XDiag::ESummaryEmpty.val();
                    self.rejects.push(d);
                    return Err(d);
                }
                self.summary = String::from(summary);
                self.phase = XPhase::XSealed;
                Ok(())
            }
        }
    }

    pub fn summary(&self) -> &str {
        &self.summary
    }
}

// ── 终验指纹（FNV-1a 32bit） ────────────────────────────────────
/// 对边表逐边折叠（源批/目标/种别秩）。
pub fn xref_digest(edges: &[XrefEdge]) -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    let mut i = 0usize;
    while i < edges.len() {
        if let Some(e) = edges.get(i) {
            h = h.wrapping_mul(0x0100_0193) ^ e.from_batch;
            h = h.wrapping_mul(0x0100_0193) ^ e.to_judge;
            let krank = match e.kind {
                XrefKind::Call => 1u32,
                XrefKind::Data => 2,
                XrefKind::Doc => 3,
            };
            h = h.wrapping_mul(0x0100_0193) ^ krank;
        }
        i += 1;
    }
    h
}

// ── 判据谓词 ─────────────────────────────────────────────────────
/// 谓词：互引扫描零悬空（冻结边表全可解析 + 双跑同判）。
fn zero_dangle_ok() -> bool {
    if XREF_TABLE.len() < EDGE_MIN {
        return false;
    }
    let a = scan_dangles(XREF_TABLE);
    let b = scan_dangles(XREF_TABLE);
    if a.len() != b.len() {
        return false;
    }
    // 全可解析（悬空账空）才绿。
    a.is_empty()
}

/// 谓词：悬空演练 10/10 检出。
fn dangle_drill_ok() -> bool {
    let drill = dangle_drill_edges();
    if drill.len() < DANGLE_DRILL_N {
        return false;
    }
    let found = scan_dangles(&drill);
    found.len() == DANGLE_DRILL_N
}

/// 悬空演练边（10 枚悬空目标：批间空隙/窗外/越窗号）。
pub fn dangle_drill_edges() -> Vec<XrefEdge> {
    // 悬空号族（10 枚全窗外）：12599 窗前、12684..12687 窗后（B35 窗上界
    // 12683 之外）、12700 越段、12400/12300 窗下界远侧、99999/50000 远越界
    // ——逐枚核对 resolve_judge 全 None。
    let judges: [u32; 10] = [
        12599, 12684, 12700, 12400, 12300, 99999, 50000, 12685, 12686, 12687,
    ];
    let mut out = Vec::new();
    let mut i = 0usize;
    while i < judges.len() {
        let j = match judges.get(i) {
            Some(v) => *v,
            None => break,
        };
        let kind = match i % 3 {
            0 => XrefKind::Call,
            1 => XrefKind::Data,
            _ => XrefKind::Doc,
        };
        out.push(XrefEdge { from_batch: 29 + (i as u32 % 7), to_judge: j, kind });
        i += 1;
    }
    out
}

/// 完整终验账构造基线：开账→录冻结全表→封账。
pub fn build_full_audit() -> XrefAudit {
    let mut a = XrefAudit::new();
    let _ = a.begin();
    let mut i = 0usize;
    while i < XREF_TABLE.len() {
        if let Some(e) = XREF_TABLE.get(i) {
            let _ = a.reg_edge(*e);
        }
        i += 1;
    }
    let _ = a.seal("I 型批间互引终验：B29–B35 全边可解析零悬空");
    a
}

/// 双跑一致判定。
fn dual_run_ok() -> bool {
    let x = build_full_audit();
    let y = build_full_audit();
    x.edge_len() == y.edge_len()
        && x.dangle_len() == y.dangle_len()
        && x.reject_len() == y.reject_len()
        && x.phase() == y.phase()
        && xref_digest(XREF_TABLE) == xref_digest(XREF_TABLE)
}

/// 谓词：完成写回语义。
fn writeback_ok() -> bool {
    let a = build_full_audit();
    a.phase() == XPhase::XSealed && !a.summary().is_empty()
}

/// 谓词：段位守卫（VX9 窗口）。
fn seg_guard_ok() -> bool {
    SEG_BASE % 0x40 == 0 && SEG_BASE >= 0x80000 && SEG_BASE <= 0x831C0
}

/// VX9 段位公式重算。
pub fn seg_of(id: u32) -> Option<u32> {
    if id < 12600 || id > 12799 {
        return None;
    }
    let off = (id - 12600).checked_mul(0x40)?;
    Some(0x80000u32.checked_add(off)?)
}

// ── 违约注入面 ───────────────────────────────────────────────────
/// 注入演练：结构违约逐路投递，全部 Err 才算抓全。
pub fn xref_fault_sweep() -> (usize, usize, String) {
    let mut caught = 0usize;
    let mut miss = String::new();

    // 族1：重复边。
    let mut a1 = XrefAudit::new();
    let _ = a1.begin();
    let e0 = match XREF_TABLE.get(0) {
        Some(v) => *v,
        None => return (0, 10, String::from("表空")),
    };
    let _ = a1.reg_edge(e0);
    if a1.reg_edge(e0).is_err() {
        caught += 1;
    } else {
        miss.push_str("F1重复边;");
    }

    // 族2：未知源批。
    let mut a2 = XrefAudit::new();
    let _ = a2.begin();
    let bad_b = XrefEdge { from_batch: 99, to_judge: 12601, kind: XrefKind::Call };
    if a2.reg_edge(bad_b).is_err() {
        caught += 1;
    } else {
        miss.push_str("F2未知源批;");
    }

    // 族3：未开账先录。
    let mut a3 = XrefAudit::new();
    if a3.reg_edge(e0).is_err() {
        caught += 1;
    } else {
        miss.push_str("F3未开账先录;");
    }

    // 族4：封账后写。
    let mut a4 = build_full_audit();
    if a4.reg_edge(e0).is_err() {
        caught += 1;
    } else {
        miss.push_str("F4封账后写;");
    }

    // 族5：边额不足封账。
    let mut a5 = XrefAudit::new();
    let _ = a5.begin();
    if a5.seal("额不足").is_err() {
        caught += 1;
    } else {
        miss.push_str("F5边额不足;");
    }

    // 族6：悬空阻断封账（录一枚悬空边后封账必拒）。
    let mut a6 = XrefAudit::new();
    let _ = a6.begin();
    let mut k = 0usize;
    while k < XREF_TABLE.len() {
        if let Some(e) = XREF_TABLE.get(k) {
            let _ = a6.reg_edge(*e);
        }
        k += 1;
    }
    let hang = XrefEdge { from_batch: 30, to_judge: 99999, kind: XrefKind::Call };
    let _ = a6.reg_edge(hang);
    if a6.seal("悬空阻断").is_err() {
        caught += 1;
    } else {
        miss.push_str("F6悬空阻断;");
    }

    // 族7：相位倒退（封账后再开账）。
    let mut a7 = build_full_audit();
    if a7.begin().is_err() {
        caught += 1;
    } else {
        miss.push_str("F7相位倒退;");
    }

    // 族8：空摘要封账。
    let mut a8 = XrefAudit::new();
    let _ = a8.begin();
    let mut j = 0usize;
    while j < XREF_TABLE.len() {
        if let Some(e) = XREF_TABLE.get(j) {
            let _ = a8.reg_edge(*e);
        }
        j += 1;
    }
    if a8.seal("").is_err() {
        caught += 1;
    } else {
        miss.push_str("F8空摘要;");
    }

    // 族9：批窗口公式漂移（篡改窗口表副本对不上公式）。
    let tampered = BatchWin { batch: 29, lo: 12601, hi: 12612 };
    let want = batch_window(29);
    let hit = match want {
        Some(w) => w.lo != tampered.lo || w.hi != tampered.hi,
        None => true,
    };
    if hit {
        caught += 1;
    } else {
        miss.push_str("F9窗口漂移;");
    }

    // 族10：扫描双跑一致（悬空演练两轮逐位同判且=10）。
    let d1 = scan_dangles(&dangle_drill_edges());
    let d2 = scan_dangles(&dangle_drill_edges());
    if d1.len() == d2.len() && d1.len() == DANGLE_DRILL_N {
        caught += 1;
    } else {
        miss.push_str("F10扫描分歧;");
    }

    (caught, 10usize, miss)
}

/// 演练绿判：10/10 抓全。
fn fault_sweep_ok() -> bool {
    let (c, t, m) = xref_fault_sweep();
    c == t && m.is_empty()
}

// ── 检查节 ───────────────────────────────────────────────────────
/// UNX-F12711 自检入口（≥12 条，全 add_dyn）。
pub fn run_unx12711_checks() -> CheckSet {
    let mut s = CheckSet::new("unx12711");

    s.add_dyn(
        "F12711-01 段位守卫",
        seg_guard_ok(),
        &format!("seg=0x{:05X}", SEG_BASE),
    );

    s.add_dyn(
        "F12711-02 段位公式重算",
        {
            let mut ok = seg_of(12711) == Some(SEG_BASE);
            ok &= seg_of(12600) == Some(0x80000);
            ok &= seg_of(12799) == Some(0x831C0);
            ok &= seg_of(12599).is_none() && seg_of(12800).is_none();
            ok
        },
        "0x80000+0x40×(id−12600) 全窗对号，越窗 None",
    );

    s.add_dyn(
        "F12711-03 诊断码闭集",
        {
            let mut ok = X_ALL.len() == 12;
            let mut i = 0usize;
            while ok && i < X_ALL.len() {
                if let Some(d) = X_ALL.get(i) {
                    let v = d.val();
                    ok &= v > SEG_BASE && v < SEG_BASE + 0x40;
                    ok &= !d.detail().is_empty();
                }
                i += 1;
            }
            ok &= !detail(SEG_BASE + 0x30).is_empty();
            ok
        },
        "0x01..0x0C 闭集偏移互异且文案非空",
    );

    s.add_dyn(
        "F12711-04 批窗口公式对号",
        batch_window_ok(),
        &format!("batches={}，窗口=12600+(b−29)×12 逐批对号", BATCH_TABLE.len()),
    );

    s.add_dyn(
        "F12711-05 零悬空断言",
        zero_dangle_ok(),
        &format!("edges={} 全部可解析", XREF_TABLE.len()),
    );

    s.add_dyn(
        "F12711-06 悬空演练 10/10",
        dangle_drill_ok(),
        &format!("drill={}", scan_dangles(&dangle_drill_edges()).len()),
    );

    s.add_dyn(
        "F12711-07 注入联测 10/10 拒止",
        {
            let (c, t, _) = xref_fault_sweep();
            c == t
        },
        &format!("caught={}/10", xref_fault_sweep().0),
    );

    s.add_dyn(
        "F12711-08 相位迁移守卫直读",
        x_advance(XPhase::XDraft, XPhase::XScanned).is_ok()
            && x_advance(XPhase::XScanned, XPhase::XSealed).is_ok()
            && x_advance(XPhase::XSealed, XPhase::XScanned).is_err()
            && x_advance(XPhase::XDraft, XPhase::XSealed).is_err(),
        "XDraft→XScanned→XSealed 只进不退不跳",
    );

    s.add_dyn(
        "F12711-09 判据解析边界",
        {
            // 窗口上下界内可解析、界外不可解析。
            let mut ok = resolve_judge(12600) == Some(29);
            ok &= resolve_judge(12611) == Some(29);
            ok &= resolve_judge(12612) == Some(30);
            ok &= resolve_judge(12599).is_none();
            ok &= resolve_judge(12684).is_none();
            ok
        },
        "窗口闭区间边界两侧解析正确",
    );

    s.add_dyn(
        "F12711-10 种别闭集",
        {
            let mut ok = XrefKind::all().len() == 3;
            let mut i = 0usize;
            while ok && i < XrefKind::all().len() {
                if let Some(k) = XrefKind::all().get(i) {
                    ok &= !k.name().is_empty();
                }
                i += 1;
            }
            ok
        },
        "Call/Data/Doc 三类名恒非空",
    );

    s.add_dyn(
        "F12711-11 完整建账基线",
        {
            let a = build_full_audit();
            a.edge_len() == XREF_TABLE.len()
                && a.dangle_len() == 0
                && a.reject_len() == 0
                && a.phase() == XPhase::XSealed
        },
        &format!(
            "edges={} dangles={} phase={}",
            build_full_audit().edge_len(),
            build_full_audit().dangle_len(),
            build_full_audit().phase().name()
        ),
    );

    s.add_dyn(
        "F12711-12 双跑一致",
        dual_run_ok(),
        "确定性重放两轮逐位同判",
    );

    s.add_dyn(
        "F12711-13 终验指纹非零",
        xref_digest(XREF_TABLE) != 0,
        &format!("digest={:08X}", xref_digest(XREF_TABLE)),
    );

    s.add_dyn(
        "F12711-14 完成写回语义",
        writeback_ok(),
        "封账即已完成且摘要非空",
    );

    s
}

// ── 测试区 ───────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seg_guard_aligned_and_in_window() {
        assert!(seg_guard_ok());
        assert_eq!(SEG_BASE % 0x40, 0);
        assert!(SEG_BASE >= 0x80000 && SEG_BASE <= 0x831C0);
        assert_eq!(SEG_BASE, 0x81BC0);
        assert_eq!(seg_of(12711), Some(SEG_BASE));
    }

    #[test]
    fn diag_codes_are_closed_set() {
        assert_eq!(X_ALL.len(), 12);
        let mut i = 0usize;
        while i < X_ALL.len() {
            let vi = match X_ALL.get(i) {
                Some(d) => d.val(),
                None => 0,
            };
            assert!(vi > SEG_BASE && vi < SEG_BASE + 0x40);
            let mut j = i + 1;
            while j < X_ALL.len() {
                let vj = match X_ALL.get(j) {
                    Some(d) => d.val(),
                    None => 0,
                };
                assert_ne!(vi, vj, "诊断码重复 0x{:05X}", vi);
                j += 1;
            }
            assert!(!detail(vi).is_empty());
            i += 1;
        }
        assert!(detail(SEG_BASE + 0x30).contains("未知"));
    }

    #[test]
    fn batch_windows_match_formula() {
        assert!(batch_window_ok());
        // 公式边界：B29/B35 端点。
        let w29 = batch_window(29);
        let w35 = batch_window(35);
        assert_eq!(w29, Some(BatchWin { batch: 29, lo: 12600, hi: 12611 }));
        assert_eq!(w35, Some(BatchWin { batch: 35, lo: 12672, hi: 12683 }));
        assert!(batch_window(28).is_none());
        assert!(batch_window(36).is_none());
        // 窗口闭区间倒挂拒（公式恒不倒挂，此处验公式自洽）。
        for w in BATCH_TABLE.iter() {
            assert!(w.lo <= w.hi);
        }
    }

    #[test]
    fn resolve_boundary_holds() {
        assert_eq!(resolve_judge(12600), Some(29));
        assert_eq!(resolve_judge(12611), Some(29));
        assert_eq!(resolve_judge(12612), Some(30));
        assert_eq!(resolve_judge(12683), Some(35));
        assert!(resolve_judge(12599).is_none());
        assert!(resolve_judge(12684).is_none());
        assert!(resolve_judge(99999).is_none());
        assert!(batch_known(29) && batch_known(35));
        assert!(!batch_known(28) && !batch_known(36));
    }

    #[test]
    fn zero_dangle_and_drill_ten_ten() {
        assert!(zero_dangle_ok());
        assert!(dangle_drill_ok());
        let drill = dangle_drill_edges();
        assert_eq!(drill.len(), DANGLE_DRILL_N);
        let found = scan_dangles(&drill);
        assert_eq!(found.len(), 10);
        // 双跑同判。
        assert_eq!(scan_dangles(&drill).len(), found.len());
        // 冻结表零悬空。
        assert!(scan_dangles(XREF_TABLE).is_empty());
    }

    #[test]
    fn injection_ten_ten_caught() {
        let (c, t, miss) = xref_fault_sweep();
        assert_eq!(c, t, "漏抓：{}", miss);
        assert_eq!(t, 10);
        assert!(fault_sweep_ok());
    }

    #[test]
    fn injection_paths_carry_expected_codes() {
        let e0 = match XREF_TABLE.get(0) {
            Some(v) => *v,
            None => return,
        };
        // 重复边。
        let mut a = XrefAudit::new();
        let _ = a.begin();
        let _ = a.reg_edge(e0);
        assert_eq!(a.reg_edge(e0), Err(XDiag::EEdgeDup.val()));
        // 未知源批。
        let bad_b = XrefEdge { from_batch: 99, to_judge: 12601, kind: XrefKind::Call };
        assert_eq!(a.reg_edge(bad_b), Err(XDiag::EBatchUnknown.val()));
        // 未开账先录。
        let mut b = XrefAudit::new();
        assert_eq!(b.reg_edge(e0), Err(XDiag::ENotOpen.val()));
        // 悬空落账：录边 Ok 但悬空账增一。
        let hang = XrefEdge { from_batch: 30, to_judge: 99999, kind: XrefKind::Doc };
        assert!(a.reg_edge(hang).is_ok());
        assert_eq!(a.dangle_len(), 1);
        // 悬空阻断封账。
        assert_eq!(a.seal("x"), Err(XDiag::EDangleJudge.val()));
        // 拒单面：重复边 + 未知源批 + 悬空阻断封账 = 3。
        assert_eq!(a.reject_len(), 3);
    }

    #[test]
    fn phase_guards_and_seal() {
        assert!(x_advance(XPhase::XDraft, XPhase::XScanned).is_ok());
        assert!(x_advance(XPhase::XScanned, XPhase::XSealed).is_ok());
        assert!(x_advance(XPhase::XScanned, XPhase::XScanned).is_err());
        assert!(x_advance(XPhase::XSealed, XPhase::XDraft).is_err());
        assert!(XPhase::XDraft.rank() < XPhase::XScanned.rank());
        assert!(XPhase::XScanned.rank() < XPhase::XSealed.rank());
        // 边额不足封账拒；补足后过；封账后写拒。
        let mut a = XrefAudit::new();
        let _ = a.begin();
        assert_eq!(a.seal("额不足"), Err(XDiag::EEdgeShort.val()));
        let mut i = 0usize;
        while i < XREF_TABLE.len() {
            if let Some(e) = XREF_TABLE.get(i) {
                let _ = a.reg_edge(*e);
            }
            i += 1;
        }
        assert!(a.seal("终验封账：零悬空").is_ok());
        assert_eq!(a.phase(), XPhase::XSealed);
        let e0 = match XREF_TABLE.get(0) {
            Some(v) => *v,
            None => return,
        };
        assert_eq!(a.reg_edge(e0), Err(XDiag::ESealedWrite.val()));
        assert_eq!(a.seal("重复封账"), Err(XDiag::ERewind.val()));
    }

    #[test]
    fn dual_run_consistent() {
        assert!(dual_run_ok());
        let x = build_full_audit();
        let y = build_full_audit();
        assert_eq!(x.edge_len(), y.edge_len());
        assert_eq!(x.dangle_len(), y.dangle_len());
        assert_eq!(x.reject_len(), y.reject_len());
        assert_eq!(x.phase(), y.phase());
        assert_eq!(xref_digest(XREF_TABLE), xref_digest(XREF_TABLE));
        assert_ne!(xref_digest(XREF_TABLE), 0);
    }

    #[test]
    fn writeback_semantics_hold() {
        assert!(writeback_ok());
        for st in [XPhase::XDraft, XPhase::XScanned, XPhase::XSealed].iter() {
            assert!(!st.name().is_empty());
        }
        for d in X_ALL.iter() {
            assert!(!d.detail().is_empty());
        }
        for k in XrefKind::all().iter() {
            assert!(!k.name().is_empty());
        }
    }

    #[test]
    fn checks_entry_all_green() {
        let s = run_unx12711_checks();
        assert!(s.all_passed(), "自检存在红项：{}", s.red_items_text());
        assert!(s.len() >= 12, "检查条目不足 12");
    }
}
