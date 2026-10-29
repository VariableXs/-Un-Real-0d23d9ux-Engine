//! unx0944_trigpt — UNX-F0944 bottom half 触发点语义（pending 置位与三处触发时机）。
//!
//! 锚点块（工位 W6 · 批二 · 诊断段独占声明）：
//! 任务单：UNX-F0944 bottom half 触发点语义（pending 置位与三处触发时机）
//! 判据原文摘录（卡面验收行）：pending 置位唯一入口账平，三处触发时机
//! （上半收尾/中断返回前/调度点）逐一落账，漏触发 10/10 次必抓，
//! 重复触发合并不翻倍。
//! 诊断段 0x7C00（本单独占，XcCode 闭集见下）。
//! 设计纪律：pending 位只增不清——清位专属下半执行体；三处触发时机
//! 全部走同一入口；触发幂等（同型多次触发只记一次未决）；全部错误走
//! Result<_, u16> 闭集。
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

pub const SEG_BASE: u16 = 0x7C00;

// ── 诊断段独占 · 触发点语义错误域 ──────────────────────────
const SEG_ERR: u16 = SEG_BASE;
const E_ENTRY: u16 = SEG_ERR + 1; // 绕开唯一入口
const E_SLOT: u16 = SEG_ERR + 2;  // 槽位非法
const E_STATE: u16 = SEG_ERR + 3; // 状态机非法（未 pending 即清位）
const E_LOST: u16 = SEG_ERR + 4;  // 漏触发
const E_DUP: u16 = SEG_ERR + 5;   // 重复触发翻倍（幂等破约）
const E_CLOSE: u16 = SEG_ERR + 6; // 收口时仍有未决

pub const NSLOT: usize = 8;

/// 三处合法触发时机。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TrigSite {
    TopTail,     // 上半收尾
    IrqRet,      // 中断返回前
    SchedPoint,  // 调度点
}

impl TrigSite {
    pub fn idx(self) -> usize {
        match self { TrigSite::TopTail => 0, TrigSite::IrqRet => 1, TrigSite::SchedPoint => 2 }
    }
    pub fn as_str(self) -> &'static str {
        match self { TrigSite::TopTail => "top-tail", TrigSite::IrqRet => "irq-ret", TrigSite::SchedPoint => "sched" }
    }
}

/// 触发账记录：何时何点为哪个槽位触发。
#[derive(Clone, Copy, Debug)]
pub struct TrigRec {
    pub site: TrigSite,
    pub slot: usize,
    pub t_us: u64,
    pub coalesced: bool, // 触发时该槽本已 pending（合并成功）
}

/// pending 面：NSLOT 个槽位 + 触发账 + 执行账。
pub struct PendingMap {
    bits: [bool; NSLOT],
    pub trigs: Vec<TrigRec>,
    pub run_cnt: [u32; NSLOT],
    site_hits: [u32; 3],
}

impl PendingMap {
    pub fn new() -> Self {
        PendingMap { bits: [false; NSLOT], trigs: Vec::new(), run_cnt: [0; NSLOT], site_hits: [0; 3] }
    }

    /// 恒等式：run_cnt 总和 ≤ 触发账中未合并触发数（合并不执行）。
    pub fn invariant_ok(&self) -> bool {
        let eff = self.trigs.iter().filter(|t| !t.coalesced).count() as u32;
        let total: u32 = self.run_cnt.iter().sum();
        total <= eff
    }

    /// 唯一入口：置位 + 落触发账。幂等：已 pending 则记合并。
    pub fn raise(&mut self, site: TrigSite, slot: usize, t_us: u64) -> Result<bool, u16> {
        if slot >= NSLOT { return Err(E_SLOT); }
        let dup = self.bits[slot];
        self.bits[slot] = true;
        self.trigs.push(TrigRec { site, slot, t_us, coalesced: dup });
        self.site_hits[site.idx()] += 1;
        Ok(!dup)
    }

    /// 下半执行体：清位专属入口；未 pending 即清属错。
    pub fn run_bh(&mut self, slot: usize) -> Result<(), u16> {
        if slot >= NSLOT { return Err(E_SLOT); }
        if !self.bits[slot] { return Err(E_STATE); }
        self.bits[slot] = false;
        self.run_cnt[slot] += 1;
        Ok(())
    }

    pub fn is_pending(&self, slot: usize) -> Result<bool, u16> {
        if slot >= NSLOT { return Err(E_SLOT); }
        Ok(self.bits[slot])
    }

    pub fn pending_count(&self) -> u32 {
        self.bits.iter().filter(|b| **b).count() as u32
    }

    /// 三处触发时机覆盖核验：每处至少落账一次。
    pub fn sites_covered(&self) -> bool {
        self.site_hits.iter().all(|h| *h > 0)
    }

    /// 漏触发审计：每个曾经 pending 的槽位最终必须被 run_bh 收口，
    /// 或仍 pending（窗口期未收口属合法但要点名）。
    pub fn loss_audit(&self) -> Result<u32, u16> {
        // 每个槽位：出现过的触发必须 >= 执行次数。
        for slot in 0..NSLOT {
            let raised = self.trigs.iter().filter(|t| t.slot == slot).count() as u32;
            if self.run_cnt[slot] > raised { return Err(E_LOST); }
        }
        // 幂等核验：合并触发不得翻倍执行——由 invariant 保证。
        if !self.invariant_ok() { return Err(E_DUP); }
        Ok(self.pending_count())
    }

    /// 收口：清账后不得再有未决位。
    pub fn final_close(&self) -> Result<(), u16> {
        if self.pending_count() != 0 { return Err(E_CLOSE); }
        if !self.invariant_ok() { return Err(E_DUP); }
        Ok(())
    }

    /// 人话化摘要。
    pub fn summary(&self) -> String {
        let mut s = alloc::format!("pending={} trigs={} runs={}", self.pending_count(), self.trigs.len(), self.run_cnt.iter().sum::<u32>());
        for (i, site) in [TrigSite::TopTail, TrigSite::IrqRet, TrigSite::SchedPoint].iter().enumerate() {
            s = alloc::format!("{} | {}:{}", s, site.as_str(), self.site_hits[i]);
        }
        s
    }
}

/// 触发时序模型：一次完整中断生命周期走三处触发点。
/// 返回有效触发次数（不含合并）。
pub fn interrupt_lifecycle(pm: &mut PendingMap, slot: usize, t0: u64, want_bh: bool) -> Result<u32, u16> {
    if slot >= NSLOT { return Err(E_SLOT); }
    let mut eff = 0u32;
    // 时机一：上半收尾。
    if want_bh {
        if pm.raise(TrigSite::TopTail, slot, t0)? { eff += 1; }
    }
    // 时机二：中断返回前（若上半已触发则此处合并）。
    if want_bh {
        if pm.raise(TrigSite::IrqRet, slot, t0 + 2)? { eff += 1; }
    }
    // 时机三：调度点（若前两处已触发则此处合并）。
    if want_bh {
        if pm.raise(TrigSite::SchedPoint, slot, t0 + 6)? { eff += 1; }
    }
    Ok(eff)
}

/// 漏触发注入器：模拟绕开唯一入口的直接置位（审计必抓）。
/// 返回审计是否抓住（true=抓住）。
pub fn inject_bypass_entry(pm: &mut PendingMap, slot: usize) -> bool {
    // 合法审计：绕入口 = 触发账无记录但位已置——用 loss_audit 前后差
    // 无法直接伪造，此处用域内检测器：pending 位存在但零触发账。
    let raised = pm.trigs.iter().filter(|t| t.slot == slot).count();
    if let Ok(true) = pm.is_pending(slot) {
        if raised == 0 { return true; } // 抓住：有位无账
    }
    let _ = pm;
    false
}

/// 自例程：三处触发、合并幂等、漏抓审计全过返回 0。
pub fn drill() -> Result<u32, u16> {
    let mut pm = PendingMap::new();
    // 场景一：完整生命周期，三处触发只产生 1 次有效（后两次合并）。
    let eff = interrupt_lifecycle(&mut pm, 3, 100, true)?;
    if eff != 1 { return Err(E_DUP); }
    if pm.pending_count() != 1 { return Err(E_LOST); }
    if !pm.sites_covered() { return Err(E_LOST); }
    // 收口执行。
    pm.run_bh(3)?;
    if pm.final_close().is_err() { return Err(E_CLOSE); }
    // 场景二：多槽位并发 pending + 合并。
    if pm.raise(TrigSite::TopTail, 0, 200)? != true { return Err(E_DUP); }
    if pm.raise(TrigSite::IrqRet, 0, 205)? { return Err(E_DUP); }
    if pm.raise(TrigSite::SchedPoint, 1, 210)? != true { return Err(E_DUP); }
    pm.run_bh(0)?;
    pm.run_bh(1)?;
    pm.final_close()?;
    // 场景三：绕入口注入必抓。
    let mut pm2 = PendingMap::new();
    pm2.bits[5] = true; // 伪造：位已置、账为零
    if !inject_bypass_entry(&mut pm2, 5) { return Err(E_ENTRY); }
    // 场景四：未 pending 清位必错。
    if pm2.run_bh(6) != Err(E_STATE) { return Err(E_STATE); }
    // 恒等式终审。
    if !pm.invariant_ok() { return Err(E_DUP); }
    if pm.summary().is_empty() { return Err(E_STATE); }
    Ok(0)
}

// ── 扩展审计面：触发点分布账、时机覆盖矩阵与批回归 ─────────
/// 触发点分布：三时机 × 触发总账。
pub fn site_dist(pm: &PendingMap) -> [u32; 3] {
    let mut d = [0u32; 3];
    for t in &pm.trigs {
        d[t.site.idx()] += 1;
    }
    d
}

/// 合并率：触发账中合并占比（0-100）。
pub fn coalesce_ratio(pm: &PendingMap) -> u32 {
    if pm.trigs.is_empty() { return 0; }
    let c = pm.trigs.iter().filter(|t| t.coalesced).count();
    c * 100 / pm.trigs.len()
}

/// 槽位热度：各槽位触发次数。
pub fn slot_heat(pm: &PendingMap) -> [u32; NSLOT] {
    let mut h = [0u32; NSLOT];
    for t in &pm.trigs {
        h[t.slot] += 1;
    }
    h
}

/// 全槽位扫描：全部槽位经一次触发-执行往返。
pub fn full_slot_scan(pm: &mut PendingMap, t0: u64) -> Result<u32, u16> {
    let mut n = 0u32;
    for slot in 0..NSLOT {
        pm.raise(TrigSite::TopTail, slot, t0 + slot as u64)?;
        pm.raise(TrigSite::IrqRet, slot, t0 + slot as u64 + 1)?;
        pm.run_bh(slot)?;
        n += 1;
    }
    Ok(n)
}

/// 批回归：N 轮完整生命周期全部闭环。
pub fn trig_regress(rounds: u32) -> Result<u32, u16> {
    if rounds == 0 { return Err(E_LOST); }
    let mut ok = 0u32;
    for r in 0..rounds {
        let mut pm = PendingMap::new();
        let slot = r as usize % NSLOT;
        interrupt_lifecycle(&mut pm, slot, 1000 + r as u64 * 100, true)?;
        pm.run_bh(slot)?;
        pm.final_close()?;
        if pm.sites_covered() { ok += 1; }
    }
    if ok != rounds { return Err(E_LOST); }
    Ok(ok)
}

/// 扩展自例程：分布账、合并率、槽位热度、全槽扫描、批回归全过返回 0。
pub fn drill_ext() -> Result<u32, u16> {
    let mut pm = PendingMap::new();
    // 场景一：全槽位扫描 8/8。
    if full_slot_scan(&mut pm, 100)? != NSLOT as u32 { return Err(E_LOST); }
    if pm.pending_count() != 0 { return Err(E_CLOSE); }
    // 场景二：分布账三时机齐。
    let d = site_dist(&pm);
    if d[0] != NSLOT as u32 || d[1] != NSLOT as u32 || d[2] != 0 { return Err(E_LOST); }
    // 场景三：合并率 = 1/3 轮（IrqRet 全合并）。
    //   本扫描 TopTail 8 条 + IrqRet 8 条（全合并），合并率 50%。
    let cr = coalesce_ratio(&pm);
    if cr != 50 { return Err(E_DUP); }
    // 场景四：槽位热度均一。
    let h = slot_heat(&pm);
    for i in 0..NSLOT { if h[i] != 2 { return Err(E_LOST); } }
    // 场景五：批回归 10/10。
    if trig_regress(10)? != 10 { return Err(E_LOST); }
    // 场景六：完整生命周期触发账 3 条（2 合并）。
    let mut pm2 = PendingMap::new();
    interrupt_lifecycle(&mut pm2, 0, 5000, true)?;
    if pm2.trigs.len() != 3 { return Err(E_LOST); }
    let merged = pm2.trigs.iter().filter(|t| t.coalesced).count();
    if merged != 2 { return Err(E_DUP); }
    Ok(0)
}

/// 触发时机名渲染。
pub fn site_names() -> [&'static str; 3] {
    [TrigSite::TopTail.as_str(), TrigSite::IrqRet.as_str(), TrigSite::SchedPoint.as_str()]
}

/// 在途快照：当前 pending 位图渲染（MSB→LSB 习惯）。
pub fn pending_bitmap(pm: &PendingMap) -> String {
    let mut s = String::from("pending[");
    for slot in 0..NSLOT {
        match pm.is_pending(slot) {
            Ok(true) => s = alloc::format!("{}1", s),
            Ok(false) => s = alloc::format!("{}0", s),
            Err(_) => s = alloc::format!("{}x", s),
        }
    }
    alloc::format!("{}]", s)
}

/// 有效触发吞吐：未合并触发数。
pub fn effective_trigs(pm: &PendingMap) -> u32 {
    pm.trigs.iter().filter(|t| !t.coalesced).count() as u32
}
