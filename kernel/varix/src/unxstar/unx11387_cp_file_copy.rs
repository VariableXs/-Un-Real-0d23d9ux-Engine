//! UNX-F11387 cp 基本复制语义（文件→文件）
//!
//! 文件到文件复制的三条真值：**内容等价**（尺寸与源一致）、**权限派生**
//! （新建目标取 `源模式 & ~umask`；目标已存在则保留目标模式）、**时间派生**
//! （mtime 随源，结算时刻记 `now`）。同源同目标不拷，目录不在此篇（见 cp -r）。
//!
//! 闸门次序：
//! - `open`：umask 越出权限位 → 时钟为零；
//! - `src`：非普通文件 → 尺寸超上限；
//! - `dst`：同源同目标 → 目标是目录 → 目标不可写；
//! - `finish`：缺源 → 缺目标。
//!
//! 账本形状：14 格 = 8 允 + 6 拒，6 拒覆盖 6 个不同域错误码。

use crate::alloc::format;
use crate::checks::CheckSet;

/// 段基址。
pub const SEG_BASE: u32 = 0xee30;

/// umask 越出权限位。
pub const CPF_EUMASK: u32 = SEG_BASE + 0x01;
/// 时钟为零。
pub const CPF_ECLOCK: u32 = SEG_BASE + 0x02;
/// 源不是普通文件。
pub const CPF_EKIND: u32 = SEG_BASE + 0x03;
/// 源尺寸超限。
pub const CPF_ESIZE: u32 = SEG_BASE + 0x04;
/// 目标不可写。
pub const CPF_EPERM: u32 = SEG_BASE + 0x05;
/// 同源同目标。
pub const CPF_ESAME: u32 = SEG_BASE + 0x06;
/// 缺源。
pub const CPF_ENOSRC: u32 = SEG_BASE + 0x07;
/// 缺目标。
pub const CPF_ENODST: u32 = SEG_BASE + 0x08;
/// 账本形状不符。
pub const CPF_EBAL: u32 = SEG_BASE + 0x09;
/// 槽位乱序。
pub const CPF_ESLOT: u32 = SEG_BASE + 0x0a;
/// 观测值与复算不符。
pub const CPF_EVALUE: u32 = SEG_BASE + 0x0b;
/// 冻结后重放不一致。
pub const CPF_ESEAL: u32 = SEG_BASE + 0x0c;
/// 观察下标越界。
pub const CPF_ENOBS: u32 = SEG_BASE + 0x0d;

/// 单次只挂一对源/目标。
pub const CAP: usize = 1;
/// 源尺寸上限（1 MiB）。
pub const SIZE_LIMIT: u64 = 1 << 20;
/// 权限位掩码。
pub const MODE_MASK: u32 = 0o7777;
/// 文件类型掩码。
pub const TYPE_MASK: u32 = 0o170000;
/// 普通文件。
pub const KIND_REG: u8 = 1;
/// 目录。
pub const KIND_DIR: u8 = 2;
/// 符号链接。
pub const KIND_LNK: u8 = 3;
/// 命名管道。
pub const KIND_FIFO: u8 = 4;
/// 账本格数。
pub const SAMPLES: usize = 14;

/// 操作码：登记源。
pub const OP_SRC: u8 = 1;
/// 操作码：登记目标。
pub const OP_DST: u8 = 2;
/// 操作码：结算。
pub const OP_FINISH: u8 = 3;
/// 操作码：开户。
pub const OP_OPEN: u8 = 4;

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

/// 一次文件复制的账。
#[derive(Clone, Copy, Debug)]
pub struct Cp {
    umask: u32,
    now: u64,
    s_set: bool,
    s_mode: u32,
    s_size: u64,
    s_mtime: u64,
    d_set: bool,
    d_exists: bool,
    d_mode: u32,
    done: bool,
    derived: u32,
}

impl Cp {
    /// 按 umask 与结算时钟开户。
    pub fn open(umask: u32, now: u64) -> Result<Cp, u32> {
        if umask & !MODE_MASK != 0 {
            return Err(CPF_EUMASK);
        }
        if now == 0 {
            return Err(CPF_ECLOCK);
        }
        Ok(Cp {
            umask,
            now,
            s_set: false,
            s_mode: 0,
            s_size: 0,
            s_mtime: 0,
            d_set: false,
            d_exists: false,
            d_mode: 0,
            done: false,
            derived: 0,
        })
    }

    /// umask。
    pub fn umask(&self) -> u32 {
        self.umask
    }

    /// 结算时钟。
    pub fn now(&self) -> u64 {
        self.now
    }

    /// 源是否已登记。
    pub fn has_src(&self) -> bool {
        self.s_set
    }

    /// 目标是否已登记。
    pub fn has_dst(&self) -> bool {
        self.d_set
    }

    /// 源尺寸。
    pub fn size(&self) -> u64 {
        self.s_size
    }

    /// 源 mtime。
    pub fn mtime(&self) -> u64 {
        self.s_mtime
    }

    /// 派生出的目标模式。
    pub fn derived(&self) -> u32 {
        self.derived
    }

    /// 是否已结算。
    pub fn is_done(&self) -> bool {
        self.done
    }

    /// 登记源，返回已挂载对数。
    ///
    /// 闸门次序：非普通文件 → 尺寸超限。
    pub fn src(&mut self, mode: u32, size: u64, mtime: u64, kind: u8) -> Result<usize, u32> {
        if kind != KIND_REG {
            return Err(CPF_EKIND);
        }
        if size > SIZE_LIMIT {
            return Err(CPF_ESIZE);
        }
        self.s_set = true;
        self.s_mode = mode & MODE_MASK;
        self.s_size = size;
        self.s_mtime = mtime;
        self.done = false;
        Ok(1)
    }

    /// 登记目标，返回已挂载对数。
    ///
    /// 闸门次序：同源同目标 → 目标是目录 → 目标不可写。
    pub fn dst(&mut self, exists: bool, mode: u32, same: bool) -> Result<usize, u32> {
        if same {
            return Err(CPF_ESAME);
        }
        if exists && (mode & TYPE_MASK) == 0o040000 {
            return Err(CPF_EKIND);
        }
        if exists && (mode & 0o200) == 0 {
            return Err(CPF_EPERM);
        }
        self.d_set = true;
        self.d_exists = exists;
        self.d_mode = mode & MODE_MASK;
        self.done = false;
        Ok(1)
    }

    /// 结算：返回派生目标模式。
    ///
    /// 闸门次序：缺源 → 缺目标。
    pub fn finish(&mut self) -> Result<u32, u32> {
        if !self.s_set {
            return Err(CPF_ENOSRC);
        }
        if !self.d_set {
            return Err(CPF_ENODST);
        }
        self.derived = self.expect_derived();
        self.done = true;
        Ok(self.derived)
    }

    /// 期望派生模式。
    fn expect_derived(&self) -> u32 {
        if self.d_exists {
            self.d_mode & MODE_MASK
        } else {
            self.s_mode & !self.umask & MODE_MASK
        }
    }

    /// 指纹。
    pub fn seal(&self) -> u64 {
        let mut h = FNV_OFF;
        h = fold(h, self.umask as u64);
        h = fold(h, self.now);
        h = fold(h, self.s_set as u64);
        h = fold(h, self.s_mode as u64);
        h = fold(h, self.s_size);
        h = fold(h, self.s_mtime);
        h = fold(h, self.d_set as u64);
        h = fold(h, self.d_exists as u64);
        h = fold(h, self.d_mode as u64);
        h = fold(h, self.derived as u64);
        fold(h, self.done as u64)
    }

    /// 不变式自检。
    pub fn consistent(&self) -> bool {
        if self.umask & !MODE_MASK != 0 || self.now == 0 {
            return false;
        }
        if self.s_set {
            if self.s_mode & !MODE_MASK != 0 || self.s_size > SIZE_LIMIT {
                return false;
            }
        }
        if self.d_set {
            if self.d_mode & !MODE_MASK != 0 {
                return false;
            }
        }
        if self.done {
            if !self.s_set || !self.d_set {
                return false;
            }
            if self.derived != self.expect_derived() {
                return false;
            }
        }
        true
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
            return Err(CPF_ESLOT);
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
            return Err(CPF_EBAL);
        }
        const VALS: [u32; 8] = [1, 1, 420, 1, 1, 384, 416, 436];
        const CODES: [u32; 6] =
            [CPF_EUMASK, CPF_ECLOCK, CPF_EKIND, CPF_ESIZE, CPF_EPERM, CPF_ENOSRC];
        let mut vi = 0usize;
        let mut ci = 0usize;
        let mut i = 0usize;
        while i < self.len {
            let s = match self.rows.get(i) {
                Some(v) => *v,
                None => return Err(CPF_EBAL),
            };
            if s.slot as usize != i {
                return Err(CPF_ESLOT);
            }
            if s.ok {
                if s.code != 0 {
                    return Err(CPF_EVALUE);
                }
                match VALS.get(vi) {
                    Some(w) => {
                        if s.value != *w {
                            return Err(CPF_EVALUE);
                        }
                    }
                    None => return Err(CPF_EBAL),
                }
                vi += 1;
            } else {
                match CODES.get(ci) {
                    Some(w) => {
                        if s.code != *w {
                            return Err(CPF_EVALUE);
                        }
                    }
                    None => return Err(CPF_EBAL),
                }
                ci += 1;
            }
            i += 1;
        }
        if vi != 8 || ci != 6 {
            return Err(CPF_EBAL);
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
            return Err(CPF_EBAL);
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
            Err(CPF_ESEAL)
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

/// 十四格账本：8 允 + 6 拒。
pub fn build_ledger() -> Ledger {
    let mut l = Ledger::new();

    let mut c = match Cp::open(0o022, 1000) {
        Ok(v) => v,
        Err(_) => return l,
    };
    let _ = l.register(cell(0, OP_SRC, c.src(0o644, 5, 900, KIND_REG)));
    let _ = l.register(cell(1, OP_DST, c.dst(false, 0, false)));
    let _ = l.register(word_cell(2, OP_FINISH, c.finish()));

    let mut c2 = match Cp::open(0o022, 1000) {
        Ok(v) => v,
        Err(_) => return l,
    };
    let _ = l.register(cell(3, OP_SRC, c2.src(0o777, 9, 950, KIND_REG)));
    let _ = l.register(cell(4, OP_DST, c2.dst(true, 0o600, false)));
    let _ = l.register(word_cell(5, OP_FINISH, c2.finish()));

    let mut c3 = match Cp::open(0o027, 1200) {
        Ok(v) => v,
        Err(_) => return l,
    };
    let _ = c3.src(0o666, 3, 800, KIND_REG);
    let _ = c3.dst(false, 0, false);
    let _ = l.register(word_cell(6, OP_FINISH, c3.finish()));

    let mut c4 = match Cp::open(0o002, 1300) {
        Ok(v) => v,
        Err(_) => return l,
    };
    let _ = c4.src(0o664, 4, 700, KIND_REG);
    let _ = c4.dst(false, 0, false);
    let _ = l.register(word_cell(7, OP_FINISH, c4.finish()));

    let _ = l.register(word_cell(8, OP_OPEN, Cp::open(0o10000, 1000).map(|v| v.umask())));
    let _ = l.register(word_cell(9, OP_OPEN, Cp::open(0o022, 0).map(|v| v.now() as u32)));

    let mut c5 = match Cp::open(0o022, 1000) {
        Ok(v) => v,
        Err(_) => return l,
    };
    let _ = l.register(cell(10, OP_SRC, c5.src(0o644, 5, 900, KIND_DIR)));
    let _ = l.register(cell(11, OP_SRC, c5.src(0o644, SIZE_LIMIT + 1, 900, KIND_REG)));
    let _ = c5.src(0o644, 5, 900, KIND_REG);
    let _ = l.register(cell(12, OP_DST, c5.dst(true, 0o400, false)));

    let mut c6 = match Cp::open(0o022, 1000) {
        Ok(v) => v,
        Err(_) => return l,
    };
    let _ = l.register(word_cell(13, OP_FINISH, c6.finish()));

    l
}

/// 四路注入。
pub fn inject_four() -> [bool; 4] {
    let mut out = [false; 4];

    let l = build_ledger();
    let rep = l.replay();
    let root = l.seal();
    out[0] = l.len() == SAMPLES && l.audit().is_ok() && rep == Ok(root) && root != 0;

    // 一路：内容、权限、时间三条派生。
    let mut c = match Cp::open(0o022, 2000) {
        Ok(v) => v,
        Err(_) => return out,
    };
    let r1 = c.src(0o755, 12, 1500, KIND_REG);
    let r2 = c.dst(false, 0, false);
    let f = c.finish();
    out[1] = r1 == Ok(1)
        && r2 == Ok(1)
        && f == Ok(0o755)
        && c.size() == 12
        && c.mtime() == 1500
        && c.now() == 2000
        && c.derived() == 0o755
        && c.is_done()
        && c.consistent();

    // 二路：八类失败各自可辨，失败不改状态。
    let mut b = match Cp::open(0o022, 2000) {
        Ok(v) => v,
        Err(_) => return out,
    };
    let kind = b.src(0o644, 1, 1, KIND_LNK) == Err(CPF_EKIND);
    let size = b.src(0o644, SIZE_LIMIT + 1, 1, KIND_REG) == Err(CPF_ESIZE);
    let empty = b.has_src() == false;
    let same = b.dst(false, 0, true) == Err(CPF_ESAME);
    let no_src = b.finish() == Err(CPF_ENOSRC);
    let first = b.src(0o644, 1, 1, KIND_REG) == Ok(1);
    let no_dst = b.finish() == Err(CPF_ENODST);
    let dir = b.dst(true, 0o040755, false) == Err(CPF_EKIND);
    let perm = b.dst(true, 0o400, false) == Err(CPF_EPERM);
    let untouched = b.has_dst() == false;
    let umask = Cp::open(0o10000, 1).is_err();
    let clock = Cp::open(0, 0).is_err();
    out[2] = kind
        && size
        && empty
        && same
        && no_src
        && first
        && no_dst
        && dir
        && perm
        && untouched
        && umask
        && clock
        && b.consistent();

    // 三路：尺寸上限与 umask 极限。
    let mut q = match Cp::open(0o777, 2000) {
        Ok(v) => v,
        Err(_) => return out,
    };
    let q1 = q.src(0o777, SIZE_LIMIT, 1, KIND_REG) == Ok(1);
    let q2 = q.src(0o777, SIZE_LIMIT + 1, 1, KIND_REG) == Err(CPF_ESIZE);
    let q3 = q.dst(false, 0, false) == Ok(1);
    let q4 = q.finish() == Ok(0);
    let mut e = match Cp::open(0o000, 2000) {
        Ok(v) => v,
        Err(_) => return out,
    };
    let _ = e.src(0o640, 1, 1, KIND_REG);
    let _ = e.dst(false, 0, false);
    let q5 = e.finish() == Ok(0o640);
    out[3] = q1 && q2 && q3 && q4 && q5 && q.consistent() && e.consistent() && CAP == 1;

    out
}

/// 十三项判据。
pub fn run_unx11387_checks() -> CheckSet {
    let mut s = CheckSet::new("unx11387.cp_file_copy");
    let l = build_ledger();

    s.add_dyn(
        "CPF1-十四格八允六拒",
        l.len() == SAMPLES && l.ok_count() == 8 && l.err_count() == 6 && l.audit().is_ok(),
        &format!("n={} ok={} err={}", l.len(), l.ok_count(), l.err_count()),
    );

    s.add(
        "CPF2-内容尺寸与时间派生",
        {
            let mut c = match Cp::open(0o022, 2000) {
                Ok(v) => v,
                Err(_) => return s,
            };
            let _ = c.src(0o644, 12, 1500, KIND_REG);
            let _ = c.dst(false, 0, false);
            c.finish() == Ok(0o644)
                && c.size() == 12
                && c.mtime() == 1500
                && c.is_done()
                && c.consistent()
        },
        "static",
    );

    s.add(
        "CPF3-权限按 umask 派生",
        {
            let mut a = match Cp::open(0o022, 2000) {
                Ok(v) => v,
                Err(_) => return s,
            };
            let _ = a.src(0o777, 1, 1, KIND_REG);
            let _ = a.dst(false, 0, false);
            let mut b = match Cp::open(0o077, 2000) {
                Ok(v) => v,
                Err(_) => return s,
            };
            let _ = b.src(0o777, 1, 1, KIND_REG);
            let _ = b.dst(false, 0, false);
            a.finish() == Ok(0o755) && b.finish() == Ok(0o700)
        },
        "static",
    );

    s.add(
        "CPF4-目标已存在保留权限",
        {
            let mut c = match Cp::open(0o022, 2000) {
                Ok(v) => v,
                Err(_) => return s,
            };
            let _ = c.src(0o777, 9, 950, KIND_REG);
            let _ = c.dst(true, 0o600, false);
            c.finish() == Ok(0o600)
                && c.derived() == 0o600
                && l.sample(5).map(|x| x.value) == Some(384)
        },
        "static",
    );

    s.add(
        "CPF5-umask 越界被拒",
        {
            Cp::open(0o10000, 1).is_err()
                && Cp::open(0o20000, 1).is_err()
                && Cp::open(0o777, 1).is_ok()
                && l.sample(8).map(|x| x.code) == Some(CPF_EUMASK)
        },
        "static",
    );

    s.add(
        "CPF6-时钟为零被拒",
        {
            Cp::open(0o022, 0).is_err()
                && Cp::open(0o022, 1).is_ok()
                && l.sample(9).map(|x| x.code) == Some(CPF_ECLOCK)
        },
        "static",
    );

    s.add(
        "CPF7-非普通文件被拒",
        {
            let mut c = match Cp::open(0o022, 2000) {
                Ok(v) => v,
                Err(_) => return s,
            };
            c.src(0o644, 1, 1, KIND_DIR) == Err(CPF_EKIND)
                && c.src(0o644, 1, 1, KIND_FIFO) == Err(CPF_EKIND)
                && !c.has_src()
                && l.sample(10).map(|x| x.code) == Some(CPF_EKIND)
        },
        "static",
    );

    s.add(
        "CPF8-尺寸超限被拒",
        {
            let mut c = match Cp::open(0o022, 2000) {
                Ok(v) => v,
                Err(_) => return s,
            };
            c.src(0o644, SIZE_LIMIT + 1, 1, KIND_REG) == Err(CPF_ESIZE)
                && c.src(0o644, SIZE_LIMIT, 1, KIND_REG) == Ok(1)
                && c.consistent()
                && l.sample(11).map(|x| x.code) == Some(CPF_ESIZE)
        },
        "static",
    );

    s.add(
        "CPF9-目标只读被拒",
        {
            let mut c = match Cp::open(0o022, 2000) {
                Ok(v) => v,
                Err(_) => return s,
            };
            c.dst(true, 0o400, false) == Err(CPF_EPERM)
                && c.dst(true, 0o040755, false) == Err(CPF_EKIND)
                && !c.has_dst()
                && l.sample(12).map(|x| x.code) == Some(CPF_EPERM)
        },
        "static",
    );

    s.add(
        "CPF10-同源同目标被拒",
        {
            let mut c = match Cp::open(0o022, 2000) {
                Ok(v) => v,
                Err(_) => return s,
            };
            let _ = c.src(0o644, 3, 1, KIND_REG);
            c.dst(true, 0o600, true) == Err(CPF_ESAME)
                && !c.has_dst()
                && c.consistent()
        },
        "static",
    );

    s.add(
        "CPF11-缺源不可收",
        {
            let mut c = match Cp::open(0o022, 2000) {
                Ok(v) => v,
                Err(_) => return s,
            };
            c.finish() == Err(CPF_ENOSRC)
                && !c.is_done()
                && l.sample(13).map(|x| x.code) == Some(CPF_ENOSRC)
        },
        "static",
    );

    s.add(
        "CPF12-缺目标不可收",
        {
            let mut c = match Cp::open(0o022, 2000) {
                Ok(v) => v,
                Err(_) => return s,
            };
            let _ = c.src(0o644, 3, 1, KIND_REG);
            c.finish() == Err(CPF_ENODST)
                && !c.is_done()
                && c.consistent()
        },
        "static"
    );

    s.add(
        "CPF13-冻结重放四注入",
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

    fn cp(umask: u32) -> Cp {
        match Cp::open(umask, 2000) {
            Ok(v) => v,
            Err(_) => panic!("open"),
        }
    }

    #[test]
    fn ledger_shape() {
        let l = build_ledger();
        assert_eq!(l.len(), SAMPLES);
        assert_eq!(l.ok_count(), 8);
        assert_eq!(l.err_count(), 6);
        assert_eq!(l.rejects(), 0);
        assert!(l.audit().is_ok());
        let mut i = 0usize;
        while i < l.len() {
            assert_eq!(l.sample(i).map(|x| x.slot as usize), Some(i));
            i += 1;
        }
    }

    #[test]
    fn checks_pass() {
        let s = run_unx11387_checks();
        assert_eq!(s.len(), 13);
        assert!(s.all_passed());
    }

    #[test]
    fn derive_new_mode() {
        let mut c = cp(0o022);
        assert_eq!(c.src(0o777, 4, 100, KIND_REG), Ok(1));
        assert_eq!(c.dst(false, 0, false), Ok(1));
        assert_eq!(c.finish(), Ok(0o755));
        assert_eq!(c.size(), 4);
        assert_eq!(c.mtime(), 100);
        assert!(c.is_done());
        assert!(c.consistent());
    }

    #[test]
    fn keep_existing_mode() {
        let mut c = cp(0o022);
        assert_eq!(c.src(0o700, 4, 100, KIND_REG), Ok(1));
        assert_eq!(c.dst(true, 0o640, false), Ok(1));
        assert_eq!(c.finish(), Ok(0o640));
    }

    #[test]
    fn source_gates() {
        let mut c = cp(0o022);
        assert_eq!(c.src(0o644, 1, 1, KIND_DIR), Err(CPF_EKIND));
        assert_eq!(c.src(0o644, SIZE_LIMIT + 1, 1, KIND_REG), Err(CPF_ESIZE));
        assert!(!c.has_src());
        assert_eq!(c.src(0o644, SIZE_LIMIT, 1, KIND_REG), Ok(1));
    }

    #[test]
    fn target_gates() {
        let mut c = cp(0o022);
        assert_eq!(c.dst(false, 0, true), Err(CPF_ESAME));
        assert_eq!(c.dst(true, 0o040755, false), Err(CPF_EKIND));
        assert_eq!(c.dst(true, 0o400, false), Err(CPF_EPERM));
        assert!(!c.has_dst());
        assert_eq!(c.dst(true, 0o600, false), Ok(1));
    }

    #[test]
    fn finish_order() {
        let mut c = cp(0o022);
        assert_eq!(c.finish(), Err(CPF_ENOSRC));
        assert_eq!(c.src(0o644, 1, 1, KIND_REG), Ok(1));
        assert_eq!(c.finish(), Err(CPF_ENODST));
        assert_eq!(c.dst(false, 0, false), Ok(1));
        assert_eq!(c.finish(), Ok(0o644));
    }

    #[test]
    fn inject_four_holds() {
        assert_eq!(inject_four(), [true; 4]);
    }
}
