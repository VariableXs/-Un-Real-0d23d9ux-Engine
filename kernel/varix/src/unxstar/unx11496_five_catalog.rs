//! UNX-F11496 五工具判据入册登记（0x20ed0 段）
//!
//! 判据不是散落的名字，而是入册记录：每条记录挂一个工具、一个判据序号、一份语料
//! 和一个全局编号。五个工具各三条判据，编号从 201 起连续排到 215，工具按 1..=5
//! 分组，组内判据按 1..=3 递增，所以“下一个编号是多少”完全由当前格数决定，
//! 任何跳号、回填、换序都会在登记口被挡下。
//!
//! 登记口按固定次序过闸：工具维 → 编号是否已存在 → 编号是否等于期望 → 判据维 →
//! 语料维 → 容量。重复检查先于顺序检查，是为了让“把 202 再登记一次”得到 EDUPI
//! 而不是 EORDI——重复是调用方的语义错误，跳号只是抄写错误，两者不能混报。
//!
//! 账本十四格：八格成功覆盖总格数、分组计数、编号连续、工具/判据反查与语料掩码；
//! 六格拒绝依次是工具越界、跳号、重复编号、判据越界、语料越界与容量满。

use crate::alloc::format;
use crate::checks::CheckSet;

/// 段基址。
pub const SEG_BASE: u32 = 0x20ed0;

/// 编号非法。
pub const FC_EID: u32 = SEG_BASE + 0x01;
/// 登记顺序错误。
pub const FC_EORDER: u32 = SEG_BASE + 0x02;
/// 重复登记。
pub const FC_EDUP: u32 = SEG_BASE + 0x03;
/// 已冻结。
pub const FC_EFROZEN: u32 = SEG_BASE + 0x04;
/// 观测不存在的格。
pub const FC_ENOBS: u32 = SEG_BASE + 0x05;
/// 结论不符。
pub const FC_EMISMATCH: u32 = SEG_BASE + 0x06;
/// 账本失衡。
pub const FC_EBAL: u32 = SEG_BASE + 0x07;
/// 容量满。
pub const FC_EFULL: u32 = SEG_BASE + 0x08;
/// 票号不一致。
pub const FC_ESEAL: u32 = SEG_BASE + 0x09;
/// 工具越界。
pub const FC_ETOOL: u32 = SEG_BASE + 0x0a;
/// 判据越界。
pub const FC_ECRIT: u32 = SEG_BASE + 0x0b;
/// 编号已存在。
pub const FC_EDUPI: u32 = SEG_BASE + 0x0c;
/// 编号跳号。
pub const FC_EORDI: u32 = SEG_BASE + 0x0d;
/// 语料越界。
pub const FC_ECORP: u32 = SEG_BASE + 0x0e;

/// 判决：成功。
pub const V_OK: u32 = 0;

/// 工具：echo。
pub const TOOL_ECHO: u8 = 1;
/// 工具：dd。
pub const TOOL_DD: u8 = 2;
/// 工具：xargs。
pub const TOOL_XARGS: u8 = 3;
/// 工具：tr。
pub const TOOL_TR: u8 = 4;
/// 工具：wc。
pub const TOOL_WC: u8 = 5;

/// 工具总数。
pub const TOOLS: u8 = 5;
/// 每工具判据数。
pub const CRITS: u8 = 3;
/// 全表格数。
pub const ENTRIES: usize = 15;
/// 入册容量。
pub const ENTRY_CAP: usize = 24;
/// 首个编号。
pub const FIRST_ID: u16 = 201;
/// 样本格数。
pub const SAMPLES: usize = 14;

/// 操作：总量。
pub const OP_COUNT: u8 = 1;
/// 操作：分组计数。
pub const OP_GROUP: u8 = 2;
/// 操作：按编号反查。
pub const OP_LOOK: u8 = 3;
/// 操作：期望编号。
pub const OP_ID: u8 = 4;
/// 操作：语料掩码。
pub const OP_MASK: u8 = 5;
/// 操作：按工具与判据反查编号。
pub const OP_ENTRY: u8 = 6;
/// 操作：登记。
pub const OP_ADD: u8 = 7;

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

/// 工具维是否合法。
pub fn valid_tool(tool: u8) -> bool {
    tool >= TOOL_ECHO && tool <= TOOL_WC
}

/// 判据维是否合法。
pub fn valid_crit(crit: u8) -> bool {
    crit >= 1 && crit <= CRITS
}

/// 语料维是否合法。
pub fn valid_corpus(corpus: u8) -> bool {
    corpus >= 1 && corpus <= CRITS
}

/// 第 index 格应有的编号。
pub fn expected_id(index: usize) -> u16 {
    FIRST_ID.saturating_add(index as u16)
}

/// 一条入册记录。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Entry {
    pub tool: u8,
    pub id: u16,
    pub corpus: u8,
    pub crit: u8,
}

impl Entry {
    /// 空白记录。
    pub const fn blank() -> Entry {
        Entry {
            tool: 0,
            id: 0,
            corpus: 0,
            crit: 0,
        }
    }
}

/// 判据册。
#[derive(Clone, Copy, Debug)]
pub struct Catalog {
    rows: [Entry; ENTRIES],
    len: usize,
    cap: usize,
}

impl Catalog {
    /// 空册。
    pub const fn new() -> Catalog {
        Catalog {
            rows: [Entry::blank(); ENTRIES],
            len: 0,
            cap: ENTRY_CAP,
        }
    }

    /// 指定容量。
    pub const fn with_cap(cap: usize) -> Catalog {
        Catalog {
            rows: [Entry::blank(); ENTRIES],
            len: 0,
            cap,
        }
    }

    /// 登记一条：工具 → 重复编号 → 期望编号 → 判据 → 语料 → 容量。
    pub fn add(&mut self, e: Entry) -> Result<usize, u32> {
        if !valid_tool(e.tool) {
            return Err(FC_ETOOL);
        }
        let mut i = 0usize;
        while i < self.len {
            if let Some(r) = self.rows.get(i) {
                if r.id == e.id {
                    return Err(FC_EDUPI);
                }
            }
            i += 1;
        }
        if e.id != expected_id(self.len) {
            return Err(FC_EORDI);
        }
        if !valid_crit(e.crit) {
            return Err(FC_ECRIT);
        }
        if !valid_corpus(e.corpus) {
            return Err(FC_ECORP);
        }
        if self.len >= self.cap || self.len >= ENTRIES {
            return Err(FC_EFULL);
        }
        match self.rows.get_mut(self.len) {
            Some(x) => *x = e,
            None => return Err(FC_EFULL),
        }
        let slot = self.len;
        self.len += 1;
        Ok(slot)
    }

    /// 格数。
    pub fn len(&self) -> usize {
        self.len
    }

    /// 是否为空。
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// 取记录。
    pub fn entry(&self, i: usize) -> Option<Entry> {
        if i >= self.len {
            return None;
        }
        self.rows.get(i).copied()
    }

    /// 工具的分组计数。
    pub fn crit_count(&self, tool: u8) -> Result<usize, u32> {
        if !valid_tool(tool) {
            return Err(FC_ETOOL);
        }
        let mut n = 0usize;
        let mut i = 0usize;
        while i < self.len {
            if let Some(r) = self.rows.get(i) {
                if r.tool == tool {
                    n += 1;
                }
            }
            i += 1;
        }
        Ok(n)
    }

    /// 按编号反查记录。
    pub fn by_id(&self, id: u16) -> Result<Entry, u32> {
        let mut i = 0usize;
        while i < self.len {
            if let Some(r) = self.rows.get(i) {
                if r.id == id {
                    return Ok(*r);
                }
            }
            i += 1;
        }
        Err(FC_ENOBS)
    }

    /// 按工具与判据反查编号。
    pub fn entry_id(&self, tool: u8, crit: u8) -> Result<u16, u32> {
        if !valid_tool(tool) {
            return Err(FC_ETOOL);
        }
        if !valid_crit(crit) {
            return Err(FC_ECRIT);
        }
        let mut i = 0usize;
        while i < self.len {
            if let Some(r) = self.rows.get(i) {
                if r.tool == tool && r.crit == crit {
                    return Ok(r.id);
                }
            }
            i += 1;
        }
        Err(FC_ENOBS)
    }

    /// 语料位掩码：第 c 份语料占第 c-1 位。
    pub fn corpus_mask(&self) -> u8 {
        let mut m = 0u8;
        let mut i = 0usize;
        while i < self.len {
            if let Some(r) = self.rows.get(i) {
                if r.corpus >= 1 && r.corpus <= 8 {
                    m |= 1u8 << (r.corpus - 1);
                }
            }
            i += 1;
        }
        m
    }

    /// 编号是否从 FIRST_ID 起连续。
    pub fn ids_contiguous(&self) -> bool {
        let mut i = 0usize;
        while i < self.len {
            match self.rows.get(i) {
                Some(r) => {
                    if r.id != expected_id(i) {
                        return false;
                    }
                }
                None => return false,
            }
            i += 1;
        }
        true
    }
}

/// 全表：五工具各三条判据。
pub fn build_catalog() -> Catalog {
    let mut c = Catalog::new();
    let mut tool = TOOL_ECHO;
    while tool <= TOOL_WC {
        let mut crit = 1u8;
        while crit <= CRITS {
            let _ = c.add(Entry {
                tool,
                id: expected_id(c.len()),
                corpus: crit,
                crit,
            });
            crit += 1;
        }
        tool += 1;
    }
    c
}

/// 一个账本样本。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Sample {
    pub slot: u8,
    pub op: u8,
    pub ok: bool,
    pub code: u32,
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

/// 样本账本。
#[derive(Clone, Copy, Debug)]
pub struct Ledger {
    rows: [Sample; SAMPLES],
    len: usize,
    cap: usize,
    seal: u64,
    frozen: bool,
    rejects: u32,
}

impl Ledger {
    /// 空账本。
    pub const fn new() -> Ledger {
        Ledger {
            rows: [Sample::blank(); SAMPLES],
            len: 0,
            cap: SAMPLES,
            seal: 0,
            frozen: false,
            rejects: 0,
        }
    }

    /// 指定容量。
    pub const fn with_cap(cap: usize) -> Ledger {
        Ledger {
            rows: [Sample::blank(); SAMPLES],
            len: 0,
            cap,
            seal: 0,
            frozen: false,
            rejects: 0,
        }
    }

    /// 登记一格：冻结 → 编号 → 重复 → 顺序 → 容量。
    pub fn register(&mut self, row: Sample) -> Result<(), u32> {
        if self.frozen {
            self.rejects += 1;
            return Err(FC_EFROZEN);
        }
        if (row.slot as usize) >= SAMPLES {
            return Err(FC_EID);
        }
        if (row.slot as usize) < self.len {
            return Err(FC_EDUP);
        }
        if (row.slot as usize) != self.len {
            return Err(FC_EORDER);
        }
        if self.len >= self.cap {
            return Err(FC_EFULL);
        }
        match self.rows.get_mut(self.len) {
            Some(x) => *x = row,
            None => return Err(FC_EID),
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

    /// 取样本。
    pub fn sample(&self, i: usize) -> Option<Sample> {
        if i >= self.len {
            return None;
        }
        self.rows.get(i).copied()
    }

    /// 成功计数。
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

    /// 拒绝计数。
    pub fn err_count(&self) -> usize {
        self.len - self.ok_count()
    }

    /// 对账。
    pub fn audit(&self) -> Result<(), u32> {
        if self.len != SAMPLES {
            return Err(FC_EBAL);
        }
        if self.ok_count() != 8 || self.err_count() != 6 {
            return Err(FC_EBAL);
        }
        let mut i = 0usize;
        while i < self.len {
            match self.rows.get(i) {
                Some(s) => {
                    if s.slot as usize != i {
                        return Err(FC_EORDER);
                    }
                    if s.ok {
                        if s.code != V_OK {
                            return Err(FC_EMISMATCH);
                        }
                    } else if s.code == V_OK || s.value != 0 {
                        return Err(FC_EMISMATCH);
                    }
                }
                None => return Err(FC_ENOBS),
            }
            i += 1;
        }
        let vals = [15u32, 3, 3, 5, 201, 215, 7, 208];
        let mut k = 0usize;
        while k < vals.len() {
            match (self.sample(k), vals.get(k)) {
                (Some(s), Some(w)) => {
                    if !s.ok || s.value != *w {
                        return Err(FC_EMISMATCH);
                    }
                }
                _ => return Err(FC_EBAL),
            }
            k += 1;
        }
        let codes = [FC_ETOOL, FC_EORDI, FC_EDUPI, FC_ECRIT, FC_ECORP, FC_EFULL];
        let mut c = 0usize;
        let mut j = 8usize;
        while j < self.len {
            match self.rows.get(j) {
                Some(s) => match codes.get(c) {
                    Some(w) => {
                        if s.code != *w {
                            return Err(FC_EMISMATCH);
                        }
                    }
                    None => return Err(FC_EBAL),
                },
                None => return Err(FC_ENOBS),
            }
            c += 1;
            j += 1;
        }
        if c != codes.len() {
            return Err(FC_EBAL);
        }
        Ok(())
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
        h
    }

    /// 冻结。
    pub fn freeze(&mut self) -> Result<u64, u32> {
        if self.frozen {
            self.rejects += 1;
            return Err(FC_EFROZEN);
        }
        let s = self.seal();
        self.seal = s;
        self.frozen = true;
        Ok(s)
    }

    /// 重放。
    pub fn replay(&self) -> Result<u64, u32> {
        let fresh = build_ledger();
        if fresh.len != self.len {
            return Err(FC_EBAL);
        }
        let h = fresh.seal();
        if self.frozen && h != self.seal {
            return Err(FC_ESEAL);
        }
        Ok(h)
    }

    /// 封存票号。
    pub fn root(&self) -> u64 {
        self.seal
    }

    /// 二次冻结次数。
    pub fn rejects(&self) -> u32 {
        self.rejects
    }

    /// 是否已冻结。
    pub fn is_frozen(&self) -> bool {
        self.frozen
    }
}

fn cell(slot: u8, op: u8, r: Result<u32, u32>) -> Sample {
    match r {
        Ok(v) => Sample {
            slot,
            op,
            ok: true,
            code: V_OK,
            value: v,
        },
        Err(c) => Sample {
            slot,
            op,
            ok: false,
            code: c,
            value: 0,
        },
    }
}

/// 十四格账本。
pub fn build_ledger() -> Ledger {
    let mut l = Ledger::new();
    let c = build_catalog();

    let _ = l.register(cell(0, OP_COUNT, Ok(c.len() as u32)));
    let _ = l.register(cell(1, OP_GROUP, c.crit_count(TOOL_ECHO).map(|n| n as u32)));
    let _ = l.register(cell(2, OP_GROUP, c.crit_count(TOOL_WC).map(|n| n as u32)));
    let _ = l.register(cell(3, OP_LOOK, c.by_id(215).map(|e| e.tool as u32)));
    let _ = l.register(cell(4, OP_ID, Ok(expected_id(0) as u32)));
    let _ = l.register(cell(5, OP_ID, Ok(expected_id(ENTRIES - 1) as u32)));
    let _ = l.register(cell(6, OP_MASK, Ok(c.corpus_mask() as u32)));
    let _ = l.register(cell(7, OP_ENTRY, c.entry_id(TOOL_XARGS, 2).map(u32::from)));

    let mut t = Catalog::new();
    let _ = l.register(cell(8, OP_ADD, t.add(Entry { tool: 9, id: 201, corpus: 1, crit: 1 }).map(|n| n as u32)));
    let mut o = Catalog::new();
    let _ = l.register(cell(9, OP_ADD, o.add(Entry { tool: TOOL_DD, id: 205, corpus: 1, crit: 1 }).map(|n| n as u32)));
    let mut d = Catalog::new();
    let _ = d.add(Entry { tool: TOOL_ECHO, id: 201, corpus: 1, crit: 1 });
    let _ = d.add(Entry { tool: TOOL_ECHO, id: 202, corpus: 2, crit: 2 });
    let _ = l.register(cell(10, OP_ADD, d.add(Entry { tool: TOOL_ECHO, id: 202, corpus: 3, crit: 3 }).map(|n| n as u32)));
    let mut k = Catalog::new();
    let _ = l.register(cell(11, OP_ADD, k.add(Entry { tool: TOOL_ECHO, id: 201, corpus: 1, crit: 0 }).map(|n| n as u32)));
    let mut p = Catalog::new();
    let _ = l.register(cell(12, OP_ADD, p.add(Entry { tool: TOOL_ECHO, id: 201, corpus: 0, crit: 1 }).map(|n| n as u32)));
    let mut f = Catalog::with_cap(2);
    let _ = f.add(Entry { tool: TOOL_ECHO, id: 201, corpus: 1, crit: 1 });
    let _ = f.add(Entry { tool: TOOL_ECHO, id: 202, corpus: 2, crit: 2 });
    let _ = l.register(cell(13, OP_ADD, f.add(Entry { tool: TOOL_ECHO, id: 203, corpus: 3, crit: 3 }).map(|n| n as u32)));

    l
}

/// 四路注入：账本自洽、全表编号、登记口次序、拒绝后恢复。
pub fn inject_four() -> [bool; 4] {
    let mut out = [false; 4];

    let mut l = build_ledger();
    let base = l.len() == SAMPLES && l.audit().is_ok();
    let rep = l.replay();
    let root = match l.freeze() {
        Ok(v) => v,
        Err(_) => 0,
    };
    let again = l.freeze() == Err(FC_EFROZEN);
    out[0] = base && root != 0 && rep == Ok(root) && again && l.rejects() == 1;

    let c = build_catalog();
    out[1] = c.len() == ENTRIES
        && c.ids_contiguous()
        && c.crit_count(TOOL_TR) == Ok(3)
        && c.entry_id(TOOL_XARGS, 2) == Ok(208)
        && c.by_id(215).map(|e| e.tool) == Ok(TOOL_WC);

    let mut d = Catalog::new();
    let one = d.add(Entry { tool: TOOL_ECHO, id: 201, corpus: 1, crit: 1 }) == Ok(0);
    let two = d.add(Entry { tool: TOOL_ECHO, id: 202, corpus: 2, crit: 2 }) == Ok(1);
    let dup = d.add(Entry { tool: TOOL_ECHO, id: 202, corpus: 3, crit: 3 }) == Err(FC_EDUPI);
    let jump = d.add(Entry { tool: TOOL_DD, id: 205, corpus: 1, crit: 1 }) == Err(FC_EORDI);
    out[2] = one && two && dup && jump && d.len() == 2;

    let tool = Catalog::new()
        .add(Entry { tool: 0, id: 201, corpus: 1, crit: 1 })
        == Err(FC_ETOOL);
    let crit = Catalog::new()
        .add(Entry { tool: TOOL_ECHO, id: 201, corpus: 1, crit: 4 })
        == Err(FC_ECRIT);
    let corp = Catalog::new()
        .add(Entry { tool: TOOL_ECHO, id: 201, corpus: 4, crit: 1 })
        == Err(FC_ECORP);
    let recovered = Catalog::new()
        .add(Entry { tool: TOOL_ECHO, id: 201, corpus: 1, crit: 1 })
        == Ok(0);
    out[3] = tool && crit && corp && recovered;
    out
}

/// 十三判据。
pub fn run_unx11496_checks() -> CheckSet {
    let mut s = CheckSet::new("unx11496.catalog");
    let l = build_ledger();
    let c = build_catalog();

    s.add(
        "FC1-十四格齐备",
        l.len() == SAMPLES && !l.is_empty() && l.sample(0).map(|x| x.op) == Some(OP_COUNT),
        "static",
    );
    s.add_dyn(
        "FC2-八允六拒分布",
        l.ok_count() == 8 && l.err_count() == 6 && l.audit().is_ok(),
        &format!("ok={} err={} audit={:?}", l.ok_count(), l.err_count(), l.audit()),
    );
    s.add(
        "FC3-全表十五格",
        c.len() == ENTRIES && !c.is_empty() && c.entry(14).map(|e| e.tool) == Some(TOOL_WC),
        "static",
    );
    s.add(
        "FC4-编号连续",
        c.ids_contiguous()
            && c.entry(0).map(|e| e.id) == Some(201)
            && c.entry(14).map(|e| e.id) == Some(215),
        "static",
    );
    s.add(
        "FC5-五工具各三条",
        c.crit_count(TOOL_ECHO) == Ok(3)
            && c.crit_count(TOOL_DD) == Ok(3)
            && c.crit_count(TOOL_XARGS) == Ok(3)
            && c.crit_count(TOOL_TR) == Ok(3)
            && c.crit_count(TOOL_WC) == Ok(3),
        "static",
    );
    s.add(
        "FC6-工具反查",
        c.by_id(201).map(|e| e.tool) == Ok(TOOL_ECHO)
            && c.by_id(208).map(|e| e.tool) == Ok(TOOL_XARGS)
            && c.by_id(215).map(|e| e.tool) == Ok(TOOL_WC)
            && l.sample(3).map(|x| x.value) == Some(5),
        "static",
    );
    s.add(
        "FC7-判据反查",
        c.entry_id(TOOL_ECHO, 1) == Ok(201)
            && c.entry_id(TOOL_XARGS, 2) == Ok(208)
            && c.entry_id(TOOL_WC, 3) == Ok(215)
            && l.sample(7).map(|x| x.value) == Some(208),
        "static",
    );
    s.add(
        "FC8-语料挂接",
        c.by_id(201).map(|e| e.corpus) == Ok(1)
            && c.by_id(208).map(|e| e.corpus) == Ok(2)
            && c.by_id(215).map(|e| e.corpus) == Ok(3)
            && c.corpus_mask() == 7
            && l.sample(6).map(|x| x.value) == Some(7),
        "static",
    );
    s.add(
        "FC9-工具越界拒绝",
        Catalog::new()
            .add(Entry { tool: 0, id: 201, corpus: 1, crit: 1 })
            == Err(FC_ETOOL)
            && l.sample(8).map(|x| x.code) == Some(FC_ETOOL),
        "static",
    );
    s.add(
        "FC10-跳号与重复",
        Catalog::new()
            .add(Entry { tool: TOOL_DD, id: 205, corpus: 1, crit: 1 })
            == Err(FC_EORDI)
            && l.sample(9).map(|x| x.code) == Some(FC_EORDI)
            && l.sample(10).map(|x| x.code) == Some(FC_EDUPI),
        "static",
    );
    s.add(
        "FC11-判据越界",
        Catalog::new()
            .add(Entry { tool: TOOL_ECHO, id: 201, corpus: 1, crit: 0 })
            == Err(FC_ECRIT)
            && c.entry_id(TOOL_ECHO, 9) == Err(FC_ECRIT)
            && l.sample(11).map(|x| x.code) == Some(FC_ECRIT),
        "static",
    );
    s.add(
        "FC12-语料越界",
        Catalog::new()
            .add(Entry { tool: TOOL_ECHO, id: 201, corpus: 0, crit: 1 })
            == Err(FC_ECORP)
            && Catalog::new()
                .add(Entry { tool: TOOL_ECHO, id: 201, corpus: 9, crit: 1 })
                == Err(FC_ECORP)
            && l.sample(12).map(|x| x.code) == Some(FC_ECORP),
        "static",
    );
    let mut full = Catalog::with_cap(2);
    let first = full.add(Entry { tool: TOOL_ECHO, id: 201, corpus: 1, crit: 1 });
    let second = full.add(Entry { tool: TOOL_ECHO, id: 202, corpus: 2, crit: 2 });
    let third = full.add(Entry { tool: TOOL_ECHO, id: 203, corpus: 3, crit: 3 });
    let mut f = build_ledger();
    let rep = f.replay();
    let root = match f.freeze() {
        Ok(v) => v,
        Err(_) => 0,
    };
    let again = f.freeze() == Err(FC_EFROZEN);
    let inj = inject_four();
    s.add_dyn(
        "FC13-容量与冻结重放",
        first == Ok(0)
            && second == Ok(1)
            && third == Err(FC_EFULL)
            && l.sample(13).map(|x| x.code) == Some(FC_EFULL)
            && root != 0
            && rep == Ok(root)
            && again
            && inj == [true; 4],
        &format!("third={:?} root={:016x} inj={:?}", third, root, inj),
    );
    s
}

/// 摘要。
pub fn summary(l: &Ledger) -> alloc::string::String {
    format!("rows={} ok={} err={}", l.len(), l.ok_count(), l.err_count())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn full_table_is_contiguous() {
        let c = build_catalog();
        assert_eq!(c.len(), ENTRIES);
        assert!(c.ids_contiguous());
        assert_eq!(c.entry(0).map(|e| e.id), Some(201));
        assert_eq!(c.entry(14).map(|e| e.id), Some(215));
        assert_eq!(c.corpus_mask(), 7);
    }

    #[test]
    fn groups_and_lookups() {
        let c = build_catalog();
        assert_eq!(c.crit_count(TOOL_WC), Ok(3));
        assert_eq!(c.by_id(208).map(|e| e.tool), Ok(TOOL_XARGS));
        assert_eq!(c.entry_id(TOOL_TR, 1), Ok(210));
        assert_eq!(c.entry_id(TOOL_WC, 3), Ok(215));
    }

    #[test]
    fn add_rejects_tool_and_order() {
        let mut c = Catalog::new();
        assert_eq!(
            c.add(Entry { tool: 0, id: 201, corpus: 1, crit: 1 }),
            Err(FC_ETOOL)
        );
        assert_eq!(
            c.add(Entry { tool: TOOL_DD, id: 205, corpus: 1, crit: 1 }),
            Err(FC_EORDI)
        );
        assert_eq!(
            c.add(Entry { tool: TOOL_ECHO, id: 201, corpus: 1, crit: 1 }),
            Ok(0)
        );
    }

    #[test]
    fn add_rejects_duplicates_first() {
        let mut c = Catalog::new();
        assert_eq!(
            c.add(Entry { tool: TOOL_ECHO, id: 201, corpus: 1, crit: 1 }),
            Ok(0)
        );
        assert_eq!(
            c.add(Entry { tool: TOOL_ECHO, id: 201, corpus: 2, crit: 2 }),
            Err(FC_EDUPI)
        );
    }

    #[test]
    fn dimensions_and_capacity() {
        assert_eq!(
            Catalog::new().add(Entry { tool: TOOL_ECHO, id: 201, corpus: 1, crit: 0 }),
            Err(FC_ECRIT)
        );
        assert_eq!(
            Catalog::new().add(Entry { tool: TOOL_ECHO, id: 201, corpus: 0, crit: 1 }),
            Err(FC_ECORP)
        );
        let mut c = Catalog::with_cap(1);
        assert_eq!(
            c.add(Entry { tool: TOOL_ECHO, id: 201, corpus: 1, crit: 1 }),
            Ok(0)
        );
        assert_eq!(
            c.add(Entry { tool: TOOL_ECHO, id: 202, corpus: 1, crit: 2 }),
            Err(FC_EFULL)
        );
    }

    #[test]
    fn ledger_and_checks() {
        let l = build_ledger();
        assert_eq!(l.len(), SAMPLES);
        assert_eq!(l.ok_count(), 8);
        assert!(l.audit().is_ok());
        let s = run_unx11496_checks();
        assert_eq!(s.len(), 13);
        assert!(s.all_passed(), "{}", s.red_items_text());
        assert_eq!(inject_four(), [true; 4]);
    }
}
