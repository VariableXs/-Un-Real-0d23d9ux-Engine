//! UNX-F11343 /proc/self/exe readlink 语义（0xeb70 段）
//!
//! `/proc/self/exe` 是一条**魔法符号链接**：它不在任何文件系统上，读取时由
//! 内核按当前进程 `mm->exe_file` 的路径现场生成。因此 `readlink(2)` 的三条
//! 语义必须逐条建模：
//!
//! 1. **不写 NUL**。`readlink` 返回的是「放进了多少字节」，缓冲里没有结尾
//!    零；调用方必须自己补。把头文件里「路径字符串」的直觉搬过来就会读出
//!    未初始化尾巴。
//! 2. **截断不报错**。缓冲小于目标时，内核装 `size` 字节并返回 `size`；
//!    返回值和「恰好填满」无法区分，调用方要按 `n == size` 再试探。缓冲区
//!    为 0 才是真正的错误（`EINVAL`）。
//! 3. **删除后缀**。可执行文件被 unlink 后，链接目标会追加 `" (deleted)"`
//!    （10 字节），路径变长；这个后缀是内核拼出来的，不是磁盘上的名字。
//!
//! 另外，链接目标恒为**绝对、规范**路径：内核解析出的路径不会含 `.`/`..`
//! 段、不会出现连续斜杠、不会以斜杠结尾。守护进程把自己的绑定路径交给
//! `/proc/self/exe` 时，这些规范性检查就是「我拿到的到底是不是内核视图」
//! 的第一道证据。
//!
//! 闸门次序（绑定）：空 → 绝对 → 长度 → 斜杠形态 → 点段。空路径与相对
//! 路径是调用方最明显的错误；长度超限先于逐段扫描，避免对超长输入做无谓
//! 遍历；斜杠形态先于点段，因为 `../` 只有在斜杠切分正确时才有意义。

use crate::alloc::format;
use crate::checks::CheckSet;

/// 段基址。
pub const SEG_BASE: u32 = 0xeb70;

/// 路径为空。
pub const EX_EEMPTY: u32 = SEG_BASE + 0x01;
/// 路径不是绝对路径。
pub const EX_ENOTABS: u32 = SEG_BASE + 0x02;
/// 路径超长。
pub const EX_ERANGE: u32 = SEG_BASE + 0x03;
/// 含 `.` 或 `..` 段。
pub const EX_EDOT: u32 = SEG_BASE + 0x04;
/// 含连续斜杠或以斜杠结尾。
pub const EX_ESLASH: u32 = SEG_BASE + 0x05;
/// 链接尚未绑定。
pub const EX_ENOENT: u32 = SEG_BASE + 0x06;
/// 删除后缀重复追加。
pub const EX_EDELETED: u32 = SEG_BASE + 0x07;
/// 读取长度为 0。
pub const EX_EINVAL: u32 = SEG_BASE + 0x08;
/// 下标越过已放置字节。
pub const EX_ENOBS: u32 = SEG_BASE + 0x09;
/// 账本失衡。
pub const EX_EBAL: u32 = SEG_BASE + 0x0a;
/// 账本槽位乱序。
pub const EX_EORDER: u32 = SEG_BASE + 0x0b;
/// 观测值与复算不符。
pub const EX_EMISMATCH: u32 = SEG_BASE + 0x0c;
/// 冻结后重放不一致。
pub const EX_ESEAL: u32 = SEG_BASE + 0x0d;

/// 目标路径容量。
pub const PATH_CAP: usize = 64;
/// 目标路径长度上限。
pub const PATH_MAX: usize = 64;
/// 删除后缀。
pub const DELETED_SUFFIX: &[u8] = b" (deleted)";
/// 删除后缀长度。
pub const SUFFIX_LEN: usize = 10;
/// 账本格数。
pub const SAMPLES: usize = 14;

/// 操作码：绑定目标。
pub const OP_BIND: u8 = 1;
/// 操作码：读取。
pub const OP_READ: u8 = 2;
/// 操作码：追加删除后缀。
pub const OP_DEL: u8 = 3;
/// 操作码：取字节。
pub const OP_BYTE: u8 = 4;

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

/// `/proc/self/exe` 的链接目标。
#[derive(Clone, Copy, Debug)]
pub struct ExeLink {
    target: [u8; PATH_CAP],
    len: usize,
    bound: bool,
    deleted: bool,
    placed: usize,
    truncated: bool,
}

impl ExeLink {
    /// 未绑定的链接。
    pub const fn new() -> ExeLink {
        ExeLink {
            target: [0u8; PATH_CAP],
            len: 0,
            bound: false,
            deleted: false,
            placed: 0,
            truncated: false,
        }
    }

    /// 绑定一个新目标。
    ///
    /// 成功返回目标字节长度。校验全部通过才落盘，失败不改变原目标。
    pub fn bind(&mut self, path: &[u8]) -> Result<u32, u32> {
        if path.is_empty() {
            return Err(EX_EEMPTY);
        }
        match path.first() {
            Some(b'/') => {}
            _ => return Err(EX_ENOTABS),
        }
        if path.len() > PATH_MAX {
            return Err(EX_ERANGE);
        }

        // 形态一：连续斜杠。
        let mut i = 1usize;
        while i < path.len() {
            match (path.get(i - 1), path.get(i)) {
                (Some(a), Some(b)) => {
                    if *a == b'/' && *b == b'/' {
                        return Err(EX_ESLASH);
                    }
                }
                _ => break,
            }
            i += 1;
        }
        // 形态二：以斜杠结尾（根除外）。
        if path.len() > 1 {
            match path.last() {
                Some(b'/') => return Err(EX_ESLASH),
                _ => {}
            }
        }

        // 点段扫描：按斜杠切分，逐段比较。
        let mut start = 0usize;
        let mut j = 0usize;
        while j <= path.len() {
            let at_end = j == path.len();
            let slash = match path.get(j) {
                Some(b) => *b == b'/',
                None => false,
            };
            if at_end || slash {
                let comp = match path.get(start..j) {
                    Some(c) => c,
                    None => &[],
                };
                let is_dot = comp.len() == 1 && comp.first() == Some(&b'.');
                let is_dotdot = comp.len() == 2
                    && comp.first() == Some(&b'.')
                    && comp.get(1) == Some(&b'.');
                if is_dot || is_dotdot {
                    return Err(EX_EDOT);
                }
                start = j + 1;
            }
            j += 1;
        }

        let mut k = 0usize;
        while k < path.len() {
            match (path.get(k), self.target.get_mut(k)) {
                (Some(src), Some(dst)) => *dst = *src,
                _ => return Err(EX_ERANGE),
            }
            k += 1;
        }
        self.len = path.len();
        self.bound = true;
        self.deleted = false;
        self.placed = 0;
        self.truncated = false;
        Ok(self.len as u32)
    }

    /// 是否已绑定。
    pub fn is_bound(&self) -> bool {
        self.bound
    }

    /// 目标长度（含删除后缀）。
    pub fn target_len(&self) -> usize {
        if self.bound {
            self.len
        } else {
            0
        }
    }

    /// 上次读取放置的字节数。
    pub fn placed_len(&self) -> usize {
        self.placed
    }

    /// 上次读取是否发生截断。
    pub fn is_truncated(&self) -> bool {
        self.truncated
    }

    /// 目标是否带删除后缀。
    pub fn is_deleted(&self) -> bool {
        self.deleted
    }

    /// 模拟 `readlink(2)`。
    ///
    /// 返回放置字节数；`size == 0` 报 `EINVAL`；未绑定报 `ENOENT`。
    /// 缓冲不足时截断且不报错，`is_truncated()` 只供观测，调用方不能依赖
    /// readlink 的返回值区分截断与恰好填满。
    pub fn readlink(&mut self, size: usize) -> Result<u32, u32> {
        if !self.bound {
            return Err(EX_ENOENT);
        }
        if size == 0 {
            return Err(EX_EINVAL);
        }
        let n = if self.len < size { self.len } else { size };
        self.placed = n;
        self.truncated = self.len > size;
        Ok(n as u32)
    }

    /// 在第 i 个已放置字节处取值。
    pub fn byte_at(&self, i: usize) -> Result<u32, u32> {
        if i >= self.placed {
            return Err(EX_ENOBS);
        }
        match self.target.get(i) {
            Some(b) => Ok(*b as u32),
            None => Err(EX_ENOBS),
        }
    }

    /// 追加 `" (deleted)"` 后缀。
    pub fn mark_deleted(&mut self) -> Result<u32, u32> {
        if !self.bound {
            return Err(EX_ENOENT);
        }
        if self.deleted {
            return Err(EX_EDELETED);
        }
        if self.len + SUFFIX_LEN > PATH_CAP {
            return Err(EX_ERANGE);
        }
        let mut i = 0usize;
        while i < SUFFIX_LEN {
            match (DELETED_SUFFIX.get(i), self.target.get_mut(self.len + i)) {
                (Some(src), Some(dst)) => *dst = *src,
                _ => return Err(EX_ERANGE),
            }
            i += 1;
        }
        self.len += SUFFIX_LEN;
        self.deleted = true;
        Ok(self.len as u32)
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

    /// 登记一格；槽位必须严格递增。
    pub fn register(&mut self, s: Sample) -> Result<(), u32> {
        if self.frozen || (s.slot as usize) != self.len || (s.slot as usize) >= SAMPLES {
            self.rejects += 1;
            return Err(EX_EORDER);
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
            return Err(EX_EBAL);
        }
        const VALS: [u32; 8] = [10, 20, 20, 8, 30, 30, 32, 1];
        const CODES: [u32; 6] = [
            EX_ENOTABS,
            EX_EDOT,
            EX_ESLASH,
            EX_ERANGE,
            EX_EEMPTY,
            EX_EINVAL,
        ];
        let mut vi = 0usize;
        let mut ci = 0usize;
        let mut i = 0usize;
        while i < self.len {
            let s = match self.rows.get(i) {
                Some(v) => *v,
                None => return Err(EX_EBAL),
            };
            if s.slot as usize != i {
                return Err(EX_EORDER);
            }
            if s.ok {
                if s.code != 0 {
                    return Err(EX_EMISMATCH);
                }
                match VALS.get(vi) {
                    Some(w) => {
                        if s.value != *w {
                            return Err(EX_EMISMATCH);
                        }
                    }
                    None => return Err(EX_EBAL),
                }
                vi += 1;
            } else {
                match CODES.get(ci) {
                    Some(w) => {
                        if s.code != *w {
                            return Err(EX_EMISMATCH);
                        }
                    }
                    None => return Err(EX_EBAL),
                }
                ci += 1;
            }
            i += 1;
        }
        if vi != 8 || ci != 6 {
            return Err(EX_EBAL);
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
            return Err(EX_EBAL);
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
            Err(EX_ESEAL)
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

/// 十四格账本：绑定 → 读取 → 后缀 → 字节，再六类拒绝。
pub fn build_ledger() -> Ledger {
    let mut l = Ledger::new();
    let mut e = ExeLink::new();

    let _ = l.register(word_cell(0, OP_BIND, e.bind(b"/sbin/init")));
    let _ = l.register(word_cell(1, OP_BIND, e.bind(b"/opt/varix/bin/vxbox")));
    let _ = l.register(word_cell(2, OP_READ, e.readlink(20)));
    let _ = l.register(word_cell(3, OP_READ, e.readlink(8)));
    let _ = l.register(word_cell(4, OP_DEL, e.mark_deleted()));
    let _ = l.register(word_cell(5, OP_READ, e.readlink(30)));
    let _ = l.register(word_cell(6, OP_BYTE, e.byte_at(20)));
    let _ = l.register(word_cell(7, OP_READ, e.readlink(1)));

    let _ = l.register(word_cell(8, OP_BIND, e.bind(b"bin/init")));
    let _ = l.register(word_cell(9, OP_BIND, e.bind(b"/sbin/./init")));
    let _ = l.register(word_cell(10, OP_BIND, e.bind(b"/sbin//init")));

    // 超长路径：首字节为斜杠，其余填充；长度超过 PATH_MAX。
    let mut long = [b'a'; 80];
    if let Some(b0) = long.get_mut(0) {
        *b0 = b'/';
    }
    let long_path: &[u8] = match long.get(..80) {
        Some(s) => s,
        None => b"/",
    };
    let _ = l.register(word_cell(11, OP_BIND, e.bind(long_path)));
    let _ = l.register(word_cell(12, OP_BIND, e.bind(b"")));
    let _ = l.register(word_cell(13, OP_READ, e.readlink(0)));

    l
}

/// 四路注入。
pub fn inject_four() -> [bool; 4] {
    let mut out = [false; 4];

    let l = build_ledger();
    let rep = l.replay();
    let root = l.seal();
    out[0] = l.len() == SAMPLES && l.audit().is_ok() && rep == Ok(root) && root != 0;

    // 一路：不写 NUL、截断不报错、零长为错。
    let mut a = ExeLink::new();
    let before = a.readlink(8) == Err(EX_ENOENT);
    let bound = a.bind(b"/sbin/init") == Ok(10);
    let exact = a.readlink(10) == Ok(10) && !a.is_truncated();
    let tail = a.byte_at(9) == Ok(b't' as u32) && a.byte_at(10) == Err(EX_ENOBS);
    let short = a.readlink(4) == Ok(4) && a.is_truncated();
    let head = a.byte_at(0) == Ok(b'/' as u32)
        && a.byte_at(1) == Ok(b's' as u32)
        && a.byte_at(3) == Ok(b'i' as u32)
        && a.byte_at(4) == Err(EX_ENOBS);
    let zero = a.readlink(0) == Err(EX_EINVAL);
    let large = a.readlink(100) == Ok(10) && !a.is_truncated();
    out[1] = before && bound && exact && tail && short && head && zero && large;

    // 二路：删除后缀追加、幂等拒绝、容量溢出。
    let mut b = ExeLink::new();
    let unbound_del = b.mark_deleted() == Err(EX_ENOENT);
    let base = b.bind(b"/sbin/init") == Ok(10);
    let del = b.mark_deleted() == Ok(20);
    let full = b.readlink(20) == Ok(20)
        && !b.is_truncated()
        && b.byte_at(10) == Ok(b' ' as u32)
        && b.byte_at(19) == Ok(b')' as u32)
        && b.byte_at(11) == Ok(b'(' as u32);
    let twice = b.mark_deleted() == Err(EX_EDELETED);
    let again = b.readlink(10) == Ok(10) && b.is_truncated();

    let mut big = [b'a'; 60];
    if let Some(b0) = big.get_mut(0) {
        *b0 = b'/';
    }
    let big_path: &[u8] = match big.get(..60) {
        Some(s) => s,
        None => b"/",
    };
    let mut c = ExeLink::new();
    let fits = c.bind(big_path) == Ok(60);
    let overflow = c.mark_deleted() == Err(EX_ERANGE);
    out[2] = unbound_del && base && del && full && twice && again && fits && overflow;

    // 三路：规范性闸门与失败不改状态。
    let mut d = ExeLink::new();
    let empty_first = d.bind(b"") == Err(EX_EEMPTY);
    let rel = d.bind(b"short") == Err(EX_ENOTABS);
    let slash_before_dot = d.bind(b"/a//../b") == Err(EX_ESLASH);
    let dot = d.bind(b"/a/../b") == Err(EX_EDOT);
    let dot2 = d.bind(b"/a/./b") == Err(EX_EDOT);
    let trail = d.bind(b"/a/") == Err(EX_ESLASH);
    let root = d.bind(b"/") == Ok(1);
    let zero_after = d.readlink(0) == Err(EX_EINVAL);

    let mut p = ExeLink::new();
    let _ = p.bind(b"/sbin/init");
    let rejected = p.bind(b"/x//y") == Err(EX_ESLASH);
    let preserved = p.readlink(10) == Ok(10)
        && p.byte_at(0) == Ok(b'/' as u32)
        && p.byte_at(9) == Ok(b't' as u32);
    out[3] = empty_first
        && rel
        && slash_before_dot
        && dot
        && dot2
        && trail
        && root
        && zero_after
        && rejected
        && preserved;

    out
}

/// 十三项判据。
pub fn run_unx11343_checks() -> CheckSet {
    let mut s = CheckSet::new("unx11343.exe_readlink");
    let l = build_ledger();

    s.add_dyn(
        "RL1-十四格八允六拒",
        l.len() == SAMPLES && l.ok_count() == 8 && l.err_count() == 6 && l.audit().is_ok(),
        &format!("n={} ok={} err={}", l.len(), l.ok_count(), l.err_count()),
    );

    s.add(
        "RL2-绑定绝对路径",
        l.sample(0).map(|x| x.value) == Some(10),
        "static",
    );

    s.add(
        "RL3-重绑定新目标",
        l.sample(1).map(|x| x.value) == Some(20),
        "static",
    );

    s.add(
        "RL4-恰好填满不截断",
        {
            let mut e = ExeLink::new();
            let _ = e.bind(b"/opt/varix/bin/vxbox");
            l.sample(2).map(|x| x.value) == Some(20)
                && e.readlink(20) == Ok(20)
                && !e.is_truncated()
        },
        "static",
    );

    s.add(
        "RL5-短缓冲截断",
        {
            let mut e = ExeLink::new();
            let _ = e.bind(b"/opt/varix/bin/vxbox");
            l.sample(3).map(|x| x.value) == Some(8)
                && e.readlink(8) == Ok(8)
                && e.is_truncated()
                && e.byte_at(7) == Ok(b'r' as u32)
                && e.byte_at(8) == Err(EX_ENOBS)
        },
        "static",
    );

    s.add(
        "RL6-删除后缀追加",
        l.sample(4).map(|x| x.value) == Some(30),
        "static",
    );

    s.add(
        "RL7-删除后完整读取",
        l.sample(5).map(|x| x.value) == Some(30),
        "static",
    );

    s.add(
        "RL8-后缀分隔空格",
        l.sample(6).map(|x| x.value) == Some(b' ' as u32),
        "static",
    );

    s.add(
        "RL9-单字节与零长读取",
        {
            let mut e = ExeLink::new();
            let _ = e.bind(b"/opt/varix/bin/vxbox");
            let _ = e.mark_deleted();
            l.sample(7).map(|x| x.value) == Some(1)
                && e.readlink(1) == Ok(1)
                && e.byte_at(0) == Ok(b'/' as u32)
                && l.sample(13).map(|x| x.code) == Some(EX_EINVAL)
        },
        "static",
    );

    s.add(
        "RL10-相对路径拒绝",
        l.sample(8).map(|x| x.code) == Some(EX_ENOTABS),
        "static",
    );

    s.add(
        "RL11-点段与双斜杠",
        l.sample(9).map(|x| x.code) == Some(EX_EDOT)
            && l.sample(10).map(|x| x.code) == Some(EX_ESLASH),
        "static",
    );

    s.add(
        "RL12-超长与空路径",
        l.sample(11).map(|x| x.code) == Some(EX_ERANGE)
            && l.sample(12).map(|x| x.code) == Some(EX_EEMPTY),
        "static",
    );

    s.add(
        "RL13-冻结重放四注入",
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
        let s = run_unx11343_checks();
        assert_eq!(s.len(), 13);
        assert!(s.all_passed());
    }

    #[test]
    fn readlink_exact_and_short() {
        let mut e = ExeLink::new();
        assert_eq!(e.bind(b"/sbin/init"), Ok(10));
        assert_eq!(e.readlink(10), Ok(10));
        assert!(!e.is_truncated());
        assert_eq!(e.byte_at(9), Ok(b't' as u32));
        assert_eq!(e.byte_at(10), Err(EX_ENOBS));
        assert_eq!(e.readlink(3), Ok(3));
        assert!(e.is_truncated());
        assert_eq!(e.readlink(0), Err(EX_EINVAL));
    }

    #[test]
    fn readlink_before_bind() {
        let mut e = ExeLink::new();
        assert_eq!(e.readlink(8), Err(EX_ENOENT));
        assert_eq!(e.mark_deleted(), Err(EX_ENOENT));
        assert_eq!(e.byte_at(0), Err(EX_ENOBS));
    }

    #[test]
    fn deleted_suffix() {
        let mut e = ExeLink::new();
        assert_eq!(e.bind(b"/sbin/init"), Ok(10));
        assert_eq!(e.mark_deleted(), Ok(20));
        assert!(e.is_deleted());
        assert_eq!(e.readlink(20), Ok(20));
        assert_eq!(e.byte_at(10), Ok(b' ' as u32));
        assert_eq!(e.byte_at(19), Ok(b')' as u32));
        assert_eq!(e.byte_at(11), Ok(b'(' as u32));
        assert_eq!(e.mark_deleted(), Err(EX_EDELETED));
    }

    #[test]
    fn bind_rejects_noncanonical() {
        let mut e = ExeLink::new();
        assert_eq!(e.bind(b""), Err(EX_EEMPTY));
        assert_eq!(e.bind(b"rel/path"), Err(EX_ENOTABS));
        assert_eq!(e.bind(b"/a//b"), Err(EX_ESLASH));
        assert_eq!(e.bind(b"/a/"), Err(EX_ESLASH));
        assert_eq!(e.bind(b"/a/./b"), Err(EX_EDOT));
        assert_eq!(e.bind(b"/a/../b"), Err(EX_EDOT));
        assert_eq!(e.bind(b"/"), Ok(1));
    }

    #[test]
    fn failed_bind_keeps_target() {
        let mut e = ExeLink::new();
        assert_eq!(e.bind(b"/sbin/init"), Ok(10));
        assert_eq!(e.bind(b"/x//y"), Err(EX_ESLASH));
        assert_eq!(e.readlink(10), Ok(10));
        assert_eq!(e.target_len(), 10);
    }

    #[test]
    fn long_path_and_suffix_overflow() {
        let mut long = [b'a'; 80];
        if let Some(b0) = long.get_mut(0) {
            *b0 = b'/';
        }
        let path: &[u8] = match long.get(..80) {
            Some(s) => s,
            None => b"/",
        };
        let mut e = ExeLink::new();
        assert_eq!(e.bind(path), Err(EX_ERANGE));

        let mut big = [b'a'; 60];
        if let Some(b0) = big.get_mut(0) {
            *b0 = b'/';
        }
        let path2: &[u8] = match big.get(..60) {
            Some(s) => s,
            None => b"/",
        };
        assert_eq!(e.bind(path2), Ok(60));
        assert_eq!(e.mark_deleted(), Err(EX_ERANGE));
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
