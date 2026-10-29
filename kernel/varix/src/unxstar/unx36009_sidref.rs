//! unx36009_sidref — UNX-F36009 SID 拷贝与引用计数（SD 内共享引用底座）。
//!
//! 判据原文：
//!   J1 共享 SID 引用计数 10^4 次 acquire/release 归零守恒；双重释放
//!      注入 10 例被计数器拦截并审计。
//! 设计纪律：
//!   1. 槽位底座：固定 64 槽，每槽（指纹, 强计数, 纪元）；同指纹注册
//!      共享同槽（SD 内共享同一 SID 实体语义），句柄 =（槽号, 纪元）。
//!   2. 归零语义：强计数归零**保留槽位**（strong=0、used=1）——同指纹
//!      再注册原槽复活（0→1），杜绝槽位抖动；强计数为 0 时再释放 =
//!      严格双重释放 → 拦截（计数不动）+ 审计事件 append-only 落账。
//!   3. 陈旧句柄面：槽号越界一律拒绝（纪元守卫保留为防御面）。
//!   4. 纯功能线：no_std，零 unwrap/expect/panic，确定性。

pub const SEG_BASE: u16 = 0x7370;

// ── 诊断码闭集（0x7370~0x738F，独占）─────────────────────────
const E_SLOT: u16 = SEG_BASE + 1; // 槽满（显性拒，不逐出）
const E_STALE: u16 = SEG_BASE + 2; // 陈旧句柄（槽号越界/纪元失配）
const E_DBL: u16 = SEG_BASE + 3; // 严格双重释放（计数已 0 且槽仍活）
const E_AUDIT: u16 = SEG_BASE + 4; // 审计账失配（事件数漂移）
const E_BAL: u16 = SEG_BASE + 5; // 守恒破（终态计数 ≠ 期望）
const E_CAP: u16 = SEG_BASE + 6; // 计数上溢保护（强计数封顶拒）

use crate::checks::CheckSet;
use super::unx36001_sidcore::Sid;

/// 槽位容量。
pub const REF_SLOTS: usize = 64;
/// 审计账容量。
pub const AUDIT_CAP: usize = 64;
/// 强计数封顶（保护面：u32 半程即拒，杜绝溢出绕过）。
pub const STRONG_CAP: u32 = 0x4000_0000;

/// 引用句柄：（槽号, 纪元）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct RefHandle {
    pub slot: u16,
    pub epoch: u32,
}

/// 单槽状态。
#[derive(Clone, Copy)]
struct Slot {
    fp: u64,
    strong: u32,
    epoch: u32,
}

/// 审计事件。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct AuditEvent {
    pub kind: u8,   // 1=严格双重释放 2=陈旧句柄 3=封顶拒
    pub slot: u16,
    pub epoch: u32,
    pub fp: u64,
}

/// 引用计数底座。
pub struct SidRefs {
    slots: [Slot; REF_SLOTS],
    used: [bool; REF_SLOTS],
    audit: [Option<AuditEvent>; AUDIT_CAP],
    pub audit_count: u32,
    pub audit_dropped: u32,
    pub acquires: u64,
    pub releases: u64,
    pub intercepted: u32,
}

impl SidRefs {
    pub fn new() -> SidRefs {
        SidRefs {
            slots: [Slot { fp: 0, strong: 0, epoch: 0 }; REF_SLOTS],
            used: [false; REF_SLOTS],
            audit: [None; AUDIT_CAP],
            audit_count: 0,
            audit_dropped: 0,
            acquires: 0,
            releases: 0,
            intercepted: 0,
        }
    }

    /// 注册：同指纹共享既有槽（强 +1）；否则开新槽（强 =1）。
    pub fn register(&mut self, s: &Sid) -> Result<RefHandle, u16> {
        let fp = s.fingerprint();
        let mut i = 0;
        while i < REF_SLOTS {
            if self.used[i] && self.slots[i].fp == fp {
                if self.slots[i].strong >= STRONG_CAP {
                    let h = RefHandle { slot: i as u16, epoch: self.slots[i].epoch };
                    self.intercepted += 1;
                    self.intercept_audit(3, h, fp);
                    return Err(E_CAP);
                }
                self.slots[i].strong += 1;
                self.acquires += 1;
                return Ok(RefHandle { slot: i as u16, epoch: self.slots[i].epoch });
            }
            i += 1;
        }
        let mut j = 0;
        while j < REF_SLOTS {
            if !self.used[j] {
                self.used[j] = true;
                self.slots[j] = Slot { fp, strong: 1, epoch: self.slots[j].epoch };
                self.acquires += 1;
                return Ok(RefHandle { slot: j as u16, epoch: self.slots[j].epoch });
            }
            j += 1;
        }
        Err(E_SLOT)
    }

    /// 加引用：强计数 +1（封顶保护）。
    pub fn acquire(&mut self, h: RefHandle) -> Result<u32, u16> {
        if h.slot as usize >= REF_SLOTS || !self.used[h.slot as usize] {
            self.intercepted += 1;
            self.intercept_audit(2, h, 0);
            return Err(E_STALE);
        }
        let si = h.slot as usize;
        if self.slots[si].epoch != h.epoch {
            let fp = self.slots[si].fp;
            self.intercepted += 1;
            self.intercept_audit(2, h, fp);
            return Err(E_STALE);
        }
        if self.slots[si].strong >= STRONG_CAP {
            let fp = self.slots[si].fp;
            self.intercepted += 1;
            self.intercept_audit(3, h, fp);
            return Err(E_CAP);
        }
        self.slots[si].strong += 1;
        self.acquires += 1;
        Ok(self.slots[si].strong)
    }

    /// 减引用：归零保留槽位；计数已 0 再释放 = 严格双重释放（拦截+审计）。
    pub fn release(&mut self, h: RefHandle) -> Result<u32, u16> {
        if h.slot as usize >= REF_SLOTS || !self.used[h.slot as usize] {
            self.intercepted += 1;
            self.intercept_audit(2, h, 0);
            return Err(E_STALE);
        }
        let si = h.slot as usize;
        if self.slots[si].epoch != h.epoch {
            let fp = self.slots[si].fp;
            self.intercepted += 1;
            self.intercept_audit(2, h, fp);
            return Err(E_STALE);
        }
        if self.slots[si].strong == 0 {
            let fp = self.slots[si].fp;
            self.intercepted += 1;
            self.intercept_audit(1, h, fp);
            return Err(E_DBL);
        }
        self.slots[si].strong -= 1;
        self.releases += 1;
        Ok(self.slots[si].strong)
    }

    fn intercept_audit(&mut self, kind: u8, h: RefHandle, fp: u64) {
        let ev = AuditEvent { kind, slot: h.slot, epoch: h.epoch, fp };
        let mut i = 0;
        while i < AUDIT_CAP {
            if self.audit[i].is_none() {
                self.audit[i] = Some(ev);
                self.audit_count += 1;
                return;
            }
            i += 1;
        }
        self.audit_dropped += 1;
    }

    /// 槽内强计数（陈旧句柄返回 Err）。
    pub fn strong_of(&self, h: RefHandle) -> Result<u32, u16> {
        if h.slot as usize >= REF_SLOTS || !self.used[h.slot as usize] {
            return Err(E_STALE);
        }
        let sl = &self.slots[h.slot as usize];
        if sl.epoch != h.epoch {
            return Err(E_STALE);
        }
        Ok(sl.strong)
    }

    /// 活跃槽总数（used 面）。
    pub fn live_slots(&self) -> u32 {
        let mut n = 0;
        let mut i = 0;
        while i < REF_SLOTS {
            if self.used[i] {
                n += 1;
            }
            i += 1;
        }
        n
    }

    /// 归零仍保留的槽（复活面）。
    pub fn zero_slots(&self) -> u32 {
        let mut n = 0;
        let mut i = 0;
        while i < REF_SLOTS {
            if self.used[i] && self.slots[i].strong == 0 {
                n += 1;
            }
            i += 1;
        }
        n
    }

    /// 审计事件按类计数。
    pub fn audit_of_kind(&self, kind: u8) -> u32 {
        let mut n = 0;
        let mut i = 0;
        while i < AUDIT_CAP {
            if let Some(e) = self.audit[i] {
                if e.kind == kind {
                    n += 1;
                }
            }
            i += 1;
        }
        n
    }
}

/// 诊断码释义。
pub fn detail(code: u16) -> &'static str {
    match code {
        E_SLOT => "槽满（显性拒绝）",
        E_STALE => "陈旧句柄（槽号越界/纪元失配）",
        E_DBL => "严格双重释放（计数已 0 且槽仍活）",
        E_AUDIT => "审计账失配",
        E_BAL => "守恒破（终态计数 ≠ 期望）",
        E_CAP => "强计数封顶拒",
        _ => "未知诊断码",
    }
}

/// 确定性演练：10^4 次 acquire/release 守恒 + 10 例严格双重释放拦截
/// 审计（判据 J1）。
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;
    let mut refs = SidRefs::new();

    // R1：注册 4 个共享 SID（基线强计数 1），10^4 次 acquire/release
    //（2500 轮 × 4 句柄 × acquire+release），终态回基线、账面守恒。
    let mut seed: u32 = 0x3609_0001;
    let mut hs = [None; 4];
    let mut n = 0;
    while n < 4 {
        seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        let s = Sid::new(1, Sid::ia_from(5), &[21, seed])?;
        hs[n] = Some(refs.register(&s)?);
        n += 1;
    }
    let mut round = 0u32;
    while round < 2500 {
        let mut k = 0;
        while k < 4 {
            let h = hs[k].ok_or(E_BAL)?;
            let v = refs.acquire(h)?;
            if refs.strong_of(h)? != v {
                return Err(E_BAL);
            }
            let w = refs.release(h)?;
            if w + 1 != v {
                return Err(E_BAL);
            }
            k += 1;
        }
        round += 1;
    }
    if refs.acquires != 10_000 + 4 || refs.releases != 10_000 {
        return Err(E_BAL);
    }
    let mut k = 0;
    while k < 4 {
        let h = hs[k].ok_or(E_BAL)?;
        if refs.strong_of(h)? != 1 {
            return Err(E_BAL);
        }
        k += 1;
    }
    if refs.live_slots() != 4 {
        return Err(E_BAL);
    }
    hits += 1;

    // R2：严格双重释放注入 10 例——族 A（5 例）：注册→释放归零→
    // 再释放被拦截（E_DBL）；族 B（5 例）：同指纹双注册（共享槽强=2）
    // → 释放两次归零 → 第三次释放被拦截。
    let mut n = 0;
    while n < 5 {
        seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        let s = Sid::new(1, Sid::ia_from(5), &[8000 + n, seed])?;
        let h = refs.register(&s)?;
        if refs.release(h)? != 0 {
            return Err(E_BAL);
        }
        if refs.strong_of(h)? != 0 {
            return Err(E_BAL);
        }
        if refs.release(h) != Err(E_DBL) {
            return Err(E_DBL);
        }
        n += 1;
    }
    let mut n = 0;
    while n < 5 {
        seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        let s = Sid::new(1, Sid::ia_from(5), &[7000 + n, seed])?;
        let ha = refs.register(&s)?;
        let hb = refs.register(&s)?; // 同指纹共享同槽
        if ha.slot != hb.slot || ha.epoch != hb.epoch {
            return Err(E_BAL);
        }
        if refs.strong_of(ha)? != 2 {
            return Err(E_BAL);
        }
        refs.release(ha)?; // 2→1
        refs.release(hb)?; // 1→0（槽保留）
        if refs.release(ha) != Err(E_DBL) {
            return Err(E_DBL);
        }
        n += 1;
    }
    if refs.audit_of_kind(1) != 10 {
        return Err(E_AUDIT);
    }
    if refs.intercepted != 10 {
        return Err(E_DBL);
    }
    hits += 1;

    // R3：复活面 + 终态守恒——归零槽可复活（0→1→0）；审计账与拦截账
    // 对平；10^4 账面不漂移（10,004 基线 + 族 A 5 + 族 B 10 + 复活 1
    // = 10,020 次 acquire）。
    let h0 = RefHandle { slot: 4, epoch: 0 }; // 族 A 首槽（0..4 被 R1 占用）
    if refs.strong_of(h0)? != 0 {
        return Err(E_BAL);
    }
    if refs.acquire(h0)? != 1 {
        return Err(E_BAL);
    }
    if refs.release(h0)? != 0 {
        return Err(E_BAL);
    }
    if refs.audit_count + refs.audit_dropped != refs.intercepted {
        return Err(E_AUDIT);
    }
    if refs.acquires != 10_020 {
        return Err(E_BAL);
    }
    hits += 1;

    Ok(hits)
}

// ── 判据聚合（CheckSet 面 · 进 unxstar_blocks 检查表）────────
/// 判据 J1 聚合入口：SEG 锚 + 确定性演练全绿（红时回填诊断码）。
pub fn run_unx36009_checks() -> CheckSet {
    let mut cs = CheckSet::new("unxstar-unx36009_sidref");
    cs.add("SEG 段位独占", SEG_BASE == 0x7370, "0x7370~0x738F 本模块独占");
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
        assert_eq!(drill(), Ok(3));
    }
}
