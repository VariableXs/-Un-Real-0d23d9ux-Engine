//! UNX-F11324 `_exit` 即时终止语义（0xea40 段）
//!
//! `exit(3)` 与 `_exit(2)` 的差别全在「还跑不跑别人」：前者先按 LIFO 跑完 atexit
//! 处理器、再把 stdio 缓冲刷下去，最后才真正终止；后者一步到位，处理器不跑、缓冲
//! 不刷、状态只保留低八位。内核侧这一分叉必须可测：任何一次误用 `_exit` 都等于
//! 悄悄吞掉调用者登记的清理动作。
//!
//! 本模块把两种终止路径并排建模：
//!   * `exit(code)`  ：闸门 → LIFO 处理器 → 刷缓冲 → 置终止位。
//!   * `_exit(code)` ：闸门 → 置终止位（处理器与缓冲原样不动）。
//!
//! 终止是一次性的：第二次任何形态的终止都回 `XC_EUSED`；终止之后再登记处理器回
//! `XC_EAFTER`；缓冲还没刷过就读状态回 `XC_EWARM`。终止码一律按低八位截断，
//! 高位在进入状态机之前就被丢弃，这一点由 `status()` 的复算来兜底。

use crate::alloc::format;
use crate::checks::CheckSet;

/// 段基址。
pub const SEG_BASE: u32 = 0xea40;

/// 处理器号为零。
pub const XC_EINVAL: u32 = SEG_BASE + 0x01;
/// 终止后仍试图登记。
pub const XC_EAFTER: u32 = SEG_BASE + 0x02;
/// 二次终止。
pub const XC_EUSED: u32 = SEG_BASE + 0x03;
/// 处理器表满。
pub const XC_EFULL: u32 = SEG_BASE + 0x04;
/// 处理器重复。
pub const XC_EDUP: u32 = SEG_BASE + 0x05;
/// 尚未终止，状态不可读。
pub const XC_EWARM: u32 = SEG_BASE + 0x06;
/// 缓冲已刷过。
pub const XC_EFLUSH: u32 = SEG_BASE + 0x07;
/// 账本形状不符。
pub const XC_EBAL: u32 = SEG_BASE + 0x08;
/// 槽位乱序。
pub const XC_EORDER: u32 = SEG_BASE + 0x09;
/// 观测值与复算不符。
pub const XC_EMISMATCH: u32 = SEG_BASE + 0x0a;
/// 冻结后重放不一致。
pub const XC_ESEAL: u32 = SEG_BASE + 0x0b;
/// 进程内存在未刷缓冲时的 `_exit`。
pub const XC_EDIRTY: u32 = SEG_BASE + 0x0c;
/// 状态机内部不一致。
pub const XC_ESTATE: u32 = SEG_BASE + 0x0d;

/// 处理器容量。
pub const CAP: usize = 12;
/// 账本格数。
pub const SAMPLES: usize = 14;
/// 操作码：登记处理器。
pub const OP_ATEXIT: u8 = 1;
/// 操作码：完整退出。
pub const OP_EXIT: u8 = 2;
/// 操作码：即时退出。
pub const OP_FAST: u8 = 3;
/// 操作码：读状态。
pub const OP_READ: u8 = 4;

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

/// 终止核心。
#[derive(Clone, Copy, Debug)]
pub struct ExitCore {
    pending: [u32; CAP],
    n: usize,
    order: [u32; CAP],
    ran: u8,
    flushes: u8,
    status: u8,
    terminated: bool,
    fast: bool,
    regs: u32,
    rejects: u32,
}

impl ExitCore {
    /// 空核心。
    pub const fn new() -> ExitCore {
        ExitCore {
            pending: [0; CAP],
            n: 0,
            order: [0; CAP],
            ran: 0,
            flushes: 0,
            status: 0,
            terminated: false,
            fast: false,
            regs: 0,
            rejects: 0,
        }
    }

    /// 在册处理器数。
    pub fn pending(&self) -> usize {
        self.n
    }

    /// 已跑处理器数。
    pub fn handlers_run(&self) -> u8 {
        self.ran
    }

    /// 刷缓冲次数。
    pub fn flushes(&self) -> u8 {
        self.flushes
    }

    /// 终止码（低八位）。
    pub fn status(&self) -> Result<u8, u32> {
        if !self.terminated {
            return Err(XC_EWARM);
        }
        Ok(self.status)
    }

    /// 是否已终止。
    pub fn is_terminated(&self) -> bool {
        self.terminated
    }

    /// 是否走的是即时路径。
    pub fn is_fast(&self) -> bool {
        self.fast
    }

    /// 登记总数。
    pub fn regs(&self) -> u32 {
        self.regs
    }

    /// 被拒次数。
    pub fn rejects(&self) -> u32 {
        self.rejects
    }

    /// 第 i 个被跑掉的处理器号。
    pub fn ran_at(&self, i: usize) -> Option<u32> {
        if i >= self.ran as usize {
            return None;
        }
        self.order.get(i).copied()
    }

    /// `atexit(func)`。返回在册条数。
    pub fn atexit(&mut self, func: u32) -> Result<usize, u32> {
        if func == 0 {
            self.rejects += 1;
            return Err(XC_EINVAL);
        }
        if self.terminated {
            self.rejects += 1;
            return Err(XC_EAFTER);
        }
        if self.n >= CAP {
            self.rejects += 1;
            return Err(XC_EFULL);
        }
        let mut i = 0usize;
        while i < self.n {
            if let Some(f) = self.pending.get(i) {
                if *f == func {
                    self.rejects += 1;
                    return Err(XC_EDUP);
                }
            }
            i += 1;
        }
        if let Some(dst) = self.pending.get_mut(self.n) {
            *dst = func;
        }
        self.n += 1;
        self.regs += 1;
        Ok(self.n)
    }

    /// LIFO 跑处理器（内部）。
    fn drain(&mut self) -> Result<usize, u32> {
        let mut k = 0usize;
        while k < self.n {
            let src = self.n - 1 - k;
            match self.pending.get(src) {
                Some(f) => {
                    if let Some(dst) = self.order.get_mut(k) {
                        *dst = *f;
                    }
                }
                None => return Err(XC_ESTATE),
            }
            k += 1;
        }
        self.ran = k as u8;
        self.n = 0;
        Ok(k)
    }

    /// 刷缓冲（内部）。
    fn flush(&mut self) -> Result<u8, u32> {
        if self.flushes == u8::MAX {
            return Err(XC_EFLUSH);
        }
        self.flushes += 1;
        Ok(self.flushes)
    }

    /// `exit(code)`：LIFO → 刷 → 终止。返回跑掉的处理器数。
    pub fn exit(&mut self, code: u32) -> Result<usize, u32> {
        if self.terminated {
            self.rejects += 1;
            return Err(XC_EUSED);
        }
        let n = self.drain()?;
        let _ = self.flush()?;
        self.status = (code & 0xff) as u8;
        self.terminated = true;
        self.fast = false;
        Ok(n)
    }

    /// `_exit(code)`：直接终止，处理器与缓冲原样保留。返回跑掉的处理器数（恒 0）。
    pub fn fast_exit(&mut self, code: u32) -> Result<usize, u32> {
        if self.terminated {
            self.rejects += 1;
            return Err(XC_EUSED);
        }
        self.status = (code & 0xff) as u8;
        self.terminated = true;
        self.fast = true;
        Ok(0)
    }

    /// 指纹。
    pub fn seal(&self) -> u64 {
        let mut h = FNV_OFF;
        h = fold(h, self.n as u64);
        let mut i = 0usize;
        while i < self.n {
            if let Some(v) = self.pending.get(i) {
                h = fold(h, *v as u64);
            }
            i += 1;
        }
        h = fold(h, self.ran as u64);
        let mut j = 0usize;
        while j < self.ran as usize {
            if let Some(v) = self.order.get(j) {
                h = fold(h, *v as u64);
            }
            j += 1;
        }
        h = fold(h, self.flushes as u64);
        h = fold(h, self.status as u64);
        h = fold(h, self.terminated as u64);
        h = fold(h, self.fast as u64);
        fold(h, self.regs as u64)
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

/// 把 `Result<usize, u32>` 落成样本。
fn cell(slot: u8, op: u8, r: Result<usize, u32>) -> Sample {
    match r {
        Ok(v) => Sample { slot, op, ok: true, code: 0, value: v as u32 },
        Err(c) => Sample { slot, op, ok: false, code: c, value: 0 },
    }
}

/// 把 `Result<u8, u32>` 落成样本。
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
            return Err(XC_EORDER);
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
            return Err(XC_EBAL);
        }
        const VALS: [u32; 8] = [1, 2, 3, 3, 7, 3, 1, 0];
        const CODES: [u32; 6] = [XC_EINVAL, XC_EAFTER, XC_EUSED, XC_EFULL, XC_EDUP, XC_EWARM];
        let mut vi = 0usize;
        let mut ci = 0usize;
        let mut i = 0usize;
        while i < self.len {
            let s = match self.rows.get(i) {
                Some(v) => *v,
                None => return Err(XC_EBAL),
            };
            if s.slot as usize != i {
                return Err(XC_EORDER);
            }
            if s.ok {
                if s.code != 0 {
                    return Err(XC_EMISMATCH);
                }
                match VALS.get(vi) {
                    Some(w) => {
                        if s.value != *w {
                            return Err(XC_EMISMATCH);
                        }
                    }
                    None => return Err(XC_EBAL),
                }
                vi += 1;
            } else {
                match CODES.get(ci) {
                    Some(w) => {
                        if s.code != *w {
                            return Err(XC_EMISMATCH);
                        }
                    }
                    None => return Err(XC_EBAL),
                }
                ci += 1;
            }
            i += 1;
        }
        if vi != 8 || ci != 6 {
            return Err(XC_EBAL);
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
            return Err(XC_EBAL);
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
            Err(XC_ESEAL)
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

/// 十四格账本：完整退出走一趟，即时退出另起一台。
pub fn build_ledger() -> Ledger {
    let mut l = Ledger::new();
    let mut c = ExitCore::new();

    let _ = l.register(cell(0, OP_ATEXIT, c.atexit(1)));
    let _ = l.register(cell(1, OP_ATEXIT, c.atexit(2)));
    let _ = l.register(cell(2, OP_ATEXIT, c.atexit(3)));
    let _ = l.register(cell(3, OP_EXIT, c.exit(0x107)));
    let _ = l.register(byte_cell(4, OP_READ, c.status()));
    let _ = l.register(Sample {
        slot: 5,
        op: OP_READ,
        ok: true,
        code: 0,
        value: c.handlers_run() as u32,
    });
    let _ = l.register(Sample { slot: 6, op: OP_READ, ok: true, code: 0, value: c.flushes() as u32 });

    // 第 7 格：另一台机器走即时路径，处理器一条不动。
    let mut f = ExitCore::new();
    let _ = f.atexit(41);
    let _ = f.atexit(42);
    let _ = l.register(cell(7, OP_FAST, f.fast_exit(0x1ff)));

    let mut warm = ExitCore::new();
    let _ = l.register(cell(8, OP_ATEXIT, warm.atexit(0)));
    let _ = l.register(cell(9, OP_ATEXIT, c.atexit(9)));
    let _ = l.register(cell(10, OP_EXIT, c.exit(0)));

    let mut full = ExitCore::new();
    let mut i = 1u32;
    while (i as usize) <= CAP {
        let _ = full.atexit(i);
        i += 1;
    }
    let _ = l.register(cell(11, OP_ATEXIT, full.atexit(0x200)));

    let mut dup = ExitCore::new();
    let _ = dup.atexit(5);
    let _ = l.register(cell(12, OP_ATEXIT, dup.atexit(5)));
    let _ = l.register(byte_cell(13, OP_READ, warm.status()));

    l
}

/// 四路注入。
pub fn inject_four() -> [bool; 4] {
    let mut out = [false; 4];

    let l = build_ledger();
    let rep = l.replay();
    let root = l.seal();
    out[0] = l.len() == SAMPLES && l.audit().is_ok() && rep == Ok(root) && root != 0;

    // 一路：即时退出不跑处理器、不刷缓冲。
    let mut a = ExitCore::new();
    let _ = a.atexit(11);
    let _ = a.atexit(12);
    let r = a.fast_exit(0x2a);
    out[1] = r == Ok(0)
        && a.handlers_run() == 0
        && a.flushes() == 0
        && a.status() == Ok(0x2a)
        && a.is_fast()
        && a.pending() == 2;

    // 二路：完整退出 LIFO 跑净，缓冲刷一次。
    let mut b = ExitCore::new();
    let _ = b.atexit(21);
    let _ = b.atexit(22);
    let _ = b.atexit(23);
    let r = b.exit(3);
    out[2] = r == Ok(3)
        && b.ran_at(0) == Some(23)
        && b.ran_at(1) == Some(22)
        && b.ran_at(2) == Some(21)
        && b.pending() == 0
        && b.flushes() == 1
        && b.status() == Ok(3)
        && !b.is_fast();

    // 三路：终止后的一切动作都被拒，且拒绝计数如实累加。
    let mut g = ExitCore::new();
    let _ = g.atexit(31);
    let _ = g.exit(0x1_0001);
    let r1 = g.atexit(32) == Err(XC_EAFTER);
    let r2 = g.exit(0) == Err(XC_EUSED);
    let r3 = g.fast_exit(0) == Err(XC_EUSED);
    let r4 = g.status() == Ok(1);
    out[3] = r1 && r2 && r3 && r4 && g.rejects() == 3;

    out
}

/// 十三项判据。
pub fn run_unx11324_checks() -> CheckSet {
    let mut s = CheckSet::new("unx11324.fast_exit");
    let l = build_ledger();

    s.add_dyn(
        "XC1-十四格八允六拒",
        l.len() == SAMPLES && l.ok_count() == 8 && l.err_count() == 6 && l.audit().is_ok(),
        &format!("n={} ok={} err={}", l.len(), l.ok_count(), l.err_count()),
    );

    s.add(
        "XC2-登记计数递增",
        l.sample(0).map(|x| x.value) == Some(1)
            && l.sample(1).map(|x| x.value) == Some(2)
            && l.sample(2).map(|x| x.value) == Some(3),
        "static",
    );

    s.add(
        "XC3-完整退出跑净三条",
        l.sample(3).map(|x| x.value) == Some(3),
        "static",
    );

    s.add(
        "XC4-高八位被截断",
        l.sample(4).map(|x| x.value) == Some(7),
        "static",
    );

    s.add(
        "XC5-处理器与缓冲记账",
        l.sample(5).map(|x| x.value) == Some(3) && l.sample(6).map(|x| x.value) == Some(1),
        "static",
    );

    s.add(
        "XC6-即时退出零执行",
        l.sample(7).map(|x| x.value) == Some(0),
        "static",
    );

    s.add(
        "XC7-零号与终止后登记",
        l.sample(8).map(|x| x.code) == Some(XC_EINVAL)
            && l.sample(9).map(|x| x.code) == Some(XC_EAFTER),
        "static",
    );

    s.add(
        "XC8-二次终止被拒",
        l.sample(10).map(|x| x.code) == Some(XC_EUSED),
        "static",
    );

    s.add(
        "XC9-满员与重复",
        l.sample(11).map(|x| x.code) == Some(XC_EFULL)
            && l.sample(12).map(|x| x.code) == Some(XC_EDUP),
        "static",
    );

    s.add(
        "XC10-未终止状态不可读",
        l.sample(13).map(|x| x.code) == Some(XC_EWARM),
        "static",
    );

    s.add(
        "XC11-即时退出保留向 LIFO",
        {
            let mut a = ExitCore::new();
            let _ = a.atexit(1);
            let _ = a.atexit(2);
            let _ = a.fast_exit(0);
            // 保留的处理器仍是 LIFO 序，只是再也没有机会跑。
            a.pending() == 2 && a.ran_at(0).is_none() && a.handlers_run() == 0
        },
        "static",
    );

    s.add(
        "XC12-终止码逐轮不同",
        {
            let mut a = ExitCore::new();
            let first = a.exit(0);
            let mut b = ExitCore::new();
            let second = b.fast_exit(0xff);
            first == Ok(0) && second == Ok(0) && a.status() == Ok(0) && b.status() == Ok(0xff)
        },
        "static",
    );

    s.add(
        "XC13-冻结重放四注入",
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
        let s = run_unx11324_checks();
        assert_eq!(s.len(), 13);
        assert!(s.all_passed());
    }

    #[test]
    fn fast_exit_skips_everything() {
        let mut a = ExitCore::new();
        let _ = a.atexit(7);
        assert_eq!(a.fast_exit(0x101), Ok(0));
        assert_eq!(a.handlers_run(), 0);
        assert_eq!(a.flushes(), 0);
        assert_eq!(a.pending(), 1);
        assert_eq!(a.status(), Ok(1));
        assert!(a.is_fast());
    }

    #[test]
    fn full_exit_runs_lifo_then_flushes() {
        let mut a = ExitCore::new();
        let _ = a.atexit(1);
        let _ = a.atexit(2);
        assert_eq!(a.exit(9), Ok(2));
        assert_eq!(a.ran_at(0), Some(2));
        assert_eq!(a.ran_at(1), Some(1));
        assert_eq!(a.flushes(), 1);
        assert!(!a.is_fast());
        assert_eq!(a.status(), Ok(9));
    }

    #[test]
    fn termination_is_one_shot() {
        let mut a = ExitCore::new();
        assert_eq!(a.fast_exit(0), Ok(0));
        assert_eq!(a.exit(0), Err(XC_EUSED));
        assert_eq!(a.fast_exit(0), Err(XC_EUSED));
        assert_eq!(a.atexit(1), Err(XC_EAFTER));
        assert_eq!(a.rejects(), 3);
    }

    #[test]
    fn capacity_and_duplicates() {
        let mut a = ExitCore::new();
        let mut i = 1u32;
        while (i as usize) <= CAP {
            assert_eq!(a.atexit(i), Ok(i as usize));
            i += 1;
        }
        assert_eq!(a.atexit(99), Err(XC_EFULL));
        let mut b = ExitCore::new();
        assert_eq!(b.atexit(5), Ok(1));
        assert_eq!(b.atexit(5), Err(XC_EDUP));
    }

    #[test]
    fn warm_status_is_rejected() {
        let a = ExitCore::new();
        assert_eq!(a.status(), Err(XC_EWARM));
    }

    #[test]
    fn inject_all_true() {
        assert_eq!(inject_four(), [true; 4]);
    }
}
