//! unx0235_auditself —— UNX-F0235 域审计自查单（finalize 前置）。
//! 锚点（CoRun Unxreal 主册）：UNX-F0235 · 域审计自查单 · 目标 220 行
//! 判据原文：UNX-F0235-J1 finalize 前自查单全绿：必查项零缺、
//!           加权分达线、缺项逐条立案可追踪。
//! 组件：自查项（编号 + 权重 + 必查标记 + 结论）+ 自查单（项集 +
//!       评分引擎 + finalize 闸）+ 缺项立案账（缺/红项逐条出票）。
//! 职责定位：finalize 闸是「域的最后一道门」——门闸规则必须
//!   **保守**：任何必查项缺席（没查过）与必查项红灯（查了不过）
//!   同罪，都拦 finalize。加权分是软指标（分数低报警不拦门），
//!   必查项是硬指标（拦门）——两层不混。重复编号当场拒（自查
//!   单自身先自洽，才有资格审别人）。
//! 性能逐项分解：评分 O(项数)；缺项立案 O(项数)；闸判定 O(1)。
//! 诊断码段：0x20040（0x20041~0x20049 闭集；u32 高段全域独占）。
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，整数算术，确定性。

use alloc::vec;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 模块版本。
pub const AS_VERSION: &str = "unx0235_auditself-v1";

/// 诊断段基址（0x20040 段 · 本模块独占）。
pub const AS_SEG_BASE: u32 = 0x20040;
pub const XC_DUP_ID: u32 = 0x20041; // 项编号重复
pub const XC_WEIGHT_OOR: u32 = 0x20042; // 权重越域
pub const XC_EMPTY_SHEET: u32 = 0x20043; // 空自查单拒
pub const XC_MANDATORY_MISS: u32 = 0x20044; // 必查项缺
pub const XC_MANDATORY_RED: u32 = 0x20045; // 必查项红
pub const XC_SCORE_LOW: u32 = 0x20046; // 加权分不达线
pub const XC_UNKNOWN_ITEM: u32 = 0x20047; // 引用未知项
pub const XC_STATE_OOR: u32 = 0x20048; // 结论态越域
pub const XC_TICKET_EMPTY: u32 = 0x20049; // 空票拒出

/// 自查结论态。
pub const ST_UNCHECKED: u8 = 0; // 未查
pub const ST_GREEN: u8 = 1; // 绿
pub const ST_RED: u8 = 2; // 红
pub const ST_WAIVED: u8 = 3; // 豁免（需必查=否才可用）

/// 单个自查项。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct AuditItem {
    pub id: u32,
    /// 权重 1~10（评分分母用）。
    pub weight: u32,
    /// 必查标记：true 则不可 UNCHECKED、不可 WAIVED。
    pub mandatory: bool,
    pub state: u8,
}

impl AuditItem {
    pub fn new(id: u32, weight: u32, mandatory: bool, state: u8) -> Result<AuditItem, u32> {
        if weight == 0 || weight > 10 {
            return Err(XC_WEIGHT_OOR);
        }
        if state > ST_WAIVED {
            return Err(XC_STATE_OOR);
        }
        if mandatory && state == ST_WAIVED {
            return Err(XC_STATE_OOR); // 必查项不得豁免
        }
        Ok(AuditItem { id, weight, mandatory, state })
    }
}

/// 域审计自查单。
pub struct AuditSheet {
    items: Vec<AuditItem>,
    /// 加权达线（千分制：达成率 ≥ 此值放行软指标）。
    pass_milli: u32,
}

impl AuditSheet {
    /// 构造：编号重复/空单当场拒。
    pub fn new(items: &[AuditItem], pass_milli: u32) -> Result<AuditSheet, u32> {
        if items.is_empty() {
            return Err(XC_EMPTY_SHEET);
        }
        for it in items.iter() {
            if items.iter().filter(|x| x.id == it.id).count() > 1 {
                return Err(XC_DUP_ID);
            }
        }
        if pass_milli > 1000 {
            return Err(XC_WEIGHT_OOR);
        }
        Ok(AuditSheet { items: items.to_vec(), pass_milli })
    }

    /// 加权达成率（千分）：绿=满权、豁免=半权、未查/红=零权。
    pub fn score_milli(&self) -> u32 {
        let mut got: u64 = 0;
        let mut total: u64 = 0;
        for it in self.items.iter() {
            total += it.weight as u64;
            match it.state {
                ST_GREEN => got += it.weight as u64,
                ST_WAIVED => got += it.weight as u64 / 2,
                _ => {}
            }
        }
        if total == 0 {
            return 0;
        }
        ((got * 1000) / total) as u32
    }

    /// 缺项立案：返回（未查的必查项, 红的必查项）两组编号。
    pub fn tickets(&self) -> (Vec<u32>, Vec<u32>) {
        let mut missed = Vec::new();
        let mut red = Vec::new();
        for it in self.items.iter() {
            if !it.mandatory {
                continue;
            }
            if it.state == ST_UNCHECKED {
                missed.push(it.id);
            }
            if it.state == ST_RED {
                red.push(it.id);
            }
        }
        (missed, red)
    }

    /// finalize 闸：必查项零缺零红 + 加权分达线，双条件齐才开。
    pub fn finalize_gate(&self) -> Result<(), u32> {
        let (missed, red) = self.tickets();
        if !missed.is_empty() {
            return Err(XC_MANDATORY_MISS);
        }
        if !red.is_empty() {
            return Err(XC_MANDATORY_RED);
        }
        if self.score_milli() < self.pass_milli {
            return Err(XC_SCORE_LOW);
        }
        Ok(())
    }

    /// 引用核：给定编号查项（未知编号显式拒，不静默给空）。
    pub fn lookup(&self, id: u32) -> Result<AuditItem, u32> {
        match self.items.iter().find(|it| it.id == id) {
            Some(it) => Ok(*it),
            None => Err(XC_UNKNOWN_ITEM),
        }
    }

    /// 项数。
    pub fn len(&self) -> u32 {
        self.items.len() as u32
    }
}

/// 出票核：立案清单非空才可出票（空票拒——票必须 carrying 信息）。
pub fn issue_ticket(ids: &[u32]) -> Result<u32, u32> {
    if ids.is_empty() {
        return Err(XC_TICKET_EMPTY);
    }
    // 票号 = FNV(编号流) 低 32 位（同清单同票号，可对账）。
    let mut h: u64 = 0xcbf29ce484222325;
    for &i in ids {
        h ^= (i as u64) << 32 | i as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    Ok(h as u32)
}

pub fn run_unx0235_checks() -> CheckSet {
    let mut cs = CheckSet::new("unx0235_auditself");
    // 族一：自查单构造。
    let items = vec![
        AuditItem::new(1, 5, true, ST_GREEN).unwrap_or(AuditItem { id: 1, weight: 5, mandatory: true, state: ST_GREEN }),
        AuditItem { id: 2, weight: 3, mandatory: true, state: ST_GREEN },
        AuditItem { id: 3, weight: 2, mandatory: false, state: ST_WAIVED },
        AuditItem { id: 4, weight: 4, mandatory: false, state: ST_RED },
    ];
    let sheet = match AuditSheet::new(&items, 600) {
        Ok(s) => s,
        Err(_) => {
            cs.add("自查单构造", false, "被拒");
            return cs;
        }
    };
    cs.add("自查单构造", sheet.len() == 4, "四项入账");
    // 评分：(5+3) 满 + 2×0.5 + 0 = 9/14 → 642‰。
    cs.add("加权评分", sheet.score_milli() == 642, "9/14=642‰");
    // 族二：finalize 闸（必查全绿 → 开；软分 642 ≥ 600 不拦）。
    cs.add("finalize 开闸", sheet.finalize_gate().is_ok(), "必查绿+分达线");
    // 族三：必查缺/红都拦。
    let items2 = vec![
        AuditItem { id: 1, weight: 5, mandatory: true, state: ST_UNCHECKED },
        AuditItem { id: 2, weight: 5, mandatory: true, state: ST_GREEN },
    ];
    let s2 = AuditSheet::new(&items2, 0).unwrap_or(AuditSheet { items: alloc::vec::Vec::new(), pass_milli: 0 });
    cs.add("缺必查拦", matches!(s2.finalize_gate(), Err(XC_MANDATORY_MISS)), "UNCHECKED 拦门");
    cs.add("缺项立案", matches!(s2.tickets().0.as_slice(), &[1u32]), "1 号出票");
    let items3 = vec![
        AuditItem { id: 1, weight: 5, mandatory: true, state: ST_RED },
        AuditItem { id: 2, weight: 5, mandatory: true, state: ST_GREEN },
    ];
    let s3 = AuditSheet::new(&items3, 0).unwrap_or(AuditSheet { items: alloc::vec::Vec::new(), pass_milli: 0 });
    cs.add("红必查拦", matches!(s3.finalize_gate(), Err(XC_MANDATORY_RED)), "RED 拦门");
    // 族四：软分不达线（必查全绿也拦）。
    let s4 = AuditSheet::new(&items3, 900).unwrap_or(AuditSheet { items: alloc::vec::Vec::new(), pass_milli: 0 });
    cs.add("软分拦", matches!(s4.finalize_gate(), Err(XC_SCORE_LOW)), "500<900");
    // 族五：构造拒。
    cs.add("空单拒", AuditSheet::new(&[], 0).is_err(), "EMPTY");
    cs.add("重号拒", AuditSheet::new(&[
        AuditItem { id: 1, weight: 1, mandatory: false, state: ST_GREEN },
        AuditItem { id: 1, weight: 2, mandatory: false, state: ST_GREEN },
    ], 0).is_err(), "DUP_ID");
    cs.add("权重越域拒", AuditItem::new(1, 0, false, ST_GREEN).is_err(), "WEIGHT");
    cs.add("必查豁免拒", AuditItem::new(1, 5, true, ST_WAIVED).is_err(), "STATE");
    cs.add("态越域拒", AuditItem::new(1, 5, false, 9).is_err(), "STATE");
    // 族六：引用核与出票。
    cs.add("引用命中", sheet.lookup(3).map(|i| i.weight).unwrap_or(0) == 2, "3 号权重 2");
    cs.add("未知引用拒", sheet.lookup(99).is_err(), "UNKNOWN");
    cs.add("空票拒", issue_ticket(&[]).is_err(), "TICKET_EMPTY");
    cs.add("出票稳定", issue_ticket(&[1, 2]) == issue_ticket(&[1, 2]), "同清单同票号");
    cs.add("异单异票", issue_ticket(&[1, 2]) != issue_ticket(&[2, 1]), "序敏感");
    cs
}
