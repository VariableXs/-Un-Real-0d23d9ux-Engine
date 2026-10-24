//! unx0539_zswapin —— UNX-F0539 换页调入验证账。
//! 锚点：主机 MMU 卷 · demand paging（缺页 → 后备取页 → 校验 → 挂载）。
//! 判据原文：取页校验不过即弃（弃页回收），重复缺页同 VA 只取一次
//!           （在途合并），调入账守恒（请 = 成 + 弃 + 在途）。

/// 后备页状态。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Slot {
    /// 空。
    Empty,
    /// 在途（有请求者）。
    Pending,
    /// 就绪（内容有效）。
    Ready,
    /// 已挂载。
    Mounted,
}

/// 后备存储槽。
#[derive(Debug, Clone, Copy)]
pub struct BSlot {
    pub idx: u32,
    pub state: Slot,
    /// 内容指纹（校验用）。
    pub fp: u32,
}

/// 调入引擎。
#[derive(Debug)]
pub struct SwapIn {
    pub slots: alloc::vec::Vec<BSlot>,
    /// 请求账 / 成功账 / 弃页账 / 在途合并账。
    pub n_req: u64,
    pub n_ok: u64,
    pub n_drop: u64,
    pub n_coal: u64,
}

/// 指纹校验：模拟 IO 完成后的内容核（fp 高 16 位为校验和，须 == 低 16 位翻转）。
pub fn fp_valid(fp: u32) -> bool {
    let hi = fp >> 16;
    let lo = fp & 0xFFFF;
    hi == (!lo & 0xFFFF)
}

impl SwapIn {
    pub fn new(n: usize) -> Self {
        let slots = (0..n as u32).map(|i| BSlot { idx: i, state: Slot::Empty, fp: 0 }).collect();
        SwapIn { slots, n_req: 0, n_ok: 0, n_drop: 0, n_coal: 0 }
    }

    /// 发起调入：Empty→Pending；Pending→合并；越界拒。
    pub fn request(&mut self, idx: u32) -> Result<bool, ()> {
        self.n_req += 1;
        let s = self.slots.get_mut(idx as usize).ok_or(())?;
        match s.state {
            Slot::Empty => {
                s.state = Slot::Pending;
                Ok(true) // 新请求
            }
            Slot::Pending => {
                self.n_coal += 1;
                Ok(false) // 在途合并
            }
            _ => Err(()), // Ready/Mounted 不可再请求
        }
    }

    /// IO 完成：校验指纹，过则 Ready，不过弃。
    pub fn io_done(&mut self, idx: u32, fp: u32) -> Result<bool, ()> {
        let s = self.slots.get_mut(idx as usize).ok_or(())?;
        if s.state != Slot::Pending {
            return Err(());
        }
        if fp_valid(fp) {
            s.fp = fp;
            s.state = Slot::Ready;
            self.n_ok += 1;
            Ok(true)
        } else {
            s.state = Slot::Empty;
            self.n_drop += 1;
            Ok(false)
        }
    }

    /// 挂载：Ready→Mounted。
    pub fn mount(&mut self, idx: u32) -> Result<(), ()> {
        let s = self.slots.get_mut(idx as usize).ok_or(())?;
        if s.state != Slot::Ready {
            return Err(());
        }
        s.state = Slot::Mounted;
        Ok(())
    }

    /// 在途计数。
    pub fn pending(&self) -> usize {
        self.slots.iter().filter(|s| s.state == Slot::Pending).count()
    }

    /// 守恒：请求 = 成 + 弃 + 在途 + 合并（合并请求不新建槽故并入请求流）。
    pub fn reconcile(&self) -> bool {
        self.n_req == self.n_ok + self.n_drop + self.pending() as u64 + self.n_coal
    }
}

/// 判据入口：换页调入全性质核。
pub fn run_unx0539_checks() -> crate::checks::CheckSet {
    let mut cs = crate::checks::CheckSet::new("UNX-F0539");

    // 族一：指纹校验器。
    let good = (0x1234u32 << 16) | (!0x1234u32 & 0xFFFF);
    cs.add("好指纹过", fp_valid(good), "高 16 = 低 16 反");
    cs.add("坏指纹拒", !fp_valid(0xDEAD_BEEF), "校验和错");
    cs.add("零指纹拒", !fp_valid(0), "0 高位 != FFFF 低位");

    // 族二：请求与在途合并。
    let mut e = SwapIn::new(8);
    cs.add("首发新请求", e.request(2) == Ok(true), "Empty→Pending");
    cs.add("再求合并", e.request(2) == Ok(false), "同槽二求并入");
    cs.add("三求仍合并", e.request(2) == Ok(false), "三求一并");
    cs.add("合并账二", e.n_coal == 2, "两次合并");
    cs.add("越界拒", e.request(99).is_err(), "槽外拒");
    cs.add("在途一", e.pending() == 1, "槽 2 在途");

    // 族三：IO 完成校验。
    cs.add("好指纹转就绪", e.io_done(2, good) == Ok(true), "校验过 Ready");
    cs.add("成账一", e.n_ok == 1, "调入成功");
    cs.add("挂载通", e.mount(2).is_ok(), "Ready→Mounted");
    cs.add("重复请求拒", e.request(2).is_err(), "Mounted 态拒");

    // 族四：坏指纹弃页。
    let mut e2 = SwapIn::new(4);
    e2.request(1).ok();
    cs.add("坏指纹弃", e2.io_done(1, 0xDEAD_BEEF) == Ok(false), "校验败弃页");
    cs.add("弃账一", e2.n_drop == 1, "弃页入账");
    cs.add("槽回空", e2.slots[1].state == Slot::Empty, "Empty 可再请求");
    cs.add("弃后重取", e2.request(1) == Ok(true), "重取新请求");

    // 族五：状态机闸（Ready 前拒挂载 / 空槽拒 IO）。
    let mut e3 = SwapIn::new(4);
    cs.add("空槽拒 IO", e3.io_done(0, good).is_err(), "非 Pending 拒");
    e3.request(0).ok();
    cs.add("Pending 拒挂", e3.mount(0).is_err(), "未就绪拒挂");
    e3.io_done(0, good).ok();
    cs.add("就绪可挂", e3.mount(0).is_ok(), "链路通");

    // 族六：守恒账。
    let mut e4 = SwapIn::new(16);
    for i in 0..6u32 {
        e4.request(i).ok();
    }
    e4.request(0).ok(); // 合并
    e4.io_done(0, good).ok();
    e4.io_done(1, 0xBAD).ok();
    e4.io_done(2, good).ok();
    e4.io_done(3, 0xBAD).ok();
    // 槽 4、5 仍 Pending。
    cs.add("成二弃二", e4.n_ok == 2 && e4.n_drop == 2, "4/6 收尾");
    cs.add("合并一", e4.n_coal == 1, "槽 0 二求");
    cs.add("请求账七", e4.n_req == 7, "6 新 + 1 合并");
    cs.add("守恒核", e4.reconcile(), "请 = 成 + 弃 + 在途 + 合并");

    // 族七：批量压力（全槽走完链路）。
    let mut e5 = SwapIn::new(32);
    let mut all_ok = true;
    for i in 0..32u32 {
        if e5.request(i) != Ok(true) {
            all_ok = false;
        }
        let fp = ((i as u32 + 0x100) << 16) | (!(i as u32 + 0x100) & 0xFFFF);
        if e5.io_done(i, fp) != Ok(true) {
            all_ok = false;
        }
        if e5.mount(i).is_err() {
            all_ok = false;
        }
    }
    cs.add("三十二槽全链", all_ok, "请求→IO→挂载全通");
    cs.add("全挂载", e5.slots.iter().all(|s| s.state == Slot::Mounted), "32 Mounted");
    cs.add("压守恒", e5.n_ok == 32 && e5.n_drop == 0 && e5.pending() == 0, "零弃零在途");

    // 族八：重复弃页循环（弃→重取→再弃）。
    let mut e6 = SwapIn::new(2);
    e6.request(0).ok();
    e6.io_done(0, 0xBAD).ok();
    e6.request(0).ok();
    e6.io_done(0, 0xBAD).ok();
    cs.add("二取二弃", e6.n_drop == 2 && e6.n_req == 2, "循环账齐");
    cs.add("槽仍空", e6.slots[0].state == Slot::Empty, "循环后可再取");

    cs
}
