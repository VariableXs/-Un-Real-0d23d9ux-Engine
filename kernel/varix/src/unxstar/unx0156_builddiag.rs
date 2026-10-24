//! UNX-F0156 · 构建失败诊断（三要素错误）· 诊断段 0x2650
//!
//! 判据原文（UNX-F0156-J1）：构建失败必须齐备三要素——阶段定位
//! （哪一步）、原因归类（闭集）、可执行建议（动作串）；缺任一即
//! 诊断不合规；诊断账守恒（Σ诊断 = Σ失败）。
//!
//! 设计要点：诊断单（DiagTicket）三字段全非空才算合规（合规闸门）；
//! 原因闭集七类；建议动作串闭集映射（原因 → 建议）。零浮点、
//! 零 unwrap/expect。

use crate::alloc::{format, string::{String, ToString}, vec::Vec};

pub const SEG_BASE: u16 = 0x2660;

const E_STAGE: u16 = SEG_BASE + 1; // 阶段定位越纲
const E_CAUSE: u16 = SEG_BASE + 2; // 原因越纲
const E_ADVICE: u16 = SEG_BASE + 3; // 建议为空（不合规）
const E_BAL: u16 = SEG_BASE + 4; // 诊断账守恒被破坏
const E_SEAL: u16 = SEG_BASE + 5; // 诊断册已封

/// 构建阶段闭集（与 unx0145 四阶段对齐 + 预检/后验）。
pub const DIAG_STAGES: [&str; 6] = [
    "preflight",
    "compile",
    "link",
    "pack",
    "verify",
    "postcheck",
];

/// 原因闭集七类。
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Cause {
    MissingInput,
    SyntaxError,
    SymbolUnresolved,
    SizeOverLimit,
    ChecksumMismatch,
    ToolMissing,
    IoDenied,
}

impl Cause {
    fn slot(&self) -> usize {
        match self {
            Cause::MissingInput => 0,
            Cause::SyntaxError => 1,
            Cause::SymbolUnresolved => 2,
            Cause::SizeOverLimit => 3,
            Cause::ChecksumMismatch => 4,
            Cause::ToolMissing => 5,
            Cause::IoDenied => 6,
        }
    }

    fn name(&self) -> &'static str {
        match self {
            Cause::MissingInput => "缺输入",
            Cause::SyntaxError => "语法错",
            Cause::SymbolUnresolved => "符号未解",
            Cause::SizeOverLimit => "体积超限",
            Cause::ChecksumMismatch => "校验和差",
            Cause::ToolMissing => "工具缺失",
            Cause::IoDenied => "IO 受拒",
        }
    }

    /// 原因 → 可执行建议（闭集映射，永不空）。
    fn advice(&self) -> &'static str {
        match self {
            Cause::MissingInput => "补齐缺失输入文件后重跑",
            Cause::SyntaxError => "按行号修复源码语法后重跑",
            Cause::SymbolUnresolved => "补链接目标或裁剪未实现引用",
            Cause::SizeOverLimit => "缩小镜像或上调拆分上限",
            Cause::ChecksumMismatch => "重算校验和并同步摘要文件",
            Cause::ToolMissing => "安装缺失工具链后重跑",
            Cause::IoDenied => "检查介质写保护与权限后重试",
        }
    }
}

/// 诊断单（三要素）。
#[derive(Clone)]
pub struct DiagTicket {
    pub stage: usize, // DIAG_STAGES 下标
    pub cause: Cause,
    pub advice: String,
}

impl DiagTicket {
    /// 合规闸门：阶段定位在闭集 + 建议非空（原因由类型闭集自保证）。
    pub fn well_formed(&self) -> Result<(), u16> {
        if self.stage >= DIAG_STAGES.len() {
            return Err(E_STAGE);
        }
        if self.advice.is_empty() {
            return Err(E_ADVICE);
        }
        Ok(())
    }
}

/// 诊断册。
pub struct DiagBook {
    pub tickets: Vec<DiagTicket>,
    pub failures: u32,
    pub sealed: bool,
}

impl DiagBook {
    pub fn new() -> Result<Self, u16> {
        Ok(DiagBook { tickets: Vec::new(), failures: 0, sealed: false })
    }

    /// 失败登记：自动生成合规诊断单（建议按原因闭集映射）。
    pub fn record_fail(&mut self, stage: usize, cause: Cause) -> Result<(), u16> {
        if self.sealed {
            return Err(E_SEAL);
        }
        if stage >= DIAG_STAGES.len() {
            return Err(E_STAGE);
        }
        let advice = cause.advice().to_string();
        if advice.is_empty() {
            return Err(E_ADVICE); // 映射闭集兜底（恒不触发）
        }
        let t = DiagTicket { stage, cause, advice };
        t.well_formed()?;
        self.tickets.push(t);
        self.failures += 1;
        Ok(())
    }

    /// 诊断账守恒：Σ单 == Σ失败；每单合规。
    pub fn audit(&self) -> Result<(), u16> {
        if self.tickets.len() as u32 != self.failures {
            return Err(E_BAL);
        }
        for t in &self.tickets {
            t.well_formed()?;
        }
        Ok(())
    }

    pub fn seal(&mut self) {
        self.sealed = true;
    }

    /// 渲染诊断单。
    pub fn render(&self) -> String {
        let mut out = String::new();
        out.push_str(&format!("诊断册 失败{}\n", self.failures));
        for t in &self.tickets {
            out.push_str(&format!(
                "[{}] {} → {}\n",
                DIAG_STAGES[t.stage],
                t.cause.name(),
                t.advice
            ));
        }
        out
    }
}

/// 手工单合规校验入口（外部传入的三要素单也必须过闸）。
pub fn check_ticket(stage: usize, cause: Cause, advice: &str) -> Result<(), u16> {
    if stage >= DIAG_STAGES.len() {
        return Err(E_STAGE);
    }
    if advice.is_empty() {
        return Err(E_ADVICE);
    }
    let _ = cause;
    Ok(())
}

// ── 确定性演练：合法 4 例 + 注入拦截 4 例 ─────────────────────────

/// 演练入口（ktest 挂点）。
pub fn drill() -> Result<(), u16> {
    let mut hits = 0u32;

    // L1：四阶段各败一因 → 四单全合规 → 账守恒。
    let mut b = DiagBook::new()?;
    b.record_fail(0, Cause::MissingInput)?;
    b.record_fail(1, Cause::SyntaxError)?;
    b.record_fail(3, Cause::SizeOverLimit)?;
    b.record_fail(5, Cause::IoDenied)?;
    b.audit()?;
    hits += 1;

    // L2：建议串闭集映射——每种原因建议非空且互不混淆（抽查三）。
    let b2 = DiagBook::new()?;
    let _ = b2;
    let a1 = Cause::ChecksumMismatch.advice();
    let a2 = Cause::ToolMissing.advice();
    let a3 = Cause::SymbolUnresolved.advice();
    if a1.is_empty() || a2.is_empty() || a3.is_empty() || a1 == a2 || a2 == a3 {
        return Err(E_BAL);
    }
    hits += 1;

    // L3：渲染含阶段名与建议。
    let rep = b.render();
    if !rep.contains("[compile]") || !rep.contains("修复源码语法") {
        return Err(E_BAL);
    }
    hits += 1;

    // L4：手工单合规通过。
    check_ticket(2, Cause::SymbolUnresolved, "补链接目标")?;
    hits += 1;

    // I1：阶段越纲登记/手工双拒。
    let mut i1 = DiagBook::new()?;
    if i1.record_fail(9, Cause::MissingInput) != Err(E_STAGE) {
        return Err(E_STAGE);
    }
    if check_ticket(9, Cause::MissingInput, "x") != Err(E_STAGE) {
        return Err(E_STAGE);
    }
    hits += 1;

    // I2：空建议手工单拒（E_ADVICE）。
    if check_ticket(0, Cause::MissingInput, "") != Err(E_ADVICE) {
        return Err(E_ADVICE);
    }
    hits += 1;

    // I3：合规闸门——手造越纲单入册也拒（well_formed 兜底路径）。
    let bad = DiagTicket {
        stage: 42,
        cause: Cause::SyntaxError,
        advice: "x".to_string(),
    };
    if bad.well_formed() != Err(E_STAGE) {
        return Err(E_STAGE);
    }
    hits += 1;

    // I4：封册拒记。
    let mut i4 = DiagBook::new()?;
    i4.seal();
    if i4.record_fail(0, Cause::ToolMissing) != Err(E_SEAL) {
        return Err(E_SEAL);
    }
    hits += 1;

    // I5：账注入——单被抽走而失败数不动 → audit 必红。
    let mut i5 = DiagBook::new()?;
    i5.record_fail(1, Cause::SyntaxError)?;
    i5.tickets.clear(); // 破账
    if i5.audit() != Err(E_BAL) {
        return Err(E_BAL);
    }
    hits += 1;

    let _ = hits;
    Ok(())
}
