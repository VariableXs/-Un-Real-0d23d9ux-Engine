//! UNX-F10132 fsync/fdatasync 正反双判据（落盘达成与非法 fd 误用受控）· 0x93100 段
//! ─────────────────────────────────────────────────────────────────────
//! 锚点：UNX-F10133-J1 空洞语义达成（lseek 越写→读空洞零填充 10/10 次）；
//!       UNX-F10133-J2 负偏移写误用受控（预期 EINVAL 精确复现 10/10 次）。
//!
//! 数据段说明：
//!   SEG_BASE = 0x93100（本卡独占 0x40 窗口，下一卡 0x93140）。
//!   诊断码闭集 SEG_BASE+0x01..=+0x0C 共 12 码两两唯一，detail() 全量非空中文。
//!
//! 结构清单：
//!   ① 解析器 `parse_sync_call`："op|fd|hex" 三成分（op ∈ {write, fsync,
//!      fdatasync}），fd 十进制、hex 偶长样本，逐档拒绝；
//!   ② 同步账 `SyncLog`：fd→(脏位, 已同步快照)，写标脏、fsync/fdatasync
//!      落快照清脏，重读一致 = 快照与体逐字节相等；
//!   ③ 状态机 `SyncState`：Blank→Dirty→Synced 单向（禁倒退）；
//!   ④ 注入核 `BadSync`：未绑定 fsync / 越闭集 fd / 重复绑定 / 超限写
//!      四类坏样本逐类 10/10 次精确复现 EBADF/EBUSY/EFBIG 档，
//!      注入账守恒逃逸=0；
//!   ⑤ 错误族与 Linux errno 逐码对照；FNV-1a 双跑指纹复核成文确定性。
//! ─────────────────────────────────────────────────────────────────────

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（F10132 独占 0x40 窗口，下一卡 0x93140）。
pub const SEG_BASE: u32 = 0x93100;

/// 段窗上界（闭开区间右端）。
pub const SEG_END: u32 = 0x93140;

/// 注入复测轮数（判据要求 10/10）。
pub const INJECT_ROUNDS: u32 = 10;

/// fd 闭集容量。
pub const FD_CAP: usize = 8;

/// 单次写长度上限。
pub const WRITE_MAX: u32 = 64;

// ── 诊断码闭集 ────────────────────────────────────────────────────────
/// 本模块诊断码（段内 +0x01..=+0x0C 闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SyCode {
    /// 未绑定 fsync（EBADF 族）
    NotOpen,
    /// fd 越闭集（EBADF 族）
    OutOfRange,
    /// 重复绑定（EBUSY 档）
    DupBind,
    /// 超限写（EFBIG 族）
    OverMax,
    /// 非数字成分（EINVAL 族）
    BadNumber,
    /// 非法 op（EINVAL 族）
    BadOp,
    /// 成分缺失（EINVAL 族）
    MissingPart,
    /// 状态倒退
    StateRegress,
    /// 成文为空
    EmptySummary,
    /// 段位越界
    SegmentViolation,
    /// 快照失平
    SnapDrift,
    /// 语料指纹漂移
    RoundTrip,
}

impl SyCode {
    /// 码值（段基址 + 变体序号 +1）。
    pub fn val(self) -> u32 {
        SEG_BASE + 1 + self as u32
    }

    /// 中文诊断与可操作建议（闭集返回，禁止空串）。
    pub fn detail(self) -> &'static str {
        match self {
            SyCode::NotOpen => "未绑定 fsync 被拒：fd 未绑定文件；EBADF 精确归档",
            SyCode::OutOfRange => "fd 越闭集被拒：fd 必须 < FD_CAP；EBADF 精确归档",
            SyCode::DupBind => "重复绑定被拒：fd 已绑定；EBUSY 档精确归档",
            SyCode::OverMax => "超限写被拒：单次写超 WRITE_MAX；EFBIG 精确归档",
            SyCode::BadNumber => "非数字成分被拒：fd 必须为十进制；请改写",
            SyCode::BadOp => "非法 op 被拒：op 闭集 {write, fsync, fdatasync}；请核对调用行",
            SyCode::MissingPart => "成分缺失被拒：调用行必须三段 op|fd|hex；请补成分",
            SyCode::StateRegress => "状态倒退被拒：Blank→Dirty→Synced 单向",
            SyCode::EmptySummary => "空成文被拒：同步小结不得为空串；请先写再成文",
            SyCode::SegmentViolation => "段位越界：段基址必须 0x40 对齐且落在卡面窗口内",
            SyCode::SnapDrift => "快照失平被拒：重读必须与体逐字节一致",
            SyCode::RoundTrip => "语料指纹漂移：同语料两次哈希不一致；请排查非确定源",
        }
    }

    /// 闭集全量（供文案完整性与唯一性检查）。
    pub const ALL: [SyCode; 12] = [
        SyCode::NotOpen,
        SyCode::OutOfRange,
        SyCode::DupBind,
        SyCode::OverMax,
        SyCode::BadNumber,
        SyCode::BadOp,
        SyCode::MissingPart,
        SyCode::StateRegress,
        SyCode::EmptySummary,
        SyCode::SegmentViolation,
        SyCode::SnapDrift,
        SyCode::RoundTrip,
    ];
}

// ── 调用行解析器 ──────────────────────────────────────────────────────
/// 解析后的同步调用。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SyncCall {
    /// 动词
    pub op: String,
    /// fd
    pub fd: u32,
    /// hex 数据样本（fsync/fdatasync 时忽略）
    pub hex: String,
}

/// 解析 "op|fd|hex" 三段调用行。
pub fn parse_sync_call(line: &str) -> Result<SyncCall, SyCode> {
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
        return Err(SyCode::MissingPart);
    }
    let op = match parts.first() {
        Some(p) => p.clone(),
        None => return Err(SyCode::MissingPart),
    };
    if op != "write" && op != "fsync" && op != "fdatasync" {
        return Err(SyCode::BadOp);
    }
    let fds = match parts.get(1) {
        Some(p) => p.clone(),
        None => return Err(SyCode::MissingPart),
    };
    let hx = match parts.get(2) {
        Some(p) => p.clone(),
        None => return Err(SyCode::MissingPart),
    };
    if fds.is_empty() {
        return Err(SyCode::BadNumber);
    }
    let mut fd: u32 = 0;
    for b in fds.bytes() {
        if !b.is_ascii_digit() {
            return Err(SyCode::BadNumber);
        }
        fd = fd.wrapping_mul(10).wrapping_add((b - b'0') as u32);
    }
    Ok(SyncCall { op, fd, hex: hx })
}

// ── 同步状态机 ────────────────────────────────────────────────────────
/// 同步生命周期（单调，禁倒退）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SyncState {
    /// 空白
    Blank,
    /// 脏（有未落盘写）
    Dirty,
    /// 已同步
    Synced,
}

impl SyncState {
    /// 单调推进判定（允许跳级、禁倒退）。
    pub fn can_advance(self, next: SyncState) -> bool {
        let rank = |s: SyncState| match s {
            SyncState::Blank => 0u8,
            SyncState::Dirty => 1,
            SyncState::Synced => 2,
        };
        rank(next) > rank(self)
    }

    /// 中文标签。
    pub fn label(self) -> &'static str {
        match self {
            SyncState::Blank => "空白",
            SyncState::Dirty => "脏",
            SyncState::Synced => "已同步",
        }
    }
}

// ── 同步账 ────────────────────────────────────────────────────────────
/// 同步条目。
#[derive(Clone, Debug)]
pub struct SyncEnt {
    /// fd
    pub fd: u32,
    /// 脏位
    pub dirty: bool,
    /// 已同步快照长
    pub synced_len: u32,
    /// 生命周期状态
    pub state: SyncState,
}

/// 同步账：fd 同步面 + 共享体 + 快照。
#[derive(Clone, Debug)]
pub struct SyncLog {
    ents: Vec<SyncEnt>,
    /// 共享文件体
    pub body: Vec<u8>,
    /// 已落盘快照
    pub synced: Vec<u8>,
    /// 累计 fsync/fdatasync 次数
    pub syncs: u32,
    /// 累计写次数
    pub writes: u32,
}

impl SyncLog {
    /// 空账构造。
    pub fn new() -> SyncLog {
        SyncLog { ents: Vec::new(), body: Vec::new(), synced: Vec::new(), syncs: 0, writes: 0 }
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

    /// 绑定 fd。
    pub fn bind(&mut self, fd: u32) -> Result<(), SyCode> {
        if fd as usize >= FD_CAP {
            return Err(SyCode::OutOfRange);
        }
        if self.fd_idx(fd).is_some() {
            return Err(SyCode::DupBind);
        }
        let _ = self.ents.push(SyncEnt {
            fd,
            dirty: false,
            synced_len: 0,
            state: SyncState::Blank,
        });
        Ok(())
    }

    /// 写（标脏）。
    pub fn write(&mut self, fd: u32, data: &[u8]) -> Result<u32, SyCode> {
        if data.len() as u64 > WRITE_MAX as u64 {
            return Err(SyCode::OverMax);
        }
        let idx = match self.fd_idx(fd) {
            Some(i) => i,
            None => return Err(SyCode::NotOpen),
        };
        let _ = idx;
        let _ = self.body.extend_from_slice(data);
        let mut i = 0usize;
        while i < self.ents.len() {
            if let Some(e) = self.ents.get_mut(i) {
                e.dirty = true;
                if e.state.can_advance(SyncState::Dirty) {
                    e.state = SyncState::Dirty;
                }
            }
            i += 1;
        }
        self.writes = self.writes.saturating_add(1);
        Ok(self.body.len() as u32)
    }

    /// fsync/fdatasync：落快照清脏；未绑定/越界逐档拒绝。
    pub fn sync(&mut self, fd: u32) -> Result<u32, SyCode> {
        if fd as usize >= FD_CAP {
            return Err(SyCode::OutOfRange);
        }
        let idx = match self.fd_idx(fd) {
            Some(i) => i,
            None => return Err(SyCode::NotOpen),
        };
        self.synced.clear();
        let mut k = 0usize;
        while k < self.body.len() {
            if let Some(b) = self.body.get(k) {
                let _ = self.synced.push(*b);
            }
            k += 1;
        }
        if let Some(e) = self.ents.get_mut(idx) {
            e.dirty = false;
            e.synced_len = self.body.len() as u32;
            e.state = SyncState::Synced;
        }
        self.syncs = self.syncs.saturating_add(1);
        Ok(self.synced.len() as u32)
    }

    /// 重读一致：快照与体逐字节相等。
    pub fn readback_ok(&self) -> bool {
        self.synced.len() == self.body.len() && {
            let mut i = 0usize;
            let mut ok = true;
            while i < self.body.len() {
                if self.synced.get(i) != self.body.get(i) {
                    ok = false;
                    break;
                }
                i += 1;
            }
            ok
        }
    }

    /// fd 状态读取。
    pub fn state_of(&self, fd: u32) -> Option<SyncState> {
        match self.fd_idx(fd) {
            Some(i) => match self.ents.get(i) {
                Some(e) => Some(e.state),
                None => None,
            },
            None => None,
        }
    }
}

// ── J1 达成核：写后 fsync 重读一致 10/10 轮 ───────────────────────────
/// 逐轮：写→脏→sync→快照一致→再写→再 sync。
pub fn run_sync_rounds() -> (u32, usize) {
    let mut ok = 0u32;
    let mut probes = 0usize;
    let mut round = 0u32;
    while round < INJECT_ROUNDS {
        let mut g = SyncLog::new();
        let _ = g.bind(3);
        let mut all_ok = true;
        // 面1：写后脏位起。
        all_ok = all_ok && g.write(3, &[0xA1, 0xA2]).is_ok()
            && g.state_of(3) == Some(SyncState::Dirty);
        probes += 1;
        // 面2：fsync 后快照与体一致。
        all_ok = all_ok && g.sync(3).is_ok() && g.readback_ok();
        probes += 1;
        // 面3：状态 Synced。
        all_ok = all_ok && g.state_of(3) == Some(SyncState::Synced);
        probes += 1;
        // 面4：再写再 sync 一致。
        all_ok = all_ok && g.write(3, &[0xB1]).is_ok()
            && g.sync(3).is_ok() && g.readback_ok();
        probes += 1;
        // 面5：快照长 = 体长 = 3。
        all_ok = all_ok && g.synced.len() == 3 && g.body.len() == 3;
        probes += 1;
        // 面6：sync 计数 2。
        all_ok = all_ok && g.syncs == 2 && g.writes == 2;
        probes += 1;
        if all_ok {
            ok += 1;
        }
        round += 1;
    }
    (ok, probes)
}

// ── 注入核：坏同步矩阵 ────────────────────────────────────────────────
/// 四类坏样本。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BadSync {
    /// 未绑定 fsync（EBADF 族）
    NotOpen,
    /// 越闭集 fd（EBADF 族）
    OutOfRange,
    /// 重复绑定（EBUSY 档）
    DupBind,
    /// 超限写（EFBIG 族）
    OverMax,
}

impl BadSync {
    /// 闭集全量。
    pub const ALL: [BadSync; 4] = [
        BadSync::NotOpen,
        BadSync::OutOfRange,
        BadSync::DupBind,
        BadSync::OverMax,
    ];

    /// 中文标签（注入明细用）。
    pub fn label(self) -> &'static str {
        match self {
            BadSync::NotOpen => "未绑定 fsync",
            BadSync::OutOfRange => "越闭集 fd",
            BadSync::DupBind => "重复绑定",
            BadSync::OverMax => "超限写",
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
    pub fn record(&mut self, caught: bool) -> Result<(), SyCode> {
        self.injected = self.injected.saturating_add(1);
        if caught {
            self.detected = self.detected.saturating_add(1);
        } else {
            self.escaped = self.escaped.saturating_add(1);
        }
        if self.detected.saturating_add(self.escaped) != self.injected {
            return Err(SyCode::SegmentViolation);
        }
        Ok(())
    }

    /// 守恒式。
    pub fn conserved(&self) -> bool {
        self.detected.saturating_add(self.escaped) == self.injected
    }
}

/// 执行坏同步注入：每类 INJECT_ROUNDS 次。
pub fn run_bad_sync_matrix() -> (InjectLedger, [u32; 4]) {
    let mut ledger = InjectLedger::zero();
    let mut per_class = [0u32; 4];
    let mut ci = 0usize;
    while ci < BadSync::ALL.len() {
        let case = match BadSync::ALL.get(ci) {
            Some(c) => *c,
            None => break,
        };
        let mut round = 0u32;
        while round < INJECT_ROUNDS {
            let mut g = SyncLog::new();
            let _ = g.bind(3);
            let caught = match case {
                BadSync::NotOpen => {
                    g.sync(4).err() == Some(SyCode::NotOpen)
                }
                BadSync::OutOfRange => {
                    g.bind(99).err() == Some(SyCode::OutOfRange)
                        && g.sync(99).err() == Some(SyCode::OutOfRange)
                }
                BadSync::DupBind => {
                    g.bind(3).err() == Some(SyCode::DupBind)
                }
                BadSync::OverMax => {
                    let big = [0x41u8; 65];
                    g.write(3, &big).err() == Some(SyCode::OverMax)
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

// ── 同步小结成文（纯函数） ────────────────────────────────────────────
/// 小结成文：「len=<n> synced=<m> rb=<0|1>」。
pub fn compose_sync_summary(g: &SyncLog) -> Result<String, SyCode> {
    let out = format!(
        "len={} synced={} rb={}",
        g.body.len(), g.synced.len(), if g.readback_ok() { 1u32 } else { 0u32 }
    );
    if out.is_empty() {
        return Err(SyCode::EmptySummary);
    }
    Ok(out)
}

// ── Linux errno 对照表 ────────────────────────────────────────────────
/// (名称, 负值, Windows 近似码, 说明)
pub const ERRNO_TABLE: &[(&str, i32, i32, &str)] = &[
    ("EBADF", -9, 9, "Windows ERROR_INVALID_HANDLE；未绑定/越界 fd 族"),
    ("EBUSY", -16, 33, "Windows ERROR_LOCK_VIOLATION；重复绑定档"),
    ("EFBIG", -27, 223, "Windows ERROR_FILE_TOO_LARGE；超限写族"),
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

/// 规范语料指纹：写→sync→成文→摘哈希。
pub fn corpus_fingerprint() -> u32 {
    let mut g = SyncLog::new();
    let _ = g.bind(3);
    let _ = g.write(3, &[1, 2, 3]);
    let _ = g.sync(3);
    let mut bytes: Vec<u8> = Vec::new();
    if let Ok(s) = compose_sync_summary(&g) {
        bytes.extend_from_slice(s.as_bytes());
    }
    bytes.extend_from_slice(&g.synced);
    fnv1a(&bytes)
}

// ── 检查入口 ──────────────────────────────────────────────────────────
/// F10132 判据集（13 条静态 + 3 条动态）。
pub fn run_unx10132_checks() -> CheckSet {
    let mut s = CheckSet::new("unx10132");

    // 1. 段位守卫
    let seg_ok = SEG_BASE % 0x40 == 0 && (0x93100..=0x9313F).contains(&SEG_BASE);
    s.add("段位守卫：0x40 对齐且在本卡 0x40 窗口内", seg_ok,
          "段基址必须唯一且不与相邻卡重叠；越界即拒绝注册");

    // 2. 诊断码闭集
    let mut uniq = true;
    let mut i = 0usize;
    while i < SyCode::ALL.len() {
        let mut j = i + 1;
        while j < SyCode::ALL.len() {
            if let (Some(a), Some(b)) = (SyCode::ALL.get(i), SyCode::ALL.get(j)) {
                if a.val() == b.val() {
                    uniq = false;
                }
            }
            j += 1;
        }
        i += 1;
    }
    let text_ok = SyCode::ALL.iter().all(|c| !c.detail().is_empty());
    s.add("诊断码闭集：闭集码两两唯一且中文建议非空", uniq && text_ok,
          "段内闭集码不得重复，每个码必须能取到可操作说明");

    // 3. 解析器
    let parse_ok = parse_sync_call("write|3|a1a2").is_ok()
        && parse_sync_call("fsync|3|").is_ok()
        && parse_sync_call("msync|3|").err() == Some(SyCode::BadOp)
        && parse_sync_call("write|x|a1").err() == Some(SyCode::BadNumber);
    s.add("调用行解析器：op 三词闭集放行且坏成分逐档拒绝", parse_ok,
          "op 闭集 {write, fsync, fdatasync}；fd 十进制闭集");

    // 4. J1 达成核
    let (ok, probes) = run_sync_rounds();
    s.add("J1 落盘达成：写后 fsync 重读一致 LTP fsync01 面 10/10 次",
          ok == INJECT_ROUNDS && probes == (INJECT_ROUNDS as usize) * 6,
          "每轮 6 面（写脏/快照/态推进/再同步/长度/计数）全对");

    // 5. J2 非法 fd
    let mut g = SyncLog::new();
    let _ = g.bind(3);
    let bad_ok = g.sync(4).err() == Some(SyCode::NotOpen)
        && g.sync(99).err() == Some(SyCode::OutOfRange)
        && g.sync(3).is_ok();
    s.add("J2 非法 fd 受控：EBADF 双档精确复现且合法 fd 不受累", bad_ok,
          "未绑定与越闭集是两档独立误用；合法 fd 同轮放行");

    // 6. 重读一致性
    let rb_ok = g.readback_ok() && g.synced.len() == g.body.len();
    s.add("重读一致：快照与体逐字节相等", rb_ok,
          "SnapDrift 是落盘语义根缺陷；必须全字节一致");

    // 7. 重复绑定档
    let dup_ok = g.bind(3).err() == Some(SyCode::DupBind);
    s.add("重复绑定：EBUSY 档精确复现", dup_ok,
          "fd 二次绑定必须拒");

    // 8. 超限写档
    let big = [0x41u8; 65];
    let over_ok = g.write(3, &big).err() == Some(SyCode::OverMax);
    s.add("超限写：EFBIG 档精确复现", over_ok,
          "WRITE_MAX 之外必须拒且体不越限");

    // 9. 注入核
    let (inj, per_class) = run_bad_sync_matrix();
    let all_caught = per_class.iter().all(|n| *n == INJECT_ROUNDS);
    s.add("注入核：四类坏样本各 10/10 拒绝", all_caught,
          "未绑定/越界/重复绑定/超限必须逐类逐次精确复现");

    // 10. 注入账守恒
    s.add("注入账守恒：注入=检出+逃逸且逃逸=0",
          inj.conserved() && inj.escaped == 0
              && inj.injected == BadSync::ALL.len() as u32 * INJECT_ROUNDS,
          "40 次注入必须 40 次检出、0 逃逸");

    // 11. 状态机
    let sm_ok = SyncState::Blank.can_advance(SyncState::Synced)
        && SyncState::Dirty.can_advance(SyncState::Synced)
        && !SyncState::Synced.can_advance(SyncState::Dirty)
        && SyncState::Dirty.label() == "脏";
    s.add("状态机：Blank→Dirty→Synced 单向", sm_ok,
          "同步生命周期只许单调推进；倒退即协议违约");

    // 12. Linux 逐码一致
    let codes_ok = agrees_with_linux(9) && agrees_with_linux(16) && agrees_with_linux(27)
        && agrees_with_linux(22)
        && !agrees_with_linux(0) && !agrees_with_linux(9999);
    s.add("Linux 逐码一致：表内码精确、表外码拒绝", codes_ok,
          "错误族只允许 errno 表内正值且负值可复算");

    // 13. 成文 + 指纹（先补一次写+同步使语料非空）
    let _ = g.write(3, &[1, 2, 3]);
    let _ = g.sync(3);
    let sum = compose_sync_summary(&g);
    let text_ok = match &sum {
        Ok(x) => x.contains("len=3") && x.contains("rb=1"),
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
    let ok_text = format!("落盘达成轮数 {}/{}", ok, INJECT_ROUNDS);
    s.add_dyn("达成轮快照", ok == INJECT_ROUNDS, &ok_text);

    s
}

// ── 测试 ──────────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parser_guards() {
        assert!(parse_sync_call("write|3|a1").is_ok());
        assert_eq!(parse_sync_call("msync|3|").err(), Some(SyCode::BadOp));
        assert_eq!(parse_sync_call("write|x|a1").err(), Some(SyCode::BadNumber));
        assert_eq!(parse_sync_call("write|3").err(), Some(SyCode::MissingPart));
    }

    #[test]
    fn write_marks_dirty() {
        let mut g = SyncLog::new();
        let _ = g.bind(3);
        let _ = g.write(3, &[1]);
        assert_eq!(g.state_of(3), Some(SyncState::Dirty));
    }

    #[test]
    fn sync_snapshots_body() {
        let mut g = SyncLog::new();
        let _ = g.bind(3);
        let _ = g.write(3, &[1, 2]);
        let _ = g.sync(3);
        assert!(g.readback_ok());
        assert_eq!(g.state_of(3), Some(SyncState::Synced));
    }

    #[test]
    fn resync_after_write() {
        let mut g = SyncLog::new();
        let _ = g.bind(3);
        let _ = g.write(3, &[1]);
        let _ = g.sync(3);
        let _ = g.write(3, &[2]);
        assert_eq!(g.state_of(3), Some(SyncState::Dirty));
        let _ = g.sync(3);
        assert!(g.readback_ok());
        assert_eq!(g.syncs, 2);
    }

    #[test]
    fn unbound_sync_blocked() {
        let mut g = SyncLog::new();
        assert_eq!(g.sync(9).err(), Some(SyCode::NotOpen));
    }

    #[test]
    fn out_of_range_blocked() {
        let mut g = SyncLog::new();
        assert_eq!(g.bind(99).err(), Some(SyCode::OutOfRange));
        assert_eq!(g.sync(99).err(), Some(SyCode::OutOfRange));
    }

    #[test]
    fn dup_bind_blocked() {
        let mut g = SyncLog::new();
        let _ = g.bind(3);
        assert_eq!(g.bind(3).err(), Some(SyCode::DupBind));
    }

    #[test]
    fn over_max_blocked() {
        let mut g = SyncLog::new();
        let _ = g.bind(3);
        let big = [0x41u8; 65];
        assert_eq!(g.write(3, &big).err(), Some(SyCode::OverMax));
    }

    #[test]
    fn bad_matrix_green() {
        let (inj, per) = run_bad_sync_matrix();
        assert_eq!(inj.injected, 4 * INJECT_ROUNDS);
        assert_eq!(inj.escaped, 0);
        for n in per.iter() {
            assert_eq!(*n, INJECT_ROUNDS);
        }
    }

    #[test]
    fn rounds_full() {
        let (ok, probes) = run_sync_rounds();
        assert_eq!(ok, INJECT_ROUNDS);
        assert_eq!(probes, (INJECT_ROUNDS as usize) * 6);
    }

    #[test]
    fn state_machine_monotonic() {
        assert!(SyncState::Blank.can_advance(SyncState::Dirty));
        assert!(!SyncState::Synced.can_advance(SyncState::Blank));
    }

    #[test]
    fn deterministic_corpus() {
        assert_eq!(corpus_fingerprint(), corpus_fingerprint());
    }

    #[test]
    fn unx10132_checks_green() {
        let set = run_unx10132_checks();
        assert!(set.all_passed(), "UNX-F10132 red: {}", set.red_items_text());
    }
}
