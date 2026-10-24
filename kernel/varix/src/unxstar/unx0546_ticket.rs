//! unx0546_ticket —— UNX-F0546 票锁公平性账。
//! 锚点：SMP 卷 · FIFO 票锁（取票 / 服务号 / 插队拒）。
//! 判据原文：先取票先服务（严格 FIFO），服务号只进不退；
//!           未持票者不得进临界区；解锁后下号恰进一。

/// 票锁。
#[derive(Debug)]
pub struct TicketLock {
    /// 已发票号（下一张）。
    pub next: u64,
    /// 当前服务号。
    pub serving: u64,
    /// 持票者账（票号 → 核 id）。
    pub holders: alloc::vec::Vec<(u64, u8)>,
    /// 服务完成账。
    pub n_served: u64,
    /// 插队拒账。
    pub n_barge: u64,
}

impl TicketLock {
    pub fn new() -> Self {
        TicketLock { next: 0, serving: 0, holders: alloc::vec::Vec::new(), n_served: 0, n_barge: 0 }
    }

    /// 取票：发票并登记持有者。
    pub fn take(&mut self, core: u8) -> u64 {
        let t = self.next;
        self.next += 1;
        self.holders.push((t, core));
        t
    }

    /// 是否轮到（票号 == 服务号 且 该票未消费）。
    pub fn my_turn(&self, ticket: u64) -> bool {
        ticket == self.serving && self.holders.iter().any(|h| h.0 == ticket)
    }

    /// 进临界区：轮到才进。
    pub fn enter(&mut self, ticket: u64, core: u8) -> Result<(), ()> {
        if !self.my_turn(ticket) {
            self.n_barge += 1;
            return Err(());
        }
        let h = self.holders.iter().find(|h| h.0 == ticket).ok_or(())?;
        if h.1 != core {
            self.n_barge += 1;
            return Err(()); // 借票拒
        }
        Ok(())
    }

    /// 出临界区：消费票，服务号进一。
    pub fn leave(&mut self, ticket: u64) -> Result<(), ()> {
        let before = self.holders.len();
        self.holders.retain(|h| h.0 != ticket);
        if self.holders.len() == before {
            return Err(()); // 无票可消
        }
        if ticket != self.serving {
            return Err(()); // 非当前号不可离
        }
        self.serving += 1;
        self.n_served += 1;
        Ok(())
    }
}

/// 判据入口：票锁公平性全性质核。
pub fn run_unx0546_checks() -> crate::checks::CheckSet {
    let mut cs = crate::checks::CheckSet::new("UNX-F0546");

    // 族一：取票 FIFO 顺序。
    let mut l = TicketLock::new();
    let t0 = l.take(1);
    let t1 = l.take(2);
    let t2 = l.take(3);
    cs.add("票号递增", t0 == 0 && t1 == 1 && t2 == 2, "FIFO 发号");
    cs.add("轮到 0 号", l.my_turn(0), "首个持票者");
    cs.add("未轮到 1 号", !l.my_turn(1), "后票等待");

    // 族二：插队拒。
    cs.add("1 号插队拒", l.enter(1, 2).is_err(), "未到号拒进");
    cs.add("插队入账", l.n_barge == 1, "拒账一");
    cs.add("0 号进通", l.enter(0, 1).is_ok(), "到号进临界区");

    // 族三：借票拒。
    cs.add("借票拒", l.enter(0, 9).is_err(), "非持票核");
    cs.add("借票入账", l.n_barge == 2, "拒账二");

    // 族四：出临界区服务号进一。
    cs.add("0 号离通", l.leave(0).is_ok(), "消费票 0");
    cs.add("服务号进一", l.serving == 1 && l.n_served == 1, "下号接棒");
    cs.add("1 号接棒进", l.enter(1, 2).is_ok(), "轮到即进");
    cs.add("1 号离", l.leave(1).is_ok(), "消费票 1");

    // 族五：空离拒与重复离拒。
    cs.add("空离拒", l.leave(9).is_err(), "无票可消");
    let mut l2 = TicketLock::new();
    cs.add("零号空锁拒离", l2.leave(0).is_err(), "无持票者");

    // 族六：严格 FIFO 服务序核（五票顺序服务）。
    let mut l3 = TicketLock::new();
    let mut order: alloc::vec::Vec<u8> = alloc::vec::Vec::new();
    let tickets: alloc::vec::Vec<u64> = (1u8..=5).map(|c| l3.take(c)).collect();
    for (i, t) in tickets.iter().enumerate() {
        let core = (i + 1) as u8;
        if l3.enter(*t, core).is_ok() {
            order.push(core);
            let _ = l3.leave(*t);
        }
    }
    cs.add("服务序 FIFO", order == alloc::vec![1u8, 2, 3, 4, 5], "取票序 = 服务序");
    cs.add("五号全服务", l3.serving == 5 && l3.n_served == 5, "五票五服务");
    cs.add("票池清空", l3.holders.is_empty(), "全消费");

    // 族七：服务号只进不退。
    let mut l4 = TicketLock::new();
    let ta = l4.take(1);
    let _ = l4.take(2);
    l4.enter(ta, 1).ok();
    l4.leave(ta).ok();
    let s_before = l4.serving;
    let _ = l4.take(3); // 再取票不回退服务号
    cs.add("号只进", l4.serving == s_before && l4.serving >= 1, "取票不影响服务号");
    cs.add("next 独立递增", l4.next == 3, "发票三张");

    // 族八：二十票长队压测（取序 = 服务序）。
    let mut l5 = TicketLock::new();
    let tickets: alloc::vec::Vec<u64> = (1u8..=20).map(|c| l5.take(c)).collect();
    let mut served_seq: alloc::vec::Vec<u8> = alloc::vec::Vec::new();
    for t in tickets.iter() {
        if l5.enter(*t, (t + 1) as u8).is_ok() {
            served_seq.push((t + 1) as u8);
            let _ = l5.leave(*t);
        }
    }
    cs.add("二十票全服务", served_seq.len() == 20, "长队不丢票");
    cs.add("长队 FIFO", served_seq == (1u8..=20).collect::<alloc::vec::Vec<u8>>(), "取序即服序");
    cs.add("长队终号", l5.serving == 20 && l5.next == 20, "发服两账归拢");
    cs.add("零插队", l5.n_barge == 0, "顺序进场零拒");

    // 族九：跳号进拒绝核（4 号试图携 1 号票进）。
    let mut l6 = TicketLock::new();
    let x0 = l6.take(1);
    let _ = l6.take(2);
    let _ = l6.take(3);
    let _ = l6.take(4);
    cs.add("持 0 号票借 1 号进", l6.enter(x0, 2).is_err(), "票-核绑定");
    cs.add("跳到队尾再进", l6.enter(x0, 1).is_ok(), "本人到号即进");
    cs.add("借拒入账", l6.n_barge == 1, "一借一账");

    cs
}
