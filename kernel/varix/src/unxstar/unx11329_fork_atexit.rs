//! UNX-F11329 `fork` 后 atexit 面语义（0xea90 段）
//!
//! `atexit(3)` 登记表是**进程级**的，而 `fork(2)` 复制的是**调用瞬间**的那份表。
//! 这条时间线决定了三件容易写错的事：
//!
//! 1. 分叉那一刻父表全部条目被复制进子表，一条不多、一条不少；
//! 2. 分叉之后父侧再登记只进父表，子侧再登记只进子表，两张表从此各自生长；
//! 3. 子进程 `exit` 只展开子表，父进程的登记项在父地址空间里一次都不跑——账上
//!    父侧运行标记必须仍是「未运行」。
//!
//! 本模块把这两张表放在同一个对象里各自成账，用运行序表（`*_seq`）把 LIFO 展开
//! 逐条记下：任何一次「以为在操作父表其实是子表」的串线，都会让两张表的计数与
//! 序号表对不上。继承前缀另由 `verify_inherit` 单独核一遍——它只认分叉瞬间的那段
//! 公共前缀，分叉后再往父表追加的条目不算继承，这正是 Linux 的实况。
//!
//! 闸门次序：
//! - `register`：零号 → 父已运行 → 父表满 → 父表重复；
//! - `fork`：已分叉 → 父已运行；
//! - `register_child`：未分叉 → 零号 → 子已运行 → 子表满 → 子表重复；
//! - `child_exit`：未分叉 → 子已运行；
//! - `parent_exit`：父已运行。

use crate::alloc::format;
use crate::checks::CheckSet;

/// 段基址。
pub const SEG_BASE: u32 = 0xea90;

/// 功能号为零。
pub const FK_EINVAL: u32 = SEG_BASE + 0x01;
/// 该侧已经运行过。
pub const FK_ERUN: u32 = SEG_BASE + 0x02;
/// 该侧表满。
pub const FK_EFULL: u32 = SEG_BASE + 0x03;
/// 该侧同功能号重复。
pub const FK_EDUP: u32 = SEG_BASE + 0x04;
/// 尚未分叉就操作子侧。
pub const FK_EFORK: u32 = SEG_BASE + 0x05;
/// 重复分叉。
pub const FK_EREENTER: u32 = SEG_BASE + 0x06;
/// 运行序表下标越界。
pub const FK_ENOBS: u32 = SEG_BASE + 0x07;
/// 账本形状不符。
pub const FK_EBAL: u32 = SEG_BASE + 0x08;
/// 槽位乱序。
pub const FK_EORDER: u32 = SEG_BASE + 0x09;
/// 观测值与复算不符。
pub const FK_EMISMATCH: u32 = SEG_BASE + 0x0a;
/// 冻结后重放不一致。
pub const FK_ESEAL: u32 = SEG_BASE + 0x0b;
/// 继承前缀两侧不一致。
pub const FK_ESIDE: u32 = SEG_BASE + 0x0c;
/// 两张表互相污染。
pub const FK_EPAIR: u32 = SEG_BASE + 0x0d;

/// 单侧容量。
pub const CAP: usize = 16;
/// 账本格数。
pub const SAMPLES: usize = 14;
/// 操作码：父侧登记。
pub const OP_REG: u8 = 1;
/// 操作码：分叉。
pub const OP_FORK: u8 = 2;
/// 操作码：子侧登记。
pub const OP_CHILD: u8 = 3;
/// 操作码：子侧退出。
pub const OP_CEXIT: u8 = 4;
/// 操作码：父侧退出。
pub const OP_PEXIT: u8 = 5;
/// 操作码：读运行序表。
pub const OP_OBS: u8 = 6;

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

/// 在表项 `[0, n)` 内找功能号，返回下标。
fn find(tab: &[u32; CAP], n: usize, f: u32) -> Option<usize> {
    let mut i = 0usize;
    while i < n {
        match tab.get(i) {
            Some(v) => {
                if *v == f {
                    return Some(i);
                }
            }
            None => return None,
        }
        i += 1;
    }
    None
}

/// 把 `f` 写进第 `n` 格；越界即满。
fn put(tab: &mut [u32; CAP], n: usize, f: u32) -> Result<(), u32> {
    match tab.get_mut(n) {
        Some(x) => {
            *x = f;
            Ok(())
        }
        None => Err(FK_EFULL),
    }
}

/// 按 LIFO 把 `[0, n)` 倒序搬进 `seq`，返回搬运条数。
fn drain(tab: &[u32; CAP], n: usize, seq: &mut [u32; CAP]) -> usize {
    let mut k = 0usize;
    while k < n {
        let src = n - 1 - k;
        match tab.get(src) {
            Some(v) => {
                if let Some(dst) = seq.get_mut(k) {
                    *dst = *v;
                }
            }
            None => break,
        }
        k += 1;
    }
    k
}

/// 双表退出面。
///
/// 父表在分叉前登记，分叉时整体复制进子表；此后两侧各自追加。退出时各自按
/// LIFO 展开到各自的运行序表，互不触碰。
#[derive(Clone, Copy, Debug)]
pub struct Forker {
    parent: [u32; CAP],
    pn: usize,
    child: [u32; CAP],
    cn: usize,
    snap: usize,
    forked: bool,
    p_ran: bool,
    c_ran: bool,
    parent_seq: [u32; CAP],
    pseq: usize,
    child_seq: [u32; CAP],
    cseq: usize,
}

impl Forker {
    /// 空的双表。
    pub const fn new() -> Forker {
        Forker {
            parent: [0; CAP],
            pn: 0,
            child: [0; CAP],
            cn: 0,
            snap: 0,
            forked: false,
            p_ran: false,
            c_ran: false,
            parent_seq: [0; CAP],
            pseq: 0,
            child_seq: [0; CAP],
            cseq: 0,
        }
    }

    /// 父表条数。
    pub fn parent_count(&self) -> usize {
        self.pn
    }

    /// 子表条数。
    pub fn child_count(&self) -> usize {
        self.cn
    }

    /// 是否已经分叉。
    pub fn is_forked(&self) -> bool {
        self.forked
    }

    /// 父侧是否已退出。
    pub fn parent_ran(&self) -> bool {
        self.p_ran
    }

    /// 子侧是否已退出。
    pub fn child_ran(&self) -> bool {
        self.c_ran
    }

    /// 父侧运行序表长度。
    pub fn parent_seq_len(&self) -> usize {
        self.pseq
    }

    /// 子侧运行序表长度。
    pub fn child_seq_len(&self) -> usize {
        self.cseq
    }

    /// 父侧登记。
    pub fn register(&mut self, f: u32) -> Result<usize, u32> {
        if f == 0 {
            return Err(FK_EINVAL);
        }
        if self.p_ran {
            return Err(FK_ERUN);
        }
        if self.pn >= CAP {
            return Err(FK_EFULL);
        }
        if find(&self.parent, self.pn, f).is_some() {
            return Err(FK_EDUP);
        }
        put(&mut self.parent, self.pn, f)?;
        self.pn += 1;
        Ok(self.pn)
    }

    /// 分叉：父表整体复制进子表，返回继承条数。
    pub fn fork(&mut self) -> Result<usize, u32> {
        if self.forked {
            return Err(FK_EREENTER);
        }
        if self.p_ran {
            return Err(FK_ERUN);
        }
        let mut k = 0usize;
        while k < self.pn {
            let v = match self.parent.get(k) {
                Some(x) => *x,
                None => break,
            };
            put(&mut self.child, k, v)?;
            k += 1;
        }
        self.cn = k;
        self.snap = k;
        self.forked = true;
        Ok(self.cn)
    }

    /// 子侧登记（只进子表）。
    pub fn register_child(&mut self, f: u32) -> Result<usize, u32> {
        if !self.forked {
            return Err(FK_EFORK);
        }
        if f == 0 {
            return Err(FK_EINVAL);
        }
        if self.c_ran {
            return Err(FK_ERUN);
        }
        if self.cn >= CAP {
            return Err(FK_EFULL);
        }
        if find(&self.child, self.cn, f).is_some() {
            return Err(FK_EDUP);
        }
        put(&mut self.child, self.cn, f)?;
        self.cn += 1;
        Ok(self.cn)
    }

    /// 子进程退出：只展开子表。
    pub fn child_exit(&mut self) -> Result<usize, u32> {
        if !self.forked {
            return Err(FK_EFORK);
        }
        if self.c_ran {
            return Err(FK_ERUN);
        }
        let k = drain(&self.child, self.cn, &mut self.child_seq);
        self.cseq = k;
        self.c_ran = true;
        Ok(k)
    }

    /// 父进程退出：只展开父表。
    pub fn parent_exit(&mut self) -> Result<usize, u32> {
        if self.p_ran {
            return Err(FK_ERUN);
        }
        let k = drain(&self.parent, self.pn, &mut self.parent_seq);
        self.pseq = k;
        self.p_ran = true;
        Ok(k)
    }

    /// 父侧运行序第 i 条。
    pub fn parent_func_at(&self, i: usize) -> Result<u32, u32> {
        if i >= self.pseq {
            return Err(FK_ENOBS);
        }
        match self.parent_seq.get(i) {
            Some(v) => Ok(*v),
            None => Err(FK_ENOBS),
        }
    }

    /// 子侧运行序第 i 条。
    pub fn child_func_at(&self, i: usize) -> Result<u32, u32> {
        if i >= self.cseq {
            return Err(FK_ENOBS);
        }
        match self.child_seq.get(i) {
            Some(v) => Ok(*v),
            None => Err(FK_ENOBS),
        }
    }

    /// 分叉瞬间的继承前缀长度：只比对快照段，分叉后追加的条目不算继承。
    pub fn verify_inherit(&self) -> Result<usize, u32> {
        if !self.forked {
            return Err(FK_EFORK);
        }
        if self.pn < self.snap || self.cn < self.snap {
            return Err(FK_EPAIR);
        }
        let mut k = 0usize;
        while k < self.snap {
            let a = match self.parent.get(k) {
                Some(v) => *v,
                None => return Err(FK_EPAIR),
            };
            let b = match self.child.get(k) {
                Some(v) => *v,
                None => return Err(FK_EPAIR),
            };
            if a != b {
                return Err(FK_ESIDE);
            }
            k += 1;
        }
        Ok(k)
    }

    /// 运行序表是否严格等于各自表的倒序。
    pub fn consistent(&self) -> bool {
        let mut k = 0usize;
        while k < self.pseq {
            let want = match self.pn.checked_sub(1 + k).and_then(|i| self.parent.get(i)) {
                Some(v) => *v,
                None => return false,
            };
            match self.parent_seq.get(k) {
                Some(v) => {
                    if *v != want {
                        return false;
                    }
                }
                None => return false,
            }
            k += 1;
        }
        let mut j = 0usize;
        while j < self.cseq {
            let want = match self.cn.checked_sub(1 + j).and_then(|i| self.child.get(i)) {
                Some(v) => *v,
                None => return false,
            };
            match self.child_seq.get(j) {
                Some(v) => {
                    if *v != want {
                        return false;
                    }
                }
                None => return false,
            }
            j += 1;
        }
        true
    }

    /// 两侧条目是否互不串线：子表前 `min` 格必须都是父表登记时的快照。
    pub fn splits_clean(&self) -> bool {
        if !self.forked {
            return self.cn == 0;
        }
        match self.verify_inherit() {
            Ok(_) => true,
            Err(_) => false,
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
            return Err(FK_EORDER);
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
            return Err(FK_EBAL);
        }
        const VALS: [u32; 8] = [1, 2, 2, 3, 3, 9, 2, 2];
        const CODES: [u32; 6] = [FK_EDUP, FK_ENOBS, FK_EINVAL, FK_ERUN, FK_EFORK, FK_EFULL];
        let mut vi = 0usize;
        let mut ci = 0usize;
        let mut i = 0usize;
        while i < self.len {
            let s = match self.rows.get(i) {
                Some(v) => *v,
                None => return Err(FK_EBAL),
            };
            if s.slot as usize != i {
                return Err(FK_EORDER);
            }
            if s.ok {
                if s.code != 0 {
                    return Err(FK_EMISMATCH);
                }
                match VALS.get(vi) {
                    Some(w) => {
                        if s.value != *w {
                            return Err(FK_EMISMATCH);
                        }
                    }
                    None => return Err(FK_EBAL),
                }
                vi += 1;
            } else {
                match CODES.get(ci) {
                    Some(w) => {
                        if s.code != *w {
                            return Err(FK_EMISMATCH);
                        }
                    }
                    None => return Err(FK_EBAL),
                }
                ci += 1;
            }
            i += 1;
        }
        if vi != 8 || ci != 6 {
            return Err(FK_EBAL);
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
            return Err(FK_EBAL);
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
            Err(FK_ESEAL)
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

/// 十四格账本：父登记两件 → 分叉 → 子侧追加 → 子退 → 父退 → 各类拒绝。
pub fn build_ledger() -> Ledger {
    let mut l = Ledger::new();
    let mut f = Forker::new();

    let _ = l.register(cell(0, OP_REG, f.register(1)));
    let _ = l.register(cell(1, OP_REG, f.register(2)));
    let _ = l.register(cell(2, OP_FORK, f.fork()));
    let _ = l.register(cell(3, OP_CHILD, f.register_child(9)));
    let _ = l.register(cell(4, OP_CHILD, f.register_child(9)));
    let _ = l.register(cell(5, OP_CEXIT, f.child_exit()));
    let _ = l.register(word_cell(6, OP_OBS, f.child_func_at(0)));
    let _ = l.register(word_cell(7, OP_OBS, f.child_func_at(3)));
    let _ = l.register(cell(8, OP_PEXIT, f.parent_exit()));
    let _ = l.register(word_cell(9, OP_OBS, f.parent_func_at(0)));
    let _ = l.register(cell(10, OP_REG, f.register(0)));
    let _ = l.register(cell(11, OP_REG, f.register(3)));

    let mut nofork = Forker::new();
    let _ = l.register(cell(12, OP_CHILD, nofork.register_child(10)));

    let mut full = Forker::new();
    let mut i = 1u32;
    while (i as usize) <= CAP {
        let _ = full.register(i);
        i += 1;
    }
    let _ = full.fork();
    let _ = l.register(cell(13, OP_CHILD, full.register_child(0x100)));

    l
}

/// 四路注入。
pub fn inject_four() -> [bool; 4] {
    let mut out = [false; 4];

    let l = build_ledger();
    let rep = l.replay();
    let root = l.seal();
    out[0] = l.len() == SAMPLES && l.audit().is_ok() && rep == Ok(root) && root != 0;

    // 一路：分叉瞬间整表继承，继承前缀严格等于父表当时的长度。
    let mut a = Forker::new();
    let _ = a.register(1);
    let _ = a.register(2);
    let fk = a.fork();
    let _ = a.register(3);
    out[1] = fk == Ok(2)
        && a.parent_count() == 3
        && a.child_count() == 2
        && a.verify_inherit() == Ok(2)
        && a.splits_clean()
        && a.parent_seq_len() == 0
        && a.child_seq_len() == 0;

    // 二路：子退出只跑子表（含继承项），父侧运行标记不动。
    let mut b = Forker::new();
    let _ = b.register(4);
    let _ = b.register(5);
    let _ = b.fork();
    let _ = b.register_child(6);
    let ce = b.child_exit();
    out[2] = ce == Ok(3)
        && b.child_func_at(0) == Ok(6)
        && b.child_func_at(1) == Ok(5)
        && b.child_func_at(2) == Ok(4)
        && b.child_func_at(3) == Err(FK_ENOBS)
        && b.parent_func_at(0) == Err(FK_ENOBS)
        && !b.parent_ran()
        && b.child_ran()
        && b.consistent();

    // 三路：闸门与两侧独立（父退后子仍可退；已分叉不可再分叉）。
    let mut c = Forker::new();
    let g1 = c.register_child(1) == Err(FK_EFORK);
    let g2 = c.register(0) == Err(FK_EINVAL);
    let g3 = c.register(7) == Ok(1);
    let g4 = c.register(8) == Ok(2);
    let g5 = c.fork() == Ok(2);
    let g6 = c.fork() == Err(FK_EREENTER);
    let g7 = c.register_child(8) == Err(FK_EDUP);
    let g8 = c.register_child(9) == Ok(3);
    let g9 = c.parent_exit() == Ok(2);
    let g10 = c.parent_func_at(0) == Ok(8) && c.parent_func_at(1) == Ok(7);
    let g11 = c.register(10) == Err(FK_ERUN);
    let g12 = c.child_exit() == Ok(3);
    let g13 = c.child_func_at(0) == Ok(9) && c.child_func_at(1) == Ok(8);
    let g14 = c.child_exit() == Err(FK_ERUN) && c.register_child(11) == Err(FK_ERUN);
    out[3] = g1 && g2 && g3 && g4 && g5 && g6 && g7 && g8 && g9 && g10 && g11 && g12 && g13 && g14;

    out
}

/// 十三项判据。
pub fn run_unx11329_checks() -> CheckSet {
    let mut s = CheckSet::new("unx11329.fork_atexit");
    let l = build_ledger();

    s.add_dyn(
        "FK1-十四格八允六拒",
        l.len() == SAMPLES && l.ok_count() == 8 && l.err_count() == 6 && l.audit().is_ok(),
        &format!("n={} ok={} err={}", l.len(), l.ok_count(), l.err_count()),
    );

    s.add(
        "FK2-父侧登记计数递增",
        l.sample(0).map(|x| x.value) == Some(1) && l.sample(1).map(|x| x.value) == Some(2),
        "static",
    );

    s.add(
        "FK3-分叉整表继承",
        l.sample(2).map(|x| x.value) == Some(2),
        "static",
    );

    s.add(
        "FK4-子侧追加只入子表",
        l.sample(3).map(|x| x.value) == Some(3),
        "static",
    );

    s.add(
        "FK5-重复登记被拒",
        l.sample(4).map(|x| x.code) == Some(FK_EDUP),
        "static",
    );

    s.add(
        "FK6-子退出条数与前序",
        l.sample(5).map(|x| x.value) == Some(3) && l.sample(6).map(|x| x.value) == Some(9),
        "static",
    );

    s.add(
        "FK7-子序越界拒绝",
        l.sample(7).map(|x| x.code) == Some(FK_ENOBS),
        "static",
    );

    s.add(
        "FK8-父退出条数与前序",
        l.sample(8).map(|x| x.value) == Some(2) && l.sample(9).map(|x| x.value) == Some(2),
        "static",
    );

    s.add(
        "FK9-零号与运行后登记",
        l.sample(10).map(|x| x.code) == Some(FK_EINVAL)
            && l.sample(11).map(|x| x.code) == Some(FK_ERUN),
        "static",
    );

    s.add(
        "FK10-未分叉与子表满",
        l.sample(12).map(|x| x.code) == Some(FK_EFORK)
            && l.sample(13).map(|x| x.code) == Some(FK_EFULL),
        "static",
    );

    s.add(
        "FK11-分叉后父登记不入子",
        {
            let mut a = Forker::new();
            let _ = a.register(21);
            let _ = a.register(22);
            let _ = a.fork();
            let _ = a.register(23);
            let _ = a.register_child(24);
            a.parent_count() == 3
                && a.child_count() == 3
                && a.verify_inherit() == Ok(2)
                && a.parent_seq_len() == 0
        },
        "static",
    );

    s.add(
        "FK12-子退不触父且双表独立",
        {
            let mut a = Forker::new();
            let _ = a.register(31);
            let _ = a.register(32);
            let _ = a.register(33);
            let _ = a.fork();
            let ce = a.child_exit();
            let parent_free = !a.parent_ran() && a.parent_func_at(0) == Err(FK_ENOBS);
            let pe = a.parent_exit();
            ce == Ok(3)
                && parent_free
                && pe == Ok(3)
                && a.parent_func_at(0) == Ok(33)
                && a.parent_func_at(2) == Ok(31)
                && a.child_func_at(0) == Ok(33)
                && a.consistent()
        },
        "static",
    );

    s.add(
        "FK13-冻结重放四注入",
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
        let s = run_unx11329_checks();
        assert_eq!(s.len(), 13);
        assert!(s.all_passed());
    }

    #[test]
    fn fork_copies_snapshot() {
        let mut a = Forker::new();
        assert_eq!(a.register(1), Ok(1));
        assert_eq!(a.register(2), Ok(2));
        assert_eq!(a.fork(), Ok(2));
        assert_eq!(a.verify_inherit(), Ok(2));
        assert_eq!(a.register(3), Ok(3));
        assert_eq!(a.parent_count(), 3);
        assert_eq!(a.child_count(), 2);
    }

    #[test]
    fn child_exit_leaves_parent_untouched() {
        let mut a = Forker::new();
        let _ = a.register(11);
        let _ = a.register(12);
        let _ = a.fork();
        let _ = a.register_child(13);
        assert_eq!(a.child_exit(), Ok(3));
        assert!(!a.parent_ran());
        assert_eq!(a.child_func_at(0), Ok(13));
        assert_eq!(a.child_func_at(2), Ok(11));
        assert_eq!(a.parent_func_at(0), Err(FK_ENOBS));
        assert!(a.consistent());
    }

    #[test]
    fn parent_exit_is_lifo() {
        let mut a = Forker::new();
        let _ = a.register(1);
        let _ = a.register(2);
        let _ = a.register(3);
        assert_eq!(a.parent_exit(), Ok(3));
        assert_eq!(a.parent_func_at(0), Ok(3));
        assert_eq!(a.parent_func_at(1), Ok(2));
        assert_eq!(a.parent_func_at(2), Ok(1));
        assert_eq!(a.parent_func_at(3), Err(FK_ENOBS));
    }

    #[test]
    fn gates() {
        let mut a = Forker::new();
        assert_eq!(a.register_child(1), Err(FK_EFORK));
        assert_eq!(a.register(0), Err(FK_EINVAL));
        assert_eq!(a.register(5), Ok(1));
        assert_eq!(a.register(5), Err(FK_EDUP));
        assert_eq!(a.fork(), Ok(1));
        assert_eq!(a.fork(), Err(FK_EREENTER));
        assert_eq!(a.register_child(0), Err(FK_EINVAL));
        assert_eq!(a.parent_exit(), Ok(1));
        assert_eq!(a.register(6), Err(FK_ERUN));
        assert_eq!(a.parent_exit(), Err(FK_ERUN));
        assert_eq!(a.child_exit(), Ok(1));
        assert_eq!(a.child_exit(), Err(FK_ERUN));
    }

    #[test]
    fn capacity_is_per_side() {
        let mut a = Forker::new();
        let mut i = 1u32;
        while (i as usize) <= CAP {
            assert!(a.register(i).is_ok());
            i += 1;
        }
        assert_eq!(a.register(0xFFFF), Err(FK_EFULL));
        assert_eq!(a.fork(), Ok(CAP));
        assert_eq!(a.register_child(0xFFFF), Err(FK_EFULL));
    }

    #[test]
    fn freeze_blocks_writes() {
        let mut l = build_ledger();
        assert!(l.freeze().is_ok());
        assert!(l.register(Sample::blank()).is_err());
        assert_eq!(l.rejects(), 1);
    }
}
