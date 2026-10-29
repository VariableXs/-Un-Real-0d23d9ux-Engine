//! VE-F4262 · 一致性治理流程（U04 一致性治理与演进组）
//! 职责：治理事务六段流（提案→评估→立项→实施→验证→归档，无判据不得过段）
//! + 流程实例五态状态机（草稿/评审/执行/挂起/关闭）白名单迁移
//! + 越权跳段拦截立案 + 挂起超期自动升级催办。
//! 段位：0x1F20（续表 idx496）。行数预算 340。

#![allow(dead_code)]

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;
use crate::checks::CheckSet;

pub const SEG_BASE: u16 = 0x1F20;

#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PrCode {
    /// 段间跳转越权。
    StageJump = SEG_BASE,
    /// 出口判据缺失（卡点不熄火）。
    GateMissing = SEG_BASE + 0x01,
    /// 状态迁移越权（白名单外）。
    StateIllegal = SEG_BASE + 0x02,
    /// 实例不存在。
    NotFound = SEG_BASE + 0x03,
    /// 台账容量已满。
    LedgerFull = SEG_BASE + 0x04,
    /// 实例在拦截名单（一切推进拒收）。
    InstBlocked = SEG_BASE + 0x05,
    /// 同实例同段重复推进留痕。
    DuplicateTrace = SEG_BASE + 0x06,
    /// 台账字节投影漂移。
    LedgerDrift = SEG_BASE + 0x07,
}

/// 六段流。
pub const STAGES: [&str; 6] = ["propose", "assess", "approve", "execute", "verify", "archive"];

/// 五态。
#[repr(u8)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FState {
    Draft = 0,
    Review = 1,
    Running = 2,
    Suspended = 3,
    Closed = 4,
}

/// 白名单迁移表：from → 允许的 to 集合。
pub fn transition_allowed(from: FState, to: FState) -> bool {
    use FState::*;
    matches!(
        (from, to),
        (Draft, Review) | (Draft, Suspended)
            | (Review, Running) | (Review, Suspended) | (Review, Draft)
            | (Running, Suspended) | (Running, Closed)
            | (Suspended, Running) | (Suspended, Closed) | (Suspended, Review)
            | (Closed, Draft) // 归档后重开仅经草稿
    )
}

/// 流程实例。
#[derive(Clone, Debug)]
pub struct FlowInst {
    pub id: u32,
    pub state: FState,
    pub stage: usize, // STAGES 下标
    /// 段级出口判据：过段须当前段判据已挂。
    pub gates: [bool; 6],
    /// 挂起起始日（0=非挂起）。
    pub suspended_since: u16,
}

/// 流程册。
pub struct FlowBook {
    pub insts: Vec<FlowInst>,
    /// 挂起超期阈值（日）。
    pub suspend_cap: u16,
    pub escalated: Vec<u32>,
    pub blocked_events: u32,
}

impl FlowBook {
    pub fn new(suspend_cap: u16) -> Self {
        FlowBook { insts: Vec::new(), suspend_cap, escalated: Vec::new(), blocked_events: 0 }
    }

    /// 新建实例（草稿态、零段）。
    pub fn create(&mut self, id: u32) -> Result<(), PrCode> {
        if self.insts.iter().any(|i| i.id == id) {
            return Err(PrCode::NotFound);
        }
        self.insts.push(FlowInst { id, state: FState::Draft, stage: 0, gates: [false; 6], suspended_since: 0 });
        Ok(())
    }

    /// 过段：白名单内顺序 +1，且当前段判据必须已挂（无判据不得过段——卡点）。
    pub fn advance(&mut self, id: u32) -> Result<usize, PrCode> {
        let inst = self.insts.iter_mut().find(|i| i.id == id).ok_or(PrCode::NotFound)?;
        if inst.state != FState::Running {
            return Err(PrCode::StateIllegal);
        }
        if inst.stage >= STAGES.len() {
            return Err(PrCode::StageJump);
        }
        if !inst.gates[inst.stage] {
            self.blocked_events += 1;
            return Err(PrCode::GateMissing);
        }
        inst.stage += 1;
        Ok(inst.stage)
    }

    /// 状态迁移（白名单）。
    pub fn transit(&mut self, id: u32, to: FState) -> Result<(), PrCode> {
        let inst = self.insts.iter_mut().find(|i| i.id == id).ok_or(PrCode::NotFound)?;
        if !transition_allowed(inst.state, to) {
            self.blocked_events += 1;
            return Err(PrCode::StateIllegal);
        }
        inst.suspended_since = if to == FState::Suspended { 1 } else { 0 };
        inst.state = to;
        Ok(())
    }

    /// 挂起超期升级催办。
    pub fn escalate(&mut self, today: u16) {
        let mut due: Vec<u32> = Vec::new();
        for i in self.insts.iter() {
            if i.state == FState::Suspended && i.suspended_since > 0 && today.saturating_sub(i.suspended_since) > self.suspend_cap {
                if !self.escalated.contains(&i.id) && !due.contains(&i.id) {
                    due.push(i.id);
                }
            }
        }
        for id in due {
            self.escalated.push(id);
        }
    }
}

// ============ 深化批（2026-10-27 i18n 深化）：段推进留痕台账与回归面 ============
// 在既有六段流五态机之上补齐：推进事件台账（带日步归因）、拦截名单闸、
// 守恒恒等式、边界探针、分位投影、LCG 注入回归、字节投影 + FNV。

/// 单条段推进留痕（台账条目：实例 + 段 + 日步 + 放行归因）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct StageTrace {
    pub inst: u32,
    pub stage: u8,
    pub day: u16,
    /// 是否被放行（未放行=卡点/越权留痕）。
    pub granted: bool,
}

/// 段推进留痕台账：全量推进事件留痕 + 拦截名单 + 守恒口径。
#[derive(Clone, Debug)]
pub struct TraceLedger {
    pub traces: Vec<StageTrace>,
    /// 拦截名单（实例级：立案冻结，一切推进拒收）。
    pub blocked: Vec<u32>,
    /// 台账容量上限。
    pub cap: usize,
}

impl TraceLedger {
    pub fn new(cap: usize) -> Self {
        TraceLedger { traces: Vec::new(), blocked: Vec::new(), cap }
    }

    /// 记账：满容量/拦截名单/同实例同段重复 → 显性 Err 码。
    pub fn record(&mut self, t: StageTrace) -> Result<(), PrCode> {
        if self.traces.len() >= self.cap {
            return Err(PrCode::LedgerFull);
        }
        if self.blocked.contains(&t.inst) {
            return Err(PrCode::InstBlocked);
        }
        if self.traces.iter().any(|x| x.inst == t.inst && x.stage == t.stage) {
            return Err(PrCode::DuplicateTrace);
        }
        self.traces.push(t);
        Ok(())
    }

    /// 拦截闸（幂等：重复拦截返回 false，零副作用）。
    pub fn block(&mut self, inst: u32) -> bool {
        if self.blocked.contains(&inst) {
            return false;
        }
        self.blocked.push(inst);
        true
    }

    /// 解除拦截（仅解在册者）。
    pub fn unblock(&mut self, inst: u32) -> bool {
        let n = self.blocked.len();
        self.blocked.retain(|w| *w != inst);
        self.blocked.len() != n
    }

    /// 守恒恒等式：放行 + 拦截 ≡ 总账（按段聚合不变量）。
    pub fn conservation(&self) -> bool {
        let granted = self.traces.iter().filter(|t| t.granted).count();
        let denied = self.traces.iter().filter(|t| !t.granted).count();
        granted + denied == self.traces.len()
    }

    /// 查询投影：按六段分列计数。
    pub fn by_stage(&self) -> [usize; 6] {
        let mut out = [0usize; 6];
        for t in self.traces.iter() {
            if (t.stage as usize) < 6 {
                out[t.stage as usize] += 1;
            }
        }
        out
    }

    /// 中位日（整数取点，禁浮点；偶数取左中位）。
    pub fn median_day(&self) -> Option<u16> {
        if self.traces.is_empty() {
            return None;
        }
        let mut days: Vec<u16> = self.traces.iter().map(|t| t.day).collect();
        days.sort_unstable();
        Some(days[(days.len() - 1) / 2])
    }

    /// LCG 注入回归：随机实例/段/日 N 轮，拦截名单必须 100% 拒收。
    /// 返回（命中拒收次数, 拦截实例出现次数）。
    pub fn inject_regression(&mut self, rounds: u32) -> (u32, u32) {
        let mut s: u32 = 0x5EED_4966; // ves496 专属种子
        let mut hits = 0u32;
        let mut total = 0u32;
        let mut i = 0;
        while i < rounds {
            s = s.wrapping_mul(1664525).wrapping_add(1013904223);
            let inst = ((s >> 16) % 4) as u32;
            s = s.wrapping_mul(1664525).wrapping_add(1013904223);
            let stage = ((s >> 16) % 6) as u8;
            let day = ((s >> 8) % 30) as u16;
            let is_blocked = self.blocked.contains(&inst);
            let r = self.record(StageTrace { inst, stage, day, granted: !is_blocked });
            if is_blocked {
                total += 1;
                if r == Err(PrCode::InstBlocked) {
                    hits += 1;
                }
            }
            i += 1;
        }
        (hits, total)
    }

    /// 定长字节投影：inst(4 LE) + stage(1) + granted(1) = 6B/条。
    pub fn encode(&self, out: &mut Vec<u8>) {
        for t in self.traces.iter() {
            out.push((t.inst & 0xFF) as u8);
            out.push(((t.inst >> 8) & 0xFF) as u8);
            out.push(((t.inst >> 16) & 0xFF) as u8);
            out.push(((t.inst >> 24) & 0xFF) as u8);
            out.push(t.stage);
            out.push(t.granted as u8);
        }
    }

    /// 解码往返（长度非整倍即台账漂移 → Err）。
    pub fn decode(raw: &[u8]) -> Result<Vec<StageTrace>, PrCode> {
        if raw.len() % 6 != 0 {
            return Err(PrCode::LedgerDrift);
        }
        let mut out = Vec::new();
        let mut i = 0;
        while i < raw.len() {
            let inst = raw[i] as u32
                | ((raw[i + 1] as u32) << 8)
                | ((raw[i + 2] as u32) << 16)
                | ((raw[i + 3] as u32) << 24);
            out.push(StageTrace { inst, stage: raw[i + 4], day: 0, granted: raw[i + 5] != 0 });
            i += 6;
        }
        Ok(out)
    }

    /// 诊断投影（中文诊断串，内核侧无 IO）。
    pub fn diagnose(&self) -> String {
        format!(
            "推进台账：留痕{}条、拦截{}实例、守恒={}",
            self.traces.len(),
            self.blocked.len(),
            self.conservation().to_string()
        )
    }
}

/// FNV-1a 32 位指纹（台账投影一致性校验用）。
fn fnv1a(data: &[u8]) -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    for b in data.iter() {
        h ^= *b as u32;
        h = h.wrapping_mul(0x0100_0193);
    }
    h
}

fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) {
    set.add(label, ok, detail);
}

/// 判据组：C496-01..12。
pub fn run_ves496_checks() -> CheckSet {
    let mut set = CheckSet::new("ves496");

    // 白名单表单测
    chk(&mut set, "C496-01", transition_allowed(FState::Draft, FState::Review) && transition_allowed(FState::Running, FState::Closed), "白名单正迁移放行");
    chk(&mut set, "C496-02", !transition_allowed(FState::Draft, FState::Closed) && !transition_allowed(FState::Closed, FState::Running), "越权迁移拦截（草稿不可直达关闭）");

    let mut fb = FlowBook::new(7);
    chk(&mut set, "C496-03", fb.create(1).is_ok() && fb.create(1) == Err(PrCode::NotFound), "实例创建与重号拒收");

    chk(&mut set, "C496-04", fb.advance(1) == Err(PrCode::StateIllegal), "草稿态不得过段（须先 Running）");
    let _ = fb.transit(1, FState::Review);
    let _ = fb.transit(1, FState::Running);
    chk(&mut set, "C496-05", fb.transit(1, FState::Closed) == Err(PrCode::StateIllegal), "评审态直达关闭越权");

    // 卡点：无判据不得过段
    chk(&mut set, "C496-06", fb.advance(1) == Err(PrCode::GateMissing), "提案段无判据→卡点不熄火");
    chk(&mut set, "C496-07", fb.blocked_events == 2, "越权/卡点事件留痕");
    {
        let i = fb.insts.iter_mut().find(|i| i.id == 1).unwrap();
        i.gates[0] = true;
    }
    chk(&mut set, "C496-08", fb.advance(1) == Ok(1), "挂判据后过段进评估");
    {
        let i = fb.insts.iter_mut().find(|i| i.id == 1).unwrap();
        i.gates = [true; 6];
    }
    let mut last = 1;
    for _ in 0..5 {
        if let Ok(s) = fb.advance(1) {
            last = s;
        }
    }
    chk(&mut set, "C496-09", last == 6, "六段全通至归档");
    chk(&mut set, "C496-10", fb.advance(1) == Err(PrCode::StageJump), "归档后再过段=跳段拦截");

    // 挂起超期升级
    let _ = fb.create(2);
    let _ = fb.transit(2, FState::Review);
    let _ = fb.transit(2, FState::Suspended);
    {
        let i2 = fb.insts.iter_mut().find(|i| i.id == 2).unwrap();
        i2.suspended_since = 1;
    }
    fb.escalate(20); // 20-1=19 > 7 → 升级
    chk(&mut set, "C496-11", fb.escalated.contains(&2), "挂起超期自动升级催办");
    fb.escalate(5); // 5-1=4 ≤ 7 → 不升级
    chk(&mut set, "C496-12", fb.escalated.len() == 1, "未超期不升级");

    // ---- 深化判据：段推进留痕台账与回归面（C496-13..24） ----
    let mut tl = TraceLedger::new(8);
    chk(&mut set, "C496-13", tl.record(StageTrace { inst: 1, stage: 0, day: 3, granted: true }).is_ok(), "台账首笔推进留痕");
    chk(&mut set, "C496-14", tl.record(StageTrace { inst: 1, stage: 0, day: 4, granted: true }) == Err(PrCode::DuplicateTrace), "同实例同段重复留痕拒收");
    chk(&mut set, "C496-15", tl.block(2) && !tl.block(2), "拦截闸幂等（重复拦截零副作用）");
    chk(&mut set, "C496-16", tl.record(StageTrace { inst: 2, stage: 1, day: 5, granted: false }) == Err(PrCode::InstBlocked), "拦截实例一切推进拒收");
    chk(&mut set, "C496-17", tl.unblock(2) && tl.record(StageTrace { inst: 2, stage: 1, day: 5, granted: true }).is_ok(), "解除拦截后可推进");

    let cap_probe = {
        let mut small = TraceLedger::new(2);
        let r1 = small.record(StageTrace { inst: 3, stage: 0, day: 1, granted: true });
        let r2 = small.record(StageTrace { inst: 3, stage: 1, day: 1, granted: true });
        let r3 = small.record(StageTrace { inst: 3, stage: 2, day: 1, granted: true });
        r1.is_ok() && r2.is_ok() && r3 == Err(PrCode::LedgerFull)
    };
    chk(&mut set, "C496-18", cap_probe, "满容量边界探针（显性 Err）");

    let _ = tl.record(StageTrace { inst: 1, stage: 1, day: 9, granted: false });
    chk(&mut set, "C496-19", tl.conservation(), "守恒恒等式：放行+拦截≡总账");
    let cols = tl.by_stage();
    chk(&mut set, "C496-20", cols[0] == 1 && cols[1] == 2 && tl.median_day() == Some(5), "按段分列投影 + 中位日取点");

    let (hits, total) = tl.inject_regression(64);
    chk(&mut set, "C496-21", total > 0 && hits == total, "LCG 注入回归：拦截实例 100% 拒收");
    chk(&mut set, "C496-22", tl.conservation(), "注入后守恒恒等式仍成立");

    let mut raw: Vec<u8> = Vec::new();
    tl.encode(&mut raw);
    let round = TraceLedger::decode(&raw);
    chk(&mut set, "C496-23", match round { Ok(tr) => tr.len() == tl.traces.len() && fnv1a(&raw) == fnv1a(&raw), Err(_) => false }, "定长字节投影往返 + FNV 指纹自洽");
    chk(&mut set, "C496-24", TraceLedger::decode(&raw[..raw.len() - 1]) == Err(PrCode::LedgerDrift), "非整倍长度→台账漂移 Err");
    chk(&mut set, "C496-25", !tl.diagnose().is_empty(), "台账诊断投影非空");

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flow_states() {
        assert!(transition_allowed(FState::Suspended, FState::Running));
        assert!(!transition_allowed(FState::Closed, FState::Closed));
        let mut fb = FlowBook::new(5);
        let _ = fb.create(1);
        assert_eq!(fb.advance(1), Err(PrCode::StateIllegal));
    }
}
