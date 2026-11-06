//! UNX-F11330 退出与 mmap 清账联签（0xeaa0 段）
//!
//! `exit_group(2)` 落地时地址空间整块消失，但消失不是无条件抹账：凡是
//! `MAP_SHARED` 且被写过的映射，页缓存里的脏页必须先回写文件，否则进程一退，
//! 别处再看到的就是旧内容。`MAP_PRIVATE` 相反——它的脏页属于进程私有副本，
//! 退出时直接丢弃，文件一个字节都不该动。
//!
//! 本模块用一个固定容量的地址空间（基址 `0x10000`，跨度 `0x10000`）把两件事
//! 绑在一本账上：映射按页向上取整、首尾相接分配；写入按 VMA 命中判定权限与
//! 共享性；退出时先把共享脏映射逐条回写并计数，再整表清空。清账后任何一次
//! `write`/`mmap` 都必须被拒——这一条「退出后不再受理」是整套语义的收口。
//!
//! 闸门次序：
//! - `mmap`：空文件/零长/非法共享位 → 已退出 → 表满 → 空间不足；
//! - `write`：零长 → 已退出 → 无映射（`ESRCH`）→ 无写权限（`EACCES`）；
//! - `exit_group`：已退出。

use crate::alloc::format;
use crate::checks::CheckSet;

/// 段基址。
pub const SEG_BASE: u32 = 0xeaa0;

/// 空文件、零长或非法共享位。
pub const MM_EINVAL: u32 = SEG_BASE + 0x01;
/// 地址不落在任何映射内。
pub const MM_ESRCH: u32 = SEG_BASE + 0x02;
/// 该映射没有写权限。
pub const MM_EACCES: u32 = SEG_BASE + 0x03;
/// 地址空间不足。
pub const MM_ELIMIT: u32 = SEG_BASE + 0x04;
/// 映射表满。
pub const MM_EFULL: u32 = SEG_BASE + 0x05;
/// 已退出或重复退出。
pub const MM_ERUN: u32 = SEG_BASE + 0x06;
/// 账本形状不符。
pub const MM_EBAL: u32 = SEG_BASE + 0x07;
/// 槽位乱序。
pub const MM_EORDER: u32 = SEG_BASE + 0x08;
/// 观测值与复算不符。
pub const MM_EMISMATCH: u32 = SEG_BASE + 0x09;
/// 冻结后重放不一致。
pub const MM_ESEAL: u32 = SEG_BASE + 0x0a;
/// 观察下标越界。
pub const MM_ENOBS: u32 = SEG_BASE + 0x0b;
/// 回写账溢出。
pub const MM_ESTALE: u32 = SEG_BASE + 0x0c;
/// 映射区间自相矛盾。
pub const MM_EPART: u32 = SEG_BASE + 0x0d;

/// 地址空间基址。
pub const BASE: u32 = 0x1_0000;
/// 地址空间跨度。
pub const SPAN: u32 = 0x1_0000;
/// 页大小。
pub const PAGE: u32 = 0x1000;
/// 映射表容量。
pub const CAP: usize = 8;
/// 账本格数。
pub const SAMPLES: usize = 14;

/// 共享映射。
pub const MAP_SHARED: u8 = 0x01;
/// 私有映射。
pub const MAP_PRIVATE: u8 = 0x02;
/// 可写保护。
pub const PROT_WRITE: u8 = 0x04;

/// 操作码：映射。
pub const OP_MMAP: u8 = 1;
/// 操作码：写入。
pub const OP_WRITE: u8 = 2;
/// 操作码：退出清账。
pub const OP_EXIT: u8 = 3;
/// 操作码：读计数。
pub const OP_COUNT: u8 = 4;
/// 操作码：读回写字节。
pub const OP_WB: u8 = 5;

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

/// 一段映射。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Vma {
    /// 起始地址。
    pub start: u32,
    /// 页对齐长度。
    pub len: u32,
    /// 共享/私有与保护位。
    pub flags: u8,
    /// 所属文件号。
    pub file: u32,
    /// 是否被写过。
    pub dirty: bool,
}

impl Vma {
    /// 空映射。
    pub const fn blank() -> Vma {
        Vma { start: 0, len: 0, flags: 0, file: 0, dirty: false }
    }

    /// 是否共享。
    pub fn is_shared(&self) -> bool {
        self.flags & MAP_SHARED != 0
    }

    /// 是否可写。
    pub fn writable(&self) -> bool {
        self.flags & PROT_WRITE != 0
    }

    /// 是否覆盖 `[addr, end)`。
    pub fn covers(&self, addr: u32, end: u32) -> bool {
        let stop = match self.start.checked_add(self.len) {
            Some(v) => v,
            None => return false,
        };
        addr >= self.start && end <= stop
    }
}

/// 地址空间 + 回写账。
#[derive(Clone, Copy, Debug)]
pub struct VmaBook {
    tab: [Vma; CAP],
    n: usize,
    next: u32,
    exited: bool,
    flushes: u32,
    dirty_pages: u32,
    wb_file: [u32; CAP],
    wb_len: [u32; CAP],
    wbn: usize,
}

impl VmaBook {
    /// 空地址空间。
    pub const fn new() -> VmaBook {
        VmaBook {
            tab: [Vma::blank(); CAP],
            n: 0,
            next: BASE,
            exited: false,
            flushes: 0,
            dirty_pages: 0,
            wb_file: [0; CAP],
            wb_len: [0; CAP],
            wbn: 0,
        }
    }

    /// 当前映射条数。
    pub fn vma_count(&self) -> usize {
        self.n
    }

    /// 是否已退出。
    pub fn is_exited(&self) -> bool {
        self.exited
    }

    /// 退出时回写的映射条数。
    pub fn flushes(&self) -> u32 {
        self.flushes
    }

    /// 累计脏页数。
    pub fn dirty_pages(&self) -> u32 {
        self.dirty_pages
    }

    /// 第 i 段映射。
    pub fn vma_at(&self, i: usize) -> Option<Vma> {
        if i >= self.n {
            return None;
        }
        self.tab.get(i).copied()
    }

    /// 按文件号查回写字节。
    pub fn wb_bytes(&self, file: u32) -> u32 {
        let mut i = 0usize;
        while i < self.wbn {
            let f = match self.wb_file.get(i) {
                Some(v) => *v,
                None => return 0,
            };
            if f == file {
                return match self.wb_len.get(i) {
                    Some(v) => *v,
                    None => 0,
                };
            }
            i += 1;
        }
        0
    }

    /// 回写账条数。
    pub fn wb_count(&self) -> usize {
        self.wbn
    }

    /// 记一笔回写。
    fn add_wb(&mut self, file: u32, len: u32) -> Result<(), u32> {
        let mut i = 0usize;
        while i < self.wbn {
            let f = match self.wb_file.get(i) {
                Some(v) => *v,
                None => return Err(MM_ESTALE),
            };
            if f == file {
                if let Some(slot) = self.wb_len.get_mut(i) {
                    *slot = slot.saturating_add(len);
                }
                return Ok(());
            }
            i += 1;
        }
        if self.wbn >= CAP {
            return Err(MM_ESTALE);
        }
        if let Some(slot) = self.wb_file.get_mut(self.wbn) {
            *slot = file;
        }
        if let Some(slot) = self.wb_len.get_mut(self.wbn) {
            *slot = len;
        }
        self.wbn += 1;
        Ok(())
    }

    /// 建立映射，返回起始地址。
    pub fn mmap(&mut self, file: u32, len: u32, flags: u8) -> Result<u32, u32> {
        let share = flags & (MAP_SHARED | MAP_PRIVATE);
        if file == 0 || len == 0 || share == 0 || share == (MAP_SHARED | MAP_PRIVATE) {
            return Err(MM_EINVAL);
        }
        if self.exited {
            return Err(MM_ERUN);
        }
        if self.n >= CAP {
            return Err(MM_EFULL);
        }
        let pages = len.saturating_add(PAGE - 1) / PAGE;
        let need = match pages.checked_mul(PAGE) {
            Some(v) => v,
            None => return Err(MM_ELIMIT),
        };
        let end = match self.next.checked_add(need) {
            Some(v) => v,
            None => return Err(MM_ELIMIT),
        };
        let limit = match BASE.checked_add(SPAN) {
            Some(v) => v,
            None => return Err(MM_ELIMIT),
        };
        if end > limit {
            return Err(MM_ELIMIT);
        }
        if let Some(slot) = self.tab.get_mut(self.n) {
            *slot = Vma { start: self.next, len: need, flags, file, dirty: false };
        }
        self.n += 1;
        self.next = end;
        Ok(self.next - need)
    }

    /// 往映射里写 `len` 字节，返回触及的页数。
    pub fn write(&mut self, addr: u32, len: u32) -> Result<u32, u32> {
        if len == 0 {
            return Err(MM_EINVAL);
        }
        if self.exited {
            return Err(MM_ERUN);
        }
        let end = match addr.checked_add(len) {
            Some(v) => v,
            None => return Err(MM_ESRCH),
        };
        let mut hit: Option<usize> = None;
        let mut i = 0usize;
        while i < self.n {
            match self.tab.get(i) {
                Some(v) => {
                    if v.covers(addr, end) {
                        hit = Some(i);
                        break;
                    }
                }
                None => return Err(MM_EPART),
            }
            i += 1;
        }
        let idx = match hit {
            Some(k) => k,
            None => return Err(MM_ESRCH),
        };
        let v = match self.tab.get(idx) {
            Some(v) => *v,
            None => return Err(MM_EPART),
        };
        if !v.writable() {
            return Err(MM_EACCES);
        }
        let pages = len.saturating_add(PAGE - 1) / PAGE;
        if v.is_shared() {
            if let Some(slot) = self.tab.get_mut(idx) {
                slot.dirty = true;
            }
            self.dirty_pages = self.dirty_pages.saturating_add(pages);
        }
        Ok(pages)
    }

    /// 退出清账：共享脏映射逐条回写，随后整表清空。
    pub fn exit_group(&mut self) -> Result<usize, u32> {
        if self.exited {
            return Err(MM_ERUN);
        }
        let mut flushed = 0usize;
        let mut i = 0usize;
        while i < self.n {
            let v = match self.tab.get(i) {
                Some(v) => *v,
                None => return Err(MM_EPART),
            };
            if v.is_shared() && v.dirty {
                self.add_wb(v.file, v.len)?;
                flushed += 1;
            }
            i += 1;
        }
        self.flushes = flushed as u32;
        self.n = 0;
        self.next = BASE;
        self.exited = true;
        Ok(flushed)
    }

    /// 退出后仍残留的映射条数（正确实现恒为 0）。
    pub fn residual(&self) -> usize {
        self.n
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
            return Err(MM_EORDER);
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
            return Err(MM_EBAL);
        }
        const VALS: [u32; 8] = [0x1_0000, 0x1_1000, 1, 1, 1, 0, 0x1000, 0];
        const CODES: [u32; 6] =
            [MM_EINVAL, MM_ESRCH, MM_EACCES, MM_ELIMIT, MM_EFULL, MM_ERUN];
        let mut vi = 0usize;
        let mut ci = 0usize;
        let mut i = 0usize;
        while i < self.len {
            let s = match self.rows.get(i) {
                Some(v) => *v,
                None => return Err(MM_EBAL),
            };
            if s.slot as usize != i {
                return Err(MM_EORDER);
            }
            if s.ok {
                if s.code != 0 {
                    return Err(MM_EMISMATCH);
                }
                match VALS.get(vi) {
                    Some(w) => {
                        if s.value != *w {
                            return Err(MM_EMISMATCH);
                        }
                    }
                    None => return Err(MM_EBAL),
                }
                vi += 1;
            } else {
                match CODES.get(ci) {
                    Some(w) => {
                        if s.code != *w {
                            return Err(MM_EMISMATCH);
                        }
                    }
                    None => return Err(MM_EBAL),
                }
                ci += 1;
            }
            i += 1;
        }
        if vi != 8 || ci != 6 {
            return Err(MM_EBAL);
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
            return Err(MM_EBAL);
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
            Err(MM_ESEAL)
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

/// 十四格账本：两段映射 → 共享/私有各写一次 → 退出清账 → 读计数与回写 → 各类拒绝。
pub fn build_ledger() -> Ledger {
    let mut l = Ledger::new();
    let mut b = VmaBook::new();

    let _ = l.register(word_cell(0, OP_MMAP, b.mmap(11, 0x1000, MAP_SHARED | PROT_WRITE)));
    let _ = l.register(word_cell(1, OP_MMAP, b.mmap(12, 0x2000, MAP_PRIVATE | PROT_WRITE)));
    let _ = l.register(word_cell(2, OP_WRITE, b.write(0x1_0000, 256)));
    let _ = l.register(word_cell(3, OP_WRITE, b.write(0x1_1000, 256)));
    let _ = l.register(cell(4, OP_EXIT, b.exit_group()));
    let _ = l.register(cell(5, OP_COUNT, Ok(b.vma_count())));
    let _ = l.register(word_cell(6, OP_WB, Ok(b.wb_bytes(11))));
    let _ = l.register(word_cell(7, OP_WB, Ok(b.wb_bytes(12))));

    let mut e = VmaBook::new();
    let _ = l.register(word_cell(8, OP_MMAP, e.mmap(0, 0x1000, MAP_SHARED | PROT_WRITE)));
    let _ = l.register(word_cell(9, OP_WRITE, e.write(0x9000, 16)));

    let mut ro = VmaBook::new();
    let _ = ro.mmap(14, 0x1000, MAP_SHARED);
    let _ = l.register(word_cell(10, OP_WRITE, ro.write(0x1_0000, 16)));

    let mut big = VmaBook::new();
    let _ = l.register(word_cell(11, OP_MMAP, big.mmap(15, 0x2_0000, MAP_SHARED | PROT_WRITE)));

    let mut fill = VmaBook::new();
    let mut i = 1u32;
    while (i as usize) <= CAP {
        let _ = fill.mmap(100 + i, PAGE, MAP_PRIVATE);
        i += 1;
    }
    let _ = l.register(word_cell(12, OP_MMAP, fill.mmap(200, PAGE, MAP_PRIVATE)));
    let _ = l.register(cell(13, OP_EXIT, b.exit_group()));

    l
}

/// 四路注入。
pub fn inject_four() -> [bool; 4] {
    let mut out = [false; 4];

    let l = build_ledger();
    let rep = l.replay();
    let root = l.seal();
    out[0] = l.len() == SAMPLES && l.audit().is_ok() && rep == Ok(root) && root != 0;

    // 一路：共享回写、私有不回写。
    let mut a = VmaBook::new();
    let s = a.mmap(1, 0x2000, MAP_SHARED | PROT_WRITE);
    let p = a.mmap(2, 0x1000, MAP_PRIVATE | PROT_WRITE);
    let w1 = a.write(0x1_0000, 64);
    let w2 = a.write(0x1_2000, 64);
    let ex = a.exit_group();
    out[1] = s == Ok(0x1_0000)
        && p == Ok(0x1_2000)
        && w1 == Ok(1)
        && w2 == Ok(1)
        && ex == Ok(1)
        && a.wb_bytes(1) == 0x2000
        && a.wb_bytes(2) == 0
        && a.wb_count() == 1;

    // 二路：退出后全清且不再受理。
    let mut c = VmaBook::new();
    let _ = c.mmap(3, 0x1000, MAP_SHARED | PROT_WRITE);
    let _ = c.mmap(4, 0x1000, MAP_PRIVATE | PROT_WRITE);
    let _ = c.write(0x1_0000, 8);
    let ex = c.exit_group();
    out[2] = ex == Ok(1)
        && c.residual() == 0
        && c.vma_count() == 0
        && c.is_exited()
        && c.write(0x1_0000, 8) == Err(MM_ERUN)
        && c.mmap(5, 0x1000, MAP_PRIVATE) == Err(MM_ERUN)
        && c.exit_group() == Err(MM_ERUN);

    // 三路：闸门。
    let mut g = VmaBook::new();
    let g1 = g.mmap(0, PAGE, MAP_SHARED) == Err(MM_EINVAL);
    let g2 = g.mmap(6, 0, MAP_SHARED) == Err(MM_EINVAL);
    let g3 = g.mmap(6, PAGE, 0) == Err(MM_EINVAL);
    let g4 = g.mmap(6, PAGE, MAP_SHARED | MAP_PRIVATE) == Err(MM_EINVAL);
    let g5 = g.write(0x9000, 8) == Err(MM_ESRCH);
    let g6 = g.mmap(7, PAGE, MAP_SHARED) == Ok(0x1_0000);
    let g7 = g.write(0x1_0000, 8) == Err(MM_EACCES);
    let g8 = g.mmap(8, 0x2_0000, MAP_PRIVATE) == Err(MM_ELIMIT);
    out[3] = g1 && g2 && g3 && g4 && g5 && g6 && g7 && g8;

    out
}

/// 十三项判据。
pub fn run_unx11330_checks() -> CheckSet {
    let mut s = CheckSet::new("unx11330.exit_mmap");
    let l = build_ledger();

    s.add_dyn(
        "VM1-十四格八允六拒",
        l.len() == SAMPLES && l.ok_count() == 8 && l.err_count() == 6 && l.audit().is_ok(),
        &format!("n={} ok={} err={}", l.len(), l.ok_count(), l.err_count()),
    );

    s.add(
        "VM2-映射地址首尾相接",
        l.sample(0).map(|x| x.value) == Some(0x1_0000)
            && l.sample(1).map(|x| x.value) == Some(0x1_1000),
        "static",
    );

    s.add(
        "VM3-共享写记脏页",
        l.sample(2).map(|x| x.value) == Some(1),
        "static",
    );

    s.add(
        "VM4-私有写不记脏页",
        l.sample(3).map(|x| x.value) == Some(1),
        "static",
    );

    s.add(
        "VM5-退出只回写共享脏映射",
        l.sample(4).map(|x| x.value) == Some(1),
        "static",
    );

    s.add(
        "VM6-退出后VMA全清",
        l.sample(5).map(|x| x.value) == Some(0),
        "static",
    );

    s.add(
        "VM7-整段回写字节",
        l.sample(6).map(|x| x.value) == Some(0x1000),
        "static",
    );

    s.add(
        "VM8-私有文件零回写",
        l.sample(7).map(|x| x.value) == Some(0),
        "static",
    );

    s.add(
        "VM9-空文件被拒",
        l.sample(8).map(|x| x.code) == Some(MM_EINVAL),
        "static",
    );

    s.add(
        "VM10-越界写与只读写",
        l.sample(9).map(|x| x.code) == Some(MM_ESRCH)
            && l.sample(10).map(|x| x.code) == Some(MM_EACCES),
        "static",
    );

    s.add(
        "VM11-空间不足与表满",
        l.sample(11).map(|x| x.code) == Some(MM_ELIMIT)
            && l.sample(12).map(|x| x.code) == Some(MM_EFULL),
        "static",
    );

    s.add(
        "VM12-重复退出被拒",
        l.sample(13).map(|x| x.code) == Some(MM_ERUN),
        "static",
    );

    s.add(
        "VM13-冻结重放四注入",
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
        let s = run_unx11330_checks();
        assert_eq!(s.len(), 13);
        assert!(s.all_passed());
    }

    #[test]
    fn mappings_are_page_aligned_and_adjacent() {
        let mut b = VmaBook::new();
        assert_eq!(b.mmap(1, 100, MAP_PRIVATE), Ok(BASE));
        assert_eq!(b.mmap(2, 1, MAP_PRIVATE), Ok(BASE + PAGE));
        assert_eq!(b.mmap(3, 2 * PAGE, MAP_PRIVATE), Ok(BASE + 2 * PAGE));
        assert_eq!(b.vma_count(), 3);
        match b.vma_at(0) {
            Some(v) => assert_eq!(v.len, PAGE),
            None => panic!(),
        }
        assert!(b.vma_at(3).is_none());
    }

    #[test]
    fn shared_dirty_is_written_back_once() {
        let mut b = VmaBook::new();
        let _ = b.mmap(7, 2 * PAGE, MAP_SHARED | PROT_WRITE);
        assert_eq!(b.write(BASE, 16), Ok(1));
        assert_eq!(b.write(BASE + PAGE, 16), Ok(1));
        assert_eq!(b.dirty_pages(), 2);
        assert_eq!(b.exit_group(), Ok(1));
        assert_eq!(b.wb_bytes(7), 2 * PAGE);
        assert_eq!(b.exit_group(), Err(MM_ERUN));
    }

    #[test]
    fn private_never_reaches_writeback() {
        let mut b = VmaBook::new();
        let _ = b.mmap(9, PAGE, MAP_PRIVATE | PROT_WRITE);
        assert_eq!(b.write(BASE, PAGE), Ok(1));
        assert_eq!(b.exit_group(), Ok(0));
        assert_eq!(b.wb_bytes(9), 0);
        assert_eq!(b.wb_count(), 0);
        assert_eq!(b.residual(), 0);
    }

    #[test]
    fn gates() {
        let mut b = VmaBook::new();
        assert_eq!(b.mmap(0, PAGE, MAP_SHARED), Err(MM_EINVAL));
        assert_eq!(b.mmap(1, 0, MAP_SHARED), Err(MM_EINVAL));
        assert_eq!(b.mmap(1, PAGE, 0), Err(MM_EINVAL));
        assert_eq!(b.write(BASE, 0), Err(MM_EINVAL));
        assert_eq!(b.write(BASE, 8), Err(MM_ESRCH));
        assert_eq!(b.mmap(1, PAGE, MAP_SHARED), Ok(BASE));
        assert_eq!(b.write(BASE, 8), Err(MM_EACCES));
        assert_eq!(b.mmap(2, SPAN, MAP_PRIVATE), Err(MM_ELIMIT));
    }

    #[test]
    fn capacity_and_limit() {
        let mut b = VmaBook::new();
        let mut i = 0usize;
        while i < CAP {
            assert!(b.mmap(50 + i as u32, PAGE, MAP_PRIVATE).is_ok());
            i += 1;
        }
        assert_eq!(b.mmap(99, PAGE, MAP_PRIVATE), Err(MM_EFULL));
        let mut big = VmaBook::new();
        assert_eq!(big.mmap(1, SPAN + PAGE, MAP_PRIVATE), Err(MM_ELIMIT));
    }

    #[test]
    fn freeze_blocks_writes() {
        let mut l = build_ledger();
        assert!(l.freeze().is_ok());
        assert!(l.register(Sample::blank()).is_err());
        assert_eq!(l.rejects(), 1);
    }
}
