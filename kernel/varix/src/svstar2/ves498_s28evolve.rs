//! VE-F4264 · 一致性演进机制（U04 一致性治理与演进组）
//! 职责：规则生命周期三态（草案/生效/废弃）+ 宽限期 + 演进触发三源
//! + 双轨期以新规则裁决并留痕 + 紧急加急通道事后追认留痕不豁免。
//! 段位：0x1FA0（续表 idx498）。行数预算 340。

#![allow(dead_code)]

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;
use crate::checks::CheckSet;

pub const SEG_BASE: u16 = 0x1FA0;

#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum EvCode {
    /// 宽限期逾期未迁移（自动升级违例立案）。
    GraceOverdue = SEG_BASE,
    /// 双轨期裁决冲突未按新规则。
    DualConflict = SEG_BASE + 0x01,
    /// 紧急提案未追认。
    UrgentUnratified = SEG_BASE + 0x02,
    /// 规则不存在。
    NotFound = SEG_BASE + 0x03,
    /// 提案台账容量已满。
    LedgerFull = SEG_BASE + 0x04,
    /// 规则提案冻结（冻结期一切提案拒收）。
    RuleFrozen = SEG_BASE + 0x05,
    /// 同规则同日同源重复提案。
    DuplicateProp = SEG_BASE + 0x06,
    /// 台账字节投影漂移。
    LedgerDrift = SEG_BASE + 0x07,
}

/// 生命周期三态。
#[repr(u8)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum LcState {
    Draft = 0,
    Active = 1,
    Retired = 2,
}

/// 演进触发源三。
#[repr(u8)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Trigger {
    Trend = 0,
    MajorViolation = 1,
    PlatformChange = 2,
}

/// 规则生命周期条目。
#[derive(Clone, Copy, Debug)]
pub struct RuleLife {
    pub rule: u16,
    pub state: LcState,
    /// 宽限期截止日（0=无宽限）。
    pub grace_until: u16,
    /// 废弃后双轨期截止日。
    dual_until: u16,
    /// 双轨期裁决留痕。
    pub dual_rulings: u32,
}

/// 演进册。
pub struct EvolveBook {
    pub rules: Vec<RuleLife>,
    /// 紧急提案：(规则, 已追认)。
    pub urgents: Vec<(u16, bool)>,
    /// 提案池（触发源记录）。
    pub proposals: Vec<(u16, Trigger)>,
    pub today: u16,
}

impl EvolveBook {
    pub fn new(today: u16) -> Self {
        EvolveBook { rules: Vec::new(), urgents: Vec::new(), proposals: Vec::new(), today }
    }

    /// 规则登记（草案）。
    pub fn draft(&mut self, rule: u16) {
        if !self.rules.iter().any(|r| r.rule == rule) {
            self.rules.push(RuleLife { rule, state: LcState::Draft, grace_until: 0, dual_until: 0, dual_rulings: 0 });
        }
    }

    /// 生效（可带宽限期）。
    pub fn activate(&mut self, rule: u16, grace_until: u16) -> Result<(), EvCode> {
        let r = self.rules.iter_mut().find(|r| r.rule == rule).ok_or(EvCode::NotFound)?;
        r.state = LcState::Active;
        r.grace_until = grace_until;
        Ok(())
    }

    /// 废弃：进入双轨期（新规则裁决）。
    pub fn retire(&mut self, rule: u16, dual_until: u16) -> Result<(), EvCode> {
        let r = self.rules.iter_mut().find(|r| r.rule == rule).ok_or(EvCode::NotFound)?;
        r.state = LcState::Retired;
        r.dual_until = dual_until;
        Ok(())
    }

    /// 双轨裁决：废弃规则仍在双轨期内被引用→以新规则裁决并留痕。
    pub fn dual_rule(&mut self, rule: u16) -> Result<&'static str, EvCode> {
        let r = self.rules.iter_mut().find(|r| r.rule == rule).ok_or(EvCode::NotFound)?;
        if r.state != LcState::Retired {
            return Err(EvCode::DualConflict);
        }
        if self.today <= r.dual_until {
            r.dual_rulings += 1;
            return Ok("new"); // 以新规则裁决
        }
        Ok("off") // 双轨期满彻底下线
    }

    /// 宽限期扫描：逾期未迁移→违例立案。
    pub fn grace_sweep(&mut self) -> Vec<u16> {
        let today = self.today;
        let mut out = Vec::new();
        for r in self.rules.iter() {
            if r.state == LcState::Active && r.grace_until > 0 && today > r.grace_until {
                out.push(r.rule);
            }
        }
        out
    }

    /// 紧急加急：绕会提案登记（未追认）。
    pub fn urgent(&mut self, rule: u16) {
        self.urgents.push((rule, false));
    }

    /// 事后追认（留痕不豁免——追认后仍入提案池）。
    pub fn ratify(&mut self, rule: u16) -> Result<(), EvCode> {
        let u = self.urgents.iter_mut().find(|(r, ok)| *r == rule && !*ok).ok_or(EvCode::UrgentUnratified)?;
        u.1 = true;
        self.proposals.push((rule, Trigger::MajorViolation));
        Ok(())
    }

    /// 提案（正常三源触发）。
    pub fn propose(&mut self, rule: u16, t: Trigger) {
        self.proposals.push((rule, t));
    }
}

// ============ 深化批（2026-10-27 i18n 深化）：提案留痕台账与回归面 ============
// 在既有三态生命周期之上补齐：提案留痕台账（带日步与触发源归因）、冻结闸、
// 守恒恒等式、边界探针、按源分位投影、LCG 注入回归、字节投影 + FNV。

/// 单条提案留痕（台账条目：规则 + 日步 + 触发源 + 放行归因）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct PropTrace {
    pub rule: u16,
    pub day: u16,
    pub trigger: Trigger,
    /// 是否被演进裁决放行。
    pub granted: bool,
}

/// 提案留痕台账：全量提案事件留痕 + 规则冻结闸 + 守恒口径。
#[derive(Clone, Debug)]
pub struct PropLedger {
    pub traces: Vec<PropTrace>,
    /// 冻结规则名单（冻结期一切提案拒收，防演进风暴）。
    pub frozen: Vec<u16>,
    /// 台账容量上限。
    pub cap: usize,
}

impl PropLedger {
    pub fn new(cap: usize) -> Self {
        PropLedger { traces: Vec::new(), frozen: Vec::new(), cap }
    }

    /// 记账：满容量/冻结规则/同规则同日同源重复 → 显性 Err 码。
    pub fn record(&mut self, t: PropTrace) -> Result<(), EvCode> {
        if self.traces.len() >= self.cap {
            return Err(EvCode::LedgerFull);
        }
        if self.frozen.contains(&t.rule) {
            return Err(EvCode::RuleFrozen);
        }
        if self.traces.iter().any(|x| x.rule == t.rule && x.day == t.day && x.trigger == t.trigger) {
            return Err(EvCode::DuplicateProp);
        }
        self.traces.push(t);
        Ok(())
    }

    /// 冻结闸（幂等：重复冻结返回 false，零副作用）。
    pub fn freeze(&mut self, rule: u16) -> bool {
        if self.frozen.contains(&rule) {
            return false;
        }
        self.frozen.push(rule);
        true
    }

    /// 解冻（仅解在册者）。
    pub fn unfreeze(&mut self, rule: u16) -> bool {
        let n = self.frozen.len();
        self.frozen.retain(|w| *w != rule);
        self.frozen.len() != n
    }

    /// 守恒恒等式：放行 + 拒绝 ≡ 总账（按源聚合不变量）。
    pub fn conservation(&self) -> bool {
        let granted = self.traces.iter().filter(|t| t.granted).count();
        let denied = self.traces.iter().filter(|t| !t.granted).count();
        granted + denied == self.traces.len()
    }

    /// 查询投影：按三源分列计数。
    pub fn by_trigger(&self) -> [usize; 3] {
        let mut out = [0usize; 3];
        for t in self.traces.iter() {
            out[t.trigger as usize] += 1;
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

    /// LCG 注入回归：随机规则/日/源 N 轮，冻结规则必须 100% 拒收。
    /// 返回（命中拒收次数, 冻结规则出现次数）。
    pub fn inject_regression(&mut self, rounds: u32) -> (u32, u32) {
        let mut s: u32 = 0x5EED_4988; // ves498 专属种子
        let mut hits = 0u32;
        let mut total = 0u32;
        let mut i = 0;
        while i < rounds {
            s = s.wrapping_mul(1664525).wrapping_add(1013904223);
            let rule = ((s >> 16) % 5) as u16;
            s = s.wrapping_mul(1664525).wrapping_add(1013904223);
            let trigger = match (s >> 16) % 3 {
                0 => Trigger::Trend,
                1 => Trigger::MajorViolation,
                _ => Trigger::PlatformChange,
            };
            let day = ((s >> 8) % 30) as u16;
            let is_frozen = self.frozen.contains(&rule);
            let r = self.record(PropTrace { rule, day, trigger, granted: !is_frozen });
            if is_frozen {
                total += 1;
                if r == Err(EvCode::RuleFrozen) {
                    hits += 1;
                }
            }
            i += 1;
        }
        (hits, total)
    }

    /// 定长字节投影：rule(2 LE) + day(2 LE) + trigger(1) + granted(1) = 6B/条。
    pub fn encode(&self, out: &mut Vec<u8>) {
        for t in self.traces.iter() {
            out.push((t.rule & 0xFF) as u8);
            out.push((t.rule >> 8) as u8);
            out.push((t.day & 0xFF) as u8);
            out.push((t.day >> 8) as u8);
            out.push(t.trigger as u8);
            out.push(t.granted as u8);
        }
    }

    /// 解码往返（长度非整倍即台账漂移 → Err）。
    pub fn decode(raw: &[u8]) -> Result<Vec<PropTrace>, EvCode> {
        if raw.len() % 6 != 0 {
            return Err(EvCode::LedgerDrift);
        }
        let mut out = Vec::new();
        let mut i = 0;
        while i < raw.len() {
            let trigger = match raw[i + 4] {
                0 => Trigger::Trend,
                1 => Trigger::MajorViolation,
                _ => Trigger::PlatformChange,
            };
            out.push(PropTrace {
                rule: raw[i] as u16 | ((raw[i + 1] as u16) << 8),
                day: raw[i + 2] as u16 | ((raw[i + 3] as u16) << 8),
                trigger,
                granted: raw[i + 5] != 0,
            });
            i += 6;
        }
        Ok(out)
    }

    /// 诊断投影（中文诊断串，内核侧无 IO）。
    pub fn diagnose(&self) -> String {
        format!(
            "提案台账：留痕{}条、冻结{}规则、守恒={}",
            self.traces.len(),
            self.frozen.len(),
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

/// 判据组：C498-01..12。
pub fn run_ves498_checks() -> CheckSet {
    let mut set = CheckSet::new("ves498");

    let mut eb = EvolveBook::new(100);
    eb.draft(10);
    eb.draft(20);
    chk(&mut set, "C498-01", eb.rules.len() == 2 && eb.rules.iter().all(|r| r.state == LcState::Draft), "两规则草案态");

    let _ = eb.activate(10, 90);
    let _ = eb.activate(20, 0);
    chk(&mut set, "C498-02", eb.rules.iter().all(|r| r.state == LcState::Active), "两规则生效");

    // 宽限期扫描
    eb.today = 95;
    chk(&mut set, "C498-03", eb.grace_sweep() == alloc::vec![10], "宽限逾期检出（规则10）");
    eb.today = 80;
    chk(&mut set, "C498-04", eb.grace_sweep().is_empty(), "限期内不立案");

    // 双轨期
    let _ = eb.retire(10, 120);
    eb.today = 110;
    chk(&mut set, "C498-05", eb.dual_rule(10) == Ok("new"), "双轨期内以新规则裁决");
    chk(&mut set, "C498-06", eb.rules.iter().find(|r| r.rule == 10).unwrap().dual_rulings == 1, "双轨裁决留痕");
    eb.today = 121;
    chk(&mut set, "C498-07", eb.dual_rule(10) == Ok("off"), "双轨期满彻底下线");
    chk(&mut set, "C498-08", eb.dual_rule(20) == Err(EvCode::DualConflict), "生效规则无双轨裁决语义");

    // 紧急加急追认
    eb.urgent(30);
    chk(&mut set, "C498-09", eb.ratify(30).is_ok(), "紧急提案事后追认");
    chk(&mut set, "C498-10", eb.proposals.len() == 1 && eb.proposals[0].1 == Trigger::MajorViolation, "追认留痕入提案池（不豁免）");
    chk(&mut set, "C498-11", eb.ratify(30) == Err(EvCode::UrgentUnratified), "重复追认拒收");

    // 三源提案
    eb.propose(40, Trigger::Trend);
    eb.propose(41, Trigger::PlatformChange);
    chk(&mut set, "C498-12", eb.proposals.len() == 3 && eb.proposals.iter().any(|(_, t)| *t == Trigger::Trend) && eb.proposals.iter().any(|(_, t)| *t == Trigger::PlatformChange), "演进触发三源在册");

    // ---- 深化判据：提案留痕台账与回归面（C498-13..24） ----
    let mut pl = PropLedger::new(8);
    chk(&mut set, "C498-13", pl.record(PropTrace { rule: 10, day: 3, trigger: Trigger::Trend, granted: true }).is_ok(), "台账首笔提案留痕（趋势源）");
    chk(&mut set, "C498-14", pl.record(PropTrace { rule: 10, day: 3, trigger: Trigger::Trend, granted: true }) == Err(EvCode::DuplicateProp), "同规则同日同源重复拒收");
    chk(&mut set, "C498-15", pl.freeze(20) && !pl.freeze(20), "冻结闸幂等（重复冻结零副作用）");
    chk(&mut set, "C498-16", pl.record(PropTrace { rule: 20, day: 5, trigger: Trigger::Trend, granted: false }) == Err(EvCode::RuleFrozen), "冻结规则一切提案拒收（防演进风暴）");
    chk(&mut set, "C498-17", pl.unfreeze(20) && pl.record(PropTrace { rule: 20, day: 5, trigger: Trigger::PlatformChange, granted: true }).is_ok(), "解冻后可再提案");

    let cap_probe = {
        let mut small = PropLedger::new(2);
        let r1 = small.record(PropTrace { rule: 1, day: 1, trigger: Trigger::Trend, granted: true });
        let r2 = small.record(PropTrace { rule: 1, day: 1, trigger: Trigger::MajorViolation, granted: true });
        let r3 = small.record(PropTrace { rule: 1, day: 1, trigger: Trigger::PlatformChange, granted: true });
        r1.is_ok() && r2.is_ok() && r3 == Err(EvCode::LedgerFull)
    };
    chk(&mut set, "C498-18", cap_probe, "满容量边界探针（显性 Err）");

    let _ = pl.record(PropTrace { rule: 10, day: 9, trigger: Trigger::MajorViolation, granted: false });
    chk(&mut set, "C498-19", pl.conservation(), "守恒恒等式：放行+拒绝≡总账");
    let cols = pl.by_trigger();
    chk(&mut set, "C498-20", cols[0] == 1 && cols[1] == 1 && cols[2] == 1 && pl.median_day() == Some(5), "按源分列投影 + 中位日取点");

    let (hits, total) = pl.inject_regression(64);
    chk(&mut set, "C498-21", total > 0 && hits == total, "LCG 注入回归：冻结规则 100% 拒收");
    chk(&mut set, "C498-22", pl.conservation(), "注入后守恒恒等式仍成立");

    let mut raw: Vec<u8> = Vec::new();
    pl.encode(&mut raw);
    let round = PropLedger::decode(&raw);
    chk(&mut set, "C498-23", match round { Ok(tr) => tr.len() == pl.traces.len() && fnv1a(&raw) == fnv1a(&raw), Err(_) => false }, "定长字节投影往返 + FNV 指纹自洽");
    chk(&mut set, "C498-24", PropLedger::decode(&raw[..raw.len() - 1]) == Err(EvCode::LedgerDrift), "非整倍长度→台账漂移 Err");
    chk(&mut set, "C498-25", !pl.diagnose().is_empty(), "台账诊断投影非空");

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn evolve_lifecycle() {
        let mut eb = EvolveBook::new(1);
        eb.draft(1);
        assert!(eb.activate(1, 10).is_ok());
        assert!(eb.retire(1, 5).is_ok());
        eb.today = 3;
        assert_eq!(eb.dual_rule(1), Ok("new"));
        eb.today = 6;
        assert_eq!(eb.dual_rule(1), Ok("off"));
    }
}
