//! UNX-F11413 chmod -R 递归与 X 大写特义
//!
//! `chmod -R` 不是把同一张掩码机械地套在整棵树上：大写 `X` 的语义取决于节点
//! 自身。规范把 `X` 定义为「目录，或原本已有任一执行位」才置执行位，于是同一
//! 份 `a+X` 在目录上等价于 `a+x`，在普通文件上却可能完全不动。递归账要同时
//! 守住三件事：
//!
//! 1. **树形约束**：根节点没有父；子节点只能挂在目录下；深度有上限，防止
//!    深层递归把遍历栈吃穿；
//! 2. **符号链接**：`-R` 默认不跟随（跳过、不改、不报错）；显式 `-L` 跟随
//!    时本账直接拒绝，因为跟随后可能走出子树甚至成环；
//! 3. **X 的两义**：目录必置执行位；文件只看「任一执行位是否存在」，而不是
//!    看 who 里的哪一档。
//!
//! 闸门次序：
//! - `add`：节点类型 → 权限位上界 → 表满 → 父节点存在 → 父节点是目录 → 深度；
//! - `apply`：模式串非空 → 起点存在 → 起点是目录 → 遍历中的深度 → 符号链接；
//! - `Spec::parse`：who 段结构 → 操作符 → 权限字母 → 子句数量。
//!
//! 账本契约：十四格，前八格成功、后六格失败；失败格依次覆盖
//! `ENODE/ЕKIND/EINVAL/EBITS/ESYM/EWALK` 六类，成功格记录节点号、子节点数
//! 与被改节点数。`audit` 逐格核对槽号、成功值与失败码。

use crate::alloc::format;
use crate::checks::CheckSet;

/// 段基址。
pub const SEG_BASE: u32 = 0xefd0;

/// 模式串结构非法。
pub const XR_EINVAL: u32 = SEG_BASE + 0x01;
/// 权限字母非法。
pub const XR_EBITS: u32 = SEG_BASE + 0x02;
/// 父节点不存在。
pub const XR_ENODE: u32 = SEG_BASE + 0x03;
/// 父节点不是目录。
pub const XR_EKIND: u32 = SEG_BASE + 0x04;
/// 节点表满。
pub const XR_EFULL: u32 = SEG_BASE + 0x05;
/// 深度超限。
pub const XR_EDEPTH: u32 = SEG_BASE + 0x06;
/// `-R` 下拒绝跟随符号链接。
pub const XR_ESYM: u32 = SEG_BASE + 0x07;
/// 遍历起点不是目录。
pub const XR_EWALK: u32 = SEG_BASE + 0x08;
/// 账本形状不符。
pub const XR_EBAL: u32 = SEG_BASE + 0x09;
/// 槽位乱序。
pub const XR_ESLOT: u32 = SEG_BASE + 0x0a;
/// 观测值与复算不符。
pub const XR_EVALUE: u32 = SEG_BASE + 0x0b;
/// 冻结后重放不一致。
pub const XR_ESEAL: u32 = SEG_BASE + 0x0c;
/// 观察下标越界。
pub const XR_ENOBS: u32 = SEG_BASE + 0x0d;

/// 节点表容量。
pub const CAP: usize = 8;
/// 账本格数。
pub const SAMPLES: usize = 14;
/// 深度上界。
pub const DEPTH_MAX: u8 = 5;
/// 子句数上限。
pub const MAX_CLAUSES: usize = 4;
/// 遍历栈容量。
const STACK: usize = CAP * 2;
/// 无父哨兵。
pub const NONE: u8 = 0xff;

/// 目录。
pub const KIND_DIR: u8 = 1;
/// 普通文件。
pub const KIND_FILE: u8 = 2;
/// 符号链接。
pub const KIND_LINK: u8 = 3;

/// 操作码：挂节点。
pub const OP_ADD: u8 = 1;
/// 操作码：数子节点。
pub const OP_COUNT: u8 = 2;
/// 操作码：解析模式串。
pub const OP_PARSE: u8 = 3;
/// 操作码：递归应用。
pub const OP_APPLY: u8 = 4;

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
    /// 节点号（即表下标）。
    pub id: u8,
    /// 父节点号；根为 `NONE`。
    pub parent: u8,
    /// 节点类型。
    pub kind: u8,
    /// 九位权限。
    pub mode: u16,
    /// 深度，根为 0。
    pub depth: u8,
}

/// 一条符号模式子句，例如 `u+x`、`go-w`、`a=X`。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Clause {
    /// who 位：1=u、2=g、4=o。
    pub who: u8,
    /// 操作符：`+`、`-`、`=`。
    pub op: u8,
    /// 普通权限位：4=r、2=w、1=x。
    pub bits: u8,
    /// 大写 `X` 是否存在。
    pub xcap: bool,
}

/// 最多四条子句的模式串。
#[derive(Clone, Copy, Debug)]
pub struct Spec {
    c: [Clause; MAX_CLAUSES],
    n: usize,
}

impl Spec {
    /// 空模式。
    pub const fn empty() -> Spec {
        Spec {
            c: [Clause { who: 0, op: 0, bits: 0, xcap: false }; MAX_CLAUSES],
            n: 0,
        }
    }

    /// 子句数。
    pub fn len(&self) -> usize {
        self.n
    }

    /// 第 i 条子句。
    pub fn clause(&self, i: usize) -> Option<Clause> {
        if i >= self.n {
            return None;
        }
        self.c.get(i).copied()
    }

    /// 解析形如 `a+X,g-w,o=r` 的模式串。
    ///
    /// 闸门次序：who 结构 → 操作符 → 权限字母 → 子句数。空串、缺 who、缺操作
    /// 符、缺权限字母、尾逗号都归 `EINVAL`；未知权限字母归 `EBITS`。
    pub fn parse(src: &[u8]) -> Result<Spec, u32> {
        if src.is_empty() {
            return Err(XR_EINVAL);
        }
        let mut sp = Spec::empty();
        let mut i = 0usize;
        while i < src.len() {
            let start = i;
            let mut who = 0u8;
            while i < src.len() {
                let ch = match src.get(i) {
                    Some(v) => *v,
                    None => return Err(XR_EINVAL),
                };
                if ch == b'+' || ch == b'-' || ch == b'=' {
                    break;
                }
                let bit = match ch {
                    b'u' => 1u8,
                    b'g' => 2u8,
                    b'o' => 4u8,
                    b'a' => 7u8,
                    _ => return Err(XR_EINVAL),
                };
                if who & bit != 0 {
                    return Err(XR_EINVAL);
                }
                who |= bit;
                i = i.saturating_add(1);
            }
            if i == start || i >= src.len() {
                return Err(XR_EINVAL);
            }
            let op = match src.get(i) {
                Some(v) => *v,
                None => return Err(XR_EINVAL),
            };
            if op != b'+' && op != b'-' && op != b'=' {
                return Err(XR_EINVAL);
            }
            i = i.saturating_add(1);
            let mut bits = 0u8;
            let mut xcap = false;
            let mut k = 0u8;
            while i < src.len() {
                let ch = match src.get(i) {
                    Some(v) => *v,
                    None => return Err(XR_EINVAL),
                };
                if ch == b',' {
                    break;
                }
                match ch {
                    b'r' => bits |= 4,
                    b'w' => bits |= 2,
                    b'x' => bits |= 1,
                    b'X' => {
                        if xcap {
                            return Err(XR_EINVAL);
                        }
                        xcap = true;
                    }
                    _ => return Err(XR_EBITS),
                }
                k = k.saturating_add(1);
                i = i.saturating_add(1);
            }
            if k == 0 {
                return Err(XR_EINVAL);
            }
            if sp.n >= MAX_CLAUSES {
                return Err(XR_EINVAL);
            }
            let clause = Clause { who, op, bits, xcap };
            if let Some(dst) = sp.c.get_mut(sp.n) {
                *dst = clause;
            }
            sp.n = sp.n.saturating_add(1);
            if i < src.len() && src.get(i) == Some(&b',') {
                i = i.saturating_add(1);
                if i == src.len() {
                    return Err(XR_EINVAL);
                }
            }
        }
        Ok(sp)
    }
}

/// 把三档权限按 who 展开到九位。
fn spread(who: u8, bits: u8) -> u16 {
    let b = (bits & 0o7) as u16;
    let mut m = 0u16;
    if who & 1 != 0 {
        m |= b << 6;
    }
    if who & 2 != 0 {
        m |= b << 3;
    }
    if who & 4 != 0 {
        m |= b;
    }
    m
}

/// 一棵权限树。
#[derive(Clone, Copy, Debug)]
pub struct Tree {
    nodes: [Node; CAP],
    used: usize,
    hits: u32,
    walks: u32,
}

impl Tree {
    /// 空树。
    pub const fn new() -> Tree {
        Tree {
            nodes: [Node { id: 0, parent: NONE, kind: 0, mode: 0, depth: 0 }; CAP],
            used: 0,
            hits: 0,
            walks: 0,
        }
    }

    /// 已挂节点数。
    pub fn len(&self) -> usize {
        self.used
    }

    /// 是否空树。
    pub fn is_empty(&self) -> bool {
        self.used == 0
    }

    /// 取节点。
    pub fn node(&self, id: u8) -> Option<Node> {
        if (id as usize) >= self.used {
            return None;
        }
        self.nodes.get(id as usize).copied()
    }

    /// 取权限；节点不存在归 `ENODE`。
    pub fn mode_at(&self, id: u8) -> Result<u32, u32> {
        match self.node(id) {
            Some(v) => Ok(v.mode as u32),
            None => Err(XR_ENODE),
        }
    }

    /// 累计被改节点数。
    pub fn changed_total(&self) -> u32 {
        self.hits
    }

    /// 累计遍历次数。
    pub fn walk_total(&self) -> u32 {
        self.walks
    }

    /// 数某节点的直接子节点。
    pub fn count_children(&self, id: u8) -> Result<usize, u32> {
        if self.node(id).is_none() {
            return Err(XR_ENODE);
        }
        let mut n = 0usize;
        let mut i = 0usize;
        while i < self.used {
            if let Some(v) = self.nodes.get(i) {
                if v.parent == id {
                    n = n.saturating_add(1);
                }
            }
            i = i.saturating_add(1);
        }
        Ok(n)
    }

    /// 挂一个节点，返回新节点号。
    ///
    /// 闸门次序：类型 → 权限位上界 → 表满 → 父存在 → 父是目录 → 深度。
    pub fn add(&mut self, parent: u8, kind: u8, mode: u32) -> Result<u8, u32> {
        if kind != KIND_DIR && kind != KIND_FILE && kind != KIND_LINK {
            return Err(XR_EKIND);
        }
        if mode > 0o777 {
            return Err(XR_EBITS);
        }
        if self.used >= CAP {
            return Err(XR_EFULL);
        }
        let depth;
        if parent == NONE {
            depth = 0u8;
        } else {
            let p = match self.node(parent) {
                Some(v) => v,
                None => return Err(XR_ENODE),
            };
            if p.kind != KIND_DIR {
                return Err(XR_EKIND);
            }
            depth = p.depth.saturating_add(1);
            if depth > DEPTH_MAX {
                return Err(XR_EDEPTH);
            }
        }
        let id = self.used as u8;
        let nd = Node { id, parent, kind, mode: mode as u16, depth };
        if let Some(dst) = self.nodes.get_mut(self.used) {
            *dst = nd;
        }
        self.used = self.used.saturating_add(1);
        Ok(id)
    }

    /// 对以 `root` 为起点的子树递归应用模式串，返回被改节点数。
    ///
    /// 闸门次序：模式串非空 → 起点存在 → 起点是目录 → 遍历深度 → 符号链接。
    pub fn apply(&mut self, root: u8, spec: &Spec, follow: bool) -> Result<u32, u32> {
        if spec.n == 0 {
            return Err(XR_EINVAL);
        }
        let r = match self.node(root) {
            Some(v) => v,
            None => return Err(XR_ENODE),
        };
        if r.kind != KIND_DIR {
            return Err(XR_EWALK);
        }
        let mut stack = [0u8; STACK];
        let mut top = 1usize;
        if let Some(dst) = stack.get_mut(0) {
            *dst = root;
        }
        let mut changed = 0u32;
        self.walks = self.walks.saturating_add(1);
        while top > 0 {
            top -= 1;
            let id = match stack.get(top) {
                Some(v) => *v,
                None => return Err(XR_EBAL),
            };
            let nd = match self.node(id) {
                Some(v) => v,
                None => return Err(XR_ENODE),
            };
            if nd.depth > DEPTH_MAX {
                return Err(XR_EDEPTH);
            }
            if nd.kind == KIND_LINK {
                if follow {
                    return Err(XR_ESYM);
                }
                continue;
            }
            let mut m = nd.mode;
            let mut ci = 0usize;
            while ci < spec.n {
                let c = match spec.c.get(ci) {
                    Some(v) => *v,
                    None => return Err(XR_EBAL),
                };
                let mut mask = spread(c.who, c.bits);
                if c.xcap {
                    let cond = nd.kind == KIND_DIR || (nd.mode & 0o111) != 0;
                    if cond {
                        mask |= spread(c.who, 0o1);
                    }
                }
                m = match c.op {
                    b'+' => m | mask,
                    b'-' => m & !mask,
                    b'=' => (m & !spread(c.who, 0o7)) | mask,
                    _ => return Err(XR_EINVAL),
                };
                ci = ci.saturating_add(1);
            }
            m &= 0o777;
            if m != nd.mode {
                if let Some(dst) = self.nodes.get_mut(id as usize) {
                    dst.mode = m;
                }
                self.hits = self.hits.saturating_add(1);
                changed = changed.saturating_add(1);
            }
            let mut k = CAP;
            while k > 0 {
                k -= 1;
                if k < self.used {
                    if let Some(ch) = self.nodes.get(k) {
                        if ch.parent == id {
                            if let Some(dst) = stack.get_mut(top) {
                                *dst = ch.id;
                            }
                            top = top.saturating_add(1);
                        }
                    }
                }
            }
        }
        Ok(changed)
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

/// `Result<u8, u32>` → 样本。
fn byte_cell(slot: u8, op: u8, r: Result<u8, u32>) -> Sample {
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
            return Err(XR_ESLOT);
        }
        if let Some(dst) = self.rows.get_mut(self.len) {
            *dst = s;
        }
        self.len = self.len.saturating_add(1);
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
            if let Some(v) = self.rows.get(i) {
                if v.ok {
                    n = n.saturating_add(1);
                }
            }
            i = i.saturating_add(1);
        }
        n
    }

    /// 失败格数。
    pub fn err_count(&self) -> usize {
        self.len.saturating_sub(self.ok_count())
    }

    /// 对账。
    pub fn audit(&self) -> Result<(), u32> {
        if self.len != SAMPLES || self.ok_count() != 8 || self.err_count() != 6 {
            return Err(XR_EBAL);
        }
        const VALS: [u32; 8] = [0, 1, 2, 3, 4, 4, 2, 3];
        const CODES: [u32; 6] =
            [XR_ENODE, XR_EKIND, XR_EINVAL, XR_EBITS, XR_ESYM, XR_EWALK];
        let mut vi = 0usize;
        let mut ci = 0usize;
        let mut i = 0usize;
        while i < self.len {
            let v = match self.rows.get(i) {
                Some(x) => *x,
                None => return Err(XR_EBAL),
            };
            if v.slot as usize != i {
                return Err(XR_ESLOT);
            }
            if v.ok {
                if v.code != 0 {
                    return Err(XR_EVALUE);
                }
                match VALS.get(vi) {
                    Some(w) => {
                        if v.value != *w {
                            return Err(XR_EVALUE);
                        }
                    }
                    None => return Err(XR_EBAL),
                }
                vi = vi.saturating_add(1);
            } else {
                match CODES.get(ci) {
                    Some(w) => {
                        if v.code != *w {
                            return Err(XR_EVALUE);
                        }
                    }
                    None => return Err(XR_EBAL),
                }
                ci = ci.saturating_add(1);
            }
            i = i.saturating_add(1);
        }
        if vi != 8 || ci != 6 {
            return Err(XR_EBAL);
        }
        Ok(())
    }

    /// 指纹。
    pub fn seal(&self) -> u64 {
        let mut h = FNV_OFF;
        let mut i = 0usize;
        while i < self.len {
            if let Some(v) = self.rows.get(i) {
                h = fold(h, v.slot as u64);
                h = fold(h, v.op as u64);
                h = fold(h, v.ok as u64);
                h = fold(h, v.code as u64);
                h = fold(h, v.value as u64);
            }
            i = i.saturating_add(1);
        }
        fold(h, self.rejects as u64)
    }

    /// 冻结。
    pub fn freeze(&mut self) -> Result<u64, u32> {
        if self.frozen {
            self.rejects = self.rejects.saturating_add(1);
            return Err(XR_EBAL);
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
            Err(XR_ESEAL)
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

/// 演示树：根 0750，子目录 0640，文件 0644/0755，链接 0777，内层文件 0600。
fn demo() -> Tree {
    let mut t = Tree::new();
    let _ = t.add(NONE, KIND_DIR, 0o750);
    let _ = t.add(0, KIND_DIR, 0o640);
    let _ = t.add(0, KIND_FILE, 0o644);
    let _ = t.add(0, KIND_FILE, 0o755);
    let _ = t.add(0, KIND_LINK, 0o777);
    let _ = t.add(1, KIND_FILE, 0o600);
    t
}

/// 十四格账本：五挂一数两递归 → 六类拒绝。
pub fn build_ledger() -> Ledger {
    let mut l = Ledger::new();
    let mut t = Tree::new();

    let _ = l.register(byte_cell(0, OP_ADD, t.add(NONE, KIND_DIR, 0o750)));
    let _ = l.register(byte_cell(1, OP_ADD, t.add(0, KIND_DIR, 0o640)));
    let _ = l.register(byte_cell(2, OP_ADD, t.add(0, KIND_FILE, 0o644)));
    let _ = l.register(byte_cell(3, OP_ADD, t.add(0, KIND_FILE, 0o755)));
    let _ = l.register(byte_cell(4, OP_ADD, t.add(0, KIND_LINK, 0o777)));
    let _ = l.register(cell(5, OP_COUNT, t.count_children(0)));

    let sp_x = match Spec::parse(b"a+X") {
        Ok(v) => v,
        Err(_) => return l,
    };
    let sp_u = match Spec::parse(b"u-x") {
        Ok(v) => v,
        Err(_) => return l,
    };
    let sp_r = match Spec::parse(b"a+r") {
        Ok(v) => v,
        Err(_) => return l,
    };

    let _ = l.register(word_cell(6, OP_APPLY, t.apply(0, &sp_x, false)));
    let _ = l.register(word_cell(7, OP_APPLY, t.apply(0, &sp_u, false)));

    let _ = l.register(byte_cell(8, OP_ADD, t.add(9, KIND_FILE, 0o644)));
    let _ = l.register(byte_cell(9, OP_ADD, t.add(2, KIND_DIR, 0o755)));
    let _ = l.register(word_cell(10, OP_PARSE, Spec::parse(b"").map(|p| p.len() as u32)));
    let _ = l.register(word_cell(11, OP_PARSE, Spec::parse(b"u+q").map(|p| p.len() as u32)));
    let _ = l.register(word_cell(12, OP_APPLY, t.apply(0, &sp_x, true)));
    let _ = l.register(word_cell(13, OP_APPLY, t.apply(2, &sp_r, false)));

    l
}

/// 四路注入。
pub fn inject_four() -> [bool; 4] {
    let mut out = [false; 4];

    let l = build_ledger();
    let rep = l.replay();
    let root = l.seal();
    out[0] = l.len() == SAMPLES && l.audit().is_ok() && rep == Ok(root) && root != 0;

    // 一路：X 在目录置位、在无 x 文件不动、在已有 x 文件补全。
    let sp_x = match Spec::parse(b"a+X") {
        Ok(v) => v,
        Err(_) => return out,
    };
    let mut t = demo();
    let c1 = t.apply(0, &sp_x, false);
    let dir_ok = t.mode_at(0) == Ok(0o751) && t.mode_at(1) == Ok(0o751);
    let file_ok = t.mode_at(2) == Ok(0o644) && t.mode_at(5) == Ok(0o600);
    let exec_ok = t.mode_at(3) == Ok(0o755);
    let mut u = Tree::new();
    let _ = u.add(NONE, KIND_DIR, 0o750);
    let _ = u.add(0, KIND_FILE, 0o610);
    let c2 = u.apply(0, &sp_x, false);
    out[1] = c1 == Ok(2)
        && dir_ok
        && file_ok
        && exec_ok
        && c2 == Ok(2)
        && u.mode_at(1) == Ok(0o711)
        && t.node(4).map(|n| n.kind) == Some(KIND_LINK)
        && t.mode_at(4) == Ok(0o777);

    // 二路：八类失败各自可辨。
    let mut b = demo();
    let sp_r = match Spec::parse(b"a+r") {
        Ok(v) => v,
        Err(_) => return out,
    };
    let empty = Spec::parse(b"").err() == Some(XR_EINVAL);
    let dup_who = Spec::parse(b"uu+x").err() == Some(XR_EINVAL);
    let bad_bit = Spec::parse(b"u+q").err() == Some(XR_EBITS);
    let missing_parent = b.add(9, KIND_FILE, 0o644) == Err(XR_ENODE);
    let under_file = b.add(2, KIND_DIR, 0o755) == Err(XR_EKIND);
    let bad_kind = b.add(0, 9, 0o755) == Err(XR_EKIND);
    let bad_mode = b.add(0, KIND_FILE, 0o1000) == Err(XR_EBITS);
    let follow_sym = b.apply(0, &sp_r, true) == Err(XR_ESYM);
    let not_dir = b.apply(2, &sp_r, false) == Err(XR_EWALK);
    let mut d = Tree::new();
    let _ = d.add(NONE, KIND_DIR, 0o755);
    let mut pid = 0u8;
    let mut k = 0u32;
    while k < DEPTH_MAX as u32 {
        if let Ok(id) = d.add(pid, KIND_DIR, 0o755) {
            pid = id;
        }
        k = k.saturating_add(1);
    }
    let deep = d.add(pid, KIND_DIR, 0o755) == Err(XR_EDEPTH);
    let mut f = Tree::new();
    let _ = f.add(NONE, KIND_DIR, 0o755);
    let mut j = 0u32;
    while j < (CAP as u32 - 1) {
        let _ = f.add(0, KIND_FILE, 0o644);
        j = j.saturating_add(1);
    }
    let full = f.add(0, KIND_FILE, 0o644) == Err(XR_EFULL);
    out[2] = empty
        && dup_who
        && bad_bit
        && missing_parent
        && under_file
        && bad_kind
        && bad_mode
        && follow_sym
        && not_dir
        && deep
        && full;

    // 三路：符号子句逐层运算与整棵子树隔离。
    let mut q = demo();
    let sp_go = match Spec::parse(b"go-rwx") {
        Ok(v) => v,
        Err(_) => return out,
    };
    let r1 = q.apply(0, &sp_go, false);
    let sp_ow = match Spec::parse(b"o+w") {
        Ok(v) => v,
        Err(_) => return out,
    };
    let r2 = q.apply(0, &sp_ow, false);
    let sp_ar = match Spec::parse(b"a=r") {
        Ok(v) => v,
        Err(_) => return out,
    };
    let r3 = q.apply(0, &sp_ar, false);
    out[3] = r1 == Ok(4)
        && r2 == Ok(5)
        && r3 == Ok(5)
        && q.mode_at(0) == Ok(0o444)
        && q.mode_at(2) == Ok(0o444)
        && q.mode_at(4) == Ok(0o777)
        && q.count_children(0) == Ok(4)
        && q.changed_total() == 14;

    out
}

/// 十三项判据。
pub fn run_unx11413_checks() -> CheckSet {
    let mut s = CheckSet::new("unx11413.chmod_recursive_x");
    let l = build_ledger();

    s.add_dyn(
        "XR1-十四格八允六拒",
        l.len() == SAMPLES && l.ok_count() == 8 && l.err_count() == 6 && l.audit().is_ok(),
        &format!("n={} ok={} err={}", l.len(), l.ok_count(), l.err_count()),
    );

    s.add(
        "XR2-X在目录上置位",
        {
            let mut t = demo();
            let sp = match Spec::parse(b"a+X") {
                Ok(v) => v,
                Err(_) => return s,
            };
            t.apply(0, &sp, false) == Ok(2)
                && t.mode_at(0) == Ok(0o751)
                && t.mode_at(1) == Ok(0o751)
                && l.sample(6).map(|x| x.value) == Some(2)
        },
        "static",
    );

    s.add(
        "XR3-X对无x文件不动",
        {
            let mut t = demo();
            let sp = match Spec::parse(b"a+X") {
                Ok(v) => v,
                Err(_) => return s,
            };
            let _ = t.apply(0, &sp, false);
            t.mode_at(2) == Ok(0o644)
                && t.mode_at(5) == Ok(0o600)
                && t.node(2).map(|n| n.kind) == Some(KIND_FILE)
        },
        "static",
    );

    s.add(
        "XR4-X对已有x文件补全",
        {
            let mut t = Tree::new();
            let _ = t.add(NONE, KIND_DIR, 0o750);
            let _ = t.add(0, KIND_FILE, 0o610);
            let sp = match Spec::parse(b"a+X") {
                Ok(v) => v,
                Err(_) => return s,
            };
            t.apply(0, &sp, false) == Ok(2) && t.mode_at(1) == Ok(0o711)
        },
        "static",
    );

    s.add(
        "XR5-递归覆盖与兄弟隔离",
        {
            let mut t = demo();
            let sp = match Spec::parse(b"go-r") {
                Ok(v) => v,
                Err(_) => return s,
            };
            let c = t.apply(1, &sp, false);
            c == Ok(1)
                && t.mode_at(1) == Ok(0o600)
                && t.mode_at(5) == Ok(0o600)
                && t.mode_at(2) == Ok(0o644)
                && t.mode_at(0) == Ok(0o750)
        },
        "static",
    );

    s.add(
        "XR6-符号链接默认跳过",
        {
            let mut t = demo();
            let sp = match Spec::parse(b"a+r") {
                Ok(v) => v,
                Err(_) => return s,
            };
            let c = t.apply(0, &sp, false);
            c == Ok(3)
                && t.mode_at(4) == Ok(0o777)
                && t.node(4).map(|n| n.kind) == Some(KIND_LINK)
        },
        "static",
    );

    s.add(
        "XR7-跟随符号链接被拒",
        {
            let mut t = demo();
            let sp = match Spec::parse(b"a+r") {
                Ok(v) => v,
                Err(_) => return s,
            };
            t.apply(0, &sp, true) == Err(XR_ESYM)
                && t.mode_at(0) == Ok(0o754)
                && l.sample(12).map(|x| x.code) == Some(XR_ESYM)
        },
        "static",
    );

    s.add(
        "XR8-深度超限被拒",
        {
            let mut t = Tree::new();
            let _ = t.add(NONE, KIND_DIR, 0o755);
            let mut pid = 0u8;
            let mut k = 0u32;
            while k < DEPTH_MAX as u32 {
                if let Ok(id) = t.add(pid, KIND_DIR, 0o755) {
                    pid = id;
                }
                k = k.saturating_add(1);
            }
            let over = t.add(pid, KIND_DIR, 0o755);
            over == Err(XR_EDEPTH)
                && t.len() == (DEPTH_MAX as usize + 1)
                && t.node(pid).map(|n| n.depth) == Some(DEPTH_MAX)
        },
        "static",
    );

    s.add(
        "XR9-表满被拒且不越界",
        {
            let mut t = Tree::new();
            let _ = t.add(NONE, KIND_DIR, 0o755);
            let mut j = 0u32;
            while j < (CAP as u32 - 1) {
                let _ = t.add(0, KIND_FILE, 0o644);
                j = j.saturating_add(1);
            }
            t.len() == CAP
                && t.add(0, KIND_FILE, 0o644) == Err(XR_EFULL)
                && t.count_children(0) == Ok(CAP - 1)
        },
        "static",
    );

    s.add(
        "XR10-父节点与起点闸门",
        {
            let mut t = demo();
            let sp = match Spec::parse(b"a+r") {
                Ok(v) => v,
                Err(_) => return s,
            };
            t.add(9, KIND_FILE, 0o644) == Err(XR_ENODE)
                && t.add(2, KIND_DIR, 0o755) == Err(XR_EKIND)
                && t.add(0, 9, 0o755) == Err(XR_EKIND)
                && t.add(0, KIND_FILE, 0o1000) == Err(XR_EBITS)
                && t.apply(2, &sp, false) == Err(XR_EWALK)
                && t.apply(99, &sp, false) == Err(XR_ENODE)
                && l.sample(8).map(|x| x.code) == Some(XR_ENODE)
                && l.sample(9).map(|x| x.code) == Some(XR_EKIND)
        },
        "static",
    );

    s.add(
        "XR11-模式串结构闸门",
        {
            let empty = Spec::parse(b"").err() == Some(XR_EINVAL);
            let no_op = Spec::parse(b"u").err() == Some(XR_EINVAL);
            let no_bit = Spec::parse(b"u+").err() == Some(XR_EINVAL);
            let tail = Spec::parse(b"u+x,").err() == Some(XR_EINVAL);
            let dup = Spec::parse(b"uu+x").err() == Some(XR_EINVAL);
            let bad = Spec::parse(b"u+q").err() == Some(XR_EBITS);
            let many = Spec::parse(b"u+x,u+r,u+w,u+r,u+x").err() == Some(XR_EINVAL);
            empty
                && no_op
                && no_bit
                && tail
                && dup
                && bad
                && many
                && l.sample(10).map(|x| x.code) == Some(XR_EINVAL)
                && l.sample(11).map(|x| x.code) == Some(XR_EBITS)
        },
        "static",
    );

    s.add(
        "XR12-三类符号运算合成",
        {
            let mut t = demo();
            let sp = match Spec::parse(b"u=rwx,g=rx,o-rwx") {
                Ok(v) => v,
                Err(_) => return s,
            };
            let c = t.apply(0, &sp, false);
            c == Ok(4)
                && t.mode_at(0) == Ok(0o750)
                && t.mode_at(2) == Ok(0o750)
                && t.mode_at(4) == Ok(0o777)
                && t.mode_at(5) == Ok(0o750)
                && t.changed_total() == 4
        },
        "static",
    );

    s.add(
        "XR13-冻结重放四注入",
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

    fn spec(src: &[u8]) -> Spec {
        match Spec::parse(src) {
            Ok(v) => v,
            Err(_) => panic!("spec"),
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
        assert_ne!(l.seal(), 0);
        assert_eq!(l.replay(), Ok(l.seal()));
        let mut i = 0usize;
        while i < l.len() {
            assert_eq!(l.sample(i).map(|x| x.slot as usize), Some(i));
            i += 1;
        }
    }

    #[test]
    fn checks_pass() {
        let s = run_unx11413_checks();
        assert_eq!(s.len(), 13);
        assert!(s.all_passed());
    }

    #[test]
    fn x_semantics() {
        let mut t = demo();
        assert_eq!(t.apply(0, &spec(b"a+X"), false), Ok(2));
        assert_eq!(t.mode_at(0), Ok(0o751));
        assert_eq!(t.mode_at(1), Ok(0o751));
        assert_eq!(t.mode_at(2), Ok(0o644));
        assert_eq!(t.mode_at(3), Ok(0o755));
        assert_eq!(t.mode_at(5), Ok(0o600));
    }

    #[test]
    fn recursion_isolated() {
        let mut t = demo();
        assert_eq!(t.apply(1, &spec(b"go-r"), false), Ok(1));
        assert_eq!(t.mode_at(1), Ok(0o600));
        assert_eq!(t.mode_at(2), Ok(0o644));
        assert_eq!(t.mode_at(0), Ok(0o750));
    }

    #[test]
    fn symlink_policy() {
        let mut t = demo();
        assert_eq!(t.apply(0, &spec(b"a+r"), false), Ok(3));
        assert_eq!(t.mode_at(4), Ok(0o777));
        assert_eq!(t.apply(0, &spec(b"a+r"), true), Err(XR_ESYM));
    }

    #[test]
    fn depth_and_capacity() {
        let mut t = Tree::new();
        assert_eq!(t.add(NONE, KIND_DIR, 0o755), Ok(0));
        let mut pid = 0u8;
        let mut k = 0u32;
        while k < DEPTH_MAX as u32 {
            match t.add(pid, KIND_DIR, 0o755) {
                Ok(id) => pid = id,
                Err(_) => panic!("add"),
            }
            k += 1;
        }
        assert_eq!(t.add(pid, KIND_DIR, 0o755), Err(XR_EDEPTH));
        let mut f = Tree::new();
        assert_eq!(f.add(NONE, KIND_DIR, 0o755), Ok(0));
        let mut j = 0u32;
        while j < (CAP as u32 - 1) {
            assert_eq!(f.add(0, KIND_FILE, 0o644), Ok((j + 1) as u8));
            j += 1;
        }
        assert_eq!(f.add(0, KIND_FILE, 0o644), Err(XR_EFULL));
    }

    #[test]
    fn spec_errors_and_ops() {
        assert_eq!(Spec::parse(b"").err(), Some(XR_EINVAL));
        assert_eq!(Spec::parse(b"u+").err(), Some(XR_EINVAL));
        assert_eq!(Spec::parse(b"u+q").err(), Some(XR_EBITS));
        assert_eq!(Spec::parse(b"u+x,").err(), Some(XR_EINVAL));
        let mut t = demo();
        assert_eq!(t.apply(0, &spec(b"go-rwx"), false), Ok(4));
        assert_eq!(t.mode_at(2), Ok(0o600));
        assert_eq!(t.apply(0, &spec(b"o+w"), false), Ok(5));
        assert_eq!(t.mode_at(0), Ok(0o702));
        assert_eq!(t.apply(0, &spec(b"a=r"), false), Ok(5));
        assert_eq!(t.mode_at(3), Ok(0o444));
    }

    #[test]
    fn inject_four_holds() {
        assert_eq!(inject_four(), [true; 4]);
    }
}
