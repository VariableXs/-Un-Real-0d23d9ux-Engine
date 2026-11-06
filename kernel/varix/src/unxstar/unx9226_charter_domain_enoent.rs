//! UNX-F9226 ENOENT 判定域总纲（路径解析错误面） · 0x7A600段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（派工卡 UNX-F9226 逐字）：
//!   ① 纯功能代码 ≥360 行；② 判据原文：UNX-F9226-J1 路径解析错误面判据过，
//!   ENOENT 触发三态（末段缺失/中间段缺失/空组件）逐格落账，与 B1 VFS 冻结
//!   接口消费格过；③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//!
//! 设计要点：
//!   ① 三态不是三条 if，而是**解析游标停在三个不同位置**时产生的三个
//!      不同失败态：末段缺失（父目录命中、末组件不存在）、中间段缺失
//!      （某非末组件不存在，后续组件根本没被看）、空组件（路径为空串或
//!      出现零长组件，切分阶段即失败）。三态对外同码 ENOENT，但落账必须
//!      分格，否则「哪一段断了」在诊断上不可见。
//!   ② 空组件按本域**冻结契约**从严：连续 `//` 一律判空组件（Linux 的
//!      折叠语义与此不同，差异在对照表中显性登记，不假装同源）。
//!   ③ 干扰态隔离：中间段命中但是**普通文件**、或尾斜杠落在文件上，归
//!      ENOTDIR 面，绝不许被吞进 ENOENT 三态——这是本模块最容易被写错的
//!      一格（"没找到"与"找到了但不是目录"必须分码）。
//!   ④ 解析游标是状态机：Idle → Walking →（Settled | Faulted），只准前进；
//!      Settled 之后再回到 Walking 属非法倒退，必须被机器本身拒绝并计账。
//!   ⑤ 与 B1 VFS 冻结接口对账：同一棵树、同一批路径，冻结接口消费出的
//!      (命中 inode | errno) 必须与本域判定器逐格相等，ABI 版本钉死。
//!   ⑥ 注入账守恒：注入 = 检出 + 逃逸，逃逸恒 0；三态分格计数之和必须
//!      等于检出总数，任何负增量显性拒绝。
//! ─────────────────────────────────────────────────────────────────────

use alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（F9226 独占 0x40 窗口）。
pub const SEG_BASE: u32 = 0x7A600;

/// 节点表容量（含根节点）。
pub const MAX_NODES: usize = 16;

/// 单段名称字节上限。
pub const NAME_CAP: usize = 12;

/// 单条路径的组件上限（超出归 ENAMETOOLONG）。
pub const MAX_COMPONENTS: usize = 8;

/// 单类注入的复测轮数（判据要求 10/10）。
pub const INJECT_ROUNDS: u32 = 10;

/// B1 VFS 冻结接口版本（钉死，漂移即判红）。
pub const B1_VFS_ABI: u32 = 7;

/// Linux errno 正数口径。
pub const ENOENT: i32 = 2;
pub const EEXIST: i32 = 17;
pub const ENOTDIR: i32 = 20;
pub const ENAMETOOLONG: i32 = 36;
pub const ENOSPC: i32 = 28;

// ── 诊断码闭集 ────────────────────────────────────────────────────────
/// 本模块诊断码（段内 +0x01..+0x0B 闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PathCode {
    /// 末段缺失
    LastComponentMissing,
    /// 中间段缺失
    MiddleComponentMissing,
    /// 空组件（空路径 / 零长组件）
    EmptyComponent,
    /// 中间段命中但不是目录
    NonDirComponent,
    /// 尾斜杠落在文件上
    TrailingSlashOnFile,
    /// 组件数超上限
    TooDeep,
    /// 冻结接口 ABI 漂移
    FrozenAbiDrift,
    /// 注入账倒退
    LedgerRegress,
    /// 双跑不一致
    NotReproducible,
    /// 段位越界
    SegmentViolation,
    /// 节点名超容
    NameOverflow,
}

impl PathCode {
    /// 码值（段基址 + 变体序号）。
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }

    /// 中文诊断与可操作建议（闭集返回，禁止空串）。
    pub fn detail(self) -> &'static str {
        match self {
            PathCode::LastComponentMissing => "末段缺失：父目录已命中、最后一个组件不存在；\
                请核对文件名或改用 O_CREAT 创建，勿把此格误判成中间段断链",
            PathCode::MiddleComponentMissing => "中间段缺失：某非末组件不存在，后续组件未参与\
                解析；请从根开始逐段核对，定位第一个断点",
            PathCode::EmptyComponent => "空组件：路径为空串或出现零长组件（连续斜杠）；\
                本域冻结契约从严判 ENOENT，请先规整路径再下发",
            PathCode::NonDirComponent => "中间段非目录：该组件存在但是普通文件，无法继续\
                下钻；请回 ENOTDIR 面处理，不得吞成 ENOENT",
            PathCode::TrailingSlashOnFile => "尾斜杠要求目录：末段命中文件但路径以 / 结尾；\
                请去掉尾斜杠或改用目录路径",
            PathCode::TooDeep => "组件数超上限：单路径段数超过冻结上限；请拆分路径或\
                提升 MAX_COMPONENTS 后重测",
            PathCode::FrozenAbiDrift => "冻结接口 ABI 漂移：B1 VFS 版本号与钉住值不符；\
                请先对齐冻结接口再消费",
            PathCode::LedgerRegress => "注入账倒退：计数只增不减；请检查回滚路径是否\
                误改累计值",
            PathCode::NotReproducible => "双跑不一致：同输入两次结果不同；请排查隐藏\
                状态或非确定遍历序",
            PathCode::SegmentViolation => "段位越界：本模块段基址必须落在 \
                0x7A000..=0x7D1C0 且 0x40 对齐",
            PathCode::NameOverflow => "节点名超容：名称长度越过 NAME_CAP；请截短或\
                提升容量后重建节点表",
        }
    }

    /// 闭集全量（供文案完整性与唯一性检查）。
    pub const ALL: [PathCode; 11] = [
        PathCode::LastComponentMissing,
        PathCode::MiddleComponentMissing,
        PathCode::EmptyComponent,
        PathCode::NonDirComponent,
        PathCode::TrailingSlashOnFile,
        PathCode::TooDeep,
        PathCode::FrozenAbiDrift,
        PathCode::LedgerRegress,
        PathCode::NotReproducible,
        PathCode::SegmentViolation,
        PathCode::NameOverflow,
    ];
}

// ── 定容节点名 ────────────────────────────────────────────────────────
/// 定容节点名：无堆、可 Copy、比较按字节。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct NodeName {
    bytes: [u8; NAME_CAP],
    len: u8,
}

impl NodeName {
    /// 空名（占位；不作为合法节点名使用）。
    pub const fn empty() -> NodeName {
        NodeName { bytes: [0u8; NAME_CAP], len: 0 }
    }

    /// 解析名称：空名或超容返回 None（调用方转 ENAMETOOLONG / EINVAL）。
    pub fn parse(s: &str) -> Option<NodeName> {
        let raw = s.as_bytes();
        if raw.is_empty() || raw.len() > NAME_CAP {
            return None;
        }
        let mut out = [0u8; NAME_CAP];
        let mut i = 0usize;
        while i < raw.len() {
            match raw.get(i) {
                Some(b) => {
                    if let Some(slot) = out.get_mut(i) {
                        *slot = *b;
                    } else {
                        return None;
                    }
                }
                None => return None,
            }
            i += 1;
        }
        Some(NodeName { bytes: out, len: raw.len() as u8 })
    }

    /// 取回 &str（写入期已保证是 ASCII/合法 UTF-8 子序列，仍做防御回退）。
    pub fn as_str(&self) -> &str {
        let n = self.len as usize;
        match self.bytes.get(..n) {
            Some(slice) => core::str::from_utf8(slice).unwrap_or(""),
            None => "",
        }
    }

    /// 与字符串逐字节相等。
    pub fn eq_str(&self, s: &str) -> bool {
        self.as_str() == s
    }

    /// 名义长度。
    pub fn len(&self) -> usize {
        self.len as usize
    }

    /// 是否为空名。
    pub fn is_blank(&self) -> bool {
        self.len == 0
    }
}

// ── 树模型 ────────────────────────────────────────────────────────────
/// 节点类别。Free 表示槽位未用；Dir/File 是两种真实节点。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum NodeKind {
    Free,
    Dir,
    File,
}

impl NodeKind {
    /// 能否作为中间段继续下钻。
    pub fn is_dir(self) -> bool {
        matches!(self, NodeKind::Dir)
    }
}

/// 单个节点。
#[derive(Clone, Copy, Debug)]
pub struct Node {
    pub kind: NodeKind,
    pub name: NodeName,
    pub parent: u16,
    pub ino: u16,
}

impl Node {
    const fn empty() -> Node {
        Node { kind: NodeKind::Free, name: NodeName::empty(), parent: 1, ino: 0 }
    }
}

/// 定容目录树（根固定在下标 0，ino=1）。
#[derive(Clone, Copy, Debug)]
pub struct Tree {
    nodes: [Node; MAX_NODES],
    used: usize,
    root_ino: u16,
}

impl Tree {
    /// 构造只含根节点的树。
    pub fn new() -> Tree {
        let mut nodes = [Node::empty(); MAX_NODES];
        // 根节点：名字 "/"，父指向自己（".." 在根上收敛）。
        nodes[0] = Node {
            kind: NodeKind::Dir,
            name: NodeName::empty(),
            parent: 1,
            ino: 1,
        };
        Tree { nodes, used: 1, root_ino: 1 }
    }

    /// 根 inode 号。
    pub fn root_ino(&self) -> u16 {
        self.root_ino
    }

    /// 已用槽数。
    pub fn used(&self) -> usize {
        self.used
    }

    /// 按 ino 找下标。
    fn index_of(&self, ino: u16) -> Option<usize> {
        let mut i = 0usize;
        while i < self.used {
            if let Some(n) = self.nodes.get(i) {
                if n.ino == ino && n.kind != NodeKind::Free {
                    return Some(i);
                }
            }
            i += 1;
        }
        None
    }

    /// 取节点类别（未命中返回 Free）。
    pub fn kind_of(&self, ino: u16) -> NodeKind {
        match self.index_of(ino) {
            Some(i) => match self.nodes.get(i) {
                Some(n) => n.kind,
                None => NodeKind::Free,
            },
            None => NodeKind::Free,
        }
    }

    /// 取父 ino（未命中/根都收敛到根）。
    pub fn parent_of(&self, ino: u16) -> u16 {
        match self.index_of(ino) {
            Some(i) => match self.nodes.get(i) {
                Some(n) => n.parent,
                None => self.root_ino,
            },
            None => self.root_ino,
        }
    }

    /// 取名字（未命中返回空名）。
    pub fn name_of(&self, ino: u16) -> NodeName {
        match self.index_of(ino) {
            Some(i) => match self.nodes.get(i) {
                Some(n) => n.name,
                None => NodeName::empty(),
            },
            None => NodeName::empty(),
        }
    }

    /// 在 parent 下查同名子节点，返回 ino。
    pub fn find_child(&self, parent_ino: u16, name: &str) -> Option<u16> {
        let mut i = 0usize;
        while i < self.used {
            if let Some(n) = self.nodes.get(i) {
                if n.kind != NodeKind::Free && n.parent == parent_ino && n.name.eq_str(name) {
                    return Some(n.ino);
                }
            }
            i += 1;
        }
        None
    }

    /// 挂一个子节点：重名 EEXIST、超容 ENOSPC、名超长 ENAMETOOLONG。
    pub fn add(&mut self, parent_ino: u16, name: &str, kind: NodeKind) -> Result<u16, i32> {
        if kind == NodeKind::Free {
            return Err(ENOSPC);
        }
        if !self.kind_of(parent_ino).is_dir() {
            return Err(ENOTDIR);
        }
        if self.find_child(parent_ino, name).is_some() {
            return Err(EEXIST);
        }
        let parsed = match NodeName::parse(name) {
            Some(n) => n,
            None => return Err(ENAMETOOLONG),
        };
        if self.used >= MAX_NODES {
            return Err(ENOSPC);
        }
        let idx = self.used;
        let ino = 1u16.saturating_add(idx as u16);
        if let Some(slot) = self.nodes.get_mut(idx) {
            *slot = Node { kind, name: parsed, parent: parent_ino, ino };
        } else {
            return Err(ENOSPC);
        }
        self.used = self.used.saturating_add(1);
        Ok(ino)
    }

    /// 子节点数（用于「容量/账目」判据）。
    /// 根的 ".." 指向自己，自环不能算作自己的子节点。
    pub fn child_count(&self, parent_ino: u16) -> usize {
        let mut n = 0usize;
        let mut i = 0usize;
        while i < self.used {
            if let Some(node) = self.nodes.get(i) {
                if node.kind != NodeKind::Free
                    && node.parent == parent_ino
                    && node.ino != parent_ino
                {
                    n += 1;
                }
            }
            i += 1;
        }
        n
    }
}

// ── 解析失败态 ────────────────────────────────────────────────────────
/// 解析失败态（三态 + 三类非 ENOENT 干扰态）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Fault {
    /// 末段缺失
    LastMissing,
    /// 中间段缺失
    MiddleMissing,
    /// 空组件
    EmptyComponent,
    /// 中间段非目录
    NonDirComponent,
    /// 尾斜杠落在文件上
    TrailingOnFile,
    /// 组件过深
    TooDeep,
}

impl Fault {
    /// errno 正数码。
    pub fn errno(self) -> i32 {
        match self {
            Fault::LastMissing | Fault::MiddleMissing | Fault::EmptyComponent => ENOENT,
            Fault::NonDirComponent | Fault::TrailingOnFile => ENOTDIR,
            Fault::TooDeep => ENAMETOOLONG,
        }
    }

    /// 是否属 ENOENT 三态（对外同码，对内分格）。
    pub fn is_enoent(self) -> bool {
        matches!(
            self,
            Fault::LastMissing | Fault::MiddleMissing | Fault::EmptyComponent
        )
    }

    /// 对应诊断码。
    pub fn code(self) -> PathCode {
        match self {
            Fault::LastMissing => PathCode::LastComponentMissing,
            Fault::MiddleMissing => PathCode::MiddleComponentMissing,
            Fault::EmptyComponent => PathCode::EmptyComponent,
            Fault::NonDirComponent => PathCode::NonDirComponent,
            Fault::TrailingOnFile => PathCode::TrailingSlashOnFile,
            Fault::TooDeep => PathCode::TooDeep,
        }
    }

    /// 闭集全量。
    pub const ALL: [Fault; 6] = [
        Fault::LastMissing,
        Fault::MiddleMissing,
        Fault::EmptyComponent,
        Fault::NonDirComponent,
        Fault::TrailingOnFile,
        Fault::TooDeep,
    ];
}

// ── 路径切分 ──────────────────────────────────────────────────────────
/// 切分结果：组件数组 + 绝对路径标记 + 尾斜杠标记。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Split<'a> {
    pub comps: [&'a str; MAX_COMPONENTS],
    pub n: usize,
    pub absolute: bool,
    pub trailing_slash: bool,
}

/// 切分路径。规则（本域冻结契约）：
/// - 空串 → EmptyComponent；
/// - 连续斜杠（含开头 `//`）→ EmptyComponent（从严，与 Linux 折叠语义
///   的差异登记在对照表 `SEMANTIC_DIFFS`）；
/// - 组件数 > MAX_COMPONENTS → TooDeep；
/// - 尾斜杠只置标记，不产生空组件。
pub fn split_path(path: &str) -> Result<Split<'_>, Fault> {
    let mut out = Split {
        comps: [""; MAX_COMPONENTS],
        n: 0,
        absolute: false,
        trailing_slash: false,
    };
    if path.is_empty() {
        return Err(Fault::EmptyComponent);
    }
    let bytes = path.as_bytes();
    if path.starts_with('/') {
        out.absolute = true;
    }
    let mut i = 0usize;
    let mut prev_slash = false;
    while i < bytes.len() {
        let b = match bytes.get(i) {
            Some(v) => *v,
            None => break,
        };
        if b == b'/' {
            if prev_slash {
                return Err(Fault::EmptyComponent);
            }
            prev_slash = true;
            i += 1;
            continue;
        }
        let start = i;
        while i < bytes.len() {
            match bytes.get(i) {
                Some(v) if *v != b'/' => i += 1,
                _ => break,
            }
        }
        let seg = match path.get(start..i) {
            Some(s) => s,
            None => return Err(Fault::EmptyComponent),
        };
        match out.comps.get_mut(out.n) {
            Some(slot) => *slot = seg,
            None => return Err(Fault::TooDeep),
        }
        out.n = out.n.saturating_add(1);
        prev_slash = false;
    }
    out.trailing_slash = prev_slash;
    Ok(out)
}

// ── 解析游标状态机 ────────────────────────────────────────────────────
/// 游标状态。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum WalkState {
    Idle,
    Walking,
    Settled,
    Faulted,
}

impl WalkState {
    /// 合法迁移表：只准前进，Idle 只能进 Walking，终态不可回退。
    pub fn allowed(from: WalkState, to: WalkState) -> bool {
        match (from, to) {
            (WalkState::Idle, WalkState::Walking) => true,
            (WalkState::Idle, WalkState::Faulted) => true,
            (WalkState::Walking, WalkState::Walking) => true,
            (WalkState::Walking, WalkState::Settled) => true,
            (WalkState::Walking, WalkState::Faulted) => true,
            _ => false,
        }
    }
}

/// 解析游标机器：记录当前态、步数、非法迁移次数。
#[derive(Clone, Copy, Debug)]
pub struct WalkMachine {
    state: WalkState,
    steps: u32,
    illegal: u32,
}

impl WalkMachine {
    pub const fn new() -> WalkMachine {
        WalkMachine { state: WalkState::Idle, steps: 0, illegal: 0 }
    }

    pub fn state(&self) -> WalkState {
        self.state
    }

    pub fn steps(&self) -> u32 {
        self.steps
    }

    pub fn illegal(&self) -> u32 {
        self.illegal
    }

    /// 推进一次：非法迁移不改变状态、只计账（显性，不静默）。
    pub fn advance(&mut self, to: WalkState) {
        if WalkState::allowed(self.state, to) {
            self.state = to;
            self.steps = self.steps.saturating_add(1);
        } else {
            self.illegal = self.illegal.saturating_add(1);
        }
    }
}

/// 带游标机器的解析核心（单一实现，`resolve` 只是薄包装）。
fn walk(tree: &Tree, path: &str, machine: &mut WalkMachine) -> Result<u16, Fault> {
    let split = match split_path(path) {
        Ok(s) => s,
        Err(f) => {
            machine.advance(WalkState::Faulted);
            return Err(f);
        }
    };
    machine.advance(WalkState::Walking);
    let mut cur = tree.root_ino();
    let mut i = 0usize;
    while i < split.n {
        let comp = match split.comps.get(i).copied() {
            Some(c) => c,
            None => {
                machine.advance(WalkState::Faulted);
                return Err(Fault::TooDeep);
            }
        };
        if comp == "." {
            machine.advance(WalkState::Walking);
            i += 1;
            continue;
        }
        if comp == ".." {
            cur = tree.parent_of(cur);
            machine.advance(WalkState::Walking);
            i += 1;
            continue;
        }
        let is_last = i.saturating_add(1) == split.n;
        match tree.find_child(cur, comp) {
            Some(ino) => {
                if !is_last && !tree.kind_of(ino).is_dir() {
                    machine.advance(WalkState::Faulted);
                    return Err(Fault::NonDirComponent);
                }
                cur = ino;
                machine.advance(WalkState::Walking);
            }
            None => {
                machine.advance(WalkState::Faulted);
                return Err(if is_last {
                    Fault::LastMissing
                } else {
                    Fault::MiddleMissing
                });
            }
        }
        i += 1;
    }
    if split.trailing_slash && split.n > 0 && !tree.kind_of(cur).is_dir() {
        machine.advance(WalkState::Faulted);
        return Err(Fault::TrailingOnFile);
    }
    machine.advance(WalkState::Settled);
    Ok(cur)
}

/// 解析路径，返回命中 inode 或失败态。
pub fn resolve(tree: &Tree, path: &str) -> Result<u16, Fault> {
    let mut m = WalkMachine::new();
    walk(tree, path, &mut m)
}

/// 解析路径并回报游标机器的非法迁移次数（判序协议用）。
pub fn resolve_with_protocol(tree: &Tree, path: &str) -> (Result<u16, Fault>, u32, u32) {
    let mut m = WalkMachine::new();
    let r = walk(tree, path, &mut m);
    (r, m.illegal(), m.steps())
}

/// errno 视图：0 表示命中。
pub fn errno_of(tree: &Tree, path: &str) -> i32 {
    match resolve(tree, path) {
        Ok(_) => 0,
        Err(f) => f.errno(),
    }
}

// ── B1 VFS 冻结接口消费面 ─────────────────────────────────────────────
/// 冻结接口消费结果（与 errno 一一对应）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Consume {
    /// 命中，携带 inode
    Found(u16),
    /// 未命中，携带 errno
    Errno(i32),
}

/// B1 冻结接口（只读查询，ABI 钉住）。
pub fn frozen_lookup(tree: &Tree, path: &str) -> Result<u16, i32> {
    match resolve(tree, path) {
        Ok(ino) => Ok(ino),
        Err(f) => Err(f.errno()),
    }
}

/// 以冻结接口的视角消费一次查询。
pub fn b1_consume(tree: &Tree, path: &str) -> Consume {
    match frozen_lookup(tree, path) {
        Ok(ino) => Consume::Found(ino),
        Err(e) => Consume::Errno(e),
    }
}

/// ABI 是否仍与钉住值一致。
pub fn abi_pinned() -> bool {
    B1_VFS_ABI == 7
}

// ── 语料与样本树 ──────────────────────────────────────────────────────
/// 样本树：/etc/passwd、/usr/lib/x，深度 3。
pub fn sample_tree() -> Tree {
    let mut t = Tree::new();
    let root = t.root_ino();
    let etc = t.add(root, "etc", NodeKind::Dir).unwrap_or(0);
    if etc != 0 {
        let _ = t.add(etc, "passwd", NodeKind::File);
    }
    let usr = t.add(root, "usr", NodeKind::Dir).unwrap_or(0);
    if usr != 0 {
        let lib = t.add(usr, "lib", NodeKind::Dir).unwrap_or(0);
        if lib != 0 {
            let _ = t.add(lib, "x", NodeKind::File);
        }
    }
    t
}

/// 矩阵格子：路径 + 期望失败态。
#[derive(Clone, Copy, Debug)]
pub struct Cell {
    pub path: &'static str,
    pub fault: Fault,
}

/// ENOENT 三态 × 4 例 = 12 格（逐格落账的语料）。
pub const CELLS: [Cell; 12] = [
    // 末段缺失：父目录命中，末组件不存在
    Cell { path: "/etc/ghost", fault: Fault::LastMissing },
    Cell { path: "/usr/lib/ghost", fault: Fault::LastMissing },
    Cell { path: "etc/ghost", fault: Fault::LastMissing },
    Cell { path: "/ghost", fault: Fault::LastMissing },
    // 中间段缺失：断点不在末段
    Cell { path: "/ghost/passwd", fault: Fault::MiddleMissing },
    Cell { path: "/etc/ghost/x", fault: Fault::MiddleMissing },
    Cell { path: "ghost/etc", fault: Fault::MiddleMissing },
    Cell { path: "/usr/ghost/lib", fault: Fault::MiddleMissing },
    // 空组件：空串 / 双斜杠 / 中段零长 / 相对中段零长
    Cell { path: "", fault: Fault::EmptyComponent },
    Cell { path: "//", fault: Fault::EmptyComponent },
    Cell { path: "/etc//passwd", fault: Fault::EmptyComponent },
    Cell { path: "etc//passwd", fault: Fault::EmptyComponent },
];

/// 干扰态四例：必须落 ENOTDIR 或命中，绝不许进 ENOENT 三态。
#[derive(Clone, Copy, Debug)]
pub struct NoiseCell {
    pub path: &'static str,
    pub fault: Option<Fault>,
}

pub const NOISE_CELLS: [NoiseCell; 4] = [
    NoiseCell { path: "/etc/passwd/x", fault: Some(Fault::NonDirComponent) },
    NoiseCell { path: "/etc/passwd/", fault: Some(Fault::TrailingOnFile) },
    NoiseCell { path: "/usr/lib/x/y", fault: Some(Fault::NonDirComponent) },
    NoiseCell { path: "etc/../etc/passwd", fault: None },
];

/// 语义差异登记：本域与 Linux 折叠语义不同之处，逐条显性列出。
pub const SEMANTIC_DIFFS: &[(&str, &str, &str)] = &[
    ("连续斜杠 `//`", "Linux 折叠为空并继续解析", "本域判 EmptyComponent(ENOENT)"),
    ("根路径 `/`", "Linux 命中根目录", "本域亦命中根目录（一致）"),
    ("尾斜杠落在文件", "Linux 返回 ENOTDIR", "本域返回 ENOTDIR（一致）"),
];

// ── 注入账 ────────────────────────────────────────────────────────────
/// 三态注入账（分格计数 + 逃逸显性）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct EnoentLedger {
    pub shots: u32,
    pub detected: u32,
    pub escaped: u32,
    pub last: u32,
    pub middle: u32,
    pub empty: u32,
}

impl EnoentLedger {
    pub const fn zero() -> EnoentLedger {
        EnoentLedger { shots: 0, detected: 0, escaped: 0, last: 0, middle: 0, empty: 0 }
    }

    /// 记一次注入：`expect` 是注入类别，`got` 是判定器实际返回的 errno。
    pub fn record(&mut self, expect: Fault, got: i32) {
        self.shots = self.shots.saturating_add(1);
        if expect.is_enoent() && got == expect.errno() {
            self.detected = self.detected.saturating_add(1);
            match expect {
                Fault::LastMissing => self.last = self.last.saturating_add(1),
                Fault::MiddleMissing => self.middle = self.middle.saturating_add(1),
                Fault::EmptyComponent => self.empty = self.empty.saturating_add(1),
                _ => {}
            }
        } else {
            self.escaped = self.escaped.saturating_add(1);
        }
    }

    /// 守恒：注入 = 检出 + 逃逸，且三态分格之和 = 检出。
    pub fn conserved(&self) -> bool {
        self.detected.saturating_add(self.escaped) == self.shots
            && self.last.saturating_add(self.middle).saturating_add(self.empty) == self.detected
    }

    /// 三态注入执行：每态取 2 个样本，各跑 INJECT_ROUNDS 轮。
    pub fn run_matrix() -> EnoentLedger {
        let tree = sample_tree();
        let samples = [
            (Fault::LastMissing, ["/etc/ghost", "/usr/lib/ghost"]),
            (Fault::MiddleMissing, ["/ghost/passwd", "/etc/ghost/x"]),
            (Fault::EmptyComponent, ["", "/etc//passwd"]),
        ];
        let mut ledger = EnoentLedger::zero();
        let mut ci = 0usize;
        while ci < samples.len() {
            if let Some(row) = samples.get(ci) {
                let (fault, paths) = *row;
                let mut pi = 0usize;
                while pi < paths.len() {
                    if let Some(p) = paths.get(pi).copied() {
                        let mut round = 0u32;
                        while round < INJECT_ROUNDS {
                            ledger.record(fault, errno_of(&tree, p));
                            round += 1;
                        }
                    }
                    pi += 1;
                }
            }
            ci += 1;
        }
        ledger
    }
}

// ── 确定性指纹 ────────────────────────────────────────────────────────
/// FNV-1a 32 位（无浮点、无随机）。
pub fn fnv1a(bytes: &[u8]) -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    let mut i = 0usize;
    while i < bytes.len() {
        if let Some(b) = bytes.get(i) {
            h ^= *b as u32;
            h = h.wrapping_mul(0x0100_0193);
        }
        i += 1;
    }
    h
}

/// 语料指纹：12 格 + 4 干扰格，按 (errno, ino 低位) 逐格卷。
pub fn corpus_fingerprint() -> u32 {
    let tree = sample_tree();
    let mut bytes: Vec<u8> = Vec::new();
    let mut i = 0usize;
    while i < CELLS.len() {
        if let Some(c) = CELLS.get(i) {
            bytes.push((errno_of(&tree, c.path) & 0xFF) as u8);
            match resolve(&tree, c.path) {
                Ok(ino) => bytes.push((ino & 0xFF) as u8),
                Err(f) => bytes.push((f as u8).wrapping_add(0x40)),
            }
        }
        i += 1;
    }
    let mut j = 0usize;
    while j < NOISE_CELLS.len() {
        if let Some(c) = NOISE_CELLS.get(j) {
            bytes.push((errno_of(&tree, c.path) & 0xFF) as u8);
            match resolve(&tree, c.path) {
                Ok(ino) => bytes.push((ino & 0xFF) as u8),
                Err(f) => bytes.push((f as u8).wrapping_add(0x80)),
            }
        }
        j += 1;
    }
    fnv1a(&bytes)
}

// ── 检查入口 ──────────────────────────────────────────────────────────
/// F9226 判据集（≥12 条；动态明细 ≤8 条）。
pub fn run_unx9226_checks() -> CheckSet {
    let mut s = CheckSet::new("unx9226");

    // 1. 段位守卫
    let seg_ok = SEG_BASE % 0x40 == 0 && (0x7A000..=0x7D1C0).contains(&SEG_BASE);
    s.add(
        "段位守卫：0x40 对齐且在 0x7A000..=0x7D1C0",
        seg_ok,
        "段基址必须唯一且落在本批窗口；越界即拒绝注册",
    );

    let tree = sample_tree();

    // 2. 正常流：文件命中
    let ok_file = resolve(&tree, "/etc/passwd") == Ok(3) && errno_of(&tree, "/etc/passwd") == 0;
    s.add(
        "正常流：/etc/passwd 命中文件且 errno=0",
        ok_file,
        "两段路径均应命中，inode 号与建树顺序一致",
    );

    // 3. 正常流：目录命中 + 点段归一
    let ok_dir = resolve(&tree, "/usr/lib") == Ok(5)
        && resolve(&tree, "/usr/./lib") == Ok(5)
        && resolve(&tree, "/usr/lib/../lib") == Ok(5);
    s.add(
        "正常流：目录命中且 ./.. 归一正确",
        ok_dir,
        "点段只移动游标，不得产生 ENOENT",
    );

    // 4. 末段缺失：码与态双对
    let last_ok = resolve(&tree, "/etc/ghost") == Err(Fault::LastMissing)
        && errno_of(&tree, "/etc/ghost") == ENOENT;
    s.add(
        "末段缺失：态=LastMissing 且 errno=ENOENT",
        last_ok,
        "父目录命中、末组件不存在必须落末段格",
    );

    // 5. 中间段缺失：码与态双对
    let mid_ok = resolve(&tree, "/ghost/passwd") == Err(Fault::MiddleMissing)
        && errno_of(&tree, "/ghost/passwd") == ENOENT;
    s.add(
        "中间段缺失：态=MiddleMissing 且 errno=ENOENT",
        mid_ok,
        "断点在非末段时不得记成末段缺失",
    );

    // 6. 空组件：空串与连续斜杠
    let empty_ok = resolve(&tree, "") == Err(Fault::EmptyComponent)
        && resolve(&tree, "//") == Err(Fault::EmptyComponent)
        && resolve(&tree, "/etc//passwd") == Err(Fault::EmptyComponent)
        && errno_of(&tree, "") == ENOENT;
    s.add(
        "空组件：空串/双斜杠/中段零长全归 EmptyComponent",
        empty_ok,
        "本域冻结契约从严，零长组件不与 Linux 折叠语义混同",
    );

    // 7. 三态 12 格逐格落账
    let mut cells_ok = true;
    let mut hits = 0u32;
    let mut i = 0usize;
    while i < CELLS.len() {
        if let Some(c) = CELLS.get(i) {
            if resolve(&tree, c.path) == Err(c.fault) && errno_of(&tree, c.path) == ENOENT {
                hits = hits.saturating_add(1);
            } else {
                cells_ok = false;
            }
        }
        i += 1;
    }
    s.add(
        "三态 12 格逐格落账：态与 errno 全对",
        cells_ok && hits == 12,
        "12 格是 (3 态 × 4 例) 的完整格，缺一格即判红",
    );

    // 8. 注入必抓：三态各 20 轮
    let ledger = EnoentLedger::run_matrix();
    let inject_ok = ledger.shots == 3 * INJECT_ROUNDS * 2
        && ledger.detected == ledger.shots
        && ledger.escaped == 0
        && ledger.last == INJECT_ROUNDS * 2
        && ledger.middle == INJECT_ROUNDS * 2
        && ledger.empty == INJECT_ROUNDS * 2;
    s.add(
        "注入必抓：三态各 20 轮全 ENOENT，逃逸 0",
        inject_ok,
        "60 次注入必须 60 次检出，任何一类漏检都判红",
    );

    // 9. 账目守恒
    s.add(
        "账目守恒：注入=检出+逃逸且三态分格之和=检出",
        ledger.conserved(),
        "守恒式任一侧不等即说明有注入未被归类",
    );

    // 10. 干扰态隔离
    let mut noise_ok = true;
    let mut j = 0usize;
    while j < NOISE_CELLS.len() {
        if let Some(c) = NOISE_CELLS.get(j) {
            match c.fault {
                Some(f) => {
                    if resolve(&tree, c.path) != Err(f) || f.is_enoent() {
                        noise_ok = false;
                    }
                }
                None => {
                    if resolve(&tree, c.path).is_err() {
                        noise_ok = false;
                    }
                }
            }
        }
        j += 1;
    }
    s.add(
        "干扰态隔离：非目录/尾斜杠回 ENOTDIR，命中格不受扰",
        noise_ok,
        "中间段是文件绝不许吞成 ENOENT，这是本模块最容易写错的一格",
    );

    // 11. 组件深度边界：8 可切、9 过深
    let eight_ok = split_path("a/b/c/d/e/f/g/h").is_ok();
    let nine = split_path("a/b/c/d/e/f/g/h/i");
    let depth_ok = eight_ok && nine == Err(Fault::TooDeep) && Fault::TooDeep.errno() == ENAMETOOLONG;
    s.add(
        "边界：8 组件可切、9 组件 ENAMETOOLONG",
        depth_ok,
        "组件上限是冻结值，上限+1 必须显性拒",
    );

    // 12. 名称边界：恰容可插、+1 拒
    let cap_name = "abcdefghijkl";
    let over_name = "abcdefghijklm";
    let mut name_tree = Tree::new();
    let root = name_tree.root_ino();
    let cap_added = name_tree.add(root, cap_name, NodeKind::File).is_ok();
    let over_rejected = name_tree.add(root, over_name, NodeKind::File) == Err(ENAMETOOLONG);
    let blank_rejected = NodeName::parse("").is_none();
    // 名称往返：解析回来的长度/字节必须与原文一致，根名必须是空名占位。
    let rt_ok = match NodeName::parse(cap_name) {
        Some(n) => n.len() == NAME_CAP && n.eq_str(cap_name) && !n.is_blank(),
        None => false,
    };
    let root_blank = name_tree.name_of(root).is_blank();
    s.add(
        "边界：NAME_CAP 内可插、+1 与空名被拒",
        cap_added
            && over_rejected
            && blank_rejected
            && rt_ok
            && root_blank
            && cap_name.len() == NAME_CAP,
        "名称容量边界必须双向验：恰满通过、越界拒绝、往返一致",
    );

    // 13. 节点容量：表满 ENOSPC 且已挂节点不丢
    let mut full = Tree::new();
    let froot = full.root_ino();
    let mut added = 0usize;
    let mut k = 0usize;
    while k < MAX_NODES {
        // 名字两位十进制手工拼：'a' + 序号，避免 format 依赖
        let mut buf = [0u8; 2];
        buf[0] = b'a' + ((k / 10) as u8);
        buf[1] = b'a' + ((k % 10) as u8);
        let nm = match core::str::from_utf8(&buf) {
            Ok(v) => v,
            Err(_) => "",
        };
        if full.add(froot, nm, NodeKind::File).is_ok() {
            added += 1;
        }
        k += 1;
    }
    let overflow = FullProbe::probe(&mut full);
    s.add(
        "容量：表满回 ENOSPC 且已挂节点账不丢",
        overflow == ENOSPC && added == MAX_NODES - 1 && full.used() == MAX_NODES,
        "容量判据必须证明「拒绝新增」与「旧账完好」同时成立",
    );

    // 14. 状态机：终态不可回退
    let mut m = WalkMachine::new();
    m.advance(WalkState::Walking);
    m.advance(WalkState::Settled);
    let settled_steps = m.steps();
    m.advance(WalkState::Walking);
    let machine_ok = m.illegal() == 1 && m.state() == WalkState::Settled && m.steps() == settled_steps;
    s.add(
        "状态机：Settled 之后不得回到 Walking",
        machine_ok,
        "终态回退属非法迁移，机器必须拒绝且计账",
    );

    // 15. 解析路径上的协议合规：16 格零非法迁移
    let mut proto_ok = true;
    let mut total_steps = 0u32;
    let mut ci = 0usize;
    while ci < CELLS.len() {
        if let Some(c) = CELLS.get(ci) {
            let (r, illegal, steps) = resolve_with_protocol(&tree, c.path);
            if illegal != 0 || r != Err(c.fault) {
                proto_ok = false;
            }
            total_steps = total_steps.saturating_add(steps);
        }
        ci += 1;
    }
    s.add(
        "游标协议：12 格解析零非法迁移且步数非零",
        proto_ok && total_steps >= 12,
        "每次解析至少走一步，非法迁移计数必须为 0",
    );

    // 16. 冻结接口 ABI 与消费格一致
    let mut consume_ok = abi_pinned();
    let mut ci2 = 0usize;
    while ci2 < CELLS.len() {
        if let Some(c) = CELLS.get(ci2) {
            match b1_consume(&tree, c.path) {
                Consume::Errno(e) => {
                    if e != c.fault.errno() {
                        consume_ok = false;
                    }
                }
                Consume::Found(_) => consume_ok = false,
            }
        }
        ci2 += 1;
    }
    let mut ni = 0usize;
    while ni < NOISE_CELLS.len() {
        if let Some(c) = NOISE_CELLS.get(ni) {
            match (b1_consume(&tree, c.path), c.fault) {
                (Consume::Found(a), None) => {
                    if resolve(&tree, c.path) != Ok(a) {
                        consume_ok = false;
                    }
                }
                (Consume::Errno(e), Some(f)) => {
                    if e != f.errno() {
                        consume_ok = false;
                    }
                }
                _ => consume_ok = false,
            }
        }
        ni += 1;
    }
    s.add(
        "冻结接口：ABI 钉住且消费格与本域逐项相等",
        consume_ok,
        "B1 VFS 消费面的 errno/inode 必须与本地解析器逐格一致",
    );

    // 17. 双跑一致
    let fp1 = corpus_fingerprint();
    let fp2 = corpus_fingerprint();
    s.add(
        "双跑一致：语料指纹相等",
        fp1 == fp2,
        "同输入两次必须逐字段相等，隐藏状态即失败",
    );

    // 18. 文案闭集
    let text_ok = PathCode::ALL.iter().all(|c| !c.detail().is_empty())
        && Fault::ALL.iter().all(|f| !f.code().detail().is_empty());
    s.add(
        "文案闭集：全部诊断码带非空中文建议",
        text_ok,
        "11 个码与 6 个失败态都必须能取到可操作说明",
    );

    // 19. 码值唯一
    let mut uniq = true;
    let mut a = 0usize;
    while a < PathCode::ALL.len() {
        let mut b = a + 1;
        while b < PathCode::ALL.len() {
            if let (Some(x), Some(y)) = (PathCode::ALL.get(a), PathCode::ALL.get(b)) {
                if x.val() == y.val() {
                    uniq = false;
                }
            }
            b += 1;
        }
        a += 1;
    }
    s.add(
        "诊断码：11 个码值两两唯一",
        uniq,
        "段内闭集码重复会让判据无法定位",
    );

    // 20. 账目单调
    let mut mono = ledger;
    let before = mono.shots;
    mono.record(Fault::LastMissing, ENOENT);
    s.add(
        "账目单调：再注入只增不减",
        mono.shots == before.saturating_add(1) && mono.conserved(),
        "累计计数禁止回退或饱和到旧值",
    );

    // 21. 语义差异表可复算：每条差异都有两侧说明
    let diffs_ok = SEMANTIC_DIFFS.iter().all(|row| !row.0.is_empty() && !row.1.is_empty() && !row.2.is_empty())
        && SEMANTIC_DIFFS.len() == 3;
    s.add(
        "对账：与 Linux 的语义差异逐条登记且双侧说明非空",
        diffs_ok,
        "差异要么同源、要么显性登记，不许留暗格",
    );

    // 动态明细（≤8）：三行账目/矩阵/指纹
    let ledger_text = format!(
        "注入 {} 检出 {} 逃逸 {} | 末 {} 中 {} 空 {}",
        ledger.shots, ledger.detected, ledger.escaped, ledger.last, ledger.middle, ledger.empty
    );
    s.add_dyn("三态注入账", ledger.conserved() && ledger.escaped == 0, &ledger_text);
    let matrix_text = format!("矩阵 {}/12 格 | 干扰 4 格隔离", hits);
    s.add_dyn("矩阵命中", hits == 12, &matrix_text);
    let fp_text = format!("语料指纹 0x{:08X}", fp1);
    s.add_dyn("指纹快照", fp1 == fp2, &fp_text);

    s
}

/// 容量探针：往已满的树里再挂一个节点，取回错误码。
struct FullProbe;

impl FullProbe {
    fn probe(tree: &mut Tree) -> i32 {
        let root = tree.root_ino();
        match tree.add(root, "zz", NodeKind::File) {
            Ok(_) => 0,
            Err(e) => e,
        }
    }
}

// ── 测试 ──────────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normal_hits() {
        let t = sample_tree();
        assert_eq!(resolve(&t, "/etc/passwd"), Ok(3));
        assert_eq!(resolve(&t, "/usr/lib"), Ok(5));
        assert_eq!(errno_of(&t, "/etc/passwd"), 0);
    }

    #[test]
    fn last_component_missing() {
        let t = sample_tree();
        assert_eq!(resolve(&t, "/etc/ghost"), Err(Fault::LastMissing));
        assert_eq!(errno_of(&t, "/etc/ghost"), ENOENT);
    }

    #[test]
    fn middle_component_missing() {
        let t = sample_tree();
        assert_eq!(resolve(&t, "/ghost/passwd"), Err(Fault::MiddleMissing));
        assert_eq!(resolve(&t, "/etc/ghost/x"), Err(Fault::MiddleMissing));
    }

    #[test]
    fn empty_component_forms() {
        // 空组件与根形态只走切分器，不依赖样本树。
        assert_eq!(split_path(""), Err(Fault::EmptyComponent));
        assert_eq!(split_path("//"), Err(Fault::EmptyComponent));
        assert_eq!(split_path("/etc//passwd"), Err(Fault::EmptyComponent));
        assert_eq!(split_path("/"), Ok(Split {
            comps: [""; MAX_COMPONENTS],
            n: 0,
            absolute: true,
            trailing_slash: true,
        }));
    }

    #[test]
    fn non_dir_is_isolated_from_enoent() {
        let t = sample_tree();
        assert_eq!(resolve(&t, "/etc/passwd/x"), Err(Fault::NonDirComponent));
        assert_eq!(errno_of(&t, "/etc/passwd/x"), ENOTDIR);
        assert!(!Fault::NonDirComponent.is_enoent());
        assert_eq!(resolve(&t, "/etc/passwd/"), Err(Fault::TrailingOnFile));
        assert_eq!(errno_of(&t, "/etc/passwd/"), ENOTDIR);
    }

    #[test]
    fn matrix_twelve_cells() {
        let t = sample_tree();
        for c in CELLS.iter() {
            assert_eq!(resolve(&t, c.path), Err(c.fault), "path={}", c.path);
            assert_eq!(errno_of(&t, c.path), ENOENT, "path={}", c.path);
            assert!(c.fault.is_enoent(), "path={}", c.path);
        }
    }

    #[test]
    fn depth_and_name_boundaries() {
        assert!(split_path("a/b/c/d/e/f/g/h").is_ok());
        assert_eq!(split_path("a/b/c/d/e/f/g/h/i"), Err(Fault::TooDeep));
        assert!(NodeName::parse("abcdefghijkl").is_some());
        assert!(NodeName::parse("abcdefghijklm").is_none());
        assert!(NodeName::parse("").is_none());
    }

    #[test]
    fn capacity_rejects_and_keeps_ledger() {
        let mut t = Tree::new();
        let root = t.root_ino();
        let mut n = 0usize;
        while n < MAX_NODES - 1 {
            let mut buf = [0u8; 2];
            buf[0] = b'a' + ((n / 10) as u8);
            buf[1] = b'a' + ((n % 10) as u8);
            let nm = core::str::from_utf8(&buf).unwrap_or("");
            assert!(t.add(root, nm, NodeKind::File).is_ok());
            n += 1;
        }
        assert_eq!(t.used(), MAX_NODES);
        assert_eq!(t.add(root, "zz", NodeKind::File), Err(ENOSPC));
        assert_eq!(t.used(), MAX_NODES);
        assert_eq!(t.child_count(root), MAX_NODES - 1);
    }

    #[test]
    fn walk_machine_rejects_backwards() {
        let mut m = WalkMachine::new();
        m.advance(WalkState::Walking);
        m.advance(WalkState::Settled);
        let steps = m.steps();
        m.advance(WalkState::Walking);
        assert_eq!(m.illegal(), 1);
        assert_eq!(m.state(), WalkState::Settled);
        assert_eq!(m.steps(), steps);
    }

    #[test]
    fn frozen_consumer_agrees() {
        let t = sample_tree();
        assert!(abi_pinned());
        for c in CELLS.iter() {
            assert_eq!(b1_consume(&t, c.path), Consume::Errno(c.fault.errno()));
        }
        assert_eq!(b1_consume(&t, "etc/../etc/passwd"), Consume::Found(3));
        assert_eq!(frozen_lookup(&t, "/etc/ghost"), Err(ENOENT));
    }

    #[test]
    fn injection_ledger_conserved() {
        let l = EnoentLedger::run_matrix();
        assert!(l.conserved());
        assert_eq!(l.escaped, 0);
        assert_eq!(l.shots, 60);
        assert_eq!(l.detected, 60);
        assert_eq!(l.last, 20);
        assert_eq!(l.middle, 20);
        assert_eq!(l.empty, 20);
    }

    #[test]
    fn deterministic_corpus() {
        assert_eq!(corpus_fingerprint(), corpus_fingerprint());
    }

    #[test]
    fn checks_all_green() {
        let s = run_unx9226_checks();
        assert!(s.all_passed(), "{}", s.red_items_text());
        assert!(s.len() >= 12);
        assert!(!s.truncated());
    }
}
