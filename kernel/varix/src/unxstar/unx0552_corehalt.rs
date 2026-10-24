//! unx0552_corehalt —— UNX-F0552 核节能态机账。
//! 锚点：SMP 卷 · RUN/HALT/WAKE 三态（自陷唤醒 / 外源唤醒 / 态序封印）。
//! 判据原文：HALT 态不计时（tick 账冻结），WAKE 必有唤醒源；
//!           空转唤醒拒；态账守恒。

/// 核能态。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CState {
    /// 运行。
    Run,
    /// 暂停（等中断）。
    Halt,
}

/// 单核能态账。
#[derive(Debug)]
pub struct CorePower {
    pub apic_id: u8,
    pub state: CState,
    /// 运行 tick 账。
    pub run_ticks: u64,
    /// HALT 段 tick（冻结校验用：HALT 期不该增）。
    pub halt_ticks: u64,
    /// 唤醒源（None = 未挂源）。
    pub wake_src: Option<&'static str>,
    /// 态违例账。
    pub n_bad: u64,
}

impl CorePower {
    pub fn new(apic_id: u8) -> Self {
        CorePower { apic_id, state: CState::Run, run_ticks: 0, halt_ticks: 0, wake_src: None, n_bad: 0 }
    }

    /// tick 推进：Run 态计运行账，Halt 态冻结运行账（只记 halt 观察账）。
    pub fn tick(&mut self) {
        match self.state {
            CState::Run => self.run_ticks += 1,
            CState::Halt => self.halt_ticks += 1,
        }
    }

    /// 自陷 HALT：仅 Run 态可入。
    pub fn halt(&mut self) -> Result<(), ()> {
        if self.state != CState::Run {
            self.n_bad += 1;
            return Err(());
        }
        self.state = CState::Halt;
        Ok(())
    }

    /// 挂唤醒源（HALT 态才可挂）。
    pub fn arm(&mut self, src: &'static str) -> Result<(), ()> {
        if self.state != CState::Halt {
            self.n_bad += 1;
            return Err(());
        }
        if self.wake_src.is_some() {
            return Err(()); // 已挂源拒重挂
        }
        self.wake_src = Some(src);
        Ok(())
    }

    /// 唤醒：须 HALT + 有源；无源唤醒拒（空转唤醒）。
    pub fn wake(&mut self) -> Result<&'static str, ()> {
        if self.state != CState::Halt {
            self.n_bad += 1;
            return Err(());
        }
        match self.wake_src.take() {
            Some(s) => {
                self.state = CState::Run;
                Ok(s)
            }
            None => {
                self.n_bad += 1;
                Err(())
            }
        }
    }
}

/// 判据入口：核能态全性质核。
pub fn run_unx0552_checks() -> crate::checks::CheckSet {
    let mut cs = crate::checks::CheckSet::new("UNX-F0552");

    // 族一：Run 态计时。
    let mut p = CorePower::new(0);
    for _ in 0..5 {
        p.tick();
    }
    cs.add("运行账五", p.run_ticks == 5, "Run 计 tick");
    cs.add("冻结账零", p.halt_ticks == 0, "未 Halt 不计");

    // 族二：HALT 冻结。
    p.halt().ok();
    for _ in 0..3 {
        p.tick();
    }
    cs.add("Halt 冻运行账", p.run_ticks == 5, "HALT 期不增");
    cs.add("Halt 观察账三", p.halt_ticks == 3, "观察账入");

    // 族三：空转唤醒拒与挂源唤醒。
    cs.add("无源唤醒拒", p.wake().is_err(), "未挂源");
    cs.add("违例账一", p.n_bad == 1, "空转入账");
    cs.add("挂源通", p.arm("timer").is_ok(), "timer 唤醒源");
    cs.add("重挂拒", p.arm("ipi").is_err(), "单源封印");
    cs.add("有源唤醒通", p.wake() == Ok("timer"), "源触发回 Run");
    cs.add("源清空", p.wake_src.is_none(), "一次性消费");
    cs.add("唤醒后计时", { p.tick(); p.run_ticks == 6 }, "恢复计 tick");

    // 族四：态序封印。
    cs.add("HALT 中再 HALT 拒", { p.halt().ok(); p.halt().is_err() }, "Halt 态拒再 Halt");
    let _ = p.wake(); // 先回 Run（有源即醒，无源拒也无妨）
    cs.add("Run 态挂源拒", p.arm("x").is_err(), "Run 不可挂源");
    cs.add("违例账累计", p.n_bad == 3, "三违例齐账");

    // 族五：多核能态独立账。
    let mut ps: alloc::vec::Vec<CorePower> = (0u8..4).map(CorePower::new).collect();
    for (i, p) in ps.iter_mut().enumerate() {
        for _ in 0..(i as u64 + 1) {
            p.tick();
        }
    }
    cs.add("各核独立计", ps.iter().enumerate().all(|(i, p)| p.run_ticks == i as u64 + 1), "账随核号");
    // 偶数核 HALT 挂源唤醒。
    let mut wake_ok = true;
    for (i, p) in ps.iter_mut().enumerate() {
        if i % 2 == 0 {
            if p.halt().is_err() || p.arm("ipi").is_err() || p.wake() != Ok("ipi") {
                wake_ok = false;
            }
        }
    }
    cs.add("偶核挂醒链", wake_ok, "HALT→挂源→唤醒全通");
    cs.add("态还原", ps.iter().all(|p| p.state == CState::Run), "全员回 Run");

    // 族六：HALT-WAKE 十轮循环（冻结账与运行账分立增长）。
    let mut q = CorePower::new(1);
    for _ in 0..10 {
        q.tick(); // Run 计一
        q.halt().ok();
        for _ in 0..2 {
            q.tick(); // Halt 观察计二
        }
        q.arm("timer").ok();
        let _ = q.wake();
    }
    cs.add("循环运行账十", q.run_ticks == 10, "每轮 +1");
    cs.add("循环冻结账二十", q.halt_ticks == 20, "每轮 +2");
    cs.add("循环终态 Run", q.state == CState::Run, "十轮全醒");
    cs.add("循环零违例", q.n_bad == 0, "规范循环零拒");

    // 族七：唤醒源种类核（timer 与 ipi 交替，消费后源清）。
    let mut r = CorePower::new(2);
    let mut src_ok = true;
    for (i, src) in ["timer", "ipi", "timer", "ipi"].iter().enumerate() {
        r.halt().ok();
        r.arm(src).ok();
        if r.wake() != Ok(*src) {
            src_ok = false;
        }
        if r.wake_src.is_some() {
            src_ok = false; // 源必须消费即清
        }
        let _ = i;
    }
    cs.add("源交替四轮", src_ok, "源名回读一致");
    cs.add("四轮零违例", r.n_bad == 0, "交替零拒");

    cs
}
