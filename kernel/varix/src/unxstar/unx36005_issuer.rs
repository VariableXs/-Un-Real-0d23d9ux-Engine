//! unx36005_issuer — UNX-F36005 SID 发射器：本域前缀 + RID 偏移约定
//! （联 POSIX uid 映射锚）。
//!
//! 判据原文：
//!   J1 uid→SID 发射 100 例与偏移约定公式零差；重复 uid 发射返回既有
//!      SID（幂等断言）；越界 uid 注入 10 例拒绝。
//! 设计纪律：
//!   1. 域锚：本域 SID 前缀恒为 S-1-5-21-<a>-<b>-<c>（三个 32 位域子权
//!      固定常量，会话内不变）；RID = RID_BASE + uid（RID_BASE=1000，
//!      Windows 账户族首 RID 约定）。
//!   2. 幂等面：同 uid 重复发射必须还原同 RID（发射表去重留痕），绝不
//!      产出第二个 SID——身份发射是纯函数语义，账本只是命中账。
//!   3. 越界面：uid > UID_MAX 构造期拒绝，越界请求入拒绝账。
//!   4. 纯功能线：no_std，零 unwrap/expect/panic，确定性。

pub const SEG_BASE: u16 = 0x72F0;

// ── 诊断码闭集（0x72F0~0x730F，独占）─────────────────────────
const E_RANGE: u16 = SEG_BASE + 1; // uid 越界（> UID_MAX）
const E_CAP: u16 = SEG_BASE + 2; // 发射表满（显性拒，不静默覆盖）
const E_ANCHOR: u16 = SEG_BASE + 3; // 偏移公式失配（RID ≠ BASE + uid）
const E_IDEM: u16 = SEG_BASE + 4; // 幂等失配（同 uid 双发射 RID 漂移）
const E_BAL: u16 = SEG_BASE + 5; // 账目守恒破（新发射+幂等+拒绝≠总请求）
const E_DOM: u16 = SEG_BASE + 6; // 域锚失配（前缀子权漂移）

use crate::checks::CheckSet;
use super::unx36001_sidcore::{Sid, MAX_SUBS};

/// RID 基线（Windows 账户族首 RID）。
pub const RID_BASE: u32 = 1000;
/// uid 上限（RID ≤ RID_BASE + UID_MAX，32 位内恒不溢出）。
pub const UID_MAX: u32 = 0x000F_FFFF;
/// 发射表容量（去重账）。
pub const ISSUER_CAP: usize = 256;

/// 域锚三子权（S-1-5-21-<a>-<b>-<c>，会话内恒定）。
pub const DOMAIN_SUBS: [u32; 3] = [3_623_811_015, 3_361_044_348, 3_030_082_020];

/// 发射器：uid → RID 偏移账 + 去重账。
pub struct SidIssuer {
    /// 已发射 uid 表（线性去重账）。
    seen: [Option<u32>; ISSUER_CAP],
    pub emitted: u32,    // 新发射账
    pub idem_hits: u32,  // 幂等命中账（重复 uid）
    pub rejected: u32,   // 越界拒绝账
    pub req_total: u32,  // 总请求账
}

impl SidIssuer {
    pub fn new() -> SidIssuer {
        SidIssuer {
            seen: [None; ISSUER_CAP],
            emitted: 0,
            idem_hits: 0,
            rejected: 0,
            req_total: 0,
        }
    }

    /// 发射：uid → 域 SID。幂等：同 uid 还原同 RID。
    pub fn emit(&mut self, uid: u32) -> Result<Sid, u16> {
        self.req_total += 1;
        if uid > UID_MAX {
            self.rejected += 1;
            return Err(E_RANGE);
        }
        let rid = RID_BASE.wrapping_add(uid);
        // 去重账扫描：命中即幂等返回。
        let mut i = 0;
        while i < ISSUER_CAP {
            if self.seen[i] == Some(uid) {
                self.idem_hits += 1;
                return self.build_domain_sid(uid, rid);
            }
            i += 1;
        }
        // 未命中：登记后新发射。
        let mut slot = ISSUER_CAP;
        let mut j = 0;
        while j < ISSUER_CAP {
            if self.seen[j].is_none() {
                slot = j;
                break;
            }
            j += 1;
        }
        if slot == ISSUER_CAP {
            self.rejected += 1;
            return Err(E_CAP);
        }
        self.seen[slot] = Some(uid);
        self.emitted += 1;
        self.build_domain_sid(uid, rid)
    }

    fn build_domain_sid(&self, uid: u32, rid: u32) -> Result<Sid, u16> {
        // 偏移公式锚：RID == RID_BASE + uid（判据 J1 公式面）。
        if rid != RID_BASE.wrapping_add(uid) {
            return Err(E_ANCHOR);
        }
        let mut subs = [0u32; MAX_SUBS];
        subs[0] = DOMAIN_SUBS[0];
        subs[1] = DOMAIN_SUBS[1];
        subs[2] = DOMAIN_SUBS[2];
        subs[3] = rid;
        Sid::new(1, Sid::ia_from(5), &subs[..4])
    }

    /// 守恒：新发射 + 幂等命中 + 拒绝 == 总请求。
    pub fn conserve(&self) -> bool {
        self.emitted + self.idem_hits + self.rejected == self.req_total
    }

    /// uid 是否已发射过（去重账直查）。
    pub fn is_issued(&self, uid: u32) -> bool {
        let mut i = 0;
        while i < ISSUER_CAP {
            if self.seen[i] == Some(uid) {
                return true;
            }
            i += 1;
        }
        false
    }
}

/// 偏移公式纯函数：uid → RID（无状态面，判据 J1 的 100 例公式锚）。
pub fn rid_of(uid: u32) -> Result<u32, u16> {
    if uid > UID_MAX {
        return Err(E_RANGE);
    }
    Ok(RID_BASE.wrapping_add(uid))
}

/// RID 全域边界（发射器可分配闭区间 [RID_BASE, RID_BASE+UID_MAX]）。
pub fn rid_span() -> (u32, u32) {
    (RID_BASE, RID_BASE.wrapping_add(UID_MAX))
}

/// RID 是否在发射器全域内。
pub fn rid_in_span(rid: u32) -> bool {
    let (lo, hi) = rid_span();
    rid >= lo && rid <= hi
}

/// 发射册轮廓：已发射 uid 的最小/最大/计数（域占用轮廓面）。
pub struct IssuerCensus {
    pub issued_count: u32,
    pub min_uid: Option<u32>,
    pub max_uid: Option<u32>,
    pub min_rid: u32,
    pub max_rid: u32,
}

impl SidIssuer {
    /// 轮廓导出：扫描去重账汇总（O(CAP) 一次过）。
    pub fn census(&self) -> IssuerCensus {
        let mut cnt = 0u32;
        let mut lo = u32::MAX;
        let mut hi = 0u32;
        let mut i = 0;
        while i < ISSUER_CAP {
            if let Some(uid) = self.seen[i] {
                cnt += 1;
                if uid < lo {
                    lo = uid;
                }
                if uid > hi {
                    hi = uid;
                }
            }
            i += 1;
        }
        let (min_uid, max_uid) = if cnt == 0 { (None, None) } else { (Some(lo), Some(hi)) };
        IssuerCensus {
            issued_count: cnt,
            min_uid,
            max_uid,
            min_rid: if cnt == 0 { 0 } else { RID_BASE.wrapping_add(lo) },
            max_rid: if cnt == 0 { 0 } else { RID_BASE.wrapping_add(hi) },
        }
    }

    /// 域锚指纹：三域子权 + RID 基线合成指纹（域锚漂移可检面）。
    pub fn domain_anchor_fp(&self) -> u64 {
        let mut h: u64 = 0x811C_9DC5;
        let mut i = 0;
        while i < 3 {
            h = (h ^ DOMAIN_SUBS[i] as u64).wrapping_mul(0x0100_0193);
            i += 1;
        }
        (h ^ RID_BASE as u64).wrapping_mul(0x0100_0193)
    }

    /// 去重账占用率（万分比，确定性整数面）。
    pub fn occupancy_bp(&self) -> u32 {
        self.emitted * 10_000 / (ISSUER_CAP as u32)
    }
}

/// 诊断码释义。
pub fn detail(code: u16) -> &'static str {
    match code {
        E_RANGE => "uid 越界（> UID_MAX）",
        E_CAP => "发射表满（显性拒绝）",
        E_ANCHOR => "偏移公式失配（RID ≠ RID_BASE + uid）",
        E_IDEM => "幂等失配（同 uid 双发射 RID 漂移）",
        E_BAL => "发射账守恒破",
        E_DOM => "域锚失配（前缀子权漂移）",
        _ => "未知诊断码",
    }
}

/// 确定性演练：100 例公式零差 + 幂等 + 10 例越界拒 + 守恒（判据 J1）。
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;
    let mut iss = SidIssuer::new();

    // R1：100 例发射公式零差——uid 由 LCG 铺 0..1000 与高段混合，
    // RID == RID_BASE + uid、域锚四子权逐位一致。
    let mut seed: u32 = 0x3605_0001;
    let mut n = 0usize;
    while n < 100 {
        seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        let uid = seed % 2000;
        let s = iss.emit(uid)?;
        if s.rid() != RID_BASE.wrapping_add(uid) {
            return Err(E_ANCHOR);
        }
        if s.ia_value() != 5 || s.subs()[0] != DOMAIN_SUBS[0]
            || s.subs()[1] != DOMAIN_SUBS[1] || s.subs()[2] != DOMAIN_SUBS[2] {
            return Err(E_DOM);
        }
        if rid_of(uid)? != s.rid() {
            return Err(E_ANCHOR);
        }
        n += 1;
    }
    hits += 1;

    // R2：幂等断言——同 uid 两次发射 RID 必还原，第二次必为幂等命中
    // （去重账命中），第二次发射零新发射账增长。
    let s1 = iss.emit(0)?;
    let after_first = iss.emitted;
    let idem_mid = iss.idem_hits;
    if !iss.is_issued(0) {
        return Err(E_IDEM);
    }
    let s2 = iss.emit(0)?;
    if s2.rid() != s1.rid() || s2.rid() != RID_BASE {
        return Err(E_IDEM);
    }
    if iss.idem_hits != idem_mid + 1 {
        return Err(E_IDEM);
    }
    if iss.emitted != after_first {
        return Err(E_IDEM);
    }
    hits += 1;

    // R3：越界 uid 注入 10 例——UID_MAX+1..+10 全拒且入拒绝账。
    let mut k = 1u32;
    while k <= 10 {
        let uid = UID_MAX.wrapping_add(k);
        if iss.emit(uid) != Err(E_RANGE) {
            return Err(E_RANGE);
        }
        if rid_of(uid).is_ok() {
            return Err(E_RANGE);
        }
        k += 1;
    }
    if iss.rejected < 10 {
        return Err(E_BAL);
    }
    hits += 1;

    // R4：账目守恒——新发射 + 幂等 + 拒绝 == 总请求。
    if !iss.conserve() {
        return Err(E_BAL);
    }
    hits += 1;

    // R5：轮廓面——发射册 census 与账面互证；域锚指纹双构造一致；
    // RID 全域边界与公式锚对平；占用率万分比在 [0,10000]。
    let cz = iss.census();
    if cz.issued_count != iss.emitted {
        return Err(E_BAL);
    }
    if cz.min_uid.is_none() || cz.max_uid.is_none() {
        return Err(E_BAL);
    }
    if cz.min_rid != RID_BASE.wrapping_add(cz.min_uid.unwrap_or(0))
        || cz.max_rid != RID_BASE.wrapping_add(cz.max_uid.unwrap_or(0))
    {
        return Err(E_ANCHOR);
    }
    let (rlo, rhi) = rid_span();
    if rlo != RID_BASE || rhi != RID_BASE.wrapping_add(UID_MAX) || !rid_in_span(rlo)
        || rid_in_span(rhi.wrapping_add(1))
    {
        return Err(E_ANCHOR);
    }
    let fp1 = iss.domain_anchor_fp();
    let fp2 = iss.domain_anchor_fp();
    if fp1 != fp2 || fp1 == 0 {
        return Err(E_DOM);
    }
    if iss.occupancy_bp() > 10_000 {
        return Err(E_BAL);
    }
    hits += 1;

    Ok(hits)
}

// ── 判据聚合（CheckSet 面 · 进 unxstar_blocks 检查表）────────
/// 判据 J1 聚合入口：SEG 锚 + 确定性演练全绿（红时回填诊断码）。
pub fn run_unx36005_checks() -> CheckSet {
    let mut cs = CheckSet::new("unxstar-unx36005_issuer");
    cs.add("SEG 段位独占", SEG_BASE == 0x72F0, "0x72F0~0x730F 本模块独占");
    match drill() {
        Ok(_) => cs.add("J1 确定性演练全绿", true, "演练段全对平、注入全拒"),
        Err(code) => cs.add("J1 确定性演练全绿", false, detail(code)),
    }
    cs
}

#[cfg(test)]
mod tests {
    use super::drill;

    #[test]
    fn drill_green() {
        assert_eq!(drill(), Ok(5));
    }
}
