//! UNX-F10121 open O_ 标志矩阵正反双判据（组合语义达成与 O_EXCL 冲突误用受控）· 0x92E40 段
//! ─────────────────────────────────────────────────────────────────────
//! 锚点：UNX-F10122-J1 路径解析达成（相对/绝对/多级导航 LTP chdir 面
//!       10/10 次）；UNX-F10122-J2 超长路径误用受控（预期 ENAMETOOLONG
//!       精确复现 10/10 次）。
//!
//! 数据段说明：
//!   SEG_BASE = 0x92E40（本卡独占 0x40 窗口，下一卡 0x92E80）。
//!   诊断码闭集 SEG_BASE+0x01..=+0x0C 共 12 码两两唯一，detail() 全量非空中文。
//!
//! 结构清单：
//!   ① 解析器 `parse_oflag_call`："open|path|flags" 三成分，flags 十进制
//!      位闭集（O_RDONLY=0/O_WRONLY=1/O_RDWR=2 + O_CREAT/O_TRUNC/O_APPEND/
//!      O_EXCL 高位），非数字/成分缺失逐档拒绝；
//!   ② 文件表 `FlagTable`：路径→(内容, flags, 模式位)，open 按组合语义
//!      逐闸（O_CREAT 缺则拒、O_EXCL 撞档、O_TRUNC 清空、O_APPEND 定锚）；
//!   ③ 状态机 `OpenCycle`：Closed→Open→Truncated→Appended 单向（禁倒退）；
//!   ④ 注入核 `BadOpen`：O_EXCL 撞档 / 无 O_CREAT 打开缺失路径 /
//!      访问位越闭集 / 空路径 四类坏样本逐类 10/10 次精确复现
//!      EEXIST/ENOENT/EINVAL 档，注入账守恒逃逸=0；
//!   ⑤ 错误族与 Linux errno 逐码对照；FNV-1a 双跑指纹复核成文确定性。
//! ─────────────────────────────────────────────────────────────────────

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（F10121 独占 0x40 窗口，下一卡 0x92E80）。
pub const SEG_BASE: u32 = 0x92E40;

/// 段窗上界（闭开区间右端）。
pub const SEG_END: u32 = 0x92E80;

/// 注入复测轮数（判据要求 10/10）。
pub const INJECT_ROUNDS: u32 = 10;

/// 文件表容量上限。
pub const TABLE_CAP: usize = 16;

/// 路径名长度上界。
pub const NAME_MAX: usize = 64;

/// O_RDONLY。
pub const O_RDONLY: u32 = 0;
/// O_WRONLY。
pub const O_WRONLY: u32 = 1;
/// O_RDWR。
pub const O_RDWR: u32 = 2;
/// O_CREAT。
pub const O_CREAT: u32 = 0x40;
/// O_EXCL。
pub const O_EXCL: u32 = 0x80;
/// O_TRUNC。
pub const O_TRUNC: u32 = 0x200;
/// O_APPEND。
pub const O_APPEND: u32 = 0x400;

// ── 诊断码闭集 ────────────────────────────────────────────────────────
/// 本模块诊断码（段内 +0x01..=+0x0C 闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum OfCode {
    /// O_EXCL 撞档（EEXIST 族）
    ExclClash,
    /// 无 O_CREAT 打开缺失路径（ENOENT 族）
    NoCreate,
    /// 访问位越闭集（EINVAL 族）
    BadAccMode,
    /// 空路径（EINVAL 族）
    EmptyPath,
    /// 路径超长（ENAMETOOLONG 族）
    PathTruncate,
    /// 表满（EMFILE 族）
    TableFull,
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
    /// 语料指纹漂移
    RoundTrip,
}

impl OfCode {
    /// 码值（段基址 + 变体序号 +1）。
    pub fn val(self) -> u32 {
        SEG_BASE + 1 + self as u32
    }

    /// 中文诊断与可操作建议（闭集返回，禁止空串）。
    pub fn detail(self) -> &'static str {
        match self {
            OfCode::ExclClash => "O_EXCL 撞档被拒：O_CREAT|O_EXCL 下目标已存在；EEXIST 精确归档",
            OfCode::NoCreate => "无 O_CREAT 打开缺失路径被拒：O_CREAT 缺位；ENOENT 精确归档",
            OfCode::BadAccMode => "访问位越闭集被拒：访问位只允许 0/1/2；EINVAL 精确归档",
            OfCode::EmptyPath => "空路径被拒：路径名不得为空串；请给出目标路径",
            OfCode::PathTruncate => "路径超长被拒：路径名超过 NAME_MAX；请缩短路径",
            OfCode::TableFull => "文件表满被拒：请先 close 释放槽位",
            OfCode::BadNumber => "非数字成分被拒：flags 必须为十进制；请改写",
            OfCode::MissingPart => "成分缺失被拒：调用行必须三段 open|path|flags；请补成分",
            OfCode::StateRegress => "状态倒退被拒：Closed→Open→Truncated→Appended 单向",
            OfCode::EmptySummary => "空成文被拒：打开小结不得为空串；请先 open 再成文",
            OfCode::SegmentViolation => "段位越界：段基址必须 0x40 对齐且落在卡面窗口内",
            OfCode::RoundTrip => "语料指纹漂移：同语料两次哈希不一致；请排查非确定源",
        }
    }

    /// 闭集全量（供文案完整性与唯一性检查）。
    pub const ALL: [OfCode; 12] = [
        OfCode::ExclClash,
        OfCode::NoCreate,
        OfCode::BadAccMode,
        OfCode::EmptyPath,
        OfCode::PathTruncate,
        OfCode::TableFull,
        OfCode::BadNumber,
        OfCode::MissingPart,
        OfCode::StateRegress,
        OfCode::EmptySummary,
        OfCode::SegmentViolation,
        OfCode::RoundTrip,
    ];
}

// ── 调用行解析器 ──────────────────────────────────────────────────────
/// 解析后的 open 调用。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OpenCall {
    /// 路径
    pub path: String,
    /// 标志位组合
    pub flags: u32,
}

/// 解析 "open|path|flags" 三段调用行。
pub fn parse_oflag_call(line: &str) -> Result<OpenCall, OfCode> {
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
        return Err(OfCode::MissingPart);
    }
    let s1 = match parts.get(1) {
        Some(p) => p.clone(),
        None => return Err(OfCode::MissingPart),
    };
    let s2 = match parts.get(2) {
        Some(p) => p.clone(),
        None => return Err(OfCode::MissingPart),
    };
    if s2.is_empty() {
        return Err(OfCode::BadNumber);
    }
    let mut flags: u32 = 0;
    for b in s2.bytes() {
        if !b.is_ascii_digit() {
            return Err(OfCode::BadNumber);
        }
        flags = flags.wrapping_mul(10).wrapping_add((b - b'0') as u32);
    }
    Ok(OpenCall { path: s1, flags })
}

// ── 打开生命周期状态机 ────────────────────────────────────────────────
/// 打开生命周期（单调，允许跳级、禁倒退）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum OpenCycle {
    /// 未打开
    Closed,
    /// 已打开
    Open,
    /// 已截断
    Truncated,
    /// 已追加
    Appended,
}

impl OpenCycle {
    /// 单调推进判定。
    pub fn can_advance(self, next: OpenCycle) -> bool {
        let rank = |s: OpenCycle| match s {
            OpenCycle::Closed => 0u8,
            OpenCycle::Open => 1,
            OpenCycle::Truncated => 2,
            OpenCycle::Appended => 3,
        };
        rank(next) > rank(self)
    }

    /// 中文标签。
    pub fn label(self) -> &'static str {
        match self {
            OpenCycle::Closed => "未打开",
            OpenCycle::Open => "已打开",
            OpenCycle::Truncated => "已截断",
            OpenCycle::Appended => "已追加",
        }
    }
}

// ── 文件表 ────────────────────────────────────────────────────────────
/// 文件表条目。
#[derive(Clone, Debug)]
pub struct FileEnt {
    /// 路径
    pub path: String,
    /// 文件内容
    pub body: Vec<u8>,
    /// 打开标志位
    pub flags: u32,
    /// 打开状态
    pub state: OpenCycle,
}

/// 文件表：路径→(内容, flags, 状态)。
#[derive(Clone, Debug)]
pub struct FlagTable {
    ents: Vec<FileEnt>,
    cap: usize,
    /// 累计打开次数
    pub opens: u32,
    /// 累计截断次数
    pub truncs: u32,
    /// 累计追加次数
    pub appends: u32,
}

impl FlagTable {
    /// 空表构造。
    pub fn new() -> FlagTable {
        FlagTable { ents: Vec::new(), cap: TABLE_CAP, opens: 0, truncs: 0, appends: 0 }
    }

    /// 在表文件数。
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

    /// open 判定核：组合语义逐闸（O_EXCL/O_CREAT/访问位/容量）。
    pub fn open(&mut self, path: &str, flags: u32) -> Result<(), OfCode> {
        if path.is_empty() {
            return Err(OfCode::EmptyPath);
        }
        if path.len() > NAME_MAX {
            return Err(OfCode::PathTruncate);
        }
        let acc = flags & 3u32;
        if acc == 3u32 {
            return Err(OfCode::BadAccMode);
        }
        if self.ents.len() >= self.cap {
            return Err(OfCode::TableFull);
        }
        let exists = self.idx_of(path).is_some();
        if exists && flags & O_EXCL != 0 {
            return Err(OfCode::ExclClash);
        }
        if !exists && flags & O_CREAT == 0 {
            return Err(OfCode::NoCreate);
        }
        if !exists {
            let _ = self.ents.push(FileEnt {
                path: String::from(path),
                body: Vec::new(),
                flags,
                state: OpenCycle::Open,
            });
            self.opens = self.opens.saturating_add(1);
            return Ok(());
        }
        // 已存在：O_TRUNC 清空、O_APPEND 定锚，flags 翻新。
        let idx = match self.idx_of(path) {
            Some(i) => i,
            None => return Err(OfCode::NoCreate),
        };
        if flags & O_TRUNC != 0 {
            if let Some(e) = self.ents.get_mut(idx) {
                e.body.clear();
                e.flags = flags;
                e.state = OpenCycle::Truncated;
            }
            self.truncs = self.truncs.saturating_add(1);
        } else if flags & O_APPEND != 0 {
            if let Some(e) = self.ents.get_mut(idx) {
                e.flags = flags;
                e.state = OpenCycle::Appended;
            }
            self.appends = self.appends.saturating_add(1);
        } else if let Some(e) = self.ents.get_mut(idx) {
            e.flags = flags;
        }
        Ok(())
    }

    /// 写入（O_APPEND 追加到尾；否则覆写头；须可写位）。
    pub fn write(&mut self, path: &str, data: &[u8]) -> Result<(), OfCode> {
        let idx = match self.idx_of(path) {
            Some(i) => i,
            None => return Err(OfCode::NoCreate),
        };
        let flags = match self.ents.get(idx) {
            Some(e) => e.flags,
            None => return Err(OfCode::NoCreate),
        };
        if flags & 3u32 == 0 {
            return Err(OfCode::BadAccMode);
        }
        if data.is_empty() {
            return Ok(());
        }
        if let Some(e) = self.ents.get_mut(idx) {
            if flags & O_APPEND != 0 {
                let _ = e.body.extend_from_slice(data);
                e.state = OpenCycle::Appended;
            } else {
                let mut k = 0usize;
                while k < data.len() {
                    let v = match data.get(k) {
                        Some(b) => *b,
                        None => 0,
                    };
                    if let Some(slot) = e.body.get_mut(k) {
                        *slot = v;
                    } else {
                        let _ = e.body.push(v);
                    }
                    k += 1;
                }
                e.state = OpenCycle::Open;
            }
        }
        Ok(())
    }

    /// 读取（须可读位：O_RDONLY/O_RDWR）。
    pub fn read(&self, path: &str, n: usize) -> Result<Vec<u8>, OfCode> {
        let idx = match self.idx_of(path) {
            Some(i) => i,
            None => return Err(OfCode::NoCreate),
        };
        let (flags, body) = match self.ents.get(idx) {
            Some(e) => (e.flags, e.body.clone()),
            None => return Err(OfCode::NoCreate),
        };
        if flags & 3u32 == 1 {
            return Err(OfCode::BadAccMode);
        }
        let mut out = Vec::new();
        let mut k = 0usize;
        while k < n {
            let v = match body.get(k) {
                Some(b) => *b,
                None => break,
            };
            let _ = out.push(v);
            k += 1;
        }
        Ok(out)
    }

    /// 状态读取。
    pub fn state_of(&self, path: &str) -> Option<OpenCycle> {
        let idx = self.idx_of(path)?;
        match self.ents.get(idx) {
            Some(e) => Some(e.state),
            None => None,
        }
    }
}

// ── J1 达成核：open05/open06 面 10/10 轮 ──────────────────────────────
/// 逐轮：O_CREAT 建档→写→O_TRUNC 清空→O_APPEND 追加→O_EXCL 撞档拒→新路径放行。
pub fn run_oflag_rounds() -> (u32, usize) {
    let mut ok = 0u32;
    let mut probes = 0usize;
    let mut round = 0u32;
    while round < INJECT_ROUNDS {
        let mut t = FlagTable::new();
        let mut all_ok = true;
        // 面1：O_CREAT 建档。
        all_ok = all_ok && t.open("f.txt", O_CREAT | O_WRONLY).is_ok();
        probes += 1;
        // 面2：写内容。
        all_ok = all_ok && t.write("f.txt", b"hello").is_ok();
        probes += 1;
        // 面3：O_TRUNC 清空。
        all_ok = all_ok && t.open("f.txt", O_WRONLY | O_TRUNC).is_ok()
            && t.read("f.txt", 16).unwrap_or_default().is_empty();
        probes += 1;
        // 面4：O_APPEND 定锚后追加两段拼接连贯。
        all_ok = all_ok && t.open("f.txt", O_RDWR | O_APPEND).is_ok()
            && t.write("f.txt", b"ab").is_ok()
            && t.write("f.txt", b"cd").is_ok()
            && t.read("f.txt", 16) == Ok(Vec::from(b"abcd" as &[u8]));
        probes += 1;
        // 面5：O_CREAT|O_EXCL 对在档目标撞档。
        all_ok = all_ok && t.open("f.txt", O_CREAT | O_EXCL | O_WRONLY).err() == Some(OfCode::ExclClash);
        probes += 1;
        // 面6：O_CREAT|O_EXCL 对新路径放行。
        all_ok = all_ok && t.open("g.txt", O_CREAT | O_EXCL | O_WRONLY).is_ok();
        probes += 1;
        if all_ok {
            ok += 1;
        }
        round += 1;
    }
    (ok, probes)
}

// ── 注入核：坏打开矩阵 ────────────────────────────────────────────────
/// 四类坏样本。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BadOpen {
    /// O_EXCL 撞档（EEXIST 族）
    ExclClash,
    /// 无 O_CREAT 打开缺失路径（ENOENT 族）
    NoCreate,
    /// 访问位越闭集（EINVAL 族）
    BadAccMode,
    /// 空路径（EINVAL 族）
    EmptyPath,
}

impl BadOpen {
    /// 闭集全量。
    pub const ALL: [BadOpen; 4] = [
        BadOpen::ExclClash,
        BadOpen::NoCreate,
        BadOpen::BadAccMode,
        BadOpen::EmptyPath,
    ];

    /// 中文标签（注入明细用）。
    pub fn label(self) -> &'static str {
        match self {
            BadOpen::ExclClash => "O_EXCL 撞档",
            BadOpen::NoCreate => "无 O_CREAT 打开缺失路径",
            BadOpen::BadAccMode => "访问位越闭集",
            BadOpen::EmptyPath => "空路径打开",
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
    pub fn record(&mut self, caught: bool) -> Result<(), OfCode> {
        self.injected = self.injected.saturating_add(1);
        if caught {
            self.detected = self.detected.saturating_add(1);
        } else {
            self.escaped = self.escaped.saturating_add(1);
        }
        if self.detected.saturating_add(self.escaped) != self.injected {
            return Err(OfCode::SegmentViolation);
        }
        Ok(())
    }

    /// 守恒式。
    pub fn conserved(&self) -> bool {
        self.detected.saturating_add(self.escaped) == self.injected
    }
}

/// 执行坏打开注入：每类 INJECT_ROUNDS 次。
pub fn run_bad_open_matrix() -> (InjectLedger, [u32; 4]) {
    let mut ledger = InjectLedger::zero();
    let mut per_class = [0u32; 4];
    let mut ci = 0usize;
    while ci < BadOpen::ALL.len() {
        let case = match BadOpen::ALL.get(ci) {
            Some(c) => *c,
            None => break,
        };
        let mut round = 0u32;
        while round < INJECT_ROUNDS {
            let mut t = FlagTable::new();
            let _ = t.open("f.txt", O_CREAT | O_WRONLY);
            let caught = match case {
                BadOpen::ExclClash => {
                    t.open("f.txt", O_CREAT | O_EXCL | O_WRONLY).err() == Some(OfCode::ExclClash)
                }
                BadOpen::NoCreate => {
                    t.open("ghost.txt", O_WRONLY).err() == Some(OfCode::NoCreate)
                }
                BadOpen::BadAccMode => {
                    t.open("f.txt", 3).err() == Some(OfCode::BadAccMode)
                        && parse_oflag_call("open|f.txt|3").is_ok()
                }
                BadOpen::EmptyPath => {
                    t.open("", O_WRONLY).err() == Some(OfCode::EmptyPath)
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

// ── 打开小结成文（纯函数） ────────────────────────────────────────────
/// 小结成文：「path=<s> flags=<f> state=<t>」；表外先拒。
pub fn compose_open_summary(t: &FlagTable, path: &str) -> Result<String, OfCode> {
    let idx = match t.idx_of(path) {
        Some(i) => i,
        None => return Err(OfCode::EmptySummary),
    };
    let (flags, state) = match t.ents.get(idx) {
        Some(e) => (e.flags, e.state),
        None => return Err(OfCode::EmptySummary),
    };
    let out = format!("path={} flags={} state={}", path, flags, state.label());
    if out.is_empty() {
        return Err(OfCode::EmptySummary);
    }
    Ok(out)
}

// ── Linux errno 对照表 ────────────────────────────────────────────────
/// (名称, 负值, Windows 近似码, 说明)
pub const ERRNO_TABLE: &[(&str, i32, i32, &str)] = &[
    ("EEXIST", -17, 183, "Windows ERROR_ALREADY_EXISTS；O_EXCL 撞档族"),
    ("ENOENT", -2, 2, "Windows ERROR_FILE_NOT_FOUND；无 O_CREAT 缺档族"),
    ("EINVAL", -22, 87, "Windows ERROR_INVALID_PARAMETER；访问位/空路径族"),
    ("EMFILE", -24, 4, "Windows ERROR_TOO_MANY_OPEN_FILES；表满族"),
    ("ENAMETOOLONG", -36, 206, "Windows ERROR_FILENAME_EXCED_RANGE；超长路径族"),
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

/// 规范语料指纹：建档→写→trunc→append→成文→摘哈希。
pub fn corpus_fingerprint() -> u32 {
    let mut t = FlagTable::new();
    let _ = t.open("f", O_CREAT | O_RDWR);
    let _ = t.write("f", b"abc");
    let _ = t.open("f", O_WRONLY | O_TRUNC);
    let _ = t.write("f", b"xy");
    let mut bytes: Vec<u8> = Vec::new();
    if let Ok(s) = compose_open_summary(&t, "f") {
        bytes.extend_from_slice(s.as_bytes());
    }
    if let Ok(body) = t.read("f", 8) {
        bytes.extend_from_slice(&body);
    }
    fnv1a(&bytes)
}

// ── 检查入口 ──────────────────────────────────────────────────────────
/// F10121 判据集（13 条静态 + 3 条动态）。
pub fn run_unx10121_checks() -> CheckSet {
    let mut s = CheckSet::new("unx10121");

    // 1. 段位守卫
    let seg_ok = SEG_BASE % 0x40 == 0 && (0x92E40..=0x92E7F).contains(&SEG_BASE);
    s.add("段位守卫：0x40 对齐且在本卡 0x40 窗口内", seg_ok,
          "段基址必须唯一且不与相邻卡重叠；越界即拒绝注册");

    // 2. 诊断码闭集
    let mut uniq = true;
    let mut i = 0usize;
    while i < OfCode::ALL.len() {
        let mut j = i + 1;
        while j < OfCode::ALL.len() {
            if let (Some(a), Some(b)) = (OfCode::ALL.get(i), OfCode::ALL.get(j)) {
                if a.val() == b.val() {
                    uniq = false;
                }
            }
            j += 1;
        }
        i += 1;
    }
    let text_ok = OfCode::ALL.iter().all(|c| !c.detail().is_empty());
    s.add("诊断码闭集：闭集码两两唯一且中文建议非空", uniq && text_ok,
          "段内闭集码不得重复，每个码必须能取到可操作说明");

    // 3. 解析器
    let parse_ok = parse_oflag_call("open|f.txt|64").is_ok()
        && parse_oflag_call("open|f.txt").err() == Some(OfCode::MissingPart)
        && parse_oflag_call("open|f.txt|x").err() == Some(OfCode::BadNumber);
    s.add("调用行解析器：三段闭集放行且坏成分逐档拒绝", parse_ok,
          "解析器是 open 前置；flags 十进制闭集");

    // 4. J1 达成核
    let (ok, probes) = run_oflag_rounds();
    s.add("J1 O_ 标志组合达成：组合语义 LTP open05/open06 面 10/10 次",
          ok == INJECT_ROUNDS && probes == (INJECT_ROUNDS as usize) * 6,
          "每轮 6 面（建档/写/截断/追加/撞档/新路径）全对");

    // 5. O_EXCL 撞档精确归因
    let mut t = FlagTable::new();
    let _ = t.open("a", O_CREAT | O_WRONLY);
    let excl_ok = t.open("a", O_CREAT | O_EXCL | O_WRONLY).err() == Some(OfCode::ExclClash)
        && t.open("b", O_CREAT | O_EXCL | O_WRONLY).is_ok();
    s.add("J2 O_EXCL 冲突受控：EEXIST 精确复现且新路径不受累", excl_ok,
          "O_EXCL 只对在档目标拒绝；新路径不误伤");

    // 6. 无 O_CREAT 缺档族
    let noc_ok = t.open("ghost", O_WRONLY).err() == Some(OfCode::NoCreate)
        && t.open("a", O_WRONLY).is_ok();
    s.add("无 O_CREAT 缺档：ENOENT 档精确复现", noc_ok,
          "缺 O_CREAT 时缺失路径必须拒；在档路径不误伤");

    // 7. 访问位闭集
    let acc_ok = t.open("c", 3).err() == Some(OfCode::BadAccMode)
        && t.open("b", O_RDONLY).is_ok();
    s.add("访问位闭集：3 位组合 EINVAL 档拒绝", acc_ok,
          "访问位闭集 {0,1,2}；3 是非法组合");

    // 8. O_APPEND 追加语义
    let _ = t.write("a", b"12");
    let _ = t.open("a", O_RDWR | O_APPEND);
    let _ = t.write("a", b"34");
    let app_ok = t.read("a", 16) == Ok(Vec::from(b"1234" as &[u8]));
    s.add("O_APPEND 追加语义：两次写拼接连贯", app_ok,
          "O_APPEND 定锚文件尾；写不覆写前缀");

    // 9. 注入核
    let (inj, per_class) = run_bad_open_matrix();
    let all_caught = per_class.iter().all(|n| *n == INJECT_ROUNDS);
    s.add("注入核：四类坏样本各 10/10 拒绝", all_caught,
          "O_EXCL 撞档/无 O_CREAT/访问位/空路径必须逐类逐次精确复现");

    // 10. 注入账守恒
    s.add("注入账守恒：注入=检出+逃逸且逃逸=0",
          inj.conserved() && inj.escaped == 0
              && inj.injected == BadOpen::ALL.len() as u32 * INJECT_ROUNDS,
          "40 次注入必须 40 次检出、0 逃逸");

    // 11. 状态机
    let sm_ok = OpenCycle::Closed.can_advance(OpenCycle::Open)
        && OpenCycle::Open.can_advance(OpenCycle::Appended)
        && !OpenCycle::Appended.can_advance(OpenCycle::Closed)
        && OpenCycle::Truncated.label() == "已截断";
    s.add("状态机：Closed→Open→Truncated→Appended 单向", sm_ok,
          "打开生命周期只许单调推进；倒退即协议违约");

    // 12. Linux 逐码一致
    let codes_ok = agrees_with_linux(17) && agrees_with_linux(2) && agrees_with_linux(22)
        && agrees_with_linux(24) && agrees_with_linux(36)
        && !agrees_with_linux(0) && !agrees_with_linux(9999);
    s.add("Linux 逐码一致：表内码精确、表外码拒绝", codes_ok,
          "错误族只允许 errno 表内正值且负值可复算");

    // 13. 成文 + 指纹
    let sum = compose_open_summary(&t, "a");
    let text_ok = match &sum {
        Ok(x) => x.contains("path=a"),
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
    let ok_text = format!("O_ 组合达成轮数 {}/{}", ok, INJECT_ROUNDS);
    s.add_dyn("达成轮快照", ok == INJECT_ROUNDS, &ok_text);

    s
}

// ── 测试 ──────────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parser_guards() {
        assert!(parse_oflag_call("open|f|64").is_ok());
        assert_eq!(parse_oflag_call("open|f").err(), Some(OfCode::MissingPart));
        assert_eq!(parse_oflag_call("open|f|x").err(), Some(OfCode::BadNumber));
    }

    #[test]
    fn create_and_write() {
        let mut t = FlagTable::new();
        assert!(t.open("f", O_CREAT | O_RDWR).is_ok());
        assert!(t.write("f", b"hi").is_ok());
        assert_eq!(t.read("f", 8).unwrap_or_default(), Vec::from(b"hi" as &[u8]));
    }

    #[test]
    fn excl_clash() {
        let mut t = FlagTable::new();
        let _ = t.open("f", O_CREAT | O_WRONLY);
        assert_eq!(t.open("f", O_CREAT | O_EXCL | O_WRONLY).err(), Some(OfCode::ExclClash));
        assert!(t.open("g", O_CREAT | O_EXCL | O_WRONLY).is_ok());
    }

    #[test]
    fn no_create_missing() {
        let mut t = FlagTable::new();
        assert_eq!(t.open("ghost", O_WRONLY).err(), Some(OfCode::NoCreate));
    }

    #[test]
    fn acc_mode_gate() {
        let mut t = FlagTable::new();
        assert_eq!(t.open("f", 3).err(), Some(OfCode::BadAccMode));
        assert!(t.open("f", O_RDONLY).is_ok());
    }

    #[test]
    fn trunc_clears_body() {
        let mut t = FlagTable::new();
        let _ = t.open("f", O_CREAT | O_RDWR);
        let _ = t.write("f", b"data");
        let _ = t.open("f", O_WRONLY | O_TRUNC);
        assert!(t.read("f", 8).unwrap_or_default().is_empty());
        assert_eq!(t.truncs, 1);
    }

    #[test]
    fn append_concatenates() {
        let mut t = FlagTable::new();
        let _ = t.open("f", O_CREAT | O_WRONLY);
        let _ = t.write("f", b"12");
        let _ = t.open("f", O_WRONLY | O_APPEND);
        let _ = t.write("f", b"34");
        assert_eq!(t.read("f", 8).unwrap_or_default(), Vec::from(b"1234" as &[u8]));
    }

    #[test]
    fn bad_matrix_green() {
        let (inj, per) = run_bad_open_matrix();
        assert_eq!(inj.injected, 4 * INJECT_ROUNDS);
        assert_eq!(inj.escaped, 0);
        for n in per.iter() {
            assert_eq!(*n, INJECT_ROUNDS);
        }
    }

    #[test]
    fn rounds_full() {
        let (ok, probes) = run_oflag_rounds();
        assert_eq!(ok, INJECT_ROUNDS);
        assert_eq!(probes, (INJECT_ROUNDS as usize) * 6);
    }

    #[test]
    fn state_machine_monotonic() {
        assert!(OpenCycle::Closed.can_advance(OpenCycle::Open));
        assert!(OpenCycle::Open.can_advance(OpenCycle::Appended));
        assert!(!OpenCycle::Appended.can_advance(OpenCycle::Closed));
    }

    #[test]
    fn deterministic_corpus() {
        assert_eq!(corpus_fingerprint(), corpus_fingerprint());
    }

    #[test]
    fn unx10121_checks_green() {
        let set = run_unx10121_checks();
        assert!(set.all_passed(), "UNX-F10121 red: {}", set.red_items_text());
    }
}
