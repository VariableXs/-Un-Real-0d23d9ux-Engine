//! UNX-F11375 执行器沙盒（cwd 预置文件态构造引擎）
//!
//! 判据执行前要在 `cwd` 里铺出预置文件态，跑完再把现场清退干净。三段式
//! 沙盒把「铺 / 跑 / 拆」分成三个互斥相位，任何跨相位操作都拒收：
//!
//! 1. **预置闸门**：铺文件只允许在未开跑时进行；文件号非法、体积超限、
//!    重复铺都拒收。预置态必须是开跑前就完整的。
//! 2. **隔离闸门**：一次只允许一场运行。开跑后不许再铺文件，触碰不存在的
//!    路径直接 `EMISS`——隔离面里没有的东西不许碰。
//! 3. **清退闸门**：还有脏文件时不许拆场；拆场返回本轮移除的文件数，
//!    清退后现场必须是零残留。
//!
//! 闸门次序：
//! - `stage`：文件号越界 → 体积超限 → 重复 → 未在运行 → 文件表满；
//! - `begin`：空预置态 → 已在运行；
//! - `touch` / `restore`：未在运行 → 路径不存在；
//! - `teardown`：未在运行 → 仍有脏文件。

use crate::alloc::format;
use crate::checks::CheckSet;

/// 段基址。
pub const SEG_BASE: u32 = 0xed70;

/// 文件号越界。
pub const SB_ERANGE: u32 = SEG_BASE + 0x01;
/// 预置体积超限。
pub const SB_ESIZE: u32 = SEG_BASE + 0x02;
/// 同号文件重复预置。
pub const SB_EDUP: u32 = SEG_BASE + 0x03;
/// 文件表已满。
pub const SB_EFULL: u32 = SEG_BASE + 0x04;
/// 空预置态不许开跑。
pub const SB_EEMPTY: u32 = SEG_BASE + 0x05;
/// 相位错乱。
pub const SB_EPHASE: u32 = SEG_BASE + 0x06;
/// 隔离面内无此路径。
pub const SB_EMISS: u32 = SEG_BASE + 0x07;
/// 拆场仍有残留。
pub const SB_ERESID: u32 = SEG_BASE + 0x08;
/// 账本形状不符。
pub const SB_EBAL: u32 = SEG_BASE + 0x09;
/// 槽位乱序。
pub const SB_ESLOT: u32 = SEG_BASE + 0x0a;
/// 观测值与复算不符。
pub const SB_EVALUE: u32 = SEG_BASE + 0x0b;
/// 冻结后重放不一致。
pub const SB_ESEAL: u32 = SEG_BASE + 0x0c;
/// 观察下标越界。
pub const SB_ENOBS: u32 = SEG_BASE + 0x0d;

/// 文件表容量。
pub const CAP: usize = 6;
/// 账本格数。
pub const SAMPLES: usize = 14;
/// 文件号上界。
pub const ID_MAX: u32 = 64;
/// 单文件体积上界。
pub const SIZE_MAX: u32 = 32;

/// 操作码：预置文件。
pub const OP_STAGE: u8 = 1;
/// 操作码：开跑。
pub const OP_BEGIN: u8 = 2;
/// 操作码：触碰文件。
pub const OP_TOUCH: u8 = 3;
/// 操作码：还原文件。
pub const OP_RESTORE: u8 = 4;
/// 操作码：拆场。
pub const OP_TEARDOWN: u8 = 5;

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

/// 预置态里的一个文件。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct File {
    /// 文件号。
    pub id: u32,
    /// 体积。
    pub size: u32,
    /// 是否被本轮运行改脏。
    pub dirty: bool,
}

/// 三段式沙盒。
#[derive(Clone, Copy, Debug)]
pub struct Sand {
    f: [File; CAP],
    n: usize,
    run: u32,
    active: bool,
    dirty: u32,
    removed: u32,
}

impl Sand {
    /// 空沙盒。
    pub const fn new() -> Sand {
        Sand {
            f: [File { id: 0, size: 0, dirty: false }; CAP],
            n: 0,
            run: 0,
            active: false,
            dirty: 0,
            removed: 0,
        }
    }

    /// 预置文件数。
    pub fn count(&self) -> usize {
        self.n
    }

    /// 运行场次累计。
    pub fn run_count(&self) -> u32 {
        self.run
    }

    /// 是否正在运行。
    pub fn is_active(&self) -> bool {
        self.active
    }

    /// 脏文件数。
    pub fn dirty_count(&self) -> u32 {
        self.dirty
    }

    /// 累计移除文件数。
    pub fn removed(&self) -> u32 {
        self.removed
    }

    /// 第 i 个预置文件。
    pub fn at(&self, i: usize) -> Option<File> {
        if i >= self.n {
            return None;
        }
        self.f.get(i).copied()
    }

    fn find(&self, id: u32) -> Option<usize> {
        let mut i = 0usize;
        while i < self.n {
            match self.f.get(i) {
                Some(e) => {
                    if e.id == id {
                        return Some(i);
                    }
                }
                None => return None,
            }
            i += 1;
        }
        None
    }

    /// 铺一个预置文件，返回文件数。
    pub fn stage(&mut self, id: u32, size: u32) -> Result<usize, u32> {
        if id == 0 || id > ID_MAX {
            return Err(SB_ERANGE);
        }
        if size > SIZE_MAX {
            return Err(SB_ESIZE);
        }
        if self.find(id).is_some() {
            return Err(SB_EDUP);
        }
        if self.active {
            return Err(SB_EPHASE);
        }
        if self.n >= CAP {
            return Err(SB_EFULL);
        }
        if let Some(slot) = self.f.get_mut(self.n) {
            *slot = File { id, size, dirty: false };
        }
        self.n += 1;
        Ok(self.n)
    }

    /// 开跑：空预置态不许开，已有运行不许并发。
    pub fn begin(&mut self) -> Result<u32, u32> {
        if self.n == 0 {
            return Err(SB_EEMPTY);
        }
        if self.active {
            return Err(SB_EPHASE);
        }
        self.active = true;
        self.run = self.run.saturating_add(1);
        Ok(self.run)
    }

    /// 运行中触碰一个预置文件，返回脏文件数。
    pub fn touch(&mut self, id: u32) -> Result<u32, u32> {
        if !self.active {
            return Err(SB_EPHASE);
        }
        let at = match self.find(id) {
            Some(v) => v,
            None => return Err(SB_EMISS),
        };
        match self.f.get(at) {
            Some(e) => {
                if !e.dirty {
                    if let Some(slot) = self.f.get_mut(at) {
                        slot.dirty = true;
                    }
                    self.dirty = self.dirty.saturating_add(1);
                }
            }
            None => return Err(SB_EMISS),
        }
        Ok(self.dirty)
    }

    /// 还原一个预置文件，返回剩余脏文件数。
    pub fn restore(&mut self, id: u32) -> Result<u32, u32> {
        if !self.active {
            return Err(SB_EPHASE);
        }
        let at = match self.find(id) {
            Some(v) => v,
            None => return Err(SB_EMISS),
        };
        match self.f.get(at) {
            Some(e) => {
                if e.dirty {
                    if let Some(slot) = self.f.get_mut(at) {
                        slot.dirty = false;
                    }
                    self.dirty -= 1;
                }
            }
            None => return Err(SB_EMISS),
        }
        Ok(self.dirty)
    }

    /// 拆场：必须零脏文件；返回本轮移除数。
    pub fn teardown(&mut self) -> Result<u32, u32> {
        if !self.active {
            return Err(SB_EPHASE);
        }
        if self.dirty != 0 {
            return Err(SB_ERESID);
        }
        let r = self.n as u32;
        self.n = 0;
        self.active = false;
        self.removed = self.removed.saturating_add(r);
        Ok(r)
    }

    /// 零残留自检：无文件、无运行、无脏。
    pub fn clean(&self) -> Result<u32, u32> {
        if self.n == 0 && !self.active && self.dirty == 0 {
            Ok(self.removed)
        } else {
            Err(SB_ERESID)
        }
    }

    /// 指纹。
    pub fn seal(&self) -> u64 {
        let mut h = FNV_OFF;
        let mut i = 0usize;
        while i < self.n {
            if let Some(e) = self.f.get(i) {
                h = fold(h, e.id as u64);
                h = fold(h, e.size as u64);
                h = fold(h, e.dirty as u64);
            }
            i += 1;
        }
        h = fold(h, self.run as u64);
        fold(h, self.removed as u64)
    }

    /// 不变式自检。
    pub fn consistent(&self) -> bool {
        if self.n > CAP || self.dirty > self.n as u32 {
            return false;
        }
        if !self.active && self.dirty != 0 {
            return false;
        }
        let mut dirty = 0u32;
        let mut i = 0usize;
        while i < self.n {
            let e = match self.f.get(i) {
                Some(v) => *v,
                None => return false,
            };
            if e.id == 0 || e.id > ID_MAX || e.size > SIZE_MAX {
                return false;
            }
            if e.dirty {
                dirty = dirty.saturating_add(1);
            }
            let mut j = 0usize;
            while j < i {
                match self.f.get(j) {
                    Some(p) => {
                        if p.id == e.id {
                            return false;
                        }
                    }
                    None => return false,
                }
                j += 1;
            }
            i += 1;
        }
        dirty == self.dirty
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
            return Err(SB_ESLOT);
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
            return Err(SB_EBAL);
        }
        const VALS: [u32; 8] = [1, 2, 1, 1, 2, 1, 0, 2];
        const CODES: [u32; 6] = [SB_EDUP, SB_ERANGE, SB_EPHASE, SB_EMISS, SB_ERESID, SB_EFULL];
        let mut vi = 0usize;
        let mut ci = 0usize;
        let mut i = 0usize;
        while i < self.len {
            let s = match self.rows.get(i) {
                Some(v) => *v,
                None => return Err(SB_EBAL),
            };
            if s.slot as usize != i {
                return Err(SB_ESLOT);
            }
            if s.ok {
                if s.code != 0 {
                    return Err(SB_EVALUE);
                }
                match VALS.get(vi) {
                    Some(w) => {
                        if s.value != *w {
                            return Err(SB_EVALUE);
                        }
                    }
                    None => return Err(SB_EBAL),
                }
                vi += 1;
            } else {
                match CODES.get(ci) {
                    Some(w) => {
                        if s.code != *w {
                            return Err(SB_EVALUE);
                        }
                    }
                    None => return Err(SB_EBAL),
                }
                ci += 1;
            }
            i += 1;
        }
        if vi != 8 || ci != 6 {
            return Err(SB_EBAL);
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
            return Err(SB_EBAL);
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
            Err(SB_ESEAL)
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

/// 十四格账本：铺两件 → 重复/越界 → 开跑 → 运行中铺被拒 → 触碰/还原 →
/// 缺路径 → 带脏拆场被拒 → 清退 → 满表。
pub fn build_ledger() -> Ledger {
    let mut l = Ledger::new();
    let mut s = Sand::new();

    let _ = l.register(cell(0, OP_STAGE, s.stage(1, 4)));
    let _ = l.register(cell(1, OP_STAGE, s.stage(2, 8)));
    let _ = l.register(cell(2, OP_STAGE, s.stage(1, 4)));
    let _ = l.register(cell(3, OP_STAGE, s.stage(0, 4)));
    let _ = l.register(word_cell(4, OP_BEGIN, s.begin()));
    let _ = l.register(cell(5, OP_STAGE, s.stage(3, 4)));
    let _ = l.register(word_cell(6, OP_TOUCH, s.touch(1)));
    let _ = l.register(word_cell(7, OP_TOUCH, s.touch(2)));
    let _ = l.register(word_cell(8, OP_TOUCH, s.touch(9)));
    let _ = l.register(word_cell(9, OP_RESTORE, s.restore(1)));
    let _ = l.register(word_cell(10, OP_TEARDOWN, s.teardown()));
    let _ = l.register(word_cell(11, OP_RESTORE, s.restore(2)));
    let _ = l.register(word_cell(12, OP_TEARDOWN, s.teardown()));

    let mut i = 3u32;
    while (i as usize) < CAP + 3 {
        let _ = s.stage(i, 1);
        i += 1;
    }
    let _ = l.register(cell(13, OP_STAGE, s.stage(9, 1)));

    l
}

/// 四路注入。
pub fn inject_four() -> [bool; 4] {
    let mut out = [false; 4];

    let l = build_ledger();
    let rep = l.replay();
    let root = l.seal();
    out[0] = l.len() == SAMPLES && l.audit().is_ok() && rep == Ok(root) && root != 0;

    // 一路：铺 / 跑 / 拆三段走通，零残留。
    let mut a = Sand::new();
    let s1 = a.stage(1, 4);
    let s2 = a.stage(2, 8);
    let b = a.begin();
    let t = a.touch(1);
    let r = a.restore(1);
    let d = a.teardown();
    out[1] = s1 == Ok(1)
        && s2 == Ok(2)
        && b == Ok(1)
        && t == Ok(1)
        && r == Ok(0)
        && d == Ok(2)
        && a.count() == 0
        && a.is_active() == false
        && a.clean() == Ok(2)
        && a.consistent();

    // 二路：八类失败各自可辨，且失败不改现场。
    let mut b = Sand::new();
    let empty = b.begin() == Err(SB_EEMPTY);
    let range = b.stage(0, 1) == Err(SB_ERANGE);
    let big = b.stage(1, SIZE_MAX + 1) == Err(SB_ESIZE);
    let s1 = b.stage(1, 2) == Ok(1);
    let dup = b.stage(1, 2) == Err(SB_EDUP);
    let no_run = b.touch(1) == Err(SB_EPHASE);
    let h0 = b.seal();
    let run = b.begin() == Ok(1);
    let phase = b.stage(2, 2) == Err(SB_EPHASE);
    let miss = b.touch(9) == Err(SB_EMISS);
    let kept = b.seal() != h0 && b.count() == 1;
    let _ = b.touch(1);
    let resid = b.teardown() == Err(SB_ERESID);
    let _ = b.restore(1);
    let done = b.teardown() == Ok(1);
    let mut c = Sand::new();
    let mut i = 1u32;
    while (i as usize) <= CAP {
        let _ = c.stage(i, 1);
        i += 1;
    }
    let full = c.stage((CAP + 1) as u32, 1) == Err(SB_EFULL);
    out[2] = empty
        && range
        && big
        && s1
        && dup
        && no_run
        && run
        && phase
        && miss
        && kept
        && resid
        && done
        && full
        && b.consistent()
        && c.consistent();

    // 三路：满表硬拒，拆场后名额全部回收。
    let mut q = Sand::new();
    let mut i = 1u32;
    while (i as usize) <= CAP {
        let _ = q.stage(i, i);
        i += 1;
    }
    let q1 = q.count() == CAP;
    let q2 = q.stage((CAP + 1) as u32, 1) == Err(SB_EFULL);
    let q3 = q.begin() == Ok(1);
    let q4 = q.teardown() == Ok(CAP as u32);
    let q5 = q.clean() == Ok(CAP as u32);
    let q6 = q.stage(1, 1) == Ok(1) && q.begin() == Ok(2);
    out[3] = q1 && q2 && q3 && q4 && q5 && q6 && q.consistent();

    out
}

/// 十三项判据。
pub fn run_unx11375_checks() -> CheckSet {
    let mut s = CheckSet::new("unx11375.sandbox_stage");
    let l = build_ledger();

    s.add_dyn(
        "SB1-十四格八允六拒",
        l.len() == SAMPLES && l.ok_count() == 8 && l.err_count() == 6 && l.audit().is_ok(),
        &format!("n={} ok={} err={}", l.len(), l.ok_count(), l.err_count()),
    );

    s.add(
        "SB2-预置态按序铺开",
        {
            let mut a = Sand::new();
            a.stage(1, 4) == Ok(1)
                && a.stage(2, 8) == Ok(2)
                && a.at(1) == Some(File { id: 2, size: 8, dirty: false })
                && a.count() == 2
                && a.clean() == Err(SB_ERESID)
                && a.consistent()
        },
        "static",
    );

    s.add(
        "SB3-空预置不许开跑",
        {
            let mut a = Sand::new();
            a.begin() == Err(SB_EEMPTY)
                && a.run_count() == 0
                && a.is_active() == false
                && a.consistent()
        },
        "static",
    );

    s.add(
        "SB4-运行中禁止再铺",
        {
            let mut a = Sand::new();
            let _ = a.stage(1, 1);
            let _ = a.begin();
            a.stage(2, 1) == Err(SB_EPHASE)
                && a.count() == 1
                && a.consistent()
                && l.sample(5).map(|x| x.code) == Some(SB_EPHASE)
        },
        "static",
    );

    s.add(
        "SB5-隔离面无此路径",
        {
            let mut a = Sand::new();
            let _ = a.stage(1, 1);
            let _ = a.begin();
            let t = a.touch(9) == Err(SB_EMISS);
            let r = a.restore(9) == Err(SB_EMISS);
            t && r && a.dirty_count() == 0 && a.consistent()
        },
        "static",
    );

    s.add(
        "SB6-重铺同号被拒",
        {
            let mut a = Sand::new();
            let _ = a.stage(3, 5);
            a.stage(3, 6) == Err(SB_EDUP)
                && a.count() == 1
                && a.at(0).map(|f| f.size) == Some(5)
                && l.sample(2).map(|x| x.code) == Some(SB_EDUP)
        },
        "static",
    );

    s.add(
        "SB7-文件号越界被拒",
        {
            let mut a = Sand::new();
            a.stage(0, 1) == Err(SB_ERANGE)
                && a.stage(ID_MAX + 1, 1) == Err(SB_ERANGE)
                && a.count() == 0
                && l.sample(3).map(|x| x.code) == Some(SB_ERANGE)
        },
        "static"
    );

    s.add(
        "SB8-体积超限被拒",
        {
            let mut a = Sand::new();
            a.stage(1, SIZE_MAX + 1) == Err(SB_ESIZE)
                && a.stage(1, SIZE_MAX) == Ok(1)
                && a.count() == 1
        },
        "static"
    );

    s.add(
        "SB9-脏现场不许拆",
        {
            let mut a = Sand::new();
            let _ = a.stage(1, 1);
            let _ = a.stage(2, 1);
            let _ = a.begin();
            let _ = a.touch(1);
            let _ = a.touch(2);
            let bad = a.teardown() == Err(SB_ERESID);
            let _ = a.restore(1);
            let still = a.teardown() == Err(SB_ERESID);
            let _ = a.restore(2);
            bad && still && a.teardown() == Ok(2) && a.clean() == Ok(2) && l.sample(10).map(|x| x.code) == Some(SB_ERESID)
        },
        "static"
    );

    s.add(
        "SB10-拆场后零残留",
        {
            let mut a = Sand::new();
            let mut i = 1u32;
            while (i as usize) <= CAP {
                let _ = a.stage(i, i);
                i += 1;
            }
            let _ = a.begin();
            let n = a.teardown();
            n == Ok(CAP as u32)
                && a.count() == 0
                && a.dirty_count() == 0
                && a.is_active() == false
                && a.clean() == Ok(CAP as u32)
                && a.consistent()
        },
        "static"
    );

    s.add(
        "SB11-满表硬上限",
        {
            let mut a = Sand::new();
            let mut i = 1u32;
            while (i as usize) <= CAP {
                let _ = a.stage(i, 1);
                i += 1;
            }
            a.count() == CAP
                && a.stage((CAP + 1) as u32, 1) == Err(SB_EFULL)
                && a.consistent()
                && l.sample(13).map(|x| x.code) == Some(SB_EFULL)
        },
        "static"
    );

    s.add(
        "SB12-触碰计数单调",
        {
            let mut a = Sand::new();
            let _ = a.stage(1, 1);
            let _ = a.stage(2, 1);
            let _ = a.begin();
            let t1 = a.touch(1) == Ok(1);
            let t2 = a.touch(1) == Ok(1);
            let t3 = a.touch(2) == Ok(2);
            let r = a.restore(1) == Ok(1);
            t1 && t2 && t3 && r && a.dirty_count() == 1 && a.consistent()
        },
        "static"
    );

    s.add(
        "SB13-冻结重放四注入",
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
        let mut i = 0usize;
        while i < l.len() {
            assert_eq!(l.sample(i).map(|x| x.slot as usize), Some(i));
            i += 1;
        }
    }

    #[test]
    fn checks_pass() {
        let s = run_unx11375_checks();
        assert_eq!(s.len(), 13);
        assert!(s.all_passed());
    }

    #[test]
    fn three_stages() {
        let mut a = Sand::new();
        assert_eq!(a.stage(1, 4), Ok(1));
        assert_eq!(a.begin(), Ok(1));
        assert_eq!(a.touch(1), Ok(1));
        assert_eq!(a.restore(1), Ok(0));
        assert_eq!(a.teardown(), Ok(1));
        assert_eq!(a.clean(), Ok(1));
    }

    #[test]
    fn phase_is_enforced() {
        let mut a = Sand::new();
        assert_eq!(a.begin(), Err(SB_EEMPTY));
        assert_eq!(a.stage(1, 1), Ok(1));
        assert_eq!(a.begin(), Ok(1));
        assert_eq!(a.begin(), Err(SB_EPHASE));
        assert_eq!(a.stage(2, 1), Err(SB_EPHASE));
        assert_eq!(a.touch(7), Err(SB_EMISS));
    }

    #[test]
    fn residue_blocks_teardown() {
        let mut a = Sand::new();
        assert_eq!(a.stage(1, 1), Ok(1));
        assert_eq!(a.begin(), Ok(1));
        assert_eq!(a.touch(1), Ok(1));
        assert_eq!(a.teardown(), Err(SB_ERESID));
        assert_eq!(a.restore(1), Ok(0));
        assert_eq!(a.teardown(), Ok(1));
    }

    #[test]
    fn capacity_limit() {
        let mut a = Sand::new();
        let mut i = 1u32;
        while (i as usize) <= CAP {
            assert_eq!(a.stage(i, 1), Ok(i as usize));
            i += 1;
        }
        assert_eq!(a.stage((CAP + 1) as u32, 1), Err(SB_EFULL));
        assert!(a.consistent());
    }

    #[test]
    fn inject_four_holds() {
        assert_eq!(inject_four(), [true; 4]);
    }
}
