//! UNX-F11336 退出码饱和与异常值（0xeb00 段）
//!
//! `wait(2)` 看到的状态字只有 16 位，正常退出码被硬塞进高 8 位，低 8 位留
//! 给「是否被信号终止」和信号号。于是 `exit(256)`、`exit(-1)` 这类在 C 里
//! 完全合法的 `int` 实参，到了父进程眼里都会被**截断**：256 变 0，-1 变
//! 255。内核侧若直接把 `int` 当退出码往里塞，父进程与子进程对「退出码是
//! 多少」的认知就会分裂，而且这种分裂在任何日志里都看不出来。
//!
//! 本模块把「饱和」显式化，而不是悄悄截断：
//!
//! - 负值饱和为 0，并计入 `clamped_low`；
//! - 大于 255 的值饱和为 255，并计入 `clamped_high`；
//! - 状态字按 `code << 8` 编码，低 8 位恒 0，`low_byte` 可复核；
//! - 信号终止走另一个入口 `signal`，状态字低 8 位写 `0x7f` 标记，信号号
//!   放高 8 位。两类状态互斥：一旦记了正常退出，`term_signal` 永远
//!   `ENOTSIG`；一旦记了信号，`exit_status` 永远 `ENOTEXIT`。
//!
//! 闸门次序刻意让**参数校验压过状态校验**：
//!
//! - `record`：已记录 → `EREC`（没有别的参数可校验）；
//! - `signal`：信号号为 0 → `ESIG`，超过 64 → `ESIGRANGE`，然后才查
//!   「已记录」。
//!
//! 这样即便状态已经终结，`signal(0)` 仍然报参数错误——参数错误与状态无关，
//! 先钉参数可以让复现脚本在任何状态下得到同一结论。
//!
//! 账本十四格里特意安排了三个「成功但观测值为 0」的格（负值饱和、零退出码、
//! 零状态字）。它们证明对账必须看 `ok` 标志而不能用 `value != 0` 判成功。

use crate::alloc::format;
use crate::checks::CheckSet;

/// 段基址。
pub const SEG_BASE: u32 = 0xeb00;

/// 状态已记录。
pub const EC_EREC: u32 = SEG_BASE + 0x01;
/// 尚未记录任何状态。
pub const EC_EEMPTY: u32 = SEG_BASE + 0x02;
/// 信号状态没有退出码。
pub const EC_ENOTEXIT: u32 = SEG_BASE + 0x03;
/// 正常退出没有终止信号。
pub const EC_ENOTSIG: u32 = SEG_BASE + 0x04;
/// 信号号为零。
pub const EC_ESIG: u32 = SEG_BASE + 0x05;
/// 信号号越界。
pub const EC_ESIGRANGE: u32 = SEG_BASE + 0x06;
/// 账本失衡。
pub const EC_EBAL: u32 = SEG_BASE + 0x07;
/// 账本槽位乱序。
pub const EC_EORDER: u32 = SEG_BASE + 0x08;
/// 观测值与复算不符。
pub const EC_EMISMATCH: u32 = SEG_BASE + 0x09;
/// 冻结后重放不一致。
pub const EC_ESEAL: u32 = SEG_BASE + 0x0a;
/// 观测下标越界。
pub const EC_ENOBS: u32 = SEG_BASE + 0x0b;
/// 状态字位域非法。
pub const EC_EBITS: u32 = SEG_BASE + 0x0c;
/// 尚无可观测状态。
pub const EC_EWARM: u32 = SEG_BASE + 0x0d;

/// 账本格数。
pub const SAMPLES: usize = 14;
/// 退出码上限。
pub const MAX_CODE: i32 = 255;
/// 信号号上限（与 Linux 的 _NSIG-1 对齐）。
pub const MAX_SIG: u32 = 64;
/// 低字节掩码。
pub const LOW_MASK: u32 = 0xff;
/// 信号状态标记。
pub const SIG_MARK: u32 = 0x7f;

/// 操作码：记录退出码。
pub const OP_RECORD: u8 = 1;
/// 操作码：记录信号。
pub const OP_SIGNAL: u8 = 2;
/// 操作码：取退出码。
pub const OP_EXIT: u8 = 3;
/// 操作码：取信号。
pub const OP_TERM: u8 = 4;

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

/// 一份等待状态（退出码或终止信号，二选一）。
#[derive(Clone, Copy, Debug)]
pub struct Code {
    set: bool,
    signaled: bool,
    code: u32,
    sig: u32,
    status: u32,
    clamped_low: u32,
    clamped_high: u32,
}

impl Code {
    /// 空状态。
    pub const fn new() -> Code {
        Code {
            set: false,
            signaled: false,
            code: 0,
            sig: 0,
            status: 0,
            clamped_low: 0,
            clamped_high: 0,
        }
    }

    /// 是否已记录。
    pub fn is_set(&self) -> bool {
        self.set
    }

    /// 是否信号终止。
    pub fn is_signaled(&self) -> bool {
        self.set && self.signaled
    }

    /// 低位饱和次数。
    pub fn clamped_low(&self) -> u32 {
        self.clamped_low
    }

    /// 高位饱和次数。
    pub fn clamped_high(&self) -> u32 {
        self.clamped_high
    }

    /// 饱和总次数。
    pub fn clamps(&self) -> u32 {
        self.clamped_low.saturating_add(self.clamped_high)
    }

    /// 记录一次正常退出，返回等待状态字。
    ///
    /// 饱和是**值域收缩**而不是错误：`exit(-1)` 在 POSIX 上本来就只能被父
    /// 进程观察成 255，这里把收缩记进计数器，让「发生过截断」有痕迹。
    pub fn record(&mut self, code: i32) -> Result<u32, u32> {
        if self.set {
            return Err(EC_EREC);
        }
        let c = if code < 0 {
            self.clamped_low = self.clamped_low.saturating_add(1);
            0u32
        } else if code > MAX_CODE {
            self.clamped_high = self.clamped_high.saturating_add(1);
            MAX_CODE as u32
        } else {
            code as u32
        };
        self.set = true;
        self.signaled = false;
        self.code = c;
        self.sig = 0;
        self.status = c << 8;
        Ok(self.status)
    }

    /// 记录一次信号终止，返回等待状态字。
    pub fn signal(&mut self, sig: u32) -> Result<u32, u32> {
        if sig == 0 {
            return Err(EC_ESIG);
        }
        if sig > MAX_SIG {
            return Err(EC_ESIGRANGE);
        }
        if self.set {
            return Err(EC_EREC);
        }
        self.set = true;
        self.signaled = true;
        self.code = 0;
        self.sig = sig;
        self.status = (sig << 8) | SIG_MARK;
        Ok(self.status)
    }

    /// 取正常退出码。
    pub fn exit_status(&self) -> Result<u32, u32> {
        if !self.set {
            return Err(EC_EEMPTY);
        }
        if self.signaled {
            return Err(EC_ENOTEXIT);
        }
        Ok(self.code)
    }

    /// 取终止信号。
    pub fn term_signal(&self) -> Result<u32, u32> {
        if !self.set {
            return Err(EC_EEMPTY);
        }
        if !self.signaled {
            return Err(EC_ENOTSIG);
        }
        Ok(self.sig)
    }

    /// 原始等待状态字。
    pub fn raw_status(&self) -> Result<u32, u32> {
        if !self.set {
            return Err(EC_EEMPTY);
        }
        Ok(self.status)
    }

    /// 状态字低字节。
    ///
    /// 正常退出的低字节必须恒为 0；信号终止必须恒为 `0x7f`。这条不变量是
    /// 解码侧 `WIFEXITED`/`WIFSIGNALED` 判断的根，单独暴露出来便于判据复核。
    pub fn low_byte(&self) -> Result<u32, u32> {
        if !self.set {
            return Err(EC_EEMPTY);
        }
        Ok(self.status & LOW_MASK)
    }

    /// 从状态字高字节还原码值。
    pub fn exit_byte(&self) -> Result<u32, u32> {
        if !self.set {
            return Err(EC_EEMPTY);
        }
        if self.signaled {
            return Err(EC_ENOTEXIT);
        }
        let hi = (self.status >> 8) & LOW_MASK;
        if hi != self.code {
            return Err(EC_EBITS);
        }
        Ok(hi)
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
            return Err(EC_EORDER);
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
            return Err(EC_EBAL);
        }
        const VALS: [u32; 8] = [10752, 42, 65280, 255, 0, 0, 2431, 9];
        const CODES: [u32; 6] = [
            EC_EREC,
            EC_ENOTSIG,
            EC_ENOTEXIT,
            EC_EEMPTY,
            EC_ESIG,
            EC_ESIGRANGE,
        ];
        let mut vi = 0usize;
        let mut ci = 0usize;
        let mut i = 0usize;
        while i < self.len {
            let s = match self.rows.get(i) {
                Some(v) => *v,
                None => return Err(EC_EBAL),
            };
            if s.slot as usize != i {
                return Err(EC_EORDER);
            }
            if s.ok {
                if s.code != 0 {
                    return Err(EC_EMISMATCH);
                }
                match VALS.get(vi) {
                    Some(w) => {
                        if s.value != *w {
                            return Err(EC_EMISMATCH);
                        }
                    }
                    None => return Err(EC_EBAL),
                }
                vi += 1;
            } else {
                match CODES.get(ci) {
                    Some(w) => {
                        if s.code != *w {
                            return Err(EC_EMISMATCH);
                        }
                    }
                    None => return Err(EC_EBAL),
                }
                ci += 1;
            }
            i += 1;
        }
        if vi != 8 || ci != 6 {
            return Err(EC_EBAL);
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
            return Err(EC_EBAL);
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
            Err(EC_ESEAL)
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

/// 十四格账本：正常码 → 重复 → 非信号 → 高位饱和 → 负值饱和 → 信号 → 互斥 → 越界。
pub fn build_ledger() -> Ledger {
    let mut l = Ledger::new();

    let mut a = Code::new();
    let _ = l.register(word_cell(0, OP_RECORD, a.record(42)));
    let _ = l.register(word_cell(1, OP_EXIT, a.exit_status()));
    let _ = l.register(word_cell(2, OP_RECORD, a.record(43)));
    let _ = l.register(word_cell(3, OP_TERM, a.term_signal()));

    let mut b = Code::new();
    let _ = l.register(word_cell(4, OP_RECORD, b.record(300)));
    let _ = l.register(word_cell(5, OP_EXIT, b.exit_status()));

    let mut c = Code::new();
    let _ = l.register(word_cell(6, OP_RECORD, c.record(-5)));
    let _ = l.register(word_cell(7, OP_EXIT, c.exit_status()));

    let mut d = Code::new();
    let _ = l.register(word_cell(8, OP_SIGNAL, d.signal(9)));
    let _ = l.register(word_cell(9, OP_EXIT, d.exit_status()));
    let _ = l.register(word_cell(10, OP_TERM, d.term_signal()));

    let e = Code::new();
    let _ = l.register(word_cell(11, OP_EXIT, e.exit_status()));

    let mut f = Code::new();
    let _ = l.register(word_cell(12, OP_SIGNAL, f.signal(0)));
    let _ = l.register(word_cell(13, OP_SIGNAL, f.signal(65)));

    l
}

/// 四路注入。
pub fn inject_four() -> [bool; 4] {
    let mut out = [false; 4];

    let l = build_ledger();
    let rep = l.replay();
    let root = l.seal();
    out[0] = l.len() == SAMPLES && l.audit().is_ok() && rep == Ok(root) && root != 0;

    // 一路：位域布局。退出码放高 8 位，低 8 位恒零。
    let mut a = Code::new();
    let s1 = a.record(0);
    let b1 = a.low_byte();
    let e1 = a.exit_status();
    let mut b = Code::new();
    let s2 = b.record(255);
    let e2 = b.exit_byte();
    let mut c = Code::new();
    let s3 = c.record(1);
    out[1] = s1 == Ok(0)
        && b1 == Ok(0)
        && e1 == Ok(0)
        && s2 == Ok(65280)
        && e2 == Ok(255)
        && s3 == Ok(256)
        && b.raw_status() == Ok(65280)
        && !b.is_signaled()
        && a.clamps() == 0;

    // 二路：饱和边界与计数。
    let mut lo = Code::new();
    let r1 = lo.record(-1);
    let mut hi = Code::new();
    let r2 = hi.record(256);
    let mut hi2 = Code::new();
    let r3 = hi2.record(0x7fff_ffff);
    let mut mid = Code::new();
    let r4 = mid.record(255);
    out[2] = r1 == Ok(0)
        && lo.exit_status() == Ok(0)
        && lo.clamped_low() == 1
        && lo.clamped_high() == 0
        && r2 == Ok(65280)
        && hi.exit_status() == Ok(255)
        && hi.clamped_high() == 1
        && r3 == Ok(65280)
        && hi2.clamps() == 1
        && r4 == Ok(65280)
        && mid.clamps() == 0;

    // 三路：端子闸门与互斥。
    let mut s = Code::new();
    let g1 = s.signal(0) == Err(EC_ESIG);
    let g2 = s.signal(65) == Err(EC_ESIGRANGE);
    let g3 = s.signal(64) == Ok((64 << 8) | SIG_MARK);
    let g4 = s.exit_status() == Err(EC_ENOTEXIT);
    let g5 = s.term_signal() == Ok(64);
    let g6 = s.record(1) == Err(EC_EREC);
    let g7 = s.low_byte() == Ok(SIG_MARK);
    let mut n = Code::new();
    let g8 = n.term_signal() == Err(EC_EEMPTY);
    let g9 = n.exit_status() == Err(EC_EEMPTY);
    let g10 = n.low_byte() == Err(EC_EEMPTY);
    let _ = n.record(7);
    let g11 = n.term_signal() == Err(EC_ENOTSIG);
    let g12 = n.low_byte() == Ok(0);
    out[3] = g1 && g2 && g3 && g4 && g5 && g6 && g7 && g8 && g9 && g10 && g11 && g12;

    out
}

/// 十三项判据。
pub fn run_unx11336_checks() -> CheckSet {
    let mut s = CheckSet::new("unx11336.exit_code_sat");
    let l = build_ledger();

    s.add_dyn(
        "EC1-十四格八允六拒",
        l.len() == SAMPLES && l.ok_count() == 8 && l.err_count() == 6 && l.audit().is_ok(),
        &format!("n={} ok={} err={}", l.len(), l.ok_count(), l.err_count()),
    );

    s.add(
        "EC2-正常退出码编码",
        l.sample(0).map(|x| x.value) == Some(42 << 8),
        "static",
    );

    s.add(
        "EC3-解码还原退出码",
        l.sample(1).map(|x| x.value) == Some(42),
        "static",
    );

    s.add(
        "EC4-重复记录被拒",
        l.sample(2).map(|x| x.code) == Some(EC_EREC),
        "static",
    );

    s.add(
        "EC5-正常退出没有终止信号",
        l.sample(3).map(|x| x.code) == Some(EC_ENOTSIG),
        "static",
    );

    s.add(
        "EC6-超上限饱和为二五五",
        l.sample(4).map(|x| x.value) == Some(255 << 8)
            && l.sample(5).map(|x| x.value) == Some(255),
        "static",
    );

    s.add(
        "EC7-负值饱和为零",
        l.sample(6).map(|x| x.value) == Some(0)
            && l.sample(7).map(|x| x.value) == Some(0),
        "static",
    );

    s.add(
        "EC8-信号状态编码",
        l.sample(8).map(|x| x.value) == Some((9 << 8) | SIG_MARK),
        "static",
    );

    s.add(
        "EC9-信号状态无退出码",
        l.sample(9).map(|x| x.code) == Some(EC_ENOTEXIT),
        "static",
    );

    s.add(
        "EC10-终止信号还原",
        l.sample(10).map(|x| x.value) == Some(9),
        "static",
    );

    s.add(
        "EC11-暂未记录与信号号闸门",
        l.sample(11).map(|x| x.code) == Some(EC_EEMPTY)
            && l.sample(12).map(|x| x.code) == Some(EC_ESIG)
            && l.sample(13).map(|x| x.code) == Some(EC_ESIGRANGE),
        "static",
    );

    s.add(
        "EC12-饱和计数与位域真值",
        {
            let mut c = Code::new();
            let r1 = c.record(-100);
            let low = c.low_byte();
            let sig = c.is_signaled();
            let mut d = Code::new();
            let r2 = d.record(0x0100);
            let w = d.raw_status();
            let e = d.exit_byte();
            r1 == Ok(0)
                && low == Ok(0)
                && !sig
                && c.clamped_low() == 1
                && r2 == Ok(0xff00)
                && w == Ok(0xff00)
                && e == Ok(0xff)
                && d.clamped_high() == 1
                && d.clamps() == 1
        },
        "static",
    );

    s.add(
        "EC13-冻结重放四注入",
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
        let s = run_unx11336_checks();
        assert_eq!(s.len(), 13);
        assert!(s.all_passed());
    }

    #[test]
    fn code_is_high_byte() {
        let mut c = Code::new();
        assert_eq!(c.record(7), Ok(7 << 8));
        assert_eq!(c.low_byte(), Ok(0));
        assert_eq!(c.exit_status(), Ok(7));
        assert_eq!(c.exit_byte(), Ok(7));
        assert_eq!(c.term_signal(), Err(EC_ENOTSIG));
    }

    #[test]
    fn saturation_is_counted() {
        let mut c = Code::new();
        assert_eq!(c.record(-3), Ok(0));
        assert_eq!(c.clamped_low(), 1);
        assert_eq!(c.clamped_high(), 0);
        let mut d = Code::new();
        assert_eq!(d.record(256), Ok(0xff00));
        assert_eq!(d.clamped_high(), 1);
        assert_eq!(d.exit_byte(), Ok(0xff));
    }

    #[test]
    fn signal_status_is_disjoint() {
        let mut c = Code::new();
        assert_eq!(c.signal(11), Ok((11 << 8) | SIG_MARK));
        assert_eq!(c.exit_status(), Err(EC_ENOTEXIT));
        assert_eq!(c.term_signal(), Ok(11));
        assert_eq!(c.low_byte(), Ok(SIG_MARK));
        assert_eq!(c.record(0), Err(EC_EREC));
    }

    #[test]
    fn signal_gates_come_first() {
        let mut c = Code::new();
        assert_eq!(c.signal(0), Err(EC_ESIG));
        assert_eq!(c.signal(65), Err(EC_ESIGRANGE));
        assert!(!c.is_set());
        assert_eq!(c.signal(64), Ok((64 << 8) | SIG_MARK));
        assert_eq!(c.signal(1), Err(EC_EREC));
    }

    #[test]
    fn empty_status_has_no_reading() {
        let c = Code::new();
        assert_eq!(c.exit_status(), Err(EC_EEMPTY));
        assert_eq!(c.term_signal(), Err(EC_EEMPTY));
        assert_eq!(c.raw_status(), Err(EC_EEMPTY));
        assert_eq!(c.low_byte(), Err(EC_EEMPTY));
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
