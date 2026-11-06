//! UNX-F11353 procfs 读取错误路径（0xec10 段）
//!
//! procfs 的读取不是普通的文件读取：内容是内核在 `read` 时才现场生成的快照，
//! 因此每一种失败都发生在不同的时点上，诊断工具必须能把它们区分开，否则会把
//! 「进程刚好退出」误判成「权限不足」，或者把「生成器出错」误判成「文件为空」。
//!
//! 本模块把读取生命周期中会出现的失败按**时点**分层：
//!
//! - 打开前：目标号从未注册（`ENOENT`）；进程已不可 dump（`EACCES`）；
//!   句柄表满（`EFULL`）。
//! - 打开后、读取中：进程在两次读之间消失（`ENOENT`——链接还在，内容没了）；
//!   生成器在后续读取时失败（`EIO`——已经吐出过一部分，再吐就断）。
//! - 读取本身：缓冲区为零（`EINVAL`）；尾段不足是整个长度（短读，成功）；
//!   已经到底（返回 0，即 EOF）。
//! - 定位：不可定位的流（`ESPIPE`）；越过文件尾（`EINVAL`）；恰好等于尾
//!   （允许，代表 EOF 位置）。
//! - 关闭后再用（`EBADF`）。
//!
//! 关键区分点：
//!
//! 1. **短读不是错误**：剩余长度不足请求长度时按剩余长度返回；只有剩余为零才
//!    返回 0。把短读当错误会让 `head`、`sed` 之类的工具提前退出。
//! 2. **消失与无权不同**：打开时的 `EACCES` 说明内核拒绝了这次访问；打开之后
//!    的 `ENOENT` 说明目标在读取窗口里没了。前者重试无用，后者重试应当重新打开。
//! 3. **EIO 只在半途出现**：第一次读取已经成功、偏移非零之后再失败，才叫生成
//!    器中途出错；偏移为零时的失败与打开失败同义。
//!
//! 闸门次序：
//! - `file`：零号 → 重复 → 表满；
//! - `open`：未注册 → 不可 dump → 句柄满；
//! - `read`：坏句柄 → 中途消失 → 半途生成失败 → 零缓冲 → 短读/EOF；
//! - `seek`：坏句柄 → 不可定位 → 越过尾。

use crate::alloc::format;
use crate::checks::CheckSet;

/// 段基址。
pub const SEG_BASE: u32 = 0xec10;

/// 参数非法。
pub const PE_EINVAL: u32 = SEG_BASE + 0x01;
/// 同号镜像重复注册。
pub const PE_EDUP: u32 = SEG_BASE + 0x02;
/// 目标不存在（未注册，或读取窗口内消失）。
pub const PE_ENOENT: u32 = SEG_BASE + 0x03;
/// 不可 dump（打开时拒绝）。
pub const PE_EACCES: u32 = SEG_BASE + 0x04;
/// 生成器半途失败。
pub const PE_EIO: u32 = SEG_BASE + 0x05;
/// 不可定位的流。
pub const PE_ESPIPE: u32 = SEG_BASE + 0x06;
/// 句柄坏（未开或已关）。
pub const PE_EBADF: u32 = SEG_BASE + 0x07;
/// 表满。
pub const PE_EFULL: u32 = SEG_BASE + 0x08;
/// 账本形状不符。
pub const PE_EBAL: u32 = SEG_BASE + 0x09;
/// 槽位乱序。
pub const PE_EORDER: u32 = SEG_BASE + 0x0a;
/// 观测值与复算不符。
pub const PE_EMISMATCH: u32 = SEG_BASE + 0x0b;
/// 冻结后重放不一致。
pub const PE_ESEAL: u32 = SEG_BASE + 0x0c;
/// 观察下标越界。
pub const PE_ENOBS: u32 = SEG_BASE + 0x0d;

/// 镜像表容量。
pub const FCAP: usize = 6;
/// 句柄表容量。
pub const HCAP: usize = 4;
/// 账本格数。
pub const SAMPLES: usize = 14;

/// 操作码：注册镜像。
pub const OP_FILE: u8 = 1;
/// 操作码：打开。
pub const OP_OPEN: u8 = 2;
/// 操作码：读取。
pub const OP_READ: u8 = 3;
/// 操作码：定位。
pub const OP_SEEK: u8 = 4;
/// 操作码：关闭。
pub const OP_CLOSE: u8 = 5;

/// 镜像标志：打开时不可 dump。
pub const F_NODUMP: u8 = 1;
/// 镜像标志：第二次及以后的读取失败。
pub const F_MIDEIO: u8 = 2;
/// 镜像标志：打开后消失。
pub const F_VANISH: u8 = 4;
/// 合法标志掩码。
pub const F_VALID: u8 = F_NODUMP | F_MIDEIO | F_VANISH;

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

/// 一个可读镜像。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct File {
    /// 镜像号。
    pub id: u32,
    /// 生成后的长度。
    pub size: u32,
    /// 是否可定位。
    pub seekable: bool,
    /// 行为标志。
    pub flags: u8,
}

impl File {
    /// 是否拒绝打开。
    pub fn nodump(&self) -> bool {
        self.flags & F_NODUMP != 0
    }

    /// 是否半途失败。
    pub fn mid_eio(&self) -> bool {
        self.flags & F_MIDEIO != 0
    }

    /// 是否打开后消失。
    pub fn vanish(&self) -> bool {
        self.flags & F_VANISH != 0
    }
}

/// 一个读取句柄。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Handle {
    /// 句柄号。
    pub fd: u32,
    /// 镜像号。
    pub file: u32,
    /// 当前偏移。
    pub off: u32,
    /// 是否仍打开。
    pub open: bool,
}

/// 读取器。
#[derive(Clone, Copy, Debug)]
pub struct Reader {
    files: [File; FCAP],
    fn_: usize,
    hs: [Handle; HCAP],
    hn: usize,
    files_opened: usize,
    reads: usize,
    bytes: u32,
    seeks: usize,
    eofs: usize,
}

impl Reader {
    /// 空读取器。
    pub const fn new() -> Reader {
        Reader {
            files: [File { id: 0, size: 0, seekable: false, flags: 0 }; FCAP],
            fn_: 0,
            hs: [Handle { fd: 0, file: 0, off: 0, open: false }; HCAP],
            hn: 0,
            files_opened: 0,
            reads: 0,
            bytes: 0,
            seeks: 0,
            eofs: 0,
        }
    }

    /// 镜像条数。
    pub fn file_count(&self) -> usize {
        self.fn_
    }

    /// 句柄条数（含已关闭）。
    pub fn handle_count(&self) -> usize {
        self.hn
    }

    /// 成功读取次数。
    pub fn reads(&self) -> usize {
        self.reads
    }

    /// 累计读取字节。
    pub fn bytes_read(&self) -> u32 {
        self.bytes
    }

    /// 定位次数。
    pub fn seeks(&self) -> usize {
        self.seeks
    }

    /// 读到 EOF 的次数。
    pub fn eofs(&self) -> usize {
        self.eofs
    }

    /// 第 i 个镜像。
    pub fn file_at(&self, i: usize) -> Option<File> {
        if i >= self.fn_ {
            return None;
        }
        self.files.get(i).copied()
    }

    /// 第 i 个句柄。
    pub fn handle_at(&self, i: usize) -> Option<Handle> {
        if i >= self.hn {
            return None;
        }
        self.hs.get(i).copied()
    }

    /// 句柄是否仍打开。
    pub fn is_open(&self, fd: u32) -> bool {
        match self.find_handle(fd) {
            Some(at) => match self.hs.get(at) {
                Some(h) => h.open,
                None => false,
            },
            None => false,
        }
    }

    /// 句柄当前偏移。
    pub fn offset_of(&self, fd: u32) -> Result<u32, u32> {
        match self.find_handle(fd) {
            Some(at) => match self.hs.get(at) {
                Some(h) => {
                    if h.open {
                        Ok(h.off)
                    } else {
                        Err(PE_EBADF)
                    }
                }
                None => Err(PE_EBADF),
            },
            None => Err(PE_EBADF),
        }
    }

    /// 按号找镜像。
    fn find_file(&self, id: u32) -> Option<usize> {
        let mut i = 0usize;
        while i < self.fn_ {
            match self.files.get(i) {
                Some(f) => {
                    if f.id == id {
                        return Some(i);
                    }
                }
                None => return None,
            }
            i += 1;
        }
        None
    }

    /// 按号找句柄。
    fn find_handle(&self, fd: u32) -> Option<usize> {
        let mut i = 0usize;
        while i < self.hn {
            match self.hs.get(i) {
                Some(h) => {
                    if h.fd == fd {
                        return Some(i);
                    }
                }
                None => return None,
            }
            i += 1;
        }
        None
    }

    /// 注册一个镜像。
    pub fn file(&mut self, id: u32, size: u32, seekable: bool, flags: u8) -> Result<usize, u32> {
        if id == 0 || flags & !F_VALID != 0 {
            return Err(PE_EINVAL);
        }
        if self.find_file(id).is_some() {
            return Err(PE_EDUP);
        }
        if self.fn_ >= FCAP {
            return Err(PE_EFULL);
        }
        if let Some(slot) = self.files.get_mut(self.fn_) {
            *slot = File { id, size, seekable, flags };
        }
        self.fn_ += 1;
        Ok(self.fn_)
    }

    /// 打开一个镜像，得到句柄号（从 1 起）。
    pub fn open(&mut self, id: u32) -> Result<usize, u32> {
        let at = match self.find_file(id) {
            Some(v) => v,
            None => return Err(PE_ENOENT),
        };
        let f = match self.files.get(at) {
            Some(v) => *v,
            None => return Err(PE_ENOENT),
        };
        if f.nodump() {
            return Err(PE_EACCES);
        }
        if self.hn >= HCAP {
            return Err(PE_EFULL);
        }
        let fd = (self.hn as u32) + 1;
        if let Some(slot) = self.hs.get_mut(self.hn) {
            *slot = Handle { fd, file: f.id, off: 0, open: true };
        }
        self.hn += 1;
        self.files_opened += 1;
        Ok(fd as usize)
    }

    /// 从当前偏移读取至多 `want` 字节。
    pub fn read(&mut self, fd: u32, want: u32) -> Result<usize, u32> {
        let at = match self.find_handle(fd) {
            Some(v) => v,
            None => return Err(PE_EBADF),
        };
        let h = match self.hs.get(at) {
            Some(v) => *v,
            None => return Err(PE_EBADF),
        };
        if !h.open {
            return Err(PE_EBADF);
        }
        let fat = match self.find_file(h.file) {
            Some(v) => v,
            None => return Err(PE_ENOENT),
        };
        let f = match self.files.get(fat) {
            Some(v) => *v,
            None => return Err(PE_ENOENT),
        };
        if f.vanish() {
            return Err(PE_ENOENT);
        }
        if f.mid_eio() && h.off > 0 {
            return Err(PE_EIO);
        }
        if want == 0 {
            return Err(PE_EINVAL);
        }
        let remaining = f.size.saturating_sub(h.off);
        let n = if want < remaining { want } else { remaining };
        if n == 0 {
            self.eofs += 1;
            return Ok(0);
        }
        if let Some(slot) = self.hs.get_mut(at) {
            slot.off = slot.off.saturating_add(n);
        }
        self.bytes = self.bytes.saturating_add(n);
        self.reads += 1;
        Ok(n as usize)
    }

    /// 定位到绝对偏移。
    pub fn seek(&mut self, fd: u32, off: u32) -> Result<usize, u32> {
        let at = match self.find_handle(fd) {
            Some(v) => v,
            None => return Err(PE_EBADF),
        };
        let h = match self.hs.get(at) {
            Some(v) => *v,
            None => return Err(PE_EBADF),
        };
        if !h.open {
            return Err(PE_EBADF);
        }
        let fat = match self.find_file(h.file) {
            Some(v) => v,
            None => return Err(PE_ENOENT),
        };
        let f = match self.files.get(fat) {
            Some(v) => *v,
            None => return Err(PE_ENOENT),
        };
        if !f.seekable {
            return Err(PE_ESPIPE);
        }
        if off > f.size {
            return Err(PE_EINVAL);
        }
        if let Some(slot) = self.hs.get_mut(at) {
            slot.off = off;
        }
        self.seeks += 1;
        Ok(off as usize)
    }

    /// 关闭句柄。
    pub fn close(&mut self, fd: u32) -> Result<usize, u32> {
        let at = match self.find_handle(fd) {
            Some(v) => v,
            None => return Err(PE_EBADF),
        };
        let h = match self.hs.get(at) {
            Some(v) => *v,
            None => return Err(PE_EBADF),
        };
        if !h.open {
            return Err(PE_EBADF);
        }
        if let Some(slot) = self.hs.get_mut(at) {
            slot.open = false;
        }
        Ok(fd as usize)
    }

    /// 不变式自检。
    pub fn consistent(&self) -> bool {
        if self.fn_ > FCAP || self.hn > HCAP {
            return false;
        }
        let mut i = 0usize;
        while i < self.fn_ {
            let f = match self.files.get(i) {
                Some(v) => *v,
                None => return false,
            };
            if f.id == 0 {
                return false;
            }
            let mut j = i + 1;
            while j < self.fn_ {
                match self.files.get(j) {
                    Some(g) => {
                        if g.id == f.id {
                            return false;
                        }
                    }
                    None => return false,
                }
                j += 1;
            }
            i += 1;
        }
        let mut k = 0usize;
        while k < self.hn {
            let h = match self.hs.get(k) {
                Some(v) => *v,
                None => return false,
            };
            if h.fd != (k as u32) + 1 {
                return false;
            }
            if self.find_file(h.file).is_none() {
                return false;
            }
            if let Some(fat) = self.find_file(h.file) {
                match self.files.get(fat) {
                    Some(f) => {
                        if h.off > f.size {
                            return false;
                        }
                    }
                    None => return false,
                }
            }
            k += 1;
        }
        true
    }

    /// 打开次数。
    pub fn opens(&self) -> usize {
        self.files_opened
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
            return Err(PE_EORDER);
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
            return Err(PE_EBAL);
        }
        const VALS: [u32; 8] = [1, 2, 1, 40, 80, 20, 0, 1];
        const CODES: [u32; 6] =
            [PE_EINVAL, PE_EDUP, PE_EIO, PE_ESPIPE, PE_EBADF, PE_EACCES];
        let mut vi = 0usize;
        let mut ci = 0usize;
        let mut i = 0usize;
        while i < self.len {
            let s = match self.rows.get(i) {
                Some(v) => *v,
                None => return Err(PE_EBAL),
            };
            if s.slot as usize != i {
                return Err(PE_EORDER);
            }
            if s.ok {
                if s.code != 0 {
                    return Err(PE_EMISMATCH);
                }
                match VALS.get(vi) {
                    Some(w) => {
                        if s.value != *w {
                            return Err(PE_EMISMATCH);
                        }
                    }
                    None => return Err(PE_EBAL),
                }
                vi += 1;
            } else {
                match CODES.get(ci) {
                    Some(w) => {
                        if s.code != *w {
                            return Err(PE_EMISMATCH);
                        }
                    }
                    None => return Err(PE_EBAL),
                }
                ci += 1;
            }
            i += 1;
        }
        if vi != 8 || ci != 6 {
            return Err(PE_EBAL);
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
            return Err(PE_EBAL);
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
            Err(PE_ESEAL)
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

/// 十四格账本：注册两条 → 读满 → 定位 → 短读 → EOF → 关闭 → 各时点失败。
pub fn build_ledger() -> Ledger {
    let mut l = Ledger::new();
    let mut r = Reader::new();

    let _ = l.register(cell(0, OP_FILE, r.file(1, 100, true, 0)));
    let _ = l.register(cell(1, OP_FILE, r.file(2, 0, false, 0)));
    let _ = l.register(cell(2, OP_OPEN, r.open(1)));
    let _ = l.register(cell(3, OP_READ, r.read(1, 40)));
    let _ = l.register(cell(4, OP_SEEK, r.seek(1, 80)));
    let _ = l.register(cell(5, OP_READ, r.read(1, 40)));
    let _ = l.register(cell(6, OP_READ, r.read(1, 40)));
    let _ = l.register(cell(7, OP_CLOSE, r.close(1)));

    let mut z = Reader::new();
    let _ = l.register(cell(8, OP_FILE, z.file(0, 10, true, 0)));

    let mut d = Reader::new();
    let _ = d.file(3, 10, true, 0);
    let _ = l.register(cell(9, OP_FILE, d.file(3, 10, true, 0)));

    let mut m = Reader::new();
    let _ = m.file(4, 50, true, F_MIDEIO);
    let _ = m.open(4);
    let _ = m.read(1, 10);
    let _ = l.register(cell(10, OP_READ, m.read(1, 10)));

    let mut p = Reader::new();
    let _ = p.file(5, 0, false, 0);
    let _ = p.open(5);
    let _ = l.register(cell(11, OP_SEEK, p.seek(1, 0)));

    let mut b = Reader::new();
    let _ = l.register(cell(12, OP_READ, b.read(99, 10)));

    let mut a = Reader::new();
    let _ = a.file(6, 10, true, F_NODUMP);
    let _ = l.register(cell(13, OP_OPEN, a.open(6)));

    l
}

/// 四路注入。
pub fn inject_four() -> [bool; 4] {
    let mut out = [false; 4];

    let l = build_ledger();
    let rep = l.replay();
    let root = l.seal();
    out[0] = l.len() == SAMPLES && l.audit().is_ok() && rep == Ok(root) && root != 0;

    // 一路：正常路径——短读、EOF、回定位、累计字节。
    let mut a = Reader::new();
    let _ = a.file(1, 10, true, 0);
    let f = a.open(1);
    let r1 = a.read(1, 4);
    let r2 = a.read(1, 4);
    let r3 = a.read(1, 4);
    let eof = a.read(1, 4);
    let back = a.seek(1, 0);
    let full = a.read(1, 10);
    let closed = a.close(1);
    out[1] = f == Ok(1)
        && r1 == Ok(4)
        && r2 == Ok(4)
        && r3 == Ok(2)
        && eof == Ok(0)
        && back == Ok(0)
        && full == Ok(10)
        && closed == Ok(1)
        && a.reads() == 4
        && a.bytes_read() == 20
        && a.eofs() == 1
        && a.seeks() == 1
        && a.consistent();

    // 二路：四类失败各自可辨（消失 / 无权 / 半途 / 坏句柄）。
    let mut v = Reader::new();
    let _ = v.file(7, 8, true, F_VANISH);
    let _ = v.open(7);
    let gone = v.read(1, 1) == Err(PE_ENOENT);
    let mut w = Reader::new();
    let _ = w.file(8, 8, true, F_NODUMP);
    let denied = w.open(8) == Err(PE_EACCES);
    let mut g = Reader::new();
    let _ = g.file(9, 8, true, F_MIDEIO);
    let _ = g.open(9);
    let first = g.read(1, 1) == Ok(1);
    let broken = g.read(1, 1) == Err(PE_EIO);
    let mut h = Reader::new();
    let bad = h.read(1, 1) == Err(PE_EBADF);
    let _ = h.file(10, 8, true, 0);
    let _ = h.open(10);
    let _ = h.close(1);
    let dead = h.read(1, 1) == Err(PE_EBADF) && h.close(1) == Err(PE_EBADF);
    out[2] = gone && denied && first && broken && bad && dead;

    // 三路：闸门与容量。
    let mut q = Reader::new();
    let q1 = q.file(0, 1, true, 0) == Err(PE_EINVAL);
    let q2 = q.file(1, 1, true, 0x80) == Err(PE_EINVAL);
    let q3 = q.file(1, 1, true, 0) == Ok(1);
    let q4 = q.file(1, 2, true, 0) == Err(PE_EDUP);
    let q5 = q.open(1) == Ok(1);
    let q6 = q.read(1, 0) == Err(PE_EINVAL);
    let q7 = q.seek(1, 2) == Err(PE_EINVAL);
    let q8 = q.seek(1, 1) == Ok(1);
    let mut cap = Reader::new();
    let mut i = 1u32;
    while (i as usize) <= FCAP {
        let _ = cap.file(i, 1, true, 0);
        i += 1;
    }
    let q9 = cap.file(99, 1, true, 0) == Err(PE_EFULL);
    let mut hc = Reader::new();
    let _ = hc.file(1, 8, true, 0);
    let mut k = 0usize;
    let mut ok = true;
    while k < HCAP {
        if hc.open(1) != Ok(k + 1) {
            ok = false;
        }
        k += 1;
    }
    let q10 = ok && hc.open(1) == Err(PE_EFULL);
    out[3] = q1 && q2 && q3 && q4 && q5 && q6 && q7 && q8 && q9 && q10 && q.consistent();

    out
}

/// 十三项判据。
pub fn run_unx11353_checks() -> CheckSet {
    let mut s = CheckSet::new("unx11353.procfs_err_paths");
    let l = build_ledger();

    s.add_dyn(
        "PE1-十四格八允六拒",
        l.len() == SAMPLES && l.ok_count() == 8 && l.err_count() == 6 && l.audit().is_ok(),
        &format!("n={} ok={} err={}", l.len(), l.ok_count(), l.err_count()),
    );

    s.add(
        "PE2-两条镜像注册",
        l.sample(0).map(|x| x.value) == Some(1) && l.sample(1).map(|x| x.value) == Some(2),
        "static",
    );

    s.add(
        "PE3-打开取得句柄",
        l.sample(2).map(|x| x.value) == Some(1),
        "static",
    );

    s.add(
        "PE4-首读取满四成",
        l.sample(3).map(|x| x.value) == Some(40),
        "static",
    );

    s.add(
        "PE5-定位到八十字节",
        l.sample(4).map(|x| x.value) == Some(80),
        "static",
    );

    s.add(
        "PE6-尾段短读不报错",
        l.sample(5).map(|x| x.value) == Some(20),
        "static",
    );

    s.add(
        "PE7-到底返回零",
        l.sample(6).map(|x| x.value) == Some(0),
        "static",
    );

    s.add(
        "PE8-关闭返回句柄号",
        l.sample(7).map(|x| x.value) == Some(1),
        "static",
    );

    s.add(
        "PE9-零号与重复被拒",
        l.sample(8).map(|x| x.code) == Some(PE_EINVAL)
            && l.sample(9).map(|x| x.code) == Some(PE_EDUP),
        "static",
    );

    s.add(
        "PE10-半途生成失败为EIO",
        l.sample(10).map(|x| x.code) == Some(PE_EIO),
        "static",
    );

    s.add(
        "PE11-不可定位为ESPIPE",
        l.sample(11).map(|x| x.code) == Some(PE_ESPIPE),
        "static",
    );

    s.add(
        "PE12-坏句柄与无权分离",
        l.sample(12).map(|x| x.code) == Some(PE_EBADF)
            && l.sample(13).map(|x| x.code) == Some(PE_EACCES),
        "static",
    );

    s.add(
        "PE13-冻结重放四注入",
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
        let s = run_unx11353_checks();
        assert_eq!(s.len(), 13);
        assert!(s.all_passed());
    }

    #[test]
    fn short_read_is_success() {
        let mut r = Reader::new();
        assert_eq!(r.file(1, 10, true, 0), Ok(1));
        assert_eq!(r.open(1), Ok(1));
        assert_eq!(r.read(1, 7), Ok(7));
        assert_eq!(r.read(1, 7), Ok(3));
        assert_eq!(r.read(1, 7), Ok(0));
        assert_eq!(r.reads(), 2);
        assert_eq!(r.bytes_read(), 10);
        assert_eq!(r.eofs(), 1);
    }

    #[test]
    fn vanish_differs_from_denied() {
        let mut v = Reader::new();
        assert_eq!(v.file(2, 4, true, F_VANISH), Ok(1));
        assert_eq!(v.open(2), Ok(1));
        assert_eq!(v.read(1, 1), Err(PE_ENOENT));
        let mut d = Reader::new();
        assert_eq!(d.file(3, 4, true, F_NODUMP), Ok(1));
        assert_eq!(d.open(3), Err(PE_EACCES));
    }

    #[test]
    fn mid_read_failure_only_after_progress() {
        let mut r = Reader::new();
        assert_eq!(r.file(4, 16, true, F_MIDEIO), Ok(1));
        assert_eq!(r.open(4), Ok(1));
        assert_eq!(r.read(1, 4), Ok(4));
        assert_eq!(r.read(1, 4), Err(PE_EIO));
        assert_eq!(r.seek(1, 0), Ok(0));
        assert_eq!(r.read(1, 4), Ok(4));
    }

    #[test]
    fn seek_bounds_and_spipe() {
        let mut r = Reader::new();
        assert_eq!(r.file(5, 8, true, 0), Ok(1));
        assert_eq!(r.file(6, 8, false, 0), Ok(2));
        assert_eq!(r.open(5), Ok(1));
        assert_eq!(r.open(6), Ok(2));
        assert_eq!(r.seek(1, 8), Ok(8));
        assert_eq!(r.seek(1, 9), Err(PE_EINVAL));
        assert_eq!(r.seek(2, 0), Err(PE_ESPIPE));
        assert_eq!(r.offset_of(1), Ok(8));
    }

    #[test]
    fn capacity_is_exact() {
        let mut r = Reader::new();
        let mut i = 1u32;
        while (i as usize) <= FCAP {
            assert_eq!(r.file(i, 1, true, 0), Ok(i as usize));
            i += 1;
        }
        assert_eq!(r.file(99, 1, true, 0), Err(PE_EFULL));
        let mut k = 0usize;
        while k < HCAP {
            assert_eq!(r.open(1), Ok(k + 1));
            k += 1;
        }
        assert_eq!(r.open(1), Err(PE_EFULL));
        assert!(r.consistent());
    }

    #[test]
    fn closed_handle_is_bad() {
        let mut r = Reader::new();
        assert_eq!(r.file(7, 4, true, 0), Ok(1));
        assert_eq!(r.open(7), Ok(1));
        assert_eq!(r.read(1, 2), Ok(2));
        assert_eq!(r.close(1), Ok(1));
        assert_eq!(r.read(1, 2), Err(PE_EBADF));
        assert_eq!(r.seek(1, 0), Err(PE_EBADF));
        assert_eq!(r.offset_of(1), Err(PE_EBADF));
        assert!(!r.is_open(1));
        assert!(r.consistent());
    }
}
