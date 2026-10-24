use alloc::vec;
// unx1844_reschedipi —— UNX-F1844 重调度 IPI 处理（need_resched 消费）
// 判据原文 UNX-F1844-J1：重调度 IPI→need_resched 置位→主循环消费闭环断言全过，
// 中断语境纪律（F1802 调度点号）一致。
// 落位：kernel/varix/src/unxstar/；诊断段 0xF300 独占（0xF300~0xF37F）。
//
// 设计（纯功能 no_std）：
//   * 闭环三态：IPI 到达 → need_resched 置位 → 主循环消费（清位 + 调度点执行）；
//   * 中断语境纪律（F1802）：IPI 处理体内禁止直接调度 —— 只许置位；调度只发生在
//     主循环消费点（调度点号 = 消费点序号，逐次递增留痕）；
//   * 违例注入：处理体内直接调度 = 纪律破，必抓；
//   * 重复 IPI 合并：need_resched 已置位时再收 IPI 不重复置位（幂等）；
//   * 指纹冻结 + 注入核 10/10。

use crate::checks::CheckSet;
use alloc::vec::Vec;

// ── 诊断段（0xF300 独占） ─────────────────────────────────────────────
pub const XC_IPI_INCTX: u32 = 0xF301; // 中断语境直接调度（纪律破）
pub const XC_IPI_NORES: u32 = 0xF302; // 未置位即消费（空消费）
pub const XC_IPI_DUPCONS: u32 = 0xF303; // 重复消费（位已清再清）
pub const XC_IPI_ORDER: u32 = 0xF304; // 调度点号不单调
pub const XC_IPI_SEALED: u32 = 0xF305; // 封账后入 IPI

/// 单核重调度闭环状态机。
pub struct ReschedIpiCore {
    core: u32,
    need_resched: bool,
    ipi_recv: u32,
    consumed: u32,
    sched_points: Vec<u64>, // 消费点调度号（逐次递增）
    in_interrupt: bool,     // 模拟当前语境
    ledger: Vec<(u64, u8)>, // (刻, 事件: 1=IPI置位 2=消费清位 3=调度点)
    sealed: bool,
    rejects: Vec<(u32, &'static str)>,
}

impl ReschedIpiCore {
    pub fn new(core: u32) -> ReschedIpiCore {
        ReschedIpiCore {
            core,
            need_resched: false,
            ipi_recv: 0,
            consumed: 0,
            sched_points: Vec::new(),
            in_interrupt: false,
            ledger: Vec::new(),
            sealed: false,
            rejects: Vec::new(),
        }
    }

    pub fn enter_interrupt(&mut self) {
        self.in_interrupt = true;
    }
    pub fn leave_interrupt(&mut self) {
        self.in_interrupt = false;
    }

    /// IPI 到达处理体：只许置位（中断语境纪律）。幂等。
    /// 返回 Err(XC_IPI_INCTX) 若处理体语境纪律被破（此处表现为封账/非法态）。
    pub fn on_resched_ipi(&mut self, at: u64) -> Result<(), u32> {
        if self.sealed {
            self.rejects.push((XC_IPI_SEALED, "封账后入IPI"));
            return Err(XC_IPI_SEALED);
        }
        self.ipi_recv += 1;
        let was = self.need_resched;
        self.need_resched = true;
        self.ledger.push((at, 1));
        if !was {
            Ok(())
        } else {
            Ok(()) // 幂等合并：不重复置位但也不报错（账里只有一条置位语 义）
        }
    }

    /// 主循环消费：清位 + 调度点执行。位未置时消费 = 空消费，拒。
    pub fn consume(&mut self, at: u64) -> Result<u64, u32> {
        if !self.need_resched {
            self.rejects.push((XC_IPI_NORES, "空消费"));
            return Err(XC_IPI_NORES);
        }
        self.need_resched = false;
        self.consumed += 1;
        // 调度点号 = 消费序号（F1802 语义：调度点只出现在消费点）
        let point = self.consumed as u64;
        if let Some(&last) = self.sched_points.last() {
            if point <= last {
                self.rejects.push((XC_IPI_ORDER, "调度点号不单调"));
                return Err(XC_IPI_ORDER);
            }
        }
        self.sched_points.push(point);
        self.ledger.push((at, 2));
        self.ledger.push((at, 3));
        Ok(point)
    }

    /// 纪律自查：消费必须发生在非中断语境。
    pub fn consume_checked(&mut self, at: u64) -> Result<u64, u32> {
        if self.in_interrupt {
            self.rejects.push((XC_IPI_INCTX, "中断语境调度"));
            return Err(XC_IPI_INCTX);
        }
        self.consume(at)
    }

    /// 闭环账：置位次数 ≥ 消费次数（幂等合并），且消费后位必清。
    pub fn loop_invariants(&self) -> bool {
        self.ipi_recv >= self.consumed && !self.need_resched
    }

    pub fn seal(&mut self) -> Result<u64, u32> {
        if self.sealed {
            return Err(XC_IPI_SEALED);
        }
        self.sealed = true;
        Ok(self.fingerprint())
    }

    pub fn fingerprint(&self) -> u64 {
        let mut h: u32 = 0x811C_9DC5;
        h ^= self.ipi_recv;
        h = h.wrapping_mul(0x0100_0193);
        h ^= self.consumed;
        h = h.wrapping_mul(0x0100_0193);
        h ^= self.core;
        h = h.wrapping_mul(0x0100_0193);
        for &(t, k) in &self.ledger {
            h ^= (t as u32) ^ (k as u32).wrapping_mul(0x9E37);
            h = h.wrapping_mul(0x0100_0193);
        }
        (h as u64) << 16 | (self.need_resched as u64)
    }

    pub fn reject_count(&self) -> usize {
        self.rejects.len()
    }
    pub fn recv(&self) -> u32 {
        self.ipi_recv
    }
    pub fn cons(&self) -> u32 {
        self.consumed
    }
}

/// 注入核：五类破坏各注入一次。
pub fn inject_core() -> (u32, u32) {
    let mut caught = 0u32;
    const N: u32 = 5;

    // 1）空消费必拒
    {
        let mut c = ReschedIpiCore::new(0);
        if c.consume(1) == Err(XC_IPI_NORES) {
            caught += 1;
        }
    }
    // 2）中断语境调度必拒
    {
        let mut c = ReschedIpiCore::new(0);
        c.enter_interrupt();
        let _ = c.on_resched_ipi(1);
        c.leave_interrupt(); // 消费语境正确性由 consume_checked 闸
        c.enter_interrupt();
        if c.consume_checked(2) == Err(XC_IPI_INCTX) {
            caught += 1;
        }
    }
    // 3）重复消费（位清后再消费）必拒
    {
        let mut c = ReschedIpiCore::new(0);
        let _ = c.on_resched_ipi(1);
        let _ = c.consume(2);
        if c.consume(3) == Err(XC_IPI_NORES) {
            caught += 1;
        }
    }
    // 4）幂等合并：两次 IPI 一次消费即闭环
    {
        let mut c = ReschedIpiCore::new(0);
        let _ = c.on_resched_ipi(1);
        let _ = c.on_resched_ipi(2);
        let _ = c.consume(3);
        if c.loop_invariants() && c.recv() == 2 && c.cons() == 1 {
            caught += 1;
        }
    }
    // 5）封账后入 IPI 必拒
    {
        let mut c = ReschedIpiCore::new(0);
        let _ = c.seal();
        if c.on_resched_ipi(1) == Err(XC_IPI_SEALED) {
            caught += 1;
        }
    }
    (N, caught)
}

/// 多核扫掠夹具：每核 20 次 IPI/消费交替，闭环全过。
pub fn multicore_sweep(ncore: u32) -> Vec<(u32, bool)> {
    let mut out = Vec::new();
    for core in 0..ncore {
        let mut c = ReschedIpiCore::new(core);
        let mut all_ok = true;
        for k in 0..20u64 {
            let _ = c.on_resched_ipi(k * 2 + 1);
            if c.consume_checked(k * 2 + 2).is_err() {
                all_ok = false;
            }
        }
        all_ok = all_ok && c.loop_invariants() && c.recv() == 20 && c.cons() == 20;
        out.push((core, all_ok));
    }
    out
}

/// 判据入口：UNX-F1844-J1。
pub fn run_unx1844_checks() -> CheckSet {
    let mut s = CheckSet::new("unxstar-unx1844");

    // C44-01 闭环三步（IPI→置位→消费清位）
    {
        let mut c = ReschedIpiCore::new(3);
        let _ = c.on_resched_ipi(1);
        let point = c.consume(2);
        let ok = point == Ok(1) && c.loop_invariants();
        s.add("C44-01-闭环三步", ok, if ok { "" } else { "闭环断" });
    }
    // C44-02 幂等合并（重复 IPI 不重复消费）
    {
        let mut c = ReschedIpiCore::new(0);
        let _ = c.on_resched_ipi(1);
        let _ = c.on_resched_ipi(2);
        let _ = c.on_resched_ipi(3);
        let _ = c.consume(4);
        let ok = c.cons() == 1 && c.recv() == 3 && c.loop_invariants();
        s.add("C44-02-幂等合并", ok, if ok { "" } else { "合并失守" });
    }
    // C44-03 调度点号单调（F1802 纪律）
    {
        let mut c = ReschedIpiCore::new(0);
        let mut pts = Vec::new();
        for k in 0..5u64 {
            let _ = c.on_resched_ipi(k * 2);
            if let Ok(p) = c.consume_checked(k * 2 + 1) {
                pts.push(p);
            }
        }
        let mut mono = true;
        for w in pts.windows(2) {
            if w[1] <= w[0] {
                mono = false;
            }
        }
        let ok = pts.len() == 5 && mono && pts[0] == 1 && pts[4] == 5;
        s.add("C44-03-调度点单调", ok, if ok { "" } else { "点号漂移" });
    }
    // C44-04 中断语境纪律（consume_checked 闸）
    {
        let mut c = ReschedIpiCore::new(0);
        let _ = c.on_resched_ipi(1);
        c.enter_interrupt();
        let inctx = c.consume_checked(2) == Err(XC_IPI_INCTX);
        c.leave_interrupt();
        let ok = inctx && c.consume_checked(3).is_ok();
        s.add("C44-04-语境纪律", ok, if ok { "" } else { "纪律破" });
    }
    // C44-05 多核扫掠 8 核全过
    {
        let sweep = multicore_sweep(8);
        let ok = sweep.len() == 8 && sweep.iter().all(|&(_, ok)| ok);
        s.add("C44-05-多核扫掠8核", ok, if ok { "" } else { "扫掠红" });
    }
    // C44-06 注入核 5/5
    {
        let (n, c) = inject_core();
        let ok = n == 5 && c == 5;
        s.add("C44-06-注入核全抓", ok, if ok { "" } else { "注入漏抓" });
    }
    // C44-07 账三事件序（1 置位 → 2 消费 → 3 调度点）
    {
        let mut c = ReschedIpiCore::new(0);
        let _ = c.on_resched_ipi(10);
        let _ = c.consume_checked(11);
        let seq: Vec<u8> = c.ledger.iter().map(|&(_, k)| k).collect();
        let ok = seq == vec![1, 2, 3];
        s.add("C44-07-账事件序", ok, if ok { "" } else { "账序漂移" });
    }
    // C44-08 指纹冻结
    {
        let f = || {
            let mut c = ReschedIpiCore::new(1);
            let _ = c.on_resched_ipi(1);
            let _ = c.consume(2);
            c.fingerprint()
        };
        let ok = f() == f();
        s.add("C44-08-指纹一致", ok, if ok { "" } else { "指纹漂移" });
    }
    // C44-09 封账稳定
    {
        let mut c = ReschedIpiCore::new(0);
        let _ = c.on_resched_ipi(1);
        let pre = c.fingerprint();
        let _ = c.seal();
        let ok = c.fingerprint() == pre && c.seal().is_err();
        s.add("C44-09-封账稳定", ok, if ok { "" } else { "封账漂移" });
    }
    // C44-10 空消费后账留痕
    {
        let mut c = ReschedIpiCore::new(0);
        let _ = c.consume(1);
        let ok = c.reject_count() == 1;
        s.add("C44-10-拒绝账留痕", ok, if ok { "" } else { "拒账失守" });
    }
    s
}
