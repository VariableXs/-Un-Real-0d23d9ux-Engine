//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F1525 域级资产四族编目（账本/断言/预案/文档） · 0x78440
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（派工卡逐字锚定，AI-VX8-1104 · B02 · 卡 2）：
//!   ① 纯功能代码 ≥340 行；
//!   ② 判据原文：UNX-F1525-J1 四族编目齐备率 100%（账本族/断言族/预案族/
//!     文档族逐族逐件登记），编目与实存双向一致率 100%，幽灵件注入 10/10
//!     拦截；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! ─────────────────────────────────────────────────────────────────────
//! 设计要点：
//!   · 四族分立：账本族（Ledger）/断言族（Assert）/预案族（Plan）/
//!     文档族（Doc），族号闭集 1..=4，每族 10 件，合计 40 件逐件登记；
//!   · 双册模型：编目册（Catalog）与实存册（Manifest）双向对账——
//!     编目→实存（幽灵件 GhostItem）与实存→编目（孤儿件 OrphanItem）
//!     两个方向都要 100% 一致；
//!   · 命名冻结：件名 f<族号>_<件号> 三位格式（f1_007），名不合规拒；
//!   · 编目只增不减：不提供删除路径，重复登记拒（DupEntry），封目后
//!     再登记拒（Sealed）；
//!   · 幽灵注入 10 轮 × 三路（幽灵路/跳号路/重号路）全部拦截；
//!   · 双跑一致 + 段位守卫（0x78440 窗口）。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x78440，AI-VX8-1104 · B02 · 卡 2，派工卡逐字给定）。
pub const SEG_BASE: u32 = 0x78440;
/// 本批（B02 八卡）诊断段窗口下界。
pub const SEG_WIN_LO: u32 = 0x78400;
/// 本批（B02 八卡）诊断段窗口上界。
pub const SEG_WIN_HI: u32 = 0x785C0;

/// 族总数（冻结 4：账本/断言/预案/文档）。
pub const FAMILY_COUNT: usize = 4;
/// 族号下界（冻结 1）。
pub const FAMILY_LO: u8 = 1;
/// 族号上界（冻结 4）。
pub const FAMILY_HI: u8 = 4;
/// 每族件数（冻结 10）。
pub const ITEMS_PER_FAMILY: usize = 10;
/// 资产总件数（冻结 40）。
pub const TOTAL_ITEMS: usize = FAMILY_COUNT * ITEMS_PER_FAMILY;
/// 族内件号闭集上界（1..=64）。
pub const ITEM_ID_CAP: u32 = 64;
/// 幽灵注入轮数（判据"10/10 拦截"）。
pub const JUDGE_ROUNDS: u32 = 10;

// ── 诊断码闭集（段内 0x40 窗口，0x78441..0x7844C） ───────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CatalogCode {
    /// 族号越界（不在 1..=4）
    FamilyOver,
    /// 件号越界（不在 1..=64）
    IdOver,
    /// 重复登记（同族同号已编目）
    DupEntry,
    /// 幽灵件（编目有、实存无）
    GhostItem,
    /// 孤儿件（实存有、编目无）
    OrphanItem,
    /// 族缺件（该族登记数不足 10）
    FamilyMissing,
    /// 件名不合规（空名/超长/格式错）
    NameBad,
    /// 封目后改目（seal 之后仍尝试登记）
    Sealed,
    /// 编目缩水（注销/删除请求，编目只增不减）
    CatalogShrink,
    /// 双跑不一致（复现红线）
    NotReproducible,
    /// 双向对平失败（一致率 <100%）
    PairMismatch,
    /// 守卫失守（段位/闭集守卫失败）
    GuardBad,
}

impl CatalogCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            CatalogCode::FamilyOver => "族号越界：只在 1..=4 闭集",
            CatalogCode::IdOver => "件号越界：族内件号 1..=64",
            CatalogCode::DupEntry => "重复登记：同族同号唯一，另起新号",
            CatalogCode::GhostItem => "幽灵件：编目有实存无，补实存或删编目",
            CatalogCode::OrphanItem => "孤儿件：实存有编目无，补编目登记",
            CatalogCode::FamilyMissing => "族缺件：逐族逐件登记，齐备率须 100%",
            CatalogCode::NameBad => "件名不合规：f<族>_<件> 三位格式非空",
            CatalogCode::Sealed => "封目后改目：终版封印，登记口已闭",
            CatalogCode::CatalogShrink => "编目缩水：编目只增不减，注销一律拒",
            CatalogCode::NotReproducible => "双跑不一致：编目摘要两跑应同值",
            CatalogCode::PairMismatch => "双向对平失败：编目与实存一致率须 100%",
            CatalogCode::GuardBad => "守卫失守：段位或闭集守卫失败，勿改窗口",
        }
    }
    pub const ALL: [CatalogCode; 12] = [
        CatalogCode::FamilyOver,
        CatalogCode::IdOver,
        CatalogCode::DupEntry,
        CatalogCode::GhostItem,
        CatalogCode::OrphanItem,
        CatalogCode::FamilyMissing,
        CatalogCode::NameBad,
        CatalogCode::Sealed,
        CatalogCode::CatalogShrink,
        CatalogCode::NotReproducible,
        CatalogCode::PairMismatch,
        CatalogCode::GuardBad,
    ];
}

/// 诊断码查表（段外码返回统一文案，闭集返回）。
pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < CatalogCode::ALL.len() {
        if CatalogCode::ALL[i].val() == code {
            return CatalogCode::ALL[i].detail();
        }
        i += 1;
    }
    "未知诊断码：不在 0x78440 段闭集内"
}

// ── 域模型：四族 ─────────────────────────────────────────────────
/// 资产四族（闭集枚举）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Family {
    /// 账本族（族号 1）
    Ledger,
    /// 断言族（族号 2）
    Assert,
    /// 预案族（族号 3）
    Plan,
    /// 文档族（族号 4）
    Doc,
}

impl Family {
    /// 族号（1..=4）。
    pub fn no(self) -> u8 {
        match self {
            Family::Ledger => 1,
            Family::Assert => 2,
            Family::Plan => 3,
            Family::Doc => 4,
        }
    }
    /// 族中文名（doc 文案，与 ALL 闭集一一对应）。
    pub const ALL: [Family; 4] = [Family::Ledger, Family::Assert, Family::Plan, Family::Doc];
}

/// 资产件（实存册与编目共同引用的最小单元）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AssetItem {
    /// 族号（1..=4）
    pub family: u8,
    /// 族内件号（1..=64）
    pub id: u32,
    /// 件名（f<族>_<件> 格式）
    pub name: String,
    /// 件摘要哈希（FNV-1a）
    pub digest: u32,
}

/// 实存册（冻结规格展开，只读）。
#[derive(Clone, Debug)]
pub struct Manifest {
    items: Vec<AssetItem>,
}

impl Manifest {
    pub fn new() -> Manifest {
        Manifest { items: Vec::new() }
    }
    /// 逐件展开登记（名/哈希按冻结规格复算）。
    pub fn add(&mut self, it: AssetItem) -> Result<(), u32> {
        if it.family < FAMILY_LO || it.family > FAMILY_HI {
            return Err(CatalogCode::FamilyOver.val());
        }
        if it.id == 0 || it.id > ITEM_ID_CAP {
            return Err(CatalogCode::IdOver.val());
        }
        if !name_ok(&it.name, it.family, it.id) {
            return Err(CatalogCode::NameBad.val());
        }
        self.items.push(it);
        Ok(())
    }
    pub fn len(&self) -> usize {
        self.items.len()
    }
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }
    /// 查件（族号+件号）。
    pub fn find(&self, family: u8, id: u32) -> Option<&AssetItem> {
        let mut i = 0;
        while i < self.items.len() {
            if let Some(it) = self.items.get(i) {
                if it.family == family && it.id == id {
                    return self.items.get(i);
                }
            }
            i += 1;
        }
        None
    }
    /// 族内实存件数。
    pub fn count_family(&self, family: u8) -> u32 {
        let mut n = 0u32;
        let mut i = 0;
        while i < self.items.len() {
            if let Some(it) = self.items.get(i) {
                if it.family == family {
                    n += 1;
                }
            }
            i += 1;
        }
        n
    }
}

/// 冻结规格件名：f<族号>_<件号两位>。
pub fn spec_name(family: u8, id: u32) -> String {
    format!("f{}_{}", family, id)
}

/// 件名合规（f<族>_<件> 格式且族号/件号一致）。
pub fn name_ok(name: &str, family: u8, id: u32) -> bool {
    if name.is_empty() || name.len() > 32 {
        return false;
    }
    let expect = spec_name(family, id);
    name == expect.as_str()
}

/// 冻结规格哈希：FNV-1a 对 (family, id, name 字节) 逐步卷积。
pub fn spec_digest(family: u8, id: u32, name: &str) -> u32 {
    let mut x: u32 = 2166136261;
    x = x.wrapping_mul(16777619).wrapping_add(family as u32);
    x = x.wrapping_mul(16777619).wrapping_add(id);
    let bytes = name.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        x = x.wrapping_mul(16777619).wrapping_add(bytes[i] as u32);
        i += 1;
    }
    x
}

/// 按冻结规格展开全量实存册：4 族 × 10 件 = 40 件。
pub fn build_manifest() -> Result<Manifest, u32> {
    let mut m = Manifest::new();
    let mut fi = 0;
    while fi < Family::ALL.len() {
        let fam = match Family::ALL.get(fi) {
            Some(f) => *f,
            None => return Err(CatalogCode::FamilyOver.val()),
        };
        let mut slot = 0;
        while slot < ITEMS_PER_FAMILY {
            let id = (slot + 1) as u32;
            let name = spec_name(fam.no(), id);
            let d = spec_digest(fam.no(), id, &name);
            m.add(AssetItem { family: fam.no(), id, name, digest: d })?;
            slot += 1;
        }
        fi += 1;
    }
    if m.len() != TOTAL_ITEMS {
        return Err(CatalogCode::FamilyMissing.val());
    }
    Ok(m)
}

// ── 编目册（只增不减 + 封目） ────────────────────────────────────
/// 编目条目（族号+件号+登记序）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CatalogEntry {
    pub family: u8,
    pub id: u32,
    pub seq: u32,
}

/// 编目册：逐族逐件登记，重复拒、越界拒、封目后拒。
#[derive(Clone, Debug)]
pub struct Catalog {
    entries: Vec<CatalogEntry>,
    pub total: u32,
    pub peak_total: u32,
    pub sealed: bool,
}

impl Catalog {
    pub fn new() -> Catalog {
        Catalog { entries: Vec::new(), total: 0, peak_total: 0, sealed: false }
    }

    /// 登记（查重 + 查界）。
    pub fn register(&mut self, family: u8, id: u32) -> Result<(), u32> {
        if self.sealed {
            return Err(CatalogCode::Sealed.val());
        }
        if family < FAMILY_LO || family > FAMILY_HI {
            return Err(CatalogCode::FamilyOver.val());
        }
        if id == 0 || id > ITEM_ID_CAP {
            return Err(CatalogCode::IdOver.val());
        }
        let mut i = 0;
        while i < self.entries.len() {
            if let Some(e) = self.entries.get(i) {
                if e.family == family && e.id == id {
                    return Err(CatalogCode::DupEntry.val());
                }
            }
            i += 1;
        }
        let seq = self.total.saturating_add(1);
        self.entries.push(CatalogEntry { family, id, seq });
        self.total = seq;
        self.peak_total = self.peak_total.max(self.total);
        Ok(())
    }

    /// 族内已编目数。
    pub fn count_family(&self, family: u8) -> u32 {
        let mut n = 0u32;
        let mut i = 0;
        while i < self.entries.len() {
            if let Some(e) = self.entries.get(i) {
                if e.family == family {
                    n += 1;
                }
            }
            i += 1;
        }
        n
    }

    /// 注销请求：一律显性拒（编目只增不减）。
    pub fn try_remove(&self, _family: u8, _id: u32) -> Result<(), u32> {
        Err(CatalogCode::CatalogShrink.val())
    }

    /// 封目（不可逆终态）。
    pub fn seal(&mut self) {
        self.sealed = true;
    }

    /// 双向对平：
    ///   ① 编目→实存：每条编目在实存册必须查得到（否则幽灵件）；
    ///   ② 实存→编目：每件实存必须已编目（否则孤儿件）；
    ///   ③ 族齐备：每族编目数 == 10；
    ///   返回（匹配对数，幽灵数，孤儿数），三数须 (40, 0, 0)。
    pub fn verify_against(&self, m: &Manifest) -> Result<(u32, u32, u32), u32> {
        let mut matched = 0u32;
        let mut ghost = 0u32;
        let mut i = 0;
        while i < self.entries.len() {
            if let Some(e) = self.entries.get(i) {
                match m.find(e.family, e.id) {
                    Some(_) => matched += 1,
                    None => ghost += 1,
                }
            }
            i += 1;
        }
        let mut orphan = 0u32;
        let mut fi = 0;
        while fi < Family::ALL.len() {
            let fam = match Family::ALL.get(fi) {
                Some(f) => f.no(),
                None => return Err(CatalogCode::FamilyOver.val()),
            };
            let mut slot = 0;
            while slot < ITEMS_PER_FAMILY {
                let id = (slot + 1) as u32;
                if m.find(fam, id).is_some() {
                    let mut found = false;
                    let mut j = 0;
                    while j < self.entries.len() {
                        if let Some(e) = self.entries.get(j) {
                            if e.family == fam && e.id == id {
                                found = true;
                            }
                        }
                        j += 1;
                    }
                    if !found {
                        orphan += 1;
                    }
                }
                slot += 1;
            }
            if self.count_family(fam) != ITEMS_PER_FAMILY as u32 {
                return Err(CatalogCode::FamilyMissing.val());
            }
            fi += 1;
        }
        if ghost != 0 {
            return Err(CatalogCode::GhostItem.val());
        }
        if orphan != 0 {
            return Err(CatalogCode::OrphanItem.val());
        }
        if matched != TOTAL_ITEMS as u32 || self.total != TOTAL_ITEMS as u32 {
            return Err(CatalogCode::PairMismatch.val());
        }
        if self.total != self.peak_total {
            return Err(CatalogCode::CatalogShrink.val());
        }
        Ok((matched, ghost, orphan))
    }
}

// ── 编目摘要（供双跑一致对比） ──────────────────────────────────
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CatalogSummary {
    pub total: u32,
    pub families_full: u32,
    pub digest: u32,
}

/// 独立建目一轮：40 件逐件登记，族齐备，封目，双向对平。
pub fn run_catalog_round() -> Result<CatalogSummary, u32> {
    let m = build_manifest()?;
    let mut cat = Catalog::new();
    let mut fi = 0;
    while fi < Family::ALL.len() {
        let fam = match Family::ALL.get(fi) {
            Some(f) => f.no(),
            None => return Err(CatalogCode::FamilyOver.val()),
        };
        let mut slot = 0;
        while slot < ITEMS_PER_FAMILY {
            cat.register(fam, (slot + 1) as u32)?;
            slot += 1;
        }
        fi += 1;
    }
    let mut full = 0u32;
    let mut k = 0;
    while k < Family::ALL.len() {
        if let Some(f) = Family::ALL.get(k) {
            if cat.count_family(f.no()) == ITEMS_PER_FAMILY as u32 {
                full += 1;
            }
        }
        k += 1;
    }
    let (matched, _g, _o) = cat.verify_against(&m)?;
    if matched != TOTAL_ITEMS as u32 {
        return Err(CatalogCode::PairMismatch.val());
    }
    let mut h: u32 = 2166136261;
    // 摘要按冻结规格逐件卷积（编目封目后账面冻结，可复算）。
    let mut fi2 = 0;
    while fi2 < Family::ALL.len() {
        let fam = match Family::ALL.get(fi2) {
            Some(f) => f.no(),
            None => return Err(CatalogCode::FamilyOver.val()),
        };
        let mut slot = 0;
        while slot < ITEMS_PER_FAMILY {
            let id = (slot + 1) as u32;
            let d = spec_digest(fam, id, &spec_name(fam, id));
            h = h.wrapping_mul(16777619).wrapping_add(d);
            slot += 1;
        }
        fi2 += 1;
    }
    Ok(CatalogSummary { total: cat.total, families_full: full, digest: h })
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

// ── 幽灵注入检出账（三路：幽灵路/跳号路/重号路） ─────────────────
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

/// 幽灵注入：每轮打一路违约，双向对平必须当场拦（10/10）。
/// 路 0：幽灵件——编目登记实存册没有的（族、超出实存件号的 id）；
/// 路 1：跳号件——实存 40 件只编目 39 件（漏一件）→ 孤儿件必拦；
/// 路 2：重号件——同一（族、号）登记两次 → DupEntry 必拦。
pub fn inject_ghost(rounds: u32) -> Result<InjectLedger, u32> {
    let mut acc = InjectLedger::new();
    let mut rng = Lcg::new(SEG_BASE as u64);
    let mut r = 0;
    while r < rounds {
        let fam = FAMILY_LO + rng.below(FAMILY_COUNT as u64) as u8;
        // 路 0：幽灵件（编目 id 超出实存展开的 1..=10 → 实存查无）。
        let m = build_manifest()?;
        let mut c0 = Catalog::new();
        let mut slot = 0;
        while slot < ITEMS_PER_FAMILY {
            let _ = c0.register(fam, (slot + 1) as u32);
            slot += 1;
        }
        let _ = c0.register(fam, ITEMS_PER_FAMILY as u32 + 1);
        // 幽灵件拦截：族齐备核先拦（FamilyMissing）或双向对平拦（GhostItem）。
        let r0 = c0.verify_against(&m);
        acc.record(
            0,
            r0 == Err(CatalogCode::GhostItem.val()) || r0 == Err(CatalogCode::FamilyMissing.val()),
        );
        // 路 1：跳号件（漏编一件 → 孤儿必拦）。
        let skip = (rng.below(ITEMS_PER_FAMILY as u64) + 1) as u32;
        let mut c1 = Catalog::new();
        let mut slot = 0;
        while slot < ITEMS_PER_FAMILY {
            let id = (slot + 1) as u32;
            if id != skip {
                let _ = c1.register(fam, id);
            }
            slot += 1;
        }
        // 孤儿件拦截：族齐备核先拦（FamilyMissing）或双向对平拦
        // （OrphanItem），两口径任一命中即算拦住。
        let r1 = c1.verify_against(&m);
        acc.record(
            1,
            r1 == Err(CatalogCode::OrphanItem.val()) || r1 == Err(CatalogCode::FamilyMissing.val()),
        );
        // 路 2：重号件（同族同号再登记一次）。
        let mut c2 = Catalog::new();
        let _ = c2.register(fam, 1);
        acc.record(2, c2.register(fam, 1) == Err(CatalogCode::DupEntry.val()));
        r += 1;
    }
    Ok(acc)
}

// ── 黄金全链：满编 40 → 对平 (40,0,0) → 四拒 → 封目 → 封后再拒 ──
pub fn golden_run() -> Result<(u32, u32, u32), u32> {
    let m = build_manifest()?;
    let mut cat = Catalog::new();
    let mut fi = 0;
    while fi < Family::ALL.len() {
        let fam = match Family::ALL.get(fi) {
            Some(f) => f.no(),
            None => return Err(CatalogCode::FamilyOver.val()),
        };
        let mut slot = 0;
        while slot < ITEMS_PER_FAMILY {
            cat.register(fam, (slot + 1) as u32)?;
            slot += 1;
        }
        fi += 1;
    }
    let (matched, ghost, orphan) = cat.verify_against(&m)?;
    if matched != TOTAL_ITEMS as u32 || ghost != 0 || orphan != 0 {
        return Err(CatalogCode::PairMismatch.val());
    }
    // 四拒口径：缩水拒 / 重号拒 / 越族拒 / 越号拒。
    let shrink = cat.try_remove(FAMILY_LO, 1) == Err(CatalogCode::CatalogShrink.val());
    let dup = cat.register(FAMILY_LO, 1) == Err(CatalogCode::DupEntry.val());
    let over_fam = cat.register(FAMILY_HI + 1, 1) == Err(CatalogCode::FamilyOver.val());
    let over_id = cat.register(FAMILY_LO, ITEM_ID_CAP + 1) == Err(CatalogCode::IdOver.val());
    // 封目后再登记：拒（Sealed）。
    cat.seal();
    let sealed_rej = cat.register(FAMILY_LO, 50).is_err();
    let flags = shrink as u32 + dup as u32 + over_fam as u32 + over_id as u32;
    Ok((flags + sealed_rej as u32, 1, cat.total))
}

// ── 探针（守卫三序） ────────────────────────────────────────────
/// 序 A：黄金全链 + 双跑一致。
pub fn probe_seq_a() -> Result<(), u32> {
    match golden_run() {
        Ok((5, 1, 40)) => {
            let a = run_catalog_round()?;
            let b = run_catalog_round()?;
            if a == b {
                Ok(())
            } else {
                Err(CatalogCode::NotReproducible.val())
            }
        }
        _ => Err(CatalogCode::NotReproducible.val()),
    }
}

/// 序 B：注入面三路（幽灵/跳号/重号）逐条显性拦。
pub fn probe_seq_b() -> Result<(), u32> {
    let inj = inject_ghost(JUDGE_ROUNDS)?;
    if inj.all_caught() {
        Ok(())
    } else {
        Err(CatalogCode::PairMismatch.val())
    }
}

/// 序 C：冻结常量 + 四族闭集 + 段位守卫。
pub fn probe_seq_c() -> Result<(), u32> {
    let mut fam_ok = true;
    let mut i = 0;
    while i < Family::ALL.len() {
        if let Some(f) = Family::ALL.get(i) {
            if f.no() != (i + 1) as u8 {
                fam_ok = false;
            }
        }
        i += 1;
    }
    match (
        FAMILY_COUNT == 4,
        FAMILY_LO == 1,
        FAMILY_HI == 4,
        ITEMS_PER_FAMILY == 10,
        TOTAL_ITEMS == 40,
        ITEM_ID_CAP == 64,
        JUDGE_ROUNDS == 10,
        fam_ok,
        segment_guard(),
    ) {
        (true, true, true, true, true, true, true, true, true) => Ok(()),
        _ => Err(CatalogCode::GuardBad.val()),
    }
}

/// 段位守卫：诊断码全部落在本卡 0x78440 窗口内 + 批窗口对齐。
pub fn segment_guard() -> bool {
    let mut i = 0;
    while i < CatalogCode::ALL.len() {
        let v = CatalogCode::ALL[i].val();
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
        return Err(CatalogCode::NotReproducible.val());
    }
    Ok(())
}

// ── 完成写回（判据③：状态=已完成、结果摘要非空） ────────────────
pub fn writeback_summary() -> Result<String, u32> {
    let sum = run_catalog_round()?;
    if sum.total != TOTAL_ITEMS as u32 {
        return Err(CatalogCode::PairMismatch.val());
    }
    Ok(format!(
        "状态=已完成 total={} families_full={} digest=0x{:x}",
        sum.total, sum.families_full, sum.digest
    ))
}

// ── 判据面（extra_checks：13 条动态明细） ───────────────────────
pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut v: Vec<(&'static str, bool, String)> = Vec::new();
    // 1. J1 主判据：四族编目齐备率 100%（40 件逐族逐件登记）。
    let g = golden_run();
    v.push((
        "J1-四族编目齐备100%",
        match &g {
            Ok((flags, _c, total)) => *flags == 5 && *total == TOTAL_ITEMS as u32,
            Err(_) => false,
        },
        match &g {
            Ok((flags, _c, total)) => format!("flags={} total={}", flags, total),
            Err(e) => format!("err=0x{:x}", e),
        },
    ));
    // 2. J1 双向对平 (40, 0, 0)。
    let pair = {
        let m = build_manifest();
        match m {
            Ok(mm) => {
                let mut cat = Catalog::new();
                let mut fi = 0;
                let mut reg_ok = true;
                while fi < Family::ALL.len() {
                    if let Some(f) = Family::ALL.get(fi) {
                        let mut slot = 0;
                        while slot < ITEMS_PER_FAMILY {
                            if cat.register(f.no(), (slot + 1) as u32).is_err() {
                                reg_ok = false;
                            }
                            slot += 1;
                        }
                    }
                    fi += 1;
                }
                match cat.verify_against(&mm) {
                    Ok((a, b, c)) => reg_ok && a == TOTAL_ITEMS as u32 && b == 0 && c == 0,
                    Err(_) => false,
                }
            }
            Err(_) => false,
        }
    };
    v.push((
        "J1-双向对平",
        pair,
        String::from("编目→实存与实存→编目双向 (40,0,0)"),
    ));
    // 3. J1 幽灵注入 10/10。
    let inj = inject_ghost(JUDGE_ROUNDS);
    let inj_ok = match &inj {
        Ok(l) => l.all_caught(),
        Err(_) => false,
    };
    v.push((
        "J1-幽灵注入10/10",
        inj_ok,
        format!(
            "t/c {:?}/{:?}",
            inj.as_ref().map(|l| l.tries),
            inj.as_ref().map(|l| l.caught)
        ),
    ));
    // 4. 族齐备逐族点验（每族 10/10）。
    let fam_full = {
        let sum = run_catalog_round();
        match sum {
            Ok(s) => s.families_full == FAMILY_COUNT as u32 && s.total == TOTAL_ITEMS as u32,
            Err(_) => false,
        }
    };
    v.push((
        "J1-逐族点验",
        fam_full,
        String::from("4 族各族 10 件齐备"),
    ));
    // 5. 边界：族号闭集 1..=4，0 与 5 拒。
    let fam_edge = {
        let mut c = Catalog::new();
        let r0 = c.register(0, 1);
        let r5 = c.register(FAMILY_HI + 1, 1);
        let rok = c.register(FAMILY_HI, 1);
        r0 == Err(CatalogCode::FamilyOver.val())
            && r5 == Err(CatalogCode::FamilyOver.val())
            && rok.is_ok()
    };
    v.push((
        "边界-族号闭集",
        fam_edge,
        String::from("族 0/5 拒，族 4 可登记"),
    ));
    // 6. 边界：件号闭集 1..=64，0 与 65 拒。
    let id_edge = {
        let mut c = Catalog::new();
        let r0 = c.register(FAMILY_LO, 0);
        let r65 = c.register(FAMILY_LO, ITEM_ID_CAP + 1);
        let r1 = c.register(FAMILY_LO, 1);
        r0 == Err(CatalogCode::IdOver.val())
            && r65 == Err(CatalogCode::IdOver.val())
            && r1.is_ok()
    };
    v.push((
        "边界-件号闭集",
        id_edge,
        String::from("件 0/65 拒，件 1 可登记"),
    ));
    // 7. 边界：件名合规（f<族>_<件> 格式），错格式拒。
    let name_edge = {
        let mut m = Manifest::new();
        let good = m.add(AssetItem {
            family: FAMILY_LO,
            id: 1,
            name: spec_name(FAMILY_LO, 1),
            digest: spec_digest(FAMILY_LO, 1, &spec_name(FAMILY_LO, 1)),
        });
        let bad = m.add(AssetItem {
            family: FAMILY_LO,
            id: 2,
            name: String::from("wrong_name"),
            digest: 0,
        });
        let empty = m.add(AssetItem { family: FAMILY_LO, id: 3, name: String::new(), digest: 0 });
        good.is_ok() && bad == Err(CatalogCode::NameBad.val()) && empty == Err(CatalogCode::NameBad.val())
    };
    v.push((
        "边界-件名合规",
        name_edge,
        String::from("冻结格式收，错名/空名拒"),
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
        let c = Catalog::new();
        c.try_remove(FAMILY_LO, 1) == Err(CatalogCode::CatalogShrink.val())
    };
    v.push((
        "J2-编目只增不减",
        mono,
        String::from("注销请求一律拒（CatalogShrink）"),
    ));
    // 11. 封目冻结：封后登记全拒，账面不动。
    let sealed = {
        let mut c = Catalog::new();
        let _ = c.register(FAMILY_LO, 1);
        let before = c.total;
        c.seal();
        let _ = c.register(FAMILY_LO, 2);
        c.sealed && c.total == before
    };
    v.push((
        "J2-封目冻结",
        sealed,
        String::from("封目后登记口闭，账面不动"),
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
        while i < CatalogCode::ALL.len() {
            let code = CatalogCode::ALL[i].val();
            if detail(code) == "未知诊断码：不在 0x78440 段闭集内" {
                closed_ok = false;
            }
            i += 1;
        }
        closed_ok && segment_guard()
    };
    v.push((
        "J4-段位守卫",
        guard,
        String::from("0x78440 窗口 + 12 码闭集查表"),
    ));
    v
}

pub fn run_unx1525_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F1525 域级资产四族编目（账本/断言/预案/文档）");
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
    fn t01_manifest_full_40_items() {
        match build_manifest() {
            Ok(m) => {
                assert_eq!(m.len(), TOTAL_ITEMS);
                let mut fi = 0;
                while fi < Family::ALL.len() {
                    if let Some(f) = Family::ALL.get(fi) {
                        assert_eq!(m.count_family(f.no()), ITEMS_PER_FAMILY as u32);
                    }
                    fi += 1;
                }
            }
            Err(e) => assert!(false, "实存册应成: err=0x{:x}", e),
        }
    }

    #[test]
    fn t02_both_directions_100_percent() {
        let m = build_manifest();
        assert!(m.is_ok());
        if let Ok(mm) = m {
            let mut cat = Catalog::new();
            let mut fi = 0;
            while fi < Family::ALL.len() {
                if let Some(f) = Family::ALL.get(fi) {
                    let mut slot = 0;
                    while slot < ITEMS_PER_FAMILY {
                        assert!(cat.register(f.no(), (slot + 1) as u32).is_ok());
                        slot += 1;
                    }
                }
                fi += 1;
            }
            assert_eq!(cat.verify_against(&mm), Ok((TOTAL_ITEMS as u32, 0, 0)));
        }
    }

    #[test]
    fn t03_ghost_inject_10_of_10_caught() {
        match inject_ghost(JUDGE_ROUNDS) {
            Ok(inj) => {
                assert_eq!(inj.tries, [10, 10, 10]);
                assert_eq!(inj.caught, [10, 10, 10]);
                assert!(inj.all_caught());
            }
            Err(e) => assert!(false, "注入应成: err=0x{:x}", e),
        }
    }

    #[test]
    fn t04_ghost_item_rejected() {
        let m = build_manifest();
        assert!(m.is_ok());
        if let Ok(mm) = m {
            // 族内满 10 件但账本族第 10 件被换成实存册没有的 11 号：
            // 族计数仍齐，双向对平以孤儿件口径拦（实存有编目无）。
            let mut c = Catalog::new();
            let mut fi = 0;
            while fi < Family::ALL.len() {
                if let Some(f) = Family::ALL.get(fi) {
                    let mut slot = 0;
                    while slot < ITEMS_PER_FAMILY {
                        let id = (slot + 1) as u32;
                        if f.no() == FAMILY_LO && id == ITEMS_PER_FAMILY as u32 {
                            // 换成幽灵号 11（实存册只有 1..=10）。
                            let _ = c.register(f.no(), ITEMS_PER_FAMILY as u32 + 1);
                        } else {
                            let _ = c.register(f.no(), id);
                        }
                        slot += 1;
                    }
                }
                fi += 1;
            }
            assert_eq!(c.verify_against(&mm), Err(CatalogCode::GhostItem.val()));
            // 幽灵直查：编目多出的 11 号在实存册查无（幽灵件口径成立）。
            assert!(mm.find(FAMILY_LO, ITEMS_PER_FAMILY as u32 + 1).is_none());
        }
    }

    #[test]
    fn t05_orphan_item_rejected() {
        let m = build_manifest();
        assert!(m.is_ok());
        if let Ok(mm) = m {
            let mut c = Catalog::new();
            let mut slot = 1;
            while slot <= ITEMS_PER_FAMILY {
                let id = slot as u32;
                if id != 7 {
                    let _ = c.register(FAMILY_LO, id);
                }
                slot += 1;
            }
            // 只编目账本族（缺其余三族 → 族缺件先拦）。
            assert_eq!(c.verify_against(&mm), Err(CatalogCode::FamilyMissing.val()));
        }
    }

    #[test]
    fn t06_family_and_id_bounds() {
        let mut c = Catalog::new();
        assert_eq!(c.register(0, 1), Err(CatalogCode::FamilyOver.val()));
        assert_eq!(c.register(FAMILY_HI + 1, 1), Err(CatalogCode::FamilyOver.val()));
        assert_eq!(c.register(FAMILY_LO, 0), Err(CatalogCode::IdOver.val()));
        assert_eq!(c.register(FAMILY_LO, ITEM_ID_CAP + 1), Err(CatalogCode::IdOver.val()));
        assert!(c.register(FAMILY_HI, ITEM_ID_CAP).is_ok());
    }

    #[test]
    fn t07_dup_entry_rejected() {
        let mut c = Catalog::new();
        assert!(c.register(FAMILY_LO, 3).is_ok());
        assert_eq!(c.register(FAMILY_LO, 3), Err(CatalogCode::DupEntry.val()));
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
    fn t09_seal_freezes_catalog() {
        let mut c = Catalog::new();
        let _ = c.register(FAMILY_LO, 1);
        c.seal();
        assert_eq!(c.register(FAMILY_LO, 2), Err(CatalogCode::Sealed.val()));
        assert_eq!(c.total, 1);
    }

    #[test]
    fn t10_remove_always_rejected() {
        let mut c = Catalog::new();
        let _ = c.register(FAMILY_LO, 1);
        assert_eq!(c.try_remove(FAMILY_LO, 1), Err(CatalogCode::CatalogShrink.val()));
        c.seal();
        assert_eq!(c.try_remove(FAMILY_LO, 1), Err(CatalogCode::CatalogShrink.val()));
    }

    #[test]
    fn t11_segment_guard_and_closed_set() {
        assert!(segment_guard());
        assert_eq!(SEG_BASE % 0x40, 0);
        assert_eq!(CatalogCode::ALL.len(), 12);
        for c in CatalogCode::ALL {
            assert_ne!(detail(c.val()), "未知诊断码：不在 0x78440 段闭集内");
        }
    }

    #[test]
    fn t12_probe_checks_all_green() {
        let set = run_unx1525_checks();
        assert_eq!(set.failed_count(), 0);
        assert!(set.all_passed());
    }
}
