#![allow(dead_code)]
//! cgb763_perdocs —— CGPU-F0763 感知文档。
//!
//! ═══════════════════════════════════════════════════════════════════════
//! 锚点：CGPU-F0763 感知文档  <SEG 0x6AA0>
//! 目标行数：≥300（判据：纯功能代码 ≥300 行）
//! 判据原文摘录：
//!   ① 纯功能代码 ≥300 行；
//!   ② 规格与判据按册内锚点原文逐条落实：CGPU Varix STAR II · 总纲与施工书 #CGPU-F0763；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//!
//! 设计纪律（内嵌感知面文档账，整数面）：
//!   1. 文档种类闭集四项（写死语义）：SPEC 规格 / GUIDE 指南 /
//!      REF 参考 / FAQ 问答；
//!   2. 文档状态机：Draft → Reviewed → Frozen 单向，非法跃迁
//!      红码（跳审/回退/冻结后再审均拒）；
//!   3. 文档登记账：定容 12 槽（id 去重，重复登记红码），行数
//!      面合法域 1..=20000；守恒恒等式：登记数 = 在库数（无逐出，
//!      槽满即拒）+ 0 = 各状态计数之和；
//!   4. 引用账：定容 32 槽，文档间引用（from→to），引用目标必须
//!      已登记且不可自引；重复引用红码；
//!   5. 内容指纹：FNV-1a 滚过 (id, kind, lines) 三元组，冻结时
//!      定格，复核不符红码；
//!   6. 完备闸门：四种类各至少一篇 Frozen 才放行（缺类红码）；
//!   7. 零 unwrap/expect/panic，整数算术，确定性，no_std + alloc。
//!
//! 诊断段声明：0x6AA0 段为本模块独占（SEG_BASE=0x6AA0），判别式
//!   SEG_BASE+1..+N；与 0x6A80/0x6AC0 邻段不重叠。
//! 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::vec::Vec;

/// 模块版本。
pub const PD_VERSION: &str = "cgb763_perdocs-v1";

/// 诊断段基址（0x6AA0 · 本模块独占）。
pub const SEG_BASE: u16 = 0x6AA0;

// ── XcCode 显式判别式（SEG_BASE+1..+N）────────────────────────────────
/// 0x6AA1 文档 id 非法（0）。
pub const XC_BAD_ID: u16 = SEG_BASE + 1;
/// 0x6AA2 文档 id 重复登记。
pub const XC_DUP_ID: u16 = SEG_BASE + 2;
/// 0x6AA3 文档种类越界。
pub const XC_BAD_KIND: u16 = SEG_BASE + 3;
/// 0x6AA4 行数越界（0 或 >20000）。
pub const XC_BAD_LINES: u16 = SEG_BASE + 4;
/// 0x6AA5 登记账满（12 槽）。
pub const XC_REG_FULL: u16 = SEG_BASE + 5;
/// 0x6AA6 状态非法跃迁。
pub const XC_STATE_BAD: u16 = SEG_BASE + 6;
/// 0x6AA7 引用目标未登记。
pub const XC_REF_MISSING: u16 = SEG_BASE + 7;
/// 0x6AA8 引用重复或自引。
pub const XC_REF_BAD: u16 = SEG_BASE + 8;
/// 0x6AA9 完备闸门未过（有种类无冻结篇）。
pub const XC_GATE_INCOMPLETE: u16 = SEG_BASE + 9;
/// 0x6AAA 指纹复核不符。
pub const XC_DIGEST_MISMATCH: u16 = SEG_BASE + 10;
/// 0x6AAB 文档未登记。
pub const XC_DOC_MISSING: u16 = SEG_BASE + 11;
/// 0x6AAC 账目不守恒。
pub const XC_UNBALANCED: u16 = SEG_BASE + 12;

// ── 写死常量面 ────────────────────────────────────────────────────────
/// 种类闭集：0=SPEC 1=GUIDE 2=REF 3=FAQ。
pub const D_SPEC: usize = 0;
pub const D_GUIDE: usize = 1;
pub const D_REF: usize = 2;
pub const D_FAQ: usize = 3;
/// 种类数（写死：4）。
pub const KIND_CAP: usize = 4;
/// 登记账容量（写死：12）。
pub const REG_CAP: usize = 12;
/// 引用账容量（写死：32）。
pub const REF_CAP: usize = 32;
/// 行数上限（写死：20000）。
pub const LINES_MAX: u32 = 20_000;

/// 文档状态机。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DocState {
    /// 草稿。
    Draft,
    /// 已评审。
    Reviewed,
    /// 已冻结。
    Frozen,
}

/// 文档条目。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DocRec {
    /// 文档 id（>0）。
    pub id: u16,
    /// 种类（0..=3）。
    pub kind: usize,
    /// 行数。
    pub lines: u32,
    /// 状态。
    pub state: DocState,
    /// 内容指纹（冻结时定格）。
    pub digest: u32,
}

/// FNV-1a 32 位单步。
fn fnv_step(h: u32, b: u8) -> u32 {
    (h ^ (b as u32)).wrapping_mul(0x0100_0193)
}

/// 文档指纹：滚过 (id, kind, lines)。
pub fn doc_digest(id: u16, kind: usize, lines: u32) -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    for b in id.to_le_bytes() {
        h = fnv_step(h, b);
    }
    h = fnv_step(h, kind as u8);
    for b in lines.to_le_bytes() {
        h = fnv_step(h, b);
    }
    h
}

/// 引用边（from → to）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RefEdge {
    /// 引用方文档 id。
    pub from: u16,
    /// 被引文档 id。
    pub to: u16,
}

/// 感知文档账：登记账 + 引用账 + 完备闸门。
/// 守恒恒等式：登记数 = Draft 数 + Reviewed 数 + Frozen 数。
pub struct DocLedger {
    docs: Vec<DocRec>,
    refs: Vec<RefEdge>,
    draft_cnt: u32,
    reviewed_cnt: u32,
    frozen_cnt: u32,
}

impl DocLedger {
    /// 空账。
    pub fn new() -> Self {
        DocLedger {
            docs: Vec::new(),
            refs: Vec::new(),
            draft_cnt: 0,
            reviewed_cnt: 0,
            frozen_cnt: 0,
        }
    }

    /// 查文档（id → 下标）。
    pub fn find(&self, id: u16) -> Result<usize, u16> {
        if id == 0 {
            return Err(XC_BAD_ID);
        }
        for i in 0..self.docs.len() {
            if self.docs[i].id == id {
                return Ok(i);
            }
        }
        Err(XC_DOC_MISSING)
    }

    /// 登记：id/kind/lines 三查 + 去重 + 定容。
    pub fn register(&mut self, id: u16, kind: usize, lines: u32) -> Result<u32, u16> {
        if id == 0 {
            return Err(XC_BAD_ID);
        }
        if kind >= KIND_CAP {
            return Err(XC_BAD_KIND);
        }
        if lines == 0 || lines > LINES_MAX {
            return Err(XC_BAD_LINES);
        }
        if self.find(id).is_ok() {
            return Err(XC_DUP_ID);
        }
        if self.docs.len() >= REG_CAP {
            return Err(XC_REG_FULL);
        }
        let d = doc_digest(id, kind, lines);
        self.docs.push(DocRec {
            id,
            kind,
            lines,
            state: DocState::Draft,
            digest: d,
        });
        self.draft_cnt += 1;
        Ok(d)
    }

    /// 评审：Draft → Reviewed。
    pub fn review(&mut self, id: u16) -> Result<(), u16> {
        let i = self.find(id)?;
        if self.docs[i].state != DocState::Draft {
            return Err(XC_STATE_BAD);
        }
        self.docs[i].state = DocState::Reviewed;
        self.draft_cnt -= 1;
        self.reviewed_cnt += 1;
        Ok(())
    }

    /// 冻结：Reviewed → Frozen；指纹定格。
    pub fn freeze(&mut self, id: u16) -> Result<u32, u16> {
        let i = self.find(id)?;
        if self.docs[i].state != DocState::Reviewed {
            return Err(XC_STATE_BAD);
        }
        let d = self.docs[i].digest;
        // 冻结前指纹复核（登记后行数/种类不可变，应恒一致）。
        if d != doc_digest(self.docs[i].id, self.docs[i].kind, self.docs[i].lines) {
            return Err(XC_DIGEST_MISMATCH);
        }
        self.docs[i].state = DocState::Frozen;
        self.reviewed_cnt -= 1;
        self.frozen_cnt += 1;
        Ok(d)
    }

    /// 行数修订（仅 Draft 态；指纹随行数重算）。
    pub fn revise(&mut self, id: u16, lines: u32) -> Result<u32, u16> {
        let i = self.find(id)?;
        if self.docs[i].state != DocState::Draft {
            return Err(XC_STATE_BAD);
        }
        if lines == 0 || lines > LINES_MAX {
            return Err(XC_BAD_LINES);
        }
        self.docs[i].lines = lines;
        let d = doc_digest(self.docs[i].id, self.docs[i].kind, lines);
        self.docs[i].digest = d;
        Ok(d)
    }

    /// 引用登记：目标必须已登记、禁自引、禁重复、定容 32。
    pub fn link(&mut self, from: u16, to: u16) -> Result<(), u16> {
        if from == 0 || to == 0 {
            return Err(XC_BAD_ID);
        }
        if from == to {
            return Err(XC_REF_BAD);
        }
        self.find(from)?;
        self.find(to)?;
        if self.refs.len() >= REF_CAP {
            return Err(XC_REG_FULL);
        }
        for r in self.refs.iter() {
            if r.from == from && r.to == to {
                return Err(XC_REF_BAD);
            }
        }
        self.refs.push(RefEdge { from, to });
        Ok(())
    }

    /// 引用数。
    pub fn ref_count(&self) -> usize {
        self.refs.len()
    }

    /// 完备闸门：四种类各至少一篇 Frozen。
    pub fn gate(&self) -> Result<(), u16> {
        for k in 0..KIND_CAP {
            let mut hit = false;
            for d in self.docs.iter() {
                if d.kind == k && d.state == DocState::Frozen {
                    hit = true;
                }
            }
            if !hit {
                return Err(XC_GATE_INCOMPLETE);
            }
        }
        Ok(())
    }

    /// 守恒核：三态计数和 = 在库数；引用双方必在库。
    pub fn conserved(&self) -> Result<(), u16> {
        if self.draft_cnt + self.reviewed_cnt + self.frozen_cnt != self.docs.len() as u32 {
            return Err(XC_UNBALANCED);
        }
        for r in self.refs.iter() {
            if self.find(r.from).is_err() || self.find(r.to).is_err() {
                return Err(XC_UNBALANCED);
            }
        }
        Ok(())
    }

    /// 在库数。
    pub fn len(&self) -> usize {
        self.docs.len()
    }

    /// 指纹复核全量。
    pub fn verify_all(&self) -> Result<(), u16> {
        for d in self.docs.iter() {
            if d.digest != doc_digest(d.id, d.kind, d.lines) {
                return Err(XC_DIGEST_MISMATCH);
            }
        }
        Ok(())
    }

    /// 文档行数总和。
    pub fn total_lines(&self) -> u64 {
        let mut s: u64 = 0;
        for d in self.docs.iter() {
            s += d.lines as u64;
        }
        s
    }
}

/// 自例程：登记 → 修订 → 评审 → 冻结 → 引用 → 闸门 → 守恒 →
/// 错误路径（重复/跳审/自引/重引/缺类闸门/坏行数）。
pub fn drill() -> Result<u32, u16> {
    let mut g = DocLedger::new();
    // 错误路径 1：id=0 / 坏种类 / 坏行数。
    if g.register(0, D_SPEC, 100) != Err(XC_BAD_ID) {
        return Err(XC_BAD_ID);
    }
    if g.register(1, 4, 100) != Err(XC_BAD_KIND) {
        return Err(XC_BAD_KIND);
    }
    if g.register(1, D_SPEC, 0) != Err(XC_BAD_LINES) {
        return Err(XC_BAD_LINES);
    }
    if g.register(1, D_SPEC, LINES_MAX + 1) != Err(XC_BAD_LINES) {
        return Err(XC_BAD_LINES);
    }
    // 主路径：四篇文档（四种类各一）。
    g.register(1, D_SPEC, 320)?;
    g.register(2, D_GUIDE, 200)?;
    g.register(3, D_REF, 260)?;
    g.register(4, D_FAQ, 80)?;
    // 错误路径 2：重复登记。
    if g.register(1, D_SPEC, 300) != Err(XC_DUP_ID) {
        return Err(XC_DUP_ID);
    }
    // 修订（Draft 态）：SPEC 320 → 340，指纹随动。
    let d1 = g.revise(1, 340)?;
    g.verify_all()?;
    // 错误路径 3：跳审冻结（Draft → Frozen 拒）。
    if g.freeze(1) != Err(XC_STATE_BAD) {
        return Err(XC_STATE_BAD);
    }
    // 评审 + 冻结 ×4。
    g.review(1)?;
    g.review(2)?;
    g.review(3)?;
    g.review(4)?;
    g.freeze(1)?;
    g.freeze(2)?;
    g.freeze(3)?;
    g.freeze(4)?;
    // 错误路径 4：冻结后再修/再审。
    if g.revise(1, 350) != Err(XC_STATE_BAD) {
        return Err(XC_STATE_BAD);
    }
    if g.review(1) != Err(XC_STATE_BAD) {
        return Err(XC_STATE_BAD);
    }
    // 引用：1→2 2→3 3→4。
    g.link(1, 2)?;
    g.link(2, 3)?;
    g.link(3, 4)?;
    if g.ref_count() != 3 {
        return Err(XC_UNBALANCED);
    }
    // 错误路径 5：自引 / 重引 / 引用未登记。
    if g.link(2, 2) != Err(XC_REF_BAD) {
        return Err(XC_REF_BAD);
    }
    if g.link(1, 2) != Err(XC_REF_BAD) {
        return Err(XC_REF_BAD);
    }
    if g.link(1, 9) != Err(XC_DOC_MISSING) {
        return Err(XC_DOC_MISSING);
    }
    // 闸门：全类已冻结 → 放行。
    g.gate()?;
    g.conserved()?;
    g.verify_all()?;
    if g.len() != 4 {
        return Err(XC_UNBALANCED);
    }
    // 行数总和：340+200+260+80 = 880。
    if g.total_lines() != 880 {
        return Err(XC_UNBALANCED);
    }
    // 指纹确定性：同 (id,kind,lines) 双算同值。
    if doc_digest(1, D_SPEC, 340) != d1 {
        return Err(XC_DIGEST_MISMATCH);
    }
    // 缺类闸门：新账只冻三类 → 红码。
    let mut g2 = DocLedger::new();
    g2.register(1, D_SPEC, 100)?;
    g2.register(2, D_GUIDE, 100)?;
    g2.register(3, D_REF, 100)?;
    g2.review(1)?;
    g2.review(2)?;
    g2.review(3)?;
    g2.freeze(1)?;
    g2.freeze(2)?;
    g2.freeze(3)?;
    if g2.gate() != Err(XC_GATE_INCOMPLETE) {
        return Err(XC_GATE_INCOMPLETE);
    }
    // 登记账满：12 槽灌满后拒。
    let mut g3 = DocLedger::new();
    for i in 1..=12u16 {
        g3.register(i, D_FAQ, 10 + i as u32)?;
    }
    if g3.register(13, D_FAQ, 30) != Err(XC_REG_FULL) {
        return Err(XC_REG_FULL);
    }
    g3.conserved()?;
    Ok(0)
}
