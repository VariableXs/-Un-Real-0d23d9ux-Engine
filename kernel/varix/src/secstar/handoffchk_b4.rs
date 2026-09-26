//! F181 交接预检器 · 批次四深化（secstar · G-G-11）。
//!
//! 批次四功能面（与批次三互补：批次三管「调度与账本」，本批管
//! 「面板节奏与快照编码」）：
//! - [`PanelFlow`]：面板全流程状态机——Idle→Probing→Holding(全绿停
//!   500ms)→Proceed / Blocked(红停) / 越权三段（置灰→武装→通行）——
//!   每条路有名字有出口（交互状态机第 3 章的闸门面）；
//! - [`Snapshot6Codec`]：三查快照 6bit 编解码——三态×三查打包/解包
//!   round-trip（审计留痕的可逆编码面）；
//! - [`ProbeCache`]：探测结果缓存——同会话同目标不重复探测（预算
//!   纪律的缓存面：350ms 不花在重复工作上）；
//! - [`hold_progress`]：全绿停留进度条——500ms 停留的 ‰ 进度
//!   （确认感的可视化：不是卡住，是在确认）。
//!
//! 零堆纪律：定长状态机 + 定长缓存，无 alloc。

use super::handoffchk::{ALLGREEN_HOLD_MS, CHECKS_BUDGET_MS, CheckState, OVERRIDE_GRAY_MS};
use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// 面板全流程状态机
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PanelPhase {
    Idle,
    Probing,
    /// 全绿停留（500ms 倒计时——确认感）。
    Holding,
    /// 放行完成。
    Proceeded,
    /// 红态拦停（终态——除非重新开始预检）。
    Blocked,
    /// 越权三段：置灰 3s。
    OverrideGrayed,
    /// 越权武装（灰满后可按确认）。
    OverrideArmed,
    /// 越权通行完成（审计已留痕）。
    OverrideProceeded,
}

pub struct PanelFlow {
    pub phase: PanelPhase,
    hold_elapsed_ms: u64,
    gray_elapsed_ms: u64,
}

impl PanelFlow {
    pub const fn new() -> PanelFlow {
        PanelFlow { phase: PanelPhase::Idle, hold_elapsed_ms: 0, gray_elapsed_ms: 0 }
    }

    pub fn start_probing(&mut self) -> bool {
        if matches!(self.phase, PanelPhase::Idle | PanelPhase::Blocked | PanelPhase::Proceeded | PanelPhase::OverrideProceeded) {
            self.phase = PanelPhase::Probing;
            self.hold_elapsed_ms = 0;
            self.gray_elapsed_ms = 0;
            true
        } else {
            false
        }
    }

    /// 探测完成 → 三态入面板（全绿进停留、异常/红进拦停）。
    pub fn probes_done(&mut self, all_green: bool, any_bad: bool) -> bool {
        if self.phase != PanelPhase::Probing {
            return false;
        }
        self.phase = if all_green {
            PanelPhase::Holding
        } else if any_bad {
            PanelPhase::Blocked
        } else {
            PanelPhase::Holding
        };
        true
    }

    /// 停留计时一拍：满 500ms 自动放行。
    pub fn tick(&mut self, dt_ms: u64) -> bool {
        match self.phase {
            PanelPhase::Holding => {
                self.hold_elapsed_ms += dt_ms;
                if self.hold_elapsed_ms >= ALLGREEN_HOLD_MS {
                    self.phase = PanelPhase::Proceeded;
                    return true;
                }
            }
            PanelPhase::OverrideGrayed => {
                self.gray_elapsed_ms += dt_ms;
                if self.gray_elapsed_ms >= OVERRIDE_GRAY_MS {
                    self.phase = PanelPhase::OverrideArmed;
                }
            }
            _ => {}
        }
        false
    }

    /// 红态下按「仍要继续」：Blocked → 越权置灰（审计将留痕）。
    pub fn override_press(&mut self) -> bool {
        if self.phase == PanelPhase::Blocked {
            self.phase = PanelPhase::OverrideGrayed;
            self.gray_elapsed_ms = 0;
            true
        } else {
            false
        }
    }

    /// 越权确认：仅武装态可通行（置灰期按 = 无效——防手滑）。
    pub fn override_confirm(&mut self) -> bool {
        if self.phase == PanelPhase::OverrideArmed {
            self.phase = PanelPhase::OverrideProceeded;
            true
        } else {
            false
        }
    }

    /// 取消越权（Esc = 安全出路——回到 Blocked 等修复）。
    pub fn override_cancel(&mut self) -> bool {
        if matches!(self.phase, PanelPhase::OverrideGrayed | PanelPhase::OverrideArmed) {
            self.phase = PanelPhase::Blocked;
            self.gray_elapsed_ms = 0;
            true
        } else {
            false
        }
    }
}

/// 全绿停留进度 ‰（确认感的可视化面）。
pub fn hold_progress(elapsed_ms: u64) -> u32 {
    (elapsed_ms.min(ALLGREEN_HOLD_MS) * 1_000 / ALLGREEN_HOLD_MS) as u32
}

// ---------------------------------------------------------------------------
// 三查快照 6bit 编解码
// ---------------------------------------------------------------------------

/// 三态 2bit：Green=0 Red=1 Exception=2。
fn state2bits(s: CheckState) -> u8 {
    match s {
        CheckState::Green => 0,
        CheckState::Red => 1,
        CheckState::Exception => 2,
    }
}

fn bits2state(v: u8) -> Option<CheckState> {
    match v {
        0 => Some(CheckState::Green),
        1 => Some(CheckState::Red),
        2 => Some(CheckState::Exception),
        _ => None,
    }
}

/// 打包：三查 × 2bit = 6bit（低 6 位，位序 = 查序）。
pub fn snapshot6_pack(states: &[CheckState; 3]) -> u8 {
    let mut v = 0u8;
    for (i, s) in states.iter().enumerate() {
        v |= state2bits(*s) << (i * 2);
    }
    v
}

/// 解包：非法 2bit 组合（3）→ None（编码面可逆且有校验）。
pub fn snapshot6_unpack(v: u8) -> Option<[CheckState; 3]> {
    let mut out = [CheckState::Green; 3];
    for i in 0..3 {
        out[i] = bits2state((v >> (i * 2)) & 0b11)?;
    }
    Some(out)
}

// ---------------------------------------------------------------------------
// 探测结果缓存
// ---------------------------------------------------------------------------

/// 缓存容量。
pub const PROBE_CACHE_CAP: usize = 8;

/// 探测缓存：目标键（u64——目标指纹）→ 三查结果。
pub struct ProbeCache {
    keys: [Option<u64>; PROBE_CACHE_CAP],
    vals: [[CheckState; 3]; PROBE_CACHE_CAP],
    pub n: usize,
    pub hits: u32,
    pub misses: u32,
}

impl ProbeCache {
    pub const fn new() -> ProbeCache {
        ProbeCache { keys: [const { None }; PROBE_CACHE_CAP], vals: [[CheckState::Green; 3]; PROBE_CACHE_CAP], n: 0, hits: 0, misses: 0 }
    }

    /// 查缓存（命中计数——预算账可见）。
    pub fn get(&mut self, key: u64) -> Option<[CheckState; 3]> {
        for i in 0..self.n {
            if self.keys[i] == Some(key) {
                self.hits += 1;
                return Some(self.vals[i]);
            }
        }
        self.misses += 1;
        None
    }

    /// 放入（满容驱逐最老槽——确定性）。
    pub fn put(&mut self, key: u64, states: [CheckState; 3]) {
        for i in 0..self.n {
            if self.keys[i] == Some(key) {
                self.vals[i] = states;
                return;
            }
        }
        if self.n < PROBE_CACHE_CAP {
            self.keys[self.n] = Some(key);
            self.vals[self.n] = states;
            self.n += 1;
        } else {
            for i in 0..PROBE_CACHE_CAP - 1 {
                self.keys[i] = self.keys[i + 1];
                self.vals[i] = self.vals[i + 1];
            }
            self.keys[PROBE_CACHE_CAP - 1] = Some(key);
            self.vals[PROBE_CACHE_CAP - 1] = states;
        }
    }
}

// ---------------------------------------------------------------------------
// 批次四自检
// ---------------------------------------------------------------------------

#[inline(never)]
pub fn run_handoffchk_b4_checks() -> CheckSet {
    let mut cs = CheckSet::new("F181-b4");

    // 1) 全绿正线：Probing → Holding → 500ms 满自动 Proceed。
    let mut p = PanelFlow::new();
    let s1 = p.start_probing();
    let s2 = p.probes_done(true, false);
    let not_yet = !p.tick(400);
    let done = p.tick(100);
    cs.add(
        "panel_green_path",
        s1 && s2 && not_yet && done && p.phase == PanelPhase::Proceeded,
        "",
    );

    // 2) 停留进度：400ms = 800‰、500ms = 1000‰（确认感可视化）。
    cs.add("hold_progress", hold_progress(400) == 800 && hold_progress(500) == 1_000 && hold_progress(9_999) == 1_000, "");

    // 3) 红停正线：有坏 → Blocked（终态，不能自动过）。
    let mut p2 = PanelFlow::new();
    p2.start_probing();
    p2.probes_done(false, true);
    let stuck = !p2.tick(10_000) && p2.phase == PanelPhase::Blocked;
    cs.add("panel_blocked_stuck", stuck, "");

    // 4) 越权三段：Blocked → 置灰 3s → 武装 → 通行（每段有出口）。
    let mut p3 = PanelFlow::new();
    p3.start_probing();
    p3.probes_done(false, true);
    let press = p3.override_press();
    let early = !p3.override_confirm();
    p3.tick(OVERRIDE_GRAY_MS);
    let armed = p3.phase == PanelPhase::OverrideArmed;
    let proceed = p3.override_confirm();
    cs.add(
        "override_three_steps",
        press && early && armed && proceed && p3.phase == PanelPhase::OverrideProceeded,
        "",
    );

    // 5) 越权取消：Esc 回 Blocked 等修复（取消是安全出路）。
    let mut p4 = PanelFlow::new();
    p4.start_probing();
    p4.probes_done(false, true);
    p4.override_press();
    p4.tick(OVERRIDE_GRAY_MS);
    let cancelled = p4.override_cancel();
    cs.add("override_esc_safe_exit", cancelled && p4.phase == PanelPhase::Blocked && !p4.override_confirm(), "");

    // 6) 重复探测启动拒：Probing 中再 start = 无效（状态机不重入）。
    let mut p5 = PanelFlow::new();
    p5.start_probing();
    cs.add("panel_no_reentry", !p5.start_probing(), "");

    // 7) 快照 6bit round-trip：全 27 组合（3³）编解全保真。
    let mut all = true;
    for a in 0..3u8 {
        for b in 0..3u8 {
            for c in 0..3u8 {
                let st = [
                    bits2state(a).unwrap(),
                    bits2state(b).unwrap(),
                    bits2state(c).unwrap(),
                ];
                all &= snapshot6_unpack(snapshot6_pack(&st)) == Some(st);
            }
        }
    }
    cs.add("snapshot6_roundtrip_27", all, "");

    // 8) 快照非法 2bit 组合拒：值 0b11 → None（编码面有校验）。
    cs.add("snapshot6_invalid_rejected", snapshot6_unpack(0b110000).is_none(), "");

    // 9) 探测缓存：首次 miss、二次 hit（预算纪律面）。
    let mut c = ProbeCache::new();
    let m1 = c.get(0xAA).is_none();
    c.put(0xAA, [CheckState::Green; 3]);
    let h1 = c.get(0xAA);
    cs.add("probe_cache_hit", m1 && h1 == Some([CheckState::Green; 3]) && c.hits == 1 && c.misses == 1, "");

    // 10) 缓存满容驱逐最老：8 满后第 9 个挤掉最老键（确定性）。
    let mut c2 = ProbeCache::new();
    for k in 0..PROBE_CACHE_CAP as u64 {
        c2.put(100 + k, [CheckState::Green; 3]);
    }
    c2.put(200, [CheckState::Red; 3]);
    cs.add(
        "probe_cache_evict_oldest",
        c2.get(100).is_none() && c2.get(200) == Some([CheckState::Red; 3]) && c2.n == PROBE_CACHE_CAP,
        "",
    );

    // 11) 主册常量贯通：停留 500ms / 置灰 3s / 预算 350ms 一处一事实。
    cs.add(
        "consts_aligned",
        ALLGREEN_HOLD_MS == 500 && OVERRIDE_GRAY_MS == 3_000 && CHECKS_BUDGET_MS == 350,
        "",
    );

    cs
}

// ---------------------------------------------------------------------------
// 宿主单测（批次四）
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests_b4 {
    use super::*;

    #[test]
    fn panel_full_reset_cycle() {
        // 收口后重开：Proceeded → start_probing 再走全绿（面板可复用）。
        let mut p = PanelFlow::new();
        p.start_probing();
        p.probes_done(true, false);
        p.tick(ALLGREEN_HOLD_MS);
        assert_eq!(p.phase, PanelPhase::Proceeded);
        assert!(p.start_probing());
        p.probes_done(true, false);
        assert!(p.tick(ALLGREEN_HOLD_MS));
        assert_eq!(p.phase, PanelPhase::Proceeded);
    }

    #[test]
    fn snapshot6_all_bits_boundary() {
        // 6bit 全 1 = 三查全 Exception → 可解（位界无死角）。
        let st = [CheckState::Exception; 3];
        let packed = snapshot6_pack(&st);
        assert_eq!(packed, 0b10_10_10);
        assert_eq!(snapshot6_unpack(packed), Some(st));
    }

    #[test]
    fn hold_progress_never_regresses() {
        // 停留进度单调（确认条不倒退）。
        let mut prev = 0;
        for ms in (0..=600u64).step_by(37) {
            let v = hold_progress(ms);
            assert!(v >= prev, "ms={ms}");
            prev = v;
        }
    }
}
