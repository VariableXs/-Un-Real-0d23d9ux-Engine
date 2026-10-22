//! cga137_declare — CGPU-F0137 A 域完成宣告。
//!
//! 判据原文（施工书锚点 CGPU-F0137）：A 域 160 项完成宣告（F0396
//! 程序）：证据包（自检/验收矩阵/压测/查重/缺陷账/安全审查/性能内存
//! 错误码三账）、双签（施工与验收 AI）、台账状态变更、宣告后缺陷按
//! 回归级处理。判据：证据齐、双签、台账变更、回归规则、可追溯。
//!
//! **设计定盘**：证据包账面化：七件证据闭集（自检/验收矩阵/压测/
//! 查重/缺陷账/安全审查/三账合册），每件登记指纹（缺件即拒——证据
//! 包不许缺角）。双签 = 施工签 + 验收签两枚独立签章（同人同签即拒
//! ——双签的意义在于两个主体）。台账变更 = 宣告后域状态迁移（Active
//! → Declared，单向）。回归规则 = 宣告后新缺陷的处置档闭集（回归级/
//! 里程碑级/下一版级），回归级必须立即立案。可追溯 = 宣告指纹 =
//! 七件证据指纹 + 双签的链式 FNV。
//!
//! 诊断码段 0x0280~0x028F（0x02 段第 9 子区）。
//! no_std + alloc 零 unwrap 零 expect 零 panic 面零 IO 零墙钟零浮点。

use alloc::vec::Vec;

/// 模块版本。
pub const CGA137_VERSION: &str = "cga137-declare-v1";

/// 诊断子区基值（0x02 段第 9 子区）。
pub const SEG_BASE: u16 = 0x0280;

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

/// 七件证据闭集（判据"证据包"）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Evidence {
    /// 聚合自检。
    SelfCheck,
    /// 验收矩阵。
    AcceptMatrix,
    /// 压测。
    Stress,
    /// 查重。
    Dedup,
    /// 缺陷账。
    Defects,
    /// 安全审查。
    Security,
    /// 性能/内存/错误码三账合册。
    TripleLedger,
}

impl Evidence {
    /// 全集稳定序。
    pub const ALL: [Evidence; 7] = [
        Evidence::SelfCheck,
        Evidence::AcceptMatrix,
        Evidence::Stress,
        Evidence::Dedup,
        Evidence::Defects,
        Evidence::Security,
        Evidence::TripleLedger,
    ];

    /// 稳定序号。
    pub fn ordinal(self) -> u8 {
        match self {
            Evidence::SelfCheck => 0,
            Evidence::AcceptMatrix => 1,
            Evidence::Stress => 2,
            Evidence::Dedup => 3,
            Evidence::Defects => 4,
            Evidence::Security => 5,
            Evidence::TripleLedger => 6,
        }
    }

    /// 诊断码。
    pub fn code(self) -> u16 {
        SEG_BASE + self.ordinal() as u16 + 1
    }
}

/// 宣告后缺陷处置档闭集（判据"回归规则"）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PostDeclareLevel {
    /// 回归级（宣告前已绿的面变红——立即立案）。
    Regression,
    /// 里程碑级（阻塞下一里程碑）。
    Milestone,
    /// 下一版级（滚入下版迭代）。
    NextRelease,
}

impl PostDeclareLevel {
    /// 是否必须立即立案。
    pub fn immediate(self) -> bool {
        matches!(self, PostDeclareLevel::Regression)
    }

    /// 诊断码。
    pub fn code(self) -> u16 {
        match self {
            PostDeclareLevel::Regression => SEG_BASE + 8,
            PostDeclareLevel::Milestone => SEG_BASE + 9,
            PostDeclareLevel::NextRelease => SEG_BASE + 10,
        }
    }
}

/// 宣告账。
pub struct DeclareLedger {
    /// 证据指纹（七件逐件）。
    evidence: [Option<u64>; 7],
    /// 施工签。
    builder_sig: Option<u64>,
    /// 验收签。
    acceptor_sig: Option<u64>,
    /// 宣告指纹（双签后落账）。
    declared: Option<u64>,
    /// 宣告后缺陷账。
    post_defects: Vec<(u32, PostDeclareLevel)>,
}

impl DeclareLedger {
    /// 空账。
    pub fn new() -> Self {
        DeclareLedger {
            evidence: [None; 7],
            builder_sig: None,
            acceptor_sig: None,
            declared: None,
            post_defects: Vec::new(),
        }
    }

    /// 证据入包（判据"证据齐"：件号对齐七件闭集；重复入包即拒；
    /// 零指纹即拒）。
    pub fn submit_evidence(&mut self, ev: Evidence, fp: u64) -> Result<(), u16> {
        if self.declared.is_some() {
            return Err(SEG_BASE + 11);
        }
        if fp == 0 {
            return Err(SEG_BASE + 12);
        }
        let slot = &mut self.evidence[ev.ordinal() as usize];
        if slot.is_some() {
            return Err(SEG_BASE + 13);
        }
        *slot = Some(fp);
        Ok(())
    }

    /// 证据齐核验。
    pub fn evidence_complete(&self) -> Result<(), u16> {
        for ev in Evidence::ALL {
            if self.evidence[ev.ordinal() as usize].is_none() {
                return Err(ev.code());
            }
        }
        Ok(())
    }

    /// 施工签（判据"双签"：先施工后验收；重复签即拒）。
    pub fn sign_builder(&mut self, sig: &[u8]) -> Result<u64, u16> {
        if self.declared.is_some() {
            return Err(SEG_BASE + 11);
        }
        if self.builder_sig.is_some() {
            return Err(SEG_BASE + 14);
        }
        if sig.is_empty() {
            return Err(SEG_BASE + 15);
        }
        let fp = fnv1a64(sig);
        self.builder_sig = Some(fp);
        Ok(fp)
    }

    /// 验收签（判据"双签"：同签即拒——两个主体才能双签）。
    pub fn sign_acceptor(&mut self, sig: &[u8]) -> Result<u64, u16> {
        if self.declared.is_some() {
            return Err(SEG_BASE + 11);
        }
        let b = self.builder_sig.ok_or(SEG_BASE + 16)?;
        if self.acceptor_sig.is_some() {
            return Err(SEG_BASE + 17);
        }
        if sig.is_empty() {
            return Err(SEG_BASE + 15);
        }
        let fp = fnv1a64(sig);
        if fp == b {
            return Err(SEG_BASE + 18);
        }
        self.acceptor_sig = Some(fp);
        Ok(fp)
    }

    /// 宣告（判据"台账变更"：证据齐 + 双签齐才能宣告；宣告指纹 =
    /// 七件证据 + 双签链式 FNV——可追溯判据的账面）。
    pub fn declare(&mut self) -> Result<u64, u16> {
        if self.declared.is_some() {
            return Err(SEG_BASE + 19);
        }
        self.evidence_complete()?;
        let b = self.builder_sig.ok_or(SEG_BASE + 16)?;
        let a = self.acceptor_sig.ok_or(SEG_BASE + 17)?;
        let mut fp = FNV_OFFSET;
        for slot in &self.evidence {
            fp = fp.wrapping_mul(FNV_PRIME) ^ slot.unwrap_or(0);
        }
        fp = fp.wrapping_mul(FNV_PRIME) ^ b;
        fp = fp.wrapping_mul(FNV_PRIME) ^ a;
        self.declared = Some(fp);
        Ok(fp)
    }

    /// 宣告后缺陷登记（判据"回归规则"：回归级立即立案——本入口
    /// 直接入账，回归级条目计入立即立案数）。
    pub fn report_post_defect(&mut self, id: u32, lv: PostDeclareLevel) -> Result<(), u16> {
        if self.declared.is_none() {
            return Err(SEG_BASE + 20);
        }
        if id == 0 || self.post_defects.iter().any(|(i, _)| *i == id) {
            return Err(SEG_BASE + 21);
        }
        self.post_defects.push((id, lv));
        Ok(())
    }

    /// 立即立案数（回归级）。
    pub fn immediate_cases(&self) -> usize {
        self.post_defects
            .iter()
            .filter(|(_, lv)| lv.immediate())
            .count()
    }

    /// 宣告指纹。
    pub fn declare_fp(&self) -> Option<u64> {
        self.declared
    }

    /// 宣告后缺陷数。
    pub fn post_defect_count(&self) -> usize {
        self.post_defects.len()
    }
}

impl Default for DeclareLedger {
    fn default() -> Self {
        Self::new()
    }
}

impl DeclareLedger {
    /// 证据计数（判据"证据齐"的进度面：已入包件数 / 7）。
    pub fn evidence_count(&self) -> usize {
        self.evidence.iter().filter(|e| e.is_some()).count()
    }

    /// 缺件清单（判据"证据包不许缺角"的定位面）。
    pub fn missing_evidence(&self) -> Vec<Evidence> {
        Evidence::ALL
            .iter()
            .copied()
            .filter(|ev| self.evidence[ev.ordinal() as usize].is_none())
            .collect()
    }

    /// 处置档分布（判据"回归规则"的账面：三档各几条）。
    pub fn level_census(&self) -> (usize, usize, usize) {
        let r = self
            .post_defects
            .iter()
            .filter(|(_, lv)| *lv == PostDeclareLevel::Regression)
            .count();
        let m = self
            .post_defects
            .iter()
            .filter(|(_, lv)| *lv == PostDeclareLevel::Milestone)
            .count();
        let n = self
            .post_defects
            .iter()
            .filter(|(_, lv)| *lv == PostDeclareLevel::NextRelease)
            .count();
        (r, m, n)
    }

    /// 双签齐备。
    pub fn dual_signed(&self) -> bool {
        self.builder_sig.is_some() && self.acceptor_sig.is_some()
    }

    /// 施工签指纹。
    pub fn builder_fp(&self) -> Option<u64> {
        self.builder_sig
    }

    /// 验收签指纹。
    pub fn acceptor_fp(&self) -> Option<u64> {
        self.acceptor_sig
    }
}
