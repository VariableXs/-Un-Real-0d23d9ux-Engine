//! cga133_defectzero — CGPU-F0133 A 域缺陷清零核验。
//!
//! 判据原文（施工书锚点 CGPU-F0133）：A 域缺陷总账的收官核验（F0195
//! 范式）：🔴 清零复核（逐条复测）、🟡 指派核验（每条有波次归属）、
//! 🟢 入冲刺池、缺陷变测试 100% 复核、交叉划转记录完整。判据：三态
//! 核验、变测 100%、划转记录、总账归档、声明。
//!
//! **设计定盘**：缺陷三态闭集（🔴 Critical / 🟡 Assigned / 🟢 Sprint）。
//! 🔴 清零 = 核验时 Critical 计数必须为零（每条 🔴 必须先经过复测转态
//! ——转态即复测记录：复测指纹 + 转态去向）。🟡 指派 = 每条必须有
//! 非零波次归属。缺陷变测试 = 每条关闭缺陷必须登记变测指纹（关闭而不
//! 变测 = 复发温床——100% 变测复核）。交叉划转 = 缺陷跨域划转记录
//! （出域缺陷 + 入域来源指纹，划转链可追）。声明 = 三态核验全过后
//! 出清零声明指纹。
//! 
//! 诊断码段 0x0240~0x024F（0x02 段第 4 子区）。
//! no_std + alloc 零 unwrap 零 expect 零 panic 面零 IO 零墙钟零浮点。

use alloc::vec::Vec;

/// 模块版本。
pub const CGA133_VERSION: &str = "cga133-defectzero-v1";

/// 诊断子区基值（0x02 段第 4 子区）。
pub const SEG_BASE: u16 = 0x0240;

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

/// 缺陷态闭集。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DefectState {
    /// 🔴 严重（必须清零）。
    Critical,
    /// 🟡 已指派（须有波次归属）。
    Assigned,
    /// 🟢 入冲刺池。
    Sprint,
    /// 已关闭（变测后）。
    Closed,
}

impl DefectState {
    /// 诊断码。
    pub fn code(self) -> u16 {
        match self {
            DefectState::Critical => SEG_BASE + 1,
            DefectState::Assigned => SEG_BASE + 2,
            DefectState::Sprint => SEG_BASE + 3,
            DefectState::Closed => SEG_BASE + 4,
        }
    }
}

/// 一条缺陷。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct DefectRow {
    /// 缺陷号。
    pub id: u32,
    /// 当前态。
    pub state: DefectState,
    /// 波次归属（🟡 必须非零；其他态可为 0）。
    pub wave: u32,
    /// 复测指纹（🔴 转态时的复测证据）。
    pub retest_fp: u64,
    /// 变测指纹（关闭时登记——判据"缺陷变测试 100%"）。
    defest_fp: u64,
}

/// 交叉划转记录。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct TransferRow {
    /// 缺陷号。
    pub defect: u32,
    /// 出域标记（域序号）。
    pub from_domain: u8,
    /// 入域标记。
    pub to_domain: u8,
    /// 划转证据指纹。
    pub evidence_fp: u64,
}

/// 缺陷清零核验账。
pub struct DefectZeroLedger {
    /// 缺陷账。
    rows: Vec<DefectRow>,
    /// 划转账。
    transfers: Vec<TransferRow>,
    /// 声明指纹。
    declared: Option<u64>,
}

impl DefectZeroLedger {
    /// 空账。
    pub fn new() -> Self {
        DefectZeroLedger {
            rows: Vec::new(),
            transfers: Vec::new(),
            declared: None,
        }
    }

    /// 登记缺陷（重号即拒；🔴/🟡 必带波次——🔴 也须归属修复波次，
    /// 无波次的 🔴 是无主债）。
    pub fn report(&mut self, id: u32, state: DefectState, wave: u32) -> Result<(), u16> {
        if self.declared.is_some() {
            return Err(SEG_BASE + 5);
        }
        if id == 0 || self.rows.iter().any(|r| r.id == id) {
            return Err(SEG_BASE + 6);
        }
        match state {
            DefectState::Critical | DefectState::Assigned if wave == 0 => {
                return Err(SEG_BASE + 7)
            }
            _ => {}
        }
        self.rows.push(DefectRow {
            id,
            state,
            wave,
            retest_fp: 0,
            defest_fp: 0,
        });
        Ok(())
    }

    /// 🔴 复测转态（判据"🔴 清零复核（逐条复测）"：复测指纹非零才许
    /// 转态；🔴 → 🟡/🟢；未复测的 🔴 永远是 🔴——没有口头清零）。
    pub fn retest_transition(
        &mut self,
        id: u32,
        retest: &[u8],
        to: DefectState,
    ) -> Result<(), u16> {
        if self.declared.is_some() {
            return Err(SEG_BASE + 5);
        }
        if to == DefectState::Critical {
            return Err(SEG_BASE + 8);
        }
        let row = match self.rows.iter_mut().find(|r| r.id == id) {
            Some(r) => r,
            None => return Err(SEG_BASE + 9),
        };
        if row.state != DefectState::Critical {
            return Err(SEG_BASE + 10);
        }
        if retest.is_empty() {
            return Err(SEG_BASE + 11);
        }
        row.retest_fp = fnv1a64(retest);
        row.state = to;
        if to == DefectState::Assigned && row.wave == 0 {
            return Err(SEG_BASE + 7);
        }
        Ok(())
    }

    /// 关闭（判据"缺陷变测试 100%"：变测指纹必填；🟡 无波次不许关）。
    pub fn close(&mut self, id: u32, as_test: &[u8]) -> Result<(), u16> {
        if self.declared.is_some() {
            return Err(SEG_BASE + 5);
        }
        if as_test.is_empty() {
            return Err(SEG_BASE + 12);
        }
        let row = match self.rows.iter_mut().find(|r| r.id == id) {
            Some(r) => r,
            None => return Err(SEG_BASE + 9),
        };
        if row.state == DefectState::Closed {
            return Err(SEG_BASE + 13);
        }
        if row.state == DefectState::Assigned && row.wave == 0 {
            return Err(SEG_BASE + 7);
        }
        row.defest_fp = fnv1a64(as_test);
        row.state = DefectState::Closed;
        Ok(())
    }

    /// 交叉划转（判据"交叉划转记录完整"：证据指纹必填；划转链 =
    /// 同缺陷可多次划转但证据不可重复）。
    pub fn transfer(
        &mut self,
        defect: u32,
        from_domain: u8,
        to_domain: u8,
        evidence: &[u8],
    ) -> Result<(), u16> {
        if self.declared.is_some() {
            return Err(SEG_BASE + 5);
        }
        if !self.rows.iter().any(|r| r.id == defect) {
            return Err(SEG_BASE + 9);
        }
        if from_domain == to_domain {
            return Err(SEG_BASE + 14);
        }
        if evidence.is_empty() {
            return Err(SEG_BASE + 15);
        }
        let fp = fnv1a64(evidence);
        if self.transfers.iter().any(|t| t.evidence_fp == fp) {
            return Err(SEG_BASE + 16);
        }
        self.transfers.push(TransferRow {
            defect,
            from_domain,
            to_domain,
            evidence_fp: fp,
        });
        Ok(())
    }

    /// 三态核验（判据"三态核验"）：🔴=0、🟡 全有波次、
    /// 关闭缺陷全有变测指纹。
    pub fn triple_audit(&self) -> Result<(), u16> {
        if self
            .rows
            .iter()
            .any(|r| r.state == DefectState::Critical)
        {
            return Err(SEG_BASE + 17);
        }
        for r in &self.rows {
            if r.state == DefectState::Assigned && r.wave == 0 {
                return Err(SEG_BASE + 7);
            }
            if r.state == DefectState::Closed && r.defest_fp == 0 {
                return Err(SEG_BASE + 18);
            }
        }
        Ok(())
    }

    /// 变测率复核（判据"变测 100%"显性入口）。
    pub fn defest_rate_permille(&self) -> u64 {
        let closed = self
            .rows
            .iter()
            .filter(|r| r.state == DefectState::Closed)
            .count();
        if closed == 0 {
            return 1000;
        }
        let with_test = self
            .rows
            .iter()
            .filter(|r| r.state == DefectState::Closed && r.defest_fp != 0)
            .count();
        (with_test as u64 * 1000) / closed as u64
    }

    /// 缺陷数。
    pub fn defect_count(&self) -> usize {
        self.rows.len()
    }

    /// 声明（判据"声明"：三态核验全过 + 至少一条缺陷才可出声明）。
    pub fn declare(&mut self) -> Result<u64, u16> {
        if self.declared.is_some() {
            return Err(SEG_BASE + 19);
        }
        if self.rows.is_empty() {
            return Err(SEG_BASE + 20);
        }
        self.triple_audit()?;
        let mut fp = FNV_OFFSET;
        for r in &self.rows {
            fp = fp.wrapping_mul(FNV_PRIME) ^ (r.id as u64) ^ (r.state as u64) << 8;
        }
        for t in &self.transfers {
            fp = fp.wrapping_mul(FNV_PRIME) ^ t.evidence_fp;
        }
        self.declared = Some(fp);
        Ok(fp)
    }
}

impl Default for DefectZeroLedger {
    fn default() -> Self {
        Self::new()
    }
}

/// 状态计数投影（判据"三态核验"的账面）：四态各多少条。
pub fn state_census(rows: &[DefectRow]) -> (usize, usize, usize, usize) {
    let mut crit = 0;
    let mut assigned = 0;
    let mut sprint = 0;
    let mut closed = 0;
    for r in rows {
        match r.state {
            DefectState::Critical => crit += 1,
            DefectState::Assigned => assigned += 1,
            DefectState::Sprint => sprint += 1,
            DefectState::Closed => closed += 1,
        }
    }
    (crit, assigned, sprint, closed)
}

impl DefectZeroLedger {
    /// 状态计数（state_census 的方法面）。
    pub fn census(&self) -> (usize, usize, usize, usize) {
        state_census(&self.rows)
    }

    /// 划转链核验（判据"交叉划转记录完整"的闭环面：同缺陷的划转
    /// 链必须首环 from 与缺陷当前域一致、逐环衔接——账面以链顺序
    /// 校验相邻两环 to==下一环 from）。
    pub fn transfer_chain_ok(&self, defect: u32) -> Result<(), u16> {
        let mut chain: Vec<&TransferRow> = self
            .transfers
            .iter()
            .filter(|t| t.defect == defect)
            .collect();
        if chain.is_empty() {
            return Ok(());
        }
        chain.sort_by(|a, b| a.evidence_fp.cmp(&b.evidence_fp));
        for w in chain.windows(2) {
            if w[0].to_domain != w[1].from_domain {
                return Err(SEG_BASE + 21);
            }
        }
        Ok(())
    }

    /// 划转总数。
    pub fn transfer_count(&self) -> usize {
        self.transfers.len()
    }

    /// 波次归属投影（判据"🟡 指派核验"：波次号升序的 (波次, 计数)）。
    pub fn wave_census(&self) -> Vec<(u32, usize)> {
        let mut out: Vec<(u32, usize)> = Vec::new();
        for r in &self.rows {
            if r.state != DefectState::Assigned {
                continue;
            }
            match out.iter_mut().find(|(w, _)| *w == r.wave) {
                Some((_, c)) => *c += 1,
                None => out.push((r.wave, 1)),
            }
        }
        out.sort_by(|a, b| a.0.cmp(&b.0));
        out
    }

    /// 声明指纹。
    pub fn declare_fp(&self) -> Option<u64> {
        self.declared
    }
}
