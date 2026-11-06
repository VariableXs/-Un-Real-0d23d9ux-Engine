//! UNX-F11325 退出码与 wait 状态字（0xea50 段）
//!
//! 子进程的结局压在一个 16 位状态字里，Linux 的编码是「低七位是结局，高八位是
//! 细节」：正常退出低七位为 0、`code << 8`；被信号杀死低七位是信号号、第 8 位
//! 表示是否 core；被暂停低七位固定 0x7f、高八位是暂停信号；继续运行则是全 1。
//! 这套编码最大的危险是「看起来都合法」：0x7f 的低七位配上高八位 0 在语法上成立、
//! 语义上却是「信号 0 暂停」，必须单独判死。
//!
//! 本模块给出三层：
//!   1. 编码器：`encode_exit / encode_signal / encode_stop / encode_cont`；
//!   2. 解码器：`classify` 先查高位污染，再按低七位分流，最后校验信号区间；
//!   3. 子表 `WaitBook`：登记、退出、等待、收尸，把「僵尸只可被等一次」钉死。
//!
//! 闸门次序（`classify`）：高位污染 → 继续运行 → 正常退出 → 暂停 → 有信号 → 非法
//! 暂停。`wait` 的闸门是：指定 pid 不在册 → 非僵尸 → 无僵尸可等。

use crate::alloc::format;
use crate::checks::CheckSet;

/// 段基址。
pub const SEG_BASE: u32 = 0xea50;

/// pid 为零。
pub const WT_EINVAL: u32 = SEG_BASE + 0x01;
/// 指定 pid 不在册。
pub const WT_ESRCH: u32 = SEG_BASE + 0x02;
/// 没有僵尸可等。
pub const WT_ECHILD: u32 = SEG_BASE + 0x03;
/// 信号号越界。
pub const WT_ESIG: u32 = SEG_BASE + 0x04;
/// 状态字高位被污染。
pub const WT_ESTAT: u32 = SEG_BASE + 0x05;
/// 暂停/继续语义非法（信号 0）。
pub const WT_EBADSIG: u32 = SEG_BASE + 0x06;
/// 该子进程已经退出过。
pub const WT_EDEAD: u32 = SEG_BASE + 0x07;
/// 子表满。
pub const WT_EFULL: u32 = SEG_BASE + 0x08;
/// pid 重复。
pub const WT_EDUP: u32 = SEG_BASE + 0x09;
/// 账本形状不符。
pub const WT_EBAL: u32 = SEG_BASE + 0x0a;
/// 槽位乱序。
pub const WT_EORDER: u32 = SEG_BASE + 0x0b;
/// 观测值与复算不符。
pub const WT_EMISMATCH: u32 = SEG_BASE + 0x0c;
/// 冻结后重放不一致。
pub const WT_ESEAL: u32 = SEG_BASE + 0x0d;

/// 结局类别：正常退出。
pub const KIND_EXIT: u32 = 1;
/// 结局类别：被信号杀死。
pub const KIND_SIGNAL: u32 = 2;
/// 结局类别：继续运行。
pub const KIND_CONT: u32 = 3;
/// 结局类别：被暂停。
pub const KIND_STOP: u32 = 4;

/// 子表容量。
pub const CAP: usize = 16;
/// 信号上界。
pub const MAX_SIG: u32 = 64;
/// 账本格数。
pub const SAMPLES: usize = 14;
/// 操作码：登记子进程。
pub const OP_SPAWN: u8 = 1;
/// 操作码：子进程退出。
pub const OP_EXIT: u8 = 2;
/// 操作码：等待。
pub const OP_WAIT: u8 = 3;
/// 操作码：解码/编码。
pub const OP_CODE: u8 = 4;

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

/// 正常退出编码。
pub fn encode_exit(code: u32) -> u32 {
    (code & 0xff) << 8
}

/// 信号终止编码：`core` 置位即写第 8 位。
pub fn encode_signal(sig: u32, core: bool) -> u32 {
    let low = if core { (sig & 0x7f) | 0x80 } else { sig & 0x7f };
    low
}

/// 暂停编码。
pub fn encode_stop(sig: u32) -> u32 {
    ((sig & 0xff) << 8) | 0x7f
}

/// 继续运行编码。
pub const fn encode_cont() -> u32 {
    0xffff
}

/// 解码结论。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Decoded {
    /// 类别。
    pub kind: u32,
    /// 退出码或信号号。
    pub value: u32,
    /// 是否 core。
    pub core: bool,
}

impl Decoded {
    /// 空结论。
    pub const fn blank() -> Decoded {
        Decoded { kind: 0, value: 0, core: false }
    }
}

/// 判定状态字类别。
///
/// 闸门：高位污染 → 全 1 继续 → 低七位为零（正常退出）→ 低七位 0x7f（暂停）→
/// 其余有信号终止。暂停与继续的信号号为 0 一律判非法。
pub fn classify(st: u32) -> Result<Decoded, u32> {
    if st > 0xffff {
        return Err(WT_ESTAT);
    }
    if st == encode_cont() {
        return Ok(Decoded { kind: KIND_CONT, value: 0, core: false });
    }
    let low = st & 0x7f;
    if low == 0 {
        let code = (st >> 8) & 0xff;
        let core = st & 0x80 != 0;
        if core {
            // 正常退出不该带 core 位。
            return Err(WT_ESTAT);
        }
        return Ok(Decoded { kind: KIND_EXIT, value: code, core: false });
    }
    if low == 0x7f {
        let sig = (st >> 8) & 0xff;
        if sig == 0 || sig > MAX_SIG {
            return Err(WT_EBADSIG);
        }
        return Ok(Decoded { kind: KIND_STOP, value: sig, core: false });
    }
    if low > MAX_SIG {
        return Err(WT_ESIG);
    }
    Ok(Decoded { kind: KIND_SIGNAL, value: low, core: st & 0x80 != 0 })
}

/// 子进程结局。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Child {
    /// 进程号。
    pub pid: u32,
    /// 0 运行中 / 1 僵尸 / 2 已收尸。
    pub state: u8,
    /// 状态字。
    pub status: u32,
}

impl Child {
    /// 空项。
    pub const fn blank() -> Child {
        Child { pid: 0, state: 0, status: 0 }
    }
}

/// 子表。
#[derive(Clone, Copy, Debug)]
pub struct WaitBook {
    kids: [Child; CAP],
    n: usize,
    waits: u32,
    reaps: u32,
}

impl WaitBook {
    /// 空表。
    pub const fn new() -> WaitBook {
        WaitBook { kids: [Child::blank(); CAP], n: 0, waits: 0, reaps: 0 }
    }

    /// 在册数。
    pub fn len(&self) -> usize {
        self.n
    }

    /// 等待次数。
    pub fn waits(&self) -> u32 {
        self.waits
    }

    /// 收尸次数。
    pub fn reaps(&self) -> u32 {
        self.reaps
    }

    /// 僵尸数。
    pub fn zombies(&self) -> usize {
        self.count_state(1)
    }

    /// 存活数。
    pub fn live(&self) -> usize {
        self.count_state(0)
    }

    /// 已收尸数。
    pub fn reaped(&self) -> usize {
        self.count_state(2)
    }

    fn count_state(&self, want: u8) -> usize {
        let mut c = 0usize;
        let mut i = 0usize;
        while i < self.n {
            if let Some(k) = self.kids.get(i) {
                if k.state == want {
                    c += 1;
                }
            }
            i += 1;
        }
        c
    }

    /// 按下标取。
    pub fn at(&self, i: usize) -> Option<Child> {
        if i >= self.n {
            return None;
        }
        self.kids.get(i).copied()
    }

    /// 按 pid 找下标。
    pub fn index_of(&self, pid: u32) -> Option<usize> {
        let mut i = 0usize;
        while i < self.n {
            if let Some(k) = self.kids.get(i) {
                if k.pid == pid {
                    return Some(i);
                }
            }
            i += 1;
        }
        None
    }

    /// 登记子进程。
    pub fn spawn(&mut self, pid: u32) -> Result<usize, u32> {
        if pid == 0 {
            return Err(WT_EINVAL);
        }
        if self.index_of(pid).is_some() {
            return Err(WT_EDUP);
        }
        if self.n >= CAP {
            return Err(WT_EFULL);
        }
        if let Some(dst) = self.kids.get_mut(self.n) {
            *dst = Child { pid, state: 0, status: 0 };
        }
        self.n += 1;
        Ok(self.n)
    }

    /// 子进程以退出码结束，落成僵尸。返回状态字。
    pub fn exit_child(&mut self, pid: u32, code: u32) -> Result<u32, u32> {
        if code > 0xff {
            return Err(WT_EINVAL);
        }
        let i = match self.index_of(pid) {
            Some(v) => v,
            None => return Err(WT_ESRCH),
        };
        let st = encode_exit(code);
        match self.kids.get_mut(i) {
            Some(k) => {
                if k.state != 0 {
                    return Err(WT_EDEAD);
                }
                k.state = 1;
                k.status = st;
            }
            None => return Err(WT_ESRCH),
        }
        Ok(st)
    }

    /// 子进程被信号杀死，落成僵尸。
    pub fn signal_child(&mut self, pid: u32, sig: u32, core: bool) -> Result<u32, u32> {
        if sig == 0 || sig > MAX_SIG {
            return Err(WT_ESIG);
        }
        let i = match self.index_of(pid) {
            Some(v) => v,
            None => return Err(WT_ESRCH),
        };
        let st = encode_signal(sig, core);
        match self.kids.get_mut(i) {
            Some(k) => {
                if k.state != 0 {
                    return Err(WT_EDEAD);
                }
                k.state = 1;
                k.status = st;
            }
            None => return Err(WT_ESRCH),
        }
        Ok(st)
    }

    /// 等指定 pid：必须已在册且已僵尸。
    pub fn wait_pid(&mut self, pid: u32) -> Result<(u32, u32), u32> {
        let i = match self.index_of(pid) {
            Some(v) => v,
            None => return Err(WT_ECHILD),
        };
        match self.kids.get_mut(i) {
            Some(k) => {
                if k.state != 1 {
                    return Err(WT_ECHILD);
                }
                k.state = 2;
                self.waits += 1;
                self.reaps += 1;
                Ok((pid, k.status))
            }
            None => Err(WT_ECHILD),
        }
    }

    /// 等任一僵尸：按下标序取第一个，取不到即无子可等。
    pub fn wait_any(&mut self) -> Result<(u32, u32), u32> {
        let mut i = 0usize;
        while i < self.n {
            let hit = match self.kids.get(i) {
                Some(k) => k.state == 1,
                None => false,
            };
            if hit {
                if let Some(k) = self.kids.get_mut(i) {
                    k.state = 2;
                    let out = (k.pid, k.status);
                    self.waits += 1;
                    self.reaps += 1;
                    return Ok(out);
                }
            }
            i += 1;
        }
        Err(WT_ECHILD)
    }

    /// 指纹。
    pub fn seal(&self) -> u64 {
        let mut h = FNV_OFF;
        h = fold(h, self.n as u64);
        let mut i = 0usize;
        while i < self.n {
            if let Some(k) = self.kids.get(i) {
                h = fold(h, k.pid as u64);
                h = fold(h, k.state as u64);
                h = fold(h, k.status as u64);
            }
            i += 1;
        }
        h = fold(h, self.waits as u64);
        fold(h, self.reaps as u64)
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
            return Err(WT_EORDER);
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
            return Err(WT_EBAL);
        }
        const VALS: [u32; 8] = [1, 2, 1792, 1792, 101, 7, 137, 3];
        const CODES: [u32; 6] = [WT_EINVAL, WT_ESRCH, WT_ECHILD, WT_ESIG, WT_EBADSIG, WT_ESTAT];
        let mut vi = 0usize;
        let mut ci = 0usize;
        let mut i = 0usize;
        while i < self.len {
            let s = match self.rows.get(i) {
                Some(v) => *v,
                None => return Err(WT_EBAL),
            };
            if s.slot as usize != i {
                return Err(WT_EORDER);
            }
            if s.ok {
                if s.code != 0 {
                    return Err(WT_EMISMATCH);
                }
                match VALS.get(vi) {
                    Some(w) => {
                        if s.value != *w {
                            return Err(WT_EMISMATCH);
                        }
                    }
                    None => return Err(WT_EBAL),
                }
                vi += 1;
            } else {
                match CODES.get(ci) {
                    Some(w) => {
                        if s.code != *w {
                            return Err(WT_EMISMATCH);
                        }
                    }
                    None => return Err(WT_EBAL),
                }
                ci += 1;
            }
            i += 1;
        }
        if vi != 8 || ci != 6 {
            return Err(WT_EBAL);
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
            return Err(WT_EBAL);
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
            Err(WT_ESEAL)
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

/// 十四格账本：一台子表走完「登记 → 退出 → 等待」，另一笔记编码与解码。
pub fn build_ledger() -> Ledger {
    let mut l = Ledger::new();
    let mut b = WaitBook::new();

    let _ = l.register(cell(0, OP_SPAWN, b.spawn(101)));
    let _ = l.register(cell(1, OP_SPAWN, b.spawn(102)));
    let _ = l.register(word_cell(2, OP_EXIT, b.exit_child(101, 7)));
    let _ = l.register(Sample {
        slot: 3,
        op: OP_CODE,
        ok: true,
        code: 0,
        value: b.at(0).map(|k| k.status).unwrap_or(0),
    });
    let _ = l.register(Sample {
        slot: 4,
        op: OP_WAIT,
        ok: true,
        code: 0,
        value: b.wait_pid(101).map(|p| p.0).unwrap_or(0),
    });
    let _ = l.register(match classify(0x700) {
        Ok(d) => Sample { slot: 5, op: OP_CODE, ok: true, code: 0, value: d.value },
        Err(c) => Sample { slot: 5, op: OP_CODE, ok: false, code: c, value: 0 },
    });
    let _ = l.register(Sample {
        slot: 6,
        op: OP_CODE,
        ok: true,
        code: 0,
        value: encode_signal(9, true),
    });
    let _ = l.register(match classify(encode_cont()) {
        Ok(d) => Sample { slot: 7, op: OP_CODE, ok: true, code: 0, value: d.kind },
        Err(c) => Sample { slot: 7, op: OP_CODE, ok: false, code: c, value: 0 },
    });

    let mut w = WaitBook::new();
    let _ = l.register(cell(8, OP_SPAWN, w.spawn(0)));
    let _ = l.register(word_cell(9, OP_EXIT, b.exit_child(999, 1)));
    // 第 10 格：只剩一个活着的子进程，没有任何僵尸可等。
    let _ = l.register(match b.wait_any() {
        Ok(_) => Sample { slot: 10, op: OP_WAIT, ok: true, code: 0, value: 0 },
        Err(c) => Sample { slot: 10, op: OP_WAIT, ok: false, code: c, value: 0 },
    });

    // 第 11 格：低七位越过信号上界。
    let _ = l.register(match classify(0x41) {
        Ok(d) => Sample { slot: 11, op: OP_CODE, ok: true, code: 0, value: d.value },
        Err(c) => Sample { slot: 11, op: OP_CODE, ok: false, code: c, value: 0 },
    });
    let _ = l.register(match classify(encode_stop(0)) {
        Ok(d) => Sample { slot: 12, op: OP_CODE, ok: true, code: 0, value: d.value },
        Err(c) => Sample { slot: 12, op: OP_CODE, ok: false, code: c, value: 0 },
    });
    let _ = l.register(match classify(0x0001_0000) {
        Ok(d) => Sample { slot: 13, op: OP_CODE, ok: true, code: 0, value: d.value },
        Err(c) => Sample { slot: 13, op: OP_CODE, ok: false, code: c, value: 0 },
    });

    l
}

/// 四路注入。
pub fn inject_four() -> [bool; 4] {
    let mut out = [false; 4];

    let l = build_ledger();
    let rep = l.replay();
    let root = l.seal();
    out[0] = l.len() == SAMPLES && l.audit().is_ok() && rep == Ok(root) && root != 0;

    // 一路：四类编码自洽往返。
    let ex = encode_exit(0x107);
    let sg = encode_signal(11, true);
    let st = encode_stop(19);
    let e1 = classify(ex) == Ok(Decoded { kind: KIND_EXIT, value: 7, core: false });
    let e2 = classify(sg) == Ok(Decoded { kind: KIND_SIGNAL, value: 11, core: true });
    let e3 = classify(st) == Ok(Decoded { kind: KIND_STOP, value: 19, core: false });
    let e4 = classify(encode_cont()) == Ok(Decoded { kind: KIND_CONT, value: 0, core: false });
    out[1] = e1 && e2 && e3 && e4;

    // 二路：僵尸只可被等一次。
    let mut b = WaitBook::new();
    let _ = b.spawn(1);
    let _ = b.spawn(2);
    let _ = b.exit_child(2, 3);
    let first = b.wait_any() == Ok((2, encode_exit(3)));
    let second = b.wait_any() == Err(WT_ECHILD);
    let third = b.wait_pid(2) == Err(WT_ECHILD);
    out[2] = first && second && third && b.zombies() == 0 && b.reaped() == 1 && b.live() == 1;

    // 三路：非法状态字逐档拒绝。
    let f1 = classify(0x1_0000) == Err(WT_ESTAT);
    let f2 = classify(0x80) == Err(WT_ESTAT);
    let f3 = classify(encode_stop(0)) == Err(WT_EBADSIG);
    let f4 = classify(0x80 | 0x50) == Err(WT_ESIG);
    let f5 = classify(0x7f | (65 << 8)) == Err(WT_EBADSIG);
    out[3] = f1 && f2 && f3 && f4 && f5;

    out
}

/// 十三项判据。
pub fn run_unx11325_checks() -> CheckSet {
    let mut s = CheckSet::new("unx11325.wait_status");
    let l = build_ledger();

    s.add_dyn(
        "WT1-十四格八允六拒",
        l.len() == SAMPLES && l.ok_count() == 8 && l.err_count() == 6 && l.audit().is_ok(),
        &format!("n={} ok={} err={}", l.len(), l.ok_count(), l.err_count()),
    );

    s.add(
        "WT2-登记计数递增",
        l.sample(0).map(|x| x.value) == Some(1) && l.sample(1).map(|x| x.value) == Some(2),
        "static",
    );

    s.add(
        "WT3-退出码左移八位",
        l.sample(2).map(|x| x.value) == Some(0x700) && l.sample(3).map(|x| x.value) == Some(0x700),
        "static",
    );

    s.add(
        "WT4-等待返回 pid 并收尸",
        l.sample(4).map(|x| x.value) == Some(101),
        "static",
    );

    s.add(
        "WT5-解码取回原退出码",
        l.sample(5).map(|x| x.value) == Some(7),
        "static",
    );

    s.add(
        "WT6-core 位编码",
        l.sample(6).map(|x| x.value) == Some(0x89),
        "static",
    );

    s.add(
        "WT7-全一为继续运行",
        l.sample(7).map(|x| x.value) == Some(KIND_CONT),
        "static",
    );

    s.add(
        "WT8-零 pid 与未知 pid",
        l.sample(8).map(|x| x.code) == Some(WT_EINVAL) && l.sample(9).map(|x| x.code) == Some(WT_ESRCH),
        "static",
    );

    s.add(
        "WT9-无僵尸可等",
        l.sample(10).map(|x| x.code) == Some(WT_ECHILD) && l.sample(10).map(|x| x.ok) == Some(false),
        "static",
    );

    s.add(
        "WT10-信号零与高位污染",
        l.sample(11).map(|x| x.code) == Some(WT_ESIG)
            && l.sample(13).map(|x| x.code) == Some(WT_ESTAT),
        "static",
    );

    s.add(
        "WT11-暂停零信号非法",
        l.sample(12).map(|x| x.code) == Some(WT_EBADSIG),
        "static",
    );

    s.add(
        "WT12-四类往返自洽",
        {
            let a = classify(encode_exit(255)) == Ok(Decoded { kind: KIND_EXIT, value: 255, core: false });
            let b = classify(encode_signal(64, false)) == Ok(Decoded { kind: KIND_SIGNAL, value: 64, core: false });
            let c = classify(encode_stop(64)) == Ok(Decoded { kind: KIND_STOP, value: 64, core: false });
            a && b && c
        },
        "static",
    );

    s.add(
        "WT13-冻结重放四注入",
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
        let s = run_unx11325_checks();
        assert_eq!(s.len(), 13);
        assert!(s.all_passed());
    }

    #[test]
    fn encode_decode_roundtrip() {
        assert_eq!(classify(encode_exit(1)), Ok(Decoded { kind: KIND_EXIT, value: 1, core: false }));
        assert_eq!(classify(encode_signal(11, true)), Ok(Decoded { kind: KIND_SIGNAL, value: 11, core: true }));
        assert_eq!(classify(encode_stop(9)), Ok(Decoded { kind: KIND_STOP, value: 9, core: false }));
    }

    #[test]
    fn zombie_is_waited_once() {
        let mut b = WaitBook::new();
        assert_eq!(b.spawn(7), Ok(1));
        assert_eq!(b.exit_child(7, 4), Ok(0x400));
        assert_eq!(b.wait_pid(7), Ok((7, 0x400)));
        assert_eq!(b.wait_pid(7), Err(WT_ECHILD));
        assert_eq!(b.reaped(), 1);
    }

    #[test]
    fn live_child_cannot_be_waited() {
        let mut b = WaitBook::new();
        let _ = b.spawn(8);
        assert_eq!(b.wait_any(), Err(WT_ECHILD));
        assert_eq!(b.wait_pid(8), Err(WT_ECHILD));
        assert_eq!(b.wait_pid(9), Err(WT_ECHILD));
    }

    #[test]
    fn double_exit_is_rejected() {
        let mut b = WaitBook::new();
        let _ = b.spawn(9);
        assert_eq!(b.exit_child(9, 0), Ok(0));
        assert_eq!(b.exit_child(9, 0), Err(WT_EDEAD));
        assert_eq!(b.signal_child(9, 9, false), Err(WT_EDEAD));
    }

    #[test]
    fn signal_bounds() {
        assert_eq!(classify(0x81), Ok(Decoded { kind: KIND_SIGNAL, value: 1, core: true }));
        assert_eq!(classify(0x7f | (0u32 << 8)), Err(WT_EBADSIG));
        assert_eq!(classify(0x7f | (100u32 << 8)), Err(WT_EBADSIG));
        assert_eq!(classify(0x41), Err(WT_ESIG));
    }

    #[test]
    fn freeze_blocks_writes() {
        let mut l = build_ledger();
        assert!(l.freeze().is_ok());
        assert!(l.register(Sample::blank()).is_err());
        assert_eq!(l.rejects(), 1);
    }
}
