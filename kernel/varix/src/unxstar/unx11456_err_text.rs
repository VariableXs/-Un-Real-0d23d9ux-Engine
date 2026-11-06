//! UNX-F11456 错误文案真值（高频三档）（0x20c40 段）
//!
//! 判据里比对错误文案不是为了好看，而是因为脚本与回归基线会直接匹配这些字节。
//! 最高频的三档是“文件不存在”“是个目录”“权限不足”，对应的 strerror 文本在
//! 任何 C 本地化下都必须逐字节一致：`No such file or directory`（25 字节）、
//! `Is a directory`（14 字节）、`Permission denied`（17 字节）。少一个空格、
//! 多一个句点，基线就红。
//!
//! 工具前缀是第二层真值：`grep: ` 恒为 6 字节；`sort: ` 也是 6 字节，但 `sort`
//! 读目录时走的是 `sort: cannot read: `（19 字节）——同一条 EISDIR 在两个工具里
//! 前缀不同，把前缀写成常量拼接是最容易漂的地方。模型把前缀、名字、`: `、正文
//! 四段显式拼装，长度先算后写：总长超缓冲记 ESHORT，名字为空记 ENONAME，超过
//! 32 字节记 ENAME，未知错误类记 ECLASS，未知工具记 ETOOL。
//!
//! 闸门次序：空名 → 超长名 → 未知工具 → 未知错误类 → 缓冲不足。名字先于工具，
//! 是因为空名是调用方的编程错误，应当在任何查表之前暴露。
//!
//! 账本十四格：八格成功（grep 三档长度、sort 目录长度、strerror 长度、错误类
//! 映射、sort 权限长度、类目数），六格拒绝（未知类、未知工具、缓冲不足、
//! 名字超长、空名、容量满）。审计逐格钉值。

use crate::alloc::format;
use crate::checks::CheckSet;

/// 段基址。
pub const SEG_BASE: u32 = 0x20c40;

/// 段内低十六位。
const SEG_LO: u16 = (SEG_BASE & 0xffff) as u16;

/// 登记顺序错误。
pub const ET_EORDER: u16 = SEG_LO + 0x01;
/// 重复登记。
pub const ET_EDUP: u16 = SEG_LO + 0x02;
/// 已冻结。
pub const ET_EFROZEN: u16 = SEG_LO + 0x03;
/// 观测缺失。
pub const ET_ENOBS: u16 = SEG_LO + 0x04;
/// 结论不符。
pub const ET_EMISMATCH: u16 = SEG_LO + 0x05;
/// 账本失衡。
pub const ET_EBAL: u16 = SEG_LO + 0x06;
/// 容量满。
pub const ET_EFULL: u16 = SEG_LO + 0x07;
/// 未知错误类。
pub const ET_ECLASS: u16 = SEG_LO + 0x08;
/// 未知工具。
pub const ET_ETOOL: u16 = SEG_LO + 0x09;
/// 缓冲不足。
pub const ET_ESHORT: u16 = SEG_LO + 0x0a;
/// 名字超长。
pub const ET_ENAME: u16 = SEG_LO + 0x0b;
/// 空名字。
pub const ET_ENONAME: u16 = SEG_LO + 0x0c;
/// 票号不一致。
pub const ET_ESEAL: u16 = SEG_LO + 0x0d;

/// 工具：grep。
pub const TOOL_GREP: u8 = 1;
/// 工具：sort。
pub const TOOL_SORT: u8 = 2;

/// 错误类：不存在。
pub const CLS_MISS: u8 = 1;
/// 错误类：是目录。
pub const CLS_DIR: u8 = 2;
/// 错误类：权限不足。
pub const CLS_PERM: u8 = 3;
/// 错误类总数。
pub const CLASSES: u32 = 3;

/// errno：ENOENT。
pub const ENOENT: u16 = 2;
/// errno：EACCES。
pub const EACCES: u16 = 13;
/// errno：EISDIR。
pub const EISDIR: u16 = 21;

/// grep 前缀。
pub const PREFIX_GREP: &[u8] = b"grep: ";
/// sort 前缀。
pub const PREFIX_SORT: &[u8] = b"sort: ";
/// sort 读目录前缀。
pub const PREFIX_SORT_DIR: &[u8] = b"sort: cannot read: ";

/// 不存在正文。
pub const TXT_ENOENT: &[u8] = b"No such file or directory";
/// 目录正文。
pub const TXT_EISDIR: &[u8] = b"Is a directory";
/// 权限正文。
pub const TXT_EACCES: &[u8] = b"Permission denied";

/// 名字容量。
pub const NAME_CAP: usize = 32;

/// 操作：拼装文案。
pub const OP_TEXT: u8 = 1;
/// 操作：查类。
pub const OP_CLASS: u8 = 2;
/// 操作：账本自检。
pub const OP_LEDGER: u8 = 3;

/// 账本格数。
pub const ROWS: usize = 14;

/// FNV 偏移基。
pub const FNV_OFF: u64 = 0xcbf2_9ce4_8422_2325;
/// FNV 质数。
pub const FNV_PRIME: u64 = 0x100_0000_01b3;

/// 折入。
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

/// 错误类映射。
pub fn class_of(errno: u16) -> Result<u8, u16> {
    match errno {
        ENOENT => Ok(CLS_MISS),
        EISDIR => Ok(CLS_DIR),
        EACCES => Ok(CLS_PERM),
        _ => Err(ET_ECLASS),
    }
}

/// 正文查表。
pub fn strerror(class: u8) -> Result<&'static [u8], u16> {
    match class {
        CLS_MISS => Ok(TXT_ENOENT),
        CLS_DIR => Ok(TXT_EISDIR),
        CLS_PERM => Ok(TXT_EACCES),
        _ => Err(ET_ECLASS),
    }
}

/// 前缀查表：sort 读目录单独一档。
pub fn prefix(tool: u8, class: u8) -> Result<&'static [u8], u16> {
    match tool {
        TOOL_GREP => Ok(PREFIX_GREP),
        TOOL_SORT => {
            if class == CLS_DIR {
                Ok(PREFIX_SORT_DIR)
            } else {
                Ok(PREFIX_SORT)
            }
        }
        _ => Err(ET_ETOOL),
    }
}

/// 拷贝到缓冲，返回新写入位置（调用方已保证容量）。
fn copy_into(dst: &mut [u8], mut w: usize, src: &[u8]) -> usize {
    let mut i = 0usize;
    while i < src.len() {
        let b = match src.get(i) {
            Some(v) => *v,
            None => break,
        };
        match dst.get_mut(w) {
            Some(d) => {
                *d = b;
                w += 1;
            }
            None => return w,
        }
        i += 1;
    }
    w
}

/// 拼装一条完整文案，返回字节数。
pub fn message(tool: u8, class: u8, name: &[u8], buf: &mut [u8]) -> Result<u32, u16> {
    if name.is_empty() {
        return Err(ET_ENONAME);
    }
    if name.len() > NAME_CAP {
        return Err(ET_ENAME);
    }
    let pre = prefix(tool, class)?;
    let txt = strerror(class)?;
    let total = pre.len() + name.len() + 2 + txt.len();
    if total > buf.len() {
        return Err(ET_ESHORT);
    }
    let mut w = 0usize;
    w = copy_into(buf, w, pre);
    w = copy_into(buf, w, name);
    w = copy_into(buf, w, b": ");
    w = copy_into(buf, w, txt);
    match u32::try_from(w) {
        Ok(v) => Ok(v),
        Err(_) => Err(ET_ESHORT),
    }
}

/// 文案长度（不写缓冲）。
pub fn message_len(tool: u8, class: u8, name_len: usize) -> Result<u32, u16> {
    if name_len == 0 {
        return Err(ET_ENONAME);
    }
    if name_len > NAME_CAP {
        return Err(ET_ENAME);
    }
    let pre = prefix(tool, class)?;
    let txt = strerror(class)?;
    let total = pre.len() + name_len + 2 + txt.len();
    match u32::try_from(total) {
        Ok(v) => Ok(v),
        Err(_) => Err(ET_ESHORT),
    }
}

/// 一格。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Sample {
    pub slot: u8,
    pub op: u8,
    pub ok: bool,
    pub code: u16,
    pub value: u32,
}

impl Sample {
    /// 空白格。
    pub const fn blank() -> Sample {
        Sample {
            slot: 0,
            op: 0,
            ok: false,
            code: 0,
            value: 0,
        }
    }
}

/// 审计回执。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AuditReport {
    pub rows: u32,
    pub ok: u32,
    pub err: u32,
    pub root: u64,
}

/// 样本账本。
pub struct Ledger {
    rows: [Sample; ROWS],
    len: usize,
    cap: usize,
    frozen: bool,
    root: u64,
    rejects: u32,
}

impl Ledger {
    /// 默认容量。
    pub const fn new() -> Ledger {
        Ledger {
            rows: [Sample::blank(); ROWS],
            len: 0,
            cap: ROWS,
            frozen: false,
            root: 0,
            rejects: 0,
        }
    }

    /// 指定容量。
    pub const fn with_cap(cap: usize) -> Ledger {
        Ledger {
            rows: [Sample::blank(); ROWS],
            len: 0,
            cap: if cap > ROWS { ROWS } else { cap },
            frozen: false,
            root: 0,
            rejects: 0,
        }
    }

    /// 登记一格：冻结 → 重复 → 顺序 → 容量 → 一致性。
    pub fn register(&mut self, s: Sample) -> Result<(), u16> {
        if self.frozen {
            self.rejects += 1;
            return Err(ET_EFROZEN);
        }
        let mut i = 0usize;
        while i < self.len {
            match self.rows.get(i) {
                Some(x) => {
                    if x.slot == s.slot {
                        self.rejects += 1;
                        return Err(ET_EDUP);
                    }
                }
                None => return Err(ET_ENOBS),
            }
            i += 1;
        }
        if s.slot as usize != self.len {
            self.rejects += 1;
            return Err(ET_EORDER);
        }
        if self.len >= self.cap || self.len >= ROWS {
            self.rejects += 1;
            return Err(ET_EFULL);
        }
        if s.op == 0 {
            self.rejects += 1;
            return Err(ET_EMISMATCH);
        }
        if s.ok && s.code != 0 {
            self.rejects += 1;
            return Err(ET_EMISMATCH);
        }
        if !s.ok && s.code == 0 {
            self.rejects += 1;
            return Err(ET_EMISMATCH);
        }
        match self.rows.get_mut(self.len) {
            Some(x) => *x = s,
            None => return Err(ET_EFULL),
        }
        self.len += 1;
        Ok(())
    }

    /// 格数。
    pub fn len(&self) -> usize {
        self.len
    }

    /// 是否为空。
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// 取一格。
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
    pub fn audit(&self) -> Result<AuditReport, u16> {
        if self.len != ROWS {
            return Err(ET_EBAL);
        }
        if self.ok_count() != 8 || self.err_count() != 6 {
            return Err(ET_EBAL);
        }
        let mut i = 0usize;
        while i < self.len {
            let s = match self.rows.get(i) {
                Some(v) => *v,
                None => return Err(ET_ENOBS),
            };
            if s.slot as usize != i {
                return Err(ET_EORDER);
            }
            if s.ok {
                if s.code != 0 {
                    return Err(ET_EMISMATCH);
                }
            } else if s.code == 0 || s.value != 0 {
                return Err(ET_EMISMATCH);
            }
            i += 1;
        }
        let vals = [34u32, 23, 26, 36, 17, 1, 26, 3];
        let mut k = 0usize;
        while k < vals.len() {
            match (self.sample(k), vals.get(k)) {
                (Some(s), Some(w)) => {
                    if !s.ok || s.value != *w {
                        return Err(ET_EMISMATCH);
                    }
                }
                _ => return Err(ET_EBAL),
            }
            k += 1;
        }
        let codes = [
            ET_ECLASS, ET_ETOOL, ET_ESHORT, ET_ENAME, ET_ENONAME, ET_EFULL,
        ];
        let mut c = 0usize;
        let mut j = 8usize;
        while j < self.len {
            match self.rows.get(j) {
                Some(s) => match codes.get(c) {
                    Some(w) => {
                        if s.ok || s.code != *w || s.value != 0 {
                            return Err(ET_EMISMATCH);
                        }
                    }
                    None => return Err(ET_EBAL),
                },
                None => return Err(ET_ENOBS),
            }
            c += 1;
            j += 1;
        }
        if c != codes.len() {
            return Err(ET_EBAL);
        }
        Ok(AuditReport {
            rows: self.len as u32,
            ok: self.ok_count() as u32,
            err: self.err_count() as u32,
            root: self.seal(),
        })
    }

    /// 熔断票号。
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
    pub fn freeze(&mut self) -> Result<u64, u16> {
        if self.frozen {
            self.rejects += 1;
            return Err(ET_EFROZEN);
        }
        let s = self.seal();
        self.root = s;
        self.frozen = true;
        Ok(s)
    }

    /// 重放。
    pub fn replay(&self) -> Result<u64, u16> {
        let fresh = build_ledger();
        if fresh.len != self.len {
            return Err(ET_EBAL);
        }
        let h = fresh.seal();
        if self.frozen && h != self.root {
            return Err(ET_ESEAL);
        }
        Ok(h)
    }

    /// 封存票号。
    pub fn root(&self) -> u64 {
        self.root
    }

    /// 是否已冻结。
    pub fn is_frozen(&self) -> bool {
        self.frozen
    }

    /// 越闸次数。
    pub fn rejects(&self) -> u32 {
        self.rejects
    }
}

/// 成功格。
fn ok_row(slot: u8, op: u8, value: u32) -> Sample {
    Sample {
        slot,
        op,
        ok: true,
        code: 0,
        value,
    }
}

/// 失败格。
fn err_row(slot: u8, op: u8, code: u16) -> Sample {
    Sample {
        slot,
        op,
        ok: false,
        code,
        value: 0,
    }
}

/// 取登记错误码。
fn reg_code(r: Result<(), u16>) -> u16 {
    match r {
        Ok(()) => 0,
        Err(c) => c,
    }
}

/// 结果取数值。
fn value_of(r: Result<u32, u16>) -> u32 {
    match r {
        Ok(v) => v,
        Err(_) => 0xffff_ffff,
    }
}

/// 渲染一条消息并取长度。
fn render_len(tool: u8, class: u8, name: &[u8]) -> u32 {
    let mut buf = [0u8; 128];
    value_of(message(tool, class, name, &mut buf))
}

/// 构建十四格账本。
pub fn build_ledger() -> Ledger {
    let mut l = Ledger::new();

    let _ = l.register(ok_row(0, OP_TEXT, render_len(TOOL_GREP, CLS_MISS, b"x")));
    let _ = l.register(ok_row(1, OP_TEXT, render_len(TOOL_GREP, CLS_DIR, b"x")));
    let _ = l.register(ok_row(2, OP_TEXT, render_len(TOOL_GREP, CLS_PERM, b"x")));
    let _ = l.register(ok_row(3, OP_TEXT, render_len(TOOL_SORT, CLS_DIR, b"x")));

    let perm_len = match strerror(CLS_PERM) {
        Ok(t) => u32::try_from(t.len()).unwrap_or(0),
        Err(_) => 0,
    };
    let _ = l.register(ok_row(4, OP_TEXT, perm_len));

    let class = match class_of(ENOENT) {
        Ok(v) => u32::from(v),
        Err(_) => 0,
    };
    let _ = l.register(ok_row(5, OP_CLASS, class));

    let _ = l.register(ok_row(6, OP_TEXT, render_len(TOOL_SORT, CLS_PERM, b"d")));
    let _ = l.register(ok_row(7, OP_CLASS, CLASSES));

    let cls = match class_of(99) {
        Ok(_) => 0,
        Err(c) => c,
    };
    let _ = l.register(err_row(8, OP_CLASS, cls));

    let mut probe = [0u8; 128];
    let tool = match message(9, CLS_MISS, b"x", &mut probe) {
        Ok(_) => 0,
        Err(c) => c,
    };
    let _ = l.register(err_row(9, OP_TEXT, tool));

    let mut tight = [0u8; 8];
    let short = match message(TOOL_GREP, CLS_MISS, b"x", &mut tight) {
        Ok(_) => 0,
        Err(c) => c,
    };
    let _ = l.register(err_row(10, OP_TEXT, short));

    let long_name = [b'n'; NAME_CAP + 1];
    let name = match message(TOOL_GREP, CLS_MISS, &long_name, &mut probe) {
        Ok(_) => 0,
        Err(c) => c,
    };
    let _ = l.register(err_row(11, OP_TEXT, name));

    let none = match message(TOOL_GREP, CLS_MISS, b"", &mut probe) {
        Ok(_) => 0,
        Err(c) => c,
    };
    let _ = l.register(err_row(12, OP_TEXT, none));

    let mut small = Ledger::with_cap(3);
    let _ = small.register(ok_row(0, OP_TEXT, 1));
    let _ = small.register(ok_row(1, OP_TEXT, 1));
    let _ = small.register(ok_row(2, OP_TEXT, 1));
    let full = reg_code(small.register(ok_row(3, OP_TEXT, 1)));
    let _ = l.register(err_row(13, OP_LEDGER, full));

    l
}

/// 四路注入：重放冻结、三档长度、前缀纪律、篡改复原。
pub fn inject_four() -> [bool; 4] {
    let mut out = [false; 4];

    let mut l = build_ledger();
    let before = l.len();
    let rep = l.replay();
    let one = l.freeze();
    let two = l.freeze();
    let root = l.root();
    out[0] = rep == one
        && one == Ok(root)
        && two == Err(ET_EFROZEN)
        && l.rejects() == 1
        && before == l.len()
        && root != 0
        && l.audit().is_ok();

    let a = render_len(TOOL_GREP, CLS_MISS, b"x");
    let b = render_len(TOOL_GREP, CLS_DIR, b"x");
    let c = render_len(TOOL_GREP, CLS_PERM, b"x");
    let d = render_len(TOOL_SORT, CLS_DIR, b"x");
    out[1] = a == 34 && b == 23 && c == 26 && d == 36 && a > b && b < c && d > c;

    out[2] = PREFIX_GREP.len() == 6
        && PREFIX_SORT.len() == 6
        && PREFIX_SORT_DIR.len() == 19
        && prefix(TOOL_GREP, CLS_DIR) == Ok(PREFIX_GREP)
        && prefix(TOOL_SORT, CLS_DIR) == Ok(PREFIX_SORT_DIR)
        && prefix(TOOL_SORT, CLS_PERM) == Ok(PREFIX_SORT);

    out[3] = {
        let mut t = build_ledger();
        let clean = t.audit().is_ok();
        let old = match t.sample(4) {
            Some(s) => s.value,
            None => 0,
        };
        if let Some(r) = t.rows.get_mut(4) {
            r.value = old.saturating_add(1);
        }
        let caught = t.audit() == Err(ET_EMISMATCH);
        if let Some(r) = t.rows.get_mut(4) {
            r.value = old;
        }
        clean && caught && t.audit().is_ok()
    };

    out
}

/// 十三判据。
pub fn run_unx11456_checks() -> CheckSet {
    let mut s = CheckSet::new("unx11456.errtext");
    let l = build_ledger();

    s.add_dyn(
        "ET1-十四格齐备",
        l.len() == ROWS && l.ok_count() == 8 && l.err_count() == 6 && l.audit().is_ok(),
        &format!("n={} ok={} err={}", l.len(), l.ok_count(), l.err_count()),
    );

    s.add(
        "ET2-grep三档长度",
        l.sample(0).map(|x| x.value) == Some(34)
            && l.sample(1).map(|x| x.value) == Some(23)
            && l.sample(2).map(|x| x.value) == Some(26),
        "static",
    );

    s.add(
        "ET3-sort目录前缀",
        l.sample(3).map(|x| x.value) == Some(36)
            && message_len(TOOL_SORT, CLS_DIR, 1) == Ok(36)
            && message_len(TOOL_SORT, CLS_PERM, 1) == Ok(26),
        "static",
    );

    s.add(
        "ET4-正文逐字节",
        strerror(CLS_MISS) == Ok(TXT_ENOENT)
            && strerror(CLS_DIR) == Ok(TXT_EISDIR)
            && strerror(CLS_PERM) == Ok(TXT_EACCES)
            && TXT_ENOENT.len() == 25
            && TXT_EISDIR.len() == 14
            && TXT_EACCES.len() == 17,
        "static",
    );

    s.add(
        "ET5-错误类映射",
        l.sample(5).map(|x| x.value) == Some(1)
            && class_of(2) == Ok(CLS_MISS)
            && class_of(21) == Ok(CLS_DIR)
            && class_of(13) == Ok(CLS_PERM)
            && class_of(1) == Err(ET_ECLASS),
        "static",
    );

    s.add(
        "ET6-同文不同前缀",
        message_len(TOOL_GREP, CLS_PERM, 1) == Ok(26)
            && message_len(TOOL_SORT, CLS_PERM, 1) == Ok(26)
            && message_len(TOOL_GREP, CLS_DIR, 1) == Ok(23)
            && message_len(TOOL_SORT, CLS_DIR, 1) == Ok(36),
        "static",
    );

    s.add(
        "ET7-名字长度线性",
        message_len(TOOL_GREP, CLS_MISS, 1) == Ok(34)
            && message_len(TOOL_GREP, CLS_MISS, 2) == Ok(35)
            && message_len(TOOL_GREP, CLS_MISS, NAME_CAP) == Ok(65)
            && message_len(TOOL_GREP, CLS_MISS, NAME_CAP + 1) == Err(ET_ENAME),
        "static",
    );

    s.add(
        "ET8-拼装字节流",
        {
            let mut buf = [0u8; 64];
            match message(TOOL_GREP, CLS_MISS, b"x", &mut buf) {
                Ok(n) => match buf.get(..n as usize) {
                    Some(v) => v == b"grep: x: No such file or directory",
                    None => false,
                },
                Err(_) => false,
            }
        },
        "static",
    );

    s.add(
        "ET9-未知类与工具",
        l.sample(8).map(|x| x.code) == Some(ET_ECLASS)
            && l.sample(9).map(|x| x.code) == Some(ET_ETOOL)
            && strerror(0) == Err(ET_ECLASS)
            && prefix(0, CLS_MISS) == Err(ET_ETOOL),
        "static",
    );

    s.add(
        "ET10-缓冲不足闸门",
        l.sample(10).map(|x| x.code) == Some(ET_ESHORT)
            && {
                let mut tight = [0u8; 8];
                message(TOOL_GREP, CLS_MISS, b"x", &mut tight) == Err(ET_ESHORT)
            }
            && {
                let mut exact = [0u8; 34];
                message(TOOL_GREP, CLS_MISS, b"x", &mut exact) == Ok(34)
            },
        "static",
    );

    s.add(
        "ET11-名字闸门",
        l.sample(11).map(|x| x.code) == Some(ET_ENAME)
            && l.sample(12).map(|x| x.code) == Some(ET_ENONAME)
            && {
                let long = [b'n'; NAME_CAP + 1];
                let mut buf = [0u8; 128];
                message(TOOL_GREP, CLS_MISS, &long, &mut buf) == Err(ET_ENAME)
                    && message(TOOL_GREP, CLS_MISS, b"", &mut buf) == Err(ET_ENONAME)
            },
        "static",
    );

    s.add(
        "ET12-容量闸门",
        l.sample(13).map(|x| x.code) == Some(ET_EFULL)
            && {
                let mut small = Ledger::with_cap(2);
                let _ = small.register(ok_row(0, OP_TEXT, 1));
                let _ = small.register(ok_row(1, OP_TEXT, 1));
                small.register(ok_row(2, OP_TEXT, 1)) == Err(ET_EFULL) && small.len() == 2
            },
        "static",
    );

    s.add(
        "ET13-冻结重放与四注入",
        {
            let mut f = build_ledger();
            let rep = f.replay();
            let one = f.freeze();
            let two = f.freeze();
            let inj = inject_four();
            one.is_ok()
                && one == rep
                && two == Err(ET_EFROZEN)
                && f.rejects() == 1
                && f.is_frozen()
                && f.root() != 0
                && inj == [true, true, true, true]
        },
        "static",
    );

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn three_tiers_of_text() {
        assert_eq!(TXT_ENOENT.len(), 25);
        assert_eq!(TXT_EISDIR.len(), 14);
        assert_eq!(TXT_EACCES.len(), 17);
        assert_eq!(message_len(TOOL_GREP, CLS_MISS, 1), Ok(34));
        assert_eq!(message_len(TOOL_GREP, CLS_DIR, 1), Ok(23));
        assert_eq!(message_len(TOOL_GREP, CLS_PERM, 1), Ok(26));
    }

    #[test]
    fn sort_directory_prefix_differs() {
        assert_eq!(PREFIX_SORT_DIR.len(), 19);
        assert_eq!(message_len(TOOL_SORT, CLS_DIR, 1), Ok(36));
        assert_eq!(message_len(TOOL_SORT, CLS_PERM, 1), Ok(26));
        assert_eq!(prefix(TOOL_SORT, CLS_DIR), Ok(PREFIX_SORT_DIR));
    }

    #[test]
    fn message_is_exact() {
        let mut buf = [0u8; 64];
        let n = message(TOOL_GREP, CLS_PERM, b"a", &mut buf).unwrap_or(0);
        let got = buf.get(..n as usize).unwrap_or(&[]);
        assert_eq!(got, b"grep: a: Permission denied");
    }

    #[test]
    fn class_mapping() {
        assert_eq!(class_of(ENOENT), Ok(CLS_MISS));
        assert_eq!(class_of(EISDIR), Ok(CLS_DIR));
        assert_eq!(class_of(EACCES), Ok(CLS_PERM));
        assert_eq!(class_of(0), Err(ET_ECLASS));
        assert_eq!(strerror(CLS_PERM), Ok(TXT_EACCES));
    }

    #[test]
    fn name_and_buffer_gates() {
        let mut buf = [0u8; 128];
        assert_eq!(message(TOOL_GREP, CLS_MISS, b"", &mut buf), Err(ET_ENONAME));
        let long = [b'n'; NAME_CAP + 1];
        assert_eq!(message(TOOL_GREP, CLS_MISS, &long, &mut buf), Err(ET_ENAME));
        let mut tight = [0u8; 8];
        assert_eq!(
            message(TOOL_GREP, CLS_MISS, b"x", &mut tight),
            Err(ET_ESHORT)
        );
    }

    #[test]
    fn ledger_shape() {
        let l = build_ledger();
        assert_eq!(l.len(), ROWS);
        assert_eq!(l.ok_count(), 8);
        assert_eq!(l.err_count(), 6);
        assert!(l.audit().is_ok());
    }

    #[test]
    fn freeze_then_replay_matches() {
        let mut l = build_ledger();
        let root = l.seal();
        assert_eq!(l.freeze(), Ok(root));
        assert_eq!(l.replay(), Ok(root));
        assert_eq!(l.freeze(), Err(ET_EFROZEN));
    }

    #[test]
    fn checks_pass() {
        let s = run_unx11456_checks();
        assert_eq!(s.len(), 13);
        assert!(s.all_passed(), "{}", s.red_items_text());
    }
}
