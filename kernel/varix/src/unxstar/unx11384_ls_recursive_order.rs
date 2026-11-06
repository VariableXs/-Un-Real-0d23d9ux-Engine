//! UNX-F11384 ls -R 递归列举与序规则
//!
//! 递归列举的序是**先序遍历**：打印当前目录块，再按目录块内的入表序逐个下
//! 潜。头行形如 `==> ./a/c <==`，长度 = 8 + 路径字节数。`-r` 与 `-R` 都走
//! 同一套走树规则，只是一份树的两种写法。
//!
//! 闸门次序：
//! - `mkdir`：空名或超长 → 父下标非目录 → 深度超限 → 同级重名 → 表满；
//! - `reparent`：根不可改挂/下标越界 → 新父非目录 → 成环 → 深度超限 →
//!   同级重名；
//! - `walk`：根非目录 → 访问中重复到达（防御，判成环）；
//! - `entry`：未走树 → 序越界；
//! - `header`：下标越界或非目录 → 未走树 → 未访问 → 深度超限。
//!
//! 账本形状：14 格 = 8 允 + 6 拒，6 拒覆盖 6 个不同域错误码。

use crate::alloc::format;
use crate::checks::CheckSet;

/// 段基址。
pub const SEG_BASE: u32 = 0xee00;

/// 名字为空或超长。
pub const LSR_ENAME: u32 = SEG_BASE + 0x01;
/// 父不是目录。
pub const LSR_ENOTDIR: u32 = SEG_BASE + 0x02;
/// 同级重名。
pub const LSR_EDUP: u32 = SEG_BASE + 0x03;
/// 改挂成环。
pub const LSR_ECYCLE: u32 = SEG_BASE + 0x04;
/// 嵌套深度超限。
pub const LSR_EDEPTH: u32 = SEG_BASE + 0x05;
/// 节点表满。
pub const LSR_EFULL: u32 = SEG_BASE + 0x06;
/// 序越界。
pub const LSR_EGAP: u32 = SEG_BASE + 0x07;
/// 未走树/根不可改挂。
pub const LSR_EORDER: u32 = SEG_BASE + 0x08;
/// 账本形状不符。
pub const LSR_EBAL: u32 = SEG_BASE + 0x09;
/// 槽位乱序。
pub const LSR_ESLOT: u32 = SEG_BASE + 0x0a;
/// 观测值与复算不符。
pub const LSR_EVALUE: u32 = SEG_BASE + 0x0b;
/// 冻结后重放不一致。
pub const LSR_ESEAL: u32 = SEG_BASE + 0x0c;
/// 观察下标越界。
pub const LSR_ENOBS: u32 = SEG_BASE + 0x0d;

/// 节点表容量。
pub const CAP: usize = 16;
/// 名字字节上限。
pub const NAME_MAX: usize = 12;
/// 最大嵌套深度（根为 0）。
pub const MAXD: u32 = 3;
/// 无父标记。
pub const NO_PARENT: u16 = 0xffff;
/// 账本格数。
pub const SAMPLES: usize = 14;

/// 操作码：建节点。
pub const OP_MKDIR: u8 = 1;
/// 操作码：走树。
pub const OP_WALK: u8 = 2;
/// 操作码：按序取节点。
pub const OP_ENTRY: u8 = 3;
/// 操作码：头行长度。
pub const OP_HEADER: u8 = 4;
/// 操作码：改挂。
pub const OP_MOVE: u8 = 5;

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

/// 一个树节点。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Node {
    name: [u8; NAME_MAX],
    nlen: u8,
    parent: u16,
    is_dir: bool,
}

impl Node {
    const fn blank() -> Node {
        Node { name: [0; NAME_MAX], nlen: 0, parent: NO_PARENT, is_dir: false }
    }

    /// 名字字节。
    pub fn bytes(&self) -> &[u8] {
        match self.name.get(..self.nlen as usize) {
            Some(v) => v,
            None => &[],
        }
    }

    /// 父下标。
    pub fn parent(&self) -> u16 {
        self.parent
    }

    /// 是否目录。
    pub fn is_dir(&self) -> bool {
        self.is_dir
    }
}

/// 拷入名字。
fn put(dst: &mut [u8; NAME_MAX], src: &[u8]) -> u8 {
    let n = if src.len() > NAME_MAX { NAME_MAX } else { src.len() };
    let mut i = 0usize;
    while i < n {
        if let (Some(d), Some(s)) = (dst.get_mut(i), src.get(i)) {
            *d = *s;
        }
        i += 1;
    }
    n as u8
}

/// 字节序相等。
fn same_bytes(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut i = 0usize;
    while i < a.len() {
        match (a.get(i), b.get(i)) {
            (Some(x), Some(y)) => {
                if x != y {
                    return false;
                }
            }
            _ => return false,
        }
        i += 1;
    }
    true
}

/// 递归树。
#[derive(Clone, Copy, Debug)]
pub struct Tree {
    nodes: [Node; CAP],
    n: usize,
    order: [u16; CAP],
    seen: usize,
    walked: bool,
}

impl Tree {
    /// 建一棵只有根 `.` 的树。
    pub fn new() -> Tree {
        let mut t = Tree {
            nodes: [Node::blank(); CAP],
            n: 0,
            order: [0; CAP],
            seen: 0,
            walked: false,
        };
        let root = Node {
            name: [0; NAME_MAX],
            nlen: put(&mut [0u8; NAME_MAX], b"."),
            parent: NO_PARENT,
            is_dir: true,
        };
        if let Some(slot) = t.nodes.get_mut(0) {
            *slot = root;
        }
        t.n = 1;
        t
    }

    /// 节点数。
    pub fn count(&self) -> usize {
        self.n
    }

    /// 第 i 个节点。
    pub fn node_at(&self, i: usize) -> Option<Node> {
        if i >= self.n {
            return None;
        }
        self.nodes.get(i).copied()
    }

    /// 第 i 个节点名。
    pub fn name_at(&self, i: usize) -> Option<&[u8]> {
        match self.nodes.get(i) {
            Some(v) if i < self.n => Some(v.bytes()),
            _ => None,
        }
    }

    /// 节点深度（根为 0）。
    pub fn depth_of(&self, idx: usize) -> Option<u32> {
        if idx >= self.n {
            return None;
        }
        let mut cur = idx;
        let mut d = 0u32;
        let mut guard = 0u32;
        loop {
            if guard > CAP as u32 {
                return None;
            }
            guard += 1;
            let v = match self.nodes.get(cur) {
                Some(x) => *x,
                None => return None,
            };
            if v.parent == NO_PARENT {
                return Some(d);
            }
            cur = v.parent as usize;
            d = d.saturating_add(1);
            if cur >= self.n {
                return None;
            }
        }
    }

    /// 按名字找同级节点。
    fn sibling(&self, parent: usize, name: &[u8], skip: usize) -> Option<usize> {
        let mut i = 0usize;
        while i < self.n {
            if i != skip {
                match self.nodes.get(i) {
                    Some(v) => {
                        if v.parent as usize == parent && same_bytes(v.bytes(), name) {
                            return Some(i);
                        }
                    }
                    None => return None,
                }
            }
            i += 1;
        }
        None
    }

    /// `node` 是否为 `anc` 的后代（含自身）。
    fn descendant(&self, node: usize, anc: usize) -> bool {
        let mut cur = node;
        let mut guard = 0u32;
        while guard <= CAP as u32 {
            guard += 1;
            if cur == anc {
                return true;
            }
            let v = match self.nodes.get(cur) {
                Some(x) => *x,
                None => return false,
            };
            if v.parent == NO_PARENT {
                return false;
            }
            cur = v.parent as usize;
        }
        true
    }

    /// 在 `parent` 下建一个节点，返回新下标。
    ///
    /// 闸门次序：空名/超长 → 父非目录 → 深度超限 → 同级重名 → 表满。
    pub fn mkdir(&mut self, name: &[u8], parent: usize, is_dir: bool) -> Result<usize, u32> {
        if name.is_empty() || name.len() > NAME_MAX {
            return Err(LSR_ENAME);
        }
        if parent >= self.n {
            return Err(LSR_ENOTDIR);
        }
        match self.nodes.get(parent) {
            Some(v) => {
                if !v.is_dir {
                    return Err(LSR_ENOTDIR);
                }
            }
            None => return Err(LSR_ENOTDIR),
        }
        let pd = match self.depth_of(parent) {
            Some(v) => v,
            None => return Err(LSR_EDEPTH),
        };
        if pd.saturating_add(1) > MAXD {
            return Err(LSR_EDEPTH);
        }
        if self.sibling(parent, name, usize::MAX).is_some() {
            return Err(LSR_EDUP);
        }
        if self.n >= CAP {
            return Err(LSR_EFULL);
        }
        let slot = match self.nodes.get_mut(self.n) {
            Some(v) => v,
            None => return Err(LSR_EFULL),
        };
        slot.name = [0; NAME_MAX];
        slot.nlen = put(&mut slot.name, name);
        slot.parent = parent as u16;
        slot.is_dir = is_dir;
        self.n += 1;
        self.walked = false;
        self.seen = 0;
        Ok(self.n - 1)
    }

    /// 改挂一个节点，返回其下标。
    ///
    /// 闸门次序：根/越界 → 新父非目录 → 成环 → 深度超限 → 同级重名。
    pub fn reparent(&mut self, idx: usize, new_parent: usize) -> Result<usize, u32> {
        if idx == 0 || idx >= self.n {
            return Err(LSR_EORDER);
        }
        if new_parent >= self.n {
            return Err(LSR_ENOTDIR);
        }
        match self.nodes.get(new_parent) {
            Some(v) => {
                if !v.is_dir {
                    return Err(LSR_ENOTDIR);
                }
            }
            None => return Err(LSR_ENOTDIR),
        }
        if self.descendant(new_parent, idx) {
            return Err(LSR_ECYCLE);
        }
        let nd = match self.depth_of(new_parent) {
            Some(v) => v,
            None => return Err(LSR_EDEPTH),
        };
        if nd.saturating_add(1) > MAXD {
            return Err(LSR_EDEPTH);
        }
        let name = match self.nodes.get(idx) {
            Some(v) => *v,
            None => return Err(LSR_EORDER),
        };
        if self.sibling(new_parent, name.bytes(), idx).is_some() {
            return Err(LSR_EDUP);
        }
        if let Some(v) = self.nodes.get_mut(idx) {
            v.parent = new_parent as u16;
        }
        self.walked = false;
        Ok(idx)
    }

    /// 先序遍历目录块，返回访问到的目录数。
    ///
    /// 闸门次序：根非目录 → 重复到达（判成环）。
    pub fn walk(&mut self, root: usize) -> Result<u32, u32> {
        if root >= self.n {
            return Err(LSR_ENOTDIR);
        }
        match self.nodes.get(root) {
            Some(v) => {
                if !v.is_dir {
                    return Err(LSR_ENOTDIR);
                }
            }
            None => return Err(LSR_ENOTDIR),
        }
        let mut stack = [0u16; CAP];
        let mut sp = 1usize;
        if let Some(s) = stack.get_mut(0) {
            *s = root as u16;
        }
        let mut visited = [false; CAP];
        let mut seen = 0usize;
        while sp > 0 {
            sp -= 1;
            let cur = match stack.get(sp) {
                Some(v) => *v as usize,
                None => return Err(LSR_ECYCLE),
            };
            if cur >= self.n {
                return Err(LSR_ENOTDIR);
            }
            match visited.get_mut(cur) {
                Some(v) => {
                    if *v {
                        return Err(LSR_ECYCLE);
                    }
                    *v = true;
                }
                None => return Err(LSR_ECYCLE),
            }
            let node = match self.nodes.get(cur) {
                Some(v) => *v,
                None => return Err(LSR_ENOTDIR),
            };
            if !node.is_dir {
                continue;
            }
            match self.order.get_mut(seen) {
                Some(d) => *d = cur as u16,
                None => return Err(LSR_ECYCLE),
            }
            seen += 1;
            let mut i = self.n;
            while i > 0 {
                i -= 1;
                match self.nodes.get(i) {
                    Some(ch) => {
                        if ch.parent as usize == cur {
                            if sp >= CAP {
                                return Err(LSR_ECYCLE);
                            }
                            if let Some(s) = stack.get_mut(sp) {
                                *s = i as u16;
                            }
                            sp += 1;
                        }
                    }
                    None => return Err(LSR_ECYCLE),
                }
            }
        }
        self.seen = seen;
        self.walked = true;
        Ok(seen as u32)
    }

    /// 访问序第 k 个目录的节点下标。
    ///
    /// 闸门次序：未走树 → 序越界。
    pub fn entry(&self, k: usize) -> Result<u32, u32> {
        if !self.walked {
            return Err(LSR_EORDER);
        }
        if k >= self.seen {
            return Err(LSR_EGAP);
        }
        match self.order.get(k) {
            Some(v) => Ok(*v as u32),
            None => Err(LSR_EGAP),
        }
    }

    /// 访问序目录数。
    pub fn visited(&self) -> usize {
        self.seen
    }

    /// 头行 `==> path <==` 的字节长度。
    ///
    /// 闸门次序：越界或非目录 → 未走树 → 未访问 → 深度超限。
    pub fn header(&self, idx: usize) -> Result<u32, u32> {
        if idx >= self.n {
            return Err(LSR_ENOTDIR);
        }
        let v = match self.nodes.get(idx) {
            Some(x) => *x,
            None => return Err(LSR_ENOTDIR),
        };
        if !v.is_dir {
            return Err(LSR_ENOTDIR);
        }
        if !self.walked {
            return Err(LSR_EORDER);
        }
        let mut hit = false;
        let mut i = 0usize;
        while i < self.seen {
            match self.order.get(i) {
                Some(x) => {
                    if *x as usize == idx {
                        hit = true;
                    }
                }
                None => return Err(LSR_EORDER),
            }
            i += 1;
        }
        if !hit {
            return Err(LSR_EORDER);
        }
        let d = match self.depth_of(idx) {
            Some(x) => x,
            None => return Err(LSR_EDEPTH),
        };
        if d > MAXD {
            return Err(LSR_EDEPTH);
        }
        let mut path = v.nlen as u32;
        let mut cur = idx;
        let mut guard = 0u32;
        loop {
            if guard > CAP as u32 {
                return Err(LSR_EDEPTH);
            }
            guard += 1;
            let p = match self.nodes.get(cur) {
                Some(x) => *x,
                None => return Err(LSR_EDEPTH),
            };
            if p.parent == NO_PARENT {
                break;
            }
            let par = p.parent as usize;
            let pn = match self.nodes.get(par) {
                Some(x) => *x,
                None => return Err(LSR_EDEPTH),
            };
            path = path.saturating_add(1).saturating_add(pn.nlen as u32);
            cur = par;
        }
        Ok(path.saturating_add(8))
    }

    /// 指纹。
    pub fn seal(&self) -> u64 {
        let mut h = FNV_OFF;
        let mut i = 0usize;
        while i < self.n {
            if let Some(v) = self.nodes.get(i) {
                let b = v.bytes();
                let mut k = 0usize;
                while k < b.len() {
                    if let Some(x) = b.get(k) {
                        h = fold(h, *x as u64);
                    }
                    k += 1;
                }
                h = fold(h, 0xff);
                h = fold(h, v.parent as u64);
                h = fold(h, v.is_dir as u64);
            }
            i += 1;
        }
        fold(h, self.n as u64)
    }

    /// 不变式自检。
    pub fn consistent(&self) -> bool {
        if self.n == 0 || self.n > CAP || self.seen > CAP {
            return false;
        }
        let root = match self.nodes.get(0) {
            Some(v) => *v,
            None => return false,
        };
        if !root.is_dir || root.parent != NO_PARENT {
            return false;
        }
        let mut i = 0usize;
        while i < self.n {
            let v = match self.nodes.get(i) {
                Some(x) => *x,
                None => return false,
            };
            if v.nlen == 0 || v.nlen as usize > NAME_MAX {
                return false;
            }
            if i > 0 {
                if v.parent == NO_PARENT || v.parent as usize >= self.n {
                    return false;
                }
                let d = match self.depth_of(i) {
                    Some(x) => x,
                    None => return false,
                };
                if d > MAXD {
                    return false;
                }
                let mut j = 0usize;
                while j < i {
                    match self.nodes.get(j) {
                        Some(p) => {
                            if p.parent == v.parent && same_bytes(p.bytes(), v.bytes()) {
                                return false;
                            }
                        }
                        None => return false,
                    }
                    j += 1;
                }
            }
            i += 1;
        }
        if self.walked {
            let mut k = 0usize;
            while k < self.seen {
                match self.order.get(k) {
                    Some(x) => {
                        if *x as usize >= self.n {
                            return false;
                        }
                    }
                    None => return false,
                }
                k += 1;
            }
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
            return Err(LSR_ESLOT);
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
            return Err(LSR_EBAL);
        }
        const VALS: [u32; 8] = [1, 2, 3, 3, 0, 3, 13, 4];
        const CODES: [u32; 6] =
            [LSR_ENAME, LSR_ENOTDIR, LSR_EDUP, LSR_EORDER, LSR_ECYCLE, LSR_EGAP];
        let mut vi = 0usize;
        let mut ci = 0usize;
        let mut i = 0usize;
        while i < self.len {
            let s = match self.rows.get(i) {
                Some(v) => *v,
                None => return Err(LSR_EBAL),
            };
            if s.slot as usize != i {
                return Err(LSR_ESLOT);
            }
            if s.ok {
                if s.code != 0 {
                    return Err(LSR_EVALUE);
                }
                match VALS.get(vi) {
                    Some(w) => {
                        if s.value != *w {
                            return Err(LSR_EVALUE);
                        }
                    }
                    None => return Err(LSR_EBAL),
                }
                vi += 1;
            } else {
                match CODES.get(ci) {
                    Some(w) => {
                        if s.code != *w {
                            return Err(LSR_EVALUE);
                        }
                    }
                    None => return Err(LSR_EBAL),
                }
                ci += 1;
            }
            i += 1;
        }
        if vi != 8 || ci != 6 {
            return Err(LSR_EBAL);
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
            return Err(LSR_EBAL);
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
            Err(LSR_ESEAL)
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

/// 十四格账本：8 允 + 6 拒。
pub fn build_ledger() -> Ledger {
    let mut l = Ledger::new();
    let mut t = Tree::new();

    let _ = l.register(cell(0, OP_MKDIR, t.mkdir(b"a", 0, true)));
    let _ = l.register(cell(1, OP_MKDIR, t.mkdir(b"b", 0, false)));
    let _ = l.register(cell(2, OP_MKDIR, t.mkdir(b"c", 1, true)));
    let _ = l.register(word_cell(3, OP_WALK, t.walk(0)));
    let _ = l.register(word_cell(4, OP_ENTRY, t.entry(0)));
    let _ = l.register(word_cell(5, OP_ENTRY, t.entry(2)));
    let _ = l.register(word_cell(6, OP_HEADER, t.header(3)));
    let _ = l.register(cell(7, OP_MKDIR, t.mkdir(b"d", 3, true)));

    let _ = l.register(cell(8, OP_MKDIR, t.mkdir(b"", 0, true)));
    let _ = l.register(cell(9, OP_MKDIR, t.mkdir(b"x", 2, true)));
    let _ = l.register(cell(10, OP_MKDIR, t.mkdir(b"a", 0, false)));
    let _ = l.register(cell(11, OP_MOVE, t.reparent(0, 1)));
    let _ = l.register(cell(12, OP_MOVE, t.reparent(1, 3)));
    let _ = t.walk(0);
    let _ = l.register(word_cell(13, OP_ENTRY, t.entry(4)));

    l
}

/// 四路注入。
pub fn inject_four() -> [bool; 4] {
    let mut out = [false; 4];

    let l = build_ledger();
    let rep = l.replay();
    let root = l.seal();
    out[0] = l.len() == SAMPLES && l.audit().is_ok() && rep == Ok(root) && root != 0;

    // 一路：先序访问与头行长度。
    let mut t = Tree::new();
    let a = t.mkdir(b"a", 0, true);
    let b = t.mkdir(b"bb", 0, false);
    let c = t.mkdir(b"c", 1, true);
    let w = t.walk(0);
    out[1] = a == Ok(1)
        && b == Ok(2)
        && c == Ok(3)
        && w == Ok(3)
        && t.entry(0) == Ok(0)
        && t.entry(1) == Ok(1)
        && t.entry(2) == Ok(3)
        && t.header(3) == Ok(13)
        && t.name_at(1) == Some(&b"a"[..])
        && t.count() == 4
        && t.consistent();

    // 二路：八类失败各自可辨，失败不改状态。
    let mut q = Tree::new();
    let empty = q.mkdir(b"", 0, true) == Err(LSR_ENAME);
    let long = q.mkdir(b"0123456789abc", 0, true) == Err(LSR_ENAME);
    let _ = q.mkdir(b"f", 0, false);
    let notdir = q.mkdir(b"x", 1, true) == Err(LSR_ENOTDIR);
    let first = q.mkdir(b"a", 0, true) == Ok(2);
    let dup = q.mkdir(b"a", 0, false) == Err(LSR_EDUP);
    let root_move = q.reparent(0, 1) == Err(LSR_EORDER);
    let cycle = {
        let dd = q.mkdir(b"e", 2, true);
        match dd {
            Ok(v) => q.reparent(2, v) == Err(LSR_ECYCLE),
            Err(_) => false,
        }
    };
    let early = q.entry(0) == Err(LSR_EORDER);
    let _ = q.walk(0);
    let gap = q.entry(9) == Err(LSR_EGAP);
    let before = q.header(1) == Err(LSR_ENOTDIR);
    out[2] = empty
        && long
        && notdir
        && first
        && dup
        && root_move
        && cycle
        && early
        && gap
        && before
        && q.consistent();

    // 三路：表满与深度上限。
    let mut f = Tree::new();
    let mut i = 1usize;
    while i < CAP {
        let name: [u8; 3] = [b'n', b'0' + (i / 10) as u8, b'0' + (i % 10) as u8];
        let _ = f.mkdir(&name, 0, false);
        i += 1;
    }
    let q1 = f.count() == CAP;
    let q2 = f.mkdir(b"zz", 0, false) == Err(LSR_EFULL);
    let mut d = Tree::new();
    let d1 = d.mkdir(b"a", 0, true);
    let d2 = d.mkdir(b"b", 1, true);
    let d3 = d.mkdir(b"c", 2, true);
    let d4 = d.mkdir(b"e", 3, true);
    out[3] = q1 && q2 && d1 == Ok(1) && d2 == Ok(2) && d3 == Ok(3) && d4 == Err(LSR_EDEPTH)
        && f.consistent()
        && d.consistent();

    out
}

/// 十三项判据。
pub fn run_unx11384_checks() -> CheckSet {
    let mut s = CheckSet::new("unx11384.ls_recursive_order");
    let l = build_ledger();

    s.add_dyn(
        "LSR1-十四格八允六拒",
        l.len() == SAMPLES && l.ok_count() == 8 && l.err_count() == 6 && l.audit().is_ok(),
        &format!("n={} ok={} err={}", l.len(), l.ok_count(), l.err_count()),
    );

    s.add(
        "LSR2-递归目录访问序",
        l.sample(3).map(|x| x.value) == Some(3)
            && l.sample(4).map(|x| x.value) == Some(0)
            && l.sample(5).map(|x| x.value) == Some(3),
        "static",
    );

    s.add(
        "LSR3-条目序号取回",
        {
            let mut t = Tree::new();
            let _ = t.mkdir(b"a", 0, true);
            let _ = t.mkdir(b"b", 0, false);
            let _ = t.mkdir(b"c", 1, true);
            let w = t.walk(0);
            w == Ok(3)
                && t.entry(0) == Ok(0)
                && t.entry(1) == Ok(1)
                && t.entry(2) == Ok(3)
                && t.entry(3) == Err(LSR_EGAP)
                && t.visited() == 3
                && t.consistent()
        },
        "static",
    );

    s.add(
        "LSR4-空名被拒",
        {
            let mut t = Tree::new();
            t.mkdir(b"", 0, true) == Err(LSR_ENAME)
                && t.mkdir(b"0123456789abc", 0, true) == Err(LSR_ENAME)
                && t.count() == 1
                && l.sample(8).map(|x| x.code) == Some(LSR_ENAME)
        },
        "static",
    );

    s.add(
        "LSR5-父不是目录被拒",
        {
            let mut t = Tree::new();
            let _ = t.mkdir(b"f", 0, false);
            t.mkdir(b"x", 1, true) == Err(LSR_ENOTDIR)
                && t.mkdir(b"y", 99, true) == Err(LSR_ENOTDIR)
                && t.count() == 2
                && t.consistent()
                && l.sample(9).map(|x| x.code) == Some(LSR_ENOTDIR)
        },
        "static",
    );

    s.add(
        "LSR6-同级重名被拒",
        {
            let mut t = Tree::new();
            let _ = t.mkdir(b"a", 0, true);
            t.mkdir(b"a", 0, false) == Err(LSR_EDUP)
                && t.mkdir(b"a", 1, true) == Ok(2)
                && t.consistent()
                && l.sample(10).map(|x| x.code) == Some(LSR_EDUP)
        },
        "static",
    );

    s.add(
        "LSR7-根不可改挂",
        {
            let mut t = Tree::new();
            let _ = t.mkdir(b"a", 0, true);
            t.reparent(0, 1) == Err(LSR_EORDER)
                && t.reparent(9, 1) == Err(LSR_EORDER)
                && t.node_at(0).map(|x| x.parent()) == Some(NO_PARENT)
                && l.sample(11).map(|x| x.code) == Some(LSR_EORDER)
        },
        "static",
    );

    s.add(
        "LSR8-成环改挂被拒",
        {
            let mut t = Tree::new();
            let _ = t.mkdir(b"a", 0, true);
            let _ = t.mkdir(b"b", 1, true);
            t.reparent(1, 2) == Err(LSR_ECYCLE)
                && t.reparent(1, 0) == Ok(1)
                && t.consistent()
                && l.sample(12).map(|x| x.code) == Some(LSR_ECYCLE)
        },
        "static",
    );

    s.add(
        "LSR9-深度超限被拒",
        {
            let mut t = Tree::new();
            let _ = t.mkdir(b"a", 0, true);
            let _ = t.mkdir(b"b", 1, true);
            let _ = t.mkdir(b"c", 2, true);
            let deep = t.mkdir(b"d", 3, true);
            deep == Err(LSR_EDEPTH) && t.count() == 4 && t.consistent()
        },
        "static",
    );

    s.add(
        "LSR10-越界取条目",
        {
            let mut t = Tree::new();
            t.entry(0) == Err(LSR_EORDER)
                && t.walk(0) == Ok(1)
                && t.entry(0) == Ok(0)
                && t.entry(1) == Err(LSR_EGAP)
                && l.sample(13).map(|x| x.code) == Some(LSR_EGAP)
        },
        "static",
    );

    s.add(
        "LSR11-头行字节长度",
        {
            let mut t = Tree::new();
            let _ = t.mkdir(b"a", 0, true);
            let _ = t.mkdir(b"c", 1, true);
            let _ = t.walk(0);
            t.header(0) == Ok(9)
                && t.header(1) == Ok(11)
                && t.header(2) == Ok(13)
                && l.sample(6).map(|x| x.value) == Some(13)
        },
        "static",
    );

    s.add(
        "LSR12-节点表满被拒",
        {
            let mut t = Tree::new();
            let mut i = 1usize;
            while i < CAP {
                let name: [u8; 3] = [b'n', b'0' + (i / 10) as u8, b'0' + (i % 10) as u8];
                let _ = t.mkdir(&name, 0, false);
                i += 1;
            }
            t.count() == CAP
                && t.mkdir(b"zz", 0, false) == Err(LSR_EFULL)
                && t.consistent()
        },
        "static",
    );

    s.add(
        "LSR13-冻结重放四注入",
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
        assert_eq!(l.rejects(), 0);
        assert!(l.audit().is_ok());
        let mut i = 0usize;
        while i < l.len() {
            assert_eq!(l.sample(i).map(|x| x.slot as usize), Some(i));
            i += 1;
        }
    }

    #[test]
    fn checks_pass() {
        let s = run_unx11384_checks();
        assert_eq!(s.len(), 13);
        assert!(s.all_passed());
    }

    #[test]
    fn preorder_shape() {
        let mut t = Tree::new();
        assert_eq!(t.mkdir(b"a", 0, true), Ok(1));
        assert_eq!(t.mkdir(b"b", 0, false), Ok(2));
        assert_eq!(t.mkdir(b"c", 1, true), Ok(3));
        assert_eq!(t.walk(0), Ok(3));
        assert_eq!(t.entry(0), Ok(0));
        assert_eq!(t.entry(1), Ok(1));
        assert_eq!(t.entry(2), Ok(3));
        assert!(t.consistent());
    }

    #[test]
    fn header_lengths() {
        let mut t = Tree::new();
        let _ = t.mkdir(b"a", 0, true);
        let _ = t.mkdir(b"c", 1, true);
        let _ = t.walk(0);
        assert_eq!(t.header(0), Ok(9));
        assert_eq!(t.header(1), Ok(11));
        assert_eq!(t.header(2), Ok(13));
    }

    #[test]
    fn cycle_rejected() {
        let mut t = Tree::new();
        assert_eq!(t.mkdir(b"a", 0, true), Ok(1));
        assert_eq!(t.mkdir(b"b", 1, true), Ok(2));
        assert_eq!(t.reparent(1, 2), Err(LSR_ECYCLE));
        assert_eq!(t.reparent(0, 1), Err(LSR_EORDER));
        assert!(t.consistent());
    }

    #[test]
    fn depth_limit() {
        let mut t = Tree::new();
        assert_eq!(t.mkdir(b"a", 0, true), Ok(1));
        assert_eq!(t.mkdir(b"b", 1, true), Ok(2));
        assert_eq!(t.mkdir(b"c", 2, true), Ok(3));
        assert_eq!(t.mkdir(b"d", 3, true), Err(LSR_EDEPTH));
        assert_eq!(t.depth_of(3), Some(3));
    }

    #[test]
    fn capacity_limit() {
        let mut t = Tree::new();
        let mut i = 1usize;
        while i < CAP {
            let name: [u8; 3] = [b'n', b'0' + (i / 10) as u8, b'0' + (i % 10) as u8];
            assert_eq!(t.mkdir(&name, 0, false), Ok(i));
            i += 1;
        }
        assert_eq!(t.mkdir(b"zz", 0, false), Err(LSR_EFULL));
        assert!(t.consistent());
    }

    #[test]
    fn inject_four_holds() {
        assert_eq!(inject_four(), [true; 4]);
    }
}
