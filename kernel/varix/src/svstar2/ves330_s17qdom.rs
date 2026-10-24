//! ves330_s17qdom — VE-F4090 · T05 语言资源与 Q 域联动
//! Q 域渲染能力对多语言资产的支撑面：字体渲染管线/图集管理/纹理压缩格式 × 语种矩阵适配 + 能力缺口登记转 Q 域排期（不越界实现——登记即止）+ 能力不支持给降级方案 + 缺口长期未解升级评审 + 高对比模式字体兼容核验（域本色）。
#![allow(dead_code)]
use alloc::vec;
use alloc::vec::Vec;
use alloc::string::String;
use crate::checks::CheckSet;

pub const SEG_BASE: u16 = 0x9640;

#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum QdCode {
    Ok = 0,
    CapabilityMissing = SEG_BASE + 0x01,
    Overreach = SEG_BASE + 0x02,
    StaleGap = SEG_BASE + 0x03,
    HcMismatch = SEG_BASE + 0x04,
}

/// 三个联动面。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Face { GlyphPipeline = 0, Atlas = 1, TexCompress = 2 }

impl Face {
    pub const ALL: [Face; 3] = [Face::GlyphPipeline, Face::Atlas, Face::TexCompress];
    pub fn name(self) -> &'static str {
        match self { Face::GlyphPipeline => "glyph-pipeline", Face::Atlas => "atlas", Face::TexCompress => "tex-compress" }
    }
}

/// Q 域能力声明：面 × 能力级（0 不支持/1 部分/2 全量）。
pub struct Capability { pub face: Face, pub level: u8 }

/// 语种适配需求：面 × 语种 → 需要的能力级。
pub struct Need { pub face: Face, pub lang: &'static str, pub level: u8 }

/// 联动面册：声明 × 需求 → 矩阵适配（判级：需求 ≤ 声明则绿）。
pub struct LinkMatrix {
    pub caps: Vec<Capability>,
    pub needs: Vec<Need>,
}

impl LinkMatrix {
    pub fn new() -> Self { LinkMatrix { caps: Vec::new(), needs: Vec::new() } }

    pub fn declare(&mut self, face: Face, level: u8) { self.caps.push(Capability { face, level }); }
    pub fn require(&mut self, face: Face, lang: &'static str, level: u8) { self.needs.push(Need { face, lang, level }); }

    pub fn cap_of(&self, face: Face) -> u8 {
        self.caps.iter().find(|c| c.face == face).map_or(0, |c| c.level)
    }

    /// 矩阵适配：单需求判定。
    pub fn fit(&self, n: &Need) -> Result<(), QdCode> {
        if self.cap_of(n.face) >= n.level { Ok(()) } else { Err(QdCode::CapabilityMissing) }
    }

    /// 全矩阵扫描：不满足的需求清单。
    pub fn unfit_langs(&self) -> Vec<&'static str> {
        self.needs.iter().filter(|n| self.fit(n).is_err()).map(|n| n.lang).collect()
    }
}

/// 缺口登记：登记转 Q 域排期——绝不越界实现（登记结构本身只含转办信息）。
#[derive(Clone)]
pub struct GapEntry {
    pub face: Face,
    pub lang: &'static str,
    /// 降级方案（能力缺时的替代路径，域内可守）。
    pub fallback: &'static str,
    /// 登记轮次（用于长期未解升级判定）。
    pub rounds_open: u32,
}

pub const STALE_ROUNDS: u32 = 6;

pub struct GapBook { pub entries: Vec<GapEntry> }

impl GapBook {
    pub fn new() -> Self { GapBook { entries: Vec::new() } }

    /// 登记：必须带降级方案；否则视为越界实现倾向拒收。
    pub fn register(&mut self, face: Face, lang: &'static str, fallback: &'static str) -> Result<(), QdCode> {
        if fallback.is_empty() { return Err(QdCode::Overreach); }
        if let Some(e) = self.entries.iter_mut().find(|e| e.face == face && e.lang == lang) {
            e.rounds_open += 1;
            return Ok(());
        }
        self.entries.push(GapEntry { face, lang, fallback, rounds_open: 1 });
        Ok(())
    }

    /// 长期未解升级评审。
    pub fn stale(&self) -> Vec<&'static str> {
        self.entries.iter().filter(|e| e.rounds_open >= STALE_ROUNDS).map(|e| e.lang).collect()
    }

    pub fn count(&self) -> usize { self.entries.len() }
}

/// 越界守卫：语言资源侧只登记不实现——本册 API 面没有"实现"入口（结构即证明），配运行时标记。
pub struct OverreachGuard { pub implemented_in_t05: Vec<&'static str> }

impl OverreachGuard {
    pub fn new() -> Self { OverreachGuard { implemented_in_t05: Vec::new() } }
    /// T05 侧不得登记任何 Q 域实现——调用即记录违规（测试面断言为空）。
    pub fn flag(&mut self, what: &'static str) { self.implemented_in_t05.push(what); }
    pub fn clean(&self) -> bool { self.implemented_in_t05.is_empty() }
}

/// 高对比模式字体兼容核验（域本色）：四族字重 700 档在 HC 下声明可用。
pub struct HcFontCheck { pub hc_ready: Vec<&'static str> } // 族名

impl HcFontCheck {
    pub fn new() -> Self { HcFontCheck { hc_ready: Vec::new() } }
    pub fn mark(&mut self, family: &'static str) {
        if !self.hc_ready.contains(&family) { self.hc_ready.push(family); }
    }
    pub fn verify(&self, families: &[&'static str]) -> Result<(), QdCode> {
        for f in families.iter() {
            if !self.hc_ready.contains(f) { return Err(QdCode::HcMismatch); }
        }
        Ok(())
    }
}

fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) { set.add(label, ok, detail); }

pub fn run_ves330_checks() -> CheckSet {
    let mut set = CheckSet::new("ves330");
    let mut m = LinkMatrix::new();

    // 夹具：Q 域声明——管线全量 2、图集部分 1、压缩全量 2。
    m.declare(Face::GlyphPipeline, 2);
    m.declare(Face::Atlas, 1);
    m.declare(Face::TexCompress, 2);

    // C4090-01 三面适配：需求 ≤ 声明全绿。
    m.require(Face::GlyphPipeline, "zh", 2);
    m.require(Face::Atlas, "ar", 1);
    m.require(Face::TexCompress, "th", 2);
    chk(&mut set, "C4090-01", m.unfit_langs().is_empty(), "三面需求全满足");

    // C4090-02 图集缺口：CJK 全量需求 2 > 声明 1 → 缺口。
    m.require(Face::Atlas, "zh", 2);
    chk(&mut set, "C4090-02", m.unfit_langs() == vec!["zh"], "图集声明 1 < 需求 2 → 缺");

    // C4090-03 缺口登记必须带降级方案。
    let mut gb = GapBook::new();
    let nofallback = gb.register(Face::Atlas, "zh", "");
    chk(&mut set, "C4090-03", nofallback == Err(QdCode::Overreach), "无降级方案 => 拒收登记");

    // C4090-04 合法登记 + 降级方案可读。
    let _ = gb.register(Face::Atlas, "zh", "位图缓存降级");
    chk(&mut set, "C4090-04", gb.count() == 1, "登记转 Q 域排期");

    // C4090-05 重复登记轮次累计。
    let _ = gb.register(Face::Atlas, "zh", "位图缓存降级");
    chk(&mut set, "C4090-05", gb.count() == 1 && gb.entries[0].rounds_open == 2, "同缺口轮次累计");

    // C4090-06 长期未解升级评审（>=6 轮）。
    for _ in 0..5 { let _ = gb.register(Face::Atlas, "zh", "位图缓存降级"); }
    chk(&mut set, "C4090-06", gb.entries[0].rounds_open == 7 && gb.stale() == vec!["zh"], "7 轮未解 → 升级");

    // C4090-07 未到升级线不升。
    let mut gb2 = GapBook::new();
    let _ = gb2.register(Face::TexCompress, "ko", "RLE 降级");
    chk(&mut set, "C4090-07", gb2.stale().is_empty(), "1 轮不升级");

    // C4090-08 不越界守卫：T05 实现面为空。
    let mut og = OverreachGuard::new();
    chk(&mut set, "C4090-08", og.clean(), "T05 无 Q 域实现");

    // C4090-09 越界即记违规并可断言。
    og.flag("atlas-evict-lru");
    chk(&mut set, "C4090-09", !og.clean(), "flag 后不干净（守卫可测）");

    // C4090-10 能力单源复述：cap_of 三面读数。
    chk(&mut set, "C4090-10",
        m.cap_of(Face::GlyphPipeline) == 2 && m.cap_of(Face::Atlas) == 1 && m.cap_of(Face::TexCompress) == 2,
        "三面声明单源");

    // C4090-11 未声明面需求 → 缺口。
    let mut m2 = LinkMatrix::new();
    m2.declare(Face::GlyphPipeline, 2);
    m2.require(Face::Atlas, "de", 1);
    chk(&mut set, "C4090-11", m2.fit(&m2.needs[0]) == Err(QdCode::CapabilityMissing), "未声明面 => 缺口");

    // C4090-12 高对比模式字体兼容核验：四族齐备过闸，缺族拒。
    let mut hc = HcFontCheck::new();
    hc.mark("latin"); hc.mark("cjk"); hc.mark("arabic");
    chk(&mut set, "C4090-12",
        hc.verify(&["latin", "cjk", "arabic"]).is_err()
        && { hc.mark("thai-indic"); hc.verify(&["latin", "cjk", "arabic", "thai-indic"]).is_ok() },
        "缺 thai-indic 拒 + 补齐过闸");

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ves330_smoke() {
        let set = run_ves330_checks();
        let red = set.red_items();
        assert!(red.is_empty(), "ves330 red: {:?}", red);
    }

    #[test]
    fn ves330_matrix() {
        let mut m = LinkMatrix::new();
        m.declare(Face::Atlas, 1);
        m.require(Face::Atlas, "fr", 1);
        assert!(m.fit(&m.needs[0]).is_ok());
        m.require(Face::Atlas, "en", 2);
        assert_eq!(m.unfit_langs(), vec!["en"]);
    }
}
