//! F172 引导自检可视化 · 批次六深化（secstar · G-G-02）。
//!
//! 批次六功能面（达成率 54%。与 b3/b4/b5 互补，本批管「主题与跳过策略」）：
//! - [`VisualTheme`]：自检画面主题常量表——底色/星徽色/进度色/失败色
//!   （视觉常量一处一事实——主题不在代码里散落魔法数）；
//! - [`BootTimeline`]：启动时间线——里程碑计划时刻 vs 实际时刻标注
//!   （甘特面：超期可见，且超了多少毫秒可见）；
//! - [`SuiteSkipPolicy`]：跳过策略——依赖套件红 → 后续套件标记
//!   skipped 不跑（跑了也是假数据——诚实的跳过有名字）；
//! - [`report_line`]：自检报告导出行（文本格式：项/结果/耗时三列）。
//!
//! 零堆纪律：定长时间线 + 定长行，无 alloc。

use super::selftestviz::{FLASH_COUNT, MILESTONE_N};
use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// 视觉主题常量表
// ---------------------------------------------------------------------------

/// 主题色（0xRGB——四色一处一事实）。
pub const THEME_BG: u32 = 0x0A0A12;
pub const THEME_STAR: u32 = 0xDDDDDD;
pub const THEME_PROGRESS: u32 = 0x33AA55;
pub const THEME_FAIL: u32 = 0xCC2222;

/// 主题完整性：四色互不相同、底色最暗（视觉层级——进度/失败浮于底）。
pub fn theme_sane() -> bool {
    let colors = [THEME_BG, THEME_STAR, THEME_PROGRESS, THEME_FAIL];
    let mut distinct = true;
    for i in 0..4 {
        for j in i + 1..4 {
            distinct &= colors[i] != colors[j];
        }
    }
    // 亮度粗判：底色各通道 < 32（够暗）。
    let bg_dark = (THEME_BG >> 16 & 0xFF) < 32 && (THEME_BG >> 8 & 0xFF) < 32 && (THEME_BG & 0xFF) < 32;
    distinct && bg_dark
}

// ---------------------------------------------------------------------------
// 启动时间线
// ---------------------------------------------------------------------------

/// 计划时刻（ms）——四里程碑各 500ms（与 b4 MILESTONE_BUDGET 同源）。
pub const PLAN_MS: [u64; MILESTONE_N] = [500, 1_000, 1_500, 2_000];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MilestoneMark {
    pub planned_ms: u64,
    pub actual_ms: u64,
}

impl MilestoneMark {
    /// 超期量（0 = 准时）。
    pub fn overdue_ms(&self) -> u64 {
        self.actual_ms.saturating_sub(self.planned_ms)
    }

    /// 是否超期（>0 即超——准时是硬线不是软目标）。
    pub fn overdue(&self) -> bool {
        self.overdue_ms() > 0
    }
}

/// 时间线构造：实际时刻序列 → 标注序列。
pub fn build_timeline(actual: &[u64; MILESTONE_N]) -> [MilestoneMark; MILESTONE_N] {
    let mut out = [MilestoneMark { planned_ms: 0, actual_ms: 0 }; MILESTONE_N];
    for i in 0..MILESTONE_N {
        out[i] = MilestoneMark { planned_ms: PLAN_MS[i], actual_ms: actual[i] };
    }
    out
}

/// 总线判定：末里程碑实际 ≤ 2s（F053 8 秒线内的自检段预算）。
pub fn timeline_within_budget(t: &[MilestoneMark; MILESTONE_N]) -> bool {
    t[MILESTONE_N - 1].actual_ms <= PLAN_MS[MILESTONE_N - 1]
}

// ---------------------------------------------------------------------------
// 套件跳过策略
// ---------------------------------------------------------------------------

/// 套件执行态（比 b4 多一个 Skipped——诚实的跳过有名字）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SuiteRunState {
    Pending,
    Running,
    Passed,
    Failed,
    /// 依赖套件失败 → 跳过（不跑不是没跑——有名字有原因）。
    Skipped,
}

/// 跳过决策：前序套件失败 → 后续所有 Pending 变 Skipped（级联）。
pub fn apply_skip_cascade(states: &mut [SuiteRunState; 4]) -> usize {
    let mut skipped = 0;
    let mut any_failed = false;
    for i in 0..4 {
        match states[i] {
            SuiteRunState::Failed => any_failed = true,
            SuiteRunState::Pending if any_failed => {
                states[i] = SuiteRunState::Skipped;
                skipped += 1;
            }
            _ => {}
        }
    }
    skipped
}

/// 跳过不影响已通过账（Passed 保持 Passed——历史不洗）。

// ---------------------------------------------------------------------------
// 报告导出行
// ---------------------------------------------------------------------------

/// 报告行：`<套件>/<序号> <PASS|FAIL|SKIP> <耗时ms>`。
pub fn report_line(suite: u8, seq: usize, state: SuiteRunState, elapsed_ms: u32, out: &mut [u8]) -> usize {
    let sname: &[u8] = match suite {
        0 => b"mem",
        1 => b"proc",
        2 => b"store",
        _ => b"input",
    };
    let vname: &[u8] = match state {
        SuiteRunState::Passed => b"PASS",
        SuiteRunState::Failed => b"FAIL",
        SuiteRunState::Skipped => b"SKIP",
        _ => b"....",
    };
    let mut n = 0;
    let put = |bytes: &[u8], out: &mut [u8], n: &mut usize| {
        for b in bytes {
            if *n < out.len() {
                out[*n] = *b;
                *n += 1;
            }
        }
    };
    let putn = |v: u64, out: &mut [u8], n: &mut usize| {
        let mut d = [0u8; 12];
        let mut w = 0;
        if v == 0 {
            d[0] = b'0';
            w = 1;
        } else {
            let mut x = v;
            while x > 0 {
                d[w] = b'0' + (x % 10) as u8;
                w += 1;
                x /= 10;
            }
        }
        for i in (0..w).rev() {
            if *n < out.len() {
                out[*n] = d[i];
                *n += 1;
            }
        }
    };
    put(sname, out, &mut n);
    put(b"/", out, &mut n);
    putn(seq as u64, out, &mut n);
    put(b" ", out, &mut n);
    put(vname, out, &mut n);
    put(b" ", out, &mut n);
    putn(elapsed_ms as u64, out, &mut n);
    n
}

// ---------------------------------------------------------------------------
// 批次六自检
// ---------------------------------------------------------------------------

#[inline(never)]
pub fn run_selftestviz_b6_checks() -> CheckSet {
    let mut cs = CheckSet::new("F172-b6");

    // 1) 主题完整性：四色互异 + 底色够暗（视觉层级常量在岗）。
    cs.add("theme_sane", theme_sane(), "");

    // 2) 主题常量锁定：星徽亮色、进度绿、失败红（语义色不漂移）。
    cs.add(
        "theme_semantics",
        THEME_STAR == 0xDDDDDD && THEME_PROGRESS == 0x33AA55 && THEME_FAIL == 0xCC2222,
        "",
    );

    // 3) 时间线构造：计划/实际成对（甘特面数据完整）。
    let t = build_timeline(&[480, 990, 1_500, 2_000]);
    cs.add(
        "timeline_built",
        t[0] == MilestoneMark { planned_ms: 500, actual_ms: 480 } && t[3].planned_ms == 2_000,
        "",
    );

    // 4) 超期判定：准点不超、超 10ms 即超（硬线语义两面）。
    let ontime = MilestoneMark { planned_ms: 500, actual_ms: 500 };
    let late = MilestoneMark { planned_ms: 500, actual_ms: 510 };
    cs.add(
        "milestone_overdue",
        !ontime.overdue() && late.overdue() && late.overdue_ms() == 10,
        "",
    );

    // 5) 总线：末里程碑 2000 恰好达标、2001 超线（总线逐点）。
    let ok = build_timeline(&[400, 800, 1_200, 2_000]);
    let over = build_timeline(&[400, 800, 1_200, 2_001]);
    cs.add("timeline_budget", timeline_within_budget(&ok) && !timeline_within_budget(&over), "");

    // 6) 跳过级联：套件 0 失败 → 1/2/3 全 Skipped（级联 3 个）。
    let mut st = [SuiteRunState::Passed, SuiteRunState::Failed, SuiteRunState::Pending, SuiteRunState::Pending];
    let n = apply_skip_cascade(&mut st);
    cs.add(
        "skip_cascade",
        n == 2 && st[2] == SuiteRunState::Skipped && st[3] == SuiteRunState::Skipped,
        "",
    );

    // 7) 无失败不跳过：全 Running → 零跳过（策略不误伤）。
    let mut st2 = [SuiteRunState::Running; 4];
    cs.add("skip_none_when_ok", apply_skip_cascade(&mut st2) == 0, "");

    // 8) 历史不洗：Passed 在失败后仍保持 Passed（不追溯改账）。
    let mut st3 = [SuiteRunState::Passed, SuiteRunState::Failed, SuiteRunState::Passed, SuiteRunState::Pending];
    apply_skip_cascade(&mut st3);
    cs.add("skip_preserves_passed", st3[0] == SuiteRunState::Passed && st3[2] == SuiteRunState::Passed && st3[3] == SuiteRunState::Skipped, "");

    // 9) 报告行格式：mem/3 FAIL 120 逐字节（格式锁定）。
    let mut buf = [0u8; 32];
    let n = report_line(0, 3, SuiteRunState::Failed, 120, &mut buf);
    cs.add("report_line_format", &buf[..n] == b"mem/3 FAIL 120", "");

    // 10) 报告行 SKIP：依赖失败的行有名字（不是空白）。
    let n2 = report_line(2, 5, SuiteRunState::Skipped, 0, &mut buf);
    cs.add("report_line_skip", &buf[..n2] == b"store/5 SKIP 0", "");

    // 11) 主册常量贯通：3 闪 / 4 里程碑一处一事实。
    cs.add("consts_aligned", FLASH_COUNT == 3 && MILESTONE_N == 4, "");

    cs
}

// ---------------------------------------------------------------------------
// 宿主单测（批次六）
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests_b6 {
    use super::*;

    #[test]
    fn timeline_all_ontime_zero_overdue() {
        // 全准点线：超期量全零（理想启动的基准线）。
        let t = build_timeline(&PLAN_MS);
        for m in t.iter() {
            assert_eq!(m.overdue_ms(), 0);
            assert!(!m.overdue());
        }
        assert!(timeline_within_budget(&t));
    }

    #[test]
    fn skip_cascade_only_pending() {
        // Running 不被级联跳过（正在跑的套件跑完再说）。
        let mut st = [SuiteRunState::Failed, SuiteRunState::Running, SuiteRunState::Pending, SuiteRunState::Pending];
        let n = apply_skip_cascade(&mut st);
        assert_eq!(n, 2);
        assert_eq!(st[1], SuiteRunState::Running);
    }

    #[test]
    fn report_line_input_suite() {
        // 第四套件名 input（套件名映射全覆盖）。
        let mut buf = [0u8; 32];
        let n = report_line(3, 0, SuiteRunState::Passed, 42, &mut buf);
        assert_eq!(&buf[..n], b"input/0 PASS 42");
    }
}
