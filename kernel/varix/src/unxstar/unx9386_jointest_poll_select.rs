//! UNX-F9386 多路复用四件套联测（poll/select/ppoll/pselect × C4 事件面） · 0x7CE00段
//! ─────────────────────────────────────────────────────────────────────
//! 锚点：UNX-F9386-J1 四件套联测判据过，逐件与 C4 事件面对接可观测，
//!       timeout 语义逐件一致格过。
//!
//! 设计要点：
//!   ① 事件面按「fd 就绪注册表 + 待决信号旗」建模；四件套
//!      （poll/select/ppoll/pselect）共享同一就绪判定内核（单源），
//!      差异只在 timeout 表达式与信号掩码两处。
//!   ② timeout 四式归一化解析器：poll 整数 ms（负值一律无限）、
//!      select timeval（NULL 无限；负值 EINVAL）、ppoll/pselect timespec
//!      （NULL 无限；tv_nsec ≥ 1e9 或负值 EINVAL）。归一化后只有
//!      {无限, 零, 有界 ns} 三态，四件套逐件等价可复算。
//!   ③ EINTR 判据：待决信号旗挂起且 timeout 非零（阻塞或有限等待）
//!      时回 EINTR；零超时（非阻塞）不吃信号（Linux 口径：非阻塞
//!      调用不被信号中断）。
//!   ④ 就绪账：四件套对同一事件面状态必须给出同一就绪数（单源）；
//!      零超时两次调用幂等；EBADF 坏 fd 先于就绪判定。
//!   ⑤ 注入核覆盖四类坏样本：非法 timespec、坏 fd、阻塞中待决信号、
//!      零超时与信号交互口径，逐类 10/10 检出且账守恒。
//! ─────────────────────────────────────────────────────────────────────

use alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（F9386 独占 0x40 窗口）。
pub const SEG_BASE: u32 = 0x7CE00;

/// tv_nsec 合法上界（≥1e9 即 EINVAL）。
pub const NSEC_LIMIT: u64 = 1_000_000_000;

/// 复测轮数（注入矩阵每类轮数）。
pub const INJECT_ROUNDS: u32 = 10;

/// Linux errno 正值口径。
pub const EBADF: i32 = 9;
pub const EINTR: i32 = 4;
pub const EINVAL: i32 = 22;

// ── Linux errno 对照表 ───────────────────────────────────────────────
/// (名称, 负值, Windows 近似码, 说明)。
pub const ERRNO_TABLE: &[(&str, i32, i32, &str)] = &[
    ("EINTR", -4, 995, "Windows ERROR_OPERATION_ABORTED；等待中断族"),
    ("EBADF", -9, 6, "Windows ERROR_INVALID_HANDLE；坏句柄族"),
    ("EINVAL", -22, 87, "Windows ERROR_INVALID_PARAMETER；坏时间值族"),
    ("ETIMEDOUT", -110, 1460, "Windows ERROR_TIMEOUT；有限等待超时族"),
];

/// 按 errno 正数查负值；表外返回 0。
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
    code > 0 && linux_neg(code) == -code
}

// ── 诊断码闭集 ────────────────────────────────────────────────────────
/// 本模块诊断码（段内 +0x01..=+0x0A 闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PsCode {
    /// 非法 timespec（tv_nsec 越界/负值）
    BadTimeSpec,
    /// 负 timeval
    NegativeTimeval,
    /// 集内坏 fd
    BadFdInSet,
    /// 阻塞中吃信号
    EintrBlocking,
    /// 零超时误吃信号
    EintrNonblocking,
    /// 四件套就绪数分歧
    ReadyDiverged,
    /// 零超时幂等破坏
    ZeroTimeoutNotIdempotent,
    /// 等待账倒退
    LedgerRegress,
    /// 双跑不一致
    NotReproducible,
    /// 段位越界
    SegmentViolation,
    /// 文案闭集为空
    EmptyDetail,
}

impl PsCode {
    /// 码值（SEG_BASE + 变体序号）。
    pub fn code(self) -> u32 {
        SEG_BASE + self as u32
    }

    /// 中文诊断（闭集返回，禁止空串）。
    pub fn detail(self) -> &'static str {
        match self {
            PsCode::BadTimeSpec => "非法 timespec 被拒：tv_nsec 必须 < 1e9 且非负；\
                请规范化到秒位后再调用",
            PsCode::NegativeTimeval => "负 timeval 被拒：select 的时间值不得为负；\
                请改用 NULL 表无限或清零表非阻塞",
            PsCode::BadFdInSet => "集内坏 fd 被拒：负号/未开/已关 fd 一律 EBADF；\
                就绪判定先验 fd 有效性",
            PsCode::EintrBlocking => "阻塞等待被信号中断：EINTR 透传给调用方；\
                请按 TEMP_FAILURE_RETRY 惯例重试",
            PsCode::EintrNonblocking => "零超时误吃信号：非阻塞调用不得被中断；\
                请检查信号旗判定是否泄漏到零超时路径",
            PsCode::ReadyDiverged => "四件套就绪数分歧：同一事件面状态必须给出\
                同一就绪数；就绪判定必须单源",
            PsCode::ZeroTimeoutNotIdempotent => "零超时幂等破坏：零超时两次调用\
                必须同结果；请排查隐藏状态",
            PsCode::LedgerRegress => "账倒退被拒：等待/判定计数只增不减；请检查\
                回滚路径是否误改累计值",
            PsCode::NotReproducible => "双跑不一致：同语料两次指纹不同；请排查\
                隐藏状态或非确定遍历",
            PsCode::SegmentViolation => "段位越界：段基址必须落在 \
                0x7CC00..=0x7D1C0 且 0x40 对齐",
            PsCode::EmptyDetail => "诊断文案为空：闭集每码必须携带非空中文建议",
        }
    }

    /// 闭集全量。
    pub const ALL: [PsCode; 11] = [
        PsCode::BadTimeSpec,
        PsCode::NegativeTimeval,
        PsCode::BadFdInSet,
        PsCode::EintrBlocking,
        PsCode::EintrNonblocking,
        PsCode::ReadyDiverged,
        PsCode::ZeroTimeoutNotIdempotent,
        PsCode::LedgerRegress,
        PsCode::NotReproducible,
        PsCode::SegmentViolation,
        PsCode::EmptyDetail,
    ];
}

// ── timeout 归一化解析器 ──────────────────────────────────────────────
/// 归一化 timeout 三态。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Timeout {
    /// 无限等待
    Infinite,
    /// 非阻塞立即返回
    Zero,
    /// 有界等待（ns）
    Finite(u64),
}

/// timespec（ppoll/pselect 口径）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct TimeSpec {
    /// 秒
    pub sec: u64,
    /// 纳秒（必须 < 1e9）
    pub nsec: u64,
}

impl TimeSpec {
    /// 校验：nsec < 1e9。
    pub fn valid(&self) -> bool {
        self.nsec < NSEC_LIMIT
    }

    /// 归一化 ns 总量。
    pub fn total_ns(&self) -> u64 {
        self.sec
            .saturating_mul(NSEC_LIMIT)
            .saturating_add(self.nsec)
    }
}

/// timeval（select 口径），usec 必须 < 1e6。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct TimeVal {
    /// 秒
    pub sec: u64,
    /// 微秒（必须 < 1e6）
    pub usec: u64,
}

impl TimeVal {
    /// 校验：usec < 1e6。
    pub fn valid(&self) -> bool {
        self.usec < 1_000_000
    }

    /// 归一化 ns。
    pub fn total_ns(&self) -> u64 {
        self.sec
            .saturating_mul(1_000_000_000)
            .saturating_add(self.usec.saturating_mul(1000))
    }
}

/// poll 整数 ms → 归一化（负值一律无限，0 非阻塞）。
pub fn parse_poll_ms(ms: i64) -> Timeout {
    if ms < 0 {
        Timeout::Infinite
    } else if ms == 0 {
        Timeout::Zero
    } else {
        Timeout::Finite((ms as u64).saturating_mul(1_000_000))
    }
}

/// select timeval 可选值 → 归一化（None 无限；非法值显性拒绝）。
pub fn parse_select_tv(tv: Option<TimeVal>) -> Result<Timeout, i32> {
    match tv {
        None => Ok(Timeout::Infinite),
        Some(t) => {
            if !t.valid() {
                Err(EINVAL)
            } else if t.total_ns() == 0 {
                Ok(Timeout::Zero)
            } else {
                Ok(Timeout::Finite(t.total_ns()))
            }
        }
    }
}

/// ppoll/pselect timespec 可选值 → 归一化（None 无限；非法值显性拒绝）。
pub fn parse_ppoll_ts(ts: Option<TimeSpec>) -> Result<Timeout, i32> {
    match ts {
        None => Ok(Timeout::Infinite),
        Some(t) => {
            if !t.valid() {
                Err(EINVAL)
            } else if t.total_ns() == 0 {
                Ok(Timeout::Zero)
            } else {
                Ok(Timeout::Finite(t.total_ns()))
            }
        }
    }
}

// ── C4 事件面 ─────────────────────────────────────────────────────────
/// 事件面：fd 就绪注册表 + 待决信号旗。
#[derive(Clone, Debug)]
pub struct EventFace {
    /// (fd, 就绪位)
    fds: Vec<(i32, bool)>,
    /// 待决信号旗
    signal_pending: bool,
    /// 累计判定次数（账只增）
    verdicts: u32,
    /// 累计 EINTR 次数
    eintrs: u32,
}

impl EventFace {
    /// 构造事件面：标准 fds 3..=6，就绪态按传入序列。
    pub fn new(ready: [bool; 4], signal_pending: bool) -> EventFace {
        let mut fds = Vec::new();
        let mut i = 0usize;
        while i < 4 {
            let _ = fds.push((3 + i as i32, ready[i]));
            i += 1;
        }
        EventFace { fds, signal_pending, verdicts: 0, eintrs: 0 }
    }

    /// fd 有效性：3..=6 且已登记。
    pub fn fd_valid(&self, fd: i32) -> bool {
        let mut i = 0usize;
        while i < self.fds.len() {
            if let Some((num, _)) = self.fds.get(i) {
                if *num == fd {
                    return true;
                }
            }
            i += 1;
        }
        false
    }

    /// fd 就绪位（无效 fd 返回 None）。
    pub fn ready_of(&self, fd: i32) -> Option<bool> {
        let mut i = 0usize;
        while i < self.fds.len() {
            if let Some((num, r)) = self.fds.get(i) {
                if *num == fd {
                    return Some(*r);
                }
            }
            i += 1;
        }
        None
    }

    /// 就绪判定内核（四件套单源）：返回就绪 fd 数；集内坏 fd 先验。
    pub fn ready_count(&self, set: &[i32]) -> Result<u32, i32> {
        let mut count = 0u32;
        let mut i = 0usize;
        while i < set.len() {
            match set.get(i) {
                Some(fd) => match self.ready_of(*fd) {
                    Some(true) => count = count.saturating_add(1),
                    Some(false) => {}
                    None => return Err(EBADF),
                },
                None => return Err(EBADF),
            }
            i += 1;
        }
        Ok(count)
    }

    /// 待决信号旗。
    pub fn has_signal(&self) -> bool {
        self.signal_pending
    }

    /// 判定账只增。
    fn note_verdict(&mut self) {
        self.verdicts = self.verdicts.saturating_add(1);
    }

    /// 记一次 EINTR。
    fn note_eintr(&mut self) {
        self.eintrs = self.eintrs.saturating_add(1);
    }

    /// 统一等待入口（单源）：四件套共用；EINTR 只在阻塞/有限等待时发生。
    pub fn wait(&mut self, set: &[i32], to: Timeout) -> Result<u32, i32> {
        let count = self.ready_count(set)?;
        self.note_verdict();
        if count > 0 {
            return Ok(count);
        }
        match to {
            Timeout::Zero => Ok(0),
            Timeout::Infinite | Timeout::Finite(_) => {
                if self.signal_pending {
                    self.note_eintr();
                    return Err(EINTR);
                }
                Ok(0)
            }
        }
    }

    /// 判定账。
    pub fn verdicts(&self) -> u32 {
        self.verdicts
    }

    /// EINTR 账。
    pub fn eintrs(&self) -> u32 {
        self.eintrs
    }
}

// ── 四件套包装（各自 timeout 表达式，共用 wait 内核） ─────────────────
/// poll(fds, ms)：负 ms 无限。
pub fn do_poll(face: &mut EventFace, set: &[i32], ms: i64) -> Result<u32, i32> {
    face.wait(set, parse_poll_ms(ms))
}

/// select(tv)：NULL 无限，负值由 TimeVal 类型排除（u64），非法 usec EINVAL。
pub fn do_select(face: &mut EventFace, set: &[i32], tv: Option<TimeVal>) -> Result<u32, i32> {
    let to = parse_select_tv(tv)?;
    face.wait(set, to)
}

/// ppoll(ts)：NULL 无限，nsec ≥ 1e9 EINVAL。
pub fn do_ppoll(face: &mut EventFace, set: &[i32], ts: Option<TimeSpec>) -> Result<u32, i32> {
    let to = parse_ppoll_ts(ts)?;
    face.wait(set, to)
}

/// pselect(ts)：同 ppoll；信号旗待决即中断（掩码模型：旗未被屏蔽）。
pub fn do_pselect(face: &mut EventFace, set: &[i32], ts: Option<TimeSpec>) -> Result<u32, i32> {
    let to = parse_ppoll_ts(ts)?;
    face.wait(set, to)
}

// ── 注入账与注入核 ────────────────────────────────────────────────────
/// 注入账：注入 = 检出 + 逃逸。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct InjectLedger {
    pub injected: u32,
    pub detected: u32,
    pub escaped: u32,
}

impl InjectLedger {
    const fn zero() -> InjectLedger {
        InjectLedger { injected: 0, detected: 0, escaped: 0 }
    }

    /// 记一次注入结果。
    pub fn record(&mut self, caught: bool) {
        self.injected = self.injected.saturating_add(1);
        if caught {
            self.detected = self.detected.saturating_add(1);
        } else {
            self.escaped = self.escaped.saturating_add(1);
        }
    }

    /// 守恒式。
    pub fn conserved(&self) -> bool {
        self.detected.saturating_add(self.escaped) == self.injected
    }
}

/// 四类坏样本。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BadTsCase {
    /// 非法 timespec（nsec = 1e9）
    BadNsec,
    /// 集内坏 fd
    BadFd,
    /// 阻塞中待决信号 → EINTR
    SignalDuringBlock,
    /// 零超时不吃信号
    ZeroNoEintr,
}

impl BadTsCase {
    /// 闭集全量。
    pub const ALL: [BadTsCase; 4] = [
        BadTsCase::BadNsec,
        BadTsCase::BadFd,
        BadTsCase::SignalDuringBlock,
        BadTsCase::ZeroNoEintr,
    ];
}

/// 注入核：四类坏样本各注入 INJECT_ROUNDS 次，逐类断言口径正确。
pub fn run_ts_injection() -> (InjectLedger, [u32; 4]) {
    let mut ledger = InjectLedger::zero();
    let mut per_class = [0u32; 4];
    let mut ci = 0usize;
    while ci < BadTsCase::ALL.len() {
        let case = match BadTsCase::ALL.get(ci) {
            Some(c) => *c,
            None => break,
        };
        let mut round = 0u32;
        while round < INJECT_ROUNDS {
            let caught = match case {
                BadTsCase::BadNsec => {
                    let bad = TimeSpec { sec: 0, nsec: NSEC_LIMIT };
                    parse_ppoll_ts(Some(bad)) == Err(EINVAL)
                        && parse_select_tv(Some(TimeVal { sec: 0, usec: 1_000_000 }))
                            == Err(EINVAL)
                }
                BadTsCase::BadFd => {
                    let mut face = EventFace::new([false, false, false, false], false);
                    face.wait(&[3, 42], Timeout::Zero) == Err(EBADF)
                }
                BadTsCase::SignalDuringBlock => {
                    let mut face = EventFace::new([false, false, false, false], true);
                    face.wait(&[3], Timeout::Infinite) == Err(EINTR)
                        && face.wait(&[3], Timeout::Finite(5)) == Err(EINTR)
                }
                BadTsCase::ZeroNoEintr => {
                    let mut face = EventFace::new([false, false, false, false], true);
                    face.wait(&[3], Timeout::Zero) == Ok(0)
                }
            };
            ledger.record(caught);
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

// ── FNV-1a 双跑指纹 ───────────────────────────────────────────────────
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

/// 规范语料指纹：四件套全语料 + 等价对拍 + 注入逐格落语料。
pub fn corpus_fingerprint() -> u32 {
    let mut bytes: Vec<u8> = Vec::new();
    // 场景 A：fd3 就绪 → 四件套同就绪数 1
    let mut fa = EventFace::new([true, false, false, false], false);
    let r1 = do_poll(&mut fa, &[3, 4], -1);
    let r2 = do_select(&mut fa, &[3, 4], None);
    let r3 = do_ppoll(&mut fa, &[3, 4], None);
    let r4 = do_pselect(&mut fa, &[3, 4], None);
    for r in [r1, r2, r3, r4].iter() {
        bytes.push(match r {
            Ok(n) => (*n & 0xFF) as u8,
            Err(e) => (e & 0xFF) as u8,
        });
    }
    // 场景 B：全不就绪 + 零超时 → 四件套同 0
    let mut fb = EventFace::new([false, false, false, false], false);
    let s1 = do_poll(&mut fb, &[3], 0);
    let s2 = do_select(&mut fb, &[3], Some(TimeVal { sec: 0, usec: 0 }));
    let s3 = do_ppoll(&mut fb, &[3], Some(TimeSpec { sec: 0, nsec: 0 }));
    let s4 = do_pselect(&mut fb, &[3], Some(TimeSpec { sec: 0, nsec: 0 }));
    for r in [s1, s2, s3, s4].iter() {
        bytes.push(match r {
            Ok(n) => (*n & 0xFF) as u8,
            Err(e) => (e & 0xFF) as u8,
        });
    }
    // 场景 C：全不就绪 + 待决信号 + 有限等待 → 四件套同 EINTR
    let mut fc = EventFace::new([false, false, false, false], true);
    let t1 = do_poll(&mut fc, &[3], 10);
    let t2 = do_select(&mut fc, &[3], Some(TimeVal { sec: 0, usec: 1 }));
    let t3 = do_ppoll(&mut fc, &[3], Some(TimeSpec { sec: 0, nsec: 1 }));
    let t4 = do_pselect(&mut fc, &[3], Some(TimeSpec { sec: 0, nsec: 1 }));
    for r in [t1, t2, t3, t4].iter() {
        bytes.push(match r {
            Ok(n) => (*n & 0xFF) as u8,
            Err(e) => (e & 0xFF) as u8,
        });
    }
    // timeout 归一化等价：poll(50ms) ≡ ppoll(50ms)
    let p1 = parse_poll_ms(50);
    let p2 = parse_ppoll_ts(Some(TimeSpec { sec: 0, nsec: 50_000_000 }));
    let p2v = match p2 {
        Ok(t) => t,
        Err(_) => Timeout::Infinite,
    };
    bytes.push(if p1 == p2v { 1 } else { 0 });
    // 注入账
    let (inj, per) = run_ts_injection();
    bytes.push(inj.detected as u8);
    let mut ci = 0usize;
    while ci < per.len() {
        if let Some(n) = per.get(ci) {
            bytes.push(*n as u8);
        }
        ci += 1;
    }
    fnv1a(&bytes)
}

// ── 检查入口 ──────────────────────────────────────────────────────────
/// F9386 判据集（静态 ≥14 条 + 动态 ≤4 条）。
pub fn run_unx9386_checks() -> CheckSet {
    let mut s = CheckSet::new("unx9386");

    // 1. 段位守卫
    let seg_ok = SEG_BASE % 0x40 == 0 && (0x7CC00..=0x7D1C0).contains(&SEG_BASE);
    s.add("段位守卫：0x40 对齐且在 0x7CC00..=0x7D1C0", seg_ok,
          "段基址必须唯一且落在本批窗口；越界即拒绝注册");

    // 2. 场景 A：fd3 就绪 → 四件套同就绪数
    let mut fa = EventFace::new([true, false, false, false], false);
    let r1 = do_poll(&mut fa, &[3, 4], -1);
    let r2 = do_select(&mut fa, &[3, 4], None);
    let r3 = do_ppoll(&mut fa, &[3, 4], None);
    let r4 = do_pselect(&mut fa, &[3, 4], None);
    s.add("就绪对接：四件套同就绪数 1（单源内核）",
          r1 == Ok(1) && r2 == Ok(1) && r3 == Ok(1) && r4 == Ok(1),
          "同一事件面状态必须四件同判");

    // 3. 场景 B：全不就绪 + 零超时 → 四件套同 0
    let mut fb = EventFace::new([false, false, false, false], false);
    let z1 = do_poll(&mut fb, &[3], 0);
    let z2 = do_select(&mut fb, &[3], Some(TimeVal { sec: 0, usec: 0 }));
    let z3 = do_ppoll(&mut fb, &[3], Some(TimeSpec { sec: 0, nsec: 0 }));
    let z4 = do_pselect(&mut fb, &[3], Some(TimeSpec { sec: 0, nsec: 0 }));
    s.add("零超时：四件套同 0 且不吃信号",
          z1 == Ok(0) && z2 == Ok(0) && z3 == Ok(0) && z4 == Ok(0),
          "timeout 语义逐件一致格");

    // 4. 零超时幂等
    let i1 = do_poll(&mut fb, &[3], 0);
    let i2 = do_poll(&mut fb, &[3], 0);
    s.add("零超时幂等：两次调用同结果", i1 == i2 && i1 == Ok(0),
          "非阻塞路径不得有隐藏状态");

    // 5. 场景 C：待决信号 + 阻塞/有限 → 四件套同 EINTR
    let mut fc = EventFace::new([false, false, false, false], true);
    let e1 = do_poll(&mut fc, &[3], 10);
    let e2 = do_select(&mut fc, &[3], Some(TimeVal { sec: 0, usec: 1 }));
    let e3 = do_ppoll(&mut fc, &[3], Some(TimeSpec { sec: 0, nsec: 1 }));
    let e4 = do_pselect(&mut fc, &[3], Some(TimeSpec { sec: 0, nsec: 1 }));
    s.add("EINTR：阻塞与有限等待逐件同 EINTR",
          e1 == Err(EINTR) && e2 == Err(EINTR) && e3 == Err(EINTR) && e4 == Err(EINTR),
          "EINTR 判定与等待方式解耦（单源）");

    // 6. 零超时不吃信号
    let mut fz = EventFace::new([false, false, false, false], true);
    let z_ok = do_poll(&mut fz, &[3], 0) == Ok(0);
    s.add("零超时 × 信号：非阻塞调用不吃信号", z_ok,
          "EINTR 只属于阻塞路径");

    // 7. timeout 归一化等价：poll(50ms) ≡ ppoll(50ms)
    let eq_to = match parse_ppoll_ts(Some(TimeSpec { sec: 0, nsec: 50_000_000 })) {
        Ok(t) => parse_poll_ms(50) == t,
        Err(_) => false,
    };
    s.add("timeout 归一化：poll 50ms ≡ ppoll 50ms", eq_to,
          "四种表达式归一化后同态");

    // 8. 非法 timespec EINVAL
    let bad_ts = parse_ppoll_ts(Some(TimeSpec { sec: 0, nsec: NSEC_LIMIT })) == Err(EINVAL)
        && parse_select_tv(Some(TimeVal { sec: 0, usec: 1_000_000 })) == Err(EINVAL);
    s.add("时间值闭集：nsec ≥ 1e9 / usec ≥ 1e6 显性 EINVAL", bad_ts,
          "非法时间值不得滑入无限等待");

    // 9. 坏 fd EBADF 先于就绪判定
    let mut fg = EventFace::new([true, false, false, false], false);
    s.add("fd 守卫：集内坏 fd 一律 EBADF",
          fg.wait(&[3, 42], Timeout::Zero) == Err(EBADF),
          "fd 校验先于就绪判定");

    // 10. 注入核：四类坏样本各 10/10 检出
    let (inj, per_class) = run_ts_injection();
    let all_caught = per_class.iter().all(|n| *n == INJECT_ROUNDS);
    s.add("注入核：四类坏样本各 10/10 检出", all_caught,
          "非法时间值/坏 fd/阻塞吃信号/零超时口径逐类检出");

    // 11. 注入账守恒
    s.add("注入账守恒：注入=检出+逃逸且逃逸=0",
          inj.conserved() && inj.escaped == 0
              && inj.injected == 4 * INJECT_ROUNDS,
          "40 次注入必须 40 次检出、0 逃逸");

    // 12. 判定账只增：EINTR 账与判定账单调
    s.add("等待账单调：EINTR 4 次、判定 4 次（场景 C 面）",
          fc.eintrs() == 4 && fc.verdicts() == 4,
          "账只增不减，逐次判定各记一笔");

    // 13. Linux 逐码一致
    let codes_ok = agrees_with_linux(EINTR) && agrees_with_linux(EBADF)
        && agrees_with_linux(EINVAL) && !agrees_with_linux(0)
        && !agrees_with_linux(999) && linux_neg(110) == -110;
    s.add("Linux 逐码一致：表内码精确、表外码拒绝", codes_ok,
          "errno 表正值与负值逐项可复算");

    // 14. 双跑一致
    let a = corpus_fingerprint();
    let b = corpus_fingerprint();
    s.add("双跑一致：规范语料指纹相等", a == b,
          "同输入两次必须逐字段相等，隐藏状态即失败");

    // 15. 文案闭集
    let text_ok = PsCode::ALL.iter().all(|c| !c.detail().is_empty());
    s.add("文案闭集：全部诊断码带中文建议", text_ok,
          "每个码必须能取到非空且可操作的说明");

    // 16. 码值唯一
    let mut uniq = true;
    let mut i = 0usize;
    while i < PsCode::ALL.len() {
        let mut j = i + 1;
        while j < PsCode::ALL.len() {
            if let (Some(x), Some(y)) = (PsCode::ALL.get(i), PsCode::ALL.get(j)) {
                if x.code() == y.code() {
                    uniq = false;
                }
            }
            j += 1;
        }
        i += 1;
    }
    s.add("诊断码：11 个码值两两唯一", uniq,
          "段内闭集码不得重复，否则判据无法定位");

    // 动态明细（≤8）
    let fa_text = format!(
        "判定 {} / EINTR {} / 注入检出 {}",
        fa.verdicts(),
        fc.eintrs(),
        inj.detected
    );
    s.add_dyn("事件面快照", inj.conserved(), &fa_text);
    let fp_text = format!("语料指纹 0x{:08X}", a);
    s.add_dyn("指纹快照", a == b, &fp_text);

    s
}

// ── 测试 ──────────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn four_suite_agree_on_ready() {
        let mut f = EventFace::new([true, false, false, false], false);
        assert_eq!(do_poll(&mut f, &[3, 4], -1), Ok(1));
        assert_eq!(do_select(&mut f, &[3, 4], None), Ok(1));
        assert_eq!(do_ppoll(&mut f, &[3, 4], None), Ok(1));
        assert_eq!(do_pselect(&mut f, &[3, 4], None), Ok(1));
    }

    #[test]
    fn zero_timeout_returns_zero() {
        let mut f = EventFace::new([false, false, false, false], false);
        assert_eq!(do_poll(&mut f, &[3], 0), Ok(0));
        assert_eq!(do_select(&mut f, &[3], Some(TimeVal { sec: 0, usec: 0 })), Ok(0));
        assert_eq!(do_ppoll(&mut f, &[3], Some(TimeSpec { sec: 0, nsec: 0 })), Ok(0));
    }

    #[test]
    fn signal_interrupts_blocking_only() {
        let mut f = EventFace::new([false, false, false, false], true);
        assert_eq!(do_poll(&mut f, &[3], 5), Err(EINTR));
        assert_eq!(do_poll(&mut f, &[3], 0), Ok(0));
    }

    #[test]
    fn bad_nsec_einval() {
        assert_eq!(parse_ppoll_ts(Some(TimeSpec { sec: 0, nsec: 1_000_000_000 })), Err(EINVAL));
        assert!(parse_ppoll_ts(Some(TimeSpec { sec: 0, nsec: 999_999_999 })).is_ok());
    }

    #[test]
    fn bad_usec_einval() {
        assert_eq!(parse_select_tv(Some(TimeVal { sec: 0, usec: 1_000_000 })), Err(EINVAL));
        assert!(parse_select_tv(Some(TimeVal { sec: 1, usec: 999_999 })).is_ok());
    }

    #[test]
    fn poll_negative_ms_is_infinite() {
        assert_eq!(parse_poll_ms(-1), Timeout::Infinite);
        assert_eq!(parse_poll_ms(-999), Timeout::Infinite);
        assert_eq!(parse_poll_ms(0), Timeout::Zero);
    }

    #[test]
    fn bad_fd_in_set_ebadf() {
        let mut f = EventFace::new([false, false, false, false], false);
        assert_eq!(f.wait(&[3, 42], Timeout::Zero), Err(EBADF));
        assert_eq!(f.wait(&[-1], Timeout::Zero), Err(EBADF));
    }

    #[test]
    fn poll_ms_equals_ppoll_ns() {
        let a = parse_poll_ms(50);
        let b = parse_ppoll_ts(Some(TimeSpec { sec: 0, nsec: 50_000_000 }));
        assert_eq!(Ok(a), b);
    }

    #[test]
    fn injection_matrix_green() {
        let (l, per) = run_ts_injection();
        assert!(l.conserved());
        assert_eq!(l.escaped, 0);
        for n in per.iter() {
            assert_eq!(*n, INJECT_ROUNDS);
        }
    }

    #[test]
    fn ledger_monotonic() {
        let mut f = EventFace::new([false, false, false, false], true);
        let _ = f.wait(&[3], Timeout::Finite(1));
        let _ = f.wait(&[3], Timeout::Finite(1));
        assert_eq!(f.eintrs(), 2);
        assert_eq!(f.verdicts(), 2);
    }

    #[test]
    fn fingerprint_deterministic() {
        assert_eq!(corpus_fingerprint(), corpus_fingerprint());
    }

    #[test]
    fn unx9386_checks_green() {
        let s = run_unx9386_checks();
        assert!(s.all_passed(), "UNX-F9386 red: {}", s.red_items_text());
    }
}
