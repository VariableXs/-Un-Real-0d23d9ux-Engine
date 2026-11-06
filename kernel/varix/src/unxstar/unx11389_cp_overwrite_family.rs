//! UNX-F11389 cp -i/-n/-f 覆盖策略族（0xee50 段）
//!
//! `cp` 的覆盖行为由 `-i`、`-n`、`-f` 三个开关决定。三者是同一族里的互斥策略，
//! 命令行的规则是**后出现的生效**（`alias cp='cp -i'` 之后再写 `-f`，最终是
//! 强制覆盖）。策略不是布尔量，而是一条有历史的序列：策略栈保留了用户到底按
//! 什么顺序给出这些开关，回滚时必须能退回上一条，而不是回到「无策略」。
//!
//! 决议只依赖两件事：当前生效策略 + 目的地状态。
//! - 目的地不存在：直接创建，任何策略都一样（没有东西可覆盖）；
//! - 目的地是同一个文件：`cp` 一律拒绝（`ESAME`）；
//! - 目的地是目录：未递归时拒绝（`EISDIR`），这条与策略无关；
//! - 目的地是普通文件：`-n` 跳过、`-i` 询问、`-f` 直接覆盖。
//!
//! 同一批里同时给出两个**不同**的策略（例如配置里同时写了 `-f` 和 `-n`）不能
//! 靠「谁先谁后」蒙混，必须报 `ECONFLICT`：没有顺序的地方就没有「后者生效」。
//!
//! 闸门次序：
//! - `push`：策略非法 → 历史表满；
//! - `request`：策略非法 → 两个策略冲突；
//! - `decide`：无策略 → 目的地非法 → 同文件 → 目录 → 按策略决议。

use crate::alloc::format;
use crate::checks::CheckSet;

/// 段基址。
pub const SEG_BASE: u32 = 0xee50;

/// 策略或目的地非法。
pub const CP_EINVAL: u32 = SEG_BASE + 0x01;
/// 策略历史表满。
pub const CP_EFULL: u32 = SEG_BASE + 0x02;
/// 尚未给出任何策略。
pub const CP_ENOPOLICY: u32 = SEG_BASE + 0x03;
/// 源与目的地是同一个文件。
pub const CP_ESAME: u32 = SEG_BASE + 0x04;
/// 目的地是目录。
pub const CP_EISDIR: u32 = SEG_BASE + 0x05;
/// 同一批给出两个不同策略。
pub const CP_ECONFLICT: u32 = SEG_BASE + 0x06;
/// 没有可回滚的策略。
pub const CP_EEMPTY: u32 = SEG_BASE + 0x07;
/// 历史下标越界。
pub const CP_ERANGE: u32 = SEG_BASE + 0x08;
/// 账本形状不符。
pub const CP_EBAL: u32 = SEG_BASE + 0x09;
/// 槽位乱序。
pub const CP_ESLOT: u32 = SEG_BASE + 0x0a;
/// 观测值与复算不符。
pub const CP_EVALUE: u32 = SEG_BASE + 0x0b;
/// 冻结后重放不一致。
pub const CP_ESEAL: u32 = SEG_BASE + 0x0c;
/// 观察下标越界。
pub const CP_ENOBS: u32 = SEG_BASE + 0x0d;

/// 策略历史容量。
pub const CAP: usize = 6;
/// 账本格数。
pub const SAMPLES: usize = 14;

/// 策略：交互确认。
pub const P_INTERACTIVE: u8 = 1;
/// 策略：不覆盖已存在文件。
pub const P_NOCLOBBER: u8 = 2;
/// 策略：强制覆盖。
pub const P_FORCE: u8 = 3;

/// 目的地：不存在。
pub const D_NONE: u8 = 1;
/// 目的地：普通文件。
pub const D_FILE: u8 = 2;
/// 目的地：目录。
pub const D_DIR: u8 = 3;
/// 目的地：与源同一个文件。
pub const D_SAME: u8 = 4;

/// 决议：创建。
pub const A_CREATE: u8 = 1;
/// 决议：覆盖。
pub const A_REPLACE: u8 = 2;
/// 决议：跳过。
pub const A_SKIP: u8 = 3;
/// 决议：询问。
pub const A_ASK: u8 = 4;

/// 操作码：压入策略。
pub const OP_PUSH: u8 = 1;
/// 操作码：同批请求。
pub const OP_REQUEST: u8 = 2;
/// 操作码：决议。
pub const OP_DECIDE: u8 = 3;
/// 操作码：回滚。
pub const OP_ROLLBACK: u8 = 4;
/// 操作码：观察。
pub const OP_OBS: u8 = 5;
/// 操作码：读取生效策略。
pub const OP_EFFECTIVE: u8 = 6;

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

/// 策略是否合法。
pub fn valid_mode(m: u8) -> bool {
    m == P_INTERACTIVE || m == P_NOCLOBBER || m == P_FORCE
}

/// 目的地是否合法。
pub fn valid_dest(d: u8) -> bool {
    d == D_NONE || d == D_FILE || d == D_DIR || d == D_SAME
}

/// 覆盖策略族。
#[derive(Clone, Copy, Debug)]
pub struct Policy {
    hist: [u8; CAP],
    n: usize,
    mode: u8,
    last_wins: u32,
}

impl Policy {
    /// 空策略族（尚未给出任何开关）。
    pub const fn new() -> Policy {
        Policy { hist: [0; CAP], n: 0, mode: 0, last_wins: 0 }
    }

    /// 压入一个策略；后压入的生效。
    pub fn push(&mut self, mode: u8) -> Result<usize, u32> {
        if !valid_mode(mode) {
            return Err(CP_EINVAL);
        }
        if self.n >= CAP {
            return Err(CP_EFULL);
        }
        if let Some(slot) = self.hist.get_mut(self.n) {
            *slot = mode;
        }
        self.n += 1;
        self.mode = mode;
        self.last_wins = self.last_wins.saturating_add(1);
        Ok(self.n)
    }

    /// 同一批给出两个策略；相等才算一致。
    pub fn request(&mut self, a: u8, b: u8) -> Result<usize, u32> {
        if !valid_mode(a) || !valid_mode(b) {
            return Err(CP_EINVAL);
        }
        if a != b {
            return Err(CP_ECONFLICT);
        }
        self.push(a)
    }

    /// 当前生效策略。
    pub fn effective(&self) -> Result<u8, u32> {
        if self.n == 0 {
            return Err(CP_ENOPOLICY);
        }
        Ok(self.mode)
    }

    /// 按目的地状态决议。
    pub fn decide(&self, dest: u8) -> Result<u8, u32> {
        if self.n == 0 {
            return Err(CP_ENOPOLICY);
        }
        if !valid_dest(dest) {
            return Err(CP_EINVAL);
        }
        if dest == D_SAME {
            return Err(CP_ESAME);
        }
        if dest == D_DIR {
            return Err(CP_EISDIR);
        }
        if dest == D_NONE {
            return Ok(A_CREATE);
        }
        match self.mode {
            P_NOCLOBBER => Ok(A_SKIP),
            P_INTERACTIVE => Ok(A_ASK),
            _ => Ok(A_REPLACE),
        }
    }

    /// 回滚最后一个策略。
    pub fn rollback(&mut self) -> Result<usize, u32> {
        if self.n == 0 {
            return Err(CP_EEMPTY);
        }
        self.n -= 1;
        self.mode = if self.n == 0 {
            0
        } else {
            match self.hist.get(self.n - 1) {
                Some(v) => *v,
                None => 0,
            }
        };
        Ok(self.n)
    }

    /// 历史里第 i 个策略。
    pub fn mode_at(&self, i: usize) -> Result<u8, u32> {
        if i >= self.n {
            return Err(CP_ERANGE);
        }
        match self.hist.get(i) {
            Some(v) => Ok(*v),
            None => Err(CP_ERANGE),
        }
    }

    /// 历史长度。
    pub fn count(&self) -> usize {
        self.n
    }

    /// 压入次数。
    pub fn last_wins(&self) -> u32 {
        self.last_wins
    }

    /// 不变式自检。
    pub fn consistent(&self) -> bool {
        if self.n > CAP {
            return false;
        }
        let mut i = 0usize;
        while i < self.n {
            match self.hist.get(i) {
                Some(v) => {
                    if !valid_mode(*v) {
                        return false;
                    }
                }
                None => return false,
            }
            i += 1;
        }
        if self.n == 0 {
            return self.mode == 0;
        }
        match self.hist.get(self.n - 1) {
            Some(v) => self.mode == *v,
            None => false,
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
            return Err(CP_ESLOT);
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
            return Err(CP_EBAL);
        }
        const VALS: [u32; 8] = [1, 2, 3, 2, 3, 3, 1, 2];
        const CODES: [u32; 6] =
            [CP_ESAME, CP_EINVAL, CP_ECONFLICT, CP_ERANGE, CP_ENOPOLICY, CP_EFULL];
        let mut vi = 0usize;
        let mut ci = 0usize;
        let mut i = 0usize;
        while i < self.len {
            let s = match self.rows.get(i) {
                Some(v) => *v,
                None => return Err(CP_EBAL),
            };
            if s.slot as usize != i {
                return Err(CP_ESLOT);
            }
            if s.ok {
                if s.code != 0 {
                    return Err(CP_EVALUE);
                }
                match VALS.get(vi) {
                    Some(w) => {
                        if s.value != *w {
                            return Err(CP_EVALUE);
                        }
                    }
                    None => return Err(CP_EBAL),
                }
                vi += 1;
            } else {
                match CODES.get(ci) {
                    Some(w) => {
                        if s.code != *w {
                            return Err(CP_EVALUE);
                        }
                    }
                    None => return Err(CP_EBAL),
                }
                ci += 1;
            }
            i += 1;
        }
        if vi != 8 || ci != 6 {
            return Err(CP_EBAL);
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
            return Err(CP_EBAL);
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
            Err(CP_ESEAL)
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

/// 十四格账本：两次压入 → 决议 → 末位生效 → 各类拒绝。
pub fn build_ledger() -> Ledger {
    let mut l = Ledger::new();
    let mut p = Policy::new();

    let _ = l.register(cell(0, OP_PUSH, p.push(P_INTERACTIVE)));
    let _ = l.register(cell(1, OP_PUSH, p.push(P_FORCE)));
    let _ = l.register(byte_cell(2, OP_EFFECTIVE, p.effective()));
    let _ = l.register(byte_cell(3, OP_DECIDE, p.decide(D_FILE)));
    let _ = l.register(cell(4, OP_PUSH, p.push(P_NOCLOBBER)));
    let _ = l.register(byte_cell(5, OP_DECIDE, p.decide(D_FILE)));
    let _ = l.register(byte_cell(6, OP_DECIDE, p.decide(D_NONE)));
    let _ = l.register(cell(7, OP_ROLLBACK, p.rollback()));

    let _ = l.register(byte_cell(8, OP_DECIDE, p.decide(D_SAME)));
    let _ = l.register(cell(9, OP_PUSH, p.push(9)));
    let _ = l.register(cell(10, OP_REQUEST, p.request(P_FORCE, P_NOCLOBBER)));

    let mut q = Policy::new();
    let _ = q.push(P_FORCE);
    let _ = l.register(byte_cell(11, OP_OBS, q.mode_at(99)));

    let empty = Policy::new();
    let _ = l.register(byte_cell(12, OP_EFFECTIVE, empty.effective()));

    let mut full = Policy::new();
    let mut i = 0u32;
    while i < CAP as u32 {
        let _ = full.push(P_FORCE);
        i += 1;
    }
    let _ = l.register(cell(13, OP_PUSH, full.push(P_INTERACTIVE)));

    l
}

/// 四路注入。
pub fn inject_four() -> [bool; 4] {
    let mut out = [false; 4];

    let l = build_ledger();
    let rep = l.replay();
    let root = l.seal();
    out[0] = l.len() == SAMPLES && l.audit().is_ok() && rep == Ok(root) && root != 0;

    // 一路：末位生效 → 决议 → 回滚回到上一条。
    let mut p = Policy::new();
    let a1 = p.push(P_INTERACTIVE);
    let a2 = p.push(P_NOCLOBBER);
    let e1 = p.effective();
    let d1 = p.decide(D_FILE);
    let a3 = p.request(P_FORCE, P_FORCE);
    let e2 = p.effective();
    let d2 = p.decide(D_FILE);
    let d3 = p.decide(D_NONE);
    let r1 = p.rollback();
    let e3 = p.effective();
    out[1] = a1 == Ok(1)
        && a2 == Ok(2)
        && e1 == Ok(P_NOCLOBBER)
        && d1 == Ok(A_SKIP)
        && a3 == Ok(3)
        && e2 == Ok(P_FORCE)
        && d2 == Ok(A_REPLACE)
        && d3 == Ok(A_CREATE)
        && r1 == Ok(2)
        && e3 == Ok(P_NOCLOBBER)
        && p.mode_at(0) == Ok(P_INTERACTIVE)
        && p.mode_at(1) == Ok(P_NOCLOBBER)
        && p.count() == 2
        && p.last_wins() == 3
        && p.consistent();

    // 二路：八类失败各自可辨。
    let mut b = Policy::new();
    let early = b.effective() == Err(CP_ENOPOLICY);
    let undecided = b.decide(D_FILE) == Err(CP_ENOPOLICY);
    let bad_mode = b.push(0) == Err(CP_EINVAL) && b.push(9) == Err(CP_EINVAL);
    let conflict = b.request(P_INTERACTIVE, P_FORCE) == Err(CP_ECONFLICT);
    let bad_req = b.request(P_NOCLOBBER, 7) == Err(CP_EINVAL);
    let empty = b.rollback() == Err(CP_EEMPTY);
    let pushed = b.push(P_NOCLOBBER) == Ok(1);
    let bad_dest = b.decide(0) == Err(CP_EINVAL) && b.decide(9) == Err(CP_EINVAL);
    let same = b.decide(D_SAME) == Err(CP_ESAME);
    let dir = b.decide(D_DIR) == Err(CP_EISDIR);
    let range = b.mode_at(1) == Err(CP_ERANGE);
    let still = b.count() == 1 && b.effective() == Ok(P_NOCLOBBER);
    let back = b.rollback() == Ok(0) && b.rollback() == Err(CP_EEMPTY);
    out[2] = early
        && undecided
        && bad_mode
        && conflict
        && bad_req
        && empty
        && pushed
        && bad_dest
        && same
        && dir
        && range
        && still
        && back
        && b.consistent();

    // 三路：历史表满仍可回滚让位。
    let mut q = Policy::new();
    let mut i = 0u32;
    while i < CAP as u32 {
        let _ = q.push(P_FORCE);
        i += 1;
    }
    let q1 = q.count() == CAP;
    let q2 = q.push(P_INTERACTIVE) == Err(CP_EFULL);
    let q3 = q.rollback() == Ok(CAP - 1);
    let q4 = q.push(P_INTERACTIVE) == Ok(CAP);
    let q5 = q.effective() == Ok(P_INTERACTIVE);
    let q6 = q.mode_at(CAP - 1) == Ok(P_INTERACTIVE);
    out[3] = q1 && q2 && q3 && q4 && q5 && q6 && q.consistent();

    out
}

/// 十三项判据。
pub fn run_unx11389_checks() -> CheckSet {
    let mut s = CheckSet::new("unx11389.cp_overwrite_family");
    let l = build_ledger();

    s.add_dyn(
        "CP1-十四格八允六拒",
        l.len() == SAMPLES && l.ok_count() == 8 && l.err_count() == 6 && l.audit().is_ok(),
        &format!("n={} ok={} err={}", l.len(), l.ok_count(), l.err_count()),
    );

    s.add(
        "CP2-后压入者生效",
        l.sample(2).map(|x| x.value) == Some(P_FORCE as u32)
            && l.sample(3).map(|x| x.value) == Some(A_REPLACE as u32),
        "static",
    );

    s.add(
        "CP3-末位改判跳过",
        l.sample(5).map(|x| x.value) == Some(A_SKIP as u32),
        "static",
    );

    s.add(
        "CP4-目的地不存在必创建",
        {
            let mut p = Policy::new();
            let _ = p.push(P_NOCLOBBER);
            p.decide(D_NONE) == Ok(A_CREATE)
                && l.sample(6).map(|x| x.value) == Some(A_CREATE as u32)
        },
        "static",
    );

    s.add(
        "CP5-回滚回到上一条",
        {
            let mut p = Policy::new();
            let _ = p.push(P_INTERACTIVE);
            let _ = p.push(P_FORCE);
            let n = p.rollback();
            n == Ok(1)
                && p.effective() == Ok(P_INTERACTIVE)
                && p.decide(D_FILE) == Ok(A_ASK)
                && l.sample(7).map(|x| x.value) == Some(2)
        },
        "static",
    );

    s.add(
        "CP6-同文件被拒",
        l.sample(8).map(|x| x.code) == Some(CP_ESAME),
        "static",
    );

    s.add(
        "CP7-策略非法被拒",
        l.sample(9).map(|x| x.code) == Some(CP_EINVAL),
        "static",
    );

    s.add(
        "CP8-同批冲突被拒",
        {
            let mut p = Policy::new();
            let r = p.request(P_FORCE, P_NOCLOBBER);
            r == Err(CP_ECONFLICT)
                && p.count() == 0
                && p.effective() == Err(CP_ENOPOLICY)
                && p.request(P_FORCE, P_FORCE) == Ok(1)
                && l.sample(10).map(|x| x.code) == Some(CP_ECONFLICT)
        },
        "static",
    );

    s.add(
        "CP9-历史下标越界",
        {
            let mut p = Policy::new();
            let _ = p.push(P_FORCE);
            p.mode_at(1) == Err(CP_ERANGE)
                && p.mode_at(0) == Ok(P_FORCE)
                && l.sample(11).map(|x| x.code) == Some(CP_ERANGE)
        },
        "static",
    );

    s.add(
        "CP10-无策略先拒",
        {
            let mut p = Policy::new();
            p.effective() == Err(CP_ENOPOLICY)
                && p.decide(D_NONE) == Err(CP_ENOPOLICY)
                && p.rollback() == Err(CP_EEMPTY)
                && l.sample(12).map(|x| x.code) == Some(CP_ENOPOLICY)
        },
        "static",
    );

    s.add(
        "CP11-历史表满被拒",
        l.sample(13).map(|x| x.code) == Some(CP_EFULL),
        "static",
    );

    s.add(
        "CP12-目录与询问决议",
        {
            let mut p = Policy::new();
            let _ = p.push(P_INTERACTIVE);
            let ask = p.decide(D_FILE) == Ok(A_ASK);
            let dir = p.decide(D_DIR) == Err(CP_EISDIR);
            let same = p.decide(D_SAME) == Err(CP_ESAME);
            let _ = p.push(P_FORCE);
            let force = p.decide(D_FILE) == Ok(A_REPLACE);
            let skip = {
                let _ = p.push(P_NOCLOBBER);
                p.decide(D_FILE) == Ok(A_SKIP)
            };
            ask && dir && same && force && skip && p.consistent()
        },
        "static",
    );

    s.add(
        "CP13-冻结重放四注入",
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
        assert_eq!(l.rejects(), 0);
        assert!(l.audit().is_ok());
    }

    #[test]
    fn checks_pass() {
        let s = run_unx11389_checks();
        assert_eq!(s.len(), 13);
        assert!(s.all_passed());
    }

    #[test]
    fn last_flag_wins() {
        let mut p = Policy::new();
        assert_eq!(p.push(P_INTERACTIVE), Ok(1));
        assert_eq!(p.push(P_NOCLOBBER), Ok(2));
        assert_eq!(p.push(P_FORCE), Ok(3));
        assert_eq!(p.effective(), Ok(P_FORCE));
        assert_eq!(p.decide(D_FILE), Ok(A_REPLACE));
        assert_eq!(p.mode_at(0), Ok(P_INTERACTIVE));
        assert!(p.consistent());
    }

    #[test]
    fn rollback_restores_previous() {
        let mut p = Policy::new();
        assert_eq!(p.push(P_INTERACTIVE), Ok(1));
        assert_eq!(p.push(P_NOCLOBBER), Ok(2));
        assert_eq!(p.rollback(), Ok(1));
        assert_eq!(p.effective(), Ok(P_INTERACTIVE));
        assert_eq!(p.decide(D_FILE), Ok(A_ASK));
        assert_eq!(p.rollback(), Ok(0));
        assert_eq!(p.effective(), Err(CP_ENOPOLICY));
        assert_eq!(p.rollback(), Err(CP_EEMPTY));
        assert!(p.consistent());
    }

    #[test]
    fn conflicts_need_order() {
        let mut p = Policy::new();
        assert_eq!(p.request(P_FORCE, P_NOCLOBBER), Err(CP_ECONFLICT));
        assert_eq!(p.count(), 0);
        assert_eq!(p.request(P_FORCE, P_FORCE), Ok(1));
        assert_eq!(p.effective(), Ok(P_FORCE));
    }

    #[test]
    fn destinations_gate_first() {
        let mut p = Policy::new();
        assert_eq!(p.decide(D_NONE), Err(CP_ENOPOLICY));
        assert_eq!(p.push(P_FORCE), Ok(1));
        assert_eq!(p.decide(D_SAME), Err(CP_ESAME));
        assert_eq!(p.decide(D_DIR), Err(CP_EISDIR));
        assert_eq!(p.decide(5), Err(CP_EINVAL));
        assert_eq!(p.decide(D_NONE), Ok(A_CREATE));
        assert_eq!(p.decide(D_FILE), Ok(A_REPLACE));
    }

    #[test]
    fn history_capacity() {
        let mut p = Policy::new();
        let mut i = 0usize;
        while i < CAP {
            assert_eq!(p.push(P_FORCE), Ok(i + 1));
            i += 1;
        }
        assert_eq!(p.push(P_FORCE), Err(CP_EFULL));
        assert_eq!(p.rollback(), Ok(CAP - 1));
        assert_eq!(p.push(P_NOCLOBBER), Ok(CAP));
        assert_eq!(p.effective(), Ok(P_NOCLOBBER));
        assert!(p.consistent());
    }

    #[test]
    fn inject_four_holds() {
        assert_eq!(inject_four(), [true; 4]);
    }
}
