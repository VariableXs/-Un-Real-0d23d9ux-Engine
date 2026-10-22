//! cga147_secfin — CGPU-F0147 A 域安全终审。
//!
//! 判据原文（施工书锚点 CGPU-F0147）：A 域安全终审（F0135 的域级
//! 终版）：越界写/无锁结构/死锁/输入清洗的终审复核 + 新增批
//! （A05-A10）的安全面补审，🔴 零终验。判据：五面终审、新增补审、
//! 🔴 零、注入全绿、归档。
//!
//! **设计定盘**：终审账 = F0135 四面 + 第五面「新增批补审」：
//! A05-A10 六个批次的安全登记补全（每批至少一条面登记——收官批
//! 不能没有安全记录）。终审语义 = 五面全复测（不信任阶段结论，
//! 终版独立重测——终审是复测不是抄账）。🔴 零终验 = 复测红单
//! 空。注入全绿 = 终版注入套件全拦截。归档 = 五面 + 注入指纹链。
//!
//! 诊断码段 0x0320~0x032F（0x03 段第 3 子区）。
//! no_std + alloc 零 unwrap 零 expect 零 panic 面零 IO 零墙钟零浮点。

use alloc::vec::Vec;

/// 模块版本。
pub const CGA147_VERSION: &str = "cga147-secfin-v1";

/// 诊断子区基值（0x03 段第 3 子区）。
pub const SEG_BASE: u16 = 0x0320;

/// FNV-1a 64 位偏移基。
pub const FNV_OFFSET: u64 = 0xcbf2_9ce4_8422_2325;

/// FNV-1a 64 位素数。
pub const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;

/// FNV-1a 64。
pub fn fnv1a64(bytes: &[u8]) -> u64 {
    let mut h = FNV_OFFSET;
    for &b in bytes {
        h ^= b as u64;
        h = h.wrapping_mul(FNV_PRIME);
    }
    h
}

/// 新增批闭集（判据"A05-A10"）。
pub const NEW_BATCHES: [u32; 6] = [5, 6, 7, 8, 9, 10];

/// 终审五面闭集。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FinFace {
    /// 越界写。
    Bounds,
    /// 无锁结构。
    LockFree,
    /// 死锁。
    Deadlock,
    /// 输入清洗。
    Sanitize,
    /// 新增批补审。
    NewBatch,
}

impl FinFace {
    /// 全集稳定序。
    pub const ALL: [FinFace; 5] = [
        FinFace::Bounds,
        FinFace::LockFree,
        FinFace::Deadlock,
        FinFace::Sanitize,
        FinFace::NewBatch,
    ];

    /// 稳定序号。
    pub fn ordinal(self) -> u8 {
        match self {
            FinFace::Bounds => 0,
            FinFace::LockFree => 1,
            FinFace::Deadlock => 2,
            FinFace::Sanitize => 3,
            FinFace::NewBatch => 4,
        }
    }

    /// 诊断码。
    pub fn code(self) -> u16 {
        SEG_BASE + self.ordinal() as u16 + 1
    }
}

/// 终审复测条目（面 × 主体 → 复测结论）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct RetestRow {
    /// 面。
    pub face: FinFace,
    /// 主体号（算子/结构/入口/批次号）。
    pub subject: u32,
    /// 复测结论（true=安全）。
    pub safe: bool,
    /// 复测证据指纹。
    pub evidence_fp: u64,
}

impl RetestRow {
    /// 行合法性。
    pub fn validate(&self) -> Result<(), u16> {
        if self.subject == 0 || self.evidence_fp == 0 {
            return Err(SEG_BASE + 6);
        }
        Ok(())
    }
}

/// 安全终审账。
pub struct SecFinalLedger {
    /// 复测账。
    rows: Vec<RetestRow>,
    /// 注入套件（用例指纹, 是否拦截）。
    injections: Vec<(u64, bool)>,
    /// 豁免（主体 → ADR）。
    waivers: Vec<(u32, u32)>,
    /// 归档。
    archived: Option<u64>,
}

impl SecFinalLedger {
    /// 空账。
    pub fn new() -> Self {
        SecFinalLedger {
            rows: Vec::new(),
            injections: Vec::new(),
            waivers: Vec::new(),
            archived: None,
        }
    }

    /// 复测登记（同面同主体重复即拒——终审复测一次定论，不许刷）。
    pub fn retest(&mut self, row: RetestRow) -> Result<(), u16> {
        if self.archived.is_some() {
            return Err(SEG_BASE + 7);
        }
        row.validate()?;
        if self
            .rows
            .iter()
            .any(|r| r.face == row.face && r.subject == row.subject)
        {
            return Err(SEG_BASE + 8);
        }
        self.rows.push(row);
        Ok(())
    }

    /// 注入用例（重复指纹即拒）。
    pub fn inject(&mut self, case: &[u8], intercepted: bool) -> Result<(), u16> {
        if self.archived.is_some() {
            return Err(SEG_BASE + 7);
        }
        let fp = fnv1a64(case);
        if self.injections.iter().any(|(f, _)| *f == fp) {
            return Err(SEG_BASE + 9);
        }
        self.injections.push((fp, intercepted));
        Ok(())
    }

    /// 豁免登记。
    pub fn waive(&mut self, subject: u32, adr: u32) -> Result<(), u16> {
        if self.archived.is_some() {
            return Err(SEG_BASE + 7);
        }
        if adr == 0 || self.waivers.iter().any(|(s, _)| *s == subject) {
            return Err(SEG_BASE + 10);
        }
        self.waivers.push((subject, adr));
        Ok(())
    }

    /// 面覆盖核验（判据"五面终审"：每面至少一条复测）。
    pub fn faces_covered(&self) -> Result<(), u16> {
        for f in FinFace::ALL {
            if !self.rows.iter().any(|r| r.face == f) {
                return Err(f.code());
            }
        }
        Ok(())
    }

    /// 新增批补审核验（判据"新增批（A05-A10）的安全面补审"：六批
    /// 每批至少一条 NewBatch 面复测）。
    pub fn new_batches_covered(&self) -> Result<(), u16> {
        for b in NEW_BATCHES {
            if !self
                .rows
                .iter()
                .any(|r| r.face == FinFace::NewBatch && r.subject == b)
            {
                return Err(SEG_BASE + 11);
            }
        }
        Ok(())
    }

    /// 🔴 红单（复测不安全且未豁免的主体）。
    pub fn red_items(&self) -> Vec<u32> {
        let mut out: Vec<u32> = Vec::new();
        for r in &self.rows {
            if !r.safe && !self.waivers.iter().any(|(s, _)| *s == r.subject) {
                out.push(r.subject);
            }
        }
        out
    }

    /// 🔴 零终验。
    pub fn reds_clear(&self) -> bool {
        self.red_items().is_empty()
    }

    /// 注入全绿。
    pub fn injections_green(&self) -> bool {
        !self.injections.is_empty() && self.injections.iter().all(|(_, i)| *i)
    }

    /// 复测总数。
    pub fn retest_count(&self) -> usize {
        self.rows.len()
    }

    /// 归档（判据"归档"：五面齐 + 新增批齐 + 红零 + 注入绿）。
    pub fn archive(&mut self) -> Result<u64, u16> {
        if self.archived.is_some() {
            return Err(SEG_BASE + 12);
        }
        self.faces_covered()?;
        self.new_batches_covered()?;
        if !self.reds_clear() {
            return Err(SEG_BASE + 13);
        }
        if !self.injections_green() {
            return Err(SEG_BASE + 14);
        }
        let mut fp = FNV_OFFSET;
        for r in &self.rows {
            fp = fp.wrapping_mul(FNV_PRIME)
                ^ ((r.face as u64) << 24)
                ^ (r.subject as u64)
                ^ r.evidence_fp;
        }
        self.archived = Some(fp);
        Ok(fp)
    }

    /// 归档指纹。
    pub fn archive_fp(&self) -> Option<u64> {
        self.archived
    }
}

impl Default for SecFinalLedger {
    fn default() -> Self {
        Self::new()
    }
}

impl SecFinalLedger {
    /// 拦截率（千分比）：注入套件中被拦截用例占比。
    pub fn intercept_permille(&self) -> Option<u32> {
        if self.injections.is_empty() {
            return None;
        }
        let hit = self.injections.iter().filter(|(_, ok)| *ok).count();
        Some(((hit * 1000) / self.injections.len()) as u32)
    }

    /// 豁免冲突检查（判据「豁免须显性备案」hygiene）：同一主体
    /// 不得两条豁免（一个风险一个 ADR，多 ADR = 备案失控）。
    pub fn waiver_hygiene(&self) -> Result<(), u16> {
        for (i, (s, _)) in self.waivers.iter().enumerate() {
            for (j, (s2, _)) in self.waivers.iter().enumerate() {
                if i != j && s == s2 {
                    return Err(SEG_BASE + 20);
                }
            }
        }
        Ok(())
    }

    /// 复测分面普查：六面各多少行（覆盖缺口直接可见）。
    pub fn face_census(&self) -> [usize; 6] {
        let mut out = [0usize; 6];
        for r in &self.rows {
            out[r.face.ordinal() as usize] += 1;
        }
        out
    }

    /// 归档前置自检：无红项 + 注入全拦 + 豁免卫生。
    pub fn pre_archive(&self) -> Result<(), u16> {
        self.faces_covered()?;
        if !self.reds_clear() {
            return Err(SEG_BASE + 21);
        }
        if !self.injections_green() {
            return Err(SEG_BASE + 22);
        }
        self.waiver_hygiene()
    }
}
