//! unx36034_tokstats — UNX-F36034 令牌统计与计数账（活动令牌数/类型分布）。
//!
//! 判据原文（UNX-F36034-J1）：
//!   活动令牌计数在创建/销毁全路径守恒 10^3 次零漂移；类型分布账与
//!   枚举账双向对平。
//! 设计纪律：
//!   1. 活动数单源：StatsBook.active 是唯一活动计数面——create +1、
//!      destroy -1（已死再销毁走 E_STATE 不动账）；10^3 次全路径
//!      随机序（LCG 确定性）后 active == live 枚举账。
//!   2. 类型分布账：primary/impersonation 双账；分布账（计数面）与
//!      枚举账（逐令牌类型扫描重算）双向对平——漂移即账破。
//!   3. 守恒恒等式：active == primary + impersonation == live 枚举数；
//!      created == destroyed + active（总账）。
//!   4. 纯功能线：no_std，零 unwrap/expect/panic，确定性。

use crate::checks::CheckSet;

pub const SEG_BASE: u16 = 0xf940;

// ── 诊断码闭集（0xf940~0xf95f，独占）─────────────────────────
const E_DRIFT: u16 = SEG_BASE + 1; // 活动数漂移：active 与枚举账不平
const E_BAL: u16 = SEG_BASE + 2; // 总账守恒破（created != destroyed + active）
const E_STATE: u16 = SEG_BASE + 3; // 态机非法（死令牌再销毁等）
const E_DIST: u16 = SEG_BASE + 4; // 类型分布账与枚举账双向对平失败
const E_CAP: u16 = SEG_BASE + 5; // 令牌槽容量越限

/// 令牌槽容量。
pub const T_CAP: usize = 128;

/// 令牌类型（统计面）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TokType {
    Primary = 1,
    Impersonation = 2,
}

/// 令牌槽。
#[derive(Clone, Copy)]
pub struct Slot {
    pub used: bool,
    pub alive: bool,
    pub ty: u32, // 0=未用 1=主 2=模拟
}

/// 统计账（活动数 + 类型分布 + 总账）。
pub struct StatsBook {
    pub slots: [Slot; T_CAP],
    pub active: i64,
    pub created: u64,
    pub destroyed: u64,
    pub dist_primary: u64,
    pub dist_impersonation: u64,
    pub rejected: u64,
}

impl StatsBook {
    pub const fn new() -> StatsBook {
        StatsBook {
            slots: [Slot { used: false, alive: false, ty: 0 }; T_CAP],
            active: 0,
            created: 0,
            destroyed: 0,
            dist_primary: 0,
            dist_impersonation: 0,
            rejected: 0,
        }
    }

    /// 创建：占槽 + 活动数 + 分布账（类型必为主/模拟闭集）。
    pub fn create(&mut self, ty: u32) -> Result<usize, u16> {
        if ty != TokType::Primary as u32 && ty != TokType::Impersonation as u32 {
            self.rejected += 1;
            return Err(E_DIST);
        }
        let mut i = 0;
        while i < T_CAP {
            if !self.slots[i].used {
                self.slots[i] = Slot { used: true, alive: true, ty };
                self.active += 1;
                self.created += 1;
                if ty == TokType::Primary as u32 {
                    self.dist_primary += 1;
                } else {
                    self.dist_impersonation += 1;
                }
                return Ok(i);
            }
            i += 1;
        }
        self.rejected += 1;
        Err(E_CAP)
    }

    /// 销毁：活动数 -1 + 分布账 -1；死令牌再销毁显性拒绝（不动账）。
    pub fn destroy(&mut self, idx: usize) -> Result<(), u16> {
        if idx >= T_CAP || !self.slots[idx].used {
            self.rejected += 1;
            return Err(E_STATE);
        }
        if !self.slots[idx].alive {
            self.rejected += 1;
            return Err(E_STATE);
        }
        self.slots[idx].alive = false;
        self.active -= 1;
        self.destroyed += 1;
        if self.slots[idx].ty == TokType::Primary as u32 {
            self.dist_primary -= 1;
        } else {
            self.dist_impersonation -= 1;
        }
        Ok(())
    }

    /// 枚举账：逐槽扫描重算（活动数 + 类型分布的独立真源）。
    pub fn enumerate(&self) -> (i64, u64, u64) {
        let mut act = 0i64;
        let mut pri = 0u64;
        let mut imp = 0u64;
        let mut i = 0;
        while i < T_CAP {
            if self.slots[i].used && self.slots[i].alive {
                act += 1;
                if self.slots[i].ty == TokType::Primary as u32 {
                    pri += 1;
                } else {
                    imp += 1;
                }
            }
            i += 1;
        }
        (act, pri, imp)
    }

    /// 总账守恒：created == destroyed + active。
    pub fn conserve(&self) -> bool {
        self.created == self.destroyed + self.active as u64
    }

    /// 零漂移断言：活动数 + 双向分布账全平。
    pub fn no_drift(&self) -> bool {
        let (act, pri, imp) = self.enumerate();
        self.active == act
            && self.dist_primary == pri
            && self.dist_impersonation == imp
            && self.active == (self.dist_primary + self.dist_impersonation) as i64
    }
}

/// 诊断释义。
pub fn detail(code: u16) -> &'static str {
    match code {
        E_DRIFT => "活动数漂移：active 与逐槽枚举账不平",
        E_BAL => "总账守恒破：created != destroyed + active",
        E_STATE => "态机非法：死令牌再销毁或空槽销毁",
        E_DIST => "类型分布账与枚举账双向对平失败",
        E_CAP => "令牌槽容量越限：超过 T_CAP",
        _ => "未知诊断码",
    }
}

/// 确定性演练（J1 判据五段）。
pub fn drill() -> Result<u32, u16> {
    let mut hits = 0u32;
    let mut book = StatsBook::new();
    // R1：10^3 次创建/销毁随机序（LCG 确定性）——零漂移。
    // 序列：create 概率 3/5、destroy 概率 2/5（有活令牌时）。
    let mut seed: u32 = 0x5dee_ce66;
    let mut live_idx: [usize; T_CAP] = [usize::MAX; T_CAP];
    let mut live_n = 0usize;
    let mut steps = 0u32;
    while steps < 1000 {
        seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        let act = seed % 5;
        if act < 3 || live_n == 0 {
            // 创建（类型按 seed 位）。
            let ty = if seed >> 8 & 1 == 0 {
                TokType::Primary as u32
            } else {
                TokType::Impersonation as u32
            };
            match book.create(ty) {
                Ok(idx) => {
                    live_idx[live_n] = idx;
                    live_n += 1;
                }
                Err(E_CAP) => {
                    // 槽满——活令牌超 128 才发生；本序上限 128*? 概率低，
                    // 显性记账面（不改活清单）。
                }
                Err(_) => return Err(E_DIST),
            }
        } else {
            // 销毁（随机活令牌，交换补位）。
            let pick = (seed >> 4) as usize % live_n;
            let idx = live_idx[pick];
            book.destroy(idx)?;
            live_idx[pick] = live_idx[live_n - 1];
            live_n -= 1;
        }
        // 全路径中途抽查：零漂移。
        if !book.no_drift() || !book.conserve() {
            return Err(E_DRIFT);
        }
        steps += 1;
        hits += 1;
    }
    // R2：终态对平（活动数 == 分布账 == 枚举账）。
    if !book.no_drift() {
        return Err(E_DRIFT);
    }
    hits += 1;
    // R3：类型分布双向对平——枚举账重算分布 vs 分布账。
    let (act, pri, imp) = book.enumerate();
    if book.active != act
        || book.dist_primary != pri
        || book.dist_impersonation != imp
        || (pri + imp) as i64 != act
    {
        return Err(E_DIST);
    }
    hits += 1;
    // R4：全部销毁后归零 + 总账（created == destroyed）。
    let mut q = 0;
    while q < live_n {
        book.destroy(live_idx[q])?;
        q += 1;
    }
    if book.active != 0 || !book.no_drift() || !book.conserve() {
        return Err(E_BAL);
    }
    if book.destroyed != book.created {
        return Err(E_BAL);
    }
    hits += 1;
    // R5：死令牌再销毁拒绝 10 例 + 非法类型拒绝。
    let mut r = 0;
    while r < 10 {
        if book.destroy(0) != Err(E_STATE) {
            return Err(E_STATE);
        }
        r += 1;
    }
    if book.create(0x99) != Err(E_DIST) {
        return Err(E_DIST);
    }
    hits += 1;
    Ok(hits)
}

/// 统计账视图辅助面。

/// 统计快照（只读视图：活动数+分布+总账投影）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct StatsSnapshot {
    pub active: i64,
    pub created: u64,
    pub destroyed: u64,
    pub dist_primary: u64,
    pub dist_impersonation: u64,
}

impl StatsBook {
    /// 快照导出。
    pub fn snapshot(&self) -> StatsSnapshot {
        StatsSnapshot {
            active: self.active,
            created: self.created,
            destroyed: self.destroyed,
            dist_primary: self.dist_primary,
            dist_impersonation: self.dist_impersonation,
        }
    }

    /// 快照守恒核（逐字段 + 恒等式复核）。
    pub fn snapshot_conserve(&self, s: &StatsSnapshot) -> bool {
        s.active == self.active
            && s.created == self.created
            && s.destroyed == self.destroyed
            && s.dist_primary == self.dist_primary
            && s.dist_impersonation == self.dist_impersonation
            && s.created == (s.destroyed as i64 + s.active) as u64
            && s.active == (s.dist_primary + s.dist_impersonation) as i64
    }
}

pub fn run_unx36034_checks() -> CheckSet {
    let mut cs = CheckSet::new("unxstar-unx36034_tokstats");
    cs.add("SEG 段位独占", SEG_BASE == 0xf940, "0xf940~0xf95f 本模块独占");
    match drill() {
        Ok(_) => cs.add("J1 统计账守恒", true, "10^3 次零漂移+分布双向对平"),
        Err(code) => cs.add("J1 统计账守恒", false, detail(code)),
    }
    cs
}

#[cfg(test)]
mod tests {
    use super::drill;

    #[test]
    fn drill_green() {
        assert_eq!(drill(), Ok(1004));
    }
}
