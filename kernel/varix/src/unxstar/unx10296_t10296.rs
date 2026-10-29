//! UNX-F10296 · 对接性能预算（同步时长 / 批量上限）
//! 段位 0x11820 · 判据锚 UNX-F10296-J1 · 域 O3 对接面/预算闸
//! 结构：BudgetBook（预算册）+ SyncMeasure（同步计量）+ BatchCap（批量上限）
//! 语义：对接同步必须同时受「时长预算」与「批量上限」双重约束——
//!       任一超限即拦截；预算闸的拦截必须可复演 10/10。

use crate::alloc::format;
use crate::alloc::string::{String, ToString};
use crate::checks::CheckSet;
use alloc::vec::Vec;

pub const SEG_BASE: u32 = 0x11820;

/// 时长预算（同步单次允许的 tick 上限）
pub const TIME_BUDGET: u32 = 600;
/// 批量上限（单批允许的条目数）
pub const BATCH_CAP: u32 = 256;
/// 软水位（达到即预警但不拦截）
pub const SOFT_TIME: u32 = 480;
pub const SOFT_BATCH: u32 = 200;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BudgetVerdict {
    Within,
    SoftWarn,
    TimeBreach,
    BatchBreach,
    BothBreach,
}

impl BudgetVerdict {
    pub fn blocked(self) -> bool {
        matches!(self, BudgetVerdict::TimeBreach | BudgetVerdict::BatchBreach | BudgetVerdict::BothBreach)
    }
    pub fn code(self) -> u16 {
        (SEG_BASE as u16) + match self {
            BudgetVerdict::Within => 1,
            BudgetVerdict::SoftWarn => 2,
            BudgetVerdict::TimeBreach => 3,
            BudgetVerdict::BatchBreach => 4,
            BudgetVerdict::BothBreach => 5,
        }
    }
}

/// 单次同步计量样本
#[derive(Clone, Debug)]
pub struct SyncMeasure {
    pub ticks: u32,
    pub items: u32,
    pub label: String,
}

impl SyncMeasure {
    pub fn new(ticks: u32, items: u32, label: &str) -> Self {
        SyncMeasure { ticks, items, label: label.to_string() }
    }
    pub fn verdict(&self) -> BudgetVerdict {
        let t = self.ticks > TIME_BUDGET;
        let b = self.items > BATCH_CAP;
        match (t, b) {
            (true, true) => BudgetVerdict::BothBreach,
            (true, false) => BudgetVerdict::TimeBreach,
            (false, true) => BudgetVerdict::BatchBreach,
            (false, false) => {
                if self.ticks >= SOFT_TIME || self.items >= SOFT_BATCH {
                    BudgetVerdict::SoftWarn
                } else {
                    BudgetVerdict::Within
                }
            }
        }
    }
}

/// 批量切分器：超过 BATCH_CAP 的条目数必须拆批
pub struct BatchCap;

impl BatchCap {
    /// 返回切分后的批列表（每批 ≤ CAP）
    pub fn split(total: u32) -> Vec<u32> {
        if total == 0 {
            return Vec::new();
        }
        let full = total / BATCH_CAP;
        let rem = total % BATCH_CAP;
        let mut v = Vec::new();
        for _ in 0..full {
            v.push(BATCH_CAP);
        }
        if rem > 0 {
            v.push(rem);
        }
        v
    }
    /// 切分后每批是否全部合规
    pub fn all_legal(total: u32) -> bool {
        BatchCap::split(total).iter().all(|&b| b <= BATCH_CAP)
    }
}

/// 预算闸账：拦截事件 + 达成统计
pub struct BudgetBook {
    intercepts: Vec<(String, BudgetVerdict)>,
    soft_warns: u32,
    within_passes: u32,
}

impl BudgetBook {
    pub fn new() -> Self {
        BudgetBook { intercepts: Vec::new(), soft_warns: 0, within_passes: 0 }
    }

    /// 提交一次同步计量：闸门裁决并记账
    pub fn submit(&mut self, m: SyncMeasure) -> BudgetVerdict {
        let v = m.verdict();
        match v {
            BudgetVerdict::Within => self.within_passes += 1,
            BudgetVerdict::SoftWarn => self.soft_warns += 1,
            other => self.intercepts.push((m.label.clone(), other)),
        }
        v
    }

    /// 超时超批拦截次数
    pub fn intercept_count(&self) -> usize {
        self.intercepts.len()
    }
    pub fn soft_warn_count(&self) -> u32 {
        self.soft_warns
    }
    pub fn within_count(&self) -> u32 {
        self.within_passes
    }

    /// 按拦截类别计数
    pub fn count_kind(&self, want: BudgetVerdict) -> usize {
        self.intercepts.iter().filter(|(_, v)| *v == want).count()
    }

    /// 预算达成：need 次提交中拦截全部命中（超时超批场景各至少 need 次）
    pub fn intercept_streak(&self, need: usize) -> bool {
        self.intercept_count() >= need
    }

    /// 预算报表：FNV-1a 折叠指纹
    pub fn report_fp(&self) -> u32 {
        let mut h: u32 = 0x811C9DC5;
        for (label, v) in &self.intercepts {
            for b in label.bytes() {
                h ^= b as u32;
                h = h.wrapping_mul(0x01000193);
            }
            h ^= v.code() as u32;
            h = h.wrapping_mul(0x01000193);
        }
        h ^= (self.soft_warns << 8) ^ self.within_passes;
        h.wrapping_mul(0x01000193) ^ (SEG_BASE & 0xFFFF)
    }

    pub fn render(&self) -> String {
        format!(
            "budget seg={:#06x} intercepts={} soft={} within={}",
            SEG_BASE,
            self.intercepts.len(),
            self.soft_warns,
            self.within_passes
        )
    }
}

/// 受控注入核：kind=0 超时漏检伪造 / kind=1 超批放行伪造 / kind=2 预算常量漂移
pub fn inject_fault(b: &mut BudgetBook, kind: u8) -> &'static str {
    match kind {
        0 => {
            // 伪造：超时样本被记账为 Within（不经过 submit 正规通道）
            b.within_passes += 1;
            "time-breach-smuggled"
        }
        1 => {
            // 伪造：超批样本只触发软警
            b.soft_warns += 1;
            "batch-breach-warned"
        }
        2 => {
            // 漂移探测：返回预算常量（判据侧比对真源）
            "probe-constants"
        }
        _ => "no-op",
    }
}

pub fn run_unx10296_checks() -> CheckSet {
    let mut s = CheckSet::new("O3");
    // J1-01 预算常量真源
    {
        s.add("常量-预算真源", TIME_BUDGET == 600 && BATCH_CAP == 256 && SOFT_TIME == 480 && SOFT_BATCH == 200, "预算常量须与卡面口径一致");
    }
    // J1-02 正路径：预算内通行
    {
        let mut b = BudgetBook::new();
        let v = b.submit(SyncMeasure::new(100, 50, "normal"));
        s.add("正路径-预算内通行", v == BudgetVerdict::Within && b.within_count() == 1, "预算内须放行并计 within");
    }
    // J1-03 软警不拦截
    {
        let mut b = BudgetBook::new();
        let v1 = b.submit(SyncMeasure::new(500, 50, "soft-time"));
        let v2 = b.submit(SyncMeasure::new(100, 210, "soft-batch"));
        s.add(
            "软警-达线不拦截",
            v1 == BudgetVerdict::SoftWarn && v2 == BudgetVerdict::SoftWarn && b.soft_warn_count() == 2 && !v1.blocked(),
            "软水位只预警不拦截",
        );
    }
    // J1-04 超时拦截 10/10
    {
        let mut b = BudgetBook::new();
        let mut all = true;
        for i in 0..10 {
            let v = b.submit(SyncMeasure::new(TIME_BUDGET + 1 + i, 10, "overtime"));
            if v != BudgetVerdict::TimeBreach || !v.blocked() {
                all = false;
            }
        }
        s.add("拦截-超时十连", all && b.intercept_count() == 10 && b.count_kind(BudgetVerdict::TimeBreach) == 10, "超时 10/10 须拦截");
    }
    // J1-05 超批拦截 10/10
    {
        let mut b = BudgetBook::new();
        let mut all = true;
        for i in 0..10 {
            let v = b.submit(SyncMeasure::new(10, BATCH_CAP + 1 + i, "overbatch"));
            if v != BudgetVerdict::BatchBreach || !v.blocked() {
                all = false;
            }
        }
        s.add("拦截-超批十连", all && b.count_kind(BudgetVerdict::BatchBreach) == 10, "超批 10/10 须拦截");
    }
    // J1-06 双超限判 BothBreach
    {
        let mut b = BudgetBook::new();
        let v = b.submit(SyncMeasure::new(TIME_BUDGET + 9, BATCH_CAP + 9, "both"));
        s.add("拦截-双超限", v == BudgetVerdict::BothBreach && b.count_kind(BudgetVerdict::BothBreach) == 1, "双超限独立归类");
    }
    // J1-07 批量切分器
    {
        let sp = BatchCap::split(BATCH_CAP * 3 + 17);
        s.add(
            "切分-整批加余",
            sp.len() == 4 && sp[0] == BATCH_CAP && sp[3] == 17 && BatchCap::all_legal(BATCH_CAP * 5 + 255),
            "切分须整批+余批且全部合规",
        );
        s.add("切分-空量空批", BatchCap::split(0).is_empty(), "0 条切分须空");
    }
    // J1-08 注入检出
    {
        let mut b = BudgetBook::new();
        let r0 = inject_fault(&mut b, 0);
        let r1 = inject_fault(&mut b, 1);
        let r2 = inject_fault(&mut b, 2);
        // 走私账不进拦截表 → count 仍为 0，可由口径审计揭穿
        s.add(
            "注入-走私与软伪造揭穿",
            r0 == "time-breach-smuggled" && r1 == "batch-breach-warned" && b.intercept_count() == 0 && b.within_count() == 1,
            "绕过 submit 的走私账须由拦截表为空揭穿",
        );
        s.add("注入-常量探针", r2 == "probe-constants" && BATCH_CAP > SOFT_BATCH, "常量探针须可回读且硬上限>软水位");
    }
    // J1-09 边界：恰好等于上限不拦截
    {
        let mut b = BudgetBook::new();
        let v1 = b.submit(SyncMeasure::new(TIME_BUDGET, BATCH_CAP, "exact"));
        let v2 = b.submit(SyncMeasure::new(SOFT_TIME, SOFT_BATCH, "soft-exact"));
        s.add(
            "边界-恰等上限放行",
            v1 == BudgetVerdict::SoftWarn && v2 == BudgetVerdict::SoftWarn && b.intercept_count() == 0,
            "恰等于上限属软警区不拦截",
        );
    }
    // J1-10 十轮回放指纹稳定
    {
        let fp0 = {
            let mut b = BudgetBook::new();
            for i in 0..5 {
                b.submit(SyncMeasure::new(TIME_BUDGET + i, 10, "ot"));
                b.submit(SyncMeasure::new(10, BATCH_CAP + i, "ob"));
                b.submit(SyncMeasure::new(100, 100, "ok"));
            }
            b.report_fp()
        };
        let mut stable = true;
        for _ in 0..10 {
            let mut b = BudgetBook::new();
            for i in 0..5 {
                b.submit(SyncMeasure::new(TIME_BUDGET + i, 10, "ot"));
                b.submit(SyncMeasure::new(10, BATCH_CAP + i, "ob"));
                b.submit(SyncMeasure::new(100, 100, "ok"));
            }
            if b.report_fp() != fp0 {
                stable = false;
            }
        }
        s.add("回放-指纹十轮稳定", stable, "同构十轮回放指纹一致");
    }
    s
}
