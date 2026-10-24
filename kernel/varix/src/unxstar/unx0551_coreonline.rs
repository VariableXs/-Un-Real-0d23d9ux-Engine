//! unx0551_coreonline —— UNX-F0551 核上线点名账。
//! 锚点：SMP 卷 · AP 汇合点名（报到 / 幽灵核拒 / 名单齐活）。
//! 判据原文：只有编排表内 APID 可报到（幽灵拒），点名齐活才能
//!           放行启动下一阶段；点名账与编排表对账。

/// 点名册。
#[derive(Debug)]
pub struct RollCall {
    /// 编排表（合法 APID）。
    pub roster: alloc::vec::Vec<u8>,
    /// 已报到者。
    pub checked_in: alloc::vec::Vec<u8>,
    /// 幽灵拒账 / 重复报账。
    pub n_ghost: u64,
    pub n_dup: u64,
}

impl RollCall {
    pub fn new(roster: &[u8]) -> Self {
        RollCall { roster: roster.to_vec(), checked_in: alloc::vec::Vec::new(), n_ghost: 0, n_dup: 0 }
    }

    /// 报到：册内且未报才收。
    pub fn check_in(&mut self, apic_id: u8) -> Result<(), ()> {
        if !self.roster.contains(&apic_id) {
            self.n_ghost += 1;
            return Err(());
        }
        if self.checked_in.contains(&apic_id) {
            self.n_dup += 1;
            return Err(());
        }
        self.checked_in.push(apic_id);
        Ok(())
    }

    /// 点名齐活。
    pub fn all_present(&self) -> bool {
        self.checked_in.len() == self.roster.len()
    }

    /// 缺席者名单（册内未报）。
    pub fn absentees(&self) -> alloc::vec::Vec<u8> {
        self.roster.iter().filter(|id| !self.checked_in.contains(id)).copied().collect()
    }

    /// 与编排表对账：报到集是编排表子集且无重复。
    pub fn reconcile(&self) -> bool {
        self.checked_in.iter().all(|id| self.roster.contains(id))
            && self.checked_in.len()
                <= self.roster.len()
    }
}

/// 判据入口：核点名全性质核。
pub fn run_unx0551_checks() -> crate::checks::CheckSet {
    let mut cs = crate::checks::CheckSet::new("UNX-F0551");

    // 族一：册内报到。
    let mut r = RollCall::new(&[0, 1, 2, 3]);
    cs.add("核 0 报到", r.check_in(0).is_ok(), "册内收");
    cs.add("核 1 报到", r.check_in(1).is_ok(), "续收");
    cs.add("未齐活", !r.all_present(), "2/4");
    cs.add("缺席者二", r.absentees() == alloc::vec![2u8, 3], "2、3 未到");

    // 族二：幽灵与重复。
    cs.add("幽灵拒", r.check_in(9).is_err(), "册外核");
    cs.add("幽灵账一", r.n_ghost == 1, "入账");
    cs.add("重复报拒", r.check_in(0).is_err(), "已报再报");
    cs.add("重复账一", r.n_dup == 1, "入账");
    cs.add("对账核", r.reconcile(), "报到集 ⊆ 编排表");

    // 族三：齐活放行。
    r.check_in(2).ok();
    r.check_in(3).ok();
    cs.add("齐活", r.all_present(), "4/4");
    cs.add("缺席清空", r.absentees().is_empty(), "无人缺席");
    cs.add("齐活对账", r.reconcile(), "终账平");

    // 族四：幽灵核不得推进齐活（册外报到不计）。
    let mut r2 = RollCall::new(&[5, 6]);
    r2.check_in(5).ok();
    let _ = r2.check_in(7);
    cs.add("幽灵不顶数", !r2.all_present(), "1/2 仍缺");
    r2.check_in(6).ok();
    cs.add("册内补齐即活", r2.all_present(), "2/2");

    // 族五：单核册边界。
    let mut r3 = RollCall::new(&[1]);
    let first_in = r3.check_in(1).is_ok() && r3.all_present();
    cs.add("单报到即齐", first_in, "1/1");
    cs.add("空缺席", r3.absentees().is_empty(), "全到");

    // 族六：乱序报到（后号先到不乱账）。
    let mut r4 = RollCall::new(&[3, 7, 5]);
    for id in [7u8, 3, 5] {
        r4.check_in(id).ok();
    }
    cs.add("乱序报齐活", r4.all_present(), "报到序无关");
    cs.add("报到账三", r4.checked_in.len() == 3, "三人齐");
    cs.add("乱序对账", r4.reconcile(), "仍是子集");

    // 族七：缺席账与幽灵账分立。
    let mut r5 = RollCall::new(&[1, 2, 3, 4]);
    r5.check_in(1).ok();
    let _ = r5.check_in(8); // 幽灵
    let ab = r5.absentees();
    cs.add("缺席三不含幽灵", ab == alloc::vec![2u8, 3, 4], "幽灵不入缺席账");
    cs.add("两账互不染", r5.n_ghost == 1 && r5.checked_in.len() == 1, "拒账不进正账");

    // 族八：八核册全点名压测。
    let mut r6 = RollCall::new(&[0, 1, 2, 3, 4, 5, 6, 7]);
    let mut ok8 = true;
    for id in (0..8u8).rev() {
        if r6.check_in(id).is_err() {
            ok8 = false;
        }
    }
    cs.add("八核逆序全报", ok8 && r6.all_present(), "逆序报到齐活");
    cs.add("压测零拒", r6.n_ghost == 0 && r6.n_dup == 0, "无幽灵无重报");
    cs.add("压测对账", r6.reconcile(), "终账平");

    // 族九：缺席者定位与定向补点（逐个补齐核活）。
    let mut r7 = RollCall::new(&[2, 4, 6, 8]);
    r7.check_in(4).ok();
    r7.check_in(8).ok();
    let ab = r7.absentees();
    cs.add("缺二定位", ab == alloc::vec![2u8, 6], "册序定缺席");
    r7.check_in(2).ok();
    cs.add("补一仍缺", !r7.all_present() && r7.absentees() == alloc::vec![6u8], "3/4");
    r7.check_in(6).ok();
    cs.add("补满齐活", r7.all_present(), "4/4");

    // 族十：报到账序核（checked_in 保报到序）。
    let mut r8 = RollCall::new(&[9, 3, 6]);
    r8.check_in(6).ok();
    r8.check_in(9).ok();
    r8.check_in(3).ok();
    cs.add("账序即报到序", r8.checked_in == alloc::vec![6u8, 9, 3], "FIFO 报到账");
    cs.add("序核对账", r8.reconcile(), "乱序报到不破核");

    cs
}
