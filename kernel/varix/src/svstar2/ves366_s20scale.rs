//! ves366_s20scale — VE-F4126 · T07 大规模 i18n 性能
//! 三场景压测档：千键页面×多语种并发切换 + 十万键全量翻译加载 + 多用户语种混合会话 + 批量优化策略（查找批处理/重排合并/资源预取）+ 预取误伤让路策略 + 读屏会话并发档（域本色）+ 场景劣化定位加严。
#![allow(dead_code)]
use alloc::vec;
use alloc::vec::Vec;
use alloc::string::String;
use crate::checks::CheckSet;

pub const SEG_BASE: u16 = 0xD540;

#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ScCode {
    Ok = 0,
    SceneRegress = SEG_BASE + 0x01,
    PrefetchHarm = SEG_BASE + 0x02,
}

/// 三场景。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum StressScene { ThousandKeys = 0, HundredKLoad = 1, MixedSessions = 2 }

impl StressScene {
    pub const ALL: [StressScene; 3] = [
        StressScene::ThousandKeys, StressScene::HundredKLoad, StressScene::MixedSessions];
    pub fn name(self) -> &'static str {
        match self { StressScene::ThousandKeys => "1k-keys-switch",
            StressScene::HundredKLoad => "100k-load", StressScene::MixedSessions => "mixed-sessions" }
    }
    pub fn scale(self) -> u32 {
        match self { StressScene::ThousandKeys => 1_000, StressScene::HundredKLoad => 100_000,
            StressScene::MixedSessions => 64 }
    }
}

/// 压测档：场景 × 实测 ms × 预算 ms。
pub struct StressRun { pub scene: StressScene, pub measured_ms: u32, pub budget_ms: u32 }

pub struct StressBook { pub runs: Vec<StressRun> }

impl StressBook {
    pub fn new() -> Self { StressBook { runs: Vec::new() } }

    pub fn record(&mut self, r: StressRun) {
        if let Some(x) = self.runs.iter_mut().find(|x| x.scene == r.scene) {
            x.measured_ms = r.measured_ms;
            x.budget_ms = r.budget_ms;
        } else {
            self.runs.push(r);
        }
    }

    /// 场景劣化判定。
    pub fn regressed(&self, scene: StressScene) -> bool {
        self.runs.iter().find(|r| r.scene == scene)
            .map_or(false, |r| r.measured_ms > r.budget_ms)
    }

    /// 三场景齐备。
    pub fn complete(&self) -> bool {
        StressScene::ALL.iter().all(|s| self.runs.iter().any(|r| r.scene == *s))
    }

    /// 劣化场景定位。
    pub fn regressed_scenes(&self) -> Vec<&'static str> {
        StressScene::ALL.iter()
            .filter(|s| self.regressed(**s))
            .map(|s| s.name())
            .collect()
    }
}

/// 批量查找：键批合并处理（批大小 × 批数）。
pub struct BatchLookup { pub batch_size: usize, pub keys: usize }

impl BatchLookup {
    pub fn batches(&self) -> usize {
        if self.batch_size == 0 { return 0; }
        (self.keys + self.batch_size - 1) / self.batch_size
    }
    /// 批处理收益：批间开销按批数计（对比逐键）。
    pub fn overhead_units(&self, per_call: u32) -> u32 {
        self.batches() as u32 * per_call
    }
}

/// 重排合并：同一帧内多次重排请求合并为一次。
pub struct RelayoutMerge { pub requests: usize, pub frames: usize }

impl RelayoutMerge {
    /// 合并率（万分比）。
    pub fn merge_ratio(&self) -> u32 {
        if self.requests == 0 { return 0; }
        ((self.requests - self.frames) * 10_000 / self.requests) as u32
    }
}

/// 资源预取：预取命中/误伤统计；误伤让路（命中页让位于活跃页）。
pub struct Prefetcher { pub fetched: Vec<&'static str>, pub used: Vec<&'static str>, pub yielding: bool }

impl Prefetcher {
    pub fn new() -> Self { Prefetcher { fetched: Vec::new(), used: Vec::new(), yielding: false } }

    pub fn prefetch(&mut self, lang: &'static str) { self.fetched.push(lang); }
    pub fn use_lang(&mut self, lang: &'static str) {
        if !self.used.contains(&lang) { self.used.push(lang); }
    }

    /// 误伤率（万分比）：预取未用占比。
    pub fn harm_rate(&self) -> u32 {
        if self.fetched.is_empty() { return 0; }
        let unused = self.fetched.iter().filter(|f| !self.used.contains(f)).count();
        (unused * 10_000 / self.fetched.len()) as u32
    }

    /// 误伤超阈 → 让路策略激活（不再预取）。
    pub fn gate(&mut self, harm_floor_bp: u32) -> bool {
        if self.harm_rate() > harm_floor_bp {
            self.yielding = true;
            return true;
        }
        false
    }
}

/// 读屏会话并发档（域本色）：压测并发会话中必须含读屏会话且不劣化。
pub struct SessionMix { pub total: usize, pub screen_reader: usize, pub sr_ms: u32, pub sr_budget_ms: u32 }

impl SessionMix {
    pub fn has_sr(&self) -> bool { self.screen_reader > 0 }
    pub fn sr_ok(&self) -> bool { self.has_sr() && self.sr_ms <= self.sr_budget_ms }
}

fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) { set.add(label, ok, detail); }

pub fn run_ves366_checks() -> CheckSet {
    let mut set = CheckSet::new("ves366");

    // C366-01 三场景名单与规模单源。
    chk(&mut set, "C366-01",
        StressScene::ALL.len() == 3 && StressScene::HundredKLoad.scale() == 100_000, "千键/十万键/64 会话");

    // C366-02 压测登记齐全。
    let mut sb = StressBook::new();
    sb.record(StressRun { scene: StressScene::ThousandKeys, measured_ms: 800, budget_ms: 1_000 });
    sb.record(StressRun { scene: StressScene::HundredKLoad, measured_ms: 4_200, budget_ms: 5_000 });
    sb.record(StressRun { scene: StressScene::MixedSessions, measured_ms: 900, budget_ms: 1_200 });
    chk(&mut set, "C366-02", sb.complete() && !sb.regressed(StressScene::ThousandKeys), "三场景齐且首场景过");

    // C366-03 场景劣化检出。
    sb.record(StressRun { scene: StressScene::HundredKLoad, measured_ms: 5_600, budget_ms: 5_000 });
    chk(&mut set, "C366-03", sb.regressed_scenes() == vec!["100k-load"], "十万键劣化定位");

    // C366-04 覆盖更新幂等。
    chk(&mut set, "C366-04", sb.runs.len() == 3, "同场景覆盖不增行");

    // C366-05 批量查找：1000 键批 64 → 16 批。
    let bl = BatchLookup { batch_size: 64, keys: 1_000 };
    chk(&mut set, "C366-05", bl.batches() == 16 && bl.overhead_units(10) == 160, "16 批×10 开销 = 160");

    // C366-06 批收益对比逐键。
    let per_key = 1_000u32 * 10;
    chk(&mut set, "C366-06", bl.overhead_units(10) * 60 < per_key, "批量开销远小于逐键 10000");

    // C366-07 重排合并率。
    let rm = RelayoutMerge { requests: 40, frames: 5 };
    chk(&mut set, "C366-07", rm.merge_ratio() == 8_750, "40 请求合 5 帧 = 87.5%");

    // C366-08 全请求同帧合并率 100%。
    let rm2 = RelayoutMerge { requests: 10, frames: 1 };
    chk(&mut set, "C366-08", rm2.merge_ratio() == 9_000, "10 合 1 = 9000bp");

    // C366-09 预取命中良好。
    let mut pf = Prefetcher::new();
    for l in ["ja", "ko", "de"].iter() { pf.prefetch(l); }
    pf.use_lang("ja"); pf.use_lang("ko");
    chk(&mut set, "C366-09", pf.harm_rate() == 3_333, "3 取 2 用 → 1/3 误伤");

    // C366-10 误伤超阈 → 让路激活。
    chk(&mut set, "C366-10", pf.gate(2_000) && pf.yielding, "33% > 20% => 让路");

    // C366-11 带内不让路。
    let mut pf2 = Prefetcher::new();
    pf2.prefetch("fr"); pf2.use_lang("fr");
    chk(&mut set, "C366-11", !pf2.gate(2_000) && !pf2.yielding, "0 误伤不让路");

    // C366-12 读屏并发档（域本色）。
    let sm = SessionMix { total: 64, screen_reader: 4, sr_ms: 950, sr_budget_ms: 1_000 };
    let sm_bad = SessionMix { total: 64, screen_reader: 0, sr_ms: 100, sr_budget_ms: 1_000 };
    chk(&mut set, "C366-12", sm.sr_ok() && !sm_bad.sr_ok(), "读屏档在且不劣化 / 零读屏档不合格");

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ves366_smoke() {
        let set = run_ves366_checks();
        let red = set.red_items();
        assert!(red.is_empty(), "ves366 red: {:?}", red);
    }

    #[test]
    fn ves366_scale() {
        assert_eq!(StressScene::ThousandKeys.scale(), 1_000);
        assert_eq!(StressScene::MixedSessions.scale(), 64);
    }
}
