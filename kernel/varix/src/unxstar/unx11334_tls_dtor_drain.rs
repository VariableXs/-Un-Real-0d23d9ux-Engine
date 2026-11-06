//! UNX-F11334 退出时 TLS dtor 消费（0xeae0 段）
//!
//! 线程局部存储（TLS）的析构不是一次遍历就能收干净的：析构函数自己完全
//! 可能把新的析构指针写回本线程的 dtor 槽（标准库的 `thread_local!` 在
//! 析构闭包里再碰别的 TLS 就会发生这件事）。POSIX 因此给了迭代上限
//! `PTHREAD_DESTRUCTOR_ITERATIONS`（glibc/musl 都取 4）：每趟扫一遍全部
//! 槽位，把非零值取走并调用，最多四趟；四趟之后仍有残留，就带着残值终止，
//! 不能无限重扫。
//!
//! 本模块把这件事压成一台显式状态机：
//!
//! - 槽值 0 表示空；登记非零值算一次「待销毁」；
//! - `pass_once` 一趟按**槽位升序**扫描，取出即清零，调用记入调用序列；
//! - 值 `77` 是「一次性重挂」令牌：被取走时会往 2 号槽写入 88。因为扫描
//!   顺序是升序、2 号槽已经扫过，新值必然留到下一趟——这正是「析构中登记
//!   的析构要下一轮才跑」的最小可观测证据；
//! - 值 `99` 是「常驻」令牌：每趟被取走后立刻原槽重挂，用来把迭代上限逼
//!   到台面上（第 5 趟返回 `DT_EITER`）。
//!
//! 三套闸门必须分清楚：
//!
//! - 登记 `set`：下标越界 → 线程已终结 → 零值 → 槽位已占；
//! - 单槽 `consume`：下标越界 → 线程已终结 → 槽位已空；
//! - 整趟 `drain_pass`：线程已终结 → 迭代已满 → 无待销毁项。
//!
//! 注意 `set` 里「零值」排在「槽位已占」之前：往一个已有值的槽写 0，语义
//! 上不是「冲突」，而是「用非法值登记」——先报参数错误，诊断才稳定。
//!
//! 线程终结 `thread_exit` 会一路跑到干净或迭代上限；无论成败都把线程标成
//! 已终结，因为线程本身不会因为析构没跑完而复活。

use crate::alloc::format;
use crate::checks::CheckSet;

/// 段基址。
pub const SEG_BASE: u32 = 0xeae0;

/// 槽位下标越界。
pub const DT_EBOUND: u32 = SEG_BASE + 0x01;
/// 线程已终结。
pub const DT_EDEAD: u32 = SEG_BASE + 0x02;
/// 零不是登记值。
pub const DT_EZERO: u32 = SEG_BASE + 0x03;
/// 槽位已被占用。
pub const DT_EBUSY: u32 = SEG_BASE + 0x04;
/// 线程不可用。
pub const DT_ETHREAD: u32 = SEG_BASE + 0x05;
/// 槽位为空。
pub const DT_EEMPTY: u32 = SEG_BASE + 0x06;
/// 迭代上限已到。
pub const DT_EITER: u32 = SEG_BASE + 0x07;
/// 调用序列下标越界。
pub const DT_ENOBS: u32 = SEG_BASE + 0x08;
/// 账本失衡。
pub const DT_EBAL: u32 = SEG_BASE + 0x09;
/// 槽位乱序。
pub const DT_EORDER: u32 = SEG_BASE + 0x0a;
/// 观测值与复算不符。
pub const DT_EMISMATCH: u32 = SEG_BASE + 0x0b;
/// 冻结后重放不一致。
pub const DT_ESEAL: u32 = SEG_BASE + 0x0c;
/// 令牌语义非法。
pub const DT_ETOKEN: u32 = SEG_BASE + 0x0d;

/// 槽位容量。
pub const CAP: usize = 6;
/// 析构迭代上限（与 PTHREAD_DESTRUCTOR_ITERATIONS 同值）。
pub const ITERATIONS: u8 = 4;
/// 调用序列容量。
pub const CALL_CAP: usize = 12;
/// 账本格数。
pub const SAMPLES: usize = 14;

/// 一次性重挂令牌。
pub const TOKEN_RESPAWN: u32 = 77;
/// 重挂后写回的值。
pub const RESPAWN_VALUE: u32 = 88;
/// 常驻令牌。
pub const TOKEN_PERPETUAL: u32 = 99;
/// 重挂落点槽位（小于令牌常规落点，保证延后一趟才被看到）。
pub const RESPAWN_SLOT: usize = 2;

/// 操作码：登记。
pub const OP_SET: u8 = 1;
/// 操作码：整趟消耗。
pub const OP_PASS: u8 = 2;
/// 操作码：读待销毁数。
pub const OP_PENDING: u8 = 3;
/// 操作码：线程终结。
pub const OP_EXIT: u8 = 4;
/// 操作码：读调用总数。
pub const OP_CALLS: u8 = 5;
/// 操作码：读已跑趟数。
pub const OP_PASSES: u8 = 6;

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

/// 单线程的 TLS 析构槽组。
#[derive(Clone, Copy, Debug)]
pub struct Tls {
    vals: [u32; CAP],
    calls: [u32; CALL_CAP],
    pending: usize,
    ncalls: usize,
    passes: u8,
    alive: bool,
    exited: bool,
    respawned: bool,
}

impl Tls {
    /// 新线程，槽全空。
    pub const fn new() -> Tls {
        Tls {
            vals: [0; CAP],
            calls: [0; CALL_CAP],
            pending: 0,
            ncalls: 0,
            passes: 0,
            alive: true,
            exited: false,
            respawned: false,
        }
    }

    /// 线程是否仍可登记。
    pub fn is_alive(&self) -> bool {
        self.alive
    }

    /// 是否已经走过终结流程。
    pub fn is_exited(&self) -> bool {
        self.exited
    }

    /// 待销毁件数。
    pub fn pending(&self) -> usize {
        self.pending
    }

    /// 已跑趟数。
    pub fn passes(&self) -> u8 {
        self.passes
    }

    /// 已调用析构总数。
    pub fn total_calls(&self) -> usize {
        self.ncalls
    }

    /// 第 i 个槽值。
    pub fn value_at(&self, i: usize) -> Result<u32, u32> {
        if i >= CAP {
            return Err(DT_ENOBS);
        }
        match self.vals.get(i) {
            Some(v) => Ok(*v),
            None => Err(DT_ENOBS),
        }
    }

    /// 第 i 次调用的令牌值。
    pub fn call_at(&self, i: usize) -> Result<u32, u32> {
        if i >= self.ncalls {
            return Err(DT_ENOBS);
        }
        match self.calls.get(i) {
            Some(v) => Ok(*v),
            None => Err(DT_ENOBS),
        }
    }

    /// 写一个槽值；成功返回写入后的待销毁数。
    pub fn set(&mut self, slot: usize, v: u32) -> Result<usize, u32> {
        if slot >= CAP {
            return Err(DT_EBOUND);
        }
        if !self.alive {
            return Err(DT_EDEAD);
        }
        if v == 0 {
            return Err(DT_EZERO);
        }
        let occupied = match self.vals.get(slot) {
            Some(x) => *x != 0,
            None => return Err(DT_EBOUND),
        };
        if occupied {
            return Err(DT_EBUSY);
        }
        if let Some(x) = self.vals.get_mut(slot) {
            *x = v;
        }
        self.pending += 1;
        Ok(self.pending)
    }

    /// 内部重挂：仅当落点为空时成功。
    fn place(&mut self, slot: usize, v: u32) -> bool {
        if slot >= CAP || v == 0 {
            return false;
        }
        match self.vals.get(slot) {
            Some(x) => {
                if *x != 0 {
                    return false;
                }
            }
            None => return false,
        }
        if let Some(x) = self.vals.get_mut(slot) {
            *x = v;
        }
        self.pending += 1;
        true
    }

    /// 记一次调用。
    fn record_call(&mut self, v: u32) {
        if self.ncalls < CALL_CAP {
            if let Some(s) = self.calls.get_mut(self.ncalls) {
                *s = v;
            }
            self.ncalls += 1;
        }
    }

    /// 单槽取走（显式消费一件）。
    pub fn consume(&mut self, slot: usize) -> Result<u32, u32> {
        if slot >= CAP {
            return Err(DT_EBOUND);
        }
        if !self.alive {
            return Err(DT_EDEAD);
        }
        let v = match self.vals.get(slot) {
            Some(x) => *x,
            None => return Err(DT_EBOUND),
        };
        if v == 0 {
            return Err(DT_EEMPTY);
        }
        if let Some(x) = self.vals.get_mut(slot) {
            *x = 0;
        }
        self.pending = self.pending.saturating_sub(1);
        self.record_call(v);
        Ok(v)
    }

    /// 一趟扫描（不做线程/迭代闸门，供 `thread_exit` 复用）。
    ///
    /// 返回本趟实际取走的件数。扫描严格按槽位升序；重挂落点 `RESPAWN_SLOT`
    /// 小于常规令牌落点，所以新值不会在本趟被再次看到。
    fn pass_once(&mut self) -> usize {
        let mut consumed = 0usize;
        let mut i = 0usize;
        while i < CAP {
            let v = match self.vals.get(i) {
                Some(x) => *x,
                None => 0,
            };
            if v != 0 {
                if let Some(x) = self.vals.get_mut(i) {
                    *x = 0;
                }
                self.pending = self.pending.saturating_sub(1);
                self.record_call(v);
                consumed += 1;
                if v == TOKEN_RESPAWN {
                    if !self.respawned {
                        self.respawned = true;
                        let _ = self.place(RESPAWN_SLOT, RESPAWN_VALUE);
                    }
                } else if v == TOKEN_PERPETUAL && self.ncalls < CALL_CAP {
                    let _ = self.place(RESPAWN_SLOT, TOKEN_PERPETUAL);
                }
            }
            i += 1;
        }
        self.passes = self.passes.saturating_add(1);
        consumed
    }

    /// 一趟受闸门约束的析构扫描。
    pub fn drain_pass(&mut self) -> Result<usize, u32> {
        if !self.alive {
            return Err(DT_ETHREAD);
        }
        if self.passes >= ITERATIONS {
            return Err(DT_EITER);
        }
        if self.pending == 0 {
            return Err(DT_EEMPTY);
        }
        Ok(self.pass_once())
    }

    /// 线程终结：跑到干净或迭代上限，返回调用总数。
    pub fn thread_exit(&mut self) -> Result<usize, u32> {
        if !self.alive {
            return Err(DT_EDEAD);
        }
        self.alive = false;
        self.exited = true;
        while self.pending > 0 && self.passes < ITERATIONS {
            let _ = self.pass_once();
        }
        if self.pending > 0 {
            Err(DT_EITER)
        } else {
            Ok(self.ncalls)
        }
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
            return Err(DT_EORDER);
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
            return Err(DT_EBAL);
        }
        const VALS: [u32; 8] = [1, 2, 3, 3, 1, 4, 4, 2];
        const CODES: [u32; 6] = [
            DT_EBUSY,
            DT_EBOUND,
            DT_EZERO,
            DT_ETHREAD,
            DT_EDEAD,
            DT_EEMPTY,
        ];
        let mut vi = 0usize;
        let mut ci = 0usize;
        let mut i = 0usize;
        while i < self.len {
            let s = match self.rows.get(i) {
                Some(v) => *v,
                None => return Err(DT_EBAL),
            };
            if s.slot as usize != i {
                return Err(DT_EORDER);
            }
            if s.ok {
                if s.code != 0 {
                    return Err(DT_EMISMATCH);
                }
                match VALS.get(vi) {
                    Some(w) => {
                        if s.value != *w {
                            return Err(DT_EMISMATCH);
                        }
                    }
                    None => return Err(DT_EBAL),
                }
                vi += 1;
            } else {
                match CODES.get(ci) {
                    Some(w) => {
                        if s.code != *w {
                            return Err(DT_EMISMATCH);
                        }
                    }
                    None => return Err(DT_EBAL),
                }
                ci += 1;
            }
            i += 1;
        }
        if vi != 8 || ci != 6 {
            return Err(DT_EBAL);
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
            return Err(DT_EBAL);
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
            Err(DT_ESEAL)
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

/// 十四格账本：两个槽登记 → 冲突 → 越界/零值 → 重挂令牌 → 一趟 → 二趟 → 终结 → 各类死路。
pub fn build_ledger() -> Ledger {
    let mut l = Ledger::new();
    let mut t = Tls::new();

    let _ = l.register(cell(0, OP_SET, t.set(0, 11)));
    let _ = l.register(cell(1, OP_SET, t.set(0, 12)));
    let _ = l.register(cell(2, OP_SET, t.set(1, 22)));
    let _ = l.register(cell(3, OP_SET, t.set(6, 1)));
    let _ = l.register(cell(4, OP_SET, t.set(2, 0)));
    let _ = l.register(cell(5, OP_SET, t.set(5, TOKEN_RESPAWN)));
    let _ = l.register(cell(6, OP_PASS, t.drain_pass()));
    let _ = l.register(cell(7, OP_PENDING, Ok(t.pending())));
    let _ = l.register(cell(8, OP_EXIT, t.thread_exit()));
    let _ = l.register(cell(9, OP_PASS, t.drain_pass()));
    let _ = l.register(cell(10, OP_SET, t.set(3, 9)));
    let _ = l.register(cell(11, OP_PASS, Tls::new().drain_pass()));
    let _ = l.register(cell(12, OP_CALLS, Ok(t.total_calls())));
    let _ = l.register(cell(13, OP_PASSES, Ok(t.passes() as usize)));

    l
}

/// 四路注入。
pub fn inject_four() -> [bool; 4] {
    let mut out = [false; 4];

    let l = build_ledger();
    let rep = l.replay();
    let root = l.seal();
    out[0] = l.len() == SAMPLES && l.audit().is_ok() && rep == Ok(root) && root != 0;

    // 一路：一次性重挂必须延后一趟。
    let mut a = Tls::new();
    let e1 = a.set(5, TOKEN_RESPAWN);
    let p1 = a.drain_pass();
    let p2 = a.pending();
    // 重挂槽必须**读在第二趟之前**：第二趟会把它清零，事后再比就永远不等。
    let v2 = a.value_at(RESPAWN_SLOT);
    let p3 = a.drain_pass();
    let p4 = a.pending();
    out[1] = e1 == Ok(1)
        && p1 == Ok(1)
        && p2 == 1
        && v2 == Ok(RESPAWN_VALUE)
        && p3 == Ok(1)
        && p4 == 0
        && a.call_at(0) == Ok(TOKEN_RESPAWN)
        && a.call_at(1) == Ok(RESPAWN_VALUE)
        && a.call_at(2) == Err(DT_ENOBS)
        && a.passes() == 2;

    // 二路：常驻令牌把迭代上限逼出来。
    let mut b = Tls::new();
    let _ = b.set(5, TOKEN_PERPETUAL);
    let mut k = 0usize;
    let mut all_one = true;
    while k < ITERATIONS as usize {
        if b.drain_pass() != Ok(1) {
            all_one = false;
        }
        k += 1;
    }
    let over = b.drain_pass();
    let exit = b.thread_exit();
    out[2] = all_one
        && over == Err(DT_EITER)
        && b.passes() == ITERATIONS
        && b.pending() == 1
        && b.total_calls() == ITERATIONS as usize
        && exit == Err(DT_EITER)
        && b.is_exited()
        && b.drain_pass() == Err(DT_ETHREAD);

    // 三路：闸门次序（越界 / 零值 / 占用 / 空槽 / 死后）。
    let mut c = Tls::new();
    let g1 = c.set(9, 1) == Err(DT_EBOUND);
    let g2 = c.set(0, 0) == Err(DT_EZERO);
    let g3 = c.set(0, 5) == Ok(1);
    let g4 = c.set(0, 0) == Err(DT_EZERO);
    let g5 = c.set(0, 6) == Err(DT_EBUSY);
    let g6 = c.consume(0) == Ok(5);
    let g7 = c.consume(0) == Err(DT_EEMPTY);
    let g8 = c.drain_pass() == Err(DT_EEMPTY);
    let _ = c.thread_exit();
    let g9 = c.set(1, 1) == Err(DT_EDEAD);
    let g10 = c.consume(1) == Err(DT_EDEAD);
    out[3] = g1 && g2 && g3 && g4 && g5 && g6 && g7 && g8 && g9 && g10;

    out
}

/// 十三项判据。
pub fn run_unx11334_checks() -> CheckSet {
    let mut s = CheckSet::new("unx11334.tls_dtor_drain");
    let l = build_ledger();

    s.add_dyn(
        "DT1-十四格八允六拒",
        l.len() == SAMPLES && l.ok_count() == 8 && l.err_count() == 6 && l.audit().is_ok(),
        &format!("n={} ok={} err={}", l.len(), l.ok_count(), l.err_count()),
    );

    s.add(
        "DT2-登记递增待销毁数",
        l.sample(0).map(|x| x.value) == Some(1) && l.sample(2).map(|x| x.value) == Some(2),
        "static",
    );

    s.add(
        "DT3-同槽二次登记被拒",
        l.sample(1).map(|x| x.code) == Some(DT_EBUSY),
        "static",
    );

    s.add(
        "DT4-槽位越界先于占用判断",
        l.sample(3).map(|x| x.code) == Some(DT_EBOUND),
        "static",
    );

    s.add(
        "DT5-零值不是登记",
        l.sample(4).map(|x| x.code) == Some(DT_EZERO),
        "static",
    );

    s.add(
        "DT6-一趟消耗三件",
        l.sample(5).map(|x| x.value) == Some(3)
            && l.sample(6).map(|x| x.value) == Some(3),
        "static",
    );

    s.add(
        "DT7-销毁中重挂延后一趟",
        l.sample(7).map(|x| x.value) == Some(1),
        "static",
    );

    s.add(
        "DT8-线程终结收尾调用数",
        l.sample(8).map(|x| x.value) == Some(4),
        "static",
    );

    s.add(
        "DT9-终结后整趟被拒",
        l.sample(9).map(|x| x.code) == Some(DT_ETHREAD),
        "static",
    );

    s.add(
        "DT10-终结后登记被拒",
        l.sample(10).map(|x| x.code) == Some(DT_EDEAD),
        "static",
    );

    s.add(
        "DT11-空趟被拒",
        l.sample(11).map(|x| x.code) == Some(DT_EEMPTY)
            && l.sample(12).map(|x| x.value) == Some(4)
            && l.sample(13).map(|x| x.value) == Some(2),
        "static",
    );

    s.add(
        "DT12-调用序与令牌语义",
        {
            let mut t = Tls::new();
            let _ = t.set(0, 5);
            // 令牌必须落在重挂槽**之后**：扫描升序，重挂点已扫过才会延后一趟。
            let _ = t.set(5, TOKEN_RESPAWN);
            let first = t.drain_pass();
            let second = t.drain_pass();
            first == Ok(2)
                && second == Ok(1)
                && t.call_at(0) == Ok(5)
                && t.call_at(1) == Ok(TOKEN_RESPAWN)
                && t.call_at(2) == Ok(RESPAWN_VALUE)
                && t.pending() == 0
                && t.passes() == 2
                && t.value_at(1) == Ok(0)
                && t.value_at(RESPAWN_SLOT) == Ok(0)
                && t.value_at(CAP) == Err(DT_ENOBS)
        },
        "static",
    );

    s.add(
        "DT13-冻结重放四注入",
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
        let s = run_unx11334_checks();
        assert_eq!(s.len(), 13);
        assert!(s.all_passed());
    }

    #[test]
    fn respawn_needs_second_pass() {
        let mut t = Tls::new();
        assert_eq!(t.set(5, TOKEN_RESPAWN), Ok(1));
        assert_eq!(t.drain_pass(), Ok(1));
        assert_eq!(t.pending(), 1);
        assert_eq!(t.value_at(RESPAWN_SLOT), Ok(RESPAWN_VALUE));
        assert_eq!(t.drain_pass(), Ok(1));
        assert_eq!(t.pending(), 0);
        assert_eq!(t.passes(), 2);
    }

    #[test]
    fn iteration_limit_is_four() {
        let mut t = Tls::new();
        let _ = t.set(5, TOKEN_PERPETUAL);
        let mut i = 0u32;
        while i < ITERATIONS as u32 {
            assert_eq!(t.drain_pass(), Ok(1));
            i += 1;
        }
        assert_eq!(t.drain_pass(), Err(DT_EITER));
        assert_eq!(t.thread_exit(), Err(DT_EITER));
        assert_eq!(t.pending(), 1);
    }

    #[test]
    fn gates_are_ordered() {
        let mut t = Tls::new();
        assert_eq!(t.set(8, 1), Err(DT_EBOUND));
        assert_eq!(t.set(0, 0), Err(DT_EZERO));
        assert_eq!(t.set(0, 1), Ok(1));
        assert_eq!(t.set(0, 0), Err(DT_EZERO));
        assert_eq!(t.set(0, 2), Err(DT_EBUSY));
        assert_eq!(t.consume(1), Err(DT_EEMPTY));
        assert_eq!(t.drain_pass(), Ok(1));
        assert_eq!(t.drain_pass(), Err(DT_EEMPTY));
    }

    #[test]
    fn dead_thread_rejects_everything() {
        let mut t = Tls::new();
        let _ = t.set(0, 4);
        assert_eq!(t.thread_exit(), Ok(1));
        assert!(t.is_exited());
        assert_eq!(t.set(1, 5), Err(DT_EDEAD));
        assert_eq!(t.consume(1), Err(DT_EDEAD));
        assert_eq!(t.drain_pass(), Err(DT_ETHREAD));
        assert_eq!(t.thread_exit(), Err(DT_EDEAD));
    }

    #[test]
    fn freeze_blocks_writes() {
        let mut l = build_ledger();
        assert!(l.freeze().is_ok());
        assert!(l.register(Sample::blank()).is_err());
        assert_eq!(l.rejects(), 1);
    }

    #[test]
    fn inject_paths_agree() {
        assert_eq!(inject_four(), [true; 4]);
    }
}
