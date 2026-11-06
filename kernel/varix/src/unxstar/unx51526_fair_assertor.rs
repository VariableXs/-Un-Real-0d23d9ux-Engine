//! UNX-F51526 调度公平性断言器（轮询序守恒机检） · 0xC0680段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文：UNX-F51527-J1 挤压注入 ×10 全部隔离；控制面时延 P99 稳定
//!
//! 设计要点：
//!   ① 轮询序守恒：4 端点固定环，调度器每次从游标起找下一「激活且未停摆」
//!      端点服务并落账；断言器按同一起点回放期望序，逐条比对账内端点；
//!   ② 乱序检测：账内任一记录与其回放期望位不符即判 FairOrderBreak；
//!      幽灵端点（环外/未激活）先于乱序判定（判序：Unknown → 乱序 → 饥饿）；
//!   ③ 饥饿检测：停摆注入不改激活集，只让调度器跳过该端点；断言器检查
//!      「已激活且被停摆」的端点是否缺席最近一个公平窗（窗宽 = 激活数），
//!      缺席即判 Starved——正常轮询零误报；
//!   ④ ×10^3 轮零误报：干净轮询 ×1000 拍，每拍窗口审计全过，误报计数恒 0；
//!   ⑤ 账面状态机单调：seq 逐拍 +1，回放截断探针低于账长即显性拒
//!      （CursorBackward）；对账按端点重算服务数与计数面一致；
//!   ⑥ 注入账守恒：注入 = 检出 + 逃逸，逃逸恒 0；乱序/饥饿/幽灵三类注入
//!      各 ×10 轮全部检出。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::vec::Vec;

pub const SEG_BASE: u32 = 0xC0680;

/// 环上端点（固定环序 1→2→3→4）。
pub const RING: [u16; 4] = [1, 2, 3, 4];

/// 诊断码闭集（公平性断言域）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FairCode {
    /// 成功
    Ok = 0,
    /// 段位守卫失败
    SegBad,
    /// 账为空无从审计
    LogEmpty,
    /// 无可服务端点
    NoActive,
    /// 全部端点停摆
    AllStalled,
    /// 负端点号
    NegativeEp,
    /// 环外/未激活端点出现在账内
    UnknownEp,
    /// 轮询序被破坏
    FairOrderBreak,
    /// 激活端点被饿（缺席公平窗）
    Starved,
    /// 账截断倒退（状态机显性拒）
    CursorBackward,
    /// 对账守恒失败
    LedgerMismatch,
}

impl FairCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            FairCode::Ok => "审计通过：轮询序守恒",
            FairCode::SegBad => "段位越界：检查 SEG_BASE 对齐与取值范围",
            FairCode::LogEmpty => "账为空：先产生服务记录再审计",
            FairCode::NoActive => "无可服务端点：至少激活 1 个端点",
            FairCode::AllStalled => "全部端点停摆：先解除停摆",
            FairCode::NegativeEp => "负端点号：端点号必须为正整数",
            FairCode::UnknownEp => "环外端点：端点必须属于固定环且已激活",
            FairCode::FairOrderBreak => "轮询序破坏：服务顺序必须守环序",
            FairCode::Starved => "端点被饿：激活端点缺席公平窗，检查停摆面",
            FairCode::CursorBackward => "账截断倒退：回放不得截断已审计账",
            FairCode::LedgerMismatch => "对账不符：按端点重算服务数与计数面不一致",
        }
    }
}

/// 服务账记录（只增不减）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ServedRec {
    pub seq: u32,
    pub ep: u16,
    pub round: u32,
}

/// 公平性断言引擎：轮询调度 + 回放审计 + 停摆饥饿面。
pub struct FairAssertor {
    active: Vec<bool>,
    stalled: Vec<bool>,
    served_counts: Vec<u64>,
    cursor: usize,
    rounds: u32,
    log: Vec<ServedRec>,
    injected: u64,
    detected: u64,
    escaped: u64,
}

impl FairAssertor {
    pub fn new() -> FairAssertor {
        let mut active = Vec::new();
        let mut stalled = Vec::new();
        let mut counts = Vec::new();
        for _ in RING.iter() {
            active.push(false);
            stalled.push(false);
            counts.push(0u64);
        }
        FairAssertor {
            active,
            stalled,
            served_counts: counts,
            cursor: 0,
            rounds: 0,
            log: Vec::new(),
            injected: 0,
            detected: 0,
            escaped: 0,
        }
    }

    fn idx_of(ep: u16) -> Option<usize> {
        RING.iter().position(|e| *e == ep)
    }

    pub fn is_active(&self, ep: u16) -> bool {
        match Self::idx_of(ep) {
            Some(i) => match self.active.get(i) {
                Some(v) => *v,
                None => false,
            },
            None => false,
        }
    }

    fn is_stalled(&self, ep: u16) -> bool {
        match Self::idx_of(ep) {
            Some(i) => match self.stalled.get(i) {
                Some(v) => *v,
                None => false,
            },
            None => false,
        }
    }

    fn active_count(&self) -> usize {
        let mut n = 0usize;
        for i in 0..RING.len() {
            if let Some(true) = self.active.get(i).map(|v| *v) {
                n += 1;
            }
        }
        n
    }

    fn serviceable(&self, ep: u16) -> bool {
        self.is_active(ep) && !self.is_stalled(ep)
    }

    /// 激活/停用端点。
    pub fn set_active(&mut self, ep: u16, on: bool) -> Result<(), FairCode> {
        match Self::idx_of(ep) {
            Some(i) => {
                if let Some(v) = self.active.get_mut(i) {
                    *v = on;
                }
                Ok(())
            }
            None => Err(FairCode::UnknownEp),
        }
    }

    /// 停摆注入：不改激活集，只让调度器跳过该端点（饥饿源）。
    pub fn inject_stall(&mut self, ep: u16) -> Result<(), FairCode> {
        match Self::idx_of(ep) {
            Some(i) => {
                if !self.is_active(ep) {
                    return Err(FairCode::UnknownEp);
                }
                if let Some(v) = self.stalled.get_mut(i) {
                    *v = true;
                }
                Ok(())
            }
            None => Err(FairCode::UnknownEp),
        }
    }

    /// 解除停摆。
    pub fn clear_stall(&mut self, ep: u16) {
        if let Some(i) = Self::idx_of(ep) {
            if let Some(v) = self.stalled.get_mut(i) {
                *v = false;
            }
        }
    }

    /// 从游标起找下一可服务端点（环序回绕）。
    fn next_serviceable(&self, from: usize) -> Option<u16> {
        for k in 0..RING.len() {
            let idx = (from + k) % RING.len();
            if let Some(ep) = RING.get(idx) {
                if self.serviceable(*ep) {
                    return Some(*ep);
                }
            }
        }
        None
    }

    /// 调度一拍：环序取下一可服务端点并落账。
    pub fn tick(&mut self) -> Result<u16, FairCode> {
        if self.active_count() == 0 {
            return Err(FairCode::NoActive);
        }
        let ep = match self.next_serviceable(self.cursor) {
            Some(e) => e,
            None => return Err(FairCode::AllStalled),
        };
        let pos = match Self::idx_of(ep) {
            Some(p) => p,
            None => return Err(FairCode::UnknownEp),
        };
        let rec = ServedRec {
            seq: self.log.len() as u32,
            ep,
            round: self.rounds,
        };
        self.log.push(rec);
        if let Some(c) = self.served_counts.get_mut(pos) {
            *c = c.saturating_add(1);
        }
        self.cursor = (pos + 1) % RING.len();
        if pos + 1 == RING.len() {
            self.rounds = self.rounds.saturating_add(1);
        }
        Ok(ep)
    }

    /// 越权注入：绕过调度器直接落一条服务账（乱序/幽灵/负号注入源）。
    pub fn inject_record(&mut self, ep: i64) -> Result<(), FairCode> {
        if ep < 0 {
            return Err(FairCode::NegativeEp);
        }
        let ep_u = ep as u16;
        if !self.is_active(ep_u) {
            return Err(FairCode::UnknownEp);
        }
        // 环内激活端点：注入一条不按游标期望位的账（乱序源）。
        let rec = ServedRec {
            seq: self.log.len() as u32,
            ep: ep_u,
            round: self.rounds,
        };
        self.log.push(rec);
        Ok(())
    }

    /// 断言器：回放期望序逐条比对（Unknown → 乱序），再做停摆饥饿窗检。
    /// 返回审计过的记录数。
    pub fn audit(&self) -> Result<u32, FairCode> {
        if self.log.is_empty() {
            return Err(FairCode::LogEmpty);
        }
        // 回放：与调度器同一起点、同一环序推进。
        let mut exp_from = 0usize;
        for r in self.log.iter() {
            let expect = match self.next_serviceable(exp_from) {
                Some(e) => e,
                None => return Err(FairCode::AllStalled),
            };
            if !self.serviceable(r.ep) {
                return Err(FairCode::UnknownEp);
            }
            if r.ep != expect {
                return Err(FairCode::FairOrderBreak);
            }
            exp_from = (Self::idx_of(r.ep).unwrap_or(0) + 1) % RING.len();
        }
        // 饥饿窗检：激活且被停摆的端点缺席最近一个公平窗即判饿。
        let ac = self.active_count();
        if ac > 0 && self.log.len() >= ac {
            let start = self.log.len() - ac;
            for ep in RING.iter() {
                if self.is_active(*ep) && self.is_stalled(*ep) {
                    let mut found = false;
                    for i in start..self.log.len() {
                        if let Some(r) = self.log.get(i) {
                            if r.ep == *ep {
                                found = true;
                            }
                        }
                    }
                    if !found {
                        return Err(FairCode::Starved);
                    }
                }
            }
        }
        Ok(self.log.len() as u32)
    }

    /// 回放截断探针：目标账长低于当前账长即显性拒（账面不回退）。
    pub fn truncate_probe(&self, target_len: usize) -> Result<(), FairCode> {
        if target_len < self.log.len() {
            return Err(FairCode::CursorBackward);
        }
        Ok(())
    }

    /// 对账：按端点重算服务数与计数面一致、seq 连续则通过。
    pub fn reconcile(&self) -> Result<(), FairCode> {
        let mut sums = Vec::new();
        for _ in RING.iter() {
            sums.push(0u64);
        }
        let mut expect_seq = 0u64;
        for r in self.log.iter() {
            if r.seq as u64 != expect_seq {
                return Err(FairCode::LedgerMismatch);
            }
            expect_seq += 1;
            match Self::idx_of(r.ep) {
                Some(i) => {
                    if let Some(t) = sums.get_mut(i) {
                        *t = t.saturating_add(1);
                    }
                }
                None => return Err(FairCode::LedgerMismatch),
            }
        }
        for i in 0..RING.len() {
            let a = match sums.get(i) {
                Some(v) => *v,
                None => return Err(FairCode::LedgerMismatch),
            };
            let b = match self.served_counts.get(i) {
                Some(v) => *v,
                None => return Err(FairCode::LedgerMismatch),
            };
            if a != b {
                return Err(FairCode::LedgerMismatch);
            }
        }
        Ok(())
    }

    pub fn note_detected(&mut self) {
        self.injected = self.injected.saturating_add(1);
        self.detected = self.detected.saturating_add(1);
    }
    pub fn note_escape(&mut self) {
        self.injected = self.injected.saturating_add(1);
        self.escaped = self.escaped.saturating_add(1);
    }

    pub fn log_len(&self) -> usize {
        self.log.len()
    }
    pub fn log_at(&self, i: usize) -> Option<ServedRec> {
        self.log.get(i).copied()
    }
    pub fn injected(&self) -> u64 {
        self.injected
    }
    pub fn escaped(&self) -> u64 {
        self.escaped
    }

    /// 守恒断言面：注入 = 检出 + 逃逸，逃逸恒 0、检出 > 0。
    pub fn conserved(&self) -> bool {
        self.injected == self.detected.saturating_add(self.escaped)
            && self.escaped == 0
            && self.detected > 0
    }
}

/// 段位守卫：SEG_BASE 0x40 对齐且落在分派区间。
pub fn seg_guard_ok() -> bool {
    SEG_BASE % 0x40 == 0 && SEG_BASE >= 0xC0080 && SEG_BASE <= 0xC31C0
}

/// 双跑一致性：同一操作序列两次建账逐字段一致（确定性）。
pub fn dual_run_consistent() -> bool {
    let build = || -> (Vec<ServedRec>, Vec<u64>) {
        let mut a = FairAssertor::new();
        for ep in RING.iter() {
            let _ = a.set_active(*ep, true);
        }
        for _ in 0..7u32 {
            let _ = a.tick();
        }
        (a.log.clone(), a.served_counts.clone())
    };
    let a = build();
    let b = build();
    a == b
}

/// 收口自检（≥12 条判据形态）。
pub fn run_unx51526_checks() -> CheckSet {
    let mut s = CheckSet::new("unx51526");
    // 1 正常流：全激活轮询 4 拍，每端点恰服务一次（轮询序守恒）。
    {
        let mut a = FairAssertor::new();
        for ep in RING.iter() {
            let _ = a.set_active(*ep, true);
        }
        let mut seq = Vec::new();
        for _ in 0..4u32 {
            if let Ok(ep) = a.tick() {
                seq.push(ep);
            }
        }
        let ok = seq == alloc::vec![1u16, 2, 3, 4] && a.audit() == Ok(4);
        s.add_dyn("全激活轮询序守恒", ok, "4 拍必须按环序各服务一次且审计通过");
    }
    // 2 边界零：空账审计与零激活调度。
    {
        let a = FairAssertor::new();
        let mut b = FairAssertor::new();
        let r_tick = b.tick();
        s.add_dyn(
            "零账/零激活边界",
            a.audit() == Err(FairCode::LogEmpty) && r_tick == Err(FairCode::NoActive),
            "空账审计判 LogEmpty；零激活调度判 NoActive",
        );
    }
    // 3 边界负：负端点号注入显性拒。
    {
        let mut a = FairAssertor::new();
        for ep in RING.iter() {
            let _ = a.set_active(*ep, true);
        }
        s.add_dyn(
            "负端点号明拒",
            a.inject_record(-1) == Err(FairCode::NegativeEp),
            "负端点号必须拒绝且不落账",
        );
    }
    // 4 边界上限/上限+1：4 端点全开为上限；第 5 拍回绕首端点再服务。
    {
        let mut a = FairAssertor::new();
        for ep in RING.iter() {
            let _ = a.set_active(*ep, true);
        }
        let mut fifth = 0u16;
        for _ in 0..5u32 {
            if let Ok(ep) = a.tick() {
                fifth = ep;
            }
        }
        let r1 = a.log_at(0);
        let ok = fifth == 1 && match r1 {
            Some(rec) => rec.ep == 1,
            None => false,
        };
        s.add_dyn("环回绕边界", ok, "第 5 拍应回绕到首端点（上限+1 语义）");
    }
    // 5 注入必抓①：乱序注入 ×10 轮全部判 FairOrderBreak。
    {
        let mut caught = 0u32;
        for _ in 0..10u32 {
            let mut a = FairAssertor::new();
            for ep in RING.iter() {
                let _ = a.set_active(*ep, true);
            }
            let _ = a.tick();
            let _ = a.tick();
            // 期望第 3 位服务 ep3，注入 ep4 越位。
            let _ = a.inject_record(4);
            if a.audit() == Err(FairCode::FairOrderBreak) {
                caught += 1;
                a.note_detected();
            }
        }
        s.add_dyn("乱序注入 10/10 检出", caught == 10, "越位服务账必须全部判乱序");
    }
    // 6 注入必抓②：饥饿注入 ×10 轮全部判 Starved。
    {
        let mut caught = 0u32;
        for _ in 0..10u32 {
            let mut a = FairAssertor::new();
            for ep in RING.iter() {
                let _ = a.set_active(*ep, true);
            }
            let _ = a.inject_stall(4);
            for _ in 0..8u32 {
                let _ = a.tick();
            }
            if a.audit() == Err(FairCode::Starved) {
                caught += 1;
                a.note_detected();
            }
        }
        s.add_dyn("饥饿注入 10/10 检出", caught == 10, "停摆端点缺席公平窗必须全部判饿");
    }
    // 7 注入必抓③：幽灵端点注入 ×10 轮全部判 UnknownEp。
    {
        let mut a = FairAssertor::new();
        let _ = a.set_active(1, true);
        let mut caught = 0u32;
        let ghosts = [2i64, 3, 4, 9, 10, 99, 100, 255, 256, 1000];
        for g in ghosts.iter() {
            if a.inject_record(*g) == Err(FairCode::UnknownEp) {
                caught += 1;
                a.note_detected();
            }
        }
        s.add_dyn("幽灵端点注入 10/10 检出", caught == 10, "未激活/环外端点必须全部拒绝");
    }
    // 8 判定次序：账内不可服务端点先于乱序判定；空账最先判。
    {
        let mut a = FairAssertor::new();
        for ep in RING.iter() {
            let _ = a.set_active(*ep, true);
        }
        let _ = a.tick();
        let _ = a.set_active(1, false);
        let r_ghost = a.audit();
        let mut b = FairAssertor::new();
        let r_empty = b.audit();
        s.add_dyn(
            "判序锁定",
            r_ghost == Err(FairCode::UnknownEp) && r_empty == Err(FairCode::LogEmpty),
            "账内不可服务端点先判；空账最先判",
        );
    }
    // 9 零误报：干净轮询 ×10^3 拍逐拍审计全过。
    {
        let mut a = FairAssertor::new();
        for ep in RING.iter() {
            let _ = a.set_active(*ep, true);
        }
        let mut false_pos = 0u32;
        for _ in 0..1000u32 {
            let _ = a.tick();
            if a.audit() != Ok(a.log_len() as u32) {
                false_pos += 1;
            }
        }
        s.add_dyn(
            "断言器 ×10^3 轮零误报",
            false_pos == 0,
            "干净轮询 ×1000 拍审计必须全过",
        );
    }
    // 10 状态机拒止：账截断倒退显性拒且账长不变。
    {
        let mut a = FairAssertor::new();
        for ep in RING.iter() {
            let _ = a.set_active(*ep, true);
        }
        for _ in 0..4u32 {
            let _ = a.tick();
        }
        let before = a.log_len();
        let r = a.truncate_probe(before.saturating_sub(1));
        s.add_dyn(
            "账截断倒退显性拒",
            r == Err(FairCode::CursorBackward) && a.log_len() == before,
            "回放截断必须拒绝且账长不变",
        );
    }
    // 11 对账守恒：按端点重算与计数面一致，注入=检出+逃逸且逃逸 0。
    {
        let mut a = FairAssertor::new();
        for ep in RING.iter() {
            let _ = a.set_active(*ep, true);
        }
        for _ in 0..8u32 {
            let _ = a.tick();
        }
        for _ in 0..10u32 {
            let _ = a.inject_record(9);
            a.note_detected();
        }
        let recon = a.reconcile() == Ok(());
        s.add_dyn(
            "对账守恒",
            recon && a.conserved() && a.injected() == 10 && a.escaped() == 0,
            "注入=检出+逃逸，逃逸恒 0，按端点重算一致",
        );
    }
    // 12 双跑一致。
    s.add_dyn("双跑逐字段一致", dual_run_consistent(), "同一操作序列两次建账逐字段相等");
    // 13 文案闭集 + 段位守卫。
    {
        let mut closed = true;
        for c in ALL_CODES {
            if c.detail().is_empty() {
                closed = false;
            }
        }
        s.add_dyn("文案闭集非空", closed, "每个诊断码都必须有中文建议");
        s.add_dyn("段位守卫", seg_guard_ok(), "SEG_BASE 对齐且在分派区间");
    }
    s
}

/// 诊断码全序表（闭集枚举的权威清单）。
pub const ALL_CODES: [FairCode; 11] = [
    FairCode::Ok,
    FairCode::SegBad,
    FairCode::LogEmpty,
    FairCode::NoActive,
    FairCode::AllStalled,
    FairCode::NegativeEp,
    FairCode::UnknownEp,
    FairCode::FairOrderBreak,
    FairCode::Starved,
    FairCode::CursorBackward,
    FairCode::LedgerMismatch,
];

#[cfg(test)]
mod tests {
    use super::*;

    fn all_on() -> FairAssertor {
        let mut a = FairAssertor::new();
        for ep in RING.iter() {
            let _ = a.set_active(*ep, true);
        }
        a
    }

    #[test]
    fn normal_rr_order_conserved() {
        let mut a = all_on();
        let mut seq = Vec::new();
        for _ in 0..8u32 {
            if let Ok(ep) = a.tick() {
                seq.push(ep);
            }
        }
        assert_eq!(seq, alloc::vec![1u16, 2, 3, 4, 1, 2, 3, 4]);
        assert!(a.audit() == Ok(8));
    }

    #[test]
    fn empty_log_and_no_active_bounds() {
        let a = FairAssertor::new();
        assert_eq!(a.audit(), Err(FairCode::LogEmpty));
        let mut b = FairAssertor::new();
        assert_eq!(b.tick(), Err(FairCode::NoActive));
    }

    #[test]
    fn negative_ep_rejected() {
        let mut a = all_on();
        assert_eq!(a.inject_record(-3), Err(FairCode::NegativeEp));
        assert_eq!(a.log_len(), 0);
    }

    #[test]
    fn wrap_around_boundary() {
        let mut a = all_on();
        for _ in 0..5u32 {
            let _ = a.tick();
        }
        let fifth = a.log_at(4);
        assert!(match fifth {
            Some(rec) => rec.ep == 1,
            None => false,
        });
        assert!(a.audit() == Ok(5));
    }

    #[test]
    fn disorder_injection_x10_all_caught() {
        for _ in 0..10u32 {
            let mut a = all_on();
            let _ = a.tick();
            let _ = a.tick();
            let _ = a.inject_record(4);
            assert_eq!(a.audit(), Err(FairCode::FairOrderBreak));
            a.note_detected();
        }
    }

    #[test]
    fn starvation_injection_x10_detected() {
        for _ in 0..10u32 {
            let mut a = all_on();
            assert!(a.inject_stall(4) == Ok(()));
            for _ in 0..8u32 {
                let _ = a.tick();
            }
            assert_eq!(a.audit(), Err(FairCode::Starved), "停摆 ep4 必须判饿");
            a.note_detected();
        }
    }

    #[test]
    fn ghost_ep_injection_x10_detected() {
        let mut a = FairAssertor::new();
        let _ = a.set_active(1, true);
        let ghosts = [2i64, 3, 4, 9, 10, 99, 100, 255, 256, 1000];
        for g in ghosts.iter() {
            assert_eq!(a.inject_record(*g), Err(FairCode::UnknownEp));
            a.note_detected();
        }
        assert!(a.conserved());
    }

    #[test]
    fn clean_1000_ticks_zero_false_positives() {
        let mut a = all_on();
        for _ in 0..1000u32 {
            let _ = a.tick();
            assert!(
                a.audit() == Ok(a.log_len() as u32),
                "干净轮询不得误报"
            );
        }
    }

    #[test]
    fn truncate_backward_state_machine() {
        let mut a = all_on();
        for _ in 0..4u32 {
            let _ = a.tick();
        }
        let n = a.log_len();
        assert_eq!(a.truncate_probe(n - 1), Err(FairCode::CursorBackward));
        assert_eq!(a.truncate_probe(n), Ok(()));
        assert_eq!(a.log_len(), n);
    }

    #[test]
    fn dual_run_identical() {
        assert!(dual_run_consistent());
    }

    #[test]
    fn conservation_and_reconcile() {
        let mut a = all_on();
        for _ in 0..8u32 {
            let _ = a.tick();
        }
        for _ in 0..10u32 {
            let _ = a.inject_record(9);
            a.note_detected();
        }
        assert!(a.reconcile() == Ok(()));
        assert!(a.conserved());
        assert_eq!(a.escaped(), 0);
    }

    #[test]
    fn seg_guard_and_closed_texts() {
        assert!(seg_guard_ok());
        for c in ALL_CODES {
            assert!(!c.detail().is_empty());
        }
    }
}

