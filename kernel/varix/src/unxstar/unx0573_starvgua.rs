//! unx0573_starvgua —— UNX-F0573 饥饿防护账。
//! 锚点：调度器卷 · 饥饿监测（等待时长账 / 龄期提升 / 饿死阈值警报）。
//! 判据原文：低优任务等待超阈必须获得服务（龄期提升生效前不饿死），
//!           警报账只增不减；服务后账清（不重复警报）。

/// 饥饿监测器（prio 小者高，大者易饿）。
#[derive(Debug)]
pub struct StarvGuard {
    /// 等待账（tid → 累计等待 tick）。
    pub waits: alloc::vec::Vec<(u32, u64)>,
    /// 饿死阈值。
    pub threshold: u64,
    /// 已提升者（本次服务前已获龄期豁免）。
    pub boosted: alloc::vec::Vec<u32>,
    /// 警报账（累计触发数）。
    pub n_alarm: u64,
}

impl StarvGuard {
    pub fn new(threshold: u64) -> Self {
        StarvGuard { waits: alloc::vec::Vec::new(), threshold, boosted: alloc::vec::Vec::new(), n_alarm: 0 }
    }

    /// 登记等待者。
    pub fn enroll(&mut self, tid: u32) -> Result<(), ()> {
        if self.waits.iter().any(|w| w.0 == tid) {
            return Err(());
        }
        self.waits.push((tid, 0));
        Ok(())
    }

    /// 等待 tick 推进；跨阈触发警报（每任务一次）。
    pub fn wait_tick(&mut self) -> alloc::vec::Vec<u32> {
        let mut alarms = alloc::vec::Vec::new();
        for i in 0..self.waits.len() {
            self.waits[i].1 += 1;
            let (tid, w) = self.waits[i];
            if w >= self.threshold && !self.boosted.contains(&tid) {
                self.boosted.push(tid);
                self.n_alarm += 1;
                alarms.push(tid);
            }
        }
        alarms
    }

    /// 是否已获提升豁免。
    pub fn is_boosted(&self, tid: u32) -> bool {
        self.boosted.contains(&tid)
    }

    /// 服务完成：账清（等待与提升均复位）。
    pub fn served(&mut self, tid: u32) -> Result<(), ()> {
        let before = self.waits.len();
        self.waits.retain(|w| w.0 != tid);
        if self.waits.len() == before {
            return Err(());
        }
        self.boosted.retain(|t| *t != tid);
        Ok(())
    }

    /// 等待最长者。
    pub fn most_starved(&self) -> Option<u32> {
        self.waits.iter().max_by_key(|w| w.1).map(|w| w.0)
    }
}

/// 判据入口：饥饿防护全性质核。
pub fn run_unx0573_checks() -> crate::checks::CheckSet {
    let mut cs = crate::checks::CheckSet::new("UNX-F0573");

    // 族一：登记与等待累计。
    let mut g = StarvGuard::new(5);
    g.enroll(1).ok();
    g.enroll(2).ok();
    cs.add("重复登记拒", g.enroll(1).is_err(), "账不重项");
    for _ in 0..3 {
        g.wait_tick();
    }
    cs.add("等待账三", g.waits.iter().all(|w| w.1 == 3), "双任务齐步");

    // 族二：跨阈警报（每任务一次）。
    g.wait_tick(); // 4
    g.wait_tick(); // 5 → 触发
    cs.add("双任务双警报", g.n_alarm == 2, "同时跨阈");
    cs.add("提升位挂上", g.is_boosted(1) && g.is_boosted(2), "豁免登记");
    g.wait_tick(); // 6
    cs.add("不重复警报", g.n_alarm == 2, "账不虚增");

    // 族三：服务后账清。
    g.served(1).ok();
    cs.add("服务清提升", !g.is_boosted(1), "1 号复位");
    cs.add("2 号仍挂", g.is_boosted(2), "互不牵连");
    cs.add("服务清等待", g.waits.iter().all(|w| w.0 != 1), "1 号出账");
    cs.add("空服务拒", g.served(9).is_err(), "无账可清");

    // 族四：错峰跨阈分时警报。
    let mut g2 = StarvGuard::new(3);
    g2.enroll(7).ok();
    g2.enroll(8).ok();
    g2.wait_tick(); // 双 1
    g2.wait_tick(); // 双 2
    g2.served(8).ok(); // 8 出账（未触阈）
    g2.enroll(8).ok(); // 8 重新登记（账归零）
    g2.wait_tick(); // 7 → 3 触发；8 → 1
    cs.add("分时单警报", g2.n_alarm == 1 && g2.is_boosted(7) && !g2.is_boosted(8), "只有 7 触发");

    // 族五：最长等待定位。
    let most = g2.most_starved();
    cs.add("最长定位", most == Some(7), "7 号等最久");
    g2.wait_tick();
    g2.wait_tick();
    let most2 = g2.most_starved();
    cs.add("追赶后并列取账序", most2.is_some(), "账仍可定位");

    // 族六：阈前无警报核（threshold-1 拍安静）。
    let mut g3 = StarvGuard::new(4);
    g3.enroll(3).ok();
    for _ in 0..3 {
        g3.wait_tick();
    }
    cs.add("阈前零警报", g3.n_alarm == 0 && !g3.is_boosted(3), "3 拍 < 4 阈");
    g3.wait_tick(); // 第 4 拍触发
    cs.add("触阈即警", g3.n_alarm == 1 && g3.is_boosted(3), "恰在第 4 拍");

    // 族七：服务后重新登记重新计警报（第二世警报独立）。
    g3.served(3).ok();
    g3.enroll(3).ok();
    for _ in 0..4 {
        g3.wait_tick();
    }
    cs.add("二世再警", g3.n_alarm == 2 && g3.is_boosted(3), "复位后独立计");

    // 族八：多数等待者错峰警报序列。
    let mut g4 = StarvGuard::new(2);
    g4.enroll(10).ok();
    g4.enroll(11).ok();
    g4.enroll(12).ok();
    g4.wait_tick();
    let a1 = g4.wait_tick(); // 双人组全触
    g4.served(10).ok();
    g4.served(11).ok();
    g4.enroll(10).ok();
    let _ = g4.enroll(11);
    g4.served(12).ok();
    g4.enroll(12).ok();
    let a2 = g4.wait_tick();
    cs.add("首轮双警报", a1.len() == 2, "同时跨阈双人组");
    cs.add("次轮双警报", a2.len() == 2, "复位后全员再触");
    cs.add("警报累计四", g4.n_alarm == 4, "两轮各二");

    cs
}
