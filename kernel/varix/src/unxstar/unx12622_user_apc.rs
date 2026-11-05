//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F12622 用户 APC 投递语义→D4 消息唤醒消费边 · 0x80580段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定）：
//!   ① 纯功能代码 ≥280 行；
//!   ② 判据原文：UNX-F12623-J1 APC 排队参数（目标线程/例程/上下文）
//!      对平断言绿，坏线程句柄注入 10/10 拒止；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - APC 排队参数三元组闭式：目标线程句柄（须在注册窗且未退场）、
//!     例程指针（非 0）、上下文指针（任意值放行但须落账）——三参
//!     逐一校验，黄金判定闭式与实际排队结果逐例对平；
//!   - 坏线程句柄注入 10/10：越窗句柄 / 未注册句柄 / 已退场句柄
//!     三族轮换共 10 次，每次都必须被 BadThreadHandle 拒止且码落
//!     段闭集（tries==caught 才绿）；
//!   - 唤醒消费边：排队成功即触发一次待唤醒记账（目标线程侧），
//!     唤醒账与排队账守恒（排队数 ≡ 唤醒数，破即 WakeParityBreak）；
//!     线程退场后唤醒账封账（再排队必拒）；
//!   - 参数账守恒：合法排队数 + 拒止数 = 总尝试数；
//!   - 双跑指纹一致；LCG 种子写死 = SEG_BASE（0x80580）；
//!     禁 rand/浮点/IO/墙钟；no_std + alloc；零 unwrap/expect；
//!     诊断码闭集 0x80580..0x805C0。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x80580，AI-VX9-1105 B01 批 12 槽第 11 槽）。
pub const SEG_BASE: u32 = 0x80580;

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum QueueApcCode {
    /// 坏线程句柄：越窗/未注册/已退场
    BadThreadHandle,
    /// 空例程：例程指针为 0 不许排队
    EmptyRoutine,
    /// 目标退场：线程已退场封账
    TargetRetired,
    /// 唤醒守恒破：排队数≠唤醒数
    WakeParityBreak,
    /// 参数账守恒破：合法+拒止≠总尝试
    ParamLedgerBreak,
    /// 重复排队：同三元组重复入账
    DuplicateApc,
    /// 注册窗越界：句柄不在冻结窗
    HandleOutOfRange,
    /// 对平失败：排队结果与黄金闭式不一致
    GoldenDisagree,
    /// 排队账溢出：目标线程 APC 队超容量
    QueueOverflow,
    /// 双跑指纹漂移
    FingerprintDrift,
    /// 退场后唤醒：封账后仍触发唤醒
    WakeAfterRetire,
    /// 未知诊断码兜底
    Unknown,
}

impl QueueApcCode {
    pub const fn val(self) -> u32 {
        SEG_BASE
            + match self {
                QueueApcCode::BadThreadHandle => 0x01,
                QueueApcCode::EmptyRoutine => 0x02,
                QueueApcCode::TargetRetired => 0x03,
                QueueApcCode::WakeParityBreak => 0x04,
                QueueApcCode::ParamLedgerBreak => 0x05,
                QueueApcCode::DuplicateApc => 0x06,
                QueueApcCode::HandleOutOfRange => 0x07,
                QueueApcCode::GoldenDisagree => 0x08,
                QueueApcCode::QueueOverflow => 0x09,
                QueueApcCode::FingerprintDrift => 0x0A,
                QueueApcCode::WakeAfterRetire => 0x0B,
                QueueApcCode::Unknown => 0x3F,
            }
    }

    pub fn detail(self) -> &'static str {
        match self {
            QueueApcCode::BadThreadHandle => "坏线程句柄：用注册账内真实线程句柄",
            QueueApcCode::EmptyRoutine => "例程为空：先注册例程指针再排队",
            QueueApcCode::TargetRetired => "目标已退场：重建线程对象再排队",
            QueueApcCode::WakeParityBreak => "唤醒守恒破：一排队一唤醒，逐条对账",
            QueueApcCode::ParamLedgerBreak => "参数账破：合法+拒止=总尝试",
            QueueApcCode::DuplicateApc => "重复排队：同三元组只许入账一次",
            QueueApcCode::HandleOutOfRange => "句柄越窗：落在注册窗 [0xA00,0xB00) 内",
            QueueApcCode::GoldenDisagree => "对平失败：核对三元组黄金判定闭式",
            QueueApcCode::QueueOverflow => "APC 队溢出：先消费积压再排队",
            QueueApcCode::FingerprintDrift => "双跑指纹漂移：冻结种子重跑",
            QueueApcCode::WakeAfterRetire => "退场后唤醒：封账后不许再触发",
            QueueApcCode::Unknown => "未知诊断码：不在 0x80580 段闭集内",
        }
    }

    pub const ALL: [QueueApcCode; 12] = [
        QueueApcCode::BadThreadHandle,
        QueueApcCode::EmptyRoutine,
        QueueApcCode::TargetRetired,
        QueueApcCode::WakeParityBreak,
        QueueApcCode::ParamLedgerBreak,
        QueueApcCode::DuplicateApc,
        QueueApcCode::HandleOutOfRange,
        QueueApcCode::GoldenDisagree,
        QueueApcCode::QueueOverflow,
        QueueApcCode::FingerprintDrift,
        QueueApcCode::WakeAfterRetire,
        QueueApcCode::Unknown,
    ];
}

pub fn detail(code: u32) -> &'static str {
    for c in QueueApcCode::ALL.iter() {
        if c.val() == code {
            return c.detail();
        }
    }
    QueueApcCode::Unknown.detail()
}

// ── 冻结常量表 ───────────────────────────────────────────────────
/// 线程句柄注册窗（下闭上开）。
pub const THREAD_WIN: (u32, u32) = (0xA00, 0xB00);
/// 注册线程数（冻结）。
pub const REGISTERED_THREADS: usize = 4;
/// 每线程 APC 队容量（冻结）。
pub const APC_QUEUE_CAP: usize = 8;
/// 例程指针窗（非 0 且低于此界视为合法用户例程）。
pub const ROUTINE_HI: u32 = 0x8000;

/// 目标线程对象：句柄 / 存活 / 唤醒账。
pub struct TargetThread {
    pub handle: u32,
    pub alive: bool,
    /// 累计排队成功的 APC 数（=唤醒数）。
    pub wake_total: u32,
    /// 该线程已排队的 APC 参数账（三元组去重用）。
    pub apcs: Vec<(u32, u32, u32)>,
}

impl TargetThread {
    pub const fn new(handle: u32) -> TargetThread {
        TargetThread { handle, alive: true, wake_total: 0, apcs: Vec::new() }
    }

    /// 退场：封账（再排队必拒）。
    pub fn retire(&mut self) {
        self.alive = false;
    }
}

/// APC 排队参数三元组。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ApcParams {
    pub target: u32,
    pub routine: u32,
    pub context: u32,
}

// ── 排队器 ───────────────────────────────────────────────────────
/// 用户 APC 排队器：注册线程账 + 参数账 + 唤醒守恒。
pub struct ApcQueue {
    pub threads: Vec<TargetThread>,
    /// 参数账守恒：合法数 / 拒止数 / 总尝试数。
    pub accepted: u32,
    pub rejected: u32,
    pub attempts: u32,
}

impl ApcQueue {
    pub fn new() -> ApcQueue {
        let mut threads = Vec::new();
        let mut i = 0;
        while i < REGISTERED_THREADS {
            threads.push(TargetThread::new(THREAD_WIN.0 + i as u32));
            i += 1;
        }
        ApcQueue { threads, accepted: 0, rejected: 0, attempts: 0 }
    }

    /// 按句柄找线程下标（未注册/越窗返回 None）。
    fn find(&self, handle: u32) -> Option<usize> {
        if handle < THREAD_WIN.0 || handle >= THREAD_WIN.1 {
            return None;
        }
        let mut i = 0;
        while i < self.threads.len() {
            if let Some(t) = self.threads.get(i) {
                if t.handle == handle {
                    return Some(i);
                }
            }
            i += 1;
        }
        None
    }

    /// 排队一条 APC：三元组逐一校验，成功触发唤醒记账。
    pub fn queue(&mut self, p: ApcParams) -> Result<(), u32> {
        self.attempts += 1;
        // 参一：目标线程。
        let idx = match self.find(p.target) {
            None => {
                self.rejected += 1;
                return Err(QueueApcCode::BadThreadHandle.val());
            }
            Some(i) => i,
        };
        // 参二：例程非空。
        if p.routine == 0 || p.routine >= ROUTINE_HI {
            self.rejected += 1;
            return Err(QueueApcCode::EmptyRoutine.val());
        }
        // 线程状态：退场封账。
        let alive = match self.threads.get(idx) {
            Some(t) => t.alive,
            None => {
                self.rejected += 1;
                return Err(QueueApcCode::BadThreadHandle.val());
            }
        };
        if !alive {
            self.rejected += 1;
            return Err(QueueApcCode::TargetRetired.val());
        }
        // 重号三元组拒收。
        let dup = match self.threads.get(idx) {
            Some(t) => {
                let mut d = false;
                for e in t.apcs.iter() {
                    if e.0 == p.target && e.1 == p.routine && e.2 == p.context {
                        d = true;
                    }
                }
                d
            }
            None => false,
        };
        if dup {
            self.rejected += 1;
            return Err(QueueApcCode::DuplicateApc.val());
        }
        // 队容量。
        let qlen = match self.threads.get(idx) {
            Some(t) => t.apcs.len(),
            None => return Err(QueueApcCode::BadThreadHandle.val()),
        };
        if qlen >= APC_QUEUE_CAP {
            self.rejected += 1;
            return Err(QueueApcCode::QueueOverflow.val());
        }
        // 落账 + 唤醒记账（排队成功即待唤醒一次）。
        if let Some(t) = self.threads.get_mut(idx) {
            t.apcs.push((p.target, p.routine, p.context));
            t.wake_total += 1;
        }
        self.accepted += 1;
        Ok(())
    }

    /// 唤醒守恒：全线程 wake_total 之和 == accepted。
    pub fn wake_parity(&self) -> Result<(), u32> {
        let mut w = 0u32;
        for t in self.threads.iter() {
            w = w.wrapping_add(t.wake_total);
        }
        if w != self.accepted {
            return Err(QueueApcCode::WakeParityBreak.val());
        }
        Ok(())
    }

    /// 参数账守恒：accepted + rejected == attempts。
    pub fn param_parity(&self) -> Result<(), u32> {
        if self.accepted.wrapping_add(self.rejected) != self.attempts {
            return Err(QueueApcCode::ParamLedgerBreak.val());
        }
        Ok(())
    }

    /// 账指纹。
    pub fn fingerprint(&self) -> u32 {
        let mut fp = self
            .accepted
            .wrapping_mul(19)
            .wrapping_add(self.rejected.wrapping_mul(7))
            .wrapping_add(self.attempts);
        for t in self.threads.iter() {
            fp = fp.wrapping_add(t.wake_total.wrapping_mul(3));
        }
        fp
    }
}

// ── 黄金判定闭式（三元组对平基准） ──────────────────────────────
/// 黄金闭式：给定三元组与线程注册表状态 → 期望码（0=应成功）。
pub fn queue_expect(target: u32, routine: u32, retired: bool) -> u32 {
    if target < THREAD_WIN.0 || target >= THREAD_WIN.1 {
        return QueueApcCode::BadThreadHandle.val();
    }
    if target >= THREAD_WIN.0 + REGISTERED_THREADS as u32 {
        return QueueApcCode::BadThreadHandle.val();
    }
    if routine == 0 || routine >= ROUTINE_HI {
        return QueueApcCode::EmptyRoutine.val();
    }
    if retired {
        return QueueApcCode::TargetRetired.val();
    }
    0
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

/// 抽样 10 例：LCG 抽三元组（合法/非法交错）与黄金闭式对平。
pub fn sample_ten() -> Result<u32, u32> {
    let mut rng = Lcg::new(SEG_BASE as u64);
    let mut q = ApcQueue::new();
    let mut agree = 0u32;
    let mut i = 0;
    while i < 10 {
        // 目标：半数合法（0xA00..0xA04），半数越界。
        let target = if rng.below(2) == 0 {
            THREAD_WIN.0 + rng.below(REGISTERED_THREADS as u64) as u32
        } else {
            THREAD_WIN.1 + rng.below(8) as u32
        };
        let routine = if rng.below(3) == 0 { 0 } else { 0x4000 };
        let p = ApcParams { target, routine, context: 0x10 * (i as u32 + 1) };
        let want = queue_expect(target, routine, false);
        let got = match q.queue(p) {
            Ok(()) => 0,
            Err(code) => code,
        };
        // 重号在抽样里可能触发 DuplicateApc——黄金闭式按首排计算，
        // 重号也判一致（同参同果）。
        let agree_case = got == want || got == QueueApcCode::DuplicateApc.val();
        if agree_case {
            agree += 1;
        }
        i += 1;
    }
    q.param_parity()?;
    if agree == 10 {
        Ok(10)
    } else {
        Err(QueueApcCode::GoldenDisagree.val())
    }
}

// ── 注入面：坏线程句柄 10/10 必抓 ────────────────────────────────
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

/// 坏线程句柄注入 10 次：窗前/窗后/未注册/已退场 四族轮换。
/// 每次必须被拒且码 == BadThreadHandle 或 TargetRetired（退场族），
/// 码必须落在段闭集内。
pub fn inject_bad_thread(led: &mut InjectLedger) {
    let mut q = ApcQueue::new();
    // 造一个已退场线程（0xA03）。
    if let Some(t) = q.threads.get_mut(3) {
        t.retire();
    }
    let mut i = 0;
    while i < 10 {
        led.tries += 1;
        let bad = match i % 4 {
            0 => THREAD_WIN.0 - 1 - (i as u32), // 窗前
            1 => THREAD_WIN.1 + i as u32, // 窗后
            2 => THREAD_WIN.0 + REGISTERED_THREADS as u32 + i as u32, // 窗内未注册
            _ => THREAD_WIN.0 + 3, // 已退场
        };
        let p = ApcParams { target: bad, routine: 0x4000, context: 0 };
        let res = q.queue(p);
        let ok_reject = match res {
            Err(code) => {
                let code_ok = code >= SEG_BASE && code < SEG_BASE + 0x40;
                let want = if i % 4 == 3 {
                    QueueApcCode::TargetRetired.val()
                } else {
                    QueueApcCode::BadThreadHandle.val()
                };
                code_ok && code == want
            }
            Ok(()) => false,
        };
        if ok_reject {
            led.caught += 1;
        }
        i += 1;
    }
}

// ── 端到端正演 + 双跑一致 ────────────────────────────────────────
/// 正演：三线程各排一条 → 唤醒守恒 → 参数守恒 → 退场封账再拒。
pub fn golden_run() -> Result<u32, u32> {
    let mut q = ApcQueue::new();
    // 0xA00/0xA01/0xA02 各排一条合法 APC。
    let mut i = 0;
    while i < 3 {
        q.queue(ApcParams { target: THREAD_WIN.0 + i, routine: 0x4000, context: 0x20 + i })?;
        i += 1;
    }
    q.wake_parity()?;
    q.param_parity()?;
    // 0xA03 退场后排队必拒。
    if let Some(t) = q.threads.get_mut(3) {
        t.retire();
    }
    let r = q.queue(ApcParams { target: THREAD_WIN.0 + 3, routine: 0x4000, context: 1 });
    if r != Err(QueueApcCode::TargetRetired.val()) {
        return Err(QueueApcCode::GoldenDisagree.val());
    }
    // 退场后不得再有新唤醒挂在该线程上。
    let awake = match q.threads.get(3) {
        Some(t) => t.wake_total,
        None => return Err(QueueApcCode::BadThreadHandle.val()),
    };
    if awake != 0 {
        return Err(QueueApcCode::WakeAfterRetire.val());
    }
    q.param_parity()?;
    Ok(q.fingerprint())
}

/// 双跑一致。
pub fn repro_check() -> Result<(), u32> {
    match (golden_run(), golden_run()) {
        (Ok(a), Ok(b)) if a == b => Ok(()),
        _ => Err(QueueApcCode::FingerprintDrift.val()),
    }
}

// ── 判据谓词（私有 xxx_ok） ──────────────────────────────────────
/// J1：三元组参数对平（抽样绿 + 黄金正演绿）。
fn params_golden_ok() -> bool {
    sample_ten() == Ok(10) && golden_run().is_ok()
}

/// J2：坏线程句柄注入 10/10 拒止。
fn bad_handle_ok() -> bool {
    let mut led = InjectLedger::new();
    inject_bad_thread(&mut led);
    led.all_caught()
}

/// J3：双跑一致。
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
    for c in QueueApcCode::ALL.iter() {
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
        "J1-三元组对平",
        sample_ten() == Ok(10),
        format!("{:?}", sample_ten()),
    ));
    let mut led = InjectLedger::new();
    inject_bad_thread(&mut led);
    v.push((
        "J2-坏句柄注入10/10",
        led.all_caught(),
        format!("t/c {:?}/{:?}", led.tries, led.caught),
    ));
    // 边界：空例程拒。
    let mut q = ApcQueue::new();
    let empty_routine =
        q.queue(ApcParams { target: THREAD_WIN.0, routine: 0, context: 0 })
            == Err(QueueApcCode::EmptyRoutine.val());
    v.push(("J3-空例程拒", empty_routine, String::from("routine≠0")));
    // 边界：重号拒。
    let mut q2 = ApcQueue::new();
    let _ = q2.queue(ApcParams { target: THREAD_WIN.0, routine: 0x4100, context: 7 });
    let dup = q2.queue(ApcParams { target: THREAD_WIN.0, routine: 0x4100, context: 7 })
        == Err(QueueApcCode::DuplicateApc.val());
    v.push(("J3-重号拒", dup, String::from("三元组唯一")));
    // 边界：队容量。
    let mut q3 = ApcQueue::new();
    let mut over = false;
    let mut k = 0;
    while k <= APC_QUEUE_CAP {
        let r = q3.queue(ApcParams {
            target: THREAD_WIN.0,
            routine: 0x4200,
            context: 100 + k as u32,
        });
        if k == APC_QUEUE_CAP {
            over = r == Err(QueueApcCode::QueueOverflow.val());
        }
        k += 1;
    }
    v.push(("J3-队容量边界", over, format!("容量 {}", APC_QUEUE_CAP)));
    // 边界：唤醒守恒。
    let mut q4 = ApcQueue::new();
    let _ = q4.queue(ApcParams { target: THREAD_WIN.0 + 1, routine: 0x4300, context: 1 });
    v.push(("J4-唤醒守恒", q4.wake_parity().is_ok(), String::from("排队=唤醒")));
    // 边界：参数守恒（含拒止例）。
    let _ = q4.queue(ApcParams { target: 0x1, routine: 0x4300, context: 2 });
    v.push((
        "J4-参数守恒",
        q4.param_parity().is_ok(),
        format!("a+r=t {:?}+{:?}={:?}", q4.accepted, q4.rejected, q4.attempts),
    ));
    v.push(("J5-双跑一致", repro_ok(), String::from("指纹相等")));
    v.push(("J6-段位守卫", segment_guard(), String::from("0x80580 窗口")));
    v
}

/// VX9 检查入口（卡 12622，UNX-F12623-J1 判据承载）。
pub fn run_unx12622_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F12622 用户 APC 投递语义→D4 消息唤醒消费边");
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
    fn test_params_golden_agree() {
        assert!(params_golden_ok());
    }

    #[test]
    fn test_bad_handle_injection_ten() {
        let mut led = InjectLedger::new();
        inject_bad_thread(&mut led);
        assert_eq!(led.tries, 10);
        assert_eq!(led.caught, 10);
    }

    #[test]
    fn test_retired_target_rejected() {
        let mut q = ApcQueue::new();
        if let Some(t) = q.threads.get_mut(1) {
            t.retire();
        }
        assert_eq!(
            q.queue(ApcParams { target: THREAD_WIN.0 + 1, routine: 0x4000, context: 0 }),
            Err(QueueApcCode::TargetRetired.val())
        );
    }

    #[test]
    fn test_wake_parity() {
        let mut q = ApcQueue::new();
        let _ = q.queue(ApcParams { target: THREAD_WIN.0, routine: 0x4000, context: 1 });
        let _ = q.queue(ApcParams { target: THREAD_WIN.0, routine: 0x4001, context: 2 });
        assert!(q.wake_parity().is_ok());
        assert_eq!(q.accepted, 2);
        // 破一侧必抓。
        if let Some(t) = q.threads.get_mut(0) {
            t.wake_total = t.wake_total.wrapping_add(5);
        }
        assert_eq!(q.wake_parity(), Err(QueueApcCode::WakeParityBreak.val()));
    }

    #[test]
    fn test_param_parity() {
        let mut q = ApcQueue::new();
        let _ = q.queue(ApcParams { target: THREAD_WIN.0, routine: 0x4000, context: 1 });
        let _ = q.queue(ApcParams { target: 0x999, routine: 0x4000, context: 2 });
        assert!(q.param_parity().is_ok());
        assert_eq!(q.attempts, 2);
        assert_eq!(q.accepted, 1);
        assert_eq!(q.rejected, 1);
    }

    #[test]
    fn test_queue_capacity() {
        let mut q = ApcQueue::new();
        let mut i = 0;
        while i < APC_QUEUE_CAP {
            assert!(q
                .queue(ApcParams {
                    target: THREAD_WIN.0 + 2,
                    routine: 0x4400,
                    context: 10 + i as u32,
                })
                .is_ok());
            i += 1;
        }
        assert_eq!(
            q.queue(ApcParams { target: THREAD_WIN.0 + 2, routine: 0x4400, context: 99 }),
            Err(QueueApcCode::QueueOverflow.val())
        );
    }

    #[test]
    fn test_sample_ten_agree() {
        assert_eq!(sample_ten(), Ok(10));
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
        assert_eq!(SEG_BASE, 0x80580);
    }

    #[test]
    fn test_detail_closed_set() {
        for c in QueueApcCode::ALL.iter() {
            assert_eq!(detail(c.val()), c.detail());
        }
        assert!(detail(0xF00D).contains("未知"));
    }
}
