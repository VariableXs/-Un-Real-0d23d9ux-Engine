//! unx48216_xmulticli — UNX-F48216 Xwayland 并发多客户纪律（多 X 客户并发输入 context 隔离）（诊断段 0xBBE0）。
//!
//! 判据原文：
//!   J1 并发 ×10^3 零串扰；判据锚定 M5 母版。
//!   J2 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计纪律：
//!   1. 客户-语境模型：每个 X 客户持一个输入 context（icid、属主、焦点窗、
//!      预编辑缓冲、已提交量）；语境属主唯一，路由以 icid 为键。
//!   2. 路由纪律：注入/提交操作必须显式携带 (client, icid)；路由器核对
//!      icid 属主与 client 逐位相等，不等即记串扰并拒——串扰计数是
//!      纪律红线，×10^3 并发操作后必须恒 0。
//!   3. 值级隔离：每客户专属字母表（互不重叠的码位区间），预编辑缓冲
//!      只允许出现本客户字母表内的码位；即使路由失守，值级审计仍能
//!      抓住越界字符（双保险）。
//!   4. 守恒恒等式：每客户 已提交字数 + 预编辑现存字数 == 本客户被路由
//!      键数；全局 路由成功数 == Σ客户路由数，账目闭合可查。
//!   5. 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。

use crate::alloc::vec::Vec;

pub const SEG_BASE: u16 = 0xBBE0;

// ── 诊断码（SEG_BASE+1 起，≤0x1F 个）──────────────────────
const E_CROSS_TALK: u16 = SEG_BASE + 1; // 越属主路由（串扰）
const E_NO_IC: u16 = SEG_BASE + 2; // icid 不在册
const E_ALIEN_CHAR: u16 = SEG_BASE + 3; // 预编辑混入他客户字母表
const E_CONSERVE: u16 = SEG_BASE + 4; // 守恒恒等式破
const E_WIN_MAP: u16 = SEG_BASE + 5; // 焦点窗映射失守
const E_STATE_BAD: u16 = SEG_BASE + 6; // 路由器状态非法
const E_EMPTY_SUM: u16 = SEG_BASE + 7; // 写回摘要为空

/// 并发口径：8 客户 × 交织 1000 操作。
pub const CLIENT_N: u32 = 8;
pub const OPS_TOTAL: u32 = 1000;

/// 操作种类：注入一键（携带码位）/ 提交预编辑。
#[derive(Clone, Copy, Debug)]
pub enum Op {
    Key(u16),
    Commit,
}

/// 一个 X 客户的输入 context。
#[derive(Clone, Debug)]
pub struct InputContext {
    pub icid: u32,
    pub owner: u32,
    pub focus_win: u32,
    pub preedit: Vec<u16>,
    pub committed: u32,
    pub routed_keys: u32,
}

impl InputContext {
    /// 本客户专属字母表基址：码位区间 [base, base+64) 互不重叠。
    pub fn alphabet_base(owner: u32) -> u16 {
        0x4E00 + (owner as u16) * 64
    }
    pub fn in_alphabet(&self, ch: u16) -> bool {
        let base = Self::alphabet_base(self.owner);
        ch >= base && ch < base + 64
    }
}

/// 多客户路由枢纽。
pub struct Dispatcher {
    ics: Vec<InputContext>,
    win_map: Vec<(u32, u32)>, // (focus_win, icid)
    pub crosstalk: u32,
    pub routed: u32,
    pub rejected: u32,
    pub focus_registered: u32,
}

impl Dispatcher {
    pub fn new() -> Dispatcher {
        let mut ics = Vec::new();
        let mut c = 0u32;
        while c < CLIENT_N {
            ics.push(InputContext {
                icid: 0x1C00 + c,
                owner: c,
                focus_win: 0,
                preedit: Vec::new(),
                committed: 0,
                routed_keys: 0,
            });
            c += 1;
        }
        Dispatcher {
            ics,
            win_map: Vec::new(),
            crosstalk: 0,
            routed: 0,
            rejected: 0,
            focus_registered: 0,
        }
    }

    fn find_ic(&self, icid: u32) -> Option<usize> {
        let mut i = 0usize;
        while i < self.ics.len() {
            if self.ics[i].icid == icid {
                return Some(i);
            }
            i += 1;
        }
        None
    }

    pub fn ic_count(&self) -> usize {
        self.ics.len()
    }

    /// 焦点登记：窗口 → context 映射（每窗唯一归属）。
    pub fn focus(&mut self, client: u32, icid: u32, win: u32) -> Result<(), u16> {
        let idx = match self.find_ic(icid) {
            Some(i) => i,
            None => return Err(E_NO_IC),
        };
        if self.ics[idx].owner != client {
            self.crosstalk += 1;
            return Err(E_CROSS_TALK);
        }
        // 同窗易主（换绑）先摘旧绑。
        let mut kept: Vec<(u32, u32)> = Vec::new();
        let mut i = 0usize;
        while i < self.win_map.len() {
            if self.win_map[i].0 != win {
                kept.push(self.win_map[i]);
            }
            i += 1;
        }
        self.win_map = kept;
        self.win_map.push((win, icid));
        self.ics[idx].focus_win = win;
        self.focus_registered += 1;
        Ok(())
    }

    /// 按窗口路由（焦点路径）：win → icid → 属主核对 → 应用操作。
    pub fn route_by_window(&mut self, client: u32, win: u32, op: Op) -> Result<(), u16> {
        let mut found = None;
        let mut i = 0usize;
        while i < self.win_map.len() {
            if self.win_map[i].0 == win {
                found = Some(self.win_map[i].1);
                break;
            }
            i += 1;
        }
        match found {
            None => Err(E_WIN_MAP),
            Some(icid) => self.route(client, icid, op),
        }
    }

    /// 显式路由（J1 主体）：属主核对失败即串扰计数并拒；值级隔离双层校验。
    pub fn route(&mut self, client: u32, icid: u32, op: Op) -> Result<(), u16> {
        let idx = match self.find_ic(icid) {
            Some(i) => i,
            None => {
                self.rejected += 1;
                return Err(E_NO_IC);
            }
        };
        if self.ics[idx].owner != client {
            self.crosstalk += 1;
            self.rejected += 1;
            return Err(E_CROSS_TALK);
        }
        match op {
            Op::Key(ch) => {
                if !self.ics[idx].in_alphabet(ch) {
                    self.rejected += 1;
                    return Err(E_ALIEN_CHAR);
                }
                self.ics[idx].preedit.push(ch);
                self.ics[idx].routed_keys += 1;
            }
            Op::Commit => {
                self.ics[idx].committed += self.ics[idx].preedit.len() as u32;
                self.ics[idx].preedit.clear();
            }
        }
        self.routed += 1;
        Ok(())
    }

    /// 值级隔离审计：每客户预编辑只含本客户字母表码位。
    pub fn audit_alphabets(&self) -> Result<(), u16> {
        let mut i = 0usize;
        while i < self.ics.len() {
            let mut j = 0usize;
            while j < self.ics[i].preedit.len() {
                if !self.ics[i].in_alphabet(self.ics[i].preedit[j]) {
                    return Err(E_ALIEN_CHAR);
                }
                j += 1;
            }
            i += 1;
        }
        Ok(())
    }

    /// 守恒审计（纪律 4）：每客户 committed + preedit == routed_keys；
    /// ic 数闭包、焦点映射每窗唯一。
    pub fn audit_conserve(&self) -> Result<(), u16> {
        if self.ics.len() as u32 != CLIENT_N {
            return Err(E_STATE_BAD);
        }
        let mut i = 0usize;
        while i < self.ics.len() {
            let ic = &self.ics[i];
            if ic.committed + ic.preedit.len() as u32 != ic.routed_keys {
                return Err(E_CONSERVE);
            }
            i += 1;
        }
        let mut a = 0usize;
        while a < self.win_map.len() {
            let mut b = a + 1;
            while b < self.win_map.len() {
                if self.win_map[a].0 == self.win_map[b].0 {
                    return Err(E_WIN_MAP); // 一窗多绑
                }
                b += 1;
            }
            a += 1;
        }
        Ok(())
    }

    /// ×10^3 零串扰终判（J1 硬口径）。
    pub fn assert_no_crosstalk(&self, want_ops: u32) -> Result<(), u16> {
        if self.routed < want_ops {
            return Err(E_STATE_BAD);
        }
        if self.crosstalk != 0 {
            return Err(E_CROSS_TALK);
        }
        Ok(())
    }

    /// 完成写回（判据 J2）：零串扰闭合 + 非空摘要方放行。
    pub fn writeback(&self, done: bool, summary_len: u32) -> Result<(), u16> {
        if !done || self.crosstalk != 0 {
            return Err(E_STATE_BAD);
        }
        if summary_len == 0 {
            return Err(E_EMPTY_SUM);
        }
        Ok(())
    }
}

/// 自例程判据：逐条落实 J1（并发 ×10^3 零串扰），返回命中数。
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;

    // R1 主对齐 ×10^3：8 客户 round-robin 交织 1000 操作（每 5 操作一提交），
    //    全部路由成功、串扰恒 0。
    let mut dp = Dispatcher::new();
    let mut c = 0u32;
    while c < CLIENT_N {
        dp.focus(c, 0x1C00 + c, 0xF000 + c)?;
        c += 1;
    }
    if dp.focus_registered != CLIENT_N {
        return Err(E_WIN_MAP);
    }
    let mut i = 0u32;
    while i < OPS_TOTAL {
        let client = i % CLIENT_N;
        let icid = 0x1C00 + client;
        if i % 5 == 4 {
            dp.route(client, icid, Op::Commit)?;
        } else {
            let base = InputContext::alphabet_base(client);
            let ch = base + ((i / CLIENT_N) % 64) as u16;
            dp.route(client, icid, Op::Key(ch))?;
        }
        i += 1;
    }
    dp.assert_no_crosstalk(OPS_TOTAL)?;
    if dp.crosstalk != 0 || dp.rejected != 0 {
        return Err(E_CROSS_TALK);
    }
    dp.audit_alphabets()?;
    dp.audit_conserve()?;
    hits += 1;

    // R2 值级隔离终判：每客户预编辑码位全部落在专属区间（字母表闭包）。
    dp.audit_alphabets()?;
    hits += 1;

    // R3 守恒终判：committed + preedit == routed_keys 逐客户成立。
    dp.audit_conserve()?;
    hits += 1;

    // R4 串扰负例：客户 0 冒充路由客户 3 的 icid → 必拒（E_CROSS_TALK），
    //    串扰计数 +1，对方语境分毫不动。
    let mut dp2 = Dispatcher::new();
    if dp2.route(0, 0x1C00 + 3, Op::Key(0x4E03)).is_ok() {
        return Err(E_CROSS_TALK);
    }
    if dp2.crosstalk != 1 || dp2.rejected != 1 {
        return Err(E_STATE_BAD);
    }
    dp2.route(3, 0x1C00 + 3, Op::Key(0x4E00 + 64 * 3 + 7))?; // 客户 3 自身字母表内
    if dp2.routed != 1 {
        return Err(E_CONSERVE);
    }
    hits += 1;

    // R5 野 icid 拒收：不在册 icid → E_NO_IC，不计串扰只计拒。
    let mut dp3 = Dispatcher::new();
    if dp3.route(2, 0xDEAD, Op::Commit).is_ok() {
        return Err(E_NO_IC);
    }
    if dp3.crosstalk != 0 || dp3.rejected != 1 {
        return Err(E_STATE_BAD);
    }
    hits += 1;

    // R6 焦点路径隔离：两窗各绑其主；按窗路由只落属主语境，
    //    邻窗预编辑零波及（焦点隔离 = icid 隔离的窗口面）。
    let mut dp4 = Dispatcher::new();
    dp4.focus(0, 0x1C00, 0x2000)?;
    dp4.focus(1, 0x1C01, 0x2001)?;
    dp4.route_by_window(0, 0x2000, Op::Key(0x4E00 + 3))?;
    dp4.route_by_window(1, 0x2001, Op::Key(0x4E00 + 64 + 5))?;
    if dp4.route_by_window(1, 0x2000, Op::Commit).is_ok() {
        return Err(E_CROSS_TALK); // 客户 1 冒路由客户 0 的焦点窗
    }
    dp4.route_by_window(0, 0x2000, Op::Commit)?;
    dp4.audit_conserve()?;
    dp4.audit_alphabets()?;
    hits += 1;

    // R7 字母表越界拒收：属主核对过、但码位落在他人区间 → E_ALIEN_CHAR。
    let mut dp5 = Dispatcher::new();
    if dp5.route(4, 0x1C04, Op::Key(0x4E00)).is_ok() {
        return Err(E_ALIEN_CHAR); // 0x4E00 属客户 0 区间
    }
    if dp5.rejected != 1 || dp5.crosstalk != 0 {
        return Err(E_STATE_BAD);
    }
    hits += 1;

    // R8 写回（判据 J2）：串扰未清零不写回、空摘要拒、闭合+非空摘要放行。
    let mut dp6 = Dispatcher::new();
    if dp6.writeback(true, 0x10).is_ok() {
        return Err(E_STATE_BAD); // 零操作不得写回
    }
    dp6.route(1, 0x1C01, Op::Key(0x4E00 + 64))?;
    if dp6.writeback(true, 0).is_ok() {
        return Err(E_EMPTY_SUM);
    }
    dp6.writeback(true, 0x50)?;
    hits += 1;

    Ok(hits)
}
