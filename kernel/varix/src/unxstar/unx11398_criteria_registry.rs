//! UNX-F11398 三工具判据入册登记（0xeee0 段）
//!
//! 判据要能被机器复核，第一步不是跑它，而是**把它登记进册**：每条判据必须
//! 说明自己属于哪个工具、来自语料清单里的哪一条、该语料是第几版、内容摘要
//! 是什么、可溯源到哪张源卡（本域统一挂 `11377`）。缺任何一列，判据就只是
//! 一句口头承诺。
//!
//! 本模块把「入册」做成一个带闸门的登记处：
//!
//! - `Manifest` 是语料清单。每条语料记录 `(tag, rev, tool)`，`tag` 是语料
//!   编号，`rev` 是版本戳，`tool` 是这份语料服务的工具（ls/cp/rm）。清单可以
//!   自检：版本戳为 0、工具列非法、编号重复、条数越界，都判 `EMANIF`——这是
//!   **清单本身坏了**，与「判据引用了不存在的语料」(`ECORP`) 不同；
//! - `Reg` 是登记处。`reg` 按次序检查：编号与溯源列的形状 → 工具 → 语料在
//!   册 → 语料与判据同源 → 版本戳一致 → 摘要非零 → 编号未重复 → 容量。
//!
//! 错误类为何不能合并：
//!
//! - `EINVAL`：判据编号或溯源卡号为 0。这是**空主键**，一入册就无法与任何
//!   东西对齐；
//! - `ETOOL`：工具码不在 0..=2。判据不知该归到哪一栏；
//! - `ECORP`：语料清单里没有这个 tag，或该 tag 记的工具与判据声明的工具不同。
//!   两种子情形同属「来源对不上」，处理方式都是回去补/改清单，而不是改判据；
//! - `EREV`：语料在册但版本戳与判据声明的不一致。这说明判据是照着旧语料写
//!   的，必须重新核对内容——绝不能用「取最新版」糊过去；
//! - `EDIGEST`：内容摘要为 0。摘要没算就等于没有内容指纹；
//! - `EDUP`：同一编号二次入册。重复入册会让「本批多少条」失去意义；
//! - `EFULL`：登记表满（12 条）。容量是硬边界，不能悄悄扩容；
//! - `EMANIF`：清单自检失败。它只由 `intact`/`trace` 报出，因为清单是共享
//!   底座，坏了要先修底座。
//!
//! 闸门次序（`reg`）：形状 → 工具 → 语料在册 → 同源 → 版本 → 摘要 → 重复
//! → 容量。次序即判据：编号为 0 且工具非法必须报 `EINVAL`；语料缺失且版本
//! 对不上必须报 `ECORP`；摘要为 0 且编号已存在必须报 `EDIGEST`。先报的错
//! 是**更要先修**的错。

use crate::alloc::format;
use crate::checks::CheckSet;

/// 段基址。
pub const SEG_BASE: u32 = 0xeee0;

/// 判据编号或溯源卡号为 0。
pub const RG_EINVAL: u32 = SEG_BASE + 0x01;
/// 工具码越界。
pub const RG_ETOOL: u32 = SEG_BASE + 0x02;
/// 语料未在册或与判据不同源。
pub const RG_ECORP: u32 = SEG_BASE + 0x03;
/// 语料版本戳与判据不一致。
pub const RG_EREV: u32 = SEG_BASE + 0x04;
/// 判据编号重复。
pub const RG_EDUP: u32 = SEG_BASE + 0x05;
/// 登记表已满。
pub const RG_EFULL: u32 = SEG_BASE + 0x06;
/// 内容摘要为 0。
pub const RG_EDIGEST: u32 = SEG_BASE + 0x07;
/// 语料清单自检失败。
pub const RG_EMANIF: u32 = SEG_BASE + 0x08;
/// 账本形状不符。
pub const RG_EBAL: u32 = SEG_BASE + 0x09;
/// 槽位乱序。
pub const RG_ESLOT: u32 = SEG_BASE + 0x0a;
/// 观测值与复算不符。
pub const RG_EVALUE: u32 = SEG_BASE + 0x0b;
/// 冻结后重放不一致。
pub const RG_ESEAL: u32 = SEG_BASE + 0x0c;
/// 观察下标越界。
pub const RG_ENOBS: u32 = SEG_BASE + 0x0d;

/// 工具：ls。
pub const LS: u8 = 0;
/// 工具：cp。
pub const CP: u8 = 1;
/// 工具：rm。
pub const RM: u8 = 2;

/// 语料清单容量。
pub const MCAP: usize = 6;
/// 登记表容量（即 `CAP`）。
pub const CAP: usize = 12;
/// 账本格数。
pub const SAMPLES: usize = 14;
/// 本域统一溯源卡号。
pub const TRACE_CARD: u32 = 11377;
/// 语料版本戳。
pub const REV: u16 = 3;

/// 操作码：入册。
pub const OP_REG: u8 = 1;
/// 操作码：按工具计数。
pub const OP_COUNT: u8 = 2;
/// 操作码：溯源清点。
pub const OP_TRACE: u8 = 3;
/// 操作码：清单自检。
pub const OP_INTACT: u8 = 4;
/// 操作码：覆盖度。
pub const OP_COVER: u8 = 5;

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

/// 一条语料。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Corp {
    /// 语料编号。
    pub tag: u32,
    /// 版本戳。
    pub rev: u16,
    /// 服务的工具。
    pub tool: u8,
    /// 是否在册。
    pub live: bool,
}

impl Corp {
    /// 空槽。
    pub const fn blank() -> Corp {
        Corp { tag: 0, rev: 0, tool: 0, live: false }
    }
}

/// 语料清单。
#[derive(Clone, Copy, Debug)]
pub struct Manifest {
    e: [Corp; MCAP],
    n: usize,
}

impl Manifest {
    /// 空清单。
    pub const fn new() -> Manifest {
        Manifest { e: [Corp::blank(); MCAP], n: 0 }
    }

    /// 收录一条语料。闸门：编号 → 工具 → 重复 → 容量。
    pub fn add(&mut self, tag: u32, rev: u16, tool: u8) -> Result<usize, u32> {
        if tag == 0 {
            return Err(RG_EINVAL);
        }
        if tool != LS && tool != CP && tool != RM {
            return Err(RG_ETOOL);
        }
        if self.find(tag).is_some() {
            return Err(RG_EDUP);
        }
        if self.n >= MCAP {
            return Err(RG_EFULL);
        }
        if let Some(dst) = self.e.get_mut(self.n) {
            *dst = Corp { tag, rev, tool, live: true };
        }
        self.n += 1;
        Ok(self.n)
    }

    /// 条数。
    pub fn len(&self) -> usize {
        self.n
    }

    /// 是否空清单。
    pub fn is_empty(&self) -> bool {
        self.n == 0
    }

    /// 按编号查找。
    pub fn find(&self, tag: u32) -> Option<Corp> {
        let mut i = 0usize;
        while i < self.n {
            if let Some(c) = self.e.get(i) {
                if c.live && c.tag == tag {
                    return Some(*c);
                }
            }
            i += 1;
        }
        None
    }

    /// 清单自检：编号非零、版本非零、工具合法、编号不重复、条数不越界。
    pub fn intact(&self) -> Result<usize, u32> {
        if self.n > MCAP {
            return Err(RG_EMANIF);
        }
        let mut i = 0usize;
        while i < self.n {
            let c = match self.e.get(i) {
                Some(v) => *v,
                None => return Err(RG_EMANIF),
            };
            if !c.live || c.tag == 0 || c.rev == 0 {
                return Err(RG_EMANIF);
            }
            if c.tool != LS && c.tool != CP && c.tool != RM {
                return Err(RG_EMANIF);
            }
            let mut j = i + 1;
            while j < self.n {
                if let Some(d) = self.e.get(j) {
                    if d.live && d.tag == c.tag {
                        return Err(RG_EMANIF);
                    }
                }
                j += 1;
            }
            i += 1;
        }
        Ok(self.n)
    }

    /// 每个工具各有几条语料。越界工具判 `ETOOL`。
    pub fn tool_count(&self, tool: u8) -> Result<u32, u32> {
        if tool != LS && tool != CP && tool != RM {
            return Err(RG_ETOOL);
        }
        let mut n = 0u32;
        let mut i = 0usize;
        while i < self.n {
            if let Some(c) = self.e.get(i) {
                if c.live && c.tool == tool {
                    n += 1;
                }
            }
            i += 1;
        }
        Ok(n)
    }
}

/// 预置清单：ls 两条、cp 两条、rm 两条，统一版本 3。
pub fn seed_manifest() -> Manifest {
    let mut m = Manifest::new();
    let _ = m.add(100, REV, LS);
    let _ = m.add(101, REV, LS);
    let _ = m.add(102, REV, CP);
    let _ = m.add(103, REV, CP);
    let _ = m.add(104, REV, RM);
    let _ = m.add(105, REV, RM);
    m
}

/// 一条已登记的判据。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Crit {
    /// 判据编号。
    pub id: u16,
    /// 工具。
    pub tool: u8,
    /// 引自哪条语料。
    pub corpus: u32,
    /// 声明的语料版本。
    pub rev: u16,
    /// 内容摘要。
    pub digest: u32,
    /// 溯源卡号。
    pub tag: u32,
    /// 是否在册。
    pub live: bool,
}

impl Crit {
    /// 空槽。
    pub const fn blank() -> Crit {
        Crit { id: 0, tool: 0, corpus: 0, rev: 0, digest: 0, tag: 0, live: false }
    }
}

/// 判据登记处。
#[derive(Clone, Copy, Debug)]
pub struct Reg {
    c: [Crit; CAP],
    n: usize,
    m: Manifest,
    rejects: u32,
}

impl Reg {
    /// 空登记处，挂预置清单。
    pub fn new() -> Reg {
        Reg { c: [Crit::blank(); CAP], n: 0, m: seed_manifest(), rejects: 0 }
    }

    /// 挂一份给定清单的登记处。
    pub fn with_manifest(m: Manifest) -> Reg {
        Reg { c: [Crit::blank(); CAP], n: 0, m, rejects: 0 }
    }

    /// 在册条数。
    pub fn live(&self) -> usize {
        let mut k = 0usize;
        let mut i = 0usize;
        while i < self.n {
            if let Some(c) = self.c.get(i) {
                if c.live {
                    k += 1;
                }
            }
            i += 1;
        }
        k
    }

    /// 拒绝计数。
    pub fn rejects(&self) -> u32 {
        self.rejects
    }

    /// 清单。
    pub fn manifest(&self) -> Manifest {
        self.m
    }

    /// 按编号找判据。
    pub fn find(&self, id: u16) -> Option<Crit> {
        let mut i = 0usize;
        while i < self.n {
            if let Some(c) = self.c.get(i) {
                if c.live && c.id == id {
                    return Some(*c);
                }
            }
            i += 1;
        }
        None
    }

    /// 入册。闸门：形状 → 工具 → 语料在册 → 同源 → 版本 → 摘要 → 重复 → 容量。
    pub fn reg(
        &mut self,
        id: u16,
        tool: u8,
        corpus: u32,
        rev: u16,
        digest: u32,
        tag: u32,
    ) -> Result<usize, u32> {
        let bad = self.reg_inner(id, tool, corpus, rev, digest, tag);
        if bad.is_err() {
            self.rejects = self.rejects.saturating_add(1);
        }
        bad
    }

    fn reg_inner(
        &mut self,
        id: u16,
        tool: u8,
        corpus: u32,
        rev: u16,
        digest: u32,
        tag: u32,
    ) -> Result<usize, u32> {
        if id == 0 || tag == 0 {
            return Err(RG_EINVAL);
        }
        if tool != LS && tool != CP && tool != RM {
            return Err(RG_ETOOL);
        }
        let corp = match self.m.find(corpus) {
            Some(c) => c,
            None => return Err(RG_ECORP),
        };
        if corp.tool != tool {
            return Err(RG_ECORP);
        }
        if rev == 0 || corp.rev != rev {
            return Err(RG_EREV);
        }
        if digest == 0 {
            return Err(RG_EDIGEST);
        }
        if self.find(id).is_some() {
            return Err(RG_EDUP);
        }
        if self.n >= CAP {
            return Err(RG_EFULL);
        }
        if let Some(dst) = self.c.get_mut(self.n) {
            *dst = Crit { id, tool, corpus, rev, digest, tag, live: true };
        }
        self.n += 1;
        Ok(self.live())
    }

    /// 按工具计数。
    pub fn count(&self, tool: u8) -> Result<u32, u32> {
        if tool != LS && tool != CP && tool != RM {
            return Err(RG_ETOOL);
        }
        let mut n = 0u32;
        let mut i = 0usize;
        while i < self.n {
            if let Some(c) = self.c.get(i) {
                if c.live && c.tool == tool {
                    n += 1;
                }
            }
            i += 1;
        }
        Ok(n)
    }

    /// 覆盖度：三个工具各至少三条，返回最小覆盖数。
    pub fn full_cover(&self) -> Result<u32, u32> {
        let mut min = u32::MAX;
        let mut t = 0u8;
        while t < 3 {
            let n = self.count(t)?;
            if n < 3 {
                return Err(RG_ECORP);
            }
            if n < min {
                min = n;
            }
            t += 1;
        }
        Ok(min)
    }

    /// 溯源清点：每条在册判据都必须挂本域源卡，且其语料仍在册。
    pub fn trace(&self) -> Result<u32, u32> {
        let mut n = 0u32;
        let mut i = 0usize;
        while i < self.n {
            let c = match self.c.get(i) {
                Some(v) => *v,
                None => return Err(RG_EMANIF),
            };
            if c.live {
                if c.tag != TRACE_CARD {
                    return Err(RG_EMANIF);
                }
                if self.m.find(c.corpus).is_none() {
                    return Err(RG_EMANIF);
                }
                n += 1;
            }
            i += 1;
        }
        Ok(n)
    }

    /// 清单自检的代入口。
    pub fn intact(&self) -> Result<usize, u32> {
        self.m.intact()
    }

    /// 不变式：条数不越容量、在册编号互不相同、清单自身完整。
    pub fn consistent(&self) -> bool {
        if self.n > CAP || self.intact().is_err() {
            return false;
        }
        let mut i = 0usize;
        while i < self.n {
            let a = match self.c.get(i) {
                Some(v) => *v,
                None => return false,
            };
            if a.live {
                let mut j = i + 1;
                while j < self.n {
                    if let Some(b) = self.c.get(j) {
                        if b.live && b.id == a.id {
                            return false;
                        }
                    }
                    j += 1;
                }
            }
            i += 1;
        }
        true
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

    /// 登记一格。
    pub fn register(&mut self, s: Sample) -> Result<(), u32> {
        if self.frozen || (s.slot as usize) != self.len || (s.slot as usize) >= SAMPLES {
            self.rejects = self.rejects.saturating_add(1);
            return Err(RG_ESLOT);
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
            return Err(RG_EBAL);
        }
        const VALS: [u32; 8] = [1, 2, 3, 4, 5, 6, 2, 6];
        const CODES: [u32; 6] = [RG_EINVAL, RG_ETOOL, RG_ECORP, RG_EREV, RG_EDUP, RG_EDIGEST];
        let mut vi = 0usize;
        let mut ci = 0usize;
        let mut i = 0usize;
        while i < self.len {
            let s = match self.rows.get(i) {
                Some(v) => *v,
                None => return Err(RG_EBAL),
            };
            if s.slot as usize != i {
                return Err(RG_ESLOT);
            }
            if s.ok {
                if s.code != 0 {
                    return Err(RG_EVALUE);
                }
                match VALS.get(vi) {
                    Some(w) => {
                        if s.value != *w {
                            return Err(RG_EVALUE);
                        }
                    }
                    None => return Err(RG_EBAL),
                }
                vi += 1;
            } else {
                match CODES.get(ci) {
                    Some(w) => {
                        if s.code != *w {
                            return Err(RG_EVALUE);
                        }
                    }
                    None => return Err(RG_EBAL),
                }
                ci += 1;
            }
            i += 1;
        }
        if vi != 8 || ci != 6 {
            return Err(RG_EBAL);
        }
        Ok(())
    }

    /// 指纹（只折内容行）。
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
            return Err(RG_EBAL);
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
            Err(RG_ESEAL)
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

/// 入册一批三工具判据。工具与语料严格同源。
pub fn seed_registry() -> Reg {
    let mut r = Reg::new();
    let _ = r.reg(1, LS, 100, REV, 0xa1, TRACE_CARD);
    let _ = r.reg(2, LS, 101, REV, 0xa2, TRACE_CARD);
    let _ = r.reg(3, CP, 102, REV, 0xa3, TRACE_CARD);
    let _ = r.reg(4, CP, 103, REV, 0xa4, TRACE_CARD);
    let _ = r.reg(5, RM, 104, REV, 0xa5, TRACE_CARD);
    let _ = r.reg(6, RM, 105, REV, 0xa6, TRACE_CARD);
    r
}

/// 十四格账本：六条入册 + 计数 + 溯源 + 六类拒绝。
pub fn build_ledger() -> Ledger {
    let mut l = Ledger::new();
    let mut r = Reg::new();

    let _ = l.register(cell(0, OP_REG, r.reg(1, LS, 100, REV, 0xa1, TRACE_CARD)));
    let _ = l.register(cell(1, OP_REG, r.reg(2, LS, 101, REV, 0xa2, TRACE_CARD)));
    let _ = l.register(cell(2, OP_REG, r.reg(3, CP, 102, REV, 0xa3, TRACE_CARD)));
    let _ = l.register(cell(3, OP_REG, r.reg(4, CP, 103, REV, 0xa4, TRACE_CARD)));
    let _ = l.register(cell(4, OP_REG, r.reg(5, RM, 104, REV, 0xa5, TRACE_CARD)));
    let _ = l.register(cell(5, OP_REG, r.reg(6, RM, 105, REV, 0xa6, TRACE_CARD)));
    let _ = l.register(word_cell(6, OP_COUNT, r.count(LS)));
    let _ = l.register(word_cell(7, OP_TRACE, r.trace()));

    let mut a = Reg::new();
    let _ = l.register(cell(8, OP_REG, a.reg(0, LS, 100, REV, 1, TRACE_CARD)));

    let mut b = Reg::new();
    let _ = l.register(cell(9, OP_REG, b.reg(1, 9, 100, REV, 1, TRACE_CARD)));

    let mut c = Reg::new();
    let _ = l.register(cell(10, OP_REG, c.reg(1, LS, 999, REV, 1, TRACE_CARD)));

    let mut d = Reg::new();
    let _ = l.register(cell(11, OP_REG, d.reg(1, LS, 100, 4, 1, TRACE_CARD)));

    let _ = l.register(cell(12, OP_REG, r.reg(1, LS, 100, REV, 1, TRACE_CARD)));

    let mut e = Reg::new();
    let _ = l.register(cell(13, OP_REG, e.reg(7, LS, 100, REV, 0, TRACE_CARD)));

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

    // 二路：三工具各三条，覆盖度、溯源、清单自检全绿。
    let r = seed_registry();
    out[1] = r.count(LS) == Ok(2)
        && r.count(CP) == Ok(2)
        && r.count(RM) == Ok(2)
        && r.live() == 6
        && r.intact() == Ok(MCAP)
        && r.trace() == Ok(6)
        && r.full_cover() == Err(RG_ECORP)
        && {
            let mut f = seed_registry();
            let _ = f.reg(7, LS, 100, REV, 0xb1, TRACE_CARD);
            let _ = f.reg(8, CP, 102, REV, 0xb2, TRACE_CARD);
            let _ = f.reg(9, RM, 104, REV, 0xb3, TRACE_CARD);
            f.full_cover() == Ok(3)
                && f.count(LS) == Ok(3)
                && f.count(CP) == Ok(3)
                && f.count(RM) == Ok(3)
                && f.trace() == Ok(9)
                && f.consistent()
        }
        && r.consistent();

    // 三路：六类拒绝彼此可辨。
    let mut a = Reg::new();
    let p = a.reg(0, LS, 100, REV, 1, TRACE_CARD) == Err(RG_EINVAL);
    let b = a.reg(1, 7, 100, REV, 1, TRACE_CARD) == Err(RG_ETOOL);
    let c = a.reg(1, LS, 999, REV, 1, TRACE_CARD) == Err(RG_ECORP);
    let d = a.reg(1, LS, 100, 9, 1, TRACE_CARD) == Err(RG_EREV);
    let e = a.reg(1, LS, 100, REV, 0, TRACE_CARD) == Err(RG_EDIGEST);
    let _ = a.reg(1, LS, 100, REV, 1, TRACE_CARD);
    let f = a.reg(1, LS, 100, REV, 1, TRACE_CARD) == Err(RG_EDUP);
    let g = a.intact() == Ok(MCAP) && a.live() == 1 && a.consistent();
    out[2] = p && b && c && d && e && f && g;

    // 四路：闸门次序、容量边界与清单自检。
    let mut q = Reg::new();
    let g1 = q.reg(0, 9, 999, 4, 0, TRACE_CARD) == Err(RG_EINVAL);
    let g2 = q.reg(1, 9, 999, 4, 0, TRACE_CARD) == Err(RG_ETOOL);
    let g3 = q.reg(1, LS, 999, 4, 0, TRACE_CARD) == Err(RG_ECORP);
    let g4 = q.reg(1, LS, 100, 4, 0, TRACE_CARD) == Err(RG_EREV);
    let g5 = q.reg(1, LS, 100, REV, 0, TRACE_CARD) == Err(RG_EDIGEST);
    let mut full = Reg::new();
    let mut i = 0u32;
    while (full.live() as usize) < CAP {
        let corpus = 100 + (i % 6);
        let tool = if i % 6 < 2 {
            LS
        } else if i % 6 < 4 {
            CP
        } else {
            RM
        };
        let _ = full.reg(1 + i as u16, tool, corpus, REV, i + 1, TRACE_CARD);
        i += 1;
    }
    let g6 = full.reg(120, LS, 100, REV, 9, TRACE_CARD) == Err(RG_EFULL);
    let mut bad = Manifest::new();
    let _ = bad.add(300, 0, LS);
    let g7 = bad.intact() == Err(RG_EMANIF) && q.consistent() && full.consistent();
    out[3] = g1 && g2 && g3 && g4 && g5 && g6 && g7;

    out
}

/// 十三项判据。
pub fn run_unx11398_checks() -> CheckSet {
    let mut s = CheckSet::new("unx11398.criteria_registry");
    let l = build_ledger();

    s.add_dyn(
        "RG1-十四格八允六拒",
        l.len() == SAMPLES && l.ok_count() == 8 && l.err_count() == 6 && l.audit().is_ok(),
        &format!("n={} ok={} err={}", l.len(), l.ok_count(), l.err_count()),
    );

    s.add(
        "RG2-六条入册递增",
        l.sample(0).map(|x| x.value) == Some(1)
            && l.sample(1).map(|x| x.value) == Some(2)
            && l.sample(2).map(|x| x.value) == Some(3)
            && l.sample(3).map(|x| x.value) == Some(4)
            && l.sample(4).map(|x| x.value) == Some(5)
            && l.sample(5).map(|x| x.value) == Some(6),
        "static",
    );

    s.add(
        "RG3-每工具计数正确",
        l.sample(6).map(|x| x.value) == Some(2)
            && {
                let r = seed_registry();
                r.count(LS) == Ok(2) && r.count(CP) == Ok(2) && r.count(RM) == Ok(2)
            },
        "static",
    );

    s.add(
        "RG4-清单字段齐备",
        {
            let m = seed_manifest();
            m.intact() == Ok(MCAP)
                && m.len() == MCAP
                && m.tool_count(LS) == Ok(2)
                && m.tool_count(CP) == Ok(2)
                && m.tool_count(RM) == Ok(2)
                && m.find(100).map(|c| c.rev) == Some(REV)
                && m.find(105).map(|c| c.tool) == Some(RM)
                && m.find(106).is_none()
        },
        "static",
    );

    s.add(
        "RG5-溯源全命中11377",
        l.sample(7).map(|x| x.value) == Some(6)
            && {
                let mut r = seed_registry();
                let _ = r.reg(7, LS, 100, REV, 0xb1, 9999);
                r.trace() == Err(RG_EMANIF)
                    && {
                        let ok = r.find(7).map(|c| c.tag) == Some(9999);
                        ok && seed_registry().trace() == Ok(6)
                    }
            },
        "static",
    );

    s.add(
        "RG6-空编号EINVAL",
        l.sample(8).map(|x| x.code) == Some(RG_EINVAL)
            && {
                let mut r = Reg::new();
                r.reg(0, LS, 100, REV, 1, TRACE_CARD) == Err(RG_EINVAL)
                    && r.reg(1, LS, 100, REV, 1, 0) == Err(RG_EINVAL)
                    && r.live() == 0
            },
        "static",
    );

    s.add(
        "RG7-越界工具ETOOL",
        l.sample(9).map(|x| x.code) == Some(RG_ETOOL)
            && {
                let r = seed_registry();
                r.count(3) == Err(RG_ETOOL)
                    && r.full_cover().is_err()
                    && {
                        let mut m = Manifest::new();
                        m.add(200, REV, 3) == Err(RG_ETOOL)
                    }
            },
        "static",
    );

    s.add(
        "RG8-语料缺失ECORP",
        l.sample(10).map(|x| x.code) == Some(RG_ECORP)
            && {
                let mut r = Reg::new();
                r.reg(1, LS, 999, REV, 1, TRACE_CARD) == Err(RG_ECORP)
                    && r.reg(2, CP, 100, REV, 1, TRACE_CARD) == Err(RG_ECORP)
                    && r.live() == 0
            },
        "static",
    );

    s.add(
        "RG9-版本不符EREV",
        l.sample(11).map(|x| x.code) == Some(RG_EREV)
            && {
                let mut r = Reg::new();
                r.reg(1, LS, 100, 2, 1, TRACE_CARD) == Err(RG_EREV)
                    && r.reg(1, LS, 100, 4, 1, TRACE_CARD) == Err(RG_EREV)
                    && r.live() == 0
            },
        "static",
    );

    s.add(
        "RG10-重复入册EDUP",
        l.sample(12).map(|x| x.code) == Some(RG_EDUP)
            && {
                let mut r = seed_registry();
                r.reg(1, LS, 100, REV, 0xc1, TRACE_CARD) == Err(RG_EDUP)
                    && r.live() == 6
                    && r.rejects() == 1
            },
        "static",
    );

    s.add(
        "RG11-清单自检EMANIF",
        {
            let mut m = Manifest::new();
            let _ = m.add(300, 0, LS);
            let zero_rev = m.intact() == Err(RG_EMANIF);
            let mut d = Manifest::new();
            let _ = d.add(300, REV, LS);
            let dupish = d.intact() == Ok(1);
            zero_rev && dupish && {
                let mut r = Reg::with_manifest(m);
                r.reg(1, LS, 300, 0, 1, TRACE_CARD) == Err(RG_EREV)
            }
        },
        "static",
    );

    s.add(
        "RG12-三工具各三与容量",
        {
            let mut f = seed_registry();
            let _ = f.reg(7, LS, 100, REV, 0xb1, TRACE_CARD);
            let _ = f.reg(8, CP, 102, REV, 0xb2, TRACE_CARD);
            let _ = f.reg(9, RM, 104, REV, 0xb3, TRACE_CARD);
            let cover = f.full_cover() == Ok(3);
            let mut g = Reg::new();
            let mut i = 0u32;
            while (g.live() as usize) < CAP {
                let corpus = 100 + (i % 6);
                let tool = if i % 6 < 2 {
                    LS
                } else if i % 6 < 4 {
                    CP
                } else {
                    RM
                };
                let _ = g.reg(1 + i as u16, tool, corpus, REV, i + 1, TRACE_CARD);
                i += 1;
            }
            cover
                && g.live() == CAP
                && g.reg(120, LS, 100, REV, 9, TRACE_CARD) == Err(RG_EFULL)
                && g.consistent()
                && f.consistent()
        },
        "static",
    );

    s.add(
        "RG13-冻结重放四注入",
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
        let s = run_unx11398_checks();
        assert_eq!(s.len(), 13);
        assert!(s.all_passed());
    }

    #[test]
    fn registry_registers_six() {
        let r = seed_registry();
        assert_eq!(r.live(), 6);
        assert_eq!(r.count(LS), Ok(2));
        assert_eq!(r.count(CP), Ok(2));
        assert_eq!(r.count(RM), Ok(2));
        assert_eq!(r.intact(), Ok(MCAP));
        assert!(r.consistent());
    }

    #[test]
    fn manifest_columns_are_complete() {
        let m = seed_manifest();
        assert_eq!(m.intact(), Ok(6));
        assert_eq!(m.find(100).map(|c| (c.rev, c.tool)), Some((REV, LS)));
        assert_eq!(m.find(105).map(|c| (c.rev, c.tool)), Some((REV, RM)));
        assert!(m.find(999).is_none());
    }

    #[test]
    fn gates_follow_documented_order() {
        let mut r = Reg::new();
        assert_eq!(r.reg(0, 9, 999, 4, 0, TRACE_CARD), Err(RG_EINVAL));
        assert_eq!(r.reg(1, 9, 999, 4, 0, TRACE_CARD), Err(RG_ETOOL));
        assert_eq!(r.reg(1, LS, 999, 4, 0, TRACE_CARD), Err(RG_ECORP));
        assert_eq!(r.reg(1, CP, 100, 4, 0, TRACE_CARD), Err(RG_ECORP));
        assert_eq!(r.reg(1, LS, 100, 4, 0, TRACE_CARD), Err(RG_EREV));
        assert_eq!(r.reg(1, LS, 100, REV, 0, TRACE_CARD), Err(RG_EDIGEST));
        assert_eq!(r.reg(1, LS, 100, REV, 1, TRACE_CARD), Ok(1));
        assert_eq!(r.reg(1, LS, 100, REV, 1, TRACE_CARD), Err(RG_EDUP));
    }

    #[test]
    fn coverage_needs_three_each() {
        let mut r = seed_registry();
        assert_eq!(r.full_cover(), Err(RG_ECORP));
        let _ = r.reg(7, LS, 100, REV, 0xb1, TRACE_CARD);
        let _ = r.reg(8, CP, 102, REV, 0xb2, TRACE_CARD);
        let _ = r.reg(9, RM, 104, REV, 0xb3, TRACE_CARD);
        assert_eq!(r.full_cover(), Ok(3));
        assert_eq!(r.trace(), Ok(9));
    }

    #[test]
    fn capacity_is_hard() {
        let mut r = Reg::new();
        let mut i = 0u32;
        while (r.live() as usize) < CAP {
            let corpus = 100 + (i % 6);
            let tool = if i % 6 < 2 {
                LS
            } else if i % 6 < 4 {
                CP
            } else {
                RM
            };
            assert!(r.reg(1 + i as u16, tool, corpus, REV, i + 1, TRACE_CARD).is_ok());
            i += 1;
        }
        assert_eq!(r.reg(120, LS, 100, REV, 9, TRACE_CARD), Err(RG_EFULL));
        assert!(r.consistent());
    }

    #[test]
    fn inject_four_is_all_true() {
        assert_eq!(inject_four(), [true; 4]);
    }
}
