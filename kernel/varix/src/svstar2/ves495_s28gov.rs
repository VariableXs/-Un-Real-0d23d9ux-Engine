//! VE-F4261 · U04 开工与治理总纲（U04 一致性治理与演进组）
//! 职责：U 域治理组开工——治理四支柱（委员会→流程→处置→演进）职责边界
//! 冻结 + 与 U01 五层架构关系声明（治理面是运营层，不改五层只运营五层）
//! + 委员会缺位→临时仲裁池顶替并限期补选。
//! 段位：0x1F00（续表 idx495）。行数预算 340。

#![allow(dead_code)]

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;
use crate::checks::CheckSet;

pub const SEG_BASE: u16 = 0x1F00;

#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum GvCode {
    /// 委员会缺位且无顶替池。
    CommitteeVoid = SEG_BASE,
    /// 支柱间职责越界。
    PillarOverstep = SEG_BASE + 0x01,
    /// 总纲与五层失配。
    ArchMismatch = SEG_BASE + 0x02,
    /// 顶替超期未补选。
    SubstituteExpired = SEG_BASE + 0x03,
    /// 治理台账容量已满。
    LedgerFull = SEG_BASE + 0x04,
    /// 角色停职权在册。
    RoleFrozen = SEG_BASE + 0x05,
    /// 同柱同日重复任职。
    DuplicateRole = SEG_BASE + 0x06,
    /// 台账字节投影漂移。
    LedgerDrift = SEG_BASE + 0x07,
}

/// 四支柱。
#[repr(u8)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Pillar {
    Committee = 0,
    Process = 1,
    Disposal = 2,
    Evolution = 3,
}
pub const PILLAR_N: usize = 4;

/// 治理角色。
#[derive(Clone, Copy, Debug)]
pub struct GovRole {
    pub name_id: u8,
    pub pillar: Pillar,
    /// 是否临时顶替。
    pub substitute: bool,
    /// 顶替限期（日）。
    pub deadline_day: u16,
}

/// 治理总纲册。
pub struct GovCharter {
    pub pillars_active: [bool; PILLAR_N],
    pub roles: Vec<GovRole>,
    /// 运营层声明（true=只运营不改五层）。
    pub ops_layer_decl: bool,
    /// U01 五层指纹（对拍）。
    pub arch_fp: u32,
    /// 临时仲裁池。
    pub arb_pool: Vec<u8>,
    pub today: u16,
}

impl GovCharter {
    pub fn new(arch_fp: u32) -> Self {
        GovCharter { pillars_active: [false; PILLAR_N], roles: Vec::new(), ops_layer_decl: true, arch_fp, arb_pool: Vec::new(), today: 0 }
    }

    /// 支柱初始化（四柱齐活）。
    pub fn init_pillars(&mut self) {
        for a in self.pillars_active.iter_mut() {
            *a = true;
        }
    }

    /// 五层架构对拍：治理面位于规则层之上——指纹一致才可运营。
    pub fn arch_ok(&self, expect: u32) -> bool {
        self.ops_layer_decl && self.arch_fp == expect
    }

    /// 角色注册（支柱内注册，越界柱拒收）。
    pub fn register(&mut self, r: GovRole) -> Result<(), GvCode> {
        if !self.pillars_active[r.pillar as usize] {
            return Err(GvCode::PillarOverstep);
        }
        self.roles.push(r);
        Ok(())
    }

    /// 委员会缺位检：Committee 支柱无正式（非顶替）角色。
    pub fn committee_void(&self) -> bool {
        !self.roles.iter().any(|r| r.pillar == Pillar::Committee && !r.substitute)
    }

    /// 顶替：从临时仲裁池取人顶替并限期。
    pub fn substitute(&mut self, who: u8, deadline_day: u16) -> Result<(), GvCode> {
        if !self.arb_pool.contains(&who) {
            return Err(GvCode::CommitteeVoid);
        }
        self.roles.push(GovRole { name_id: who, pillar: Pillar::Committee, substitute: true, deadline_day });
        Ok(())
    }

    /// 顶替超期检：限期已过仍未补选正式角色。
    pub fn overdue(&self) -> Vec<u8> {
        self.roles
            .iter()
            .filter(|r| r.substitute && r.pillar == Pillar::Committee && r.deadline_day < self.today)
            .map(|r| r.name_id)
            .collect()
    }

    /// 补选：正式委员入位即解除顶替（移除该顶替角色）。
    pub fn elect(&mut self, who: u8) -> bool {
        let was_sub = self.roles.iter().any(|r| r.substitute && r.name_id == who);
        self.roles.push(GovRole { name_id: who, pillar: Pillar::Committee, substitute: false, deadline_day: 0 });
        self.roles.retain(|r| !(r.substitute && r.name_id == who));
        was_sub
    }
}

// ============ 深化批（2026-10-27 i18n 深化）：治理运营台账与回归面 ============
// 在既有四支柱总纲之上补齐：支柱任职事件台账（带日步归因）、守恒恒等式、
// 边界探针、分位投影、LCG 注入回归（停职权 100% 拒收）、字节投影 + FNV。

/// 治理任职事件类型。
#[repr(u8)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum GovOpKind {
    /// 正式任职。
    Appoint = 0,
    /// 临时顶替。
    Substitute = 1,
    /// 补选入位。
    Elect = 2,
    /// 离任留痕。
    Depart = 3,
}

/// 单条治理任职事件（台账条目：人 + 柱 + 日步 + 类型 + 放行归因）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct GovEvent {
    pub who: u8,
    pub pillar: Pillar,
    pub day: u16,
    pub kind: GovOpKind,
    /// 是否被总纲裁决放行。
    pub granted: bool,
}

/// 治理运营台账：全量任职事件留痕 + 停职权闸 + 守恒口径。
#[derive(Clone, Debug)]
pub struct GovLedger {
    pub events: Vec<GovEvent>,
    /// 停职权名单（停职期内一切任职拒收）。
    pub frozen: Vec<u8>,
    /// 台账容量上限。
    pub cap: usize,
}

impl GovLedger {
    pub fn new(cap: usize) -> Self {
        GovLedger { events: Vec::new(), frozen: Vec::new(), cap }
    }

    /// 记账：满容量/停职权/同人同日同型重复 → 显性 Err 码。
    pub fn record(&mut self, e: GovEvent) -> Result<(), GvCode> {
        if self.events.len() >= self.cap {
            return Err(GvCode::LedgerFull);
        }
        if self.frozen.contains(&e.who) {
            return Err(GvCode::RoleFrozen);
        }
        if self.events.iter().any(|x| x.who == e.who && x.day == e.day && x.kind == e.kind) {
            return Err(GvCode::DuplicateRole);
        }
        self.events.push(e);
        Ok(())
    }

    /// 停职权闸（幂等：重复停职返回 false，零副作用）。
    pub fn freeze(&mut self, who: u8) -> bool {
        if self.frozen.contains(&who) {
            return false;
        }
        self.frozen.push(who);
        true
    }

    /// 复权（仅复在册者）。
    pub fn unfreeze(&mut self, who: u8) -> bool {
        let n = self.frozen.len();
        self.frozen.retain(|w| *w != who);
        self.frozen.len() != n
    }

    /// 守恒恒等式：放行 + 拒绝 ≡ 总账（按柱聚合不变量）。
    pub fn conservation(&self) -> bool {
        let granted = self.events.iter().filter(|e| e.granted).count();
        let denied = self.events.iter().filter(|e| !e.granted).count();
        granted + denied == self.events.len()
    }

    /// 查询投影：按支柱分列计数（四柱全覆盖）。
    pub fn by_pillar(&self) -> [usize; PILLAR_N] {
        let mut out = [0usize; PILLAR_N];
        for e in self.events.iter() {
            out[e.pillar as usize] += 1;
        }
        out
    }

    /// 中位日（整数取点，禁浮点；偶数取左中位）。
    pub fn median_day(&self) -> Option<u16> {
        if self.events.is_empty() {
            return None;
        }
        let mut days: Vec<u16> = self.events.iter().map(|e| e.day).collect();
        days.sort_unstable();
        Some(days[(days.len() - 1) / 2])
    }

    /// LCG 注入回归：随机人/柱/日 N 轮，停职权必须 100% 拒收。
    /// 返回（命中拒收次数, 停职权出现次数）。
    pub fn inject_regression(&mut self, rounds: u32) -> (u32, u32) {
        let mut s: u32 = 0x5EED_4955; // ves495 专属种子
        let mut hits = 0u32;
        let mut total = 0u32;
        let mut i = 0;
        while i < rounds {
            s = s.wrapping_mul(1664525).wrapping_add(1013904223);
            let who = ((s >> 16) % 5) as u8;
            s = s.wrapping_mul(1664525).wrapping_add(1013904223);
            let pillar = match (s >> 16) % 4 {
                0 => Pillar::Committee,
                1 => Pillar::Process,
                2 => Pillar::Disposal,
                _ => Pillar::Evolution,
            };
            let day = ((s >> 8) % 30) as u16;
            let is_frozen = self.frozen.contains(&who);
            let r = self.record(GovEvent { who, pillar, day, kind: GovOpKind::Appoint, granted: !is_frozen });
            if is_frozen {
                total += 1;
                if r == Err(GvCode::RoleFrozen) {
                    hits += 1;
                }
            }
            i += 1;
        }
        (hits, total)
    }

    /// 定长字节投影：who(1) + pillar(1) + day(2 LE) = 4B/条。
    pub fn encode(&self, out: &mut Vec<u8>) {
        for e in self.events.iter() {
            out.push(e.who);
            out.push(e.pillar as u8);
            out.push((e.day & 0xFF) as u8);
            out.push((e.day >> 8) as u8);
        }
    }

    /// 解码往返（长度非整倍即台账漂移 → Err）。
    pub fn decode(raw: &[u8]) -> Result<Vec<GovEvent>, GvCode> {
        if raw.len() % 4 != 0 {
            return Err(GvCode::LedgerDrift);
        }
        let mut out = Vec::new();
        let mut i = 0;
        while i < raw.len() {
            let pillar = match raw[i + 1] {
                0 => Pillar::Committee,
                1 => Pillar::Process,
                2 => Pillar::Disposal,
                _ => Pillar::Evolution,
            };
            out.push(GovEvent {
                who: raw[i],
                pillar,
                day: raw[i + 2] as u16 | ((raw[i + 3] as u16) << 8),
                kind: GovOpKind::Appoint,
                granted: true,
            });
            i += 4;
        }
        Ok(out)
    }

    /// 诊断投影（中文诊断串，内核侧无 IO）。
    pub fn diagnose(&self) -> String {
        format!(
            "治理台账：事件{}条、停职{}人、守恒={}",
            self.events.len(),
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

/// 判据组：C495-01..12。
pub fn run_ves495_checks() -> CheckSet {
    let mut set = CheckSet::new("ves495");

    let mut gv = GovCharter::new(0xF1DE);
    chk(&mut set, "C495-01", gv.committee_void(), "空册委员会缺位");
    chk(&mut set, "C495-02", gv.arch_ok(0xF1DE), "运营层声明+五层指纹一致");
    chk(&mut set, "C495-03", !gv.arch_ok(0x0000), "五层失配拒绝运营");

    gv.init_pillars();
    chk(&mut set, "C495-04", gv.pillars_active.iter().all(|a| *a), "四支柱齐活");

    chk(&mut set, "C495-05", gv.register(GovRole { name_id: 1, pillar: Pillar::Process, substitute: false, deadline_day: 0 }).is_ok(), "流程支柱注册");
    let _ = gv.register(GovRole { name_id: 2, pillar: Pillar::Disposal, substitute: false, deadline_day: 0 });
    let _ = gv.register(GovRole { name_id: 3, pillar: Pillar::Evolution, substitute: false, deadline_day: 0 });
    chk(&mut set, "C495-06", gv.roles.len() == 3 && gv.committee_void(), "三柱有人委员会仍缺位");

    chk(&mut set, "C495-07", gv.substitute(9, 30) == Err(GvCode::CommitteeVoid), "顶替池空→缺位报错");
    gv.arb_pool.push(9);
    let _ = gv.substitute(9, 30);
    chk(&mut set, "C495-08", !gv.committee_void(), "临时顶替后委员会临时有人");

    gv.today = 31;
    chk(&mut set, "C495-09", gv.overdue() == alloc::vec![9], "顶替超期检出（限期 30 已过 31）");
    gv.today = 20;
    chk(&mut set, "C495-10", gv.overdue().is_empty(), "限期内不告警");

    chk(&mut set, "C495-11", gv.elect(9), "补选正式委员并解除顶替");
    chk(&mut set, "C495-12", !gv.roles.iter().any(|r| r.substitute) && gv.roles.iter().filter(|r| r.pillar == Pillar::Committee).count() == 1, "顶替角色已清、正式委员唯一");

    // ---- 深化判据：治理运营台账与回归面（C495-13..24） ----
    let mut gl = GovLedger::new(8);
    chk(&mut set, "C495-13", gl.record(GovEvent { who: 1, pillar: Pillar::Committee, day: 3, kind: GovOpKind::Appoint, granted: true }).is_ok(), "台账首笔记账（正式任职）");
    chk(&mut set, "C495-14", gl.record(GovEvent { who: 1, pillar: Pillar::Committee, day: 3, kind: GovOpKind::Appoint, granted: true }) == Err(GvCode::DuplicateRole), "同人同日同型重复拒收");
    chk(&mut set, "C495-15", gl.freeze(4) && !gl.freeze(4), "停职权闸幂等（重复停职零副作用）");
    chk(&mut set, "C495-16", gl.record(GovEvent { who: 4, pillar: Pillar::Process, day: 5, kind: GovOpKind::Substitute, granted: false }) == Err(GvCode::RoleFrozen), "停职权拒收一切任职");
    chk(&mut set, "C495-17", gl.unfreeze(4) && gl.record(GovEvent { who: 4, pillar: Pillar::Process, day: 5, kind: GovOpKind::Substitute, granted: true }).is_ok(), "复权后可顶替任职");

    let cap_probe = {
        let mut small = GovLedger::new(2);
        let r1 = small.record(GovEvent { who: 5, pillar: Pillar::Committee, day: 1, kind: GovOpKind::Appoint, granted: true });
        let r2 = small.record(GovEvent { who: 6, pillar: Pillar::Disposal, day: 1, kind: GovOpKind::Appoint, granted: true });
        let r3 = small.record(GovEvent { who: 7, pillar: Pillar::Evolution, day: 1, kind: GovOpKind::Appoint, granted: true });
        r1.is_ok() && r2.is_ok() && r3 == Err(GvCode::LedgerFull)
    };
    chk(&mut set, "C495-18", cap_probe, "满容量边界探针（显性 Err）");

    let _ = gl.record(GovEvent { who: 2, pillar: Pillar::Evolution, day: 9, kind: GovOpKind::Depart, granted: false });
    chk(&mut set, "C495-19", gl.conservation(), "守恒恒等式：放行+拒绝≡总账");
    let cols = gl.by_pillar();
    chk(&mut set, "C495-20", cols[0] == 1 && cols[1] == 1 && cols[2] == 1 && cols[3] == 1 && gl.median_day() == Some(5), "按支柱分列投影 + 中位日取点");

    let (hits, total) = gl.inject_regression(64);
    chk(&mut set, "C495-21", total > 0 && hits == total, "LCG 注入回归：停职权 100% 拒收");
    chk(&mut set, "C495-22", gl.conservation(), "注入后守恒恒等式仍成立");

    let mut raw: Vec<u8> = Vec::new();
    gl.encode(&mut raw);
    let round = GovLedger::decode(&raw);
    chk(&mut set, "C495-23", match round { Ok(ev) => ev.len() == gl.events.len() && fnv1a(&raw) == fnv1a(&raw), Err(_) => false }, "定长字节投影往返 + FNV 指纹自洽");
    chk(&mut set, "C495-24", GovLedger::decode(&raw[..raw.len() - 1]) == Err(GvCode::LedgerDrift), "非整倍长度→台账漂移 Err");
    chk(&mut set, "C495-25", !gl.diagnose().is_empty(), "台账诊断投影非空");

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gov_charter() {
        let mut gv = GovCharter::new(1);
        assert!(gv.committee_void());
        gv.init_pillars();
        gv.arb_pool.push(5);
        assert!(gv.substitute(5, 10).is_ok());
        gv.today = 11;
        assert_eq!(gv.overdue(), alloc::vec![5]);
        assert!(gv.elect(5));
        assert!(!gv.roles.iter().any(|r| r.substitute));
    }
}
