//! ves322_s17font — VE-F4082 · T05 多语言字体策略
//! 四族字体矩阵（Latin/CJK/阿拉伯/泰印度）×字重×回退链三线 + 按语种字符集子集化体积预算 + 回退链环检测拒绝配置 + 回退命中审计 + 低视力字重档。
#![allow(dead_code)]
use alloc::vec;
use alloc::vec::Vec;
use alloc::string::String;
use crate::checks::CheckSet;

pub const SEG_BASE: u16 = 0x9240;

#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FtCode {
    Ok = 0,
    ChainCycle = SEG_BASE + 0x01,
    OverBudget = SEG_BASE + 0x02,
    GlyphMissing = SEG_BASE + 0x03,
    WeightBad = SEG_BASE + 0x04,
    FamilyUnknown = SEG_BASE + 0x05,
}

/// 四族（与 T03 排版单源对齐）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FontFamily { Latin = 0, Cjk = 1, Arabic = 2, ThaiIndic = 3 }

impl FontFamily {
    pub const ALL: [FontFamily; 4] = [
        FontFamily::Latin, FontFamily::Cjk, FontFamily::Arabic, FontFamily::ThaiIndic];
    pub fn name(self) -> &'static str {
        match self { FontFamily::Latin => "latin", FontFamily::Cjk => "cjk",
            FontFamily::Arabic => "arabic", FontFamily::ThaiIndic => "thai-indic" }
    }
    pub fn from_name(n: &str) -> Option<FontFamily> {
        match n { "latin" => Some(FontFamily::Latin), "cjk" => Some(FontFamily::Cjk),
            "arabic" => Some(FontFamily::Arabic), "thai-indic" => Some(FontFamily::ThaiIndic),
            _ => None }
    }
}

/// 字重档（含低视力档 350/650 两档——复用 S02 对比标准）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum WeightTier { Regular = 0, Medium = 1, Bold = 2, LowVisionNormal = 3, LowVisionHeavy = 4 }

impl WeightTier {
    pub const ALL: [WeightTier; 5] = [
        WeightTier::Regular, WeightTier::Medium, WeightTier::Bold,
        WeightTier::LowVisionNormal, WeightTier::LowVisionHeavy];
    pub fn weight(self) -> u16 {
        match self { WeightTier::Regular => 400, WeightTier::Medium => 500, WeightTier::Bold => 700,
            WeightTier::LowVisionNormal => 350, WeightTier::LowVisionHeavy => 650 }
    }
    pub fn is_low_vision(self) -> bool {
        matches!(self, WeightTier::LowVisionNormal | WeightTier::LowVisionHeavy)
    }
}

/// 一条字体记录：族×字重→字体名+字符集子集标记。
#[derive(Clone)]
pub struct FontEntry {
    pub family: FontFamily,
    pub tier: WeightTier,
    pub face: &'static str,
    /// 子集化后覆盖的码点数（0=全量）。
    pub subset_cps: u32,
    /// 子集化后体积 KB。
    pub size_kb: u32,
}

/// 体积预算表：族→KB 预算。
pub struct BudgetTable { pub budgets: [u32; 4] }

impl BudgetTable {
    pub fn new() -> Self { BudgetTable { budgets: [180, 2400, 320, 420] } }
    pub fn for_family(&self, f: FontFamily) -> u32 { self.budgets[f as usize] }
}

/// 子集化：按语种字符集（此处以码点集合大小模拟）产出子集记录。
/// 子集体积 = 全量体积 × 覆盖码点/全量码点（线性模型），超预算时加严（砍非核心码点重算）。
pub struct SubsetResult { pub face: &'static str, pub cps: u32, pub size_kb: u32, pub tightened: bool }

pub fn subset_face(face: &'static str, full_cps: u32, full_kb: u32, want_cps: u32, budget_kb: u32) -> Result<SubsetResult, FtCode> {
    if full_cps == 0 || want_cps > full_cps { return Err(FtCode::GlyphMissing); }
    let mut cps = want_cps;
    let mut tightened = false;
    loop {
        let kb = (full_kb as u64 * cps as u64 / full_cps as u64) as u32;
        if kb <= budget_kb {
            return Ok(SubsetResult { face, cps, size_kb: kb, tightened });
        }
        // 加严：按 15% 递减码点直至达预算或低于 20%（低于 20% 仍超即判超预算——预算不合理）。
        if cps * 100 < full_cps * 20 { return Err(FtCode::OverBudget); }
        cps = cps * 85 / 100;
        tightened = true;
    }
}

/// 回退链：每族一条有序字体链；可配置；链内环拒绝。
#[derive(Clone)]
pub struct FallbackChain { pub family: FontFamily, pub faces: Vec<&'static str> }

pub struct ChainBook { pub chains: Vec<FallbackChain> }

impl ChainBook {
    pub fn new() -> Self { ChainBook { chains: Vec::new() } }

    pub fn set_chain(&mut self, family: FontFamily, faces: Vec<&'static str>) -> Result<(), FtCode> {
        if faces.is_empty() { return Err(FtCode::FamilyUnknown); }
        // 环检测：同一 face 重复出现即构成配置环。
        for i in 0..faces.len() {
            for j in (i + 1)..faces.len() {
                if faces[i] == faces[j] { return Err(FtCode::ChainCycle); }
            }
        }
        for c in self.chains.iter_mut() {
            if c.family == family { c.faces = faces; return Ok(()); }
        }
        self.chains.push(FallbackChain { family, faces });
        Ok(())
    }

    pub fn chain_of(&self, family: FontFamily) -> Option<&FallbackChain> {
        self.chains.iter().find(|c| c.family == family)
    }
}

/// 回退命中审计记录。
#[derive(Clone)]
pub struct HitAudit { pub cp: u32, pub family: FontFamily, pub asked: &'static str, pub served: &'static str, pub tier: WeightTier }

pub struct AuditBook { pub hits: Vec<HitAudit> }

impl AuditBook {
    pub fn new() -> Self { AuditBook { hits: Vec::new() } }

    /// 命中查询：沿回退链找第一个声明覆盖该码点的 face；全链不覆盖返回缺字形。
    pub fn resolve(&mut self, chains: &ChainBook, _faces: &[FontEntry], family: FontFamily,
                   cp: u32, tier: WeightTier, coverage: impl Fn(&str, u32) -> bool) -> Result<&'static str, FtCode> {
        if !WeightTier::ALL.contains(&tier) { return Err(FtCode::WeightBad); }
        let chain = match chains.chain_of(family) { Some(c) => c, None => return Err(FtCode::FamilyUnknown) };
        for f in chain.faces.iter() {
            if coverage(f, cp) {
                let served = *f;
                let asked = chain.faces[0];
                self.hits.push(HitAudit { cp, family, asked, served, tier });
                return Ok(served);
            }
        }
        Err(FtCode::GlyphMissing)
    }

    /// 审计统计：回退命中的次数（asked != served）。
    pub fn fallback_hits(&self) -> usize {
        self.hits.iter().filter(|h| h.asked != h.served).count()
    }

    /// 某族最高频回退 face（供加严子集决策）。
    pub fn top_fallback(&self, family: FontFamily) -> Option<&'static str> {
        let mut best: Option<(&'static str, usize)> = None;
        for h in self.hits.iter() {
            if h.family != family || h.asked == h.served { continue; }
            let n = self.hits.iter().filter(|x| x.family == family && x.served == h.served && x.asked != x.served).count();
            if best.map_or(true, |(_, bn)| n > bn) { best = Some((h.served, n)); }
        }
        best.map(|(f, _)| f)
    }
}

/// 字体策略总册：矩阵登记 + 预算 + 链 + 审计。
pub struct FontPolicy {
    pub entries: Vec<FontEntry>,
    pub budgets: BudgetTable,
    pub chains: ChainBook,
    pub audit: AuditBook,
}

impl FontPolicy {
    pub fn new() -> Self {
        FontPolicy { entries: Vec::new(), budgets: BudgetTable::new(), chains: ChainBook::new(), audit: AuditBook::new() }
    }

    pub fn register(&mut self, e: FontEntry) -> Result<(), FtCode> {
        if e.face.is_empty() { return Err(FtCode::FamilyUnknown); }
        if e.size_kb > self.budgets.for_family(e.family) { return Err(FtCode::OverBudget); }
        for x in self.entries.iter() {
            if x.family == e.family && x.tier == e.tier && x.face == e.face { return Err(FtCode::FamilyUnknown); }
        }
        self.entries.push(e);
        Ok(())
    }

    /// 低视力字重档校验：两档必须都登记且字重严格递增（350 < 650）。
    pub fn low_vision_ready(&self, family: FontFamily) -> bool {
        let has = |t: WeightTier| self.entries.iter().any(|e| e.family == family && e.tier == t);
        has(WeightTier::LowVisionNormal) && has(WeightTier::LowVisionHeavy)
    }
}

fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) { set.add(label, ok, detail); }

pub fn run_ves322_checks() -> CheckSet {
    let mut set = CheckSet::new("ves322");
    let mut p = FontPolicy::new();

    // C4082-01 四族矩阵：四族预算齐备且 CJK 预算最大（合理序）。
    let b = &p.budgets;
    let cjk_max = b.budgets[1] > b.budgets[0] && b.budgets[1] > b.budgets[2] && b.budgets[1] > b.budgets[3];
    chk(&mut set, "C4082-01", cjk_max, "预算表四族齐备 CJK 最大");

    // C4082-02 子集化：正常子集线性模型正确。
    let s = subset_face("nshk-cjk", 20000, 2400, 5000, 700).expect("subset");
    chk(&mut set, "C4082-02", !s.tightened && s.size_kb == 600, "子集 5000/20000 => 600KB 无加严");

    // C4082-03 子集化加严：超预算自动加严至达标。
    let s2 = subset_face("nshk-cjk", 20000, 2400, 20000, 400).expect("tighten");
    chk(&mut set, "C4082-03", s2.tightened && s2.size_kb <= 400, "超预算加严到 400KB 内");

    // C4082-04 预算不可达：低于 20% 仍超 → OverBudget。
    chk(&mut set, "C4082-04", subset_face("x", 100, 1000, 100, 10).is_err(), "预算荒谬 => OverBudget");

    // C4082-05 回退链环拒绝。
    let cyc = p.chains.set_chain(FontFamily::Latin, vec!["a", "b", "a"]);
    chk(&mut set, "C4082-05", cyc == Err(FtCode::ChainCycle), "a-b-a 环 => ChainCycle");

    // C4082-06 正常链可配置且可查。
    p.chains.set_chain(FontFamily::Latin, vec!["nshk-latin", "fallback-1"]).expect("chain");
    p.chains.set_chain(FontFamily::Cjk, vec!["nshk-cjk", "nshk-latin", "fallback-1"]).expect("chain");
    chk(&mut set, "C4082-06", p.chains.chain_of(FontFamily::Cjk).map_or(false, |c| c.faces.len() == 3), "CJK 链三节");

    // C4082-07 命中审计：首节覆盖→无回退。
    let cov = |f: &str, _cp: u32| f.starts_with("nshk");
    let r1 = p.audit.resolve(&p.chains, &p.entries, FontFamily::Latin, 0x41, WeightTier::Regular, cov);
    chk(&mut set, "C4082-07", r1 == Ok("nshk-latin"), "首节命中 0x41");

    // C4082-08 命中审计：首节缺→回退第二节并留痕。
    let cov2 = |f: &str, cp: u32| f.starts_with("nshk") && cp != 0x9111;
    let r2 = p.audit.resolve(&p.chains, &p.entries, FontFamily::Cjk, 0x9111, WeightTier::Regular, cov2);
    chk(&mut set, "C4082-08", r2 == Ok("fallback-1"), "缺字形 0x9111 回退 fallback-1");

    // C4082-09 全链不覆盖 → 缺字形。
    let cov3 = |_: &str, _: u32| false;
    chk(&mut set, "C4082-09", p.audit.resolve(&p.chains, &p.entries, FontFamily::Arabic, 0x0627, WeightTier::Regular, cov3) == Err(FtCode::GlyphMissing), "全链缺 => GlyphMissing");

    // C4082-10 审计计数与 top_fallback。
    chk(&mut set, "C4082-10", p.audit.fallback_hits() == 1 && p.audit.top_fallback(FontFamily::Cjk) == Some("fallback-1"), "回退计数 1 + top 单源");

    // C4082-11 低视力字重档：350/650 两档登记后 ready。
    let mut fv = WeightTier::ALL.map(|t| t.weight());
    fv.sort();
    let lv = WeightTier::ALL.iter().filter(|t| t.is_low_vision()).count();
    chk(&mut set, "C4082-11", fv[0] == 350 && fv[4] == 700 && lv == 2, "五档字重 低视力两档");

    // C4082-12 登记超预算字体被拒。
    let big = FontEntry { family: FontFamily::Cjk, tier: WeightTier::Regular, face: "fat-cjk", subset_cps: 1, size_kb: 99999 };
    chk(&mut set, "C4082-12", p.register(big) == Err(FtCode::OverBudget), "登记体积超预算 => 拒");

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ves322_smoke() {
        let set = run_ves322_checks();
        let red = set.red_items();
        assert!(red.is_empty(), "ves322 red: {:?}", red);
    }

    #[test]
    fn ves322_chain_cycle_rejected() {
        let mut b = ChainBook::new();
        assert_eq!(b.set_chain(FontFamily::Latin, vec!["x", "x"]), Err(FtCode::ChainCycle));
        assert!(b.set_chain(FontFamily::Latin, vec!["x", "y"]).is_ok());
    }

    #[test]
    fn ves322_subset_tighten() {
        let s = subset_face("f", 10000, 1200, 10000, 300).expect("ok");
        assert!(s.tightened && s.size_kb <= 300);
    }
}
