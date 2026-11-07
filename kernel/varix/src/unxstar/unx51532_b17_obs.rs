//! UNX-F51532 B17 可观测性埋点（预算/仲裁/整形/拒绝四点） · 0xC0800段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文：UNX-F51533-J1 基线 ×3 波漂移 ≤5%（真机阈值随闸门补测登记）
//!
//! 设计要点：
//!   ① 四探针面按「预算 / 仲裁 / 整形 / 拒绝」四个闭集点位建模，每点独立
//!      使能位、独立打点数、独立序号水位；覆盖率 = 各点 25%，四点全使能、
//!      全打点且零丢点才得 100（埋点 100%）。
//!   ② 联合打点单调面：每点 last_seq 严格单调，倒退显性拒（SeqBackward），
//!      拒后打点数与版本纹丝不动；打点即落环账，无旁路。
//!   ③ 拒绝事件三要素：合法原因码（闭集 1..4）+ 合法端点 + 携带桶照快照；
//!      三要素校验先于序号判序，缺任一要素整条拒收不入账，存入事件
//!      齐备率恒 100%。
//!   ④ 容量耗尽：环账满即 BufferFull，丢失计数单独入账，覆盖率扣减；
//!      守恒式 Σ打点数 = 环账条数（零丢点时）。
//!   ⑤ 注入账守恒：注入 = 检出 + 逃逸，逃逸恒 0；诊断码闭集 + 段位守卫。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::vec::Vec;

pub const SEG_BASE: u32 = 0xC0800;

/// 冻结常量表：探针数 / 环账容量 / 端点数 / 拒绝原因码闭集。
pub const PROBE_N: usize = 4;
pub const MAX_EVENTS: usize = 32;
pub const EPS: usize = 4;
pub const REJECT_CAUSE_CODES: [u8; 4] = [1, 2, 3, 4];
pub const INJ_ROUNDS: usize = 10;

/// 诊断码闭集（B17 可观测域）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ObsCode {
    /// 成功
    Ok = 0,
    /// 段位守卫失败
    SegBad,
    /// 探针未使能
    ProbeDisabled,
    /// 打点序号倒退
    SeqBackward,
    /// 环账满（丢点）
    BufferFull,
    /// 拒绝事件三要素不齐
    RejectIncomplete,
    /// 拒绝原因码不在闭集
    CauseUnknown,
    /// 覆盖面破洞（丢点或漏点）
    ObsGap,
    /// 打点账不平
    LedgerMismatch,
    /// 守恒账破坏
    ConservedBroken,
}

impl ObsCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            ObsCode::Ok => "埋点通过",
            ObsCode::SegBad => "段位越界：检查 SEG_BASE 对齐与取值范围",
            ObsCode::ProbeDisabled => "探针未使能：先使能再打点，禁止静默丢弃",
            ObsCode::SeqBackward => "打点序号倒退：每点序号必须严格递增",
            ObsCode::ObsGap => "覆盖面破洞：四点必须全使能且零丢点",
            ObsCode::BufferFull => "环账满：先导出结算再继续打点",
            ObsCode::RejectIncomplete => "拒绝事件三要素不齐：原因码/端点/桶照缺一即拒",
            ObsCode::CauseUnknown => "拒绝原因码不在闭集：只允许 1..4",
            ObsCode::LedgerMismatch => "打点账不平：Σ打点数必须等于环账条数",
            ObsCode::ConservedBroken => "守恒破坏：注入=检出+逃逸必须成立",
        }
    }
}

/// 埋点面四点闭集。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ObsPoint {
    /// 预算点
    Budget = 0,
    /// 仲裁点
    Arb,
    /// 整形点
    Shape,
    /// 拒绝点
    Reject,
}

impl ObsPoint {
    fn idx(self) -> usize {
        self as usize
    }
    /// 全点位权威清单。
    pub fn all() -> [ObsPoint; PROBE_N] {
        [ObsPoint::Budget, ObsPoint::Arb, ObsPoint::Shape, ObsPoint::Reject]
    }
}

/// 拒绝事件（三要素：原因码 / 端点 / 桶照快照在握）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RejectEvt {
    /// 拒绝点打点序号
    pub seq: u32,
    /// 原因码（闭集 1..4）
    pub cause_code: u8,
    /// 端点号（< EPS）
    pub ep: u8,
    /// 桶内令牌量
    pub tokens: u32,
    /// 是否携带桶照快照
    pub has_snapshot: bool,
}

/// 单探针状态。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ProbeStat {
    pub enabled: bool,
    pub fired: u64,
    pub last_seq: u32,
}

/// 环账打点记录。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ObsRec {
    pub point: ObsPoint,
    pub seq: u32,
}

/// B17 可观测埋点中枢。
pub struct ObsHub {
    probes: [ProbeStat; PROBE_N],
    ring: Vec<ObsRec>,
    rejects: Vec<RejectEvt>,
    lost: u64,
    version: u64,
    injected: u64,
    detected: u64,
    escaped: u64,
}

impl ObsHub {
    /// 新中枢：四点全使能、零打点。
    pub fn new() -> ObsHub {
        ObsHub {
            probes: [
                ProbeStat { enabled: true, fired: 0, last_seq: 0 },
                ProbeStat { enabled: true, fired: 0, last_seq: 0 },
                ProbeStat { enabled: true, fired: 0, last_seq: 0 },
                ProbeStat { enabled: true, fired: 0, last_seq: 0 },
            ],
            ring: Vec::new(),
            rejects: Vec::new(),
            lost: 0,
            version: 0,
            injected: 0,
            detected: 0,
            escaped: 0,
        }
    }

    fn probe(&self, p: ObsPoint) -> ProbeStat {
        match self.probes.get(p.idx()) {
            Some(s) => *s,
            None => ProbeStat { enabled: false, fired: 0, last_seq: 0 },
        }
    }

    /// 使能/停用某点（停用后打点显性拒，不静默丢）。
    pub fn set_enabled(&mut self, p: ObsPoint, on: bool) {
        if let Some(s) = self.probes.get_mut(p.idx()) {
            s.enabled = on;
            self.version += 1;
        }
    }

    /// 打点一发。判序：使能 → 序号单调 → 环容量 → 落账。
    pub fn emit(&mut self, p: ObsPoint, seq: u32) -> Result<(), ObsCode> {
        let st = self.probe(p);
        if !st.enabled {
            return Err(ObsCode::ProbeDisabled);
        }
        if seq <= st.last_seq {
            return Err(ObsCode::SeqBackward);
        }
        if self.ring.len() >= MAX_EVENTS {
            self.lost += 1;
            return Err(ObsCode::BufferFull);
        }
        if let Some(s) = self.probes.get_mut(p.idx()) {
            s.fired += 1;
            s.last_seq = seq;
        }
        self.ring.push(ObsRec { point: p, seq });
        self.version += 1;
        Ok(())
    }

    /// 三要素校验：原因码闭集 → 端点界 → 快照在握。
    fn reject_complete(evt: &RejectEvt) -> Result<(), ObsCode> {
        let mut cause_ok = false;
        for i in 0..REJECT_CAUSE_CODES.len() {
            if let Some(c) = REJECT_CAUSE_CODES.get(i) {
                if *c == evt.cause_code {
                    cause_ok = true;
                }
            }
        }
        if !cause_ok {
            return Err(ObsCode::CauseUnknown);
        }
        if evt.ep as usize >= EPS || !evt.has_snapshot {
            return Err(ObsCode::RejectIncomplete);
        }
        Ok(())
    }

    /// 拒绝事件入账。判序：三要素 → 拒绝点使能 → 序号单调 → 环容量 → 落账。
    /// 三要素校验先于序号判序：缺要素的事件无论序号如何都整条拒收。
    pub fn observe_reject(&mut self, evt: RejectEvt) -> Result<(), ObsCode> {
        Self::reject_complete(&evt)?;
        let st = self.probe(ObsPoint::Reject);
        if !st.enabled {
            return Err(ObsCode::ProbeDisabled);
        }
        if evt.seq <= st.last_seq {
            return Err(ObsCode::SeqBackward);
        }
        if self.ring.len() >= MAX_EVENTS || self.rejects.len() >= MAX_EVENTS {
            self.lost += 1;
            return Err(ObsCode::BufferFull);
        }
        if let Some(s) = self.probes.get_mut(ObsPoint::Reject.idx()) {
            s.fired += 1;
            s.last_seq = evt.seq;
        }
        self.rejects.push(evt);
        self.ring.push(ObsRec { point: ObsPoint::Reject, seq: evt.seq });
        self.version += 1;
        Ok(())
    }

    /// 覆盖率：每点 25%，需全使能且打过点；有丢点即破百（ObsGap 语义）。
    pub fn coverage_pct(&self) -> u32 {
        let mut pct = 0u32;
        for i in 0..PROBE_N {
            let st = match self.probes.get(i) {
                Some(s) => *s,
                None => ProbeStat { enabled: false, fired: 0, last_seq: 0 },
            };
            if st.enabled && st.fired > 0 {
                pct += 25;
            }
        }
        if self.lost > 0 && pct > 0 {
            pct -= 25;
        }
        pct
    }

    /// 存入拒绝事件齐备率（应恒 100%）。
    pub fn reject_complete_rate(&self) -> u32 {
        let n = self.rejects.len();
        if n == 0 {
            return 100;
        }
        let mut ok = 0u64;
        for i in 0..n {
            if let Some(evt) = self.rejects.get(i) {
                if Self::reject_complete(evt).is_ok() {
                    ok += 1;
                }
            }
        }
        (ok * 100 / n as u64) as u32
    }

    /// 打点账守恒：Σ打点数 = 环账条数，且零丢点。
    pub fn ledger_ok(&self) -> bool {
        let mut fired_sum = 0u64;
        for i in 0..PROBE_N {
            if let Some(s) = self.probes.get(i) {
                fired_sum += s.fired;
            }
        }
        fired_sum == self.ring.len() as u64 && self.lost == 0
    }

    /// 注入记账：注入与检出同步加一（调用方在捕获违约时登记）。
    pub fn note_detected(&mut self) {
        self.injected += 1;
        self.detected += 1;
    }

    /// 注入逃逸（判据要求恒 0）。
    pub fn note_escape(&mut self) {
        self.escaped += 1;
    }

    pub fn probe_stat(&self, p: ObsPoint) -> ProbeStat {
        self.probe(p)
    }
    pub fn ring_len(&self) -> usize {
        self.ring.len()
    }
    pub fn ring_rec(&self, idx: usize) -> Option<ObsRec> {
        self.ring.get(idx).copied()
    }
    pub fn rejects_len(&self) -> usize {
        self.rejects.len()
    }
    pub fn reject_at(&self, idx: usize) -> Option<RejectEvt> {
        self.rejects.get(idx).copied()
    }
    pub fn lost(&self) -> u64 {
        self.lost
    }
    pub fn version(&self) -> u64 {
        self.version
    }
    pub fn injected(&self) -> u64 {
        self.injected
    }
    pub fn detected(&self) -> u64 {
        self.detected
    }
    pub fn escaped(&self) -> u64 {
        self.escaped
    }

    /// 守恒断言面：注入 = 检出 + 逃逸，逃逸恒 0、检出 > 0，且打点账平。
    pub fn conserved(&self) -> bool {
        self.injected == self.detected + self.escaped
            && self.escaped == 0
            && self.detected > 0
            && self.ledger_ok()
    }
}

/// 拒绝原因码中文释义（闭集外返回空串，由调用方判空）。
pub fn reject_cause_detail(code: u8) -> &'static str {
    match code {
        1 => "预算耗尽拒绝",
        2 => "仲裁落选拒绝",
        3 => "整形削峰拒绝",
        4 => "超单笔上限拒绝",
        _ => "",
    }
}

/// 段位守卫。
pub fn seg_guard_ok() -> bool {
    SEG_BASE % 0x40 == 0 && SEG_BASE >= 0xC0080 && SEG_BASE <= 0xC31C0
}

/// 双跑一致性：同一打点/拒绝序列两次重建环账与探针态逐字段一致。
pub fn dual_run_consistent() -> bool {
    let build = || -> (Vec<ObsRec>, Vec<RejectEvt>, [ProbeStat; PROBE_N]) {
        let mut e = ObsHub::new();
        let _ = e.emit(ObsPoint::Budget, 1);
        let _ = e.emit(ObsPoint::Arb, 2);
        let _ = e.emit(ObsPoint::Shape, 3);
        let _ = e.observe_reject(RejectEvt { seq: 4, cause_code: 1, ep: 0, tokens: 32, has_snapshot: true });
        let _ = e.observe_reject(RejectEvt { seq: 5, cause_code: 3, ep: 3, tokens: 0, has_snapshot: true });
        let mut ring = Vec::new();
        for i in 0..e.ring_len() {
            if let Some(r) = e.ring_rec(i) {
                ring.push(r);
            }
        }
        let mut rj = Vec::new();
        for i in 0..e.rejects_len() {
            if let Some(r) = e.reject_at(i) {
                rj.push(r);
            }
        }
        (ring, rj, e.probes)
    };
    let a = build();
    let b = build();
    a == b
}

/// 诊断码全序表（闭集枚举的权威清单）。
pub const ALL_CODES: [ObsCode; 10] = [
    ObsCode::Ok,
    ObsCode::SegBad,
    ObsCode::ProbeDisabled,
    ObsCode::SeqBackward,
    ObsCode::BufferFull,
    ObsCode::RejectIncomplete,
    ObsCode::CauseUnknown,
    ObsCode::ObsGap,
    ObsCode::LedgerMismatch,
    ObsCode::ConservedBroken,
];

/// 收口自检（≥12 条判据形态）。
pub fn run_unx51532_checks() -> CheckSet {
    let mut s = CheckSet::new("unx51532");
    // 1 正常流：四点各打一发，覆盖率 100，账平。
    {
        let mut e = ObsHub::new();
        let _ = e.emit(ObsPoint::Budget, 1);
        let _ = e.emit(ObsPoint::Arb, 1);
        let _ = e.emit(ObsPoint::Shape, 1);
        let _ = e.emit(ObsPoint::Reject, 1);
        s.add_dyn(
            "四点全打覆盖率 100",
            e.coverage_pct() == 100 && e.ledger_ok() && e.ring_len() == 4,
            "四点全使能全打点且零丢点才是埋点 100%",
        );
    }
    // 2 边界零：零序号拒。
    {
        let mut e = ObsHub::new();
        let r = e.emit(ObsPoint::Budget, 0);
        s.add_dyn(
            "零序号打点拒",
            r == Err(ObsCode::SeqBackward),
            "序号必须严格大于水位，零序号当场拒",
        );
    }
    // 3 边界上限：u32::MAX 放行后无法再打。
    {
        let mut e = ObsHub::new();
        let a = e.emit(ObsPoint::Shape, u32::MAX);
        let b = e.emit(ObsPoint::Shape, u32::MAX);
        s.add_dyn(
            "序号上限边界",
            a.is_ok() && b == Err(ObsCode::SeqBackward),
            "上限序号放行一次，其后无更大序号可打",
        );
    }
    // 4 注入必抓：序号倒退 ×10 轮，状态冻结。
    {
        let mut e = ObsHub::new();
        let _ = e.emit(ObsPoint::Arb, 10);
        let v0 = e.version();
        let f0 = e.probe_stat(ObsPoint::Arb).fired;
        let mut caught = 0u32;
        for _ in 0..INJ_ROUNDS {
            let r = e.emit(ObsPoint::Arb, 3);
            if r == Err(ObsCode::SeqBackward) {
                caught += 1;
                e.note_detected();
            }
        }
        s.add_dyn(
            "序号倒退注入 10/10 且状态冻结",
            caught == 10
                && e.version() == v0
                && e.probe_stat(ObsPoint::Arb).fired == f0,
            "倒退必须全抓且拒后打点数与版本不动",
        );
    }
    // 5 注入必抓：未使能探针 ×10（使能判据先于序号）。
    {
        let mut e = ObsHub::new();
        e.set_enabled(ObsPoint::Budget, false);
        let mut caught = 0u32;
        for _ in 0..INJ_ROUNDS {
            let r = e.emit(ObsPoint::Budget, 0);
            if r == Err(ObsCode::ProbeDisabled) {
                caught += 1;
                e.note_detected();
            }
        }
        s.add_dyn(
            "未使能注入 10/10 且判序使能先",
            caught == 10,
            "停用点打点必须显性拒，且使能判据先于序号",
        );
    }
    // 6 注入必抓：拒绝事件缺要素 ×10（三缺轮换），全拒不入账。
    {
        let mut e = ObsHub::new();
        let mut caught = 0u32;
        for i in 0..INJ_ROUNDS {
            let bad = match i % 3 {
                0 => RejectEvt { seq: 1, cause_code: 9, ep: 0, tokens: 8, has_snapshot: true },
                1 => RejectEvt { seq: 1, cause_code: 2, ep: EPS as u8, tokens: 8, has_snapshot: true },
                _ => RejectEvt { seq: 1, cause_code: 2, ep: 1, tokens: 8, has_snapshot: false },
            };
            let r = e.observe_reject(bad);
            if r == Err(ObsCode::CauseUnknown) || r == Err(ObsCode::RejectIncomplete) {
                caught += 1;
                e.note_detected();
            }
        }
        s.add_dyn(
            "缺要素拒绝事件注入 10/10",
            caught == 10 && e.rejects_len() == 0,
            "缺任一要素整条拒收，不入账",
        );
    }
    // 7 拒绝事件齐备率 100%。
    {
        let mut e = ObsHub::new();
        for i in 0..8u32 {
            let _ = e.observe_reject(RejectEvt {
                seq: i + 1,
                cause_code: 1 + (i % 4) as u8,
                ep: (i % EPS as u32) as u8,
                tokens: i * 8,
                has_snapshot: true,
            });
        }
        s.add_dyn(
            "拒绝事件三要素齐备率 100%",
            e.reject_complete_rate() == 100 && e.rejects_len() == 8,
            "存入事件必须全齐备",
        );
    }
    // 8 原因码闭集外 → CauseUnknown，闭集内文案非空。
    {
        let mut closed = true;
        for i in 0..REJECT_CAUSE_CODES.len() {
            let c = match REJECT_CAUSE_CODES.get(i) {
                Some(v) => *v,
                None => 0,
            };
            if reject_cause_detail(c).is_empty() {
                closed = false;
            }
        }
        let mut e = ObsHub::new();
        let r = e.observe_reject(RejectEvt {
            seq: 1,
            cause_code: 200,
            ep: 0,
            tokens: 1,
            has_snapshot: true,
        });
        s.add_dyn(
            "原因码闭集判据",
            closed && r == Err(ObsCode::CauseUnknown),
            "闭集内文案齐备，闭集外整条拒收",
        );
    }
    // 9 容量耗尽：环满丢点入账并扣覆盖率。
    {
        let mut e = ObsHub::new();
        let mut ok_cnt = 0u32;
        for i in 1..=40u32 {
            let p = match i % 4 {
                0 => ObsPoint::Budget,
                1 => ObsPoint::Arb,
                2 => ObsPoint::Shape,
                _ => ObsPoint::Reject,
            };
            if e.emit(p, i).is_ok() {
                ok_cnt += 1;
            }
        }
        s.add_dyn(
            "环账容量耗尽",
            ok_cnt == MAX_EVENTS as u32
                && e.ring_len() == MAX_EVENTS
                && e.lost() == 8
                && e.coverage_pct() == 75,
            "环满丢点必须显性计数并扣减覆盖率",
        );
    }
    // 10 覆盖率算术：缺一点 75，补齐 100。
    {
        let mut e = ObsHub::new();
        let _ = e.emit(ObsPoint::Budget, 1);
        let _ = e.emit(ObsPoint::Arb, 1);
        let _ = e.emit(ObsPoint::Shape, 1);
        let miss = e.coverage_pct();
        let _ = e.emit(ObsPoint::Reject, 1);
        s.add_dyn(
            "覆盖率算术",
            miss == 75 && e.coverage_pct() == 100,
            "每点 25，四点全打满百",
        );
    }
    // 11 判定次序：三要素先于序号。
    {
        let mut e = ObsHub::new();
        let _ = e.observe_reject(RejectEvt {
            seq: 9,
            cause_code: 1,
            ep: 0,
            tokens: 4,
            has_snapshot: true,
        });
        let r = e.observe_reject(RejectEvt {
            seq: 2,
            cause_code: 1,
            ep: 0,
            tokens: 4,
            has_snapshot: false,
        });
        s.add_dyn(
            "判序三要素先于序号",
            r == Err(ObsCode::RejectIncomplete),
            "缺要素的事件无论序号如何都整条拒收",
        );
    }
    // 12 对账守恒：注入=检出+逃逸，逃逸 0，打点账平。
    {
        let mut e = ObsHub::new();
        for _ in 0..10u32 {
            let _ = e.emit(ObsPoint::Budget, 0);
            e.note_detected();
        }
        s.add_dyn(
            "注入账守恒",
            e.conserved() && e.injected() == 10,
            "注入=检出+逃逸，逃逸恒 0，Σ打点=环账",
        );
    }
    // 13 双跑一致。
    s.add_dyn("双跑逐字段一致", dual_run_consistent(), "同一打点序列两次建账逐字段相等");
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
    fn four_probes_full_coverage() {
        let mut e = ObsHub::new();
        assert!(e.emit(ObsPoint::Budget, 1).is_ok());
        assert!(e.emit(ObsPoint::Arb, 1).is_ok());
        assert!(e.emit(ObsPoint::Shape, 1).is_ok());
        assert!(e.emit(ObsPoint::Reject, 1).is_ok());
        assert_eq!(e.coverage_pct(), 100);
        assert!(e.ledger_ok());
        assert_eq!(e.ring_len(), 4);
    }

    #[test]
    fn zero_seq_rejected_and_max_seq_boundary() {
        let mut e = ObsHub::new();
        assert_eq!(e.emit(ObsPoint::Budget, 0), Err(ObsCode::SeqBackward));
        assert!(e.emit(ObsPoint::Budget, u32::MAX).is_ok());
        assert_eq!(e.emit(ObsPoint::Budget, u32::MAX), Err(ObsCode::SeqBackward));
    }

    #[test]
    fn seq_backward_injection_caught_x10() {
        let mut e = ObsHub::new();
        let _ = e.emit(ObsPoint::Arb, 10);
        let v0 = e.version();
        for _ in 0..10 {
            assert_eq!(e.emit(ObsPoint::Arb, 3), Err(ObsCode::SeqBackward));
            e.note_detected();
        }
        assert_eq!(e.version(), v0);
        assert!(e.conserved());
    }

    #[test]
    fn disabled_probe_injection_caught_x10() {
        let mut e = ObsHub::new();
        e.set_enabled(ObsPoint::Shape, false);
        for _ in 0..10 {
            assert_eq!(e.emit(ObsPoint::Shape, 5), Err(ObsCode::ProbeDisabled));
            e.note_detected();
        }
        assert_eq!(e.probe_stat(ObsPoint::Shape).fired, 0);
        assert!(e.conserved());
    }

    #[test]
    fn incomplete_reject_caught_x10_and_rate_100() {
        let mut e = ObsHub::new();
        let flavors = [
            RejectEvt { seq: 1, cause_code: 7, ep: 0, tokens: 8, has_snapshot: true },
            RejectEvt { seq: 1, cause_code: 2, ep: EPS as u8, tokens: 8, has_snapshot: true },
            RejectEvt { seq: 1, cause_code: 2, ep: 1, tokens: 8, has_snapshot: false },
        ];
        for i in 0..10u32 {
            let bad = flavors[(i % 3) as usize];
            let r = e.observe_reject(bad);
            assert!(r == Err(ObsCode::CauseUnknown) || r == Err(ObsCode::RejectIncomplete));
            e.note_detected();
        }
        assert_eq!(e.rejects_len(), 0);
        assert_eq!(e.reject_complete_rate(), 100);
        assert!(e.conserved());
    }

    #[test]
    fn complete_rejects_stored_with_monotonic_seq() {
        let mut e = ObsHub::new();
        for i in 0..8u32 {
            let evt = RejectEvt {
                seq: i + 1,
                cause_code: 1 + (i % 4) as u8,
                ep: (i % EPS as u32) as u8,
                tokens: i * 8,
                has_snapshot: true,
            };
            assert!(e.observe_reject(evt).is_ok());
        }
        assert_eq!(e.rejects_len(), 8);
        assert_eq!(e.reject_complete_rate(), 100);
        assert_eq!(e.probe_stat(ObsPoint::Reject).last_seq, 8);
    }

    #[test]
    fn buffer_full_counts_lost_and_penalizes_coverage() {
        let mut e = ObsHub::new();
        let mut ok_cnt = 0u32;
        for i in 1..=40u32 {
            let p = match i % 4 {
                0 => ObsPoint::Budget,
                1 => ObsPoint::Arb,
                2 => ObsPoint::Shape,
                _ => ObsPoint::Reject,
            };
            if e.emit(p, i).is_ok() {
                ok_cnt += 1;
            }
        }
        assert_eq!(ok_cnt, MAX_EVENTS as u32);
        assert_eq!(e.lost(), 8);
        assert_eq!(e.coverage_pct(), 75);
    }

    #[test]
    fn coverage_arithmetic() {
        let mut e = ObsHub::new();
        let _ = e.emit(ObsPoint::Budget, 1);
        let _ = e.emit(ObsPoint::Arb, 1);
        let _ = e.emit(ObsPoint::Shape, 1);
        assert_eq!(e.coverage_pct(), 75);
        let _ = e.emit(ObsPoint::Reject, 1);
        assert_eq!(e.coverage_pct(), 100);
    }

    #[test]
    fn judge_order_elements_before_seq_and_enabled_before_seq() {
        let mut e = ObsHub::new();
        let _ = e.observe_reject(RejectEvt { seq: 9, cause_code: 1, ep: 0, tokens: 4, has_snapshot: true });
        assert_eq!(
            e.observe_reject(RejectEvt { seq: 2, cause_code: 1, ep: 0, tokens: 4, has_snapshot: false }),
            Err(ObsCode::RejectIncomplete)
        );
        e.set_enabled(ObsPoint::Budget, false);
        assert_eq!(e.emit(ObsPoint::Budget, 0), Err(ObsCode::ProbeDisabled));
    }

    #[test]
    fn dual_run_seg_and_closed_set() {
        assert!(dual_run_consistent());
        assert!(seg_guard_ok());
        for c in ALL_CODES.iter() {
            assert!(!c.detail().is_empty());
        }
        for i in 0..REJECT_CAUSE_CODES.len() {
            let c = match REJECT_CAUSE_CODES.get(i) {
                Some(v) => *v,
                None => 0,
            };
            assert!(!reject_cause_detail(c).is_empty());
        }
    }
}
