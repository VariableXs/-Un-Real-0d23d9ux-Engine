//! UNX-F21598 · 域写锁（防他人/防自误改）
//!
//! 诊断段 0x91089~0x91090（F2-B40 域收官段 18/100）。
//!
//! 判据原文（施工书 UNX-F21598-J1）：写锁判据：域账二次写入尝试被校验器拒绝并告警。
//!
//! 设计要点（本单与 F21588 的「封域写锁」不是同一个东西）：
//! F21588 管的是「封域之后禁新增条目」——单向、无持有者、无租期，宣示一次永久生效；
//! 本单管的是「多工号并发写入的互斥」——谁在写、写多久、到期怎么收回、别人来抢怎么记账。
//! 两单共用「写锁」这个词，但一个是闸门（gate，状态随域而变），一个是互斥量（mutex，
//! 状态随持有者而变）。若合并建模，「加锁」会同时意味着封域，两种语义就再也拆不开了。
//!
//! 时钟只用单调 tick：墙钟被 NTP 回跳后，「已过期」会倒流成「未过期」，同一把锁会被两个
//! 工号同时持有——这是互斥量最经典的双持有缺陷。tick 由 `tick()` 显式推进，判据因此可以
//! 逐 tick 重放，不依赖任何外部时序。
//!
//! 续租窗口只在租约尾段开放（剩余 <= RENEW_WINDOW）：若允许随时续租，持有者每次刚加锁就
//! 续一次，等待方的时间上界就被无限推后，互斥退化成永久占用。
//!
//! 过期不即时清理、而是留到下一次操作才回收（`settle`）：这样「过期后旧持有者仍来续租/释放」
//! 能被单独报成 `LeaseExpired`（防自误改），而不是被无差别地报成「没有锁」。

use alloc::string::{String, ToString};
use alloc::vec::Vec;

/// 租约时长（tick）。12 是判据里能走完「早期续租 / 窗口续租 / 过期」三段的整值。
pub const LEASE_TTL: u64 = 12;
/// 续租窗口（只允许在剩余 tick <= 该值时续租）。
pub const RENEW_WINDOW: u64 = 4;
/// 同一持有者的重入深度上限（超过即视为递归失控，而不是正常嵌套）。
pub const REENTRY_CAP: u16 = 3;
/// 告警账容量（按工号去重；满则溢出计数显性记账，不静默丢）。
pub const MAX_ALARMS: usize = 8;

/// 写锁诊断码（本单码段仅两位）。
///
/// 只有两位不是偷懒：派工单把 0x91089/0x91090 定成「授予/拒绝」这一对语义，具体成因交给
/// `DenyCause`。若把六种成因摊成六个码，就会侵占相邻单的码段，日志对账时与别单撞键。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum LockCode {
    /// 0x91089 授予成功（新租约或重入）。
    LockOk = 0x91089,
    /// 0x91090 写入请求被锁拒绝（成因见 `LockFault::cause`）。
    LockDenied = 0x91090,
}

impl LockCode {
    /// 现象。
    pub const fn what(self) -> &'static str {
        match self {
            LockCode::LockOk => "写入请求获得写锁",
            LockCode::LockDenied => "写入请求被写锁拒绝",
        }
    }
    /// 原因。
    pub const fn why(self) -> &'static str {
        match self {
            LockCode::LockOk => "锁空闲或由本工号持有且未超深",
            LockCode::LockDenied => "锁由他人持有、已过期，或请求本身不合法",
        }
    }
    /// 建议。
    pub const fn advice(self) -> &'static str {
        match self {
            LockCode::LockOk => "完成写入后按重入次数配对释放",
            LockCode::LockDenied => "按成因处理：等待释放、续租或重新申购",
        }
    }
    /// 数值形式（u32：五位数码段，`as u16` 会截断成 0x1089）。
    pub const fn code(self) -> u32 {
        self as u32
    }
}

/// 拒绝成因（六元闭集，定位粒度在此而不在码上）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DenyCause {
    /// 锁由他人持有。
    NotHolder,
    /// 同一持有者重入超过上限。
    ReentryOverflow,
    /// 租约已过期（含过期后仍来续租/释放）。
    LeaseExpired,
    /// 未进续租窗口就续租。
    RenewTooEarly,
    /// 本来就没有锁（空锁续租、重复释放）。
    NoLease,
    /// 工号为空，无法认定持有者。
    HolderUnknown,
}

impl DenyCause {
    /// 短标签（导出文本与日志用，稳定不可改）。
    pub const fn label(self) -> &'static str {
        match self {
            DenyCause::NotHolder => "not_holder",
            DenyCause::ReentryOverflow => "reentry_overflow",
            DenyCause::LeaseExpired => "lease_expired",
            DenyCause::RenewTooEarly => "renew_too_early",
            DenyCause::NoLease => "no_lease",
            DenyCause::HolderUnknown => "holder_unknown",
        }
    }
    /// 该成因是否属于「他人干扰」（用来区分防他人与防自误改两类告警）。
    pub const fn is_intrusion(self) -> bool {
        matches!(self, DenyCause::NotHolder | DenyCause::HolderUnknown)
    }
}

/// 拒绝记录（码 + 成因 + 是否他人干扰）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct LockFault {
    /// 诊断码（u32）。
    pub code: u32,
    /// 成因。
    pub cause: DenyCause,
}

impl LockFault {
    const fn denied(cause: DenyCause) -> LockFault {
        LockFault { code: LockCode::LockDenied.code(), cause }
    }
    /// 码是否确为拒绝码。
    pub const fn is_denied(self) -> bool {
        self.code == LockCode::LockDenied as u32
    }
    /// 三要素 + 成因拼一行（运维面板直读）。
    pub fn text(&self) -> String {
        let mut out = String::with_capacity(160);
        out.push_str(LockCode::LockDenied.what());
        out.push_str(" | ");
        out.push_str(LockCode::LockDenied.why());
        out.push_str(" | ");
        out.push_str(LockCode::LockDenied.advice());
        out.push_str(" | cause=");
        out.push_str(self.cause.label());
        out
    }
}

/// 锁状态（相对某个工号的观测结果，不是锁自身的绝对状态）。
///
/// 「由谁持锁」决定了状态该怎么读：同一把锁对甲是 `Locked`、对乙是 `LockedByOther`。
/// 把状态做成绝对量（只记 Held/Free）会丢掉「谁在看」这一维，而互斥判据恰恰只关心它。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum LockState {
    /// 无人持有。
    Unlocked,
    /// 本工号持有。
    Locked,
    /// 他人持有。
    LockedByOther,
}

impl LockState {
    /// 短标签（导出用）。
    pub const fn label(self) -> &'static str {
        match self {
            LockState::Unlocked => "unlocked",
            LockState::Locked => "locked",
            LockState::LockedByOther => "locked_by_other",
        }
    }
    /// 是否处于「被持有」形态（含他人持有）。
    pub const fn is_taken(self) -> bool {
        !matches!(self, LockState::Unlocked)
    }
}

/// 锁视图（一次加锁/查询的回执）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct LockView {
    /// 观测状态。
    pub state: LockState,
    /// 租约序号（每次新授予 +1，重入不换序号）。
    pub seq: u64,
    /// 本工号在本次租约上的重入深度。
    pub depth: u16,
    /// 租约剩余 tick（未持有时为 0）。
    pub remaining: u64,
}

impl LockView {
    /// 本工号是否真的握着锁。
    pub const fn is_held(&self) -> bool {
        matches!(self.state, LockState::Locked)
    }
}

/// 告警账条目（按工号去重累计）。
///
/// 去重而不是逐次追加：一个工号刷一百次拒绝，日志里应该是一行「count=100」，而不是
/// 一百行把别的工号的记录挤出容量。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Alarm {
    /// 违规工号。
    pub worker: String,
    /// 累计次数。
    pub count: u16,
    /// 最近一次成因。
    pub last: DenyCause,
}

impl Alarm {
    /// 是否已被反复告警（>= 3 次视为惯犯，运维优先处理）。
    pub fn is_repeat(&self) -> bool {
        self.count >= 3
    }
}

/// 域写锁。
pub struct DomainLock {
    holder: Option<String>,
    /// 租约序号（单调，跨解锁不复位；判据靠它证明「换过人」）。
    seq: u64,
    /// 本次租约的起始 tick。
    acquired_at: u64,
    /// 当前重入深度（0 表示无人持有）。
    depth: u16,
    /// 单调 tick（替代墙钟）。
    clock: u64,
    grants: u16,
    denials: u16,
    expiries: u16,
    renewals: u16,
    alarms: Vec<Alarm>,
    alarm_overflow: u16,
}

impl DomainLock {
    /// 新建一把空锁。
    pub const fn new() -> DomainLock {
        DomainLock {
            holder: None,
            seq: 0,
            acquired_at: 0,
            depth: 0,
            clock: 0,
            grants: 0,
            denials: 0,
            expiries: 0,
            renewals: 0,
            alarms: Vec::new(),
            alarm_overflow: 0,
        }
    }

    /// 租约时长（判据要拿它算边界，故对外可读）。
    pub const fn ttl(&self) -> u64 {
        LEASE_TTL
    }

    /// 当前 tick。
    pub const fn now(&self) -> u64 {
        self.clock
    }

    /// 推进单调 tick（只进不退；回退请求被夹成 0 步）。
    ///
    /// 不做 `clock -= n` 的原因：tick 的语义是单调，允许回退就等于把墙钟的缺陷搬进来。
    pub fn tick(&mut self, n: u64) {
        self.clock = self.clock.saturating_add(n);
    }

    /// 是否已过租约终点（按当前 tick 判，不改状态）。
    fn expired(&self) -> bool {
        if self.holder.is_none() {
            return false;
        }
        self.clock >= self.acquired_at.saturating_add(LEASE_TTL)
    }

    /// 剩余 tick（未持有时为 0）。
    fn remaining_ticks(&self) -> u64 {
        if self.holder.is_none() {
            return 0;
        }
        let end = self.acquired_at.saturating_add(LEASE_TTL);
        if end <= self.clock {
            0
        } else {
            end - self.clock
        }
    }

    /// 回收过期租约（原子地把持有者摘掉）。
    fn settle(&mut self) {
        if self.expired() {
            self.holder = None;
            self.depth = 0;
            self.expiries = self.expiries.saturating_add(1);
        }
    }

    /// 是否由该工号持有（不看是否过期——过期与否由调用点决定先后）。
    fn holds(&self, worker: &str) -> bool {
        match self.holder.as_deref() {
            Some(h) => h == worker,
            None => false,
        }
    }

    /// 某工号看到的锁状态（虚过期也报 Unlocked，避免「看着被占其实早该回收」）。
    pub fn state_for(&self, worker: &str) -> LockState {
        if self.holder.is_none() || self.expired() {
            return LockState::Unlocked;
        }
        if self.holds(worker) {
            LockState::Locked
        } else {
            LockState::LockedByOther
        }
    }

    /// 某工号看到的锁视图。
    pub fn view_for(&self, worker: &str) -> LockView {
        LockView {
            state: self.state_for(worker),
            seq: self.seq,
            depth: if self.holds(worker) && !self.expired() { self.depth } else { 0 },
            remaining: if self.expired() { 0 } else { self.remaining_ticks() },
        }
    }

    /// 记一次告警（按工号去重；账满则计溢出）。
    fn record_alarm(&mut self, worker: &str, cause: DenyCause) {
        let mut i = 0usize;
        while i < self.alarms.len() {
            if self.alarms[i].worker == worker {
                self.alarms[i].count = self.alarms[i].count.saturating_add(1);
                self.alarms[i].last = cause;
                return;
            }
            i += 1;
        }
        if self.alarms.len() >= MAX_ALARMS {
            self.alarm_overflow = self.alarm_overflow.saturating_add(1);
            return;
        }
        self.alarms.push(Alarm { worker: worker.to_string(), count: 1, last: cause });
    }

    /// 记一次拒绝并产出故障（拒绝必须留痕：静默拒绝等于没有告警）。
    fn deny(&mut self, cause: DenyCause, worker: &str) -> LockFault {
        self.denials = self.denials.saturating_add(1);
        self.record_alarm(worker, cause);
        LockFault::denied(cause)
    }

    /// 申请写锁（新租约或重入）。
    ///
    /// 判定顺序「空工号 → 回收过期 → 空闲授予 → 同号重入 → 他人拒绝」：
    /// 空工号必须最先拦，否则它会以 `""` 的身份拿到锁，之后谁也无法认领；
    /// 回收放在授予之前，是为了让「过期后才来的人」走同一条新授予路径，
    /// 而不是各自复制一段「顺手清一下」的代码。
    pub fn acquire(&mut self, worker: &str) -> Result<LockView, LockFault> {
        if worker.is_empty() {
            return Err(self.deny(DenyCause::HolderUnknown, worker));
        }
        self.settle();
        if self.holder.is_none() {
            self.holder = Some(worker.to_string());
            self.seq = self.seq.saturating_add(1);
            self.acquired_at = self.clock;
            self.depth = 1;
            self.grants = self.grants.saturating_add(1);
            return Ok(self.view_for(worker));
        }
        if !self.holds(worker) {
            return Err(self.deny(DenyCause::NotHolder, worker));
        }
        if self.depth >= REENTRY_CAP {
            return Err(self.deny(DenyCause::ReentryOverflow, worker));
        }
        self.depth = self.depth.saturating_add(1);
        self.grants = self.grants.saturating_add(1);
        Ok(self.view_for(worker))
    }

    /// 续租（只在尾段窗口开放）。
    ///
    /// 顺序「无锁 → 非持有者 → 已过期 → 未进窗口」：身份先于时效。若先判时效，
    /// 旁观者在租约过期瞬间来续租会被报成「过期」，看上去像是他有锁而没续上。
    pub fn renew(&mut self, worker: &str) -> Result<LockView, LockFault> {
        if self.holder.is_none() {
            return Err(self.deny(DenyCause::NoLease, worker));
        }
        if !self.holds(worker) {
            return Err(self.deny(DenyCause::NotHolder, worker));
        }
        if self.expired() {
            return Err(self.deny(DenyCause::LeaseExpired, worker));
        }
        if self.remaining_ticks() > RENEW_WINDOW {
            return Err(self.deny(DenyCause::RenewTooEarly, worker));
        }
        self.acquired_at = self.clock;
        self.renewals = self.renewals.saturating_add(1);
        Ok(self.view_for(worker))
    }

    /// 释放（重入必须配对释放，深度归零才真正解锁）。
    ///
    /// 「释放」不校验过期之前先校验身份，因为身份错属于入侵（该报警），过期属于自己
    /// 疏忽（该提示重申购）。两类事故的处置动作不同，合成一个码会让值班人无从下手。
    pub fn release(&mut self, worker: &str) -> Result<LockView, LockFault> {
        if self.holder.is_none() {
            return Err(self.deny(DenyCause::NoLease, worker));
        }
        if !self.holds(worker) {
            return Err(self.deny(DenyCause::NotHolder, worker));
        }
        if self.expired() {
            self.settle();
            return Err(self.deny(DenyCause::LeaseExpired, worker));
        }
        if self.depth > 1 {
            self.depth -= 1;
            return Ok(self.view_for(worker));
        }
        self.holder = None;
        self.depth = 0;
        Ok(self.view_for(worker))
    }

    /// 当前持有者（无人持有为 None）。
    pub fn holder(&self) -> Option<&str> {
        self.holder.as_deref()
    }

    /// 当前租约序号。
    pub const fn seq(&self) -> u64 {
        self.seq
    }

    /// 当前重入深度。
    pub const fn depth(&self) -> u16 {
        self.depth
    }

    /// 授予次数（含重入）。
    pub const fn grants(&self) -> u16 {
        self.grants
    }

    /// 被拒次数。
    pub const fn denials(&self) -> u16 {
        self.denials
    }

    /// 过期回收次数。
    pub const fn expiries(&self) -> u16 {
        self.expiries
    }

    /// 续租次数。
    pub const fn renewals(&self) -> u16 {
        self.renewals
    }

    /// 某工号的告警次数（不在账上即为 0）。
    pub fn alarm_count(&self, worker: &str) -> u16 {
        let mut i = 0usize;
        while i < self.alarms.len() {
            if self.alarms[i].worker == worker {
                return self.alarms[i].count;
            }
            i += 1;
        }
        0
    }

    /// 告警账只读视图。
    pub fn alarms(&self) -> &[Alarm] {
        &self.alarms
    }

    /// 告警账溢出次数（账满后未能落账的条数）。
    pub const fn alarm_overflow(&self) -> u16 {
        self.alarm_overflow
    }

    /// 锁状态导出（字段序固定，便于逐行 diff；告警账逐行附后）。
    pub fn export(&self) -> String {
        let mut out = String::with_capacity(256);
        out.push_str("lock ttl=");
        out.push_str(&self.ttl().to_string());
        out.push_str(" now=");
        out.push_str(&self.now().to_string());
        out.push_str(" seq=");
        out.push_str(&self.seq.to_string());
        out.push_str(" depth=");
        out.push_str(&self.depth.to_string());
        out.push_str(" remaining=");
        out.push_str(&self.remaining_ticks().to_string());
        out.push_str(" grants=");
        out.push_str(&self.grants.to_string());
        out.push_str(" denials=");
        out.push_str(&self.denials.to_string());
        out.push_str(" expiries=");
        out.push_str(&self.expiries.to_string());
        out.push_str(" renewals=");
        out.push_str(&self.renewals.to_string());
        out.push_str(" holder=");
        out.push_str(match self.holder.as_deref() {
            Some(h) => h,
            None => "-",
        });
        out.push_str(" state=");
        out.push_str(if self.holder.is_none() || self.expired() {
            LockState::Unlocked.label()
        } else {
            LockState::Locked.label()
        });
        out.push_str(" alarms=");
        out.push_str(&self.alarms.len().to_string());
        out.push_str(" overflow=");
        out.push_str(&self.alarm_overflow.to_string());
        out.push('\n');
        let mut i = 0usize;
        while i < self.alarms.len() {
            out.push_str("alarm ");
            out.push_str(&self.alarms[i].worker);
            out.push_str(" count=");
            out.push_str(&self.alarms[i].count.to_string());
            out.push_str(" last=");
            out.push_str(self.alarms[i].last.label());
            out.push('\n');
            i += 1;
        }
        out
    }
}

/// 标准夹具：甲工号持锁，深度 1，尚未推进 tick。
///
/// 返回 `Result` 而不是直出：夹具必须自证可用，否则整册判据会被一个拿不到锁的夹具
/// 挡在门外，看着全绿其实一条都没跑过。
pub fn standard_lock() -> Result<DomainLock, LockFault> {
    let mut lock = DomainLock::new();
    lock.acquire("WX-01")?;
    Ok(lock)
}

/// 判据入口（UNX-F21598-J1 及其配套十四条）。
///
/// 判据常量独立写死（12 / 4 / 3 / 8 / 0x91090），不复用被测常量名：若断言写成
/// `remaining == LEASE_TTL`，把常量改成 13 时判据会跟着变，等于自证。
pub fn run_unx21598_domainlock_checks() -> crate::checks::CheckSet {
    let mut cs = crate::checks::CheckSet::new("unx-f21598");

    // 1) 夹具可构造：空锁 + 甲工号拿到锁。
    let standard = standard_lock();
    cs.add("f21598.fixture_buildable", standard.is_ok(), "标准写锁构造失败");

    // 2) 三态：空锁对任何人都是 Unlocked；甲持锁后甲=Locked、乙=LockedByOther。
    let mut ok = false;
    {
        let mut lock = DomainLock::new();
        let cold = lock.state_for("WX-01") == LockState::Unlocked
            && lock.state_for("WX-02") == LockState::Unlocked
            && !lock.state_for("WX-01").is_taken();
        if lock.acquire("WX-01").is_ok() {
            let mine = lock.view_for("WX-01");
            let other = lock.view_for("WX-02");
            ok = cold
                && mine.state == LockState::Locked
                && mine.is_held()
                && mine.depth == 1
                && mine.remaining == 12
                && other.state == LockState::LockedByOther
                && !other.is_held()
                && lock.holder() == Some("WX-01");
        }
    }
    cs.add("f21598.three_states", ok, "三态判定不符（空/自持/他持）");

    // 3) 反向：非持有者写入被拒 + 告警入账（本单的核心判据）。
    let mut ok = false;
    if let Ok(mut lock) = standard_lock() {
        let denied = lock.acquire("WX-02");
        ok = denied.as_ref().err().map(|f| f.code) == Some(0x91090)
            && denied.as_ref().err().map(|f| f.cause) == Some(DenyCause::NotHolder)
            && denied.as_ref().err().map(|f| f.is_denied()) == Some(true)
            && lock.denials() == 1
            && lock.alarm_count("WX-02") == 1
            && lock.alarms().len() == 1
            && lock.alarms()[0].last == DenyCause::NotHolder
            && lock.alarms()[0].last.is_intrusion()
            && lock.holder() == Some("WX-01")
            && lock.grants() == 1;
    }
    cs.add(
        "f21598.non_holder_denied",
        ok,
        "非持有者未被拒、码不为 0x91090 或告警未入账",
    );

    // 4) 边界：重入上限 3（第 3 次成功、第 4 次拒绝且深度不涨）。
    let mut ok = false;
    if let Ok(mut lock) = standard_lock() {
        let second = lock.acquire("WX-01");
        let third = lock.acquire("WX-01");
        let fourth = lock.acquire("WX-01");
        ok = second.is_ok()
            && third.is_ok()
            && lock.depth() == 3
            && fourth.err().map(|f| f.cause) == Some(DenyCause::ReentryOverflow)
            && lock.depth() == 3
            && lock.denials() == 1;
    }
    cs.add("f21598.reentry_cap", ok, "重入第 4 次未被拒或深度不符");

    // 5) 重入必须配对释放：深度 3 → 2 → 1 仍持有，第 3 次才解锁。
    let mut ok = false;
    if let Ok(mut lock) = standard_lock() {
        let _ = lock.acquire("WX-01");
        let _ = lock.acquire("WX-01");
        let a = lock.release("WX-01");
        let b = lock.release("WX-01");
        let held_between = lock.state_for("WX-01") == LockState::Locked && lock.depth() == 1;
        let c = lock.release("WX-01");
        ok = a.is_ok()
            && b.is_ok()
            && held_between
            && c.is_ok()
            && lock.depth() == 0
            && lock.state_for("WX-01") == LockState::Unlocked
            && lock.holder().is_none();
    }
    cs.add("f21598.release_pairs", ok, "重入释放未配对计数或提前解锁");

    // 6) 反向：无锁释放（自误改的第一种形态：凭空释放）。
    let ok = {
        let mut lock = DomainLock::new();
        let r = lock.release("WX-01");
        r.err().map(|f| f.cause) == Some(DenyCause::NoLease) && lock.denials() == 1
    };
    cs.add("f21598.release_without_hold", ok, "空锁释放未被拒或码不符");

    // 7) 反向：旁观者代释放（自误改的第二种形态：认错锁）。
    let mut ok = false;
    if let Ok(mut lock) = standard_lock() {
        let r = lock.release("WX-02");
        ok = r.err().map(|f| f.cause) == Some(DenyCause::NotHolder)
            && lock.holder() == Some("WX-01")
            && lock.depth() == 1;
    }
    cs.add("f21598.release_by_bystander", ok, "旁观者释放未被拒或持锁者被顶掉");

    // 8) 边界：剩余恰 1 tick 仍算持有，他人仍抢不到。
    let mut ok = false;
    if let Ok(mut lock) = standard_lock() {
        lock.tick(11);
        let view = lock.view_for("WX-01");
        ok = view.remaining == 1
            && view.state == LockState::Locked
            && lock.state_for("WX-02") == LockState::LockedByOther
            && lock.acquire("WX-02").err().map(|f| f.cause) == Some(DenyCause::NotHolder);
    }
    cs.add("f21598.remaining_one_tick", ok, "剩余 1 tick 时提前失锁或被人抢占");

    // 9) 边界：整 12 tick 到期 → 自动回收、序号 +1、过期计数 +1。
    let mut ok = false;
    if let Ok(mut lock) = standard_lock() {
        lock.tick(12);
        let dead = lock.state_for("WX-01") == LockState::Unlocked && lock.view_for("WX-01").remaining == 0;
        let taken = lock.acquire("WX-02");
        ok = dead
            && taken.is_ok()
            && lock.expiries() == 1
            && lock.seq() == 2
            && lock.holder() == Some("WX-02")
            && lock.depth() == 1;
    }
    cs.add("f21598.expiry_reclaims", ok, "到期未回收或新租约序号不符");

    // 10) 反向：过期后旧持有者释放（防自误改：他以为自己还握着）。
    let mut ok = false;
    if let Ok(mut lock) = standard_lock() {
        lock.tick(12);
        let r = lock.release("WX-01");
        ok = r.err().map(|f| f.cause) == Some(DenyCause::LeaseExpired)
            && lock.holder().is_none()
            && lock.expiries() == 1
            && lock.alarm_count("WX-01") == 1;
    }
    cs.add("f21598.expired_release", ok, "过期释放未被拒或锁未被回收");

    // 11) 反向：未进续租窗口就续租（防止无限延长占用）。
    let mut ok = false;
    if let Ok(mut lock) = standard_lock() {
        lock.tick(1);
        let r = lock.renew("WX-01");
        ok = r.err().map(|f| f.cause) == Some(DenyCause::RenewTooEarly)
            && lock.renewals() == 0
            && lock.view_for("WX-01").remaining == 11;
    }
    cs.add("f21598.renew_too_early", ok, "早期续租未被拒或剩余 tick 被改动");

    // 12) 正向：进窗口后续租成功，剩余回满 12。
    let mut ok = false;
    if let Ok(mut lock) = standard_lock() {
        lock.tick(8);
        let before = lock.view_for("WX-01").remaining == 4;
        let r = lock.renew("WX-01");
        ok = before
            && r.is_ok()
            && lock.renewals() == 1
            && lock.view_for("WX-01").remaining == 12
            && lock.depth() == 1;
    }
    cs.add("f21598.renew_in_window", ok, "窗口内续租未生效或剩余 tick 未回满");

    // 13) 反向：过期后续租。
    let mut ok = false;
    if let Ok(mut lock) = standard_lock() {
        lock.tick(12);
        let r = lock.renew("WX-01");
        ok = r.err().map(|f| f.cause) == Some(DenyCause::LeaseExpired) && lock.renewals() == 0;
    }
    cs.add("f21598.renew_after_expiry", ok, "过期续租未被拒或码不符");

    // 14) 反向：空工号（无法认定持有者，必须最早拦下）。
    let ok = {
        let mut lock = DomainLock::new();
        let r = lock.acquire("");
        r.err().map(|f| f.cause) == Some(DenyCause::HolderUnknown)
            && lock.holder().is_none()
            && lock.seq() == 0
            && lock.state_for("") == LockState::Unlocked
            && r.as_ref().err().map(|f| f.text().contains("写锁拒绝")).unwrap_or(false)
    };
    cs.add("f21598.empty_worker", ok, "空工号未被拒或占用了锁");

    // 15) 幂等：同一工号连拒 3 次只留一条告警、计数为 3，导出文本可重放。
    let mut ok = false;
    if let Ok(mut lock) = standard_lock() {
        let _ = lock.acquire("WX-02");
        let _ = lock.acquire("WX-02");
        let _ = lock.acquire("WX-02");
        let text_a = lock.export();
        let text_b = lock.export();
        ok = lock.denials() == 3
            && lock.alarms().len() == 1
            && lock.alarm_count("WX-02") == 3
            && lock.alarms()[0].is_repeat()
            && text_a == text_b
            && text_a.starts_with("lock ttl=12 now=0 seq=1 depth=1 remaining=12 grants=1 denials=3")
            && text_a.contains("alarm WX-02 count=3 last=not_holder");
    }
    cs.add(
        "f21598.alarm_ledger",
        ok,
        "重复拒绝未去重累计，或导出文本不可重放/字段不符",
    );

    // 16) 边界：告警账容量 8，第 9 个工号溢出但不撑破账本。
    let mut ok = false;
    if let Ok(mut lock) = standard_lock() {
        let names = ["WX-02", "WX-03", "WX-04", "WX-05", "WX-06", "WX-07", "WX-08", "WX-09", "WX-10"];
        let mut i = 0usize;
        while i < names.len() {
            let _ = lock.acquire(names[i]);
            i += 1;
        }
        ok = lock.alarms().len() == 8
            && lock.alarm_overflow() == 1
            && lock.alarm_count("WX-09") == 1
            && lock.alarm_count("WX-10") == 0
            && lock.denials() == 9;
    }
    cs.add("f21598.alarm_overflow", ok, "告警账超容后计数不符（应留 8 条 + 1 溢出）");

    cs
}
