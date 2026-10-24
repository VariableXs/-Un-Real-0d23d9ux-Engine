//! ves335_s18example — VE-F4095 · T05 语言资源示例
//! 示例集：新语种资产接入全套样例（字体子集+图片分层+音频字幕）+ 热更新接入示例 + 可执行跑通即验收 + 示例跑挂即缺陷 + 资产缺样例接入阻断 + 与 F4076 翻译示例对拍 + 示例资产虚构（域本色：字幕/转录全配套示范）。
#![allow(dead_code)]
use alloc::vec;
use alloc::vec::Vec;
use alloc::string::String;
use crate::checks::CheckSet;

pub const SEG_BASE: u16 = 0x9940;

#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ExCode {
    Ok = 0,
    RunFail = SEG_BASE + 0x01,
    BlockNoSample = SEG_BASE + 0x02,
    CanonMismatch = SEG_BASE + 0x03,
    DupExample = SEG_BASE + 0x04,
}

/// 示例五主题（接入全套 + 热更新）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ExTheme { FontSubset = 0, ImageLayer = 1, AudioSubtitle = 2, HotUpdate = 3, VerifyActivate = 4 }

impl ExTheme {
    pub const ALL: [ExTheme; 5] = [
        ExTheme::FontSubset, ExTheme::ImageLayer, ExTheme::AudioSubtitle,
        ExTheme::HotUpdate, ExTheme::VerifyActivate];
    pub fn name(self) -> &'static str {
        match self {
            ExTheme::FontSubset => "font-subset", ExTheme::ImageLayer => "image-layer",
            ExTheme::AudioSubtitle => "audio-subtitle", ExTheme::HotUpdate => "hot-update",
            ExTheme::VerifyActivate => "verify-activate",
        }
    }
}

/// 一条示例：虚构语种资产 + 可执行脚本（确定性函数模拟）+ 字幕/转录配套。
#[derive(Clone)]
pub struct Example {
    pub theme: ExTheme,
    pub lang: &'static str,        // 示例用虚构语种码（如 "zz"）
    pub asset_ids: Vec<&'static str>,
    pub subtitle_key: &'static str, // 字幕配套（域本色）
    pub transcript_key: &'static str, // 转录配套
}

impl Example {
    pub fn subtitle_ready(&self) -> bool { !self.subtitle_key.is_empty() && !self.transcript_key.is_empty() }
}

/// 示例库。
pub struct ExampleBook { pub examples: Vec<Example> }

impl ExampleBook {
    pub fn new() -> Self { ExampleBook { examples: Vec::new() } }

    pub fn add(&mut self, e: Example) -> Result<(), ExCode> {
        if self.examples.iter().any(|x| x.theme == e.theme && x.lang == e.lang) {
            return Err(ExCode::DupExample);
        }
        self.examples.push(e);
        Ok(())
    }

    /// 五主题均衡：接入新语种必须五主题样例齐备——缺则接入阻断。
    pub fn lang_covered(&self, lang: &str) -> bool {
        ExTheme::ALL.iter().all(|t| self.examples.iter().any(|e| e.theme == *t && e.lang == lang))
    }

    /// 接入门：样例齐 + 全部字幕配套。
    pub fn admission_gate(&self, lang: &str) -> Result<(), ExCode> {
        if !self.lang_covered(lang) { return Err(ExCode::BlockNoSample); }
        if !self.examples.iter().filter(|e| e.lang == lang).all(|e| e.subtitle_ready()) {
            return Err(ExCode::BlockNoSample);
        }
        Ok(())
    }

    /// 可执行验收：每条示例的脚本跑通才算（脚本 = 确定性校验闭包注入）。
    pub fn execute_all(&self, script: impl Fn(&Example) -> bool) -> Result<usize, ExCode> {
        let mut n = 0;
        for e in self.examples.iter() {
            if !script(e) { return Err(ExCode::RunFail); }
            n += 1;
        }
        Ok(n)
    }
}

/// 与 F4076 翻译示例对拍：示例键须与正例键（canonical）一致。
pub struct CanonPair { pub example_key: &'static str, pub canonical_key: &'static str }

pub fn canon_check(pairs: &[CanonPair]) -> Result<(), ExCode> {
    for p in pairs.iter() {
        if p.example_key != p.canonical_key { return Err(ExCode::CanonMismatch); }
    }
    Ok(())
}

/// 接入阻断登记。
pub struct BlockCase { pub lang: &'static str, pub missing_theme: &'static str }

pub struct BlockBook { pub cases: Vec<BlockCase> }

impl BlockBook {
    pub fn new() -> Self { BlockBook { cases: Vec::new() } }
    pub fn open(&mut self, lang: &'static str, theme: &'static str) { self.cases.push(BlockCase { lang, missing_theme: theme }); }
    pub fn count(&self) -> usize { self.cases.len() }
}

fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) { set.add(label, ok, detail); }

pub fn run_ves335_checks() -> CheckSet {
    let mut set = CheckSet::new("ves335");
    let mut book = ExampleBook::new();

    let mk = |t: ExTheme| Example {
        theme: t, lang: "zz",
        asset_ids: vec!["fnt-zz-sub", "img-zz-hero", "aud-zz-cue"],
        subtitle_key: "K.SUB.ZZ", transcript_key: "K.TRA.ZZ",
    };

    // C335-01 五主题样例逐条入库。
    for t in ExTheme::ALL.iter() {
        let _ = book.add(mk(*t));
    }
    chk(&mut set, "C335-01", book.examples.len() == 5, "zz 五主题全登");

    // C335-02 重复示例拒。
    chk(&mut set, "C335-02", book.add(mk(ExTheme::HotUpdate)) == Err(ExCode::DupExample), "同主题同语种 => DupExample");

    // C335-03 五主题均衡覆盖判定。
    chk(&mut set, "C335-03", book.lang_covered("zz") && !book.lang_covered("yy"), "zz 齐 yy 缺");

    // C335-04 字幕/转录配套（域本色）。
    chk(&mut set, "C335-04", book.examples.iter().all(|e| e.subtitle_ready()), "五例字幕转录全配套");

    // C335-05 接入门：齐备放行。
    chk(&mut set, "C335-05", book.admission_gate("zz").is_ok(), "zz 接入放行");

    // C335-06 缺样例 → 接入阻断 + 立案。
    let mut bb = BlockBook::new();
    match book.admission_gate("yy") {
        Err(ExCode::BlockNoSample) => bb.open("yy", "all-themes"),
        _ => {}
    }
    chk(&mut set, "C335-06", bb.count() == 1, "yy 缺样例 → 阻断立案");

    // C335-07 可执行验收：脚本全跑通计数。
    let ok_script = |_: &Example| true;
    chk(&mut set, "C335-07", book.execute_all(ok_script) == Ok(5), "五例全跑通");

    // C335-08 示例跑挂即缺陷。
    let bad_script = |e: &Example| e.theme != ExTheme::VerifyActivate;
    chk(&mut set, "C335-08", book.execute_all(bad_script) == Err(ExCode::RunFail), "一例挂 => 示例即缺陷");

    // C335-09 主题名单单源。
    chk(&mut set, "C335-09",
        ExTheme::ALL[0].name() == "font-subset" && ExTheme::ALL[4].name() == "verify-activate",
        "五主题命名单源");

    // C335-10 与 F4076 正例键对拍一致。
    let pairs = [CanonPair { example_key: "K.SUB.ZZ", canonical_key: "K.SUB.ZZ" }];
    chk(&mut set, "C335-10", canon_check(&pairs).is_ok(), "示例键=正例键");

    // C335-11 对拍失配 → 修订立案。
    let bad_pairs = [CanonPair { example_key: "K.SUB.ZZ1", canonical_key: "K.SUB.ZZ" }];
    chk(&mut set, "C335-11", canon_check(&bad_pairs) == Err(ExCode::CanonMismatch), "键漂移 => CanonMismatch");

    // C335-12 示例资产虚构红线：语种码 zz/yy 非真实语言。
    chk(&mut set, "C335-12",
        book.examples.iter().all(|e| e.lang.len() == 2 && e.lang != "zh" && e.lang != "en" && e.lang != "ja"),
        "示例语种全虚构");

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ves335_smoke() {
        let set = run_ves335_checks();
        let red = set.red_items();
        assert!(red.is_empty(), "ves335 red: {:?}", red);
    }

    #[test]
    fn ves335_block() {
        let book = ExampleBook::new();
        assert_eq!(book.admission_gate("zz"), Err(ExCode::BlockNoSample));
    }
}
