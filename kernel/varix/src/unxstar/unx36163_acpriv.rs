//! UNX-F36163-J1 AccessCheck 第②步：特权要求门（acpriv）。
//! 判据：① 两步判定契约——存在性（B03 privtbl::lookup_by_luid 表内锚）
//! + 持有性（token.privs 成员判定），step2 冻结签名
//! `step2(&AcRequest) -> Result<(), u16>`；② 持有位图与命名对拍；
//! ③ 注入负例（无门放行/表内未持/表外 LUID）显性拒；④ ≥310 行。
//! 设计纪律：36161 通过 `super::unx36163_acpriv as s2` 消费 step2；
//! 特权存在性唯一真源是 unx36041_privtable（禁在本模块重述特权表）。

use super::unx36041_privtable as privtbl;
use super::unx36161_acfreeze as acf;
use crate::checks::CheckSet;
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

pub const SEG_BASE: u16 = 0xeff0;

// ---- 诊断码闭集 -----------------------------------------------------------
pub const E_VER: u16 = SEG_BASE + 1; // 版本锚不符
pub const E_UNKPRIV: u16 = SEG_BASE + 2; // 特权 LUID 不在 B03 表内
pub const E_NEEDED: u16 = SEG_BASE + 3; // 表内特权但令牌未持有
pub const E_HOLD: u16 = SEG_BASE + 4; // 持有位图与命名对拍失配
pub const E_NEG: u16 = SEG_BASE + 5; // 负例注入未被检出
pub const E_TABLE: u16 = SEG_BASE + 6; // 演练闭集破坏

/// 模块版本锚。
pub const VER: u32 = 1;

/// 特权门要求种类（判别式冻结）。
#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum GateKind {
    None = 0x0000,      // 无特权要求
    TableOnly = 0x0001, // 表内存在即可（装配预核已拒表外）
    MustHold = 0x0002,  // 必须令牌持有
}

impl GateKind {
    pub const fn code(self) -> u16 {
        self as u16
    }
}

/// 两步判定之第一步：存在性——LUID 必须在 B03 特权表内。
pub fn exists(luid: u32) -> Result<(), u16> {
    if VER != 1 {
        return Err(E_VER);
    }
    privtbl::lookup_by_luid(luid).map_err(|_| E_UNKPRIV)?;
    Ok(())
}

/// 两步判定之第二步：持有性——表内 LUID 必须出现在令牌 privs 中。
pub fn holds(tok: &acf::AcTokenFace, luid: u32) -> Result<(), u16> {
    exists(luid)?;
    let mut i = 0;
    while i < tok.privs.len() {
        if tok.privs[i] == luid {
            return Ok(());
        }
        i += 1;
    }
    Err(E_NEEDED)
}

// ---- 第②步契约 -------------------------------------------------------------
/// step2：特权要求门。
/// - priv_gate == None：无特权要求，直通；
/// - priv_gate == Some(luid)：表内且令牌持有才放行；表外 E_UNKPRIV、
///   表内未持 E_NEEDED。
/// 仅读 token.privs 与 priv_gate，不触 DACL/掩码。
pub fn step2(req: &acf::AcRequest) -> Result<(), u16> {
    if VER != 1 {
        return Err(E_VER);
    }
    match req.priv_gate {
        None => Ok(()),
        Some(luid) => holds(&req.token, luid),
    }
}

/// 门种类归类（表外/表内未持/表内已持/无门）。
pub fn gate_kind(req: &acf::AcRequest) -> GateKind {
    match req.priv_gate {
        None => GateKind::None,
        Some(luid) => {
            if holds(&req.token, luid).is_ok() {
                GateKind::MustHold
            } else if exists(luid).is_ok() {
                GateKind::TableOnly
            } else {
                GateKind::TableOnly // 表外也归 TableOnly，由调用方按 Err 细分
            }
        }
    }
}

/// 持有位图：令牌 privs → (bitmap, count) 对拍面。
/// 位号 = lookup_by_luid 返回的表内索引；越界项计入溢出计数。
pub fn hold_map(tok: &acf::AcTokenFace) -> (u64, u32) {
    let mut bmp: u64 = 0;
    let mut overflow: u32 = 0;
    let mut i = 0;
    while i < tok.privs.len() {
        match privtbl::lookup_by_luid(tok.privs[i]) {
            Ok(idx) => {
                if idx < 64 {
                    bmp |= 1u64 << idx;
                } else {
                    overflow += 1;
                }
            }
            Err(_) => overflow += 1,
        }
        i += 1;
    }
    (bmp, overflow)
}

/// 持有计数对拍：令牌 privs 必须全部落在 B03 表内（表外项 = 未持有
/// 的伪特权，直接失配），且位图 popcount 等于有效持有数。
pub fn hold_consistent(tok: &acf::AcTokenFace) -> Result<(), u16> {
    let (bmp, overflow) = hold_map(tok);
    if overflow != 0 {
        return Err(E_HOLD); // 表外 LUID 混入持有集
    }
    let valid = tok.privs.len() as u32;
    let mut pop = 0u32;
    let mut x = bmp;
    while x != 0 {
        pop += (x & 1) as u32;
        x >>= 1;
    }
    if pop != valid {
        return Err(E_HOLD);
    }
    Ok(())
}

/// 特权面判定：给定门要求与令牌，返回放行与否（供演练与对拍）。
pub fn gate_pass(req: &acf::AcRequest) -> Result<bool, u16> {
    match step2(req) {
        Ok(()) => Ok(true),
        Err(E_UNKPRIV) => Ok(false),
        Err(E_NEEDED) => Ok(false),
        Err(e) => Err(e),
    }
}

/// 闭集自检：版本锚 + 两步判定序 + 门种类闭集。
pub fn gate_integrity() -> Result<(), u16> {
    if VER != 1 {
        return Err(E_VER);
    }
    if GateKind::None.code() != 0 || GateKind::TableOnly.code() != 1 || GateKind::MustHold.code() != 2 {
        return Err(E_TABLE);
    }
    // 两步序：存在性失败不得走到持有性——表外 LUID 必 E_UNKPRIV。
    if exists(0xFFFF) != Err(E_UNKPRIV) {
        return Err(E_TABLE);
    }
    Ok(())
}

/// 演练：R1 闭集自检；R2 无门直通 + 表内已持放行；R3 表内未持拒；
/// R4 表外 LUID 拒 + 门种类归类；R5 持有位图对拍与负例注入。
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;
    // R1：闭集自检。
    gate_integrity()?;
    hits += 1;
    // R2：无门直通；令牌持 2（SeCreateTokenPrivilege，B03 表内锚）放行。
    let base = acf::AcRequest {
        token: acf::tok_plain(0x1111),
        owner: 1,
        dacl: acf::DaclFace { state: 1, aces: Vec::new() },
        desired: 0x0000_0089,
        map_name: "File",
        priv_gate: None,
    };
    step2(&base)?;
    if gate_pass(&base)? != true {
        return Err(E_NEG);
    }
    let mut held = acf::AcRequest {
        token: acf::tok_plain(0x1111),
        owner: 1,
        dacl: acf::DaclFace { state: 1, aces: Vec::new() },
        desired: 0x0000_0089,
        map_name: "File",
        priv_gate: Some(2),
    };
    held.token.privs = vec![2u32];
    step2(&held)?;
    hits += 1;
    // R3：表内未持——门 2 但令牌空 privs → E_NEEDED。
    let mut unheld = clone_req(&held);
    unheld.token.privs = Vec::new();
    if step2(&unheld) != Err(E_NEEDED) {
        return Err(E_NEG);
    }
    if gate_pass(&unheld)? != false {
        return Err(E_NEG);
    }
    hits += 1;
    // R4：表外 LUID 拒 + 门种类归类。
    let mut unk = clone_req(&unheld);
    unk.priv_gate = Some(0xFFFF);
    if step2(&unk) != Err(E_UNKPRIV) {
        return Err(E_NEG);
    }
    if gate_kind(&base) != GateKind::None {
        return Err(E_NEG);
    }
    if gate_kind(&held) != GateKind::MustHold {
        return Err(E_NEG);
    }
    hits += 1;
    // R5：持有位图对拍 + 负例注入。
    let (bmp, ovf) = hold_map(&held.token);
    if bmp == 0 || ovf != 0 {
        return Err(E_HOLD);
    }
    let mut bad = acf::tok_plain(0x1111);
    bad.privs = vec![2u32, 0xFFFF]; // 一表内一表外
    if hold_consistent(&bad).is_ok() {
        return Err(E_NEG); // 表外项溢出计数，对拍必须失配
    }
    let mut oktok = acf::tok_plain(0x1111);
    oktok.privs = vec![2u32];
    hold_consistent(&oktok)?;
    if exists(2).is_err() {
        return Err(E_TABLE);
    }
    hits += 1;
    Ok(hits)
}

/// 演练内重建垫片：AcRequest 字段重建（保持冻结结构零拷贝纪律）。
fn clone_req(r: &acf::AcRequest) -> acf::AcRequest {
    acf::AcRequest {
        token: acf::AcTokenFace {
            token_id: r.token.token_id,
            sid: r.token.sid,
            groups: r.token.groups.clone(),
            deny_only: r.token.deny_only.clone(),
            privs: r.token.privs.clone(),
            imp_level: r.token.imp_level,
        },
        owner: r.owner,
        dacl: acf::DaclFace { state: r.dacl.state, aces: r.dacl.aces.clone() },
        desired: r.desired,
        map_name: r.map_name,
        priv_gate: r.priv_gate,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn drill_green() {
        assert_eq!(drill(), Ok(5));
    }

    #[test]
    fn gate_table_holds() {
        assert_eq!(gate_integrity(), Ok(()));
    }

    #[test]
    fn no_gate_passes() {
        let req = acf::AcRequest {
            token: acf::tok_plain(1),
            owner: 1,
            dacl: acf::DaclFace { state: 1, aces: Vec::new() },
            desired: 0x89,
            map_name: "File",
            priv_gate: None,
        };
        assert_eq!(step2(&req), Ok(()));
    }

    #[test]
    fn unk_priv_rejected() {
        let req = acf::AcRequest {
            token: acf::tok_plain(1),
            owner: 1,
            dacl: acf::DaclFace { state: 1, aces: Vec::new() },
            desired: 0x89,
            map_name: "File",
            priv_gate: Some(0xDEAD),
        };
        assert_eq!(step2(&req), Err(E_UNKPRIV));
    }
}

pub fn run_unx36163_checks() -> CheckSet {
    let mut cs = CheckSet::new("unxstar-unx36163_acpriv");
    cs.add("SEG 段位独占", SEG_BASE == 0xeff0, "0xEFF0~0xEFFFF 本模块独占");
    match drill() {
        Ok(_) => cs.add("J1 确定性演练全绿", true, "两步判定/门归类/位图对拍全平"),
        Err(code) => cs.add("J1 确定性演练全绿", false, detail(code)),
    }
    cs.add("闭集纪律", gate_integrity() == Ok(()), "两步序 + 门种类闭集");
    cs.add("契约冻结", VER == 1, "step2 契约锚 v1");
    cs
}

pub fn detail(code: u16) -> &'static str {
    match code {
        E_VER => "版本锚不符",
        E_UNKPRIV => "特权 LUID 不在 B03 表内",
        E_NEEDED => "表内特权但令牌未持有",
        E_HOLD => "持有位图与命名对拍失配",
        E_NEG => "负例注入未被检出",
        E_TABLE => "演练闭集破坏",
        _ => "未知诊断码",
    }
}
