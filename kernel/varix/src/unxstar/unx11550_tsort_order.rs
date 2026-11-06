//! UNX-F11550 tsort 拓扑排序
//!
//! `tsort` 的输入是一串**先后约束**：每两个记号构成一条边「前者必须先于后者」。
//! 输出是满足全部约束的一个全序。三条语义决定了它能不能被信任：
//!
//! 1. **词数为奇必须拒绝**。最后一个落单的记号不是「没有后继」，而是输入被截断，
//!    静默忽略它会把「约束缺失」变成「约束满足」。
//! 2. **有环必须报错**，不能输出一个勉强凑出来的顺序。环意味着约束互相矛盾，
//!    任何输出都必然违反其中一条边；此时唯一诚实的行为是失败。
//! 3. **顺序必须确定**。Kahn 算法每一步可能有多个入度为零的节点，本实现固定选
//!    「节点编号最小者」——编号来自首次出现的次序，于是同一个输入永远给出同一个
//!    顺序，账本才可重放。
//!
//! 边是集合而不是多重集：重复边只记一次并单独计数。同一条约束说两遍不改变语义，
//! 但它是一个值得观测的信号（生成器把约束重复输出了），所以计进 `dups` 而不是
//! 直接丢弃。
//!
//! 自环（`a a`）在插入时就被判为环：它自己就是一条无法满足的约束，没必要等到
//! 排序阶段才发现。
//!
//! 排序是**一次性**的：成功后再排序返回状态错误。理由与 `tsort` 的实现方式一致
//! ——排序会消耗图（Kahn 每步删边），第二次排序看到的是一张已经被掏空的图。
//!
//! 闸门次序：
//!
//! - `feed`：记号数超上限 → 词数为奇 → 逐对登记（自环 → 环）；
//! - `intern`：名字为空 → 名字超长 → 非法字节 → 表满；
//! - `sort`：已排序 → 无零入度节点（环）；
//! - `position` / `render`：未排序 → 名字不在表中 → 缓冲超限。

use crate::alloc::string::String;
use crate::checks::CheckSet;

/// 段基址。
pub const SEG_BASE: u32 = 0xf370;

/// 词数为奇数。
pub const TS_ETOK: u32 = SEG_BASE + 0x01;
/// 输入含环。
pub const TS_ECYC: u32 = SEG_BASE + 0x02;
/// 名字为空或超长。
pub const TS_ENAME: u32 = SEG_BASE + 0x03;
/// 节点或边表满。
pub const TS_EFULL: u32 = SEG_BASE + 0x04;
/// 状态不符。
pub const TS_ESTATE: u32 = SEG_BASE + 0x05;
/// 非法字节。
pub const TS_EBYTE: u32 = SEG_BASE + 0x06;
/// 名字不在表中。
pub const TS_EINDEX: u32 = SEG_BASE + 0x07;
/// 渲染缓冲超限。
pub const TS_EFMT: u32 = SEG_BASE + 0x08;
/// 账本形状不符。
pub const TS_EBAL: u32 = SEG_BASE + 0x09;
/// 槽位乱序。
pub const TS_ESLOT: u32 = SEG_BASE + 0x0a;
/// 观测值与复算不符。
pub const TS_EVALUE: u32 = SEG_BASE + 0x0b;
/// 冻结后重放不一致。
pub const TS_ESEAL: u32 = SEG_BASE + 0x0c;
/// 观察下标越界。
pub const TS_ENOBS: u32 = SEG_BASE + 0x0d;

/// 节点表容量。
pub const NODECAP: usize = 16;
/// 边表容量。
pub const ECAP: usize = 32;
/// 名字上限。
pub const NAMEMAX: usize = 8;
/// 单次喂入的记号上限。
pub const TOKMAX: usize = 32;
/// 渲染文本上限。
pub const BUFLEN: usize = 64;
/// 账本格数。
pub const SAMPLES: usize = 14;

/// 操作码：喂入约束。
pub const OP_FEED: u8 = 1;
/// 操作码：排序。
pub const OP_SORT: u8 = 2;
/// 操作码：查名次。
pub const OP_POS: u8 = 3;
/// 操作码：渲染。
pub const OP_RENDER: u8 = 4;
/// 操作码：统计。
pub const OP_STAT: u8 = 5;

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

/// 是否空白（约束文本的分隔符）。
pub fn is_space(c: u8) -> bool {
    c == b' ' || c == b'\t' || c == b'\n' || c == b'\r' || c == 0x0b || c == 0x0c
}

/// 是否可打印 ASCII。
pub fn is_printable(c: u8) -> bool {
    c >= 0x20 && c <= 0x7e
}

/// 符号表：名字 → 编号，编号即首次出现次序。
#[derive(Clone, Copy, Debug)]
pub struct Symtab {
    names: [[u8; NAMEMAX]; NODECAP],
    lens: [u8; NODECAP],
    n: usize,
}

impl Symtab {
    /// 空表。
    pub const fn new() -> Symtab {
        Symtab { names: [[0; NAMEMAX]; NODECAP], lens: [0; NODECAP], n: 0 }
    }

    /// 节点数。
    pub fn len(&self) -> usize {
        self.n
    }

    /// 是否空表。
    pub fn is_empty(&self) -> bool {
        self.n == 0
    }

    /// 第 i 个名字。
    pub fn get(&self, i: usize) -> Option<&[u8]> {
        let len = match self.lens.get(i) {
            Some(v) => *v as usize,
            None => return None,
        };
        let arr = self.names.get(i)?;
        arr.get(..len)
    }

    /// 第 i 个名字是否等于 `other`。
    pub fn name_is(&self, i: usize, other: &[u8]) -> Option<bool> {
        match self.get(i) {
            Some(v) => Some(v == other),
            None => None,
        }
    }

    /// 查编号。
    pub fn index_of(&self, s: &[u8]) -> Option<usize> {
        let mut i = 0usize;
        while i < self.n {
            if self.name_is(i, s) == Some(true) {
                return Some(i);
            }
            i += 1;
        }
        None
    }

    /// 登记名字，已存在则返回原编号。
    pub fn intern(&mut self, s: &[u8]) -> Result<u32, u32> {
        if s.is_empty() || s.len() > NAMEMAX {
            return Err(TS_ENAME);
        }
        let mut k = 0usize;
        while k < s.len() {
            match s.get(k) {
                Some(c) => {
                    if !is_printable(*c) || is_space(*c) {
                        return Err(TS_EBYTE);
                    }
                }
                None => return Err(TS_EBYTE),
            }
            k += 1;
        }
        if let Some(i) = self.index_of(s) {
            return Ok(i as u32);
        }
        if self.n >= NODECAP {
            return Err(TS_EFULL);
        }
        if let Some(arr) = self.names.get_mut(self.n) {
            let mut j = 0usize;
            while j < s.len() {
                match s.get(j) {
                    Some(c) => {
                        if let Some(d) = arr.get_mut(j) {
                            *d = *c;
                        }
                    }
                    None => break,
                }
                j += 1;
            }
        }
        if let Some(d) = self.lens.get_mut(self.n) {
            *d = s.len() as u8;
        }
        self.n += 1;
        Ok((self.n - 1) as u32)
    }
}

/// 一条先后约束。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Edge {
    /// 前驱编号。
    pub from: u32,
    /// 后继编号。
    pub to: u32,
}

/// 约束图。
#[derive(Clone, Copy, Debug)]
pub struct Graph {
    sym: Symtab,
    edges: [Edge; ECAP],
    m: usize,
    dups: u32,
    rejects: u32,
    order: [u32; NODECAP],
    on: usize,
    sorted: bool,
    cycle: u32,
}

impl Graph {
    /// 空图。
    pub const fn new() -> Graph {
        Graph {
            sym: Symtab::new(),
            edges: [Edge { from: 0, to: 0 }; ECAP],
            m: 0,
            dups: 0,
            rejects: 0,
            order: [0; NODECAP],
            on: 0,
            sorted: false,
            cycle: 0,
        }
    }

    /// 节点数。
    pub fn node_count(&self) -> usize {
        self.sym.len()
    }

    /// 边数（去重后）。
    pub fn edge_count(&self) -> usize {
        self.m
    }

    /// 重复边计数。
    pub fn dups(&self) -> u32 {
        self.dups
    }

    /// 拒绝计数。
    pub fn rejects(&self) -> u32 {
        self.rejects
    }

    /// 环中残留节点数（排序失败时非零）。
    pub fn cycle(&self) -> u32 {
        self.cycle
    }

    /// 是否已排序。
    pub fn is_sorted(&self) -> bool {
        self.sorted
    }

    /// 已输出节点数。
    pub fn emitted(&self) -> usize {
        self.on
    }

    /// 名字表。
    pub fn names(&self) -> Symtab {
        self.sym
    }

    /// 第 i 条边。
    pub fn edge(&self, i: usize) -> Option<Edge> {
        self.edges.get(i).copied()
    }

    /// 登记名字。
    pub fn intern(&mut self, s: &[u8]) -> Result<u32, u32> {
        match self.sym.intern(s) {
            Ok(v) => Ok(v),
            Err(c) => {
                self.rejects = self.rejects.saturating_add(1);
                Err(c)
            }
        }
    }

    /// 查编号。
    pub fn id_of(&self, s: &[u8]) -> Option<u32> {
        match self.sym.index_of(s) {
            Some(i) => Some(i as u32),
            None => None,
        }
    }

    /// 加一条约束。
    pub fn add_edge(&mut self, a: &[u8], b: &[u8]) -> Result<u32, u32> {
        let from = self.intern(a)?;
        let to = self.intern(b)?;
        if from == to {
            self.rejects = self.rejects.saturating_add(1);
            return Err(TS_ECYC);
        }
        let e = Edge { from, to };
        let mut i = 0usize;
        while i < self.m {
            if self.edges.get(i) == Some(&e) {
                self.dups = self.dups.saturating_add(1);
                return Ok(self.m as u32);
            }
            i += 1;
        }
        if self.m >= ECAP {
            self.rejects = self.rejects.saturating_add(1);
            return Err(TS_EFULL);
        }
        if let Some(d) = self.edges.get_mut(self.m) {
            *d = e;
        }
        self.m += 1;
        Ok(self.m as u32)
    }

    /// 喂入一段约束文本。
    ///
    /// 先把记号全部切出来再逐对登记：切分阶段失败（词数为奇、记号过多）时图完全
    /// 不变；登记阶段失败时保留已成功的前缀——这与流式读入的行为一致。
    pub fn feed(&mut self, text: &[u8]) -> Result<u32, u32> {
        let mut toks: [&[u8]; TOKMAX] = [b""; TOKMAX];
        let mut nt = 0usize;
        let mut i = 0usize;
        while i < text.len() {
            let c = match text.get(i) {
                Some(v) => *v,
                None => break,
            };
            if is_space(c) {
                i += 1;
                continue;
            }
            let start = i;
            while i < text.len() {
                match text.get(i) {
                    Some(v) => {
                        if is_space(*v) {
                            break;
                        }
                    }
                    None => break,
                }
                i += 1;
            }
            if nt >= TOKMAX {
                self.rejects = self.rejects.saturating_add(1);
                return Err(TS_EFULL);
            }
            if let Some(d) = toks.get_mut(nt) {
                *d = text.get(start..i).unwrap_or(b"");
            }
            nt += 1;
        }
        if nt % 2 != 0 {
            self.rejects = self.rejects.saturating_add(1);
            return Err(TS_ETOK);
        }
        let mut j = 0usize;
        let mut pairs = 0u32;
        while j + 1 < nt {
            let a = match toks.get(j) {
                Some(v) => *v,
                None => return Err(TS_ETOK),
            };
            let b = match toks.get(j + 1) {
                Some(v) => *v,
                None => return Err(TS_ETOK),
            };
            let _ = self.add_edge(a, b)?;
            pairs += 1;
            j += 2;
        }
        Ok(pairs)
    }

    /// 入度。
    pub fn indeg(&self, id: u32) -> Result<u32, u32> {
        if id as usize >= self.sym.len() {
            return Err(TS_EINDEX);
        }
        let mut d = 0u32;
        let mut i = 0usize;
        while i < self.m {
            if let Some(e) = self.edges.get(i) {
                if e.to == id {
                    d = d.saturating_add(1);
                }
            }
            i += 1;
        }
        Ok(d)
    }

    /// Kahn 拓扑排序。
    pub fn sort(&mut self) -> Result<u32, u32> {
        if self.sorted {
            self.rejects = self.rejects.saturating_add(1);
            return Err(TS_ESTATE);
        }
        let n = self.sym.len();
        let mut indeg = [0u32; NODECAP];
        let mut i = 0usize;
        while i < self.m {
            if let Some(e) = self.edges.get(i) {
                if let Some(d) = indeg.get_mut(e.to as usize) {
                    *d = d.saturating_add(1);
                }
            }
            i += 1;
        }
        let mut done = [false; NODECAP];
        let mut on = 0usize;
        loop {
            let mut pick: Option<usize> = None;
            let mut k = 0usize;
            while k < n {
                let ready = match indeg.get(k) {
                    Some(d) => *d == 0,
                    None => false,
                };
                let taken = match done.get(k) {
                    Some(v) => *v,
                    None => true,
                };
                if ready && !taken {
                    pick = Some(k);
                    break;
                }
                k += 1;
            }
            let node = match pick {
                Some(v) => v,
                None => break,
            };
            if let Some(d) = done.get_mut(node) {
                *d = true;
            }
            if let Some(d) = self.order.get_mut(on) {
                *d = node as u32;
            }
            on += 1;
            let mut j = 0usize;
            while j < self.m {
                if let Some(e) = self.edges.get(j) {
                    if e.from as usize == node {
                        if let Some(d) = indeg.get_mut(e.to as usize) {
                            *d = d.saturating_sub(1);
                        }
                    }
                }
                j += 1;
            }
        }
        self.on = on;
        self.cycle = (n - on) as u32;
        if on < n {
            self.rejects = self.rejects.saturating_add(1);
            return Err(TS_ECYC);
        }
        self.sorted = true;
        Ok(on as u32)
    }

    /// 第 i 名的编号。
    pub fn order_at(&self, i: usize) -> Option<u32> {
        if !self.sorted {
            return None;
        }
        self.order.get(i).copied()
    }

    /// 编号在输出中的名次。
    fn pos_of(&self, id: u32) -> Option<usize> {
        let mut i = 0usize;
        while i < self.on {
            if self.order.get(i) == Some(&id) {
                return Some(i);
            }
            i += 1;
        }
        None
    }

    /// 名字的名次。
    pub fn position(&self, name: &[u8]) -> Result<u32, u32> {
        if !self.sorted {
            return Err(TS_ESTATE);
        }
        let id = match self.sym.index_of(name) {
            Some(v) => v as u32,
            None => return Err(TS_EINDEX),
        };
        match self.pos_of(id) {
            Some(p) => Ok(p as u32),
            None => Err(TS_EINDEX),
        }
    }

    /// 渲染成每行一个名字。
    pub fn render(&self) -> Result<String, u32> {
        if !self.sorted {
            return Err(TS_ESTATE);
        }
        let mut s = String::new();
        let mut i = 0usize;
        while i < self.on {
            let id = match self.order.get(i) {
                Some(v) => *v as usize,
                None => return Err(TS_ESTATE),
            };
            let name = match self.sym.get(id) {
                Some(v) => v,
                None => return Err(TS_EINDEX),
            };
            if i > 0 {
                s.push('\n');
            }
            let mut k = 0usize;
            while k < name.len() {
                match name.get(k) {
                    Some(c) => s.push(*c as char),
                    None => break,
                }
                k += 1;
            }
            i += 1;
        }
        if s.len() > BUFLEN {
            return Err(TS_EFMT);
        }
        Ok(s)
    }

    /// 图自洽：输出是全体节点的排列，且每条边都指向更晚的名次。
    pub fn verify(&self) -> bool {
        if !self.sorted || self.on != self.sym.len() {
            return false;
        }
        let mut seen = [false; NODECAP];
        let mut i = 0usize;
        while i < self.on {
            let id = match self.order.get(i) {
                Some(v) => *v as usize,
                None => return false,
            };
            if id >= NODECAP {
                return false;
            }
            match seen.get_mut(id) {
                Some(v) => {
                    if *v {
                        return false;
                    }
                    *v = true;
                }
                None => return false,
            }
            i += 1;
        }
        let mut j = 0usize;
        while j < self.m {
            let e = match self.edges.get(j) {
                Some(v) => *v,
                None => return false,
            };
            let a = match self.pos_of(e.from) {
                Some(v) => v,
                None => return false,
            };
            let b = match self.pos_of(e.to) {
                Some(v) => v,
                None => return false,
            };
            if a >= b {
                return false;
            }
            j += 1;
        }
        true
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

/// `Result<u32, u32>` → 样本。
fn word_cell(slot: u8, op: u8, r: Result<u32, u32>) -> Sample {
    match r {
        Ok(v) => Sample { slot, op, ok: true, code: 0, value: v },
        Err(c) => Sample { slot, op, ok: false, code: c, value: 0 },
    }
}

/// `Result<usize, u32>` → 样本。
fn count_cell(slot: u8, op: u8, r: Result<usize, u32>) -> Sample {
    match r {
        Ok(v) => Sample { slot, op, ok: true, code: 0, value: v as u32 },
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
            return Err(TS_ESLOT);
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

    /// 是否空账本。
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// 第 i 格。
    pub fn sample(&self, i: usize) -> Option<Sample> {
        self.rows.get(i).copied()
    }

    /// 成功格数。
    pub fn ok_count(&self) -> usize {
        let mut n = 0usize;
        let mut i = 0usize;
        while i < self.len {
            match self.rows.get(i) {
                Some(s) => {
                    if s.ok {
                        n += 1;
                    }
                }
                None => return n,
            }
            i += 1;
        }
        n
    }

    /// 失败格数。
    pub fn err_count(&self) -> usize {
        self.len - self.ok_count()
    }

    /// 审计。
    pub fn audit(&self) -> Result<(), u32> {
        if self.len != SAMPLES {
            return Err(TS_EBAL);
        }
        if self.frozen && self.seal() != self.root {
            return Err(TS_ESEAL);
        }
        let mut vi = 0usize;
        let mut ci = 0usize;
        let mut i = 0usize;
        while i < SAMPLES {
            let s = match self.rows.get(i) {
                Some(v) => *v,
                None => return Err(TS_ENOBS),
            };
            if s.slot as usize != i {
                return Err(TS_ESLOT);
            }
            if s.op == 0 {
                return Err(TS_EVALUE);
            }
            if s.ok {
                if s.code != 0 {
                    return Err(TS_EVALUE);
                }
                match VALS.get(vi) {
                    Some(w) => {
                        if *w != s.value {
                            return Err(TS_EVALUE);
                        }
                    }
                    None => return Err(TS_EBAL),
                }
                vi += 1;
            } else {
                if s.value != 0 {
                    return Err(TS_EVALUE);
                }
                match CODES.get(ci) {
                    Some(w) => {
                        if *w != s.code {
                            return Err(TS_EVALUE);
                        }
                    }
                    None => return Err(TS_EBAL),
                }
                ci += 1;
            }
            i += 1;
        }
        if vi != 8 || ci != 6 {
            return Err(TS_EBAL);
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
            return Err(TS_EBAL);
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
            Err(TS_ESEAL)
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

/// 八个成功格的真实观测值。
const VALS: [u32; 8] = [4, 5, 4, 0, 9, 5, 4, 0];

/// 六个失败格的错误码，按槽位顺序。
const CODES: [u32; 6] = [TS_ETOK, TS_ECYC, TS_ENAME, TS_ESTATE, TS_EBYTE, TS_EINDEX];

/// 十四格账本。
pub fn build_ledger() -> Ledger {
    let mut l = Ledger::new();
    let mut g = Graph::new();

    let _ = l.register(word_cell(0, OP_FEED, g.feed(b"a b c d\nb c\nd e\n")));
    let _ = l.register(word_cell(1, OP_SORT, g.sort()));
    let _ = l.register(word_cell(2, OP_POS, g.position(b"e")));
    let _ = l.register(word_cell(3, OP_POS, g.position(b"a")));
    let _ = l.register(count_cell(4, OP_RENDER, g.render().map(|s| s.len())));
    let _ = l.register(count_cell(5, OP_STAT, Ok(g.node_count())));
    let _ = l.register(count_cell(6, OP_STAT, Ok(g.edge_count())));
    let _ = l.register(count_cell(7, OP_STAT, Ok(g.dups() as usize)));

    let mut odd = Graph::new();
    let _ = l.register(word_cell(8, OP_FEED, odd.feed(b"a b c")));

    let mut cyc = Graph::new();
    let _ = cyc.feed(b"a b\nb a\n");
    let _ = l.register(word_cell(9, OP_SORT, cyc.sort()));

    let mut nan = Graph::new();
    let _ = l.register(word_cell(10, OP_FEED, nan.add_edge(b"", b"x")));

    let _ = l.register(word_cell(11, OP_SORT, g.sort()));

    let mut bad = Graph::new();
    let _ = l.register(word_cell(12, OP_FEED, bad.add_edge(b"a\x01", b"x")));

    let _ = l.register(word_cell(13, OP_POS, g.position(b"zz")));

    l
}

/// 四路注入。
pub fn inject_four() -> [bool; 4] {
    let mut out = [false; 4];

    let l = build_ledger();
    let rep = l.replay();
    let root = l.seal();
    out[0] = l.len() == SAMPLES && l.audit().is_ok() && rep == Ok(root) && root != 0;

    // 一路：菱形约束的拓扑次序。
    let mut g = Graph::new();
    let a = g.feed(b"a b\na c\nb d\nc d\n") == Ok(4);
    let b = g.sort() == Ok(4);
    let c = g.position(b"a") == Ok(0) && g.position(b"b") == Ok(1) && g.position(b"c") == Ok(2);
    let d = g.position(b"d") == Ok(3);
    let e = g.render().map(|s| s.len()) == Ok(7);
    out[1] = a && b && c && d && e && g.verify() && g.cycle() == 0;

    // 二路：去重、离散节点与名次映射。
    let mut h = Graph::new();
    let f1 = h.feed(b"p q\np q\np q\n") == Ok(3);
    let dup = h.dups() == 2 && h.edge_count() == 1;
    let extra = h.intern(b"zz") == Ok(2);
    let s1 = h.sort() == Ok(3);
    let ord = h.order_at(0) == Some(0)
        && h.order_at(1) == Some(1)
        && h.order_at(2) == Some(2)
        && h.position(b"zz") == Ok(2);
    let idx = h.id_of(b"q") == Some(1) && h.id_of(b"nope").is_none();
    out[2] = f1 && dup && extra && s1 && ord && idx && h.verify();

    // 三路：环、自环、奇数词与六类拒绝。
    let mut c1 = Graph::new();
    let _ = c1.feed(b"a b\nb c\nc a\n");
    let ring = c1.sort() == Err(TS_ECYC) && c1.cycle() == 3 && !c1.is_sorted();

    let mut c2 = Graph::new();
    let selfloop = c2.add_edge(b"x", b"x") == Err(TS_ECYC);

    let mut c3 = Graph::new();
    let odd = c3.feed(b"a b c") == Err(TS_ETOK) && c3.node_count() == 0;

    let mut c4 = Graph::new();
    let nan = c4.intern(b"") == Err(TS_ENAME);
    let byte = c4.intern(b"a\x02") == Err(TS_EBYTE);
    let state = c4.position(b"a") == Err(TS_ESTATE) && c4.render() == Err(TS_ESTATE);

    let mut led = build_ledger();
    let one = led.freeze();
    let rep2 = led.replay();
    let two = led.freeze();
    let reg = led.register(Sample::blank());
    out[3] = ring
        && selfloop
        && odd
        && nan
        && byte
        && state
        && one.is_ok()
        && rep2 == one
        && two.is_err()
        && reg == Err(TS_ESLOT)
        && led.is_frozen()
        && led.rejects() == 2;

    out
}

/// 十三项判据。
pub fn run_unx11550_checks() -> CheckSet {
    let mut s = CheckSet::new("unx11550.tsort");

    s.add(
        "TS01-喂入与去重",
        {
            let mut g = Graph::new();
            let a = g.feed(b"a b\nb c\n") == Ok(2);
            let b = g.edge_count() == 2 && g.node_count() == 3 && g.dups() == 0;
            let c = g.feed(b"a b\n") == Ok(1);
            let d = g.dups() == 1 && g.edge_count() == 2;
            let e = g.edge(0) == Some(Edge { from: 0, to: 1 })
                && g.edge(1) == Some(Edge { from: 1, to: 2 });
            a && b && c && d && e
        },
        "static",
    );

    s.add(
        "TS02-拓扑次序",
        {
            let mut g = Graph::new();
            let _ = g.feed(b"a b c d\nb c\nd e\n");
            let n = g.sort() == Ok(5);
            let o = g.order_at(0) == Some(0)
                && g.order_at(1) == Some(1)
                && g.order_at(2) == Some(2)
                && g.order_at(3) == Some(3)
                && g.order_at(4) == Some(4);
            n && o
                && g.position(b"a") == Ok(0)
                && g.position(b"e") == Ok(4)
                && g.render().map(|t| t.len()) == Ok(9)
                && g.verify()
        },
        "static",
    );

    s.add(
        "TS03-离散节点也入序",
        {
            let mut g = Graph::new();
            let _ = g.feed(b"x y\n");
            let _ = g.intern(b"z");
            let n = g.sort() == Ok(3);
            n && g.order_at(0) == Some(0)
                && g.order_at(1) == Some(1)
                && g.order_at(2) == Some(2)
                && g.indeg(2) == Ok(0)
                && g.verify()
        },
        "static",
    );

    s.add(
        "TS04-环检测",
        {
            let mut a = Graph::new();
            let _ = a.feed(b"a b\nb a\n");
            let two = a.sort() == Err(TS_ECYC) && a.cycle() == 2 && !a.is_sorted();

            let mut b = Graph::new();
            let _ = b.feed(b"a b\nb c\nc a\n");
            let three = b.sort() == Err(TS_ECYC) && b.cycle() == 3;

            let mut c = Graph::new();
            let _ = c.feed(b"a b\nb c\nc d\nd b\n");
            let tail = c.sort() == Err(TS_ECYC) && c.cycle() == 3;
            two && three && tail
        },
        "static",
    );

    s.add(
        "TS05-自环即拒绝",
        {
            let mut g = Graph::new();
            let a = g.add_edge(b"x", b"x") == Err(TS_ECYC);
            let b = g.node_count() == 1;
            let c = g.edge_count() == 0;
            let d = g.rejects() == 1;
            let e = g.sort() == Ok(1);
            a && b && c && d && e
        },
        "static",
    );

    s.add(
        "TS06-词数必须成对",
        {
            let mut g = Graph::new();
            let a = g.feed(b"a b c") == Err(TS_ETOK);
            let b = g.node_count() == 0 && g.edge_count() == 0;
            let c = g.feed(b"a b\nc") == Err(TS_ETOK);
            let d = g.edge_count() == 0;
            let e = g.feed(b"") == Ok(0);
            a && b && c && d && e
        },
        "static",
    );

    s.add(
        "TS07-名字约束",
        {
            let mut g = Graph::new();
            let a = g.intern(b"") == Err(TS_ENAME);
            let long = [b'n'; NAMEMAX + 1];
            let b = g.intern(&long) == Err(TS_ENAME);
            let c = g.intern(b"a\x01") == Err(TS_EBYTE);
            let d = g.intern(b"a\tb") == Err(TS_EBYTE);
            let e = g.intern(b"ok") == Ok(0) && g.intern(b"ok") == Ok(0);
            a && b && c && d && e && g.node_count() == 1
        },
        "static",
    );

    s.add(
        "TS08-容量上限",
        {
            let mut g = Graph::new();
            let names: [&[u8]; 17] = [
                b"n0", b"n1", b"n2", b"n3", b"n4", b"n5", b"n6", b"n7", b"n8", b"n9", b"n10",
                b"n11", b"n12", b"n13", b"n14", b"n15", b"n16",
            ];
            let mut i = 0usize;
            let mut full = true;
            while i < NODECAP {
                match names.get(i) {
                    Some(n) => {
                        if g.intern(*n) != Ok(i as u32) {
                            full = false;
                        }
                    }
                    None => full = false,
                }
                i += 1;
            }
            let over = match names.get(NODECAP) {
                Some(n) => g.intern(*n) == Err(TS_EFULL),
                None => false,
            };
            full && over && g.node_count() == NODECAP
        },
        "static",
    );

    s.add(
        "TS09-排序一次性",
        {
            let mut g = Graph::new();
            let _ = g.feed(b"a b\n");
            let a = g.sort() == Ok(2);
            let b = g.sort() == Err(TS_ESTATE);
            let c = g.is_sorted();
            let d = Graph::new().sort() == Ok(0);
            a && b && c && d
        },
        "static",
    );

    s.add(
        "TS10-渲染约束",
        {
            let mut g = Graph::new();
            let _ = g.feed(b"a b\n");
            let a = g.render() == Err(TS_ESTATE);
            let _ = g.sort();
            let b = g.render().map(|t| t.len()) == Ok(3);
            let mut big = Graph::new();
            let names: [&[u8]; 16] = [
                b"nnnn", b"mmmm", b"oooo", b"pppp", b"qqqq", b"rrrr", b"ssss", b"tttt", b"uuuu",
                b"vvvv", b"wwww", b"xxxx", b"yyyy", b"zzzz", b"aaaa", b"bbbb",
            ];
            let mut i = 0usize;
            while i < names.len() {
                if let Some(n) = names.get(i) {
                    let _ = big.intern(*n);
                }
                i += 1;
            }
            let _ = big.sort();
            let c = big.render() == Err(TS_EFMT);
            a && b && c
        },
        "static",
    );

    s.add(
        "TS11-图自洽",
        {
            let mut g = Graph::new();
            let _ = g.feed(b"a b\nb c\nc d\na d\n");
            let pre = g.verify() == false;
            let b = g.sort() == Ok(4);
            let c = g.verify();
            let d = g.emitted() == 4 && g.cycle() == 0;
            pre && b && c && d
        },
        "static",
    );

    s.add(
        "TS12-账本审计",
        {
            let l = build_ledger();
            l.len() == SAMPLES
                && l.ok_count() == 8
                && l.err_count() == 6
                && l.audit().is_ok()
                && l.rejects() == 0
                && l.sample(0).map(|x| x.value) == Some(4)
                && l.sample(13).map(|x| x.code) == Some(TS_EINDEX)
        },
        "static",
    );

    s.add(
        "TS13-冻结重放四注入",
        inject_four() == [true; 4] && build_ledger().audit().is_ok(),
        "static",
    );

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn feeds_pairs() {
        let mut g = Graph::new();
        assert_eq!(g.feed(b"a b\nb c\n"), Ok(2));
        assert_eq!(g.node_count(), 3);
        assert_eq!(g.edge_count(), 2);
        assert_eq!(g.feed(b"a b\n"), Ok(1));
        assert_eq!(g.dups(), 1);
        assert_eq!(g.edge_count(), 2);
    }

    #[test]
    fn sorts_chain() {
        let mut g = Graph::new();
        let _ = g.feed(b"a b\nb c\nc d\n");
        assert_eq!(g.sort(), Ok(4));
        assert_eq!(g.position(b"d"), Ok(3));
        assert!(g.verify());
    }

    #[test]
    fn detects_cycles() {
        let mut g = Graph::new();
        let _ = g.feed(b"a b\nb a\n");
        assert_eq!(g.sort(), Err(TS_ECYC));
        assert_eq!(g.cycle(), 2);
        assert!(!g.is_sorted());
    }

    #[test]
    fn rejects_self_loop() {
        let mut g = Graph::new();
        assert_eq!(g.add_edge(b"x", b"x"), Err(TS_ECYC));
        assert_eq!(g.edge_count(), 0);
    }

    #[test]
    fn rejects_odd_tokens() {
        let mut g = Graph::new();
        assert_eq!(g.feed(b"a b c"), Err(TS_ETOK));
        assert_eq!(g.node_count(), 0);
        assert_eq!(g.feed(b""), Ok(0));
    }

    #[test]
    fn names_and_capacity() {
        let mut g = Graph::new();
        assert_eq!(g.intern(b""), Err(TS_ENAME));
        assert_eq!(g.intern(b"a\x01"), Err(TS_EBYTE));
        assert_eq!(g.intern(b"ok"), Ok(0));
        let mut k = 1u32;
        while k < NODECAP as u32 {
            let name = [b'a' + (k % 26) as u8, b'b'];
            assert!(g.intern(&name).is_ok());
            k += 1;
        }
        assert_eq!(g.intern(b"zzz"), Err(TS_EFULL));
    }

    #[test]
    fn state_is_one_shot() {
        let mut g = Graph::new();
        assert_eq!(g.render(), Err(TS_ESTATE));
        let _ = g.feed(b"a b\n");
        assert_eq!(g.sort(), Ok(2));
        assert_eq!(g.sort(), Err(TS_ESTATE));
        assert!(g.render().is_ok());
    }

    #[test]
    fn ledger_audits_and_freezes() {
        let mut l = build_ledger();
        assert_eq!(l.len(), SAMPLES);
        assert!(l.audit().is_ok());
        assert_eq!(l.ok_count(), 8);
        assert_eq!(l.err_count(), 6);
        let one = l.freeze();
        assert!(one.is_ok());
        assert_eq!(l.replay(), one);
        assert!(l.freeze().is_err());
        assert_eq!(l.register(Sample::blank()), Err(TS_ESLOT));
        assert_eq!(l.rejects(), 2);
    }
}
