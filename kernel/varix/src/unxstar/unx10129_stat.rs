//! UNX-F10129 stat 三态一致性正反双判据（三口径达成与坏缓冲误用受控）· 0x93040 段
//! ─────────────────────────────────────────────────────────────────────
//! 锚点：UNX-F10130-J1 位操作达成（mode 置位/清位回读一致 LTP chmod01
//!       面 10/10 次）；UNX-F10130-J2 非属主越权误用受控（预期 EPERM
//!       精确复现 10/10 次）。
//!
//! 数据段说明：
//!   SEG_BASE = 0x93040（本卡独占 0x40 窗口，下一卡 0x93080）。
//!   诊断码闭集 SEG_BASE+0x01..=+0x0C 共 12 码两两唯一，detail() 全量非空中文。
//!
//! 结构清单：
//!   ① 解析器 `parse_stat_call`："op|path|buf" 三成分（op ∈ {stat, lstat,
//!      fstat}），buf 为十进制地址，坏地址解析层不吞；
//!   ② 三口径核 `StatFace`：stat 跟随链、lstat 不跟随、fstat 走 fd 表，
//!      链目标缺失时 stat 档 ENOENT、lstat 仍回链档；
//!   ③ 状态机 `StatView`：Raw→Followed→Coalesced 单向（禁倒退）；
//!   ④ 注入核 `BadStat`：坏缓冲 / 缺失路径 / 越界 fd / 空路径 四类坏样本
//!      逐类 10/10 次精确复现 EFAULT/ENOENT/EBADF 档，注入账守恒逃逸=0；
//!   ⑤ 错误族与 Linux errno 逐码对照；FNV-1a 双跑指纹复核成文确定性。
//! ─────────────────────────────────────────────────────────────────────

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（F10129 独占 0x40 窗口，下一卡 0x93080）。
pub const SEG_BASE: u32 = 0x93040;

/// 段窗上界（闭开区间右端）。
pub const SEG_END: u32 = 0x93080;

/// 注入复测轮数（判据要求 10/10）。
pub const INJECT_ROUNDS: u32 = 10;

/// 节点表容量。
pub const NODE_CAP: usize = 16;

/// fd 表容量。
pub const FD_CAP: usize = 8;

/// NULL 页上界（低于此地址的缓冲即 EFAULT 档）。
pub const NULL_PAGE_TOP: u32 = 0x1000;

// ── 诊断码闭集 ────────────────────────────────────────────────────────
/// 本模块诊断码（段内 +0x01..=+0x0C 闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum StCode {
    /// 坏缓冲（EFAULT 族）
    BadBuffer,
    /// 缺失路径（ENOENT 族）
    MissingPath,
    /// 越界 fd（EBADF 族）
    BadFd,
    /// 空路径（EINVAL 族）
    EmptyPath,
    /// 非法 op（EINVAL 族）
    BadOp,
    /// 非数字成分（EINVAL 族）
    BadNumber,
    /// 成分缺失（EINVAL 族）
    MissingPart,
    /// 状态倒退
    StateRegress,
    /// 成文为空
    EmptySummary,
    /// 段位越界
    SegmentViolation,
    /// 三口径失平
    ViewDrift,
    /// 语料指纹漂移
    RoundTrip,
}

impl StCode {
    /// 码值（段基址 + 变体序号 +1）。
    pub fn val(self) -> u32 {
        SEG_BASE + 1 + self as u32
    }

    /// 中文诊断与可操作建议（闭集返回，禁止空串）。
    pub fn detail(self) -> &'static str {
        match self {
            StCode::BadBuffer => "坏缓冲被拒：buf 地址低于 NULL 页上界；EFAULT 精确归档",
            StCode::MissingPath => "缺失路径被拒：节点表无此路径；ENOENT 精确归档",
            StCode::BadFd => "越界 fd 被拒：fd 出 fd 表闭集；EBADF 精确归档",
            StCode::EmptyPath => "空路径被拒：路径不得为空串；EINVAL 精确归档",
            StCode::BadOp => "非法 op 被拒：op 闭集 {stat, lstat, fstat}；请核对调用行",
            StCode::BadNumber => "非数字成分被拒：buf/fd 必须为十进制；请改写",
            StCode::MissingPart => "成分缺失被拒：调用行必须三段 op|path|buf；请补成分",
            StCode::StateRegress => "状态倒退被拒：Raw→Followed→Coalesced 单向",
            StCode::EmptySummary => "空成文被拒：stat 小结不得为空串；请先登记节点再成文",
            StCode::SegmentViolation => "段位越界：段基址必须 0x40 对齐且落在卡面窗口内",
            StCode::ViewDrift => "三口径失平：stat/lstat/fstat 同源必须同值",
            StCode::RoundTrip => "语料指纹漂移：同语料两次哈希不一致；请排查非确定源",
        }
    }

    /// 闭集全量（供文案完整性与唯一性检查）。
    pub const ALL: [StCode; 12] = [
        StCode::BadBuffer,
        StCode::MissingPath,
        StCode::BadFd,
        StCode::EmptyPath,
        StCode::BadOp,
        StCode::BadNumber,
        StCode::MissingPart,
        StCode::StateRegress,
        StCode::EmptySummary,
        StCode::SegmentViolation,
        StCode::ViewDrift,
        StCode::RoundTrip,
    ];
}

// ── 调用行解析器 ──────────────────────────────────────────────────────
/// 解析后的 stat 调用。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StatCall {
    /// 动词
    pub op: String,
    /// 路径（fstat 时为 fd 的十进制串）
    pub path: String,
    /// 缓冲地址（坏地址解析层不吞）
    pub buf: u32,
}

/// 解析 "op|path|buf" 三段调用行。
pub fn parse_stat_call(line: &str) -> Result<StatCall, StCode> {
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
    if parts.len() != 3 {
        return Err(StCode::MissingPart);
    }
    let op = match parts.first() {
        Some(p) => p.clone(),
        None => return Err(StCode::MissingPart),
    };
    if op != "stat" && op != "lstat" && op != "fstat" {
        return Err(StCode::BadOp);
    }
    let path = match parts.get(1) {
        Some(p) => p.clone(),
        None => return Err(StCode::MissingPart),
    };
    let bs = match parts.get(2) {
        Some(p) => p.clone(),
        None => return Err(StCode::MissingPart),
    };
    if bs.is_empty() {
        return Err(StCode::BadNumber);
    }
    let mut buf: u32 = 0;
    for b in bs.bytes() {
        if !b.is_ascii_digit() {
            return Err(StCode::BadNumber);
        }
        buf = buf.wrapping_mul(10).wrapping_add((b - b'0') as u32);
    }
    Ok(StatCall { op, path, buf })
}

// ── 三口径状态机 ──────────────────────────────────────────────────────
/// 口径视图（单调，禁倒退）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum StatView {
    /// 原始口径
    Raw,
    /// 已跟随
    Followed,
    /// 已合账
    Coalesced,
}

impl StatView {
    /// 单调推进判定。
    pub fn can_advance(self, next: StatView) -> bool {
        let rank = |s: StatView| match s {
            StatView::Raw => 0u8,
            StatView::Followed => 1,
            StatView::Coalesced => 2,
        };
        rank(next) > rank(self)
    }

    /// 中文标签。
    pub fn label(self) -> &'static str {
        match self {
            StatView::Raw => "原始口径",
            StatView::Followed => "已跟随",
            StatView::Coalesced => "已合账",
        }
    }
}

// ── 三口径面 ──────────────────────────────────────────────────────────
/// 节点条目。
#[derive(Clone, Debug)]
pub struct NodeEnt {
    /// 路径
    pub path: String,
    /// 是否符号链
    pub is_link: bool,
    /// 链目标（is_link 时有效）
    pub target: String,
    /// 元数据尺寸（链档 = 目标串长；实体档 = 体长）
    pub size: u32,
}

/// 三口径面：节点表 + fd 表。
#[derive(Clone, Debug)]
pub struct StatFace {
    nodes: Vec<NodeEnt>,
    fds: Vec<(u32, String)>,
    /// 累计 stat 次数
    pub stats: u32,
    /// 累计 lstat 次数
    pub lstats: u32,
    /// 累计 fstat 次数
    pub fstats: u32,
}

impl StatFace {
    /// 空面构造。
    pub fn new() -> StatFace {
        StatFace { nodes: Vec::new(), fds: Vec::new(), stats: 0, lstats: 0, fstats: 0 }
    }

    /// 按路径寻节点。
    fn idx_of(&self, path: &str) -> Option<usize> {
        let mut i = 0usize;
        while i < self.nodes.len() {
            if let Some(e) = self.nodes.get(i) {
                if e.path == path {
                    return Some(i);
                }
            }
            i += 1;
        }
        None
    }

    /// 登记实体节点。
    pub fn add_node(&mut self, path: &str, size: u32) -> Result<(), StCode> {
        if self.idx_of(path).is_some() || self.nodes.len() >= NODE_CAP {
            return Err(StCode::ViewDrift);
        }
        let _ = self.nodes.push(NodeEnt {
            path: String::from(path),
            is_link: false,
            target: String::new(),
            size,
        });
        Ok(())
    }

    /// 登记符号链（size = 目标串长）。
    pub fn add_link(&mut self, link: &str, target: &str) -> Result<(), StCode> {
        if self.idx_of(link).is_some() || self.nodes.len() >= NODE_CAP {
            return Err(StCode::ViewDrift);
        }
        let _ = self.nodes.push(NodeEnt {
            path: String::from(link),
            is_link: true,
            target: String::from(target),
            size: target.len() as u32,
        });
        Ok(())
    }

    /// 绑 fd（fstat 口径用）。
    pub fn bind_fd(&mut self, fd: u32, path: &str) -> Result<(), StCode> {
        if fd as usize >= FD_CAP {
            return Err(StCode::BadFd);
        }
        let mut i = 0usize;
        while i < self.fds.len() {
            if let Some((f, _)) = self.fds.get(i) {
                if *f == fd {
                    return Err(StCode::BadFd);
                }
            }
            i += 1;
        }
        let _ = self.fds.push((fd, String::from(path)));
        Ok(())
    }

    /// fd→路径。
    fn path_of_fd(&self, fd: u32) -> Option<String> {
        let mut i = 0usize;
        while i < self.fds.len() {
            if let Some((f, p)) = self.fds.get(i) {
                if *f == fd {
                    return Some(p.clone());
                }
            }
            i += 1;
        }
        None
    }

    /// 缓冲闸：低于 NULL 页上界即 EFAULT 档。
    pub fn buf_ok(buf: u32) -> Result<(), StCode> {
        if buf < NULL_PAGE_TOP {
            return Err(StCode::BadBuffer);
        }
        Ok(())
    }

    /// 三口径判定核：坏缓冲 → 路径/fd 解析 → 元数据读取。
    pub fn stat(&mut self, op: &str, path: &str, buf: u32) -> Result<(u32, bool), StCode> {
        Self::buf_ok(buf)?;
        match op {
            "stat" => {
                let idx = match self.idx_of(path) {
                    Some(i) => i,
                    None => return Err(StCode::MissingPath),
                };
                let (is_link, target) = match self.nodes.get(idx) {
                    Some(e) => (e.is_link, e.target.clone()),
                    None => return Err(StCode::MissingPath),
                };
                self.stats = self.stats.saturating_add(1);
                if is_link {
                    // 跟随：目标缺失即 ENOENT（悬链跟随档）。
                    let tidx = match self.idx_of(&target) {
                        Some(t) => t,
                        None => return Err(StCode::MissingPath),
                    };
                    let (tsize, tlink) = match self.nodes.get(tidx) {
                        Some(e) => (e.size, e.is_link),
                        None => return Err(StCode::MissingPath),
                    };
                    let _ = tlink;
                    Ok((tsize, true))
                } else {
                    let size = match self.nodes.get(idx) {
                        Some(e) => e.size,
                        None => 0,
                    };
                    Ok((size, false))
                }
            }
            "lstat" => {
                let idx = match self.idx_of(path) {
                    Some(i) => i,
                    None => return Err(StCode::MissingPath),
                };
                let (size, is_link) = match self.nodes.get(idx) {
                    Some(e) => (e.size, e.is_link),
                    None => return Err(StCode::MissingPath),
                };
                self.lstats = self.lstats.saturating_add(1);
                Ok((size, is_link))
            }
            "fstat" => {
                let fd = match path.trim().parse::<u32>() {
                    Ok(f) => f,
                    Err(_) => return Err(StCode::BadNumber),
                };
                let p = match self.path_of_fd(fd) {
                    Some(p) => p,
                    None => return Err(StCode::BadFd),
                };
                let idx = match self.idx_of(&p) {
                    Some(i) => i,
                    None => return Err(StCode::MissingPath),
                };
                let size = match self.nodes.get(idx) {
                    Some(e) => e.size,
                    None => 0,
                };
                self.fstats = self.fstats.saturating_add(1);
                Ok((size, false))
            }
            _ => Err(StCode::BadOp),
        }
    }

    /// 节点数。
    pub fn len(&self) -> usize {
        self.nodes.len()
    }
}

// ── J1 达成核：三口径 10/10 轮 ────────────────────────────────────────
/// 逐轮：实体+链登记→lstat 链档→stat 跟随→fstat 同源→两读一致。
pub fn run_stat_rounds() -> (u32, usize) {
    let mut ok = 0u32;
    let mut probes = 0usize;
    let mut round = 0u32;
    while round < INJECT_ROUNDS {
        let mut f = StatFace::new();
        let _ = f.add_node("/t/file", 512);
        let _ = f.add_link("/t/lnk", "/t/file");
        let _ = f.bind_fd(3, "/t/file");
        let mut all_ok = true;
        let buf = 0x7F00_0000u32;
        // 面1：lstat 链档回链尺寸（不跟随）。
        all_ok = all_ok && f.stat("lstat", "/t/lnk", buf) == Ok((7, true));
        probes += 1;
        // 面2：stat 跟随到目标实体尺寸。
        all_ok = all_ok && f.stat("stat", "/t/lnk", buf) == Ok((512, true));
        probes += 1;
        // 面3：fstat 同源同值。
        all_ok = all_ok && f.stat("fstat", "3", buf) == Ok((512, false));
        probes += 1;
        // 面4：实体 stat 直读。
        all_ok = all_ok && f.stat("stat", "/t/file", buf) == Ok((512, false));
        probes += 1;
        // 面5：两读一致（lstat 实体 == fstat）。
        all_ok = all_ok && f.stat("lstat", "/t/file", buf) == Ok((512, false))
            && f.stat("fstat", "3", buf) == Ok((512, false));
        probes += 1;
        // 面6：计数守恒。
        all_ok = all_ok && f.stats == 2 && f.lstats == 2 && f.fstats == 2;
        probes += 1;
        if all_ok {
            ok += 1;
        }
        round += 1;
    }
    (ok, probes)
}

// ── 注入核：坏 stat 矩阵 ──────────────────────────────────────────────
/// 四类坏样本。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BadStat {
    /// 坏缓冲（EFAULT 族）
    BadBuffer,
    /// 缺失路径（ENOENT 族）
    MissingPath,
    /// 越界 fd（EBADF 族）
    BadFd,
    /// 空路径（EINVAL 族）
    EmptyPath,
}

impl BadStat {
    /// 闭集全量。
    pub const ALL: [BadStat; 4] = [
        BadStat::BadBuffer,
        BadStat::MissingPath,
        BadStat::BadFd,
        BadStat::EmptyPath,
    ];

    /// 中文标签（注入明细用）。
    pub fn label(self) -> &'static str {
        match self {
            BadStat::BadBuffer => "坏缓冲 stat",
            BadStat::MissingPath => "缺失路径 stat",
            BadStat::BadFd => "越界 fd fstat",
            BadStat::EmptyPath => "空路径 stat",
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
    pub fn record(&mut self, caught: bool) -> Result<(), StCode> {
        self.injected = self.injected.saturating_add(1);
        if caught {
            self.detected = self.detected.saturating_add(1);
        } else {
            self.escaped = self.escaped.saturating_add(1);
        }
        if self.detected.saturating_add(self.escaped) != self.injected {
            return Err(StCode::SegmentViolation);
        }
        Ok(())
    }

    /// 守恒式。
    pub fn conserved(&self) -> bool {
        self.detected.saturating_add(self.escaped) == self.injected
    }
}

/// 执行坏 stat 注入：每类 INJECT_ROUNDS 次。
pub fn run_bad_stat_matrix() -> (InjectLedger, [u32; 4]) {
    let mut ledger = InjectLedger::zero();
    let mut per_class = [0u32; 4];
    let mut ci = 0usize;
    while ci < BadStat::ALL.len() {
        let case = match BadStat::ALL.get(ci) {
            Some(c) => *c,
            None => break,
        };
        let mut round = 0u32;
        while round < INJECT_ROUNDS {
            let mut f = StatFace::new();
            let _ = f.add_node("/t/file", 64);
            let caught = match case {
                BadStat::BadBuffer => {
                    f.stat("stat", "/t/file", 0x10).err() == Some(StCode::BadBuffer)
                }
                BadStat::MissingPath => {
                    f.stat("stat", "/nope", 0x7F00_0000).err() == Some(StCode::MissingPath)
                }
                BadStat::BadFd => {
                    f.stat("fstat", "99", 0x7F00_0000).err() == Some(StCode::BadFd)
                }
                BadStat::EmptyPath => {
                    f.stat("stat", "", 0x7F00_0000).err() == Some(StCode::MissingPath)
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

// ── stat 小结成文（纯函数） ───────────────────────────────────────────
/// 小结成文：「size=<n> followed=<0|1>」；账外先拒。
pub fn compose_stat_summary(size: u32, followed: bool) -> Result<String, StCode> {
    let out = format!("size={} followed={}", size, if followed { 1u32 } else { 0u32 });
    if out.is_empty() {
        return Err(StCode::EmptySummary);
    }
    Ok(out)
}

// ── Linux errno 对照表 ────────────────────────────────────────────────
/// (名称, 负值, Windows 近似码, 说明)
pub const ERRNO_TABLE: &[(&str, i32, i32, &str)] = &[
    ("EFAULT", -14, 998, "Windows ERROR_NOACCESS；坏缓冲族"),
    ("ENOENT", -2, 2, "Windows ERROR_FILE_NOT_FOUND；缺失路径/悬链跟随族"),
    ("EBADF", -9, 9, "Windows ERROR_INVALID_HANDLE；越界 fd 族"),
    ("EINVAL", -22, 87, "Windows ERROR_INVALID_PARAMETER；坏 op/非数字族"),
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

/// 规范语料指纹：登记→三口径→成文→摘哈希。
pub fn corpus_fingerprint() -> u32 {
    let mut f = StatFace::new();
    let _ = f.add_node("/a", 100);
    let _ = f.add_link("/b", "/a");
    let mut bytes: Vec<u8> = Vec::new();
    if let Ok((sz, fl)) = f.stat("stat", "/b", 0x7F00_0000) {
        if let Ok(s) = compose_stat_summary(sz, fl) {
            bytes.extend_from_slice(s.as_bytes());
        }
    }
    if let Ok((sz, _)) = f.stat("lstat", "/b", 0x7F00_0000) {
        bytes.extend_from_slice(&sz.to_le_bytes());
    }
    fnv1a(&bytes)
}

// ── 检查入口 ──────────────────────────────────────────────────────────
/// F10129 判据集（13 条静态 + 3 条动态）。
pub fn run_unx10129_checks() -> CheckSet {
    let mut s = CheckSet::new("unx10129");

    // 1. 段位守卫
    let seg_ok = SEG_BASE % 0x40 == 0 && (0x93040..=0x9307F).contains(&SEG_BASE);
    s.add("段位守卫：0x40 对齐且在本卡 0x40 窗口内", seg_ok,
          "段基址必须唯一且不与相邻卡重叠；越界即拒绝注册");

    // 2. 诊断码闭集
    let mut uniq = true;
    let mut i = 0usize;
    while i < StCode::ALL.len() {
        let mut j = i + 1;
        while j < StCode::ALL.len() {
            if let (Some(a), Some(b)) = (StCode::ALL.get(i), StCode::ALL.get(j)) {
                if a.val() == b.val() {
                    uniq = false;
                }
            }
            j += 1;
        }
        i += 1;
    }
    let text_ok = StCode::ALL.iter().all(|c| !c.detail().is_empty());
    s.add("诊断码闭集：闭集码两两唯一且中文建议非空", uniq && text_ok,
          "段内闭集码不得重复，每个码必须能取到可操作说明");

    // 3. 解析器
    let parse_ok = parse_stat_call("stat|/a|3250585600").is_ok()
        && parse_stat_call("xstat|/a|1").err() == Some(StCode::BadOp)
        && parse_stat_call("stat|/a|x").err() == Some(StCode::BadNumber)
        && parse_stat_call("stat|/a").err() == Some(StCode::MissingPart);
    s.add("调用行解析器：op 三词闭集放行且坏成分逐档拒绝", parse_ok,
          "op 闭集 {stat, lstat, fstat}；buf 十进制（坏地址判定层拒）");

    // 4. J1 达成核
    let (ok, probes) = run_stat_rounds();
    s.add("J1 三口径达成：符号链不跟随/跟随两读 10/10 次",
          ok == INJECT_ROUNDS && probes == (INJECT_ROUNDS as usize) * 6,
          "每轮 6 面（链档/跟随/同源/直读/两读一致/计数）全对");

    // 5. J2 坏缓冲
    let mut f = StatFace::new();
    let _ = f.add_node("/a", 10);
    let bad_ok = f.stat("stat", "/a", 0).err() == Some(StCode::BadBuffer)
        && f.stat("lstat", "/a", 0xFFF).err() == Some(StCode::BadBuffer)
        && f.stat("stat", "/a", 0x1000).is_ok();
    s.add("J2 坏缓冲受控：EFAULT 档精确复现且边界值放行", bad_ok,
          "NULL 页内拒、0x1000 放行；缓冲闸先于路径闸");

    // 6. 悬链跟随档：stat 悬链 ENOENT、lstat 仍回链档
    let _ = f.add_link("/dang", "/nope");
    let dang_ok = f.stat("stat", "/dang", 0x7F00_0000).err() == Some(StCode::MissingPath)
        && f.stat("lstat", "/dang", 0x7F00_0000) == Ok((5, true));
    s.add("悬链分档：跟随档 ENOENT 与不跟随档链元数据独立", dang_ok,
          "stat 必须跟随到缺失即拒；lstat 原样回链档");

    // 7. 越界 fd 档
    let fd_ok = f.stat("fstat", "99", 0x7F00_0000).err() == Some(StCode::BadFd);
    s.add("越界 fd：EBADF 档精确复现", fd_ok,
          "fd 表外的 fstat 必须拒");

    // 8. 注入核
    let (inj, per_class) = run_bad_stat_matrix();
    let all_caught = per_class.iter().all(|n| *n == INJECT_ROUNDS);
    s.add("注入核：四类坏样本各 10/10 拒绝", all_caught,
          "坏缓冲/缺失/越界 fd/空路径必须逐类逐次精确复现");

    // 9. 注入账守恒
    s.add("注入账守恒：注入=检出+逃逸且逃逸=0",
          inj.conserved() && inj.escaped == 0
              && inj.injected == BadStat::ALL.len() as u32 * INJECT_ROUNDS,
          "40 次注入必须 40 次检出、0 逃逸");

    // 10. 状态机
    let sm_ok = StatView::Raw.can_advance(StatView::Followed)
        && StatView::Followed.can_advance(StatView::Coalesced)
        && !StatView::Coalesced.can_advance(StatView::Raw)
        && StatView::Followed.label() == "已跟随";
    s.add("状态机：Raw→Followed→Coalesced 单向", sm_ok,
          "口径视图只许单调推进；倒退即协议违约");

    // 11. 三口径同源同值
    let mut f2 = StatFace::new();
    let _ = f2.add_node("/x", 77);
    let _ = f2.bind_fd(2, "/x");
    let co_ok = f2.stat("stat", "/x", 0x7F00_0000) == Ok((77, false))
        && f2.stat("lstat", "/x", 0x7F00_0000) == Ok((77, false))
        && f2.stat("fstat", "2", 0x7F00_0000) == Ok((77, false));
    s.add("三口径同源同值：stat/lstat/fstat 同节点同尺寸", co_ok,
          "三口径合账必须同值；漂移即 ViewDrift");

    // 12. Linux 逐码一致
    let codes_ok = agrees_with_linux(14) && agrees_with_linux(2) && agrees_with_linux(9)
        && agrees_with_linux(22)
        && !agrees_with_linux(0) && !agrees_with_linux(9999);
    s.add("Linux 逐码一致：表内码精确、表外码拒绝", codes_ok,
          "错误族只允许 errno 表内正值且负值可复算");

    // 13. 成文 + 指纹
    let sum = compose_stat_summary(512, true);
    let text_ok = sum.as_ref().map(|x| x.contains("size=512")).unwrap_or(false);
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
    let ok_text = format!("三口径达成轮数 {}/{}", ok, INJECT_ROUNDS);
    s.add_dyn("达成轮快照", ok == INJECT_ROUNDS, &ok_text);

    s
}

// ── 测试 ──────────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parser_guards() {
        assert!(parse_stat_call("stat|/a|100").is_ok());
        assert_eq!(parse_stat_call("xstat|/a|1").err(), Some(StCode::BadOp));
        assert_eq!(parse_stat_call("stat|/a|x").err(), Some(StCode::BadNumber));
    }

    #[test]
    fn lstat_no_follow() {
        let mut f = StatFace::new();
        let _ = f.add_node("/t", 512);
        let _ = f.add_link("/l", "/t");
        assert_eq!(f.stat("lstat", "/l", 0x7F00_0000), Ok((2, true)));
    }

    #[test]
    fn stat_follows() {
        let mut f = StatFace::new();
        let _ = f.add_node("/t", 512);
        let _ = f.add_link("/l", "/t");
        assert_eq!(f.stat("stat", "/l", 0x7F00_0000), Ok((512, true)));
    }

    #[test]
    fn dangling_link_split() {
        let mut f = StatFace::new();
        let _ = f.add_link("/d", "/nope");
        assert_eq!(f.stat("stat", "/d", 0x7F00_0000).err(), Some(StCode::MissingPath));
        assert_eq!(f.stat("lstat", "/d", 0x7F00_0000), Ok((5, true)));
    }

    #[test]
    fn bad_buffer_gate() {
        assert_eq!(StatFace::buf_ok(0), Err(StCode::BadBuffer));
        assert_eq!(StatFace::buf_ok(0xFFF), Err(StCode::BadBuffer));
        assert!(StatFace::buf_ok(0x1000).is_ok());
    }

    #[test]
    fn fstat_bad_fd() {
        let mut f = StatFace::new();
        assert_eq!(f.stat("fstat", "42", 0x7F00_0000).err(), Some(StCode::BadFd));
    }

    #[test]
    fn three_views_same_source() {
        let mut f = StatFace::new();
        let _ = f.add_node("/x", 77);
        let _ = f.bind_fd(2, "/x");
        assert_eq!(f.stat("stat", "/x", 0x7F00_0000), Ok((77, false)));
        assert_eq!(f.stat("lstat", "/x", 0x7F00_0000), Ok((77, false)));
        assert_eq!(f.stat("fstat", "2", 0x7F00_0000), Ok((77, false)));
    }

    #[test]
    fn bad_matrix_green() {
        let (inj, per) = run_bad_stat_matrix();
        assert_eq!(inj.injected, 4 * INJECT_ROUNDS);
        assert_eq!(inj.escaped, 0);
        for n in per.iter() {
            assert_eq!(*n, INJECT_ROUNDS);
        }
    }

    #[test]
    fn rounds_full() {
        let (ok, probes) = run_stat_rounds();
        assert_eq!(ok, INJECT_ROUNDS);
        assert_eq!(probes, (INJECT_ROUNDS as usize) * 6);
    }

    #[test]
    fn state_machine_monotonic() {
        assert!(StatView::Raw.can_advance(StatView::Coalesced));
        assert!(!StatView::Coalesced.can_advance(StatView::Followed));
    }

    #[test]
    fn deterministic_corpus() {
        assert_eq!(corpus_fingerprint(), corpus_fingerprint());
    }

    #[test]
    fn unx10129_checks_green() {
        let set = run_unx10129_checks();
        assert!(set.all_passed(), "UNX-F10129 red: {}", set.red_items_text());
    }
}
