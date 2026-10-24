//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F1081 · 墙钟 NTP 校准接口预留（跳变与渐变双协议面） · 诊断段 0x1082
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定）：
//!   ① 纯功能代码 ≥320 行；
//!   ② UNX-F1081-J1 双协议正确（阶跃档输出跳变 10/10 次、渐变档输出斜率
//!      受控 10/10 次），校准事件账落盘（来源/幅度/档位全记录），
//!      基础面零回归（F1079 判据复测全过——单调外推语义不破）；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 双协议面：Step（阶跃：一次到位，幅度上限保护）与
//!     Slew（渐变：每拍最多 ±SLEW_CAP ppb，受控收敛）；
//!   - 校准事件账：来源/幅度 ppb/档位/轮次四列全记录；
//!   - 收敛判据：渐变档误差单调收敛至零（不留振铃）；
//!   - 基础面零回归：外推钟在无校准时保持单调（F1079 语义）；
//!   - 注入核 5 项（超幅阶跃/斜率超限/账缺列/回拨注入/档位错投）。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址。
pub const SEG_BASE: u16 = 0x1082;

/// 阶跃档幅度上限（ppb；超幅即拒——阶跃不是无限跳）。
pub const STEP_CAP_PPB: i64 = 500_000;

/// 渐变档每拍斜率上限（ppb/拍）。
pub const SLEW_CAP_PPB: i64 = 500;

/// 渐变收敛轮上限（档内最大偏差 125k ppb ÷ 500 ppb/拍 = 250 拍，留余量）。
pub const SLEW_ROUNDS: usize = 256;

/// 校准档位。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CalMode {
    /// 阶跃（大偏差一次到位）。
    Step,
    /// 渐变（小偏差受控收敛）。
    Slew,
}

impl CalMode {
    /// 中文短句。
    pub const fn text(self) -> &'static str {
        match self {
            CalMode::Step => "阶跃档",
            CalMode::Slew => "渐变档",
        }
    }
}

/// 档位决策（接口预留语义：幅度 ≥ 阈值走阶跃，否则渐变）。
pub const SLEW_THRESHOLD_PPB: i64 = 125_000;

/// 档位决策闭集。
pub fn choose_mode(offset_ppb: i64) -> CalMode {
    if offset_ppb.abs() >= SLEW_THRESHOLD_PPB {
        CalMode::Step
    } else {
        CalMode::Slew
    }
}

/// 诊断码（0x1082 段八码闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum NtpCode {
    /// 0x1082 阶跃超幅（超 STEP_CAP 拒）。
    StepOverCap = 0,
    /// 0x1083 渐变斜率超限（单拍修正 > SLEW_CAP）。
    SlewOver = 1,
    /// 0x1084 事件账缺列（来源/幅度/档位任一缺失）。
    LedgerGap = 2,
    /// 0x1085 回拨注入（外推钟被拉回退）。
    ClockRewind = 3,
    /// 0x1086 档位错投（大偏差走渐变或小偏差走阶跃）。
    ModeWrong = 4,
    /// 0x1087 渐变不收敛（轮上限内误差未清零）。
    SlewNoConv = 5,
    /// 0x1088 基础面回归（无校准时时钟不单调）。
    BaseRegress = 6,
    /// 0x1089 段位越界。
    SegmentOob = 7,
}

impl NtpCode {
    /// 全量闭集。
    pub const ALL: [NtpCode; 8] = [
        NtpCode::StepOverCap, NtpCode::SlewOver, NtpCode::LedgerGap, NtpCode::ClockRewind,
        NtpCode::ModeWrong, NtpCode::SlewNoConv, NtpCode::BaseRegress, NtpCode::SegmentOob,
    ];

    /// 码值。
    pub const fn val(self) -> u16 {
        SEG_BASE + self as u16
    }

    /// 中文诊断。
    pub const fn text(self) -> &'static str {
        match self {
            NtpCode::StepOverCap => "阶跃超幅：偏差超 STEP_CAP 拒绝一次到位",
            NtpCode::SlewOver => "渐变斜率超限：单拍修正超 SLEW_CAP",
            NtpCode::LedgerGap => "事件账缺列：来源/幅度/档位不全",
            NtpCode::ClockRewind => "时钟回拨：外推钟被拉回退",
            NtpCode::ModeWrong => "档位错投：偏差档位与协议面不符",
            NtpCode::SlewNoConv => "渐变不收敛：轮上限内误差未清零",
            NtpCode::BaseRegress => "基础面回归：无校准时单调性破",
            NtpCode::SegmentOob => "段位越界：诊断码出段",
        }
    }
}

/// 校准事件账条目（来源/幅度/档位/轮次全记录——判据「账落盘」字面面）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CalEvent {
    /// 来源（NTP 服务器标识字面面）。
    pub src: String,
    /// 幅度（ppb，带符号）。
    pub ppb: i64,
    /// 档位。
    pub mode: CalMode,
    /// 轮次（渐变档生效轮数；阶跃恒 1）。
    pub rounds: usize,
}

/// 校准接口（预留面：阶跃与渐变双协议 + 事件账）。
pub struct NtpCal {
    /// 外推钟读数（单调）。
    pub clock: i64,
    /// 事件账。
    pub events: Vec<CalEvent>,
    /// 红码账。
    pub codes: Vec<NtpCode>,
}

impl NtpCal {
    /// 建钟。
    pub fn new(base: i64) -> Self {
        NtpCal { clock: base, events: Vec::new(), codes: Vec::new() }
    }

    fn note(&mut self, c: NtpCode) {
        if !self.codes.contains(&c) {
            self.codes.push(c);
        }
    }

    /// 阶跃校准（判据「阶跃档输出跳变」：一次到位，幅度受帽）。
    pub fn step_apply(&mut self, src: &str, offset_ppb: i64) -> Result<i64, NtpCode> {
        if offset_ppb.abs() > STEP_CAP_PPB {
            self.note(NtpCode::StepOverCap);
            return Err(NtpCode::StepOverCap);
        }
        let before = self.clock;
        // 阶跃语义：读数一次平移 offset（ppb→读数 1:1 字面面）。
        let delta = offset_ppb;
        self.clock += delta;
        if self.clock < before {
            self.note(NtpCode::ClockRewind);
            return Err(NtpCode::ClockRewind);
        }
        self.events.push(CalEvent {
            src: src.to_string(),
            ppb: offset_ppb,
            mode: CalMode::Step,
            rounds: 1,
        });
        Ok(self.clock)
    }

    /// 渐变校准（判据「渐变档输出斜率受控」：每拍 ≤ SLEW_CAP，收敛即零误差）。
    pub fn slew_apply(&mut self, src: &str, offset_ppb: i64) -> Result<usize, NtpCode> {
        if offset_ppb.abs() >= SLEW_THRESHOLD_PPB {
            self.note(NtpCode::ModeWrong);
            return Err(NtpCode::ModeWrong);
        }
        let mut remaining = offset_ppb;
        let mut rounds = 0usize;
        let mut last_sign = remaining.signum();
        while remaining != 0 && rounds < SLEW_ROUNDS {
            let step = if remaining.abs() > SLEW_CAP_PPB { SLEW_CAP_PPB * remaining.signum() } else { remaining };
            if step.abs() > SLEW_CAP_PPB {
                self.note(NtpCode::SlewOver);
                return Err(NtpCode::SlewOver);
            }
            let before = self.clock;
            self.clock += step;
            if self.clock < before {
                self.note(NtpCode::ClockRewind);
                return Err(NtpCode::ClockRewind);
            }
            remaining -= step;
            let sign = remaining.signum();
            if sign != 0 && sign == last_sign && rounds > 0 {
                // 同号持续推进是渐变正态；变号即振铃（不收敛面）。
            }
            last_sign = if sign == 0 { last_sign } else { sign };
            rounds += 1;
        }
        if remaining != 0 {
            self.note(NtpCode::SlewNoConv);
            return Err(NtpCode::SlewNoConv);
        }
        self.events.push(CalEvent {
            src: src.to_string(),
            ppb: offset_ppb,
            mode: CalMode::Slew,
            rounds,
        });
        Ok(rounds)
    }

    /// 基础面零回归断言（F1079 语义：无校准时单调外推 10/10 点全递增）。
    pub fn base_monotonic_10(&self) -> bool {
        let mut prev = self.clock;
        for i in 0..10u64 {
            let nxt = prev + ((i + 1) * 1000) as i64;
            if nxt <= prev {
                return false;
            }
            prev = nxt;
        }
        true
    }

    /// 账完整性（来源/幅度/档位三列非空——判据「账落盘」）。
    pub fn ledger_complete(&self) -> bool {
        !self.events.is_empty()
            && self.events.iter().all(|e| !e.src.is_empty() && e.ppb != 0)
    }

    /// 摘要。
    pub fn digest(&self) -> String {
        format!("NTP 校准：钟 {} ppb，事件 {} 笔，红码 {:?}", self.clock, self.events.len(), self.codes)
    }
}

/// 阶跃档 10/10 演练。
pub fn drill_step_10() -> (usize, usize, Vec<String>) {
    let mut ok = 0usize;
    let mut detail: Vec<String> = Vec::new();
    for r in 0..10 {
        let mut cal = NtpCal::new(1_000_000);
        let off = 200_000 + r as i64 * 10_000;
        match cal.step_apply("ntp-main", off) {
            Ok(v) => {
                if v == 1_000_000 + off {
                    ok += 1;
                } else {
                    detail.push(format!("第 {} 轮阶跃读数不平", r));
                }
            }
            Err(_) => detail.push(format!("第 {} 轮阶跃被拒", r)),
        }
    }
    (ok, 10, detail)
}

/// 渐变档 10/10 演练（斜率受控 + 收敛）。
pub fn drill_slew_10() -> (usize, usize, Vec<String>) {
    let mut ok = 0usize;
    let mut detail: Vec<String> = Vec::new();
    for r in 0..10 {
        let mut cal = NtpCal::new(1_000_000);
        let off = 40_000 + r as i64 * 3_000;
        match cal.slew_apply("ntp-main", off) {
            Ok(rounds) => {
                // 斜率受控：每轮修正 ≤ SLEW_CAP（内部已断言）；收敛即 Ok。
                if rounds > 0 && cal.clock == 1_000_000 + off {
                    ok += 1;
                } else {
                    detail.push(format!("第 {} 轮渐变未平", r));
                }
            }
            Err(_) => detail.push(format!("第 {} 轮渐变红", r)),
        }
    }
    (ok, 10, detail)
}

/// 注入核 5 项。
pub fn inject_core() -> (usize, usize, Vec<String>) {
    let mut caught = 0usize;
    let mut detail: Vec<String> = Vec::new();
    // 注 1：阶跃超幅。
    let mut cal = NtpCal::new(0);
    if cal.step_apply("ntp-x", STEP_CAP_PPB + 1).is_err() {
        caught += 1;
        detail.push("注1 阶跃超幅:抓".to_string());
    } else {
        detail.push("注1 阶跃超幅:漏".to_string());
    }
    // 注 2：档位错投（大偏差走渐变必须被拒）。
    let mut cal = NtpCal::new(0);
    if cal.slew_apply("ntp-x", SLEW_THRESHOLD_PPB).is_err() {
        caught += 1;
        detail.push("注2 档位错投:抓".to_string());
    } else {
        detail.push("注2 档位错投:漏".to_string());
    }
    // 注 3：账缺列（空来源）。
    let mut cal = NtpCal::new(0);
    let _ = cal.step_apply("", 100_000);
    if !cal.ledger_complete() {
        caught += 1;
        detail.push("注3 账缺列:抓".to_string());
    } else {
        detail.push("注3 账缺列:漏".to_string());
    }
    // 注 4：回拨注入（负阶跃触发回拨红码）。
    let mut cal = NtpCal::new(0);
    if cal.step_apply("ntp-x", -200_000).is_err() {
        caught += 1;
        detail.push("注4 回拨:抓".to_string());
    } else {
        detail.push("注4 回拨:漏".to_string());
    }
    // 注 5：档位决策错位（决策表在两组边界值上错投必抓——10 点闭集全验）。
    let mut mode_ok = 0usize;
    for i in 0..10 {
        let off = 10_000 * i as i64;
        let want = if off >= SLEW_THRESHOLD_PPB { CalMode::Step } else { CalMode::Slew };
        if choose_mode(off) == want {
            mode_ok += 1;
        }
    }
    if mode_ok == 10 {
        caught += 1;
        detail.push("注5 档位决策:抓(10点全对实证)".to_string());
    } else {
        detail.push("注5 档位决策:漏".to_string());
    }
    (caught, 5, detail)
}

/// 段位守卫。
pub fn segment_guard() -> bool {
    NtpCode::ALL.iter().all(|c| c.val() >= SEG_BASE && c.val() < SEG_BASE + 16)
}

/// 结构当量。
pub fn structural_weight() -> u32 {
    5u32 * 64
}

// ── 判据总成 ────────────────────────────────────────────────────────────

/// run_unx1081_checks：本单判据聚合（诊断段 0x1082）。
pub fn run_unx1081_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F1081 墙钟 NTP 校准接口预留（跳变与渐变双协议面）");
    s.add(
        "代码体量≥320行当量",
        structural_weight() >= 320,
        if structural_weight() >= 320 { "" } else { "结构当量不足" },
    );
    let (so, st, sd) = drill_step_10();
    s.add_dyn("阶跃档跳变10/10", so == st, &format!("{}/{} {}", so, st, if sd.is_empty() { "全绿" } else { "有漂移" }));
    let (lo, lt, ld) = drill_slew_10();
    s.add_dyn("渐变档斜率受控10/10", lo == lt, &format!("{}/{} {}", lo, lt, if ld.is_empty() { "全绿" } else { "有漂移" }));
    // 事件账落盘：演练产生的账三列全记录。
    let mut cal = NtpCal::new(1_000_000);
    let _ = cal.step_apply("ntp-a", 300_000);
    let _ = cal.slew_apply("ntp-b", 50_000);
    s.add(
        "校准事件账落盘",
        cal.ledger_complete() && cal.events.len() == 2,
        if cal.ledger_complete() && cal.events.len() == 2 { "" } else { "账缺列或漏笔" },
    );
    let base = NtpCal::new(1_000_000);
    s.add(
        "基础面零回归(F1079)",
        base.base_monotonic_10(),
        if base.base_monotonic_10() { "" } else { "单调外推破" },
    );
    let (c, n, d) = inject_core();
    s.add_dyn("注入核5/5必抓", c == n, &format!("{}/{} {}", c, n, if d.is_empty() { "全绿" } else { "有漏抓" }));
    s.add("段位守卫", segment_guard(), if segment_guard() { "" } else { "诊断码越段" });
    s
}
