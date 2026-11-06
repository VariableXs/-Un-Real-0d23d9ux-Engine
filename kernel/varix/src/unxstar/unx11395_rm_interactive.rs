//! UNX-F11395 rm -i 交互确认语义（0xeeb0 段）
//!
//! `rm -i` 把「删除」变成一次问答：对**每个候选名**先向 stderr 打一行
//! `rm: remove regular file 'x'? `，再从 stdin 读一个回答。只有回答以 `y`/`Y`
//! 开头才真的删除，其它一律视为「不删」。不删不是错误：退出码仍然是 0。
//!
//! 这条交互协议有三处容易写歪：
//!
//! 1. **提问在判错之前，但判存在在提问之前**。名字根本不存在时不该先问再报
//!    `ENOENT`——GNU rm 先判存在，缺件直接报错，不浪费一次提问。名字存在但
//!    类型/权限不允许时，提问已经发出，回答也被消费，然后才报对应错误。
//! 2. **回答按「件」消费，不按字节重读**。一次问答消费一个回答；回答队列见底
//!    即视为 stdin 关闭（EOF），此后所有提问都按「不删」处理，但提问本身照发。
//!    这条降级档保证脚本在 `</dev/null` 下不会误删。
//! 3. **`-f` 压过 `-i`**。两个开关同时出现时，最后一个生效；本模型把 `force`
//!    视为最终态：带 `force` 就不提问、直接走删除，缺件也按 `-f` 吞掉。
//!
//! 错误类为何彼此独立：
//!
//! - `EINVAL` 是形状错（零名、零调用者），与问答无关；
//! - `ENOENT` 是名字不存在，且**在提问之前**判定，所以诊断次数增加而提问次数
//!   不增加，这一点与「回答了 y 但删除失败」形成可区分的两组计数；
//! - `EISDIR` 是回答 y 之后才发现类型不对，提问已被消费；
//! - `EACCES` 是父目录不可写；
//! - `EPERM` 是 sticky 目录越权；
//! - `EBUSY` 是挂载点；
//! - `EROFS` 是只读文件系统；
//! - `EFULL` 是条目表满。
//!
//! 闸门次序：空名 → 存在（force 时在此吞掉）→ 强制分流 → 提问/消费 →
//! 忙 → 只读 → 类型 → sticky → 父目录权限 → 摘项。

use crate::alloc::format;
use crate::checks::CheckSet;

/// 段基址。
pub const SEG_BASE: u32 = 0xeeb0;

/// 形状错（零名、零调用者）。
pub const RI_EINVAL: u32 = SEG_BASE + 0x01;
/// 名字不存在。
pub const RI_ENOENT: u32 = SEG_BASE + 0x02;
/// 目标是目录。
pub const RI_EISDIR: u32 = SEG_BASE + 0x03;
/// 父目录不可写。
pub const RI_EACCES: u32 = SEG_BASE + 0x04;
/// sticky 目录越权。
pub const RI_EPERM: u32 = SEG_BASE + 0x05;
/// 目标是挂载点。
pub const RI_EBUSY: u32 = SEG_BASE + 0x06;
/// 文件系统只读。
pub const RI_EROFS: u32 = SEG_BASE + 0x07;
/// 条目表满。
pub const RI_EFULL: u32 = SEG_BASE + 0x08;
/// 账本形状不符。
pub const RI_EBAL: u32 = SEG_BASE + 0x09;
/// 槽位乱序。
pub const RI_ESLOT: u32 = SEG_BASE + 0x0a;
/// 观测值与复算不符。
pub const RI_EVALUE: u32 = SEG_BASE + 0x0b;
/// 冻结后重放不一致。
pub const RI_ESEAL: u32 = SEG_BASE + 0x0c;
/// 观察下标越界。
pub const RI_ENOBS: u32 = SEG_BASE + 0x0d;

/// 条目表容量（即 `CAP`）。
pub const CAP: usize = 8;
/// 回答队列容量。
pub const ACAP: usize = 8;
/// 账本格数。
pub const SAMPLES: usize = 14;
/// 一次提问的字节数。
pub const PROMPT_LEN: u32 = 32;
/// 一条诊断的字节数。
pub const MSG_LEN: u32 = 26;

/// 条目类型：普通文件。
pub const K_FILE: u8 = 1;
/// 条目类型：目录。
pub const K_DIR: u8 = 2;

/// 回答：是。
pub const AYES: u8 = b'y';
/// 回答：否。
pub const ANO: u8 = b'n';

/// 操作码：播种。
pub const OP_ADD: u8 = 1;
/// 操作码：喂回答。
pub const OP_FEED: u8 = 2;
/// 操作码：交互删除。
pub const OP_REMOVE: u8 = 3;
/// 操作码：批量删除。
pub const OP_RUN: u8 = 4;
/// 操作码：读提问数。
pub const OP_PROMPT: u8 = 5;
/// 操作码：读保留数。
pub const OP_KEEP: u8 = 6;

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

/// 一个条目。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Ent {
    /// 名字。
    pub name: u32,
    /// 类型。
    pub kind: u8,
    /// 属主。
    pub uid: u32,
    /// 父目录是否可写。
    pub writable: bool,
    /// 父目录是否 sticky。
    pub sticky: bool,
    /// 是否挂载点。
    pub busy: bool,
    /// 是否只读文件系统。
    pub ro: bool,
    /// 是否存活。
    pub live: bool,
}

impl Ent {
    /// 空槽。
    pub const fn blank() -> Ent {
        Ent { name: 0, kind: 0, uid: 0, writable: false, sticky: false, busy: false, ro: false, live: false }
    }
}

/// 交互删除模型。
#[derive(Clone, Copy, Debug)]
pub struct Inter {
    e: [Ent; CAP],
    ans: [u8; ACAP],
    an: usize,
    ai: usize,
    eof: bool,
    prompts: u32,
    kept: u32,
    out_bytes: u32,
    deleted: u32,
    last_code: u32,
}

impl Inter {
    /// 空模型。
    pub const fn new() -> Inter {
        Inter {
            e: [Ent::blank(); CAP],
            ans: [0u8; ACAP],
            an: 0,
            ai: 0,
            eof: false,
            prompts: 0,
            kept: 0,
            out_bytes: 0,
            deleted: 0,
            last_code: 0,
        }
    }

    /// 活条目数。
    pub fn live(&self) -> usize {
        let mut k = 0usize;
        let mut c = 0usize;
        while k < CAP {
            if let Some(x) = self.e.get(k) {
                if x.live {
                    c += 1;
                }
            }
            k += 1;
        }
        c
    }

    /// 提问次数。
    pub fn prompt_count(&self) -> u32 {
        self.prompts
    }

    /// 因回答非 y 而保留的次数。
    pub fn kept_count(&self) -> u32 {
        self.kept
    }

    /// 累计输出字节（提问 + 诊断）。
    pub fn out_bytes(&self) -> u32 {
        self.out_bytes
    }

    /// 删除条数。
    pub fn deleted_count(&self) -> u32 {
        self.deleted
    }

    /// 是否已见 EOF。
    pub fn eof(&self) -> bool {
        self.eof
    }

    /// 最近错误码。
    pub fn last_code(&self) -> u32 {
        self.last_code
    }

    /// 第 k 个槽。
    pub fn ent_at(&self, k: usize) -> Option<Ent> {
        self.e.get(k).copied()
    }

    /// 按名找活条目。
    fn find(&self, name: u32) -> Option<usize> {
        let mut k = 0usize;
        while k < CAP {
            match self.e.get(k) {
                Some(x) => {
                    if x.live && x.name == name {
                        return Some(k);
                    }
                }
                None => return None,
            }
            k += 1;
        }
        None
    }

    /// 空槽。
    fn free(&self) -> Option<usize> {
        let mut k = 0usize;
        while k < CAP {
            match self.e.get(k) {
                Some(x) => {
                    if !x.live {
                        return Some(k);
                    }
                }
                None => return None,
            }
            k += 1;
        }
        None
    }

    /// 记一次诊断。
    fn complain(&mut self, code: u32) -> Result<usize, u32> {
        self.out_bytes = self.out_bytes.saturating_add(MSG_LEN);
        self.last_code = code;
        Err(code)
    }

    /// 播种条目。
    pub fn add(&mut self, name: u32, kind: u8, uid: u32, writable: bool, sticky: bool) -> Result<usize, u32> {
        if name == 0 || uid == 0 || (kind != K_FILE && kind != K_DIR) {
            return self.complain(RI_EINVAL);
        }
        if self.find(name).is_some() {
            return self.complain(RI_EINVAL);
        }
        let slot = match self.free() {
            Some(v) => v,
            None => return self.complain(RI_EFULL),
        };
        if let Some(x) = self.e.get_mut(slot) {
            *x = Ent { name, kind, uid, writable, sticky, busy: false, ro: false, live: true };
        }
        Ok(self.live())
    }

    /// 喂一个回答。
    pub fn feed(&mut self, a: u8) -> Result<usize, u32> {
        if a != AYES && a != ANO {
            return self.complain(RI_EINVAL);
        }
        if self.an >= ACAP {
            return self.complain(RI_EFULL);
        }
        if let Some(slot) = self.ans.get_mut(self.an) {
            *slot = a;
        }
        self.an += 1;
        Ok(self.an)
    }

    /// 取一个回答；队列见底按 EOF 处理并降级为「不删」。
    fn take_answer(&mut self) -> u8 {
        if self.ai >= self.an {
            self.eof = true;
            return ANO;
        }
        let a = match self.ans.get(self.ai) {
            Some(v) => *v,
            None => ANO,
        };
        self.ai += 1;
        a
    }

    /// 标记挂载点。
    pub fn mark_busy(&mut self, name: u32) -> Result<usize, u32> {
        match self.find(name) {
            Some(k) => {
                if let Some(x) = self.e.get_mut(k) {
                    x.busy = true;
                }
                Ok(k + 1)
            }
            None => self.complain(RI_ENOENT),
        }
    }

    /// 标记只读。
    pub fn mark_ro(&mut self, name: u32) -> Result<usize, u32> {
        match self.find(name) {
            Some(k) => {
                if let Some(x) = self.e.get_mut(k) {
                    x.ro = true;
                }
                Ok(k + 1)
            }
            None => self.complain(RI_ENOENT),
        }
    }

    /// 单件删除：`interactive` 决定是否提问，`force` 压过 `interactive`。
    pub fn remove_one(&mut self, name: u32, caller: u32, interactive: bool, force: bool) -> Result<usize, u32> {
        if name == 0 || caller == 0 {
            return self.complain(RI_EINVAL);
        }
        let slot = match self.find(name) {
            Some(v) => v,
            None => {
                if force {
                    return Ok(0);
                }
                return self.complain(RI_ENOENT);
            }
        };
        let ent = match self.e.get(slot) {
            Some(v) => *v,
            None => return self.complain(RI_ENOENT),
        };
        if interactive && !force {
            self.prompts = self.prompts.saturating_add(1);
            self.out_bytes = self.out_bytes.saturating_add(PROMPT_LEN);
            let a = self.take_answer();
            if a != AYES {
                self.kept = self.kept.saturating_add(1);
                return Ok(0);
            }
        }
        if ent.busy {
            return self.complain(RI_EBUSY);
        }
        if ent.ro {
            return self.complain(RI_EROFS);
        }
        if ent.kind == K_DIR {
            return self.complain(RI_EISDIR);
        }
        if ent.sticky && caller != ent.uid {
            return self.complain(RI_EPERM);
        }
        if !ent.writable {
            return self.complain(RI_EACCES);
        }
        if let Some(x) = self.e.get_mut(slot) {
            x.live = false;
        }
        self.deleted = self.deleted.saturating_add(1);
        Ok(1)
    }

    /// 批量：任一件真失败退出 1，否则 0。
    pub fn run(&mut self, names: &[u32], caller: u32, interactive: bool, force: bool) -> Result<usize, u32> {
        if caller == 0 {
            return self.complain(RI_EINVAL);
        }
        let mut failed = false;
        let mut i = 0usize;
        while i < names.len() {
            let n = match names.get(i) {
                Some(v) => *v,
                None => break,
            };
            if self.remove_one(n, caller, interactive, force).is_err() {
                failed = true;
            }
            i += 1;
        }
        if failed {
            Ok(1)
        } else {
            Ok(0)
        }
    }

    /// 不变式：名字唯一；提问数 = 保留数 + 已答 y 的删除尝试数。
    pub fn consistent(&self) -> bool {
        let mut k = 0usize;
        while k < CAP {
            let x = match self.e.get(k) {
                Some(v) => *v,
                None => return false,
            };
            if x.live {
                if x.name == 0 || x.uid == 0 || (x.kind != K_FILE && x.kind != K_DIR) {
                    return false;
                }
                let mut j = k + 1;
                while j < CAP {
                    match self.e.get(j) {
                        Some(o) => {
                            if o.live && o.name == x.name {
                                return false;
                            }
                        }
                        None => return false,
                    }
                    j += 1;
                }
            }
            k += 1;
        }
        if self.ai > self.an {
            return false;
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
            self.rejects = self.rejects.saturating_add(1);
            return Err(RI_ESLOT);
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
            return Err(RI_EBAL);
        }
        const VALS: [u32; 8] = [1, 2, 1, 0, 0, 2, 1, 1];
        const CODES: [u32; 6] =
            [RI_EINVAL, RI_ENOENT, RI_EISDIR, RI_EBUSY, RI_EROFS, RI_EPERM];
        let mut vi = 0usize;
        let mut ci = 0usize;
        let mut i = 0usize;
        while i < self.len {
            let s = match self.rows.get(i) {
                Some(v) => *v,
                None => return Err(RI_EBAL),
            };
            if s.slot as usize != i {
                return Err(RI_ESLOT);
            }
            if s.ok {
                if s.code != 0 {
                    return Err(RI_EVALUE);
                }
                match VALS.get(vi) {
                    Some(w) => {
                        if s.value != *w {
                            return Err(RI_EVALUE);
                        }
                    }
                    None => return Err(RI_EBAL),
                }
                vi += 1;
            } else {
                match CODES.get(ci) {
                    Some(w) => {
                        if s.code != *w {
                            return Err(RI_EVALUE);
                        }
                    }
                    None => return Err(RI_EBAL),
                }
                ci += 1;
            }
            i += 1;
        }
        if vi != 8 || ci != 6 {
            return Err(RI_EBAL);
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
            return Err(RI_EBAL);
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
            Err(RI_ESEAL)
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

/// 十四格账本：两件播种 → 回答 y 删 → 回答 n 留 → 强制吞缺 → 六类拒绝。
pub fn build_ledger() -> Ledger {
    let mut l = Ledger::new();
    let mut m = Inter::new();

    let _ = l.register(cell(0, OP_ADD, m.add(10, K_FILE, 100, true, false)));
    let _ = l.register(cell(1, OP_ADD, m.add(20, K_FILE, 100, true, false)));
    let _ = m.feed(AYES);
    let _ = l.register(cell(2, OP_REMOVE, m.remove_one(10, 100, true, false)));
    let _ = m.feed(ANO);
    let _ = l.register(cell(3, OP_REMOVE, m.remove_one(20, 100, true, false)));
    let _ = l.register(cell(4, OP_REMOVE, m.remove_one(99, 100, false, true)));
    let _ = l.register(word_cell(5, OP_PROMPT, Ok(m.prompt_count())));
    let _ = l.register(word_cell(6, OP_KEEP, Ok(m.kept_count())));
    let _ = l.register(cell(7, OP_RUN, Ok(m.live())));

    let mut a = Inter::new();
    let _ = l.register(cell(8, OP_REMOVE, a.remove_one(0, 100, true, false)));

    let mut b = Inter::new();
    let _ = l.register(cell(9, OP_REMOVE, b.remove_one(7, 100, true, false)));

    let mut c = Inter::new();
    let _ = c.add(30, K_DIR, 100, true, false);
    let _ = c.feed(AYES);
    let _ = l.register(cell(10, OP_REMOVE, c.remove_one(30, 100, true, false)));

    let mut d = Inter::new();
    let _ = d.add(40, K_FILE, 100, true, false);
    let _ = d.mark_busy(40);
    let _ = d.feed(AYES);
    let _ = l.register(cell(11, OP_REMOVE, d.remove_one(40, 100, true, false)));

    let mut e = Inter::new();
    let _ = e.add(50, K_FILE, 100, true, false);
    let _ = e.mark_ro(50);
    let _ = e.feed(AYES);
    let _ = l.register(cell(12, OP_REMOVE, e.remove_one(50, 100, true, false)));

    let mut f = Inter::new();
    let _ = f.add(60, K_FILE, 200, true, true);
    let _ = f.feed(AYES);
    let _ = l.register(cell(13, OP_REMOVE, f.remove_one(60, 100, true, false)));

    l
}

/// 四路注入。
pub fn inject_four() -> [bool; 4] {
    let mut out = [false; 4];

    let l = build_ledger();
    let rep = l.replay();
    let root = l.seal();
    out[0] = l.len() == SAMPLES && l.audit().is_ok() && rep == Ok(root) && root != 0;

    // 一路：y/n/y 逐件消费，删除两件保留一件。
    let mut a = Inter::new();
    let _ = a.add(1, K_FILE, 100, true, false);
    let _ = a.add(2, K_FILE, 100, true, false);
    let _ = a.add(3, K_FILE, 100, true, false);
    let _ = a.feed(AYES);
    let _ = a.feed(ANO);
    let _ = a.feed(AYES);
    let code = a.run(&[1, 2, 3], 100, true, false);
    out[1] = code == Ok(0)
        && a.deleted_count() == 2
        && a.kept_count() == 1
        && a.prompt_count() == 3
        && a.out_bytes() == PROMPT_LEN * 3
        && a.live() == 1
        && !a.eof()
        && a.consistent();

    // 二路：EOF 降级为不删；force 压过交互。
    let mut b = Inter::new();
    let _ = b.add(4, K_FILE, 100, true, false);
    let _ = b.add(5, K_FILE, 100, true, false);
    let eof_code = b.run(&[4, 5], 100, true, false);
    let eof_kept = b.kept_count();
    let closed = b.run(&[4], 100, true, false);
    let forced = b.remove_one(4, 100, true, true);
    out[2] = eof_code == Ok(0)
        && eof_kept == 2
        && b.eof()
        && b.prompt_count() == 3
        && closed == Ok(0)
        && forced == Ok(1)
        && b.live() == 1;

    // 三路：闸门次序（缺件不提问、y 后才发现目录）与容量。
    let mut q = Inter::new();
    let _ = q.add(10, K_DIR, 100, true, false);
    let _ = q.add(11, K_FILE, 100, false, false);
    let g1 = q.remove_one(0, 100, true, false) == Err(RI_EINVAL);
    let g2 = q.remove_one(99, 100, true, false) == Err(RI_ENOENT);
    let before_prompts = q.prompt_count();
    let _ = q.feed(AYES);
    let g3 = q.remove_one(10, 100, true, false) == Err(RI_EISDIR);
    let g4 = q.remove_one(11, 100, false, false) == Err(RI_EACCES);
    let mut cap = Inter::new();
    let mut i = 1u32;
    while (i as usize) <= CAP {
        let _ = cap.add(i, K_FILE, 100, true, false);
        i += 1;
    }
    let g5 = cap.add(99, K_FILE, 100, true, false) == Err(RI_EFULL);
    let _ = cap.feed(AYES);
    let g6 = cap.remove_one(3, 100, true, false) == Ok(1);
    let g7 = cap.add(99, K_FILE, 100, true, false) == Ok(CAP);
    out[3] = g1
        && g2
        && before_prompts == 0
        && g3
        && g4
        && g5
        && g6
        && g7
        && q.consistent()
        && cap.consistent();

    out
}

/// 十三项判据。
pub fn run_unx11395_checks() -> CheckSet {
    let mut s = CheckSet::new("unx11395.rm_interactive");
    let l = build_ledger();

    s.add_dyn(
        "RI1-十四格八允六拒",
        l.len() == SAMPLES && l.ok_count() == 8 && l.err_count() == 6 && l.audit().is_ok(),
        &format!("n={} ok={} err={}", l.len(), l.ok_count(), l.err_count()),
    );

    s.add(
        "RI2-两件播种",
        l.sample(0).map(|x| x.value) == Some(1) && l.sample(1).map(|x| x.value) == Some(2),
        "static",
    );

    s.add(
        "RI3-答y即删",
        l.sample(2).map(|x| x.value) == Some(1),
        "static",
    );

    s.add(
        "RI4-答n即留",
        l.sample(3).map(|x| x.value) == Some(0),
        "static",
    );

    s.add(
        "RI5-强制吞缺",
        l.sample(4).map(|x| x.value) == Some(0),
        "static",
    );

    s.add(
        "RI6-两次提问一次保留",
        l.sample(5).map(|x| x.value) == Some(2) && l.sample(6).map(|x| x.value) == Some(1),
        "static",
    );

    s.add(
        "RI7-余一件存活",
        l.sample(7).map(|x| x.value) == Some(1),
        "static",
    );

    s.add(
        "RI8-空名被拒",
        l.sample(8).map(|x| x.code) == Some(RI_EINVAL),
        "static",
    );

    s.add(
        "RI9-缺件不提问",
        l.sample(9).map(|x| x.code) == Some(RI_ENOENT),
        "static",
    );

    s.add(
        "RI10-答y后报目录错",
        l.sample(10).map(|x| x.code) == Some(RI_EISDIR),
        "static",
    );

    s.add(
        "RI11-忙与只读",
        l.sample(11).map(|x| x.code) == Some(RI_EBUSY)
            && l.sample(12).map(|x| x.code) == Some(RI_EROFS),
        "static",
    );

    s.add(
        "RI12-EOF降级且不误删",
        {
            let mut m = Inter::new();
            let _ = m.add(1, K_FILE, 100, true, false);
            let _ = m.add(2, K_FILE, 100, true, false);
            let code = m.run(&[1, 2], 100, true, false);
            code == Ok(0)
                && m.eof()
                && m.kept_count() == 2
                && m.deleted_count() == 0
                && m.live() == 2
                && m.prompt_count() == 2
                && m.consistent()
        },
        "static",
    );

    s.add(
        "RI13-冻结重放四注入",
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
        let s = run_unx11395_checks();
        assert_eq!(s.len(), 13);
        assert!(s.all_passed());
    }

    #[test]
    fn yes_deletes_no_keeps() {
        let mut m = Inter::new();
        let _ = m.add(1, K_FILE, 100, true, false);
        let _ = m.feed(AYES);
        assert_eq!(m.remove_one(1, 100, true, false), Ok(1));
        assert_eq!(m.prompt_count(), 1);
        assert_eq!(m.live(), 0);
        let _ = m.add(2, K_FILE, 100, true, false);
        let _ = m.feed(ANO);
        assert_eq!(m.remove_one(2, 100, true, false), Ok(0));
        assert_eq!(m.kept_count(), 1);
        assert_eq!(m.live(), 1);
    }

    #[test]
    fn missing_does_not_prompt() {
        let mut m = Inter::new();
        let _ = m.feed(AYES);
        assert_eq!(m.remove_one(9, 100, true, false), Err(RI_ENOENT));
        assert_eq!(m.prompt_count(), 0);
        assert_eq!(m.out_bytes(), MSG_LEN);
    }

    #[test]
    fn eof_degrades_to_no() {
        let mut m = Inter::new();
        let _ = m.add(1, K_FILE, 100, true, false);
        assert_eq!(m.remove_one(1, 100, true, false), Ok(0));
        assert!(m.eof());
        assert_eq!(m.kept_count(), 1);
        assert_eq!(m.live(), 1);
    }

    #[test]
    fn force_beats_interactive() {
        let mut m = Inter::new();
        let _ = m.add(1, K_FILE, 100, true, false);
        assert_eq!(m.remove_one(1, 100, true, true), Ok(1));
        assert_eq!(m.prompt_count(), 0);
        assert_eq!(m.live(), 0);
    }

    #[test]
    fn answer_after_prompt_only() {
        let mut m = Inter::new();
        let _ = m.add(1, K_DIR, 100, true, false);
        let _ = m.feed(AYES);
        assert_eq!(m.remove_one(1, 100, true, false), Err(RI_EISDIR));
        assert_eq!(m.prompt_count(), 1);
        // 回答已被消费：下一件会撞 EOF 而不是复用这次的 y。
        let _ = m.add(2, K_FILE, 100, true, false);
        assert_eq!(m.remove_one(2, 100, true, false), Ok(0));
        assert!(m.eof());
    }

    #[test]
    fn capacity_slots_reused() {
        let mut m = Inter::new();
        let mut i = 1u32;
        while (i as usize) <= CAP {
            assert!(m.add(i, K_FILE, 100, true, false).is_ok());
            i += 1;
        }
        assert_eq!(m.add(99, K_FILE, 100, true, false), Err(RI_EFULL));
        let _ = m.feed(AYES);
        assert_eq!(m.remove_one(1, 100, true, false), Ok(1));
        assert_eq!(m.add(99, K_FILE, 100, true, false), Ok(CAP));
        assert!(m.consistent());
    }
}
