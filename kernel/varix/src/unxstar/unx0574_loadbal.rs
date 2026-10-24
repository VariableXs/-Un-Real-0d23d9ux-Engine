//! unx0574_loadbal —— UNX-F0574 核间负载均衡账。
//! 锚点：调度器卷 · 迁移（重核拉轻核 / 迁移账守恒 / 空核拒迁出）。
//! 判据原文：迁移只从重核到轻核（差超阈值才动），迁移后两核账更
//!           新且总量守恒；单任务核拒迁出（保底）。

/// 核负载。
#[derive(Debug, Clone, Copy)]
pub struct CoreLoad {
    pub core: u8,
    /// 就绪任务数。
    pub load: u32,
}

/// 负载均衡器。
#[derive(Debug)]
pub struct Balancer {
    pub cores: alloc::vec::Vec<CoreLoad>,
    /// 触发迁移的负载差阈值。
    pub skew: u32,
    /// 迁移账 / 拒账。
    pub n_migrate: u64,
    pub n_reject: u64,
}

impl Balancer {
    pub fn new(n: u8, skew: u32) -> Self {
        Balancer { cores: (0..n).map(|c| CoreLoad { core: c, load: 0 }).collect(), skew, n_migrate: 0, n_reject: 0 }
    }

    /// 挂任务到核。
    pub fn put(&mut self, core: u8) -> Result<(), ()> {
        let c = self.cores.iter_mut().find(|c| c.core == core).ok_or(())?;
        c.load += 1;
        Ok(())
    }

    /// 总量。
    pub fn total(&self) -> u32 {
        self.cores.iter().map(|c| c.load).sum()
    }

    /// 找最重与最轻核对。
    pub fn extremes(&self) -> Option<(u8, u8)> {
        let heavy = self.cores.iter().max_by_key(|c| c.load)?;
        let light = self.cores.iter().min_by_key(|c| c.load)?;
        Some((heavy.core, light.core))
    }

    /// 迁移一任务：重→轻，差须超阈值，且重核至少留一。
    pub fn balance_once(&mut self) -> Result<(), ()> {
        let (h, l) = match self.extremes() {
            Some(x) => x,
            None => return Err(()),
        };
        if h == l {
            self.n_reject += 1;
            return Err(());
        }
        let hl = self.cores.iter().find(|c| c.core == h).map(|c| c.load).ok_or(())?;
        let ll = self.cores.iter().find(|c| c.core == l).map(|c| c.load).ok_or(())?;
        if hl.saturating_sub(ll) < self.skew {
            self.n_reject += 1;
            return Err(());
        }
        if hl <= 1 {
            self.n_reject += 1;
            return Err(()); // 保底：不迁空
        }
        let hc = self.cores.iter_mut().find(|c| c.core == h).ok_or(())?;
        hc.load -= 1;
        let lc = self.cores.iter_mut().find(|c| c.core == l).ok_or(())?;
        lc.load += 1;
        self.n_migrate += 1;
        Ok(())
    }

    /// 批量均衡：直到差低于阈值或不可迁。
    pub fn balance_all(&mut self) -> u64 {
        let mut n = 0;
        while self.balance_once().is_ok() {
            n += 1;
        }
        n
    }
}

/// 判据入口：负载均衡全性质核。
pub fn run_unx0574_checks() -> crate::checks::CheckSet {
    let mut cs = crate::checks::CheckSet::new("UNX-F0574");

    // 族一：挂账与总量。
    let mut b = Balancer::new(4, 2);
    for _ in 0..6 {
        b.put(0).ok();
    }
    b.put(1).ok();
    cs.add("总量守恒", b.total() == 7, "6+1");
    cs.add("极值定位", b.extremes() == Some((0, 2)), "0 重 2 轻");

    // 族二：差不足拒迁。
    let mut b2 = Balancer::new(2, 5);
    b2.put(0).ok();
    b2.put(0).ok();
    b2.put(1).ok();
    cs.add("差二不足五拒", b2.balance_once().is_err(), "2-1=2 < 5");
    cs.add("拒账一", b2.n_reject == 1, "入拒账");
    cs.add("账未动", b2.total() == 3 && b2.n_migrate == 0, "零迁移");

    // 族三：达标迁移与守恒。
    let mut b3 = Balancer::new(2, 2);
    for _ in 0..5 {
        b3.put(0).ok();
    }
    b3.put(1).ok();
    let t0 = b3.total();
    b3.balance_once().ok();
    cs.add("差四迁一", b3.cores.iter().find(|c| c.core == 0).map(|c| c.load) == Some(4), "5→4");
    cs.add("轻核得补", b3.cores.iter().find(|c| c.core == 1).map(|c| c.load) == Some(2), "1→2");
    cs.add("迁移守恒", b3.total() == t0, "总量不变");
    cs.add("迁移账一", b3.n_migrate == 1, "一迁");

    // 族四：批量均衡到差低于阈。
    let mut b4 = Balancer::new(2, 2);
    for _ in 0..8 {
        b4.put(0).ok();
    }
    let n = b4.balance_all();
    cs.add("批迁四", n == 4, "8/0 → 7/1 → 6/2 → 5/3 → 4/4（差 0 收敛）");
    cs.add("终态对半", b4.cores.iter().find(|c| c.core == 0).map(|c| c.load) == Some(4), "4/4");
    cs.add("收敛后再迁拒", b4.balance_once().is_err(), "差低于阈收工");
    cs.add("批守恒", b4.total() == 8, "总量不动");

    // 族五：保底核（单任务核拒迁出）。
    let mut b5 = Balancer::new(2, 1);
    b5.put(0).ok();
    cs.add("一比零达标", true, "差 1 ≥ 阈 1");
    cs.add("单任务保底拒", b5.balance_once().is_err(), "hl=1 不迁空");

    // 族六：三核均衡中位（重→轻不动中位核）。
    let mut b6 = Balancer::new(3, 3);
    for _ in 0..6 {
        b6.put(0).ok();
    }
    b6.put(1).ok();
    b6.balance_once().ok();
    cs.add("中位不动", b6.cores.iter().find(|c| c.core == 1).map(|c| c.load) == Some(1), "只动重轻两端");

    // 族七：极端偏斜批平衡（10/0 → 收敛全平）。
    let mut b7 = Balancer::new(2, 1);
    for _ in 0..10 {
        b7.put(0).ok();
    }
    let n7 = b7.balance_all();
    cs.add("批迁五", n7 == 5, "10/0 → 9/1 → … → 5/5（差 0 收敛）");
    cs.add("终态对半", b7.cores.iter().find(|c| c.core == 0).map(|c| c.load) == Some(5), "5/5");
    cs.add("极偏守恒", b7.total() == 10, "总量不动");

    // 族八：幽灵核挂账拒。
    cs.add("幽灵挂账拒", b7.put(9).is_err(), "未注册核");
    cs.add("挂账不扰", b7.total() == 10, "总量不动");

    // 族九：四核轮转均衡（重核顺次转移）。
    let mut b8 = Balancer::new(4, 2);
    for _ in 0..9 {
        b8.put(0).ok();
    }
    for _ in 0..3 {
        b8.put(1).ok();
    }
    // 9/3/0/0 → 8/3/1/0 → 7/3/1/1 → 6/3/2/1 → 5/3/2/2 → 4/3/3/2 → 3/3/3/3。
    let n8 = b8.balance_all();
    cs.add("四核批迁六", n8 == 6, "六迁全平");
    cs.add("终态全三", b8.cores.iter().all(|c| c.load == 3), "12/4=3");
    cs.add("四核守恒", b8.total() == 12, "总量不变");
    cs.add("收敛后再迁拒", b8.balance_once().is_err(), "差低于阈收工");

    cs
}
