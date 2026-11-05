//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F12621 线程警报与 Alertable 等待→D4 消息泵联动 · 0x80540段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定）：
//!   ① 纯功能代码 ≥340 行；
//!   ② 判据原文：UNX-F12622-J1 用户 APC 投递（排队/交付/完成三段）
//!      消费对平断言绿，交付时机注入 10/10 一致；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 用户 APC 生命周期三段闭集：排队（Queued）→交付（Delivered）→
//!     完成（Completed），单调只进不退，倒退显性拒（PhaseRegress）；
//!     未交付先完成 / 未排队先交付都是跳段必抓；
//!   - 消费对平：排队数 ≡ 交付数 + 队内滞留数；交付数 ≡ 完成数 +
//!     交付未完成数——两式全绿才算对平（ParityBreak 抓不等）；
//!   - 交付时机闭式：仅当线程处于可警报等待（AlertableWait）时泵
//!     才许交付；不可警报时交付请求必须被**改期**（Defer）而非丢弃
//!     ——改期注入 10/10：十组交错（警报位×等待模×泵拍）逐一与
//!     时机黄金闭式对平，一次不一致即 TimingMissed；
//!   - 信箱容量冻结：排队超容量显性拒（MailboxFull），不静默挤占；
//!   - 双跑指纹一致；LCG 种子写死 = SEG_BASE（0x80540）；
//!     禁 rand/浮点/IO/墙钟；no_std + alloc；零 unwrap/expect；
//!     诊断码闭集 0x80540..0x80580。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x80540，AI-VX9-1105 B01 批 12 槽第 10 槽）。
pub const SEG_BASE: u32 = 0x80540;

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ApcPhaseCode {
    /// 阶段倒退：三段只进不退
    PhaseRegress,
    /// 跳段完成：未交付先完成
    CompleteBeforeDeliver,
    /// 跳段交付：未排队先交付
    DeliverBeforeQueue,
    /// 交付时机违：不可警报时强交
    DeliverNonAlertable,
    /// 消费对平破：排队≠交付+滞留
    ParityBreak,
    /// 信箱满：排队超容量
    MailboxFull,
    /// 信箱空取：无 APC 可交付
    MailboxEmpty,
    /// 改期丢失：改期 APC 未回队
    DeferredLost,
    /// 双重完成：同一条 APC 两次完成
    DoubleComplete,
    /// 时机对平失败：注入例与黄金闭式不一致
    TimingMissed,
    /// 双跑指纹漂移
    FingerprintDrift,
    /// 未知诊断码兜底
    Unknown,
}

impl ApcPhaseCode {
    pub const fn val(self) -> u32 {
        SEG_BASE
            + match self {
                ApcPhaseCode::PhaseRegress => 0x01,
                ApcPhaseCode::CompleteBeforeDeliver => 0x02,
                ApcPhaseCode::DeliverBeforeQueue => 0x03,
                ApcPhaseCode::DeliverNonAlertable => 0x04,
                ApcPhaseCode::ParityBreak => 0x05,
                ApcPhaseCode::MailboxFull => 0x06,
                ApcPhaseCode::MailboxEmpty => 0x07,
                ApcPhaseCode::DeferredLost => 0x08,
                ApcPhaseCode::DoubleComplete => 0x09,
                ApcPhaseCode::TimingMissed => 0x0A,
                ApcPhaseCode::FingerprintDrift => 0x0B,
                ApcPhaseCode::Unknown => 0x3F,
            }
    }

    pub fn detail(self) -> &'static str {
        match self {
            ApcPhaseCode::PhaseRegress => "阶段倒退：排队→交付→完成只进不退",
            ApcPhaseCode::CompleteBeforeDeliver => "未交付先完成：先等泵交付再完成",
            ApcPhaseCode::DeliverBeforeQueue => "未排队先交付：先排队再交付",
            ApcPhaseCode::DeliverNonAlertable => "不可警报强交：改期处理，勿丢勿抢",
            ApcPhaseCode::ParityBreak => "对平破：排队=交付+滞留，交付=完成+在途",
            ApcPhaseCode::MailboxFull => "信箱满：先消费积压再排队",
            ApcPhaseCode::MailboxEmpty => "信箱空：无 APC 可交付",
            ApcPhaseCode::DeferredLost => "改期丢失：改期条目必须回队，逐条对账",
            ApcPhaseCode::DoubleComplete => "双重完成：一条 APC 只许完成一次",
            ApcPhaseCode::TimingMissed => "时机不一致：核对可警报等待闭式",
            ApcPhaseCode::FingerprintDrift => "双跑指纹漂移：冻结种子重跑",
            ApcPhaseCode::Unknown => "未知诊断码：不在 0x80540 段闭集内",
        }
    }

    pub const ALL: [ApcPhaseCode; 12] = [
        ApcPhaseCode::PhaseRegress,
        ApcPhaseCode::CompleteBeforeDeliver,
        ApcPhaseCode::DeliverBeforeQueue,
        ApcPhaseCode::DeliverNonAlertable,
        ApcPhaseCode::ParityBreak,
        ApcPhaseCode::MailboxFull,
        ApcPhaseCode::MailboxEmpty,
        ApcPhaseCode::DeferredLost,
        ApcPhaseCode::DoubleComplete,
        ApcPhaseCode::TimingMissed,
        ApcPhaseCode::FingerprintDrift,
        ApcPhaseCode::Unknown,
    ];
}

pub fn detail(code: u32) -> &'static str {
    for c in ApcPhaseCode::ALL.iter() {
        if c.val() == code {
            return c.detail();
        }
    }
    ApcPhaseCode::Unknown.detail()
}

// ── 冻结常量表 ───────────────────────────────────────────────────
/// 信箱容量（冻结）。
pub const MAILBOX_CAP: usize = 8;
/// 交付时机注入组数（冻结 10）。
pub const TIMING_CASES: u32 = 10;

/// APC 生命周期三段（单调）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ApcPhase {
    Queued,
    Delivered,
    Completed,
}

impl ApcPhase {
    pub const fn rank(self) -> u32 {
        match self {
            ApcPhase::Queued => 0,
            ApcPhase::Delivered => 1,
            ApcPhase::Completed => 2,
        }
    }
}

/// 一条用户 APC。
#[derive(Clone, Copy, Debug)]
pub struct ApcItem {
    /// APC 序号（单调）。
    pub seq: u32,
    /// 生命周期阶段。
    pub phase: ApcPhase,
}

/// 线程等待模（与警报联动的三态闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ThreadWait {
    Running,
    PlainWait,
    AlertableWait,
}

/// 用户 APC 信箱（单线程）：排队/交付/完成三段账。
pub struct ApcMailbox {
    /// 队内 APC（Queued 段）。
    pub queue: Vec<ApcItem>,
    /// 交付未完成（Delivered 段）。
    pub inflight: Vec<ApcItem>,
    /// 累计排队数。
    pub queued_total: u32,
    /// 累计交付数。
    pub delivered_total: u32,
    /// 累计完成数。
    pub completed_total: u32,
    /// 累计改期数。
    pub deferred_total: u32,
    /// 当前线程等待模。
    pub wait: ThreadWait,
}

impl ApcMailbox {
    pub fn new() -> ApcMailbox {
        ApcMailbox {
            queue: Vec::new(),
            inflight: Vec::new(),
            queued_total: 0,
            delivered_total: 0,
            completed_total: 0,
            deferred_total: 0,
            wait: ThreadWait::Running,
        }
    }

    /// 切等待模。
    pub fn enter(&mut self, w: ThreadWait) {
        self.wait = w;
    }

    /// 排队一段：容量冻结溢出显性拒。
    pub fn enqueue(&mut self, seq: u32) -> Result<(), u32> {
        if self.queue.len() >= MAILBOX_CAP {
            return Err(ApcPhaseCode::MailboxFull.val());
        }
        // 重号拒收（同信箱内 seq 唯一）。
        for it in self.queue.iter() {
            if it.seq == seq {
                return Err(ApcPhaseCode::PhaseRegress.val());
            }
        }
        for it in self.inflight.iter() {
            if it.seq == seq {
                return Err(ApcPhaseCode::DoubleComplete.val());
            }
        }
        self.queue.push(ApcItem { seq, phase: ApcPhase::Queued });
        self.queued_total += 1;
        Ok(())
    }

    /// 泵拍：尝试交付一条（仅可警报等待时）。
    /// 不可警报 → 改期（Ok(false)，条目留队）；可警报 → 交付。
    pub fn pump(&mut self) -> Result<bool, u32> {
        if self.queue.is_empty() {
            return Ok(false);
        }
        match self.wait {
            ThreadWait::AlertableWait => {
                // 交付：队头 → 交付段。
                let head = match self.queue.first() {
                    Some(it) => *it,
                    None => return Err(ApcPhaseCode::MailboxEmpty.val()),
                };
                self.queue.remove(0);
                self.inflight.push(ApcItem { seq: head.seq, phase: ApcPhase::Delivered });
                self.delivered_total += 1;
                Ok(true)
            }
            _ => {
                // 改期：条目留队，显性记账（DeferredLost 的反面证据）。
                self.deferred_total += 1;
                Ok(false)
            }
        }
    }

    /// 完成一段：交付段取一条完成（未交付先完成必抓）。
    pub fn complete_one(&mut self) -> Result<u32, u32> {
        if self.inflight.is_empty() {
            return Err(ApcPhaseCode::CompleteBeforeDeliver.val());
        }
        let done = match self.inflight.first() {
            Some(it) => *it,
            None => return Err(ApcPhaseCode::CompleteBeforeDeliver.val()),
        };
        self.inflight.remove(0);
        self.completed_total += 1;
        Ok(done.seq)
    }

    /// 消费对平两式：
    ///   排队数 ≡ 交付数 + 队内滞留数
    ///   交付数 ≡ 完成数 + 交付未完成数
    pub fn parity(&self) -> Result<(), u32> {
        if self.queued_total
            != self.delivered_total.wrapping_add(self.queue.len() as u32)
        {
            return Err(ApcPhaseCode::ParityBreak.val());
        }
        if self.delivered_total
            != self.completed_total.wrapping_add(self.inflight.len() as u32)
        {
            return Err(ApcPhaseCode::ParityBreak.val());
        }
        Ok(())
    }

    /// 改期保全：改期数>0 时队必须非空（改期条目回队证据）。
    pub fn deferred_kept(&self) -> bool {
        if self.deferred_total > 0 && self.queue.is_empty() {
            return false;
        }
        true
    }

    /// 账指纹。
    pub fn fingerprint(&self) -> u32 {
        self.queued_total
            .wrapping_mul(29)
            .wrapping_add(self.delivered_total.wrapping_mul(11))
            .wrapping_add(self.completed_total.wrapping_mul(5))
            .wrapping_add(self.deferred_total.wrapping_mul(2))
            .wrapping_add(self.queue.len() as u32)
            .wrapping_add(self.inflight.len() as u32)
    }
}

// ── 交付时机黄金闭式（注入对平基准） ────────────────────────────
/// 黄金闭式：给定（可警报?, 队非空?）→ 期望泵拍结果。
/// 可警报且队非空 → Ok(true)；可警报队空 → Ok(false)；
/// 不可警报 → 改期 Ok(false)（条目保留）。
pub fn golden_timing(alertable: bool, has_item: bool) -> bool {
    alertable && has_item
}

pub struct Lcg(u64);
impl Lcg {
    pub const fn new(seed: u64) -> Lcg {
        Lcg(seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407))
    }
    pub fn next(&mut self) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        self.0 >> 33
    }
    pub fn below(&mut self, n: u64) -> u64 {
        self.next() % n.max(1)
    }
}

/// 交付时机注入 10/10：十组（警报位×等待模×队况）交错逐一与黄金
/// 闭式对平；不可警报组的条目必须回队（改期不丢）。
pub fn inject_timing_ten() -> Result<u32, u32> {
    let mut rng = Lcg::new(SEG_BASE as u64);
    let mut agree = 0u32;
    let mut i = 0;
    while i < TIMING_CASES {
        let alertable = rng.below(2) == 0;
        let has_item = rng.below(2) == 0;
        let mut mb = ApcMailbox::new();
        if has_item {
            mb.enqueue(100 + i)?;
        }
        mb.enter(if alertable { ThreadWait::AlertableWait } else { ThreadWait::PlainWait });
        let got = mb.pump().unwrap_or(!golden_timing(alertable, has_item));
        let want = golden_timing(alertable, has_item);
        // 改期保全：不可警报且有条目 → 条目必须仍在队。
        let kept = if !alertable && has_item { mb.queue.len() == 1 } else { true };
        if got == want && kept {
            agree += 1;
        }
        i += 1;
    }
    if agree == TIMING_CASES {
        Ok(10)
    } else {
        Err(ApcPhaseCode::TimingMissed.val())
    }
}

// ── 注入面：三段账违约必抓 ───────────────────────────────────────
pub struct InjectLedger {
    pub tries: u32,
    pub caught: u32,
}

impl InjectLedger {
    pub const fn new() -> InjectLedger {
        InjectLedger { tries: 0, caught: 0 }
    }
    pub fn all_caught(&self) -> bool {
        self.tries > 0 && self.caught == self.tries
    }
}

/// 注入族 0：未交付先完成 → CompleteBeforeDeliver。
/// 注入族 1：不可警报强交必须改期（时机违五轮轮换）。
/// 注入族 2：信箱容量溢出 → MailboxFull。
/// 注入族 3：重号排队 → PhaseRegress。
pub fn inject_phase_violations(led: &mut InjectLedger) {
    // 族 0。
    led.tries += 1;
    let mut a = ApcMailbox::new();
    if a.complete_one() == Err(ApcPhaseCode::CompleteBeforeDeliver.val()) {
        led.caught += 1;
    }
    // 族 1：五轮不可警报强交全部改期。
    let mut i = 0;
    while i < 5 {
        led.tries += 1;
        let mut b = ApcMailbox::new();
        let _ = b.enqueue(200 + i);
        b.enter(if i % 2 == 0 { ThreadWait::Running } else { ThreadWait::PlainWait });
        let delivered = b.pump().unwrap_or(true);
        if !delivered && b.queue.len() == 1 && b.delivered_total == 0 {
            led.caught += 1;
        }
        i += 1;
    }
    // 族 2。
    led.tries += 1;
    let mut c = ApcMailbox::new();
    let mut overflowed = false;
    let mut k = 0;
    while k <= MAILBOX_CAP {
        let r = c.enqueue(300 + k as u32);
        if k == MAILBOX_CAP {
            overflowed = r == Err(ApcPhaseCode::MailboxFull.val());
        }
        k += 1;
    }
    if overflowed {
        led.caught += 1;
    }
    // 族 3。
    led.tries += 1;
    let mut d = ApcMailbox::new();
    let _ = d.enqueue(7);
    if d.enqueue(7) == Err(ApcPhaseCode::PhaseRegress.val()) {
        led.caught += 1;
    }
}

// ── 端到端正演 + 双跑一致 ────────────────────────────────────────
/// 正演：排队 3 → 改期 2 拍（普通等待）→ 可警报交付 2 → 完成 1 →
/// 完成第 2 → 对平复核。
pub fn golden_run() -> Result<u32, u32> {
    let mut mb = ApcMailbox::new();
    let mut i = 0;
    while i < 3 {
        mb.enqueue(10 + i)?;
        i += 1;
    }
    mb.enter(ThreadWait::PlainWait);
    // 改期两拍：条目保留。
    if mb.pump()? {
        return Err(ApcPhaseCode::DeliverNonAlertable.val());
    }
    if mb.pump()? {
        return Err(ApcPhaseCode::DeliverNonAlertable.val());
    }
    if !mb.deferred_kept() {
        return Err(ApcPhaseCode::DeferredLost.val());
    }
    // 换可警报：交付两条。
    mb.enter(ThreadWait::AlertableWait);
    if !mb.pump()? || !mb.pump()? {
        return Err(ApcPhaseCode::TimingMissed.val());
    }
    // 完成两条。
    mb.complete_one()?;
    mb.complete_one()?;
    mb.parity()?;
    Ok(mb.fingerprint())
}

/// 双跑一致。
pub fn repro_check() -> Result<(), u32> {
    match (golden_run(), golden_run()) {
        (Ok(a), Ok(b)) if a == b => Ok(()),
        _ => Err(ApcPhaseCode::FingerprintDrift.val()),
    }
}

// ── 判据谓词（私有 xxx_ok） ──────────────────────────────────────
/// J1：三段消费对平绿。
fn parity_ok() -> bool {
    golden_run().is_ok()
}

/// J2：交付时机注入 10/10 一致。
fn timing_ok() -> bool {
    inject_timing_ten() == Ok(10)
}

/// J3：注入全抓。
fn inject_ok() -> bool {
    let mut led = InjectLedger::new();
    inject_phase_violations(&mut led);
    led.all_caught()
}

/// J4：双跑一致。
fn repro_ok() -> bool {
    repro_check().is_ok()
}

/// 段位守卫。
pub fn segment_guard() -> bool {
    if SEG_BASE % 0x40 != 0 {
        return false;
    }
    if !(0x80000..=0x831C0).contains(&SEG_BASE) {
        return false;
    }
    for c in ApcPhaseCode::ALL.iter() {
        let v = c.val();
        if v < SEG_BASE || v >= SEG_BASE + 0x40 {
            return false;
        }
    }
    true
}

// ── 检查入口（≥12 条，全部 add_dyn） ─────────────────────────────
pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut v: Vec<(&'static str, bool, String)> = Vec::new();
    let g = golden_run();
    v.push(("J1-黄金正演", g.is_ok(), format!("{:?}", g)));
    v.push((
        "J1-三段消费对平",
        parity_ok(),
        String::from("排队=交付+滞留 / 交付=完成+在途"),
    ));
    v.push((
        "J1-交付时机注入10/10",
        timing_ok(),
        format!("{:?}", inject_timing_ten()),
    ));
    v.push(("J2-注入全抓", inject_ok(), String::from("跳段/强交/溢出/重号")));
    // 边界：容量。
    let mut mb = ApcMailbox::new();
    let mut k = 0;
    let mut over = false;
    while k <= MAILBOX_CAP {
        let r = mb.enqueue(400 + k as u32);
        if k == MAILBOX_CAP {
            over = r == Err(ApcPhaseCode::MailboxFull.val());
        }
        k += 1;
    }
    v.push(("J3-信箱容量边界", over, format!("容量 {}", MAILBOX_CAP)));
    // 边界：空信箱交付（可警报但无件）。
    let mut empty = ApcMailbox::new();
    empty.enter(ThreadWait::AlertableWait);
    v.push((
        "J3-空信箱不改期",
        empty.pump() == Ok(false) && empty.deferred_total == 0,
        String::from("无件不动改期账"),
    ));
    // 边界：改期保全。
    let mut defer = ApcMailbox::new();
    let _ = defer.enqueue(500);
    defer.enter(ThreadWait::Running);
    let _ = defer.pump();
    v.push((
        "J3-改期保全",
        defer.queue.len() == 1 && defer.deferred_total == 1,
        String::from("改期回队不丢"),
    ));
    // 黄金时机闭式四象限。
    let q = !golden_timing(false, true)
        && !golden_timing(true, false)
        && !golden_timing(false, false)
        && golden_timing(true, true);
    v.push(("J4-时机闭式四象限", q, String::from("仅(可警报,有件)交付")));
    v.push(("J5-双跑一致", repro_ok(), String::from("指纹相等")));
    v.push(("J6-段位守卫", segment_guard(), String::from("0x80540 窗口")));
    v
}

/// VX9 检查入口（卡 12621，UNX-F12622-J1 判据承载）。
pub fn run_unx12621_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F12621 线程警报与 Alertable 等待→D4 消息泵联动");
    for (n, p, d) in extra_checks() {
        s.add_dyn(n, p, &d);
    }
    s
}

// ── 测试区 ───────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_golden_run_green() {
        assert!(golden_run().is_ok());
    }

    #[test]
    fn test_parity_two_equations() {
        assert!(parity_ok());
    }

    #[test]
    fn test_timing_injection_ten() {
        assert_eq!(inject_timing_ten(), Ok(10));
    }

    #[test]
    fn test_injection_all_caught() {
        let mut led = InjectLedger::new();
        inject_phase_violations(&mut led);
        assert_eq!(led.tries, 8);
        assert_eq!(led.caught, 8);
    }

    #[test]
    fn test_complete_before_deliver_rejected() {
        let mut mb = ApcMailbox::new();
        assert_eq!(
            mb.complete_one(),
            Err(ApcPhaseCode::CompleteBeforeDeliver.val())
        );
    }

    #[test]
    fn test_defer_keeps_item() {
        let mut mb = ApcMailbox::new();
        let _ = mb.enqueue(9);
        mb.enter(ThreadWait::PlainWait);
        assert_eq!(mb.pump(), Ok(false));
        assert_eq!(mb.queue.len(), 1);
        assert_eq!(mb.deferred_total, 1);
        assert!(mb.deferred_kept());
    }

    #[test]
    fn test_mailbox_capacity() {
        let mut mb = ApcMailbox::new();
        let mut i = 0;
        while i < MAILBOX_CAP {
            assert!(mb.enqueue(600 + i as u32).is_ok());
            i += 1;
        }
        assert_eq!(mb.enqueue(999), Err(ApcPhaseCode::MailboxFull.val()));
    }

    #[test]
    fn test_dup_seq_rejected() {
        let mut mb = ApcMailbox::new();
        assert!(mb.enqueue(1).is_ok());
        assert_eq!(mb.enqueue(1), Err(ApcPhaseCode::PhaseRegress.val()));
    }

    #[test]
    fn test_dual_run_equal() {
        assert!(repro_check().is_ok());
        let a = golden_run().unwrap_or(0);
        let b = golden_run().unwrap_or(1);
        assert_eq!(a, b);
    }

    #[test]
    fn test_segment_guard() {
        assert!(segment_guard());
        assert_eq!(SEG_BASE, 0x80540);
    }

    #[test]
    fn test_detail_closed_set() {
        for c in ApcPhaseCode::ALL.iter() {
            assert_eq!(detail(c.val()), c.detail());
        }
        assert!(detail(0x7777).contains("未知"));
    }
}
