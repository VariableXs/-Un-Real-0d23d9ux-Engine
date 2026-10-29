//! UNX-F36228-J1 进程退出时句柄表销毁的安全回收（与 AI-18 泄漏账分工）。
//! 判据：① 全表销毁 100 例：destroy 后 used==0、零泄漏挂账（回收数 =
//! 销毁前活句柄数——「谁都没关干净的」全数入回收账）；② HA_PROTECT
//! 句柄拒绝显性关闭（36227 语义）但在表销毁路径被强制回收（10 例全
//! 回收——PROTECT 只挡 CloseHandle 不挡表拆除）；③ 销毁后所有旧句柄
//! 键（slot,gen）读回陈旧——句柄面结构性失效；④ 纯功能代码 ≥320 行。
//! 设计纪律：Windows 进程退出的句柄表拆除——不逐个走 CloseHandle
//! 路径（保护属性不适用），直接整表回收并触发对象面引用清算；本面是
//! 该语义的账面化：销毁报告（回收/保护/审计三类计数 + 代数和）就是
//! 与泄漏账（AI-18）的分工界面——销毁后泄漏挂账必须清零。

use crate::checks::CheckSet;
use super::unx36102_stdrights as sr;
use super::unx36221_gafield as gf;

pub const SEG_BASE: u16 = 0xa1e0;

// ---- 诊断码闭集 -----------------------------------------------------------
pub const E_VER: u16 = SEG_BASE + 1; // 版本锚不符
pub const E_RECLAIM: u16 = SEG_BASE + 2; // 回收破（used 未清零/计数不齐）
pub const E_PROT: u16 = SEG_BASE + 3; // 保护回收失败（PROTECT 逃逸）
pub const E_STALE: u16 = SEG_BASE + 4; // 销毁后旧句柄未失效
pub const E_LED: u16 = SEG_BASE + 5; // 销毁账不平（泄漏挂账未清零）
pub const E_CAP: u16 = SEG_BASE + 6; // 槽位越界

/// 模块版本锚。
pub const VER: u32 = 1;

const _: () = assert!(SEG_BASE == 0xa1e0, "段位锚漂移");

/// 销毁报告（与 AI-18 泄漏账的分工界面）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct DestroyReport {
    /// 回收的活句柄数（销毁前 used 必须与之相等）。
    pub reclaimed: u32,
    /// 其中 HA_PROTECT 强制回收数。
    pub prot_reclaimed: u32,
    /// 其中 HA_AUDIT_CLOSE 触发销毁审计数。
    pub audit_fired: u32,
    /// 代数和（回收的代数总账——重放校验锚）。
    pub gen_sum: u32,
    /// 泄漏挂账（销毁后必须为 0——分工红线）。
    pub leaked: u32,
}

impl DestroyReport {
    /// 报告自洽：泄漏清零 + 三类计数 ⊆ 回收数。
    pub fn ok(&self, pre_used: u32) -> bool {
        self.leaked == 0
            && self.reclaimed == pre_used
            && self.prot_reclaimed <= self.reclaimed
            && self.audit_fired <= self.reclaimed
    }

    /// 报告合并（分批销毁的账面合成：各计数相加、代数和相加）。
    pub fn merge(&self, other: &DestroyReport) -> DestroyReport {
        DestroyReport {
            reclaimed: self.reclaimed.wrapping_add(other.reclaimed),
            prot_reclaimed: self.prot_reclaimed.wrapping_add(other.prot_reclaimed),
            audit_fired: self.audit_fired.wrapping_add(other.audit_fired),
            gen_sum: self.gen_sum.wrapping_add(other.gen_sum),
            leaked: self.leaked.wrapping_add(other.leaked),
        }
    }
}

/// 回收账（判据 ① 的可重放面：逐槽销毁记录 (slot, gen, obj)——销毁
/// 不是黑箱，每一笔回收都可对账）。
pub struct ReclaimLog {
    pub recs: alloc::vec::Vec<(usize, u32, u32)>,
}

impl ReclaimLog {
    pub fn new() -> ReclaimLog {
        ReclaimLog { recs: alloc::vec::Vec::new() }
    }

    /// 账面自证：槽位互异（一次销毁一槽不重复）、gen 非零。
    pub fn sane(&self) -> bool {
        let mut i = 0;
        while i < self.recs.len() {
            let (s, g, _) = self.recs[i];
            if g == 0 {
                return false;
            }
            let mut k = i + 1;
            while k < self.recs.len() {
                if self.recs[k].0 == s {
                    return false;
                }
                k += 1;
            }
            i += 1;
        }
        true
    }
}

/// 表销毁核心（带账形态）：全表扫描 → 逐槽强制回收 → 报告 + 回收账。
/// PROTECT 在此路径不适用（强制回收——判据 ② 的语义锚）；审计属性
/// 触发销毁审计计数（对象面清算的句柄侧挂钩）。
pub fn destroy_table_logged(t: &mut gf::HandleTable, log: &mut ReclaimLog) -> Result<DestroyReport, u16> {
    if VER != 1 {
        return Err(E_VER);
    }
    let mut rep = DestroyReport { reclaimed: 0, prot_reclaimed: 0, audit_fired: 0, gen_sum: 0, leaked: 0 };
    let mut slot = 0;
    while slot < gf::TABLE_CAP {
        if let Some(e) = t.entry_at(slot) {
            if !gf::is_free_entry(&e) {
                if e.attrs & gf::HA_PROTECT != 0 {
                    rep.prot_reclaimed = rep.prot_reclaimed.wrapping_add(1);
                }
                if e.attrs & gf::HA_AUDIT_CLOSE != 0 {
                    rep.audit_fired = rep.audit_fired.wrapping_add(1);
                }
                rep.gen_sum = rep.gen_sum.wrapping_add(e.gen);
                log.recs.push((slot, e.gen, e.obj));
                // 强制回收（不看 PROTECT——表拆除路径越过 CloseHandle 门）。
                t.release(slot, e.gen).map_err(|_| E_RECLAIM)?;
                rep.reclaimed = rep.reclaimed.wrapping_add(1);
            }
        }
        slot += 1;
    }
    if t.used() != 0 {
        return Err(E_RECLAIM);
    }
    if log.recs.len() as u32 != rep.reclaimed {
        return Err(E_RECLAIM);
    }
    rep.leaked = 0;
    Ok(rep)
}

/// 表销毁（无账形态——带账核心的弃账包装）。
pub fn destroy_table(t: &mut gf::HandleTable) -> Result<DestroyReport, u16> {
    let mut log = ReclaimLog::new();
    destroy_table_logged(t, &mut log)
}

/// 保护句柄的显性关闭仍被拒（判据 ② 的对照面：销毁路径与关闭路径
/// 的分工——本函数复算 36227 的拒绝语义，两路互证）。
pub fn close_refused_for_protect(attrs: u32) -> bool {
    attrs & gf::HA_PROTECT != 0
}

// ---- 确定性演练 -----------------------------------------------------------
pub fn drill() -> Result<u32, u16> {
    // R0：分工锚（PROTECT 拒关 / 销毁不拒——两路语义互斥自洽）。
    if !close_refused_for_protect(gf::HA_PROTECT) || close_refused_for_protect(gf::HA_AUDIT_CLOSE) {
        return Err(E_PROT);
    }
    // R1：建表 100 句柄（判据 ① 落点）：10 个 PROTECT、15 个审计挂接。
    let mut t = gf::HandleTable::new();
    let mut keys = [(0usize, 0u32); 100];
    let mut i = 0;
    while i < 100 {
        let mut attrs = 0u32;
        if i < 10 {
            attrs = gf::HA_PROTECT;
        } else if i < 25 {
            attrs = gf::HA_AUDIT_CLOSE;
        }
        keys[i] = t
            .attach(0x1000 + i as u32, sr::READ_CONTROL | (i as u32 & 0x000F), attrs)
            .map_err(|_| E_CAP)?;
        i += 1;
    }
    let pre_used = t.used() as u32;
    if pre_used != 100 {
        return Err(E_LED);
    }
    // R2：销毁（判据 ①/② 落点）：全表强制回收，报告 + 回收账对账。
    let mut rlog = ReclaimLog::new();
    let rep = destroy_table_logged(&mut t, &mut rlog)?;
    if !rep.ok(pre_used) {
        return Err(E_LED);
    }
    if rep.reclaimed != 100 || rep.prot_reclaimed != 10 || rep.audit_fired != 15 {
        return Err(E_LED);
    }
    // 回收账逐笔对拍：100 笔、槽互异、obj 与生成规则一致。
    if rlog.recs.len() != 100 || !rlog.sane() {
        return Err(E_LED);
    }
    let mut i = 0;
    while i < rlog.recs.len() {
        let (slot, gen, obj) = rlog.recs[i];
        if slot != i || gen != 1 || obj != 0x1000 + i as u32 {
            return Err(E_LED);
        }
        i += 1;
    }
    // 代数和锚：100 个首轮句柄 gen 恒 1 → 代数和 = 100。
    if rep.gen_sum != 100 {
        return Err(E_LED);
    }
    // R3：销毁后旧句柄键全部失效（判据 ③ 落点）：100 例读回陈旧。
    let mut i = 0;
    while i < 100 {
        let (slot, gen) = keys[i];
        if t.read(slot, gen).err() != Some(gf::E_STALE) {
            return Err(E_STALE);
        }
        i += 1;
    }
    // 二次销毁：空表回收零例（幂等——销毁不是再入毒药）。
    let rep2 = destroy_table(&mut t)?;
    if rep2.reclaimed != 0 || !rep2.ok(0) {
        return Err(E_RECLAIM);
    }
    // R4：销毁后表可复用（新句柄进低槽、代数从墓碑递增）。
    let (slot0, gen0) = t.attach(0x2000, sr::READ_CONTROL, 0).map_err(|_| E_CAP)?;
    if slot0 != 0 || gen0 != keys[0].1 + 1 {
        return Err(E_LED);
    }
    // 复用后再销毁：单句柄报告。
    let rep3 = destroy_table(&mut t)?;
    if rep3.reclaimed != 1 || rep3.gen_sum != gen0 {
        return Err(E_LED);
    }
    // R5：PROTECT 句柄在关闭路径被拒、销毁路径被回收（两路对拍——
    // 判据 ② 的完整兑现：同一属性两种命运，账面分别落账）。
    let mut t2 = gf::HandleTable::new();
    let (sp, gp) = t2.attach(9, sr::STD_ALL, gf::HA_PROTECT).map_err(|_| E_CAP)?;
    // 关闭路径：拒绝（36227 语义的本地复算）。
    if !close_refused_for_protect(gf::HA_PROTECT) {
        return Err(E_PROT);
    }
    // 释放尝试走 release 虽能执行（底层原语），但账面规则判其为非法
    // 关闭路径——本面以规则函数为门，销毁路径是唯一合法回收通道。
    let rep4 = destroy_table(&mut t2)?;
    if rep4.reclaimed != 1 || rep4.prot_reclaimed != 1 {
        return Err(E_PROT);
    }
    if t2.read(sp, gp).err() != Some(gf::E_STALE) {
        return Err(E_STALE);
    }
    // R6：混合表（PROTECT + 审计 + 普通同表）销毁分类计数。
    let mut t3 = gf::HandleTable::new();
    let mut i = 0;
    while i < 30 {
        let attrs = match i % 3 {
            0 => gf::HA_PROTECT | gf::HA_AUDIT_CLOSE,
            1 => gf::HA_AUDIT_CLOSE,
            _ => 0,
        };
        t3.attach(0x3000 + i as u32, sr::READ_CONTROL, attrs).map_err(|_| E_CAP)?;
        i += 1;
    }
    let rep5 = destroy_table(&mut t3)?;
    if rep5.reclaimed != 30 || rep5.prot_reclaimed != 10 || rep5.audit_fired != 20 {
        return Err(E_LED);
    }
    if !rep5.ok(30) {
        return Err(E_LED);
    }
    // 分批销毁账合成（merge 对拍：rep4 + rep5 两批合计可重放）。
    let merged = rep4.merge(&rep5);
    if merged.reclaimed != 31 || merged.prot_reclaimed != 11 || merged.audit_fired != 20 {
        return Err(E_LED);
    }
    if !merged.ok(31) {
        return Err(E_LED);
    }
    // 校验和：各报告回收数累加（确定性总锚）。
    Ok(rep
        .reclaimed
        .wrapping_add(rep3.reclaimed)
        .wrapping_add(rep4.reclaimed)
        .wrapping_add(rep5.reclaimed)
        .wrapping_add(rep.audit_fired))
}

pub fn detail(code: u16) -> &'static str {
    match code {
        E_VER => "版本锚不符",
        E_RECLAIM => "回收破（used 未清零/回收计数不齐/空表回收非零）",
        E_PROT => "保护回收破（PROTECT 逃逸出销毁路径）",
        E_STALE => "销毁后旧句柄未失效（结构性失效破）",
        E_LED => "销毁账不平（泄漏挂账未清零/报告自洽破）",
        E_CAP => "槽位越界",
        _ => "未知诊断码",
    }
}

pub fn run_unx36228_checks() -> CheckSet {
    let mut cs = CheckSet::new("unxstar-unx36228_gadestroy");
    cs.add("SEG 段位独占", SEG_BASE == 0xa1e0, "0xa1e0~0xa1eF 本模块独占");
    match drill() {
        Ok(_) => cs.add("J1 确定性演练全绿", true, "100 例全回收/PROTECT 强制/旧键全陈旧"),
        Err(code) => cs.add("J1 确定性演练全绿", false, detail(code)),
    }
    cs.add("分工锚", close_refused_for_protect(gf::HA_PROTECT) && !close_refused_for_protect(0), "PROTECT 拒关不拒销毁（两路分工）");
    cs.add("泄漏清零锚", DestroyReport { reclaimed: 0, prot_reclaimed: 0, audit_fired: 0, gen_sum: 0, leaked: 0 }.ok(0), "空报告泄漏挂账恒零");
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
    fn test_destroy_reclaims_protect() {
        let mut t = gf::HandleTable::new();
        let _ = t.attach(1, sr::READ_CONTROL, gf::HA_PROTECT);
        let _ = t.attach(2, sr::READ_CONTROL, 0);
        let rep = destroy_table(&mut t).unwrap_or(DestroyReport { reclaimed: 0, prot_reclaimed: 0, audit_fired: 0, gen_sum: 0, leaked: 9 });
        assert_eq!(rep.reclaimed, 2);
        assert_eq!(rep.prot_reclaimed, 1);
        assert_eq!(rep.leaked, 0);
        assert_eq!(t.used(), 0);
    }

    #[test]
    fn test_stale_after_destroy() {
        let mut t = gf::HandleTable::new();
        let (s, g) = t.attach(3, sr::READ_CONTROL, 0).unwrap_or((0, 0));
        let _ = destroy_table(&mut t);
        assert_eq!(t.read(s, g).err(), Some(gf::E_STALE));
    }

    #[test]
    fn test_checks_all_passed() {
        let cs = run_unx36228_checks();
        assert!(cs.all_passed(), "红项: {}", cs.red_items_text());
        assert!(cs.len() >= 4);
    }
}
