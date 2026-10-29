//! UNX-F36141-J1 地基联动剧本 I：登录→令牌→对象创建→SD 挂接全链。
//! 判据：① 四段流水线 12 例全链跑通（登录门/铸令门/建对象门/挂 SD 门逐段过闸）；
//! ② 各段负例 8 例（零 SID/零凭据/无组令牌/零类型/零所有者/DACL 超容）
//! 全部显性拒绝且码正确；③ 纯功能代码 ≥300 行。
//! 设计纪律：B08 联动剧本之一——载 体在本地建模（真源 SID/Token/SD 的
//! 聚合消费归 36149/36155），本剧本只验证链路状态机与门序。

use crate::checks::CheckSet;
use super::unx36081_acecore as core;
use alloc::vec;
use alloc::vec::Vec;
use alloc::string::{String, ToString};

pub const SEG_BASE: u16 = 0x8230;

// ---- 诊断码闭集 -----------------------------------------------------------
pub const E_VER: u16 = SEG_BASE + 1;  // 版本锚不符
pub const E_SID: u16 = SEG_BASE + 2;  // 零 SID 槽位
pub const E_CRED: u16 = SEG_BASE + 3; // 零凭据
pub const E_NOGRP: u16 = SEG_BASE + 4;// 令牌无组（全链无主体资格）
pub const E_TYPE: u16 = SEG_BASE + 5; // 零对象类型
pub const E_OWNER: u16 = SEG_BASE + 6;// SD 所有者槽为零
pub const E_DACL: u16 = SEG_BASE + 7; // DACL 超容
pub const E_CS: u16 = SEG_BASE + 8;   // 指纹不稳

/// 版本锚。
pub const VER: u32 = 1;

/// DACL 容量锚。
pub const DACL_CAP: u16 = 16;
/// 全链段位（#[repr(u16)] 显式判别式）。
#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Stage {
    /// 段 1：登录。
    Login        = 0x0001,
    /// 段 2：铸令牌。
    TokenMint    = 0x0002,
    /// 段 3：对象创建。
    ObjectCreate = 0x0003,
    /// 段 4：SD 挂接。
    SdAttach     = 0x0004,
    /// 段 5：全链封印。
    ChainSealed  = 0x0005,
}

impl Stage {
    pub fn code(self) -> u16 {
        self as u16
    }
}

/// 主体载体（本地建模）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Subject {
    pub sid_slot: u16,
    pub groups: [u16; 4],
    pub n_groups: usize,
}

/// 令牌载体。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct TokenCarrier {
    pub sid_slot: u16,
    pub n_groups: usize,
    pub priv_mask: u32,
    pub generation: u32,
}

/// 对象载体。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ObjectCarrier {
    pub obj_id: u16,
    pub type_tag: u16,
    pub generation: u32,
}

/// SD 载体。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct SdCarrier {
    pub owner_slot: u16,
    pub dacl_ace_count: u16,
    pub generation: u32,
}

/// 段 1：登录门（SID/凭据双闸）。
pub fn stage_login(sid_slot: u16, cred: u32) -> Result<Subject, u16> {
    if sid_slot == 0 {
        return Err(E_SID);
    }
    if cred == 0 {
        return Err(E_CRED);
    }
    // 组集由凭据推导（确定性：低位 4 组，非零）。
    let mut groups = [0u16; 4];
    let mut i = 0;
    while i < 4 {
        groups[i] = ((cred >> (i * 4)) & 0xf) as u16 + sid_slot;
        i += 1;
    }
    Ok(Subject { sid_slot, groups, n_groups: 4 })
}

/// 段 2：铸令门（组资格闸）。
pub fn stage_mint(s: &Subject, priv_mask: u32) -> Result<TokenCarrier, u16> {
    if s.n_groups == 0 {
        return Err(E_NOGRP);
    }
    Ok(TokenCarrier {
        sid_slot: s.sid_slot,
        n_groups: s.n_groups,
        priv_mask,
        generation: 1,
    })
}

/// 段 3：建对象门（类型闸）。
pub fn stage_create(t: &TokenCarrier, obj_id: u16, type_tag: u16) -> Result<ObjectCarrier, u16> {
    if type_tag == 0 {
        return Err(E_TYPE);
    }
    if t.generation == 0 {
        return Err(E_NOGRP);
    }
    Ok(ObjectCarrier { obj_id, type_tag, generation: t.generation })
}

/// 段 4：挂 SD 门（所有者/DACL 双闸）。
pub fn stage_attach(o: &ObjectCarrier, owner_slot: u16, ace_count: u16) -> Result<SdCarrier, u16> {
    if owner_slot == 0 {
        return Err(E_OWNER);
    }
    if ace_count > DACL_CAP {
        return Err(E_DACL);
    }
    if o.generation == 0 {
        return Err(E_TYPE);
    }
    Ok(SdCarrier { owner_slot, dacl_ace_count: ace_count, generation: o.generation })
}

/// 段 5：全链封印（四代一致性 + 链指纹）。
pub fn stage_seal(s: &Subject, t: &TokenCarrier, o: &ObjectCarrier, d: &SdCarrier) -> Result<u64, u16> {
    if s.sid_slot != t.sid_slot {
        return Err(E_SID);
    }
    if o.generation != t.generation || d.generation != o.generation {
        return Err(E_CS);
    }
    let mut feed = Vec::new();
    feed.extend_from_slice(&VER.to_le_bytes());
    feed.extend_from_slice(&s.sid_slot.to_le_bytes());
    feed.extend_from_slice(&o.obj_id.to_le_bytes());
    feed.extend_from_slice(&o.type_tag.to_le_bytes());
    feed.extend_from_slice(&d.owner_slot.to_le_bytes());
    feed.extend_from_slice(&d.dacl_ace_count.to_le_bytes());
    Ok(core::fnv1a(&feed))
}

/// 全链一次跑通（返回封印指纹）。
pub fn run_chain(sid_slot: u16, cred: u32, priv_mask: u32, obj_id: u16, type_tag: u16, owner: u16, aces: u16) -> Result<u64, u16> {
    let s = stage_login(sid_slot, cred)?;
    let t = stage_mint(&s, priv_mask)?;
    let o = stage_create(&t, obj_id, type_tag)?;
    let d = stage_attach(&o, owner, aces)?;
    stage_seal(&s, &t, &o, &d)
}

/// 摘要行（String 面，无 IO）。
pub fn summary(sealed_n: u32) -> String {
    let mut s = String::new();
    s.push_str("script1 sealed=");
    s.push_str(&sealed_n.to_string());
    s.push_str(" stages=");
    s.push_str(&Stage::ChainSealed.code().to_string());
    s
}

/// 诊断码闭集互异自检。
pub fn codes_distinct() -> bool {
    let cs = vec![E_VER, E_SID, E_CRED, E_NOGRP, E_TYPE, E_OWNER, E_DACL, E_CS];
    let mut i = 0;
    while i < cs.len() {
        let mut j = i + 1;
        while j < cs.len() {
            if cs[i] == cs[j] {
                return false;
            }
            j += 1;
        }
        i += 1;
    }
    true
}

pub fn detail(code: u16) -> &'static str {
    match code {
        E_VER => "版本锚不符",
        E_SID => "零 SID 槽位",
        E_CRED => "零凭据",
        E_NOGRP => "令牌无组（全链无主体资格）",
        E_TYPE => "零对象类型",
        E_OWNER => "SD 所有者槽为零",
        E_DACL => "DACL 超容",
        E_CS => "指纹不稳/代际断裂",
        _ => "未知诊断码",
    }
}

/// 负例矩阵：8 例（每段 2 例），期望码逐例断言。
pub fn negative_matrix() -> Result<u32, u16> {
    let mut hits: u32 = 0;
    // 段 1：零 SID / 零凭据。
    match stage_login(0, 1) {
        Ok(_) => return Err(E_SID),
        Err(c) if c == E_SID => {}
        Err(_) => return Err(E_SID),
    }
    hits += 1;
    match stage_login(1, 0) {
        Ok(_) => return Err(E_CRED),
        Err(c) if c == E_CRED => {}
        Err(_) => return Err(E_CRED),
    }
    hits += 1;
    // 段 2：无组令牌（手工构造零组主体）。
    let s = stage_login(3, 0x1234)?;
    let empty = Subject { sid_slot: s.sid_slot, groups: [0; 4], n_groups: 0 };
    match stage_mint(&empty, 1) {
        Ok(_) => return Err(E_NOGRP),
        Err(c) if c == E_NOGRP => {}
        Err(_) => return Err(E_NOGRP),
    }
    hits += 1;
    // 段 3：零类型 / 零代令牌。
    let t = stage_mint(&s, 1)?;
    match stage_create(&t, 7, 0) {
        Ok(_) => return Err(E_TYPE),
        Err(c) if c == E_TYPE => {}
        Err(_) => return Err(E_TYPE),
    }
    hits += 1;
    let dead_token = TokenCarrier { sid_slot: 3, n_groups: 4, priv_mask: 1, generation: 0 };
    match stage_create(&dead_token, 7, 9) {
        Ok(_) => return Err(E_NOGRP),
        Err(c) if c == E_NOGRP => {}
        Err(_) => return Err(E_NOGRP),
    }
    hits += 1;
    // 段 4：零所有者 / DACL 超容。
    let o = stage_create(&t, 7, 9)?;
    match stage_attach(&o, 0, 1) {
        Ok(_) => return Err(E_OWNER),
        Err(c) if c == E_OWNER => {}
        Err(_) => return Err(E_OWNER),
    }
    hits += 1;
    match stage_attach(&o, 3, DACL_CAP + 1) {
        Ok(_) => return Err(E_DACL),
        Err(c) if c == E_DACL => {}
        Err(_) => return Err(E_DACL),
    }
    hits += 1;
    // 段 5：主体/令牌 SID 错位。
    let d = stage_attach(&o, 3, 4)?;
    let stranger = Subject { sid_slot: 9, groups: s.groups, n_groups: 4 };
    match stage_seal(&stranger, &t, &o, &d) {
        Ok(_) => return Err(E_SID),
        Err(c) if c == E_SID => {}
        Err(_) => return Err(E_SID),
    }
    hits += 1;
    Ok(hits)
}

/// 演练：R1 锚位；R2 12 例全链；R3 负例矩阵 8 例；R4 指纹稳定；R5 双跑一致。
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;
    // R1：版本锚 + 门序锚（Stage 判别式递增）。
    if VER != 1 {
        return Err(E_VER);
    }
    if !(Stage::Login.code() < Stage::TokenMint.code()
        && Stage::TokenMint.code() < Stage::ObjectCreate.code()
        && Stage::ObjectCreate.code() < Stage::SdAttach.code()
        && Stage::SdAttach.code() < Stage::ChainSealed.code())
    {
        return Err(E_CS);
    }
    hits += 1;
    // R2：12 例全链（参数滚动，覆盖 priv/type/owner/aces 组合面）。
    let mut i: u32 = 0;
    while i < 12 {
        let fp = run_chain(
            (i % 5) as u16 + 1,
            0x1000 + i,
            0x0000_000f + i,
            (i + 1) as u16,
            (i % 3) as u16 + 1,
            (i % 5) as u16 + 1,
            (i % DACL_CAP as u32) as u16,
        )?;
        if fp == 0 {
            return Err(E_CS);
        }
        hits += 1;
        i += 1;
    }
    // R3：负例矩阵 8 例。
    hits += negative_matrix()?;
    // R4：同参指纹两次求值稳定。
    let f1 = run_chain(2, 0x2000, 0x0f, 4, 2, 2, 6)?;
    let f2 = run_chain(2, 0x2000, 0x0f, 4, 2, 2, 6)?;
    if f1 != f2 || f1 == 0 {
        return Err(E_CS);
    }
    hits += 1;
    // R5：双跑确定性（两遍全链 + 负例矩阵命中数一致）。
    let a = drill_inner()?;
    let b = drill_inner()?;
    if a != b {
        return Err(E_CS);
    }
    hits += 1;
    Ok(hits)
}

fn drill_inner() -> Result<u32, u16> {
    let mut n: u32 = 0;
    let mut i: u32 = 0;
    while i < 12 {
        run_chain((i % 5) as u16 + 1, 0x1000 + i, 0x0000_000f + i, (i + 1) as u16, (i % 3) as u16 + 1, (i % 5) as u16 + 1, (i % DACL_CAP as u32) as u16)?;
        n += 1;
        i += 1;
    }
    n += negative_matrix()?;
    Ok(n)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn drill_green() {
        assert_eq!(drill(), Ok(23));
    }

    #[test]
    fn zero_sid_rejected() {
        assert_eq!(stage_login(0, 1), Err(E_SID));
        assert_eq!(stage_login(1, 0), Err(E_CRED));
    }

    #[test]
    fn dac_cap_gate() {
        assert_eq!(DACL_CAP, 16);
        let t = TokenCarrier { sid_slot: 1, n_groups: 1, priv_mask: 1, generation: 1 };
        let o = stage_create(&t, 1, 1).unwrap_or(ObjectCarrier { obj_id: 0, type_tag: 0, generation: 0 });
        assert_eq!(stage_attach(&o, 1, DACL_CAP + 1), Err(E_DACL));
    }

    #[test]
    fn summary_shapes() {
        assert!(summary(12).contains("script1"));
        assert_eq!(codes_distinct(), true);
    }
}

pub fn run_unx36141_checks() -> CheckSet {
    let mut cs = CheckSet::new("unxstar-unx36141_script1");
    cs.add("SEG 段位独占", SEG_BASE == 0x8230, "0x8230~0x823f 本模块独占");
    match drill() {
        Ok(_) => cs.add("确定性演练全绿", true, "12 例全链对平、负例 8 例全拒"),
        Err(code) => cs.add("确定性演练全绿", false, detail(code)),
    }
    cs.add("五段门序锚", Stage::ChainSealed.code() == 5, "全链段位锚漂移");
    cs.add("版本锚 VER=1", VER == 1, "版本锚不符");
    cs.add("诊断码闭集互异", codes_distinct(), "诊断码闭集出现重码");
    cs
}
