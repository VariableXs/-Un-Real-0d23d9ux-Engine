//! cgb752_e07close2 —— CGPU-F0752 E07 组收口与 E08 移交（二批面）。
//!
//! ═══════════════════════════════════════════════════════════════════════
//! 锚点：CGPU-F0752 E07 组收口与 E08 移交  <SEG 0x4c40>
//! 目标行数：≥320（判据：纯功能代码 ≥320 行）
//! 判据原文摘录：
//!   ① 纯功能代码 ≥320 行；
//!   ② 规格与判据按册内锚点原文逐条落实：CGPU Varix STAR II · 总纲与施工书 #CGPU-F0752；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//!
//! 设计纪律（与 F0744 一批收口互补：本单管"多批移交面"）：
//!   1. 多批移交账：E07 组可分多批（每批 ≤8 工单）分别收口，全部
//!      批收口完成后才能发总移交单（组级收口）；
//!   2. 批账三闸：批内工单号不跨批重复（全局唯一）、批摘要非空、
//!      批指纹逐批独立；总移交指纹 = 逐批指纹按批号序再聚；
//!   3. 移交状态机四态：Batches→AllSealed→Composed→Delivered，
//!      终态后任何批操作红码；
//!   4. 对账守恒：Σ批工单数 = 组工单总数；Σ批绿数 = 组绿数；
//      Σ批红单 = 0（收口前提）——三条恒等式逐条核；
//!   5. 交付回执：接收方回执号必须等于组指纹低 16 位（对账闸），
//      回执不符红码且不可二次交付；
//!   6. 零 unwrap/expect/panic，整数算术，确定性，no_std + alloc。
//!
//! 诊断段声明：0x4c40 段为本模块独占（SEG_BASE=0x4c40），判别式
//!   SEG_BASE+1..+N；与 0x4c20/0x4c60 邻段不重叠。
//! 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::string::{String, ToString};
use alloc::vec::Vec;

/// 模块版本。
pub const E7B_VERSION: &str = "cgb752_e07close2-v1";

/// 诊断段基址（0x4c40 · 本模块独占）。
pub const SEG_BASE: u16 = 0x4C40;

// ── XcCode 显式判别式（SEG_BASE+1..+N）────────────────────────────────
/// 0x4c41 批号越界（0..=15 之外）。
pub const XC_BAD_BATCH: u16 = SEG_BASE + 1;
/// 0x4c42 批工单超容（>8）。
pub const XC_BATCH_OVER: u16 = SEG_BASE + 2;
/// 0x4c43 工单号跨批重复（全局唯一被破坏）。
pub const XC_WID_DUP: u16 = SEG_BASE + 3;
/// 0x4c44 批未收口就组总（次序红线）。
pub const XC_BATCH_OPEN: u16 = SEG_BASE + 4;
/// 0x4c45 状态机非法边。
pub const XC_BAD_EDGE: u16 = SEG_BASE + 5;
/// 0x4c46 对账恒等式破（Σ批 ≠ 组）。
pub const XC_RECON_MISMATCH: u16 = SEG_BASE + 6;
/// 0x4c47 回执号失配（交付闸拒收）。
pub const XC_RECEIPT_MISMATCH: u16 = SEG_BASE + 7;
/// 0x4c48 二次交付红码。
pub const XC_DOUBLE_DELIVER: u16 = SEG_BASE + 8;
/// 0x4c49 空组组总（无批不可组总）。
pub const XC_NO_BATCHES: u16 = SEG_BASE + 9;
/// 0x4c4a 红单混入收口批。
pub const XC_RED_IN_BATCH: u16 = SEG_BASE + 10;

/// 批容（写死：8 工单/批）。
pub const BATCH_CAP: usize = 8;
/// 批数上限（写死：16 批）。
pub const BATCH_NUM: usize = 16;

/// 工单条目。
#[derive(Clone, Debug)]
pub struct WorkCard {
    /// 工单号（组内全局唯一）。
    pub wid: u16,
    /// 批号。
    pub bid: u8,
    /// 标题。
    pub title: String,
    /// 摘要（收口时必填非空）。
    pub summary: String,
    /// 绿旗。
    pub green: bool,
}

/// 移交状态机四态。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HandoverState {
    /// 批进行中。
    Batches,
    /// 全批已收口。
    AllSealed,
    /// 总移交单已组。
    Composed,
    /// 已交付（终态）。
    Delivered,
}

/// 合法边：Batches→AllSealed→Composed→Delivered。
pub fn legal_edges(s: HandoverState) -> &'static [HandoverState] {
    match s {
        HandoverState::Batches => &[HandoverState::AllSealed],
        HandoverState::AllSealed => &[HandoverState::Composed],
        HandoverState::Composed => &[HandoverState::Delivered],
        HandoverState::Delivered => &[],
    }
}

fn step(cur: HandoverState, next: HandoverState) -> Result<HandoverState, u16> {
    if legal_edges(cur).iter().any(|n| *n == next) {
        Ok(next)
    } else {
        Err(XC_BAD_EDGE)
    }
}

fn fnv32(mut h: u32, b: u8) -> u32 {
    h ^= b as u32;
    h.wrapping_mul(0x0100_0193)
}

fn eat_u32(h: u32, v: u32) -> u32 {
    let mut x = h;
    for i in 0..4 {
        x = fnv32(x, (v >> (i * 8)) as u8);
    }
    x
}

/// 批指纹：批号 + 逐工单号+绿旗 顺序吃。
pub fn batch_fingerprint(bid: u8, cards: &[&WorkCard]) -> u32 {
    let mut h = 0x811C_9DC5u32;
    h = eat_u32(h, bid as u32);
    for c in cards {
        h = eat_u32(h, c.wid as u32);
        h = fnv32(h, c.green as u8);
    }
    h
}

/// 总移交指纹：逐批指纹按批号升序再聚。
pub fn composed_fingerprint(per_batch: &[(u8, u32)]) -> u32 {
    let mut h = 0x811C_9DC5u32;
    for (bid, fp) in per_batch {
        h = eat_u32(h, *bid as u32);
        h = eat_u32(h, *fp);
    }
    h
}

/// 批收口单。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BatchSeal {
    /// 批号。
    pub bid: u8,
    /// 批内工单数。
    pub total: u16,
    /// 绿数。
    pub green: u16,
    /// 批指纹。
    pub fp: u32,
}

/// 组移交单。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GroupHandover {
    /// 总工单数。
    pub total: u16,
    /// 总绿数。
    pub green: u16,
    /// 批数。
    pub batches: u16,
    /// 总指纹。
    pub fp: u32,
}

/// 多批移交引擎。
pub struct MultiHandover {
    cards: Vec<WorkCard>,
    seals: Vec<BatchSeal>,
    state: HandoverState,
    composed: Option<GroupHandover>,
    /// 回执号（0 = 未交付）。
    pub receipt: u16,
}

impl MultiHandover {
    /// 空引擎。
    pub fn new() -> Self {
        MultiHandover {
            cards: Vec::new(),
            seals: Vec::new(),
            state: HandoverState::Batches,
            composed: None,
            receipt: 0,
        }
    }

    /// 加工单（Batches 限定；批号越界/批容超/号重复红码）。
    pub fn add_card(&mut self, wid: u16, bid: u8, title: &str) -> Result<(), u16> {
        if self.state != HandoverState::Batches {
            return Err(XC_BAD_EDGE);
        }
        if (bid as usize) >= BATCH_NUM {
            return Err(XC_BAD_BATCH);
        }
        if self.cards.iter().filter(|c| c.bid == bid).count() >= BATCH_CAP {
            return Err(XC_BATCH_OVER);
        }
        if self.cards.iter().any(|c| c.wid == wid) {
            return Err(XC_WID_DUP);
        }
        self.cards.push(WorkCard {
            wid,
            bid,
            title: String::from(title),
            summary: String::new(),
            green: false,
        });
        Ok(())
    }

    /// 工单摘要+绿旗（收口前设置）。
    pub fn settle_card(&mut self, wid: u16, summary: &str, green: bool) -> Result<(), u16> {
        if self.state != HandoverState::Batches {
            return Err(XC_BAD_EDGE);
        }
        if summary.is_empty() {
            return Err(XC_BATCH_OPEN);
        }
        let c = self.cards.iter_mut().find(|c| c.wid == wid).ok_or(XC_WID_DUP)?;
        c.summary = String::from(summary);
        c.green = green;
        Ok(())
    }

    /// 批收口：批内全部工单绿且摘要非空 → 落批收口单。
    pub fn seal_batch(&mut self, bid: u8) -> Result<BatchSeal, u16> {
        if self.state != HandoverState::Batches {
            return Err(XC_BAD_EDGE);
        }
        if (bid as usize) >= BATCH_NUM {
            return Err(XC_BAD_BATCH);
        }
        let in_batch: Vec<&WorkCard> = self.cards.iter().filter(|c| c.bid == bid).collect();
        if in_batch.is_empty() {
            return Err(XC_NO_BATCHES);
        }
        for c in &in_batch {
            if !c.green {
                return Err(XC_RED_IN_BATCH);
            }
            if c.summary.is_empty() {
                return Err(XC_BATCH_OPEN);
            }
        }
        if self.seals.iter().any(|s| s.bid == bid) {
            return Err(XC_BAD_EDGE);
        }
        let fp = batch_fingerprint(bid, &in_batch);
        let s = BatchSeal {
            bid,
            total: in_batch.len() as u16,
            green: in_batch.len() as u16,
            fp,
        };
        self.seals.push(s);
        Ok(s)
    }

    /// 组总：全批收口 → Batches→AllSealed→Composed，对账三条恒等式。
    pub fn compose(&mut self) -> Result<GroupHandover, u16> {
        if self.state != HandoverState::Batches {
            return Err(XC_BAD_EDGE);
        }
        if self.seals.is_empty() {
            return Err(XC_NO_BATCHES);
        }
        // 每个已用批号都必须已收口。
        let mut used: Vec<u8> = Vec::new();
        for c in &self.cards {
            if !used.contains(&c.bid) {
                used.push(c.bid);
            }
        }
        for b in &used {
            if !self.seals.iter().any(|s| s.bid == *b) {
                return Err(XC_BATCH_OPEN);
            }
        }
        // 对账恒等式。
        let sum_total: u32 = self.seals.iter().map(|s| s.total as u32).sum();
        let sum_green: u32 = self.seals.iter().map(|s| s.green as u32).sum();
        if sum_total != self.cards.len() as u32 || sum_green != self.cards.len() as u32 {
            return Err(XC_RECON_MISMATCH);
        }
        let mut per: Vec<(u8, u32)> = self.seals.iter().map(|s| (s.bid, s.fp)).collect();
        per.sort_by_key(|(b, _)| *b);
        let fp = composed_fingerprint(&per);
        self.state = step(self.state, HandoverState::AllSealed)?;
        self.state = step(self.state, HandoverState::Composed)?;
        let g = GroupHandover {
            total: sum_total as u16,
            green: sum_green as u16,
            batches: self.seals.len() as u16,
            fp,
        };
        self.composed = Some(g);
        Ok(g)
    }

    /// 交付：回执号 = 总指纹低 16 位。不符红码；二次交付红码。
    pub fn deliver(&mut self, claimed_receipt: u16) -> Result<(), u16> {
        if self.state != HandoverState::Composed {
            return Err(XC_BAD_EDGE);
        }
        if self.receipt != 0 {
            return Err(XC_DOUBLE_DELIVER);
        }
        let g = self.composed.ok_or(XC_BAD_EDGE)?;
        let want = (g.fp & 0xFFFF) as u16;
        if claimed_receipt != want {
            return Err(XC_RECEIPT_MISMATCH);
        }
        self.state = step(self.state, HandoverState::Delivered)?;
        self.receipt = claimed_receipt;
        Ok(())
    }

    /// 组总单只读。
    pub fn handover(&self) -> Option<GroupHandover> {
        self.composed
    }

    /// 当前状态。
    pub fn state(&self) -> HandoverState {
        self.state
    }

    /// 已收口批数。
    pub fn sealed_count(&self) -> usize {
        self.seals.len()
    }
}

/// 自例程：两批八工单全绿 → 批收口 → 组总 → 对账 → 交付 →
/// 错误路径（跨批重号/红单收口/漏批组总/回执失配/二次交付）。
pub fn drill() -> Result<u32, u16> {
    let mut mh = MultiHandover::new();
    // 批 0：四工单。
    for i in 0..4u16 {
        mh.add_card(i, 0, "批0工单")?;
    }
    // 批 1：四工单。
    for i in 4..8u16 {
        mh.add_card(i, 1, "批1工单")?;
    }
    // 全部 settle。
    for i in 0..8u16 {
        mh.settle_card(i, "ok", true)?;
    }
    // 批收口。
    let s0 = mh.seal_batch(0)?;
    let s1 = mh.seal_batch(1)?;
    if s0.total != 4 || s1.total != 4 {
        return Err(XC_RECON_MISMATCH);
    }
    let g = mh.compose()?;
    if g.total != 8 || g.green != 8 || g.batches != 2 {
        return Err(XC_RECON_MISMATCH);
    }
    // 回执 = fp 低 16 位。
    let want = (g.fp & 0xFFFF) as u16;
    if mh.deliver(want.wrapping_add(1)) != Err(XC_RECEIPT_MISMATCH) {
        return Err(XC_RECEIPT_MISMATCH);
    }
    mh.deliver(want)?;
    if mh.state() != HandoverState::Delivered || mh.receipt != want {
        return Err(XC_BAD_EDGE);
    }
    // 二次交付红码。
    if mh.deliver(want) != Err(XC_DOUBLE_DELIVER) {
        return Err(XC_DOUBLE_DELIVER);
    }

    // 错误路径 1：跨批重号。
    let mut mb = MultiHandover::new();
    mb.add_card(0, 0, "a")?;
    if mb.add_card(0, 1, "b") != Err(XC_WID_DUP) {
        return Err(XC_WID_DUP);
    }
    // 错误路径 2：红单收口。
    let mut mc = MultiHandover::new();
    mc.add_card(0, 0, "a")?;
    mc.settle_card(0, "bad", false)?;
    if mc.seal_batch(0) != Err(XC_RED_IN_BATCH) {
        return Err(XC_RED_IN_BATCH);
    }
    // 错误路径 3：漏批组总。
    let mut md = MultiHandover::new();
    md.add_card(0, 0, "a")?;
    md.add_card(1, 1, "b")?;
    md.settle_card(0, "ok", true)?;
    md.settle_card(1, "ok", true)?;
    md.seal_batch(0)?;
    if md.compose() != Err(XC_BATCH_OPEN) {
        return Err(XC_BATCH_OPEN);
    }
    // 错误路径 4：空组组总。
    let mut me = MultiHandover::new();
    if me.compose() != Err(XC_NO_BATCHES) {
        return Err(XC_NO_BATCHES);
    }
    // 错误路径 5：批容超。
    let mut mf = MultiHandover::new();
    for i in 0..8u16 {
        mf.add_card(i, 0, "x")?;
    }
    if mf.add_card(100, 0, "y") != Err(XC_BATCH_OVER) {
        return Err(XC_BATCH_OVER);
    }
    // 错误路径 6：批号越界。
    if mf.add_card(101, BATCH_NUM as u8, "z") != Err(XC_BAD_BATCH) {
        return Err(XC_BAD_BATCH);
    }
    // 错误路径 7：空摘要 settle。
    if mf.settle_card(0, "", true) != Err(XC_BATCH_OPEN) {
        return Err(XC_BATCH_OPEN);
    }
    // 确定性：同批两指纹同。
    let f1 = batch_fingerprint(0, &[&WorkCard { wid: 1, bid: 0, title: String::new(), summary: String::new(), green: true }]);
    let f2 = batch_fingerprint(0, &[&WorkCard { wid: 1, bid: 0, title: String::new(), summary: String::new(), green: true }]);
    if f1 != f2 {
        return Err(XC_RECON_MISMATCH);
    }
    Ok(0)
}
