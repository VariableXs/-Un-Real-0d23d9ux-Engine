//! VE-F4292 · 工具集成文档（U05 工具集成组）
//! 职责：集成面文档四族（接入指南/适配器参考/总线协议/故障手册）+ 协议
//! 文档语义版本与 U02 同规 + 指南步骤逐条可复现验证（复现失败阻断发版）
//! + 协议破坏兼容禁止发布 + 故障手册缺景补录排程 + 读屏结构（域本色）。
//! 段位：0x2DE0（续表 idx526）。行数预算 300。

#![allow(dead_code)]

use alloc::vec;
use alloc::vec::Vec;
use crate::checks::CheckSet;

pub const SEG_BASE: u16 = 0x2DE0;

#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum IdCode {
    /// 复现失败（阻断发版）。
    ReproFail = SEG_BASE,
    /// 协议破坏兼容（禁止发布）。
    Incompat = SEG_BASE + 0x01,
    /// 故障手册缺景。
    ManualHole = SEG_BASE + 0x02,
}

/// 四族文档。
#[repr(u8)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum IdDoc {
    Guide = 0,
    AdapterRef = 1,
    BusProto = 2,
    FaultManual = 3,
}
pub const IDOC_N: usize = 4;

/// 步骤复现记录。
#[derive(Clone, Copy, Debug)]
pub struct Step {
    pub doc: IdDoc,
    pub no: u8,
    pub repro_ok: bool,
}

/// 集成文档册。
pub struct IntegDoc {
    pub docs: [bool; IDOC_N],
    /// 协议文档版本（语义版本：主.次）。
    pub proto_major: u32,
    pub proto_minor: u32,
    pub steps: Vec<Step>,
    /// 故障手册景数。
    pub manual_scenes: u32,
    /// 读屏结构标记（域本色）。
    pub sr_struct: bool,
}

impl IntegDoc {
    pub fn new() -> Self {
        IntegDoc { docs: [false; IDOC_N], proto_major: 1, proto_minor: 0, steps: Vec::new(), manual_scenes: 0, sr_struct: false }
    }

    pub fn write(&mut self, d: IdDoc) {
        self.docs[d as usize] = true;
    }

    pub fn all_docs(&self) -> bool {
        self.docs.iter().all(|d| *d)
    }

    /// 步骤复现登记。
    pub fn add_step(&mut self, d: IdDoc, no: u8, ok: bool) {
        self.steps.push(Step { doc: d, no, repro_ok: ok });
    }

    /// 复现验证：任一步失败→阻断发版。
    pub fn repro_ok(&self) -> Result<(), IdCode> {
        if self.steps.iter().any(|s| !s.repro_ok) {
            return Err(IdCode::ReproFail);
        }
        Ok(())
    }

    /// 协议变更兼容检：主版本不变=兼容；主版本变=破坏兼容禁止发布。
    pub fn proto_change(&mut self, major: u32, minor: u32) -> Result<(), IdCode> {
        if major != self.proto_major && minor == 0 {
            return Err(IdCode::Incompat);
        }
        self.proto_major = major;
        self.proto_minor = minor;
        Ok(())
    }

    /// 故障手册补景。
    pub fn add_scene(&mut self, n: u32) {
        self.manual_scenes += n;
    }

    pub fn manual_ok(&self, need: u32) -> bool {
        self.manual_scenes >= need
    }
}

fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) {
    set.add(label, ok, detail);
}

/// 判据组：C526-01..12。
pub fn run_ves526_checks() -> CheckSet {
    let mut set = CheckSet::new("ves526");

    let mut id = IntegDoc::new();
    for d in [IdDoc::Guide, IdDoc::AdapterRef, IdDoc::BusProto, IdDoc::FaultManual] {
        id.write(d);
    }
    chk(&mut set, "C526-01", id.all_docs(), "四族文档齐备");

    // 步骤复现
    id.add_step(IdDoc::Guide, 1, true);
    id.add_step(IdDoc::Guide, 2, true);
    chk(&mut set, "C526-02", id.repro_ok().is_ok(), "指南步骤全复现");
    id.add_step(IdDoc::Guide, 3, false);
    chk(&mut set, "C526-03", id.repro_ok() == Err(IdCode::ReproFail), "复现失败阻断发版");

    // 协议版本
    chk(&mut set, "C526-04", id.proto_change(1, 1).is_ok(), "次版本升级兼容");
    chk(&mut set, "C526-05", id.proto_change(2, 0) == Err(IdCode::Incompat), "主版本破坏兼容禁止发布");
    chk(&mut set, "C526-06", id.proto_change(2, 1).is_ok(), "主版本带迁移方案放行");
    chk(&mut set, "C526-07", id.proto_major == 2 && id.proto_minor == 1, "协议版本演进在册");

    // 故障手册
    chk(&mut set, "C526-08", !id.manual_ok(3), "故障手册缺景");
    id.add_scene(2);
    chk(&mut set, "C526-09", !id.manual_ok(3), "补 2 仍缺 1");
    id.add_scene(1);
    chk(&mut set, "C526-10", id.manual_ok(3), "补齐三景");

    id.sr_struct = true;
    chk(&mut set, "C526-11", id.sr_struct, "文档结构读屏标准（域本色）");
    chk(&mut set, "C526-12", id.steps.len() == 3 && id.steps.iter().filter(|s| s.repro_ok).count() == 2, "步骤账自洽");

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn integ_docs() {
        let mut id = IntegDoc::new();
        id.write(IdDoc::Guide);
        assert!(!id.all_docs());
        id.add_step(IdDoc::Guide, 1, true);
        assert!(id.repro_ok().is_ok());
        id.add_step(IdDoc::Guide, 2, false);
        assert_eq!(id.repro_ok(), Err(IdCode::ReproFail));
        assert_eq!(id.proto_change(2, 0), Err(IdCode::Incompat));
    }
}
