//! UNX-F11458 两工具判据入册登记（语料清单挂接）（0x20c60 段）
//!
//! 判据不是凭空的：每一条都必须挂在一份具体语料上，并说明这份语料对哪个工具、
//! 哪一类内容、能提供几个判别见证。语料清单（catalog）因此是判据的**前置账**：
//! 没有入册的语料不许当证据用，入册的语料必须能被两条工具线同时引用。清单里
//! 的编号严格递增——这既保证追加式登记（不回头改历史），也让“重复编号”只可能
//! 出现在表尾：拿旧编号来登记会被判成逆序，只有拿最后一条再来一次才是重复。
//!
//! 每条记录的闸门次序：工具（ETOOL）→ 内容类型（EKIND）→ 判别见证数（ENOCUE）
//! → 单件字节上限（EBIG）→ 编号（重复 EDUP / 逆序 EORDER）→ 容量（EFULL）。
//! 见证数为零的语料不是“弱证据”而是**没有证据**，必须拒绝；字节上限取 1 MiB，
//! 超过的语料要走分片而不能整件入册。
//!
//! 汇总侧只暴露四个只读投影：按工具计数、按类型计数、总字节（`checked_add`，
//! 溢出记 EOVER）、见证总数。四者都由同一份表算出，任何一项偏移都说明表被写坏。
//!
//! 账本十四格：八格成功（入册数、grep 四件、sort 四件、总字节、空件计数、
//! 二进制计数、见证总数、最大件），六格拒绝（重复编号、逆序编号、未知工具、
//! 未知类型、单件超限、容量满）。审计逐格钉值。

use crate::alloc::format;
use crate::checks::CheckSet;

/// 段基址。
pub const SEG_BASE: u32 = 0x20c60;

/// 段内低十六位。
const SEG_LO: u16 = (SEG_BASE & 0xffff) as u16;

/// 登记顺序错误。
pub const CC_EORDER: u16 = SEG_LO + 0x01;
/// 重复编号。
pub const CC_EDUP: u16 = SEG_LO + 0x02;
/// 已冻结。
pub const CC_EFROZEN: u16 = SEG_LO + 0x03;
/// 观测缺失。
pub const CC_ENOBS: u16 = SEG_LO + 0x04;
/// 结论不符。
pub const CC_EMISMATCH: u16 = SEG_LO + 0x05;
/// 账本失衡。
pub const CC_EBAL: u16 = SEG_LO + 0x06;
/// 容量满。
pub const CC_EFULL: u16 = SEG_LO + 0x07;
/// 未知工具。
pub const CC_ETOOL: u16 = SEG_LO + 0x08;
/// 未知内容类型。
pub const CC_EKIND: u16 = SEG_LO + 0x09;
/// 单件超字节上限。
pub const CC_EBIG: u16 = SEG_LO + 0x0a;
/// 无判别见证。
pub const CC_ENOCUE: u16 = SEG_LO + 0x0b;
/// 汇总溢出。
pub const CC_EOVER: u16 = SEG_LO + 0x0c;
/// 票号不一致。
pub const CC_ESEAL: u16 = SEG_LO + 0x0d;

/// 工具：grep。
pub const TOOL_GREP: u8 = 1;
/// 工具：sort。
pub const TOOL_SORT: u8 = 2;

/// 类型：纯 ASCII。
pub const KIND_ASCII: u8 = 1;
/// 类型：UTF-8 多字节。
pub const KIND_UTF8: u8 = 2;
/// 类型：含 NUL 的二进制。
pub const KIND_BINARY: u8 = 3;
/// 类型：空文件。
pub const KIND_EMPTY: u8 = 4;

/// 单件字节上限（1 MiB）。
pub const MAX_ENTRY_BYTES: u32 = 1_048_576;
/// 入册容量。
pub const CAP: usize = 12;

/// 操作：入册。
pub const OP_INSERT: u8 = 1;
/// 操作：汇总。
pub const OP_TOTAL: u8 = 2;
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

/// 一条语料记录。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Entry {
    pub id: u16,
    pub tool: u8,
    pub kind: u8,
    pub cues: u8,
    pub bytes: u32,
}

impl Entry {
    /// 构造。
    pub const fn make(id: u16, tool: u8, kind: u8, cues: u8, bytes: u32) -> Entry {
        Entry {
            id,
            tool,
            kind,
            cues,
            bytes,
        }
    }

    /// 空白记录。
    pub const fn blank() -> Entry {
        Entry {
            id: 0,
            tool: 0,
            kind: 0,
            cues: 0,
            bytes: 0,
        }
    }
}

/// 语料清单。
#[derive(Clone, Copy, Debug)]
pub struct Catalog {
    ents: [Entry; CAP],
    n: usize,
    cap: usize,
}

impl Catalog {
    /// 默认容量。
    pub const fn new() -> Catalog {
        Catalog {
            ents: [Entry::blank(); CAP],
            n: 0,
            cap: CAP,
        }
    }

    /// 指定容量。
    pub const fn with_cap(cap: usize) -> Catalog {
        Catalog {
            ents: [Entry::blank(); CAP],
            n: 0,
            cap: if cap > CAP { CAP } else { cap },
        }
    }

    /// 条数。
    pub fn len(&self) -> usize {
        self.n
    }

    /// 是否为空。
    pub fn is_empty(&self) -> bool {
        self.n == 0
    }

    /// 末尾编号。
    pub fn last_id(&self) -> u16 {
        if self.n == 0 {
            return 0;
        }
        match self.ents.get(self.n - 1) {
            Some(e) => e.id,
            None => 0,
        }
    }

    /// 取第 i 条。
    pub fn at(&self, i: usize) -> Option<Entry> {
        if i >= self.n {
            return None;
        }
        self.ents.get(i).copied()
    }

    /// 按编号查。
    pub fn entry(&self, id: u16) -> Option<Entry> {
        let mut i = 0usize;
        while i < self.n {
            match self.ents.get(i) {
                Some(e) => {
                    if e.id == id {
                        return Some(*e);
                    }
                }
                None => return None,
            }
            i += 1;
        }
        None
    }

    /// 入册：工具 → 类型 → 见证 → 字节 → 编号 → 容量。
    pub fn insert(&mut self, e: Entry) -> Result<(), u16> {
        if e.tool != TOOL_GREP && e.tool != TOOL_SORT {
            return Err(CC_ETOOL);
        }
        if e.kind != KIND_ASCII
            && e.kind != KIND_UTF8
            && e.kind != KIND_BINARY
            && e.kind != KIND_EMPTY
        {
            return Err(CC_EKIND);
        }
        if e.cues == 0 {
            return Err(CC_ENOCUE);
        }
        if e.bytes > MAX_ENTRY_BYTES {
            return Err(CC_EBIG);
        }
        if e.id == 0 {
            return Err(CC_EORDER);
        }
        if self.n > 0 {
            let last = self.last_id();
            if e.id == last {
                return Err(CC_EDUP);
            }
            if e.id < last {
                return Err(CC_EORDER);
            }
        }
        if self.n >= self.cap || self.n >= CAP {
            return Err(CC_EFULL);
        }
        match self.ents.get_mut(self.n) {
            Some(slot) => *slot = e,
            None => return Err(CC_EFULL),
        }
        self.n += 1;
        Ok(())
    }

    /// 按工具计数。
    pub fn count_tool(&self, tool: u8) -> u32 {
        let mut n = 0u32;
        let mut i = 0usize;
        while i < self.n {
            if let Some(e) = self.ents.get(i) {
                if e.tool == tool {
                    n += 1;
                }
            }
            i += 1;
        }
        n
    }

    /// 按类型计数。
    pub fn count_kind(&self, kind: u8) -> u32 {
        let mut n = 0u32;
        let mut i = 0usize;
        while i < self.n {
            if let Some(e) = self.ents.get(i) {
                if e.kind == kind {
                    n += 1;
                }
            }
            i += 1;
        }
        n
    }

    /// 总字节。
    pub fn total_bytes(&self) -> Result<u32, u16> {
        let mut sum = 0u32;
        let mut i = 0usize;
        while i < self.n {
            match self.ents.get(i) {
                Some(e) => match sum.checked_add(e.bytes) {
                    Some(v) => sum = v,
                    None => return Err(CC_EOVER),
                },
                None => return Err(CC_ENOBS),
            }
            i += 1;
        }
        Ok(sum)
    }

    /// 见证总数。
    pub fn sum_cues(&self) -> u32 {
        let mut sum = 0u32;
        let mut i = 0usize;
        while i < self.n {
            if let Some(e) = self.ents.get(i) {
                sum = sum.saturating_add(u32::from(e.cues));
            }
            i += 1;
        }
        sum
    }

    /// 最大单件。
    pub fn max_bytes(&self) -> u32 {
        let mut m = 0u32;
        let mut i = 0usize;
        while i < self.n {
            if let Some(e) = self.ents.get(i) {
                if e.bytes > m {
                    m = e.bytes;
                }
            }
            i += 1;
        }
        m
    }
}

/// 标准语料清单：八件，两工具各四。
pub fn corpus() -> Catalog {
    let mut c = Catalog::new();
    let _ = c.insert(Entry::make(1, TOOL_GREP, KIND_ASCII, 13, 4096));
    let _ = c.insert(Entry::make(2, TOOL_GREP, KIND_UTF8, 13, 8192));
    let _ = c.insert(Entry::make(3, TOOL_GREP, KIND_BINARY, 3, 2048));
    let _ = c.insert(Entry::make(4, TOOL_GREP, KIND_EMPTY, 1, 0));
    let _ = c.insert(Entry::make(5, TOOL_SORT, KIND_ASCII, 13, 4096));
    let _ = c.insert(Entry::make(6, TOOL_SORT, KIND_UTF8, 13, 8192));
    let _ = c.insert(Entry::make(7, TOOL_SORT, KIND_BINARY, 2, 1024));
    let _ = c.insert(Entry::make(8, TOOL_SORT, KIND_EMPTY, 1, 0));
    c
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
            return Err(CC_EFROZEN);
        }
        let mut i = 0usize;
        while i < self.len {
            match self.rows.get(i) {
                Some(x) => {
                    if x.slot == s.slot {
                        self.rejects += 1;
                        return Err(CC_EDUP);
                    }
                }
                None => return Err(CC_ENOBS),
            }
            i += 1;
        }
        if s.slot as usize != self.len {
            self.rejects += 1;
            return Err(CC_EORDER);
        }
        if self.len >= self.cap || self.len >= ROWS {
            self.rejects += 1;
            return Err(CC_EFULL);
        }
        if s.op == 0 {
            self.rejects += 1;
            return Err(CC_EMISMATCH);
        }
        if s.ok && s.code != 0 {
            self.rejects += 1;
            return Err(CC_EMISMATCH);
        }
        if !s.ok && s.code == 0 {
            self.rejects += 1;
            return Err(CC_EMISMATCH);
        }
        match self.rows.get_mut(self.len) {
            Some(x) => *x = s,
            None => return Err(CC_EFULL),
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
            return Err(CC_EBAL);
        }
        if self.ok_count() != 8 || self.err_count() != 6 {
            return Err(CC_EBAL);
        }
        let mut i = 0usize;
        while i < self.len {
            let s = match self.rows.get(i) {
                Some(v) => *v,
                None => return Err(CC_ENOBS),
            };
            if s.slot as usize != i {
                return Err(CC_EORDER);
            }
            if s.ok {
                if s.code != 0 {
                    return Err(CC_EMISMATCH);
                }
            } else if s.code == 0 || s.value != 0 {
                return Err(CC_EMISMATCH);
            }
            i += 1;
        }
        let vals = [8u32, 4, 4, 27648, 2, 2, 59, 8192];
        let mut k = 0usize;
        while k < vals.len() {
            match (self.sample(k), vals.get(k)) {
                (Some(s), Some(w)) => {
                    if !s.ok || s.value != *w {
                        return Err(CC_EMISMATCH);
                    }
                }
                _ => return Err(CC_EBAL),
            }
            k += 1;
        }
        let codes = [
            CC_EDUP, CC_EORDER, CC_ETOOL, CC_EKIND, CC_EBIG, CC_EFULL,
        ];
        let mut c = 0usize;
        let mut j = 8usize;
        while j < self.len {
            match self.rows.get(j) {
                Some(s) => match codes.get(c) {
                    Some(w) => {
                        if s.ok || s.code != *w || s.value != 0 {
                            return Err(CC_EMISMATCH);
                        }
                    }
                    None => return Err(CC_EBAL),
                },
                None => return Err(CC_ENOBS),
            }
            c += 1;
            j += 1;
        }
        if c != codes.len() {
            return Err(CC_EBAL);
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
            return Err(CC_EFROZEN);
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
            return Err(CC_EBAL);
        }
        let h = fresh.seal();
        if self.frozen && h != self.root {
            return Err(CC_ESEAL);
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

/// 汇总结果取数值。
fn total_of(r: Result<u32, u16>) -> u32 {
    match r {
        Ok(v) => v,
        Err(_) => 0xffff_ffff,
    }
}

/// 构建十四格账本。
pub fn build_ledger() -> Ledger {
    let mut l = Ledger::new();
    let c = corpus();

    let _ = l.register(ok_row(0, OP_INSERT, u32::try_from(c.len()).unwrap_or(0)));
    let _ = l.register(ok_row(1, OP_TOTAL, c.count_tool(TOOL_GREP)));
    let _ = l.register(ok_row(2, OP_TOTAL, c.count_tool(TOOL_SORT)));
    let _ = l.register(ok_row(3, OP_TOTAL, total_of(c.total_bytes())));
    let _ = l.register(ok_row(4, OP_TOTAL, c.count_kind(KIND_EMPTY)));
    let _ = l.register(ok_row(5, OP_TOTAL, c.count_kind(KIND_BINARY)));
    let _ = l.register(ok_row(6, OP_TOTAL, c.sum_cues()));
    let _ = l.register(ok_row(7, OP_TOTAL, c.max_bytes()));

    let mut tail = corpus();
    let dup = match tail.insert(Entry::make(8, TOOL_GREP, KIND_ASCII, 1, 1)) {
        Ok(()) => 0,
        Err(code) => code,
    };
    let _ = l.register(err_row(8, OP_INSERT, dup));

    let mut back = corpus();
    let order = match back.insert(Entry::make(2, TOOL_SORT, KIND_ASCII, 1, 1)) {
        Ok(()) => 0,
        Err(code) => code,
    };
    let _ = l.register(err_row(9, OP_INSERT, order));

    let mut tool = corpus();
    let bad_tool = match tool.insert(Entry::make(9, 9, KIND_ASCII, 1, 1)) {
        Ok(()) => 0,
        Err(code) => code,
    };
    let _ = l.register(err_row(10, OP_INSERT, bad_tool));

    let mut kind = corpus();
    let bad_kind = match kind.insert(Entry::make(9, TOOL_GREP, 9, 1, 1)) {
        Ok(()) => 0,
        Err(code) => code,
    };
    let _ = l.register(err_row(11, OP_INSERT, bad_kind));

    let mut big = corpus();
    let too_big = match big.insert(Entry::make(9, TOOL_GREP, KIND_ASCII, 1, MAX_ENTRY_BYTES + 1)) {
        Ok(()) => 0,
        Err(code) => code,
    };
    let _ = l.register(err_row(12, OP_INSERT, too_big));

    let mut small = Ledger::with_cap(3);
    let _ = small.register(ok_row(0, OP_INSERT, 1));
    let _ = small.register(ok_row(1, OP_INSERT, 1));
    let _ = small.register(ok_row(2, OP_INSERT, 1));
    let full = reg_code(small.register(ok_row(3, OP_INSERT, 1)));
    let _ = l.register(err_row(13, OP_LEDGER, full));

    l
}

/// 四路注入：重放冻结、双向汇总一致、失败不改表、篡改复原。
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
        && two == Err(CC_EFROZEN)
        && l.rejects() == 1
        && before == l.len()
        && root != 0
        && l.audit().is_ok();

    out[1] = {
        let c = corpus();
        let by_tool = c.count_tool(TOOL_GREP) + c.count_tool(TOOL_SORT);
        let by_kind = c.count_kind(KIND_ASCII)
            + c.count_kind(KIND_UTF8)
            + c.count_kind(KIND_BINARY)
            + c.count_kind(KIND_EMPTY);
        by_tool == 8
            && by_kind == 8
            && c.total_bytes() == Ok(27648)
            && c.count_kind(KIND_EMPTY) == 2
            && c.max_bytes() == 8192
    };

    out[2] = {
        let mut c = corpus();
        let n0 = c.len();
        let total0 = c.total_bytes();
        let rejected = c.insert(Entry::make(9, 9, KIND_ASCII, 1, 1)) == Err(CC_ETOOL)
            && c.insert(Entry::make(9, TOOL_GREP, 9, 1, 1)) == Err(CC_EKIND)
            && c.insert(Entry::make(9, TOOL_GREP, KIND_ASCII, 0, 1)) == Err(CC_ENOCUE)
            && c.len() == n0
            && c.total_bytes() == total0;
        let accepted = c
            .insert(Entry::make(9, TOOL_GREP, KIND_ASCII, 1, 8))
            .is_ok()
            && c.len() == n0 + 1
            && c.total_bytes() == Ok(27656)
            && c.last_id() == 9;
        rejected && accepted
    };

    out[3] = {
        let mut t = build_ledger();
        let clean = t.audit().is_ok();
        let old = match t.sample(3) {
            Some(s) => s.value,
            None => 0,
        };
        if let Some(r) = t.rows.get_mut(3) {
            r.value = old.saturating_add(4);
        }
        let caught = t.audit() == Err(CC_EMISMATCH);
        if let Some(r) = t.rows.get_mut(3) {
            r.value = old;
        }
        clean && caught && t.audit().is_ok()
    };

    out
}

/// 十三判据。
pub fn run_unx11458_checks() -> CheckSet {
    let mut s = CheckSet::new("unx11458.corpuscatalog");
    let l = build_ledger();

    s.add_dyn(
        "CC1-十四格齐备",
        l.len() == ROWS && l.ok_count() == 8 && l.err_count() == 6 && l.audit().is_ok(),
        &format!("n={} ok={} err={}", l.len(), l.ok_count(), l.err_count()),
    );

    s.add(
        "CC2-八件入册",
        l.sample(0).map(|x| x.value) == Some(8)
            && corpus().len() == 8
            && !corpus().is_empty()
            && corpus().last_id() == 8,
        "static",
    );

    s.add(
        "CC3-两工具各四",
        l.sample(1).map(|x| x.value) == Some(4)
            && l.sample(2).map(|x| x.value) == Some(4)
            && match corpus().entry(1) {
                Some(e) => e.tool == TOOL_GREP,
                None => false,
            }
            && match corpus().entry(8) {
                Some(e) => e.tool == TOOL_SORT,
                None => false,
            },
        "static",
    );

    s.add(
        "CC4-总字节",
        l.sample(3).map(|x| x.value) == Some(27648)
            && corpus().total_bytes() == Ok(27648)
            && corpus().total_bytes() == Ok(2 * 4096 + 2 * 8192 + 2048 + 1024),
        "static",
    );

    s.add(
        "CC5-空件与二进制",
        l.sample(4).map(|x| x.value) == Some(2)
            && l.sample(5).map(|x| x.value) == Some(2)
            && corpus().count_kind(KIND_EMPTY) == 2
            && corpus().count_kind(KIND_BINARY) == 2
            && corpus().count_kind(KIND_ASCII) == 2
            && corpus().count_kind(KIND_UTF8) == 2,
        "static",
    );

    s.add(
        "CC6-见证总数",
        l.sample(6).map(|x| x.value) == Some(59)
            && corpus().sum_cues() == 59
            && corpus().sum_cues() > corpus().len() as u32,
        "static",
    );

    s.add(
        "CC7-最大件",
        l.sample(7).map(|x| x.value) == Some(8192)
            && corpus().max_bytes() == 8192
            && corpus().max_bytes() <= MAX_ENTRY_BYTES,
        "static",
    );

    s.add(
        "CC8-按编号查表",
        corpus().entry(2) == Some(Entry::make(2, TOOL_GREP, KIND_UTF8, 13, 8192))
            && corpus().entry(7) == Some(Entry::make(7, TOOL_SORT, KIND_BINARY, 2, 1024))
            && corpus().entry(9).is_none()
            && corpus().at(0) == Some(Entry::make(1, TOOL_GREP, KIND_ASCII, 13, 4096)),
        "static",
    );

    s.add(
        "CC9-重复与逆序",
        l.sample(8).map(|x| x.code) == Some(CC_EDUP)
            && l.sample(9).map(|x| x.code) == Some(CC_EORDER)
            && {
                let mut c = corpus();
                c.insert(Entry::make(8, TOOL_GREP, KIND_ASCII, 1, 1)) == Err(CC_EDUP)
                    && c.insert(Entry::make(2, TOOL_GREP, KIND_ASCII, 1, 1)) == Err(CC_EORDER)
                    && c.len() == 8
            },
        "static",
    );

    s.add(
        "CC10-工具与类型闸门",
        l.sample(10).map(|x| x.code) == Some(CC_ETOOL)
            && l.sample(11).map(|x| x.code) == Some(CC_EKIND)
            && {
                let mut c = corpus();
                c.insert(Entry::make(9, 0, KIND_ASCII, 1, 1)) == Err(CC_ETOOL)
                    && c.insert(Entry::make(9, TOOL_SORT, 0, 1, 1)) == Err(CC_EKIND)
                    && c.len() == 8
            },
        "static",
    );

    s.add(
        "CC11-见证与字节闸门",
        l.sample(12).map(|x| x.code) == Some(CC_EBIG)
            && {
                let mut c = corpus();
                c.insert(Entry::make(9, TOOL_GREP, KIND_ASCII, 0, 1)) == Err(CC_ENOCUE)
                    && c.insert(Entry::make(9, TOOL_GREP, KIND_ASCII, 1, MAX_ENTRY_BYTES)) == Ok(())
                    && c.last_id() == 9
            }
            && {
                let mut d = corpus();
                d.insert(Entry::make(9, TOOL_GREP, KIND_ASCII, 1, MAX_ENTRY_BYTES + 1))
                    == Err(CC_EBIG)
                    && d.len() == 8
            },
        "static",
    );

    s.add(
        "CC12-容量闸门",
        l.sample(13).map(|x| x.code) == Some(CC_EFULL)
            && {
                let mut c = Catalog::with_cap(2);
                c.insert(Entry::make(1, TOOL_GREP, KIND_ASCII, 1, 1)).is_ok()
                    && c.insert(Entry::make(2, TOOL_GREP, KIND_ASCII, 1, 1)).is_ok()
                    && c.insert(Entry::make(3, TOOL_GREP, KIND_ASCII, 1, 1)) == Err(CC_EFULL)
                    && c.len() == 2
            },
        "static",
    );

    s.add(
        "CC13-冻结重放与四注入",
        {
            let mut f = build_ledger();
            let rep = f.replay();
            let one = f.freeze();
            let two = f.freeze();
            let inj = inject_four();
            one.is_ok()
                && one == rep
                && two == Err(CC_EFROZEN)
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
    fn corpus_shape() {
        let c = corpus();
        assert_eq!(c.len(), 8);
        assert_eq!(c.last_id(), 8);
        assert_eq!(c.count_tool(TOOL_GREP), 4);
        assert_eq!(c.count_tool(TOOL_SORT), 4);
        assert_eq!(c.total_bytes(), Ok(27648));
        assert_eq!(c.sum_cues(), 59);
        assert_eq!(c.max_bytes(), 8192);
    }

    #[test]
    fn lookup_by_id() {
        assert_eq!(
            corpus().entry(2),
            Some(Entry::make(2, TOOL_GREP, KIND_UTF8, 13, 8192))
        );
        assert!(corpus().entry(0).is_none());
        assert!(corpus().entry(99).is_none());
    }

    #[test]
    fn strictly_increasing_ids() {
        let mut c = corpus();
        assert_eq!(
            c.insert(Entry::make(8, TOOL_GREP, KIND_ASCII, 1, 1)),
            Err(CC_EDUP)
        );
        assert_eq!(
            c.insert(Entry::make(3, TOOL_GREP, KIND_ASCII, 1, 1)),
            Err(CC_EORDER)
        );
        assert_eq!(c.len(), 8);
    }

    #[test]
    fn entry_gates() {
        let mut c = corpus();
        assert_eq!(
            c.insert(Entry::make(9, 9, KIND_ASCII, 1, 1)),
            Err(CC_ETOOL)
        );
        assert_eq!(
            c.insert(Entry::make(9, TOOL_GREP, 9, 1, 1)),
            Err(CC_EKIND)
        );
        assert_eq!(
            c.insert(Entry::make(9, TOOL_GREP, KIND_ASCII, 0, 1)),
            Err(CC_ENOCUE)
        );
        assert_eq!(
            c.insert(Entry::make(9, TOOL_GREP, KIND_ASCII, 1, MAX_ENTRY_BYTES + 1)),
            Err(CC_EBIG)
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
        assert_eq!(l.freeze(), Err(CC_EFROZEN));
    }

    #[test]
    fn checks_pass() {
        let s = run_unx11458_checks();
        assert_eq!(s.len(), 13);
        assert!(s.all_passed(), "{}", s.red_items_text());
    }
}
