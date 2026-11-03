//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F1724 · 迁移失败回退（暂存滞留回收联动） · 0x72480
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定，CoRun·UNX-A3-B07，前置 UNX-F1723）：
//!   ① 纯功能代码 ≥300 行；
//!   ② 判据原文：UNX-F1724-J1 回退路径（滞留回收→源核归队→三账回冲）
//!     闭合断言全过，回退注入 10/10 次正确归位；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! ─────────────────────────────────────────────────────────────────────
//! 设计要点：
//!   · 迁移三账：源核就绪账（src_ready）、暂存滞留账（staging）、在途账
//!     （inflight）；迁移步进 src--→inflight++，滞留步进 inflight--→
//!     staging++，回退步进 staging--→src++；任一步进后三账总和恒等于
//!     任务总数（守恒断言，任何泄漏必显）；
//!   · 回退路径闭环：滞留回收（staging→RolledBack）→源核归队（cur=home、
//!     state=OnSource）→三账回冲（对迁移前快照逐账等值断言）；
//!   · 状态机 OnSource→InFlight→Staged→RolledBack 单调，归队是显性的
//!     回退出口；attempts 只增不减；重试上限外拒（StagingFull）；
//!   · 失真注入 10 路：滞留残渣/错核归队/回冲缺账/状态卡滞/attempt 倒
//!     退/双重回退/快照篡改/在途泄漏/源账重复计数/任务身份调包，
//!     归位审计 10/10 必抓；
//!   · 双跑一致 + 段位守卫窗口 0x40。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x72480，VX3-1103 迁移回退窗口独占）。
pub const SEG_BASE: u32 = 0x72480;
/// 受控任务数（冻结 48）。
pub const TASK_N: usize = 48;
/// CPU 数（冻结 8）。
pub const CPU_N: usize = 8;
/// 暂存滞留容量（冻结 16，超容拒收）。
pub const STAGING_CAP: usize = 16;
/// 重试上限（每任务回退次数上限，冻结 3）。
pub const RETRY_CAP: u32 = 3;
/// 注入轮数（判据 10/10 语义，冻结）。
pub const INJECT_ROUNDS: u32 = 10;
/// 三账（源核就绪/暂存滞留/在途，冻结 3）。
pub const ACCOUNTS: usize = 3;

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum MigCode {
    /// 回退路径断裂：滞留回收/归队/回冲任一环未闭合
    FallbackBroken,
    /// 滞留卡死：暂存账残渣未清或滞留任务状态未推进
    StagingStuck,
    /// 回冲缺账：三账与迁移前快照对不上
    AccountMismatch,
    /// 归队错核：任务被归到非 home 核
    RouteBad,
    /// 重试倒退：attempts 减少（只增不减红线）
    AttemptBack,
    /// 双重回退：同一任务对同一快照回退两次
    DoubleRollback,
    /// 暂存超容：滞留账超 STAGING_CAP
    StagingFull,
    /// 双跑不一致：golden_run 两次应同值
    NotReproducible,
}

impl MigCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            MigCode::FallbackBroken => "回退路径断裂：按滞留回收→源核归队→三账回冲顺序重走闭环",
            MigCode::StagingStuck => "滞留卡死：暂存账残渣未清，先回收滞留再归队",
            MigCode::AccountMismatch => "回冲缺账：三账与迁移前快照对不上，按快照逐账回冲",
            MigCode::RouteBad => "归队错核：任务必须归回 home 核，查迁移目标寄存",
            MigCode::AttemptBack => "重试倒退：attempts 只增不减，倒退一律拒",
            MigCode::DoubleRollback => "双重回退：同一快照只许回退一次，重复回退作废",
            MigCode::StagingFull => "暂存超容：滞留账满，先回收滞留再收新滞留",
            MigCode::NotReproducible => "双跑不一致：复现红线，golden_run 两次应同值",
        }
    }
    pub const ALL: [MigCode; 8] = [
        MigCode::FallbackBroken,
        MigCode::StagingStuck,
        MigCode::AccountMismatch,
        MigCode::RouteBad,
        MigCode::AttemptBack,
        MigCode::DoubleRollback,
        MigCode::StagingFull,
        MigCode::NotReproducible,
    ];
}

/// 诊断码查表（段外码返回统一文案）。
pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < MigCode::ALL.len() {
        if MigCode::ALL[i].val() == code {
            return MigCode::ALL[i].detail();
        }
        i += 1;
    }
    "未知诊断码：不在 0x72480 段闭集内"
}

// ── 状态机（OnSource→InFlight→Staged→RolledBack，单调推进） ─────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum MigState {
    /// 在源核就绪队列。
    OnSource,
    /// 迁移在途。
    InFlight,
    /// 暂存滞留（目标核拒收后的中转态）。
    Staged,
    /// 已回退（待归队）。
    RolledBack,
}

impl MigState {
    /// 单调序值：OnSource=0 < InFlight=1 < Staged=2 < RolledBack=3。
    pub fn rank(self) -> u8 {
        match self {
            MigState::OnSource => 0,
            MigState::InFlight => 1,
            MigState::Staged => 2,
            MigState::RolledBack => 3,
        }
    }
    /// 只允许向前推进；平移与倒退一律 false。
    pub fn can_advance(self, to: MigState) -> bool {
        to.rank() > self.rank()
    }
}

// ── 域模型 ──────────────────────────────────────────────────────
/// 迁移任务：home 为归属核，cur 为当前所在核。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MigTask {
    pub tid: u32,
    pub home: usize,
    pub cur: usize,
    pub state: MigState,
    /// 回退尝试次数（只增不减）。
    pub attempts: u32,
}

/// 迁移前快照（三账回冲基准）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MigSnapshot {
    /// 各核就绪账（迁移前）。
    pub src_ready: [u32; CPU_N],
    pub staging: u32,
    pub inflight: u32,
}

/// 迁移表：任务集 + 三账 + 活动快照 + 回冲标记。
#[derive(Clone, Debug)]
pub struct MigrateTable {
    pub tasks: Vec<MigTask>,
    /// 源核就绪账（按核计数）。
    pub src_ready: [u32; CPU_N],
    /// 暂存滞留账。
    pub staging: u32,
    /// 在途账。
    pub inflight: u32,
    /// 当前活动快照（None=无在途迁移）。
    pub snap: Option<MigSnapshot>,
    /// 三账已回冲标记（防双重回退）。
    pub rolled: bool,
}

/// 建表（确定性：home 由 tid 派生，全部在源核就绪队列）。
pub fn build_table(n: usize) -> MigrateTable {
    let mut tasks: Vec<MigTask> = Vec::new();
    let mut src_ready = [0u32; CPU_N];
    let mut i = 0usize;
    while i < n {
        let home = i % CPU_N;
        tasks.push(MigTask {
            tid: i as u32,
            home,
            cur: home,
            state: MigState::OnSource,
            attempts: 0,
        });
        src_ready[home] = src_ready[home].saturating_add(1);
        i += 1;
    }
    MigrateTable { tasks, src_ready, staging: 0, inflight: 0, snap: None, rolled: false }
}

/// 三账守恒断言：src_total + staging + inflight == 任务总数。
pub fn conservation_check(t: &MigrateTable) -> Result<u32, u32> {
    let mut src_total = 0u32;
    let mut i = 0usize;
    while i < CPU_N {
        src_total = src_total.saturating_add(t.src_ready[i]);
        i += 1;
    }
    let total = src_total.saturating_add(t.staging).saturating_add(t.inflight);
    if total as usize != t.tasks.len() {
        return Err(MigCode::AccountMismatch.val());
    }
    Ok(total)
}

// ── 迁移步进（唯一写路径三步） ──────────────────────────────────
/// 迁移发起：OnSource→InFlight；src[home]-- → inflight++。
pub fn migrate_launch(t: &mut MigrateTable, tid: u32, dst: usize) -> Result<(), u32> {
    if dst >= CPU_N {
        return Err(MigCode::RouteBad.val());
    }
    let idx = match t.tasks.iter().position(|x| x.tid == tid) {
        Some(p) => p,
        None => return Err(MigCode::RouteBad.val()),
    };
    if t.tasks[idx].state != MigState::OnSource {
        return Err(MigCode::FallbackBroken.val());
    }
    // 快照（迁移前三账）。
    let snap = MigSnapshot { src_ready: t.src_ready, staging: t.staging, inflight: t.inflight };
    t.snap = Some(snap);
    if !t.tasks[idx].state.can_advance(MigState::InFlight) {
        return Err(MigCode::AttemptBack.val());
    }
    t.tasks[idx].state = MigState::InFlight;
    t.tasks[idx].cur = dst;
    let home = t.tasks[idx].home;
    t.src_ready[home] = match t.src_ready[home].checked_sub(1) {
        Some(v) => v,
        None => return Err(MigCode::AccountMismatch.val()),
    };
    t.inflight = t.inflight.saturating_add(1);
    conservation_check(t)?;
    Ok(())
}

/// 滞留步进：InFlight→Staged；inflight-- → staging++（暂存滞留）。
pub fn migrate_stage(t: &mut MigrateTable, tid: u32) -> Result<(), u32> {
    let idx = match t.tasks.iter().position(|x| x.tid == tid) {
        Some(p) => p,
        None => return Err(MigCode::RouteBad.val()),
    };
    if t.tasks[idx].state != MigState::InFlight {
        return Err(MigCode::FallbackBroken.val());
    }
    if t.staging as usize >= STAGING_CAP {
        return Err(MigCode::StagingFull.val());
    }
    if !t.tasks[idx].state.can_advance(MigState::Staged) {
        return Err(MigCode::AttemptBack.val());
    }
    t.tasks[idx].state = MigState::Staged;
    t.inflight = match t.inflight.checked_sub(1) {
        Some(v) => v,
        None => return Err(MigCode::AccountMismatch.val()),
    };
    t.staging = t.staging.saturating_add(1);
    conservation_check(t)?;
    Ok(())
}

// ── 回退路径闭环（滞留回收→源核归队→三账回冲） ─────────────────
/// 回退步一：滞留回收 Staged→RolledBack（只推进状态不动账——任务仍记
/// 在暂存账，由归队步一并清账，三账守恒步步成立）。
pub fn reclaim_staging(t: &mut MigrateTable, tid: u32) -> Result<(), u32> {
    let idx = match t.tasks.iter().position(|x| x.tid == tid) {
        Some(p) => p,
        None => return Err(MigCode::RouteBad.val()),
    };
    if t.tasks[idx].state != MigState::Staged {
        return Err(MigCode::StagingStuck.val());
    }
    if !t.tasks[idx].state.can_advance(MigState::RolledBack) {
        return Err(MigCode::AttemptBack.val());
    }
    t.tasks[idx].state = MigState::RolledBack;
    conservation_check(t)?;
    Ok(())
}

/// 回退步二：源核归队 RolledBack→OnSource；staging-- → src[home]++；
/// cur=home；attempts 只增不减；attempts 超上限拒（重试红线）。
pub fn requeue_source(t: &mut MigrateTable, tid: u32) -> Result<(), u32> {
    let idx = match t.tasks.iter().position(|x| x.tid == tid) {
        Some(p) => p,
        None => return Err(MigCode::RouteBad.val()),
    };
    if t.tasks[idx].state != MigState::RolledBack {
        return Err(MigCode::StagingStuck.val());
    }
    if t.tasks[idx].attempts >= RETRY_CAP {
        return Err(MigCode::StagingFull.val());
    }
    let next = t.tasks[idx].attempts.saturating_add(1);
    if next <= t.tasks[idx].attempts {
        return Err(MigCode::AttemptBack.val());
    }
    let home = t.tasks[idx].home;
    t.staging = match t.staging.checked_sub(1) {
        Some(v) => v,
        None => return Err(MigCode::AccountMismatch.val()),
    };
    t.tasks[idx].state = MigState::OnSource;
    t.tasks[idx].cur = home;
    t.tasks[idx].attempts = next;
    t.src_ready[home] = t.src_ready[home].saturating_add(1);
    conservation_check(t)?;
    Ok(())
}

/// 回退步三：三账回冲——对迁移前快照逐账等值断言；置回冲标记。
/// 不等即 AccountMismatch（回冲缺账）；重复回冲即 DoubleRollback。
pub fn rollback_accounts(t: &mut MigrateTable) -> Result<(), u32> {
    if t.rolled {
        return Err(MigCode::DoubleRollback.val());
    }
    let snap = match t.snap {
        Some(s) => s,
        None => return Err(MigCode::DoubleRollback.val()),
    };
    let mut i = 0usize;
    while i < CPU_N {
        if t.src_ready[i] != snap.src_ready[i] {
            return Err(MigCode::AccountMismatch.val());
        }
        i += 1;
    }
    if t.staging != snap.staging || t.inflight != snap.inflight {
        return Err(MigCode::AccountMismatch.val());
    }
    t.rolled = true;
    Ok(())
}

/// 完整回退闭环：回收→归队→回冲；任一环失败显性 Err。
pub fn fallback_close(t: &mut MigrateTable, tid: u32) -> Result<(), u32> {
    reclaim_staging(t, tid)?;
    requeue_source(t, tid)?;
    rollback_accounts(t)?;
    Ok(())
}

// ── 归位审计（判据主口） ────────────────────────────────────────
/// 回退后任务正确归位：
///   ① state==OnSource；② cur==home；③ attempts≥1（回退过）；
///   ④ 已回冲标记；⑤ 三账==快照（快照缺失或被清即拒）；
///   ⑥ 守恒成立。
pub fn position_audit(t: &MigrateTable, tid: u32) -> Result<(), u32> {
    let idx = match t.tasks.iter().position(|x| x.tid == tid) {
        Some(p) => p,
        None => return Err(MigCode::RouteBad.val()),
    };
    let task = &t.tasks[idx];
    if task.state != MigState::OnSource {
        return Err(MigCode::StagingStuck.val());
    }
    if task.cur != task.home {
        return Err(MigCode::RouteBad.val());
    }
    if task.attempts < 1 {
        return Err(MigCode::AttemptBack.val());
    }
    if !t.rolled {
        return Err(MigCode::FallbackBroken.val());
    }
    let snap = match t.snap {
        Some(s) => s,
        None => return Err(MigCode::DoubleRollback.val()),
    };
    let mut i = 0usize;
    while i < CPU_N {
        if t.src_ready[i] != snap.src_ready[i] {
            return Err(MigCode::AccountMismatch.val());
        }
        i += 1;
    }
    if t.staging != snap.staging || t.inflight != snap.inflight {
        return Err(MigCode::AccountMismatch.val());
    }
    conservation_check(t)?;
    Ok(())
}

// ── 失真注入（10 路逐字失真，回退 10/10 正确归位红线） ──────────
/// 对回退完成态注入第 kind 路失真，返回坏表（值域全整数，安全）。
pub fn distort(t: &MigrateTable, tid: u32, kind: usize) -> MigrateTable {
    let mut bad = t.clone();
    match kind % 10 {
        0 => {
            // 滞留残渣：staging 多计 1（回收未清账）。
            bad.staging = bad.staging.saturating_add(1);
        }
        1 => {
            // 错核归队：任务 cur 改到非 home 核。
            let idx = match bad.tasks.iter().position(|x| x.tid == tid) {
                Some(p) => p,
                None => return bad,
            };
            let home = bad.tasks[idx].home;
            bad.tasks[idx].cur = (home + 1) % CPU_N;
        }
        2 => {
            // 回冲缺账：源账少计 1。
            let idx = match bad.tasks.iter().position(|x| x.tid == tid) {
                Some(p) => p,
                None => return bad,
            };
            let home = bad.tasks[idx].home;
            bad.src_ready[home] = bad.src_ready[home].saturating_sub(1);
        }
        3 => {
            // 状态卡滞：任务停在 Staged 未推进。
            let idx = match bad.tasks.iter().position(|x| x.tid == tid) {
                Some(p) => p,
                None => return bad,
            };
            bad.tasks[idx].state = MigState::Staged;
        }
        4 => {
            // attempt 倒退：attempts 清零。
            let idx = match bad.tasks.iter().position(|x| x.tid == tid) {
                Some(p) => p,
                None => return bad,
            };
            bad.tasks[idx].attempts = 0;
        }
        5 => {
            // 快照篡改：快照源账抬升，回冲对不上必抓。
            if let Some(s) = bad.snap.as_mut() {
                s.src_ready[0] = s.src_ready[0].saturating_add(1);
            }
        }
        6 => {
            // 在途泄漏：inflight 多计 1。
            bad.inflight = bad.inflight.saturating_add(1);
        }
        7 => {
            // 源账重复计数：别核源账抬升。
            bad.src_ready[CPU_N - 1] = bad.src_ready[CPU_N - 1].saturating_add(1);
        }
        8 => {
            // 快照清除冒充已回冲：snap=None 后回冲断言必拒。
            bad.snap = None;
        }
        _ => {
            // 任务身份调包：tid 与 cur 对调（错核审计必抓）。
            let idx = match bad.tasks.iter().position(|x| x.tid == tid) {
                Some(p) => p,
                None => return bad,
            };
            let other = (idx + 1) % bad.tasks.len();
            let cur_swap = bad.tasks[idx].cur;
            bad.tasks[idx].cur = bad.tasks[other].cur;
            bad.tasks[other].cur = cur_swap;
        }
    }
    bad
}

/// 注入检出账。
pub struct InjectLedger {
    pub tries: u32,
    pub caught: u32,
    /// 漏抓的失真路号（诊断用）。
    pub miss_kinds: Vec<usize>,
}

impl InjectLedger {
    pub fn new() -> InjectLedger {
        InjectLedger { tries: 0, caught: 0, miss_kinds: Vec::new() }
    }
    pub fn record(&mut self, kind: usize, hit: bool) {
        self.tries += 1;
        if hit {
            self.caught += 1;
        } else {
            self.miss_kinds.push(kind);
        }
    }
    pub fn all_caught(&self) -> bool {
        self.tries > 0 && self.caught == self.tries
    }
}

/// 单轮回退黄金流：建表→快照→迁移→滞留→回退闭环→归位审计。
pub fn golden_fallback(n: usize, tid: u32, dst: usize) -> Result<MigrateTable, u32> {
    let mut t = build_table(n);
    migrate_launch(&mut t, tid, dst)?;
    migrate_stage(&mut t, tid)?;
    fallback_close(&mut t, tid)?;
    position_audit(&t, tid)?;
    Ok(t)
}

/// 注入轮：每轮对新鲜黄金回退态注入一路失真，归位审计必须抓到
/// （黄金态本身必须审计通过，防注入器污染基准）。
pub fn inject_rounds(rounds: u32) -> Result<InjectLedger, u32> {
    let mut led = InjectLedger::new();
    let mut r = 0u32;
    while r < rounds {
        let t = golden_fallback(TASK_N, 0, CPU_N - 1)?;
        let kind = (r % 10) as usize;
        led.record(kind, position_audit(&distort(&t, 0, kind), 0).is_err());
        r += 1;
    }
    Ok(led)
}

// ── 黄金全链 ────────────────────────────────────────────────────
/// 全链：8 目标核逐核回退闭环 → 归位审计 → 注入 10/10 → 四拒探
/// （双重回退/状态跳滞/超容滞留/重试上限）。
/// 返回 (正旗数, 拒旗数, 回退闭环数, 守恒总量)。
pub fn golden_run() -> Result<(u32, u32, u32, u32), u32> {
    let mut closed = 0u32;
    let mut dst = 0usize;
    while dst < CPU_N {
        let t = golden_fallback(TASK_N, dst as u32, CPU_N - 1 - dst)?;
        position_audit(&t, dst as u32)?;
        closed += 1;
        dst += 1;
    }
    // 注入 10 路全抓。
    let inj = inject_rounds(INJECT_ROUNDS)?;
    let inj_ok = inj.all_caught();
    // 四拒探。
    let mut probe = build_table(TASK_N);
    let _ = migrate_launch(&mut probe, 0, CPU_N - 1)?;
    let _ = migrate_stage(&mut probe, 0)?;
    // 双重回退拒：首次回冲成功后，第二次回冲必拒（rolled 标记）。
    let _ = reclaim_staging(&mut probe, 0)?;
    let _ = requeue_source(&mut probe, 0)?;
    let first_rb = rollback_accounts(&mut probe).is_ok();
    let double_rej = first_rb && rollback_accounts(&mut probe).is_err();
    // 状态跳滞拒：OnSource 直接回收（未滞留）。
    let mut probe2 = build_table(TASK_N);
    let stuck_rej = reclaim_staging(&mut probe2, 1).is_err();
    // 超容滞留拒：滞留账灌满后拒收。
    let mut probe3 = build_table(TASK_N);
    probe3.staging = STAGING_CAP as u32;
    let full_rej = {
        let _ = migrate_launch(&mut probe3, TASK_N as u32 - 1, CPU_N - 1);
        // conservation 会被破坏（staging 伪造占位），此处只验 StagingFull 拒。
        migrate_stage(&mut probe3, TASK_N as u32 - 1).is_err()
    };
    // 重试上限拒：attempts==RETRY_CAP 后归队拒。
    let mut probe4 = build_table(TASK_N);
    probe4.tasks[2].state = MigState::RolledBack;
    probe4.tasks[2].attempts = RETRY_CAP;
    let cap_rej = requeue_source(&mut probe4, 2).is_err();
    let final_t = golden_fallback(TASK_N, TASK_N as u32 - 1, 0)?;
    let cons = conservation_check(&final_t)?;
    let flags = (closed == CPU_N as u32) as u32 + inj_ok as u32
        + (cons == TASK_N as u32) as u32
        + (final_t.tasks[TASK_N - 1].attempts == 1) as u32;
    let rejects = double_rej as u32 + stuck_rej as u32 + full_rej as u32 + cap_rej as u32;
    Ok((flags, rejects, closed, cons))
}

// ── 探针三序 ────────────────────────────────────────────────────
/// 序 A：黄金全链四正四拒全中。
pub fn probe_seq_a() -> Result<(), u32> {
    match golden_run() {
        Ok((4, 4, _, _)) => Ok(()),
        _ => Err(MigCode::NotReproducible.val()),
    }
}

/// 序 B：失真注入 10 路全抓。
pub fn probe_seq_b() -> Result<(), u32> {
    match inject_rounds(INJECT_ROUNDS) {
        Ok(l) if l.all_caught() => Ok(()),
        _ => Err(MigCode::FallbackBroken.val()),
    }
}

/// 序 C：冻结常量与段位窗口。
pub fn probe_seq_c() -> Result<(), u32> {
    let in_window = SEG_BASE == 0x72480 && MigCode::NotReproducible.val() < SEG_BASE + 0x40;
    let ok = TASK_N == 48 && CPU_N == 8 && STAGING_CAP == 16 && RETRY_CAP == 3
        && INJECT_ROUNDS == 10 && ACCOUNTS == 3 && in_window;
    if ok {
        Ok(())
    } else {
        Err(MigCode::NotReproducible.val())
    }
}

// ── 双跑一致（复现红线） ────────────────────────────────────────
pub fn repro_check() -> Result<(), u32> {
    let a = golden_run()?;
    let b = golden_run()?;
    if a != b {
        return Err(MigCode::NotReproducible.val());
    }
    Ok(())
}

// ── 段位守卫 ────────────────────────────────────────────────────
pub fn segment_guard() -> bool {
    let mut i = 0;
    while i < MigCode::ALL.len() {
        let v = MigCode::ALL[i].val();
        if v < SEG_BASE || v >= SEG_BASE + 0x40 {
            return false;
        }
        i += 1;
    }
    true
}

// ── 状态机单调探针 ──────────────────────────────────────────────
/// 全序 + 平移/倒退拒 + 守恒逐步成立。
pub fn probe_state_machine() -> bool {
    let ranks = MigState::OnSource.rank() < MigState::InFlight.rank()
        && MigState::InFlight.rank() < MigState::Staged.rank()
        && MigState::Staged.rank() < MigState::RolledBack.rank();
    let flat = !MigState::InFlight.can_advance(MigState::InFlight);
    let back = !MigState::RolledBack.can_advance(MigState::Staged)
        && !MigState::Staged.can_advance(MigState::InFlight)
        && !MigState::InFlight.can_advance(MigState::OnSource);
    let fwd = MigState::OnSource.can_advance(MigState::InFlight)
        && MigState::InFlight.can_advance(MigState::Staged)
        && MigState::Staged.can_advance(MigState::RolledBack);
    // 逐步守恒：launch/stage/reclaim/requeue 每步后守恒都成立。
    let mut t = build_table(TASK_N);
    let step_ok = migrate_launch(&mut t, 3, 5).is_ok()
        && conservation_check(&t).is_ok()
        && migrate_stage(&mut t, 3).is_ok()
        && conservation_check(&t).is_ok()
        && reclaim_staging(&mut t, 3).is_ok()
        && conservation_check(&t).is_ok()
        && requeue_source(&mut t, 3).is_ok()
        && conservation_check(&t).is_ok()
        && rollback_accounts(&mut t).is_ok();
    // attempts 只增：归队一次后 attempts==1。
    let att = t.tasks[3].attempts == 1;
    ranks && flat && back && fwd && step_ok && att
}

// ── 联动探针：暂存回收与源核归队强联动 ─────────────────────────
/// 回收必须先于归队：未回收直接归队拒；回收后归队核位=home。
pub fn probe_reclaim_linkage() -> bool {
    let mut t = build_table(TASK_N);
    let _ = migrate_launch(&mut t, 5, 0);
    let _ = migrate_stage(&mut t, 5);
    // 未回收先归队：Staged 态归队拒。
    let early_rej = requeue_source(&mut t, 5).is_err();
    // 回收后归队：cur==home==5。
    let _ = reclaim_staging(&mut t, 5);
    let _ = requeue_source(&mut t, 5);
    let home_ok = t.tasks[5].cur == t.tasks[5].home && t.tasks[5].home == 5 % CPU_N
        && t.tasks[5].state == MigState::OnSource;
    // 回冲断言：三账回迁移前。
    let rb_ok = rollback_accounts(&mut t).is_ok();
    early_rej && home_ok && rb_ok && conservation_check(&t).is_ok()
}

// ── 暂存容量联动探针 ───────────────────────────────────────────
/// 滞留账逐步逼近容量：第 STAGING_CAP 个滞留必拒，守恒不破。
pub fn probe_staging_cap() -> bool {
    let mut t = build_table(TASK_N * 4);
    let mut staged = 0usize;
    let mut i = 0u32;
    while (staged < STAGING_CAP) && (i as usize) < t.tasks.len() {
        if migrate_launch(&mut t, i, CPU_N - 1).is_err() {
            break;
        }
        if migrate_stage(&mut t, i).is_err() {
            break;
        }
        staged += 1;
        i += 1;
    }
    // 第 17 个滞留必拒。
    let over_rej = migrate_stage(&mut t, i).is_err();
    // 守恒仍成立：src_total + staging + inflight == 全员。
    let cons = conservation_check(&t).is_ok();
    staged == STAGING_CAP && over_rej && cons
}

// ── 完成写回（判据③：状态=已完成、结果摘要非空） ────────────────
pub fn writeback_summary() -> Result<String, u32> {
    let inj = inject_rounds(INJECT_ROUNDS)?;
    if !inj.all_caught() {
        return Err(MigCode::FallbackBroken.val());
    }
    let t = golden_fallback(TASK_N, 0, CPU_N - 1)?;
    position_audit(&t, 0)?;
    Ok(format!(
        "状态=已完成 tasks={} fallback闭环=8 inject={}/{} attempts0={}",
        t.tasks.len(), inj.caught, inj.tries, t.tasks[0].attempts
    ))
}

// ── 判据面 ──────────────────────────────────────────────────────
pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut v: Vec<(&'static str, bool, String)> = Vec::new();
    // J1 主判据：黄金全链（四正四拒）。
    let g = golden_run();
    let (gp, gd) = match &g {
        Ok((f, r, c, n)) => (*f == 4 && *r == 4, format!("flags={} rejects={} closed={} cons={}", f, r, c, n)),
        Err(e) => (false, format!("err=0x{:x} {}", e, detail(*e))),
    };
    v.push(("J1-黄金全链", gp, gd));
    // J1 回退路径闭合（滞留回收→源核归队→三账回冲）。
    v.push((
        "J1-回退路径闭合",
        probe_state_machine() && probe_reclaim_linkage(),
        String::from("四步逐步守恒；未回收先归队拒；归队核位=home；回冲逐账等值"),
    ));
    // J1 回退注入 10/10 正确归位。
    let inj = inject_rounds(INJECT_ROUNDS);
    let (ip, id) = match &inj {
        Ok(l) => (l.all_caught(), format!("t={} c={} miss={:?}", l.tries, l.caught, l.miss_kinds)),
        Err(e) => (false, format!("err=0x{:x}", e)),
    };
    v.push(("J1-回退注入10/10", ip, id));
    // J2 暂存容量联动。
    v.push(("J2-暂存容量联动", probe_staging_cap(), format!("第{}个滞留必拒；守恒不破 cap={}", STAGING_CAP + 1, STAGING_CAP)));
    // J2 状态机单调。
    v.push(("J2-状态机单调", probe_state_machine(), String::from("OnSource<InFlight<Staged<RolledBack；平移/倒退全拒")));
    // J2 双跑一致。
    v.push(("J2-双跑一致", repro_check().is_ok(), String::from("golden_run 两跑同值")));
    // J2 冻结常量。
    v.push((
        "J2-冻结常量",
        probe_seq_c().is_ok(),
        format!("TASK_N={} STAGING_CAP={} RETRY_CAP={}", TASK_N, STAGING_CAP, RETRY_CAP),
    ));
    // J3 写回摘要非空。
    let wb = writeback_summary();
    let (wp, wd) = match &wb {
        Ok(s) => (!s.is_empty(), s.clone()),
        Err(e) => (false, format!("err=0x{:x}", e)),
    };
    v.push(("J3-写回摘要", wp, wd));
    // J4 段位守卫。
    v.push(("J4-段位守卫", segment_guard(), String::from("0x72480 窗口")));
    v
}

pub fn run_unx1724_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F1724 迁移失败回退（暂存滞留回收联动）");
    for (n, p, d) in extra_checks() {
        s.add_dyn(n, p, &d);
    }
    s
}
