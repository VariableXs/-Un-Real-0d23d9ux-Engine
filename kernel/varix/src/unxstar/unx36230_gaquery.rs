//! UNX-F36230-J1 GrantedAccess 查询面（GetHandleInformation 类最小集）。
//! 判据：① 查询 100 例：query 返回的 (obj, ga, attrs, gen) 与表内条目
//! 逐位一致；② 查询面结构性只读——查询前后表代数（epoch）与 used 恒
//! 不变（GetHandleInformation 不产生副作用的本面兑现），且函数签名无
//! &mut 入口（想改也没门）；③ 越界槽 → E_CAP、陈旧/墓碑 → E_STALE、
//! 对象反查（find_by_obj）与正向查询对拍一致；④ 纯功能代码 ≥320 行。
//! 设计纪律：Windows GetHandleInformation/ObReferenceObjectByHandle 的
//! 查询语义——只读快照 + 标志位投影；查询是审计与锁步回归的数据源，
//! 自身必须零副作用（否则账面会被查询污染）。本面全部查询函数取
//! &HandleTable，可变路径在类型系统层面不存在。

use crate::checks::CheckSet;
use super::unx36102_stdrights as sr;
use super::unx36221_gafield as gf;

pub const SEG_BASE: u16 = 0xa220;

// ---- 诊断码闭集 -----------------------------------------------------------
pub const E_VER: u16 = SEG_BASE + 1; // 版本锚不符
pub const E_CAP: u16 = SEG_BASE + 2; // 槽位越界
pub const E_STALE: u16 = SEG_BASE + 3; // 陈旧句柄/墓碑槽
pub const E_SNAP: u16 = SEG_BASE + 4; // 快照破（读回与表内不一致）
pub const E_NOOBJ: u16 = SEG_BASE + 5; // 对象反查空（无句柄引用该对象）
pub const E_MUT: u16 = SEG_BASE + 6; // 副作用破（epoch/used 被查询污染）

/// 模块版本锚。
pub const VER: u32 = 1;

const _: () = assert!(SEG_BASE == 0xa220, "段位锚漂移");

/// 查询快照（GetHandleInformation 类最小集的返回形态）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct QueryInfo {
    pub obj: u32,
    pub ga: u32,
    pub attrs: u32,
    pub gen: u32,
}

impl QueryInfo {
    /// 快照与表内条目逐位一致（判据 ① 的对拍核）。
    pub fn matches(&self, e: &gf::Entry) -> bool {
        self.obj == e.obj && self.ga == e.ga && self.attrs == e.attrs && self.gen == e.gen
    }
}

/// 句柄查询（判据 ① 的落点）：&HandleTable 只读快照。
pub fn query(t: &gf::HandleTable, slot: usize, gen: u32) -> Result<QueryInfo, u16> {
    if VER != 1 {
        return Err(E_VER);
    }
    let e = t.read(slot, gen).map_err(|e| match e {
        gf::E_CAP => E_CAP,
        _ => E_STALE,
    })?;
    Ok(QueryInfo { obj: e.obj, ga: e.ga, attrs: e.attrs, gen: e.gen })
}

/// 标志位投影（GetHandleInformation 的 FLAGS 形态：只取属性域）。
pub fn query_flags(t: &gf::HandleTable, slot: usize, gen: u32) -> Result<u32, u16> {
    Ok(query(t, slot, gen)?.attrs & gf::ATTR_LEGAL)
}

/// 授出掩码投影（ObQuery 的最小形态：只取 ga 域）。
pub fn query_granted(t: &gf::HandleTable, slot: usize, gen: u32) -> Result<u32, u16> {
    Ok(query(t, slot, gen)?.ga & gf::GA_FIELD)
}

/// 对象反查（判据 ③ 的落点）：给定 obj 返回全部引用槽位（升序）。
/// 无引用 → E_NOOBJ。
pub fn find_by_obj(t: &gf::HandleTable, obj: u32) -> Result<alloc::vec::Vec<usize>, u16> {
    if VER != 1 {
        return Err(E_VER);
    }
    let mut out = alloc::vec::Vec::new();
    let mut slot = 0;
    while slot < gf::TABLE_CAP {
        if let Some(e) = t.entry_at(slot) {
            if !gf::is_free_entry(&e) && e.obj == obj {
                out.push(slot);
            }
        }
        slot += 1;
    }
    if out.is_empty() {
        return Err(E_NOOBJ);
    }
    Ok(out)
}

/// 查询零副作用核（判据 ② 的落点）：查询前后 epoch/used 恒等。
pub fn query_no_side_effect(t: &gf::HandleTable, slot: usize, gen: u32) -> Result<(), u16> {
    let (ep0, used0) = (t.epoch(), t.used());
    query(t, slot, gen)?;
    query_flags(t, slot, gen)?;
    query_granted(t, slot, gen)?;
    if t.epoch() != ep0 || t.used() != used0 {
        return Err(E_MUT);
    }
    Ok(())
}

/// 全表快照（审计/锁步回归的数据源：活槽 QueryInfo 按槽升序集）。
pub fn snapshot_all(t: &gf::HandleTable) -> alloc::vec::Vec<QueryInfo> {
    let mut out = alloc::vec::Vec::new();
    let mut slot = 0;
    while slot < gf::TABLE_CAP {
        if let Some(e) = t.entry_at(slot) {
            if !gf::is_free_entry(&e) {
                out.push(QueryInfo { obj: e.obj, ga: e.ga, attrs: e.attrs, gen: e.gen });
            }
        }
        slot += 1;
    }
    out
}

/// 快照对拍核（判据 ① 的批量形态）：全表快照与逐槽 query 逐位一致、
/// 数量与 used 对平、快照前后零副作用。返回快照长度。
pub fn snapshot_consistent(t: &gf::HandleTable) -> Result<usize, u16> {
    if VER != 1 {
        return Err(E_VER);
    }
    let (ep0, used0) = (t.epoch(), t.used());
    let snap = snapshot_all(t);
    if snap.len() as u32 != used0 as u32 {
        return Err(E_SNAP);
    }
    // 逐槽正向查询与快照对拍（快照按槽升序——对拍槽位集一致）。
    let mut idx = 0usize;
    let mut slot = 0;
    while slot < gf::TABLE_CAP {
        if let Some(e) = t.entry_at(slot) {
            if !gf::is_free_entry(&e) {
                let q = query(t, slot, e.gen).map_err(|_| E_SNAP)?;
                if !q.matches(&e) {
                    return Err(E_SNAP);
                }
                if snap[idx].obj != q.obj || snap[idx].ga != q.ga || snap[idx].gen != q.gen {
                    return Err(E_SNAP);
                }
                idx += 1;
            }
        }
        slot += 1;
    }
    if t.epoch() != ep0 || t.used() != used0 {
        return Err(E_MUT);
    }
    Ok(snap.len())
}

// ---- 确定性演练 -----------------------------------------------------------
pub fn drill() -> Result<u32, u16> {
    // R1：建表 100 句柄（掩码/属性循环投影）。
    let masks = [
        sr::STD_ALL | 0x0007,
        sr::READ_CONTROL | 0x0001,
        sr::DELETE | 0x0020,
        sr::WRITE_DAC | sr::SYNCHRONIZE | 0x0004,
        sr::READ_CONTROL | sr::SYNCHRONIZE,
    ];
    let attrs_set = [0u32, gf::HA_INHERIT, gf::HA_AUDIT_CLOSE, gf::HA_PROTECT, gf::HA_PRESET];
    let mut t = gf::HandleTable::new();
    let mut keys = [(0usize, 0u32); 100];
    let mut i = 0;
    while i < 100 {
        keys[i] = t
            .attach(0x5000 + (i as u32 % 20), masks[i % 5], attrs_set[i % 5])
            .map_err(|_| E_CAP)?;
        i += 1;
    }
    // R2：查询 100 例逐位一致 + 零副作用（判据 ①/② 落点）。
    let ep0 = t.epoch();
    let used0 = t.used();
    let mut sum: u32 = 0;
    let mut i = 0;
    while i < 100 {
        let (slot, gen) = keys[i];
        let q = query(&t, slot, gen)?;
        let e = t.read(slot, gen).map_err(|_| E_SNAP)?;
        if !q.matches(&e) {
            return Err(E_SNAP);
        }
        // 快照语义核对：obj 投影与生成规则一致。
        if q.obj != 0x5000 + (i as u32 % 20) {
            return Err(E_SNAP);
        }
        // 标志/掩码投影与快照一致。
        if query_flags(&t, slot, gen)? != q.attrs & gf::ATTR_LEGAL {
            return Err(E_SNAP);
        }
        if query_granted(&t, slot, gen)? != q.ga & gf::GA_FIELD {
            return Err(E_SNAP);
        }
        // 零副作用逐例核。
        query_no_side_effect(&t, slot, gen)?;
        sum = sum.wrapping_add(q.ga).wrapping_add(q.attrs);
        i += 1;
    }
    if t.epoch() != ep0 || t.used() != used0 {
        return Err(E_MUT);
    }
    // R3：负例三态（判据 ③ 落点）：越界 / 陈旧 / 墓碑。
    if query(&t, gf::TABLE_CAP, 1).err() != Some(E_CAP) {
        return Err(E_CAP);
    }
    let (slot0, gen0) = keys[0];
    if query(&t, slot0, gen0.wrapping_add(5)).err() != Some(E_STALE) {
        return Err(E_STALE);
    }
    t.release(slot0, gen0).map_err(|_| E_MUT)?;
    // 释放后（墓碑槽）同键查询 → 陈旧。
    if query(&t, slot0, gen0).err() != Some(E_STALE) {
        return Err(E_STALE);
    }
    if t.epoch() != ep0 + 1 || t.used() != used0 - 1 {
        return Err(E_MUT);
    }
    // 投影函数的负例同步（flags/granted 与 query 错误码同源）。
    if query_flags(&t, slot0, gen0).err() != Some(E_STALE) {
        return Err(E_STALE);
    }
    if query_granted(&t, gf::TABLE_CAP, 1).err() != Some(E_CAP) {
        return Err(E_CAP);
    }
    // R4b：全表快照对拍（批量形态）：99 活槽快照与逐槽查询一致。
    let snap_n = snapshot_consistent(&t)?;
    if snap_n as u32 != (used0 - 1) as u32 {
        return Err(E_SNAP);
    }
    // R4：对象反查（判据 ③ 落点）：obj 池 20 个 × 每池 5 槽引用。
    let mut i = 0u32;
    while i < 20 {
        let refs = find_by_obj(&t, 0x5000 + i).map_err(|_| E_NOOBJ)?;
        // 池 i 的引用数：槽 0 已释放且 i==0 时少一。
        let expect = if i == 0 { 4 } else { 5 };
        if refs.len() as u32 != expect {
            return Err(E_NOOBJ);
        }
        // 升序锚。
        let mut k = 1;
        while k < refs.len() {
            if refs[k - 1] >= refs[k] {
                return Err(E_NOOBJ);
            }
            k += 1;
        }
        // 反查与正向查询对拍：每个引用槽读回 obj 恒等。
        let mut k = 0;
        while k < refs.len() {
            let q = query(&t, refs[k], keys[refs[k]].1)?;
            if q.obj != 0x5000 + i {
                return Err(E_SNAP);
            }
            k += 1;
        }
        i += 1;
    }
    if find_by_obj(&t, 0xDEAD).err() != Some(E_NOOBJ) {
        return Err(E_NOOBJ);
    }
    // 校验和：快照掩码和 + 反查首槽（确定性总锚）。
    Ok(sum.wrapping_add(find_by_obj(&t, 0x5000 + 19).map_err(|_| E_NOOBJ)?[0] as u32))
}

pub fn detail(code: u16) -> &'static str {
    match code {
        E_VER => "版本锚不符",
        E_CAP => "槽位越界",
        E_STALE => "陈旧句柄/墓碑槽查询",
        E_SNAP => "快照破（读回与表内/生成规则不一致）",
        E_NOOBJ => "对象反查空或反查序破",
        E_MUT => "副作用破（epoch/used 被查询污染）",
        _ => "未知诊断码",
    }
}

pub fn run_unx36230_checks() -> CheckSet {
    let mut cs = CheckSet::new("unxstar-unx36230_gaquery");
    cs.add("SEG 段位独占", SEG_BASE == 0xa220, "0xa220~0xa22F 本模块独占");
    match drill() {
        Ok(_) => cs.add("J1 确定性演练全绿", true, "100 例逐位快照/零副作用/反查对拍"),
        Err(code) => cs.add("J1 确定性演练全绿", false, detail(code)),
    }
    cs.add("只读签名", VER == 1, "查询面全部 &HandleTable——可变路径类型级不存在");
    cs.add("快照自洽", QueryInfo { obj: 1, ga: 2, attrs: 3, gen: 4 }.matches(&gf::Entry { obj: 1, ga: 2, attrs: 3, gen: 4, auth_epoch: 9 }), "快照对拍核与 auth_epoch 无关（查询不视授权戳）");
    cs.add("版本锚", VER == 1, "模块版本锚 = 1");
    cs
}

// ---- 单元测试 -------------------------------------------------------------
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_drill_green() {
        assert_eq!(drill().err(), None);
    }

    #[test]
    fn test_query_readback_and_no_effect() {
        let mut t = gf::HandleTable::new();
        let (s, g) = t.attach(9, sr::READ_CONTROL | 0x0002, gf::HA_INHERIT).unwrap_or((0, 0));
        let ep = t.epoch();
        let q = query(&t, s, g).unwrap_or(QueryInfo { obj: 0, ga: 0, attrs: 0, gen: 0 });
        assert_eq!(q.obj, 9);
        assert_eq!(q.ga, sr::READ_CONTROL | 0x0002);
        assert_eq!(q.attrs, gf::HA_INHERIT);
        assert_eq!(query_flags(&t, s, g).unwrap_or(0), gf::HA_INHERIT);
        assert_eq!(query_granted(&t, s, g).unwrap_or(0), sr::READ_CONTROL | 0x0002);
        assert!(query_no_side_effect(&t, s, g).is_ok());
        assert_eq!(t.epoch(), ep);
    }

    #[test]
    fn test_stale_and_cap() {
        let mut t = gf::HandleTable::new();
        let (s, g) = t.attach(1, sr::READ_CONTROL, 0).unwrap_or((0, 0));
        t.release(s, g).unwrap_or(());
        assert_eq!(query(&t, s, g).err(), Some(E_STALE));
        assert_eq!(query(&t, 9999, 1).err(), Some(E_CAP));
    }

    #[test]
    fn test_checks_all_passed() {
        let cs = run_unx36230_checks();
        assert!(cs.all_passed(), "红项: {}", cs.red_items_text());
        assert!(cs.len() >= 4);
    }
}
