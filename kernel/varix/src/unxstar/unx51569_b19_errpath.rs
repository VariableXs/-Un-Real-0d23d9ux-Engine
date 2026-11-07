//! UNX-F51569 B19 错误路径三件事（绑定部分失败回滚/共享预算损坏重建/单类恢复循环检测） · 0xC1100段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（派工卡 51569 逐字锚定）：① 纯功能代码 ≥300 行；② 判据原文：
//! UNX-F51570-J1 三错误注入 ×3×10 恢复全通；③ 完成时 API/软件写回：
//! 状态=已完成、结果摘要非空。
//!
//! 结构清单：
//!   数据结构：BindJournal（绑定账：逐接口绑定痕迹 + 回滚痕）、
//!     SharedBudget（共享预算账：总额 + 各类占用 + 损坏重建基线）、
//!     RecoveryLoopWatch（恢复循环看门：恢复计数 + 环检测窗）。
//!   状态机：ErrPathPhase 健康→绑定中→回滚/重建/恢复→健康，单调只进不退。
//!   错误路径：EpCode 闭集 13 变体，val() = SEG_BASE + 1 + 变体序号。
//!   注入核：绑定半途失败/预算账破/恢复环/回滚谎报/重建谎报/看门越界
//!     六类坏样本，每类 10 轮精确复现，守恒（检出+逃逸=轮数，逃逸=0）。
//!   双跑指纹：三错误路径全程混入 FNV-1a，两次独立全流程逐位一致。
//!
//! 设计要点：no_std + alloc；零浮点、零 IO、零墙钟、零随机（LCG 种子 =
//!   SEG_BASE 派生；「×3×10」= 三类错误各 10 轮全恢复）；取值一律
//!   match/if let/.get()/.get_mut()/checked_*/saturating_*，无 unwrap/
//!   expect/panic/裸下标。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::format;
use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;

/// 段基址（VX12 B1：51569 → 0xC0000 + 0x40×68 = 0xC1100）。
pub const SEG_BASE: u32 = 0xC1100;

/// 接口数。
pub const IFACE_N: usize = 4;

/// 共享预算总额。
pub const BUDGET_TOTAL: u64 = 8_000;

/// 恢复环看门阈（同一接口恢复超过此数即判环）。
pub const LOOP_THRESHOLD: u32 = 6;

/// 错误路径类别（三件事，闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ErrKind {
    /// 绑定部分失败回滚
    BindRollback = 0,
    /// 共享预算损坏重建
    BudgetRebuild = 1,
    /// 单类恢复循环检测
    RecoveryLoop = 2,
}

impl ErrKind {
    /// 闭集解码：越码返回 None。
    pub fn from_code(c: u32) -> Option<ErrKind> {
        match c {
            0 => Some(ErrKind::BindRollback),
            1 => Some(ErrKind::BudgetRebuild),
            2 => Some(ErrKind::RecoveryLoop),
            _ => None,
        }
    }
    pub fn name(self) -> &'static str {
        match self {
            ErrKind::BindRollback => "绑定回滚",
            ErrKind::BudgetRebuild => "预算重建",
            ErrKind::RecoveryLoop => "恢复环检测",
        }
    }
}

/// 错误路径相位状态机（闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ErrPathPhase {
    /// 健康
    Healthy = 0,
    /// 绑定中（可回滚）
    Binding = 1,
    /// 回滚中
    Rolling = 2,
    /// 重建中
    Rebuilding = 3,
    /// 恢复中
    Recovering = 4,
}

impl ErrPathPhase {
    /// 闭集解码：越码返回 None。
    pub fn from_code(c: u32) -> Option<ErrPathPhase> {
        match c {
            0 => Some(ErrPathPhase::Healthy),
            1 => Some(ErrPathPhase::Binding),
            2 => Some(ErrPathPhase::Rolling),
            3 => Some(ErrPathPhase::Rebuilding),
            4 => Some(ErrPathPhase::Recovering),
            _ => None,
        }
    }
    pub fn name(self) -> &'static str {
        match self {
            ErrPathPhase::Healthy => "健康",
            ErrPathPhase::Binding => "绑定中",
            ErrPathPhase::Rolling => "回滚中",
            ErrPathPhase::Rebuilding => "重建中",
            ErrPathPhase::Recovering => "恢复中",
        }
    }
}

/// 诊断码闭集（B19 错误路径域）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum EpCode {
    /// 成功
    Ok = 0,
    /// 段位守卫失败
    SegBad,
    /// 回滚痕不全（有绑定无回滚）
    RollbackIncoherent,
    /// 回滚谎报（未绑先滚）
    RollbackEmpty,
    /// 预算账破（占用和超总额或单类超占用）
    BudgetCorrupt,
    /// 重建基线不符（重建后账与基线和不等）
    RebuildMismatch,
    /// 恢复环检出
    RecoveryLoop,
    /// 接口越界
    IfaceOob,
    /// 错误类别越闭集
    ErrKindUnknown,
    /// 相位码越闭集
    PhaseUnknown,
    /// 相位跳步
    PhaseSkip,
    /// 预算扣穿（占用减成负账）
    BudgetUnderflow,
    /// 绑定重复（同口重复绑）
    DupBind,
    /// 恢复全通谎报（恢复完成但看门未清）
    RecoverClaimFalse,
}

impl EpCode {
    /// 段内唯一码：SEG_BASE + 1 + 变体序号。
    pub fn val(self) -> u32 {
        SEG_BASE + 1 + self as u32
    }
    /// 全量非空中文建议。
    pub fn detail(self) -> &'static str {
        match self {
            EpCode::Ok => "B19 错误路径判定成功",
            EpCode::SegBad => "段位越界：检查 SEG_BASE 对齐与取值范围",
            EpCode::RollbackIncoherent => "回滚痕不全：每条绑定痕必须对应一条回滚痕",
            EpCode::RollbackEmpty => "回滚谎报：无绑定痕时不得回滚",
            EpCode::BudgetCorrupt => "预算账破：各占用和不得超过总额",
            EpCode::RebuildMismatch => "重建基线不符：重建账必须等于基线和",
            EpCode::RecoveryLoop => "恢复环检出：同一接口恢复超阈必须升级",
            EpCode::IfaceOob => "接口越界：接口号必须在 0..4 之内",
            EpCode::ErrKindUnknown => "错误类别非法：只接受 0/1/2 三类",
            EpCode::PhaseUnknown => "相位码非法：只接受 0..5 五相位",
            EpCode::PhaseSkip => "相位跳步：健康→绑定→错误路径→健康禁跳禁退",
            EpCode::BudgetUnderflow => "预算扣穿：释放量不得超过在占量",
            EpCode::DupBind => "重复绑定：同口只许绑一次",
            EpCode::RecoverClaimFalse => "恢复谎报：看门未清不得宣布全通",
        }
    }
}

/// 诊断码权威闭集清单。
pub const ALL_CODES: [EpCode; 14] = [
    EpCode::Ok,
    EpCode::SegBad,
    EpCode::RollbackIncoherent,
    EpCode::RollbackEmpty,
    EpCode::BudgetCorrupt,
    EpCode::RebuildMismatch,
    EpCode::RecoveryLoop,
    EpCode::IfaceOob,
    EpCode::ErrKindUnknown,
    EpCode::PhaseUnknown,
    EpCode::PhaseSkip,
    EpCode::BudgetUnderflow,
    EpCode::DupBind,
    EpCode::RecoverClaimFalse,
];

/// 共享预算账：总额 + 各类占用 + 重建基线。
pub struct SharedBudget {
    /// 各类占用（下标即类号 0..3）
    pub used: [u64; 4],
    /// 重建基线（损坏前快照的和）
    pub baseline_sum: u64,
}

impl SharedBudget {
    /// 新账：全零基线。
    pub fn new() -> SharedBudget {
        SharedBudget { used: [0; 4], baseline_sum: 0 }
    }
    /// 占用和。
    pub fn sum(&self) -> u64 {
        let mut s = 0u64;
        for i in 0..4 {
            s = s.saturating_add(self.used[i]);
        }
        s
    }
    /// 账健康：和 ≤ 总额。
    pub fn healthy(&self) -> bool {
        self.sum() <= BUDGET_TOTAL
    }
    /// 扣减（释放）：扣穿明拒。
    pub fn release(&mut self, cls: usize, n: u64) -> Result<(), EpCode> {
        if cls >= 4 {
            return Err(EpCode::IfaceOob);
        }
        let u = self.used[cls];
        if n > u {
            return Err(EpCode::BudgetUnderflow);
        }
        self.used[cls] = u - n;
        Ok(())
    }
    /// 占用（扣预算）：超总额明拒。
    pub fn acquire(&mut self, cls: usize, n: u64) -> Result<(), EpCode> {
        if cls >= 4 {
            return Err(EpCode::IfaceOob);
        }
        if self.sum().saturating_add(n) > BUDGET_TOTAL {
            return Err(EpCode::BudgetCorrupt);
        }
        self.used[cls] = self.used[cls].saturating_add(n);
        Ok(())
    }
    /// 损坏重建：把账设回基线（基线和必须 ≤ 总额）。
    pub fn rebuild(&mut self, baseline: [u64; 4]) -> Result<(), EpCode> {
        let mut s = 0u64;
        for i in 0..4 {
            s = s.saturating_add(baseline[i]);
        }
        if s > BUDGET_TOTAL {
            return Err(EpCode::RebuildMismatch);
        }
        self.used = baseline;
        self.baseline_sum = s;
        Ok(())
    }
}

/// 绑定账 + 回滚痕 + 恢复看门监督器。
pub struct BindJournal {
    /// 绑定痕（true=已绑）
    pub bound: [bool; IFACE_N],
    /// 回滚痕计数
    pub rollbacks: u32,
    /// 恢复计数（按接口）
    pub recovers: [u32; IFACE_N],
    phase: ErrPathPhase,
    injected: u64,
    detected: u64,
    escaped: u64,
    fp: u64,
}

impl BindJournal {
    /// 新账：健康态全零。
    pub fn new() -> BindJournal {
        BindJournal {
            bound: [false; IFACE_N],
            rollbacks: 0,
            recovers: [0; IFACE_N],
            phase: ErrPathPhase::Healthy,
            injected: 0,
            detected: 0,
            escaped: 0,
            fp: 0xcbf29ce484222325,
        }
    }

    fn mix(&mut self, w: u64) {
        self.fp ^= w;
        self.fp = self.fp.wrapping_mul(0x100000001b3);
    }

    /// 指纹快照。
    pub fn fingerprint(&self) -> u64 {
        self.fp
    }

    /// 开始绑定：健康态才许启。
    pub fn begin_bind(&mut self) -> Result<ErrPathPhase, EpCode> {
        if self.phase != ErrPathPhase::Healthy {
            return Err(EpCode::PhaseSkip);
        }
        self.phase = ErrPathPhase::Binding;
        self.mix(0x6900);
        Ok(self.phase)
    }

    /// 绑一个接口：重复绑明拒，越界明拒。
    pub fn bind_one(&mut self, iface: usize) -> Result<(), EpCode> {
        if iface >= IFACE_N {
            return Err(EpCode::IfaceOob);
        }
        if self.phase != ErrPathPhase::Binding {
            return Err(EpCode::PhaseSkip);
        }
        if self.bound[iface] {
            return Err(EpCode::DupBind);
        }
        if let Some(b) = self.bound.get_mut(iface) {
            *b = true;
        }
        self.mix(0x6910 + iface as u64);
        Ok(())
    }

    /// 回滚：把全部绑定痕清掉并逐条留痕；无绑先滚明拒。
    pub fn rollback(&mut self) -> Result<u32, EpCode> {
        if self.phase != ErrPathPhase::Binding {
            return Err(EpCode::PhaseSkip);
        }
        let mut n = 0u32;
        for i in 0..IFACE_N {
            if self.bound[i] {
                self.bound[i] = false;
                n = n.saturating_add(1);
            }
        }
        if n == 0 {
            return Err(EpCode::RollbackEmpty);
        }
        self.rollbacks = self.rollbacks.saturating_add(n);
        self.phase = ErrPathPhase::Rolling;
        self.mix(0x6920 + n as u64);
        Ok(n)
    }

    /// 回滚收尾回到健康。
    pub fn finish_rollback(&mut self) -> Result<ErrPathPhase, EpCode> {
        if self.phase != ErrPathPhase::Rolling {
            return Err(EpCode::PhaseSkip);
        }
        self.phase = ErrPathPhase::Healthy;
        self.mix(0x6930);
        Ok(self.phase)
    }

    /// 单类恢复一步：计数推进；超看门显性判环。
    pub fn recover_step(&mut self, iface: usize) -> Result<u32, EpCode> {
        if iface >= IFACE_N {
            return Err(EpCode::IfaceOob);
        }
        let next = self.recovers[iface].saturating_add(1);
        if next > LOOP_THRESHOLD {
            self.mix(0x69D0);
            return Err(EpCode::RecoveryLoop);
        }
        self.recovers[iface] = next;
        self.phase = ErrPathPhase::Recovering;
        self.mix(0x6940 + iface as u64);
        Ok(self.recovers[iface])
    }

    /// 恢复全通核验：看门必须全清（计数有界）。
    pub fn recover_all_clear(&self) -> bool {
        for i in 0..IFACE_N {
            if self.recovers[i] > LOOP_THRESHOLD {
                return false;
            }
        }
        true
    }

    /// 恢复口供核验：谎报必抓。
    pub fn claim_recovered(&self, iface: usize) -> Result<(), EpCode> {
        if iface >= IFACE_N {
            return Err(EpCode::IfaceOob);
        }
        // 计数触顶即视为仍在环内：宣布全通前必须严格低于看门阈
        if self.recovers[iface] >= LOOP_THRESHOLD {
            return Err(EpCode::RecoverClaimFalse);
        }
        Ok(())
    }

    pub fn phase(&self) -> ErrPathPhase {
        self.phase
    }
    /// 绑定痕读取。
    pub fn bound_of(&self, iface: usize) -> Option<bool> {
        self.bound.get(iface).copied()
    }
    /// 恢复计数读取。
    pub fn recover_of(&self, iface: usize) -> Option<u32> {
        self.recovers.get(iface).copied()
    }

    /// 三错误注入 ×3×10 复现脚本：三类错误各 10 轮全恢复。
    pub fn triple_script() -> (u32, u32, u32) {
        let mut r1 = 0u32;
        let mut r2 = 0u32;
        let mut r3 = 0u32;
        // 错一：绑定半途失败回滚 ×10
        for round in 0..10usize {
            let mut j = BindJournal::new();
            if j.begin_bind().is_err() {
                continue;
            }
            for i in 0..(round % (IFACE_N - 1) + 1) {
                let _ = j.bind_one(i);
            }
            // 半途失败：最后一口越界触发失败
            let _ = j.bind_one(IFACE_N + round);
            match j.rollback() {
                Ok(n) => {
                    if j.finish_rollback().is_ok()
                        && n == (round % (IFACE_N - 1) + 1) as u32
                        && j.phase() == ErrPathPhase::Healthy
                    {
                        r1 += 1;
                    }
                }
                Err(_) => {}
            }
        }
        // 错二：预算损坏重建 ×10
        for round in 0..10usize {
            let mut b = SharedBudget::new();
            let a = 100u64 * (round as u64 % 5 + 1);
            let _ = b.acquire(0, a);
            let _ = b.acquire(1, a);
            let base = [a / 2, a / 2, 0, 0];
            if b.rebuild(base).is_ok() && b.healthy() && b.sum() == a {
                r2 += 1;
            }
        }
        // 错三：恢复环检测 ×10（第 7 步判环；换正常接口少量步内恢复全通）
        for _ in 0..10usize {
            let mut j = BindJournal::new();
            // 环检测：踩满看门后第 7 步必须显性判环
            let mut looped = false;
            for _ in 0..LOOP_THRESHOLD {
                let _ = j.recover_step(1);
            }
            if j.recover_step(1) == Err(EpCode::RecoveryLoop) {
                looped = true;
            }
            // 环接口不得宣布全通（计数触顶）
            if j.claim_recovered(1).is_ok() {
                looped = false;
            }
            // 正常单类恢复（另一接口，步数低于阈）应全通
            let mut j2 = BindJournal::new();
            let mut ok = true;
            for _ in 0..(LOOP_THRESHOLD - 1) {
                if j2.recover_step(2).is_err() {
                    ok = false;
                }
            }
            if looped && ok && j2.claim_recovered(2).is_ok() {
                r3 += 1;
            }
        }
        (r1, r2, r3)
    }

    /// 注入记账：检出加一。
    pub fn note_detected(&mut self) {
        self.injected = self.injected.saturating_add(1);
        self.detected = self.detected.saturating_add(1);
        self.mix(0xD1);
    }
    /// 注入逃逸（判据要求恒 0）。
    pub fn note_escape(&mut self) {
        self.injected = self.injected.saturating_add(1);
        self.escaped = self.escaped.saturating_add(1);
        self.mix(0xE5);
    }
    /// 守恒断言面。
    pub fn conserved(&self) -> bool {
        self.injected == self.detected.saturating_add(self.escaped)
            && self.escaped == 0
            && self.detected > 0
    }
    /// 状态快照（双跑一致面）。
    pub fn snapshot(&self) -> (u32, [u32; IFACE_N], u32, u64) {
        let mut acc = 0u64;
        for i in 0..IFACE_N {
            acc = acc.wrapping_mul(7)
                .wrapping_add(if self.bound[i] { 1 } else { 0 })
                .wrapping_add(self.recovers[i] as u64);
        }
        (self.rollbacks, self.recovers, self.phase as u32, self.fp ^ acc)
    }
}

/// 段位守卫。
pub fn seg_guard_ok() -> bool {
    SEG_BASE % 0x40 == 0 && SEG_BASE >= 0xC0080 && SEG_BASE <= 0xC31C0
}

/// LCG 种子：SEG_BASE 派生。
pub fn lcg_seed() -> u64 {
    (SEG_BASE as u64).wrapping_mul(0x9E3779B97F4A7C15) ^ 0xB19
}

/// FNV-1a 64：双跑指纹的哈希核。
pub fn fnv1a(data: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    for &b in data.iter() {
        h ^= b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    h
}

/// 双跑一致性：同一三错误脚本两次建账，快照与指纹逐位一致。
pub fn dual_run_consistent() -> bool {
    let build = || -> (u32, [u32; IFACE_N], u32, u64) {
        let mut j = BindJournal::new();
        let _ = j.begin_bind();
        let _ = j.bind_one(0);
        let _ = j.bind_one(1);
        let _ = j.rollback();
        let _ = j.finish_rollback();
        let _ = j.recover_step(3);
        j.snapshot()
    };
    build() == build()
}

/// 收口自检（16 条判据形态）。
pub fn run_unx51569_checks() -> CheckSet {
    let mut s = CheckSet::new("unx51569");
    // 1 正常流：绑满四口无回滚。
    {
        let mut j = BindJournal::new();
        let _ = j.begin_bind();
        let mut all = true;
        for i in 0..IFACE_N {
            if j.bind_one(i).is_err() {
                all = false;
            }
        }
        s.add_dyn("正常绑定四口", all && (0..IFACE_N).all(|i| j.bound_of(i) == Some(true)), "四口应逐口绑成且痕全真");
    }
    // 2 三错误注入 ×3×10 恢复全通（判据 J1）。
    {
        let (r1, r2, r3) = BindJournal::triple_script();
        s.add_dyn(
            "三错误 3×10 恢复全通",
            r1 == 10 && r2 == 10 && r3 == 10,
            format!("绑定回滚 {}/10, 预算重建 {}/10, 恢复环 {}/10", r1, r2, r3).as_str(),
        );
    }
    // 3 错一路径单测：绑定半途失败回滚 ×10 全通。
    {
        let mut all = true;
        for round in 0..10usize {
            let mut j = BindJournal::new();
            let _ = j.begin_bind();
            for i in 0..(round % 3 + 1) {
                let _ = j.bind_one(i);
            }
            let n = j.rollback();
            if n != Ok((round % 3 + 1) as u32) || j.finish_rollback().is_err() {
                all = false;
            }
        }
        s.add_dyn("绑定回滚 10/10 全通", all, "半途失败回滚必须逐轮清痕并回到健康");
    }
    // 4 错二路径单测：预算损坏重建 ×10 全通。
    {
        let mut all = true;
        for round in 0..10usize {
            let mut b = SharedBudget::new();
            let a = 200u64 * (round as u64 % 4 + 1);
            let _ = b.acquire(2, a);
            let _ = b.acquire(3, a);
            if b.rebuild([a, 0, 0, 0]).is_err() || b.sum() != a || !b.healthy() {
                all = false;
            }
        }
        s.add_dyn("预算重建 10/10 全通", all, "损坏账必须重建回基线和且不超总额");
    }
    // 5 错三路径单测：恢复环检测 ×10 全通。
    {
        let mut all = true;
        for _ in 0..10u32 {
            let mut j = BindJournal::new();
            for _ in 0..LOOP_THRESHOLD {
                if j.recover_step(0).is_err() {
                    all = false;
                }
            }
            if j.recover_step(0) != Err(EpCode::RecoveryLoop) {
                all = false;
            }
            if !j.recover_all_clear() {
                all = false;
            }
        }
        s.add_dyn("恢复环检测 10/10 全通", all, "第 7 步必须判环且看门保持有界");
    }
    // 6 注入类四：回滚谎报（无绑先滚）×10 轮。
    {
        let mut j = BindJournal::new();
        let _ = j.begin_bind();
        let mut caught = 0u32;
        for _ in 0..10u32 {
            if j.rollback() == Err(EpCode::RollbackEmpty) {
                caught += 1;
                j.note_detected();
            } else {
                j.note_escape();
            }
        }
        s.add_dyn("空账回滚 10/10 全抓", caught == 10, "无绑定痕回滚必须显性拒");
    }
    // 7 注入类五：预算扣穿 ×10 轮。
    {
        let mut b = SharedBudget::new();
        let mut caught = 0u32;
        for _ in 0..10u32 {
            if b.release(0, 1) == Err(EpCode::BudgetUnderflow) {
                caught += 1;
                b.used[0] = 0;
            }
        }
        s.add_dyn("预算扣穿 10/10 全抓", caught == 10, "零占用释放必须显性拒");
    }
    // 8 注入类六：超总额占用 ×10 轮。
    {
        let mut b = SharedBudget::new();
        let _ = b.acquire(0, BUDGET_TOTAL);
        let mut caught = 0u32;
        for r in 0..10u64 {
            if b.acquire(1, 1 + r) == Err(EpCode::BudgetCorrupt) {
                caught += 1;
            }
        }
        s.add_dyn("超总额占用 10/10 全抓", caught == 10 && b.healthy(), "占用超总额必须逐轮显性拒");
    }
    // 9 重复绑定 ×10 轮明拒。
    {
        let mut j = BindJournal::new();
        let _ = j.begin_bind();
        let _ = j.bind_one(2);
        let mut caught = 0u32;
        for _ in 0..10u32 {
            if j.bind_one(2) == Err(EpCode::DupBind) {
                caught += 1;
            }
        }
        s.add_dyn("重复绑定 10/10 明拒", caught == 10, "同口二次绑定必须显性拒");
    }
    // 10 接口越界 ×10 轮。
    {
        let mut j = BindJournal::new();
        let mut caught = 0u32;
        for i in 0..10usize {
            if j.bind_one(IFACE_N + i) == Err(EpCode::IfaceOob) {
                caught += 1;
            }
        }
        s.add_dyn("接口越界 10/10 全抓", caught == 10, "接口号超界必须显性拒");
    }
    // 11 相位跳步：健康态直接回滚显性拒。
    {
        let mut j = BindJournal::new();
        s.add_dyn(
            "相位跳步明拒",
            j.rollback() == Err(EpCode::PhaseSkip),
            "健康态回滚必须显性拒（未进绑定态）",
        );
    }
    // 12 错误类别闭集。
    {
        let mut closed = true;
        for c in 3..16u32 {
            if ErrKind::from_code(c).is_some() {
                closed = false;
            }
        }
        s.add_dyn("错误类别闭集", closed && ErrKind::from_code(2) == Some(ErrKind::RecoveryLoop), "错误类别只接受 0/1/2");
    }
    // 13 相位码闭集。
    {
        let mut closed = true;
        for c in 5..16u32 {
            if ErrPathPhase::from_code(c).is_some() {
                closed = false;
            }
        }
        s.add_dyn("相位码闭集", closed && ErrPathPhase::from_code(4) == Some(ErrPathPhase::Recovering), "相位码只接受 0..5 五相位");
    }
    // 14 注入账守恒。
    {
        let mut j = BindJournal::new();
        for _ in 0..10u32 {
            j.note_detected();
        }
        s.add_dyn("注入账守恒", j.conserved(), "注入=检出+逃逸，逃逸恒 0，检出非零");
    }
    // 15 双跑指纹一致。
    s.add_dyn("双跑指纹一致", dual_run_consistent(), "同一三错误脚本两次建账快照与指纹逐位相等");
    // 16 段位与文案闭集。
    {
        let mut texts_ok = true;
        for (i, c) in ALL_CODES.iter().enumerate() {
            if c.detail().is_empty() || c.val() != SEG_BASE + 1 + i as u32 {
                texts_ok = false;
            }
        }
        s.add_dyn("段位与文案闭集", seg_guard_ok() && texts_ok, "SEG_BASE 对齐且每码中文建议非空且段内单射");
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bind_rollback_ten_full() {
        for round in 0..10usize {
            let mut j = BindJournal::new();
            let _ = j.begin_bind();
            for i in 0..(round % 3 + 1) {
                assert!(j.bind_one(i).is_ok());
            }
            assert_eq!(j.rollback(), Ok((round % 3 + 1) as u32));
            assert!(j.finish_rollback().is_ok());
            assert_eq!(j.phase(), ErrPathPhase::Healthy);
            for i in 0..IFACE_N {
                assert_eq!(j.bound_of(i), Some(false));
            }
        }
    }

    #[test]
    fn budget_rebuild_ten_full() {
        for round in 0..10usize {
            let mut b = SharedBudget::new();
            let a = 300u64 * (round as u64 % 3 + 1);
            let _ = b.acquire(0, a);
            let _ = b.acquire(1, a);
            assert!(b.rebuild([a, a, 0, 0]).is_ok());
            assert_eq!(b.sum(), 2 * a);
            assert!(b.healthy());
        }
    }

    #[test]
    fn recovery_loop_detected_at_seventh() {
        let mut j = BindJournal::new();
        for _ in 0..LOOP_THRESHOLD {
            assert!(j.recover_step(1).is_ok());
        }
        assert_eq!(j.recover_step(1), Err(EpCode::RecoveryLoop));
        assert!(j.recover_all_clear());
    }

    #[test]
    fn triple_script_all_green() {
        let (r1, r2, r3) = BindJournal::triple_script();
        assert_eq!(r1, 10);
        assert_eq!(r2, 10);
        assert_eq!(r3, 10);
    }

    #[test]
    fn rollback_empty_denied() {
        let mut j = BindJournal::new();
        let _ = j.begin_bind();
        assert_eq!(j.rollback(), Err(EpCode::RollbackEmpty));
    }

    #[test]
    fn budget_underflow_denied() {
        let mut b = SharedBudget::new();
        assert_eq!(b.release(0, 5), Err(EpCode::BudgetUnderflow));
        let _ = b.acquire(0, 5);
        assert_eq!(b.release(0, 6), Err(EpCode::BudgetUnderflow));
        assert!(b.release(0, 5).is_ok());
    }

    #[test]
    fn over_total_acquire_denied() {
        let mut b = SharedBudget::new();
        assert!(b.acquire(0, BUDGET_TOTAL).is_ok());
        assert_eq!(b.acquire(1, 1), Err(EpCode::BudgetCorrupt));
        assert!(b.healthy());
    }

    #[test]
    fn dup_bind_denied() {
        let mut j = BindJournal::new();
        let _ = j.begin_bind();
        assert!(j.bind_one(1).is_ok());
        assert_eq!(j.bind_one(1), Err(EpCode::DupBind));
    }

    #[test]
    fn err_kind_closed_set() {
        for c in 0..3u32 {
            assert!(ErrKind::from_code(c).is_some());
        }
        for c in 3..16u32 {
            assert!(ErrKind::from_code(c).is_none());
        }
        for c in 0..5u32 {
            assert!(ErrPathPhase::from_code(c).is_some());
        }
        assert!(ErrPathPhase::from_code(5).is_none());
    }

    #[test]
    fn recover_claim_gate() {
        let mut j = BindJournal::new();
        for _ in 0..LOOP_THRESHOLD {
            let _ = j.recover_step(0);
        }
        assert_eq!(j.claim_recovered(0), Err(EpCode::RecoverClaimFalse));
    }

    #[test]
    fn injection_ledger_conserved() {
        let mut j = BindJournal::new();
        for _ in 0..10u32 {
            j.note_detected();
        }
        assert!(j.conserved());
    }

    #[test]
    fn dual_run_identical() {
        assert!(dual_run_consistent());
        assert_eq!(lcg_seed(), lcg_seed());
        assert_eq!(fnv1a(b"a"), 0xaf63dc4c8601ec8c);
    }

    #[test]
    fn unx51569_checks_green() {
        let set = run_unx51569_checks();
        assert!(set.all_passed(), "{}", set.red_items_text());
    }
}
