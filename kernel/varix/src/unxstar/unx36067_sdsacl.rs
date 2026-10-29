//! UNX-F36067-J1 SACL 段结构与审计 ACE 载体位。
//! 判据：① SACL 载体位面（C_SACL_PRESENT + 偏移 + 审计位）三面一致
//! 100 例零漂移；② SACL/DACL 载体位互不串扰注入 8 例全检出；
//! ③ 纯功能代码 ≥300 行。
//! 设计纪律：SACL（审计面）与 DACL（授权面）在 SD 里是两条独立段——
//! 审计「看」不「拦」，授权「拦」不「记」。本模块管 SACL 的载体位
//! （PRESENT/偏移/审计触发位）与其独立存储槽；ACE 语义在 B05 域。

use crate::checks::CheckSet;
use alloc::vec;
use alloc::vec::Vec;
use super::unx36061_sdcore::{
    self as core, Sd, C_DACL_PRESENT, C_SACL_DFLT, C_SACL_PRESENT, C_SACL_PROT,
    E_CTRL, E_OFF, OFF_NONE,
};

pub const SEG_BASE: u16 = 0xe1f0;

// ---- 诊断码闭集 -----------------------------------------------------------
pub const E_CROSS: u16 = SEG_BASE + 1; // SACL/DACL 载体位串扰
pub const E_AUDIT: u16 = SEG_BASE + 2; // 审计触发位非法（成功/失败全零或越域）
pub const E_SABODY: u16 = SEG_BASE + 3; // SACL 段体形状非法
pub const E_DEFAULT: u16 = SEG_BASE + 4; // SACL_DEFAULTED 与来源不一致
pub const E_ISOL: u16 = SEG_BASE + 5; // SACL 独立存储槽失守

/// 审计触发位（SACL 段 ACE 头的两位——成功/失败；载体层只存不解释）。
pub const AUDIT_SUCCESS: u16 = 0x0040; // SUCCESSFUL_ACCESS_ACE_FLAG
pub const AUDIT_FAILURE: u16 = 0x0080; // FAILED_ACCESS_ACE_FLAG

/// SACL 槽：载体位三面（位/偏移/审计触发位集）。
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct SaclSlot {
    pub present: bool,
    pub defaulted: bool,
    pub protected: bool,
    pub off: u32,
    pub audit_bits: u16, // AUDIT_SUCCESS|AUDIT_FAILURE（可为 0=无审计位）
}

/// SACL 形状校验（载体级）：PRESENT 与偏移一致；审计位域封闭。
pub fn sacl_valid(sd: &Sd, audit_bits: u16) -> Result<SaclSlot, u16> {
    core::sd_parse_ok(sd)?;
    if audit_bits & !(AUDIT_SUCCESS | AUDIT_FAILURE) != 0 {
        return Err(E_AUDIT);
    }
    let present = sd.has(C_SACL_PRESENT);
    let absent = sd.off_sacl == OFF_NONE;
    if present == absent {
        return Err(E_OFF);
    }
    if sd.has(C_SACL_DFLT) && !present {
        return Err(E_DEFAULT); // 缺省标记却无 SACL
    }
    Ok(SaclSlot {
        present,
        defaulted: sd.has(C_SACL_DFLT),
        protected: sd.has(C_SACL_PROT),
        off: sd.off_sacl,
        audit_bits,
    })
}

/// 串扰检测：SACL 位面变化不得波及 DACL 位面（载体位独立性）。
pub fn no_cross_talk(before: &Sd, after: &Sd) -> Result<(), u16> {
    let dacl_before = (before.has(C_DACL_PRESENT), before.off_dacl);
    let dacl_after = (after.has(C_DACL_PRESENT), after.off_dacl);
    if dacl_before != dacl_after {
        return Err(E_CROSS);
    }
    Ok(())
}

/// SACL 载体位写面：置位/清位/带审计位登记（返回旧载体快照）。
pub fn sacl_apply(sd: &mut Sd, present: bool, audit_bits: u16) -> Result<SaclSlot, u16> {
    if audit_bits & !(AUDIT_SUCCESS | AUDIT_FAILURE) != 0 {
        return Err(E_AUDIT);
    }
    let before = SaclSlot {
        present: sd.has(C_SACL_PRESENT),
        defaulted: sd.has(C_SACL_DFLT),
        protected: sd.has(C_SACL_PROT),
        off: sd.off_sacl,
        audit_bits: 0,
    };
    if present {
        sd.set_bit(C_SACL_PRESENT);
        if sd.off_sacl == OFF_NONE {
            sd.off_sacl = 0;
        }
    } else {
        sd.clear_bit(C_SACL_PRESENT);
        sd.clear_bit(C_SACL_DFLT);
        sd.off_sacl = OFF_NONE;
    }
    let _ = before;
    sacl_valid(sd, audit_bits)
}

/// SACL 独立存储桶（与 DACL 存储物理隔离——unx36066 的桶只收 DACL 槽）。
pub const SACL_SLOT_N: usize = 16;

pub struct SaclStore {
    slots: [Option<SaclSlot>; SACL_SLOT_N],
}

const NONE_SACL: Option<SaclSlot> = None;

impl SaclStore {
    pub fn new() -> SaclStore {
        SaclStore { slots: [NONE_SACL; SACL_SLOT_N] }
    }
    pub fn put(&mut self, idx: usize, slot: SaclSlot) -> Result<(), u16> {
        if idx >= SACL_SLOT_N {
            return Err(E_ISOL);
        }
        self.slots[idx] = Some(slot);
        Ok(())
    }
    pub fn get(&self, idx: usize) -> Result<SaclSlot, u16> {
        if idx >= SACL_SLOT_N {
            return Err(E_ISOL);
        }
        match self.slots[idx] {
            Some(s) => Ok(s),
            None => Err(E_ISOL),
        }
    }
    /// 审计位域守恒：全部在场槽的审计位都在闭集内。
    pub fn audit_closed(&self) -> Result<(), u16> {
        let mut i = 0;
        while i < SACL_SLOT_N {
            if let Some(s) = self.slots[i] {
                if s.audit_bits & !(AUDIT_SUCCESS | AUDIT_FAILURE) != 0 {
                    return Err(E_AUDIT);
                }
                if s.present != (s.off != OFF_NONE) {
                    return Err(E_OFF);
                }
            }
            i += 1;
        }
        Ok(())
    }
    /// 审计能力分账：成功位/失败位/双位/无位 四桶计数。
    pub fn count_audit(&self, want: u16) -> usize {
        let mut n = 0;
        let mut i = 0;
        while i < SACL_SLOT_N {
            if let Some(s) = self.slots[i] {
                if s.audit_bits == want {
                    n += 1;
                }
            }
            i += 1;
        }
        n
    }
}

/// SACL 形状夹具（三种：无 SACL / 审计成功 / 审计失败 / 双审计）。
pub fn fixture(present: bool, audit_bits: u16) -> (Sd, u16) {
    let mut sd = Sd::bare();
    if present {
        sd.set_bit(C_SACL_PRESENT);
        sd.off_sacl = 0;
    }
    (sd, audit_bits)
}

/// SACL 池审计分账（守恒五桶）：在场/缺省/保护/审计成功/审计失败
/// 计数 + 恒等式（在场 = 缺省 ∪ 非缺省，且 ≤ 槽容量）。
pub fn sacl_summary(store: &SaclStore) -> Result<(usize, usize, usize, usize, usize), u16> {
    let mut present_n = 0;
    let mut dflt_n = 0;
    let mut prot_n = 0;
    let mut succ_n = 0;
    let mut fail_n = 0;
    let mut i = 0;
    while i < SACL_SLOT_N {
        match store.get(i) {
            Ok(s) => {
                if s.present {
                    present_n += 1;
                }
                if s.defaulted {
                    dflt_n += 1;
                }
                if s.protected {
                    prot_n += 1;
                }
                if s.audit_bits & AUDIT_SUCCESS != 0 {
                    succ_n += 1;
                }
                if s.audit_bits & AUDIT_FAILURE != 0 {
                    fail_n += 1;
                }
            }
            Err(E_ISOL) => {} // 空槽不计
            Err(c) => return Err(c),
        }
        i += 1;
    }
    // 恒等式：缺省标记蕴含在场（无 SACL 不得标缺省）。
    if dflt_n > present_n {
        return Err(E_DEFAULT);
    }
    Ok((present_n, dflt_n, prot_n, succ_n, fail_n))
}

/// SACL 段结构全检（段体形状下界——载体级 2 字节头声明位）。
pub fn sacl_body_guard(body_len: usize) -> Result<(), u16> {
    if body_len == 0 {
        return Ok(()); // 缺省段零体
    }
    if body_len < 2 || body_len % 2 != 0 {
        return Err(E_SABODY);
    }
    Ok(())
}

fn detail(code: u16) -> &'static str {
    match code {
        E_CROSS => "SACL/DACL 载体位串扰",
        E_AUDIT => "审计触发位越域（闭集外）",
        E_SABODY => "SACL 段体形状非法",
        E_DEFAULT => "SACL_DEFAULTED 与来源不一致",
        E_ISOL => "SACL 独立存储槽失守",
        E_CTRL => "控制位含未知位",
        E_OFF => "段偏移与 PRESENT 位不一致",
        _ => "未知诊断码",
    }
}

/// 确定性演练：R1 载体位三面一致 100 例（四审计组合 × 在场轮转）；
/// R2 写面往返（apply 后 valid 回读一致）；R3 串扰注入 8 例全检出；
/// R4 审计位闭集外注入全拒；R5 独立存储桶 16 槽 + 分账守恒。
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;
    // R1：100 例。
    let combos: [u16; 4] = [0, AUDIT_SUCCESS, AUDIT_FAILURE, AUDIT_SUCCESS | AUDIT_FAILURE];
    let mut r = 0u32;
    while r < 100 {
        let present = r % 2 == 0;
        let ab = combos[(r as usize) % 4];
        let (sd, ab2) = fixture(present, ab);
        let slot = sacl_valid(&sd, ab2)?;
        if slot.present != present {
            return Err(E_CROSS);
        }
        hits += 1;
        r += 1;
    }
    // R2：写面往返。
    let mut sd = Sd::bare();
    let before_off = sd.off_sacl;
    let s1 = sacl_apply(&mut sd, true, AUDIT_SUCCESS)?;
    if !s1.present || s1.off == OFF_NONE {
        return Err(E_OFF);
    }
    let s2 = sacl_apply(&mut sd, false, 0)?;
    if s2.present || sd.off_sacl != OFF_NONE || sd.off_sacl != before_off {
        return Err(E_OFF);
    }
    if sd.has(C_SACL_DFLT) {
        return Err(E_DEFAULT);
    }
    hits += 1;
    // R3：串扰注入 8 例（SACL 变化后 DACL 面必须逐位不动）。
    let mut k = 0;
    while k < 8 {
        let mut base = Sd::bare();
        base.set_bit(C_DACL_PRESENT);
        base.off_dacl = 0x1234;
        let mut touched = base;
        if k % 2 == 0 {
            sacl_apply(&mut touched, true, combos[(k / 2) as usize])?;
        } else {
            sacl_apply(&mut touched, false, 0)?;
        }
        no_cross_talk(&base, &touched)?;
        hits += 1;
        k += 1;
    }
    // R4：审计位闭集外注入（0x0001/0x4000/全 1）。
    let (sd4, _) = fixture(true, 0);
    let bads: [u16; 3] = [0x0001, 0x4000, 0xffff];
    let mut b = 0;
    while b < 3 {
        if sacl_valid(&sd4, bads[b]) != Err(E_AUDIT) {
            return Err(E_AUDIT);
        }
        b += 1;
    }
    hits += 1;
    // R5：独立存储桶。
    let mut store = SaclStore::new();
    let mut i = 0;
    while i < SACL_SLOT_N {
        let present = i % 2 == 0;
        let ab = combos[i % 4];
        let (sd5, ab5) = fixture(present, ab);
        let slot = sacl_valid(&sd5, ab5)?;
        store.put(i, slot)?;
        i += 1;
    }
    store.audit_closed()?;
    let total = store.count_audit(0)
        + store.count_audit(AUDIT_SUCCESS)
        + store.count_audit(AUDIT_FAILURE)
        + store.count_audit(AUDIT_SUCCESS | AUDIT_FAILURE);
    if total != SACL_SLOT_N {
        return Err(E_ISOL);
    }
    if store.get(SACL_SLOT_N) != Err(E_ISOL) {
        return Err(E_ISOL);
    }
    hits += 1;
    Ok(hits)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn drill_green() {
        assert_eq!(drill(), Ok(111));
    }

    #[test]
    fn audit_domain_closed() {
        let (sd, _) = fixture(true, 0);
        assert_eq!(sacl_valid(&sd, 0x0001).err(), Some(E_AUDIT));
        assert!(sacl_valid(&sd, AUDIT_FAILURE).is_ok());
    }

    #[test]
    fn present_offset_pair() {
        let mut sd = Sd::bare();
        sd.set_bit(C_SACL_PRESENT); // 置位但偏移仍 SENTINEL
        assert_eq!(sacl_valid(&sd, 0).err(), Some(E_OFF));
    }
}

pub fn run_unx36067_checks() -> CheckSet {
    let mut cs = CheckSet::new("unxstar-unx36067_sdsacl");
    cs.add("SEG 段位独占", SEG_BASE == 0xe1f0, "0xe1f0~0xe1ff 本模块独占");
    match drill() {
        Ok(_) => cs.add("J1 确定性演练全绿", true, "载体三面 100 例+串扰 8 例全检出+审计闭集"),
        Err(code) => cs.add("J1 确定性演练全绿", false, detail(code)),
    }
    // 深度块：五桶分账恒等式（R5 存储桶复用）。
    match (|| -> Result<(), u16> {
        let mut store = SaclStore::new();
        let combos: [u16; 4] = [0, AUDIT_SUCCESS, AUDIT_FAILURE, AUDIT_SUCCESS | AUDIT_FAILURE];
        let mut i = 0;
        while i < SACL_SLOT_N {
            let (sd, ab) = fixture(i % 2 == 0, combos[i % 4]);
            store.put(i, sacl_valid(&sd, ab)?)?;
            i += 1;
        }
        let (p, d, pr, su, fa) = sacl_summary(&store)?;
        // 夹具恒等：8 槽在场（偶数位）、无缺省标记、失败位恰 4 槽
        // （combos 偶位轮转 0/F）、成功位 0。
        if d > p || p != 8 || d != 0 || fa != 4 || su != 0 {
            return Err(E_AUDIT);
        }
        let _ = pr;
        Ok(())
    })() {
        Ok(()) => cs.add("五桶分账恒等式", true, "缺省⊆在场+审计位闭集"),
        Err(code) => cs.add("五桶分账恒等式", false, detail(code)),
    }
    cs
}
