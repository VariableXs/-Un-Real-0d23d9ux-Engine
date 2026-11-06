//! UNX-F11370 dlopen 依赖件传递装载：DT_NEEDED 链（0xed20 段）
//!
//! 装载一个共享库时，链接器必须先把它的 `DT_NEEDED` 依赖全部装好——依赖先于
//! 本体、深度优先、同一依赖只装一次。这条递归链有三个必须单独诊断的失败面：
//!
//! 1. **依赖缺失**：链上某个 `DT_NEEDED` 找不到。报文必须指向**直接需要它的
//!    那个映像**，而不是链首；否则用户会去给根库加搜索路径，而真正缺件的是
//!    三层之后的一个小库。
//! 2. **依赖成环**：A 需要 B、B 需要 A。检测必须发生在递归**回到**某个仍在
//!    装载中的节点时；若只看「已装载」集合，环会表现为无限递归而不是错误。
//! 3. **依赖过深**：glibc 对依赖深度有上限，超过即失败。深度闸门与环检测必须
//!    分开——深链是合法的（只是太深），环是非法的，两者的修法完全不同。
//!
//! 装载次序是**后序**：先追加依赖，再追加本体。这个次序同时就是构造函数的调用
//! 次序，也是卸载时析构的反序基础（见 F11371）。
//!
//! 闸门次序（单节点）：深度上限 → 已在装载（环）→ 已装载（跳过）→ 逐条依赖
//! （缺失即报，带父子信息）→ 标记装载并追加后序。深度闸门排在最前，保证深链
//! 不会先触发环检测而产生误导性的 `ECYCLE`。

use crate::alloc::format;
use crate::checks::CheckSet;

/// 段基址。
pub const SEG_BASE: u32 = 0xed20;

/// 键为零。
pub const DN_EINVAL: u32 = SEG_BASE + 0x01;
/// 依赖件缺失。
pub const DN_EMISSING: u32 = SEG_BASE + 0x02;
/// 依赖成环。
pub const DN_ECYCLE: u32 = SEG_BASE + 0x03;
/// 节点表已满。
pub const DN_EFULL: u32 = SEG_BASE + 0x04;
/// 依赖深度超限。
pub const DN_EDEPTH: u32 = SEG_BASE + 0x05;
/// 自依赖。
pub const DN_EDEP: u32 = SEG_BASE + 0x06;
/// 重复登记同一映像。
pub const DN_EDUP: u32 = SEG_BASE + 0x07;
/// 节点号非法。
pub const DN_EBADID: u32 = SEG_BASE + 0x08;
/// 账本形状不符。
pub const DN_EBAL: u32 = SEG_BASE + 0x09;
/// 槽位乱序。
pub const DN_EORDER: u32 = SEG_BASE + 0x0a;
/// 观测值与复算不符。
pub const DN_EMISMATCH: u32 = SEG_BASE + 0x0b;
/// 冻结后重放不一致。
pub const DN_ESEAL: u32 = SEG_BASE + 0x0c;
/// 观察下标越界。
pub const DN_ENOBS: u32 = SEG_BASE + 0x0d;

/// 节点表容量。
pub const CAP: usize = 14;
/// 每个节点最多两条依赖。
pub const NEEDS: usize = 2;
/// 依赖深度上限。
pub const MAX_DEPTH: usize = 6;
/// 账本格数。
pub const SAMPLES: usize = 14;

/// 节点状态：未装载。
pub const ST_NEW: u8 = 1;
/// 节点状态：装载中。
pub const ST_LOADING: u8 = 2;
/// 节点状态：已装载。
pub const ST_LOADED: u8 = 3;
/// 节点状态：失败。
pub const ST_FAILED: u8 = 4;

/// 操作码：登记节点。
pub const OP_ADD: u8 = 1;
/// 操作码：装载。
pub const OP_LOAD: u8 = 2;
/// 操作码：读装载序。
pub const OP_ORDER: u8 = 3;
/// 操作码：读状态。
pub const OP_STATE: u8 = 4;

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

/// 依赖节点。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Node {
    /// 映像键。
    pub key: u32,
    /// 依赖键数组，0 表示空位。
    pub needs: [u32; NEEDS],
    /// 状态。
    pub state: u8,
}

impl Node {
    /// 造节点。
    pub const fn make(key: u32, a: u32, b: u32) -> Node {
        Node { key, needs: [a, b], state: ST_NEW }
    }

    /// 依赖条数。
    pub fn dep_count(&self) -> usize {
        let mut k = 0usize;
        let mut i = 0usize;
        while i < NEEDS {
            if let Some(v) = self.needs.get(i) {
                if *v != 0 {
                    k += 1;
                }
            }
            i += 1;
        }
        k
    }
}

/// 依赖图装载器。
#[derive(Clone, Copy, Debug)]
pub struct Graph {
    nodes: [Node; CAP],
    n: usize,
    order: [u32; CAP],
    order_len: usize,
    missing_parent: u32,
    missing_child: u32,
    visits: u32,
}

impl Graph {
    /// 空图。
    pub const fn new() -> Graph {
        Graph {
            nodes: [Node::make(0, 0, 0); CAP],
            n: 0,
            order: [0u32; CAP],
            order_len: 0,
            missing_parent: 0,
            missing_child: 0,
            visits: 0,
        }
    }

    /// 按键定位。
    fn idx_of(&self, key: u32) -> Option<usize> {
        let mut i = 0usize;
        while i < self.n {
            if let Some(v) = self.nodes.get(i) {
                if v.key == key {
                    return Some(i);
                }
            }
            i += 1;
        }
        None
    }

    /// 登记节点。
    pub fn add(&mut self, key: u32, a: u32, b: u32) -> Result<usize, u32> {
        if key == 0 {
            return Err(DN_EINVAL);
        }
        if a == key || b == key {
            return Err(DN_EDEP);
        }
        if self.idx_of(key).is_some() {
            return Err(DN_EDUP);
        }
        if self.n >= CAP {
            return Err(DN_EFULL);
        }
        if let Some(dst) = self.nodes.get_mut(self.n) {
            *dst = Node::make(key, a, b);
        }
        self.n += 1;
        Ok(self.n)
    }

    /// 后序追加。
    fn push_order(&mut self, key: u32) {
        if self.order_len < CAP {
            if let Some(dst) = self.order.get_mut(self.order_len) {
                *dst = key;
            }
            self.order_len += 1;
        }
    }

    /// 递归访问。
    fn visit(&mut self, idx: usize, depth: usize) -> Result<usize, u32> {
        if depth > MAX_DEPTH {
            return Err(DN_EDEPTH);
        }
        self.visits = self.visits.saturating_add(1);
        let node = match self.nodes.get(idx) {
            Some(v) => *v,
            None => return Err(DN_EBADID),
        };
        if node.state == ST_LOADED {
            return Ok(0);
        }
        if node.state == ST_LOADING {
            return Err(DN_ECYCLE);
        }
        if let Some(dst) = self.nodes.get_mut(idx) {
            dst.state = ST_LOADING;
        }
        let mut loaded = 0usize;
        let mut k = 0usize;
        while k < NEEDS {
            let dep = match node.needs.get(k) {
                Some(v) => *v,
                None => 0,
            };
            if dep != 0 {
                match self.idx_of(dep) {
                    Some(di) => match self.visit(di, depth + 1) {
                        Ok(v) => loaded += v,
                        Err(e) => {
                            if let Some(dst) = self.nodes.get_mut(idx) {
                                dst.state = ST_FAILED;
                            }
                            return Err(e);
                        }
                    },
                    None => {
                        self.missing_parent = node.key;
                        self.missing_child = dep;
                        if let Some(dst) = self.nodes.get_mut(idx) {
                            dst.state = ST_FAILED;
                        }
                        return Err(DN_EMISSING);
                    }
                }
            }
            k += 1;
        }
        if let Some(dst) = self.nodes.get_mut(idx) {
            dst.state = ST_LOADED;
        }
        self.push_order(node.key);
        Ok(loaded + 1)
    }

    /// 装载一个键及其依赖链。
    pub fn load(&mut self, key: u32) -> Result<usize, u32> {
        if key == 0 {
            return Err(DN_EINVAL);
        }
        self.missing_parent = 0;
        self.missing_child = 0;
        let idx = match self.idx_of(key) {
            Some(v) => v,
            None => {
                self.missing_parent = 0;
                self.missing_child = key;
                return Err(DN_EMISSING);
            }
        };
        self.visit(idx, 0)
    }

    /// 装载序第 i 位的键。
    pub fn order_at(&self, i: usize) -> Result<u32, u32> {
        if i >= self.order_len {
            return Err(DN_ENOBS);
        }
        match self.order.get(i) {
            Some(v) => Ok(*v),
            None => Err(DN_ENOBS),
        }
    }

    /// 装载序长度。
    pub fn order_len(&self) -> usize {
        self.order_len
    }

    /// 状态。
    pub fn state_of(&self, key: u32) -> Result<u8, u32> {
        match self.idx_of(key) {
            Some(i) => match self.nodes.get(i) {
                Some(v) => Ok(v.state),
                None => Err(DN_EBADID),
            },
            None => Err(DN_EMISSING),
        }
    }

    /// 缺失时的父键（0 表示链首即缺）。
    pub fn missing_parent(&self) -> u32 {
        self.missing_parent
    }

    /// 缺失的子键。
    pub fn missing_child(&self) -> u32 {
        self.missing_child
    }

    /// 节点数。
    pub fn len(&self) -> usize {
        self.n
    }

    /// 是否空图。
    pub fn is_empty(&self) -> bool {
        self.n == 0
    }

    /// 访问次数。
    pub fn visits(&self) -> u32 {
        self.visits
    }

    /// 摘要。
    pub fn digest(&self) -> u64 {
        let mut h = FNV_OFF;
        let mut i = 0usize;
        while i < self.n {
            if let Some(v) = self.nodes.get(i) {
                h = fold(h, v.key as u64);
                h = fold(h, v.state as u64);
                let mut k = 0usize;
                while k < NEEDS {
                    h = fold(h, v.needs.get(k).copied().unwrap_or(0) as u64);
                    k += 1;
                }
            }
            i += 1;
        }
        fold(h, self.order_len as u64)
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

    /// 登记一格。
    pub fn register(&mut self, s: Sample) -> Result<(), u32> {
        if self.frozen || (s.slot as usize) != self.len || (s.slot as usize) >= SAMPLES {
            self.rejects += 1;
            return Err(DN_EORDER);
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
            return Err(DN_EBAL);
        }
        const VALS: [u32; 8] = [1, 2, 3, 3, 0x13, 0x11, 0, 3];
        const CODES: [u32; 6] =
            [DN_EMISSING, DN_EDUP, DN_EINVAL, DN_EDEP, DN_ECYCLE, DN_EDEPTH];
        let mut vi = 0usize;
        let mut ci = 0usize;
        let mut i = 0usize;
        while i < self.len {
            let s = match self.rows.get(i) {
                Some(v) => *v,
                None => return Err(DN_EBAL),
            };
            if s.slot as usize != i {
                return Err(DN_EORDER);
            }
            if s.ok {
                if s.code != 0 {
                    return Err(DN_EMISMATCH);
                }
                match VALS.get(vi) {
                    Some(w) => {
                        if s.value != *w {
                            return Err(DN_EMISMATCH);
                        }
                    }
                    None => return Err(DN_EBAL),
                }
                vi += 1;
            } else {
                match CODES.get(ci) {
                    Some(w) => {
                        if s.code != *w {
                            return Err(DN_EMISMATCH);
                        }
                    }
                    None => return Err(DN_EBAL),
                }
                ci += 1;
            }
            i += 1;
        }
        if vi != 8 || ci != 6 {
            return Err(DN_EBAL);
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
            return Err(DN_EBAL);
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
            Err(DN_ESEAL)
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

/// 十四格账本：三层依赖后序装载，六类拒绝各据其位。
pub fn build_ledger() -> Ledger {
    let mut l = Ledger::new();
    let mut g = Graph::new();

    let _ = l.register(cell(0, OP_ADD, g.add(0x11, 0x12, 0)));
    let _ = l.register(cell(1, OP_ADD, g.add(0x12, 0x13, 0)));
    let _ = l.register(cell(2, OP_ADD, g.add(0x13, 0, 0)));
    let _ = l.register(cell(3, OP_LOAD, g.load(0x11)));
    let _ = l.register(word_cell(4, OP_ORDER, g.order_at(0)));
    let _ = l.register(word_cell(5, OP_ORDER, g.order_at(2)));
    let _ = l.register(cell(6, OP_LOAD, g.load(0x11)));
    let _ = l.register(cell(7, OP_STATE, match g.state_of(0x11) {
        Ok(v) => Ok(v as usize),
        Err(e) => Err(e),
    }));

    let _ = l.register(cell(8, OP_LOAD, g.load(0x77)));

    let _ = l.register(cell(9, OP_ADD, g.add(0x13, 0, 0)));

    let _ = l.register(cell(10, OP_ADD, g.add(0, 0, 0)));

    let _ = l.register(cell(11, OP_ADD, g.add(0x14, 0x14, 0)));

    let _ = g.add(0x21, 0x22, 0);
    let _ = g.add(0x22, 0x21, 0);
    let _ = l.register(cell(12, OP_LOAD, g.load(0x21)));

    let _ = g.add(0x30, 0x31, 0);
    let _ = g.add(0x31, 0x32, 0);
    let _ = g.add(0x32, 0x33, 0);
    let _ = g.add(0x33, 0x34, 0);
    let _ = g.add(0x34, 0x35, 0);
    let _ = g.add(0x35, 0x36, 0);
    let _ = g.add(0x36, 0x37, 0);
    let _ = g.add(0x37, 0, 0);
    let _ = l.register(cell(13, OP_LOAD, g.load(0x30)));

    l
}

/// 四路注入。
pub fn inject_four() -> [bool; 4] {
    let mut out = [false; 4];

    let l = build_ledger();
    let rep = l.replay();
    let root = l.seal();
    out[0] = l.len() == SAMPLES && l.audit().is_ok() && rep == Ok(root) && root != 0;

    // 一路：后序装载，依赖在前，重复装载为 0。
    let mut a = Graph::new();
    let _ = a.add(0x11, 0x12, 0x13);
    let _ = a.add(0x12, 0x13, 0);
    let _ = a.add(0x13, 0, 0);
    let n = a.load(0x11);
    let again = a.load(0x11);
    let first = a.order_at(0);
    let second = a.order_at(1);
    let third = a.order_at(2);
    out[1] = n == Ok(3)
        && again == Ok(0)
        && first == Ok(0x13)
        && second == Ok(0x12)
        && third == Ok(0x11)
        && a.order_len() == 3
        && a.state_of(0x11) == Ok(ST_LOADED)
        && a.state_of(0x13) == Ok(ST_LOADED);

    // 二路：缺件指向直接父，环与深链分开。
    let mut b = Graph::new();
    let _ = b.add(0x41, 0x42, 0);
    let _ = b.add(0x42, 0x43, 0);
    let miss = b.load(0x41);
    let parent = b.missing_parent();
    let child = b.missing_child();
    let mut c = Graph::new();
    let _ = c.add(0x51, 0x52, 0);
    let _ = c.add(0x52, 0x51, 0);
    let cyc = c.load(0x51);
    let mut d = Graph::new();
    let _ = d.add(0x61, 0x62, 0);
    let _ = d.add(0x62, 0x63, 0);
    let _ = d.add(0x63, 0x64, 0);
    let _ = d.add(0x64, 0x65, 0);
    let _ = d.add(0x65, 0x66, 0);
    let _ = d.add(0x66, 0x67, 0);
    let _ = d.add(0x67, 0x68, 0);
    let _ = d.add(0x68, 0, 0);
    let deep = d.load(0x61);
    out[2] = miss == Err(DN_EMISSING)
        && parent == 0x42
        && child == 0x43
        && cyc == Err(DN_ECYCLE)
        && deep == Err(DN_EDEPTH)
        && miss != cyc
        && b.state_of(0x41) == Ok(ST_FAILED)
        && c.order_len() == 0;

    // 三路：闸门与容量边界。
    let mut e = Graph::new();
    let g1 = e.add(0, 0, 0);
    let g2 = e.add(0x11, 0x11, 0);
    let g3 = e.add(0x11, 0x12, 0);
    let g4 = e.add(0x11, 0, 0);
    let g5 = e.load(0);
    let mut f = Graph::new();
    let mut i = 0u32;
    while (i as usize) < CAP {
        let _ = f.add(0x70 + i, 0, 0);
        i += 1;
    }
    let full = f.add(0x90, 0, 0);
    let dup = f.add(0x71, 0, 0);
    let obs = f.order_at(0);
    out[3] = g1 == Err(DN_EINVAL)
        && g2 == Err(DN_EDEP)
        && g3 == Ok(1)
        && g4 == Err(DN_EDUP)
        && g5 == Err(DN_EINVAL)
        && full == Err(DN_EFULL)
        && dup == Err(DN_EDUP)
        && obs == Err(DN_ENOBS)
        && f.len() == CAP;

    out
}

/// 运行本卡检查。
pub fn run_unx11370_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F11370");
    let l = build_ledger();

    s.add_dyn(
        "DN1-十四格八允六拒",
        l.len() == SAMPLES && l.ok_count() == 8 && l.err_count() == 6 && l.audit().is_ok(),
        &format!("n={} ok={} err={} audit={:?}", l.len(), l.ok_count(), l.err_count(), l.audit()),
    );

    s.add(
        "DN2-依赖先于本体",
        {
            let mut g = Graph::new();
            let _ = g.add(0x11, 0x12, 0);
            let _ = g.add(0x12, 0, 0);
            let n = g.load(0x11);
            n == Ok(2) && g.order_at(0) == Ok(0x12) && g.order_at(1) == Ok(0x11)
        },
        "static",
    );

    s.add(
        "DN3-共享依赖只装一次",
        {
            let mut g = Graph::new();
            let _ = g.add(0x11, 0x12, 0x13);
            let _ = g.add(0x12, 0x13, 0);
            let _ = g.add(0x13, 0, 0);
            let n = g.load(0x11);
            n == Ok(3) && g.order_len() == 3 && g.visits() >= 3
        },
        "static"
    );

    s.add(
        "DN4-缺件指向直接父",
        {
            let mut g = Graph::new();
            let _ = g.add(0x11, 0x12, 0);
            let _ = g.add(0x12, 0x99, 0);
            let a = g.load(0x11);
            a == Err(DN_EMISSING) && g.missing_parent() == 0x12 && g.missing_child() == 0x99
        },
        "static",
    );

    s.add(
        "DN5-环检测依赖装载中标记",
        {
            let mut g = Graph::new();
            let _ = g.add(0x11, 0x12, 0);
            let _ = g.add(0x12, 0x13, 0);
            let _ = g.add(0x13, 0x11, 0);
            let a = g.load(0x11);
            a == Err(DN_ECYCLE) && g.order_len() == 0
        },
        "static",
    );

    s.add(
        "DN6-深链与环分开",
        {
            let mut g = Graph::new();
            let _ = g.add(0x11, 0x12, 0);
            let _ = g.add(0x12, 0x13, 0);
            let _ = g.add(0x13, 0x14, 0);
            let _ = g.add(0x14, 0x15, 0);
            let _ = g.add(0x15, 0x16, 0);
            let _ = g.add(0x16, 0x17, 0);
            let _ = g.add(0x17, 0x18, 0);
            let _ = g.add(0x18, 0, 0);
            let a = g.load(0x11);
            let b = g.load(0x12);
            a == Err(DN_EDEPTH)
                && b == Ok(7)
                && g.missing_child() == 0
                && g.order_len() == 7
        },
        "static",
    );

    s.add(
        "DN7-深度极限可装",
        {
            let mut g = Graph::new();
            let _ = g.add(0x11, 0x12, 0);
            let _ = g.add(0x12, 0x13, 0);
            let _ = g.add(0x13, 0x14, 0);
            let _ = g.add(0x14, 0x15, 0);
            let _ = g.add(0x15, 0x16, 0);
            let _ = g.add(0x16, 0, 0);
            let a = g.load(0x11);
            a == Ok(6) && g.order_len() == 6 && g.state_of(0x11) == Ok(ST_LOADED)
        },
        "static",
    );

    s.add(
        "DN8-失败态可观测",
        {
            let mut g = Graph::new();
            let _ = g.add(0x11, 0x99, 0);
            let a = g.load(0x11);
            a == Err(DN_EMISSING)
                && g.state_of(0x11) == Ok(ST_FAILED)
                && g.missing_parent() == 0x11
                && g.missing_child() == 0x99
        },
        "static",
    );

    s.add(
        "DN9-自依赖先于重复",
        {
            let mut g = Graph::new();
            let _ = g.add(0x11, 0, 0);
            let a = g.add(0x11, 0x11, 0);
            let b = g.add(0x11, 0, 0);
            a == Err(DN_EDEP) && b == Err(DN_EDUP) && g.len() == 1
        },
        "static",
    );

    s.add(
        "DN10-容量精确",
        {
            let mut g = Graph::new();
            let mut i = 0u32;
            while (i as usize) < CAP {
                let _ = g.add(0x40 + i, 0, 0);
                i += 1;
            }
            let over = g.add(0x60, 0, 0);
            let dup = g.add(0x41, 0, 0);
            over == Err(DN_EFULL) && dup == Err(DN_EDUP) && g.len() == CAP
        },
        "static",
    );

    s.add(
        "DN11-账本槽序不可越",
        {
            let mut x = Ledger::new();
            let a = x.register(cell(1, OP_ADD, Ok(1)));
            let b = x.register(cell(0, OP_ADD, Ok(1)));
            let c = x.register(cell(0, OP_ADD, Ok(1)));
            a == Err(DN_EORDER) && b == Ok(()) && c == Err(DN_EORDER) && x.len() == 1
        },
        "static",
    );

    s.add(
        "DN12-观测面边界",
        {
            let x = build_ledger();
            let a = x.sample(SAMPLES - 1);
            let b = x.sample(SAMPLES);
            let mut g = Graph::new();
            let _ = g.add(0x11, 0, 0);
            let c = g.order_at(0);
            let d = g.state_of(0x12);
            a.is_some() && b.is_none() && c == Err(DN_ENOBS) && d == Err(DN_EMISSING)
        },
        "static",
    );

    s.add(
        "DN13-冻结重放四注入",
        {
            let mut f = build_ledger();
            let rep = f.replay();
            let one = f.freeze();
            let two = f.freeze();
            let inj = inject_four();
            rep.is_ok()
                && one.is_ok()
                && one == rep
                && two.is_err()
                && f.is_frozen()
                && f.rejects() == 1
                && f.root() != 0
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
        let s = run_unx11370_checks();
        assert_eq!(s.len(), 13);
        assert!(s.all_passed());
    }

    #[test]
    fn post_order_load() {
        let mut g = Graph::new();
        let _ = g.add(0x11, 0x12, 0);
        let _ = g.add(0x12, 0, 0);
        assert_eq!(g.load(0x11), Ok(2));
        assert_eq!(g.order_at(0), Ok(0x12));
        assert_eq!(g.order_at(1), Ok(0x11));
        assert_eq!(g.load(0x11), Ok(0));
    }

    #[test]
    fn missing_reports_parent() {
        let mut g = Graph::new();
        let _ = g.add(0x11, 0x12, 0);
        let _ = g.add(0x12, 0x99, 0);
        assert_eq!(g.load(0x11), Err(DN_EMISSING));
        assert_eq!(g.missing_parent(), 0x12);
        assert_eq!(g.missing_child(), 0x99);
        assert_eq!(g.state_of(0x11), Ok(ST_FAILED));
    }

    #[test]
    fn cycle_detected_while_loading() {
        let mut g = Graph::new();
        let _ = g.add(0x11, 0x12, 0);
        let _ = g.add(0x12, 0x11, 0);
        assert_eq!(g.load(0x11), Err(DN_ECYCLE));
        assert_eq!(g.order_len(), 0);
    }

    #[test]
    fn depth_limit_is_six() {
        let mut g = Graph::new();
        let _ = g.add(0x11, 0x12, 0);
        let _ = g.add(0x12, 0x13, 0);
        let _ = g.add(0x13, 0x14, 0);
        let _ = g.add(0x14, 0x15, 0);
        let _ = g.add(0x15, 0x16, 0);
        let _ = g.add(0x16, 0, 0);
        assert_eq!(g.load(0x11), Ok(6));
        let mut h = Graph::new();
        let _ = h.add(0x11, 0x12, 0);
        let _ = h.add(0x12, 0x13, 0);
        let _ = h.add(0x13, 0x14, 0);
        let _ = h.add(0x14, 0x15, 0);
        let _ = h.add(0x15, 0x16, 0);
        let _ = h.add(0x16, 0x17, 0);
        let _ = h.add(0x17, 0x18, 0);
        let _ = h.add(0x18, 0, 0);
        assert_eq!(h.load(0x11), Err(DN_EDEPTH));
    }

    #[test]
    fn gate_order_zero_dep_then_dup() {
        let mut g = Graph::new();
        assert_eq!(g.add(0, 0, 0), Err(DN_EINVAL));
        assert_eq!(g.add(0x11, 0x11, 0), Err(DN_EDEP));
        assert_eq!(g.add(0x11, 0, 0), Ok(1));
        assert_eq!(g.add(0x11, 0x12, 0), Err(DN_EDUP));
        assert_eq!(g.load(0), Err(DN_EINVAL));
    }

    #[test]
    fn capacity_is_exact() {
        let mut g = Graph::new();
        let mut i = 0u32;
        while (i as usize) < CAP {
            assert!(g.add(0x70 + i, 0, 0).is_ok());
            i += 1;
        }
        assert_eq!(g.add(0x90, 0, 0), Err(DN_EFULL));
        assert_eq!(g.len(), CAP);
    }
}
