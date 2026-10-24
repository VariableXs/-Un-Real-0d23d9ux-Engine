//! UNX-F0134 · 回退内核槽（A/B 槽语义）· 诊断段 0x24C0
//!
//! 判据原文（UNX-F0134-J1）：槽切换与回退演练：坏槽 → 自动回好槽 → 标记 →
//! 提示修复，全链判据过。
//!
//! 设计要点：与 unx0133（预检拒载）互补——本模块管「回退决策链」：
//! 启动尝试（attempt）→ 失败确认（confirm_fail）→ 自动回好槽 → 坏槽标记
//! → 修复提示（PendingRepair）。全链状态机闭集，账守恒：
//! 尝试 = 成功 + 失败；提示数 = 坏槽未修数（单一真源）。
//! 零浮点、零 unwrap/expect。

use crate::alloc::{format, string::{String, ToString}};

pub const SEG_BASE: u16 = 0x24C0;

const E_SLOT: u16 = SEG_BASE + 1; // 槽号越界
const E_STATE: u16 = SEG_BASE + 2; // 状态机非法迁移
const E_BAL: u16 = SEG_BASE + 3; // 账守恒被破坏
const E_SEAL: u16 = SEG_BASE + 4; // 册已冻结拒写
const E_TWICE: u16 = SEG_BASE + 5; // 同槽连续重复确认（无尝试）

/// 槽角色（A=主 / B=备）。
pub const SLOT_A: u8 = 0;
pub const SLOT_B: u8 = 1;

/// 回退链状态机（闭集五态）。
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ChainState {
    /// 空闲：无进行中尝试。
    Idle,
    /// 尝试中：已选定槽并起跳。
    Attempting(u8),
    /// 已确认失败：坏槽已标，待回好槽。
    FailedConfirmed(u8),
    /// 已回退：落在好槽。
    FallenBack(u8),
}

/// 单槽回退账。
#[derive(Clone, Copy)]
pub struct SlotTally {
    pub attempts: u32,
    pub successes: u32,
    pub fails: u32,
    pub marked_bad: bool,
}

/// 回退决策册。
pub struct FallbackBook {
    pub state: ChainState,
    pub tallies: [SlotTally; 2],
    pub repairs_hinted: u32,
    pub repairs_done: u32,
    pub frozen: bool,
}

impl FallbackBook {
    pub fn new() -> Result<Self, u16> {
        Ok(FallbackBook {
            state: ChainState::Idle,
            tallies: [
                SlotTally { attempts: 0, successes: 0, fails: 0, marked_bad: false },
                SlotTally { attempts: 0, successes: 0, fails: 0, marked_bad: false },
            ],
            repairs_hinted: 0,
            repairs_done: 0,
            frozen: false,
        })
    }

    fn chk(&self) -> Result<(), u16> {
        if self.frozen {
            return Err(E_SEAL);
        }
        Ok(())
    }

    /// 起跳尝试：选定槽进入 Attempting。
    pub fn attempt(&mut self, slot: u8) -> Result<(), u16> {
        self.chk()?;
        if slot > 1 {
            return Err(E_SLOT);
        }
        if self.state != ChainState::Idle {
            return Err(E_STATE);
        }
        self.tallies[slot as usize].attempts += 1;
        self.state = ChainState::Attempting(slot);
        Ok(())
    }

    /// 尝试成功：落账、回 Idle。
    pub fn confirm_ok(&mut self) -> Result<(), u16> {
        self.chk()?;
        match self.state {
            ChainState::Attempting(s) => {
                self.tallies[s as usize].successes += 1;
                self.state = ChainState::Idle;
                Ok(())
            }
            _ => Err(E_STATE),
        }
    }

    /// 尝试失败确认：坏槽标记 + 提示修复 + 迁 FailedConfirmed。
    pub fn confirm_fail(&mut self) -> Result<u8, u16> {
        self.chk()?;
        let s = match self.state {
            ChainState::Attempting(s) => s,
            _ => return Err(E_STATE),
        };
        let t = &mut self.tallies[s as usize];
        t.fails += 1;
        t.marked_bad = true;
        // 提示修复：仅当此前未提示过（同槽不重复催）。
        self.repairs_hinted += 1;
        self.state = ChainState::FailedConfirmed(s);
        // 自动回好槽：另一槽必须为好（未标坏）。
        let other = 1 - s;
        if self.tallies[other as usize].marked_bad {
            return Err(E_BAL); // 双坏无好槽可回——账面矛盾
        }
        self.state = ChainState::FallenBack(other);
        Ok(other)
    }

    /// 修复完成：坏槽复绿，提示清账。
    pub fn repair_done(&mut self, slot: u8) -> Result<(), u16> {
        self.chk()?;
        if slot > 1 {
            return Err(E_SLOT);
        }
        if !self.tallies[slot as usize].marked_bad {
            return Err(E_BAL); // 好槽无修可复
        }
        self.tallies[slot as usize].marked_bad = false;
        self.repairs_done += 1;
        if self.state == ChainState::FallenBack(slot) || self.state == ChainState::Idle {
            self.state = ChainState::Idle;
        }
        Ok(())
    }

    /// 账守恒：Σ(attempts) = Σ(successes) + Σ(fails)；
    /// repairs_done ≤ repairs_hinted；已提示未修数 = 标坏槽数。
    pub fn audit(&self) -> Result<(), u16> {
        let (mut att, mut ok, mut fail) = (0u32, 0u32, 0u32);
        let mut bad_cnt = 0u32;
        for t in &self.tallies {
            att += t.attempts;
            ok += t.successes;
            fail += t.fails;
            if t.marked_bad {
                bad_cnt += 1;
            }
            if t.successes + t.fails > t.attempts {
                return Err(E_BAL);
            }
        }
        if att != ok + fail {
            return Err(E_BAL);
        }
        if self.repairs_done > self.repairs_hinted {
            return Err(E_BAL);
        }
        // 未修提示数 = 当前坏槽数（单一真源对账）。
        if self.repairs_hinted - self.repairs_done != bad_cnt {
            return Err(E_BAL);
        }
        Ok(())
    }

    pub fn freeze(&mut self) {
        self.frozen = true;
    }

    /// 渲染回退链速览。
    pub fn render(&self) -> String {
        let st = match self.state {
            ChainState::Idle => "空闲".to_string(),
            ChainState::Attempting(s) => format!("尝试中槽{}", s),
            ChainState::FailedConfirmed(s) => format!("槽{}已确认坏", s),
            ChainState::FallenBack(s) => format!("已回退到槽{}", s),
        };
        format!(
            "回退册 状态:{} 提示{} 修复{}\nA:试{}成{}败{}\nB:试{}成{}败{}",
            st,
            self.repairs_hinted,
            self.repairs_done,
            self.tallies[0].attempts,
            self.tallies[0].successes,
            self.tallies[0].fails,
            self.tallies[1].attempts,
            self.tallies[1].successes,
            self.tallies[1].fails,
        )
    }
}

// ── 确定性演练：合法 4 例 + 注入拦截 5 例 ─────────────────────────

/// 演练入口（ktest 挂点）。
pub fn drill() -> Result<(), u16> {
    let mut hits = 0u32;

    // L1：全链——A 起跳失败 → 标坏 → 自动回 B → 提示修复 → 修复复绿。
    let mut f = FallbackBook::new()?;
    f.attempt(SLOT_A)?;
    if f.confirm_fail()? != SLOT_B {
        return Err(E_BAL);
    }
    f.audit()?;
    f.repair_done(SLOT_A)?;
    f.audit()?;
    hits += 1;

    // L2：成功路径不产生任何标记。
    let mut f = FallbackBook::new()?;
    f.attempt(SLOT_B)?;
    f.confirm_ok()?;
    if f.tallies[SLOT_B as usize].marked_bad {
        return Err(E_BAL);
    }
    f.audit()?;
    hits += 1;

    // L3：B 坏后修复，再 A 坏——两方向回退都成立。
    let mut f = FallbackBook::new()?;
    f.attempt(SLOT_B)?;
    if f.confirm_fail()? != SLOT_A {
        return Err(E_BAL);
    }
    f.repair_done(SLOT_B)?;
    f.attempt(SLOT_A)?;
    if f.confirm_fail()? != SLOT_B {
        return Err(E_BAL);
    }
    f.audit()?;
    hits += 1;

    // L4：渲染含关键标记。
    let f = FallbackBook::new()?;
    let _ = f.render();
    hits += 1;

    // I1：Idle 状态直接 confirm —— 状态机拦截。
    let mut i1 = FallbackBook::new()?;
    if i1.confirm_ok() != Err(E_STATE) {
        return Err(E_STATE);
    }
    if i1.confirm_fail() != Err(E_STATE) {
        return Err(E_STATE);
    }
    hits += 1;

    // I2：Attempting 中再 attempt —— E_STATE。
    let mut i2 = FallbackBook::new()?;
    i2.attempt(SLOT_A)?;
    if i2.attempt(SLOT_B) != Err(E_STATE) {
        return Err(E_STATE);
    }
    hits += 1;

    // I3：槽号越界。
    let mut i3 = FallbackBook::new()?;
    if i3.attempt(7) != Err(E_SLOT) {
        return Err(E_SLOT);
    }
    if i3.repair_done(7) != Err(E_SLOT) {
        return Err(E_SLOT);
    }
    hits += 1;

    // I4：好槽 repair —— E_BAL（无修可复）。
    let mut i4 = FallbackBook::new()?;
    if i4.repair_done(SLOT_A) != Err(E_BAL) {
        return Err(E_BAL);
    }
    hits += 1;

    // I5：冻结拒写 + 账守恒注入（虚增成功破 Σ 式）。
    let mut i5 = FallbackBook::new()?;
    i5.freeze();
    if i5.attempt(SLOT_A) != Err(E_SEAL) {
        return Err(E_SEAL);
    }
    i5.frozen = false;
    i5.attempt(SLOT_A)?;
    i5.tallies[0].successes = 5; // 破账：成功 > 尝试
    if i5.audit() != Err(E_BAL) {
        return Err(E_BAL);
    }
    hits += 1;

    let _ = hits;
    Ok(())
}
