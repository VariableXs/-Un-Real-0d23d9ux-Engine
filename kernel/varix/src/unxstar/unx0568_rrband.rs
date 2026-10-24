//! unx0568_rrband —— UNX-F0568 轮转调度时间片账。
//! 锚点：调度器卷 · 轮转（片耗换出回队尾 / 未耗让核先回）。
//! 判据原文：时间片耗尽才换出（提前换出拒），换出者回队尾（回环），
//!           单任务独占连跑多片；片账守恒。

/// 轮转调度器。
#[derive(Debug)]
pub struct RrSched {
    /// 环形队列（tid）。
    pub ring: alloc::vec::Vec<u32>,
    /// 当前在跑者下标。
    pub cur: usize,
    /// 当前片剩余。
    pub slice_left: u32,
    /// 每片长度。
    pub slice_len: u32,
    /// 换出账 / 片消耗账。
    pub n_switch: u64,
    pub n_ticks: u64,
}

impl RrSched {
    pub fn new(tids: &[u32], slice_len: u32) -> Self {
        RrSched { ring: tids.to_vec(), cur: 0, slice_left: slice_len, slice_len, n_switch: 0, n_ticks: 0 }
    }

    /// 当前在跑者。
    pub fn running(&self) -> Option<u32> {
        self.ring.get(self.cur).copied()
    }

    /// tick：消耗当前片；片尽自动换出（回环）。
    pub fn tick(&mut self) {
        self.n_ticks += 1;
        if self.slice_left > 1 {
            self.slice_left -= 1;
        } else {
            self.slice_left = self.slice_len;
            self.cur = (self.cur + 1) % self.ring.len();
            self.n_switch += 1;
        }
    }

    /// 手动提前换出：非片尽状态拒。
    pub fn force_switch(&mut self) -> Result<(), ()> {
        if self.slice_left != self.slice_len && self.slice_left > 0 {
            return Err(()); // 片未尽拒强切
        }
        self.cur = (self.cur + 1) % self.ring.len();
        self.n_switch += 1;
        Ok(())
    }

    /// 轮转完整圈：全员各跑一片后回到起点。
    pub fn full_round(&mut self) -> usize {
        let start = self.running();
        let mut switches = 0;
        for _ in 0..self.ring.len() {
            for _ in 0..self.slice_len {
                self.tick();
            }
            switches += 1;
            if self.running() == start {
                break;
            }
        }
        switches
    }
}

/// 判据入口：轮转调度全性质核。
pub fn run_unx0568_checks() -> crate::checks::CheckSet {
    let mut cs = crate::checks::CheckSet::new("UNX-F0568");

    // 族一：片内连跑。
    let mut r = RrSched::new(&[1, 2, 3], 3);
    cs.add("首发 1 号", r.running() == Some(1), "起点头位");
    r.tick();
    r.tick();
    cs.add("片内不换", r.running() == Some(1) && r.n_switch == 0, "3 片长跑 2 tick");
    cs.add("片账一", r.slice_left == 1, "3-2");

    // 族二：片尽自动换出回环。
    r.tick();
    cs.add("片尽换 2 号", r.running() == Some(2) && r.n_switch == 1, "回环下一位");
    cs.add("片重置", r.slice_left == 3, "新片满额");

    // 族三：整圈轮转（回到起点）。
    let switches = r.full_round();
    cs.add("整圈换出三", switches == 3, "3 任务 3 换");
    cs.add("回起点", r.running() == Some(1), "回环闭合");
    cs.add("换出账累计", r.n_switch >= 4, "自动账累计");

    // 族四：提前强切拒（片未尽）。
    let mut r2 = RrSched::new(&[1, 2], 5);
    r2.tick();
    r2.tick();
    cs.add("片未尽强切拒", r2.force_switch().is_err(), "3/5 剩余");
    cs.add("片尽点强切通", { for _ in 0..3 { r2.tick(); } r2.force_switch().is_ok() }, "片归零点放行");

    // 族五：单任务独占（换出即自环）。
    let mut r3 = RrSched::new(&[7], 2);
    r3.tick();
    r3.tick();
    cs.add("独占自环", r3.running() == Some(7), "换出回自己");
    cs.add("独占换出账", r3.n_switch == 1, "一换一回");

    // 族六：长跑守恒（60 tick / 3 任务 / 片 2 → 各跑 20）。
    let mut r4 = RrSched::new(&[1, 2, 3], 2);
    let mut counts = [0u64; 3];
    for _ in 0..60 {
        if let Some(w) = r4.running() {
            counts[(w - 1) as usize] += 1;
        }
        r4.tick();
    }
    cs.add("均分六十八", counts == [20, 20, 20], "轮转绝对公平");
    cs.add("换出账三十", r4.n_switch == 30, "60 tick / 片 2");

    // 族七：片长一（每 tick 换人）。
    let mut r5 = RrSched::new(&[1, 2, 3], 1);
    let mut seq = alloc::vec::Vec::new();
    for _ in 0..6 {
        if let Some(w) = r5.running() {
            seq.push(w);
        }
        r5.tick();
    }
    cs.add("片一快轮", seq == alloc::vec![1u32, 2, 3, 1, 2, 3], "每拍换人回环");
    cs.add("快轮换账六", r5.n_switch == 6, "六拍六换");

    // 族八：双任务乒乓（片 4 交替核）。
    let mut r6 = RrSched::new(&[5, 6], 4);
    let mut ping_ok = true;
    for half in 0..4u64 {
        let expect = if half % 2 == 0 { 5 } else { 6 };
        for _ in 0..4 {
            if r6.running() != Some(expect) {
                ping_ok = false;
            }
            r6.tick();
        }
    }
    cs.add("乒乓四轮", ping_ok, "片 4 严格交替");
    cs.add("乒乓换账八", r6.n_switch == 8, "16 tick / 4 片");

    // 族九：tick 账守恒（拍号 = 总 tick）。
    let mut r7 = RrSched::new(&[1, 2], 3);
    for _ in 0..17 {
        r7.tick();
    }
    cs.add("拍账十七", r7.n_ticks == 17, "逐拍累计");

    cs
}
