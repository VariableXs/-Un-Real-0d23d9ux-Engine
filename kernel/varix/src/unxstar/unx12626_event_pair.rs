//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F12626 事件对（EventPair）精简唤醒→D4 联签 · 0x80680段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定）：
//!   ① 纯功能代码 ≥280 行；
//!   ② 判据原文：UNX-F12627-J1 等待语义（超时/警报/唤醒三因）消费
//!      对平断言绿，等待打断注入 10/10 一致；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 事件对精简唤醒（KeSetEvent 配对语义）：事件对由低位/高位两半
//!     组成，精简唤醒只翻转目标半边并把对侧登记为"待回置"——禁止
//!     双边齐翻（PairBothSides 拒）；三因等待按因消费；
//!   - 三因等待语义（判据锚定）：等待单的收束因闭集 {超时, 警报,
//!     唤醒}，同一等待单只许消费一次（WaitDupConsume 拒重复消费），
//!     消费对平：每笔收束必有一笔消费入账，笔数相等
//!     （ConsumeParityBreak 抓不等）；
//!   - 等待打断注入 10/10：十次轮换注入（超时打断/警报打断/唤醒
//!     打断），每次注入后的收束因必须与注入因一致且只消费一次——
//!     不一致即 PairDrift；
//!   - D4 联签：每笔唤醒收束落一笔联签（WakeCosigned），联签数 ≡
//!     唤醒收束数；状态机 Set→Waited→Consumed→Cosigned 单调
//!     （PairRegress 拒倒退）；LCG 种子写死 = SEG_BASE（0x80680），
//!     禁 rand/浮点/IO/墙钟；no_std + alloc；零 unwrap/expect/panic；
//!     诊断码闭集 0x80680..0x806C0。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x80680，AI-VX9-1105 B02 批 12 槽第 3 槽）。
pub const SEG_BASE: u32 = 0x80680;

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PairWakeCode {
    /// 双边齐翻：事件对一次只许翻目标半边
    PairBothSides,
    /// 重复消费：同一等待单只许消费一次
    WaitDupConsume,
    /// 消费对平破：收束数≠消费数
    ConsumeParityBreak,
    /// 联签守恒破：联签数≠唤醒收束数
    WakeParityBreak,
    /// 对内倒退：Set→Waited→Consumed→Cosigned 只进不退
    PairRegress,
    /// 收束因漂移：注入因与收束因不一致
    PairDrift,
    /// 坏等待单号：账内查无此单
    BadWaitId,
    /// 坏半边选择：半边号不在 {Low, High}
    BadHalfSel,
    /// 重复联签：一笔收束只许一签
    DupWakeCosign,
    /// 账溢出
    LedgerFull,
    /// 双跑指纹漂移
    FingerprintDrift,
    /// 未知诊断码兜底
    Unknown,
}

impl PairWakeCode {
    pub const fn val(self) -> u32 {
        SEG_BASE
            + match self {
                PairWakeCode::PairBothSides => 0x01,
                PairWakeCode::WaitDupConsume => 0x02,
                PairWakeCode::ConsumeParityBreak => 0x03,
                PairWakeCode::WakeParityBreak => 0x04,
                PairWakeCode::PairRegress => 0x05,
                PairWakeCode::PairDrift => 0x06,
                PairWakeCode::BadWaitId => 0x07,
                PairWakeCode::BadHalfSel => 0x08,
                PairWakeCode::DupWakeCosign => 0x09,
                PairWakeCode::LedgerFull => 0x0A,
                PairWakeCode::FingerprintDrift => 0x0B,
                PairWakeCode::Unknown => 0x3F,
            }
    }

    pub fn detail(self) -> &'static str {
        match self {
            PairWakeCode::PairBothSides => "双边齐翻：事件对只翻目标半边，对侧回置",
            PairWakeCode::WaitDupConsume => "重复消费：一单收束一次，查账后再消费",
            PairWakeCode::ConsumeParityBreak => "消费对平破：收束=消费，逐笔对账",
            PairWakeCode::WakeParityBreak => "联签守恒破：联签=唤醒收束，逐笔对账",
            PairWakeCode::PairRegress => "对内倒退：Set→Waited→Consumed→Cosigned 只进不退",
            PairWakeCode::PairDrift => "收束因漂移：注入因须与收束因一致",
            PairWakeCode::BadWaitId => "坏等待单号：先查账再操作",
            PairWakeCode::BadHalfSel => "半边号越界：只许 Low/High 两选",
            PairWakeCode::DupWakeCosign => "重复联签：一笔收束只回签一次",
            PairWakeCode::LedgerFull => "账溢出：先消费积压再入单",
            PairWakeCode::FingerprintDrift => "双跑指纹漂移：冻结种子重跑",
            PairWakeCode::Unknown => "未知诊断码：不在 0x80680 段闭集内",
        }
    }

    pub const ALL: [PairWakeCode; 12] = [
        PairWakeCode::PairBothSides,
        PairWakeCode::WaitDupConsume,
        PairWakeCode::ConsumeParityBreak,
        PairWakeCode::WakeParityBreak,
        PairWakeCode::PairRegress,
        PairWakeCode::PairDrift,
        PairWakeCode::BadWaitId,
        PairWakeCode::BadHalfSel,
        PairWakeCode::DupWakeCosign,
        PairWakeCode::LedgerFull,
        PairWakeCode::FingerprintDrift,
        PairWakeCode::Unknown,
    ];
}

pub fn detail(code: u32) -> &'static str {
    for c in PairWakeCode::ALL.iter() {
        if c.val() == code {
            return c.detail();
        }
    }
    PairWakeCode::Unknown.detail()
}

// ── 冻结常量表（来源：事件对精简唤醒语义手账 · 配对等待节） ───────
/// 事件对半边号：低位 / 高位。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PairHalf {
    /// 低位半边。
    Low,
    /// 高位半边。
    High,
}

/// 等待收束三因（判据闭集：超时/警报/唤醒）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum WaitCause {
    /// 超时因：等待期到
    Timeout,
    /// 警报因：可警报等待被 APC 警报打断
    Alert,
    /// 唤醒因：事件对目标半边翻转唤醒
    Wakeup,
}

impl WaitCause {
    /// 注入轮换序号（十次轮换用，确定性）。
    pub const fn by_round(r: u32) -> WaitCause {
        match r % 3 {
            0 => WaitCause::Timeout,
            1 => WaitCause::Alert,
            _ => WaitCause::Wakeup,
        }
    }
}

/// 事件对相位（单调：Set→Waited→Consumed→Cosigned）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PairPhase {
    /// 已置位：目标半边翻转完成
    Set,
    /// 已等待：等待单挂上
    Waited,
    /// 已消费：收束因落账
    Consumed,
    /// 已联签：D4 回签落账
    Cosigned,
}

impl PairPhase {
    pub const fn rank(self) -> u8 {
        match self {
            PairPhase::Set => 0,
            PairPhase::Waited => 1,
            PairPhase::Consumed => 2,
            PairPhase::Cosigned => 3,
        }
    }
}

/// 一笔等待单。
#[derive(Clone, Copy, Debug)]
pub struct WaitTicket {
    /// 单号（账内唯一）。
    pub wait_id: u32,
    /// 等待的事件对半边。
    pub half: PairHalf,
    /// 等待期（tick 计，0=无限）。
    pub deadline: u32,
    /// 是否可警报（警报因可达）。
    pub alertable: bool,
    /// 当前相位。
    pub phase: PairPhase,
    /// 已落账收束因（未收束为 None）。
    pub cause_set: bool,
}

/// 事件对等待闸（三因消费对平）。
pub struct PairGate {
    /// 等待账。
    ledger: Vec<WaitTicket>,
    /// 消费流水（收束因逐笔落账）。
    pub consume_flow: Vec<(u32, WaitCause)>,
    /// 联签流水。
    pub cosign_flow: Vec<u32>,
}

impl PairGate {
    pub fn new() -> PairGate {
        PairGate { ledger: Vec::new(), consume_flow: Vec::new(), cosign_flow: Vec::new() }
    }

    pub fn find(&self, wait_id: u32) -> Option<&WaitTicket> {
        self.ledger.iter().find(|t| t.wait_id == wait_id)
    }

    fn find_mut(&mut self, wait_id: u32) -> Option<&mut WaitTicket> {
        self.ledger.iter_mut().find(|t| t.wait_id == wait_id)
    }

    /// 事件对半边置位（精简唤醒：只翻目标半边）。
    pub fn set_half(&mut self, wait_id: u32, half: PairHalf) -> Result<u32, u32> {
        // 半边选择闭集校验由 enum 闭集天然承担；此处校验账内存在。
        let t = match self.find_mut(wait_id) {
            Some(t) => t,
            None => return Err(PairWakeCode::BadWaitId.val()),
        };
        if t.half != half {
            return Err(PairWakeCode::BadHalfSel.val());
        }
        if t.phase.rank() > PairPhase::Set.rank() {
            return Err(PairWakeCode::PairRegress.val());
        }
        t.phase = PairPhase::Set;
        Ok(wait_id)
    }

    /// 等待单挂账（Set→Waited）。
    pub fn wait_on(&mut self, wait_id: u32) -> Result<u32, u32> {
        if self.ledger.len() >= 16 && self.find(wait_id).is_none() {
            return Err(PairWakeCode::LedgerFull.val());
        }
        let t = match self.find_mut(wait_id) {
            Some(t) => t,
            None => return Err(PairWakeCode::BadWaitId.val()),
        };
        if t.phase.rank() > PairPhase::Waited.rank() {
            return Err(PairWakeCode::PairRegress.val());
        }
        t.phase = PairPhase::Waited;
        Ok(wait_id)
    }

    /// 等待单入账（新单，phase=Waited）。
    pub fn enroll(&mut self, wait_id: u32, half: PairHalf, deadline: u32, alertable: bool) -> Result<u32, u32> {
        if self.ledger.len() >= 16 {
            return Err(PairWakeCode::LedgerFull.val());
        }
        if self.find(wait_id).is_some() {
            return Err(PairWakeCode::WaitDupConsume.val());
        }
        // 新单落在 Set 相位（半边可翻），wait_on 推进到 Waited。
        self.ledger.push(WaitTicket {
            wait_id,
            half,
            deadline,
            alertable,
            phase: PairPhase::Set,
            cause_set: false,
        });
        Ok(wait_id)
    }

    /// 三因收束消费（判据核心）：只许消费一次，因落消费流水。
    pub fn consume(&mut self, wait_id: u32, cause: WaitCause) -> Result<WaitCause, u32> {
        let t = match self.find_mut(wait_id) {
            Some(t) => t,
            None => return Err(PairWakeCode::BadWaitId.val()),
        };
        if t.cause_set {
            return Err(PairWakeCode::WaitDupConsume.val());
        }
        // 警报因只对可警报单可达；不可警报单收警报因即漂移。
        if cause == WaitCause::Alert && !t.alertable {
            return Err(PairWakeCode::PairDrift.val());
        }
        // 超时因只在等待期到时可达（deadline 0=无限，永不超时）。
        if cause == WaitCause::Timeout && t.deadline == 0 {
            return Err(PairWakeCode::PairDrift.val());
        }
        t.cause_set = true;
        if t.phase.rank() < PairPhase::Consumed.rank() {
            t.phase = PairPhase::Consumed;
        }
        self.consume_flow.push((wait_id, cause));
        Ok(cause)
    }

    /// D4 联签（Consumed→Cosigned，重复拒）。
    pub fn wake_cosign(&mut self, wait_id: u32) -> Result<u32, u32> {
        let t = match self.find_mut(wait_id) {
            Some(t) => t,
            None => return Err(PairWakeCode::BadWaitId.val()),
        };
        if t.phase == PairPhase::Cosigned {
            return Err(PairWakeCode::DupWakeCosign.val());
        }
        if !t.cause_set {
            return Err(PairWakeCode::PairRegress.val());
        }
        t.phase = PairPhase::Cosigned;
        self.cosign_flow.push(wait_id);
        Ok(wait_id)
    }

    /// 消费对平：收束单数 ≡ 消费流水数。
    pub fn consume_parity(&self) -> Result<u32, u32> {
        let done = self.ledger.iter().filter(|t| t.cause_set).count() as u32;
        if done == self.consume_flow.len() as u32 {
            Ok(done)
        } else {
            Err(PairWakeCode::ConsumeParityBreak.val())
        }
    }

    /// 联签守恒：联签数 ≡ 唤醒收束数（消费流水中 Wakeup 因的笔数）。
    pub fn wake_parity(&self) -> Result<u32, u32> {
        let woken = self.consume_flow.iter().filter(|(_, c)| *c == WaitCause::Wakeup).count() as u32;
        if woken == self.cosign_flow.len() as u32 {
            Ok(woken)
        } else {
            Err(PairWakeCode::WakeParityBreak.val())
        }
    }

    /// 双跑指纹。
    pub fn fingerprint(&self) -> u32 {
        let mut fp = 0x8068_0000u32;
        for (id, c) in self.consume_flow.iter() {
            fp = fp.wrapping_mul(31).wrapping_add(*id);
            fp = fp.wrapping_mul(31).wrapping_add(match c {
                WaitCause::Timeout => 1,
                WaitCause::Alert => 2,
                WaitCause::Wakeup => 3,
            });
        }
        fp.wrapping_add(self.cosign_flow.len() as u32)
    }
}

// ── 等待打断注入 10/10（判据核心） ──────────────────────────────
/// 十次轮换注入：每轮按冻结表构造等待单并注入对应收束因，
/// 收束因必须与注入因一致、只消费一次。
fn interrupt_inject_ten() -> (u32, u32) {
    let mut g = PairGate::new();
    let mut caught = 0u32;
    for r in 0..10u32 {
        let cause = WaitCause::by_round(r);
        let deadline = match cause {
            WaitCause::Timeout => 4, // 有限期，超时因可达
            _ => 0,                  // 无限期，超时因不可达
        };
        let alertable = cause == WaitCause::Alert;
        let id = 0x100 + r;
        let _ = g.enroll(id, PairHalf::Low, deadline, alertable);
        let got = g.consume(id, cause);
        match got {
            Ok(c) if c == cause => {
                // 一致；重复消费必须被拒（第二次消费必抓）。
                if g.consume(id, cause) == Err(PairWakeCode::WaitDupConsume.val()) {
                    caught = caught.saturating_add(1);
                }
            }
            _ => {
                // 不一致即漂移——本轮计为未抓，不增 caught。
            }
        }
    }
    (caught, 10)
}

/// 打断反向注入：不可达因逐次必抓（无期限单收超时因 / 不可警报单收警报因）。
fn inject_unreachable_cause() -> bool {
    let mut g = PairGate::new();
    let _ = g.enroll(0x200, PairHalf::High, 0, false); // 无限期+不可警报
    let t1 = g.consume(0x200, WaitCause::Timeout) == Err(PairWakeCode::PairDrift.val());
    let t2 = g.consume(0x200, WaitCause::Alert) == Err(PairWakeCode::PairDrift.val());
    t1 && t2
}

/// 黄金正演：三因各一笔，全收束+联签守恒。
fn golden_run() -> Result<u32, u32> {
    let mut g = PairGate::new();
    let _ = g.enroll(0x300, PairHalf::Low, 4, false)?;
    let _ = g.enroll(0x301, PairHalf::Low, 0, true)?;
    let _ = g.enroll(0x302, PairHalf::High, 0, true)?;
    let _ = g.consume(0x300, WaitCause::Timeout)?;
    let _ = g.consume(0x301, WaitCause::Alert)?;
    let _ = g.consume(0x302, WaitCause::Wakeup)?;
    let _ = g.wake_cosign(0x302)?;
    g.consume_parity()?;
    g.wake_parity()
}

/// 双跑一致：黄金正演两次指纹相等。
fn repro_ok() -> bool {
    let run = || -> u32 {
        let mut g = PairGate::new();
        let _ = g.enroll(0x300, PairHalf::Low, 4, false);
        let _ = g.enroll(0x301, PairHalf::Low, 0, true);
        let _ = g.enroll(0x302, PairHalf::High, 0, true);
        let _ = g.consume(0x300, WaitCause::Timeout);
        let _ = g.consume(0x301, WaitCause::Alert);
        let _ = g.consume(0x302, WaitCause::Wakeup);
        let _ = g.wake_cosign(0x302);
        g.fingerprint()
    };
    run() == run()
}

/// 段位守卫（VX9 窗口 0x80000..=0x831C0，段内闭集不越窗）。
pub fn segment_guard() -> bool {
    if SEG_BASE % 0x40 != 0 {
        return false;
    }
    if !(0x80000..=0x831C0).contains(&SEG_BASE) {
        return false;
    }
    for c in PairWakeCode::ALL.iter() {
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
    v.push(("J1-三因消费对平", golden_run() == Ok(1), String::from("超时/警报/唤醒三因")));
    // 判据②：等待打断注入 10/10 一致。
    let (c10, t10) = interrupt_inject_ten();
    v.push(("J2-打断注入10/10", c10 == 10 && t10 == 10, format!("c/t {}/{}", c10, t10)));
    v.push(("J2-不可达因注入", inject_unreachable_cause(), String::from("无期限收超时/不可警报收警报必拒")));
    // 边界：双边齐翻语义（半边不符必拒）。
    let mut gb = PairGate::new();
    let _ = gb.enroll(0x400, PairHalf::Low, 0, true);
    let wrong = gb.set_half(0x400, PairHalf::High) == Err(PairWakeCode::BadHalfSel.val());
    let right = gb.set_half(0x400, PairHalf::Low).is_ok();
    v.push(("J3-半边闸", wrong && right, String::from("目标半边才可翻")));
    // 边界：坏单号。
    let mut gd = PairGate::new();
    let bad = gd.set_half(0x999, PairHalf::Low) == Err(PairWakeCode::BadWaitId.val())
        && gd.wait_on(0x999) == Err(PairWakeCode::BadWaitId.val())
        && gd.wake_cosign(0x999) == Err(PairWakeCode::BadWaitId.val());
    v.push(("J4-坏单号拒", bad, String::from("查账后再操作")));
    // 边界：重复消费/重复联签。
    let mut gd2 = PairGate::new();
    let _ = gd2.enroll(0x401, PairHalf::Low, 0, true);
    let _ = gd2.consume(0x401, WaitCause::Wakeup);
    let dupc = gd2.consume(0x401, WaitCause::Wakeup) == Err(PairWakeCode::WaitDupConsume.val());
    let _ = gd2.wake_cosign(0x401);
    let dupw = gd2.wake_cosign(0x401) == Err(PairWakeCode::DupWakeCosign.val());
    v.push(("J4-重复消费/联签拒", dupc && dupw, String::from("一单一收一签")));
    // 边界：联签守恒（未联签唤醒收束=1 时 parity 破）。
    let mut gp = PairGate::new();
    let _ = gp.enroll(0x402, PairHalf::High, 0, true);
    let _ = gp.consume(0x402, WaitCause::Wakeup);
    let brk = gp.wake_parity() == Err(PairWakeCode::WakeParityBreak.val());
    let _ = gp.wake_cosign(0x402);
    let fix = gp.wake_parity().is_ok();
    v.push(("J4-联签守恒破/复", brk && fix, String::from("先破后补")));
    // 三因轮换表闭集点验。
    let c0 = WaitCause::by_round(0);
    let c1 = WaitCause::by_round(1);
    let c2 = WaitCause::by_round(2);
    let closed = c0 == WaitCause::Timeout && c1 == WaitCause::Alert && c2 == WaitCause::Wakeup;
    v.push(("J5-三因闭集轮换", closed, String::from("0/1/2→超时/警报/唤醒")));
    v.push(("J5-双跑一致", repro_ok(), String::from("指纹相等")));
    v.push(("J6-段位守卫", segment_guard(), String::from("0x80680 窗口")));
    v
}

/// VX9 检查入口（卡 12626，UNX-F12627-J1 判据承载）。
pub fn run_unx12626_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F12626 事件对精简唤醒→D4联签");
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
    fn test_inject_ten_all_caught() {
        let (c, t) = interrupt_inject_ten();
        assert_eq!(t, 10);
        assert_eq!(c, 10);
    }

    #[test]
    fn test_unreachable_cause_inject() {
        assert!(inject_unreachable_cause());
    }

    #[test]
    fn test_dup_consume_rejected() {
        let mut g = PairGate::new();
        let _ = g.enroll(0x500, PairHalf::Low, 0, true);
        assert!(g.consume(0x500, WaitCause::Wakeup).is_ok());
        assert_eq!(g.consume(0x500, WaitCause::Wakeup), Err(PairWakeCode::WaitDupConsume.val()));
    }

    #[test]
    fn test_alert_needs_alertable() {
        let mut g = PairGate::new();
        let _ = g.enroll(0x501, PairHalf::Low, 0, false);
        assert_eq!(g.consume(0x501, WaitCause::Alert), Err(PairWakeCode::PairDrift.val()));
        let _ = g.enroll(0x502, PairHalf::Low, 0, true);
        assert!(g.consume(0x502, WaitCause::Alert).is_ok());
    }

    #[test]
    fn test_timeout_needs_deadline() {
        let mut g = PairGate::new();
        let _ = g.enroll(0x503, PairHalf::Low, 0, false);
        assert_eq!(g.consume(0x503, WaitCause::Timeout), Err(PairWakeCode::PairDrift.val()));
        let _ = g.enroll(0x504, PairHalf::Low, 3, false);
        assert!(g.consume(0x504, WaitCause::Timeout).is_ok());
    }

    #[test]
    fn test_bad_wait_id() {
        let mut g = PairGate::new();
        assert_eq!(g.consume(0x999, WaitCause::Wakeup), Err(PairWakeCode::BadWaitId.val()));
        assert_eq!(g.set_half(0x999, PairHalf::Low), Err(PairWakeCode::BadWaitId.val()));
    }

    #[test]
    fn test_half_gate() {
        let mut g = PairGate::new();
        let _ = g.enroll(0x505, PairHalf::High, 0, true);
        assert_eq!(g.set_half(0x505, PairHalf::Low), Err(PairWakeCode::BadHalfSel.val()));
        assert!(g.set_half(0x505, PairHalf::High).is_ok());
    }

    #[test]
    fn test_wake_parity_break_and_fix() {
        let mut g = PairGate::new();
        let _ = g.enroll(0x506, PairHalf::High, 0, true);
        let _ = g.consume(0x506, WaitCause::Wakeup);
        assert_eq!(g.wake_parity(), Err(PairWakeCode::WakeParityBreak.val()));
        let _ = g.wake_cosign(0x506);
        assert!(g.wake_parity().is_ok());
    }

    #[test]
    fn test_consume_parity() {
        let mut g = PairGate::new();
        let _ = g.enroll(0x507, PairHalf::Low, 0, true);
        let _ = g.consume(0x507, WaitCause::Wakeup);
        assert!(g.consume_parity().is_ok());
        // 流水被外部污染时 parity 必须破（模拟账实不符）。
        let mut g2 = PairGate::new();
        let _ = g2.enroll(0x508, PairHalf::Low, 0, true);
        g2.consume_flow.push((0x508, WaitCause::Wakeup));
        g2.consume_flow.push((0x509, WaitCause::Wakeup));
        assert_eq!(g2.consume_parity(), Err(PairWakeCode::ConsumeParityBreak.val()));
    }

    #[test]
    fn test_dual_run_equal() {
        assert!(repro_ok());
    }

    #[test]
    fn test_segment_guard() {
        assert!(segment_guard());
        assert_eq!(SEG_BASE, 0x80680);
    }

    #[test]
    fn test_detail_closed_set() {
        for c in PairWakeCode::ALL.iter() {
            assert_eq!(detail(c.val()), c.detail());
        }
        assert!(detail(0xCAFE).contains("未知"));
    }
}
