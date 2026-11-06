//! UNX-F11322 `__cxa_atexit` 与 DSO 归属（0xea20 段）
//!
//! `atexit(3)` 只有一个进程级链表；`__cxa_atexit(func, arg, dso)` 则把每个登记
//! 项钉在「产生它的那个 DSO」上。两者结局完全不同：进程退出时剩下的全部按逆序
//! 跑一遍，而 `__cxa_finalize(dso)` 只把该 DSO 名下的项按逆序跑掉并从链表摘除，
//! 其余 DSO 与进程级（dso==0）的项原封不动。
//!
//! 本模块把这条双线建模成一台可审计的状态机：
//!   1. 登记：`__cxa_atexit(func, arg, dso)`，dso ∈ [0, MAX_DSO]。
//!   2. 摘除：`__cxa_finalize(dso)`，只动该 DSO 的项，LIFO 先后由全局登记序决定。
//!   3. 收尾：`run()`，把剩余项按全局登记逆序搬进日志，只此一次。
//!
//! 闸门次序（`register`）刻意固定为：非法函数号 → 非法 DSO → 已收尾 → 容量 → 重复。
//! 次序本身就是语义：先问「参数能不能表达」，再问「生命周期还允不允许」，最后才问
//! 「这部机器还装不装得下」。`finalize` 的闸门是：DSO 非法 → 已收尾 → 已摘除。
//! 三个失败面彼此独立，任意一个被调换都会让账本里的错误码错位。

use crate::alloc::format;
use crate::checks::CheckSet;

/// 段基址。
pub const SEG_BASE: u32 = 0xea20;

/// 函数号为零（空指针登记）。
pub const CX_EINVAL: u32 = SEG_BASE + 0x01;
/// DSO 句柄越界。
pub const CX_EDSO: u32 = SEG_BASE + 0x02;
/// 已经收尾，不再接受登记。
pub const CX_EUSED: u32 = SEG_BASE + 0x03;
/// 登记表满。
pub const CX_EFULL: u32 = SEG_BASE + 0x04;
/// 同 (func, arg, dso) 三元组重复。
pub const CX_EDUP: u32 = SEG_BASE + 0x05;
/// 重复收尾。
pub const CX_ERUN: u32 = SEG_BASE + 0x06;
/// DSO 已被摘除过一次。
pub const CX_EFIN: u32 = SEG_BASE + 0x07;
/// 日志未开写。
pub const CX_EWARM: u32 = SEG_BASE + 0x08;
/// 日志下标越界。
pub const CX_ENOBS: u32 = SEG_BASE + 0x09;
/// 账本格数不足或形状不符。
pub const CX_EBAL: u32 = SEG_BASE + 0x0a;
/// 槽位乱序。
pub const CX_EORDER: u32 = SEG_BASE + 0x0b;
/// 样本值与重算不符。
pub const CX_EMISMATCH: u32 = SEG_BASE + 0x0c;
/// 冻结后重放不一致。
pub const CX_ESEAL: u32 = SEG_BASE + 0x0d;

/// 登记容量。
pub const CAP: usize = 24;
/// 合法 DSO 上界（0 表示进程级）。
pub const MAX_DSO: u8 = 3;
/// 账本格数。
pub const SAMPLES: usize = 14;
/// 操作码：登记。
pub const OP_REG: u8 = 1;
/// 操作码：摘除（`__cxa_finalize`）。
pub const OP_FIN: u8 = 2;
/// 操作码：收尾（`run`）。
pub const OP_RUN: u8 = 3;
/// 操作码：读日志。
pub const OP_LOG: u8 = 4;

/// FNV 偏移基。
pub const FNV_OFF: u64 = 0xcbf2_9ce4_8422_2325;
/// FNV 质数。
pub const FNV_PRIME: u64 = 0x100_0000_01b3;

/// 折入一个 64 位字（八轮字节）。
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

/// 一条登记项。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Entry {
    /// 析构函数号。
    pub func: u32,
    /// 传给析构函数的参数。
    pub arg: u32,
    /// 归属 DSO，0 为进程级。
    pub dso: u8,
}

impl Entry {
    /// 空项。
    pub const fn blank() -> Entry {
        Entry { func: 0, arg: 0, dso: 0 }
    }
}

/// 日志一行：谁在哪个 DSO 名下、带什么参数被跑掉。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LogRow {
    /// 析构函数号。
    pub func: u32,
    /// 参数。
    pub arg: u32,
    /// 归属 DSO。
    pub dso: u8,
}

impl LogRow {
    /// 空行。
    pub const fn blank() -> LogRow {
        LogRow { func: 0, arg: 0, dso: 0 }
    }
}

/// C++ 析构登记机。
#[derive(Clone, Copy, Debug)]
pub struct Cxa {
    e: [Entry; CAP],
    n: usize,
    log: [LogRow; CAP],
    log_len: usize,
    ran: bool,
    fin_mask: u32,
    runs: u32,
    finals: u32,
}

impl Cxa {
    /// 空机。
    pub const fn new() -> Cxa {
        Cxa {
            e: [Entry::blank(); CAP],
            n: 0,
            log: [LogRow::blank(); CAP],
            log_len: 0,
            ran: false,
            fin_mask: 0,
            runs: 0,
            finals: 0,
        }
    }

    /// 在册条数。
    pub fn count(&self) -> usize {
        self.n
    }

    /// 日志长度。
    pub fn log_len(&self) -> usize {
        self.log_len
    }

    /// 是否已收尾。
    pub fn is_ran(&self) -> bool {
        self.ran
    }

    /// 收尾次数。
    pub fn runs(&self) -> u32 {
        self.runs
    }

    /// 摘除次数。
    pub fn finals(&self) -> u32 {
        self.finals
    }

    /// 某 DSO 是否已被摘除。
    pub fn is_finalized(&self, dso: u8) -> bool {
        if dso == 0 || dso > MAX_DSO {
            return false;
        }
        let bit = 1u32 << (dso - 1);
        self.fin_mask & bit != 0
    }

    /// 某 DSO 在册条数。
    pub fn dso_count(&self, dso: u8) -> usize {
        let mut c = 0usize;
        let mut i = 0usize;
        while i < self.n {
            if let Some(x) = self.e.get(i) {
                if x.dso == dso {
                    c += 1;
                }
            }
            i += 1;
        }
        c
    }

    /// 在册第 i 项。
    pub fn at(&self, i: usize) -> Option<Entry> {
        if i >= self.n {
            return None;
        }
        self.e.get(i).copied()
    }

    /// 日志第 i 行。
    pub fn log_at(&self, i: usize) -> Result<LogRow, u32> {
        if self.log_len == 0 {
            return Err(CX_EWARM);
        }
        if i >= self.log_len {
            return Err(CX_ENOBS);
        }
        match self.log.get(i) {
            Some(v) => Ok(*v),
            None => Err(CX_ENOBS),
        }
    }

    /// `__cxa_atexit(func, arg, dso)`。
    ///
    /// 返回登记后的在册条数。闸门：非法函数号 → 非法 DSO → 已收尾 → 容量 → 重复。
    pub fn register(&mut self, func: u32, arg: u32, dso: u8) -> Result<usize, u32> {
        if func == 0 {
            return Err(CX_EINVAL);
        }
        if dso > MAX_DSO {
            return Err(CX_EDSO);
        }
        if self.ran {
            return Err(CX_EUSED);
        }
        if self.n >= CAP {
            return Err(CX_EFULL);
        }
        if self.is_finalized(dso) {
            return Err(CX_EFIN);
        }
        let mut i = 0usize;
        while i < self.n {
            match self.e.get(i) {
                Some(x) => {
                    if x.func == func && x.arg == arg && x.dso == dso {
                        return Err(CX_EDUP);
                    }
                }
                None => return Err(CX_EBAL),
            }
            i += 1;
        }
        match self.e.get_mut(self.n) {
            Some(slot) => *slot = Entry { func, arg, dso },
            None => return Err(CX_EFULL),
        }
        self.n += 1;
        Ok(self.n)
    }

    /// `__cxa_finalize(dso)`：只跑该 DSO 的项，逆序，摘除后压紧。
    ///
    /// 返回本次跑掉的条数。闸门：DSO 非法 → 已收尾 → 已摘除。
    pub fn finalize(&mut self, dso: u8) -> Result<usize, u32> {
        if dso == 0 || dso > MAX_DSO {
            return Err(CX_EDSO);
        }
        if self.ran {
            return Err(CX_EUSED);
        }
        if self.is_finalized(dso) {
            return Err(CX_EFIN);
        }
        let mut ran = 0usize;
        let mut i = self.n;
        // 逆序扫描：先看见的项先跑，正是该 DSO 的 LIFO。
        while i > 0 {
            i -= 1;
            let hit = match self.e.get(i) {
                Some(x) => x.dso == dso,
                None => continue,
            };
            if !hit {
                continue;
            }
            if let Some(x) = self.e.get(i) {
                let row = LogRow { func: x.func, arg: x.arg, dso: x.dso };
                if let Some(dst) = self.log.get_mut(self.log_len) {
                    *dst = row;
                    self.log_len += 1;
                }
            }
            // 摘除：把尾项搬到空洞上，逆序扫描因此可以安全继续。
            let last = self.n - 1;
            if i != last {
                if let Some(tail) = self.e.get(last).copied() {
                    if let Some(dst) = self.e.get_mut(i) {
                        *dst = tail;
                    }
                }
            }
            self.n = last;
            ran += 1;
        }
        let bit = 1u32 << (dso - 1);
        self.fin_mask |= bit;
        self.finals += 1;
        Ok(ran)
    }

    /// 进程收尾：剩余项按全局登记逆序跑一遍，只此一次。
    pub fn run(&mut self) -> Result<usize, u32> {
        if self.ran {
            return Err(CX_ERUN);
        }
        self.ran = true;
        self.runs += 1;
        let mut k = 0usize;
        while k < self.n {
            let src = self.n - 1 - k;
            if let Some(x) = self.e.get(src) {
                let row = LogRow { func: x.func, arg: x.arg, dso: x.dso };
                if let Some(dst) = self.log.get_mut(self.log_len + k) {
                    *dst = row;
                }
            }
            k += 1;
        }
        self.log_len += k;
        Ok(k)
    }

    /// 指纹：在册项、日志、闸门计数一起折。
    pub fn seal(&self) -> u64 {
        let mut h = FNV_OFF;
        h = fold(h, self.n as u64);
        let mut i = 0usize;
        while i < self.n {
            if let Some(x) = self.e.get(i) {
                h = fold(h, x.func as u64);
                h = fold(h, x.arg as u64);
                h = fold(h, x.dso as u64);
            }
            i += 1;
        }
        h = fold(h, self.log_len as u64);
        let mut j = 0usize;
        while j < self.log_len {
            if let Some(x) = self.log.get(j) {
                h = fold(h, x.func as u64);
                h = fold(h, x.arg as u64);
                h = fold(h, x.dso as u64);
            }
            j += 1;
        }
        h = fold(h, self.fin_mask as u64);
        h = fold(h, self.runs as u64);
        fold(h, self.finals as u64)
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
    /// 错误码（成功为 0）。
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

/// 把一次 `Result<usize, u32>` 落成样本。
fn cell(slot: u8, op: u8, r: Result<usize, u32>) -> Sample {
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
        if self.frozen {
            self.rejects += 1;
            return Err(CX_EBAL);
        }
        if (s.slot as usize) >= SAMPLES {
            self.rejects += 1;
            return Err(CX_EORDER);
        }
        if (s.slot as usize) < self.len {
            self.rejects += 1;
            return Err(CX_EORDER);
        }
        if (s.slot as usize) != self.len {
            self.rejects += 1;
            return Err(CX_EORDER);
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

    /// 对账：形状 8 允 6 拒、逐格复算、错误码阵列。
    pub fn audit(&self) -> Result<(), u32> {
        if self.len != SAMPLES {
            return Err(CX_EBAL);
        }
        if self.ok_count() != 8 || self.err_count() != 6 {
            return Err(CX_EBAL);
        }
        // 允许格的观测值，按槽位次序写死。
        const VALS: [u32; 8] = [1, 2, 3, 4, 2, 3, 3, 165];
        // 拒绝格的错误码，按槽位次序写死。
        const CODES: [u32; 6] = [CX_EDUP, CX_EINVAL, CX_EDSO, CX_EUSED, CX_EFULL, CX_EFIN];
        let mut vi = 0usize;
        let mut ci = 0usize;
        let mut i = 0usize;
        while i < self.len {
            let s = match self.rows.get(i) {
                Some(v) => *v,
                None => return Err(CX_EBAL),
            };
            if s.slot as usize != i {
                return Err(CX_EORDER);
            }
            if s.ok {
                if s.code != 0 {
                    return Err(CX_EMISMATCH);
                }
                match VALS.get(vi) {
                    Some(want) => {
                        if s.value != *want {
                            return Err(CX_EMISMATCH);
                        }
                    }
                    None => return Err(CX_EBAL),
                }
                vi += 1;
            } else {
                match CODES.get(ci) {
                    Some(want) => {
                        if s.code != *want {
                            return Err(CX_EMISMATCH);
                        }
                    }
                    None => return Err(CX_EBAL),
                }
                ci += 1;
            }
            i += 1;
        }
        if vi != 8 || ci != 6 {
            return Err(CX_EBAL);
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
            return Err(CX_EBAL);
        }
        self.root = self.seal();
        self.frozen = true;
        Ok(self.root)
    }

    /// 重放：重建同形账本再比指纹。
    pub fn replay(&self) -> Result<u64, u32> {
        let fresh = build_ledger();
        if fresh.seal() == self.seal() {
            Ok(fresh.seal())
        } else {
            Err(CX_ESEAL)
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

/// 十四格账本：两条 DSO 线 + 进程线，先摘除后收尾。
pub fn build_ledger() -> Ledger {
    let mut l = Ledger::new();
    let mut c = Cxa::new();

    let _ = c.register(0xA1, 0, 0);
    let _ = l.register(cell(0, OP_REG, Ok(c.count())));
    let _ = c.register(0xA2, 0x10, 1);
    let _ = l.register(cell(1, OP_REG, Ok(c.count())));
    let _ = l.register(cell(2, OP_REG, c.register(0xA2, 0x10, 1)));
    let _ = c.register(0xA3, 0x11, 1);
    let _ = l.register(cell(3, OP_REG, Ok(c.count())));
    let _ = c.register(0xA4, 0x20, 2);
    let _ = l.register(cell(4, OP_REG, Ok(c.count())));
    let _ = l.register(cell(5, OP_FIN, c.finalize(1)));
    let _ = c.register(0xA5, 0x30, 3);
    let _ = l.register(cell(6, OP_REG, Ok(c.count())));
    let _ = l.register(cell(7, OP_RUN, c.run()));

    // 第 8 格观测日志第 2 行：摘除片段两行之后才是收尾片段。
    let v = match c.log_at(2) {
        Ok(r) => Sample { slot: 8, op: OP_LOG, ok: true, code: 0, value: r.func },
        Err(e) => Sample { slot: 8, op: OP_LOG, ok: false, code: e, value: 0 },
    };
    let _ = l.register(v);

    let _ = l.register(cell(9, OP_REG, c.register(0, 0, 0)));
    let _ = l.register(cell(10, OP_REG, c.register(0xB0, 0, MAX_DSO + 1)));
    let _ = l.register(cell(11, OP_REG, c.register(0xB1, 0, 0)));

    // 第 12 格：另一台机器灌满后再登记一格。
    let mut full = Cxa::new();
    let mut i = 0u32;
    while (i as usize) < CAP {
        let _ = full.register(0x100 + i, 0, 0);
        i += 1;
    }
    let _ = l.register(cell(12, OP_REG, full.register(0x200, 0, 0)));

    // 第 13 格：同一 DSO 二次摘除。
    let mut twice = Cxa::new();
    let _ = twice.register(1, 0, 1);
    let _ = twice.register(2, 0, 1);
    let _ = twice.finalize(1);
    let _ = l.register(cell(13, OP_FIN, twice.finalize(1)));

    l
}

/// 四路注入。
pub fn inject_four() -> [bool; 4] {
    let mut out = [false; 4];

    let l = build_ledger();
    let rep = l.replay();
    let root = l.seal();
    out[0] = l.len() == SAMPLES && l.audit().is_ok() && rep == Ok(root) && root != 0;

    // 一路：单 DSO 三连登记的 LIFO。
    let mut a = Cxa::new();
    let _ = a.register(11, 1, 2);
    let _ = a.register(12, 2, 2);
    let _ = a.register(13, 3, 2);
    let n = a.finalize(2);
    let mut lifo = n == Ok(3) && a.count() == 0 && a.log_len() == 3;
    if lifo {
        let mut k = 0usize;
        while k < 3 {
            let want = 13 - k as u32;
            match a.log_at(k) {
                Ok(r) => {
                    if r.func != want || r.dso != 2 {
                        lifo = false;
                    }
                }
                Err(_) => lifo = false,
            }
            k += 1;
        }
    }
    out[1] = lifo;

    // 二路：摘除只动本 DSO，进程线与另一条 DSO 线原封不动。
    let mut b = Cxa::new();
    let _ = b.register(21, 0, 0);
    let _ = b.register(22, 0, 0);
    let _ = b.register(23, 0, 1);
    let _ = b.register(24, 0, 3);
    let got = b.finalize(1);
    out[2] = got == Ok(1)
        && b.count() == 3
        && b.dso_count(0) == 2
        && b.dso_count(3) == 1
        && b.is_finalized(1)
        && !b.is_finalized(0)
        && b.register(25, 0, 1) == Err(CX_EFIN);

    // 三路：闸门次序（非法函数号先于非法 DSO；非法 DSO 先于已收尾）。
    let mut g = Cxa::new();
    let g1 = g.register(0, 0, 9) == Err(CX_EINVAL);
    let g2 = g.register(1, 0, 9) == Err(CX_EDSO);
    let _ = g.register(2, 0, 0);
    let _ = g.run();
    let g3 = g.register(3, 0, 9) == Err(CX_EDSO);
    let g4 = g.register(3, 0, 0) == Err(CX_EUSED);
    let g5 = g.run() == Err(CX_ERUN);
    let g6 = g.finalize(1) == Err(CX_EUSED);
    let g7 = g.log_at(g.log_len()) == Err(CX_ENOBS);
    let warm = Cxa::new();
    let g8 = warm.log_at(0) == Err(CX_EWARM);
    out[3] = g1 && g2 && g3 && g4 && g5 && g6 && g7 && g8;

    out
}

/// 十三项判据。
pub fn run_unx11322_checks() -> CheckSet {
    let mut s = CheckSet::new("unx11322.cxa_atexit");
    let l = build_ledger();

    s.add_dyn(
        "CX1-十四格八允六拒",
        l.len() == SAMPLES && l.ok_count() == 8 && l.err_count() == 6 && l.audit().is_ok(),
        &format!("n={} ok={} err={}", l.len(), l.ok_count(), l.err_count()),
    );

    s.add(
        "CX2-登记记数递增",
        l.sample(0).map(|x| x.value) == Some(1)
            && l.sample(1).map(|x| x.value) == Some(2)
            && l.sample(3).map(|x| x.value) == Some(3)
            && l.sample(4).map(|x| x.value) == Some(4),
        "static",
    );

    s.add(
        "CX3-重复三元组拒绝",
        l.sample(2).map(|x| x.code) == Some(CX_EDUP)
            && l.sample(2).map(|x| x.ok) == Some(false),
        "static",
    );

    s.add(
        "CX4-摘除本 DSO 两条",
        l.sample(5).map(|x| x.value) == Some(2),
        "static",
    );

    s.add(
        "CX5-摘除后计数回落",
        l.sample(6).map(|x| x.value) == Some(3),
        "static",
    );

    s.add(
        "CX6-收尾跑掉三条",
        l.sample(7).map(|x| x.value) == Some(3),
        "static",
    );

    s.add(
        "CX7-日志第二行属收尾段",
        l.sample(8).map(|x| x.value) == Some(0xA5),
        "static",
    );

    s.add(
        "CX8-非法函数号与非法 DSO",
        l.sample(9).map(|x| x.code) == Some(CX_EINVAL)
            && l.sample(10).map(|x| x.code) == Some(CX_EDSO),
        "static",
    );

    s.add(
        "CX9-收尾后登记被拒",
        l.sample(11).map(|x| x.code) == Some(CX_EUSED),
        "static",
    );

    s.add(
        "CX10-满员与二次摘除",
        l.sample(12).map(|x| x.code) == Some(CX_EFULL)
            && l.sample(13).map(|x| x.code) == Some(CX_EFIN),
        "static",
    );

    s.add(
        "CX11-双线隔离序列",
        {
            let mut a = Cxa::new();
            let _ = a.register(1, 0, 0);
            let _ = a.register(2, 0, 1);
            let _ = a.register(3, 0, 2);
            let _ = a.register(4, 0, 1);
            let f = a.finalize(1);
            let first = match a.log_at(0) {
                Ok(r) => r.func == 4 && r.dso == 1,
                Err(_) => false,
            };
            let second = match a.log_at(1) {
                Ok(r) => r.func == 2 && r.dso == 1,
                Err(_) => false,
            };
            f == Ok(2) && first && second && a.dso_count(0) == 1 && a.dso_count(2) == 1
        },
        "static",
    );

    s.add(
        "CX12-同函数换 DSO 可再登记",
        {
            let mut a = Cxa::new();
            let one = a.register(7, 7, 0);
            let two = a.register(7, 7, 1);
            let three = a.register(7, 8, 0);
            one == Ok(1) && two == Ok(2) && three == Ok(3)
        },
        "static",
    );

    s.add(
        "CX13-冻结重放四注入",
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
        let s = run_unx11322_checks();
        assert_eq!(s.len(), 13);
        assert!(s.all_passed());
    }

    #[test]
    fn dso_lifo_and_removal() {
        let mut a = Cxa::new();
        assert_eq!(a.register(1, 0, 2), Ok(1));
        assert_eq!(a.register(2, 0, 2), Ok(2));
        assert_eq!(a.register(3, 0, 1), Ok(3));
        assert_eq!(a.finalize(2), Ok(2));
        assert_eq!(a.count(), 1);
        assert_eq!(a.log_at(0), Ok(LogRow { func: 2, arg: 0, dso: 2 }));
        assert_eq!(a.log_at(1), Ok(LogRow { func: 1, arg: 0, dso: 2 }));
        assert_eq!(a.log_at(2), Err(CX_ENOBS));
    }

    #[test]
    fn process_remainder_runs_last() {
        let mut a = Cxa::new();
        let _ = a.register(9, 0, 0);
        let _ = a.register(8, 0, 3);
        let _ = a.finalize(3);
        assert_eq!(a.run(), Ok(1));
        assert_eq!(a.log_at(1).map(|r| r.func), Ok(9));
        assert_eq!(a.run(), Err(CX_ERUN));
    }

    #[test]
    fn gates_are_ordered() {
        let mut a = Cxa::new();
        assert_eq!(a.register(0, 0, MAX_DSO + 1), Err(CX_EINVAL));
        assert_eq!(a.register(1, 0, MAX_DSO + 1), Err(CX_EDSO));
        assert_eq!(a.register(1, 0, 0), Ok(1));
        assert_eq!(a.register(1, 0, 0), Err(CX_EDUP));
        assert_eq!(a.finalize(0), Err(CX_EDSO));
        assert_eq!(a.finalize(1), Ok(0));
        assert_eq!(a.finalize(1), Err(CX_EFIN));
    }

    #[test]
    fn capacity_is_exact() {
        let mut a = Cxa::new();
        let mut i = 0u32;
        while (i as usize) < CAP {
            assert!(a.register(i + 1, 0, 0).is_ok());
            i += 1;
        }
        assert_eq!(a.register(0xFFFF, 0, 0), Err(CX_EFULL));
        assert_eq!(a.count(), CAP);
    }

    #[test]
    fn freeze_blocks_writes() {
        let mut l = build_ledger();
        let root = l.freeze();
        assert!(root.is_ok());
        let again = l.register(Sample { slot: 0, op: OP_REG, ok: true, code: 0, value: 1 });
        assert!(again.is_err());
        assert_eq!(l.rejects(), 1);
    }

    #[test]
    fn inject_all_true() {
        assert_eq!(inject_four(), [true; 4]);
    }
}
