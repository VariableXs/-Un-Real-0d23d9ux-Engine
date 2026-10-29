//! unx4134_openrollback — UNX-F4134 打开中途失败回滚账（诊断段 0x92A0）。
//!
//! 判据原文：UNX-F4134-J1 装配链五步各步失败注入（各 10 次）回滚完整——inode 引用/
//!           file 池/半成品结构三账全回归基线（残差=0）。
//!   J2 第 k 步失败时的拆解次数恰为 k。
//!   J3 注入步号越界不产生部分装配。
//!   J4 失败装配不得动既有对象的账。
//! 设计纪律：
//!   1. 装配链固定五步（lookup → inode_ref → file_alloc → sec_ctx → priv_data），
//!      顺序不可交换：后一步依赖前一步的产物，故失败只能「逆序拆」，不能「跳着拆」。
//!   2. 回滚是严格逆序的镜像：第 k 步失败时，已完成的 k-1 步按 k-1..1 依次拆解；
//!      拆解动作与装配动作一一对应，绝不留下「拆了没建过的」账。
//!   3. 三账（inode 引用、file 池槽、半成品结构）在回滚后必须逐项等于基线，残差按
//!      整数判等，不设容差；成功路径则三账各加一，不得多加。
//!   4. 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。
//! 诊断段 0x92A0 本单独占。

pub const SEG_BASE: u16 = 0x92A0;

// ── 诊断码闭集 ───────────────────────────────────────────────
const E_STEP: u16 = SEG_BASE + 1; // 步号越界（链只 5 步）
const E_ORDER: u16 = SEG_BASE + 2; // 相位跳变（未复位即重开）
const E_RESIDUE: u16 = SEG_BASE + 3; // 回滚后残差非零
const E_NOINODE: u16 = SEG_BASE + 4; // inode 引用账穿底
const E_NOFILE: u16 = SEG_BASE + 5; // file 池无空槽
const E_ACCT: u16 = SEG_BASE + 6; // 装配/回滚两账不平
const E_DOUBLE: u16 = SEG_BASE + 7; // 重复回滚同一步
const E_INJECT: u16 = SEG_BASE + 8; // 注入步号超出已完成步数

/// 链长：判据原文「五步」。
pub const CHAIN_LEN: u8 = 5;

/// 装配步编号（闭集，顺序有语义）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Step {
    Lookup = 0,
    InodeRef = 1,
    FileAlloc = 2,
    SecCtx = 3,
    PrivData = 4,
}

impl Step {
    pub fn from_idx(i: u8) -> Option<Step> {
        match i {
            0 => Some(Step::Lookup),
            1 => Some(Step::InodeRef),
            2 => Some(Step::FileAlloc),
            3 => Some(Step::SecCtx),
            4 => Some(Step::PrivData),
            _ => None,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Step::Lookup => "lookup",
            Step::InodeRef => "inode_ref",
            Step::FileAlloc => "file_alloc",
            Step::SecCtx => "sec_ctx",
            Step::PrivData => "priv_data",
        }
    }
}

/// 链相位机：Idle → Reserving → Committed；失败则 RollingBack → Aborted。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ChainPhase {
    Idle,
    Reserving,
    Committed,
    RollingBack,
    Aborted,
}

/// 装配三账（inode 引用 / file 槽 / 半成品结构）。
pub struct ThreeLedger {
    /// inode 引用数
    pub inode_refs: u32,
    /// file 池占用槽数
    pub file_slots: u32,
    /// 半成品结构（sec_ctx/priv_data 等中间态）数量
    pub half_structs: u32,
}

impl ThreeLedger {
    pub fn zero() -> ThreeLedger {
        ThreeLedger {
            inode_refs: 0,
            file_slots: 0,
            half_structs: 0,
        }
    }

    /// 与基线逐项判等（残差 = 0 的整数形式）。
    pub fn equals(&self, base: &ThreeLedger) -> bool {
        self.inode_refs == base.inode_refs
            && self.file_slots == base.file_slots
            && self.half_structs == base.half_structs
    }

    pub fn snapshot(&self) -> (u32, u32, u32) {
        (self.inode_refs, self.file_slots, self.half_structs)
    }
}

/// 装配链：当前进度 + 三账 + 步骤日志（用于逆序回滚）。
pub struct AssembleChain {
    /// 相位
    pub phase: ChainPhase,
    /// 已完成步数（0..=5）
    pub done: u8,
    /// 三账
    pub led: ThreeLedger,
    /// 已落账的步序（供逆序拆解；用定长数组避免动态分配）
    pub log: [u8; CHAIN_LEN as usize],
    /// 装配次数
    pub assembles: u32,
    /// 回滚次数（每拆一步 +1）
    pub rollback_ops: u32,
    /// 回滚后残差非零次数（判据应为 0）
    pub residue_hits: u32,
}

impl AssembleChain {
    pub fn new() -> AssembleChain {
        AssembleChain {
            phase: ChainPhase::Idle,
            done: 0,
            led: ThreeLedger::zero(),
            log: [0xFF; CHAIN_LEN as usize],
            assembles: 0,
            rollback_ops: 0,
            residue_hits: 0,
        }
    }

    /// 单步装配：按步号施加对应副作用（三账各自的加一）。
    fn apply(&mut self, step: Step) -> Result<(), u16> {
        let idx = step as usize;
        if idx >= CHAIN_LEN as usize {
            return Err(E_STEP);
        }
        if self.log[idx] != 0xFF {
            return Err(E_DOUBLE); // 同一步不得装配两次
        }
        match step {
            Step::Lookup => {
                // lookup 只解析路径，不占用三账（但仍须落日志以便回滚）
            }
            Step::InodeRef => {
                self.led.inode_refs += 1;
            }
            Step::FileAlloc => {
                self.led.file_slots += 1;
            }
            Step::SecCtx => {
                self.led.half_structs += 1;
            }
            Step::PrivData => {
                self.led.half_structs += 1;
            }
        }
        self.log[idx] = step as u8;
        self.done += 1;
        Ok(())
    }

    /// 单步拆解：与 apply 严格镜像，三账各减一。
    fn undo(&mut self, step: Step) -> Result<(), u16> {
        let idx = step as usize;
        if idx >= CHAIN_LEN as usize {
            return Err(E_STEP);
        }
        if self.log[idx] == 0xFF {
            return Err(E_DOUBLE); // 未装配却拆解
        }
        match step {
            Step::Lookup => {}
            Step::InodeRef => {
                if self.led.inode_refs == 0 {
                    return Err(E_NOINODE);
                }
                self.led.inode_refs -= 1;
            }
            Step::FileAlloc => {
                if self.led.file_slots == 0 {
                    return Err(E_NOFILE);
                }
                self.led.file_slots -= 1;
            }
            Step::SecCtx | Step::PrivData => {
                if self.led.half_structs == 0 {
                    return Err(E_ACCT);
                }
                self.led.half_structs -= 1;
            }
        }
        self.log[idx] = 0xFF;
        if self.done == 0 {
            return Err(E_ACCT);
        }
        self.done -= 1;
        self.rollback_ops += 1;
        Ok(())
    }

    /// 逆序回滚全部已完成步：done-1 .. 0。
    fn rollback_all(&mut self, base: &ThreeLedger) -> Result<(), u16> {
        self.phase = ChainPhase::RollingBack;
        let mut k = self.done;
        while k > 0 {
            let idx = (k - 1) as usize;
            let step = match Step::from_idx(idx as u8) {
                Some(s) => s,
                None => return Err(E_STEP),
            };
            if self.log[idx] != 0xFF {
                self.undo(step)?;
            }
            k -= 1;
        }
        if !self.led.equals(base) {
            self.residue_hits += 1;
            self.phase = ChainPhase::Aborted;
            return Err(E_RESIDUE);
        }
        self.phase = ChainPhase::Aborted;
        Ok(())
    }

    /// 完整 open：fail_at 为 Some(k) 表示第 k 步注入失败（0 基）。
    pub fn open(&mut self, fail_at: Option<u8>) -> Result<u32, u16> {
        if self.phase == ChainPhase::Reserving || self.phase == ChainPhase::RollingBack {
            return Err(E_ORDER);
        }
        // 每次 open 从干净态起步（上一次 Aborted 已复位三账）
        self.done = 0;
        self.log = [0xFF; CHAIN_LEN as usize];
        self.phase = ChainPhase::Reserving;
        self.assembles += 1;
        let base = self.led.snapshot();

        let mut i = 0u8;
        while i < CHAIN_LEN {
            let step = match Step::from_idx(i) {
                Some(s) => s,
                None => return Err(E_STEP),
            };
            if let Some(f) = fail_at {
                if f == i {
                    // 注入点：本步失败，回滚已完成的 i 步
                    let b = ThreeLedger {
                        inode_refs: base.0,
                        file_slots: base.1,
                        half_structs: base.2,
                    };
                    self.rollback_all(&b)?;
                    return Err(E_INJECT);
                }
                if f > i && f >= CHAIN_LEN {
                    return Err(E_INJECT); // 注入步号超出链长
                }
            }
            self.apply(step)?;
            i += 1;
        }
        self.phase = ChainPhase::Committed;
        Ok(self.done as u32)
    }

    /// 提交后关闭：逆序拆全部步（正常释放路径，与回滚同构）。
    pub fn close(&mut self) -> Result<(), u16> {
        if self.phase != ChainPhase::Committed {
            return Err(E_ORDER);
        }
        let base = ThreeLedger::zero();
        self.rollback_all(&base)?;
        if !self.led.equals(&base) {
            return Err(E_RESIDUE);
        }
        Ok(())
    }
}

/// 回滚账本：五步各 10 次注入的汇总。
pub struct RollbackLedger {
    /// 注入总次数
    pub injections: u32,
    /// 回滚完整（残差 0）的次数
    pub clean_rollbacks: u32,
    /// 回滚残留次数（判据应为 0）
    pub dirty_rollbacks: u32,
    /// 成功装配次数
    pub commits: u32,
    /// 每步注入计数（5 元）
    pub per_step: [u32; CHAIN_LEN as usize],
}

impl RollbackLedger {
    pub fn new() -> RollbackLedger {
        RollbackLedger {
            injections: 0,
            clean_rollbacks: 0,
            dirty_rollbacks: 0,
            commits: 0,
            per_step: [0; CHAIN_LEN as usize],
        }
    }

    /// 记录一次注入结果。
    pub fn note(&mut self, step: u8, residue: bool) {
        self.injections += 1;
        if (step as usize) < self.per_step.len() {
            self.per_step[step as usize] += 1;
        }
        if residue {
            self.dirty_rollbacks += 1;
        } else {
            self.clean_rollbacks += 1;
        }
    }

    /// 五步均匀：每步注入数相等且非零。
    pub fn uniform(&self) -> bool {
        let mut i = 1usize;
        while i < self.per_step.len() {
            if self.per_step[i] != self.per_step[0] || self.per_step[0] == 0 {
                return false;
            }
            i += 1;
        }
        true
    }
}

/// 诊断码解释。
pub fn detail(code: u16) -> &'static str {
    match code {
        E_STEP => "步号越界（链只 5 步）",
        E_ORDER => "相位跳变（未复位即重开）",
        E_RESIDUE => "回滚后三账残差非零",
        E_NOINODE => "inode 引用账穿底",
        E_NOFILE => "file 池槽账穿底",
        E_ACCT => "装配/回滚两账不平",
        E_DOUBLE => "重复装配或重复拆解同一步",
        E_INJECT => "注入步号超出链长",
        _ => "未知诊断码",
    }
}

/// 确定性演练：五步各 10 次失败注入 → 三账归基线 → 成功路径 → 两账对账。
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;

    // R1：成功路径——五步全通，三账各为 (1,1,2)（sec_ctx 与 priv_data 各占一个半成品）。
    let mut ch = AssembleChain::new();
    if ch.open(None)? != 5 {
        return Err(E_ACCT);
    }
    if ch.phase != ChainPhase::Committed {
        return Err(E_ORDER);
    }
    if ch.led.snapshot() != (1, 1, 2) {
        return Err(E_ACCT);
    }
    hits += 1;

    // R2：正常关闭——逆序拆全部步，三账归零，回滚操作 5 次。
    ch.close()?;
    if ch.led.snapshot() != (0, 0, 0) {
        return Err(E_RESIDUE);
    }
    if ch.rollback_ops != 5 {
        return Err(E_ACCT);
    }
    hits += 1;

    // R3：五步各注入 10 次（共 50），每次回滚后三账必须回到基线 (0,0,0)。
    let mut rl = RollbackLedger::new();
    let mut s = 0u8;
    while s < CHAIN_LEN {
        let mut n = 0;
        while n < 10 {
            let mut c = AssembleChain::new();
            let r = c.open(Some(s));
            if r != Err(E_INJECT) {
                return Err(E_INJECT);
            }
            if c.phase != ChainPhase::Aborted {
                return Err(E_ORDER);
            }
            let clean = c.led.snapshot() == (0, 0, 0) && c.residue_hits == 0;
            rl.note(s, !clean);
            if !clean {
                return Err(E_RESIDUE);
            }
            n += 1;
        }
        s += 1;
    }
    if rl.injections != 50 || rl.clean_rollbacks != 50 || rl.dirty_rollbacks != 0 {
        return Err(E_ACCT);
    }
    if !rl.uniform() {
        return Err(E_ACCT);
    }
    hits += 1;

    // R4：逐步核对回滚深度——第 k 步失败时恰好拆 k 次（k=0 时零次）。
    let mut k = 0u8;
    while k < CHAIN_LEN {
        let mut c = AssembleChain::new();
        let _ = c.open(Some(k));
        if c.rollback_ops != k as u32 {
            return Err(E_ACCT);
        }
        if c.led.snapshot() != (0, 0, 0) {
            return Err(E_RESIDUE);
        }
        k += 1;
    }
    hits += 1;

    // R5：基线非零时的回滚——先成功一次（三账 1/1/2），再注入失败，须回到 1/1/2。
    let mut c = AssembleChain::new();
    c.open(None)?;
    let base = c.led.snapshot();
    let fail = c.open(Some(3));
    if fail != Err(E_INJECT) {
        return Err(E_INJECT);
    }
    if c.led.snapshot() != base {
        return Err(E_RESIDUE); // 失败装配不得动到既有对象的账
    }
    hits += 1;

    // I1：步号越界注入 → E_INJECT（不 panic、不部分装配）。
    let mut c2 = AssembleChain::new();
    if c2.open(Some(CHAIN_LEN)) != Err(E_INJECT) {
        return Err(E_INJECT);
    }
    if c2.led.snapshot() != (0, 0, 0) {
        return Err(E_RESIDUE);
    }
    hits += 1;

    // I2：未提交即关闭 → E_ORDER；未成功即再次 close → E_ORDER。
    let mut c3 = AssembleChain::new();
    if c3.close() != Err(E_ORDER) {
        return Err(E_ORDER);
    }
    let _ = c3.open(Some(2));
    if c3.close() != Err(E_ORDER) {
        return Err(E_ORDER);
    }
    hits += 1;

    // I3：步号闭集拒收。
    if Step::from_idx(CHAIN_LEN).is_some() || Step::from_idx(99).is_some() {
        return Err(E_STEP);
    }
    hits += 1;

    // C1：两账终审——装配 5 次（R1 一次、R5 一次、R5 注入一次、其余各一次属不同实例）。
    if !rl.uniform() || rl.per_step[0] != 10 {
        return Err(E_ACCT);
    }
    if rl.clean_rollbacks + rl.dirty_rollbacks != rl.injections {
        return Err(E_ACCT);
    }
    if ch.residue_hits != 0 || c.residue_hits != 0 {
        return Err(E_RESIDUE);
    }
    hits += 1;

    Ok(hits)
}
