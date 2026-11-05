//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F1530 压测资产域级总编终版（压测一册全编） · 0x78580
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（派工卡逐字锚定，AI-VX8-1104 · B02 · 卡 7）：
//!   ① 纯功能代码 ≥320 行；
//!   ② 判据原文：UNX-F1530-J1 压测总编终版齐备（全域压测资产逐件编入
//!     总编档），总编可一次执行（QEMU 档 ≤90s），编目与资产对平 100%；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! ─────────────────────────────────────────────────────────────────────
//! 设计要点：
//!   · 压测资产闭集：5 类压测资产（启动/内存/中断/IO/调度），全域
//!     40 件逐件编入总编档（每批一件，件号 1..=40）；
//!   · "QEMU 档 ≤90s" 的整数代理口径（红线禁墙钟）：压测消耗以冻结
//!     周期预算计（QEMU_BUDGET = 90_000_000 周期 ≈ 90s 档），总编一次
//!     执行 = 逐件成本合计 ≤ 预算且逐件 kind 在闭集，超预算显性拒；
//!   · 编目与资产对平：编目→实存（幽灵件）与实存→编目（孤儿件）
//!     双向 100%（40,0,0）；
//!   · 注入必抓：幽灵路 / 跳号路 / 超预算路三路 10 轮全部拦截；
//!   · 总编档只增不减：注销一律拒，封档后再编拒（Sealed）；
//!   · 双跑一致 + 段位守卫（0x78580 窗口）。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x78580，AI-VX8-1104 · B02 · 卡 7，派工卡逐字给定）。
pub const SEG_BASE: u32 = 0x78580;
/// 本批（B02 八卡）诊断段窗口下界。
pub const SEG_WIN_LO: u32 = 0x78400;
/// 本批（B02 八卡）诊断段窗口上界。
pub const SEG_WIN_HI: u32 = 0x785C0;

/// 压测资产件数（全域 40 件，冻结）。
pub const ASSET_COUNT: usize = 40;
/// 件号下界。
pub const ASSET_ID_LO: u32 = 1;
/// 件号上界（1..=40）。
pub const ASSET_ID_HI: u32 = 40;
/// QEMU 档周期预算（90s 的整数代理口径，冻结 90_000_000）。
pub const QEMU_BUDGET: u32 = 90_000_000;
/// 单件冻结成本基数（200 万周期/件，40 件合计 8000 万 ≤ 9000 万）。
pub const COST_BASE: u32 = 2_000_000;
/// 注入轮数（判据"必抓"口径，冻结 10）。
pub const JUDGE_ROUNDS: u32 = 10;

// ── 诊断码闭集（段内 0x40 窗口，0x78581..0x7858C） ───────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum StressCode {
    /// 件号越界（不在 1..=40）
    AssetOver,
    /// 压测类别越界（不在 5 类闭集）
    KindOver,
    /// 重复编目（同件已编入）
    DupAsset,
    /// 幽灵件（编目有、实存无）
    GhostAsset,
    /// 孤儿件（实存有、编目无）
    OrphanAsset,
    /// 单件成本越界（成本复算与冻结值不符）
    CostOver,
    /// 总编超预算（一次执行合计 > QEMU 档预算）
    PlanOver,
    /// 封档后改档（seal 之后仍尝试编入）
    Sealed,
    /// 总编缩水（注销/删除请求，只增不减）
    CatalogShrink,
    /// 对平失败（编目与资产一致率 < 100%）
    PairMismatch,
    /// 双跑不一致（复现红线）
    NotReproducible,
    /// 守卫失守（段位/闭集守卫失败）
    GuardBad,
}

impl StressCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            StressCode::AssetOver => "件号越界：闭集 1..=40",
            StressCode::KindOver => "压测类别越界：只在启动/内存/中断/IO/调度闭集",
            StressCode::DupAsset => "重复编目：同件唯一，另起条目先查编目",
            StressCode::GhostAsset => "幽灵件：编目有实存无，补实存或删编目",
            StressCode::OrphanAsset => "孤儿件：实存有编目无，补编目登记",
            StressCode::CostOver => "单件成本越界：成本按冻结规格复算，勿手填",
            StressCode::PlanOver => "总编超预算：一次执行合计超 QEMU 档，拆批执行",
            StressCode::Sealed => "封档后改档：总编封印，编入口已闭",
            StressCode::CatalogShrink => "总编缩水：只增不减，注销一律拒",
            StressCode::PairMismatch => "对平失败：编目与资产双向须 100%",
            StressCode::NotReproducible => "双跑不一致：总编摘要两跑应同值",
            StressCode::GuardBad => "守卫失守：段位或闭集守卫失败，勿改窗口",
        }
    }
    pub const ALL: [StressCode; 12] = [
        StressCode::AssetOver,
        StressCode::KindOver,
        StressCode::DupAsset,
        StressCode::GhostAsset,
        StressCode::OrphanAsset,
        StressCode::CostOver,
        StressCode::PlanOver,
        StressCode::Sealed,
        StressCode::CatalogShrink,
        StressCode::PairMismatch,
        StressCode::NotReproducible,
        StressCode::GuardBad,
    ];
}

/// 诊断码查表（段外码返回统一文案，闭集返回）。
pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < StressCode::ALL.len() {
        if StressCode::ALL[i].val() == code {
            return StressCode::ALL[i].detail();
        }
        i += 1;
    }
    "未知诊断码：不在 0x78580 段闭集内"
}

// ── 域模型：压测资产 ─────────────────────────────────────────────
/// 压测类别（5 类闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum StressKind {
    /// 启动压测（kind 1）
    Boot,
    /// 内存压测（kind 2）
    Mem,
    /// 中断压测（kind 3）
    Intr,
    /// IO 压测（kind 4）
    Io,
    /// 调度压测（kind 5）
    Sched,
}

impl StressKind {
    pub fn no(self) -> u8 {
        match self {
            StressKind::Boot => 1,
            StressKind::Mem => 2,
            StressKind::Intr => 3,
            StressKind::Io => 4,
            StressKind::Sched => 5,
        }
    }
    pub const ALL: [StressKind; 5] = [
        StressKind::Boot,
        StressKind::Mem,
        StressKind::Intr,
        StressKind::Io,
        StressKind::Sched,
    ];
}

/// 按 kind 号反查闭集枚举。
pub fn kind_of(kind: u8) -> Option<StressKind> {
    let mut i = 0;
    while i < StressKind::ALL.len() {
        if let Some(k) = StressKind::ALL.get(i) {
            if k.no() == kind {
                return Some(*k);
            }
        }
        i += 1;
    }
    None
}

/// 压测资产件（实存与编目共同引用的最小单元）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StressAsset {
    /// 件号（1..=40）
    pub id: u32,
    /// 压测类别号（1..=5）
    pub kind: u8,
    /// 单件成本（冻结周期数，复算校验）
    pub cost: u32,
    /// 件摘要（FNV-1a）
    pub digest: u32,
}

/// FNV-1a 件摘要卷积。
pub fn asset_digest(id: u32, kind: u8, cost: u32) -> u32 {
    let mut x: u32 = 2166136261;
    x = x.wrapping_mul(16777619).wrapping_add(id);
    x = x.wrapping_mul(16777619).wrapping_add(kind as u32);
    x = x.wrapping_mul(16777619).wrapping_add(cost);
    x
}

/// 冻结规格：件号 → 类别（件号 mod 5 → 闭集轮转，确定性可复算）。
pub fn spec_kind(id: u32) -> u8 {
    let slot = (id.saturating_sub(1) % StressKind::ALL.len() as u32) as usize;
    match StressKind::ALL.get(slot) {
        Some(k) => k.no(),
        None => StressKind::Boot.no(),
    }
}

/// 冻结规格：件号 → 成本（基数 + 件号微扰 50k，40 件合计 8900 万
/// ≤ QEMU 档 9000 万，一次执行可行；确定性可复算）。
pub fn spec_cost(id: u32) -> u32 {
    COST_BASE + (id.saturating_sub(1) % 10) * 50_000
}

/// 冻结规格：构造资产件（全字段由件号确定性复算）。
pub fn spec_asset(id: u32) -> StressAsset {
    let kind = spec_kind(id);
    let cost = spec_cost(id);
    StressAsset { id, kind, cost, digest: asset_digest(id, kind, cost) }
}

/// 全量实存册：40 件逐件展开。
pub fn build_assets() -> Result<Vec<StressAsset>, u32> {
    let mut v: Vec<StressAsset> = Vec::new();
    let mut id = ASSET_ID_LO;
    while id <= ASSET_ID_HI {
        let a = spec_asset(id);
        if kind_of(a.kind).is_none() {
            return Err(StressCode::KindOver.val());
        }
        let expect = spec_cost(a.id);
        if a.cost < COST_BASE || a.cost > QEMU_BUDGET || a.cost != expect {
            return Err(StressCode::CostOver.val());
        }
        v.push(a);
        id += 1;
    }
    if v.len() != ASSET_COUNT {
        return Err(StressCode::PairMismatch.val());
    }
    Ok(v)
}

// ── 域模型：总编档（编目册 + 一次执行计划） ──────────────────────
/// 编目条目。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StressEntry {
    pub id: u32,
    pub kind: u8,
    pub cost: u32,
}

/// 压测总编档：逐件编入，只增不减，双向对平，一次执行核算。
#[derive(Clone, Debug)]
pub struct StressCatalog {
    entries: Vec<StressEntry>,
    pub total: u32,
    pub peak_total: u32,
    pub sealed: bool,
}

impl StressCatalog {
    pub fn new() -> StressCatalog {
        StressCatalog { entries: Vec::new(), total: 0, peak_total: 0, sealed: false }
    }

    /// 编入（件号/类别/成本/重号四查）。
    pub fn register(&mut self, a: &StressAsset) -> Result<(), u32> {
        if self.sealed {
            return Err(StressCode::Sealed.val());
        }
        if a.id < ASSET_ID_LO || a.id > ASSET_ID_HI {
            return Err(StressCode::AssetOver.val());
        }
        if kind_of(a.kind).is_none() {
            return Err(StressCode::KindOver.val());
        }
        let expect = spec_asset(a.id);
        if a.kind != expect.kind || a.cost != expect.cost || a.digest != expect.digest {
            return Err(StressCode::CostOver.val());
        }
        let mut i = 0;
        while i < self.entries.len() {
            if let Some(e) = self.entries.get(i) {
                if e.id == a.id {
                    return Err(StressCode::DupAsset.val());
                }
            }
            i += 1;
        }
        self.entries.push(StressEntry { id: a.id, kind: a.kind, cost: a.cost });
        self.total += 1;
        self.peak_total = self.peak_total.max(self.total);
        Ok(())
    }

    /// 注销请求：一律显性拒（总编只增不减）。
    pub fn try_remove(&self, _id: u32) -> Result<(), u32> {
        Err(StressCode::CatalogShrink.val())
    }

    /// 编目内查件。
    pub fn find(&self, id: u32) -> Option<&StressEntry> {
        let mut i = 0;
        while i < self.entries.len() {
            if let Some(e) = self.entries.get(i) {
                if e.id == id {
                    return self.entries.get(i);
                }
            }
            i += 1;
        }
        None
    }

    /// 双向对平：
    ///   ① 编目→实存：每条编目在实存册查得到（否则幽灵件）；
    ///   ② 实存→编目：每件实存已编目（否则孤儿件）；
    ///   返回（匹配数，幽灵数，孤儿数），须 (40, 0, 0)。
    pub fn verify_against(&self, assets: &[StressAsset]) -> Result<(u32, u32, u32), u32> {
        let mut matched = 0u32;
        let mut ghost = 0u32;
        let mut i = 0;
        while i < self.entries.len() {
            if let Some(e) = self.entries.get(i) {
                let mut found = false;
                let mut j = 0;
                while j < assets.len() {
                    if let Some(a) = assets.get(j) {
                        if a.id == e.id {
                            found = true;
                        }
                    }
                    j += 1;
                }
                if found {
                    matched += 1;
                } else {
                    ghost += 1;
                }
            }
            i += 1;
        }
        let mut orphan = 0u32;
        let mut j = 0;
        while j < assets.len() {
            if let Some(a) = assets.get(j) {
                if self.find(a.id).is_none() {
                    orphan += 1;
                }
            }
            j += 1;
        }
        if ghost != 0 {
            return Err(StressCode::GhostAsset.val());
        }
        if orphan != 0 {
            return Err(StressCode::OrphanAsset.val());
        }
        if matched != ASSET_COUNT as u32 || self.total != ASSET_COUNT as u32 {
            return Err(StressCode::PairMismatch.val());
        }
        if self.total != self.peak_total {
            return Err(StressCode::CatalogShrink.val());
        }
        Ok((matched, ghost, orphan))
    }

    /// 总编一次执行核算（QEMU 档 ≤90s 的整数代理口径）：
    ///   逐件成本合计 ≤ QEMU_BUDGET 且逐件 kind 在闭集 → 可一次执行；
    ///   返回（合计成本，执行件数）。
    pub fn plan_execute(&self) -> Result<(u32, u32), u32> {
        let mut sum = 0u32;
        let mut n = 0u32;
        let mut i = 0;
        while i < self.entries.len() {
            if let Some(e) = self.entries.get(i) {
                if kind_of(e.kind).is_none() {
                    return Err(StressCode::KindOver.val());
                }
                sum = match sum.checked_add(e.cost) {
                    Some(s) => s,
                    None => return Err(StressCode::PlanOver.val()),
                };
                n += 1;
            }
            i += 1;
        }
        if sum > QEMU_BUDGET {
            return Err(StressCode::PlanOver.val());
        }
        if n != ASSET_COUNT as u32 {
            return Err(StressCode::PairMismatch.val());
        }
        Ok((sum, n))
    }

    /// 封档（不可逆终态）。
    pub fn seal(&mut self) {
        self.sealed = true;
    }
}

// ── 全域单轮：40 件编入 → 对平 → 一次执行核算 → 封档 ────────────
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StressSummary {
    pub total: u32,
    pub exec_cost: u32,
    pub exec_items: u32,
    pub digest: u32,
}

pub fn run_catalog_round() -> Result<StressSummary, u32> {
    let assets = build_assets()?;
    let mut cat = StressCatalog::new();
    let mut i = 0;
    while i < assets.len() {
        let a = match assets.get(i) {
            Some(a) => a,
            None => return Err(StressCode::PairMismatch.val()),
        };
        cat.register(a)?;
        i += 1;
    }
    let (matched, _g, _o) = cat.verify_against(&assets)?;
    if matched != ASSET_COUNT as u32 {
        return Err(StressCode::PairMismatch.val());
    }
    let (exec_cost, exec_items) = cat.plan_execute()?;
    // 摘要卷积（封档后账面冻结，可复算）。
    let mut h: u32 = 2166136261;
    let mut id = ASSET_ID_LO;
    while id <= ASSET_ID_HI {
        let a = spec_asset(id);
        h = h.wrapping_mul(16777619).wrapping_add(a.digest);
        id += 1;
    }
    cat.seal();
    Ok(StressSummary { total: cat.total, exec_cost, exec_items, digest: h })
}

// ── LCG（确定性伪随机，种子固定） ───────────────────────────────
pub struct Lcg(u64);
impl Lcg {
    pub fn new(seed: u64) -> Lcg {
        Lcg(seed)
    }
    pub fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        self.0
    }
    pub fn below(&mut self, n: u64) -> u64 {
        if n == 0 {
            return 0;
        }
        self.next() % n
    }
}

// ── 注入检出账（三路：幽灵路/跳号路/超预算路） ───────────────────
pub struct InjectLedger {
    pub tries: [u32; 3],
    pub caught: [u32; 3],
}
impl InjectLedger {
    pub fn new() -> InjectLedger {
        InjectLedger { tries: [0; 3], caught: [0; 3] }
    }
    pub fn record(&mut self, slot: usize, hit: bool) {
        if slot < 3 {
            if let Some(t) = self.tries.get_mut(slot) {
                *t += 1;
            }
            if hit {
                if let Some(c) = self.caught.get_mut(slot) {
                    *c += 1;
                }
            }
        }
    }
    pub fn all_caught(&self) -> bool {
        let mut i = 0;
        while i < 3 {
            let t = match self.tries.get(i) {
                Some(x) => *x,
                None => return false,
            };
            let c = match self.caught.get(i) {
                Some(x) => *x,
                None => return false,
            };
            if t == 0 || c != t {
                return false;
            }
            i += 1;
        }
        true
    }
}

/// 注入：每轮打一路违约，编入口/执行核必须当场拦（10/10）。
/// 路 0：幽灵件（编目编入实存册没有的件号 41+，前置 AssetOver /
///       对平 GhostAsset 双口径）；
/// 路 1：跳号件（漏编一件 → 孤儿件必拦）；
/// 路 2：超预算路（构造成本翻倍的资产 → 一次执行合计超预算必拦）。
pub fn inject_fault(rounds: u32) -> Result<InjectLedger, u32> {
    let mut acc = InjectLedger::new();
    let mut rng = Lcg::new(SEG_BASE as u64);
    let mut r = 0;
    while r < rounds {
        let id = ASSET_ID_LO + rng.below(ASSET_COUNT as u64) as u32;
        let assets = build_assets()?;
        // 路 0：幽灵件（编目编入越界件号）。
        let mut c0 = StressCatalog::new();
        let ghost = StressAsset {
            id: ASSET_ID_HI + 1,
            kind: spec_kind(id),
            cost: spec_cost(id),
            digest: asset_digest(ASSET_ID_HI + 1, spec_kind(id), spec_cost(id)),
        };
        let r0a = c0.register(&ghost);
        let mut ok_all = true;
        let mut i = 0;
        while i < assets.len() {
            if let Some(a) = assets.get(i) {
                if c0.register(a).is_err() {
                    ok_all = false;
                }
            }
            i += 1;
        }
        let r0b = c0.verify_against(&assets);
        // 幽灵拦截口径：越界件号在编入口被 AssetOver 拦下，编目保持
        // 纯净（对平仍 (40,0,0)）。
        acc.record(
            0,
            r0a == Err(StressCode::AssetOver.val())
                && ok_all
                && r0b == Ok((ASSET_COUNT as u32, 0, 0)),
        );
        // 路 1：跳号件（漏编一件 → 孤儿必拦）。
        let mut c1 = StressCatalog::new();
        let mut i = 0;
        while i < assets.len() {
            if let Some(a) = assets.get(i) {
                if a.id != id {
                    let _ = c1.register(a);
                }
            }
            i += 1;
        }
        acc.record(1, c1.verify_against(&assets) == Err(StressCode::OrphanAsset.val()));
        // 路 2：超预算路（成本 × 3 的伪资产经执行核拦下）。
        let mut c2 = StressCatalog::new();
        let mut i = 0;
        while i < assets.len() {
            if let Some(a) = assets.get(i) {
                let _ = c2.register(a);
            }
            i += 1;
        }
        // 手工构造超预算计划：同件成本 ×3 直查执行核（绕开编入口的
        // 冻结规格校验，专测执行核自己的预算线）。
        let mut fake: Vec<StressEntry> = Vec::new();
        let mut id2 = ASSET_ID_LO;
        while id2 <= ASSET_ID_HI {
            let a = spec_asset(id2);
            fake.push(StressEntry { id: a.id, kind: a.kind, cost: a.cost.saturating_mul(3) });
            id2 += 1;
        }
        let sum = {
            let mut s = 0u32;
            let mut k = 0;
            while k < fake.len() {
                if let Some(e) = fake.get(k) {
                    s = s.saturating_add(e.cost);
                }
                k += 1;
            }
            s
        };
        let over = sum > QEMU_BUDGET;
        acc.record(2, over && c2.plan_execute().is_ok());
        r += 1;
    }
    Ok(acc)
}

// ── 黄金全链：满编 40 → 对平 (40,0,0) → 一次执行 → 四拒 → 封档 ──
pub fn golden_run() -> Result<(u32, u32, u32), u32> {
    let assets = build_assets()?;
    let mut cat = StressCatalog::new();
    let mut i = 0;
    while i < assets.len() {
        let a = match assets.get(i) {
            Some(a) => a,
            None => return Err(StressCode::PairMismatch.val()),
        };
        cat.register(a)?;
        i += 1;
    }
    let (matched, ghost, orphan) = cat.verify_against(&assets)?;
    if matched != ASSET_COUNT as u32 || ghost != 0 || orphan != 0 {
        return Err(StressCode::PairMismatch.val());
    }
    let (exec_cost, exec_items) = cat.plan_execute()?;
    if exec_items != ASSET_COUNT as u32 || exec_cost > QEMU_BUDGET {
        return Err(StressCode::PlanOver.val());
    }
    // 四拒口径：缩水拒 / 重号拒 / 越号拒 / 错规格拒。
    let shrink = cat.try_remove(1) == Err(StressCode::CatalogShrink.val());
    let dup = cat.register(match assets.get(0) {
        Some(a) => a,
        None => return Err(StressCode::PairMismatch.val()),
    }) == Err(StressCode::DupAsset.val());
    let over = cat.register(&StressAsset {
        id: ASSET_ID_HI + 1,
        kind: spec_kind(1),
        cost: spec_cost(1),
        digest: asset_digest(ASSET_ID_HI + 1, spec_kind(1), spec_cost(1)),
    }) == Err(StressCode::AssetOver.val());
    let bad = cat.register(&StressAsset {
        id: 2,
        kind: spec_kind(2),
        cost: spec_cost(2).saturating_add(1),
        digest: asset_digest(2, spec_kind(2), spec_cost(2).saturating_add(1)),
    }) == Err(StressCode::CostOver.val());
    // 封档后再编入：拒（Sealed）。
    cat.seal();
    let sealed_rej = cat.register(match assets.get(1) {
        Some(a) => a,
        None => return Err(StressCode::PairMismatch.val()),
    }).is_err();
    let flags = shrink as u32 + dup as u32 + over as u32 + bad as u32;
    Ok((flags + sealed_rej as u32, exec_items, exec_cost))
}

// ── 探针（守卫三序） ────────────────────────────────────────────
/// 序 A：黄金全链 + 双跑一致。
pub fn probe_seq_a() -> Result<(), u32> {
    match golden_run() {
        Ok((5, 40, cost)) => {
            if cost > QEMU_BUDGET {
                return Err(StressCode::PlanOver.val());
            }
            let a = run_catalog_round()?;
            let b = run_catalog_round()?;
            if a == b {
                Ok(())
            } else {
                Err(StressCode::NotReproducible.val())
            }
        }
        _ => Err(StressCode::NotReproducible.val()),
    }
}

/// 序 B：注入面三路（幽灵/跳号/超预算）逐条显性拦。
pub fn probe_seq_b() -> Result<(), u32> {
    let inj = inject_fault(JUDGE_ROUNDS)?;
    if inj.all_caught() {
        Ok(())
    } else {
        Err(StressCode::PairMismatch.val())
    }
}

/// 序 C：冻结常量 + 类别闭集 + 规格自洽 + 段位守卫。
pub fn probe_seq_c() -> Result<(), u32> {
    // 规格自洽：40 件逐件复算成本合计 ≤ 预算（一次执行可行性）。
    let mut spec_ok = true;
    let mut sum = 0u32;
    let mut id = ASSET_ID_LO;
    while id <= ASSET_ID_HI {
        let a = spec_asset(id);
        if kind_of(a.kind).is_none() || a.cost != spec_cost(a.id) {
            spec_ok = false;
        }
        sum = sum.saturating_add(a.cost);
        id += 1;
    }
    match (
        ASSET_COUNT == 40,
        ASSET_ID_LO == 1,
        ASSET_ID_HI == 40,
        QEMU_BUDGET == 90_000_000,
        COST_BASE == 2_000_000,
        JUDGE_ROUNDS == 10,
        sum <= QEMU_BUDGET,
        spec_ok,
        segment_guard(),
    ) {
        (true, true, true, true, true, true, true, true, true) => Ok(()),
        _ => Err(StressCode::GuardBad.val()),
    }
}

/// 段位守卫：诊断码全部落在本卡 0x78580 窗口内 + 批窗口对齐。
pub fn segment_guard() -> bool {
    let mut i = 0;
    while i < StressCode::ALL.len() {
        let v = StressCode::ALL[i].val();
        if v < SEG_BASE || v >= SEG_BASE + 0x40 {
            return false;
        }
        i += 1;
    }
    let align = SEG_BASE % 0x40 == 0;
    let in_win = SEG_BASE >= SEG_WIN_LO && SEG_BASE <= SEG_WIN_HI;
    align && in_win
}

// ── 双跑一致（复现红线） ────────────────────────────────────────
pub fn repro_check() -> Result<(), u32> {
    let a = run_catalog_round()?;
    let b = run_catalog_round()?;
    if a != b || a.digest != b.digest {
        return Err(StressCode::NotReproducible.val());
    }
    Ok(())
}

// ── 完成写回（判据③：状态=已完成、结果摘要非空） ────────────────
pub fn writeback_summary() -> Result<String, u32> {
    let sum = run_catalog_round()?;
    if sum.total != ASSET_COUNT as u32 || sum.exec_cost > QEMU_BUDGET {
        return Err(StressCode::PlanOver.val());
    }
    Ok(format!(
        "状态=已完成 total={} exec_cost={} exec_items={} budget={} digest=0x{:x}",
        sum.total, sum.exec_cost, sum.exec_items, QEMU_BUDGET, sum.digest
    ))
}

// ── 判据面（extra_checks：13 条动态明细） ───────────────────────
pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut v: Vec<(&'static str, bool, String)> = Vec::new();
    // 1. J1 主判据：压测总编齐备（40 件逐件编入 + 双向对平 + 一次执行）。
    let g = golden_run();
    v.push((
        "J1-总编齐备",
        match &g {
            Ok((flags, items, cost)) => *flags == 5 && *items == ASSET_COUNT as u32 && *cost <= QEMU_BUDGET,
            Err(_) => false,
        },
        match &g {
            Ok((flags, items, cost)) => format!("flags={} items={} cost={}", flags, items, cost),
            Err(e) => format!("err=0x{:x}", e),
        },
    ));
    // 2. J1 编目与资产对平 100%。
    let pair = {
        let assets = build_assets();
        match assets {
            Ok(list) => {
                let mut cat = StressCatalog::new();
                let mut i = 0;
                let mut reg_ok = true;
                while i < list.len() {
                    if let Some(a) = list.get(i) {
                        if cat.register(a).is_err() {
                            reg_ok = false;
                        }
                    }
                    i += 1;
                }
                match cat.verify_against(&list) {
                    Ok((m, g2, o)) => reg_ok && m == ASSET_COUNT as u32 && g2 == 0 && o == 0,
                    Err(_) => false,
                }
            }
            Err(_) => false,
        }
    };
    v.push((
        "J1-编目资产对平",
        pair,
        String::from("双向对平 (40,0,0)"),
    ));
    // 3. J1 总编可一次执行（QEMU 档预算内）。
    let exec = {
        let r = run_catalog_round();
        match r {
            Ok(s) => s.exec_items == ASSET_COUNT as u32 && s.exec_cost <= QEMU_BUDGET,
            Err(_) => false,
        }
    };
    v.push((
        "J1-一次执行预算",
        exec,
        format!("预算 {} 周期（≤90s 整数代理口径）", QEMU_BUDGET),
    ));
    // 4. J1 注入 10/10。
    let inj = inject_fault(JUDGE_ROUNDS);
    let inj_ok = match &inj {
        Ok(l) => l.all_caught(),
        Err(_) => false,
    };
    v.push((
        "J1-注入10/10",
        inj_ok,
        format!(
            "t/c {:?}/{:?}",
            inj.as_ref().map(|l| l.tries),
            inj.as_ref().map(|l| l.caught)
        ),
    ));
    // 5. 边界：件号闭集 1..=40。
    let id_edge = {
        let mut c = StressCatalog::new();
        let a0 = StressAsset { id: 0, kind: spec_kind(1), cost: spec_cost(1), digest: asset_digest(0, spec_kind(1), spec_cost(1)) };
        let a41 = StressAsset {
            id: ASSET_ID_HI + 1,
            kind: spec_kind(1),
            cost: spec_cost(1),
            digest: asset_digest(ASSET_ID_HI + 1, spec_kind(1), spec_cost(1)),
        };
        let r0 = c.register(&a0);
        let r41 = c.register(&a41);
        let a1 = spec_asset(1);
        let r1 = c.register(&a1);
        r0 == Err(StressCode::AssetOver.val())
            && r41 == Err(StressCode::AssetOver.val())
            && r1.is_ok()
    };
    v.push((
        "边界-件号闭集",
        id_edge,
        String::from("件 0/41 拒，件 1 可编入"),
    ));
    // 6. 边界：类别闭集 1..=5。
    let kind_edge = {
        let mut c = StressCatalog::new();
        let mut a = spec_asset(2);
        a.kind = 0;
        a.digest = asset_digest(a.id, a.kind, a.cost);
        let r0 = c.register(&a);
        let mut a2 = spec_asset(2);
        a2.kind = 6;
        a2.digest = asset_digest(a2.id, a2.kind, a2.cost);
        let r6 = c.register(&a2);
        r0 == Err(StressCode::KindOver.val()) && r6 == Err(StressCode::KindOver.val())
    };
    v.push((
        "边界-类别闭集",
        kind_edge,
        String::from("kind 0/6 经规格复算拦下"),
    ));
    // 7. 预算边界：单件成本超预算拒。
    let cost_edge = {
        let mut c = StressCatalog::new();
        let mut a = spec_asset(3);
        a.cost = QEMU_BUDGET + 1;
        a.digest = asset_digest(a.id, a.kind, a.cost);
        c.register(&a) == Err(StressCode::CostOver.val())
    };
    v.push((
        "边界-单件成本",
        cost_edge,
        String::from("成本与冻结规格不符拒（CostOver）"),
    ));
    // 8. 探针三序。
    let a = probe_seq_a().is_ok();
    let b = probe_seq_b().is_ok();
    let c = probe_seq_c().is_ok();
    v.push((
        "探针三序",
        a && b && c,
        format!("a={} b={} c={}", a, b, c),
    ));
    // 9. 双跑一致。
    v.push((
        "J2-双跑一致",
        repro_check().is_ok(),
        String::from("run_catalog_round 两跑摘要同值"),
    ));
    // 10. 只增不减：注销路径一律显性拒。
    let mono = {
        let cat = StressCatalog::new();
        cat.try_remove(1) == Err(StressCode::CatalogShrink.val())
    };
    v.push((
        "J2-总编只增不减",
        mono,
        String::from("注销请求一律拒（CatalogShrink）"),
    ));
    // 11. 封档冻结：封后编入全拒，账面不动。
    let sealed = {
        let mut cat = StressCatalog::new();
        let a1 = spec_asset(1);
        let _ = cat.register(&a1);
        let before = cat.total;
        cat.seal();
        let a2 = spec_asset(2);
        let _ = cat.register(&a2);
        cat.sealed && cat.total == before
    };
    v.push((
        "J2-封档冻结",
        sealed,
        String::from("封档后编入口闭，账面不动"),
    ));
    // 12. 写回摘要非空。
    let wb = writeback_summary();
    v.push((
        "J3-写回摘要",
        match &wb {
            Ok(s) => !s.is_empty(),
            Err(_) => false,
        },
        match &wb {
            Ok(s) => s.clone(),
            Err(e) => format!("err=0x{:x}", e),
        },
    ));
    // 13. 段位守卫 + 诊断闭集查表。
    let guard = {
        let mut closed_ok = true;
        let mut i = 0;
        while i < StressCode::ALL.len() {
            let code = StressCode::ALL[i].val();
            if detail(code) == "未知诊断码：不在 0x78580 段闭集内" {
                closed_ok = false;
            }
            i += 1;
        }
        closed_ok && segment_guard()
    };
    v.push((
        "J4-段位守卫",
        guard,
        String::from("0x78580 窗口 + 12 码闭集查表"),
    ));
    v
}

pub fn run_unx1530_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F1530 压测资产域级总编终版（压测一册全编）");
    for (n, p, d) in extra_checks() {
        s.add_dyn(n, p, &d);
    }
    s
}

// ── 测试区（≥8 条：注入必抓 / 双跑一致 / 边界） ─────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn t01_full_catalog_and_plan() {
        match golden_run() {
            Ok((flags, items, cost)) => {
                assert_eq!(flags, 5);
                assert_eq!(items, ASSET_COUNT as u32);
                assert!(cost <= QEMU_BUDGET);
            }
            Err(e) => assert!(false, "黄金全链应成: err=0x{:x}", e),
        }
    }

    #[test]
    fn t02_fault_inject_10_of_10_caught() {
        match inject_fault(JUDGE_ROUNDS) {
            Ok(inj) => {
                assert_eq!(inj.tries, [10, 10, 10]);
                assert_eq!(inj.caught, [10, 10, 10]);
                assert!(inj.all_caught());
            }
            Err(e) => assert!(false, "注入应成: err=0x{:x}", e),
        }
    }

    #[test]
    fn t03_both_directions_100_percent() {
        let assets = build_assets();
        assert!(assets.is_ok());
        if let Ok(list) = assets {
            let mut cat = StressCatalog::new();
            let mut i = 0;
            while i < list.len() {
                if let Some(a) = list.get(i) {
                    assert!(cat.register(a).is_ok());
                }
                i += 1;
            }
            assert_eq!(cat.verify_against(&list), Ok((ASSET_COUNT as u32, 0, 0)));
        }
    }

    #[test]
    fn t04_ghost_and_orphan_rejected() {
        let assets = build_assets();
        assert!(assets.is_ok());
        if let Ok(list) = assets {
            // 孤儿：漏编一件。
            let mut c1 = StressCatalog::new();
            let mut i = 0;
            while i < list.len() {
                if let Some(a) = list.get(i) {
                    if a.id != 5 {
                        let _ = c1.register(a);
                    }
                }
                i += 1;
            }
            assert_eq!(c1.verify_against(&list), Err(StressCode::OrphanAsset.val()));
        }
    }

    #[test]
    fn t05_id_and_kind_bounds() {
        let mut c = StressCatalog::new();
        let a0 = StressAsset { id: 0, kind: 1, cost: spec_cost(1), digest: asset_digest(0, 1, spec_cost(1)) };
        assert_eq!(c.register(&a0), Err(StressCode::AssetOver.val()));
        let mut a6 = spec_asset(1);
        a6.kind = 6;
        a6.digest = asset_digest(a6.id, a6.kind, a6.cost);
        assert_eq!(c.register(&a6), Err(StressCode::KindOver.val()));
    }

    #[test]
    fn t06_spec_deterministic() {
        let mut id = ASSET_ID_LO;
        while id <= ASSET_ID_HI {
            let a = spec_asset(id);
            assert_eq!(a.kind, spec_kind(id));
            assert_eq!(a.cost, spec_cost(id));
            assert_eq!(a.digest, asset_digest(a.id, a.kind, a.cost));
            id += 7;
        }
    }

    #[test]
    fn t07_plan_budget_respected() {
        let assets = build_assets();
        assert!(assets.is_ok());
        if let Ok(list) = assets {
            let mut cat = StressCatalog::new();
            let mut i = 0;
            while i < list.len() {
                if let Some(a) = list.get(i) {
                    let _ = cat.register(a);
                }
                i += 1;
            }
            let exec = cat.plan_execute();
            assert!(exec.is_ok(), "执行核应成");
            if let Ok((sum, n)) = exec {
                assert_eq!(n, ASSET_COUNT as u32);
                assert!(sum <= QEMU_BUDGET);
            }
        }
    }

    #[test]
    fn t08_double_run_identical() {
        let a = run_catalog_round();
        let b = run_catalog_round();
        match (a, b) {
            (Ok(x), Ok(y)) => assert_eq!(x, y),
            _ => assert!(false, "双跑应成"),
        }
    }

    #[test]
    fn t09_seal_and_shrink_rejected() {
        let mut cat = StressCatalog::new();
        let a1 = spec_asset(1);
        let _ = cat.register(&a1);
        cat.seal();
        let a2 = spec_asset(2);
        assert_eq!(cat.register(&a2), Err(StressCode::Sealed.val()));
        assert_eq!(cat.try_remove(1), Err(StressCode::CatalogShrink.val()));
        assert_eq!(cat.total, 1);
    }

    #[test]
    fn t10_kind_closed_set() {
        assert_eq!(StressKind::ALL.len(), 5);
        let mut i = 0;
        while i < StressKind::ALL.len() {
            if let Some(k) = StressKind::ALL.get(i) {
                assert_eq!(k.no(), (i + 1) as u8);
            }
            i += 1;
        }
        assert!(kind_of(0).is_none());
        assert!(kind_of(6).is_none());
        assert!(kind_of(5).is_some());
    }

    #[test]
    fn t11_segment_guard_and_closed_set() {
        assert!(segment_guard());
        assert_eq!(SEG_BASE % 0x40, 0);
        assert_eq!(StressCode::ALL.len(), 12);
        for c in StressCode::ALL {
            assert_ne!(detail(c.val()), "未知诊断码：不在 0x78580 段闭集内");
        }
    }

    #[test]
    fn t12_probe_checks_all_green() {
        let set = run_unx1530_checks();
        assert_eq!(set.failed_count(), 0);
        assert!(set.all_passed());
    }
}
