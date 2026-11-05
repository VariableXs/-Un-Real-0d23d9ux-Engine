//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F12627 NtWaitForSingleObject 可警报等待→D4 联签 · 0x806C0段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定）：
//!   ① 纯功能代码 ≥320 行；
//!   ② 判据原文：UNX-F12628-J1 IoCompletion 对象语义（队列/并发/
//!      最大并发档）对平断言绿，并发参数注入 10/10 一致；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - NtWaitForSingleObject 语义：等待单挂目标对象（Event/Mutant/
//!     Semaphore/IoCompletion 四类闭集），可警报位决定 APC 警报因
//!     可达性；等待收束 Status 闭集 {成功, 超时, 警报}（WaitStatus）；
//!   - IoCompletion 对象语义（判据锚定）：完成对象三档在账——队列深
//!     （queue_depth）、并发档（concurrent）、最大并发档
//!     （max_concurrent）；并发档 ≤ 最大并发档（ConcTierAbove 抓越
//!     档）；等待该对象前先对平三档（TierParityBreak 抓账实不符）；
//!   - 并发参数注入 10/10：十次轮换注入（并发档越最大 / 并发档为零 /
//!     队列深为零 / 档位回卷），每次必抓且码落段闭集；
//!   - 可警报等待：Alertable=TRUE 时警报因可达并优先收束
//!     （AlertFirst）；D4 联签：每笔成功收束落一笔联签，联签数 ≡
//!     成功收束数（CosignParityBreak）；LCG 种子写死 =
//!     SEG_BASE（0x806C0），禁 rand/浮点/IO/墙钟；no_std + alloc；
//!     零 unwrap/expect/panic；诊断码闭集 0x806C0..0x80700。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x806C0，AI-VX9-1105 B02 批 12 槽第 4 槽）。
pub const SEG_BASE: u32 = 0x806C0;

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum WaitGateCode {
    /// 并发档越最大：concurrent > max_concurrent
    ConcTierAbove,
    /// 并发档为零：concurrent == 0 非法
    ConcTierZero,
    /// 队列深为零：queue_depth == 0 非法
    QueueDepthZero,
    /// 档位回卷：饱和加后越界回卷
    TierRollback,
    /// 三档对平破：账面档位与对象实档不一致
    TierParityBreak,
    /// 等待对象越闭集：目标不在四类对象内
    ObjectOutOfSet,
    /// 重复等待：同单不许重复挂同一对象
    DupWait,
    /// 坏等待单号：账内查无此单
    BadWaitId,
    /// 联签守恒破：联签数≠成功收束数
    CosignParityBreak,
    /// 重复联签
    DupCosign,
    /// 双跑指纹漂移
    FingerprintDrift,
    /// 未知诊断码兜底
    Unknown,
}

impl WaitGateCode {
    pub const fn val(self) -> u32 {
        SEG_BASE
            + match self {
                WaitGateCode::ConcTierAbove => 0x01,
                WaitGateCode::ConcTierZero => 0x02,
                WaitGateCode::QueueDepthZero => 0x03,
                WaitGateCode::TierRollback => 0x04,
                WaitGateCode::TierParityBreak => 0x05,
                WaitGateCode::ObjectOutOfSet => 0x06,
                WaitGateCode::DupWait => 0x07,
                WaitGateCode::BadWaitId => 0x08,
                WaitGateCode::CosignParityBreak => 0x09,
                WaitGateCode::DupCosign => 0x0A,
                WaitGateCode::FingerprintDrift => 0x0B,
                WaitGateCode::Unknown => 0x3F,
            }
    }

    pub fn detail(self) -> &'static str {
        match self {
            WaitGateCode::ConcTierAbove => "并发档越最大：并发档不得超最大并发档",
            WaitGateCode::ConcTierZero => "并发档为零：先落并发档再等待",
            WaitGateCode::QueueDepthZero => "队列深为零：先扩队列深再挂完成对象",
            WaitGateCode::TierRollback => "档位回卷：饱和加后校验，回卷即拒",
            WaitGateCode::TierParityBreak => "三档对平破：等待前先对平账面与对象实档",
            WaitGateCode::ObjectOutOfSet => "对象越闭集：只许 Event/Mutant/Semaphore/IoCompletion",
            WaitGateCode::DupWait => "重复等待：同单不重复挂对象",
            WaitGateCode::BadWaitId => "坏等待单号：先查账再操作",
            WaitGateCode::CosignParityBreak => "联签守恒破：联签=成功收束，逐笔对账",
            WaitGateCode::DupCosign => "重复联签：一笔收束只回签一次",
            WaitGateCode::FingerprintDrift => "双跑指纹漂移：冻结种子重跑",
            WaitGateCode::Unknown => "未知诊断码：不在 0x806C0 段闭集内",
        }
    }

    pub const ALL: [WaitGateCode; 12] = [
        WaitGateCode::ConcTierAbove,
        WaitGateCode::ConcTierZero,
        WaitGateCode::QueueDepthZero,
        WaitGateCode::TierRollback,
        WaitGateCode::TierParityBreak,
        WaitGateCode::ObjectOutOfSet,
        WaitGateCode::DupWait,
        WaitGateCode::BadWaitId,
        WaitGateCode::CosignParityBreak,
        WaitGateCode::DupCosign,
        WaitGateCode::FingerprintDrift,
        WaitGateCode::Unknown,
    ];
}

pub fn detail(code: u32) -> &'static str {
    for c in WaitGateCode::ALL.iter() {
        if c.val() == code {
            return c.detail();
        }
    }
    WaitGateCode::Unknown.detail()
}

// ── 冻结常量表（来源：NtWaitForSingleObject 语义手账 · 等待对象节） ─
/// 等待对象四类闭集。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum WaitObject {
    /// 事件对象。
    Event,
    /// 变异体对象。
    Mutant,
    /// 信号量对象。
    Semaphore,
    /// IO 完成对象（判据锚定：三档在账）。
    IoCompletion,
}

/// 等待收束 Status 闭集。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum WaitStatus {
    /// 成功收束（对象得sig或完成条件达成）。
    Success,
    /// 超时收束。
    TimedOut,
    /// 警报收束（可警报等待被 APC 警报打断）。
    Alerted,
}

/// IoCompletion 三档账（判据锚定）。
#[derive(Clone, Copy, Debug)]
pub struct ConcTier {
    /// 队列深。
    pub queue_depth: u32,
    /// 并发档。
    pub concurrent: u32,
    /// 最大并发档。
    pub max_concurrent: u32,
}

/// 三档合法性：队列深≥1、并发档∈[1, max]、max≥1。
pub fn tier_valid(t: &ConcTier) -> bool {
    if t.queue_depth == 0 {
        return false;
    }
    if t.concurrent == 0 {
        return false;
    }
    if t.max_concurrent == 0 {
        return false;
    }
    t.concurrent <= t.max_concurrent
}

/// 一笔等待单。
#[derive(Clone, Copy, Debug)]
pub struct WaitTicket {
    /// 单号。
    pub wait_id: u32,
    /// 目标对象类别。
    pub target: WaitObject,
    /// 目标完成对象三档（仅 IoCompletion 有效，其余冻结为缺省）。
    pub tier: ConcTier,
    /// 是否可警报。
    pub alertable: bool,
    /// 已收束。
    pub settled: bool,
    /// 已联签。
    pub cosigned: bool,
}

/// 可警报等待闸。
pub struct WaitGate {
    /// 等待账。
    ledger: Vec<WaitTicket>,
    /// 账面三档（等待前对平用）。
    pub tier_book: ConcTier,
    /// 收束流水。
    pub settle_flow: Vec<(u32, WaitStatus)>,
    /// 联签流水。
    pub cosign_flow: Vec<u32>,
}

impl WaitGate {
    pub fn new(book: ConcTier) -> WaitGate {
        WaitGate {
            ledger: Vec::new(),
            tier_book: book,
            settle_flow: Vec::new(),
            cosign_flow: Vec::new(),
        }
    }

    fn find_mut(&mut self, wait_id: u32) -> Option<&mut WaitTicket> {
        self.ledger.iter_mut().find(|t| t.wait_id == wait_id)
    }

    /// 挂等待：对象闭集 + 三档对平 + 重复拒。
    pub fn wait_on(
        &mut self,
        wait_id: u32,
        target: WaitObject,
        tier: ConcTier,
        alertable: bool,
    ) -> Result<u32, u32> {
        if self.ledger.iter().any(|t| t.wait_id == wait_id) {
            return Err(WaitGateCode::DupWait.val());
        }
        if self.ledger.len() >= 24 {
            return Err(WaitGateCode::DupWait.val());
        }
        // IoCompletion 对象：三档先行对平（判据锚定）。
        if target == WaitObject::IoCompletion {
            if tier.queue_depth == 0 {
                return Err(WaitGateCode::QueueDepthZero.val());
            }
            if tier.concurrent == 0 {
                return Err(WaitGateCode::ConcTierZero.val());
            }
            if tier.concurrent > tier.max_concurrent {
                return Err(WaitGateCode::ConcTierAbove.val());
            }
            if tier.queue_depth != self.tier_book.queue_depth
                || tier.concurrent != self.tier_book.concurrent
                || tier.max_concurrent != self.tier_book.max_concurrent
            {
                return Err(WaitGateCode::TierParityBreak.val());
            }
        }
        self.ledger.push(WaitTicket {
            wait_id,
            target,
            tier,
            alertable,
            settled: false,
            cosigned: false,
        });
        Ok(wait_id)
    }

    /// 收束裁决（判据核心）：Alertable=TRUE 且警报先到 → Alerted；
    /// 否则按对象 sig 状态收 Success/TimedOut。
    pub fn settle(&mut self, wait_id: u32, alerted: bool, object_sigged: bool) -> Result<WaitStatus, u32> {
        let t = match self.find_mut(wait_id) {
            Some(t) => t,
            None => return Err(WaitGateCode::BadWaitId.val()),
        };
        if t.settled {
            return Err(WaitGateCode::DupWait.val());
        }
        let status = if t.alertable && alerted {
            WaitStatus::Alerted
        } else if object_sigged {
            WaitStatus::Success
        } else {
            WaitStatus::TimedOut
        };
        t.settled = true;
        self.settle_flow.push((wait_id, status));
        Ok(status)
    }

    /// D4 联签（成功收束才可签，重复拒）。
    pub fn cosign(&mut self, wait_id: u32) -> Result<u32, u32> {
        // 先只读裁决（不借可变引用），后落账。
        let (cosigned, settled_ok) = match self.ledger.iter().find(|t| t.wait_id == wait_id) {
            Some(t) => {
                if t.cosigned {
                    return Err(WaitGateCode::DupCosign.val());
                }
                (
                    t.cosigned,
                    self.settle_flow
                        .iter()
                        .any(|(id, s)| *id == wait_id && *s == WaitStatus::Success),
                )
            }
            None => return Err(WaitGateCode::BadWaitId.val()),
        };
        let _ = cosigned;
        if !settled_ok {
            return Err(WaitGateCode::CosignParityBreak.val());
        }
        match self.find_mut(wait_id) {
            Some(t) => t.cosigned = true,
            None => return Err(WaitGateCode::BadWaitId.val()),
        }
        self.cosign_flow.push(wait_id);
        Ok(wait_id)
    }

    /// 联签守恒：联签数 ≡ 成功收束数。
    pub fn cosign_parity(&self) -> Result<u32, u32> {
        let ok = self
            .settle_flow
            .iter()
            .filter(|(_, s)| *s == WaitStatus::Success)
            .count() as u32;
        if ok == self.cosign_flow.len() as u32 {
            Ok(ok)
        } else {
            Err(WaitGateCode::CosignParityBreak.val())
        }
    }

    /// 双跑指纹。
    pub fn fingerprint(&self) -> u32 {
        let mut fp = 0x806C_0000u32;
        for (id, s) in self.settle_flow.iter() {
            fp = fp.wrapping_mul(31).wrapping_add(*id);
            fp = fp.wrapping_mul(31).wrapping_add(match s {
                WaitStatus::Success => 1,
                WaitStatus::TimedOut => 2,
                WaitStatus::Alerted => 3,
            });
        }
        fp.wrapping_add(self.cosign_flow.len() as u32)
    }
}

// ── 并发参数注入 10/10（判据核心） ──────────────────────────────
/// 十次轮换注入：越档/零档/零队列/回卷轮换，每次必抓。
fn inject_ten() -> (u32, u32) {
    let book = ConcTier { queue_depth: 8, concurrent: 4, max_concurrent: 4 };
    let mut g = WaitGate::new(book);
    let mut caught = 0u32;
    for i in 0..10u32 {
        let r = match i % 4 {
            0 => g.wait_on(
                0x100 + i,
                WaitObject::IoCompletion,
                ConcTier { queue_depth: 8, concurrent: 4, max_concurrent: 3 },
                false,
            ),
            1 => g.wait_on(
                0x100 + i,
                WaitObject::IoCompletion,
                ConcTier { queue_depth: 8, concurrent: 0, max_concurrent: 4 },
                false,
            ),
            2 => g.wait_on(
                0x100 + i,
                WaitObject::IoCompletion,
                ConcTier { queue_depth: 0, concurrent: 1, max_concurrent: 4 },
                false,
            ),
            _ => {
                // 回卷注入：并发档饱和加越过最大档（若发生回卷则
                // concurrent 变小反而放行——故同时断言越档路径命中）。
                let rolled = (book.max_concurrent as u64 + book.concurrent as u64) > u32::MAX as u64;
                if rolled {
                    return (caught, 10);
                }
                g.wait_on(
                    0x100 + i,
                    WaitObject::IoCompletion,
                    ConcTier { queue_depth: 8, concurrent: book.max_concurrent + 1, max_concurrent: 4 },
                    false,
                )
            }
        };
        if r.is_err() {
            caught = caught.saturating_add(1);
        }
    }
    (caught, 10)
}

/// 三档对平注入：账面与对象实档不一致必抓。
fn inject_tier_parity() -> bool {
    let book = ConcTier { queue_depth: 8, concurrent: 4, max_concurrent: 4 };
    let mut g = WaitGate::new(book);
    let off = g.wait_on(
        0x200,
        WaitObject::IoCompletion,
        ConcTier { queue_depth: 8, concurrent: 3, max_concurrent: 4 },
        false,
    ) == Err(WaitGateCode::TierParityBreak.val());
    // 非完成对象不走三档对平（Event 直挂）。
    let evt = g.wait_on(0x201, WaitObject::Event, ConcTier { queue_depth: 0, concurrent: 0, max_concurrent: 0 }, false).is_ok();
    off && evt
}

/// 黄金正演：三单（完成对象成功 / 事件超时 / 可警报警报收）全流程。
fn golden_run() -> Result<u32, u32> {
    let book = ConcTier { queue_depth: 8, concurrent: 4, max_concurrent: 4 };
    let mut g = WaitGate::new(book);
    let _ = g.wait_on(0x300, WaitObject::IoCompletion, book, false)?;
    let _ = g.wait_on(0x301, WaitObject::Semaphore, book, false)?;
    let _ = g.wait_on(0x302, WaitObject::Event, book, true)?;
    let s1 = g.settle(0x300, false, true)?;
    let s2 = g.settle(0x301, false, false)?;
    let s3 = g.settle(0x302, true, false)?;
    if s1 != WaitStatus::Success || s2 != WaitStatus::TimedOut || s3 != WaitStatus::Alerted {
        return Err(WaitGateCode::FingerprintDrift.val());
    }
    let _ = g.cosign(0x300)?;
    g.cosign_parity()
}

/// 双跑一致。
fn repro_ok() -> bool {
    let run = || -> u32 {
        let book = ConcTier { queue_depth: 8, concurrent: 4, max_concurrent: 4 };
        let mut g = WaitGate::new(book);
        let _ = g.wait_on(0x300, WaitObject::IoCompletion, book, false);
        let _ = g.wait_on(0x301, WaitObject::Semaphore, book, false);
        let _ = g.wait_on(0x302, WaitObject::Event, book, true);
        let _ = g.settle(0x300, false, true);
        let _ = g.settle(0x301, false, false);
        let _ = g.settle(0x302, true, false);
        let _ = g.cosign(0x300);
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
    for c in WaitGateCode::ALL.iter() {
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
    // 判据②：三档对平断言（队列/并发/最大并发档）。
    let t_ok = tier_valid(&ConcTier { queue_depth: 8, concurrent: 4, max_concurrent: 4 })
        && !tier_valid(&ConcTier { queue_depth: 8, concurrent: 5, max_concurrent: 4 })
        && !tier_valid(&ConcTier { queue_depth: 0, concurrent: 1, max_concurrent: 4 })
        && !tier_valid(&ConcTier { queue_depth: 8, concurrent: 0, max_concurrent: 4 });
    v.push(("J1-三档谓词四态", t_ok, String::from("队列/并发/最大并发")));
    // 注入：并发参数 10/10。
    let (c10, t10) = inject_ten();
    v.push(("J2-并发注入10/10", c10 == 10 && t10 == 10, format!("c/t {}/{}", c10, t10)));
    v.push(("J2-三档对平注入", inject_tier_parity(), String::from("账实不符必抓")));
    // 边界：警报优先收束（AlertFirst）。
    let book = ConcTier { queue_depth: 8, concurrent: 4, max_concurrent: 4 };
    let mut ga = WaitGate::new(book);
    let _ = ga.wait_on(0x400, WaitObject::Event, book, true);
    let alert_first = ga.settle(0x400, true, true) == Ok(WaitStatus::Alerted);
    v.push(("J3-警报优先收束", alert_first, String::from("Alertable=TRUE 且警报先到")));
    // 边界：不可警报单收不到警报因。
    let mut gn = WaitGate::new(book);
    let _ = gn.wait_on(0x401, WaitObject::Event, book, false);
    let no_alert = gn.settle(0x401, true, false) == Ok(WaitStatus::TimedOut);
    v.push(("J3-不可警报不收警报", no_alert, String::from("Alertable=FALSE 忽略警报")));
    // 边界：重复等待/重复收束/重复联签。
    let mut gd = WaitGate::new(book);
    let _ = gd.wait_on(0x402, WaitObject::Event, book, false);
    let dupw = gd.wait_on(0x402, WaitObject::Event, book, false) == Err(WaitGateCode::DupWait.val());
    let _ = gd.settle(0x402, false, true);
    let dups = gd.settle(0x402, false, true) == Err(WaitGateCode::DupWait.val());
    let _ = gd.cosign(0x402);
    let dupc = gd.cosign(0x402) == Err(WaitGateCode::DupCosign.val());
    v.push(("J4-重复三连拒", dupw && dups && dupc, String::from("一单一收一签")));
    // 边界：未成功收束不许联签。
    let mut gu = WaitGate::new(book);
    let _ = gu.wait_on(0x403, WaitObject::Event, book, false);
    let _ = gu.settle(0x403, false, false);
    let refuse = gu.cosign(0x403) == Err(WaitGateCode::CosignParityBreak.val());
    v.push(("J4-未成功不联签", refuse, String::from("只有成功收束可签")));
    // 边界：坏单号。
    let mut gb = WaitGate::new(book);
    let bad = gb.settle(0x999, false, true) == Err(WaitGateCode::BadWaitId.val())
        && gb.cosign(0x999) == Err(WaitGateCode::BadWaitId.val());
    v.push(("J4-坏单号拒", bad, String::from("查账后再操作")));
    v.push(("J5-双跑一致", repro_ok(), String::from("指纹相等")));
    v.push(("J6-段位守卫", segment_guard(), String::from("0x806C0 窗口")));
    v
}

/// VX9 检查入口（卡 12627，UNX-F12628-J1 判据承载）。
pub fn run_unx12627_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F12627 NtWaitForSingleObject可警报等待→D4联签");
    for (n, p, d) in extra_checks() {
        s.add_dyn(n, p, &d);
    }
    s
}

// ── 测试区 ───────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    fn book() -> ConcTier {
        ConcTier { queue_depth: 8, concurrent: 4, max_concurrent: 4 }
    }

    #[test]
    fn test_golden_run_green() {
        assert!(golden_run().is_ok());
    }

    #[test]
    fn test_tier_valid_quadrants() {
        assert!(tier_valid(&book()));
        assert!(!tier_valid(&ConcTier { queue_depth: 8, concurrent: 5, max_concurrent: 4 }));
        assert!(!tier_valid(&ConcTier { queue_depth: 0, concurrent: 1, max_concurrent: 4 }));
        assert!(!tier_valid(&ConcTier { queue_depth: 8, concurrent: 0, max_concurrent: 4 }));
    }

    #[test]
    fn test_inject_ten_all_caught() {
        let (c, t) = inject_ten();
        assert_eq!(t, 10);
        assert_eq!(c, 10);
    }

    #[test]
    fn test_tier_parity_inject() {
        assert!(inject_tier_parity());
    }

    #[test]
    fn test_alert_first() {
        let mut g = WaitGate::new(book());
        let _ = g.wait_on(0x500, WaitObject::Event, book(), true);
        assert_eq!(g.settle(0x500, true, true), Ok(WaitStatus::Alerted));
    }

    #[test]
    fn test_non_alertable_ignores_alert() {
        let mut g = WaitGate::new(book());
        let _ = g.wait_on(0x501, WaitObject::Event, book(), false);
        assert_eq!(g.settle(0x501, true, false), Ok(WaitStatus::TimedOut));
    }

    #[test]
    fn test_dup_wait_settle_cosign() {
        let mut g = WaitGate::new(book());
        let _ = g.wait_on(0x502, WaitObject::Event, book(), false);
        assert_eq!(
            g.wait_on(0x502, WaitObject::Event, book(), false),
            Err(WaitGateCode::DupWait.val())
        );
        let _ = g.settle(0x502, false, true);
        assert_eq!(g.settle(0x502, false, true), Err(WaitGateCode::DupWait.val()));
        let _ = g.cosign(0x502);
        assert_eq!(g.cosign(0x502), Err(WaitGateCode::DupCosign.val()));
    }

    #[test]
    fn test_cosign_requires_success() {
        let mut g = WaitGate::new(book());
        let _ = g.wait_on(0x503, WaitObject::Event, book(), false);
        let _ = g.settle(0x503, false, false);
        assert_eq!(g.cosign(0x503), Err(WaitGateCode::CosignParityBreak.val()));
    }

    #[test]
    fn test_bad_wait_id() {
        let mut g = WaitGate::new(book());
        assert_eq!(g.settle(0x999, false, true), Err(WaitGateCode::BadWaitId.val()));
        assert_eq!(g.cosign(0x999), Err(WaitGateCode::BadWaitId.val()));
    }

    #[test]
    fn test_cosign_parity() {
        let mut g = WaitGate::new(book());
        let _ = g.wait_on(0x504, WaitObject::IoCompletion, book(), false);
        let _ = g.wait_on(0x505, WaitObject::Event, book(), false);
        let _ = g.settle(0x504, false, true);
        let _ = g.settle(0x505, false, true);
        let _ = g.cosign(0x504);
        assert_eq!(g.cosign_parity(), Err(WaitGateCode::CosignParityBreak.val()));
        let _ = g.cosign(0x505);
        assert!(g.cosign_parity().is_ok());
    }

    #[test]
    fn test_dual_run_equal() {
        assert!(repro_ok());
    }

    #[test]
    fn test_segment_guard() {
        assert!(segment_guard());
        assert_eq!(SEG_BASE, 0x806C0);
    }

    #[test]
    fn test_detail_closed_set() {
        for c in WaitGateCode::ALL.iter() {
            assert_eq!(detail(c.val()), c.detail());
        }
        assert!(detail(0xCAFE).contains("未知"));
    }
}
