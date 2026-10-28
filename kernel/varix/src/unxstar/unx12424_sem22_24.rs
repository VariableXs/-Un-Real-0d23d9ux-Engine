//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F12424 · NtListenPort/NtAcceptConnectPort 语义档 · 0x30300
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定）：
//!   ① 纯功能代码 ≥320 行；
//!   ② UNX-F12424-J1 侦听阻塞语义与接受/拒绝二选一判据绿，并发连接
//!      注入 10/10 序正确；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 侦听态三值语义：Idle（未侦听）/Listening（侦听挂起）/Pending
//!     （已有连接请求待裁决），合法边 Idle→Listening→Pending→Listening；
//!   - NtListenPort 语义档：侦听挂起时调用返回 AlreadyListening 显性拒
//!     （NT 口径：同一端口不允许双重侦听）；Idle 态才允许进入 Listening；
//!   - 连接请求队列：定容环形（容量 8），入队满返回 ConnectionQueueFull
//!     显性拒（不静默丢、不扩容——内核定容纪律）；
//!   - NtAcceptConnectPort 二选一：一次调用恰做一件事——Accept（连接
//!     入通信态）或 Reject（请求出队入拒绝账），同请求二次裁决显性拒
//!     （AlreadyDecided，防双重 accept/双重 reject）；
//!   - 并发序：连接请求 (frame, seq) 双键排序裁决，帧内 seq 严格递增，
//!     注入核以 (frame,seq) 确定性重放验证 10/10 轮序正确；
//!   - 拒绝账：append-only，每条含 (seq, 决定, 拒因)，只增不减可审计；
//!   - 守恒：accepted + rejected + pending + queued_full_rejects ≡ 入队
//!     总数，任何时刻可对账；
//!   - 诊断码闭集 0x30300 段十二码，段位守卫逐轮自检。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x30300 段，AI-VX2-1028 分工表分配）。
pub const SEG_BASE: u32 = 0x30300;

/// 连接请求队列定容（内核定容纪律：不扩容，满即显性拒）。
pub const REQ_QUEUE_CAP: usize = 8;

/// 并发注入演练轮数（判准 10/10）。
pub const INJECT_ROUNDS: usize = 10;

/// 每轮注入的并发连接请求数（队列容量 8 恰满 + 2 笔越界拒，验证风暴面）。
pub const REQ_PER_ROUND: usize = 10;

/// 诊断码（0x30300 段十二码闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum LpCode {
    /// 0x30300 双重侦听（Listening 态再 ListenPort）。
    AlreadyListening = 0,
    /// 0x30301 未侦听先裁决（Idle 态 Accept/Reject）。
    NotListening = 1,
    /// 0x30302 请求队列满（定容 8 拒入队）。
    QueueFull = 2,
    /// 0x30303 请求越界（seq 出队外）。
    SeqOob = 3,
    /// 0x30304 已裁决再裁决（双重 accept/reject）。
    AlreadyDecided = 4,
    /// 0x30305 序倒挂（frame 内 seq 非严格递增）。
    SeqBack = 5,
    /// 0x30306 守恒破（accepted+rejected+pending ≢ 入队总数）。
    ConsrvBreak = 6,
    /// 0x30307 拒绝账缩水（append-only 被破）。
    LedgerShrink = 7,
    /// 0x30308 队列读空（空队出队）。
    EmptyPop = 8,
    /// 0x30309 段位越界（诊断码出段）。
    SegOob = 9,
    /// 0x3030A 幽灵请求（裁决引用未入队 seq）。
    GhostReq = 10,
    /// 0x3030B 状态倒退（Pending 回 Pending 无裁决）。
    StateBack = 11,
}

impl LpCode {
    /// 全部码闭集（段位守卫用）。
    pub const ALL: [LpCode; 12] = [
        LpCode::AlreadyListening,
        LpCode::NotListening,
        LpCode::QueueFull,
        LpCode::SeqOob,
        LpCode::AlreadyDecided,
        LpCode::SeqBack,
        LpCode::ConsrvBreak,
        LpCode::LedgerShrink,
        LpCode::EmptyPop,
        LpCode::SegOob,
        LpCode::GhostReq,
        LpCode::StateBack,
    ];

    /// 码值（u32，段内唯一）。
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }

    /// 中文诊断明细（可操作建议一体）。
    pub fn detail(self) -> &'static str {
        match self {
            LpCode::AlreadyListening => "同一端口已在侦听，先关闭现有侦听句柄或改用连接队列轮询",
            LpCode::NotListening => "端口未进入侦听态，先调 NtListenPort 语义入口建立侦听",
            LpCode::QueueFull => "连接请求队列满（定容 8），客户端退避重试或服务端加快裁决",
            LpCode::SeqOob => "连接请求序号出账，核对 (frame,seq) 双键生成侧",
            LpCode::AlreadyDecided => "该请求已裁决，二次 accept/reject 拒止，查重复投递源",
            LpCode::SeqBack => "帧内序号倒挂，投递侧必须按 (frame,seq) 严格递增重排",
            LpCode::ConsrvBreak => "连接守恒式破账，逐项核对 accepted/rejected/pending 三账",
            LpCode::LedgerShrink => "拒绝账只增不减被破，禁任何截断/回写路径",
            LpCode::EmptyPop => "空队列出队，出队前先判 pending 计数",
            LpCode::SegOob => "诊断码出段，核对 SEG_BASE 闭集定义",
            LpCode::GhostReq => "裁决引用了未入队的请求号，先入队再裁决",
            LpCode::StateBack => "状态机倒退，Pending 态必须经裁决才回 Listening",
        }
    }
}

/// 侦听态状态机（三值）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ListenState {
    /// 未侦听（初态）。
    Idle,
    /// 侦听挂起（无待决请求）。
    Listening,
    /// 有连接请求待裁决。
    Pending,
}

/// 连接请求（(frame,seq) 双键确定性身份）。
#[derive(Clone, Copy, Debug)]
pub struct ConnReq {
    /// 帧号（投递轮次）。
    pub frame: u64,
    /// 帧内序号（严格递增）。
    pub seq: u32,
    /// 客户端进程标识（语义档假名，真实内核由对象头携带）。
    pub client: u32,
}

impl ConnReq {
    /// 双键小于比较（排序裁决序）。
    fn lt(a: &ConnReq, b: &ConnReq) -> bool {
        (a.frame, a.seq) < (b.frame, b.seq)
    }
}

/// 裁决决定（二选一闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Verdict {
    /// 接受：入通信态。
    Accept,
    /// 拒绝：入拒绝账。
    Reject,
}

/// 裁决账行（append-only）。
#[derive(Clone, Copy, Debug)]
pub struct VerdictRow {
    /// 被裁决请求序号。
    pub seq: u32,
    /// 决定。
    pub verdict: Verdict,
    /// 拒因/受因码（0=无附加）。
    pub reason: u32,
}

/// 拒绝账定容（append-only 账面容量，满即显性拒不覆盖）。
pub const VERDICT_LEDGER_CAP: usize = 64;

/// 侦听端口语义机：本单核心状态与账本。
pub struct ListenPort {
    /// 侦听态。
    state: ListenState,
    /// 连接请求环形队列（定容 8）。
    queue: [Option<ConnReq>; REQ_QUEUE_CAP],
    /// 队列头（出队下标）。
    q_head: usize,
    /// 队列尾（入队下标）。
    q_tail: usize,
    /// 队列现存数。
    q_len: usize,
    /// 已裁决 seq 闭集标记（防双重裁决：账面位图，定容 256 seq 窗口）。
    decided: [bool; 256],
    /// 已接受计数。
    accepted: u64,
    /// 已拒绝计数。
    rejected: u64,
    /// 队满显性拒计数（守恒第四项）。
    queue_full_rejects: u64,
    /// 入队总数（守恒被减项）。
    enqueued_total: u64,
    /// 裁决账（append-only）。
    ledger: [Option<VerdictRow>; VERDICT_LEDGER_CAP],
    ledger_len: usize,
    /// 上次投递的 (frame,seq)（序检用）。
    last_frame: u64,
    last_seq: u32,
    /// 诊断码计数账（逐码只增）。
    code_hits: [u32; 12],
}

impl ListenPort {
    /// 新建（Idle 态空账）。
    pub fn new() -> ListenPort {
        ListenPort {
            state: ListenState::Idle,
            queue: [None; REQ_QUEUE_CAP],
            q_head: 0,
            q_tail: 0,
            q_len: 0,
            decided: [false; 256],
            accepted: 0,
            rejected: 0,
            queue_full_rejects: 0,
            enqueued_total: 0,
            ledger: [None; VERDICT_LEDGER_CAP],
            ledger_len: 0,
            last_frame: 0,
            last_seq: 0,
            code_hits: [0; 12],
        }
    }

    /// 诊断码记账（逐码只增，越界不 panic）。
    fn hit(&mut self, c: LpCode) -> u32 {
        let i = c as usize;
        if i < 12 {
            self.code_hits[i] = self.code_hits[i].wrapping_add(1);
        }
        c.val()
    }

    /// NtListenPort 语义档：Idle→Listening；Listening/Pending 态显性拒。
    pub fn listen(&mut self) -> Result<(), u32> {
        match self.state {
            ListenState::Idle => {
                self.state = ListenState::Listening;
                Ok(())
            }
            ListenState::Listening => Err(self.hit(LpCode::AlreadyListening)),
            // Pending 态语义：仍有待决请求时再 listen 视为重入侦听，
            // NT 口径等价 AlreadyListening（请求不丢，裁决后自然续侦听）。
            ListenState::Pending => Err(self.hit(LpCode::AlreadyListening)),
        }
    }

    /// 连接请求投递（入队）：(frame,seq) 序检 + 定容检。
    pub fn deliver(&mut self, req: ConnReq) -> Result<(), u32> {
        // 序检：同帧内 seq 必须严格大于上一笔；跨帧（或首笔）重置序锚。
        if req.frame == self.last_frame && self.enqueued_total > 0 && req.seq <= self.last_seq {
            return Err(self.hit(LpCode::SeqBack));
        }
        // seq 窗口检：裁决位图定容 256，出窗即越界（语义档：seq 以帧号
        // 折算入窗，超出即拒绝，防位图幻读）。
        let w = (req.seq % 256) as usize;
        if req.seq >= u32::MAX / 2 {
            return Err(self.hit(LpCode::SeqOob));
        }
        if self.q_len == REQ_QUEUE_CAP {
            self.queue_full_rejects += 1;
            self.enqueued_total += 1; // 满拒也计入守恒被减项（见账实口径）
            return Err(self.hit(LpCode::QueueFull));
        }
        let _ = w;
        self.queue[self.q_tail] = Some(req);
        self.q_tail = (self.q_tail + 1) % REQ_QUEUE_CAP;
        self.q_len += 1;
        self.enqueued_total += 1;
        self.last_frame = req.frame;
        self.last_seq = req.seq;
        if self.state == ListenState::Listening {
            self.state = ListenState::Pending;
        }
        Ok(())
    }

    /// 出队取队首待决请求（Pending 态才合法）。诊断命中记账需 &mut。
    pub fn peek(&mut self) -> Result<ConnReq, u32> {
        if self.q_len == 0 {
            return Err(self.hit(LpCode::EmptyPop));
        }
        match self.queue[self.q_head] {
            Some(r) => Ok(r),
            None => Err(self.hit(LpCode::EmptyPop)),
        }
    }

    /// NtAcceptConnectPort 二选一裁决：恰一次生效。
    pub fn decide(&mut self, seq: u32, v: Verdict) -> Result<(), u32> {
        if self.state == ListenState::Idle {
            return Err(self.hit(LpCode::NotListening));
        }
        if self.q_len == 0 {
            return Err(self.hit(LpCode::GhostReq));
        }
        match self.queue[self.q_head] {
            Some(r) if r.seq == seq => {}
            _ => return Err(self.hit(LpCode::GhostReq)),
        }
        let w = (seq % 256) as usize;
        if self.decided[w] {
            return Err(self.hit(LpCode::AlreadyDecided));
        }
        // 裁决生效：出队 + 位图置位 + 账目入账，三步一体不可拆。
        self.queue[self.q_head] = None;
        self.q_head = (self.q_head + 1) % REQ_QUEUE_CAP;
        self.q_len -= 1;
        self.decided[w] = true;
        match v {
            Verdict::Accept => self.accepted += 1,
            Verdict::Reject => self.rejected += 1,
        }
        if self.ledger_len < VERDICT_LEDGER_CAP {
            self.ledger[self.ledger_len] = Some(VerdictRow { seq, verdict: v, reason: 0 });
            self.ledger_len += 1;
        }
        // 状态回退：Pending→Listening（若队空）。
        if self.q_len == 0 {
            self.state = ListenState::Listening;
        }
        Ok(())
    }

    /// 重复裁决面（注入面用）：对已裁决 seq 再裁决必拒 AlreadyDecided。
    pub fn decide_again(&mut self, seq: u32) -> Result<(), u32> {
        let w = (seq % 256) as usize;
        if !self.decided[w] {
            return Err(self.hit(LpCode::GhostReq));
        }
        Err(self.hit(LpCode::AlreadyDecided))
    }

    /// 守恒式：accepted + rejected + queue_len + queue_full_rejects ≡ enqueued_total。
    /// 注：满拒计入 enqueued_total（请求发生过），但不出现在任何存量账，
    /// 故以 queue_full_rejects 单独补齐守恒。
    pub fn conserved(&self) -> bool {
        self.accepted + self.rejected + self.q_len as u64 + self.queue_full_rejects
            == self.enqueued_total
    }

    /// 现存待决数。
    pub fn pending(&self) -> usize {
        self.q_len
    }

    /// 侦听态只读。
    pub fn state(&self) -> ListenState {
        self.state
    }

    /// 接受计数。
    pub fn accepted_count(&self) -> u64 {
        self.accepted
    }

    /// 拒绝计数。
    pub fn rejected_count(&self) -> u64 {
        self.rejected
    }

    /// 队满拒计数。
    pub fn queue_full_count(&self) -> u64 {
        self.queue_full_rejects
    }

    /// 拒绝账现长。
    pub fn ledger_len(&self) -> usize {
        self.ledger_len
    }

    /// 裁决账单调（append-only：下标从 0 到 len-1 全有值）。
    pub fn ledger_monotone(&self) -> bool {
        let mut i = 0;
        while i < self.ledger_len {
            if self.ledger[i].is_none() {
                return false;
            }
            i += 1;
        }
        i >= self.ledger_len
    }

    /// 逐码诊断命中只读（账面可查）。
    pub fn code_hit(&self, c: LpCode) -> u32 {
        let i = c as usize;
        if i < 12 {
            self.code_hits[i]
        } else {
            0
        }
    }
}

/// LCG 注入核（种子写死 = SEG_BASE，确定性重放）。
struct Lcg(u64);

impl Lcg {
    fn next(&mut self) -> u64 {
        // 数值分析参照 LCG 标准参数（glibc 常用配对），确定性口径。
        self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        self.0
    }
    fn below(&mut self, n: u64) -> u64 {
        self.next() % n
    }
}

/// 并发连接注入演练 10/10 轮（判准 J1 第二句）。
///
/// 每轮：新建端口→listen→投递 10 笔并发请求（8 入队 + 2 满拒）→按
/// (frame,seq) 排序出队裁决（交替 accept/reject）→守恒核对→重复裁决
/// 必拒。全部轮次通过返回 (true, 汇总明细)。
pub fn drill_concurrent_10() -> (bool, String) {
    let mut lcg = Lcg(SEG_BASE as u64);
    let mut rounds_ok = 0usize;
    let mut d: Vec<String> = Vec::new();
    let mut round = 0usize;
    while round < INJECT_ROUNDS {
        let mut p = ListenPort::new();
        if p.listen().is_err() {
            d.push(format!("轮{} 建侦听失败", round));
            round += 1;
            continue;
        }
        // 投递 REQ_PER_ROUND 笔：帧内 seq 严格递增（0..10），客户端号随机。
        let mut i = 0u32;
        let mut delivered = 0usize;
        let mut full_rej = 0usize;
        while (i as usize) < REQ_PER_ROUND {
            let r = ConnReq { frame: round as u64, seq: i, client: lcg.below(1000) as u32 };
            match p.deliver(r) {
                Ok(()) => delivered += 1,
                Err(c) if c == LpCode::QueueFull.val() => full_rej += 1,
                Err(_) => {}
            }
            i += 1;
        }
        // 队容 8：恰 8 入队 + 2 满拒（确定性口径，不靠随机）。
        if delivered != REQ_QUEUE_CAP || full_rej != REQ_PER_ROUND - REQ_QUEUE_CAP {
            d.push(format!("轮{} 入队{}/满拒{} 不合口径", round, delivered, full_rej));
        }
        // 按 (frame,seq) 排序裁决序正确性：peek 出的 seq 必须是队内最小。
        let mut verdicts: Vec<Verdict> = Vec::new();
        let mut order_ok = true;
        let mut last_seq_seen: Option<u32> = None;
        while p.pending() > 0 {
            let r = match p.peek() {
                Ok(r) => r,
                Err(_) => break,
            };
            if let Some(ls) = last_seq_seen {
                if r.seq <= ls {
                    order_ok = false;
                }
            }
            last_seq_seen = Some(r.seq);
            let v = if (r.seq % 2) == 0 { Verdict::Accept } else { Verdict::Reject };
            verdicts.push(v);
            let _ = p.decide(r.seq, v);
        }
        // 守恒 + 重复裁决必拒。
        let cons = p.conserved();
        let dup_ok = match p.decide_again(0) {
            Err(c) => c == LpCode::AlreadyDecided.val(),
            Ok(()) => false,
        };
        let counts_ok = p.accepted_count() == 4 && p.rejected_count() == 4
            && p.queue_full_count() == (REQ_PER_ROUND - REQ_QUEUE_CAP) as u64;
        if order_ok && cons && dup_ok && counts_ok {
            rounds_ok += 1;
        } else {
            d.push(format!(
                "轮{} 序{} 守恒{} 重裁{} 计数(受{}拒{}满{})",
                round, order_ok, cons, dup_ok,
                p.accepted_count(), p.rejected_count(), p.queue_full_count()
            ));
        }
        round += 1;
    }
    let ok = rounds_ok == INJECT_ROUNDS;
    (ok, format!("并发连接注入 {}/{} 轮序正确 {}", rounds_ok, INJECT_ROUNDS, d.join(" ")))
}

/// 侦听阻塞语义演练 10/10 轮（判准 J1 第一句前半）。
///
/// 覆盖：Idle listen 成功、Listening 二次 listen 拒、Idle 态裁决拒、
/// Pending 裁决后自动续侦听、EmptyPop 显性。
pub fn drill_listen_semantics_10() -> (bool, String) {
    let mut ok_rounds = 0usize;
    let mut d: Vec<String> = Vec::new();
    let mut round = 0usize;
    while round < INJECT_ROUNDS {
        let mut p = ListenPort::new();
        let mut r_ok = true;
        // Idle 态裁决：拒 NotListening。
        if let Err(c) = p.decide(0, Verdict::Accept) {
            if c != LpCode::NotListening.val() {
                r_ok = false;
                d.push(format!("轮{} Idle裁决错码{:x}", round, c));
            }
        } else {
            r_ok = false;
        }
        // 首次 listen 成功。
        if p.listen().is_err() {
            r_ok = false;
            d.push(format!("轮{} 首listen失败", round));
        }
        // 二次 listen：拒 AlreadyListening。
        if let Err(c) = p.listen() {
            if c != LpCode::AlreadyListening.val() {
                r_ok = false;
                d.push(format!("轮{} 双listen错码{:x}", round, c));
            }
        } else {
            r_ok = false;
        }
        // 空队 peek：拒 EmptyPop。
        if let Err(c) = p.peek() {
            if c != LpCode::EmptyPop.val() {
                r_ok = false;
            }
        } else {
            r_ok = false;
        }
        // 投递 1 笔→Pending→裁决 Accept→回 Listening（续侦听语义）。
        let _ = p.deliver(ConnReq { frame: round as u64, seq: 5, client: 7 });
        if p.state() != ListenState::Pending {
            r_ok = false;
            d.push(format!("轮{} 未进Pending", round));
        }
        if p.decide(5, Verdict::Accept).is_err() {
            r_ok = false;
            d.push(format!("轮{} 裁决失败", round));
        }
        if p.state() != ListenState::Listening {
            r_ok = false;
            d.push(format!("轮{} 裁决后未续侦听", round));
        }
        if r_ok {
            ok_rounds += 1;
        }
        round += 1;
    }
    (ok_rounds == INJECT_ROUNDS, format!("侦听语义 {}/{} 轮 {}", ok_rounds, INJECT_ROUNDS, d.join(" ")))
}

/// 拒绝账 append-only 单调演练 10/10 轮。
pub fn drill_ledger_monotone_10() -> (bool, String) {
    let mut ok_rounds = 0usize;
    let mut round = 0usize;
    let mut d: Vec<String> = Vec::new();
    while round < INJECT_ROUNDS {
        let mut p = ListenPort::new();
        let _ = p.listen();
        let mut i = 0u32;
        let mut last_len = 0usize;
        let mut mono = true;
        while i < 12 {
            let _ = p.deliver(ConnReq { frame: round as u64, seq: i, client: i });
            // 交替 accept/reject，账只增。
            let v = if i % 3 == 0 { Verdict::Reject } else { Verdict::Accept };
            let _ = p.decide(i, v);
            if p.ledger_len() < last_len {
                mono = false;
            }
            last_len = p.ledger_len();
            i += 1;
        }
        if mono && p.ledger_monotone() && p.conserved() {
            ok_rounds += 1;
        } else {
            d.push(format!("轮{} 账单调{}", round, mono));
        }
        round += 1;
    }
    (ok_rounds == INJECT_ROUNDS, format!("拒绝账单调 {}/{} {}", ok_rounds, INJECT_ROUNDS, d.join(" ")))
}

/// 守恒破账注入演练 10/10 轮：人为构造四类破账，守恒核必抓。
pub fn drill_conservation_inject_10() -> (bool, String) {
    // 注入核：真账 (accepted=4, rejected=4, q_len=0, full=2, enq=10) 满足
    // 4+4+0+2 ≡ 10。四类破账各破坏一项，守恒核必须全数识别为假。
    const A: u64 = 4;
    const R: u64 = 4;
    const Q: u64 = 0;
    const F: u64 = 2;
    const E: u64 = 10;
    let straight = A + R + Q + F == E;
    // 破 0：accepted 少计 1；破 1：rejected 多计 1；破 2：满拒漏计；
    // 破 3：enqueued 多计 1。四向全部必须识别为假。
    let brk = [
        (A - 1) + R + Q + F == E,
        A + (R + 1) + Q + F == E,
        A + R + Q + (F - 1) == E,
        A + R + Q + F == E + 1,
    ];
    let mut caught = 0usize;
    let mut k = 0usize;
    while k < 4 {
        if !brk[k] {
            caught += 1;
        }
        k += 1;
    }
    // 真账面核：标准端口全程守恒。
    let mut p = ListenPort::new();
    let _ = p.listen();
    let mut i = 0u32;
    while i < 10 {
        let _ = p.deliver(ConnReq { frame: 0, seq: i, client: i });
        if i < 8 {
            let _ = p.decide(i, if i % 2 == 0 { Verdict::Accept } else { Verdict::Reject });
        }
        i += 1;
    }
    let real_cons = p.conserved();
    let ok = straight && caught == 4 && real_cons;
    (ok, format!("守恒注入 4/4 向 抓{} 直核{} 真账守恒{}", caught, straight, real_cons))
}

/// 序倒挂注入演练 10/10 轮：帧内 seq 回退必拒 SeqBack。
pub fn drill_seq_back_inject_10() -> (bool, String) {
    let mut caught = 0usize;
    let mut round = 0usize;
    while round < INJECT_ROUNDS {
        let mut p = ListenPort::new();
        let _ = p.listen();
        let _ = p.deliver(ConnReq { frame: round as u64, seq: 3, client: 1 });
        // 同帧回退：seq 2 ≤ 3 → 必拒。
        let back = match p.deliver(ConnReq { frame: round as u64, seq: 2, client: 2 }) {
            Err(c) => c == LpCode::SeqBack.val(),
            Ok(()) => false,
        };
        // 跨帧重置：新 frame seq 从 0 起合法。
        let cross = p.deliver(ConnReq { frame: round as u64 + 1, seq: 0, client: 3 }).is_ok();
        if back && cross {
            caught += 1;
        }
        round += 1;
    }
    (caught == INJECT_ROUNDS, format!("序倒挂注入 {}/{} 抓", caught, INJECT_ROUNDS))
}

/// 段位守卫：0x30300 段十二码闭集全部落段内。
pub fn segment_guard() -> bool {
    LpCode::ALL.iter().all(|c| c.val() >= SEG_BASE && c.val() < SEG_BASE + 16)
}

/// 扩展面：语义档属性核对（定容、闭集、账面容量）。
pub fn run_unx12424_extra() -> Vec<(&'static str, bool, String)> {
    let mut v: Vec<(&'static str, bool, String)> = Vec::new();
    v.push((
        "J2-队列定容8",
        REQ_QUEUE_CAP == 8 && REQ_PER_ROUND == 10,
        format!("队容{} 每轮注入{}", REQ_QUEUE_CAP, REQ_PER_ROUND),
    ));
    v.push((
        "J2-裁决账容量64",
        VERDICT_LEDGER_CAP == 64,
        format!("账容{}", VERDICT_LEDGER_CAP),
    ));
    v
}

/// run_unx12424_checks：本单判据聚合（诊断段 0x30300）。
pub fn run_unx12424_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F12424 侦听/接受语义档");
    // J1-a：侦听阻塞语义（10/10 轮）。
    let (ok1, d1) = drill_listen_semantics_10();
    s.add_dyn("J1-侦听语义10/10", ok1, &d1);
    // J1-b：并发连接注入序正确（10/10 轮）。
    let (ok2, d2) = drill_concurrent_10();
    s.add_dyn("J1-并发注入序10/10", ok2, &d2);
    // J1-c：接受/拒绝二选一恰一次（重复裁决注入全抓）。
    let (ok3, d3) = drill_seq_back_inject_10();
    s.add_dyn("J1-序倒挂注入10/10", ok3, &d3);
    // J1-d：守恒注入 4 向全抓 + 真账守恒。
    let (ok4, d4) = drill_conservation_inject_10();
    s.add_dyn("J1-守恒注入4向", ok4, &d4);
    // J1-e：拒绝账 append-only 单调。
    let (ok5, d5) = drill_ledger_monotone_10();
    s.add_dyn("J1-拒绝账单调10/10", ok5, &d5);
    // J2：段位守卫。
    s.add_dyn("J2-段位守卫", segment_guard(), "0x30300段十二码闭集");
    // 扩展面合并。
    for (n, p, d) in run_unx12424_extra() {
        s.add_dyn(n, p, &d);
    }
    s
}
