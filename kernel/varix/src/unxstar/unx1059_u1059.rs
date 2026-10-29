// unx1059_u1059 —— UNX-F1059 LAPIC 定时器与 EOI 联动验证（F0823/F0828 消费对齐）
// 判据原文 UNX-F1059-J1：联动正确（定时器 EOI 走 F0823 单点封装断言——旁路清零），五类分账对齐（定时器向量进 IRQ 类分账 10/10 次），EOI 延迟账交叉（F0851 可查）。
// 落位：kernel/varix/src/unxstar/；诊断段 0x2C70 独占（0x2C71~0x2C7F）。
//
// 设计（纯功能 no_std）：
//   * EOI 单点封装：定时器到期必须经单点 eoi() 出口，旁路计数必须为 0；
//   * 五类分账：定时器/外部/本机/IPI/伪中断五类 IRQ 分类落账；
//   * 定时器向量进 IRQ 类分账 10/10（分类不错账）；
//   * EOI 延迟账：每次 EOI 延时样张落账可查（F0851 交叉）；
//   * saturating 防溢出 + FNV 指纹 + 注入核全抓。

use crate::checks::CheckSet;
use alloc::vec::Vec;

// ── 诊断段（0x2C70 独占） ─────────────────────────────────────────────
pub const XC_EO_BYPASS: u32 = 0x2C71; // EOI 旁路
pub const XC_EO_OOB: u32 = 0x2C72; // 向量或类号越界
pub const XC_EO_SEALED: u32 = 0x2C73; // 封账后入账
pub const XC_EO_CLASS: u32 = 0x2C74; // 分类错账
pub const XC_EO_LATENCY: u32 = 0x2C75; // EOI 延迟越限

/// 五类 IRQ。
pub const IRQ_TIMER: u8 = 0;
pub const IRQ_EXTERNAL: u8 = 1;
pub const IRQ_LOCAL: u8 = 2;
pub const IRQ_IPI: u8 = 3;
pub const IRQ_SPURIOUS: u8 = 4;
pub const IRQ_CLASSES: usize = 5;
pub const CLASS_NAMES: [&str; 5] = ["定时器", "外部", "本机", "IPI", "伪中断"];

/// 定时器向量基址。
pub const TIMER_VECTOR: u32 = 0xEF;
/// EOI 延迟上限（µs）。
pub const EOI_LAT_LIMIT: u32 = 20;

/// EOI 联动账。
pub struct EoiLinkage {
    eoi_single: u32,  // 走单点封装次数
    bypass: u32,      // 旁路次数（必须 0）
    irq_ledger: [u32; IRQ_CLASSES], // 五类分账
    latencies: Vec<u32>, // EOI 延迟样张 µs
    sealed: bool,
    rejects: Vec<(u32, &'static str)>,
}

impl EoiLinkage {
    pub fn new() -> EoiLinkage {
        EoiLinkage {
            eoi_single: 0,
            bypass: 0,
            irq_ledger: [0; IRQ_CLASSES],
            latencies: Vec::new(),
            sealed: false,
            rejects: Vec::new(),
        }
    }

    /// 定时器到期 → 经单点 EOI 出口（向量合法且延迟达标才入账）。
    pub fn timer_fire(&mut self, vector: u32, latency_us: u32) -> Result<(), u32> {
        if self.sealed {
            self.rejects.push((XC_EO_SEALED, "封账后入账"));
            return Err(XC_EO_SEALED);
        }
        if vector != TIMER_VECTOR {
            self.rejects.push((XC_EO_OOB, "定时器向量非法"));
            return Err(XC_EO_OOB);
        }
        if latency_us > EOI_LAT_LIMIT {
            self.rejects.push((XC_EO_LATENCY, "EOI 延迟越限"));
            return Err(XC_EO_LATENCY);
        }
        self.eoi_single = self.eoi_single.saturating_add(1);
        self.irq_ledger[IRQ_TIMER as usize] = self.irq_ledger[IRQ_TIMER as usize].saturating_add(1);
        self.latencies.push(latency_us);
        Ok(())
    }

    /// 非 EOI 路径直接清 ISR（旁路注入）。
    pub fn bypass_clear(&mut self) -> Result<(), u32> {
        self.bypass = self.bypass.saturating_add(1);
        self.rejects.push((XC_EO_BYPASS, "EOI 旁路"));
        Err(XC_EO_BYPASS)
    }

    /// 单点断言：旁路计数为 0 且单点计数 >0。
    pub fn single_point_ok(&self) -> bool {
        self.bypass == 0 && self.eoi_single > 0
    }

    /// 其他类 IRQ 入账。
    pub fn irq_other(&mut self, class: u8) -> Result<(), u32> {
        if self.sealed {
            self.rejects.push((XC_EO_SEALED, "封账后入账"));
            return Err(XC_EO_SEALED);
        }
        if class >= IRQ_CLASSES as u8 {
            self.rejects.push((XC_EO_OOB, "类号越界"));
            return Err(XC_EO_OOB);
        }
        self.irq_ledger[class as usize] = self.irq_ledger[class as usize].saturating_add(1);
        Ok(())
    }

    /// 分类断言：定时器事件全部进定时器类（10/10）。
    pub fn timer_classified_10_10(&self, total_timer_events: u32) -> bool {
        self.irq_ledger[IRQ_TIMER as usize] == total_timer_events && total_timer_events >= 10
    }

    /// 五类分账对齐：总账 = 五类之和。
    pub fn ledger_conserved(&self) -> bool {
        let mut sum = 0u32;
        for &c in &self.irq_ledger {
            sum = sum.saturating_add(c);
        }
        sum == self.eoi_single.saturating_add(self.irq_ledger[IRQ_EXTERNAL as usize]
            .saturating_add(self.irq_ledger[IRQ_LOCAL as usize])
            .saturating_add(self.irq_ledger[IRQ_IPI as usize])
            .saturating_add(self.irq_ledger[IRQ_SPURIOUS as usize]))
    }

    /// EOI 延迟账可查（F0851 交叉）：返回 p99 延迟。
    pub fn latency_p99(&self) -> u32 {
        if self.latencies.is_empty() {
            return 0;
        }
        let mut v = self.latencies.clone();
        v.sort();
        let n = v.len();
        let idx = (n.saturating_mul(99).saturating_sub(1)).saturating_div(100).min(n - 1);
        v[idx]
    }

    pub fn seal(&mut self) -> Result<u64, u32> {
        if self.sealed {
            return Err(XC_EO_SEALED);
        }
        if !self.single_point_ok() || !self.ledger_conserved() {
            self.rejects.push((XC_EO_BYPASS, "单点破或账不平禁封"));
            return Err(XC_EO_BYPASS);
        }
        self.sealed = true;
        Ok(self.fingerprint())
    }

    pub fn fingerprint(&self) -> u64 {
        let mut h: u32 = 0x811C_9DC5;
        h ^= self.eoi_single;
        h = h.wrapping_mul(0x0100_0193);
        h ^= self.bypass;
        h = h.wrapping_mul(0x0100_0193);
        for (i, &c) in self.irq_ledger.iter().enumerate() {
            h ^= (i as u32).wrapping_mul(0x9E37) ^ c;
            h = h.wrapping_mul(0x0100_0193);
        }
        h ^= self.latency_p99();
        h = h.wrapping_mul(0x0100_0193);
        h as u64
    }

    pub fn eoi_count(&self) -> u32 {
        self.eoi_single
    }
    pub fn irq_of(&self, class: u8) -> u32 {
        if (class as usize) < IRQ_CLASSES {
            self.irq_ledger[class as usize]
        } else {
            0
        }
    }
    pub fn reject_count(&self) -> usize {
        self.rejects.len()
    }
}

/// 注入核：六类破坏全抓。
pub fn inject_core() -> (u32, u32) {
    let mut caught = 0u32;
    const N: u32 = 6;

    // 1）旁路清零必抓（10/10）
    {
        let mut e = EoiLinkage::new();
        let mut hits = 0;
        for _ in 0..10 {
            if e.bypass_clear() == Err(XC_EO_BYPASS) {
                hits += 1;
            }
        }
        if hits == 10 && e.bypass == 10 {
            caught += 1;
        }
    }
    // 2）定时器向量非法必拒
    {
        let mut e = EoiLinkage::new();
        if e.timer_fire(0x41, 1) == Err(XC_EO_OOB) {
            caught += 1;
        }
    }
    // 3）EOI 延迟越限必拒
    {
        let mut e = EoiLinkage::new();
        if e.timer_fire(TIMER_VECTOR, 999) == Err(XC_EO_LATENCY) {
            caught += 1;
        }
    }
    // 4）类号越界必拒（10/10）
    {
        let mut e = EoiLinkage::new();
        let mut hits = 0;
        for c in 5..15u8 {
            if e.irq_other(c) == Err(XC_EO_OOB) {
                hits += 1;
            }
        }
        if hits == 10 {
            caught += 1;
        }
    }
    // 5）空账禁封
    {
        let mut e = EoiLinkage::new();
        if e.seal() == Err(XC_EO_BYPASS) {
            caught += 1;
        }
    }
    // 6）封账后入账必拒
    {
        let mut e = EoiLinkage::new();
        let _ = e.timer_fire(TIMER_VECTOR, 2);
        let _ = e.seal();
        if e.timer_fire(TIMER_VECTOR, 2) == Err(XC_EO_SEALED) {
            caught += 1;
        }
    }
    (N, caught)
}

/// 判据入口：UNX-F1059-J1。
pub fn run_unx1059_checks() -> CheckSet {
    let mut s = CheckSet::new("unxstar-unx1059");

    // C59-01 EOI 走单点封装（旁路 0）
    {
        let mut e = EoiLinkage::new();
        let mut all_ok = true;
        for k in 0..10u32 {
            if e.timer_fire(TIMER_VECTOR, 2 + k).is_err() {
                all_ok = false;
            }
        }
        s.add("C59-01-单点封装", all_ok && e.single_point_ok() && e.eoi_count() == 10, if all_ok { "" } else { "单点破" });
    }
    // C59-02 定时器向量进 IRQ 类分账 10/10
    {
        let mut e = EoiLinkage::new();
        for _ in 0..10 {
            let _ = e.timer_fire(TIMER_VECTOR, 3);
        }
        s.add("C59-02-类分账10/10", e.timer_classified_10_10(10) && e.irq_of(IRQ_TIMER) == 10, if e.irq_of(IRQ_TIMER) == 10 { "" } else { "错账" });
    }
    // C59-03 五类分账对齐
    {
        let mut e = EoiLinkage::new();
        let _ = e.timer_fire(TIMER_VECTOR, 1);
        let _ = e.irq_other(IRQ_EXTERNAL);
        let _ = e.irq_other(IRQ_LOCAL);
        let _ = e.irq_other(IRQ_IPI);
        let _ = e.irq_other(IRQ_SPURIOUS);
        s.add("C59-03-五类对齐", e.ledger_conserved(), if e.ledger_conserved() { "" } else { "对齐破" });
    }
    // C59-04 EOI 延迟账交叉（p99 可查）
    {
        let mut e = EoiLinkage::new();
        for _ in 0..100 {
            let _ = e.timer_fire(TIMER_VECTOR, 5);
        }
        let _ = e.timer_fire(TIMER_VECTOR, 9);
        s.add("C59-04-延迟p99", e.latency_p99() == 5 && e.latency_p99() <= EOI_LAT_LIMIT, "");
    }
    // C59-05 旁路计数留痕
    {
        let mut e = EoiLinkage::new();
        let _ = e.bypass_clear();
        s.add("C59-05-旁路留痕", !e.single_point_ok(), if !e.single_point_ok() { "" } else { "旁路漏抓" });
    }
    // C59-06 延迟越限拒
    {
        let mut e = EoiLinkage::new();
        let ok = e.timer_fire(TIMER_VECTOR, EOI_LAT_LIMIT + 1) == Err(XC_EO_LATENCY);
        s.add("C59-06-延迟越限拒", ok, if ok { "" } else { "漏抓" });
    }
    // C59-07 封账产指纹
    {
        let mut e = EoiLinkage::new();
        let _ = e.timer_fire(TIMER_VECTOR, 4);
        let _ = e.irq_other(IRQ_IPI);
        let _ = e.irq_other(IRQ_SPURIOUS);
        s.add("C59-07-封账指纹", e.seal().is_ok(), "");
    }
    // C59-08 指纹一致
    {
        let f = || {
            let mut e = EoiLinkage::new();
            let _ = e.timer_fire(TIMER_VECTOR, 6);
            let _ = e.irq_other(IRQ_EXTERNAL);
            e.fingerprint()
        };
        s.add("C59-08-指纹一致", f() == f(), "");
    }
    // C59-09 注入核 6/6
    {
        let (n, c) = inject_core();
        let ok = n == 6 && c == 6;
        s.add("C59-09-注入核全抓", ok, if ok { "" } else { "注入漏抓" });
    }
    // C59-10 拒收计数
    {
        let mut e = EoiLinkage::new();
        let _ = e.irq_other(9);
        s.add("C59-10-拒收计数", e.reject_count() == 1, if e.reject_count() == 1 { "" } else { "拒收漂移" });
    }
    s
}
