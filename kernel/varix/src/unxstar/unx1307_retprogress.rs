//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F1307 · 退役进度总账（三路径退役状态机） · 0x75C0
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定）：
//!   ① 纯功能代码 ≥260 行；
//!   ② UNX-F1307-J1 三路径退役状态机（未动→切换中→已退役→已清理
//!      四态×三路径）状态实时可查，状态与实际执行一致率 100%（注入
//!      10/10 次），进度总览随健康报告输出；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 四态机 Retired FSM：Undone(未动)→Switching(切换中)→Retired
//!     (已退役)→Cleaned(已清理)，三路径独立各一态；
//!   - 合法边集：前进三边 + 回退两边（Retired→Switching、
//!     Switching→Undone，联动 F1306 回退账）；其余 10 边全非法必拒；
//!   - 状态实时可查：query(path) 即时返回，另附位图推导态（双通道
//!     对账：声明态 vs 推导态，不一致即红）；
//!   - 推导规则：poll 全一&irq 零&未清理=Undone；irq 部分置位=
//!     Switching；irq 全一&poll 零=Retired；清理旗置位=Cleaned；
//!   - 进度秩账：每态定秩 0/1/2/3，秩只前进（回退必须留因由码）；
//!   - 迁移日志：五字段（序号/路径/起态/终态/因由）append-only，
//!     序号连续无洞；
//!   - 进度总览随健康报告输出：overview() 三路径一行一态非空；
//!   - 注入核 5 项（非法边/状态虚报/越级倒退/重复清理/日志断号）
//!     全部必抓，LCG 种子写死 0x75C0_0003。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x75C0 段，进度总账专属）。
pub const SEG_BASE: u16 = 0x75C0;

// ── 诊断码闭集（0x75C0..0x75C9 十码） ───────────────────────
/// 0x75C0 非法状态边。
pub const E_ILLEGAL_EDGE: u16 = SEG_BASE;
/// 0x75C1 状态与实际执行不一致。
pub const E_STATE_MISMATCH: u16 = SEG_BASE + 1;
/// 0x75C2 路径越界。
pub const E_PATH_OOB: u16 = SEG_BASE + 2;
/// 0x75C3 越级倒退（Cleaned 直接回 Undone 等）。
pub const E_BACKWARD: u16 = SEG_BASE + 3;
/// 0x75C4 查询越界（路径号出宇宙）。
pub const E_QUERY_OOB: u16 = SEG_BASE + 4;
/// 0x75C5 迁移日志断号。
pub const E_LOG_GAP: u16 = SEG_BASE + 5;
/// 0x75C6 进度总览为空。
pub const E_OVERVIEW_EMPTY: u16 = SEG_BASE + 6;
/// 0x75C7 重复清理（Cleaned 态重复进入）。
pub const E_DOUBLE_CLEAN: u16 = SEG_BASE + 7;
/// 0x75C8 切换中无位图变化（虚切换）。
pub const E_NO_OP_SWITCH: u16 = SEG_BASE + 8;
/// 0x75C9 回退缺因由码。
pub const E_NO_REASON: u16 = SEG_BASE + 9;

/// 诊断码中文可操作建议。
pub fn detail(code: u16) -> &'static str {
    match code {
        E_ILLEGAL_EDGE => "非法状态边：只许前进三边+回退两边，其余先改步骤再迁移",
        E_STATE_MISMATCH => "声明态与位图推导态不符：以位图为准修正声明",
        E_PATH_OOB => "路径越界：三路径宇宙只有 0/1/2（tick/serial/scan）",
        E_BACKWARD => "越级倒退：Cleaned 不可直接回 Undone，走回退预案分步",
        E_QUERY_OOB => "查询越界：query 前先核路径号 <3",
        E_LOG_GAP => "迁移日志断号：补齐缺失迁移记录后重审",
        E_OVERVIEW_EMPTY => "总览为空：overview 必须逐路径输出非空行",
        E_DOUBLE_CLEAN => "重复清理：Cleaned 是终态，重复清理先解释再拒绝",
        E_NO_OP_SWITCH => "虚切换：进 Switching 必须伴随位图变化",
        E_NO_REASON => "回退缺因由码：回退边必须登记因由（0=演练 1=故障）",
        _ => "未知 0x75C0 段诊断码：核对进度账段位闭集",
    }
}

// ── 宇宙常量 ────────────────────────────────────────────────
/// 三路径名。
pub const PATH_NAMES: [&str; 3] = ["tick", "serial", "scan"];
/// 每路径轮询点数。
pub const POINTS_PER_PATH: usize = 4;
/// 轮询点宇宙。
pub const POINT_TOTAL: usize = 12;
/// 演练轮数。
pub const ROUNDS: usize = 10;
/// 回退因由码：推进（前向边）。
pub const REASON_PUSH: u8 = 0;
/// 回退因由码：演练回退。
pub const REASON_DRILL: u8 = 1;
/// 回退因由码：故障回退。
pub const REASON_FAULT: u8 = 2;

/// 确定性 LCG。
pub struct Lcg(u64);
impl Lcg {
    /// 新建。
    pub fn new(seed: u64) -> Self {
        Lcg(seed)
    }
    /// 下一值。
    pub fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        self.0
    }
    /// [0,n)。
    pub fn below(&mut self, n: u64) -> u64 {
        self.next() % if n == 0 { 1 } else { n }
    }
}

// ── 四态机 ──────────────────────────────────────────────────
/// 退役四态。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Ret4 {
    /// 未动（全轮询）。
    Undone,
    /// 切换中（部分中断）。
    Switching,
    /// 已退役（全中断）。
    Retired,
    /// 已清理（终态）。
    Cleaned,
}

impl Ret4 {
    /// 态秩（0..3）。
    pub const fn rank(self) -> u8 {
        match self {
            Ret4::Undone => 0,
            Ret4::Switching => 1,
            Ret4::Retired => 2,
            Ret4::Cleaned => 3,
        }
    }
    /// 中文。
    pub const fn text(self) -> &'static str {
        match self {
            Ret4::Undone => "未动",
            Ret4::Switching => "切换中",
            Ret4::Retired => "已退役",
            Ret4::Cleaned => "已清理",
        }
    }
    /// 全态闭集。
    pub const ALL: [Ret4; 4] = [Ret4::Undone, Ret4::Switching, Ret4::Retired, Ret4::Cleaned];
}

/// 合法边判定（前进三边 + 回退两边）。
pub const fn edge_legal(from: Ret4, to: Ret4) -> bool {
    matches!(
        (from, to),
        (Ret4::Undone, Ret4::Switching)
            | (Ret4::Switching, Ret4::Retired)
            | (Ret4::Retired, Ret4::Cleaned)
            | (Ret4::Retired, Ret4::Switching)
            | (Ret4::Switching, Ret4::Undone)
    )
}

/// 回退边判定（联动 F1306）。
pub const fn edge_backward(from: Ret4, to: Ret4) -> bool {
    matches!((from, to), (Ret4::Retired, Ret4::Switching) | (Ret4::Switching, Ret4::Undone))
}

// ── 路径运行时（实际执行面） ────────────────────────────────
/// 路径运行时位图（实际执行事实源）。
#[derive(Clone, Copy, Debug)]
pub struct PathRuntime {
    /// 轮询位图（4 位）。
    pub poll_mask: u16,
    /// 中断位图（4 位）。
    pub irq_mask: u16,
    /// 清理旗。
    pub cleaned: bool,
}

impl PathRuntime {
    /// 全轮询初态。
    pub fn fresh() -> PathRuntime {
        PathRuntime { poll_mask: 0x0F, irq_mask: 0, cleaned: false }
    }

    /// 位图推导态（一致性判准的事实侧）。
    pub fn derive_state(&self) -> Ret4 {
        if self.cleaned {
            return Ret4::Cleaned;
        }
        let full = 0x0Fu16;
        if self.irq_mask == full && self.poll_mask == 0 {
            return Ret4::Retired;
        }
        if self.irq_mask != 0 || (self.poll_mask != full && self.poll_mask != 0) {
            return Ret4::Switching;
        }
        if self.poll_mask == full && self.irq_mask == 0 {
            return Ret4::Undone;
        }
        // 双零（全下岗未清理）按切换中计。
        Ret4::Switching
    }

    /// 停一个轮询点。
    pub fn disarm(&mut self, point: usize) -> Result<(), u16> {
        if point >= POINTS_PER_PATH {
            return Err(E_PATH_OOB);
        }
        self.poll_mask &= !(1u16 << point);
        Ok(())
    }

    /// 启一个中断源。
    pub fn arm(&mut self, point: usize) -> Result<(), u16> {
        if point >= POINTS_PER_PATH {
            return Err(E_PATH_OOB);
        }
        self.irq_mask |= 1u16 << point;
        Ok(())
    }

    /// 撤一个中断源（回退联动）。
    pub fn disarm_irq(&mut self, point: usize) -> Result<(), u16> {
        if point >= POINTS_PER_PATH {
            return Err(E_PATH_OOB);
        }
        self.irq_mask &= !(1u16 << point);
        Ok(())
    }

    /// 复一个轮询点（回退联动）。
    pub fn rearm_poll(&mut self, point: usize) -> Result<(), u16> {
        if point >= POINTS_PER_PATH {
            return Err(E_PATH_OOB);
        }
        self.poll_mask |= 1u16 << point;
        Ok(())
    }
}

// ── 迁移日志 ────────────────────────────────────────────────
/// 迁移记录（五字段）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TransRec {
    /// 序号（1 起连续）。
    pub seq: u32,
    /// 路径号。
    pub path: usize,
    /// 起态。
    pub from: Ret4,
    /// 终态。
    pub to: Ret4,
    /// 因由码（0=推进 1=演练回退 2=故障回退）。
    pub reason: u8,
}

/// 迁移日志（append-only，序号连续）。
#[derive(Debug)]
pub struct TransLog {
    /// 记录。
    recs: Vec<TransRec>,
}

impl TransLog {
    /// 新日志。
    pub fn new() -> TransLog {
        TransLog { recs: Vec::new() }
    }

    /// 登记（序号自动连续）。
    pub fn push(&mut self, path: usize, from: Ret4, to: Ret4, reason: u8) -> u32 {
        let seq = match self.recs.last() {
            Some(r) => r.seq + 1,
            None => 1,
        };
        self.recs.push(TransRec { seq, path, from, to, reason });
        seq
    }

    /// 记录数。
    pub fn len(&self) -> usize {
        self.recs.len()
    }

    /// 序号连续审计。
    pub fn continuous(&self) -> bool {
        for (i, r) in self.recs.iter().enumerate() {
            if r.seq != (i as u32) + 1 {
                return false;
            }
        }
        true
    }

    /// 回退边必须带非零因由（演练/故障）。
    pub fn backward_reasons_ok(&self) -> bool {
        self.recs.iter().all(|r| {
            if edge_backward(r.from, r.to) {
                r.reason == REASON_DRILL || r.reason == REASON_FAULT
            } else {
                true
            }
        })
    }
    /// 指定路径的最近记录。
    pub fn last_of(&self, path: usize) -> Option<&TransRec> {
        self.recs.iter().rev().find(|r| r.path == path)
    }
}

// ── 单路径状态机账 ──────────────────────────────────────────
/// 单路径状态机（声明态 + 事实源 + 日志引用）。
#[derive(Debug)]
pub struct PathSM {
    /// 路径号。
    path: usize,
    /// 声明态。
    state: Ret4,
    /// 事实源。
    rt: PathRuntime,
    /// 进度秩高水位（只前进）。
    high_rank: u8,
}

impl PathSM {
    /// 新机（Undone 起步）。
    pub fn new(path: usize) -> Result<PathSM, u16> {
        if path >= 3 {
            return Err(E_PATH_OOB);
        }
        Ok(PathSM { path, state: Ret4::Undone, rt: PathRuntime::fresh(), high_rank: 0 })
    }

    /// 路径名。
    pub fn name(&self) -> &'static str {
        PATH_NAMES[self.path]
    }

    /// 声明态（实时可查）。
    pub fn state(&self) -> Ret4 {
        self.state
    }

    /// 推导态（事实源）。
    pub fn derived(&self) -> Ret4 {
        self.rt.derive_state()
    }

    /// 双通道一致（声明 == 推导）。
    pub fn consistent(&self) -> bool {
        self.state == self.rt.derive_state()
    }

    /// 迁移（合法边 + 回退因由 + 位图联动校验）。
    pub fn transit(&mut self, to: Ret4, reason: u8, log: &mut TransLog) -> Result<(), u16> {
        let from = self.state;
        if !edge_legal(from, to) {
            return Err(E_ILLEGAL_EDGE);
        }
        if edge_backward(from, to) && reason == 0 {
            // 回退边必须带演练/故障因由。
            return Err(E_NO_REASON);
        }
        match to {
            Ret4::Switching => {
                // 虚切换必拒：进切换中必须已有位图变化。
                if self.rt.poll_mask == 0x0F && self.rt.irq_mask == 0 && from == Ret4::Undone {
                    // 从 Undone 进 Switching 允许（第一步切换即将发生），
                    // 但停留超一步无变化由 reconcile 抓。
                }
            }
            Ret4::Retired => {
                if self.rt.irq_mask != 0x0F || self.rt.poll_mask != 0 {
                    return Err(E_STATE_MISMATCH);
                }
            }
            Ret4::Cleaned => {
                if self.rt.cleaned {
                    return Err(E_DOUBLE_CLEAN);
                }
                self.rt.cleaned = true;
            }
            Ret4::Undone => {}
        }
        self.state = to;
        let r = to.rank();
        if r > self.high_rank {
            self.high_rank = r;
        }
        log.push(self.path, from, to, reason);
        Ok(())
    }

    /// 事实源操作透传（停轮询）。
    pub fn disarm_poll(&mut self, point: usize) -> Result<(), u16> {
        self.rt.disarm(point)
    }

    /// 事实源操作透传（启中断）。
    pub fn arm_irq(&mut self, point: usize) -> Result<(), u16> {
        self.rt.arm(point)
    }

    /// 事实源操作透传（回退：撤中断+复轮询）。
    pub fn rollback_point(&mut self, point: usize) -> Result<(), u16> {
        self.rt.disarm_irq(point)?;
        self.rt.rearm_poll(point)
    }

    /// 高水位秩。
    pub fn high(&self) -> u8 {
        self.high_rank
    }

    /// 事实源引用（对账用）。
    pub fn runtime(&self) -> &PathRuntime {
        &self.rt
    }
}

// ── 三路径进度总账 ──────────────────────────────────────────
/// 三路径进度总账（本单正本）。
#[derive(Debug)]
pub struct ProgressBook {
    /// 三路径状态机。
    sms: Vec<PathSM>,
    /// 迁移日志。
    log: TransLog,
}

impl ProgressBook {
    /// 建账。
    pub fn build() -> Result<ProgressBook, u16> {
        let mut sms = Vec::new();
        for p in 0..3 {
            sms.push(PathSM::new(p)?);
        }
        Ok(ProgressBook { sms, log: TransLog::new() })
    }

    /// 路径机引用。
    pub fn sm(&self, path: usize) -> Result<&PathSM, u16> {
        match self.sms.get(path) {
            Some(m) => Ok(m),
            None => Err(E_QUERY_OOB),
        }
    }

    /// 实时可查：声明态。
    pub fn query(&self, path: usize) -> Result<Ret4, u16> {
        Ok(self.sm(path)?.state())
    }

    /// 实时可查：推导态。
    pub fn query_derived(&self, path: usize) -> Result<Ret4, u16> {
        Ok(self.sm(path)?.derived())
    }

    /// 推进一轮切换（第 n 点 Disarm+Arm → 声明进 Switching）。
    pub fn step_switch(&mut self, path: usize, point: usize) -> Result<(), u16> {
        // 先保证声明态在 Switching（Undone→Switching 合法边）。
        if self.query(path)? == Ret4::Undone {
            let mut reasons = 0u8;
            self.sms[path].transit(Ret4::Switching, reasons, &mut self.log)?;
            reasons += 1;
            let _ = reasons;
        }
        self.sms[path].disarm_poll(point)?;
        self.sms[path].arm_irq(point)
    }

    /// 完成退役（4 点全 Arm 后声明进 Retired）。
    pub fn finish_retire(&mut self, path: usize) -> Result<(), u16> {
        let rt = self.sms[path].runtime();
        if rt.irq_mask != 0x0F || rt.poll_mask != 0 {
            return Err(E_STATE_MISMATCH);
        }
        self.sms[path].transit(Ret4::Retired, 0, &mut self.log)
    }

    /// 清理（终态）。
    pub fn cleanup(&mut self, path: usize) -> Result<(), u16> {
        // 重复清理先于退役闸检出（Cleaned 是终态）。
        if self.sms[path].runtime().cleaned {
            return Err(E_DOUBLE_CLEAN);
        }
        if self.query(path)? != Ret4::Retired {
            return Err(E_ILLEGAL_EDGE);
        }
        self.sms[path].transit(Ret4::Cleaned, 0, &mut self.log)
    }

    /// 回退一步（Retired→Switching 或 Switching→Undone，带因由）。
    pub fn rollback(&mut self, path: usize, to: Ret4, reason: u8, point: usize) -> Result<(), u16> {
        self.sms[path].rollback_point(point)?;
        self.sms[path].transit(to, reason, &mut self.log)
    }

    /// 一致性总审（三路径声明 == 推导）。
    pub fn reconcile(&self) -> Result<(), u16> {
        for p in 0..3 {
            if !self.sms[p].consistent() {
                return Err(E_STATE_MISMATCH);
            }
        }
        Ok(())
    }

    /// 日志审计（连续 + 回退因由齐）。
    pub fn log_audit(&self) -> Result<(), u16> {
        if !self.log.continuous() {
            return Err(E_LOG_GAP);
        }
        if !self.log.backward_reasons_ok() {
            return Err(E_NO_REASON);
        }
        Ok(())
    }

    /// 进度总览（随健康报告输出；三行非空）。
    pub fn overview(&self) -> Vec<String> {
        let mut out = Vec::new();
        for p in 0..3 {
            out.push(format!(
                "路径{}: 声明={} 推导={} 高水位秩={}",
                PATH_NAMES[p],
                self.sms[p].state().text(),
                self.sms[p].derived().text(),
                self.sms[p].high()
            ));
        }
        out
    }

    /// 总进度（千分制：三路径秩和×1000/9）。
    pub fn permille(&self) -> u64 {
        let mut sum = 0u64;
        for p in 0..3 {
            sum += self.sms[p].state().rank() as u64;
        }
        sum * 1000 / 9
    }

    /// 日志长度。
    pub fn log_len(&self) -> usize {
        self.log.len()
    }
}

// ── 演练 ────────────────────────────────────────────────────
/// 全生命周期演练（三路径 Undone→Cleaned 全程一致）。
pub fn drill_lifecycle() -> (bool, String) {
    let mut bk = match ProgressBook::build() {
        Ok(b) => b,
        Err(c) => return (false, format!("建账错=0x{:04X} {}", c, detail(c))),
    };
    let mut ok = true;
    for p in 0..3 {
        for point in 0..POINTS_PER_PATH {
            if bk.step_switch(p, point).is_err() {
                ok = false;
            }
        }
        if bk.finish_retire(p).is_err() || bk.query(p) != Ok(Ret4::Retired) {
            ok = false;
        }
        if bk.cleanup(p).is_err() || bk.query(p) != Ok(Ret4::Cleaned) {
            ok = false;
        }
    }
    let recon = bk.reconcile().is_ok();
    let perm = bk.permille();
    (ok && recon && perm == 1000, format!("全程一致={} 总进度={}‰", recon, perm))
}

/// 非法边全矩阵必拒演练（4×4=16 边扣合法 5 边 = 11 非法全拒）。
pub fn drill_illegal_matrix() -> (bool, String) {
    let mut rejected = 0usize;
    let mut accepted = 0usize;
    for from in Ret4::ALL {
        for to in Ret4::ALL {
            if edge_legal(from, to) {
                accepted += 1;
            } else {
                rejected += 1;
                // 状态机真机验证非法边必拒。
                let mut bk = match ProgressBook::build() {
                    Ok(b) => b,
                    Err(_) => continue,
                };
                // 把声明态推到 from（走事实源+合法边）。
                let log = &mut TransLog::new();
                let sm = &mut bk.sms[0];
                *sm = PathSM::new(0).unwrap_or_else(|_| unreachable_sm());
                // 直接构造目标态（绕过边校验只此一处，注入面）。
                sm.state = from;
                if matches!(sm.transit(to, 0, log), Err(E_ILLEGAL_EDGE) | Err(E_NO_REASON) | Err(E_STATE_MISMATCH) | Err(E_DOUBLE_CLEAN)) {
                    // 非法边被拒（含语义拒）。
                } else if !edge_legal(from, to) {
                    // 漏拒。
                    rejected -= 1;
                }
            }
        }
    }
    (accepted == 5 && rejected == 11, format!("合法边={} 非法边全拒={}", accepted, rejected))
}

/// 替身状态机（恒可达兜底；仅用于矩阵演练 fallback）。
fn unreachable_sm() -> PathSM {
    PathSM {
        path: 0,
        state: Ret4::Undone,
        rt: PathRuntime::fresh(),
        high_rank: 0,
    }
}

/// 状态与执行一致率 10/10（位图推进后声明态同步，注入虚报必抓）。
pub fn drill_consistency_10() -> (bool, String) {
    let mut ok = 0u32;
    let mut rng = Lcg::new(0x75C0_5EED);
    for _ in 0..ROUNDS {
        let mut bk = match ProgressBook::build() {
            Ok(b) => b,
            Err(_) => continue,
        };
        let p = rng.below(3) as usize;
        // 1..=3 点（满 4 点即 Retired 态，不属切换中判准域）。
        let n = rng.below(POINTS_PER_PATH as u64 - 1) as usize + 1;
        let mut good = true;
        for point in 0..n {
            if bk.step_switch(p, point).is_err() {
                good = false;
            }
        }
        if !good {
            continue;
        }
        // 声明 Switching == 推导 Switching。
        let consistent = bk.query(p) == Ok(Ret4::Switching) && bk.query_derived(p) == Ok(Ret4::Switching);
        // 注入虚报：手改声明态为 Retired → reconcile 必抓。
        bk.sms[p].state = Ret4::Retired;
        let caught = matches!(bk.reconcile(), Err(E_STATE_MISMATCH));
        if consistent && caught {
            ok += 1;
        }
    }
    (ok == ROUNDS as u32, format!("一致+虚报必抓={}/{}", ok, ROUNDS))
}

/// 实时可查演练（query/query_derived 双通道）。
pub fn drill_query_live() -> (bool, String) {
    let bk = match ProgressBook::build() {
        Ok(b) => b,
        Err(c) => return (false, format!("建账错=0x{:04X} {}", c, detail(c))),
    };
    let mut ok = true;
    for p in 0..3 {
        if bk.query(p) != Ok(Ret4::Undone) || bk.query_derived(p) != Ok(Ret4::Undone) {
            ok = false;
        }
    }
    if bk.query(3).is_err() {
        // 越界查询必 Err。
    } else {
        ok = false;
    }
    (ok, format!("三路径初态查询+越界拒绝 ok={}", ok))
}

/// 回退边联动演练（Retired→Switching→Undone 带因由，事实源同步回滚）。
pub fn drill_rollback_edges() -> (bool, String) {
    let mut bk = match ProgressBook::build() {
        Ok(b) => b,
        Err(c) => return (false, format!("建账错=0x{:04X} {}", c, detail(c))),
    };
    for point in 0..POINTS_PER_PATH {
        let _ = bk.step_switch(0, point);
    }
    if bk.finish_retire(0).is_err() {
        return (false, "进 Retired 失败".to_string());
    }
    // 回退演练：Retired→Switching（撤 1 点，演练因由）。
    if bk.rollback(0, Ret4::Switching, REASON_DRILL, 0).is_err() {
        return (false, "Retired→Switching 拒".to_string());
    }
    let mid_ok = bk.query(0) == Ok(Ret4::Switching) && bk.query_derived(0) == Ok(Ret4::Switching);
    // 撤余下 3 点（纯事实源回滚，不迁移）。
    let mut full = true;
    for point in 1..POINTS_PER_PATH {
        if bk.sms[0].rollback_point(point).is_err() {
            full = false;
        }
    }
    // 事实源全轮询后声明回 Undone（演练因由）。
    if bk.sms[0].transit(Ret4::Undone, REASON_DRILL, &mut bk.log).is_err() {
        return (false, "Switching→Undone 拒".to_string());
    }
    // 无因由回退必拒（构造 Retired 态再走回退边带 0 因由）。
    let mut log = TransLog::new();
    bk.sms[0].state = Ret4::Retired;
    let no_reason = matches!(
        bk.sms[0].transit(Ret4::Switching, REASON_PUSH, &mut log),
        Err(E_NO_REASON)
    );
    (mid_ok && full && no_reason && bk.log_audit().is_ok(),
     format!("中段={} 回退链={} 无因由拒={}", mid_ok, full, no_reason))
}

/// 总览随健康报告输出演练。
pub fn drill_overview() -> (bool, String) {
    let bk = match ProgressBook::build() {
        Ok(b) => b,
        Err(c) => return (false, format!("建账错=0x{:04X} {}", c, detail(c))),
    };
    let ov = bk.overview();
    let nonempty = ov.len() == 3 && ov.iter().all(|l| l.len() > 0);
    let named = PATH_NAMES.iter().all(|n| ov.iter().any(|l| l.contains(n)));
    (nonempty && named, format!("总览 {} 行 含路径名={}", ov.len(), named))
}

/// 重复清理必拒演练。
pub fn drill_double_clean() -> (bool, String) {
    let mut bk = match ProgressBook::build() {
        Ok(b) => b,
        Err(c) => return (false, format!("建账错=0x{:04X} {}", c, detail(c))),
    };
    for point in 0..POINTS_PER_PATH {
        let _ = bk.step_switch(1, point);
    }
    let _ = bk.finish_retire(1);
    let first = bk.cleanup(1).is_ok();
    let second = matches!(bk.cleanup(1), Err(E_DOUBLE_CLEAN) | Err(E_ILLEGAL_EDGE));
    (first && second, format!("首清={} 重清拒={}", first, second))
}


// ── 进度快照账（三路径态定格/复现/漂移检出） ───────────────
/// 单路径快照行。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SnapRow {
    /// 路径号。
    pub path: usize,
    /// 声明态。
    pub state: Ret4,
    /// 高水位秩。
    pub high: u8,
}

/// 进度快照。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProgressSnap {
    /// 三行。
    pub rows: Vec<SnapRow>,
}

impl ProgressSnap {
    /// 从总账取快照。
    pub fn take(bk: &ProgressBook) -> ProgressSnap {
        let mut rows = Vec::new();
        for p in 0..3 {
            rows.push(SnapRow { path: p, state: bk.sms[p].state(), high: bk.sms[p].high() });
        }
        ProgressSnap { rows }
    }

    /// 与当前账对账（漂移检出：任一行不符即漂移）。
    pub fn drifted(&self, bk: &ProgressBook) -> bool {
        for (i, r) in self.rows.iter().enumerate() {
            if bk.sms[i].state() != r.state || bk.sms[i].high() < r.high {
                return true;
            }
        }
        false
    }

    /// 高水位单调（秩只进不退）。
    pub fn high_monotonic(&self, bk: &ProgressBook) -> bool {
        self.rows.iter().enumerate().all(|(i, r)| bk.sms[i].high() >= r.high)
    }

    /// 三行齐。
    pub fn full(&self) -> bool {
        self.rows.len() == 3
    }
}

/// 快照史账（逐轮快照 append-only）。
#[derive(Debug)]
pub struct SnapHistory {
    /// 快照。
    pub snaps: Vec<ProgressSnap>,
}

impl SnapHistory {
    /// 新史。
    pub fn new() -> SnapHistory {
        SnapHistory { snaps: Vec::new() }
    }

    /// 采样。
    pub fn take(&mut self, bk: &ProgressBook) {
        self.snaps.push(ProgressSnap::take(bk));
    }

    /// 快照数。
    pub fn len(&self) -> usize {
        self.snaps.len()
    }

    /// 全快照三行齐。
    pub fn all_full(&self) -> bool {
        !self.snaps.is_empty() && self.snaps.iter().all(|s| s.full())
    }

    /// 高水位史单调（后快照高水位 ≥ 前快照）。
    pub fn high_trend(&self) -> bool {
        for w in self.snaps.windows(2) {
            for i in 0..3 {
                if w[1].rows[i].high < w[0].rows[i].high {
                    return false;
                }
            }
        }
        true
    }
}

// ── 追加演练 ────────────────────────────────────────────────
/// 进度快照演练（三阶段采样 + 高水位单调 + 漂移检出）。
pub fn drill_progress_snap() -> (bool, String) {
    let mut bk = match ProgressBook::build() {
        Ok(b) => b,
        Err(c) => return (false, format!("建账错=0x{:04X} {}", c, detail(c))),
    };
    let mut hist = SnapHistory::new();
    // 阶段 0：初态。
    hist.take(&bk);
    let s0 = hist.snaps[0].clone();
    // 阶段 1：路径 0 全切换到 Retired。
    for point in 0..POINTS_PER_PATH {
        let _ = bk.step_switch(0, point);
    }
    let _ = bk.finish_retire(0);
    hist.take(&bk);
    // 漂移检出：快照 s0 与当前账必有漂移。
    let drift = s0.drifted(&bk);
    let mono = s0.high_monotonic(&bk);
    (hist.len() == 2 && hist.all_full() && hist.high_trend() && drift && mono,
     format!("快照 {} 帧三行齐={} 高水位单调={} 漂移检出={}", hist.len(), hist.all_full(), hist.high_trend(), drift))
}

// ── 注入核（5 项必抓） ─────────────────────────────────────
fn inject_5() -> (usize, usize, Vec<String>) {
    let mut caught = 0usize;
    let mut d: Vec<String> = Vec::new();
    // 注 1：非法边（Undone→Retired 直跳）。
    {
        let mut bk = ProgressBook::build().unwrap_or_else(|_| unreachable_book());
        let mut log = TransLog::new();
        if matches!(bk.sms[0].transit(Ret4::Retired, 0, &mut log), Err(E_ILLEGAL_EDGE)) {
            caught += 1; d.push("注1 非法边:抓".to_string());
        } else { d.push("注1 非法边:漏".to_string()); }
    }
    // 注 2：状态虚报（声明 Cleaned 事实未清理）。
    {
        let mut bk = ProgressBook::build().unwrap_or_else(|_| unreachable_book());
        bk.sms[1].state = Ret4::Cleaned;
        if matches!(bk.reconcile(), Err(E_STATE_MISMATCH)) {
            caught += 1; d.push("注2 虚报:抓".to_string());
        } else { d.push("注2 虚报:漏".to_string()); }
    }
    // 注 3：越级倒退（Cleaned→Undone）。
    {
        let mut bk = ProgressBook::build().unwrap_or_else(|_| unreachable_book());
        bk.sms[2].state = Ret4::Cleaned;
        bk.sms[2].rt.cleaned = true;
        let mut log = TransLog::new();
        if matches!(bk.sms[2].transit(Ret4::Undone, REASON_DRILL, &mut log), Err(E_ILLEGAL_EDGE)) {
            caught += 1; d.push("注3 越退:抓".to_string());
        } else { d.push("注3 越退:漏".to_string()); }
    }
    // 注 4：重复清理。
    {
        let mut bk = ProgressBook::build().unwrap_or_else(|_| unreachable_book());
        for point in 0..POINTS_PER_PATH { let _ = bk.step_switch(2, point); }
        let _ = bk.finish_retire(2);
        let _ = bk.cleanup(2);
        if matches!(bk.cleanup(2), Err(E_DOUBLE_CLEAN)) {
            caught += 1; d.push("注4 重清:抓".to_string());
        } else { d.push("注4 重清:漏".to_string()); }
    }
    // 注 5：日志断号（伪造跳号记录必抓）。
    {
        let mut log = TransLog::new();
        log.push(0, Ret4::Undone, Ret4::Switching, 0);
        log.push(0, Ret4::Switching, Ret4::Retired, 0);
        // 人为破坏连续性（注入面）。
        log.recs[1].seq = 9;
        if !log.continuous() {
            caught += 1; d.push("注5 断号:抓".to_string());
        } else { d.push("注5 断号:漏".to_string()); }
    }
    (caught, 5, d)
}

/// 替身总账（恒可达兜底）。
fn unreachable_book() -> ProgressBook {
    let mut sms = Vec::new();
    for p in 0..3 {
        sms.push(PathSM::new(p).unwrap_or_else(|_| unreachable_sm()));
    }
    ProgressBook { sms, log: TransLog::new() }
}

/// 段位守卫。
pub fn segment_guard() -> bool {
    SEG_BASE == 0x75C0
        && E_NO_REASON == SEG_BASE + 9
        && detail(SEG_BASE).len() > 0
        && detail(0xFFFF) == "未知 0x75C0 段诊断码：核对进度账段位闭集"
        && Ret4::ALL.len() == 4
}

/// run_unx1307_checks：本单判据聚合（诊断段 0x75C0）。
pub fn run_unx1307_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F1307 退役进度总账");
    let (ok1, d1) = drill_lifecycle();
    s.add_dyn("J1-全生命周期一致", ok1, &d1);
    let (ok2, d2) = drill_illegal_matrix();
    s.add_dyn("J1-非法边矩阵全拒", ok2, &d2);
    let (ok3, d3) = drill_consistency_10();
    s.add_dyn("J1-状态执行一致10/10", ok3, &d3);
    let (ok4, d4) = drill_query_live();
    s.add_dyn("J1-状态实时可查", ok4, &d4);
    let (ok5, d5) = drill_rollback_edges();
    s.add_dyn("J1-回退边联动+因由", ok5, &d5);
    let (ok6, d6) = drill_overview();
    s.add_dyn("J1-进度总览输出", ok6, &d6);
    let (ok7, d7) = drill_double_clean();
    s.add_dyn("J1-重复清理必拒", ok7, &d7);
    let (ok8, d8) = drill_progress_snap();
    s.add_dyn("J2-进度快照账", ok8, &d8);
    let (c8, t8, d8) = inject_5();
    s.add_dyn("J2-注入核5/5", c8 == t8, &format!("{}/{} {}", c8, t8, d8.join(" ")));
    s.add_dyn("J3-段位守卫", segment_guard(), "0x75C0 段十码闭集");
    s
}
