//! unx48228_swobs — UNX-F48228 切换状态机观测埋点（三态/抖动/直通四点）（诊断段 0xBD60）。
//!
//! 判据原文：
//!   J1 四点 100%；判据锚定 体验十三·补。
//!   J2 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计纪律：
//!   1. 四点定义（切换域口径）：一次切换会话由四个观测点完整覆盖——
//!      SW_BEGIN（会话开：离开中文态）、SW_SETTLE（落定：进入目标态）、
//!      SW_JITTER（抖动合并：窗内重按被吞）、SW_PASSTHRU（临时英文
//!      直通：临态内标点直通事件）。三态/抖动/直通三类语义四点全覆盖。
//!   2. 四点 100% 口径（J1）：每条会话账四点各 ≥1 且点序合法
//!      （BEGIN 最先、SETTLE 在 BEGIN 后、JITTER/PASSTHRU 只在临态
//!      存续期、会话闭账时四点齐），逐会话核销，缺一点即非全覆盖。
//!   3. 点序状态机：Idle → Begun → Settled → Closed；JITTER 仅在
//!      Begun/Settled 后可记；PASSTHRU 仅在临时英文会话可记；非法点
//!      拒收落违序账，不污染会话账。
//!   4. 埋点载荷：每点带 (session, tick, kind) 三元组，tick 单调；
//!      观测面只记事实不记内容（零拷贝红线），载荷即分类号。
//!   5. 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。

use crate::alloc::vec::Vec;

pub const SEG_BASE: u16 = 0xBD60;

// ── 诊断码（SEG_BASE+1 起，≤0x1F 个）──────────────────────
const E_ORDER_BAD: u16 = SEG_BASE + 1;   // 点序非法
const E_PARTIAL: u16 = SEG_BASE + 2;     // 会话四点缺一
const E_SESSION_BAD: u16 = SEG_BASE + 3; // 会话账缺失/重开
const E_TICK_BACK: u16 = SEG_BASE + 4;   // tick 回退
const E_COVER_MISS: u16 = SEG_BASE + 5;  // 四点 100% 未达
const E_LEDGER_BAD: u16 = SEG_BASE + 6;  // 账目不平
const E_EMPTY_SUM: u16 = SEG_BASE + 7;   // 写回摘要为空

/// 四个观测点。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SwObsPoint {
    SwBegin,    // 0：会话开
    SwSettle,   // 1：落定
    SwJitter,   // 2：抖动合并
    SwPassThru, // 3：临时英文直通
}

/// 会话内点序状态机。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ObsState {
    Idle,
    Begun,
    Settled,
}

/// 一条会话覆盖账。
#[derive(Clone, Copy, Debug)]
pub struct SessCover {
    pub session: u32,
    pub is_temp: bool, // 临时英文会话（PASSTHRU 必到）
    pub n_begin: u32,
    pub n_settle: u32,
    pub n_jitter: u32,
    pub n_passthru: u32,
    pub full: bool,
}

/// 观测埋点枢纽。
pub struct SwObs {
    pub state: ObsState,
    pub cur_session: u32,
    pub cur_is_temp: bool,
    pub last_tick: u64,
    covers: Vec<SessCover>,
    pub emit_total: u32,
    pub reject_total: u32,
}

impl SwObs {
    pub fn new() -> SwObs {
        SwObs {
            state: ObsState::Idle,
            cur_session: 0,
            cur_is_temp: false,
            last_tick: 0,
            covers: Vec::new(),
            emit_total: 0,
            reject_total: 0,
        }
    }

    /// SW_BEGIN：Idle → Begun，新会话入账。
    pub fn emit_begin(&mut self, session: u32, is_temp: bool, tick: u64) -> Result<(), u16> {
        if self.state != ObsState::Idle {
            return Err(E_ORDER_BAD);
        }
        if tick < self.last_tick {
            return Err(E_TICK_BACK);
        }
        // 会话号不得与未闭账会话重复。
        let mut i = 0usize;
        while i < self.covers.len() {
            if self.covers[i].session == session {
                return Err(E_SESSION_BAD);
            }
            i += 1;
        }
        self.state = ObsState::Begun;
        self.cur_session = session;
        self.cur_is_temp = is_temp;
        self.last_tick = tick;
        self.covers.push(SessCover {
            session,
            is_temp,
            n_begin: 1,
            n_settle: 0,
            n_jitter: 0,
            n_passthru: 0,
            full: false,
        });
        self.emit_total += 1;
        Ok(())
    }

    /// SW_SETTLE：Begun → Settled。
    pub fn emit_settle(&mut self, tick: u64) -> Result<(), u16> {
        if self.state != ObsState::Begun {
            return Err(E_ORDER_BAD);
        }
        if tick < self.last_tick {
            return Err(E_TICK_BACK);
        }
        self.state = ObsState::Settled;
        self.last_tick = tick;
        let sess = self.cur_session;
        self.bump(sess, 1)?;
        self.emit_total += 1;
        Ok(())
    }

    /// SW_JITTER：Begun/Settled 后可记，次数不限。
    pub fn emit_jitter(&mut self, tick: u64) -> Result<(), u16> {
        if self.state == ObsState::Idle {
            return Err(E_ORDER_BAD);
        }
        if tick < self.last_tick {
            return Err(E_TICK_BACK);
        }
        self.last_tick = tick;
        let sess = self.cur_session;
        self.bump(sess, 2)?;
        self.emit_total += 1;
        Ok(())
    }

    /// SW_PASSTHRU：仅临时英文会话可记（英文锁定会话无直通语义）。
    pub fn emit_passthru(&mut self, tick: u64) -> Result<(), u16> {
        if self.state == ObsState::Idle {
            return Err(E_ORDER_BAD);
        }
        if !self.cur_is_temp {
            return Err(E_ORDER_BAD);
        }
        if tick < self.last_tick {
            return Err(E_TICK_BACK);
        }
        self.last_tick = tick;
        let sess = self.cur_session;
        self.bump(sess, 3)?;
        self.emit_total += 1;
        Ok(())
    }

    fn bump(&mut self, session: u32, which: u8) -> Result<(), u16> {
        let mut i = self.covers.len();
        while i > 0 {
            i -= 1;
            if self.covers[i].session == session {
                match which {
                    1 => self.covers[i].n_settle += 1,
                    2 => self.covers[i].n_jitter += 1,
                    _ => self.covers[i].n_passthru += 1,
                }
                return Ok(());
            }
        }
        Err(E_SESSION_BAD)
    }

    /// 会话闭账：四点齐（BEGIN/SETTLE 各 ≥1、JITTER ≥1、临时会话
    /// PASSTHRU ≥1）才记 full；闭账回 Idle。
    pub fn close_session(&mut self) -> Result<bool, u16> {
        if self.state != ObsState::Settled {
            return Err(E_ORDER_BAD);
        }
        let sess = self.cur_session;
        let mut full = false;
        let mut i = self.covers.len();
        while i > 0 {
            i -= 1;
            if self.covers[i].session == sess {
                let c = &mut self.covers[i];
                let need_pass = if c.is_temp { c.n_passthru >= 1 } else { true };
                full = c.n_begin >= 1 && c.n_settle >= 1 && c.n_jitter >= 1 && need_pass;
                c.full = full;
                break;
            }
        }
        self.state = ObsState::Idle;
        self.cur_session = 0;
        self.cur_is_temp = false;
        Ok(full)
    }

    /// 违序拒收挂账（调用方在 emit 系列返 Err 时显式记）。
    pub fn note_reject(&mut self) {
        self.reject_total += 1;
    }

    /// 四点 100% 审计（J1）：全会话 full 全真。
    pub fn audit_full_coverage(&self) -> Result<(), u16> {
        let mut i = 0usize;
        while i < self.covers.len() {
            if !self.covers[i].full {
                return Err(E_PARTIAL);
            }
            i += 1;
        }
        Ok(())
    }

    /// ×N 会话覆盖核验（J1 硬口径）。
    pub fn coverage(&self, want: usize) -> Result<(), u16> {
        if self.covers.len() < want {
            return Err(E_COVER_MISS);
        }
        Ok(())
    }

    pub fn covers_len(&self) -> usize {
        self.covers.len()
    }

    /// 完成写回（判据 J2）。
    pub fn writeback(&self, done: bool, summary_len: u32) -> Result<(), u16> {
        if !done {
            return Err(E_ORDER_BAD);
        }
        if summary_len == 0 {
            return Err(E_EMPTY_SUM);
        }
        Ok(())
    }
}

/// 自例程判据：逐条落实 J1（四点 100%），返回命中数。
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;

    // R1 临时英文会话 ×10：BEGIN+SETTLE+2×JITTER+2×PASSTHRU 全齐，
    //    逐会话 full，四点 100%。
    let mut ob = SwObs::new();
    let mut s = 0u32;
    let mut tick = 0u64;
    while s < 10 {
        tick += 1;
        ob.emit_begin(s, true, tick)?;
        tick += 1;
        ob.emit_settle(tick)?;
        let mut k = 0u32;
        while k < 2 {
            tick += 1;
            ob.emit_jitter(tick)?;
            tick += 1;
            ob.emit_passthru(tick)?;
            k += 1;
        }
        tick += 1;
        let full = ob.close_session()?;
        if !full {
            return Err(E_PARTIAL);
        }
        s += 1;
    }
    ob.coverage(10)?;
    ob.audit_full_coverage()?;
    if ob.covers_len() != 10 {
        return Err(E_SESSION_BAD);
    }
    hits += 1;

    // R2 主扫 ×100：50 临时会话 + 50 锁定会话交替，四点口径按会话型
    //    分判（锁定会话免 PASSTHRU），100% 覆盖。
    let mut ob = SwObs::new();
    let mut s = 0u32;
    while s < 100 {
        let is_temp = s % 2 == 0;
        ob.emit_begin(s, is_temp, s as u64 * 10)?;
        ob.emit_settle(s as u64 * 10 + 1)?;
        ob.emit_jitter(s as u64 * 10 + 2)?;
        if is_temp {
            ob.emit_passthru(s as u64 * 10 + 3)?;
        }
        let full = ob.close_session()?;
        if !full {
            return Err(E_PARTIAL);
        }
        s += 1;
    }
    ob.coverage(100)?;
    ob.audit_full_coverage()?;
    hits += 1;

    // R3 点序违例×4：Idle 记 SETTLE/JITTER、重 BEGIN、未 SETTLE 闭账、
    //    闭账后记点——全拒并挂违序账。
    let mut ob = SwObs::new();
    if ob.emit_settle(1).is_ok() {
        return Err(E_ORDER_BAD);
    }
    ob.note_reject();
    if ob.emit_jitter(2).is_ok() {
        return Err(E_ORDER_BAD);
    }
    ob.note_reject();
    ob.emit_begin(1, false, 3)?;
    if ob.emit_begin(2, false, 4).is_ok() {
        return Err(E_ORDER_BAD);
    }
    ob.note_reject();
    ob.emit_settle(5)?;
    let full = ob.close_session()?; // 缺 JITTER：full=false 但闭账合法（如实记账）
    if full {
        return Err(E_PARTIAL);
    }
    if ob.close_session().is_ok() {
        return Err(E_ORDER_BAD); // 二次闭账拒
    }
    ob.note_reject();
    if ob.reject_total != 4 {
        return Err(E_LEDGER_BAD);
    }
    hits += 1;

    // R4 非临时会话记 PASSTHRU 拒收：直通语义只在临态，锁定会话越权。
    let mut ob = SwObs::new();
    ob.emit_begin(1, false, 1)?;
    ob.emit_settle(2)?;
    if ob.emit_passthru(3).is_ok() {
        return Err(E_ORDER_BAD);
    }
    ob.note_reject();
    ob.emit_jitter(4)?;
    let full = ob.close_session()?;
    if !full {
        return Err(E_PARTIAL); // 非临时会话：BEGIN+SETTLE+JITTER 即 full
    }
    if ob.reject_total != 1 {
        return Err(E_LEDGER_BAD);
    }
    hits += 1;

    // R5 会话号重复拒收：未闭账会话号重开必拒。
    let mut ob = SwObs::new();
    ob.emit_begin(7, true, 1)?;
    if ob.emit_begin(7, true, 2).is_ok() {
        return Err(E_SESSION_BAD);
    }
    ob.note_reject();
    hits += 1;

    // R6 tick 回退拒收：点时刻倒退必拒且不落账。
    let mut ob = SwObs::new();
    ob.emit_begin(1, true, 10)?;
    if ob.emit_settle(9).is_ok() {
        return Err(E_TICK_BACK);
    }
    ob.note_reject();
    ob.emit_settle(20)?;
    if ob.emit_jitter(15).is_ok() {
        return Err(E_TICK_BACK);
    }
    ob.note_reject();
    if ob.emit_total != 2 {
        return Err(E_LEDGER_BAD);
    }
    hits += 1;

    // R7 缺点会话不连坐：一条缺 PASSTHRU 的临时会话如实记 full=false，
    //    后续合规会话独立判 full（逐会话核销口径）。
    let mut ob = SwObs::new();
    ob.emit_begin(1, true, 1)?;
    ob.emit_settle(2)?;
    ob.emit_jitter(3)?;
    let full1 = ob.close_session()?;
    if full1 {
        return Err(E_PARTIAL); // 临时会话缺 PASSTHRU
    }
    ob.emit_begin(2, true, 4)?;
    ob.emit_settle(5)?;
    ob.emit_jitter(6)?;
    ob.emit_passthru(7)?;
    let full2 = ob.close_session()?;
    if !full2 {
        return Err(E_PARTIAL);
    }
    if ob.covers_len() != 2 {
        return Err(E_SESSION_BAD);
    }
    hits += 1;

    // R8 写回（判据 J2）：完成态 + 非空摘要放行，空摘要拒。
    let mut ob = SwObs::new();
    if ob.writeback(true, 0x10).is_ok() {
        return Err(E_ORDER_BAD); // 无会话不写回
    }
    ob.emit_begin(1, true, 1)?;
    ob.emit_settle(2)?;
    ob.emit_jitter(3)?;
    ob.emit_passthru(4)?;
    ob.close_session()?;
    if ob.writeback(true, 0).is_ok() {
        return Err(E_EMPTY_SUM);
    }
    ob.writeback(true, 0x40)?;
    hits += 1;

    Ok(hits)
}
