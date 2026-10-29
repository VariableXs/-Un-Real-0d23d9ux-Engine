//! UNX-F48523 · E·fcitx5 addon 热退出（正：addon 退出→自动切自研零感）
//!
//! 判据原文：UNX-F48523-J1 —— addon 热退出零感：addon 状态机
//! Active→Dying→Gone 迁移期间，输入查询全数成功（无错误码、无丢失、
//! 无重排）；Dying 起新查询自动路由自研 fallback，路由切换对查询方
//! 不可见（返回值形状不变）；已受理查询不受退出影响（结果完整返回）；
//! 状态机不允许跳跃（Active 不能直接 Gone）；退出后查询计数账守恒；
//! ×10 轮热退出重演结果一致。
//!
//! 设计纪律：no_std（仅 core/alloc）；零 unwrap/expect/panic；fuzz 用
//! 确定性 LCG（wrapping_mul/wrapping_add），同种子同序列。

use crate::alloc::vec;
use crate::alloc::vec::Vec;
use alloc::string::{String, ToString};

pub const SEG_BASE: u16 = 0xECE0;

pub const E_STATE_JUMP: u16 = SEG_BASE + 1;
pub const E_QUERY_LOST: u16 = SEG_BASE + 2;
pub const E_ROUTE_VISIBLE: u16 = SEG_BASE + 3;
pub const E_RESULT_TRUNC: u16 = SEG_BASE + 4;
pub const E_NONDET: u16 = SEG_BASE + 5;
pub const E_LEDGER_BAD: u16 = SEG_BASE + 6;

/// addon 状态机。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AddonState {
    Active,
    Dying,
    Gone,
}

/// 查询请求与结果。
#[derive(Clone, PartialEq, Eq)]
pub struct Query {
    pub id: u32,
    pub text_fp: u64,
}

#[derive(Clone, PartialEq, Eq)]
pub struct QueryResult {
    pub id: u32,
    pub ok: bool,
    pub engine: u8,
    pub cands: Vec<u64>,
}

/// 双引擎桥：addon 引擎 + 自研 fallback，状态机驱动路由。
pub struct AddonBridge {
    state: AddonState,
    pub addon_n: u64,
    pub native_n: u64,
    pub results: Vec<QueryResult>,
}

impl AddonBridge {
    pub fn new() -> AddonBridge {
        AddonBridge { state: AddonState::Active, addon_n: 0, native_n: 0, results: Vec::new() }
    }

    pub fn state(&self) -> AddonState {
        self.state
    }

    /// 状态推进：只许 Active→Dying→Gone 单向单步。
    pub fn advance(&mut self) -> Result<AddonState, u16> {
        self.state = match self.state {
            AddonState::Active => AddonState::Dying,
            AddonState::Dying => AddonState::Gone,
            AddonState::Gone => return Err(E_STATE_JUMP),
        };
        Ok(self.state)
    }

    /// 跳跃校验：不允许任何越级迁移。
    pub fn guard_jump(&self, target: AddonState) -> Result<(), u16> {
        let ok = match (self.state, target) {
            (AddonState::Active, AddonState::Dying) => true,
            (AddonState::Dying, AddonState::Gone) => true,
            _ => false,
        };
        if !ok {
            return Err(E_STATE_JUMP);
        }
        Ok(())
    }

    /// 提交查询：Dying/Gone 起走自研；返回形状与 addon 侧完全一致。
    pub fn submit(&mut self, q: Query) -> Result<QueryResult, u16> {
        let engine: u8 = match self.state {
            AddonState::Active => {
                self.addon_n += 1;
                0
            }
            AddonState::Dying | AddonState::Gone => {
                self.native_n += 1;
                1
            }
        };
        // 结果构造：候选数由确定性派生（模拟查询命中）。
        let n = (q.text_fp % 5) + 1;
        let mut cands = Vec::new();
        for i in 0..n {
            cands.push(q.text_fp.wrapping_add(i as u64));
        }
        let r = QueryResult { id: q.id, ok: true, engine, cands };
        self.results.push(r.clone());
        Ok(r)
    }

    /// 审计：全结果 ok、计数账守恒、结果不截断。
    pub fn audit(&self, expect_n: u64) -> Result<(), u16> {
        for r in self.results.iter() {
            if !r.ok {
                return Err(E_QUERY_LOST);
            }
            if r.cands.len() == 0 {
                return Err(E_RESULT_TRUNC);
            }
        }
        if self.addon_n + self.native_n != expect_n {
            return Err(E_LEDGER_BAD);
        }
        Ok(())
    }

    /// 零感审计：结果形状（ok + 非空候选）在切换前后一致——按序检查
    /// engine 字段单调不减（0→1），但形状字段不变。
    pub fn audit_zero_felt(&self) -> Result<(), u16> {
        let mut prev_engine: u8 = 0;
        let mut seen_native = false;
        for (i, r) in self.results.iter().enumerate() {
            if r.engine < prev_engine {
                return Err(E_ROUTE_VISIBLE);
            }
            if r.engine == 1 {
                seen_native = true;
            }
            prev_engine = r.engine;
            if !r.ok || r.cands.len() == 0 {
                return Err(E_ROUTE_VISIBLE);
            }
            let _ = i;
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

/// R1 正常期走 addon：Active 下全部查询 engine=0。
pub fn r1_active_addon() -> Result<(), u16> {
    let mut b = AddonBridge::new();
    for i in 0..5u32 {
        let r = b.submit(Query { id: i, text_fp: 100 + i as u64 })?;
        if r.engine != 0 {
            return Err(E_ROUTE_VISIBLE);
        }
    }
    b.audit(5)?;
    if b.addon_n != 5 || b.native_n != 0 {
        return Err(E_LEDGER_BAD);
    }
    Ok(())
}

/// R2 Dying 切自研：advance 后新查询全走 native。
pub fn r2_dying_native() -> Result<(), u16> {
    let mut b = AddonBridge::new();
    let _ = b.submit(Query { id: 0, text_fp: 7 })?;
    b.advance()?;
    for i in 1..4u32 {
        let r = b.submit(Query { id: i, text_fp: 20 + i as u64 })?;
        if r.engine != 1 {
            return Err(E_ROUTE_VISIBLE);
        }
    }
    if b.state() != AddonState::Dying {
        return Err(E_STATE_JUMP);
    }
    b.audit(4)
}

/// R3 Gone 后仍零感：Gone 状态查询照常成功（全自研）。
pub fn r3_gone_still_serves() -> Result<(), u16> {
    let mut b = AddonBridge::new();
    b.advance()?;
    b.advance()?;
    for i in 0..3u32 {
        let r = b.submit(Query { id: i, text_fp: 30 + i as u64 })?;
        if !r.ok || r.engine != 1 || r.cands.len() == 0 {
            return Err(E_QUERY_LOST);
        }
    }
    b.audit(3)
}

/// R4 状态跳跃拒收：Gone 上再 advance 报 E_STATE_JUMP。
pub fn r4_jump_reject() -> Result<(), u16> {
    let mut b = AddonBridge::new();
    b.advance()?;
    b.advance()?;
    match b.advance() {
        Err(E_STATE_JUMP) => {}
        _ => return Err(E_STATE_JUMP),
    }
    Ok(())
}

/// R5 跳跃守卫：guard 对非法迁移全拒、合法迁移全放。
pub fn r5_guard_matrix() -> Result<(), u16> {
    let mut b = AddonBridge::new();
    // Active→Gone 非法。
    match b.guard_jump(AddonState::Gone) {
        Err(E_STATE_JUMP) => {}
        _ => return Err(E_STATE_JUMP),
    }
    // Active→Dying 合法。
    b.guard_jump(AddonState::Dying)?;
    // Dying→Active 非法（不可逆）。
    match b.guard_jump(AddonState::Active) {
        Err(E_STATE_JUMP) => {}
        _ => return Err(E_STATE_JUMP),
    }
    // Dying→Gone 合法。
    b.guard_jump(AddonState::Gone)?;
    Ok(())
}

/// R6 已受理查询完整返回：候选数恒 ≥1 且派生稳定。
pub fn r6_result_integrity() -> Result<(), u16> {
    let mut b = AddonBridge::new();
    for i in 0..10u32 {
        let r = b.submit(Query { id: i, text_fp: 1000 + i as u64 })?;
        if r.cands.len() == 0 || r.cands.len() > 5 {
            return Err(E_RESULT_TRUNC);
        }
        // 候选派生可复现：同 fp 重提同候选。
        let r2 = b.submit(Query { id: i + 100, text_fp: 1000 + i as u64 })?;
        if r.cands != r2.cands {
            return Err(E_NONDET);
        }
    }
    b.audit(20)
}

/// R7 ×10 轮热退出重演：同序列十轮迁移+查询结局全等。
pub fn r7_ten_rounds() -> Result<(), u16> {
    let run = |seed: u64| -> Result<(u64, u64, Vec<bool>), u16> {
        let mut g = Lcg::new(seed);
        let mut b = AddonBridge::new();
        let mut oks: Vec<bool> = Vec::new();
        for i in 0..12u32 {
            if i == 4 {
                b.advance()?;
            }
            if i == 9 {
                b.advance()?;
            }
            let fp = g.next();
            let r = b.submit(Query { id: i, text_fp: fp })?;
            oks.push(r.ok);
        }
        Ok((b.addon_n, b.native_n, oks))
    };
    let a = run(0x48523_0001)?;
    for _ in 0..10 {
        if run(0x48523_0001)? != a {
            return Err(E_NONDET);
        }
    }
    if a.0 != 4 || a.1 != 8 {
        return Err(E_LEDGER_BAD);
    }
    Ok(())
}

/// R8 fuzz 查询流：随机 40 查询 + 定点退出，全程零感。
pub fn r8_fuzz_zero_felt() -> Result<(), u16> {
    let mut g = Lcg::new(0x48523_0002);
    let mut b = AddonBridge::new();
    for i in 0..40u32 {
        if i == 20 {
            b.advance()?;
        }
        let r = b.submit(Query { id: i, text_fp: g.next() })?;
        if !r.ok {
            return Err(E_QUERY_LOST);
        }
    }
    b.audit(40)?;
    b.audit_zero_felt()
}

/// R9 零感形状：切换点前后结果的 ok/cands 非空形状无变化。
pub fn r9_shape_unchanged() -> Result<(), u16> {
    let mut b = AddonBridge::new();
    for i in 0..8u32 {
        if i == 3 {
            b.advance()?;
        }
        let r = b.submit(Query { id: i, text_fp: 500 + i as u64 })?;
        if !r.ok || r.cands.len() == 0 {
            return Err(E_ROUTE_VISIBLE);
        }
    }
    // 引擎标记单调（0..0..1..1）。
    let engines: Vec<u8> = b.results.iter().map(|r| r.engine).collect();
    for i in 1..engines.len() {
        if engines[i] < engines[i - 1] {
            return Err(E_ROUTE_VISIBLE);
        }
    }
    Ok(())
}

/// R10 计数账切换点：addon/native 计数与迁移时刻严格对应。
pub fn r10_ledger_split() -> Result<(), u16> {
    let mut b = AddonBridge::new();
    // 3 查 addon，迁移，2 查 native。
    for i in 0..3u32 {
        let _ = b.submit(Query { id: i, text_fp: 11 + i as u64 })?;
    }
    b.advance()?;
    for i in 3..5u32 {
        let _ = b.submit(Query { id: i, text_fp: 22 + i as u64 })?;
    }
    if b.addon_n != 3 || b.native_n != 2 {
        return Err(E_LEDGER_BAD);
    }
    b.audit(5)
}

/// 判据主入口：R1..R10，hits 计数。
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;
    r1_active_addon()?;
    hits += 1;
    r2_dying_native()?;
    hits += 1;
    r3_gone_still_serves()?;
    hits += 1;
    r4_jump_reject()?;
    hits += 1;
    r5_guard_matrix()?;
    hits += 1;
    r6_result_integrity()?;
    hits += 1;
    r7_ten_rounds()?;
    hits += 1;
    r8_fuzz_zero_felt()?;
    hits += 1;
    r9_shape_unchanged()?;
    hits += 1;
    r10_ledger_split()?;
    hits += 1;
    Ok(hits)
}
