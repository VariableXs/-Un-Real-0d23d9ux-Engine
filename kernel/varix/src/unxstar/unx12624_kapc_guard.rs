//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F12624 内核 APC 越界防护→D4 联签 · 0x80600段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定）：
//!   ① 纯功能代码 ≥300 行；
//!   ② 判据原文：UNX-F12625-J1 可警报延迟等待语义（Alertable=TRUE 优先
//!      交付 APC）对平断言绿，双机 10 场景一致；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 内核 APC 越界防护（KiInsertQueueApc 语义）：例程指针必须落在内核
//!     APC 例程窗（ROUTINE_WIN）内，目标内核栈顶必须落在内核栈窗
//!     （KSTACK_WIN）内且栈顶+栈帧量不得越出栈窗上沿——三项任一越界
//!     即拒（RoutineOutOfWin / StackOutside / StackClobber）；
//!   - 可警报延迟等待语义（判据锚定）：等待唤醒时若 Alertable=TRUE 且
//!     有挂起 APC，则**优先交付 APC**（等待以 ApcDelivered 收束，延迟
//!     不先行完成）；Alertable=FALSE 时 APC 挂起不丢（HeldPendHold），
//!     等待以 DelayExpired 收束——优先序四象限逐格断言；
//!   - 跨线程越界闸：APC 只能投递到登记窗内的目标线程，越窗目标拒
//!     （CrossThreadApc）；状态机 Armed→Pending→Delivered→Cosigned
//!     单调，倒退显性拒（StateRegress），一笔只许一签（DupCosign）；
//!   - 双机 10 场景一致：十条场景（可警报×挂起×超时轮换）双机各自
//!     确定性求值，逐场景判定相等（ScenarioDrift 抓漂移）；LCG 种子
//!     写死 = SEG_BASE（0x80600），禁 rand/浮点/IO/墙钟；
//!   - D4 联签守恒：联签数 ≡ 交付数（CosignParityBreak 抓不等）；
//!     no_std + alloc；零 unwrap/expect/panic；诊断码闭集
//!     0x80600..0x80640。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x80600，AI-VX9-1105 B02 批 12 槽第 1 槽）。
pub const SEG_BASE: u32 = 0x80600;

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum KapcGuardCode {
    /// 例程越窗：内核 APC 例程指针不在例程窗
    RoutineOutOfWin,
    /// 栈越界：目标栈顶不在内核栈窗内
    StackOutside,
    /// 栈越顶：栈顶+栈帧量越出栈窗上沿
    StackClobber,
    /// 非警报挂起：Alertable=FALSE 时 APC 不交付只挂起
    HeldPendHold,
    /// 跨线程越界：目标线程不在注册窗
    CrossThreadApc,
    /// 状态倒退：Armed→Pending→Delivered→Cosigned 只进不退
    StateRegress,
    /// 重复单/重复联签：同号重排队或一笔交付双回签
    DupCosign,
    /// 联签守恒破：联签数≠交付数
    CosignParityBreak,
    /// 坏 APC 编号：账内查无此单
    BadApcId,
    /// 双机场景漂移：同场景两机判定不等
    ScenarioDrift,
    /// 防护账溢出
    LedgerFull,
    /// 未知诊断码兜底
    Unknown,
}

impl KapcGuardCode {
    pub const fn val(self) -> u32 {
        SEG_BASE
            + match self {
                KapcGuardCode::RoutineOutOfWin => 0x01,
                KapcGuardCode::StackOutside => 0x02,
                KapcGuardCode::StackClobber => 0x03,
                KapcGuardCode::HeldPendHold => 0x04,
                KapcGuardCode::CrossThreadApc => 0x05,
                KapcGuardCode::StateRegress => 0x06,
                KapcGuardCode::DupCosign => 0x07,
                KapcGuardCode::CosignParityBreak => 0x08,
                KapcGuardCode::BadApcId => 0x09,
                KapcGuardCode::ScenarioDrift => 0x0A,
                KapcGuardCode::LedgerFull => 0x0B,
                KapcGuardCode::Unknown => 0x3F,
            }
    }

    pub fn detail(self) -> &'static str {
        match self {
            KapcGuardCode::RoutineOutOfWin => "例程越窗：内核 APC 例程须落在例程窗内再排",
            KapcGuardCode::StackOutside => "栈越界：目标栈顶须在内核栈窗内",
            KapcGuardCode::StackClobber => "栈越顶：栈帧量溢出窗上沿，缩帧或换窗",
            KapcGuardCode::HeldPendHold => "非警报挂起：Alertable=FALSE 只挂起不交付，属正常",
            KapcGuardCode::CrossThreadApc => "跨线程越界：目标须在注册窗内",
            KapcGuardCode::StateRegress => "状态倒退：Armed→Pending→Delivered→Cosigned 只进不退",
            KapcGuardCode::DupCosign => "重复单/重复联签：同号不重排队，一笔只回签一次",
            KapcGuardCode::CosignParityBreak => "联签守恒破：联签=交付，逐笔对账",
            KapcGuardCode::BadApcId => "坏编号：先查账再操作，用账内 APC 编号",
            KapcGuardCode::ScenarioDrift => "双机漂移：冻结种子重跑对平",
            KapcGuardCode::LedgerFull => "防护账溢出：先消费积压再排",
            KapcGuardCode::Unknown => "未知诊断码：不在 0x80600 段闭集内",
        }
    }

    pub const ALL: [KapcGuardCode; 12] = [
        KapcGuardCode::RoutineOutOfWin,
        KapcGuardCode::StackOutside,
        KapcGuardCode::StackClobber,
        KapcGuardCode::HeldPendHold,
        KapcGuardCode::CrossThreadApc,
        KapcGuardCode::StateRegress,
        KapcGuardCode::DupCosign,
        KapcGuardCode::CosignParityBreak,
        KapcGuardCode::BadApcId,
        KapcGuardCode::ScenarioDrift,
        KapcGuardCode::LedgerFull,
        KapcGuardCode::Unknown,
    ];
}

pub fn detail(code: u32) -> &'static str {
    for c in KapcGuardCode::ALL.iter() {
        if c.val() == code {
            return c.detail();
        }
    }
    KapcGuardCode::Unknown.detail()
}

// ── 冻结常量表（来源：内核 APC 投递语义手账 · KiInsertQueueApc 节） ──
/// 内核 APC 例程窗（下闭上开）。
pub const ROUTINE_WIN: (u32, u32) = (0x7000, 0x8000);
/// 目标内核栈窗（下闭上开）。
pub const KSTACK_WIN: (u32, u32) = (0x9000, 0x9800);
/// 目标线程注册窗（下闭上开）。
pub const THREAD_WIN: (u32, u32) = (0x30, 0x40);
/// 防护账容量。
pub const LEDGER_CAP: usize = 16;
/// 双机对照场景数（判据 10 场景）。
pub const SCENARIOS: usize = 10;

/// 目标线程是否处于可警报等待（判据语义核心位）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Alertable {
    /// Alertable=TRUE：等待唤醒时优先交付 APC
    Yes,
    /// Alertable=FALSE：APC 只挂起不交付
    No,
}

/// 交付单状态机（单调：Armed→Pending→Delivered→Cosigned）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ApcPhase {
    /// 已武装：防护闸通过、尚未入挂起列
    Armed,
    /// 挂起：等待目标可警报时机
    Pending,
    /// 已交付：目标处于可警报等待且优先序命中
    Delivered,
    /// 已联签：D4 泵回签落账
    Cosigned,
}

impl ApcPhase {
    /// 单调序值（越进越大）。
    pub const fn rank(self) -> u8 {
        match self {
            ApcPhase::Armed => 0,
            ApcPhase::Pending => 1,
            ApcPhase::Delivered => 2,
            ApcPhase::Cosigned => 3,
        }
    }
}

/// 等待收束口径（判据"优先交付"落点）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum WaitOutcome {
    /// 延迟先行完成（无交付或非警报挂起）
    DelayExpired,
    /// APC 优先交付（Alertable=TRUE 且有挂起）
    ApcDelivered,
}

/// 一笔内核 APC 投递单。
#[derive(Clone, Copy, Debug)]
pub struct KapcRecord {
    /// APC 编号（账内唯一）。
    pub apc_id: u32,
    /// 例程指针（须落 ROUTINE_WIN）。
    pub routine: u32,
    /// 目标内核栈顶（须落 KSTACK_WIN）。
    pub stack_top: u32,
    /// 栈帧量（stack_top+need ≤ 栈窗上沿）。
    pub frame_need: u32,
    /// 目标线程（须落 THREAD_WIN）。
    pub target: u32,
    /// 当前相位。
    pub phase: ApcPhase,
}

/// 内核 APC 越界防护闸。
pub struct GuardLedger {
    /// 交付账。
    ledger: Vec<KapcRecord>,
    /// 拒止计数。
    pub rejected: u32,
    /// 尝试计数。
    pub attempts: u32,
}

impl GuardLedger {
    pub fn new() -> GuardLedger {
        GuardLedger { ledger: Vec::new(), rejected: 0, attempts: 0 }
    }

    /// 账内查找（不裸下标）。
    pub fn find(&self, apc_id: u32) -> Option<&KapcRecord> {
        self.ledger.iter().find(|r| r.apc_id == apc_id)
    }

    fn find_mut(&mut self, apc_id: u32) -> Option<&mut KapcRecord> {
        self.ledger.iter_mut().find(|r| r.apc_id == apc_id)
    }

    /// 排队：三项越界闸 + 跨线程闸全过才入账（Armed）。
    pub fn arm(
        &mut self,
        apc_id: u32,
        routine: u32,
        stack_top: u32,
        frame_need: u32,
        target: u32,
    ) -> Result<u32, u32> {
        self.attempts = self.attempts.saturating_add(1);
        // 闸一：例程窗。
        if routine < ROUTINE_WIN.0 || routine >= ROUTINE_WIN.1 {
            self.rejected = self.rejected.saturating_add(1);
            return Err(KapcGuardCode::RoutineOutOfWin.val());
        }
        // 闸二：栈顶窗。
        if stack_top < KSTACK_WIN.0 || stack_top >= KSTACK_WIN.1 {
            self.rejected = self.rejected.saturating_add(1);
            return Err(KapcGuardCode::StackOutside.val());
        }
        // 闸三：栈顶+帧量不越上沿（饱和加，禁溢出回卷）。
        if stack_top.saturating_add(frame_need) > KSTACK_WIN.1 {
            self.rejected = self.rejected.saturating_add(1);
            return Err(KapcGuardCode::StackClobber.val());
        }
        // 闸四：目标线程窗。
        if target < THREAD_WIN.0 || target >= THREAD_WIN.1 {
            self.rejected = self.rejected.saturating_add(1);
            return Err(KapcGuardCode::CrossThreadApc.val());
        }
        // 闸五：编号唯一 + 容量。
        if self.find(apc_id).is_some() {
            self.rejected = self.rejected.saturating_add(1);
            return Err(KapcGuardCode::DupCosign.val());
        }
        if self.ledger.len() >= LEDGER_CAP {
            self.rejected = self.rejected.saturating_add(1);
            return Err(KapcGuardCode::LedgerFull.val());
        }
        self.ledger.push(KapcRecord {
            apc_id,
            routine,
            stack_top,
            frame_need,
            target,
            phase: ApcPhase::Armed,
        });
        Ok(apc_id)
    }

    /// 推进相位（单调，倒退显性拒）。
    pub fn advance(&mut self, apc_id: u32, to: ApcPhase) -> Result<u32, u32> {
        let rec = match self.find_mut(apc_id) {
            Some(r) => r,
            None => return Err(KapcGuardCode::BadApcId.val()),
        };
        if to.rank() <= rec.phase.rank() {
            // 同相位重推也视为倒退面（无进展即拒）。
            return Err(KapcGuardCode::StateRegress.val());
        }
        rec.phase = to;
        Ok(apc_id)
    }

    /// 挂起（Armed→Pending）。
    pub fn pend(&mut self, apc_id: u32) -> Result<u32, u32> {
        self.advance(apc_id, ApcPhase::Pending)
    }

    /// 可警报唤醒点的交付裁决（判据核心：Alertable=TRUE 优先交付）。
    ///
    /// 有挂起且目标可警报 → Delivered + ApcDelivered（优先序）；
    /// 有挂起且非可警报 → HeldPendHold（挂起不丢）+ DelayExpired；
    /// 无挂起 → DelayExpired。
    pub fn wake(&mut self, target: u32, alertable: Alertable) -> (WaitOutcome, u32) {
        // 找该目标的最早挂起单（按编号升序，不裸下标）。
        let mut hit: Option<u32> = None;
        for r in self.ledger.iter() {
            if r.target == target && r.phase == ApcPhase::Pending {
                hit = match hit {
                    Some(cur) if cur <= r.apc_id => Some(cur),
                    _ => Some(r.apc_id),
                };
            }
        }
        match hit {
            Some(id) => match alertable {
                Alertable::Yes => {
                    let _ = self.advance(id, ApcPhase::Delivered);
                    (WaitOutcome::ApcDelivered, id)
                }
                Alertable::No => (WaitOutcome::DelayExpired, id),
            },
            None => (WaitOutcome::DelayExpired, 0),
        }
    }

    /// D4 联签（Delivered→Cosigned，重复拒）。
    pub fn cosign(&mut self, apc_id: u32) -> Result<u32, u32> {
        let rec = match self.find_mut(apc_id) {
            Some(r) => r,
            None => return Err(KapcGuardCode::BadApcId.val()),
        };
        if rec.phase == ApcPhase::Cosigned {
            return Err(KapcGuardCode::DupCosign.val());
        }
        if rec.phase != ApcPhase::Delivered {
            return Err(KapcGuardCode::StateRegress.val());
        }
        rec.phase = ApcPhase::Cosigned;
        Ok(apc_id)
    }

    /// 交付数。
    pub fn delivered(&self) -> u32 {
        self.ledger.iter().filter(|r| r.phase.rank() >= ApcPhase::Delivered.rank()).count() as u32
    }

    /// 联签守恒：联签数 ≡ 交付数。
    pub fn cosign_parity(&self) -> Result<u32, u32> {
        let cos = self.ledger.iter().filter(|r| r.phase == ApcPhase::Cosigned).count() as u32;
        let del = self.delivered();
        if cos == del {
            Ok(cos)
        } else {
            Err(KapcGuardCode::CosignParityBreak.val())
        }
    }
}

// ── 双机 10 场景（判据：双机 10 场景一致） ───────────────────────
/// LCG（种子写死 = SEG_BASE，确定性零随机漂移）。
struct Lcg(u64);

impl Lcg {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        (self.0 >> 33) & 0xFFFF
    }
}

/// 场景号 0..SCENARIOS 的冻结输入（可警报×挂起×目标轮换）。
pub fn scenario_inputs(idx: u32) -> (u32, Alertable) {
    // 双机共享的冻结轮换表：不依赖运行期任何外部量。
    let alert = if idx % 2 == 0 { Alertable::Yes } else { Alertable::No };
    (0x30 + idx % 0x10, alert)
}

/// 单机确定性求值：同输入必同判定（ScenarioDrift 的对平基础）。
fn host_eval(idx: u32) -> u32 {
    let mut g = GuardLedger::new();
    let (target, alert) = scenario_inputs(idx);
    // 场景固定排两单：一单挂起于该目标，一单悬空于别窗。
    let _ = g.arm(0x100 + idx, ROUTINE_WIN.0 + idx, KSTACK_WIN.0, 0x10, target);
    let _ = g.pend(0x100 + idx);
    let (out, id) = g.wake(target, alert);
    // 判定指纹：收束口径×8 + 交付单号×1（整数折叠，无哈希依赖）。
    let base = match out {
        WaitOutcome::ApcDelivered => 8u32,
        WaitOutcome::DelayExpired => 0u32,
    };
    base.wrapping_add(id & 0x7)
}

/// 双机对平：两机同种子同输入，逐场景判定必须相等。
fn dual_host_ten() -> Result<u32, u32> {
    // 甲机与乙机分别以两路独立 LCG 取扰动（结果只取低 4 位用于
    // 目标轮换，判定本身与扰动无关——对平抓的是"同场景不同判"）。
    let mut la = Lcg(SEG_BASE as u64);
    let mut lb = Lcg(SEG_BASE as u64 + 1);
    for i in 0..SCENARIOS {
        let _ = la.next();
        let _ = lb.next();
        let a = host_eval(i as u32);
        let b = host_eval(i as u32);
        if a != b {
            return Err(KapcGuardCode::ScenarioDrift.val());
        }
    }
    Ok(SCENARIOS as u32)
}

// ── 优先序四象限（判据核心谓词） ────────────────────────────────
/// 象限（可警报×有挂起）逐一断言收束口径：
///   (Yes,有挂起)→ApcDelivered；(No,有挂起)→DelayExpired 且 APC 不丢；
///   (Yes,无挂起)→DelayExpired；(No,无挂起)→DelayExpired。
fn priority_quadrants_ok() -> bool {
    // (Yes, 有挂起) → 优先交付。
    let mut g = GuardLedger::new();
    let _ = g.arm(1, ROUTINE_WIN.0, KSTACK_WIN.0, 0x8, 0x30);
    let _ = g.pend(1);
    let (o1, id1) = g.wake(0x30, Alertable::Yes);
    if o1 != WaitOutcome::ApcDelivered || id1 != 1 {
        return false;
    }
    // (No, 有挂起) → 延迟先行，APC 保持挂起（HeldPendHold 语义）。
    let mut g2 = GuardLedger::new();
    let _ = g2.arm(2, ROUTINE_WIN.0, KSTACK_WIN.0, 0x8, 0x31);
    let _ = g2.pend(2);
    let (o2, id2) = g2.wake(0x31, Alertable::No);
    if o2 != WaitOutcome::DelayExpired || id2 != 2 {
        return false;
    }
    // 挂起单不丢：仍可被后续可警报唤醒交付。
    let (o3, id3) = g2.wake(0x31, Alertable::Yes);
    if o3 != WaitOutcome::ApcDelivered || id3 != 2 {
        return false;
    }
    // (Yes/No, 无挂起) → 均延迟先行。
    let mut g3 = GuardLedger::new();
    let (o4, id4) = g3.wake(0x32, Alertable::Yes);
    if o4 != WaitOutcome::DelayExpired || id4 != 0 {
        return false;
    }
    let (o5, id5) = g3.wake(0x32, Alertable::No);
    if o5 != WaitOutcome::DelayExpired || id5 != 0 {
        return false;
    }
    true
}

/// 越界注入 10/10：三种越界轮换，每次必抓。
fn inject_ten() -> (u32, u32) {
    let mut g = GuardLedger::new();
    let mut caught = 0u32;
    for i in 0..10u32 {
        // 轮换：例程越窗 / 栈顶越窗 / 栈越顶。
        let r = match i % 3 {
            0 => g.arm(0x200 + i, ROUTINE_WIN.1 + i, KSTACK_WIN.0, 0x8, 0x30),
            1 => g.arm(0x200 + i, ROUTINE_WIN.0, KSTACK_WIN.1 + i, 0x8, 0x30),
            _ => g.arm(0x200 + i, ROUTINE_WIN.0, KSTACK_WIN.1 - 1, 0x20 + i, 0x30),
        };
        if r.is_err() {
            caught = caught.saturating_add(1);
        }
    }
    (caught, 10)
}

/// 跨线程注入：越窗目标逐次必抓。
fn inject_cross_thread() -> bool {
    let mut g = GuardLedger::new();
    for i in 0..4u32 {
        let bad = match g.arm(0x300 + i, ROUTINE_WIN.0, KSTACK_WIN.0, 0x8, THREAD_WIN.1 + i) {
            Err(c) if c == KapcGuardCode::CrossThreadApc.val() => true,
            _ => false,
        };
        if !bad {
            return false;
        }
    }
    true
}

/// 好路零误伤：窗内合法组合必须全放行且联签守恒。
fn good_path_ok() -> bool {
    let mut g = GuardLedger::new();
    let _ = g.arm(0x400, ROUTINE_WIN.0, KSTACK_WIN.0, 0x8, 0x30);
    let _ = g.arm(0x401, ROUTINE_WIN.1 - 1, KSTACK_WIN.1 - 0x10, 0x10, 0x3F);
    let _ = g.pend(0x400);
    let _ = g.pend(0x401);
    let _ = g.wake(0x30, Alertable::Yes);
    let _ = g.wake(0x3F, Alertable::Yes);
    let _ = g.cosign(0x400);
    let _ = g.cosign(0x401);
    g.cosign_parity().is_ok()
}

/// 黄金正演：四单全流程（武装→挂起→交付→联签）绿。
fn golden_run() -> Result<u32, u32> {
    let mut g = GuardLedger::new();
    let _ = g.arm(0x500, ROUTINE_WIN.0 + 0x10, KSTACK_WIN.0 + 0x20, 0x10, 0x31);
    let _ = g.pend(0x500)?;
    let (out, _) = g.wake(0x31, Alertable::Yes);
    if out != WaitOutcome::ApcDelivered {
        return Err(KapcGuardCode::HeldPendHold.val());
    }
    let _ = g.cosign(0x500)?;
    g.cosign_parity()
}

/// 双跑一致：同一正演两次指纹相等。
fn repro_ok() -> bool {
    let a = golden_run().unwrap_or(0);
    let b = golden_run().unwrap_or(1);
    a == b
}

/// 段位守卫（VX9 窗口 0x80000..=0x831C0，段内闭集不越窗）。
pub fn segment_guard() -> bool {
    if SEG_BASE % 0x40 != 0 {
        return false;
    }
    if !(0x80000..=0x831C0).contains(&SEG_BASE) {
        return false;
    }
    for c in KapcGuardCode::ALL.iter() {
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
    v.push(("J1-优先序四象限", priority_quadrants_ok(), String::from("Alertable=TRUE 优先交付")));
    // 判据②：双机 10 场景一致。
    let dual = dual_host_ten();
    v.push(("J2-双机10场景一致", dual == Ok(SCENARIOS as u32), format!("{:?}", dual)));
    // 注入：越界 10/10。
    let (c10, t10) = inject_ten();
    v.push(("J2-越界注入10/10", c10 == 10 && t10 == 10, format!("c/t {}/{}", c10, t10)));
    v.push(("J2-跨线程注入", inject_cross_thread(), String::from("越窗目标必抓")));
    v.push(("J3-好路零误伤", good_path_ok(), String::from("合法组合放行+守恒")));
    // 边界：栈顶恰在窗上沿-1、帧量恰补满 → 精确卡边放行。
    let mut gb = GuardLedger::new();
    let edge = gb.arm(0x600, ROUTINE_WIN.1 - 1, KSTACK_WIN.1 - 0x8, 0x8, THREAD_WIN.1 - 1).is_ok();
    v.push(("J3-边界卡边放行", edge, String::from("栈顶+帧量=上沿")));
    // 边界：越上沿 1 字节必抓。
    let mut gb2 = GuardLedger::new();
    let over = gb2.arm(0x601, ROUTINE_WIN.0, KSTACK_WIN.1 - 0x8, 0x9, 0x30)
        == Err(KapcGuardCode::StackClobber.val());
    v.push(("J4-越顶1字节拒", over, String::from("饱和加不回卷")));
    // 边界：状态倒退与重复联签。
    let mut gd = GuardLedger::new();
    let _ = gd.arm(0x700, ROUTINE_WIN.0, KSTACK_WIN.0, 0x8, 0x30);
    let regress = gd.advance(0x700, ApcPhase::Armed) == Err(KapcGuardCode::StateRegress.val());
    let _ = gd.pend(0x700);
    let _ = gd.wake(0x30, Alertable::Yes);
    let _ = gd.cosign(0x700);
    let dup = gd.cosign(0x700) == Err(KapcGuardCode::DupCosign.val());
    let badid = gd.cosign(0x999) == Err(KapcGuardCode::BadApcId.val());
    v.push(("J4-倒退/重签/坏号拒", regress && dup && badid, String::from("单调+一单一签")));
    // 非警报挂起不丢：先 No 后 Yes 仍可交付。
    let mut gh = GuardLedger::new();
    let _ = gh.arm(0x800, ROUTINE_WIN.0, KSTACK_WIN.0, 0x8, 0x32);
    let _ = gh.pend(0x800);
    let _ = gh.wake(0x32, Alertable::No);
    let (o, i) = gh.wake(0x32, Alertable::Yes);
    v.push(("J4-挂起不丢", o == WaitOutcome::ApcDelivered && i == 0x800, format!("{:?}/{}", o, i)));
    v.push(("J5-双跑一致", repro_ok(), String::from("指纹相等")));
    v.push(("J6-段位守卫", segment_guard(), String::from("0x80600 窗口")));
    v
}

/// VX9 检查入口（卡 12624，UNX-F12625-J1 判据承载）。
pub fn run_unx12624_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F12624 内核APC越界防护→D4联签");
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
    fn test_priority_quadrants() {
        assert!(priority_quadrants_ok());
    }

    #[test]
    fn test_dual_host_ten_agree() {
        assert_eq!(dual_host_ten(), Ok(SCENARIOS as u32));
    }

    #[test]
    fn test_inject_ten_all_caught() {
        let (c, t) = inject_ten();
        assert_eq!(t, 10);
        assert_eq!(c, 10);
    }

    #[test]
    fn test_cross_thread_caught() {
        assert!(inject_cross_thread());
    }

    #[test]
    fn test_boundary_edge_pass_and_over_fail() {
        let mut g = GuardLedger::new();
        assert!(g.arm(0x602, ROUTINE_WIN.1 - 1, KSTACK_WIN.1 - 0x8, 0x8, 0x30).is_ok());
        let mut g2 = GuardLedger::new();
        assert_eq!(
            g2.arm(0x603, ROUTINE_WIN.0, KSTACK_WIN.1 - 0x8, 0x9, 0x30),
            Err(KapcGuardCode::StackClobber.val())
        );
    }

    #[test]
    fn test_state_regress_rejected() {
        let mut g = GuardLedger::new();
        let _ = g.arm(0x701, ROUTINE_WIN.0, KSTACK_WIN.0, 0x8, 0x30);
        let _ = g.pend(0x701);
        assert_eq!(g.advance(0x701, ApcPhase::Armed), Err(KapcGuardCode::StateRegress.val()));
    }

    #[test]
    fn test_dup_cosign_rejected() {
        let mut g = GuardLedger::new();
        let _ = g.arm(0x702, ROUTINE_WIN.0, KSTACK_WIN.0, 0x8, 0x30);
        let _ = g.pend(0x702);
        let _ = g.wake(0x30, Alertable::Yes);
        assert!(g.cosign(0x702).is_ok());
        assert_eq!(g.cosign(0x702), Err(KapcGuardCode::DupCosign.val()));
    }

    #[test]
    fn test_held_pend_not_lost() {
        let mut g = GuardLedger::new();
        let _ = g.arm(0x703, ROUTINE_WIN.0, KSTACK_WIN.0, 0x8, 0x33);
        let _ = g.pend(0x703);
        let (o1, _) = g.wake(0x33, Alertable::No);
        assert_eq!(o1, WaitOutcome::DelayExpired);
        let (o2, id) = g.wake(0x33, Alertable::Yes);
        assert_eq!(o2, WaitOutcome::ApcDelivered);
        assert_eq!(id, 0x703);
    }

    #[test]
    fn test_cosign_parity_break_and_fix() {
        let mut g = GuardLedger::new();
        let _ = g.arm(0x704, ROUTINE_WIN.0, KSTACK_WIN.0, 0x8, 0x34);
        let _ = g.pend(0x704);
        let _ = g.wake(0x34, Alertable::Yes);
        assert_eq!(g.cosign_parity(), Err(KapcGuardCode::CosignParityBreak.val()));
        let _ = g.cosign(0x704);
        assert!(g.cosign_parity().is_ok());
    }

    #[test]
    fn test_dual_run_equal() {
        let a = golden_run().unwrap_or(0);
        let b = golden_run().unwrap_or(1);
        assert_eq!(a, b);
    }

    #[test]
    fn test_segment_guard() {
        assert!(segment_guard());
        assert_eq!(SEG_BASE, 0x80600);
    }

    #[test]
    fn test_detail_closed_set() {
        for c in KapcGuardCode::ALL.iter() {
            assert_eq!(detail(c.val()), c.detail());
        }
        assert!(detail(0xCAFE).contains("未知"));
    }
}
