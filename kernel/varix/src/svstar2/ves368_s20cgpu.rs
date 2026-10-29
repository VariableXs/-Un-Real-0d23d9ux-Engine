//! ves368_s20cgpu — VE-F4128 · T07 i18n 与 CGPU 联动
//! CGPU 对 i18n 算力支撑三路：排版整形加速（四族整形 GPU 路径）/ 字体光栅化加速 / 文本度量缓存 + GPU 路径失效 CPU 回退显性标记 + 结果不一致对拍停用 + 缺口长期登记 CGPU 排期 + 高对比模式渲染加速兼容核验（域本色）。
#![allow(dead_code)]
use alloc::vec;
use alloc::vec::Vec;
use alloc::string::String;
use crate::checks::CheckSet;

pub const SEG_BASE: u16 = 0xD640;

#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CgCode {
    Ok = 0,
    GpuDown = SEG_BASE + 0x01,
    Diverged = SEG_BASE + 0x02,
    StaleGap = SEG_BASE + 0x03,
    HcMismatch = SEG_BASE + 0x04,
}

/// 三路加速面。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AccelPath { Shaping = 0, Rasterize = 1, MetricCache = 2 }

impl AccelPath {
    pub const ALL: [AccelPath; 3] = [
        AccelPath::Shaping, AccelPath::Rasterize, AccelPath::MetricCache];
    pub fn name(self) -> &'static str {
        match self { AccelPath::Shaping => "shaping-gpu", AccelPath::Rasterize => "raster-gpu",
            AccelPath::MetricCache => "metric-cache" }
    }
}

/// 联动面册：每路可用性 + 回退标记。
pub struct AccelBook {
    pub available: [bool; 3],
    pub cpu_fallback: [bool; 3],
    pub disabled: Vec<&'static str>,
}

impl AccelBook {
    pub fn new(all_up: bool) -> Self {
        AccelBook {
            available: [all_up; 3],
            cpu_fallback: [false; 3],
            disabled: Vec::new(),
        }
    }

    /// 路径请求：GPU 可用走加速；失效 → CPU 回退 + 显性标记。
    pub fn request(&mut self, path: AccelPath) -> Result<&'static str, CgCode> {
        let i = path as usize;
        if self.available[i] && !self.disabled.contains(&path.name()) {
            Ok(path.name())
        } else {
            self.cpu_fallback[i] = true; // 显性标记
            Err(CgCode::GpuDown)
        }
    }

    /// 结果对拍：GPU vs CPU 输出不一致 → 停用该路。
    pub fn diverge_check(&mut self, path: AccelPath, gpu_out: u32, cpu_out: u32) -> bool {
        if gpu_out != cpu_out {
            if !self.disabled.contains(&path.name()) { self.disabled.push(path.name()); }
            return true;
        }
        false
    }

    pub fn fallback_marked(&self, path: AccelPath) -> bool { self.cpu_fallback[path as usize] }
}

/// 缺口登记：长期未解升级 CGPU 排期。
pub struct GapBook { pub entries: Vec<(&'static str, u32)>, pub upgraded: Vec<&'static str> }

pub const STALE_ROUNDS: u32 = 6;

impl GapBook {
    pub fn new() -> Self { GapBook { entries: Vec::new(), upgraded: Vec::new() } }

    pub fn register(&mut self, gap: &'static str) {
        if let Some(e) = self.entries.iter_mut().find(|e| e.0 == gap) {
            e.1 += 1;
            if e.1 >= STALE_ROUNDS && !self.upgraded.contains(&gap) {
                self.upgraded.push(gap);
            }
        } else {
            self.entries.push((gap, 1));
        }
    }

    pub fn stale(&self) -> Vec<&'static str> {
        self.entries.iter().filter(|e| e.1 >= STALE_ROUNDS).map(|e| e.0).collect()
    }
}

/// 高对比模式渲染加速兼容核验（域本色）：四族 HC 下 GPU 光栅声明兼容。
pub struct HcCheck { pub ready: Vec<&'static str> }

impl HcCheck {
    pub fn new() -> Self { HcCheck { ready: Vec::new() } }
    pub fn mark(&mut self, family: &'static str) {
        if !self.ready.contains(&family) { self.ready.push(family); }
    }
    pub fn verify(&self, families: &[&'static str]) -> Result<(), CgCode> {
        for f in families.iter() {
            if !self.ready.contains(f) { return Err(CgCode::HcMismatch); }
        }
        Ok(())
    }
}

/// 度量缓存：文本度量键 → 值（命中免 GPU 请求）。
pub struct MetricCache { pub items: Vec<(u32, u32)> }

pub fn metric_key(cp: u32, size_px: u16) -> u32 { cp ^ ((size_px as u32) << 16) }

impl MetricCache {
    pub fn new() -> Self { MetricCache { items: Vec::new() } }
    pub fn put(&mut self, k: u32, v: u32) {
        if !self.items.iter().any(|x| x.0 == k) { self.items.push((k, v)); }
    }
    pub fn get(&self, k: u32) -> Option<u32> {
        self.items.iter().find(|x| x.0 == k).map(|x| x.1)
    }
    pub fn len(&self) -> usize { self.items.len() }
}

fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) { set.add(label, ok, detail); }

pub fn run_ves368_checks() -> CheckSet {
    let mut set = CheckSet::new("ves368");

    // C368-01 三路单源。
    chk(&mut set, "C368-01", AccelPath::ALL.len() == 3 && AccelPath::ALL[0].name() == "shaping-gpu", "三路单源");

    // C368-02 全可用走加速。
    let mut ab = AccelBook::new(true);
    for p in AccelPath::ALL.iter() {
        let _ = ab.request(*p);
    }
    chk(&mut set, "C368-02", !ab.cpu_fallback.iter().any(|f| *f), "三路全加速无回退");

    // C368-03 GPU 失效 → CPU 回退显性标记。
    let mut ab2 = AccelBook::new(false);
    let r = ab2.request(AccelPath::Shaping);
    chk(&mut set, "C368-03", r == Err(CgCode::GpuDown) && ab2.fallback_marked(AccelPath::Shaping), "整形路回退标记");

    // C368-04 结果不一致 → 对拍停用。
    let mut ab3 = AccelBook::new(true);
    chk(&mut set, "C368-04", ab3.diverge_check(AccelPath::Rasterize, 100, 101) && ab3.disabled == vec!["raster-gpu"], "漂移停用光栅路");

    // C368-05 停用后请求走回退。
    let r2 = ab3.request(AccelPath::Rasterize);
    chk(&mut set, "C368-05", r2 == Err(CgCode::GpuDown) && ab3.fallback_marked(AccelPath::Rasterize), "停用即回退");

    // C368-06 一致不停用。
    let mut ab4 = AccelBook::new(true);
    chk(&mut set, "C368-06", !ab4.diverge_check(AccelPath::MetricCache, 42, 42) && ab4.disabled.is_empty(), "一致保留");

    // C368-07 缺口登记累计。
    let mut gb = GapBook::new();
    for _ in 0..3 { gb.register("cjk-shaping-slow"); }
    chk(&mut set, "C368-07", gb.entries[0].1 == 3 && gb.stale().is_empty(), "3 轮未升级");

    // C368-08 长期未解升级排期。
    for _ in 0..3 { gb.register("cjk-shaping-slow"); }
    chk(&mut set, "C368-08", gb.stale() == vec!["cjk-shaping-slow"] && gb.upgraded == vec!["cjk-shaping-slow"], "6 轮 → CGPU 排期");

    // C368-09 高对比核验（域本色）：缺族拒。
    let mut hc = HcCheck::new();
    hc.mark("latin"); hc.mark("cjk");
    chk(&mut set, "C368-09", hc.verify(&["latin", "cjk", "arabic"]) == Err(CgCode::HcMismatch), "缺 arabic => 拒");

    // C368-10 补齐过闸。
    hc.mark("arabic"); hc.mark("thai-indic");
    chk(&mut set, "C368-10", hc.verify(&["latin", "cjk", "arabic", "thai-indic"]).is_ok(), "四族齐过闸");

    // C368-11 度量缓存命中。
    let mut mc = MetricCache::new();
    let k = metric_key(0x4E00, 16);
    mc.put(k, 16);
    chk(&mut set, "C368-11", mc.get(k) == Some(16) && mc.len() == 1, "度量缓存命中");

    // C368-12 缓存键区分字号。
    chk(&mut set, "C368-12", metric_key(0x41, 12) != metric_key(0x41, 14), "同码点异字号不同键");

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ves368_smoke() {
        let set = run_ves368_checks();
        assert!(set.failed_count() == 0, "ves368 red: {}", set.red_items_text());
    }

    #[test]
    fn ves368_fallback() {
        let mut ab = AccelBook::new(false);
        for p in AccelPath::ALL.iter() {
            let _ = ab.request(*p);
        }
        assert!(ab.cpu_fallback.iter().all(|f| *f));
    }
}
