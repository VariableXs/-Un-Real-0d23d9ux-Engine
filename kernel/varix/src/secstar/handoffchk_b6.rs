//! F181 交接预检器 · 批次六深化（secstar · G-G-11）。
//!
//! 批次六功能面（达成率 56%——主攻批次。与 b3/b4/b5 互补，本批管
//! 「会话全链与指纹」）：
//! - [`GateSession`]：完整会话流——Idle→Probing→裁决→停留→放行/
//!   拦停→收口一条线走完（会话级封装：一次交接 = 一个完整故事）；
//! - [`WinEspFingerprint`]：WINESP 指纹账——引导文件三段哈希链
//!   （哈希可信查的执行面：bootmgr→BCD→WINRE 逐段对拍）；
//! - [`RepairWizardFlow`]：修复向导流——红态 → 向导三步 → 复检
//!   （拦停之后不是死路：给修复的路并复验证）；
//! - [`GateMetrics`]：闸门指标——通过率/均探测耗时/越权率三数
//!   （30 日历史的统计消费面）。
//!
//! 零堆纪律：定长状态机 + 定长链，无 alloc。

use super::handoffchk::{ALLGREEN_HOLD_MS, CheckState, CHECKS_BUDGET_MS};
use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// 完整会话流
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SessionPhase {
    Idle,
    Probing,
    /// 全绿停留（500ms）。
    Holding,
    /// 放行完成。
    Done,
    /// 拦停（等待修复或越权）。
    Stopped,
}

pub struct GateSession {
    pub phase: SessionPhase,
    pub states: [CheckState; 3],
    hold_ms: u64,
    pub probe_ms: u32,
}

impl GateSession {
    pub const fn new() -> GateSession {
        GateSession { phase: SessionPhase::Idle, states: [CheckState::Green; 3], hold_ms: 0, probe_ms: 0 }
    }

    /// 探测收口（一次会话只探一次——重探需新会话）。
    pub fn submit_probes(&mut self, states: [CheckState; 3], elapsed_ms: u32) -> bool {
        if self.phase != SessionPhase::Idle {
            return false;
        }
        self.states = states;
        self.probe_ms = elapsed_ms;
        let all_green = states.iter().all(|s| *s == CheckState::Green);
        self.phase = if all_green { SessionPhase::Holding } else { SessionPhase::Stopped };
        true
    }

    /// 停留计时 → 放行。
    pub fn tick(&mut self, dt_ms: u64) -> bool {
        if self.phase != SessionPhase::Holding {
            return false;
        }
        self.hold_ms += dt_ms;
        if self.hold_ms >= ALLGREEN_HOLD_MS {
            self.phase = SessionPhase::Done;
            return true;
        }
        false
    }

    /// 收口：会话结束出结论（Done=放行 / Stopped=拦停——Idle/Probing 中无结论）。
    pub fn verdict(&self) -> Option<bool> {
        match self.phase {
            SessionPhase::Done => Some(true),
            SessionPhase::Stopped => Some(false),
            _ => None,
        }
    }

    /// 会话重开（下一日交接——Idle 化）。
    pub fn reset(&mut self) {
        *self = GateSession::new();
    }
}

// ---------------------------------------------------------------------------
// WINESP 指纹账
// ---------------------------------------------------------------------------

/// 引导文件三段（bootmgr / BCD / WINRE——哈希可信查的覆盖面）。
pub const FINGERPRINT_SEGS: usize = 3;

pub const SEG_NAMES: [&str; 3] = ["bootmgr", "BCD", "WINRE"];

/// 指纹账：每段登记哈希（u64 简化面——真实现走 F024 证书库）。
#[derive(Clone, Copy)]
pub struct WinEspFingerprint {
    hashes: [Option<u64>; FINGERPRINT_SEGS],
}

impl WinEspFingerprint {
    pub const fn new() -> WinEspFingerprint {
        WinEspFingerprint { hashes: [const { None }; FINGERPRINT_SEGS] }
    }

    pub fn set(&mut self, seg: usize, hash: u64) -> bool {
        if seg >= FINGERPRINT_SEGS {
            return false;
        }
        self.hashes[seg] = Some(hash);
        true
    }

    /// 对拍：登记面 vs 现场面逐段比——全等才可信。
    pub fn verify(&self, live: [u64; FINGERPRINT_SEGS]) -> CheckState {
        for (i, h) in self.hashes.iter().enumerate() {
            match h {
                None => return CheckState::Exception, // 登记缺段 = 预检自身异常
                Some(reg) if *reg != live[i] => return CheckState::Red,
                _ => {}
            }
        }
        CheckState::Green
    }

    /// 登记完整度（三段齐才可对拍——不齐时哈希查本来就该异常）。
    pub fn complete(&self) -> bool {
        self.hashes.iter().all(|h| h.is_some())
    }
}

// ---------------------------------------------------------------------------
// 修复向导流
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RepairPhase {
    /// 未进入（全绿无向导）。
    NotNeeded,
    /// 向导第一步：分诊卡展示。
    Triage,
    /// 向导第二步：执行修复动作（如运行引导修复）。
    Acting,
    /// 向导第三步：复检。
    Recheck,
    /// 复检通过 → 收口。
    Healed,
    /// 复检仍坏 → 升级人工。
    NeedsHuman,
}

pub struct RepairWizard {
    pub phase: RepairPhase,
    pub rechecks: u32,
}

impl RepairWizard {
    pub const fn new(bad: bool) -> RepairWizard {
        RepairWizard { phase: if bad { RepairPhase::Triage } else { RepairPhase::NotNeeded }, rechecks: 0 }
    }

    /// 推进一步（每步成功推进；复检结果决定收口）。
    pub fn advance(&mut self, step_ok: bool, recheck_green: bool) -> RepairPhase {
        self.phase = match self.phase {
            RepairPhase::Triage if step_ok => RepairPhase::Acting,
            RepairPhase::Acting => {
                if step_ok {
                    RepairPhase::Recheck
                } else {
                    // 修复动作失败也计入失败（两败升级——不是只有复检算）。
                    self.rechecks += 1;
                    if self.rechecks >= 2 {
                        RepairPhase::NeedsHuman
                    } else {
                        RepairPhase::Acting
                    }
                }
            }
            RepairPhase::Recheck => {
                self.rechecks += 1;
                if recheck_green {
                    RepairPhase::Healed
                } else if self.rechecks >= 2 {
                    RepairPhase::NeedsHuman
                } else {
                    RepairPhase::Acting // 再修一轮
                }
            }
            other => other,
        };
        self.phase
    }
}

// ---------------------------------------------------------------------------
// 闸门指标
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, Default)]
pub struct GateMetrics {
    pub sessions: u32,
    pub allowed: u32,
    pub blocked: u32,
    pub overrides: u32,
    pub probe_ms_total: u32,
}

impl GateMetrics {
    pub fn on_session(&mut self, allowed: bool, overridden: bool, probe_ms: u32) {
        self.sessions += 1;
        self.probe_ms_total += probe_ms;
        if allowed {
            self.allowed += 1;
        } else {
            self.blocked += 1;
        }
        if overridden {
            self.overrides += 1;
        }
    }

    /// 通过率 ‰。
    pub fn pass_permille(&self) -> u32 {
        if self.sessions == 0 {
            return 0;
        }
        (self.allowed * 1_000 / self.sessions) as u32
    }

    /// 均探测耗时（预算纪律的统计面）。
    pub fn mean_probe_ms(&self) -> u32 {
        if self.sessions == 0 {
            return 0;
        }
        self.probe_ms_total / self.sessions
    }

    /// 越权率 ‰（拦停里有多少是用户强闯的）。
    pub fn override_permille(&self) -> u32 {
        if self.blocked == 0 {
            return 0;
        }
        (self.overrides * 1_000 / self.blocked) as u32
    }
}

// ---------------------------------------------------------------------------
// 批次六自检
// ---------------------------------------------------------------------------

#[inline(never)]
pub fn run_handoffchk_b6_checks() -> CheckSet {
    use RepairPhase as RP;
    let mut cs = CheckSet::new("F181-b6");

    // 1) 会话全绿线：Idle→Probing→Holding→Done（一条线走完）。
    let mut s = GateSession::new();
    let sub = s.submit_probes([CheckState::Green; 3], 200);
    let not_yet = !s.tick(300);
    let done = s.tick(200);
    cs.add(
        "session_green_line",
        sub && not_yet && done && s.phase == SessionPhase::Done && s.verdict() == Some(true),
        "",
    );

    // 2) 会话红线：有坏 → 直达 Stopped（无停留）。
    let mut s2 = GateSession::new();
    s2.submit_probes([CheckState::Green, CheckState::Red, CheckState::Green], 150);
    let stuck = !s2.tick(60_000) && s2.verdict() == Some(false);
    cs.add("session_red_stop", stuck, "");

    // 3) 会话重探拒：Probing 中再 submit = 拒（一次一探）。
    let mut s3 = GateSession::new();
    s3.submit_probes([CheckState::Green; 3], 100);
    cs.add("session_no_resubmit", !s3.submit_probes([CheckState::Green; 3], 100), "");

    // 4) 会话重开：Done 后 reset → 再走一轮（每日交接复用）。
    s.reset();
    cs.add("session_reset", s.phase == SessionPhase::Idle && s.verdict().is_none() && s.submit_probes([CheckState::Green; 3], 100), "");

    // 5) 指纹账：三段登记、缺段=Exception（预检自身异常 fail-closed）。
    let mut fp = WinEspFingerprint::new();
    fp.set(0, 0xAA);
    fp.set(1, 0xBB);
    fp.set(2, 0xCC);
    cs.add(
        "fingerprint_verify_green",
        fp.complete() && fp.verify([0xAA, 0xBB, 0xCC]) == CheckState::Green,
        "",
    );

    // 6) 指纹账篡改检出：bootmgr 变了 → Red（哈希可信查的本命）。
    cs.add("fingerprint_tamper_red", fp.verify([0xAA, 0xBB, 0xCD]) == CheckState::Red, "");

    // 7) 指纹账缺登记：少一段 → Exception（fail-closed 不猜）。
    let partial = WinEspFingerprint::new();
    cs.add("fingerprint_missing_exception", !partial.complete() && partial.verify([0; 3]) == CheckState::Exception, "");

    // 8) 修复向导：三步走完复检通过 → Healed（拦停后的路）。
    let mut w = RepairWizard::new(true);
    let s1 = w.advance(true, false);
    let s2 = w.advance(true, false);
    let s3 = w.advance(true, true);
    cs.add(
        "wizard_heal_path",
        s1 == RP::Acting && s2 == RP::Recheck && s3 == RP::Healed,
        "",
    );

    // 9) 修复向导复检失败：再修一轮、两败升级人工（自动修复不是永动机）。
    let mut w2 = RepairWizard::new(true);
    w2.advance(true, false);
    w2.advance(true, false);
    let again = w2.advance(true, false);
    let give_up = w2.advance(false, false);
    cs.add(
        "wizard_escalates",
        again == RP::Acting && give_up == RP::NeedsHuman && w2.rechecks == 2,
        "",
    );

    // 10) 向导无需修复：全绿会话 → NotNeeded（不造无谓向导）。
    cs.add("wizard_not_needed", RepairWizard::new(false).phase == RP::NotNeeded, "");

    // 11) 指标：8 会话 6 过 2 拦 1 越权 → 750‰ / 均耗 / 500‰（三数面）。
    let mut m = GateMetrics::default();
    m.on_session(true, false, 200);
    m.on_session(true, false, 300);
    m.on_session(false, true, 150);
    m.on_session(false, false, 150);
    for _ in 0..4 {
        m.on_session(true, false, 200);
    }
    cs.add(
        "gate_metrics",
        m.pass_permille() == 750 && m.mean_probe_ms() == 200 && m.override_permille() == 500,
        "",
    );

    // 12) 指标空账诚实：0 → 全零（不编造）。
    let e = GateMetrics::default();
    cs.add("gate_metrics_empty", e.pass_permille() == 0 && e.mean_probe_ms() == 0 && e.override_permille() == 0, "");

    // 13) 主册常量贯通：停留 500ms / 预算 350ms 一处一事实。
    cs.add("consts_aligned", ALLGREEN_HOLD_MS == 500 && CHECKS_BUDGET_MS == 350, "");

    cs
}

// ---------------------------------------------------------------------------
// 宿主单测（批次六）
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests_b6 {
    use super::*;

    #[test]
    fn session_exception_also_stops() {
        // 异常态（fail-closed）同样拦停——不只红态。
        let mut s = GateSession::new();
        s.submit_probes([CheckState::Green, CheckState::Green, CheckState::Exception], 100);
        assert_eq!(s.verdict(), Some(false));
    }

    #[test]
    fn fingerprint_segment_bounds() {
        // 段越界诚实拒（三段封闭——bootmgr/BCD/WINRE）。
        let mut fp = WinEspFingerprint::new();
        assert!(!fp.set(3, 0));
        assert!(fp.set(2, 0xCC));
    }

    #[test]
    fn metrics_accumulates() {
        // 长期累计：100 会话均值稳定（统计面不随时间漂移）。
        let mut m = GateMetrics::default();
        for i in 0..100u32 {
            m.on_session(i % 4 != 0, i % 8 == 0, 200);
        }
        assert_eq!(m.sessions, 100);
        assert_eq!(m.pass_permille(), 750);
        assert_eq!(m.mean_probe_ms(), 200);
    }
}
