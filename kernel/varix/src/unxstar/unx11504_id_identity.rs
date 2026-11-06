//! UNX-F11504 id 身份三账（uid/gid/组表）（0xf090 段）
//!
//! `id` 的输出是三本账拼起来的：用户账（uid）、主组账（gid）、附加组账
//! （升序的组表）。三本账不是独立字段，它们之间有三条硬约束：
//!
//! 1. **主组不进附加表**：把 gid 再塞进组表会让"用户属于哪些组"重复计数，
//!    查询结果看起来对、账目却是错的；
//! 2. **附加组必须升序**：乱序的组表无法二分、无法去重，也无法对账，所以
//!    插入时低于表尾一律拒绝，而不是替调用方排序；
//! 3. **保留段不许入账**：`0x8000_0000` 及以上（含 -1 哨兵）不是合法 id，
//!    任何一本账都不收。
//!
//! 身份切换（`set_uid` / `set_gid`）另有一道权限闸门：只有当前 uid 为 0 的
//! 身份能改自己的 uid/gid，否则返回 `ENOTROOT`。把 gid 改成组表里已有的值
//! 返回 `EPRIM`——先删组、再换主组，次序不能省。
//!
//! 入账闸门次序（`add_group`）：保留段 → 表满 → 主组重复 → 组表重复 →
//! 破坏升序。`drop_group` 对缺席组返回 `ENOMEMBER`；删除是搬移后一位覆盖，
//! 保持升序不变。渲染（`uid=… gid=… groups=…`）先量总长，放不下返回 `EBUF`
//! 且一格不写。
//!
//! 账本十四格：八格登记成功路径（三次入组、分类、成员查询、删组、计数、
//! 渲染），六格登记各类拒绝（重复组、乱序组、缺席组、主组重入、保留段、
//! 缓冲不足）。审计逐格复算，封印折叠全账，冻结后拒绝登记并计数。

use crate::alloc::format;
use crate::checks::CheckSet;

/// 段基址。
pub const SEG_BASE: u32 = 0xf090;

/// id 落在保留段。
pub const IDN_ERANGE: u32 = SEG_BASE + 0x01;
/// 组表满。
pub const IDN_EFULL: u32 = SEG_BASE + 0x02;
/// 主组重复进附加表。
pub const IDN_EPRIM: u32 = SEG_BASE + 0x03;
/// 附加组重复。
pub const IDN_EDUP: u32 = SEG_BASE + 0x04;
/// 破坏升序。
pub const IDN_EORDER: u32 = SEG_BASE + 0x05;
/// 组不在表内。
pub const IDN_ENOMEMBER: u32 = SEG_BASE + 0x06;
/// 非 root 不许切换身份。
pub const IDN_ENOTROOT: u32 = SEG_BASE + 0x07;
/// 渲染缓冲不足。
pub const IDN_EBUF: u32 = SEG_BASE + 0x08;
/// 账本形状不符。
pub const IDN_EBAL: u32 = SEG_BASE + 0x09;
/// 槽位乱序。
pub const IDN_ESLOT: u32 = SEG_BASE + 0x0a;
/// 观测值与复算不符。
pub const IDN_EVALUE: u32 = SEG_BASE + 0x0b;
/// 冻结后重放不一致。
pub const IDN_ESEAL: u32 = SEG_BASE + 0x0c;
/// 观察下标越界。
pub const IDN_ENOBS: u32 = SEG_BASE + 0x0d;

/// 组表容量。
pub const GCAP: usize = 8;
/// 合法 id 上界（含）。
pub const ID_MAX: u32 = 0x7fff_ffff;
/// 账本格数。
pub const SAMPLES: usize = 14;

/// 分类：root。
pub const CLASS_ROOT: u8 = 0;
/// 分类：系统账号。
pub const CLASS_SYS: u8 = 1;
/// 分类：普通用户。
pub const CLASS_USER: u8 = 2;

/// 操作码：入组。
pub const OP_GROUP: u8 = 1;
/// 操作码：删组。
pub const OP_DROP: u8 = 2;
/// 操作码：分类。
pub const OP_CLASS: u8 = 3;
/// 操作码：成员查询。
pub const OP_IN: u8 = 4;
/// 操作码：渲染。
pub const OP_RENDER: u8 = 5;
/// 操作码：计数。
pub const OP_COUNT: u8 = 6;

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

/// 十进制位数。
fn digits(v: u32) -> usize {
    let mut n = 1usize;
    let mut x = v;
    while x >= 10 {
        x /= 10;
        n += 1;
    }
    n
}

/// 写字节串。
fn put_bytes(dst: &mut [u8], at: usize, src: &[u8]) -> usize {
    let mut i = 0usize;
    while i < src.len() {
        if let (Some(d), Some(s)) = (dst.get_mut(at + i), src.get(i)) {
            *d = *s;
        }
        i += 1;
    }
    at + src.len()
}

/// 写十进制数。
fn put_u32(dst: &mut [u8], at: usize, v: u32) -> usize {
    let mut buf = [0u8; 10];
    let mut n = 0usize;
    let mut x = v;
    if x == 0 {
        if let Some(d) = dst.get_mut(at) {
            *d = b'0';
        }
        return at + 1;
    }
    while x > 0 {
        if let Some(d) = buf.get_mut(n) {
            *d = b'0' + (x % 10) as u8;
        }
        x /= 10;
        n += 1;
    }
    let mut i = 0usize;
    while i < n {
        if let (Some(d), Some(s)) = (dst.get_mut(at + i), buf.get(n - 1 - i)) {
            *d = *s;
        }
        i += 1;
    }
    at + n
}

/// 身份三账。
#[derive(Clone, Copy, Debug)]
pub struct Identity {
    uid: u32,
    gid: u32,
    groups: [u32; GCAP],
    gn: usize,
    adds: u32,
}

impl Identity {
    /// 开户。
    pub fn new(uid: u32, gid: u32) -> Result<Identity, u32> {
        if uid > ID_MAX || gid > ID_MAX {
            return Err(IDN_ERANGE);
        }
        Ok(Identity { uid, gid, groups: [0u32; GCAP], gn: 0, adds: 0 })
    }

    /// 用户账。
    pub fn uid(&self) -> u32 {
        self.uid
    }

    /// 主组账。
    pub fn gid(&self) -> u32 {
        self.gid
    }

    /// 组表条数。
    pub fn group_count(&self) -> usize {
        self.gn
    }

    /// 第 i 个附加组。
    pub fn group_at(&self, i: usize) -> Option<u32> {
        if i >= self.gn {
            return None;
        }
        self.groups.get(i).copied()
    }

    /// 是否属于某组（主组或附加组）。
    pub fn in_group(&self, g: u32) -> bool {
        if g == self.gid {
            return true;
        }
        let mut i = 0usize;
        while i < self.gn {
            if self.groups.get(i).copied() == Some(g) {
                return true;
            }
            i += 1;
        }
        false
    }

    /// 入组，返回组表条数。
    pub fn add_group(&mut self, g: u32) -> Result<u32, u32> {
        if g > ID_MAX {
            return Err(IDN_ERANGE);
        }
        if self.gn >= GCAP {
            return Err(IDN_EFULL);
        }
        if g == self.gid {
            return Err(IDN_EPRIM);
        }
        let mut i = 0usize;
        while i < self.gn {
            if self.groups.get(i).copied() == Some(g) {
                return Err(IDN_EDUP);
            }
            i += 1;
        }
        if self.gn > 0 {
            let last = match self.groups.get(self.gn - 1).copied() {
                Some(v) => v,
                None => return Err(IDN_EORDER),
            };
            if g < last {
                return Err(IDN_EORDER);
            }
        }
        if let Some(dst) = self.groups.get_mut(self.gn) {
            *dst = g;
        }
        self.gn += 1;
        self.adds = self.adds.saturating_add(1);
        Ok(self.gn as u32)
    }

    /// 删组，返回组表条数。
    pub fn drop_group(&mut self, g: u32) -> Result<u32, u32> {
        let mut at: Option<usize> = None;
        let mut i = 0usize;
        while i < self.gn {
            if self.groups.get(i).copied() == Some(g) {
                at = Some(i);
                break;
            }
            i += 1;
        }
        let k0 = match at {
            Some(k) => k,
            None => return Err(IDN_ENOMEMBER),
        };
        let mut k = k0;
        while k + 1 < self.gn {
            let next = match self.groups.get(k + 1).copied() {
                Some(v) => v,
                None => break,
            };
            if let Some(dst) = self.groups.get_mut(k) {
                *dst = next;
            }
            k += 1;
        }
        if self.gn > 0 {
            self.gn -= 1;
        }
        Ok(self.gn as u32)
    }

    /// 分类。
    pub fn classify(&self) -> u8 {
        if self.uid == 0 {
            CLASS_ROOT
        } else if self.uid < 1000 {
            CLASS_SYS
        } else {
            CLASS_USER
        }
    }

    /// 切换用户账。
    pub fn set_uid(&mut self, uid: u32) -> Result<u32, u32> {
        if uid > ID_MAX {
            return Err(IDN_ERANGE);
        }
        if self.uid != 0 {
            return Err(IDN_ENOTROOT);
        }
        self.uid = uid;
        Ok(self.uid)
    }

    /// 切换主组账。
    pub fn set_gid(&mut self, gid: u32) -> Result<u32, u32> {
        if gid > ID_MAX {
            return Err(IDN_ERANGE);
        }
        if self.uid != 0 {
            return Err(IDN_ENOTROOT);
        }
        let mut i = 0usize;
        while i < self.gn {
            if self.groups.get(i).copied() == Some(gid) {
                return Err(IDN_EPRIM);
            }
            i += 1;
        }
        self.gid = gid;
        Ok(self.gid)
    }

    /// 渲染总长。
    pub fn print_len(&self) -> usize {
        let mut n = 4 + digits(self.uid) + 5 + digits(self.gid) + 8;
        let mut i = 0usize;
        while i < self.gn {
            if i > 0 {
                n += 1;
            }
            if let Some(g) = self.groups.get(i) {
                n += digits(*g);
            }
            i += 1;
        }
        n
    }

    /// 渲染 `uid=… gid=… groups=…`。
    pub fn render(&self, dst: &mut [u8]) -> Result<u32, u32> {
        let need = self.print_len();
        if dst.len() < need {
            return Err(IDN_EBUF);
        }
        let mut at = 0usize;
        at = put_bytes(dst, at, b"uid=");
        at = put_u32(dst, at, self.uid);
        at = put_bytes(dst, at, b" gid=");
        at = put_u32(dst, at, self.gid);
        at = put_bytes(dst, at, b" groups=");
        let mut i = 0usize;
        while i < self.gn {
            if i > 0 {
                at = put_bytes(dst, at, b",");
            }
            if let Some(g) = self.groups.get(i).copied() {
                at = put_u32(dst, at, g);
            }
            i += 1;
        }
        Ok(at as u32)
    }

    /// 不变式自检。
    pub fn consistent(&self) -> bool {
        if self.uid > ID_MAX || self.gid > ID_MAX || self.gn > GCAP {
            return false;
        }
        if (self.adds as usize) < self.gn {
            return false;
        }
        let mut i = 0usize;
        while i < self.gn {
            let g = match self.groups.get(i).copied() {
                Some(v) => v,
                None => return false,
            };
            if g > ID_MAX || g == self.gid {
                return false;
            }
            if i > 0 {
                match self.groups.get(i - 1).copied() {
                    Some(prev) => {
                        if prev >= g {
                            return false;
                        }
                    }
                    None => return false,
                }
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
            return Err(IDN_ESLOT);
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
            return Err(IDN_EBAL);
        }
        const VALS: [u32; 8] = [1, 2, 3, 2, 1, 2, 2, 31];
        const CODES: [u32; 6] =
            [IDN_EDUP, IDN_EORDER, IDN_ENOMEMBER, IDN_EPRIM, IDN_ERANGE, IDN_EBUF];
        let mut vi = 0usize;
        let mut ci = 0usize;
        let mut i = 0usize;
        while i < self.len {
            let s = match self.rows.get(i) {
                Some(v) => *v,
                None => return Err(IDN_EBAL),
            };
            if s.slot as usize != i {
                return Err(IDN_ESLOT);
            }
            if s.op == 0 {
                return Err(IDN_EVALUE);
            }
            if s.ok {
                if s.code != 0 {
                    return Err(IDN_EVALUE);
                }
                match VALS.get(vi) {
                    Some(w) => {
                        if s.value != *w {
                            return Err(IDN_EVALUE);
                        }
                    }
                    None => return Err(IDN_EBAL),
                }
                vi += 1;
            } else {
                if s.value != 0 {
                    return Err(IDN_EVALUE);
                }
                match CODES.get(ci) {
                    Some(w) => {
                        if s.code != *w {
                            return Err(IDN_EVALUE);
                        }
                    }
                    None => return Err(IDN_EBAL),
                }
                ci += 1;
            }
            i += 1;
        }
        if vi != 8 || ci != 6 {
            return Err(IDN_EBAL);
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
            return Err(IDN_EBAL);
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
            Err(IDN_ESEAL)
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

/// 十四格账本：三次入组 → 分类 → 成员 → 删组 → 计数 → 渲染 → 六类拒绝。
pub fn build_ledger() -> Ledger {
    let mut l = Ledger::new();
    let mut id = match Identity::new(1000, 500) {
        Ok(v) => v,
        Err(_) => return l,
    };

    let _ = l.register(word_cell(0, OP_GROUP, id.add_group(27)));
    let _ = l.register(word_cell(1, OP_GROUP, id.add_group(100)));
    let _ = l.register(word_cell(2, OP_GROUP, id.add_group(1000)));
    let _ = l.register(word_cell(3, OP_CLASS, Ok(id.classify() as u32)));
    let _ = l.register(word_cell(4, OP_IN, Ok(id.in_group(100) as u32)));
    let _ = l.register(word_cell(5, OP_DROP, id.drop_group(100)));
    let _ = l.register(word_cell(6, OP_COUNT, Ok(id.group_count() as u32)));

    let mut buf = [0u8; 64];
    let _ = l.register(word_cell(7, OP_RENDER, id.render(&mut buf)));

    let _ = l.register(word_cell(8, OP_GROUP, id.add_group(27)));
    let _ = l.register(word_cell(9, OP_GROUP, id.add_group(20)));
    let _ = l.register(word_cell(10, OP_DROP, id.drop_group(999)));

    let mut prim = match Identity::new(0, 500) {
        Ok(v) => v,
        Err(_) => return l,
    };
    let _ = l.register(word_cell(11, OP_GROUP, prim.add_group(500)));
    let _ = l.register(word_cell(12, OP_GROUP, id.add_group(0x8000_0000)));

    let mut tiny = [0u8; 4];
    let _ = l.register(word_cell(13, OP_RENDER, id.render(&mut tiny)));

    l
}

/// 四路注入。
pub fn inject_four() -> [bool; 4] {
    let mut out = [false; 4];

    let l = build_ledger();
    let rep = l.replay();
    let root = l.seal();
    out[0] = l.len() == SAMPLES && l.audit().is_ok() && rep == Ok(root) && root != 0;

    // 一路：三账开户，升序入组，主组也算成员。
    let mut a = match Identity::new(1000, 500) {
        Ok(v) => v,
        Err(_) => return out,
    };
    let g1 = a.add_group(27);
    let g2 = a.add_group(100);
    let g3 = a.add_group(1000);
    let dropped = a.drop_group(100);
    let mut buf = [0u8; 64];
    let text = a.render(&mut buf);
    out[1] = g1 == Ok(1)
        && g2 == Ok(2)
        && g3 == Ok(3)
        && a.classify() == CLASS_USER
        && a.in_group(500)
        && a.in_group(27)
        && !a.in_group(28)
        && dropped == Ok(2)
        && a.group_at(1) == Some(1000)
        && text == Ok(31)
        && buf.get(..31) == Some(&b"uid=1000 gid=500 groups=27,1000"[..])
        && a.consistent();

    // 二路：身份切换只有 root 能做，换主组先清组表。
    let mut r = match Identity::new(0, 0) {
        Ok(v) => v,
        Err(_) => return out,
    };
    let to_user = r.set_uid(1000);
    let again = r.set_uid(1001);
    let mut root2 = match Identity::new(0, 0) {
        Ok(v) => v,
        Err(_) => return out,
    };
    let _ = root2.add_group(500);
    let prim = root2.set_gid(500);
    let ok_gid = root2.set_gid(501);
    out[2] = to_user == Ok(1000)
        && again == Err(IDN_ENOTROOT)
        && prim == Err(IDN_EPRIM)
        && ok_gid == Ok(501)
        && r.classify() == CLASS_USER
        && r.consistent();

    // 三路：八类失败各自可辨。
    let mut z = match Identity::new(1000, 500) {
        Ok(v) => v,
        Err(_) => return out,
    };
    let mut tiny = [0u8; 4];
    let _ = z.add_group(10);
    out[3] = z.add_group(9) == Err(IDN_EORDER)
        && z.add_group(10) == Err(IDN_EDUP)
        && z.add_group(500) == Err(IDN_EPRIM)
        && z.add_group(0x8000_0000) == Err(IDN_ERANGE)
        && z.drop_group(11) == Err(IDN_ENOMEMBER)
        && z.set_uid(1) == Err(IDN_ENOTROOT)
        && z.render(&mut tiny) == Err(IDN_EBUF)
        && matches!(Identity::new(0x8000_0000, 0), Err(IDN_ERANGE))
        && z.consistent();

    out
}

/// 十三项判据。
pub fn run_unx11504_checks() -> CheckSet {
    let mut s = CheckSet::new("unx11504.id_identity");
    let l = build_ledger();

    s.add_dyn(
        "IDN01-十四格八允六拒",
        l.len() == SAMPLES && l.ok_count() == 8 && l.err_count() == 6 && l.audit().is_ok(),
        &format!("n={} ok={} err={}", l.len(), l.ok_count(), l.err_count()),
    );

    s.add(
        "IDN02-三账开户与分类",
        {
            let root = Identity::new(0, 0);
            let sys = Identity::new(1, 1);
            let user = Identity::new(1000, 500);
            match (root, sys, user) {
                (Ok(r), Ok(sy), Ok(u)) => {
                    r.classify() == CLASS_ROOT
                        && sy.classify() == CLASS_SYS
                        && u.classify() == CLASS_USER
                        && u.uid() == 1000
                        && u.gid() == 500
                        && u.consistent()
                }
                _ => false,
            }
        },
        "static",
    );

    s.add(
        "IDN03-组表升序入账",
        {
            let mut a = match Identity::new(1000, 500) {
                Ok(v) => v,
                Err(_) => return s,
            };
            let _ = a.add_group(27);
            let _ = a.add_group(100);
            let _ = a.add_group(1000);
            a.group_count() == 3
                && a.group_at(0) == Some(27)
                && a.group_at(1) == Some(100)
                && a.group_at(2) == Some(1000)
                && a.group_at(3) == None
                && a.in_group(100)
                && !a.in_group(101)
                && l.sample(2).map(|x| x.value) == Some(3)
        },
        "static",
    );

    s.add(
        "IDN04-乱序与重复被拒",
        {
            let mut a = match Identity::new(1000, 500) {
                Ok(v) => v,
                Err(_) => return s,
            };
            let _ = a.add_group(27);
            let order = a.add_group(20);
            let dup = a.add_group(27);
            order == Err(IDN_EORDER)
                && dup == Err(IDN_EDUP)
                && a.group_count() == 1
                && l.sample(8).map(|x| x.code) == Some(IDN_EDUP)
        },
        "static",
    );

    s.add(
        "IDN05-主组不进附加表",
        {
            let mut a = match Identity::new(0, 500) {
                Ok(v) => v,
                Err(_) => return s,
            };
            let prim = a.add_group(500);
            let _ = a.add_group(600);
            let clash = a.set_gid(600);
            let ok = a.set_gid(700);
            prim == Err(IDN_EPRIM)
                && clash == Err(IDN_EPRIM)
                && ok == Ok(700)
                && a.gid() == 700
                && l.sample(11).map(|x| x.code) == Some(IDN_EPRIM)
        },
        "static",
    );

    s.add(
        "IDN06-删组与缺席",
        {
            let mut a = match Identity::new(1000, 500) {
                Ok(v) => v,
                Err(_) => return s,
            };
            let _ = a.add_group(27);
            let _ = a.add_group(100);
            let _ = a.add_group(1000);
            let gone = a.drop_group(100);
            let twice = a.drop_group(100);
            gone == Ok(2)
                && twice == Err(IDN_ENOMEMBER)
                && a.group_at(1) == Some(1000)
                && a.consistent()
                && l.sample(10).map(|x| x.code) == Some(IDN_ENOMEMBER)
        },
        "static",
    );

    s.add(
        "IDN07-成员查询含主组",
        {
            let mut a = match Identity::new(1000, 500) {
                Ok(v) => v,
                Err(_) => return s,
            };
            let _ = a.add_group(27);
            a.in_group(500)
                && a.in_group(27)
                && !a.in_group(28)
                && a.group_count() == 1
                && l.sample(4).map(|x| x.value) == Some(1)
        },
        "static",
    );

    s.add(
        "IDN08-渲染三账",
        {
            let mut a = match Identity::new(1000, 500) {
                Ok(v) => v,
                Err(_) => return s,
            };
            let _ = a.add_group(27);
            let _ = a.add_group(1000);
            let mut buf = [0u8; 64];
            let n = a.render(&mut buf);
            n == Ok(31)
                && a.print_len() == 31
                && buf.get(..31) == Some(&b"uid=1000 gid=500 groups=27,1000"[..])
                && l.sample(7).map(|x| x.value) == Some(31)
        },
        "static",
    );

    s.add(
        "IDN09-身份切换需root",
        {
            let mut u = match Identity::new(1000, 500) {
                Ok(v) => v,
                Err(_) => return s,
            };
            let denied = u.set_uid(0);
            let mut r = match Identity::new(0, 0) {
                Ok(v) => v,
                Err(_) => return s,
            };
            let up = r.set_uid(1000);
            let down = r.set_uid(0);
            denied == Err(IDN_ENOTROOT)
                && up == Ok(1000)
                && down == Err(IDN_ENOTROOT)
                && r.uid() == 1000
        },
        "static",
    );

    s.add(
        "IDN10-保留段被拒",
        {
            let add = match Identity::new(1000, 500) {
                Ok(mut v) => v.add_group(0x8000_0000),
                Err(_) => Err(0),
            };
            let gid = match Identity::new(0, 0) {
                Ok(mut v) => v.set_gid(0xffff_ffff),
                Err(_) => Err(0),
            };
            add == Err(IDN_ERANGE)
                && gid == Err(IDN_ERANGE)
                && Identity::new(0x8000_0000, 0).is_err()
                && l.sample(12).map(|x| x.code) == Some(IDN_ERANGE)
        },
        "static",
    );

    s.add(
        "IDN11-组表容量边界",
        {
            let mut a = match Identity::new(1000, 500) {
                Ok(v) => v,
                Err(_) => return s,
            };
            let mut i = 0u32;
            while (i as usize) < GCAP {
                let _ = a.add_group(10 + i);
                i += 1;
            }
            let full = a.add_group(100);
            full == Err(IDN_EFULL)
                && a.group_count() == GCAP
                && a.consistent()
        },
        "static",
    );

    s.add(
        "IDN12-渲染缓冲边界",
        {
            let mut a = match Identity::new(1000, 500) {
                Ok(v) => v,
                Err(_) => return s,
            };
            let _ = a.add_group(27);
            let _ = a.add_group(1000);
            let need = a.print_len();
            let mut exact = [0u8; 31];
            let mut short = [0u8; 30];
            a.render(&mut exact) == Ok(31)
                && need == 31
                && a.render(&mut short) == Err(IDN_EBUF)
                && l.sample(13).map(|x| x.code) == Some(IDN_EBUF)
        },
        "static",
    );

    s.add(
        "IDN13-冻结重放四注入",
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

    fn user() -> Identity {
        match Identity::new(1000, 500) {
            Ok(v) => v,
            Err(_) => panic!("identity"),
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
        let s = run_unx11504_checks();
        assert_eq!(s.len(), 13);
        assert!(s.all_passed());
    }

    #[test]
    fn ascending_groups() {
        let mut a = user();
        assert_eq!(a.add_group(27), Ok(1));
        assert_eq!(a.add_group(100), Ok(2));
        assert_eq!(a.add_group(100), Err(IDN_EDUP));
        assert_eq!(a.add_group(20), Err(IDN_EORDER));
        assert_eq!(a.group_count(), 2);
        assert_eq!(a.drop_group(27), Ok(1));
        assert_eq!(a.group_at(0), Some(100));
        assert!(a.consistent());
    }

    #[test]
    fn primary_group_guard() {
        let mut a = user();
        assert_eq!(a.add_group(500), Err(IDN_EPRIM));
        assert!(a.in_group(500));
        let mut r = match Identity::new(0, 500) {
            Ok(v) => v,
            Err(_) => panic!("root"),
        };
        assert_eq!(r.set_gid(600), Ok(600));
        assert_eq!(r.set_uid(1000), Ok(1000));
        assert_eq!(r.set_uid(0), Err(IDN_ENOTROOT));
    }

    #[test]
    fn render_bytes() {
        let mut a = user();
        let _ = a.add_group(27);
        let _ = a.add_group(1000);
        let mut buf = [0u8; 64];
        assert_eq!(a.render(&mut buf), Ok(31));
        assert_eq!(&buf[..31], b"uid=1000 gid=500 groups=27,1000");
        let mut tiny = [0u8; 4];
        assert_eq!(a.render(&mut tiny), Err(IDN_EBUF));
    }

    #[test]
    fn capacity_and_range() {
        let mut a = user();
        let mut i = 0u32;
        while (i as usize) < GCAP {
            assert_eq!(a.add_group(10 + i), Ok(i + 1));
            i += 1;
        }
        assert_eq!(a.add_group(100), Err(IDN_EFULL));
        assert_eq!(a.add_group(0x8000_0000), Err(IDN_ERANGE));
        assert_eq!(Identity::new(0x8000_0000, 0).is_err(), true);
    }

    #[test]
    fn classify_three_bands() {
        let r = match Identity::new(0, 0) {
            Ok(v) => v,
            Err(_) => panic!("root"),
        };
        let sy = match Identity::new(1, 1) {
            Ok(v) => v,
            Err(_) => panic!("sys"),
        };
        assert_eq!(r.classify(), CLASS_ROOT);
        assert_eq!(sy.classify(), CLASS_SYS);
        assert_eq!(user().classify(), CLASS_USER);
    }

    #[test]
    fn inject_four_holds() {
        assert_eq!(inject_four(), [true; 4]);
    }
}
