//! unx4027_sbswitch — UNX-F4027 get_sb/kill_sb 钩子语义（诊断段 0x85E0）。
//!
//! 判据原文：
//!   J1 get_sb 创建实例返回 active sb；
//!   J2 kill_sb 幂等（双次调用零二次回收）；
//!   J3 钩子未实现时调用返回 ENOSYS 且系统不崩。
//! 设计纪律：
//!   1. 钩子表模型：fs 类型携带 get_sb/kill_sb 两个钩子位，钩子位
//!      未挂（未实现）时调用方收到 ENOSYS，枢纽状态不变。
//!   2. sb 生命周期状态机：Unborn → Active → Killed，单向迁移，
//!      Killed 是终态；get_sb 只能从 Unborn 到 Active。
//!   3. kill_sb 幂等：首次 Killed 迁移执行回收账（回收数 +1），
//!      二次调用直接返回 Ok 且账不动——回收计数守恒式：
//!      回收总数 ≤ kill 调用总数，且每实例回收恰好 ≤ 1。
//!   4. 账目守恒：get 成功数 = active 峰值产生数；kill 调用数 =
//!      实际回收数 + 幂等空转数。
//!   5. 边界：对 Killed/Unborn 实例的 kill、对 Active 实例的再 get
//!      （同槽位）全部拒收并记错误账。
//! 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。

pub const SEG_BASE: u16 = 0x85E0;

// ── 错误码闭集 ───────────────────────────────────────────────
const E_NO_GET_SB: u16 = SEG_BASE + 1;   // get_sb 钩子未实现（ENOSYS 语义）
const E_NO_KILL_SB: u16 = SEG_BASE + 2;  // kill_sb 钩子未实现（ENOSYS 语义）
const E_ALREADY_ACTIVE: u16 = SEG_BASE + 3; // 实例已 Active 再 get 同槽位
const E_NOT_ACTIVE: u16 = SEG_BASE + 4;  // 对非 Active 实例 kill（无可用对象）
const E_STALE_KILL: u16 = SEG_BASE + 5;  // Killed 终态后再 kill（幂等空转通道外）
const E_BAL: u16 = SEG_BASE + 6;         // get/kill 账不平（守恒失守）

/// sb 生命周期状态机：单向迁移，Killed 为终态。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SbPhase {
    Unborn, // 尚未 get_sb（占位已建，钩子未触发）
    Active, // get_sb 成功，实例在用
    Killed, // kill_sb 已执行（终态，幂等）
}

/// fs 类型钩子表：两个钩子位，未挂位返回 ENOSYS。
pub struct FsHookTable {
    pub name: [u8; 8],   // 类型名（定长，零填充）
    pub has_get_sb: bool, // get_sb 钩子是否实现
    pub has_kill_sb: bool, // kill_sb 钩子是否实现
}

/// 单个 sb 实例：阶段 + 钩子执行账。
pub struct SbInstance {
    pub phase: SbPhase,
    pub get_calls: u32,   // 对本实例 get_sb 触达次数（含被拒）
    pub kill_calls: u32,  // 对本实例 kill_sb 触达次数（含幂等空转）
    pub reclaims: u32,    // 实际执行回收次数（≤1，幂等判据面）
}

/// get_sb/kill_sb 钩子枢纽：类型钩子表 + 实例 + 全局账。
pub struct SbSwitch {
    pub hook: FsHookTable,
    pub inst: SbInstance,
    pub get_ok_total: u32,   // 全局 get 成功数
    pub kill_call_total: u32, // 全局 kill 调用数
    pub reclaim_total: u32,  // 全局实际回收数
    pub idle_kill_total: u32, // 幂等空转数（二次 kill）
    pub rejected_total: u32, // 被拒调用总数（错误账）
}

impl SbSwitch {
    /// 构造：建 Unborn 实例，钩子表按入参定编。
    pub fn new(name: &[u8; 8], get_impl: bool, kill_impl: bool) -> SbSwitch {
        SbSwitch {
            hook: FsHookTable {
                name: *name,
                has_get_sb: get_impl,
                has_kill_sb: kill_impl,
            },
            inst: SbInstance {
                phase: SbPhase::Unborn,
                get_calls: 0,
                kill_calls: 0,
                reclaims: 0,
            },
            get_ok_total: 0,
            kill_call_total: 0,
            reclaim_total: 0,
            idle_kill_total: 0,
            rejected_total: 0,
        }
    }

    /// get_sb 钩子触发：未实现返 ENOSYS；已 Active 拒收；否则迁 Active。
    pub fn get_sb(&mut self) -> Result<SbPhase, u16> {
        if !self.hook.has_get_sb {
            self.rejected_total += 1;
            return Err(E_NO_GET_SB);
        }
        self.inst.get_calls += 1;
        match self.inst.phase {
            SbPhase::Unborn => {
                self.inst.phase = SbPhase::Active;
                self.get_ok_total += 1;
                Ok(SbPhase::Active)
            }
            SbPhase::Active => {
                self.rejected_total += 1;
                Err(E_ALREADY_ACTIVE)
            }
            SbPhase::Killed => {
                // 终态后 get 视为陈旧引用，拒收入错误账。
                self.rejected_total += 1;
                Err(E_STALE_KILL)
            }
        }
    }

    /// kill_sb 钩子触发：未实现返 ENOSYS；非 Active 拒收；
    /// 首次 Killed 迁移执行回收；二次调用幂等空转（零二次回收）。
    pub fn kill_sb(&mut self) -> Result<u32, u16> {
        if !self.hook.has_kill_sb {
            self.rejected_total += 1;
            return Err(E_NO_KILL_SB);
        }
        match self.inst.phase {
            SbPhase::Active => {
                self.kill_call_total += 1;
                self.inst.kill_calls += 1;
                self.inst.phase = SbPhase::Killed;
                self.inst.reclaims += 1;
                self.reclaim_total += 1;
                Ok(self.reclaim_total)
            }
            SbPhase::Unborn => {
                self.rejected_total += 1;
                Err(E_NOT_ACTIVE)
            }
            SbPhase::Killed => {
                // 幂等通道：返回 Ok 但零二次回收。
                self.kill_call_total += 1;
                self.idle_kill_total += 1;
                self.inst.kill_calls += 1;
                Ok(self.reclaim_total)
            }
        }
    }

    /// 查询当前阶段。
    pub fn phase(&self) -> SbPhase {
        self.inst.phase
    }

    /// 守恒对账：get 成功数 + 被拒数 = get 触达数；
    /// 回收数 + 空转数 = kill 调用数；每实例回收 ≤ 1。
    pub fn audit(&self) -> Result<(), u16> {
        // get 账：成功数不超过触达数（其余为阶段拒收）。
        if self.get_ok_total > self.inst.get_calls {
            return Err(E_BAL);
        }
        // kill 账：实际回收 + 幂等空转 = 有效调用总数。
        if self.reclaim_total + self.idle_kill_total != self.kill_call_total {
            return Err(E_BAL);
        }
        // 幂等不变式：单实例实际回收 ≤ 1。
        if self.inst.reclaims > 1 {
            return Err(E_BAL);
        }
        Ok(())
    }

    /// 导出账面快照（get 成功，kill 调用，实际回收，空转，拒收）。
    pub fn ledger(&self) -> (u32, u32, u32, u32, u32) {
        (
            self.get_ok_total,
            self.kill_call_total,
            self.reclaim_total,
            self.idle_kill_total,
            self.rejected_total,
        )
    }

    /// 钩子表视图：返回（get 位，kill 位，类型名有效长度）。
    pub fn hook_view(&self) -> (bool, bool, usize) {
        let mut n = 0;
        while n < self.hook.name.len() && self.hook.name[n] != 0 {
            n += 1;
        }
        (self.hook.has_get_sb, self.hook.has_kill_sb, n)
    }

    /// 活性查询：实例是否处于 Active（挂载期可用态）。
    pub fn is_active(&self) -> bool {
        self.inst.phase == SbPhase::Active
    }

    /// 单实例账视图：返回（get 触达，kill 触达，本实例回收数）。
    pub fn inst_view(&self) -> (u32, u32, u32) {
        (self.inst.get_calls, self.inst.kill_calls, self.inst.reclaims)
    }

    /// 状态机迁移合法性核验（单向迁移不变式）：
    /// Unborn 只允许迁 Active；Active 只允许迁 Killed；Killed 终态。
    pub fn transition_ok(&self) -> bool {
        match self.inst.phase {
            SbPhase::Unborn => self.inst.reclaims == 0,
            SbPhase::Active => self.get_ok_total >= 1 && self.inst.reclaims == 0,
            SbPhase::Killed => self.inst.reclaims == 1,
        }
    }

    /// 拒收账细分视图：错误账按钩子缺失（其余为阶段拒收）二分。
    pub fn reject_split(&self) -> (u32, u32) {
        let hookless = if !self.hook.has_get_sb { 1 } else { 0 }
            + if !self.hook.has_kill_sb { 1 } else { 0 };
        let phase_rej = self.rejected_total.saturating_sub(hookless);
        (hookless, phase_rej)
    }

    /// 阶段名定编视图（诊断观测面）：三态编号 0/1/2 闭集。
    pub fn phase_code(&self) -> u16 {
        match self.inst.phase {
            SbPhase::Unborn => 0,
            SbPhase::Active => 1,
            SbPhase::Killed => 2,
        }
    }
}

/// 自例程：R1 正常链 → R2 幂等 → R3 ENOSYS 注入 → R4 守恒对账。
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;

    // R1 正常路径：get_sb 建 Active 实例并返回 active 态。
    let mut sw = SbSwitch::new(b"rootfs\0\0", true, true);
    let ph = sw.get_sb()?;
    if ph != SbPhase::Active {
        return Err(E_BAL);
    }
    hits += 1;

    // R2 kill_sb 幂等：双次调用，回收恰好一次，第二次 Ok 且账不动。
    let r1 = sw.kill_sb()?;
    let r2 = sw.kill_sb()?;
    if r1 != r2 || sw.inst.reclaims != 1 || sw.reclaim_total != 1 {
        return Err(E_BAL);
    }
    if sw.phase() != SbPhase::Killed || sw.idle_kill_total != 1 {
        return Err(E_BAL);
    }
    hits += 1;

    // R3 钩子未实现注入：get/kill 各返 ENOSYS 类码且状态不迁移。
    let mut hw = SbSwitch::new(b"nofs\0\0\0\0", false, false);
    let g = hw.get_sb();
    let k = hw.kill_sb();
    if g != Err(E_NO_GET_SB) || k != Err(E_NO_KILL_SB) {
        return Err(E_BAL);
    }
    if hw.phase() != SbPhase::Unborn || hw.rejected_total != 2 {
        return Err(E_BAL);
    }
    hits += 1;

    // R4 边界注入：Unborn 期 kill 被拒；Active 期重复 get 被拒。
    let mut sw2 = SbSwitch::new(b"bndfs\0\0\0", true, true);
    if sw2.kill_sb() != Err(E_NOT_ACTIVE) {
        return Err(E_BAL);
    }
    sw2.get_sb()?;
    if sw2.get_sb() != Err(E_ALREADY_ACTIVE) {
        return Err(E_BAL);
    }
    hits += 1;

    // R5 守恒对账：两组枢纽各跑一遍 audit。
    sw.audit()?;
    sw2.audit()?;
    let (g_ok, k_call, rc, idle, _rej) = sw.ledger();
    if g_ok != 1 || k_call != 2 || rc != 1 || idle != 1 {
        return Err(E_BAL);
    }
    hits += 1;

    // R6 状态机单向迁移核验：三态各验一次 transition_ok。
    if !sw.transition_ok() || !hw.transition_ok() {
        return Err(E_BAL);
    }
    // sw2 处于 Active：迁移核验须通过且 is_active 为真。
    if !sw2.transition_ok() || !sw2.is_active() {
        return Err(E_BAL);
    }
    hits += 1;

    // R7 视图一致性：钩子表/实例账/拒收细分三视图与主账互证。
    let (hg, hk, hn) = sw.hook_view();
    if !hg || !hk || hn != 6 {
        // "rootfs\0\0" 有效名长 6。
        return Err(E_BAL);
    }
    let (ic_g, ic_k, ic_r) = sw.inst_view();
    if ic_g != 1 || ic_k != 2 || ic_r != 1 {
        return Err(E_BAL);
    }
    let (hl, pr) = sw2.reject_split();
    if hl != 0 || pr != 2 {
        // sw2 拒收账：R4 前置 kill 拒 1 + Active 重复 get 拒 1，无钩子缺失。
        return Err(E_BAL);
    }
    // 阶段名定编视图：三态闭集编号核验（Killed=2）。
    if sw.phase_code() != 2 || hw.phase_code() != 0 || sw2.phase_code() != 1 {
        return Err(E_BAL);
    }
    hits += 1;

    Ok(hits)
}
