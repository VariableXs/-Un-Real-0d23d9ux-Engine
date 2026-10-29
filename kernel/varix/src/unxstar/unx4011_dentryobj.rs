//! unx4011_dentryobj — UNX-F4011 Dentry 对象结构锚（诊断段 0x8420）。
//!
//! 判据原文：
//!   J1 Dentry 字段集（名/父指针/inode 绑定/负缓存标记/引用计数/d_flags）
//!      实例化断言通过。
//!   J2 随机树 1,000 节点父链回溯到根零断链（每次回溯步数 == 深度）。
//!   J3 负缓存语义：negative=true 时 inode 绑定必须为空；正项必须绑定 inode。
//!   J4 d_flags 位段闭集：DCACHE_* 四位之外置位拒收，引用计数不为负。
//! 设计纪律：
//!   1. dentry 以「父槽号 + 名哈希」构成键；根 dentry 的 parent=0，是唯一
//!      无父节点。任何节点的父槽必须小于自身槽号（防环），构造时强制。
//!   2. 父链回溯以步数上限 MAX_DEPTH 兜底：超过即判断链/成环，返回错误。
//!   3. 名字长度上限 NAME_MAX=255（POSIX NAME_MAX），空名与超长名拒收。
//!   4. 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。

use crate::alloc::vec::Vec;

pub const SEG_BASE: u16 = 0x8420;

// ── 诊断码闭集 ───────────────────────────────────────────────
const E_NAME_BAD: u16 = SEG_BASE + 1; // 名字为空或超 NAME_MAX
const E_PARENT_BAD: u16 = SEG_BASE + 2; // 父槽非法（越界/不小于自身）
const E_BIND_BAD: u16 = SEG_BASE + 3; // inode 绑定与负标记矛盾
const E_CHAIN_BREAK: u16 = SEG_BASE + 4; // 父链断链或成环
const E_FLAG_BAD: u16 = SEG_BASE + 5; // d_flags 越出闭集
const E_DEPTH_OVER: u16 = SEG_BASE + 6; // 回溯深度超上限（成环嫌疑）

/// 名字长度上限（POSIX NAME_MAX）。
pub const NAME_MAX: usize = 255;
/// 父链回溯步数上限（成环兜底）。
pub const MAX_DEPTH: u32 = 4096;

/// d_flags 闭集位（Linux DCACHE_* 子集）。
pub const DCACHE_MOUNTED: u32 = 0x0001;   // 挂载点
pub const DCACHE_NEED_AUTOMOUNT: u32 = 0x0002; // 需自动挂载
pub const DCACHE_MISS_TYPE: u32 = 0x0004; // 负项类型（MISS）
pub const DCACHE_OP_HASH: u32 = 0x0008;   // 自定义哈希
pub const DFLAG_MASK: u32 =
    DCACHE_MOUNTED | DCACHE_NEED_AUTOMOUNT | DCACHE_MISS_TYPE | DCACHE_OP_HASH;

/// dentry 生命周期状态机。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DState {
    Free = 0,      // 已释放
    Referenced = 1, // 被引用
    Cached = 2,    // 仅缓存（引用归零）
}

/// Dentry 对象结构锚。
pub struct Dentry {
    pub name_len: u16,   // 名字长度（0 非法，≤255）
    pub name_hash: u32,  // 名字哈希（键的一部分）
    pub parent: u16,     // 父 dentry 槽号（0=根）
    pub inode: u16,      // 绑定 inode 槽号（0=未绑定/负项）
    pub negative: bool,  // 负缓存标记
    pub refs: u32,       // 引用计数
    pub d_flags: u32,    // dentry 标志位
    pub state: DState,   // 生命周期
    pub depth: u16,      // 深度（根=0，构造时由父深度+1 得出）
}

impl Dentry {
    /// 实例化：六字段逐一校验（判据 J1）。
    pub fn new(
        name: &[u8],
        hash: u32,
        parent: u16,
        inode: u16,
        negative: bool,
        flags: u32,
        depth: u16,
    ) -> Result<Dentry, u16> {
        if name.is_empty() || name.len() > NAME_MAX {
            return Err(E_NAME_BAD);
        }
        if flags & !DFLAG_MASK != 0 {
            return Err(E_FLAG_BAD);
        }
        if negative && inode != 0 {
            return Err(E_BIND_BAD);
        }
        if !negative && parent != 0 && inode == 0 {
            return Err(E_BIND_BAD); // 非根正项必须绑 inode
        }
        Ok(Dentry {
            name_len: name.len() as u16,
            name_hash: hash,
            parent,
            inode,
            negative,
            refs: 1,
            d_flags: flags,
            state: DState::Referenced,
            depth,
        })
    }

    /// 引用 +1。
    pub fn get(&mut self) -> Result<u32, u16> {
        if self.refs >= (1 << 20) {
            return Err(E_FLAG_BAD);
        }
        self.refs += 1;
        self.state = DState::Referenced;
        Ok(self.refs)
    }

    /// 引用 -1；归零进 Cached（不立即释放，dcache 保留）。
    pub fn put(&mut self) -> Result<u32, u16> {
        if self.refs == 0 {
            return Err(E_FLAG_BAD);
        }
        self.refs -= 1;
        if self.refs == 0 {
            self.state = DState::Cached;
        }
        Ok(self.refs)
    }

    /// 负项标记与 inode 绑定的一致性自检（判据 J3）。
    pub fn bind_consistent(&self) -> bool {
        if self.negative {
            self.inode == 0
        } else {
            self.inode != 0
        }
    }
}

/// dentry 树：槽表 + 父链回溯 + 断链账。
pub struct DentryTree {
    pub slots: Vec<Dentry>, // 槽表（1-based）
    pub lookups: u32,       // 回溯次数账
    pub breaks: u32,        // 断链检出账
    pub max_seen: u16,      // 观测到的最大深度
}

impl DentryTree {
    pub fn new() -> DentryTree {
        DentryTree { slots: Vec::new(), lookups: 0, breaks: 0, max_seen: 0 }
    }

    /// 哈希：FNV-1a 32 位（与 triobjtable 一致，跨模块语义统一）。
    pub fn hash(name: &[u8]) -> u32 {
        let mut h: u32 = 0x811C_9DC5;
        for b in name.iter() {
            h ^= *b as u32;
            h = h.wrapping_mul(0x0100_0193);
        }
        h
    }

    /// 建根：parent=0、允许 inode=0 的特殊正项。
    pub fn add_root(&mut self, inode: u16) -> Result<u16, u16> {
        if !self.slots.is_empty() {
            return Err(E_PARENT_BAD); // 根唯一
        }
        let d = Dentry::new(b"/", DentryTree::hash(b"/"), 0, inode, false, 0, 0)?;
        self.slots.push(d);
        Ok(1)
    }

    /// 挂子节点：父槽必须存在且小于新槽号（防环），深度 = 父深度 + 1。
    pub fn add_child(
        &mut self,
        name: &[u8],
        parent: u16,
        inode: u16,
        negative: bool,
        flags: u32,
    ) -> Result<u16, u16> {
        if parent == 0 || parent as usize > self.slots.len() {
            return Err(E_PARENT_BAD);
        }
        let slot = (self.slots.len() + 1) as u16;
        if parent >= slot {
            return Err(E_PARENT_BAD); // 防环：父槽必须早于自身
        }
        let pdepth = self.slots[(parent - 1) as usize].depth;
        let depth = match pdepth.checked_add(1) {
            Some(v) => v,
            None => return Err(E_DEPTH_OVER),
        };
        let d = Dentry::new(
            name,
            DentryTree::hash(name),
            parent,
            inode,
            negative,
            flags,
            depth,
        )?;
        self.slots.push(d);
        if depth > self.max_seen {
            self.max_seen = depth;
        }
        Ok(slot)
    }

    /// 父链回溯到根：返回步数（= 深度），任何断链/越界/超深均报错（判据 J2）。
    pub fn walk_to_root(&mut self, slot: u16) -> Result<u32, u16> {
        if slot == 0 || slot as usize > self.slots.len() {
            self.breaks += 1;
            return Err(E_PARENT_BAD);
        }
        self.lookups += 1;
        let mut cur = slot;
        let mut steps = 0u32;
        loop {
            if steps > MAX_DEPTH {
                self.breaks += 1;
                return Err(E_DEPTH_OVER);
            }
            let p = self.slots[(cur - 1) as usize].parent;
            if p == 0 {
                return Ok(steps);
            }
            if p as usize > self.slots.len() || p >= cur {
                self.breaks += 1;
                return Err(E_CHAIN_BREAK);
            }
            cur = p;
            steps += 1;
        }
    }

    /// 全树父链自检：每个节点回溯成功且步数等于其 depth。
    pub fn chain_ok(&mut self) -> bool {
        let n = self.slots.len();
        let mut i = 1usize;
        while i <= n {
            let steps = match self.walk_to_root(i as u16) {
                Ok(s) => s,
                Err(_) => return false,
            };
            if steps != self.slots[i - 1].depth as u32 {
                self.breaks += 1;
                return false;
            }
            i += 1;
        }
        true
    }

    /// 负项账：负 dentry 数量。
    pub fn negative_count(&self) -> u32 {
        let mut c = 0u32;
        for d in self.slots.iter() {
            if d.negative {
                c += 1;
            }
        }
        c
    }

    /// 深度分布导出：最大深度。
    pub fn depth_max(&self) -> u16 {
        let mut m = 0u16;
        for d in self.slots.iter() {
            if d.depth > m {
                m = d.depth;
            }
        }
        m
    }
}

/// 诊断码释义。
pub fn detail(code: u16) -> &'static str {
    match code {
        E_NAME_BAD => "名字为空或超 NAME_MAX(255)",
        E_PARENT_BAD => "父槽非法（越界/不小于自身/非根重复）",
        E_BIND_BAD => "inode 绑定与负标记矛盾",
        E_CHAIN_BREAK => "父链断链或成环",
        E_FLAG_BAD => "d_flags 越出闭集或引用计数异常",
        E_DEPTH_OVER => "回溯深度超上限（成环嫌疑）",
        _ => "未知诊断码",
    }
}

/// 确定性演练：字段实例化 → 千节点树 → 父链回溯 → 负缓存 → 注入拦截。
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;
    let mut tree = DentryTree::new();

    // R1：根 dentry 六字段实例化断言（判据 J1）。
    let root = tree.add_root(1)?;
    if root != 1 {
        return Err(E_PARENT_BAD);
    }
    let r = &tree.slots[0];
    if r.parent != 0 || r.depth != 0 || r.negative || r.refs != 1 {
        return Err(E_BIND_BAD);
    }
    if r.d_flags != 0 || !r.bind_consistent() {
        return Err(E_FLAG_BAD);
    }
    hits += 1;

    // R2：建 1,000 节点随机链式树——每节点挂在上一个节点下（深度 1..1000）。
    let mut prev = root;
    for n in 1..=1_000u32 {
        // 名字确定性构造：d<序号>，长度 2..=5
        let mut buf = [0u8; 8];
        buf[0] = b'd';
        let mut v = n;
        let mut len = 1usize;
        let mut digits = [0u8; 6];
        let mut dc = 0usize;
        while v > 0 && dc < 6 {
            digits[dc] = b'0' + (v % 10) as u8;
            v /= 10;
            dc += 1;
        }
        let mut k = 0usize;
        while k < dc {
            buf[len] = digits[dc - 1 - k];
            len += 1;
            k += 1;
        }
        prev = tree.add_child(&buf[..len], prev, (n % 50 + 2) as u16, false, 0)?;
    }
    if tree.slots.len() != 1_001 {
        return Err(E_CHAIN_BREAK);
    }
    hits += 1;

    // R3：父链回溯到根零断链——叶节点步数 == 1000（判据 J2 原文）。
    let steps = tree.walk_to_root(prev)?;
    if steps != 1_000 {
        return Err(E_CHAIN_BREAK);
    }
    if tree.breaks != 0 {
        return Err(E_CHAIN_BREAK);
    }
    hits += 1;

    // R4：全树逐节点回溯，步数 == depth（1,001 节点全体自检）。
    if !tree.chain_ok() {
        return Err(E_CHAIN_BREAK);
    }
    if tree.depth_max() != 1_000 || tree.max_seen != 1_000 {
        return Err(E_CHAIN_BREAK);
    }
    hits += 1;

    // R5：负 dentry 登记——挂在根下，inode=0，标记一致（判据 J3）。
    let dn = tree.add_child(b"missing", root, 0, true, DCACHE_MISS_TYPE)?;
    let dneg = &tree.slots[(dn - 1) as usize];
    if !dneg.negative || dneg.inode != 0 || !dneg.bind_consistent() {
        return Err(E_BIND_BAD);
    }
    if tree.negative_count() != 1 {
        return Err(E_BIND_BAD);
    }
    hits += 1;

    // R6：引用计数 get/put 对称——两次 get 后两次 put 回 1。
    let idx = (dn - 1) as usize;
    tree.slots[idx].get()?;
    tree.slots[idx].get()?;
    if tree.slots[idx].refs != 3 {
        return Err(E_FLAG_BAD);
    }
    tree.slots[idx].put()?;
    tree.slots[idx].put()?;
    if tree.slots[idx].refs != 1 || tree.slots[idx].state != DState::Referenced {
        return Err(E_FLAG_BAD);
    }
    hits += 1;

    // R7：d_flags 闭集——置 MOUNTED 合法，记账可读。
    let dm = tree.add_child(b"mnt", root, 9, false, DCACHE_MOUNTED)?;
    if tree.slots[(dm - 1) as usize].d_flags & DCACHE_MOUNTED == 0 {
        return Err(E_FLAG_BAD);
    }
    hits += 1;

    // I1：空名注入被拒。
    if tree.add_child(b"", root, 9, false, 0) != Err(E_NAME_BAD) {
        return Err(E_NAME_BAD);
    }
    hits += 1;

    // I2：超长名（256 字节）注入被拒。
    let long = [b'x'; 256];
    if tree.add_child(&long, root, 9, false, 0) != Err(E_NAME_BAD) {
        return Err(E_NAME_BAD);
    }
    hits += 1;

    // I3：负项绑 inode / 正项不绑 inode 双注入被拒。
    if tree.add_child(b"badneg", root, 5, true, 0) != Err(E_BIND_BAD) {
        return Err(E_BIND_BAD);
    }
    if tree.add_child(b"badpos", root, 0, false, 0) != Err(E_BIND_BAD) {
        return Err(E_BIND_BAD);
    }
    hits += 1;

    // I4：父槽越界注入被拒。
    if tree.add_child(b"orphan", 9_999, 9, false, 0) != Err(E_PARENT_BAD) {
        return Err(E_PARENT_BAD);
    }
    hits += 1;

    // I5：d_flags 越界位注入被拒。
    if tree.add_child(b"flagbad", root, 9, false, 0x100) != Err(E_FLAG_BAD) {
        return Err(E_FLAG_BAD);
    }
    hits += 1;

    // I6：断链注入——把某节点父槽改成自身，回溯必须检出（判据 J2 反向）。
    let victim = (tree.slots.len()) as u16;
    tree.slots[(victim - 1) as usize].parent = victim;
    if tree.walk_to_root(victim) != Err(E_CHAIN_BREAK) {
        return Err(E_CHAIN_BREAK);
    }
    if tree.breaks == 0 {
        return Err(E_CHAIN_BREAK);
    }
    hits += 1;

    Ok(hits)
}
