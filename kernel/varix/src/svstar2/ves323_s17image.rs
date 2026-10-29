//! ves323_s17image — VE-F4083 · T05 多语言图片资源
//! 含文字图片本地化：分层渲染（底图单源+文字层本地化——体积减半策略）+ 文化适配禁忌清单（手势/符号/颜色×地区）+ 镜像需求标记（T02 RTL 联动）+ 替代文本翻译键（F4071 联动）。
#![allow(dead_code)]
use alloc::vec;
use alloc::vec::Vec;
use alloc::string::String;
use crate::checks::CheckSet;

pub const SEG_BASE: u16 = 0x92C0;

#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ImCode {
    Ok = 0,
    TabooHit = SEG_BASE + 0x01,
    LayerFail = SEG_BASE + 0x02,
    MirrorMissing = SEG_BASE + 0x03,
    AltTextMissing = SEG_BASE + 0x04,
    DupLayer = SEG_BASE + 0x05,
}

/// 分层图片：底图单源 + 文字层（每语种一版）。
#[derive(Clone)]
pub struct LayeredImage {
    pub id: u32,
    pub base_kb: u32,
    /// 每文字层体积 KB。
    pub text_layer_kb: u32,
    pub layers: Vec<(&'static str, &'static str)>, // (语种码, 文字层资产 id)
    /// 替代文本翻译键（无障碍必挂）。
    pub alt_key: &'static str,
    /// 镜像需求标记（RTL 需镜像语义）。
    pub mirror_needed: bool,
}

impl LayeredImage {
    /// 整图重绘体积 = 底图 × 语种数；分层体积 = 底图 + 文字层 × 语种数。
    pub fn monolithic_kb(&self) -> u32 { self.base_kb * self.layers.len().max(1) as u32 }
    pub fn layered_kb(&self) -> u32 { self.base_kb + self.text_layer_kb * self.layers.len() as u32 }
    /// 体积减半策略：分层必须优于整图一半。
    pub fn halves_size(&self) -> bool {
        let mono = self.monolithic_kb() as u64;
        let lay = self.layered_kb() as u64;
        lay * 2 <= mono
    }
    pub fn has_alt(&self) -> bool { !self.alt_key.is_empty() }
}

pub struct ImageBook { pub images: Vec<LayeredImage> }

impl ImageBook {
    pub fn new() -> Self { ImageBook { images: Vec::new() } }

    pub fn add(&mut self, img: LayeredImage) -> Result<(), ImCode> {
        if !img.has_alt() { return Err(ImCode::AltTextMissing); }
        // 文字层语种重复 = 同语种重绘层冲突。
        for i in 0..img.layers.len() {
            for j in (i + 1)..img.layers.len() {
                if img.layers[i].0 == img.layers[j].0 { return Err(ImCode::DupLayer); }
            }
        }
        self.images.push(img);
        Ok(())
    }

    pub fn find(&self, id: u32) -> Option<&LayeredImage> { self.images.iter().find(|i| i.id == id) }

    /// 分层失败 → 降级整图，登记体积代价。
    pub fn downgrade_monolithic(&mut self, id: u32) -> Option<u32> {
        let img = self.find(id)?;
        let cost = img.layered_kb(); // 降级前分层体积
        Some(img.monolithic_kb() - cost)
    }
}

/// 禁忌类别。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TabooKind { Gesture = 0, Symbol = 1, Color = 2 }

impl TabooKind {
    pub const ALL: [TabooKind; 3] = [TabooKind::Gesture, TabooKind::Symbol, TabooKind::Color];
    pub fn name(self) -> &'static str {
        match self { TabooKind::Gesture => "gesture", TabooKind::Symbol => "symbol", TabooKind::Color => "color" }
    }
}

/// 一条禁忌：地区码 × 类别 × 标记词。
#[derive(Clone)]
pub struct Taboo { pub region: &'static str, pub kind: TabooKind, pub token: &'static str }

pub struct TabooBook { pub entries: Vec<Taboo> }

impl TabooBook {
    pub fn new() -> Self { TabooBook { entries: Vec::new() } }

    pub fn add(&mut self, t: Taboo) { self.entries.push(t); }

    /// 审查：图片标签串在目标地区是否命中禁忌（token 子串匹配）。
    pub fn screen(&self, region: &'static str, tag: &str) -> Result<(), (TabooKind, &'static str)> {
        for t in self.entries.iter() {
            if t.region == region && tag.contains(t.token) {
                return Err((t.kind, t.token));
            }
        }
        Ok(())
    }

    /// 某地区禁忌数（审查 O(地区) 的规模口径）。
    pub fn count_region(&self, region: &str) -> usize {
        self.entries.iter().filter(|t| t.region == region).count()
    }
}

/// 镜像标记表：图片 id → 是否已标 RTL 镜像。
pub struct MirrorTable { pub marks: Vec<(u32, bool)> }

impl MirrorTable {
    pub fn new() -> Self { MirrorTable { marks: Vec::new() } }

    pub fn mark(&mut self, id: u32, mirrored: bool) {
        for m in self.marks.iter_mut() {
            if m.0 == id { m.1 = mirrored; return; }
        }
        self.marks.push((id, mirrored));
    }

    pub fn mirrored(&self, id: u32) -> bool {
        self.marks.iter().find(|m| m.0 == id).map_or(false, |m| m.1)
    }

    /// RTL 审查拦截：镜像需求图未标镜像 → 拦截。
    pub fn rtl_gate(&self, img: &LayeredImage) -> Result<(), ImCode> {
        if img.mirror_needed && !self.mirrored(img.id) { return Err(ImCode::MirrorMissing); }
        Ok(())
    }
}

fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) { set.add(label, ok, detail); }

pub fn run_ves323_checks() -> CheckSet {
    let mut set = CheckSet::new("ves323");

    // 夹具：底图 400KB + 文字层 60KB × 4 语种。
    let img = LayeredImage {
        id: 1, base_kb: 400, text_layer_kb: 60,
        layers: vec![("en", "t-en"), ("zh", "t-zh"), ("ar", "t-ar"), ("th", "t-th")],
        alt_key: "K.IMG.HERO", mirror_needed: true,
    };

    // C4083-01 体积减半：分层 640 < 整图 1600/2=800。
    chk(&mut set, "C4083-01", img.halves_size() && img.layered_kb() == 640, "分层 640KB ≤ 整图 1600KB 之半");

    // C4083-02 替代文本缺 → 拒入库。
    let mut book = ImageBook::new();
    let mut bad = img.clone();
    bad.alt_key = "";
    chk(&mut set, "C4083-02", book.add(bad) == Err(ImCode::AltTextMissing), "无替代文本键 => 拒");

    // C4083-03 语种层重复 → 拒。
    let mut dup = img.clone();
    dup.layers = vec![("en", "t-en-a"), ("en", "t-en-b")];
    chk(&mut set, "C4083-03", book.add(dup) == Err(ImCode::DupLayer), "同语种双文字层 => DupLayer");

    // C4083-04 正常入库 + 查询。
    chk(&mut set, "C4083-04", book.add(img.clone()).is_ok() && book.find(1).is_some(), "入库后可查");

    // C4083-05 降级整图体积代价登记：1600-640=960。
    chk(&mut set, "C4083-05", book.downgrade_monolithic(1) == Some(960), "降级代价 960KB");

    // C4083-06 禁忌清单三类别登记。
    let mut tb = TabooBook::new();
    tb.add(Taboo { region: "SA", kind: TabooKind::Gesture, token: "thumbs-up-left" });
    tb.add(Taboo { region: "SA", kind: TabooKind::Symbol, token: "star-hex" });
    tb.add(Taboo { region: "JP", kind: TabooKind::Color, token: "white-carnation" });
    chk(&mut set, "C4083-06", tb.count_region("SA") == 2 && tb.count_region("JP") == 1, "三类别两地区登记");

    // C4083-07 禁忌命中 → 替换评审。
    chk(&mut set, "C4083-07", tb.screen("SA", "hero-thumbs-up-left-v2") == Err((TabooKind::Gesture, "thumbs-up-left")), "手势禁忌命中");

    // C4083-08 未命中地区放行。
    chk(&mut set, "C4083-08", tb.screen("US", "hero-thumbs-up-left-v2").is_ok(), "US 无此禁忌放行");

    // C4083-09 镜像拦截：需镜像未标 → RTL 审查拦截。
    let mut mt = MirrorTable::new();
    chk(&mut set, "C4083-09", mt.rtl_gate(&img) == Err(ImCode::MirrorMissing), "镜像漏标 => 拦截");

    // C4083-10 标记后放行。
    mt.mark(1, true);
    chk(&mut set, "C4083-10", mt.mirrored(1) && mt.rtl_gate(&img).is_ok(), "标记镜像后过闸");

    // C4083-11 覆盖 mark 幂等。
    mt.mark(1, false);
    mt.mark(1, true);
    chk(&mut set, "C4083-11", mt.marks.len() == 1 && mt.mirrored(1), "重复标记幂等单条");

    // C4083-12 替代文本键与 F4071 键命名风格衔接（K. 前缀）。
    chk(&mut set, "C4083-12", img.alt_key.starts_with("K.") && !img.alt_key.is_empty(), "替代文本走翻译键");

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ves323_smoke() {
        let set = run_ves323_checks();
        assert!(set.failed_count() == 0, "ves323 red: {}", set.red_items_text());
    }

    #[test]
    fn ves323_layer_half() {
        let img = LayeredImage { id: 2, base_kb: 100, text_layer_kb: 10,
            layers: vec![("en", "a"), ("zh", "b")], alt_key: "K.X", mirror_needed: false };
        assert!(img.halves_size());
        assert_eq!(img.layered_kb(), 120);
        assert_eq!(img.monolithic_kb(), 200);
    }
}
