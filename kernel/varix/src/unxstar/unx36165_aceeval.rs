//! UNX-F36165-J1 AccessCheck 第④步：逐 ACE 求值（aceeval）。
//! 判据：① EvalOut 冻结四元（granted/denied_hit/denied_by/deny_union）+
//! step4 冻结签名 `step4(&AcRequest, &[AcAce], u32) -> Result<EvalOut, u16>`
//! ——Deny 即止（首个命中 Deny 停扫）、Allow 累积（mask ∩ expanded）、
//! MA 请求只累计不判负（never-fail 语义留给第⑤步）；② SID 匹配委托
//! 36166（ace_applies），本模块不复述匹配规则；③ 注入负例显性拒；④ ≥310 行。
//! 设计纪律：36161 通过 `super::unx36165_aceeval as s4` 消费 step4 与
//! EvalOut；ACE 类型面（0=Allow 1=Deny）真源在 36161::AcAce.typ。

use super::unx36161_acfreeze as acf;
use super::unx36166_acsidmatch as sm;
use crate::checks::CheckSet;
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

pub const SEG_BASE: u16 = 0x3c10;

// ---- 诊断码闭集 -----------------------------------------------------------
pub const E_VER: u16 = SEG_BASE + 1; // 版本锚不符
pub const E_ACE: u16 = SEG_BASE + 2; // ACE 类型越出 {0=Allow,1=Deny}
pub const E_SHAPE: u16 = SEG_BASE + 3; // 求值面形不一致（空表 PRESENT 等）
pub const E_NEG: u16 = SEG_BASE + 4; // 负例注入未被检出
pub const E_TABLE: u16 = SEG_BASE + 5; // 求值序闭集破坏

/// 模块版本锚。
pub const VER: u32 = 1;

/// ACE 类型常数（36161::AcAce.typ 的检查侧锚）。
pub const ACE_ALLOW: u16 = 0x0000;
pub const ACE_DENY: u16 = 0x0001;

/// 求值输出（冻结四元；Copy 供第⑤步零拷贝消费）。
#[derive(Clone, Copy)]
pub struct EvalOut {
    /// 已累积的 Allow 掩码（恒 ⊆ expanded，由装配 E_INV 兜底校验）。
    pub granted: u32,
    /// 是否有命中请求位的 Deny ACE（true → 第⑤步必判 Deny）。
    pub denied_hit: bool,
    /// 首个命中 Deny 的 ACE 主体 SID。
    pub denied_by: u64,
    /// 所有命中 Deny 掩码的并集（MA 尽量授路径的扣减源）。
    pub deny_union: u32,
}

impl EvalOut {
    pub fn fresh() -> EvalOut {
        EvalOut { granted: 0, denied_hit: false, denied_by: 0, deny_union: 0 }
    }
}

/// 求值轨迹行（审计面：type/sid/mask/命中位）。
#[derive(Clone, Copy)]
pub struct TraceRow {
    pub typ: u16,
    pub sid: u64,
    pub mask: u32,
    pub applied: bool,
}

/// 单条 ACE 求值：返回 (applied, deny_hit)。
/// - 非 {0,1} 类型 → Err(E_ACE)；
/// - inherit-only ACE（flags 含 F_INHERIT_ONLY）不适用（见 36166）；
/// - Deny：SID 命中且 mask ∩ expanded ≠ 0 → 命中；
/// - Allow：SID 命中 → 累积 granted（由调用方合并）。
pub fn eval_one(
    ace: &acf::AcAce,
    tok: &acf::AcTokenFace,
    expanded: u32,
    granted: &mut u32,
) -> Result<(bool, bool), u16> {
    if VER != 1 {
        return Err(E_VER);
    }
    if ace.typ != ACE_ALLOW && ace.typ != ACE_DENY {
        return Err(E_ACE);
    }
    if !sm::ace_applies(ace, tok) {
        return Ok((false, false));
    }
    if ace.typ == ACE_DENY {
        let hit = ace.mask & expanded != 0;
        if hit {
            *granted &= !ace.mask; // Deny 扣减累积面（先 deny 后 allow 时序保护）
        }
        return Ok((true, hit));
    }
    *granted |= ace.mask & expanded;
    Ok((true, false))
}

// ---- 第④步契约 -------------------------------------------------------------
/// step4：逐 ACE 求值。扫序 = DACL 表序（Deny 即止由命中短路实现：
/// 命中请求位的 Deny 一出现立即返回，后续 Allow 不再累积）。
/// MA 请求（desired 含 MA_REQUEST）不触发 denied_hit 短路——只记录
/// deny_union 供第⑤步尽量授扣减（never-fail）。
pub fn step4(
    req: &acf::AcRequest,
    aces: &[acf::AcAce],
    expanded: u32,
) -> Result<EvalOut, u16> {
    if VER != 1 {
        return Err(E_VER);
    }
    let mut out = EvalOut::fresh();
    let mut i = 0;
    while i < aces.len() {
        let ace = &aces[i];
        if ace.typ != ACE_ALLOW && ace.typ != ACE_DENY {
            return Err(E_ACE);
        }
        let applies = sm::ace_applies(ace, &req.token);
        if applies && ace.typ == ACE_DENY && ace.mask & expanded != 0 {
            out.deny_union |= ace.mask & expanded;
            if !ma_request(req) {
                out.denied_hit = true;
                out.denied_by = ace.sid;
                break; // Deny 即止
            }
        }
        if applies && ace.typ == ACE_ALLOW {
            out.granted |= ace.mask & expanded;
            out.granted &= !out.deny_union;
        }
        i += 1;
    }
    Ok(out)
}

/// MA 请求判定（读 desired 的请求位；语义真源 36110::is_ma）。
fn ma_request(req: &acf::AcRequest) -> bool {
    use super::unx36110_maxallowed as ma;
    ma::is_ma(req.desired)
}

/// 全表轨迹面：不短路地扫完 DACL，产出逐 ACE 轨迹行（审计/对拍用）。
/// 与 step4 的差异：Deny 命中不停扫（step4 的语义由 step5 消费
/// denied_hit 实现，本函数只提供观测面）；类型非法仍报 E_ACE。
pub fn eval_trace(
    req: &acf::AcRequest,
    aces: &[acf::AcAce],
    expanded: u32,
) -> Result<Vec<TraceRow>, u16> {
    if VER != 1 {
        return Err(E_VER);
    }
    let mut rows: Vec<TraceRow> = Vec::new();
    let mut i = 0;
    while i < aces.len() {
        let ace = &aces[i];
        if ace.typ != ACE_ALLOW && ace.typ != ACE_DENY {
            return Err(E_ACE);
        }
        let applies = sm::ace_applies(ace, &req.token) && ace.mask & expanded != 0;
        rows.push(TraceRow { typ: ace.typ, sid: ace.sid, mask: ace.mask, applied: applies });
        i += 1;
    }
    Ok(rows)
}

/// 轨迹聚合：命中行计数（Deny 命中 / Allow 命中分桶）。
pub fn trace_summary(rows: &[TraceRow]) -> (u32, u32) {
    let mut denies = 0u32;
    let mut allows = 0u32;
    let mut i = 0;
    while i < rows.len() {
        if rows[i].applied {
            if rows[i].typ == ACE_DENY {
                denies += 1;
            } else {
                allows += 1;
            }
        }
        i += 1;
    }
    (denies, allows)
}

/// 求值序自检：ACE 常数闭集 + EvalOut 初值 + 类型守卫。
pub fn eval_integrity() -> Result<(), u16> {
    if VER != 1 {
        return Err(E_VER);
    }
    if ACE_ALLOW != 0 || ACE_DENY != 1 {
        return Err(E_TABLE);
    }
    let f = EvalOut::fresh();
    if f.granted != 0 || f.denied_hit || f.denied_by != 0 || f.deny_union != 0 {
        return Err(E_TABLE);
    }
    let bad = acf::AcAce { typ: 5, mask: 1, sid: 1, flags: 0 };
    let mut g = 0u32;
    let tok = acf::tok_plain(1);
    if eval_one(&bad, &tok, 0xFF, &mut g) != Err(E_ACE) {
        return Err(E_TABLE);
    }
    Ok(())
}

/// 演练：R1 闭集自检；R2 Allow 累积与子集裁剪；R3 Deny 即止与主体记录；
/// R4 MA 尽量授（不短路、记录 deny_union）；R5 负例注入（坏类型/不适用
/// ACE/全不命中/继承位跳过）。
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;
    // R1：闭集自检。
    eval_integrity()?;
    hits += 1;
    // R2：Allow 累积——两条 Allow 命中并集；SID 不命中不累积。
    let tok = acf::tok_plain(0x1111);
    let mut aces: Vec<acf::AcAce> = Vec::new();
    aces.push(acf::AcAce { typ: ACE_ALLOW, mask: 0x0000_0089, sid: 0x1111, flags: 0 });
    aces.push(acf::AcAce { typ: ACE_ALLOW, mask: 0x0010_0000, sid: 0x1111, flags: 0 });
    aces.push(acf::AcAce { typ: ACE_ALLOW, mask: 0x0002_0000, sid: 0x9999, flags: 0 });
    let req = acf::AcRequest {
        token: acf::tok_plain(0x1111),
        owner: 1,
        dacl: acf::DaclFace { state: 3, aces: Vec::new() },
        desired: 0x0012_0089,
        map_name: "File",
        priv_gate: None,
    };
    let ev = step4(&req, &aces, 0x0012_0089)?;
    if ev.granted != 0x0010_0089 || ev.denied_hit {
        return Err(E_NEG);
    }
    hits += 1;
    // R3：Deny 即止——首条命中 Deny 停扫，granted 保持 0，主体记录。
    let mut aces3: Vec<acf::AcAce> = Vec::new();
    aces3.push(acf::AcAce { typ: ACE_DENY, mask: 0x0010_0000, sid: 0x1111, flags: 0 });
    aces3.push(acf::AcAce { typ: ACE_ALLOW, mask: 0x0012_01FF, sid: 0x1111, flags: 0 });
    let ev3 = step4(&req, &aces3, 0x0012_0089)?;
    if !ev3.denied_hit || ev3.denied_by != 0x1111 || ev3.granted != 0 {
        return Err(E_NEG);
    }
    if ev3.deny_union != 0x0010_0000 {
        return Err(E_NEG);
    }
    hits += 1;
    // R4：MA 尽量授——同一 DACL 下 MA 请求不短路，Allow 累积扣除 Deny 位。
    let req_ma = acf::AcRequest {
        token: acf::tok_plain(0x1111),
        owner: 1,
        dacl: acf::DaclFace { state: 3, aces: Vec::new() },
        desired: 0x0200_0000 | 0x0012_0089,
        map_name: "File",
        priv_gate: None,
    };
    let ev4 = step4(&req_ma, &aces3, 0x0012_0089)?;
    if ev4.denied_hit {
        return Err(E_NEG);
    }
    if ev4.granted != 0x0012_01FF & !(0x0010_0000) & 0x0012_0089 {
        return Err(E_NEG);
    }
    hits += 1;
    // R5：负例注入——坏类型 / 不适用 ACE / 全不命中 / 继承位跳过。
    let mut bad_aces: Vec<acf::AcAce> = Vec::new();
    bad_aces.push(acf::AcAce { typ: 9, mask: 1, sid: 0x1111, flags: 0 });
    if step4(&req, &bad_aces, 0xFF).is_ok() {
        return Err(E_NEG);
    }
    let none_hit = vec![acf::AcAce { typ: ACE_ALLOW, mask: 0x0040_0000, sid: 0x9999, flags: 0 }];
    let ev5 = step4(&req, &none_hit, 0x0012_0089)?;
    if ev5.granted != 0 || ev5.denied_hit {
        return Err(E_NEG);
    }
    let inherit_only = vec![acf::AcAce {
        typ: ACE_ALLOW,
        mask: 0x0000_0089,
        sid: 0x1111,
        flags: sm::F_INHERIT_ONLY,
    }];
    let ev6 = step4(&req, &inherit_only, 0x0000_0089)?;
    if ev6.granted != 0 {
        return Err(E_NEG);
    }
    hits += 1;
    // R6：全表轨迹——Deny 不短路观测面命中分桶、坏类型报错、空表零轨迹。
    let tr = eval_trace(&req, &aces3, 0x0012_0089)?;
    if tr.len() != 2 {
        return Err(E_NEG);
    }
    let (dn, al) = trace_summary(&tr);
    if dn != 1 || al != 1 {
        return Err(E_NEG);
    }
    if eval_trace(&req, &bad_aces, 0xFF).is_ok() {
        return Err(E_NEG);
    }
    let tr0 = eval_trace(&req, &[], 0x0012_0089)?;
    if !tr0.is_empty() || trace_summary(&tr0) != (0, 0) {
        return Err(E_NEG);
    }
    hits += 1;
    Ok(hits)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn drill_green() {
        assert_eq!(drill(), Ok(6));
    }

    #[test]
    fn eval_table_holds() {
        assert_eq!(eval_integrity(), Ok(()));
    }

    #[test]
    fn deny_stops_scan() {
        let aces = vec![
            acf::AcAce { typ: ACE_DENY, mask: 0x89, sid: 7, flags: 0 },
            acf::AcAce { typ: ACE_ALLOW, mask: 0xFF, sid: 7, flags: 0 },
        ];
        let req = acf::AcRequest {
            token: acf::tok_plain(7),
            owner: 1,
            dacl: acf::DaclFace { state: 3, aces: Vec::new() },
            desired: 0x89,
            map_name: "File",
            priv_gate: None,
        };
        let ev = step4(&req, &aces, 0x89).ok().unwrap_or_else(|| EvalOut {
            granted: 0xFFFF_FFFF,
            denied_hit: false,
            denied_by: 0,
            deny_union: 0,
        });
        assert!(ev.denied_hit && ev.granted == 0);
    }
}

pub fn run_unx36165_checks() -> CheckSet {
    let mut cs = CheckSet::new("unxstar-unx36165_aceeval");
    cs.add("SEG 段位独占", SEG_BASE == 0x3c10, "0x3C10~0x3C1F 本模块独占");
    match drill() {
        Ok(_) => cs.add("J1 确定性演练全绿", true, "累积/即止/MA 尽量授/负例全平"),
        Err(code) => cs.add("J1 确定性演练全绿", false, detail(code)),
    }
    cs.add("闭集纪律", eval_integrity() == Ok(()), "ACE 常数 + EvalOut 初值 + 类型守卫");
    cs.add("契约冻结", VER == 1, "step4 契约锚 v1");
    cs
}

pub fn detail(code: u16) -> &'static str {
    match code {
        E_VER => "版本锚不符",
        E_ACE => "ACE 类型越出 {0=Allow,1=Deny}",
        E_SHAPE => "求值面形不一致",
        E_NEG => "负例注入未被检出",
        E_TABLE => "求值序闭集破坏",
        _ => "未知诊断码",
    }
}
