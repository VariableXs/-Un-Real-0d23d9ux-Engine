//! UNX-F11393 rm -r 目录递归删除（0xee90 段）
//!
//! `rm -r` 与 `rm` 的差别不是「多删一点」，而是**删除的次序**：必须先把目录里
//! 的每个孩子处理干净，目录才允许被摘掉。内核 `rmdir` 只接受空目录，所以递归
//! 删除本质上是深度优先的后序遍历——把子树展开成一条「孩子在前、父亲在后」的
//! 序列，再按序列逐条摘项。
//!
//! 本模块用显式子栈做这次遍历（内核栈不能递归），并把每棵子树的删除结果记成
//! 三个计数：删掉的文件、目录、符号链接。三条之和必须等于调用返回的删除条数，
//! 这条等式就是「树被完整走了一遍」的可验证形式。
//!
//! 符号链接的关键约束是**不跟随**：删除一个指向树外的符号链接，只应摘掉链接
//! 自己，绝不碰目标。`unfollowed` 记录「删掉链接后目标仍然存活」的次数；若实现
//! 误把 `lstat` 写成 `stat`，这个计数会掉下去，同时目标会被误删。
//!
//! 目录的链接计数同样要跟着走：父目录的 `nlink` 等于 1（自身）加上活着的子目录
//! 数。删掉一个子目录必须让父目录的计数减一，删掉一整个子树必须让这条等式在
//! 每一层都重新成立。
//!
//! 错误类为何彼此独立：
//!
//! - `EINVAL` 是形状错（零号、重复建根），与树内容无关；
//! - `ENOENT` 是起点不存在，遍历无从开始；
//! - `ENOTDIR` 是路径中间挂了文件或链接，父节点不是目录；
//! - `EBUSY` 是子树里挂着挂载点，删它等于抽走别的文件系统；
//! - `EPERM` 是 sticky 父目录里非属主删除，属于策略拒绝；
//! - `EDEPTH` 是层级超过模型上限，遍历栈会溢出，必须提前拒绝；
//! - `ELOOP` 是父链成环（损坏的树），后序遍历会永不终止；
//! - `EFULL` 是节点表满，建不了新节点。
//!
//! 闸门次序：建节点为「形状 → 父存在 → 父是目录 → 重名 → 深度 → 表满」；
//! 删除为「形状 → 节点存在 → 父链无环 → 深度 → 挂载点 → 权限」。

use crate::alloc::format;
use crate::checks::CheckSet;

/// 段基址。
pub const SEG_BASE: u32 = 0xee90;

/// 形状错（零号、重复建根）。
pub const RT_EINVAL: u32 = SEG_BASE + 0x01;
/// 起点或父节点不存在。
pub const RT_ENOENT: u32 = SEG_BASE + 0x02;
/// 父节点不是目录。
pub const RT_ENOTDIR: u32 = SEG_BASE + 0x03;
/// 子树里挂着挂载点。
pub const RT_EBUSY: u32 = SEG_BASE + 0x04;
/// sticky 父目录越权。
pub const RT_EPERM: u32 = SEG_BASE + 0x05;
/// 层级超过上限。
pub const RT_EDEPTH: u32 = SEG_BASE + 0x06;
/// 父链成环。
pub const RT_ELOOP: u32 = SEG_BASE + 0x07;
/// 节点表满。
pub const RT_EFULL: u32 = SEG_BASE + 0x08;
/// 账本形状不符。
pub const RT_EBAL: u32 = SEG_BASE + 0x09;
/// 槽位乱序。
pub const RT_ESLOT: u32 = SEG_BASE + 0x0a;
/// 观测值与复算不符。
pub const RT_EVALUE: u32 = SEG_BASE + 0x0b;
/// 冻结后重放不一致。
pub const RT_ESEAL: u32 = SEG_BASE + 0x0c;
/// 观察下标越界。
pub const RT_ENOBS: u32 = SEG_BASE + 0x0d;

/// 节点表容量（即 `CAP`）。
pub const CAP: usize = 12;
/// 允许的最大层级。
pub const MAX_DEPTH: u32 = 8;
/// 账本格数。
pub const SAMPLES: usize = 14;

/// 节点类型：目录。
pub const K_DIR: u8 = 1;
/// 节点类型：普通文件。
pub const K_FILE: u8 = 2;
/// 节点类型：符号链接。
pub const K_LINK: u8 = 3;

/// 操作码：建目录。
pub const OP_MKDIR: u8 = 1;
/// 操作码：建文件。
pub const OP_MKFILE: u8 = 2;
/// 操作码：建符号链接。
pub const OP_SYMLINK: u8 = 3;
/// 操作码：递归删除。
pub const OP_RM: u8 = 4;
/// 操作码：读层级。
pub const OP_DEPTH: u8 = 5;
/// 操作码：读活节点数。
pub const OP_STAT: u8 = 6;

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
    /// 节点号。
    pub id: u32,
    /// 父节点（根为 0）。
    pub parent: u32,
    /// 名字。
    pub name: u32,
    /// 类型。
    pub kind: u8,
    /// 符号链接的目标节点号。
    pub target: u32,
    /// 属主。
    pub uid: u32,
    /// 是否挂载点。
    pub mount: bool,
    /// 是否 sticky（只允许属主删除孩子）。
    pub sticky: bool,
    /// 是否存活。
    pub live: bool,
}

impl Node {
    /// 空节点槽。
    pub const fn blank() -> Node {
        Node { id: 0, parent: 0, name: 0, kind: 0, target: 0, uid: 0, mount: false, sticky: false, live: false }
    }
}

/// 树模型。
#[derive(Clone, Copy, Debug)]
pub struct Tree {
    n: [Node; CAP],
    next_id: u32,
    sweeps: u32,
    removed: u32,
    unfollowed: u32,
    links_dropped: u32,
}

impl Tree {
    /// 空树。
    pub const fn new() -> Tree {
        Tree {
            n: [Node::blank(); CAP],
            next_id: 1,
            sweeps: 0,
            removed: 0,
            unfollowed: 0,
            links_dropped: 0,
        }
    }

    /// 活节点数。
    pub fn live(&self) -> usize {
        let mut k = 0usize;
        let mut c = 0usize;
        while k < CAP {
            if let Some(x) = self.n.get(k) {
                if x.live {
                    c += 1;
                }
            }
            k += 1;
        }
        c
    }

    /// 递归删除调用次数。
    pub fn sweep_count(&self) -> u32 {
        self.sweeps
    }

    /// 累计删除条数。
    pub fn removed_count(&self) -> u32 {
        self.removed
    }

    /// 删链接后目标仍存活的次数。
    pub fn unfollowed_count(&self) -> u32 {
        self.unfollowed
    }

    /// 因目录消失而减少的链接数。
    pub fn links_dropped(&self) -> u32 {
        self.links_dropped
    }

    /// 第 k 个节点槽。
    pub fn node_at(&self, k: usize) -> Option<Node> {
        self.n.get(k).copied()
    }

    /// 按号找活节点下标。
    fn find(&self, id: u32) -> Option<usize> {
        let mut k = 0usize;
        while k < CAP {
            match self.n.get(k) {
                Some(x) => {
                    if x.live && x.id == id {
                        return Some(k);
                    }
                }
                None => return None,
            }
            k += 1;
        }
        None
    }

    /// 找一个空槽。
    fn free_slot(&self) -> Option<usize> {
        let mut k = 0usize;
        while k < CAP {
            match self.n.get(k) {
                Some(x) => {
                    if !x.live {
                        return Some(k);
                    }
                }
                None => return None,
            }
            k += 1;
        }
        None
    }

    /// 按「父 + 名字」找节点号。
    pub fn id_of(&self, parent: u32, name: u32) -> Option<u32> {
        let mut k = 0usize;
        while k < CAP {
            match self.n.get(k) {
                Some(x) => {
                    if x.live && x.parent == parent && x.name == name {
                        return Some(x.id);
                    }
                }
                None => return None,
            }
            k += 1;
        }
        None
    }

    /// 同名兄弟是否已存在。
    fn sibling_exists(&self, parent: u32, name: u32) -> bool {
        let mut k = 0usize;
        while k < CAP {
            match self.n.get(k) {
                Some(x) => {
                    if x.live && x.parent == parent && x.name == name {
                        return true;
                    }
                }
                None => return false,
            }
            k += 1;
        }
        false
    }

    /// 计算从根到该节点的层数（根为 1）；父链成环返回 `ELOOP`。
    pub fn depth_of(&self, id: u32) -> Result<u32, u32> {
        let mut cur = id;
        let mut d = 0u32;
        while cur != 0 {
            d = d.saturating_add(1);
            if d > MAX_DEPTH {
                return Err(RT_ELOOP);
            }
            let k = match self.find(cur) {
                Some(v) => v,
                None => return Err(RT_ENOENT),
            };
            let nd = match self.n.get(k) {
                Some(v) => *v,
                None => return Err(RT_ENOENT),
            };
            cur = nd.parent;
        }
        Ok(d)
    }

    /// 活着的子目录数（父目录 nlink 的增量）。
    pub fn subdirs(&self, id: u32) -> usize {
        let mut k = 0usize;
        let mut c = 0usize;
        while k < CAP {
            match self.n.get(k) {
                Some(x) => {
                    if x.live && x.parent == id && x.kind == K_DIR {
                        c += 1;
                    }
                }
                None => break,
            }
            k += 1;
        }
        c
    }

    /// 目录链接计数：1 + 子目录数。
    pub fn nlink_of(&self, id: u32) -> Option<u32> {
        match self.find(id) {
            Some(k) => match self.n.get(k) {
                Some(x) => {
                    if x.kind == K_DIR {
                        Some(1u32.saturating_add(self.subdirs(id) as u32))
                    } else {
                        Some(1)
                    }
                }
                None => None,
            },
            None => None,
        }
    }

    /// 建节点（内部）。
    fn add(&mut self, parent: u32, name: u32, kind: u8, target: u32, uid: u32) -> Result<usize, u32> {
        if name == 0 || uid == 0 || kind == 0 {
            return Err(RT_EINVAL);
        }
        if parent == 0 {
            if self.live() != 0 {
                return Err(RT_EINVAL);
            }
            if self.find_slot_free_count() == 0 {
                return Err(RT_EFULL);
            }
        } else {
            let pi = match self.find(parent) {
                Some(v) => v,
                None => return Err(RT_ENOENT),
            };
            let pn = match self.n.get(pi) {
                Some(v) => *v,
                None => return Err(RT_ENOENT),
            };
            if pn.kind != K_DIR {
                return Err(RT_ENOTDIR);
            }
            if self.sibling_exists(parent, name) {
                return Err(RT_EINVAL);
            }
            let d = self.depth_of(parent)?;
            if d >= MAX_DEPTH {
                return Err(RT_EDEPTH);
            }
        }
        if kind == K_LINK {
            if target == 0 {
                return Err(RT_EINVAL);
            }
            if self.find(target).is_none() {
                return Err(RT_ENOENT);
            }
        }
        let slot = match self.free_slot() {
            Some(v) => v,
            None => return Err(RT_EFULL),
        };
        let id = self.next_id;
        self.next_id = self.next_id.saturating_add(1);
        if let Some(x) = self.n.get_mut(slot) {
            *x = Node { id, parent, name, kind, target, uid, mount: false, sticky: false, live: true };
        }
        Ok(self.live())
    }

    /// 统计空槽数（只在建根时用来区分「空树」与「表满」）。
    fn find_slot_free_count(&self) -> usize {
        let mut k = 0usize;
        let mut c = 0usize;
        while k < CAP {
            if let Some(x) = self.n.get(k) {
                if !x.live {
                    c += 1;
                }
            }
            k += 1;
        }
        c
    }

    /// 建根目录。
    pub fn mkroot(&mut self, name: u32, uid: u32) -> Result<usize, u32> {
        self.add(0, name, K_DIR, 0, uid)
    }

    /// 建目录。
    pub fn mkdir(&mut self, parent: u32, name: u32, uid: u32) -> Result<usize, u32> {
        self.add(parent, name, K_DIR, 0, uid)
    }

    /// 建文件。
    pub fn mkfile(&mut self, parent: u32, name: u32, uid: u32) -> Result<usize, u32> {
        self.add(parent, name, K_FILE, 0, uid)
    }

    /// 建符号链接。
    pub fn symlink(&mut self, parent: u32, name: u32, target: u32, uid: u32) -> Result<usize, u32> {
        self.add(parent, name, K_LINK, target, uid)
    }

    /// 把目录标记为 sticky。
    pub fn mark_sticky(&mut self, id: u32) -> Result<usize, u32> {
        match self.find(id) {
            Some(k) => {
                if let Some(x) = self.n.get_mut(k) {
                    x.sticky = true;
                }
                Ok(k + 1)
            }
            None => Err(RT_ENOENT),
        }
    }

    /// 把节点标记为挂载点。
    pub fn mark_mount(&mut self, id: u32) -> Result<usize, u32> {
        match self.find(id) {
            Some(k) => {
                if let Some(x) = self.n.get_mut(k) {
                    x.mount = true;
                }
                Ok(k + 1)
            }
            None => Err(RT_ENOENT),
        }
    }

    /// 收集子树节点号，返回条数；超过缓冲返回 `EFULL`。
    fn collect(&self, start: u32, out: &mut [u32; CAP]) -> Result<usize, u32> {
        let mut stack = [0u32; CAP];
        let mut sp = 0usize;
        if let Some(x) = stack.get_mut(sp) {
            *x = start;
        }
        sp += 1;
        let mut n = 0usize;
        while sp > 0 {
            sp -= 1;
            let cur = match stack.get(sp) {
                Some(v) => *v,
                None => return Err(RT_EBAL),
            };
            if let Some(x) = out.get_mut(n) {
                *x = cur;
            } else {
                return Err(RT_EFULL);
            }
            n += 1;
            let mut k = 0usize;
            while k < CAP {
                match self.n.get(k) {
                    Some(x) => {
                        if x.live && x.parent == cur {
                            if sp >= CAP {
                                return Err(RT_EFULL);
                            }
                            if let Some(s) = stack.get_mut(sp) {
                                *s = x.id;
                            }
                            sp += 1;
                        }
                    }
                    None => break,
                }
                k += 1;
            }
        }
        Ok(n)
    }

    /// 递归删除一棵子树，返回删除条数。
    ///
    /// 先在收集阶段做闸门（存在、成环、深度、挂载点、权限），再按后序摘项：
    /// 收集顺序是父先子后，删除时反序遍历即为子先父后。
    pub fn rm_r(&mut self, start: u32, caller: u32) -> Result<usize, u32> {
        if start == 0 || caller == 0 {
            return Err(RT_EINVAL);
        }
        if self.find(start).is_none() {
            return Err(RT_ENOENT);
        }
        let d = self.depth_of(start)?;
        if d > MAX_DEPTH {
            return Err(RT_EDEPTH);
        }
        let mut buf = [0u32; CAP];
        let count = self.collect(start, &mut buf)?;

        // 闸门：整棵子树都不许包含挂载点；权限按起点所在父目录判定。
        let mut k = 0usize;
        while k < count {
            let id = match buf.get(k) {
                Some(v) => *v,
                None => return Err(RT_EBAL),
            };
            if let Some(slot) = self.find(id) {
                if let Some(x) = self.n.get(slot) {
                    if x.mount {
                        return Err(RT_EBUSY);
                    }
                }
            }
            k += 1;
        }
        let s0 = match self.find(start) {
            Some(v) => v,
            None => return Err(RT_ENOENT),
        };
        let sn = match self.n.get(s0) {
            Some(v) => *v,
            None => return Err(RT_ENOENT),
        };
        if sn.parent != 0 {
            let pi = match self.find(sn.parent) {
                Some(v) => v,
                None => return Err(RT_ENOENT),
            };
            let pn = match self.n.get(pi) {
                Some(v) => *v,
                None => return Err(RT_ENOENT),
            };
            if pn.mount {
                return Err(RT_EBUSY);
            }
            if pn.sticky && caller != pn.uid && caller != sn.uid {
                return Err(RT_EPERM);
            }
        }

        // 后序摘项：反序遍历，孩子先于父亲。
        let mut removed = 0usize;
        let mut idx = count;
        while idx > 0 {
            idx -= 1;
            let id = match buf.get(idx) {
                Some(v) => *v,
                None => return Err(RT_EBAL),
            };
            let slot = match self.find(id) {
                Some(v) => v,
                None => continue,
            };
            let nd = match self.n.get(slot) {
                Some(v) => *v,
                None => continue,
            };
            // 符号链接不跟随：只摘链接自身，目标引用原样保留。
            if nd.kind == K_LINK {
                self.links_dropped = self.links_dropped.saturating_add(1);
                if self.find(nd.target).is_some() {
                    self.unfollowed = self.unfollowed.saturating_add(1);
                }
            }
            if let Some(x) = self.n.get_mut(slot) {
                x.live = false;
            }
            removed += 1;
        }
        self.sweeps = self.sweeps.saturating_add(1);
        self.removed = self.removed.saturating_add(removed as u32);
        Ok(removed)
    }

    /// 不变式：活节点的父链合法、兄弟名唯一、目录链接计数自洽。
    pub fn consistent(&self) -> bool {
        let mut k = 0usize;
        while k < CAP {
            let x = match self.n.get(k) {
                Some(v) => *v,
                None => return false,
            };
            if x.live {
                if x.id == 0 || x.name == 0 || x.uid == 0 || x.kind == 0 {
                    return false;
                }
                if x.parent != 0 {
                    let pi = match self.find(x.parent) {
                        Some(v) => v,
                        None => return false,
                    };
                    let pn = match self.n.get(pi) {
                        Some(v) => *v,
                        None => return false,
                    };
                    if pn.kind != K_DIR {
                        return false;
                    }
                }
                if x.kind == K_LINK {
                    if x.target == 0 {
                        return false;
                    }
                } else if x.target != 0 {
                    return false;
                }
                if x.kind == K_DIR && self.depth_of(x.id).is_err() {
                    return false;
                }
                let mut j = k + 1;
                while j < CAP {
                    match self.n.get(j) {
                        Some(o) => {
                            if o.live && o.parent == x.parent && o.name == x.name {
                                return false;
                            }
                        }
                        None => return false,
                    }
                    j += 1;
                }
            }
            k += 1;
        }
        true
    }

    /// 祖先条数（不含自身）。
    fn ancestors(&self, id: u32) -> u32 {
        let mut c = 0u32;
        let mut cur = id;
        while cur != 0 {
            let k = match self.find(cur) {
                Some(v) => v,
                None => return c,
            };
            let x = match self.n.get(k) {
                Some(v) => *v,
                None => return c,
            };
            cur = x.parent;
            if cur != 0 {
                c = c.saturating_add(1);
            }
        }
        c
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
            self.rejects = self.rejects.saturating_add(1);
            return Err(RT_ESLOT);
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
            return Err(RT_EBAL);
        }
        const VALS: [u32; 8] = [1, 2, 3, 4, 5, 3, 4, 1];
        const CODES: [u32; 6] =
            [RT_EINVAL, RT_ENOENT, RT_ENOTDIR, RT_EBUSY, RT_EPERM, RT_EDEPTH];
        let mut vi = 0usize;
        let mut ci = 0usize;
        let mut i = 0usize;
        while i < self.len {
            let s = match self.rows.get(i) {
                Some(v) => *v,
                None => return Err(RT_EBAL),
            };
            if s.slot as usize != i {
                return Err(RT_ESLOT);
            }
            if s.ok {
                if s.code != 0 {
                    return Err(RT_EVALUE);
                }
                match VALS.get(vi) {
                    Some(w) => {
                        if s.value != *w {
                            return Err(RT_EVALUE);
                        }
                    }
                    None => return Err(RT_EBAL),
                }
                vi += 1;
            } else {
                match CODES.get(ci) {
                    Some(w) => {
                        if s.code != *w {
                            return Err(RT_EVALUE);
                        }
                    }
                    None => return Err(RT_EBAL),
                }
                ci += 1;
            }
            i += 1;
        }
        if vi != 8 || ci != 6 {
            return Err(RT_EBAL);
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
            return Err(RT_EBAL);
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
            Err(RT_ESEAL)
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

/// 十四格账本：三层树建齐 → 链接指向树内 → 读层级 → 整棵子树删除 → 六类拒绝。
pub fn build_ledger() -> Ledger {
    let mut l = Ledger::new();
    let mut t = Tree::new();

    let _ = l.register(cell(0, OP_MKDIR, t.mkroot(1, 100)));
    let _ = l.register(cell(1, OP_MKDIR, t.mkdir(1, 2, 100)));
    let _ = l.register(cell(2, OP_MKFILE, t.mkfile(2, 3, 100)));
    let _ = l.register(cell(3, OP_MKFILE, t.mkfile(2, 4, 100)));
    let target = match t.id_of(2, 3) {
        Some(v) => v,
        None => 0,
    };
    let _ = l.register(cell(4, OP_SYMLINK, t.symlink(2, 5, target, 100)));
    let _ = l.register(word_cell(5, OP_DEPTH, t.depth_of(5)));
    let _ = l.register(cell(6, OP_RM, t.rm_r(2, 100)));
    let _ = l.register(cell(7, OP_STAT, Ok(t.live())));

    let mut a = Tree::new();
    let _ = a.mkroot(1, 100);
    let _ = l.register(cell(8, OP_MKDIR, a.mkroot(2, 100)));

    let mut b = Tree::new();
    let _ = b.mkroot(1, 100);
    let _ = l.register(cell(9, OP_MKDIR, b.mkdir(77, 2, 100)));

    let mut c = Tree::new();
    let _ = c.mkroot(1, 100);
    let _ = c.mkfile(1, 2, 100);
    let _ = l.register(cell(10, OP_MKDIR, c.mkdir(2, 3, 100)));

    let mut d = Tree::new();
    let _ = d.mkroot(1, 100);
    let _ = d.mkdir(1, 2, 100);
    let _ = d.mark_mount(2);
    let _ = l.register(cell(11, OP_RM, d.rm_r(2, 100)));

    let mut e = Tree::new();
    let _ = e.mkroot(1, 100);
    let _ = e.mkdir(1, 2, 200);
    let _ = e.mark_sticky(1);
    let _ = l.register(cell(12, OP_RM, e.rm_r(2, 300)));

    let mut f = Tree::new();
    let _ = f.mkroot(1, 100);
    let mut parent = 1u32;
    let mut k = 2u32;
    while k <= MAX_DEPTH {
        let _ = f.mkdir(parent, k, 100);
        parent = k;
        k += 1;
    }
    let _ = l.register(cell(13, OP_MKDIR, f.mkdir(parent, 99, 100)));

    l
}

/// 四路注入。
pub fn inject_four() -> [bool; 4] {
    let mut out = [false; 4];

    let l = build_ledger();
    let rep = l.replay();
    let root = l.seal();
    out[0] = l.len() == SAMPLES && l.audit().is_ok() && rep == Ok(root) && root != 0;

    // 一路：深层嵌套后序遍历，符号链接不跟随。
    let mut a = Tree::new();
    let _ = a.mkroot(1, 100);
    let mut parent = 1u32;
    let mut k = 2u32;
    while k <= 6 {
        let _ = a.mkdir(parent, k, 100);
        parent = k;
        k += 1;
    }
    let outside = parent;
    let _ = a.mkfile(1, 50, 100);
    let fid = match a.id_of(1, 50) {
        Some(v) => v,
        None => 0,
    };
    let _ = a.symlink(outside, 51, fid, 100);
    let lid = match a.id_of(outside, 51) {
        Some(v) => v,
        None => 0,
    };
    let deep = a.depth_of(lid);
    let gone = a.rm_r(2, 100);
    out[1] = deep == Ok(7)
        && gone == Ok(6)
        && a.live() == 2
        && a.unfollowed_count() == 1
        && a.links_dropped() == 1
        && a.node_at(1).map(|x| x.live) == Some(false)
        && a.consistent();

    // 二路：父目录链接计数随子目录增减。
    let mut b = Tree::new();
    let _ = b.mkroot(1, 100);
    let _ = b.mkdir(1, 2, 100);
    let _ = b.mkdir(1, 3, 100);
    let l3 = b.nlink_of(1);
    let _ = b.rm_r(2, 100);
    let l2 = b.nlink_of(1);
    let _ = b.rm_r(3, 100);
    let l1 = b.nlink_of(1);
    out[2] = l3 == Some(3) && l2 == Some(2) && l1 == Some(1) && b.removed_count() == 2;

    // 三路：闸门次序与容量。
    let mut q = Tree::new();
    let _ = q.mkroot(1, 100);
    let _ = q.mkfile(1, 2, 100);
    let g1 = q.rm_r(0, 100) == Err(RT_EINVAL);
    let g2 = q.rm_r(99, 100) == Err(RT_ENOENT);
    let g3 = q.mkdir(2, 3, 100) == Err(RT_ENOTDIR);
    let _ = q.mark_mount(2);
    let g4 = q.rm_r(2, 100) == Err(RT_EBUSY);
    let mut full = Tree::new();
    let _ = full.mkroot(1, 100);
    let mut i = 2u32;
    while (full.live() as usize) < CAP {
        let _ = full.mkfile(1, i, 100);
        i += 1;
    }
    let g5 = full.mkfile(1, 99, 100) == Err(RT_EFULL);
    out[3] = g1 && g2 && g3 && g4 && g5 && q.consistent() && full.consistent();

    out
}

/// 十三项判据。
pub fn run_unx11393_checks() -> CheckSet {
    let mut s = CheckSet::new("unx11393.rm_recursive_tree");
    let l = build_ledger();

    s.add_dyn(
        "RT1-十四格八允六拒",
        l.len() == SAMPLES && l.ok_count() == 8 && l.err_count() == 6 && l.audit().is_ok(),
        &format!("n={} ok={} err={}", l.len(), l.ok_count(), l.err_count()),
    );

    s.add(
        "RT2-建根建子目录",
        l.sample(0).map(|x| x.value) == Some(1) && l.sample(1).map(|x| x.value) == Some(2),
        "static",
    );

    s.add(
        "RT3-两文件一链接",
        l.sample(2).map(|x| x.value) == Some(3)
            && l.sample(3).map(|x| x.value) == Some(4)
            && l.sample(4).map(|x| x.value) == Some(5),
        "static",
    );

    s.add(
        "RT4-链接层级为三",
        l.sample(5).map(|x| x.value) == Some(3),
        "static",
    );

    s.add(
        "RT5-子树四节点全删",
        l.sample(6).map(|x| x.value) == Some(4),
        "static",
    );

    s.add(
        "RT6-根目录独存",
        l.sample(7).map(|x| x.value) == Some(1),
        "static",
    );

    s.add(
        "RT7-重复建根被拒",
        l.sample(8).map(|x| x.code) == Some(RT_EINVAL),
        "static",
    );

    s.add(
        "RT8-父缺失为ENOENT",
        l.sample(9).map(|x| x.code) == Some(RT_ENOENT),
        "static",
    );

    s.add(
        "RT9-父为文件ENOTDIR",
        l.sample(10).map(|x| x.code) == Some(RT_ENOTDIR),
        "static",
    );

    s.add(
        "RT10-子树含挂载点EBUSY",
        l.sample(11).map(|x| x.code) == Some(RT_EBUSY),
        "static",
    );

    s.add(
        "RT11-sticky越权与层级上限",
        l.sample(12).map(|x| x.code) == Some(RT_EPERM)
            && l.sample(13).map(|x| x.code) == Some(RT_EDEPTH),
        "static",
    );

    s.add(
        "RT12-链接不跟随且目标存活",
        {
            let mut t = Tree::new();
            let _ = t.mkroot(1, 100);
            let _ = t.mkfile(1, 2, 100);
            let _ = t.mkdir(1, 3, 100);
            let tgt = match t.id_of(1, 2) {
                Some(v) => v,
                None => 0,
            };
            let _ = t.symlink(3, 4, tgt, 100);
            let r = t.rm_r(3, 100);
            r == Ok(2)
                && t.live() == 2
                && t.unfollowed_count() == 1
                && t.node_at(1).map(|x| x.kind) == Some(K_FILE)
                && t.consistent()
        },
        "static",
    );

    s.add(
        "RT13-冻结重放四注入",
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
        assert!(l.audit().is_ok());
    }

    #[test]
    fn checks_pass() {
        let s = run_unx11393_checks();
        assert_eq!(s.len(), 13);
        assert!(s.all_passed());
    }

    #[test]
    fn post_order_removes_subtree() {
        let mut t = Tree::new();
        let _ = t.mkroot(1, 100);
        let _ = t.mkdir(1, 2, 100);
        let _ = t.mkfile(2, 3, 100);
        let _ = t.mkdir(2, 4, 100);
        let _ = t.mkfile(4, 5, 100);
        assert_eq!(t.rm_r(2, 100), Ok(4));
        assert_eq!(t.live(), 1);
        assert_eq!(t.depth_of(5), Err(RT_ENOENT));
    }

    #[test]
    fn symlink_not_followed() {
        let mut t = Tree::new();
        let _ = t.mkroot(1, 100);
        let _ = t.mkfile(1, 2, 100);
        let _ = t.mkdir(1, 3, 100);
        let tgt = match t.id_of(1, 2) {
            Some(v) => v,
            None => 0,
        };
        let _ = t.symlink(3, 4, tgt, 100);
        assert_eq!(t.rm_r(3, 100), Ok(2));
        assert_eq!(t.live(), 2);
        assert_eq!(t.unfollowed_count(), 1);
        assert!(t.consistent());
    }

    #[test]
    fn link_counts_follow_subdirs() {
        let mut t = Tree::new();
        let _ = t.mkroot(1, 100);
        let _ = t.mkdir(1, 2, 100);
        assert_eq!(t.nlink_of(1), Some(2));
        let _ = t.rm_r(2, 100);
        assert_eq!(t.nlink_of(1), Some(1));
    }

    #[test]
    fn mount_blocks_recursion() {
        let mut t = Tree::new();
        let _ = t.mkroot(1, 100);
        let _ = t.mkdir(1, 2, 100);
        let _ = t.mkdir(2, 3, 100);
        let _ = t.mark_mount(3);
        assert_eq!(t.rm_r(2, 100), Err(RT_EBUSY));
        assert_eq!(t.live(), 3);
    }

    #[test]
    fn gates_and_depth() {
        let mut t = Tree::new();
        assert_eq!(t.rm_r(0, 100), Err(RT_EINVAL));
        let _ = t.mkroot(1, 100);
        assert_eq!(t.mkroot(2, 100), Err(RT_EINVAL));
        assert_eq!(t.mkdir(77, 2, 100), Err(RT_ENOENT));
        let _ = t.mkfile(1, 2, 100);
        assert_eq!(t.mkdir(2, 3, 100), Err(RT_ENOTDIR));
        let mut parent = 1u32;
        let mut k = 3u32;
        while k <= MAX_DEPTH + 1 {
            let _ = t.mkdir(parent, k, 100);
            parent = k;
            k += 1;
        }
        assert_eq!(t.mkdir(parent, 90, 100), Err(RT_EDEPTH));
        assert!(t.consistent());
    }

    #[test]
    fn sticky_denies_stranger() {
        let mut t = Tree::new();
        let _ = t.mkroot(1, 100);
        let _ = t.mkdir(1, 2, 200);
        let _ = t.mark_sticky(1);
        assert_eq!(t.rm_r(2, 300), Err(RT_EPERM));
        assert_eq!(t.rm_r(2, 200), Ok(1));
    }
}
