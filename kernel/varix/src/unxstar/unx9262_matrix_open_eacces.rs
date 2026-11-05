//! UNX-F9262 open EACCES 矩阵（rwx 三位逐格） · 0x7AF00段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板 F9262 逐字）：
//!   rwx 三位逐格注入判据过，缺 r 读全 EACCES 格过，缺 w 写全 EACCES
//!   格过，缺 x 执行全 EACCES 格过。
//!
//! SEG_BASE = 0x7AF00：诊断码闭集说明——本模块全部诊断码由
//! `OpenCode` 闭集枚举给出，码值 = SEG_BASE + 变体序号（+0x01 起），
//! 段窗口 0x40 对齐独占；表外码不得进入开文件账。
//!
//! 设计要点：
//!   ① `VNode`/`FsTree`：路径分量注册表（目录 x 位穿越语义逐分量落账）。
//!   ② `parse_path`：路径字符串解析器（绝对/相对、空段、尾斜杠）。
//!   ③ `judge_open`：open 判定器——逐分量缺 x 穿越即 EACCES，末段按
//!      读/写方向比对 r/w 位；目录 x 位 × 文件 r/w 位组合矩阵全落账。
//!   ④ `OpenLedger`：注入账（注入=检出+逃逸，逃逸恒 0）。
//!   ⑤ `ERRNO_TABLE`：Linux errno 对照表；`fnv1a`/`corpus_fingerprint`
//!      双跑指纹。
//! ─────────────────────────────────────────────────────────────────────

use alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（F9262 独占 0x40 窗口）。
pub const SEG_BASE: u32 = 0x7AF00;

/// 单类注入复测轮数。
pub const INJECT_ROUNDS: u32 = 10;

/// 路径最大分量数（超出即 EINVAL）。
pub const MAX_COMPONENTS: usize = 8;

/// 权限位：读。
pub const PERM_R: u32 = 4;
/// 权限位：写。
pub const PERM_W: u32 = 2;
/// 权限位：执行/穿越。
pub const PERM_X: u32 = 1;

/// Linux errno 数值（正数口径）。
pub const EACCES: i32 = 13;
pub const ENOENT: i32 = 2;
pub const ENOTDIR: i32 = 20;
pub const EINVAL: i32 = 22;
pub const ELOOP: i32 = 40;

// ── 诊断码闭集 ────────────────────────────────────────────────────────
/// 本模块诊断码（段内 +0x01..+0x0C 闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum OpenCode {
    /// 中间分量缺 x 穿越被拒
    TraverseNoX,
    /// 末段缺 r 读被拒
    ReadNoR,
    /// 末段缺 w 写被拒
    WriteNoW,
    /// 末段缺 x 执行被拒
    ExecNoX,
    /// 中间分量不存在
    MidEnoent,
    /// 末段不存在（无 O_CREAT）
    LeafEnoent,
    /// 文件分量被当目录穿越
    NotDir,
    /// 路径分量数越上界
    TooManyComponents,
    /// 空路径
    EmptyPath,
    /// 权限矩阵格子漂移
    MatrixDrift,
    /// 双跑不一致
    NotReproducible,
    /// 段位越界
    SegmentViolation,
}

impl OpenCode {
    /// 码值（段基址 + 变体序号）。
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }

    /// 中文诊断与可操作建议（闭集返回，禁止空串）。
    pub fn detail(self) -> &'static str {
        match self {
            OpenCode::TraverseNoX => "中间分量缺 x 穿越被拒：目录的执行位是穿越权；\
                请给路径上每级目录补 x 位或换路径",
            OpenCode::ReadNoR => "末段缺 r 读被拒：文件属主/属组/other 三组按归属\
                短路取位；无 r 位读一律回 EACCES",
            OpenCode::WriteNoW => "末段缺 w 写被拒：请先 chmod 补 w 位或以属主\
                身份重试；无 w 位写一律回 EACCES",
            OpenCode::ExecNoX => "末段缺 x 执行被拒：文件无执行位不可 exec；\
                请补 x 位或改用解释器读取",
            OpenCode::MidEnoent => "中间分量不存在被拒：路径某级目录缺失；\
                请先创建目录再打开末端文件",
            OpenCode::LeafEnoent => "末段不存在被拒：无 O_CREAT 时不自动创建；\
                请确认文件名或加 O_CREAT",
            OpenCode::NotDir => "文件被当目录穿越被拒：路径中间分量是普通文件；\
                请核对路径层级，回 ENOTDIR",
            OpenCode::TooManyComponents => "路径分量数越上界被拒：最多 8 个分量；\
                请缩短路径或提升上限",
            OpenCode::EmptyPath => "空路径被拒：路径长度为 0 无解析意义；\
                请传入绝对或相对路径",
            OpenCode::MatrixDrift => "权限矩阵格子漂移：目录 x 位 × 文件 r/w 位\
                组合与登记账不符；请复算矩阵后重放",
            OpenCode::NotReproducible => "双跑不一致：同输入两次结果不同；\
                请排查隐藏状态或非确定哈希",
            OpenCode::SegmentViolation => "段位越界：本模块段基址必须落在 \
                0x7A000..=0x7D1C0 且 0x40 对齐",
        }
    }

    /// 闭集全量。
    pub const ALL: [OpenCode; 12] = [
        OpenCode::TraverseNoX,
        OpenCode::ReadNoR,
        OpenCode::WriteNoW,
        OpenCode::ExecNoX,
        OpenCode::MidEnoent,
        OpenCode::LeafEnoent,
        OpenCode::NotDir,
        OpenCode::TooManyComponents,
        OpenCode::EmptyPath,
        OpenCode::MatrixDrift,
        OpenCode::NotReproducible,
        OpenCode::SegmentViolation,
    ];
}

// ── 路径解析器 ───────────────────────────────────────────────────────
/// 解析路径字符串为分量序列：绝对路径以 '/' 开头；连续斜杠折叠为空段
/// 跳过；尾斜杠忽略；空路径拒绝；分量数越界拒绝。
pub fn parse_path(path: &str) -> Result<Vec<&str>, OpenCode> {
    if path.is_empty() {
        return Err(OpenCode::EmptyPath);
    }
    let mut comps: Vec<&str> = Vec::new();
    let bytes = path.as_bytes();
    let mut start = 0usize;
    let mut i = 0usize;
    // 首个 '/' 跳过（绝对路径）
    if bytes.first() == Some(&b'/') {
        start = 1;
        i = 1;
    }
    while i <= bytes.len() {
        if i == bytes.len() || bytes.get(i) == Some(&b'/') {
            let seg = path.get(start..i).unwrap_or("");
            if !seg.is_empty() {
                if comps.len() >= MAX_COMPONENTS {
                    return Err(OpenCode::TooManyComponents);
                }
                comps.push(seg);
            }
            start = i + 1;
        }
        i += 1;
    }
    if comps.is_empty() {
        return Err(OpenCode::EmptyPath);
    }
    Ok(comps)
}

// ── 虚拟文件系统分量表 ───────────────────────────────────────────────
/// 单个 vnode：权限位 + 是否目录。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct VNode {
    /// st_mode 低 9 位
    pub mode: u32,
    /// 是否目录
    pub is_dir: bool,
}

impl VNode {
    /// 目录构造。
    pub const fn dir(mode: u32) -> VNode {
        VNode { mode, is_dir: true }
    }

    /// 文件构造。
    pub const fn file(mode: u32) -> VNode {
        VNode { mode, is_dir: false }
    }

    /// 取某组 3 位（0=owner 1=group 2=other）。
    pub fn class_bits(&self, class: usize) -> u32 {
        match class {
            0 => (self.mode >> 6) & 0o7,
            1 => (self.mode >> 3) & 0o7,
            _ => self.mode & 0o7,
        }
    }
}

/// 归属主体（判定用 uid/gid 简化模型）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Cred {
    /// uid
    pub uid: u32,
    /// gid
    pub gid: u32,
}

impl Cred {
    /// 归属类：0=owner 1=group 2=other（owner 短路，不匹配组才看 other）。
    pub fn class_of(&self, uid: u32, gid: u32) -> usize {
        if self.uid == uid {
            return 0;
        }
        if self.gid == gid {
            return 1;
        }
        2
    }
}

/// 路径分量注册表：分量名 → vnode。
pub struct FsTree {
    names: Vec<&'static str>,
    nodes: Vec<VNode>,
    uids: Vec<u32>,
    gids: Vec<u32>,
}

impl FsTree {
    /// 构造空表。
    pub fn new() -> FsTree {
        FsTree { names: Vec::new(), nodes: Vec::new(), uids: Vec::new(), gids: Vec::new() }
    }

    /// 登记一个分量（重名拒绝，维护闭集注册表）。
    pub fn mount(&mut self, name: &'static str, node: VNode, uid: u32, gid: u32)
        -> Result<(), OpenCode> {
        let mut i = 0usize;
        while i < self.names.len() {
            if let Some(n) = self.names.get(i) {
                if *n == name {
                    return Err(OpenCode::NotDir); // 重名占位拒绝
                }
            }
            i += 1;
        }
        self.names.push(name);
        self.nodes.push(node);
        self.uids.push(uid);
        self.gids.push(gid);
        Ok(())
    }

    /// 按名查分量。
    pub fn lookup(&self, name: &str) -> Option<(usize, VNode, u32, u32)> {
        let mut i = 0usize;
        while i < self.names.len() {
            if let Some(n) = self.names.get(i) {
                if *n == name {
                    let node = self.nodes.get(i).copied().unwrap_or(VNode { mode: 0, is_dir: false });
                    let uid = self.uids.get(i).copied().unwrap_or(0);
                    let gid = self.gids.get(i).copied().unwrap_or(0);
                    return Some((i, node, uid, gid));
                }
            }
            i += 1;
        }
        None
    }

    /// 已登记分量数。
    pub fn len(&self) -> usize {
        self.names.len()
    }

    /// 判空。
    pub fn is_empty(&self) -> bool {
        self.names.is_empty()
    }
}

/// 打开方向。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum OpenKind {
    /// 只读（O_RDONLY）
    Read,
    /// 只写（O_WRONLY）
    Write,
    /// 执行（execve 语义）
    Exec,
}

/// open 判定器：逐分量 x 穿越检查 → 末段按方向比对 r/w/x 位。
/// 判定次序与 Linux 一致：先逐段穿越（EACCES/ENOENT/ENOTDIR），再末段位。
pub fn judge_open(
    tree: &FsTree,
    comps: &[&str],
    cred: &Cred,
    kind: OpenKind,
) -> Result<u32, i32> {
    if comps.is_empty() {
        return Err(EINVAL);
    }
    // 逐分量穿越：除末段外的每个分量必须是目录且需 x 位
    let mut i = 0usize;
    while i < comps.len() {
        let name = match comps.get(i) {
            Some(n) => *n,
            None => return Err(EINVAL),
        };
        let hit = match tree.lookup(name) {
            Some(h) => h,
            None => {
                return Err(ENOENT);
            }
        };
        let (_idx, node, uid, gid) = hit;
        if i + 1 == comps.len() {
            // 末段：按方向比对位
            let class = cred.class_of(uid, gid);
            let bits = node.class_bits(class);
            let need = match kind {
                OpenKind::Read => PERM_R,
                OpenKind::Write => PERM_W,
                OpenKind::Exec => PERM_X,
            };
            if bits & need == need {
                return Ok(bits);
            }
            return Err(EACCES);
        }
        // 中间分量：必须是目录且当前主体需有 x 位
        if !node.is_dir {
            return Err(ENOTDIR);
        }
        let class = cred.class_of(uid, gid);
        let bits = node.class_bits(class);
        if bits & PERM_X != PERM_X {
            return Err(EACCES);
        }
        i += 1;
    }
    Err(EINVAL)
}

// ── 目录 x × 文件 r/w 组合矩阵 ───────────────────────────────────────
/// 矩阵：目录 other 位 x∈{0,1} × 文件 other 位 r∈{0,1} × w∈{0,1}，
/// 对 other 主体读/写两个方向逐格裁定，期望账 8×2 格。
/// 返回（格子总数, 一致格数, 期望表 drift 标记）。
pub fn run_dirx_file_matrix() -> (u32, u32, bool) {
    let c = Cred { uid: 2000, gid: 3000 };
    let mut total = 0u32;
    let mut hit = 0u32;
    let mut drift = false;
    // 期望：目录 x=0 时任何方向都先 EACCES；x=1 时按文件位
    let mut dx = 0usize;
    while dx < 2 {
        let mut fr = 0usize;
        while fr < 2 {
            let mut fw = 0usize;
            while fw < 2 {
                let dir_mode = 0o755 & !(if dx == 0 { 0o001 } else { 0 });
                let file_mode = 0o600
                    | (if fr == 1 { 0o004 } else { 0 })
                    | (if fw == 1 { 0o002 } else { 0 });
                let mut tree = FsTree::new();
                let _ = tree.mount("d", VNode::dir(dir_mode), 1000, 1001);
                let _ = tree.mount("f", VNode::file(file_mode), 1000, 1001);
                // 路径 d/f：先穿 d（x 位），再按位开 f
                let comps = match parse_path("d/f") {
                    Ok(v) => v,
                    Err(_) => {
                        drift = true;
                        fw += 1;
                        continue;
                    }
                };
                // 期望：dx==0 → 穿越拒绝（读/写都 EACCES）；dx==1 → 按文件位
                let want_read_ok = dx == 1 && fr == 1;
                let want_write_ok = dx == 1 && fw == 1;
                let got_read = judge_open(&tree, &comps, &c, OpenKind::Read);
                let got_write = judge_open(&tree, &comps, &c, OpenKind::Write);
                let ok_read = match got_read {
                    Ok(_) => want_read_ok,
                    Err(e) => !want_read_ok && e == EACCES,
                };
                let ok_write = match got_write {
                    Ok(_) => want_write_ok,
                    Err(e) => !want_write_ok && e == EACCES,
                };
                total += 2;
                if ok_read {
                    hit += 1;
                }
                if ok_write {
                    hit += 1;
                }
                if !ok_read || !ok_write {
                    drift = true;
                }
                fw += 1;
            }
            fr += 1;
        }
        dx += 1;
    }
    (total, hit, !drift)
}

// ── 注入账 ───────────────────────────────────────────────────────────
/// 注入账：注入 = 检出 + 逃逸，逃逸必须恒 0。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct OpenLedger {
    /// 注入总次数
    pub injected: u32,
    /// 检出次数
    pub detected: u32,
    /// 逃逸次数
    pub escaped: u32,
}

impl OpenLedger {
    const fn zero() -> OpenLedger {
        OpenLedger { injected: 0, detected: 0, escaped: 0 }
    }

    /// 记一次注入结果。
    pub fn record(&mut self, caught: bool) -> Result<(), OpenCode> {
        self.injected = self.injected.saturating_add(1);
        if caught {
            self.detected = self.detected.saturating_add(1);
        } else {
            self.escaped = self.escaped.saturating_add(1);
        }
        if self.detected.saturating_add(self.escaped) != self.injected {
            return Err(OpenCode::MatrixDrift);
        }
        Ok(())
    }

    /// 守恒判定。
    pub fn conserved(&self) -> bool {
        self.detected.saturating_add(self.escaped) == self.injected
    }
}

// ── 坏样本注入矩阵 ───────────────────────────────────────────────────
/// 四类坏开文件样本。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BadOpenCase {
    /// 末段缺 r 读
    ReadNoR,
    /// 末段缺 w 写
    WriteNoW,
    /// 中间目录缺 x 穿越
    TraverseNoX,
    /// 中间分量缺失
    MidEnoent,
}

impl BadOpenCase {
    /// 执行注入：返回是否被判定器抓到。
    pub fn probe(self) -> bool {
        let mut tree = FsTree::new();
        let _ = tree.mount("d", VNode::dir(0o755), 1000, 1001);
        let _ = tree.mount("nr", VNode::file(0o200), 1000, 1001);
        let _ = tree.mount("nw", VNode::file(0o400), 1000, 1001);
        let _ = tree.mount("nox", VNode::dir(0o644), 1000, 1001);
        let c = Cred { uid: 2000, gid: 3000 };
        match self {
            BadOpenCase::ReadNoR => {
                let comps = parse_path("d/nr").unwrap_or_default();
                judge_open(&tree, &comps, &c, OpenKind::Read) == Err(EACCES)
            }
            BadOpenCase::WriteNoW => {
                let comps = parse_path("d/nw").unwrap_or_default();
                judge_open(&tree, &comps, &c, OpenKind::Write) == Err(EACCES)
            }
            BadOpenCase::TraverseNoX => {
                let comps = parse_path("nox/nw").unwrap_or_default();
                judge_open(&tree, &comps, &c, OpenKind::Write) == Err(EACCES)
            }
            BadOpenCase::MidEnoent => {
                let comps = parse_path("ghost/nw").unwrap_or_default();
                judge_open(&tree, &comps, &c, OpenKind::Write) == Err(ENOENT)
            }
        }
    }

    /// 期望诊断码。
    pub fn code(self) -> OpenCode {
        match self {
            BadOpenCase::ReadNoR => OpenCode::ReadNoR,
            BadOpenCase::WriteNoW => OpenCode::WriteNoW,
            BadOpenCase::TraverseNoX => OpenCode::TraverseNoX,
            BadOpenCase::MidEnoent => OpenCode::MidEnoent,
        }
    }

    /// 闭集全量。
    pub const ALL: [BadOpenCase; 4] = [
        BadOpenCase::ReadNoR,
        BadOpenCase::WriteNoW,
        BadOpenCase::TraverseNoX,
        BadOpenCase::MidEnoent,
    ];
}

/// 执行注入：每类 INJECT_ROUNDS 次，返回账本与逐类检出数。
pub fn run_bad_open_matrix() -> (OpenLedger, [u32; 4]) {
    let mut ledger = OpenLedger::zero();
    let mut per_class = [0u32; 4];
    let mut ci = 0usize;
    while ci < BadOpenCase::ALL.len() {
        if let Some(case) = BadOpenCase::ALL.get(ci).copied() {
            let mut round = 0u32;
            while round < INJECT_ROUNDS {
                let caught = case.probe();
                let _ = ledger.record(caught);
                if caught {
                    if let Some(slot) = per_class.get_mut(ci) {
                        *slot = slot.saturating_add(1);
                    }
                }
                round += 1;
            }
        }
        ci += 1;
    }
    (ledger, per_class)
}

// ── Linux errno 对照表 ───────────────────────────────────────────────
/// (名称, 正值, Windows 近似码, 说明)
pub const ERRNO_TABLE: &[(&str, i32, i32, &str)] = &[
    ("EACCES", 13, 5, "Windows ERROR_ACCESS_DENIED；对象权限不足族"),
    ("ENOENT", 2, 2, "Windows ERROR_FILE_NOT_FOUND；路径缺失族"),
    ("ENOTDIR", 20, 267, "Windows ERROR_DIRECTORY；非目录穿越"),
    ("EINVAL", 22, 87, "Windows ERROR_INVALID_PARAMETER；坏参数族"),
    ("ELOOP", 40, 10038, "Windows WSAENOTSOCK 占位；符号环"),
];

/// 查表复算：正值必须在表内。
pub fn agrees_with_linux(code: i32) -> bool {
    let mut hit = false;
    let mut i = 0usize;
    while i < ERRNO_TABLE.len() {
        if let Some(row) = ERRNO_TABLE.get(i) {
            if row.1 == code {
                hit = true;
            }
        }
        i += 1;
    }
    hit && code > 0
}

// ── 确定性指纹 ────────────────────────────────────────────────────────
/// FNV-1a 32 位。
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

/// 规范语料双跑指纹：矩阵逐格裁定 + 注入账入哈希。
pub fn corpus_fingerprint() -> u32 {
    let (total, hit, no_drift) = run_dirx_file_matrix();
    let mut bytes: Vec<u8> = Vec::new();
    let mut v = total;
    bytes.push((v & 0xFF) as u8);
    v = hit;
    bytes.push((v & 0xFF) as u8);
    bytes.push(if no_drift { 1 } else { 0 });
    let (ledger, per) = run_bad_open_matrix();
    bytes.push((ledger.detected & 0xFF) as u8);
    let mut k = 0usize;
    while k < per.len() {
        if let Some(n) = per.get(k) {
            bytes.push((*n & 0xFF) as u8);
        }
        k += 1;
    }
    fnv1a(&bytes)
}

// ── 检查入口 ──────────────────────────────────────────────────────────
/// F9262 判据集。
pub fn run_unx9262_checks() -> CheckSet {
    let mut s = CheckSet::new("unx9262");

    // 1. 段位守卫
    let seg_ok = SEG_BASE % 0x40 == 0 && (0x7A000..=0x7D1C0).contains(&SEG_BASE);
    s.add("段位守卫：0x7AF00 0x40 对齐且在批窗口内", seg_ok,
          "段基址必须唯一且落在本批窗口；越界即拒绝注册");

    // 2. 正常流：rwx 全开文件可读可写
    let mut tree = FsTree::new();
    let _ = tree.mount("d", VNode::dir(0o755), 1000, 1001);
    let _ = tree.mount("full", VNode::file(0o777), 1000, 1001);
    let c = Cred { uid: 2000, gid: 3000 };
    let comps_full = parse_path("d/full").unwrap_or_default();
    let normal_ok = judge_open(&tree, &comps_full, &c, OpenKind::Read).is_ok()
        && judge_open(&tree, &comps_full, &c, OpenKind::Write).is_ok();
    s.add("正常流：0o777 文件读/写均允许", normal_ok,
          "目录 x 位 + 文件 r/w 位齐备时两方向都放行");

    // 3. 缺 r 读全 EACCES（owner/group/other 三组各验一格）
    let owner = Cred { uid: 1000, gid: 1001 };
    let group = Cred { uid: 2000, gid: 1001 };
    let other = Cred { uid: 2000, gid: 3000 };
    let _ = tree.mount("nord_owner", VNode::file(0o077), 1000, 1001);
    let _ = tree.mount("nord_group", VNode::file(0o707), 1000, 1001);
    let _ = tree.mount("nord_other", VNode::file(0o770), 1000, 1001);
    let co = parse_path("d/nord_owner").unwrap_or_default();
    let cg = parse_path("d/nord_group").unwrap_or_default();
    let cx = parse_path("d/nord_other").unwrap_or_default();
    let nor_ok = judge_open(&tree, &co, &owner, OpenKind::Read) == Err(EACCES)
        && judge_open(&tree, &cg, &group, OpenKind::Read) == Err(EACCES)
        && judge_open(&tree, &cx, &other, OpenKind::Read) == Err(EACCES);
    s.add("缺 r 读：owner/组/other 三组各格全 EACCES", nor_ok,
          "缺 r 位读逐组注入全拒，不越组借位");

    // 4. 缺 w 写全 EACCES
    let _ = tree.mount("nowr_owner", VNode::file(0o477), 1000, 1001);
    let _ = tree.mount("nowr_group", VNode::file(0o747), 1000, 1001);
    let _ = tree.mount("nowr_other", VNode::file(0o774), 1000, 1001);
    let wo = parse_path("d/nowr_owner").unwrap_or_default();
    let wg = parse_path("d/nowr_group").unwrap_or_default();
    let wx = parse_path("d/nowr_other").unwrap_or_default();
    let now_ok = judge_open(&tree, &wo, &owner, OpenKind::Write) == Err(EACCES)
        && judge_open(&tree, &wg, &group, OpenKind::Write) == Err(EACCES)
        && judge_open(&tree, &wx, &other, OpenKind::Write) == Err(EACCES);
    s.add("缺 w 写：owner/组/other 三组各格全 EACCES", now_ok,
          "缺 w 位写逐组注入全拒");

    // 5. 缺 x 执行全 EACCES
    let _ = tree.mount("noex_owner", VNode::file(0o677), 1000, 1001);
    let _ = tree.mount("noex_group", VNode::file(0o767), 1000, 1001);
    let _ = tree.mount("noex_other", VNode::file(0o776), 1000, 1001);
    let xo = parse_path("d/noex_owner").unwrap_or_default();
    let xg = parse_path("d/noex_group").unwrap_or_default();
    let xx = parse_path("d/noex_other").unwrap_or_default();
    let nox_ok = judge_open(&tree, &xo, &owner, OpenKind::Exec) == Err(EACCES)
        && judge_open(&tree, &xg, &group, OpenKind::Exec) == Err(EACCES)
        && judge_open(&tree, &xx, &other, OpenKind::Exec) == Err(EACCES);
    s.add("缺 x 执行：owner/组/other 三组各格全 EACCES", nox_ok,
          "缺 x 位执行逐组注入全拒");

    // 6. 目录 x 位穿越：缺 x 即 EACCES（首段缺 x 优先于末段缺 w）
    let _ = tree.mount("nox", VNode::dir(0o644), 1000, 1001);
    let nox_path = parse_path("nox/nowr_owner").unwrap_or_default();
    s.add("缺 x 穿越：首段目录缺 x 优先报 EACCES",
          judge_open(&tree, &nox_path, &other, OpenKind::Write) == Err(EACCES),
          "穿越位检查先于末段位检查");

    // 7. ENOENT：末段缺失
    let ghost = parse_path("d/ghost").unwrap_or_default();
    s.add("末段缺失：ENOENT", judge_open(&tree, &ghost, &c, OpenKind::Read) == Err(ENOENT),
          "无 O_CREAT 时不自动创建");

    // 8. ENOTDIR：文件当目录穿越
    let _ = tree.mount("plain", VNode::file(0o644), 1000, 1001);
    let notdir = parse_path("plain/x").unwrap_or_default();
    s.add("非目录穿越：ENOTDIR",
          judge_open(&tree, &notdir, &c, OpenKind::Read) == Err(ENOTDIR),
          "文件分量不可当目录穿越");

    // 9. 边界：空路径 / 分量越界
    let empty_rej = parse_path("") == Err(OpenCode::EmptyPath);
    let long_path = "a/b/c/d/e/f/g/h/i";
    let too_many = parse_path(long_path) == Err(OpenCode::TooManyComponents);
    s.add("边界：空路径与 9 分量路径拒绝", empty_rej && too_many,
          "空路径 EINVAL 域拒绝；分量数超 MAX_COMPONENTS 拒绝");

    // 10. 解析器：绝对路径、双斜杠折叠、尾斜杠
    let abs = parse_path("/d/full");
    let dbl = parse_path("d//full");
    let tail = parse_path("d/full/");
    let parse_ok = match (abs, dbl, tail) {
        (Ok(a), Ok(b), Ok(t)) => a.len() == 2 && b.len() == 2 && t.len() == 2,
        _ => false,
    };
    s.add("解析器：绝对/双斜杠/尾斜杠均归一为 2 分量", parse_ok,
          "路径解析必须先归一再进判定器");

    // 11. 组合矩阵：8 格 × 2 方向全落账且零漂移
    let (total, hit, no_drift) = run_dirx_file_matrix();
    s.add("组合矩阵：目录x × 文件r/w 共 16 格全落账零漂移",
          total == 16 && hit == 16 && no_drift,
          "x=0 时两方向全拒、x=1 时按文件位逐格裁定");

    // 12. 注入必抓：四类坏样本各 10/10
    let (ledger, per_class) = run_bad_open_matrix();
    let all_caught = per_class.iter().all(|n| *n == INJECT_ROUNDS);
    s.add("注入必抓：缺r读/缺w写/缺x穿越/中间缺失各 10/10", all_caught,
          "四类违约样本必须全部被抓");

    // 13. 注入账守恒
    s.add("注入账守恒：注入=检出+逃逸且逃逸=0",
          ledger.conserved() && ledger.escaped == 0
              && ledger.injected == BadOpenCase::ALL.len() as u32 * INJECT_ROUNDS,
          "40 次注入必须 40 次检出、0 逃逸");

    // 14. 判定次序：穿越 EACCES 优先于末段 ENOENT（缺 x 目录下查不存在的末段）
    let _ = tree.mount("nx2", VNode::dir(0o644), 1000, 1001);
    let order_path = parse_path("nx2/ghost").unwrap_or_default();
    let order_ok = judge_open(&tree, &order_path, &other, OpenKind::Read) == Err(EACCES);
    s.add("判定次序：穿越 EACCES 先于末段 ENOENT", order_ok,
          "逐段检查顺序与 Linux 一致：先穿越后存在性");

    // 15. Linux 逐码一致
    let codes_ok = agrees_with_linux(EACCES) && agrees_with_linux(ENOENT)
        && agrees_with_linux(ENOTDIR) && agrees_with_linux(EINVAL)
        && !agrees_with_linux(0) && !agrees_with_linux(9999);
    s.add("Linux 逐码一致：表内精确、表外拒绝", codes_ok,
          "错误码只允许 errno 对照表内正值");

    // 16. 双跑一致
    let a = corpus_fingerprint();
    let b = corpus_fingerprint();
    s.add("双跑一致：规范语料指纹相等", a == b,
          "同输入两次必须逐字段相等，隐藏状态即失败");

    // 17. 文案闭集 + 码唯一
    let text_ok = OpenCode::ALL.iter().all(|cc| !cc.detail().is_empty());
    let mut uniq = true;
    let mut i17 = 0usize;
    while i17 < OpenCode::ALL.len() {
        let mut j17 = i17 + 1;
        while j17 < OpenCode::ALL.len() {
            if let (Some(x), Some(y)) = (OpenCode::ALL.get(i17), OpenCode::ALL.get(j17)) {
                if x.val() == y.val() {
                    uniq = false;
                }
            }
            j17 += 1;
        }
        i17 += 1;
    }
    s.add("文案闭集与码唯一：12 码非空且两两唯一", text_ok && uniq,
          "每个码必须能取到非空中文建议且段内不重复");

    // 动态明细（≤8）
    let ledger_text = format!("注入 {} / 检出 {} / 逃逸 {}",
                              ledger.injected, ledger.detected, ledger.escaped);
    s.add_dyn("账目快照", ledger.conserved() && ledger.escaped == 0, &ledger_text);
    let fp_text = format!("语料指纹 0x{:08X}", a);
    s.add_dyn("指纹快照", a == b, &fp_text);
    let matrix_text = format!("矩阵 {} 格 / 一致 {} 格", total, hit);
    s.add_dyn("矩阵快照", total == 16 && hit == 16, &matrix_text);

    // 18. 账目单调
    let mut mono = ledger;
    let before = mono.injected;
    let mut k = 0u32;
    while k < 4 {
        let _ = mono.record(true);
        k += 1;
    }
    s.add("账目单调：再注入 4 次只增不减", mono.injected == before + 4,
          "累计计数禁止回退或饱和到旧值");

    s
}

// ── 测试 ──────────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    fn build_tree() -> FsTree {
        let mut t = FsTree::new();
        let _ = t.mount("d", VNode::dir(0o755), 1000, 1001);
        let _ = t.mount("f", VNode::file(0o644), 1000, 1001);
        t
    }

    #[test]
    fn normal_rw_allowed() {
        let t = build_tree();
        let c = Cred { uid: 1000, gid: 1001 };
        let comps = parse_path("d/f").unwrap_or_default();
        assert!(judge_open(&t, &comps, &c, OpenKind::Read).is_ok());
        assert!(judge_open(&t, &comps, &c, OpenKind::Write).is_ok());
    }

    #[test]
    fn read_no_r_eacces_per_class() {
        let mut t = build_tree();
        let _ = t.mount("nr", VNode::file(0o200), 1000, 1001);
        let owner = Cred { uid: 1000, gid: 1001 };
        let other = Cred { uid: 2000, gid: 3000 };
        let comps = parse_path("d/nr").unwrap_or_default();
        assert_eq!(judge_open(&t, &comps, &owner, OpenKind::Read), Err(EACCES));
        assert_eq!(judge_open(&t, &comps, &other, OpenKind::Read), Err(EACCES));
    }

    #[test]
    fn write_no_w_eacces() {
        let mut t = build_tree();
        let _ = t.mount("nw", VNode::file(0o444), 1000, 1001);
        let owner = Cred { uid: 1000, gid: 1001 };
        let comps = parse_path("d/nw").unwrap_or_default();
        assert_eq!(judge_open(&t, &comps, &owner, OpenKind::Write), Err(EACCES));
    }

    #[test]
    fn exec_no_x_eacces() {
        let t = build_tree();
        let owner = Cred { uid: 1000, gid: 1001 };
        let comps = parse_path("d/f").unwrap_or_default();
        assert_eq!(judge_open(&t, &comps, &owner, OpenKind::Exec), Err(EACCES));
    }

    #[test]
    fn traverse_no_x_wins_over_leaf() {
        let mut t = build_tree();
        let _ = t.mount("nox", VNode::dir(0o644), 1000, 1001);
        let _ = t.mount("g", VNode::file(0o777), 1000, 1001);
        let other = Cred { uid: 2000, gid: 3000 };
        let comps = parse_path("nox/g").unwrap_or_default();
        assert_eq!(judge_open(&t, &comps, &other, OpenKind::Read), Err(EACCES));
    }

    #[test]
    fn missing_leaf_enoent() {
        let t = build_tree();
        let c = Cred { uid: 1000, gid: 1001 };
        let comps = parse_path("d/ghost").unwrap_or_default();
        assert_eq!(judge_open(&t, &comps, &c, OpenKind::Read), Err(ENOENT));
    }

    #[test]
    fn notdir_traverse() {
        let t = build_tree();
        let c = Cred { uid: 1000, gid: 1001 };
        let comps = parse_path("f/x").unwrap_or_default();
        assert_eq!(judge_open(&t, &comps, &c, OpenKind::Read), Err(ENOTDIR));
    }

    #[test]
    fn parser_normalization() {
        assert_eq!(parse_path("d//f").map(|v| v.len()), Ok(2));
        assert_eq!(parse_path("/d/f/").map(|v| v.len()), Ok(2));
        assert_eq!(parse_path(""), Err(OpenCode::EmptyPath));
        assert_eq!(parse_path("a/b/c/d/e/f/g/h/i"), Err(OpenCode::TooManyComponents));
    }

    #[test]
    fn matrix_all_cells_agree() {
        let (total, hit, no_drift) = run_dirx_file_matrix();
        assert_eq!(total, 16);
        assert_eq!(hit, 16);
        assert!(no_drift);
    }

    #[test]
    fn injection_matrix_all_caught() {
        let (l, per) = run_bad_open_matrix();
        assert!(l.conserved());
        assert_eq!(l.escaped, 0);
        for n in per.iter() {
            assert_eq!(*n, INJECT_ROUNDS);
        }
    }

    #[test]
    fn linux_codes_agree() {
        assert!(agrees_with_linux(13));
        assert!(!agrees_with_linux(0));
    }

    #[test]
    fn deterministic_corpus() {
        assert_eq!(corpus_fingerprint(), corpus_fingerprint());
    }

    #[test]
    fn unx9262_checks_green() {
        let s = run_unx9262_checks();
        assert!(s.all_passed(), "{}", s.red_items_text());
    }
}
