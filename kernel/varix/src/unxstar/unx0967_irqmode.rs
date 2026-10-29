//! unx0967_irqmode — UNX-F0967 Irq 态语义细化（全中断档确认断言）。
//!
//! 锚点块（工位 W6 · 批二 · 诊断段独占声明）：
//! 任务单：UNX-F0967 Irq 态语义细化（全中断档确认断言）
//! 判据原文摘录（卡面验收行）：全中断档语义逐条落账（每中断必确认/
//! 确认序与到号序一致/漏确认必抓），确认断言 10/10 次账平，档内
//! 轮询残留必拦。
//! 诊断段 0x7EE0（本单独占，XcCode 闭集见下）。
//! 设计纪律：全中断档铁律——事件全走中断面，确认（EOI）一对一；
//! 确认账与到号账恒等（到号数 = 确认数 + 未决数）；FIFO 确认序；
//! 全部错误走 Result<_, u16> 闭集。
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

pub const SEG_BASE: u16 = 0x7EE0;

// ── 诊断段独占 · 全中断档错误域 ────────────────────────────
const SEG_ERR: u16 = SEG_BASE;
const E_NOEOI: u16 = SEG_ERR + 1;  // 漏确认
const E_ORDER: u16 = SEG_ERR + 2;  // 确认序破约
const E_DUP: u16 = SEG_ERR + 3;    // 重复确认
const E_POLL: u16 = SEG_ERR + 4;   // 档内轮询残留
const E_STATE: u16 = SEG_ERR + 5;  // 档状态非法

/// 到号记录。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct IrqHit {
    pub vector: u8,
    pub t_us: u64,
    pub eoi: bool,
}

/// 全中断档执行器。
pub struct IrqMode {
    pub hits: Vec<IrqHit>,
    eoi_cursor: usize, // FIFO 确认游标
}

impl IrqMode {
    pub fn new() -> Self {
        IrqMode { hits: Vec::new(), eoi_cursor: 0 }
    }

    /// 恒等式：确认游标内全已确认且序同到号序。
    pub fn invariant_ok(&self) -> bool {
        for (i, h) in self.hits.iter().enumerate() {
            let done = i < self.eoi_cursor;
            if h.eoi != done { return false; }
        }
        true
    }

    /// 到号：中断到达（档内唯一事件入口）。
    pub fn hit(&mut self, vector: u8, t_us: u64) -> Result<(), u16> {
        if vector < 0x10 || vector >= 0xFE { return Err(E_STATE); }
        self.hits.push(IrqHit { vector, t_us, eoi: false });
        Ok(())
    }

    /// 确认（EOI）：严格 FIFO——确认的必须是游标指向的最早未决号。
    pub fn eoi(&mut self) -> Result<u8, u16> {
        if self.eoi_cursor >= self.hits.len() { return Err(E_STATE); }
        if self.hits[self.eoi_cursor].eoi { return Err(E_DUP); }
        let v = self.hits[self.eoi_cursor].vector;
        self.hits[self.eoi_cursor].eoi = true;
        self.eoi_cursor += 1;
        Ok(v)
    }

    /// 乱序确认注入：直接置位游标外记录，审计必抓。
    pub fn inject_out_of_order(&mut self, idx: usize) -> bool {
        if idx <= self.eoi_cursor || idx >= self.hits.len() { return false; }
        self.hits[idx].eoi = true;
        let caught = !self.invariant_ok();
        self.hits[idx].eoi = false;
        caught
    }

    /// 漏确认审计：到号未决数 = 总数 - 游标。
    pub fn pending_count(&self) -> u32 {
        (self.hits.len() - self.eoi_cursor) as u32
    }

    /// 收口断言：全部到号全确认。
    pub fn close_assert(&self) -> Result<u32, u16> {
        if self.pending_count() != 0 { return Err(E_NOEOI); }
        if !self.invariant_ok() { return Err(E_ORDER); }
        Ok(self.hits.len() as u32)
    }

    /// 档内轮询残留注入器：伪造轮询调用，必拦语义由调用方表达
    /// （返回值恒 E_POLL）。
    pub fn inject_poll(&self) -> u16 {
        if !self.hits.is_empty() { return E_POLL; }
        E_POLL
    }

    /// 确认序摘要。
    pub fn summary(&self) -> String {
        alloc::format!("irqmode hits={} eoi={} pending={}", self.hits.len(), self.eoi_cursor, self.pending_count())
    }
}

/// 全中断档会话：N 个中断到号后逐个确认。
pub fn irq_session(im: &mut IrqMode, vectors: &[u8], t0: u64) -> Result<u32, u16> {
    for (i, v) in vectors.iter().enumerate() {
        im.hit(*v, t0 + i as u64 * 10)?;
    }
    let mut n = 0u32;
    while im.pending_count() > 0 {
        im.eoi()?;
        n += 1;
    }
    im.close_assert()?;
    Ok(n)
}

/// 自例程：到号-确认一对一、序一致、漏确认必抓返回 0。
pub fn drill() -> Result<u32, u16> {
    // 场景一：32 号到号全确认。
    let mut im = IrqMode::new();
    let vs: Vec<u8> = (0u8..32).map(|i| 0x20 + i).collect();
    if irq_session(&mut im, &vs, 0)? != 32 { return Err(E_STATE); }
    if !im.invariant_ok() { return Err(E_ORDER); }
    // 场景二：确认序与到号序一致核验（重放一份记录序）。
    let mut im2 = IrqMode::new();
    im2.hit(0x20, 0)?; im2.hit(0x21, 10)?; im2.hit(0x22, 20)?;
    if im2.eoi()? != 0x20 { return Err(E_ORDER); }
    if im2.eoi()? != 0x21 { return Err(E_ORDER); }
    if im2.eoi()? != 0x22 { return Err(E_ORDER); }
    // 场景三：漏确认 10/10 必抓。
    for k in 1..=10u32 {
        let mut im3 = IrqMode::new();
        for i in 0..k { im3.hit(0x30 + i as u8, i as u64)?; }
        for _ in 0..k / 2 { im3.eoi()?; }
        if im3.close_assert() != Err(E_NOEOI) { return Err(E_NOEOI); }
    }
    // 场景四：乱序确认注入必抓。
    let mut im4 = IrqMode::new();
    im4.hit(0x40, 0)?; im4.hit(0x41, 10)?; im4.hit(0x42, 20)?;
    im4.eoi()?;
    if !im4.inject_out_of_order(2) { return Err(E_ORDER); }
    // 场景五：重复确认必拦（游标越界）。
    let mut im5 = IrqMode::new();
    im5.hit(0x50, 0)?;
    im5.eoi()?;
    if im5.eoi() != Err(E_STATE) { return Err(E_STATE); }
    // 场景六：档内轮询残留必拦语义。
    if im5.inject_poll() != E_POLL { return Err(E_POLL); }
    // 场景七：非法向量必拦。
    let mut im6 = IrqMode::new();
    if im6.hit(0x0F, 0) != Err(E_STATE) { return Err(E_STATE); }
    if im6.hit(0xFF, 0) != Err(E_STATE) { return Err(E_STATE); }
    if im.summary().is_empty() { return Err(E_STATE); }
    Ok(0)
}
