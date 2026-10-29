//! ves357_s19rdom — VE-F4117 · T06 地区与 R 域个性化联动
//! 个性化系统对地区维度的消费面：地区感知推荐边界（不因地区降低无障碍功能推荐权重——平权红线实测）+ 地区个性化合规（推荐内容过 F4104 审查——漏审补审）+ 联动登记不越界 + 个性化数据走驻留路由。
#![allow(dead_code)]
use alloc::vec;
use alloc::vec::Vec;
use alloc::string::String;
use crate::checks::CheckSet;

pub const SEG_BASE: u16 = 0xCA40;

#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RdCode {
    Ok = 0,
    WeightBias = SEG_BASE + 0x01,
    Unaudited = SEG_BASE + 0x02,
    Overreach = SEG_BASE + 0x03,
}

/// 推荐条目：内容 × 基准权重 × 是否无障碍功能。
#[derive(Clone)]
pub struct RecItem { pub content: &'static str, pub base_weight: u32, pub is_a11y: bool }

/// 地区感知推荐引擎。
pub struct RecEngine { pub items: Vec<RecItem> }

impl RecEngine {
    pub fn new() -> Self { RecEngine { items: Vec::new() } }
    pub fn push(&mut self, i: RecItem) { self.items.push(i); }

    /// 地区权重调制：地区系数作用于推荐权重。
    /// 平权红线：无障碍功能条目地区系数必须为 100（不可降权）。
    pub fn regional_weight(&self, idx: usize, region_factor: u32) -> Result<u32, RdCode> {
        let it = self.items.get(idx).ok_or(RdCode::Overreach)?;
        if it.is_a11y && region_factor < 100 {
            return Err(RdCode::WeightBias); // 平权红线：降权即拒
        }
        Ok(it.base_weight * region_factor / 100)
    }

    /// 全表平权断言：所有无障碍条目在任意地区系数下权重不低于基准。
    pub fn a11y_parity(&self, region_factor: u32) -> Result<(), RdCode> {
        for (i, it) in self.items.iter().enumerate() {
            if it.is_a11y {
                let w = self.regional_weight(i, region_factor)?;
                if w < it.base_weight { return Err(RdCode::WeightBias); }
            }
        }
        Ok(())
    }
}

/// 地区审查对拍：推荐内容必须过 F4104 审查（有审查记录）。
pub struct AuditLedger { pub audited: Vec<&'static str>, pub blocked: Vec<&'static str> }

impl AuditLedger {
    pub fn new() -> Self { AuditLedger { audited: Vec::new(), blocked: Vec::new() } }
    pub fn pass(&mut self, c: &'static str) { self.audited.push(c); }
    pub fn block(&mut self, c: &'static str) { self.blocked.push(c); }

    /// 补审：未审条目清单（漏审补审）。
    pub fn unaudited(&self, items: &[&'static str]) -> Vec<&'static str> {
        items.iter()
            .filter(|c| !self.audited.contains(c) && !self.blocked.contains(c))
            .map(|c| *c)
            .collect()
    }

    /// 推荐门：被审查阻断的内容不得进推荐。
    pub fn gate(&self, content: &str) -> Result<(), RdCode> {
        if self.blocked.contains(&content) { return Err(RdCode::Unaudited); }
        Ok(())
    }
}

/// 个性化数据驻留路由复述：个性化埋点必须走驻留路由（有落点记录）。
pub struct DataRoute { pub routed: Vec<(&'static str, &'static str)> } // (埋点, 落点)

impl DataRoute {
    pub fn new() -> Self { DataRoute { routed: Vec::new() } }
    pub fn route(&mut self, event: &'static str, dest: &'static str) { self.routed.push((event, dest)); }
    /// 驻留路由核验：事件必须有落点。
    pub fn covered(&self, event: &str) -> bool {
        self.routed.iter().any(|(e, _)| *e == event)
    }
    /// 未走路由的埋点 = 红线违规。
    pub fn uncovered(&self, events: &[&'static str]) -> Vec<&'static str> {
        events.iter().filter(|e| !self.covered(e)).map(|e| *e).collect()
    }
}

/// 联动登记（不越界）。
pub struct LinkBook { pub faces: Vec<&'static str>, pub t06_impl: Vec<&'static str> }

impl LinkBook {
    pub fn new() -> Self { LinkBook { faces: Vec::new(), t06_impl: Vec::new() } }
    pub fn register(&mut self, f: &'static str) {
        if !self.faces.contains(&f) { self.faces.push(f); }
    }
    pub fn flag(&mut self, w: &'static str) { self.t06_impl.push(w); }
    pub fn clean(&self) -> bool { self.t06_impl.is_empty() }
}

fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) { set.add(label, ok, detail); }

pub fn run_ves357_checks() -> CheckSet {
    let mut set = CheckSet::new("ves357");
    let mut eng = RecEngine::new();

    // C357-01 推荐条目登记（含无障碍功能条目）。
    eng.push(RecItem { content: "screen-reader-plus", base_weight: 80, is_a11y: true });
    eng.push(RecItem { content: "game-promo", base_weight: 60, is_a11y: false });
    eng.push(RecItem { content: "magnifier-pro", base_weight: 70, is_a11y: true });
    chk(&mut set, "C357-01", eng.items.len() == 3, "三条目登记");

    // C357-02 中性系数（100）无影响。
    chk(&mut set, "C357-02", eng.regional_weight(0, 100) == Ok(80), "系数 100 权重不变");

    // C357-03 平权红线：无障碍条目地区降权 → 拒。
    chk(&mut set, "C357-03", eng.regional_weight(0, 50) == Err(RdCode::WeightBias), "读屏功能降权 50 => 红线拒");

    // C357-04 非无障碍条目可地区调制。
    chk(&mut set, "C357-04", eng.regional_weight(1, 50) == Ok(30), "普通条目可调制");

    // C357-05 全表平权断言：中性过。
    chk(&mut set, "C357-05", eng.a11y_parity(100).is_ok(), "全表平权（100）过");

    // C357-06 全表平权断言：降权全表拒。
    chk(&mut set, "C357-06", eng.a11y_parity(80) == Err(RdCode::WeightBias), "全表 80 → 无障碍条目破线");

    // C357-07 地区审查对拍：已审放行。
    let mut led = AuditLedger::new();
    led.pass("game-promo");
    led.block("banned-promo");
    chk(&mut set, "C357-07", led.gate("game-promo").is_ok(), "已审放行");

    // C357-08 阻断内容不得进推荐。
    chk(&mut set, "C357-08", led.gate("banned-promo") == Err(RdCode::Unaudited), "审查阻断 => 拒入");

    // C357-09 漏审补审清单。
    let recs = ["game-promo", "new-item-a", "new-item-b"];
    chk(&mut set, "C357-09", led.unaudited(&recs) == vec!["new-item-a", "new-item-b"], "漏审两条补审");

    // C357-10 个性化数据驻留路由复述。
    let mut dr = DataRoute::new();
    dr.route("rec-click", "eu-fra");
    dr.route("rec-impress", "eu-fra");
    let events = ["rec-click", "rec-impress", "rec-convert"];
    chk(&mut set, "C357-10", dr.uncovered(&events) == vec!["rec-convert"], "rec-convert 未走驻留路由");

    // C357-11 补路由后全覆盖。
    dr.route("rec-convert", "eu-fra");
    chk(&mut set, "C357-11", dr.uncovered(&events).is_empty(), "补后全覆");

    // C357-12 不越界守卫。
    let mut lb = LinkBook::new();
    lb.register("rec-weight-boundary");
    lb.register("rec-content-audit");
    chk(&mut set, "C357-12", lb.faces.len() == 2 && lb.clean(), "两面登记无实现");

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ves357_smoke() {
        let set = run_ves357_checks();
        assert!(set.failed_count() == 0, "ves357 red: {}", set.red_items_text());
    }

    #[test]
    fn ves357_parity() {
        let mut e = RecEngine::new();
        e.push(RecItem { content: "a11y", base_weight: 50, is_a11y: true });
        assert!(e.a11y_parity(100).is_ok());
        assert_eq!(e.a11y_parity(99), Err(RdCode::WeightBias));
    }
}
