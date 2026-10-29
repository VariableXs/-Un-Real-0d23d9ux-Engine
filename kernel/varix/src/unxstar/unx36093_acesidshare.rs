//! UNX-F36093-J1 ACE 与 SID 共享引用（本地池化建模）。
//! 判据：① 本地 SidRef（sid 槽位+引用计数）池化：12 例 acquire / 12 例 release
//! 对称守恒（Σrefcnt == acquire_n − release_n），复用计数语义（SoleOwner/Freed）；
//! ② NIL 槽 1 例、未知槽 2 例、满池 1 例、双重释放 1 例全部显性拒绝；
//! ③ 纯功能代码 ≥300 行。
//! 设计纪律：自含模块——不 import unx36001 系列（SID 真源在 B01），
//! 本模块只建模「ACE 侧引用池」；指纹与哈希消费 unx36081_acecore。

use crate::checks::CheckSet;
use super::unx36081_acecore as core;
use alloc::vec;
use alloc::vec::Vec;
use alloc::string::{String, ToString};

pub const SEG_BASE: u16 = 0x3850;

// ---- 诊断码闭集 -----------------------------------------------------------
pub const E_VER: u16 = SEG_BASE + 1;   // 版本锚不符
pub const E_NIL: u16 = SEG_BASE + 2;   // NIL 槽（0 号保留）不可引用
pub const E_RANGE: u16 = SEG_BASE + 3; // 未知槽位（池中无此 SID）
pub const E_FULL: u16 = SEG_BASE + 4;  // 池满
pub const E_CONS: u16 = SEG_BASE + 5;  // 引用守恒破坏
pub const E_CS: u16 = SEG_BASE + 6;    // 指纹不稳

/// 版本锚。
pub const VER: u32 = 1;

/// SID 槽位空值锚（与 36082/36083/36084 同锚）。
pub const SID_NIL: u16 = 0;
/// 池容量。
pub const POOL_CAP: usize = 16;

/// 共享引用项：sid 槽位 + 引用计数。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct SidRef {
    pub sid_slot: u16,
    pub refcnt: u32,
}

const NONE_REF: Option<SidRef> = None;

/// SID 引用池。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct SidPool {
    pub slots: [Option<SidRef>; POOL_CAP],
    pub acquire_n: u64,
    pub release_n: u64,
}

impl SidPool {
    /// 空池。
    pub const fn fresh() -> SidPool {
        SidPool { slots: [NONE_REF; POOL_CAP], acquire_n: 0, release_n: 0 }
    }

    /// 活引用数（占用槽位）。
    pub fn live(&self) -> usize {
        let mut n = 0;
        let mut i = 0;
        while i < POOL_CAP {
            if self.slots[i].is_some() {
                n += 1;
            }
            i += 1;
        }
        n
    }

    /// Σrefcnt（占用槽计数和）。
    pub fn refcnt_sum(&self) -> u64 {
        let mut s: u64 = 0;
        let mut i = 0;
        while i < POOL_CAP {
            if let Some(r) = self.slots[i] {
                s += r.refcnt as u64;
            }
            i += 1;
        }
        s
    }

    /// 引用守恒式：Σrefcnt == acquire_n − release_n（对称轴）。
    pub fn check_conservation(&self) -> Result<(), u16> {
        if self.release_n > self.acquire_n {
            return Err(E_CONS);
        }
        if self.refcnt_sum() != self.acquire_n - self.release_n {
            return Err(E_CONS);
        }
        Ok(())
    }

    /// acquire：已有槽位 → 计数 +1；无则占新槽（计数 1）；NIL/池满显性拒。
    pub fn acquire(&mut self, sid_slot: u16) -> Result<u32, u16> {
        if sid_slot == SID_NIL {
            return Err(E_NIL);
        }
        let mut i = 0;
        while i < POOL_CAP {
            if let Some(r) = self.slots[i] {
                if r.sid_slot == sid_slot {
                    let newc = r.refcnt + 1;
                    self.slots[i] = Some(SidRef { sid_slot, refcnt: newc });
                    self.acquire_n += 1;
                    return Ok(newc);
                }
            }
            i += 1;
        }
        let mut j = 0;
        while j < POOL_CAP {
            if self.slots[j].is_none() {
                self.slots[j] = Some(SidRef { sid_slot, refcnt: 1 });
                self.acquire_n += 1;
                return Ok(1);
            }
            j += 1;
        }
        Err(E_FULL)
    }

    /// release：计数 −1，归零清槽；未知槽显性拒。
    pub fn release(&mut self, sid_slot: u16) -> Result<u32, u16> {
        if sid_slot == SID_NIL {
            return Err(E_NIL);
        }
        let mut i = 0;
        while i < POOL_CAP {
            if let Some(r) = self.slots[i] {
                if r.sid_slot == sid_slot {
                    if r.refcnt == 0 {
                        return Err(E_CONS);
                    }
                    let newc = r.refcnt - 1;
                    if newc == 0 {
                        self.slots[i] = NONE_REF;
                    } else {
                        self.slots[i] = Some(SidRef { sid_slot, refcnt: newc });
                    }
                    self.release_n += 1;
                    return Ok(newc);
                }
            }
            i += 1;
        }
        Err(E_RANGE)
    }

    /// 查询引用计数（只读）。
    pub fn refcnt_of(&self, sid_slot: u16) -> Result<u32, u16> {
        let mut i = 0;
        while i < POOL_CAP {
            if let Some(r) = self.slots[i] {
                if r.sid_slot == sid_slot {
                    return Ok(r.refcnt);
                }
            }
            i += 1;
        }
        Err(E_RANGE)
    }
}

/// 引用判定类别（#[repr(u16)] 显式判别式）。
#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ShareVerdict {
    /// 多方共享（refcnt ≥ 2）。
    Shared    = 0x0001,
    /// 独占（refcnt == 1）。
    SoleOwner = 0x0002,
    /// 已释放（refcnt == 0 且槽已清）。
    Freed     = 0x0003,
}

impl ShareVerdict {
    pub fn code(self) -> u16 {
        self as u16
    }

    /// 由计数归类。
    pub fn classify(refcnt: u32) -> ShareVerdict {
        if refcnt >= 2 {
            ShareVerdict::Shared
        } else if refcnt == 1 {
            ShareVerdict::SoleOwner
        } else {
            ShareVerdict::Freed
        }
    }
}

/// 池指纹（真源 FNV over 计数账 + 占用位图 + 版本锚）。
pub fn pool_fingerprint(p: &SidPool) -> u64 {
    let mut feed = Vec::new();
    feed.extend_from_slice(&VER.to_le_bytes());
    feed.extend_from_slice(&p.acquire_n.to_le_bytes());
    feed.extend_from_slice(&p.release_n.to_le_bytes());
    let mut i = 0;
    while i < POOL_CAP {
        match p.slots[i] {
            Some(r) => {
                feed.extend_from_slice(&r.sid_slot.to_le_bytes());
                feed.extend_from_slice(&r.refcnt.to_le_bytes());
            }
            None => feed.push(0xff),
        }
        i += 1;
    }
    core::fnv1a(&feed)
}

/// 摘要行（String 面，无 IO）。
pub fn summary(p: &SidPool) -> String {
    let mut s = String::new();
    s.push_str("sidpool live=");
    s.push_str(&p.live().to_string());
    s.push_str(" refs=");
    s.push_str(&p.refcnt_sum().to_string());
    s
}

/// 诊断码闭集互异自检。
pub fn codes_distinct() -> bool {
    let cs = [E_VER, E_NIL, E_RANGE, E_FULL, E_CONS, E_CS];
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
        E_NIL => "NIL 槽（0 号保留）不可引用",
        E_RANGE => "未知槽位（池中无此 SID）",
        E_FULL => "池满",
        E_CONS => "引用守恒破坏",
        E_CS => "指纹不稳",
        _ => "未知诊断码",
    }
}

/// 演练：R1 空池守恒；R2 acquire 12 例；R3 release 12 例对称归零；
/// R4 NIL 槽 1 例；R5 未知槽 2 例；R6 满池 1 例；R7 双重释放 1 例；
/// R8 复用计数语义 2 例；R9 守恒式；R10 指纹稳定。
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;
    // R1：空池守恒。
    if VER != 1 {
        return Err(E_VER);
    }
    let mut pool = SidPool::fresh();
    pool.check_conservation()?;
    hits += 1;
    // R2：acquire 12 例（槽 1..12 各一次，计数全 1）。
    let mut a = 0;
    while a < 12 {
        let slot = (a + 1) as u16;
        let c = pool.acquire(slot)?;
        if c != 1 || pool.refcnt_of(slot)? != 1 {
            return Err(E_CONS);
        }
        if ShareVerdict::classify(c).code() != ShareVerdict::SoleOwner.code() {
            return Err(E_CONS);
        }
        hits += 1;
        a += 1;
    }
    // R3：release 12 例对称归零（逆序释放）。
    let mut r = 12;
    while r > 0 {
        let slot = r as u16;
        let c = pool.release(slot)?;
        if c != 0 {
            return Err(E_CONS);
        }
        if ShareVerdict::classify(c).code() != ShareVerdict::Freed.code() {
            return Err(E_CONS);
        }
        hits += 1;
        r -= 1;
    }
    if pool.live() != 0 || pool.acquire_n != 12 || pool.release_n != 12 {
        return Err(E_CONS);
    }
    pool.check_conservation()?;
    // R4：NIL 槽 1 例——E_NIL。
    match pool.acquire(SID_NIL) {
        Ok(_) => return Err(E_NIL),
        Err(c) => {
            if c != E_NIL {
                return Err(E_NIL);
            }
        }
    }
    hits += 1;
    // R5：未知槽 2 例（99 / POOL_CAP+1）——release 报 E_RANGE。
    let unknowns: Vec<u16> = vec![99, (POOL_CAP as u16) + 1];
    let mut u = 0;
    while u < unknowns.len() {
        match pool.release(unknowns[u]) {
            Ok(_) => return Err(E_RANGE),
            Err(c) => {
                if c != E_RANGE {
                    return Err(E_RANGE);
                }
            }
        }
        hits += 1;
        u += 1;
    }
    // R6：满池 1 例（fresh 池灌 16 槽再 acquire 第 17 槽 → E_FULL）。
    let mut full = SidPool::fresh();
    let mut f = 0;
    while f < POOL_CAP {
        full.acquire((f + 1) as u16)?;
        f += 1;
    }
    match full.acquire((POOL_CAP as u16) + 1) {
        Ok(_) => return Err(E_FULL),
        Err(c) => {
            if c != E_FULL {
                return Err(E_FULL);
            }
        }
    }
    full.check_conservation()?;
    hits += 1;
    // R7：双重释放 1 例（已归零槽再 release → E_RANGE）。
    match pool.release(5) {
        Ok(_) => return Err(E_RANGE),
        Err(c) => {
            if c != E_RANGE {
                return Err(E_RANGE);
            }
        }
    }
    hits += 1;
    // R8：复用计数语义 2 例（双 acquire → Shared；单 release → SoleOwner）。
    let c1 = pool.acquire(3)?;
    if c1 != 1 {
        return Err(E_CONS);
    }
    let c2 = pool.acquire(3)?;
    if c2 != 2 || ShareVerdict::classify(c2).code() != ShareVerdict::Shared.code() {
        return Err(E_CONS);
    }
    hits += 1;
    let c3 = pool.release(3)?;
    if c3 != 1 || ShareVerdict::classify(c3).code() != ShareVerdict::SoleOwner.code() {
        return Err(E_CONS);
    }
    hits += 1;
    // R9：守恒式（acquire=14 / release=13 → Σrefcnt=1）。
    if pool.acquire_n != 14 || pool.release_n != 13 {
        return Err(E_CONS);
    }
    pool.check_conservation()?;
    if pool.refcnt_sum() != 1 {
        return Err(E_CONS);
    }
    hits += 1;
    // R10：指纹两次求值稳定且非零。
    let p1 = pool_fingerprint(&pool);
    let p2 = pool_fingerprint(&pool);
    if p1 != p2 || p1 == 0 {
        return Err(E_CS);
    }
    hits += 1;
    Ok(hits)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn drill_green() {
        assert_eq!(drill(), Ok(34));
    }

    #[test]
    fn nil_slot_rejected() {
        let mut p = SidPool::fresh();
        assert_eq!(p.acquire(SID_NIL), Err(E_NIL));
        assert_eq!(p.release(SID_NIL), Err(E_NIL));
    }

    #[test]
    fn unknown_slot_rejected() {
        let mut p = SidPool::fresh();
        assert_eq!(p.release(7), Err(E_RANGE));
        assert_eq!(p.refcnt_of(7), Err(E_RANGE));
    }

    #[test]
    fn conservation_symmetry() {
        let mut p = SidPool::fresh();
        let _ = p.acquire(2);
        let _ = p.acquire(2);
        let _ = p.release(2);
        assert_eq!(p.check_conservation(), Ok(()));
        assert!(summary(&p).contains("sidpool"));
    }
}

pub fn run_unx36093_checks() -> CheckSet {
    let mut cs = CheckSet::new("unxstar-unx36093_acesidshare");
    cs.add("SEG 段位独占", SEG_BASE == 0x3850, "0x3850~0x385f 本模块独占");
    match drill() {
        Ok(_) => cs.add("确定性演练全绿", true, "acquire/release 12+12 对称守恒、负例 5 例全拒"),
        Err(code) => cs.add("确定性演练全绿", false, detail(code)),
    }
    cs.add("池容量锚 16", POOL_CAP == 16, "SID 引用池容量漂移");
    cs.add("版本锚 VER=1", VER == 1, "版本锚不符");
    cs.add("诊断码闭集互异", codes_distinct(), "诊断码闭集出现重码");
    cs.add("共享判定三态锚", ShareVerdict::Shared.code() == 1 && ShareVerdict::Freed.code() == 3, "共享判定类别漂移");
    cs
}
