//! ves353_s19market — VE-F4113 · T06 地区与市场联动
//! 市场域分发对地区维度的消费面：地区可用性表（功能/内容×地区上架状态）+ 地区定价位（与 F4105 联动）+ 可用性变更通告链 + 上架状态与法规冲突→合规优先下架评审 + 定价不含用户画像歧视（红线）+ 无障碍功能地区可用列（域本色）。
#![allow(dead_code)]
use alloc::vec;
use alloc::vec::Vec;
use alloc::string::String;
use crate::checks::CheckSet;

pub const SEG_BASE: u16 = 0xC740;

#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum MkCode {
    Ok = 0,
    CompliancePull = SEG_BASE + 0x01,
    NoticeMissing = SEG_BASE + 0x02,
    Discriminated = SEG_BASE + 0x03,
}

/// 上架状态。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ShelfState { Live = 0, Pulled = 1, Review = 2 }

impl ShelfState {
    pub const ALL: [ShelfState; 3] = [ShelfState::Live, ShelfState::Pulled, ShelfState::Review];
    pub fn name(self) -> &'static str {
        match self { ShelfState::Live => "live", ShelfState::Pulled => "pulled", ShelfState::Review => "review" }
    }
}

/// 可用性表条目：功能 × 地区 × 状态。
#[derive(Clone)]
pub struct AvailEntry { pub feature: &'static str, pub region: &'static str, pub state: ShelfState, pub a11y: bool }

pub struct AvailTable { pub entries: Vec<AvailEntry> }

impl AvailTable {
    pub fn new() -> Self { AvailTable { entries: Vec::new() } }

    pub fn put(&mut self, e: AvailEntry) {
        if let Some(x) = self.entries.iter_mut().find(|x| x.feature == e.feature && x.region == e.region) {
            x.state = e.state;
            x.a11y = e.a11y;
        } else {
            self.entries.push(e);
        }
    }

    /// 查上架状态。
    pub fn state_of(&self, feature: &str, region: &str) -> Option<ShelfState> {
        self.entries.iter().find(|e| e.feature == feature && e.region == region).map(|e| e.state)
    }

    /// 合规优先下架评审：法规标记拉黑的功能 → Pulled + 评审。
    pub fn compliance_pull(&mut self, feature: &'static str, region: &'static str) -> bool {
        let cur = self.state_of(feature, region);
        if cur == Some(ShelfState::Live) {
            self.put(AvailEntry { feature, region, state: ShelfState::Pulled, a11y: false });
            true
        } else {
            false
        }
    }

    /// 无障碍功能地区可用列（域本色）。
    pub fn a11y_available(&self, feature: &str) -> Vec<&'static str> {
        self.entries.iter().filter(|e| e.feature == feature && e.a11y && e.state == ShelfState::Live)
            .map(|e| e.region).collect()
    }

    pub fn count(&self) -> usize { self.entries.len() }
}

/// 定价位：地区 → 价目（分）。
pub struct Price { pub feature: &'static str, pub region: &'static str, pub cents: u32 }

pub struct PriceBook { pub prices: Vec<Price> }

impl PriceBook {
    pub fn new() -> Self { PriceBook { prices: Vec::new() } }
    pub fn put(&mut self, p: Price) { self.prices.push(p); }

    /// 定价查询。
    pub fn price_of(&self, feature: &str, region: &str) -> Option<u32> {
        self.prices.iter().find(|p| p.feature == feature && p.region == region).map(|p| p.cents)
    }

    /// 非歧视红线：同功能各地区价差不得超倍率带（防画像歧视定价）。
    pub fn fair_pricing(&self, feature: &str, max_ratio_x100: u32) -> Result<(), MkCode> {
        let ps: Vec<u32> = self.prices.iter().filter(|p| p.feature == feature).map(|p| p.cents).collect();
        if ps.is_empty() { return Err(MkCode::NoticeMissing); }
        let min = *ps.iter().min().unwrap();
        let max = *ps.iter().max().unwrap();
        if min == 0 { return Err(MkCode::Discriminated); }
        if (max - min) * 100 > min * max_ratio_x100 { return Err(MkCode::Discriminated); }
        Ok(())
    }
}

/// 通告链：可用性变更 → 按链路通告。
pub struct NoticeChain { pub chain: Vec<&'static str>, pub sent: Vec<&'static str> }

impl NoticeChain {
    pub fn new(chain: Vec<&'static str>) -> Self { NoticeChain { chain, sent: Vec::new() } }

    /// 变更通告沿链发送。
    pub fn notify(&mut self, change: &'static str) -> usize {
        let mut n = 0;
        for hop in self.chain.iter() {
            let _ = hop;
            self.sent.push(change);
            n += 1;
        }
        n
    }

    /// 通告缺 → 补发。
    pub fn backfill(&mut self, change: &'static str) -> usize {
        if self.sent.iter().any(|s| *s == change) { 0 } else { self.notify(change) }
    }

    pub fn len(&self) -> usize { self.sent.len() }
}

fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) { set.add(label, ok, detail); }

pub fn run_ves353_checks() -> CheckSet {
    let mut set = CheckSet::new("ves353");
    let mut at = AvailTable::new();

    // C353-01 可用性表登记。
    at.put(AvailEntry { feature: "voice-input", region: "US", state: ShelfState::Live, a11y: true });
    at.put(AvailEntry { feature: "voice-input", region: "EU", state: ShelfState::Live, a11y: true });
    at.put(AvailEntry { feature: "pay-wallet", region: "CN", state: ShelfState::Live, a11y: false });
    chk(&mut set, "C353-01", at.count() == 3, "三登记");

    // C353-02 查上架状态。
    chk(&mut set, "C353-02", at.state_of("voice-input", "US") == Some(ShelfState::Live), "US 在架");

    // C353-03 合规优先下架评审。
    chk(&mut set, "C353-03", at.compliance_pull("pay-wallet", "CN") && at.state_of("pay-wallet", "CN") == Some(ShelfState::Pulled), "法规冲突 → 下架");

    // C353-04 幂等：非 Live 不重复下架。
    chk(&mut set, "C353-04", !at.compliance_pull("pay-wallet", "CN"), "已下架不重复");

    // C353-05 无障碍可用列（域本色）。
    at.put(AvailEntry { feature: "screen-magnify", region: "JP", state: ShelfState::Live, a11y: true });
    at.put(AvailEntry { feature: "screen-magnify", region: "BR", state: ShelfState::Review, a11y: true });
    chk(&mut set, "C353-05", at.a11y_available("screen-magnify") == vec!["JP"], "放大镜 JP 在架 BR 评审中");

    // C353-06 定价位登记与查询。
    let mut pb = PriceBook::new();
    pb.put(Price { feature: "voice-input", region: "US", cents: 990 });
    pb.put(Price { feature: "voice-input", region: "CN", cents: 690 });
    pb.put(Price { feature: "voice-input", region: "JP", cents: 1080 });
    chk(&mut set, "C353-06", pb.price_of("voice-input", "US") == Some(990), "US 990 分");

    // C353-07 非歧视红线：价差带内放行。
    chk(&mut set, "C353-07", pb.fair_pricing("voice-input", 60).is_ok(), "最大差 (1080-690)=390 ≤ 690×60%");

    // C353-08 定价歧视检出。
    pb.put(Price { feature: "pro-tier", region: "US", cents: 100 });
    pb.put(Price { feature: "pro-tier", region: "CN", cents: 900 });
    chk(&mut set, "C353-08", pb.fair_pricing("pro-tier", 60) == Err(MkCode::Discriminated), "8 倍差 => 歧视拒");

    // C353-09 通告链发送。
    let mut nc = NoticeChain::new(vec!["market", "store", "crm"]);
    let n = nc.notify("voice-input-pull-CN");
    chk(&mut set, "C353-09", n == 3 && nc.len() == 3, "三跳全通告");

    // C353-10 通告缺 → 补发。
    let mut nc2 = NoticeChain::new(vec!["market", "store"]);
    nc2.notify("x-change");
    chk(&mut set, "C353-10", nc2.backfill("x-change") == 0 && nc2.backfill("y-change") == 2, "已发不补 未发补 2");

    // C353-11 覆盖 put 幂等更新。
    at.put(AvailEntry { feature: "voice-input", region: "US", state: ShelfState::Review, a11y: true });
    chk(&mut set, "C353-11", at.count() == 4 && at.state_of("voice-input", "US") == Some(ShelfState::Review), "覆盖更新不加行");

    // C352-12 状态名单单源。
    chk(&mut set, "C353-12", ShelfState::ALL[1].name() == "pulled", "三态单源");

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ves353_smoke() {
        let set = run_ves353_checks();
        let red = set.red_items();
        assert!(red.is_empty(), "ves353 red: {:?}", red);
    }

    #[test]
    fn ves353_fair() {
        let mut pb = PriceBook::new();
        pb.put(Price { feature: "f", region: "A", cents: 100 });
        assert!(pb.fair_pricing("f", 10).is_ok()); // 单点定价无价差带可判 => 放行
    }
}
