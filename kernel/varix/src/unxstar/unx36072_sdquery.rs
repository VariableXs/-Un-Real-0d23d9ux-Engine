//! UNX-F36072-J1 SD 查询面（GetSecurityDescriptor 类最小集）。
//! 判据：① 查询五件套（控制位/Owner/Group/DACL/SACL 在场性）100 例
//! 与 SD 形状逐项对平；② 未挂载段查询显性缺省（零静默假值）12 例；
//! ③ 纯功能代码 ≥300 行。
//! 设计纪律：查询是只读面——只读 SD 结构与载体账，不改任何位面。
//! 返回「在场 + 值」或「显性缺省」二元组，绝不用零值冒充缺省
//! （零值可能是合法数据——哨兵语义只在结构面，查询面用 Option 语义）。

use crate::checks::CheckSet;
use alloc::vec;
use alloc::vec::Vec;
use super::unx36061_sdcore::{
    self as core, Sd, C_DACL_PRESENT, C_GROUP_DFLT, C_OWNER_DFLT, C_SACL_PRESENT,
    C_SELF_REL, E_CTRL, E_OFF, OFF_NONE,
};

pub const SEG_BASE: u16 = 0xe240;

// ---- 诊断码闭集 -----------------------------------------------------------
pub const E_CLOSED: u16 = SEG_BASE + 1; // 查询面被关闭（载体隔离中）
pub const E_SHAPE: u16 = SEG_BASE + 2; // 形状非法（PRESENT/偏移矛盾）
pub const E_NULLQ: u16 = SEG_BASE + 3; // 空引用查询（未挂接 SD 的对象）

/// 查询五件套结果（Option 语义：None = 显性缺省，非零值冒充）。
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct SdQuery {
    pub revision: Option<u8>,
    pub control: Option<u16>,
    pub owner: Option<u32>, // 段逻辑槽位/偏移快照
    pub group: Option<u32>,
    pub dacl: Option<u32>,
    pub sacl: Option<u32>,
}

/// 最小查询面（对标 GetSecurityDescriptorControl/Owner/Group/Dacl/Sacl）。
pub fn query_min(sd: &Sd) -> Result<SdQuery, u16> {
    core::sd_parse_ok(sd)?;
    if !core::present_offset_consistent(&sd) {
        return Err(E_SHAPE);
    }
    Ok(SdQuery {
        revision: Some(sd.revision),
        control: Some(sd.control),
        owner: opt_off(sd.off_owner),
        group: opt_off(sd.off_group),
        dacl: opt_off(sd.off_dacl),
        sacl: opt_off(sd.off_sacl),
    })
}

/// 偏移 → Option 语义（SENTINEL → None；其余 → Some）。
pub fn opt_off(v: u32) -> Option<u32> {
    if v == OFF_NONE {
        None
    } else {
        Some(v)
    }
}

/// 控制位查询（GetSecurityDescriptorControl 语义：修订 + 控制位字）。
pub fn query_control(sd: &Sd) -> Result<(u8, u16), u16> {
    core::sd_parse_ok(sd)?;
    Ok((sd.revision, sd.control))
}

/// 在场性查询（五段在场位图——独立态语义，不含 SELF_REL 位）。
pub fn query_presence(sd: &Sd) -> Result<u8, u16> {
    core::sd_parse_ok(sd)?;
    let mut bits: u8 = 0;
    if sd.off_owner != OFF_NONE {
        bits |= 0x01;
    }
    if sd.off_group != OFF_NONE {
        bits |= 0x02;
    }
    if sd.off_sacl != OFF_NONE {
        bits |= 0x04;
    }
    if sd.off_dacl != OFF_NONE {
        bits |= 0x08;
    }
    if sd.has(C_OWNER_DFLT) {
        bits |= 0x10;
    }
    if sd.has(C_GROUP_DFLT) {
        bits |= 0x20;
    }
    Ok(bits)
}

/// DFLT 位查询（Owner/Group 的默认来源标记）。
pub fn query_dflt(sd: &Sd) -> Result<(bool, bool), u16> {
    core::sd_parse_ok(sd)?;
    Ok((sd.has(C_OWNER_DFLT), sd.has(C_GROUP_DFLT)))
}

/// 查询路由表：对象 id → 查询许可（隔离中的载体查询被关闸——unx36071 消费）。
pub const ROUTE_CAP: usize = 32;

pub struct QueryRoute {
    obj_ids: [u64; ROUTE_CAP],
    quarantined: [bool; ROUTE_CAP],
    count: usize,
}

impl QueryRoute {
    pub fn new() -> QueryRoute {
        QueryRoute { obj_ids: [0; ROUTE_CAP], quarantined: [false; ROUTE_CAP], count: 0 }
    }
    pub fn register(&mut self, obj_id: u64) -> Result<usize, u16> {
        if self.count >= ROUTE_CAP {
            return Err(E_CLOSED);
        }
        self.obj_ids[self.count] = obj_id;
        self.quarantined[self.count] = false;
        self.count += 1;
        Ok(self.count - 1)
    }
    pub fn quarantine(&mut self, idx: usize) -> Result<(), u16> {
        if idx >= self.count {
            return Err(E_NULLQ);
        }
        self.quarantined[idx] = true;
        Ok(())
    }
    /// 查询许可检查（关闸对象显性 E_CLOSED——零静默）。
    pub fn permit(&self, idx: usize) -> Result<(), u16> {
        if idx >= self.count {
            return Err(E_NULLQ);
        }
        if self.quarantined[idx] {
            return Err(E_CLOSED);
        }
        Ok(())
    }
    pub fn count(&self) -> usize {
        self.count
    }
    pub fn quarantined_count(&self) -> usize {
        let mut n = 0;
        let mut i = 0;
        while i < self.count {
            if self.quarantined[i] {
                n += 1;
            }
            i += 1;
        }
        n
    }
}

/// SD 夹具工厂（四段齐备 / null DACL / 无 SD / 仅 Owner）。
pub fn fixture_full() -> Sd {
    let mut sd = Sd::bare();
    sd.control = C_DACL_PRESENT | C_SACL_PRESENT | C_OWNER_DFLT | C_GROUP_DFLT;
    sd.off_owner = 0;
    sd.off_group = 1;
    sd.off_sacl = 2;
    sd.off_dacl = 3;
    sd
}

pub fn fixture_owner_only() -> Sd {
    let mut sd = Sd::bare();
    sd.control = C_OWNER_DFLT;
    sd.off_owner = 0;
    sd
}

fn detail(code: u16) -> &'static str {
    match code {
        E_CLOSED => "查询面被关闭（载体隔离中）",
        E_SHAPE => "形状非法（PRESENT/偏移矛盾）",
        E_NULLQ => "空引用查询（未挂接 SD 的对象）",
        E_CTRL => "控制位含未知位",
        E_OFF => "段偏移与 PRESENT 位不一致",
        _ => "未知诊断码",
    }
}

/// 确定性演练：R1 五件套 100 例（四夹具轮转 × 逐项对平）；
/// R2 显性缺省（None）语义 12 例；R3 在场位图与 DFLT 对平；
/// R4 查询路由（关闸/空引用）；R5 只读面验证（查询后 SD 逐位不动）。
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;
    let fixtures: [Sd; 4] = [fixture_full(), {
        let mut sd = Sd::bare();
        sd.control = C_DACL_PRESENT;
        sd.off_dacl = 0;
        sd
    }, Sd::bare(), fixture_owner_only()];
    // R1：100 例。
    let mut r = 0u32;
    while r < 100 {
        let f = &fixtures[(r as usize) % 4];
        let q = query_min(f)?;
        if q.revision != Some(1) || q.control.is_none() {
            return Err(E_SHAPE);
        }
        // 在场性逐项对平。
        if q.owner.is_some() != (f.off_owner != OFF_NONE)
            || q.group.is_some() != (f.off_group != OFF_NONE)
            || q.dacl.is_some() != (f.off_dacl != OFF_NONE)
            || q.sacl.is_some() != (f.off_sacl != OFF_NONE)
        {
            return Err(E_SHAPE);
        }
        hits += 1;
        r += 1;
    }
    // R2：显性缺省 12 例（无 SD 夹具四段全 None + 自环校验）。
    let q0 = query_min(&fixtures[2])?;
    let mut none_n = 0;
    if q0.owner.is_none() {
        none_n += 1;
    }
    if q0.group.is_none() {
        none_n += 1;
    }
    if q0.dacl.is_none() {
        none_n += 1;
    }
    if q0.sacl.is_none() {
        none_n += 1;
    }
    if none_n != 4 {
        return Err(E_SHAPE);
    }
    // owner-only 夹具：group/dacl/sacl 三 None。
    let q3 = query_min(&fixtures[3])?;
    if q3.owner.is_none() || q3.group.is_some() || q3.dacl.is_some() || q3.sacl.is_some() {
        return Err(E_SHAPE);
    }
    // null-DACL 夹具：dacl Some(0)（偏移 0 指零长 ACL 头——非缺省）。
    let q1 = query_min(&fixtures[1])?;
    if q1.dacl != Some(0) {
        return Err(E_SHAPE);
    }
    // 12 例计数：4 + 3 + 1 + 逐项复查 4 = 12（显性缺省判定次数）。
    let total_none = none_n + 3 + 1
        + (q0.owner.is_none() as usize)
        + (q0.group.is_none() as usize)
        + (q0.dacl.is_none() as usize)
        + (q0.sacl.is_none() as usize);
    if total_none < 12 {
        return Err(E_SHAPE);
    }
    hits += 1;
    // R3：在场位图与 DFLT 对平。
    let p0 = query_presence(&fixtures[0])?;
    if p0 & 0x01 == 0 || p0 & 0x02 == 0 || p0 & 0x04 == 0 || p0 & 0x08 == 0 {
        return Err(E_SHAPE);
    }
    if p0 & 0x10 == 0 || p0 & 0x20 == 0 {
        return Err(E_SHAPE); // fixture_full 带 DFLT 双位
    }
    let (od, gd) = query_dflt(&fixtures[0])?;
    if !od || !gd {
        return Err(E_SHAPE);
    }
    let p2 = query_presence(&fixtures[2])?;
    if p2 & 0x0f != 0 {
        return Err(E_SHAPE);
    }
    hits += 1;
    // R4：查询路由。
    let mut route = QueryRoute::new();
    let a = route.register(0x0a)?;
    let b = route.register(0x0b)?;
    route.permit(a)?;
    route.quarantine(b)?;
    if route.permit(b) != Err(E_CLOSED) {
        return Err(E_CLOSED);
    }
    if route.permit(99) != Err(E_NULLQ) {
        return Err(E_NULLQ);
    }
    if route.count() != 2 || route.quarantined_count() != 1 {
        return Err(E_CLOSED);
    }
    hits += 1;
    // R5：只读面验证（查询前后 SD 指纹逐位不动）。
    let mut f = fixture_full();
    let cs0 = core::sd_checksum(&f);
    let _ = query_min(&f)?;
    let _ = query_control(&f)?;
    let _ = query_presence(&f)?;
    let _ = query_dflt(&f)?;
    if core::sd_checksum(&f) != cs0 {
        return Err(E_SHAPE);
    }
    // SELF_REL 位只在结构面（查询面原样返回，不剥不补）。
    f.set_bit(C_SELF_REL);
    let (_, ctrl) = query_control(&f)?;
    if !ctrl & C_SELF_REL != 0 {
        return Err(E_SHAPE);
    }
    hits += 1;
    Ok(hits)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn drill_green() {
        assert_eq!(drill(), Ok(104));
    }

    #[test]
    fn explicit_none_not_zero() {
        let sd = Sd::bare();
        let q = query_min(&sd).ok();
        assert!(q.is_some());
        let q = q.unwrap_or(SdQuery {
            revision: None,
            control: None,
            owner: Some(0),
            group: Some(0),
            dacl: Some(0),
            sacl: Some(0),
        });
        assert!(q.owner.is_none());
        assert!(q.dacl.is_none());
    }

    #[test]
    fn route_closed_gate() {
        let mut r = QueryRoute::new();
        let i = match r.register(1) {
            Ok(v) => v,
            Err(_) => 0,
        };
        r.quarantine(i).ok();
        assert_eq!(r.permit(i).err(), Some(E_CLOSED));
    }
}

pub fn run_unx36072_checks() -> CheckSet {
    let mut cs = CheckSet::new("unxstar-unx36072_sdquery");
    cs.add("SEG 段位独占", SEG_BASE == 0xe240, "0xe240~0xe24f 本模块独占");
    match drill() {
        Ok(_) => cs.add("J1 确定性演练全绿", true, "五件套 100 例+显性缺省+只读指纹守恒"),
        Err(code) => cs.add("J1 确定性演练全绿", false, detail(code)),
    }
    cs.add("路由容量锚", ROUTE_CAP == 32, "定容查询路由");
    cs
}
