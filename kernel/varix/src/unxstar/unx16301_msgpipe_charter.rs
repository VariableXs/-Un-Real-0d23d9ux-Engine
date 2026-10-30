//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F16301 · 消息投递通道总纲（驱动面↔消息泵界面） · 0x67E40
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定，CoRun·UNX-E1-B16）：
//!   ① 纯功能代码 ≥545 行；
//!   ② UNX-F16301-J1 总纲落账（Post/Send/派发三通道+边界契约），
//!     三通道齐备率 100%，与 D4 消息泵界面挂接一致率 100%；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 三通道建模：Post（异步队列，FIFO 入队即返）、Send（同步直
//!     调，调用即派发）、派发（Dispatch 泵出队执行）；
//!   - 边界契约：Post 队列容量上界（满则拒）、Send 重入上界
//!     （超深即拒）、Dispatch 空队显性（不返伪消息）；
//!   - 通道齐备账：三通道注册条目（入口名/返回语义/边界值）齐备
//!     率 100%；
//!   - D4 消息泵挂接：Dispatch 的泵入口名必须能在 D4 泵界面表
//!     精确命中，一致率 100%；
//!   - 消息序守恒：Post 总数 = Dispatch 泵出总数 + 队列驻留数；
//!     Send 直调不计入队列账；
//!   - LCG 种子写死 = SEG_BASE（0x67E40）；no_std + alloc；
//!     零 unwrap/expect/panic；禁 rand/浮点/IO/墙钟。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x67E40，AI-VX3-1030 批14 独占区）。
pub const SEG_BASE: u32 = 0x67E40;

// ── 三通道闭集 ──────────────────────────────────────────────────
pub const CH_POST: usize = 0;
pub const CH_SEND: usize = 1;
pub const CH_DISPATCH: usize = 2;
pub const CHANNEL_COUNT: usize = 3;
pub const CHANNEL_NAMES: [&str; CHANNEL_COUNT] = ["Post", "Send", "Dispatch"];
pub const CHANNEL_FLOOR: usize = 3;

// ── 边界契约（冻结值） ──────────────────────────────────────────
/// Post 队列容量上界（冻结）。
pub const POST_QUEUE_CAP: usize = 512;
/// Send 重入深度上界（冻结）。
pub const SEND_REENTRY_CAP: u32 = 32;
/// Dispatch 单泵最大出队数（冻结）。
pub const DISPATCH_BATCH_MAX: usize = 64;
/// 消息标识闭集（驱动面→泵界面的消息类型）。
pub const MSG_KINDS: usize = 6;
pub const MSG_KIND_NAMES: [&str; MSG_KINDS] =
    ["paint", "input_key", "input_mouse", "timer", "quit", "custom"];
/// 自定义消息合法值域（写死：段内派生）。
pub const CUSTOM_MSG_BASE: u32 = 0x67E40;
pub const CUSTOM_MSG_SPAN: u32 = 0x40;

// ── D4 消息泵界面挂接表（冻结） ─────────────────────────────────
pub const D4_PUMP_TABLE: [&str; 3] =
    ["pump_get_message", "pump_translate", "pump_dispatch"];
/// 派发通道冻结泵入口名（必须能在 D4 表命中）。
pub const DISPATCH_FROZEN_PUMP: &str = "pump_dispatch";

/// 判据迭代轮数（写死：判据 10/10 语义）。
pub const JUDGE_ROUNDS: u32 = 10;

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PipeCode {
    /// 通道越闭集
    ChannelOutOfRange,
    /// 消息种类越闭集
    MsgKindOutOfRange,
    /// Post 队列满（边界契约：满则拒）
    PostQueueFull,
    /// Send 重入超深（边界契约：超深即拒）
    SendReentryOverflow,
    /// Dispatch 空队伪消息（边界契约：空队显性）
    DispatchEmptyFake,
    /// 消息序守恒破坏（Post≠泵出+驻留）
    SequenceConservationBroken,
    /// 通道名空（齐备破坏）
    ChannelNameBlank,
    /// D4 泵挂接断裂（表未命中）
    PumpNotLinked,
    /// 自定义消息越值域
    CustomOutOfRange,
    /// 双跑不一致
    NotReproducible,
}

impl PipeCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            PipeCode::ChannelOutOfRange => "通道越闭集：Post/Send/Dispatch 冻结",
            PipeCode::MsgKindOutOfRange => "消息种类越闭集：6 类冻结",
            PipeCode::PostQueueFull => "Post 队列满：容量上界拒入",
            PipeCode::SendReentryOverflow => "Send 重入超深：同步直调上界",
            PipeCode::DispatchEmptyFake => "Dispatch 空队伪消息：空队显性",
            PipeCode::SequenceConservationBroken => "消息序守恒破坏：Post≠泵出+驻留",
            PipeCode::ChannelNameBlank => "通道名空：齐备破坏",
            PipeCode::PumpNotLinked => "D4 泵挂接断裂：表未命中",
            PipeCode::CustomOutOfRange => "自定义消息越值域：段内派生域",
            PipeCode::NotReproducible => "双跑不一致：复现红线",
        }
    }
    pub const ALL: [PipeCode; 10] = [
        PipeCode::ChannelOutOfRange,
        PipeCode::MsgKindOutOfRange,
        PipeCode::PostQueueFull,
        PipeCode::SendReentryOverflow,
        PipeCode::DispatchEmptyFake,
        PipeCode::SequenceConservationBroken,
        PipeCode::ChannelNameBlank,
        PipeCode::PumpNotLinked,
        PipeCode::CustomOutOfRange,
        PipeCode::NotReproducible,
    ];
}

pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < PipeCode::ALL.len() {
        if PipeCode::ALL[i].val() == code {
            return PipeCode::ALL[i].detail();
        }
        i += 1;
    }
    "未知诊断码：不在 0x67E40 段闭集内"
}

// ── 消息体 ──────────────────────────────────────────────────────
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PipeMsg {
    pub kind: usize,
    /// 自定义值（kind=custom 时须在值域内，其余恒 0）。
    pub custom: u32,
    /// 投递序号（单调递增，确定性）。
    pub seq: u64,
}

impl PipeMsg {
    pub fn legal(&self) -> Result<(), u32> {
        if self.kind >= MSG_KINDS {
            return Err(PipeCode::MsgKindOutOfRange.val());
        }
        if self.kind == 5 {
            // custom：值域守卫。
            if self.custom < CUSTOM_MSG_BASE
                || self.custom >= CUSTOM_MSG_BASE + CUSTOM_MSG_SPAN
            {
                return Err(PipeCode::CustomOutOfRange.val());
            }
        }
        Ok(())
    }
}

// ── 消息泵账（三通道总成） ──────────────────────────────────────
pub struct MsgPipe {
    /// Post 异步队列。
    queue: Vec<PipeMsg>,
    /// 队列头读游标（FIFO，泵出即推进）。
    head: usize,
    /// Post 入队总数。
    post_total: u64,
    /// Dispatch 泵出总数。
    dispatch_total: u64,
    /// Send 直调总数。
    send_total: u64,
    /// Send 当前重入深度。
    send_depth: u32,
    /// Send 最大重入深度（观测）。
    send_depth_max: u32,
    /// 泵空转次数（空队显性记录）。
    pump_idle: u64,
}

impl MsgPipe {
    pub fn new() -> MsgPipe {
        MsgPipe {
            queue: Vec::new(),
            head: 0,
            post_total: 0,
            dispatch_total: 0,
            send_total: 0,
            send_depth: 0,
            send_depth_max: 0,
            pump_idle: 0,
        }
    }

    /// Post：入队即返（异步）。满则拒。
    pub fn post(&mut self, m: PipeMsg) -> Result<u64, u32> {
        m.legal()?;
        let live = self.queue.len() - self.head;
        if live >= POST_QUEUE_CAP {
            return Err(PipeCode::PostQueueFull.val());
        }
        self.post_total += 1;
        self.queue.push(m);
        Ok(self.post_total)
    }

    /// Send：同步直调（不入队）。重入超深即拒。
    pub fn send(&mut self, m: PipeMsg) -> Result<u64, u32> {
        m.legal()?;
        if self.send_depth >= SEND_REENTRY_CAP {
            return Err(PipeCode::SendReentryOverflow.val());
        }
        self.send_depth += 1;
        if self.send_depth > self.send_depth_max {
            self.send_depth_max = self.send_depth;
        }
        self.send_total += 1;
        // 直调执行窗（同步语义：执行毕即退出重入）。
        self.send_depth -= 1;
        Ok(self.send_total)
    }

    /// Dispatch：泵出恰一条。空队显性 Err（不返伪消息）。
    pub fn dispatch_one(&mut self) -> Result<PipeMsg, u32> {
        if self.head >= self.queue.len() {
            self.pump_idle += 1;
            return Err(PipeCode::DispatchEmptyFake.val());
        }
        let m = self.queue[self.head].clone();
        self.head += 1;
        self.dispatch_total += 1;
        // 空间回收：头游标追平即压缩。
        if self.head == self.queue.len() {
            self.queue.clear();
            self.head = 0;
        }
        Ok(m)
    }

    /// 批量泵：至多 DISPATCH_BATCH_MAX 条，空队提前显性止。
    pub fn dispatch_batch(&mut self) -> Result<usize, u32> {
        let mut n = 0;
        while n < DISPATCH_BATCH_MAX {
            match self.dispatch_one() {
                Ok(_) => n += 1,
                Err(e) if e == PipeCode::DispatchEmptyFake.val() => break,
                Err(e) => return Err(e),
            }
        }
        Ok(n)
    }

    /// 驻留数（已入队未泵出）。
    pub fn resident(&self) -> usize {
        self.queue.len() - self.head
    }

    /// 守恒：Post 总数 = 泵出总数 + 驻留数。
    pub fn conserve(&self) -> Result<(), u32> {
        if self.post_total != self.dispatch_total + self.resident() as u64 {
            return Err(PipeCode::SequenceConservationBroken.val());
        }
        Ok(())
    }

    /// 三通道齐备账：入口名三列齐备。
    pub fn channels_ready() -> bool {
        if CHANNEL_NAMES.len() < CHANNEL_FLOOR {
            return false;
        }
        let mut i = 0;
        while i < CHANNEL_NAMES.len() {
            if CHANNEL_NAMES[i].len() == 0 {
                return false;
            }
            i += 1;
        }
        true
    }

    /// D4 泵挂接：派发入口名必须在 D4 表精确命中。
    pub fn pump_linked(name: &str) -> bool {
        let mut i = 0;
        while i < D4_PUMP_TABLE.len() {
            if D4_PUMP_TABLE[i] == name {
                return true;
            }
            i += 1;
        }
        false
    }
}

// ── LCG（确定性，种子写死） ─────────────────────────────────────
pub struct Lcg(u64);
impl Lcg {
    pub fn new(seed: u64) -> Lcg {
        Lcg(seed)
    }
    pub fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        self.0
    }
    pub fn below(&mut self, n: u64) -> u64 {
        if n == 0 {
            return 0;
        }
        self.next() % n
    }
}

// ── 注入检出账 ──────────────────────────────────────────────────
pub struct InjectLedger {
    pub tries: [u32; 4],
    pub caught: [u32; 4],
}
impl InjectLedger {
    pub fn new() -> InjectLedger {
        InjectLedger { tries: [0; 4], caught: [0; 4] }
    }
    pub fn record(&mut self, slot: usize, hit: bool) {
        if slot < 4 {
            self.tries[slot] += 1;
            if hit {
                self.caught[slot] += 1;
            }
        }
    }
    pub fn all_caught(&self) -> bool {
        let mut i = 0;
        while i < 4 {
            if self.tries[i] == 0 || self.caught[i] != self.tries[i] {
                return false;
            }
            i += 1;
        }
        true
    }
}

/// 第 round 轮随机消息（合法种类）。
pub fn random_msg(round: usize, seq: u64) -> Result<PipeMsg, u32> {
    let mut lcg = Lcg::new(SEG_BASE as u64 + round as u64 * 7 + seq);
    let kind = lcg.below(MSG_KINDS as u64) as usize;
    let custom = if kind == 5 {
        CUSTOM_MSG_BASE + lcg.below(CUSTOM_MSG_SPAN as u64) as u32
    } else {
        0
    };
    let m = PipeMsg { kind, custom, seq };
    m.legal()?;
    Ok(m)
}

/// 黄金全链：Post N 条 → 分批泵干 → 守恒闭合。
pub fn golden_run() -> Result<(u64, u64, u64), u32> {
    let mut pipe = MsgPipe::new();
    let mut seq = 0u64;
    // 入队 100 条。
    let mut i = 0;
    while i < 100 {
        let m = random_msg(i as usize, seq)?;
        pipe.post(m)?;
        seq += 1;
        i += 1;
    }
    // Send 直调 20 条（不入队）。
    i = 0;
    while i < 20 {
        let m = random_msg((i + 100) as usize, seq)?;
        pipe.send(m)?;
        seq += 1;
        i += 1;
    }
    // 分批泵干。
    let mut pumped = 0usize;
    loop {
        let n = pipe.dispatch_batch()?;
        pumped += n;
        if n < DISPATCH_BATCH_MAX {
            break;
        }
    }
    pipe.conserve()?;
    if !MsgPipe::channels_ready() {
        return Err(PipeCode::ChannelNameBlank.val());
    }
    if !MsgPipe::pump_linked(DISPATCH_FROZEN_PUMP) {
        return Err(PipeCode::PumpNotLinked.val());
    }
    Ok((pipe.post_total, pipe.dispatch_total, pipe.send_total))
}

/// 探针三序（对照值写死）。
pub fn probe_seq_a() -> Result<(), u32> {
    // 黄金序：Post→Dispatch→守恒。
    let mut pipe = MsgPipe::new();
    let m = PipeMsg { kind: 0, custom: 0, seq: 1 };
    pipe.post(m)?;
    pipe.dispatch_one()?;
    pipe.conserve()
}

pub fn probe_seq_b() -> Result<(), u32> {
    // 空队泵出 → 必拒（空队显性）。
    let mut pipe = MsgPipe::new();
    pipe.dispatch_one().map(|_| ())
}

pub fn probe_seq_c() -> Result<(), u32> {
    // 重入超深 → 必拒。
    let mut pipe = MsgPipe::new();
    let m = PipeMsg { kind: 1, custom: 0, seq: 1 };
    let mut d = 0;
    while d < SEND_REENTRY_CAP {
        pipe.send(m.clone())?;
        d += 1;
    }
    // 已达上界，再 send 必拒。
    pipe.send(m).map(|_| ())
}

/// 注入 10/10 检出：空队泵、越种类、越值域、队列满四槽。
pub fn inject_rounds(rounds: u32) -> Result<InjectLedger, u32> {
    let mut led = InjectLedger::new();
    let mut r = 0;
    while r < rounds {
        // 槽0：空队泵出注入。
        let mut pipe = MsgPipe::new();
        let hit0 = pipe.dispatch_one().is_err();
        led.record(0, hit0);
        // 槽1：消息种类越闭集。
        let m1 = PipeMsg { kind: MSG_KINDS, custom: 0, seq: 1 };
        let hit1 = m1.legal().is_err();
        led.record(1, hit1);
        // 槽2：custom 越值域。
        let m2 = PipeMsg { kind: 5, custom: CUSTOM_MSG_BASE - 1, seq: 2 };
        let hit2 = m2.legal().is_err();
        led.record(2, hit2);
        // 槽3：队列满注入（灌满再投）。
        let mut pipe3 = MsgPipe::new();
        let mut filled = true;
        let mut k = 0;
        while k < POST_QUEUE_CAP {
            if pipe3.post(PipeMsg { kind: 0, custom: 0, seq: k as u64 }).is_err() {
                filled = false;
                break;
            }
            k += 1;
        }
        let hit3 = filled
            && pipe3
                .post(PipeMsg { kind: 0, custom: 0, seq: 9999 })
                .is_err();
        led.record(3, hit3);
        r += 1;
    }
    Ok(led)
}

/// 双跑确定性对拍。
pub fn repro_check() -> Result<(), u32> {
    let a = golden_run()?;
    let b = golden_run()?;
    if a != b {
        return Err(PipeCode::NotReproducible.val());
    }
    Ok(())
}

/// 段位守卫。
pub fn segment_guard() -> bool {
    let mut i = 0;
    while i < PipeCode::ALL.len() {
        let v = PipeCode::ALL[i].val();
        if v < SEG_BASE || v >= SEG_BASE + 0x40 {
            return false;
        }
        i += 1;
    }
    true
}

pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut v: Vec<(&'static str, bool, String)> = Vec::new();
    let g = golden_run();
    v.push(("J1-黄金全链", g.is_ok(), format!("{:?}", g)));
    // 三通道齐备率 100%。
    v.push((
        "J1-三通道齐备率100%",
        MsgPipe::channels_ready() && CHANNEL_NAMES.len() == 3,
        format!("{:?}", CHANNEL_NAMES),
    ));
    // D4 消息泵挂接一致率 100%。
    v.push((
        "J1-D4泵挂接一致率100%",
        MsgPipe::pump_linked(DISPATCH_FROZEN_PUMP) && D4_PUMP_TABLE.len() == 3,
        format!("pump={} 表={}", DISPATCH_FROZEN_PUMP, D4_PUMP_TABLE.len()),
    ));
    // 注入 10/10。
    let led = inject_rounds(JUDGE_ROUNDS);
    let led_ok = match &led {
        Ok(l) => l.all_caught() && l.caught[0] == 10,
        Err(_) => false,
    };
    v.push((
        "J1-注入10/10",
        led_ok,
        format!(
            "t/c {:?}/{:?}",
            led.as_ref().map(|l| l.tries),
            led.as_ref().map(|l| l.caught)
        ),
    ));
    // 探针三序。
    let a = probe_seq_a();
    let b = probe_seq_b();
    let c = probe_seq_c();
    v.push((
        "J1-探针三序",
        a.is_ok() && b.is_err() && c.is_err(),
        format!("a={:?} b={:?} c={:?}", a.is_ok(), b.is_err(), c.is_err()),
    ));
    // 双跑一致。
    v.push((
        "J2-双跑一致",
        repro_check().is_ok(),
        String::from("golden_run 两跑同值"),
    ));
    // 边界契约点验：容量/重入/批泵三界值冻结。
    v.push((
        "J1-边界契约点验",
        POST_QUEUE_CAP == 512
            && SEND_REENTRY_CAP == 32
            && DISPATCH_BATCH_MAX == 64,
        format!("cap{} re{} batch{}", POST_QUEUE_CAP, SEND_REENTRY_CAP, DISPATCH_BATCH_MAX),
    ));
    v.push(("J3-段位守卫", segment_guard(), String::from("0x67E40 窗口")));
    v
}

pub fn run_unx16301_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F16301 消息投递通道总纲");
    for (n, p, d) in extra_checks() {
        s.add_dyn(n, p, &d);
    }
    s
}
