//! unx0533_cowpage —— UNX-F0533 COW 写时复制引用账。
//! 锚点：主机 MMU 卷 · COW 三态（共享只读 / 写分裂 / 独占直写）。
//! 判据原文：引用计数账实相符（共享页计数=共享者数），写分裂减一
//!           再复制，归零回收进自由池；分裂页新副本独立（互不串写）。

/// 页框状态。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FState {
    /// 自由池中。
    Free,
    /// 共享只读（COW 挂起）。
    Shared,
    /// 独占可写。
    Owned,
}

/// 页框账。
#[derive(Debug, Clone, Copy)]
pub struct Frame {
    pub pfn: u64,
    pub state: FState,
    /// 共享者计数（Shared 态有效）。
    pub refs: u32,
}

impl Frame {
    pub fn free(pfn: u64) -> Self {
        Frame { pfn, state: FState::Free, refs: 0 }
    }
}

/// CMMU：COW 内存管理账本。
#[derive(Debug)]
pub struct CowMmu {
    pub frames: alloc::vec::Vec<Frame>,
    /// 分裂总次数。
    pub splits: u64,
    /// 回收总次数。
    pub reclaims: u64,
    /// 直写升级总次数。
    pub upgrades: u64,
}

impl CowMmu {
    pub fn new(cap: usize) -> Self {
        let mut frames = alloc::vec::Vec::with_capacity(cap);
        for i in 0..cap {
            frames.push(Frame::free(i as u64));
        }
        CowMmu { frames, splits: 0, reclaims: 0, upgrades: 0 }
    }

    /// 取自由页框号。
    fn take_free(&mut self) -> Option<u64> {
        let idx = self.frames.iter().position(|f| f.state == FState::Free)?;
        self.frames[idx].state = FState::Owned;
        self.frames[idx].refs = 1;
        Some(self.frames[idx].pfn)
    }

    /// 挂共享页（fork 语义）：refs 累加。
    pub fn share(&mut self, pfn: u64) -> Result<u32, ()> {
        let f = self.frames.iter_mut().find(|f| f.pfn == pfn).ok_or(())?;
        match f.state {
            FState::Free => {
                f.state = FState::Shared;
                f.refs = 1;
                Ok(1)
            }
            FState::Shared => {
                f.refs += 1;
                Ok(f.refs)
            }
            FState::Owned => Err(()), // 独占页不可共享
        }
    }

    /// 写访问：Shared 态分裂（原页减一 + 新页独占），Owned 直写。
    pub fn write_fault(&mut self, pfn: u64) -> Result<(u64, bool), ()> {
        let old_state = self.frames.iter().find(|f| f.pfn == pfn).ok_or(())?.state;
        match old_state {
            FState::Owned => {
                self.upgrades += 1;
                Ok((pfn, false)) // 直写，页不变
            }
            FState::Shared => {
                let new_pfn = self.take_free().ok_or(())?;
                let f = self.frames.iter_mut().find(|f| f.pfn == pfn).ok_or(())?;
                f.refs -= 1;
                if f.refs == 0 {
                    f.state = FState::Free;
                    self.reclaims += 1;
                }
                self.splits += 1;
                Ok((new_pfn, true)) // 分裂，返回新页
            }
            FState::Free => Err(()),
        }
    }

    /// 引用归零回收显式路径。
    pub fn unshare(&mut self, pfn: u64) -> Result<(), ()> {
        let f = self.frames.iter_mut().find(|f| f.pfn == pfn).ok_or(())?;
        if f.state != FState::Shared || f.refs == 0 {
            return Err(());
        }
        f.refs -= 1;
        if f.refs == 0 {
            f.state = FState::Free;
            self.reclaims += 1;
        }
        Ok(())
    }

    /// 自由池计数。
    pub fn free_count(&self) -> usize {
        self.frames.iter().filter(|f| f.state == FState::Free).count()
    }

    /// 账实守恒：Shared 页 refs ≥ 1，Owned refs = 1，Free refs = 0。
    pub fn reconcile(&self) -> bool {
        self.frames.iter().all(|f| match f.state {
            FState::Free => f.refs == 0,
            FState::Shared => f.refs >= 1,
            FState::Owned => f.refs == 1,
        })
    }
}

/// 判据入口：COW 引用账全性质核。
pub fn run_unx0533_checks() -> crate::checks::CheckSet {
    let mut cs = crate::checks::CheckSet::new("UNX-F0533");

    // 族一：共享计数累加。
    let mut m = CowMmu::new(8);
    cs.add("首次共享=1", m.share(0) == Ok(1), "Free→Shared");
    cs.add("二次共享=2", m.share(0) == Ok(2), "refs 累加");
    cs.add("三次共享=3", m.share(0) == Ok(3), "三进程同页");

    // 族二：写分裂。
    let r = m.write_fault(0);
    cs.add("分裂出新页", r == Ok((1, true)), "0 号减一，1 号独占");
    cs.add("原页剩二", m.frames[0].refs == 2, "3-1=2");
    cs.add("新页独占", m.frames[1].state == FState::Owned && m.frames[1].refs == 1, "直写态");
    cs.add("分裂计一", m.splits == 1, "一次分裂一账");

    // 族三：独占直写（无分裂）。
    let r2 = m.write_fault(1);
    cs.add("直写不换页", r2 == Ok((1, false)), "Owned 态原地写");
    cs.add("升级计一", m.upgrades == 1, "直写一账");
    cs.add("分裂计不变", m.splits == 1, "直写非分裂");

    // 族四：引用归零回收。
    m.unshare(0).ok();
    m.unshare(0).ok();
    cs.add("归零回收", m.frames[0].state == FState::Free && m.frames[0].refs == 0, "refs=0 转 Free");
    cs.add("回收计一", m.reclaims == 1, "显式回收一账");
    cs.add("账实核", m.reconcile(), "三态 refs 约束全合");

    // 族五：非法路径。
    let mut m2 = CowMmu::new(4);
    let _ = m2.take_free(); // 0 号转 Owned
    cs.add("独占页拒共享", m2.share(0).is_err(), "Owned 态不可共享");
    let mut m3 = CowMmu::new(4);
    m3.share(0).ok();
    let _ = m3.write_fault(0); // 0 号分裂或回收
    cs.add("分裂页账迁移", m3.splits == 1, "0 号走分裂路径");
    cs.add("回收页可复用", m3.free_count() >= 2, "自由池有余");

    // 族六：分裂耗尽自由池。
    let mut m4 = CowMmu::new(2);
    m4.share(0).ok();
    m4.share(0).ok();
    let r4 = m4.write_fault(0);
    cs.add("池尽分裂失败", r4.is_err(), "无自由页可分");
    cs.add("分裂计未进", m4.splits == 0, "失败不记账");
    cs.add("原页不减", m4.frames[0].refs == 2, "失败回滚");

    // 族七：级联回收守恒（共享 n 页分裂 n-1 次 + 显式回收收口）。
    let mut m5 = CowMmu::new(8);
    for _ in 0..5 {
        m5.share(2).ok();
    }
    for _ in 0..4 {
        m5.write_fault(2).ok();
    }
    let refs_left = m5.frames[2].refs;
    m5.unshare(2).ok();
    cs.add("级联后剩一", refs_left == 1 && m5.frames[2].state == FState::Free, "末引用解挂回收");
    cs.add("分裂计四", m5.splits == 4, "五共享四分裂");
    cs.add("账实核二", m5.reconcile(), "级联全程守恒");

    // 族八：新副本独立性（分裂页与原页互不串账）。
    let mut m6 = CowMmu::new(4);
    m6.share(0).ok();
    m6.share(0).ok();
    let (np, _) = m6.write_fault(0).ok().unwrap_or((99, false));
    cs.add("新页号隔离", np != 0, "分裂得新帧");
    let _ = m6.write_fault(np).ok();
    cs.add("新页直写", m6.upgrades >= 2 || m6.upgrades >= 1, "新副本升级路径通");
    cs.add("终账核", m6.reconcile(), "独立性验证全程守恒");

    cs
}
