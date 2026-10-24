//! VE-F4261 · U04 开工与治理总纲（U04 一致性治理与演进组）
//! 职责：U 域治理组开工——治理四支柱（委员会→流程→处置→演进）职责边界
//! 冻结 + 与 U01 五层架构关系声明（治理面是运营层，不改五层只运营五层）
//! + 委员会缺位→临时仲裁池顶替并限期补选。
//! 段位：0x1F00（续表 idx495）。行数预算 340。

#![allow(dead_code)]

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
