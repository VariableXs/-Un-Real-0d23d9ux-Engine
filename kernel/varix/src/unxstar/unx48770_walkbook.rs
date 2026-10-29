//! UNX-F48770 C·真实用户视角走查账（核心旅程：唤起→输入→候选→上屏→纠错→确认）
//! 判据：UNX-F48770-J1 核心旅程走查全绿；判据锚定 体验十。
//! 纪律：六步旅程状态机走查，步骤序守恒、跳步必抓、回退合法、终态确认闭环。

use crate::alloc::vec::Vec;
use crate::alloc::vec;
use alloc::string::{String, ToString};

pub const SEG_BASE: u16 = 0x1F20;

const E_STEP_BAD: u16 = SEG_BASE + 1; // 步骤非法
const E_STEP_GAP: u16 = SEG_BASE + 2; // 跳步
const E_STEP_DUP: u16 = SEG_BASE + 3; // 重复步骤
const E_STATE_DIFF: u16 = SEG_BASE + 4; // 终态不一致
const E_TRIP_EMPTY: u16 = SEG_BASE + 5; // 空旅程拒
const E_ROLLBACK_BAD: u16 = SEG_BASE + 6; // 非法回退
const E_BAL_BREAK: u16 = SEG_BASE + 7; // 走查账守恒破
const E_CONFIRM_MISS: u16 = SEG_BASE + 8; // 未确认终态

/// 旅程步骤。
pub const S_WAKE: u8 = 1; // 唤起
pub const S_INPUT: u8 = 2; // 输入
pub const S_CAND: u8 = 3; // 候选
pub const S_COMMIT: u8 = 4; // 上屏
pub const S_FIX: u8 = 5; // 纠错
pub const S_CONFIRM: u8 = 6; // 确认

/// 走查记录。
#[derive(Clone)]
pub struct WalkStep {
    /// 步骤。
    pub step: u8,
    /// 步内耗时（毫秒）。
    pub cost_ms: u32,
}

/// 走查账。
pub struct WalkBook {
    steps: Vec<WalkStep>,
    confirmed: bool,
}

impl WalkBook {
    pub fn new() -> WalkBook {
        WalkBook {
            steps: Vec::new(),
            confirmed: false,
        }
    }

    /// 步进（主链顺序推进、纠错可回退一格重走）。
    pub fn step(&mut self, s: u8, cost_ms: u32) -> Result<bool, u16> {
        if self.confirmed {
            return Err(E_CONFIRM_MISS);
        }
        if s == 0 || s > S_CONFIRM {
            return Err(E_STEP_BAD);
        }
        if cost_ms == 0 {
            return Err(E_BAL_BREAK);
        }
        if let Some(last) = self.steps.last() {
            if s == last.step {
                return Err(E_STEP_DUP);
            }
            if s == last.step - 1 {
                // 合法回退（纠错重走上一格）
            } else if s != last.step + 1 {
                return Err(E_STEP_GAP);
            }
        } else if s != S_WAKE {
            return Err(E_STEP_GAP);
        }
        self.steps.push(WalkStep { step: s, cost_ms });
        Ok(true)
    }

    /// 终态确认：必须走到 S_CONFIRM。
    pub fn confirm(&mut self) -> Result<u32, u16> {
        if self.steps.is_empty() {
            return Err(E_TRIP_EMPTY);
        }
        let last = self.steps[self.steps.len() - 1].step;
        if last != S_CONFIRM {
            return Err(E_CONFIRM_MISS);
        }
        self.confirmed = true;
        Ok(self.steps.len() as u32)
    }

    /// 步序守恒：主链 1..=6 至少各出现一次且首步为唤起。
    pub fn assert_chain(&self) -> Result<(), u16> {
        if self.steps.is_empty() {
            return Err(E_TRIP_EMPTY);
        }
        if self.steps[0].step != S_WAKE {
            return Err(E_STEP_GAP);
        }
        let mut s = S_WAKE;
        while s <= S_CONFIRM {
            let mut hit = false;
            for st in self.steps.iter() {
                if st.step == s {
                    hit = true;
                    break;
                }
            }
            if !hit {
                return Err(E_STEP_GAP);
            }
            s += 1;
        }
        Ok(())
    }

    /// 总耗时。
    pub fn total_cost(&self) -> u64 {
        let mut t = 0u64;
        for st in self.steps.iter() {
            t += st.cost_ms as u64;
        }
        t
    }

    /// 走查账守恒：总耗时 = 各步和。
    pub fn assert_balance(&self, want: u64) -> Result<(), u16> {
        if self.total_cost() != want {
            return Err(E_BAL_BREAK);
        }
        Ok(())
    }
}

/// 确定性 LCG。
pub struct Lcg(u64);

impl Lcg {
    pub fn new(seed: u64) -> Lcg {
        Lcg(seed)
    }
    pub fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        self.0
    }
    pub fn below(&mut self, n: u64) -> u64 {
        (self.next() >> 16) % n.max(1)
    }
}

/// 核心旅程走查主判据：六步走查-回退-确认-守恒全绿。
pub fn drill() -> Result<u32, u16> {
    let mut hits = 0u32;

    // R1 标准旅程：1→2→3→4→5→6
    let mut wb = WalkBook::new();
    wb.step(S_WAKE, 30)?;
    wb.step(S_INPUT, 120)?;
    wb.step(S_CAND, 45)?;
    wb.step(S_COMMIT, 20)?;
    wb.step(S_FIX, 90)?;
    wb.step(S_CONFIRM, 15)?;
    wb.assert_chain()?;
    hits += 1;

    // R2 步骤非法必拒
    if wb.step(9, 10).err() == Some(E_STEP_BAD) {
        hits += 1;
    } else {
        return Err(E_STEP_BAD);
    }

    // R3 重复步骤必拒
    if wb.step(S_CONFIRM, 10).err() == Some(E_STEP_DUP) {
        hits += 1;
    } else {
        return Err(E_STEP_DUP);
    }

    // R4 跳步必拒：从确认跳回候选
    if wb.step(S_CAND, 10).err() == Some(E_STEP_DUP) {
        hits += 1;
    } else {
        return Err(E_STEP_DUP);
    }

    // R5 零耗时必拒
    if wb.step(S_CONFIRM, 0).err() == Some(E_BAL_BREAK) {
        hits += 1;
    } else {
        return Err(E_BAL_BREAK);
    }

    // R6 走查账守恒
    let want = 30 + 120 + 45 + 20 + 90 + 15;
    wb.assert_balance(want)?;
    hits += 1;

    // R7 终态确认
    let n = wb.confirm()?;
    if n != 6 {
        return Err(E_CONFIRM_MISS);
    }
    hits += 1;

    // R8 确认后再走必拒
    if wb.step(S_CONFIRM, 5).err() == Some(E_CONFIRM_MISS) {
        hits += 1;
    } else {
        return Err(E_CONFIRM_MISS);
    }

    // R9 未到确认先确认必抓
    let mut wb2 = WalkBook::new();
    wb2.step(S_WAKE, 10)?;
    wb2.step(S_INPUT, 20)?;
    if wb2.confirm().err() == Some(E_CONFIRM_MISS) {
        hits += 1;
    } else {
        return Err(E_CONFIRM_MISS);
    }

    // R10 首步非唤起必拒
    let mut wb3 = WalkBook::new();
    if wb3.step(S_INPUT, 10).err() == Some(E_STEP_GAP) {
        hits += 1;
    } else {
        return Err(E_STEP_GAP);
    }

    // R11 LCG 旅程 ×64：回退重走合法路径全确认
    let mut rng = Lcg::new(0x48770);
    let mut k = 0u32;
    while k < 64 {
        let mut w = WalkBook::new();
        w.step(S_WAKE, (rng.below(50) + 10) as u32)?;
        w.step(S_INPUT, (rng.below(200) + 30) as u32)?;
        w.step(S_CAND, (rng.below(60) + 10) as u32)?;
        w.step(S_COMMIT, (rng.below(30) + 5) as u32)?;
        w.step(S_FIX, (rng.below(150) + 20) as u32)?;
        // 纠错回退重走：FIX→COMMIT→FIX
        w.step(S_COMMIT, (rng.below(30) + 5) as u32)?;
        w.step(S_FIX, (rng.below(150) + 20) as u32)?;
        w.step(S_CONFIRM, (rng.below(20) + 5) as u32)?;
        w.assert_chain()?;
        w.confirm()?;
        k += 1;
    }
    hits += 1;

    // R12 空旅程确认必拒
    let wb4 = WalkBook::new();
    if wb4.assert_chain().err() == Some(E_TRIP_EMPTY) {
        hits += 1;
    } else {
        return Err(E_TRIP_EMPTY);
    }

    // R13 多轮回退旅程：三连回退重走全部合法
    let mut wb5 = WalkBook::new();
    wb5.step(S_WAKE, 20)?;
    wb5.step(S_INPUT, 100)?;
    wb5.step(S_CAND, 40)?;
    wb5.step(S_COMMIT, 15)?;
    wb5.step(S_FIX, 60)?;
    wb5.step(S_COMMIT, 15)?;
    wb5.step(S_FIX, 60)?;
    wb5.step(S_COMMIT, 15)?;
    wb5.step(S_FIX, 60)?;
    wb5.step(S_CONFIRM, 10)?;
    wb5.assert_chain()?;
    let c5 = wb5.total_cost();
    wb5.assert_balance(c5)?;
    wb5.confirm()?;
    hits += 1;

    // R14 LCG 双跑对拍：同种子旅程耗时逐项全等
    let seed14 = 0x48770_0014;
    let mut t1: Vec<u64> = Vec::new();
    let mut t2: Vec<u64> = Vec::new();
    let mut r1 = Lcg(seed14);
    let mut r2 = Lcg(seed14);
    let mut k1 = 0u32;
    while k1 < 32 {
        let mut w1 = WalkBook::new();
        let mut w2 = WalkBook::new();
        let mut s = S_WAKE;
        while s <= S_CONFIRM {
            let c1 = (r1.below(100) + 5) as u32;
            let c2 = (r2.below(100) + 5) as u32;
            w1.step(s, c1)?;
            w2.step(s, c2)?;
            s += 1;
        }
        t1.push(w1.total_cost());
        t2.push(w2.total_cost());
        k1 += 1;
    }
    if t1.len() != t2.len() {
        return Err(E_STATE_DIFF);
    }
    let mut i14 = 0usize;
    while i14 < t1.len() {
        if t1[i14] != t2[i14] {
            return Err(E_STATE_DIFF);
        }
        i14 += 1;
    }
    hits += 1;

    // R15 步数守恒断言：标准六步旅程步数 = 6
    let mut wb6 = WalkBook::new();
    let mut s6 = S_WAKE;
    while s6 <= S_CONFIRM {
        wb6.step(s6, 25)?;
        s6 += 1;
    }
    let n6 = wb6.confirm()?;
    if n6 != 6 {
        return Err(E_BAL_BREAK);
    }
    hits += 1;

    // R16 总耗时上限：标准旅程 < 300ms 预算
    let total6 = wb6.total_cost();
    if total6 != 150 || total6 >= 300 {
        return Err(E_BAL_BREAK);
    }
    hits += 1;

    Ok(hits)
}
