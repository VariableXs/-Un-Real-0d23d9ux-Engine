//! UNX-F0125 · 早期 panic 呈现（三要素）· 诊断段 0x2430
//! 验收锚点：UNX-F0125-J1 panic 呈现三要素（位置/原因/现场指纹）
//! 全备方可渲染，缺一即探针红。零浮点、零 unwrap、零 expect。


use crate::alloc::{format, string::{String, ToString}, vec::Vec};

pub const SEG_BASE: u16 = 0x2430;

// ── 诊断码闭集 ───────────────────────────────────────────────
const E_NOLOC: u16 = SEG_BASE + 1; // 位置缺失
const E_NOWHY: u16 = SEG_BASE + 2; // 原因缺失
const E_NOFP: u16 = SEG_BASE + 3;  // 现场指纹缺失（零值）
const E_TWICE: u16 = SEG_BASE + 4; // 重复呈现（一 panic 一呈现）
const E_LEN: u16 = SEG_BASE + 5;   // 文本超长（位置 24 / 原因 40）
const E_BAL: u16 = SEG_BASE + 6;   // 呈现账不平

pub fn detail(code: u16) -> &'static str {
    match code {
        E_NOLOC => "位置缺失：panic 位置（文件:行）为空",
        E_NOWHY => "原因缺失：panic 原因为空",
        E_NOFP => "指纹缺失：现场指纹为零值",
        E_TWICE => "重复呈现：一次 panic 一次呈现",
        E_LEN => "文本超长：位置 24 / 原因 40 字节上限",
        E_BAL => "呈现账不平：要素数与记账不符",
        _ => "未知诊断码",
    }
}

pub const LOC_CAP: usize = 24;
pub const WHY_CAP: usize = 40;

/// FNV-1a 32 位现场指纹。
pub fn fnv1a32(data: &[u8]) -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    for b in data {
        h ^= *b as u32;
        h = h.wrapping_mul(0x0100_0193);
    }
    h
}

/// 现场寄存器快照（指纹原料）。
#[derive(Clone)]
pub struct RegsSnapshot {
    pub regs: [u64; 8], // 八通用寄存器
    pub rip: u64,
    pub rflags: u64,
    pub cr2: u64,
}

impl RegsSnapshot {
    /// 现场指纹：全量字节折叠；全零快照得零指纹（判据红线）。
    pub fn fingerprint(&self) -> u32 {
        let mut buf: Vec<u8> = Vec::new();
        for r in &self.regs {
            buf.extend_from_slice(&r.to_le_bytes());
        }
        buf.extend_from_slice(&self.rip.to_le_bytes());
        buf.extend_from_slice(&self.rflags.to_le_bytes());
        buf.extend_from_slice(&self.cr2.to_le_bytes());
        fnv1a32(&buf)
    }

    pub fn is_all_zero(&self) -> bool {
        self.regs.iter().all(|r| *r == 0)
            && self.rip == 0
            && self.rflags == 0
            && self.cr2 == 0
    }
}

/// 呈现账：三要素齐备 → 单行 panic 幕。
pub struct PanicPresent {
    pub loc: Option<String>,
    pub why: Option<String>,
    pub fp: Option<u32>,
    pub rendered: bool,
}

impl PanicPresent {
    pub fn new() -> PanicPresent {
        PanicPresent {
            loc: None,
            why: None,
            fp: None,
            rendered: false,
        }
    }

    pub fn set_loc(&mut self, loc: &str) -> Result<(), u16> {
        if loc.is_empty() || loc.len() > LOC_CAP {
            return Err(E_NOLOC);
        }
        self.loc = Some(loc.to_string());
        Ok(())
    }

    pub fn set_why(&mut self, why: &str) -> Result<(), u16> {
        if why.is_empty() || why.len() > WHY_CAP {
            return Err(E_NOWHY);
        }
        self.why = Some(why.to_string());
        Ok(())
    }

    pub fn set_regs(&mut self, regs: &RegsSnapshot) -> Result<u32, u16> {
        if regs.is_all_zero() {
            return Err(E_NOFP);
        }
        let fp = regs.fingerprint();
        self.fp = Some(fp);
        Ok(fp)
    }

    /// 三要素齐备性核：缺哪项报哪码。
    pub fn completeness(&self) -> Result<(), u16> {
        if self.loc.is_none() {
            return Err(E_NOLOC);
        }
        if self.why.is_none() {
            return Err(E_NOWHY);
        }
        if self.fp.is_none() || self.fp == Some(0) {
            return Err(E_NOFP);
        }
        Ok(())
    }

    /// 呈现：三要素全备才出一行幕；重复呈现拒绝。
    pub fn present(&mut self) -> Result<String, u16> {
        if self.rendered {
            return Err(E_TWICE);
        }
        self.completeness()?;
        let s = format!(
            "PANIC @ {}: {}（现场 {:08X}）",
            self.loc.as_ref().unwrap_or(&String::new()),
            self.why.as_ref().unwrap_or(&String::new()),
            self.fp.unwrap_or(0)
        );
        self.rendered = true;
        Ok(s)
    }

    /// 呈现账审计：呈现后三要素账必须自洽。
    pub fn audit(&self, rendered: bool) -> Result<(), u16> {
        let complete = self.loc.is_some() && self.why.is_some() && self.fp.is_some();
        if rendered && !complete {
            return Err(E_BAL);
        }
        if !rendered && self.rendered {
            return Err(E_BAL);
        }
        Ok(())
    }
}

// ── 确定性演练：合法 3 例 + 注入拦截 6 例 ────────────────────────
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;

    // R1：三要素齐备 → 呈现一行幕，含位置/原因/指纹。
    let mut p = PanicPresent::new();
    p.set_loc("paging.rs:88")?;
    p.set_why("page fault in ktext")?;
    let regs = RegsSnapshot {
        regs: [1, 2, 3, 4, 5, 6, 7, 8],
        rip: 0xFFFF_8000_0010_0040,
        rflags: 0x202,
        cr2: 0xDEAD_BEEF,
    };
    let fp = p.set_regs(&regs)?;
    if fp == 0 {
        return Err(E_NOFP);
    }
    let line = p.present()?;
    if !line.contains("paging.rs:88") || !line.contains("page fault") {
        return Err(E_BAL);
    }
    p.audit(true)?;
    hits += 1;

    // R2：指纹确定性——同快照同指纹，异快照异指纹。
    let f1 = regs.fingerprint();
    let f2 = regs.fingerprint();
    if f1 != f2 {
        return Err(E_NOFP);
    }
    let mut regs2 = regs.clone();
    regs2.rip += 1;
    if regs2.fingerprint() == f1 {
        return Err(E_NOFP);
    }
    hits += 1;

    // R3：缺一即红——三路逐项补齐过程账。
    let mut p3 = PanicPresent::new();
    if p3.completeness() != Err(E_NOLOC) {
        return Err(E_NOLOC);
    }
    p3.set_loc("a.rs:1")?;
    if p3.completeness() != Err(E_NOWHY) {
        return Err(E_NOWHY);
    }
    p3.set_why("boom")?;
    if p3.completeness() != Err(E_NOFP) {
        return Err(E_NOFP);
    }
    p3.set_regs(&regs)?;
    p3.completeness()?;
    p3.present()?;
    hits += 1;

    // I1：位置缺失/越纲（空串 / 超 24 字节）→ E_NOLOC。
    let mut i1 = PanicPresent::new();
    if i1.set_loc("") != Err(E_NOLOC) {
        return Err(E_NOLOC);
    }
    let long_loc = "x".repeat(LOC_CAP + 1);
    if i1.set_loc(&long_loc) != Err(E_NOLOC) {
        return Err(E_NOLOC);
    }
    hits += 1;

    // I2：原因缺失/越纲 → E_NOWHY。
    if i1.set_why("") != Err(E_NOWHY) {
        return Err(E_NOWHY);
    }
    let long_why = "y".repeat(WHY_CAP + 1);
    if i1.set_why(&long_why) != Err(E_NOWHY) {
        return Err(E_NOWHY);
    }
    hits += 1;

    // I3：全零快照 → E_NOFP。
    let zero = RegsSnapshot {
        regs: [0; 8],
        rip: 0,
        rflags: 0,
        cr2: 0,
    };
    if i1.set_regs(&zero) != Err(E_NOFP) {
        return Err(E_NOFP);
    }
    hits += 1;

    // I4：重复呈现 → E_TWICE。
    if p.present() != Err(E_TWICE) {
        return Err(E_TWICE);
    }
    hits += 1;

    // I5：呈现账破（未呈现但声称已渲染 / 已渲染但要素缺）→ E_BAL。
    let p5 = PanicPresent::new();
    if p5.audit(true) != Err(E_BAL) {
        return Err(E_BAL);
    }
    let mut p5b = PanicPresent::new();
    p5b.set_loc("b.rs:2")?;
    p5b.set_why("w")?;
    p5b.set_regs(&regs)?;
    p5b.rendered = true; // 人为标记已渲染
    if p5b.audit(false) != Err(E_BAL) {
        return Err(E_BAL);
    }
    hits += 1;

    Ok(hits)
}
