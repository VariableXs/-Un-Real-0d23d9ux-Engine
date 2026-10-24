//! unx0545_apwait —— UNX-F0545 INIT-SIPI-SIPI 三段协议账。
//! 锚点：SMP 卷 · 古典启动协议（INIT → SIPI → SIPI 双发）+ 段序封印。
//! 判据原文：段序不可乱（INIT 未完成拒 SIPI），双 SIPI 间隔核验
//!           （过短拒），协议完成核转待命。

/// 协议段。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Phase {
    /// 未开始。
    Idle,
    /// INIT 已发。
    InitSent,
    /// INIT 完成等待期过。
    InitDone,
    /// 第一发 SIPI 已发。
    Sipi1,
    /// 双 SIPI 完成。
    Sipi2,
}

/// 单核协议账。
#[derive(Debug)]
pub struct Proto {
    pub apic_id: u8,
    pub phase: Phase,
    /// 上段完成时刻（模拟 tick）。
    pub t_mark: u64,
    /// 违序拒账。
    pub n_bad_order: u64,
}

/// 双 SIPI 最小间隔（tick）。
pub const SIPI_GAP: u64 = 10;

impl Proto {
    pub fn new(apic_id: u8) -> Self {
        Proto { apic_id, phase: Phase::Idle, t_mark: 0, n_bad_order: 0 }
    }

    /// 发 INIT：仅 Idle 态可发。
    pub fn send_init(&mut self, t: u64) -> Result<(), ()> {
        if self.phase != Phase::Idle {
            self.n_bad_order += 1;
            return Err(());
        }
        self.phase = Phase::InitSent;
        self.t_mark = t;
        Ok(())
    }

    /// INIT 完成期（等待 ≥ 完成延迟后转 InitDone）。
    pub fn init_settle(&mut self, t: u64, settle: u64) -> Result<(), ()> {
        if self.phase != Phase::InitSent {
            self.n_bad_order += 1;
            return Err(());
        }
        if t < self.t_mark + settle {
            self.n_bad_order += 1;
            return Err(());
        }
        self.phase = Phase::InitDone;
        self.t_mark = t;
        Ok(())
    }

    /// 发 SIPI：序号 1 或 2；双发间隔 ≥ SIPI_GAP；必须 InitDone 后。
    pub fn send_sipi(&mut self, t: u64, seq: u8) -> Result<(), ()> {
        let ok = match (self.phase, seq) {
            (Phase::InitDone, 1) => true,
            (Phase::Sipi1, 2) => t >= self.t_mark + SIPI_GAP,
            _ => false,
        };
        if !ok {
            self.n_bad_order += 1;
            return Err(());
        }
        self.phase = if seq == 1 { Phase::Sipi1 } else { Phase::Sipi2 };
        self.t_mark = t;
        Ok(())
    }

    /// 协议完成。
    pub fn ready(&self) -> bool {
        self.phase == Phase::Sipi2
    }
}

/// 判据入口：INIT-SIPI-SIPI 协议全性质核。
pub fn run_unx0545_checks() -> crate::checks::CheckSet {
    let mut cs = crate::checks::CheckSet::new("UNX-F0545");

    // 族一：段序正面全链。
    let mut p = Proto::new(0);
    cs.add("INIT 发", p.send_init(0).is_ok(), "Idle→InitSent");
    cs.add("完成期转", p.init_settle(10, 10).is_ok(), "等足转 InitDone");
    cs.add("SIPI 一发", p.send_sipi(10, 1).is_ok(), "InitDone→Sipi1");
    cs.add("SIPI 二发", p.send_sipi(20, 2).is_ok(), "间隔足 Sipi2");
    cs.add("协议就绪", p.ready(), "双 SIPI 齐");

    // 族二：违序拒（SIPI 先于 INIT）。
    let mut p2 = Proto::new(1);
    cs.add("裸 SIPI 拒", p2.send_sipi(0, 1).is_err(), "Idle 态拒 SIPI");
    cs.add("违序账一", p2.n_bad_order == 1, "入拒账");
    let mut p2b = Proto::new(2);
    p2b.send_init(0).ok();
    cs.add("双 INIT 拒", p2b.send_init(1).is_err(), "InitSent 态封 INIT");

    // 族三：完成期未足拒。
    let mut p3 = Proto::new(3);
    p3.send_init(0).ok();
    cs.add("提前转拒", p3.init_settle(5, 10).is_err(), "5 < 0+10");
    cs.add("违序账二", p3.n_bad_order == 1, "提前入拒");
    cs.add("足期转通", p3.init_settle(10, 10).is_ok(), "补足放行");

    // 族四：SIPI 间隔核验。
    let mut p4 = Proto::new(4);
    p4.send_init(0).ok();
    p4.init_settle(10, 10).ok();
    p4.send_sipi(10, 1).ok();
    cs.add("间隔过短拒", p4.send_sipi(15, 2).is_err(), "5 < 10");
    cs.add("间隔足发", p4.send_sipi(20, 2).is_ok(), "10 ≥ 10");
    cs.add("间隔拒入账", p4.n_bad_order == 1, "短拒入账");

    // 族五：序号跳跃拒（直接发 2）。
    let mut p5 = Proto::new(5);
    p5.send_init(0).ok();
    p5.init_settle(10, 10).ok();
    cs.add("跳发 2 拒", p5.send_sipi(10, 2).is_err(), "未发 1 不可发 2");
    p5.send_sipi(10, 1).ok();
    p5.send_sipi(20, 2).ok();
    cs.add("补齐后成", p5.ready(), "顺序恢复");

    // 族六：多核并行协议（各核独立账）。
    let mut ps: alloc::vec::Vec<Proto> = (0u8..4).map(Proto::new).collect();
    let mut all_ok = true;
    for (i, p) in ps.iter_mut().enumerate() {
        let t = (i as u64) * 100;
        if p.send_init(t).is_err() || p.init_settle(t + 10, 10).is_err()
            || p.send_sipi(t + 10, 1).is_err() || p.send_sipi(t + 20, 2).is_err() || !p.ready() {
            all_ok = false;
        }
    }
    cs.add("四核并链", all_ok, "各核独立时序全通");
    cs.add("零违序", ps.iter().all(|p| p.n_bad_order == 0), "规范流程零拒");

    // 族七：间隔临界边界（恰 SIPI_GAP 收 / GAP-1 拒）。
    let mut p6 = Proto::new(6);
    p6.send_init(0).ok();
    p6.init_settle(10, 10).ok();
    p6.send_sipi(10, 1).ok();
    cs.add("GAP-1 拒", p6.send_sipi(10 + SIPI_GAP - 1, 2).is_err(), "恰差一");
    cs.add("重发一后成", p6.send_sipi(10 + SIPI_GAP, 2).is_ok(), "补足间隔");
    cs.add("临界拒账", p6.n_bad_order == 1, "一拒");

    // 族八：重启协议核（新 Proto 复用同核号互不串账）。
    let mut p7 = Proto::new(7);
    p7.send_init(0).ok();
    p7.init_settle(10, 10).ok();
    p7.send_sipi(10, 1).ok();
    p7.send_sipi(20, 2).ok();
    let p8 = Proto::new(7);
    cs.add("新账归零", p8.phase == Phase::Idle && p8.n_bad_order == 0, "重建即净");
    cs.add("旧账保留", p7.ready() && p7.n_bad_order == 0, "完成态不褪");
    cs.add("完成后再 INIT 拒", p7.send_init(100).is_err(), "Sipi2 封口");

    cs
}
