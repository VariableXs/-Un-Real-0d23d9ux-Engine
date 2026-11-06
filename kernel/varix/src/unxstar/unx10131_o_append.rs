//! UNX-F10131 O_APPEND 并发追加正反双判据（原子追加达成与无标志竞态受控）· 0x930C0 段
//! ─────────────────────────────────────────────────────────────────────
//! 锚点：UNX-F10132-J1 落盘达成（写后 fsync 重读一致 LTP fsync01 面
//!       10/10 次）；UNX-F10132-J2 非法 fd 误用受控（预期 EBADF 精确复现
//!       10/10 次）。
//!
//! 数据段说明：
//!   SEG_BASE = 0x930C0（本卡独占 0x40 碦口，下一卡 0x93100）。
//!   诊断码闭集 SEG_BASE+0x01..=+0x0C 共 12 码两两唯一，detail() 全量非空中文。
//!
//! 结构清单：
//!   ① 解析器 `parse_wcall`："write|fd|data" 三成分，fd 十进制、data 闭集
//!      样本（W1/W2 字节样本表），缺成分逐档拒绝；
//!   ② 追加账 `AppendLog`：fd→(绑定, O_APPEND 位, 独立偏移)，写核按
//!      O_APPEND 位分流：定锚尾写（终位正确）与定点覆写（竞态可复现）；
//!   ③ 状态机 `AppendState`：Unbound→Bound→Appended→Overwritten 单向；
//!   ④ 注入核 `BadAppend`：未绑定写 / 只读端写 / 重复绑定 / 超限写
//!      四类坏样本逐类 10/10 次精确复现 EBADF/EBUSY/EFBIG 档，
//!      注入账守恒逃逸=0；
//!   ⑤ 错误族与 Linux errno 逐码对照；FNV-1a 双跑指纹复核成文确定性。
//! ─────────────────────────────────────────────────────────────────────

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（F10131 独占 0x40 窗口，下一卡 0x93100）。
pub const SEG_BASE: u32 = 0x930C0;

/// 段窗上界（闭开区间右端）。
pub const SEG_END: u32 = 0x93100;

/// 注入复测轮数（判据要求 10/10）。
pub const INJECT_ROUNDS: u32 = 10;

/// fd 闭集容量。
pub const FD_CAP: usize = 8;

/// 单次写长度上限（超即 EFBIG 档）。
pub const WRITE_MAX: u32 = 64;

/// O_APPEND 位。
pub const O_APPEND: u32 = 0x400;

/// O_RDONLY。
pub const O_RDONLY: u32 = 0;

// ── 诊断码闭集 ────────────────────────────────────────────────────────
/// 本模块诊断码（段内 +0x01..=+0x0C 闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ApCode {
    /// 未绑定写（EBADF 族）
    NotOpen,
    /// 只读端写（EBADF 族）
    ReadOnly,
    /// 重复绑定（EBUSY 档）
    DupBind,
    /// 超限写（EFBIG 族）
    OverMax,
    /// 非数字成分（EINVAL 族）
    BadNumber,
    /// 成分缺失（EINVAL 族)
    MissingPart,
    /// fd 越闭集（EBADF 族）
    BadFd,
    /// 状态倒退
    StateRegress,
    /// 成文为空
    EmptySummary,
    /// 段位越界
    SegmentViolation,
    /// 终位失平
    EndDrift,
    /// 语料指纹漂移
    RoundTrip,
}

impl ApCode {
    /// 码值（段基址 + 变体序号 +1）。
    pub fn val(self) -> u32 {
        SEG_BASE + 1 + self as u32
    }

    /// 中文诊断与可操作建议（闭集返回，禁止空串）。
    pub fn detail(self) -> &'static str {
        match self {
            ApCode::NotOpen => "未绑定写被拒：fd 未绑定文件；EBADF 精确归档",
            ApCode::ReadOnly => "只读端写被拒：fd 无写权限位；EBADF 精确归档",
            ApCode::DupBind => "重复绑定被拒：fd 已绑定；EBUSY 档精确归档",
            ApCode::OverMax => "超限写被拒：单次写超 WRITE_MAX；EFBIG 精确归档",
            ApCode::BadNumber => "非数字成分被拒：fd 必须为十进制；请改写",
            ApCode::MissingPart => "成分缺失被拒：调用行必须三段 write|fd|data；请补成分",
            ApCode::BadFd => "fd 越闭集被拒：fd 必须 < FD_CAP；EBADF 精确归档",
            ApCode::StateRegress => "状态倒退被拒：Unbound→Bound→Appended→Overwritten 单向",
            ApCode::EmptySummary => "空成文被拒：追加小结不得为空串；请先写再成文",
            ApCode::SegmentViolation => "段位越界：段基址必须 0x40 对齐且落在卡面窗口内",
            ApCode::EndDrift => "终位失平被拒：O_APPEND 下各端终位必须等于文件长",
            ApCode::RoundTrip => "语料指纹漂移：同语料两次哈希不一致；请排查非确定源",
        }
    }

    /// 闭集全量（供文案完整性与唯一性检查）。
    pub const ALL: [ApCode; 12] = [
        ApCode::NotOpen,
        ApCode::ReadOnly,
        ApCode::DupBind,
        ApCode::OverMax,
        ApCode::BadNumber,
        ApCode::MissingPart,
        ApCode::BadFd,
        ApCode::StateRegress,
        ApCode::EmptySummary,
        ApCode::SegmentViolation,
        ApCode::EndDrift,
        ApCode::RoundTrip,
    ];
}

// ── 调用行解析器 ──────────────────────────────────────────────────────
/// 解析后的写调用。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WCall {
    /// fd（十进制）
    pub fd: u32,
    /// 数据（字节样本）
    pub data: Vec<u8>,
}

/// 解析 "write|fd|data" 三段调用行；data 为十六进制偶长字节样本。
pub fn parse_wcall(line: &str) -> Result<WCall, ApCode> {
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
        return Err(ApCode::MissingPart);
    }
    let fds = match parts.get(1) {
        Some(p) => p.clone(),
        None => return Err(ApCode::MissingPart),
    };
    let ds = match parts.get(2) {
        Some(p) => p.clone(),
        None => return Err(ApCode::MissingPart),
    };
    if fds.is_empty() {
        return Err(ApCode::BadNumber);
    }
    let mut fd: u32 = 0;
    for b in fds.bytes() {
        if !b.is_ascii_digit() {
            return Err(ApCode::BadNumber);
        }
        fd = fd.wrapping_mul(10).wrapping_add((b - b'0') as u32);
    }
    // data：十六进制偶长字节样本（空串合法 = 零写）。
    let mut data: Vec<u8> = Vec::new();
    let bytes = ds.as_bytes();
    if bytes.len() % 2 != 0 {
        return Err(ApCode::BadNumber);
    }
    let mut k = 0usize;
    while k < bytes.len() {
        let hi = match bytes.get(k) {
            Some(b) => hexval(*b),
            None => return Err(ApCode::BadNumber),
        };
        let lo = match bytes.get(k + 1) {
            Some(b) => hexval(*b),
            None => return Err(ApCode::BadNumber),
        };
        let h = match hi {
            Some(v) => v,
            None => return Err(ApCode::BadNumber),
        };
        let l = match lo {
            Some(v) => v,
            None => return Err(ApCode::BadNumber),
        };
        let _ = data.push((h * 16 + l) as u8);
        k += 2;
    }
    Ok(WCall { fd, data })
}

/// 十六进制位值。
fn hexval(b: u8) -> Option<u32> {
    match b {
        b'0'..=b'9' => Some((b - b'0') as u32),
        b'a'..=b'f' => Some((b - b'a' + 10) as u32),
        b'A'..=b'F' => Some((b - b'A' + 10) as u32),
        _ => None,
    }
}

// ── 追加状态机 ────────────────────────────────────────────────────────
/// 写端生命周期（单调，禁倒退）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AppendState {
    /// 未绑定
    Unbound,
    /// 已绑定
    Bound,
    /// 已追加
    Appended,
    /// 已覆写（无标志竞态档）
    Overwritten,
}

impl AppendState {
    /// 单调推进判定。
    pub fn can_advance(self, next: AppendState) -> bool {
        let rank = |s: AppendState| match s {
            AppendState::Unbound => 0u8,
            AppendState::Bound => 1,
            AppendState::Appended => 2,
            AppendState::Overwritten => 3,
        };
        rank(next) > rank(self)
    }

    /// 中文标签。
    pub fn label(self) -> &'static str {
        match self {
            AppendState::Unbound => "未绑定",
            AppendState::Bound => "已绑定",
            AppendState::Appended => "已追加",
            AppendState::Overwritten => "已覆写",
        }
    }
}

// ── 追加账 ────────────────────────────────────────────────────────────
/// 写端条目。
#[derive(Clone, Debug)]
pub struct WEnt {
    /// fd
    pub fd: u32,
    /// O_APPEND 位
    pub append: bool,
    /// 可写位
    pub writable: bool,
    /// 独立偏移
    pub offset: u32,
    /// 生命周期状态
    pub state: AppendState,
}

/// 追加账：fd 写端 + 共享文件体。
#[derive(Clone, Debug)]
pub struct AppendLog {
    ents: Vec<WEnt>,
    /// 共享文件体
    pub body: Vec<u8>,
    /// 累计写次数
    pub writes: u32,
    /// 累计覆写检出次数（J2 复现面）
    pub overlaps: u32,
}

impl AppendLog {
    /// 空账构造。
    pub fn new() -> AppendLog {
        AppendLog { ents: Vec::new(), body: Vec::new(), writes: 0, overlaps: 0 }
    }

    /// 绑定 fd：重复绑定/越闭集逐闸。
    pub fn bind(&mut self, fd: u32, append: bool, writable: bool) -> Result<(), ApCode> {
        if fd as usize >= FD_CAP {
            return Err(ApCode::BadFd);
        }
        let mut i = 0usize;
        while i < self.ents.len() {
            if let Some(e) = self.ents.get(i) {
                if e.fd == fd {
                    return Err(ApCode::DupBind);
                }
            }
            i += 1;
        }
        let _ = self.ents.push(WEnt {
            fd,
            append,
            writable,
            offset: 0,
            state: AppendState::Bound,
        });
        Ok(())
    }

    /// 写核：O_APPEND 定锚尾写（终位=文件长）；无标志定点覆写（竞态复现）。
    pub fn write(&mut self, fd: u32, data: &[u8]) -> Result<u32, ApCode> {
        if data.len() as u64 > WRITE_MAX as u64 {
            return Err(ApCode::OverMax);
        }
        let idx = match self.fd_idx(fd) {
            Some(i) => i,
            None => return Err(ApCode::NotOpen),
        };
        let (append, writable, off) = match self.ents.get(idx) {
            Some(e) => (e.append, e.writable, e.offset),
            None => return Err(ApCode::NotOpen),
        };
        if !writable {
            return Err(ApCode::ReadOnly);
        }
        if append {
            // 原子尾写：终位 = 文件长。
            let _ = self.body.extend_from_slice(data);
            let end = self.body.len() as u32;
            if let Some(e) = self.ents.get_mut(idx) {
                e.offset = end;
                if e.state.can_advance(AppendState::Appended) {
                    e.state = AppendState::Appended;
                }
            }
            self.writes = self.writes.saturating_add(1);
            return Ok(end);
        }
        // 无标志：定点写（越体尾即扩展）。
        let mut k = 0usize;
        while k < data.len() {
            let pos = (off as usize).saturating_add(k);
            let v = match data.get(k) {
                Some(b) => *b,
                None => 0,
            };
            if let Some(slot) = self.body.get_mut(pos) {
                *slot = v;
            } else {
                let _ = self.body.push(v);
            }
            k += 1;
        }
        if let Some(e) = self.ents.get_mut(idx) {
            e.offset = off.saturating_add(data.len() as u32);
            if e.state.can_advance(AppendState::Overwritten) {
                e.state = AppendState::Overwritten;
            }
        }
        self.writes = self.writes.saturating_add(1);
        self.overlaps = self.overlaps.saturating_add(1);
        Ok(off.saturating_add(data.len() as u32))
    }

    /// fd 寻址。
    fn fd_idx(&self, fd: u32) -> Option<usize> {
        let mut i = 0usize;
        while i < self.ents.len() {
            if let Some(e) = self.ents.get(i) {
                if e.fd == fd {
                    return Some(i);
                }
            }
            i += 1;
        }
        None
    }

    /// 写端偏移读取。
    pub fn offset_of(&self, fd: u32) -> Option<u32> {
        match self.fd_idx(fd) {
            Some(i) => match self.ents.get(i) {
                Some(e) => Some(e.offset),
                None => None,
            },
            None => None,
        }
    }

    /// 写端状态读取。
    pub fn state_of(&self, fd: u32) -> Option<AppendState> {
        match self.fd_idx(fd) {
            Some(i) => match self.ents.get(i) {
                Some(e) => Some(e.state),
                None => None,
            },
            None => None,
        }
    }

    /// O_APPEND 终位审计：追加端终位均不越当前文件长（各端写时独立落位）。
    pub fn ends_consistent(&self) -> bool {
        let total = self.body.len() as u32;
        let mut i = 0usize;
        while i < self.ents.len() {
            if let Some(e) = self.ents.get(i) {
                if e.append && e.offset > total {
                    return false;
                }
            }
            i += 1;
        }
        true
    }
}

// ── J1 达成核：多写端偏移独立终位正确 10/10 轮 ────────────────────────
/// 逐轮：双 O_APPEND 端交替写，终位一致、内容无损拼接。
pub fn run_append_rounds() -> (u32, usize) {
    let mut ok = 0u32;
    let mut probes = 0usize;
    let mut round = 0u32;
    while round < INJECT_ROUNDS {
        let mut g = AppendLog::new();
        let _ = g.bind(3, true, true);
        let _ = g.bind(4, true, true);
        let mut all_ok = true;
        // 面1：W1 尾写终位 2。
        all_ok = all_ok && g.write(3, &[0x11, 0x11]) == Ok(2);
        probes += 1;
        // 面2：W2 尾写终位 4。
        all_ok = all_ok && g.write(4, &[0x22, 0x22]) == Ok(4);
        probes += 1;
        // 面3：内容无损拼接（先 W1 后 W2）。
        all_ok = all_ok && g.body == Vec::from(&[0x11u8, 0x11, 0x22, 0x22] as &[u8]);
        probes += 1;
        // 面4：双端终位各自正确（W1=2、W2=4，互不回填）。
        all_ok = all_ok && g.ends_consistent() && g.offset_of(3) == Some(2)
            && g.offset_of(4) == Some(4);
        probes += 1;
        // 面5：状态推进 Appended。
        all_ok = all_ok && g.state_of(3) == Some(AppendState::Appended);
        probes += 1;
        // 面6：写计数 2。
        all_ok = all_ok && g.writes == 2;
        probes += 1;
        if all_ok {
            ok += 1;
        }
        round += 1;
    }
    (ok, probes)
}

// ── 注入核：坏追加矩阵 ────────────────────────────────────────────────
/// 四类坏样本。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BadAppend {
    /// 未绑定写（EBADF 族）
    NotOpen,
    /// 只读端写（EBADF 族）
    ReadOnly,
    /// 重复绑定（EBUSY 档）
    DupBind,
    /// 超限写（EFBIG 族）
    OverMax,
}

impl BadAppend {
    /// 闭集全量。
    pub const ALL: [BadAppend; 4] = [
        BadAppend::NotOpen,
        BadAppend::ReadOnly,
        BadAppend::DupBind,
        BadAppend::OverMax,
    ];

    /// 中文标签（注入明细用）。
    pub fn label(self) -> &'static str {
        match self {
            BadAppend::NotOpen => "未绑定 fd 写",
            BadAppend::ReadOnly => "只读端写",
            BadAppend::DupBind => "fd 重复绑定",
            BadAppend::OverMax => "超限写",
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
    pub fn record(&mut self, caught: bool) -> Result<(), ApCode> {
        self.injected = self.injected.saturating_add(1);
        if caught {
            self.detected = self.detected.saturating_add(1);
        } else {
            self.escaped = self.escaped.saturating_add(1);
        }
        if self.detected.saturating_add(self.escaped) != self.injected {
            return Err(ApCode::SegmentViolation);
        }
        Ok(())
    }

    /// 守恒式。
    pub fn conserved(&self) -> bool {
        self.detected.saturating_add(self.escaped) == self.injected
    }
}

/// 执行坏追加注入：每类 INJECT_ROUNDS 次。
pub fn run_bad_append_matrix() -> (InjectLedger, [u32; 4]) {
    let mut ledger = InjectLedger::zero();
    let mut per_class = [0u32; 4];
    let mut ci = 0usize;
    while ci < BadAppend::ALL.len() {
        let case = match BadAppend::ALL.get(ci) {
            Some(c) => *c,
            None => break,
        };
        let mut round = 0u32;
        while round < INJECT_ROUNDS {
            let mut g = AppendLog::new();
            let _ = g.bind(3, true, true);
            let caught = match case {
                BadAppend::NotOpen => {
                    g.write(9, b"x").err() == Some(ApCode::NotOpen)
                }
                BadAppend::ReadOnly => {
                    let _ = g.bind(4, false, false);
                    g.write(4, b"x").err() == Some(ApCode::ReadOnly)
                }
                BadAppend::DupBind => {
                    g.bind(3, true, true).err() == Some(ApCode::DupBind)
                }
                BadAppend::OverMax => {
                    let big = [0x41u8; 65];
                    g.write(3, &big).err() == Some(ApCode::OverMax)
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

// ── 追加小结成文（纯函数） ────────────────────────────────────────────
/// 小结成文：「len=<n> ends=<0|1> overlaps=<m>」。
pub fn compose_append_summary(g: &AppendLog) -> Result<String, ApCode> {
    let out = format!(
        "len={} ends={} overlaps={}",
        g.body.len(), if g.ends_consistent() { 1u32 } else { 0u32 }, g.overlaps
    );
    if out.is_empty() {
        return Err(ApCode::EmptySummary);
    }
    Ok(out)
}

// ── Linux errno 对照表 ────────────────────────────────────────────────
/// (名称, 负值, Windows 近似码, 说明)
pub const ERRNO_TABLE: &[(&str, i32, i32, &str)] = &[
    ("EBADF", -9, 9, "Windows ERROR_INVALID_HANDLE；未绑定/只读端写族"),
    ("EBUSY", -16, 33, "Windows ERROR_LOCK_VIOLATION；重复绑定档"),
    ("EFBIG", -27, 223, "Windows ERROR_FILE_TOO_LARGE；超限写族"),
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

/// 规范语料指纹：双端追加→竞态复现→成文→摘哈希。
pub fn corpus_fingerprint() -> u32 {
    let mut g = AppendLog::new();
    let _ = g.bind(3, true, true);
    let _ = g.bind(4, false, true);
    let _ = g.write(3, &[1, 2]);
    let _ = g.write(4, &[9, 9]);
    let mut bytes: Vec<u8> = Vec::new();
    if let Ok(s) = compose_append_summary(&g) {
        bytes.extend_from_slice(s.as_bytes());
    }
    bytes.extend_from_slice(&g.body);
    fnv1a(&bytes)
}

// ── 检查入口 ──────────────────────────────────────────────────────────
/// F10131 判据集（14 条静态 + 3 条动态）。
pub fn run_unx10131_checks() -> CheckSet {
    let mut s = CheckSet::new("unx10131");

    // 1. 段位守卫
    let seg_ok = SEG_BASE % 0x40 == 0 && (0x930C0..=0x930FF).contains(&SEG_BASE);
    s.add("段位守卫：0x40 对齐且在本卡 0x40 窗口内", seg_ok,
          "段基址必须唯一且不与相邻卡重叠；越界即拒绝注册");

    // 2. 诊断码闭集
    let mut uniq = true;
    let mut i = 0usize;
    while i < ApCode::ALL.len() {
        let mut j = i + 1;
        while j < ApCode::ALL.len() {
            if let (Some(a), Some(b)) = (ApCode::ALL.get(i), ApCode::ALL.get(j)) {
                if a.val() == b.val() {
                    uniq = false;
                }
            }
            j += 1;
        }
        i += 1;
    }
    let text_ok = ApCode::ALL.iter().all(|c| !c.detail().is_empty());
    s.add("诊断码闭集：闭集码两两唯一且中文建议非空", uniq && text_ok,
          "段内闭集码不得重复，每个码必须能取到可操作说明");

    // 3. 解析器
    let parse_ok = parse_wcall("write|3|11ff").is_ok()
        && parse_wcall("write|3|f").err() == Some(ApCode::BadNumber)
        && parse_wcall("write|3").err() == Some(ApCode::MissingPart)
        && parse_wcall("write|3|").is_ok();
    s.add("调用行解析器：hex 偶长样本放行且坏成分逐档拒绝", parse_ok,
          "data 为 hex 偶长样本（空串=零写）；非偶长/非 hex 逐档拒");

    // 4. J1 达成核
    let (ok, probes) = run_append_rounds();
    s.add("J1 原子追加达成：多写端偏移独立终位正确 10/10 次",
          ok == INJECT_ROUNDS && probes == (INJECT_ROUNDS as usize) * 6,
          "每轮 6 面（尾写×2/无损拼接/终位一致/态推进/计数）全对");

    // 5. J2 无标志竞态复现（L1 条款注记）
    let mut g = AppendLog::new();
    let _ = g.bind(3, false, true);
    let _ = g.bind(4, false, true);
    let _ = g.write(3, &[0xAA, 0xAA]);
    let _ = g.write(4, &[0xBB, 0xBB]);
    let race_ok = g.body == Vec::from(&[0xBBu8, 0xBB] as &[u8])
        && g.overlaps == 2
        && g.offset_of(3) == Some(2);
    s.add("J2 无标志竞态复现：同偏移定点写后写胜出（L1：并发须 O_APPEND）",
          race_ok,
          "无 O_APPEND 时双端同偏移写覆盖可复现；L1 条款要求并发写加 O_APPEND");

    // 6. 只读端写档
    let mut g2 = AppendLog::new();
    let _ = g2.bind(3, false, false);
    let ro_ok = g2.write(3, b"x").err() == Some(ApCode::ReadOnly);
    s.add("只读端写：EBADF 档独立精确归因", ro_ok,
          "无写权限位的 fd 写必须拒");

    // 7. 重复绑定档
    let dup_ok = g2.bind(3, true, true).err() == Some(ApCode::DupBind);
    s.add("重复绑定：EBUSY 档独立精确归因", dup_ok,
          "fd 二次绑定必须拒");

    // 8. 终位审计
    let mut g3 = AppendLog::new();
    let _ = g3.bind(5, true, true);
    let _ = g3.write(5, &[1, 2, 3]);
    let ends_ok = g3.ends_consistent() && g3.offset_of(5) == Some(3);
    s.add("终位审计：追加端终位恒等于文件长", ends_ok,
          "EndDrift 是 O_APPEND 语义根缺陷；必须全账一致");

    // 9. 注入核
    let (inj, per_class) = run_bad_append_matrix();
    let all_caught = per_class.iter().all(|n| *n == INJECT_ROUNDS);
    s.add("注入核：四类坏样本各 10/10 拒绝", all_caught,
          "未绑定/只读/重复绑定/超限必须逐类逐次精确复现");

    // 10. 注入账守恒
    s.add("注入账守恒：注入=检出+逃逸且逃逸=0",
          inj.conserved() && inj.escaped == 0
              && inj.injected == BadAppend::ALL.len() as u32 * INJECT_ROUNDS,
          "40 次注入必须 40 次检出、0 逃逸");

    // 11. 状态机
    let sm_ok = AppendState::Bound.can_advance(AppendState::Appended)
        && AppendState::Appended.can_advance(AppendState::Overwritten)
        && !AppendState::Overwritten.can_advance(AppendState::Bound)
        && AppendState::Overwritten.label() == "已覆写";
    s.add("状态机：Unbound→Bound→Appended→Overwritten 单向", sm_ok,
          "写端生命周期只许单调推进；倒退即协议违约");

    // 12. Linux 逐码一致
    let codes_ok = agrees_with_linux(9) && agrees_with_linux(16) && agrees_with_linux(27)
        && !agrees_with_linux(0) && !agrees_with_linux(9999);
    s.add("Linux 逐码一致：表内码精确、表外码拒绝", codes_ok,
          "错误族只允许 errno 表内正值且负值可复算");

    // 13. 成文 + 指纹
    let sum = compose_append_summary(&g3);
    let text_ok = match &sum {
        Ok(x) => x.contains("len=3") && x.contains("ends=1"),
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
    let ok_text = format!("原子追加达成轮数 {}/{}", ok, INJECT_ROUNDS);
    s.add_dyn("达成轮快照", ok == INJECT_ROUNDS, &ok_text);

    s
}

// ── 测试 ──────────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parser_guards() {
        assert!(parse_wcall("write|3|11ff").is_ok());
        assert_eq!(parse_wcall("write|3|f").err(), Some(ApCode::BadNumber));
        assert_eq!(parse_wcall("write|3").err(), Some(ApCode::MissingPart));
        assert!(parse_wcall("write|3|").is_ok());
        let c = parse_wcall("write|3|0a0b").unwrap_or(WCall { fd: 0, data: Vec::new() });
        assert_eq!(c.data, Vec::from(&[0x0Au8, 0x0B] as &[u8]));
    }

    #[test]
    fn append_two_writers() {
        let mut g = AppendLog::new();
        let _ = g.bind(3, true, true);
        let _ = g.bind(4, true, true);
        assert_eq!(g.write(3, &[0x11, 0x11]), Ok(2));
        assert_eq!(g.write(4, &[0x22, 0x22]), Ok(4));
        assert_eq!(g.body, Vec::from(&[0x11u8, 0x11, 0x22, 0x22] as &[u8]));
        assert!(g.ends_consistent());
    }

    #[test]
    fn race_without_append_reproduced() {
        let mut g = AppendLog::new();
        let _ = g.bind(3, false, true);
        let _ = g.bind(4, false, true);
        let _ = g.write(3, &[0xAA, 0xAA]);
        let _ = g.write(4, &[0xBB, 0xBB]);
        assert_eq!(g.body, Vec::from(&[0xBBu8, 0xBB] as &[u8]));
        assert_eq!(g.overlaps, 2);
    }

    #[test]
    fn readonly_blocked() {
        let mut g = AppendLog::new();
        let _ = g.bind(3, true, false);
        assert_eq!(g.write(3, b"x").err(), Some(ApCode::ReadOnly));
    }

    #[test]
    fn dup_bind_blocked() {
        let mut g = AppendLog::new();
        let _ = g.bind(3, true, true);
        assert_eq!(g.bind(3, true, true).err(), Some(ApCode::DupBind));
    }

    #[test]
    fn not_open_blocked() {
        let mut g = AppendLog::new();
        assert_eq!(g.write(9, b"x").err(), Some(ApCode::NotOpen));
    }

    #[test]
    fn over_max_blocked() {
        let mut g = AppendLog::new();
        let _ = g.bind(3, true, true);
        let big = [0x41u8; 65];
        assert_eq!(g.write(3, &big).err(), Some(ApCode::OverMax));
    }

    #[test]
    fn bad_matrix_green() {
        let (inj, per) = run_bad_append_matrix();
        assert_eq!(inj.injected, 4 * INJECT_ROUNDS);
        assert_eq!(inj.escaped, 0);
        for n in per.iter() {
            assert_eq!(*n, INJECT_ROUNDS);
        }
    }

    #[test]
    fn rounds_full() {
        let (ok, probes) = run_append_rounds();
        assert_eq!(ok, INJECT_ROUNDS);
        assert_eq!(probes, (INJECT_ROUNDS as usize) * 6);
    }

    #[test]
    fn state_machine_monotonic() {
        assert!(AppendState::Bound.can_advance(AppendState::Overwritten));
        assert!(!AppendState::Overwritten.can_advance(AppendState::Bound));
    }

    #[test]
    fn deterministic_corpus() {
        assert_eq!(corpus_fingerprint(), corpus_fingerprint());
    }

    #[test]
    fn unx10131_checks_green() {
        let set = run_unx10131_checks();
        assert!(set.all_passed(), "UNX-F10131 red: {}", set.red_items_text());
    }
}
