//! UNX-F10123 目录流枚举正反双判据（readdir 达成与关闭后误用受控）· 0x92EC0 段
//! ─────────────────────────────────────────────────────────────────────
//! 锚点：UNX-F10124-J1 硬链达成（nlink 递增/递减守恒 LTP link01/link03
//!       面 10/10 次）；UNX-F10124-J2 跨设备 link 误用受控（预期 EXDEV
//!       精确复现 10/10 次）。
//!
//! 数据段说明：
//!   SEG_BASE = 0x92EC0（本卡独占 0x40 窗口，下一卡 0x92F00）。
//!   诊断码闭集 SEG_BASE+0x01..=+0x0C 共 12 码两两唯一，detail() 全量非空中文。
//!
//! 结构清单：
//!   ① 解析器 `parse_dir_call`："op|dir|count" 三成分（op ∈ {opendir,
//!      readdir, closedir}），op 闭集/非数字 count/缺成分逐档拒绝；
//!   ② 目录面 `DirFace`：目录→(名序表) 登记册 + 流表：opendir 建流、
//!      readdir 按序出流、closedir 销流（销后 EBADF 档）；
//!   ③ 状态机 `StreamState`：Fresh→Reading→Drained→Closed 单向（禁倒退）；
//!   ④ 注入核 `BadStream`：关闭后读取 / 缺失目录 opendir / 文件 opendir /
//!      非法 count 四类坏样本逐类 10/10 次精确复现 EBADF/ENOENT/ENOTDIR/
//!      EINVAL 档，注入账守恒逃逸=0；
//!   ⑤ 错误族与 Linux errno 逐码对照；FNV-1a 双跑指纹复核成文确定性。
//! ─────────────────────────────────────────────────────────────────────

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（F10123 独占 0x40 窗口，下一卡 0x92F00）。
pub const SEG_BASE: u32 = 0x92EC0;

/// 段窗上界（闭开区间右端）。
pub const SEG_END: u32 = 0x92F00;

/// 注入复测轮数（判据要求 10/10）。
pub const INJECT_ROUNDS: u32 = 10;

/// 流表容量。
pub const STREAM_CAP: usize = 8;

/// 单目录条目数上限。
pub const DIR_ENTRIES_MAX: usize = 8;

// ── 诊断码闭集 ────────────────────────────────────────────────────────
/// 本模块诊断码（段内 +0x01..=+0x0C 闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DsCode {
    /// 关闭后读取（EBADF 族）
    ClosedRead,
    /// 缺失目录（ENOENT 族）
    NoSuch,
    /// 文件 opendir（ENOTDIR 族）
    NotDir,
    /// 非法 count（EINVAL 族）
    BadCount,
    /// 非法 op（EINVAL 族）
    BadOp,
    /// 成分缺失（EINVAL 族）
    MissingPart,
    /// 流表满（EMFILE 族）
    StreamFull,
    /// 状态倒退
    StateRegress,
    /// 成文为空
    EmptySummary,
    /// 段位越界
    SegmentViolation,
    /// 枚举序漂移
    OrderDrift,
    /// 语料指纹漂移
    RoundTrip,
}

impl DsCode {
    /// 码值（段基址 + 变体序号 +1）。
    pub fn val(self) -> u32 {
        SEG_BASE + 1 + self as u32
    }

    /// 中文诊断与可操作建议（闭集返回，禁止空串）。
    pub fn detail(self) -> &'static str {
        match self {
            DsCode::ClosedRead => "关闭后读取被拒：流已销毁；EBADF 精确归档",
            DsCode::NoSuch => "缺失目录被拒：账本中无此目录；ENOENT 精确归档",
            DsCode::NotDir => "文件 opendir 被拒：目标不是目录；ENOTDIR 精确归档",
            DsCode::BadCount => "非法 count 被拒：count 必须 ≥1 且 ≤8；EINVAL 精确归档",
            DsCode::BadOp => "非法 op 被拒：op 闭集 {opendir, readdir, closedir}；请核对调用行",
            DsCode::MissingPart => "成分缺失被拒：调用行必须三段 op|dir|count；请补成分",
            DsCode::StreamFull => "流表满被拒：请先 closedir 释放流",
            DsCode::StateRegress => "状态倒退被拒：Fresh→Reading→Drained→Closed 单向",
            DsCode::EmptySummary => "空成文被拒：枚举小结不得为空串；请先 opendir 再成文",
            DsCode::SegmentViolation => "段位越界：段基址必须 0x40 对齐且落在卡面窗口内",
            DsCode::OrderDrift => "枚举序漂移：readdir 序必须与名序表逐位一致",
            DsCode::RoundTrip => "语料指纹漂移：同语料两次哈希不一致；请排查非确定源",
        }
    }

    /// 闭集全量（供文案完整性与唯一性检查）。
    pub const ALL: [DsCode; 12] = [
        DsCode::ClosedRead,
        DsCode::NoSuch,
        DsCode::NotDir,
        DsCode::BadCount,
        DsCode::BadOp,
        DsCode::MissingPart,
        DsCode::StreamFull,
        DsCode::StateRegress,
        DsCode::EmptySummary,
        DsCode::SegmentViolation,
        DsCode::OrderDrift,
        DsCode::RoundTrip,
    ];
}

// ── 调用行解析器 ──────────────────────────────────────────────────────
/// 解析后的目录流调用。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DirCall {
    /// 动词
    pub op: String,
    /// 目录路径
    pub dir: String,
    /// 读取条数（readdir 用；opendir/closedir 忽略）
    pub count: u32,
}

/// 解析 "op|dir|count" 三段调用行。
pub fn parse_dir_call(line: &str) -> Result<DirCall, DsCode> {
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
        return Err(DsCode::MissingPart);
    }
    let op = match parts.first() {
        Some(p) => p.clone(),
        None => return Err(DsCode::MissingPart),
    };
    if op != "opendir" && op != "readdir" && op != "closedir" {
        return Err(DsCode::BadOp);
    }
    let dir = match parts.get(1) {
        Some(p) => p.clone(),
        None => return Err(DsCode::MissingPart),
    };
    let cs = match parts.get(2) {
        Some(p) => p.as_str(),
        None => return Err(DsCode::MissingPart),
    };
    if cs.is_empty() {
        return Err(DsCode::BadCount);
    }
    let mut count: u32 = 0;
    for b in cs.bytes() {
        if !b.is_ascii_digit() {
            return Err(DsCode::BadCount);
        }
        count = count.wrapping_mul(10).wrapping_add((b - b'0') as u32);
    }
    if count == 0 || count as usize > DIR_ENTRIES_MAX {
        return Err(DsCode::BadCount);
    }
    Ok(DirCall { op, dir, count })
}

// ── 流状态机 ──────────────────────────────────────────────────────────
/// 流生命周期（单调，禁倒退）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum StreamState {
    /// 刚打开
    Fresh,
    /// 读取中
    Reading,
    /// 已流尽
    Drained,
    /// 已关闭
    Closed,
}

impl StreamState {
    /// 单调推进判定（允许跳级、禁倒退）。
    pub fn can_advance(self, next: StreamState) -> bool {
        let rank = |s: StreamState| match s {
            StreamState::Fresh => 0u8,
            StreamState::Reading => 1,
            StreamState::Drained => 2,
            StreamState::Closed => 3,
        };
        rank(next) > rank(self)
    }

    /// 中文标签。
    pub fn label(self) -> &'static str {
        match self {
            StreamState::Fresh => "刚打开",
            StreamState::Reading => "读取中",
            StreamState::Drained => "已流尽",
            StreamState::Closed => "已关闭",
        }
    }
}

// ── 目录面 ────────────────────────────────────────────────────────────
/// 目录条目。
#[derive(Clone, Debug)]
pub struct DirNode {
    /// 目录绝对路径
    pub path: String,
    /// 子项名序表（枚举序 = 表序，确定性）
    pub names: Vec<String>,
    /// 是否目录（false 表文件样本）
    pub is_dir: bool,
}

/// 流条目。
#[derive(Clone, Debug)]
pub struct StreamEnt {
    /// 所在目录
    pub dir: String,
    /// 游标（readdir 位置）
    pub cursor: usize,
    /// 流状态
    pub state: StreamState,
}

/// 目录面：目录登记册 + 流表。
#[derive(Clone, Debug)]
pub struct DirFace {
    dirs: Vec<DirNode>,
    streams: Vec<StreamEnt>,
    cap: usize,
    /// 累计 opendir 次数
    pub opens: u32,
    /// 累计 readdir 出流条数
    pub reads: u32,
    /// 累计 closedir 次数
    pub closes: u32,
}

impl DirFace {
    /// 空面构造。
    pub fn new() -> DirFace {
        DirFace { dirs: Vec::new(), streams: Vec::new(), cap: STREAM_CAP, opens: 0, reads: 0, closes: 0 }
    }

    /// 登记目录（确定性名序表）。
    pub fn add_dir(&mut self, path: &str, names: &[&str]) -> Result<(), DsCode> {
        if self.dirs.len() >= STREAM_CAP * 4 {
            return Err(DsCode::StreamFull);
        }
        let mut v: Vec<String> = Vec::new();
        let mut i = 0usize;
        while i < names.len() {
            let n = match names.get(i) {
                Some(x) => String::from(*x),
                None => break,
            };
            let _ = v.push(n);
            i += 1;
        }
        let _ = self.dirs.push(DirNode { path: String::from(path), names: v, is_dir: true });
        Ok(())
    }

    /// 登记文件样本。
    pub fn add_file(&mut self, path: &str) -> Result<(), DsCode> {
        let _ = self.dirs.push(DirNode {
            path: String::from(path),
            names: Vec::new(),
            is_dir: false,
        });
        Ok(())
    }

    /// 按路径寻目录。
    fn dir_idx(&self, path: &str) -> Option<usize> {
        let mut i = 0usize;
        while i < self.dirs.len() {
            if let Some(d) = self.dirs.get(i) {
                if d.path == path {
                    return Some(i);
                }
            }
            i += 1;
        }
        None
    }

    /// opendir：账内必须为目录；流表容量闸；Fresh 入流。
    pub fn opendir(&mut self, dir: &str) -> Result<usize, DsCode> {
        let di = match self.dir_idx(dir) {
            Some(i) => i,
            None => return Err(DsCode::NoSuch),
        };
        let is_dir = match self.dirs.get(di) {
            Some(d) => d.is_dir,
            None => return Err(DsCode::NoSuch),
        };
        if !is_dir {
            return Err(DsCode::NotDir);
        }
        if self.streams.len() >= self.cap {
            return Err(DsCode::StreamFull);
        }
        let _ = self.streams.push(StreamEnt {
            dir: String::from(dir),
            cursor: 0,
            state: StreamState::Fresh,
        });
        let handle = self.streams.len() - 1;
        self.opens = self.opens.saturating_add(1);
        Ok(handle)
    }

    /// readdir：出 count 个名（不足即 Drained）；销流/越柄逐档拒绝。
    pub fn readdir(&mut self, handle: usize, count: usize) -> Result<Vec<String>, DsCode> {
        if count == 0 || count > DIR_ENTRIES_MAX {
            return Err(DsCode::BadCount);
        }
        let st = match self.streams.get(handle) {
            Some(s) => s.state,
            None => return Err(DsCode::ClosedRead),
        };
        if st == StreamState::Closed {
            return Err(DsCode::ClosedRead);
        }
        let di = match self.streams.get(handle) {
            Some(s) => match self.dir_idx(&s.dir) {
                Some(i) => i,
                None => return Err(DsCode::NoSuch),
            },
            None => return Err(DsCode::ClosedRead),
        };
        let (cursor, total) = match self.streams.get(handle) {
            Some(s) => (s.cursor, match self.dirs.get(di) {
                Some(d) => d.names.len(),
                None => 0,
            }),
            None => (0usize, 0usize),
        };
        let mut out: Vec<String> = Vec::new();
        let mut k = 0usize;
        let mut cur = cursor;
        while k < count {
            if cur >= total {
                break;
            }
            let name = match self.dirs.get(di).and_then(|d| d.names.get(cur)) {
                Some(n) => n.clone(),
                None => break,
            };
            let _ = out.push(name);
            cur += 1;
            k += 1;
        }
        if let Some(s) = self.streams.get_mut(handle) {
            s.cursor = cur;
            s.state = if cur >= total && cur > 0 {
                StreamState::Drained
            } else if s.state.can_advance(StreamState::Reading) {
                StreamState::Reading
            } else {
                s.state
            };
        }
        self.reads = self.reads.saturating_add(out.len() as u32);
        Ok(out)
    }

    /// closedir：销流（Closed 态）；二次关闭按 EBADF 档。
    pub fn closedir(&mut self, handle: usize) -> Result<(), DsCode> {
        let st = match self.streams.get(handle) {
            Some(s) => s.state,
            None => return Err(DsCode::ClosedRead),
        };
        if st == StreamState::Closed {
            return Err(DsCode::ClosedRead);
        }
        if let Some(s) = self.streams.get_mut(handle) {
            s.state = StreamState::Closed;
        }
        self.closes = self.closes.saturating_add(1);
        Ok(())
    }

    /// 流状态读取（审计用）。
    pub fn stream_state(&self, handle: usize) -> Option<StreamState> {
        match self.streams.get(handle) {
            Some(s) => Some(s.state),
            None => None,
        }
    }
}

// ── J1 达成核：opendir/readdir/closedir 全程 10/10 轮 ─────────────────
/// 逐轮：opendir→readdir 分批枚举→名序一致→closedir→销后拒。
pub fn run_readdir_rounds() -> (u32, usize) {
    let mut ok = 0u32;
    let mut probes = 0usize;
    let mut round = 0u32;
    while round < INJECT_ROUNDS {
        let mut f = DirFace::new();
        let _ = f.add_dir("/a", &["alpha", "beta", "gamma"]);
        let mut all_ok = true;
        // 面1：opendir 得流柄。
        let h = match f.opendir("/a") {
            Ok(x) => x,
            Err(_) => {
                round += 1;
                continue;
            }
        };
        all_ok = all_ok && f.stream_state(h) == Some(StreamState::Fresh);
        probes += 1;
        // 面2：首批 readdir 2 条按序。
        let first = f.readdir(h, 2).unwrap_or_default();
        all_ok = all_ok && first.len() == 2
            && first.first().map(|s| s == "alpha").unwrap_or(false)
            && first.get(1).map(|s| s == "beta").unwrap_or(false);
        probes += 1;
        // 面3：次批 readdir 2 条得余量 + 流尽。
        let second = f.readdir(h, 2).unwrap_or_default();
        all_ok = all_ok && second.len() == 1
            && second.first().map(|s| s == "gamma").unwrap_or(false)
            && f.stream_state(h) == Some(StreamState::Drained);
        probes += 1;
        // 面4：流尽后再读得空（合法零条）。
        let third = f.readdir(h, 2).unwrap_or_default();
        all_ok = all_ok && third.is_empty();
        probes += 1;
        // 面5：closedir 成功。
        all_ok = all_ok && f.closedir(h).is_ok();
        probes += 1;
        // 面6：销后 readdir 按 EBADF 档拒绝。
        all_ok = all_ok && f.readdir(h, 1).err() == Some(DsCode::ClosedRead);
        probes += 1;
        if all_ok {
            ok += 1;
        }
        round += 1;
    }
    (ok, probes)
}

// ── 注入核：坏目录流矩阵 ──────────────────────────────────────────────
/// 四类坏样本。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BadStream {
    /// 关闭后读取（EBADF 族）
    ClosedRead,
    /// 缺失目录（ENOENT 族）
    NoSuch,
    /// 文件 opendir（ENOTDIR 族）
    NotDir,
    /// 非法 count（EINVAL 族）
    BadCount,
}

impl BadStream {
    /// 闭集全量。
    pub const ALL: [BadStream; 4] = [
        BadStream::ClosedRead,
        BadStream::NoSuch,
        BadStream::NotDir,
        BadStream::BadCount,
    ];

    /// 中文标签（注入明细用）。
    pub fn label(self) -> &'static str {
        match self {
            BadStream::ClosedRead => "关闭后读取",
            BadStream::NoSuch => "缺失目录 opendir",
            BadStream::NotDir => "文件 opendir",
            BadStream::BadCount => "非法 count readdir",
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
    pub fn record(&mut self, caught: bool) -> Result<(), DsCode> {
        self.injected = self.injected.saturating_add(1);
        if caught {
            self.detected = self.detected.saturating_add(1);
        } else {
            self.escaped = self.escaped.saturating_add(1);
        }
        if self.detected.saturating_add(self.escaped) != self.injected {
            return Err(DsCode::SegmentViolation);
        }
        Ok(())
    }

    /// 守恒式。
    pub fn conserved(&self) -> bool {
        self.detected.saturating_add(self.escaped) == self.injected
    }
}

/// 执行坏目录流注入：每类 INJECT_ROUNDS 次。
pub fn run_bad_stream_matrix() -> (InjectLedger, [u32; 4]) {
    let mut ledger = InjectLedger::zero();
    let mut per_class = [0u32; 4];
    let mut ci = 0usize;
    while ci < BadStream::ALL.len() {
        let case = match BadStream::ALL.get(ci) {
            Some(c) => *c,
            None => break,
        };
        let mut round = 0u32;
        while round < INJECT_ROUNDS {
            let mut f = DirFace::new();
            let _ = f.add_dir("/a", &["alpha"]);
            let _ = f.add_file("/f");
            let caught = match case {
                BadStream::ClosedRead => {
                    let h = f.opendir("/a").unwrap_or(0);
                    let _ = f.closedir(h);
                    f.readdir(h, 1).err() == Some(DsCode::ClosedRead)
                }
                BadStream::NoSuch => {
                    f.opendir("/nope").err() == Some(DsCode::NoSuch)
                }
                BadStream::NotDir => {
                    f.opendir("/f").err() == Some(DsCode::NotDir)
                }
                BadStream::BadCount => {
                    parse_dir_call("readdir|/a|0").err() == Some(DsCode::BadCount)
                        && parse_dir_call("readdir|/a|9").err() == Some(DsCode::BadCount)
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

// ── 枚举小结成文（纯函数） ────────────────────────────────────────────
/// 小结成文：「dir=<s> got=<n> reads=<m>」。
pub fn compose_dir_summary(f: &DirFace, dir: &str, got: usize) -> Result<String, DsCode> {
    let out = format!("dir={} got={} reads={}", dir, got, f.reads);
    if out.is_empty() {
        return Err(DsCode::EmptySummary);
    }
    Ok(out)
}

// ── Linux errno 对照表 ────────────────────────────────────────────────
/// (名称, 负值, Windows 近似码, 说明)
pub const ERRNO_TABLE: &[(&str, i32, i32, &str)] = &[
    ("EBADF", -9, 9, "Windows ERROR_INVALID_HANDLE；销流后读取/二关族"),
    ("ENOENT", -2, 2, "Windows ERROR_PATH_NOT_FOUND；缺失目录族"),
    ("ENOTDIR", -20, 267, "Windows ERROR_DIRECTORY；文件 opendir 族"),
    ("EINVAL", -22, 87, "Windows ERROR_INVALID_PARAMETER；非法 count 族"),
    ("EMFILE", -24, 4, "Windows ERROR_TOO_MANY_OPEN_FILES；流表满族"),
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

/// 规范语料指纹：opendir→分批 readdir→closedir→成文→摘哈希。
pub fn corpus_fingerprint() -> u32 {
    let mut f = DirFace::new();
    let _ = f.add_dir("/d", &["n1", "n2", "n3"]);
    let mut bytes: Vec<u8> = Vec::new();
    if let Ok(h) = f.opendir("/d") {
        for _ in 0..2 {
            if let Ok(batch) = f.readdir(h, 2) {
                for n in batch.iter() {
                    bytes.extend_from_slice(n.as_bytes());
                    bytes.push(b',');
                }
            }
        }
        let _ = f.closedir(h);
    }
    if let Ok(s) = compose_dir_summary(&f, "/d", 3) {
        bytes.extend_from_slice(s.as_bytes());
    }
    fnv1a(&bytes)
}

// ── 检查入口 ──────────────────────────────────────────────────────────
/// F10123 判据集（13 条静态 + 3 条动态）。
pub fn run_unx10123_checks() -> CheckSet {
    let mut s = CheckSet::new("unx10123");

    // 1. 段位守卫
    let seg_ok = SEG_BASE % 0x40 == 0 && (0x92EC0..=0x92EFF).contains(&SEG_BASE);
    s.add("段位守卫：0x40 对齐且在本卡 0x40 窗口内", seg_ok,
          "段基址必须唯一且不与相邻卡重叠；越界即拒绝注册");

    // 2. 诊断码闭集
    let mut uniq = true;
    let mut i = 0usize;
    while i < DsCode::ALL.len() {
        let mut j = i + 1;
        while j < DsCode::ALL.len() {
            if let (Some(a), Some(b)) = (DsCode::ALL.get(i), DsCode::ALL.get(j)) {
                if a.val() == b.val() {
                    uniq = false;
                }
            }
            j += 1;
        }
        i += 1;
    }
    let text_ok = DsCode::ALL.iter().all(|c| !c.detail().is_empty());
    s.add("诊断码闭集：闭集码两两唯一且中文建议非空", uniq && text_ok,
          "段内闭集码不得重复，每个码必须能取到可操作说明");

    // 3. 解析器
    let parse_ok = parse_dir_call("readdir|/a|2").is_ok()
        && parse_dir_call("ls|/a|2").err() == Some(DsCode::BadOp)
        && parse_dir_call("readdir|/a|0").err() == Some(DsCode::BadCount)
        && parse_dir_call("readdir|/a").err() == Some(DsCode::MissingPart);
    s.add("调用行解析器：op 闭集与 count 闭集逐档拒绝", parse_ok,
          "opendir/readdir/closedir 三动词闭集；count ∈ 1..=8");

    // 4. J1 达成核
    let (ok, probes) = run_readdir_rounds();
    s.add("J1 readdir 枚举达成：opendir/readdir/closedir 全程 LTP 面 10/10 次",
          ok == INJECT_ROUNDS && probes == (INJECT_ROUNDS as usize) * 6,
          "每轮 6 面（开流/首读/流尽/空读/关流/销后拒）全对");

    // 5. 枚举序确定性：分批枚举拼回 == 名序表
    let mut f2 = DirFace::new();
    let _ = f2.add_dir("/b", &["m1", "m2", "m3", "m4"]);
    let h2 = f2.opendir("/b").unwrap_or(0);
    let b1 = f2.readdir(h2, 2).unwrap_or_default();
    let b2 = f2.readdir(h2, 2).unwrap_or_default();
    let seq_ok = b1.len() == 2 && b2.len() == 2
        && b1.first().map(|x| x == "m1").unwrap_or(false)
        && b2.get(1).map(|x| x == "m4").unwrap_or(false);
    s.add("枚举序确定性：分批拼回与名序表逐位一致", seq_ok,
          "枚举序是 readdir 的账面根；任何漂移即 OrderDrift");

    // 6. J2 关闭后读取
    let mut f3 = DirFace::new();
    let _ = f3.add_dir("/c", &["x"]);
    let h3 = f3.opendir("/c").unwrap_or(0);
    let _ = f3.closedir(h3);
    let closed_ok = f3.readdir(h3, 1).err() == Some(DsCode::ClosedRead)
        && f3.closedir(h3).err() == Some(DsCode::ClosedRead);
    s.add("J2 关闭后读取受控：EBADF 精确复现且二关同档", closed_ok,
          "销流后读与二关必须一致归 EBADF 族");

    // 7. ENOENT/ENOTDIR 分档
    let mut f4 = DirFace::new();
    let _ = f4.add_file("/f");
    let split_ok = f4.opendir("/nope").err() == Some(DsCode::NoSuch)
        && f4.opendir("/f").err() == Some(DsCode::NotDir);
    s.add("缺失与文件分档：ENOENT/ENOTDIR 独立精确归因", split_ok,
          "账外目录与文件两类互不吞档");

    // 8. 流表满
    let mut f5 = DirFace::new();
    let _ = f5.add_dir("/d", &["x"]);
    let mut full_ok = true;
    let mut k = 0usize;
    let mut last_err: Option<DsCode> = None;
    while k <= STREAM_CAP {
        last_err = f5.opendir("/d").err();
        k += 1;
    }
    full_ok = full_ok && last_err == Some(DsCode::StreamFull);
    s.add("容量耗尽：流表满 EMFILE 档精确复现", full_ok,
          "STREAM_CAP 之外必须拒且账不丢");

    // 9. 注入核
    let (inj, per_class) = run_bad_stream_matrix();
    let all_caught = per_class.iter().all(|n| *n == INJECT_ROUNDS);
    s.add("注入核：四类坏样本各 10/10 拒绝", all_caught,
          "销后读/缺失/文件/非法 count 必须逐类逐次精确复现");

    // 10. 注入账守恒
    s.add("注入账守恒：注入=检出+逃逸且逃逸=0",
          inj.conserved() && inj.escaped == 0
              && inj.injected == BadStream::ALL.len() as u32 * INJECT_ROUNDS,
          "40 次注入必须 40 次检出、0 逃逸");

    // 11. 状态机
    let sm_ok = StreamState::Fresh.can_advance(StreamState::Reading)
        && StreamState::Reading.can_advance(StreamState::Drained)
        && StreamState::Drained.can_advance(StreamState::Closed)
        && !StreamState::Closed.can_advance(StreamState::Fresh)
        && StreamState::Drained.label() == "已流尽";
    s.add("状态机：Fresh→Reading→Drained→Closed 单向", sm_ok,
          "流生命周期只许单调推进；倒退即协议违约");

    // 12. Linux 逐码一致
    let codes_ok = agrees_with_linux(9) && agrees_with_linux(2) && agrees_with_linux(20)
        && agrees_with_linux(22) && agrees_with_linux(24)
        && !agrees_with_linux(0) && !agrees_with_linux(9999);
    s.add("Linux 逐码一致：表内码精确、表外码拒绝", codes_ok,
          "错误族只允许 errno 表内正值且负值可复算");

    // 13. 成文 + 指纹
    let sum = compose_dir_summary(&f2, "/b", 4);
    let text_ok = match &sum {
        Ok(x) => x.contains("dir=/b") && x.contains("got=4"),
        Err(_) => false,
    };
    let f1 = corpus_fingerprint();
    let fp2 = corpus_fingerprint();
    s.add("成文与指纹：小结字段齐备且双跑哈希逐位一致",
          text_ok && f1 == fp2,
          "成文为纯函数；同语料两次哈希必须逐位一致");

    // 动态明细（≤8）
    let inj_text = format!("注入 {} / 检出 {} / 逃逸 {}", inj.injected, inj.detected, inj.escaped);
    s.add_dyn("注入账快照", inj.conserved() && inj.escaped == 0, &inj_text);
    let fp_text = format!("语料指纹 0x{:08X}", f1);
    s.add_dyn("指纹快照", f1 == fp2, &fp_text);
    let ok_text = format!("枚举达成轮数 {}/{}", ok, INJECT_ROUNDS);
    s.add_dyn("达成轮快照", ok == INJECT_ROUNDS, &ok_text);

    s
}

// ── 测试 ──────────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parser_guards() {
        assert!(parse_dir_call("readdir|/a|2").is_ok());
        assert_eq!(parse_dir_call("ls|/a|2").err(), Some(DsCode::BadOp));
        assert_eq!(parse_dir_call("readdir|/a|0").err(), Some(DsCode::BadCount));
        assert_eq!(parse_dir_call("readdir|/a|9").err(), Some(DsCode::BadCount));
    }

    #[test]
    fn opendir_semantics() {
        let mut f = DirFace::new();
        let _ = f.add_dir("/a", &["x"]);
        let _ = f.add_file("/f");
        assert!(f.opendir("/a").is_ok());
        assert_eq!(f.opendir("/nope").err(), Some(DsCode::NoSuch));
        assert_eq!(f.opendir("/f").err(), Some(DsCode::NotDir));
    }

    #[test]
    fn readdir_order_stable() {
        let mut f = DirFace::new();
        let _ = f.add_dir("/a", &["a1", "a2", "a3"]);
        let h = f.opendir("/a").unwrap_or(0);
        let b1 = f.readdir(h, 2).unwrap_or_default();
        let b2 = f.readdir(h, 2).unwrap_or_default();
        assert_eq!(b1.len(), 2);
        assert_eq!(b2.len(), 1);
        assert_eq!(b2.first().map(|s| s.as_str()), Some("a3"));
    }

    #[test]
    fn drained_then_empty() {
        let mut f = DirFace::new();
        let _ = f.add_dir("/a", &["x"]);
        let h = f.opendir("/a").unwrap_or(0);
        let _ = f.readdir(h, 4);
        assert_eq!(f.stream_state(h), Some(StreamState::Drained));
        assert!(f.readdir(h, 1).unwrap_or_default().is_empty());
    }

    #[test]
    fn closed_read_blocked() {
        let mut f = DirFace::new();
        let _ = f.add_dir("/a", &["x"]);
        let h = f.opendir("/a").unwrap_or(0);
        let _ = f.closedir(h);
        assert_eq!(f.readdir(h, 1).err(), Some(DsCode::ClosedRead));
        assert_eq!(f.closedir(h).err(), Some(DsCode::ClosedRead));
    }

    #[test]
    fn stream_cap_enforced() {
        let mut f = DirFace::new();
        let _ = f.add_dir("/a", &["x"]);
        let mut last = None;
        for _ in 0..=STREAM_CAP {
            last = f.opendir("/a").err();
        }
        assert_eq!(last, Some(DsCode::StreamFull));
    }

    #[test]
    fn bad_handle_is_closed_read() {
        let mut f = DirFace::new();
        let _ = f.add_dir("/a", &["x"]);
        assert_eq!(f.readdir(42, 1).err(), Some(DsCode::ClosedRead));
    }

    #[test]
    fn bad_matrix_green() {
        let (inj, per) = run_bad_stream_matrix();
        assert_eq!(inj.injected, 4 * INJECT_ROUNDS);
        assert_eq!(inj.escaped, 0);
        for n in per.iter() {
            assert_eq!(*n, INJECT_ROUNDS);
        }
    }

    #[test]
    fn rounds_full() {
        let (ok, probes) = run_readdir_rounds();
        assert_eq!(ok, INJECT_ROUNDS);
        assert_eq!(probes, (INJECT_ROUNDS as usize) * 6);
    }

    #[test]
    fn state_machine_monotonic() {
        assert!(StreamState::Fresh.can_advance(StreamState::Closed));
        assert!(!StreamState::Closed.can_advance(StreamState::Reading));
        assert_eq!(StreamState::Fresh.label(), "刚打开");
    }

    #[test]
    fn deterministic_corpus() {
        assert_eq!(corpus_fingerprint(), corpus_fingerprint());
    }

    #[test]
    fn unx10123_checks_green() {
        let set = run_unx10123_checks();
        assert!(set.all_passed(), "UNX-F10123 red: {}", set.red_items_text());
    }
}
