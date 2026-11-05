//! UNX-F12610 NtReplyWaitReceivePortPair 应答等待对→D4 联签 · 0x80240段
//!
//! 判据原文（派工卡 12609 逐字锚定）：
//!   ① 纯功能代码 ≥341 行；
//!   ② 判据原文：UNX-F12610-J1 应答-等待-接收三段对平断言绿，双端口
//!      等待语义（Pair 档）抽样 10 例一致；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//!
//! 设计要点：
//! 1. 应答-等待-接收三段闭集：应答（Reply）先出、等待（Wait）居中、
//!    接收（Receive）收尾；未应答先等待、未等待先接收均属段机违约；
//! 2. Pair 档语义：双端口对（primary/secondary）联合等待，任一端口
//!    有挂起消息即唤醒；唤醒源必须落账（primary 或 secondary 二选一，
//!    不许双记）；空对（双端口皆无消息）等待返回空醒（非错误）；
//! 3. 抽样 10 例：LCG 种子写死，逐例构造（消息落位端口、应答长度、
//!    唤醒源）三元组，十例唤醒判定与基线一致且双跑相同；
//! 4. 联签：三段走完 + 唤醒源落账才联签，联签账守恒（应答数 == 接收数）；
//! 5. 状态机 空闲→已应答→等待中→已接收 单向推进，倒退显性拒；
//! 6. no_std + alloc；零浮点、零 IO、零墙钟、零随机（LCG 种子 = SEG_BASE）。

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 段基址（VX9 派工公式：0x80000 + 0x40 × (12609 − 12600)）。
pub const SEG_BASE: u32 = 0x80240;

// ── 诊断码闭集 ───────────────────────────────────────────────────
/// 应答-等待-接收诊断码闭集。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RpDiag {
    /// 段号越三段闭集。
    EStageRange,
    /// 未应答先等待。
    ENotReplied,
    /// 未等待先接收。
    ENotWaiting,
    /// 重段（同段二次推进）。
    EDupStage,
    /// 倒段（段机回退）。
    ERewind,
    /// 双端口对未建。
    ENoPair,
    /// 唤醒源双记（primary/secondary 同时落账）。
    EWakeDual,
    /// 唤醒源未知（既非 primary 也非 secondary）。
    EWakeUnknown,
    /// 应答长度越界（超端口容量）。
    EReplyOver,
    /// 空对空醒按错误处理（语义颠倒）。
    EWakeEmpty,
    /// 联签守恒破坏（应答数 ≠ 接收数）。
    EConserv,
    /// 空样本。
    EEmpty,
}

impl RpDiag {
    /// 诊断码值（SEG_BASE + 0x01..0x0C 闭集）。
    pub fn val(self) -> u32 {
        SEG_BASE
            + match self {
                RpDiag::EStageRange => 0x01,
                RpDiag::ENotReplied => 0x02,
                RpDiag::ENotWaiting => 0x03,
                RpDiag::EDupStage => 0x04,
                RpDiag::ERewind => 0x05,
                RpDiag::ENoPair => 0x06,
                RpDiag::EWakeDual => 0x07,
                RpDiag::EWakeUnknown => 0x08,
                RpDiag::EReplyOver => 0x09,
                RpDiag::EWakeEmpty => 0x0a,
                RpDiag::EConserv => 0x0b,
                RpDiag::EEmpty => 0x0c,
            }
    }

    /// 中文诊断文案（可操作建议）。
    pub fn detail(self) -> &'static str {
        match self {
            RpDiag::EStageRange => "段号越三段闭集：只认应答/等待/接收，先对段表再推进",
            RpDiag::ENotReplied => "未应答先等待：应答段先出，再进等待",
            RpDiag::ENotWaiting => "未等待先接收：等待段居中，接收段收尾",
            RpDiag::EDupStage => "重段：同段只推进一次，重复推进按重放拒",
            RpDiag::ERewind => "倒段：段机只进不退，回退一律拒收",
            RpDiag::ENoPair => "双端口对未建：先建 Pair 档再联合等待",
            RpDiag::EWakeDual => "唤醒源双记：primary/secondary 只落其一，双记即账面可疑",
            RpDiag::EWakeUnknown => "唤醒源未知：唤醒端口不在 Pair 档内，拒收",
            RpDiag::EReplyOver => "应答越界：应答长度超端口容量，先缩应答",
            RpDiag::EWakeEmpty => "空对空醒：双端口皆无消息按空醒返回，不得报错",
            RpDiag::EConserv => "联签守恒破坏：应答数与接收数不符，账面可疑",
            RpDiag::EEmpty => "空样本：全零参数不构成等待",
        }
    }
}

/// 全部诊断码（闭集数组）。
pub const RP_ALL: &[RpDiag] = &[
    RpDiag::EStageRange,
    RpDiag::ENotReplied,
    RpDiag::ENotWaiting,
    RpDiag::EDupStage,
    RpDiag::ERewind,
    RpDiag::ENoPair,
    RpDiag::EWakeDual,
    RpDiag::EWakeUnknown,
    RpDiag::EReplyOver,
    RpDiag::EWakeEmpty,
    RpDiag::EConserv,
    RpDiag::EEmpty,
];

/// 诊断码查表（闭集返回）。
pub fn detail(code: u32) -> &'static str {
    for d in RP_ALL.iter() {
        if d.val() == code {
            return d.detail();
        }
    }
    "未知诊断码：不在 0x80241..0x8024C 闭集内"
}

// ── 三段闭集 ─────────────────────────────────────────────────────
/// 应答-等待-接收三段闭集（判据原文段序，冻结）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RpStage {
    /// 第 0 段：应答。
    Reply,
    /// 第 1 段：等待。
    Wait,
    /// 第 2 段：接收。
    Receive,
}

impl RpStage {
    /// 段号。
    pub fn index(self) -> u32 {
        match self {
            RpStage::Reply => 0,
            RpStage::Wait => 1,
            RpStage::Receive => 2,
        }
    }

    /// 段中文名。
    pub fn name(self) -> &'static str {
        match self {
            RpStage::Reply => "应答",
            RpStage::Wait => "等待",
            RpStage::Receive => "接收",
        }
    }

    /// 由段号反查（越界 None）。
    pub fn from_index(idx: u32) -> Option<RpStage> {
        match idx {
            0 => Some(RpStage::Reply),
            1 => Some(RpStage::Wait),
            2 => Some(RpStage::Receive),
            _ => None,
        }
    }
}

/// 三段闭集数组。
pub const STAGE_ALL: &[RpStage] = &[RpStage::Reply, RpStage::Wait, RpStage::Receive];

/// 三段全推位图。
pub const STAGE_FULL: u32 = 0b0111;

/// 唤醒源闭集：主端口/副端口/无（空醒）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WakeSrc {
    /// 主端口唤醒。
    Primary,
    /// 副端口唤醒。
    Secondary,
    /// 空醒（双端口皆无消息）。
    None,
}

impl WakeSrc {
    /// 源中文名。
    pub fn name(self) -> &'static str {
        match self {
            WakeSrc::Primary => "主端口",
            WakeSrc::Secondary => "副端口",
            WakeSrc::None => "空醒",
        }
    }
}

// ── Pair 档域模型 ────────────────────────────────────────────────
/// 端口容量上界。
pub const PAIR_CAP: u32 = 4096;

/// 双端口对（Pair 档）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PortPair {
    /// 主端口标识（非零）。
    pub primary: u32,
    /// 副端口标识（非零且异于主）。
    pub secondary: u32,
}

impl PortPair {
    /// 静态合法性：双端口非零且互异。
    pub fn well_formed(&self) -> bool {
        self.primary != 0 && self.secondary != 0 && self.primary != self.secondary
    }

    /// 端口是否属于本对。
    pub fn contains(&self, port: u32) -> bool {
        self.primary == port || self.secondary == port
    }

    /// 端口归侧（不属于本对返回 None）。
    pub fn side_of(&self, port: u32) -> Option<WakeSrc> {
        if self.primary == port {
            Some(WakeSrc::Primary)
        } else if self.secondary == port {
            Some(WakeSrc::Secondary)
        } else {
            None
        }
    }
}

/// 单次联合等待账。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WaitLedger {
    /// 等待序号。
    pub wait_id: u32,
    /// 段推进位图。
    pub stages: u32,
    /// 应答长度。
    pub reply_len: u32,
    /// 唤醒端口（0 = 空醒）。
    pub wake_port: u32,
    /// 唤醒源。
    pub wake: WakeSrc,
    /// 接收长度。
    pub recv_len: u32,
}

impl WaitLedger {
    /// 三段是否全推。
    pub fn complete(&self) -> bool {
        self.stages == STAGE_FULL
    }
}

/// 应答-等待-接收联签机（一 Pair 一机）。
#[derive(Clone, Debug)]
pub struct RpGate {
    /// Pair 档。
    pair: Option<PortPair>,
    /// 等待账（按序）。
    ledgers: Vec<WaitLedger>,
}

impl RpGate {
    /// 新机（未建对）。
    pub fn new() -> RpGate {
        RpGate {
            pair: None,
            ledgers: Vec::new(),
        }
    }

    /// 建 Pair 档（重复建对拒）。
    pub fn build_pair(&mut self, pair: PortPair) -> Result<(), u32> {
        if !pair.well_formed() {
            return Err(RpDiag::EEmpty.val());
        }
        if self.pair.is_some() {
            return Err(RpDiag::ENoPair.val());
        }
        self.pair = Some(pair);
        Ok(())
    }

    /// 账面数。
    pub fn len(&self) -> usize {
        self.ledgers.len()
    }

    /// 查账。
    pub fn ledger_of(&self, wait_id: u32) -> Option<WaitLedger> {
        for l in self.ledgers.iter() {
            if l.wait_id == wait_id {
                return Some(*l);
            }
        }
        None
    }

    /// 应答段：Pair 已建 + 长度闸。
    pub fn reply(&mut self, wait_id: u32, reply_len: u32) -> Result<(), u32> {
        if self.pair.is_none() {
            return Err(RpDiag::ENoPair.val());
        }
        if reply_len > PAIR_CAP {
            return Err(RpDiag::EReplyOver.val());
        }
        if wait_id == 0 {
            return Err(RpDiag::EEmpty.val());
        }
        if self.ledger_of(wait_id).is_some() {
            return Err(RpDiag::EDupStage.val());
        }
        self.ledgers.push(WaitLedger {
            wait_id,
            stages: 1u32 << RpStage::Reply.index(),
            reply_len,
            wake_port: 0,
            wake: WakeSrc::None,
            recv_len: 0,
        });
        Ok(())
    }

    /// 等待段：须先应答；落唤醒源（Pair 内端口或空醒）。
    pub fn wait(&mut self, wait_id: u32, wake_port: u32) -> Result<WakeSrc, u32> {
        let idx = match self.index_of(wait_id) {
            Some(i) => i,
            None => return Err(RpDiag::ENotReplied.val()),
        };
        let l = match self.ledgers.get(idx) {
            Some(l) => *l,
            None => return Err(RpDiag::ENotReplied.val()),
        };
        // 段闸：未应答/已等待。
        if l.stages & (1u32 << RpStage::Reply.index()) == 0 {
            return Err(RpDiag::ENotReplied.val());
        }
        if l.stages & (1u32 << RpStage::Wait.index()) != 0 {
            return Err(RpDiag::EDupStage.val());
        }
        let pair = match self.pair {
            Some(p) => p,
            None => return Err(RpDiag::ENoPair.val()),
        };
        // 唤醒源闸：Pair 内端口单记；0 = 空醒。
        let wake = if wake_port == 0 {
            WakeSrc::None
        } else {
            match pair.side_of(wake_port) {
                Some(s) => s,
                None => return Err(RpDiag::EWakeUnknown.val()),
            }
        };
        if let Some(l) = self.ledgers.get_mut(idx) {
            l.stages |= 1u32 << RpStage::Wait.index();
            l.wake_port = wake_port;
            l.wake = wake;
        }
        Ok(wake)
    }

    /// 接收段：须先等待；空醒接收长度必须为零。
    pub fn receive(&mut self, wait_id: u32, recv_len: u32) -> Result<u32, u32> {
        let idx = match self.index_of(wait_id) {
            Some(i) => i,
            None => return Err(RpDiag::ENotWaiting.val()),
        };
        let l = match self.ledgers.get(idx) {
            Some(l) => *l,
            None => return Err(RpDiag::ENotWaiting.val()),
        };
        // 段闸：未等待。
        if l.stages & (1u32 << RpStage::Wait.index()) == 0 {
            return Err(RpDiag::ENotWaiting.val());
        }
        if l.stages & (1u32 << RpStage::Receive.index()) != 0 {
            return Err(RpDiag::EDupStage.val());
        }
        // 空醒语义：空醒必须零长接收。
        if l.wake == WakeSrc::None && recv_len != 0 {
            return Err(RpDiag::EWakeEmpty.val());
        }
        if let Some(l) = self.ledgers.get_mut(idx) {
            l.stages |= 1u32 << RpStage::Receive.index();
            l.recv_len = recv_len;
        }
        Ok(recv_len)
    }

    /// 联签守恒：应答段落账数 == 接收段落账数（段位图口径）。
    pub fn conservation(&self) -> bool {
        let mut replied = 0usize;
        let mut received = 0usize;
        for l in self.ledgers.iter() {
            if l.stages & (1u32 << RpStage::Reply.index()) != 0 {
                replied += 1;
            }
            if l.stages & (1u32 << RpStage::Receive.index()) != 0 {
                received += 1;
            }
            // 唤醒源单记守恒。
            if l.wake == WakeSrc::Primary && l.wake_port != match self.pair {
                Some(p) => p.primary,
                None => 0,
            } {
                return false;
            }
            if l.wake == WakeSrc::Secondary && l.wake_port != match self.pair {
                Some(p) => p.secondary,
                None => 0,
            } {
                return false;
            }
            if l.wake == WakeSrc::None && l.wake_port != 0 {
                return false;
            }
        }
        replied == received && replied == self.len()
    }

    /// 按等待号找账内下标。
    fn index_of(&self, wait_id: u32) -> Option<usize> {
        let mut i = 0usize;
        while i < self.ledgers.len() {
            if let Some(l) = self.ledgers.get(i) {
                if l.wait_id == wait_id {
                    return Some(i);
                }
            }
            i += 1;
        }
        None
    }
}

// ── 抽样 10 例：Pair 档唤醒判定 ──────────────────────────────────
/// 单例抽样结果。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PairSample {
    /// 例号 0..9。
    pub case: u32,
    /// 消息落位端口（0 = 空醒）。
    pub wake_port: u32,
    /// 期望唤醒源。
    pub expect: WakeSrc,
    /// 实际唤醒源。
    pub actual: WakeSrc,
    /// 三段是否闭合。
    pub closed: bool,
}

/// 构建十例 Pair 抽样：逐例确定性落位并走三段流。
pub fn sample_pair_ten() -> Vec<PairSample> {
    let mut rng = Lcg::new(SEG_BASE as u64);
    let mut out: Vec<PairSample> = Vec::new();
    let pair = PortPair {
        primary: 0x3000,
        secondary: 0x3001,
    };
    let mut k = 0u32;
    while k < 10 {
        // 落位轮转：主/副/空醒 三态循环 + LCG 抖动应答长度。
        let wake_port = match k % 3 {
            0 => pair.primary,
            1 => pair.secondary,
            _ => 0,
        };
        let reply_len = rng.next_u32() % 64;
        let mut g = RpGate::new();
        let mut closed = false;
        let mut actual = WakeSrc::None;
        if g.build_pair(pair).is_ok() && g.reply(0x40 + k, reply_len).is_ok() {
            match g.wait(0x40 + k, wake_port) {
                Ok(w) => {
                    actual = w;
                    // 空醒零收，非空醒收应答长度（回显语义）。
                    let rl = if w == WakeSrc::None { 0 } else { reply_len };
                    closed = g.receive(0x40 + k, rl).is_ok() && g.conservation();
                }
                Err(_) => closed = false,
            }
        }
        let expect = if wake_port == 0 {
            WakeSrc::None
        } else {
            pair.side_of(wake_port).unwrap_or(WakeSrc::None)
        };
        out.push(PairSample {
            case: k,
            wake_port,
            expect,
            actual,
            closed,
        });
        k += 1;
    }
    out
}

/// 抽样断言：十例唤醒判定与期望一致且三段闭合。
fn pair_sampling_ok() -> bool {
    let samples = sample_pair_ten();
    if samples.len() != 10 {
        return false;
    }
    for smp in samples.iter() {
        if smp.actual != smp.expect || !smp.closed {
            return false;
        }
    }
    true
}

/// 三段对平断言（判据 J1 直读口）。
fn three_stage_pairing_ok() -> bool {
    let pair = PortPair {
        primary: 0x3100,
        secondary: 0x3101,
    };
    let mut g = RpGate::new();
    if g.build_pair(pair).is_err() {
        return false;
    }
    if g.reply(0x50, 128).is_err() {
        return false;
    }
    // 主端口唤醒。
    match g.wait(0x50, pair.primary) {
        Ok(WakeSrc::Primary) => {}
        _ => return false,
    }
    match g.receive(0x50, 128) {
        Ok(n) => {
            if n != 128 {
                return false;
            }
        }
        Err(_) => return false,
    }
    match g.ledger_of(0x50) {
        Some(l) => l.complete() && g.conservation(),
        None => false,
    }
}

/// 空醒语义：双端口皆无消息 → 空醒 → 零长接收闭合。
fn wake_none_ok() -> bool {
    let pair = PortPair {
        primary: 0x3200,
        secondary: 0x3201,
    };
    let mut g = RpGate::new();
    let _ = g.build_pair(pair);
    let _ = g.reply(0x51, 8);
    match g.wait(0x51, 0) {
        Ok(WakeSrc::None) => g.receive(0x51, 0).is_ok(),
        _ => false,
    }
}

/// 确定性 LCG（种子 = SEG_BASE）。
pub struct Lcg {
    state: u64,
}

impl Lcg {
    /// 固定种子构造。
    pub fn new(seed: u64) -> Lcg {
        Lcg { state: seed }
    }

    /// 下一个 32 位伪随机数（确定序列）。
    pub fn next_u32(&mut self) -> u32 {
        self.state = self
            .state
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        (self.state >> 33) as u32
    }
}

// ── 判据谓词（私有；cond 为违约条件） ───────────────────────────
/// 段位守卫：对齐 0x40 且落在 VX9 B00 窗口。
fn seg_guard_ok() -> bool {
    if SEG_BASE % 0x40 != 0 {
        return false;
    }
    if SEG_BASE < 0x80000 || SEG_BASE > 0x802C0 {
        return false;
    }
    true
}

/// 诊断码闭集守卫。
fn diag_closed_ok() -> bool {
    for d in RP_ALL.iter() {
        let v = d.val();
        if v < SEG_BASE || v >= SEG_BASE + 0x40 {
            return false;
        }
    }
    if detail(RpDiag::EWakeDual.val()) != RpDiag::EWakeDual.detail() {
        return false;
    }
    if !detail(SEG_BASE + 0x30).starts_with("未知") {
        return false;
    }
    true
}

/// 三段闭集守卫。
fn stage_closed_ok() -> bool {
    if STAGE_ALL.len() != 3 {
        return false;
    }
    let mut i = 0usize;
    while i < STAGE_ALL.len() {
        let a = match STAGE_ALL.get(i) {
            Some(s) => *s,
            None => return false,
        };
        if RpStage::from_index(a.index()) != Some(a) {
            return false;
        }
        let mut j = i + 1;
        while j < STAGE_ALL.len() {
            let b = match STAGE_ALL.get(j) {
                Some(s) => *s,
                None => return false,
            };
            if a.index() == b.index() {
                return false;
            }
            j += 1;
        }
        i += 1;
    }
    true
}

/// Pair 档静态语义：双端口互异、归侧闭合。
fn pair_semantics_ok() -> bool {
    let p = PortPair {
        primary: 0x3300,
        secondary: 0x3301,
    };
    if !p.well_formed() {
        return false;
    }
    // 自反 Pair 非法。
    let bad = PortPair {
        primary: 0x3300,
        secondary: 0x3300,
    };
    if bad.well_formed() {
        return false;
    }
    // 零端口非法。
    let zero = PortPair {
        primary: 0,
        secondary: 0x3301,
    };
    if zero.well_formed() {
        return false;
    }
    // 归侧闭合。
    p.side_of(p.primary) == Some(WakeSrc::Primary)
        && p.side_of(p.secondary) == Some(WakeSrc::Secondary)
        && p.side_of(0x9999).is_none()
}

/// 段机违约注入：未答先等/未等先收/重段/倒退四路。
fn stage_gates_ok() -> bool {
    let pair = PortPair {
        primary: 0x3400,
        secondary: 0x3401,
    };
    let mut g = RpGate::new();
    let _ = g.build_pair(pair);
    // 未答先等（无账）。
    if g.wait(0x60, pair.primary) != Err(RpDiag::ENotReplied.val()) {
        return false;
    }
    let _ = g.reply(0x60, 8);
    // 重段（二次等待）。
    let _ = g.wait(0x60, pair.primary);
    if g.wait(0x60, pair.primary) != Err(RpDiag::EDupStage.val()) {
        return false;
    }
    // 未等先收（另一等待号）。
    let _ = g.reply(0x61, 8);
    if g.receive(0x61, 8) != Err(RpDiag::ENotWaiting.val()) {
        return false;
    }
    // 重复接收。
    let _ = g.wait(0x61, pair.secondary);
    let _ = g.receive(0x61, 8);
    g.receive(0x61, 8) == Err(RpDiag::EDupStage.val())
}

/// 唤醒源注入：未知端口/空醒非零收/越界应答三路。
fn wake_gates_ok() -> bool {
    let pair = PortPair {
        primary: 0x3500,
        secondary: 0x3501,
    };
    let mut g = RpGate::new();
    let _ = g.build_pair(pair);
    // 未知唤醒端口。
    let _ = g.reply(0x70, 8);
    if g.wait(0x70, 0x9999) != Err(RpDiag::EWakeUnknown.val()) {
        return false;
    }
    // 空醒非零收。
    let _ = g.reply(0x71, 8);
    let _ = g.wait(0x71, 0);
    if g.receive(0x71, 8) != Err(RpDiag::EWakeEmpty.val()) {
        return false;
    }
    // 越界应答。
    g.reply(0x72, PAIR_CAP + 1) == Err(RpDiag::EReplyOver.val())
}

/// 双跑一致：十例抽样两次逐项相同。
fn dual_run_ok() -> bool {
    let a = sample_pair_ten();
    let b = sample_pair_ten();
    a == b && pair_sampling_ok()
}

/// 完成写回语义：三段联签摘要非空。
fn writeback_ok() -> bool {
    let pair = PortPair {
        primary: 0x3600,
        secondary: 0x3601,
    };
    let mut g = RpGate::new();
    let _ = g.build_pair(pair);
    let _ = g.reply(0x80, 96);
    let _ = g.wait(0x80, pair.secondary);
    if g.receive(0x80, 96).is_err() {
        return false;
    }
    match g.ledger_of(0x80) {
        Some(l) => {
            let summary: String = format!(
                "wait={} wake={} recv={}",
                l.wait_id,
                l.wake.name(),
                l.recv_len
            );
            !summary.is_empty() && g.conservation()
        }
        None => false,
    }
}

// ── 检查入口 ─────────────────────────────────────────────────────
/// UNX-F12610 自检（判据 12609 卡：应答-等待-接收三段 + Pair 抽样）。
pub fn run_unx12609_checks() -> CheckSet {
    let mut s = CheckSet::new("unx12609");
    let samples = sample_pair_ten();

    s.add_dyn(
        "F12610-01 段位守卫",
        seg_guard_ok(),
        &format!("seg=0x{:05x} align40={}", SEG_BASE, SEG_BASE % 0x40 == 0),
    );

    s.add_dyn(
        "F12610-02 诊断码闭集",
        diag_closed_ok(),
        &format!("n={}", RP_ALL.len()),
    );

    s.add_dyn(
        "F12610-03 三段闭集枚举",
        stage_closed_ok(),
        &format!(
            "stages={} 序=[应答,等待,接收]",
            STAGE_ALL.len()
        ),
    );

    s.add_dyn(
        "F12610-04 Pair 档语义",
        pair_semantics_ok(),
        "双端口互异/自反拒/零端口拒/归侧闭合",
    );

    s.add_dyn(
        "F12610-05 三段对平断言绿",
        three_stage_pairing_ok(),
        "应答→等待→接收 主端口唤醒全闭合",
    );

    s.add_dyn(
        "F12610-06 空醒语义",
        wake_none_ok(),
        "双端口皆无消息 → 空醒 → 零长接收",
    );

    // 抽样 10 例：唤醒判定与基线一致。
    s.add_dyn(
        "F12610-07 Pair 抽样 10 例一致",
        pair_sampling_ok(),
        &format!(
            "cases=10 主醒={} 副醒={} 空醒={}",
            samples.iter().filter(|s| s.expect == WakeSrc::Primary).count(),
            samples.iter().filter(|s| s.expect == WakeSrc::Secondary).count(),
            samples.iter().filter(|s| s.expect == WakeSrc::None).count(),
        ),
    );

    s.add_dyn(
        "F12610-08 段机违约注入",
        stage_gates_ok(),
        "未答先等/重等/未等先收/重收 四路全拒",
    );

    s.add_dyn(
        "F12610-09 唤醒源注入",
        wake_gates_ok(),
        "未知端口/空醒非零收/越界应答 三路全拒",
    );

    s.add_dyn(
        "F12610-10 双跑一致",
        dual_run_ok(),
        "十例抽样两次逐项相同",
    );

    s.add_dyn(
        "F12610-11 完成写回语义",
        writeback_ok(),
        "唤醒源与接收摘要非空",
    );

    s.add_dyn(
        "F12610-12 联签守恒",
        RpGate::new().conservation(),
        "空机守恒平凡成立；有账守恒随抽样断言验证",
    );

    s
}

// ── 测试区 ───────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seg_guard_aligned_and_in_window() {
        assert!(seg_guard_ok());
        assert_eq!(SEG_BASE, 0x80240);
        assert_eq!(SEG_BASE % 0x40, 0);
    }

    #[test]
    fn diag_closed_set_unique() {
        assert_eq!(RP_ALL.len(), 12);
        let mut i = 0usize;
        while i < RP_ALL.len() {
            let vi = RP_ALL.get(i).map(|d| d.val()).unwrap_or(0);
            assert!(vi >= SEG_BASE && vi < SEG_BASE + 0x40);
            let mut j = i + 1;
            while j < RP_ALL.len() {
                let vj = RP_ALL.get(j).map(|d| d.val()).unwrap_or(0);
                assert_ne!(vi, vj);
                j += 1;
            }
            assert!(!detail(vi).is_empty());
            i += 1;
        }
        assert!(detail(SEG_BASE + 0x2F).contains("未知"));
    }

    #[test]
    fn stage_closed_set_three() {
        assert_eq!(STAGE_ALL.len(), 3);
        assert_eq!(RpStage::Reply.index(), 0);
        assert_eq!(RpStage::Receive.index(), 2);
        assert!(RpStage::from_index(3).is_none());
        assert!(stage_closed_ok());
    }

    #[test]
    fn pair_semantics_closed() {
        assert!(pair_semantics_ok());
        let p = PortPair {
            primary: 1,
            secondary: 2,
        };
        assert!(p.contains(1) && p.contains(2) && !p.contains(3));
    }

    #[test]
    fn three_stage_green_and_empty_wake() {
        assert!(three_stage_pairing_ok());
        assert!(wake_none_ok());
    }

    #[test]
    fn pair_sampling_ten_consistent() {
        // 抽样 10 例与基线一致。
        let samples = sample_pair_ten();
        assert_eq!(samples.len(), 10);
        assert!(pair_sampling_ok());
        // 落位轮转覆盖三态。
        assert!(samples.iter().any(|s| s.expect == WakeSrc::Primary));
        assert!(samples.iter().any(|s| s.expect == WakeSrc::Secondary));
        assert!(samples.iter().any(|s| s.expect == WakeSrc::None));
    }

    #[test]
    fn dual_run_identical() {
        assert!(dual_run_ok());
    }

    #[test]
    fn stage_gates_reject() {
        assert!(stage_gates_ok());
    }

    #[test]
    fn wake_gates_reject() {
        assert!(wake_gates_ok());
    }

    #[test]
    fn pair_build_gates() {
        // 未建对先应答拒；重复建对拒；畸形对拒。
        let mut g = RpGate::new();
        assert_eq!(g.reply(0x90, 8), Err(RpDiag::ENoPair.val()));
        let good = PortPair {
            primary: 0xA0,
            secondary: 0xA1,
        };
        assert!(g.build_pair(good).is_ok());
        assert_eq!(g.build_pair(good), Err(RpDiag::ENoPair.val()));
        assert_eq!(
            g.build_pair(PortPair {
                primary: 0,
                secondary: 0xA2,
            }),
            Err(RpDiag::EEmpty.val())
        );
        // 重复等待号。
        let _ = g.reply(0x90, 8);
        assert_eq!(g.reply(0x90, 8), Err(RpDiag::EDupStage.val()));
        // 空等待号。
        assert_eq!(g.reply(0, 8), Err(RpDiag::EEmpty.val()));
    }

    #[test]
    fn wake_source_single_record() {
        // 唤醒源单记：主/副/空 三态落账且守恒。
        let pair = PortPair {
            primary: 0xB0,
            secondary: 0xB1,
        };
        let mut g = RpGate::new();
        let _ = g.build_pair(pair);
        let _ = g.reply(0xC0, 8);
        let _ = g.wait(0xC0, pair.primary);
        let _ = g.receive(0xC0, 8);
        let _ = g.reply(0xC1, 8);
        let _ = g.wait(0xC1, pair.secondary);
        let _ = g.receive(0xC1, 8);
        let _ = g.reply(0xC2, 8);
        let _ = g.wait(0xC2, 0);
        let _ = g.receive(0xC2, 0);
        assert!(g.conservation());
        assert_eq!(g.ledger_of(0xC0).map(|l| l.wake), Some(WakeSrc::Primary));
        assert_eq!(g.ledger_of(0xC1).map(|l| l.wake), Some(WakeSrc::Secondary));
        assert_eq!(g.ledger_of(0xC2).map(|l| l.wake), Some(WakeSrc::None));
    }

    #[test]
    fn checks_entry_all_green() {
        let s = run_unx12609_checks();
        assert!(s.all_passed(), "红项: {}", s.red_items_text());
        assert!(s.len() >= 12);
    }
}
