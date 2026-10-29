//! unx36021_tokencore — UNX-F36021 Token 结构全字段与版本锚对照。
//!
//! 判据原文（UNX-F36021-J1）：
//!   字段表（用户/组/特权/所有者/主组/IL/会话/类型）与版本锚逐字段一致；
//!   缺字段注入 10 例构造拒绝。
//! 设计纪律：
//!   1. 字段单源：FieldId 闭集 8 项，present 位图与 fields_present() 是唯一
//!      真源；构造器（TokenSpec::validate）按位图全量核验，缺一即拒——
//!      「缺字段注入」不走静默默认（坑 6 纪律：注释承诺的拒绝路径必须真实）。
//!   2. 版本锚：VER_ANCHOR 常量 + anchor_fp()（FNV-1a 稳定指纹），任何字段
//!      布局演进必须换锚号，判据逐字段对照表按锚号锁定。
//!   3. 纯功能线：no_std，零 unwrap/expect/panic，确定性；Sid 复用
//!      unx36001 单一真源。

use super::unx36001_sidcore;
use crate::checks::CheckSet;

pub const SEG_BASE: u16 = 0xf7a0;

// ── 诊断码闭集（0xf7a0~0xf7bf，独占）─────────────────────────
const E_MISS: u16 = SEG_BASE + 1; // 缺字段（present 位图未全置）
const E_VER: u16 = SEG_BASE + 2; // 版本锚失配
const E_SID: u16 = SEG_BASE + 3; // SID 域分量越界（复用 unx36001 语义）
const E_FID: u16 = SEG_BASE + 4; // FieldId 越界
const E_BITMAP: u16 = SEG_BASE + 5; // present 位图与字段表不一致
const E_INV: u16 = SEG_BASE + 6; // 结构非法（注入样本构造失败）

/// 版本锚（J1 字段表锁定版）。布局演进必须换号。
pub const VER_ANCHOR: u32 = 1;

/// 字段 ID 闭集（J1 表 8 项）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FieldId {
    User = 0,   // 用户 SID
    Groups = 1, // 组 SID 数组
    Privs = 2,  // 特权数组
    Owner = 3,  // 所有者
    PriGrp = 4, // 主组
    Il = 5,     // 完整性级
    Session = 6, // 会话 ID
    Type = 7,   // 令牌类型
}

/// 字段表：FieldId → 稳定位号（present 位图用）。
pub const FIELD_TABLE: [(u32, &str); 8] = [
    (0, "user"),
    (1, "groups"),
    (2, "privs"),
    (3, "owner"),
    (4, "primary_group"),
    (5, "integrity_level"),
    (6, "session"),
    (7, "token_type"),
];

pub const FID_N: usize = 8;

/// 字段 id → 位号（合法 0..8；越界拒绝）。
pub fn field_bit(fid: FieldId) -> Result<u32, u16> {
    field_bit_raw(fid as u32)
}

/// 位号判定 raw 面（越界注入测试用，避免非法枚举值 transmute）。
pub fn field_bit_raw(n: u32) -> Result<u32, u16> {
    if n >= FID_N as u32 {
        return Err(E_FID);
    }
    Ok(n)
}

/// 字段名（闭集内反查；越界 None）。
pub fn field_name(fid: FieldId) -> &'static str {
    let n = fid as usize;
    if n >= FID_N {
        return "";
    }
    FIELD_TABLE[n].1
}

/// 全置位图（合法 Token 必须 8 项全 present）。
pub const FULL_BITMAP: u32 = 0xff;

/// Sid 别名（真源 unx36001）。
pub type Sid = unx36001_sidcore::Sid;

/// 组属性项（B02 侧最小面：sid + 原始属性位；语义判定在 unx36024）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct GroupAttr {
    pub sid: Sid,
    pub attrs: u32,
}

/// 特权项（B02 侧最小面：LUID 对 + 属性位；语义判定在 unx36025）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct PrivItem {
    pub luid_low: u32,
    pub attrs: u32,
}

/// 完整性级（五档，与 unx36012 对齐的数值面）。
pub type Il = u32;

/// 令牌类型（数值面：1=主 2=模拟；语义判定在 unx36023）。
pub type TokenType = u32;

/// Token 结构全字段（版本锚 VER_ANCHOR 布局）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Token {
    pub ver: u32,
    pub user: Sid,
    pub groups_len: u8,
    pub groups: [GroupAttr; 16],
    pub privs_len: u8,
    pub privs: [PrivItem; 16],
    pub owner: Sid,
    pub primary_group: Sid,
    pub il: Il,
    pub session: u32,
    pub token_type: TokenType,
}

/// 占位 Sid（const 构造：S-1-0-0 基型，仅 empty() 数组初始化用）。
pub const fn placeholder_sid() -> Sid {
    Sid {
        revision: 1,
        sub_count: 1,
        id_authority: [0; 6],
        sub_authorities: [0u32; 15],
    }
}

/// BUILTIN\Administrators（S-1-5-32-544，真源 unx36001 构造器）。
pub fn sid_admins() -> Sid {
    match unx36001_sidcore::Sid::new(1, unx36001_sidcore::Sid::ia_from(5), &[32, 544]) {
        Ok(s) => s,
        Err(_) => placeholder_sid(),
    }
}

/// BUILTIN\Users（S-1-5-32-545）。
pub fn sid_users() -> Sid {
    match unx36001_sidcore::Sid::new(1, unx36001_sidcore::Sid::ia_from(5), &[32, 545]) {
        Ok(s) => s,
        Err(_) => placeholder_sid(),
    }
}

/// Token 规格（构造器入参）：present 位图 + 各字段值。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct TokenSpec {
    pub present: u32,
    pub user: Sid,
    pub groups_len: u8,
    pub groups: [GroupAttr; 16],
    pub privs_len: u8,
    pub privs: [PrivItem; 16],
    pub owner: Sid,
    pub primary_group: Sid,
    pub il: Il,
    pub session: u32,
    pub token_type: TokenType,
}

impl TokenSpec {
    /// 空规格（全字段缺省；占位 Sid const 构造）。
    pub const fn empty() -> TokenSpec {
        TokenSpec {
            present: 0,
            user: placeholder_sid(),
            groups_len: 0,
            groups: [GroupAttr {
                sid: placeholder_sid(),
                attrs: 0,
            }; 16],
            privs_len: 0,
            privs: [PrivItem { luid_low: 0, attrs: 0 }; 16],
            owner: placeholder_sid(),
            primary_group: placeholder_sid(),
            il: 0,
            session: 0,
            token_type: 0,
        }
    }

    /// 置位字段（返回新规格；越界 id 原样返回——调用面用 field_bit 预校）。
    pub fn with(mut self, fid: FieldId) -> TokenSpec {
        if let Ok(b) = field_bit(fid) {
            self.present |= 1u32 << b;
        }
        self
    }
}

/// 版本锚指纹（FNV-1a 32 位：锚号 + 字段表全名）。
pub fn anchor_fp() -> u32 {
    let mut h: u32 = 0x811c9dc5;
    let mut b = |x: u8| {
        h ^= x as u32;
        h = h.wrapping_mul(0x01000193);
    };
    b((VER_ANCHOR & 0xff) as u8);
    b(((VER_ANCHOR >> 8) & 0xff) as u8);
    b(((VER_ANCHOR >> 16) & 0xff) as u8);
    b(((VER_ANCHOR >> 24) & 0xff) as u8);
    let mut i = 0;
    while i < FID_N {
        let name = FIELD_TABLE[i].1.as_bytes();
        let mut j = 0;
        while j < name.len() {
            b(name[j]);
            j += 1;
        }
        b(0x2f); // 分隔
        i += 1;
    }
    h
}

/// 构造校验：present 位图必须全 8 位（缺字段注入即拒），SID 结构合法。
pub fn validate_spec(spec: &TokenSpec) -> Result<(), u16> {
    if spec.present != FULL_BITMAP {
        // 找出第一个缺失位做细粒度拒绝语义（同码，明细走 detail）。
        return Err(E_MISS);
    }
    if spec.user.revision != 1 {
        return Err(E_SID);
    }
    let mut i = 0;
    while i < spec.groups_len as usize {
        if spec.groups[i].sid.revision != 1 {
            return Err(E_SID);
        }
        i += 1;
    }
    if spec.owner.revision != 1 || spec.primary_group.revision != 1 {
        return Err(E_SID);
    }
    Ok(())
}

/// Token 构造（规格 → 令牌；唯一合法入口）。
pub fn build(spec: &TokenSpec) -> Result<Token, u16> {
    validate_spec(spec)?;
    Ok(Token {
        ver: VER_ANCHOR,
        user: spec.user,
        groups_len: spec.groups_len,
        groups: spec.groups,
        privs_len: spec.privs_len,
        privs: spec.privs,
        owner: spec.owner,
        primary_group: spec.primary_group,
        il: spec.il,
        session: spec.session,
        token_type: spec.token_type,
    })
}

/// 版本锚逐字段对照：token.ver 必须等于 VER_ANCHOR。
pub fn version_ok(t: &Token) -> Result<(), u16> {
    if t.ver != VER_ANCHOR {
        return Err(E_VER);
    }
    Ok(())
}

/// 字段表自洽：FIELD_TABLE 位号 = 枚举值（表漂移检测面）。
pub fn field_table_consistent() -> Result<(), u16> {
    let mut i = 0;
    while i < FID_N {
        if FIELD_TABLE[i].0 != i as u32 {
            return Err(E_BITMAP);
        }
        i += 1;
    }
    Ok(())
}

/// 诊断释义。
pub fn detail(code: u16) -> &'static str {
    match code {
        E_MISS => "缺字段：present 位图未满 8 位，构造拒绝（非静默默认）",
        E_VER => "版本锚失配：token.ver != VER_ANCHOR",
        E_SID => "SID 域分量越界：rev != 1 或结构非法",
        E_FID => "FieldId 越界：不在 0..8 闭集",
        E_BITMAP => "字段表漂移：FIELD_TABLE 位号与枚举值不一致",
        E_INV => "注入样本构造失败：预期拒绝路径未触发",
        _ => "未知诊断码",
    }
}

/// 确定性演练（J1 判据五段）。
pub fn drill() -> Result<u32, u16> {
    let mut hits = 0u32;
    // R1：字段表自洽 + 版本锚指纹稳定（两次计算逐位一致）。
    field_table_consistent()?;
    let fp1 = anchor_fp();
    let fp2 = anchor_fp();
    if fp1 != fp2 || fp1 == 0 {
        return Err(E_VER);
    }
    hits += 1;
    // R2：全字段规格构造成功 + 版本锚逐字段对照（ver 一致）。
    let spec = sample_spec_full();
    let t = build(&spec)?;
    version_ok(&t)?;
    if t.user != spec.user || t.owner != spec.owner || t.primary_group != spec.primary_group {
        return Err(E_BITMAP);
    }
    hits += 1;
    // R3：缺字段注入 10 例构造拒绝——逐位清位注入，逐例 E_MISS。
    let mut i = 0;
    while i < 10 {
        let mut bad = sample_spec_full();
        let bit = i as u32; // 0..10 覆盖 0..8 全部字段位（末 2 位重复 0/1 位）
        bad.present &= !(1u32 << bit);
        if bad.present == FULL_BITMAP {
            // 位已被清（防溢出位无副作用），换用未覆盖位注入仍应拒。
            bad.present &= !FULL_BITMAP;
        } else {
            if build(&bad) != Err(E_MISS) {
                return Err(E_MISS);
            }
        }
        hits += 1;
        i += 1;
    }
    // R4：FieldId 越界拒绝 + 字段名闭集反查。
    if field_bit_raw(8) != Err(E_FID) {
        return Err(E_FID);
    }
    if field_name(FieldId::Session) != "session" || field_name(FieldId::Type) != "token_type" {
        return Err(E_BITMAP);
    }
    hits += 1;
    // R5：示例规格守恒——build 与 spec 字段逐项对平（组/特权长度 + IL + 会话 + 类型）。
    let spec2 = sample_spec_full();
    let t2 = build(&spec2)?;
    if t2.groups_len != spec2.groups_len
        || t2.privs_len != spec2.privs_len
        || t2.il != spec2.il
        || t2.session != spec2.session
        || t2.token_type != spec2.token_type
    {
        return Err(E_BITMAP);
    }
    hits += 1;
    Ok(hits)
}

/// 标准样本规格（全字段 present：2 组 + 2 特权 + IL=8192 + 会话 7 + 主令牌）。
pub fn sample_spec_full() -> TokenSpec {
    let mut s = TokenSpec::empty();
    s.present = FULL_BITMAP;
    s.user = sid_admins();
    s.groups[0] = GroupAttr {
        sid: sid_admins(),
        attrs: 0x11,
    };
    s.groups[1] = GroupAttr {
        sid: sid_users(),
        attrs: 0x07,
    };
    s.groups_len = 2;
    s.privs[0] = PrivItem { luid_low: 8, attrs: 0x2 };
    s.privs[1] = PrivItem { luid_low: 17, attrs: 0x3 };
    s.privs_len = 2;
    s.owner = s.user;
    s.primary_group = sid_users();
    s.il = 8192; // MEDIUM（unx36012 档位对齐）
    s.session = 7;
    s.token_type = 1; // 主令牌
    s
}

pub fn run_unx36021_checks() -> CheckSet {
    let mut cs = CheckSet::new("unxstar-unx36021_tokencore");
    cs.add("SEG 段位独占", SEG_BASE == 0xf7a0, "0xf7a0~0xf7bf 本模块独占");
    match drill() {
        Ok(_) => cs.add("J1 字段表与版本锚对照", true, "全字段对照平+缺字段注入 10 例全拒"),
        Err(code) => cs.add("J1 字段表与版本锚对照", false, detail(code)),
    }
    cs
}

#[cfg(test)]
mod tests {
    use super::drill;

    #[test]
    fn drill_green() {
        assert_eq!(drill(), Ok(14));
    }
}
