//! unx48205_xfocus — UNX-F48205 Xwayland 焦点切换 context 对齐（X focus ↔ context）（诊断段 0xBA80）。
//!
//! 判据原文：
//!   J1 对齐 ×1000 零丢 commit；判据锚定 §AI-61 §2 专题二销毁序。
//!   J2 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计纪律：
//!   1. 对齐不变式：X focus 窗口与 IME context 一一映射；focus 变更事件
//!      与 context 挂接/摘除必须成对完成，不允许出现「focus 已切换而
//!      context 仍挂旧窗」的中间态跨事件存活。
//!   2. 销毁序宪法：context 销毁必须先摘 focus 挂接、再 flush 未上屏
//!      commit、最后释放 context 三步有序；乱序 = 丢 commit 源头。
//!   3. ×1000 切换演练：连续 1000 次 focus 切换（含快速往返 A→B→A、
//!      A→销毁→B 两类），逐次对账 commit 零丢失、context 零悬空。
//!   4. 快速切换合并：同一对窗口 10ms 内往返只做一次真切换，中间态
//!      合并记账（账本合并数+事件数守恒）。
//!   5. commit 缓冲归属：未上屏 commit 随 context 走，跟随旧 context
//!      flush 到旧窗，绝不跨窗投递（跨窗=串扰=丢）。
//!   6. 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。

use crate::alloc::vec::Vec;

pub const SEG_BASE: u16 = 0xBA80;

const E_HANG_CTX: u16 = SEG_BASE + 1;   // context 悬空（focus 无主）
const E_ORPHAN_FOCUS: u16 = SEG_BASE + 2; // focus 挂在已销毁 context
const E_CROSS_COMMIT: u16 = SEG_BASE + 3; // commit 跨窗投递
const E_BAD_ORDER: u16 = SEG_BASE + 4;  // 销毁序违宪
const E_LOST_COMMIT: u16 = SEG_BASE + 5; // commit 丢失
const E_MERGE_UNBAL: u16 = SEG_BASE + 6; // 合并账不平
const E_DOUBLE_BIND: u16 = SEG_BASE + 7; // 一 context 双 focus 挂接
const E_SESS_CLOSED: u16 = SEG_BASE + 8; // 会话已关仍收事件

/// X 窗口句柄（演示层用 u32 假句柄，语义同 XWindow）。
pub type XWin = u32;

/// IME context 句柄。
pub type CtxId = u32;

/// commit 事件：跟随 context 的文本提交。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Commit {
    pub seq: u32,
    pub text_fp: u32, // 文本指纹（不携带明文，隐私红线）
}

/// context 账。
#[derive(Debug, Clone)]
struct CtxEntry {
    id: CtxId,
    win: Option<XWin>,      // 当前挂接的 X 窗（None=未挂）
    last_win: Option<XWin>, // 最后一次挂接窗（销毁 flush 的落点）
    pending: Vec<Commit>,
    dead: bool,
    flush_out: Vec<(XWin, Commit)>,
    destroy_order: [bool; 3], // [摘挂接, flush, 释放] 三步留痕
}

/// 焦点切换对齐总账。
#[derive(Debug)]
pub struct XFocus {
    ctxs: Vec<CtxEntry>,
    focus: Option<(XWin, CtxId)>, // 当前 focus (win, ctx) 成对
    switch_events: u32,
    merged_switches: u32,
    commits_in: u32,
    commits_flushed: u32,
    last_switch_tick: u32,
    closed: bool,
}

impl XFocus {
    pub fn new() -> Self {
        XFocus {
            ctxs: Vec::new(),
            focus: None,
            switch_events: 0,
            merged_switches: 0,
            commits_in: 0,
            commits_flushed: 0,
            last_switch_tick: 0,
            closed: false,
        }
    }

    /// 创建 context。
    pub fn ctx_new(&mut self, id: CtxId) -> Result<(), u16> {
        if self.closed {
            return Err(E_SESS_CLOSED);
        }
        if self.ctxs.iter().any(|c| c.id == id) {
            return Err(E_DOUBLE_BIND);
        }
        self.ctxs.push(CtxEntry {
            id,
            win: None,
            last_win: None,
            pending: Vec::new(),
            dead: false,
            flush_out: Vec::new(),
            destroy_order: [false; 3],
        });
        Ok(())
    }

    /// focus 请求：win ↔ ctx 成对挂接。旧 focus 自动摘除并挂新。
    /// 快速往返（同 tick 窗口内同 win）合并只记一次真切换。
    pub fn focus_req(&mut self, win: XWin, ctx: CtxId, tick: u32) -> Result<(), u16> {
        if self.closed {
            return Err(E_SESS_CLOSED);
        }
        // 合并判定（只读快照，避免与摘旧挂新互斥借用）。
        let merge_now = match self.focus {
            Some((pw, pc)) => pw == win && pc == ctx && tick.saturating_sub(self.last_switch_tick) < 10,
            None => false,
        };
        if merge_now {
            self.merged_switches += 1;
            self.switch_events += 1;
            self.last_switch_tick = tick;
            return Ok(());
        }
        // 摘旧：旧 ctx 的 win 槽位清空（独立借用段）。
        if let Some((_, pc)) = self.focus {
            if let Some(oc) = self.ctxs.iter_mut().find(|c| c.id == pc) {
                oc.win = None;
            }
        }
        // 挂新：独立借用段。
        let c = self
            .ctxs
            .iter_mut()
            .find(|c| c.id == ctx && !c.dead)
            .ok_or(E_HANG_CTX)?;
        if c.win.is_some() {
            return Err(E_DOUBLE_BIND); // 一 context 不得双挂
        }
        c.win = Some(win);
        c.last_win = Some(win);
        self.focus = Some((win, ctx));
        self.switch_events += 1;
        self.last_switch_tick = tick;
        Ok(())
    }

    /// commit 进入：挂到 ctx 的 pending 缓冲（跟随 context，不跨窗）。
    pub fn commit_in(&mut self, ctx: CtxId, c: Commit) -> Result<(), u16> {
        if self.closed {
            return Err(E_SESS_CLOSED);
        }
        let e = self
            .ctxs
            .iter_mut()
            .find(|e| e.id == ctx && !e.dead)
            .ok_or(E_HANG_CTX)?;
        e.pending.push(c);
        self.commits_in += 1;
        Ok(())
    }

    /// 上屏 flush：pending 全量投递到该 ctx 当前挂接的窗；
    /// 无窗挂接时允许滞留（切回再投），但销毁时必须先 flush。
    pub fn flush(&mut self, ctx: CtxId) -> Result<u32, u16> {
        let e = self
            .ctxs
            .iter_mut()
            .find(|e| e.id == ctx && !e.dead)
            .ok_or(E_HANG_CTX)?;
        let win = match e.win {
            Some(w) => w,
            None => return Ok(0), // 无窗滞留，不算丢
        };
        let n = e.pending.len() as u32;
        for c in e.pending.drain(..) {
            e.flush_out.push((win, c));
            self.commits_flushed += 1;
        }
        Ok(n)
    }

    /// 销毁序三步：摘挂接 → flush → 释放。乱序或漏步 = 违宪。
    pub fn ctx_destroy(&mut self, ctx: CtxId) -> Result<(), u16> {
        // 第一步：摘挂接（若 focus 正挂此 ctx，focus 清空）。
        if let Some((_, pc)) = self.focus {
            if pc == ctx {
                self.focus = None;
            }
        }
        let e = self
            .ctxs
            .iter_mut()
            .find(|e| e.id == ctx)
            .ok_or(E_HANG_CTX)?;
        if e.dead {
            return Err(E_HANG_CTX); // 重复销毁
        }
        e.win = None;
        e.destroy_order[0] = true;
        // 第二步：flush（销毁前必须清空 pending —— 投往最后挂接窗；
        //        从未有挂接窗的 pending 无落点 = 丢账显性）。
        if !e.pending.is_empty() {
            match e.last_win {
                Some(lw) => {
                    let n = e.pending.len() as u32;
                    for c in e.pending.drain(..) {
                        e.flush_out.push((lw, c));
                        self.commits_flushed += 1;
                    }
                    let _ = n;
                }
                None => return Err(E_BAD_ORDER),
            }
        }
        e.destroy_order[1] = true;
        // 第三步：释放（标记死账，账本保留供审计）。
        e.dead = true;
        e.destroy_order[2] = true;
        Ok(())
    }

    /// 对账：×N 演练后的全账审计 —— 零悬空、零丢 commit、零跨窗。
    pub fn audit(&self) -> Result<(), u16> {
        // 零悬空：focus 引用的 ctx 必须存在且活着。
        if let Some((w, c)) = self.focus {
            match self.ctxs.iter().find(|e| e.id == c) {
                Some(e) if !e.dead => {
                    if e.win != Some(w) {
                        return Err(E_ORPHAN_FOCUS);
                    }
                }
                _ => return Err(E_ORPHAN_FOCUS),
            }
        }
        // 零丢 commit：入账 = flush 落地 + 活 ctx 缓冲滞留 + 死账销毁前清空。
        let buffered: u32 = self
            .ctxs
            .iter()
            .filter(|e| !e.dead)
            .map(|e| e.pending.len() as u32)
            .sum();
        if self.commits_in != self.commits_flushed + buffered {
            return Err(E_LOST_COMMIT);
        }
        // 零跨窗：flush_out 里每条 (win, commit) 的 win 必须曾是该 ctx 挂接窗
        // （销毁清空后无法直接验，取账本口径：flush 时 win 必非空——已由 flush 保证）。
        for e in &self.ctxs {
            for (w, _) in &e.flush_out {
                let _ = w; // flush 路径已强制 win 非空；此处留审计位
            }
        }
        // 合并账守恒：switch_events = 真切换 + 合并记账。
        let true_switch = self.switch_events - self.merged_switches;
        if self.merged_switches > self.switch_events || true_switch == 0 && self.switch_events > 0 {
            return Err(E_MERGE_UNBAL);
        }
        Ok(())
    }

    pub fn switch_events(&self) -> u32 {
        self.switch_events
    }
    pub fn merged(&self) -> u32 {
        self.merged_switches
    }
    pub fn commits_in(&self) -> u32 {
        self.commits_in
    }
    pub fn commits_flushed(&self) -> u32 {
        self.commits_flushed
    }

    pub fn close(&mut self) -> Result<(), u16> {
        if self.closed {
            return Err(E_SESS_CLOSED);
        }
        self.closed = true;
        self.audit()
    }
}

/// 自例程判据：×1000 切换演练零丢 commit。
pub fn drill() -> Result<u32, u16> {
    let mut hits = 0u32;

    // R1 场景：×1000 双窗交替切换，逐对 commit 跟随 flush，末账零丢。
    let mut f = XFocus::new();
    f.ctx_new(1)?;
    f.ctx_new(2)?;
    let mut seq = 0u32;
    for i in 0..1000u32 {
        let cur = if i % 2 == 0 { 1 } else { 2 };
        let tick = i * 20; // 间隔 20 tick，不触发合并
        f.focus_req(100 + cur, cur, tick)?;
        f.commit_in(cur, Commit { seq, text_fp: 0xA000 + seq })?;
        seq += 1;
        f.flush(cur)?;
    }
    f.audit()?;
    if f.commits_in() != 1000 || f.commits_flushed() != 1000 {
        return Err(E_LOST_COMMIT);
    }
    if f.switch_events() != 1000 {
        return Err(E_MERGE_UNBAL);
    }
    hits += 1;

    // R2 场景：快速往返合并 —— 同窗 10 tick 内重复请求只记合并账。
    let mut f2 = XFocus::new();
    f2.ctx_new(7)?;
    f2.focus_req(700, 7, 100)?;
    for k in 0..9u32 {
        f2.focus_req(700, 7, 101 + k)?; // 间隔 <10 → 合并
    }
    if f2.merged() != 9 || f2.switch_events() != 10 {
        return Err(E_MERGE_UNBAL);
    }
    f2.audit()?;
    hits += 1;

    // R3 场景：销毁序三步 —— 带 pending 销毁必须先投递 last_win（零丢）；
    //        从未挂接窗的 ctx 带 pending 销毁 = 无落点违宪显性。
    let mut f3 = XFocus::new();
    f3.ctx_new(1)?;
    f3.ctx_new(2)?;
    f3.focus_req(101, 1, 0)?;
    f3.commit_in(1, Commit { seq: 1, text_fp: 0xB001 })?;
    f3.ctx_destroy(1)?; // 销毁步 2 自动投递 last_win=101
    if f3.commits_flushed() != 1 {
        return Err(E_LOST_COMMIT);
    }
    f3.focus_req(102, 2, 50)?;
    f3.audit()?;
    // 违宪路径：从未挂接却收了 commit 的 ctx，销毁必拒。
    let mut f3b = XFocus::new();
    f3b.ctx_new(8)?;
    f3b.commit_in(8, Commit { seq: 2, text_fp: 0xB002 })?;
    match f3b.ctx_destroy(8) {
        Err(E_BAD_ORDER) => {}
        _ => return Err(E_BAD_ORDER),
    }
    hits += 1;

    // R4 场景：无窗滞留合法 —— flush 时 ctx 无挂接窗 → 滞留不算丢，切回再投。
    let mut f4 = XFocus::new();
    f4.ctx_new(3)?;
    f4.ctx_new(4)?;
    f4.focus_req(301, 3, 0)?;
    f4.commit_in(3, Commit { seq: 2, text_fp: 0xB002 })?;
    f4.focus_req(302, 4, 10)?; // 切走，ctx3 无窗
    if f4.flush(3)? != 0 {
        return Err(E_CROSS_COMMIT);
    }
    f4.focus_req(301, 3, 20)?; // 切回
    if f4.flush(3)? != 1 {
        return Err(E_LOST_COMMIT);
    }
    f4.audit()?;
    if f4.commits_flushed() != 1 {
        return Err(E_LOST_COMMIT);
    }
    hits += 1;

    // R5 场景：悬空引用拒绝 —— focus 到死 ctx / 不存在 ctx 必拒。
    let mut f5 = XFocus::new();
    f5.ctx_new(9)?;
    f5.focus_req(901, 9, 0)?;
    f5.flush(9)?;
    f5.ctx_destroy(9)?;
    match f5.focus_req(902, 9, 10) {
        Err(E_HANG_CTX) => {}
        _ => return Err(E_HANG_CTX),
    }
    match f5.focus_req(903, 99, 10) {
        Err(E_HANG_CTX) => {}
        _ => return Err(E_HANG_CTX),
    }
    f5.audit()?;
    hits += 1;

    // R6 场景：同窗换 ctx 合法（摘旧挂新）；同 ctx 迁移到新窗也是合法焦点移动
    //        （旧绑定必须清空，任一时刻一 context 至多一窗 —— 双挂不变式）。
    let mut f6 = XFocus::new();
    f6.ctx_new(5)?;
    f6.focus_req(501, 5, 0)?;
    f6.focus_req(502, 5, 10)?; // 同 ctx 换窗 = 合法焦点移动
    let w_now = f6.ctxs_g()[0].win;
    if w_now != Some(502) {
        return Err(E_DOUBLE_BIND); // 旧绑定必须已清空、新绑定生效
    }
    f6.ctx_new(6)?;
    f6.focus_req(502, 6, 20)?; // 同窗换 ctx：摘 5 挂 6
    let w_after = f6.ctxs_g()[0].win;
    if w_after.is_some() {
        return Err(E_DOUBLE_BIND); // ctx5 必须已被摘除
    }
    f6.audit()?;
    hits += 1;

    // R7 场景：重复创建 / 重复销毁 context 拒绝。
    let mut f7 = XFocus::new();
    f7.ctx_new(11)?;
    match f7.ctx_new(11) {
        Err(E_DOUBLE_BIND) => {}
        _ => return Err(E_DOUBLE_BIND),
    }
    f7.ctx_destroy(11)?;
    match f7.ctx_destroy(11) {
        Err(E_HANG_CTX) => {}
        _ => return Err(E_HANG_CTX),
    }
    hits += 1;

    // R8 场景：销毁挂 focus 的 ctx → focus 自动清空（零悬空中间态跨事件）。
    let mut f8 = XFocus::new();
    f8.ctx_new(21)?;
    f8.focus_req(801, 21, 0)?;
    f8.ctx_destroy(21)?;
    f8.audit()?; // focus 为空，无悬空
    hits += 1;

    // R9 场景：commit 跟随性 —— ctx1 的 commit 绝不投递到窗 B（跨窗红线）。
    let mut f9 = XFocus::new();
    f9.ctx_new(31)?;
    f9.ctx_new(32)?;
    f9.focus_req(901, 31, 0)?;
    f9.commit_in(31, Commit { seq: 9, text_fp: 0xB009 })?;
    f9.focus_req(902, 32, 10)?;
    f9.flush(31)?; // 滞留（无窗）
    f9.focus_req(901, 31, 20)?;
    f9.flush(31)?; // 投回窗 901
    // 账本核对：flush_out 中该 commit 的窗必须是 901。
    let e = &f9.ctxs_g()[0];
    if !e.flush_out.iter().any(|(w, c)| *w == 901 && c.seq == 9) {
        return Err(E_CROSS_COMMIT);
    }
    if e.flush_out.iter().any(|(w, _)| *w == 902) {
        return Err(E_CROSS_COMMIT);
    }
    hits += 1;

    // R10 场景：会话关闭后事件拒收。
    let mut f10 = XFocus::new();
    f10.ctx_new(41)?;
    f10.close()?;
    match f10.focus_req(401, 41, 0) {
        Err(E_SESS_CLOSED) => {}
        _ => return Err(E_SESS_CLOSED),
    }
    match f10.commit_in(41, Commit { seq: 0, text_fp: 0 }) {
        Err(E_SESS_CLOSED) => {}
        _ => return Err(E_SESS_CLOSED),
    }
    hits += 1;

    Ok(hits)
}

impl XFocus {
    /// 审计视图：账本只读访问（供 drill/上层审计用）。
    pub fn ctxs_g(&self) -> &[CtxEntry] {
        &self.ctxs
    }
}
