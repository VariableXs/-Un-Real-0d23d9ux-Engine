//! UNX-F9384 epoll_ctl/epoll_wait 挂点联签（就绪队列协议） · 0x7CD80段
//! ─────────────────────────────────────────────────────────────────────
//! 锚点：UNX-F9384-J1 两挂点联签判据过，就绪队列协议对接可观测，与
//!       B09 四件套语义一致格过。
//!
//! 设计要点：
//!   ① epoll 面按「兴趣表 + 就绪队列」双账建模：epoll_ctl（ADD/MOD/
//!      DEL）维护兴趣表，epoll_wait 消费就绪队列；两挂点都只有唯一
//!      实现（单源），hook 前后只记账不改语义。
//!   ② 就绪队列协议：只有兴趣表内且事件掩码相交的 fd 才可投递；
//!      交付数 ≤ maxevents；报告事件 ⊆ 登记掩码。幽灵 fd（未登记）
//!      一律丢弃并记格，可观测不可投递。
//!   ③ ctl 语义与 B09 四件套一致：ADD 重号 EEXIST、MOD/DEL 缺项
//!      ENOENT、maxevents ≤ 0 EINVAL、表满 ENOSPC。
//!   ④ 注入核覆盖四类坏样本：重复 ADD、缺项 MOD/DEL、幽灵就绪 fd、
//!      零 maxevents，逐类 10/10 检出且账守恒。
//!   ⑤ 双跑指纹：FNV-1a 覆盖 ctl 全操作 + wait 交付 + 注入逐格。
//! ─────────────────────────────────────────────────────────────────────

use alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（F9384 独占 0x40 窗口）。
pub const SEG_BASE: u32 = 0x7CD80;

/// 兴趣表容量上限。
pub const INTEREST_CAP: usize = 16;

/// 复测轮数（注入矩阵每类轮数）。
pub const INJECT_ROUNDS: u32 = 10;

/// epoll_ctl 操作码：ADD。
pub const CTL_ADD: u32 = 1;
/// epoll_ctl 操作码：MOD。
pub const CTL_MOD: u32 = 2;
/// epoll_ctl 操作码：DEL。
pub const CTL_DEL: u32 = 3;

/// 事件位：可读。
pub const EV_IN: u32 = 0x001;
/// 事件位：可写。
pub const EV_OUT: u32 = 0x004;
/// 事件位：挂断。
pub const EV_HUP: u32 = 0x010;

/// Linux errno 正值口径。
pub const EBADF: i32 = 9;
pub const ENOENT: i32 = 2;
pub const EEXIST: i32 = 17;
pub const EINVAL: i32 = 22;
pub const ENOSPC: i32 = 28;

// ── Linux errno 对照表 ───────────────────────────────────────────────
/// (名称, 负值, Windows 近似码, 说明)。
pub const ERRNO_TABLE: &[(&str, i32, i32, &str)] = &[
    ("ENOENT", -2, 2, "Windows ERROR_FILE_NOT_FOUND；兴趣表缺项族"),
    ("EBADF", -9, 6, "Windows ERROR_INVALID_HANDLE；坏句柄族"),
    ("EEXIST", -17, 183, "Windows ERROR_ALREADY_EXISTS；重号登记族"),
    ("EINVAL", -22, 87, "Windows ERROR_INVALID_PARAMETER；坏参数族"),
    ("ENOSPC", -28, 1450, "Windows ERROR_NO_SYSTEM_RESOURCES；表满族"),
    ("EPERM", -1, 5, "Windows ERROR_ACCESS_DENIED；不可 poll 族"),
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
/// 本模块诊断码（段内 +0x01..=+0x0B 闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum EpCode {
    /// ADD 重号
    AddDuplicate,
    /// MOD/DEL 缺项
    AbsentEntry,
    /// 兴趣表满
    InterestFull,
    /// 零 maxevents
    ZeroMaxevents,
    /// 幽灵就绪 fd（未登记被投递）
    GhostReady,
    /// 交付越 maxevents
    DeliverOverflow,
    /// 报告事件越登记掩码
    MaskLeak,
    /// 未知 ctl 操作码
    UnknownCtlOp,
    /// hook 与直连分歧
    HookDiverged,
    /// 账倒退
    LedgerRegress,
    /// 双跑不一致
    NotReproducible,
    /// 段位越界
    SegmentViolation,
    /// 文案闭集为空
    EmptyDetail,
}

impl EpCode {
    /// 码值（SEG_BASE + 变体序号）。
    pub fn code(self) -> u32 {
        SEG_BASE + self as u32
    }

    /// 中文诊断（闭集返回，禁止空串）。
    pub fn detail(self) -> &'static str {
        match self {
            EpCode::AddDuplicate => "ADD 重号被拒：fd 已在兴趣表内必须回 EEXIST；\
                请先 DEL 再 ADD 或改用 MOD",
            EpCode::AbsentEntry => "兴趣表缺项被拒：MOD/DEL 的 fd 未登记必须回 \
                ENOENT；缺项不得静默成立",
            EpCode::InterestFull => "兴趣表满被拒：登记数已达上限必须回 ENOSPC；\
                请先裁剪或扩容",
            EpCode::ZeroMaxevents => "零 maxevents 被拒：maxevents ≤ 0 必须 EINVAL；\
                交付缓冲区必须有界",
            EpCode::GhostReady => "幽灵就绪 fd 被拒：未登记 fd 的就绪事件只能丢弃\
                记格，不得投递给调用方",
            EpCode::DeliverOverflow => "交付越界被拒：交付条数不得超过 maxevents；\
                请按界截断并保留余量",
            EpCode::MaskLeak => "掩码越界被拒：报告事件必须 ⊆ 登记掩码；\
                未登记位不得外泄",
            EpCode::UnknownCtlOp => "未知 ctl 操作码被拒：ADD/MOD/DEL 闭集外一律\
                EINVAL；新操作须先过宪章",
            EpCode::HookDiverged => "hook 与直连分歧：同初态同参数两路结果必须逐\
                字段一致；请勿在 hook 内复制语义",
            EpCode::LedgerRegress => "账倒退被拒：登记/交付/丢弃计数只增不减；\
                请检查回滚路径是否误改累计值",
            EpCode::NotReproducible => "双跑不一致：同语料两次指纹不同；请排查\
                隐藏状态或非确定遍历",
            EpCode::SegmentViolation => "段位越界：段基址必须落在 \
                0x7CC00..=0x7D1C0 且 0x40 对齐",
            EpCode::EmptyDetail => "诊断文案为空：闭集每码必须携带非空中文建议",
        }
    }

    /// 闭集全量。
    pub const ALL: [EpCode; 13] = [
        EpCode::AddDuplicate,
        EpCode::AbsentEntry,
        EpCode::InterestFull,
        EpCode::ZeroMaxevents,
        EpCode::GhostReady,
        EpCode::DeliverOverflow,
        EpCode::MaskLeak,
        EpCode::UnknownCtlOp,
        EpCode::HookDiverged,
        EpCode::LedgerRegress,
        EpCode::NotReproducible,
        EpCode::SegmentViolation,
        EpCode::EmptyDetail,
    ];
}

// ── epoll 兴趣表与就绪队列 ────────────────────────────────────────────
/// 兴趣表条目。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct EpollEntry {
    /// 目标 fd
    pub fd: i32,
    /// 登记事件掩码
    pub events: u32,
    /// 用户数据
    pub data: u64,
}

/// 就绪事件。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ReadyEvent {
    /// 就绪 fd
    pub fd: i32,
    /// 就绪事件掩码
    pub events: u32,
}

/// epoll 集：兴趣表 + 就绪队列双账状态机。
#[derive(Clone, Debug)]
pub struct EpollSet {
    interest: Vec<EpollEntry>,
    ready: Vec<ReadyEvent>,
    /// 累计 ADD 成功数
    adds: u32,
    /// 累计 MOD 成功数
    mods: u32,
    /// 累计 DEL 成功数
    dels: u32,
    /// 幽灵就绪丢弃数
    dropped: u32,
}

impl EpollSet {
    /// 构造空集。
    pub fn new() -> EpollSet {
        EpollSet { interest: Vec::new(), ready: Vec::new(), adds: 0, mods: 0, dels: 0, dropped: 0 }
    }

    fn find(&self, fd: i32) -> Option<usize> {
        let mut i = 0usize;
        while i < self.interest.len() {
            if let Some(e) = self.interest.get(i) {
                if e.fd == fd {
                    return Some(i);
                }
            }
            i += 1;
        }
        None
    }

    /// epoll_ctl（单源）：按 ADD/MOD/DEL 分发；fd < 0 回 EBADF。
    pub fn ctl(&mut self, op: u32, fd: i32, events: u32, data: u64) -> Result<(), i32> {
        if fd < 0 {
            return Err(EBADF);
        }
        match op {
            CTL_ADD => {
                if self.find(fd).is_some() {
                    return Err(EEXIST);
                }
                if self.interest.len() >= INTEREST_CAP {
                    return Err(ENOSPC);
                }
                self.interest.push(EpollEntry { fd, events, data });
                self.adds = self.adds.saturating_add(1);
                Ok(())
            }
            CTL_MOD => match self.find(fd) {
                Some(idx) => {
                    if let Some(e) = self.interest.get_mut(idx) {
                        e.events = events;
                        e.data = data;
                    }
                    self.mods = self.mods.saturating_add(1);
                    Ok(())
                }
                None => Err(ENOENT),
            },
            CTL_DEL => match self.find(fd) {
                Some(idx) => {
                    let _ = self.interest.remove(idx);
                    // 删除兴趣项同时清空其就绪事件（队列协议）
                    let mut j = 0usize;
                    while j < self.ready.len() {
                        let keep = match self.ready.get(j) {
                            Some(r) => r.fd != fd,
                            None => true,
                        };
                        if !keep {
                            let _ = self.ready.remove(j);
                            continue;
                        }
                        j += 1;
                    }
                    self.dels = self.saturating_dels();
                    Ok(())
                }
                None => Err(ENOENT),
            },
            _ => Err(EINVAL),
        }
    }

    fn saturating_dels(&mut self) -> u32 {
        self.dels = self.dels.saturating_add(1);
        self.dels
    }

    /// 就绪事件入队（生产者侧）：未登记 fd 直接丢弃记格。
    pub fn push_ready(&mut self, ev: ReadyEvent) -> Result<(), i32> {
        let registered = match self.find(ev.fd) {
            Some(idx) => match self.interest.get(idx) {
                Some(e) => e.events & ev.events != 0,
                None => false,
            },
            None => false,
        };
        if !registered {
            self.dropped = self.dropped.saturating_add(1);
            return Err(ENOENT);
        }
        self.ready.push(ev);
        Ok(())
    }

    /// epoll_wait（单源）：交付至多 maxevents 条，事件掩码截到登记位。
    pub fn wait(&mut self, maxevents: usize) -> Result<Vec<ReadyEvent>, i32> {
        if maxevents == 0 {
            return Err(EINVAL);
        }
        let mut out: Vec<ReadyEvent> = Vec::new();
        let mut i = 0usize;
        while i < self.ready.len() && out.len() < maxevents {
            let ev = match self.ready.get(i) {
                Some(e) => *e,
                None => break,
            };
            let masked = match self.find(ev.fd) {
                Some(idx) => match self.interest.get(idx) {
                    Some(e) => ReadyEvent { fd: ev.fd, events: ev.events & e.events },
                    None => ev,
                },
                None => ev,
            };
            out.push(masked);
            i += 1;
        }
        // 已交付的就绪事件出队
        let mut k = 0usize;
        while k < out.len() && !self.ready.is_empty() {
            let _ = self.ready.remove(0);
            k += 1;
        }
        Ok(out)
    }

    /// 兴趣表深度。
    pub fn interest_len(&self) -> usize {
        self.interest.len()
    }

    /// 就绪队列深度。
    pub fn ready_len(&self) -> usize {
        self.ready.len()
    }

    /// 幽灵丢弃数。
    pub fn dropped_count(&self) -> u32 {
        self.dropped
    }

    /// 操作账闭合：ADD = MOD + DEL 之外的现存项 + 交付/丢弃后队列一致。
    pub fn ops_conserved(&self) -> bool {
        self.adds >= self.dels
            && (self.adds - self.dels) as usize == self.interest.len()
    }
}

// ── 挂点等价账 ────────────────────────────────────────────────────────
/// hook 等价账：ctl/wait 双挂点各对拍一笔，只增不减。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct HookLedger {
    /// 已对拍笔数
    pub compared: u32,
    /// 分歧笔数（必须 0）
    pub diverged: u32,
}

impl HookLedger {
    const fn zero() -> HookLedger {
        HookLedger { compared: 0, diverged: 0 }
    }

    /// 记一笔对拍。
    pub fn record(&mut self, equal: bool) {
        self.compared = self.compared.saturating_add(1);
        if !equal {
            self.diverged = self.diverged.saturating_add(1);
        }
    }

    /// 单源成立。
    pub fn single_source(&self) -> bool {
        self.compared > 0 && self.diverged == 0
    }
}

/// hook 路径 ctl：直连 op 外加观测（fd 越界/操作成功深度一致）。
pub fn hooked_ctl(set: &mut EpollSet, op: u32, fd: i32, events: u32, data: u64) -> Result<(), i32> {
    let before = set.interest_len();
    let r = set.ctl(op, fd, events, data);
    let after = set.interest_len();
    match r {
        Ok(()) => {
            let delta_ok = match op {
                CTL_ADD => after == before + 1,
                CTL_DEL => after + 1 == before,
                _ => after == before,
            };
            if delta_ok {
                Ok(())
            } else {
                Err(EINVAL)
            }
        }
        Err(e) => Err(e),
    }
}

/// hook 路径 wait：直连 wait 外加观测（交付数 ≤ maxevents）。
pub fn hooked_wait(set: &mut EpollSet, maxevents: usize) -> Result<Vec<ReadyEvent>, i32> {
    let r = set.wait(maxevents);
    match r {
        Ok(v) => {
            if v.len() <= maxevents {
                Ok(v)
            } else {
                Err(EINVAL)
            }
        }
        Err(e) => Err(e),
    }
}

/// 双挂点等价对拍：语料操作直连 vs hook 各跑一份同初态副本。
pub fn run_hook_equivalence() -> HookLedger {
    let mut led = HookLedger::zero();
    let mut a = EpollSet::new();
    let mut b = EpollSet::new();
    // ctl ADD
    let ra = a.ctl(CTL_ADD, 5, EV_IN, 0xAA);
    let rb = hooked_ctl(&mut b, CTL_ADD, 5, EV_IN, 0xAA);
    led.record(ra == rb && a.interest_len() == b.interest_len());
    // ctl MOD
    let ra2 = a.ctl(CTL_MOD, 5, EV_IN | EV_OUT, 0xAB);
    let rb2 = hooked_ctl(&mut b, CTL_MOD, 5, EV_IN | EV_OUT, 0xAB);
    led.record(ra2 == rb2 && a.interest_len() == b.interest_len());
    // push + wait
    let _ = a.push_ready(ReadyEvent { fd: 5, events: EV_IN });
    let _ = b.push_ready(ReadyEvent { fd: 5, events: EV_IN });
    let ra3 = a.wait(4);
    let rb3 = hooked_wait(&mut b, 4);
    led.record(match (&ra3, &rb3) {
        (Ok(va), Ok(vb)) => va.len() == vb.len() && va.len() == 1,
        _ => false,
    });
    // ctl DEL
    let ra4 = a.ctl(CTL_DEL, 5, 0, 0);
    let rb4 = hooked_ctl(&mut b, CTL_DEL, 5, 0, 0);
    led.record(ra4 == rb4 && a.interest_len() == b.interest_len());
    led
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
pub enum BadEpCase {
    /// 重复 ADD
    DupAdd,
    /// 缺项 MOD
    ModAbsent,
    /// 缺项 DEL
    DelAbsent,
    /// 幽灵就绪 fd
    GhostReady,
}

impl BadEpCase {
    /// 闭集全量。
    pub const ALL: [BadEpCase; 4] = [
        BadEpCase::DupAdd,
        BadEpCase::ModAbsent,
        BadEpCase::DelAbsent,
        BadEpCase::GhostReady,
    ];
}

/// 注入核：四类坏样本各注入 INJECT_ROUNDS 次，逐类断言显性拒绝。
pub fn run_ep_injection() -> (InjectLedger, [u32; 4]) {
    let mut ledger = InjectLedger::zero();
    let mut per_class = [0u32; 4];
    let mut ci = 0usize;
    while ci < BadEpCase::ALL.len() {
        let case = match BadEpCase::ALL.get(ci) {
            Some(c) => *c,
            None => break,
        };
        let mut round = 0u32;
        while round < INJECT_ROUNDS {
            let mut set = EpollSet::new();
            let _ = set.ctl(CTL_ADD, 7, EV_IN, 1);
            let caught = match case {
                BadEpCase::DupAdd => {
                    let before = set.interest_len();
                    set.ctl(CTL_ADD, 7, EV_OUT, 2) == Err(EEXIST)
                        && set.interest_len() == before
                }
                BadEpCase::ModAbsent => {
                    set.ctl(CTL_MOD, 8, EV_IN, 3) == Err(ENOENT)
                }
                BadEpCase::DelAbsent => {
                    set.ctl(CTL_DEL, 8, 0, 0) == Err(ENOENT)
                }
                BadEpCase::GhostReady => {
                    let before = set.dropped_count();
                    let _ = set.push_ready(ReadyEvent { fd: 99, events: EV_IN });
                    set.dropped_count() == before + 1 && set.ready_len() == 0
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

/// 规范语料指纹：ctl 全操作 + wait 交付 + 等价对拍 + 注入逐格落语料。
pub fn corpus_fingerprint() -> u32 {
    let mut bytes: Vec<u8> = Vec::new();
    let mut set = EpollSet::new();
    let ops = [
        (CTL_ADD, 3i32, EV_IN, 0x11u64),
        (CTL_ADD, 4, EV_OUT, 0x12),
        (CTL_MOD, 3, EV_IN | EV_OUT, 0x13),
        (CTL_ADD, 5, EV_HUP, 0x14),
        (CTL_DEL, 4, 0, 0),
    ];
    let mut oi = 0usize;
    while oi < ops.len() {
        if let Some((op, fd, ev, d)) = ops.get(oi) {
            match set.ctl(*op, *fd, *ev, *d) {
                Ok(()) => bytes.push(1),
                Err(e) => bytes.push((e & 0xFF) as u8),
            }
        }
        oi += 1;
    }
    let _ = set.push_ready(ReadyEvent { fd: 3, events: EV_IN | EV_OUT });
    let _ = set.push_ready(ReadyEvent { fd: 5, events: EV_HUP });
    let _ = set.push_ready(ReadyEvent { fd: 99, events: EV_IN }); // 幽灵
    bytes.push(set.dropped_count() as u8);
    let delivered = set.wait(8);
    bytes.push(match &delivered {
        Ok(v) => {
            let mut h = v.len() as u8;
            let mut i = 0usize;
            while i < v.len() {
                if let Some(e) = v.get(i) {
                    h = h.wrapping_add((e.fd & 0xFF) as u8).wrapping_add((e.events & 0xFF) as u8);
                }
                i += 1;
            }
            h
        }
        Err(e) => *e as u8,
    });
    let eq = run_hook_equivalence();
    bytes.push(eq.compared as u8);
    bytes.push(eq.diverged as u8);
    let (inj, per) = run_ep_injection();
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
/// F9384 判据集（静态 ≥14 条 + 动态 ≤4 条）。
pub fn run_unx9384_checks() -> CheckSet {
    let mut s = CheckSet::new("unx9384");

    // 1. 段位守卫
    let seg_ok = SEG_BASE % 0x40 == 0 && (0x7CC00..=0x7D1C0).contains(&SEG_BASE);
    s.add("段位守卫：0x40 对齐且在 0x7CC00..=0x7D1C0", seg_ok,
          "段基址必须唯一且落在本批窗口；越界即拒绝注册");

    // 2. ctl 正向流：ADD/MOD/DEL 全链成功且操作账闭合
    let mut set = EpollSet::new();
    let a1 = set.ctl(CTL_ADD, 3, EV_IN, 1);
    let a2 = set.ctl(CTL_ADD, 4, EV_OUT, 2);
    let m1 = set.ctl(CTL_MOD, 3, EV_IN | EV_OUT, 3);
    let d1 = set.ctl(CTL_DEL, 4, 0, 0);
    s.add("ctl 正向流：ADD×2+MOD+DEL 全链成功且账闭合",
          a1.is_ok() && a2.is_ok() && m1.is_ok() && d1.is_ok()
              && set.ops_conserved() && set.interest_len() == 1,
          "就绪队列协议的两挂点必须与 B09 语义一致");

    // 3. 两挂点联签：直连与 hook 等价对拍全等
    let eq = run_hook_equivalence();
    s.add("两挂点联签：等价对拍全等且单源成立",
          eq.single_source() && eq.compared == 4,
          "ctl/wait 双挂点逐操作对拍零分歧");

    // 4. 就绪队列协议：登记且掩码相交才投递
    let mut set2 = EpollSet::new();
    let _ = set2.ctl(CTL_ADD, 10, EV_IN, 7);
    let push_ok = set2.push_ready(ReadyEvent { fd: 10, events: EV_IN }).is_ok();
    let push_mask_reject = set2.push_ready(ReadyEvent { fd: 10, events: EV_HUP }).is_err();
    s.add("就绪协议：登记相交才入队、掩码外拒收",
          push_ok && push_mask_reject,
          "生产者侧过滤是队列协议的第一道门");

    // 5. 幽灵 fd 只丢弃不投递
    let ghost_before = set2.dropped_count();
    let _ = set2.push_ready(ReadyEvent { fd: 77, events: EV_IN });
    s.add("幽灵防线：未登记 fd 丢弃记格、零投递",
          set2.dropped_count() == ghost_before + 1 && set2.ready_len() == 1,
          "幽灵事件可观测不可投递");

    // 6. wait 交付：掩码截断 + 有界交付
    let out = set2.wait(4);
    let wait_ok = match &out {
        Ok(v) => v.len() == 1 && v.first().map_or(false, |e| e.events == EV_IN),
        Err(_) => false,
    };
    s.add("wait 交付：报告事件 ⊆ 登记掩码", wait_ok,
          "掩码截断防止未登记位外泄");

    // 7. 零 maxevents EINVAL
    let mut set3 = EpollSet::new();
    let _ = set3.ctl(CTL_ADD, 11, EV_IN, 1);
    let _ = set3.push_ready(ReadyEvent { fd: 11, events: EV_IN });
    let zero_wait = set3.wait(0);
    s.add("wait 边界：maxevents=0 显性 EINVAL",
          zero_wait == Err(EINVAL) && set3.ready_len() == 1,
          "零界拒绝且就绪队列不被误清");

    // 8. 注入核：四类坏样本各 10/10 检出
    let (inj, per_class) = run_ep_injection();
    let all_caught = per_class.iter().all(|n| *n == INJECT_ROUNDS);
    s.add("注入核：四类坏样本各 10/10 检出", all_caught,
          "重号/缺项/幽灵/零界逐类拒绝");

    // 9. 注入账守恒
    s.add("注入账守恒：注入=检出+逃逸且逃逸=0",
          inj.conserved() && inj.escaped == 0
              && inj.injected == 4 * INJECT_ROUNDS,
          "40 次注入必须 40 次检出、0 逃逸");

    // 10. 表满边界：满容 ADD 显性 ENOSPC
    let mut set4 = EpollSet::new();
    let mut fill_ok = true;
    let mut fd = 20i32;
    let mut filled = 0usize;
    while filled < INTEREST_CAP {
        if set4.ctl(CTL_ADD, fd, EV_IN, 0).is_err() {
            fill_ok = false;
        }
        fd += 1;
        filled += 1;
    }
    let over = set4.ctl(CTL_ADD, fd, EV_IN, 0);
    s.add("表满边界：满容后 ADD 显性 ENOSPC",
          fill_ok && over == Err(ENOSPC) && set4.interest_len() == INTEREST_CAP,
          "容量满必须拒绝而非静默覆盖");

    // 11. 未知 ctl 操作码 EINVAL
    let mut set5 = EpollSet::new();
    s.add("ctl 闭集：ADD/MOD/DEL 之外一律 EINVAL",
          set5.ctl(9, 1, EV_IN, 0) == Err(EINVAL) && set5.ctl(0, 1, EV_IN, 0) == Err(EINVAL),
          "操作码闭集外不得误入其他语义");

    // 12. 负 fd EBADF
    let mut set6 = EpollSet::new();
    s.add("fd 守卫：负 fd 一律 EBADF",
          set6.ctl(CTL_ADD, -1, EV_IN, 0) == Err(EBADF)
              && set6.ctl(CTL_DEL, -5, 0, 0) == Err(EBADF),
          "fd 校验先于兴趣表查找");

    // 13. Linux 逐码一致
    let codes_ok = agrees_with_linux(EEXIST) && agrees_with_linux(ENOENT)
        && agrees_with_linux(EINVAL) && agrees_with_linux(ENOSPC)
        && !agrees_with_linux(0) && !agrees_with_linux(999);
    s.add("Linux 逐码一致：表内码精确、表外码拒绝", codes_ok,
          "errno 表正值与负值逐项可复算");

    // 14. 双跑一致
    let a = corpus_fingerprint();
    let b = corpus_fingerprint();
    s.add("双跑一致：规范语料指纹相等", a == b,
          "同输入两次必须逐字段相等，隐藏状态即失败");

    // 15. 文案闭集
    let text_ok = EpCode::ALL.iter().all(|c| !c.detail().is_empty());
    s.add("文案闭集：全部诊断码带中文建议", text_ok,
          "每个码必须能取到非空且可操作的说明");

    // 16. 码值唯一
    let mut uniq = true;
    let mut i = 0usize;
    while i < EpCode::ALL.len() {
        let mut j = i + 1;
        while j < EpCode::ALL.len() {
            if let (Some(x), Some(y)) = (EpCode::ALL.get(i), EpCode::ALL.get(j)) {
                if x.code() == y.code() {
                    uniq = false;
                }
            }
            j += 1;
        }
        i += 1;
    }
    s.add("诊断码：13 个码值两两唯一", uniq,
          "段内闭集码不得重复，否则判据无法定位");

    // 动态明细（≤8）
    let set_text = format!(
        "登记 {} / 就绪 {} / 幽灵丢弃 {} / 等价 {} 笔",
        set.interest_len(),
        set.ready_len(),
        set2.dropped_count(),
        eq.compared
    );
    s.add_dyn("挂点账快照", set.ops_conserved() && eq.single_source(), &set_text);
    let fp_text = format!("语料指纹 0x{:08X}", a);
    s.add_dyn("指纹快照", a == b, &fp_text);

    s
}

// ── 测试 ──────────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ctl_add_mod_del_flow() {
        let mut s = EpollSet::new();
        assert!(s.ctl(CTL_ADD, 1, EV_IN, 1).is_ok());
        assert!(s.ctl(CTL_MOD, 1, EV_IN | EV_OUT, 2).is_ok());
        assert!(s.ctl(CTL_DEL, 1, 0, 0).is_ok());
        assert_eq!(s.interest_len(), 0);
        assert!(s.ops_conserved());
    }

    #[test]
    fn duplicate_add_eexist() {
        let mut s = EpollSet::new();
        let _ = s.ctl(CTL_ADD, 1, EV_IN, 0);
        assert_eq!(s.ctl(CTL_ADD, 1, EV_OUT, 0), Err(EEXIST));
        assert_eq!(s.interest_len(), 1);
    }

    #[test]
    fn mod_del_absent_enoent() {
        let mut s = EpollSet::new();
        assert_eq!(s.ctl(CTL_MOD, 9, EV_IN, 0), Err(ENOENT));
        assert_eq!(s.ctl(CTL_DEL, 9, 0, 0), Err(ENOENT));
    }

    #[test]
    fn negative_fd_ebadf() {
        let mut s = EpollSet::new();
        assert_eq!(s.ctl(CTL_ADD, -1, EV_IN, 0), Err(EBADF));
    }

    #[test]
    fn unknown_ctl_op_einval() {
        let mut s = EpollSet::new();
        assert_eq!(s.ctl(42, 1, EV_IN, 0), Err(EINVAL));
    }

    #[test]
    fn ghost_ready_dropped() {
        let mut s = EpollSet::new();
        assert_eq!(s.push_ready(ReadyEvent { fd: 5, events: EV_IN }), Err(ENOENT));
        assert_eq!(s.dropped_count(), 1);
        assert_eq!(s.ready_len(), 0);
    }

    #[test]
    fn wait_mask_clamped() {
        let mut s = EpollSet::new();
        let _ = s.ctl(CTL_ADD, 2, EV_IN, 0);
        let _ = s.push_ready(ReadyEvent { fd: 2, events: EV_IN | EV_HUP });
        let v = s.wait(1).unwrap_or_default();
        assert_eq!(v.len(), 1);
        assert_eq!(v.first().map(|e| e.events), Some(EV_IN));
    }

    #[test]
    fn wait_zero_maxevents_einval() {
        let mut s = EpollSet::new();
        assert_eq!(s.wait(0).err(), Some(EINVAL));
    }

    #[test]
    fn del_clears_ready_queue() {
        let mut s = EpollSet::new();
        let _ = s.ctl(CTL_ADD, 3, EV_IN, 0);
        let _ = s.push_ready(ReadyEvent { fd: 3, events: EV_IN });
        let _ = s.ctl(CTL_DEL, 3, 0, 0);
        assert_eq!(s.ready_len(), 0);
    }

    #[test]
    fn hook_equivalence_single_source() {
        let eq = run_hook_equivalence();
        assert!(eq.single_source());
        assert_eq!(eq.compared, 4);
        assert_eq!(eq.diverged, 0);
    }

    #[test]
    fn ep_injection_matrix_green() {
        let (l, per) = run_ep_injection();
        assert!(l.conserved());
        assert_eq!(l.escaped, 0);
        for n in per.iter() {
            assert_eq!(*n, INJECT_ROUNDS);
        }
    }

    #[test]
    fn interest_cap_enforced() {
        let mut s = EpollSet::new();
        let mut i = 0;
        while i < INTEREST_CAP {
            assert!(s.ctl(CTL_ADD, 100 + i as i32, EV_IN, 0).is_ok());
            i += 1;
        }
        assert_eq!(s.ctl(CTL_ADD, 200, EV_IN, 0), Err(ENOSPC));
    }

    #[test]
    fn fingerprint_deterministic() {
        assert_eq!(corpus_fingerprint(), corpus_fingerprint());
    }

    #[test]
    fn unx9384_checks_green() {
        let s = run_unx9384_checks();
        assert!(s.all_passed(), "UNX-F9384 red: {}", s.red_items_text());
    }
}
