//! UNX-F11524 cmp 字节级比较（0xf1d0 段）
//!
//! `cmp` 的职责不是「相等/不相等」两个字，而是把两份字节流按顺序对齐后给出
//! 第一处不一致的偏移、两侧的字节值，以及「前缀相同但长度不同」这种最容易
//! 被误判成相等的形态。Linux 手册把后一种情形写成 `cmp: EOF on FILE1`，退出
//! 码与「内容不同」相同，但语义上是长度问题而不是字节问题，因此本模块单独
//! 用 `CM_EPART` 表达。
//!
//! 比较窗口由三个参数决定：
//!
//! - `skip1` / `skip2`：两侧各自跳过的头部字节数，允许不同；
//! - `limit`：最多比较多少字节，为零即非法；超过窗口长度时按窗口截断，
//!   不算错误；
//!
//! 超过有效长度（`len - skip`）的跳过量属于参数错误，必须在开户时拒绝：
//! 若留到比较阶段，会把「调用者算错了偏移」伪装成「文件更短」。
//!
//! 三种输出模式：
//!
//! - `MODE_FIRST`：遇到第一处差异立刻停，只报告它；
//! - `MODE_LIST`：把差异逐条登记，超过表容量报 `CM_EFULL`，不静默丢弃；
//! - `MODE_SILENT`：只给判定，不保留差异细节。
//!
//! 闸门次序：
//!
//! - `open`：跳过量越界 → 上限为零；
//! - `run`：模式非法 → 重复比较 → 表容量溢出；
//! - `tail`：尚未比较 → 已有字节差异 → 长度不等（`CM_EPART`）。
//!
//! 重复 `run` 被拒绝而不是重新开始：一次开户只对应一次比较，避免调用者拿两
//! 组互不一致的统计拼凑出假结论。


use crate::checks::CheckSet;

/// 段基址。
pub const SEG_BASE: u32 = 0xf1d0;

/// 参数非法。
pub const CM_EINVAL: u32 = SEG_BASE + 0x01;
/// 跳过量越过有效长度。
pub const CM_ESKIP: u32 = SEG_BASE + 0x02;
/// 比较上限非法。
pub const CM_ELIMIT: u32 = SEG_BASE + 0x03;
/// 输出模式非法。
pub const CM_EMODE: u32 = SEG_BASE + 0x04;
/// 差异表满。
pub const CM_EFULL: u32 = SEG_BASE + 0x05;
/// 前缀相同但长度不同。
pub const CM_EPART: u32 = SEG_BASE + 0x06;
/// 不存在字节差异。
pub const CM_EDIFF: u32 = SEG_BASE + 0x07;
/// 尚未比较或重复比较。
pub const CM_ERUN: u32 = SEG_BASE + 0x08;
/// 账本形状不符。
pub const CM_EBAL: u32 = SEG_BASE + 0x09;
/// 槽位乱序。
pub const CM_ESLOT: u32 = SEG_BASE + 0x0a;
/// 观测值与复算不符。
pub const CM_EVALUE: u32 = SEG_BASE + 0x0b;
/// 冻结后重放不一致。
pub const CM_ESEAL: u32 = SEG_BASE + 0x0c;
/// 观察下标越界。
pub const CM_ENOBS: u32 = SEG_BASE + 0x0d;

/// 差异表容量。
pub const CAP: usize = 8;
/// 账本格数。
pub const SAMPLES: usize = 14;

/// 模式：遇到第一处差异即停。
pub const MODE_FIRST: u8 = 1;
/// 模式：登记全部差异。
pub const MODE_LIST: u8 = 2;
/// 模式：只给判定。
pub const MODE_SILENT: u8 = 3;

/// 操作码：比较。
pub const OP_RUN: u8 = 1;
/// 操作码：查首差。
pub const OP_FIRST: u8 = 2;
/// 操作码：查尾部结论。
pub const OP_TAIL: u8 = 3;
/// 操作码：查整体判定。
pub const OP_VERDICT: u8 = 4;
/// 操作码：查第 i 条差异。
pub const OP_DIFF: u8 = 5;

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

/// 一处字节差异。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Diff {
    /// 窗口内偏移。
    pub pos: u32,
    /// 左侧字节。
    pub a: u8,
    /// 右侧字节。
    pub b: u8,
}

/// 一次比较的结论。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Outcome {
    /// 实际参与比较的字节数。
    pub compared: u32,
    /// 窗口内差异条数（`MODE_LIST` 下不超过表容量）。
    pub diffs: u32,
    /// 第一处差异的窗口内偏移。
    pub first: Option<u32>,
}

/// 比较器。
#[derive(Clone, Debug)]
pub struct Cmp<'a> {
    a: &'a [u8],
    b: &'a [u8],
    skip1: usize,
    skip2: usize,
    limit: usize,
    diffs: [Diff; CAP],
    n: usize,
    found: u32,
    first: Option<u32>,
    compared: u32,
    ran: bool,
}

impl<'a> Cmp<'a> {
    /// 开户：两侧字节流、各自跳过量与比较上限。
    pub fn open(
        a: &'a [u8],
        b: &'a [u8],
        skip1: u32,
        skip2: u32,
        limit: u32,
    ) -> Result<Cmp<'a>, u32> {
        if skip1 as usize > a.len() || skip2 as usize > b.len() {
            return Err(CM_ESKIP);
        }
        if limit == 0 {
            return Err(CM_ELIMIT);
        }
        Ok(Cmp {
            a,
            b,
            skip1: skip1 as usize,
            skip2: skip2 as usize,
            limit: limit as usize,
            diffs: [Diff { pos: 0, a: 0, b: 0 }; CAP],
            n: 0,
            found: 0,
            first: None,
            compared: 0,
            ran: false,
        })
    }

    /// 左侧有效长度。
    pub fn len_a(&self) -> usize {
        self.a.len() - self.skip1
    }

    /// 右侧有效长度。
    pub fn len_b(&self) -> usize {
        self.b.len() - self.skip2
    }

    /// 窗口长度（两侧较短者与上限的较小值）。
    pub fn window(&self) -> usize {
        let m = if self.len_a() < self.len_b() { self.len_a() } else { self.len_b() };
        if self.limit < m {
            self.limit
        } else {
            m
        }
    }

    /// 左侧窗口起点。
    pub fn skip_a(&self) -> usize {
        self.skip1
    }

    /// 右侧窗口起点。
    pub fn skip_b(&self) -> usize {
        self.skip2
    }

    /// 执行比较。
    pub fn run(&mut self, mode: u8) -> Result<Outcome, u32> {
        if mode != MODE_FIRST && mode != MODE_LIST && mode != MODE_SILENT {
            return Err(CM_EMODE);
        }
        if self.ran {
            return Err(CM_ERUN);
        }
        let win = self.window();
        let mut i = 0usize;
        let mut count = 0u32;
        let mut first: Option<u32> = None;
        while i < win {
            let x = match self.a.get(self.skip1 + i) {
                Some(v) => *v,
                None => return Err(CM_EINVAL),
            };
            let y = match self.b.get(self.skip2 + i) {
                Some(v) => *v,
                None => return Err(CM_EINVAL),
            };
            if x != y {
                count = count.saturating_add(1);
                if first.is_none() {
                    first = Some(i as u32);
                }
                if mode == MODE_LIST {
                    if self.n >= CAP {
                        self.ran = true;
                        self.compared = win as u32;
                        return Err(CM_EFULL);
                    }
                    if let Some(dst) = self.diffs.get_mut(self.n) {
                        *dst = Diff { pos: i as u32, a: x, b: y };
                    }
                    self.n += 1;
                }
                if mode == MODE_FIRST {
                    break;
                }
            }
            i += 1;
        }
        self.ran = true;
        self.compared = win as u32;
        self.found = count;
        self.first = first;
        Ok(Outcome { compared: self.compared, diffs: count, first })
    }

    /// 第一处差异的窗口内偏移。
    pub fn first_diff(&self) -> Result<u32, u32> {
        if !self.ran {
            return Err(CM_ERUN);
        }
        if self.found == 0 {
            return Err(CM_EDIFF);
        }
        match self.first {
            Some(p) => Ok(p),
            None => Err(CM_EBAL),
        }
    }

    /// 窗口内差异总数（不受差异表容量影响）。
    pub fn found_count(&self) -> u32 {
        self.found
    }

    /// 差异条数。
    pub fn diff_count(&self) -> usize {
        self.n
    }

    /// 第 i 条差异。
    pub fn diff_at(&self, i: usize) -> Option<Diff> {
        if i >= self.n {
            return None;
        }
        self.diffs.get(i).copied()
    }

    /// 第 i 条差异压成一个字：`pos << 8 | (a ^ b)`。
    pub fn diff_word(&self, i: usize) -> Result<u32, u32> {
        match self.diff_at(i) {
            Some(d) => Ok(((d.pos & 0x00ff_ffff) << 8) | (d.a ^ d.b) as u32),
            None => Err(CM_ENOBS),
        }
    }

    /// 尾部结论：两侧字节完全相同时返回有效长度，否则给出原因。
    pub fn tail(&self) -> Result<u32, u32> {
        if !self.ran {
            return Err(CM_ERUN);
        }
        if self.found != 0 {
            return Err(CM_EDIFF);
        }
        let la = self.len_a();
        let lb = self.len_b();
        if la != lb {
            return Err(CM_EPART);
        }
        Ok(la as u32)
    }

    /// 整体判定：在窗口与尾部都一致时才算相同。
    pub fn verdict(&self) -> Result<bool, u32> {
        if !self.ran {
            return Err(CM_ERUN);
        }
        Ok(self.found == 0 && self.len_a() == self.len_b())
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

/// `Result<bool, u32>` → 样本。
fn flag_cell(slot: u8, op: u8, r: Result<bool, u32>) -> Sample {
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
            self.rejects = self.rejects.saturating_add(1);
            return Err(CM_ESLOT);
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
            return Err(CM_EBAL);
        }
        const VALS: [u32; 8] = [1, 2, 2, 4, 1, 1, 0x53f, 4];
        const CODES: [u32; 6] = [CM_ESKIP, CM_ELIMIT, CM_EMODE, CM_EFULL, CM_EDIFF, CM_ERUN];
        let mut vi = 0usize;
        let mut ci = 0usize;
        let mut i = 0usize;
        while i < self.len {
            let s = match self.rows.get(i) {
                Some(v) => *v,
                None => return Err(CM_EBAL),
            };
            if s.slot as usize != i {
                return Err(CM_ESLOT);
            }
            if s.ok {
                if s.code != 0 {
                    return Err(CM_EVALUE);
                }
                match VALS.get(vi) {
                    Some(w) => {
                        if s.value != *w {
                            return Err(CM_EVALUE);
                        }
                    }
                    None => return Err(CM_EBAL),
                }
                vi += 1;
            } else {
                match CODES.get(ci) {
                    Some(w) => {
                        if s.code != *w {
                            return Err(CM_EVALUE);
                        }
                    }
                    None => return Err(CM_EBAL),
                }
                ci += 1;
            }
            i += 1;
        }
        if vi != 8 || ci != 6 {
            return Err(CM_EBAL);
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
            self.rejects = self.rejects.saturating_add(1);
            return Err(CM_EBAL);
        }
        self.root = self.seal();
        self.frozen = true;
        Ok(self.root)
    }

    /// 重放。
    pub fn replay(&self) -> Result<u64, u32> {
        let fresh = build_ledger();
        let r = fresh.seal();
        if r == self.seal() {
            Ok(r)
        } else {
            Err(CM_ESEAL)
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

/// 十四格账本：同一对字节流的两种模式、尾部、判定、逐条差异与六类拒绝。
pub fn build_ledger() -> Ledger {
    let mut l = Ledger::new();
    let a = b"abcdef";
    let b = b"abXdeY";
    let same = b"abcd";

    let mut first = match Cmp::open(a, b, 0, 0, 64) {
        Ok(v) => v,
        Err(_) => return l,
    };
    let r0 = first.run(MODE_FIRST);
    let _ = l.register(match r0 {
        Ok(o) => Sample { slot: 0, op: OP_RUN, ok: true, code: 0, value: o.diffs },
        Err(c) => Sample { slot: 0, op: OP_RUN, ok: false, code: c, value: 0 },
    });

    let mut list = match Cmp::open(a, b, 0, 0, 64) {
        Ok(v) => v,
        Err(_) => return l,
    };
    let r1 = list.run(MODE_LIST);
    let _ = l.register(match r1 {
        Ok(o) => Sample { slot: 1, op: OP_RUN, ok: true, code: 0, value: o.diffs },
        Err(c) => Sample { slot: 1, op: OP_RUN, ok: false, code: c, value: 0 },
    });
    let _ = l.register(word_cell(2, OP_FIRST, list.first_diff()));

    let mut eq = match Cmp::open(same, same, 0, 0, 64) {
        Ok(v) => v,
        Err(_) => return l,
    };
    let _ = eq.run(MODE_LIST);
    let _ = l.register(word_cell(3, OP_TAIL, eq.tail()));
    let _ = l.register(flag_cell(4, OP_VERDICT, eq.verdict()));

    let mut silent = match Cmp::open(b"abcd", b"abce", 0, 0, 64) {
        Ok(v) => v,
        Err(_) => return l,
    };
    let r5 = silent.run(MODE_SILENT);
    let _ = l.register(match r5 {
        Ok(o) => Sample { slot: 5, op: OP_RUN, ok: true, code: 0, value: o.diffs },
        Err(c) => Sample { slot: 5, op: OP_RUN, ok: false, code: c, value: 0 },
    });
    let _ = l.register(word_cell(6, OP_DIFF, list.diff_word(1)));

    let mut dup = match Cmp::open(same, same, 0, 0, 64) {
        Ok(v) => v,
        Err(_) => return l,
    };
    let r7 = dup.run(MODE_LIST);
    let _ = l.register(match r7 {
        Ok(o) => Sample { slot: 7, op: OP_RUN, ok: true, code: 0, value: o.compared },
        Err(c) => Sample { slot: 7, op: OP_RUN, ok: false, code: c, value: 0 },
    });

    let skip_bad = match Cmp::open(a, b, 7, 0, 8) {
        Ok(_) => 0,
        Err(c) => c,
    };
    let _ = l.register(Sample {
        slot: 8,
        op: OP_RUN,
        ok: false,
        code: skip_bad,
        value: 0,
    });

    let lim_bad = match Cmp::open(a, b, 0, 0, 0) {
        Ok(_) => 0,
        Err(c) => c,
    };
    let _ = l.register(Sample { slot: 9, op: OP_RUN, ok: false, code: lim_bad, value: 0 });

    let mut bad = match Cmp::open(a, b, 0, 0, 8) {
        Ok(v) => v,
        Err(_) => return l,
    };
    let mode_bad = match bad.run(9) {
        Ok(_) => 0,
        Err(c) => c,
    };
    let _ = l.register(Sample { slot: 10, op: OP_RUN, ok: false, code: mode_bad, value: 0 });

    let x9 = b"aaaaaaaaa";
    let y9 = b"bbbbbbbbb";
    let mut full = match Cmp::open(x9, y9, 0, 0, 64) {
        Ok(v) => v,
        Err(_) => return l,
    };
    let full_code = match full.run(MODE_LIST) {
        Ok(_) => 0,
        Err(c) => c,
    };
    let _ = l.register(Sample { slot: 11, op: OP_RUN, ok: false, code: full_code, value: 0 });

    let no_diff = match dup.first_diff() {
        Ok(_) => 0,
        Err(c) => c,
    };
    let _ = l.register(Sample { slot: 12, op: OP_FIRST, ok: false, code: no_diff, value: 0 });

    let late = match Cmp::open(same, same, 0, 0, 8) {
        Ok(v) => v,
        Err(_) => return l,
    };
    let early = match late.tail() {
        Ok(_) => 0,
        Err(c) => c,
    };
    let _ = l.register(Sample { slot: 13, op: OP_TAIL, ok: false, code: early, value: 0 });

    l
}

/// 四路注入。
pub fn inject_four() -> [bool; 4] {
    let mut out = [false; 4];

    let l = build_ledger();
    let rep = l.replay();
    let root = l.seal();
    out[0] = l.len() == SAMPLES && l.audit().is_ok() && rep == Ok(root) && root != 0;

    // 一路：相同流、首差定位、列表细节、尾部结论。
    let s = b"hello";
    let mut eq = match Cmp::open(s, s, 0, 0, 16) {
        Ok(v) => v,
        Err(_) => return out,
    };
    let same = eq.run(MODE_FIRST) == Ok(Outcome { compared: 5, diffs: 0, first: None });
    let verdict = eq.verdict() == Ok(true);
    let tail = eq.tail() == Ok(5);
    let no_first = eq.first_diff() == Err(CM_EDIFF);

    let mut d = match Cmp::open(b"abcdef", b"abXdeY", 0, 0, 16) {
        Ok(v) => v,
        Err(_) => return out,
    };
    let got = d.run(MODE_LIST) == Ok(Outcome { compared: 6, diffs: 2, first: Some(2) });
    let at0 = d.diff_at(0) == Some(Diff { pos: 2, a: b'c', b: b'X' });
    let at1 = d.diff_at(1) == Some(Diff { pos: 5, a: b'f', b: b'Y' });
    let w1 = d.diff_word(1) == Ok((5u32 << 8) | (b'f' ^ b'Y') as u32);
    out[1] = same && verdict && tail && no_first && got && at0 && at1 && w1;

    // 二路：长度不同、table 溢出、模式与重复比较。
    let mut p = match Cmp::open(b"abc", b"abcd", 0, 0, 16) {
        Ok(v) => v,
        Err(_) => return out,
    };
    let _ = p.run(MODE_FIRST);
    let part = p.tail() == Err(CM_EPART) && p.verdict() == Ok(false);

    let mut f = match Cmp::open(b"aaaaaaaaaa", b"bbbbbbbbbb", 0, 0, 16) {
        Ok(v) => v,
        Err(_) => return out,
    };
    let full = f.run(MODE_LIST) == Err(CM_EFULL) && f.diff_count() == CAP;
    let twice = f.run(MODE_LIST) == Err(CM_ERUN);

    let mut m = match Cmp::open(b"a", b"b", 0, 0, 4) {
        Ok(v) => v,
        Err(_) => return out,
    };
    let mode = m.run(0) == Err(CM_EMODE) && m.tail() == Err(CM_ERUN);
    out[2] = part && full && twice && mode;

    // 三路：跳过与上限的边界、limit 截断、冻结后拒绝。
    let skip_hi = matches!(Cmp::open(b"abc", b"abc", 4, 0, 8), Err(CM_ESKIP));
    let skip_b = matches!(Cmp::open(b"abc", b"abc", 0, 4, 8), Err(CM_ESKIP));
    let lim_zero = matches!(Cmp::open(b"abc", b"abc", 0, 0, 0), Err(CM_ELIMIT));

    let mut sk = match Cmp::open(b"xxabc", b"yyabc", 2, 2, 8) {
        Ok(v) => v,
        Err(_) => return out,
    };
    let sk_ok = sk.run(MODE_FIRST) == Ok(Outcome { compared: 3, diffs: 0, first: None });
    let sk_win = sk.window() == 3 && sk.skip_a() == 2 && sk.skip_b() == 2;

    let mut lim = match Cmp::open(b"abcdef", b"abcXef", 0, 0, 3) {
        Ok(v) => v,
        Err(_) => return out,
    };
    let lim_ok = lim.run(MODE_FIRST) == Ok(Outcome { compared: 3, diffs: 0, first: None });

    let mut led = build_ledger();
    let one = led.freeze();
    let rep = led.replay();
    let two = led.freeze();
    let reg = led.register(Sample { slot: 14, op: OP_RUN, ok: true, code: 0, value: 0 });
    out[3] = skip_hi
        && skip_b
        && lim_zero
        && sk_ok
        && sk_win
        && lim_ok
        && one.is_ok()
        && rep == one
        && two.is_err()
        && reg == Err(CM_ESLOT)
        && led.is_frozen()
        && led.rejects() == 2;

    out
}

/// 十三项判据。
pub fn run_unx11524_checks() -> CheckSet {
    let mut s = CheckSet::new("unx11524.cmp_bytes");

    s.add(
        "CM01-相同流判定",
        {
            let mut good = false;
            if let Ok(mut c) = Cmp::open(b"kernel", b"kernel", 0, 0, 16) {
                good = c.run(MODE_FIRST) == Ok(Outcome { compared: 6, diffs: 0, first: None })
                    && c.verdict() == Ok(true)
                    && c.tail() == Ok(6)
                    && c.first_diff() == Err(CM_EDIFF);
            }
            good
        },
        "static",
    );

    s.add(
        "CM02-首差定位",
        {
            let mut good = false;
            if let Ok(mut c) = Cmp::open(b"abcdef", b"abXdeY", 0, 0, 16) {
                let _ = c.run(MODE_LIST);
                good = c.first_diff() == Ok(2)
                    && c.diff_at(0) == Some(Diff { pos: 2, a: b'c', b: b'X' })
                    && c.diff_at(2).is_none();
            }
            good
        },
        "static",
    );

    s.add(
        "CM03-列表收集",
        {
            let mut good = false;
            if let Ok(mut c) = Cmp::open(b"abcdef", b"abXdeY", 0, 0, 16) {
                if let Ok(o) = c.run(MODE_LIST) {
                    good = o.diffs == 2
                        && c.diff_count() == 2
                        && c.diff_at(1) == Some(Diff { pos: 5, a: b'f', b: b'Y' })
                        && c.diff_word(1) == Ok((5u32 << 8) | (b'f' ^ b'Y') as u32)
                        && c.diff_word(2) == Err(CM_ENOBS);
                }
            }
            good
        },
        "static",
    );

    s.add(
        "CM04-尾部长度判据",
        {
            let mut good = false;
            if let Ok(mut a) = Cmp::open(b"abc", b"abcd", 0, 0, 16) {
                let _ = a.run(MODE_FIRST);
                if let Ok(mut b) = Cmp::open(b"abcd", b"abce", 0, 0, 16) {
                    let _ = b.run(MODE_FIRST);
                    good = a.tail() == Err(CM_EPART)
                        && a.verdict() == Ok(false)
                        && b.tail() == Err(CM_EDIFF);
                }
            }
            good
        },
        "static",
    );

    s.add(
        "CM05-差异表容量",
        {
            let mut good = false;
            if let Ok(mut c) = Cmp::open(b"aaaaaaaaaa", b"bbbbbbbbbb", 0, 0, 16) {
                good = c.run(MODE_LIST) == Err(CM_EFULL)
                    && c.diff_count() == CAP
                    && c.run(MODE_LIST) == Err(CM_ERUN);
            }
            good
        },
        "static",
    );

    s.add(
        "CM06-跳过量越界",
        {
            let hi = matches!(Cmp::open(b"abc", b"abc", 4, 0, 8), Err(CM_ESKIP));
            let wi = matches!(Cmp::open(b"abc", b"abc", 0, 4, 8), Err(CM_ESKIP));
            let mut good = false;
            if let Ok(mut ok) = Cmp::open(b"xxabc", b"yyabc", 2, 2, 8) {
                good = hi
                    && wi
                    && ok.run(MODE_FIRST) == Ok(Outcome { compared: 3, diffs: 0, first: None })
                    && ok.skip_a() == 2
                    && ok.skip_b() == 2;
            }
            good
        },
        "static",
    );

    s.add(
        "CM07-上限与窗口截断",
        {
            let zero = matches!(Cmp::open(b"abc", b"abc", 0, 0, 0), Err(CM_ELIMIT));
            let mut good = false;
            if let Ok(mut c) = Cmp::open(b"abcdef", b"abcXef", 0, 0, 3) {
                good = zero
                    && c.run(MODE_FIRST) == Ok(Outcome { compared: 3, diffs: 0, first: None })
                    && c.window() == 3;
            }
            good
        },
        "static",
    );

    s.add(
        "CM08-模式与重复运行",
        {
            let mut good = false;
            if let Ok(mut c) = Cmp::open(b"a", b"b", 0, 0, 4) {
                let m = c.run(0) == Err(CM_EMODE);
                let t = c.tail() == Err(CM_ERUN) && c.verdict() == Err(CM_ERUN);
                let one = c.run(MODE_SILENT).is_ok();
                let twice = c.run(MODE_SILENT) == Err(CM_ERUN);
                good = m && t && one && twice;
            }
            good
        },
        "static",
    );

    s.add(
        "CM09-账本审计",
        {
            let l = build_ledger();
            l.len() == SAMPLES
                && l.ok_count() == 8
                && l.err_count() == 6
                && l.audit().is_ok()
                && l.rejects() == 0
        },
        "static",
    );

    s.add(
        "CM10-账本错误行码序",
        {
            let l = build_ledger();
            let mut got = [0u32; 6];
            let mut ci = 0usize;
            let mut i = 0usize;
            let mut good = true;
            while i < l.len() {
                match l.sample(i) {
                    Some(smp) => {
                        if !smp.ok {
                            if let Some(dst) = got.get_mut(ci) {
                                *dst = smp.code;
                            }
                            ci += 1;
                        }
                    }
                    None => {
                        good = false;
                    }
                }
                i += 1;
            }
            good
                && ci == 6
                && got == [CM_ESKIP, CM_ELIMIT, CM_EMODE, CM_EFULL, CM_EDIFF, CM_ERUN]
        },
        "static",
    );

    s.add(
        "CM11-冻结与拒绝计数",
        {
            let mut l = build_ledger();
            let first = l.freeze();
            let second = l.freeze();
            let reg = l.register(Sample::blank());
            first.is_ok() && second == Err(CM_EBAL) && reg == Err(CM_ESLOT) && l.rejects() == 2
        },
        "static",
    );

    s.add(
        "CM12-重放指纹一致",
        {
            let l = build_ledger();
            let a = l.seal();
            let b = l.replay();
            let mut f = build_ledger();
            let c = f.freeze();
            a != 0 && b == Ok(a) && c == Ok(a) && f.root() == a
        },
        "static",
    );

    s.add(
        "CM13-冻结重放四注入",
        {
            let inj = inject_four();
            let l = build_ledger();
            let mut m = build_ledger();
            let ok = m.freeze();
            inj == [true; 4]
                && l.audit().is_ok()
                && ok.is_ok()
                && m.is_frozen()
                && m.root() != 0
                && m.replay() == Ok(m.root())
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
        let mut i = 0usize;
        while i < l.len() {
            assert_eq!(l.sample(i).map(|x| x.slot as usize), Some(i));
            i += 1;
        }
    }

    #[test]
    fn equality_and_first_diff() {
        let mut c = Cmp::open(b"abcdef", b"abXdeY", 0, 0, 16).ok().unwrap();
        assert_eq!(c.run(MODE_LIST), Ok(Outcome { compared: 6, diffs: 2, first: Some(2) }));
        assert_eq!(c.first_diff(), Ok(2));
        assert_eq!(c.verdict(), Ok(false));
        assert_eq!(c.diff_at(1), Some(Diff { pos: 5, a: b'f', b: b'Y' }));
        assert_eq!(c.diff_at(2), None);
    }

    #[test]
    fn identical_streams() {
        let mut c = Cmp::open(b"same", b"same", 0, 0, 16).ok().unwrap();
        assert_eq!(c.run(MODE_SILENT), Ok(Outcome { compared: 4, diffs: 0, first: None }));
        assert_eq!(c.tail(), Ok(4));
        assert_eq!(c.verdict(), Ok(true));
        assert_eq!(c.first_diff(), Err(CM_EDIFF));
    }

    #[test]
    fn partial_prefix() {
        let mut c = Cmp::open(b"abc", b"abcd", 0, 0, 16).ok().unwrap();
        assert_eq!(c.run(MODE_FIRST), Ok(Outcome { compared: 3, diffs: 0, first: None }));
        assert_eq!(c.tail(), Err(CM_EPART));
        assert_eq!(c.verdict(), Ok(false));
    }

    #[test]
    fn list_capacity_refuses() {
        let mut c = Cmp::open(b"aaaaaaaaaa", b"bbbbbbbbbb", 0, 0, 16).ok().unwrap();
        assert_eq!(c.run(MODE_LIST), Err(CM_EFULL));
        assert_eq!(c.diff_count(), CAP);
        assert_eq!(c.run(MODE_LIST), Err(CM_ERUN));
    }

    #[test]
    fn skip_and_limit_gates() {
        assert_eq!(Cmp::open(b"abc", b"abc", 4, 0, 8).err(), Some(CM_ESKIP));
        assert_eq!(Cmp::open(b"abc", b"abc", 0, 4, 8).err(), Some(CM_ESKIP));
        assert_eq!(Cmp::open(b"abc", b"abc", 0, 0, 0).err(), Some(CM_ELIMIT));
        let mut c = Cmp::open(b"abcdef", b"abcXef", 0, 0, 3).ok().unwrap();
        assert_eq!(c.run(MODE_FIRST), Ok(Outcome { compared: 3, diffs: 0, first: None }));
    }

    #[test]
    fn mode_gate_and_rerun_gate() {
        let mut c = Cmp::open(b"a", b"b", 0, 0, 4).ok().unwrap();
        assert_eq!(c.run(0), Err(CM_EMODE));
        assert_eq!(c.tail(), Err(CM_ERUN));
        assert!(c.run(MODE_SILENT).is_ok());
        assert_eq!(c.run(MODE_SILENT), Err(CM_ERUN));
    }

    #[test]
    fn freeze_and_replay() {
        let mut l = build_ledger();
        let r = l.freeze().ok().unwrap();
        assert_eq!(l.replay(), Ok(r));
        assert_eq!(l.freeze(), Err(CM_EBAL));
        assert_eq!(l.register(Sample::blank()), Err(CM_ESLOT));
        assert_eq!(l.rejects(), 2);
        assert_eq!(l.root(), r);
        assert_eq!(l.replay(), Err(CM_ESEAL));
    }
}
