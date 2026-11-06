//! UNX-F11409 mv -i/-n/-f 覆盖策略族
//!
//! `mv` 的覆盖行为由三个互斥的选项决定：`-i` 逐个询问、`-n` 绝不覆盖、`-f`
//! 强制覆盖。它们**不是**三个可以叠加的布尔量，而是一条「最后一次出现者生效」
//! 的优先级链——`mv -nf` 等价于 `-f`，`mv -fi` 等价于 `-i`。把三者当独立位
//! 存起来，就会出现 `-nf` 既强制又禁止的自相矛盾；本账用两个字段拆开：
//! `mask` 记录**出现过**哪些选项（供审计），`last` 只记录三个策略选项里最后
//! 一个（供决策）。
//!
//! ## 决策矩阵（目标已存在时）
//! | 策略 | 目标可写 | 目标只读 |
//! |------|----------|----------|
//! | `-f` | 覆盖 | 覆盖（强制） |
//! | `-n` | 跳过 | 跳过 |
//! | `-i` 答是 | 覆盖 | 覆盖 |
//! | `-i` 答否 | 跳过 | 跳过 |
//! | `-i` 无答 | `EACCES` | `EACCES` |
//! | 缺省 | 覆盖 | `EACCES` |
//!
//! 目标不存在时一律直接落位（`DEC_PLAIN`），任何策略都不改变这一点。
//!
//! ## `execute` 的闸门次序
//! 1. 源不存在 → `ENOENT`；
//! 2. 类型闸门：文件改目录 `EISDIR`、目录改文件 `ENOTDIR`、目录覆盖非空目录
//!    `ENOTEMPTY`——**类型先于策略**，`mv -f f dir` 仍然是错误；
//! 3. 文件系统只读 → `EROFS`；
//! 4. 策略决策：`-i` 无人可问、缺省遇只读目标 → `EACCES`；
//! 5. 落位（返回 1）或跳过（返回 0）。
//!
//! ## 账本契约
//! 十四格：八格成功（四次解析、三次决策、一次强制落位），六格失败（未知选项、
//! 选项表满、源缺失、文件改目录、无人可问、非空目录）。`audit`/`seal`/`replay`
//! 与前册同构。

use crate::alloc::format;
use crate::checks::CheckSet;

/// 段基址。
pub const SEG_BASE: u32 = 0xef90;

/// 未知选项或选项前缀非法。
pub const OW_EINVAL: u32 = SEG_BASE + 0x01;
/// 选项串超过容量。
pub const OW_EFULL: u32 = SEG_BASE + 0x02;
/// 源不存在。
pub const OW_ENOENT: u32 = SEG_BASE + 0x03;
/// 文件改到目录。
pub const OW_EISDIR: u32 = SEG_BASE + 0x04;
/// 目录改到文件。
pub const OW_ENOTDIR: u32 = SEG_BASE + 0x05;
/// 目录覆盖非空目录。
pub const OW_ENOTEMPTY: u32 = SEG_BASE + 0x06;
/// 目标不可写且无强制/无回答。
pub const OW_EACCES: u32 = SEG_BASE + 0x07;
/// 文件系统只读。
pub const OW_EROFS: u32 = SEG_BASE + 0x08;
/// 账本形状不符。
pub const OW_EBAL: u32 = SEG_BASE + 0x09;
/// 槽位乱序。
pub const OW_ESLOT: u32 = SEG_BASE + 0x0a;
/// 观测值与复算不符。
pub const OW_EVALUE: u32 = SEG_BASE + 0x0b;
/// 冻结后重放不一致。
pub const OW_ESEAL: u32 = SEG_BASE + 0x0c;
/// 观察下标越界。
pub const OW_ENOBS: u32 = SEG_BASE + 0x0d;

/// 选项串容量。
pub const CAP: usize = 8;
/// 账本格数。
pub const SAMPLES: usize = 14;

/// 选项位：交互。
pub const P_I: u8 = 0x01;
/// 选项位：不覆盖。
pub const P_N: u8 = 0x02;
/// 选项位：强制。
pub const P_F: u8 = 0x04;
/// 选项位：啰嗦。
pub const P_V: u8 = 0x08;

/// 决策：目标不存在，直接落位。
pub const DEC_PLAIN: u32 = 0;
/// 决策：覆盖。
pub const DEC_REPLACE: u32 = 1;
/// 决策：跳过。
pub const DEC_SKIP: u32 = 2;

/// 回答：未给出。
pub const ANS_NONE: u8 = 0;
/// 回答：是。
pub const ANS_YES: u8 = 1;
/// 回答：否。
pub const ANS_NO: u8 = 2;

/// 类型：不存在。
pub const KIND_NONE: u8 = 0;
/// 类型：目录。
pub const KIND_DIR: u8 = 1;
/// 类型：普通文件。
pub const KIND_FILE: u8 = 2;

/// 操作码：解析选项。
pub const OP_PARSE: u8 = 1;
/// 操作码：决策。
pub const OP_DECIDE: u8 = 2;
/// 操作码：执行。
pub const OP_EXEC: u8 = 3;

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

/// 覆盖策略。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Policy {
    mask: u8,
    last: u8,
    count: u8,
}

impl Policy {
    /// 缺省策略（无选项）。
    pub const fn default_policy() -> Policy {
        Policy { mask: 0, last: 0, count: 0 }
    }

    /// 解析选项串（不含前导 `-`）。
    pub fn parse(spec: &[u8]) -> Result<Policy, u32> {
        if spec.len() > CAP {
            return Err(OW_EFULL);
        }
        let mut mask = 0u8;
        let mut last = 0u8;
        let mut count = 0u8;
        let mut i = 0usize;
        while i < spec.len() {
            let b = match spec.get(i) {
                Some(v) => *v,
                None => return Err(OW_EINVAL),
            };
            let bit = match b {
                b'i' => P_I,
                b'n' => P_N,
                b'f' => P_F,
                b'v' => P_V,
                _ => return Err(OW_EINVAL),
            };
            mask |= bit;
            if bit == P_I || bit == P_N || bit == P_F {
                last = bit;
            }
            count = count.saturating_add(1);
            i += 1;
        }
        Ok(Policy { mask, last, count })
    }

    /// 出现过的选项位。
    pub fn mask(&self) -> u8 {
        self.mask
    }

    /// 生效的策略位（`i`/`n`/`f` 之一或 0）。
    pub fn mode(&self) -> u8 {
        self.last
    }

    /// 选项个数。
    pub fn count(&self) -> u8 {
        self.count
    }

    /// 是否啰嗦。
    pub fn verbose(&self) -> bool {
        self.mask & P_V != 0
    }

    /// 决策：目标是否存在、是否可写、交互回答。
    pub fn decide(&self, exists: bool, writable: bool, answer: u8) -> Result<u32, u32> {
        if !exists {
            return Ok(DEC_PLAIN);
        }
        if self.last == P_F {
            return Ok(DEC_REPLACE);
        }
        if self.last == P_N {
            return Ok(DEC_SKIP);
        }
        if self.last == P_I {
            if answer == ANS_YES {
                return Ok(DEC_REPLACE);
            }
            if answer == ANS_NO {
                return Ok(DEC_SKIP);
            }
            return Err(OW_EACCES);
        }
        if writable {
            Ok(DEC_REPLACE)
        } else {
            Err(OW_EACCES)
        }
    }
}

/// 一次覆盖执行的现场。
#[derive(Clone, Copy, Debug)]
pub struct Mover {
    src_ok: bool,
    src_kind: u8,
    dst_kind: u8,
    dst_kids: u16,
    writable: bool,
    ro: bool,
    moved: u32,
    replaced: u32,
    skipped: u32,
    refused: u32,
}

impl Mover {
    /// 源与目标类型开户。
    pub fn new(src_kind: u8, dst_kind: u8) -> Mover {
        Mover {
            src_ok: true,
            src_kind,
            dst_kind,
            dst_kids: 0,
            writable: true,
            ro: false,
            moved: 0,
            replaced: 0,
            skipped: 0,
            refused: 0,
        }
    }

    /// 设置源是否存在。
    pub fn set_src_ok(&mut self, on: bool) {
        self.src_ok = on;
    }

    /// 设置目标目录子项数。
    pub fn set_dst_kids(&mut self, n: u16) {
        self.dst_kids = n;
    }

    /// 设置目标是否可写。
    pub fn set_writable(&mut self, on: bool) {
        self.writable = on;
    }

    /// 设置文件系统只读。
    pub fn set_ro(&mut self, on: bool) {
        self.ro = on;
    }

    /// 落位次数。
    pub fn moved(&self) -> u32 {
        self.moved
    }

    /// 覆盖次数。
    pub fn replaced(&self) -> u32 {
        self.replaced
    }

    /// 跳过次数。
    pub fn skipped(&self) -> u32 {
        self.skipped
    }

    /// 被拒次数。
    pub fn refused(&self) -> u32 {
        self.refused
    }

    /// 执行一次移动。
    pub fn execute(&mut self, p: &Policy, answer: u8) -> Result<u32, u32> {
        if !self.src_ok {
            self.refused = self.refused.saturating_add(1);
            return Err(OW_ENOENT);
        }
        if self.src_kind == KIND_DIR && self.dst_kind == KIND_FILE {
            self.refused = self.refused.saturating_add(1);
            return Err(OW_ENOTDIR);
        }
        if self.src_kind == KIND_FILE && self.dst_kind == KIND_DIR {
            self.refused = self.refused.saturating_add(1);
            return Err(OW_EISDIR);
        }
        if self.src_kind == KIND_DIR && self.dst_kind == KIND_DIR && self.dst_kids > 0 {
            self.refused = self.refused.saturating_add(1);
            return Err(OW_ENOTEMPTY);
        }
        if self.ro {
            self.refused = self.refused.saturating_add(1);
            return Err(OW_EROFS);
        }
        let dec = match p.decide(self.dst_kind != KIND_NONE, self.writable, answer) {
            Ok(d) => d,
            Err(code) => {
                self.refused = self.refused.saturating_add(1);
                return Err(code);
            }
        };
        if dec == DEC_SKIP {
            self.skipped = self.skipped.saturating_add(1);
            return Ok(0);
        }
        if dec != DEC_PLAIN && dec != DEC_REPLACE {
            self.refused = self.refused.saturating_add(1);
            return Err(OW_EINVAL);
        }
        if dec == DEC_REPLACE {
            self.replaced = self.replaced.saturating_add(1);
        }
        self.moved = self.moved.saturating_add(1);
        Ok(1)
    }

    /// 不变式自检。
    pub fn consistent(&self) -> bool {
        if self.src_kind != KIND_DIR && self.src_kind != KIND_FILE {
            return false;
        }
        if self.dst_kind != KIND_NONE
            && self.dst_kind != KIND_DIR
            && self.dst_kind != KIND_FILE
        {
            return false;
        }
        if self.dst_kind == KIND_FILE && self.dst_kids != 0 {
            return false;
        }
        if self.replaced > self.moved {
            return false;
        }
        self.moved.saturating_add(self.skipped) <= self.moved.saturating_add(self.skipped)
            && self.moved <= u32::MAX
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
    pub fn register(&mut self, row: Sample) -> Result<(), u32> {
        if self.frozen || (row.slot as usize) != self.len || (row.slot as usize) >= SAMPLES {
            self.rejects = self.rejects.saturating_add(1);
            return Err(OW_ESLOT);
        }
        if let Some(dst) = self.rows.get_mut(self.len) {
            *dst = row;
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
            if let Some(r) = self.rows.get(i) {
                if r.ok {
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
            return Err(OW_EBAL);
        }
        const VALS: [u32; 8] = [1, 4, 1, 11, 1, 0, 2, 1];
        const CODES: [u32; 6] =
            [OW_EINVAL, OW_EFULL, OW_ENOENT, OW_EISDIR, OW_EACCES, OW_ENOTEMPTY];
        let mut vi = 0usize;
        let mut ci = 0usize;
        let mut i = 0usize;
        while i < self.len {
            let r = match self.rows.get(i) {
                Some(v) => *v,
                None => return Err(OW_EBAL),
            };
            if r.slot as usize != i {
                return Err(OW_ESLOT);
            }
            if r.ok {
                if r.code != 0 {
                    return Err(OW_EVALUE);
                }
                match VALS.get(vi) {
                    Some(w) => {
                        if r.value != *w {
                            return Err(OW_EVALUE);
                        }
                    }
                    None => return Err(OW_EBAL),
                }
                vi += 1;
            } else {
                if r.value != 0 {
                    return Err(OW_EVALUE);
                }
                match CODES.get(ci) {
                    Some(w) => {
                        if r.code != *w {
                            return Err(OW_EVALUE);
                        }
                    }
                    None => return Err(OW_EBAL),
                }
                ci += 1;
            }
            i += 1;
        }
        if vi != 8 || ci != 6 {
            return Err(OW_EBAL);
        }
        Ok(())
    }

    /// 指纹。
    pub fn seal(&self) -> u64 {
        let mut h = FNV_OFF;
        let mut i = 0usize;
        while i < self.len {
            if let Some(r) = self.rows.get(i) {
                h = fold(h, r.slot as u64);
                h = fold(h, r.op as u64);
                h = fold(h, r.ok as u64);
                h = fold(h, r.code as u64);
                h = fold(h, r.value as u64);
            }
            i += 1;
        }
        fold(h, self.rejects as u64)
    }

    /// 冻结。
    pub fn freeze(&mut self) -> Result<u64, u32> {
        if self.frozen {
            self.rejects = self.rejects.saturating_add(1);
            return Err(OW_EBAL);
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
            Err(OW_ESEAL)
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

fn policy(spec: &[u8]) -> Policy {
    match Policy::parse(spec) {
        Ok(v) => v,
        Err(_) => Policy::default_policy(),
    }
}

/// 十四格账本：四次解析、三次决策、一次强制落位，再撞六类失败。
pub fn build_ledger() -> Ledger {
    let mut l = Ledger::new();

    let _ = l.register(word_cell(0, OP_PARSE, Policy::parse(b"i").map(|v| v.mode() as u32)));
    let _ = l.register(word_cell(1, OP_PARSE, Policy::parse(b"nf").map(|v| v.mode() as u32)));
    let _ = l.register(word_cell(2, OP_PARSE, Policy::parse(b"fi").map(|v| v.mode() as u32)));
    let _ = l.register(word_cell(3, OP_PARSE, Policy::parse(b"inv").map(|v| v.mask() as u32)));

    let d = Policy::default_policy();
    let _ = l.register(word_cell(4, OP_DECIDE, d.decide(true, true, ANS_NONE)));
    let i = policy(b"i");
    let _ = l.register(word_cell(5, OP_DECIDE, i.decide(false, true, ANS_NONE)));
    let n = policy(b"n");
    let _ = l.register(word_cell(6, OP_DECIDE, n.decide(true, true, ANS_YES)));

    let f = policy(b"f");
    let mut m = Mover::new(KIND_FILE, KIND_FILE);
    m.set_writable(false);
    let _ = l.register(word_cell(7, OP_EXEC, m.execute(&f, ANS_NONE)));

    let _ = l.register(word_cell(8, OP_PARSE, Policy::parse(b"z").map(|v| v.mode() as u32)));
    let mut long = [b'v'; CAP + 1];
    if let Some(slot) = long.get_mut(0) {
        *slot = b'f';
    }
    let _ = l.register(word_cell(9, OP_PARSE, Policy::parse(&long).map(|v| v.mode() as u32)));

    let mut a = Mover::new(KIND_FILE, KIND_FILE);
    a.set_src_ok(false);
    let _ = l.register(word_cell(10, OP_EXEC, a.execute(&d, ANS_NONE)));

    let mut b = Mover::new(KIND_FILE, KIND_DIR);
    let _ = l.register(word_cell(11, OP_EXEC, b.execute(&d, ANS_NONE)));

    let mut c = Mover::new(KIND_FILE, KIND_FILE);
    c.set_writable(false);
    let _ = l.register(word_cell(12, OP_EXEC, c.execute(&d, ANS_NONE)));

    let mut e = Mover::new(KIND_DIR, KIND_DIR);
    e.set_dst_kids(3);
    let _ = l.register(word_cell(13, OP_EXEC, e.execute(&f, ANS_YES)));

    l
}

/// 四路注入：账本身、解析与决策矩阵、八类失败、闸门次序。
pub fn inject_four() -> [bool; 4] {
    let mut out = [false; 4];

    let l = build_ledger();
    let rep = l.replay();
    let root = l.seal();
    out[0] = l.len() == SAMPLES && l.audit().is_ok() && rep == Ok(root) && root != 0;

    // 一路：解析与决策矩阵。
    let pi = policy(b"i");
    let pn = policy(b"n");
    let pf = policy(b"f");
    let pnf = policy(b"nf");
    let pfi = policy(b"fi");
    let pv = policy(b"v");
    out[1] = pi.mode() == P_I
        && pn.mode() == P_N
        && pf.mode() == P_F
        && pnf.mode() == P_F
        && pfi.mode() == P_I
        && pnf.mask() == (P_N | P_F)
        && pv.mode() == 0
        && pv.verbose()
        && pi.decide(true, true, ANS_YES) == Ok(DEC_REPLACE)
        && pi.decide(true, true, ANS_NO) == Ok(DEC_SKIP)
        && pi.decide(true, true, ANS_NONE) == Err(OW_EACCES)
        && pn.decide(true, true, ANS_YES) == Ok(DEC_SKIP)
        && pf.decide(true, false, ANS_NONE) == Ok(DEC_REPLACE)
        && pi.decide(false, false, ANS_NONE) == Ok(DEC_PLAIN)
        && Policy::parse(b"").map(|p| p.count()) == Ok(0);

    // 二路：八类失败各自可辨。
    let mut a = Mover::new(KIND_FILE, KIND_FILE);
    a.set_src_ok(false);
    let missing = a.execute(&pn, ANS_NONE) == Err(OW_ENOENT);
    let mut b = Mover::new(KIND_FILE, KIND_DIR);
    let f2d = b.execute(&pf, ANS_YES) == Err(OW_EISDIR);
    let mut c = Mover::new(KIND_DIR, KIND_FILE);
    let d2f = c.execute(&pf, ANS_YES) == Err(OW_ENOTDIR);
    let mut d = Mover::new(KIND_DIR, KIND_DIR);
    d.set_dst_kids(1);
    let nonempty = d.execute(&pf, ANS_YES) == Err(OW_ENOTEMPTY);
    let mut e = Mover::new(KIND_FILE, KIND_FILE);
    e.set_writable(false);
    let denied = e.execute(&Policy::default_policy(), ANS_NONE) == Err(OW_EACCES);
    let mut g = Mover::new(KIND_FILE, KIND_FILE);
    g.set_ro(true);
    let erased = g.execute(&pf, ANS_YES) == Err(OW_EROFS);
    let bad_opt = Policy::parse(b"q").is_err();
    let mut long = [b'v'; CAP + 1];
    if let Some(slot) = long.get_mut(0) {
        *slot = b'q';
    }
    let too_long = Policy::parse(&long) == Err(OW_EFULL);
    out[2] = missing
        && f2d
        && d2f
        && nonempty
        && denied
        && erased
        && bad_opt
        && too_long
        && a.consistent()
        && b.consistent()
        && c.consistent()
        && d.consistent();

    // 三路：类型先于策略、只读先于决策、缺省与强制对照。
    let mut p = Mover::new(KIND_FILE, KIND_DIR);
    p.set_writable(false);
    let type_first = p.execute(&pn, ANS_NO) == Err(OW_EISDIR);
    let mut q = Mover::new(KIND_FILE, KIND_FILE);
    q.set_ro(true);
    let ro_first = q.execute(&Policy::default_policy(), ANS_NONE) == Err(OW_EROFS);
    let mut r = Mover::new(KIND_FILE, KIND_FILE);
    r.set_writable(false);
    let forced = r.execute(&pf, ANS_NONE) == Ok(1);
    let forced_count = r.replaced() == 1 && r.moved() == 1 && r.skipped() == 0;
    let mut skip = Mover::new(KIND_FILE, KIND_FILE);
    let skipped = skip.execute(&pn, ANS_YES) == Ok(0);
    let skip_count = skip.skipped() == 1 && skip.moved() == 0 && skip.replaced() == 0;
    out[3] = type_first
        && ro_first
        && forced
        && forced_count
        && skipped
        && skip_count
        && p.consistent()
        && q.consistent()
        && r.consistent();

    out
}

/// 十三项判据。
pub fn run_unx11409_checks() -> CheckSet {
    let mut s = CheckSet::new("unx11409.mv_overwrite_family");
    let l = build_ledger();

    s.add_dyn(
        "OW1-十四格八允六拒",
        l.len() == SAMPLES && l.ok_count() == 8 && l.err_count() == 6 && l.audit().is_ok(),
        &format!("n={} ok={} err={}", l.len(), l.ok_count(), l.err_count()),
    );

    s.add(
        "OW2-选项解析",
        {
            let i = Policy::parse(b"i");
            let v = Policy::parse(b"v");
            let empty = Policy::parse(b"");
            i.map(|p| p.mode()) == Ok(P_I)
                && v.map(|p| p.verbose()) == Ok(true)
                && v.map(|p| p.mode()) == Ok(0)
                && empty.map(|p| p.count()) == Ok(0)
                && l.sample(0).map(|x| x.value) == Some(1)
        },
        "static",
    );

    s.add(
        "OW3-后写者生效",
        {
            let nf = policy(b"nf");
            let fi = policy(b"fi");
            let inv = policy(b"inv");
            nf.mode() == P_F
                && fi.mode() == P_I
                && inv.mode() == P_N
                && inv.mask() == (P_I | P_N | P_V)
                && inv.count() == 3
                && l.sample(1).map(|x| x.value) == Some(4)
                && l.sample(2).map(|x| x.value) == Some(1)
                && l.sample(3).map(|x| x.value) == Some(11)
        },
        "static",
    );

    s.add(
        "OW4-绝不覆盖跳过",
        {
            let mut m = Mover::new(KIND_FILE, KIND_FILE);
            let n = policy(b"n");
            let r = m.execute(&n, ANS_YES);
            r == Ok(0)
                && m.skipped() == 1
                && m.moved() == 0
                && m.replaced() == 0
                && l.sample(6).map(|x| x.value) == Some(2)
        },
        "static",
    );

    s.add(
        "OW5-交互三态",
        {
            let mut a = Mover::new(KIND_FILE, KIND_FILE);
            let mut b = Mover::new(KIND_FILE, KIND_FILE);
            let mut c = Mover::new(KIND_FILE, KIND_FILE);
            let i = policy(b"i");
            let yes = a.execute(&i, ANS_YES);
            let no = b.execute(&i, ANS_NO);
            let none = c.execute(&i, ANS_NONE);
            yes == Ok(1)
                && a.replaced() == 1
                && no == Ok(0)
                && b.skipped() == 1
                && none == Err(OW_EACCES)
                && c.moved() == 0
        },
        "static",
    );

    s.add(
        "OW6-缺省覆盖可写",
        {
            let mut m = Mover::new(KIND_FILE, KIND_FILE);
            let d = Policy::default_policy();
            m.execute(&d, ANS_NONE) == Ok(1)
                && m.replaced() == 1
                && m.moved() == 1
                && l.sample(4).map(|x| x.value) == Some(1)
        },
        "static",
    );

    s.add(
        "OW7-缺省遇只读拒绝",
        {
            let mut m = Mover::new(KIND_FILE, KIND_FILE);
            m.set_writable(false);
            let d = Policy::default_policy();
            m.execute(&d, ANS_NONE) == Err(OW_EACCES)
                && m.moved() == 0
                && m.refused() == 1
                && l.sample(12).map(|x| x.code) == Some(OW_EACCES)
        },
        "static",
    );

    s.add(
        "OW8-强制覆盖只读",
        {
            let mut m = Mover::new(KIND_FILE, KIND_FILE);
            m.set_writable(false);
            let f = policy(b"f");
            m.execute(&f, ANS_NONE) == Ok(1)
                && m.replaced() == 1
                && m.skipped() == 0
                && l.sample(7).map(|x| x.value) == Some(1)
        },
        "static",
    );

    s.add(
        "OW9-文件改目录被拒",
        {
            let mut m = Mover::new(KIND_FILE, KIND_DIR);
            let d = Policy::default_policy();
            m.execute(&d, ANS_YES) == Err(OW_EISDIR)
                && m.moved() == 0
                && l.sample(11).map(|x| x.code) == Some(OW_EISDIR)
        },
        "static",
    );

    s.add(
        "OW10-目录类型与非空",
        {
            let mut a = Mover::new(KIND_DIR, KIND_FILE);
            let mut b = Mover::new(KIND_DIR, KIND_DIR);
            b.set_dst_kids(2);
            let f = policy(b"f");
            a.execute(&f, ANS_YES) == Err(OW_ENOTDIR)
                && b.execute(&f, ANS_YES) == Err(OW_ENOTEMPTY)
                && a.consistent()
                && b.consistent()
                && l.sample(13).map(|x| x.code) == Some(OW_ENOTEMPTY)
        },
        "static",
    );

    s.add(
        "OW11-缺源与只读",
        {
            let mut a = Mover::new(KIND_FILE, KIND_FILE);
            a.set_src_ok(false);
            let mut b = Mover::new(KIND_FILE, KIND_FILE);
            b.set_ro(true);
            let d = Policy::default_policy();
            a.execute(&d, ANS_NONE) == Err(OW_ENOENT)
                && b.execute(&policy(b"f"), ANS_YES) == Err(OW_EROFS)
                && l.sample(10).map(|x| x.code) == Some(OW_ENOENT)
        },
        "static",
    );

    s.add(
        "OW12-非法选项与表满",
        {
            let bad = Policy::parse(b"z") == Err(OW_EINVAL);
            let prefix = Policy::parse(b"-f") == Err(OW_EINVAL);
            let mut long = [b'v'; CAP + 1];
            if let Some(slot) = long.get_mut(0) {
                *slot = b'z';
            }
            let full = Policy::parse(&long) == Err(OW_EFULL);
            bad && prefix && full && l.sample(8).map(|x| x.code) == Some(OW_EINVAL)
                && l.sample(9).map(|x| x.code) == Some(OW_EFULL)
        },
        "static",
    );

    s.add(
        "OW13-冻结重放四注入",
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

    fn pol(spec: &[u8]) -> Policy {
        match Policy::parse(spec) {
            Ok(v) => v,
            Err(_) => panic!("parse"),
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
        assert_ne!(l.seal(), 0);
        assert_eq!(l.replay(), Ok(l.seal()));
        let mut i = 0usize;
        while i < l.len() {
            assert_eq!(l.sample(i).map(|x| x.slot as usize), Some(i));
            i += 1;
        }
    }

    #[test]
    fn checks_pass() {
        let s = run_unx11409_checks();
        assert_eq!(s.len(), 13);
        assert!(s.all_passed());
    }

    #[test]
    fn parse_and_last_wins() {
        assert_eq!(pol(b"i").mode(), P_I);
        assert_eq!(pol(b"nf").mode(), P_F);
        assert_eq!(pol(b"fi").mode(), P_I);
        assert_eq!(pol(b"inf").mask(), P_I | P_N | P_F);
        assert_eq!(pol(b"ifn").mode(), P_N);
        assert_eq!(Policy::parse(b"x"), Err(OW_EINVAL));
        assert_eq!(Policy::parse(b"-n"), Err(OW_EINVAL));
        let long = [b'f'; CAP + 1];
        assert_eq!(Policy::parse(&long), Err(OW_EFULL));
    }

    #[test]
    fn decision_matrix() {
        let i = pol(b"i");
        let n = pol(b"n");
        let f = pol(b"f");
        let d = Policy::default_policy();
        assert_eq!(i.decide(true, true, ANS_YES), Ok(DEC_REPLACE));
        assert_eq!(i.decide(true, true, ANS_NO), Ok(DEC_SKIP));
        assert_eq!(i.decide(true, true, ANS_NONE), Err(OW_EACCES));
        assert_eq!(n.decide(true, true, ANS_YES), Ok(DEC_SKIP));
        assert_eq!(f.decide(true, false, ANS_NONE), Ok(DEC_REPLACE));
        assert_eq!(d.decide(true, true, ANS_NONE), Ok(DEC_REPLACE));
        assert_eq!(d.decide(true, false, ANS_NONE), Err(OW_EACCES));
        assert_eq!(n.decide(false, false, ANS_NONE), Ok(DEC_PLAIN));
    }

    #[test]
    fn execute_counts() {
        let mut m = Mover::new(KIND_FILE, KIND_FILE);
        let f = pol(b"f");
        assert_eq!(m.execute(&f, ANS_NONE), Ok(1));
        assert_eq!(m.moved(), 1);
        assert_eq!(m.replaced(), 1);
        let mut s = Mover::new(KIND_FILE, KIND_FILE);
        assert_eq!(s.execute(&pol(b"n"), ANS_YES), Ok(0));
        assert_eq!(s.skipped(), 1);
        assert_eq!(s.moved(), 0);
        assert!(m.consistent() && s.consistent());
    }

    #[test]
    fn type_gates_precede_policy() {
        let f = pol(b"f");
        let mut a = Mover::new(KIND_FILE, KIND_DIR);
        assert_eq!(a.execute(&f, ANS_YES), Err(OW_EISDIR));
        let mut b = Mover::new(KIND_DIR, KIND_FILE);
        assert_eq!(b.execute(&f, ANS_YES), Err(OW_ENOTDIR));
        let mut c = Mover::new(KIND_DIR, KIND_DIR);
        c.set_dst_kids(1);
        assert_eq!(c.execute(&f, ANS_YES), Err(OW_ENOTEMPTY));
        assert_eq!(a.moved() + b.moved() + c.moved(), 0);
    }

    #[test]
    fn source_ro_and_denied() {
        let d = Policy::default_policy();
        let mut a = Mover::new(KIND_FILE, KIND_FILE);
        a.set_src_ok(false);
        assert_eq!(a.execute(&d, ANS_NONE), Err(OW_ENOENT));
        let mut b = Mover::new(KIND_FILE, KIND_FILE);
        b.set_ro(true);
        assert_eq!(b.execute(&d, ANS_NONE), Err(OW_EROFS));
        let mut c = Mover::new(KIND_FILE, KIND_FILE);
        c.set_writable(false);
        assert_eq!(c.execute(&d, ANS_NONE), Err(OW_EACCES));
        assert_eq!(c.execute(&pol(b"f"), ANS_NONE), Ok(1));
    }

    #[test]
    fn inject_four_holds() {
        assert_eq!(inject_four(), [true; 4]);
    }
}
