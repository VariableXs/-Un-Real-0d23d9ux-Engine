//! UNX-F36070-J1 SD 的引用共享与写时复制（大对象场景）。
//! 判据：① 引用计数守恒 200 例（acquire/release 严格配对零泄漏）；
//! ② 写时复制语义（共享态写 → 私有副本，原共享体逐字节不动）100 例；
//! ③ 纯功能代码 ≥300 行。
//! 设计纪律：大对象（数 MB ACL 体）场景下 SD 按引用共享——读方共享、
//! 写方复制。共享体的字节面是「冻结快照」：任何写操作触发 COW，
//! 写者拿到私有副本，其余引用者看到的体不变（对拍面）。

use crate::checks::CheckSet;
use alloc::vec;
use alloc::vec::Vec;
use super::unx36061_sdcore::{self as core, Sd, E_CS, FNV_OFF};

pub const SEG_BASE: u16 = 0xe220;

// ---- 诊断码闭集 -----------------------------------------------------------
pub const E_REF: u16 = SEG_BASE + 1; // 引用计数失守（负数/泄漏/悬增）
pub const E_SHARED: u16 = SEG_BASE + 2; // 共享体被原位写（COW 红线）
pub const E_COW: u16 = SEG_BASE + 3; // 写时副本与源不一致（复制面破）
pub const E_CAP: u16 = SEG_BASE + 4; // 共享池容量满
pub const E_HANDLE: u16 = SEG_BASE + 5; // 句柄非法（未登记/已释放）

/// 共享体：冻结字节面 + FNV 指纹（写检测锚）。
pub struct SharedBody {
    pub bytes: Vec<u8>,
    pub frozen_cs: u64,
}

impl SharedBody {
    pub fn freeze(bytes: Vec<u8>) -> SharedBody {
        let mut h = FNV_OFF;
        core::fnv_push(&bytes, &mut h);
        SharedBody { bytes, frozen_cs: h }
    }
    /// 冻结完整性（任何原位写都会破指纹）。
    pub fn intact(&self) -> bool {
        let mut h = FNV_OFF;
        core::fnv_push(&self.bytes, &mut h);
        h == self.frozen_cs
    }
}

/// 共享池：定容 16 体，每体引用计数 + 句柄登记。
pub const POOL_CAP: usize = 16;
pub const REFCNT_CAP: u32 = 64;

pub struct SharedPool {
    bodies: [Option<SharedBody>; POOL_CAP],
    refcnt: [u32; POOL_CAP],
    used: u16,
}

const NONE_BODY: Option<SharedBody> = None;

impl SharedPool {
    pub fn new() -> SharedPool {
        SharedPool { bodies: [NONE_BODY; POOL_CAP], refcnt: [0; POOL_CAP], used: 0 }
    }

    /// 入池（冻结）→ 返回体号。
    pub fn publish(&mut self, bytes: Vec<u8>) -> Result<usize, u16> {
        let mut slot = POOL_CAP;
        let mut i = 0;
        while i < POOL_CAP {
            if self.used & (1u16 << i) == 0 {
                slot = i;
                break;
            }
            i += 1;
        }
        if slot == POOL_CAP {
            return Err(E_CAP);
        }
        self.bodies[slot] = Some(SharedBody::freeze(bytes));
        self.refcnt[slot] = 0;
        self.used |= 1u16 << slot;
        Ok(slot)
    }

    /// 引用获取（计数增；上限显性拒）。
    pub fn acquire(&mut self, slot: usize) -> Result<u32, u16> {
        if slot >= POOL_CAP || self.used & (1u16 << slot) == 0 {
            return Err(E_HANDLE);
        }
        if self.refcnt[slot] >= REFCNT_CAP {
            return Err(E_REF);
        }
        self.refcnt[slot] += 1;
        Ok(self.refcnt[slot])
    }

    /// 引用释放（计数减；零时体可回收——此处显性留冻结体待清扫）。
    pub fn release(&mut self, slot: usize) -> Result<u32, u16> {
        if slot >= POOL_CAP || self.used & (1u16 << slot) == 0 {
            return Err(E_HANDLE);
        }
        if self.refcnt[slot] == 0 {
            return Err(E_REF); // 悬放
        }
        self.refcnt[slot] -= 1;
        Ok(self.refcnt[slot])
    }

    /// 回收（计数为零才可回收；否则泄漏拒绝）。
    pub fn reclaim(&mut self, slot: usize) -> Result<(), u16> {
        if slot >= POOL_CAP || self.used & (1u16 << slot) == 0 {
            return Err(E_HANDLE);
        }
        if self.refcnt[slot] != 0 {
            return Err(E_REF);
        }
        self.bodies[slot] = None;
        self.used &= !(1u16 << slot);
        Ok(())
    }

    /// 只读视图（共享读路径——不复制）。
    pub fn view(&self, slot: usize) -> Result<&[u8], u16> {
        if slot >= POOL_CAP || self.used & (1u16 << slot) == 0 {
            return Err(E_HANDLE);
        }
        match &self.bodies[slot] {
            Some(b) => Ok(&b.bytes),
            None => Err(E_HANDLE),
        }
    }

    /// 冻结完整性全检（池内全部在场体）。
    pub fn all_intact(&self) -> Result<(), u16> {
        let mut i = 0;
        while i < POOL_CAP {
            if self.used & (1u16 << i) != 0 {
                match &self.bodies[i] {
                    Some(b) if !b.intact() => return Err(E_SHARED),
                    None => return Err(E_HANDLE),
                    _ => {}
                }
            }
            i += 1;
        }
        Ok(())
    }
}

/// 写时复制（COW）：refcnt > 1 时写 → 复制私有副本并返回新体号；
/// refcnt == 1 时写 → 原位改写合法（独占）。写后两体指纹必然不同。
pub fn copy_on_write(
    pool: &mut SharedPool,
    slot: usize,
    mutate: fn(&mut Vec<u8>),
) -> Result<(usize, bool), u16> {
    if slot >= POOL_CAP || pool.used & (1u16 << slot) == 0 {
        return Err(E_HANDLE);
    }
    if pool.refcnt[slot] > 1 {
        // 共享态：复制。
        let orig = match &pool.bodies[slot] {
            Some(b) => b.bytes.clone(),
            None => return Err(E_HANDLE),
        };
        let mut copy = orig.clone();
        mutate(&mut copy);
        if copy == orig {
            return Err(E_COW); // 写操作未生效（变异函数失职）
        }
        let new_slot = pool.publish(copy)?;
        pool.refcnt[slot] -= 1; // 写者让出对旧体的引用
        pool.refcnt[new_slot] += 1; // 并持新体
        Ok((new_slot, true))
    } else {
        // 独占态：原位写。
        let b = match &mut pool.bodies[slot] {
            Some(b) => b,
            None => return Err(E_HANDLE),
        };
        mutate(&mut b.bytes);
        let mut h = FNV_OFF;
        let bytes = b.bytes.clone();
        core::fnv_push(&bytes, &mut h);
        b.frozen_cs = h; // 独占写后重冻结
        Ok((slot, false))
    }
}

fn detail(code: u16) -> &'static str {
    match code {
        E_REF => "引用计数失守（负数/泄漏/悬增/悬放）",
        E_SHARED => "共享体被原位写（冻结指纹破——COW 红线）",
        E_COW => "写时副本异常（未生效/与源一致）",
        E_CAP => "共享池容量满",
        E_HANDLE => "句柄非法（未登记/已回收）",
        E_CS => "校验和不符",
        _ => "未知诊断码",
    }
}

/// 确定性演练：R1 引用守恒 200 例（acquire/release 配对 + 悬放/悬增注入）；
/// R2 共享态写 → COW 副本 + 原体指纹不动（100 例）；R3 独占态原位写合法；
/// R4 池容量与回收纪律；R5 冻结完整性全检。
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;
    let mk = |n: u8| -> Vec<u8> {
        let mut v: Vec<u8> = Vec::new();
        let mut i = 0;
        while i < 24 {
            v.push(n.wrapping_add(i as u8));
            i += 1;
        }
        v
    };
    // R1：200 例引用守恒（4 体 × 50 轮 acquire/release）。
    let mut pool = SharedPool::new();
    let mut slots: Vec<usize> = Vec::new();
    let mut s = 0;
    while s < 4 {
        slots.push(pool.publish(mk(s as u8))?);
        s += 1;
    }
    let mut r = 0u32;
    while r < 200 {
        let slot = slots[(r as usize) % 4];
        // 每槽前 25 轮 acquire、后 25 轮 release（按槽内轮次交替，
        // 保证配对守恒——按 r 奇偶交替会让单槽只见一种操作）。
        let do_acquire = (r / 4) % 2 == 0;
        if do_acquire {
            pool.acquire(slot)?;
        } else {
            pool.release(slot)?;
        }
        hits += 1;
        r += 1;
    }
    // 平衡态：acquire 100 次、release 100 次 → 全体计数应为 0。
    let mut s2 = 0;
    while s2 < 4 {
        pool.reclaim(slots[s2])?; // 计数 0 才可回收
        s2 += 1;
    }
    hits += 1;
    // R2：COW 100 例（双引用共享态写 → 副本 + 原体不动）。
    let bump = |bytes: &mut Vec<u8>| {
        let n = bytes.len();
        if n > 0 {
            bytes[n - 1] = bytes[n - 1].wrapping_add(1);
        }
    };
    let mut i = 0;
    while i < 100 {
        let mut p2 = SharedPool::new();
        let body = p2.publish(mk((i % 251) as u8))?;
        p2.acquire(body)?; // refcnt=1
        p2.acquire(body)?; // refcnt=2 → 共享态（写者 + 一名读者）
        let (ns, was_shared) = copy_on_write(&mut p2, body, bump)?;
        if !was_shared {
            return Err(E_COW);
        }
        if ns == body {
            return Err(E_COW);
        }
        // 原体逐字节不动（指纹守恒 + 与副本尾字节差 1）。
        if p2.all_intact() != Ok(()) {
            return Err(E_SHARED);
        }
        let old = p2.view(body)?;
        let new = p2.view(ns)?;
        let ol = old.len();
        if new[ol - 1] != old[ol - 1].wrapping_add(1) {
            return Err(E_COW);
        }
        hits += 1;
        i += 1;
    }
    // R3：独占态原位写合法（refcnt ≤ 1 → 原位改写，体号不变）。
    let mut p3 = SharedPool::new();
    let b3 = p3.publish(mk(7))?;
    p3.acquire(b3)?; // refcnt=1（独占写者）
    let (ns3, shared3) = copy_on_write(&mut p3, b3, bump)?;
    if shared3 || ns3 != b3 {
        return Err(E_COW);
    }
    if p3.all_intact() != Ok(()) {
        return Err(E_SHARED);
    }
    hits += 1;
    // R4：池容量与回收纪律。
    let mut p4 = SharedPool::new();
    let mut held: Vec<usize> = Vec::new();
    let mut k = 0;
    while k < POOL_CAP {
        held.push(p4.publish(mk((k + 1) as u8))?);
        k += 1;
    }
    if p4.publish(mk(0xff)) != Err(E_CAP) {
        return Err(E_CAP);
    }
    // 有引用不可回收：先 acquire（refcnt=1）再回收必拒。
    p4.acquire(held[0])?;
    if p4.reclaim(held[0]) != Err(E_REF) {
        return Err(E_REF);
    }
    p4.release(held[0])?; // 回到 0
    p4.reclaim(held[0])?; // 计数 0：回收合法
    hits += 1;
    // R5：悬增/悬放注入。
    if p4.release(held[1]) != Err(E_REF) {
        return Err(E_REF); // refcnt=0 悬放
    }
    if p4.acquire(POOL_CAP) != Err(E_HANDLE) || p4.view(99) != Err(E_HANDLE) {
        return Err(E_HANDLE);
    }
    if p4.all_intact() != Ok(()) {
        return Err(E_SHARED);
    }
    hits += 1;
    Ok(hits)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn drill_green() {
        assert_eq!(drill(), Ok(304));
    }

    #[test]
    fn refcnt_balance() {
        let mut p = SharedPool::new();
        assert!(p.publish(vec![1, 2, 3]).is_ok());
        assert_eq!(p.release(0).err(), Some(E_REF)); // 未获取先释放 = 悬放
    }

    #[test]
    fn cow_on_shared_write() {
        let mut p = SharedPool::new();
        let s = match p.publish(vec![9u8; 8]) {
            Ok(v) => v,
            Err(_) => 0,
        };
        let _ = p.acquire(s); // refcnt=1：独占写路径
        assert!(copy_on_write(&mut p, s, |b| b[0] = 1).is_ok());
    }
}

pub fn run_unx36070_checks() -> CheckSet {
    let mut cs = CheckSet::new("unxstar-unx36070_sdcow");
    cs.add("SEG 段位独占", SEG_BASE == 0xe220, "0xe220~0xe22f 本模块独占");
    match drill() {
        Ok(_) => cs.add("J1 确定性演练全绿", true, "引用守恒 200 例+COW 100 例+冻结指纹全检"),
        Err(code) => cs.add("J1 确定性演练全绿", false, detail(code)),
    }
    cs.add("池容量锚", POOL_CAP == 16, "定容共享池");
    cs
}
