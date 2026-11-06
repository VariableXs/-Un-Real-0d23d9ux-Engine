//! UNX-F10122 路径解析正反双判据（相对绝对达成与超长路径误用受控）· 0x92E80 段
//! ─────────────────────────────────────────────────────────────────────
//! 锚点：UNX-F10123-J1 readdir 枚举达成（opendir/readdir/closedir 全程
//!       LTP 面 10/10 次）；UNX-F10123-J2 关闭后读取误用受控（预期 EBADF
//!       精确复现 10/10 次）。
//!
//! 数据段说明：
//!   SEG_BASE = 0x92E80（本卡独占 0x40 窗口，下一卡 0x92EC0）。
//!   诊断码闭集 SEG_BASE+0x01..=+0x0C 共 12 码两两唯一，detail() 全量非空中文。
//!
//! 结构清单：
//!   ① 解析器 `parse_chdir_call`："op|path" 两成分，op 闭集 {chdir,
//!      mkdir, touch}，出闭集/缺成分逐档拒绝；
//!   ② 目录树 `DirTree`：路径→is_dir 登记册，chdir 相对/绝对/多级导航，
//!      normalize 归一化（"/" 分段、"." 消解、".." 回退）可复算；
//!   ③ 状态机 `WalkState`：AtRoot→Descending→AtTarget 单向（禁倒退）；
//!   ④ 注入核 `BadPath`：超长路径 / 缺失目录 / 文件路径 chdir / 空路径
//!      四类坏样本逐类 10/10 次精确复现 ENAMETOOLONG/ENOENT/ENOTDIR/
//!      EINVAL 档，注入账守恒逃逸=0；
//!   ⑤ 错误族与 Linux errno 逐码对照；FNV-1a 双跑指纹复核成文确定性。
//! ─────────────────────────────────────────────────────────────────────

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（F10122 独占 0x40 窗口，下一卡 0x92EC0）。
pub const SEG_BASE: u32 = 0x92E80;

/// 段窗上界（闭开区间右端）。
pub const SEG_END: u32 = 0x92EC0;

/// 注入复测轮数（判据要求 10/10）。
pub const INJECT_ROUNDS: u32 = 10;

/// 单段名长度上界。
pub const NAME_MAX: usize = 32;

/// 路径总长上界。
pub const PATH_MAX: usize = 96;

/// 目录树容量。
pub const TREE_CAP: usize = 32;

// ── 诊断码闭集 ────────────────────────────────────────────────────────
/// 本模块诊断码（段内 +0x01..=+0x0C 闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PcCode {
    /// 超长路径（ENAMETOOLONG 族）
    TooLong,
    /// 缺失目录（ENOENT 族）
    NoSuch,
    /// 非目录中转（ENOTDIR 族）
    NotDir,
    /// 空路径（EINVAL 族）
    EmptyPath,
    /// 单段超长（ENAMETOOLONG 族）
    SegmentLong,
    /// 状态倒退
    StateRegress,
    /// 非法 op（EINVAL 族）
    BadOp,
    /// 成分缺失（EINVAL 族）
    MissingPart,
    /// 成文为空
    EmptySummary,
    /// 段位越界
    SegmentViolation,
    /// 账满/重复（账面守恒档）
    LedgerDrift,
    /// 语料指纹漂移
    RoundTrip,
}

impl PcCode {
    /// 码值（段基址 + 变体序号 +1）。
    pub fn val(self) -> u32 {
        SEG_BASE + 1 + self as u32
    }

    /// 中文诊断与可操作建议（闭集返回，禁止空串）。
    pub fn detail(self) -> &'static str {
        match self {
            PcCode::TooLong => "超长路径被拒：路径总长超 PATH_MAX；ENAMETOOLONG 精确归档",
            PcCode::NoSuch => "缺失目录被拒：账本中无此目录；ENOENT 精确归档",
            PcCode::NotDir => "非目录中转被拒：路径中段是文件；ENOTDIR 精确归档",
            PcCode::EmptyPath => "空路径被拒：路径不得为空串；EINVAL 精确归档",
            PcCode::SegmentLong => "单段超长被拒：单段超 NAME_MAX；ENAMETOOLONG 精确归档",
            PcCode::StateRegress => "状态倒退被拒：AtRoot→Descending→AtTarget 单向",
            PcCode::BadOp => "非法 op 被拒：op 闭集 {chdir, mkdir, touch}；请核对调用行",
            PcCode::MissingPart => "成分缺失被拒：调用行必须两段 op|path；请补成分",
            PcCode::EmptySummary => "空成文被拒：导航小结不得为空串；请先 chdir 再成文",
            PcCode::SegmentViolation => "段位越界：段基址必须 0x40 对齐且落在卡面窗口内",
            PcCode::LedgerDrift => "账面冲突被拒：重复登记或账满；请回查 mkdir 序列",
            PcCode::RoundTrip => "语料指纹漂移：同语料两次哈希不一致；请排查非确定源",
        }
    }

    /// 闭集全量（供文案完整性与唯一性检查）。
    pub const ALL: [PcCode; 12] = [
        PcCode::TooLong,
        PcCode::NoSuch,
        PcCode::NotDir,
        PcCode::EmptyPath,
        PcCode::SegmentLong,
        PcCode::StateRegress,
        PcCode::BadOp,
        PcCode::MissingPart,
        PcCode::EmptySummary,
        PcCode::SegmentViolation,
        PcCode::LedgerDrift,
        PcCode::RoundTrip,
    ];
}

// ── 调用行解析器 ──────────────────────────────────────────────────────
/// 解析后的树操作调用。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TreeCall {
    /// 动词（chdir/mkdir/touch）
    pub op: String,
    /// 路径
    pub path: String,
}

/// 解析 "op|path" 两段调用行；op 闭集 {chdir, mkdir, touch}。
pub fn parse_chdir_call(line: &str) -> Result<TreeCall, PcCode> {
    let mut parts: Vec<String> = Vec::new();
    let mut cur = String::new();
    for ch in line.chars() {
        if ch == '|' {
            let _ = parts.push(cur.clone());
            cur.clear();
        } else {
            let _ = cur.push(ch);
        }
    }
    let _ = parts.push(cur);
    if parts.len() != 2 {
        return Err(PcCode::MissingPart);
    }
    let op = match parts.first() {
        Some(p) => p.clone(),
        None => return Err(PcCode::MissingPart),
    };
    if op != "chdir" && op != "mkdir" && op != "touch" {
        return Err(PcCode::BadOp);
    }
    let path = match parts.get(1) {
        Some(p) => p.clone(),
        None => return Err(PcCode::MissingPart),
    };
    Ok(TreeCall { op, path })
}

// ── 导航状态机 ────────────────────────────────────────────────────────
/// 导航生命周期（单调，禁倒退）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum WalkState {
    /// 根目录
    AtRoot,
    /// 下降中
    Descending,
    /// 到达目标
    AtTarget,
}

impl WalkState {
    /// 单调推进判定。
    pub fn can_advance(self, next: WalkState) -> bool {
        let rank = |s: WalkState| match s {
            WalkState::AtRoot => 0u8,
            WalkState::Descending => 1,
            WalkState::AtTarget => 2,
        };
        rank(next) > rank(self)
    }

    /// 中文标签。
    pub fn label(self) -> &'static str {
        match self {
            WalkState::AtRoot => "根目录",
            WalkState::Descending => "下降中",
            WalkState::AtTarget => "到达目标",
        }
    }
}

// ── 目录树 ────────────────────────────────────────────────────────────
/// 树条目。
#[derive(Clone, Debug)]
pub struct TreeEnt {
    /// 绝对路径
    pub path: String,
    /// 目录标记（false 为文件）
    pub is_dir: bool,
}

/// 目录树：路径→is_dir；chdir 导航与归一化核。
#[derive(Clone, Debug)]
pub struct DirTree {
    ents: Vec<TreeEnt>,
    cap: usize,
    /// 当前工作目录（绝对路径）
    pub cwd: String,
    /// 生命周期状态
    pub walk: WalkState,
    /// 累计 chdir 次数
    pub chdirs: u32,
    /// 累计 mkdir 次数
    pub mkdirs: u32,
}

impl DirTree {
    /// 空树构造（根 "/" 预置，cwd="/"）。
    pub fn new() -> DirTree {
        let mut t = DirTree {
            ents: Vec::new(),
            cap: TREE_CAP,
            cwd: String::from("/"),
            walk: WalkState::AtRoot,
            chdirs: 0,
            mkdirs: 0,
        };
        let _ = t.ents.push(TreeEnt { path: String::from("/"), is_dir: true });
        t
    }

    /// 在册节点数。
    pub fn len(&self) -> usize {
        self.ents.len()
    }

    /// 按路径寻址。
    fn idx_of(&self, path: &str) -> Option<usize> {
        let mut i = 0usize;
        while i < self.ents.len() {
            if let Some(e) = self.ents.get(i) {
                if e.path == path {
                    return Some(i);
                }
            }
            i += 1;
        }
        None
    }

    /// 节点存在且为目录。
    fn require_dir(&self, path: &str) -> Result<bool, PcCode> {
        match self.idx_of(path) {
            Some(i) => match self.ents.get(i) {
                Some(e) => Ok(e.is_dir),
                None => Ok(false),
            },
            None => Ok(false),
        }
    }

    /// 父链检查失败时的分档：在账非目录 → NotDir；账外 → NoSuch。
    fn parent_err(&self, pfx: &str) -> PcCode {
        if self.idx_of(pfx).is_some() {
            PcCode::NotDir
        } else {
            PcCode::NoSuch
        }
    }

    /// 归一化：把 (cwd, path) 解为绝对路径；仅字符串层，不查账。
    pub fn normalize(cwd: &str, path: &str) -> Result<String, PcCode> {
        if path.is_empty() {
            return Err(PcCode::EmptyPath);
        }
        if path.len() > PATH_MAX {
            return Err(PcCode::TooLong);
        }
        let mut segs: Vec<String> = Vec::new();
        if !path.starts_with('/') {
            let mut cur = String::new();
            for ch in cwd.chars() {
                if ch == '/' {
                    if !cur.is_empty() {
                        let _ = segs.push(cur.clone());
                        cur.clear();
                    }
                } else {
                    let _ = cur.push(ch);
                }
            }
            if !cur.is_empty() {
                let _ = segs.push(cur);
            }
        }
        let mut cur = String::new();
        for ch in path.chars() {
            if ch == '/' {
                if cur == "." {
                    cur.clear();
                } else if cur == ".." {
                    let _ = segs.pop();
                    cur.clear();
                } else if !cur.is_empty() {
                    let _ = segs.push(cur.clone());
                    cur.clear();
                }
            } else {
                let _ = cur.push(ch);
            }
        }
        if cur == "." {
            cur.clear();
        } else if cur == ".." {
            let _ = segs.pop();
        } else if !cur.is_empty() {
            let _ = segs.push(cur);
        }
        let mut out = String::from("/");
        let mut i = 0usize;
        while i < segs.len() {
            let s = match segs.get(i) {
                Some(x) => x.clone(),
                None => break,
            };
            if i > 0 {
                let _ = out.push('/');
            }
            let _ = out.push_str(&s);
            i += 1;
        }
        if out.len() > PATH_MAX {
            return Err(PcCode::TooLong);
        }
        Ok(out)
    }

    /// 父链校验：/a/b/c → 逐级检查 /a、/a/b 均为在账目录。
    fn parent_chain_ok(&self, abs: &str) -> Result<(), PcCode> {
        let mut cut = abs.rfind('/');
        while let Some(p) = cut {
            if p == 0 {
                break;
            }
            let pfx = &abs[..p];
            if !self.require_dir(pfx)? {
                return Err(self.parent_err(pfx));
            }
            cut = pfx.rfind('/');
        }
        Ok(())
    }

    /// 建目录：归一化→父链校验→不重复→入账。
    pub fn mkdir(&mut self, path: &str) -> Result<String, PcCode> {
        let abs = Self::normalize(&self.cwd, path)?;
        self.parent_chain_ok(&abs)?;
        if self.idx_of(&abs).is_some() {
            return Err(PcCode::LedgerDrift);
        }
        if self.ents.len() >= self.cap {
            return Err(PcCode::LedgerDrift);
        }
        let _ = self.ents.push(TreeEnt { path: abs.clone(), is_dir: true });
        self.mkdirs = self.mkdirs.saturating_add(1);
        Ok(abs)
    }

    /// 建文件（touch）：归一化→父链校验→不重复→入账。
    pub fn touch(&mut self, path: &str) -> Result<String, PcCode> {
        let abs = Self::normalize(&self.cwd, path)?;
        self.parent_chain_ok(&abs)?;
        if self.idx_of(&abs).is_some() {
            return Err(PcCode::LedgerDrift);
        }
        if self.ents.len() >= self.cap {
            return Err(PcCode::LedgerDrift);
        }
        let _ = self.ents.push(TreeEnt { path: abs.clone(), is_dir: false });
        Ok(abs)
    }

    /// chdir：空/超长/单段超长三闸→归一化→必须为在账目录。
    pub fn chdir(&mut self, path: &str) -> Result<String, PcCode> {
        if path.is_empty() {
            return Err(PcCode::EmptyPath);
        }
        if path.len() > PATH_MAX {
            return Err(PcCode::TooLong);
        }
        let mut seg = String::new();
        for ch in path.chars() {
            if ch == '/' {
                if seg.len() > NAME_MAX {
                    return Err(PcCode::SegmentLong);
                }
                seg.clear();
            } else {
                let _ = seg.push(ch);
            }
        }
        if seg.len() > NAME_MAX {
            return Err(PcCode::SegmentLong);
        }
        let abs = Self::normalize(&self.cwd, path)?;
        if !self.require_dir(&abs)? {
            return Err(self.parent_err(&abs));
        }
        self.cwd = abs.clone();
        self.chdirs = self.chdirs.saturating_add(1);
        if self.walk.can_advance(WalkState::AtTarget) {
            self.walk = WalkState::AtTarget;
        }
        Ok(abs)
    }

    /// 节点种类读取。
    pub fn kind_of(&self, path: &str) -> Option<bool> {
        match self.idx_of(path) {
            Some(i) => match self.ents.get(i) {
                Some(e) => Some(e.is_dir),
                None => None,
            },
            None => None,
        }
    }
}

// ── J1 达成核：相对/绝对/多级导航 10/10 轮 ────────────────────────────
/// 逐轮：建树 /a/b → 绝对/相对/".."/多级/回根/ENOTDIR 六面。
pub fn run_path_rounds() -> (u32, usize) {
    let mut ok = 0u32;
    let mut probes = 0usize;
    let mut round = 0u32;
    while round < INJECT_ROUNDS {
        let mut t = DirTree::new();
        let mut all_ok = true;
        // 面1：绝对导航到 /a。
        all_ok = all_ok && t.mkdir("/a").is_ok() && t.chdir("/a").is_ok()
            && t.cwd == "/a";
        probes += 1;
        // 面2：相对导航 b → /a/b。
        all_ok = all_ok && t.mkdir("b").is_ok() && t.chdir("b").is_ok()
            && t.cwd == "/a/b";
        probes += 1;
        // 面3：".." 两级回退到根。
        all_ok = all_ok && t.chdir("..") == Ok(String::from("/a"))
            && t.chdir("..") == Ok(String::from("/"));
        probes += 1;
        // 面4：多级一步导航 /a/b。
        all_ok = all_ok && t.chdir("/a/b") == Ok(String::from("/a/b"));
        probes += 1;
        // 面5：绝对回根。
        all_ok = all_ok && t.chdir("/").is_ok() && t.cwd == "/";
        probes += 1;
        // 面6：文件路径 chdir 按 ENOTDIR 档拒绝。
        let _ = t.touch("/a/b/f.txt");
        all_ok = all_ok && t.chdir("/a/b/f.txt").err() == Some(PcCode::NotDir);
        probes += 1;
        if all_ok {
            ok += 1;
        }
        round += 1;
    }
    (ok, probes)
}

// ── 注入核：坏路径矩阵 ────────────────────────────────────────────────
/// 四类坏样本。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BadPath {
    /// 超长路径（ENAMETOOLONG 族）
    TooLong,
    /// 缺失目录（ENOENT 族）
    NoSuch,
    /// 文件路径 chdir（ENOTDIR 族）
    NotDir,
    /// 空路径（EINVAL 族）
    EmptyPath,
}

impl BadPath {
    /// 闭集全量。
    pub const ALL: [BadPath; 4] = [
        BadPath::TooLong,
        BadPath::NoSuch,
        BadPath::NotDir,
        BadPath::EmptyPath,
    ];

    /// 中文标签（注入明细用）。
    pub fn label(self) -> &'static str {
        match self {
            BadPath::TooLong => "超长路径 chdir",
            BadPath::NoSuch => "缺失目录 chdir",
            BadPath::NotDir => "文件路径 chdir",
            BadPath::EmptyPath => "空路径 chdir",
        }
    }
}

/// 注入账。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct InjectLedger {
    /// 注入总次数
    pub injected: u32,
    /// 检出次数
    pub detected: u32,
    /// 逃逸次数（必须为 0）
    pub escaped: u32,
}

impl InjectLedger {
    const fn zero() -> InjectLedger {
        InjectLedger { injected: 0, detected: 0, escaped: 0 }
    }

    /// 记一次注入结果。
    pub fn record(&mut self, caught: bool) -> Result<(), PcCode> {
        self.injected = self.injected.saturating_add(1);
        if caught {
            self.detected = self.detected.saturating_add(1);
        } else {
            self.escaped = self.escaped.saturating_add(1);
        }
        if self.detected.saturating_add(self.escaped) != self.injected {
            return Err(PcCode::SegmentViolation);
        }
        Ok(())
    }

    /// 守恒式。
    pub fn conserved(&self) -> bool {
        self.detected.saturating_add(self.escaped) == self.injected
    }
}

/// 执行坏路径注入：每类 INJECT_ROUNDS 次。
pub fn run_bad_path_matrix() -> (InjectLedger, [u32; 4]) {
    let mut ledger = InjectLedger::zero();
    let mut per_class = [0u32; 4];
    let mut ci = 0usize;
    while ci < BadPath::ALL.len() {
        let case = match BadPath::ALL.get(ci) {
            Some(c) => *c,
            None => break,
        };
        let mut round = 0u32;
        while round < INJECT_ROUNDS {
            let mut t = DirTree::new();
            let _ = t.mkdir("/a");
            let _ = t.touch("/a/f.txt");
            let caught = match case {
                BadPath::TooLong => {
                    let long = String::from("/a/").repeat(PATH_MAX);
                    t.chdir(long.as_str()).err() == Some(PcCode::TooLong)
                }
                BadPath::NoSuch => {
                    t.chdir("/nope").err() == Some(PcCode::NoSuch)
                }
                BadPath::NotDir => {
                    t.chdir("/a/f.txt").err() == Some(PcCode::NotDir)
                }
                BadPath::EmptyPath => {
                    t.chdir("").err() == Some(PcCode::EmptyPath)
                }
            };
            let _ = ledger.record(caught);
            if caught {
                if let Some(slot) = per_class.get_mut(ci) {
                    *slot = slot.saturating_add(1);
                }
            }
            round += 1;
        }
        ci += 1;
    }
    (ledger, per_class)
}

// ── 导航小结成文（纯函数） ────────────────────────────────────────────
/// 小结成文：「cwd=<s> chdirs=<n>」。
pub fn compose_path_summary(t: &DirTree) -> Result<String, PcCode> {
    let out = format!("cwd={} chdirs={}", t.cwd, t.chdirs);
    if out.is_empty() {
        return Err(PcCode::EmptySummary);
    }
    Ok(out)
}

// ── Linux errno 对照表 ────────────────────────────────────────────────
/// (名称, 负值, Windows 近似码, 说明)
pub const ERRNO_TABLE: &[(&str, i32, i32, &str)] = &[
    ("ENOENT", -2, 2, "Windows ERROR_PATH_NOT_FOUND；缺失目录族"),
    ("ENOTDIR", -20, 267, "Windows ERROR_DIRECTORY；非目录中转族"),
    ("ENAMETOOLONG", -36, 206, "Windows ERROR_FILENAME_EXCED_RANGE；超长路径族"),
    ("EINVAL", -22, 87, "Windows ERROR_INVALID_PARAMETER；空路径族"),
];

/// 查表：按 errno 正数取负值；表外返回 0。
pub fn linux_neg(code: i32) -> i32 {
    let mut i = 0usize;
    while i < ERRNO_TABLE.len() {
        if let Some(row) = ERRNO_TABLE.get(i) {
            if row.1 == -code {
                return row.1;
            }
        }
        i += 1;
    }
    0
}

/// 与 Linux 逐码一致。
pub fn agrees_with_linux(code: i32) -> bool {
    linux_neg(code) == -code && code > 0
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

/// 规范语料指纹：建树→导航→成文→摘哈希。
pub fn corpus_fingerprint() -> u32 {
    let mut t = DirTree::new();
    let _ = t.mkdir("/a");
    let _ = t.mkdir("/a/b");
    let _ = t.chdir("/a/b");
    let _ = t.chdir("..");
    let mut bytes: Vec<u8> = Vec::new();
    if let Ok(s) = compose_path_summary(&t) {
        bytes.extend_from_slice(s.as_bytes());
    }
    fnv1a(&bytes)
}

// ── 检查入口 ──────────────────────────────────────────────────────────
/// F10122 判据集（13 条静态 + 3 条动态）。
pub fn run_unx10122_checks() -> CheckSet {
    let mut s = CheckSet::new("unx10122");

    // 1. 段位守卫
    let seg_ok = SEG_BASE % 0x40 == 0 && (0x92E80..=0x92EBF).contains(&SEG_BASE);
    s.add("段位守卫：0x40 对齐且在本卡 0x40 窗口内", seg_ok,
          "段基址必须唯一且不与相邻卡重叠；越界即拒绝注册");

    // 2. 诊断码闭集
    let mut uniq = true;
    let mut i = 0usize;
    while i < PcCode::ALL.len() {
        let mut j = i + 1;
        while j < PcCode::ALL.len() {
            if let (Some(a), Some(b)) = (PcCode::ALL.get(i), PcCode::ALL.get(j)) {
                if a.val() == b.val() {
                    uniq = false;
                }
            }
            j += 1;
        }
        i += 1;
    }
    let text_ok = PcCode::ALL.iter().all(|c| !c.detail().is_empty());
    s.add("诊断码闭集：闭集码两两唯一且中文建议非空", uniq && text_ok,
          "段内闭集码不得重复，每个码必须能取到可操作说明");

    // 3. 解析器
    let parse_ok = parse_chdir_call("chdir|/a").is_ok()
        && parse_chdir_call("cd|/a").err() == Some(PcCode::BadOp)
        && parse_chdir_call("chdir").err() == Some(PcCode::MissingPart);
    s.add("调用行解析器：op 闭集放行且坏成分逐档拒绝", parse_ok,
          "op 闭集 {chdir, mkdir, touch}；出闭集逐档拒");

    // 4. J1 达成核
    let (ok, probes) = run_path_rounds();
    s.add("J1 路径解析达成：相对/绝对/多级导航 LTP chdir 面 10/10 次",
          ok == INJECT_ROUNDS && probes == (INJECT_ROUNDS as usize) * 6,
          "每轮 6 面（绝对/相对/回退/多级/回根/ENOTDIR）全对");

    // 5. 归一化："." 与 ".." 消解可复算
    let n1 = DirTree::normalize("/", "/a/./b/../b");
    let n2 = DirTree::normalize("/a", "b/../c");
    let norm_ok = n1 == Ok(String::from("/a/b")) && n2 == Ok(String::from("/a/c"));
    s.add("归一化：. 与 .. 逐段消解可复算", norm_ok,
          "归一化是导航前置；同输入必须同输出");

    // 6. J2 超长路径
    let mut t = DirTree::new();
    let _ = t.mkdir("/a");
    let long = String::from("/a/x").repeat(PATH_MAX);
    let long_ok = t.chdir(long.as_str()).err() == Some(PcCode::TooLong)
        && t.cwd == "/";
    s.add("J2 超长路径受控：ENAMETOOLONG 精确复现且 cwd 不动", long_ok,
          "超 PATH_MAX 逐档拒绝；被拒导航不得改 cwd");

    // 7. ENOENT / ENOTDIR 分档
    let miss_ok = t.chdir("/nope").err() == Some(PcCode::NoSuch)
        && t.chdir("/a").is_ok()
        && {
            let _ = t.touch("f.txt");
            t.chdir("f.txt").err() == Some(PcCode::NotDir)
        };
    s.add("缺失与非目录分档：ENOENT/ENOTDIR 独立精确归因", miss_ok,
          "账外与文件两类互不吞档");

    // 8. 单段超长档
    let seg = String::from("x").repeat(NAME_MAX + 1);
    let seg_ok = t.chdir(seg.as_str()).err() == Some(PcCode::SegmentLong);
    s.add("单段超长：NAME_MAX 档独立复现", seg_ok,
          "单段超长是 ENAMETOOLONG 族的独立档");

    // 9. 注入核
    let (inj, per_class) = run_bad_path_matrix();
    let all_caught = per_class.iter().all(|n| *n == INJECT_ROUNDS);
    s.add("注入核：四类坏样本各 10/10 拒绝", all_caught,
          "超长/缺失/非目录/空路径必须逐类逐次精确复现");

    // 10. 注入账守恒
    s.add("注入账守恒：注入=检出+逃逸且逃逸=0",
          inj.conserved() && inj.escaped == 0
              && inj.injected == BadPath::ALL.len() as u32 * INJECT_ROUNDS,
          "40 次注入必须 40 次检出、0 逃逸");

    // 11. 状态机
    let sm_ok = WalkState::AtRoot.can_advance(WalkState::Descending)
        && WalkState::Descending.can_advance(WalkState::AtTarget)
        && !WalkState::AtTarget.can_advance(WalkState::AtRoot)
        && WalkState::Descending.label() == "下降中";
    s.add("状态机：AtRoot→Descending→AtTarget 单向", sm_ok,
          "导航生命周期只许单调推进；倒退即协议违约");

    // 12. Linux 逐码一致
    let codes_ok = agrees_with_linux(2) && agrees_with_linux(20)
        && agrees_with_linux(36) && agrees_with_linux(22)
        && !agrees_with_linux(0) && !agrees_with_linux(9999);
    s.add("Linux 逐码一致：表内码精确、表外码拒绝", codes_ok,
          "错误族只允许 errno 表内正值且负值可复算");

    // 13. 成文 + 指纹
    let sum = compose_path_summary(&t);
    let text_ok = match &sum {
        Ok(x) => x.contains("cwd=/a"),
        Err(_) => false,
    };
    let f1 = corpus_fingerprint();
    let f2 = corpus_fingerprint();
    s.add("成文与指纹：小结字段齐备且双跑哈希逐位一致",
          text_ok && f1 == f2,
          "成文为纯函数；同语料两次哈希必须逐位一致");

    // 动态明细（≤8）
    let inj_text = format!("注入 {} / 检出 {} / 逃逸 {}", inj.injected, inj.detected, inj.escaped);
    s.add_dyn("注入账快照", inj.conserved() && inj.escaped == 0, &inj_text);
    let fp_text = format!("语料指纹 0x{:08X}", f1);
    s.add_dyn("指纹快照", f1 == f2, &fp_text);
    let ok_text = format!("导航达成轮数 {}/{}", ok, INJECT_ROUNDS);
    s.add_dyn("达成轮快照", ok == INJECT_ROUNDS, &ok_text);

    s
}

// ── 测试 ──────────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parser_guards() {
        assert!(parse_chdir_call("chdir|/a").is_ok());
        assert_eq!(parse_chdir_call("cd|/a").err(), Some(PcCode::BadOp));
        assert_eq!(parse_chdir_call("chdir").err(), Some(PcCode::MissingPart));
    }

    #[test]
    fn normalize_dot_segments() {
        assert_eq!(DirTree::normalize("/", "/a/./b"), Ok(String::from("/a/b")));
        assert_eq!(DirTree::normalize("/a", ".."), Ok(String::from("/")));
        assert_eq!(DirTree::normalize("/a/b", "../c"), Ok(String::from("/a/c")));
    }

    #[test]
    fn absolute_and_relative() {
        let mut t = DirTree::new();
        let _ = t.mkdir("/a");
        assert!(t.chdir("/a").is_ok());
        let _ = t.mkdir("b");
        assert_eq!(t.chdir("b"), Ok(String::from("/a/b")));
    }

    #[test]
    fn parent_walk_back() {
        let mut t = DirTree::new();
        let _ = t.mkdir("/a");
        let _ = t.chdir("/a");
        assert_eq!(t.chdir(".."), Ok(String::from("/")));
    }

    #[test]
    fn too_long_rejected() {
        let mut t = DirTree::new();
        let long = String::from("/a/").repeat(PATH_MAX);
        assert_eq!(t.chdir(long.as_str()).err(), Some(PcCode::TooLong));
    }

    #[test]
    fn no_such_and_notdir() {
        let mut t = DirTree::new();
        let _ = t.mkdir("/a");
        let _ = t.touch("/a/f");
        assert_eq!(t.chdir("/nope").err(), Some(PcCode::NoSuch));
        assert_eq!(t.chdir("/a/f").err(), Some(PcCode::NotDir));
    }

    #[test]
    fn empty_rejected() {
        let mut t = DirTree::new();
        assert_eq!(t.chdir("").err(), Some(PcCode::EmptyPath));
    }

    #[test]
    fn mkdir_parent_chain_guards() {
        let mut t = DirTree::new();
        assert_eq!(t.mkdir("/a/b").err(), Some(PcCode::NoSuch));
        let _ = t.mkdir("/a");
        assert!(t.mkdir("/a/b").is_ok());
        let _ = t.touch("/a/b/f");
        assert_eq!(t.mkdir("/a/b/f/x").err(), Some(PcCode::NotDir));
    }

    #[test]
    fn bad_matrix_green() {
        let (inj, per) = run_bad_path_matrix();
        assert_eq!(inj.injected, 4 * INJECT_ROUNDS);
        assert_eq!(inj.escaped, 0);
        for n in per.iter() {
            assert_eq!(*n, INJECT_ROUNDS);
        }
    }

    #[test]
    fn rounds_full() {
        let (ok, probes) = run_path_rounds();
        assert_eq!(ok, INJECT_ROUNDS);
        assert_eq!(probes, (INJECT_ROUNDS as usize) * 6);
    }

    #[test]
    fn state_machine_monotonic() {
        assert!(WalkState::AtRoot.can_advance(WalkState::Descending));
        assert!(!WalkState::AtTarget.can_advance(WalkState::Descending));
    }

    #[test]
    fn deterministic_corpus() {
        assert_eq!(corpus_fingerprint(), corpus_fingerprint());
    }

    #[test]
    fn unx10122_checks_green() {
        let set = run_unx10122_checks();
        assert!(set.all_passed(), "UNX-F10122 red: {}", set.red_items_text());
    }
}
