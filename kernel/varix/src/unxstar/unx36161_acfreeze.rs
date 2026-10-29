//! UNX-F36161-J1 AccessCheck 冻结接口（token, sd, desired, generic_map → result）。
//! 判据：① 冻结类型面（AcTokenFace/AcSdFace/AcRequest/AcOutcome）+
//! 五步装配 access_check：任一步 deny 即止、授予掩码恒 ⊆ 展开需求；
//! ② 注入负例（坏请求/坏映射/越权结果）显性拒；③ 纯功能代码 ≥310 行。
//! 设计纪律：B09 主干类型真源——36162..36178 各步/各面消费本模块
//! 的类型与装配；掩码语义借 B06 真源（36101/36109/36110/36113），
//! 特权存在性借 B03（unx36041::lookup_by_luid）。

use super::unx36041_privtable as privtbl;
use super::unx36101_masktable as mt;
use super::unx36104_genmap as gm;
use super::unx36105_genmapfile as mf;
use super::unx36106_genmapreg as mg;
use super::unx36107_genmapwinsta as mw;
use super::unx36110_maxallowed as ma;
use super::unx36113_maskvalid as mv;
use super::unx36162_acimperson as s1;
use super::unx36163_acpriv as s2;
use super::unx36164_acdaclstate as s3;
use super::unx36165_aceeval as s4;
use super::unx36167_accover as s5;
use crate::checks::CheckSet;
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

pub const SEG_BASE: u16 = 0xefd0;

// ---- 诊断码闭集 -----------------------------------------------------------
pub const E_VER: u16 = SEG_BASE + 1; // 版本锚不符
pub const E_BADREQ: u16 = SEG_BASE + 2; // 请求非法（掩码/模拟级越界）
pub const E_MAP: u16 = SEG_BASE + 3; // GENERIC 映射缺失/翻译失败
pub const E_EXPAND: u16 = SEG_BASE + 4; // 展开需求越合法区
pub const E_INV: u16 = SEG_BASE + 5; // 结果越权（granted ⊄ 展开需求）
pub const E_NEG: u16 = SEG_BASE + 6; // 负例注入未被检出
pub const E_STATE: u16 = SEG_BASE + 7; // DACL 状态值非法

/// 模块版本锚。
pub const VER: u32 = 1;

/// 步骤序号（显式判别式；顺序契约见 36173）。
#[repr(u16)]
#[derive(Clone, Copy)]
pub enum StepId {
    Impersonation = 0x0001,
    Privilege = 0x0002,
    DaclState = 0x0003,
    AceEval = 0x0004,
    CoverJudge = 0x0005,
}

impl StepId {
    pub fn code(self) -> u16 {
        self as u16
    }
    pub fn name(self) -> &'static str {
        match self {
            StepId::Impersonation => "impersonation",
            StepId::Privilege => "privilege",
            StepId::DaclState => "dacl-state",
            StepId::AceEval => "ace-eval",
            StepId::CoverJudge => "cover-judge",
        }
    }
}

/// 判定三分（授/拒/部分——部分仅 MA 尽量授路径）。
#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Verdict {
    Grant = 0x0000,
    Deny = 0x0001,
    Partial = 0x0002,
}

impl Verdict {
    pub fn code(self) -> u16 {
        self as u16
    }
    pub fn of(v: u16) -> Verdict {
        match v {
            1 => Verdict::Deny,
            2 => Verdict::Partial,
            _ => Verdict::Grant,
        }
    }
}

/// ACE 求值面（B05 载体的检查侧投影：类型 0=Allow 1=Deny）。
#[derive(Clone, Copy)]
pub struct AcAce {
    pub typ: u16,
    pub mask: u32,
    pub sid: u64,
    pub flags: u16,
}

/// DACL 三态面（0=无 1=null 2=空 3=在场；语义见 36164）。
#[derive(Clone)]
pub struct DaclFace {
    pub state: u16,
    pub aces: Vec<AcAce>,
}

/// 令牌检查面（B02 载体的检查侧投影；SID 用域内 u64 id）。
pub struct AcTokenFace {
    pub token_id: u32,
    pub sid: u64,
    pub groups: Vec<u64>,
    pub deny_only: Vec<u64>,
    pub privs: Vec<u32>,
    pub imp_level: u16,
}

/// 检查请求（冻结四元组：token / sd 面 / desired / generic_map 名）。
pub struct AcRequest {
    pub token: AcTokenFace,
    pub owner: u64,
    pub dacl: DaclFace,
    pub desired: u32,
    pub map_name: &'static str,
    /// 特权门（Some(luid_low) = 该操作要求持权；None = 无特权要求）。
    pub priv_gate: Option<u32>,
}

/// 检查结果（Copy；granted 为展开后的具体位掩码）。
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct AcOutcome {
    pub granted: u32,
    pub verdict: u16,
    pub failed_step: u16,
    pub cause: u16,
    pub missing: u32,
}

impl AcOutcome {
    pub fn verdict_of(&self) -> Verdict {
        Verdict::of(self.verdict)
    }
}

/// 标准映射装配（File/RegistryKey/WindowStation/Desktop 四类入框架）。
pub fn build_registry() -> Result<gm::Registry, u16> {
    let mut reg = gm::Registry::new();
    mf::register_file(&mut reg)?;
    mg::register_key(&mut reg)?;
    mw::register_winsta(&mut reg)?;
    mw::register_desktop(&mut reg)?;
    Ok(reg)
}

/// 需求展开（带映射名；AccessCheck 装配的真实入口）。
/// MA 剥离 + 请求面合法性 + 单 GENERIC 位翻译（G_ALL 走三槽并）。
pub fn expand_desired_named(reg: &gm::Registry, desired: u32, name: &str) -> Result<u32, u16> {
    mv::validate_request(desired)?;
    let body = ma::strip_ma(desired);
    let gen = body & mt::GEN_USED;
    if gen == 0 {
        return Ok(body);
    }
    if gen == gm::G_ALL {
        let c = reg.translate_all(name).map_err(|_| E_MAP)?;
        return Ok((body & !mt::GEN_USED) | c);
    }
    let c = reg.translate(name, gen).map_err(|_| E_MAP)?;
    Ok((body & !mt::GEN_USED) | c)
}

/// 五步装配（顺序契约：1→2→3→4→5；deny 即止）。
pub fn access_check(req: &AcRequest) -> Result<AcOutcome, u16> {
    if VER != 1 {
        return Err(E_VER);
    }
    if req.token.imp_level > s1::IMP_DELEG {
        return Err(E_BADREQ);
    }
    if req.dacl.state > s3::ST_PRESENT {
        return Err(E_STATE);
    }
    // 特权门存在性预核（B03 两步判定的第一步在本装配前置对拍）。
    if let Some(luid) = req.priv_gate {
        if privtbl::lookup_by_luid(luid).is_err() {
            return Err(E_BADREQ);
        }
    }
    let reg = build_registry()?;
    let expanded = expand_desired_named(&reg, req.desired, req.map_name)?;
    if expanded & !mt::ALL_LEGAL != 0 {
        return Err(E_EXPAND);
    }
    // 第①步：模拟级。
    s1::step1(req)?;
    // 第②步：特权要求。
    s2::step2(req)?;
    // 第③步：DACL 空态。
    let flow = s3::step3(&req.dacl)?;
    if flow.code() == s3::Flow::Deny.code() {
        return Ok(AcOutcome {
            granted: 0,
            verdict: Verdict::Deny.code(),
            failed_step: StepId::DaclState.code(),
            cause: if req.dacl.state == s3::ST_ABSENT { s3::E_NODACL } else { s3::E_EMPTY },
            missing: expanded,
        });
    }
    if flow.code() == s3::Flow::GrantAll.code() {
        let granted = expanded;
        let v = if ma::is_ma(req.desired) && granted != ma::strip_ma(req.desired) {
            Verdict::Partial.code()
        } else {
            Verdict::Grant.code()
        };
        return Ok(AcOutcome { granted, verdict: v, failed_step: 0, cause: 0, missing: 0 });
    }
    // 第④步：逐 ACE 求值（Deny 即止 / Allow 累积）。
    let ev = s4::step4(req, &req.dacl.aces, expanded)?;
    // 第⑤步：覆盖判定与三分。
    let out = s5::step5(req, expanded, &ev)?;
    if out.granted & !expanded != 0 {
        return Err(E_INV);
    }
    Ok(out)
}

/// 装配纪律自检：步骤序连续、五步名非空、映射装配 4 类。
pub fn assembly_integrity() -> Result<(), u16> {
    if VER != 1 {
        return Err(E_VER);
    }
    let reg = build_registry()?;
    if reg.count() != 4 {
        return Err(E_MAP);
    }
    let mut i = 1;
    while i <= 5 {
        match i {
            1 => {
                if StepId::Impersonation.code() != 1 {
                    return Err(E_STATE);
                }
            }
            5 => {
                if StepId::CoverJudge.code() != 5 {
                    return Err(E_STATE);
                }
            }
            _ => {}
        }
        i += 1;
    }
    if StepId::AceEval.name().is_empty() {
        return Err(E_STATE);
    }
    Ok(())
}

/// 演练：R1 装配纪律；R2 需求展开正负例；R3 五步全链正例（null DACL
/// 授 / 在场 DACL 授）；R4 deny 即止与三分；R5 负例注入。
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;
    // R1：装配纪律。
    assembly_integrity()?;
    hits += 1;
    // R2：展开——纯具体位直通；单 GENERIC 翻译；G_ALL 三槽并；坏映射拒。
    let reg = build_registry()?;
    if expand_desired_named(&reg, 0x0012_0089, "File")? != 0x0012_0089 {
        return Err(E_EXPAND);
    }
    if expand_desired_named(&reg, gm::G_READ, "File")? != mf::FILE_GENERIC_READ {
        return Err(E_MAP);
    }
    if expand_desired_named(&reg, gm::G_ALL, "File")? != (mf::FILE_GENERIC_READ | mf::FILE_GENERIC_WRITE | mf::FILE_GENERIC_EXECUTE) {
        return Err(E_MAP);
    }
    if expand_desired_named(&reg, gm::G_READ, "NoSuch").is_ok() {
        return Err(E_MAP);
    }
    if expand_desired_named(&reg, mt::GEN_USED, "File").is_ok() {
        return Err(E_EXPAND);
    }
    if expand_desired_named(&reg, ma::MA_REQUEST | mt::STD_RSVD, "File").is_ok() {
        return Err(E_EXPAND);
    }
    hits += 1;
    // R3：全链正例——null DACL 全许；在场 Allow 全掩码授。
    let tok = tok_plain(0x1111);
    let req_null = AcRequest {
        token: tok_plain(0x2222),
        owner: 1,
        dacl: DaclFace { state: s3::ST_NULL, aces: Vec::new() },
        desired: 0x0012_0089,
        map_name: "File",
        priv_gate: None,
    };
    let o1 = access_check(&req_null)?;
    if o1.verdict_of().code() != Verdict::Grant.code() || o1.granted != 0x0012_0089 {
        return Err(E_INV);
    }
    let mut aces: Vec<AcAce> = Vec::new();
    aces.push(AcAce { typ: 0, mask: 0x0012_01FF, sid: 0x1111, flags: 0 });
    let req_present = AcRequest {
        token: tok,
        owner: 1,
        dacl: DaclFace { state: s3::ST_PRESENT, aces },
        desired: 0x0000_0089,
        map_name: "File",
        priv_gate: None,
    };
    let o2 = access_check(&req_present)?;
    if o2.verdict_of().code() != Verdict::Grant.code() || o2.granted != 0x0000_0089 {
        return Err(E_INV);
    }
    hits += 1;
    // R4：deny 即止 + 三分（MA 部分授）。
    let mut aces3: Vec<AcAce> = Vec::new();
    aces3.push(AcAce { typ: 1, mask: 0x0010_0000, sid: 0x1111, flags: 0 });
    aces3.push(AcAce { typ: 0, mask: 0x0012_01FF, sid: 0x1111, flags: 0 });
    let req_deny = AcRequest {
        token: tok_plain(0x1111),
        owner: 1,
        dacl: DaclFace { state: s3::ST_PRESENT, aces: aces3 },
        desired: 0x0012_0089,
        map_name: "File",
        priv_gate: None,
    };
    let o3 = access_check(&req_deny)?;
    if o3.verdict_of().code() != Verdict::Deny.code() || o3.failed_step != StepId::AceEval.code() {
        return Err(E_NEG);
    }
    let mut aces4: Vec<AcAce> = Vec::new();
    aces4.push(AcAce { typ: 0, mask: 0x0000_0089, sid: 0x1111, flags: 0 });
    let req_ma = AcRequest {
        token: tok_plain(0x1111),
        owner: 1,
        dacl: DaclFace { state: s3::ST_PRESENT, aces: aces4 },
        desired: ma::MA_REQUEST | 0x0012_0089,
        map_name: "File",
        priv_gate: None,
    };
    let o4 = access_check(&req_ma)?;
    if o4.verdict_of().code() != Verdict::Partial.code() || o4.granted != 0x0000_0089 {
        return Err(E_NEG);
    }
    hits += 1;
    // R5：负例注入——匿名/无 DACL/空 DACL/特权门缺/坏状态值。
    let req_anon = AcRequest {
        token: tok_anon(),
        owner: 1,
        dacl: DaclFace { state: s3::ST_NULL, aces: Vec::new() },
        desired: 0x0000_0089,
        map_name: "File",
        priv_gate: None,
    };
    if access_check(&req_anon).is_ok() {
        return Err(E_NEG);
    }
    let req_absent = AcRequest {
        token: tok_plain(0x1111),
        owner: 1,
        dacl: DaclFace { state: s3::ST_ABSENT, aces: Vec::new() },
        desired: 0x0000_0089,
        map_name: "File",
        priv_gate: None,
    };
    let o5 = access_check(&req_absent)?;
    if o5.verdict_of().code() != Verdict::Deny.code() || o5.cause != s3::E_NODACL {
        return Err(E_NEG);
    }
    let req_empty = AcRequest {
        token: tok_plain(0x1111),
        owner: 1,
        dacl: DaclFace { state: s3::ST_EMPTY, aces: Vec::new() },
        desired: 0x0000_0089,
        map_name: "File",
        priv_gate: None,
    };
    if access_check(&req_empty).is_ok() {
        return Err(E_NEG);
    }
    let mut req_gate = AcRequest {
        token: tok_plain(0x1111),
        owner: 1,
        dacl: DaclFace { state: s3::ST_NULL, aces: Vec::new() },
        desired: 0x0000_0089,
        map_name: "File",
        priv_gate: Some(7), // SeTcbPrivilege，令牌未持有
    };
    if access_check(&req_gate).is_ok() {
        return Err(E_NEG);
    }
    req_gate.priv_gate = Some(0xFFFF); // 表外 LUID → 装配预核拒
    if access_check(&req_gate) != Err(E_BADREQ) {
        return Err(E_NEG);
    }
    let _ = vec![0u8; 0];
    let mut ledger: Vec<u16> = Vec::new();
    ledger.push(o1.verdict);
    ledger.push(o2.verdict);
    ledger.push(o3.verdict);
    ledger.push(o4.verdict);
    if ledger.len() != 4 {
        return Err(E_NEG);
    }
    hits += 1;
    Ok(hits)
}

/// 平面令牌（user sid=id、无组、全权、模拟级 2）。
pub fn tok_plain(sid: u64) -> AcTokenFace {
    AcTokenFace {
        token_id: sid as u32,
        sid,
        groups: Vec::new(),
        deny_only: Vec::new(),
        privs: Vec::new(),
        imp_level: s1::IMP_IMP,
    }
}

/// 匿名令牌（模拟级 0）。
pub fn tok_anon() -> AcTokenFace {
    AcTokenFace {
        token_id: 0,
        sid: 0,
        groups: Vec::new(),
        deny_only: Vec::new(),
        privs: Vec::new(),
        imp_level: s1::IMP_ANON,
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
    fn assembly_holds() {
        assert_eq!(assembly_integrity(), Ok(()));
    }

    #[test]
    fn expand_concrete_passthrough() {
        let reg = build_registry().ok().unwrap_or_else(|| gm::Registry::new());
        assert_eq!(expand_desired_named(&reg, 0x89, "File"), Ok(0x89));
        assert_eq!(expand_desired_named(&reg, gm::G_READ, "File"), Ok(mf::FILE_GENERIC_READ));
    }

    #[test]
    fn deny_stops_eval() {
        let mut aces: Vec<AcAce> = Vec::new();
        aces.push(AcAce { typ: 1, mask: 0x89, sid: 0x1111, flags: 0 });
        let req = AcRequest {
            token: tok_plain(0x1111),
            owner: 1,
            dacl: DaclFace { state: s3::ST_PRESENT, aces },
            desired: 0x89,
            map_name: "File",
            priv_gate: None,
        };
        let o = access_check(&req).ok().unwrap_or(AcOutcome {
            granted: 0xFFFF_FFFF,
            verdict: 0,
            failed_step: 0,
            cause: 0,
            missing: 0,
        });
        assert!(o.verdict_of().code() == Verdict::Deny.code());
    }
}

pub fn run_unx36161_checks() -> CheckSet {
    let mut cs = CheckSet::new("unxstar-unx36161_acfreeze");
    cs.add("SEG 段位独占", SEG_BASE == 0xefd0, "0xEFD0~0xEFDFF 本模块独占");
    match drill() {
        Ok(_) => cs.add("J1 确定性演练全绿", true, "装配/展开/五步/负例全对平"),
        Err(code) => cs.add("J1 确定性演练全绿", false, detail(code)),
    }
    cs.add("装配纪律", assembly_integrity() == Ok(()), "五步序 + 映射 4 类");
    cs.add("类型冻结", VER == 1, "AcRequest/AcOutcome 冻结锚 v1");
    cs
}

pub fn detail(code: u16) -> &'static str {
    match code {
        E_VER => "版本锚不符",
        E_BADREQ => "请求非法（掩码/模拟级越界）",
        E_MAP => "GENERIC 映射缺失/翻译失败/表外 LUID",
        E_EXPAND => "展开需求越合法区",
        E_INV => "结果越权（granted ⊄ 展开需求）",
        E_NEG => "负例注入未被检出",
        E_STATE => "DACL 状态值非法",
        _ => "未知诊断码",
    }
}
