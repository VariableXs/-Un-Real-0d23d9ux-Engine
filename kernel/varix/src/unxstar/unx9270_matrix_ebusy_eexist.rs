//! UNX-F9270 EBUSY/EEXIST 冲突矩阵（独占创建） · 0x7B100段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板 F9270 逐字）：
//!   ① 纯功能代码 ≥320 行；② 判据原文：UNX-F9270-J1 O_EXCL 冲突注入判据过，
//!   全 EEXIST 格过，EBUSY 独占冲突格过；③ 完成时 API/软件写回：
//!   状态=已完成、结果摘要非空。
//!
//! 设计要点：
//!   ① 两个码不同层，不可互替：EEXIST 出自 VFS 名字解析——O_CREAT|O_EXCL 在
//!      lookup 阶段发现名字已存在就返回，根本不进驱动；EBUSY 出自驱动独占
//!      声明——块设备已被挂载（bd_holder）或已被 O_EXCL 租用。把两者折成一
//!      句「资源已存在」，挂载中的设备就会被误报成 EEXIST。
//!   ② 判定次序是本卡核心：同一目标同时满足「名字存在」与「设备忙」时，
//!      O_CREAT|O_EXCL 必须回 EEXIST（VFS 先于驱动），只有不带 O_CREAT 的
//!      纯 O_EXCL 才可能到达 EBUSY。矩阵专设 DevTakenByCreat 一格钉死此序。
//!   ③ 命名空间定容 12 槽：满时回 ENOSPC 且不得破坏既有节点，容量拒绝必须
//!      发生在落节点之前（先判容量后写入），否则「半创建」会污染对账。
//!   ④ 独占状态机 Idle→Inspect→Conflict→Claim→Commit 单向推进；冲突格停在
//!      Conflict，成功格才允许进 Claim/Commit，回退与跳档一律报红。
//!   ⑤ 注入账守恒：尝试 = 成功 + 冲突拒绝 + 其它拒绝，冲突五格各 10 轮共 50
//!      次注入零逃逸；账目只增不减，任何负增量显性拒绝。
//!   ⑥ 取值一律走 get()/match：节点表按下标取，越界回 None 而不是越界索引；
//!      错误码闭集枚举给出中文处置建议，杜绝自由文本与空串。
//! ─────────────────────────────────────────────────────────────────────
use alloc::format;

use crate::checks::CheckSet;

/// 段基址：F9270 独占 0x40 窗口。
pub const SEG_BASE: u32 = 0x7B100;
pub const MAX_NODES: usize = 12; // 命名空间节点上限（满即 ENOSPC）
pub const INJECT_ROUNDS: u32 = 10; // 单格注入复测轮数
pub const CONFLICT_CELLS: usize = 5; // 冲突格：EEXIST 三格 + EBUSY 两格
pub const MATRIX_CELLS: usize = 8; // 矩阵格总数

pub const ENOENT: i32 = 2; // 名字不存在
pub const EBUSY: i32 = 16; // 独占声明冲突
pub const EEXIST: i32 = 17; // 名字已存在
pub const EINVAL: i32 = 22; // 非设备上发起设备独占
pub const ENOSPC: i32 = 28; // 命名空间容量耗尽

pub const SLOT_NEW: u8 = 0; // 新建目标（初始不存在）
pub const SLOT_FILE: u8 = 1; // 已存在的普通文件
pub const SLOT_DIR: u8 = 2; // 已存在的目录
pub const SLOT_DEV: u8 = 3; // 块设备（可挂载/可租用）
pub const SLOT_DEV2: u8 = 4; // 第二块块设备
pub const SLOT_ABSENT_DEV: u8 = 5; // 不存在的设备路径

/// 名册（只用于文案与指纹；节点表只存槽号，故无字符串生命周期问题）。
pub const ROSTER: [&str; 6] = [
    "/tmp/f9270-new",
    "/tmp/f9270-taken.txt",
    "/tmp/f9270-dir",
    "/dev/f9270-sda",
    "/dev/f9270-sdb",
    "/tmp/f9270-absent-dev",
];

/// 槽号取名（越界给显式占位，禁止空串）。
pub fn name_of(slot: u8) -> &'static str {
    match ROSTER.get(slot as usize) {
        Some(n) => n,
        None => "<out-of-roster>",
    }
}

// ── 诊断码闭集 ────────────────────────────────────────────────────────
/// 本模块诊断码（段内 +0x00..+0x0B 闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ExclCode {
    NameTaken,        // EEXIST：名字已被占用
    DeviceMounted,    // EBUSY：设备已挂载
    DeviceLeased,     // EBUSY：设备已被独占租用
    OrderViolation,   // 次序错：EEXIST 必须早于 EBUSY
    NamespaceFull,    // ENOSPC：命名空间满
    WrongErrno,       // 码不符
    StageRegress,     // 状态机回退或跳档
    Escaped,          // 注入逃逸
    LedgerRegress,    // 注入账倒退
    NotReproducible,  // 双跑不一致
    TextGap,          // 文案缺口
    SegmentViolation, // 段位越界
}

impl ExclCode {
    /// 码值（段基址 + 变体序号）。
    pub fn val(self) -> u32 {
        SEG_BASE + match self {
            ExclCode::NameTaken => 0,
            ExclCode::DeviceMounted => 1,
            ExclCode::DeviceLeased => 2,
            ExclCode::OrderViolation => 3,
            ExclCode::NamespaceFull => 4,
            ExclCode::WrongErrno => 5,
            ExclCode::StageRegress => 6,
            ExclCode::Escaped => 7,
            ExclCode::LedgerRegress => 8,
            ExclCode::NotReproducible => 9,
            ExclCode::TextGap => 10,
            ExclCode::SegmentViolation => 11,
        }
    }
    /// 中文诊断与可操作建议（闭集返回，禁止空串）。
    pub fn detail(self) -> &'static str {
        match self {
            ExclCode::NameTaken => "名字已存在被拒（EEXIST）：O_CREAT|O_EXCL 在名字解析阶段即失败；请换名、先 unlink，或去掉 O_EXCL 改用普通创建",
            ExclCode::DeviceMounted => "设备忙被拒（EBUSY）：块设备仍被挂载持有；请先 umount 再发起 O_EXCL 独占打开",
            ExclCode::DeviceLeased => "设备忙被拒（EBUSY）：设备已被其它 O_EXCL 持有者租用；请等租约释放或改用非独占打开",
            ExclCode::OrderViolation => "判定次序错误：名字存在（EEXIST）必须先于设备忙（EBUSY）；顺序反了会掩盖 VFS 层的冲突语义",
            ExclCode::NamespaceFull => "命名空间满被拒：节点已达 12 槽上限；请在写入前判容量，先回收再申请，不得半创建",
            ExclCode::WrongErrno => "错误码不符：同形态同前置状态必须固定返回同一 errno；请对照冻结矩阵复算判定器",
            ExclCode::StageRegress => "状态机非法推进：只许 Idle→Inspect→Conflict→Claim→Commit 逐档前进，回退与跳档都拒绝",
            ExclCode::Escaped => "注入逃逸：冲突未被检出即视为判据失效；请回到判定器确认 EEXIST/EBUSY 分支是否可达",
            ExclCode::LedgerRegress => "注入账倒退：计数只增不减；请检查是否有回滚路径误改累计值",
            ExclCode::NotReproducible => "双跑不一致：同输入两次结果不同；请排查隐藏状态或非确定哈希",
            ExclCode::TextGap => "文案缺口：每个诊断码都必须带非空中文建议",
            ExclCode::SegmentViolation => "段位越界：本模块段基址必须落在 0x7A000..=0x7D1C0 且 0x40 对齐",
        }
    }
    /// 闭集全量（顺序即码序）。
    pub const ALL: [ExclCode; 12] = [
        ExclCode::NameTaken,
        ExclCode::DeviceMounted,
        ExclCode::DeviceLeased,
        ExclCode::OrderViolation,
        ExclCode::NamespaceFull,
        ExclCode::WrongErrno,
        ExclCode::StageRegress,
        ExclCode::Escaped,
        ExclCode::LedgerRegress,
        ExclCode::NotReproducible,
        ExclCode::TextGap,
        ExclCode::SegmentViolation,
    ];
}

// ── 命名空间模型 ──────────────────────────────────────────────────────
/// 节点类型：普通文件/目录/块设备。只有块设备会走驱动独占声明。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum NodeKind {
    File,
    Dir,
    BlockDev,
}

/// 已注册节点。`mounted` 与 `leased` 是两条独立的忙来源，必须分开记账：
/// 挂载由 mount 子系统持有，租用由 bd_holder 独占打开持有。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Node {
    pub slot: u8,
    pub kind: NodeKind,
    pub mounted: bool,
    pub leased: bool,
}

impl Node {
    /// 空占位（仅用于定容数组初始化）。
    pub const EMPTY: Node = Node { slot: 0, kind: NodeKind::File, mounted: false, leased: false };
    pub const fn file(slot: u8) -> Node {
        Node { slot, kind: NodeKind::File, mounted: false, leased: false }
    }
    pub const fn dir(slot: u8) -> Node {
        Node { slot, kind: NodeKind::Dir, mounted: false, leased: false }
    }
    pub const fn dev(slot: u8, mounted: bool, leased: bool) -> Node {
        Node { slot, kind: NodeKind::BlockDev, mounted, leased }
    }
    pub fn is_dev(self) -> bool {
        matches!(self.kind, NodeKind::BlockDev)
    }
}

/// 定容命名空间 + 四本账（尝试/成功/冲突/其它拒绝）。
///
/// 只存槽号不存名字：名字来自 `ROSTER`，避免把 `&'static str` 塞进可变表，
/// 也让「按名查重」退化成「按槽查重」，查找路径没有字符串比较的歧义。
#[derive(Clone, Copy, Debug)]
pub struct Namespace {
    nodes: [Node; MAX_NODES],
    len: usize,
    pub attempts: u32,  // 累计尝试（每次 open 入口 +1）
    pub creates: u32,   // 累计成功
    pub conflicts: u32, // 累计冲突拒绝（EEXIST/EBUSY）
    pub misses: u32,    // 累计其它拒绝（ENOENT/EINVAL/ENOSPC）
}

impl Namespace {
    pub const fn new() -> Namespace {
        Namespace {
            nodes: [Node::EMPTY; MAX_NODES],
            len: 0,
            attempts: 0,
            creates: 0,
            conflicts: 0,
            misses: 0,
        }
    }
    pub fn len(&self) -> usize {
        self.len
    }
    /// 按下标取节点（越界回 None）。
    pub fn at(&self, idx: usize) -> Option<Node> {
        if idx >= self.len {
            return None;
        }
        self.nodes.get(idx).copied()
    }
    /// 按槽号查节点下标（无字符串比较、无裸索引）。
    pub fn find(&self, slot: u8) -> Option<usize> {
        let mut i = 0usize;
        while i < self.len {
            if let Some(n) = self.nodes.get(i) {
                if n.slot == slot {
                    return Some(i);
                }
            }
            i += 1;
        }
        None
    }
    /// 追加节点：先判容量再写入，绝不半创建。
    pub fn push(&mut self, node: Node) -> Result<usize, i32> {
        let idx = self.len;
        if idx >= MAX_NODES {
            return Err(ENOSPC);
        }
        if let Some(cell) = self.nodes.get_mut(idx) {
            *cell = node;
        }
        self.len = idx + 1;
        Ok(idx)
    }
    /// 预置节点（不进尝试账，仅搭场景）。
    pub fn seed(&mut self, node: Node) -> Result<usize, i32> {
        self.push(node)
    }
    /// 守恒式：尝试 = 成功 + 冲突 + 其它。
    pub fn conserved(&self) -> bool {
        self.creates.saturating_add(self.conflicts).saturating_add(self.misses) == self.attempts
    }
}

// ── 独占创建判定器 ────────────────────────────────────────────────────
/// 打开形态。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum OpenShape {
    CreatExcl, // O_CREAT|O_EXCL：名字存在即 EEXIST（VFS 层，先于驱动）
    DevExcl,   // 纯 O_EXCL：名字须存在，随后走驱动独占声明（EBUSY 面）
}

/// O_CREAT|O_EXCL：名字查重 → 容量 → 落节点。三步有序，任何一步失败都不
/// 改动节点表。
pub fn create_excl(ns: &mut Namespace, slot: u8) -> Result<usize, i32> {
    ns.attempts = ns.attempts.saturating_add(1);
    if ns.find(slot).is_some() {
        ns.conflicts = ns.conflicts.saturating_add(1);
        return Err(EEXIST);
    }
    if ns.len >= MAX_NODES {
        ns.misses = ns.misses.saturating_add(1);
        return Err(ENOSPC);
    }
    match ns.push(Node::file(slot)) {
        Ok(idx) => {
            ns.creates = ns.creates.saturating_add(1);
            Ok(idx)
        }
        Err(_) => {
            ns.misses = ns.misses.saturating_add(1);
            Err(ENOSPC)
        }
    }
}

/// 纯 O_EXCL 打开设备：名字缺失 → ENOENT；非设备 → EINVAL；挂载或租用 →
/// EBUSY；否则声明成功。
pub fn open_dev_excl(ns: &mut Namespace, slot: u8) -> Result<usize, i32> {
    ns.attempts = ns.attempts.saturating_add(1);
    let idx = match ns.find(slot) {
        Some(i) => i,
        None => {
            ns.misses = ns.misses.saturating_add(1);
            return Err(ENOENT);
        }
    };
    let (kind, mounted, leased) = match ns.nodes.get(idx) {
        Some(n) => (n.kind, n.mounted, n.leased),
        None => {
            ns.misses = ns.misses.saturating_add(1);
            return Err(ENOENT);
        }
    };
    if kind != NodeKind::BlockDev {
        ns.misses = ns.misses.saturating_add(1);
        return Err(EINVAL);
    }
    if mounted || leased {
        ns.conflicts = ns.conflicts.saturating_add(1);
        return Err(EBUSY);
    }
    ns.creates = ns.creates.saturating_add(1);
    Ok(idx)
}

/// 按形态分派（矩阵统一入口）。
pub fn open_excl(ns: &mut Namespace, slot: u8, shape: OpenShape) -> Result<usize, i32> {
    match shape {
        OpenShape::CreatExcl => create_excl(ns, slot),
        OpenShape::DevExcl => open_dev_excl(ns, slot),
    }
}

// ── 冲突矩阵 ──────────────────────────────────────────────────────────
/// 八格矩阵：覆盖 EEXIST 三格、EBUSY 两格、成功两格、ENOENT 一格。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ExclCase {
    CleanCreate,     // 不存在 + O_CREAT|O_EXCL → 成功
    FileTaken,       // 普通文件已存在 + O_CREAT|O_EXCL → EEXIST
    DirTaken,        // 目录已存在 + O_CREAT|O_EXCL → EEXIST
    DevTakenByCreat, // 已挂载设备 + O_CREAT|O_EXCL → EEXIST（次序格）
    DevFree,         // 空闲设备 + 纯 O_EXCL → 成功
    DevMounted,      // 已挂载设备 + 纯 O_EXCL → EBUSY
    DevLeased,       // 已租用设备 + 纯 O_EXCL → EBUSY
    AbsentDevOpen,   // 设备路径不存在 + 纯 O_EXCL → ENOENT
}

impl ExclCase {
    /// 闭集全量（顺序即矩阵行序）。
    pub const ALL: [ExclCase; MATRIX_CELLS] = [
        ExclCase::CleanCreate,
        ExclCase::FileTaken,
        ExclCase::DirTaken,
        ExclCase::DevTakenByCreat,
        ExclCase::DevFree,
        ExclCase::DevMounted,
        ExclCase::DevLeased,
        ExclCase::AbsentDevOpen,
    ];
    /// 单元格名（文案与指纹共用；禁止自由文本）。
    pub fn name(self) -> &'static str {
        match self {
            ExclCase::CleanCreate => "CleanCreate",
            ExclCase::FileTaken => "FileTaken",
            ExclCase::DirTaken => "DirTaken",
            ExclCase::DevTakenByCreat => "DevTakenByCreat",
            ExclCase::DevFree => "DevFree",
            ExclCase::DevMounted => "DevMounted",
            ExclCase::DevLeased => "DevLeased",
            ExclCase::AbsentDevOpen => "AbsentDevOpen",
        }
    }
    /// 期望结果：0 表示成功，正数为 errno。
    pub fn expected(self) -> i32 {
        match self {
            ExclCase::CleanCreate | ExclCase::DevFree => 0,
            ExclCase::FileTaken | ExclCase::DirTaken | ExclCase::DevTakenByCreat => EEXIST,
            ExclCase::DevMounted | ExclCase::DevLeased => EBUSY,
            ExclCase::AbsentDevOpen => ENOENT,
        }
    }
    /// 是否冲突格（EEXIST/EBUSY 两类才进注入账）。
    pub fn is_conflict(self) -> bool {
        matches!(self.expected(), EEXIST | EBUSY)
    }
    /// 搭场景：每格独立命名空间，互不污染。
    pub fn setup(self) -> Namespace {
        let mut ns = Namespace::new();
        match self {
            ExclCase::CleanCreate => {}
            ExclCase::FileTaken => {
                let _ = ns.seed(Node::file(SLOT_FILE));
            }
            ExclCase::DirTaken => {
                let _ = ns.seed(Node::dir(SLOT_DIR));
            }
            ExclCase::DevTakenByCreat => {
                let _ = ns.seed(Node::dev(SLOT_DEV, true, false));
            }
            ExclCase::DevFree => {
                let _ = ns.seed(Node::dev(SLOT_DEV2, false, false));
            }
            ExclCase::DevMounted => {
                let _ = ns.seed(Node::dev(SLOT_DEV, true, false));
            }
            ExclCase::DevLeased => {
                let _ = ns.seed(Node::dev(SLOT_DEV2, false, true));
            }
            ExclCase::AbsentDevOpen => {}
        }
        ns
    }
    /// 本格的打开目标与形态。
    pub fn target(self) -> (u8, OpenShape) {
        match self {
            ExclCase::CleanCreate => (SLOT_NEW, OpenShape::CreatExcl),
            ExclCase::FileTaken => (SLOT_FILE, OpenShape::CreatExcl),
            ExclCase::DirTaken => (SLOT_DIR, OpenShape::CreatExcl),
            ExclCase::DevTakenByCreat => (SLOT_DEV, OpenShape::CreatExcl),
            ExclCase::DevFree => (SLOT_DEV2, OpenShape::DevExcl),
            ExclCase::DevMounted => (SLOT_DEV, OpenShape::DevExcl),
            ExclCase::DevLeased => (SLOT_DEV2, OpenShape::DevExcl),
            ExclCase::AbsentDevOpen => (SLOT_ABSENT_DEV, OpenShape::DevExcl),
        }
    }
}

/// 跑一格，返回 0 或 errno。
pub fn run_case(case: ExclCase) -> i32 {
    let mut ns = case.setup();
    let (slot, shape) = case.target();
    match open_excl(&mut ns, slot, shape) {
        Ok(_) => 0,
        Err(e) => e,
    }
}

/// 跑一格并回传命名空间（账目检查用）。
pub fn run_case_with_ns(case: ExclCase) -> (i32, Namespace) {
    let mut ns = case.setup();
    let (slot, shape) = case.target();
    let got = match open_excl(&mut ns, slot, shape) {
        Ok(_) => 0,
        Err(e) => e,
    };
    (got, ns)
}

// ── 状态机 ────────────────────────────────────────────────────────────
/// 独占创建状态机（只许单向逐档）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Stage {
    Idle,
    Inspect,
    Conflict,
    Claim,
    Commit,
}

impl Stage {
    pub fn rank(self) -> u8 {
        match self {
            Stage::Idle => 0,
            Stage::Inspect => 1,
            Stage::Conflict => 2,
            Stage::Claim => 3,
            Stage::Commit => 4,
        }
    }
    /// 下一档（Commit 之后没有下一档）。
    pub fn step(self) -> Option<Stage> {
        match self {
            Stage::Idle => Some(Stage::Inspect),
            Stage::Inspect => Some(Stage::Conflict),
            Stage::Conflict => Some(Stage::Claim),
            Stage::Claim => Some(Stage::Commit),
            Stage::Commit => None,
        }
    }
}

/// 只接受「恰好下一档」的推进；回退与跳档都返回 StageRegress。
pub fn advance(from: Stage, to: Stage) -> Result<Stage, ExclCode> {
    match from.step() {
        Some(next) => {
            if next == to {
                Ok(to)
            } else {
                Err(ExclCode::StageRegress)
            }
        }
        None => Err(ExclCode::StageRegress),
    }
}

/// 冲突格停在 Conflict，成功格走到 Commit。
pub fn stage_path(ok: bool) -> Result<Stage, ExclCode> {
    let mut st = Stage::Idle;
    st = advance(st, Stage::Inspect)?;
    st = advance(st, Stage::Conflict)?;
    if ok {
        st = advance(st, Stage::Claim)?;
        st = advance(st, Stage::Commit)?;
    }
    Ok(st)
}

// ── 注入账 ────────────────────────────────────────────────────────────
/// 注入账：注入 = 检出 + 逃逸，逃逸必须恒为 0。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Ledger {
    pub injected: u32,
    pub detected: u32,
    pub escaped: u32,
}

impl Ledger {
    pub const fn zero() -> Ledger {
        Ledger { injected: 0, detected: 0, escaped: 0 }
    }
    /// 记一次注入结果；自校验失败回 LedgerRegress。
    pub fn record(&mut self, caught: bool) -> Result<(), ExclCode> {
        self.injected = self.injected.saturating_add(1);
        if caught {
            self.detected = self.detected.saturating_add(1);
        } else {
            self.escaped = self.escaped.saturating_add(1);
        }
        if self.detected.saturating_add(self.escaped) != self.injected {
            return Err(ExclCode::LedgerRegress);
        }
        Ok(())
    }
    pub fn conserved(&self) -> bool {
        self.detected.saturating_add(self.escaped) == self.injected
    }
}

/// 冲突五格 × 10 轮注入；回传账本与逐格检出数（非冲突格恒为 0）。
pub fn run_excl_matrix() -> (Ledger, [u32; MATRIX_CELLS]) {
    let mut ledger = Ledger::zero();
    let mut per = [0u32; MATRIX_CELLS];
    let mut ci = 0usize;
    while ci < ExclCase::ALL.len() {
        if let Some(case) = ExclCase::ALL.get(ci).copied() {
            if case.is_conflict() {
                let want = case.expected();
                let mut round = 0u32;
                while round < INJECT_ROUNDS {
                    let caught = run_case(case) == want;
                    let _ = ledger.record(caught);
                    if caught {
                        if let Some(cell) = per.get_mut(ci) {
                            *cell = cell.saturating_add(1);
                        }
                    }
                    round += 1;
                }
            }
        }
        ci += 1;
    }
    (ledger, per)
}

/// 矩阵八格命中数。
pub fn matrix_hits() -> u32 {
    let mut hit = 0u32;
    let mut i = 0usize;
    while i < ExclCase::ALL.len() {
        if let Some(case) = ExclCase::ALL.get(i).copied() {
            if run_case(case) == case.expected() {
                hit = hit.saturating_add(1);
            }
        }
        i += 1;
    }
    hit
}

// ── 确定性指纹 ────────────────────────────────────────────────────────
/// FNV-1a 32 位（无浮点、无随机、无墙钟）。
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

/// 矩阵语料指纹：逐格名字 + 实际码 + 账目快照。
pub fn corpus_fingerprint() -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    let mut i = 0usize;
    while i < ExclCase::ALL.len() {
        if let Some(case) = ExclCase::ALL.get(i).copied() {
            let got = run_case(case);
            h ^= fnv1a(case.name().as_bytes()) ^ fnv1a(&got.to_le_bytes());
            h = h.wrapping_mul(0x0100_0193);
        }
        i += 1;
    }
    let (got, ns) = run_case_with_ns(ExclCase::FileTaken);
    h ^= got as u32;
    h = h.wrapping_mul(0x0100_0193);
    h ^= ns.attempts;
    h = h.wrapping_mul(0x0100_0193);
    h ^= ns.conflicts;
    h
}

// ── 检查入口 ──────────────────────────────────────────────────────────
/// F9270 判据集（≥12 条；动态明细 ≤8 条）。
pub fn run_unx9270_checks() -> CheckSet {
    let mut s = CheckSet::new("unx9270");

    // 1. 段位守卫
    let seg_ok = SEG_BASE % 0x40 == 0 && (0x7A000..=0x7D1C0).contains(&SEG_BASE);
    s.add("段位守卫：0x40 对齐且落在 0x7A000..=0x7D1C0", seg_ok,
          "段基址必须独占且落在本批窗口；越界即拒绝注册");

    // 2. 正常流：空命名空间上独占创建成功且节点可见
    let mut fresh = Namespace::new();
    let clean_ok = match create_excl(&mut fresh, SLOT_NEW) {
        Ok(idx) => fresh.at(idx).map(|n| n.slot) == Some(SLOT_NEW) && fresh.len() == 1,
        Err(_) => false,
    };
    s.add("正常流：不存在路径 + O_CREAT|O_EXCL 创建成功且可见", clean_ok,
          "成功路径必须落节点、入账，且可按槽号查回");

    // 3. 全 EEXIST 格：文件/目录/已挂载设备×创建形态 + 重复创建
    let mut dup = Namespace::new();
    let _ = create_excl(&mut dup, SLOT_NEW);
    let repeat = create_excl(&mut dup, SLOT_NEW) == Err(EEXIST);
    let eexist_cells = [ExclCase::FileTaken, ExclCase::DirTaken, ExclCase::DevTakenByCreat];
    let eexist_ok = eexist_cells.iter().all(|c| run_case(*c) == EEXIST) && repeat;
    s.add("全 EEXIST 格：文件/目录/设备创建态 + 重复创建 4/4 报 EEXIST", eexist_ok,
          "名字已存在一律 EEXIST，不得因类型不同改码");

    // 4. EBUSY 独占冲突格：已挂载、已租用
    let busy_ok = run_case(ExclCase::DevMounted) == EBUSY
        && run_case(ExclCase::DevLeased) == EBUSY;
    s.add("EBUSY 独占冲突格：已挂载/已租用 2/2 报 EBUSY", busy_ok,
          "驱动独占声明失败只报 EBUSY，与名字冲突彻底分层");

    // 5. 判定次序：EEXIST 先于 EBUSY
    let order_ok = run_case(ExclCase::DevTakenByCreat) == EEXIST
        && ExclCase::DevTakenByCreat.expected() != EBUSY;
    s.add("判定次序：已挂载设备 + O_CREAT|O_EXCL 报 EEXIST 而非 EBUSY", order_ok,
          "VFS 名字解析先于驱动独占声明，次序不可交换");

    // 6. 矩阵八格全命中
    let hits = matrix_hits();
    s.add("矩阵八格：逐格实际码与冻结期望全等", hits == MATRIX_CELLS as u32,
          "任一格漂移即说明判定器与矩阵失去同步");

    // 7. 注入 10/10 全检出
    let (ledger, per) = run_excl_matrix();
    let mut all_ten = true;
    let mut ci = 0usize;
    while ci < per.len() {
        if let Some(case) = ExclCase::ALL.get(ci).copied() {
            if let Some(n) = per.get(ci) {
                if case.is_conflict() && *n != INJECT_ROUNDS {
                    all_ten = false;
                }
            }
        }
        ci += 1;
    }
    s.add("冲突五格注入：各 10/10 全检出", all_ten,
          "每格每轮都必须复现期望码，一轮漏检即判据失效");

    // 8. 注入账守恒
    s.add("注入账守恒：注入=检出+逃逸且逃逸=0",
          ledger.conserved() && ledger.escaped == 0
              && ledger.injected == CONFLICT_CELLS as u32 * INJECT_ROUNDS,
          "50 次注入对应 50 次检出，逃逸恒为零");

    // 9. 账目守恒：尝试 = 成功 + 冲突 + 其它
    let (_, ns_file) = run_case_with_ns(ExclCase::FileTaken);
    let (_, ns_dev) = run_case_with_ns(ExclCase::DevMounted);
    let (_, ns_abs) = run_case_with_ns(ExclCase::AbsentDevOpen);
    s.add("账目守恒：尝试=成功+冲突+其它（三种场景逐本核对）",
          ns_file.conserved() && ns_dev.conserved() && ns_abs.conserved()
              && ns_file.attempts == 1 && ns_file.conflicts == 1 && ns_abs.misses == 1,
          "每格独立命名空间只允许一笔尝试，禁止重复记账");

    // 10. 容量：满 12 槽后拒绝且不破坏既有节点
    let mut full = Namespace::new();
    let mut fill = 0u8;
    while fill < MAX_NODES as u8 {
        let _ = full.seed(Node::file(100 + fill));
        fill += 1;
    }
    let before_len = full.len();
    let full_reject = create_excl(&mut full, 250) == Err(ENOSPC);
    let intact = full.len() == before_len && full.find(100).is_some() && full.find(250).is_none();
    s.add("容量：12 槽满后回 ENOSPC 且既有节点不受损", full_reject && intact,
          "容量拒绝必须先于落节点，半创建会污染对账");

    // 11. 缺失路径的设备独占 → ENOENT（不误报 EEXIST）
    let absent_ok = run_case(ExclCase::AbsentDevOpen) == ENOENT
        && run_case(ExclCase::DevFree) == 0;
    s.add("缺席面：设备路径不存在报 ENOENT，空闲设备独占成功", absent_ok,
          "ENOENT 与 EEXIST 必须由前置状态区分，不许合并");

    // 12. 非设备上发起设备独占 → EINVAL
    let mut file_ns = Namespace::new();
    let _ = file_ns.seed(Node::file(SLOT_FILE));
    let not_dev_ok = open_dev_excl(&mut file_ns, SLOT_FILE) == Err(EINVAL);
    s.add("类型面：普通文件上发起设备独占回 EINVAL", not_dev_ok,
          "设备独占只对块设备有意义，类型不符是参数错而非忙");

    // 13. 状态机单向：合法链可走通，回退与跳档拒绝
    let chain = [Stage::Inspect, Stage::Conflict, Stage::Claim, Stage::Commit];
    let mut st = Stage::Idle;
    let mut mono = true;
    for to in chain.iter() {
        match advance(st, *to) {
            Ok(next) => st = next,
            Err(_) => mono = false,
        }
    }
    let regress_rejected = advance(Stage::Commit, Stage::Claim).is_err();
    let skip_rejected = advance(Stage::Inspect, Stage::Commit).is_err();
    let stage_ok = mono
        && st == Stage::Commit
        && regress_rejected
        && skip_rejected
        && stage_path(true) == Ok(Stage::Commit)
        && stage_path(false) == Ok(Stage::Conflict);
    s.add("状态机：单向逐档推进，回退/跳档均拒", stage_ok,
          "冲突格停 Conflict、成功格进 Commit，档位不可逆");

    // 14. 双跑一致
    let a = corpus_fingerprint();
    let b = corpus_fingerprint();
    s.add("双跑一致：矩阵语料指纹相等", a == b,
          "同输入两次必须逐格相等，隐藏状态即失败");

    // 15. 文案闭集
    let text_ok = ExclCode::ALL.iter().all(|c| !c.detail().is_empty());
    s.add("文案闭集：12 个诊断码均有非空中文建议", text_ok,
          "每个码必须能取到可操作说明，空串即判据不可读");

    // 16. 诊断码唯一
    let mut uniq = true;
    let mut i = 0usize;
    while i < ExclCode::ALL.len() {
        let mut j = i + 1;
        while j < ExclCode::ALL.len() {
            if let (Some(x), Some(y)) = (ExclCode::ALL.get(i), ExclCode::ALL.get(j)) {
                if x.val() == y.val() {
                    uniq = false;
                }
            }
            j += 1;
        }
        i += 1;
    }
    s.add("诊断码：12 个码值两两唯一", uniq,
          "闭集码重复会让判据无法定位到具体缺陷");

    // 动态明细（≤8）：注入账、指纹、矩阵分布
    let ledger_text = format!("注入 {} / 检出 {} / 逃逸 {}", ledger.injected, ledger.detected, ledger.escaped);
    s.add_dyn("账目快照", ledger.conserved() && ledger.escaped == 0, &ledger_text);
    let fp_text = format!("语料指纹 0x{:08X}", a);
    s.add_dyn("指纹快照", a == b, &fp_text);
    let cell_text = format!("矩阵命中 {}/{} · EEXIST {} · EBUSY {}", hits, MATRIX_CELLS, EEXIST, EBUSY);
    s.add_dyn("矩阵快照", hits == MATRIX_CELLS as u32, &cell_text);

    s
}

// ── 测试 ──────────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clean_create_succeeds() {
        let mut ns = Namespace::new();
        let idx = create_excl(&mut ns, SLOT_NEW).unwrap_or(usize::MAX);
        assert_ne!(idx, usize::MAX);
        assert_eq!(ns.len(), 1);
        assert_eq!(ns.at(0).map(|n| n.slot), Some(SLOT_NEW));
        assert_eq!(ns.creates, 1);
        assert!(ns.conserved());
    }

    #[test]
    fn taken_name_eexist() {
        let mut ns = Namespace::new();
        let _ = ns.seed(Node::file(SLOT_FILE));
        assert_eq!(create_excl(&mut ns, SLOT_FILE), Err(EEXIST));
        let mut dir_ns = Namespace::new();
        let _ = dir_ns.seed(Node::dir(SLOT_DIR));
        assert_eq!(create_excl(&mut dir_ns, SLOT_DIR), Err(EEXIST));
        assert_eq!(ns.len(), 1);
        assert_eq!(ns.conflicts, 1);
    }

    #[test]
    fn mounted_dev_creat_returns_eexist_not_ebusy() {
        let got = run_case(ExclCase::DevTakenByCreat);
        assert_eq!(got, EEXIST);
        assert_ne!(got, EBUSY);
    }

    #[test]
    fn dev_conflict_cells() {
        assert_eq!(run_case(ExclCase::DevMounted), EBUSY);
        assert_eq!(run_case(ExclCase::DevLeased), EBUSY);
        assert_eq!(run_case(ExclCase::DevFree), 0);
        assert_eq!(run_case(ExclCase::AbsentDevOpen), ENOENT);
    }

    #[test]
    fn non_device_excl_einval() {
        let mut ns = Namespace::new();
        let _ = ns.seed(Node::file(SLOT_FILE));
        assert_eq!(open_dev_excl(&mut ns, SLOT_FILE), Err(EINVAL));
    }

    #[test]
    fn capacity_rejects_without_damage() {
        let mut ns = Namespace::new();
        let mut i = 0u8;
        while i < MAX_NODES as u8 {
            assert!(ns.seed(Node::file(i)).is_ok());
            i += 1;
        }
        assert_eq!(ns.len(), MAX_NODES);
        assert_eq!(create_excl(&mut ns, 200), Err(ENOSPC));
        assert_eq!(ns.len(), MAX_NODES);
        assert!(ns.find(200).is_none());
    }

    #[test]
    fn matrix_eight_cells_exact() {
        assert_eq!(matrix_hits(), MATRIX_CELLS as u32);
        for case in ExclCase::ALL.iter() {
            assert_eq!(run_case(*case), case.expected(), "cell {}", case.name());
        }
    }

    #[test]
    fn injection_ledger_conserved() {
        let (ledger, per) = run_excl_matrix();
        assert!(ledger.conserved());
        assert_eq!(ledger.escaped, 0);
        assert_eq!(ledger.injected, CONFLICT_CELLS as u32 * INJECT_ROUNDS);
        for (i, case) in ExclCase::ALL.iter().enumerate() {
            let n = per.get(i).copied().unwrap_or(0);
            if case.is_conflict() {
                assert_eq!(n, INJECT_ROUNDS);
            } else {
                assert_eq!(n, 0);
            }
        }
    }

    #[test]
    fn stage_machine_monotonic() {
        assert_eq!(stage_path(true), Ok(Stage::Commit));
        assert_eq!(stage_path(false), Ok(Stage::Conflict));
        assert!(advance(Stage::Commit, Stage::Claim).is_err());
        assert!(advance(Stage::Inspect, Stage::Commit).is_err());
        assert_eq!(advance(Stage::Idle, Stage::Inspect), Ok(Stage::Inspect));
        assert_eq!(Stage::Commit.step(), None);
    }

    #[test]
    fn deterministic_corpus() {
        assert_eq!(corpus_fingerprint(), corpus_fingerprint());
    }

    #[test]
    fn names_are_non_empty() {
        for slot in 0..ROSTER.len() {
            assert!(!name_of(slot as u8).is_empty());
        }
        assert!(!name_of(200).is_empty());
    }

    #[test]
    fn checks_all_green() {
        let s = run_unx9270_checks();
        assert!(s.all_passed(), "{}", s.red_items_text());
    }
}
