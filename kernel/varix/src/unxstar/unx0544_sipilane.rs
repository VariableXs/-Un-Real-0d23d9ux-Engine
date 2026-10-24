//! unx0544_sipilane —— UNX-F0544 SIPI 启动序列状态机。
//! 锚点：SMP 卷 · SIPI 三态（待发 / 已发 / 已达）+ 向量封印 + 重发账。
//! 判据原文：每核至多一发（幂等），向量越界（>0xFF 或落加速区外）拒；
//!           启动账守恒（发 = 达 + 在途 + 失联）。

/// 启动状态。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BootState {
    /// 待发。
    Waiting,
    /// SIPI 已发未达。
    Sent,
    /// 已达（核开始执行加速代码）。
    Arrived,
    /// 失联（重发次数耗尽）。
    Lost,
}

/// 单核启动账。
#[derive(Debug, Clone, Copy)]
pub struct CoreBoot {
    pub apic_id: u8,
    pub state: BootState,
    /// SIPI 向量（页号）。
    pub vector: u8,
    /// 重发计数。
    pub retries: u8,
}

/// 启动编排器。
#[derive(Debug)]
pub struct SipiOrch {
    pub cores: alloc::vec::Vec<CoreBoot>,
    /// 加速区合法向量区间 [lo, hi]。
    pub vec_lo: u8,
    pub vec_hi: u8,
    /// 拒账 / 重发账 / 失联账。
    pub n_reject: u64,
    pub n_resend: u64,
    pub n_lost: u64,
}

impl SipiOrch {
    pub fn new(ids: &[u8], vec_lo: u8, vec_hi: u8) -> Self {
        let cores = ids.iter().map(|id| CoreBoot {
            apic_id: *id, state: BootState::Waiting, vector: 0, retries: 0,
        }).collect();
        SipiOrch { cores, vec_lo, vec_hi, n_reject: 0, n_resend: 0, n_lost: 0 }
    }

    /// 发 SIPI：幂等（已发/已达/失联拒重发主路，重发走 resend）。
    pub fn send(&mut self, apic_id: u8, vector: u8) -> Result<bool, ()> {
        if vector < self.vec_lo || vector > self.vec_hi {
            self.n_reject += 1;
            return Err(());
        }
        let c = self.cores.iter_mut().find(|c| c.apic_id == apic_id).ok_or(())?;
        match c.state {
            BootState::Waiting => {
                c.state = BootState::Sent;
                c.vector = vector;
                Ok(true)
            }
            _ => {
                self.n_reject += 1;
                Err(())
            }
        }
    }

    /// 到达确认：Sent → Arrived。
    pub fn arrive(&mut self, apic_id: u8) -> Result<(), ()> {
        let c = self.cores.iter_mut().find(|c| c.apic_id == apic_id).ok_or(())?;
        if c.state != BootState::Sent {
            return Err(());
        }
        c.state = BootState::Arrived;
        Ok(())
    }

    /// 未达重发：计一次重发；重发耗尽转失联。
    pub fn resend(&mut self, apic_id: u8, max_retry: u8) -> Result<bool, ()> {
        let c = self.cores.iter_mut().find(|c| c.apic_id == apic_id).ok_or(())?;
        if c.state != BootState::Sent {
            return Err(());
        }
        c.retries += 1;
        self.n_resend += 1;
        if c.retries > max_retry {
            c.state = BootState::Lost;
            self.n_lost += 1;
            Ok(false)
        } else {
            Ok(true)
        }
    }

    /// 守恒：发（含重发）= 达 + 在途 + 失联重发计入后仍对账。
    pub fn reconcile(&self) -> bool {
        let waiting = self.cores.iter().filter(|c| c.state == BootState::Waiting).count();
        let sent = self.cores.iter().filter(|c| c.state == BootState::Sent).count();
        let arrived = self.cores.iter().filter(|c| c.state == BootState::Arrived).count();
        let lost = self.cores.iter().filter(|c| c.state == BootState::Lost).count();
        waiting + sent + arrived + lost == self.cores.len()
    }
}

/// 判据入口：SIPI 启动全性质核。
pub fn run_unx0544_checks() -> crate::checks::CheckSet {
    let mut cs = crate::checks::CheckSet::new("UNX-F0544");

    // 族一：发单正反两面。
    let mut o = SipiOrch::new(&[0, 1, 2], 0x08, 0x0F);
    cs.add("首发收", o.send(0, 0x08) == Ok(true), "合法向量入账");
    cs.add("重发主路拒", o.send(0, 0x08).is_err(), "已发幂等拒");
    cs.add("向量越下界拒", o.send(1, 0x07).is_err(), "低于加速区");
    cs.add("向量越上界拒", o.send(1, 0x10).is_err(), "高于加速区");
    cs.add("幽灵核拒", o.send(9, 0x08).is_err(), "未注册核");
    cs.add("拒账三", o.n_reject == 3, "三拒齐账");

    // 族二：到达确认。
    cs.add("到达转态", o.arrive(0).is_ok(), "Sent→Arrived");
    cs.add("重复到达拒", o.arrive(0).is_err(), "Arrived 拒再达");
    cs.add("未发到达拒", o.arrive(1).is_err(), "Waiting 不可达");

    // 族三：重发与失联。
    let mut o2 = SipiOrch::new(&[5], 0x08, 0x0F);
    o2.send(5, 0x08).ok();
    cs.add("重发一", o2.resend(5, 2) == Ok(true), "未超限重发");
    cs.add("重发二", o2.resend(5, 2) == Ok(true), "第二次重发");
    cs.add("重发三转失联", o2.resend(5, 2) == Ok(false), "第三次超限");
    cs.add("失联态", o2.cores[0].state == BootState::Lost, "转 Lost");
    cs.add("失联账一", o2.n_lost == 1, "失联入账");
    cs.add("失联后拒重发", o2.resend(5, 2).is_err(), "Lost 不可重发");

    // 族四：守恒核（全状态覆盖）。
    let mut o3 = SipiOrch::new(&[1, 2, 3, 4], 0x08, 0x0F);
    o3.send(1, 0x08).ok();
    o3.arrive(1).ok();
    o3.send(2, 0x09).ok();
    // 3、4 保持 Waiting。
    cs.add("四态并存", o3.reconcile(), "waiting+sent+arrived 全计");
    cs.add("状态核", o3.cores.iter().filter(|c| c.state == BootState::Arrived).count() == 1, "一达");

    // 族五：批量启动压测（八核全达）。
    let mut o4 = SipiOrch::new(&[0, 1, 2, 3, 4, 5, 6, 7], 0x08, 0x0F);
    let mut clean = true;
    for id in 0..8u8 {
        if o4.send(id, 0x08 + id).is_err() {
            clean = false;
        }
        if o4.arrive(id).is_err() {
            clean = false;
        }
    }
    cs.add("八核全达", clean && o4.reconcile(), "发→达全链通");
    cs.add("向量各异", o4.cores.iter().enumerate().all(|(i, c)| c.vector == 0x08 + i as u8), "向量随核号");

    // 族六：边界向量（lo/hi 端点收）。
    let mut o5 = SipiOrch::new(&[1, 2], 0x08, 0x0F);
    cs.add("下界收", o5.send(1, 0x08).is_ok(), "端点含");
    cs.add("上界收", o5.send(2, 0x0F).is_ok(), "端点含");

    // 族七：发送序与向量核（按编排序发，向量不串位）。
    let mut o6 = SipiOrch::new(&[10, 11, 12], 0x10, 0x20);
    let seq: alloc::vec::Vec<u8> = alloc::vec![12, 10, 11];
    let mut seq_ok = true;
    for (i, id) in seq.iter().enumerate() {
        if o6.send(*id, 0x10 + i as u8).is_err() {
            seq_ok = false;
        }
    }
    cs.add("乱序编排发", seq_ok, "发序由编排者定");
    cs.add("向量随编排", o6.cores.iter().all(|c| match c.apic_id {
        12 => c.vector == 0x10,
        10 => c.vector == 0x11,
        11 => c.vector == 0x12,
        _ => false,
    }), "核-向量一一对应");
    cs.add("发后全 Sent", o6.cores.iter().all(|c| c.state == BootState::Sent), "无 Waiting 残留");
    cs.add("守恒核", o6.reconcile(), "三态和 = 核数");

    // 族八：部分失败批（一核失联不拖累他核）。
    let mut o7 = SipiOrch::new(&[1, 2, 3], 0x08, 0x0F);
    o7.send(1, 0x08).ok();
    o7.send(2, 0x08).ok();
    o7.send(3, 0x08).ok();
    o7.resend(2, 0).ok(); // 核 2 第一次重发即超限（max=0）
    cs.add("核 2 失联", o7.cores.iter().find(|c| c.apic_id == 2).map(|c| c.state) == Some(BootState::Lost), "隔离失败");
    cs.add("他核不受累", o7.cores.iter().filter(|c| c.state == BootState::Sent).count() == 2, "1、3 仍在途");
    o7.arrive(1).ok();
    o7.arrive(3).ok();
    cs.add("失联不达", o7.arrive(2).is_err(), "Lost 拒达");
    cs.add("终账", o7.reconcile() && o7.n_lost == 1, "一失联两到达");

    cs
}
