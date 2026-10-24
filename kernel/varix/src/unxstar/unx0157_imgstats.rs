//! UNX-F0157 · 镜像引导成功率统计接口 · 诊断段 0x2670
//!
//! 判据原文（UNX-F0157-J1）：按镜像版本聚合引导结局（成功/失败），
//! 接口只读；聚合账与明细账守恒（Σ版本聚合 = Σ明细行）；
//! 千分比成功率（整型，无浮点）。
//!
//! 设计要点：版本键（主.次.补折叠 u32）→ 聚合行（成/败/总）；
//! 明细行闭集（版本键 + 结局二态）；聚合器只读扫描明细册。
//! 零浮点、零 unwrap/expect。

use crate::alloc::vec::Vec;

pub const SEG_BASE: u16 = 0x2680;

const E_VER: u16 = SEG_BASE + 1; // 版本字段越纲（>999）
const E_BAL: u16 = SEG_BASE + 2; // 聚合与明细守恒被破坏
const E_EMPTY: u16 = SEG_BASE + 3; // 空明细聚合拒（口径：至少一行）
const E_SEAL: u16 = SEG_BASE + 4; // 明细册已封拒写

/// 版本键折叠：主<<20 | 次<<10 | 补（各 0~999）。
pub fn ver_key(major: u16, minor: u16, patch: u16) -> Result<u32, u16> {
    if major > 999 || minor > 999 || patch > 999 {
        return Err(E_VER);
    }
    Ok(((major as u32) << 20) | ((minor as u32) << 10) | patch as u32)
}

/// 版本键展开。
pub fn ver_unfold(k: u32) -> (u16, u16, u16) {
    (
        (k >> 20) as u16,
        ((k >> 10) & 0x3FF) as u16,
        (k & 0x3FF) as u16,
    )
}

/// 引导结局二态。
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum BootOk {
    Yes,
    No,
}

/// 明细行。
#[derive(Clone, Copy)]
pub struct StatRow {
    pub key: u32,
    pub ok: BootOk,
}

/// 聚合行。
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct AggRow {
    pub key: u32,
    pub ok_cnt: u32,
    pub fail_cnt: u32,
}

impl AggRow {
    pub fn total(&self) -> u32 {
        self.ok_cnt + self.fail_cnt
    }

    /// 千分比成功率（整型）。
    pub fn permille(&self) -> u32 {
        if self.total() == 0 {
            0
        } else {
            self.ok_cnt * 1000 / self.total()
        }
    }
}

/// 明细册（只读聚合的数据面）。
pub struct StatLedger {
    pub rows: Vec<StatRow>,
    pub sealed: bool,
}

impl StatLedger {
    pub fn new() -> Result<Self, u16> {
        Ok(StatLedger { rows: Vec::new(), sealed: false })
    }

    pub fn record(&mut self, key: u32, ok: BootOk) -> Result<(), u16> {
        if self.sealed {
            return Err(E_SEAL);
        }
        let (_, m, p) = ver_unfold(key);
        if m > 999 || p > 999 || (key >> 20) > 999 {
            return Err(E_VER);
        }
        self.rows.push(StatRow { key, ok });
        Ok(())
    }

    pub fn seal(&mut self) {
        self.sealed = true;
    }

    /// 只读聚合器：按键分组聚合计数（输出按键升序）。
    pub fn aggregate(&self) -> Result<Vec<AggRow>, u16> {
        if self.rows.is_empty() {
            return Err(E_EMPTY);
        }
        let mut keys: Vec<u32> = Vec::new();
        for r in &self.rows {
            if !keys.contains(&r.key) {
                keys.push(r.key);
            }
        }
        keys.sort_unstable();
        let mut out = Vec::new();
        for k in keys {
            let ok_cnt = self
                .rows
                .iter()
                .filter(|r| r.key == k && r.ok == BootOk::Yes)
                .count() as u32;
            let fail_cnt = self
                .rows
                .iter()
                .filter(|r| r.key == k && r.ok == BootOk::No)
                .count() as u32;
            out.push(AggRow { key: k, ok_cnt, fail_cnt });
        }
        Ok(out)
    }
}

/// 聚合守恒：Σ(ok+fail) == 明细行数；Σok == 明细 Yes 数。
pub fn agg_consistent(led: &StatLedger, agg: &[AggRow]) -> Result<(), u16> {
    let agg_total: u32 = agg.iter().map(|a| a.total()).sum();
    let yes = led.rows.iter().filter(|r| r.ok == BootOk::Yes).count() as u32;
    let agg_ok: u32 = agg.iter().map(|a| a.ok_cnt).sum();
    if agg_total != led.rows.len() as u32 || agg_ok != yes {
        return Err(E_BAL);
    }
    Ok(())
}

/// 成功率接口（只读门面）：版本三元组 → 千分比。
pub fn success_permille(led: &StatLedger, major: u16, minor: u16, patch: u16) -> Result<u32, u16> {
    let k = ver_key(major, minor, patch)?;
    let agg = led.aggregate()?;
    agg.iter()
        .find(|a| a.key == k)
        .map(|a| a.permille())
        .ok_or(E_BAL)
}

// ── 确定性演练：合法 4 例 + 注入拦截 4 例 ─────────────────────────

/// 演练入口（ktest 挂点）。
pub fn drill() -> Result<(), u16> {
    let mut hits = 0u32;

    // L1：三版本十四行明细 → 聚合三行 → 守恒。
    let mut led = StatLedger::new()?;
    let k1 = ver_key(1, 0, 0)?;
    let k2 = ver_key(1, 0, 1)?;
    let k3 = ver_key(2, 3, 7)?;
    for i in 0..6 {
        led.record(k1, if i < 5 { BootOk::Yes } else { BootOk::No })?;
    }
    for i in 0..4 {
        led.record(k2, if i < 2 { BootOk::Yes } else { BootOk::No })?;
    }
    for _ in 0..4 {
        led.record(k3, BootOk::Yes)?;
    }
    led.seal();
    let agg = led.aggregate()?;
    if agg.len() != 3 {
        return Err(E_BAL);
    }
    agg_consistent(&led, &agg)?;
    hits += 1;

    // L2：千分比——k1: 5/6=833‰；k2: 2/4=500‰；k3: 4/4=1000‰。
    if agg[0].permille() != 833 || agg[1].permille() != 500 || agg[2].permille() != 1000 {
        return Err(E_BAL);
    }
    hits += 1;

    // L3：接口门面——按版本查千分比。
    if success_permille(&led, 1, 0, 0)? != 833 {
        return Err(E_BAL);
    }
    if success_permille(&led, 2, 3, 7)? != 1000 {
        return Err(E_BAL);
    }
    hits += 1;

    // L4：版本键折叠/展开往返。
    let (m, n, p) = ver_unfold(ver_key(999, 123, 45)?);
    if (m, n, p) != (999, 123, 45) {
        return Err(E_BAL);
    }
    hits += 1;

    // I1：版本字段越纲（折叠与登记双路）。
    if ver_key(1000, 0, 0) != Err(E_VER) {
        return Err(E_VER);
    }
    let mut i1 = StatLedger::new()?;
    if i1.record(u32::MAX, BootOk::Yes) != Err(E_VER) {
        return Err(E_VER);
    }
    hits += 1;

    // I2：空明细聚合拒。
    let i2 = StatLedger::new()?;
    if i2.aggregate() != Err(E_EMPTY) {
        return Err(E_EMPTY);
    }
    hits += 1;

    // I3：未收录版本接口查千分比拒（E_BAL）。
    if !matches!(success_permille(&led, 9, 9, 9), Err(E_BAL)) {
        return Err(E_BAL);
    }
    hits += 1;

    // I4：封册拒记。
    let mut i4 = StatLedger::new()?;
    i4.seal();
    if i4.record(1, BootOk::Yes) != Err(E_SEAL) {
        return Err(E_SEAL);
    }
    hits += 1;

    // I5：守恒注入——聚合行被抽走一行 → agg_consistent 必红。
    let mut agg5 = led.aggregate()?;
    agg5.remove(0);
    if agg_consistent(&led, &agg5) != Err(E_BAL) {
        return Err(E_BAL);
    }
    hits += 1;

    let _ = hits;
    Ok(())
}
