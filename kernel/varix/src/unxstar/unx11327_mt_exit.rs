//! UNX-F11327 多线程退出边界（0xea70 段）
//!
//! `exit(3)` 在多线程进程里只做「一份工作」：由调用线程把整个进程带走。其余线程
//! 被直接抹掉，**它们的线程局部析构不跑**；只有调用线程自己的 TLS 析构与进程级
//! atexit 链表会被执行。这条边界极容易被写错成「每个线程各自清理一遍」，症状是
//! 析构函数在已经半死的堆上跑，或者同一个析构被跑两遍。
//!
//! 本模块把边界压成三个动作：
//!   * `exit_thread(tid)`：单线程正常收尾，跑自己的 TLS 析构，置「可 join」；
//!   * `join(tid)`：只接受已收尾且非自己的线程，返回其析构条数；
//!   * `process_exit(caller)`：进程级收尾，只跑 caller 的析构，其余线程零析构抹除。
//!
//! 闸门次序（`process_exit`）：未知 caller → 已在退出中 → 正常收尾。`join` 的闸门
//! 是：未知 tid → 自 join → 尚未收尾。

use crate::alloc::format;
use crate::checks::CheckSet;

/// 段基址。
pub const SEG_BASE: u32 = 0xea70;

/// 线程号为零。
pub const MT_EINVAL: u32 = SEG_BASE + 0x01;
/// 线程号重复。
pub const MT_EDUP: u32 = SEG_BASE + 0x02;
/// 线程尚未收尾，不能 join。
pub const MT_EJOIN: u32 = SEG_BASE + 0x03;
/// 线程不在册。
pub const MT_ESRCH: u32 = SEG_BASE + 0x04;
/// 退出流程已在执行。
pub const MT_EREENTER: u32 = SEG_BASE + 0x05;
/// 进程已退出，不再受理。
pub const MT_EAFTER: u32 = SEG_BASE + 0x06;
/// 自 join。
pub const MT_EDEADLK: u32 = SEG_BASE + 0x07;
/// 线程表满。
pub const MT_EFULL: u32 = SEG_BASE + 0x08;
/// TLS 析构条数越界。
pub const MT_EDTOR: u32 = SEG_BASE + 0x09;
/// 账本形状不符。
pub const MT_EBAL: u32 = SEG_BASE + 0x0a;
/// 槽位乱序。
pub const MT_EORDER: u32 = SEG_BASE + 0x0b;
/// 观测值与复算不符。
pub const MT_EMISMATCH: u32 = SEG_BASE + 0x0c;
/// 冻结后重放不一致。
pub const MT_ESEAL: u32 = SEG_BASE + 0x0d;

/// 线程表容量。
pub const TMAX: usize = 8;
/// 单线程析构上限。
pub const DTOR_MAX: u8 = 8;
/// 账本格数。
pub const SAMPLES: usize = 14;
/// 线程状态：运行中。
pub const ST_RUN: u8 = 0;
/// 线程状态：已收尾（可 join）。
pub const ST_DONE: u8 = 1;
/// 线程状态：进程退出时被抹除。
pub const ST_KILLED: u8 = 2;

/// 操作码：登记线程。
pub const OP_SPAWN: u8 = 1;
/// 操作码：挂析构。
pub const OP_DTOR: u8 = 2;
/// 操作码：线程收尾。
pub const OP_THREAD_EXIT: u8 = 3;
/// 操作码：join。
pub const OP_JOIN: u8 = 4;
/// 操作码：进程退出。
pub const OP_PROC_EXIT: u8 = 5;

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

/// 一条线程。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Thread {
    /// 线程号。
    pub tid: u32,
    /// 状态。
    pub state: u8,
    /// 登记过的 TLS 析构条数。
    pub dtors: u8,
    /// 实际跑过的析构条数。
    pub ran: u8,
}

impl Thread {
    /// 空线程。
    pub const fn blank() -> Thread {
        Thread { tid: 0, state: ST_RUN, dtors: 0, ran: 0 }
    }
}

/// 线程表。
#[derive(Clone, Copy, Debug)]
pub struct Threads {
    t: [Thread; TMAX],
    n: usize,
    exits: u32,
    joins: u32,
    killed: u32,
    dtors_run: u32,
    proc_exit: bool,
}

impl Threads {
    /// 空表。
    pub const fn new() -> Threads {
        Threads {
            t: [Thread::blank(); TMAX],
            n: 0,
            exits: 0,
            joins: 0,
            killed: 0,
            dtors_run: 0,
            proc_exit: false,
        }
    }

    /// 在册线程数。
    pub fn len(&self) -> usize {
        self.n
    }

    /// 线程收尾次数。
    pub fn exits(&self) -> u32 {
        self.exits
    }

    /// join 次数。
    pub fn joins(&self) -> u32 {
        self.joins
    }

    /// 被进程退出抹除的线程数。
    pub fn killed(&self) -> u32 {
        self.killed
    }

    /// 全进程跑过的析构总数。
    pub fn dtors_run(&self) -> u32 {
        self.dtors_run
    }

    /// 进程是否已退出。
    pub fn is_dead(&self) -> bool {
        self.proc_exit
    }

    /// 第 i 条线程。
    pub fn at(&self, i: usize) -> Option<Thread> {
        if i >= self.n {
            return None;
        }
        self.t.get(i).copied()
    }

    /// 按 tid 找下标。
    pub fn index_of(&self, tid: u32) -> Option<usize> {
        let mut i = 0usize;
        while i < self.n {
            if let Some(x) = self.t.get(i) {
                if x.tid == tid {
                    return Some(i);
                }
            }
            i += 1;
        }
        None
    }

    /// 按 tid 取线程。
    pub fn get(&self, tid: u32) -> Option<Thread> {
        match self.index_of(tid) {
            Some(i) => self.t.get(i).copied(),
            None => None,
        }
    }

    /// 登记线程。
    pub fn spawn(&mut self, tid: u32) -> Result<usize, u32> {
        if tid == 0 {
            return Err(MT_EINVAL);
        }
        if self.proc_exit {
            return Err(MT_EAFTER);
        }
        if self.index_of(tid).is_some() {
            return Err(MT_EDUP);
        }
        if self.n >= TMAX {
            return Err(MT_EFULL);
        }
        if let Some(dst) = self.t.get_mut(self.n) {
            *dst = Thread { tid, state: ST_RUN, dtors: 0, ran: 0 };
        }
        self.n += 1;
        Ok(self.n)
    }

    /// 给线程挂 TLS 析构。返回该线程登记总数。
    pub fn add_dtor(&mut self, tid: u32, count: u8) -> Result<u8, u32> {
        let i = match self.index_of(tid) {
            Some(v) => v,
            None => return Err(MT_ESRCH),
        };
        if self.proc_exit {
            return Err(MT_EAFTER);
        }
        match self.t.get_mut(i) {
            Some(x) => {
                if x.state == ST_DONE || x.state == ST_KILLED {
                    return Err(MT_EREENTER);
                }
                let next = x.dtors.saturating_add(count);
                if next > DTOR_MAX {
                    return Err(MT_EDTOR);
                }
                x.dtors = next;
                Ok(next)
            }
            None => Err(MT_ESRCH),
        }
    }

    /// 单线程收尾：跑自己的 TLS 析构。返回条数。
    pub fn exit_thread(&mut self, tid: u32) -> Result<u8, u32> {
        let i = match self.index_of(tid) {
            Some(v) => v,
            None => return Err(MT_ESRCH),
        };
        if self.proc_exit {
            return Err(MT_EAFTER);
        }
        match self.t.get_mut(i) {
            Some(x) => {
                if x.state != ST_RUN {
                    return Err(MT_EREENTER);
                }
                x.state = ST_DONE;
                x.ran = x.dtors;
                let r = x.ran;
                self.exits += 1;
                self.dtors_run = self.dtors_run.saturating_add(r as u32);
                Ok(r)
            }
            None => Err(MT_ESRCH),
        }
    }

    /// join：必须已收尾、且不是自己。返回该线程跑过的析构条数。
    pub fn join(&mut self, tid: u32, self_tid: u32) -> Result<u8, u32> {
        if tid == self_tid {
            return Err(MT_EDEADLK);
        }
        let i = match self.index_of(tid) {
            Some(v) => v,
            None => return Err(MT_ESRCH),
        };
        match self.t.get(i) {
            Some(x) => {
                if x.state != ST_DONE {
                    return Err(MT_EJOIN);
                }
                let ran = x.ran;
                self.joins += 1;
                Ok(ran)
            }
            None => Err(MT_ESRCH),
        }
    }

    /// 进程级收尾：只跑 caller 的析构，其余线程零析构抹除。返回 caller 的析构条数。
    pub fn process_exit(&mut self, caller: u32) -> Result<u8, u32> {
        let ci = match self.index_of(caller) {
            Some(v) => v,
            None => return Err(MT_EINVAL),
        };
        if self.proc_exit {
            return Err(MT_EREENTER);
        }
        let mut caller_ran = 0u8;
        let mut i = 0usize;
        while i < self.n {
            let is_caller = i == ci;
            let state = match self.t.get(i) {
                Some(x) => x.state,
                None => ST_KILLED,
            };
            if is_caller {
                if let Some(x) = self.t.get_mut(i) {
                    caller_ran = x.dtors;
                    x.ran = caller_ran;
                    x.state = ST_DONE;
                }
                self.dtors_run = self.dtors_run.saturating_add(caller_ran as u32);
            } else if state == ST_RUN {
                if let Some(x) = self.t.get_mut(i) {
                    x.state = ST_KILLED;
                }
                self.killed += 1;
            }
            i += 1;
        }
        self.proc_exit = true;
        Ok(caller_ran)
    }

    /// 指纹。
    pub fn seal(&self) -> u64 {
        let mut h = FNV_OFF;
        h = fold(h, self.n as u64);
        let mut i = 0usize;
        while i < self.n {
            if let Some(x) = self.t.get(i) {
                h = fold(h, x.tid as u64);
                h = fold(h, x.state as u64);
                h = fold(h, x.dtors as u64);
                h = fold(h, x.ran as u64);
            }
            i += 1;
        }
        h = fold(h, self.exits as u64);
        h = fold(h, self.joins as u64);
        h = fold(h, self.killed as u64);
        h = fold(h, self.dtors_run as u64);
        fold(h, self.proc_exit as u64)
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

/// `Result<u8, u32>` → 样本。
fn byte_cell(slot: u8, op: u8, r: Result<u8, u32>) -> Sample {
    match r {
        Ok(v) => Sample { slot, op, ok: true, code: 0, value: v as u32 },
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
            return Err(MT_EORDER);
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
            return Err(MT_EBAL);
        }
        const VALS: [u32; 8] = [1, 2, 3, 2, 3, 2, 2, 3];
        const CODES: [u32; 6] = [MT_EINVAL, MT_EDUP, MT_EJOIN, MT_ESRCH, MT_EREENTER, MT_EAFTER];
        let mut vi = 0usize;
        let mut ci = 0usize;
        let mut i = 0usize;
        while i < self.len {
            let s = match self.rows.get(i) {
                Some(v) => *v,
                None => return Err(MT_EBAL),
            };
            if s.slot as usize != i {
                return Err(MT_EORDER);
            }
            if s.ok {
                if s.code != 0 {
                    return Err(MT_EMISMATCH);
                }
                match VALS.get(vi) {
                    Some(w) => {
                        if s.value != *w {
                            return Err(MT_EMISMATCH);
                        }
                    }
                    None => return Err(MT_EBAL),
                }
                vi += 1;
            } else {
                match CODES.get(ci) {
                    Some(w) => {
                        if s.code != *w {
                            return Err(MT_EMISMATCH);
                        }
                    }
                    None => return Err(MT_EBAL),
                }
                ci += 1;
            }
            i += 1;
        }
        if vi != 8 || ci != 6 {
            return Err(MT_EBAL);
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
            return Err(MT_EBAL);
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
            Err(MT_ESEAL)
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

/// 十四格账本：一线程正常收尾 + join，另一线程带三份析构被进程退出带走。
pub fn build_ledger() -> Ledger {
    let mut l = Ledger::new();
    let mut t = Threads::new();

    let _ = l.register(cell(0, OP_SPAWN, t.spawn(1)));
    let _ = l.register(cell(1, OP_SPAWN, t.spawn(2)));
    let _ = l.register(cell(2, OP_SPAWN, t.spawn(3)));
    let _ = l.register(byte_cell(3, OP_DTOR, t.add_dtor(1, 2)));
    let _ = l.register(byte_cell(4, OP_DTOR, t.add_dtor(2, 3)));
    let _ = l.register(byte_cell(5, OP_THREAD_EXIT, t.exit_thread(1)));
    let _ = l.register(byte_cell(6, OP_JOIN, t.join(1, 2)));
    let _ = l.register(byte_cell(7, OP_PROC_EXIT, t.process_exit(2)));

    let mut w = Threads::new();
    let _ = l.register(cell(8, OP_SPAWN, w.spawn(0)));
    let mut dup = Threads::new();
    let _ = dup.spawn(1);
    let _ = l.register(cell(9, OP_SPAWN, dup.spawn(1)));
    let _ = l.register(byte_cell(10, OP_JOIN, t.join(3, 2)));
    let _ = l.register(byte_cell(11, OP_JOIN, t.join(99, 2)));
    let _ = l.register(byte_cell(12, OP_PROC_EXIT, t.process_exit(2)));

    let mut late = Threads::new();
    let _ = late.spawn(5);
    let _ = late.process_exit(5);
    let _ = l.register(byte_cell(13, OP_DTOR, late.add_dtor(5, 1)));

    l
}

/// 四路注入。
pub fn inject_four() -> [bool; 4] {
    let mut out = [false; 4];

    let l = build_ledger();
    let rep = l.replay();
    let root = l.seal();
    out[0] = l.len() == SAMPLES && l.audit().is_ok() && rep == Ok(root) && root != 0;

    // 一路：进程退出只跑 caller 的析构，其余线程零析构抹除。
    let mut a = Threads::new();
    let _ = a.spawn(1);
    let _ = a.spawn(2);
    let _ = a.spawn(3);
    let _ = a.add_dtor(1, 4);
    let _ = a.add_dtor(2, 2);
    let _ = a.add_dtor(3, 1);
    let r = a.process_exit(2);
    out[1] = r == Ok(2)
        && a.killed() == 2
        && a.dtors_run() == 2
        && a.get(1).map(|x| x.state) == Some(ST_KILLED)
        && a.get(1).map(|x| x.ran) == Some(0)
        && a.get(3).map(|x| x.ran) == Some(0)
        && a.get(2).map(|x| x.ran) == Some(2);

    // 二路：join 闸门（自 join / 未收尾 / 不在册）。
    let mut b = Threads::new();
    let _ = b.spawn(11);
    let _ = b.spawn(12);
    let _ = b.exit_thread(11);
    let self_join = b.join(11, 11) == Err(MT_EDEADLK);
    let live_join = b.join(12, 11) == Err(MT_EJOIN);
    let ghost = b.join(13, 11) == Err(MT_ESRCH);
    let ok = b.join(11, 12) == Ok(0);
    out[2] = self_join && live_join && ghost && ok && b.joins() == 1;

    // 三路：退出之后一切登记被拒。
    let mut c = Threads::new();
    let _ = c.spawn(21);
    let _ = c.add_dtor(21, 1);
    let _ = c.process_exit(21);
    out[3] = c.spawn(22) == Err(MT_EAFTER)
        && c.add_dtor(21, 1) == Err(MT_EAFTER)
        && c.exit_thread(21) == Err(MT_EAFTER)
        && c.process_exit(21) == Err(MT_EREENTER)
        && c.is_dead();

    out
}

/// 十三项判据。
pub fn run_unx11327_checks() -> CheckSet {
    let mut s = CheckSet::new("unx11327.mt_exit");
    let l = build_ledger();

    s.add_dyn(
        "MT1-十四格八允六拒",
        l.len() == SAMPLES && l.ok_count() == 8 && l.err_count() == 6 && l.audit().is_ok(),
        &format!("n={} ok={} err={}", l.len(), l.ok_count(), l.err_count()),
    );

    s.add(
        "MT2-登记线程递增",
        l.sample(0).map(|x| x.value) == Some(1)
            && l.sample(1).map(|x| x.value) == Some(2)
            && l.sample(2).map(|x| x.value) == Some(3),
        "static",
    );

    s.add(
        "MT3-析构累加",
        l.sample(3).map(|x| x.value) == Some(2) && l.sample(4).map(|x| x.value) == Some(3),
        "static",
    );

    s.add(
        "MT4-单线程收尾跑净自己",
        l.sample(5).map(|x| x.value) == Some(2),
        "static",
    );

    s.add(
        "MT5-join 返回析构条数",
        l.sample(6).map(|x| x.value) == Some(2),
        "static",
    );

    s.add(
        "MT6-进程退出一份析构",
        l.sample(7).map(|x| x.value) == Some(3),
        "static",
    );

    s.add(
        "MT7-零号线程与重复登记",
        l.sample(8).map(|x| x.code) == Some(MT_EINVAL) && l.sample(9).map(|x| x.code) == Some(MT_EDUP),
        "static",
    );

    s.add(
        "MT8-未收尾与不在册",
        l.sample(10).map(|x| x.code) == Some(MT_EJOIN) && l.sample(11).map(|x| x.code) == Some(MT_ESRCH),
        "static",
    );

    s.add(
        "MT9-二次进程退出",
        l.sample(12).map(|x| x.code) == Some(MT_EREENTER),
        "static",
    );

    s.add(
        "MT10-退出后挂析构被拒",
        l.sample(13).map(|x| x.code) == Some(MT_EAFTER),
        "static",
    );

    s.add(
        "MT11-被抹除线程零析构",
        {
            let mut a = Threads::new();
            let _ = a.spawn(1);
            let _ = a.spawn(2);
            let _ = a.add_dtor(1, 5);
            let _ = a.process_exit(2);
            a.killed() == 1
                && a.dtors_run() == 0
                && a.get(1).map(|x| x.state) == Some(ST_KILLED)
                && a.get(1).map(|x| x.ran) == Some(0)
        },
        "static",
    );

    s.add(
        "MT12-自 join 先于未收尾",
        {
            let mut a = Threads::new();
            let _ = a.spawn(7);
            a.join(7, 7) == Err(MT_EDEADLK) && a.join(7, 8) == Err(MT_EJOIN) && a.joins() == 0
        },
        "static",
    );

    s.add(
        "MT13-冻结重放四注入",
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
        let s = run_unx11327_checks();
        assert_eq!(s.len(), 13);
        assert!(s.all_passed());
    }

    #[test]
    fn process_exit_only_runs_caller_dtors() {
        let mut a = Threads::new();
        let _ = a.spawn(1);
        let _ = a.spawn(2);
        let _ = a.add_dtor(1, 3);
        let _ = a.add_dtor(2, 1);
        assert_eq!(a.process_exit(2), Ok(1));
        assert_eq!(a.dtors_run(), 1);
        assert_eq!(a.killed(), 1);
        assert_eq!(a.get(1).map(|x| x.ran), Some(0));
    }

    #[test]
    fn join_requires_done_state() {
        let mut a = Threads::new();
        let _ = a.spawn(1);
        assert_eq!(a.join(1, 2), Err(MT_EJOIN));
        assert_eq!(a.exit_thread(1), Ok(0));
        assert_eq!(a.join(1, 1), Err(MT_EDEADLK));
        assert_eq!(a.join(1, 2), Ok(0));
        assert_eq!(a.joins(), 1);
    }

    #[test]
    fn thread_exit_is_single_shot() {
        let mut a = Threads::new();
        let _ = a.spawn(1);
        let _ = a.add_dtor(1, 2);
        assert_eq!(a.exit_thread(1), Ok(2));
        assert_eq!(a.exit_thread(1), Err(MT_EREENTER));
        assert_eq!(a.add_dtor(1, 1), Err(MT_EREENTER));
    }

    #[test]
    fn dtor_limit_is_enforced() {
        let mut a = Threads::new();
        let _ = a.spawn(1);
        assert_eq!(a.add_dtor(1, DTOR_MAX), Ok(DTOR_MAX));
        assert_eq!(a.add_dtor(1, 1), Err(MT_EDTOR));
    }

    #[test]
    fn after_exit_everything_rejected() {
        let mut a = Threads::new();
        let _ = a.spawn(1);
        let _ = a.process_exit(1);
        assert_eq!(a.spawn(2), Err(MT_EAFTER));
        assert_eq!(a.exit_thread(1), Err(MT_EAFTER));
        assert_eq!(a.process_exit(1), Err(MT_EREENTER));
        assert_eq!(a.process_exit(9), Err(MT_EINVAL));
    }

    #[test]
    fn freeze_blocks_writes() {
        let mut l = build_ledger();
        assert!(l.freeze().is_ok());
        assert!(l.register(Sample::blank()).is_err());
        assert_eq!(l.rejects(), 1);
    }
}
