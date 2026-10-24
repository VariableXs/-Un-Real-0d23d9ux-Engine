//! unx0575_schedtick —— UNX-F0575 调度时钟节拍账。
//! 锚点：调度器卷 · 节拍源（tick 分发 / 换算账 / 丢拍补偿）。
//! 判据原文：节拍单调不回退，批量补拍一次入账（不逐拍计）；
//!           拍-毫秒换算带余数截断核；丢拍上限封顶。

/// 节拍源。
#[derive(Debug)]
pub struct TickSrc {
    /// 当前拍号。
    pub now: u64,
    /// 每拍毫秒数。
    pub ms_per_tick: u32,
    /// 丢拍上限（一次补拍最大值）。
    pub max_catchup: u64,
    /// 补拍账 / 丢拍账。
    pub n_catchup: u64,
    pub n_dropped: u64,
}

impl TickSrc {
    pub fn new(ms_per_tick: u32, max_catchup: u64) -> Self {
        TickSrc { now: 0, ms_per_tick, max_catchup, n_catchup: 0, n_dropped: 0 }
    }

    /// 单拍推进（单调核：now 只增）。
    pub fn tick(&mut self) -> u64 {
        self.now += 1;
        self.now
    }

    /// 丢拍补偿：目标拍号必须大于当前（回退拒），补偿量封顶。
    pub fn catchup(&mut self, target: u64) -> Result<u64, ()> {
        if target <= self.now {
            self.n_dropped += 1;
            return Err(()); // 回退/原地拒
        }
        let want = target - self.now;
        let grant = if want > self.max_catchup { self.max_catchup } else { want };
        self.now += grant;
        self.n_catchup += 1;
        if want > self.max_catchup {
            self.n_dropped += want - self.max_catchup;
        }
        Ok(grant)
    }

    /// 拍 → 毫秒（截断核：整除后的余拍不入账）。
    pub fn to_ms(&self, ticks: u64) -> u64 {
        ticks * self.ms_per_tick as u64
    }

    /// 毫秒 → 拍（向下取整）。
    pub fn from_ms(&self, ms: u64) -> u64 {
        ms / self.ms_per_tick as u64
    }
}

/// 判据入口：调度节拍全性质核。
pub fn run_unx0575_checks() -> crate::checks::CheckSet {
    let mut cs = crate::checks::CheckSet::new("UNX-F0575");

    // 族一：节拍单调。
    let mut t = TickSrc::new(10, 100);
    let a = t.tick();
    let b = t.tick();
    let c = t.tick();
    cs.add("单调递增", a < b && b < c, "1→2→3");
    cs.add("拍号对齐", c == 3, "三拍三号");

    // 族二：补拍。
    t.catchup(100).ok();
    cs.add("补到百", t.now == 100, "一次跳 97");
    cs.add("补拍账一", t.n_catchup == 1, "一次补拍");
    cs.add("回退拒", t.catchup(50).is_err(), "目标落后");
    cs.add("原地拒", t.catchup(100).is_err(), "目标持平");
    cs.add("回退入账", t.n_dropped == 2, "两拒两账");

    // 族三：补拍封顶（want 1000 > cap 100）。
    let mut t2 = TickSrc::new(1, 100);
    let grant = t2.catchup(1000);
    cs.add("封顶一百", grant == Ok(100) && t2.now == 100, "只补 100");
    cs.add("差额入丢账", t2.n_dropped == 900, "900 丢");
    cs.add("续补到标", t2.catchup(1000) == Ok(100), "再补 100");
    cs.add("三次补齐", t2.catchup(1000) == Ok(100) && t2.now == 300, "第三次仍封顶");

    // 族四：拍-毫秒换算。
    let t3 = TickSrc::new(10, 100);
    cs.add("十拍百毫秒", t3.to_ms(10) == 100, "10×10");
    cs.add("五十五毫秒五拍", t3.from_ms(55) == 5, "55/10 截断");
    cs.add("整除核", t3.from_ms(50) == 5, "50/10=5");
    cs.add("余数丢弃", t3.from_ms(59) == 5, "59→5 不入");

    // 族五：换算往返守恒（拍→毫秒→拍 恒等）。
    let t4 = TickSrc::new(4, 100);
    let mut round_ok = true;
    for k in 1..=20u64 {
        if t4.from_ms(t4.to_ms(k)) != k {
            round_ok = false;
        }
    }
    cs.add("二十轮往返", round_ok, "拍毫互转恒等");

    // 族六：丢拍后节拍续单调（补拍后 tick 接力不乱）。
    let mut t5 = TickSrc::new(10, 50);
    t5.catchup(60).ok();
    let n1 = t5.tick();
    let n2 = t5.tick();
    cs.add("补后续拍", n1 == 61 && n2 == 62, "61→62 接力");
    cs.add("续拍单调", n1 < n2, "仍单调");

    // 族七：毫秒零与拍零。
    cs.add("零拍零毫秒", t5.to_ms(0) == 0, "0×10=0");
    cs.add("零毫秒零拍", t5.from_ms(0) == 0, "0/10=0");

    // 族八：封顶账对称核（补两轮封顶，丢账累计两轮差额）。
    let mut t6 = TickSrc::new(1, 10);
    let _ = t6.catchup(25);
    let _ = t6.catchup(25);
    cs.add("两轮补二十", t6.now == 20, "2×10");
    cs.add("两轮丢三十", t6.n_dropped == 30, "2×15 差额");
    cs.add("补账二", t6.n_catchup == 2, "两次补拍");

    // 族九：零额补拍（max_catchup=0 恒拒实质推进）。
    let mut t7 = TickSrc::new(10, 0);
    cs.add("零额补拍空转", t7.catchup(50) == Ok(0), "grant=0");
    cs.add("零额不进", t7.now == 0, "拍号不动");
    cs.add("零额差额全丢", t7.n_dropped == 50, "50 全丢");
    cs.add("零额补账仍计", t7.n_catchup == 1, "补动作照记");

    // 族十：大数换算核（u64 远端）。
    let t8 = TickSrc::new(1000, u64::MAX);
    cs.add("万拍千万毫秒", t8.to_ms(10_000) == 10_000_000, "×1000");
    cs.add("亿毫秒十万拍", t8.from_ms(100_000_000) == 100_000, "÷1000");
    cs.add("大数无溢出", t8.to_ms(1_000_000) == 1_000_000_000, "u64 安全");

    // 族十一：混合单拍与补拍接力（tick 与 catchup 交错单调）。
    let mut t9 = TickSrc::new(5, 100);
    let mut mono_ok = true;
    let mut last = 0u64;
    for round in 0..5u64 {
        let a = t9.tick();
        if a <= last {
            mono_ok = false;
        }
        last = a;
        let target = last + 7;
        if let Ok(g) = t9.catchup(target) {
            if t9.now != last + g {
                mono_ok = false;
            }
            last = t9.now;
        }
    }
    cs.add("交错单调", mono_ok && t9.now >= 40, "拍补交替不回退");

    cs
}
