//! UNX-F48526 · E·换装失败注入（反：坏库换装 ×10 回退旧库零空窗）
//!
//! 判据原文：UNX-F48526-J1 —— 坏库换装 ×10 全部安全回退：每次坏库
//! 装载失败后 active 版本不变、服务零空窗（失败前后查询全成功）；staging
//! 清理干净（失败后无残留状态，可立即发起新换装）；失败计数账精确
//! （失败 N 次 fail_n == N）；连续 10 次失败后服务健康度不变（查询成功率
//! 仍 100%）；好库穿插场景不受历史失败影响；同序列重放全等。
//!
//! 设计纪律：no_std（仅 core/alloc）；零 unwrap/expect/panic；fuzz 用
//! 确定性 LCG（wrapping_mul/wrapping_add），同种子同序列。

use crate::alloc::vec;
use crate::alloc::vec::Vec;
use alloc::string::{String, ToString};

pub const SEG_BASE: u16 = 0xED60;

pub const E_RESIDUE: u16 = SEG_BASE + 1;
pub const E_GAP: u16 = SEG_BASE + 2;
pub const E_VERSION_DRIFT: u16 = SEG_BASE + 3;
pub const E_FAIL_MISS: u16 = SEG_BASE + 4;
pub const E_NONDET: u16 = SEG_BASE + 5;
pub const E_HEALTH: u16 = SEG_BASE + 6;

/// 回退引擎（与 48525 换装同域，聚焦失败路径）。
pub struct RollbackEngine {
    active_ver: u32,
    staging: Option<u32>,
    staging_bad: bool,
    pub fail_n: u64,
    pub ok_n: u64,
    pub served: Vec<u32>,
    pub health_ok: bool,
}

impl RollbackEngine {
    pub fn new(v0: u32) -> RollbackEngine {
        RollbackEngine {
            active_ver: v0,
            staging: None,
            staging_bad: false,
            fail_n: 0,
            ok_n: 0,
            served: Vec::new(),
            health_ok: true,
        }
    }

    pub fn active_version(&self) -> u32 {
        self.active_ver
    }

    /// staging 是否残留（失败后必须为空）。
    pub fn staging_clean(&self) -> bool {
        self.staging.is_none() && !self.staging_bad
    }

    pub fn begin_swap(&mut self, target: u32, bad: bool) -> Result<(), u16> {
        if !self.staging_clean() {
            return Err(E_RESIDUE);
        }
        if target <= self.active_ver {
            return Err(E_NONDET);
        }
        self.staging = Some(target);
        self.staging_bad = bad;
        Ok(())
    }

    /// 完成装载：bad → 回退（staging 清空、版本不变、fail++）；好 → 切换。
    pub fn finish_load(&mut self) -> Result<(), u16> {
        match self.staging {
            None => return Err(E_NONDET),
            Some(v) => {
                if self.staging_bad {
                    self.fail_n += 1;
                    self.staging = None;
                    self.staging_bad = false;
                    // 服务连续性：active 不动。
                    let _ = v;
                    Ok(())
                } else {
                    self.ok_n += 1;
                    self.active_ver = v;
                    self.staging = None;
                    Ok(())
                }
            }
        }
    }

    pub fn query(&mut self, fp: u64) -> Result<u32, u16> {
        let _ = fp;
        self.served.push(self.active_ver);
        Ok(self.active_ver)
    }

    /// 健康审计：served 全部 == active（无跨版本漂移即零空窗）。
    pub fn audit(&self, expect_n: usize, expect_ver: u32) -> Result<(), u16> {
        if self.served.len() != expect_n {
            return Err(E_GAP);
        }
        for v in self.served.iter() {
            if *v != expect_ver {
                return Err(E_HEALTH);
            }
        }
        if !self.health_ok {
            return Err(E_HEALTH);
        }
        Ok(())
    }
}

/// 确定性 LCG。
pub struct Lcg {
    st: u64,
}

impl Lcg {
    pub fn new(seed: u64) -> Lcg {
        Lcg { st: seed }
    }
    pub fn next(&mut self) -> u64 {
        self.st = self
            .st
            .wrapping_mul(1_664_525)
            .wrapping_add(1_013_904_223);
        self.st
    }
    pub fn below(&mut self, b: u64) -> u64 {
        if b == 0 {
            return 0;
        }
        self.next() % b
    }
}

/// R1 单次坏库回退：版本不变、查询照常。
pub fn r1_single_rollback() -> Result<(), u16> {
    let mut e = RollbackEngine::new(5);
    let _ = e.query(1)?;
    e.begin_swap(6, true)?;
    e.finish_load()?;
    if e.active_version() != 5 {
        return Err(E_VERSION_DRIFT);
    }
    if e.fail_n != 1 {
        return Err(E_FAIL_MISS);
    }
    if !e.staging_clean() {
        return Err(E_RESIDUE);
    }
    let v = e.query(2)?;
    if v != 5 {
        return Err(E_GAP);
    }
    e.audit(2, 5)
}

/// R2 ×10 连续坏库：版本恒定、失败账 10、全程零空窗。
pub fn r2_ten_failures() -> Result<(), u16> {
    let mut e = RollbackEngine::new(3);
    let mut qn = 0usize;
    for round in 0..10u32 {
        let _ = e.query(round as u64)?;
        qn += 1;
        e.begin_swap(4 + round, true)?;
        e.finish_load()?;
        let _ = e.query(round as u64)?;
        qn += 1;
        if e.active_version() != 3 {
            return Err(E_VERSION_DRIFT);
        }
    }
    if e.fail_n != 10 {
        return Err(E_FAIL_MISS);
    }
    e.audit(qn, 3)
}

/// R3 staging 清理：失败后可立即发起新换装（无残留拒收）。
pub fn r3_staging_clean_after_fail() -> Result<(), u16> {
    let mut e = RollbackEngine::new(1);
    e.begin_swap(2, true)?;
    e.finish_load()?;
    if !e.staging_clean() {
        return Err(E_RESIDUE);
    }
    // 立即再来一次好库换装成功。
    e.begin_swap(2, false)?;
    e.finish_load()?;
    if e.active_version() != 2 || e.ok_n != 1 {
        return Err(E_VERSION_DRIFT);
    }
    Ok(())
}

/// R4 失败计数精确：好 2 + 坏 3 → fail 3 ok 2。
pub fn r4_fail_ledger() -> Result<(), u16> {
    let mut e = RollbackEngine::new(0);
    let seq: [bool; 5] = [false, true, false, true, true];
    for (i, bad) in seq.iter().enumerate() {
        e.begin_swap((i + 1) as u32, *bad)?;
        e.finish_load()?;
    }
    if e.fail_n != 3 || e.ok_n != 2 {
        return Err(E_FAIL_MISS);
    }
    Ok(())
}

/// R5 坏库后版本零漂移：10 次坏库 served 轨迹全等。
pub fn r5_served_flat() -> Result<(), u16> {
    let mut e = RollbackEngine::new(8);
    for _ in 0..10u32 {
        e.begin_swap(9, true)?;
        e.finish_load()?;
        let _ = e.query(1)?;
    }
    for v in e.served.iter() {
        if *v != 8 {
            return Err(E_HEALTH);
        }
    }
    e.audit(10, 8)
}

/// R6 穿插好库：坏-好-坏-好 后版本只反映好库。
pub fn r6_interleave() -> Result<(), u16> {
    let mut e = RollbackEngine::new(0);
    e.begin_swap(1, true)?;
    e.finish_load()?; // fail
    e.begin_swap(2, false)?;
    e.finish_load()?; // ok → 2
    e.begin_swap(3, true)?;
    e.finish_load()?; // fail
    if e.active_version() != 2 {
        return Err(E_VERSION_DRIFT);
    }
    e.begin_swap(5, false)?;
    e.finish_load()?; // ok → 5
    if e.active_version() != 5 || e.fail_n != 2 || e.ok_n != 2 {
        return Err(E_VERSION_DRIFT);
    }
    Ok(())
}

/// R7 健康度：连续失败后查询成功率仍 100%（无 fail 注入到查询路径）。
pub fn r7_health_100() -> Result<(), u16> {
    let mut e = RollbackEngine::new(4);
    for _ in 0..10 {
        e.begin_swap(5, true)?;
        e.finish_load()?;
    }
    let mut ok_q = 0;
    for i in 0..20u64 {
        let v = e.query(i)?;
        if v == e.active_version() {
            ok_q += 1;
        }
    }
    if ok_q != 20 {
        return Err(E_HEALTH);
    }
    Ok(())
}

/// R8 fuzz：随机好/坏库 40 轮，最终版本 == 好库数累计。
pub fn r8_fuzz_swaps() -> Result<(), u16> {
    let mut g = Lcg::new(0x48526_0001);
    let mut e = RollbackEngine::new(0);
    let mut expect_ver: u32 = 0;
    for i in 0..40u64 {
        let bad = g.below(2) == 0;
        expect_ver += 1;
        e.begin_swap(expect_ver, bad)?;
        e.finish_load()?;
        let _ = e.query(i)?;
    }
    if e.active_version() != expect_ver {
        return Err(E_VERSION_DRIFT);
    }
    Ok(())
}

/// R9 空窗探测：每轮「查询-失败-查询」间隔内版本恒等（零空窗）。
pub fn r9_no_gap_probe() -> Result<(), u16> {
    let mut e = RollbackEngine::new(6);
    for round in 0..8u32 {
        let v1 = e.query(round as u64)?;
        e.begin_swap(100 + round, true)?;
        e.finish_load()?;
        let v2 = e.query(round as u64)?;
        if v1 != v2 {
            return Err(E_GAP);
        }
    }
    Ok(())
}

/// R10 确定性重放：同种子两轮全账全等。
pub fn r10_deterministic() -> Result<(), u16> {
    let run = |seed: u64| -> Result<(u32, u64, u64), u16> {
        let mut g = Lcg::new(seed);
        let mut e = RollbackEngine::new(0);
        let mut ver: u32 = 0;
        for _ in 0..30u64 {
            ver += 1;
            e.begin_swap(ver, g.below(2) == 0)?;
            e.finish_load()?;
            let _ = e.query(g.next())?;
        }
        Ok((e.active_version(), e.fail_n, e.ok_n))
    };
    let a = run(0x48526_0002)?;
    let b = run(0x48526_0002)?;
    if a != b {
        return Err(E_NONDET);
    }
    Ok(())
}

/// 判据主入口：R1..R10，hits 计数。
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;
    r1_single_rollback()?;
    hits += 1;
    r2_ten_failures()?;
    hits += 1;
    r3_staging_clean_after_fail()?;
    hits += 1;
    r4_fail_ledger()?;
    hits += 1;
    r5_served_flat()?;
    hits += 1;
    r6_interleave()?;
    hits += 1;
    r7_health_100()?;
    hits += 1;
    r8_fuzz_swaps()?;
    hits += 1;
    r9_no_gap_probe()?;
    hits += 1;
    r10_deterministic()?;
    hits += 1;
    Ok(hits)
}
