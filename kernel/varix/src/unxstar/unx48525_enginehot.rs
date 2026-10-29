//! UNX-F48525 · E·引擎热换装期输入（正：词库换装期查询不断流）
//!
//! 判据原文：UNX-F48525-J1 —— 词库热换装零空窗：换装流程装载期（旧库
//! 继续服务）→ 原子切换 → 卸载期，全程查询成功率 100%（无空窗、无
//! 失败）；服务版本单调不减（切换后不再回旧版本）；切换原子（不存在
//! 半新半旧混合应答）；装载失败路径不影响在服务库（坏库拒收，旧库
//! 照常）；换装期查询计数账守恒；同序列重放版本轨迹全等。
//!
//! 设计纪律：no_std（仅 core/alloc）；零 unwrap/expect/panic；fuzz 用
//! 确定性 LCG（wrapping_mul/wrapping_add），同种子同序列。

use crate::alloc::vec;
use crate::alloc::vec::Vec;
use alloc::string::{String, ToString};

pub const SEG_BASE: u16 = 0xED40;

pub const E_GAP: u16 = SEG_BASE + 1;
pub const E_VERSION_ROLLBACK: u16 = SEG_BASE + 2;
pub const E_MIXED_ANSWER: u16 = SEG_BASE + 3;
pub const E_LOAD_POISON: u16 = SEG_BASE + 4;
pub const E_NONDET: u16 = SEG_BASE + 5;
pub const E_LEDGER_BAD: u16 = SEG_BASE + 6;

/// 词库版本号。
pub type LexVer = u32;

/// 换装阶段。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SwapPhase {
    /// 稳定服务。
    Serving,
    /// 装载新库中（旧库服务）。
    Loading,
    /// 卸载旧库中（新库服务）。
    Unloading,
}

/// 双库引擎： Serving 用 active，Loading 装到 staging，原子 commit。
pub struct HotEngine {
    active_ver: LexVer,
    staging_ver: Option<LexVer>,
    staging_bad: bool,
    phase: SwapPhase,
    pub served: Vec<LexVer>,
    pub fail_n: u64,
}

impl HotEngine {
    pub fn new(v0: LexVer) -> HotEngine {
        HotEngine {
            active_ver: v0,
            staging_ver: None,
            staging_bad: false,
            phase: SwapPhase::Serving,
            served: Vec::new(),
            fail_n: 0,
        }
    }

    pub fn phase(&self) -> SwapPhase {
        self.phase
    }
    pub fn active_version(&self) -> LexVer {
        self.active_ver
    }

    /// 发起换装：装载目标版本（bad = 坏库模拟）。
    pub fn begin_swap(&mut self, target: LexVer, bad: bool) -> Result<(), u16> {
        if self.phase != SwapPhase::Serving {
            return Err(E_NONDET);
        }
        if target <= self.active_ver {
            // 版本回退的换装直接拒收。
            return Err(E_VERSION_ROLLBACK);
        }
        self.staging_ver = Some(target);
        self.staging_bad = bad;
        self.phase = SwapPhase::Loading;
        Ok(())
    }

    /// 完成装载：坏库 → 回 Serving（旧库不动）；好库 → 原子切换。
    pub fn finish_load(&mut self) -> Result<(), u16> {
        match self.phase {
            SwapPhase::Loading => {}
            _ => return Err(E_NONDET),
        }
        if self.staging_bad {
            // 坏库：丢弃 staging，旧库照常。
            self.staging_ver = None;
            self.staging_bad = false;
            self.phase = SwapPhase::Serving;
            self.fail_n += 1;
            return Ok(());
        }
        // 原子切换：active 一步到位。
        self.active_ver = match self.staging_ver {
            Some(v) => v,
            None => return Err(E_NONDET),
        };
        self.staging_ver = None;
        self.phase = SwapPhase::Unloading;
        Ok(())
    }

    /// 卸载完成：回 Serving。
    pub fn finish_unload(&mut self) -> Result<(), u16> {
        if self.phase != SwapPhase::Unloading {
            return Err(E_NONDET);
        }
        self.phase = SwapPhase::Serving;
        Ok(())
    }

    /// 查询：任何阶段都由 active 服务，返回应答版本。
    pub fn query(&mut self, fp: u64) -> Result<LexVer, u16> {
        let _ = fp;
        let v = self.active_ver;
        self.served.push(v);
        Ok(v)
    }

    /// 审计：版本轨迹单调不减 + 零空窗（served 长度 == 提交数）。
    pub fn audit(&self, expect_n: usize) -> Result<(), u16> {
        if self.served.len() != expect_n {
            return Err(E_GAP);
        }
        for i in 1..self.served.len() {
            if self.served[i] < self.served[i - 1] {
                return Err(E_VERSION_ROLLBACK);
            }
            // 原子性：同刻只可能有一个版本——单调已保证。
            if self.served[i] != self.served[i - 1] && self.served[i] != self.served[i - 1] + 1 {
                return Err(E_MIXED_ANSWER);
            }
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

/// R1 稳定期查询全走当前版本。
pub fn r1_serving_steady() -> Result<(), u16> {
    let mut e = HotEngine::new(7);
    for _ in 0..5 {
        let v = e.query(1)?;
        if v != 7 {
            return Err(E_VERSION_ROLLBACK);
        }
    }
    e.audit(5)
}

/// R2 换装期不断流：Loading 阶段查询仍由旧版本服务。
pub fn r2_loading_no_gap() -> Result<(), u16> {
    let mut e = HotEngine::new(3);
    let _ = e.query(1)?;
    e.begin_swap(4, false)?;
    if e.phase() != SwapPhase::Loading {
        return Err(E_NONDET);
    }
    // 装载期查询：仍 3。
    let v = e.query(2)?;
    if v != 3 {
        return Err(E_VERSION_ROLLBACK);
    }
    e.finish_load()?;
    // 切换后：新版本。
    let v2 = e.query(3)?;
    if v2 != 4 {
        return Err(E_VERSION_ROLLBACK);
    }
    e.finish_unload()?;
    let v3 = e.query(4)?;
    if v3 != 4 {
        return Err(E_VERSION_ROLLBACK);
    }
    e.audit(4)
}

/// R3 版本轨迹单调：多次换装版本只增不减。
pub fn r3_version_monotone() -> Result<(), u16> {
    let mut e = HotEngine::new(1);
    for round in 0..4u32 {
        e.begin_swap(2 + round, false)?;
        let _ = e.query(round as u64)?;
        e.finish_load()?;
        e.finish_unload()?;
    }
    e.audit(4)?;
    if e.active_version() != 5 {
        return Err(E_VERSION_ROLLBACK);
    }
    Ok(())
}

/// R4 坏库换装拒收：bad 库 finish_load 后旧库照常、fail 计数。
pub fn r4_bad_lib_fallback() -> Result<(), u16> {
    let mut e = HotEngine::new(9);
    e.begin_swap(10, true)?;
    e.finish_load()?;
    if e.phase() != SwapPhase::Serving {
        return Err(E_NONDET);
    }
    if e.active_version() != 9 {
        return Err(E_VERSION_ROLLBACK);
    }
    if e.fail_n != 1 {
        return Err(E_LEDGER_BAD);
    }
    // 旧库继续服务。
    let v = e.query(5)?;
    if v != 9 {
        return Err(E_GAP);
    }
    e.audit(1)
}

/// R5 版本回退换装拒收：target ≤ active 报 E_VERSION_ROLLBACK。
pub fn r5_rollback_reject() -> Result<(), u16> {
    let mut e = HotEngine::new(5);
    match e.begin_swap(5, false) {
        Err(E_VERSION_ROLLBACK) => {}
        _ => return Err(E_VERSION_ROLLBACK),
    }
    match e.begin_swap(3, false) {
        Err(E_VERSION_ROLLBACK) => {}
        _ => return Err(E_VERSION_ROLLBACK),
    }
    if e.active_version() != 5 {
        return Err(E_VERSION_ROLLBACK);
    }
    Ok(())
}

/// R6 原子性：切换前全旧、切换后全新、无混合。
pub fn r6_atomic_switch() -> Result<(), u16> {
    let mut e = HotEngine::new(2);
    for _ in 0..3 {
        let _ = e.query(0)?;
    }
    e.begin_swap(3, false)?;
    let _ = e.query(0)?; // 仍旧
    e.finish_load()?;
    for _ in 0..3 {
        let v = e.query(0)?;
        if v != 3 {
            return Err(E_MIXED_ANSWER);
        }
    }
    e.finish_unload()?;
    e.audit(7)
}

/// R7 阶段机守卫：Loading 中再 begin 拒收。
pub fn r7_phase_guard() -> Result<(), u16> {
    let mut e = HotEngine::new(1);
    e.begin_swap(2, false)?;
    match e.begin_swap(3, false) {
        Err(_) => {}
        Ok(()) => return Err(E_NONDET),
    }
    e.finish_load()?;
    e.finish_unload()?;
    Ok(())
}

/// R8 ×10 轮换装风暴：10 次好库 + 3 次坏库交替，服务流零断。
pub fn r8_ten_swaps() -> Result<(), u16> {
    let mut e = HotEngine::new(0);
    let mut ver: LexVer = 0;
    let mut served_n = 0usize;
    for round in 0..13u32 {
        ver += 1;
        e.begin_swap(ver, round % 4 == 3)?;
        let _ = e.query(round as u64)?;
        served_n += 1;
        e.finish_load()?;
        if round % 4 != 3 {
            e.finish_unload()?;
        }
        let _ = e.query(round as u64)?;
        served_n += 1;
    }
    e.audit(served_n)?;
    if e.fail_n != 3 {
        return Err(E_LEDGER_BAD);
    }
    Ok(())
}

/// R9 fuzz 混合流：随机查询/换装交错 60 步全绿。
pub fn r9_fuzz_mixed() -> Result<(), u16> {
    let mut g = Lcg::new(0x48525_0001);
    let mut e = HotEngine::new(1);
    let mut ver: LexVer = 1;
    let mut qn = 0usize;
    for _ in 0..60u64 {
        let op = g.below(3);
        if op == 0 && e.phase() == SwapPhase::Serving {
            ver += 1;
            let bad = g.below(4) == 0;
            e.begin_swap(ver, bad)?;
        } else if op == 1 && e.phase() == SwapPhase::Loading {
            e.finish_load()?;
            if e.phase() == SwapPhase::Unloading {
                e.finish_unload()?;
            }
        } else {
            let _ = e.query(g.next())?;
            qn += 1;
        }
    }
    e.audit(qn)
}

/// R10 确定性重放：同种子两轮版本轨迹与账目全等。
pub fn r10_deterministic() -> Result<(), u16> {
    let run = |seed: u64| -> Result<(LexVer, usize, u64), u16> {
        let mut g = Lcg::new(seed);
        let mut e = HotEngine::new(1);
        let mut ver: LexVer = 1;
        let mut qn = 0usize;
        for _ in 0..40u64 {
            let op = g.below(3);
            if op == 0 && e.phase() == SwapPhase::Serving {
                ver += 1;
                e.begin_swap(ver, g.below(5) == 0)?;
            } else if op == 1 && e.phase() == SwapPhase::Loading {
                e.finish_load()?;
                if e.phase() == SwapPhase::Unloading {
                    e.finish_unload()?;
                }
            } else {
                let _ = e.query(g.next())?;
                qn += 1;
            }
        }
        Ok((e.active_version(), qn, e.fail_n))
    };
    let a = run(0x48525_0002)?;
    let b = run(0x48525_0002)?;
    if a != b {
        return Err(E_NONDET);
    }
    Ok(())
}

/// 判据主入口：R1..R10，hits 计数。
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;
    r1_serving_steady()?;
    hits += 1;
    r2_loading_no_gap()?;
    hits += 1;
    r3_version_monotone()?;
    hits += 1;
    r4_bad_lib_fallback()?;
    hits += 1;
    r5_rollback_reject()?;
    hits += 1;
    r6_atomic_switch()?;
    hits += 1;
    r7_phase_guard()?;
    hits += 1;
    r8_ten_swaps()?;
    hits += 1;
    r9_fuzz_mixed()?;
    hits += 1;
    r10_deterministic()?;
    hits += 1;
    Ok(hits)
}
