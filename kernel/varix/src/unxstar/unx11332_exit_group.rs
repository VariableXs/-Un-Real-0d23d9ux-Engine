//! UNX-F11332 `exit_group` 语义与线程群终结（0xeac0 段）
//!
//! `exit(2)` 只终结调用线程，`exit_group(2)` 终结**整个线程群**——这是两者唯一
//! 但致命的差别。线程群里剩下的线程不会「继续跑完」，它们被直接抹掉；而所有
//! 在这些线程上阻塞等待的 `join` 等待者，必须在这同一次调用里被一并唤醒，否则
//! 等待者就成了永远醒不来的僵尸。
//!
//! 本模块把线程表压成四态（运行/已退/被杀/已收），并把「谁在等谁」记成一条单向
//! 等待边：
//!
//! - `exit_thread` 终结单个线程，若其名下有等待者，立刻唤醒；
//! - `join` 对运行中的目标返回 `EAGAIN`，同时把等待边挂到目标名下；
//! - `exit_group` 杀光调用者以外的运行线程，逐条唤醒等待者，最后终结调用者。
//!
//! 唤醒计数（`wakes`）与杀戮计数（`kills`）必须彼此自洽：唤醒次数永远不少于被
//! 唤醒的等待边数，杀戮数永远等于被抹掉的运行线程数。
//!
//! 闸门次序：
//! - `spawn`：零号 → 群已终结 → 重复号 → 表满；
//! - `exit_thread`：无此号 → 群已终结 → 已经不是运行态；
//! - `join`：自我等待 → 无此号 → 群已终结 → 目标未终结 → 已收尸。

use crate::alloc::format;
use crate::checks::CheckSet;

/// 段基址。
pub const SEG_BASE: u32 = 0xeac0;

/// 线程号为零。
pub const EG_EINVAL: u32 = SEG_BASE + 0x01;
/// 群已终结。
pub const EG_ERUN: u32 = SEG_BASE + 0x02;
/// 同号线程重复。
pub const EG_EDUP: u32 = SEG_BASE + 0x03;
/// 线程表满。
pub const EG_EFULL: u32 = SEG_BASE + 0x04;
/// 无此线程号。
pub const EG_ESRCH: u32 = SEG_BASE + 0x05;
/// 目标尚未终结，等待登记在案。
pub const EG_EAGAIN: u32 = SEG_BASE + 0x06;
/// 自我等待。
pub const EG_EDEADLK: u32 = SEG_BASE + 0x07;
/// 该线程已经收尸。
pub const EG_EJOIN: u32 = SEG_BASE + 0x08;
/// 账本形状不符。
pub const EG_EBAL: u32 = SEG_BASE + 0x09;
/// 槽位乱序。
pub const EG_EORDER: u32 = SEG_BASE + 0x0a;
/// 观测值与复算不符。
pub const EG_EMISMATCH: u32 = SEG_BASE + 0x0b;
/// 冻结后重放不一致。
pub const EG_ESEAL: u32 = SEG_BASE + 0x0c;
/// 观察下标越界。
pub const EG_ENOBS: u32 = SEG_BASE + 0x0d;

/// 线程容量。
pub const CAP: usize = 8;
/// 账本格数。
pub const SAMPLES: usize = 14;

/// 运行中。
pub const ST_RUN: u8 = 1;
/// 已退出。
pub const ST_DONE: u8 = 2;
/// 被杀。
pub const ST_KILLED: u8 = 3;
/// 已收尸。
pub const ST_REAPED: u8 = 4;

/// 操作码：创建。
pub const OP_SPAWN: u8 = 1;
/// 操作码：单线程退出。
pub const OP_EXIT: u8 = 2;
/// 操作码：等待。
pub const OP_JOIN: u8 = 3;
/// 操作码：整群终结。
pub const OP_GROUP: u8 = 4;
/// 操作码：读存活数。
pub const OP_COUNT: u8 = 5;

/// FNV 偏移基。
pub const FNV_OFF: u64 = 0xcbf2_9ce4_8422_2325;
/// FNV 质数。
pub const FNV_PRIME: u64 = 0x100_0000_01b3;

/// 折入一个 64 位字。
pub fn fold(h: u64, v: u64) -> u64 {
    let mut x = h;
    let mut n = v;
    let mut i = 0u32;
    while i < 8 {
        x ^= n & 0xff;
        x = x.wrapping_mul(FNV_PRIME);
        n >>= 8;
        i += 1;
    }
    x
}

/// 一个线程槽。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Thread {
    /// 线程号。
    pub tid: u32,
    /// 四态。
    pub state: u8,
    /// 正在等待它的线程号（0 表示无人等）。
    pub waiter: u32,
    /// 该线程自己是否挂在某条等待边上。
    pub blocked: bool,
}

impl Thread {
    /// 空槽。
    pub const fn blank() -> Thread {
        Thread { tid: 0, state: 0, waiter: 0, blocked: false }
    }

    /// 是否仍在运行。
    pub fn running(&self) -> bool {
        self.state == ST_RUN
    }
}

/// 线程群。
#[derive(Clone, Copy, Debug)]
pub struct Group {
    t: [Thread; CAP],
    n: usize,
    group_exited: bool,
    wakes: u32,
    kills: u32,
    exits: u32,
    reaps: u32,
}

impl Group {
    /// 空群。
    pub const fn new() -> Group {
        Group {
            t: [Thread::blank(); CAP],
            n: 0,
            group_exited: false,
            wakes: 0,
            kills: 0,
            exits: 0,
            reaps: 0,
        }
    }

    /// 线程条数（含已收尸）。
    pub fn count(&self) -> usize {
        self.n
    }

    /// 存活（运行中）线程数。
    pub fn alive(&self) -> usize {
        let mut k = 0usize;
        let mut i = 0usize;
        while i < self.n {
            if let Some(t) = self.t.get(i) {
                if t.running() {
                    k += 1;
                }
            }
            i += 1;
        }
        k
    }

    /// 群是否已终结。
    pub fn is_exited(&self) -> bool {
        self.group_exited
    }

    /// 唤醒次数。
    pub fn wake_count(&self) -> u32 {
        self.wakes
    }

    /// 杀戮次数。
    pub fn kill_count(&self) -> u32 {
        self.kills
    }

    /// 单线程退出次数。
    pub fn exit_count(&self) -> u32 {
        self.exits
    }

    /// 收尸次数。
    pub fn reap_count(&self) -> u32 {
        self.reaps
    }

    /// 第 i 个线程槽。
    pub fn thread_at(&self, i: usize) -> Option<Thread> {
        if i >= self.n {
            return None;
        }
        self.t.get(i).copied()
    }

    /// 按号查槽下标。
    fn slot(&self, tid: u32) -> Option<usize> {
        let mut i = 0usize;
        while i < self.n {
            match self.t.get(i) {
                Some(t) => {
                    if t.tid == tid {
                        return Some(i);
                    }
                }
                None => return None,
            }
            i += 1;
        }
        None
    }

    /// 按号查状态。
    pub fn state_of(&self, tid: u32) -> Result<u8, u32> {
        match self.slot(tid) {
            Some(i) => match self.t.get(i) {
                Some(t) => Ok(t.state),
                None => Err(EG_ESRCH),
            },
            None => Err(EG_ESRCH),
        }
    }

    /// 该线程名下是否挂着等待者。
    pub fn has_waiter(&self, tid: u32) -> bool {
        match self.slot(tid) {
            Some(i) => match self.t.get(i) {
                Some(t) => t.waiter != 0,
                None => false,
            },
            None => false,
        }
    }

    /// 该线程自己是否在阻塞等待。
    pub fn is_blocked(&self, tid: u32) -> bool {
        match self.slot(tid) {
            Some(i) => match self.t.get(i) {
                Some(t) => t.blocked,
                None => false,
            },
            None => false,
        }
    }

    /// 创建线程。
    pub fn spawn(&mut self, tid: u32) -> Result<usize, u32> {
        if tid == 0 {
            return Err(EG_EINVAL);
        }
        if self.group_exited {
            return Err(EG_ERUN);
        }
        if self.slot(tid).is_some() {
            return Err(EG_EDUP);
        }
        if self.n >= CAP {
            return Err(EG_EFULL);
        }
        if let Some(slot) = self.t.get_mut(self.n) {
            *slot = Thread { tid, state: ST_RUN, waiter: 0, blocked: false };
        }
        self.n += 1;
        Ok(self.n)
    }

    /// 唤醒挂在 `tid` 名下的等待者，返回是否真的唤醒了谁。
    fn wake_waiter(&mut self, tid: u32) -> bool {
        let w = match self.slot(tid) {
            Some(i) => match self.t.get(i) {
                Some(t) => t.waiter,
                None => return false,
            },
            None => return false,
        };
        if w == 0 {
            return false;
        }
        if let Some(wi) = self.slot(w) {
            if let Some(t) = self.t.get_mut(wi) {
                t.blocked = false;
            }
        }
        if let Some(i) = self.slot(tid) {
            if let Some(t) = self.t.get_mut(i) {
                t.waiter = 0;
            }
        }
        self.wakes = self.wakes.saturating_add(1);
        true
    }

    /// 单线程退出，返回退出后的存活数。
    pub fn exit_thread(&mut self, tid: u32) -> Result<usize, u32> {
        let i = match self.slot(tid) {
            Some(i) => i,
            None => return Err(EG_ESRCH),
        };
        if self.group_exited {
            return Err(EG_ERUN);
        }
        let st = match self.t.get(i) {
            Some(t) => t.state,
            None => return Err(EG_ESRCH),
        };
        if st != ST_RUN {
            return Err(EG_EAGAIN);
        }
        if let Some(t) = self.t.get_mut(i) {
            t.state = ST_DONE;
        }
        self.exits = self.exits.saturating_add(1);
        self.wake_waiter(tid);
        Ok(self.alive())
    }

    /// 等待目标终结：未终结则登记等待边并返回 `EAGAIN`。
    pub fn join(&mut self, self_tid: u32, target: u32) -> Result<u32, u32> {
        if self_tid == target {
            return Err(EG_EDEADLK);
        }
        let si = match self.slot(self_tid) {
            Some(i) => i,
            None => return Err(EG_ESRCH),
        };
        let ti = match self.slot(target) {
            Some(i) => i,
            None => return Err(EG_ESRCH),
        };
        if self.group_exited {
            return Err(EG_ERUN);
        }
        let tst = match self.t.get(ti) {
            Some(t) => t.state,
            None => return Err(EG_ESRCH),
        };
        if tst == ST_RUN {
            if let Some(t) = self.t.get_mut(ti) {
                t.waiter = self_tid;
            }
            if let Some(t) = self.t.get_mut(si) {
                t.blocked = true;
            }
            return Err(EG_EAGAIN);
        }
        if tst == ST_REAPED {
            return Err(EG_EJOIN);
        }
        if let Some(t) = self.t.get_mut(ti) {
            t.state = ST_REAPED;
        }
        self.reaps = self.reaps.saturating_add(1);
        Ok(tst as u32)
    }

    /// 整群终结：杀光调用者以外的运行线程，逐个唤醒等待者，最后终结调用者。
    pub fn exit_group(&mut self, caller: u32) -> Result<usize, u32> {
        if caller == 0 {
            return Err(EG_EINVAL);
        }
        if self.group_exited {
            return Err(EG_ERUN);
        }
        if self.slot(caller).is_none() {
            return Err(EG_ESRCH);
        }
        let mut done = 1usize;
        let mut i = 0usize;
        while i < self.n {
            let tid = match self.t.get(i) {
                Some(t) => t.tid,
                None => break,
            };
            if tid == caller {
                i += 1;
                continue;
            }
            let run = match self.t.get(i) {
                Some(t) => t.running(),
                None => false,
            };
            if run {
                if let Some(t) = self.t.get_mut(i) {
                    t.state = ST_KILLED;
                }
                self.kills = self.kills.saturating_add(1);
                done += 1;
                self.wake_waiter(tid);
            }
            i += 1;
        }
        if let Some(ci) = self.slot(caller) {
            if let Some(t) = self.t.get_mut(ci) {
                t.state = ST_DONE;
            }
        }
        self.wake_waiter(caller);
        self.group_exited = true;
        Ok(done)
    }
}

/// 一个账本样本。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Sample {
    /// 槽位。
    pub slot: u8,
    /// 操作码。
    pub op: u8,
    /// 是否成功。
    pub ok: bool,
    /// 错误码。
    pub code: u32,
    /// 观测值。
    pub value: u32,
}

impl Sample {
    /// 空白格。
    pub const fn blank() -> Sample {
        Sample { slot: 0, op: 0, ok: false, code: 0, value: 0 }
    }
}

/// `Result<usize, u32>` → 样本。
fn cell(slot: u8, op: u8, r: Result<usize, u32>) -> Sample {
    match r {
        Ok(v) => Sample { slot, op, ok: true, code: 0, value: v as u32 },
        Err(c) => Sample { slot, op, ok: false, code: c, value: 0 },
    }
}

/// `Result<u32, u32>` → 样本。
fn word_cell(slot: u8, op: u8, r: Result<u32, u32>) -> Sample {
    match r {
        Ok(v) => Sample { slot, op, ok: true, code: 0, value: v },
        Err(c) => Sample { slot, op, ok: false, code: c, value: 0 },
    }
}

/// 样本账本。
#[derive(Clone, Copy, Debug)]
pub struct Ledger {
    rows: [Sample; SAMPLES],
    len: usize,
    root: u64,
    frozen: bool,
    rejects: u32,
}

impl Ledger {
    /// 空账本。
    pub const fn new() -> Ledger {
        Ledger { rows: [Sample::blank(); SAMPLES], len: 0, root: 0, frozen: false, rejects: 0 }
    }

    /// 登记一格。
    pub fn register(&mut self, s: Sample) -> Result<(), u32> {
        if self.frozen || (s.slot as usize) != self.len || (s.slot as usize) >= SAMPLES {
            self.rejects += 1;
            return Err(EG_EORDER);
        }
        if let Some(dst) = self.rows.get_mut(self.len) {
            *dst = s;
        }
        self.len += 1;
        Ok(())
    }

    /// 格数。
    pub fn len(&self) -> usize {
        self.len
    }

    /// 是否空账。
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// 第 i 格。
    pub fn sample(&self, i: usize) -> Option<Sample> {
        if i >= self.len {
            return None;
        }
        self.rows.get(i).copied()
    }

    /// 成功格数。
    pub fn ok_count(&self) -> usize {
        let mut n = 0usize;
        let mut i = 0usize;
        while i < self.len {
            if let Some(s) = self.rows.get(i) {
                if s.ok {
                    n += 1;
                }
            }
            i += 1;
        }
        n
    }

    /// 失败格数。
    pub fn err_count(&self) -> usize {
        self.len - self.ok_count()
    }

    /// 对账。
    pub fn audit(&self) -> Result<(), u32> {
        if self.len != SAMPLES || self.ok_count() != 8 || self.err_count() != 6 {
            return Err(EG_EBAL);
        }
        const VALS: [u32; 8] = [1, 2, 1, 2, 3, 4, 3, 0];
        const CODES: [u32; 6] =
            [EG_EINVAL, EG_EDUP, EG_EFULL, EG_ESRCH, EG_EAGAIN, EG_ERUN];
        let mut vi = 0usize;
        let mut ci = 0usize;
        let mut i = 0usize;
        while i < self.len {
            let s = match self.rows.get(i) {
                Some(v) => *v,
                None => return Err(EG_EBAL),
            };
            if s.slot as usize != i {
                return Err(EG_EORDER);
            }
            if s.ok {
                if s.code != 0 {
                    return Err(EG_EMISMATCH);
                }
                match VALS.get(vi) {
                    Some(w) => {
                        if s.value != *w {
                            return Err(EG_EMISMATCH);
                        }
                    }
                    None => return Err(EG_EBAL),
                }
                vi += 1;
            } else {
                match CODES.get(ci) {
                    Some(w) => {
                        if s.code != *w {
                            return Err(EG_EMISMATCH);
                        }
                    }
                    None => return Err(EG_EBAL),
                }
                ci += 1;
            }
            i += 1;
        }
        if vi != 8 || ci != 6 {
            return Err(EG_EBAL);
        }
        Ok(())
    }

    /// 指纹。
    pub fn seal(&self) -> u64 {
        let mut h = FNV_OFF;
        let mut i = 0usize;
        while i < self.len {
            if let Some(s) = self.rows.get(i) {
                h = fold(h, s.slot as u64);
                h = fold(h, s.op as u64);
                h = fold(h, s.ok as u64);
                h = fold(h, s.code as u64);
                h = fold(h, s.value as u64);
            }
            i += 1;
        }
        fold(h, self.rejects as u64)
    }

    /// 冻结。
    pub fn freeze(&mut self) -> Result<u64, u32> {
        if self.frozen {
            self.rejects += 1;
            return Err(EG_EBAL);
        }
        self.root = self.seal();
        self.frozen = true;
        Ok(self.root)
    }

    /// 重放。
    pub fn replay(&self) -> Result<u64, u32> {
        let fresh = build_ledger();
        if fresh.seal() == self.seal() {
            Ok(fresh.seal())
        } else {
            Err(EG_ESEAL)
        }
    }

    /// 冻结指纹。
    pub fn root(&self) -> u64 {
        self.root
    }

    /// 是否冻结。
    pub fn is_frozen(&self) -> bool {
        self.frozen
    }

    /// 拒绝计数。
    pub fn rejects(&self) -> u32 {
        self.rejects
    }
}

/// 十四格账本：三线程开局 → 单退 → 收尸 → 再建两个 → 整群终结 → 查存活 → 各类拒绝。
pub fn build_ledger() -> Ledger {
    let mut l = Ledger::new();
    let mut g = Group::new();

    let _ = l.register(cell(0, OP_SPAWN, g.spawn(2)));
    let _ = l.register(cell(1, OP_SPAWN, g.spawn(3)));
    let _ = l.register(cell(2, OP_EXIT, g.exit_thread(2)));
    let _ = l.register(word_cell(3, OP_JOIN, g.join(3, 2)));
    let _ = l.register(cell(4, OP_SPAWN, g.spawn(4)));
    let _ = l.register(cell(5, OP_SPAWN, g.spawn(5)));
    let _ = l.register(cell(6, OP_GROUP, g.exit_group(5)));
    let _ = l.register(cell(7, OP_COUNT, Ok(g.alive())));

    let mut e = Group::new();
    let _ = l.register(cell(8, OP_SPAWN, e.spawn(0)));

    let mut dup = Group::new();
    let _ = dup.spawn(2);
    let _ = l.register(cell(9, OP_SPAWN, dup.spawn(2)));

    let mut full = Group::new();
    let mut i = 1u32;
    while (i as usize) <= CAP {
        let _ = full.spawn(i);
        i += 1;
    }
    let _ = l.register(cell(10, OP_SPAWN, full.spawn(0x100)));

    let mut n = Group::new();
    let _ = l.register(cell(11, OP_EXIT, n.exit_thread(7)));

    let mut m = Group::new();
    let _ = m.spawn(2);
    let _ = m.spawn(3);
    let _ = l.register(word_cell(12, OP_JOIN, m.join(3, 2)));

    let _ = l.register(cell(13, OP_GROUP, g.exit_group(2)));

    l
}

/// 四路注入。
pub fn inject_four() -> [bool; 4] {
    let mut out = [false; 4];

    let l = build_ledger();
    let rep = l.replay();
    let root = l.seal();
    out[0] = l.len() == SAMPLES && l.audit().is_ok() && rep == Ok(root) && root != 0;

    // 一路：等待边登记后被唤醒，唤醒后可收尸。
    let mut a = Group::new();
    let _ = a.spawn(2);
    let _ = a.spawn(3);
    let e1 = a.join(3, 2);
    let blocked = a.is_blocked(3) && a.has_waiter(2);
    let _ = a.exit_thread(2);
    let woken = !a.is_blocked(3) && a.wake_count() == 1;
    let e2 = a.join(3, 2);
    out[1] = e1 == Err(EG_EAGAIN)
        && blocked
        && woken
        && e2 == Ok(ST_DONE as u32)
        && a.state_of(2) == Ok(ST_REAPED)
        && a.reap_count() == 1;

    // 二路：整群终结杀光其余线程并唤醒其名下等待者。
    let mut b = Group::new();
    let _ = b.spawn(2);
    let _ = b.spawn(3);
    let _ = b.spawn(4);
    let _ = b.join(3, 4);
    let killed = b.exit_group(2);
    out[2] = killed == Ok(3)
        && b.is_exited()
        && b.alive() == 0
        && b.kill_count() == 2
        && b.wake_count() == 1
        && !b.is_blocked(3)
        && b.state_of(4) == Ok(ST_KILLED)
        && b.state_of(2) == Ok(ST_DONE)
        && b.exit_group(2) == Err(EG_ERUN)
        && b.spawn(9) == Err(EG_ERUN);

    // 三路：闸门（自等 / 无号 / 满 / 重号）。
    let mut g = Group::new();
    let g1 = g.join(1, 1) == Err(EG_EDEADLK);
    let g2 = g.exit_thread(1) == Err(EG_ESRCH);
    let g3 = g.spawn(0) == Err(EG_EINVAL);
    let g4 = g.spawn(1) == Ok(1);
    let g5 = g.spawn(1) == Err(EG_EDUP);
    let mut h = Group::new();
    let mut i = 1u32;
    while (i as usize) <= CAP {
        let _ = h.spawn(i);
        i += 1;
    }
    let g6 = h.spawn(0x100) == Err(EG_EFULL);
    let g7 = h.exit_thread(1) == Ok(CAP - 1);
    let g8 = h.exit_thread(1) == Err(EG_EAGAIN);
    out[3] = g1 && g2 && g3 && g4 && g5 && g6 && g7 && g8;

    out
}

/// 十三项判据。
pub fn run_unx11332_checks() -> CheckSet {
    let mut s = CheckSet::new("unx11332.exit_group");
    let l = build_ledger();

    s.add_dyn(
        "EG1-十四格八允六拒",
        l.len() == SAMPLES && l.ok_count() == 8 && l.err_count() == 6 && l.audit().is_ok(),
        &format!("n={} ok={} err={}", l.len(), l.ok_count(), l.err_count()),
    );

    s.add(
        "EG2-创建计数递增",
        l.sample(0).map(|x| x.value) == Some(1) && l.sample(1).map(|x| x.value) == Some(2),
        "static",
    );

    s.add(
        "EG3-单线程退出后的存活数",
        l.sample(2).map(|x| x.value) == Some(1),
        "static",
    );

    s.add(
        "EG4-收尸返回终态",
        l.sample(3).map(|x| x.value) == Some(ST_DONE as u32),
        "static",
    );

    s.add(
        "EG5-续建两线程",
        l.sample(4).map(|x| x.value) == Some(3) && l.sample(5).map(|x| x.value) == Some(4),
        "static",
    );

    s.add(
        "EG6-整群终结计数含调用者",
        l.sample(6).map(|x| x.value) == Some(3),
        "static",
    );

    s.add(
        "EG7-终结后零存活",
        l.sample(7).map(|x| x.value) == Some(0),
        "static",
    );

    s.add(
        "EG8-零号创建被拒",
        l.sample(8).map(|x| x.code) == Some(EG_EINVAL),
        "static",
    );

    s.add(
        "EG9-重号与表满",
        l.sample(9).map(|x| x.code) == Some(EG_EDUP)
            && l.sample(10).map(|x| x.code) == Some(EG_EFULL),
        "static",
    );

    s.add(
        "EG10-无号退出与运行中等待",
        l.sample(11).map(|x| x.code) == Some(EG_ESRCH)
            && l.sample(12).map(|x| x.code) == Some(EG_EAGAIN),
        "static",
    );

    s.add(
        "EG11-重复整群终结被拒",
        l.sample(13).map(|x| x.code) == Some(EG_ERUN),
        "static",
    );

    s.add(
        "EG12-等待边与唤醒计数自洽",
        {
            let mut a = Group::new();
            let _ = a.spawn(2);
            let _ = a.spawn(3);
            let _ = a.spawn(4);
            let e1 = a.join(3, 2);
            let e2 = a.join(4, 2);
            let _ = a.exit_thread(2);
            e1 == Err(EG_EAGAIN)
                && e2 == Err(EG_EAGAIN)
                && a.wake_count() == 1
                && !a.is_blocked(4)
                && a.has_waiter(2) == false
                && a.join(3, 2) == Ok(ST_DONE as u32)
        },
        "static",
    );

    s.add(
        "EG13-冻结重放四注入",
        {
            let mut f = build_ledger();
            let rep = f.replay();
            let one = f.freeze();
            let two = f.freeze();
            let inj = inject_four();
            one.is_ok()
                && one == rep
                && two.is_err()
                && f.is_frozen()
                && f.root() != 0
                && f.rejects() == 1
                && inj == [true; 4]
        },
        "static",
    );

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ledger_shape() {
        let l = build_ledger();
        assert_eq!(l.len(), SAMPLES);
        assert_eq!(l.ok_count(), 8);
        assert_eq!(l.err_count(), 6);
        assert!(l.audit().is_ok());
    }

    #[test]
    fn checks_pass() {
        let s = run_unx11332_checks();
        assert_eq!(s.len(), 13);
        assert!(s.all_passed());
    }

    #[test]
    fn single_exit_then_reap() {
        let mut g = Group::new();
        let _ = g.spawn(2);
        let _ = g.spawn(3);
        assert_eq!(g.exit_thread(2), Ok(1));
        assert_eq!(g.state_of(2), Ok(ST_DONE));
        assert_eq!(g.join(3, 2), Ok(ST_DONE as u32));
        assert_eq!(g.state_of(2), Ok(ST_REAPED));
        assert_eq!(g.join(3, 2), Err(EG_EJOIN));
    }

    #[test]
    fn group_exit_kills_others() {
        let mut g = Group::new();
        let _ = g.spawn(2);
        let _ = g.spawn(3);
        let _ = g.spawn(4);
        assert_eq!(g.exit_group(2), Ok(3));
        assert_eq!(g.alive(), 0);
        assert_eq!(g.kill_count(), 2);
        assert_eq!(g.state_of(3), Ok(ST_KILLED));
        assert_eq!(g.state_of(4), Ok(ST_KILLED));
        assert!(g.is_exited());
    }

    #[test]
    fn waiting_edge_is_registered() {
        let mut g = Group::new();
        let _ = g.spawn(2);
        let _ = g.spawn(3);
        assert_eq!(g.join(3, 2), Err(EG_EAGAIN));
        assert!(g.has_waiter(2));
        assert!(g.is_blocked(3));
        assert_eq!(g.exit_thread(2), Ok(1));
        assert!(!g.is_blocked(3));
        assert!(!g.has_waiter(2));
        assert_eq!(g.wake_count(), 1);
    }

    #[test]
    fn gates() {
        let mut g = Group::new();
        assert_eq!(g.spawn(0), Err(EG_EINVAL));
        assert_eq!(g.exit_thread(1), Err(EG_ESRCH));
        assert_eq!(g.join(1, 1), Err(EG_EDEADLK));
        assert_eq!(g.spawn(4), Ok(1));
        assert_eq!(g.spawn(4), Err(EG_EDUP));
        assert_eq!(g.exit_thread(4), Ok(0));
        assert_eq!(g.exit_thread(4), Err(EG_EAGAIN));
        assert_eq!(g.exit_group(0), Err(EG_EINVAL));
        assert_eq!(g.exit_group(9), Err(EG_ESRCH));
        assert_eq!(g.exit_group(4), Ok(1));
        assert_eq!(g.exit_group(4), Err(EG_ERUN));
    }

    #[test]
    fn capacity_is_exact() {
        let mut g = Group::new();
        let mut i = 1u32;
        while (i as usize) <= CAP {
            assert!(g.spawn(i).is_ok());
            i += 1;
        }
        assert_eq!(g.spawn(0xFFFF), Err(EG_EFULL));
        assert_eq!(g.count(), CAP);
    }

    #[test]
    fn kill_wakes_waiter() {
        let mut g = Group::new();
        let _ = g.spawn(2);
        let _ = g.spawn(3);
        let _ = g.spawn(4);
        assert_eq!(g.join(3, 4), Err(EG_EAGAIN));
        assert_eq!(g.exit_group(2), Ok(3));
        assert!(!g.is_blocked(3));
        assert_eq!(g.wake_count(), 1);
        assert_eq!(g.state_of(4), Ok(ST_KILLED));
    }

    #[test]
    fn freeze_blocks_writes() {
        let mut l = build_ledger();
        assert!(l.freeze().is_ok());
        assert!(l.register(Sample::blank()).is_err());
        assert_eq!(l.rejects(), 1);
    }
}
