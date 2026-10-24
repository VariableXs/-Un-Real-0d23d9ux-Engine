// SPDX-License-Identifier: Apache-2.0
// Varix Kernel — SVSTAR2 · ves97_ecoprivacy
// VE-F3697 · 生态数据隐私（R05 组 · 生态运营组）
//
// 锚点：VE-F3697
// 判据原文：出售禁令 P0、流向断言、删除时限、脱敏聚合、判据。
//
// 难点分析：
//   1. 数据出售禁令是红线最高级——数据变现=用户沦为商品，命中即 P0。
//   2. 数据流向必须可查——流向不可查是 P1 红线，审计链不能断。
//   3. 删除请求 30 天内完成——遗忘权有时限红线，超时即违规。

#![allow(dead_code)]

extern crate alloc;

use alloc::vec;
use alloc::vec::Vec;
use alloc::string::{String, ToString};

use crate::checks::CheckSet;

// ── 诊断码（0x9100 起）────────────────────────────────────────────

/// 诊断码段基址。
pub const SEG_BASE: u16 = 0x9100;

/// 诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct C91Code(pub u16);

impl C91Code {
    pub const DATA_SOLD: Self = Self(SEG_BASE);           // 数据出售 P0
    pub const FLOW_UNTRACEABLE: Self = Self(SEG_BASE + 1); // 流向不可查 P1
    pub const DELETE_TIMEOUT: Self = Self(SEG_BASE + 2);  // 删除超时 P1
    pub const AGGREGATE_FAIL: Self = Self(SEG_BASE + 3);  // 聚合失效
    pub const OWNER_ONLY: Self = Self(SEG_BASE + 4);      // 仅本人可见
    pub const DELETE_DONE: Self = Self(SEG_BASE + 5);     // 删除完成
    pub const FLOW_OK: Self = Self(SEG_BASE + 6);         // 流向可查

    pub fn code(self) -> u16 {
        self.0
    }

    pub fn explain(self) -> &'static str {
        match self {
            Self::DATA_SOLD => "用户数据出售检出——出售禁令 P0（数据红线最高级）",
            Self::FLOW_UNTRACEABLE => "数据流向不可查——流向红线 P1",
            Self::DELETE_TIMEOUT => "删除请求超 30 天——遗忘红线 P1",
            Self::AGGREGATE_FAIL => "脱敏聚合失效——匿名单源",
            Self::OWNER_ONLY => "收益数据仅本人可见——创作者隐私",
            Self::DELETE_DONE => "删除请求已按期完成",
            Self::FLOW_OK => "数据流向链完整可查",
            _ => "未知诊断",
        }
    }
}

// ── 脱敏聚合 ──────────────────────────────────────────────────────

/// 行为数据类别。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BehaviorKind {
    /// 浏览。
    View,
    /// 下载。
    Download,
    /// 评分。
    Rating,
}

impl BehaviorKind {
    pub fn name(self) -> &'static str {
        match self {
            Self::View => "浏览",
            Self::Download => "下载",
            Self::Rating => "评分",
        }
    }
}

/// 脱敏聚合器——输出计数，不输出个体。
pub struct AnonAggregator {
    /// 各类累计计数。
    counts: [u64; 3],
    /// 泄露个体的次数（必须为 0）。
    leaks: u32,
}

impl AnonAggregator {
    pub fn new() -> Self {
        Self { counts: [0; 3], leaks: 0 }
    }

    fn idx(k: BehaviorKind) -> usize {
        match k {
            BehaviorKind::View => 0,
            BehaviorKind::Download => 1,
            BehaviorKind::Rating => 2,
        }
    }

    /// 记录一条匿名聚合（只加计数，不带用户 ID）。
    pub fn bump(&mut self, k: BehaviorKind) {
        self.counts[Self::idx(k)] = self.counts[Self::idx(k)].saturating_add(1);
    }

    /// 导出聚合计数（仅总量）。
    pub fn export_counts(&self) -> [u64; 3] {
        self.counts
    }

    /// 导出个体明细是泄漏行为——检出即计数。
    pub fn leak_individual(&mut self) -> Result<(), C91Code> {
        self.leaks = self.leaks.saturating_add(1);
        Err(C91Code::AGGREGATE_FAIL)
    }

    pub fn leaks(&self) -> u32 {
        self.leaks
    }
}

impl Default for AnonAggregator {
    fn default() -> Self {
        Self::new()
    }
}

// ── 出售禁令 ──────────────────────────────────────────────────────

/// 数据用途。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DataUse {
    /// 内部运营分析。
    InternalOps,
    /// 产品改进。
    ProductImprove,
    /// 出售给第三方（禁止）。
    SellThirdParty,
    /// 广告定向（禁止）。
    AdTargeting,
}

impl DataUse {
    /// 是否为禁令用途。
    pub fn is_forbidden(self) -> bool {
        matches!(self, Self::SellThirdParty | Self::AdTargeting)
    }

    pub fn name(self) -> &'static str {
        match self {
            Self::InternalOps => "内部运营",
            Self::ProductImprove => "产品改进",
            Self::SellThirdParty => "第三方出售",
            Self::AdTargeting => "广告定向",
        }
    }
}

/// 出售禁令网关。
pub struct SellBan {
    /// 命中禁令次数。
    hits: u32,
}

impl SellBan {
    pub fn new() -> Self {
        Self { hits: 0 }
    }

    /// 申请数据用途。禁令用途即 P0。
    pub fn authorize(&mut self, use_: DataUse) -> Result<(), C91Code> {
        if use_.is_forbidden() {
            self.hits = self.hits.saturating_add(1);
            return Err(C91Code::DATA_SOLD);
        }
        Ok(())
    }

    pub fn hits(&self) -> u32 {
        self.hits
    }
}

impl Default for SellBan {
    fn default() -> Self {
        Self::new()
    }
}

// ── 数据流向 ──────────────────────────────────────────────────────

/// 流向节点。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FlowNode {
    /// 用户端。
    User,
    /// 平台存储。
    Storage,
    /// 分析引擎。
    Analytics,
    /// 第三方（须可查）。
    ThirdParty,
}

impl FlowNode {
    pub fn name(self) -> &'static str {
        match self {
            Self::User => "用户端",
            Self::Storage => "平台存储",
            Self::Analytics => "分析引擎",
            Self::ThirdParty => "第三方",
        }
    }
}

/// 流向审计链——每跳必登记，断链即 P1。
pub struct FlowAudit {
    /// 已登记跳段：(from, to)。
    hops: Vec<(FlowNode, FlowNode)>,
    /// 断链次数。
    breaks: u32,
}

impl FlowAudit {
    pub fn new() -> Self {
        Self { hops: Vec::new(), breaks: 0 }
    }

    /// 登记一跳。
    pub fn log_hop(&mut self, from: FlowNode, to: FlowNode) {
        self.hops.push((from, to));
    }

    /// 审计：从 User 出发的流向是否全链可查（每跳首尾相接）。
    pub fn audit(&mut self) -> Result<(), C91Code> {
        if self.hops.is_empty() {
            self.breaks = self.breaks.saturating_add(1);
            return Err(C91Code::FLOW_UNTRACEABLE);
        }
        for w in self.hops.windows(2) {
            if w[0].1 != w[1].0 {
                self.breaks = self.breaks.saturating_add(1);
                return Err(C91Code::FLOW_UNTRACEABLE);
            }
        }
        Ok(())
    }

    /// 链起点须为 User。
    pub fn starts_at_user(&self) -> bool {
        self.hops.first().map(|&(f, _)| f == FlowNode::User).unwrap_or(false)
    }

    pub fn hop_count(&self) -> usize {
        self.hops.len()
    }

    pub fn breaks(&self) -> u32 {
        self.breaks
    }
}

impl Default for FlowAudit {
    fn default() -> Self {
        Self::new()
    }
}

// ── 删除时限 ──────────────────────────────────────────────────────

/// 删除请求。
pub struct DeleteRequest {
    pub id: u64,
    /// 提交时间（天）。
    pub submitted_day: u32,
    /// 完成时间（天）。
    pub completed_day: Option<u32>,
}

impl DeleteRequest {
    pub fn new(id: u64, submitted_day: u32) -> Self {
        Self { id, submitted_day, completed_day: None }
    }

    /// 完成删除。
    pub fn complete(&mut self, day: u32) {
        self.completed_day = Some(day);
    }

    /// 耗时（天）。
    pub fn elapsed_days(&self, now_day: u32) -> u32 {
        match self.completed_day {
            Some(d) => d.saturating_sub(self.submitted_day),
            None => now_day.saturating_sub(self.submitted_day),
        }
    }

    /// 是否超 30 天。
    pub fn is_overdue(&self, now_day: u32) -> bool {
        self.elapsed_days(now_day) > 30
    }
}

/// 遗忘权执行器。
pub struct RightToForget {
    requests: Vec<DeleteRequest>,
    timeouts: u32,
}

impl RightToForget {
    pub fn new() -> Self {
        Self { requests: Vec::new(), timeouts: 0 }
    }

    /// 提交删除请求。
    pub fn submit(&mut self, id: u64, day: u32) {
        self.requests.push(DeleteRequest::new(id, day));
    }

    /// 完成删除。
    pub fn complete(&mut self, id: u64, day: u32) -> Result<(), C91Code> {
        for r in self.requests.iter_mut() {
            if r.id == id {
                r.complete(day);
                return Ok(());
            }
        }
        Err(C91Code::DELETE_TIMEOUT)
    }

    /// 巡检超时。
    pub fn patrol(&mut self, now_day: u32) -> u32 {
        self.timeouts = 0;
        for r in &self.requests {
            if r.is_overdue(now_day) {
                self.timeouts = self.timeouts.saturating_add(1);
            }
        }
        self.timeouts
    }

    pub fn timeouts(&self) -> u32 {
        self.timeouts
    }

    pub fn pending_count(&self) -> usize {
        self.requests.iter().filter(|r| r.completed_day.is_none()).count()
    }
}

impl Default for RightToForget {
    fn default() -> Self {
        Self::new()
    }
}

// ── 收益隐私 ──────────────────────────────────────────────────────

/// 收益数据可见性。
pub struct EarningsPrivacy {
    /// owner_id → 收益（微单位）。
    ledger: Vec<(u64, u64)>,
}

impl EarningsPrivacy {
    pub fn new() -> Self {
        Self { ledger: Vec::new() }
    }

    /// 记账。
    pub fn record(&mut self, owner: u64, amount: u64) {
        for entry in self.ledger.iter_mut() {
            if entry.0 == owner {
                entry.1 = entry.1.saturating_add(amount);
                return;
            }
        }
        self.ledger.push((owner, amount));
    }

    /// 查询：仅本人可见。viewer != owner 即拒。
    pub fn query(&self, viewer: u64, owner: u64) -> Result<u64, C91Code> {
        if viewer != owner {
            return Err(C91Code::OWNER_ONLY);
        }
        for &(o, amt) in &self.ledger {
            if o == owner {
                return Ok(amt);
            }
        }
        Ok(0)
    }
}

impl Default for EarningsPrivacy {
    fn default() -> Self {
        Self::new()
    }
}

// ── 判据 ──────────────────────────────────────────────────────────

pub fn run_ves97_checks() -> CheckSet {
    fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) {
        set.add(label, ok, detail);
    }
    let mut set = CheckSet::new("ves97_ecoprivacy");
    {
        // 01 数据出售即 P0
        let mut ban = SellBan::new();
        let r = ban.authorize(DataUse::SellThirdParty);
        chk(
            &mut set,
            "C391-01",
            r.err() == Some(C91Code::DATA_SOLD) && ban.hits() == 1,
            "向第三方出售用户数据即 P0（出售禁令红线最高级）",
        );
    }
    {
        // 02 广告定向同禁
        let mut ban = SellBan::new();
        let r = ban.authorize(DataUse::AdTargeting);
        chk(&mut set, "C391-02", r.err() == Some(C91Code::DATA_SOLD), "广告定向同为禁令用途");
    }
    {
        // 03 合法用途放行
        let mut ban = SellBan::new();
        let r1 = ban.authorize(DataUse::InternalOps);
        let r2 = ban.authorize(DataUse::ProductImprove);
        chk(&mut set, "C391-03", r1.is_ok() && r2.is_ok(), "内部运营/产品改进合法放行");
    }
    {
        // 04 流向断链即 P1
        let mut fa = FlowAudit::new();
        fa.log_hop(FlowNode::User, FlowNode::Storage);
        fa.log_hop(FlowNode::Analytics, FlowNode::ThirdParty); // 断链：Storage→?→Analytics
        let r = fa.audit();
        chk(
            &mut set,
            "C391-04",
            r.err() == Some(C91Code::FLOW_UNTRACEABLE) && fa.breaks() == 1,
            "流向断链即 P1（流向红线）",
        );
    }
    {
        // 05 全链可查
        let mut fa = FlowAudit::new();
        fa.log_hop(FlowNode::User, FlowNode::Storage);
        fa.log_hop(FlowNode::Storage, FlowNode::Analytics);
        fa.log_hop(FlowNode::Analytics, FlowNode::ThirdParty);
        let r = fa.audit();
        chk(&mut set, "C391-05", r.is_ok() && fa.starts_at_user(), "User→Storage→Analytics→ThirdParty 全链可查");
    }
    {
        // 06 删除按期完成
        let mut rf = RightToForget::new();
        rf.submit(1, 0);
        assert!(rf.complete(1, 15).is_ok());
        let overdue = rf.patrol(100);
        chk(&mut set, "C391-06", overdue == 0, "15 天内完成不算超时");
    }
    {
        // 07 删除超时即 P1
        let mut rf = RightToForget::new();
        rf.submit(1, 0);
        let t = rf.patrol(31);
        chk(
            &mut set,
            "C391-07",
            t == 1 && rf.timeouts() == 1,
            "31 天未完成即遗忘红线超时",
        );
    }
    {
        // 08 恰好 30 天不超
        let mut rf = RightToForget::new();
        rf.submit(1, 0);
        let t = rf.patrol(30);
        chk(&mut set, "C391-08", t == 0, "恰好 30 天边界不算超时");
    }
    {
        // 09 收益仅本人可见
        let mut ep = EarningsPrivacy::new();
        ep.record(42, 1000);
        let r1 = ep.query(42, 42);
        let r2 = ep.query(99, 42);
        chk(
            &mut set,
            "C391-09",
            r1 == Ok(1000) && r2.err() == Some(C91Code::OWNER_ONLY),
            "收益数据仅本人可见，他人查询拒绝",
        );
    }
    {
        // 10 聚合不泄露个体
        let mut agg = AnonAggregator::new();
        agg.bump(BehaviorKind::View);
        agg.bump(BehaviorKind::View);
        agg.bump(BehaviorKind::Download);
        let counts = agg.export_counts();
        chk(
            &mut set,
            "C391-10",
            counts[0] == 2 && counts[1] == 1 && agg.leaks() == 0,
            "脱敏聚合只出总量",
        );
    }
    {
        // 11 导出个体即泄漏
        let mut agg = AnonAggregator::new();
        let r = agg.leak_individual();
        chk(
            &mut set,
            "C391-11",
            r.err() == Some(C91Code::AGGREGATE_FAIL),
            "导出个体明细是聚合泄漏",
        );
    }
    {
        // 12 空流向即断链
        let mut fa = FlowAudit::new();
        let r = fa.audit();
        chk(
            &mut set,
            "C391-12",
            r.err() == Some(C91Code::FLOW_UNTRACEABLE),
            "空流向审计不通过（无可查链）",
        );
    }
    set
}
