//! UNX-F11333 atexit 容量与失败路径（0xead0 段）
//!
//! `atexit` 在正常路径上只有一个返回值（0 成功、非 0 失败），这个签名把
//! 「为什么失败」彻底抹掉了。对内核侧的实现者来说，这恰恰是最难排查的一类
//! 缺陷：进程退出时回调没跑，调用方只看到一次非零返回，既分不清是「函数号
//! 非法」「重复登记」「表满」还是「退出流程已经关闭登记口」，也拿不到「是哪
//! 一个函数号被拒」。
//!
//! 本模块把**返回码**（闸门身份）与**失败日志**（闸门针对的目标）分开记账：
//!
//! - `attempt` 的 `Result` 只回一个错误码，错误码对应唯一一条闸门；
//! - 每次拒绝同时向定容失败日志追加一条 `(cause, target)`，日志可按下标
//!   取回，越界取日志本身也有独立错误码；
//! - `cause_count` 让调用方能在退出后复盘「这一轮到底被哪条闸门挡了几次」。
//!
//! 之所以让**零号函数闸门排在最前**：函数号 0 是「空指针」的替身，它无论
//! 在什么状态下都不该被解释成「重复」或「表满」。同理，登记口关闭后仍然优
//! 先报零号，是为了让「参数错误」永远盖过「状态错误」——状态会随退出流程
//! 变化，参数错误不会，先钉参数能让诊断结论稳定可复现。
//!
//! 闸门次序（`attempt`）：
//! 1. 函数号为零 → `AC_EFNID`
//! 2. 登记口已关闭 → `AC_ECLOSED`
//! 3. 同号重复 → `AC_EDUP`
//! 4. 容量已满 → `AC_EFULL`
//!
//! 注意第 3 条排在第 4 条之前：表满时对已登记的函数号再登记，正确答案是
//! 「重复」而不是「满」——因为即使腾出一格，这次调用依然不该成功。这个次
//! 序在四路注入里被单独验证。
//!
//! `drain`（退出时的实际执行入口）只跑一次：第一次返回被消耗的回调数，第
//! 二次返回 `AC_EDRAIN`。`drain` 之后登记口关闭，`attempt` 一律
//! `AC_ECLOSED`；此时容量计数已经归零，若把「满」当成结论就会误判。

use crate::alloc::format;
use crate::checks::CheckSet;

/// 段基址。
pub const SEG_BASE: u32 = 0xead0;

/// 函数号为零。
pub const AC_EFNID: u32 = SEG_BASE + 0x01;
/// 同号重复登记。
pub const AC_EDUP: u32 = SEG_BASE + 0x02;
/// 回调表已满。
pub const AC_EFULL: u32 = SEG_BASE + 0x03;
/// 登记口已关闭。
pub const AC_ECLOSED: u32 = SEG_BASE + 0x04;
/// 重复消耗。
pub const AC_EDRAIN: u32 = SEG_BASE + 0x05;
/// 失败日志下标越界。
pub const AC_ENOFAIL: u32 = SEG_BASE + 0x06;
/// 账本失衡。
pub const AC_EBAL: u32 = SEG_BASE + 0x07;
/// 槽位乱序。
pub const AC_EORDER: u32 = SEG_BASE + 0x08;
/// 观测值与复算不符。
pub const AC_EMISMATCH: u32 = SEG_BASE + 0x09;
/// 冻结后重放不一致。
pub const AC_ESEAL: u32 = SEG_BASE + 0x0a;
/// 观测下标越界。
pub const AC_ENOBS: u32 = SEG_BASE + 0x0b;
/// 槽位下标越界。
pub const AC_EBOUND: u32 = SEG_BASE + 0x0c;
/// 尚未消耗就取结果。
pub const AC_EWARM: u32 = SEG_BASE + 0x0d;

/// 回调槽容量。
pub const CAP: usize = 6;
/// 失败日志容量。
pub const FAILCAP: usize = 8;
/// 账本格数。
pub const SAMPLES: usize = 14;

/// 槽位空闲。
pub const SLOT_EMPTY: u8 = 0;
/// 槽位已登记。
pub const SLOT_LIVE: u8 = 1;
/// 槽位已被消耗。
pub const SLOT_DRAINED: u8 = 2;

/// 失败原因：函数号非法。
pub const FC_BADFN: u8 = 1;
/// 失败原因：重复登记。
pub const FC_DUP: u8 = 2;
/// 失败原因：表满。
pub const FC_FULL: u8 = 3;
/// 失败原因：登记口关闭。
pub const FC_CLOSED: u8 = 4;

/// 操作码：登记。
pub const OP_REG: u8 = 1;
/// 操作码：消耗。
pub const OP_DRAIN: u8 = 2;
/// 操作码：读余量。
pub const OP_LEFT: u8 = 3;
/// 操作码：读失败原因。
pub const OP_CAUSE: u8 = 4;

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

/// 一个回调槽。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Entry {
    /// 回调函数号。
    pub fn_id: u32,
    /// 槽位状态。
    pub state: u8,
}

impl Entry {
    /// 空槽。
    pub const fn blank() -> Entry {
        Entry { fn_id: 0, state: SLOT_EMPTY }
    }

    /// 是否仍可执行。
    pub fn is_live(&self) -> bool {
        self.state == SLOT_LIVE
    }
}

/// 一条失败记录。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Fail {
    /// 失败原因码。
    pub cause: u8,
    /// 被拒的函数号。
    pub target: u32,
}

impl Fail {
    /// 空记录。
    pub const fn blank() -> Fail {
        Fail { cause: 0, target: 0 }
    }
}

/// 退出回调登记表。
///
/// 设计要点：`n` 记录「曾经占用的最高槽数」，`live` 记录当前仍待执行的回调
/// 数。两者分离，是因为消耗只改 `live` 不改 `n`——槽位不被复用，序号因此
/// 稳定，登记返回值可以直接当作「第几件回调」使用。
#[derive(Clone, Copy, Debug)]
pub struct Registry {
    e: [Entry; CAP],
    n: usize,
    live: usize,
    closed: bool,
    drained: bool,
    fails: [Fail; FAILCAP],
    nf: usize,
}

impl Registry {
    /// 空表。
    pub const fn new() -> Registry {
        Registry {
            e: [Entry::blank(); CAP],
            n: 0,
            live: 0,
            closed: false,
            drained: false,
            fails: [Fail::blank(); FAILCAP],
            nf: 0,
        }
    }

    /// 表容量。
    pub fn capacity(&self) -> usize {
        CAP
    }

    /// 仍待执行的回调数。
    pub fn live(&self) -> usize {
        self.live
    }

    /// 剩余可登记数。
    pub fn left(&self) -> usize {
        CAP - self.live
    }

    /// 登记口是否关闭。
    pub fn is_closed(&self) -> bool {
        self.closed
    }

    /// 是否已经消耗过。
    pub fn is_drained(&self) -> bool {
        self.drained
    }

    /// 已占用的槽数（含已消耗）。
    pub fn occupied(&self) -> usize {
        self.n
    }

    /// 第 i 个槽。
    pub fn entry_at(&self, i: usize) -> Option<Entry> {
        if i >= self.n {
            return None;
        }
        self.e.get(i).copied()
    }

    /// 记一条失败。
    fn note(&mut self, cause: u8, target: u32) {
        if self.nf < FAILCAP {
            if let Some(slot) = self.fails.get_mut(self.nf) {
                *slot = Fail { cause, target };
            }
            self.nf += 1;
        }
    }

    /// 失败记录条数。
    pub fn fail_count(&self) -> usize {
        self.nf
    }

    /// 第 i 条失败原因。
    ///
    /// 返回 `u32` 而不是 `u8`，是为了和账本样本的 `value` 直接对齐；这让
    /// 「失败路径可观测」这条判据能落在同一套数值比较上。
    pub fn fail_kind(&self, i: usize) -> Result<u32, u32> {
        if i >= self.nf {
            return Err(AC_ENOFAIL);
        }
        match self.fails.get(i) {
            Some(f) => Ok(f.cause as u32),
            None => Err(AC_ENOFAIL),
        }
    }

    /// 第 i 条失败记录的目标函数号。
    pub fn fail_target(&self, i: usize) -> Result<u32, u32> {
        if i >= self.nf {
            return Err(AC_ENOFAIL);
        }
        match self.fails.get(i) {
            Some(f) => Ok(f.target),
            None => Err(AC_ENOFAIL),
        }
    }

    /// 某个原因出现的次数。
    pub fn cause_count(&self, cause: u8) -> u32 {
        let mut k = 0u32;
        let mut i = 0usize;
        while i < self.nf {
            if let Some(f) = self.fails.get(i) {
                if f.cause == cause {
                    k = k.saturating_add(1);
                }
            }
            i += 1;
        }
        k
    }

    /// 按函数号找槽位下标（包含已消耗槽：同一号永远只占一格）。
    fn slot_of(&self, fn_id: u32) -> Option<usize> {
        let mut i = 0usize;
        while i < self.n {
            match self.e.get(i) {
                Some(en) => {
                    if en.fn_id == fn_id {
                        return Some(i);
                    }
                }
                None => return None,
            }
            i += 1;
        }
        None
    }

    /// 登记一个退出回调。
    ///
    /// 成功返回「注册序号」（从 1 起），失败返回闸门错误码并记一条失败日志。
    pub fn attempt(&mut self, fn_id: u32) -> Result<u32, u32> {
        if fn_id == 0 {
            self.note(FC_BADFN, fn_id);
            return Err(AC_EFNID);
        }
        if self.closed {
            self.note(FC_CLOSED, fn_id);
            return Err(AC_ECLOSED);
        }
        if self.slot_of(fn_id).is_some() {
            self.note(FC_DUP, fn_id);
            return Err(AC_EDUP);
        }
        if self.live >= CAP {
            self.note(FC_FULL, fn_id);
            return Err(AC_EFULL);
        }
        let mut i = 0usize;
        while i < CAP {
            let empty = match self.e.get(i) {
                Some(en) => en.state == SLOT_EMPTY,
                None => false,
            };
            if empty {
                if let Some(slot) = self.e.get_mut(i) {
                    *slot = Entry { fn_id, state: SLOT_LIVE };
                }
                self.n += 1;
                self.live += 1;
                return Ok((i as u32) + 1);
            }
            i += 1;
        }
        // 逻辑上不可达：`live < CAP` 且槽位不复用，必存在空槽。
        self.note(FC_FULL, fn_id);
        Err(AC_EFULL)
    }

    /// 消耗全部待执行回调，返回消耗件数。
    ///
    /// 只允许一次：退出流程不可重入。第二次调用被单独的错误码拒绝，而不是
    /// 复用「已关闭」——「重复消耗」和「关闭后登记」是两类不同的调用事故，
    /// 混用错误码会让复用调用栈的复盘工具把两条路径并成一条。
    pub fn drain(&mut self) -> Result<usize, u32> {
        if self.drained {
            return Err(AC_EDRAIN);
        }
        let mut k = 0usize;
        let mut i = 0usize;
        while i < self.n {
            let live = match self.e.get(i) {
                Some(en) => en.is_live(),
                None => false,
            };
            if live {
                if let Some(slot) = self.e.get_mut(i) {
                    slot.state = SLOT_DRAINED;
                }
                k += 1;
            }
            i += 1;
        }
        self.live = 0;
        self.drained = true;
        self.closed = true;
        Ok(k)
    }

    /// 已消耗槽位上的函数号（用于复盘执行序）。
    pub fn drained_at(&self, i: usize) -> Result<u32, u32> {
        if i >= self.n {
            return Err(AC_ENOBS);
        }
        match self.e.get(i) {
            Some(en) => {
                if en.state != SLOT_DRAINED {
                    return Err(AC_EWARM);
                }
                Ok(en.fn_id)
            }
            None => Err(AC_ENOBS),
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
            return Err(AC_EORDER);
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

    /// 对账：形状、顺序、成功值与失败码逐格钉死。
    pub fn audit(&self) -> Result<(), u32> {
        if self.len != SAMPLES || self.ok_count() != 8 || self.err_count() != 6 {
            return Err(AC_EBAL);
        }
        const VALS: [u32; 8] = [1, 2, 3, 3, 4, 5, 6, 6];
        const CODES: [u32; 6] = [
            AC_EFNID,
            AC_EDUP,
            AC_EFULL,
            AC_EDRAIN,
            AC_ECLOSED,
            AC_ENOFAIL,
        ];
        let mut vi = 0usize;
        let mut ci = 0usize;
        let mut i = 0usize;
        while i < self.len {
            let s = match self.rows.get(i) {
                Some(v) => *v,
                None => return Err(AC_EBAL),
            };
            if s.slot as usize != i {
                return Err(AC_EORDER);
            }
            if s.ok {
                if s.code != 0 {
                    return Err(AC_EMISMATCH);
                }
                match VALS.get(vi) {
                    Some(w) => {
                        if s.value != *w {
                            return Err(AC_EMISMATCH);
                        }
                    }
                    None => return Err(AC_EBAL),
                }
                vi += 1;
            } else {
                match CODES.get(ci) {
                    Some(w) => {
                        if s.code != *w {
                            return Err(AC_EMISMATCH);
                        }
                    }
                    None => return Err(AC_EBAL),
                }
                ci += 1;
            }
            i += 1;
        }
        if vi != 8 || ci != 6 {
            return Err(AC_EBAL);
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
            return Err(AC_EBAL);
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
            Err(AC_ESEAL)
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

/// 十四格账本：三次登记、一次读余量、满表、消耗、关闭后拒绝、失败路径观测。
pub fn build_ledger() -> Ledger {
    let mut l = Ledger::new();
    let mut r = Registry::new();

    let _ = l.register(word_cell(0, OP_REG, r.attempt(11)));
    let _ = l.register(word_cell(1, OP_REG, r.attempt(12)));
    let _ = l.register(word_cell(2, OP_REG, r.attempt(0)));
    let _ = l.register(word_cell(3, OP_REG, r.attempt(11)));
    let _ = l.register(word_cell(4, OP_REG, r.attempt(13)));
    let _ = l.register(cell(5, OP_LEFT, Ok(r.left())));
    let _ = l.register(word_cell(6, OP_REG, r.attempt(14)));
    let _ = l.register(word_cell(7, OP_REG, r.attempt(15)));
    let _ = l.register(word_cell(8, OP_REG, r.attempt(16)));
    let _ = l.register(word_cell(9, OP_REG, r.attempt(17)));
    let _ = l.register(cell(10, OP_DRAIN, r.drain()));
    let _ = l.register(cell(11, OP_DRAIN, r.drain()));
    let _ = l.register(word_cell(12, OP_REG, r.attempt(18)));
    let _ = l.register(word_cell(13, OP_CAUSE, r.fail_kind(9)));

    l
}

/// 四路注入。
///
/// 一路查账本；另外三路分别钉住「容量耗尽」「关闭闸门压过重复与满」「重复
/// 闸门压过满」——这三条次序如果写反，账本本身仍然会是八允六拒，只有注入
/// 能把它抓出来。
pub fn inject_four() -> [bool; 4] {
    let mut out = [false; 4];

    let l = build_ledger();
    let rep = l.replay();
    let root = l.seal();
    out[0] = l.len() == SAMPLES && l.audit().is_ok() && rep == Ok(root) && root != 0;

    // 一路：容量精确耗尽。
    let mut a = Registry::new();
    let mut i = 1u32;
    let mut serial_ok = true;
    while (i as usize) <= CAP {
        match a.attempt(i) {
            Ok(serial) => {
                if serial != i {
                    serial_ok = false;
                }
            }
            Err(_) => serial_ok = false,
        }
        i += 1;
    }
    let full = a.attempt(0x40);
    out[1] = serial_ok
        && a.live() == CAP
        && a.left() == 0
        && full == Err(AC_EFULL)
        && a.fail_kind(a.fail_count().saturating_sub(1)) == Ok(FC_FULL as u32)
        && a.cause_count(FC_FULL) == 1;

    // 二路：关闭闸门压过重复与满；零号闸门仍压过关闭。
    let mut b = Registry::new();
    let _ = b.attempt(1);
    let _ = b.drain();
    let closed_dup = b.attempt(1);
    let closed_new = b.attempt(9);
    let zero_closed = b.attempt(0);
    out[2] = closed_dup == Err(AC_ECLOSED)
        && closed_new == Err(AC_ECLOSED)
        && zero_closed == Err(AC_EFNID)
        && b.drain() == Err(AC_EDRAIN)
        && b.cause_count(FC_CLOSED) == 2
        && b.cause_count(FC_BADFN) == 1
        && b.fail_count() == 3;

    // 三路：满表时重复闸门先于容量闸门。
    let mut c = Registry::new();
    let mut j = 1u32;
    while (j as usize) <= CAP {
        let _ = c.attempt(j);
        j += 1;
    }
    let dup_full = c.attempt(3);
    let new_full = c.attempt(3 + CAP as u32);
    out[3] = dup_full == Err(AC_EDUP)
        && new_full == Err(AC_EFULL)
        && c.cause_count(FC_DUP) == 1
        && c.cause_count(FC_FULL) == 1
        && c.live() == CAP;

    out
}

/// 十三项判据。
pub fn run_unx11333_checks() -> CheckSet {
    let mut s = CheckSet::new("unx11333.atexit_capacity");
    let l = build_ledger();

    s.add_dyn(
        "AC1-十四格八允六拒",
        l.len() == SAMPLES && l.ok_count() == 8 && l.err_count() == 6 && l.audit().is_ok(),
        &format!("n={} ok={} err={}", l.len(), l.ok_count(), l.err_count()),
    );

    s.add(
        "AC2-登记序号递增",
        l.sample(0).map(|x| x.value) == Some(1) && l.sample(1).map(|x| x.value) == Some(2),
        "static",
    );

    s.add(
        "AC3-零号函数被拒",
        l.sample(2).map(|x| x.code) == Some(AC_EFNID),
        "static",
    );

    s.add(
        "AC4-重号登记被拒",
        l.sample(3).map(|x| x.code) == Some(AC_EDUP),
        "static",
    );

    s.add(
        "AC5-余量随登记递减",
        l.sample(4).map(|x| x.value) == Some(3)
            && l.sample(5).map(|x| x.value) == Some(3),
        "static",
    );

    s.add(
        "AC6-满表后拒绝新号",
        l.sample(9).map(|x| x.code) == Some(AC_EFULL),
        "static",
    );

    s.add(
        "AC7-消耗计数等于登记数",
        l.sample(10).map(|x| x.value) == Some(6),
        "static",
    );

    s.add(
        "AC8-二次消耗被拒",
        l.sample(11).map(|x| x.code) == Some(AC_EDRAIN),
        "static",
    );

    s.add(
        "AC9-关闭闸门压过满闸门",
        l.sample(12).map(|x| x.code) == Some(AC_ECLOSED),
        "static",
    );

    s.add(
        "AC10-失败日志越界有独立错误码",
        l.sample(13).map(|x| x.code) == Some(AC_ENOFAIL),
        "static",
    );

    s.add(
        "AC11-失败路径逐条可观测",
        {
            let mut r = Registry::new();
            let _ = r.attempt(0);
            let _ = r.attempt(5);
            let _ = r.attempt(5);
            r.fail_count() == 2
                && r.fail_kind(0) == Ok(FC_BADFN as u32)
                && r.fail_kind(1) == Ok(FC_DUP as u32)
                && r.fail_target(0) == Ok(0)
                && r.fail_target(1) == Ok(5)
                && r.fail_kind(2) == Err(AC_ENOFAIL)
                && r.cause_count(FC_BADFN) == 1
                && r.cause_count(FC_FULL) == 0
        },
        "static",
    );

    s.add(
        "AC12-容量边界独立复算",
        {
            let mut r = Registry::new();
            let mut i = 1u32;
            let mut ok = true;
            while (i as usize) <= CAP {
                if r.attempt(i) != Ok(i) {
                    ok = false;
                }
                i += 1;
            }
            let before = r.left();
            let rejected = r.attempt(100);
            ok && before == 0
                && rejected == Err(AC_EFULL)
                && r.occupied() == CAP
                && r.entry_at(CAP).is_none()
                && r.entry_at(CAP - 1).map(|e| e.fn_id) == Some(CAP as u32)
        },
        "static",
    );

    s.add(
        "AC13-冻结重放四注入",
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
        let s = run_unx11333_checks();
        assert_eq!(s.len(), 13);
        assert!(s.all_passed());
    }

    #[test]
    fn capacity_is_exact() {
        let mut r = Registry::new();
        let mut i = 1u32;
        while (i as usize) <= CAP {
            assert_eq!(r.attempt(i), Ok(i));
            i += 1;
        }
        assert_eq!(r.left(), 0);
        assert_eq!(r.attempt(CAP as u32 + 1), Err(AC_EFULL));
        assert_eq!(r.attempt(1), Err(AC_EDUP));
    }

    #[test]
    fn zero_function_wins() {
        let mut r = Registry::new();
        assert_eq!(r.attempt(0), Err(AC_EFNID));
        let _ = r.drain();
        assert_eq!(r.attempt(0), Err(AC_EFNID));
        assert_eq!(r.attempt(7), Err(AC_ECLOSED));
    }

    #[test]
    fn drain_once() {
        let mut r = Registry::new();
        assert_eq!(r.attempt(3), Ok(1));
        assert_eq!(r.attempt(9), Ok(2));
        assert_eq!(r.drain(), Ok(2));
        assert!(r.is_drained());
        assert!(r.is_closed());
        assert_eq!(r.live(), 0);
        assert_eq!(r.drain(), Err(AC_EDRAIN));
        assert_eq!(r.drained_at(0), Ok(3));
        assert_eq!(r.drained_at(1), Ok(9));
        assert_eq!(r.drained_at(2), Err(AC_ENOBS));
    }

    #[test]
    fn failure_log_is_ordered() {
        let mut r = Registry::new();
        let _ = r.attempt(0);
        let _ = r.attempt(4);
        let _ = r.attempt(4);
        assert_eq!(r.fail_count(), 2);
        assert_eq!(r.fail_kind(0), Ok(FC_BADFN as u32));
        assert_eq!(r.fail_kind(1), Ok(FC_DUP as u32));
        assert_eq!(r.fail_target(1), Ok(4));
        assert_eq!(r.fail_kind(2), Err(AC_ENOFAIL));
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
