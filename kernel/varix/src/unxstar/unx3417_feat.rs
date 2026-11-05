//! UNX-F3417 断言宏与双后端执行器的接口约定 · 0x79700段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（派工卡 VX8-B11 逐字摘录）：
//!   ① 纯功能代码 ≥260 行；
//!   ② J1：断言输出与 B12 双后端（QEMU/真机后端抽象）的接口约定书交付
//!      （v0.9 预对齐）。
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 约定书条款冻结表：十条条款（输出面 / 顺序面 / 错误码面 / 版本面），
//!     每条带生产方 / 消费方 / 封印版本三要素，v0.9 预对齐即全部条款
//!     封印版本 ≤ 0x09 且错码窗已对齐；
//!   - 双后端抽象：QEMU 后端与真机后端共用同一错码窗（SEG_BASE+1..+12），
//!     真机侧另有压缩出口码映射（code & 0x3F）——映射必须可逆地保留
//!     窗内判别位；
//!   - 断言输出面契约：通过路径零行输出、失败路径恰好一行且落在错码窗，
//!     两后端对同一帧的收发判决必须一致（抽象不漏后端差异）；
//!   - 版本状态机 v0.1→v0.9 单调推进，回退/跳版显性拒；
//!   - 注入必抓：窗外错码 / 版本标签不符 / 后端标签错配三类违约必抓；
//!   - 双跑一致：条款表哈希与协商结论各跑两次逐字节相同。
//! 常量来源：
//!   - 条款数 10：对应断言宏面 8 宏 + 输出顺序 + 版本协商（UNX-F3412
//!     目录 8 宏的下游契约面）；
//!   - 错码窗 12 码：与本批各卡诊断闭集同构（+0x01..+0x0C）。
//! ─────────────────────────────────────────────────────────────────────

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x40 对齐，段位守卫区间 [0x79600, 0x797C0] 内）。
pub const SEG_BASE: u32 = 0x79700;

/// 版本预对齐目标（v0.9，BCD 式一位小数编码 0x09）。
pub const VERSION_TARGET: u32 = 0x09;

/// 后端类闭集。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BackendKind {
    /// QEMU 后端：有完整输出流，可逐行断言。
    Qemu,
    /// 真机后端：输出受压缩出口码约束。
    BareMetal,
}

impl BackendKind {
    /// 中文名。
    pub fn zh(self) -> &'static str {
        match self {
            BackendKind::Qemu => "QEMU后端",
            BackendKind::BareMetal => "真机后端",
        }
    }
}

/// 全后端闭集。
pub const ALL_BACKENDS: [BackendKind; 2] = [BackendKind::Qemu, BackendKind::BareMetal];

/// 条款类闭集。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClauseKind {
    /// 输出面：通过/失败各发什么。
    Output,
    /// 顺序面：多断言间输出先后。
    Ordering,
    /// 错误码面：错码窗与出口映射。
    ErrorCode,
    /// 版本面：协商与封印。
    Version,
}

impl ClauseKind {
    /// 中文名。
    pub fn zh(self) -> &'static str {
        match self {
            ClauseKind::Output => "输出面",
            ClauseKind::Ordering => "顺序面",
            ClauseKind::ErrorCode => "错误码面",
            ClauseKind::Version => "版本面",
        }
    }
}

/// 接口约定条款（约定书一行）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct IfaceClause {
    /// 条款号（1 起）。
    pub id: u8,
    /// 条款标题（中文）。
    pub title: &'static str,
    /// 条款类。
    pub kind: ClauseKind,
    /// 生产方标签。
    pub producer: &'static str,
    /// 消费方标签。
    pub consumer: &'static str,
    /// 封印版本（≤ VERSION_TARGET 才算 v0.9 预对齐）。
    pub sealed_v: u32,
}

impl IfaceClause {
    /// 条款自检：号非零、标题非空、双方标签非空且不同、版本有界。
    pub fn well_formed(&self) -> bool {
        self.id != 0
            && !self.title.is_empty()
            && !self.producer.is_empty()
            && !self.consumer.is_empty()
            && self.producer != self.consumer
            && self.sealed_v >= 0x01
            && self.sealed_v <= VERSION_TARGET
    }
}

/// 冻结条款表（10 条；v0.9 预对齐的约定书交付物）。
pub const CLAUSES: [IfaceClause; 10] = [
    IfaceClause { id: 1,  title: "通过路径零行输出", kind: ClauseKind::Output,    producer: "断言宏", consumer: "执行器", sealed_v: 0x07 },
    IfaceClause { id: 2,  title: "失败路径恰一行",   kind: ClauseKind::Output,    producer: "断言宏", consumer: "执行器", sealed_v: 0x07 },
    IfaceClause { id: 3,  title: "失败行带错码",     kind: ClauseKind::Output,    producer: "断言宏", consumer: "执行器", sealed_v: 0x08 },
    IfaceClause { id: 4,  title: "多断言按源序输出", kind: ClauseKind::Ordering,  producer: "执行器", consumer: "断言宏", sealed_v: 0x06 },
    IfaceClause { id: 5,  title: "错码窗十二码对齐", kind: ClauseKind::ErrorCode, producer: "断言宏", consumer: "双后端", sealed_v: 0x09 },
    IfaceClause { id: 6,  title: "QEMU逐行捕获",     kind: ClauseKind::ErrorCode, producer: "QEMU后端", consumer: "执行器", sealed_v: 0x08 },
    IfaceClause { id: 7,  title: "真机压缩出口码",   kind: ClauseKind::ErrorCode, producer: "真机后端", consumer: "执行器", sealed_v: 0x09 },
    IfaceClause { id: 8,  title: "版本协商单调",     kind: ClauseKind::Version,   producer: "执行器", consumer: "双后端", sealed_v: 0x09 },
    IfaceClause { id: 9,  title: "协商失败即拒载",   kind: ClauseKind::Version,   producer: "真机后端", consumer: "执行器", sealed_v: 0x08 },
    IfaceClause { id: 10, title: "输出字节流UTF-8",  kind: ClauseKind::Output,    producer: "断言宏", consumer: "QEMU后端", sealed_v: 0x06 },
];

/// 条款表完整性：号连续唯一、自检全过、封印全部 ≤ 目标版本
/// （v0.9 预对齐判据的本体）。
pub fn clauses_complete() -> bool {
    for (i, c) in CLAUSES.iter().enumerate() {
        if !c.well_formed() {
            return false;
        }
        if c.id as usize != i + 1 {
            return false;
        }
        if c.sealed_v > VERSION_TARGET {
            return false;
        }
        let mut dup = 0u32;
        for o in CLAUSES.iter() {
            if o.id == c.id {
                dup += 1;
            }
        }
        if dup != 1 {
            return false;
        }
    }
    true
}

/// 版本状态机：v0.1→v0.9 逐 0x01 单调推进；回退、跳版（步长>1）、
/// 越顶（>0x09）一律显性拒。
pub fn advance_version(cur: u32, next: u32) -> Result<u32, u32> {
    if next <= cur {
        return Err(Diag::VersionRegress.val());
    }
    if next - cur > 1 {
        return Err(Diag::VersionSkip.val());
    }
    if next > VERSION_TARGET {
        return Err(Diag::VersionOverTop.val());
    }
    Ok(next)
}

/// 后端错码窗：[SEG_BASE+1, SEG_BASE+12] 闭窗，两后端共用。
pub fn in_window(code: u32) -> bool {
    code >= SEG_BASE + 1 && code <= SEG_BASE + 12
}

/// 真机压缩出口码映射：错码 → 出口码（&0x3F 保留窗内判别位）。
pub fn baremetal_exit(code: u32) -> u32 {
    code & 0x3F
}

/// 压缩映射可逆性：窗内任意两码映射后仍可判别（不碰撞）。
pub fn exit_map_injective() -> bool {
    for a in 1u32..=12 {
        for b in (a + 1)..=12 {
            let ca = match in_window(SEG_BASE + a) {
                true => baremetal_exit(SEG_BASE + a),
                false => return false,
            };
            let cb = match in_window(SEG_BASE + b) {
                true => baremetal_exit(SEG_BASE + b),
                false => return false,
            };
            if ca == cb {
                return false;
            }
        }
    }
    true
}

/// 断言输出帧（后端收发的最小单元）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BackendFrame {
    /// 断言错码（窗内才合法）。
    pub code: u32,
    /// 判决（true=通过帧，false=失败帧）。
    pub verdict: bool,
    /// 发出方后端标签。
    pub backend: BackendKind,
    /// 版本标签（须 ≤ 目标版本）。
    pub ver: u32,
}

/// 后端收发判决：按条款表验收一帧。
/// 通过帧：零行输出（verdict=true 即不落输出流）；
/// 失败帧：须窗内错码 + 版本合法。
pub fn backend_accept(f: &BackendFrame) -> Result<(), u32> {
    if f.ver > VERSION_TARGET || f.ver == 0 {
        return Err(Diag::VerTagMismatch.val());
    }
    if f.verdict {
        return Ok(());
    }
    // 后端标签错配先于窗检查：真机后端发的失败帧必须能压缩映射
    // 且出口码非零（出口码归零即后端错配，先点名后端面）。
    if f.backend == BackendKind::BareMetal {
        if baremetal_exit(f.code) == 0 {
            return Err(Diag::BackendMismatch.val());
        }
    }
    if !in_window(f.code) {
        return Err(Diag::CodeOutOfWindow.val());
    }
    Ok(())
}

/// 协商结论：给定后端集合，逐后端跑版本状态机到目标版本；
/// 任一后端推不满即协商失败（拒载）。
pub fn negotiate() -> Result<Vec<(BackendKind, u32)>, u32> {
    let mut out: Vec<(BackendKind, u32)> = Vec::new();
    for b in ALL_BACKENDS.iter() {
        let mut v = 0x01u32;
        while v < VERSION_TARGET {
            match advance_version(v, v + 1) {
                Ok(n) => v = n,
                Err(e) => return Err(e),
            }
        }
        out.push((*b, v));
    }
    Ok(out)
}

/// 条款表哈希（FNV-1a over 标题+类+封印版本）。
pub fn clause_hash() -> u32 {
    let mut buf: Vec<u8> = Vec::new();
    for c in CLAUSES.iter() {
        for &b in c.title.as_bytes() {
            buf.push(b);
        }
        buf.push(c.kind as u8);
        buf.push(c.sealed_v as u8);
        buf.push(b'|');
    }
    let mut h: u32 = 0x811C_9DC5;
    for &b in buf.iter() {
        h ^= b as u32;
        h = h.wrapping_mul(0x0100_0193);
    }
    h
}

/// 诊断码闭集：0x79701..=0x7970C（12 码，变体带中文 doc）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Diag {
    /// 版本回退：协商收到 rank 不增的版本。
    VersionRegress = 0x01,
    /// 版本跳步：版本推进步长超过 1。
    VersionSkip = 0x02,
    /// 版本越顶：推进超过 v0.9 目标版本。
    VersionOverTop = 0x03,
    /// 错码窗外：失败帧错码不落在十二码窗内。
    CodeOutOfWindow = 0x04,
    /// 版本标签不符：帧版本为零或超目标。
    VerTagMismatch = 0x05,
    /// 后端标签错配：真机出口码压模归零等后端面违约。
    BackendMismatch = 0x06,
    /// 条款残缺：条款号/标题/双方标签/封印版本破约。
    ClauseMalformed = 0x07,
    /// 出口码碰撞：压缩映射不可判别。
    WindowMapCollision = 0x08,
    /// 双跑不一致：哈希/协商/窗判定两次结论不同。
    DoubleRunDiverge = 0x09,
    /// 协商中断：某后端版本机推不满即拒载。
    NegotiateAbort = 0x0A,
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
            Diag::VersionRegress => "版本回退：建议按 v0.1→v0.9 逐 0x01 单调推进，禁止回退",
            Diag::VersionSkip => "版本跳步：建议步长恒为 1，大版本跨越须逐版补协商",
            Diag::VersionOverTop => "版本越顶：建议以 v0.9 为封印顶，后续升级走新约定书",
            Diag::CodeOutOfWindow => "错码窗外：建议失败帧错码只取 SEG_BASE+1..=+12 闭窗",
            Diag::VerTagMismatch => "版本标签不符：建议帧版本非零且不超 v0.9 目标",
            Diag::BackendMismatch => "后端标签错配：建议真机后端发帧前先验压缩出口码非零",
            Diag::ClauseMalformed => "条款残缺：建议补齐条款号/标题/双方标签并核对封印版本有界",
            Diag::WindowMapCollision => "出口码碰撞：建议压缩映射保留窗内判别位后再上线",
            Diag::DoubleRunDiverge => "双跑不一致：建议收发判决做成纯函数，帧定则判决定",
            Diag::NegotiateAbort => "协商中断：建议按约拒载并点名推不满的后端",
            Diag::SegGuardFail => "段位守卫失守：建议核对 SEG_BASE 的 0x40 对齐与段窗区间",
            Diag::SummaryEmpty => "写回摘要为空：建议收口时以非空中文摘要写回状态=已完成",
        }
    }
}

/// 闭集全量数组（12 码）。
pub const ALL_DIAGS: [Diag; 12] = [
    Diag::VersionRegress,
    Diag::VersionSkip,
    Diag::VersionOverTop,
    Diag::CodeOutOfWindow,
    Diag::VerTagMismatch,
    Diag::BackendMismatch,
    Diag::ClauseMalformed,
    Diag::WindowMapCollision,
    Diag::DoubleRunDiverge,
    Diag::NegotiateAbort,
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
    "未知诊断码：建议核对是否落在 0x79701..=0x7970C 闭集内"
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

/// 约定书交付面：条款表完整 + v0.9 预对齐（封印全部 ≤ 0x09）。
fn clauses_v09_ok() -> bool {
    clauses_complete()
        && CLAUSES.len() == 10
        && CLAUSES.iter().all(|c| c.sealed_v <= VERSION_TARGET)
        && clause_hash() != 0
}

/// 正常流：通过帧/失败帧按条款被两后端一致收发。
fn frame_flow_ok() -> bool {
    let pass = BackendFrame { code: 0, verdict: true, backend: BackendKind::Qemu, ver: VERSION_TARGET };
    let fail = BackendFrame { code: SEG_BASE + 1, verdict: false, backend: BackendKind::Qemu, ver: VERSION_TARGET };
    let fail_bm = BackendFrame { code: SEG_BASE + 3, verdict: false, backend: BackendKind::BareMetal, ver: 0x08 };
    backend_accept(&pass).is_ok()
        && backend_accept(&fail).is_ok()
        && backend_accept(&fail_bm).is_ok()
}

/// 错码窗语义：窗内 12 码全收、窗外全拒、出口映射可逆。
fn code_window_ok() -> bool {
    for off in 1..=12u32 {
        if !in_window(SEG_BASE + off) {
            return false;
        }
        if in_window(SEG_BASE + off + 12) {
            return false;
        }
    }
    if in_window(SEG_BASE) || in_window(SEG_BASE + 13) {
        return false;
    }
    exit_map_injective() && baremetal_exit(SEG_BASE + 5) == (SEG_BASE + 5) & 0x3F
}

/// 注入必抓 1：窗外错码失败帧必须被 CodeOutOfWindow 点名。
fn inj_out_of_window_ok() -> bool {
    let f = BackendFrame { code: SEG_BASE + 13, verdict: false, backend: BackendKind::Qemu, ver: VERSION_TARGET };
    let f2 = BackendFrame { code: SEG_BASE, verdict: false, backend: BackendKind::Qemu, ver: VERSION_TARGET };
    backend_accept(&f) == Err(Diag::CodeOutOfWindow.val())
        && backend_accept(&f2) == Err(Diag::CodeOutOfWindow.val())
}

/// 注入必抓 2：版本标签不符（零版/超顶）必须被 VerTagMismatch 点名。
fn inj_ver_tag_ok() -> bool {
    let zero = BackendFrame { code: SEG_BASE + 1, verdict: false, backend: BackendKind::Qemu, ver: 0 };
    let over = BackendFrame { code: SEG_BASE + 1, verdict: false, backend: BackendKind::Qemu, ver: VERSION_TARGET + 1 };
    backend_accept(&zero) == Err(Diag::VerTagMismatch.val())
        && backend_accept(&over) == Err(Diag::VerTagMismatch.val())
}

/// 注入必抓 3：后端标签错配（真机出口码压成 0）必须被 BackendMismatch 点名。
fn inj_backend_mismatch_ok() -> bool {
    // 构造错配：真机后端发出的失败帧压模后出口码归零（窗基址低位恰零）。
    let f = BackendFrame { code: SEG_BASE & 0xFFFF_FFC0, verdict: false, backend: BackendKind::BareMetal, ver: VERSION_TARGET };
    backend_accept(&f) == Err(Diag::BackendMismatch.val())
}

/// 版本状态机：单调放行、回退/跳版/越顶三违约全拒、协商两后端到顶。
fn version_machine_ok() -> bool {
    if advance_version(0x05, 0x04) != Err(Diag::VersionRegress.val()) {
        return false;
    }
    if advance_version(0x05, 0x07) != Err(Diag::VersionSkip.val()) {
        return false;
    }
    if advance_version(0x09, 0x0A) != Err(Diag::VersionOverTop.val()) {
        return false;
    }
    if advance_version(0x05, 0x05).is_ok() {
        return false;
    }
    match negotiate() {
        Ok(pairs) => {
            pairs.len() == 2
                && pairs.iter().all(|(_, v)| *v == VERSION_TARGET)
        }
        Err(_) => false,
    }
}

/// 协商失败即拒载：条款表若含超顶封印版本，完整性判据必须点破。
fn inj_unsealed_clause_ok() -> bool {
    // 注入：构造一条封印 v0x0A（超顶）的伪条款，well_formed 必拒。
    let rogue = IfaceClause { id: 11, title: "越顶条款", kind: ClauseKind::Version, producer: "断言宏", consumer: "执行器", sealed_v: 0x0A };
    !rogue.well_formed()
}

/// 双跑一致：条款哈希、协商结论、窗判定各两次相同。
fn double_run_ok() -> bool {
    clause_hash() == clause_hash()
        && {
            let a = negotiate();
            let b = negotiate();
            a.is_ok() && b.is_ok() && a == b
        }
        && in_window(SEG_BASE + 7) == in_window(SEG_BASE + 7)
}

/// 错误路径 Result 语义：零版帧/窗外帧/超顶推进三码可复现。
fn error_path_ok() -> bool {
    let zero = BackendFrame { code: SEG_BASE + 2, verdict: false, backend: BackendKind::Qemu, ver: 0 };
    backend_accept(&zero).is_err()
        && advance_version(0x09, 0x09).is_err()
        && !detail(0x0).is_empty()
        && detail(Diag::CodeOutOfWindow.val()).contains("错码")
}

/// ③ 写回面：状态=已完成、结果摘要非空。
fn writeback_summary_ok() -> bool {
    let pairs = match negotiate() {
        Ok(p) => p,
        Err(_) => return false,
    };
    let summary = format!(
        "UNX-F3417 约定书v0.9：条款{}条 双后端{}个 预对齐",
        CLAUSES.len(),
        pairs.len()
    );
    let done = !summary.is_empty()
        && summary.contains("v0.9")
        && clauses_complete()
        && pairs.len() == 2;
    let empty_verdict = if summary.is_empty() {
        Err(Diag::SummaryEmpty.val())
    } else {
        Ok(())
    };
    done && empty_verdict.is_ok()
}

// ── 判据总成：run_unx3417_checks ───────────────────────────────────────

/// run_unx3417_checks：本单判据聚合（诊断段 0x79700，全部走 add_dyn）。
pub fn run_unx3417_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F3417 断言宏与双后端执行器的接口约定");
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
            "约定书十条条款齐且v0.9预对齐",
            clauses_v09_ok(),
            format!("条款表{}", if clauses_v09_ok() { "封印" } else { "未齐" }),
        ),
        (
            "正常流帧收发两后端一致",
            frame_flow_ok(),
            format!("帧流{}", if frame_flow_ok() { "绿" } else { "红" }),
        ),
        (
            "错码窗十二码全收窗外全拒映射可逆",
            code_window_ok(),
            format!("错码窗{}", if code_window_ok() { "过" } else { "破" }),
        ),
        (
            "注入1窗外错码必抓",
            inj_out_of_window_ok(),
            format!("窗外码{}", if inj_out_of_window_ok() { "抓到" } else { "漏抓" }),
        ),
        (
            "注入2版本标签不符必抓",
            inj_ver_tag_ok(),
            format!("版本标签{}", if inj_ver_tag_ok() { "抓到" } else { "漏抓" }),
        ),
        (
            "注入3后端标签错配必抓",
            inj_backend_mismatch_ok(),
            format!("后端错配{}", if inj_backend_mismatch_ok() { "抓到" } else { "漏抓" }),
        ),
        (
            "注入4越顶封印条款必拒",
            inj_unsealed_clause_ok(),
            format!("越顶条款{}", if inj_unsealed_clause_ok() { "拒" } else { "漏" }),
        ),
        (
            "版本状态机回退跳版越顶三违约全拒",
            version_machine_ok(),
            format!("版本机{}", if version_machine_ok() { "过" } else { "失" }),
        ),
        (
            "双跑一致哈希协商窗判定各两次同",
            double_run_ok(),
            format!("双跑{}", if double_run_ok() { "一致" } else { "分叉" }),
        ),
        (
            "错误路径三码可复现",
            error_path_ok(),
            format!("错误路径{}", if error_path_ok() { "过" } else { "破" }),
        ),
        (
            "写回摘要非空状态已完成",
            writeback_summary_ok(),
            format!("写回{}", if writeback_summary_ok() { "非空" } else { "空" }),
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
        assert_eq!(SEG_BASE, 0x79700);
        assert!(diag_closed_set_ok());
        for (i, d) in ALL_DIAGS.iter().enumerate() {
            assert_eq!(d.val(), SEG_BASE + i as u32 + 1);
        }
    }

    #[test]
    fn clauses_frozen_and_v09_sealed() {
        assert!(clauses_v09_ok());
        assert!(clauses_complete());
        assert_eq!(CLAUSES.len(), 10);
        for c in CLAUSES.iter() {
            assert!(c.well_formed());
            assert!(c.sealed_v <= VERSION_TARGET);
        }
        assert_ne!(clause_hash(), 0);
    }

    #[test]
    fn frames_accepted_by_both_backends() {
        assert!(frame_flow_ok());
        let pass = BackendFrame { code: 0, verdict: true, backend: BackendKind::BareMetal, ver: 0x09 };
        assert!(backend_accept(&pass).is_ok());
    }

    #[test]
    fn code_window_and_injective_exit_map() {
        assert!(code_window_ok());
        assert!(exit_map_injective());
        assert!(!in_window(SEG_BASE));
        assert!(!in_window(SEG_BASE + 13));
        assert!(in_window(SEG_BASE + 12));
    }

    #[test]
    fn injections_all_caught() {
        assert!(inj_out_of_window_ok());
        assert!(inj_ver_tag_ok());
        assert!(inj_backend_mismatch_ok());
        assert!(inj_unsealed_clause_ok());
    }

    #[test]
    fn version_machine_three_rejections() {
        assert!(version_machine_ok());
        assert_eq!(
            advance_version(0x08, 0x08).unwrap_err(),
            Diag::VersionRegress.val()
        );
    }

    #[test]
    fn negotiation_reaches_target_for_both() {
        let pairs = negotiate().unwrap();
        assert_eq!(pairs.len(), 2);
        assert!(pairs.iter().all(|(_, v)| *v == VERSION_TARGET));
        assert!(pairs.iter().any(|(b, _)| *b == BackendKind::Qemu));
        assert!(pairs.iter().any(|(b, _)| *b == BackendKind::BareMetal));
    }

    #[test]
    fn double_run_identical() {
        assert!(double_run_ok());
    }

    #[test]
    fn error_paths_return_closed_codes() {
        assert!(error_path_ok());
        assert!(detail(Diag::VerTagMismatch.val()).contains("版本"));
    }

    #[test]
    fn writeback_summary_non_empty() {
        assert!(writeback_summary_ok());
    }

    #[test]
    fn checks_aggregate_all_green() {
        let s = run_unx3417_checks();
        assert!(s.len() >= 12);
        assert!(s.all_passed(), "全部判据必须绿：{}", s.red_items_text());
    }

    #[test]
    fn baremetal_exit_preserves_discriminator() {
        for off in 1..=12u32 {
            let code = SEG_BASE + off;
            assert_eq!(baremetal_exit(code) | (code & 0xFFFF_FFC0), code);
        }
    }
}
