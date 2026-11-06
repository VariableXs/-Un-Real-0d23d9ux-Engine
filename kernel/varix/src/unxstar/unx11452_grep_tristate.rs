//! UNX-F11452 `grep` 退出码三态（0x20c00 段）
//!
//! grep 的退出码不是布尔值而是三态：0 表示至少选中一行，1 表示一行都没选中，
//! 2 表示出了错。把 2 折叠进 1 是最常见的实现错误——脚本里 `grep -q pat file`
//! 一旦把“读失败”当成“没匹配”，`if grep -q ...; then` 会安静地走 else 分支，
//! 故障从此隐身。本模型把三态做成显式常量，任何派生路径都只能落到这三个值。
//!
//! 选行语义：模式先反转义（`\x` 表示字面 x），再按行做朴素子串匹配；`-v` 反转
//! 选择，`-c` 只关心行数，`-q` 命中第一行就返回。错误闸门有固定次序：未知开关
//! （EFLAG）→ 文本超限（ETEXT）→ 模式超限（ELONG）→ 空模式（EEMPTY）→ 字符类
//! 括号失配（EPAT）。次序固定是为了让两个错误同时成立时答案唯一，审计才能钉值。
//!
//! 故障与命中的优先级是另一条不变量：读取中途失败时，即使前面已经打印过命中行，
//! 退出码仍是 2——错误压过命中。唯一的例外是 `-q`：它在第一处命中就退出，根本
//! 读不到后面的故障，因此退出码是 0。这两条放在同一组判据里对照，防止有人把
//! “故障优先”实现成无条件覆盖。
//!
//! 账本十四格：八格成功（三态各一、命中计数、反转计数、静默早退、故障压制、
//! 计数模式），六格拒绝（未知开关、模式超限、括号失配、文本超限、容量满、重复
//! 登记）。审计硬编码八格的值与六格的错误码，按行序逐格核对。

use crate::alloc::format;
use crate::checks::CheckSet;

/// 段基址。
pub const SEG_BASE: u32 = 0x20c00;

/// 段内低十六位（错误码只用低半区，掩码后转换不会截断）。
const SEG_LO: u16 = (SEG_BASE & 0xffff) as u16;

/// 登记顺序错误。
pub const GT_EORDER: u16 = SEG_LO + 0x01;
/// 重复登记。
pub const GT_EDUP: u16 = SEG_LO + 0x02;
/// 已冻结。
pub const GT_EFROZEN: u16 = SEG_LO + 0x03;
/// 观测缺失。
pub const GT_ENOBS: u16 = SEG_LO + 0x04;
/// 结论不符。
pub const GT_EMISMATCH: u16 = SEG_LO + 0x05;
/// 账本失衡。
pub const GT_EBAL: u16 = SEG_LO + 0x06;
/// 容量满。
pub const GT_EFULL: u16 = SEG_LO + 0x07;
/// 空模式。
pub const GT_EEMPTY: u16 = SEG_LO + 0x08;
/// 未知开关。
pub const GT_EFLAG: u16 = SEG_LO + 0x09;
/// 字符类括号失配。
pub const GT_EPAT: u16 = SEG_LO + 0x0a;
/// 模式超限。
pub const GT_ELONG: u16 = SEG_LO + 0x0b;
/// 文本超限。
pub const GT_ETEXT: u16 = SEG_LO + 0x0c;
/// 票号不一致。
pub const GT_ESEAL: u16 = SEG_LO + 0x0d;

/// 退出码：命中。
pub const EX_MATCH: u8 = 0;
/// 退出码：未命中。
pub const EX_NOMATCH: u8 = 1;
/// 退出码：故障。
pub const EX_FAULT: u8 = 2;

/// 开关：反转选择。
pub const F_INVERT: u8 = 0x01;
/// 开关：只数行数。
pub const F_COUNT: u8 = 0x02;
/// 开关：静默，首处命中即退。
pub const F_QUIET: u8 = 0x04;
/// 开关：带行号。
pub const F_LINE_NO: u8 = 0x08;
/// 合法开关掩码。
pub const F_MASK: u8 = 0x0f;

/// 操作：取退出码。
pub const OP_EXIT: u8 = 1;
/// 操作：取命中行数。
pub const OP_HITS: u8 = 2;
/// 操作：账本自检。
pub const OP_LEDGER: u8 = 3;

/// 模式容量。
pub const PAT_CAP: usize = 16;
/// 文本容量。
pub const TEXT_CAP: usize = 128;
/// 账本格数。
pub const ROWS: usize = 14;

/// 审计用样本语料，五行。
pub const CORPUS: &[u8] = b"alpha\nbeta\ngamma\nbeta gamma\ndelta\n";

/// FNV 偏移基。
pub const FNV_OFF: u64 = 0xcbf2_9ce4_8422_2325;
/// FNV 质数。
pub const FNV_PRIME: u64 = 0x100_0000_01b3;

/// 折入一个 64 位字（小端逐字节）。
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

/// 反转义：`\x` 折叠为字面 x，反斜杠收尾视为括号失配。
fn unescape(pat: &[u8], dst: &mut [u8; PAT_CAP]) -> Result<usize, u16> {
    if pat.len() > PAT_CAP {
        return Err(GT_ELONG);
    }
    let mut i = 0usize;
    let mut w = 0usize;
    while i < pat.len() {
        let b = match pat.get(i) {
            Some(v) => *v,
            None => return Err(GT_EPAT),
        };
        if b == b'\\' {
            let next = match pat.get(i + 1) {
                Some(v) => *v,
                None => return Err(GT_EPAT),
            };
            if let Some(slot) = dst.get_mut(w) {
                *slot = next;
            }
            w += 1;
            i += 2;
        } else {
            if let Some(slot) = dst.get_mut(w) {
                *slot = b;
            }
            w += 1;
            i += 1;
        }
    }
    Ok(w)
}

/// 字符类括号配平：未转义的 `[` 必须成对，且不允许嵌套。
fn brackets_ok(pat: &[u8]) -> bool {
    let mut i = 0usize;
    let mut depth = 0u8;
    while i < pat.len() {
        let b = match pat.get(i) {
            Some(v) => *v,
            None => return false,
        };
        if b == b'\\' {
            i += 2;
            continue;
        }
        if b == b'[' {
            if depth != 0 {
                return false;
            }
            depth = 1;
        } else if b == b']' {
            if depth == 0 {
                return false;
            }
            depth = 0;
        }
        i += 1;
    }
    depth == 0
}

/// 朴素子串匹配。
fn match_lit(lit: &[u8], line: &[u8]) -> bool {
    if lit.len() > line.len() {
        return false;
    }
    let mut i = 0usize;
    while i + lit.len() <= line.len() {
        let mut j = 0usize;
        let mut hit = true;
        while j < lit.len() {
            match (line.get(i + j), lit.get(j)) {
                (Some(a), Some(b)) => {
                    if a != b {
                        hit = false;
                        break;
                    }
                }
                _ => {
                    hit = false;
                    break;
                }
            }
            j += 1;
        }
        if hit {
            return true;
        }
        i += 1;
    }
    false
}

/// 扫描：返回（选中行数，首个选中行的 1 基行号）。
pub fn scan(pat: &[u8], text: &[u8], flags: u8) -> Result<(u32, u32), u16> {
    if flags & !F_MASK != 0 {
        return Err(GT_EFLAG);
    }
    if text.len() > TEXT_CAP {
        return Err(GT_ETEXT);
    }
    if pat.len() > PAT_CAP {
        return Err(GT_ELONG);
    }
    let mut lit = [0u8; PAT_CAP];
    let n = unescape(pat, &mut lit)?;
    if n == 0 {
        return Err(GT_EEMPTY);
    }
    if !brackets_ok(pat) {
        return Err(GT_EPAT);
    }
    let invert = flags & F_INVERT != 0;
    let quiet = flags & F_QUIET != 0;
    let mut hits = 0u32;
    let mut idx = 0u32;
    let mut first = 0u32;
    let mut start = 0usize;
    while start < text.len() {
        let mut end = start;
        while end < text.len() && text.get(end) != Some(&b'\n') {
            end += 1;
        }
        let line = match text.get(start..end) {
            Some(v) => v,
            None => return Err(GT_EBAL),
        };
        let hit = match_lit(&lit[..n], line);
        let selected = if invert { !hit } else { hit };
        if selected {
            hits += 1;
            if hits == 1 {
                first = idx + 1;
            }
            if quiet {
                return Ok((hits, first));
            }
        }
        idx += 1;
        if end >= text.len() {
            break;
        }
        start = end + 1;
    }
    Ok((hits, first))
}

/// 取前 k 行的字节前缀。
pub fn prefix_lines(text: &[u8], k: u32) -> &[u8] {
    if k == 0 || text.len() > TEXT_CAP {
        return b"";
    }
    let mut seen = 0u32;
    let mut i = 0usize;
    while i < text.len() {
        if text.get(i) == Some(&b'\n') {
            seen += 1;
            if seen == k {
                return match text.get(..i + 1) {
                    Some(v) => v,
                    None => b"",
                };
            }
        }
        i += 1;
    }
    text
}

/// 结果折算退出码：错误恒为 2。
pub fn exit_of(r: Result<(u32, u32), u16>) -> u8 {
    match r {
        Err(_) => EX_FAULT,
        Ok((hits, _)) => {
            if hits > 0 {
                EX_MATCH
            } else {
                EX_NOMATCH
            }
        }
    }
}

/// 带读取故障的退出码：故障压过命中，但 `-q` 早退压过故障。
pub fn exit_with_fault(pat: &[u8], text: &[u8], flags: u8, fault_line: u32) -> Result<u8, u16> {
    if flags & !F_MASK != 0 {
        return Err(GT_EFLAG);
    }
    if fault_line == 0 {
        let whole = scan(pat, text, flags);
        return Ok(exit_of(whole));
    }
    let pre = prefix_lines(text, fault_line.saturating_sub(1));
    let (hits, _) = scan(pat, pre, flags)?;
    if flags & F_QUIET != 0 && hits > 0 {
        return Ok(EX_MATCH);
    }
    Ok(EX_FAULT)
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

    /// 指定容量（不超过格数）。
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
            return Err(GT_EFROZEN);
        }
        let mut i = 0usize;
        while i < self.len {
            match self.rows.get(i) {
                Some(x) => {
                    if x.slot == s.slot {
                        self.rejects += 1;
                        return Err(GT_EDUP);
                    }
                }
                None => return Err(GT_ENOBS),
            }
            i += 1;
        }
        if s.slot as usize != self.len {
            self.rejects += 1;
            return Err(GT_EORDER);
        }
        if self.len >= self.cap || self.len >= ROWS {
            self.rejects += 1;
            return Err(GT_EFULL);
        }
        if s.op == 0 {
            self.rejects += 1;
            return Err(GT_EMISMATCH);
        }
        if s.ok && s.code != 0 {
            self.rejects += 1;
            return Err(GT_EMISMATCH);
        }
        if !s.ok && s.code == 0 {
            self.rejects += 1;
            return Err(GT_EMISMATCH);
        }
        match self.rows.get_mut(self.len) {
            Some(x) => *x = s,
            None => return Err(GT_EFULL),
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

    /// 对账：八格成功逐格钉值，六格拒绝逐格钉码。
    pub fn audit(&self) -> Result<AuditReport, u16> {
        if self.len != ROWS {
            return Err(GT_EBAL);
        }
        if self.ok_count() != 8 || self.err_count() != 6 {
            return Err(GT_EBAL);
        }
        let mut i = 0usize;
        while i < self.len {
            let s = match self.rows.get(i) {
                Some(v) => *v,
                None => return Err(GT_ENOBS),
            };
            if s.slot as usize != i {
                return Err(GT_EORDER);
            }
            if s.op != OP_EXIT && s.op != OP_HITS && s.op != OP_LEDGER {
                return Err(GT_EMISMATCH);
            }
            if s.ok {
                if s.code != 0 {
                    return Err(GT_EMISMATCH);
                }
            } else if s.code == 0 || s.value != 0 {
                return Err(GT_EMISMATCH);
            }
            i += 1;
        }
        let vals = [0u32, 1, 2, 2, 3, 0, 2, 5];
        let mut k = 0usize;
        while k < vals.len() {
            match (self.sample(k), vals.get(k)) {
                (Some(s), Some(w)) => {
                    if !s.ok || s.value != *w {
                        return Err(GT_EMISMATCH);
                    }
                }
                _ => return Err(GT_EBAL),
            }
            k += 1;
        }
        let codes = [
            GT_EFLAG, GT_ELONG, GT_EPAT, GT_ETEXT, GT_EFULL, GT_EDUP,
        ];
        let mut c = 0usize;
        let mut j = 8usize;
        while j < self.len {
            match self.rows.get(j) {
                Some(s) => match codes.get(c) {
                    Some(w) => {
                        if s.ok || s.code != *w || s.value != 0 {
                            return Err(GT_EMISMATCH);
                        }
                    }
                    None => return Err(GT_EBAL),
                },
                None => return Err(GT_ENOBS),
            }
            c += 1;
            j += 1;
        }
        if c != codes.len() {
            return Err(GT_EBAL);
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
            return Err(GT_EFROZEN);
        }
        let s = self.seal();
        self.root = s;
        self.frozen = true;
        Ok(s)
    }

    /// 重放：重建账本比票号。
    pub fn replay(&self) -> Result<u64, u16> {
        let fresh = build_ledger();
        if fresh.len != self.len {
            return Err(GT_EBAL);
        }
        let h = fresh.seal();
        if self.frozen && h != self.root {
            return Err(GT_ESEAL);
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

    /// 二次冻结或越闸次数。
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

/// 取登记错误码（成功则 0，供审计暴露异常）。
fn reg_code(r: Result<(), u16>) -> u16 {
    match r {
        Ok(()) => 0,
        Err(c) => c,
    }
}

/// 构建十四格账本。
pub fn build_ledger() -> Ledger {
    let mut l = Ledger::new();

    let m = scan(b"beta", CORPUS, 0);
    let nm = scan(b"zz", CORPUS, 0);
    let e = scan(b"", CORPUS, 0);
    let _ = l.register(ok_row(0, OP_EXIT, u32::from(exit_of(m))));
    let _ = l.register(ok_row(1, OP_EXIT, u32::from(exit_of(nm))));
    let _ = l.register(ok_row(2, OP_EXIT, u32::from(exit_of(e))));

    let hits = match scan(b"beta", CORPUS, 0) {
        Ok((h, _)) => h,
        Err(_) => 0,
    };
    let _ = l.register(ok_row(3, OP_HITS, hits));

    let inv = match scan(b"beta", CORPUS, F_INVERT) {
        Ok((h, _)) => h,
        Err(_) => 0,
    };
    let _ = l.register(ok_row(4, OP_HITS, inv));

    let quiet = match exit_with_fault(b"beta", CORPUS, F_QUIET, 4) {
        Ok(v) => u32::from(v),
        Err(_) => 0xffff_ffff,
    };
    let _ = l.register(ok_row(5, OP_EXIT, quiet));

    let fault = match exit_with_fault(b"beta", CORPUS, 0, 4) {
        Ok(v) => u32::from(v),
        Err(_) => 0xffff_ffff,
    };
    let _ = l.register(ok_row(6, OP_EXIT, fault));

    let counted = match scan(b"a", CORPUS, F_COUNT) {
        Ok((h, _)) => h,
        Err(_) => 0,
    };
    let _ = l.register(ok_row(7, OP_HITS, counted));

    let flag = match scan(b"beta", CORPUS, 0x10) {
        Ok(_) => 0,
        Err(c) => c,
    };
    let _ = l.register(err_row(8, OP_EXIT, flag));

    let long_pat = [b'a'; PAT_CAP + 4];
    let long = match scan(&long_pat, CORPUS, 0) {
        Ok(_) => 0,
        Err(c) => c,
    };
    let _ = l.register(err_row(9, OP_EXIT, long));

    let bracket = match scan(b"[ab", CORPUS, 0) {
        Ok(_) => 0,
        Err(c) => c,
    };
    let _ = l.register(err_row(10, OP_EXIT, bracket));

    let long_text = [b'x'; TEXT_CAP + 1];
    let text = match scan(b"a", &long_text, 0) {
        Ok(_) => 0,
        Err(c) => c,
    };
    let _ = l.register(err_row(11, OP_EXIT, text));

    let mut small = Ledger::with_cap(2);
    let _ = small.register(ok_row(0, OP_HITS, 1));
    let _ = small.register(ok_row(1, OP_HITS, 2));
    let full = reg_code(small.register(ok_row(2, OP_HITS, 3)));
    let _ = l.register(err_row(12, OP_LEDGER, full));

    let mut d = Ledger::new();
    let _ = d.register(ok_row(0, OP_HITS, 1));
    let dup = reg_code(d.register(ok_row(0, OP_HITS, 9)));
    let _ = l.register(err_row(13, OP_LEDGER, dup));

    l
}

/// 四路注入：重放冻结、三态覆盖、故障优先级、篡改复原。
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
        && two == Err(GT_EFROZEN)
        && l.rejects() == 1
        && before == l.len()
        && root != 0
        && l.audit().is_ok();

    out[1] = exit_of(scan(b"beta", CORPUS, 0)) == EX_MATCH
        && exit_of(scan(b"zz", CORPUS, 0)) == EX_NOMATCH
        && exit_of(scan(b"[", CORPUS, 0)) == EX_FAULT;

    out[2] = exit_with_fault(b"beta", CORPUS, F_QUIET, 4) == Ok(EX_MATCH)
        && exit_with_fault(b"beta", CORPUS, 0, 4) == Ok(EX_FAULT)
        && exit_with_fault(b"zz", CORPUS, F_QUIET, 4) == Ok(EX_FAULT);

    out[3] = {
        let mut t = build_ledger();
        let clean = t.audit().is_ok();
        let old = match t.sample(3) {
            Some(s) => s.value,
            None => 0,
        };
        if let Some(r) = t.rows.get_mut(3) {
            r.value = old.wrapping_add(1);
        }
        let caught = t.audit() == Err(GT_EMISMATCH);
        if let Some(r) = t.rows.get_mut(3) {
            r.value = old;
        }
        clean && caught && t.audit().is_ok() && t.len() == ROWS
    };

    out
}

/// 十三判据。
pub fn run_unx11452_checks() -> CheckSet {
    let mut s = CheckSet::new("unx11452.greptristate");
    let l = build_ledger();

    s.add_dyn(
        "GT1-十四格齐备",
        l.len() == ROWS && l.ok_count() == 8 && l.err_count() == 6 && l.audit().is_ok(),
        &format!("n={} ok={} err={}", l.len(), l.ok_count(), l.err_count()),
    );

    s.add(
        "GT2-三态互斥",
        l.sample(0).map(|x| x.value) == Some(0)
            && l.sample(1).map(|x| x.value) == Some(1)
            && l.sample(2).map(|x| x.value) == Some(2),
        "static",
    );

    s.add(
        "GT3-命中行数与首行号",
        l.sample(3).map(|x| x.value) == Some(2)
            && scan(b"beta", CORPUS, 0) == Ok((2, 2)),
        "static",
    );

    s.add(
        "GT4-反转计数",
        l.sample(4).map(|x| x.value) == Some(3)
            && scan(b"beta", CORPUS, F_INVERT) == Ok((3, 1)),
        "static",
    );

    s.add(
        "GT5-故障压过命中",
        l.sample(5).map(|x| x.value) == Some(0)
            && l.sample(6).map(|x| x.value) == Some(2)
            && exit_with_fault(b"beta", CORPUS, 0, 2) == Ok(EX_FAULT),
        "static",
    );

    s.add(
        "GT6-空模式即错",
        l.sample(2).map(|x| x.value) == Some(2)
            && scan(b"", CORPUS, 0) == Err(GT_EEMPTY)
            && exit_of(scan(b"", CORPUS, 0)) == EX_FAULT,
        "static",
    );

    s.add(
        "GT7-未知开关闸门",
        l.sample(8).map(|x| x.code) == Some(GT_EFLAG)
            && scan(b"beta", CORPUS, 0x10) == Err(GT_EFLAG)
            && scan(b"beta", CORPUS, F_MASK) != Err(GT_EFLAG),
        "static",
    );

    s.add(
        "GT8-模式超限闸门",
        l.sample(9).map(|x| x.code) == Some(GT_ELONG)
            && {
                let p = [b'a'; PAT_CAP + 1];
                scan(&p, CORPUS, 0) == Err(GT_ELONG)
                    && scan(&[b'a'; PAT_CAP], CORPUS, 0).is_ok()
            },
        "static",
    );

    s.add(
        "GT9-括号失配闸门",
        l.sample(10).map(|x| x.code) == Some(GT_EPAT)
            && scan(b"[ab", CORPUS, 0) == Err(GT_EPAT)
            && scan(b"a]b", CORPUS, 0) == Err(GT_EPAT)
            && scan(b"[ab]", CORPUS, 0).is_ok(),
        "static",
    );

    s.add(
        "GT10-文本超限闸门",
        l.sample(11).map(|x| x.code) == Some(GT_ETEXT)
            && {
                let t = [b'x'; TEXT_CAP + 1];
                scan(b"a", &t, 0) == Err(GT_ETEXT)
                    && scan(b"a", &[b'x'; TEXT_CAP], 0).is_ok()
            },
        "static",
    );

    s.add(
        "GT11-容量闸门",
        l.sample(12).map(|x| x.code) == Some(GT_EFULL)
            && {
                let mut small = Ledger::with_cap(2);
                let _ = small.register(ok_row(0, OP_HITS, 1));
                let _ = small.register(ok_row(1, OP_HITS, 2));
                small.register(ok_row(2, OP_HITS, 3)) == Err(GT_EFULL) && small.len() == 2
            },
        "static",
    );

    s.add(
        "GT12-重复登记闸门",
        l.sample(13).map(|x| x.code) == Some(GT_EDUP)
            && {
                let mut d = Ledger::new();
                let _ = d.register(ok_row(0, OP_HITS, 1));
                d.register(ok_row(0, OP_HITS, 9)) == Err(GT_EDUP)
                    && d.register(ok_row(2, OP_HITS, 9)) == Err(GT_EORDER)
                    && d.len() == 1
            },
        "static",
    );

    s.add(
        "GT13-冻结重放与四注入",
        {
            let mut f = build_ledger();
            let rep = f.replay();
            let one = f.freeze();
            let two = f.freeze();
            let inj = inject_four();
            one.is_ok()
                && one == rep
                && two == Err(GT_EFROZEN)
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
    fn tri_state_codes() {
        assert_eq!(exit_of(scan(b"beta", CORPUS, 0)), EX_MATCH);
        assert_eq!(exit_of(scan(b"zz", CORPUS, 0)), EX_NOMATCH);
        assert_eq!(exit_of(scan(b"", CORPUS, 0)), EX_FAULT);
        assert_eq!(EX_MATCH, 0);
        assert_eq!(EX_NOMATCH, 1);
        assert_eq!(EX_FAULT, 2);
    }

    #[test]
    fn counts_and_invert() {
        assert_eq!(scan(b"beta", CORPUS, 0), Ok((2, 2)));
        assert_eq!(scan(b"beta", CORPUS, F_INVERT), Ok((3, 1)));
        assert_eq!(scan(b"a", CORPUS, F_COUNT), Ok((5, 1)));
        assert_eq!(scan(b"zz", CORPUS, F_INVERT), Ok((5, 1)));
    }

    #[test]
    fn fault_dominates_match() {
        assert_eq!(exit_with_fault(b"beta", CORPUS, 0, 4), Ok(EX_FAULT));
        assert_eq!(exit_with_fault(b"beta", CORPUS, F_QUIET, 4), Ok(EX_MATCH));
        assert_eq!(exit_with_fault(b"beta", CORPUS, F_QUIET, 3), Ok(EX_MATCH));
        assert_eq!(exit_with_fault(b"beta", CORPUS, F_QUIET, 2), Ok(EX_FAULT));
        assert_eq!(exit_with_fault(b"zz", CORPUS, F_QUIET, 2), Ok(EX_FAULT));
    }

    #[test]
    fn pattern_gates_are_ordered() {
        assert_eq!(scan(b"", CORPUS, 0x10), Err(GT_EFLAG));
        assert_eq!(scan(b"", CORPUS, 0), Err(GT_EEMPTY));
        assert_eq!(scan(b"[", CORPUS, 0), Err(GT_EPAT));
        assert_eq!(scan(b"\\", CORPUS, 0), Err(GT_EPAT));
        let p = [b'a'; PAT_CAP + 1];
        assert_eq!(scan(&p, CORPUS, 0), Err(GT_ELONG));
    }

    #[test]
    fn escape_folds_to_literal() {
        assert_eq!(scan(b"\\[ab", CORPUS, 0), Ok((0, 0)));
        assert_eq!(scan(b"beta gamma", CORPUS, 0), Ok((1, 4)));
        assert_eq!(scan(b"\\", b"", 0), Err(GT_EPAT));
    }

    #[test]
    fn ledger_shape() {
        let l = build_ledger();
        assert_eq!(l.len(), ROWS);
        assert_eq!(l.ok_count(), 8);
        assert_eq!(l.err_count(), 6);
        assert!(l.audit().is_ok());
        assert_eq!(l.rejects(), 0);
    }

    #[test]
    fn freeze_then_replay_matches() {
        let mut l = build_ledger();
        let root = l.seal();
        assert_eq!(l.freeze(), Ok(root));
        assert_eq!(l.replay(), Ok(root));
        assert_eq!(l.freeze(), Err(GT_EFROZEN));
        assert!(l.is_frozen());
        assert_eq!(l.root(), root);
    }

    #[test]
    fn checks_pass() {
        let s = run_unx11452_checks();
        assert_eq!(s.len(), 13);
        assert!(s.all_passed(), "{}", s.red_items_text());
        assert!(!s.truncated());
    }
}
