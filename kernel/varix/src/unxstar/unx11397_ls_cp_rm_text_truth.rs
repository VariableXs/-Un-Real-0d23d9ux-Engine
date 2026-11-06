//! UNX-F11397 ls/cp/rm 错误文案真值（0xeed0 段）
//!
//! 命令行工具的报错文本不是装饰，而是**接口**。大量脚本、expect 用例与日志
//! 告警规则都按字节匹配这些句子，任何人「顺手美化」一下措辞就会让下游静默
//! 失效。GNU coreutils 的文案由三段拼成：
//!
//! ```text
//! <程序名>: <动词> '<操作数>'[: <strerror 文本>]
//! ```
//!
//! 例：
//!
//! ```text
//! ls: cannot access 'a.txt': No such file or directory
//! cp: cannot stat 'b.txt': No such file or directory
//! cp: -r not specified; omitting directory 'd1'
//! rm: cannot remove 'd2': Is a directory
//! ```
//!
//! 注意第三条：`omitting directory` **没有** errno 尾巴。它不是在报告某个
//! 系统调用失败，而是在解释「我按规则跳过了这个对象」，所以后缀必须为空。
//! 本模块把这条差异建模为模板的 `errno` 标志位——若给它补上后缀，逐字判据
//! 立刻转红。
//!
//! 错误类为何必须分开：
//!
//! - `EPROG`：程序名不在 ls/cp/rm 三件之内。没有程序名就没有前缀，文案无从
//!   拼起，必须在取模板之前拒绝；
//! - `EINVAL`：操作数为空。空操作数在日志里与「漏传参数」无法区分，本引擎
//!   在真值档里直接拒绝，而不是伪造一行指向空串的文案；
//! - `ELONG`：操作数超过 24 字节的展示上限，拼接会撑破定长缓冲；
//! - `EQUOTE`：操作数内含单引号。GNU 用单引号包裹操作数且不做转义，`o'b'
//!   ` 这类路径回读时会分不清边界，故严格档拒绝；
//! - `EUNK`：该 (程序, errno) 组合没有模板。这是**表缺口**，与调用方传错
//!   程序名的 `EPROG` 完全不同：一个要补表，一个要改调用方；
//! - `EFULL`：会话已收满 8 条，再报错也无处安放；
//! - `EBUF`：拼接结果超过 96 字节，属于渲染期溢出，和容量满不是一回事；
//! - `ETRUTH`：渲染结果与真值字面量不相等。这是判据失败，不是运行失败。
//!
//! 闸门次序（`run`）：程序身份 → 操作数形状（空/长/引号）→ `rm -f` 吞
//! `ENOENT` → 模板查找 → 会话容量 → 渲染 → 抬退出码并落账。次序是判据的
//! 一部分：空路径配未知 errno 必须报 `EINVAL`（形状先于模板），非法程序名
//! 配空路径必须报 `EPROG`（身份先于形状），`rm -f` 遇到 `EISDIR` 绝不能被
//! 吞掉（吞并只认 `ENOENT`）。
//!
//! 退出码按程序语义分档：`ls` 遇到「打不开某个操作数」记 2，仅有个别条目
//! 读不了记 1；`cp`/`rm` 任何未吞并的失败记 1；被 `-f` 吞掉的缺失文件不
//! 抬码。分档取最大值，绝不回退。

use crate::alloc::format;
use crate::checks::CheckSet;

/// 段基址。
pub const SEG_BASE: u32 = 0xeed0;

/// 程序名不在 ls/cp/rm 之内。
pub const MS_EPROG: u32 = SEG_BASE + 0x01;
/// 操作数为空。
pub const MS_EINVAL: u32 = SEG_BASE + 0x02;
/// 操作数超过展示上限。
pub const MS_ELONG: u32 = SEG_BASE + 0x03;
/// 操作数含单引号，引号边界有歧义。
pub const MS_EQUOTE: u32 = SEG_BASE + 0x04;
/// 该 (程序, errno) 没有文案模板。
pub const MS_EUNK: u32 = SEG_BASE + 0x05;
/// 会话消息表已满。
pub const MS_EFULL: u32 = SEG_BASE + 0x06;
/// 渲染结果超出定长缓冲。
pub const MS_EBUF: u32 = SEG_BASE + 0x07;
/// 渲染结果与真值字面量不符。
pub const MS_ETRUTH: u32 = SEG_BASE + 0x08;
/// 账本形状不符。
pub const MS_EBAL: u32 = SEG_BASE + 0x09;
/// 槽位乱序。
pub const MS_ESLOT: u32 = SEG_BASE + 0x0a;
/// 观测值与复算不符。
pub const MS_EVALUE: u32 = SEG_BASE + 0x0b;
/// 冻结后重放不一致。
pub const MS_ESEAL: u32 = SEG_BASE + 0x0c;
/// 观察下标越界。
pub const MS_ENOBS: u32 = SEG_BASE + 0x0d;

/// 会话消息容量。
pub const CAP: usize = 8;
/// 消息文本容量（字节）。
pub const TXT_CAP: usize = 96;
/// 操作数展示上限。
pub const PLEN: usize = 24;
/// 账本格数。
pub const SAMPLES: usize = 14;

/// 操作码：跑一条文案。
pub const OP_RUN: u8 = 1;
/// 操作码：逐字比对。
pub const OP_VERIFY: u8 = 2;
/// 操作码：读退出码档位。
pub const OP_STATUS: u8 = 3;
/// 操作码：`rm -f` 吞并路径。
pub const OP_FORCE: u8 = 4;
/// 操作码：填充到容量。
pub const OP_FILL: u8 = 5;

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

/// 程序名：ls。
pub const LS: u8 = 0;
/// 程序名：cp。
pub const CP: u8 = 1;
/// 程序名：rm。
pub const RM: u8 = 2;

/// errno：不存在。
pub const ENOENT: u32 = 2;
/// errno：权限不足。
pub const EACCES: u32 = 13;
/// errno：操作不允许。
pub const EPERM: u32 = 1;
/// errno：是目录。
pub const EISDIR: u32 = 21;
/// errno：不是目录。
pub const ENOTDIR: u32 = 20;
/// errno：空间不足。
pub const ENOSPC: u32 = 28;
/// errno：只读文件系统。
pub const EROFS: u32 = 30;

/// 一条文案模板。
#[derive(Clone, Copy, Debug)]
pub struct Tpl {
    /// 程序名。
    pub prog: u8,
    /// errno。
    pub code: u32,
    /// 动词段。
    pub verb: &'static str,
    /// 是否追加 `: <strerror>` 尾巴。
    pub errno: bool,
}

/// 模板表。`omitting directory` 一行的 `errno` 为假，这是逐字判据的关键。
pub const TPLS: [Tpl; 12] = [
    Tpl { prog: LS, code: ENOENT, verb: "cannot access", errno: true },
    Tpl { prog: LS, code: ENOTDIR, verb: "cannot access", errno: true },
    Tpl { prog: LS, code: EACCES, verb: "cannot access", errno: true },
    Tpl { prog: CP, code: ENOENT, verb: "cannot stat", errno: true },
    Tpl { prog: CP, code: EISDIR, verb: "-r not specified; omitting directory", errno: false },
    Tpl { prog: CP, code: EACCES, verb: "cannot open", errno: true },
    Tpl { prog: CP, code: ENOSPC, verb: "error writing", errno: true },
    Tpl { prog: RM, code: ENOENT, verb: "cannot remove", errno: true },
    Tpl { prog: RM, code: EISDIR, verb: "cannot remove", errno: true },
    Tpl { prog: RM, code: EPERM, verb: "cannot remove", errno: true },
    Tpl { prog: RM, code: EROFS, verb: "cannot remove", errno: true },
    Tpl { prog: RM, code: EACCES, verb: "cannot remove", errno: true },
];

/// 程序名文本。非法程序返回空串，调用方在 `run` 里已先拒绝。
pub fn prog_name(prog: u8) -> &'static str {
    if prog == LS {
        "ls"
    } else if prog == CP {
        "cp"
    } else if prog == RM {
        "rm"
    } else {
        ""
    }
}

/// strerror 文本表。
pub fn strerr(code: u32) -> &'static str {
    if code == ENOENT {
        "No such file or directory"
    } else if code == EACCES {
        "Permission denied"
    } else if code == EPERM {
        "Operation not permitted"
    } else if code == EISDIR {
        "Is a directory"
    } else if code == ENOTDIR {
        "Not a directory"
    } else if code == ENOSPC {
        "No space left on device"
    } else if code == EROFS {
        "Read-only file system"
    } else {
        "Unknown error"
    }
}

/// 取模板。表里没有就是 `EUNK`——这是表缺口，不是调用方错误。
pub fn lookup(prog: u8, code: u32) -> Result<Tpl, u32> {
    let mut i = 0usize;
    while i < TPLS.len() {
        if let Some(t) = TPLS.get(i) {
            if t.prog == prog && t.code == code {
                return Ok(*t);
            }
        }
        i += 1;
    }
    Err(MS_EUNK)
}

/// 定长文本。所有写入都走 `get_mut`，越界即 `EBUF`。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Txt {
    b: [u8; TXT_CAP],
    n: usize,
}

impl Txt {
    /// 空文本。
    pub const fn blank() -> Txt {
        Txt { b: [0u8; TXT_CAP], n: 0 }
    }

    /// 字节数。
    pub fn len(&self) -> usize {
        self.n
    }

    /// 是否为空。
    pub fn is_empty(&self) -> bool {
        self.n == 0
    }

    /// 追加一个字节。
    pub fn push_byte(&mut self, c: u8) -> Result<(), u32> {
        if self.n >= TXT_CAP {
            return Err(MS_EBUF);
        }
        if let Some(d) = self.b.get_mut(self.n) {
            *d = c;
        }
        self.n += 1;
        Ok(())
    }

    /// 追加一段字节。
    pub fn push_str(&mut self, s: &str) -> Result<(), u32> {
        let src = s.as_bytes();
        let mut i = 0usize;
        while i < src.len() {
            match src.get(i) {
                Some(c) => self.push_byte(*c)?,
                None => break,
            }
            i += 1;
        }
        Ok(())
    }

    /// 追加十进制无符号数。
    pub fn push_u32(&mut self, v: u32) -> Result<(), u32> {
        let mut buf = [0u8; 10];
        let mut at = 10usize;
        let mut x = v;
        loop {
            at -= 1;
            if let Some(p) = buf.get_mut(at) {
                *p = b'0' + (x % 10) as u8;
            }
            x /= 10;
            if x == 0 {
                break;
            }
        }
        while at < 10 {
            match buf.get(at) {
                Some(c) => self.push_byte(*c)?,
                None => break,
            }
            at += 1;
        }
        Ok(())
    }

    /// 与字面量逐字节相等。
    pub fn eq_str(&self, s: &str) -> bool {
        let rhs = s.as_bytes();
        if self.n != rhs.len() {
            return false;
        }
        let mut i = 0usize;
        while i < self.n {
            let a = match self.b.get(i) {
                Some(v) => *v,
                None => return false,
            };
            let c = match rhs.get(i) {
                Some(v) => *v,
                None => return false,
            };
            if a != c {
                return false;
            }
            i += 1;
        }
        true
    }

    /// 与字面量首个不同字节的下标；完全相等返回 `None`。
    pub fn first_diff(&self, s: &str) -> Option<usize> {
        let rhs = s.as_bytes();
        let mut i = 0usize;
        while i < self.n && i < rhs.len() {
            let a = match self.b.get(i) {
                Some(v) => *v,
                None => return None,
            };
            let c = match rhs.get(i) {
                Some(v) => *v,
                None => return None,
            };
            if a != c {
                return Some(i);
            }
            i += 1;
        }
        if self.n == rhs.len() {
            None
        } else {
            Some(i)
        }
    }
}

/// 操作数里是否有单引号。
pub fn has_quote(s: &str) -> bool {
    let b = s.as_bytes();
    let mut i = 0usize;
    while i < b.len() {
        if let Some(c) = b.get(i) {
            if *c == b'\'' {
                return true;
            }
        }
        i += 1;
    }
    false
}

/// 一次会话：定长消息表 + 退出码 + 吞并计数。
#[derive(Clone, Copy, Debug)]
pub struct Tr {
    msgs: [Txt; CAP],
    n: usize,
    status: u32,
    suppressed: u32,
    emitted: u32,
}

impl Tr {
    /// 空会话。
    pub const fn new() -> Tr {
        Tr { msgs: [Txt::blank(); CAP], n: 0, status: 0, suppressed: 0, emitted: 0 }
    }

    /// 消息条数。
    pub fn len(&self) -> usize {
        self.n
    }

    /// 是否一条都没发。
    pub fn is_empty(&self) -> bool {
        self.n == 0
    }

    /// 当前退出码。
    pub fn status(&self) -> u32 {
        self.status
    }

    /// 被 `-f` 吞掉的条数。
    pub fn suppressed(&self) -> u32 {
        self.suppressed
    }

    /// 累计落账条数。
    pub fn emitted(&self) -> u32 {
        self.emitted
    }

    /// 第 i 条消息。
    pub fn msg(&self, i: usize) -> Option<Txt> {
        if i >= self.n {
            return None;
        }
        self.msgs.get(i).copied()
    }

    /// 不变式：落账数等于表内条数，条数不超容量，退出码只在 0..=2。
    pub fn consistent(&self) -> bool {
        self.n <= CAP && self.emitted as usize == self.n && self.n == self.count_live() && self.status <= 2
    }

    fn count_live(&self) -> usize {
        let mut c = 0usize;
        let mut i = 0usize;
        while i < self.n {
            if let Some(m) = self.msgs.get(i) {
                if !m.is_empty() {
                    c += 1;
                }
            }
            i += 1;
        }
        c
    }

    /// 按程序语义抬退出码，只升不降。
    fn raise(&mut self, prog: u8, code: u32) {
        let v = if prog == LS && code == EACCES {
            1
        } else if prog == LS {
            2
        } else {
            1
        };
        if v > self.status {
            self.status = v;
        }
    }

    /// 渲染一条文案。
    pub fn render(t: Tpl, path: &str) -> Result<Txt, u32> {
        let mut x = Txt::blank();
        x.push_str(prog_name(t.prog))?;
        x.push_str(": ")?;
        x.push_str(t.verb)?;
        x.push_str(" '")?;
        x.push_str(path)?;
        x.push_str("'")?;
        if t.errno {
            x.push_str(": ")?;
            x.push_str(strerr(t.code))?;
        }
        Ok(x)
    }

    /// 跑一条文案。
    ///
    /// 闸门：程序身份 → 操作数形状（空/长/引号）→ `rm -f` 吞 `ENOENT` →
    /// 模板查找 → 会话容量 → 渲染 → 抬码落账。返回本次落账条数（吞并为 0）。
    pub fn run(&mut self, prog: u8, code: u32, path: &str, force: bool) -> Result<usize, u32> {
        if prog != LS && prog != CP && prog != RM {
            return Err(MS_EPROG);
        }
        if path.is_empty() {
            return Err(MS_EINVAL);
        }
        if path.len() > PLEN {
            return Err(MS_ELONG);
        }
        if has_quote(path) {
            return Err(MS_EQUOTE);
        }
        if force && prog == RM && code == ENOENT {
            self.suppressed = self.suppressed.saturating_add(1);
            return Ok(0);
        }
        let t = lookup(prog, code)?;
        if self.n >= CAP {
            return Err(MS_EFULL);
        }
        let text = Tr::render(t, path)?;
        if let Some(dst) = self.msgs.get_mut(self.n) {
            *dst = text;
        }
        self.n += 1;
        self.emitted = self.emitted.saturating_add(1);
        self.raise(prog, code);
        Ok(1)
    }

    /// 逐字比对第 i 条。越界是观测范围错（`ENOBS`），不等是判据错（`ETRUTH`）。
    pub fn verify(&self, i: usize, truth: &str) -> Result<usize, u32> {
        if i >= self.n {
            return Err(MS_ENOBS);
        }
        let m = match self.msgs.get(i) {
            Some(v) => *v,
            None => return Err(MS_ENOBS),
        };
        if m.eq_str(truth) {
            Ok(1)
        } else {
            Err(MS_ETRUTH)
        }
    }

    /// 退出码档位：0/1/2 之外的取值说明状态机坏了。
    pub fn status_class(&self) -> Result<u32, u32> {
        if self.status <= 2 {
            Ok(self.status)
        } else {
            Err(MS_EBAL)
        }
    }

    /// 填满消息表，供容量边界使用。
    pub fn saturate(&mut self) -> Result<usize, u32> {
        let mut k = 0usize;
        while self.n < CAP {
            let r = self.run(LS, ENOENT, "f", false)?;
            k = k.saturating_add(r);
        }
        Ok(k)
    }
}

/// 一格观测。
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

    /// 登记一格。冻结后或槽位乱序一律拒绝并计数。
    pub fn register(&mut self, s: Sample) -> Result<(), u32> {
        if self.frozen || (s.slot as usize) != self.len || (s.slot as usize) >= SAMPLES {
            self.rejects = self.rejects.saturating_add(1);
            return Err(MS_ESLOT);
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

    /// 对账：14 格、8 允 6 拒、值/码/槽序全钉死。
    pub fn audit(&self) -> Result<(), u32> {
        if self.len != SAMPLES || self.ok_count() != 8 || self.err_count() != 6 {
            return Err(MS_EBAL);
        }
        const VALS: [u32; 8] = [1, 1, 0, 1, 1, 1, 1, 2];
        const CODES: [u32; 6] = [MS_EPROG, MS_EINVAL, MS_ELONG, MS_EQUOTE, MS_EUNK, MS_EFULL];
        let mut vi = 0usize;
        let mut ci = 0usize;
        let mut i = 0usize;
        while i < self.len {
            let s = match self.rows.get(i) {
                Some(v) => *v,
                None => return Err(MS_EBAL),
            };
            if s.slot as usize != i {
                return Err(MS_ESLOT);
            }
            if s.ok {
                if s.code != 0 {
                    return Err(MS_EVALUE);
                }
                match VALS.get(vi) {
                    Some(w) => {
                        if s.value != *w {
                            return Err(MS_EVALUE);
                        }
                    }
                    None => return Err(MS_EBAL),
                }
                vi += 1;
            } else {
                match CODES.get(ci) {
                    Some(w) => {
                        if s.code != *w {
                            return Err(MS_EVALUE);
                        }
                    }
                    None => return Err(MS_EBAL),
                }
                ci += 1;
            }
            i += 1;
        }
        if vi != 8 || ci != 6 {
            return Err(MS_EBAL);
        }
        Ok(())
    }

    /// 指纹。只折内容行，不折拒绝计数——否则一次被拒的重复登记会让同一份
    /// 内容算出两个不同的根，冻结与重放就永远对不上。
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
        h
    }

    /// 冻结并记根。
    pub fn freeze(&mut self) -> Result<u64, u32> {
        if self.frozen {
            self.rejects = self.rejects.saturating_add(1);
            return Err(MS_EBAL);
        }
        self.root = self.seal();
        self.frozen = true;
        Ok(self.root)
    }

    /// 重放：重建一份账本并比对指纹。
    pub fn replay(&self) -> Result<u64, u32> {
        let fresh = build_ledger();
        if fresh.seal() == self.seal() {
            Ok(fresh.seal())
        } else {
            Err(MS_ESEAL)
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

/// 逐字真值：ls 打不开操作数。
pub const LIT_LS_ACCESS: &str = "ls: cannot access 'a.txt': No such file or directory";
/// 逐字真值：cp 的源不存在。
pub const LIT_CP_STAT: &str = "cp: cannot stat 'b.txt': No such file or directory";
/// 逐字真值：cp 跳过目录（无 errno 尾巴）。
pub const LIT_CP_OMIT: &str = "cp: -r not specified; omitting directory 'd1'";
/// 逐字真值：rm 遇到目录。
pub const LIT_RM_DIR: &str = "rm: cannot remove 'd2': Is a directory";
/// 逐字真值：rm 被拒绝。
pub const LIT_RM_PERM: &str = "rm: cannot remove 'p': Operation not permitted";
/// 逐字真值：rm 权限不足。
pub const LIT_RM_ACC: &str = "rm: cannot remove 'q': Permission denied";
/// 逐字真值：cp 写满。
pub const LIT_CP_FULL: &str = "cp: error writing 'out': No space left on device";
/// 25 字节操作数，越过 24 字节展示上限。
pub const LONG_PATH: &str = "abcdefghijklmnopqrstuvwxy";

/// 十四格账本：七条真值文案 + 退出码档位 + 六类拒绝。
pub fn build_ledger() -> Ledger {
    let mut l = Ledger::new();
    let mut t = Tr::new();

    let _ = l.register(cell(0, OP_RUN, t.run(LS, ENOENT, "a.txt", false)));
    let _ = l.register(cell(1, OP_VERIFY, t.verify(0, LIT_LS_ACCESS)));
    let _ = l.register(cell(2, OP_FORCE, t.run(RM, ENOENT, "gone", true)));
    let _ = l.register(cell(3, OP_RUN, t.run(CP, ENOENT, "b.txt", false)));
    let _ = l.register(cell(4, OP_RUN, t.run(CP, EISDIR, "d1", false)));
    let _ = l.register(cell(5, OP_RUN, t.run(RM, EISDIR, "d2", false)));
    let _ = l.register(cell(6, OP_RUN, t.run(RM, EPERM, "p", false)));
    let _ = l.register(word_cell(7, OP_STATUS, t.status_class()));

    let mut a = Tr::new();
    let _ = l.register(cell(8, OP_RUN, a.run(7, ENOENT, "x", false)));

    let mut b = Tr::new();
    let _ = l.register(cell(9, OP_RUN, b.run(LS, ENOENT, "", false)));

    let mut c = Tr::new();
    let _ = l.register(cell(10, OP_RUN, c.run(LS, ENOENT, LONG_PATH, false)));

    let mut d = Tr::new();
    let _ = l.register(cell(11, OP_RUN, d.run(LS, ENOENT, "o'brien", false)));

    let mut e = Tr::new();
    let _ = l.register(cell(12, OP_RUN, e.run(LS, 999, "x", false)));

    let mut f = Tr::new();
    let _ = f.saturate();
    let _ = l.register(cell(13, OP_RUN, f.run(CP, ENOENT, "x", false)));

    l
}

/// 四路注入。
pub fn inject_four() -> [bool; 4] {
    let mut out = [false; 4];

    // 一路：账本形状、对账、重放、指纹。
    let l = build_ledger();
    let rep = l.replay();
    let root = l.seal();
    out[0] = l.len() == SAMPLES
        && l.ok_count() == 8
        && l.err_count() == 6
        && l.audit().is_ok()
        && rep == Ok(root)
        && root != 0;

    // 二路：快乐路径逐字相等，吞并计数与退出码精确。
    let mut t = Tr::new();
    let r0 = t.run(LS, ENOENT, "a.txt", false);
    let v0 = t.verify(0, LIT_LS_ACCESS);
    let r1 = t.run(RM, ENOENT, "gone", true);
    let v1 = t.verify(1, "rm: cannot remove 'gone': No such file or directory") == Err(MS_ENOBS);
    out[1] = r0 == Ok(1)
        && v0 == Ok(1)
        && r1 == Ok(0)
        && v1
        && t.len() == 1
        && t.suppressed() == 1
        && t.emitted() == 1
        && t.status() == 2
        && t.consistent();

    // 三路：六类拒绝彼此可辨。
    let mut a = Tr::new();
    let p = a.run(9, ENOENT, "x", false) == Err(MS_EPROG);
    let b = a.run(LS, ENOENT, "", false) == Err(MS_EINVAL);
    let c = a.run(LS, ENOENT, LONG_PATH, false) == Err(MS_ELONG);
    let d = a.run(LS, ENOENT, "o'b", false) == Err(MS_EQUOTE);
    let e = a.run(LS, 4242, "x", false) == Err(MS_EUNK);
    let mut s = Tr::new();
    let _ = s.saturate();
    let f = s.run(RM, ENOENT, "x", false) == Err(MS_EFULL);
    out[2] = p && b && c && d && e && f && a.consistent() && s.consistent();

    // 四路：闸门次序、吞并范围与观测边界。
    let mut q = Tr::new();
    let g1 = q.run(9, 4242, "", false) == Err(MS_EPROG);
    let g2 = q.run(LS, 4242, "", false) == Err(MS_EINVAL);
    let g3 = q.run(RM, ENOENT, "o'b", true) == Err(MS_EQUOTE);
    let g4 = q.run(RM, EISDIR, "d", true) == Ok(1);
    let g5 = q.verify(9, LIT_RM_DIR) == Err(MS_ENOBS);
    let g6 = q.verify(0, LIT_RM_DIR) == Err(MS_ETRUTH);
    out[3] = g1 && g2 && g3 && g4 && g5 && g6 && q.consistent();

    out
}

/// 十三项判据。
pub fn run_unx11397_checks() -> CheckSet {
    let mut s = CheckSet::new("unx11397.ls_cp_rm_text_truth");
    let l = build_ledger();

    s.add_dyn(
        "MS1-十四格八允六拒",
        l.len() == SAMPLES && l.ok_count() == 8 && l.err_count() == 6 && l.audit().is_ok(),
        &format!("n={} ok={} err={}", l.len(), l.ok_count(), l.err_count()),
    );

    s.add(
        "MS2-ls原文cannot-access",
        {
            let mut t = Tr::new();
            let _ = t.run(LS, ENOENT, "a.txt", false);
            t.msg(0).map(|x| x.eq_str(LIT_LS_ACCESS)) == Some(true)
        },
        "static",
    );

    s.add(
        "MS3-cp原文cannot-stat",
        {
            let mut t = Tr::new();
            let _ = t.run(CP, ENOENT, "b.txt", false);
            t.msg(0).map(|x| x.eq_str(LIT_CP_STAT)) == Some(true)
        },
        "static",
    );

    s.add(
        "MS4-cp省略目录无errno",
        {
            let mut t = Tr::new();
            let _ = t.run(CP, EISDIR, "d1", false);
            t.msg(0).map(|x| x.eq_str(LIT_CP_OMIT)) == Some(true)
                && t.msg(0).map(|x| x.eq_str("cp: -r not specified; omitting directory 'd1': Is a directory"))
                    == Some(false)
        },
        "static",
    );

    s.add(
        "MS5-rm原文Is-a-directory",
        l.sample(5).map(|x| x.value) == Some(1)
            && {
                let mut t = Tr::new();
                let _ = t.run(RM, EISDIR, "d2", false);
                t.msg(0).map(|x| x.eq_str(LIT_RM_DIR)) == Some(true)
            },
        "static",
    );

    s.add(
        "MS6-rm原文Permission-denied",
        {
            let mut t = Tr::new();
            let _ = t.run(RM, EACCES, "q", false);
            t.msg(0).map(|x| x.eq_str(LIT_RM_ACC)) == Some(true)
                && t.status() == 1
        },
        "static",
    );

    s.add(
        "MS7-强制档吞ENOENT",
        l.sample(2).map(|x| x.value) == Some(0)
            && {
                let mut t = Tr::new();
                let r = t.run(RM, ENOENT, "gone", true);
                r == Ok(0) && t.len() == 0 && t.suppressed() == 1 && t.status() == 0
            },
        "static",
    );

    s.add(
        "MS8-退出码阶梯",
        {
            let mut t = Tr::new();
            let _ = t.run(LS, EACCES, "m", false);
            let minor = t.status();
            let _ = t.run(LS, ENOENT, "n", false);
            let serious = t.status();
            let mut u = Tr::new();
            let _ = u.run(CP, ENOSPC, "out", false);
            minor == 1 && serious == 2 && u.status() == 1 && l.sample(7).map(|x| x.value) == Some(2)
        },
        "static",
    );

    s.add(
        "MS9-空路径EINVAL",
        l.sample(9).map(|x| x.code) == Some(MS_EINVAL),
        "static",
    );

    s.add(
        "MS10-超长路径ELONG",
        l.sample(10).map(|x| x.code) == Some(MS_ELONG),
        "static",
    );

    s.add(
        "MS11-未知errno-EUNK",
        l.sample(12).map(|x| x.code) == Some(MS_EUNK),
        "static",
    );

    s.add(
        "MS12-真值不符ETRUTH",
        l.sample(11).map(|x| x.code) == Some(MS_EQUOTE)
            && {
                let mut t = Tr::new();
                let _ = t.run(LS, ENOENT, "a.txt", false);
                t.verify(0, "ls: cannot access 'a.txt': No such file or directory ") == Err(MS_ETRUTH)
                    && t.msg(0).map(|x| x.first_diff(LIT_LS_ACCESS)) == Some(None)
            },
        "static",
    );

    s.add(
        "MS13-冻结重放四注入",
        {
            let mut f = build_ledger();
            let rep = f.replay();
            let one = f.freeze();
            let two = f.freeze();
            let inj = inject_four();
            one.is_ok()
                && one == rep
                && two.is_err()
                && f.rejects() == 1
                && f.is_frozen()
                && f.root() != 0
                && f.root() == f.seal()
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
        let s = run_unx11397_checks();
        assert_eq!(s.len(), 13);
        assert!(s.all_passed());
    }

    #[test]
    fn three_literals_exact() {
        let mut t = Tr::new();
        assert_eq!(t.run(LS, ENOENT, "a.txt", false), Ok(1));
        assert_eq!(t.run(CP, ENOENT, "b.txt", false), Ok(1));
        assert_eq!(t.run(RM, EISDIR, "d2", false), Ok(1));
        assert_eq!(t.verify(0, LIT_LS_ACCESS), Ok(1));
        assert_eq!(t.verify(1, LIT_CP_STAT), Ok(1));
        assert_eq!(t.verify(2, LIT_RM_DIR), Ok(1));
        assert!(t.consistent());
    }

    #[test]
    fn omitting_directory_has_no_suffix() {
        let mut t = Tr::new();
        let _ = t.run(CP, EISDIR, "d1", false);
        let m = t.msg(0).expect("message");
        assert!(m.eq_str(LIT_CP_OMIT));
        assert_eq!(m.len(), LIT_CP_OMIT.len());
        assert_eq!(m.first_diff(LIT_CP_OMIT), None);
    }

    #[test]
    fn force_swallows_only_enoent() {
        let mut t = Tr::new();
        assert_eq!(t.run(RM, ENOENT, "gone", true), Ok(0));
        assert_eq!(t.len(), 0);
        assert_eq!(t.suppressed(), 1);
        assert_eq!(t.status(), 0);
        assert_eq!(t.run(RM, EISDIR, "d", true), Ok(1));
        assert_eq!(t.status(), 1);
        assert_eq!(t.run(CP, ENOENT, "b", true), Ok(1));
        assert_eq!(t.len(), 2);
    }

    #[test]
    fn status_ladder_only_rises() {
        let mut t = Tr::new();
        let _ = t.run(LS, EACCES, "m", false);
        assert_eq!(t.status(), 1);
        let _ = t.run(LS, EACCES, "n", false);
        assert_eq!(t.status(), 1);
        let _ = t.run(LS, ENOENT, "n", false);
        assert_eq!(t.status(), 2);
        let _ = t.run(RM, ENOENT, "n", false);
        assert_eq!(t.status(), 2);
    }

    #[test]
    fn gate_order_and_capacity() {
        let mut t = Tr::new();
        assert_eq!(t.run(9, 4242, "", false), Err(MS_EPROG));
        assert_eq!(t.run(LS, 4242, "", false), Err(MS_EINVAL));
        assert_eq!(t.run(LS, 4242, LONG_PATH, false), Err(MS_ELONG));
        assert_eq!(t.run(LS, 4242, "o'b", false), Err(MS_EQUOTE));
        assert_eq!(t.run(LS, 4242, "x", false), Err(MS_EUNK));
        assert_eq!(t.saturate(), Ok(CAP));
        assert_eq!(t.run(LS, ENOENT, "x", false), Err(MS_EFULL));
        assert_eq!(t.verify(CAP, LIT_LS_ACCESS), Err(MS_ENOBS));
        assert!(t.consistent());
    }

    #[test]
    fn inject_four_is_all_true() {
        assert_eq!(inject_four(), [true; 4]);
    }
}
