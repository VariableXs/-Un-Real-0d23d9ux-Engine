//! ves325_s17video — VE-F4085 · T05 多语言视频资源
//! 多音轨+多字幕轨封装规范（轨语种元数据必填）+ 字幕时间轴与音轨同步断言 + SDH 听障字幕规范位（域本色）+ 封面文化适配（复用 F4083 禁忌审查）。
#![allow(dead_code)]
use alloc::vec;
use alloc::vec::Vec;
use alloc::string::String;
use crate::checks::CheckSet;

pub const SEG_BASE: u16 = 0x93C0;

#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum VdCode {
    Ok = 0,
    MetaMissing = SEG_BASE + 0x01,
    SyncDrift = SEG_BASE + 0x02,
    SdhMissing = SEG_BASE + 0x03,
    CoverTaboo = SEG_BASE + 0x04,
    DupTrack = SEG_BASE + 0x05,
}

/// 一条轨：音轨或字幕轨，语种元数据必填。
#[derive(Clone)]
pub struct Track {
    pub lang: &'static str,   // 必填（空即违规）
    pub is_sdh: bool,         // 字幕轨的 SDH 规范位
    pub is_audio: bool,       // true=音轨 false=字幕轨
}

/// 封装规范：一个视频资产的多轨登记 + 同步断言集。
#[derive(Clone)]
pub struct VideoAsset {
    pub id: u32,
    pub tracks: Vec<Track>,
    /// 音轨时长 ms（同步基准）。
    pub audio_ms: u32,
    /// 字幕轨尾时间戳 ms（每字幕轨一条——与 tracks 同序的 None 表示该轨无时间轴）。
    pub sub_tail_ms: Vec<Option<u32>>,
    /// 封面图标签（走 F4083 审查）。
    pub cover_tag: &'static str,
}

impl VideoAsset {
    /// 封装规范校验：每轨语种必填；同轨型语种重复 = DupTrack；字幕轨必须有时间轴登记。
    pub fn validate(&self) -> Result<(), VdCode> {
        if self.tracks.is_empty() { return Err(VdCode::MetaMissing); }
        for t in self.tracks.iter() {
            if t.lang.is_empty() { return Err(VdCode::MetaMissing); }
        }
        for i in 0..self.tracks.len() {
            for j in (i + 1)..self.tracks.len() {
                if self.tracks[i].is_audio == self.tracks[j].is_audio
                    && self.tracks[i].lang == self.tracks[j].lang
                    && self.tracks[i].is_sdh == self.tracks[j].is_sdh {
                    return Err(VdCode::DupTrack);
                }
            }
        }
        if self.sub_tail_ms.len() != self.tracks.len() { return Err(VdCode::MetaMissing); }
        for (i, t) in self.tracks.iter().enumerate() {
            if !t.is_audio && self.sub_tail_ms[i].is_none() { return Err(VdCode::MetaMissing); }
        }
        Ok(())
    }

    /// SDH 位：至少一条字幕轨声明 SDH。
    pub fn has_sdh(&self) -> bool {
        self.tracks.iter().any(|t| !t.is_audio && t.is_sdh)
    }

    /// 同步断言：字幕轨尾不得超过音轨时长 + 容差，且不得早于音轨一半（完整性）。
    /// 容差 1500ms（帧率/封装抖动口径）。
    pub fn sync_ok(&self) -> Result<bool, VdCode> {
        self.validate()?;
        const TOL: u32 = 1500;
        for (i, t) in self.tracks.iter().enumerate() {
            if t.is_audio { continue; }
            let tail = self.sub_tail_ms[i].unwrap_or(0);
            if tail > self.audio_ms + TOL { return Err(VdCode::SyncDrift); }
            if tail < self.audio_ms / 2 { return Ok(false); }
        }
        Ok(true)
    }
}

/// 封面文化适配门：复用 F4083 的禁忌册接口形状（region × token 子串匹配）。
pub struct CoverGate<'a> { pub tokens: Vec<(&'a str, &'a str)> } // (region, token)

impl<'a> CoverGate<'a> {
    pub fn new() -> Self { CoverGate { tokens: Vec::new() } }
    pub fn add(&mut self, region: &'a str, token: &'a str) { self.tokens.push((region, token)); }
    pub fn screen(&self, region: &str, tag: &str) -> Result<(), &'a str> {
        for (r, t) in self.tokens.iter() {
            if *r == region && tag.contains(t) { return Err(t); }
        }
        Ok(())
    }
}

/// 视频登记册：入库前 validate + SDH 必检 + 封面审查。
pub struct VideoBook<'a> {
    pub assets: Vec<VideoAsset>,
    pub cover: CoverGate<'a>,
    pub sdh_required: bool,
}

impl<'a> VideoBook<'a> {
    pub fn new(sdh_required: bool) -> Self {
        VideoBook { assets: Vec::new(), cover: CoverGate::new(), sdh_required }
    }

    pub fn add(&mut self, v: VideoAsset) -> Result<(), VdCode> {
        v.validate()?;
        if self.sdh_required && !v.has_sdh() { return Err(VdCode::SdhMissing); }
        if self.assets.iter().any(|x| x.id == v.id) { return Err(VdCode::DupTrack); }
        self.assets.push(v);
        Ok(())
    }

    /// 封面审查门：命中禁忌 → 替换评审。
    pub fn screen_cover(&self, region: &str, v: &VideoAsset) -> Result<(), &'a str> {
        self.cover.screen(region, v.cover_tag)
    }
}

/// 重封装立案（同步漂移处置）。
pub struct RemuxCase { pub video: u32, pub track_lang: &'static str }

pub struct RemuxBook { pub cases: Vec<RemuxCase> }

impl RemuxBook {
    pub fn new() -> Self { RemuxBook { cases: Vec::new() } }
    pub fn open(&mut self, video: u32, lang: &'static str) { self.cases.push(RemuxCase { video, track_lang: lang }); }
    pub fn count(&self) -> usize { self.cases.len() }
}

fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) { set.add(label, ok, detail); }

pub fn run_ves325_checks() -> CheckSet {
    let mut set = CheckSet::new("ves325");

    let mk = |id: u32, audio_ms: u32, tail: u32, sdh: bool| VideoAsset {
        id,
        tracks: vec![
            Track { lang: "en", is_sdh: false, is_audio: true },
            Track { lang: "zh", is_sdh: false, is_audio: false },
            Track { lang: "zh", is_sdh: sdh, is_audio: false },
        ],
        audio_ms,
        sub_tail_ms: vec![None, Some(tail), Some(tail)],
        cover_tag: "cover-hero-blue",
    };

    // C4085-01 封装规范校验：合法封装过闸。
    let v = mk(1, 120_000, 118_000, true);
    chk(&mut set, "C4085-01", v.validate().is_ok(), "三轨封装（1 音轨+2 字幕轨）过闸");

    // C4085-02 轨元数据缺语种 → 拒。
    let mut bad = v.clone();
    bad.tracks[1].lang = "";
    chk(&mut set, "C4085-02", bad.validate() == Err(VdCode::MetaMissing), "轨语种空 => MetaMissing");

    // C4085-03 字幕轨缺时间轴登记 → 拒。
    let mut bad2 = v.clone();
    bad2.sub_tail_ms = vec![None, None, Some(118_000)];
    chk(&mut set, "C4085-03", bad2.validate() == Err(VdCode::MetaMissing), "字幕轨无时间轴 => 拒");

    // C4085-04 同步断言：尾部超容差（120000+1500）→ SyncDrift。
    let drift = mk(2, 120_000, 122_000, true);
    chk(&mut set, "C4085-04", drift.sync_ok() == Err(VdCode::SyncDrift), "漂移 2000ms > 容差 1500ms");

    // C4085-05 同步断言：尾部早于音轨一半 → false（完整性）。
    let short = mk(3, 120_000, 50_000, true);
    chk(&mut set, "C4085-05", short.sync_ok() == Ok(false), "字幕轨只覆盖 42% => 不同步");

    // C4085-06 SDH 位：含 SDH 轨通过硬检。
    let mut book = VideoBook::new(true);
    chk(&mut set, "C4085-06", book.add(v.clone()).is_ok(), "含 SDH 入库过硬检");

    // C4085-07 SDH 缺失硬检拒。
    let nosdh = mk(4, 120_000, 118_000, false);
    chk(&mut set, "C4085-07", book.add(nosdh) == Err(VdCode::SdhMissing), "无 SDH => 硬检拒");

    // C4085-08 重复 id 拒。
    chk(&mut set, "C4085-08", book.add(v.clone()) == Err(VdCode::DupTrack), "重复 id => 拒");

    // C4085-09 封面文化适配：登记禁忌并命中。
    book.cover.add("SA", "hex-star");
    book.cover.add("JP", "white-carnation");
    let taboo_cover = VideoAsset { cover_tag: "hex-star-emblem", ..v.clone() };
    let hit = book.screen_cover("SA", &taboo_cover);
    chk(&mut set, "C4085-09", hit == Err("hex-star"), "封面禁忌命中 => 替换评审");

    // C4085-10 封面未命中地区放行。
    let safe = VideoAsset { cover_tag: "cover-plain", ..v.clone() };
    chk(&mut set, "C4085-10", book.screen_cover("SA", &safe).is_ok(), "未命中放行");

    // C4085-11 同步漂移立案重封装。
    let mut rb = RemuxBook::new();
    if drift.sync_ok() == Err(VdCode::SyncDrift) { rb.open(2, "zh"); }
    chk(&mut set, "C4085-11", rb.count() == 1, "漂移定位 + 重封装立案");

    // C4085-12 has_sdh 判定单源复述。
    chk(&mut set, "C4085-12", v.has_sdh() && !nosdh_has(&mk(9, 100, 90, false)), "SDH 位判定一致");

    set
}

fn nosdh_has(v: &VideoAsset) -> bool { v.has_sdh() }

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ves325_smoke() {
        let set = run_ves325_checks();
        assert!(set.failed_count() == 0, "ves325 red: {}", set.red_items_text());
    }

    #[test]
    fn ves325_sync_tolerance() {
        let v = VideoAsset { id: 7,
            tracks: vec![Track { lang: "en", is_sdh: false, is_audio: true },
                         Track { lang: "zh", is_sdh: true, is_audio: false }],
            audio_ms: 60_000,
            sub_tail_ms: vec![None, Some(61_500)],
            cover_tag: "c" };
        assert_eq!(v.sync_ok(), Ok(true));
    }
}
