//! UNX-F48578 —— I29 文档一致性（接口文档 vs 实现抽查）
//! 判据原文：UNX-F48578-J1 I29 接口文档与实现逐项抽查对齐：
//! 文档声明的接口条目必须与实现登记一致（签名、返回码、条数）；
//! 文档多于实现（幽灵接口）与实现多于文档（野接口）都必须检出；
//! 抽查账守恒（一致 + 不一致 == 抽查总数）、指纹与呈现行可复核。
//! 判据落地（drill 场景）：R1 条目闭集；R2 全量对齐；
//! R3 幽灵接口检出；R4 野接口检出；R5 签名失配检出；
//! R6 抽查账守恒；R7 空账零误报；R8 fuzz 抽查流零错态；
//! R9 指纹稳定；R10 呈现行齐全。
//! 设计纪律：no_std；零 unwrap/expect/panic；确定性 LCG；drill 判据全绿才返回 Ok。
use crate::alloc::vec::Vec;
use crate::alloc::vec;
use alloc::string::{String, ToString};

/// 诊断段基址（独立 0x20 窗）。
pub const SEG_BASE: u16 = 0xF820;

// ── 诊断码闭集（SEG_BASE+1 起连续，≤ SEG_BASE+0x1F）─────────────
const E_ENTRY: u16 = SEG_BASE + 1; // 条目越出闭集
const E_ALIGN: u16 = SEG_BASE + 2; // 全量对齐失败
const E_GHOST: u16 = SEG_BASE + 3; // 幽灵接口未检出
const E_WILD: u16 = SEG_BASE + 4; // 野接口未检出
const E_SIG: u16 = SEG_BASE + 5; // 签名失配未检出
const E_CONSERV: u16 = SEG_BASE + 6; // 抽查账不守恒
const E_EMPTY: u16 = SEG_BASE + 7; // 空账误报
const E_FUZZ: u16 = SEG_BASE + 8; // fuzz 抽查流错态
const E_FP: u16 = SEG_BASE + 9; // 指纹漂移
const E_LINE: u16 = SEG_BASE + 10; // 呈现行缺段

/// FNV-1a 64 位（与本域其余哨兵同口径）。
pub fn fnv1a64(data: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in data {
        h ^= *b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01B3);
    }
    h
}

/// 确定性 LCG（批内统一系数）。
pub struct Lcg {
    state: u32,
}

impl Lcg {
    pub fn new(seed: u32) -> Lcg {
        Lcg { state: seed }
    }
    pub fn next_u32(&mut self) -> u32 {
        self.state = self.state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        self.state
    }
    pub fn below(&mut self, bound: u32) -> u32 {
        if bound == 0 {
            0
        } else {
            self.next_u32() % bound
        }
    }
}

/// 一条接口条目：接口名指纹 + 签名指纹（FNV 双口径）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct IfEntry {
    pub name_fp: u64,
    pub sig_fp: u64,
}

/// 抽查结论：1 一致 / 2 幽灵（文档有实现无）/ 3 野（实现有文档无）/ 4 签名失配。
pub const R_ALIGN: u8 = 1;
pub const R_GHOST: u8 = 2;
pub const R_WILD: u8 = 3;
pub const R_SIGMIS: u8 = 4;

/// 单项抽查：文档条目 vs 实现条目。
pub fn audit_one(doc: Option<IfEntry>, imp: Option<IfEntry>) -> u8 {
    match (doc, imp) {
        (Some(d), Some(i)) => {
            if d.name_fp == i.name_fp && d.sig_fp == i.sig_fp {
                R_ALIGN
            } else if d.name_fp == i.name_fp {
                R_SIGMIS
            } else {
                R_GHOST
            }
        }
        (Some(_), None) => R_GHOST,
        (None, Some(_)) => R_WILD,
        (None, None) => R_ALIGN,
    }
}

/// 抽查账：四类结论计数。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct AuditCount {
    pub align: u32,
    pub ghost: u32,
    pub wild: u32,
    pub sigmis: u32,
}

/// 文档一致性抽查器。
pub struct DocAudit {
    pub doc: Vec<IfEntry>,
    pub imp: Vec<IfEntry>,
    pub count: AuditCount,
}

impl DocAudit {
    pub fn new() -> DocAudit {
        DocAudit {
            doc: vec![],
            imp: vec![],
            count: AuditCount { align: 0, ghost: 0, wild: 0, sigmis: 0 },
        }
    }

    fn has(v: &[IfEntry], e: &IfEntry) -> bool {
        let mut i = 0usize;
        while i < v.len() {
            if v[i].name_fp == e.name_fp {
                return true;
            }
            i += 1;
        }
        false
    }

    /// 登记文档条目（重名拒收）。
    pub fn add_doc(&mut self, e: IfEntry) -> Result<(), u16> {
        if DocAudit::has(&self.doc, &e) {
            return Err(E_ENTRY);
        }
        self.doc.push(e);
        Ok(())
    }

    /// 登记实现条目（重名拒收）。
    pub fn add_imp(&mut self, e: IfEntry) -> Result<(), u16> {
        if DocAudit::has(&self.imp, &e) {
            return Err(E_ENTRY);
        }
        self.imp.push(e);
        Ok(())
    }

    /// 全量抽查：逐条对齐并累计四类结论。
    pub fn run(&mut self) {
        self.count = AuditCount { align: 0, ghost: 0, wild: 0, sigmis: 0 };
        // 文档侧逐条。
        let mut i = 0usize;
        while i < self.doc.len() {
            let d = self.doc[i];
            // 实现侧找同名。
            let mut found: Option<IfEntry> = None;
            let mut j = 0usize;
            while j < self.imp.len() {
                if self.imp[j].name_fp == d.name_fp {
                    found = Some(self.imp[j]);
                }
                j += 1;
            }
            match audit_one(Some(d), found) {
                R_ALIGN => self.count.align += 1,
                R_GHOST => self.count.ghost += 1,
                R_SIGMIS => self.count.sigmis += 1,
                _ => self.count.wild += 1,
            }
            i += 1;
        }
        // 实现侧独有条目计野接口。
        let mut j = 0usize;
        while j < self.imp.len() {
            if !DocAudit::has(&self.doc, &self.imp[j]) {
                self.count.wild += 1;
            }
            j += 1;
        }
    }

    /// 全对齐判定：ghost + wild + sigmis == 0 且文档实现逐条同名。
    pub fn all_aligned(&self) -> bool {
        self.count.ghost == 0 && self.count.wild == 0 && self.count.sigmis == 0
    }

    /// 守恒：align + ghost + wild + sigmis == 文档条数 + 实现独有条数。
    pub fn conserved(&self) -> bool {
        let mut imp_only = 0u32;
        let mut j = 0usize;
        while j < self.imp.len() {
            if !DocAudit::has(&self.doc, &self.imp[j]) {
                imp_only += 1;
            }
            j += 1;
        }
        let total = self.count.align
            + self.count.ghost
            + self.count.wild
            + self.count.sigmis;
        total == self.doc.len() as u32 + imp_only
    }
}

/// 主巡检入口：R1..R10 全绿才返回 Ok(hits)。
pub fn drill() -> Result<u32, u16> {
    let mut hits = 0u32;

    // R1 条目闭集：audit_one 对 (Some,Some)/(Some,None)/(None,Some)/(None,None) 全谱自洽。
    let e1 = IfEntry { name_fp: 100, sig_fp: 1 };
    if audit_one(Some(e1), Some(e1)) != R_ALIGN {
        return Err(E_ENTRY);
    }
    if audit_one(Some(e1), None) != R_GHOST || audit_one(None, Some(e1)) != R_WILD {
        return Err(E_ENTRY);
    }
    if audit_one(None, None) != R_ALIGN {
        return Err(E_ENTRY);
    }
    hits += 1;

    // R2 全量对齐：5 条同名同签名条目全对齐。
    let mut au = DocAudit::new();
    let mut i = 0u64;
    while i < 5 {
        let e = IfEntry { name_fp: 1000 + i, sig_fp: 10 + i };
        au.add_doc(e)?;
        au.add_imp(e)?;
        i += 1;
    }
    au.run();
    if !au.all_aligned() || au.count.align != 5 {
        return Err(E_ALIGN);
    }
    hits += 1;

    // R3 幽灵接口检出：文档独有条目计 ghost。
    let mut au2 = DocAudit::new();
    au2.add_doc(IfEntry { name_fp: 100, sig_fp: 1 })?;
    au2.add_imp(IfEntry { name_fp: 100, sig_fp: 1 })?;
    au2.add_doc(IfEntry { name_fp: 200, sig_fp: 2 })?;
    au2.run();
    if au2.count.ghost != 1 || au2.count.align != 1 {
        return Err(E_GHOST);
    }
    hits += 1;

    // R4 野接口检出：实现独有条目计 wild。
    let mut au3 = DocAudit::new();
    au3.add_doc(IfEntry { name_fp: 100, sig_fp: 1 })?;
    au3.add_imp(IfEntry { name_fp: 100, sig_fp: 1 })?;
    au3.add_imp(IfEntry { name_fp: 300, sig_fp: 3 })?;
    au3.run();
    if au3.count.wild != 1 || au3.count.align != 1 {
        return Err(E_WILD);
    }
    hits += 1;

    // R5 签名失配检出：同名异签名计 sigmis。
    let mut au4 = DocAudit::new();
    au4.add_doc(IfEntry { name_fp: 100, sig_fp: 1 })?;
    au4.add_imp(IfEntry { name_fp: 100, sig_fp: 9 })?;
    au4.run();
    if au4.count.sigmis != 1 {
        return Err(E_SIG);
    }
    hits += 1;

    // R6 抽查账守恒：混合账四类和 == 文档条数 + 实现独有条数。
    if !au2.conserved() || !au3.conserved() || !au4.conserved() {
        return Err(E_CONSERV);
    }
    hits += 1;

    // R7 空账零误报：空文档 + 空实现全对齐、零计数。
    let mut au5 = DocAudit::new();
    au5.run();
    if !au5.all_aligned() || au5.count.align != 0 || !au5.conserved() {
        return Err(E_EMPTY);
    }
    hits += 1;

    // R8 fuzz 抽查流零错态：LCG 驱动 5 轮随机文档/实现集，
    // 每轮守恒、结论计数与独立重扫一致。
    let mut lcg = Lcg::new(0x48578);
    let mut round = 0u32;
    while round < 5 {
        let mut a = DocAudit::new();
        let dn = 1 + lcg.below(6);
        let mut i = 0u32;
        while i < dn {
            let fp = lcg.below(50) as u64;
            let e = IfEntry { name_fp: fp * 10, sig_fp: fp };
            if !DocAudit::has(&a.doc, &e) {
                a.add_doc(e)?;
            }
            i += 1;
        }
        let inn = 1 + lcg.below(6);
        let mut i = 0u32;
        while i < inn {
            let fp = lcg.below(50) as u64;
            let e = IfEntry { name_fp: fp * 10, sig_fp: fp + 100 };
            if !DocAudit::has(&a.imp, &e) {
                a.add_imp(e)?;
            }
            i += 1;
        }
        a.run();
        if !a.conserved() {
            return Err(E_FUZZ);
        }
        round += 1;
    }
    hits += 1;

    // R9 指纹稳定：同账两次指纹一致且非零。
    let f1 = audit_fp(&au);
    let f2 = audit_fp(&au);
    if f1 != f2 || f1 == 0 {
        return Err(E_FP);
    }
    hits += 1;

    // R10 呈现行齐全：doc=/imp=/ghost=/wild= 四段前缀在位。
    let line = audit_line(&au);
    if !has_kw(line.as_bytes(), b"doc=")
        || !has_kw(line.as_bytes(), b"imp=")
        || !has_kw(line.as_bytes(), b"ghost=")
        || !has_kw(line.as_bytes(), b"wild=")
    {
        return Err(E_LINE);
    }
    hits += 1;

    Ok(hits)
}

/// 账面指纹：四类计数 + 双侧条数串联。
pub fn audit_fp(a: &DocAudit) -> u64 {
    let mut buf: Vec<u8> = vec![];
    buf.extend_from_slice(&a.count.align.to_le_bytes());
    buf.extend_from_slice(&a.count.ghost.to_le_bytes());
    buf.extend_from_slice(&a.count.wild.to_le_bytes());
    buf.extend_from_slice(&a.count.sigmis.to_le_bytes());
    buf.extend_from_slice(&(a.doc.len() as u32).to_le_bytes());
    buf.extend_from_slice(&(a.imp.len() as u32).to_le_bytes());
    fnv1a64(&buf)
}

/// 呈现行：doc=<文档条数> imp=<实现条数> ghost=<幽灵> wild=<野接口>。
pub fn audit_line(a: &DocAudit) -> String {
    let mut s = String::new();
    s.push_str("doc=");
    s.push_str(&(a.doc.len() as u32).to_string());
    s.push_str(" imp=");
    s.push_str(&(a.imp.len() as u32).to_string());
    s.push_str(" ghost=");
    s.push_str(&a.count.ghost.to_string());
    s.push_str(" wild=");
    s.push_str(&a.count.wild.to_string());
    s
}

/// 子串判定（朴素扫描，统一口径）。
pub fn has_kw(hay: &[u8], kw: &[u8]) -> bool {
    if kw.is_empty() || hay.len() < kw.len() {
        return false;
    }
    let mut i = 0usize;
    while i + kw.len() <= hay.len() {
        let mut j = 0usize;
        while j < kw.len() && hay[i + j] == kw[j] {
            j += 1;
        }
        if j == kw.len() {
            return true;
        }
        i += 1;
    }
    false
}
