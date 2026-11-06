//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F18552 · 档案热切换（运行中 bottle 变更策略）· 0x805C0 段
//! ─────────────────────────────────────────────────────────────────────
//! 锚点：UNX-F18552-J1 运行中变更下会话生效策略断言全过，存活进程
//!       不受扰断言
//! 本模块结构清单：
//!   1. 冻结常量层：6 个策略闭集（policy 1..6）、8 会话容量、4 档
//!      优先级、边界闸（事务开放/未知策略/代数倒退三拒）；
//!   2. 诊断码闭集 HsCode（code() = SEG_BASE + n，0x805C0 段窗口）；
//!   3. 数据结构一（状态机）：SessionState——Idle→Bound→SwapPending
//!      →Swapped 单向梯，倒退/越级显式拒；Session 载 strategy 代数；
//!   4. 数据结构二（热切账）：SwapLedger——applied/rejected×3/存活
//!      不受扰计数，守恒律 applied+rejected == 总申请数；
//!   5. 变更核 begin_swap：事务开放拒、未知策略拒、代数倒退拒；
//!      钉扎会话（存活进程）只挂 SwapPending 不改代数；
//!   6. 提交核 commit_at_boundary：静默边界才落新代数（gen+1 单调）；
//!      钉扎会话解钉后才换——存活进程策略代数全程不动（不受扰）；
//!   7. 注入核 3 类×10 轮：事务中变更 / 未知策略 / 代数倒退，必拒；
//!   8. 黄金全链 golden_run + run_unx18552_checks：14 条判据 + 8 例
//!      #[cfg(test)]。
//! 为什么换代数必须等静默边界：运行中会话持有旧代数的活引用，
//! 热切若即时生效会让存活进程读到半新半旧策略——钉扎机制把
//! 「生效时刻」推到解钉后的第一个边界，代数单调 +1，可复算可对账。
//! ═══════════════════════════════════════════════════════════════════════

use crate::alloc::format;
use crate::alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x805C0，AI-VX11-1105-G1·A16 批 08 独占窗口）。
pub const SEG_BASE: u32 = 0x805C0;

// ── 冻结常量（推导注记） ─────────────────────────────────────────
/// 策略闭集大小（冻结：6 个策略 id，1..=6）。
pub const POLICY_N: u32 = 6;
/// 会话容量（冻结：8 槽，满即拒）。
pub const SESSION_CAP: usize = 8;
/// 优先级档数（冻结：4 档——normal/low/interactive/realtime）。
pub const PRIORITY_LEVELS: u32 = 4;
/// 存活进程钉扎槽（冻结：2 个钉扎位，模拟长命进程持有旧代数）。
pub const PIN_SLOTS: usize = 2;
/// 注入判定轮数（冻结：10/10 语义）。
pub const JUDGE_ROUNDS: u32 = 10;

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum HsCode {
    /// 会话表满
    SessionFull,
    /// 会话 id 重复
    SessionDup,
    /// 事务开放中变更被拒
    TxnOpenReject,
    /// 未知策略被拒
    PolicyUnknown,
    /// 代数倒退被拒（gen 非单调）
    GenRegress,
    /// 状态机非法倒退/越级
    StateIllegal,
    /// 钉扎会话被扰（存活进程代数变了）
    PinnedDisturbed,
    /// 守恒律破：applied+rejected ≠ 总申请
    ConservationBroken,
    /// 双跑不一致
    NotReproducible,
}

impl HsCode {
    /// 诊断码：SEG_BASE + n（契约骨架口径）。
    pub fn code(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 中文诊断文案（闭集，逐码非空）。
    pub fn detail(self) -> &'static str {
        match self {
            HsCode::SessionFull => "会话表满：8 槽上限，拒绝新会话",
            HsCode::SessionDup => "会话 id 重复：热切面键不唯一",
            HsCode::TxnOpenReject => "事务开放中变更被拒：边界闸一",
            HsCode::PolicyUnknown => "未知策略：不在 1..=6 闭集，边界闸二",
            HsCode::GenRegress => "代数倒退：策略代数必须单调，边界闸三",
            HsCode::StateIllegal => "状态机非法倒退/越级：单向梯违逆",
            HsCode::PinnedDisturbed => "钉扎会话被扰：存活进程代数不得变更",
            HsCode::ConservationBroken => "守恒律破：applied+rejected≠总申请",
            HsCode::NotReproducible => "双跑不一致：确定性红线",
        }
    }
    pub const ALL: [HsCode; 9] = [
        HsCode::SessionFull,
        HsCode::SessionDup,
        HsCode::TxnOpenReject,
        HsCode::PolicyUnknown,
        HsCode::GenRegress,
        HsCode::StateIllegal,
        HsCode::PinnedDisturbed,
        HsCode::ConservationBroken,
        HsCode::NotReproducible,
    ];
}

/// 按数值取中文文案（闭集外返回固定提示）。
pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < HsCode::ALL.len() {
        if HsCode::ALL[i].code() == code {
            return HsCode::ALL[i].detail();
        }
        i += 1;
    }
    "未知诊断码：不在 0x805C0 段闭集内"
}

// ── 数据结构一：会话状态机 ──────────────────────────────────────
/// 会话状态单向梯（冻结序）：
/// Idle(0) → Bound(1) → SwapPending(2) → Swapped(3)。
/// 任何逆序转换（如 Swapped→Bound）与越级（Idle→Swapped）显式拒。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Session {
    pub id: u32,
    pub state: u8,
    /// 当前生效策略代数（单调：只 +1，不回落）。
    pub policy_gen: u32,
    /// 当前生效策略 id（1..=6）。
    pub policy_id: u32,
    /// 事务开放标志：开放中禁变更。
    pub txn_open: bool,
    /// 钉扎标志：存活进程持有本会话，代数不得变更。
    pub pinned: bool,
    /// 待挂策略（SwapPending 态暂存）。
    pub pending_policy: u32,
}

impl Session {
    /// 新建 Idle 会话（零代数、零策略、无事务、无钉扎）。
    pub fn new(id: u32) -> Session {
        Session {
            id,
            state: 0,
            policy_gen: 0,
            policy_id: 0,
            txn_open: false,
            pinned: false,
            pending_policy: 0,
        }
    }
    /// 状态梯序（数字即序）。
    pub fn state_rank(state: u8) -> u32 {
        state as u32
    }
}

/// 单步状态推进：只许 rank+1 的相邻前进，其余一律拒。
pub fn state_advance(cur: u8, next: u8) -> Result<u8, u32> {
    if Session::state_rank(next) != Session::state_rank(cur).saturating_add(1) {
        return Err(HsCode::StateIllegal.code());
    }
    Ok(next)
}

// ── 数据结构二：热切账 ──────────────────────────────────────────
/// 热切账：applied + rejected×3 == 总申请（守恒）；不受扰计数独立。
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct SwapLedger {
    pub applied: u32,
    pub rejected_txn: u32,
    pub rejected_unknown: u32,
    pub rejected_regress: u32,
    pub rejected_state: u32,
    pub pinned_untouched: u32,
    pub attempts: u32,
}

impl SwapLedger {
    /// 守恒核：applied + 四路拒绝 == 总申请数。
    pub fn conserve(&self) -> Result<(), u32> {
        let sum = self
            .applied
            .saturating_add(self.rejected_txn)
            .saturating_add(self.rejected_unknown)
            .saturating_add(self.rejected_regress)
            .saturating_add(self.rejected_state);
        if sum != self.attempts {
            return Err(HsCode::ConservationBroken.code());
        }
        Ok(())
    }
}

// ── 热切机（主体） ──────────────────────────────────────────────
/// 热切机：会话表（定容 8）+ 全局策略代数 + 热切账。
/// 全局代数 gen_counter 只随成功 commit 单调 +1。
pub struct HotSwapMachine {
    sessions: Vec<Session>,
    gen_counter: u32,
    pub ledger: SwapLedger,
}

impl HotSwapMachine {
    /// 新建空热切机（代数从 1 起步）。
    pub fn new() -> HotSwapMachine {
        HotSwapMachine { sessions: Vec::new(), gen_counter: 1, ledger: SwapLedger::default() }
    }

    /// 注册会话：满/重复显式拒；返回会话 id。
    pub fn register(&mut self, id: u32) -> Result<(), u32> {
        if self.sessions.len() >= SESSION_CAP {
            return Err(HsCode::SessionFull.code());
        }
        let mut i = 0;
        while i < self.sessions.len() {
            if let Some(s) = self.sessions.get(i) {
                if s.id == id {
                    return Err(HsCode::SessionDup.code());
                }
            }
            i += 1;
        }
        self.sessions.push(Session::new(id));
        Ok(())
    }

    /// 按号取会话副本。
    pub fn get_session(&self, id: u32) -> Option<Session> {
        let mut i = 0;
        while i < self.sessions.len() {
            if let Some(s) = self.sessions.get(i) {
                if s.id == id {
                    return Some(*s);
                }
            }
            i += 1;
        }
        None
    }

    /// 变更申请（运行中变更入口）：三闸逐一过，全过才挂 SwapPending。
    /// 钉扎会话同样只挂 pending——代数此刻不得动。
    pub fn begin_swap(&mut self, id: u32, new_policy: u32) -> Result<(), u32> {
        self.ledger.attempts = self.ledger.attempts.saturating_add(1);
        // 闸 1：会话必须在场。
        let mut slot: Option<usize> = None;
        let mut i = 0;
        while i < self.sessions.len() {
            if let Some(s) = self.sessions.get(i) {
                if s.id == id {
                    slot = Some(i);
                }
            }
            i += 1;
        }
        let idx = match slot {
            Some(x) => x,
            None => return Err(HsCode::SessionDup.code()), // 不在场=非法申请
        };
        // 闸 2：事务开放拒。
        let txn_open = match self.sessions.get(idx) {
            Some(s) => s.txn_open,
            None => return Err(HsCode::SessionDup.code()),
        };
        if txn_open {
            self.ledger.rejected_txn = self.ledger.rejected_txn.saturating_add(1);
            return Err(HsCode::TxnOpenReject.code());
        }
        // 闸 3：未知策略拒。
        if new_policy == 0 || new_policy > POLICY_N {
            self.ledger.rejected_unknown = self.ledger.rejected_unknown.saturating_add(1);
            return Err(HsCode::PolicyUnknown.code());
        }
        // 闸 4：目标策略不得低于当前生效（代数语义上的倒退拒）。
        let cur_policy = match self.sessions.get(idx) {
            Some(s) => s.policy_id,
            None => return Err(HsCode::SessionDup.code()),
        };
        if cur_policy != 0 && new_policy < cur_policy {
            self.ledger.rejected_regress = self.ledger.rejected_regress.saturating_add(1);
            return Err(HsCode::GenRegress.code());
        }
        // 全过：Bound→SwapPending（单向梯）。倒退/越级也入账，
        // 保证 attempts = applied + 四路拒绝 恒成立。
        let cur_state = match self.sessions.get(idx) {
            Some(s) => s.state,
            None => return Err(HsCode::SessionDup.code()),
        };
        let next = match state_advance(cur_state, 2) {
            Ok(n) => n,
            Err(c) => {
                self.ledger.rejected_state = self.ledger.rejected_state.saturating_add(1);
                return Err(c);
            }
        };
        match self.sessions.get_mut(idx) {
            Some(s) => {
                s.state = next;
                s.pending_policy = new_policy;
            }
            None => return Err(HsCode::StateIllegal.code()),
        }
        Ok(())
    }

    /// 静默边界提交：SwapPending → Swapped，gen+1 落账。
    /// 钉扎会话在本边界跳过（存活进程不受扰），解钉后由下一边界补挂。
    pub fn commit_at_boundary(&mut self) -> u32 {
        let gen = self.gen_counter;
        let mut applied = 0u32;
        let mut i = 0;
        while i < self.sessions.len() {
            let (state, pinned) = match self.sessions.get(i) {
                Some(s) => (s.state, s.pinned),
                None => (0u8, false),
            };
            if state == 2 && !pinned {
                let pending = match self.sessions.get(i) {
                    Some(s) => s.pending_policy,
                    None => 0,
                };
                match self.sessions.get_mut(i) {
                    Some(s) => {
                        s.state = 3;
                        s.policy_id = pending;
                        s.policy_gen = s.policy_gen.saturating_add(1);
                    }
                    None => {}
                }
                applied = applied.saturating_add(1);
            } else if state == 2 && pinned {
                // 钉扎会话保持 SwapPending：代数不动，计数不受扰入账。
                self.ledger.pinned_untouched = self.ledger.pinned_untouched.saturating_add(1);
            }
            i += 1;
        }
        if applied > 0 {
            // 全局代数只在真生效时单调 +1。
            self.gen_counter = gen.saturating_add(1);
            self.ledger.applied = self.ledger.applied.saturating_add(applied);
        }
        applied
    }

    /// 钉扎/解钉（模拟存活进程进出）。
    pub fn set_pinned(&mut self, id: u32, pin: bool) -> Result<(), u32> {
        let mut i = 0;
        while i < self.sessions.len() {
            if let Some(s) = self.sessions.get(i) {
                if s.id == id {
                    match self.sessions.get_mut(i) {
                        Some(ss) => {
                            ss.pinned = pin;
                            return Ok(());
                        }
                        None => return Err(HsCode::StateIllegal.code()),
                    }
                }
            }
            i += 1;
        }
        Err(HsCode::SessionDup.code())
    }

    /// 事务开合。
    pub fn set_txn(&mut self, id: u32, open: bool) -> Result<(), u32> {
        let mut i = 0;
        while i < self.sessions.len() {
            if let Some(s) = self.sessions.get(i) {
                if s.id == id {
                    match self.sessions.get_mut(i) {
                        Some(ss) => {
                            ss.txn_open = open;
                            return Ok(());
                        }
                        None => return Err(HsCode::StateIllegal.code()),
                    }
                }
            }
            i += 1;
        }
        Err(HsCode::SessionDup.code())
    }

    /// 全局代数只读。
    pub fn global_gen(&self) -> u32 {
        self.gen_counter
    }

    /// 会话数只读。
    pub fn session_count(&self) -> usize {
        self.sessions.len()
    }
}

// ── 注入核（3 类坏样本，逐类必拒） ──────────────────────────────
/// 注入检出账（tries/caught 双列 + 漏检守恒）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InjectLedger {
    pub tries: [u32; 3],
    pub caught: [u32; 3],
}

impl InjectLedger {
    pub fn new() -> InjectLedger {
        InjectLedger { tries: [0; 3], caught: [0; 3] }
    }
    /// 记一轮。
    pub fn record(&mut self, slot: usize, hit: bool) {
        if slot < 3 {
            self.tries[slot] = self.tries[slot].saturating_add(1);
            if hit {
                self.caught[slot] = self.caught[slot].saturating_add(1);
            }
        }
    }
    /// 全抓：三类逐类 try==catch==rounds。
    pub fn all_caught(&self, rounds: u32) -> bool {
        let mut i = 0;
        while i < 3 {
            if self.tries[i] != rounds || self.caught[i] != rounds {
                return false;
            }
            i += 1;
        }
        true
    }
    /// 漏检数（恒零判据）。
    pub fn missed(&self) -> u32 {
        let mut t: u32 = 0;
        let mut c: u32 = 0;
        let mut i = 0;
        while i < 3 {
            t = t.saturating_add(self.tries[i]);
            c = c.saturating_add(self.caught[i]);
            i += 1;
        }
        t.saturating_sub(c)
    }
}

/// 注入轮：事务中变更 / 未知策略 / 状态倒退三类各一轮。
pub fn inject_round(rounds: u32) -> InjectLedger {
    let mut led = InjectLedger::new();
    let mut r = 0;
    while r < rounds {
        // 类 1：事务开放中变更 → 必拒。
        let mut m = HotSwapMachine::new();
        let _ = m.register(1);
        let _ = m.set_txn(1, true);
        let _ = m.begin_swap(1, 3);
        led.record(0, m.get_session(1).map(|s| s.state).unwrap_or(0) == 1);
        // 类 2：未知策略（7 超出 1..=6）→ 必拒。
        let mut m2 = HotSwapMachine::new();
        let _ = m2.register(2);
        led.record(1, m2.begin_swap(2, 7).is_err());
        // 类 3：代数倒退（先升 4 再请 2）→ 必拒。
        let mut m3 = HotSwapMachine::new();
        let _ = m3.register(3);
        let _ = m3.begin_swap(3, 4);
        let _ = m3.commit_at_boundary();
        led.record(2, m3.begin_swap(3, 2).is_err());
        r += 1;
    }
    led
}

// ── 黄金全链 ────────────────────────────────────────────────────
/// 黄金全链：注册 4 会话（含 1 钉扎）→ 4 会话全申请变更 → 边界提交
/// → 非钉扎全部生效新策略、钉扎会话代数不动（存活进程不受扰）
/// → 解钉再提交补挂 → 守恒。
pub fn golden_run() -> Result<(u32, u32, u32), u32> {
    let mut m = HotSwapMachine::new();
    let mut i = 1;
    while i <= 4 {
        m.register(i)?;
        i += 1;
    }
    // 钉扎会话 3（模拟存活进程）。
    m.set_pinned(3, true)?;
    // 四会话各申请一次变更（策略 4）。
    let mut k = 1;
    while k <= 4 {
        m.begin_swap(k, 4)?;
        k += 1;
    }
    let touched = m.commit_at_boundary();
    if touched != 3 {
        return Err(HsCode::ConservationBroken.code());
    }
    // 断言一：非钉扎三会话全部生效新策略。
    let mut j = 1;
    while j <= 4 {
        if let Some(s) = m.get_session(j) {
            if j != 3 {
                if s.policy_id != 4 || s.policy_gen != 1 || s.state != 3 {
                    return Err(HsCode::ConservationBroken.code());
                }
            } else {
                // 断言二：钉扎会话（存活进程）代数与策略全程不动。
                if s.policy_id != 0 || s.policy_gen != 0 || s.state != 2 {
                    return Err(HsCode::PinnedDisturbed.code());
                }
            }
        }
        j += 1;
    }
    // 解钉后下一边界补挂。
    m.set_pinned(3, false)?;
    let touched2 = m.commit_at_boundary();
    if touched2 != 1 {
        return Err(HsCode::ConservationBroken.code());
    }
    let s3 = match m.get_session(3) {
        Some(s) => s,
        None => return Err(HsCode::SessionDup.code()),
    };
    if s3.policy_id != 4 || s3.policy_gen != 1 {
        return Err(HsCode::ConservationBroken.code());
    }
    m.ledger.conserve()?;
    Ok((m.ledger.applied, m.ledger.attempts, m.global_gen()))
}

/// 探针三序（正常生效/钉扎不受扰/倒退拒）。
pub fn probe_seq_a() -> Result<u32, u32> {
    let mut m = HotSwapMachine::new();
    m.register(1)?;
    m.begin_swap(1, 2)?;
    m.commit_at_boundary();
    match m.get_session(1) {
        Some(s) if s.policy_id == 2 && s.policy_gen == 1 => Ok(s.policy_gen),
        _ => Err(HsCode::ConservationBroken.code()),
    }
}

/// 存活进程不受扰：钉扎期间连续两轮提交，代数保持 0。
pub fn probe_seq_b() -> Result<u32, u32> {
    let mut m = HotSwapMachine::new();
    m.register(1)?;
    m.set_pinned(1, true)?;
    m.begin_swap(1, 5)?;
    m.commit_at_boundary();
    m.commit_at_boundary();
    match m.get_session(1) {
        Some(s) if s.pinned && s.policy_gen == 0 => Ok(s.policy_gen),
        _ => Err(HsCode::PinnedDisturbed.code()),
    }
}

/// 代数倒退拒探针。
pub fn probe_seq_c() -> Result<(), u32> {
    let mut m = HotSwapMachine::new();
    m.register(1)?;
    m.begin_swap(1, 6)?;
    m.commit_at_boundary();
    match m.begin_swap(1, 1) {
        Err(c) if c == HsCode::GenRegress.code() => Ok(()),
        _ => Err(HsCode::GenRegress.code()),
    }
}

/// 双跑一致。
pub fn repro_check() -> Result<(), u32> {
    let a = golden_run()?;
    let b = golden_run()?;
    if a != b {
        return Err(HsCode::NotReproducible.code());
    }
    Ok(())
}

/// 段位守卫。
pub fn segment_guard() -> bool {
    let mut i = 0;
    while i < HsCode::ALL.len() {
        let v = HsCode::ALL[i].code();
        if v < SEG_BASE || v >= SEG_BASE + 0x40 {
            return false;
        }
        i += 1;
    }
    SEG_BASE % 0x40 == 0
}

/// 文案闭集。
pub fn detail_closed() -> bool {
    let mut i = 0;
    while i < HsCode::ALL.len() {
        if HsCode::ALL[i].detail().is_empty() {
            return false;
        }
        i += 1;
    }
    true
}

/// 判据汇总。
pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut v: Vec<(&'static str, bool, String)> = Vec::new();
    // J1 主判据：运行中变更下会话生效策略断言全过。
    let g = golden_run();
    let g_ok = match &g {
        Ok((a, at, gen)) => *a == 4 && *at == 4 && *gen == 2,
        Err(_) => false,
    };
    v.push(("J1-会话生效策略断言全过", g_ok, format!("{:?}", g)));
    // J1 存活进程不受扰断言。
    v.push(("J1-存活进程不受扰", probe_seq_b() == Ok(0u32), String::from("钉扎两轮提交代数保持 0")));
    // 注入三类 10/10。
    let inj = inject_round(JUDGE_ROUNDS);
    v.push((
        "J1-注入三类10/10",
        inj.all_caught(JUDGE_ROUNDS) && inj.missed() == 0,
        format!("t={:?} c={:?}", inj.tries, inj.caught),
    ));
    // 状态机单向梯：倒退/越级必拒。
    let ladder_bad = state_advance(3, 1).is_err() && state_advance(0, 3).is_err();
    v.push(("J2-状态机倒退越级拒", ladder_bad, String::from("3→1 与 0→3 均拒")));
    // 状态机正例：相邻前进。
    v.push(("J2-状态机相邻前进", state_advance(1, 2) == Ok(2u8), String::from("1→2 放行")));
    // 容量边界：8 满 + 第 9 拒 + 重复 id 拒。
    let mut m = HotSwapMachine::new();
    let mut i = 1;
    while i <= SESSION_CAP as u32 {
        let _ = m.register(i);
        i += 1;
    }
    let full_reject = m.register(9).err() == Some(HsCode::SessionFull.code());
    let dup_reject = HotSwapMachine::new().register(1).is_ok()
        && {
            let mut m2 = HotSwapMachine::new();
            let _ = m2.register(1);
            m2.register(1).err() == Some(HsCode::SessionDup.code())
        };
    v.push((
        "J2-容量与重复边界",
        full_reject && dup_reject,
        format!("cap={} 满+重复双拒", SESSION_CAP),
    ));
    // 守恒律。
    let cons = {
        let mut m3 = HotSwapMachine::new();
        let _ = m3.register(1);
        let _ = m3.set_txn(1, true);
        let _ = m3.begin_swap(1, 2); // 拒（txn）
        let _ = m3.set_txn(1, false);
        let _ = m3.begin_swap(1, 2); // 过
        let _ = m3.commit_at_boundary();
        m3.ledger.conserve().is_ok()
    };
    v.push(("J2-applied+rejected守恒", cons, String::from("四路拒绝+applied=attempts")));
    // 探针三序。
    v.push(("J2-探针三序", probe_seq_a() == Ok(1u32) && probe_seq_c().is_ok(),
        String::from("生效/钉扎/倒退三序")));
    // 双跑一致。
    v.push(("J2-双跑一致", repro_check().is_ok(), String::from("golden_run 两跑同值")));
    // 段位守卫 + 文案闭集。
    v.push(("J3-段位守卫", segment_guard(), String::from("0x805C0 窗口")));
    v.push(("J3-文案闭集", detail_closed(), String::from("9 码逐条非空")));
    v
}

/// 判据收口（动态明细槽仅 8 个：前 8 条走 add_dyn，余项静态明细）。
pub fn run_unx18552_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F18552 档案热切换（运行中 bottle 变更策略）");
    let items = extra_checks();
    let mut i = 0;
    while i < items.len() {
        if let Some((n, p, d)) = items.get(i) {
            if i < 8 {
                s.add_dyn(n, *p, d);
            } else {
                s.add(n, *p, "明细见域账/前序动态槽");
            }
        }
        i += 1;
    }
    s
}
// ═══════════════════════════════════════════════════════════════════════
// 测试区
// ═══════════════════════════════════════════════════════════════════════
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn golden_sessions_take_new_policy() {
        let g = golden_run().unwrap_or((0, 0, 0));
        assert_eq!(g.0, 4, "四会话全部生效（含钉扎解钉补挂）");
        assert_eq!(g.2, 2, "全局代数两次边界单调至 2");
    }

    #[test]
    fn pinned_process_untouched() {
        assert_eq!(probe_seq_b(), Ok(0u32), "钉扎期间代数恒 0");
        let mut m = HotSwapMachine::new();
        let _ = m.register(1);
        let _ = m.set_pinned(1, true);
        let _ = m.begin_swap(1, 4);
        let _ = m.commit_at_boundary();
        let s = m.get_session(1).unwrap_or(Session::new(1));
        assert_eq!(s.policy_gen, 0, "存活进程策略代数不受扰");
        assert_eq!(m.ledger.pinned_untouched, 1, "不受扰计数入账");
    }

    #[test]
    fn injections_three_classes() {
        let inj = inject_round(JUDGE_ROUNDS);
        assert!(inj.all_caught(JUDGE_ROUNDS));
        assert_eq!(inj.missed(), 0);
    }

    #[test]
    fn state_machine_monotonic_ladder() {
        assert!(state_advance(0, 1).is_ok());
        assert!(state_advance(1, 2).is_ok());
        assert!(state_advance(2, 3).is_ok());
        assert!(state_advance(3, 2).is_err(), "Swapped→SwapPending 倒退拒");
        assert!(state_advance(0, 3).is_err(), "Idle→Swapped 越级拒");
    }

    #[test]
    fn generation_monotonic_global() {
        let mut m = HotSwapMachine::new();
        let _ = m.register(1);
        let _ = m.begin_swap(1, 2);
        let _ = m.commit_at_boundary();
        let g1 = m.global_gen();
        let _ = m.begin_swap(1, 3);
        let _ = m.commit_at_boundary();
        assert!(m.global_gen() == g1.saturating_add(1), "代数单调 +1");
        assert!(m.global_gen() > 0);
    }

    #[test]
    fn capacity_and_duplicate_boundaries() {
        let mut m = HotSwapMachine::new();
        let mut i = 1;
        while i <= SESSION_CAP as u32 {
            assert!(m.register(i).is_ok());
            i += 1;
        }
        assert_eq!(m.register(9).err(), Some(HsCode::SessionFull.code()));
        assert_eq!(m.session_count(), SESSION_CAP);
        let mut m2 = HotSwapMachine::new();
        let _ = m2.register(1);
        assert_eq!(m2.register(1).err(), Some(HsCode::SessionDup.code()));
    }

    #[test]
    fn conservation_law() {
        assert!(repro_check().is_ok(), "双跑一致");
        let mut m = HotSwapMachine::new();
        let _ = m.register(1);
        let _ = m.set_txn(1, true);
        let _ = m.begin_swap(1, 2);
        let _ = m.set_txn(1, false);
        let _ = m.begin_swap(1, 2);
        let _ = m.commit_at_boundary();
        assert!(m.ledger.conserve().is_ok());
        assert_eq!(m.ledger.attempts, 2);
    }

    #[test]
    fn checkset_all_green() {
        let set = run_unx18552_checks();
        assert!(set.all_passed(), "判据集全绿：{}", set.red_items_text());
    }
}
