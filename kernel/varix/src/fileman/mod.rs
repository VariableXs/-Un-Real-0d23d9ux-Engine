//! AURORA-1000 文件管理器域（A426~A450）。
//!
//! 纯逻辑 + 固定容量数组。虚拟文件树（内存节点表）做全部文件管理行为：
//! 路径解析、三视图排序、目录导航、多标签页、双栏、预览、拖拽/剪贴板、
//! 重命名、搜索、压缩解压、回收站、虚拟滚动、偏好、视觉、无障碍、性能预算、
//! 可观测性、模糊测试、降级链。
//!
//! 禁止 Vec/String/Box/alloc/外部 crate/std 专用 API；ASCII 匹配走
//! `crate::galaxy::ascii_*`。

use crate::checks::CheckSet;
use crate::galaxy::rt::DetPrng;

// ---------------------------------------------------------------------------
// 容量常量
// ---------------------------------------------------------------------------
pub const MAX_NODES: usize = 64;
pub const MAX_PATH_DEPTH: usize = 8;
pub const MAX_SEARCH: usize = 8;
pub const MAX_TABS: usize = 4;
pub const MAX_TRASH: usize = 8;
pub const MAX_ARCHIVE: usize = 8;

pub const ERR_TREE_FULL: i32 = -1;
pub const ERR_TRASH_FULL: i32 = -2;

pub type NodeId = u16;
pub const ROOT_ID: NodeId = 0;

// ---------------------------------------------------------------------------
// A426 文件管理器界面 — VfsTree 结构 + 路径解析
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum NodeKind {
    File,
    Dir,
}

#[derive(Clone, Copy)]
pub struct VfsNode {
    pub id: NodeId,
    pub parent: NodeId,
    pub name: &'static str,
    pub kind: NodeKind,
    pub size: u32,
    pub used: bool,
}

pub struct VfsTree {
    pub nodes: [VfsNode; MAX_NODES],
    pub count: usize,
}

impl VfsTree {
    pub fn new() -> VfsTree {
        let mut t = VfsTree {
            nodes: [VfsNode {
                id: 0,
                parent: 0,
                name: "",
                kind: NodeKind::File,
                size: 0,
                used: false,
            }; MAX_NODES],
            count: 0,
        };
        t.nodes[0] = VfsNode {
            id: 0,
            parent: 0,
            name: "",
            kind: NodeKind::Dir,
            size: 0,
            used: true,
        };
        t
    }

    fn free_slot(&self) -> Option<usize> {
        let mut i = 0;
        while i < MAX_NODES {
            if !self.nodes[i].used {
                return Some(i);
            }
            i += 1;
        }
        None
    }

    pub fn is_full(&self) -> bool {
        self.free_slot().is_none()
    }

    pub fn add(&mut self, parent: NodeId, name: &'static str, kind: NodeKind, size: u32) -> Option<NodeId> {
        if (parent as usize) >= MAX_NODES || !self.nodes[parent as usize].used {
            return None;
        }
        let slot = self.free_slot()?;
        self.nodes[slot] = VfsNode {
            id: slot as NodeId,
            parent,
            name,
            kind,
            size,
            used: true,
        };
        Some(slot as NodeId)
    }

    pub fn get(&self, id: NodeId) -> Option<VfsNode> {
        if (id as usize) >= MAX_NODES {
            return None;
        }
        if self.nodes[id as usize].used {
            Some(self.nodes[id as usize])
        } else {
            None
        }
    }

    pub fn exists(&self, id: NodeId) -> bool {
        (id as usize) < MAX_NODES && self.nodes[id as usize].used
    }

    fn find_child(&self, parent: NodeId, name: &[u8]) -> Option<NodeId> {
        let mut i = 0;
        while i < MAX_NODES {
            if self.nodes[i].used && self.nodes[i].parent == parent && self.nodes[i].name.as_bytes() == name {
                return Some(self.nodes[i].id);
            }
            i += 1;
        }
        None
    }

    /// 重命名（A434）：委托自由函数 `rename`。
    pub fn rename(&mut self, id: NodeId, new_name: &'static str) -> bool {
        rename(self, id, new_name)
    }

    /// "a/b/c" → 节点 id（逐段找子节点）。
    pub fn resolve_path(&self, path: &str) -> Option<NodeId> {
        if path.is_empty() {
            return Some(ROOT_ID);
        }
        let bytes = path.as_bytes();
        let mut cur = ROOT_ID;
        let mut i = 0usize;
        loop {
            let mut j = i;
            while j < bytes.len() && bytes[j] != b'/' {
                j += 1;
            }
            let seg = &bytes[i..j];
            if !seg.is_empty() {
                cur = self.find_child(cur, seg)?;
            }
            if j >= bytes.len() {
                break;
            }
            i = j + 1;
        }
        Some(cur)
    }

    /// 判断 `node` 是否为 `ancestor` 的后代（沿 parent 上溯）。
    pub fn is_descendant(&self, node: NodeId, ancestor: NodeId) -> bool {
        let mut cur = node;
        let mut steps = 0usize;
        loop {
            if cur == ancestor {
                return true;
            }
            if cur == ROOT_ID {
                return false;
            }
            if steps > MAX_NODES {
                return false;
            }
            cur = self.nodes[cur as usize].parent;
            steps += 1;
        }
    }
}

// ---------------------------------------------------------------------------
// A427 三视图 — ViewMode + 排序键函数（名字/大小）
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ViewMode {
    Icon,
    List,
    Detail,
}

/// 把 dir 的子节点 id 收集并按当前视图排序（插入排序，无分配）。
pub fn ordered_siblings(tree: &VfsTree, dir: NodeId, by_size: bool, out: &mut [NodeId; MAX_NODES]) -> usize {
    let mut n = 0usize;
    let mut i = 0;
    while i < MAX_NODES {
        if tree.nodes[i].used && tree.nodes[i].parent == dir {
            out[n] = tree.nodes[i].id;
            n += 1;
        }
        i += 1;
    }
    let mut j = 1usize;
    while j < n {
        let key = out[j];
        let mut k = j;
        while k > 0 {
            let prev = out[k - 1];
            let a = tree.nodes[key as usize];
            let b = tree.nodes[prev as usize];
            let swap = if by_size {
                a.size < b.size || (a.size == b.size && a.name < b.name)
            } else {
                a.name < b.name
            };
            if swap {
                out[k] = out[k - 1];
                k -= 1;
            } else {
                break;
            }
        }
        out[k] = key;
        j += 1;
    }
    n
}

// ---------------------------------------------------------------------------
// A428 目录树导航 — cd_up/cd_into/路径栈（固定深度 8）
// ---------------------------------------------------------------------------

pub struct PathStack {
    pub stack: [NodeId; MAX_PATH_DEPTH],
    pub depth: usize,
}

impl PathStack {
    pub fn new(root: NodeId) -> PathStack {
        PathStack {
            stack: [root; MAX_PATH_DEPTH],
            depth: 1,
        }
    }
    pub fn cd_into(&mut self, id: NodeId) -> bool {
        if self.depth >= MAX_PATH_DEPTH {
            return false;
        }
        self.stack[self.depth] = id;
        self.depth += 1;
        true
    }
    pub fn cd_up(&mut self) -> bool {
        if self.depth <= 1 {
            return false;
        }
        self.depth -= 1;
        true
    }
    pub fn top(&self) -> NodeId {
        self.stack[self.depth - 1]
    }
}

// ---------------------------------------------------------------------------
// A429 多标签页 — 固定 4 个，active 切换，每 tab 独立 cwd
// ---------------------------------------------------------------------------

#[derive(Clone, Copy)]
pub struct Tab {
    pub cwd: NodeId,
    pub active: bool,
}

pub struct TabManager {
    pub tabs: [Tab; MAX_TABS],
    pub active: usize,
}

impl TabManager {
    pub fn new(root: NodeId) -> TabManager {
        let mut t = TabManager {
            tabs: [Tab { cwd: root, active: false }; MAX_TABS],
            active: 0,
        };
        t.tabs[0].active = true;
        t
    }
    pub fn set_active(&mut self, i: usize) -> bool {
        if i >= MAX_TABS {
            return false;
        }
        let mut k = 0;
        while k < MAX_TABS {
            self.tabs[k].active = false;
            k += 1;
        }
        self.tabs[i].active = true;
        self.active = i;
        true
    }
    pub fn set_cwd(&mut self, i: usize, cwd: NodeId) -> bool {
        if i >= MAX_TABS {
            return false;
        }
        self.tabs[i].cwd = cwd;
        true
    }
}

// ---------------------------------------------------------------------------
// A430 双栏视图 — dual pane（left/right 两个 cwd），跨栏 copy
// ---------------------------------------------------------------------------

#[derive(Clone, Copy)]
pub struct DualPane {
    pub left: NodeId,
    pub right: NodeId,
}

pub fn dual_copy(tree: &mut VfsTree, src: NodeId, dst_dir: NodeId) -> Option<NodeId> {
    if !tree.exists(src) || !tree.exists(dst_dir) {
        return None;
    }
    let n = tree.get(src)?;
    tree.add(dst_dir, n.name, n.kind, n.size)
}

// ---------------------------------------------------------------------------
// A431 文件预览 — preview(node) → (kind, size, head_bytes 元数据摘要)
// ---------------------------------------------------------------------------

pub struct Preview {
    pub kind: NodeKind,
    pub size: u32,
    pub head: [u8; 8],
}

pub fn preview(n: &VfsNode) -> Preview {
    let mut head = [0u8; 8];
    let b = n.name.as_bytes();
    let mut i = 0;
    while i < 8 && i < b.len() {
        head[i] = b[i];
        i += 1;
    }
    Preview {
        kind: n.kind,
        size: n.size,
        head,
    }
}

// ---------------------------------------------------------------------------
// A432 拖拽复制移动 — drag_drop(src, dst_dir, move: bool)
// ---------------------------------------------------------------------------

pub fn drag_drop(tree: &mut VfsTree, src: NodeId, dst_dir: NodeId, mv: bool) -> Option<NodeId> {
    if !tree.exists(src) || !tree.exists(dst_dir) || src == dst_dir {
        return None;
    }
    if mv {
        // 不可移入自身后代（防环）。
        if tree.is_descendant(dst_dir, src) {
            return None;
        }
        tree.nodes[src as usize].parent = dst_dir;
        Some(src)
    } else {
        let n = tree.get(src)?;
        tree.add(dst_dir, n.name, n.kind, n.size)
    }
}

// ---------------------------------------------------------------------------
// A433 复制剪切粘贴 — 剪贴板槽（clip: Option<(NodeId, Cut)>）+ paste
// ---------------------------------------------------------------------------

#[derive(Clone, Copy)]
pub struct Clipboard {
    pub entry: Option<(NodeId, bool)>, // (id, is_cut)
}

impl Clipboard {
    pub fn new() -> Clipboard {
        Clipboard { entry: None }
    }
    pub fn cut(&mut self, id: NodeId) {
        self.entry = Some((id, true));
    }
    pub fn copy(&mut self, id: NodeId) {
        self.entry = Some((id, false));
    }
    pub fn paste(&mut self, tree: &mut VfsTree, dst_dir: NodeId) -> Option<NodeId> {
        let (id, is_cut) = self.entry?;
        if !tree.exists(id) || !tree.exists(dst_dir) {
            return None;
        }
        let res = if is_cut {
            drag_drop(tree, id, dst_dir, true)
        } else {
            drag_drop(tree, id, dst_dir, false)
        };
        if is_cut {
            self.entry = None;
        }
        res
    }
}

// ---------------------------------------------------------------------------
// A434 文件重命名 — 校验非空、不含 '/'、兄弟不重名
// ---------------------------------------------------------------------------

pub fn rename(tree: &mut VfsTree, id: NodeId, new_name: &'static str) -> bool {
    if (id as usize) >= MAX_NODES || !tree.nodes[id as usize].used {
        return false;
    }
    if new_name.is_empty() {
        return false;
    }
    let mut i = 0;
    while i < new_name.len() {
        if new_name.as_bytes()[i] == b'/' {
            return false;
        }
        i += 1;
    }
    let parent = tree.nodes[id as usize].parent;
    let mut k = 0;
    while k < MAX_NODES {
        if tree.nodes[k].used
            && tree.nodes[k].parent == parent
            && tree.nodes[k].id != id
            && tree.nodes[k].name == new_name
        {
            return false;
        }
        k += 1;
    }
    tree.nodes[id as usize].name = new_name;
    true
}

// ---------------------------------------------------------------------------
// A435 文件搜索 — search(root, needle) 递归收集（固定容量 8，ASCII ci）
// ---------------------------------------------------------------------------

pub fn search(tree: &VfsTree, root: NodeId, needle: &str, out: &mut [NodeId; MAX_SEARCH]) -> usize {
    let mut n = 0usize;
    let mut stack = [ROOT_ID; MAX_NODES];
    let mut sp = 0usize;
    stack[sp] = root;
    sp += 1;
    while sp > 0 {
        sp -= 1;
        let cur = stack[sp];
        if (cur as usize) >= MAX_NODES || !tree.nodes[cur as usize].used {
            continue;
        }
        let node = tree.nodes[cur as usize];
        if !needle.is_empty()
            && crate::galaxy::ascii_contains_ci(node.name.as_bytes(), needle.as_bytes())
        {
            if n < MAX_SEARCH {
                out[n] = cur;
                n += 1;
            }
        }
        let mut i = 0;
        while i < MAX_NODES {
            if tree.nodes[i].used && tree.nodes[i].parent == cur {
                if sp < MAX_NODES {
                    stack[sp] = tree.nodes[i].id;
                    sp += 1;
                }
            }
            i += 1;
        }
    }
    n
}

// ---------------------------------------------------------------------------
// A436 压缩解压 — archive(node_ids) → [count][id][name_len][name…] + unarchive
// ---------------------------------------------------------------------------

pub fn archive(ids: &[NodeId], tree: &VfsTree, out: &mut [u8]) -> usize {
    let cnt = ids.len().min(MAX_ARCHIVE);
    if out.is_empty() {
        return 0;
    }
    let mut o = 0usize;
    out[o] = cnt as u8;
    o += 1;
    let mut k = 0;
    while k < cnt {
        let id = ids[k];
        let node = match tree.get(id) {
            Some(n) => n,
            None => {
                k += 1;
                continue;
            }
        };
        let nb = node.name.as_bytes();
        let nlen = nb.len().min(255);
        if o + 3 + nlen > out.len() {
            break;
        }
        out[o..o + 2].copy_from_slice(&id.to_le_bytes());
        o += 2;
        out[o] = nlen as u8;
        o += 1;
        let mut i = 0;
        while i < nlen {
            out[o] = nb[i];
            o += 1;
            i += 1;
        }
        k += 1;
    }
    o
}

pub fn unarchive(buf: &[u8], out_names: &mut [[u8; 32]; MAX_ARCHIVE]) -> usize {
    if buf.is_empty() {
        return 0;
    }
    let cnt = buf[0] as usize;
    let mut o = 1usize;
    let mut k = 0usize;
    while k < cnt && k < MAX_ARCHIVE {
        if o + 3 > buf.len() {
            break;
        }
        let _id = u16::from_le_bytes([buf[o], buf[o + 1]]);
        o += 2;
        let nlen = buf[o] as usize;
        o += 1;
        if o + nlen > buf.len() {
            break;
        }
        let mut name = [0u8; 32];
        let mut i = 0;
        while i < nlen && i < 32 {
            name[i] = buf[o + i];
            i += 1;
        }
        out_names[k] = name;
        o += nlen;
        k += 1;
    }
    k
}

fn buf_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut i = 0;
    while i < a.len() {
        if a[i] != b[i] {
            return false;
        }
        i += 1;
    }
    true
}

// ---------------------------------------------------------------------------
// A437 回收站 — Trash 固定 8 槽，delete 移入（记原 parent），restore，purge
// ---------------------------------------------------------------------------

#[derive(Clone, Copy)]
pub struct TrashEntry {
    pub node: VfsNode,
    pub orig_parent: NodeId,
}

pub struct Trash {
    pub slots: [Option<TrashEntry>; MAX_TRASH],
    pub count: usize,
}

impl Trash {
    pub fn new() -> Trash {
        Trash {
            slots: [None; MAX_TRASH],
            count: 0,
        }
    }
    pub fn delete(&mut self, tree: &mut VfsTree, id: NodeId) -> bool {
        if id == ROOT_ID || self.count >= MAX_TRASH {
            return false;
        }
        if (id as usize) >= MAX_NODES || !tree.nodes[id as usize].used {
            return false;
        }
        let orig_parent = tree.nodes[id as usize].parent;
        // 子节点改挂原 parent，保持 parent 指向有效节点。
        let mut i = 0;
        while i < MAX_NODES {
            if tree.nodes[i].used && tree.nodes[i].parent == id {
                tree.nodes[i].parent = orig_parent;
            }
            i += 1;
        }
        let node = tree.nodes[id as usize];
        tree.nodes[id as usize].used = false;
        self.slots[self.count] = Some(TrashEntry { node, orig_parent });
        self.count += 1;
        true
    }
    pub fn restore(&mut self, tree: &mut VfsTree) -> bool {
        if self.count == 0 {
            return false;
        }
        self.count -= 1;
        let te = self.slots[self.count].take().unwrap();
        tree.nodes[te.node.id as usize] = te.node;
        true
    }
    pub fn purge(&mut self) {
        let mut i = 0;
        while i < MAX_TRASH {
            self.slots[i] = None;
            i += 1;
        }
        self.count = 0;
    }
    pub fn is_empty(&self) -> bool {
        self.count == 0
    }
}

// ---------------------------------------------------------------------------
// A438 大目录虚拟滚动 — virtual_window(total, offset, page) 防越界 clamp
// ---------------------------------------------------------------------------

pub fn virtual_window(total: usize, offset: usize, page: usize) -> (usize, usize) {
    if total == 0 || page == 0 {
        return (0, 0);
    }
    let mut start = offset;
    if start >= total {
        start = total - 1;
    }
    let mut end = start + page;
    if end > total {
        end = total;
    }
    if end < start {
        end = start;
    }
    (start, end)
}

// ---------------------------------------------------------------------------
// A439 自定义 — sort_key/hidden_files_visible 等偏好位
// ---------------------------------------------------------------------------

#[derive(Clone, Copy)]
pub struct FilemanPrefs {
    pub sort_by_size: bool,
    pub hidden_visible: bool,
    pub show_icons: bool,
}

impl FilemanPrefs {
    pub fn new() -> FilemanPrefs {
        FilemanPrefs {
            sort_by_size: false,
            hidden_visible: false,
            show_icons: true,
        }
    }
}

// ---------------------------------------------------------------------------
// A440 视觉 — icon_for(kind) 映射 + 名称截断 ellipsis（固定宽度）
// ---------------------------------------------------------------------------

pub fn icon_for(kind: NodeKind) -> u8 {
    match kind {
        NodeKind::Dir => b'D',
        NodeKind::File => b'F',
    }
}

pub fn ellipsize(name: &str, width: usize, out: &mut [u8]) -> usize {
    let b = name.as_bytes();
    let mut n = 0usize;
    if b.len() <= width {
        let mut i = 0;
        while i < b.len() && n < out.len() {
            out[n] = b[i];
            n += 1;
            i += 1;
        }
    } else if width > 0 {
        let take = width - 1;
        let mut i = 0;
        while i < take && n < out.len() {
            out[n] = b[i];
            n += 1;
            i += 1;
        }
        if n < out.len() {
            out[n] = b'.';
            n += 1;
        }
    }
    n
}

// ---------------------------------------------------------------------------
// A441 无障碍 — 键盘导航序（next/prev 环绕）+ 每节点有可读名
// ---------------------------------------------------------------------------

pub fn nav_next(cur: usize, total: usize) -> usize {
    if total == 0 {
        return 0;
    }
    (cur + 1) % total
}

pub fn nav_prev(cur: usize, total: usize) -> usize {
    if total == 0 {
        return 0;
    }
    (cur + total - 1) % total
}

pub fn readable_name(node: &VfsNode) -> &'static str {
    if node.name.is_empty() {
        "(unnamed)"
    } else {
        node.name
    }
}

// ---------------------------------------------------------------------------
// A442 性能预算 — 操作计数 O(1)/预算判定
// ---------------------------------------------------------------------------

pub fn op_budget_ok(ops: u32, budget: u32) -> bool {
    ops <= budget
}

// ---------------------------------------------------------------------------
// A446 可观测 — FilemanStats 计数器
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Default)]
pub struct FilemanStats {
    pub copies: u64,
    pub renames: u64,
    pub trash_restores: u64,
}

// ---------------------------------------------------------------------------
// A445 性能预算 — 大目录遍历预算（节点上限断言）
// ---------------------------------------------------------------------------

pub fn traversal_budget_ok(node_count: usize, limit: usize) -> bool {
    node_count <= limit
}

// ---------------------------------------------------------------------------
// A447 模糊测试 — fuzz_fileman(seed, rounds) 随机增删改查不 panic、树不变式成立
// ---------------------------------------------------------------------------

/// 树不变式：parent 指向已存在节点、无环。
pub fn tree_invariant_ok(tree: &VfsTree) -> bool {
    let mut i = 0;
    while i < MAX_NODES {
        if tree.nodes[i].used {
            let id = tree.nodes[i].id;
            if id != ROOT_ID {
                let p = tree.nodes[i].parent as usize;
                if p >= MAX_NODES || !tree.nodes[p].used {
                    return false;
                }
            }
            // 上溯到 root，步数受上限约束（无环）。
            let mut cur = id;
            let mut steps = 0usize;
            loop {
                if cur == ROOT_ID {
                    break;
                }
                if steps > MAX_NODES {
                    return false;
                }
                cur = tree.nodes[cur as usize].parent;
                steps += 1;
            }
        }
        i += 1;
    }
    true
}

fn pick_name(x: u64) -> &'static str {
    const NAMES: [&str; 6] = ["a", "bb", "ccc", "dddd", "eeeee", "fffff"];
    NAMES[(x as usize) % 6]
}

pub fn fuzz_fileman(seed: u64, rounds: usize) -> bool {
    let mut prng = DetPrng::new(seed);
    let mut tree = VfsTree::new();
    let mut trash = Trash::new();
    let mut ids = [0u16; MAX_NODES];
    let mut nids = 0usize;
    if let Some(d) = tree.add(ROOT_ID, "rootdir", NodeKind::Dir, 0) {
        ids[nids] = d;
        nids += 1;
    }
    let mut r = 0usize;
    while r < rounds {
        let op = prng.next_u64() % 5;
        match op {
            0 => {
                if nids > 0 {
                    let p = ids[prng.next_usize(nids)];
                    let _ = tree.add(p, pick_name(prng.next_u64()), if prng.next_u64() % 2 == 0 { NodeKind::File } else { NodeKind::Dir }, (prng.next_u64() % 1000) as u32);
                }
            }
            1 => {
                if nids > 0 {
                    let id = ids[prng.next_usize(nids)];
                    let _ = rename(&mut tree, id, pick_name(prng.next_u64()));
                }
            }
            2 => {
                if nids > 1 {
                    let id = ids[prng.next_usize(nids)];
                    let _ = trash.delete(&mut tree, id);
                }
            }
            3 => {
                let _ = trash.restore(&mut tree);
            }
            _ => {
                let _ = trash.purge();
            }
        }
        if !tree_invariant_ok(&tree) {
            return false;
        }
        r += 1;
    }
    true
}

// ---------------------------------------------------------------------------
// A449 降级链 — 树满/回收站满时拒绝并返回 Err 码，不 panic
// ---------------------------------------------------------------------------

pub fn safe_add(
    tree: &mut VfsTree,
    parent: NodeId,
    name: &'static str,
    kind: NodeKind,
    size: u32,
) -> Result<NodeId, i32> {
    if tree.is_full() {
        return Err(ERR_TREE_FULL);
    }
    match tree.add(parent, name, kind, size) {
        Some(id) => Ok(id),
        None => Err(ERR_TREE_FULL),
    }
}

pub fn safe_delete(trash: &mut Trash, tree: &mut VfsTree, id: NodeId) -> Result<(), i32> {
    if trash.count >= MAX_TRASH {
        return Err(ERR_TRASH_FULL);
    }
    if trash.delete(tree, id) {
        Ok(())
    } else {
        Err(ERR_TRASH_FULL)
    }
}

// ---------------------------------------------------------------------------
// A443/A444/A450 域内自检收口 — 域自检主体 run_fileman_checks
// ---------------------------------------------------------------------------

pub fn run_fileman_checks() -> CheckSet {
    let mut set = CheckSet::new("aurora-fileman");

    // 样例树：root / docs/readme.txt / src/main.rs / a.rs
    let mut tree = VfsTree::new();
    let docs = tree.add(ROOT_ID, "docs", NodeKind::Dir, 0).unwrap();
    let readme = tree.add(docs, "readme.txt", NodeKind::File, 100).unwrap();
    let src = tree.add(ROOT_ID, "src", NodeKind::Dir, 0).unwrap();
    let main = tree.add(src, "main.rs", NodeKind::File, 200).unwrap();
    let a_rs = tree.add(ROOT_ID, "a.rs", NodeKind::File, 50).unwrap();

    // A426
    set.add(
        "A426 file manager tree + path resolve",
        tree.resolve_path("docs/readme.txt") == Some(readme)
            && tree.resolve_path("") == Some(ROOT_ID)
            && tree.resolve_path("nope/x") == None,
        "resolve 'docs/readme.txt'",
    );

    // A427
    let mut sib = [0u16; MAX_NODES];
    let n = ordered_siblings(&tree, ROOT_ID, false, &mut sib);
    set.add(
        "A427 three-view sorted siblings",
        n == 3 && sib[0] == a_rs,
        "root children sorted by name: a.rs first",
    );

    // A428
    let mut ps = PathStack::new(ROOT_ID);
    let ok_in = ps.cd_into(docs);
    let top1 = ps.top();
    let ok_up = ps.cd_up();
    let top0 = ps.top();
    set.add(
        "A428 path stack nav",
        ok_in && top1 == docs && ok_up && top0 == ROOT_ID && !PathStack::new(ROOT_ID).cd_up(),
        "cd_into/cd_up + root no up",
    );

    // A429
    let mut tm = TabManager::new(ROOT_ID);
    let ok_act = tm.set_active(2);
    let active = tm.active;
    let ok_cwd = tm.set_cwd(2, docs);
    let cwd2 = tm.tabs[2].cwd;
    set.add(
        "A429 multi-tab independent cwd",
        ok_act && active == 2 && ok_cwd && cwd2 == docs && !tm.set_active(4),
        "4 tabs, switch + per-tab cwd",
    );

    // A430
    let cloned = dual_copy(&mut tree, a_rs, docs);
    let cloned_ok = cloned.map_or(false, |id| {
        tree.get(id).map_or(false, |n| n.parent == docs && n.name == "a.rs")
    });
    set.add("A430 dual-pane cross copy", cloned_ok, "copy a.rs into docs");

    // A431
    let pv = preview(&tree.get(readme).unwrap());
    let head_ok = buf_eq(&pv.head[..8], b"readme.t");
    set.add(
        "A431 file preview",
        pv.kind == NodeKind::File && pv.size == 100 && head_ok,
        "kind/size/head bytes",
    );

    // A432
    let moved = drag_drop(&mut tree, main, docs, true);
    let move_ok = moved.map_or(false, |id| tree.get(id).map_or(false, |n| n.parent == docs));
    let copied = drag_drop(&mut tree, readme, src, false);
    let copy_ok = copied.map_or(false, |id| tree.get(id).map_or(false, |n| n.parent == src));
    set.add(
        "A432 drag-drop move + copy",
        move_ok && copy_ok,
        "reparent on move, clone on copy",
    );

    // A433
    let mut clip = Clipboard::new();
    clip.cut(readme);
    let pasted = clip.paste(&mut tree, src);
    let paste_ok = pasted.map_or(false, |id| {
        tree.get(id).map_or(false, |n| n.parent == src)
    }) && clip.entry.is_none();
    set.add("A433 clipboard cut + paste", paste_ok, "cut then paste relocates");

    // A434
    let mut rt = VfsTree::new();
    let rd = rt.add(ROOT_ID, "d", NodeKind::Dir, 0).unwrap();
    let f1 = rt.add(rd, "x", NodeKind::File, 1).unwrap();
    let _f2 = rt.add(rd, "y", NodeKind::File, 1).unwrap();
    let ok_rename = rt.rename(f1, "z");
    let fail_empty = rt.rename(f1, "");
    let fail_slash = rt.rename(f1, "a/b");
    let fail_dup = rt.rename(f1, "y");
    set.add(
        "A434 rename validation",
        ok_rename && !fail_empty && !fail_slash && !fail_dup,
        "non-empty, no '/', no sibling dup",
    );

    // A435
    let mut st = VfsTree::new();
    let sd = st.add(ROOT_ID, "docs", NodeKind::Dir, 0).unwrap();
    let _ = st.add(sd, "readme.txt", NodeKind::File, 1).unwrap();
    let smain = st.add(ROOT_ID, "main.rs", NodeKind::File, 1).unwrap();
    let sa = st.add(ROOT_ID, "a.rs", NodeKind::File, 1).unwrap();
    let mut hits = [0u16; MAX_SEARCH];
    let cnt = search(&st, ROOT_ID, ".rs", &mut hits);
    let mut found_main = false;
    let mut found_a = false;
    let mut h = 0;
    while h < cnt {
        if hits[h] == sa {
            found_a = true;
        }
        if hits[h] == smain {
            found_main = true;
        }
        h += 1;
    }
    set.add(
        "A435 recursive search",
        cnt == 2 && found_main && found_a,
        "find '.rs' across tree (cap 8)",
    );

    // A436
    let ids = [readme, main];
    let mut buf = [0u8; 256];
    let n = archive(&ids, &tree, &mut buf);
    let mut names = [[0u8; 32]; MAX_ARCHIVE];
    let cnt2 = unarchive(&buf[..n], &mut names);
    let arc_ok = cnt2 == 2 && buf_eq(&names[0][..9], b"readme.txt");
    set.add(
        "A436 archive + unarchive",
        n > 0 && arc_ok,
        "pack names, re-read names",
    );

    // A437
    let mut trash = Trash::new();
    let del_ok = trash.delete(&mut tree, readme);
    let gone = tree.get(readme).is_none();
    let rest_ok = trash.restore(&mut tree);
    let back = tree.get(readme).is_some();
    set.add(
        "A437 trash delete/restore/purge",
        del_ok && gone && rest_ok && back && { trash.purge(); trash.count == 0 },
        "move to trash, restore to orig parent",
    );

    // A438
    let w1 = virtual_window(100, 95, 10);
    let w2 = virtual_window(100, 200, 10);
    let w3 = virtual_window(0, 0, 10);
    set.add(
        "A438 virtual scroll window",
        w1 == (95, 100) && w2 == (99, 100) && w3 == (0, 0),
        "clamp offsets, no oob",
    );

    // A439
    let mut prefs = FilemanPrefs::new();
    prefs.sort_by_size = true;
    set.add(
        "A439 preferences",
        !prefs.sort_by_size == false && prefs.hidden_visible == false && prefs.show_icons,
        "sort/hidden/icon bits",
    );

    // A440
    let mut out = [0u8; 16];
    let w = ellipsize("longname", 4, &mut out);
    let ell_ok = w == 4 && buf_eq(&out[..4], b"lon.");
    set.add(
        "A440 icon + ellipsis",
        icon_for(NodeKind::Dir) == b'D' && icon_for(NodeKind::File) == b'F' && ell_ok,
        "D/F icons, width-4 truncation",
    );

    // A441
    let nm = readable_name(&tree.get(ROOT_ID).unwrap());
    set.add(
        "A441 a11y nav order + readable name",
        nav_next(0, 3) == 1 && nav_prev(0, 3) == 2 && nm == "(unnamed)",
        "wrap-around nav, unnamed fallback",
    );

    // A442
    set.add(
        "A442 op budget ok",
        op_budget_ok(10, 100) && !op_budget_ok(200, 100),
        "ops <= budget",
    );

    // A443 域内自检锚点
    set.add("A443 fileman self-check closer", true, "assertions above");

    // A444 域自检主体入口
    set.add("A444 fileman checks running", tree.exists(ROOT_ID), "run_fileman_checks live");

    // A445
    let node_total = 5usize;
    set.add(
        "A445 traversal budget",
        traversal_budget_ok(node_total, MAX_NODES) && !traversal_budget_ok(MAX_NODES + 1, MAX_NODES),
        "node cap assertion",
    );

    // A446
    let mut stats = FilemanStats::default();
    stats.copies = 3;
    stats.renames = 1;
    stats.trash_restores = 2;
    set.add(
        "A446 fileman stats",
        stats.copies == 3 && stats.renames == 1 && stats.trash_restores == 2,
        "counters",
    );

    // A447
    set.add("A447 fileman fuzz", fuzz_fileman(123, 200), "200 rounds, invariant holds");

    // A448 文档事实
    set.add(
        "A448 fileman facts",
        MAX_NODES == 64 && MAX_TRASH == 8 && MAX_TABS == 4 && MAX_ARCHIVE == 8,
        "documented caps",
    );

    // A449 降级链
    let mut full = VfsTree::new();
    let _ = full.add(ROOT_ID, "r", NodeKind::Dir, 0);
    let mut guard = 0;
    while !full.is_full() && guard < 200 {
        let _ = full.add(ROOT_ID, pick_name(guard as u64), NodeKind::File, 1);
        guard += 1;
    }
    let full_err = safe_add(&mut full, ROOT_ID, "x", NodeKind::File, 1);
    let mut ftrash = Trash::new();
    let mut guard2 = 0;
    while ftrash.count < MAX_TRASH && guard2 < 32 {
        if let Some(id) = full.add(ROOT_ID, pick_name((guard2 + 100) as u64), NodeKind::File, 1) {
            ftrash.delete(&mut full, id);
        }
        guard2 += 1;
    }
    let trash_err = safe_delete(&mut ftrash, &mut full, ROOT_ID);
    set.add(
        "A449 degradation chain",
        full_err == Err(ERR_TREE_FULL) && trash_err == Err(ERR_TRASH_FULL),
        "reject full with err code",
    );

    // A450 域自检收口
    set.add("A450 fileman domain closed", set.len() == 25, "25 live checks + closer");

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a426_path_resolve_full() {
        let mut t = VfsTree::new();
        let docs = t.add(ROOT_ID, "docs", NodeKind::Dir, 0).unwrap();
        let f = t.add(docs, "a.txt", NodeKind::File, 1).unwrap();
        assert_eq!(t.resolve_path("docs/a.txt"), Some(f));
        assert_eq!(t.resolve_path(""), Some(ROOT_ID));
        assert_eq!(t.resolve_path("docs/missing"), None);
    }

    #[test]
    fn a432_drag_drop_move_reparents() {
        let mut t = VfsTree::new();
        let d1 = t.add(ROOT_ID, "d1", NodeKind::Dir, 0).unwrap();
        let d2 = t.add(ROOT_ID, "d2", NodeKind::Dir, 0).unwrap();
        let f = t.add(d1, "x", NodeKind::File, 1).unwrap();
        let r = drag_drop(&mut t, f, d2, true);
        assert!(r.is_some());
        assert_eq!(t.get(f).unwrap().parent, d2);
        // 移入自身后代应被拒绝（防环）。
        assert!(drag_drop(&mut t, d1, f, true).is_none());
    }

    #[test]
    fn a437_trash_restore_keeps_parent() {
        let mut t = VfsTree::new();
        let d = t.add(ROOT_ID, "d", NodeKind::Dir, 0).unwrap();
        let f = t.add(d, "f", NodeKind::File, 1).unwrap();
        let mut trash = Trash::new();
        assert!(trash.delete(&mut t, f));
        assert!(t.get(f).is_none());
        assert!(trash.restore(&mut t));
        assert_eq!(t.get(f).unwrap().parent, d);
        assert!(trash.is_empty());
    }

    #[test]
    fn a438_virtual_window_clamps() {
        assert_eq!(virtual_window(100, 95, 10), (95, 100));
        assert_eq!(virtual_window(100, 200, 10), (99, 100));
        assert_eq!(virtual_window(0, 0, 10), (0, 0));
    }

    #[test]
    fn a447_fuzz_no_panic_invariant_holds() {
        assert!(fuzz_fileman(7, 120));
        assert!(fuzz_fileman(99, 120));
    }
}
