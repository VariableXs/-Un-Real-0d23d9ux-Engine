//! unx0549_ipiburst —— UNX-F0549 IPI 广播账。
//! 锚点：SMP 卷 · 核间中断广播（定向 / 广播 / 风暴限流 / 幂等）。
//! 判据原文：定向单发单达，广播全员各达一次（收账去重）；
//!           限流窗口内超额拒；收账守恒（发 = 收 + 丢弃）。

/// IPI 消息。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Ipi {
    pub from: u8,
    pub to: u8,
    /// 向量号。
    pub vec: u8,
}

/// IPI 编排器。
#[derive(Debug)]
pub struct IpiBus {
    /// 核数。
    pub ncpu: u8,
    /// 收账（核 → 已收向量集，去重）。
    pub inbox: alloc::vec::Vec<(u8, alloc::vec::Vec<u8>)>,
    /// 发账 / 丢弃账 / 限流拒账。
    pub n_sent: u64,
    pub n_dup: u64,
    pub n_drop: u64,
    /// 每核限流窗口。
    pub quota: u8,
}

impl IpiBus {
    pub fn new(ncpu: u8, quota: u8) -> Self {
        let inbox = (0..ncpu).map(|c| (c, alloc::vec::Vec::new())).collect();
        IpiBus { ncpu, inbox, n_sent: 0, n_dup: 0, n_drop: 0, quota }
    }

    /// 定向发：to 越界拒；同 (to, vec) 重复收记去重账。
    pub fn send(&mut self, from: u8, to: u8, vec: u8) -> Result<bool, ()> {
        if from >= self.ncpu || to >= self.ncpu {
            return Err(());
        }
        self.n_sent += 1;
        let box_ = self.inbox.iter_mut().find(|b| b.0 == to).ok_or(())?;
        if box_.1.contains(&vec) {
            self.n_dup += 1;
            return Ok(false); // 去重
        }
        if box_.1.len() >= self.quota as usize {
            self.n_drop += 1;
            return Ok(false); // 限流
        }
        box_.1.push(vec);
        Ok(true)
    }

    /// 广播：from 外全员各收一（自身不收）。
    pub fn broadcast(&mut self, from: u8, vec: u8) -> Result<usize, ()> {
        if from >= self.ncpu {
            return Err(());
        }
        let mut delivered = 0;
        for core in 0..self.ncpu {
            if core == from {
                continue;
            }
            if self.send(from, core, vec) == Ok(true) {
                delivered += 1;
            }
        }
        Ok(delivered)
    }

    /// 某核收账数。
    pub fn count_of(&self, core: u8) -> usize {
        self.inbox.iter().find(|b| b.0 == core).map(|b| b.1.len()).unwrap_or(0)
    }

    /// 守恒：发 = 实收 + 去重 + 丢弃。
    pub fn reconcile(&self) -> bool {
        let delivered: usize = self.inbox.iter().map(|b| b.1.len()).sum();
        self.n_sent as usize == delivered + self.n_dup as usize + self.n_drop as usize
    }
}

/// 判据入口：IPI 广播全性质核。
pub fn run_unx0549_checks() -> crate::checks::CheckSet {
    let mut cs = crate::checks::CheckSet::new("UNX-F0549");

    // 族一：定向单发。
    let mut bus = IpiBus::new(4, 8);
    cs.add("定向收", bus.send(0, 1, 0x20) == Ok(true), "首达");
    cs.add("同向同量去重", bus.send(0, 1, 0x20) == Ok(false), "重复收拒");
    cs.add("去重账一", bus.n_dup == 1, "入去重账");
    cs.add("越界拒", bus.send(0, 9, 0x20).is_err(), "to 越界");
    cs.add("源越界拒", bus.send(9, 0, 0x20).is_err(), "from 越界");

    // 族二：限流。
    let mut bus2 = IpiBus::new(2, 2);
    bus2.send(0, 1, 0x30).ok();
    bus2.send(0, 1, 0x31).ok();
    cs.add("额度内收二", bus2.count_of(1) == 2, "quota=2");
    cs.add("超额丢", bus2.send(0, 1, 0x32) == Ok(false), "第三发丢");
    cs.add("丢账一", bus2.n_drop == 1, "入丢账");
    cs.add("守恒核", bus2.reconcile(), "3 发 = 2 收 + 0 重 + 1 丢");

    // 族三：广播（自身不收）。
    let mut bus3 = IpiBus::new(4, 8);
    let d = bus3.broadcast(0, 0x40);
    cs.add("广播达三", d == Ok(3), "4 核减自身");
    cs.add("自身零收", bus3.count_of(0) == 0, "不自发");
    cs.add("他核各一", bus3.count_of(1) == 1 && bus3.count_of(2) == 1 && bus3.count_of(3) == 1, "各一");
    cs.add("广播守恒", bus3.reconcile(), "3 发 = 3 收");

    // 族四：重复广播去重。
    let d2 = bus3.broadcast(0, 0x40);
    cs.add("复播达零", d2 == Ok(0), "全去重");
    cs.add("去重账三", bus3.n_dup == 3, "三核各拒一");
    cs.add("复播守恒", bus3.reconcile(), "6 发 = 3 收 + 3 重");

    // 族五：异向量复播照收。
    let d3 = bus3.broadcast(0, 0x41);
    cs.add("异量达三", d3 == Ok(3), "新向量全员");
    cs.add("各核双收", bus3.count_of(1) == 2, "0x40+0x41");

    // 族六：风暴压测（限流保护核 1）。
    let mut bus4 = IpiBus::new(2, 4);
    let mut storm_ok = true;
    for i in 0..10u8 {
        match bus4.send(0, 1, 0x50 + i) {
            Ok(true) if bus4.count_of(1) <= 4 => {}
            Ok(false) => {}
            _ => storm_ok = false,
        }
    }
    cs.add("风暴限流四", storm_ok && bus4.count_of(1) == 4, "只进 quota 个");
    cs.add("风暴守恒", bus4.reconcile(), "10 发 = 4 收 + 6 丢");
    cs.add("零去重", bus4.n_dup == 0, "向量各异无重复");

    // 族七：多源广播同向量（各源发同向量给同目标——首源收余源重）。
    let mut bus5 = IpiBus::new(3, 8);
    bus5.send(0, 2, 0x60).ok();
    cs.add("源 0 首达", bus5.count_of(2) == 1, "先到先收");
    cs.add("源 1 重复去重", bus5.send(1, 2, 0x60) == Ok(false), "同 (to,vec) 拒");
    cs.add("源 2 同样拒", bus5.send(2, 2, 0x60) == Ok(false), "第三源仍拒");
    cs.add("去重账二", bus5.n_dup == 2, "两源去重");
    cs.add("异源异量通", bus5.send(1, 2, 0x61).is_ok(), "换向量即收");

    // 族八：全员互播（每核发向量给其他核——星形全联）。
    let mut bus6 = IpiBus::new(4, 8);
    let mut mesh_ok = true;
    for from in 0..4u8 {
        for to in 0..4u8 {
            if from != to && bus6.send(from, to, 0x70).is_err() {
                mesh_ok = false;
            }
        }
    }
    cs.add("十二发全达", mesh_ok && bus6.count_of(0) == 3, "每核收三源");
    cs.add("全联守恒", bus6.reconcile(), "12 发 = 12 收");
    cs.add("广播复刻核", bus6.broadcast(0, 0x70) == Ok(0), "再播全去重");

    cs
}
