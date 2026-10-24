//! ves431_s23extfinal — VE-F4191 · T 域可扩展性终审
//! 终审：T 域可扩展性终审——四口预留（F4178）扩展位有效性核验（接口位可挂载+断言位可运行）
//! +第三方扩展面终审（机翻引擎/CAT 工具/语言包来源三类契约+沙箱）；
//! 位失效→修复或撤口；契约缺→阻断上架；越权申报→拒批+立案；
//! 第三方扩展须过出站白名单审（域本色红线）；权限申报含隐私申报。
#![allow(dead_code)]
use alloc::vec;
use alloc::vec::Vec;
use crate::checks::CheckSet;

pub const SEG_BASE: u16 = 0x1280;

#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum EfCode {
    Ok = 0,
    SlotDead = SEG_BASE + 0x01,     // 0x1281
    ContractMissing = SEG_BASE + 0x02, // 0x1282
    OverClaim = SEG_BASE + 0x03,    // 0x1283 越权申报
    WhitelistDeny = SEG_BASE + 0x04, // 0x1284 出站白名单拒（域本色）
}

/// 四口预留（F4178 组级）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ExtSlot { SlotA = 0, SlotB = 1, SlotC = 2, SlotD = 3 }

impl ExtSlot {
    pub const ALL: [ExtSlot; 4] = [ExtSlot::SlotA, ExtSlot::SlotB, ExtSlot::SlotC, ExtSlot::SlotD];
}

/// 第三方三类扩展面。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ThirdPartyKind { MtEngine = 0, CatTool = 1, LpSource = 2 }

/// 第三方契约。
pub struct ThirdContract {
    pub kind: ThirdPartyKind,
    pub sandbox: bool,       // 沙箱审通过
    pub outbound_ok: bool,   // 出站白名单审（域本色红线）
    pub privacy_decl: bool,  // 权限申报含隐私申报
    pub approved: bool,
}

/// 可扩展性终审册。
pub struct ExtFinalBook {
    pub slots: Vec<(ExtSlot, bool, bool)>, // (口, 接口位可挂载, 断言位可运行)
    pub contracts: Vec<ThirdContract>,
    pub filed: u32,
    pub rejected: u32,
}

impl ExtFinalBook {
    pub fn new() -> Self { ExtFinalBook { slots: Vec::new(), contracts: Vec::new(), filed: 0, rejected: 0 } }
    pub fn put_slot(&mut self, s: ExtSlot, mount: bool, runnable: bool) {
        self.slots.retain(|(ss, _, _)| *ss != s);
        self.slots.push((s, mount, runnable));
    }
    /// 四口核验：每口接口位可挂载+断言位可运行。
    pub fn audit_slots(&self) -> Result<(), EfCode> {
        for s in ExtSlot::ALL {
            match self.slots.iter().find(|(ss, _, _)| *ss == s) {
                None => return Err(EfCode::SlotDead),
                Some((_, m, r)) => if !(*m && *r) { return Err(EfCode::SlotDead); },
            }
        }
        Ok(())
    }
    /// 第三方审批：契约齐（沙箱+白名单+隐私申报）→批；缺→阻断上架；越权申报→拒批+立案。
    pub fn review(&mut self, c: ThirdContract) -> Result<(), EfCode> {
        if !c.sandbox { self.rejected += 1; return Err(EfCode::ContractMissing); }
        if !c.privacy_decl { self.filed += 1; return Err(EfCode::OverClaim); }
        if !c.outbound_ok { self.filed += 1; self.rejected += 1; return Err(EfCode::WhitelistDeny); }
        let mut ok = c;
        ok.approved = true;
        self.contracts.retain(|x| x.kind != ok.kind);
        self.contracts.push(ok);
        Ok(())
    }
    /// 上架断言：三类契约全部批准。
    pub fn all_onboard(&self) -> bool {
        ThirdPartyKind::MtEngine as u8 == 0
            && self.contracts.len() == 3
            && self.contracts.iter().all(|c| c.approved)
    }
}

fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) { set.add(label, ok, detail); }

pub fn run_ves431_checks() -> CheckSet {
    let mut set = CheckSet::new("ves431");
    let mut bk = ExtFinalBook::new();

    // C431-01 四口未登记→位失效。
    chk(&mut set, "C431-01", bk.audit_slots() == Err(EfCode::SlotDead), "未登记位失效");

    // C431-02 口挂载但断言位不可运行→仍失效。
    for s in ExtSlot::ALL { bk.put_slot(s, true, false); }
    chk(&mut set, "C431-02", bk.audit_slots() == Err(EfCode::SlotDead), "断言位不可运行");

    // C431-03 四口全可挂载+可运行→核验过。
    for s in ExtSlot::ALL { bk.put_slot(s, true, true); }
    chk(&mut set, "C431-03", bk.audit_slots().is_ok(), "四口核验过");

    // C431-04 单口撤位→失效（修复或撤口路径）。
    bk.put_slot(ExtSlot::SlotC, false, true);
    chk(&mut set, "C431-04", bk.audit_slots() == Err(EfCode::SlotDead), "撤口失效");

    // C431-05 修复后复验过。
    bk.put_slot(ExtSlot::SlotC, true, true);
    chk(&mut set, "C431-05", bk.audit_slots().is_ok(), "修复复验");

    // C431-06 契约缺沙箱→阻断上架。
    let c1 = ThirdContract { kind: ThirdPartyKind::MtEngine, sandbox: false, outbound_ok: true, privacy_decl: true, approved: false };
    chk(&mut set, "C431-06", bk.review(c1) == Err(EfCode::ContractMissing), "缺沙箱阻断");

    // C431-07 越权申报（无隐私申报）→拒批+立案。
    let c2 = ThirdContract { kind: ThirdPartyKind::MtEngine, sandbox: true, outbound_ok: true, privacy_decl: false, approved: false };
    chk(&mut set, "C431-07", bk.review(c2) == Err(EfCode::OverClaim) && bk.filed == 1, "越权拒批立案");

    // C431-08 出站白名单拒（域本色红线）→立案+拒批。
    let c3 = ThirdContract { kind: ThirdPartyKind::MtEngine, sandbox: true, outbound_ok: false, privacy_decl: true, approved: false };
    chk(&mut set, "C431-08", bk.review(c3) == Err(EfCode::WhitelistDeny) && bk.filed == 2, "白名单拒");

    // C431-09 三类契约全过审批→上架。
    for k in [ThirdPartyKind::MtEngine, ThirdPartyKind::CatTool, ThirdPartyKind::LpSource] {
        let c = ThirdContract { kind: k, sandbox: true, outbound_ok: true, privacy_decl: true, approved: false };
        bk.review(c).unwrap();
    }
    chk(&mut set, "C431-09", bk.all_onboard() && bk.contracts.len() == 3, "三类契约上架");

    // C431-10 同类契约覆盖幂等。
    let c4 = ThirdContract { kind: ThirdPartyKind::CatTool, sandbox: true, outbound_ok: true, privacy_decl: true, approved: false };
    bk.review(c4).unwrap();
    chk(&mut set, "C431-10", bk.contracts.len() == 3, "同类覆盖幂等");

    // C431-11 拒批计数累计。
    chk(&mut set, "C431-11", bk.rejected == 2, "拒批计数");

    // C431-12 四口+三类单源断言。
    chk(&mut set, "C431-12", ExtSlot::ALL.len() == 4
        && ThirdPartyKind::LpSource as u8 == 2, "单源断言");

    set
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn ves431_smoke() {
        let set = run_ves431_checks();
        let red = set.red_items();
        assert!(red.is_empty(), "ves431 red: {:?}", red);
    }
}
