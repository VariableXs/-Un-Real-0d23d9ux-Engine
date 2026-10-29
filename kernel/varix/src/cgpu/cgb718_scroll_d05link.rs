//! CGPU-F0718 · 滚动与 D05 验证联动（目标 370 行）
//! ==========================================================================
//! 锚点判据（清单原文摘录）：
//! - 纯功能代码 ≥300 行；② 规格与判据按册内锚点原文逐条落实
//!   （CGPU Varix STAR II · 总纲与施工书#CGPU-F0718，册内文档不在本仓，
//!    以清单判据为施工依据）；③ 完成时 API/软件写回：状态=已完成。
//! - 主题：滚动与 D05 验证联动——滚动子系统发布事件到 D05
//!   验证总线，验证端订阅/确认/否决；发布-确认账守恒，
//!   否决可升级为阻断标记。
//!
//! 设计纪律：
//! - 主题闭集：四类主题（Armed/Ticked/Latched/Torn），
//!   订阅按主题掩码过滤。
//! - 投递状态机：Pending→Acked 或 Pending→Nacked（闭集两出边），
//!   无跳边；Nacked 带否决码。
//! - 联动账守恒：sent == acked + nacked + pending（恒等式显式）。
//! - 阻断升级：同一表面累计 3 次 Torn 否决 → 阻断标记置位，
//  解除需 Admin 级 ack（本模块以权限枚举表达）。
//! - 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，
//!   确定性。
//! - 诊断段 0x4800 本单独占，错误码 SEG_BASE+1..+N 闭集。

use alloc::vec;
use alloc::vec::Vec;

/// 诊断段基址（0x4800，批内 CGPU-F0718 独占）。
pub const SEG_BASE: u16 = 0x4800;

/// 未订阅主题上的事件投递。
pub const E_NO_SUB: u16 = SEG_BASE + 1;
/// 投递已终结（重复 ack/nack）。
pub const E_TERMINAL: u16 = SEG_BASE + 2;
/// 未决投递不存在（ack/nack 未知 id）。
pub const E_NO_PEND: u16 = SEG_BASE + 3;
/// 联动账不平（sent ≠ acked+nacked+pending）。
pub const E_LEDGER: u16 = SEG_BASE + 4;
/// 权限不足（解除阻断需 Admin）。
pub const E_PERM: u16 = SEG_BASE + 5;
/// 订阅掩码非法（超出主题位宽）。
pub const E_MASK: u16 = SEG_BASE + 6;

/// 阻断阈值（同一表面 Torn 否决次数）。
pub const BLOCK_THRESHOLD: u32 = 3;
/// 待决队列定容。
pub const PEND_CAP: usize = 64;

/// 事件主题（闭集，位掩码位序）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Topic {
    /// 武装。
    Armed,
    /// 推进。
    Ticked,
    /// 锁存。
    Latched,
    /// 撕裂。
    Torn,
}

impl Topic {
    /// 位掩码位。
    pub fn bit(self) -> u8 {
        match self {
            Topic::Armed => 0,
            Topic::Ticked => 1,
            Topic::Latched => 2,
            Topic::Torn => 3,
        }
    }
}

/// 权限档。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Level {
    /// 观察者：只 ack。
    Watcher,
    /// 验证员：可 nack。
    Verifier,
    /// 管理员：可解除阻断。
    Admin,
}

/// 投递状态机。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DlvState {
    /// 待决。
    Pending,
    /// 已确认。
    Acked,
    /// 已否决。
    Nacked,
}

/// 一条 D05 投递。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Delivery {
    /// 投递 id（单调）。
    pub id: u64,
    /// 主题。
    pub topic: Topic,
    /// 表面号。
    pub surf: u16,
    /// 帧号。
    pub frame: u32,
    /// 状态。
    pub st: DlvState,
    /// 否决码（仅 Nacked 有效）。
    pub nack_code: u16,
}

/// D05 验证总线（滚动侧发布端 + 验证侧订阅端）。
///
/// 联动账守恒（显式恒等式）：
///   sent == acked + nacked + pending
///   pending ≤ PEND_CAP（定容，满则发布被拒且不入账）
#[derive(Clone, Debug)]
pub struct D05Bus {
    /// 订阅掩码（位 i = 主题 i 订阅）。
    mask: u8,
    /// 全部投递账（含已终结）。
    book: Vec<Delivery>,
    /// 下一投递 id。
    next_id: u64,
    sent: u64,
    acked: u64,
    nacked: u64,
    /// 每表面 Torn 否决计数。
    torn_votes: Vec<(u16, u32)>,
    /// 阻断标记（表面号）。
    blocked: Vec<u16>,
}

impl D05Bus {
    /// 建总线并订阅（掩码 > 0x0F 非法）。
    pub fn new(mask: u8) -> Result<Self, u16> {
        if mask > 0x0F {
            return Err(E_MASK);
        }
        Ok(Self {
            mask,
            book: Vec::new(),
            next_id: 1,
            sent: 0,
            acked: 0,
            nacked: 0,
            torn_votes: Vec::new(),
            blocked: Vec::new(),
        })
    }

    /// 发布一条事件：主题未订阅 → 拒绝（不入账）。
    pub fn publish(&mut self, t: Topic, surf: u16, frame: u32) -> Result<u64, u16> {
        if self.mask & (1 << t.bit()) == 0 {
            return Err(E_NO_SUB);
        }
        if self.pending_count() >= PEND_CAP {
            return Err(E_LEDGER);
        }
        let id = self.next_id;
        self.next_id += 1;
        self.book.push(Delivery {
            id,
            topic: t,
            surf,
            frame,
            st: DlvState::Pending,
            nack_code: 0,
        });
        self.sent += 1;
        Ok(id)
    }

    /// 确认一条待决投递。
    pub fn ack(&mut self, id: u64) -> Result<(), u16> {
        let d = match self.book.iter_mut().find(|d| d.id == id) {
            Some(d) => d,
            None => return Err(E_NO_PEND),
        };
        if d.st != DlvState::Pending {
            return Err(E_TERMINAL);
        }
        d.st = DlvState::Acked;
        self.acked += 1;
        Ok(())
    }

    /// 否决一条待决投递（Verifier 及以上）；Torn 否决累计阻断。
    pub fn nack(&mut self, id: u64, code: u16, who: Level) -> Result<(), u16> {
        if who == Level::Watcher {
            return Err(E_PERM);
        }
        let (surf, topic, was_pending) = match self.book.iter_mut().find(|d| d.id == id) {
            Some(d) => {
                if d.st != DlvState::Pending {
                    return Err(E_TERMINAL);
                }
                d.st = DlvState::Nacked;
                d.nack_code = code;
                (d.surf, d.topic, true)
            }
            None => return Err(E_NO_PEND),
        };
        let _ = was_pending;
        self.nacked += 1;
        if topic == Topic::Torn {
            let cnt = match self.torn_votes.iter_mut().find(|(s, _)| *s == surf) {
                Some((_, c)) => {
                    *c += 1;
                    *c
                }
                None => {
                    self.torn_votes.push((surf, 1));
                    1
                }
            };
            if cnt >= BLOCK_THRESHOLD && !self.blocked.contains(&surf) {
                self.blocked.push(surf);
            }
        }
        Ok(())
    }

    /// 解除阻断（仅 Admin）。
    pub fn unblock(&mut self, surf: u16, who: Level) -> Result<(), u16> {
        if who != Level::Admin {
            return Err(E_PERM);
        }
        if let Some(i) = self.blocked.iter().position(|s| *s == surf) {
            self.blocked.remove(i);
            Ok(())
        } else {
            Err(E_NO_PEND)
        }
    }

    /// 待决数。
    pub fn pending_count(&self) -> usize {
        self.book
            .iter()
            .filter(|d| d.st == DlvState::Pending)
            .count()
    }

    /// 守恒审计：sent == acked + nacked + pending。
    pub fn audit(&self) -> Result<(), u16> {
        let pend = self.pending_count() as u64;
        if self.sent != self.acked + self.nacked + pend {
            return Err(E_LEDGER);
        }
        if self.book.len() as u64 != self.sent {
            return Err(E_LEDGER);
        }
        Ok(())
    }

    /// 阻断表只读。
    pub fn blocked_view(&self) -> &[u16] {
        &self.blocked
    }

    /// 账本长度。
    pub fn book_len(&self) -> usize {
        self.book.len()
    }

    /// 订阅掩码。
    pub fn mask(&self) -> u8 {
        self.mask
    }
}

/// 投递摘要行（报告导出）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DlvSummary {
    /// 已发。
    pub sent: u64,
    /// 已确认。
    pub acked: u64,
    /// 已否决。
    pub nacked: u64,
    /// 待决。
    pub pending: u64,
}

/// 生成摘要（含守恒预检，不平返回错误）。
pub fn summarize(bus: &D05Bus) -> Result<DlvSummary, u16> {
    bus.audit()?;
    Ok(DlvSummary {
        sent: bus.sent,
        acked: bus.acked,
        nacked: bus.nacked,
        pending: bus.pending_count() as u64,
    })
}

/// 自例程：主路径（订阅→发布→ack/nack→阻断→解除）+ 错误路径。
pub fn drill() -> Result<u32, u16> {
    // 错误路径 0：掩码越界。
    if D05Bus::new(0x10).err() != Some(E_MASK) {
        return Err(E_MASK);
    }
    // 主路径：订阅全部四主题。
    let mut bus = D05Bus::new(0x0F)?;
    let id1 = bus.publish(Topic::Armed, 1, 0)?;
    let id2 = bus.publish(Topic::Ticked, 1, 0)?;
    let id3 = bus.publish(Topic::Latched, 2, 1)?;
    bus.ack(id1)?;
    bus.ack(id2)?;
    // 主路径：三次 Torn 否决同一表面 → 阻断。
    for f in 0..3u32 {
        let id = bus.publish(Topic::Torn, 7, 10 + f)?;
        bus.nack(id, 0x4801, Level::Verifier)?;
    }
    bus.audit()?;
    if bus.blocked_view() != &[7] {
        return Err(E_LEDGER);
    }
    // 错误路径 1：重复终结（对已 ack 的 id1 再 ack）。
    if bus.ack(id1) != Err(E_TERMINAL) {
        return Err(E_TERMINAL);
    }
    // 错误路径 2：未知 id。
    if bus.ack(9999) != Err(E_NO_PEND) {
        return Err(E_NO_PEND);
    }
    // 错误路径 3：Watcher 否决。
    let id4 = bus.publish(Topic::Armed, 3, 20)?;
    if bus.nack(id4, 0, Level::Watcher) != Err(E_PERM) {
        return Err(E_PERM);
    }
    bus.ack(id4)?;
    // 错误路径 4：非 Admin 解除阻断。
    if bus.unblock(7, Level::Verifier) != Err(E_PERM) {
        return Err(E_PERM);
    }
    // Admin 解除。
    bus.unblock(7, Level::Admin)?;
    if !bus.blocked_view().is_empty() {
        return Err(E_LEDGER);
    }
    // 错误路径 5：未订阅主题。
    let mut bus2 = D05Bus::new(0x01)?; // 只订阅 Armed
    if bus2.publish(Topic::Torn, 1, 0) != Err(E_NO_SUB) {
        return Err(E_NO_SUB);
    }
    bus2.publish(Topic::Armed, 1, 0)?;
    bus2.audit()?;
    if bus2.mask() != 0x01 || bus2.book_len() != 1 {
        return Err(E_LEDGER);
    }
    // 摘要守恒。
    let s = summarize(&bus)?;
    if s.sent != s.acked + s.nacked + s.pending {
        return Err(E_LEDGER);
    }
    if s.sent != 7 || s.nacked != 3 {
        return Err(E_LEDGER);
    }
    Ok(0)
}
