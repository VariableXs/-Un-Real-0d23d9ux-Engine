//! CGPU-F0812 · 认证文档（目标 300 行）
//! ==========================================================================
//! 锚点判据（清单原文摘录）：
//! - 纯功能代码 ≥300 行；② 规格与判据按册内锚点原文逐条落实
//!   （CGPU Varix STAR II · 总纲与施工书#CGPU-F0812，册内文档不在本仓，
//!    以清单判据为施工依据）；③ 完成时 API/软件写回：状态=已完成。
//! - 主题：认证文档——认证体系文档集的结构管理：
//!   必备章节闭集、每章状态机（提纲/草稿/评审/定稿）、
//!   交叉引用一致性（引用的章节必须存在且已定稿）、
//!   文档齐备度千分比与文档账本守恒。
//!
//! 设计纪律：
//! - 必备章节闭集六类：Overview / ThreatModel / Procedure /
//!   Criteria / AppealProcess / ChangeLog——缺任一类即文档不齐备。
//! - 章节状态机闭集：Outline→Draft→Review→Final；
///   只能前进一格；Final 后可修订回 Review（修订计数 +1）。
//! - 交叉引用：章节可声明依赖其他章节；被依赖章节必须存在
///   且状态为 Final，否则引用悬空（悬空数计入齐备度罚分）。
//! - 文档账本定容：DOC_CAP=40 章，超容逐出最旧定稿章
///   （无则拒绝），守恒恒等式：total_sections == held + evicted。
//! - 纯功能线：no_std + alloc，整数算术，确定性；
///   诊断段 0x7000 本单独占，错误码 SEG_BASE+1..+N 闭集。

#![allow(dead_code)]
use alloc::string::{String, ToString};
use alloc::vec::Vec;

/// 诊断段基址（0x7000，批内 CGPU-F0812 独占）。
pub const SEG_BASE: u16 = 0x7000;

/// 章节编号重复。
pub const E_DUP: u16 = SEG_BASE + 1;
/// 文档册已满且无可逐出定稿章。
pub const E_CAP: u16 = SEG_BASE + 2;
/// 状态机非法转移。
pub const E_STATE: u16 = SEG_BASE + 3;
/// 未知章节或未知引用。
pub const E_NOSEC: u16 = SEG_BASE + 4;
/// 类别口径非法。
pub const E_KIND: u16 = SEG_BASE + 5;
/// 守恒破坏。
pub const E_LEDGER: u16 = SEG_BASE + 6;

/// 文档册容量。
pub const DOC_CAP: usize = 40;
/// 必备章节数（闭集六类）。
pub const REQUIRED_KINDS: usize = 6;

/// 章节类别（闭集六类）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DocKind {
    Overview,
    ThreatModel,
    Procedure,
    Criteria,
    AppealProcess,
    ChangeLog,
}

impl DocKind {
    pub fn ord(self) -> u8 {
        match self {
            DocKind::Overview => 0,
            DocKind::ThreatModel => 1,
            DocKind::Procedure => 2,
            DocKind::Criteria => 3,
            DocKind::AppealProcess => 4,
            DocKind::ChangeLog => 5,
        }
    }
}

/// 章节状态（闭集四态）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DocState {
    Outline,
    Draft,
    Review,
    Final,
}

impl DocState {
    /// 前进一格。
    pub fn next(self) -> Option<DocState> {
        match self {
            DocState::Outline => Some(DocState::Draft),
            DocState::Draft => Some(DocState::Review),
            DocState::Review => Some(DocState::Final),
            DocState::Final => None,
        }
    }
    /// 修订回退：Final→Review。
    pub fn revise(self) -> Option<DocState> {
        match self {
            DocState::Final => Some(DocState::Review),
            _ => None,
        }
    }
    pub fn ord(self) -> u8 {
        match self {
            DocState::Outline => 0,
            DocState::Draft => 1,
            DocState::Review => 2,
            DocState::Final => 3,
        }
    }
}

/// 文档章节。
#[derive(Clone, Debug)]
pub struct DocSection {
    /// 编号。
    pub id: u32,
    /// 标题。
    pub title: String,
    /// 类别。
    pub kind: DocKind,
    /// 状态。
    pub state: DocState,
    /// 修订次数。
    pub revisions: u32,
    /// 交叉引用（依赖的章节编号）。
    pub refs: Vec<u32>,
}

/// 文档册。
#[derive(Clone, Debug)]
pub struct DocBook {
    secs: Vec<DocSection>,
    evicted: u64,
    total_sections: u64,
}

impl DocBook {
    pub fn new() -> Self {
        DocBook {
            secs: Vec::new(),
            evicted: 0,
            total_sections: 0,
        }
    }
    /// 新增章节（Outline 态；类别在册内可重复但引用延迟校验）。
    pub fn add(&mut self, id: u32, title: &str, kind: DocKind) -> Result<(), u16> {
        if title.is_empty() {
            return Err(E_KIND);
        }
        if self.secs.iter().any(|s| s.id == id) {
            return Err(E_DUP);
        }
        if self.secs.len() >= DOC_CAP {
            let p = self.secs.iter().position(|s| s.state == DocState::Final);
            match p {
                Some(idx) => {
                    self.secs.remove(idx);
                    self.evicted += 1;
                }
                None => return Err(E_CAP),
            }
        }
        self.secs.push(DocSection {
            id,
            title: title.to_string(),
            kind,
            state: DocState::Outline,
            revisions: 0,
            refs: Vec::new(),
        });
        self.total_sections += 1;
        Ok(())
    }
    fn pos(&self, id: u32) -> Result<usize, u16> {
        self.secs.iter().position(|s| s.id == id).ok_or(E_NOSEC)
    }
    /// 状态推进一格。
    pub fn advance(&mut self, id: u32) -> Result<DocState, u16> {
        let p = self.pos(id)?;
        let s = &mut self.secs[p];
        let n = s.state.next().ok_or(E_STATE)?;
        s.state = n;
        Ok(n)
    }
    /// 修订：Final→Review，计数 +1。
    pub fn revise(&mut self, id: u32) -> Result<DocState, u16> {
        let p = self.pos(id)?;
        let s = &mut self.secs[p];
        let n = s.state.revise().ok_or(E_STATE)?;
        s.state = n;
        s.revisions += 1;
        Ok(n)
    }
    /// 挂交叉引用：目标必须存在（存在性即时校验）。
    pub fn link(&mut self, id: u32, target: u32) -> Result<(), u16> {
        let p = self.pos(id)?;
        if self.secs.iter().find(|s| s.id == id).map(|s| s.id) != Some(id) || self.pos(target).is_err() {
            return Err(E_NOSEC);
        }
        if self.secs[p].refs.iter().any(|r| *r == target) {
            return Err(E_DUP);
        }
        self.secs[p].refs.push(target);
        Ok(())
    }
    /// 悬空引用计数：引用目标不存在或未定稿。
    pub fn dangling_refs(&self) -> u32 {
        let mut n = 0u32;
        for s in &self.secs {
            for r in &s.refs {
                let ok = self
                    .secs
                    .iter()
                    .any(|t| t.id == *r && t.state == DocState::Final);
                if !ok {
                    n += 1;
                }
            }
        }
        n
    }
    /// 必备类别齐备性：六类各有 ≥1 章。
    pub fn kinds_covered(&self) -> [bool; REQUIRED_KINDS] {
        let mut m = [false; REQUIRED_KINDS];
        for s in &self.secs {
            m[s.kind.ord() as usize] = true;
        }
        m
    }
    /// 齐备度（千分比）：类别覆盖 600‰ + 全部章节定稿 400‰
    /// − 悬空引用罚（每个 50‰，下限 0）。
    pub fn readiness(&self) -> Result<u32, u16> {
        if self.secs.is_empty() {
            return Err(E_NOSEC);
        }
        let cov = self.kinds_covered().iter().filter(|b| **b).count();
        let mut score = cov * 100;
        let finals = self.secs.iter().filter(|s| s.state == DocState::Final).count();
        score += finals * 400 / self.secs.len();
        let dang = self.dangling_refs() as u32;
        score = score.saturating_sub(dang * 50);
        Ok(score.min(1000))
    }
    /// 守恒审计：total_sections == held + evicted。
    pub fn audit(&self) -> Result<(), u16> {
        if self.total_sections != self.secs.len() as u64 + self.evicted {
            return Err(E_LEDGER);
        }
        Ok(())
    }
    pub fn held(&self) -> usize {
        self.secs.len()
    }
}

/// 自例程：主路径（全定稿全引用齐备 1000‰）+ 悬空罚分 + 状态机 + 守恒。
pub fn drill() -> Result<u32, u16> {
    let mut b = DocBook::new();
    // 主路径：六类各一章。
    b.add(1, "总览", DocKind::Overview)?;
    b.add(2, "威胁模型", DocKind::ThreatModel)?;
    b.add(3, "流程", DocKind::Procedure)?;
    b.add(4, "判据", DocKind::Criteria)?;
    b.add(5, "申诉", DocKind::AppealProcess)?;
    b.add(6, "变更记录", DocKind::ChangeLog)?;
    b.audit()?;
    // 类别覆盖 600‰；全 Outline → 定稿 0 → readiness 600。
    if b.readiness()? != 600 {
        return Err(E_KIND);
    }
    // 状态机推进：全部到 Final。
    for id in 1..=6u32 {
        b.advance(id)?;
        b.advance(id)?;
        b.advance(id)?;
    }
    if b.readiness()? != 1000 {
        return Err(E_KIND);
    }
    // 修订：Final→Review → 定稿数 5/6 → readiness 600+333=933。
    b.revise(1)?;
    if b.readiness()? != 933 {
        return Err(E_KIND);
    }
    b.advance(1)?; // Review→Final
    if b.readiness()? != 1000 {
        return Err(E_KIND);
    }
    // 交叉引用：1 → 2（2 已 Final）→ 无悬空。
    b.link(1, 2)?;
    if b.dangling_refs() != 0 {
        return Err(E_KIND);
    }
    // 悬空：引用未定稿章 → 先把 3 修回 Review 再从 4 引它。
    b.revise(3)?;
    b.link(4, 3)?;
    // 悬空 1 → 罚 50 → 950。
    if b.readiness()? != 950 {
        return Err(E_KIND);
    }
    b.advance(3)?; // Review→Final
    if b.dangling_refs() != 0 || b.readiness()? != 1000 {
        return Err(E_KIND);
    }
    // 错误路径 1：状态机——Outline 不能修订、Final 不能前进。
    if b.revise(2) != Err(E_STATE) {
        return Err(E_STATE);
    }
    if b.advance(2) != Err(E_STATE) {
        return Err(E_STATE);
    }
    // 错误路径 2：未知章节/引用目标。
    if b.advance(99) != Err(E_NOSEC) {
        return Err(E_NOSEC);
    }
    // 错误路径 3：重复编号 / 空标题。
    if b.add(1, "dup", DocKind::Overview) != Err(E_DUP) {
        return Err(E_DUP);
    }
    if b.add(7, "", DocKind::Overview) != Err(E_KIND) {
        return Err(E_KIND);
    }
    // 重复引用拒绝。
    if b.link(1, 2) != Err(E_DUP) {
        return Err(E_DUP);
    }
    // 定容逐出：44 章全 Final + 2 新章触发逐出。
    let mut b2 = DocBook::new();
    for i in 1..=44u32 {
        let t = alloc::format!("s{}", i);
        b2.add(i, &t, DocKind::Overview)?;
        b2.advance(i)?;
        b2.advance(i)?;
        b2.advance(i)?;
    }
    b2.add(45, "n1", DocKind::Overview)?;
    b2.add(46, "n2", DocKind::Overview)?;
    b2.audit()?;
    if b2.held() != DOC_CAP || b2.evicted != 2 {
        return Err(E_LEDGER);
    }
    // 满且无 Final 可逐 → 拒绝。
    let mut b3 = DocBook::new();
    for i in 1..=40u32 {
        let t = alloc::format!("d{}", i);
        b3.add(i, &t, DocKind::Overview)?;
    }
    if b3.add(41, "no room", DocKind::Overview) != Err(E_CAP) {
        return Err(E_CAP);
    }
    // 空册齐备度。
    if DocBook::new().readiness() != Err(E_NOSEC) {
        return Err(E_NOSEC);
    }
    Ok(0)
}
