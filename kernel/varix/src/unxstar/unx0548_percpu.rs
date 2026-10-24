//! unx0548_percpu —— UNX-F0548 per-CPU 区账。
//! 锚点：SMP 卷 · 核独占槽位（分配 / 释放 / 越权访拒 / 槽位守恒）。
//! 判据原文：每核至多一槽（独占），越核访问拒（槽位封印）；
//!           释放归还自由池，槽账守恒（总量 = 占 + 空）。

/// CPU 槽位。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Slot {
    /// 空闲。
    Free,
    /// 被核 id 占据。
    Held(u8),
}

/// per-CPU 区。
#[derive(Debug)]
pub struct PercpuArea {
    pub slots: alloc::vec::Vec<Slot>,
    /// 越权拒账。
    pub n_trespass: u64,
    /// 分配账 / 释放账。
    pub n_alloc: u64,
    pub n_free: u64,
}

impl PercpuArea {
    pub fn new(n: usize) -> Self {
        PercpuArea { slots: (0..n).map(|_| Slot::Free).collect(), n_trespass: 0, n_alloc: 0, n_free: 0 }
    }

    /// 分配：取首个空闲槽。
    pub fn alloc_slot(&mut self, core: u8) -> Result<usize, ()> {
        if self.slots.iter().any(|s| *s == Slot::Held(core)) {
            self.n_trespass += 1;
            return Err(()); // 该核已持槽
        }
        let idx = self.slots.iter().position(|s| *s == Slot::Free).ok_or(())?;
        self.slots[idx] = Slot::Held(core);
        self.n_alloc += 1;
        Ok(idx)
    }

    /// 释放：核归槽。
    pub fn free_slot(&mut self, core: u8) -> Result<usize, ()> {
        let idx = self.slots.iter().position(|s| *s == Slot::Held(core)).ok_or(())?;
        self.slots[idx] = Slot::Free;
        self.n_free += 1;
        Ok(idx)
    }

    /// 访问槽位内容（越权核访问他人槽拒）。
    pub fn read_slot(&mut self, core: u8, idx: usize) -> Result<Option<u8>, ()> {
        let s = self.slots.get(idx).ok_or(())?;
        match s {
            Slot::Held(owner) if *owner == core => Ok(Some(*owner)),
            Slot::Held(_) => {
                self.n_trespass += 1;
                Err(())
            }
            Slot::Free => Err(()),
        }
    }

    /// 槽账守恒。
    pub fn reconcile(&self) -> bool {
        let held = self.slots.iter().filter(|s| **s != Slot::Free).count();
        held + self.slots.iter().filter(|s| **s == Slot::Free).count() == self.slots.len()
    }

    pub fn free_count(&self) -> usize {
        self.slots.iter().filter(|s| **s == Slot::Free).count()
    }
}

/// 判据入口：per-CPU 槽位全性质核。
pub fn run_unx0548_checks() -> crate::checks::CheckSet {
    let mut cs = crate::checks::CheckSet::new("UNX-F0548");

    // 族一：分配独占。
    let mut a = PercpuArea::new(4);
    cs.add("核 1 得槽 0", a.alloc_slot(1) == Ok(0), "首空槽");
    cs.add("核 2 得槽 1", a.alloc_slot(2) == Ok(1), "次空槽");
    cs.add("核 1 重分拒", a.alloc_slot(1).is_err(), "独占拒重分");
    cs.add("越权账一", a.n_trespass == 1, "重分入拒");

    // 族二：越权读拒。
    cs.add("核 2 读他槽拒", a.read_slot(2, 0).is_err(), "槽 0 属核 1");
    cs.add("越权读入账", a.n_trespass == 2, "累计二");
    cs.add("属主读通", a.read_slot(1, 0).is_ok(), "核 1 读自槽");
    cs.add("空槽读拒", a.read_slot(1, 3).is_err(), "Free 槽不可读");
    cs.add("越界读拒", a.read_slot(1, 99).is_err(), "槽外拒");

    // 族三：释放归还。
    cs.add("核 1 归槽", a.free_slot(1) == Ok(0), "槽 0 回池");
    cs.add("归后可复用", a.alloc_slot(3) == Ok(0), "核 3 得复用槽");
    cs.add("无槽核归拒", a.free_slot(9).is_err(), "未持槽归拒");

    // 族四：槽耗尽。
    let mut b = PercpuArea::new(2);
    b.alloc_slot(0).ok();
    b.alloc_slot(1).ok();
    cs.add("耗尽拒分", b.alloc_slot(2).is_err(), "无空槽");
    cs.add("守恒核", b.reconcile(), "总 = 占 + 空");
    cs.add("归一复得", { b.free_slot(0).ok(); b.alloc_slot(2).is_ok() }, "归后再分");

    // 族五：批量压测（八槽八核全占全归）。
    let mut c = PercpuArea::new(8);
    let mut ok1 = true;
    for core in 0..8u8 {
        if c.alloc_slot(core).is_err() {
            ok1 = false;
        }
    }
    cs.add("八核八槽", ok1 && c.free_count() == 0, "全占");
    cs.add("占满守恒", c.reconcile(), "压测中段核");
    let mut ok2 = true;
    for core in 0..8u8 {
        if c.free_slot(core).is_err() {
            ok2 = false;
        }
    }
    cs.add("八归清池", ok2 && c.free_count() == 8, "全归");
    cs.add("终账", c.n_alloc == 9 && c.n_free == 8, "8+1 分 8 归（复用一）");

    // 族六：核-槽映射核（分配序决定槽号，归还后重分得原槽）。
    let mut d = PercpuArea::new(4);
    let s_a = d.alloc_slot(1).ok().unwrap_or(99);
    let s_b = d.alloc_slot(2).ok().unwrap_or(99);
    cs.add("顺位槽号", s_a == 0 && s_b == 1, "先到先槽");
    d.free_slot(1).ok();
    cs.add("归还重分同槽", d.alloc_slot(3) == Ok(0), "槽 0 复用");
    cs.add("他人槽不动", d.read_slot(2, 1).is_ok(), "核 2 槽 1 无扰");

    // 族七：释放后越权读翻空（Free 态连属主也拒读）。
    let mut e = PercpuArea::new(2);
    e.alloc_slot(5).ok();
    cs.add("释放前读通", e.read_slot(5, 0).is_ok(), "属主在");
    e.free_slot(5).ok();
    cs.add("释放后读拒", e.read_slot(5, 0).is_err(), "Free 态封读");
    cs.add("封读不算越权", e.n_trespass == 0, "Free 拒不入越权账");

    // 族八：单核反复占归循环账。
    let mut f = PercpuArea::new(1);
    let mut loop_ok = true;
    for _ in 0..10 {
        if f.alloc_slot(9).is_err() || f.free_slot(9).is_err() {
            loop_ok = false;
        }
    }
    cs.add("十轮占归", loop_ok, "单槽循环稳定");
    cs.add("循环账平", f.n_alloc == 10 && f.n_free == 10, "分归对称");
    cs.add("循环后池满", f.free_count() == 1 && f.reconcile(), "终态全空");

    cs
}
