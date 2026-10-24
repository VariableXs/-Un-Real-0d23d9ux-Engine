//! ves331_s18perf — VE-F4091 · T05 语言资源性能
//! 切换热更新三步各设阈值（重绑/重排/热换）+ 字体加载首屏预算 + 大语言包（CJK 全量）内存占用档 + 越阈定位与预算重议（T07 仲裁）+ 内存超档子集化加严 + 首屏劣化懒加载 + 低端设备无障碍保底（域本色）。
#![allow(dead_code)]
use alloc::vec;
use alloc::vec::Vec;
use alloc::string::String;
use crate::checks::CheckSet;

pub const SEG_BASE: u16 = 0x96C0;

#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PfCode {
    Ok = 0,
    RebindOver = SEG_BASE + 0x01,
    RelayoutOver = SEG_BASE + 0x02,
    SwapOver = SEG_BASE + 0x03,
    MemOver = SEG_BASE + 0x04,
    FirstPaintOver = SEG_BASE + 0x05,
}

/// 三步阈值表（单源）。
pub struct StepThresholds { pub rebind_ms: u32, pub relayout_ms: u32, pub swap_ms: u32 }

impl StepThresholds {
    pub fn new() -> Self { StepThresholds { rebind_ms: 40, relayout_ms: 400, swap_ms: 120 } }

    /// 度量：耗时 vs 阈值 → Ok 或对应越阈码。
    pub fn measure(&self, step: u8, elapsed_ms: u32) -> Result<(), PfCode> {
        match step {
            0 => if elapsed_ms > self.rebind_ms { Err(PfCode::RebindOver) } else { Ok(()) },
            1 => if elapsed_ms > self.relayout_ms { Err(PfCode::RelayoutOver) } else { Ok(()) },
            2 => if elapsed_ms > self.swap_ms { Err(PfCode::SwapOver) } else { Ok(()) },
            _ => Err(PfCode::RebindOver),
        }
    }
}

/// 首屏预算：字体加载（子集解码+首屏字形栅格化）ms。
pub struct FirstPaintBudget { pub budget_ms: u32 }

impl FirstPaintBudget {
    pub fn new() -> Self { FirstPaintBudget { budget_ms: 250 } }
    pub fn measure(&self, elapsed_ms: u32) -> Result<(), PfCode> {
        if elapsed_ms > self.budget_ms { Err(PfCode::FirstPaintOver) } else { Ok(()) }
    }
}

/// 内存占用档：语言包内存档位（KB）。
pub struct MemTier { pub lang: &'static str, pub tier_kb: u32 }

pub struct MemBook { pub tiers: Vec<MemTier> }

impl MemBook {
    pub fn new() -> Self {
        MemBook { tiers: vec![
            MemTier { lang: "en", tier_kb: 8_000 },
            MemTier { lang: "zh", tier_kb: 48_000 }, // CJK 全量档
            MemTier { lang: "ar", tier_kb: 12_000 },
            MemTier { lang: "th", tier_kb: 14_000 },
        ] }
    }
    pub fn tier_of(&self, lang: &str) -> Option<u32> {
        self.tiers.iter().find(|t| t.lang == lang).map(|t| t.tier_kb)
    }
    /// 内存占用判定：实际 > 档位 → 子集化加严。
    pub fn measure(&self, lang: &str, actual_kb: u32) -> Result<(), PfCode> {
        match self.tier_of(lang) {
            Some(t) if actual_kb <= t => Ok(()),
            Some(_) => Err(PfCode::MemOver),
            None => Ok(()), // 未登档语种不判（登记制外）
        }
    }
}

/// 预算重议立案（T07 仲裁输入）。
pub struct RenegotiateCase { pub subject: &'static str, pub over_ms: u32, pub advice: &'static str }

pub struct RenegotiateBook { pub cases: Vec<RenegotiateCase> }

impl RenegotiateBook {
    pub fn new() -> Self { RenegotiateBook { cases: Vec::new() } }
    pub fn open(&mut self, subject: &'static str, over_ms: u32, advice: &'static str) {
        self.cases.push(RenegotiateCase { subject, over_ms, advice });
    }
    pub fn count(&self) -> usize { self.cases.len() }
}

/// 懒加载策略：首屏劣化时非首屏字形/资产延迟绑定。
pub struct LazyPolicy { pub engaged: bool, pub deferred: Vec<&'static str> }

impl LazyPolicy {
    pub fn new() -> Self { LazyPolicy { engaged: false, deferred: Vec::new() } }

    /// 首屏劣化 → 激懒：首屏字形集之外的资产全部延迟。
    pub fn engage(&mut self, first_screen: &[&'static str], all_assets: &[&'static str]) {
        self.engaged = true;
        self.deferred = all_assets.iter()
            .filter(|a| !first_screen.contains(a))
            .map(|a| *a)
            .collect();
    }

    pub fn deferred_count(&self) -> usize { self.deferred.len() }
}

/// 低端设备无障碍保底（域本色）：即使内存/首屏紧张，读屏对比度与字幕轨保底启用。
pub struct LowEndFloor { pub contrast_ok: bool, pub subtitle_ok: bool, pub screen_reader_ok: bool }

impl LowEndFloor {
    pub fn new() -> Self { LowEndFloor { contrast_ok: false, subtitle_ok: false, screen_reader_ok: false } }
    pub fn all_guaranteed(&self) -> bool { self.contrast_ok && self.subtitle_ok && self.screen_reader_ok }
    /// 保底声明与性能档无关——档位压缩不得关闭这三项。
    pub fn respects_floor(&self, disabled: &[&'static str]) -> bool {
        !disabled.iter().any(|f| *f == "contrast" || *f == "subtitle" || *f == "screen-reader")
    }
}

fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) { set.add(label, ok, detail); }

pub fn run_ves331_checks() -> CheckSet {
    let mut set = CheckSet::new("ves331");
    let th = StepThresholds::new();
    let mut rb = RenegotiateBook::new();

    // C331-01 三步阈值单源：三档齐备且重排最宽。
    chk(&mut set, "C331-01", th.rebind_ms < th.swap_ms && th.swap_ms < th.relayout_ms, "40<120<400 序合理");

    // C331-02 重绑越阈。
    chk(&mut set, "C331-02", th.measure(0, 41) == Err(PfCode::RebindOver) && th.measure(0, 40).is_ok(), "重绑 40ms 阈值边界");

    // C331-03 重排越阈。
    chk(&mut set, "C331-03", th.measure(1, 401) == Err(PfCode::RelayoutOver) && th.measure(1, 400).is_ok(), "重排 400ms 阈值边界");

    // C331-04 热换越阈。
    chk(&mut set, "C331-04", th.measure(2, 121) == Err(PfCode::SwapOver) && th.measure(2, 120).is_ok(), "热换 120ms 阈值边界");

    // C331-05 越阈 → 定位 + 预算重议立案。
    for (s, ms, sub) in [(0u8, 55u32, "rebind"), (1, 500, "relayout"), (2, 130, "swap")] {
        if th.measure(s, ms).is_err() { rb.open(sub, ms, "预算重议 T07 仲裁"); }
    }
    chk(&mut set, "C331-05", rb.count() == 3, "三步越阈全部立案");

    // C331-06 首屏预算：劣化判红。
    let fp = FirstPaintBudget::new();
    chk(&mut set, "C331-06", fp.measure(249).is_ok() && fp.measure(251) == Err(PfCode::FirstPaintOver), "首屏 250ms 预算");

    // C331-07 首屏劣化 → 懒加载：非首屏资产全部延迟。
    let mut lz = LazyPolicy::new();
    lz.engage(&["fnt-zh-core", "txt-zh-menu"], &["fnt-zh-core", "txt-zh-menu", "aud-zh-cue", "img-zh-hero", "fnt-zh-ext"]);
    chk(&mut set, "C331-07", lz.engaged && lz.deferred_count() == 3
        && !lz.deferred.iter().any(|d| *d == "fnt-zh-core"), "懒加载 3 项延迟 首屏不动");

    // C331-08 内存档：CJK 全量档位最大。
    let mb = MemBook::new();
    chk(&mut set, "C331-08", mb.tier_of("zh") == Some(48_000) && mb.tiers.len() == 4, "四语种档 CJK 48000KB");

    // C331-09 内存超档 → 加严判定。
    chk(&mut set, "C331-09", mb.measure("zh", 47_999).is_ok() && mb.measure("zh", 48_001) == Err(PfCode::MemOver), "CJK 超档 → 子集化加严");

    // C331-10 未登档语种不误判。
    chk(&mut set, "C331-10", mb.measure("ko", 999_999).is_ok(), "ko 未登档放行（登记制外）");

    // C331-11 低端保底：三项保底齐备才合格。
    let le = LowEndFloor { contrast_ok: true, subtitle_ok: true, screen_reader_ok: true };
    chk(&mut set, "C331-11", le.all_guaranteed(), "低端三项保底齐");

    // C331-12 档位压缩不得关闭保底项。
    let le2 = LowEndFloor::new();
    chk(&mut set, "C331-12", le2.respects_floor(&["atlas-lru", "anim-scale"])
        && !le2.respects_floor(&["subtitle"]), "保底项白名单外才可关");

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ves331_smoke() {
        let set = run_ves331_checks();
        let red = set.red_items();
        assert!(red.is_empty(), "ves331 red: {:?}", red);
    }

    #[test]
    fn ves331_thresholds() {
        let th = StepThresholds::new();
        assert!(th.measure(0, 39).is_ok());
        assert!(th.measure(1, 399).is_ok());
        assert_eq!(th.measure(2, 200), Err(PfCode::SwapOver));
    }
}
