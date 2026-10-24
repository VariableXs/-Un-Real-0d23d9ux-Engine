//! unx0570_preempt2 —— UNX-F0570 抢占账。
//! 锚点：调度器卷 · 抢占（高优到达踢低优 / 被抢者回队 / 嵌套抢占深度）。
//! 判据原文：到达者优先级高于在跑者才抢占（等于不抢），被抢者回
//!           原带队头（恢复位次），抢占深度账单调。

/// 抢占调度器（prio 小者高）。
#[derive(Debug)]
pub struct Preempter {
    /// 在跑者（tid, prio）。
    pub running: Option<(u32, u8)>,
    /// 就绪队（回队用，FIFO）。
    pub ready: alloc::vec::Vec<(u32, u8)>,
    /// 抢占深度（当前嵌套层数）。
    pub depth: u64,
    /// 总抢占账 / 嵌套最深账。
    pub n_preempt: u64,
    pub max_depth: u64,
}

impl Preempter {
    pub fn new() -> Self {
        Preempter { running: None, ready: alloc::vec::Vec::new(), depth: 0, n_preempt: 0, max_depth: 0 }
    }

    /// 到达：无在跑直接上；高于在跑者则抢占；否则入就绪队尾。
    pub fn arrive(&mut self, tid: u32, prio: u8) -> bool {
        match self.running {
            None => {
                self.running = Some((tid, prio));
                self.depth = 1;
                if self.depth > self.max_depth {
                    self.max_depth = self.depth;
                }
                true // 直接上
            }
            Some((_, rp)) if prio < rp => {
                // 抢占：被抢者回就绪队头。
                if let Some(old) = self.running.take() {
                    self.ready.insert(0, old);
                }
                self.running = Some((tid, prio));
                self.depth += 1;
                self.n_preempt += 1;
                if self.depth > self.max_depth {
                    self.max_depth = self.depth;
                }
                true // 抢占成功
            }
            _ => {
                self.ready.push((tid, prio));
                false // 排队
            }
        }
    }

    /// 在跑者完成：从就绪队头恢复；无恢复者则空转。
    pub fn finish(&mut self) -> Option<(u32, u8)> {
        let done = self.running.take();
        if let Some(next) = if self.ready.is_empty() { None } else { Some(self.ready.remove(0)) } {
            self.running = Some(next);
        } else {
            self.depth = 0;
        }
        done
    }
}

/// 判据入口：抢占全性质核。
pub fn run_unx0570_checks() -> crate::checks::CheckSet {
    let mut cs = crate::checks::CheckSet::new("UNX-F0570");

    // 族一：空机直接上。
    let mut p = Preempter::new();
    cs.add("空机上跑", p.arrive(1, 5), "无竞争即跑");
    cs.add("深度一", p.depth == 1, "首层");

    // 族二：同优排队不抢。
    cs.add("同优不抢", !p.arrive(2, 5), "等于不抢");
    cs.add("入就绪队", p.ready == alloc::vec![(2u32, 5u8)], "队尾");

    // 族三：高优抢占。
    cs.add("高优抢占", p.arrive(3, 2), "2 < 5 抢");
    cs.add("被抢者回队头", p.ready == alloc::vec![(1u32, 5u8), (2u32, 5u8)], "1 号插回头");
    cs.add("抢占账一", p.n_preempt == 1, "一抢");
    cs.add("深度二", p.depth == 2, "嵌套两层");

    // 族四：完成恢复位次。
    cs.add("完成出 3 号", p.finish() == Some((3u32, 2u8)), "顶层完");
    cs.add("恢复 1 号", p.running == Some((1u32, 5u8)), "回队头接棒");
    cs.add("1 号完出 2 号", { p.finish(); p.running == Some((2u32, 5u8)) }, "队列续位");

    // 族五：三层嵌套深度账。
    let mut q = Preempter::new();
    q.arrive(10, 9);
    q.arrive(11, 7);
    q.arrive(12, 3);
    cs.add("深度三", q.depth == 3, "9→7→3 嵌套");
    cs.add("最深账三", q.max_depth == 3, "峰值记账");
    cs.add("两抢账", q.n_preempt == 2, "两次抢占");
    // 逆序完成回卷。
    q.finish(); // 12 完 → 11 上
    cs.add("回卷 11", q.running == Some((11u32, 7u8)), "次高恢复");
    q.finish(); // 11 完 → 10 上
    cs.add("回卷 10", q.running == Some((10u32, 9u8)), "最低优最后");
    q.finish(); // 10 完 → 空
    cs.add("空转清零", q.running.is_none() && q.depth == 0, "全收尾");

    // 族六：峰值不回退核。
    let mut r = Preempter::new();
    r.arrive(1, 9);
    r.arrive(2, 1); // 深度 2
    r.finish();
    r.finish();
    cs.add("峰值保留", r.max_depth == 2 && r.depth == 0, "历史峰值不褪");

    // 族七：同优多任务 FIFO 恢复序（两同优被先后抢 → 逆序恢复）。
    let mut s = Preempter::new();
    s.arrive(20, 5);
    s.arrive(21, 5); // 队尾
    s.arrive(22, 1); // 抢占 → 20 回队头，21 随后
    cs.add("抢后队序", s.ready == alloc::vec![(20u32, 5u8), (21u32, 5u8)], "20 先恢复");
    s.finish(); // 22 完 → 20 上
    cs.add("恢复 20", s.running == Some((20u32, 5u8)), "队头接棒");
    s.finish(); // 20 完 → 21 上
    cs.add("恢复 21", s.running == Some((21u32, 5u8)), "次位接棒");
    s.finish();
    cs.add("全收空", s.running.is_none(), "三任务全完");

    // 族八：回队后再次被抢（反复嵌套深度峰值刷新）。
    let mut u = Preempter::new();
    u.arrive(30, 9);
    u.arrive(31, 5); // 深 2
    u.arrive(32, 3); // 深 3
    u.arrive(33, 1); // 深 4
    cs.add("四层嵌套", u.depth == 4 && u.max_depth == 4, "连抢三层");
    cs.add("三抢账", u.n_preempt == 3, "三次抢占");
    u.finish();
    u.finish();
    u.finish();
    u.finish();
    cs.add("逆序全收", u.running.is_none() && u.depth == 0, "四层全回卷");
    cs.add("峰值四不褪", u.max_depth == 4, "历史峰值");

    cs
}
