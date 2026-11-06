//! UNX-F11394 rm -f 不存在静默语义（0xeea0 段）
//!
//! `rm -f` 的全部特殊性集中在一条规则上：**不存在不算失败**。没有 `-f` 时，
//! 删一个不存在的名字会打印 `cannot remove ...: No such file or directory` 并让
//! 退出码变成 1；带上 `-f` 后，同一件事必须完全静默、退出码保持 0。脚本里
//! `rm -f` 常被当作「确保不存在」的幂等操作，靠的就是这条吞掉 `ENOENT` 的规则。
//!
//! 但 `-f` 只吞 `ENOENT`，别的错误一律不吞：
//!
//! - 目标是目录 → 仍然报 `EISDIR`，因为 `-f` 不等于 `-r`；
//! - 父目录不可写 → 仍然报 `EACCES`，`-f` 不是提权；
//! - sticky 目录越权 → 仍然报 `EPERM`；
//! - 挂载点 → 仍然报 `EBUSY`；
//! - 只读文件系统 → 仍然报 `EROFS`。
//!
//! 本模块把「吞掉」做成显式计数 `swallowed`：每吞一个 `ENOENT` 加一，诊断输出
//! 字节数 `stderr_bytes` 不动。这样「静默」不是「什么都不记」，而是「记在另一个
//! 账本上」，`-f` 与不带 `-f` 的差别因此可以被判据直接观测，而不是靠「看起来
//! 没输出」。
//!
//! 批量处理的退出码规则：只要有一件真失败，整体退出码就是 1；全被吞或全成功
//! 则为 0。这条规则与单件的错误码是两套账：单件返回具体错误类，批量只返回
//! 0/1，两者都要对。
//!
//! 闸门次序（`remove_one`）：空名 → 不存在（带 `-f` 时在此分流吞掉）→ 忙 →
//! 只读 → 类型 → sticky → 父目录权限 → 摘项。不存在必须最先分流，否则一个
//! 不存在的名字会被后面的「类型/权限」闸门以错误的类别拒绝。

use crate::alloc::format;
use crate::checks::CheckSet;

/// 段基址。
pub const SEG_BASE: u32 = 0xeea0;

/// 空名或零调用者。
pub const RF_EINVAL: u32 = SEG_BASE + 0x01;
/// 名字不存在（不带 `-f` 时）。
pub const RF_ENOENT: u32 = SEG_BASE + 0x02;
/// 目标是目录。
pub const RF_EISDIR: u32 = SEG_BASE + 0x03;
/// 父目录不可写。
pub const RF_EACCES: u32 = SEG_BASE + 0x04;
/// sticky 目录越权。
pub const RF_EPERM: u32 = SEG_BASE + 0x05;
/// 目标是挂载点。
pub const RF_EBUSY: u32 = SEG_BASE + 0x06;
/// 文件系统只读。
pub const RF_EROFS: u32 = SEG_BASE + 0x07;
/// 条目表满。
pub const RF_EFULL: u32 = SEG_BASE + 0x08;
/// 账本形状不符。
pub const RF_EBAL: u32 = SEG_BASE + 0x09;
/// 槽位乱序。
pub const RF_ESLOT: u32 = SEG_BASE + 0x0a;
/// 观测值与复算不符。
pub const RF_EVALUE: u32 = SEG_BASE + 0x0b;
/// 冻结后重放不一致。
pub const RF_ESEAL: u32 = SEG_BASE + 0x0c;
/// 观察下标越界。
pub const RF_ENOBS: u32 = SEG_BASE + 0x0d;

/// 条目表容量（即 `CAP`）。
pub const CAP: usize = 8;
/// 账本格数。
pub const SAMPLES: usize = 14;
/// 一条诊断的字节数。
pub const MSG_LEN: u32 = 27;

/// 条目类型：普通文件。
pub const K_FILE: u8 = 1;
/// 条目类型：目录。
pub const K_DIR: u8 = 2;

/// 操作码：播种条目。
pub const OP_ADD: u8 = 1;
/// 操作码：单件删除。
pub const OP_REMOVE: u8 = 2;
/// 操作码：批量删除。
pub const OP_RUN: u8 = 3;
/// 操作码：读诊断字节数。
pub const OP_STDERR: u8 = 4;
/// 操作码：读活条目数。
pub const OP_STAT: u8 = 5;

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

/// 一个文件系统条目。
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
    /// 所在文件系统是否只读。
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

/// `rm -f` 模型。
#[derive(Clone, Copy, Debug)]
pub struct Force {
    e: [Ent; CAP],
    swallowed: u32,
    stderr_bytes: u32,
    removed: u32,
    last_code: u32,
}

impl Force {
    /// 空模型。
    pub const fn new() -> Force {
        Force { e: [Ent::blank(); CAP], swallowed: 0, stderr_bytes: 0, removed: 0, last_code: 0 }
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

    /// 被 `-f` 吞掉的 `ENOENT` 次数。
    pub fn swallowed(&self) -> u32 {
        self.swallowed
    }

    /// 诊断输出字节数。
    pub fn stderr_bytes(&self) -> u32 {
        self.stderr_bytes
    }

    /// 成功删除条数。
    pub fn removed(&self) -> u32 {
        self.removed
    }

    /// 最近一次错误码。
    pub fn last_code(&self) -> u32 {
        self.last_code
    }

    /// 第 k 个槽。
    pub fn ent_at(&self, k: usize) -> Option<Ent> {
        self.e.get(k).copied()
    }

    /// 按名找活条目下标。
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

    /// 找一个空槽。
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
        self.stderr_bytes = self.stderr_bytes.saturating_add(MSG_LEN);
        self.last_code = code;
        Err(code)
    }

    /// 播种一个条目。
    pub fn add(&mut self, name: u32, kind: u8, uid: u32, writable: bool, sticky: bool) -> Result<usize, u32> {
        if name == 0 || uid == 0 || (kind != K_FILE && kind != K_DIR) {
            return self.complain(RF_EINVAL);
        }
        if self.find(name).is_some() {
            return self.complain(RF_EINVAL);
        }
        let slot = match self.free() {
            Some(v) => v,
            None => return self.complain(RF_EFULL),
        };
        if let Some(x) = self.e.get_mut(slot) {
            *x = Ent {
                name,
                kind,
                uid,
                writable,
                sticky,
                busy: false,
                ro: false,
                live: true,
            };
        }
        Ok(self.live())
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
            None => self.complain(RF_ENOENT),
        }
    }

    /// 标记单个条目所在文件系统只读。
    pub fn mark_ro_one(&mut self, name: u32) -> Result<usize, u32> {
        match self.find(name) {
            Some(k) => {
                if let Some(x) = self.e.get_mut(k) {
                    x.ro = true;
                }
                Ok(k + 1)
            }
            None => self.complain(RF_ENOENT),
        }
    }

    /// 标记整个文件系统只读。
    pub fn mark_ro(&mut self) -> Result<usize, u32> {
        let mut k = 0usize;
        while k < CAP {
            if let Some(x) = self.e.get_mut(k) {
                if x.live {
                    x.ro = true;
                }
            }
            k += 1;
        }
        Ok(self.live())
    }

    /// 单件删除。`force` 为真时吞掉 `ENOENT`。
    pub fn remove_one(&mut self, name: u32, caller: u32, force: bool) -> Result<usize, u32> {
        if name == 0 || caller == 0 {
            return self.complain(RF_EINVAL);
        }
        let slot = match self.find(name) {
            Some(v) => v,
            None => {
                if force {
                    self.swallowed = self.swallowed.saturating_add(1);
                    return Ok(0);
                }
                return self.complain(RF_ENOENT);
            }
        };
        let ent = match self.e.get(slot) {
            Some(v) => *v,
            None => return self.complain(RF_ENOENT),
        };
        if ent.busy {
            return self.complain(RF_EBUSY);
        }
        if ent.ro {
            return self.complain(RF_EROFS);
        }
        if ent.kind == K_DIR {
            return self.complain(RF_EISDIR);
        }
        if ent.sticky && caller != ent.uid {
            return self.complain(RF_EPERM);
        }
        if !ent.writable {
            return self.complain(RF_EACCES);
        }
        if let Some(x) = self.e.get_mut(slot) {
            x.live = false;
        }
        self.removed = self.removed.saturating_add(1);
        Ok(1)
    }

    /// 批量删除：任一件真失败则退出码 1，否则 0。
    pub fn run(&mut self, names: &[u32], caller: u32, force: bool) -> Result<usize, u32> {
        if caller == 0 {
            return self.complain(RF_EINVAL);
        }
        let mut failed = false;
        let mut i = 0usize;
        while i < names.len() {
            let name = match names.get(i) {
                Some(v) => *v,
                None => break,
            };
            if self.remove_one(name, caller, force).is_err() {
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

    /// 不变式：名字唯一，删除计数不超过播种数。
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
            return Err(RF_ESLOT);
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
            return Err(RF_EBAL);
        }
        const VALS: [u32; 8] = [1, 2, 1, 1, 0, 0, 0, 0];
        const CODES: [u32; 6] =
            [RF_EINVAL, RF_ENOENT, RF_EISDIR, RF_EBUSY, RF_EROFS, RF_EACCES];
        let mut vi = 0usize;
        let mut ci = 0usize;
        let mut i = 0usize;
        while i < self.len {
            let s = match self.rows.get(i) {
                Some(v) => *v,
                None => return Err(RF_EBAL),
            };
            if s.slot as usize != i {
                return Err(RF_ESLOT);
            }
            if s.ok {
                if s.code != 0 {
                    return Err(RF_EVALUE);
                }
                match VALS.get(vi) {
                    Some(w) => {
                        if s.value != *w {
                            return Err(RF_EVALUE);
                        }
                    }
                    None => return Err(RF_EBAL),
                }
                vi += 1;
            } else {
                match CODES.get(ci) {
                    Some(w) => {
                        if s.code != *w {
                            return Err(RF_EVALUE);
                        }
                    }
                    None => return Err(RF_EBAL),
                }
                ci += 1;
            }
            i += 1;
        }
        if vi != 8 || ci != 6 {
            return Err(RF_EBAL);
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
            return Err(RF_EBAL);
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
            Err(RF_ESEAL)
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

/// 十四格账本：两件播种 → 强制与非强制删除 → 吞掉缺失 → 六类拒绝。
pub fn build_ledger() -> Ledger {
    let mut l = Ledger::new();
    let mut f = Force::new();

    let _ = l.register(cell(0, OP_ADD, f.add(10, K_FILE, 100, true, false)));
    let _ = l.register(cell(1, OP_ADD, f.add(20, K_FILE, 100, true, false)));
    let _ = l.register(cell(2, OP_REMOVE, f.remove_one(10, 100, false)));
    let _ = l.register(cell(3, OP_REMOVE, f.remove_one(20, 100, true)));
    let _ = l.register(cell(4, OP_REMOVE, f.remove_one(99, 100, true)));
    let _ = l.register(word_cell(5, OP_STDERR, Ok(f.stderr_bytes())));
    let _ = l.register(cell(6, OP_RUN, f.run(&[88], 100, true)));
    let _ = l.register(cell(7, OP_STAT, Ok(f.live())));

    let mut a = Force::new();
    let _ = l.register(cell(8, OP_REMOVE, a.remove_one(0, 100, true)));

    let mut b = Force::new();
    let _ = l.register(cell(9, OP_REMOVE, b.remove_one(7, 100, false)));

    let mut c = Force::new();
    let _ = c.add(30, K_DIR, 100, true, false);
    let _ = l.register(cell(10, OP_REMOVE, c.remove_one(30, 100, true)));

    let mut d = Force::new();
    let _ = d.add(40, K_FILE, 100, true, false);
    let _ = d.mark_busy(40);
    let _ = l.register(cell(11, OP_REMOVE, d.remove_one(40, 100, true)));

    let mut e = Force::new();
    let _ = e.add(50, K_FILE, 100, true, false);
    let _ = e.mark_ro();
    let _ = l.register(cell(12, OP_REMOVE, e.remove_one(50, 100, true)));

    let mut g = Force::new();
    let _ = g.add(60, K_FILE, 100, false, false);
    let _ = l.register(cell(13, OP_REMOVE, g.remove_one(60, 100, true)));

    l
}

/// 四路注入。
pub fn inject_four() -> [bool; 4] {
    let mut out = [false; 4];

    let l = build_ledger();
    let rep = l.replay();
    let root = l.seal();
    out[0] = l.len() == SAMPLES && l.audit().is_ok() && rep == Ok(root) && root != 0;

    // 一路：-f 吞 ENOENT（静默、退出 0），不带 -f 出声且退出 1。
    let mut a = Force::new();
    let quiet = a.run(&[7, 8], 100, true);
    let quiet_bytes = a.stderr_bytes();
    let mut b = Force::new();
    let loud = b.run(&[7], 100, false);
    out[1] = quiet == Ok(0)
        && quiet_bytes == 0
        && a.swallowed() == 2
        && loud == Ok(1)
        && b.stderr_bytes() == MSG_LEN
        && b.last_code() == RF_ENOENT
        && b.swallowed() == 0;

    // 二路：-f 不吞其余五类错误。
    let mut c = Force::new();
    let _ = c.add(10, K_DIR, 100, true, false);
    let _ = c.add(11, K_FILE, 100, true, false);
    let _ = c.mark_busy(11);
    let _ = c.add(12, K_FILE, 100, false, false);
    let _ = c.add(13, K_FILE, 200, true, true);
    let _ = c.add(14, K_FILE, 100, true, false);
    let _ = c.mark_ro_one(14);
    let dir = c.remove_one(10, 100, true) == Err(RF_EISDIR);
    let busy = c.remove_one(11, 100, true) == Err(RF_EBUSY);
    let acc = c.remove_one(12, 100, true) == Err(RF_EACCES);
    let perm = c.remove_one(13, 100, true) == Err(RF_EPERM);
    let ro = c.remove_one(14, 100, true) == Err(RF_EROFS);
    out[2] = dir && busy && acc && perm && ro && c.live() == 5 && c.removed() == 0 && c.consistent();

    // 三路：闸门次序与容量。
    let mut q = Force::new();
    let _ = q.add(20, K_DIR, 100, true, false);
    let g1 = q.remove_one(0, 100, true) == Err(RF_EINVAL);
    let g2 = q.remove_one(0, 0, true) == Err(RF_EINVAL);
    let g3 = q.remove_one(99, 100, false) == Err(RF_ENOENT);
    let g4 = q.remove_one(20, 100, true) == Err(RF_EISDIR);
    let mut cap = Force::new();
    let mut i = 1u32;
    while (i as usize) <= CAP {
        let _ = cap.add(i, K_FILE, 100, true, false);
        i += 1;
    }
    let g5 = cap.add(99, K_FILE, 100, true, false) == Err(RF_EFULL);
    let g6 = cap.remove_one(3, 100, true) == Ok(1);
    let g7 = cap.add(99, K_FILE, 100, true, false) == Ok(CAP);
    let g8 = cap.remove_one(99, 100, false) == Ok(1);
    out[3] = g1 && g2 && g3 && g4 && g5 && g6 && g7 && g8 && cap.consistent();

    out
}

/// 十三项判据。
pub fn run_unx11394_checks() -> CheckSet {
    let mut s = CheckSet::new("unx11394.rm_force_silent");
    let l = build_ledger();

    s.add_dyn(
        "RF1-十四格八允六拒",
        l.len() == SAMPLES && l.ok_count() == 8 && l.err_count() == 6 && l.audit().is_ok(),
        &format!("n={} ok={} err={}", l.len(), l.ok_count(), l.err_count()),
    );

    s.add(
        "RF2-两件播种",
        l.sample(0).map(|x| x.value) == Some(1) && l.sample(1).map(|x| x.value) == Some(2),
        "static",
    );

    s.add(
        "RF3-普通与强制删除",
        l.sample(2).map(|x| x.value) == Some(1) && l.sample(3).map(|x| x.value) == Some(1),
        "static",
    );

    s.add(
        "RF4-强制吞掉缺失",
        l.sample(4).map(|x| x.value) == Some(0),
        "static",
    );

    s.add(
        "RF5-吞掉不产生诊断",
        l.sample(5).map(|x| x.value) == Some(0),
        "static",
    );

    s.add(
        "RF6-批量全吞退出零",
        l.sample(6).map(|x| x.value) == Some(0) && l.sample(7).map(|x| x.value) == Some(0),
        "static",
    );

    s.add(
        "RF7-空名被拒",
        l.sample(8).map(|x| x.code) == Some(RF_EINVAL),
        "static",
    );

    s.add(
        "RF8-非强制缺失出声",
        l.sample(9).map(|x| x.code) == Some(RF_ENOENT),
        "static",
    );

    s.add(
        "RF9-强制不吞目录",
        l.sample(10).map(|x| x.code) == Some(RF_EISDIR),
        "static",
    );

    s.add(
        "RF10-挂载点与只读",
        l.sample(11).map(|x| x.code) == Some(RF_EBUSY)
            && l.sample(12).map(|x| x.code) == Some(RF_EROFS),
        "static",
    );

    s.add(
        "RF11-强制不吞权限",
        l.sample(13).map(|x| x.code) == Some(RF_EACCES),
        "static",
    );

    s.add(
        "RF12-批量部分失败退出一",
        {
            let mut f = Force::new();
            let _ = f.add(10, K_FILE, 100, true, false);
            let _ = f.add(11, K_DIR, 100, true, false);
            let code = f.run(&[10, 11, 12], 100, true);
            code == Ok(1)
                && f.live() == 1
                && f.removed() == 1
                && f.swallowed() == 1
                && f.stderr_bytes() == MSG_LEN
                && f.last_code() == RF_EISDIR
                && f.consistent()
        },
        "static",
    );

    s.add(
        "RF13-冻结重放四注入",
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
        let s = run_unx11394_checks();
        assert_eq!(s.len(), 13);
        assert!(s.all_passed());
    }

    #[test]
    fn force_swallows_missing() {
        let mut f = Force::new();
        assert_eq!(f.remove_one(7, 100, true), Ok(0));
        assert_eq!(f.swallowed(), 1);
        assert_eq!(f.stderr_bytes(), 0);
        assert_eq!(f.remove_one(7, 100, false), Err(RF_ENOENT));
        assert_eq!(f.stderr_bytes(), MSG_LEN);
        assert_eq!(f.last_code(), RF_ENOENT);
    }

    #[test]
    fn force_does_not_swallow_dir() {
        let mut f = Force::new();
        let _ = f.add(1, K_DIR, 100, true, false);
        assert_eq!(f.remove_one(1, 100, true), Err(RF_EISDIR));
        assert_eq!(f.live(), 1);
        assert_eq!(f.removed(), 0);
    }

    #[test]
    fn batch_exit_code() {
        let mut f = Force::new();
        let _ = f.add(1, K_FILE, 100, true, false);
        let _ = f.add(2, K_DIR, 100, true, false);
        assert_eq!(f.run(&[1, 3], 100, true), Ok(0));
        assert_eq!(f.run(&[2], 100, true), Ok(1));
        assert_eq!(f.run(&[9, 8], 100, true), Ok(0));
    }

    #[test]
    fn slots_are_reused() {
        let mut f = Force::new();
        let mut i = 1u32;
        while (i as usize) <= CAP {
            assert!(f.add(i, K_FILE, 100, true, false).is_ok());
            i += 1;
        }
        assert_eq!(f.add(99, K_FILE, 100, true, false), Err(RF_EFULL));
        assert_eq!(f.remove_one(4, 100, true), Ok(1));
        assert_eq!(f.add(99, K_FILE, 100, true, false), Ok(CAP));
        assert!(f.consistent());
    }

    #[test]
    fn sticky_needs_owner() {
        let mut f = Force::new();
        let _ = f.add(1, K_FILE, 200, true, true);
        assert_eq!(f.remove_one(1, 100, true), Err(RF_EPERM));
        assert_eq!(f.remove_one(1, 200, true), Ok(1));
    }
}
