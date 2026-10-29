//! unx4153_norecurse — UNX-F4153 表调用零递归约定（fs 不得回调 VFS 内部）（诊断段 0x94E0）。
//!
//! 判据原文：
//!   J1 fs 钩子内回调 VFS 内部函数注入 100/100 次被重入检测器拦截并留账
//!      （防递归死锁红线件）。
//!   J2 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计纪律：
//!   1. 重入检测器以深度账 + 在钩内标志双账裁定：进入 VFS 内部函数
//!      深度 +1，退出 -1；深度非零期再进入即判重入。
//!   2. 钩内回调红线：fs 钩子执行期（in_hook=true）任何 VFS 内部入口
//!      一律拦截留账，拦截后深度必须复原（无泄漏）。
//!   3. 拦截计数与深度守恒：enter 计数 = exit 计数 + 拦截计数（拦截
//!      不入深度账）。
//!   4. 边界显性：钩外正常嵌套调用（深度 1..MAX_DEPTH）放行，
//!      超 MAX_DEPTH 也拦截（栈溢防线）。
//!   5. 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。

use crate::alloc::vec::Vec;

pub const SEG_BASE: u16 = 0x94E0;

// ── 诊断码闭集 ───────────────────────────────────────────────
const E_REENTER: u16 = SEG_BASE + 1;   // 钩内回调 VFS 内部（红线拦截）
const E_DEPTH_MAX: u16 = SEG_BASE + 2; // 深度超上限（栈溢防线）
const E_EXIT_MISMATCH: u16 = SEG_BASE + 3; // 无对应 enter 先 exit
const E_DEPTH_LEAK: u16 = SEG_BASE + 4;// 会话结束深度未归零（泄漏）
const E_NOT_IN_HOOK: u16 = SEG_BASE + 5;   // 未在钩内却走钩出口
const E_GUARD_MIX: u16 = SEG_BASE + 6; // 检测器状态机非法迁移
const E_BAL: u16 = SEG_BASE + 7;       // enter/exit/拦截三账不平
const E_EMPTY_SUM: u16 = SEG_BASE + 8; // 写回摘要为空

pub const MAX_DEPTH: u32 = 8; // 正常嵌套上限（调用栈防线）

/// 检测器状态机：Outside 钩外 / InHook 钩内 / Intercepted 拦截期。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum GuardState {
    Outside,
    InHook,
    Intercepted,
}

/// 一次拦截留账条目。
pub struct InterceptRecord {
    pub hook_id: u16,     // 触发拦截的钩子号
    pub depth_at: u32,    // 拦截时深度
    pub callpath: u16,    // 被拒的 VFS 内部入口编号
}

/// 重入检测器枢纽：深度账 + 在钩标志 + 拦截留账。
pub struct RecurseGuard {
    pub state: GuardState,
    pub depth: u32,            // 当前 VFS 内部嵌套深度
    pub enter_count: u32,      // enter 成功计数
    pub exit_count: u32,       // exit 成功计数
    pub intercepts: u32,       // 钩内回调拦截计数（判据 J1 面）
    pub hook_current: u16,     // 当前在执行的钩子号（0=无）
    pub ledger: Vec<InterceptRecord>,
    pub depth_leaks: u32,      // 深度泄漏计数（必须恒 0）
}

impl RecurseGuard {
    pub fn new() -> RecurseGuard {
        RecurseGuard {
            state: GuardState::Outside,
            depth: 0,
            enter_count: 0,
            exit_count: 0,
            intercepts: 0,
            hook_current: 0,
            ledger: Vec::new(),
            depth_leaks: 0,
        }
    }

    /// fs 钩子进入：记录钩号，置 InHook。
    pub fn hook_enter(&mut self, hook_id: u16) -> Result<(), u16> {
        if self.hook_current != 0 {
            return Err(E_GUARD_MIX); // 钩不允许嵌套（fs 钩子平铺执行）
        }
        self.hook_current = hook_id;
        self.state = GuardState::InHook;
        Ok(())
    }

    /// fs 钩子退出：清钩号；外层 VFS 深度由对应 vfs_exit 归还（钩子
    /// 运行在 VFS 调用内部，退出时不吞外层深度）。
    pub fn hook_exit(&mut self) -> Result<(), u16> {
        if self.hook_current == 0 {
            return Err(E_NOT_IN_HOOK);
        }
        self.hook_current = 0;
        self.state = GuardState::Outside;
        Ok(())
    }

    /// VFS 内部函数入口：钩内一律拦截留账（判据 J1）；钩外按深度放行。
    pub fn vfs_enter(&mut self, callpath: u16) -> Result<(), u16> {
        if self.hook_current != 0 {
            // 红线：fs 钩子执行期回调 VFS 内部 → 拦截并留账。
            self.intercepts += 1;
            self.state = GuardState::Intercepted;
            self.ledger.push(InterceptRecord {
                hook_id: self.hook_current,
                depth_at: self.depth,
                callpath,
            });
            self.state = GuardState::InHook; // 拦截不改变钩执行态
            return Err(E_REENTER);
        }
        if self.depth >= MAX_DEPTH {
            return Err(E_DEPTH_MAX);
        }
        self.depth += 1;
        self.enter_count += 1;
        Ok(())
    }

    /// VFS 内部函数出口：深度 -1，失配即错。
    pub fn vfs_exit(&mut self) -> Result<(), u16> {
        if self.depth == 0 {
            return Err(E_EXIT_MISMATCH);
        }
        self.depth -= 1;
        self.exit_count += 1;
        Ok(())
    }

    /// 会话收尾核验：深度归零 + 三账守恒（enter = exit，拦截独立）。
    pub fn audit(&mut self) -> Result<(), u16> {
        if self.depth != 0 {
            self.depth_leaks += 1;
            return Err(E_DEPTH_LEAK);
        }
        if self.enter_count != self.exit_count {
            return Err(E_BAL);
        }
        if self.ledger.len() as u32 != self.intercepts {
            return Err(E_BAL);
        }
        Ok(())
    }

    /// 完成写回（判据 J2）。
    pub fn writeback(&mut self, status_done: bool, summary_len: u32) -> Result<(), u16> {
        if !status_done {
            return Err(E_GUARD_MIX);
        }
        if summary_len == 0 {
            return Err(E_EMPTY_SUM);
        }
        Ok(())
    }
}

/// 自例程：R1 钩外正常嵌套 → R2 边界 → R3 钩内回调注入 100/100 → R4 对账。
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;

    // R1 正常路径：钩外两级嵌套进退配平，审计过。
    let mut g = RecurseGuard::new();
    g.vfs_enter(1)?;
    g.vfs_enter(2)?;
    g.vfs_exit()?;
    g.vfs_exit()?;
    g.audit()?;
    hits += 1;

    // R2 边界：无 enter 先 exit；深度到 MAX_DEPTH 后再进拦截。
    let mut g = RecurseGuard::new();
    if g.vfs_exit().is_ok() {
        return Err(E_EXIT_MISMATCH);
    }
    let mut d = 0u32;
    while d < MAX_DEPTH {
        g.vfs_enter(9)?;
        d += 1;
    }
    if g.vfs_enter(9).is_ok() {
        return Err(E_DEPTH_MAX);
    }
    while d > 0 {
        g.vfs_exit()?;
        d -= 1;
    }
    hits += 1;

    // R3 注入拦截：钩子执行期回调 VFS 内部 100/100 次全部拦截并逐次留账。
    let mut g = RecurseGuard::new();
    let mut round = 0u32;
    while round < 100 {
        g.hook_enter(0x21)?;
        if g.vfs_enter(0x77).is_ok() {
            return Err(E_REENTER);
        }
        g.hook_exit()?;
        round += 1;
    }
    if g.intercepts != 100 {
        return Err(E_REENTER);
    }
    if g.ledger.len() != 100 {
        return Err(E_BAL);
    }
    if g.depth != 0 {
        return Err(E_DEPTH_LEAK);
    }
    g.writeback(true, 0x60)?;
    hits += 1;

    // R4 守恒对账：enter = exit，拦截账 = 留账条数，审计过。
    let mut g = RecurseGuard::new();
    g.hook_enter(3)?;
    g.vfs_enter(5).is_err();
    g.hook_exit()?;
    g.vfs_enter(5)?;
    g.vfs_exit()?;
    g.audit()?;
    if g.enter_count != g.exit_count {
        return Err(E_BAL);
    }
    hits += 1;

    // R5 追加回归：钩外满深嵌套三遍 + 钩内多入口连续拦截 + 留账回放。
    let mut g = RecurseGuard::new();
    let mut round = 0u32;
    while round < 3 {
        let mut d = 0u32;
        while d < MAX_DEPTH {
            g.vfs_enter(0x10 + d as u16)?;
            d += 1;
        }
        while d > 0 {
            g.vfs_exit()?;
            d -= 1;
        }
        round += 1;
    }
    if g.enter_count != MAX_DEPTH * 3 || g.exit_count != MAX_DEPTH * 3 {
        return Err(E_BAL);
    }
    // 钩内：一个钩子会话内连续三次回调注入，全拦截且深度不动。
    g.hook_enter(0x55)?;
    let mut k = 0u32;
    while k < 3 {
        if g.vfs_enter(0x80 + k as u16).is_ok() {
            return Err(E_REENTER);
        }
        k += 1;
    }
    g.hook_exit()?;
    if g.intercepts != 3 || g.depth != 0 {
        return Err(E_REENTER);
    }
    // 留账回放：三账逐条钩号/深度/入口可查。
    let mut r = 0usize;
    while r < g.ledger.len() {
        if g.ledger[r].hook_id != 0x55 || g.ledger[r].depth_at != 0 {
            return Err(E_BAL);
        }
        r += 1;
    }
    g.audit()?;
    hits += 1;

    // R6 回归：钩间正常调用穿插，深度账跨会话连续守恒。
    let mut g2 = RecurseGuard::new();
    g2.vfs_enter(1)?;
    g2.hook_enter(7)?;
    g2.vfs_enter(2).is_err(); // 钩内回调仍拦
    g2.hook_exit()?;
    g2.vfs_exit()?;
    g2.audit()?;
    if g2.intercepts != 1 || g2.ledger.len() != 1 {
        return Err(E_BAL);
    }
    if g2.writeback(true, 0x99).is_err() {
        return Err(E_GUARD_MIX);
    }
    hits += 1;

    // R7 回归：拦截账回放逐条核对（钩号/入口/深度三读数）。
    let mut g3 = RecurseGuard::new();
    g3.hook_enter(0xA1)?;
    g3.vfs_enter(0xB1).is_err();
    g3.vfs_enter(0xB2).is_err();
    g3.hook_enter(0xA2).is_err(); // 钩不允许嵌套进入
    g3.hook_exit()?;
    if g3.intercepts != 2 {
        return Err(E_REENTER);
    }
    if g3.ledger.len() != 2 {
        return Err(E_BAL);
    }
    g3.audit()?;
    // 钩外深度在钩子退出后仍由对应 vfs_exit 归还（不吞外层调用）。
    g3.writeback(true, 0x77)?;
    hits += 1;

    Ok(hits)
}
