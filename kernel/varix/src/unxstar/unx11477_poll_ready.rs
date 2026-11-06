//! UNX-F11477 poll 就绪与超时（0x20da0 段）
//!
//! `poll` 的返回值是“有多少个描述符的 `revents` 非零”，而不是事件总数；
//! 一个描述符可以同时报告 `POLLIN|POLLHUP`，但只算一个。`POLLERR`、
//! `POLLHUP`、`POLLNVAL` 是始终报告的例外事件，即使调用方没有请求；
//! 其余事件只有出现在 `events` 里才会被置位。
//!
//! 超时分为三档：0 立即返回、正数等待若干毫秒、-1 无限等待。单线程模拟
//! 不做真实睡眠，而是把“没有任何就绪且超时为负或正”标记为 `blocked`，
//! 调用方可据此决定是否让出。已关闭的描述符不参与就绪判断，直接 `NVAL`；
//! 错误描述符只报 `ERR`，不再报读写就绪。
//!
//! 不变量：`ready` 等于 `revents != 0` 的个数；`mask` 是各 `revents` 的
//! 按位或；`blocked` 为真当且仅当 `ready == 0` 且 `timeout != 0`；
//! 非法事件位或非法超时让整次调用失败且不产生部分结果。

use crate::alloc::format;
use crate::checks::CheckSet;
use alloc::vec::Vec;

/// 段基址。
pub const SEG_BASE: u32 = 0x20da0;
pub const PL_EID: u32 = SEG_BASE + 0x01;
pub const PL_EORDER: u32 = SEG_BASE + 0x02;
pub const PL_EDUP: u32 = SEG_BASE + 0x03;
pub const PL_EFROZEN: u32 = SEG_BASE + 0x04;
pub const PL_ENOBS: u32 = SEG_BASE + 0x05;
pub const PL_EMISMATCH: u32 = SEG_BASE + 0x06;
pub const PL_EBAL: u32 = SEG_BASE + 0x07;
pub const PL_ESEAL: u32 = SEG_BASE + 0x08;
pub const PL_EINVAL: u32 = SEG_BASE + 0x09;
pub const PL_EBADF: u32 = SEG_BASE + 0x0a;
pub const PL_ENOMEM: u32 = SEG_BASE + 0x0b;
pub const PL_EFAULT: u32 = SEG_BASE + 0x0c;
pub const PL_EINTR: u32 = SEG_BASE + 0x0d;
pub const PL_EOVERFLOW: u32 = SEG_BASE + 0x0e;

pub const P_IN: u8 = 0x01;
pub const P_OUT: u8 = 0x04;
pub const P_ERR: u8 = 0x08;
pub const P_HUP: u8 = 0x10;
pub const P_NVAL: u8 = 0x20;
pub const P_PRI: u8 = 0x02;
pub const REQ_MASK: u8 = P_IN | P_OUT | P_PRI;

pub const SAMPLES: usize = 14;

pub const FNV_OFF: u64 = 0xcbf2_9ce4_8422_2325;
pub const FNV_PRIME: u64 = 0x100_0000_01b3;

fn fold(h: u64, v: u64) -> u64 {
    let mut h = h;
    let mut i = 0u32;
    while i < 8 {
        h ^= (v >> (i * 8)) & 0xff;
        h = h.wrapping_mul(FNV_PRIME);
        i += 1;
    }
    h
}

/// 被轮询的描述符。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FdState {
    pub fd: u8,
    pub events: u8,
    pub open: bool,
    pub readable: bool,
    pub writable: bool,
    pub err: bool,
    pub hup: bool,
}

/// 结果快照。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Out {
    pub ready: u8,
    pub mask: u8,
    pub blocked: bool,
}

fn revents(f: &FdState) -> u8 {
    if !f.open {
        return P_NVAL;
    }
    let mut r = 0u8;
    if f.err {
        r |= P_ERR;
    }
    if f.hup {
        r |= P_HUP;
    }
    if f.events & P_IN != 0 && f.readable && !f.err {
        r |= P_IN;
    }
    if f.events & P_OUT != 0 && f.writable && !f.err {
        r |= P_OUT;
    }
    if f.events & P_PRI != 0 && f.err {
        r |= P_PRI;
    }
    r
}

/// poll 语义。
pub fn poll(fds: &[FdState], timeout: i32) -> Result<Out, u32> {
    if timeout < -1 {
        return Err(PL_EINVAL);
    }
    let mut i = 0usize;
    while i < fds.len() {
        let f = match fds.get(i) {
            Some(v) => *v,
            None => return Err(PL_EFAULT),
        };
        if f.events & !REQ_MASK != 0 {
            return Err(PL_EINVAL);
        }
        i += 1;
    }
    let mut ready = 0u8;
    let mut mask = 0u8;
    let mut k = 0usize;
    while k < fds.len() {
        if let Some(f) = fds.get(k) {
            let r = revents(f);
            if r != 0 {
                ready = ready.saturating_add(1);
                mask |= r;
            }
        }
        k += 1;
    }
    let blocked = ready == 0 && timeout != 0;
    Ok(Out { ready, mask, blocked })
}

/// 基础描述符集合。
pub fn base_fds() -> Vec<FdState> {
    let mut v = Vec::new();
    v.push(FdState { fd: 0, events: P_IN, open: true, readable: true, writable: false, err: false, hup: false });
    v.push(FdState { fd: 1, events: P_OUT, open: true, readable: false, writable: true, err: false, hup: false });
    v.push(FdState { fd: 2, events: P_IN | P_OUT, open: true, readable: false, writable: true, err: false, hup: false });
    v.push(FdState { fd: 3, events: P_IN, open: true, readable: false, writable: false, err: false, hup: true });
    v.push(FdState { fd: 4, events: P_IN, open: true, readable: true, writable: false, err: true, hup: false });
    v.push(FdState { fd: 5, events: P_IN, open: false, readable: false, writable: false, err: false, hup: false });
    v.push(FdState { fd: 6, events: 0, open: true, readable: true, writable: true, err: false, hup: false });
    v.push(FdState { fd: 7, events: P_IN, open: true, readable: false, writable: false, err: false, hup: false });
    v.push(FdState { fd: 8, events: P_IN | P_OUT, open: false, readable: false, writable: false, err: false, hup: false });
    v.push(FdState { fd: 9, events: P_IN, open: true, readable: true, writable: true, err: false, hup: false });
    v
}

fn subset(from: usize, to: usize) -> Vec<FdState> {
    let all = base_fds();
    let mut v = Vec::new();
    let mut i = from;
    while i < to {
        if let Some(f) = all.get(i) {
            v.push(*f);
        }
        i += 1;
    }
    v
}

/// 场景执行。
pub fn run_scen(id: u8) -> Result<Out, u32> {
    match id {
        0 => poll(&subset(0, 3), 0),
        1 => poll(&subset(0, 1), -1),
        2 => poll(&subset(7, 8), 0),
        3 => poll(&subset(7, 8), -1),
        4 => poll(&subset(3, 4), 0),
        5 => poll(&subset(4, 5), 0),
        6 => poll(&subset(5, 6), 0),
        7 => poll(&subset(6, 7), 0),
        8 => poll(&base_fds(), 0),
        9 => {
            let mut v = subset(1, 2);
            if let Some(f) = v.get_mut(0) {
                f.events = 0;
            }
            poll(&v, 0)
        }
        10 => {
            let mut v = subset(1, 2);
            if let Some(f) = v.get_mut(0) {
                f.events = 0x80;
            }
            poll(&v, 0)
        }
        11 => poll(&subset(9, 10), 0),
        12 => {
            let mut v = subset(0, 1);
            if let Some(f) = subset(3, 4).get(0) {
                v.push(*f);
            }
            poll(&v, 0)
        }
        13 => poll(&subset(0, 1), -2),
        _ => Err(PL_EID),
    }
}

/// 样本。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Sample {
    pub scen: u8,
    pub expect_ok: bool,
    pub ready: u8,
    pub mask: u8,
    pub blocked: bool,
    pub code: u16,
}

/// 结果。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Outcome {
    pub ok: bool,
    pub ready: u8,
    pub mask: u8,
    pub blocked: bool,
    pub code: u16,
}

/// 账本。
pub struct Ledger {
    samples: Vec<Sample>,
    outcomes: Vec<Outcome>,
    frozen: bool,
    root: u64,
    rejects: u32,
}

impl Ledger {
    pub fn new() -> Ledger {
        Ledger {
            samples: Vec::new(),
            outcomes: Vec::new(),
            frozen: false,
            root: 0,
            rejects: 0,
        }
    }
    pub fn register(&mut self, slot: u8, scen: u8, ok: bool, ready: u8, mask: u8, blocked: bool, code: u16) -> Result<(), u32> {
        if self.frozen {
            self.rejects += 1;
            return Err(PL_EFROZEN);
        }
        if (slot as usize) >= SAMPLES {
            self.rejects += 1;
            return Err(PL_EID);
        }
        if (slot as usize) < self.samples.len() {
            self.rejects += 1;
            return Err(PL_EDUP);
        }
        if (slot as usize) != self.samples.len() {
            self.rejects += 1;
            return Err(PL_EORDER);
        }
        let s = Sample { scen, expect_ok: ok, ready, mask, blocked, code };
        let out = match run_scen(scen) {
            Ok(o) => {
                if !ok || o.ready != ready || o.mask != mask || o.blocked != blocked {
                    return Err(PL_EMISMATCH);
                }
                Outcome { ok: true, ready: o.ready, mask: o.mask, blocked: o.blocked, code: 0 }
            }
            Err(e) => {
                let low = (e & 0xffff) as u16;
                if ok || low != code {
                    return Err(PL_EMISMATCH);
                }
                Outcome { ok: false, ready: 0, mask: 0, blocked: false, code: low }
            }
        };
        self.samples.push(s);
        self.outcomes.push(out);
        Ok(())
    }
    pub fn len(&self) -> usize {
        self.samples.len()
    }
    pub fn sample(&self, i: usize) -> Option<Sample> {
        self.samples.get(i).copied()
    }
    pub fn outcome(&self, i: usize) -> Option<Outcome> {
        self.outcomes.get(i).copied()
    }
    pub fn ok_count(&self) -> usize {
        let mut n = 0;
        let mut i = 0;
        while i < self.outcomes.len() {
            if let Some(o) = self.outcomes.get(i) {
                if o.ok {
                    n += 1;
                }
            }
            i += 1;
        }
        n
    }
    pub fn err_count(&self) -> usize {
        self.outcomes.len().saturating_sub(self.ok_count())
    }
    pub fn audit(&self) -> Result<(), u32> {
        if self.samples.len() != self.outcomes.len() || self.samples.is_empty() {
            return Err(PL_EBAL);
        }
        let mut i = 0;
        while i < self.samples.len() {
            let s = match self.samples.get(i) {
                Some(v) => *v,
                None => return Err(PL_ENOBS),
            };
            let o = match self.outcomes.get(i) {
                Some(v) => *v,
                None => return Err(PL_ENOBS),
            };
            if s.expect_ok != o.ok {
                return Err(PL_EMISMATCH);
            }
            match run_scen(s.scen) {
                Ok(v) => {
                    if !o.ok || v.ready != o.ready || v.mask != o.mask || v.blocked != o.blocked {
                        return Err(PL_EMISMATCH);
                    }
                }
                Err(e) => {
                    let low = (e & 0xffff) as u16;
                    if o.ok || low != o.code {
                        return Err(PL_EMISMATCH);
                    }
                }
            }
            i += 1;
        }
        Ok(())
    }
    pub fn freeze(&mut self) -> Result<u64, u32> {
        if self.frozen {
            self.rejects += 1;
            return Err(PL_EFROZEN);
        }
        self.root = self.seal();
        self.frozen = true;
        Ok(self.root)
    }
    pub fn seal(&self) -> u64 {
        let mut h = FNV_OFF;
        let mut i = 0;
        while i < self.outcomes.len() {
            if let Some(o) = self.outcomes.get(i) {
                h = fold(h, u64::from(o.ok));
                h = fold(h, o.ready as u64);
                h = fold(h, o.mask as u64);
                h = fold(h, u64::from(o.blocked));
                h = fold(h, o.code as u64);
            }
            if let Some(s) = self.samples.get(i) {
                h = fold(h, s.scen as u64);
            }
            i += 1;
        }
        fold(h, self.rejects as u64)
    }
    pub fn root(&self) -> u64 {
        self.root
    }
    pub fn replay(&self) -> Result<u64, u32> {
        let fresh = build_ledger();
        if fresh.seal() == self.seal() && fresh.ok_count() == self.ok_count() && fresh.err_count() == self.err_count() {
            Ok(fresh.seal())
        } else {
            Err(PL_ESEAL)
        }
    }
}

/// 十四行账本。
pub fn build_ledger() -> Ledger {
    let mut l = Ledger::new();
    let einval = (PL_EINVAL & 0xffff) as u16;
    let _ = l.register(0, 0, true, 3, P_IN | P_OUT, false, 0);
    let _ = l.register(1, 1, true, 1, P_IN, false, 0);
    let _ = l.register(2, 2, true, 0, 0, false, 0);
    let _ = l.register(3, 3, true, 0, 0, true, 0);
    let _ = l.register(4, 4, true, 1, P_HUP, false, 0);
    let _ = l.register(5, 5, true, 1, P_ERR, false, 0);
    let _ = l.register(6, 6, true, 1, P_NVAL, false, 0);
    let _ = l.register(7, 7, true, 0, 0, false, 0);
    let _ = l.register(8, 8, true, 8, 0x3d, false, 0);
    let _ = l.register(9, 9, true, 0, 0, false, 0);
    let _ = l.register(10, 10, false, 0, 0, false, einval);
    let _ = l.register(11, 11, true, 1, P_IN, false, 0);
    let _ = l.register(12, 12, true, 2, P_IN | P_HUP, false, 0);
    let _ = l.register(13, 13, false, 0, 0, false, einval);
    l
}

/// 四路注入。
pub fn inject_four() -> [bool; 4] {
    let mut out = [false; 4];
    let mut l = build_ledger();
    let base = l.ok_count() == 12 && l.err_count() == 2 && l.audit().is_ok();
    let rep = l.replay();
    let root = match l.freeze() {
        Ok(v) => v,
        Err(_) => 0,
    };
    let twice = l.freeze() == Err(PL_EFROZEN);
    out[0] = base && root != 0 && rep == Ok(root) && twice;

    let three = poll(&subset(0, 3), 0);
    out[1] = three.map(|o| (o.ready, o.mask, o.blocked)) == Ok((3, P_IN | P_OUT, false));

    let block = poll(&subset(7, 8), -1);
    let quick = poll(&subset(7, 8), 0);
    let events = poll(&subset(3, 6), 0);
    out[2] = block.map(|o| (o.ready, o.blocked)) == Ok((0, true))
        && quick.map(|o| (o.ready, o.blocked)) == Ok((0, false))
        && events.map(|o| o.mask) == Ok(P_HUP | P_ERR | P_NVAL);

    let mut bad = subset(0, 1);
    if let Some(f) = bad.get_mut(0) {
        f.events = 0x80;
    }
    out[3] = poll(&bad, 0) == Err(PL_EINVAL)
        && poll(&subset(0, 1), -2) == Err(PL_EINVAL)
        && poll(&[], 0).map(|o| (o.ready, o.blocked)) == Ok((0, false))
        && poll(&[], -1).map(|o| o.blocked) == Ok(true);
    out
}

/// 十三项判据。
pub fn run_unx11477_checks() -> CheckSet {
    let mut s = CheckSet::new("unx11477_poll_ready");
    let l = build_ledger();
    s.add("PL1-十四行齐备", l.len() == SAMPLES && l.sample(0).map(|x| x.scen) == Some(0), "static");
    s.add_dyn(
        "PL2-十二应二拒",
        l.ok_count() == 12 && l.err_count() == 2 && l.audit().is_ok(),
        &format!("ok={} err={}", l.ok_count(), l.err_count()),
    );
    s.add(
        "PL3-就绪计数",
        poll(&subset(0, 3), 0).map(|o| (o.ready, o.mask)) == Ok((3, P_IN | P_OUT)),
        "static",
    );
    s.add(
        "PL4-超时分类",
        poll(&subset(7, 8), -1).map(|o| o.blocked) == Ok(true)
            && poll(&subset(7, 8), 0).map(|o| o.blocked) == Ok(false)
            && poll(&subset(0, 1), -1).map(|o| o.blocked) == Ok(false),
        "static",
    );
    s.add(
        "PL5-HUP 始终报告",
        poll(&subset(3, 4), 0).map(|o| o.mask) == Ok(P_HUP),
        "static",
    );
    s.add(
        "PL6-ERR 压制读写",
        {
            let o = poll(&subset(4, 5), 0);
            let raw = match subset(4, 5).get(0) {
                Some(f) => revents(f) == P_ERR,
                None => false,
            };
            o.map(|v| v.mask) == Ok(P_ERR) && raw
        },
        "static",
    );
    s.add(
        "PL7-关闭描述符 NVAL",
        poll(&subset(5, 6), 0).map(|o| (o.ready, o.mask)) == Ok((1, P_NVAL)),
        "static",
    );
    s.add(
        "PL8-未请求事件不置位",
        poll(&subset(6, 7), 0).map(|o| (o.ready, o.mask)) == Ok((0, 0)),
        "static",
    );
    s.add(
        "PL9-混合掩码",
        poll(&base_fds(), 0).map(|o| (o.ready, o.mask)) == Ok((8, 0x3d)),
        "static",
    );
    s.add(
        "PL10-非法事件位",
        {
            let mut v = subset(0, 1);
            if let Some(f) = v.get_mut(0) {
                f.events = 0x80;
            }
            poll(&v, 0) == Err(PL_EINVAL)
        },
        "static",
    );
    s.add(
        "PL11-非法超时",
        poll(&subset(0, 1), -2) == Err(PL_EINVAL)
            && poll(&subset(0, 1), -1).is_ok(),
        "static",
    );
    s.add(
        "PL12-多事件单计数",
        {
            let mut v = subset(9, 10);
            if let Some(f) = v.get_mut(0) {
                f.hup = true;
                f.events = P_IN | P_OUT;
            }
            poll(&v, 0).map(|o| (o.ready, o.mask)) == Ok((1, P_IN | P_OUT | P_HUP))
        },
        "static",
    );
    let mut f = build_ledger();
    let rep = f.replay();
    let root = match f.freeze() {
        Ok(v) => v,
        Err(_) => 0,
    };
    let again = f.freeze() == Err(PL_EFROZEN);
    s.add_dyn(
        "PL13-冻结重放与四路",
        root != 0 && rep == Ok(root) && again && inject_four() == [true; 4],
        &format!("root={:016x}", root),
    );
    s
}

/// 摘要。
pub fn summary(l: &Ledger) -> alloc::string::String {
    format!("ok={} err={} rows={}", l.ok_count(), l.err_count(), l.len())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ready_counts_descriptors() {
        assert_eq!(poll(&subset(0, 3), 0).map(|o| (o.ready, o.mask)), Ok((3, P_IN | P_OUT)));
    }

    #[test]
    fn timeout_zero_never_blocks() {
        assert_eq!(poll(&subset(7, 8), 0).map(|o| o.blocked), Ok(false));
        assert_eq!(poll(&subset(7, 8), -1).map(|o| o.blocked), Ok(true));
    }

    #[test]
    fn hup_and_err_always_reported() {
        assert_eq!(poll(&subset(3, 4), 0).map(|o| o.mask), Ok(P_HUP));
        assert_eq!(poll(&subset(4, 5), 0).map(|o| o.mask), Ok(P_ERR));
    }

    #[test]
    fn closed_fd_is_nval() {
        assert_eq!(poll(&subset(5, 6), 0).map(|o| o.mask), Ok(P_NVAL));
    }

    #[test]
    fn unrequested_events_are_zero() {
        assert_eq!(poll(&subset(6, 7), 0).map(|o| o.ready), Ok(0));
    }

    #[test]
    fn invalid_inputs_fail() {
        let mut v = subset(0, 1);
        if let Some(f) = v.get_mut(0) {
            f.events = 0x80;
        }
        assert_eq!(poll(&v, 0), Err(PL_EINVAL));
        assert_eq!(poll(&subset(0, 1), -2), Err(PL_EINVAL));
    }

    #[test]
    fn ledger_is_consistent() {
        let l = build_ledger();
        assert_eq!(l.len(), SAMPLES);
        assert_eq!(l.ok_count(), 12);
        assert_eq!(l.err_count(), 2);
        assert!(l.audit().is_ok());
        let s = run_unx11477_checks();
        assert_eq!(s.len(), 13);
        assert_eq!(s.failed_count(), 0);
        assert_eq!(inject_four(), [true; 4]);
    }
}
