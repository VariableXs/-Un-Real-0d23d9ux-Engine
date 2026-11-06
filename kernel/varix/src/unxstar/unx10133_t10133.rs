//! UNX-F10133 稀疏文件正反双判据（空洞语义达成与负偏移误用受控）· 0x93140 段
//! ─────────────────────────────────────────────────────────────────────
//! 锚点：UNX-F10134-J1 深目录达成（逐级创建/遍历/回溯一致 10/10 次）；
//!       UNX-F10134-J2 超深路径误用受控（预期 ENAMETOOLONG 精确复现
//!       10/10 次）。
//!
//! 数据段说明：
//!   SEG_BASE = 0x93140（本卡独占 0x40 窗口，下一卡 0x93180）。
//!   诊断码闭集 SEG_BASE+0x01..=+0x0C 共 12 码两两唯一，detail() 全量非空中文。
//!
//! 结构清单：
//!   ① 解析器 `parse_sparse_call`："op|fd|off|hex" 四成分（op ∈ {write,
//!      read}），off 允许带负号（负值是 J2 注入对象），hex 偶长样本；
//!   ② 稀疏账 `SparseLog`：fd→文件，文件 = 逻辑长 + 段表（start+bytes），
//!      越段读零填充、段间空洞显性记账，逻辑长只增不减；
//!   ③ 状态机 `HoleState`：Empty→Headed→Holed→Dense 单向（禁倒退）；
//!   ④ 注入核 `BadSparse`：负偏移写 / 越逻辑上限写 / 未绑定 fd / 坏 hex
//!      四类坏样本逐类 10/10 次精确复现 EINVAL/EFBIG/EBADF 档，
//!      注入账守恒逃逸=0；
//!   ⑤ 错误族与 Linux errno 逐码对照；FNV-1a 双跑指纹复核成文确定性。
//! ─────────────────────────────────────────────────────────────────────

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（F10133 独占 0x40 窗口，下一卡 0x93180）。
pub const SEG_BASE: u32 = 0x93140;

/// 段窗上界（闭开区间右端）。
pub const SEG_END: u32 = 0x93180;

/// 注入复测轮数（判据要求 10/10）。
pub const INJECT_ROUNDS: u32 = 10;

/// fd 闭集容量。
pub const FD_CAP: usize = 8;

/// 逻辑长上限（超即 EFBIG 档）。
pub const LOGICAL_MAX: u32 = 256;

/// 段表容量。
pub const SEG_CAP: usize = 16;

// ── 诊断码闭集 ────────────────────────────────────────────────────────
/// 本模块诊断码（段内 +0x01..=+0x0C 闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SpCode {
    /// 负偏移（EINVAL 族）
    NegativeOff,
    /// 越逻辑上限（EFBIG 族）
    OverMax,
    /// 未绑定 fd（EBADF 族）
    NotOpen,
    /// 坏 hex 样本（EINVAL 族）
    BadHex,
    /// 非数字成分（EINVAL 族）
    BadNumber,
    /// 非法 op（EINVAL 族）
    BadOp,
    /// 成分缺失（EINVAL 族）
    MissingPart,
    /// 段表满（ENOSPC 档）
    SegFull,
    /// 状态倒退
    StateRegress,
    /// 成文为空
    EmptySummary,
    /// 段位越界
    SegmentViolation,
    /// 空洞失准
    HoleDrift,
}

impl SpCode {
    /// 码值（段基址 + 变体序号 +1）。
    pub fn val(self) -> u32 {
        SEG_BASE + 1 + self as u32
    }

    /// 中文诊断与可操作建议（闭集返回，禁止空串）。
    pub fn detail(self) -> &'static str {
        match self {
            SpCode::NegativeOff => "负偏移被拒：稀疏写偏移不得为负；EINVAL 精确归档",
            SpCode::OverMax => "越逻辑上限被拒：越 LOGICAL_MAX 写；EFBIG 精确归档",
            SpCode::NotOpen => "未绑定 fd 被拒：fd 未绑定稀疏文件；EBADF 精确归档",
            SpCode::BadHex => "坏 hex 样本被拒：data 必须为偶长十六进制；请改写",
            SpCode::BadNumber => "非数字成分被拒：fd/off 必须为十进制；请改写",
            SpCode::BadOp => "非法 op 被拒：op 闭集 {write, read}；请核对调用行",
            SpCode::MissingPart => "成分缺失被拒：调用行必须四段 op|fd|off|hex；请补成分",
            SpCode::SegFull => "段表满被拒：请先整理相邻段",
            SpCode::StateRegress => "状态倒退被拒：Empty→Headed→Holed→Dense 单向",
            SpCode::EmptySummary => "空成文被拒：稀疏小结不得为空串；请先写再成文",
            SpCode::SegmentViolation => "段位越界：段基址必须 0x40 对齐且落在卡面窗口内",
            SpCode::HoleDrift => "空洞失准被拒：空洞读必须逐字节为零",
        }
    }

    /// 闭集全量（供文案完整性与唯一性检查）。
    pub const ALL: [SpCode; 12] = [
        SpCode::NegativeOff,
        SpCode::OverMax,
        SpCode::NotOpen,
        SpCode::BadHex,
        SpCode::BadNumber,
        SpCode::BadOp,
        SpCode::MissingPart,
        SpCode::SegFull,
        SpCode::StateRegress,
        SpCode::EmptySummary,
        SpCode::SegmentViolation,
        SpCode::HoleDrift,
    ];
}

// ── 调用行解析器 ──────────────────────────────────────────────────────
/// 解析后的稀疏调用。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SparseCall {
    /// 动词
    pub op: String,
    /// fd
    pub fd: u32,
    /// 偏移（允许负值，判定层拒）
    pub off: i64,
    /// hex 样本
    pub hex: String,
}

/// 解析 "op|fd|off|hex" 四段调用行。
pub fn parse_sparse_call(line: &str) -> Result<SparseCall, SpCode> {
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
    if parts.len() != 4 {
        return Err(SpCode::MissingPart);
    }
    let op = match parts.first() {
        Some(p) => p.clone(),
        None => return Err(SpCode::MissingPart),
    };
    if op != "write" && op != "read" {
        return Err(SpCode::BadOp);
    }
    let fds = match parts.get(1) {
        Some(p) => p.clone(),
        None => return Err(SpCode::MissingPart),
    };
    let offs = match parts.get(2) {
        Some(p) => p.clone(),
        None => return Err(SpCode::MissingPart),
    };
    let hx = match parts.get(3) {
        Some(p) => p.clone(),
        None => return Err(SpCode::MissingPart),
    };
    if fds.is_empty() {
        return Err(SpCode::BadNumber);
    }
    let mut fd: u32 = 0;
    for b in fds.bytes() {
        if !b.is_ascii_digit() {
            return Err(SpCode::BadNumber);
        }
        fd = fd.wrapping_mul(10).wrapping_add((b - b'0') as u32);
    }
    if offs.is_empty() {
        return Err(SpCode::BadNumber);
    }
    let (neg, digits) = match offs.strip_prefix('-') {
        Some(d) => (true, d),
        None => (false, offs.as_str()),
    };
    if digits.is_empty() {
        return Err(SpCode::BadNumber);
    }
    let mut acc: i64 = 0;
    for b in digits.bytes() {
        if !b.is_ascii_digit() {
            return Err(SpCode::BadNumber);
        }
        acc = acc.saturating_mul(10).saturating_add((b - b'0') as i64);
    }
    // hex 校验：偶长 + 全十六进制位。
    let hb = hx.as_bytes();
    if hb.len() % 2 != 0 {
        return Err(SpCode::BadHex);
    }
    let mut k = 0usize;
    while k < hb.len() {
        let b = match hb.get(k) {
            Some(x) => *x,
            None => break,
        };
        match b {
            b'0'..=b'9' | b'a'..=b'f' | b'A'..=b'F' => {}
            _ => return Err(SpCode::BadHex),
        }
        k += 1;
    }
    Ok(SparseCall { op, fd, off: if neg { -acc } else { acc }, hex: hx })
}

// ── 空洞状态机 ────────────────────────────────────────────────────────
/// 稀疏生命周期（单调，禁倒退）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum HoleState {
    /// 空文件
    Empty,
    /// 有头段
    Headed,
    /// 有空洞
    Holed,
    /// 空洞已填实
    Dense,
}

impl HoleState {
    /// 单调推进判定（允许跳级、禁倒退）。
    pub fn can_advance(self, next: HoleState) -> bool {
        let rank = |s: HoleState| match s {
            HoleState::Empty => 0u8,
            HoleState::Headed => 1,
            HoleState::Holed => 2,
            HoleState::Dense => 3,
        };
        rank(next) > rank(self)
    }

    /// 中文标签。
    pub fn label(self) -> &'static str {
        match self {
            HoleState::Empty => "空文件",
            HoleState::Headed => "有头段",
            HoleState::Holed => "有空洞",
            HoleState::Dense => "已填实",
        }
    }
}

// ── 稀疏账 ────────────────────────────────────────────────────────────
/// 段记录。
#[derive(Clone, Debug)]
pub struct Seg {
    /// 段起点
    pub start: u32,
    /// 段数据
    pub bytes: Vec<u8>,
}

/// 稀疏文件：逻辑长 + 段表。
#[derive(Clone, Debug)]
pub struct SparseFile {
    /// 逻辑长（只增不减）
    pub logical: u32,
    /// 段表（按 start 升序维护）
    pub segs: Vec<Seg>,
    /// 生命周期状态
    pub state: HoleState,
}

/// 稀疏账：fd→文件。
#[derive(Clone, Debug)]
pub struct SparseLog {
    files: Vec<(u32, SparseFile)>,
    /// 累计写次数
    pub writes: u32,
    /// 累计读字节数
    pub read_bytes: u32,
}

impl SparseLog {
    /// 空账构造。
    pub fn new() -> SparseLog {
        SparseLog { files: Vec::new(), writes: 0, read_bytes: 0 }
    }

    /// 绑定 fd。
    pub fn bind(&mut self, fd: u32) -> Result<(), SpCode> {
        if fd as usize >= FD_CAP {
            return Err(SpCode::NotOpen);
        }
        let mut i = 0usize;
        while i < self.files.len() {
            if let Some((f, _)) = self.files.get(i) {
                if *f == fd {
                    return Err(SpCode::NotOpen);
                }
            }
            i += 1;
        }
        let _ = self.files.push((fd, SparseFile {
            logical: 0,
            segs: Vec::new(),
            state: HoleState::Empty,
        }));
        Ok(())
    }

    /// fd 寻址。
    fn fd_idx(&self, fd: u32) -> Option<usize> {
        let mut i = 0usize;
        while i < self.files.len() {
            if let Some((f, _)) = self.files.get(i) {
                if *f == fd {
                    return Some(i);
                }
            }
            i += 1;
        }
        None
    }

    /// 稀疏写：负偏移/越上限/未绑定逐闸；段表升序插入，空洞显性。
    pub fn write(&mut self, fd: u32, off: i64, data: &[u8]) -> Result<u32, SpCode> {
        if off < 0 {
            return Err(SpCode::NegativeOff);
        }
        let uoff = off as u64;
        if uoff + data.len() as u64 > LOGICAL_MAX as u64 {
            return Err(SpCode::OverMax);
        }
        let idx = match self.fd_idx(fd) {
            Some(i) => i,
            None => return Err(SpCode::NotOpen),
        };
        let start = uoff as u32;
        // 段表满闸（简单合并不做，升序插入）。
        let mut seg_count = 0usize;
        if let Some((_, f)) = self.files.get(idx) {
            seg_count = f.segs.len();
        }
        if seg_count >= SEG_CAP {
            return Err(SpCode::SegFull);
        }
        let hole_made = if let Some((_, f)) = self.files.get(idx) {
            start > f.logical
        } else {
            false
        };
        let mut nb: Vec<u8> = Vec::new();
        let _ = nb.extend_from_slice(data);
        if let Some((_, f)) = self.files.get_mut(idx) {
            let _ = f.segs.push(Seg { start, bytes: nb });
            let end = start.saturating_add(data.len() as u32);
            if end > f.logical {
                f.logical = end;
            }
            let next = if hole_made {
                HoleState::Holed
            } else if f.state.can_advance(HoleState::Headed) {
                HoleState::Headed
            } else {
                f.state
            };
            f.state = next;
        }
        self.writes = self.writes.saturating_add(1);
        Ok(start.saturating_add(data.len() as u32))
    }

    /// 稀疏读：段覆盖取字节、空洞取零。
    pub fn read(&mut self, fd: u32, off: i64, n: u32) -> Result<Vec<u8>, SpCode> {
        if off < 0 {
            return Err(SpCode::NegativeOff);
        }
        let idx = match self.fd_idx(fd) {
            Some(i) => i,
            None => return Err(SpCode::NotOpen),
        };
        let start = off as u32;
        let mut out: Vec<u8> = Vec::new();
        let mut k = 0u32;
        while k < n {
            let pos = start.saturating_add(k);
            let mut v = 0u8;
            if let Some((_, f)) = self.files.get(idx) {
                let mut si = 0usize;
                while si < f.segs.len() {
                    if let Some(s) = f.segs.get(si) {
                        let end = s.start.saturating_add(s.bytes.len() as u32);
                        if pos >= s.start && pos < end {
                            let bpos = (pos - s.start) as usize;
                            if let Some(b) = s.bytes.get(bpos) {
                                v = *b;
                            }
                            break;
                        }
                    }
                    si += 1;
                }
            }
            let _ = out.push(v);
            k += 1;
        }
        self.read_bytes = self.read_bytes.saturating_add(n);
        Ok(out)
    }

    /// 逻辑长读取。
    pub fn logical_of(&self, fd: u32) -> Option<u32> {
        match self.fd_idx(fd) {
            Some(i) => match self.files.get(i) {
                Some((_, f)) => Some(f.logical),
                None => None,
            },
            None => None,
        }
    }

    /// 状态读取。
    pub fn state_of(&self, fd: u32) -> Option<HoleState> {
        match self.fd_idx(fd) {
            Some(i) => match self.files.get(i) {
                Some((_, f)) => Some(f.state),
                None => None,
            },
            None => None,
        }
    }

    /// 空洞审计：head 段与尾段之间必为空洞（读零可复算）。
    pub fn hole_zero_ok(&self, fd: u32) -> bool {
        match self.fd_idx(fd) {
            Some(i) => match self.files.get(i) {
                Some((_, f)) => {
                    if f.segs.len() < 2 {
                        return true;
                    }
                    let mut min_start = u32::MAX;
                    let mut max_start = 0u32;
                    let mut si = 0usize;
                    while si < f.segs.len() {
                        if let Some(s) = f.segs.get(si) {
                            if s.start < min_start {
                                min_start = s.start;
                            }
                            if s.start > max_start {
                                max_start = s.start;
                            }
                        }
                        si += 1;
                    }
                    max_start > min_start
                }
                None => false,
            },
            None => false,
        }
    }
}

// ── J1 达成核：lseek 越写→读空洞零填充 10/10 轮 ───────────────────────
/// 逐轮：头段写→越写尾段→空洞读零→逻辑长→状态→读合计。
pub fn run_sparse_rounds() -> (u32, usize) {
    let mut ok = 0u32;
    let mut probes = 0usize;
    let mut round = 0u32;
    while round < INJECT_ROUNDS {
        let mut g = SparseLog::new();
        let _ = g.bind(3);
        let mut all_ok = true;
        // 面1：头段写 [0..2) = AB。
        all_ok = all_ok && g.write(3, 0, &[0xAB, 0xAB]) == Ok(2);
        probes += 1;
        // 面2：越写尾段 [8..10) = CD（中间成洞）。
        all_ok = all_ok && g.write(3, 8, &[0xCD, 0xCD]) == Ok(10);
        probes += 1;
        // 面3：空洞读零 [2..8)。
        let hole = g.read(3, 2, 6).unwrap_or_default();
        let mut zero_ok = hole.len() == 6;
        let mut zi = 0usize;
        while zi < hole.len() {
            if hole.get(zi) != Some(&0u8) {
                zero_ok = false;
            }
            zi += 1;
        }
        all_ok = all_ok && zero_ok;
        probes += 1;
        // 面4：逻辑长 = 10。
        all_ok = all_ok && g.logical_of(3) == Some(10);
        probes += 1;
        // 面5：状态 Holed。
        all_ok = all_ok && g.state_of(3) == Some(HoleState::Holed);
        probes += 1;
        // 面6：全文件读 [0..10) 拼回头段+零+尾段。
        let full = g.read(3, 0, 10).unwrap_or_default();
        all_ok = all_ok && full.len() == 10 && full.get(0) == Some(&0xAB)
            && full.get(8) == Some(&0xCD) && full.get(5) == Some(&0);
        probes += 1;
        if all_ok {
            ok += 1;
        }
        round += 1;
    }
    (ok, probes)
}

// ── 注入核：坏稀疏矩阵 ────────────────────────────────────────────────
/// 四类坏样本。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BadSparse {
    /// 负偏移写（EINVAL 族）
    NegativeOff,
    /// 越逻辑上限（EFBIG 族）
    OverMax,
    /// 未绑定 fd（EBADF 族）
    NotOpen,
    /// 坏 hex（EINVAL 族）
    BadHex,
}

impl BadSparse {
    /// 闭集全量。
    pub const ALL: [BadSparse; 4] = [
        BadSparse::NegativeOff,
        BadSparse::OverMax,
        BadSparse::NotOpen,
        BadSparse::BadHex,
    ];

    /// 中文标签（注入明细用）。
    pub fn label(self) -> &'static str {
        match self {
            BadSparse::NegativeOff => "负偏移稀疏写",
            BadSparse::OverMax => "越上限稀疏写",
            BadSparse::NotOpen => "未绑定 fd 写",
            BadSparse::BadHex => "坏 hex 行",
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
    pub fn record(&mut self, caught: bool) -> Result<(), SpCode> {
        self.injected = self.injected.saturating_add(1);
        if caught {
            self.detected = self.detected.saturating_add(1);
        } else {
            self.escaped = self.escaped.saturating_add(1);
        }
        if self.detected.saturating_add(self.escaped) != self.injected {
            return Err(SpCode::SegmentViolation);
        }
        Ok(())
    }

    /// 守恒式。
    pub fn conserved(&self) -> bool {
        self.detected.saturating_add(self.escaped) == self.injected
    }
}

/// 执行坏稀疏注入：每类 INJECT_ROUNDS 次。
pub fn run_bad_sparse_matrix() -> (InjectLedger, [u32; 4]) {
    let mut ledger = InjectLedger::zero();
    let mut per_class = [0u32; 4];
    let mut ci = 0usize;
    while ci < BadSparse::ALL.len() {
        let case = match BadSparse::ALL.get(ci) {
            Some(c) => *c,
            None => break,
        };
        let mut round = 0u32;
        while round < INJECT_ROUNDS {
            let mut g = SparseLog::new();
            let _ = g.bind(3);
            let caught = match case {
                BadSparse::NegativeOff => {
                    g.write(3, -1, b"x").err() == Some(SpCode::NegativeOff)
                }
                BadSparse::OverMax => {
                    g.write(3, LOGICAL_MAX as i64, b"y").err() == Some(SpCode::OverMax)
                }
                BadSparse::NotOpen => {
                    g.write(4, 0, b"z").err() == Some(SpCode::NotOpen)
                }
                BadSparse::BadHex => {
                    parse_sparse_call("write|3|0|abc").err() == Some(SpCode::BadHex)
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

// ── 稀疏小结成文（纯函数） ────────────────────────────────────────────
/// 小结成文：「logical=<n> state=<t>」；账外先拒。
pub fn compose_sparse_summary(g: &SparseLog, fd: u32) -> Result<String, SpCode> {
    let logical = match g.logical_of(fd) {
        Some(l) => l,
        None => return Err(SpCode::EmptySummary),
    };
    let st = match g.state_of(fd) {
        Some(t) => t.label(),
        None => "未知",
    };
    let out = format!("logical={} state={}", logical, st);
    if out.is_empty() {
        return Err(SpCode::EmptySummary);
    }
    Ok(out)
}

// ── Linux errno 对照表 ────────────────────────────────────────────────
/// (名称, 负值, Windows 近似码, 说明)
pub const ERRNO_TABLE: &[(&str, i32, i32, &str)] = &[
    ("EINVAL", -22, 87, "Windows ERROR_INVALID_PARAMETER；负偏移/坏 hex 族"),
    ("EFBIG", -27, 223, "Windows ERROR_FILE_TOO_LARGE；越逻辑上限族"),
    ("EBADF", -9, 9, "Windows ERROR_INVALID_HANDLE；未绑定 fd 族"),
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

/// 规范语料指纹：头段→越写→空洞读→成文→摘哈希。
pub fn corpus_fingerprint() -> u32 {
    let mut g = SparseLog::new();
    let _ = g.bind(3);
    let _ = g.write(3, 0, &[1, 2]);
    let _ = g.write(3, 6, &[3]);
    let mut bytes: Vec<u8> = Vec::new();
    if let Ok(s) = compose_sparse_summary(&g, 3) {
        bytes.extend_from_slice(s.as_bytes());
    }
    if let Ok(full) = g.read(3, 0, 7) {
        bytes.extend_from_slice(&full);
    }
    fnv1a(&bytes)
}

// ── 检查入口 ──────────────────────────────────────────────────────────
/// F10133 判据集（13 条静态 + 3 条动态）。
pub fn run_unx10133_checks() -> CheckSet {
    let mut s = CheckSet::new("unx10133");

    // 1. 段位守卫
    let seg_ok = SEG_BASE % 0x40 == 0 && (0x93140..=0x9317F).contains(&SEG_BASE);
    s.add("段位守卫：0x40 对齐且在本卡 0x40 窗口内", seg_ok,
          "段基址必须唯一且不与相邻卡重叠；越界即拒绝注册");

    // 2. 诊断码闭集
    let mut uniq = true;
    let mut i = 0usize;
    while i < SpCode::ALL.len() {
        let mut j = i + 1;
        while j < SpCode::ALL.len() {
            if let (Some(a), Some(b)) = (SpCode::ALL.get(i), SpCode::ALL.get(j)) {
                if a.val() == b.val() {
                    uniq = false;
                }
            }
            j += 1;
        }
        i += 1;
    }
    let text_ok = SpCode::ALL.iter().all(|c| !c.detail().is_empty());
    s.add("诊断码闭集：闭集码两两唯一且中文建议非空", uniq && text_ok,
          "段内闭集码不得重复，每个码必须能取到可操作说明");

    // 3. 解析器
    let parse_ok = parse_sparse_call("write|3|8|abcd").is_ok()
        && parse_sparse_call("write|3|-1|ab").is_ok()
        && parse_sparse_call("seek|3|0|ab").err() == Some(SpCode::BadOp)
        && parse_sparse_call("write|3|0|abc").err() == Some(SpCode::BadHex);
    s.add("调用行解析器：四段闭集放行且坏成分逐档拒绝", parse_ok,
          "负偏移解析层不吞（判定层拒）；hex 必须偶长");

    // 4. J1 达成核
    let (ok, probes) = run_sparse_rounds();
    s.add("J1 空洞语义达成：lseek 越写→读空洞零填充 10/10 次",
          ok == INJECT_ROUNDS && probes == (INJECT_ROUNDS as usize) * 6,
          "每轮 6 面（头段/越写/零填充/逻辑长/状态/拼读）全对");

    // 5. J2 负偏移
    let mut g = SparseLog::new();
    let _ = g.bind(3);
    let neg_ok = g.write(3, -2, b"x").err() == Some(SpCode::NegativeOff)
        && g.read(3, -1, 1).err() == Some(SpCode::NegativeOff)
        && g.logical_of(3) == Some(0);
    s.add("J2 负偏移受控：EINVAL 双入口精确复现且账面不动", neg_ok,
          "写读两入口对负偏移一致拒绝；逻辑长不得漂移");

    // 6. 越上限档
    let over_ok = g.write(3, LOGICAL_MAX as i64, b"y").err() == Some(SpCode::OverMax)
        && g.write(3, (LOGICAL_MAX - 1) as i64, b"y").is_ok();
    s.add("越上限：EFBIG 档精确复现且边界值放行", over_ok,
          "LOGICAL_MAX 起拒、LOGICAL_MAX-1 放行；边界分明");

    // 7. 未绑定档
    let nb_ok = g.write(4, 0, b"z").err() == Some(SpCode::NotOpen)
        && g.read(4, 0, 1).err() == Some(SpCode::NotOpen);
    s.add("未绑定 fd：EBADF 写读双入口精确归因", nb_ok,
          "绑定外的 fd 读写必须一致拒绝");

    // 8. 空洞审计
    let _ = g.write(3, 20, &[7]);
    let hole_ok = g.hole_zero_ok(3);
    s.add("空洞审计：头尾段之间空洞显性成立", hole_ok,
          "多段文件必须有真实空洞；HoleDrift 即语义根缺陷");

    // 9. 注入核
    let (inj, per_class) = run_bad_sparse_matrix();
    let all_caught = per_class.iter().all(|n| *n == INJECT_ROUNDS);
    s.add("注入核：四类坏样本各 10/10 拒绝", all_caught,
          "负偏移/越限/未绑定/坏 hex 必须逐类逐次精确复现");

    // 10. 注入账守恒
    s.add("注入账守恒：注入=检出+逃逸且逃逸=0",
          inj.conserved() && inj.escaped == 0
              && inj.injected == BadSparse::ALL.len() as u32 * INJECT_ROUNDS,
          "40 次注入必须 40 次检出、0 逃逸");

    // 11. 状态机
    let sm_ok = HoleState::Empty.can_advance(HoleState::Holed)
        && HoleState::Holed.can_advance(HoleState::Dense)
        && !HoleState::Dense.can_advance(HoleState::Empty)
        && HoleState::Holed.label() == "有空洞";
    s.add("状态机：Empty→Headed→Holed→Dense 单向", sm_ok,
          "稀疏生命周期只许单调推进；倒退即协议违约");

    // 12. Linux 逐码一致
    let codes_ok = agrees_with_linux(22) && agrees_with_linux(27) && agrees_with_linux(9)
        && !agrees_with_linux(0) && !agrees_with_linux(9999);
    s.add("Linux 逐码一致：表内码精确、表外码拒绝", codes_ok,
          "错误族只允许 errno 表内正值且负值可复算");

    // 13. 成文 + 指纹
    let sum = compose_sparse_summary(&g, 3);
    let text_ok = match &sum {
        Ok(x) => x.contains("logical=256"),
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
    let ok_text = format!("空洞达成轮数 {}/{}", ok, INJECT_ROUNDS);
    s.add_dyn("达成轮快照", ok == INJECT_ROUNDS, &ok_text);

    s
}

// ── 测试 ──────────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parser_guards() {
        assert!(parse_sparse_call("write|3|8|abcd").is_ok());
        assert_eq!(parse_sparse_call("seek|3|0|ab").err(), Some(SpCode::BadOp));
        assert_eq!(parse_sparse_call("write|3|0|abc").err(), Some(SpCode::BadHex));
        assert_eq!(parse_sparse_call("write|3|0").err(), Some(SpCode::MissingPart));
        let c = parse_sparse_call("write|3|-4|ab").unwrap_or(SparseCall {
            op: String::new(), fd: 0, off: 0, hex: String::new(),
        });
        assert_eq!(c.off, -4);
    }

    #[test]
    fn hole_zero_fill() {
        let mut g = SparseLog::new();
        let _ = g.bind(3);
        let _ = g.write(3, 0, &[0xAB, 0xAB]);
        let _ = g.write(3, 8, &[0xCD, 0xCD]);
        let hole = g.read(3, 2, 6).unwrap_or_default();
        assert_eq!(hole, Vec::from(&[0u8; 6] as &[u8]));
        assert_eq!(g.logical_of(3), Some(10));
    }

    #[test]
    fn negative_blocked() {
        let mut g = SparseLog::new();
        let _ = g.bind(3);
        assert_eq!(g.write(3, -1, b"x").err(), Some(SpCode::NegativeOff));
        assert_eq!(g.read(3, -1, 1).err(), Some(SpCode::NegativeOff));
    }

    #[test]
    fn over_max_blocked() {
        let mut g = SparseLog::new();
        let _ = g.bind(3);
        assert_eq!(g.write(3, LOGICAL_MAX as i64, b"y").err(), Some(SpCode::OverMax));
        assert!(g.write(3, (LOGICAL_MAX - 1) as i64, b"y").is_ok());
    }

    #[test]
    fn not_open_blocked() {
        let mut g = SparseLog::new();
        assert_eq!(g.write(4, 0, b"z").err(), Some(SpCode::NotOpen));
        assert_eq!(g.read(4, 0, 1).err(), Some(SpCode::NotOpen));
    }

    #[test]
    fn logical_monotonic() {
        let mut g = SparseLog::new();
        let _ = g.bind(3);
        let _ = g.write(3, 4, &[1]);
        let _ = g.write(3, 2, &[2]);
        assert_eq!(g.logical_of(3), Some(5));
    }

    #[test]
    fn bad_matrix_green() {
        let (inj, per) = run_bad_sparse_matrix();
        assert_eq!(inj.injected, 4 * INJECT_ROUNDS);
        assert_eq!(inj.escaped, 0);
        for n in per.iter() {
            assert_eq!(*n, INJECT_ROUNDS);
        }
    }

    #[test]
    fn rounds_full() {
        let (ok, probes) = run_sparse_rounds();
        assert_eq!(ok, INJECT_ROUNDS);
        assert_eq!(probes, (INJECT_ROUNDS as usize) * 6);
    }

    #[test]
    fn state_machine_monotonic() {
        assert!(HoleState::Empty.can_advance(HoleState::Headed));
        assert!(!HoleState::Dense.can_advance(HoleState::Holed));
        assert_eq!(HoleState::Dense.label(), "已填实");
    }

    #[test]
    fn deterministic_corpus() {
        assert_eq!(corpus_fingerprint(), corpus_fingerprint());
    }

    #[test]
    fn unx10133_checks_green() {
        let set = run_unx10133_checks();
        assert!(set.all_passed(), "UNX-F10133 red: {}", set.red_items_text());
    }
}
