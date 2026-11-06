//! UNX-F11368 dlopen 文件名解析：斜杠直开与无斜杠搜索序（0xed00 段）
//!
//! `dlopen("libfoo.so")` 与 `dlopen("./libfoo.so")` 走的是**两条完全不同的路径**：
//!
//! - 名字里带斜杠：动态链接器把它当路径直接打开，**不搜索任何目录**。失败就是
//!   失败，绝不会「再去 /lib 里找找看」。
//! - 名字里不带斜杠：按固定的搜索序查找——调用者的 `DT_RPATH`、环境变量
//!   `LD_LIBRARY_PATH`、`ld.so.cache`、默认目录——找到第一个就停。
//!
//! 这两条路径的失败诊断必须分开：`ESLASH` 表示「你给了路径，它不存在」，
//! `ENOENT` 表示「你在搜索序里，所有候选都找过了」。把它们混成一个 ENOENT，
//! 用户会去改 `LD_LIBRARY_PATH`，而真正的问题是他写死了一个错误路径。
//!
//! 第三个必须单列的情形是**缓存陈旧**：`ld.so.cache` 里登记了一个路径，但文件
//! 已经不在了。真实链接器会跳过它继续找；如果最终什么都没找到，报错文案指向的
//! 是缓存条目而不是「库没装」。因此本模块以 `ECACHE` 区分「缓存骗了我」与
//! 「确实没这个库」。
//!
//! 闸门次序：
//! - `add`：空名 → 名字超长 → 阶段越界 → 目录越界 → 同键幂等 → 表满；
//! - `resolve`：空名 → 名字超长 → 单点路径（带斜杠）→ 逐阶段逐目录 → 陈旧缓存 → 未命中。
//!
//! 单点路径的判定必须早于搜索序，否则带斜杠的名字会被拿去和目录拼接，产生
//! `//opt//x//lib.so` 这类只在某些文件系统上才侥幸成立的候选。

use crate::alloc::format;
use crate::checks::CheckSet;

/// 段基址。
pub const SEG_BASE: u32 = 0xed00;

/// 名字为空。
pub const SP_EINVAL: u32 = SEG_BASE + 0x01;
/// 带斜杠的名字直接打开失败。
pub const SP_ESLASH: u32 = SEG_BASE + 0x02;
/// 搜索序全部落空。
pub const SP_ENOENT: u32 = SEG_BASE + 0x03;
/// 只有缓存条目命中，但文件已不存在。
pub const SP_ECACHE: u32 = SEG_BASE + 0x04;
/// 名字超出定长缓冲。
pub const SP_EPATH: u32 = SEG_BASE + 0x05;
/// 文件表已满。
pub const SP_EFULL: u32 = SEG_BASE + 0x06;
/// 阶段号越界。
pub const SP_EBADSTAGE: u32 = SEG_BASE + 0x07;
/// 目录号越界。
pub const SP_EBADDIR: u32 = SEG_BASE + 0x08;
/// 账本形状不符。
pub const SP_EBAL: u32 = SEG_BASE + 0x09;
/// 槽位乱序。
pub const SP_EORDER: u32 = SEG_BASE + 0x0a;
/// 观测值与复算不符。
pub const SP_EMISMATCH: u32 = SEG_BASE + 0x0b;
/// 冻结后重放不一致。
pub const SP_ESEAL: u32 = SEG_BASE + 0x0c;
/// 观察下标越界。
pub const SP_ENOBS: u32 = SEG_BASE + 0x0d;

/// 名字缓冲长度。
pub const NAME_MAX: usize = 20;
/// 文件表容量。
pub const CAP: usize = 8;
/// 搜索阶段数。
pub const STAGES: usize = 4;
/// 账本格数。
pub const SAMPLES: usize = 14;

/// 阶段：调用者 RPATH。
pub const STAGE_RPATH: u8 = 0;
/// 阶段：环境目录。
pub const STAGE_ENV: u8 = 1;
/// 阶段：缓存。
pub const STAGE_CACHE: u8 = 2;
/// 阶段：默认目录。
pub const STAGE_DEFAULT: u8 = 3;

/// 操作码：登记一个可解析的文件。
pub const OP_ADD: u8 = 1;
/// 操作码：解析名字。
pub const OP_RESOLVE: u8 = 2;
/// 操作码：读文件所属阶段。
pub const OP_STAGE: u8 = 3;
/// 操作码：读解析尝试次数。
pub const OP_TRIES: u8 = 4;

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

/// 定长名字。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Str {
    bytes: [u8; NAME_MAX],
    len: u8,
}

impl Str {
    /// 空名。
    pub const fn empty() -> Str {
        Str { bytes: [0u8; NAME_MAX], len: 0 }
    }

    /// 由字面量构造（超长部分截断，调用方应先自检长度）。
    pub fn lit(s: &str) -> Str {
        let mut b = [0u8; NAME_MAX];
        let src = s.as_bytes();
        let mut i = 0usize;
        while i < NAME_MAX {
            match src.get(i) {
                Some(v) => {
                    if let Some(d) = b.get_mut(i) {
                        *d = *v;
                    } else {
                        break;
                    }
                }
                None => break,
            }
            i += 1;
        }
        Str { bytes: b, len: i as u8 }
    }

    /// 长度。
    pub fn len(&self) -> usize {
        self.len as usize
    }

    /// 是否为空。
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// 第 i 个字节。
    pub fn at(&self, i: usize) -> Option<u8> {
        if i >= self.len as usize {
            return None;
        }
        self.bytes.get(i).copied()
    }

    /// 是否含斜杠。
    pub fn has_slash(&self) -> bool {
        let mut i = 0usize;
        while i < self.len as usize {
            if self.at(i) == Some(b'/') {
                return true;
            }
            i += 1;
        }
        false
    }

    /// 是否是 `.` 或 `..`。
    pub fn is_dot(&self) -> bool {
        if self.len == 1 {
            return self.at(0) == Some(b'.');
        }
        if self.len == 2 {
            return self.at(0) == Some(b'.') && self.at(1) == Some(b'.');
        }
        false
    }

    /// 内容比较。
    pub fn same(&self, other: &Str) -> bool {
        if self.len != other.len {
            return false;
        }
        let mut i = 0usize;
        while i < self.len as usize {
            if self.at(i) != other.at(i) {
                return false;
            }
            i += 1;
        }
        true
    }

    /// 指纹。
    pub fn digest(&self) -> u64 {
        let mut h = FNV_OFF;
        let mut i = 0usize;
        while i < self.len as usize {
            h = fold(h, self.at(i).unwrap_or(0) as u64);
            i += 1;
        }
        h
    }
}

/// 文件表条目。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Ent {
    /// 所属阶段。
    pub stage: u8,
    /// 目录下标。
    pub dir: u8,
    /// 文件名。
    pub name: Str,
    /// 是否陈旧（缓存里有、文件没了）。
    pub stale: bool,
}

impl Ent {
    /// 空条目。
    pub const fn empty() -> Ent {
        Ent { stage: 0, dir: 0, name: Str::empty(), stale: false }
    }
}

/// 解析结果。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Hit {
    /// 文件号。
    pub id: u32,
    /// 命中阶段。
    pub stage: u8,
    /// 命中前的候选比较次数。
    pub tries: u32,
}

/// 名字解析器。
#[derive(Clone, Copy, Debug)]
pub struct Fs {
    ents: [Ent; CAP],
    n: usize,
    dirs: [u8; STAGES],
    resolves: u32,
    misses: u32,
    stale_hits: u32,
}

impl Fs {
    /// 每阶段的目录数：RPATH 2、环境 2、缓存 1、默认 1。
    pub const fn new() -> Fs {
        let mut dirs = [0u8; STAGES];
        dirs[STAGE_RPATH as usize] = 2;
        dirs[STAGE_ENV as usize] = 2;
        dirs[STAGE_CACHE as usize] = 1;
        dirs[STAGE_DEFAULT as usize] = 1;
        Fs {
            ents: [Ent::empty(); CAP],
            n: 0,
            dirs,
            resolves: 0,
            misses: 0,
            stale_hits: 0,
        }
    }

    /// 某阶段的目录数。
    pub fn dirs_of(&self, stage: u8) -> Result<usize, u32> {
        if stage as usize >= STAGES {
            return Err(SP_EBADSTAGE);
        }
        match self.dirs.get(stage as usize) {
            Some(v) => Ok(*v as usize),
            None => Err(SP_EBADSTAGE),
        }
    }

    /// 按名字找同阶段同目录的条目。
    fn find(&self, stage: u8, dir: u8, name: &Str) -> Option<usize> {
        let mut i = 0usize;
        while i < self.n {
            if let Some(e) = self.ents.get(i) {
                if e.stage == stage && e.dir == dir && e.name.same(name) {
                    return Some(i);
                }
            }
            i += 1;
        }
        None
    }

    /// 条目所属阶段。
    pub fn stage_of(&self, id: u32) -> Result<u8, u32> {
        if id == 0 || (id as usize) > self.n {
            return Err(SP_ENOENT);
        }
        match self.ents.get((id as usize) - 1) {
            Some(e) => Ok(e.stage),
            None => Err(SP_ENOENT),
        }
    }

    /// 登记文件。
    pub fn add(&mut self, stage: u8, dir: u8, name: &str, stale: bool) -> Result<usize, u32> {
        if name.is_empty() {
            return Err(SP_EINVAL);
        }
        if name.len() > NAME_MAX {
            return Err(SP_EPATH);
        }
        if stage as usize >= STAGES {
            return Err(SP_EBADSTAGE);
        }
        let maxdir = match self.dirs.get(stage as usize) {
            Some(v) => *v,
            None => return Err(SP_EBADSTAGE),
        };
        if dir >= maxdir {
            return Err(SP_EBADDIR);
        }
        let key = Str::lit(name);
        if let Some(i) = self.find(stage, dir, &key) {
            return Ok(i + 1);
        }
        if self.n >= CAP {
            return Err(SP_EFULL);
        }
        if let Some(dst) = self.ents.get_mut(self.n) {
            *dst = Ent { stage, dir, name: key, stale };
        }
        self.n += 1;
        Ok(self.n)
    }

    /// 解析名字。
    pub fn resolve(&mut self, name: &str) -> Result<Hit, u32> {
        if name.is_empty() {
            return Err(SP_EINVAL);
        }
        if name.len() > NAME_MAX {
            return Err(SP_EPATH);
        }
        let key = Str::lit(name);
        if key.is_dot() {
            return Err(SP_EINVAL);
        }
        let mut tries = 0u32;
        if key.has_slash() {
            let mut i = 0usize;
            while i < self.n {
                tries = tries.saturating_add(1);
                if let Some(e) = self.ents.get(i) {
                    if e.name.same(&key) && !e.stale {
                        self.resolves = self.resolves.saturating_add(1);
                        return Ok(Hit { id: (i as u32) + 1, stage: e.stage, tries });
                    }
                }
                i += 1;
            }
            self.misses = self.misses.saturating_add(1);
            return Err(SP_ESLASH);
        }
        let mut stage = 0usize;
        let mut stale_seen = false;
        while stage < STAGES {
            let maxdir = match self.dirs.get(stage) {
                Some(v) => *v,
                None => 0,
            };
            let mut dir = 0u8;
            while dir < maxdir {
                let mut i = 0usize;
                while i < self.n {
                    tries = tries.saturating_add(1);
                    if let Some(e) = self.ents.get(i) {
                        if e.stage == stage as u8 && e.dir == dir && e.name.same(&key) {
                            if e.stale {
                                stale_seen = true;
                            } else {
                                self.resolves = self.resolves.saturating_add(1);
                                return Ok(Hit {
                                    id: (i as u32) + 1,
                                    stage: e.stage,
                                    tries,
                                });
                            }
                        }
                    }
                    i += 1;
                }
                dir += 1;
            }
            stage += 1;
        }
        self.misses = self.misses.saturating_add(1);
        if stale_seen {
            self.stale_hits = self.stale_hits.saturating_add(1);
            return Err(SP_ECACHE);
        }
        Err(SP_ENOENT)
    }

    /// 登记条目数。
    pub fn len(&self) -> usize {
        self.n
    }

    /// 是否空表。
    pub fn is_empty(&self) -> bool {
        self.n == 0
    }

    /// 成功解析次数。
    pub fn resolves(&self) -> u32 {
        self.resolves
    }

    /// 未命中次数。
    pub fn misses(&self) -> u32 {
        self.misses
    }

    /// 陈旧缓存命中次数。
    pub fn stale_hits(&self) -> u32 {
        self.stale_hits
    }

    /// 表摘要。
    pub fn digest(&self) -> u64 {
        let mut h = FNV_OFF;
        let mut i = 0usize;
        while i < self.n {
            if let Some(e) = self.ents.get(i) {
                h = fold(h, e.stage as u64);
                h = fold(h, e.dir as u64);
                h = fold(h, e.name.digest());
                h = fold(h, e.stale as u64);
            }
            i += 1;
        }
        fold(h, self.resolves as u64)
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
            return Err(SP_EORDER);
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
            return Err(SP_EBAL);
        }
        const VALS: [u32; 8] = [1, 2, 3, 4, 1, 2, 4, 3];
        const CODES: [u32; 6] =
            [SP_ENOENT, SP_ESLASH, SP_EINVAL, SP_EBADSTAGE, SP_ECACHE, SP_EFULL];
        let mut vi = 0usize;
        let mut ci = 0usize;
        let mut i = 0usize;
        while i < self.len {
            let s = match self.rows.get(i) {
                Some(v) => *v,
                None => return Err(SP_EBAL),
            };
            if s.slot as usize != i {
                return Err(SP_EORDER);
            }
            if s.ok {
                if s.code != 0 {
                    return Err(SP_EMISMATCH);
                }
                match VALS.get(vi) {
                    Some(w) => {
                        if s.value != *w {
                            return Err(SP_EMISMATCH);
                        }
                    }
                    None => return Err(SP_EBAL),
                }
                vi += 1;
            } else {
                match CODES.get(ci) {
                    Some(w) => {
                        if s.code != *w {
                            return Err(SP_EMISMATCH);
                        }
                    }
                    None => return Err(SP_EBAL),
                }
                ci += 1;
            }
            i += 1;
        }
        if vi != 8 || ci != 6 {
            return Err(SP_EBAL);
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
            return Err(SP_EBAL);
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
            Err(SP_ESEAL)
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

/// 十四格账本：四登记、四解析、六类拒绝。
pub fn build_ledger() -> Ledger {
    let mut l = Ledger::new();
    let mut f = Fs::new();

    let _ = l.register(cell(0, OP_ADD, f.add(STAGE_RPATH, 0, "a.so", false)));
    let _ = l.register(cell(1, OP_ADD, f.add(STAGE_ENV, 0, "b.so", false)));
    let _ = l.register(cell(2, OP_ADD, f.add(STAGE_CACHE, 0, "c.so", false)));
    let _ = l.register(cell(3, OP_ADD, f.add(STAGE_DEFAULT, 0, "/opt/d.so", false)));
    let _ = l.register(word_cell(4, OP_RESOLVE, f.resolve("a.so").map(|h| h.id)));
    let _ = l.register(word_cell(5, OP_RESOLVE, f.resolve("b.so").map(|h| h.id)));
    let _ = l.register(word_cell(6, OP_RESOLVE, f.resolve("/opt/d.so").map(|h| h.id)));
    let _ = l.register(word_cell(7, OP_RESOLVE, f.resolve("c.so").map(|h| h.id)));

    let _ = l.register(word_cell(8, OP_RESOLVE, f.resolve("zz.so").map(|h| h.id)));

    let _ = l.register(word_cell(9, OP_RESOLVE, f.resolve("/no/x.so").map(|h| h.id)));

    let _ = l.register(word_cell(10, OP_RESOLVE, f.resolve("").map(|h| h.id)));

    let _ = l.register(cell(11, OP_ADD, f.add(9, 0, "q.so", false)));

    let _ = f.add(STAGE_CACHE, 0, "s.so", true);
    let _ = l.register(word_cell(12, OP_RESOLVE, f.resolve("s.so").map(|h| h.id)));

    let _ = f.add(STAGE_DEFAULT, 0, "e1.so", false);
    let _ = f.add(STAGE_DEFAULT, 0, "e2.so", false);
    let _ = f.add(STAGE_DEFAULT, 0, "e3.so", false);
    let _ = l.register(cell(13, OP_ADD, f.add(STAGE_DEFAULT, 0, "e4.so", false)));

    l
}

/// 四路注入。
pub fn inject_four() -> [bool; 4] {
    let mut out = [false; 4];

    let l = build_ledger();
    let rep = l.replay();
    let root = l.seal();
    out[0] = l.len() == SAMPLES && l.audit().is_ok() && rep == Ok(root) && root != 0;

    // 一路：搜索序首命即停，阶段可读出。
    let mut a = Fs::new();
    let _ = a.add(STAGE_RPATH, 0, "x.so", false);
    let _ = a.add(STAGE_ENV, 0, "x.so", false);
    let _ = a.add(STAGE_ENV, 1, "y.so", false);
    let x = a.resolve("x.so");
    let y = a.resolve("y.so");
    let sx = a.stage_of(1);
    let sy = a.stage_of(3);
    out[1] = match x {
        Ok(h) => h.id == 1 && h.stage == STAGE_RPATH && h.tries >= 1,
        Err(_) => false,
    } && match y {
        Ok(h) => h.id == 3 && h.stage == STAGE_ENV && h.tries >= 1,
        Err(_) => false,
    } && sx == Ok(STAGE_RPATH)
        && sy == Ok(STAGE_ENV)
        && a.resolves() == 2
        && a.misses() == 0
        && a.len() == 3;

    // 二路：斜杠直开、搜索落空、缓存陈旧三类可辨。
    let mut b = Fs::new();
    let _ = b.add(STAGE_RPATH, 0, "s.so", false);
    let _ = b.add(STAGE_CACHE, 0, "s.so", true);
    let _ = b.add(STAGE_CACHE, 0, "t.so", true);
    let slash = b.resolve("/abs/s.so");
    let miss = b.resolve("nope.so");
    let stale = b.resolve("t.so");
    let hit = b.resolve("s.so");
    out[2] = slash == Err(SP_ESLASH)
        && miss == Err(SP_ENOENT)
        && stale == Err(SP_ECACHE)
        && hit.map(|h| h.id) == Ok(1)
        && b.stale_hits() == 1
        && b.misses() == 3;

    // 三路：闸门次序与容量边界。
    let mut c = Fs::new();
    let g1 = c.add(0, 0, "", false);
    let g2 = c.add(0, 0, "012345678901234567890", false);
    let g3 = c.add(4, 0, "q.so", false);
    let g4 = c.add(0, 2, "q.so", false);
    let ok = c.add(0, 0, "q.so", false);
    let dup = c.add(0, 0, "q.so", false);
    let dot = c.resolve("..");
    let empty = c.resolve("");
    let mut d = Fs::new();
    let mut i = 0u32;
    while (i as usize) < CAP {
        let _ = d.add(STAGE_DEFAULT, 0, match i {
            0 => "k0.so",
            1 => "k1.so",
            2 => "k2.so",
            3 => "k3.so",
            4 => "k4.so",
            5 => "k5.so",
            6 => "k6.so",
            _ => "k7.so",
        }, false);
        i += 1;
    }
    let full = d.add(STAGE_DEFAULT, 0, "k8.so", false);
    out[3] = g1 == Err(SP_EINVAL)
        && g2 == Err(SP_EPATH)
        && g3 == Err(SP_EBADSTAGE)
        && g4 == Err(SP_EBADDIR)
        && ok == Ok(1)
        && dup == Ok(1)
        && dot == Err(SP_EINVAL)
        && empty == Err(SP_EINVAL)
        && full == Err(SP_EFULL)
        && d.len() == CAP;

    out
}

/// 运行本卡检查。
pub fn run_unx11368_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F11368");
    let l = build_ledger();

    s.add_dyn(
        "SP1-十四格八允六拒",
        l.len() == SAMPLES && l.ok_count() == 8 && l.err_count() == 6 && l.audit().is_ok(),
        &format!("n={} ok={} err={} audit={:?}", l.len(), l.ok_count(), l.err_count(), l.audit()),
    );

    s.add(
        "SP2-斜杠直开不搜索",
        {
            let mut f = Fs::new();
            let _ = f.add(STAGE_DEFAULT, 0, "libz.so", false);
            let a = f.resolve("libz.so");
            let b = f.resolve("/usr/lib/libz.so");
            let c = f.resolve("./libz.so");
            a.map(|h| h.id) == Ok(1) && b == Err(SP_ESLASH) && c == Err(SP_ESLASH)
        },
        "static",
    );

    s.add(
        "SP3-搜索序首命即停",
        {
            let mut f = Fs::new();
            let _ = f.add(STAGE_ENV, 0, "m.so", false);
            let _ = f.add(STAGE_CACHE, 0, "m.so", false);
            let _ = f.add(STAGE_DEFAULT, 0, "m.so", false);
            let h = f.resolve("m.so");
            match h {
                Ok(v) => v.id == 1 && v.stage == STAGE_ENV,
                Err(_) => false,
            }
        },
        "static",
    );

    s.add(
        "SP4-目录序内先后",
        {
            let mut f = Fs::new();
            let _ = f.add(STAGE_RPATH, 1, "n.so", false);
            let _ = f.add(STAGE_RPATH, 0, "n.so", false);
            let h = f.resolve("n.so");
            match h {
                Ok(v) => v.id == 2 && v.stage == STAGE_RPATH && v.tries >= 1,
                Err(_) => false,
            }
        },
        "static",
    );

    s.add(
        "SP5-缓存陈旧单列",
        {
            let mut f = Fs::new();
            let _ = f.add(STAGE_CACHE, 0, "old.so", true);
            let a = f.resolve("old.so");
            let b = f.resolve("absent.so");
            a == Err(SP_ECACHE) && b == Err(SP_ENOENT) && f.stale_hits() == 1 && f.misses() == 2
        },
        "static",
    );

    s.add(
        "SP6-陈旧条目不阻断后继",
        {
            let mut f = Fs::new();
            let _ = f.add(STAGE_CACHE, 0, "p.so", true);
            let _ = f.add(STAGE_DEFAULT, 0, "p.so", false);
            let h = f.resolve("p.so");
            match h {
                Ok(v) => v.id == 2 && v.stage == STAGE_DEFAULT,
                Err(_) => false,
            }
        },
        "static",
    );

    s.add(
        "SP7-空名与点名被拒",
        {
            let mut f = Fs::new();
            let a = f.resolve("");
            let b = f.resolve(".");
            let c = f.resolve("..");
            let d = f.add(0, 0, "", false);
            a == Err(SP_EINVAL) && b == Err(SP_EINVAL) && c == Err(SP_EINVAL) && d == Err(SP_EINVAL)
        },
        "static",
    );

    s.add(
        "SP8-名字超长被拒",
        {
            let mut f = Fs::new();
            let a = f.add(0, 0, "012345678901234567890", false);
            let b = f.resolve("012345678901234567890");
            a == Err(SP_EPATH) && b == Err(SP_EPATH) && f.is_empty()
        },
        "static",
    );

    s.add(
        "SP9-阶段目录越界",
        {
            let mut f = Fs::new();
            let a = f.add(STAGES as u8, 0, "a.so", false);
            let b = f.add(STAGE_RPATH, 2, "a.so", false);
            let c = f.add(STAGE_RPATH, 1, "a.so", false);
            let d = f.dirs_of(STAGES as u8);
            a == Err(SP_EBADSTAGE)
                && b == Err(SP_EBADDIR)
                && c == Ok(1)
                && d == Err(SP_EBADSTAGE)
        },
        "static",
    );

    s.add(
        "SP10-同键登记幂等",
        {
            let mut f = Fs::new();
            let a = f.add(STAGE_ENV, 1, "same.so", false);
            let b = f.add(STAGE_ENV, 1, "same.so", true);
            let c = f.add(STAGE_ENV, 0, "same.so", false);
            a == Ok(1) && b == Ok(1) && c == Ok(2) && f.len() == 2
        },
        "static",
    );

    s.add(
        "SP11-账本槽序不可越",
        {
            let mut x = Ledger::new();
            let a = x.register(cell(1, OP_ADD, Ok(1)));
            let b = x.register(cell(0, OP_ADD, Ok(1)));
            let c = x.register(cell(0, OP_ADD, Ok(1)));
            a == Err(SP_EORDER) && b == Ok(()) && c == Err(SP_EORDER) && x.len() == 1
        },
        "static",
    );

    s.add(
        "SP12-观测面边界",
        {
            let x = build_ledger();
            let a = x.sample(SAMPLES - 1);
            let b = x.sample(SAMPLES);
            let mut f = Fs::new();
            let _ = f.add(STAGE_ENV, 0, "obs.so", false);
            let c = f.stage_of(0);
            let d = f.stage_of((CAP + 1) as u32);
            a.is_some() && b.is_none() && c == Err(SP_ENOENT) && d == Err(SP_ENOENT)
        },
        "static",
    );

    s.add(
        "SP13-冻结重放四注入",
        {
            let mut f = build_ledger();
            let rep = f.replay();
            let one = f.freeze();
            let two = f.freeze();
            let inj = inject_four();
            rep.is_ok()
                && one.is_ok()
                && one == rep
                && two.is_err()
                && f.is_frozen()
                && f.rejects() == 1
                && f.root() != 0
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
        let s = run_unx11368_checks();
        assert_eq!(s.len(), 13);
        assert!(s.all_passed());
    }

    #[test]
    fn slash_bypasses_search() {
        let mut f = Fs::new();
        let _ = f.add(STAGE_DEFAULT, 0, "libq.so", false);
        assert_eq!(f.resolve("libq.so").map(|h| h.id), Ok(1));
        assert_eq!(f.resolve("/usr/lib/libq.so"), Err(SP_ESLASH));
        assert_eq!(f.resolve("dir/libq.so"), Err(SP_ESLASH));
        assert_eq!(f.misses(), 2);
    }

    #[test]
    fn first_stage_wins() {
        let mut f = Fs::new();
        let _ = f.add(STAGE_DEFAULT, 0, "w.so", false);
        let _ = f.add(STAGE_RPATH, 0, "w.so", false);
        let h = f.resolve("w.so");
        assert!(h.is_ok());
        assert_eq!(h.map(|v| v.stage), Ok(STAGE_RPATH));
        assert_eq!(h.map(|v| v.id), Ok(2));
    }

    #[test]
    fn stale_cache_is_distinct() {
        let mut f = Fs::new();
        let _ = f.add(STAGE_CACHE, 0, "gone.so", true);
        assert_eq!(f.resolve("gone.so"), Err(SP_ECACHE));
        assert_eq!(f.resolve("never.so"), Err(SP_ENOENT));
        assert_eq!(f.stale_hits(), 1);
        assert_eq!(f.misses(), 2);
    }

    #[test]
    fn gate_order_is_fixed() {
        let mut f = Fs::new();
        assert_eq!(f.add(0, 0, "", false), Err(SP_EINVAL));
        assert_eq!(f.add(0, 0, "012345678901234567890", false), Err(SP_EPATH));
        assert_eq!(f.add(4, 0, "x.so", false), Err(SP_EBADSTAGE));
        assert_eq!(f.add(0, 2, "x.so", false), Err(SP_EBADDIR));
    }

    #[test]
    fn capacity_is_exact() {
        let mut f = Fs::new();
        let _ = f.add(STAGE_DEFAULT, 0, "a.so", false);
        let _ = f.add(STAGE_DEFAULT, 0, "b.so", false);
        let _ = f.add(STAGE_DEFAULT, 0, "c.so", false);
        let _ = f.add(STAGE_DEFAULT, 0, "d.so", false);
        let _ = f.add(STAGE_DEFAULT, 0, "e.so", false);
        let _ = f.add(STAGE_DEFAULT, 0, "f.so", false);
        let _ = f.add(STAGE_DEFAULT, 0, "g.so", false);
        let _ = f.add(STAGE_DEFAULT, 0, "h.so", false);
        assert_eq!(f.len(), CAP);
        assert_eq!(f.add(STAGE_DEFAULT, 0, "i.so", false), Err(SP_EFULL));
        assert_eq!(f.resolve("h.so").map(|h| h.id), Ok(8));
    }

    #[test]
    fn duplicate_registration_is_idempotent() {
        let mut f = Fs::new();
        assert_eq!(f.add(STAGE_ENV, 1, "d.so", false), Ok(1));
        assert_eq!(f.add(STAGE_ENV, 1, "d.so", true), Ok(1));
        assert_eq!(f.len(), 1);
        assert_eq!(f.resolve("d.so").map(|h| h.stage), Ok(STAGE_ENV));
    }
}
