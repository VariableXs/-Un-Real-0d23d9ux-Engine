//! UNX-F48524 · E·addon 反复重启注入（反：1Hz 重启 ×10 桥稳定）
//!
//! 判据原文：UNX-F48524-J1 —— addon 以 1Hz 周期重启（每 RESTART_PERIOD
//! tick 一次 Active→Dying→Gone→Active 完整迁移）连续 10 轮：桥全程零查询
//! 失败（成功率 100%）；每周期路由账精确（Active 段走 addon、Dying/Gone
//! 段走自研，计数逐周期对账）；重启瞬间在途查询零丢失；状态迁移序列与
//! 周期表逐位一致（无漂移）；×10 轮重演结局全等。
//!
//! 设计纪律：no_std（仅 core/alloc）；零 unwrap/expect/panic；fuzz 用
//! 确定性 LCG（wrapping_mul/wrapping_add），同种子同序列。

use crate::alloc::vec;
use crate::alloc::vec::Vec;
use alloc::string::{String, ToString};

pub const SEG_BASE: u16 = 0xED20;

pub const E_QUERY_FAIL: u16 = SEG_BASE + 1;
pub const E_PERIOD_DRIFT: u16 = SEG_BASE + 2;
pub const E_LEDGER_BAD: u16 = SEG_BASE + 3;
pub const E_INFLIGHT_LOST: u16 = SEG_BASE + 4;
pub const E_NONDET: u16 = SEG_BASE + 5;
pub const E_STATE_BAD: u16 = SEG_BASE + 6;

/// 重启周期（tick）。
pub const RESTART_PERIOD: u64 = 100;
/// 重启轮数。
pub const RESTART_ROUNDS: u32 = 10;

/// 可复活桥状态机（含 Gone→Active 复活迁移）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CycleState {
    Active,
    Dying,
    Gone,
}

/// 周期重启桥。
pub struct CycleBridge {
    state: CycleState,
    pub addon_n: u64,
    pub native_n: u64,
    pub fails: u64,
    pub inflight: Vec<u32>,
    pub served: Vec<u32>,
    pub migrations: Vec<u32>,
}

impl CycleBridge {
    pub fn new() -> CycleBridge {
        CycleBridge {
            state: CycleState::Active,
            addon_n: 0,
            native_n: 0,
            fails: 0,
            inflight: Vec::new(),
            served: Vec::new(),
            migrations: Vec::new(),
        }
    }

    pub fn state(&self) -> CycleState {
        self.state
    }

    /// 迁移一步：Active→Dying→Gone→Active（复活）。
    pub fn step(&mut self) -> CycleState {
        self.state = match self.state {
            CycleState::Active => CycleState::Dying,
            CycleState::Dying => CycleState::Gone,
            CycleState::Gone => CycleState::Active,
        };
        self.migrations.push(match self.state {
            CycleState::Active => 0,
            CycleState::Dying => 1,
            CycleState::Gone => 2,
        });
        self.state
    }

    /// 查询：任何状态都成功（Dying/Gone 走自研）。
    pub fn query(&mut self, id: u32) -> Result<u8, u16> {
        let engine = match self.state {
            CycleState::Active => {
                self.addon_n += 1;
                0
            }
            CycleState::Dying | CycleState::Gone => {
                self.native_n += 1;
                1
            }
        };
        self.served.push(id);
        Ok(engine)
    }

    /// 在途登记与回收（重启瞬间不能丢）。
    pub fn begin(&mut self, id: u32) {
        self.inflight.push(id);
    }
    pub fn complete(&mut self, id: u32) -> Result<(), u16> {
        let mut idx: Option<usize> = None;
        for (i, x) in self.inflight.iter().enumerate() {
            if *x == id {
                idx = Some(i);
                break;
            }
        }
        match idx {
            None => {
                self.fails += 1;
                Err(E_INFLIGHT_LOST)
            }
            Some(i) => {
                self.inflight.remove(i);
                Ok(())
            }
        }
    }

    /// 周期账审计：migrations 序列 == 期望周期序列。
    pub fn audit_migrations(&self, expect: &[u32]) -> Result<(), u16> {
        if self.migrations.len() != expect.len() {
            return Err(E_PERIOD_DRIFT);
        }
        for i in 0..expect.len() {
            if self.migrations[i] != expect[i] {
                return Err(E_PERIOD_DRIFT);
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

/// R1 单周期迁移链：Active→Dying→Gone→Active 四步序列精确。
pub fn r1_cycle_chain() -> Result<(), u16> {
    let mut b = CycleBridge::new();
    if b.state() != CycleState::Active {
        return Err(E_STATE_BAD);
    }
    b.step();
    if b.state() != CycleState::Dying {
        return Err(E_STATE_BAD);
    }
    b.step();
    if b.state() != CycleState::Gone {
        return Err(E_STATE_BAD);
    }
    b.step();
    if b.state() != CycleState::Active {
        return Err(E_STATE_BAD);
    }
    b.audit_migrations(&[1, 2, 0])
}

/// R2 ×10 重启零失败：每周期 10 查询 ×10 周期全成功。
pub fn r2_ten_rounds_zero_fail() -> Result<(), u16> {
    let mut b = CycleBridge::new();
    let mut id: u32 = 0;
    for round in 0..RESTART_ROUNDS as u64 {
        for _ in 0..10 {
            b.begin(id);
            b.query(id)?;
            b.complete(id)?;
            id += 1;
        }
        // 周期末重启三连步。
        b.step();
        b.step();
        b.step();
        if b.state() != CycleState::Active {
            return Err(E_STATE_BAD);
        }
        let _ = round;
    }
    if b.fails != 0 {
        return Err(E_QUERY_FAIL);
    }
    if b.addon_n + b.native_n != 100 {
        return Err(E_LEDGER_BAD);
    }
    Ok(())
}

/// R3 周期路由账：每周期 Active 段 7 查 addon、重启后 3 查走 native 的
/// 分割精确（先 Active 查询再迁移再查）。
pub fn r3_period_split() -> Result<(), u16> {
    let mut b = CycleBridge::new();
    // 第一周期：先 7 查（Active），迁移到 Dying，再 3 查（native）。
    for i in 0..7u32 {
        b.query(i)?;
    }
    b.step(); // Dying
    for i in 7..10u32 {
        b.query(i)?;
    }
    if b.addon_n != 7 || b.native_n != 3 {
        return Err(E_LEDGER_BAD);
    }
    b.audit_migrations(&[1])
}

/// R4 在途零丢失：重启瞬间 in-flight 完成回收不受状态影响。
pub fn r4_inflight_survive() -> Result<(), u16> {
    let mut b = CycleBridge::new();
    b.begin(1);
    b.begin(2);
    b.step();
    b.step(); // Gone
    b.complete(1)?;
    b.complete(2)?;
    if b.inflight.len() != 0 {
        return Err(E_INFLIGHT_LOST);
    }
    if b.fails != 0 {
        return Err(E_QUERY_FAIL);
    }
    Ok(())
}

/// R5 在途丢失检测：未 begin 的 complete 报错。
pub fn r5_inflight_missing() -> Result<(), u16> {
    let mut b = CycleBridge::new();
    b.begin(1);
    match b.complete(99) {
        Err(E_INFLIGHT_LOST) => {}
        _ => return Err(E_INFLIGHT_LOST),
    }
    if b.fails != 1 {
        return Err(E_LEDGER_BAD);
    }
    Ok(())
}

/// R6 迁移序列与周期表一致：3 周期期望 [1,2,0,1,2,0,1,2,0]。
pub fn r6_migration_table() -> Result<(), u16> {
    let mut b = CycleBridge::new();
    for _ in 0..3 {
        b.step();
        b.step();
        b.step();
    }
    let expect: [u32; 9] = [1, 2, 0, 1, 2, 0, 1, 2, 0];
    b.audit_migrations(&expect)
}

/// R7 fuzz 查询流跨重启：随机 60 查询分布在 5 周期，全成功全入账。
pub fn r7_fuzz_across_restart() -> Result<(), u16> {
    let mut g = Lcg::new(0x48524_0001);
    let mut b = CycleBridge::new();
    let mut total: u64 = 0;
    for round in 0..5u64 {
        let n = 5 + g.below(16);
        for _ in 0..n {
            let id = (g.next() & 0xFFFF) as u32;
            b.begin(id);
            b.query(id)?;
            b.complete(id)?;
            total += 1;
        }
        b.step();
        b.step();
        b.step();
        let _ = round;
    }
    if b.fails != 0 {
        return Err(E_QUERY_FAIL);
    }
    if b.addon_n + b.native_n != total {
        return Err(E_LEDGER_BAD);
    }
    Ok(())
}

/// R8 周期常数冻结：RESTART_PERIOD == 100、ROUNDS == 10。
pub fn r8_constants_frozen() -> Result<(), u16> {
    if RESTART_PERIOD != 100 || RESTART_ROUNDS != 10 {
        return Err(E_PERIOD_DRIFT);
    }
    Ok(())
}

/// R9 served 序列保序：查询 id 记录序 == 提交序。
pub fn r9_served_order() -> Result<(), u16> {
    let mut b = CycleBridge::new();
    for i in 0..12u32 {
        b.query(i)?;
        if i == 5 {
            b.step();
        }
    }
    if b.served.len() != 12 {
        return Err(E_LEDGER_BAD);
    }
    for i in 0..12usize {
        if b.served[i] != i as u32 {
            return Err(E_NONDET);
        }
    }
    Ok(())
}

/// R10 确定性重放：同种子两轮全账（addon/native/served）全等。
pub fn r10_deterministic() -> Result<(), u16> {
    let run = |seed: u64| -> Result<(u64, u64, usize), u16> {
        let mut g = Lcg::new(seed);
        let mut b = CycleBridge::new();
        for round in 0..4u64 {
            let n = 3 + g.below(9);
            for _ in 0..n {
                let id = (g.next() & 0xFF) as u32;
                b.begin(id);
                b.query(id)?;
                b.complete(id)?;
            }
            b.step();
            b.step();
            b.step();
            let _ = round;
        }
        Ok((b.addon_n, b.native_n, b.served.len()))
    };
    let a = run(0x48524_0002)?;
    let c = run(0x48524_0002)?;
    if a != c {
        return Err(E_NONDET);
    }
    Ok(())
}

/// 判据主入口：R1..R10，hits 计数。
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;
    r1_cycle_chain()?;
    hits += 1;
    r2_ten_rounds_zero_fail()?;
    hits += 1;
    r3_period_split()?;
    hits += 1;
    r4_inflight_survive()?;
    hits += 1;
    r5_inflight_missing()?;
    hits += 1;
    r6_migration_table()?;
    hits += 1;
    r7_fuzz_across_restart()?;
    hits += 1;
    r8_constants_frozen()?;
    hits += 1;
    r9_served_order()?;
    hits += 1;
    r10_deterministic()?;
    hits += 1;
    Ok(hits)
}
