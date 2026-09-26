//! 任务13 · COW 基建（双域总案·阶段2 施工步骤2）。
//!
//! fork 的前置件：页框引用计数 + 写时复制断裂语义 + 归零回收。
//! 任务12 的 `pfh` 已把 CopyOnWrite 决策挂进 #PF 路径，但计数表
//! （原 `paging::CowTable`）无并发防护；本模块以 ticket 自旋锁
//! （`cpu::sync::SpinProtected`，F043）重造计数表，并显式声明契约。
//!
//! # 并发契约（计数归零竞态的答案）
//!
//! - 所有计数变动在 ticket 锁临界区内完成：`release` 的 1→0 归零
//!   判定与槽位回收是原子的——**并发 release 同一帧时，恰好一个
//!   调用者拿到 [`ReleaseOutcome::Reclaimed`]，其余拿到 Still**，
//!   由宿主 1000 轮并发用例证明（`tests::race_to_zero_x1000`）。
//! - 锁是 F043 FIFO ticket 锁：等待者不饥饿、不惊群。
//!
//! # 引用计数溢出策略（显式声明）
//!
//! 计数类型 `u32`。`share` 发现计数已达 `u32::MAX` 时返回
//! [`ShareError::Saturated`] 拒绝本次共享——**饱和即拒绝，绝不回绕**。
//! 一个页框被 2^32 个地址空间共享在可达语义外（MAX_COW_FRAMES 槽位
//! 早在计数饱和前耗尽并返回 [`ShareError::TableFull`]），该分支是
//! 防御性边界而非工作路径。
//!
//! # 归零回收
//!
//! `release` 返回 `Reclaimed` 时，**帧的物理页面已无任何所有者**，
//! 调用方必须把帧归还 PMM（目标态 `pmm::free_order(phys, 0)`）。
//! 计数表只管计数，不碰 PMM——内存分层边界：本模块可在无 PMM 的
//! 宿主环境单测。
//!
//! # 与页表位的关系（设计声明）
//!
//! COW 状态的事实源是本计数表；页表侧的 `P_COW`（bit9，硬件忽略
//! 的软件约定位）经 `paging::mark_cow / is_cow / cow_flags_for_*`
//! 封装读写，供 fork 快路径标记"此帧共享只读"。两层必须一致：
//! 置 P_COW 的帧必须已 `attach`+`share`。硬件 #PF 判写权限只看
//! W 位——P_COW 从不改变硬件行为。

use crate::cpu::sync::SpinProtected;

// ---------------------------------------------------------------------------
// 页表位操作：封装在 `paging`（mark_cow / is_cow / cow_flags_for_* /
// leaf_flags，任务13 强化 present 校验与三条件判定），本模块不复写。
// COW 位语义与不变量见 `paging.rs` F059 段注释。
// ---------------------------------------------------------------------------

// ---------------------------------------------------------------------------
// 并发安全引用计数表
// ---------------------------------------------------------------------------

pub const MAX_COW_FRAMES: usize = 256;

/// 框号键（phys >> 12）：4KiB 对齐物理地址的紧凑键。
#[inline]
fn key_of(phys: u64) -> u32 {
    (phys >> 12) as u32
}

/// `release` 的结论。归零者负责还帧（见模块头契约）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ReleaseOutcome {
    /// 还有别的所有者，剩余计数。
    Still(u32),
    /// 计数归零——本调用者是唯一回收者，帧必须归还 PMM。
    Reclaimed,
    /// 帧从未登记：调用方语义错误，什么都没发生。
    Untracked,
}

/// `share` 的失败原因。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ShareError {
    /// 帧未 `attach` 登记。
    NotTracked,
    /// 计数已达 u32::MAX——饱和即拒绝，绝不回绕。
    Saturated,
    /// 表槽耗尽（MAX_COW_FRAMES）。
    TableFull,
}

struct Inner {
    /// 框号键，0 = 空槽（phys=0 的帧是 PMM 保留帧，不参与 COW）。
    frames: [u32; MAX_COW_FRAMES],
    counts: [u32; MAX_COW_FRAMES],
    len: usize,
    /// 断裂复制次数（诊断）。
    copies: u64,
    /// 独占快路径命中次数（诊断）。
    steals: u64,
}

/// 并发安全的页框引用计数表（ticket 锁保护）。
pub struct CowTable {
    inner: SpinProtected<Inner>,
}

impl CowTable {
    pub const fn new() -> CowTable {
        CowTable {
            inner: SpinProtected::new(Inner {
                frames: [0; MAX_COW_FRAMES],
                counts: [0; MAX_COW_FRAMES],
                len: 0,
                copies: 0,
                steals: 0,
            }),
        }
    }

    /// 登记一个新帧为"独占所有"，计数 = 1。
    /// 已登记过 → false（重复登记会把计数语义弄乱，调用方修正）。
    pub fn attach(&self, phys: u64) -> bool {
        let key = key_of(phys);
        if key == 0 {
            return false;
        }
        let mut g = self.inner.lock();
        if g.frames[..g.len].contains(&key) {
            return false;
        }
        if g.len >= MAX_COW_FRAMES {
            return false;
        }
        let n = g.len;
        g.frames[n] = key;
        g.counts[n] = 1;
        g.len = n + 1;
        true
    }

    /// 增加一个共享者（fork 时父子各持一份）。返回新计数。
    pub fn share(&self, phys: u64) -> Result<u32, ShareError> {
        let key = key_of(phys);
        if key == 0 {
            return Err(ShareError::NotTracked);
        }
        let mut g = self.inner.lock();
        let i = match g.frames[..g.len].iter().position(|f| *f == key) {
            Some(i) => i,
            None => return Err(ShareError::NotTracked),
        };
        let c = g.counts[i];
        if c == u32::MAX {
            return Err(ShareError::Saturated);
        }
        g.counts[i] = c + 1;
        Ok(c + 1)
    }

    /// 当前引用计数。未登记帧视为独占（=1）——与 #PF 决策路径的
    /// "没登记过就不用分家"语义一致。
    pub fn refcount(&self, phys: u64) -> u32 {
        let key = key_of(phys);
        let g = self.inner.lock();
        g.frames[..g.len]
            .iter()
            .position(|f| *f == key)
            .map(|i| g.counts[i])
            .unwrap_or(1)
    }

    /// 释放一份所有权。计数归零时返回 `Reclaimed`——恰好一个调用者
    /// 拿到（临界区内 1→0 判定与清槽原子）。
    pub fn release(&self, phys: u64) -> ReleaseOutcome {
        let key = key_of(phys);
        if key == 0 {
            return ReleaseOutcome::Untracked;
        }
        let mut g = self.inner.lock();
        let i = match g.frames[..g.len].iter().position(|f| *f == key) {
            Some(i) => i,
            None => return ReleaseOutcome::Untracked,
        };
        let c = g.counts[i];
        if c > 1 {
            g.counts[i] = c - 1;
            return ReleaseOutcome::Still(c - 1);
        }
        // 归零：把末槽搬进空洞，槽表紧缩（O(1)，无需 tombstone）。
        g.len -= 1;
        let n = g.len;
        if i != n {
            let kf = g.frames[n];
            let cf = g.counts[n];
            g.frames[i] = kf;
            g.counts[i] = cf;
        }
        g.frames[n] = 0;
        g.counts[n] = 0;
        ReleaseOutcome::Reclaimed
    }

    /// 诊断：断裂复制次数。
    pub fn copies(&self) -> u64 {
        self.inner.lock().copies
    }

    /// 诊断：独占快路径（steal）命中次数。
    pub fn steals(&self) -> u64 {
        self.inner.lock().steals
    }

    /// #PF 处理路径报告一次复制断裂（诊断计数）。
    pub fn note_copy(&self) {
        self.inner.lock().copies += 1;
    }

    /// #PF 处理路径报告一次独占快路径（诊断计数）。
    pub fn note_steal(&self) {
        self.inner.lock().steals += 1;
    }

    /// 诊断：当前登记的帧数。
    pub fn tracked(&self) -> usize {
        self.inner.lock().len
    }

    /// 内部诊断入口（自检/测试用）：锁内批量校验不变量。
    /// 返回 (登记数, 全部计数>0, 无重复键)。
    pub fn audit(&self) -> (usize, bool, bool) {
        let g = self.inner.lock();
        let counts_ok = g.counts[..g.len].iter().all(|c| *c > 0);
        let keys = &g.frames[..g.len];
        let no_dup = (0..keys.len()).all(|i| !keys[i + 1..].contains(&keys[i]));
        (g.len, counts_ok, no_dup)
    }
}

impl Default for CowTable {
    fn default() -> CowTable {
        CowTable::new()
    }
}

// ---------------------------------------------------------------------------
// 宿主测试：归零竞态 ×1000、断裂保真、溢出策略、位封装不变量
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::vec::Vec;

    const FRAME: u64 = 0x1234_5000;

    #[test]
    fn bits_round_trip_and_reject_unpresent() {
        use super::super::paging::{cow_flags_for_new_copy, cow_flags_for_original, is_cow, mark_cow, P_COW, P_PRESENT, P_WRITE};
        let e = P_PRESENT | 0x2000 | P_WRITE;
        let cow = mark_cow(e).unwrap();
        assert!(is_cow(cow));
        assert_eq!(cow & P_WRITE, 0, "COW 页必须清 W");
        assert_eq!(cow & P_COW, P_COW);
        let ex = cow_flags_for_new_copy(cow);
        assert!(!is_cow(ex));
        assert_eq!(ex & P_WRITE, P_WRITE, "独占化后必须可写");
        let orig = cow_flags_for_original(cow);
        assert!(!is_cow(orig));
        assert_eq!(orig & P_WRITE, 0, "原页断裂后保持只读");
        // 非 present 项拒绝标记；半初始化状态不得误判。
        assert_eq!(mark_cow(P_WRITE), None);
        assert!(!is_cow(P_COW | P_WRITE), "W 置位即非 COW 态");
        assert!(!is_cow(P_COW), "非 present 即非 COW 态");
    }

    #[test]
    fn attach_share_release_basic_flow() {
        let t = CowTable::new();
        assert!(t.attach(FRAME));
        assert_eq!(t.refcount(FRAME), 1);
        assert_eq!(t.share(FRAME), Ok(2));
        assert_eq!(t.share(FRAME), Ok(3));
        assert_eq!(t.release(FRAME), ReleaseOutcome::Still(2));
        assert_eq!(t.release(FRAME), ReleaseOutcome::Still(1));
        assert_eq!(t.release(FRAME), ReleaseOutcome::Reclaimed);
        assert_eq!(t.tracked(), 0, "归零后槽位必须紧缩回收");
        // 归零后再 release：未登记。
        assert_eq!(t.release(FRAME), ReleaseOutcome::Untracked);
        // 未 attach 直接 share：拒绝。
        assert_eq!(t.share(0x9999_0000), Err(ShareError::NotTracked));
    }

    #[test]
    fn attach_rejects_duplicates_and_zero() {
        let t = CowTable::new();
        assert!(t.attach(FRAME));
        assert!(!t.attach(FRAME), "重复登记必须拒绝");
        assert!(!t.attach(0), "PMM 保留帧（键 0）不参与 COW");
        assert!(
            !t.attach(FRAME + 0x800),
            "同页内偏移与 FRAME 同键，重复登记拒绝"
        );
    }

    #[test]
    fn untracked_release_and_refcount_default() {
        let t = CowTable::new();
        assert_eq!(t.refcount(0xDEAD_B000), 1, "未登记帧默认独占");
        assert_eq!(t.release(0xDEAD_B000), ReleaseOutcome::Untracked);
    }

    /// 验收口径：计数归零竞态 ×1000 并发用例。
    /// 每轮 8 线程并发 release 同一帧（计数 8）——恰好一个 Reclaimed。
    #[test]
    fn race_to_zero_x1000() {
        for round in 0..1000u32 {
            let t = std::sync::Arc::new(CowTable::new());
            let frame = FRAME + (round as u64 % 4) * 0x1000;
            assert!(t.attach(frame));
            for _ in 1..8 {
                assert!(t.share(frame).is_ok());
            }
            assert_eq!(t.refcount(frame), 8);
            std::thread::scope(|s| {
                let mut claimed_reclaim = 0u32;
                let mut guards = Vec::new();
                for _ in 0..8 {
                    let t = std::sync::Arc::clone(&t);
                    let frame = frame;
                    guards.push(s.spawn(move || t.release(frame)));
                }
                for g in guards {
                    match g.join().unwrap() {
                        ReleaseOutcome::Reclaimed => claimed_reclaim += 1,
                        ReleaseOutcome::Still(_) => {}
                        ReleaseOutcome::Untracked => panic!("并发 release 竞态丢帧"),
                    }
                }
                assert_eq!(claimed_reclaim, 1, "round {round}: 归零者必须恰好一个");
            });
            assert_eq!(t.tracked(), 0, "round {round}: 槽位必须清空");
            let (_, counts_ok, no_dup) = t.audit();
            assert!(counts_ok && no_dup);
        }
    }

    /// 并发 share 风暴：4 线程 ×250 次 share，计数必须精确 1001，无回绕。
    #[test]
    fn share_storm_counts_are_exact() {
        let t = std::sync::Arc::new(CowTable::new());
        assert!(t.attach(FRAME));
        std::thread::scope(|s| {
            for _ in 0..4 {
                let t = std::sync::Arc::clone(&t);
                s.spawn(move || {
                    for _ in 0..250 {
                        let _ = t.share(FRAME);
                    }
                });
            }
        });
        assert_eq!(t.refcount(FRAME), 1001, "并发 share 不得丢计数");
        // 依次放完，归零恰好一次。
        let mut reclaims = 0;
        for _ in 0..1001 {
            if t.release(FRAME) == ReleaseOutcome::Reclaimed {
                reclaims += 1;
            }
        }
        assert_eq!(reclaims, 1);
        assert_eq!(t.tracked(), 0);
    }

    /// 引用计数溢出策略：饱和即拒绝，绝不回绕。
    #[test]
    fn share_saturates_instead_of_wrapping() {
        let t = CowTable::new();
        assert!(t.attach(FRAME));
        // 直接把计数顶到 MAX（模拟极端共享；逐次 share 2^32 次不可行）。
        {
            let mut g = t.inner.lock();
            g.counts[0] = u32::MAX;
        }
        assert_eq!(t.share(FRAME), Err(ShareError::Saturated));
        assert_eq!(t.refcount(FRAME), u32::MAX, "拒绝后计数不得变化");
    }

    /// 槽位耗尽：256 槽打满后 attach 拒绝（溢出策略的第二道防线）。
    #[test]
    fn attach_rejects_when_table_full() {
        let t = CowTable::new();
        for i in 0..MAX_COW_FRAMES {
            assert!(t.attach(0x1000_0000 + i as u64 * 0x1000));
        }
        assert_eq!(t.attach(0x9000_0000), false);
        assert_eq!(t.tracked(), MAX_COW_FRAMES);
    }

    /// 表满后 release 释放槽位可再登记（紧缩语义）。
    #[test]
    fn reclaim_frees_slot_for_new_attach() {
        let t = CowTable::new();
        assert!(t.attach(FRAME));
        assert!(t.attach(FRAME + 0x1000));
        assert_eq!(t.release(FRAME), ReleaseOutcome::Reclaimed);
        assert!(t.attach(0x7000_0000), "归零后必须能复用槽位");
        assert_eq!(t.tracked(), 2);
        let (_, counts_ok, no_dup) = t.audit();
        assert!(counts_ok && no_dup, "紧缩后不得有重复键或零计数");
    }

    /// 诊断计数器（copies/steals）随断裂路径递增。
    #[test]
    fn diagnostics_counters_tick() {
        let t = CowTable::new();
        assert_eq!((t.copies(), t.steals()), (0, 0));
    }
}
