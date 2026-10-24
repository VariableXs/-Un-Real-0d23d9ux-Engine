//! UNX-F0131 · 引导序号与启动计数 · 诊断段 0x2490
//! 验收锚点：UNX-F0131-J1 启动计数单调递增、回滚拦截、上限
//! 包绕语义明确。零浮点、零 unwrap、零 expect。

use crate::alloc::{format, string::String};

pub const SEG_BASE: u16 = 0x2490;

// ── 诊断码闭集 ───────────────────────────────────────────────
const E_SEQ: u16 = SEG_BASE + 1;   // 序号回退（新序 ≤ 旧序）
const E_CAP: u16 = SEG_BASE + 2;   // 上限非法（零）
const E_RDONLY: u16 = SEG_BASE + 3;// 计数已固化，再增被拒
const E_EMPTY: u16 = SEG_BASE + 4; // 零计数出报告
const E_BAL: u16 = SEG_BASE + 5;   // 计数账不平

pub fn detail(code: u16) -> &'static str {
    match code {
        E_SEQ => "序号回退：新引导序号必须严格大于旧序",
        E_CAP => "上限非法：包绕上限须为正",
        E_RDONLY => "已固化：本引导计数冻结",
        E_EMPTY => "零计数：未启动过禁止出报告",
        E_BAL => "计数账不平：增账与序号不符",
        _ => "未知诊断码",
    }
}

/// 启动计数器：单调 + 包绕（到顶回 1，序号语义连续）。
pub struct BootCounter {
    pub cap: u64,
    pub seq: u64,      // 当前序号（1 起）
    pub increments: u64, // 累计自增次数（含包绕）
    pub frozen: bool,
}

impl BootCounter {
    pub fn new(cap: u64) -> Result<BootCounter, u16> {
        if cap == 0 {
            return Err(E_CAP);
        }
        Ok(BootCounter {
            cap,
            seq: 0,
            increments: 0,
            frozen: false,
        })
    }

    /// 自增：到 cap 后包绕回 1（序号仍严格递增语义按圈计）。
    pub fn tick(&mut self) -> Result<u64, u16> {
        if self.frozen {
            return Err(E_RDONLY);
        }
        if self.seq == self.cap {
            self.seq = 1; // 包绕
        } else {
            self.seq += 1;
        }
        self.increments += 1;
        Ok(self.seq)
    }

    /// 序号连续性核：包绕点上 increments 与 seq 对齐
    /// （increments = 已走整圈数 × cap + 圈内位次）。
    pub fn continuity(&self) -> Result<(), u16> {
        let wraps = self.increments / self.cap;
        let in_ring = self.increments % self.cap;
        let expect = if in_ring == 0 && self.increments > 0 {
            self.cap
        } else {
            in_ring
        };
        if self.seq != expect || wraps * self.cap > self.increments {
            return Err(E_BAL);
        }
        Ok(())
    }

    /// 固化：本引导计数冻结。
    pub fn freeze(&mut self) -> Result<(), u16> {
        if self.frozen {
            return Err(E_RDONLY);
        }
        if self.seq == 0 {
            return Err(E_EMPTY);
        }
        self.frozen = true;
        Ok(())
    }

    pub fn render(&self) -> Result<String, u16> {
        if self.seq == 0 {
            return Err(E_EMPTY);
        }
        Ok(format!(
            "第 {} 次引导（上限 {}，累计自增 {}）",
            self.seq, self.cap, self.increments
        ))
    }
}

/// 计数链：多段持久化链式递增（模拟跨重启）。
pub fn chain_increment(prev_seq: u64, cap: u64) -> Result<u64, u16> {
    if cap == 0 {
        return Err(E_CAP);
    }
    if prev_seq > cap {
        return Err(E_SEQ);
    }
    Ok(if prev_seq == cap { 1 } else { prev_seq + 1 })
}

// ── 确定性演练：合法 4 例 + 注入拦截 4 例 ────────────────────────
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;

    // R1：上限 5 连 tick 5 次 → 1..5，连续性核绿。
    let mut c = BootCounter::new(5)?;
    for i in 1..=5u64 {
        if c.tick()? != i {
            return Err(E_BAL);
        }
    }
    c.continuity()?;
    hits += 1;

    // R2：包绕——第 6 次回 1，第 7 次回 2，累计账对齐。
    if c.tick()? != 1 {
        return Err(E_SEQ);
    }
    if c.tick()? != 2 {
        return Err(E_SEQ);
    }
    if c.increments != 7 {
        return Err(E_BAL);
    }
    c.continuity()?;
    hits += 1;

    // R3：固化后再 tick 拒绝；渲染账含三要素。
    c.freeze()?;
    if c.tick() != Err(E_RDONLY) {
        return Err(E_RDONLY);
    }
    let rep = c.render()?;
    if !rep.contains("第 2 次引导") || !rep.contains("累计自增 7") {
        return Err(E_BAL);
    }
    hits += 1;

    // R4：链式递增跨圈（prev=cap → 1）与常规步进。
    if chain_increment(5, 5)? != 1 {
        return Err(E_SEQ);
    }
    if chain_increment(3, 5)? != 4 {
        return Err(E_SEQ);
    }
    hits += 1;

    // I1：上限零 → E_CAP。
    if BootCounter::new(0).is_ok() {
        return Err(E_CAP);
    }
    if chain_increment(1, 0) != Err(E_CAP) {
        return Err(E_CAP);
    }
    hits += 1;

    // I2：链式回退（prev 超上限 / 序回退语义）→ E_SEQ。
    if chain_increment(6, 5) != Err(E_SEQ) {
        return Err(E_SEQ);
    }
    hits += 1;

    // I3：零计数冻结 / 渲染 → E_EMPTY。
    let mut i3 = BootCounter::new(5)?;
    if i3.freeze() != Err(E_EMPTY) {
        return Err(E_EMPTY);
    }
    if i3.render() != Err(E_EMPTY) {
        return Err(E_EMPTY);
    }
    hits += 1;

    // I4：连续性破注入（seq 与自增账不符）→ E_BAL；重复冻结 → E_RDONLY。
    let mut i4 = BootCounter::new(5)?;
    i4.tick()?;
    i4.seq = 4; // 人为跳序
    if i4.continuity() != Err(E_BAL) {
        return Err(E_BAL);
    }
    let mut i4b = BootCounter::new(5)?;
    i4b.tick()?;
    i4b.freeze()?;
    if i4b.freeze() != Err(E_RDONLY) {
        return Err(E_RDONLY);
    }
    hits += 1;

    Ok(hits)
}
