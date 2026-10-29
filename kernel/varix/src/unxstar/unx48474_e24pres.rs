//! UNX-F48474 · E24 异常呈现三要素（断连重连用户可见）
//!
//! 判据原文：UNX-F48474-J1 —— 断连与重连两类异常的用户可见呈现满足三要素：
//! ① 文案非空且含原因码（可定位）；② 时机戳在事件后单步内（及时性）；
//! ③ 附可恢复指引（行动项非空）。三要素缺一即呈现失败；文案指纹对
//! （异常类型 × 严重级）一一对应不串用；呈现队列 FIFO 不重排；隐藏级
//! 事件不产生用户可见条目；全场景 drill 零偏差。
//!
//! 设计纪律：no_std（仅 core/alloc）；零 unwrap/expect/panic；fuzz 用
//! 确定性 LCG（wrapping_mul/wrapping_add），同种子同序列。

use crate::alloc::vec;
use crate::alloc::vec::Vec;
use alloc::string::{String, ToString};

pub const SEG_BASE: u16 = 0xE4E0;

pub const E_EMPTY_TEXT: u16 = SEG_BASE + 1;
pub const E_LATE: u16 = SEG_BASE + 2;
pub const E_NO_GUIDE: u16 = SEG_BASE + 3;
pub const E_FP_CLASH: u16 = SEG_BASE + 4;
pub const E_REORDER: u16 = SEG_BASE + 5;
pub const E_HIDDEN_LEAK: u16 = SEG_BASE + 6;
pub const E_KIND_OOB: u16 = SEG_BASE + 7;

/// 异常类型。
pub const K_DISCONNECT: usize = 0;
pub const K_RECONNECT: usize = 1;
pub const K_THROTTLE: usize = 2;
pub const K_VIOLATION: usize = 3;
pub const KIND_N: usize = 4;

/// 严重级。
pub const SEV_NOTICE: u8 = 0;
pub const SEV_WARN: u8 = 1;
pub const SEV_FATAL: u8 = 2;

/// 时机容忍：呈现 tick 与事件 tick 差不得超过 1 步。
pub const TIMING_TOLERANCE: u64 = 1;

/// 用户可见呈现条目。
#[derive(Clone, PartialEq, Eq)]
pub struct PresentCard {
    pub kind: usize,
    pub sev: u8,
    pub text: String,
    pub guide: String,
    pub ev_tick: u64,
    pub shown_tick: u64,
    pub visible: bool,
}

impl PresentCard {
    /// 要素一：文案非空且含原因码（kind 对应 token）。
    pub fn has_text(&self) -> bool {
        if self.text.len() == 0 {
            return false;
        }
        let token = match self.kind {
            K_DISCONNECT => "disc",
            K_RECONNECT => "recn",
            K_THROTTLE => "thrt",
            K_VIOLATION => "viol",
            _ => return false,
        };
        self.contains_token(token)
    }
    /// 要素三：指引非空。
    pub fn has_guide(&self) -> bool {
        self.guide.len() > 0
    }
    /// 要素二：时机单步内。
    pub fn in_time(&self) -> bool {
        self.shown_tick >= self.ev_tick && self.shown_tick - self.ev_tick <= TIMING_TOLERANCE
    }
    /// 三要素总检。
    pub fn tri_ok(&self) -> Result<(), u16> {
        if !self.has_text() {
            return Err(E_EMPTY_TEXT);
        }
        if !self.in_time() {
            return Err(E_LATE);
        }
        if !self.has_guide() {
            return Err(E_NO_GUIDE);
        }
        Ok(())
    }
    /// 子串包含（no_std 无 str::contains 可用面，手写）。
    fn contains_token(&self, token: &str) -> bool {
        let t = token.as_bytes();
        let hay = self.text.as_bytes();
        if t.len() > hay.len() {
            return false;
        }
        for i in 0..=(hay.len() - t.len()) {
            let mut ok = true;
            for j in 0..t.len() {
                if hay[i + j] != t[j] {
                    ok = false;
                    break;
                }
            }
            if ok {
                return true;
            }
        }
        false
    }
    /// 文案指纹：kind/sev 混入 FNV。
    pub fn fp(&self) -> u64 {
        let mut h: u64 = 0xcbf29ce484222325;
        h ^= self.kind as u64;
        h = h.wrapping_mul(0x100000001b3);
        h ^= self.sev as u64;
        h = h.wrapping_mul(0x100000001b3);
        for b in self.text.as_bytes().iter() {
            h ^= *b as u64;
            h = h.wrapping_mul(0x100000001b3);
        }
        h
    }
}

/// 呈现工厂：按（kind × sev）产出文案与指引（指纹一一对应的单一真源）。
pub fn render(kind: usize, sev: u8, ev_tick: u64) -> Result<PresentCard, u16> {
    if kind >= KIND_N {
        return Err(E_KIND_OOB);
    }
    if sev > SEV_FATAL {
        return Err(E_KIND_OOB);
    }
    let text = match (kind, sev) {
        (K_DISCONNECT, SEV_NOTICE) => "disc: link down, buffering".to_string(),
        (K_DISCONNECT, SEV_WARN) => "disc: link down, reconnecting".to_string(),
        (K_DISCONNECT, SEV_FATAL) => "disc: link down, context lost".to_string(),
        (K_RECONNECT, SEV_NOTICE) => "recn: back online".to_string(),
        (K_RECONNECT, SEV_WARN) => "recn: back online after retry".to_string(),
        (K_RECONNECT, SEV_FATAL) => "recn: manual reconnect required".to_string(),
        (K_THROTTLE, SEV_NOTICE) => "thrt: rate limited, waiting".to_string(),
        (K_THROTTLE, SEV_WARN) => "thrt: heavy rate limit".to_string(),
        (K_THROTTLE, SEV_FATAL) => "thrt: throttle ceiling reached".to_string(),
        (K_VIOLATION, SEV_NOTICE) => "viol: protocol deviation logged".to_string(),
        (K_VIOLATION, SEV_WARN) => "viol: protocol deviation, degrading".to_string(),
        (K_VIOLATION, SEV_FATAL) => "viol: fatal deviation, isolating".to_string(),
        _ => return Err(E_FP_CLASH),
    };
    let guide = match sev {
        SEV_NOTICE => "no action needed".to_string(),
        SEV_WARN => "keep window open; auto recovery in progress".to_string(),
        _ => "save work; follow recovery checklist".to_string(),
    };
    Ok(PresentCard {
        kind,
        sev,
        text,
        guide,
        ev_tick,
        shown_tick: ev_tick + 1,
        visible: true,
    })
}

/// 呈现队列：FIFO，隐藏级不入队。
pub struct PresentQueue {
    items: Vec<PresentCard>,
}

impl PresentQueue {
    pub fn new() -> PresentQueue {
        PresentQueue { items: Vec::new() }
    }
    pub fn push(&mut self, card: PresentCard) {
        if !card.visible {
            return;
        }
        self.items.push(card);
    }
    pub fn len(&self) -> usize {
        self.items.len()
    }
    /// FIFO 顺序读出。
    pub fn snapshot(&self) -> Vec<PresentCard> {
        self.items.clone()
    }
    /// 全队三要素审计：逐条 tri_ok。
    pub fn audit_tri(&self) -> Result<(), u16> {
        for c in self.items.iter() {
            c.tri_ok()?;
        }
        Ok(())
    }
    /// 指纹一一对应审计：同（kind,sev）指纹相等、异（kind,sev）指纹互异。
    pub fn audit_fp_map(&self) -> Result<(), u16> {
        for i in 0..self.items.len() {
            for j in (i + 1)..self.items.len() {
                let a = &self.items[i];
                let b = &self.items[j];
                if a.kind == b.kind && a.sev == b.sev {
                    if a.fp() != b.fp() {
                        return Err(E_FP_CLASH);
                    }
                } else if a.fp() == b.fp() {
                    return Err(E_FP_CLASH);
                }
            }
        }
        Ok(())
    }
}

/// 确定性 LCG。
pub struct Lcg {
    st: u64,
}

impl Lcg {
    pub fn new(seed: u64) -> Lcg {
        Lcg { st: seed }
    }
    pub fn next(&mut self) -> u64 {
        self.st = self
            .st
            .wrapping_mul(1_664_525)
            .wrapping_add(1_013_904_223);
        self.st
    }
    pub fn below(&mut self, b: u64) -> u64 {
        if b == 0 {
            return 0;
        }
        self.next() % b
    }
}

/// R1 全矩阵三要素：4 kind × 3 sev 全部 render 成功且 tri_ok。
pub fn r1_matrix_tri() -> Result<(), u16> {
    for kind in 0..KIND_N {
        for sev in [SEV_NOTICE, SEV_WARN, SEV_FATAL].iter() {
            let c = render(kind, *sev, 100)?;
            c.tri_ok()?;
        }
    }
    Ok(())
}

/// R2 时机单步：shown_tick 恰为 ev_tick+1；人为延迟 2 步判 E_LATE。
pub fn r2_timing_bound() -> Result<(), u16> {
    let c = render(K_DISCONNECT, SEV_WARN, 50)?;
    if c.shown_tick - c.ev_tick != 1 {
        return Err(E_LATE);
    }
    let mut late = c.clone();
    late.shown_tick = late.ev_tick + 2;
    match late.tri_ok() {
        Err(E_LATE) => {}
        _ => return Err(E_LATE),
    }
    Ok(())
}

/// R3 指纹一一对应：12 组（kind,sev）指纹全互异。
pub fn r3_fp_matrix() -> Result<(), u16> {
    let mut q = PresentQueue::new();
    for kind in 0..KIND_N {
        for sev in [SEV_NOTICE, SEV_WARN, SEV_FATAL].iter() {
            q.push(render(kind, *sev, 1)?);
        }
    }
    if q.len() != 12 {
        return Err(E_REORDER);
    }
    q.audit_fp_map()
}

/// R4 隐藏级不入队：visible=false 的卡不产生用户可见条目。
pub fn r4_hidden_no_leak() -> Result<(), u16> {
    let mut q = PresentQueue::new();
    let mut hid = render(K_RECONNECT, SEV_NOTICE, 5)?;
    hid.visible = false;
    q.push(hid);
    q.push(render(K_DISCONNECT, SEV_WARN, 6)?);
    if q.len() != 1 {
        return Err(E_HIDDEN_LEAK);
    }
    if q.snapshot()[0].kind != K_DISCONNECT {
        return Err(E_REORDER);
    }
    Ok(())
}

/// R5 FIFO 不重排：乱序 tick 入队，读出顺序与入队顺序一致。
pub fn r5_fifo_order() -> Result<(), u16> {
    let mut q = PresentQueue::new();
    q.push(render(K_VIOLATION, SEV_WARN, 90)?);
    q.push(render(K_THROTTLE, SEV_NOTICE, 10)?);
    q.push(render(K_DISCONNECT, SEV_FATAL, 55)?);
    let snap = q.snapshot();
    if snap[0].kind != K_VIOLATION || snap[1].kind != K_THROTTLE || snap[2].kind != K_DISCONNECT {
        return Err(E_REORDER);
    }
    Ok(())
}

/// R6 文案 token 对位：kind 对应 token 全部命中、错 kind 不命中。
pub fn r6_token_match() -> Result<(), u16> {
    let c = render(K_THROTTLE, SEV_WARN, 1)?;
    if !c.has_text() {
        return Err(E_EMPTY_TEXT);
    }
    let mut wrong = c.clone();
    wrong.kind = K_DISCONNECT;
    // 文案含 thrt 但 kind 断言 disc —— has_text 以 kind 取 token，应失败。
    if wrong.has_text() {
        return Err(E_FP_CLASH);
    }
    Ok(())
}

/// R7 空文案/空指引拒收：手工构造空条目 tri_ok 分别报 E_EMPTY_TEXT/E_NO_GUIDE。
pub fn r7_empty_reject() -> Result<(), u16> {
    let mut c = render(K_DISCONNECT, SEV_NOTICE, 1)?;
    c.text = String::new();
    match c.tri_ok() {
        Err(E_EMPTY_TEXT) => {}
        _ => return Err(E_EMPTY_TEXT),
    }
    let mut c2 = render(K_DISCONNECT, SEV_NOTICE, 1)?;
    c2.guide = String::new();
    match c2.tri_ok() {
        Err(E_NO_GUIDE) => {}
        _ => return Err(E_NO_GUIDE),
    }
    Ok(())
}

/// R8 kind 越界拒收：render(KIND_N..) 报 E_KIND_OOB。
pub fn r8_kind_oob() -> Result<(), u16> {
    for k in [KIND_N, KIND_N + 9, 99].iter() {
        match render(*k, SEV_WARN, 1) {
            Err(E_KIND_OOB) => {}
            _ => return Err(E_KIND_OOB),
        }
    }
    match render(K_DISCONNECT, 9, 1) {
        Err(E_KIND_OOB) => {}
        _ => return Err(E_KIND_OOB),
    }
    Ok(())
}

/// R9 fuzz 混合呈现：60 卡随机（kind,sev,tick），全队三要素+指纹图双绿。
pub fn r9_fuzz_mix() -> Result<(), u16> {
    let mut g = Lcg::new(0x48474_0001);
    let mut q = PresentQueue::new();
    for _ in 0..60 {
        let k = g.below(KIND_N as u64) as usize;
        let s = g.below(3) as u8;
        let t = g.next();
        q.push(render(k, s, t)?);
    }
    q.audit_tri()?;
    q.audit_fp_map()
}

/// R10 断连→重连叙事链：dis 断连卡后接 recn 恢复卡，时序单调且都可见。
pub fn r10_disc_recn_story() -> Result<(), u16> {
    let mut q = PresentQueue::new();
    q.push(render(K_DISCONNECT, SEV_WARN, 100)?);
    q.push(render(K_DISCONNECT, SEV_WARN, 110)?);
    q.push(render(K_RECONNECT, SEV_NOTICE, 111)?);
    let snap = q.snapshot();
    if snap.len() != 3 {
        return Err(E_REORDER);
    }
    for i in 1..snap.len() {
        if snap[i].ev_tick < snap[i - 1].ev_tick {
            return Err(E_REORDER);
        }
    }
    for c in snap.iter() {
        if !c.visible {
            return Err(E_HIDDEN_LEAK);
        }
        c.tri_ok()?;
    }
    Ok(())
}

/// 判据主入口：R1..R10，hits 计数。
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;
    r1_matrix_tri()?;
    hits += 1;
    r2_timing_bound()?;
    hits += 1;
    r3_fp_matrix()?;
    hits += 1;
    r4_hidden_no_leak()?;
    hits += 1;
    r5_fifo_order()?;
    hits += 1;
    r6_token_match()?;
    hits += 1;
    r7_empty_reject()?;
    hits += 1;
    r8_kind_oob()?;
    hits += 1;
    r9_fuzz_mix()?;
    hits += 1;
    r10_disc_recn_story()?;
    hits += 1;
    Ok(hits)
}
