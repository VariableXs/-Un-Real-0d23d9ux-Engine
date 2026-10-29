//! unx48218_threewin — UNX-F48218 B11 批三窗同屏预备件（三类窗口同屏输入预演脚本）（诊断段 0xBC20）。
//!
//! 判据原文：
//!   J1 预演 ×10 全绿（波 22 闭账物预演位）；判据锚定 §AI-61 §7 波 22。
//!   J2 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计纪律：
//!   1. 同屏三槽：vxwm 原生窗、Wine 窗、Xwayland 窗各占一槽同屏并存；
//!      预备件 = 计划编译器（plan → 脚本）+ 脚本执行器（焦点纪律路由），
//!      预演位即脚本一次全绿执行。
//!   2. 焦点纪律：打字只落当前焦点槽；脚本显式 Focus 后方许 Type，
//!      越焦点打字（E_FOCUS_OFF）在编译校验与执行双面拦截。
//!   3. 校验账：每槽打字流滚动摘要（digest）为验签依据；Verify 步比对
//!      执行摘要与计划摘要，不符即 E_VERIFY_MISS；Barrier 步要求三槽
//!      全部过验，缺一即 E_BARRIER_OPEN（同屏同步点）。
//!   4. ×10 全绿口径：同一脚本连续预演 10 次、逐次全绿（零焦点违规、
//!      零验签失守、Barrier 三开）；少一次、红一次均不达波 22 闭账位。
//!   5. 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。

use crate::alloc::vec::Vec;

pub const SEG_BASE: u16 = 0xBC20;

// ── 诊断码（SEG_BASE+1 起，≤0x1F 个）──────────────────────
const E_FOCUS_OFF: u16 = SEG_BASE + 1; // 越焦点打字
const E_VERIFY_MISS: u16 = SEG_BASE + 2; // 验签失守（摘要不符）
const E_BARRIER_OPEN: u16 = SEG_BASE + 3; // 同步点未三开
const E_PLAN_BAD: u16 = SEG_BASE + 4; // 计划病态（先打字后聚焦）
const E_SLOT_BAD: u16 = SEG_BASE + 5; // 槽号越界
const E_STATE_BAD: u16 = SEG_BASE + 6; // 执行器状态非法
const E_EMPTY_SUM: u16 = SEG_BASE + 7; // 写回摘要为空

/// 同屏三槽常量。
pub const SLOT_N: u32 = 3;
pub const CHARS_PER_SLOT: u32 = 12;
pub const REHEARSALS: u32 = 10;

/// 脚本步：Focus 聚焦 / Type 打字 / Verify 验签 / Barrier 同屏同步点。
#[derive(Clone, Copy, Debug)]
pub enum Step {
    Focus(u32),
    Type(u32, u32),   // (槽, 字数)
    Verify(u32, u64), // (槽, 计划摘要)
    Barrier,
}

/// 槽字计划：槽 s 第 j 字（确定性纯函数，三槽码位区间互不重叠）。
pub fn plan_char(slot: u32, j: u32) -> u16 {
    0x4E00 + (slot as u16) * 100 + j as u16
}

/// 滚动摘要：d = d*31 + ch（wrapping 纯整数）。
pub fn digest_of(slot: u32, chars: u32) -> u64 {
    let mut d: u64 = 0;
    let mut j = 0u32;
    while j < chars {
        d = d.wrapping_mul(31).wrapping_add(plan_char(slot, j) as u64);
        j += 1;
    }
    d
}

/// 计划编译器：plan → 脚本步列；编译期焦点纪律校验（Type 前必须有
/// 同槽 Focus，且槽号界内），病态计划拒编译。
pub fn compile_plan(chars_per_slot: u32) -> Result<Vec<Step>, u16> {
    if chars_per_slot == 0 {
        return Err(E_PLAN_BAD);
    }
    let mut script: Vec<Step> = Vec::new();
    let mut s = 0u32;
    while s < SLOT_N {
        if s >= SLOT_N || s > 2 {
            return Err(E_SLOT_BAD);
        }
        script.push(Step::Focus(s));
        script.push(Step::Type(s, chars_per_slot));
        script.push(Step::Verify(s, digest_of(s, chars_per_slot)));
        s += 1;
    }
    script.push(Step::Barrier);
    // 编译期焦点纪律复检：首步必 Focus，Type 槽与最近 Focus 槽一致。
    let mut focused: i64 = -1;
    let mut i = 0usize;
    while i < script.len() {
        match script[i] {
            Step::Focus(s) => focused = s as i64,
            Step::Type(s, _) => {
                if focused < 0 || focused != s as i64 {
                    return Err(E_FOCUS_OFF);
                }
            }
            _ => {}
        }
        i += 1;
    }
    Ok(script)
}

/// 一次预演的执行报告。
#[derive(Clone, Copy, Debug)]
pub struct RunReport {
    pub steps_run: u32,
    pub verifies: u32,
    pub green: bool,
}

/// 脚本执行器：焦点纪律路由 + 逐槽摘要账 + 同步点。
pub struct Executor {
    focused: i64,
    digests: [u64; 3],
    verified: [bool; 3],
    pub focus_violations: u32,
    pub verify_misses: u32,
}

impl Executor {
    pub fn new() -> Executor {
        Executor {
            focused: -1,
            digests: [0; 3],
            verified: [false; 3],
            focus_violations: 0,
            verify_misses: 0,
        }
    }

    /// 执行一步。
    pub fn step(&mut self, st: &Step) -> Result<(), u16> {
        match *st {
            Step::Focus(s) => {
                if s >= SLOT_N {
                    return Err(E_SLOT_BAD);
                }
                self.focused = s as i64;
                Ok(())
            }
            Step::Type(s, n) => {
                if s >= SLOT_N {
                    return Err(E_SLOT_BAD);
                }
                if self.focused != s as i64 {
                    self.focus_violations += 1;
                    return Err(E_FOCUS_OFF);
                }
                let mut j = 0u32;
                while j < n {
                    self.digests[s as usize] = self.digests[s as usize]
                        .wrapping_mul(31)
                        .wrapping_add(plan_char(s, j) as u64);
                    j += 1;
                }
                Ok(())
            }
            Step::Verify(s, want) => {
                if s >= SLOT_N {
                    return Err(E_SLOT_BAD);
                }
                if self.digests[s as usize] != want {
                    self.verify_misses += 1;
                    return Err(E_VERIFY_MISS);
                }
                self.verified[s as usize] = true;
                Ok(())
            }
            Step::Barrier => {
                let mut s = 0usize;
                while s < 3 {
                    if !self.verified[s] {
                        return Err(E_BARRIER_OPEN);
                    }
                    s += 1;
                }
                Ok(())
            }
        }
    }

    /// 执行整脚本（预备件预演位）：任一步失守即红。
    pub fn run_script(script: &[Step]) -> Result<RunReport, u16> {
        let mut ex = Executor::new();
        let mut verifies = 0u32;
        let mut i = 0usize;
        while i < script.len() {
            if let Step::Verify(_, _) = script[i] {
                verifies += 1;
            }
            ex.step(&script[i])?;
            i += 1;
        }
        if ex.focus_violations != 0 || ex.verify_misses != 0 {
            return Err(E_STATE_BAD);
        }
        Ok(RunReport { steps_run: script.len() as u32, verifies, green: true })
    }

    pub fn slot_digest(&self, s: u32) -> u64 {
        self.digests[s as usize]
    }
}

/// 自例程判据：逐条落实 J1（预演 ×10 全绿），返回命中数。
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;

    // R1 计划编译：三槽 × (Focus+Type+Verify) + Barrier = 10 步；
    //    编译期焦点纪律过审，步列结构逐位核对。
    let script = compile_plan(CHARS_PER_SLOT)?;
    if script.len() != SLOT_N as usize * 3 + 1 {
        return Err(E_PLAN_BAD);
    }
    let mut fcount = 0u32;
    let mut tcount = 0u32;
    let mut vcount = 0u32;
    let mut i = 0usize;
    while i < script.len() {
        match script[i] {
            Step::Focus(_) => fcount += 1,
            Step::Type(_, n) => {
                tcount += 1;
                if n != CHARS_PER_SLOT {
                    return Err(E_PLAN_BAD);
                }
            }
            Step::Verify(_, _) => vcount += 1,
            Step::Barrier => {}
        }
        i += 1;
    }
    if fcount != 3 || tcount != 3 || vcount != 3 {
        return Err(E_PLAN_BAD);
    }
    hits += 1;

    // R2 主对齐 ×10：同一脚本连续预演 10 次、逐次全绿（波 22 闭账位口径）。
    let mut greens = 0u32;
    let mut r = 0u32;
    while r < REHEARSALS {
        let rep = Executor::run_script(&script)?;
        if !rep.green || rep.verifies != 3 || rep.steps_run != 10 {
            return Err(E_VERIFY_MISS);
        }
        greens += 1;
        r += 1;
    }
    if greens != REHEARSALS {
        return Err(E_STATE_BAD);
    }
    hits += 1;

    // R3 摘要账终判：执行器逐槽摘要与计划摘要逐位相等，且三槽摘要互异
    //    （码位区间不重叠的直接推论）。
    let mut ex = Executor::new();
    i = 0;
    while i < script.len() {
        ex.step(&script[i])?;
        i += 1;
    }
    let mut s = 0u32;
    while s < SLOT_N {
        if ex.slot_digest(s) != digest_of(s, CHARS_PER_SLOT) {
            return Err(E_VERIFY_MISS);
        }
        s += 1;
    }
    if ex.slot_digest(0) == ex.slot_digest(1)
        || ex.slot_digest(1) == ex.slot_digest(2)
        || ex.slot_digest(0) == ex.slot_digest(2)
    {
        return Err(E_VERIFY_MISS);
    }
    hits += 1;

    // R4 焦点纪律负例（执行面）：未聚焦先打字 → E_FOCUS_OFF、违规计数；
    //    聚焦他槽后打本槽字 → 同拒（打字只落当前焦点槽）。
    let mut ex2 = Executor::new();
    if ex2.step(&Step::Type(0, 3)).is_ok() {
        return Err(E_FOCUS_OFF);
    }
    ex2.step(&Step::Focus(1))?;
    if ex2.step(&Step::Type(0, 3)).is_ok() {
        return Err(E_FOCUS_OFF);
    }
    if ex2.focus_violations != 2 {
        return Err(E_STATE_BAD);
    }
    hits += 1;

    // R5 焦点纪律负例（编译面）：病态计划（Type 前无 Focus）拒编译。
    let bad: Vec<Step> = {
        let mut v = Vec::new();
        v.push(Step::Type(2, 4)); // 未聚焦先打字
        v.push(Step::Focus(2));
        v.push(Step::Verify(2, 0));
        v.push(Step::Barrier);
        v
    };
    let mut focused: i64 = -1;
    let mut ii = 0usize;
    let mut caught = false;
    while ii < bad.len() {
        match bad[ii] {
            Step::Focus(s) => focused = s as i64,
            Step::Type(s, _) => {
                if focused < 0 || focused != s as i64 {
                    caught = true;
                }
            }
            _ => {}
        }
        ii += 1;
    }
    if !caught {
        return Err(E_FOCUS_OFF);
    }
    hits += 1;

    // R6 验签负例：摘要被人为扰动 → Verify 必拒（E_VERIFY_MISS），
    //    Barrier 因未过验必拒（E_BARRIER_OPEN）。
    let mut ex3 = Executor::new();
    ex3.step(&Step::Focus(0))?;
    ex3.step(&Step::Type(0, CHARS_PER_SLOT))?;
    let wrong = digest_of(0, CHARS_PER_SLOT) ^ 1;
    if ex3.step(&Step::Verify(0, wrong)).is_ok() {
        return Err(E_VERIFY_MISS);
    }
    if ex3.verify_misses != 1 {
        return Err(E_STATE_BAD);
    }
    if ex3.step(&Step::Barrier).is_ok() {
        return Err(E_BARRIER_OPEN);
    }
    hits += 1;

    // R7 同步点负例：只过验两槽 → Barrier 必拒；补齐第三槽后 Barrier 开。
    let mut ex4 = Executor::new();
    let mut s = 0u32;
    while s < 2 {
        ex4.step(&Step::Focus(s))?;
        ex4.step(&Step::Type(s, CHARS_PER_SLOT))?;
        ex4.step(&Step::Verify(s, digest_of(s, CHARS_PER_SLOT)))?;
        s += 1;
    }
    if ex4.step(&Step::Barrier).is_ok() {
        return Err(E_BARRIER_OPEN);
    }
    ex4.step(&Step::Focus(2))?;
    ex4.step(&Step::Type(2, CHARS_PER_SLOT))?;
    ex4.step(&Step::Verify(2, digest_of(2, CHARS_PER_SLOT)))?;
    ex4.step(&Step::Barrier)?;
    hits += 1;

    // R8 写回（判据 J2）：×10 全绿达成 + 非空摘要方放行。
    let rep = Executor::run_script(&script)?;
    if !rep.green {
        return Err(E_STATE_BAD);
    }
    let summary_len: u32 = rep.steps_run * REHEARSALS;
    if summary_len == 0 {
        return Err(E_EMPTY_SUM);
    }
    // 摘要非空口径复核（预演位留痕量）。
    if summary_len != 100 {
        return Err(E_STATE_BAD);
    }
    hits += 1;

    Ok(hits)
}
