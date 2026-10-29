//! ves324_s17audio — VE-F4084 · T05 多语言音频资源
//! 配音/提示音按语种版本化 + 命名规范（语种码_场景_版本）强制入库门 + 缺失语种降级链（次选语种音频+字幕兜底——显性标记）+ 转录必备（无障碍硬要求）+ 时长超限重录评审。
#![allow(dead_code)]
use alloc::vec;
use alloc::vec::Vec;
use alloc::string::String;
use crate::checks::CheckSet;

pub const SEG_BASE: u16 = 0x9340;

#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AuCode {
    Ok = 0,
    NameViolation = SEG_BASE + 0x01,
    TranscriptMissing = SEG_BASE + 0x02,
    DurationOver = SEG_BASE + 0x03,
    DupAsset = SEG_BASE + 0x04,
    NoFallback = SEG_BASE + 0x05,
}

/// 音频类别。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AudioKind { Narration = 0, Cue = 1 }

/// 一条音频资产：命名 `lang_scene_vNNN`，配转录文本，登记时长。
#[derive(Clone)]
pub struct AudioAsset {
    pub name: &'static str,   // 例 "zh_menu_v003"
    pub kind: AudioKind,
    pub duration_ms: u32,
    /// 转录/字幕文本键（无障碍硬要求——必配）。
    pub transcript_key: &'static str,
}

impl AudioAsset {
    /// 命名规范解析：`<语种码>_<场景>_<vNNN>`，语种码 2 小写字母、场景非空、v+3 位数字。
    pub fn parse_name<'a>(name: &'a str) -> Option<(&'a str, &'a str, u32)> {
        let parts: Vec<&str> = name.split('_').collect();
        if parts.len() != 3 { return None; }
        let (lang, scene, ver) = (parts[0], parts[1], parts[2]);
        if lang.len() != 2 || !lang.bytes().all(|b| b.is_ascii_lowercase()) { return None; }
        if scene.is_empty() { return None; }
        if !(ver.len() == 4 && ver.starts_with('v') && ver[1..].bytes().all(|b| b.is_ascii_digit())) { return None; }
        Some((lang, scene, ver[1..].parse::<u32>().ok()?))
    }
}

/// 音频登记册。
pub struct AudioBook { pub assets: Vec<AudioAsset>, pub max_duration_ms: u32 }

impl AudioBook {
    pub fn new() -> Self { AudioBook { assets: Vec::new(), max_duration_ms: 120_000 } }

    /// 入库门：命名违规/缺转录/超时长/重复 全部拒绝。
    pub fn add(&mut self, a: AudioAsset) -> Result<(), AuCode> {
        let (_, _, _) = AudioAsset::parse_name(a.name).ok_or(AuCode::NameViolation)?;
        if a.transcript_key.is_empty() { return Err(AuCode::TranscriptMissing); }
        if a.duration_ms == 0 || a.duration_ms > self.max_duration_ms { return Err(AuCode::DurationOver); }
        if self.assets.iter().any(|x| x.name == a.name) { return Err(AuCode::DupAsset); }
        self.assets.push(a);
        Ok(())
    }

    /// 语种场景查找。
    pub fn find(&self, lang: &str, scene: &str) -> Option<&AudioAsset> {
        self.assets.iter().find(|a| {
            if let Some((l, s, _)) = AudioAsset::parse_name(a.name) { l == lang && s == scene } else { false }
        })
    }

    /// 降级链解析：目标语种缺 → 沿次选语种链找；全链无 → NoFallback。
    /// 返回（命中的资产名，是否降级——显性标记）。
    pub fn resolve(&self, want_lang: &str, scene: &str, chain: &[&str]) -> Result<(&'static str, bool), AuCode> {
        if self.find(want_lang, scene).is_some() {
            let n = self.find(want_lang, scene).unwrap().name;
            return Ok((n, false));
        }
        for lang in chain.iter() {
            if *lang == want_lang { continue; }
            if let Some(a) = self.find(lang, scene) {
                return Ok((a.name, true));
            }
        }
        Err(AuCode::NoFallback)
    }
}

/// 显性降级标记表：播放位 →（请求语种，实际资产，字幕兜底键）。
#[derive(Clone)]
pub struct DegradeMark { pub slot: &'static str, pub want_lang: &'static str, pub served: &'static str, pub subtitle_key: &'static str }

pub struct DegradeBook { pub marks: Vec<DegradeMark> }

impl DegradeBook {
    pub fn new() -> Self { DegradeBook { marks: Vec::new() } }

    /// 降级显性登记：降级必须带字幕兜底键。
    pub fn mark(&mut self, slot: &'static str, want_lang: &'static str, served: &'static str, subtitle_key: &'static str) -> Result<(), AuCode> {
        if subtitle_key.is_empty() { return Err(AuCode::TranscriptMissing); }
        self.marks.push(DegradeMark { slot, want_lang, served, subtitle_key });
        Ok(())
    }

    /// 未显性标记的降级数（应为 0——全部显性）。
    pub fn explicit_all(&self) -> bool { !self.marks.iter().any(|m| m.subtitle_key.is_empty()) }

    pub fn count(&self) -> usize { self.marks.len() }
}

/// 时长审查：超限 → 重录评审立案。
pub struct ReviewCase { pub asset: &'static str, pub reason: &'static str }

pub struct ReviewBook { pub cases: Vec<ReviewCase> }

impl ReviewBook {
    pub fn new() -> Self { ReviewBook { cases: Vec::new() } }
    pub fn open(&mut self, asset: &'static str, reason: &'static str) { self.cases.push(ReviewCase { asset, reason }); }
    pub fn count(&self) -> usize { self.cases.len() }
}

fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) { set.add(label, ok, detail); }

pub fn run_ves324_checks() -> CheckSet {
    let mut set = CheckSet::new("ves324");
    let mut book = AudioBook::new();

    // C4084-01 命名规范：合法命名解析。
    let p = AudioAsset::parse_name("zh_menu_v003");
    chk(&mut set, "C4084-01", p == Some(("zh", "menu", 3)), "zh_menu_v003 => (zh,menu,3)");

    // C4084-02 命名违规族：大写语种/坏版本/段数错。
    chk(&mut set, "C4084-02",
        AudioAsset::parse_name("ZH_menu_v003").is_none()
        && AudioAsset::parse_name("zh_menu_3").is_none()
        && AudioAsset::parse_name("zh_menu").is_none(), "三类命名违规全拒");

    // C4084-03 转录必备：无转录键拒入库。
    let bad = AudioAsset { name: "en_cue_v001", kind: AudioKind::Cue, duration_ms: 800, transcript_key: "" };
    chk(&mut set, "C4084-03", book.add(bad) == Err(AuCode::TranscriptMissing), "缺转录 => 拒");

    // C4084-04 时长超限拒 + 立案。
    let long = AudioAsset { name: "en_narr_v001", kind: AudioKind::Narration, duration_ms: 999_999, transcript_key: "K.T.N1" };
    let mut rb = ReviewBook::new();
    match book.add(long) {
        Err(AuCode::DurationOver) => rb.open("en_narr_v001", "时长超限重录评审"),
        _ => {}
    }
    chk(&mut set, "C4084-04", rb.count() == 1, "超限拒 + 重录评审立案");

    // C4084-05 正常入库四条。
    for a in [
        AudioAsset { name: "zh_menu_v001", kind: AudioKind::Cue, duration_ms: 1200, transcript_key: "K.T.Z1" },
        AudioAsset { name: "en_menu_v001", kind: AudioKind::Cue, duration_ms: 1100, transcript_key: "K.T.E1" },
        AudioAsset { name: "ar_menu_v001", kind: AudioKind::Cue, duration_ms: 1300, transcript_key: "K.T.A1" },
        AudioAsset { name: "ja_alert_v002", kind: AudioKind::Cue, duration_ms: 900, transcript_key: "K.T.J1" },
    ] { let _ = book.add(a); }
    chk(&mut set, "C4084-05", book.assets.len() == 4, "四条合法入库");

    // C4084-06 重复名拒。
    let dup = AudioAsset { name: "zh_menu_v001", kind: AudioKind::Cue, duration_ms: 1200, transcript_key: "K.T.Z1" };
    chk(&mut set, "C4084-06", book.add(dup) == Err(AuCode::DupAsset), "同名重复 => DupAsset");

    // C4084-07 目标语种命中：不降级。
    let r = book.resolve("zh", "menu", &["en", "ja"]);
    chk(&mut set, "C4084-07", r == Ok(("zh_menu_v001", false)), "zh 命中不降级");

    // C4084-08 降级链：zh alert 缺 → 次选 ja 命中并显性。
    let r2 = book.resolve("zh", "alert", &["ja", "en"]);
    chk(&mut set, "C4084-08", r2 == Ok(("ja_alert_v002", true)), "zh 缺 → ja 降级命中");

    // C4084-09 全链无 → NoFallback。
    chk(&mut set, "C4084-09", book.resolve("ko", "alert", &["en"]) == Err(AuCode::NoFallback), "ko 全链无 => NoFallback");

    // C4084-10 降级显性标记：必须带字幕兜底键。
    let mut db = DegradeBook::new();
    let nomark = db.mark("alert-slot", "zh", "ja_alert_v002", "");
    chk(&mut set, "C4084-10", nomark == Err(AuCode::TranscriptMissing), "无字幕兜底键 => 拒标记");

    // C4084-11 合法标记 + 全显性核验。
    let _ = db.mark("alert-slot", "zh", "ja_alert_v002", "K.T.J1.sub");
    let _ = db.mark("menu-slot", "ar", "en_menu_v001", "K.T.E1.sub");
    chk(&mut set, "C4084-11", db.count() == 2 && db.explicit_all(), "两处降级全显性带字幕");

    // C4084-12 语种版本化：同场景多语种并存且可查。
    chk(&mut set, "C4084-12",
        book.find("en", "menu").is_some() && book.find("ar", "menu").is_some() && book.find("ja", "alert").is_some(),
        "三语种版本化并存可查");

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ves324_smoke() {
        let set = run_ves324_checks();
        assert!(set.failed_count() == 0, "ves324 red: {}", set.red_items_text());
    }

    #[test]
    fn ves324_name_rules() {
        assert!(AudioAsset::parse_name("th_cue_v012").is_some());
        assert!(AudioAsset::parse_name("e_cue_v012").is_none());
        assert!(AudioAsset::parse_name("zh_menu_vXyz").is_none());
    }
}
