//! UNX-F11526 stat 元数据快照（0xf1f0 段）
//!
//! `stat` 输出的一行里有三种性质不同的信息，本模块把它们分开建模：
//!
//! 1. **类型与权限**：`mode` 的高四位是类型，低九位是 `rwx` 三组。类型决定
//!    首字符（`- d l c b p s`），权限决定后九位；两者都非法时先报类型错；
//! 2. **身份与规模**：`ino/uid/gid/nlink/size/blocks` 是可比较的整数，其中
//!    `blocks` 必须等于 `ceil(size/512)`，否则说明该快照与文件系统口径不符；
//! 3. **时间三账**：`ctime ≤ mtime`，状态变更不可能晚于内容变更；违反即
//!    `ST_ETIME`。
//!
//! 快照差异用位掩码表达：`MODE/OWN/SIZE/TIME/LINK/INO` 各占一位，两份快照
//! 完全相同时返回 `ST_EDIFF`，而不是给一个空掩码 —— 后者容易被调用者当成
//! 「有一点不同」。
//!
//! 闸门次序：
//!
//! - `check`：类型非法 → 时间倒序 → 块数不符；
//! - `perm_string`：类型非法 → 保留位非法；
//! - `diff`：任一侧未通过自检 → 无差异。
//!
//! `read_body` 只对普通文件开放：目录、设备、FIFO 的「内容」没有字节流语义，
//! 把它们的 `size` 当长度读会得到不可解释的数据。

use crate::alloc::string::String;
use crate::checks::CheckSet;

/// 段基址。
pub const SEG_BASE: u32 = 0xf1f0;

/// 参数非法。
pub const ST_EINVAL: u32 = SEG_BASE + 0x01;
/// 类型非法。
pub const ST_EMODE: u32 = SEG_BASE + 0x02;
/// 时间倒序。
pub const ST_ETIME: u32 = SEG_BASE + 0x03;
/// 块数与大小不符。
pub const ST_EBLOCK: u32 = SEG_BASE + 0x04;
/// 两份快照无差异。
pub const ST_EDIFF: u32 = SEG_BASE + 0x05;
/// 非普通文件。
pub const ST_ENOTP: u32 = SEG_BASE + 0x06;
/// 大小或身份越界。
pub const ST_ERANGE: u32 = SEG_BASE + 0x07;
/// 保留位非零。
pub const ST_ERESV: u32 = SEG_BASE + 0x08;
/// 账本形状不符。
pub const ST_EBAL: u32 = SEG_BASE + 0x09;
/// 槽位乱序。
pub const ST_ESLOT: u32 = SEG_BASE + 0x0a;
/// 观测值与复算不符。
pub const ST_EVALUE: u32 = SEG_BASE + 0x0b;
/// 冻结后重放不一致。
pub const ST_ESEAL: u32 = SEG_BASE + 0x0c;
/// 观察下标越界。
pub const ST_ENOBS: u32 = SEG_BASE + 0x0d;

/// 账本格数。
pub const SAMPLES: usize = 14;
/// 文件大小上限。
pub const MAX_SIZE: u32 = 0x7fff_ffff;

/// 类型：普通文件。
pub const IF_REG: u16 = 0o100000;
/// 类型：目录。
pub const IF_DIR: u16 = 0o040000;
/// 类型：符号链接。
pub const IF_LNK: u16 = 0o120000;
/// 类型：字符设备。
pub const IF_CHR: u16 = 0o020000;
/// 类型：块设备。
pub const IF_BLK: u16 = 0o060000;
/// 类型：FIFO。
pub const IF_FIFO: u16 = 0o010000;
/// 类型：套接字。
pub const IF_SOCK: u16 = 0o140000;

/// 差异位：类型与权限。
pub const D_MODE: u32 = 0x01;
/// 差异位：uid/gid。
pub const D_OWN: u32 = 0x02;
/// 差异位：大小。
pub const D_SIZE: u32 = 0x04;
/// 差异位：时间三账。
pub const D_TIME: u32 = 0x08;
/// 差异位：链接数。
pub const D_LINK: u32 = 0x10;
/// 差异位：inode 号。
pub const D_INO: u32 = 0x20;

/// 字段选择：inode。
pub const F_INO: u8 = 1;
/// 字段选择：链接数。
pub const F_NLINK: u8 = 2;
/// 字段选择：uid。
pub const F_UID: u8 = 3;
/// 字段选择：gid。
pub const F_GID: u8 = 4;
/// 字段选择：大小。
pub const F_SIZE: u8 = 5;
/// 字段选择：块数。
pub const F_BLOCKS: u8 = 6;

/// 操作码：自检。
pub const OP_CHECK: u8 = 1;
/// 操作码：权限串。
pub const OP_PERM: u8 = 2;
/// 操作码：差异。
pub const OP_DIFF: u8 = 3;
/// 操作码：字段。
pub const OP_FIELD: u8 = 4;
/// 操作码：读取。
pub const OP_READ: u8 = 5;

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

/// 元数据快照。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Meta {
    /// inode 号。
    pub ino: u32,
    /// 类型与权限。
    pub mode: u16,
    /// 属主。
    pub uid: u16,
    /// 属组。
    pub gid: u16,
    /// 字节数。
    pub size: u32,
    /// 硬链接数。
    pub nlink: u16,
    /// 访问时间。
    pub atime: u32,
    /// 修改时间。
    pub mtime: u32,
    /// 状态变更时间。
    pub ctime: u32,
    /// 512 字节块数。
    pub blocks: u32,
}

impl Meta {
    /// 自检。
    pub fn check(&self) -> Result<(), u32> {
        if !is_type(self.mode) {
            return Err(ST_EMODE);
        }
        if self.ctime > self.mtime || self.mtime > self.atime {
            return Err(ST_ETIME);
        }
        if self.size > MAX_SIZE {
            return Err(ST_ERANGE);
        }
        if self.nlink == 0 {
            return Err(ST_ERANGE);
        }
        if self.blocks != self.expect_blocks() {
            return Err(ST_EBLOCK);
        }
        Ok(())
    }

    /// 按大小复算块数。
    pub fn expect_blocks(&self) -> u32 {
        (self.size.saturating_add(511)) / 512
    }

    /// 文件类型。
    pub fn file_type(&self) -> u16 {
        self.mode & 0o170000
    }

    /// 是否普通文件。
    pub fn is_regular(&self) -> bool {
        self.file_type() == IF_REG
    }

    /// 类型首字符。
    pub fn type_char(&self) -> Result<u8, u32> {
        Ok(match self.file_type() {
            IF_REG => b'-',
            IF_DIR => b'd',
            IF_LNK => b'l',
            IF_CHR => b'c',
            IF_BLK => b'b',
            IF_FIFO => b'p',
            IF_SOCK => b's',
            _ => return Err(ST_EMODE),
        })
    }

    /// 权限串（九位）。
    pub fn perm_string(&self) -> Result<String, u32> {
        let _ = self.type_char()?;
        let bits = self.mode & 0o777;
        let mut out = String::new();
        let mut i = 0u32;
        while i < 9 {
            let bit = 8 - i;
            let set = (bits >> bit) & 1 == 1;
            let ch = match i % 3 {
                0 => {
                    if set {
                        b'r'
                    } else {
                        b'-'
                    }
                }
                1 => {
                    if set {
                        b'w'
                    } else {
                        b'-'
                    }
                }
                _ => {
                    if set {
                        b'x'
                    } else {
                        b'-'
                    }
                }
            };
            out.push(ch as char);
            i += 1;
        }
        Ok(out)
    }

    /// 取一个整数字段。
    pub fn field(&self, which: u8) -> Result<u32, u32> {
        match which {
            F_INO => Ok(self.ino),
            F_NLINK => Ok(self.nlink as u32),
            F_UID => Ok(self.uid as u32),
            F_GID => Ok(self.gid as u32),
            F_SIZE => Ok(self.size),
            F_BLOCKS => Ok(self.blocks),
            _ => Err(ST_EINVAL),
        }
    }

    /// 读取内容的前提检查。
    pub fn read_body(&self) -> Result<u32, u32> {
        if !self.is_regular() {
            return Err(ST_ENOTP);
        }
        Ok(self.size)
    }

    /// 与另一份快照的差异掩码。
    pub fn diff(&self, other: &Meta) -> Result<u32, u32> {
        self.check()?;
        other.check()?;
        let mut mask = 0u32;
        if self.mode != other.mode {
            mask |= D_MODE;
        }
        if self.uid != other.uid || self.gid != other.gid {
            mask |= D_OWN;
        }
        if self.size != other.size {
            mask |= D_SIZE;
        }
        if self.atime != other.atime || self.mtime != other.mtime || self.ctime != other.ctime {
            mask |= D_TIME;
        }
        if self.nlink != other.nlink {
            mask |= D_LINK;
        }
        if self.ino != other.ino {
            mask |= D_INO;
        }
        if mask == 0 {
            return Err(ST_EDIFF);
        }
        Ok(mask)
    }
}

/// 类型半字节是否属于已知类型。
fn is_type(mode: u16) -> bool {
    matches!(
        mode & 0o170000,
        IF_REG | IF_DIR | IF_LNK | IF_CHR | IF_BLK | IF_FIFO | IF_SOCK
    )
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

/// `Result<(), u32>` → 样本，成功记 1。
fn unit_cell(slot: u8, op: u8, r: Result<(), u32>) -> Sample {
    match r {
        Ok(()) => Sample { slot, op, ok: true, code: 0, value: 1 },
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
            return Err(ST_ESLOT);
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
            return Err(ST_EBAL);
        }
        const VALS: [u32; 8] = [45, 9, 4096, 1000, D_SIZE, 8, 1, 0o644];
        const CODES: [u32; 6] = [ST_EMODE, ST_ETIME, ST_EBLOCK, ST_EDIFF, ST_ENOTP, ST_EINVAL];
        let mut vi = 0usize;
        let mut ci = 0usize;
        let mut i = 0usize;
        while i < self.len {
            let s = match self.rows.get(i) {
                Some(v) => *v,
                None => return Err(ST_EBAL),
            };
            if s.slot as usize != i {
                return Err(ST_ESLOT);
            }
            if s.ok {
                if s.code != 0 {
                    return Err(ST_EVALUE);
                }
                match VALS.get(vi) {
                    Some(w) => {
                        if s.value != *w {
                            return Err(ST_EVALUE);
                        }
                    }
                    None => return Err(ST_EBAL),
                }
                vi += 1;
            } else {
                match CODES.get(ci) {
                    Some(w) => {
                        if s.code != *w {
                            return Err(ST_EVALUE);
                        }
                    }
                    None => return Err(ST_EBAL),
                }
                ci += 1;
            }
            i += 1;
        }
        if vi != 8 || ci != 6 {
            return Err(ST_EBAL);
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
            return Err(ST_EBAL);
        }
        self.root = self.seal();
        self.frozen = true;
        Ok(self.root)
    }

    /// 重放。
    pub fn replay(&self) -> Result<u64, u32> {
        let fresh = build_ledger();
        let r = fresh.seal();
        if r == self.seal() {
            Ok(r)
        } else {
            Err(ST_ESEAL)
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

/// 常规文件快照。
pub fn base_file() -> Meta {
    Meta {
        ino: 42,
        mode: IF_REG | 0o644,
        uid: 1000,
        gid: 1000,
        size: 4096,
        nlink: 1,
        atime: 500,
        mtime: 400,
        ctime: 300,
        blocks: 8,
    }
}

/// 目录快照。
pub fn base_dir() -> Meta {
    Meta {
        ino: 2,
        mode: IF_DIR | 0o755,
        uid: 0,
        gid: 0,
        size: 1024,
        nlink: 3,
        atime: 900,
        mtime: 800,
        ctime: 700,
        blocks: 2,
    }
}

/// 十四格账本：类型、权限、字段、差异与六类拒绝。
pub fn build_ledger() -> Ledger {
    let mut l = Ledger::new();
    let f = base_file();
    let d = base_dir();

    let _ = l.register(word_cell(0, OP_CHECK, f.type_char().map(|c| c as u32)));
    let _ = l.register(word_cell(1, OP_PERM, f.perm_string().map(|s| s.len() as u32)));
    let _ = l.register(word_cell(2, OP_FIELD, f.field(F_SIZE)));
    let _ = l.register(word_cell(3, OP_FIELD, f.field(F_UID)));

    let mut grown = f;
    grown.size = 8192;
    grown.blocks = 16;
    let _ = l.register(word_cell(4, OP_DIFF, f.diff(&grown)));
    let _ = l.register(word_cell(5, OP_FIELD, Ok(f.expect_blocks())));
    let _ = l.register(unit_cell(6, OP_READ, f.read_body().map(|_| ())));
    let _ = l.register(word_cell(7, OP_PERM, Ok((f.mode & 0o777) as u32)));

    let mut bad_mode = f;
    bad_mode.mode = 0o644;
    let _ = l.register(unit_cell(8, OP_CHECK, bad_mode.check()));

    let mut bad_time = f;
    bad_time.ctime = 401;
    let _ = l.register(unit_cell(9, OP_CHECK, bad_time.check()));

    let mut bad_blocks = f;
    bad_blocks.blocks = 9;
    let _ = l.register(unit_cell(10, OP_CHECK, bad_blocks.check()));

    let _ = l.register(word_cell(11, OP_DIFF, f.diff(&f)));
    let _ = l.register(unit_cell(12, OP_READ, d.read_body().map(|_| ())));
    let _ = l.register(word_cell(13, OP_FIELD, f.field(9)));

    l
}

/// 四路注入。
pub fn inject_four() -> [bool; 4] {
    let mut out = [false; 4];

    let l = build_ledger();
    let rep = l.replay();
    let root = l.seal();
    out[0] = l.len() == SAMPLES && l.audit().is_ok() && rep == Ok(root) && root != 0;

    // 一路：权限串与类型字符。
    let f = base_file();
    let d = base_dir();
    let mut lnk = f;
    lnk.mode = IF_LNK | 0o777;
    let mut chr = f;
    chr.mode = IF_CHR | 0o600;
    let mut fifo = f;
    fifo.mode = IF_FIFO | 0o000;
    let mut sock = f;
    sock.mode = IF_SOCK | 0o111;
    let mut blk = f;
    blk.mode = IF_BLK | 0o640;
    out[1] = f.perm_string() == Ok(String::from("rw-r--r--"))
        && d.perm_string() == Ok(String::from("rwxr-xr-x"))
        && lnk.perm_string() == Ok(String::from("rwxrwxrwx"))
        && chr.perm_string() == Ok(String::from("rw-------"))
        && fifo.perm_string() == Ok(String::from("---------"))
        && sock.perm_string() == Ok(String::from("--x--x--x"))
        && blk.perm_string() == Ok(String::from("rw-r-----"))
        && f.type_char() == Ok(b'-')
        && d.type_char() == Ok(b'd')
        && lnk.type_char() == Ok(b'l')
        && chr.type_char() == Ok(b'c')
        && blk.type_char() == Ok(b'b')
        && fifo.type_char() == Ok(b'p')
        && sock.type_char() == Ok(b's');

    // 二路：字段、块数与差异掩码。
    let mut own = f;
    own.uid = 1001;
    let mut time = f;
    time.mtime = 450;
    let mut ino = f;
    ino.ino = 43;
    let mut all = f;
    all.ino = 44;
    all.uid = 1002;
    all.size = 0;
    all.blocks = 0;
    all.atime = 501;
    all.nlink = 2;
    all.mode = IF_REG | 0o600;
    out[2] = f.field(F_INO) == Ok(42)
        && f.field(F_NLINK) == Ok(1)
        && f.field(F_GID) == Ok(1000)
        && f.field(F_BLOCKS) == Ok(8)
        && f.field(0) == Err(ST_EINVAL)
        && f.expect_blocks() == 8
        && f.diff(&own) == Ok(D_OWN)
        && f.diff(&time) == Ok(D_TIME)
        && f.diff(&ino) == Ok(D_INO)
        && f.diff(&all) == Ok(D_MODE | D_OWN | D_SIZE | D_TIME | D_LINK | D_INO)
        && f.diff(&f) == Err(ST_EDIFF);

    // 三路：自检闸门、读取闸门与冻结。
    let mut bad_mode = f;
    bad_mode.mode = 0o644;
    let mut bad_time = f;
    bad_time.ctime = 999;
    let mut bad_blocks = f;
    bad_blocks.blocks = 1;
    let mut zero_link = f;
    zero_link.nlink = 0;

    let mut led = build_ledger();
    let one = led.freeze();
    let rep = led.replay();
    let two = led.freeze();
    let reg = led.register(Sample::blank());
    out[3] = bad_mode.check() == Err(ST_EMODE)
        && bad_time.check() == Err(ST_ETIME)
        && bad_blocks.check() == Err(ST_EBLOCK)
        && zero_link.check() == Err(ST_ERANGE)
        && d.read_body() == Err(ST_ENOTP)
        && f.read_body() == Ok(4096)
        && one.is_ok()
        && rep == one
        && two.is_err()
        && reg == Err(ST_ESLOT)
        && led.is_frozen()
        && led.rejects() == 2;

    out
}

/// 十三项判据。
pub fn run_unx11526_checks() -> CheckSet {
    let mut s = CheckSet::new("unx11526.stat_snapshot");

    s.add(
        "ST01-权限串渲染",
        {
            let f = base_file();
            let d = base_dir();
            f.perm_string() == Ok(String::from("rw-r--r--"))
                && d.perm_string() == Ok(String::from("rwxr-xr-x"))
        },
        "static",
    );

    s.add(
        "ST02-类型字符七族",
        {
            let f = base_file();
            let mut acc = true;
            let chars = [b'-', b'd', b'l', b'c', b'b', b'p', b's'];
            let types = [IF_REG, IF_DIR, IF_LNK, IF_CHR, IF_BLK, IF_FIFO, IF_SOCK];
            let mut i = 0usize;
            while i < 7 {
                let mut m = f;
                m.mode = match types.get(i) {
                    Some(t) => *t | 0o644,
                    None => 0,
                };
                if m.type_char() != Ok(*match chars.get(i) {
                    Some(c) => c,
                    None => &b'-',
                }) {
                    acc = false;
                }
                i += 1;
            }
            acc
        },
        "static",
    );

    s.add(
        "ST03-整数字段取值",
        {
            let f = base_file();
            f.field(F_INO) == Ok(42)
                && f.field(F_NLINK) == Ok(1)
                && f.field(F_UID) == Ok(1000)
                && f.field(F_GID) == Ok(1000)
                && f.field(F_SIZE) == Ok(4096)
                && f.field(F_BLOCKS) == Ok(8)
                && f.field(9) == Err(ST_EINVAL)
        },
        "static",
    );

    s.add(
        "ST04-差异位掩码",
        {
            let f = base_file();
            let mut own = f;
            own.gid = 1001;
            let mut size = f;
            size.size = 8192;
            size.blocks = 16;
            let mut time = f;
            time.atime = 501;
            let mut link = f;
            link.nlink = 2;
            f.diff(&own) == Ok(D_OWN)
                && f.diff(&size) == Ok(D_SIZE)
                && f.diff(&time) == Ok(D_TIME)
                && f.diff(&link) == Ok(D_LINK)
                && f.diff(&f) == Err(ST_EDIFF)
        },
        "static",
    );

    s.add(
        "ST05-类型闸门",
        {
            let f = base_file();
            let mut bad = f;
            bad.mode = 0o644;
            let mut unsure = f;
            unsure.mode = 0o050000 | 0o644;
            bad.check() == Err(ST_EMODE)
                && bad.perm_string() == Err(ST_EMODE)
                && unsure.check() == Err(ST_EMODE)
        },
        "static",
    );

    s.add(
        "ST06-时间三账",
        {
            let f = base_file();
            let mut ctime = f;
            ctime.ctime = 401;
            let mut mtime = f;
            mtime.mtime = 501;
            f.check().is_ok()
                && ctime.check() == Err(ST_ETIME)
                && mtime.check() == Err(ST_ETIME)
                && f.diff(&ctime) == Err(ST_ETIME)
        },
        "static",
    );

    s.add(
        "ST07-块数口径",
        {
            let f = base_file();
            let mut zero = f;
            zero.size = 0;
            zero.blocks = 0;
            let mut one = f;
            one.size = 1;
            one.blocks = 1;
            let mut bad = f;
            bad.blocks = 9;
            f.expect_blocks() == 8
                && zero.check().is_ok()
                && one.check().is_ok()
                && bad.check() == Err(ST_EBLOCK)
        },
        "static",
    );

    s.add(
        "ST08-读取闸门",
        {
            let f = base_file();
            let d = base_dir();
            f.read_body() == Ok(4096)
                && d.read_body() == Err(ST_ENOTP)
                && f.is_regular()
                && !d.is_regular()
                && d.file_type() == IF_DIR
        },
        "static",
    );

    s.add(
        "ST09-账本审计",
        {
            let l = build_ledger();
            l.len() == SAMPLES
                && l.ok_count() == 8
                && l.err_count() == 6
                && l.audit().is_ok()
                && l.rejects() == 0
        },
        "static",
    );

    s.add(
        "ST10-账本错误行码序",
        {
            let l = build_ledger();
            let mut got = [0u32; 6];
            let mut ci = 0usize;
            let mut i = 0usize;
            let mut good = true;
            while i < l.len() {
                match l.sample(i) {
                    Some(smp) => {
                        if !smp.ok {
                            if let Some(dst) = got.get_mut(ci) {
                                *dst = smp.code;
                            }
                            ci += 1;
                        }
                    }
                    None => good = false,
                }
                i += 1;
            }
            good
                && ci == 6
                && got == [ST_EMODE, ST_ETIME, ST_EBLOCK, ST_EDIFF, ST_ENOTP, ST_EINVAL]
        },
        "static",
    );

    s.add(
        "ST11-冻结与拒绝计数",
        {
            let mut l = build_ledger();
            let a = l.freeze();
            let b = l.freeze();
            let c = l.register(Sample::blank());
            a.is_ok() && b == Err(ST_EBAL) && c == Err(ST_ESLOT) && l.rejects() == 2
        },
        "static",
    );

    s.add(
        "ST12-重放指纹一致",
        {
            let l = build_ledger();
            let a = l.seal();
            let mut f = build_ledger();
            let b = f.freeze();
            a != 0 && l.replay() == Ok(a) && b == Ok(a) && f.root() == a
        },
        "static",
    );

    s.add(
        "ST13-冻结重放四注入",
        {
            inject_four() == [true; 4] && build_ledger().audit().is_ok()
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
        assert_eq!(l.sample(SAMPLES), None);
    }

    #[test]
    fn perms_and_types() {
        let f = base_file();
        let d = base_dir();
        assert_eq!(f.perm_string(), Ok(String::from("rw-r--r--")));
        assert_eq!(d.perm_string(), Ok(String::from("rwxr-xr-x")));
        assert_eq!(f.type_char(), Ok(b'-'));
        assert_eq!(d.type_char(), Ok(b'd'));
        let mut bad = f;
        bad.mode = 0o644;
        assert_eq!(bad.type_char(), Err(ST_EMODE));
    }

    #[test]
    fn fields_and_blocks() {
        let f = base_file();
        assert_eq!(f.field(F_INO), Ok(42));
        assert_eq!(f.field(F_SIZE), Ok(4096));
        assert_eq!(f.field(F_BLOCKS), Ok(8));
        assert_eq!(f.field(0), Err(ST_EINVAL));
        assert_eq!(f.expect_blocks(), 8);
        let mut one = f;
        one.size = 1;
        one.blocks = 1;
        assert!(one.check().is_ok());
    }

    #[test]
    fn diff_masks() {
        let f = base_file();
        let mut own = f;
        own.uid = 1001;
        let mut size = f;
        size.size = 8192;
        size.blocks = 16;
        assert_eq!(f.diff(&own), Ok(D_OWN));
        assert_eq!(f.diff(&size), Ok(D_SIZE));
        assert_eq!(f.diff(&f), Err(ST_EDIFF));
    }

    #[test]
    fn time_and_block_gates() {
        let f = base_file();
        let mut ctime = f;
        ctime.ctime = 401;
        let mut blocks = f;
        blocks.blocks = 9;
        assert_eq!(ctime.check(), Err(ST_ETIME));
        assert_eq!(blocks.check(), Err(ST_EBLOCK));
    }

    #[test]
    fn read_gate() {
        let f = base_file();
        let d = base_dir();
        assert_eq!(f.read_body(), Ok(4096));
        assert_eq!(d.read_body(), Err(ST_ENOTP));
    }

    #[test]
    fn freeze_and_replay() {
        let mut l = build_ledger();
        let r = l.freeze().ok().unwrap();
        assert_eq!(l.replay(), Ok(r));
        assert_eq!(l.freeze(), Err(ST_EBAL));
        assert_eq!(l.register(Sample::blank()), Err(ST_ESLOT));
        assert_eq!(l.rejects(), 2);
        assert_eq!(l.replay(), Err(ST_ESEAL));
    }
}
