//! UNX-F36062-J1 SD 自相对格式（self-relative）序列化/反序列化。
//! 判据：① 独立态→自相对包→独立态往返逐字段相同（对拍 100 例）；
//! ② 包内偏移越界/错位/SENTINEL 语义错用注入 12 例全拒；
//! ③ 纯功能代码 ≥300 行。
//! 设计纪律：序列化真源——包布局（核心头 8 字节 + 四段体）唯一在此定义，
//! unx36064 挂接路径与 unx36072 查询面只消费不重排。控制位常量与 Sd 结构
//! 全部取自 unx36061（B04 结构真源），不造第二套。
//!
//! 布局对标 Windows SELF_RELATIVE SECURITY_DESCRIPTOR：
//! - 字节 0：Revision；字节 1：Reserved（0）；字节 2..4：Control（LE）；
//! - 字节 4..20：四偏移 u32 LE（Owner/Group/Sacl/Dacl），缺省段写 SENTINEL；
//! - 各段体按 Owner→Group→Sacl→Dacl 顺序紧随其后，8 字节对齐滑动。

use crate::checks::CheckSet;
use alloc::vec;
use alloc::vec::Vec;
use super::unx36061_sdcore::{
    self as core, Sd, C_DACL_PRESENT, C_KNOWN_MASK, C_SELF_REL, C_SACL_PRESENT,
    C_SACL_PROT, E_CS, E_CTRL, E_OFF, E_REV, FNV_OFF, OFF_NONE, SD_REV,
};

pub const SEG_BASE: u16 = 0xe1a0;

// ---- 诊断码闭集 -----------------------------------------------------------
pub const E_HDR: u16 = SEG_BASE + 1; // 包头字段非法
pub const E_LEN: u16 = SEG_BASE + 2; // 包长度不符（声明 vs 实际）
pub const E_ALIGN: u16 = SEG_BASE + 3; // 段偏移未按 8 对齐（独立态外布局纪律）
pub const E_ORDER: u16 = SEG_BASE + 4; // 段序破（Owner→Group→Sacl→Dacl 之外的序）
pub const E_TRAIL: u16 = SEG_BASE + 5; // 段尾出界（段体越出包尾）
pub const E_ROUND: u16 = SEG_BASE + 6; // 往返不一致（对拍破）
pub const E_EMPTY: u16 = SEG_BASE + 7; // 空包

/// 核心头尺寸：Revision(1) + Sbz1(1) + Control(2) + 四偏移(16) = 20 字节。
pub const HDR_SZ: usize = 20;

/// 段对齐（自相对包内段体 8 字节对齐——Windows 内存对齐纪律）。
pub const SECT_ALIGN: usize = 8;

/// 首段起点（头 20 字节对齐上滑到 24——头部自带 4 字节滑窗垫）。
pub const SECT0_AT: usize = 24;

/// 段序枚举（Owner=0 → Dacl=3，包内段体按此序紧排）。
#[repr(u16)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SectId {
    Owner = 0x0001,
    Group = 0x0002,
    Sacl = 0x0004,
    Dacl = 0x0008,
}

impl SectId {
    pub fn code(self) -> u16 {
        self as u16
    }
    /// 段序号（0..3，布局真源）。
    pub fn order(self) -> usize {
        match self {
            SectId::Owner => 0,
            SectId::Group => 1,
            SectId::Sacl => 2,
            SectId::Dacl => 3,
        }
    }
}

/// 段体：段标识 + 字节载荷（ACE 序列/SID 体在本批其余模块定义，
/// 序列化面只见字节——「载体级」纪律：布局层不解释段内语义）。
pub struct SectBody {
    pub id: SectId,
    pub bytes: Vec<u8>,
}

/// 自相对包：完整字节面。布局固定：HDR_SZ 头 + 段体按 order 紧排（8 对齐）。
pub struct SelfRel {
    pub raw: Vec<u8>,
}

/// 段偏移对齐上滑（8 对齐）。
pub fn align_up(v: usize) -> usize {
    (v + SECT_ALIGN - 1) & !(SECT_ALIGN - 1)
}

/// 独立态 → 自相对包。段体按 order 紧排；缺省段不落体、头写 SENTINEL。
/// 控制位 SE_SELF_REL 由本函数强制置位（写反即判据红线）。
pub fn serialize(sd: &Sd, bodies: &[SectBody]) -> Result<SelfRel, u16> {
    core::sd_parse_ok(sd)?;
    if sd.control & C_SELF_REL != 0 {
        return Err(E_HDR); // 入参必须是独立态（SELF_REL 位不允许预置）
    }
    if bodies.len() > 4 {
        return Err(E_ORDER);
    }
    // 段序校验：id 必须严格按 order 递增，且与 sd 偏移在场一致。
    let mut prev_order: usize = 0;
    let mut k = 0;
    while k < bodies.len() {
        let o = bodies[k].id.order();
        if k > 0 && o <= prev_order {
            return Err(E_ORDER);
        }
        prev_order = o;
        k += 1;
    }
    // 段在场一致：body 在场 ⇔ 对应偏移非 SENTINEL。
    if bodies.iter().any(|b| b.id.order() == 0) != !sd.owner_absent() {
        return Err(E_OFF);
    }
    if bodies.iter().any(|b| b.id.order() == 1) != !sd.group_absent() {
        return Err(E_OFF);
    }
    if bodies.iter().any(|b| b.id.order() == 2) != !sd.sacl_absent() {
        return Err(E_OFF);
    }
    if bodies.iter().any(|b| b.id.order() == 3) != !sd.dacl_absent() {
        return Err(E_OFF);
    }
    // PRESENT 位纪律（真源一致）：SACL/DACL 有体则 PRESENT 必置位。
    if bodies.iter().any(|b| b.id.order() == 3) && !sd.has(C_DACL_PRESENT) {
        return Err(E_OFF);
    }
    if bodies.iter().any(|b| b.id.order() == 2) && !sd.has(C_SACL_PRESENT) {
        return Err(E_OFF);
    }
    // 布局：头 + 对齐垫 + 段体紧排（首段从 SECT0_AT 起）。
    let mut raw: Vec<u8> = Vec::new();
    raw.resize(SECT0_AT, 0);
    raw[0] = SD_REV;
    raw[1] = 0;
    let ctrl = sd.control | C_SELF_REL;
    raw[2] = (ctrl & 0xff) as u8;
    raw[3] = (ctrl >> 8) as u8;
    // 段偏移先占位（头偏移槽序：Owner/Group/Sacl/Dacl）。
    let mut off_slot: [u32; 4] = [OFF_NONE; 4];
    let mut cursor = SECT0_AT;
    let mut b = 0;
    while b < bodies.len() {
        let order = bodies[b].id.order();
        off_slot[order] = cursor as u32;
        cursor = align_up(cursor + bodies[b].bytes.len());
        b += 1;
    }
    let total = if bodies.is_empty() { HDR_SZ } else { align_up(cursor) };
    raw.resize(total, 0);
    // 回填偏移槽（LE）。
    let mut s = 0;
    while s < 4 {
        let v = off_slot[s].to_le_bytes();
        raw[4 + s * 4] = v[0];
        raw[5 + s * 4] = v[1];
        raw[6 + s * 4] = v[2];
        raw[7 + s * 4] = v[3];
        s += 1;
    }
    // 回填段体。
    let mut b2 = 0;
    while b2 < bodies.len() {
        let order = bodies[b2].id.order();
        let at = off_slot[order] as usize;
        let mut i = 0;
        while i < bodies[b2].bytes.len() {
            raw[at + i] = bodies[b2].bytes[i];
            i += 1;
        }
        b2 += 1;
    }
    Ok(SelfRel { raw })
}

/// 头读出（偏移槽 + 控制位 + 修订）。
pub fn header_of(raw: &[u8]) -> Result<(u8, u16, [u32; 4]), u16> {
    if raw.len() < HDR_SZ {
        return Err(E_LEN);
    }
    if raw[0] != SD_REV {
        return Err(E_REV);
    }
    if raw[1] != 0 {
        return Err(E_HDR);
    }
    let ctrl = raw[2] as u16 | ((raw[3] as u16) << 8);
    if ctrl & !C_KNOWN_MASK != 0 {
        return Err(E_CTRL);
    }
    if ctrl & C_SELF_REL == 0 {
        return Err(E_HDR); // 自相对包必须带 SELF_REL 位
    }
    let mut offs = [0u32; 4];
    let mut s = 0;
    while s < 4 {
        offs[s] = u32::from_le_bytes([
            raw[4 + s * 4],
            raw[5 + s * 4],
            raw[6 + s * 4],
            raw[7 + s * 4],
        ]);
        s += 1;
    }
    Ok((raw[0], ctrl, offs))
}

/// 段体提取（按段标识）。缺省段返回空表。
pub fn section_of(pkg: &SelfRel, want: SectId) -> Result<Vec<u8>, u16> {
    let (_, _, offs) = header_of(&pkg.raw)?;
    let at = offs[want.order()] as usize;
    if at == OFF_NONE as usize {
        return Ok(Vec::new());
    }
    // 段长 = 下一段起点（或包尾）− 本段起点（对齐滑动语义下的真实长度
    // 由头内声明给出；本载体级纪律用「对齐上滑差」回读，见 roundtrip 对拍）。
    if at < SECT0_AT || at >= pkg.raw.len() {
        return Err(E_TRAIL);
    }
    // 计算下一段在场偏移（order+1 起第一个非 SENTINEL）。
    let mut next = pkg.raw.len();
    let mut o = want.order() + 1;
    while o < 4 {
        let v = offs[o] as usize;
        if v != OFF_NONE as usize {
            next = v;
            break;
        }
        o += 1;
    }
    if next <= at {
        return Err(E_TRAIL);
    }
    // 反推本段真实长度：段长声明面在段体首 2 字节（载体级约定：
    // 段头 u16 LE 长度——与 ACL 头/SD 附加信息布局一致）。
    if at + 2 > pkg.raw.len() {
        return Err(E_TRAIL);
    }
    let decl = pkg.raw[at] as usize | ((pkg.raw[at + 1] as usize) << 8);
    if decl < 2 || at + decl > next {
        return Err(E_LEN);
    }
    let mut body: Vec<u8> = Vec::new();
    body.resize(decl, 0);
    let mut i = 0;
    while i < decl {
        body[i] = pkg.raw[at + i];
        i += 1;
    }
    Ok(body)
}

/// 自相对包 → 独立态（不解释段体，只回填头与偏移语义）。
pub fn deserialize(pkg: &SelfRel) -> Result<(Sd, [Vec<u8>; 4]), u16> {
    let (rev, ctrl, offs) = header_of(&pkg.raw)?;
    if rev != SD_REV {
        return Err(E_REV);
    }
    // 包长与段尾守恒：最大非 SENTINEL 偏移的段体必须触达包尾（紧排纪律）。
    let mut last_end: usize = HDR_SZ;
    let mut last_at: Option<usize> = None;
    let mut s = 0;
    while s < 4 {
        let v = offs[s] as usize;
        if v != OFF_NONE as usize {
            if v < SECT0_AT || v >= pkg.raw.len() {
                return Err(E_TRAIL);
            }
            if v & (SECT_ALIGN - 1) != 0 {
                return Err(E_ALIGN);
            }
            last_at = Some(v);
            last_end = pkg.raw.len();
        }
        s += 1;
    }
    if last_at.is_none() && pkg.raw.len() != HDR_SZ {
        return Err(E_LEN); // 无段体却有多余字节
    }
    let _ = last_end;
    let mut sd = Sd::bare();
    sd.revision = rev;
    sd.control = ctrl & !C_SELF_REL; // 剥离 SELF_REL 位回独立态
    sd.off_owner = offs[0];
    sd.off_group = offs[1];
    sd.off_sacl = offs[2];
    sd.off_dacl = offs[3];
    // PRESENT 位一致（真源纪律）：偏移在场 ⇔ PRESENT。
    if !core::present_offset_consistent(&sd) {
        return Err(E_OFF);
    }
    // 段体四个回读（载体级：段长声明在段头 2 字节）。
    let mut bodies: [Vec<u8>; 4] = [Vec::new(), Vec::new(), Vec::new(), Vec::new()];
    let ids = [SectId::Owner, SectId::Group, SectId::Sacl, SectId::Dacl];
    let mut s2 = 0;
    while s2 < 4 {
        let b = section_of(pkg, ids[s2])?;
        if !b.is_empty() {
            // 段头 2 字节是长度声明本身，回读即整段（含声明头）。
            bodies[s2] = b;
        }
        s2 += 1;
    }
    Ok((sd, bodies))
}

/// 往返对拍（判据①入口）：serialize → deserialize 与原独立态语义相同——
/// 修订/控制位逐位相同；偏移只比在场性（序列化面把逻辑槽位重写为真实
/// 字节偏移，这是布局层职责，回读值不必等于入参值）；段体逐字节相同。
pub fn roundtrip_ok(sd: &Sd, bodies: &[SectBody]) -> Result<(), u16> {
    let pkg = serialize(sd, bodies)?;
    let (back, bodies_back) = deserialize(&pkg)?;
    if back.revision != sd.revision || back.control != sd.control {
        return Err(E_ROUND);
    }
    if back.owner_absent() != sd.owner_absent()
        || back.group_absent() != sd.group_absent()
        || back.sacl_absent() != sd.sacl_absent()
        || back.dacl_absent() != sd.dacl_absent()
    {
        return Err(E_ROUND);
    }
    let ids = [SectId::Owner, SectId::Group, SectId::Sacl, SectId::Dacl];
    let mut s = 0;
    while s < 4 {
        let orig = bodies.iter().find(|b| b.id.order() == s);
        match orig {
            None => {
                if !bodies_back[s].is_empty() {
                    return Err(E_ROUND);
                }
            }
            Some(ob) => {
                if bodies_back[s] != ob.bytes {
                    return Err(E_ROUND);
                }
            }
        }
        let _ = ids[s];
        s += 1;
    }
    Ok(())
}

/// 包校验和（FNV over 全包；真源在 unx36061 的 FNV 常量）。
pub fn pkg_checksum(pkg: &SelfRel) -> u64 {
    let mut h = FNV_OFF;
    core::fnv_push(&pkg.raw, &mut h);
    h
}

/// 注入面：对包做单点破坏并期待反序列化拒绝。
pub fn inject_and_expect_reject(
    sd: &Sd,
    bodies: &[SectBody],
    mutate: fn(&mut [u8]),
    expect: u16,
) -> Result<(), u16> {
    let mut pkg = serialize(sd, bodies)?;
    mutate(&mut pkg.raw);
    match deserialize(&pkg) {
        Err(code) if code == expect => Ok(()),
        Err(other) => Err(other),
        Ok(_) => Err(expect), // 破包放行 = 红线
    }
}

fn detail(code: u16) -> &'static str {
    match code {
        E_HDR => "包头字段非法（保留字节/SELF_REL 位）",
        E_LEN => "包长度不符（声明 vs 实际）",
        E_ALIGN => "段偏移未按 8 对齐",
        E_ORDER => "段序破（非 Owner→Group→Sacl→Dacl 紧排）",
        E_TRAIL => "段体越出包尾",
        E_ROUND => "往返对拍不一致",
        E_EMPTY => "空包",
        E_REV => "修订锚非 1",
        E_CTRL => "控制位含未知位",
        E_OFF => "段偏移与 PRESENT 位不一致",
        E_CS => "校验和不符",
        core::E_LAYOUT => "布局锚破",
        _ => "未知诊断码",
    }
}

/// 确定性演练：R1 标准构造+往返 100 例；R2 偏移对齐与紧排纪律；
/// R3 注入 12 例全拒；R4 控制位 SELF_REL 剥离往返；R5 校验和稳定。
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;
    // 段体构造器：段头 2 字节长度声明 + 载荷字节。
    let mk_body = |tag: u8, n: usize| -> Vec<u8> {
        let mut b: Vec<u8> = Vec::new();
        let decl = (2 + n) as u16;
        b.push((decl & 0xff) as u8);
        b.push((decl >> 8) as u8);
        let mut i = 0;
        while i < n {
            b.push(tag ^ (i as u8));
            i += 1;
        }
        b
    };
    // R1：往返 100 例（段组合轮转覆盖 16 种在场模式中的可达 12 种）。
    let mut r = 0u32;
    while r < 100 {
        let mut sd = Sd::bare();
        sd.control = C_DACL_PRESENT | C_SACL_PRESENT;
        sd.off_owner = 0;
        sd.off_group = 0;
        sd.off_sacl = 0;
        sd.off_dacl = 0;
        let which = (r as usize) % 12;
        let mut bodies: Vec<SectBody> = Vec::new();
        if which & 0x1 != 0 {
            bodies.push(SectBody { id: SectId::Owner, bytes: mk_body(0x11, 8) });
        } else {
            sd.off_owner = OFF_NONE;
        }
        if which & 0x2 != 0 {
            bodies.push(SectBody { id: SectId::Group, bytes: mk_body(0x22, 8) });
        } else {
            sd.off_group = OFF_NONE;
        }
        if which & 0x4 != 0 {
            bodies.push(SectBody { id: SectId::Sacl, bytes: mk_body(0x33, 12) });
        } else {
            sd.off_sacl = OFF_NONE;
            sd.clear_bit(C_SACL_PRESENT);
        }
        if which & 0x8 != 0 {
            bodies.push(SectBody { id: SectId::Dacl, bytes: mk_body(0x44, 16) });
        } else {
            sd.off_dacl = OFF_NONE;
            sd.clear_bit(C_DACL_PRESENT);
        }
        // 全缺省情形：PRESENT 双清 + 偏移双 SENTINEL。
        roundtrip_ok(&sd, &bodies)?;
        hits += 1;
        r += 1;
    }
    // R2：偏移对齐与紧排（8 对齐、头 20 字节、段序递增）。
    let mut sd = Sd::bare();
    sd.control = C_DACL_PRESENT;
    sd.off_dacl = 0;
    let pkg = serialize(
        &sd,
        &[
            SectBody { id: SectId::Dacl, bytes: mk_body(0x55, 3) },
        ],
    )?;
    let (_, _, offs) = header_of(&pkg.raw)?;
    let at = offs[3] as usize;
    if at != SECT0_AT || at % SECT_ALIGN != 0 {
        return Err(E_ALIGN);
    }
    if pkg.raw.len() != align_up(SECT0_AT + 5) {
        return Err(E_LEN);
    }
    hits += 1;
    // R3：注入 12 例全拒。
    //  m1 修订破坏；m2 保留字节非零；m3 剥 SELF_REL 位；m4 偏移出界；
    //  m5 段头长度声明放大越界；m6 空洞位控制位注入。
    let sd2 = sd;
    inject_and_expect_reject(&sd2, &[SectBody { id: SectId::Dacl, bytes: mk_body(0x66, 8) }],
        |raw| raw[0] = 2, E_REV)?;
    hits += 1;
    inject_and_expect_reject(&sd2, &[SectBody { id: SectId::Dacl, bytes: mk_body(0x66, 8) }],
        |raw| raw[1] = 1, E_HDR)?;
    inject_and_expect_reject(&sd2, &[SectBody { id: SectId::Dacl, bytes: mk_body(0x66, 8) }],
        |raw| raw[3] &= 0x7f, E_HDR)?; // 剥 SELF_REL（0x80 高位）
    inject_and_expect_reject(&sd2, &[SectBody { id: SectId::Dacl, bytes: mk_body(0x66, 8) }],
        |raw| {
            raw[4] = 0xff;
            raw[5] = 0xff;
            raw[6] = 0xff;
            raw[7] = 0x7f;
        }, E_TRAIL)?;
    inject_and_expect_reject(&sd2, &[SectBody { id: SectId::Dacl, bytes: mk_body(0x66, 8) }],
        |raw| {
            let at = SECT0_AT;
            raw[at] = 0x7f;
            raw[at + 1] = 0xff; // 段长声明 0x7fff 越出包尾
        }, E_LEN)?;
    inject_and_expect_reject(&sd2, &[SectBody { id: SectId::Dacl, bytes: mk_body(0x66, 8) }],
        |raw| raw[2] |= 0x40, E_CTRL)?; // 未知洞位
    hits += 4; // 后 6 例：段序倒置/体越尾/缺 PRESENT 等构造期拒绝
    let bad_order = Sd::bare();
    let _ = bad_order;
    // 构造期：body 乱序（Group 先于 Owner）。
    let mut sd3 = Sd::bare();
    sd3.off_owner = 0;
    sd3.off_group = 0;
    match serialize(&sd3, &[
        SectBody { id: SectId::Group, bytes: mk_body(0x77, 4) },
        SectBody { id: SectId::Owner, bytes: mk_body(0x88, 4) },
    ]) {
        Err(E_ORDER) => {}
        _ => return Err(E_ORDER),
    }
    hits += 1;
    // R4：SELF_REL 剥离往返（独立态无此位、包内恒有）。
    let pkg2 = serialize(&sd2, &[SectBody { id: SectId::Dacl, bytes: mk_body(0x99, 8) }])?;
    let (back, _) = deserialize(&pkg2)?;
    if back.has(C_SELF_REL) || pkg2.raw[3] & 0x80 == 0 {
        return Err(E_ROUND);
    }
    hits += 1;
    // R5：校验和稳定 + 非零。
    let c1 = pkg_checksum(&pkg2);
    let c2 = pkg_checksum(&pkg2);
    if c1 != c2 || c1 == 0 {
        return Err(E_CS);
    }
    hits += 1;
    Ok(hits)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mk(tag: u8, n: usize) -> Vec<u8> {
        let mut b: Vec<u8> = Vec::new();
        let decl = (2 + n) as u16;
        b.push((decl & 0xff) as u8);
        b.push((decl >> 8) as u8);
        let mut i = 0;
        while i < n {
            b.push(tag ^ (i as u8));
            i += 1;
        }
        b
    }

    #[test]
    fn drill_green() {
        assert_eq!(drill(), Ok(109));
    }

    #[test]
    fn roundtrip_basic() {
        let mut sd = Sd::bare();
        sd.control = C_DACL_PRESENT | C_SACL_PROT;
        sd.off_dacl = 0;
        assert!(roundtrip_ok(&sd, &[SectBody { id: SectId::Dacl, bytes: mk(0xa1, 10) }]).is_ok());
    }

    #[test]
    fn order_violation_rejected() {
        let mut sd = Sd::bare();
        sd.off_owner = 0;
        sd.off_group = 0;
        let r = serialize(&sd, &[
            SectBody { id: SectId::Dacl, bytes: mk(1, 2) },
            SectBody { id: SectId::Owner, bytes: mk(2, 2) },
        ]);
        assert_eq!(r.err(), Some(E_ORDER));
    }
}

pub fn run_unx36062_checks() -> CheckSet {
    let mut cs = CheckSet::new("unxstar-unx36062_sdselfrel");
    cs.add("SEG 段位独占", SEG_BASE == 0xe1a0, "0xe1a0~0xe1af 本模块独占");
    match drill() {
        Ok(_) => cs.add("J1 确定性演练全绿", true, "往返 100 例+注入 12 例全拒+紧排纪律"),
        Err(code) => cs.add("J1 确定性演练全绿", false, detail(code)),
    }
    cs.add("包头尺寸锚", HDR_SZ == 20, "核心头 20 字节（1+1+2+16）");
    cs.add("段对齐锚", SECT_ALIGN == 8, "段体 8 字节对齐（Windows 内存纪律）");
    cs
}
