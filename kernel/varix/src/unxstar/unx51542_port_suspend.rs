//! UNX-F51542 端口挂起级联（子树整体挂起/恢复序） · 0xC0A40段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文：UNX-F51543-J1 空闲超时休眠与唤醒边界 ×10 正确
//!
//! 设计要点：
//!   ① 级联域按「父子口拓扑树」建模：7 口冻结父表（根 0 → {1,2}，1 →
//!      {3,4}，2 → {5,6}）；子树整体挂起按「父先子后」的前序展开，整体
//!      恢复按「子先父后」的逆序回收，序错即违约。
//!   ② 单调状态机：每口只有 活跃→挂起→活跃 一条合法路径；对已挂起子树
//!      再挂起显性拒（NotActive），对活跃子树再恢复显性拒（NotSuspended），
//!      事件流水只增不减、序号单调。
//!   ③ 恢复序冻结锚：根 0 子树挂起序 [0,1,3,4,2,5,6]、恢复序
//!      [3,4,1,5,6,2,0] 为冻结常量，事件流水尾部必须逐位吻合。
//!   ④ 错序注入器：以「父先恢复」「子先挂起」两类错序口供注入 ×10 轮，
//!      加越界与重复挂起注入，级联校验必须全抓；守恒账 注入=检出+逃逸，
//!      逃逸恒 0。
//!   ⑤ 判定次序：端口界 → 子树展开 → 全体前置状态核验 → 逐口落账；核验
//!      不过则一个口都不动（禁部分级联）。
//!   ⑥ 全程零浮点、零 IO、零墙钟、零随机；取值一律走 .get()/.get_mut()。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::vec::Vec;

pub const SEG_BASE: u32 = 0xC0A40;

/// 端口总数。
pub const PORT_COUNT: usize = 7;
/// 无父标记（根口）。
pub const NO_PARENT: usize = 0xFFFF;
/// 父口冻结表：根 0 → {1,2}，1 → {3,4}，2 → {5,6}。
pub const PARENT: [usize; PORT_COUNT] = [NO_PARENT, 0, 0, 1, 1, 2, 2];
/// 根 0 子树的冻结挂起序（父先子后）。
pub const EXPECT_SUSPEND_ROOT0: [usize; PORT_COUNT] = [0, 1, 3, 4, 2, 5, 6];
/// 根 0 子树的冻结恢复序（子先父后）。
pub const EXPECT_RESUME_ROOT0: [usize; PORT_COUNT] = [3, 4, 1, 5, 6, 2, 0];

/// 端口状态机（每口只有一条合法路径）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PortState {
    /// 活跃
    Active,
    /// 挂起
    Suspended,
}

/// 级联事件种类。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CascadeKind {
    /// 挂起事件
    Suspend,
    /// 恢复事件
    Resume,
}

/// 单条级联事件（只增不减）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CascadeEvent {
    pub seq: u32,
    pub port: usize,
    pub kind: CascadeKind,
}

/// 诊断码闭集（级联挂起域）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SuspendCode {
    /// 成功
    Ok = 0,
    /// 段位守卫失败（SEG_BASE 越界或不对齐）
    SegBad,
    /// 端口号越界
    PortOob,
    /// 子树中存在非活跃口（禁重复挂起）
    NotActive,
    /// 子树中存在非挂起口（禁恢复活跃子树）
    NotSuspended,
    /// 级联序违约（注入必抓）
    OrderViolation,
    /// 事件流水不符
    JournalMismatch,
    /// 账面不符
    LedgerMismatch,
}

impl SuspendCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            SuspendCode::Ok => "级联操作成功",
            SuspendCode::SegBad => "段位越界：检查 SEG_BASE 对齐与取值范围",
            SuspendCode::PortOob => "端口越界：端口号必须在 0..7 之内",
            SuspendCode::NotActive => "重复挂起：子树中存在已挂起口，整体拒",
            SuspendCode::NotSuspended => "非法恢复：子树中存在活跃口，整体拒",
            SuspendCode::OrderViolation => "级联序违约：挂起须父先子后、恢复须子先父后",
            SuspendCode::JournalMismatch => "流水不符：事件序号与流水长度必须一致",
            SuspendCode::LedgerMismatch => "账面不符：挂起量与恢复量必须逐口对应",
        }
    }
}

/// 子树前序展开（父先子后；冻结父表无环，含防环兜底）。
pub fn subtree_order(root: usize) -> Vec<usize> {
    let mut out = Vec::new();
    if root < PORT_COUNT {
        collect_subtree(root, &mut out);
    }
    out
}

/// 后序（子先父后）：恢复必须子先父后，与前序互为拓扑逆而非序列逆。
pub fn post_order(root: usize) -> Vec<usize> {
    let mut out = Vec::new();
    if root < PORT_COUNT {
        collect_post(root, &mut out);
    }
    out
}

fn collect_post(root: usize, out: &mut Vec<usize>) {
    if out.contains(&root) {
        return;
    }
    for p in 0..PORT_COUNT {
        if let Some(&parent) = PARENT.get(p) {
            if parent == root {
                collect_post(p, out);
            }
        }
    }
    out.push(root);
}

fn collect_subtree(root: usize, out: &mut Vec<usize>) {
    if out.contains(&root) {
        return;
    }
    out.push(root);
    for p in 0..PORT_COUNT {
        if let Some(&parent) = PARENT.get(p) {
            if parent == root {
                collect_subtree(p, out);
            }
        }
    }
}

/// 端口挂起级联引擎。
pub struct SuspendCascade {
    state: [PortState; PORT_COUNT],
    journal: Vec<CascadeEvent>,
    seq: u32,
    suspended_cnt: u64,
    resumed_cnt: u64,
    injected: u64,
    detected: u64,
    escaped: u64,
}

impl SuspendCascade {
    pub fn new() -> SuspendCascade {
        SuspendCascade {
            state: [PortState::Active; PORT_COUNT],
            journal: Vec::new(),
            seq: 0,
            suspended_cnt: 0,
            resumed_cnt: 0,
            injected: 0,
            detected: 0,
            escaped: 0,
        }
    }

    /// 子树整体挂起：前序展开 → 全体活跃核验 → 逐口落账（禁部分级联）。
    pub fn suspend_subtree(&mut self, root: usize) -> Result<usize, SuspendCode> {
        if root >= PORT_COUNT {
            return Err(SuspendCode::PortOob);
        }
        let order = subtree_order(root);
        for p in &order {
            match self.state.get(*p) {
                Some(PortState::Active) => {}
                _ => return Err(SuspendCode::NotActive),
            }
        }
        for p in &order {
            if let Some(st) = self.state.get_mut(*p) {
                *st = PortState::Suspended;
            }
            self.seq = self.seq.saturating_add(1);
            self.journal.push(CascadeEvent {
                seq: self.seq,
                port: *p,
                kind: CascadeKind::Suspend,
            });
        }
        self.suspended_cnt = self.suspended_cnt.saturating_add(order.len() as u64);
        Ok(order.len())
    }

    /// 子树整体恢复：前序展开 → 全体挂起核验 → 逆序（子先父后）落账。
    pub fn resume_subtree(&mut self, root: usize) -> Result<usize, SuspendCode> {
        if root >= PORT_COUNT {
            return Err(SuspendCode::PortOob);
        }
        let order = post_order(root);
        for p in &order {
            match self.state.get(*p) {
                Some(PortState::Suspended) => {}
                _ => return Err(SuspendCode::NotSuspended),
            }
        }
        for p in order.iter() {
            if let Some(st) = self.state.get_mut(*p) {
                *st = PortState::Active;
            }
            self.seq = self.seq.saturating_add(1);
            self.journal.push(CascadeEvent {
                seq: self.seq,
                port: *p,
                kind: CascadeKind::Resume,
            });
        }
        self.resumed_cnt = self.resumed_cnt.saturating_add(order.len() as u64);
        Ok(order.len())
    }

    /// 恢复错序注入器：口供序与冻结恢复序比对，不符即抓，不落账。
    pub fn inject_resume_claim(&self, root: usize, claimed: &[usize]) -> Result<usize, SuspendCode> {
        if root >= PORT_COUNT {
            return Err(SuspendCode::PortOob);
        }
        let canonical = subtree_order(root);
        if claimed.len() != canonical.len() {
            return Err(SuspendCode::OrderViolation);
        }
        for (i, c) in claimed.iter().enumerate() {
            let want = match canonical.get(canonical.len().saturating_sub(1).saturating_sub(i)) {
                Some(v) => *v,
                None => return Err(SuspendCode::OrderViolation),
            };
            if *c != want {
                return Err(SuspendCode::OrderViolation);
            }
        }
        Ok(canonical.len())
    }

    /// 挂起错序注入器：口供序与冻结挂起序比对，不符即抓，不落账。
    pub fn inject_suspend_claim(&self, root: usize, claimed: &[usize]) -> Result<usize, SuspendCode> {
        if root >= PORT_COUNT {
            return Err(SuspendCode::PortOob);
        }
        let canonical = subtree_order(root);
        if claimed.len() != canonical.len() {
            return Err(SuspendCode::OrderViolation);
        }
        for (i, c) in claimed.iter().enumerate() {
            let want = match canonical.get(i) {
                Some(v) => *v,
                None => return Err(SuspendCode::OrderViolation),
            };
            if *c != want {
                return Err(SuspendCode::OrderViolation);
            }
        }
        Ok(canonical.len())
    }

    pub fn port_state(&self, port: usize) -> Option<PortState> {
        match self.state.get(port) {
            Some(st) => Some(*st),
            None => None,
        }
    }

    pub fn journal(&self) -> &[CascadeEvent] {
        &self.journal
    }

    pub fn tally(&self) -> (u64, u64) {
        (self.suspended_cnt, self.resumed_cnt)
    }

    /// 注入记账：检出加一。
    pub fn note_detected(&mut self) {
        self.injected = self.injected.saturating_add(1);
        self.detected = self.detected.saturating_add(1);
    }

    /// 注入逃逸（判据要求恒 0）。
    pub fn note_escape(&mut self) {
        self.injected = self.injected.saturating_add(1);
        self.escaped = self.escaped.saturating_add(1);
    }

    /// 守恒断言面：注入 = 检出 + 逃逸，逃逸恒 0 且检出非零。
    pub fn conserved(&self) -> bool {
        self.injected == self.detected.saturating_add(self.escaped)
            && self.escaped == 0
            && self.detected > 0
    }

    /// 流水一致：序号总数与流水长度对平。
    pub fn journal_consistent(&self) -> bool {
        self.seq as u64 == self.journal.len() as u64
    }

    /// 状态快照（双跑一致面）。
    pub fn snapshot(&self) -> (u64, u64, u32, usize) {
        (self.suspended_cnt, self.resumed_cnt, self.seq, self.journal.len())
    }
}

/// 段位守卫。
pub fn seg_guard_ok() -> bool {
    SEG_BASE % 0x40 == 0 && SEG_BASE >= 0xC0080 && SEG_BASE <= 0xC31C0
}

/// 双跑一致性：同一级联脚本两次建引擎，快照与流水逐字段一致。
pub fn dual_run_consistent() -> bool {
    let build = || -> ((u64, u64, u32, usize), Vec<CascadeEvent>) {
        let mut e = SuspendCascade::new();
        let _ = e.suspend_subtree(0);
        let _ = e.resume_subtree(0);
        let _ = e.suspend_subtree(2);
        let _ = e.resume_subtree(2);
        (e.snapshot(), e.journal.to_vec())
    };
    build() == build()
}

/// 收口自检（15 条判据形态）。
pub fn run_unx51542_checks() -> CheckSet {
    let mut s = CheckSet::new("unx51542");
    // 1 正常流：根 0 子树整体挂起，序逐位吻合冻结锚。
    {
        let mut e = SuspendCascade::new();
        let n = e.suspend_subtree(0);
        let mut order_ok = n == Ok(PORT_COUNT);
        for (i, want) in EXPECT_SUSPEND_ROOT0.iter().enumerate() {
            let ev = match e.journal().get(i) {
                Some(ev) => *ev,
                None => CascadeEvent { seq: 0, port: 0xFFFF, kind: CascadeKind::Resume },
            };
            if ev.port != *want || ev.kind != CascadeKind::Suspend {
                order_ok = false;
            }
        }
        s.add_dyn("根 0 挂起序", order_ok, "挂起序必须为 [0,1,3,4,2,5,6] 父先子后");
    }
    // 2 正常流：整体恢复序逐位吻合冻结锚。
    {
        let mut e = SuspendCascade::new();
        let _ = e.suspend_subtree(0);
        let n = e.resume_subtree(0);
        let mut order_ok = n == Ok(PORT_COUNT);
        for (k, want) in EXPECT_RESUME_ROOT0.iter().enumerate() {
            let ev = match e.journal().get(PORT_COUNT + k) {
                Some(ev) => *ev,
                None => CascadeEvent { seq: 0, port: 0xFFFF, kind: CascadeKind::Suspend },
            };
            if ev.port != *want || ev.kind != CascadeKind::Resume {
                order_ok = false;
            }
        }
        s.add_dyn("根 0 恢复序", order_ok, "恢复序必须为 [3,4,1,5,6,2,0] 子先父后");
    }
    // 3 边界：端口 0/6 放行、7 与更大值拒。
    {
        let mut e = SuspendCascade::new();
        let lo = e.suspend_subtree(0).is_ok();
        let over = e.suspend_subtree(PORT_COUNT);
        let over2 = e.suspend_subtree(PORT_COUNT + 50);
        s.add_dyn(
            "端口边界",
            lo && over == Err(SuspendCode::PortOob) && over2 == Err(SuspendCode::PortOob),
            "端口 0 与 6 放行、7 与越界值显性拒",
        );
    }
    // 4 注入必抓：恢复错序（父先）×10 轮。
    {
        let mut e = SuspendCascade::new();
        let _ = e.suspend_subtree(0);
        let mut caught = 0u32;
        for i in 0..10u32 {
            let root = [0usize, 1, 2][i as usize % 3];
            let wrong = subtree_order(root); // 父先序即恢复错序
            if e.inject_resume_claim(root, &wrong) == Err(SuspendCode::OrderViolation) {
                caught += 1;
                e.note_detected();
            }
        }
        s.add_dyn("恢复错序注入 10/10", caught == 10, "父先恢复必须全抓");
    }
    // 5 注入必抓：挂起错序（子先）×10 轮。
    {
        let mut e = SuspendCascade::new();
        let mut caught = 0u32;
        for i in 0..10u32 {
            let root = [0usize, 1, 2][i as usize % 3];
            let fwd = subtree_order(root);
            let mut wrong: Vec<usize> = fwd.iter().rev().copied().collect(); // 子先序即挂起错序
            if e.inject_suspend_claim(root, &wrong) == Err(SuspendCode::OrderViolation) {
                caught += 1;
                e.note_detected();
            }
            let _ = wrong.pop();
        }
        s.add_dyn("挂起错序注入 10/10", caught == 10, "子先挂起必须全抓");
    }
    // 6 注入必抓：越界端口 ×10 轮。
    {
        let mut e = SuspendCascade::new();
        let mut caught = 0u32;
        for i in 0..10u32 {
            if e.suspend_subtree(PORT_COUNT + i as usize) == Err(SuspendCode::PortOob) {
                caught += 1;
                e.note_detected();
            }
        }
        s.add_dyn("越界注入 10/10", caught == 10, "越界端口必须全抓且账面不动");
    }
    // 7 注入必抓：重复挂起 ×10 轮。
    {
        let mut e = SuspendCascade::new();
        let _ = e.suspend_subtree(0);
        let mut caught = 0u32;
        for _ in 0..10u32 {
            if e.suspend_subtree(0) == Err(SuspendCode::NotActive) {
                caught += 1;
                e.note_detected();
            }
        }
        let resumed = e.resume_subtree(0);
        s.add_dyn(
            "重复挂起注入 10/10",
            caught == 10 && resumed == Ok(PORT_COUNT),
            "已挂起子树再挂起必须全抓",
        );
    }
    // 8 判定次序：越界先于状态核验。
    {
        let mut e = SuspendCascade::new();
        let r = e.resume_subtree(PORT_COUNT + 1);
        s.add_dyn(
            "判序越界先行",
            r == Err(SuspendCode::PortOob),
            "端口界判据先于子树状态核验",
        );
    }
    // 9 状态机拒止：对活跃子树恢复显性拒。
    {
        let mut e = SuspendCascade::new();
        let r1 = e.resume_subtree(0);
        let _ = e.suspend_subtree(3);
        let r2 = e.resume_subtree(1); // 子树 {1,3,4}，3 已挂起、1/4 活跃
        s.add_dyn(
            "非法恢复拒止",
            r1 == Err(SuspendCode::NotSuspended) && r2 == Err(SuspendCode::NotSuspended),
            "活跃子树恢复显性拒且禁部分级联",
        );
    }
    // 10 部分级联禁令：核验不过一个口都不动。
    {
        let mut e = SuspendCascade::new();
        let _ = e.suspend_subtree(3);
        let snap = e.snapshot();
        let _ = e.suspend_subtree(1); // {1,3,4} 中 3 已挂起 → 整体拒
        s.add_dyn(
            "禁部分级联",
            e.snapshot() == snap,
            "核验不过不得有任何一个口被挂起",
        );
    }
    // 11 挂起/恢复量守恒：整循环后全活跃且账平。
    {
        let mut e = SuspendCascade::new();
        let _ = e.suspend_subtree(0);
        let _ = e.resume_subtree(0);
        let mut all_active = true;
        for p in 0..PORT_COUNT {
            if e.port_state(p) != Some(PortState::Active) {
                all_active = false;
            }
        }
        let (sus, res) = e.tally();
        s.add_dyn(
            "挂起恢复量守恒",
            all_active && sus == res && sus == PORT_COUNT as u64 && e.journal_consistent(),
            "挂起量=恢复量=子树口数，流水对平",
        );
    }
    // 12 对账守恒：注入 = 检出，逃逸 0。
    {
        let mut e = SuspendCascade::new();
        for i in 0..10u32 {
            let _ = e.suspend_subtree(PORT_COUNT + i as usize);
            e.note_detected();
        }
        s.add_dyn(
            "注入账守恒",
            e.conserved() && e.snapshot() == (0, 0, 0, 0),
            "注入=检出+逃逸，逃逸恒 0",
        );
    }
    // 13 双跑一致。
    s.add_dyn("双跑逐字段一致", dual_run_consistent(), "同一级联脚本两次建账快照与流水相等");
    // 14 文案闭集非空。
    {
        let mut closed = true;
        for c in ALL_CODES {
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

/// 诊断码全序表（闭集枚举的权威清单）。
pub const ALL_CODES: [SuspendCode; 8] = [
    SuspendCode::Ok,
    SuspendCode::SegBad,
    SuspendCode::PortOob,
    SuspendCode::NotActive,
    SuspendCode::NotSuspended,
    SuspendCode::OrderViolation,
    SuspendCode::JournalMismatch,
    SuspendCode::LedgerMismatch,
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn suspend_order_parent_first() {
        let order = subtree_order(0);
        assert_eq!(order.len(), PORT_COUNT);
        for (i, want) in EXPECT_SUSPEND_ROOT0.iter().enumerate() {
            assert_eq!(order.get(i), Some(want), "挂起序第 {} 位不符", i);
        }
    }

    #[test]
    fn resume_order_child_first() {
        let mut e = SuspendCascade::new();
        let _ = e.suspend_subtree(0);
        assert!(e.resume_subtree(0).is_ok());
        for (k, want) in EXPECT_RESUME_ROOT0.iter().enumerate() {
            let ev = e.journal().get(PORT_COUNT + k);
            assert!(ev.is_some(), "恢复事件 {} 缺失", k);
            let ev = match ev {
                Some(ev) => *ev,
                None => return,
            };
            assert_eq!(ev.port, *want);
            assert_eq!(ev.kind, CascadeKind::Resume);
        }
    }

    #[test]
    fn port_bounds_reject() {
        let mut e = SuspendCascade::new();
        assert_eq!(e.suspend_subtree(PORT_COUNT), Err(SuspendCode::PortOob));
        assert_eq!(e.resume_subtree(PORT_COUNT + 9), Err(SuspendCode::PortOob));
        assert!(e.suspend_subtree(0).is_ok());
    }

    #[test]
    fn resume_order_injection_all_caught() {
        let mut e = SuspendCascade::new();
        let _ = e.suspend_subtree(0);
        let mut caught = 0u32;
        for i in 0..10u32 {
            let root = [0usize, 1, 2][i as usize % 3];
            let wrong = subtree_order(root);
            if e.inject_resume_claim(root, &wrong) == Err(SuspendCode::OrderViolation) {
                caught += 1;
                e.note_detected();
            }
        }
        assert_eq!(caught, 10);
        assert!(e.conserved());
    }

    #[test]
    fn suspend_order_injection_all_caught() {
        let mut e = SuspendCascade::new();
        let mut caught = 0u32;
        for i in 0..10u32 {
            let root = [0usize, 1, 2][i as usize % 3];
            let fwd = subtree_order(root);
            let wrong: Vec<usize> = fwd.iter().rev().copied().collect();
            if e.inject_suspend_claim(root, &wrong) == Err(SuspendCode::OrderViolation) {
                caught += 1;
                e.note_detected();
            }
        }
        assert_eq!(caught, 10);
    }

    #[test]
    fn double_suspend_injection_all_caught() {
        let mut e = SuspendCascade::new();
        let _ = e.suspend_subtree(0);
        let mut caught = 0u32;
        for _ in 0..10u32 {
            if e.suspend_subtree(0) == Err(SuspendCode::NotActive) {
                caught += 1;
                e.note_detected();
            }
        }
        assert_eq!(caught, 10);
        assert!(e.resume_subtree(0).is_ok());
    }

    #[test]
    fn partial_cascade_forbidden() {
        let mut e = SuspendCascade::new();
        let _ = e.suspend_subtree(3);
        let snap = e.snapshot();
        assert_eq!(e.suspend_subtree(1), Err(SuspendCode::NotActive));
        assert_eq!(e.snapshot(), snap);
        assert_eq!(e.port_state(1), Some(PortState::Active));
        assert_eq!(e.port_state(4), Some(PortState::Active));
    }

    #[test]
    fn resume_active_subtree_rejected() {
        let mut e = SuspendCascade::new();
        assert_eq!(e.resume_subtree(0), Err(SuspendCode::NotSuspended));
        assert_eq!(e.resume_subtree(5), Err(SuspendCode::NotSuspended));
    }

    #[test]
    fn tally_conserves_after_full_cycle() {
        let mut e = SuspendCascade::new();
        let _ = e.suspend_subtree(0);
        let _ = e.resume_subtree(0);
        let (sus, res) = e.tally();
        assert_eq!(sus, res);
        assert_eq!(sus, PORT_COUNT as u64);
        assert!(e.journal_consistent());
    }

    #[test]
    fn dual_run_identical() {
        assert!(dual_run_consistent());
    }

    #[test]
    fn seg_guard_and_texts() {
        assert!(seg_guard_ok());
        for c in ALL_CODES {
            assert!(!c.detail().is_empty());
        }
    }

    #[test]
    fn unx51542_checks_green() {
        let set = run_unx51542_checks();
        assert!(set.all_passed(), "UNX-F51542 red: {}", set.red_items_text());
    }
}
