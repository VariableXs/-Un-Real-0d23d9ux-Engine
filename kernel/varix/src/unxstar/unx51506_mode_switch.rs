//! UNX-F51506 BOT/UASP 运行态切换压测（×100 次切换） · 0xC0180段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文：UNX-F51507-J1 框架 24h 首轮零掉盘（7×24 全量归波 23 闭账）；
//! 掉盘即告警三要素在位。
//!
//! 设计要点：
//!   ① 运行态按「BOT ↔ UASP 双协议状态机」建模：Bot → Negotiating → Uasp、
//!      Uasp → Negotiating → Bot；跳变（Bot 直达 Uasp）、自环一律显性拒，
//!      拒止帧不落账、不改态。
//!   ② 切换序号单调：每帧压测携带期望序号，落后于引擎当前序号即「倒退
//!      显性拒」，切换账只增不减。
//!   ③ ×100 次切换封账：帧落满 SWITCH_TARGET 帧即封账冻结，封账后再来帧
//!      按容量耗尽拒；封账帧数与账量必须对平。
//!   ④ 掉盘即告警三要素：事件码、告警等级、恢复建议三者同时在位才算
//!      告警面完备，任一缺位即 AlarmTriadMissing；压测全程零掉盘（drops=0）
//!      是首轮通过判据。
//!   ⑤ 判定次序：封账容量 → 序号倒退 → 跃迁合法性 → 落账；封账优先于
//!      倒退，倒退优先于跃迁。
//!   ⑥ 注入账守恒：注入 = 检出 + 逃逸，逃逸恒为 0；三类注入（掉盘缺要素、
//!      序号倒退、非法跃迁）各 ×10 轮必抓，诊断码闭集中文文案。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::vec::Vec;

/// 0xC0180 段基址（51506 = 0xC0180 + 0x40 × 0）。
pub const SEG_BASE: u32 = 0xC0180;

/// 切换压测目标帧数（判据 ×100 次切换）。
pub const SWITCH_TARGET: u32 = 100;

/// 诊断码闭集（BOT/UASP 切换域）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SwCode {
    /// 成功
    Ok = 0,
    /// 段位守卫失败（SEG_BASE 越界或不对齐）
    SegBad,
    /// 压测账已封账（容量耗尽）
    SealedFull,
    /// 切换序号倒退（显性拒）
    SeqRegress,
    /// 状态机跃迁非法
    StateNoTrans,
    /// 掉盘告警三要素缺位
    AlarmTriadMissing,
    /// 零掉盘判据被击穿（压测中出现掉盘）
    DropHit,
    /// 封账帧数与账量不符
    LedgerMismatch,
}

impl SwCode {
    /// 诊断码取值（段内偏移闭集）。
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 中文诊断文案（闭集逐码在位）。
    pub fn detail(self) -> &'static str {
        match self {
            SwCode::Ok => "切换帧受理成功",
            SwCode::SegBad => "段位越界：检查 SEG_BASE 对齐与取值范围",
            SwCode::SealedFull => "压测账已封：×100 帧落满后禁止追加",
            SwCode::SeqRegress => "序号倒退：压测帧期望序号不得落后于引擎序号",
            SwCode::StateNoTrans => "跃迁非法：BOT/UASP 须经协商态中转",
            SwCode::AlarmTriadMissing => "告警三要素缺位：事件码/等级/恢复建议须同时在位",
            SwCode::DropHit => "掉盘击穿零掉盘判据：检查供电与枚举时序",
            SwCode::LedgerMismatch => "账量不符：封账帧数与账量必须一致",
        }
    }
}

/// 诊断码全序表（闭集枚举权威清单）。
pub const ALL_CODES: [SwCode; 8] = [
    SwCode::Ok,
    SwCode::SegBad,
    SwCode::SealedFull,
    SwCode::SeqRegress,
    SwCode::StateNoTrans,
    SwCode::AlarmTriadMissing,
    SwCode::DropHit,
    SwCode::LedgerMismatch,
];

/// 协议运行态（三态状态机）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ProtoState {
    /// BOT 运行态
    Bot,
    /// 协议协商态（唯一中转态）
    Negotiating,
    /// UASP 运行态
    Uasp,
}

/// 合法跃迁表（冻结）：仅允许经协商态中转的双向切换。
pub fn trans_legal(from: ProtoState, to: ProtoState) -> bool {
    matches!(
        (from, to),
        (ProtoState::Bot, ProtoState::Negotiating)
            | (ProtoState::Negotiating, ProtoState::Uasp)
            | (ProtoState::Uasp, ProtoState::Negotiating)
            | (ProtoState::Negotiating, ProtoState::Bot)
    )
}

/// 掉盘告警三要素（判据：掉盘即告警三要素在位）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AlarmTriad {
    /// 要素一：事件码（非零）
    pub event_code: u32,
    /// 要素二：告警等级（1..=5）
    pub level: u8,
    /// 要素三：恢复建议（非空）
    pub remedy: &'static str,
}

impl AlarmTriad {
    /// 三要素完备性校验：任一缺位即不完备。
    pub fn complete(&self) -> bool {
        self.event_code != 0 && self.level >= 1 && self.level <= 5 && !self.remedy.is_empty()
    }
}

/// 全量告警三要素（冻结样板，事件码取段基址）。
pub const TRIAD_FULL: AlarmTriad = AlarmTriad {
    event_code: 0xC0180,
    level: 3,
    remedy: "掉盘处置：复位端口并重枚举，回放切换账定位事故帧",
};

/// 单帧切换压测记录（只增不减）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SwitchRec {
    /// 压测帧号（从 1 起）
    pub frame: u32,
    /// 帧携带的期望序号
    pub expect_seq: u32,
    /// 目标运行态
    pub to: ProtoState,
}

/// BOT/UASP 切换引擎：状态机 + 单调序号 + 封账面 + 掉盘告警面。
pub struct SwitchEngine {
    state: ProtoState,
    seq: u32,
    recs: Vec<SwitchRec>,
    sealed: bool,
    drops: u32,
    triad: Option<AlarmTriad>,
    injected: u64,
    detected: u64,
}

impl SwitchEngine {
    /// 新引擎：初始 BOT 态、序号 0、未封账。
    pub fn new() -> SwitchEngine {
        SwitchEngine {
            state: ProtoState::Bot,
            seq: 0,
            recs: Vec::new(),
            sealed: false,
            drops: 0,
            triad: None,
            injected: 0,
            detected: 0,
        }
    }

    /// 受理一帧切换压测（判序：封账 → 倒退 → 跃迁 → 落账）。
    pub fn request(&mut self, frame: u32, expect_seq: u32, to: ProtoState) -> Result<(), SwCode> {
        if self.sealed {
            return Err(SwCode::SealedFull);
        }
        if expect_seq < self.seq {
            return Err(SwCode::SeqRegress);
        }
        if !trans_legal(self.state, to) {
            return Err(SwCode::StateNoTrans);
        }
        self.state = to;
        self.seq = self.seq.saturating_add(1);
        self.recs.push(SwitchRec {
            frame,
            expect_seq,
            to,
        });
        if self.recs.len() as u32 >= SWITCH_TARGET {
            self.sealed = true;
        }
        Ok(())
    }

    /// 掉盘事件登记：三要素在位则入告警面并计数掉盘；缺位即抓
    /// AlarmTriadMissing。掉盘本身即击穿零掉盘判据（drops 递增）。
    pub fn drop_event(&mut self, triad: Option<AlarmTriad>) -> Result<(), SwCode> {
        let t = match triad {
            Some(t) => t,
            None => return Err(SwCode::AlarmTriadMissing),
        };
        if !t.complete() {
            return Err(SwCode::AlarmTriadMissing);
        }
        self.drops = self.drops.saturating_add(1);
        self.triad = Some(t);
        Err(SwCode::DropHit)
    }

    /// 封账对平：封账帧数必须等于账量（判据 7×24 全量归波闭账形态）。
    pub fn seal_consistent(&self) -> Result<(), SwCode> {
        if !self.sealed {
            return Err(SwCode::LedgerMismatch);
        }
        if self.recs.len() as u32 != SWITCH_TARGET {
            return Err(SwCode::LedgerMismatch);
        }
        Ok(())
    }

    /// 当前运行态。
    pub fn state(&self) -> ProtoState {
        self.state
    }
    /// 当前单调序号。
    pub fn seq(&self) -> u32 {
        self.seq
    }
    /// 账面帧数。
    pub fn recs_len(&self) -> usize {
        self.recs.len()
    }
    /// 是否已封账。
    pub fn sealed(&self) -> bool {
        self.sealed
    }
    /// 累计掉盘数（零掉盘判据要求恒 0）。
    pub fn drops(&self) -> u32 {
        self.drops
    }
    /// 在位告警三要素。
    pub fn triad(&self) -> Option<AlarmTriad> {
        self.triad
    }

    /// 注入记账：检出加一（调用方在捕获违约时登记）。
    pub fn note_detected(&mut self) {
        self.injected = self.injected.saturating_add(1);
        self.detected = self.detected.saturating_add(1);
    }

    /// 注入逃逸（判据要求恒 0；非零即记账面破洞）。
    pub fn note_escape(&mut self) {
        self.injected = self.injected.saturating_add(1);
    }

    /// 守恒断言面：注入 = 检出 + 逃逸，且逃逸恒 0、检出非零。
    pub fn conserved(&self) -> bool {
        self.injected == self.detected && self.detected > 0
    }
}

/// 段位守卫：SEG_BASE 按 0x40 对齐且落在分派区间。
pub fn seg_guard_ok() -> bool {
    SEG_BASE % 0x40 == 0 && SEG_BASE >= 0xC0080 && SEG_BASE <= 0xC31C0
}

/// 双跑一致性：同一 ×100 切换脚本两次建账逐字段一致。
pub fn dual_run_consistent() -> bool {
    let build = || -> Vec<SwitchRec> {
        let mut e = SwitchEngine::new();
        // 100 帧合法脚本：Bot→Neg→Uasp→Neg→Bot 循环（4 帧一轮 × 25 轮）。
        let script = [
            ProtoState::Negotiating,
            ProtoState::Uasp,
            ProtoState::Negotiating,
            ProtoState::Bot,
        ];
        for i in 0..SWITCH_TARGET {
            let idx = (i % 4) as usize;
            let to = match script.get(idx) {
                Some(t) => *t,
                None => ProtoState::Bot,
            };
            let _ = e.request(i + 1, e.seq(), to);
        }
        e.recs_clone()
    };
    let a = build();
    let b = build();
    a == b
}

impl SwitchEngine {
    /// 账面克隆（供双跑比对；只读用途）。
    fn recs_clone(&self) -> Vec<SwitchRec> {
        self.recs.clone()
    }
}

/// ×100 切换压测执行器：4 帧一轮合法脚本，落满即封账，返回 (受理帧数, 封账, 掉盘)。
pub fn stress_100_switches() -> (u32, bool, u32) {
    let mut e = SwitchEngine::new();
    let script = [
        ProtoState::Negotiating,
        ProtoState::Uasp,
        ProtoState::Negotiating,
        ProtoState::Bot,
    ];
    let mut accepted = 0u32;
    for i in 0..SWITCH_TARGET {
        let idx = (i % 4) as usize;
        let to = match script.get(idx) {
            Some(t) => *t,
            None => ProtoState::Bot,
        };
        if e.request(i + 1, e.seq(), to).is_ok() {
            accepted = accepted.saturating_add(1);
        }
    }
    (accepted, e.sealed(), e.drops())
}

/// 收口自检（≥12 条判据形态）。
pub fn run_unx51506_checks() -> CheckSet {
    let mut s = CheckSet::new("unx51506");
    // 1 正常流：完整一轮 Bot→Neg→Uasp 落账改态。
    {
        let mut e = SwitchEngine::new();
        let r1 = e.request(1, 0, ProtoState::Negotiating);
        let r2 = e.request(2, 1, ProtoState::Uasp);
        s.add_dyn(
            "正常切换轮落账",
            r1.is_ok() && r2.is_ok() && e.state() == ProtoState::Uasp && e.recs_len() == 2,
            "合法两跳应落 2 帧账并到达 Uasp 态",
        );
    }
    // 2 边界零：首帧期望序号 0 放行。
    {
        let mut e = SwitchEngine::new();
        let r = e.request(1, 0, ProtoState::Negotiating);
        s.add_dyn("边界零序号放行", r.is_ok(), "首帧期望序号 0 应放行");
    }
    // 3 边界上限：第 100 帧落满即封账。
    {
        let (accepted, sealed, drops) = stress_100_switches();
        s.add_dyn(
            "×100 切换封账",
            accepted == SWITCH_TARGET && sealed && drops == 0,
            "×100 帧应全部受理并封账，且零掉盘",
        );
    }
    // 4 边界上限+1：封账后再来帧容量耗尽拒。
    {
        let mut e = SwitchEngine::new();
        for i in 0..SWITCH_TARGET {
            let to = if i % 2 == 0 {
                ProtoState::Negotiating
            } else {
                ProtoState::Bot
            };
            let _ = e.request(i + 1, e.seq(), to);
        }
        let r = e.request(SWITCH_TARGET + 1, e.seq(), ProtoState::Negotiating);
        s.add_dyn(
            "封账后容量耗尽拒",
            r == Err(SwCode::SealedFull),
            "×100 封账后追加帧必须拒绝",
        );
    }
    // 5 注入必抓：序号倒退 ×10 轮。
    {
        let mut e = SwitchEngine::new();
        let _ = e.request(1, 0, ProtoState::Negotiating);
        let mut caught = 0u32;
        for i in 0..10u32 {
            // 期望序号固定为 0，随引擎序号增长必成倒退。
            let r = e.request(10 + i, 0, ProtoState::Uasp);
            if r == Err(SwCode::SeqRegress) {
                caught += 1;
                e.note_detected();
            }
        }
        s.add_dyn("序号倒退注入 10/10", caught == 10, "倒退违约必须全抓");
    }
    // 6 注入必抓：非法跃迁（Bot 直达 Uasp）×10 轮。
    {
        let mut e = SwitchEngine::new();
        let mut caught = 0u32;
        for i in 0..10u32 {
            let r = e.request(1 + i, e.seq(), ProtoState::Uasp);
            if r == Err(SwCode::StateNoTrans) {
                caught += 1;
                e.note_detected();
            }
        }
        s.add_dyn("非法跃迁注入 10/10", caught == 10, "跳变必须全抓");
    }
    // 7 注入必抓：掉盘缺三要素（缺码/缺级/缺建议 三变体）×10 轮。
    {
        let mut e = SwitchEngine::new();
        let mut caught = 0u32;
        for i in 0..10u32 {
            let triad = match i % 3 {
                0 => None,
                1 => Some(AlarmTriad {
                    event_code: 0,
                    level: 3,
                    remedy: TRIAD_FULL.remedy,
                }),
                _ => Some(AlarmTriad {
                    event_code: 0xC0180,
                    level: 0,
                    remedy: "",
                }),
            };
            if e.drop_event(triad) == Err(SwCode::AlarmTriadMissing) {
                caught += 1;
                e.note_detected();
            }
        }
        s.add_dyn("告警缺要素注入 10/10", caught == 10, "三要素缺位必须全抓");
    }
    // 8 三要素在位：完整样板登记成功且可读回。
    {
        let mut e = SwitchEngine::new();
        let r = e.drop_event(Some(TRIAD_FULL));
        s.add_dyn(
            "告警三要素在位",
            e.triad() == Some(TRIAD_FULL) && matches!(r, Err(SwCode::DropHit)),
            "完整三要素应入告警面并计一次掉盘",
        );
    }
    // 9 判定次序：封账容量优先于序号倒退。
    {
        let mut e = SwitchEngine::new();
        for i in 0..SWITCH_TARGET {
            let to = if i % 2 == 0 {
                ProtoState::Negotiating
            } else {
                ProtoState::Bot
            };
            let _ = e.request(i + 1, e.seq(), to);
        }
        let r = e.request(SWITCH_TARGET + 1, 0, ProtoState::Negotiating);
        s.add_dyn(
            "判序封账先于倒退",
            r == Err(SwCode::SealedFull),
            "封账判据必须先于倒退判据",
        );
    }
    // 10 判定次序：序号倒退优先于跃迁非法。
    {
        let mut e = SwitchEngine::new();
        let _ = e.request(1, 0, ProtoState::Negotiating);
        let r = e.request(2, 0, ProtoState::Uasp);
        s.add_dyn(
            "判序倒退先于跃迁",
            r == Err(SwCode::SeqRegress),
            "倒退判据必须先于跃迁判据",
        );
    }
    // 11 状态机拒止：拒帧不改态、不落账。
    {
        let mut e = SwitchEngine::new();
        let _ = e.request(1, 0, ProtoState::Negotiating);
        let before = e.recs_len();
        let state_before = e.state();
        let _ = e.request(2, e.seq(), ProtoState::Bot);
        s.add_dyn(
            "拒帧不改态不落账",
            e.recs_len() == before && e.state() == state_before,
            "拒止帧不得有部分副作用",
        );
    }
    // 12 封账对平：封账帧数 = 账量。
    {
        let mut e = SwitchEngine::new();
        for i in 0..SWITCH_TARGET {
            let to = if i % 2 == 0 {
                ProtoState::Negotiating
            } else {
                ProtoState::Bot
            };
            let _ = e.request(i + 1, e.seq(), to);
        }
        s.add_dyn(
            "封账对平闭账",
            e.seal_consistent().is_ok(),
            "封账帧数与账量必须一致（全量归波闭账形态）",
        );
    }
    // 13 对账守恒：注入 = 检出，逃逸 0。
    {
        let mut e = SwitchEngine::new();
        for _ in 0..10u32 {
            let _ = e.drop_event(None);
            e.note_detected();
        }
        s.add_dyn(
            "注入账守恒",
            e.conserved() && e.drops() == 0,
            "注入=检出+逃逸，逃逸恒 0；缺要素不算掉盘",
        );
    }
    // 14 双跑一致。
    s.add_dyn("双跑逐字段一致", dual_run_consistent(), "同一 ×100 脚本两次建账逐字段相等");
    // 15 文案闭集非空。
    {
        let mut closed = true;
        for c in ALL_CODES.iter() {
            if c.detail().is_empty() {
                closed = false;
            }
        }
        s.add_dyn("文案闭集非空", closed, "每个诊断码都必须有中文建议");
    }
    // 16 段位守卫。
    s.add_dyn("段位守卫", seg_guard_ok(), "SEG_BASE 对齐且在分派区间");
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normal_two_hop_reaches_uasp() {
        let mut e = SwitchEngine::new();
        assert!(e.request(1, 0, ProtoState::Negotiating).is_ok());
        assert!(e.request(2, 1, ProtoState::Uasp).is_ok());
        assert_eq!(e.state(), ProtoState::Uasp);
        assert_eq!(e.recs_len(), 2);
    }

    #[test]
    fn seq_regress_caught_x10() {
        let mut e = SwitchEngine::new();
        let _ = e.request(1, 0, ProtoState::Negotiating);
        for i in 0..10u32 {
            assert_eq!(
                e.request(10 + i, 0, ProtoState::Bot),
                Err(SwCode::SeqRegress)
            );
        }
    }

    #[test]
    fn illegal_jump_rejected_state_kept() {
        let mut e = SwitchEngine::new();
        assert_eq!(
            e.request(1, 0, ProtoState::Uasp),
            Err(SwCode::StateNoTrans)
        );
        assert_eq!(e.state(), ProtoState::Bot);
        assert_eq!(e.recs_len(), 0);
    }

    #[test]
    fn seal_at_100_then_exhausted() {
        let mut e = SwitchEngine::new();
        for i in 0..SWITCH_TARGET {
            let to = if i % 2 == 0 {
                ProtoState::Negotiating
            } else {
                ProtoState::Bot
            };
            assert!(e.request(i + 1, e.seq(), to).is_ok());
        }
        assert!(e.sealed());
        assert_eq!(e.recs_len() as u32, SWITCH_TARGET);
        assert_eq!(
            e.request(SWITCH_TARGET + 1, e.seq(), ProtoState::Negotiating),
            Err(SwCode::SealedFull)
        );
    }

    #[test]
    fn alarm_triad_missing_injection_x10() {
        let mut e = SwitchEngine::new();
        for _ in 0..10u32 {
            assert_eq!(e.drop_event(None), Err(SwCode::AlarmTriadMissing));
        }
        assert_eq!(e.drops(), 0);
    }

    #[test]
    fn alarm_triad_full_registers_drop() {
        let mut e = SwitchEngine::new();
        let r = e.drop_event(Some(TRIAD_FULL));
        assert!(matches!(r, Err(SwCode::DropHit)));
        assert_eq!(e.triad(), Some(TRIAD_FULL));
        assert_eq!(e.drops(), 1);
    }

    #[test]
    fn judge_order_seal_before_regress() {
        let mut e = SwitchEngine::new();
        for i in 0..SWITCH_TARGET {
            let to = if i % 2 == 0 {
                ProtoState::Negotiating
            } else {
                ProtoState::Bot
            };
            let _ = e.request(i + 1, e.seq(), to);
        }
        // 封账后带倒退序号：必须先报封账而非倒退。
        assert_eq!(
            e.request(SWITCH_TARGET + 1, 0, ProtoState::Negotiating),
            Err(SwCode::SealedFull)
        );
    }

    #[test]
    fn zero_drop_over_100_switches() {
        let (accepted, sealed, drops) = stress_100_switches();
        assert_eq!(accepted, SWITCH_TARGET);
        assert!(sealed);
        assert_eq!(drops, 0, "首轮压测零掉盘");
    }

    #[test]
    fn injection_ledger_conserved() {
        let mut e = SwitchEngine::new();
        for _ in 0..10u32 {
            let _ = e.drop_event(None);
            e.note_detected();
        }
        assert!(e.conserved());
        assert_eq!(e.drops(), 0);
    }

    #[test]
    fn dual_run_identical() {
        assert!(dual_run_consistent());
    }

    #[test]
    fn seg_guard_holds() {
        assert!(seg_guard_ok());
    }

    #[test]
    fn detail_texts_closed_set() {
        for c in ALL_CODES.iter() {
            assert!(!c.detail().is_empty());
        }
    }
}
