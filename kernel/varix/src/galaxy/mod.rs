//! GALAXY-1800 AI-16~AI-30 落地基座（G901~G1800）。
//!
//! 十四个域，每域三文件，每文件 20 项功能（纯逻辑 + 固定容量数组）：
//! AI-16 `rt` / `clock` / `gpower`，AI-17 `infer` / `prefetch` / `forensics`，
//! AI-18 `gverify` / `selfheal` / `testfw`，AI-19 `codec` / `capture` / `simd`，
//! AI-20 `gfx` / `audio3d` / `stream`，AI-21 `editor` / `debugger` / `replay`，
//! AI-22 `profiler` / `provenance` / `vcs`，AI-23 `teaching` / `docgen` / `i18n`，
//! AI-24 `design` / `theming` / `icons`，AI-25 `wallpaper` / `motion` / `sound`，
//! AI-26 `widgets` / `gestures` / `notify`，AI-27 `settings` / `basicapps` / `inputexp`，
//! AI-28 `fileman` / `terminal` / `launcher`，AI-29 `display` / `chaos` / `longevity`。
//!
//! 纪律：不覆盖既有模块；每文件导出 `run_<x>_checks() -> CheckSet`；
//! `run_galaxy_checks()` 汇总 42 个 CheckSet 供终检闭环消费。

pub mod math;

pub mod rt;
pub mod clock;
pub mod gpower;

pub mod infer;
pub mod prefetch;
pub mod forensics;

pub mod gverify;
pub mod selfheal;
pub mod testfw;

pub mod codec;
pub mod capture;
pub mod simd;

pub mod gfx;
pub mod audio3d;
pub mod stream;

pub mod editor;
pub mod debugger;
pub mod replay;

pub mod profiler;
pub mod provenance;
pub mod vcs;

pub mod teaching;
pub mod docgen;
pub mod i18n;

pub mod design;
pub mod theming;
pub mod icons;

pub mod wallpaper;
pub mod motion;
pub mod sound;

pub mod widgets;
pub mod gestures;
pub mod notify;

pub mod settings;
pub mod basicapps;
pub mod inputexp;

pub mod fileman;
pub mod terminal;
pub mod launcher;

pub mod display;
pub mod chaos;
pub mod longevity;

pub mod finalgate;
pub mod verifyall;
pub mod closure;

use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// 共享工具：无分配 ASCII 匹配（no_std 下 str::to_ascii_lowercase 需分配，不可用）
// ---------------------------------------------------------------------------

/// 子串匹配（ASCII 大小写不敏感，无分配）。needle 为空时恒真（与 str::contains 一致）。
pub(crate) fn ascii_contains_ci(hay: &[u8], needle: &[u8]) -> bool {
    let n = needle.len();
    if n == 0 {
        return true;
    }
    if n > hay.len() {
        return false;
    }
    (0..=hay.len() - n).any(|i| {
        hay[i..i + n]
            .iter()
            .zip(needle)
            .all(|(a, b)| a.to_ascii_lowercase() == b.to_ascii_lowercase())
    })
}

/// 前缀匹配（ASCII 大小写不敏感，无分配）。
pub(crate) fn ascii_starts_with_ci(hay: &[u8], needle: &[u8]) -> bool {
    hay.len() >= needle.len()
        && hay[..needle.len()]
            .iter()
            .zip(needle)
            .all(|(a, b)| a.to_ascii_lowercase() == b.to_ascii_lowercase())
}

/// 相等比较（ASCII 大小写不敏感，无分配）。
pub(crate) fn ascii_eq_ci(a: &[u8], b: &[u8]) -> bool {
    a.len() == b.len() && ascii_starts_with_ci(a, b)
}

/// 一次 GALAXY 42 域汇总检查的结果（固定容量）。
pub struct GalaxyReport {
    sets: [Option<CheckSet>; 42],
    count: usize,
}

impl GalaxyReport {
    pub const fn new() -> GalaxyReport {
        GalaxyReport {
            sets: [const { None }; 42],
            count: 0,
        }
    }

    fn register(&mut self, set: CheckSet) {
        if self.count < 45 {
            self.sets[self.count] = Some(set);
            self.count += 1;
        }
    }

    pub fn len(&self) -> usize {
        self.count
    }

    pub fn get(&self, index: usize) -> Option<CheckSet> {
        if index < self.count {
            self.sets[index]
        } else {
            None
        }
    }

    /// (passed, failed) 全量统计。
    pub fn tally(&self) -> (usize, usize) {
        let mut passed = 0usize;
        let mut total = 0usize;
        for i in 0..self.count {
            if let Some(s) = self.get(i) {
                let (p, f) = s.tally();
                passed += p;
                total += p + f;
            }
        }
        (passed, total - passed)
    }

    pub fn all_passed(&self) -> bool {
        (0..self.count).all(|i| self.get(i).map(|s| s.all_passed()).unwrap_or(true))
    }
}

/// 汇总 AI-16~AI-30 全部 45 个子域自检。
pub fn run_galaxy_checks() -> GalaxyReport {
    let mut r = GalaxyReport::new();
    // AI-16
    r.register(rt::run_rt_checks());
    r.register(clock::run_clock_checks());
    r.register(gpower::run_gpower_checks());
    // AI-17
    r.register(infer::run_infer_checks());
    r.register(prefetch::run_prefetch_checks());
    r.register(forensics::run_forensics_checks());
    // AI-18
    r.register(gverify::run_gverify_checks());
    r.register(selfheal::run_selfheal_checks());
    r.register(testfw::run_testfw_checks());
    // AI-19
    r.register(codec::run_codec_checks());
    r.register(capture::run_capture_checks());
    r.register(simd::run_simd_checks());
    // AI-20
    r.register(gfx::run_gfx_checks());
    r.register(audio3d::run_audio3d_checks());
    r.register(stream::run_stream_checks());
    // AI-21
    r.register(editor::run_editor_checks());
    r.register(debugger::run_debugger_checks());
    r.register(replay::run_replay_checks());
    // AI-22
    r.register(profiler::run_profiler_checks());
    r.register(provenance::run_provenance_checks());
    r.register(vcs::run_vcs_checks());
    // AI-23
    r.register(teaching::run_teaching_checks());
    r.register(docgen::run_docgen_checks());
    r.register(i18n::run_i18n_checks());
    // AI-24
    r.register(design::run_design_checks());
    r.register(theming::run_theming_checks());
    r.register(icons::run_icons_checks());
    // AI-25
    r.register(wallpaper::run_wallpaper_checks());
    r.register(motion::run_motion_checks());
    r.register(sound::run_sound_checks());
    // AI-26
    r.register(widgets::run_widgets_checks());
    r.register(gestures::run_gestures_checks());
    r.register(notify::run_notify_checks());
    // AI-27
    r.register(settings::run_settings_checks());
    r.register(basicapps::run_basicapps_checks());
    r.register(inputexp::run_inputexp_checks());
    // AI-28
    r.register(fileman::run_fileman_checks());
    r.register(terminal::run_terminal_checks());
    r.register(launcher::run_launcher_checks());
    // AI-29
    r.register(display::run_display_checks());
    r.register(chaos::run_chaos_checks());
    r.register(longevity::run_longevity_checks());
    r
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn galaxy_report_counts_42_domains() {
        let r = run_galaxy_checks();
        assert_eq!(r.len(), 42);
        // 失败时 panic 出各域 render 明细，定位到具体 F/G 项。
        if !r.all_passed() {
            let mut buf = [0u8; 6144];
            let mut n = 0usize;
            for i in 0..r.len() {
                if let Some(s) = r.get(i) {
                    if !s.all_passed() {
                        let m = s.render(&mut buf[n..]);
                        n += m;
                    }
                }
            }
            panic!("failing galaxy domains:\n{}", core::str::from_utf8(&buf[..n]).unwrap_or("<render>"));
        }
        let (p, f) = r.tally();
        // 840 个 G 项全覆盖（AI-16~29：G901~G1740）；部分项带附加守卫检查，总数略超 840。
        assert!(p + f >= 840, "all 840 GALAXY items G901~G1740 must have checks");
        assert_eq!(f, 0);
    }
}
