//! unx48365_storyline — UNX-F48365 会话操作故事线（会话内事件串成可回放时间轴）（诊断段 0xCE80）。
//!
//! 判据原文：
//!   J1 回放 ×10 与事件流逐条一致；判据锚定 体验十三 结构化。
//!   J2 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计纪律：
//!   1. 故事线即时间轴：会话内的每次按键/选词/上屏/模式切换都以（序号,
//!      种类, 载荷）三元组落进时间轴，序号从 0 起连续递增，缺号跳号都算
//!      账目残缺。
//!   2. 回放 ×10 逐条一致：10 个会话各自的完整事件流都要能从记录侧原样
//!      读回，回放序列与录制序列在（seq, kind, payload）三元组上逐条相
//!      等——任何重排、去重、补白都算回放失真。
//!   3. 会话隔离：回放器按会话切片取数，跨会话不得串味；相邻会话的指
//!      纹必须可以区分（载荷由会话种子决定，不同种子必不同串）。
//!   4. 顺序即真相：录制顺序必须原样保留，回放器对逆序输入必须报出首
//!      个失配位（假阴性为零）；对同序输入不得误报（假阳性为零）。
//!   5. 结构化留痕：每个事件载荷带会话内语义位（键位/候选序/提交串码
//!      元），回放侧可独立复核语义位域，不依赖录制侧记忆。
//!   6. 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。

use crate::alloc::vec::Vec;

pub const SEG_BASE: u16 = 0xCE80;

// ── 诊断码（SEG_BASE+1 起，≤0x1F 个）──────────────────────
const E_SEQ_GAP: u16 = SEG_BASE + 1;       // 序号缺号/跳号/回退
const E_REPLAY_MISMATCH: u16 = SEG_BASE + 2; // 回放与录制逐条不一致
const E_EMPTY_SESSION: u16 = SEG_BASE + 3; // 会话故事线为空
const E_CROSS_BLEED: u16 = SEG_BASE + 4;   // 跨会话串味
const E_PAYLOAD_DOMAIN: u16 = SEG_BASE + 5; // 载荷语义位越域
const E_TAMPER_UNDETECTED: u16 = SEG_BASE + 6; // 注入漂移未检出
const E_FP_POSITIVE: u16 = SEG_BASE + 7;   // 同序输入误报
const E_ORDER_FAULT: u16 = SEG_BASE + 8;   // 顺序未被保留
const E_FINGERPRINT_FAULT: u16 = SEG_BASE + 9; // 指纹不一致
const E_CODE_RANGE: u16 = SEG_BASE + 10;   // 诊断码越段

/// 会话内事件种类：按键 / 候选选词 / 上屏提交 / 中英切换。
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum EvKind {
    Key,
    Candidate,
    Commit,
    ModeSwitch,
}

/// 时间轴事件三元组：seq 会话内从 0 连续递增。
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct StoryEvent {
    pub seq: u32,
    pub kind: EvKind,
    pub payload: u16,
}

/// 载荷语义位域：低 10 位数据，高 6 位种类校验码。
pub const PAYLOAD_DATA_MASK: u16 = 0x03FF;
pub const PAYLOAD_KIND_SHIFT: u16 = 10;

/// 打包载荷：语义位域（种类校验码 + 数据）。
pub fn pack_payload(kind: EvKind, data: u16) -> u16 {
    let kc: u16 = match kind {
        EvKind::Key => 1,
        EvKind::Candidate => 2,
        EvKind::Commit => 3,
        EvKind::ModeSwitch => 4,
    };
    (kc << PAYLOAD_KIND_SHIFT) | (data & PAYLOAD_DATA_MASK)
}

/// 载荷域自查：种类校验码与事件种类一致且数据不越 10 位域。
pub fn payload_in_domain(kind: EvKind, payload: u16) -> bool {
    let kc: u16 = match kind {
        EvKind::Key => 1,
        EvKind::Candidate => 2,
        EvKind::Commit => 3,
        EvKind::ModeSwitch => 4,
    };
    (payload >> PAYLOAD_KIND_SHIFT) == kc
}

/// 录制侧：会话故事线（时间轴）。
pub struct Timeline {
    session: u32,
    events: Vec<StoryEvent>,
}

impl Timeline {
    pub fn new(session: u32) -> Self {
        Timeline {
            session,
            events: Vec::new(),
        }
    }

    /// 追加事件：序号必须连续递增，缺号跳号拒收。
    pub fn append(&mut self, kind: EvKind, payload: u16) -> Result<u32, u16> {
        let seq = self.events.len() as u32;
        self.events.push(StoryEvent {
            seq,
            kind,
            payload,
        });
        Ok(seq)
    }

    pub fn session(&self) -> u32 {
        self.session
    }

    pub fn events(&self) -> &[StoryEvent] {
        &self.events
    }

    pub fn len(&self) -> usize {
        self.events.len()
    }
}

/// 回放侧：从录制切片读回事件流，逐条核对（seq, kind, payload）。
/// 返回首个失配位；None = 完全一致。
pub fn replay_compare(recorded: &[StoryEvent], played: &[StoryEvent]) -> Option<usize> {
    if recorded.len() != played.len() {
        return Some(usize::MAX);
    }
    let mut i = 0usize;
    while i < recorded.len() {
        if recorded[i] != played[i] {
            return Some(i);
        }
        i += 1;
    }
    None
}

/// 回放器：按录制顺序原样重演（不得重排、去重、补白）。
pub fn replay(events: &[StoryEvent]) -> Vec<StoryEvent> {
    let mut out: Vec<StoryEvent> = Vec::new();
    let mut i = 0usize;
    while i < events.len() {
        out.push(events[i]);
        i += 1;
    }
    out
}

/// 故事线指纹：FNV-1a 折叠三元组序列。
pub fn fingerprint(events: &[StoryEvent]) -> u64 {
    let mut h: u64 = 0xCBF2_9CE4_8422_2325;
    let mut i = 0usize;
    while i < events.len() {
        h ^= events[i].seq as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01B3);
        h ^= events[i].kind as u8 as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01B3);
        h ^= events[i].payload as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01B3);
        i += 1;
    }
    h
}

/// 确定性线性同余发生器（会话剧本生成统一入口，禁真随机）。
pub struct Lcg(u32);

impl Lcg {
    pub fn new(seed: u32) -> Self {
        Lcg(seed | 1)
    }
    pub fn next(&mut self) -> u32 {
        self.0 = self.0.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        self.0
    }
}

/// 会话剧本：确定性生成一个会话的故事线（20~48 个事件）。
/// 载荷按语义域生成：Key→键位 0..26，Candidate→候选序 0..9，
/// Commit→提交码元 0x4E00..，ModeSwitch→目标模式 0/1。
pub fn script_session(session: u32, seed: u32) -> Timeline {
    let mut lc = Lcg::new(seed);
    let mut tl = Timeline::new(session);
    let n = 20 + (lc.next() % 29) as usize;
    let mut i = 0usize;
    while i < n {
        let kind = match lc.next() % 8 {
            0 | 1 | 2 | 3 => EvKind::Key,
            4 | 5 => EvKind::Candidate,
            6 => EvKind::Commit,
            _ => EvKind::ModeSwitch,
        };
        let data: u16 = match kind {
            EvKind::Key => (lc.next() % 26) as u16,
            EvKind::Candidate => (lc.next() % 10) as u16,
            EvKind::Commit => 0x4E00 + (lc.next() % 0x0200) as u16,
            EvKind::ModeSwitch => (lc.next() % 2) as u16,
        };
        let _ = tl.append(kind, pack_payload(kind, data));
        i += 1;
    }
    tl
}

/// 自例程判据：逐条落实 J1 的全部断言（回放 ×10 与事件流逐条一致）。
pub fn drill() -> Result<u32, u16> {
    let mut hits = 0u32;

    // R1 ×10 会话录制：10 条故事线全部非空、序号从 0 连续无缺号。
    let mut tls: Vec<Timeline> = Vec::new();
    let mut s = 0u32;
    while s < 10 {
        let tl = script_session(s, (0x4836_5001) ^ (s * 7919));
        if tl.len() == 0 {
            return Err(E_EMPTY_SESSION);
        }
        let evs = tl.events();
        let mut i = 0usize;
        while i < evs.len() {
            if evs[i].seq != i as u32 {
                return Err(E_SEQ_GAP);
            }
            i += 1;
        }
        tls.push(tl);
        s += 1;
    }
    hits += 1;

    // R2 回放 ×10 逐条一致：每条故事线回放后与录制流逐三元组相等。
    s = 0;
    while s < 10 {
        let tl = &tls[s as usize];
        let played = replay(tl.events());
        if replay_compare(tl.events(), &played).is_some() {
            return Err(E_REPLAY_MISMATCH);
        }
        s += 1;
    }
    hits += 1;

    // R3 指纹一致：回放指纹 == 录制指纹（×10 会话逐一核对）。
    s = 0;
    while s < 10 {
        let tl = &tls[s as usize];
        let played = replay(tl.events());
        if fingerprint(&played) != fingerprint(tl.events()) {
            return Err(E_FINGERPRINT_FAULT);
        }
        s += 1;
    }
    hits += 1;

    // R4 会话隔离：不同会话种子的指纹必须两两可区分（防串味）。
    let mut a = 0usize;
    while a < tls.len() {
        let mut b = a + 1;
        while b < tls.len() {
            if fingerprint(tls[a].events()) == fingerprint(tls[b].events()) {
                return Err(E_CROSS_BLEED);
            }
            b += 1;
        }
        a += 1;
    }
    // 回放器按切片取数：会话 k 的回放长度必须恰为该会话录制长度。
    s = 0;
    while s < 10 {
        let tl = &tls[s as usize];
        if replay(tl.events()).len() != tl.len() {
            return Err(E_CROSS_BLEED);
        }
        s += 1;
    }
    hits += 1;

    // R5 载荷语义域：全部事件载荷的种类校验码与事件种类一致、数据不越域。
    s = 0;
    while s < 10 {
        let tl = &tls[s as usize];
        let evs = tl.events();
        let mut i = 0usize;
        while i < evs.len() {
            if !payload_in_domain(evs[i].kind, evs[i].payload) {
                return Err(E_PAYLOAD_DOMAIN);
            }
            i += 1;
        }
        s += 1;
    }
    hits += 1;

    // R6 假阴性自查：对会话 3 的回放输入注入单载荷漂移，必须报出失配位。
    let tl = &tls[3];
    let mut tampered: Vec<StoryEvent> = Vec::new();
    let mut i = 0usize;
    while i < tl.events().len() {
        tampered.push(tl.events()[i]);
        i += 1;
    }
    tampered[7] = StoryEvent {
        seq: tampered[7].seq,
        kind: tampered[7].kind,
        payload: tampered[7].payload ^ 0x0001,
    };
    match replay_compare(tl.events(), &tampered) {
        Some(7) => {
            // 恰好报出注入位。
        }
        _ => return Err(E_TAMPER_UNDETECTED),
    }
    hits += 1;

    // R7 假阳性自查：同序重放不得误报；逆序输入必须在 0 位报失配。
    let tl = &tls[5];
    let played = replay(tl.events());
    if replay_compare(tl.events(), &played).is_some() {
        return Err(E_FP_POSITIVE);
    }
    let mut reversed: Vec<StoryEvent> = Vec::new();
    let mut i = tl.events().len();
    while i > 0 {
        i -= 1;
        reversed.push(tl.events()[i]);
    }
    match replay_compare(tl.events(), &reversed) {
        Some(0) => {
            // 首位即失配：顺序未被保留立刻暴露。
        }
        _ => return Err(E_ORDER_FAULT),
    }
    hits += 1;

    // R8 录制侧拒收自查：序号连续性由 Timeline 自身保证（append 即发号），
    // 此处验证重复回放不改变录制侧（账只读）。
    let before = fingerprint(tls[8].events());
    let _ = replay(tls[8].events());
    let _ = replay(tls[8].events());
    if fingerprint(tls[8].events()) != before {
        return Err(E_REPLAY_MISMATCH);
    }
    hits += 1;

    // R9 诊断码闭包：全部错误码落在 [SEG_BASE+1, SEG_BASE+0x1F] 段内。
    const CODES: [u16; 10] = [
        E_SEQ_GAP,
        E_REPLAY_MISMATCH,
        E_EMPTY_SESSION,
        E_CROSS_BLEED,
        E_PAYLOAD_DOMAIN,
        E_TAMPER_UNDETECTED,
        E_FP_POSITIVE,
        E_ORDER_FAULT,
        E_FINGERPRINT_FAULT,
        E_CODE_RANGE,
    ];
    let mut ci = 0usize;
    while ci < CODES.len() {
        if CODES[ci] <= SEG_BASE || CODES[ci] > SEG_BASE + 0x1F {
            return Err(E_CODE_RANGE);
        }
        ci += 1;
    }
    hits += 1;

    Ok(hits)
}
