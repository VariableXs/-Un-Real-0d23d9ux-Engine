//! UNX-F11505 whoami 有效用户名查询（0xf0a0 段）
//!
//! `whoami` 问的不是"我是谁"，而是"我**此刻以谁的身份**在跑"：它查的是会话的
//! 有效 uid，而不是登录时的真实 uid。`logname` / `id -un` 的真实名问法正好相反。
//! 本模块把两个问题拆成两条独立的查询路径，共用同一张 passwd 表：
//!
//! - **有效路径**（`whoami`）：`session.eff` → 表内名字；
//! - **真实路径**（`logname`）：`session.real` → 表内名字。
//!
//! 两条路径的闸门相同：身份位置为空（保留段 `0x8000_0000` 以上，含 -1 哨兵）
//! 返回 `ENOEFF`；位置合法但表里没有这条记录返回 `ENOENT`。两者必须分开报：
//! "没有身份"是会话状态问题，"名字查不到"是表覆盖问题，修法完全不同。
//!
//! passwd 表入账闸门次序：表满 → id 落保留段 → 名字为空或含 ':' → 名字超容 →
//! uid 重复 → 名字重复。':' 是 passwd 行的字段分隔符，名字里出现它等于让
//! 一条记录变成两条，必须拒绝而不是转义。
//!
//! 名字回写先量长度：缓冲放不下返回 `EBUF` 且一格不写。`same()` 回答真实与
//! 有效身份是否同一——su 之后它就从真变假，这正是"两个名"存在的理由。
//!
//! 账本十四格：八格登记成功路径（三次开户、一次按 id 查组、两次有效名查询、
//! 一次真实名、一次按 id 直取名），六格登记各类拒绝（重名、重 uid、非法名、
//! 查无此人、缓冲不足、无有效身份）。审计逐格复算，封印折叠全账，冻结后
//! 拒绝登记并计数。

use crate::alloc::format;
use crate::checks::CheckSet;

/// 段基址。
pub const SEG_BASE: u32 = 0xf0a0;

/// passwd 表满。
pub const WA_EFULL: u32 = SEG_BASE + 0x01;
/// 名字非法（空或含字段分隔符）。
pub const WA_EBADNAME: u32 = SEG_BASE + 0x02;
/// 名字超出容量。
pub const WA_ENAMELONG: u32 = SEG_BASE + 0x03;
/// uid 重复登记。
pub const WA_EDUPUID: u32 = SEG_BASE + 0x04;
/// 名字重复登记。
pub const WA_EDUPNAME: u32 = SEG_BASE + 0x05;
/// 表内查无此 id。
pub const WA_ENOENT: u32 = SEG_BASE + 0x06;
/// 输出缓冲不足。
pub const WA_EBUF: u32 = SEG_BASE + 0x07;
/// 会话没有有效身份。
pub const WA_ENOEFF: u32 = SEG_BASE + 0x08;
/// 账本形状不符。
pub const WA_EBAL: u32 = SEG_BASE + 0x09;
/// 槽位乱序。
pub const WA_ESLOT: u32 = SEG_BASE + 0x0a;
/// 观测值与复算不符。
pub const WA_EVALUE: u32 = SEG_BASE + 0x0b;
/// 冻结后重放不一致。
pub const WA_ESEAL: u32 = SEG_BASE + 0x0c;
/// 观察下标越界。
pub const WA_ENOBS: u32 = SEG_BASE + 0x0d;

/// 表容量。
pub const CAP: usize = 8;
/// 名字容量。
pub const NAME_CAP: usize = 32;
/// 合法 id 上界（含）。
pub const ID_MAX: u32 = 0x7fff_ffff;
/// 无身份哨兵。
pub const NOBODY: u32 = 0xffff_ffff;
/// 账本格数。
pub const SAMPLES: usize = 14;

/// 操作码：开户。
pub const OP_ADD: u8 = 1;
/// 操作码：查记录。
pub const OP_LOOKUP: u8 = 2;
/// 操作码：有效名。
pub const OP_WHOAMI: u8 = 3;
/// 操作码：真实名。
pub const OP_LOGNAME: u8 = 4;
/// 操作码：按 id 直取名。
pub const OP_NAMEOF: u8 = 5;

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

/// 一行 passwd 记录。
#[derive(Clone, Copy, Debug)]
pub struct Passwd {
    name: [u8; NAME_CAP],
    nlen: u8,
    uid: u32,
    gid: u32,
}

impl Passwd {
    const fn blank() -> Passwd {
        Passwd { name: [0u8; NAME_CAP], nlen: 0, uid: 0, gid: 0 }
    }

    /// 名字。
    pub fn name(&self) -> Option<&[u8]> {
        self.name.get(..self.nlen as usize)
    }

    /// 用户 id。
    pub fn uid(&self) -> u32 {
        self.uid
    }

    /// 主组 id。
    pub fn gid(&self) -> u32 {
        self.gid
    }
}

/// 会话的两个身份位置。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Session {
    /// 登录时的真实 uid。
    pub real: u32,
    /// 当前生效的 uid。
    pub eff: u32,
}

impl Session {
    /// 两个位置是否同一身份。
    pub fn same(&self) -> bool {
        self.real == self.eff
    }
}

/// 定容 passwd 表。
pub struct Db {
    rows: [Passwd; CAP],
    n: usize,
}

impl Db {
    /// 空表。
    pub const fn new() -> Db {
        Db { rows: [Passwd::blank(); CAP], n: 0 }
    }

    /// 记录条数。
    pub fn count(&self) -> usize {
        self.n
    }

    /// 第 i 条记录。
    pub fn row_at(&self, i: usize) -> Option<Passwd> {
        if i >= self.n {
            return None;
        }
        self.rows.get(i).copied()
    }

    /// 开户。
    pub fn add(&mut self, name: &[u8], uid: u32, gid: u32) -> Result<u32, u32> {
        if self.n >= CAP {
            return Err(WA_EFULL);
        }
        if uid > ID_MAX || gid > ID_MAX {
            return Err(WA_ENOEFF);
        }
        if name.is_empty() {
            return Err(WA_EBADNAME);
        }
        let mut k = 0usize;
        while k < name.len() {
            if name.get(k).copied() == Some(b':') {
                return Err(WA_EBADNAME);
            }
            k += 1;
        }
        if name.len() > NAME_CAP {
            return Err(WA_ENAMELONG);
        }
        let mut i = 0usize;
        while i < self.n {
            match self.rows.get(i) {
                Some(r) => {
                    if r.uid == uid {
                        return Err(WA_EDUPUID);
                    }
                    if r.nlen as usize == name.len() && r.name.get(..name.len()) == Some(name) {
                        return Err(WA_EDUPNAME);
                    }
                }
                None => return Err(WA_EBAL),
            }
            i += 1;
        }
        if let Some(dst) = self.rows.get_mut(self.n) {
            let mut j = 0usize;
            while j < NAME_CAP {
                if let Some(b) = dst.name.get_mut(j) {
                    *b = 0;
                }
                j += 1;
            }
            j = 0;
            while j < name.len() {
                if let (Some(b), Some(s)) = (dst.name.get_mut(j), name.get(j)) {
                    *b = *s;
                }
                j += 1;
            }
            dst.nlen = name.len() as u8;
            dst.uid = uid;
            dst.gid = gid;
        }
        self.n += 1;
        Ok(self.n as u32)
    }

    /// 按 uid 查记录。
    pub fn lookup(&self, uid: u32) -> Result<&Passwd, u32> {
        let mut i = 0usize;
        while i < self.n {
            match self.rows.get(i) {
                Some(r) => {
                    if r.uid == uid {
                        return Ok(r);
                    }
                }
                None => return Err(WA_EBAL),
            }
            i += 1;
        }
        Err(WA_ENOENT)
    }

    /// 按 uid 取名并写入缓冲。
    pub fn name_of(&self, uid: u32, dst: &mut [u8]) -> Result<u32, u32> {
        let rec = self.lookup(uid)?;
        let name = match rec.name() {
            Some(v) => v,
            None => return Err(WA_EBADNAME),
        };
        if dst.len() < name.len() {
            return Err(WA_EBUF);
        }
        let mut i = 0usize;
        while i < name.len() {
            if let (Some(d), Some(s)) = (dst.get_mut(i), name.get(i)) {
                *d = *s;
            }
            i += 1;
        }
        Ok(name.len() as u32)
    }

    /// 有效名。
    pub fn whoami(&self, s: Session, dst: &mut [u8]) -> Result<u32, u32> {
        if s.eff > ID_MAX {
            return Err(WA_ENOEFF);
        }
        self.name_of(s.eff, dst)
    }

    /// 真实名。
    pub fn logname(&self, s: Session, dst: &mut [u8]) -> Result<u32, u32> {
        if s.real > ID_MAX {
            return Err(WA_ENOEFF);
        }
        self.name_of(s.real, dst)
    }

    /// 不变式自检。
    pub fn consistent(&self) -> bool {
        if self.n > CAP {
            return false;
        }
        let mut i = 0usize;
        while i < self.n {
            let r = match self.rows.get(i) {
                Some(v) => v,
                None => return false,
            };
            if r.nlen as usize == 0 || r.nlen as usize > NAME_CAP || r.uid > ID_MAX {
                return false;
            }
            let mut j = 0usize;
            while j < i {
                match self.rows.get(j) {
                    Some(prev) => {
                        if prev.uid == r.uid {
                            return false;
                        }
                    }
                    None => return false,
                }
                j += 1;
            }
            i += 1;
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
            return Err(WA_ESLOT);
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
            return Err(WA_EBAL);
        }
        const VALS: [u32; 8] = [1, 2, 3, 1000, 5, 4, 5, 6];
        const CODES: [u32; 6] =
            [WA_EDUPNAME, WA_EDUPUID, WA_EBADNAME, WA_ENOENT, WA_EBUF, WA_ENOEFF];
        let mut vi = 0usize;
        let mut ci = 0usize;
        let mut i = 0usize;
        while i < self.len {
            let s = match self.rows.get(i) {
                Some(v) => *v,
                None => return Err(WA_EBAL),
            };
            if s.slot as usize != i {
                return Err(WA_ESLOT);
            }
            if s.op == 0 {
                return Err(WA_EVALUE);
            }
            if s.ok {
                if s.code != 0 {
                    return Err(WA_EVALUE);
                }
                match VALS.get(vi) {
                    Some(w) => {
                        if s.value != *w {
                            return Err(WA_EVALUE);
                        }
                    }
                    None => return Err(WA_EBAL),
                }
                vi += 1;
            } else {
                if s.value != 0 {
                    return Err(WA_EVALUE);
                }
                match CODES.get(ci) {
                    Some(w) => {
                        if s.code != *w {
                            return Err(WA_EVALUE);
                        }
                    }
                    None => return Err(WA_EBAL),
                }
                ci += 1;
            }
            i += 1;
        }
        if vi != 8 || ci != 6 {
            return Err(WA_EBAL);
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
            return Err(WA_EBAL);
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
            Err(WA_ESEAL)
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

/// 十四格账本：三次开户 → 查组 → 两次有效名 → 真实名 → 直取名 → 六类拒绝。
pub fn build_ledger() -> Ledger {
    let mut l = Ledger::new();
    let mut db = Db::new();

    let _ = l.register(word_cell(0, OP_ADD, db.add(b"root", 0, 0)));
    let _ = l.register(word_cell(1, OP_ADD, db.add(b"daemon", 1, 1)));
    let _ = l.register(word_cell(2, OP_ADD, db.add(b"varix", 1000, 1000)));

    let gid = match db.lookup(1000) {
        Ok(r) => Ok(r.gid()),
        Err(c) => Err(c),
    };
    let _ = l.register(word_cell(3, OP_LOOKUP, gid));

    let user = Session { real: 1000, eff: 1000 };
    let root = Session { real: 1000, eff: 0 };
    let mut buf = [0u8; 16];
    let _ = l.register(word_cell(4, OP_WHOAMI, db.whoami(user, &mut buf)));
    let _ = l.register(word_cell(5, OP_WHOAMI, db.whoami(root, &mut buf)));
    let _ = l.register(word_cell(6, OP_LOGNAME, db.logname(root, &mut buf)));

    let mut small = [0u8; 8];
    let _ = l.register(word_cell(7, OP_NAMEOF, db.name_of(1, &mut small)));

    let _ = l.register(word_cell(8, OP_ADD, db.add(b"daemon", 2, 2)));
    let _ = l.register(word_cell(9, OP_ADD, db.add(b"other", 1, 1)));
    let _ = l.register(word_cell(10, OP_ADD, db.add(b"a:b", 9, 9)));

    let unknown = match db.lookup(4242) {
        Ok(r) => Ok(r.uid()),
        Err(c) => Err(c),
    };
    let _ = l.register(word_cell(11, OP_LOOKUP, unknown));

    let mut tiny = [0u8; 2];
    let _ = l.register(word_cell(12, OP_NAMEOF, db.name_of(1, &mut tiny)));

    let ghost = Session { real: 0, eff: NOBODY };
    let _ = l.register(word_cell(13, OP_WHOAMI, db.whoami(ghost, &mut small)));

    l
}

/// 四路注入。
pub fn inject_four() -> [bool; 4] {
    let mut out = [false; 4];

    let l = build_ledger();
    let rep = l.replay();
    let root = l.seal();
    out[0] = l.len() == SAMPLES && l.audit().is_ok() && rep == Ok(root) && root != 0;

    // 一路：开户、查表、两条命名路径。
    let mut db = Db::new();
    let a = db.add(b"root", 0, 0);
    let b = db.add(b"varix", 1000, 1000);
    let mut buf = [0u8; 16];
    let eff = Session { real: 0, eff: 1000 };
    let mut named = [0u8; 8];
    let w = db.whoami(eff, &mut named);
    let w_ok = w == Ok(5) && named.get(..5) == Some(&b"varix"[..]);
    let mut real_buf = [0u8; 8];
    let real = db.logname(eff, &mut real_buf);
    let cut = db.name_of(1000, &mut buf[..4]);
    out[1] = a == Ok(1)
        && b == Ok(2)
        && db.count() == 2
        && db.lookup(1000).map(|r| r.gid()) == Ok(1000)
        && w_ok
        && real == Ok(4)
        && real_buf.get(..4) == Some(&b"root"[..])
        && cut == Err(WA_EBUF)
        && !eff.same()
        && Session { real: 1000, eff: 1000 }.same()
        && db.consistent();

    // 二路：无身份与查无此人分开报。
    let mut db2 = Db::new();
    let _ = db2.add(b"root", 0, 0);
    let mut buf = [0u8; 8];
    let ghost = Session { real: 0, eff: NOBODY };
    let ghost_real = Session { real: NOBODY, eff: 0 };
    let missing = Session { real: 0, eff: 4242 };
    out[2] = db2.whoami(ghost, &mut buf) == Err(WA_ENOEFF)
        && db2.logname(ghost_real, &mut buf) == Err(WA_ENOEFF)
        && db2.whoami(missing, &mut buf) == Err(WA_ENOENT)
        && db2.whoami(Session { real: 0, eff: 0 }, &mut buf) == Ok(4)
        && buf.get(..4) == Some(&b"root"[..]);

    // 三路：六道闸门各自可辨。
    let mut db3 = Db::new();
    let long = [b'n'; NAME_CAP + 2];
    let _ = db3.add(b"root", 0, 0);
    let mut tiny = [0u8; 2];
    let full = {
        let mut d = Db::new();
        let mut i = 0u32;
        while (i as usize) < CAP {
            let _ = d.add(&[b'u', b'0' + i as u8], i + 10, i + 10);
            i += 1;
        }
        d.add(b"over", 99, 99) == Err(WA_EFULL) && d.count() == CAP
    };
    out[3] = db3.add(b"root", 5, 5) == Err(WA_EDUPNAME)
        && db3.add(b"other", 0, 5) == Err(WA_EDUPUID)
        && db3.add(b"", 5, 5) == Err(WA_EBADNAME)
        && db3.add(b"a:b", 5, 5) == Err(WA_EBADNAME)
        && db3.add(&long, 5, 5) == Err(WA_ENAMELONG)
        && db3.add(b"bad", NOBODY, 5) == Err(WA_ENOEFF)
        && db3.name_of(0, &mut tiny) == Err(WA_EBUF)
        && full
        && db3.consistent();

    out
}

/// 十三项判据。
pub fn run_unx11505_checks() -> CheckSet {
    let mut s = CheckSet::new("unx11505.whoami_effective");
    let l = build_ledger();

    s.add_dyn(
        "WA01-十四格八允六拒",
        l.len() == SAMPLES && l.ok_count() == 8 && l.err_count() == 6 && l.audit().is_ok(),
        &format!("n={} ok={} err={}", l.len(), l.ok_count(), l.err_count()),
    );

    s.add(
        "WA02-开户三行有序",
        {
            let mut db = Db::new();
            let _ = db.add(b"root", 0, 0);
            let _ = db.add(b"daemon", 1, 1);
            let _ = db.add(b"varix", 1000, 1000);
            db.count() == 3
                && db.lookup(0).ok().and_then(|r| r.name()) == Some(&b"root"[..])
                && db.row_at(1).map(|r| r.uid()) == Some(1)
                && db.row_at(2).map(|r| r.gid()) == Some(1000)
                && db.row_at(3).is_none()
                && db.consistent()
        },
        "static",
    );

    s.add(
        "WA03-按id查组",
        {
            let mut db = Db::new();
            let _ = db.add(b"root", 0, 0);
            let _ = db.add(b"varix", 1000, 1000);
            db.lookup(1000).map(|r| r.gid()) == Ok(1000)
                && db.lookup(0).map(|r| r.gid()) == Ok(0)
                && l.sample(3).map(|x| x.value) == Some(1000)
        },
        "static",
    );

    s.add(
        "WA04-有效名优先于真实名",
        {
            let mut db = Db::new();
            let _ = db.add(b"root", 0, 0);
            let _ = db.add(b"varix", 1000, 1000);
            let s1 = Session { real: 1000, eff: 0 };
            let mut a = [0u8; 8];
            let mut b = [0u8; 8];
            let w = db.whoami(s1, &mut a);
            let g = db.logname(s1, &mut b);
            w == Ok(4)
                && a.get(..4) == Some(&b"root"[..])
                && g == Ok(5)
                && b.get(..5) == Some(&b"varix"[..])
                && !s1.same()
                && l.sample(4).map(|x| x.value) == Some(5)
        },
        "static",
    );

    s.add(
        "WA05-查无此人",
        {
            let mut db = Db::new();
            let _ = db.add(b"root", 0, 0);
            let mut buf = [0u8; 8];
            db.lookup(4242).is_err()
                && db.name_of(4242, &mut buf) == Err(WA_ENOENT)
                && db.whoami(Session { real: 0, eff: 4242 }, &mut buf) == Err(WA_ENOENT)
                && l.sample(11).map(|x| x.code) == Some(WA_ENOENT)
        },
        "static",
    );

    s.add(
        "WA06-重名被拒",
        {
            let mut db = Db::new();
            let _ = db.add(b"root", 0, 0);
            let r = db.add(b"root", 2, 2);
            r == Err(WA_EDUPNAME)
                && db.count() == 1
                && l.sample(8).map(|x| x.code) == Some(WA_EDUPNAME)
        },
        "static",
    );

    s.add(
        "WA07-重uid被拒",
        {
            let mut db = Db::new();
            let _ = db.add(b"root", 0, 0);
            let r = db.add(b"other", 0, 1);
            r == Err(WA_EDUPUID)
                && db.count() == 1
                && db.lookup(0).map(|x| x.uid()) == Ok(0)
                && l.sample(9).map(|x| x.code) == Some(WA_EDUPUID)
        },
        "static",
    );

    s.add(
        "WA08-名字含分隔符被拒",
        {
            let mut db = Db::new();
            db.add(b"a:b", 1, 1) == Err(WA_EBADNAME)
                && db.add(b"", 2, 2) == Err(WA_EBADNAME)
                && db.count() == 0
                && l.sample(10).map(|x| x.code) == Some(WA_EBADNAME)
        },
        "static",
    );

    s.add(
        "WA09-名字容量边界",
        {
            let mut db = Db::new();
            let exact = [b'n'; NAME_CAP];
            let over = [b'n'; NAME_CAP + 1];
            db.add(&exact, 1, 1) == Ok(1)
                && db.add(&over, 2, 2) == Err(WA_ENAMELONG)
                && db.count() == 1
        },
        "static",
    );

    s.add(
        "WA10-无身份与有身份",
        {
            let mut db = Db::new();
            let _ = db.add(b"root", 0, 0);
            let mut buf = [0u8; 8];
            db.whoami(Session { real: 0, eff: NOBODY }, &mut buf) == Err(WA_ENOEFF)
                && db.logname(Session { real: NOBODY, eff: 0 }, &mut buf) == Err(WA_ENOEFF)
                && db.whoami(Session { real: 0, eff: 0 }, &mut buf) == Ok(4)
                && l.sample(13).map(|x| x.code) == Some(WA_ENOEFF)
        },
        "static",
    );

    s.add(
        "WA11-缓冲边界",
        {
            let mut db = Db::new();
            let _ = db.add(b"daemon", 1, 1);
            let mut exact = [0u8; 6];
            let mut short = [0u8; 5];
            db.name_of(1, &mut exact) == Ok(6)
                && exact == [b'd', b'a', b'e', b'm', b'o', b'n']
                && db.name_of(1, &mut short) == Err(WA_EBUF)
                && l.sample(12).map(|x| x.code) == Some(WA_EBUF)
        },
        "static",
    );

    s.add(
        "WA12-表满边界",
        {
            let mut db = Db::new();
            let mut i = 0u32;
            while (i as usize) < CAP {
                let _ = db.add(&[b'u', b'0' + i as u8], i + 10, i + 10);
                i += 1;
            }
            let full = db.add(b"over", 99, 99);
            full == Err(WA_EFULL)
                && db.count() == CAP
                && db.lookup(10).is_ok()
                && db.consistent()
        },
        "static",
    );

    s.add(
        "WA13-冻结重放四注入",
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

    fn table() -> Db {
        let mut db = Db::new();
        let _ = db.add(b"root", 0, 0);
        let _ = db.add(b"daemon", 1, 1);
        let _ = db.add(b"varix", 1000, 1000);
        db
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
        let s = run_unx11505_checks();
        assert_eq!(s.len(), 13);
        assert!(s.all_passed());
    }

    #[test]
    fn effective_vs_real() {
        let db = table();
        let mut buf = [0u8; 8];
        let s = Session { real: 1000, eff: 0 };
        assert_eq!(db.whoami(s, &mut buf), Ok(4));
        assert_eq!(&buf[..4], b"root");
        assert_eq!(db.logname(s, &mut buf), Ok(5));
        assert_eq!(&buf[..5], b"varix");
        assert!(!s.same());
        assert!(Session { real: 1, eff: 1 }.same());
    }

    #[test]
    fn lookup_gates() {
        let db = table();
        assert_eq!(db.lookup(1000).map(|r| r.gid()), Ok(1000));
        assert_eq!(db.lookup(4242).is_err(), true);
        let mut buf = [0u8; 8];
        assert_eq!(db.name_of(4242, &mut buf), Err(WA_ENOENT));
    }

    #[test]
    fn no_identity() {
        let db = table();
        let mut buf = [0u8; 8];
        assert_eq!(db.whoami(Session { real: 0, eff: NOBODY }, &mut buf), Err(WA_ENOEFF));
        assert_eq!(db.logname(Session { real: NOBODY, eff: 0 }, &mut buf), Err(WA_ENOEFF));
    }

    #[test]
    fn duplicate_and_bad_names() {
        let mut db = table();
        assert_eq!(db.add(b"root", 7, 7), Err(WA_EDUPNAME));
        assert_eq!(db.add(b"other", 0, 7), Err(WA_EDUPUID));
        assert_eq!(db.add(b"", 7, 7), Err(WA_EBADNAME));
        assert_eq!(db.add(b"a:b", 7, 7), Err(WA_EBADNAME));
        assert_eq!(db.add(b"ok", NOBODY, 7), Err(WA_ENOEFF));
    }

    #[test]
    fn capacity_and_buffer() {
        let mut db = Db::new();
        let mut i = 0u32;
        while (i as usize) < CAP {
            assert_eq!(db.add(&[b'u', b'0' + i as u8], i + 10, i + 10), Ok(i + 1));
            i += 1;
        }
        assert_eq!(db.add(b"over", 99, 99), Err(WA_EFULL));
        let mut short = [0u8; 1];
        assert_eq!(db.name_of(10, &mut short), Err(WA_EBUF));
        assert!(db.consistent());
    }

    #[test]
    fn inject_four_holds() {
        assert_eq!(inject_four(), [true; 4]);
    }
}
