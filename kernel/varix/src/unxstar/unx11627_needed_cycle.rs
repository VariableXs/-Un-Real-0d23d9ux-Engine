//! UNX-F11627 DT_NEEDED 循环依赖拒收判据
//!
//! 闸门次序：动态段自洽 → 边名非空 → 同名唯一 → 自环 SONAME 一致 → 预算 → 环截断复用 → 文案。
//! 契约：纯 no_std 风格（只用 `alloc::string::String` 渲染），无 unsafe、无 unwrap/expect/panic!，失败一律 `Result<_, u32>`，错误码恰好 13 个。
//!
//! 动态装载器展开 `DT_NEEDED` 时，图里出现环是**常态**而不是病态：`liba` 用
//! `libb` 的服务，`libb` 又回头引用 `liba` 的一个符号，链接器会各写一条
//! `DT_NEEDED`。glibc 在 `_dl_map_object_deps` 里用 `l_reserved`/`l_initfini`
//! 做"正在展开"标记，遇到指回展开栈的边就地截断——**环本身不拒收**，这正是
//! 本卡要钉死的实况：J1 判的是"环依赖装载成功档判据绿（不拒收实况）"。
//!
//! 拒收只发生在环**携带坏边**的时候，且次序固定：
//!
//! 1. 动态段自洽（`DT_STRTAB`/`DT_STRSZ` 可解析、`DT_NEEDED` 偏移在表内）；
//! 2. 边名非空（空 soname 是构造错误，不是环）；
//! 3. 自环的 `DT_SONAME` 与边名一致（否则同一文件被当成两个对象）；
//! 4. 边名能在闭包里命中唯一对象（`libgone.so` 是缺件，不是环）；
//! 5. 同名不同文件（两个 `/a/libdup.so`、`/b/libdup.so`）是歧义，不是环；
//! 6. 展开预算（节点数）不越界；
//! 7. 真环只记一次 `Cycle` 事件并复用已映射对象，装载继续。
//!
//! 因此"环依赖拒收判据"的严格读法是：**环不拒收，环里的坏边才拒收**。本模
//! 块 14 槽语料里 7 槽是各类环（二环、三环、自环、分支环、RUNPATH 环、预载
//! 截断环、重复边环）全部 `Ready`，6 槽坏边按上述六类各拒一次，另 1 槽是无
//! 环 DAG 的对照。环被接受时 `cycles>0` 而 `code==0`；坏边被拒时 `cycles==0`
//! 或保留已记账次数，但 `code` 必须落在六个域码之一。
//!
//! 本模块的判据面与文案面共用同一份 14 槽语料，避免"判一套、写一套"。

use crate::alloc::string::String;
use crate::checks::CheckSet;

/// 段基址。
pub const SEG_BASE: u32 = 0xfe00;

/// 语料槽数。
pub const CORPUS: usize = 14;

/// 账本格数。
pub const SAMPLES: usize = 14;

/// 单节点最多 `DT_NEEDED` 条数。
pub const NEED_MAX: usize = 3;

/// 图节点容量。
pub const NODE_CAP: usize = 8;

/// 装载事件容量。
pub const EVENT_CAP: usize = 32;

/// 装载序容量。
pub const ORDER_CAP: usize = 8;

/// 默认展开预算（节点数）。
pub const BUDGET_DEF: u8 = 8;

// ---------------------------------------------------------------------------
// 错误码
// ---------------------------------------------------------------------------

/// 环成员在搜索路径上找不到落点。
pub const NC_ENOENT: u32 = SEG_BASE + 0x01;
/// 环内对象的动态段/字符串表被截断。
pub const NC_ETRUNC: u32 = SEG_BASE + 0x02;
/// `DT_NEEDED` 偏移落在字符串表的空串上。
pub const NC_ENAME: u32 = SEG_BASE + 0x03;
/// 环展开的节点数超出预算。
pub const NC_EBUDGET: u32 = SEG_BASE + 0x04;
/// 同一 soname 命中两个不同文件。
pub const NC_EAMBIG: u32 = SEG_BASE + 0x05;
/// 自环边名与对象 `DT_SONAME` 不一致。
pub const NC_ESELF: u32 = SEG_BASE + 0x06;
/// 账本状态错（重复冻结、格序跳跃等）。
pub const NC_ESTATE: u32 = SEG_BASE + 0x07;
/// 版本/结构期望不符。
pub const NC_EVER: u32 = SEG_BASE + 0x08;
/// 对账不平。
pub const NC_EBAL: u32 = SEG_BASE + 0x09;
/// 槽位越界。
pub const NC_ESLOT: u32 = SEG_BASE + 0x0a;
/// 观测值与常量表不符。
pub const NC_EVALUE: u32 = SEG_BASE + 0x0b;
/// 冻结指纹失配。
pub const NC_ESEAL: u32 = SEG_BASE + 0x0c;
/// 没有可观测样本。
pub const NC_ENOBS: u32 = SEG_BASE + 0x0d;

// ---------------------------------------------------------------------------
// 操作码（账本格用）
// ---------------------------------------------------------------------------

/// 语料汇总。
pub const OP_CORPUS: u8 = 1;
/// 环识别。
pub const OP_CYCLE: u8 = 2;
/// 装载状态机。
pub const OP_LOAD: u8 = 3;
/// 预算。
pub const OP_BUDGET: u8 = 4;
/// 歧义。
pub const OP_AMBIG: u8 = 5;
/// 文案。
pub const OP_TEXT: u8 = 6;

// ---------------------------------------------------------------------------
// FNV-1a 指纹
// ---------------------------------------------------------------------------

const FNV_OFF: u64 = 0xcbf2_9ce4_8422_2325;
const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;

/// 折入一个字节。
fn fold_byte(h: u64, b: u8) -> u64 {
    (h ^ (b as u64)).wrapping_mul(FNV_PRIME)
}

/// 折入一个字（小端序逐字节）。
fn fold_word(h: u64, w: u64) -> u64 {
    let mut acc = h;
    let mut i = 0u32;
    while i < 8 {
        acc = fold_byte(acc, ((w >> (i * 8)) & 0xff) as u8);
        i += 1;
    }
    acc
}

/// 字节串指纹。
fn digest(bytes: &[u8]) -> u64 {
    let mut h = FNV_OFF;
    let mut i = 0usize;
    while i < bytes.len() {
        if let Some(b) = bytes.get(i) {
            h = fold_byte(h, *b);
        }
        i += 1;
    }
    h
}

// ---------------------------------------------------------------------------
// 文本工具
// ---------------------------------------------------------------------------

/// `u32` 的十进制文本。
pub fn u32_text(v: u32) -> String {
    let mut buf = [0u8; 10];
    let mut n = 0usize;
    let mut x = v;
    if x == 0 {
        if let Some(b) = buf.get_mut(0) {
            *b = b'0';
        }
        n = 1;
    }
    while x > 0 {
        if let Some(b) = buf.get_mut(n) {
            *b = b'0' + (x % 10) as u8;
        }
        n += 1;
        x /= 10;
    }
    let mut s = String::new();
    let mut i = n;
    while i > 0 {
        i -= 1;
        if let Some(b) = buf.get(i) {
            s.push(*b as char);
        }
    }
    s
}

/// `u32` 的定宽十六进制文本（`0x` + 8 位）。
pub fn hex_text(v: u32) -> String {
    const HEX: [u8; 16] = [
        b'0', b'1', b'2', b'3', b'4', b'5', b'6', b'7', b'8', b'9', b'a', b'b', b'c', b'd', b'e',
        b'f',
    ];
    let mut s = String::from("0x");
    let mut shift = 28i32;
    while shift >= 0 {
        let nib = ((v >> shift) & 0x0f) as usize;
        let b = HEX.get(nib).copied().unwrap_or(b'0');
        s.push(b as char);
        shift -= 4;
    }
    s
}

/// `subject: tail` 文案。
pub fn quote_err(subject: &str, tail: &str) -> String {
    let mut s = String::new();
    s.push_str(subject);
    s.push_str(": ");
    s.push_str(tail);
    s
}

/// 判断字符串是否为空。
fn is_blank(s: &str) -> bool {
    s.is_empty()
}

// ---------------------------------------------------------------------------
// 节点
// ---------------------------------------------------------------------------

/// 依赖图中的一个对象（= 一份被映射的文件）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Node {
    /// `DT_SONAME`。
    pub name: &'static str,
    /// 磁盘身份（空串表示未安装）。
    pub file: &'static str,
    /// `DT_NEEDED` 边名表。
    pub needed: [&'static str; NEED_MAX],
    /// 有效边数。
    pub needed_len: u8,
    /// 动态段可解析。
    pub dyn_ok: bool,
    /// `DT_NEEDED` 偏移都在 `DT_STRSZ` 内。
    pub strsz_ok: bool,
    /// `DT_SONAME` 与文件名一致。
    pub name_ok: bool,
    /// 由 `DT_RUNPATH` 命中。
    pub runpath: bool,
    /// 进程启动时已在全局作用域（`LD_PRELOAD` 或主程序已载）。
    pub preload: bool,
}

impl Node {
    /// 空节点。
    pub const fn blank(name: &'static str) -> Node {
        Node {
            name,
            file: "",
            needed: ["", "", ""],
            needed_len: 0,
            dyn_ok: true,
            strsz_ok: true,
            name_ok: true,
            runpath: false,
            preload: false,
        }
    }

    /// 带文件身份的节点。
    pub const fn at(name: &'static str, file: &'static str) -> Node {
        Node {
            name,
            file,
            needed: ["", "", ""],
            needed_len: 0,
            dyn_ok: true,
            strsz_ok: true,
            name_ok: true,
            runpath: false,
            preload: false,
        }
    }

    /// 追加一条 `DT_NEEDED`。
    pub fn need(&mut self, soname: &'static str) -> bool {
        if (self.needed_len as usize) >= NEED_MAX {
            return false;
        }
        if let Some(slot) = self.needed.get_mut(self.needed_len as usize) {
            *slot = soname;
        }
        self.needed_len += 1;
        true
    }

    /// 边数。
    pub const fn edges(&self) -> usize {
        self.needed_len as usize
    }

    /// 第 `i` 条边。
    pub fn edge_at(&self, i: usize) -> Option<&'static str> {
        if i >= self.needed_len as usize {
            return None;
        }
        self.needed.get(i).copied()
    }

    /// 是否已安装到搜索路径。
    pub fn installed(&self) -> bool {
        !is_blank(self.file)
    }

    /// 是否含自环边。
    pub fn self_edge(&self) -> bool {
        let mut i = 0usize;
        while i < self.edges() {
            if self.edge_at(i) == Some(self.name) {
                return true;
            }
            i += 1;
        }
        false
    }

    /// 动态段是否整体可信。
    pub fn sane(&self) -> bool {
        self.dyn_ok && self.strsz_ok
    }

    /// 节点是否走 RUNPATH 搜索。
    pub const fn via_runpath(&self) -> bool {
        self.runpath
    }

    /// 节点是否已预载。
    pub const fn is_preload(&self) -> bool {
        self.preload
    }

    /// 标记 RUNPATH 命中。
    pub fn mark_runpath(&mut self) {
        self.runpath = true;
    }

    /// 标记预载。
    pub fn mark_preload(&mut self) {
        self.preload = true;
    }
}

// ---------------------------------------------------------------------------
// 图
// ---------------------------------------------------------------------------

/// `DT_NEEDED` 有向图。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Graph {
    nodes: [Node; NODE_CAP],
    len: u8,
}

impl Graph {
    /// 空图。
    pub const fn new() -> Graph {
        Graph { nodes: [Node::blank(""); NODE_CAP], len: 0 }
    }

    /// 加节点。
    pub fn push(&mut self, n: Node) -> bool {
        if (self.len as usize) >= NODE_CAP {
            return false;
        }
        if let Some(slot) = self.nodes.get_mut(self.len as usize) {
            *slot = n;
        }
        self.len += 1;
        true
    }

    /// 节点数。
    pub const fn len(&self) -> usize {
        self.len as usize
    }

    /// 是否空图。
    pub const fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// 取节点。
    pub fn node(&self, i: usize) -> Option<&Node> {
        if i >= self.len as usize {
            return None;
        }
        self.nodes.get(i)
    }

    /// 按 soname 找节点（首个）。
    pub fn find(&self, name: &str) -> Option<usize> {
        let mut i = 0usize;
        while i < self.len as usize {
            if let Some(n) = self.nodes.get(i) {
                if n.name == name {
                    return Some(i);
                }
            }
            i += 1;
        }
        None
    }

    /// 同名节点个数。
    pub fn name_count(&self, name: &str) -> usize {
        let mut n = 0usize;
        let mut i = 0usize;
        while i < self.len as usize {
            if let Some(node) = self.nodes.get(i) {
                if node.name == name {
                    n += 1;
                }
            }
            i += 1;
        }
        n
    }

    /// 同名节点是否落在不同文件。
    pub fn ambiguous(&self, name: &str) -> bool {
        let mut file = "";
        let mut seen = 0usize;
        let mut i = 0usize;
        while i < self.len as usize {
            if let Some(n) = self.nodes.get(i) {
                if n.name == name {
                    if seen == 0 {
                        file = n.file;
                    } else if n.file != file {
                        return true;
                    }
                    seen += 1;
                }
            }
            i += 1;
        }
        false
    }

    /// 边是否命中。
    pub fn has_edge(&self, from: &str, to: &str) -> bool {
        match self.find(from) {
            Some(i) => match self.node(i) {
                Some(n) => n.edge_at(0) == Some(to) || n.edge_at(1) == Some(to) || n.edge_at(2) == Some(to),
                None => false,
            },
            None => false,
        }
    }

    /// 从任一节点出发做三色 DFS，报告是否成环。
    pub fn has_cycle(&self) -> bool {
        let mut color = [0u8; NODE_CAP];
        let mut i = 0usize;
        while i < self.len as usize {
            if color.get(i).copied().unwrap_or(0) == 0 && self.dfs_cycle(i, &mut color) {
                return true;
            }
            i += 1;
        }
        false
    }

    /// 递归 DFS（深度 ≤ NODE_CAP）。
    fn dfs_cycle(&self, i: usize, color: &mut [u8; NODE_CAP]) -> bool {
        if let Some(c) = color.get_mut(i) {
            *c = 1;
        }
        let n = match self.nodes.get(i) {
            Some(v) => *v,
            None => return false,
        };
        let mut k = 0usize;
        while k < n.edges() {
            let name = match n.edge_at(k) {
                Some(v) => v,
                None => "",
            };
            if let Some(j) = self.find(name) {
                match color.get(j).copied().unwrap_or(0) {
                    1 => return true,
                    0 => {
                        if self.dfs_cycle(j, color) {
                            return true;
                        }
                    }
                    _ => {}
                }
            }
            k += 1;
        }
        if let Some(c) = color.get_mut(i) {
            *c = 2;
        }
        false
    }

    /// 所有 `DT_RUNPATH` 命中的节点确实标了 runpath。
    pub fn runpath_ok(&self) -> bool {
        let mut i = 0usize;
        while i < self.len as usize {
            if let Some(n) = self.nodes.get(i) {
                if n.file.starts_with("/opt/") && !n.runpath {
                    return false;
                }
            }
            i += 1;
        }
        true
    }

    /// 图内边总数。
    pub fn edge_total(&self) -> usize {
        let mut n = 0usize;
        let mut i = 0usize;
        while i < self.len as usize {
            if let Some(node) = self.nodes.get(i) {
                n += node.edges();
            }
            i += 1;
        }
        n
    }

    /// 预载节点数。
    pub fn preload_count(&self) -> usize {
        let mut n = 0usize;
        let mut i = 0usize;
        while i < self.len as usize {
            if let Some(node) = self.nodes.get(i) {
                if node.preload {
                    n += 1;
                }
            }
            i += 1;
        }
        n
    }
}

// ---------------------------------------------------------------------------
// 装载事件
// ---------------------------------------------------------------------------

/// 事件种类。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EventKind {
    /// 首次进入节点。
    Enter,
    /// 命中已完成的节点，复用映射。
    Reuse,
    /// 命中展开栈上的节点，环截断。
    Cycle,
    /// 缺件。
    Missing,
    /// 动态段截断。
    Truncated,
    /// 空边名。
    Empty,
    /// 同名歧义。
    Ambiguous,
    /// 自环 SONAME 不一致。
    BadSelf,
    /// 预算越界。
    Budget,
}

impl EventKind {
    /// 文本名。
    pub const fn label(self) -> &'static str {
        match self {
            EventKind::Enter => "enter",
            EventKind::Reuse => "reuse",
            EventKind::Cycle => "cycle",
            EventKind::Missing => "missing",
            EventKind::Truncated => "truncated",
            EventKind::Empty => "empty",
            EventKind::Ambiguous => "ambiguous",
            EventKind::BadSelf => "bad-self",
            EventKind::Budget => "budget",
        }
    }

    /// 是否拒收事件。
    pub const fn rejecting(self) -> bool {
        match self {
            EventKind::Enter | EventKind::Reuse | EventKind::Cycle => false,
            _ => true,
        }
    }
}

/// 单个装载事件。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Event {
    /// 节点序号。
    pub node: u8,
    /// 深度。
    pub depth: u8,
    /// 种类。
    pub kind: EventKind,
}

impl Event {
    /// 空事件。
    pub const fn blank() -> Event {
        Event { node: 0, depth: 0, kind: EventKind::Enter }
    }
}

// ---------------------------------------------------------------------------
// 装载结果
// ---------------------------------------------------------------------------

/// 装载判面。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Outcome {
    /// 装载成功（可能是环）。
    Ready,
    /// 缺件。
    Missing,
    /// 动态段截断。
    Truncated,
    /// 空边名。
    Empty,
    /// 预算越界。
    Budget,
    /// 同名歧义。
    Ambiguous,
    /// 自环不一致。
    BadSelf,
}

impl Outcome {
    /// 是否成功。
    pub const fn ready(self) -> bool {
        match self {
            Outcome::Ready => true,
            _ => false,
        }
    }

    /// 错误码（成功为 0）。
    pub const fn code(self) -> u32 {
        match self {
            Outcome::Ready => 0,
            Outcome::Missing => NC_ENOENT,
            Outcome::Truncated => NC_ETRUNC,
            Outcome::Empty => NC_ENAME,
            Outcome::Budget => NC_EBUDGET,
            Outcome::Ambiguous => NC_EAMBIG,
            Outcome::BadSelf => NC_ESELF,
        }
    }

    /// 文本名。
    pub const fn label(self) -> &'static str {
        match self {
            Outcome::Ready => "ready",
            Outcome::Missing => "missing",
            Outcome::Truncated => "truncated",
            Outcome::Empty => "empty-name",
            Outcome::Budget => "budget",
            Outcome::Ambiguous => "ambiguous",
            Outcome::BadSelf => "bad-self",
        }
    }
}

/// 一次装载的完整观测。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Run {
    outcome: Outcome,
    subject: &'static str,
    requester: &'static str,
    order: [u8; ORDER_CAP],
    order_len: u8,
    events: [Event; EVENT_CAP],
    event_len: u8,
    cycles: u8,
    budget_used: u8,
    budget: u8,
}

impl Run {
    /// 空结果。
    pub const fn new(budget: u8) -> Run {
        Run {
            outcome: Outcome::Ready,
            subject: "",
            requester: "",
            order: [0u8; ORDER_CAP],
            order_len: 0,
            events: [Event::blank(); EVENT_CAP],
            event_len: 0,
            cycles: 0,
            budget_used: 0,
            budget,
        }
    }

    /// 判面。
    pub const fn outcome(&self) -> Outcome {
        self.outcome
    }

    /// 错误码。
    pub const fn code(&self) -> u32 {
        self.outcome.code()
    }

    /// 主体（缺件名/坏节点名/起点名）。
    pub const fn subject(&self) -> &'static str {
        self.subject
    }

    /// 请求者。
    pub const fn requester(&self) -> &'static str {
        self.requester
    }

    /// 是否成功。
    pub const fn ready(&self) -> bool {
        self.outcome.ready()
    }

    /// 截断的环次数。
    pub const fn cycles(&self) -> u8 {
        self.cycles
    }

    /// 已用预算。
    pub const fn budget_used(&self) -> u8 {
        self.budget_used
    }

    /// 预算上限。
    pub const fn budget(&self) -> u8 {
        self.budget
    }

    /// 装载序长度。
    pub const fn order_len(&self) -> usize {
        self.order_len as usize
    }

    /// 装载序第 `i` 项。
    pub fn order_at(&self, i: usize) -> Option<usize> {
        if i >= self.order_len as usize {
            return None;
        }
        self.order.get(i).map(|v| *v as usize)
    }

    /// 事件数。
    pub const fn event_len(&self) -> usize {
        self.event_len as usize
    }

    /// 事件第 `i` 项。
    pub fn event_at(&self, i: usize) -> Option<Event> {
        if i >= self.event_len as usize {
            return None;
        }
        self.events.get(i).copied()
    }

    /// 事件计数。
    pub fn event_count(&self, kind: EventKind) -> usize {
        let mut n = 0usize;
        let mut i = 0usize;
        while i < self.event_len as usize {
            if let Some(e) = self.events.get(i) {
                if e.kind == kind {
                    n += 1;
                }
            }
            i += 1;
        }
        n
    }

    /// 是否在环上成功（装载成功且至少一次环截断）。
    pub fn accepted_cycle(&self) -> bool {
        self.ready() && self.cycles > 0
    }

    /// 记录事件。
    fn note(&mut self, node: u8, depth: u8, kind: EventKind) {
        if (self.event_len as usize) < EVENT_CAP {
            if let Some(slot) = self.events.get_mut(self.event_len as usize) {
                *slot = Event { node, depth, kind };
            }
            self.event_len += 1;
        }
    }

    /// 记录装载序。
    fn push_order(&mut self, node: u8) {
        if (self.order_len as usize) < ORDER_CAP {
            if let Some(slot) = self.order.get_mut(self.order_len as usize) {
                *slot = node;
            }
            self.order_len += 1;
        }
    }

    /// 失败收尾。
    fn fail(&mut self, outcome: Outcome, subject: &'static str, requester: &'static str) {
        self.outcome = outcome;
        self.subject = subject;
        self.requester = requester;
    }

    /// 逐字文案。
    pub fn text(&self) -> String {
        match self.outcome {
            Outcome::Ready => {
                let mut s = String::from("cycle-ok ");
                s.push_str(self.subject);
                s
            }
            Outcome::Missing => quote_err(
                self.subject,
                "cannot open shared object file: No such file or directory",
            ),
            Outcome::Truncated => quote_err(
                self.subject,
                "invalid dynamic section: DT_NEEDED offset beyond DT_STRSZ",
            ),
            Outcome::Empty => quote_err(self.subject, "empty DT_NEEDED string offset"),
            Outcome::Budget => quote_err(self.subject, "dependency expansion exceeds budget"),
            Outcome::Ambiguous => {
                quote_err(self.subject, "same soname resolved to different objects")
            }
            Outcome::BadSelf => quote_err(self.subject, "DT_SONAME mismatch on self reference"),
        }
    }

    /// 诊断多行文本。
    pub fn diagnose(&self) -> String {
        let mut s = String::new();
        s.push_str("run outcome=");
        s.push_str(self.outcome.label());
        s.push_str(" subject=");
        s.push_str(self.subject);
        s.push_str(" requester=");
        s.push_str(self.requester);
        s.push_str(" cycles=");
        s.push_str(u32_text(self.cycles as u32).as_str());
        s.push_str(" budget=");
        s.push_str(u32_text(self.budget_used as u32).as_str());
        s.push('/');
        s.push_str(u32_text(self.budget as u32).as_str());
        s.push('\n');
        let mut i = 0usize;
        while i < self.event_len as usize {
            if let Some(e) = self.events.get(i) {
                s.push_str("ev ");
                s.push_str(u32_text(i as u32).as_str());
                s.push(' ');
                s.push_str(e.kind.label());
                s.push_str(" #");
                s.push_str(u32_text(e.node as u32).as_str());
                s.push_str(" d=");
                s.push_str(u32_text(e.depth as u32).as_str());
                s.push('\n');
            }
            i += 1;
        }
        s
    }
}

// ---------------------------------------------------------------------------
// 装载状态机
// ---------------------------------------------------------------------------

/// 在图上展开一次装载。
pub fn load(g: &Graph, start: &'static str, budget: u8) -> Run {
    let mut r = Run::new(budget);
    let start_idx = match g.find(start) {
        Some(i) => i,
        None => {
            r.note(0, 0, EventKind::Missing);
            r.fail(Outcome::Missing, start, start);
            return r;
        }
    };

    // 预载节点先入全局作用域：它们不占展开预算，也不再下探。
    let mut color = [0u8; NODE_CAP];
    let mut i = 0usize;
    while i < g.len() {
        if let Some(n) = g.node(i) {
            if n.preload {
                color[i] = 2;
                r.push_order(i as u8);
                r.note(i as u8, 0, EventKind::Reuse);
            }
        }
        i += 1;
    }

    if color.get(start_idx).copied().unwrap_or(0) == 0 {
        let n = match g.node(start_idx) {
            Some(v) => *v,
            None => {
                r.fail(Outcome::Missing, start, start);
                return r;
            }
        };
        if !n.sane() {
            r.note(start_idx as u8, 0, EventKind::Truncated);
            r.fail(Outcome::Truncated, n.name, n.name);
            return r;
        }
        r.budget_used = 1;
        if r.budget_used > budget {
            r.note(start_idx as u8, 0, EventKind::Budget);
            r.fail(Outcome::Budget, n.name, n.name);
            return r;
        }
        color[start_idx] = 1;
        r.push_order(start_idx as u8);
        r.note(start_idx as u8, 0, EventKind::Enter);
    }

    let mut stack = [0u8; NODE_CAP];
    let mut cursor = [0u8; NODE_CAP];
    let mut depth = 0usize;
    if color.get(start_idx).copied().unwrap_or(2) == 1 {
        stack[0] = start_idx as u8;
        depth = 1;
    }

    while depth > 0 {
        let d = depth - 1;
        let ni = stack.get(d).copied().unwrap_or(0) as usize;
        let n = match g.node(ni) {
            Some(v) => *v,
            None => {
                r.fail(Outcome::Missing, start, start);
                return r;
            }
        };
        let pos = cursor.get(d).copied().unwrap_or(0) as usize;
        if pos >= n.edges() {
            // 节点展开完，标记完成。
            if let Some(c) = color.get_mut(ni) {
                *c = 2;
            }
            depth -= 1;
            continue;
        }
        if let Some(c) = cursor.get_mut(d) {
            *c = (pos + 1) as u8;
        }
        let edge = match n.edge_at(pos) {
            Some(v) => v,
            None => "",
        };
        let here = n.name;
        if is_blank(edge) {
            r.note(ni as u8, d as u8, EventKind::Empty);
            r.fail(Outcome::Empty, here, here);
            return r;
        }
        if edge == here && !n.name_ok {
            r.note(ni as u8, d as u8, EventKind::BadSelf);
            r.fail(Outcome::BadSelf, here, here);
            return r;
        }
        let j = match g.find(edge) {
            Some(v) => v,
            None => {
                r.note(ni as u8, d as u8, EventKind::Missing);
                r.fail(Outcome::Missing, edge, here);
                return r;
            }
        };
        if g.ambiguous(edge) {
            r.note(ni as u8, d as u8, EventKind::Ambiguous);
            r.fail(Outcome::Ambiguous, edge, here);
            return r;
        }
        let child = match g.node(j) {
            Some(v) => *v,
            None => {
                r.note(ni as u8, d as u8, EventKind::Missing);
                r.fail(Outcome::Missing, edge, here);
                return r;
            }
        };
        match color.get(j).copied().unwrap_or(0) {
            1 => {
                // 环：就地截断，已映射对象复用，装载继续。
                r.cycles = r.cycles.saturating_add(1);
                r.note(j as u8, d as u8, EventKind::Cycle);
            }
            2 => {
                r.note(j as u8, d as u8, EventKind::Reuse);
            }
            _ => {
                if !child.sane() {
                    r.note(j as u8, d as u8, EventKind::Truncated);
                    r.fail(Outcome::Truncated, child.name, here);
                    return r;
                }
                r.budget_used = r.budget_used.saturating_add(1);
                if r.budget_used > budget {
                    r.note(j as u8, d as u8, EventKind::Budget);
                    r.fail(Outcome::Budget, child.name, here);
                    return r;
                }
                if depth >= NODE_CAP {
                    r.note(j as u8, d as u8, EventKind::Budget);
                    r.fail(Outcome::Budget, child.name, here);
                    return r;
                }
                if let Some(c) = color.get_mut(j) {
                    *c = 1;
                }
                if let Some(slot) = stack.get_mut(depth) {
                    *slot = j as u8;
                }
                if let Some(slot) = cursor.get_mut(depth) {
                    *slot = 0;
                }
                depth += 1;
                r.push_order(j as u8);
                r.note(j as u8, d as u8, EventKind::Enter);
            }
        }
    }

    r.subject = start;
    r.outcome = Outcome::Ready;
    r
}

// ---------------------------------------------------------------------------
// 语料
// ---------------------------------------------------------------------------

/// 场景标签。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Scenario {
    /// 无环 DAG 对照。
    Dag,
    /// 二节点环。
    Cycle2,
    /// 三节点环。
    Cycle3,
    /// 自环。
    SelfLoop,
    /// 带分支的环。
    BranchCycle,
    /// RUNPATH 命中的环。
    RunpathCycle,
    /// 预载打断的环。
    PreloadCycle,
    /// 重复边环。
    DupEdges,
    /// 环成员缺件。
    MissingEdge,
    /// 环内动态段截断。
    TruncatedNode,
    /// 环内空边名。
    EmptyEdge,
    /// 长环超预算。
    BudgetCycle,
    /// 同名不同文件。
    AmbiguousName,
    /// 自环 SONAME 不一致。
    SelfMismatch,
}

impl Scenario {
    /// 标签名。
    pub const fn label(self) -> &'static str {
        match self {
            Scenario::Dag => "dag",
            Scenario::Cycle2 => "cycle2",
            Scenario::Cycle3 => "cycle3",
            Scenario::SelfLoop => "self-loop",
            Scenario::BranchCycle => "branch-cycle",
            Scenario::RunpathCycle => "runpath-cycle",
            Scenario::PreloadCycle => "preload-cycle",
            Scenario::DupEdges => "dup-edges",
            Scenario::MissingEdge => "missing-edge",
            Scenario::TruncatedNode => "truncated-node",
            Scenario::EmptyEdge => "empty-edge",
            Scenario::BudgetCycle => "budget-cycle",
            Scenario::AmbiguousName => "ambiguous-name",
            Scenario::SelfMismatch => "self-mismatch",
        }
    }

    /// 是否环族场景。
    pub const fn cyclic(self) -> bool {
        match self {
            Scenario::Cycle2
            | Scenario::Cycle3
            | Scenario::SelfLoop
            | Scenario::BranchCycle
            | Scenario::RunpathCycle
            | Scenario::PreloadCycle
            | Scenario::DupEdges
            | Scenario::BudgetCycle => true,
            _ => false,
        }
    }
}

/// 一条语料。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Case {
    /// 语料名。
    pub name: &'static str,
    /// 期望判面。
    pub expect: Outcome,
    /// 场景。
    pub scenario: Scenario,
    /// 预算。
    pub budget: u8,
    /// 期望环截断次数（0 表示不要求）。
    pub cycles: u8,
    /// 说明（唯一性键之一）。
    pub note: &'static str,
}

impl Case {
    /// 期望错误码。
    pub const fn code(&self) -> u32 {
        self.expect.code()
    }

    /// 输入特征键。
    pub fn input_key(&self) -> u64 {
        let mut h = digest(self.name.as_bytes());
        h = fold_word(h, self.scenario as u64);
        h = fold_word(h, self.budget as u64);
        h
    }

    /// 说明指纹。
    pub fn note_key(&self) -> u64 {
        digest(self.note.as_bytes())
    }
}

/// 14 槽语料：8 活 6 死。
pub const CASES: [Case; CORPUS] = [
    Case {
        name: "dag-app",
        expect: Outcome::Ready,
        scenario: Scenario::Dag,
        budget: BUDGET_DEF,
        cycles: 0,
        note: "app -> liba -> libb -> libc 无环对照",
    },
    Case {
        name: "ring-two",
        expect: Outcome::Ready,
        scenario: Scenario::Cycle2,
        budget: BUDGET_DEF,
        cycles: 1,
        note: "app -> liba -> libb -> liba 二环装载成功",
    },
    Case {
        name: "ring-three",
        expect: Outcome::Ready,
        scenario: Scenario::Cycle3,
        budget: BUDGET_DEF,
        cycles: 1,
        note: "app -> liba -> libb -> libc -> liba 三环装载成功",
    },
    Case {
        name: "ring-self",
        expect: Outcome::Ready,
        scenario: Scenario::SelfLoop,
        budget: BUDGET_DEF,
        cycles: 1,
        note: "liba 依赖自身，SONAME 一致时按环截断",
    },
    Case {
        name: "ring-branch",
        expect: Outcome::Ready,
        scenario: Scenario::BranchCycle,
        budget: BUDGET_DEF,
        cycles: 1,
        note: "环上再挂 libc 分支，拓扑序仍可完成",
    },
    Case {
        name: "ring-runpath",
        expect: Outcome::Ready,
        scenario: Scenario::RunpathCycle,
        budget: BUDGET_DEF,
        cycles: 1,
        note: "/opt 下 libx 与 liby 互指，RUNPATH 命中后成环",
    },
    Case {
        name: "ring-preload",
        expect: Outcome::Ready,
        scenario: Scenario::PreloadCycle,
        budget: BUDGET_DEF,
        cycles: 0,
        note: "libb 已预载，回指边走 Reuse 不重复展开",
    },
    Case {
        name: "ring-dup",
        expect: Outcome::Ready,
        scenario: Scenario::DupEdges,
        budget: BUDGET_DEF,
        cycles: 1,
        note: "app 连写两条 liba NEEDED，第二条走 Reuse 去重",
    },
    Case {
        name: "gone-in-ring",
        expect: Outcome::Missing,
        scenario: Scenario::MissingEdge,
        budget: BUDGET_DEF,
        cycles: 0,
        note: "libb 回指 liba 前先缺 libgone.so，缺件优先于环",
    },
    Case {
        name: "trunc-in-ring",
        expect: Outcome::Truncated,
        scenario: Scenario::TruncatedNode,
        budget: BUDGET_DEF,
        cycles: 0,
        note: "环内 libb 动态段截断，拒收不是环造成",
    },
    Case {
        name: "empty-in-ring",
        expect: Outcome::Empty,
        scenario: Scenario::EmptyEdge,
        budget: BUDGET_DEF,
        cycles: 0,
        note: "环内 libb 的 NEEDED 偏移落空串",
    },
    Case {
        name: "long-ring",
        expect: Outcome::Budget,
        scenario: Scenario::BudgetCycle,
        budget: 6,
        cycles: 0,
        note: "八节点长环预算六，第七个节点越界拒收",
    },
    Case {
        name: "dup-name",
        expect: Outcome::Ambiguous,
        scenario: Scenario::AmbiguousName,
        budget: BUDGET_DEF,
        cycles: 0,
        note: "两份 libdup.so 分居 /a /b，同名不同文件",
    },
    Case {
        name: "self-alias",
        expect: Outcome::BadSelf,
        scenario: Scenario::SelfMismatch,
        budget: BUDGET_DEF,
        cycles: 0,
        note: "libself.so 自指但 DT_SONAME 写成 libself.so.1",
    },
];

/// 取槽。
pub fn slot_at(i: usize) -> Option<Case> {
    if i >= CORPUS {
        return None;
    }
    CASES.get(i).copied()
}

/// 按场景构图。
pub fn graph_for(sc: Scenario) -> Graph {
    let mut g = Graph::new();
    match sc {
        Scenario::Dag => {
            let mut app = Node::at("./app", "/bin/app");
            app.need("liba.so");
            let mut a = Node::at("liba.so", "/lib/liba.so");
            a.need("libb.so");
            let mut b = Node::at("libb.so", "/lib/libb.so");
            b.need("libc.so");
            let c = Node::at("libc.so", "/lib/libc.so");
            g.push(app);
            g.push(a);
            g.push(b);
            g.push(c);
        }
        Scenario::Cycle2 => {
            let mut app = Node::at("./app", "/bin/app");
            app.need("liba.so");
            let mut a = Node::at("liba.so", "/lib/liba.so");
            a.need("libb.so");
            let mut b = Node::at("libb.so", "/lib/libb.so");
            b.need("liba.so");
            g.push(app);
            g.push(a);
            g.push(b);
        }
        Scenario::Cycle3 => {
            let mut app = Node::at("./app", "/bin/app");
            app.need("liba.so");
            let mut a = Node::at("liba.so", "/lib/liba.so");
            a.need("libb.so");
            let mut b = Node::at("libb.so", "/lib/libb.so");
            b.need("libc.so");
            let mut c = Node::at("libc.so", "/lib/libc.so");
            c.need("liba.so");
            g.push(app);
            g.push(a);
            g.push(b);
            g.push(c);
        }
        Scenario::SelfLoop => {
            let mut app = Node::at("./app", "/bin/app");
            app.need("liba.so");
            let mut a = Node::at("liba.so", "/lib/liba.so");
            a.need("liba.so");
            g.push(app);
            g.push(a);
        }
        Scenario::BranchCycle => {
            let mut app = Node::at("./app", "/bin/app");
            app.need("liba.so");
            let mut a = Node::at("liba.so", "/lib/liba.so");
            a.need("libb.so");
            a.need("libc.so");
            let mut b = Node::at("libb.so", "/lib/libb.so");
            b.need("liba.so");
            let c = Node::at("libc.so", "/lib/libc.so");
            g.push(app);
            g.push(a);
            g.push(b);
            g.push(c);
        }
        Scenario::RunpathCycle => {
            let mut app = Node::at("./app", "/bin/app");
            app.need("libx.so");
            let mut x = Node::at("libx.so", "/opt/x/libx.so");
            x.mark_runpath();
            x.need("liby.so");
            let mut y = Node::at("liby.so", "/opt/x/liby.so");
            y.mark_runpath();
            y.need("libx.so");
            g.push(app);
            g.push(x);
            g.push(y);
        }
        Scenario::PreloadCycle => {
            let mut app = Node::at("./app", "/bin/app");
            app.need("liba.so");
            let mut a = Node::at("liba.so", "/lib/liba.so");
            a.need("libb.so");
            let mut b = Node::at("libb.so", "/lib/libb.so");
            b.need("liba.so");
            b.mark_preload();
            g.push(app);
            g.push(a);
            g.push(b);
        }
        Scenario::DupEdges => {
            let mut app = Node::at("./app", "/bin/app");
            app.need("liba.so");
            app.need("liba.so");
            let mut a = Node::at("liba.so", "/lib/liba.so");
            a.need("libb.so");
            let mut b = Node::at("libb.so", "/lib/libb.so");
            b.need("liba.so");
            g.push(app);
            g.push(a);
            g.push(b);
        }
        Scenario::MissingEdge => {
            let mut app = Node::at("./app", "/bin/app");
            app.need("liba.so");
            let mut a = Node::at("liba.so", "/lib/liba.so");
            a.need("libb.so");
            let mut b = Node::at("libb.so", "/lib/libb.so");
            b.need("libgone.so");
            b.need("liba.so");
            g.push(app);
            g.push(a);
            g.push(b);
        }
        Scenario::TruncatedNode => {
            let mut app = Node::at("./app", "/bin/app");
            app.need("liba.so");
            let mut a = Node::at("liba.so", "/lib/liba.so");
            a.need("libb.so");
            let mut b = Node::at("libb.so", "/lib/libb.so");
            b.dyn_ok = false;
            b.need("liba.so");
            g.push(app);
            g.push(a);
            g.push(b);
        }
        Scenario::EmptyEdge => {
            let mut app = Node::at("./app", "/bin/app");
            app.need("liba.so");
            let mut a = Node::at("liba.so", "/lib/liba.so");
            a.need("libb.so");
            let mut b = Node::at("libb.so", "/lib/libb.so");
            b.need("");
            b.need("liba.so");
            g.push(app);
            g.push(a);
            g.push(b);
        }
        Scenario::BudgetCycle => {
            let mut app = Node::at("./app", "/bin/app");
            app.need("libn1.so");
            let mut n1 = Node::at("libn1.so", "/lib/libn1.so");
            n1.need("libn2.so");
            let mut n2 = Node::at("libn2.so", "/lib/libn2.so");
            n2.need("libn3.so");
            let mut n3 = Node::at("libn3.so", "/lib/libn3.so");
            n3.need("libn4.so");
            let mut n4 = Node::at("libn4.so", "/lib/libn4.so");
            n4.need("libn5.so");
            let mut n5 = Node::at("libn5.so", "/lib/libn5.so");
            n5.need("libn6.so");
            let mut n6 = Node::at("libn6.so", "/lib/libn6.so");
            n6.need("libn7.so");
            let mut n7 = Node::at("libn7.so", "/lib/libn7.so");
            n7.need("libn1.so");
            g.push(app);
            g.push(n1);
            g.push(n2);
            g.push(n3);
            g.push(n4);
            g.push(n5);
            g.push(n6);
            g.push(n7);
        }
        Scenario::AmbiguousName => {
            let mut app = Node::at("./app", "/bin/app");
            app.need("libdup.so");
            let a = Node::at("libdup.so", "/a/libdup.so");
            let b = Node::at("libdup.so", "/b/libdup.so");
            g.push(app);
            g.push(a);
            g.push(b);
        }
        Scenario::SelfMismatch => {
            let mut app = Node::at("./app", "/bin/app");
            app.need("libself.so");
            let mut s = Node::at("libself.so", "/lib/libself.so");
            s.name_ok = false;
            s.need("libself.so");
            g.push(app);
            g.push(s);
        }
    }
    g
}

/// 求解一条语料。
pub fn solve(c: &Case) -> Run {
    let g = graph_for(c.scenario);
    let start = match g.node(0) {
        Some(n) => n.name,
        None => "",
    };
    load(&g, start, c.budget)
}

/// 语料唯一性：输入键、说明键、槽名三者两两互异。
pub fn unique() -> bool {
    let mut i = 0usize;
    while i < CASES.len() {
        let a = match CASES.get(i) {
            Some(v) => v,
            None => return false,
        };
        let mut j = i + 1;
        while j < CASES.len() {
            let b = match CASES.get(j) {
                Some(v) => v,
                None => return false,
            };
            if a.name == b.name {
                return false;
            }
            if a.input_key() == b.input_key() {
                return false;
            }
            if a.note_key() == b.note_key() || a.note == b.note {
                return false;
            }
            if a.scenario == b.scenario {
                return false;
            }
            j += 1;
        }
        i += 1;
    }
    true
}

/// 说明非空且场景标签自洽。
pub fn notes_ok() -> bool {
    let mut i = 0usize;
    while i < CASES.len() {
        let c = match CASES.get(i) {
            Some(v) => v,
            None => return false,
        };
        if c.note.is_empty() || c.name.is_empty() {
            return false;
        }
        if c.scenario.label().is_empty() {
            return false;
        }
        let expect_cyclic = c.expect.ready() && c.cycles > 0;
        if expect_cyclic && !c.scenario.cyclic() {
            return false;
        }
        i += 1;
    }
    true
}

/// 表对：逐槽求解并比对判面、码、环次数。
pub fn table_ok() -> bool {
    let mut i = 0usize;
    while i < CASES.len() {
        let c = match CASES.get(i) {
            Some(v) => v,
            None => return false,
        };
        let r = solve(&c);
        if r.outcome() != c.expect {
            return false;
        }
        if r.code() != c.code() {
            return false;
        }
        if c.cycles > 0 && r.cycles() != c.cycles {
            return false;
        }
        if c.expect.ready() && !r.ready() {
            return false;
        }
        i += 1;
    }
    true
}

// ---------------------------------------------------------------------------
// 汇总
// ---------------------------------------------------------------------------

/// 语料汇总。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Report {
    total: usize,
    accept: usize,
    reject: usize,
    unresolved: usize,
    malformed: usize,
    budget: usize,
    selfref: usize,
    matched: usize,
    red: bool,
}

impl Report {
    /// 空汇总。
    pub const fn new() -> Report {
        Report {
            total: 0,
            accept: 0,
            reject: 0,
            unresolved: 0,
            malformed: 0,
            budget: 0,
            selfref: 0,
            matched: 0,
            red: false,
        }
    }

    /// 总条数。
    pub const fn total(&self) -> usize {
        self.total
    }

    /// 接受条数。
    pub const fn accept(&self) -> usize {
        self.accept
    }

    /// 拒收条数。
    pub const fn reject(&self) -> usize {
        self.reject
    }

    /// 缺件/歧义桶。
    pub const fn unresolved(&self) -> usize {
        self.unresolved
    }

    /// 截断/空名桶。
    pub const fn malformed(&self) -> usize {
        self.malformed
    }

    /// 预算桶。
    pub const fn budget(&self) -> usize {
        self.budget
    }

    /// 自环桶。
    pub const fn selfref(&self) -> usize {
        self.selfref
    }

    /// 对上条数。
    pub const fn matched(&self) -> usize {
        self.matched
    }

    /// 是否红档。
    pub const fn red(&self) -> bool {
        self.red
    }

    /// 是否空。
    pub const fn is_empty(&self) -> bool {
        self.total == 0
    }

    /// 千分率：四舍五入。
    pub fn rate(&self) -> u32 {
        if self.total == 0 {
            return 0;
        }
        let matched = self.matched as u32;
        let total = self.total as u32;
        (matched * 1000 + total / 2) / total
    }

    /// 闸门。
    pub fn gate(&self) -> Result<(), u32> {
        if self.total == 0 {
            return Err(NC_ENOBS);
        }
        if self.total != CORPUS {
            return Err(NC_EVALUE);
        }
        if self.accept != 8 || self.reject != 6 {
            return Err(NC_EBAL);
        }
        if self.unresolved + self.malformed + self.budget + self.selfref != self.reject {
            return Err(NC_EBAL);
        }
        if self.red {
            return Err(NC_ESTATE);
        }
        if self.matched != self.total {
            return Err(NC_EVER);
        }
        Ok(())
    }

    /// 是否绿。
    pub fn green(&self) -> bool {
        match self.gate() {
            Ok(()) => true,
            Err(_) => false,
        }
    }

    /// 渲染。
    pub fn render(&self) -> String {
        let mut s = String::from("cycle corpus=");
        s.push_str(u32_text(self.total as u32).as_str());
        s.push_str(" accept=");
        s.push_str(u32_text(self.accept as u32).as_str());
        s.push_str(" reject=");
        s.push_str(u32_text(self.reject as u32).as_str());
        s.push_str(" unresolved=");
        s.push_str(u32_text(self.unresolved as u32).as_str());
        s.push_str(" malformed=");
        s.push_str(u32_text(self.malformed as u32).as_str());
        s.push_str(" budget=");
        s.push_str(u32_text(self.budget as u32).as_str());
        s.push_str(" selfref=");
        s.push_str(u32_text(self.selfref as u32).as_str());
        s.push_str(" match=");
        s.push_str(u32_text(self.matched as u32).as_str());
        s.push_str(" rate=");
        s.push_str(u32_text(self.rate()).as_str());
        s.push_str("ppm\n");
        s
    }
}

/// 跑一遍。
pub fn run() -> Result<Report, u32> {
    walk(None)
}

/// 跑一遍并注入红档。
pub fn run_with(red: Option<(usize, u32)>) -> Result<Report, u32> {
    walk(red)
}

/// 行走。
fn walk(red: Option<(usize, u32)>) -> Result<Report, u32> {
    let mut r = Report::new();
    let mut i = 0usize;
    while i < CASES.len() {
        let c = match CASES.get(i) {
            Some(v) => *v,
            None => return Err(NC_EVALUE),
        };
        let run = solve(&c);
        r.total += 1;
        if run.ready() {
            r.accept += 1;
        } else {
            r.reject += 1;
            match run.code() {
                NC_ENOENT | NC_EAMBIG => r.unresolved += 1,
                NC_ETRUNC | NC_ENAME => r.malformed += 1,
                NC_EBUDGET => r.budget += 1,
                NC_ESELF => r.selfref += 1,
                _ => return Err(NC_EVER),
            }
        }
        if run.outcome() == c.expect {
            r.matched += 1;
        }
        i += 1;
    }
    if let Some((slot, code)) = red {
        if slot >= r.total {
            return Err(NC_ESLOT);
        }
        if code == 0 {
            return Err(NC_EVALUE);
        }
        r.matched = r.matched.saturating_sub(1);
        r.red = true;
    }
    Ok(r)
}

/// 前缀封面。
pub fn cover(n: usize) -> Result<(), u32> {
    if n > CORPUS {
        return Err(NC_ESLOT);
    }
    let mut i = 0usize;
    while i < n {
        let c = match slot_at(i) {
            Some(v) => v,
            None => return Err(NC_EVALUE),
        };
        if solve(&c).outcome() != c.expect {
            return Err(NC_EVALUE);
        }
        i += 1;
    }
    Ok(())
}

/// 某槽的码。
fn case_code(i: usize) -> u32 {
    match slot_at(i) {
        Some(c) => solve(&c).code(),
        None => NC_EVALUE,
    }
}

// ---------------------------------------------------------------------------
// 账本
// ---------------------------------------------------------------------------

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
    /// 空格。
    pub const fn blank() -> Sample {
        Sample { slot: 0, op: 0, ok: false, code: 0, value: 0 }
    }
}

/// 账本。
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
    pub fn new() -> Ledger {
        Ledger {
            rows: [Sample::blank(); SAMPLES],
            len: 0,
            root: 0,
            frozen: false,
            rejects: 0,
        }
    }

    /// 登记。
    pub fn register(&mut self, s: Sample) -> Result<(), u32> {
        if self.frozen {
            self.rejects = self.rejects.saturating_add(1);
            return Err(NC_ESEAL);
        }
        if (s.slot as usize) != self.len || (s.slot as usize) >= SAMPLES {
            self.rejects = self.rejects.saturating_add(1);
            return Err(NC_ESLOT);
        }
        if let Some(dst) = self.rows.get_mut(self.len) {
            *dst = s;
        }
        self.len += 1;
        Ok(())
    }

    /// 格数。
    pub const fn len(&self) -> usize {
        self.len
    }

    /// 是否空。
    pub const fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// 取格。
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
            return Err(NC_EBAL);
        }
        const VALS: [u32; 8] = [14, 8, 6, 2, 2, 1, 1, 929];
        const CODES: [u32; 6] =
            [NC_ENOENT, NC_ETRUNC, NC_ENAME, NC_EBUDGET, NC_EAMBIG, NC_ESELF];
        let mut vi = 0usize;
        let mut ci = 0usize;
        let mut i = 0usize;
        while i < self.len {
            let s = match self.rows.get(i) {
                Some(v) => *v,
                None => return Err(NC_EBAL),
            };
            if s.slot as usize != i {
                return Err(NC_ESLOT);
            }
            if s.ok {
                if s.code != 0 {
                    return Err(NC_EVALUE);
                }
                match VALS.get(vi) {
                    Some(w) => {
                        if s.value != *w {
                            return Err(NC_EVALUE);
                        }
                    }
                    None => return Err(NC_EBAL),
                }
                vi += 1;
            } else {
                match CODES.get(ci) {
                    Some(w) => {
                        if s.code != *w {
                            return Err(NC_EVALUE);
                        }
                    }
                    None => return Err(NC_EBAL),
                }
                ci += 1;
            }
            i += 1;
        }
        if vi != 8 || ci != 6 {
            return Err(NC_EBAL);
        }
        Ok(())
    }

    /// 指纹。
    pub fn seal(&self) -> u64 {
        let mut h = FNV_OFF;
        let mut i = 0usize;
        while i < self.len {
            if let Some(s) = self.rows.get(i) {
                h = fold_word(h, s.slot as u64);
                h = fold_word(h, s.op as u64);
                h = fold_word(h, s.ok as u64);
                h = fold_word(h, s.code as u64);
                h = fold_word(h, s.value as u64);
            }
            i += 1;
        }
        fold_word(h, self.rejects as u64)
    }

    /// 冻结。
    pub fn freeze(&mut self) -> Result<u64, u32> {
        if self.frozen {
            self.rejects = self.rejects.saturating_add(1);
            return Err(NC_ESTATE);
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
            Err(NC_ESEAL)
        }
    }

    /// 冻结指纹。
    pub const fn root(&self) -> u64 {
        self.root
    }

    /// 是否冻结。
    pub const fn is_frozen(&self) -> bool {
        self.frozen
    }

    /// 拒绝计数。
    pub const fn rejects(&self) -> u32 {
        self.rejects
    }
}

/// 计数格。
fn count_cell(slot: u8, op: u8, r: Result<usize, u32>) -> Sample {
    match r {
        Ok(v) => Sample { slot, op, ok: true, code: 0, value: v as u32 },
        Err(c) => Sample { slot, op, ok: false, code: c, value: 0 },
    }
}

/// 失败格。
fn err_cell(slot: u8, op: u8, code: u32) -> Sample {
    Sample { slot, op, ok: false, code, value: 0 }
}

/// 建账本：8 成 6 败。
pub fn build_ledger() -> Ledger {
    let mut l = Ledger::new();
    let r = run();
    let rr = run_with(Some((0, NC_ETRUNC)));

    let _ = l.register(count_cell(0, OP_CORPUS, r.map(|x| x.total())));
    let _ = l.register(count_cell(1, OP_CORPUS, r.map(|x| x.accept())));
    let _ = l.register(count_cell(2, OP_CORPUS, r.map(|x| x.reject())));
    let _ = l.register(count_cell(3, OP_LOAD, r.map(|x| x.unresolved())));
    let _ = l.register(count_cell(4, OP_LOAD, r.map(|x| x.malformed())));
    let _ = l.register(count_cell(5, OP_BUDGET, r.map(|x| x.budget())));
    let _ = l.register(count_cell(6, OP_CYCLE, r.map(|x| x.selfref())));
    let _ = l.register(count_cell(7, OP_TEXT, rr.map(|x| x.rate() as usize)));

    let _ = l.register(err_cell(8, OP_LOAD, case_code(8)));
    let _ = l.register(err_cell(9, OP_LOAD, case_code(9)));
    let _ = l.register(err_cell(10, OP_LOAD, case_code(10)));
    let _ = l.register(err_cell(11, OP_BUDGET, case_code(11)));
    let _ = l.register(err_cell(12, OP_AMBIG, case_code(12)));
    let _ = l.register(err_cell(13, OP_CYCLE, case_code(13)));

    l
}

// ---------------------------------------------------------------------------
// 判据
// ---------------------------------------------------------------------------

/// 节点基本语义。
fn node_ok() -> bool {
    let mut n = Node::blank("libz.so");
    let a = n.edges() == 0 && n.edge_at(0).is_none() && !n.installed();
    let b = n.need("liba.so") && n.need("libb.so") && n.need("libc.so");
    let c = !n.need("libd.so") && n.edges() == 3;
    let d = n.edge_at(0) == Some("liba.so") && n.edge_at(3).is_none();
    let mut s = Node::at("liba.so", "/lib/liba.so");
    s.need("liba.so");
    let e = s.self_edge() && s.installed() && s.sane();
    let mut r = Node::at("libx.so", "/opt/x/libx.so");
    let f = !r.via_runpath();
    r.mark_runpath();
    let g = r.via_runpath() && !r.is_preload();
    let mut p = Node::blank("libp.so");
    p.mark_preload();
    let h = p.is_preload();
    let mut t = Node::blank("libt.so");
    t.dyn_ok = false;
    let i = !t.sane();
    a && b && c && d && e && f && g && h && i
}

/// 图基本语义。
fn graph_ok() -> bool {
    let mut g = Graph::new();
    let a = g.is_empty() && g.len() == 0 && g.node(0).is_none();
    let mut x = Node::at("libx.so", "/lib/libx.so");
    x.need("liby.so");
    let mut y = Node::at("liby.so", "/lib/liby.so");
    y.need("libx.so");
    let b = g.push(x) && g.push(y) && g.len() == 2;
    let c = g.find("libx.so") == Some(0) && g.find("liby.so") == Some(1);
    let d = g.find("libz.so").is_none() && g.name_count("libx.so") == 1;
    let e = g.has_edge("libx.so", "liby.so") && !g.has_edge("libx.so", "libz.so");
    let f = g.edge_total() == 2 && g.preload_count() == 0;
    let mut dup = Graph::new();
    let _ = dup.push(Node::at("libdup.so", "/a/libdup.so"));
    let _ = dup.push(Node::at("libdup.so", "/b/libdup.so"));
    let h = dup.name_count("libdup.so") == 2 && dup.ambiguous("libdup.so");
    let i = !dup.ambiguous("libnone.so") && dup.find("libdup.so") == Some(0);
    let mut pre = Graph::new();
    let mut p = Node::at("libp.so", "/lib/libp.so");
    p.mark_preload();
    let _ = pre.push(p);
    let j = pre.preload_count() == 1;
    a && b && c && d && e && f && h && i && j
}

/// 环识别。
fn detect_ok() -> bool {
    let mut ring = Graph::new();
    let mut a = Node::at("liba.so", "/lib/liba.so");
    a.need("libb.so");
    let mut b = Node::at("libb.so", "/lib/libb.so");
    b.need("liba.so");
    let _ = ring.push(a);
    let _ = ring.push(b);
    let a_ok = ring.has_cycle();
    let mut dag = Graph::new();
    let mut x = Node::at("libx.so", "/lib/libx.so");
    x.need("liby.so");
    let y = Node::at("liby.so", "/lib/liby.so");
    let _ = dag.push(x);
    let _ = dag.push(y);
    let b_ok = !dag.has_cycle();
    let mut selfr = Graph::new();
    let mut s = Node::at("libs.so", "/lib/libs.so");
    s.need("libs.so");
    let _ = selfr.push(s);
    let c_ok = selfr.has_cycle();
    let mut three = Graph::new();
    let mut p = Node::at("libp.so", "/lib/libp.so");
    p.need("libq.so");
    let mut q = Node::at("libq.so", "/lib/libq.so");
    q.need("libr.so");
    let mut r = Node::at("libr.so", "/lib/libr.so");
    r.need("libp.so");
    let _ = three.push(p);
    let _ = three.push(q);
    let _ = three.push(r);
    let d_ok = three.has_cycle();
    let e_ok = Graph::new().has_cycle() == false;
    a_ok && b_ok && c_ok && d_ok && e_ok
}

/// 装载状态机：DAG 顺序与事件。
fn load_ok() -> bool {
    let c = match slot_at(0) {
        Some(v) => v,
        None => return false,
    };
    let g = graph_for(c.scenario);
    let r = load(&g, "./app", BUDGET_DEF);
    let a = r.ready() && r.code() == 0 && r.cycles() == 0;
    let b = r.order_len() == 4;
    let c1 = r.event_count(EventKind::Enter) == 4;
    let c2 = r.event_count(EventKind::Cycle) == 0;
    let c3 = r.budget_used() == 4;
    let d = r.order_at(0) == Some(0) && r.order_at(3) == Some(3);
    let e = r.order_at(4).is_none();
    let f = r.event_at(0).map(|x| x.kind) == Some(EventKind::Enter);
    let gg = match slot_at(1) {
        Some(v) => v,
        None => return false,
    };
    let r2 = solve(&gg);
    let h = r2.ready() && r2.cycles() == 1 && r2.event_count(EventKind::Cycle) == 1;
    let i = r2.order_len() == 3;
    a && b && c1 && c2 && c3 && d && e && f && h && i
}

/// 环接受实况：7 槽环族全 Ready 且环数符合。
fn cycle_accept_ok() -> bool {
    let mut i = 1usize;
    let mut seen = 0usize;
    let mut cycles = 0usize;
    while i < 8 {
        let c = match slot_at(i) {
            Some(v) => v,
            None => return false,
        };
        let r = solve(&c);
        if !r.ready() || r.code() != 0 {
            return false;
        }
        if r.cycles() != c.cycles {
            return false;
        }
        seen += 1;
        cycles += r.cycles() as usize;
        i += 1;
    }
    seen == 7 && cycles == 6
}

/// 环内拒收。
fn rejects_ok() -> bool {
    let mut i = 8usize;
    let mut n = 0usize;
    while i < 14 {
        let c = match slot_at(i) {
            Some(v) => v,
            None => return false,
        };
        let r = solve(&c);
        if r.ready() || r.code() == 0 || r.code() != c.code() {
            return false;
        }
        n += 1;
        i += 1;
    }
    n == 6
}

/// 文案。
fn text_ok() -> bool {
    let missing = match slot_at(8) {
        Some(c) => solve(&c),
        None => return false,
    };
    let trunc = match slot_at(9) {
        Some(c) => solve(&c),
        None => return false,
    };
    let empty = match slot_at(10) {
        Some(c) => solve(&c),
        None => return false,
    };
    let budget = match slot_at(11) {
        Some(c) => solve(&c),
        None => return false,
    };
    let amb = match slot_at(12) {
        Some(c) => solve(&c),
        None => return false,
    };
    let selfm = match slot_at(13) {
        Some(c) => solve(&c),
        None => return false,
    };
    missing.text()
        == "libgone.so: cannot open shared object file: No such file or directory"
        && trunc.text() == "libb.so: invalid dynamic section: DT_NEEDED offset beyond DT_STRSZ"
        && empty.text() == "libb.so: empty DT_NEEDED string offset"
        && budget.text() == "libn6.so: dependency expansion exceeds budget"
        && amb.text() == "libdup.so: same soname resolved to different objects"
        && selfm.text() == "libself.so: DT_SONAME mismatch on self reference"
}

/// 装载序与预载复用。
fn order_ok() -> bool {
    let c = match slot_at(6) {
        Some(v) => v,
        None => return false,
    };
    let r = solve(&c);
    let a = r.ready() && r.cycles() == 0;
    let b = r.order_at(0) == Some(2);
    let c1 = r.event_count(EventKind::Reuse) >= 2;
    let d = r.budget_used() == 2;
    let selfc = match slot_at(3) {
        Some(v) => v,
        None => return false,
    };
    let rs = solve(&selfc);
    let e = rs.ready() && rs.cycles() == 1 && rs.budget_used() == 2;
    let dup = match slot_at(7) {
        Some(v) => v,
        None => return false,
    };
    let rd = solve(&dup);
    let f = rd.ready() && rd.cycles() == 1;
    a && b && c1 && d && e && f
}

/// 事件账。
fn event_ok() -> bool {
    let c = match slot_at(1) {
        Some(v) => v,
        None => return false,
    };
    let r = solve(&c);
    let a = r.event_len() >= 4;
    let b = r.event_at(0).map(|e| e.kind) == Some(EventKind::Enter);
    let c1 = r.event_count(EventKind::Enter) == 3;
    let d = r.event_count(EventKind::Cycle) == 1;
    let e = r.event_count(EventKind::Reuse) == 0;
    let f = r.event_at(r.event_len()).is_none();
    let g = EventKind::Cycle.rejecting() == false;
    let h = EventKind::Missing.rejecting() && EventKind::Budget.rejecting();
    let i = EventKind::Enter.label() == "enter" && EventKind::BadSelf.label() == "bad-self";
    let j = r.diagnose().starts_with("run outcome=ready");
    a && b && c1 && d && e && f && g && h && i && j
}

/// 预算。
fn budget_ok() -> bool {
    let c = match slot_at(11) {
        Some(v) => v,
        None => return false,
    };
    let g = graph_for(c.scenario);
    let r = load(&g, "./app", 6);
    let a = r.outcome() == Outcome::Budget && r.code() == NC_EBUDGET;
    let b = r.budget_used() == 7 && r.budget() == 6;
    let c1 = r.subject() == "libn6.so" && r.requester() == "libn5.so";
    let c2 = match slot_at(11) {
        Some(v) => v,
        None => return false,
    };
    let r2 = load(&g, "./app", 8);
    let d = r2.ready() && r2.cycles() == 1 && r2.budget_used() == 8;
    let e = c2.budget == 6;
    a && b && c1 && d && e
}

/// 歧义与自环坏边。
fn ambiguity_ok() -> bool {
    let a = match slot_at(12) {
        Some(c) => solve(&c),
        None => return false,
    };
    let b = match slot_at(13) {
        Some(c) => solve(&c),
        None => return false,
    };
    let g = graph_for(Scenario::AmbiguousName);
    let c1 = g.name_count("libdup.so") == 2 && g.ambiguous("libdup.so");
    let d = a.outcome() == Outcome::Ambiguous && a.requester() == "./app";
    let e = b.outcome() == Outcome::BadSelf && b.subject() == "libself.so";
    let f = b.event_count(EventKind::BadSelf) == 1;
    a.code() == NC_EAMBIG && b.code() == NC_ESELF && c1 && d && e && f
}

/// 语料形态。
fn corpus_ok() -> bool {
    if CASES.len() != CORPUS {
        return false;
    }
    let mut ok = 0usize;
    let mut bad = 0usize;
    let mut i = 0usize;
    while i < CASES.len() {
        match CASES.get(i) {
            Some(c) => {
                if c.expect.ready() {
                    ok += 1;
                } else {
                    bad += 1;
                }
            }
            None => return false,
        }
        i += 1;
    }
    ok == 8 && bad == 6
}

/// 槽与表。
fn slots_ok() -> bool {
    unique()
        && notes_ok()
        && table_ok()
        && cover(0).is_ok()
        && cover(14).is_ok()
        && cover(15) == Err(NC_ESLOT)
        && slot_at(14).is_none()
        && slot_at(0).map(|c| c.scenario) == Some(Scenario::Dag)
        && slot_at(13).map(|c| c.scenario) == Some(Scenario::SelfMismatch)
}

/// 渲染。
fn report_ok() -> bool {
    let r = match run() {
        Ok(v) => v,
        Err(_) => return false,
    };
    let a = r.render()
        == "cycle corpus=14 accept=8 reject=6 unresolved=2 malformed=2 budget=1 selfref=1 match=14 rate=1000ppm\n";
    let b = r.rate() == 1000 && r.matched() == 14;
    let c = r.green() && r.unresolved() + r.malformed() + r.budget() + r.selfref() == 6;
    a && b && c
}

/// 红档与四舍五入。
fn reds_ok() -> bool {
    let red = match run_with(Some((3, NC_ENOENT))) {
        Ok(v) => v,
        Err(_) => return false,
    };
    let a = red.matched() == 13 && red.rate() == 929;
    let b = !red.green() && red.gate() == Err(NC_ESTATE);
    let c = run_with(Some((14, NC_ENOENT))) == Err(NC_ESLOT);
    let d = run_with(Some((0, 0))) == Err(NC_EVALUE);
    let e = match run() {
        Ok(v) => v.rate() == 1000,
        Err(_) => false,
    };
    a && b && c && d && e
}

/// 指纹与文本工具。
fn hash_ok() -> bool {
    let a = u32_text(0) == "0" && u32_text(929) == "929" && u32_text(4294967295) == "4294967295";
    let b = hex_text(0xfe00) == "0x0000fe00" && hex_text(0) == "0x00000000";
    let c = fold_byte(FNV_OFF, b'a') != FNV_OFF;
    let d = fold_word(FNV_OFF, 0) == digest(&[0u8; 8]);
    let mut h = FNV_OFF;
    h = fold_byte(h, b'U');
    h = fold_byte(h, b'N');
    h = fold_byte(h, b'X');
    let e = digest(b"UNX") == h;
    let f = quote_err("liba.so", "bad") == "liba.so: bad";
    a && b && c && d && e && f
}

/// 冻结重放。
fn pin_ok() -> bool {
    let mut l = build_ledger();
    let a = l.audit() == Ok(()) && l.len() == SAMPLES;
    let b = l.ok_count() == 8 && l.err_count() == 6;
    let root = match l.freeze() {
        Ok(v) => v,
        Err(_) => return false,
    };
    let c = root != 0 && l.root() == root && l.is_frozen();
    let d = l.replay() == Ok(root);
    let e = l.register(Sample::blank()) == Err(NC_ESEAL);
    let f = l.rejects() == 1 && l.freeze() == Err(NC_ESTATE) && l.rejects() == 2;
    let mut fresh = build_ledger();
    let g = match fresh.freeze() {
        Ok(v) => v == root,
        Err(_) => false,
    };
    let h = l.sample(0).map(|s| s.value) == Some(14)
        && l.sample(7).map(|s| s.value) == Some(929)
        && l.sample(14).is_none();
    a && b && c && d && e && f && g && h
}

/// 四注入。
pub fn inject_four() -> [bool; 4] {
    let mut out = [false; 4];
    out[0] = node_ok() && graph_ok() && detect_ok() && load_ok();
    out[1] = cycle_accept_ok() && rejects_ok() && text_ok();
    out[2] = order_ok() && event_ok() && budget_ok() && ambiguity_ok();
    out[3] = corpus_ok()
        && slots_ok()
        && report_ok()
        && reds_ok()
        && hash_ok()
        && pin_ok()
        && build_ledger().audit() == Ok(());
    out
}

/// 判据集合。
pub fn run_unx11627_checks() -> CheckSet {
    let mut s = CheckSet::new("unx11627.needed_cycle");

    s.add("NC01-节点与边表", node_ok(), "static");
    s.add("NC02-依赖图与同名节点", graph_ok(), "static");
    s.add("NC03-环识别三色 DFS", detect_ok(), "static");
    s.add("NC04-装载状态机与装载序", load_ok(), "static");
    s.add("NC05-环依赖装载成功", cycle_accept_ok(), "static");
    s.add("NC06-环内六类坏边拒收", rejects_ok(), "static");
    s.add("NC07-拒收文案与主体", text_ok(), "static");
    s.add("NC08-预载复用与重复边", order_ok(), "static");
    s.add("NC09-事件账与诊断行", event_ok(), "static");
    s.add("NC10-展开预算", budget_ok(), "static");
    s.add("NC11-同名歧义与自环 SONAME", ambiguity_ok(), "static");
    s.add("NC12-十四槽语料与渲染红档", slots_ok() && report_ok() && reds_ok(), "static");
    s.add("NC13-冻结重放四注入", inject_four() == [true; 4], "static");

    s
}

// ---------------------------------------------------------------------------
// 测试
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ledger_shape() {
        let l = build_ledger();
        assert_eq!(l.len(), SAMPLES);
        assert!(!l.is_empty());
        assert_eq!(l.ok_count(), 8);
        assert_eq!(l.err_count(), 6);
        assert_eq!(l.audit(), Ok(()));
        assert!(l.sample(0).is_some());
        assert!(l.sample(SAMPLES).is_none());
    }

    #[test]
    fn ledger_freeze_replay() {
        let mut l = build_ledger();
        assert_eq!(l.audit(), Ok(()));
        assert!(!l.is_frozen());
        let root = match l.freeze() {
            Ok(v) => v,
            Err(_) => return,
        };
        assert_ne!(root, 0);
        assert_eq!(l.root(), root);
        assert!(l.is_frozen());
        assert_eq!(l.replay(), Ok(root));
        assert_eq!(l.register(Sample::blank()), Err(NC_ESEAL));
        assert_eq!(l.rejects(), 1);
    }

    #[test]
    fn injections() {
        assert_eq!(inject_four(), [true; 4]);
    }

    #[test]
    fn all_checks_green() {
        let s = run_unx11627_checks();
        assert_eq!(s.len(), 13);
        assert!(s.all_passed());
        assert_eq!(s.failed_count(), 0);
    }

    #[test]
    fn text_tools() {
        assert_eq!(u32_text(0).as_str(), "0");
        assert_eq!(u32_text(14).as_str(), "14");
        assert_eq!(u32_text(929).as_str(), "929");
        assert_eq!(hex_text(0xfe00).as_str(), "0x0000fe00");
        assert_eq!(quote_err("a", "b").as_str(), "a: b");
    }

    #[test]
    fn fnv_known() {
        let mut h = FNV_OFF;
        h = fold_byte(h, b'a');
        assert_eq!(h, 0xaf63dc4c8601ec8c);
        assert_eq!(digest(b"a"), h);
        assert_ne!(fold_word(FNV_OFF, 1), FNV_OFF);
    }

    #[test]
    fn node_edges() {
        let mut n = Node::blank("liba.so");
        assert_eq!(n.edges(), 0);
        assert!(!n.installed());
        assert!(n.need("libb.so"));
        assert!(n.need("libc.so"));
        assert!(n.need("libd.so"));
        assert!(!n.need("libe.so"));
        assert_eq!(n.edges(), 3);
        assert_eq!(n.edge_at(0), Some("libb.so"));
        assert_eq!(n.edge_at(3), None);
        assert!(n.sane());
    }

    #[test]
    fn node_self_edge() {
        let mut n = Node::at("liba.so", "/lib/liba.so");
        assert!(!n.self_edge());
        n.need("liba.so");
        assert!(n.self_edge());
        assert!(n.installed());
    }

    #[test]
    fn graph_edges() {
        let mut g = Graph::new();
        assert!(g.is_empty());
        let mut a = Node::at("liba.so", "/lib/liba.so");
        a.need("libb.so");
        let b = Node::at("libb.so", "/lib/libb.so");
        assert!(g.push(a));
        assert!(g.push(b));
        assert_eq!(g.len(), 2);
        assert_eq!(g.find("libb.so"), Some(1));
        assert!(g.has_edge("liba.so", "libb.so"));
        assert_eq!(g.edge_total(), 1);
        assert!(!g.has_cycle());
    }

    #[test]
    fn graph_ambiguity() {
        let mut g = Graph::new();
        assert!(g.push(Node::at("libd.so", "/a/libd.so")));
        assert!(g.push(Node::at("libd.so", "/b/libd.so")));
        assert_eq!(g.name_count("libd.so"), 2);
        assert!(g.ambiguous("libd.so"));
        assert!(!g.ambiguous("libx.so"));
    }

    #[test]
    fn cycle_detection() {
        let g = graph_for(Scenario::Cycle2);
        assert!(g.has_cycle());
        let dag = graph_for(Scenario::Dag);
        assert!(!dag.has_cycle());
        let s = graph_for(Scenario::SelfLoop);
        assert!(s.has_cycle());
    }

    #[test]
    fn cycle2_accepted() {
        let c = match slot_at(1) {
            Some(v) => v,
            None => return,
        };
        let r = solve(&c);
        assert!(r.ready());
        assert_eq!(r.code(), 0);
        assert_eq!(r.cycles(), 1);
        assert!(r.accepted_cycle());
        assert_eq!(r.event_count(EventKind::Cycle), 1);
    }

    #[test]
    fn cycle3_accepted() {
        let c = match slot_at(2) {
            Some(v) => v,
            None => return,
        };
        let r = solve(&c);
        assert!(r.ready());
        assert_eq!(r.cycles(), 1);
        assert_eq!(r.budget_used(), 4);
    }

    #[test]
    fn self_loop_accepted() {
        let c = match slot_at(3) {
            Some(v) => v,
            None => return,
        };
        let r = solve(&c);
        assert!(r.ready());
        assert_eq!(r.order_len(), 2);
        assert_eq!(r.cycles(), 1);
    }

    #[test]
    fn branch_cycle_accepted() {
        let c = match slot_at(4) {
            Some(v) => v,
            None => return,
        };
        let r = solve(&c);
        assert!(r.ready());
        assert_eq!(r.budget_used(), 4);
        assert_eq!(r.cycles(), 1);
        assert_eq!(r.event_count(EventKind::Enter), 4);
    }

    #[test]
    fn runpath_cycle_accepted() {
        let c = match slot_at(5) {
            Some(v) => v,
            None => return,
        };
        let g = graph_for(c.scenario);
        assert!(g.runpath_ok());
        let r = solve(&c);
        assert!(r.ready());
        assert_eq!(r.cycles(), 1);
    }

    #[test]
    fn preload_breaks_cycle() {
        let c = match slot_at(6) {
            Some(v) => v,
            None => return,
        };
        let r = solve(&c);
        assert!(r.ready());
        assert_eq!(r.cycles(), 0);
        assert_eq!(r.order_at(0), Some(2));
        assert_eq!(r.budget_used(), 2);
    }

    #[test]
    fn duplicate_edge_single_cycle() {
        let c = match slot_at(7) {
            Some(v) => v,
            None => return,
        };
        let r = solve(&c);
        assert!(r.ready());
        assert_eq!(r.cycles(), 1);
        assert_eq!(r.event_count(EventKind::Reuse), 1);
    }

    #[test]
    fn missing_in_ring() {
        let c = match slot_at(8) {
            Some(v) => v,
            None => return,
        };
        let r = solve(&c);
        assert_eq!(r.outcome(), Outcome::Missing);
        assert_eq!(r.subject(), "libgone.so");
        assert_eq!(r.requester(), "libb.so");
    }

    #[test]
    fn trunc_in_ring() {
        let c = match slot_at(9) {
            Some(v) => v,
            None => return,
        };
        let r = solve(&c);
        assert_eq!(r.code(), NC_ETRUNC);
        assert_eq!(r.subject(), "libb.so");
        assert_eq!(r.requester(), "liba.so");
    }

    #[test]
    fn empty_in_ring() {
        let c = match slot_at(10) {
            Some(v) => v,
            None => return,
        };
        let r = solve(&c);
        assert_eq!(r.code(), NC_ENAME);
        assert!(r.text().ends_with("empty DT_NEEDED string offset"));
    }

    #[test]
    fn budget_in_ring() {
        let c = match slot_at(11) {
            Some(v) => v,
            None => return,
        };
        let r = solve(&c);
        assert_eq!(r.code(), NC_EBUDGET);
        assert_eq!(r.budget_used(), 7);
    }

    #[test]
    fn ambiguous_name() {
        let c = match slot_at(12) {
            Some(v) => v,
            None => return,
        };
        let r = solve(&c);
        assert_eq!(r.outcome(), Outcome::Ambiguous);
        assert_eq!(r.text().as_str(), "libdup.so: same soname resolved to different objects");
    }

    #[test]
    fn bad_self() {
        let c = match slot_at(13) {
            Some(v) => v,
            None => return,
        };
        let r = solve(&c);
        assert_eq!(r.outcome(), Outcome::BadSelf);
        assert_eq!(r.code(), NC_ESELF);
    }

    #[test]
    fn corpus_shape() {
        assert!(unique());
        assert!(notes_ok());
        assert!(table_ok());
        assert_eq!(CASES.len(), 14);
        assert!(slot_at(14).is_none());
    }

    #[test]
    fn corpus_counts() {
        let r = match run() {
            Ok(v) => v,
            Err(_) => Report::new(),
        };
        assert_eq!(r.total(), 14);
        assert_eq!(r.accept(), 8);
        assert_eq!(r.reject(), 6);
        assert!(r.green());
    }

    #[test]
    fn render_literal() {
        let r = match run() {
            Ok(v) => v,
            Err(_) => Report::new(),
        };
        assert_eq!(
            r.render().as_str(),
            "cycle corpus=14 accept=8 reject=6 unresolved=2 malformed=2 budget=1 selfref=1 match=14 rate=1000ppm\n"
        );
    }

    #[test]
    fn red_rate_rounding() {
        let r = match run_with(Some((0, NC_ENOENT))) {
            Ok(v) => v,
            Err(_) => Report::new(),
        };
        assert_eq!(r.rate(), 929);
        assert_eq!(r.gate(), Err(NC_ESTATE));
        assert_eq!(run_with(Some((14, NC_ENOENT))), Err(NC_ESLOT));
    }

    #[test]
    fn cover_prefix() {
        assert_eq!(cover(0), Ok(()));
        assert_eq!(cover(14), Ok(()));
        assert_eq!(cover(15), Err(NC_ESLOT));
    }

    #[test]
    fn error_codes_distinct() {
        let codes = [
            NC_ENOENT,
            NC_ETRUNC,
            NC_ENAME,
            NC_EBUDGET,
            NC_EAMBIG,
            NC_ESELF,
            NC_ESTATE,
            NC_EVER,
            NC_EBAL,
            NC_ESLOT,
            NC_EVALUE,
            NC_ESEAL,
            NC_ENOBS,
        ];
        let mut i = 0usize;
        while i < codes.len() {
            assert_eq!(codes[i], SEG_BASE + 1 + i as u32);
            i += 1;
        }
    }

    #[test]
    fn diagnose_shape() {
        let c = match slot_at(1) {
            Some(v) => v,
            None => return,
        };
        let r = solve(&c);
        let d = r.diagnose();
        assert!(d.starts_with("run outcome=ready subject=./app"));
        assert!(d.contains("cycles=1"));
        assert!(d.contains("ev 0 enter"));
        assert!(d.ends_with('\n'));
    }
}