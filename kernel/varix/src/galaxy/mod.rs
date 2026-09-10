//! GALAXY-1800 AI-16~AI-25 落地基座（G901~G1500）。
//!
//! 十个域，每域三文件，每文件 20 项功能（纯逻辑 + 固定容量数组）：
//! AI-16 `rt` / `clock` / `gpower`，AI-17 `infer` / `prefetch` / `forensics`，
//! AI-18 `gverify` / `selfheal` / `testfw`，AI-19 `codec` / `capture` / `simd`，
//! AI-20 `gfx` / `audio3d` / `stream`，AI-21 `editor` / `debugger` / `replay`，
//! AI-22 `profiler` / `provenance` / `vcs`，AI-23 `teaching` / `docgen` / `i18n`，
//! AI-24 `design` / `theming` / `icons`，AI-25 `wallpaper` / `motion` / `sound`。
//!
//! 纪律：不覆盖既有模块；每文件导出 `run_<x>_checks() -> CheckSet`；
//! `run_galaxy_checks()` 汇总 30 个 CheckSet 供终检闭环消费。

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

use crate::checks::CheckSet;

/// 一次 GALAXY 十域汇总检查的结果（30 个 CheckSet，固定容量）。
pub struct GalaxyReport {
    sets: [Option<CheckSet>; 30],
    count: usize,
}

impl GalaxyReport {
    pub const fn new() -> GalaxyReport {
        GalaxyReport {
            sets: [None; 30],
            count: 0,
        }
    }

    fn register(&mut self, set: CheckSet) {
        if self.count < 30 {
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

/// 汇总 AI-16~AI-25 全部 30 个子域自检。
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
    r
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn galaxy_report_counts_30_domains() {
        let r = run_galaxy_checks();
        assert_eq!(r.len(), 30);
        // 失败时 panic 出各域 render 明细，定位到具体 F/G 项。
        if !r.all_passed() {
            let mut buf = [0u8; 4096];
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
        // 600 个 G 项全覆盖；G923/G1312 等带附加守卫检查，故总数略超 600。
        assert!(p + f >= 600, "all 600 GALAXY items G901~G1500 must have checks");
        assert_eq!(f, 0);
    }
}
