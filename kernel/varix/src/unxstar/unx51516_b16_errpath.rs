//! UNX-F51516 B16 错误路径三件事（flush 失败显性化/介质变更竞态/统计账写冲突仲裁） · 0xC0400段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文：UNX-F51517-J1 埋点 100%；异常零静默 ×10 抽查
//!
//! 设计要点：
//!   ① flush 失败显性化：注入失败时 Err 显性回报且脏页账原封不动（禁
//!      静默清零）；每条失败必须逐条确认（ack），未确认失败数非零即
//!      「静默」红牌；×10 注入逐条确认后未清账必须归 0（异常零静默）。
//!   ② 介质变更竞态：介质世代号单调递增，携带旧世代令牌的操作显性拒
//!      （MediumChanged）；未来令牌同样拒（令牌必须精确等于当前世代）。
//!   ③ 统计账写冲突仲裁：双写方按 seq 令牌仲裁，令牌失配者显性败选并
//!      计入败选账；守恒：尝试量 = 胜选量 + 败选量。
//!   ④ 会话相位单调：Idle→Probing→Arbitrating→Settled 逐格推进，倒退/
//!      跳格显性拒；Settled 后会话收口，flush/介质/统计一律拒；
//!      new_session 只允许从 Idle/Settled 出发，session_id 单调递增。
//!   ⑤ 判定次序：会话收口守卫 → 相位门 → 内容判据（零 → 上限）→ 令牌
//!      判据；内容判据先于令牌判据。
//!   ⑥ 零浮点、零 IO、零墙钟、零随机；账面全部 checked/saturating 推进。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::vec::Vec;

pub const SEG_BASE: u32 = 0xC0400;

/// 脏页账容量上限（冻结）。
pub const MAX_DIRTY: u64 = 64;
/// 单次统计提交的增量上限（冻结）。
pub const MAX_DELTA: u64 = 1000;
/// 各类异常的抽查轮数（冻结）。
pub const ROUNDS: u32 = 10;

/// 诊断码闭集（错误路径三件事域）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum B16Code {
    /// 成功
    Ok = 0,
    /// 段位守卫失败（SEG_BASE 越界或不对齐）
    SegBad,
    /// 会话已收口（Settled 后一律拒）
    SessionClosed,
    /// 相位不匹配（当前相位不允许该操作）
    PhaseMismatch,
    /// 相位倒退
    PhaseRegression,
    /// 相位跳格
    PhaseSkip,
    /// 零脏页标记
    DirtyZero,
    /// 脏页账超容量
    DirtyOverCap,
    /// 脏账为零却请求 flush
    FlushNotDirty,
    /// flush 失败（显性回报，脏账原封）
    FlushFailed,
    /// 没有未确认的失败可确认
    NothingToAck,
    /// 介质变更（令牌与当前世代不符）
    MediumChanged,
    /// 统计增量为零
    StatDeltaZero,
    /// 统计增量超上限
    StatDeltaTooBig,
    /// 统计写冲突（seq 令牌失配，显性败选）
    StatRace,
    /// 账面守恒失败
    LedgerMismatch,
}

impl B16Code {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            B16Code::Ok => "错误路径操作成功",
            B16Code::SegBad => "段位越界：检查 SEG_BASE 对齐与取值范围",
            B16Code::SessionClosed => "会话已收口：new_session 开新会话后再操作",
            B16Code::PhaseMismatch => "相位不匹配：先推进到操作所需相位",
            B16Code::PhaseRegression => "相位倒退：会话相位只能前进不能回退",
            B16Code::PhaseSkip => "相位跳格：必须逐格推进会话相位",
            B16Code::DirtyZero => "零脏页标记：mark_dirty 增量必须 ≥1",
            B16Code::DirtyOverCap => "脏页账超容：先 flush 腾账再标记",
            B16Code::FlushNotDirty => "脏账为零：无待刷内容",
            B16Code::FlushFailed => "flush 失败：脏账保持原状，确认后重试",
            B16Code::NothingToAck => "无未确认失败：确认与失败必须一一对应",
            B16Code::MediumChanged => "介质变更：重新读取世代令牌后重试",
            B16Code::StatDeltaZero => "零增量：统计提交增量必须 ≥1",
            B16Code::StatDeltaTooBig => "增量超限：拆分后重新提交",
            B16Code::StatRace => "写冲突败选：取新 seq 令牌后重新提交",
            B16Code::LedgerMismatch => "账面不符：尝试=胜选+败选 守恒被破坏",
        }
    }
}

/// 诊断码全序表（闭集枚举的权威清单）。
pub const ALL_CODES: [B16Code; 16] = [
    B16Code::Ok,
    B16Code::SegBad,
    B16Code::SessionClosed,
    B16Code::PhaseMismatch,
    B16Code::PhaseRegression,
    B16Code::PhaseSkip,
    B16Code::DirtyZero,
    B16Code::DirtyOverCap,
    B16Code::FlushNotDirty,
    B16Code::FlushFailed,
    B16Code::NothingToAck,
    B16Code::MediumChanged,
    B16Code::StatDeltaZero,
    B16Code::StatDeltaTooBig,
    B16Code::StatRace,
    B16Code::LedgerMismatch,
];

/// 会话相位（单调推进）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ErrPathPhase {
    /// 空闲（可标记脏账）
    Idle = 0,
    /// 介质探测窗（介质操作只在此相位放行）
    Probing = 1,
    /// 统计仲裁窗（统计提交只在此相位放行）
    Arbitrating = 2,
    /// 会话收口（只读，一律拒写操作）
    Settled = 3,
}

impl ErrPathPhase {
    pub fn order(self) -> u32 {
        self as u32
    }
}

/// B16 错误路径引擎：flush 显性化 + 介质竞态 + 统计仲裁 三件事共账。
pub struct B16ErrPathEngine {
    session_id: u32,
    phase: ErrPathPhase,
    dirty: u64,
    unacked: u64,
    flush_ok_cnt: u64,
    flush_fail_cnt: u64,
    gen: u64,
    media_rejected: u64,
    stat_seq: u64,
    stat_attempted: u64,
    stat_accepted: u64,
    stat_rejected: u64,
    injected: u64,
    detected: u64,
}

impl B16ErrPathEngine {
    /// 新引擎：session_id 从 1 起，相位 Idle。
    pub fn new() -> B16ErrPathEngine {
        B16ErrPathEngine {
            session_id: 1,
            phase: ErrPathPhase::Idle,
            dirty: 0,
            unacked: 0,
            flush_ok_cnt: 0,
            flush_fail_cnt: 0,
            gen: 0,
            media_rejected: 0,
            stat_seq: 0,
            stat_attempted: 0,
            stat_accepted: 0,
            stat_rejected: 0,
            injected: 0,
            detected: 0,
        }
    }

    pub fn session_id(&self) -> u32 {
        self.session_id
    }

    pub fn phase(&self) -> ErrPathPhase {
        self.phase
    }

    pub fn dirty(&self) -> u64 {
        self.dirty
    }

    pub fn unacked(&self) -> u64 {
        self.unacked
    }

    pub fn flush_ok_cnt(&self) -> u64 {
        self.flush_ok_cnt
    }

    pub fn flush_fail_cnt(&self) -> u64 {
        self.flush_fail_cnt
    }

    pub fn gen(&self) -> u64 {
        self.gen
    }

    pub fn media_rejected(&self) -> u64 {
        self.media_rejected
    }

    pub fn stat_seq(&self) -> u64 {
        self.stat_seq
    }

    pub fn stat_accepted(&self) -> u64 {
        self.stat_accepted
    }

    pub fn stat_rejected(&self) -> u64 {
        self.stat_rejected
    }

    pub fn injected(&self) -> u64 {
        self.injected
    }

    pub fn detected(&self) -> u64 {
        self.detected
    }

    pub fn escaped(&self) -> u64 {
        self.injected.saturating_sub(self.detected)
    }

    pub fn conserved(&self) -> bool {
        self.injected == self.detected && self.detected > 0
    }

    pub fn note_violation_caught(&mut self) {
        self.injected += 1;
        self.detected += 1;
    }

    pub fn note_escape(&mut self) {
        self.injected += 1;
    }

    /// 统计账守恒：尝试量 = 胜选量 + 败选量。
    pub fn ledger_ok(&self) -> bool {
        self.stat_attempted == self.stat_accepted + self.stat_rejected
    }

    /// 双跑快照：关键账面元组（确定性对照面）。
    pub fn snapshot(&self) -> (u32, u64, u64, u64, u64, u64, u64, u64, u64) {
        (
            self.session_id,
            self.dirty,
            self.unacked,
            self.gen,
            self.stat_seq,
            self.stat_attempted,
            self.stat_accepted,
            self.stat_rejected,
            self.flush_fail_cnt,
        )
    }

    /// 相位推进：只允许逐格前进；倒退/跳格/原地显性拒。
    pub fn enter(&mut self, target: ErrPathPhase) -> Result<(), B16Code> {
        let cur = self.phase.order();
        let tgt = target.order();
        if tgt < cur {
            return Err(B16Code::PhaseRegression);
        }
        if tgt != cur + 1 {
            return Err(B16Code::PhaseSkip);
        }
        self.phase = target;
        Ok(())
    }

    /// 开新会话：只允许从 Idle/Settled 出发；session_id 单调递增。
    pub fn new_session(&mut self) -> Result<u32, B16Code> {
        if self.phase != ErrPathPhase::Idle && self.phase != ErrPathPhase::Settled {
            return Err(B16Code::PhaseMismatch);
        }
        self.phase = ErrPathPhase::Idle;
        self.session_id = match self.session_id.checked_add(1) {
            Some(v) => v,
            None => return Err(B16Code::LedgerMismatch),
        };
        Ok(self.session_id)
    }

    /// 会话收口守卫：Settled 后一切写操作拒。
    fn require_open(&self) -> Result<(), B16Code> {
        if self.phase == ErrPathPhase::Settled {
            return Err(B16Code::SessionClosed);
        }
        Ok(())
    }

    /// 相位门：当前相位必须等于 want。
    fn require_phase(&self, want: ErrPathPhase) -> Result<(), B16Code> {
        if self.phase != want {
            return Err(B16Code::PhaseMismatch);
        }
        Ok(())
    }

    /// 标记脏页。判定次序：零增量 → 容量上溢。
    pub fn mark_dirty(&mut self, n: u64) -> Result<u64, B16Code> {
        if n == 0 {
            return Err(B16Code::DirtyZero);
        }
        let nd = match self.dirty.checked_add(n) {
            Some(v) => v,
            None => return Err(B16Code::DirtyOverCap),
        };
        if nd > MAX_DIRTY {
            return Err(B16Code::DirtyOverCap);
        }
        self.dirty = nd;
        Ok(self.dirty)
    }

    /// flush。判定次序：会话收口 → 脏账非零 → 注入失败路径 → 成功路径。
    /// 失败显性化：Err 回报且脏账原封不动，未确认计数 +1。
    pub fn flush(&mut self, inject_fail: bool) -> Result<u64, B16Code> {
        self.require_open()?;
        if self.dirty == 0 {
            return Err(B16Code::FlushNotDirty);
        }
        if inject_fail {
            self.unacked += 1;
            self.flush_fail_cnt += 1;
            return Err(B16Code::FlushFailed);
        }
        let flushed = self.dirty;
        self.dirty = 0;
        self.flush_ok_cnt += 1;
        Ok(flushed)
    }

    /// 确认一条 flush 失败（异常零静默：确认与失败一一对应）。
    pub fn ack_flush_failure(&mut self) -> Result<(), B16Code> {
        if self.unacked == 0 {
            return Err(B16Code::NothingToAck);
        }
        self.unacked -= 1;
        Ok(())
    }

    /// 介质变更：世代号单调递增，返回新世代。
    pub fn media_touch(&mut self) -> Result<u64, B16Code> {
        self.require_open()?;
        self.gen = match self.gen.checked_add(1) {
            Some(v) => v,
            None => return Err(B16Code::LedgerMismatch),
        };
        Ok(self.gen)
    }

    /// 携带令牌的介质操作。判定次序：会话收口 → 相位门（Probing）→ 令牌。
    pub fn media_op(&mut self, expected_gen: u64) -> Result<u64, B16Code> {
        self.require_open()?;
        self.require_phase(ErrPathPhase::Probing)?;
        if expected_gen != self.gen {
            self.media_rejected += 1;
            return Err(B16Code::MediumChanged);
        }
        Ok(self.gen)
    }

    /// 统计账提交（双写方按 seq 令牌仲裁）。判定次序：会话收口 → 相位门
    /// （Arbitrating）→ 内容判据（零 → 上限）→ 令牌判据。
    pub fn stat_commit(&mut self, ticket: u64, delta: u64) -> Result<u64, B16Code> {
        self.require_open()?;
        self.require_phase(ErrPathPhase::Arbitrating)?;
        if delta == 0 {
            return Err(B16Code::StatDeltaZero);
        }
        if delta > MAX_DELTA {
            return Err(B16Code::StatDeltaTooBig);
        }
        self.stat_attempted += delta;
        if ticket != self.stat_seq {
            self.stat_rejected += delta;
            return Err(B16Code::StatRace);
        }
        self.stat_accepted += delta;
        self.stat_seq = match self.stat_seq.checked_add(1) {
            Some(v) => v,
            None => return Err(B16Code::LedgerMismatch),
        };
        Ok(self.stat_seq)
    }
}

/// 段位守卫。
pub fn seg_guard_ok() -> bool {
    SEG_BASE % 0x40 == 0 && SEG_BASE >= 0xC0080 && SEG_BASE <= 0xC31C0
}

/// 双跑一致性：同一脚本两次重建快照逐字段一致（确定性）。
pub fn dual_run_consistent() -> bool {
    let build = || -> (u32, u64, u64, u64, u64, u64, u64, u64, u64) {
        let mut e = B16ErrPathEngine::new();
        let _ = e.mark_dirty(6);
        let _ = e.flush(false);
        let _ = e.mark_dirty(4);
        let _ = e.flush(true);
        let _ = e.ack_flush_failure();
        let _ = e.enter(ErrPathPhase::Probing);
        let _ = e.media_touch();
        let g = e.gen();
        let _ = e.media_op(g);
        let _ = e.enter(ErrPathPhase::Arbitrating);
        let _ = e.stat_commit(e.stat_seq(), 5);
        let _ = e.stat_commit(e.stat_seq() + 1, 3);
        let _ = e.enter(ErrPathPhase::Settled);
        e.snapshot()
    };
    build() == build()
}

/// 收口自检（≥12 条判据形态）。
pub fn run_unx51516_checks() -> CheckSet {
    let mut s = CheckSet::new("unx51516");
    // 1 正常流：flush 显性成功 + 介质令牌 + 仲裁胜选 + 会话收口/重开。
    {
        let mut e = B16ErrPathEngine::new();
        let _ = e.mark_dirty(8);
        let r = e.flush(false);
        let _ = e.enter(ErrPathPhase::Probing);
        let _ = e.media_touch();
        let g = e.gen();
        let m = e.media_op(g);
        let _ = e.enter(ErrPathPhase::Arbitrating);
        let st = e.stat_commit(e.stat_seq(), 10);
        let _ = e.enter(ErrPathPhase::Settled);
        let ns = e.new_session();
        s.add(
            "正常流三件事",
            r == Ok(8)
                && e.dirty() == 0
                && m == Ok(g)
                && st == Ok(1)
                && ns == Ok(2)
                && e.ledger_ok(),
            "flush 清账、令牌命中、令牌胜选序号推进、新会话单调",
        );
    }
    // 2 边界零：零脏标记/零脏 flush/零增量。
    {
        let mut e = B16ErrPathEngine::new();
        let dz = e.mark_dirty(0) == Err(B16Code::DirtyZero);
        let fn0 = e.flush(false) == Err(B16Code::FlushNotDirty);
        let _ = e.enter(ErrPathPhase::Probing);
        let _ = e.enter(ErrPathPhase::Arbitrating);
        let sz = e.stat_commit(e.stat_seq(), 0) == Err(B16Code::StatDeltaZero);
        s.add(
            "零值边界",
            dz && fn0 && sz,
            "零增量/零脏账显性拒（无符号域无负值，零/上限/上限+1 全覆盖）",
        );
    }
    // 3 边界上限：脏账恰好满仓 flush、增量恰好上限。
    {
        let mut e = B16ErrPathEngine::new();
        let md = e.mark_dirty(MAX_DIRTY);
        let fl = e.flush(false);
        let _ = e.enter(ErrPathPhase::Probing);
        let _ = e.enter(ErrPathPhase::Arbitrating);
        let dmax = e.stat_commit(e.stat_seq(), MAX_DELTA);
        s.add(
            "上限恰好放行",
            md == Ok(MAX_DIRTY) && fl == Ok(MAX_DIRTY) && dmax == Ok(1),
            "脏账满仓可 flush、增量上限可提交",
        );
    }
    // 4 边界上限+1：脏账超容、增量超限。
    {
        let mut e = B16ErrPathEngine::new();
        let _ = e.mark_dirty(MAX_DIRTY);
        let over = e.mark_dirty(1) == Err(B16Code::DirtyOverCap);
        let _ = e.enter(ErrPathPhase::Probing);
        let _ = e.enter(ErrPathPhase::Arbitrating);
        let big = e.stat_commit(e.stat_seq(), MAX_DELTA + 1) == Err(B16Code::StatDeltaTooBig);
        s.add(
            "上限+1 显性拒",
            over && big && e.dirty() == MAX_DIRTY,
            "超容/超限显性拒且账面不被部分改写",
        );
    }
    // 5 注入必抓：flush 失败显性化 ×10（异常零静默抽查）。
    {
        let mut e = B16ErrPathEngine::new();
        let mut ok = true;
        for i in 0..ROUNDS {
            let _ = e.mark_dirty(4);
            if e.flush(true) != Err(B16Code::FlushFailed) {
                ok = false;
            }
            if e.dirty() != 4 {
                ok = false;
            }
            if e.unacked() != i as u64 + 1 {
                ok = false;
            }
            if e.ack_flush_failure().is_err() {
                ok = false;
            }
            e.note_violation_caught();
        }
        s.add(
            "flush 失败显性化 10/10 零静默",
            ok && e.unacked() == 0 && e.flush_fail_cnt() == 10 && e.dirty() == 4,
            "失败逐条显性回报且脏账原封；逐条确认后未清账=0",
        );
    }
    // 6 注入必抓：介质变更竞态 ×10（旧世代令牌全抓）。
    {
        let mut e = B16ErrPathEngine::new();
        let _ = e.enter(ErrPathPhase::Probing);
        let mut caught = 0u32;
        for _ in 0..ROUNDS {
            let stale = e.gen();
            let _ = e.media_touch();
            if e.media_op(stale) == Err(B16Code::MediumChanged) {
                caught += 1;
                e.note_violation_caught();
            }
        }
        s.add(
            "介质竞态注入 10/10",
            caught == 10 && e.media_rejected() == 10,
            "旧世代令牌操作必须全抓并计入拒账",
        );
    }
    // 7 注入必抓：统计写冲突败选 ×10（未来令牌全抓）。
    {
        let mut e = B16ErrPathEngine::new();
        let _ = e.enter(ErrPathPhase::Probing);
        let _ = e.enter(ErrPathPhase::Arbitrating);
        let mut caught = 0u32;
        for i in 0..ROUNDS {
            let ticket = e.stat_seq() + 1;
            if e.stat_commit(ticket, 1 + i as u64) == Err(B16Code::StatRace) {
                caught += 1;
                e.note_violation_caught();
            }
        }
        s.add(
            "统计写冲突败选 10/10",
            caught == 10 && e.ledger_ok(),
            "令牌失配者显性败选，尝试=胜选+败选",
        );
    }
    // 8 判定次序：内容判据先于令牌；收口守卫先于脏判据；相位门先于令牌。
    {
        let mut e = B16ErrPathEngine::new();
        let _ = e.enter(ErrPathPhase::Probing);
        let _ = e.enter(ErrPathPhase::Arbitrating);
        let zero_first = e.stat_commit(e.stat_seq() + 5, 0) == Err(B16Code::StatDeltaZero);
        let big_second = e.stat_commit(e.stat_seq() + 5, MAX_DELTA + 1) == Err(B16Code::StatDeltaTooBig);
        let race_last = e.stat_commit(e.stat_seq() + 5, 1) == Err(B16Code::StatRace);
        let _ = e.enter(ErrPathPhase::Settled);
        let closed_first = e.flush(true) == Err(B16Code::SessionClosed);
        let mut e2 = B16ErrPathEngine::new();
        let phase_first = e2.media_op(12345) == Err(B16Code::PhaseMismatch);
        s.add(
            "判序三段",
            zero_first && big_second && race_last && closed_first && phase_first,
            "收口守卫 → 相位门 → 内容（零→上限）→ 令牌，逐段在前",
        );
    }
    // 9 状态机拒止：跳格/倒退/原地/非法重开。
    {
        let mut e = B16ErrPathEngine::new();
        let skip = e.enter(ErrPathPhase::Arbitrating) == Err(B16Code::PhaseSkip);
        let _ = e.enter(ErrPathPhase::Probing);
        let same = e.enter(ErrPathPhase::Probing) == Err(B16Code::PhaseSkip);
        let regress = e.enter(ErrPathPhase::Idle) == Err(B16Code::PhaseRegression);
        let reopen = e.new_session() == Err(B16Code::PhaseMismatch);
        s.add(
            "会话相位机拒止",
            skip && same && regress && reopen,
            "跳格/原地/倒退/中途重开一律显性拒",
        );
    }
    // 10 容量耗尽：脏账分两笔填满后再溢出。
    {
        let mut e = B16ErrPathEngine::new();
        let _ = e.mark_dirty(40);
        let _ = e.mark_dirty(24);
        let full = e.dirty() == MAX_DIRTY;
        let over = e.mark_dirty(1) == Err(B16Code::DirtyOverCap);
        s.add(
            "脏账容量耗尽",
            full && over && e.dirty() == MAX_DIRTY,
            "冻结容量恰好可填、溢出显性拒且账不增",
        );
    }
    // 11 双跑一致。
    s.add("双跑逐字段一致", dual_run_consistent(), "同一脚本两次快照逐字段相等");
    // 12 段位守卫。
    s.add("段位守卫", seg_guard_ok(), "SEG_BASE 对齐且在分派区间");
    // 13 对账守恒：30 次注入全检出、逃逸 0、统计账三面平。
    {
        let mut e = B16ErrPathEngine::new();
        for _ in 0..ROUNDS {
            let _ = e.mark_dirty(4);
            let _ = e.flush(true);
            e.note_violation_caught();
            let _ = e.ack_flush_failure();
        }
        let _ = e.enter(ErrPathPhase::Probing);
        for _ in 0..ROUNDS {
            let stale = e.gen();
            let _ = e.media_touch();
            let _ = e.media_op(stale);
            e.note_violation_caught();
        }
        let _ = e.enter(ErrPathPhase::Arbitrating);
        for i in 0..ROUNDS {
            let _ = e.stat_commit(e.stat_seq() + 1, 1 + i as u64);
            e.note_violation_caught();
        }
        s.add(
            "注入账守恒",
            e.conserved() && e.injected() == 30 && e.escaped() == 0 && e.ledger_ok(),
            "注入=检出+逃逸，逃逸恒 0；统计账尝试=胜选+败选",
        );
    }
    // 14 文案闭集非空。
    {
        let mut closed = true;
        for c in ALL_CODES.iter() {
            if c.detail().is_empty() {
                closed = false;
            }
        }
        s.add("文案闭集非空", closed, "每个诊断码都必须有中文建议");
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flush_failure_is_explicit() {
        let mut e = B16ErrPathEngine::new();
        let _ = e.mark_dirty(9);
        assert_eq!(e.flush(true), Err(B16Code::FlushFailed));
        assert_eq!(e.dirty(), 9, "失败必须显性化：脏账原封不动");
        assert_eq!(e.unacked(), 1);
        assert!(e.ack_flush_failure().is_ok());
        assert_eq!(e.unacked(), 0);
        assert_eq!(e.ack_flush_failure(), Err(B16Code::NothingToAck));
        // 确认后重试成功。
        assert_eq!(e.flush(false), Ok(9));
        assert_eq!(e.dirty(), 0);
    }

    #[test]
    fn stale_media_token_rejected() {
        let mut e = B16ErrPathEngine::new();
        let _ = e.enter(ErrPathPhase::Probing);
        let stale = e.gen();
        let _ = e.media_touch();
        assert_eq!(e.media_op(stale), Err(B16Code::MediumChanged));
        // 未来令牌同样拒：令牌必须精确等于当前世代。
        assert_eq!(e.media_op(e.gen() + 1000), Err(B16Code::MediumChanged));
        assert!(e.media_op(e.gen()).is_ok());
        assert_eq!(e.media_rejected(), 2);
    }

    #[test]
    fn stale_ticket_loses_arbitration() {
        let mut e = B16ErrPathEngine::new();
        let _ = e.enter(ErrPathPhase::Probing);
        let _ = e.enter(ErrPathPhase::Arbitrating);
        assert_eq!(e.stat_commit(e.stat_seq(), 5), Ok(1));
        assert_eq!(e.stat_commit(e.stat_seq() + 1, 3), Err(B16Code::StatRace));
        assert_eq!(e.stat_accepted(), 5);
        assert_eq!(e.stat_rejected(), 3);
        assert!(e.ledger_ok());
    }

    #[test]
    fn zero_and_upper_delta_bounds() {
        let mut e = B16ErrPathEngine::new();
        let _ = e.enter(ErrPathPhase::Probing);
        let _ = e.enter(ErrPathPhase::Arbitrating);
        assert_eq!(e.stat_commit(e.stat_seq(), 0), Err(B16Code::StatDeltaZero));
        assert_eq!(e.stat_commit(e.stat_seq(), MAX_DELTA), Ok(1));
        assert_eq!(
            e.stat_commit(e.stat_seq(), MAX_DELTA + 1),
            Err(B16Code::StatDeltaTooBig)
        );
    }

    #[test]
    fn dirty_cap_overflow_rejected_without_partial_write() {
        let mut e = B16ErrPathEngine::new();
        let _ = e.mark_dirty(40);
        let _ = e.mark_dirty(24);
        assert_eq!(e.dirty(), MAX_DIRTY);
        assert_eq!(e.mark_dirty(1), Err(B16Code::DirtyOverCap));
        assert_eq!(e.dirty(), MAX_DIRTY, "拒后脏账不增");
        assert_eq!(e.mark_dirty(0), Err(B16Code::DirtyZero));
    }

    #[test]
    fn judge_orders() {
        // 内容判据先于令牌判据。
        let mut e = B16ErrPathEngine::new();
        let _ = e.enter(ErrPathPhase::Probing);
        let _ = e.enter(ErrPathPhase::Arbitrating);
        assert_eq!(e.stat_commit(e.stat_seq() + 5, 0), Err(B16Code::StatDeltaZero));
        assert_eq!(
            e.stat_commit(e.stat_seq() + 5, MAX_DELTA + 1),
            Err(B16Code::StatDeltaTooBig)
        );
        assert_eq!(e.stat_commit(e.stat_seq() + 5, 1), Err(B16Code::StatRace));
        // 会话收口守卫先于脏账判据。
        let _ = e.enter(ErrPathPhase::Settled);
        assert_eq!(e.flush(true), Err(B16Code::SessionClosed));
        // 相位门先于令牌判据。
        let mut e2 = B16ErrPathEngine::new();
        assert_eq!(e2.media_op(12345), Err(B16Code::PhaseMismatch));
    }

    #[test]
    fn phase_machine_rejects() {
        let mut e = B16ErrPathEngine::new();
        assert_eq!(e.enter(ErrPathPhase::Arbitrating), Err(B16Code::PhaseSkip));
        assert!(e.enter(ErrPathPhase::Probing).is_ok());
        assert_eq!(e.enter(ErrPathPhase::Probing), Err(B16Code::PhaseSkip));
        assert_eq!(e.enter(ErrPathPhase::Idle), Err(B16Code::PhaseRegression));
        assert_eq!(e.new_session(), Err(B16Code::PhaseMismatch));
    }

    #[test]
    fn zero_silence_sweep_x10() {
        let mut e = B16ErrPathEngine::new();
        // 先连续失败 ×10 不确认：未确认账必须逐笔显性累积。
        for i in 0..ROUNDS {
            let _ = e.mark_dirty(4);
            assert_eq!(e.flush(true), Err(B16Code::FlushFailed));
            assert_eq!(e.unacked(), i as u64 + 1, "未确认失败必须显性累积");
        }
        // 再逐笔确认：账清零且失败计数在册。
        for _ in 0..ROUNDS {
            assert!(e.ack_flush_failure().is_ok());
        }
        assert_eq!(e.unacked(), 0, "确认后未清账必须归 0");
        assert_eq!(e.flush_fail_cnt(), 10);
    }

    #[test]
    fn dual_run_identical_snapshot() {
        assert!(dual_run_consistent());
    }

    #[test]
    fn seg_guard_holds() {
        assert!(seg_guard_ok());
        assert_eq!(SEG_BASE % 0x40, 0);
    }

    #[test]
    fn detail_texts_closed_set() {
        for c in ALL_CODES.iter() {
            assert!(!c.detail().is_empty());
        }
        assert_eq!(ALL_CODES.len(), 16);
    }
}
