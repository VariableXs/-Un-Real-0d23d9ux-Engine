//! unx0576_schedclose —— UNX-F0576 调度全链收口。
//! 锚点：调度器卷 · 五段链（入队 → 选取 → 片跑 → 片尽降带 → 老化升带）。
//! 判据原文：前段产物喂后段（未入队不选取），链账守恒（提交 = 完成
//!           + 在队 + 等待），收口压力（三任务长跑）全绿。

/// 全链引擎（简化：单带 MLFQ + 轮转 + 老化）。
#[derive(Debug, Default)]
pub struct SchedChain {
    /// 就绪队（FIFO）。
    pub ready: alloc::vec::Vec<(u32, u64)>,
    /// 提交账 / 完成账。
    pub n_admit: u64,
    pub n_done: u64,
    /// 片尽账 / 老化提升账。
    pub n_slice_end: u64,
    pub n_aged: u64,
    /// 老化阈值。
    pub age_boost: u64,
    /// 拒账。
    pub n_reject: u64,
}

impl SchedChain {
    pub fn new(age_boost: u64) -> Self {
        SchedChain { ready: alloc::vec::Vec::new(), n_admit: 0, n_done: 0, n_slice_end: 0, n_aged: 0, age_boost, n_reject: 0 }
    }

    /// 段一：提交入队（重复拒）。
    pub fn admit(&mut self, tid: u32, slice: u64) -> Result<(), ()> {
        if self.ready.iter().any(|r| r.0 == tid) {
            self.n_reject += 1;
            return Err(());
        }
        self.ready.push((tid, slice));
        self.n_admit += 1;
        Ok(())
    }

    /// 段二：选取队头（空拒）。
    pub fn pick(&mut self) -> Result<(u32, u64), ()> {
        if self.ready.is_empty() {
            self.n_reject += 1;
            return Err(());
        }
        Ok(self.ready[0])
    }

    /// 段三/四：跑一片。片尽降（出队尾回环）或完成（出账）。
    /// 返回 (是否完成, 是否片尽)。
    pub fn run_slice(&mut self, tid: u32) -> Result<(bool, bool), ()> {
        let pos = self.ready.iter().position(|r| r.0 == tid).ok_or(())?;
        let slice_left = self.ready[pos].1;
        if slice_left <= 1 {
            // 完成。
            self.ready.remove(pos);
            self.n_done += 1;
            Ok((true, false))
        } else {
            // 片尽：减片回队尾。
            self.ready.remove(pos);
            self.ready.push((tid, slice_left - 1));
            self.n_slice_end += 1;
            Ok((false, true))
        }
    }

    /// 段五：老化提升（队尾等久者提到队首一次，提升总数封顶 age_boost）。
    pub fn age(&mut self) -> bool {
        if self.ready.len() < 2 || self.n_aged >= self.age_boost {
            return false;
        }
        let last = match self.ready.pop() {
            Some(l) => l,
            None => return false,
        };
        self.ready.insert(0, last);
        self.n_aged += 1;
        true
    }

    /// 守恒：提交 = 完成 + 在队。
    pub fn reconcile(&self) -> bool {
        self.n_admit == self.n_done + self.ready.len() as u64
    }
}

/// 判据入口：调度全链收口核。
pub fn run_unx0576_checks() -> crate::checks::CheckSet {
    let mut cs = crate::checks::CheckSet::new("UNX-F0576");

    // 族一：段序违例拒。
    let mut c = SchedChain::new(0);
    cs.add("空选取拒", c.pick().is_err(), "未入队不选取");
    cs.add("幽灵跑片拒", c.run_slice(9).is_err(), "未入队不跑片");
    cs.add("拒账二", c.n_reject == 2, "两段各一拒");

    // 族二：正链三段。
    c.admit(1, 3).ok();
    c.admit(2, 3).ok();
    cs.add("重复提交拒", c.admit(1, 3).is_err(), "在队重拒");
    cs.add("选取头位", c.pick() == Ok((1u32, 3u64)), "FIFO 首位");
    cs.add("片跑未完", c.run_slice(1) == Ok((false, true)), "片尽回环");
    cs.add("回环到尾", c.ready == alloc::vec![(2u32, 3u64), (1u32, 2u64)], "1 号片减尾接");
    cs.add("片尽账一", c.n_slice_end == 1, "一片一账");

    // 族三：完成出账。
    c.run_slice(2).ok();
    c.run_slice(1).ok();
    c.run_slice(2).ok();
    cs.add("1 号完", c.run_slice(1) == Ok((true, false)), "片尽即完");
    cs.add("完成账一", c.n_done == 1, "一完");
    cs.add("守恒核", c.reconcile(), "提交 = 完成 + 在队");

    // 族四：二任务跑完守恒。
    c.run_slice(2).ok();
    c.run_slice(2).ok();
    cs.add("2 号完", c.run_slice(2) == Ok((true, false)), "全完");
    cs.add("双完守恒", c.n_admit == 2 && c.n_done == 2 && c.ready.is_empty(), "提交二完成二");
    cs.add("空机再选拒", c.pick().is_err(), "全清后拒");

    // 族五：三任务长跑压力（各片 3 → 9 片全完）。
    let mut d = SchedChain::new(0);
    d.admit(10, 3).ok();
    d.admit(11, 3).ok();
    d.admit(12, 3).ok();
    let mut rounds = 0;
    while d.n_done < 3 && rounds < 100 {
        let (tid, _) = d.pick().ok().unwrap_or((0, 0));
        let _ = d.run_slice(tid);
        rounds += 1;
    }
    cs.add("九片全完", d.n_done == 3 && rounds == 9, "3 任务 × 3 片");
    cs.add("片尽账六", d.n_slice_end == 6, "9-3 完 = 6 换");
    cs.add("压测守恒", d.reconcile() && d.ready.is_empty(), "全链收平");

    // 族六：老化提升生效核（队尾者被提至队首）。
    let mut e = SchedChain::new(1);
    e.admit(1, 9).ok();
    e.admit(2, 9).ok();
    e.admit(3, 9).ok();
    cs.add("提升触发", e.age(), "队尾 3 号上提");
    cs.add("提升后头位", e.ready[0].0 == 3, "3 号到队首");
    cs.add("提升账一", e.n_aged == 1, "一次提升");
    cs.add("封顶后再提拒", !e.age() && e.n_aged == 1, "age_boost=1 封顶");

    // 族七：提升不破守恒。
    cs.add("提升守恒", e.reconcile(), "提交 = 完成 + 在队");
    cs.add("全员仍在队", e.ready.len() == 3, "无完成无拒");

    // 族八：零阈禁提升。
    let mut f = SchedChain::new(0);
    f.admit(1, 2).ok();
    f.admit(2, 2).ok();
    cs.add("零阈不提升", !f.age() && f.n_aged == 0, "age_boost=0 禁用");
    cs.add("队序不动", f.ready[0].0 == 1, "FIFO 保持");

    cs
}
