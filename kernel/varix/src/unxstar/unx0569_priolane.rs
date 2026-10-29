//! unx0569_priolane —— UNX-F0569 优先级带调度账。
//! 锚点：调度器卷 · 优先级带（高带先服务 / 同带 FIFO / 带越界拒）。
//! 判据原文：选取恒取最高带（带内 FIFO），带号越界拒挂，
//!           提带后立即生效（下一选取即见）。

/// 优先级调度器（0 = 最高带）。
#[derive(Debug)]
pub struct PrioSched {
    /// 带数组（每带一 FIFO）。
    pub bands: alloc::vec::Vec<alloc::vec::Vec<u32>>,
    /// 服务账（tid → 次数）。
    pub served: alloc::vec::Vec<(u32, u64)>,
    /// 拒账。
    pub n_reject: u64,
}

impl PrioSched {
    pub fn new(nbands: usize) -> Self {
        PrioSched { bands: (0..nbands).map(|_| alloc::vec::Vec::new()).collect(), served: alloc::vec::Vec::new(), n_reject: 0 }
    }

    /// 挂任务：带号越界拒。
    pub fn submit(&mut self, band: usize, tid: u32) -> Result<(), ()> {
        if band >= self.bands.len() {
            self.n_reject += 1;
            return Err(());
        }
        self.bands[band].push(tid);
        Ok(())
    }

    /// 选取：最高非空带取头；空调度器拒。
    pub fn pick(&mut self) -> Result<u32, ()> {
        for b in self.bands.iter_mut() {
            if !b.is_empty() {
                let tid = b.remove(0);
                match self.served.iter_mut().find(|s| s.0 == tid) {
                    Some(s) => s.1 += 1,
                    None => self.served.push((tid, 1)),
                }
                return Ok(tid);
            }
        }
        self.n_reject += 1;
        Err(())
    }

    /// 高带任务在队核（是否有比某带更高的带在等）。
    pub fn higher_pending(&self, band: usize) -> bool {
        self.bands.iter().take(band).any(|b| !b.is_empty())
    }
}

/// 判据入口：优先级带全性质核。
pub fn run_unx0569_checks() -> crate::checks::CheckSet {
    let mut cs = crate::checks::CheckSet::new("UNX-F0569");

    // 族一：高带先服务。
    let mut p = PrioSched::new(4);
    p.submit(2, 100).ok();
    p.submit(0, 200).ok();
    p.submit(1, 300).ok();
    cs.add("最高带先取", p.pick() == Ok(200), "带 0 先");
    cs.add("次高带接棒", p.pick() == Ok(300), "带 1 次");
    cs.add("末带收尾", p.pick() == Ok(100), "带 2 尾");

    // 族二：同带 FIFO。
    let mut p2 = PrioSched::new(2);
    p2.submit(1, 10).ok();
    p2.submit(1, 20).ok();
    p2.submit(1, 30).ok();
    cs.add("同带头取", p2.pick() == Ok(10), "FIFO 首位");
    cs.add("同带接棒", p2.pick() == Ok(20), "FIFO 次位");
    cs.add("同带收尾", p2.pick() == Ok(30), "FIFO 末位");

    // 族三：带越界拒。
    cs.add("带越界拒", p2.submit(9, 99).is_err(), "超出带数");
    cs.add("拒账一", p2.n_reject == 1, "入账");

    // 族四：空调度器拒取。
    let mut p3 = PrioSched::new(2);
    cs.add("全空取拒", p3.pick().is_err(), "无任务可取");
    cs.add("空取入拒", p3.n_reject == 1, "拒账一");

    // 族五：高带插队核（低带排队中高带到，下一取即高带）。
    let mut p4 = PrioSched::new(3);
    p4.submit(2, 50).ok();
    cs.add("低带在队", !p4.higher_pending(2), "带 2 之上无待");
    p4.submit(0, 60).ok();
    cs.add("高带插上", p4.higher_pending(2), "带 0 有待");
    cs.add("选取跳带", p4.pick() == Ok(60), "高带即刻生效");

    // 族六：服务账对齐。
    let mut p5 = PrioSched::new(2);
    p5.submit(0, 1).ok();
    p5.submit(0, 1).ok(); // 同 tid 两挂（允许重挂：不同时刻提交）
    p5.pick().ok();
    p5.pick().ok();
    cs.add("服务账二", p5.served == alloc::vec![(1u32, 2u64)], "同 tid 累计二");

    // 族七：高带常驻饿低带核（顶带不断供则低带恒不取）。
    let mut p6 = PrioSched::new(3);
    p6.submit(1, 50).ok();
    p6.submit(2, 60).ok();
    for i in 0..3u32 {
        p6.submit(0, 100 + i).ok();
        p6.pick().ok();
    }
    cs.add("顶带三连取", p6.served.iter().all(|s| s.0 >= 100), "低带全饿");
    cs.add("低带仍在队", !p6.bands[1].is_empty() && !p6.bands[2].is_empty(), "50、60 未动");

    // 族八：低带释放序（顶带清空后依带号顺次）。
    let mut seq = alloc::vec::Vec::new();
    while p6.pick().is_ok() {
        if let Some(s) = p6.served.last() {
            seq.push(s.0);
        }
    }
    cs.add("释放序 50→60", seq == alloc::vec![50u32, 60], "带 1 先带 2 后");
    cs.add("空后再取拒", p6.pick().is_err(), "全空");

    // 族九：多带混合提交选取序核（带决定序、tid 无关）。
    let mut p7 = PrioSched::new(3);
    p7.submit(2, 1).ok();
    p7.submit(0, 9).ok();
    p7.submit(1, 5).ok();
    p7.submit(0, 8).ok();
    let mut order = alloc::vec::Vec::new();
    while let Ok(tid) = p7.pick() {
        order.push(tid);
    }
    cs.add("带序全取", order == alloc::vec![9u32, 8, 5, 1], "0 带 FIFO → 1 → 2");
    cs.add("服务账四", p7.served.len() == 4, "四任务齐账");

    // 族十：服务账深核（同任务多轮提交累计、异任务分立）。
    let mut p8 = PrioSched::new(2);
    let mut deep_ok = true;
    for round in 0..5u32 {
        p8.submit(0, 10 + round).ok();
        p8.submit(1, 20 + round).ok();
        let a = p8.pick().ok();
        let b = p8.pick().ok();
        if a != Some(10 + round) || b != Some(20 + round) {
            deep_ok = false;
        }
    }
    cs.add("十轮双带序", deep_ok, "每轮带 0 先带 1 后");
    cs.add("两任务各五", p8.served == alloc::vec![(10u32, 5u64), (20u32, 5u64)], "账各五");

    // 族十一：peek 型挂账后清带核（全取后 bands 全空）。
    let mut p9 = PrioSched::new(3);
    for band in 0..3usize {
        for k in 0..4u32 {
            p9.submit(band, band as u32 * 100 + k).ok();
        }
    }
    let mut all = 0;
    while p9.pick().is_ok() {
        all += 1;
    }
    cs.add("十二任务全取", all == 12, "3 带 × 4 任务");
    cs.add("全带清空", p9.bands.iter().all(|b| b.is_empty()), "无残留");
    cs.add("清后守恒账", p9.served.iter().map(|s| s.1).sum::<u64>() == 12, "服务账合计 12");

    // 族十二：单带退化核（nbands=1 纯 FIFO）。
    let mut pa = PrioSched::new(1);
    for tid in [7u32, 3, 9, 1] {
        pa.submit(0, tid).ok();
    }
    let mut fifo_ok = true;
    for expect in [7u32, 3, 9, 1] {
        if pa.pick() != Ok(expect) {
            fifo_ok = false;
        }
    }
    cs.add("单带纯 FIFO", fifo_ok, "无带序干扰");
    cs.add("越下带拒", pa.submit(0, 5).is_ok() && pa.submit(1, 6).is_err(), "仅带 0 合法");

    cs
}
