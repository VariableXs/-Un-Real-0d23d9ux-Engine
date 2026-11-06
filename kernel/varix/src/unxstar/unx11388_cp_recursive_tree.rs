//! UNX-F11388 cp -r/-R 目录递归复制
//!
//! 目录复制的真值是**树形态等价**：空目录要留、嵌套层级要留、每个节点的权限
//! 位要留。`-r` 与 `-R` 在本实现里归一成同一条递归规则（同一 effective 值），
//! 只保留原始写法用于回显——两者不许出现行为差。
//!
//! 闸门次序：
//! - `open`：标志既不是 -r 也不是 -R；
//! - `mkdir`：空名/超长 → 父非目录 → 深度超限 → 同级重名 → 节点表满；
//! - `copy`：空树（只有根）→ 逐节点深度复核（防御）；
//! - `equiv`：effective 标志不同 → 任一侧未复制 → 节点数/形态/权限不等。
//!
//! 账本形状：14 格 = 8 允 + 6 拒，6 拒覆盖 6 个不同域错误码。

use crate::alloc::format;
use crate::checks::CheckSet;

/// 段基址。
pub const SEG_BASE: u32 = 0xee40;

/// 非法标志。
pub const CPR_EFLAG: u32 = SEG_BASE + 0x01;
/// 名字为空或超长。
pub const CPR_ENAME: u32 = SEG_BASE + 0x02;
/// 父不是目录。
pub const CPR_ENOTDIR: u32 = SEG_BASE + 0x03;
/// 深度超限。
pub const CPR_EDEPTH: u32 = SEG_BASE + 0x04;
/// 同级重名。
pub const CPR_EDUP: u32 = SEG_BASE + 0x05;
/// 节点表满。
pub const CPR_EFULL: u32 = SEG_BASE + 0x06;
/// 空树不可复制。
pub const CPR_EEMPTY: u32 = SEG_BASE + 0x07;
/// 树形不等价。
pub const CPR_ESHAPE: u32 = SEG_BASE + 0x08;
/// 账本形状不符。
pub const CPR_EBAL: u32 = SEG_BASE + 0x09;
/// 槽位乱序。
pub const CPR_ESLOT: u32 = SEG_BASE + 0x0a;
/// 观测值与复算不符。
pub const CPR_EVALUE: u32 = SEG_BASE + 0x0b;
/// 冻结后重放不一致。
pub const CPR_ESEAL: u32 = SEG_BASE + 0x0c;
/// 观察下标越界。
pub const CPR_ENOBS: u32 = SEG_BASE + 0x0d;

/// 节点表容量。
pub const CAP: usize = 12;
/// 名字字节上限。
pub const NAME_MAX: usize = 12;
/// 最大嵌套深度（根为 0）。
pub const MAXD: u32 = 3;
/// 无父标记。
pub const NO_PARENT: u16 = 0xffff;
/// 权限位掩码。
pub const MODE_MASK: u32 = 0o7777;
/// `-r` 写法。
pub const FLAG_R: u8 = 1;
/// `-R` 写法。
pub const FLAG_RR: u8 = 2;
/// 归一后的递归语义。
pub const FLAG_REC: u8 = 1;
/// 账本格数。
pub const SAMPLES: usize = 14;

/// 操作码：建节点。
pub const OP_MKDIR: u8 = 1;
/// 操作码：复制。
pub const OP_COPY: u8 = 2;
/// 操作码：等价比较。
pub const OP_EQUIV: u8 = 3;
/// 操作码：开户。
pub const OP_OPEN: u8 = 4;

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
    mode: u32,
}

impl Node {
    const fn blank() -> Node {
        Node {
            name: [0; NAME_MAX],
            nlen: 0,
            parent: NO_PARENT,
            is_dir: false,
            mode: 0,
        }
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

    /// 权限位。
    pub fn mode(&self) -> u32 {
        self.mode
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

/// 递归复制树。
#[derive(Clone, Copy, Debug)]
pub struct Tree {
    flag: u8,
    nodes: [Node; CAP],
    n: usize,
    copied: bool,
}

impl Tree {
    /// 按 `-r` / `-R` 开户；根节点 `.` 是目录。
    ///
    /// 闸门：标志既不是 -r 也不是 -R。
    pub fn open(flag: u8) -> Result<Tree, u32> {
        if flag != FLAG_R && flag != FLAG_RR {
            return Err(CPR_EFLAG);
        }
        let mut t = Tree { flag, nodes: [Node::blank(); CAP], n: 0, copied: false };
        let root = Node {
            name: [0; NAME_MAX],
            nlen: put(&mut [0u8; NAME_MAX], b"."),
            parent: NO_PARENT,
            is_dir: true,
            mode: 0o755,
        };
        if let Some(slot) = t.nodes.get_mut(0) {
            *slot = root;
        }
        t.n = 1;
        Ok(t)
    }

    /// 原始标志写法。
    pub fn flag(&self) -> u8 {
        self.flag
    }

    /// 归一后的递归语义。
    pub fn effective(&self) -> u8 {
        FLAG_REC
    }

    /// 节点数。
    pub fn count(&self) -> usize {
        self.n
    }

    /// 是否已复制。
    pub fn is_copied(&self) -> bool {
        self.copied
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

    /// 同级同名查找。
    fn sibling(&self, parent: usize, name: &[u8]) -> Option<usize> {
        let mut i = 0usize;
        while i < self.n {
            match self.nodes.get(i) {
                Some(v) => {
                    if v.parent as usize == parent && same_bytes(v.bytes(), name) {
                        return Some(i);
                    }
                }
                None => return None,
            }
            i += 1;
        }
        None
    }

    /// 建一个节点（含空目录），返回新下标。
    ///
    /// 闸门次序：空名/超长 → 父非目录 → 深度超限 → 同级重名 → 表满。
    pub fn mkdir(&mut self, name: &[u8], parent: usize, is_dir: bool, mode: u32) -> Result<usize, u32> {
        if name.is_empty() || name.len() > NAME_MAX {
            return Err(CPR_ENAME);
        }
        if parent >= self.n {
            return Err(CPR_ENOTDIR);
        }
        match self.nodes.get(parent) {
            Some(v) => {
                if !v.is_dir {
                    return Err(CPR_ENOTDIR);
                }
            }
            None => return Err(CPR_ENOTDIR),
        }
        let pd = match self.depth_of(parent) {
            Some(v) => v,
            None => return Err(CPR_EDEPTH),
        };
        if pd.saturating_add(1) > MAXD {
            return Err(CPR_EDEPTH);
        }
        if self.sibling(parent, name).is_some() {
            return Err(CPR_EDUP);
        }
        if self.n >= CAP {
            return Err(CPR_EFULL);
        }
        let slot = match self.nodes.get_mut(self.n) {
            Some(v) => v,
            None => return Err(CPR_EFULL),
        };
        slot.name = [0; NAME_MAX];
        slot.nlen = put(&mut slot.name, name);
        slot.parent = parent as u16;
        slot.is_dir = is_dir;
        slot.mode = mode & MODE_MASK;
        self.n += 1;
        self.copied = false;
        Ok(self.n - 1)
    }

    /// 递归复制整棵树，返回复制出的节点数。
    ///
    /// 闸门次序：空树 → 逐节点深度复核。
    pub fn copy(&mut self) -> Result<u32, u32> {
        if self.n <= 1 {
            return Err(CPR_EEMPTY);
        }
        let mut i = 0usize;
        while i < self.n {
            match self.depth_of(i) {
                Some(d) => {
                    if d > MAXD {
                        return Err(CPR_EDEPTH);
                    }
                }
                None => return Err(CPR_EDEPTH),
            }
            i += 1;
        }
        self.copied = true;
        Ok(self.n as u32)
    }

    /// 两棵树形态是否等价（含空目录与权限位）。
    ///
    /// 闸门次序：effective 标志不同 → 任一侧未复制 → 节点数不等 → 逐节点不等。
    pub fn equiv(&self, other: &Tree) -> Result<u32, u32> {
        if self.effective() != other.effective() {
            return Err(CPR_EFLAG);
        }
        if !self.copied || !other.copied {
            return Err(CPR_ESHAPE);
        }
        if self.n != other.n {
            return Err(CPR_ESHAPE);
        }
        let mut i = 0usize;
        while i < self.n {
            let a = match self.nodes.get(i) {
                Some(v) => *v,
                None => return Err(CPR_ESHAPE),
            };
            let b = match other.nodes.get(i) {
                Some(v) => *v,
                None => return Err(CPR_ESHAPE),
            };
            if a.parent != b.parent || a.is_dir != b.is_dir || a.mode != b.mode {
                return Err(CPR_ESHAPE);
            }
            if !same_bytes(a.bytes(), b.bytes()) {
                return Err(CPR_ESHAPE);
            }
            i += 1;
        }
        Ok(1)
    }

    /// 树形指纹。
    pub fn shape_hash(&self) -> u64 {
        let mut h = FNV_OFF;
        h = fold(h, self.effective() as u64);
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
                h = fold(h, v.mode as u64);
            }
            i += 1;
        }
        fold(h, self.n as u64)
    }

    /// 不变式自检。
    pub fn consistent(&self) -> bool {
        if self.n == 0 || self.n > CAP {
            return false;
        }
        if self.flag != FLAG_R && self.flag != FLAG_RR {
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
            if v.nlen == 0 || v.nlen as usize > NAME_MAX || v.mode & !MODE_MASK != 0 {
                return false;
            }
            if i > 0 {
                if v.parent == NO_PARENT || v.parent as usize >= self.n {
                    return false;
                }
                match self.depth_of(i) {
                    Some(d) => {
                        if d > MAXD {
                            return false;
                        }
                    }
                    None => return false,
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
            return Err(CPR_ESLOT);
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
            return Err(CPR_EBAL);
        }
        const VALS: [u32; 8] = [1, 2, 3, 4, 1, 4, 5, 4];
        const CODES: [u32; 6] =
            [CPR_EFLAG, CPR_ENAME, CPR_ENOTDIR, CPR_EDUP, CPR_EDEPTH, CPR_EEMPTY];
        let mut vi = 0usize;
        let mut ci = 0usize;
        let mut i = 0usize;
        while i < self.len {
            let s = match self.rows.get(i) {
                Some(v) => *v,
                None => return Err(CPR_EBAL),
            };
            if s.slot as usize != i {
                return Err(CPR_ESLOT);
            }
            if s.ok {
                if s.code != 0 {
                    return Err(CPR_EVALUE);
                }
                match VALS.get(vi) {
                    Some(w) => {
                        if s.value != *w {
                            return Err(CPR_EVALUE);
                        }
                    }
                    None => return Err(CPR_EBAL),
                }
                vi += 1;
            } else {
                match CODES.get(ci) {
                    Some(w) => {
                        if s.code != *w {
                            return Err(CPR_EVALUE);
                        }
                    }
                    None => return Err(CPR_EBAL),
                }
                ci += 1;
            }
            i += 1;
        }
        if vi != 8 || ci != 6 {
            return Err(CPR_EBAL);
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
            return Err(CPR_EBAL);
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
            Err(CPR_ESEAL)
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
    let mut t = match Tree::open(FLAG_R) {
        Ok(v) => v,
        Err(_) => return l,
    };

    let _ = l.register(cell(0, OP_MKDIR, t.mkdir(b"a", 0, true, 0o755)));
    let _ = l.register(cell(1, OP_MKDIR, t.mkdir(b"empty", 0, true, 0o700)));
    let _ = l.register(cell(2, OP_MKDIR, t.mkdir(b"deep", 1, true, 0o750)));
    let _ = l.register(word_cell(3, OP_COPY, t.copy()));

    let mut t2 = match Tree::open(FLAG_RR) {
        Ok(v) => v,
        Err(_) => return l,
    };
    let _ = t2.mkdir(b"a", 0, true, 0o755);
    let _ = t2.mkdir(b"empty", 0, true, 0o700);
    let _ = t2.mkdir(b"deep", 1, true, 0o750);
    let _ = t2.copy();
    let _ = l.register(word_cell(4, OP_EQUIV, t.equiv(&t2)));

    let _ = l.register(cell(5, OP_MKDIR, t.mkdir(b"f", 3, false, 0o644)));
    let _ = l.register(word_cell(6, OP_COPY, t.copy()));
    let _ = l.register(word_cell(7, OP_COPY, t2.copy()));

    let _ = l.register(word_cell(
        8,
        OP_OPEN,
        Tree::open(0).map(|v| v.flag() as u32),
    ));
    let _ = l.register(cell(9, OP_MKDIR, t.mkdir(b"", 0, true, 0o755)));
    let _ = l.register(cell(10, OP_MKDIR, t.mkdir(b"x", 4, true, 0o755)));
    let _ = l.register(cell(11, OP_MKDIR, t.mkdir(b"a", 0, true, 0o755)));

    let q = t.mkdir(b"q", 3, true, 0o755);
    let qidx = match q {
        Ok(v) => v,
        Err(_) => 0,
    };
    let _ = l.register(cell(12, OP_MKDIR, t.mkdir(b"r", qidx, true, 0o755)));

    let mut e = match Tree::open(FLAG_R) {
        Ok(v) => v,
        Err(_) => return l,
    };
    let _ = l.register(word_cell(13, OP_COPY, e.copy()));

    l
}

/// 四路注入。
pub fn inject_four() -> [bool; 4] {
    let mut out = [false; 4];

    let l = build_ledger();
    let rep = l.replay();
    let root = l.seal();
    out[0] = l.len() == SAMPLES && l.audit().is_ok() && rep == Ok(root) && root != 0;

    // 一路：树形、空目录与权限位原样。
    let mut a = match Tree::open(FLAG_R) {
        Ok(v) => v,
        Err(_) => return out,
    };
    let r1 = a.mkdir(b"d", 0, true, 0o750);
    let r2 = a.mkdir(b"f", 0, false, 0o640);
    let r3 = a.mkdir(b"e", 1, true, 0o700);
    let c = a.copy();
    out[1] = r1 == Ok(1)
        && r2 == Ok(2)
        && r3 == Ok(3)
        && c == Ok(4)
        && a.node_at(3).map(|x| x.mode()) == Some(0o700)
        && a.node_at(1).map(|x| x.is_dir()) == Some(true)
        && a.node_at(2).map(|x| x.is_dir()) == Some(false)
        && a.count() == 4
        && a.effective() == FLAG_REC
        && a.consistent();

    // 二路：八类失败各自可辨，失败不改状态。
    let mut b = match Tree::open(FLAG_RR) {
        Ok(v) => v,
        Err(_) => return out,
    };
    let empty = b.copy() == Err(CPR_EEMPTY);
    let empty_name = b.mkdir(b"", 0, true, 0o755) == Err(CPR_ENAME);
    let long = b.mkdir(b"0123456789abc", 0, true, 0o755) == Err(CPR_ENAME);
    let _ = b.mkdir(b"f", 0, false, 0o644);
    let notdir = b.mkdir(b"x", 1, true, 0o755) == Err(CPR_ENOTDIR);
    let first = b.mkdir(b"a", 0, true, 0o755) == Ok(2);
    let dup = b.mkdir(b"a", 0, false, 0o644) == Err(CPR_EDUP);
    let q = b.mkdir(b"q", 2, true, 0o755);
    let depth = match q {
        Ok(v) => match b.mkdir(b"s", v, true, 0o755) {
            Ok(w) => b.mkdir(b"t", w, true, 0o755) == Err(CPR_EDEPTH),
            Err(_) => false,
        },
        Err(_) => false,
    };
    let uncopied = {
        let other = match Tree::open(FLAG_R) {
            Ok(v) => v,
            Err(_) => return out,
        };
        b.equiv(&other) == Err(CPR_ESHAPE)
    };
    out[2] = empty
        && empty_name
        && long
        && notdir
        && first
        && dup
        && depth
        && uncopied
        && b.consistent();

    // 三路：表满、深度上限与 -r/-R 等价锁。
    let mut f = match Tree::open(FLAG_R) {
        Ok(v) => v,
        Err(_) => return out,
    };
    let mut i = 1usize;
    while i < CAP {
        let name: [u8; 3] = [b'n', b'0' + (i / 10) as u8, b'0' + (i % 10) as u8];
        let _ = f.mkdir(&name, 0, false, 0o644);
        i += 1;
    }
    let q1 = f.count() == CAP;
    let q2 = f.mkdir(b"zz", 0, false, 0o644) == Err(CPR_EFULL);
    let mut g = match Tree::open(FLAG_R) {
        Ok(v) => v,
        Err(_) => return out,
    };
    let g1 = g.mkdir(b"a", 0, true, 0o755);
    let g2 = g.mkdir(b"b", 1, true, 0o755);
    let g3 = g.mkdir(b"c", 2, true, 0o755);
    let g4 = g.mkdir(b"d", 3, true, 0o755);
    let mut h = match Tree::open(FLAG_RR) {
        Ok(v) => v,
        Err(_) => return out,
    };
    let _ = h.mkdir(b"a", 0, true, 0o755);
    let _ = h.copy();
    let mut k = match Tree::open(FLAG_R) {
        Ok(v) => v,
        Err(_) => return out,
    };
    let _ = k.mkdir(b"a", 0, true, 0o755);
    let _ = k.copy();
    let q3 = h.equiv(&k) == Ok(1) && h.flag() != k.flag() && h.effective() == k.effective();
    out[3] = q1
        && q2
        && g1 == Ok(1)
        && g2 == Ok(2)
        && g3 == Ok(3)
        && g4 == Err(CPR_EDEPTH)
        && q3
        && f.consistent()
        && g.consistent();

    out
}

/// 十三项判据。
pub fn run_unx11388_checks() -> CheckSet {
    let mut s = CheckSet::new("unx11388.cp_recursive_tree");
    let l = build_ledger();

    s.add_dyn(
        "CPR1-十四格八允六拒",
        l.len() == SAMPLES && l.ok_count() == 8 && l.err_count() == 6 && l.audit().is_ok(),
        &format!("n={} ok={} err={}", l.len(), l.ok_count(), l.err_count()),
    );

    s.add(
        "CPR2-树形等价真值",
        l.sample(4).map(|x| x.value) == Some(1)
            && l.sample(6).map(|x| x.value) == Some(5)
            && l.sample(7).map(|x| x.value) == Some(4),
        "static",
    );

    s.add(
        "CPR3--r 与 -R 等价锁",
        {
            let mut a = match Tree::open(FLAG_R) {
                Ok(v) => v,
                Err(_) => return s,
            };
            let mut b = match Tree::open(FLAG_RR) {
                Ok(v) => v,
                Err(_) => return s,
            };
            let _ = a.mkdir(b"d", 0, true, 0o750);
            let _ = b.mkdir(b"d", 0, true, 0o750);
            let _ = a.mkdir(b"f", 1, false, 0o644);
            let _ = b.mkdir(b"f", 1, false, 0o644);
            a.copy() == Ok(3)
                && b.copy() == Ok(3)
                && a.flag() == FLAG_R
                && b.flag() == FLAG_RR
                && a.effective() == b.effective()
                && a.equiv(&b) == Ok(1)
                && a.shape_hash() == b.shape_hash()
        },
        "static",
    );

    s.add(
        "CPR4-空目录保留",
        {
            let mut a = match Tree::open(FLAG_R) {
                Ok(v) => v,
                Err(_) => return s,
            };
            let _ = a.mkdir(b"empty", 0, true, 0o700);
            let _ = a.mkdir(b"nested", 0, true, 0o755);
            let _ = a.mkdir(b"inner", 2, true, 0o750);
            a.copy() == Ok(4)
                && a.node_at(1).map(|x| x.is_dir()) == Some(true)
                && a.node_at(1).map(|x| x.mode()) == Some(0o700)
                && a.node_at(3).map(|x| x.mode()) == Some(0o750)
                && a.count() == 4
                && a.consistent()
        },
        "static",
    );

    s.add(
        "CPR5-空名被拒",
        {
            let mut a = match Tree::open(FLAG_R) {
                Ok(v) => v,
                Err(_) => return s,
            };
            a.mkdir(b"", 0, true, 0o755) == Err(CPR_ENAME)
                && a.mkdir(b"0123456789abc", 0, true, 0o755) == Err(CPR_ENAME)
                && a.count() == 1
                && l.sample(9).map(|x| x.code) == Some(CPR_ENAME)
        },
        "static",
    );

    s.add(
        "CPR6-父不是目录被拒",
        {
            let mut a = match Tree::open(FLAG_R) {
                Ok(v) => v,
                Err(_) => return s,
            };
            let _ = a.mkdir(b"f", 0, false, 0o644);
            a.mkdir(b"x", 1, true, 0o755) == Err(CPR_ENOTDIR)
                && a.mkdir(b"y", 99, true, 0o755) == Err(CPR_ENOTDIR)
                && a.count() == 2
                && a.consistent()
                && l.sample(10).map(|x| x.code) == Some(CPR_ENOTDIR)
        },
        "static",
    );

    s.add(
        "CPR7-同级重名被拒",
        {
            let mut a = match Tree::open(FLAG_R) {
                Ok(v) => v,
                Err(_) => return s,
            };
            let _ = a.mkdir(b"a", 0, true, 0o755);
            a.mkdir(b"a", 0, false, 0o644) == Err(CPR_EDUP)
                && a.mkdir(b"a", 1, true, 0o700) == Ok(2)
                && a.count() == 3
                && a.consistent()
                && l.sample(11).map(|x| x.code) == Some(CPR_EDUP)
        },
        "static",
    );

    s.add(
        "CPR8-深度超限被拒",
        {
            let mut a = match Tree::open(FLAG_R) {
                Ok(v) => v,
                Err(_) => return s,
            };
            let _ = a.mkdir(b"a", 0, true, 0o755);
            let _ = a.mkdir(b"b", 1, true, 0o755);
            let _ = a.mkdir(b"c", 2, true, 0o755);
            a.mkdir(b"d", 3, true, 0o755) == Err(CPR_EDEPTH)
                && a.count() == 4
                && a.depth_of(3) == Some(3)
                && l.sample(12).map(|x| x.code) == Some(CPR_EDEPTH)
        },
        "static",
    );

    s.add(
        "CPR9-非法标志被拒",
        {
            let mut ok = Tree::open(0).is_err() && Tree::open(3).is_err();
            if let Ok(v) = Tree::open(0) {
                ok = v.count() == 0;
            }
            ok && l.sample(8).map(|x| x.code) == Some(CPR_EFLAG)
        },
        "static",
    );

    s.add(
        "CPR10-空树不可复制",
        {
            let mut a = match Tree::open(FLAG_R) {
                Ok(v) => v,
                Err(_) => return s,
            };
            a.copy() == Err(CPR_EEMPTY)
                && !a.is_copied()
                && a.count() == 1
                && l.sample(13).map(|x| x.code) == Some(CPR_EEMPTY)
        },
        "static",
    );

    s.add(
        "CPR11-节点表满被拒",
        {
            let mut a = match Tree::open(FLAG_R) {
                Ok(v) => v,
                Err(_) => return s,
            };
            let mut i = 1usize;
            while i < CAP {
                let name: [u8; 3] = [b'n', b'0' + (i / 10) as u8, b'0' + (i % 10) as u8];
                let _ = a.mkdir(&name, 0, false, 0o644);
                i += 1;
            }
            a.count() == CAP
                && a.mkdir(b"zz", 0, false, 0o644) == Err(CPR_EFULL)
                && a.consistent()
        },
        "static",
    );

    s.add(
        "CPR12-未复制不得等价",
        {
            let mut a = match Tree::open(FLAG_R) {
                Ok(v) => v,
                Err(_) => return s,
            };
            let mut b = match Tree::open(FLAG_R) {
                Ok(v) => v,
                Err(_) => return s,
            };
            let _ = a.mkdir(b"d", 0, true, 0o750);
            let _ = b.mkdir(b"d", 0, true, 0o750);
            let early = a.equiv(&b) == Err(CPR_ESHAPE);
            let _ = a.copy();
            let _ = b.copy();
            let eq = a.equiv(&b) == Ok(1);
            let _ = b.mkdir(b"e", 0, true, 0o700);
            let _ = b.copy();
            early && eq && a.equiv(&b) == Err(CPR_ESHAPE)
        },
        "static",
    );

    s.add(
        "CPR13-冻结重放四注入",
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

    fn tree(flag: u8) -> Tree {
        match Tree::open(flag) {
            Ok(v) => v,
            Err(_) => panic!("open"),
        }
    }

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
        let s = run_unx11388_checks();
        assert_eq!(s.len(), 13);
        assert!(s.all_passed());
    }

    #[test]
    fn empty_dirs_survive() {
        let mut a = tree(FLAG_R);
        assert_eq!(a.mkdir(b"empty", 0, true, 0o700), Ok(1));
        assert_eq!(a.mkdir(b"nested", 0, true, 0o755), Ok(2));
        assert_eq!(a.mkdir(b"inner", 2, true, 0o750), Ok(3));
        assert_eq!(a.copy(), Ok(4));
        assert_eq!(a.node_at(1).map(|x| x.mode()), Some(0o700));
        assert_eq!(a.node_at(3).map(|x| x.is_dir()), Some(true));
        assert!(a.consistent());
    }

    #[test]
    fn flag_equivalence() {
        let mut a = tree(FLAG_R);
        let mut b = tree(FLAG_RR);
        let _ = a.mkdir(b"d", 0, true, 0o750);
        let _ = b.mkdir(b"d", 0, true, 0o750);
        assert_eq!(a.copy(), Ok(2));
        assert_eq!(b.copy(), Ok(2));
        assert_eq!(a.effective(), b.effective());
        assert_eq!(a.equiv(&b), Ok(1));
        assert_eq!(a.shape_hash(), b.shape_hash());
    }

    #[test]
    fn flag_gate() {
        assert!(Tree::open(0).is_err());
        assert!(Tree::open(3).is_err());
        assert_eq!(tree(FLAG_R).effective(), FLAG_REC);
        assert_eq!(tree(FLAG_RR).flag(), FLAG_RR);
    }

    #[test]
    fn shape_mismatch() {
        let mut a = tree(FLAG_R);
        let mut b = tree(FLAG_R);
        let _ = a.mkdir(b"d", 0, true, 0o750);
        let _ = b.mkdir(b"d", 0, true, 0o700);
        assert_eq!(a.copy(), Ok(2));
        assert_eq!(b.copy(), Ok(2));
        assert_eq!(a.equiv(&b), Err(CPR_ESHAPE));
    }

    #[test]
    fn depth_limit() {
        let mut a = tree(FLAG_R);
        assert_eq!(a.mkdir(b"a", 0, true, 0o755), Ok(1));
        assert_eq!(a.mkdir(b"b", 1, true, 0o755), Ok(2));
        assert_eq!(a.mkdir(b"c", 2, true, 0o755), Ok(3));
        assert_eq!(a.mkdir(b"d", 3, true, 0o755), Err(CPR_EDEPTH));
    }

    #[test]
    fn inject_four_holds() {
        assert_eq!(inject_four(), [true; 4]);
    }
}
