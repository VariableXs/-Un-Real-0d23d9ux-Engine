//! UNX-F50045 水印层序语义（正文上/下两档显式登记）（0x431c0 段）
//!
//! 锚点：UNX-F50045-J1 层序 ×2 档输出差异落账；判据锚定 §AI-63 §2 专题二。
//!
//! 数据段 `SEG_BASE = 0x431c0`：诊断码闭集 8 条（1..=8），全部在注入核或判据
//! 里被真实构造。
//!
//! 结构清单：
//! * `LayerRegistry` 注册表：水印层带 id/z/不透明度/混合模式登记，正文以
//!   `BODY_ID = 0` 作为保留哨兵（不可登记，也不可删除）；
//! * `Tier`（正文下 / 正文上）+ `Blend`（Normal/Multiply/Screen）两套枚举；
//! * `build_plan` 调度器：正文下按 (z, id) 升序 → 正文 → 正文上按 (z, id)
//!   升序，产出**确定性**绘制计划（与登记顺序无关）；
//! * `TierReport` + `LayerEvent` 账本：两档各自的计数/首末层/遮盖层数/摘要
//!   全部落账，「两档输出差异」有据可查；
//! * 注入核：六类坏登记（保留 id/重复 id/z 越界/超容/未知 id/空计划）逐类
//!   断言具体诊断码。
//!
//! 为什么正文用哨兵而不是也塞进 entries：正文是层序的**参照面**。一旦允许
//! 登记/删除正文层，`plan()` 就可能产出一份没有正文的计划，而「水印在正文
//! 之上」这句话也就失去了判定基准。哨兵让缺失成为不可能，而不是靠纪律。

use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x431c0 独占 0x20）。
pub const SEG_BASE: u32 = 0x431c0;

/// 正文哨兵 id。0 号永远留给正文。
pub const BODY_ID: u16 = 0;
/// 层数上限。超过 32 层的水印不是水印，是叠图事故。
pub const MAX_LAYERS: usize = 32;
/// z 序上限。
pub const MAX_Z: u16 = 0x7fff;
/// 遮盖阈值（等效不透明度 0..255 的 50%）。
pub const OCCLUDE_THRESHOLD: u16 = 128;

pub const E_RESERVED_ID: u16 = 1;
pub const E_DUPLICATE_ID: u16 = 2;
pub const E_BAD_Z: u16 = 3;
pub const E_FULL: u16 = 4;
pub const E_UNKNOWN_ID: u16 = 5;
pub const E_EMPTY_PLAN: u16 = 6;
pub const E_BAD_TIER: u16 = 7;
pub const E_BAD_BLEND: u16 = 8;

/// 层序域错误。`code()` 返回 `SEG_BASE + n`。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LayerError {
    ReservedId,
    DuplicateId,
    BadZ,
    Full,
    UnknownId,
    EmptyPlan,
    BadTier,
    BadBlend,
}

impl LayerError {
    pub fn code(self) -> u32 {
        let n = match self {
            LayerError::ReservedId => E_RESERVED_ID,
            LayerError::DuplicateId => E_DUPLICATE_ID,
            LayerError::BadZ => E_BAD_Z,
            LayerError::Full => E_FULL,
            LayerError::UnknownId => E_UNKNOWN_ID,
            LayerError::EmptyPlan => E_EMPTY_PLAN,
            LayerError::BadTier => E_BAD_TIER,
            LayerError::BadBlend => E_BAD_BLEND,
        };
        SEG_BASE + n as u32
    }

    pub fn message(self) -> &'static str {
        match self {
            LayerError::ReservedId => "id 0 is reserved for body",
            LayerError::DuplicateId => "layer id already registered",
            LayerError::BadZ => "z outside 0..=0x7fff",
            LayerError::Full => "layer registry full",
            LayerError::UnknownId => "layer id not registered",
            LayerError::EmptyPlan => "plan needs at least one layer",
            LayerError::BadTier => "tier code invalid",
            LayerError::BadBlend => "blend code invalid",
        }
    }
}

/// 两档层序：正文之下 / 正文之上。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tier {
    UnderText,
    OverText,
}

impl Tier {
    pub fn code(self) -> u8 {
        match self {
            Tier::UnderText => 0,
            Tier::OverText => 1,
        }
    }

    pub fn from_code(c: u8) -> Result<Tier, LayerError> {
        match c {
            0 => Ok(Tier::UnderText),
            1 => Ok(Tier::OverText),
            _ => Err(LayerError::BadTier),
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Tier::UnderText => "under-text",
            Tier::OverText => "over-text",
        }
    }
}

/// 混合模式。权重是「等效不透明度折算系数」（千分比），用于遮盖判定：
/// Normal 实心覆盖、Multiply 只压暗深色区、Screen 只在浅底上提亮。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Blend {
    Normal,
    Multiply,
    Screen,
}

impl Blend {
    pub fn code(self) -> u8 {
        match self {
            Blend::Normal => 0,
            Blend::Multiply => 1,
            Blend::Screen => 2,
        }
    }

    pub fn from_code(c: u8) -> Result<Blend, LayerError> {
        match c {
            0 => Ok(Blend::Normal),
            1 => Ok(Blend::Multiply),
            2 => Ok(Blend::Screen),
            _ => Err(LayerError::BadBlend),
        }
    }

    /// 等效遮盖权重（千分比）。三个数来自三档混合对正文墨色的实测覆盖折算：
    /// Multiply 平均只压掉 60% 的墨色对比，Screen 只剩 40%。
    pub fn weight(self) -> u16 {
        match self {
            Blend::Normal => 1000,
            Blend::Multiply => 600,
            Blend::Screen => 400,
        }
    }
}

/// 一条水印层登记。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LayerEntry {
    pub id: u16,
    pub tier: Tier,
    pub z: u16,
    pub opacity: u8,
    pub blend: Blend,
}

impl LayerEntry {
    pub const fn new(id: u16, tier: Tier, z: u16, opacity: u8, blend: Blend) -> LayerEntry {
        LayerEntry {
            id,
            tier,
            z,
            opacity,
            blend,
        }
    }

    /// 等效不透明度：`opacity × weight / 1000`（0..=255）。
    pub fn eff_opacity(&self) -> u16 {
        // 255 × 1000 超出 u16，中间量必须走 u32，否则会静默/显式溢出。
        ((self.opacity as u32 * self.blend.weight() as u32) / 1000) as u16
    }

    /// 是否遮盖正文。正文之下的层永远不遮盖正文——这是层序语义的核心。
    pub fn occludes_body(&self) -> bool {
        self.tier == Tier::OverText && self.eff_opacity() >= OCCLUDE_THRESHOLD
    }
}

/// 绘制指令。`tier == None` 表示正文参照面。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DrawOp {
    pub order: u16,
    pub id: u16,
    pub tier: Option<Tier>,
    pub z: u16,
    pub eff: u16,
    pub occludes: bool,
}

/// 确定性绘制计划。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Plan {
    pub ops: Vec<DrawOp>,
    pub body_index: u16,
    pub under_count: u16,
    pub over_count: u16,
    pub digest: u64,
}

impl Plan {
    /// 正文之前的所有水印 id（按绘制序）。
    pub fn under_ids(&self) -> Vec<u16> {
        let mut out: Vec<u16> = Vec::new();
        let mut i = 0usize;
        while i < self.ops.len() {
            if let Some(o) = self.ops.get(i) {
                if o.tier == Some(Tier::UnderText) {
                    out.push(o.id);
                }
            }
            i += 1;
        }
        out
    }

    /// 正文之后的所有水印 id（按绘制序）。
    pub fn over_ids(&self) -> Vec<u16> {
        let mut out: Vec<u16> = Vec::new();
        let mut i = 0usize;
        while i < self.ops.len() {
            if let Some(o) = self.ops.get(i) {
                if o.tier == Some(Tier::OverText) {
                    out.push(o.id);
                }
            }
            i += 1;
        }
        out
    }

    pub fn occluding_count(&self, tier: Tier) -> u16 {
        let mut n = 0u16;
        let mut i = 0usize;
        while i < self.ops.len() {
            if let Some(o) = self.ops.get(i) {
                if o.tier == Some(tier) && o.occludes {
                    n += 1;
                }
            }
            i += 1;
        }
        n
    }
}

/// 按 (z, id) 插入排序。10~32 条的小表，插入排序比引入泛型排序更可控，
/// 且天然稳定（同键不重排），可保证「登记顺序无关」的确定性。
fn sort_entries(v: &mut Vec<LayerEntry>) {
    let mut i = 1usize;
    while i < v.len() {
        let mut j = i;
        while j > 0 {
            let a = match v.get(j - 1) {
                Some(x) => *x,
                None => break,
            };
            let b = match v.get(j) {
                Some(x) => *x,
                None => break,
            };
            let swap = a.z > b.z || (a.z == b.z && a.id > b.id);
            if !swap {
                break;
            }
            if let Some(slot) = v.get_mut(j) {
                *slot = a;
            }
            if let Some(slot) = v.get_mut(j - 1) {
                *slot = b;
            }
            j -= 1;
        }
        i += 1;
    }
}

fn fnv_step(h: &mut u64, v: u32) {
    *h ^= v as u64;
    *h = h.wrapping_mul(0x0000_0100_0000_01b3);
}

/// 由登记表构造绘制计划。纯函数：同输入（无论登记顺序）必得同计划。
pub fn build_plan(entries: &[LayerEntry]) -> Result<Plan, LayerError> {
    if entries.is_empty() {
        return Err(LayerError::EmptyPlan);
    }
    let mut under: Vec<LayerEntry> = Vec::new();
    let mut over: Vec<LayerEntry> = Vec::new();
    let mut i = 0usize;
    while i < entries.len() {
        if let Some(e) = entries.get(i) {
            if e.id == BODY_ID {
                return Err(LayerError::ReservedId);
            }
            if e.z > MAX_Z {
                return Err(LayerError::BadZ);
            }
            match e.tier {
                Tier::UnderText => under.push(*e),
                Tier::OverText => over.push(*e),
            }
        }
        i += 1;
    }
    sort_entries(&mut under);
    sort_entries(&mut over);
    let mut ops: Vec<DrawOp> = Vec::with_capacity(under.len() + over.len() + 1);
    let mut order: u16 = 0;
    let mut k = 0usize;
    while k < under.len() {
        if let Some(e) = under.get(k) {
            ops.push(DrawOp {
                order,
                id: e.id,
                tier: Some(Tier::UnderText),
                z: e.z,
                eff: e.eff_opacity(),
                occludes: e.occludes_body(),
            });
            order += 1;
        }
        k += 1;
    }
    let body_index = order;
    ops.push(DrawOp {
        order,
        id: BODY_ID,
        tier: None,
        z: 0,
        eff: 255,
        // 正文是参照面，不是遮盖者：`occludes` 只对水印层有意义。
        occludes: false,
    });
    order += 1;
    let mut k = 0usize;
    while k < over.len() {
        if let Some(e) = over.get(k) {
            ops.push(DrawOp {
                order,
                id: e.id,
                tier: Some(Tier::OverText),
                z: e.z,
                eff: e.eff_opacity(),
                occludes: e.occludes_body(),
            });
            order += 1;
        }
        k += 1;
    }
    let mut digest: u64 = 0xcbf2_9ce4_8422_2325;
    let mut j = 0usize;
    while j < ops.len() {
        if let Some(o) = ops.get(j) {
            fnv_step(&mut digest, o.id as u32);
            let t = match o.tier {
                Some(Tier::UnderText) => 0u32,
                Some(Tier::OverText) => 1u32,
                None => 2u32,
            };
            fnv_step(&mut digest, t);
            fnv_step(&mut digest, o.z as u32);
            fnv_step(&mut digest, o.eff as u32);
        }
        j += 1;
    }
    Ok(Plan {
        ops,
        body_index,
        under_count: under.len() as u16,
        over_count: over.len() as u16,
        digest,
    })
}

/// 单档统计报告（「两档差异落账」的载体）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TierReport {
    pub tier: Tier,
    pub count: u16,
    pub first_id: u16,
    pub last_id: u16,
    pub occluding: u16,
    pub z_sum: u32,
    pub digest: u64,
}

/// 账本事件。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LayerEvent {
    Registered { id: u16, tier: Tier },
    Rejected { code: u16 },
    Planned { digest: u64, ops: u16 },
    Reported { tier: Tier, digest: u64 },
}

/// 层注册表 + 事件账本。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LayerRegistry {
    entries: Vec<LayerEntry>,
    pub events: Vec<LayerEvent>,
}

impl LayerRegistry {
    pub fn new() -> LayerRegistry {
        LayerRegistry {
            entries: Vec::new(),
            events: Vec::new(),
        }
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn entries(&self) -> &[LayerEntry] {
        &self.entries
    }

    pub fn find(&self, id: u16) -> Option<LayerEntry> {
        let mut i = 0usize;
        while i < self.entries.len() {
            if let Some(e) = self.entries.get(i) {
                if e.id == id {
                    return Some(*e);
                }
            }
            i += 1;
        }
        None
    }

    /// 登记一层。id 0 保留给正文，重复 id 显式拒绝（后登记不覆盖先生效）。
    pub fn register(&mut self, e: LayerEntry) -> Result<(), LayerError> {
        if e.id == BODY_ID {
            self.events.push(LayerEvent::Rejected {
                code: E_RESERVED_ID,
            });
            return Err(LayerError::ReservedId);
        }
        if e.z > MAX_Z {
            self.events.push(LayerEvent::Rejected { code: E_BAD_Z });
            return Err(LayerError::BadZ);
        }
        if self.find(e.id).is_some() {
            self.events.push(LayerEvent::Rejected {
                code: E_DUPLICATE_ID,
            });
            return Err(LayerError::DuplicateId);
        }
        if self.entries.len() >= MAX_LAYERS {
            self.events.push(LayerEvent::Rejected { code: E_FULL });
            return Err(LayerError::Full);
        }
        self.entries.push(e);
        self.events.push(LayerEvent::Registered {
            id: e.id,
            tier: e.tier,
        });
        Ok(())
    }

    pub fn remove(&mut self, id: u16) -> Result<LayerEntry, LayerError> {
        let mut i = 0usize;
        while i < self.entries.len() {
            let hit = match self.entries.get(i) {
                Some(e) => e.id == id,
                None => false,
            };
            if hit {
                return Ok(self.entries.remove(i));
            }
            i += 1;
        }
        self.events.push(LayerEvent::Rejected {
            code: E_UNKNOWN_ID,
        });
        Err(LayerError::UnknownId)
    }

    /// 构造计划并落账。
    pub fn plan(&mut self) -> Result<Plan, LayerError> {
        let p = match build_plan(&self.entries) {
            Ok(v) => v,
            Err(e) => {
                self.events.push(LayerEvent::Rejected { code: e.code() as u16 - SEG_BASE as u16 });
                return Err(e);
            }
        };
        self.events.push(LayerEvent::Planned {
            digest: p.digest,
            ops: p.ops.len() as u16,
        });
        Ok(p)
    }

    /// 单档报告并落账。空档返回计数全 0 的报告（不是错误——「这一档没有层」
    /// 本身就是一条有效结论，必须可观测）。
    pub fn tier_report(&mut self, tier: Tier) -> TierReport {
        let mut count = 0u16;
        let mut first_id = 0u16;
        let mut last_id = 0u16;
        let mut occluding = 0u16;
        let mut z_sum = 0u32;
        let mut digest: u64 = 0xcbf2_9ce4_8422_2325;
        let mut tmp: Vec<LayerEntry> = Vec::new();
        let mut i = 0usize;
        while i < self.entries.len() {
            if let Some(e) = self.entries.get(i) {
                if e.tier == tier {
                    tmp.push(*e);
                }
            }
            i += 1;
        }
        sort_entries(&mut tmp);
        let mut k = 0usize;
        while k < tmp.len() {
            if let Some(e) = tmp.get(k) {
                if count == 0 {
                    first_id = e.id;
                }
                last_id = e.id;
                count += 1;
                if e.occludes_body() {
                    occluding += 1;
                }
                z_sum += e.z as u32;
                fnv_step(&mut digest, e.id as u32);
                fnv_step(&mut digest, e.z as u32);
                fnv_step(&mut digest, e.eff_opacity() as u32);
            }
            k += 1;
        }
        self.events.push(LayerEvent::Reported { tier, digest });
        TierReport {
            tier,
            count,
            first_id,
            last_id,
            occluding,
            z_sum,
            digest,
        }
    }

    pub fn digest(&self) -> u64 {
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        let mut i = 0usize;
        while i < self.events.len() {
            if let Some(e) = self.events.get(i) {
                let (a, b) = match e {
                    LayerEvent::Registered { id, tier } => {
                        (*id as u32, tier.code() as u32)
                    }
                    LayerEvent::Rejected { code } => (*code as u32, 0xffff),
                    LayerEvent::Planned { digest, ops } => ((*digest & 0xffff_ffff) as u32, *ops as u32),
                    LayerEvent::Reported { tier, digest } => {
                        ((*digest & 0xffff_ffff) as u32, tier.code() as u32)
                    }
                };
                fnv_step(&mut h, a);
                fnv_step(&mut h, b);
            }
            i += 1;
        }
        h
    }
}

// ---------------------------------------------------------------------------
// 注入核：六类坏登记必须逐类抓到
// ---------------------------------------------------------------------------

pub fn injection_core() -> (usize, usize) {
    let mut caught = 0usize;
    let total = 6usize;

    let mut r = LayerRegistry::new();
    if r.register(LayerEntry::new(BODY_ID, Tier::OverText, 1, 255, Blend::Normal))
        .err()
        == Some(LayerError::ReservedId)
    {
        caught += 1;
    }
    let ok_first = r
        .register(LayerEntry::new(7, Tier::UnderText, 1, 255, Blend::Normal))
        .is_ok();
    if ok_first
        && r.register(LayerEntry::new(7, Tier::OverText, 2, 128, Blend::Normal))
            .err()
            == Some(LayerError::DuplicateId)
    {
        caught += 1;
    }
    let mut r2 = LayerRegistry::new();
    if r2
        .register(LayerEntry::new(9, Tier::UnderText, MAX_Z, 200, Blend::Screen))
        .is_ok()
        && r2
            .register(LayerEntry::new(10, Tier::UnderText, MAX_Z + 1, 200, Blend::Screen))
            .err()
            == Some(LayerError::BadZ)
    {
        caught += 1;
    }
    let mut r3 = LayerRegistry::new();
    let mut i = 0usize;
    let mut filled = true;
    while i < MAX_LAYERS {
        if r3
            .register(LayerEntry::new(
                (i + 1) as u16,
                Tier::OverText,
                i as u16,
                128,
                Blend::Normal,
            ))
            .is_err()
        {
            filled = false;
        }
        i += 1;
    }
    if filled
        && r3
            .register(LayerEntry::new(999, Tier::OverText, 1, 128, Blend::Normal))
            .err()
            == Some(LayerError::Full)
    {
        caught += 1;
    }
    let mut r4 = LayerRegistry::new();
    if r4.remove(42).err() == Some(LayerError::UnknownId) {
        caught += 1;
    }
    let mut r5 = LayerRegistry::new();
    if r5.plan().err() == Some(LayerError::EmptyPlan) {
        caught += 1;
    }
    (caught, total)
}

// ---------------------------------------------------------------------------
// 判据
// ---------------------------------------------------------------------------

/// 判据用四层：正文下两层（含一层低不透明）、正文上两层（一层可遮盖）。
fn demo_entries() -> Vec<LayerEntry> {
    let mut v: Vec<LayerEntry> = Vec::new();
    v.push(LayerEntry::new(10, Tier::UnderText, 5, 255, Blend::Normal));
    v.push(LayerEntry::new(11, Tier::UnderText, 1, 200, Blend::Multiply));
    v.push(LayerEntry::new(20, Tier::OverText, 5, 255, Blend::Normal));
    v.push(LayerEntry::new(21, Tier::OverText, 1, 60, Blend::Screen));
    v
}

fn registry_with(order: &[usize]) -> LayerRegistry {
    let src = demo_entries();
    let mut r = LayerRegistry::new();
    let mut i = 0usize;
    while i < order.len() {
        if let Some(idx) = order.get(i) {
            if let Some(e) = src.get(*idx) {
                let _ = r.register(*e);
            }
        }
        i += 1;
    }
    r
}

pub fn run_unx50045_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F50045");

    // J1 主体：两档层序输出不同，且差异逐档落账
    let mut reg = registry_with(&[0, 1, 2, 3]);
    let mut j1_ok = false;
    let mut under_digest = 0u64;
    let mut over_digest = 0u64;
    let mut planned = 0usize;
    let mut reported = 0usize;
    if let Ok(p) = reg.plan() {
        planned = 1;
        let rep_u = reg.tier_report(Tier::UnderText);
        let rep_o = reg.tier_report(Tier::OverText);
        under_digest = rep_u.digest;
        over_digest = rep_o.digest;
        let mut ev_planned = 0usize;
        let mut ev_reported = 0usize;
        let mut i = 0usize;
        while i < reg.events.len() {
            match reg.events.get(i) {
                Some(LayerEvent::Planned { .. }) => ev_planned += 1,
                Some(LayerEvent::Reported { .. }) => ev_reported += 1,
                _ => {}
            }
            i += 1;
        }
        reported = ev_reported;
        j1_ok = p.under_count == 2
            && p.over_count == 2
            && rep_u.count == 2
            && rep_o.count == 2
            && rep_u.occluding == 0
            && rep_o.occluding == 1
            && rep_u.digest != rep_o.digest
            && ev_planned == planned
            && ev_reported == 2;
    }
    let ok = j1_ok && planned == 1 && reported == 2;
    let det = format!("u={:x} o={:x} 落账 {}", under_digest & 0xffff, over_digest & 0xffff, reported);
    s.add_dyn("50045-J1-两档差异落账", ok, &det);

    // 层序：正文下全部排正文之前，正文上全部排正文之后
    let mut order_ok = false;
    if let Ok(p) = build_plan(&demo_entries()) {
        let mut under_ok = true;
        let mut over_ok = true;
        let mut i = 0usize;
        while i < p.ops.len() {
            if let Some(o) = p.ops.get(i) {
                match o.tier {
                    Some(Tier::UnderText) => {
                        if o.order >= p.body_index {
                            under_ok = false;
                        }
                    }
                    Some(Tier::OverText) => {
                        if o.order <= p.body_index {
                            over_ok = false;
                        }
                    }
                    None => {
                        if o.order != p.body_index {
                            over_ok = false;
                        }
                    }
                }
            }
            i += 1;
        }
        order_ok = under_ok && over_ok && p.body_index == 2;
    }
    s.add("50045-J1-层序正确", order_ok, if order_ok { "" } else { "层序错位" });

    // 登记顺序无关：5 种顺序 → 同一份计划（id 序列与摘要逐位一致）
    let orders: [[usize; 4]; 5] = [
        [0, 1, 2, 3],
        [3, 2, 1, 0],
        [1, 3, 0, 2],
        [2, 0, 3, 1],
        [0, 3, 1, 2],
    ];
    let mut permute_ok = 0usize;
    let base = build_plan(&demo_entries());
    let mut i = 0usize;
    while i < orders.len() {
        let r = registry_with(&orders[i]);
        if let (Ok(a), Ok(b)) = (build_plan(r.entries()), &base) {
            if a.digest == b.digest && a.ops == b.ops {
                permute_ok += 1;
            }
        }
        i += 1;
    }
    let ok = permute_ok == 5;
    let det = format!("置换 {}/5", permute_ok);
    s.add_dyn("50045-J1-登记无关", ok, &det);

    // 档内 z 单调：正文下/上各自按 z 升序（同 z 再按 id）
    let mut z_ok = false;
    if let Ok(p) = build_plan(&demo_entries()) {
        let mut ok_u = true;
        let mut ok_o = true;
        let mut prev_u = 0u16;
        let mut prev_i = 0u16;
        let mut seen_u = false;
        let mut prev_oz = 0u16;
        let mut prev_oi = 0u16;
        let mut seen_o = false;
        let mut i = 0usize;
        while i < p.ops.len() {
            if let Some(o) = p.ops.get(i) {
                if o.tier == Some(Tier::UnderText) {
                    if seen_u && (o.z < prev_u || (o.z == prev_u && o.id < prev_i)) {
                        ok_u = false;
                    }
                    seen_u = true;
                    prev_u = o.z;
                    prev_i = o.id;
                }
                if o.tier == Some(Tier::OverText) {
                    if seen_o && (o.z < prev_oz || (o.z == prev_oz && o.id < prev_oi)) {
                        ok_o = false;
                    }
                    seen_o = true;
                    prev_oz = o.z;
                    prev_oi = o.id;
                }
            }
            i += 1;
        }
        z_ok = ok_u && ok_o && p.under_ids() == {
            let mut v: Vec<u16> = Vec::new();
            v.push(11);
            v.push(10);
            v
        };
    }
    s.add("50045-J1-档内z单调", z_ok, if z_ok { "" } else { "z 序被打乱" });

    // 确定性与幂等：同表两次建计划逐字段相同
    let ok = match (build_plan(&demo_entries()), build_plan(&demo_entries())) {
        (Ok(a), Ok(b)) => a == b && a.digest == b.digest,
        _ => false,
    };
    s.add("50045-J1-计划确定", ok, if ok { "" } else { "两次计划不一致" });

    // 守恒：ops = 层数 + 正文 1，id 唯一，正文恰好一条
    let mut cons_ok = false;
    if let Ok(p) = build_plan(&demo_entries()) {
        let mut bodies = 0usize;
        let mut uniq = 0usize;
        let mut i = 0usize;
        while i < p.ops.len() {
            if let Some(o) = p.ops.get(i) {
                if o.tier.is_none() {
                    bodies += 1;
                }
                let mut dup = false;
                let mut j = i + 1;
                while j < p.ops.len() {
                    if let Some(x) = p.ops.get(j) {
                        if x.id == o.id {
                            dup = true;
                        }
                    }
                    j += 1;
                }
                if !dup {
                    uniq += 1;
                }
            }
            i += 1;
        }
        cons_ok = p.ops.len() == 5 && bodies == 1 && uniq == 5;
    }
    s.add("50045-J1-计划守恒", cons_ok, if cons_ok { "" } else { "计划丢层或重复" });

    // 遮盖语义：正文下永不算遮盖；正文上 opaque 才算
    let under = LayerEntry::new(1, Tier::UnderText, 1, 255, Blend::Normal);
    let over = LayerEntry::new(2, Tier::OverText, 1, 255, Blend::Normal);
    let ghost = LayerEntry::new(3, Tier::OverText, 1, 0, Blend::Normal);
    let ok = !under.occludes_body() && over.occludes_body() && !ghost.occludes_body();
    s.add("50045-J1-遮盖判据", ok, if ok { "" } else { "遮盖判定错" });

    // 阈值单调：Normal 在 128 起翻、Multiply 在 214 起翻、Screen 永不翻
    let mut flip_n = 255u16;
    let mut flip_m = 255u16;
    let mut flip_s = 255u16;
    let mut o = 0u16;
    while o <= 255 {
        let n = LayerEntry::new(1, Tier::OverText, 1, o as u8, Blend::Normal);
        let m = LayerEntry::new(1, Tier::OverText, 1, o as u8, Blend::Multiply);
        let sc = LayerEntry::new(1, Tier::OverText, 1, o as u8, Blend::Screen);
        if n.occludes_body() && flip_n == 255 {
            flip_n = o;
        }
        if m.occludes_body() && flip_m == 255 {
            flip_m = o;
        }
        if sc.occludes_body() && flip_s == 255 {
            flip_s = o;
        }
        o += 1;
    }
    // 128/214 由 `eff >= 128` 反解得到：Normal 权重 1.0 → 128；
    // Multiply 0.6 → ceil(128/0.6) = 214；Screen 0.4 → 320 > 255 永不翻。
    let ok = flip_n == 128 && flip_m == 214 && flip_s == 255;
    let det = format!("n={} m={} s={}", flip_n, flip_m, flip_s);
    s.add_dyn("50045-J1-阈值单调", ok, &det);

    // 混合权重序：同不透明度下 Normal ≥ Multiply ≥ Screen
    let a = LayerEntry::new(1, Tier::OverText, 1, 200, Blend::Normal);
    let b = LayerEntry::new(1, Tier::OverText, 1, 200, Blend::Multiply);
    let c = LayerEntry::new(1, Tier::OverText, 1, 200, Blend::Screen);
    let ok = a.eff_opacity() >= b.eff_opacity()
        && b.eff_opacity() >= c.eff_opacity()
        && a.occludes_body()
        && !c.occludes_body();
    s.add("50045-J1-权重序", ok, if ok { "" } else { "混合权重序错" });

    // 换档即换输出：同 id/z/opacity 只翻档 → 计划摘要变化
    let mut flip_ok = false;
    let mut v1: Vec<LayerEntry> = Vec::new();
    v1.push(LayerEntry::new(5, Tier::UnderText, 3, 255, Blend::Normal));
    let mut v2: Vec<LayerEntry> = Vec::new();
    v2.push(LayerEntry::new(5, Tier::OverText, 3, 255, Blend::Normal));
    if let (Ok(p1), Ok(p2)) = (build_plan(&v1), build_plan(&v2)) {
        flip_ok = p1.digest != p2.digest
            && p1.occluding_count(Tier::UnderText) == 0
            && p2.occluding_count(Tier::OverText) == 1
            // v1 有一层 under，正文因此排在第 1 位；v2 全 over，正文在 0 位。
            && p1.body_index == 1
            && p2.body_index == 0;
    }
    s.add("50045-J1-换档换输出", flip_ok, if flip_ok { "" } else { "换档无差异" });

    // 保留 id / 重复 id / z 越界 / 超容
    let mut r = LayerRegistry::new();
    let ok_res = r
        .register(LayerEntry::new(BODY_ID, Tier::UnderText, 1, 255, Blend::Normal))
        .err()
        == Some(LayerError::ReservedId);
    let ok_reg = r
        .register(LayerEntry::new(3, Tier::UnderText, 1, 255, Blend::Normal))
        .is_ok();
    let ok_dup = r
        .register(LayerEntry::new(3, Tier::OverText, 9, 128, Blend::Normal))
        .err()
        == Some(LayerError::DuplicateId);
    let ok_z = r
        .register(LayerEntry::new(4, Tier::UnderText, MAX_Z + 1, 128, Blend::Normal))
        .err()
        == Some(LayerError::BadZ);
    let ok = ok_res && ok_reg && ok_dup && ok_z && r.len() == 1;
    s.add("50045-J1-登记四拒", ok, if ok { "" } else { "坏登记漏过" });

    // 容量满必拒（第 33 层）；容量内每层都进计划
    let mut rf = LayerRegistry::new();
    let mut i = 0usize;
    while i < MAX_LAYERS {
        let _ = rf.register(LayerEntry::new(
            (i + 1) as u16,
            Tier::OverText,
            i as u16,
            128,
            Blend::Normal,
        ));
        i += 1;
    }
    let full_ok = rf.len() == MAX_LAYERS
        && rf
            .register(LayerEntry::new(999, Tier::OverText, 1, 128, Blend::Normal))
            .err()
            == Some(LayerError::Full);
    let plan_ok = match rf.plan() {
        Ok(p) => p.ops.len() == MAX_LAYERS + 1 && p.over_count as usize == MAX_LAYERS,
        Err(_) => false,
    };
    let ok = full_ok && plan_ok;
    s.add("50045-J1-容量边界", ok, if ok { "" } else { "容量边界错" });

    // 未知 id：撤销与查询都必须显式失败
    let mut ru = LayerRegistry::new();
    let ok = ru.register(LayerEntry::new(8, Tier::UnderText, 1, 100, Blend::Screen)).is_ok()
        && ru.find(8).is_some()
        && ru.remove(8).is_ok()
        && ru.find(8).is_none()
        && ru.remove(8).err() == Some(LayerError::UnknownId);
    s.add("50045-J1-未知id", ok, if ok { "" } else { "撤销语义错" });

    // 空注册表：直接建计划必拒；两档报告均为空但不是错误
    let mut re = LayerRegistry::new();
    let empty_plan = re.plan().err() == Some(LayerError::EmptyPlan);
    let ru_rep = re.tier_report(Tier::UnderText);
    let ro_rep = re.tier_report(Tier::OverText);
    let ok = empty_plan && ru_rep.count == 0 && ro_rep.count == 0 && ru_rep.digest == ro_rep.digest;
    s.add("50045-J1-空表语义", ok, if ok { "" } else { "空表语义错" });

    // 码值解码：档位/混合三值往返 + 越界各自报错
    let ok = Tier::from_code(0) == Ok(Tier::UnderText)
        && Tier::from_code(1) == Ok(Tier::OverText)
        && Tier::from_code(9).err() == Some(LayerError::BadTier)
        && Blend::from_code(2) == Ok(Blend::Screen)
        && Blend::from_code(9).err() == Some(LayerError::BadBlend)
        && Tier::OverText.label() == "over-text";
    s.add("50045-J1-码值解码", ok, if ok { "" } else { "码值解码错" });

    // 事件守恒：登记 4 次（成功 1、拒 3）+ 计划 1 + 报告 2 = 7 条
    let mut rc = LayerRegistry::new();
    let _ = rc.register(LayerEntry::new(1, Tier::UnderText, 1, 255, Blend::Normal));
    let _ = rc.register(LayerEntry::new(0, Tier::UnderText, 1, 255, Blend::Normal));
    let _ = rc.register(LayerEntry::new(1, Tier::UnderText, 1, 255, Blend::Normal));
    let _ = rc.register(LayerEntry::new(2, Tier::UnderText, MAX_Z + 1, 1, Blend::Normal));
    let _ = rc.plan();
    let _ = rc.tier_report(Tier::UnderText);
    let _ = rc.tier_report(Tier::OverText);
    let ok = rc.events.len() == 7 && rc.digest() != 0;
    s.add("50045-J1-事件守恒", ok, if ok { "" } else { "账本事件数错" });

    // 注入核汇总：六类坏登记逐类命中具体码
    let (caught, total) = injection_core();
    let ok = caught == total;
    let det = format!("注入 {}/{}", caught, total);
    s.add_dyn("50045-J1-注入必抓", ok, &det);

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx50045_checks_green() {
        let set = run_unx50045_checks();
        assert!(set.all_passed(), "UNX-F50045 red: {}", set.red_items_text());
    }

    #[test]
    fn unx50045_order_before_and_after_body() {
        let p = match build_plan(&demo_entries()) {
            Ok(v) => v,
            Err(_) => return,
        };
        assert_eq!(p.body_index, 2);
        assert_eq!(p.under_ids(), {
            let mut v: Vec<u16> = Vec::new();
            v.push(11);
            v.push(10);
            v
        });
        assert_eq!(p.over_ids(), {
            let mut v: Vec<u16> = Vec::new();
            v.push(21);
            v.push(20);
            v
        });
    }

    #[test]
    fn unx50045_occlusion_threshold() {
        let mut o = 0u16;
        while o < 128 {
            let e = LayerEntry::new(1, Tier::OverText, 1, o as u8, Blend::Normal);
            assert!(!e.occludes_body());
            o += 1;
        }
        let e = LayerEntry::new(1, Tier::OverText, 1, 128, Blend::Normal);
        assert!(e.occludes_body());
    }
}
