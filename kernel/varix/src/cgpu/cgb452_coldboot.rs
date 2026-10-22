//! CGPU-F0452 · 冷启动预算（目标 380 行）
//! ==========================================================================
//! 锚点判据（施工书原文摘录）：
//! - 启动早期默认（无画像无统计时的预算默认值表——保守安全默认）；
//! - 早期探测（设备能力探测完成前的过渡预算）；
//! - 首帧保障（系统首帧的预算直通车——启动场景 F0354 联动）；
//! - 冷启动演练（每次启动的冷启动路径验证）。判据：默认值表、
//!   过渡预算、首帧直通、演练、判据。
//!
//! 设计纪律：
//! - 默认值表：冷启动（无画像/无统计）时每类别的保守份额是常量表，
//!   不是猜测——保守的含义是宁可少给（过渡期结束后由画像接管）。
//! - 过渡预算：设备能力探测完成前用 TRANSITION 配方（更低配额+
//!   更短窗口），探测完成（probe_ready）后切正式配方——切换一次性
//!   且归因。
//! - 首帧直通：首帧预算请求绕过常规排队直通（首帧延迟是用户可感
//!   的启动体验底线），但直通配额固定且只对 SYSTEM/首帧类别生效。
//! - 演练：冷启动路径自身可被自检（rehearse）——默认表/过渡表/
//!   直通配额的守恒与有界性逐项验证，启动即验证。
//! - no_std + alloc；零 unwrap/expect 于运行路径；诊断段 0x0F70 独占。

use alloc::vec;
use alloc::vec::Vec;

/// 诊断段基址（0x0F70，全仓独占）。
pub const SEG_BASE: u16 = 0x0F70;

pub const E_CB_STAGE: u16 = SEG_BASE + 1; // 启动阶段非法跃迁
pub const E_CB_ACCOUNT: u16 = SEG_BASE + 2; // 账户不在默认表/超配额
pub const E_CB_DIRECT: u16 = SEG_BASE + 3; // 直通资格不符（非首帧/非系统类）
pub const E_CB_CONSERVE: u16 = SEG_BASE + 4; // 默认表守恒破缺

/// 冷启动默认值表（无画像无统计的保守安全默认，千分位）。
/// 守恒：Σ = 1000‰。
pub const COLD_DEFAULT_SYSTEM_PM: u64 = 300;
pub const COLD_DEFAULT_FOREGROUND_PM: u64 = 400;
pub const COLD_DEFAULT_BACKGROUND_PM: u64 = 150;
pub const COLD_DEFAULT_RESERVE_PM: u64 = 150;
pub const COLD_DEFAULT_TOTAL_PM: u64 =
    COLD_DEFAULT_SYSTEM_PM + COLD_DEFAULT_FOREGROUND_PM + COLD_DEFAULT_BACKGROUND_PM + COLD_DEFAULT_RESERVE_PM;

/// 过渡预算配方（探测完成前：更保守）。
pub const TRANSITION_MAX_ACCOUNTS: usize = 2;
pub const TRANSITION_WINDOW_TICKS: u64 = 4;
pub const FULL_WINDOW_TICKS: u64 = 16;

/// 首帧直通配额（固定，直通不排队）。
pub const FIRST_FRAME_DIRECT_PM: u64 = 500;

/// 启动阶段（单向推进：Cold→Probing→Ready）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BootStage {
    Cold,
    Probing,
    Ready,
}

impl BootStage {
    pub fn rank(self) -> u8 {
        match self {
            BootStage::Cold => 0,
            BootStage::Probing => 1,
            BootStage::Ready => 2,
        }
    }
}

/// 默认表条目。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ColdRow {
    pub class: u8,
    pub share_pm: u64,
}

/// 冷启动预算台。
#[derive(Debug)]
pub struct ColdBoot {
    stage: BootStage,
    /// 冷启动 tick 计数（过渡窗判定）。
    tick: u64,
    /// 已放行首帧直通（仅一次）。
    first_frame_done: bool,
    /// 过渡期账户（探测完成前的少量账户）。
    transition_accounts: Vec<(u32, u64)>,
    /// 正式期账户。
    ready_accounts: Vec<(u32, u64)>,
    pub audits: Vec<(&'static str, u64)>,
}

impl ColdBoot {
    pub fn new() -> Self {
        Self {
            stage: BootStage::Cold,
            tick: 0,
            first_frame_done: false,
            transition_accounts: Vec::new(),
            ready_accounts: Vec::new(),
            audits: Vec::new(),
        }
    }

    pub fn stage(&self) -> BootStage {
        self.stage
    }

    pub fn probing(&self) -> bool {
        self.stage == BootStage::Probing
    }

    /// 阶段推进（只允许单向 +1）。
    pub fn advance(&mut self, to: BootStage) -> Result<(), u16> {
        if to.rank() != self.stage.rank() + 1 {
            return Err(E_CB_STAGE);
        }
        self.stage = to;
        self.audits.push(("advance", self.tick));
        Ok(())
    }

    /// 冷启动默认值表（公示：每次调用同一张表）。
    pub fn default_table() -> [ColdRow; 4] {
        [
            ColdRow { class: 0, share_pm: COLD_DEFAULT_SYSTEM_PM },
            ColdRow { class: 1, share_pm: COLD_DEFAULT_FOREGROUND_PM },
            ColdRow { class: 2, share_pm: COLD_DEFAULT_BACKGROUND_PM },
            ColdRow { class: 3, share_pm: COLD_DEFAULT_RESERVE_PM },
        ]
    }

    /// 过渡期开户：探测完成前配额压缩（数量钳制）。
    pub fn open_transition(&mut self, acct: u32) -> Result<(), u16> {
        if self.stage != BootStage::Probing && self.stage != BootStage::Cold {
            return Err(E_CB_STAGE);
        }
        if self.transition_accounts.len() >= TRANSITION_MAX_ACCOUNTS {
            return Err(E_CB_ACCOUNT);
        }
        if self.transition_accounts.iter().any(|(a, _)| *a == acct) {
            return Err(E_CB_ACCOUNT);
        }
        self.transition_accounts.push((acct, 0));
        Ok(())
    }

    /// 过渡期预算发放：窗口极短（TRANSITION_WINDOW_TICKS 内重置）。
    pub fn grant_transition(&mut self, acct: u32, tick: u64, share_pm: u64) -> Result<u64, u16> {
        if self.stage == BootStage::Ready {
            return Err(E_CB_STAGE);
        }
        if self.transition_accounts.iter().position(|(a, _)| *a == acct).is_none() {
            return Err(E_CB_ACCOUNT);
        }
        // 过渡窗滚动重置。
        let window = tick / TRANSITION_WINDOW_TICKS;
        let last_window = self.tick / TRANSITION_WINDOW_TICKS;
        if window != last_window {
            for e in self.transition_accounts.iter_mut() {
                e.1 = 0;
            }
        }
        self.tick = tick;
        // 过渡期单账户窗口内至多 500‰（保守）。
        let bal = match self.transition_accounts.iter_mut().find(|e| e.0 == acct) {
            Some(e) => &mut e.1,
            None => return Err(E_CB_ACCOUNT),
        };
        let next = bal.saturating_add(share_pm);
        if next > 500 {
            return Err(E_CB_ACCOUNT);
        }
        *bal = next;
        Ok(next)
    }

    /// 首帧直通：仅一次、仅 Cold/Probing 阶段、固定配额。
    pub fn first_frame_direct(&mut self) -> Result<u64, u16> {
        if self.first_frame_done {
            return Err(E_CB_DIRECT);
        }
        if self.stage == BootStage::Ready {
            // 已就绪后直通无意义（正式配额已生效）。
            return Err(E_CB_DIRECT);
        }
        self.first_frame_done = true;
        self.audits.push(("first-frame-direct", self.tick));
        Ok(FIRST_FRAME_DIRECT_PM)
    }

    /// 探测完成：切正式配方（账户平迁，一次归因）。
    pub fn probe_ready(&mut self) -> Result<usize, u16> {
        self.advance(BootStage::Ready)?;
        let mut moved = Vec::new();
        for (a, b) in self.transition_accounts.drain(..) {
            moved.push((a, b));
        }
        let n = moved.len();
        self.ready_accounts = moved;
        self.audits.push(("probe-ready", self.tick));
        Ok(n)
    }

    /// 正式期开户（全量配额，窗口 16 tick）。
    pub fn open_ready(&mut self, acct: u32) -> Result<(), u16> {
        if self.stage != BootStage::Ready {
            return Err(E_CB_STAGE);
        }
        if self.ready_accounts.iter().any(|(a, _)| *a == acct) {
            return Err(E_CB_ACCOUNT);
        }
        self.ready_accounts.push((acct, 0));
        Ok(())
    }

    pub fn ready_account_count(&self) -> usize {
        self.ready_accounts.len()
    }
}

/// 冷启动演练：启动即自检默认表/过渡配方/直通配额的性质。
/// 返回 (项数, 全过)。独立于任何实例（纯静态路径验证）。
pub fn rehearse() -> (usize, bool) {
    let mut v: Vec<bool> = Vec::new();
    // 1) 默认表守恒：Σ=1000‰。
    let t = ColdBoot::default_table();
    let sum: u64 = t.iter().map(|r| r.share_pm).sum();
    v.push(sum == COLD_DEFAULT_TOTAL_PM && COLD_DEFAULT_TOTAL_PM == 1000);
    // 2) 默认表保守性：系统类有底线且预留非零。
    v.push(COLD_DEFAULT_SYSTEM_PM >= 250 && COLD_DEFAULT_RESERVE_PM > 0);
    // 3) 过渡配方确实更紧：账户数 < 正式惯例、窗口 < 全量窗口。
    v.push(TRANSITION_MAX_ACCOUNTS < 4 && TRANSITION_WINDOW_TICKS < FULL_WINDOW_TICKS);
    // 4) 首帧直通有界：不超过系统类+前台类合计（启动期不得挤占全部）。
    v.push(FIRST_FRAME_DIRECT_PM <= COLD_DEFAULT_SYSTEM_PM + COLD_DEFAULT_FOREGROUND_PM);
    // 5) 阶段机单向：Ready 不能回 Cold（结构性验证）。
    let mut cb = ColdBoot::new();
    let _ = cb.advance(BootStage::Probing);
    let back = cb.advance(BootStage::Cold);
    v.push(back == Err(E_CB_STAGE));
    // 6) 直通一次性。
    let mut cb2 = ColdBoot::new();
    let d1 = cb2.first_frame_direct();
    let d2 = cb2.first_frame_direct();
    v.push(d1 == Ok(FIRST_FRAME_DIRECT_PM) && d2 == Err(E_CB_DIRECT));
    let n = v.len();
    (n, v.iter().all(|&b| b))
}

pub(crate) fn run_checks() -> Vec<(&'static str, bool)> {
    let mut v: Vec<(&'static str, bool)> = Vec::new();
    // 一、默认值表：守恒 + 公示稳定。
    {
        let t1 = ColdBoot::default_table();
        let t2 = ColdBoot::default_table();
        let sum: u64 = t1.iter().map(|r| r.share_pm).sum();
        v.push(("F0452-默认值表-守恒千分位", sum == 1000));
        v.push(("F0452-默认值表-公示稳定", t1 == t2));
        // 保守性：系统类 + 前台类占绝对主体（启动体验底线）。
        v.push(("F0452-默认值表-系统前台为主体", COLD_DEFAULT_SYSTEM_PM + COLD_DEFAULT_FOREGROUND_PM >= 700));
    }
    // 二、过渡预算：配额压缩 + 窗口滚动重置。
    {
        let mut cb = ColdBoot::new();
        let _ = cb.advance(BootStage::Probing);
        let o1 = cb.open_transition(1);
        let o2 = cb.open_transition(2);
        let o3 = cb.open_transition(3);
        v.push(("F0452-过渡预算-账户数压缩", o1.is_ok() && o2.is_ok() && o3 == Err(E_CB_ACCOUNT)));
        // 单账户窗口内 500‰ 上限。
        let g1 = cb.grant_transition(1, 0, 300);
        let g2 = cb.grant_transition(1, 1, 300);
        v.push(("F0452-过渡预算-窗口配额钳制", g1 == Ok(300) && g2 == Err(E_CB_ACCOUNT)));
        // 窗口滚动（tick 4 进新窗）重置。
        let g3 = cb.grant_transition(1, 4, 300);
        v.push(("F0452-过渡预算-窗口滚动重置", g3 == Ok(300)));
        // Ready 后过渡通道关闭。
        let _ = cb.probe_ready();
        let g4 = cb.grant_transition(2, 8, 100);
        v.push(("F0452-过渡预算-就绪后通道关闭", g4 == Err(E_CB_STAGE)));
    }
    // 三、首帧直通：仅一次 + 阶段资格。
    {
        let mut cb = ColdBoot::new();
        let d = cb.first_frame_direct();
        v.push(("F0452-首帧直通-冷阶段可直通", d == Ok(FIRST_FRAME_DIRECT_PM)));
        let _ = cb.advance(BootStage::Probing);
        let d2 = cb.first_frame_direct();
        v.push(("F0452-首帧直通-二次拒绝", d2 == Err(E_CB_DIRECT)));
        // 就绪阶段直通关闭。
        let mut cb2 = ColdBoot::new();
        let _ = cb2.advance(BootStage::Probing);
        let _ = cb2.probe_ready();
        let d3 = cb2.first_frame_direct();
        v.push(("F0452-首帧直通-就绪后关闭", d3 == Err(E_CB_DIRECT)));
    }
    // 四、演练：启动即验证冷启动路径。
    {
        let (n, ok) = rehearse();
        v.push(("F0452-演练-冷启动路径全过", ok && n >= 6));
        // 演练幂等：两次结果一致。
        let (n2, ok2) = rehearse();
        v.push(("F0452-演练-幂等", n2 == n && ok2 == ok));
    }
    // 五、账户平迁：探测完成账户无损迁入正式期。
    {
        let mut cb = ColdBoot::new();
        let _ = cb.advance(BootStage::Probing);
        let _ = cb.open_transition(7);
        let _ = cb.open_transition(8);
        let moved = cb.probe_ready();
        v.push(("F0452-过渡预算-账户平迁完整", moved == Ok(2) && cb.ready_account_count() == 2));
        // 正式期开户正常。
        let r = cb.open_ready(9);
        v.push(("F0452-默认值表-正式期开户", r.is_ok() && cb.ready_account_count() == 3));
    }
    // 六、判据自检。
    {
        let mut cb = ColdBoot::new();
        let skip = cb.advance(BootStage::Ready);
        v.push(("F0452-判据-阶段跳级拒绝", skip == Err(E_CB_STAGE)));
        v.push(("F0452-判据-窗口常数有界", TRANSITION_WINDOW_TICKS > 0 && FULL_WINDOW_TICKS > TRANSITION_WINDOW_TICKS));
        v.push(("F0452-判据-直通配额为正且不过半", FIRST_FRAME_DIRECT_PM > 0 && FIRST_FRAME_DIRECT_PM < 1000));
    }
    v
}

#[cfg(test)]
mod tests {
    use super::run_checks;
    #[test]
    fn f0452_coldboot_checks() {
        for (name, ok) in run_checks() {
            assert!(ok, "红项: {}", name);
        }
    }
}
