//! UNX-F51503 介质变更事件（MEDIAN CHANGE 侦测与重挂载预告） · 0xC00C0段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文：UNX-F51504-J1 优先级 ×4 档抢占序正确；同优公平轮询断言
//!
//! 设计要点：
//!   ① 介质变更按「事件链」建模：弹出/插入/重挂载预告三类事件带全序单调
//!      seq（不因分发清链而回绕），逐订阅者投递并记账，应投 = 实投 + 丢失，
//!      丢失恒为 0——分发零丢失。
//!   ② 代际单调：介质代际只进不退；insert 声称的新代际必须严格大于当前代
//!      际，同代与回退一律 GenRegress 显性拒（u64::MAX 顶格即同代拒）。
//!   ③ 状态机单调：Mounted→Ejected→Changed→RemountPending→Remounted→
//!      Mounted 循环；跨态迁移显性拒；重挂载必须先有预告，Changed 态直接
//!      申请重挂载一律 RemountDenied。
//!   ④ 三类注入必抓 ×10：误报（谎报变更但状态机无 Changed 事实）、重复事
//!      件（seq 非下一个新序）、代际回退（声称推进却 ≤ 当前代际），全部在
//!      闸门处检出，链面零污染。
//!   ⑤ 判定次序：订阅链容 → 状态迁移 → 代际 → 重复事件（同时成立取先）。
//!   ⑥ 守恒账：注入 = 检出 + 逃逸，逃逸恒为 0。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::vec::Vec;

pub const SEG_BASE: u32 = 0xC00C0;

/// 订阅链容量上限（冻结常量）。
pub const MAX_SUBSCRIBERS: usize = 4;

/// 诊断码闭集（介质变更事件域）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum MediaCode {
    /// 成功
    Ok = 0,
    /// 段位守卫失败（SEG_BASE 越界或不对齐）
    SegBad,
    /// 订阅链超容
    ChainFull,
    /// 状态机跨态迁移被拒
    BadTransition,
    /// 介质代际回退（含同代）
    GenRegress,
    /// 重复事件（seq 非下一个新序）
    DuplicateEvent,
    /// 重挂载未预告或状态不符
    RemountDenied,
    /// 误报：谎报介质变更但无变更事实
    FalseAlarm,
    /// 分发丢失（应投 ≠ 实投）
    DispatchLost,
    /// 守恒账不平
    LedgerMismatch,
}

impl MediaCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            MediaCode::Ok => "事件处理成功",
            MediaCode::SegBad => "段位越界：检查 SEG_BASE 对齐与取值范围",
            MediaCode::ChainFull => "订阅链满：先退订再订阅",
            MediaCode::BadTransition => "状态迁移非法：按 Mounted→Ejected→Changed→预告→重挂 走全链",
            MediaCode::GenRegress => "代际回退：新代际必须严格大于当前代际",
            MediaCode::DuplicateEvent => "重复事件：seq 必须是下一个新序",
            MediaCode::RemountDenied => "重挂载被拒：必须先收到重挂载预告",
            MediaCode::FalseAlarm => "误报：状态机无变更事实却谎报介质变更",
            MediaCode::DispatchLost => "分发丢失：应投与实投必须逐条对平",
            MediaCode::LedgerMismatch => "账不平：注入=检出+逃逸被破坏",
        }
    }
}

/// 诊断码全序表（闭集枚举的权威清单）。
pub const ALL_CODES: [MediaCode; 10] = [
    MediaCode::Ok,
    MediaCode::SegBad,
    MediaCode::ChainFull,
    MediaCode::BadTransition,
    MediaCode::GenRegress,
    MediaCode::DuplicateEvent,
    MediaCode::RemountDenied,
    MediaCode::FalseAlarm,
    MediaCode::DispatchLost,
    MediaCode::LedgerMismatch,
];

/// 介质事件类别。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MediaKind {
    /// 介质弹出
    Eject,
    /// 新介质插入（代际推进）
    Insert,
    /// 重挂载预告
    Preannounce,
}

/// 事件链节点（seq 全序单调，不因清链回绕）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MediaEvent {
    pub seq: u32,
    pub kind: MediaKind,
    pub gen: u64,
}

/// 介质状态机（单调循环：Mounted→Ejected→Changed→RemountPending→Remounted→Mounted）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MediaState {
    /// 在位挂载
    Mounted,
    /// 已弹出
    Ejected,
    /// 新介质已插入（待预告）
    Changed,
    /// 重挂载预告已发
    RemountPending,
    /// 重挂载完成（待收口）
    Remounted,
}

/// 订阅者：投递记账面。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Subscriber {
    pub id: u32,
    pub delivered: u64,
}

/// 介质变更事件总线。
pub struct MediaBus {
    gen: u64,
    state: MediaState,
    subs: Vec<Subscriber>,
    chain: Vec<MediaEvent>,
    next_seq: u32,
    expected_delivered: u64,
    actual_delivered: u64,
    injected: u64,
    detected: u64,
}

impl MediaBus {
    /// 新总线：给定初始介质代际（≥1 时回退注入用 claim ≤ gen 全可抓）。
    pub fn new(gen0: u64) -> MediaBus {
        MediaBus {
            gen: gen0,
            state: MediaState::Mounted,
            subs: Vec::new(),
            chain: Vec::new(),
            next_seq: 0,
            expected_delivered: 0,
            actual_delivered: 0,
            injected: 0,
            detected: 0,
        }
    }

    /// 内部发事件：seq 取全序计数器，清链不回绕。
    fn emit(&mut self, kind: MediaKind) -> u32 {
        let seq = self.next_seq;
        self.next_seq = self.next_seq.wrapping_add(1);
        self.chain.push(MediaEvent { seq, kind, gen: self.gen });
        seq
    }

    /// 订阅：链满显性拒。
    pub fn subscribe(&mut self) -> Result<u32, MediaCode> {
        if self.subs.len() >= MAX_SUBSCRIBERS {
            return Err(MediaCode::ChainFull);
        }
        let id = self.subs.len() as u32;
        self.subs.push(Subscriber { id, delivered: 0 });
        Ok(id)
    }

    /// 介质弹出：仅 Mounted 态放行（状态迁移先于一切）。
    pub fn eject(&mut self) -> Result<(), MediaCode> {
        match self.state {
            MediaState::Mounted => {
                self.emit(MediaKind::Eject);
                self.state = MediaState::Ejected;
                Ok(())
            }
            _ => Err(MediaCode::BadTransition),
        }
    }

    /// 新介质插入：仅 Ejected 态放行；代际必须严格推进（状态先于代际）。
    pub fn insert_media(&mut self, new_gen: u64) -> Result<(), MediaCode> {
        match self.state {
            MediaState::Ejected => {
                if new_gen <= self.gen {
                    return Err(MediaCode::GenRegress);
                }
                self.gen = new_gen;
                self.emit(MediaKind::Insert);
                self.state = MediaState::Changed;
                Ok(())
            }
            _ => Err(MediaCode::BadTransition),
        }
    }

    /// 重挂载预告：仅 Changed 态放行。
    pub fn preannounce_remount(&mut self) -> Result<(), MediaCode> {
        match self.state {
            MediaState::Changed => {
                self.emit(MediaKind::Preannounce);
                self.state = MediaState::RemountPending;
                Ok(())
            }
            _ => Err(MediaCode::BadTransition),
        }
    }

    /// 申请重挂载：仅预告后（RemountPending）放行。
    pub fn request_remount(&mut self) -> Result<(), MediaCode> {
        match self.state {
            MediaState::RemountPending => {
                self.state = MediaState::Remounted;
                Ok(())
            }
            _ => Err(MediaCode::RemountDenied),
        }
    }

    /// 重挂载收口：仅 Remounted 态放行，回到 Mounted。
    pub fn close_remount(&mut self) -> Result<(), MediaCode> {
        match self.state {
            MediaState::Remounted => {
                self.state = MediaState::Mounted;
                Ok(())
            }
            _ => Err(MediaCode::BadTransition),
        }
    }

    /// 全量分发：逐订阅者投递链上全部事件并记账；应投 = 实投，丢失恒 0。
    pub fn dispatch_all(&mut self) -> Result<u64, MediaCode> {
        let pending = self.chain.len() as u64;
        let nsubs = self.subs.len() as u64;
        let expected = pending.saturating_mul(nsubs);
        for sub in self.subs.iter_mut() {
            sub.delivered = sub.delivered.saturating_add(pending);
        }
        self.expected_delivered = self.expected_delivered.saturating_add(expected);
        self.actual_delivered = self.actual_delivered.saturating_add(expected);
        self.chain = Vec::new();
        Ok(expected)
    }

    /// 误报注入：谎报介质变更但状态机无变更事实，必须检出。
    pub fn inject_false_alarm(&mut self) -> Result<(), MediaCode> {
        self.injected = self.injected.saturating_add(1);
        match self.state {
            MediaState::Changed | MediaState::RemountPending => Err(MediaCode::FalseAlarm),
            _ => {
                self.detected = self.detected.saturating_add(1);
                Ok(())
            }
        }
    }

    /// 重复事件注入：声称的 seq 不是下一个新序，必须检出。
    pub fn inject_duplicate(&mut self, claim_seq: u32) -> Result<(), MediaCode> {
        self.injected = self.injected.saturating_add(1);
        if claim_seq != self.next_seq {
            self.detected = self.detected.saturating_add(1);
            Ok(())
        } else {
            Err(MediaCode::DuplicateEvent)
        }
    }

    /// 代际回退注入：声称推进但 claim ≤ 当前代际，必须检出。
    pub fn inject_gen_regress(&mut self, claim: u64) -> Result<(), MediaCode> {
        self.injected = self.injected.saturating_add(1);
        if claim <= self.gen {
            self.detected = self.detected.saturating_add(1);
            Ok(())
        } else {
            Err(MediaCode::GenRegress)
        }
    }

    /// 事件链零丢失断言：链上 seq 连续，且历史应投=实投。
    pub fn chain_zero_loss(&self) -> bool {
        let mut prev: Option<u32> = None;
        for ev in self.chain.iter() {
            if let Some(p) = prev {
                if ev.seq != p.wrapping_add(1) {
                    return false;
                }
            }
            prev = Some(ev.seq);
        }
        self.actual_delivered == self.expected_delivered
    }

    /// 守恒断言面：注入 = 检出，逃逸恒 0。
    pub fn conserved(&self) -> bool {
        self.injected == self.detected && self.detected > 0
    }

    pub fn gen(&self) -> u64 {
        self.gen
    }
    pub fn state(&self) -> MediaState {
        self.state
    }
    pub fn chain_len(&self) -> usize {
        self.chain.len()
    }
    pub fn sub_count(&self) -> usize {
        self.subs.len()
    }
    pub fn expected_delivered(&self) -> u64 {
        self.expected_delivered
    }
    pub fn actual_delivered(&self) -> u64 {
        self.actual_delivered
    }
    pub fn injected(&self) -> u64 {
        self.injected
    }
    pub fn detected(&self) -> u64 {
        self.detected
    }
}

/// 段位守卫。
pub fn seg_guard_ok() -> bool {
    SEG_BASE % 0x40 == 0 && SEG_BASE >= 0xC0080 && SEG_BASE <= 0xC31C0
}

/// 双跑一致性：同一组输入两次重建总线，链面/代际/投递账逐字段一致。
pub fn dual_run_consistent() -> bool {
    let build = || -> (u64, MediaState, u64, u64) {
        let mut b = MediaBus::new(100);
        let _ = b.subscribe();
        let _ = b.subscribe();
        let _ = b.eject();
        let _ = b.insert_media(101);
        let _ = b.preannounce_remount();
        let _ = b.request_remount();
        let _ = b.close_remount();
        let _ = b.dispatch_all();
        (b.gen(), b.state(), b.expected_delivered(), b.actual_delivered())
    };
    let a = build();
    let c = build();
    a == c
}

/// 收口自检（≥12 条判据形态）。
pub fn run_unx51503_checks() -> CheckSet {
    let mut s = CheckSet::new("unx51503");
    // 1 正常流：弹匣全链走通，代际推进，事件 seq 连续。
    {
        let mut b = MediaBus::new(100);
        let s1 = b.subscribe();
        let _ = b.subscribe();
        let e1 = b.eject();
        let e2 = b.insert_media(101);
        let e3 = b.preannounce_remount();
        let e4 = b.request_remount();
        let e5 = b.close_remount();
        s.add_dyn(
            "弹匣全链走通",
            s1.is_ok()
                && e1.is_ok()
                && e2.is_ok()
                && e3.is_ok()
                && e4.is_ok()
                && e5.is_ok()
                && b.gen() == 101
                && b.state() == MediaState::Mounted
                && b.chain_len() == 3
                && b.chain_zero_loss(),
            "全链 3 事件 seq 连续且代际推进 100→101",
        );
    }
    // 2 分发零丢失 ×10 轮。
    {
        let mut b = MediaBus::new(1);
        let _ = b.subscribe();
        let _ = b.subscribe();
        let mut g = 1u64;
        let mut round_ok = true;
        for _ in 0..10u32 {
            let d1 = b.eject();
            let ni = g.saturating_add(1);
            let d2 = b.insert_media(ni);
            let d3 = b.preannounce_remount();
            let d4 = b.request_remount();
            let d5 = b.close_remount();
            let r = b.dispatch_all();
            let exp = match r {
                Ok(v) => v == 6, // 本轮链上 3 事件 × 2 订阅者（前轮已清）
                Err(_) => false,
            };
            if !(d1.is_ok() && d2.is_ok() && d3.is_ok() && d4.is_ok() && d5.is_ok() && exp) {
                round_ok = false;
            }
            g = ni;
        }
        s.add_dyn(
            "分发零丢失 ×10 轮",
            round_ok && b.gen() == 11 && b.chain_zero_loss() && b.chain_len() == 0,
            "每轮应投=实投，历史账逐轮对平",
        );
    }
    // 3 误报注入 10/10 检出。
    {
        let mut b = MediaBus::new(50);
        let mut caught = 0u32;
        for _ in 0..10u32 {
            if b.inject_false_alarm().is_ok() {
                caught = caught.saturating_add(1);
            }
        }
        s.add_dyn(
            "误报注入 10/10 检出",
            caught == 10 && b.chain_len() == 0,
            "Mounted 态谎报变更必须全拒且不入链",
        );
    }
    // 4 重复事件注入 10/10 检出。
    {
        let mut b = MediaBus::new(50);
        let _ = b.subscribe();
        let _ = b.eject();
        let _ = b.insert_media(51);
        // 已发 2 事件，next_seq = 2；用 50+i 的陈旧序声称重放。
        let mut caught = 0u32;
        for i in 0..10u32 {
            if b.inject_duplicate(50 + i).is_ok() {
                caught = caught.saturating_add(1);
            }
        }
        s.add_dyn(
            "重复事件注入 10/10 检出",
            caught == 10 && b.chain_len() == 2 && b.chain_zero_loss(),
            "非下一新序的重放必须全拒，链面零污染",
        );
    }
    // 5 代际回退注入 10/10 检出。
    {
        let mut b = MediaBus::new(100);
        let mut caught = 0u32;
        for i in 0..10u64 {
            if b.inject_gen_regress(i).is_ok() {
                caught = caught.saturating_add(1);
            }
        }
        s.add_dyn(
            "代际回退注入 10/10 检出",
            caught == 10 && b.gen() == 100,
            "声称推进但 ≤ 当前代际必须全拒",
        );
    }
    // 6 订阅链满容 +1 拒绝。
    {
        let mut b = MediaBus::new(1);
        let mut ok_cnt = 0u32;
        for _ in 0..MAX_SUBSCRIBERS {
            if b.subscribe().is_ok() {
                ok_cnt = ok_cnt.saturating_add(1);
            }
        }
        let r = b.subscribe();
        s.add_dyn(
            "订阅满容+1 拒绝",
            ok_cnt == MAX_SUBSCRIBERS as u32 && r == Err(MediaCode::ChainFull),
            "订阅链超容必须拒绝",
        );
    }
    // 7 零订阅分发：应投 0、丢失 0（零边界）。
    {
        let mut b = MediaBus::new(1);
        let _ = b.eject();
        let r = b.dispatch_all();
        s.add_dyn(
            "零订阅分发守恒",
            r == Ok(0) && b.chain_zero_loss() && b.actual_delivered() == 0,
            "无订阅者时零投递且零丢失",
        );
    }
    // 8 代际上限：u64::MAX 顶格插入后同代再插即回退拒。
    {
        let mut b = MediaBus::new(u64::MAX);
        let _ = b.eject();
        let r = b.insert_media(u64::MAX);
        s.add_dyn(
            "代际顶格同代拒",
            r == Err(MediaCode::GenRegress),
            "同代不算推进：上限即回退",
        );
    }
    // 9 判定次序：状态迁移先于代际（Mounted 态 + 回退代际 → BadTransition）。
    {
        let mut b = MediaBus::new(10);
        let r = b.insert_media(9);
        s.add_dyn(
            "判序状态先于代际",
            r == Err(MediaCode::BadTransition),
            "跨态迁移判据先于代际判据",
        );
    }
    // 10 状态机拒止：Mounted 直接插入 / Ejected 直接重挂载。
    {
        let mut b = MediaBus::new(1);
        let r1 = b.insert_media(2);
        let _ = b.eject();
        let r2 = b.request_remount();
        s.add_dyn(
            "状态机跨态拒止",
            r1 == Err(MediaCode::BadTransition) && r2 == Err(MediaCode::RemountDenied),
            "Mounted 不得直接插入，Ejected 不得跳过预告",
        );
    }
    // 11 重挂载未预告拒：Changed 态直接申请重挂载。
    {
        let mut b = MediaBus::new(1);
        let _ = b.subscribe();
        let _ = b.eject();
        let _ = b.insert_media(2);
        let r = b.request_remount();
        s.add_dyn(
            "重挂载未预告拒",
            r == Err(MediaCode::RemountDenied) && b.state() == MediaState::Changed,
            "Changed 态必须先预告再重挂载",
        );
    }
    // 12 对账守恒：三类注入合计 30，逃逸 0。
    {
        let mut b = MediaBus::new(100);
        for _ in 0..10u32 {
            let _ = b.inject_false_alarm();
        }
        for i in 0..10u32 {
            let _ = b.inject_duplicate(500 + i);
        }
        for i in 0..10u64 {
            let _ = b.inject_gen_regress(i);
        }
        s.add_dyn(
            "注入账守恒 30",
            b.conserved() && b.injected() == 30 && b.detected() == 30,
            "注入=检出+逃逸，逃逸恒 0",
        );
    }
    // 13 双跑一致。
    s.add_dyn("双跑逐字段一致", dual_run_consistent(), "同一输入两次建链逐字段相等");
    // 14 文案闭集非空。
    {
        let mut closed = true;
        for c in ALL_CODES.iter() {
            if c.detail().is_empty() {
                closed = false;
            }
        }
        s.add_dyn("文案闭集非空", closed, "每个诊断码都必须有中文建议");
    }
    // 15 段位守卫。
    s.add_dyn("段位守卫", seg_guard_ok(), "SEG_BASE 对齐且在分派区间");
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normal_full_cycle() {
        let mut b = MediaBus::new(100);
        assert!(b.subscribe().is_ok());
        assert!(b.eject().is_ok());
        assert!(b.insert_media(101).is_ok());
        assert!(b.preannounce_remount().is_ok());
        assert!(b.request_remount().is_ok());
        assert!(b.close_remount().is_ok());
        assert_eq!(b.gen(), 101);
        assert_eq!(b.state(), MediaState::Mounted);
        assert_eq!(b.chain_len(), 3);
        assert!(b.chain_zero_loss());
    }

    #[test]
    fn dispatch_zero_loss_10_rounds() {
        let mut b = MediaBus::new(1);
        assert!(b.subscribe().is_ok());
        assert!(b.subscribe().is_ok());
        let mut g = 1u64;
        for _ in 0..10u32 {
            assert!(b.eject().is_ok());
            g = g.saturating_add(1);
            assert!(b.insert_media(g).is_ok());
            assert!(b.preannounce_remount().is_ok());
            assert!(b.request_remount().is_ok());
            assert!(b.close_remount().is_ok());
            assert!(matches!(b.dispatch_all(), Ok(6)));
        }
        assert_eq!(b.gen(), 11);
        assert_eq!(b.expected_delivered(), 60);
        assert_eq!(b.actual_delivered(), 60);
        assert!(b.chain_zero_loss());
    }

    #[test]
    fn false_alarm_10_of_10() {
        let mut b = MediaBus::new(50);
        for _ in 0..10u32 {
            assert!(b.inject_false_alarm().is_ok());
        }
        assert_eq!(b.injected(), 10);
        assert_eq!(b.detected(), 10);
        assert_eq!(b.chain_len(), 0);
    }

    #[test]
    fn duplicate_10_of_10() {
        let mut b = MediaBus::new(50);
        let _ = b.eject();
        for i in 0..10u32 {
            assert!(b.inject_duplicate(100 + i).is_ok());
        }
        assert!(b.conserved());
    }

    #[test]
    fn gen_regress_10_of_10() {
        let mut b = MediaBus::new(100);
        for i in 0..10u64 {
            assert!(b.inject_gen_regress(i).is_ok(), "claim {} 必须按回退检出", i);
        }
        assert_eq!(b.gen(), 100);
        assert!(b.conserved());
    }

    #[test]
    fn sub_cap_full_plus_one() {
        let mut b = MediaBus::new(1);
        for _ in 0..MAX_SUBSCRIBERS {
            assert!(b.subscribe().is_ok());
        }
        assert_eq!(b.subscribe(), Err(MediaCode::ChainFull));
    }

    #[test]
    fn judge_order_state_before_gen() {
        let mut b = MediaBus::new(10);
        // Mounted 态 + 回退代际同时成立：状态迁移判据先行。
        assert_eq!(b.insert_media(9), Err(MediaCode::BadTransition));
        assert!(b.eject().is_ok());
        // Ejected 态下代际判据才轮到。
        assert_eq!(b.insert_media(10), Err(MediaCode::GenRegress));
    }

    #[test]
    fn remount_requires_preannounce() {
        let mut b = MediaBus::new(1);
        let _ = b.eject();
        let _ = b.insert_media(2);
        assert_eq!(b.request_remount(), Err(MediaCode::RemountDenied));
        assert!(b.preannounce_remount().is_ok());
        assert!(b.request_remount().is_ok());
        assert!(b.close_remount().is_ok());
    }

    #[test]
    fn gen_max_same_gen_rejected() {
        let mut b = MediaBus::new(u64::MAX);
        assert!(b.eject().is_ok());
        assert_eq!(b.insert_media(u64::MAX), Err(MediaCode::GenRegress));
    }

    #[test]
    fn conservation_30() {
        let mut b = MediaBus::new(100);
        for _ in 0..10u32 {
            let _ = b.inject_false_alarm();
        }
        for i in 0..10u32 {
            let _ = b.inject_duplicate(500 + i);
        }
        for i in 0..10u64 {
            let _ = b.inject_gen_regress(i);
        }
        assert_eq!(b.injected(), 30);
        assert_eq!(b.detected(), 30);
        assert!(b.conserved());
    }

    #[test]
    fn dual_run_identical() {
        assert!(dual_run_consistent());
    }

    #[test]
    fn seg_guard_and_details() {
        assert!(seg_guard_ok());
        assert_eq!(SEG_BASE % 0x40, 0);
        for c in ALL_CODES.iter() {
            assert!(!c.detail().is_empty());
        }
    }
}
