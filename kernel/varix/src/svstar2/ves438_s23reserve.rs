//! ves438_s23reserve — VE-F4198 · T 域演进预留（域级）
//! 预留：T 域级演进预留——F4178 组级四口之上域级增三口（新书写系统协议口/新合规法域口/新辅助语言技术口）
//! +预留纪律复述（不预写）；预写越界→回退；口被占→迁移评审；登记缺→补登；
//! 三口均含无障碍向说明（域本色）；预留不引入新隐私面。
#![allow(dead_code)]
use alloc::vec;
use alloc::vec::Vec;
use crate::checks::CheckSet;

pub const SEG_BASE: u16 = 0x1380;

#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RvCode {
    Ok = 0,
    Prewrite = SEG_BASE + 0x01,    // 0x1381 预写越界
    SlotOccupied = SEG_BASE + 0x02, // 0x1382 口被占
    RegisterMissing = SEG_BASE + 0x03, // 0x1383 登记缺
}

/// 域级三口。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DomainSlot { NewScript = 0, NewLegalRegion = 1, NewAssistTech = 2 }

impl DomainSlot {
    pub const ALL: [DomainSlot; 3] = [DomainSlot::NewScript, DomainSlot::NewLegalRegion, DomainSlot::NewAssistTech];
    pub fn name(self) -> &'static str {
        match self {
            DomainSlot::NewScript => "new-script-protocol", DomainSlot::NewLegalRegion => "new-legal-region",
            DomainSlot::NewAssistTech => "new-assist-tech",
        }
    }
}

/// 预留口（接口位 + 断言位 + 无障碍向说明——域本色 + 预写检测）。
pub struct ReservedSlot {
    pub slot: DomainSlot,
    pub iface_ready: bool,   // 接口位就绪
    pub assert_ready: bool,  // 断言位就绪
    pub a11y_note: bool,     // 无障碍向说明
    pub prewritten: bool,    // 预写实现（越界检测）
    pub occupied: bool,      // 口被占
    pub registered: bool,    // 登记在册
}

/// 域级预留册。
pub struct ReserveBook { pub slots: Vec<ReservedSlot>, pub migrations: u32, pub rollbacks: u32 }

impl ReserveBook {
    pub fn new() -> Self { ReserveBook { slots: Vec::new(), migrations: 0, rollbacks: 0 } }
    pub fn put(&mut self, s: DomainSlot, iface: bool, assert: bool, a11y: bool, pre: bool) {
        self.slots.retain(|x| x.slot != s);
        self.slots.push(ReservedSlot { slot: s, iface_ready: iface, assert_ready: assert,
            a11y_note: a11y, prewritten: pre, occupied: false, registered: true });
    }
    /// 预留纪律核验：三口齐+登记+不预写+断言位齐+无障碍说明。
    pub fn audit(&self) -> Result<(), RvCode> {
        for s in DomainSlot::ALL {
            let x = match self.slots.iter().find(|x| x.slot == s) {
                Some(x) => x, None => return Err(RvCode::RegisterMissing),
            };
            if !x.registered { return Err(RvCode::RegisterMissing); }
            if x.prewritten { return Err(RvCode::Prewrite); }
            if !x.assert_ready { return Err(RvCode::RegisterMissing); }
            if !x.a11y_note { return Err(RvCode::RegisterMissing); } // 无障碍向说明缺→补登
        }
        Ok(())
    }
    /// 预写越界→回退（清预写标记）。
    pub fn rollback_prewrite(&mut self, s: DomainSlot) -> bool {
        for x in self.slots.iter_mut() {
            if x.slot == s && x.prewritten { x.prewritten = false; self.rollbacks += 1; return true; }
        }
        false
    }
    /// 口被占→迁移评审。
    pub fn occupy(&mut self, s: DomainSlot) -> Result<(), RvCode> {
        for x in self.slots.iter_mut() {
            if x.slot == s {
                if x.occupied {
                    self.migrations += 1;
                    return Err(RvCode::SlotOccupied);
                }
                x.occupied = true;
                return Ok(());
            }
        }
        Err(RvCode::RegisterMissing)
    }
    /// 三口名单含无障碍技术口（域本色位）。
    pub fn has_assist_tech_slot(&self) -> bool {
        self.slots.iter().any(|x| x.slot == DomainSlot::NewAssistTech)
    }
}

fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) { set.add(label, ok, detail); }

pub fn run_ves438_checks() -> CheckSet {
    let mut set = CheckSet::new("ves438");
    let mut bk = ReserveBook::new();

    // C438-01 三口未登记→补登码。
    chk(&mut set, "C438-01", bk.audit() == Err(RvCode::RegisterMissing), "未登记补登");

    // C438-02 登记但预写→越界码。
    for s in DomainSlot::ALL { bk.put(s, true, true, true, true); }
    chk(&mut set, "C438-02", bk.audit() == Err(RvCode::Prewrite), "预写越界");

    // C438-03 预写回退→纪律复绿。
    for s in DomainSlot::ALL { bk.rollback_prewrite(s); }
    chk(&mut set, "C438-03", bk.audit().is_ok() && bk.rollbacks == 3, "回退复绿");

    // C438-04 无障碍向说明缺→补登拒（域本色）。
    bk.put(DomainSlot::NewScript, true, true, false, false);
    chk(&mut set, "C438-04", bk.audit() == Err(RvCode::RegisterMissing), "a11y 说明缺拒");

    // C438-05 补说明后复绿。
    bk.put(DomainSlot::NewScript, true, true, true, false);
    chk(&mut set, "C438-05", bk.audit().is_ok(), "补说明复绿");

    // C438-06 首次占用放行。
    chk(&mut set, "C438-06", bk.occupy(DomainSlot::NewScript).is_ok(), "首次占用");

    // C438-07 口被占→迁移评审。
    chk(&mut set, "C438-07", bk.occupy(DomainSlot::NewScript) == Err(RvCode::SlotOccupied)
        && bk.migrations == 1, "口被占迁移");

    // C438-08 断言位缺→拒。
    bk.put(DomainSlot::NewLegalRegion, true, false, true, false);
    chk(&mut set, "C438-08", bk.audit() == Err(RvCode::RegisterMissing), "断言位缺拒");

    // C438-09 三口名单单源（书写系统/法域/辅助技术）。
    chk(&mut set, "C438-09", DomainSlot::ALL.len() == 3
        && DomainSlot::ALL[0].name() == "new-script-protocol"
        && DomainSlot::ALL[1].name() == "new-legal-region"
        && DomainSlot::ALL[2].name() == "new-assist-tech", "三口单源");

    // C438-10 无障碍技术口在册（域本色位）。
    chk(&mut set, "C438-10", bk.has_assist_tech_slot(), "辅助技术口在册");

    // C438-11 无新隐私面断言（预留册只含预留字段）。
    chk(&mut set, "C438-11", bk.slots.len() == 3
        && bk.slots.iter().all(|x| x.registered), "登记全在册");

    // C438-12 终态：全绿（回退后全部合规）。
    bk.put(DomainSlot::NewLegalRegion, true, true, true, false);
    chk(&mut set, "C438-12", bk.audit().is_ok(), "终态全绿");

    set
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn ves438_smoke() {
        let set = run_ves438_checks();
        let red = set.red_items();
        assert!(red.is_empty(), "ves438 red: {:?}", red);
    }
}
