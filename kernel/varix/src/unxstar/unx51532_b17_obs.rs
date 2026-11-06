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

// __PART2__
