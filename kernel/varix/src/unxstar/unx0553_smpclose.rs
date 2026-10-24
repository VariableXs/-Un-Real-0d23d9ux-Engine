//! unx0553_smpclose —— UNX-F0553 SMP 全链收口。
//! 锚点：SMP 卷 · 六段链（SIPI 发 → 到达 → 点名 → 屏障 → IPI 广播 → 能态收口）。
//! 判据原文：前段产物喂后段（未到核不点名、未点名不进屏障）；
//!           全链分段账守恒，收口压力（四核全链）全绿。

/// 全链引擎。
#[derive(Debug, Default)]
pub struct SmpChain {
    /// 编排表。
    pub roster: alloc::vec::Vec<u8>,
    /// SIPI 已发核。
    pub sent: alloc::vec::Vec<u8>,
    /// 已达核。
    pub arrived: alloc::vec::Vec<u8>,
    /// 已点名核。
    pub rolled: alloc::vec::Vec<u8>,
    /// 屏障代号。
    pub gen: u64,
    /// 屏障聚合位（本代进位计数）。
    pub bar_marks: u64,
    /// IPI 收账（核 → 收量）。
    pub ipi_in: alloc::vec::Vec<(u8, u64)>,
    /// 分段拒账。
    pub r_sipi: u64,
    pub r_roll: u64,
    pub r_bar: u64,
}

impl SmpChain {
    pub fn new(roster: &[u8]) -> Self {
        SmpChain {
            roster: roster.to_vec(),
            sent: alloc::vec::Vec::new(),
            arrived: alloc::vec::Vec::new(),
            rolled: alloc::vec::Vec::new(),
            gen: 0,
            ipi_in: roster.iter().map(|c| (*c, 0u64)).collect(),
            r_sipi: 0, r_roll: 0, r_bar: 0,
            bar_marks: 0,
        }
    }

    /// 段一：发 SIPI（册内核幂等一发）。
    pub fn sipi(&mut self, id: u8) -> Result<(), ()> {
        if !self.roster.contains(&id) || self.sent.contains(&id) {
            self.r_sipi += 1;
            return Err(());
        }
        self.sent.push(id);
        Ok(())
    }

    /// 段二：到达（须已发）。
    pub fn arrive(&mut self, id: u8) -> Result<(), ()> {
        if !self.sent.contains(&id) || self.arrived.contains(&id) {
            self.r_sipi += 1;
            return Err(());
        }
        self.arrived.push(id);
        Ok(())
    }

    /// 段三：点名（须已达）。
    pub fn roll(&mut self, id: u8) -> Result<(), ()> {
        if !self.arrived.contains(&id) || self.rolled.contains(&id) {
            self.r_roll += 1;
            return Err(());
        }
        self.rolled.push(id);
        Ok(())
    }

    /// 段四：屏障（须已点名，全员到齐翻代）。聚合位账：本代进位计数。
    pub fn barrier(&mut self, id: u8) -> Result<bool, ()> {
        if !self.rolled.contains(&id) {
            self.r_bar += 1;
            return Err(());
        }
        let marks = self.bar_marks + 1;
        self.bar_marks = marks;
        if marks == self.rolled.len() as u64 {
            self.bar_marks = 0;
            self.gen += 1;
            Ok(true)
        } else {
            Ok(false)
        }
    }

    /// 段五：IPI 广播（仅已点名核间，全员各收一）。
    pub fn ipi_all(&mut self) -> Result<usize, ()> {
        let n = self.rolled.len();
        if n < 2 {
            return Err(()); // 单核无广播意义
        }
        for (c, v) in self.ipi_in.iter_mut() {
            if self.rolled.contains(c) {
                *v += 1;
            }
        }
        Ok(n)
    }

    /// 全链分段账核。
    pub fn reconcile(&self) -> bool {
        self.rolled.iter().all(|id| self.arrived.contains(id))
            && self.arrived.iter().all(|id| self.sent.contains(id))
            && self.sent.iter().all(|id| self.roster.contains(id))
    }
}

/// 判据入口：SMP 全链收口核。
pub fn run_unx0553_checks() -> crate::checks::CheckSet {
    let mut cs = crate::checks::CheckSet::new("UNX-F0553");

    // 族一：段序违例拒。
    let mut c = SmpChain::new(&[0, 1, 2]);
    cs.add("裸点名拒", c.roll(0).is_err(), "未达不点名");
    cs.add("裸屏障拒", c.barrier(0).is_err(), "未点名不进屏障");
    cs.add("幽灵 SIPI 拒", c.sipi(9).is_err(), "册外不发");
    cs.add("拒账齐", c.r_sipi == 1 && c.r_roll == 1 && c.r_bar == 1, "三段各一拒");

    // 族二：正链推进。
    c.sipi(0).ok();
    cs.add("重发拒", c.sipi(0).is_err(), "幂等");
    c.arrive(0).ok();
    cs.add("未发到达拒", c.arrive(1).is_err(), "段序核");
    c.roll(0).ok();
    cs.add("重复点名拒", c.roll(0).is_err(), "点名幂等");
    cs.add("链核", c.reconcile(), "rolled ⊆ arrived ⊆ sent ⊆ roster");

    // 族三：屏障与 IPI。
    cs.add("单核屏障未翻", c.barrier(0) == Ok(false), "1/3");
    cs.add("单核 IPI 拒", c.ipi_all().is_err(), "不足两核");
    // 补齐另两核。
    for id in [1u8, 2] {
        c.sipi(id).ok();
        c.arrive(id).ok();
        c.roll(id).ok();
    }
    cs.add("屏障翻代", c.barrier(1) == Ok(false) && c.barrier(2) == Ok(true), "3/3 翻");
    cs.add("代进一", c.gen == 1, "代号单调");
    cs.add("全员 IPI", c.ipi_all() == Ok(3), "三核各收一");
    cs.add("IPI 账三", c.ipi_in.iter().all(|(_, v)| *v == 1), "各一");

    // 族四：四核全链收口压测。
    let mut d = SmpChain::new(&[0, 1, 2, 3]);
    let mut clean = true;
    for id in 0..4u8 {
        if d.sipi(id).is_err() || d.arrive(id).is_err() || d.roll(id).is_err() {
            clean = false;
        }
    }
    cs.add("四核三段全通", clean && d.reconcile(), "SIPI→达→点名");
    let mut bar_ok = true;
    for id in 0..4u8 {
        if d.barrier(id) != Ok(id == 3) {
            bar_ok = false;
        }
    }
    cs.add("四核屏障翻", bar_ok && d.gen == 1, "4/4 翻代");
    cs.add("四核 IPI", d.ipi_all() == Ok(4), "全员广播");
    cs.add("压测终账", d.reconcile(), "全链核平");

    // 族五：两代全链（第二次屏障需再全员进位）。
    let mut bar2_ok = true;
    for id in 0..4u8 {
        if d.barrier(id) != Ok(id == 3) {
            bar2_ok = false;
        }
    }
    cs.add("二代翻", bar2_ok && d.gen == 2, "4/4 再翻");
    cs.add("IPI 双轮", d.ipi_all() == Ok(4), "二次广播");
    cs.add("IPI 累计账", d.ipi_in.iter().all(|(_, v)| *v == 2), "每核收二");

    // 族六：慢核链（3 号慢到，前段不被拖累）。
    let mut e = SmpChain::new(&[0, 1, 2, 3]);
    e.sipi(0).ok();
    e.arrive(0).ok();
    e.roll(0).ok();
    cs.add("快核先行", e.rolled.len() == 1 && e.reconcile(), "1 核已过三段");
    e.sipi(3).ok();
    e.arrive(3).ok();
    e.roll(3).ok();
    cs.add("慢核后至", e.rolled.len() == 2, "补位不乱账");
    cs.add("两核 IPI 通", e.ipi_all() == Ok(2), "≥2 核即广播");

    cs
}
