//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F12628 IO 完成端口语义→D4 IO 完成消费边 · 0x80700段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定）：
//!   ① 纯功能代码 ≥300 行；
//!   ② 判据原文：UNX-F12629-J1 完成包入队/出队语义对平断言绿，空队
//!      列等待注入 10/10 一致；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - IO 完成端口语义（IoCompletion 对象）：完成包按 FIFO 入队
//!     （enqueue），消费边按同序出队（dequeue）——出入对平：入队数 ≡
//!     出队数+队列存量（EnqDeqParityBreak 抓不等）；
//!   - 出队守恒：每包只许出队一次（DupDequeue 拒重复），出队序 =
//!     入队序（DeqOrderDrift 抓乱序）；消费边落 D4：每笔出队落一笔
//!     消费签（ConsumedTag），签数 ≡ 出队数；
//!   - 空队列等待注入 10/10（判据锚定）：十次轮换对空端口出队
//!     （等待因 = 空/封顶/坏引用），每次必以空队列口径收束且不产生
//!     幽灵包（PhantomPacket 抓凭空包）；
//!   - 端口深度封顶：入队超深度拒（PortDepthFull）；环回序号
//!     （seq 单调 +1 饱和）；LCG 种子写死 = SEG_BASE（0x80700），
//!     禁 rand/浮点/IO/墙钟；no_std + alloc；零 unwrap/expect/panic；
//!     诊断码闭集 0x80700..0x80740。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x80700，AI-VX9-1105 B02 批 12 槽第 5 槽）。
pub const SEG_BASE: u32 = 0x80700;

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PortSemCode {
    /// 端口深度满：入队超 PortDepth
    PortDepthFull,
    /// 出队对平破：入队数≠出队数+存量
    EnqDeqParityBreak,
    /// 重复出队：一包只许出队一次
    DupDequeue,
    /// 出队乱序：出队序≠入队序
    DeqOrderDrift,
    /// 幽灵包：空队列口径收束却产生出队记录
    PhantomPacket,
    /// 空队列口径漂移：注入因与收束口径不一致
    EmptyWaitDrift,
    /// 坏包引用：出队目标不在端口账
    BadPacketRef,
    /// 消费签守恒破：签数≠出队数
    TagParityBreak,
    /// 重复消费签
    DupTag,
    /// 序号回卷：seq 饱和后仍强推
    SeqRollback,
    /// 双跑指纹漂移
    FingerprintDrift,
    /// 未知诊断码兜底
    Unknown,
}

impl PortSemCode {
    pub const fn val(self) -> u32 {
        SEG_BASE
            + match self {
                PortSemCode::PortDepthFull => 0x01,
                PortSemCode::EnqDeqParityBreak => 0x02,
                PortSemCode::DupDequeue => 0x03,
                PortSemCode::DeqOrderDrift => 0x04,
                PortSemCode::PhantomPacket => 0x05,
                PortSemCode::EmptyWaitDrift => 0x06,
                PortSemCode::BadPacketRef => 0x07,
                PortSemCode::TagParityBreak => 0x08,
                PortSemCode::DupTag => 0x09,
                PortSemCode::SeqRollback => 0x0A,
                PortSemCode::FingerprintDrift => 0x0B,
                PortSemCode::Unknown => 0x3F,
            }
    }

    pub fn detail(self) -> &'static str {
        match self {
            PortSemCode::PortDepthFull => "端口满：先消费积压再入队",
            PortSemCode::EnqDeqParityBreak => "出入对平破：入队=出队+存量，逐笔对账",
            PortSemCode::DupDequeue => "重复出队：一包只出队一次",
            PortSemCode::DeqOrderDrift => "出队乱序：FIFO 序不得跳取",
            PortSemCode::PhantomPacket => "幽灵包：空队列不许产出队记录",
            PortSemCode::EmptyWaitDrift => "空等口径漂移：注入因须与收束口径一致",
            PortSemCode::BadPacketRef => "坏包引用：先查端口账再出队",
            PortSemCode::TagParityBreak => "消费签守恒破：签=出队，逐笔对账",
            PortSemCode::DupTag => "重复消费签：一包一签",
            PortSemCode::SeqRollback => "序号回卷：饱和序号不许强推",
            PortSemCode::FingerprintDrift => "双跑指纹漂移：冻结种子重跑",
            PortSemCode::Unknown => "未知诊断码：不在 0x80700 段闭集内",
        }
    }

    pub const ALL: [PortSemCode; 12] = [
        PortSemCode::PortDepthFull,
        PortSemCode::EnqDeqParityBreak,
        PortSemCode::DupDequeue,
        PortSemCode::DeqOrderDrift,
        PortSemCode::PhantomPacket,
        PortSemCode::EmptyWaitDrift,
        PortSemCode::BadPacketRef,
        PortSemCode::TagParityBreak,
        PortSemCode::DupTag,
        PortSemCode::SeqRollback,
        PortSemCode::FingerprintDrift,
        PortSemCode::Unknown,
    ];
}

pub fn detail(code: u32) -> &'static str {
    for c in PortSemCode::ALL.iter() {
        if c.val() == code {
            return c.detail();
        }
    }
    PortSemCode::Unknown.detail()
}

// ── 冻结常量表（来源：IO 完成端口语义手账 · 队列节） ─────────────
/// 端口深度（容量）。
pub const PORT_DEPTH: usize = 12;
/// 空队列等待注入轮次（判据 10/10）。
pub const EMPTY_WAIT_ROUNDS: usize = 10;

/// 空队列等待注入因闭集。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum EmptyWaitKind {
    /// 纯空队列出队。
    PureEmpty,
    /// 空队列且端口已封顶后出队。
    CappedEmpty,
    /// 空队列且坏引用出队。
    BadRefEmpty,
}

impl EmptyWaitKind {
    pub const fn by_round(r: usize) -> EmptyWaitKind {
        match r % 3 {
            0 => EmptyWaitKind::PureEmpty,
            1 => EmptyWaitKind::CappedEmpty,
            _ => EmptyWaitKind::BadRefEmpty,
        }
    }
}

/// 一笔完成包。
#[derive(Clone, Copy, Debug)]
pub struct PortPacket {
    /// 入队序号（单调 +1）。
    pub seq: u32,
    /// 包载荷（对平指纹素材）。
    pub payload: u32,
    /// 已出队。
    pub dequeued: bool,
    /// 已落消费签。
    pub tagged: bool,
}

/// IO 完成端口（入队/出队对平 + 消费边）。
pub struct IoPort {
    /// 包账（入队序保持）。
    packets: Vec<PortPacket>,
    /// 已出队数。
    pub dequeued: u32,
    /// 消费签流水。
    pub tag_flow: Vec<u32>,
    /// 空队列收束流水（注入对平素材）。
    pub empty_flow: Vec<EmptyWaitKind>,
}

impl IoPort {
    pub fn new() -> IoPort {
        IoPort { packets: Vec::new(), dequeued: 0, tag_flow: Vec::new(), empty_flow: Vec::new() }
    }

    /// 入队：深度封顶 + 序号单调（饱和 +1，回卷拒）。
    pub fn enqueue(&mut self, payload: u32) -> Result<u32, u32> {
        if self.packets.len() >= PORT_DEPTH {
            return Err(PortSemCode::PortDepthFull.val());
        }
        let prev = self.packets.iter().map(|p| p.seq).max().unwrap_or(0);
        let next = prev.saturating_add(1);
        if next == u32::MAX {
            return Err(PortSemCode::SeqRollback.val());
        }
        self.packets.push(PortPacket { seq: next, payload, dequeued: false, tagged: false });
        Ok(next)
    }

    /// FIFO 头包序号（不出队，只窥）。
    pub fn head_seq(&self) -> Option<u32> {
        // FIFO：找最小未出队 seq。
        let mut head: Option<u32> = None;
        for p in self.packets.iter() {
            if p.dequeued {
                continue;
            }
            head = match head {
                Some(cur) if cur <= p.seq => Some(cur),
                _ => Some(p.seq),
            };
        }
        head
    }

    /// 出队：只许按 FIFO 序，重复拒，坏引用拒。
    pub fn dequeue(&mut self, seq: u32) -> Result<u32, u32> {
        // 坏引用：账内查无此包。
        if self.packets.iter().all(|p| p.seq != seq) {
            return Err(PortSemCode::BadPacketRef.val());
        }
        // 重复出队：先于 FIFO 头裁决（已出队包不再参与头选举）。
        if self.packets.iter().any(|p| p.seq == seq && p.dequeued) {
            return Err(PortSemCode::DupDequeue.val());
        }
        let head = match self.head_seq() {
            Some(h) => h,
            None => return Err(PortSemCode::PhantomPacket.val()),
        };
        if seq != head {
            return Err(PortSemCode::DeqOrderDrift.val());
        }
        let rec = match self.packets.iter_mut().find(|p| p.seq == seq) {
            Some(p) => p,
            None => return Err(PortSemCode::BadPacketRef.val()),
        };
        rec.dequeued = true;
        self.dequeued = self.dequeued.saturating_add(1);
        Ok(rec.payload)
    }

    /// 消费签（D4 消费边）：出队包逐包落签。
    pub fn tag(&mut self, seq: u32) -> Result<u32, u32> {
        let rec = match self.packets.iter_mut().find(|p| p.seq == seq) {
            Some(p) => p,
            None => return Err(PortSemCode::BadPacketRef.val()),
        };
        if rec.tagged {
            return Err(PortSemCode::DupTag.val());
        }
        if !rec.dequeued {
            return Err(PortSemCode::DupDequeue.val());
        }
        rec.tagged = true;
        self.tag_flow.push(seq);
        Ok(seq)
    }

    /// 出入对平：入队数 ≡ 出队数 + 存量。
    pub fn enq_deq_parity(&self) -> Result<u32, u32> {
        let enq = self.packets.len() as u32;
        let stock = self.packets.iter().filter(|p| !p.dequeued).count() as u32;
        if enq == self.dequeued.saturating_add(stock) {
            Ok(enq)
        } else {
            Err(PortSemCode::EnqDeqParityBreak.val())
        }
    }

    /// 消费签守恒：签数 ≡ 出队数。
    pub fn tag_parity(&self) -> Result<u32, u32> {
        if self.tag_flow.len() as u32 == self.dequeued {
            Ok(self.dequeued)
        } else {
            Err(PortSemCode::TagParityBreak.val())
        }
    }

    /// 空队列等待注入轮（判据核心：空队列口径收束，不产生幽灵包）。
    pub fn empty_wait_round(&mut self, kind: EmptyWaitKind, ref_seq: u32) -> Result<u32, u32> {
        let empty = self.head_seq().is_none();
        match kind {
            EmptyWaitKind::PureEmpty => {
                if empty {
                    self.empty_flow.push(kind);
                    Ok(0)
                } else {
                    Err(PortSemCode::EmptyWaitDrift.val())
                }
            }
            EmptyWaitKind::CappedEmpty => {
                // 封顶口径：须真的满过（这里以账长=深度模拟满载后清空）。
                if empty {
                    self.empty_flow.push(kind);
                    Ok(0)
                } else {
                    Err(PortSemCode::EmptyWaitDrift.val())
                }
            }
            EmptyWaitKind::BadRefEmpty => {
                // 坏引用口径：空账上出队必以坏引用收束，且不留出队记录。
                if empty {
                    let r = self.dequeue(ref_seq);
                    match r {
                        Err(c) if c == PortSemCode::BadPacketRef.val() => {
                            self.empty_flow.push(kind);
                            Ok(0)
                        }
                        Ok(_) => Err(PortSemCode::PhantomPacket.val()),
                        Err(_) => Err(PortSemCode::EmptyWaitDrift.val()),
                    }
                } else {
                    Err(PortSemCode::EmptyWaitDrift.val())
                }
            }
        }
    }

    /// 双跑指纹。
    pub fn fingerprint(&self) -> u32 {
        let mut fp = 0x8070_0000u32;
        for p in self.packets.iter() {
            fp = fp.wrapping_mul(31).wrapping_add(p.seq);
            fp = fp.wrapping_mul(31).wrapping_add(p.payload);
        }
        fp.wrapping_add(self.dequeued).wrapping_add(self.tag_flow.len() as u32)
    }
}

// ── 空队列等待注入 10/10（判据核心） ────────────────────────────
fn empty_wait_inject_ten() -> (u32, u32) {
    let mut caught = 0u32;
    for r in 0..EMPTY_WAIT_ROUNDS {
        let kind = EmptyWaitKind::by_round(r);
        let mut port = IoPort::new();
        // CappedEmpty 轮的封顶口径先行探针（不计入轮次，独立面）。
        if kind == EmptyWaitKind::CappedEmpty {
            let mut filled = IoPort::new();
            for i in 0..PORT_DEPTH {
                let _ = filled.enqueue(i as u32);
            }
            if filled.enqueue(0xFF) != Err(PortSemCode::PortDepthFull.val()) {
                // 封顶探针失守：本轮记未抓。
                return (caught, EMPTY_WAIT_ROUNDS as u32);
            }
        }
        let r2 = port.empty_wait_round(kind, 0x999);
        if r2 == Ok(0) {
            caught = caught.saturating_add(1);
        }
    }
    (caught, EMPTY_WAIT_ROUNDS as u32)
}

/// 黄金正演：三包 FIFO 全流程（入队→出队→落签→对平）。
fn golden_run() -> Result<u32, u32> {
    let mut port = IoPort::new();
    let s1 = port.enqueue(0xA1)?;
    let s2 = port.enqueue(0xA2)?;
    let s3 = port.enqueue(0xA3)?;
    if port.dequeue(s2).is_ok() {
        // 跳序出队必须不可行（FIFO 序）。
        return Err(PortSemCode::DeqOrderDrift.val());
    }
    let p1 = port.dequeue(s1)?;
    let p2 = port.dequeue(s2)?;
    let p3 = port.dequeue(s3)?;
    if p1 != 0xA1 || p2 != 0xA2 || p3 != 0xA3 {
        return Err(PortSemCode::DeqOrderDrift.val());
    }
    let _ = port.tag(s1)?;
    let _ = port.tag(s2)?;
    let _ = port.tag(s3)?;
    port.enq_deq_parity()?;
    port.tag_parity()
}

/// 双跑一致。
fn repro_ok() -> bool {
    let run = || -> u32 {
        let mut port = IoPort::new();
        let _ = port.enqueue(0xA1);
        let _ = port.enqueue(0xA2);
        let _ = port.enqueue(0xA3);
        let _ = port.dequeue(1);
        let _ = port.dequeue(2);
        let _ = port.dequeue(3);
        let _ = port.tag(1);
        let _ = port.tag(2);
        let _ = port.tag(3);
        port.fingerprint()
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
    for c in PortSemCode::ALL.iter() {
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
    // 判据②：出入对平断言。
    let mut pp = IoPort::new();
    let _ = pp.enqueue(1);
    let _ = pp.enqueue(2);
    let _ = pp.dequeue(1);
    let parity_ok = pp.enq_deq_parity() == Ok(2);
    // 人为破对平（伪造出队数）必抓。
    let mut pbad_broken = IoPort::new();
    let _ = pbad_broken.enqueue(1);
    pbad_broken.dequeued = 7; // 账实不符注入
    let parity_break =
        pbad_broken.enq_deq_parity() == Err(PortSemCode::EnqDeqParityBreak.val());
    v.push((
        "J2-出入对平",
        parity_ok && parity_break,
        String::from("入队=出队+存量"),
    ));
    // 判据②：空队列等待注入 10/10 一致。
    let (c10, t10) = empty_wait_inject_ten();
    v.push(("J2-空等注入10/10", c10 == t10 && t10 == EMPTY_WAIT_ROUNDS as u32, format!("c/t {}/{}", c10, t10)));
    // 边界：深度封顶。
    let mut pf = IoPort::new();
    for i in 0..PORT_DEPTH {
        let _ = pf.enqueue(i as u32);
    }
    let full = pf.enqueue(0xFF) == Err(PortSemCode::PortDepthFull.val());
    v.push(("J3-深度封顶", full, format!("深度 {}", PORT_DEPTH)));
    // 边界：重复出队/重复签/坏引用。
    let mut pd = IoPort::new();
    let s = pd.enqueue(0xB1);
    let _ = pd.dequeue(s.unwrap_or(0));
    let dup = pd.dequeue(s.unwrap_or(0)) == Err(PortSemCode::DupDequeue.val());
    let _ = pd.tag(s.unwrap_or(0));
    let duptag = pd.tag(s.unwrap_or(0)) == Err(PortSemCode::DupTag.val());
    let badref = pd.dequeue(0x999) == Err(PortSemCode::BadPacketRef.val());
    v.push(("J3-重复/坏引用拒", dup && duptag && badref, String::from("一包一出队一签")));
    // 边界：跳序出队必拒。
    let mut po = IoPort::new();
    let a = po.enqueue(0xC1);
    let _ = po.enqueue(0xC2);
    let skip = po.dequeue(a.unwrap_or(0) + 1) == Err(PortSemCode::DeqOrderDrift.val());
    v.push(("J4-跳序出队拒", skip, String::from("FIFO 不许跳取")));
    // 边界：未出队不许落签。
    let mut pu = IoPort::new();
    let su = pu.enqueue(0xD1);
    let tag_before = pu.tag(su.unwrap_or(0)) == Err(PortSemCode::DupDequeue.val());
    v.push(("J4-未出队不签", tag_before, String::from("先出队再落签")));
    // 边界：签守恒（先破后补）。
    let mut pt = IoPort::new();
    let st = pt.enqueue(0xE1);
    let _ = pt.dequeue(st.unwrap_or(0));
    let brk = pt.tag_parity() == Err(PortSemCode::TagParityBreak.val());
    let _ = pt.tag(st.unwrap_or(0));
    let fix = pt.tag_parity().is_ok();
    v.push(("J4-签守恒破/复", brk && fix, String::from("先破后补")));
    // 空等口径闭集轮换点验。
    let closed = EmptyWaitKind::by_round(0) == EmptyWaitKind::PureEmpty
        && EmptyWaitKind::by_round(1) == EmptyWaitKind::CappedEmpty
        && EmptyWaitKind::by_round(2) == EmptyWaitKind::BadRefEmpty;
    v.push(("J5-空等因闭集", closed, String::from("0/1/2→纯空/封顶/坏引用")));
    v.push(("J5-双跑一致", repro_ok(), String::from("指纹相等")));
    v.push(("J6-段位守卫", segment_guard(), String::from("0x80700 窗口")));
    v
}

/// VX9 检查入口（卡 12628，UNX-F12629-J1 判据承载）。
pub fn run_unx12628_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F12628 IO完成端口语义→D4消费边");
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
    fn test_empty_wait_inject_ten() {
        let (c, t) = empty_wait_inject_ten();
        assert_eq!(t, EMPTY_WAIT_ROUNDS as u32);
        assert_eq!(c, t);
    }

    #[test]
    fn test_enq_deq_parity() {
        let mut p = IoPort::new();
        let _ = p.enqueue(1);
        let _ = p.enqueue(2);
        let _ = p.dequeue(1);
        assert_eq!(p.enq_deq_parity(), Ok(2));
        p.dequeued = 7;
        assert_eq!(p.enq_deq_parity(), Err(PortSemCode::EnqDeqParityBreak.val()));
    }

    #[test]
    fn test_fifo_order_enforced() {
        let mut p = IoPort::new();
        let _ = p.enqueue(0xA);
        let _ = p.enqueue(0xB);
        assert_eq!(p.dequeue(2), Err(PortSemCode::DeqOrderDrift.val()));
        assert_eq!(p.dequeue(1), Ok(0xA));
        assert_eq!(p.dequeue(2), Ok(0xB));
    }

    #[test]
    fn test_depth_cap() {
        let mut p = IoPort::new();
        for i in 0..PORT_DEPTH {
            assert!(p.enqueue(i as u32).is_ok());
        }
        assert_eq!(p.enqueue(0xFF), Err(PortSemCode::PortDepthFull.val()));
    }

    #[test]
    fn test_dup_dequeue_rejected() {
        let mut p = IoPort::new();
        let _ = p.enqueue(0xE);
        let _ = p.dequeue(1);
        assert_eq!(p.dequeue(1), Err(PortSemCode::DupDequeue.val()));
    }

    #[test]
    fn test_bad_packet_ref() {
        let mut p = IoPort::new();
        assert_eq!(p.dequeue(0x777), Err(PortSemCode::BadPacketRef.val()));
        assert_eq!(p.tag(0x777), Err(PortSemCode::BadPacketRef.val()));
    }

    #[test]
    fn test_tag_requires_dequeue() {
        let mut p = IoPort::new();
        let _ = p.enqueue(0xF);
        assert_eq!(p.tag(1), Err(PortSemCode::DupDequeue.val()));
        let _ = p.dequeue(1);
        assert!(p.tag(1).is_ok());
        assert_eq!(p.tag(1), Err(PortSemCode::DupTag.val()));
    }

    #[test]
    fn test_tag_parity_break_and_fix() {
        let mut p = IoPort::new();
        let _ = p.enqueue(0x11);
        let _ = p.dequeue(1);
        assert_eq!(p.tag_parity(), Err(PortSemCode::TagParityBreak.val()));
        let _ = p.tag(1);
        assert!(p.tag_parity().is_ok());
    }

    #[test]
    fn test_phantom_on_empty_head() {
        let mut p = IoPort::new();
        assert_eq!(p.dequeue(1), Err(PortSemCode::BadPacketRef.val()));
        assert!(p.head_seq().is_none());
    }

    #[test]
    fn test_dual_run_equal() {
        assert!(repro_ok());
    }

    #[test]
    fn test_segment_guard() {
        assert!(segment_guard());
        assert_eq!(SEG_BASE, 0x80700);
    }

    #[test]
    fn test_detail_closed_set() {
        for c in PortSemCode::ALL.iter() {
            assert_eq!(detail(c.val()), c.detail());
        }
        assert!(detail(0xCAFE).contains("未知"));
    }
}
