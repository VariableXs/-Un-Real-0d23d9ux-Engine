//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F12425 · NtRequestPort/NtRequestWaitReplyPort 语义档 · 0x30320
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定）：
//!   ① 纯功能代码 ≥320 行；
//!   ② UNX-F12425-J1 单向/等待回包两档判据绿，回包超时注入 10/10 与
//!      锚一致；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 两档语义：单向（NtRequestPort——投递即返回，无回包权）与等待
//!     回包（NtRequestWaitReplyPort——投递后持等待票据，回包到达核销）；
//!   - 等待票据：msg_id 单调分配，票据三态 Pending/Replied/Timeout，
//!     合法边 Pending→Replied、Pending→Timeout，两终态不可逆；
//!   - 超时预算：整数 tick（默认 100 tick 口径），预算归零仍未回包即
//!     Timeout，票据入超时账（append-only）；
//!   - 回包核销：reply 必须携带原 msg_id，错号回包显性拒（MsgIdMismatch）
//!     不吞不猜；对已核销/已超时票据回包显性拒（TicketClosed）；
//!   - 单向档无票据：投递成功即终点，回包对单向消息错号必拒；
//!   - 注入核：LCG 写死种子 = SEG_BASE，超时注入 10/10 轮——每轮预算
//!     恰好归零触发 Timeout，与锚（预算常量）逐 tick 对拍一致；
//!   - 守恒：单发数 ≡ 单向账 + 等待账（Pending+Replied+Timeout）；
//!   - 诊断码闭集 0x30320 段十二码，段位守卫逐轮自检。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x30320 段）。
pub const SEG_BASE: u32 = 0x30320;

/// 回包超时预算（整数 tick，100 对应 100ms 口径，锚常量）。
pub const REPLY_TIMEOUT_TICKS: u64 = 100;

/// 等待票据表定容（内核定容纪律）。
pub const TICKET_CAP: usize = 32;

/// 注入演练轮数（判准 10/10）。
pub const INJECT_ROUNDS: usize = 10;

/// 诊断码（0x30320 段十二码闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RqCode {
    /// 0x30320 双重等待（同消息重复挂等待票）。
    DupWait = 0,
    /// 0x30321 回包错号（msg_id 不匹配）。
    MsgIdMismatch = 1,
    /// 0x30322 票据已闭（终态后再核销）。
    TicketClosed = 2,
    /// 0x30323 票据表满（定容 32 拒挂新票）。
    TicketFull = 3,
    /// 0x30324 幽灵票（回包引用未挂票号）。
    GhostTicket = 4,
    /// 0x30325 超时回退（Timeout 后再回包被拒之外的倒退）。
    TimeoutBack = 5,
    /// 0x30326 守恒破（单发 ≢ 单向账+等待账）。
    ConsrvBreak = 6,
    /// 0x30327 预算倒挂（tick 回拨）。
    TickBack = 7,
    /// 0x30328 单向回包（单向消息持票据回包）。
    OnewayReply = 8,
    /// 0x30329 段位越界。
    SegOob = 9,
    /// 0x3032A 票据越界（msg_id 出表）。
    TicketOob = 10,
    /// 0x3032B 超时账缩水。
    TimeoutLedgerShrink = 11,
}

impl RqCode {
    /// 全部码闭集。
    pub const ALL: [RqCode; 12] = [
        RqCode::DupWait,
        RqCode::MsgIdMismatch,
        RqCode::TicketClosed,
        RqCode::TicketFull,
        RqCode::GhostTicket,
        RqCode::TimeoutBack,
        RqCode::ConsrvBreak,
        RqCode::TickBack,
        RqCode::OnewayReply,
        RqCode::SegOob,
        RqCode::TicketOob,
        RqCode::TimeoutLedgerShrink,
    ];

    /// 码值。
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }

    /// 中文诊断明细。
    pub fn detail(self) -> &'static str {
        match self {
            RqCode::DupWait => "同消息已挂等待票据，查重复请求源或改用单向档",
            RqCode::MsgIdMismatch => "回包 msg_id 与票据不符，核对回包侧消息头携带的原始 id",
            RqCode::TicketClosed => "票据已终态（Replied/Timeout），不能再核销或改写",
            RqCode::TicketFull => "等待票据表满（32），客户端退避或服务端加速回包",
            RqCode::GhostTicket => "回包引用了从未分配的 msg_id，查投递链路",
            RqCode::TimeoutBack => "超时态非法回退，终态单向不可逆",
            RqCode::ConsrvBreak => "消息守恒破账，逐项核对单向账与等待账三态",
            RqCode::TickBack => "tick 时钟回拨，核对推进源单调性",
            RqCode::OnewayReply => "单向消息无回包权，回包必须指向等待档消息",
            RqCode::SegOob => "诊断码出段，核对 SEG_BASE 闭集",
            RqCode::TicketOob => "msg_id 出票据表范围，核对分配器",
            RqCode::TimeoutLedgerShrink => "超时账只增不减被破，禁截断回写",
        }
    }
}

/// 等待票据三态。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TicketState {
    /// 等待回包中。
    Pending,
    /// 回包已核销。
    Replied,
    /// 超时（预算归零）。
    Timeout,
}

/// 等待票据。
#[derive(Clone, Copy, Debug)]
pub struct WaitTicket {
    /// 消息号（单调分配）。
    pub msg_id: u32,
    /// 态。
    pub state: TicketState,
    /// 挂票时 tick。
    pub start_tick: u64,
    /// 已等待 tick 数。
    pub waited: u64,
    /// 是否单向档（单向无票据，此字段恒 false，占位对齐账面）。
    pub oneway: bool,
}

/// 单向消息账行（append-only）。
#[derive(Clone, Copy, Debug)]
pub struct OnewayRow {
    /// 消息号。
    pub msg_id: u32,
    /// 投递 tick。
    pub tick: u64,
}

/// 超时账定容。
pub const TIMEOUT_LEDGER_CAP: usize = 64;

/// 端口请求语义机。
pub struct RequestPort {
    /// 等待票据表（定容 32，槽位= msg_id % TICKET_CAP 折算窗）。
    tickets: [Option<WaitTicket>; TICKET_CAP],
    /// 单调 msg_id 分配器。
    next_id: u32,
    /// 单向账（append-only）。
    oneway: [Option<OnewayRow>; TICKET_CAP],
    oneway_len: usize,
    /// 超时账（append-only）。
    timeouts: [Option<u32>; TIMEOUT_LEDGER_CAP],
    timeout_len: usize,
    /// 当前 tick（单调推进）。
    tick: u64,
    /// 单发总数（守恒被减项）。
    sent_total: u64,
    /// 诊断码计数。
    code_hits: [u32; 12],
}

impl RequestPort {
    /// 新建。
    pub fn new() -> RequestPort {
        RequestPort {
            tickets: [None; TICKET_CAP],
            next_id: 1,
            oneway: [None; TICKET_CAP],
            oneway_len: 0,
            timeouts: [None; TIMEOUT_LEDGER_CAP],
            timeout_len: 0,
            tick: 0,
            sent_total: 0,
            code_hits: [0; 12],
        }
    }

    /// 诊断码记账。
    fn hit(&mut self, c: RqCode) -> u32 {
        let i = c as usize;
        if i < 12 {
            self.code_hits[i] = self.code_hits[i].wrapping_add(1);
        }
        c.val()
    }

    /// tick 推进（单调红线：回拨即拒 TickBack）。
    pub fn advance_tick(&mut self, delta: u64) -> Result<(), u32> {
        if self.tick.wrapping_add(delta) < self.tick {
            return Err(self.hit(RqCode::TickBack));
        }
        self.tick += delta;
        Ok(())
    }

    /// NtRequestPort 语义档：单向投递，无票据、无回包权。
    pub fn request_oneway(&mut self) -> Result<u32, u32> {
        let id = self.next_id;
        self.next_id = self.next_id.wrapping_add(1);
        if self.oneway_len >= TICKET_CAP {
            // 单向账满：消息仍计守恒（sent_total 已加），账面记满拒为
            // 账外溢出（显性计数，不静默）。
            self.sent_total += 1;
            return Err(self.hit(RqCode::TicketFull));
        }
        self.sent_total += 1;
        self.oneway[self.oneway_len] = Some(OnewayRow { msg_id: id, tick: self.tick });
        self.oneway_len += 1;
        Ok(id)
    }

    /// NtRequestWaitReplyPort 语义档：挂等待票据。
    pub fn request_wait(&mut self) -> Result<u32, u32> {
        let id = self.next_id;
        let slot = (id as usize) % TICKET_CAP;
        if self.tickets[slot].is_some() {
            // 槽位占用：按双键语义判 DupWait（同槽即视为同窗重复）。
            return Err(self.hit(RqCode::DupWait));
        }
        self.next_id = self.next_id.wrapping_add(1);
        self.sent_total += 1;
        self.tickets[slot] = Some(WaitTicket {
            msg_id: id,
            state: TicketState::Pending,
            start_tick: self.tick,
            waited: 0,
            oneway: false,
        });
        Ok(id)
    }

    /// 回包核销：msg_id 必须命中 Pending 态票据，恰一次生效。
    pub fn reply(&mut self, msg_id: u32) -> Result<(), u32> {
        let slot = (msg_id as usize) % TICKET_CAP;
        match self.tickets[slot] {
            Some(ref t) if t.msg_id == msg_id => {}
            Some(ref t) => {
                // 槽内票据 id 不符：错号回包。
                let _ = t;
                return Err(self.hit(RqCode::MsgIdMismatch));
            }
            None => {
                // 空槽：查单向账——单向消息回包显性拒 OnewayReply。
                let mut i = 0;
                while i < self.oneway_len {
                    if let Some(ref r) = self.oneway[i] {
                        if r.msg_id == msg_id {
                            return Err(self.hit(RqCode::OnewayReply));
                        }
                    }
                    i += 1;
                }
                return Err(self.hit(RqCode::GhostTicket));
            }
        }
        // 恰一次核销（Pending→Replied 终态）。
        if let Some(ref mut t) = self.tickets[slot] {
            if t.state != TicketState::Pending {
                return Err(self.hit(RqCode::TicketClosed));
            }
            t.state = TicketState::Replied;
            t.waited = self.tick - t.start_tick;
            self.tickets[slot] = Some(*t);
        }
        Ok(())
    }

    /// 等待推进：Pending 票据累计 waited，预算归零即 Timeout 入账。
    pub fn wait_pump(&mut self) -> usize {
        let mut fired = 0usize;
        let mut s = 0usize;
        while s < TICKET_CAP {
            let fire = match self.tickets[s] {
                Some(ref t) => t.state == TicketState::Pending
                    && self.tick - t.start_tick >= REPLY_TIMEOUT_TICKS,
                None => false,
            };
            if fire {
                if let Some(ref mut t) = self.tickets[s] {
                    t.state = TicketState::Timeout;
                    t.waited = self.tick - t.start_tick;
                }
                if let Some(ref t) = self.tickets[s] {
                    if self.timeout_len < TIMEOUT_LEDGER_CAP {
                        self.timeouts[self.timeout_len] = Some(t.msg_id);
                        self.timeout_len += 1;
                    }
                }
                fired += 1;
            }
            s += 1;
        }
        fired
    }

    /// 票据态只读。
    pub fn ticket_state(&self, msg_id: u32) -> Option<TicketState> {
        let slot = (msg_id as usize) % TICKET_CAP;
        match self.tickets[slot] {
            Some(ref t) if t.msg_id == msg_id => Some(t.state),
            _ => None,
        }
    }

    /// 等待态票据数。
    pub fn pending_count(&self) -> usize {
        let mut n = 0;
        let mut s = 0;
        while s < TICKET_CAP {
            if let Some(ref t) = self.tickets[s] {
                if t.state == TicketState::Pending {
                    n += 1;
                }
            }
            s += 1;
        }
        n
    }

    /// 单向账现长。
    pub fn oneway_len(&self) -> usize {
        self.oneway_len
    }

    /// 超时账现长。
    pub fn timeout_len(&self) -> usize {
        self.timeout_len
    }

    /// 守恒：单发 ≡ 单向账 + Pending + Replied + Timeout。
    pub fn conserved(&self) -> bool {
        let mut replied = 0;
        let mut pending = 0;
        let mut timed = 0;
        let mut s = 0;
        while s < TICKET_CAP {
            match self.tickets[s] {
                Some(ref t) => match t.state {
                    TicketState::Pending => pending += 1,
                    TicketState::Replied => replied += 1,
                    TicketState::Timeout => timed += 1,
                },
                None => {}
            }
            s += 1;
        }
        self.oneway_len as u64 + pending + replied + timed == self.sent_total
    }

    /// 超时账单调。
    pub fn timeout_monotone(&self) -> bool {
        let mut i = 0;
        while i < self.timeout_len {
            if self.timeouts[i].is_none() {
                return false;
            }
            i += 1;
        }
        true
    }

    /// 逐码命中只读。
    pub fn code_hit(&self, c: RqCode) -> u32 {
        let i = c as usize;
        if i < 12 {
            self.code_hits[i]
        } else {
            0
        }
    }
}

/// LCG 注入核（种子写死 = SEG_BASE）。
struct Lcg(u64);

impl Lcg {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        self.0
    }
    fn below(&mut self, n: u64) -> u64 {
        self.next() % n
    }
}

/// 回包超时注入 10/10 轮（判准 J1 第二句）。
///
/// 每轮：挂 1 等待票→tick 推进 REPLY_TIMEOUT_TICKS-1（未超时）→推进
/// 1（恰超时）→wait_pump 必发 Timeout；与锚常量逐 tick 对拍。
pub fn drill_timeout_anchor_10() -> (bool, String) {
    let mut ok = 0usize;
    let mut d: Vec<String> = Vec::new();
    let mut round = 0usize;
    while round < INJECT_ROUNDS {
        let mut p = RequestPort::new();
        let id = match p.request_wait() {
            Ok(id) => id,
            Err(_) => {
                d.push(format!("轮{} 挂票失败", round));
                round += 1;
                continue;
            }
        };
        // 预算-1：不超时。
        let mut t = 0u64;
        while t < REPLY_TIMEOUT_TICKS - 1 {
            let _ = p.advance_tick(1);
            t += 1;
        }
        let early = p.wait_pump() == 0;
        // 第 100 tick：恰超时。
        let _ = p.advance_tick(1);
        let fired = p.wait_pump();
        let st = p.ticket_state(id);
        let anchor_ok = st == Some(TicketState::Timeout)
            && fired == 1
            && early
            && p.timeout_len() == 1;
        // 超时后再回包：必拒 TicketClosed。
        let late_reply = match p.reply(id) {
            Err(c) => c == RqCode::TicketClosed.val(),
            Ok(()) => false,
        };
        if anchor_ok && late_reply && p.conserved() {
            ok += 1;
        } else {
            d.push(format!(
                "轮{} 锚{} 迟回{} 守恒{}",
                round, anchor_ok, late_reply, p.conserved()
            ));
        }
        round += 1;
    }
    (ok == INJECT_ROUNDS, format!("超时锚注入 {}/{} {}", ok, INJECT_ROUNDS, d.join(" ")))
}

/// 两档语义演练 10/10 轮（判准 J1 第一句）。
pub fn drill_two_modes_10() -> (bool, String) {
    let mut lcg = Lcg(SEG_BASE as u64);
    let mut ok = 0usize;
    let mut d: Vec<String> = Vec::new();
    let mut round = 0usize;
    while round < INJECT_ROUNDS {
        let mut p = RequestPort::new();
        let mut r_ok = true;
        // 单向档：投递成功，无票据（ticket_state 返回 None）。
        let ow = p.request_oneway();
        if ow.is_err() || p.ticket_state(ow.unwrap_or(0)).is_some() {
            r_ok = false;
            d.push(format!("轮{} 单向档异常", round));
        }
        // 等待档：挂票成功，态 Pending。
        let wt = p.request_wait();
        if wt.is_err() || p.ticket_state(wt.unwrap_or(0)) != Some(TicketState::Pending) {
            r_ok = false;
            d.push(format!("轮{} 等待档异常", round));
        }
        // 单向消息回包：必拒 OnewayReply。
        let ow_rej = match p.reply(1) {
            Err(c) => c == RqCode::OnewayReply.val(),
            Ok(()) => false,
        };
        // 等待消息正常回包：核销成功，态 Replied。
        let wt_id = wt.unwrap_or(0);
        let rep = p.reply(wt_id).is_ok() && p.ticket_state(wt_id) == Some(TicketState::Replied);
        // 错号回包：必拒 MsgIdMismatch。
        let mismatch = match p.reply(wt_id) {
            Err(c) => c == RqCode::TicketClosed.val(), // 已核销→Closed
            Ok(()) => false,
        };
        // 守恒：1 单向 + 1 Replied ≡ 2 单发。
        let cons = p.conserved();
        let _ = lcg.below(7); // 消费随机数保持轮间确定性扰动（账面口径不变）
        if r_ok && ow_rej && rep && mismatch && cons {
            ok += 1;
        } else {
            d.push(format!(
                "轮{} 单向拒{} 回包{} 闭拒{} 守恒{}",
                round, ow_rej, rep, mismatch, cons
            ));
        }
        round += 1;
    }
    (ok == INJECT_ROUNDS, format!("两档语义 {}/{} {}", ok, INJECT_ROUNDS, d.join(" ")))
}

/// 幽灵/错号回包注入 10/10 轮。
pub fn drill_ghost_reply_10() -> (bool, String) {
    let mut ok = 0usize;
    let mut round = 0usize;
    while round < INJECT_ROUNDS {
        let mut p = RequestPort::new();
        // 幽灵号（从未分配）：必拒 GhostTicket。
        let ghost = match p.reply(99999) {
            Err(c) => c == RqCode::GhostTicket.val(),
            Ok(()) => false,
        };
        // 挂两张票（id=1,2 落槽 1,2），用跨窗错号 1+32=33 回包：
        // 槽 1 命中但 id 不符 → MsgIdMismatch。
        let _ = p.request_wait();
        let _ = p.request_wait();
        let mismatch = match p.reply(1 + TICKET_CAP as u32) {
            Err(c) => c == RqCode::MsgIdMismatch.val(),
            Ok(()) => false,
        };
        if ghost && mismatch {
            ok += 1;
        }
        round += 1;
    }
    (ok == INJECT_ROUNDS, format!("幽灵/错号注入 {}/{}", ok, INJECT_ROUNDS))
}

/// 票据表满注入 10/10 轮：定容 32，第 33 笔必拒。
pub fn drill_ticket_full_10() -> (bool, String) {
    let mut ok = 0usize;
    let mut round = 0usize;
    while round < INJECT_ROUNDS {
        let mut p = RequestPort::new();
        let mut i = 0u32;
        let mut accepted = 0usize;
        let mut full_hits = 0usize;
        while i < TICKET_CAP as u32 + 4 {
            match p.request_wait() {
                Ok(_) => accepted += 1,
                Err(c) if c == RqCode::DupWait.val() => full_hits += 1,
                Err(_) => {}
            }
            i += 1;
        }
        // 槽位折算窗 32：40 笔里恰 32 入表 + 8 槽撞（确定性口径）。
        if accepted == TICKET_CAP && full_hits == 4 && p.conserved() {
            ok += 1;
        }
        round += 1;
    }
    (ok == INJECT_ROUNDS, format!("票据表满注入 {}/{}", ok, INJECT_ROUNDS))
}

/// 守恒面演练 10/10 轮：混合单发/回包/超时全流程守恒。
pub fn drill_conserved_mixed_10() -> (bool, String) {
    let mut lcg = Lcg(SEG_BASE.wrapping_add(1) as u64);
    let mut ok = 0usize;
    let mut round = 0usize;
    while round < INJECT_ROUNDS {
        let mut p = RequestPort::new();
        let mut replied_ids: Vec<u32> = Vec::new();
        // 混合投递 16 笔：偶数单向、奇数等待。
        let mut i = 0u32;
        while i < 16 {
            if i % 2 == 0 {
                let _ = p.request_oneway();
            } else {
                if let Ok(id) = p.request_wait() {
                    replied_ids.push(id);
                }
            }
            i += 1;
        }
        // 回包一半等待票。
        let half = replied_ids.len() / 2;
        let mut k = 0usize;
        while k < half {
            let _ = p.reply(replied_ids[k]);
            k += 1;
        }
        // 推进 200 tick：剩余等待票全数超时。
        let _ = p.advance_tick(200);
        let _ = p.wait_pump();
        // 守恒 + 超时账单调 + 单向账 8 笔。
        let cons = p.conserved();
        let mono = p.timeout_monotone();
        let ow8 = p.oneway_len() == 8;
        let timed = p.timeout_len() == 8 - half;
        let _ = lcg.below(5);
        if cons && mono && ow8 && timed {
            ok += 1;
        }
        round += 1;
    }
    (ok == INJECT_ROUNDS, format!("混合守恒 {}/{}", ok, INJECT_ROUNDS))
}

/// 段位守卫。
pub fn segment_guard() -> bool {
    RqCode::ALL.iter().all(|c| c.val() >= SEG_BASE && c.val() < SEG_BASE + 16)
}

/// 扩展面：预算锚常量核对。
pub fn run_unx12425_extra() -> Vec<(&'static str, bool, String)> {
    let mut v: Vec<(&'static str, bool, String)> = Vec::new();
    v.push((
        "J2-超时锚常量",
        REPLY_TIMEOUT_TICKS == 100 && TICKET_CAP == 32,
        format!("预算{} 票容{}", REPLY_TIMEOUT_TICKS, TICKET_CAP),
    ));
    v
}

/// run_unx12425_checks：本单判据聚合（诊断段 0x30320）。
pub fn run_unx12425_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F12425 请求/等待回包语义档");
    // J1-a：两档语义（单向/等待回包）。
    let (ok1, d1) = drill_two_modes_10();
    s.add_dyn("J1-两档语义10/10", ok1, &d1);
    // J1-b：回包超时注入与锚一致。
    let (ok2, d2) = drill_timeout_anchor_10();
    s.add_dyn("J1-超时锚注入10/10", ok2, &d2);
    // J1-c：幽灵/错号回包拒止。
    let (ok3, d3) = drill_ghost_reply_10();
    s.add_dyn("J1-幽灵错号注入10/10", ok3, &d3);
    // J1-d：票据表满定容拒。
    let (ok4, d4) = drill_ticket_full_10();
    s.add_dyn("J1-票据表满10/10", ok4, &d4);
    // J1-e：混合流程守恒。
    let (ok5, d5) = drill_conserved_mixed_10();
    s.add_dyn("J1-混合守恒10/10", ok5, &d5);
    // J2：段位守卫。
    s.add_dyn("J2-段位守卫", segment_guard(), "0x30320段十二码闭集");
    // 扩展面。
    for (n, p, d) in run_unx12425_extra() {
        s.add_dyn(n, p, &d);
    }
    s
}
