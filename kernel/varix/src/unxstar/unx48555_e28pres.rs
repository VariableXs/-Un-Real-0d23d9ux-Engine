//! UNX-F48555 · E28 异常呈现三要素（恢复提示含找回路径）
//!
//! 判据原文：UNX-F48555-J1 —— 恢复面异常呈现满足三要素：① 文案非空
//! 且含原因 token（powerloss/recover/cleanup/reset 可定位）；② 时机单步
//! 内（呈现刻 - 事件刻 ≤1）；③ 指引非空且含找回路径（断电类必含
//! recover 路径 token）。三要素缺一即失败；文案指纹对（类型×级别）
//! 一一对应；呈现队列 FIFO 不重排；隐藏级不入队；全场景 drill 零偏差。
//!
//! 设计纪律：no_std（仅 core/alloc）；零 unwrap/expect/panic；fuzz 用
//! 确定性 LCG（wrapping_mul/wrapping_add），同种子同序列。

use crate::alloc::vec;
use crate::alloc::vec::Vec;
use alloc::string::{String, ToString};

pub const SEG_BASE: u16 = 0xF460;

pub const E_EMPTY_TEXT: u16 = SEG_BASE + 1;
pub const E_LATE: u16 = SEG_BASE + 2;
pub const E_NO_GUIDE: u16 = SEG_BASE + 3;
pub const E_NO_PATH: u16 = SEG_BASE + 4;
pub const E_FP_CLASH: u16 = SEG_BASE + 5;
pub const E_REORDER: u16 = SEG_BASE + 6;
pub const E_HIDDEN_LEAK: u16 = SEG_BASE + 7;
pub const E_KIND_OOB: u16 = SEG_BASE + 8;

/// 恢复面异常类型。
pub const K_POWERLOSS: usize = 0;
pub const K_RECOVER: usize = 1;
pub const K_CLEANUP: usize = 2;
pub const K_RESET: usize = 3;
pub const KIND_N: usize = 4;

/// 级别。
pub const SEV_INFO: u8 = 0;
pub const SEV_WARN: u8 = 1;
pub const SEV_FATAL: u8 = 2;

/// 时机容忍。
pub const TIMING_TOL: u64 = 1;

/// 呈现卡。
#[derive(Clone, PartialEq, Eq)]
pub struct RecCard {
    pub kind: usize,
    pub sev: u8,
    pub text: String,
    pub guide: String,
    pub ev_tick: u64,
    pub shown_tick: u64,
    pub visible: bool,
}

impl RecCard {
    fn has_token(&self, token: &str) -> bool {
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
    /// 要素一：文案含类型 token。
    pub fn has_text(&self) -> bool {
        if self.text.len() == 0 {
            return false;
        }
        let token = match self.kind {
            K_POWERLOSS => "powerloss",
            K_RECOVER => "recover",
            K_CLEANUP => "cleanup",
            K_RESET => "reset",
            _ => return false,
        };
        self.has_token(token)
    }
    /// 要素二：时机单步。
    pub fn in_time(&self) -> bool {
        self.shown_tick >= self.ev_tick && self.shown_tick - self.ev_tick <= TIMING_TOL
    }
    /// 要素三：指引非空且断电类含找回路径。
    pub fn has_guide(&self) -> bool {
        if self.guide.len() == 0 {
            return false;
        }
        if self.kind == K_POWERLOSS {
            // 找回路径 token：guidance 需含 "recover"。
            let g = self.guide.as_bytes();
            let t = b"recover";
            if t.len() > g.len() {
                return false;
            }
            for i in 0..=(g.len() - t.len()) {
                let mut ok = true;
                for j in 0..t.len() {
                    if g[i + j] != t[j] {
                        ok = false;
                        break;
                    }
                }
                if ok {
                    return true;
                }
            }
            return false;
        }
        true
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
            if self.guide.len() == 0 {
                return Err(E_NO_GUIDE);
            }
            return Err(E_NO_PATH);
        }
        Ok(())
    }
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

/// 呈现工厂。
pub fn render(kind: usize, sev: u8, ev_tick: u64) -> Result<RecCard, u16> {
    if kind >= KIND_N || sev > SEV_FATAL {
        return Err(E_KIND_OOB);
    }
    let text = match (kind, sev) {
        (K_POWERLOSS, SEV_INFO) => "powerloss: journal replay done".to_string(),
        (K_POWERLOSS, SEV_WARN) => "powerloss: partial loss, replayed".to_string(),
        (K_POWERLOSS, SEV_FATAL) => "powerloss: state rollback applied".to_string(),
        (K_RECOVER, SEV_INFO) => "recover: session restored".to_string(),
        (K_RECOVER, SEV_WARN) => "recover: restored with gaps".to_string(),
        (K_RECOVER, SEV_FATAL) => "recover: manual restore needed".to_string(),
        (K_CLEANUP, SEV_INFO) => "cleanup: stale locks removed".to_string(),
        (K_CLEANUP, SEV_WARN) => "cleanup: temp files purged".to_string(),
        (K_CLEANUP, SEV_FATAL) => "cleanup: forced purge done".to_string(),
        (K_RESET, SEV_INFO) => "reset: factory defaults set".to_string(),
        (K_RESET, SEV_WARN) => "reset: user data cleared".to_string(),
        (K_RESET, SEV_FATAL) => "reset: irreversible wipe done".to_string(),
        _ => return Err(E_FP_CLASH),
    };
    let guide = if kind == K_POWERLOSS {
        match sev {
            SEV_INFO => "no action; auto recover path active".to_string(),
            SEV_WARN => "see recover log to find lost entries".to_string(),
            _ => "run recover checklist; recover path: journal replay".to_string(),
        }
    } else {
        match sev {
            SEV_INFO => "no action needed".to_string(),
            SEV_WARN => "verify settings after this step".to_string(),
            _ => "follow recovery checklist now".to_string(),
        }
    };
    Ok(RecCard {
        kind,
        sev,
        text,
        guide,
        ev_tick,
        shown_tick: ev_tick + 1,
        visible: true,
    })
}

/// FIFO 队列。
pub struct RecQueue {
    items: Vec<RecCard>,
}

impl RecQueue {
    pub fn new() -> RecQueue {
        RecQueue { items: Vec::new() }
    }
    pub fn push(&mut self, c: RecCard) {
        if !c.visible {
            return;
        }
        self.items.push(c);
    }
    pub fn len(&self) -> usize {
        self.items.len()
    }
    pub fn audit_tri(&self) -> Result<(), u16> {
        for c in self.items.iter() {
            c.tri_ok()?;
        }
        Ok(())
    }
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

/// R1 全矩阵三要素：4 kind × 3 sev 全过。
pub fn r1_matrix_tri() -> Result<(), u16> {
    for kind in 0..KIND_N {
        for sev in [SEV_INFO, SEV_WARN, SEV_FATAL].iter() {
            render(kind, *sev, 100)?.tri_ok()?;
        }
    }
    Ok(())
}

/// R2 找回路径强制：断电类指引必含 recover；其他类豁免。
pub fn r2_recover_path() -> Result<(), u16> {
    for sev in [SEV_INFO, SEV_WARN, SEV_FATAL].iter() {
        let c = render(K_POWERLOSS, *sev, 1)?;
        if !c.has_guide() {
            return Err(E_NO_PATH);
        }
    }
    Ok(())
}

/// R3 指纹一一对应：12 组全互异。
pub fn r3_fp_matrix() -> Result<(), u16> {
    let mut q = RecQueue::new();
    for kind in 0..KIND_N {
        for sev in [SEV_INFO, SEV_WARN, SEV_FATAL].iter() {
            q.push(render(kind, *sev, 1)?);
        }
    }
    if q.len() != 12 {
        return Err(E_REORDER);
    }
    q.audit_fp_map()
}

/// R4 隐藏级不入队。
pub fn r4_hidden_no_leak() -> Result<(), u16> {
    let mut q = RecQueue::new();
    let mut hid = render(K_RESET, SEV_INFO, 5)?;
    hid.visible = false;
    q.push(hid);
    q.push(render(K_CLEANUP, SEV_WARN, 6)?);
    if q.len() != 1 {
        return Err(E_HIDDEN_LEAK);
    }
    Ok(())
}

/// R5 FIFO 不重排。
pub fn r5_fifo_order() -> Result<(), u16> {
    let mut q = RecQueue::new();
    q.push(render(K_RESET, SEV_FATAL, 90)?);
    q.push(render(K_CLEANUP, SEV_INFO, 10)?);
    q.push(render(K_POWERLOSS, SEV_WARN, 55)?);
    let snap_fp: Vec<u64> = Vec::new();
    let _ = snap_fp;
    // 顺序核对：入队序 == 读出序。
    let kinds = [K_RESET, K_CLEANUP, K_POWERLOSS];
    for i in 0..3usize {
        // 通过三要素审计保证存在；顺序用指纹重放验证。
        let _ = kinds[i];
    }
    q.audit_tri()?;
    // 重放同序列构造指纹比对。
    let mut expect: Vec<u64> = Vec::new();
    expect.push(render(K_RESET, SEV_FATAL, 90)?.fp());
    expect.push(render(K_CLEANUP, SEV_INFO, 10)?.fp());
    expect.push(render(K_POWERLOSS, SEV_WARN, 55)?.fp());
    let mut actual: Vec<u64> = Vec::new();
    for c in q.items.iter() {
        actual.push(c.fp());
    }
    if expect != actual {
        return Err(E_REORDER);
    }
    Ok(())
}

/// R6 时机单步：人为延迟 2 步判 E_LATE。
pub fn r6_timing() -> Result<(), u16> {
    let c = render(K_RECOVER, SEV_WARN, 50)?;
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

/// R7 空文案/空指引/无路径三分拒。
pub fn r7_empty_reject() -> Result<(), u16> {
    let mut c = render(K_POWERLOSS, SEV_WARN, 1)?;
    c.text = String::new();
    match c.tri_ok() {
        Err(E_EMPTY_TEXT) => {}
        _ => return Err(E_EMPTY_TEXT),
    }
    let mut c2 = render(K_POWERLOSS, SEV_WARN, 1)?;
    c2.guide = String::new();
    match c2.tri_ok() {
        Err(E_NO_GUIDE) => {}
        _ => return Err(E_NO_GUIDE),
    }
    // 断电类指引无 recover token → E_NO_PATH。
    let mut c3 = render(K_POWERLOSS, SEV_WARN, 1)?;
    c3.guide = "check settings".to_string();
    match c3.tri_ok() {
        Err(E_NO_PATH) => {}
        _ => return Err(E_NO_PATH),
    }
    Ok(())
}

/// R8 kind 越界拒收。
pub fn r8_kind_oob() -> Result<(), u16> {
    for k in [KIND_N, KIND_N + 3, 88].iter() {
        match render(*k, SEV_WARN, 1) {
            Err(E_KIND_OOB) => {}
            _ => return Err(E_KIND_OOB),
        }
    }
    match render(K_RESET, 9, 1) {
        Err(E_KIND_OOB) => {}
        _ => return Err(E_KIND_OOB),
    }
    Ok(())
}

/// R9 fuzz 混合呈现：60 卡随机全绿。
pub fn r9_fuzz_mix() -> Result<(), u16> {
    let mut g = Lcg::new(0x48555_0001);
    let mut q = RecQueue::new();
    for _ in 0..60 {
        let k = g.below(KIND_N as u64) as usize;
        let s = g.below(3) as u8;
        q.push(render(k, s, g.next())?);
    }
    q.audit_tri()?;
    q.audit_fp_map()
}

/// R10 断电→恢复叙事链：powerloss 卡后接 recover 卡，时序单调。
pub fn r10_loss_recover_story() -> Result<(), u16> {
    let mut q = RecQueue::new();
    q.push(render(K_POWERLOSS, SEV_FATAL, 100)?);
    q.push(render(K_RECOVER, SEV_WARN, 101)?);
    q.push(render(K_RECOVER, SEV_INFO, 102)?);
    if q.len() != 3 {
        return Err(E_REORDER);
    }
    for i in 1..q.len() {
        if q.items[i].ev_tick < q.items[i - 1].ev_tick {
            return Err(E_REORDER);
        }
    }
    q.audit_tri()
}

/// 判据主入口：R1..R10，hits 计数。
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;
    r1_matrix_tri()?;
    hits += 1;
    r2_recover_path()?;
    hits += 1;
    r3_fp_matrix()?;
    hits += 1;
    r4_hidden_no_leak()?;
    hits += 1;
    r5_fifo_order()?;
    hits += 1;
    r6_timing()?;
    hits += 1;
    r7_empty_reject()?;
    hits += 1;
    r8_kind_oob()?;
    hits += 1;
    r9_fuzz_mix()?;
    hits += 1;
    r10_loss_recover_story()?;
    hits += 1;
    Ok(hits)
}
