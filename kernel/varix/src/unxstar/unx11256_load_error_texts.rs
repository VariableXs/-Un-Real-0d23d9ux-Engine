//! UNX-F11256 坏路径与搜索失败错误文本（0x1f380 段）
//!
//! 模型把装载失败报告拆成“程序名 + 固定文法片段 + 对象名”三段，
//! 用固定字节缓冲区逐段拼出完整消息，不借助任何字符串格式化设施。
//! 文法族以 glibc 的原文为准：装载期失败统一包含
//! “error while loading shared libraries: ”，随后跟对象名和具体原因；
//! 符号期失败则走 “undefined symbol: ”“symbol lookup error: ”
//! 与 “relocation error: ” 三条独立文法。渲染完成后由审计逐字节
//! 复算长度：程序名长度 + 固定片段长度 + 对象名长度必须等于缓冲区
//! 长度，换行符一律禁止，装载类文法的固定前缀必须恰好出现一次。
//! 所有失败路径的退出码都是 127，与 shell 观察到的 loader 行为一致。
//! 拒绝条件：程序名为空、对象名为空、未知文法编号、装载类对象名含
//! 冒号造成文法歧义、消息超长。十二个样本逐格登记并观测。

use crate::checks::CheckSet;

/// 段基址。
pub const SEG_BASE: u32 = 0x1f380;

/// 编号非法。
pub const ET_EID: u32 = SEG_BASE + 0x01;
/// 状态顺序错误。
pub const ET_EORDER: u32 = SEG_BASE + 0x02;
/// 重复登记。
pub const ET_EDUP: u32 = SEG_BASE + 0x03;
/// 已冻结。
pub const ET_EFROZEN: u32 = SEG_BASE + 0x04;
/// 观测不存在的格。
pub const ET_ENOBS: u32 = SEG_BASE + 0x05;
/// 观测结果不符。
pub const ET_EMISMATCH: u32 = SEG_BASE + 0x06;
/// 账本失衡。
pub const ET_EBAL: u32 = SEG_BASE + 0x07;
/// 必填字段为空。
pub const ET_EEMPTY: u32 = SEG_BASE + 0x08;
/// 未知文法编号。
pub const ET_EKIND: u32 = SEG_BASE + 0x09;
/// 对象名含歧义字符。
pub const ET_EBADCHAR: u32 = SEG_BASE + 0x0a;
/// 消息超长。
pub const ET_ETRUNC: u32 = SEG_BASE + 0x0b;
/// 缺少程序名。
pub const ET_EMISSING: u32 = SEG_BASE + 0x0c;

/// 装载器失败统一退出码。
pub const EXIT_LOADER: u8 = 127;
/// 程序名缓冲上限。
pub const PROG_MAX: usize = 24;
/// 对象名缓冲上限。
pub const OBJ_MAX: usize = 56;
/// 消息缓冲上限。
pub const MSG_MAX: usize = 192;
/// 文法种类数。
pub const KINDS: usize = 6;
/// 样本个数。
pub const SAMPLES: usize = 12;

/// 缺失共享对象。
pub const K_MISSING: u8 = 0;
/// 权限不足。
pub const K_PERM: u8 = 1;
/// ELF 类别不符。
pub const K_CLASS: u8 = 2;
/// 未定义符号。
pub const K_UNDEF: u8 = 3;
/// 符号查找失败。
pub const K_LOOKUP: u8 = 4;
/// 重定位失败。
pub const K_RELOC: u8 = 5;

/// 装载类公共片段。
pub const LOAD_PREFIX: &[u8] = b": error while loading shared libraries: ";
/// 文件不存在原因。
pub const NOSUCH: &[u8] = b": cannot open shared object file: No such file or directory";
/// 权限拒绝原因。
pub const PERM: &[u8] = b": cannot open shared object file: Permission denied";
/// ELF 类别原因。
pub const WRONG: &[u8] = b": wrong ELF class: ELFCLASS64";
/// 未定义符号片段。
pub const UNDEF_PREFIX: &[u8] = b": undefined symbol: ";
/// 符号查找片段。
pub const LOOKUP_PREFIX: &[u8] = b": symbol lookup error: ";
/// 重定位片段。
pub const RELOC_PREFIX: &[u8] = b": relocation error: ";

/// FNV 偏移基。
pub const FNV_OFF: u64 = 0xcbf2_9ce4_8422_2325;
/// FNV 质数。
pub const FNV_PRIME: u64 = 0x100_0000_01b3;

/// 一条失败报告样本。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ErrCase {
    pub kind: u8,
    pub prog: [u8; PROG_MAX],
    pub plen: u8,
    pub obj: [u8; OBJ_MAX],
    pub olen: u8,
}

impl ErrCase {
    /// 空样本。
    pub const fn blank() -> ErrCase {
        ErrCase {
            kind: K_MISSING,
            prog: [0; PROG_MAX],
            plen: 0,
            obj: [0; OBJ_MAX],
            olen: 0,
        }
    }
}

/// 把字符串写入固定数组，返回写入长度（不含 NUL）。
pub fn fill(dst: &mut [u8], s: &str) -> u8 {
    let bytes = s.as_bytes();
    let mut i = 0usize;
    while i < bytes.len() && i < dst.len() {
        if let Some(b) = dst.get_mut(i) {
            *b = match bytes.get(i) {
                Some(v) => *v,
                None => 0,
            };
        }
        i += 1;
    }
    i as u8
}

/// 构造样本。
pub fn make_case(kind: u8, prog: &str, obj: &str) -> ErrCase {
    let mut c = ErrCase::blank();
    c.kind = kind;
    c.plen = fill(&mut c.prog, prog);
    c.olen = fill(&mut c.obj, obj);
    c
}

/// 渲染后的消息。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Msg {
    pub buf: [u8; MSG_MAX],
    pub len: u16,
}

impl Msg {
    /// 空消息。
    pub const fn none() -> Msg {
        Msg { buf: [0; MSG_MAX], len: 0 }
    }

    /// 当前字节切片。
    pub fn bytes(&self) -> &[u8] {
        match self.buf.get(0..self.len as usize) {
            Some(v) => v,
            None => &[],
        }
    }
}

/// 一次成功渲染的结果。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Report {
    pub msg: Msg,
    pub exit: u8,
}

/// 追加字节串。
pub fn push(m: &mut Msg, part: &[u8]) -> Result<(), u32> {
    let mut i = 0usize;
    while i < part.len() {
        let at = m.len as usize;
        if at >= MSG_MAX {
            return Err(ET_ETRUNC);
        }
        if let Some(b) = m.buf.get_mut(at) {
            *b = match part.get(i) {
                Some(v) => *v,
                None => return Err(ET_ETRUNC),
            };
        }
        m.len = m.len.wrapping_add(1);
        i += 1;
    }
    Ok(())
}

/// 追加样本中前 n 字节。
pub fn push_slice(m: &mut Msg, src: &[u8], n: u8) -> Result<(), u32> {
    let part = match src.get(0..n as usize) {
        Some(v) => v,
        None => return Err(ET_ETRUNC),
    };
    push(m, part)
}

/// 文法固定片段的长度（不含程序名与对象名）。
pub fn lit_len(kind: u8) -> usize {
    match kind {
        K_MISSING => LOAD_PREFIX.len() + NOSUCH.len(),
        K_PERM => LOAD_PREFIX.len() + PERM.len(),
        K_CLASS => LOAD_PREFIX.len() + WRONG.len(),
        K_UNDEF => UNDEF_PREFIX.len(),
        K_LOOKUP => LOOKUP_PREFIX.len(),
        K_RELOC => RELOC_PREFIX.len(),
        _ => 0,
    }
}

fn has_byte(c: &ErrCase, b: u8) -> bool {
    let mut i = 0usize;
    while i < c.olen as usize {
        if let Some(v) = c.obj.get(i) {
            if *v == b {
                return true;
            }
        }
        i += 1;
    }
    false
}

fn count_sub(hay: &[u8], needle: &[u8]) -> u32 {
    if needle.is_empty() || hay.len() < needle.len() {
        return 0;
    }
    let mut n = 0u32;
    let mut i = 0usize;
    while i + needle.len() <= hay.len() {
        if let Some(v) = hay.get(i..i + needle.len()) {
            if v == needle {
                n = n.wrapping_add(1);
            }
        }
        i += 1;
    }
    n
}

fn starts_with(hay: &[u8], head: &[u8]) -> bool {
    if hay.len() < head.len() {
        return false;
    }
    match hay.get(0..head.len()) {
        Some(v) => v == head,
        None => false,
    }
}

fn ends_with(hay: &[u8], tail: &[u8]) -> bool {
    if hay.len() < tail.len() {
        return false;
    }
    let at = hay.len() - tail.len();
    match hay.get(at..) {
        Some(v) => v == tail,
        None => false,
    }
}

/// 渲染一条失败报告。
pub fn report(c: &ErrCase) -> Result<Report, u32> {
    if c.kind >= KINDS as u8 {
        return Err(ET_EKIND);
    }
    if c.plen == 0 {
        return Err(ET_EMISSING);
    }
    if c.olen == 0 {
        return Err(ET_EMISSING);
    }
    if has_byte(c, b'\n') {
        return Err(ET_EBADCHAR);
    }
    let load = c.kind == K_MISSING || c.kind == K_PERM || c.kind == K_CLASS;
    if load && has_byte(c, b':') {
        return Err(ET_EBADCHAR);
    }
    let mut m = Msg::none();
    push_slice(&mut m, &c.prog, c.plen)?;
    match c.kind {
        K_MISSING => {
            push(&mut m, LOAD_PREFIX)?;
            push_slice(&mut m, &c.obj, c.olen)?;
            push(&mut m, NOSUCH)?;
        }
        K_PERM => {
            push(&mut m, LOAD_PREFIX)?;
            push_slice(&mut m, &c.obj, c.olen)?;
            push(&mut m, PERM)?;
        }
        K_CLASS => {
            push(&mut m, LOAD_PREFIX)?;
            push_slice(&mut m, &c.obj, c.olen)?;
            push(&mut m, WRONG)?;
        }
        K_UNDEF => {
            push(&mut m, UNDEF_PREFIX)?;
            push_slice(&mut m, &c.obj, c.olen)?;
        }
        K_LOOKUP => {
            push(&mut m, LOOKUP_PREFIX)?;
            push_slice(&mut m, &c.obj, c.olen)?;
        }
        _ => {
            push(&mut m, RELOC_PREFIX)?;
            push_slice(&mut m, &c.obj, c.olen)?;
        }
    }
    Ok(Report { msg: m, exit: EXIT_LOADER })
}

/// 逐字节审计：长度、前缀、原因后缀、换行与文法片段出现次数。
pub fn audit(c: &ErrCase, r: &Report) -> bool {
    if r.exit != EXIT_LOADER {
        return false;
    }
    let want = c.plen as usize + lit_len(c.kind) + c.olen as usize;
    if r.msg.len as usize != want {
        return false;
    }
    let body = r.msg.bytes();
    let head = match c.prog.get(0..c.plen as usize) {
        Some(v) => v,
        None => return false,
    };
    if !starts_with(body, head) {
        return false;
    }
    if count_sub(body, b"\n") != 0 {
        return false;
    }
    match c.kind {
        K_MISSING => {
            count_sub(body, LOAD_PREFIX) == 1
                && count_sub(body, UNDEF_PREFIX) == 0
                && ends_with(body, NOSUCH)
        }
        K_PERM => {
            count_sub(body, LOAD_PREFIX) == 1
                && count_sub(body, UNDEF_PREFIX) == 0
                && ends_with(body, PERM)
        }
        K_CLASS => {
            count_sub(body, LOAD_PREFIX) == 1
                && count_sub(body, UNDEF_PREFIX) == 0
                && ends_with(body, WRONG)
        }
        K_UNDEF => count_sub(body, UNDEF_PREFIX) == 1,
        K_LOOKUP => {
            count_sub(body, LOOKUP_PREFIX) == 1 && count_sub(body, UNDEF_PREFIX) >= 1
        }
        K_RELOC => count_sub(body, RELOC_PREFIX) == 1,
        _ => false,
    }
}

/// 样本规格。
pub fn spec_of(id: usize) -> ErrCase {
    match id {
        0 => make_case(K_MISSING, "demo", "libfoo.so"),
        1 => make_case(K_PERM, "demo", "libbar.so"),
        2 => make_case(K_CLASS, "demo", "libbaz.so"),
        3 => make_case(K_UNDEF, "demo", "init_mod"),
        4 => make_case(K_LOOKUP, "demo", "libc.so.6: undefined symbol: strlen"),
        5 => make_case(K_RELOC, "demo", "libc.so.6: symbol foo, version GLIBC_2.2.5 not defined"),
        6 => make_case(K_MISSING, "/usr/bin/app", "libmissing.so.2"),
        7 => make_case(K_MISSING, "demo", ""),
        8 => make_case(6, "demo", "x"),
        9 => make_case(K_MISSING, "", "libx.so"),
        10 => make_case(K_MISSING, "demo", "lib:bad.so"),
        11 => make_case(K_UNDEF, "demo", ""),
        _ => ErrCase::blank(),
    }
}

/// 样本期望：(错误码, 文法, 程序名长, 对象名长, 消息长, 退出码)。
pub fn expected_of(id: usize) -> (u32, u8, u8, u8, u16, u8) {
    let c = spec_of(id);
    let base = c.plen as u16 + c.olen as u16 + lit_len(c.kind) as u16;
    match id {
        0 | 1 | 2 | 3 | 4 | 5 | 6 => (0, c.kind, c.plen, c.olen, base, EXIT_LOADER),
        7 => (ET_EMISSING, 0, 0, 0, 0, 0),
        8 => (ET_EKIND, 0, 0, 0, 0, 0),
        9 => (ET_EMISSING, 0, 0, 0, 0, 0),
        10 => (ET_EBADCHAR, 0, 0, 0, 0, 0),
        11 => (ET_EMISSING, 0, 0, 0, 0, 0),
        _ => (ET_EID, 0, 0, 0, 0, 0),
    }
}

/// 账本中的一格。
#[derive(Clone, Copy)]
struct ErrSlot {
    code: u8,
    kind: u8,
    plen: u8,
    olen: u8,
    exit: u8,
    seen: bool,
    len: u16,
    spec: ErrCase,
}

impl ErrSlot {
    const fn zero() -> ErrSlot {
        ErrSlot {
            code: 0,
            kind: 0,
            plen: 0,
            olen: 0,
            exit: 0,
            seen: false,
            len: 0,
            spec: ErrCase::blank(),
        }
    }
}

/// 失败文本账本。
pub struct ErrLedger {
    slots: [ErrSlot; SAMPLES],
    n: usize,
    accepted: u32,
    rejected: u32,
    observed: u32,
    matched: u32,
    frozen: bool,
}

impl ErrLedger {
    /// 空账本。
    pub const fn new() -> ErrLedger {
        ErrLedger {
            slots: [ErrSlot::zero(); SAMPLES],
            n: 0,
            accepted: 0,
            rejected: 0,
            observed: 0,
            matched: 0,
            frozen: false,
        }
    }

    fn fail(&mut self, code: u32) -> u32 {
        self.rejected = self.rejected.wrapping_add(1);
        code
    }

    fn duplicate(&self, c: &ErrCase) -> bool {
        let mut i = 0usize;
        while i < self.n {
            if let Some(s) = self.slots.get(i) {
                if s.spec == *c {
                    return true;
                }
            }
            i += 1;
        }
        false
    }

    fn commit(&mut self, c: &ErrCase, code: u32, r: &Report) -> Result<u64, u32> {
        let at = self.n;
        if at >= SAMPLES {
            return Err(self.fail(ET_EID));
        }
        if let Some(slot) = self.slots.get_mut(at) {
            slot.code = if code == 0 { 0 } else { code.wrapping_sub(SEG_BASE) as u8 };
            slot.kind = if code == 0 { c.kind } else { 0 };
            slot.plen = if code == 0 { c.plen } else { 0 };
            slot.olen = if code == 0 { c.olen } else { 0 };
            slot.exit = if code == 0 { r.exit } else { 0 };
            slot.len = if code == 0 { r.msg.len } else { 0 };
            slot.seen = false;
            slot.spec = *c;
        }
        self.n = at + 1;
        self.accepted = self.accepted.wrapping_add(1);
        Ok(self.fold())
    }

    /// 登记一条失败报告样本。
    pub fn register(&mut self, c: &ErrCase) -> Result<u64, u32> {
        if self.frozen {
            return Err(self.fail(ET_EFROZEN));
        }
        if self.duplicate(c) {
            return Err(self.fail(ET_EDUP));
        }
        match report(c) {
            Ok(r) => self.commit(c, 0, &r),
            Err(code) => Err(self.fail(code)),
        }
    }

    /// 登记并核对期望错误码；期望失败也占一格。
    pub fn register_ruled(&mut self, c: &ErrCase, want: u32) -> Result<u64, u32> {
        if self.frozen {
            return Err(self.fail(ET_EFROZEN));
        }
        if self.duplicate(c) {
            return Err(self.fail(ET_EDUP));
        }
        match report(c) {
            Ok(r) => {
                if want != 0 {
                    return Err(self.fail(ET_EMISMATCH));
                }
                self.commit(c, 0, &r)
            }
            Err(code) => {
                if want == 0 || want != code {
                    return Err(self.fail(ET_EMISMATCH));
                }
                self.commit(c, code, &Report { msg: Msg::none(), exit: 0 })
            }
        }
    }

    /// 观测一格。
    pub fn observe(&mut self, id: usize) -> Result<u64, u32> {
        if self.frozen {
            return Err(self.fail(ET_EFROZEN));
        }
        if id >= self.n {
            return Err(self.fail(ET_ENOBS));
        }
        let got = match self.slots.get(id) {
            Some(s) => *s,
            None => return Err(self.fail(ET_ENOBS)),
        };
        if got.seen {
            return Err(self.fail(ET_EDUP));
        }
        let (wc, wk, wp, wo, wl, we) = expected_of(id);
        self.observed = self.observed.wrapping_add(1);
        if let Some(slot) = self.slots.get_mut(id) {
            slot.seen = true;
        }
        let code_ok = if wc == 0 {
            got.code == 0
        } else {
            got.code == (wc.wrapping_sub(SEG_BASE)) as u8
        };
        if !code_ok
            || got.kind != wk
            || got.plen != wp
            || got.olen != wo
            || got.len != wl
            || got.exit != we
        {
            return Err(self.fail(ET_EMISMATCH));
        }
        self.matched = self.matched.wrapping_add(1);
        Ok(self.fold())
    }

    /// 已登记格数。
    pub fn len(&self) -> usize {
        self.n
    }

    /// 账本是否为空。
    pub fn is_empty(&self) -> bool {
        self.n == 0
    }

    /// 成功登记次数。
    pub fn accepted(&self) -> u32 {
        self.accepted
    }

    /// 拒绝次数。
    pub fn rejected(&self) -> u32 {
        self.rejected
    }

    /// 观测次数。
    pub fn observed(&self) -> u32 {
        self.observed
    }

    /// 观测通过次数。
    pub fn matched(&self) -> u32 {
        self.matched
    }

    /// 冻结标志。
    pub fn is_frozen(&self) -> bool {
        self.frozen
    }

    /// 记录到非零错误码的格数。
    pub fn errors(&self) -> u32 {
        let mut c = 0u32;
        let mut i = 0usize;
        while i < self.n {
            if let Some(s) = self.slots.get(i) {
                if s.code != 0 {
                    c = c.wrapping_add(1);
                }
            }
            i += 1;
        }
        c
    }

    /// 成功样本按文法计数。
    pub fn kind_counts(&self) -> [u32; KINDS] {
        let mut out = [0u32; KINDS];
        let mut i = 0usize;
        while i < self.n {
            if let Some(s) = self.slots.get(i) {
                if s.code == 0 {
                    if let Some(v) = out.get_mut(s.kind as usize) {
                        *v = v.wrapping_add(1);
                    }
                }
            }
            i += 1;
        }
        out
    }

    /// 成功样本中退出码为 127 的个数。
    pub fn all_exit(&self) -> u32 {
        let mut c = 0u32;
        let mut i = 0usize;
        while i < self.n {
            if let Some(s) = self.slots.get(i) {
                if s.code == 0 && s.exit == EXIT_LOADER {
                    c = c.wrapping_add(1);
                }
            }
            i += 1;
        }
        c
    }

    /// 成功样本的消息总长。
    pub fn total_len(&self) -> u32 {
        let mut t = 0u32;
        let mut i = 0usize;
        while i < self.n {
            if let Some(s) = self.slots.get(i) {
                if s.code == 0 {
                    t = t.wrapping_add(s.len as u32);
                }
            }
            i += 1;
        }
        t
    }

    /// 成功消息中装载类公共前缀的总出现次数。
    pub fn prefix_total(&self) -> u32 {
        let mut t = 0u32;
        let mut i = 0usize;
        while i < self.n {
            if let Some(s) = self.slots.get(i) {
                if s.code == 0 {
                    if let Ok(r) = report(&s.spec) {
                        t = t.wrapping_add(count_sub(r.msg.bytes(), LOAD_PREFIX));
                    }
                }
            }
            i += 1;
        }
        t
    }

    /// 逐格重渲染并审计。
    pub fn audit_all(&self) -> bool {
        let mut i = 0usize;
        while i < self.n {
            if let Some(s) = self.slots.get(i) {
                if s.code == 0 {
                    let r = match report(&s.spec) {
                        Ok(v) => v,
                        Err(_) => return false,
                    };
                    if !audit(&s.spec, &r)
                        || r.exit != s.exit
                        || r.msg.len != s.len
                        || s.spec.kind != s.kind
                    {
                        return false;
                    }
                }
            }
            i += 1;
        }
        true
    }

    /// 冻结核对：全登记、全观测、全匹配、文法计数、逐格审计。
    pub fn replay(&mut self) -> Result<u64, u32> {
        if self.n != SAMPLES || self.accepted != SAMPLES as u32 {
            return Err(self.fail(ET_ENOBS));
        }
        if self.observed != SAMPLES as u32 || self.matched != SAMPLES as u32 {
            return Err(self.fail(ET_EMISMATCH));
        }
        if !self.audit_all() {
            return Err(self.fail(ET_EBAL));
        }
        if self.errors() != 5
            || self.kind_counts() != [2, 1, 1, 1, 1, 1]
            || self.all_exit() != 7
            || self.prefix_total() != 4
            || self.total_len() == 0
        {
            return Err(self.fail(ET_EBAL));
        }
        Ok(self.fold())
    }

    /// 冻结：先重放再上锁。
    pub fn freeze(&mut self) -> Result<u64, u32> {
        if self.frozen {
            return Err(self.fail(ET_EFROZEN));
        }
        self.replay()?;
        self.frozen = true;
        Ok(self.fold())
    }

    /// 对全部字段做 FNV-1a 折叠。
    pub fn fold(&self) -> u64 {
        let mut h = FNV_OFF;
        h = mix(h, self.n as u64);
        h = mix(h, self.accepted as u64);
        h = mix(h, self.rejected as u64);
        h = mix(h, self.observed as u64);
        h = mix(h, self.matched as u64);
        h = mix(h, if self.frozen { 1 } else { 0 });
        let mut i = 0usize;
        while i < SAMPLES {
            if let Some(s) = self.slots.get(i) {
                h = mix(h, s.code as u64);
                h = mix(h, s.kind as u64);
                h = mix(h, s.plen as u64);
                h = mix(h, s.olen as u64);
                h = mix(h, s.exit as u64);
                h = mix(h, s.len as u64);
                h = mix(h, if s.seen { 1 } else { 0 });
                h = mix(h, s.spec.kind as u64);
                h = mix(h, s.spec.plen as u64);
                h = mix(h, s.spec.olen as u64);
                let mut k = 0usize;
                while k < PROG_MAX {
                    if let Some(v) = s.spec.prog.get(k) {
                        h = mix(h, *v as u64);
                    }
                    k += 1;
                }
                let mut o = 0usize;
                while o < OBJ_MAX {
                    if let Some(v) = s.spec.obj.get(o) {
                        h = mix(h, *v as u64);
                    }
                    o += 1;
                }
            }
            i += 1;
        }
        h
    }
}

fn mix(mut h: u64, v: u64) -> u64 {
    let mut i = 0u32;
    while i < 8 {
        h ^= (v >> (i * 8)) & 0xff;
        h = h.wrapping_mul(FNV_PRIME);
        i += 1;
    }
    h
}

/// 构建基准账本。
pub fn build_ledger() -> Result<ErrLedger, u32> {
    let mut l = ErrLedger::new();
    let mut i = 0usize;
    while i < SAMPLES {
        let (want, _, _, _, _, _) = expected_of(i);
        l.register_ruled(&spec_of(i), want)?;
        i += 1;
    }
    let mut j = 0usize;
    while j < SAMPLES {
        l.observe(j)?;
        j += 1;
    }
    Ok(l)
}

/// 四路独立注入：越序、重复、越界、冻结。
pub fn inject_four() -> [bool; 4] {
    let mut ok = [false; 4];
    let c0 = spec_of(0);

    let mut a = ErrLedger::new();
    match a.observe(0) {
        Err(code) if code == ET_ENOBS => ok[0] = a.rejected() == 1 && a.len() == 0,
        _ => {}
    }

    let mut b = ErrLedger::new();
    let _ = b.register(&c0);
    match b.register(&c0) {
        Err(code) if code == ET_EDUP => ok[1] = b.rejected() == 1 && b.len() == 1,
        _ => {}
    }

    let mut d = ErrLedger::new();
    let mut i = 0usize;
    while i < SAMPLES {
        let (want, _, _, _, _, _) = expected_of(i);
        let _ = d.register_ruled(&spec_of(i), want);
        i += 1;
    }
    let extra = make_case(K_MISSING, "extra", "libz.so");
    match d.register(&extra) {
        Err(code) if code == ET_EID => ok[2] = d.rejected() == 1 && d.len() == SAMPLES,
        _ => {}
    }

    if let Ok(mut l) = build_ledger() {
        if l.freeze().is_ok() {
            match l.register(&c0) {
                Err(code) if code == ET_EFROZEN => ok[3] = l.is_frozen() && l.rejected() == 1,
                _ => {}
            }
        }
    }
    ok
}

/// 十三项自检。
pub fn run_unx11256_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F11256");

    let fresh = ErrLedger::new();
    s.add(
        "ET1-建场静默",
        fresh.len() == 0
            && fresh.is_empty()
            && fresh.rejected() == 0
            && fresh.observed() == 0
            && !fresh.is_frozen(),
        "新账本零格零拒绝",
    );

    let exact = b"demo: error while loading shared libraries: libfoo.so: cannot open shared object file: No such file or directory";
    let e2 = match report(&spec_of(0)) {
        Ok(r) => r.exit == EXIT_LOADER && r.msg.bytes() == exact,
        Err(_) => false,
    };
    s.add("ET2-缺失对象原文", e2, "整句与glibc原文逐字一致");

    let e3 = match report(&spec_of(1)) {
        Ok(r) => {
            audit(&spec_of(1), &r)
                && ends_with(r.msg.bytes(), PERM)
                && count_sub(r.msg.bytes(), LOAD_PREFIX) == 1
        }
        Err(_) => false,
    };
    s.add("ET3-权限原因后缀", e3, "装载前缀唯一且以Permission denied结尾");

    let e4 = match report(&spec_of(2)) {
        Ok(r) => ends_with(r.msg.bytes(), WRONG) && audit(&spec_of(2), &r),
        Err(_) => false,
    };
    s.add("ET4-ELF类别后缀", e4, "wrong ELF class: ELFCLASS64逐字出现");

    let e5 = match report(&spec_of(3)) {
        Ok(r) => r.msg.bytes() == b"demo: undefined symbol: init_mod",
        Err(_) => false,
    };
    s.add("ET5-未定义符号文法", e5, "符号期报告不套装载前缀");

    let e6 = match (report(&spec_of(4)), report(&spec_of(5))) {
        (Ok(a), Ok(b)) => {
            starts_with(a.msg.bytes(), b"demo: symbol lookup error: ")
                && count_sub(a.msg.bytes(), UNDEF_PREFIX) >= 1
                && count_sub(b.msg.bytes(), RELOC_PREFIX) == 1
        }
        _ => false,
    };
    s.add("ET6-查找与重定位文法", e6, "两条符号期文法各自独立");

    let mut e7 = true;
    let mut i = 0usize;
    while i <= 6 {
        let c = spec_of(i);
        match report(&c) {
            Ok(r) => {
                if !audit(&c, &r) || r.msg.len != expected_of(i).4 {
                    e7 = false;
                }
            }
            Err(_) => e7 = false,
        }
        i += 1;
    }
    s.add("ET7-长度逐字节审计", e7, "三段长度之和等于缓冲区长度");

    let e8 = report(&spec_of(7)) == Err(ET_EMISSING)
        && report(&spec_of(9)) == Err(ET_EMISSING)
        && report(&spec_of(11)) == Err(ET_EMISSING);
    s.add("ET8-空字段拒绝", e8, "程序名或对象名为空即拒绝");

    s.add(
        "ET9-未知文法拒绝",
        report(&spec_of(8)) == Err(ET_EKIND),
        "文法编号超出KINDS",
    );
    s.add(
        "ET10-冒号歧义拒绝",
        report(&spec_of(10)) == Err(ET_EBADCHAR),
        "装载类对象名含冒号无法无歧义拼接",
    );

    let built = build_ledger();
    let (e11, e12) = match &built {
        Ok(l) => {
            let r = l.len() == SAMPLES
                && l.accepted() == SAMPLES as u32
                && l.rejected() == 0
                && l.errors() == 5
                && l.kind_counts() == [2, 1, 1, 1, 1, 1]
                && l.all_exit() == 7
                && l.prefix_total() == 4
                && l.audit_all()
                && l.total_len() > 0;
            let mut m = match build_ledger() {
                Ok(v) => v,
                Err(_) => ErrLedger::new(),
            };
            let f = match m.freeze() {
                Ok(_) => m.is_frozen() && m.freeze() == Err(ET_EFROZEN) && m.rejected() == 1,
                Err(_) => false,
            };
            (r, f)
        }
        Err(_) => (false, false),
    };
    s.add("ET11-文法计数与退出码", e11, "装载类四句前缀 全部退出码127");
    s.add("ET12-冻结后二次拒绝", e12, "冻结成功且再次冻结返回错误");

    s.add(
        "ET13-四路注入",
        inject_four() == [true, true, true, true],
        "越序 重复 越界 冻结四条拒绝路径均触发",
    );

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exact_missing_line() {
        let r = match report(&spec_of(0)) {
            Ok(v) => v,
            Err(_) => Report { msg: Msg::none(), exit: 0 },
        };
        let want = b"demo: error while loading shared libraries: libfoo.so: cannot open shared object file: No such file or directory";
        assert_eq!(r.msg.bytes(), want);
        assert_eq!(r.exit, 127);
        assert!(audit(&spec_of(0), &r));
    }

    #[test]
    fn perms_and_class_suffixes() {
        let a = match report(&spec_of(1)) {
            Ok(v) => v,
            Err(_) => Report { msg: Msg::none(), exit: 0 },
        };
        assert!(ends_with(a.msg.bytes(), PERM));
        let b = match report(&spec_of(2)) {
            Ok(v) => v,
            Err(_) => Report { msg: Msg::none(), exit: 0 },
        };
        assert!(ends_with(b.msg.bytes(), WRONG));
        assert!(audit(&spec_of(2), &b));
    }

    #[test]
    fn symbol_error_families() {
        let a = match report(&spec_of(3)) {
            Ok(v) => v,
            Err(_) => Report { msg: Msg::none(), exit: 0 },
        };
        assert_eq!(a.msg.bytes(), b"demo: undefined symbol: init_mod");
        let b = match report(&spec_of(4)) {
            Ok(v) => v,
            Err(_) => Report { msg: Msg::none(), exit: 0 },
        };
        assert!(starts_with(b.msg.bytes(), b"demo: symbol lookup error: "));
        let c = match report(&spec_of(5)) {
            Ok(v) => v,
            Err(_) => Report { msg: Msg::none(), exit: 0 },
        };
        assert_eq!(count_sub(c.msg.bytes(), RELOC_PREFIX), 1);
    }

    #[test]
    fn audit_recomputes_lengths() {
        let c = spec_of(6);
        let r = match report(&c) {
            Ok(v) => v,
            Err(_) => Report { msg: Msg::none(), exit: 0 },
        };
        assert!(audit(&c, &r));
        assert_eq!(
            r.msg.len as usize,
            c.plen as usize + c.olen as usize + lit_len(c.kind)
        );
        let mut bad = r;
        bad.msg.len = bad.msg.len.wrapping_add(1);
        assert!(!audit(&c, &bad));
        let mut wrong_exit = r;
        wrong_exit.exit = 1;
        assert!(!audit(&c, &wrong_exit));
    }

    #[test]
    fn refusal_codes() {
        assert_eq!(report(&spec_of(7)), Err(ET_EMISSING));
        assert_eq!(report(&spec_of(8)), Err(ET_EKIND));
        assert_eq!(report(&spec_of(9)), Err(ET_EMISSING));
        assert_eq!(report(&spec_of(10)), Err(ET_EBADCHAR));
        assert_eq!(report(&spec_of(11)), Err(ET_EMISSING));
        let mut nl = make_case(K_UNDEF, "demo", "bad\nsym");
        nl.olen = 7;
        assert_eq!(report(&nl), Err(ET_EBADCHAR));
    }

    #[test]
    fn ledger_counts_and_freeze() {
        let mut l = match build_ledger() {
            Ok(v) => v,
            Err(_) => ErrLedger::new(),
        };
        assert_eq!(l.len(), SAMPLES);
        assert_eq!(l.errors(), 5);
        assert_eq!(l.kind_counts(), [2, 1, 1, 1, 1, 1]);
        assert_eq!(l.all_exit(), 7);
        assert_eq!(l.prefix_total(), 4);
        assert!(l.audit_all());
        assert_eq!(inject_four(), [true, true, true, true]);
        let h = match l.freeze() {
            Ok(v) => v,
            Err(_) => 0,
        };
        assert_eq!(l.replay(), Ok(h));
        assert_eq!(l.freeze(), Err(ET_EFROZEN));
        assert_eq!(l.register(&spec_of(0)), Err(ET_EFROZEN));
    }
}
