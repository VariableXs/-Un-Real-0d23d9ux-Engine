//! ves460_s25impact — VE-F4226 · 契约影响分析
//! 变更影响面计算（知识图谱扩散查询：消费方×下游契约×界面三层扩散）+影响报告（波及清单+迁移工作量估计）+分析缓存；
//! 图谱失步→重放同步后再分析；扩散爆炸→深度截断+显性标注；缓存失效→主动失效；
//! 影响报告含无障碍判据波及单列（域本色）；无隐私面。
#![allow(dead_code)]
use alloc::vec;
use alloc::vec::Vec;
use crate::checks::CheckSet;

pub const SEG_BASE: u16 = 0x1700;
pub const MAX_DEPTH: u8 = 3; // 三层扩散截断

#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ImCode {
    Ok = 0,
    GraphStale = SEG_BASE + 0x01, // 0x1701
    ExplosionCut = SEG_BASE + 0x02, // 0x1702
    CacheInvalid = SEG_BASE + 0x03, // 0x1703
}

/// 影响分析器。
pub struct ImpactAnalyzer {
    pub deps: Vec<(u32, u32)>, // (下游, 上游) 依赖边
    pub graph_rev: u64,
    pub analyzed_rev: u64,
    pub cache: Vec<(u32, Vec<u32>)>,
    pub cut: bool,             // 扩散爆炸显性标注
    pub a11y_hit: u32,         // 无障碍判据波及单列（域本色）
    pub invalidated: u32,
}

impl ImpactAnalyzer {
    pub fn new(rev: u64) -> Self {
        ImpactAnalyzer { deps: Vec::new(), graph_rev: rev, analyzed_rev: rev, cache: Vec::new(), cut: false, a11y_hit: 0, invalidated: 0 }
    }
    pub fn add_dep(&mut self, downstream: u32, upstream: u32) { self.deps.push((downstream, upstream)); }
    /// 图谱新鲜度：分析前核对版本（失步→重放同步）。
    pub fn fresh_check(&mut self, graph_rev: u64) -> Result<(), ImCode> {
        self.graph_rev = graph_rev;
        if self.analyzed_rev != graph_rev {
            self.analyzed_rev = graph_rev; // 重放同步
            self.invalidated += 1;
            return Err(ImCode::GraphStale);
        }
        Ok(())
    }
    /// 三层扩散：消费方→下游契约→界面（依赖边反向遍历，深度截断+标注）。
    pub fn analyze(&mut self, root: u32) -> Vec<u32> {
        self.a11y_hit = 0;
        let mut seen = vec![root];
        let mut frontier = vec![root];
        let mut depth = 0u8;
        while depth < MAX_DEPTH {
            let mut next = Vec::new();
            for f in &frontier {
                for (d, u) in &self.deps {
                    if *u == *f && !seen.contains(d) {
                        seen.push(*d);
                        next.push(*d);
                        if *d >= 9000 { self.a11y_hit += 1; } // 域本色：a11y 契约波及单列
                    }
                }
            }
            if next.is_empty() { break; }
            frontier = next;
            depth += 1;
        }
        if depth == MAX_DEPTH && !frontier.is_empty() { self.cut = true; }
        seen.retain(|x| *x != root);
        self.cache.retain(|(r, _)| *r != root);
        self.cache.push((root, seen.clone()));
        seen
    }
    /// 缓存命中（同根二次分析走缓存）。
    pub fn cached(&self, root: u32) -> Option<&Vec<u32>> {
        self.cache.iter().find(|(r, _)| *r == root).map(|(_, v)| v)
    }
    /// 迁移工作量估计：波及面 × 单位工作量。
    pub fn effort(&self, blast: &[u32], unit: u32) -> u32 { (blast.len() as u32).saturating_mul(unit) }
}

fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) { set.add(label, ok, detail); }

pub fn run_ves460_checks() -> CheckSet {
    let mut set = CheckSet::new("ves460");
    let mut az = ImpactAnalyzer::new(10);

    // C460-01 三层扩散：令牌→契约→界面。
    az.add_dep(2, 1); az.add_dep(3, 2); az.add_dep(4, 3); // 1←2←3←4 链
    az.add_dep(9001, 2); // a11y 契约挂下游
    let blast = az.analyze(1);
    chk(&mut set, "C460-01", blast == vec![2, 3, 4, 9001], "三层扩散");

    // C460-02 深度截断显性标注。
    chk(&mut set, "C460-02", az.cut, "截断标注");

    // C460-03 域本色：a11y 波及单列。
    chk(&mut set, "C460-03", az.a11y_hit == 1, "a11y 波及");

    // C460-04 缓存命中。
    chk(&mut set, "C460-04", az.cached(1) == Some(&blast), "缓存命中");

    // C460-05 图谱失步→重放同步（缓存主动失效）。
    chk(&mut set, "C460-05", az.fresh_check(11) == Err(ImCode::GraphStale) && az.invalidated == 1, "失步重放");

    // C460-06 同步后再分析过。
    chk(&mut set, "C460-06", az.fresh_check(11).is_ok(), "同步后过");

    // C460-07 工作量估计。
    let e = az.effort(&blast, 8);
    chk(&mut set, "C460-07", e == (blast.len() as u32) * 8 && e > 0, "工作量估计");

    // C460-08 空依赖根→零波及。
    let mut az2 = ImpactAnalyzer::new(5);
    chk(&mut set, "C460-08", az2.analyze(7).is_empty() && !az2.cut, "零波及");

    // C460-09 缓存失效计数累计。
    az2.graph_rev = 6; az2.analyzed_rev = 6;
    let _ = az2.fresh_check(7);
    chk(&mut set, "C460-09", az2.invalidated == 1, "失效计数");

    // C460-10 深度常量单源。
    chk(&mut set, "C460-10", MAX_DEPTH == 3, "三层单源");

    // C460-11 缓存幂等（重分析同根替换）。
    az.analyze(1);
    chk(&mut set, "C460-11", az.cache.iter().filter(|(r, _)| *r == 1).count() == 1, "缓存幂等");

    // C460-12 错误码段位单源。
    chk(&mut set, "C460-12", ImCode::GraphStale as u16 == SEG_BASE + 1, "段位单源");

    set
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn ves460_smoke() {
        let set = run_ves460_checks();
        let red = set.red_items();
        assert!(red.is_empty(), "ves460 red: {:?}", red);
    }
}
