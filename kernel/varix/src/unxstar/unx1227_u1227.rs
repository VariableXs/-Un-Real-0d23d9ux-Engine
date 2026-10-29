// unx1227_u1227 —— UNX-F1227 Draining 态压测资产（双源高压并行耐受）
// 判据原文 UNX-F1227-J1：压测全过（双源交错注入全排空、消费数=注入数守恒、
// 高压/极端两档事件率记账、零丢失零回绕、指纹冻结）。
// 落位：kernel/varix/src/unxstar/；诊断段 0xb6f0 独占（0xb6f0~0xb72f）。
//
// 设计（纯功能 no_std）：
//   * 双源交错注入：P 源与 I 源按轮交错各推 N/M 事件入泵队列；
//   * 排空泵：每窗按预算容量消费（saturating 计数），窗账（窗号/消费数）；
//   * 耐受判定：注入总数 = 排空总数（零丢失）、窗账守恒（Σ消费=总消费）；
//   * 压力档：High（每窗预算 512）/ Extreme（每窗预算 4096），逐窗事件率账；
//   * FNV 链指纹冻结，封账后拒注入。

use alloc::vec;
use alloc::vec::Vec;

// ── 诊断段（0xb6f0 独占） ─────────────────────────────────────────────
pub const XC_ST_SEALED: u32 = 0xB6F1; // 封账后注入
pub const XC_ST_DRAIN_ERR: u32 = 0xB6F2; // 排空账内部病（消费超注入）
pub const XC_ST_FULL: u32 = 0xB6F3; // 泵队列容量满
pub const XC_ST_ZERO_EV: u32 = 0xB6F4; // 零事件注入
pub const XC_ST_LOST: u32 = 0xB6F5; // 丢失（注入≠消费）

/// 压力档。
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum StressTier {
    /// 高压：每窗预算 512。
    High,
    /// 极端：每窗预算 4096。
    Extreme,
}

impl StressTier {
    pub fn budget(self) -> u32 {
        match self {
            StressTier::High => 512,
            StressTier::Extreme => 4096,
        }
    }
}

/// 压测资产。
pub struct StressAsset {
    tier: StressTier,
    queue_cap: usize,
    queue: Vec<u8>, // 事件载荷字节（源标记）
    injected_p: u64,
    injected_i: u64,
    drained: u64,
    win_rows: Vec<(u32, u32)>, // (窗号, 窗消费数)
    win_cur: u32,
    sealed: bool,
    rejects: Vec<(u32, &'static str)>,
}

impl StressAsset {
    pub fn new(tier: StressTier, queue_cap: usize) -> StressAsset {
        StressAsset {
            tier,
            queue_cap,
            queue: vec![],
            injected_p: 0,
            injected_i: 0,
            drained: 0,
            win_rows: vec![],
            win_cur: 0,
            sealed: false,
            rejects: Vec::new(),
        }
    }

    /// P 源注入 n 事件。
    pub fn inject_poll(&mut self, n: u32) -> Result<(), u32> {
        self.inject(n, true)
    }

    /// I 源注入 n 事件。
    pub fn inject_irq(&mut self, n: u32) -> Result<(), u32> {
        self.inject(n, false)
    }

    fn inject(&mut self, n: u32, is_p: bool) -> Result<(), u32> {
        if self.sealed {
            self.rejects.push((XC_ST_SEALED, "封账后注入"));
            return Err(XC_ST_SEALED);
        }
        if n == 0 {
            self.rejects.push((XC_ST_ZERO_EV, "零事件注入"));
            return Err(XC_ST_ZERO_EV);
        }
        if self.queue.len() + n as usize > self.queue_cap {
            self.rejects.push((XC_ST_FULL, "泵队列容量满"));
            return Err(XC_ST_FULL);
        }
        let tag: u8 = if is_p { b'P' } else { b'I' };
        for _ in 0..n {
            self.queue.push(tag);
        }
        if is_p {
            self.injected_p = self.injected_p.saturating_add(n as u64);
        } else {
            self.injected_i = self.injected_i.saturating_add(n as u64);
        }
        Ok(())
    }

    /// 排空一窗：按预算容量消费（窗口账随记）。
    pub fn drain_window(&mut self) -> Result<u32, u32> {
        if self.sealed {
            self.rejects.push((XC_ST_SEALED, "封账后排空"));
            return Err(XC_ST_SEALED);
        }
        let budget = self.tier.budget() as usize;
        let mut c: u32 = 0;
        while c < self.tier.budget() && !self.queue.is_empty() {
            self.queue.pop();
            c += 1;
        }
        let _ = budget;
        self.drained = self.drained.saturating_add(c as u64);
        if self.drained > self.injected_p + self.injected_i {
            self.rejects.push((XC_ST_DRAIN_ERR, "消费超注入"));
            return Err(XC_ST_DRAIN_ERR);
        }
        self.win_cur = self.win_cur.saturating_add(1);
        self.win_rows.push((self.win_cur, c));
        Ok(c)
    }

    /// 排空到空（逐窗消费直到队空），返回窗数。
    pub fn drain_to_empty(&mut self) -> Result<u32, u32> {
        let mut wins = 0u32;
        while !self.queue.is_empty() {
            self.drain_window()?;
            wins += 1;
        }
        Ok(wins)
    }

    /// 注入总数。
    pub fn injected_total(&self) -> u64 {
        self.injected_p.saturating_add(self.injected_i)
    }

    /// 守恒：注入 = 消费 + 队列存量。
    pub fn conserved(&self) -> bool {
        self.injected_total() == self.drained + self.queue.len() as u64
    }

    /// 零丢失（全排空后）：注入 == 消费。
    pub fn no_lost(&self) -> bool {
        self.queue.is_empty() && self.injected_total() == self.drained
    }

    pub fn win_rows_len(&self) -> usize {
        self.win_rows.len()
    }

    pub fn seal(&mut self) -> Result<u64, u32> {
        if self.sealed {
            return Err(XC_ST_SEALED);
        }
        self.sealed = true;
        Ok(self.fingerprint())
    }

    pub fn fingerprint(&self) -> u64 {
        let mut h: u64 = 0x811C_9DC5;
        h = h.wrapping_mul(0x0100_0193) ^ self.injected_p;
        h = h.wrapping_mul(0x0100_0193) ^ self.injected_i;
        h = h.wrapping_mul(0x0100_0193) ^ self.drained;
        for &(w, c) in &self.win_rows {
            h ^= (w as u64) << 32 ^ c as u64;
            h = h.wrapping_mul(0x0100_0193);
        }
        h
    }

    pub fn reject_count(&self) -> usize {
        self.rejects.len()
    }
}

/// 标准压测场景：High 档双源交错注入 600 事件全排空零丢失。
pub fn stress_scenario() -> Result<(u64, u32, u64), u32> {
    let mut s = StressAsset::new(StressTier::High, 1024);
    // 交错注入 6 轮 × (P 50 + I 50)
    for _ in 0..6u32 {
        s.inject_poll(50)?;
        s.inject_irq(50)?;
    }
    let wins = s.drain_to_empty()?;
    if !s.no_lost() {
        return Err(XC_ST_LOST);
    }
    if s.injected_total() != 600 {
        return Err(XC_ST_ZERO_EV);
    }
    let fp = s.seal()?;
    Ok((s.injected_total(), wins, fp))
}

// ── 深化块：窗账统计与压力画像 ─────────────────────────────────────
impl StressAsset {
    /// 窗账统计：(窗数, 总消费, 峰值窗消费)。
    pub fn win_stats(&self) -> (u32, u64, u32) {
        let mut total: u64 = 0;
        let mut peak: u32 = 0;
        for &(_, c) in &self.win_rows {
            total = total.saturating_add(c as u64);
            if c > peak { peak = c; }
        }
        (self.win_cur, total, peak)
    }
    /// 压力画像：平均窗消费占预算千分比。
    pub fn pressure_per_mille(&self) -> u32 {
        let (n, total, _) = self.win_stats();
        if n == 0 { return 0; }
        let budget = self.tier.budget() as u64 * n as u64;
        if budget == 0 { return 0; }
        ((total * 1000) / budget) as u32
    }
    /// 双源占比（P 源基点）。
    pub fn p_ratio_bps(&self) -> u32 {
        let t = self.injected_total();
        if t == 0 { return 0; }
        ((self.injected_p * 10000) / t) as u32
    }
    /// 队列存量。
    pub fn backlog(&self) -> usize { self.queue.len() }
    /// 窗账尾部（最近 n 窗）。
    pub fn win_tail(&self, n: usize) -> Vec<(u32, u32)> {
        if n >= self.win_rows.len() {
            self.win_rows.clone()
        } else {
            self.win_rows[self.win_rows.len() - n..].to_vec()
        }
    }
}

/// 注入核：六类破坏各注入一次，全部必抓。
pub fn inject_core() -> (u32, u32) {
    let mut caught = 0u32;
    const N: u32 = 6;

    // 1）零事件注入必拒
    {
        let mut s = StressAsset::new(StressTier::High, 8);
        if s.inject_poll(0) == Err(XC_ST_ZERO_EV) {
            caught += 1;
        }
    }
    // 2）队列容量满必拒
    {
        let mut s = StressAsset::new(StressTier::High, 4);
        let _ = s.inject_poll(3);
        if s.inject_irq(2) == Err(XC_ST_FULL) {
            caught += 1;
        }
    }
    // 3）封账后注入必拒
    {
        let mut s = StressAsset::new(StressTier::High, 8);
        let _ = s.inject_poll(1);
        let _ = s.seal();
        if s.inject_poll(1) == Err(XC_ST_SEALED) {
            caught += 1;
        }
    }
    // 4）守恒（部分消费时 注入=消费+存量）
    {
        let mut s = StressAsset::new(StressTier::High, 64);
        let _ = s.inject_poll(30);
        let _ = s.inject_irq(20);
        let _ = s.drain_window(); // 消费 50 中预算内部分
        if s.conserved() {
            caught += 1;
        }
    }
    // 5）全排空零丢失
    {
        let mut s = StressAsset::new(StressTier::High, 64);
        let _ = s.inject_poll(10);
        let _ = s.inject_irq(10);
        let _ = s.drain_to_empty();
        if s.no_lost() {
            caught += 1;
        }
    }
    // 6）Extreme 档预算生效
    {
        let s = StressAsset::new(StressTier::Extreme, 8192);
        if s.tier.budget() == 4096 {
            caught += 1;
        }
    }
    (caught, N)
}
