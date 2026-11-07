//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F18425 · API 维度枚举（版本族全 API 登记） · 0x7E600 段
//! ─────────────────────────────────────────────────────────────────────
//! 锚点：UNX-F18425-J1 API 维度全集（12 族）登记与 B01 入口清单勾稽
//!       12/12 零缺漏
//! 本模块结构清单：
//!   1. 冻结常量层：12 族闭集（族号/族名/服务层/查询维度位面）；
//!   2. B01 入口清单（冻结 12 号，与卡 B01 入口备案逐号勾稽）；
//!   3. 诊断码闭集 AeCode（code()=SEG_BASE+n，detail() 中文闭集）；
//!   4. 族模型 ApiFamily：族号+族名+服务层+维度位面（6 位）；
//!   5. 登记册 ApiRegistry：12 槽数组账本，登记闸（族号闭集/
//!      重号/空名/位面杂质），全量勾稽 reconcile 双向核对；
//!   6. 注入核：族号越界/重号/空名/位面杂质 四类逐类 10/10 断言；
//!   7. 黄金全链：12 族全登记→B01 清单双向勾稽 12/12→注入全抓；
//!   8. run_unx18425_checks：15 条判据（断言性质）。
//! ═══════════════════════════════════════════════════════════════════════

use crate::alloc::format;
use crate::alloc::string::ToString;
use alloc::vec::Vec;
use alloc::string::String;
use crate::checks::CheckSet;

/// 诊断段基址（0x7E600，G1 工单 A03 批02 独占窗口，0x40 对齐）。
pub const SEG_BASE: u32 = 0x7E600;

// ── 冻结常量层 ─────────────────────────────────────────────────────
/// API 族全集（写死 12：Windows 版本族可谎报面盘点——
/// 0 GetVersion / 1 GetVersionExA / 2 GetVersionExW / 3 RtlGetVersion /
/// 4 GetProductInfo / 5 VerifyVersionInfo / 6 VerSetConditionMask /
/// 7 GetFileVersionInfo / 8 GetFileVersionInfoSize / 9 VerQueryValue /
/// 10 PEB 直读 / 11 WMI 查询。12 族为口径全量，扩族须换版）。
pub const API_FAMILIES: usize = 12;
/// 查询维度数（冻结 6：与 unx18426 六维枚举勾稽——
/// 主版本/次版本/Build/SP/ProductType/Suite）。
pub const QUERY_DIMS: usize = 6;
/// 判据轮数（写死 10）。
pub const JUDGE_ROUNDS: u32 = 10;
/// 服务层闭集（写死 3：0=Win32 用户层 1=GDI/外围 2=内核/运行时）。
pub const TIER_USER: u8 = 0;
pub const TIER_PERIPH: u8 = 1;
pub const TIER_KERNEL: u8 = 2;

/// 12 族名闭集（冻结 v1，序与族号一一对应）。
pub const FAMILY_NAMES: [&str; API_FAMILIES] = [
    "GetVersion",
    "GetVersionExA",
    "GetVersionExW",
    "RtlGetVersion",
    "GetProductInfo",
    "VerifyVersionInfo",
    "VerSetConditionMask",
    "GetFileVersionInfo",
    "GetFileVersionInfoSize",
    "VerQueryValue",
    "PebDirect",
    "WmiQuery",
];

/// 12 族服务层闭集（冻结：0/1/2，推导——版本核心 API 皆用户层，
/// PEB 直读与 WMI 归运行时/外围层）。
pub const FAMILY_TIERS: [u8; API_FAMILIES] = [
    TIER_USER,    // 0 GetVersion
    TIER_USER,    // 1 GetVersionExA
    TIER_USER,    // 2 GetVersionExW
    TIER_USER,    // 3 RtlGetVersion（ntdll 导出，仍用户态入口）
    TIER_USER,    // 4 GetProductInfo
    TIER_USER,    // 5 VerifyVersionInfo
    TIER_USER,    // 6 VerSetConditionMask
    TIER_PERIPH,  // 7 GetFileVersionInfo（版本资源外围）
    TIER_PERIPH,  // 8 GetFileVersionInfoSize
    TIER_PERIPH,  // 9 VerQueryValue
    TIER_KERNEL,  // 10 PEB 直读（运行时结构）
    TIER_KERNEL,  // 11 WMI 查询（系统服务面）
];

/// 12 族可查询维度位面（冻结：bit0 主版本 bit1 次版本 bit2 Build
/// bit3 SP bit4 ProductType bit5 Suite。推导——核心四族（0..4）
/// 全维可查；条件族（5/6）只查主/次；文件版本族（7..10）只查
/// 主/次/Build；PEB 直读查主/次/SP；WMI 查全维）。
pub const FAMILY_DIM_MASKS: [u8; API_FAMILIES] = [
    0b001111, // 0 GetVersion：主/次/Build/SP
    0b001111, // 1 GetVersionExA
    0b001111, // 2 GetVersionExW
    0b001111, // 3 RtlGetVersion
    0b011111, // 4 GetProductInfo：+ProductType
    0b000011, // 5 VerifyVersionInfo：主/次
    0b000011, // 6 VerSetConditionMask：主/次
    0b000111, // 7 GetFileVersionInfo：主/次/Build
    0b000111, // 8 GetFileVersionInfoSize
    0b000111, // 9 VerQueryValue
    0b001011, // 10 PEB 直读：主/次/SP
    0b111111, // 11 WMI 查询：全维
];

/// B01 入口清单（冻结 12 号：0..11 恒序——B01 入口备案逐号登记，
/// 勾稽即双向集合相等核对）。
pub const B01_ENTRY_LIST: [u8; API_FAMILIES] = [0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11];

// ── 诊断码闭集（段内 0x40 窗口） ───────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AeCode {
    /// 族号越界：落 0..11 闭集外
    FamilyOutOfRange,
    /// 族重号：同族号重复登记
    FamilyDuplicated,
    /// 族名空：登记名空串
    NameEmpty,
    /// 族名不符：与冻结闭集名不一致
    NameMismatch,
    /// 服务层越界：落 0..2 闭集外
    TierOutOfRange,
    /// 位面杂质：维度位面出现 bit5 之外高位
    DimMaskDirty,
    /// 位面为零：族不挂任何查询维度（空挂拒）
    DimMaskEmpty,
    /// 勾稽缺漏：B01 清单有号而登记册无
    ReconcileMissing,
    /// 勾稽多出：登记册有号而 B01 清单无
    ReconcileExtra,
    /// 册未满：12 族未登记全
    RegistryIncomplete,
    /// 双跑不一致
    NotReproducible,
    /// 清单自坏：B01 清单自身闭集破坏
    EntryListBroken,
}

impl AeCode {
    /// 诊断码取值：SEG_BASE + 枚举序（闭集，段内窗口 ≤0x40）。
    pub fn code(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 中文诊断文案（闭集，逐码非空）。
    pub fn detail(self) -> &'static str {
        match self {
            AeCode::FamilyOutOfRange => "族号越界：须落 0..11 闭集",
            AeCode::FamilyDuplicated => "族重号：族号在册唯一",
            AeCode::NameEmpty => "族名空：登记名不得为空串",
            AeCode::NameMismatch => "族名不符：与冻结闭集名不一致",
            AeCode::TierOutOfRange => "服务层越界：须落 0..2 闭集",
            AeCode::DimMaskDirty => "维度位面杂质：bit5 之外高位出现",
            AeCode::DimMaskEmpty => "维度位面为零：族空挂拒",
            AeCode::ReconcileMissing => "勾稽缺漏：B01 有号而册无",
            AeCode::ReconcileExtra => "勾稽多出：册有号而 B01 无",
            AeCode::RegistryIncomplete => "册未满：12 族未登记全",
            AeCode::NotReproducible => "双跑不一致：复现红线",
            AeCode::EntryListBroken => "清单自坏：B01 清单闭集破坏",
        }
    }
    /// 全集（闭集核对用）。
    pub const ALL: [AeCode; 12] = [
        AeCode::FamilyOutOfRange,
        AeCode::FamilyDuplicated,
        AeCode::NameEmpty,
        AeCode::NameMismatch,
        AeCode::TierOutOfRange,
        AeCode::DimMaskDirty,
        AeCode::DimMaskEmpty,
        AeCode::ReconcileMissing,
        AeCode::ReconcileExtra,
        AeCode::RegistryIncomplete,
        AeCode::NotReproducible,
        AeCode::EntryListBroken,
    ];
}

/// 未知码回查（闭集外返回固定文案）。
pub fn detail_of(code: u32) -> &'static str {
    let mut i = 0;
    while i < AeCode::ALL.len() {
        if let Some(c) = AeCode::ALL.get(i) {
            if c.code() == code {
                return c.detail();
            }
        }
        i += 1;
    }
    "未知诊断码：不在 0x7E600 段闭集内"
}

// ── 族模型 ─────────────────────────────────────────────────────────
/// 单族登记条目。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ApiFamily {
    /// 族号（0..11 闭集）。
    pub family_id: u8,
    /// 族名（须与 FAMILY_NAMES[fid] 全等）。
    pub name: String,
    /// 服务层（0/1/2 闭集）。
    pub tier: u8,
    /// 可查询维度位面（6 位，非零，无高位杂质）。
    pub dim_mask: u8,
}

impl ApiFamily {
    /// 位面合法：非零且无 bit5 之外高位。
    pub fn mask_legal(&self) -> bool {
        self.dim_mask != 0 && (self.dim_mask & !0b111111) == 0
    }
    /// 该族是否可查某维度（dim<6）。
    pub fn covers_dim(&self, dim: u8) -> bool {
        if dim as usize >= QUERY_DIMS {
            return false;
        }
        self.dim_mask & (1u8 << dim) != 0
    }
}

/// 族登记构造：五闸——族号闭集/族名非空/族名全等/服务层闭集/位面。
pub fn make_family(family_id: u8, name: &str, tier: u8, dim_mask: u8) -> Result<ApiFamily, u32> {
    if family_id as usize >= API_FAMILIES {
        return Err(AeCode::FamilyOutOfRange.code());
    }
    if name.is_empty() {
        return Err(AeCode::NameEmpty.code());
    }
    let frozen = match FAMILY_NAMES.get(family_id as usize) {
        Some(n) => *n,
        None => return Err(AeCode::FamilyOutOfRange.code()),
    };
    if name != frozen {
        return Err(AeCode::NameMismatch.code());
    }
    if tier > TIER_KERNEL {
        return Err(AeCode::TierOutOfRange.code());
    }
    let f = ApiFamily {
        family_id,
        name: name.to_string(),
        tier,
        dim_mask,
    };
    if f.dim_mask == 0 {
        return Err(AeCode::DimMaskEmpty.code());
    }
    if !f.mask_legal() {
        return Err(AeCode::DimMaskDirty.code());
    }
    Ok(f)
}

// ── 登记册 ─────────────────────────────────────────────────────────
/// API 登记册：12 槽数组账本（按族号直挂，None=未登记）。
pub struct ApiRegistry {
    /// 槽表：下标=族号（登记后 Some）。直挂式避免重复扫。
    slots: [Option<ApiFamily>; API_FAMILIES],
    pub tried: u32,
    pub accepted: u32,
    pub rejected: u32,
    pub reject_codes: Vec<u32>,
}

impl ApiRegistry {
    /// 新册：12 槽全空。
    pub fn new() -> ApiRegistry {
        ApiRegistry {
            slots: [None, None, None, None, None, None, None, None, None, None, None, None],
            tried: 0,
            accepted: 0,
            rejected: 0,
            reject_codes: Vec::new(),
        }
    }

    /// 登记一族：族号闭集→重号→构造五闸，拒因留账。
    pub fn register(&mut self, family_id: u8, name: &str, tier: u8, dim_mask: u8) -> Result<(), u32> {
        self.tried = self.tried.saturating_add(1);
        if family_id as usize >= API_FAMILIES {
            self.rejected = self.rejected.saturating_add(1);
            self.reject_codes.push(AeCode::FamilyOutOfRange.code());
            return Err(AeCode::FamilyOutOfRange.code());
        }
        let occupied = match self.slots.get(family_id as usize) {
            Some(s) => s.is_some(),
            None => true,
        };
        if occupied {
            self.rejected = self.rejected.saturating_add(1);
            self.reject_codes.push(AeCode::FamilyDuplicated.code());
            return Err(AeCode::FamilyDuplicated.code());
        }
        let f = match make_family(family_id, name, tier, dim_mask) {
            Ok(f) => f,
            Err(c) => {
                self.rejected = self.rejected.saturating_add(1);
                self.reject_codes.push(c);
                return Err(c);
            }
        };
        if let Some(slot) = self.slots.get_mut(family_id as usize) {
            *slot = Some(f);
        }
        self.accepted = self.accepted.saturating_add(1);
        Ok(())
    }

    /// 在册数。
    pub fn count(&self) -> usize {
        let mut n = 0;
        let mut i = 0;
        while i < API_FAMILIES {
            if let Some(s) = self.slots.get(i) {
                if s.is_some() {
                    n += 1;
                }
            }
            i += 1;
        }
        n
    }

    /// 按族号查族（只读副本）。
    pub fn find(&self, family_id: u8) -> Option<ApiFamily> {
        if family_id as usize >= API_FAMILIES {
            return None;
        }
        match self.slots.get(family_id as usize) {
            Some(Some(f)) => Some(f.clone()),
            _ => None,
        }
    }

    /// 全量勾稽（双向）：B01 清单每号在册（无缺漏）且册上每号在清单
    /// （无多出）。返回 (清单侧命中数, 册侧命中数)。
    pub fn reconcile(&self) -> Result<(usize, usize), u32> {
        // 清单自检：12 号两两互异且全部落 0..11。
        let mut seen = [0u8; API_FAMILIES];
        let mut i = 0;
        while i < API_FAMILIES {
            let id = match B01_ENTRY_LIST.get(i) {
                Some(v) => *v as usize,
                None => return Err(AeCode::EntryListBroken.code()),
            };
            if id >= API_FAMILIES {
                return Err(AeCode::EntryListBroken.code());
            }
            let c = match seen.get_mut(id) {
                Some(c) => c,
                None => return Err(AeCode::EntryListBroken.code()),
            };
            *c = c.saturating_add(1);
            i += 1;
        }
        let mut k = 0;
        while k < API_FAMILIES {
            if seen.get(k).copied().unwrap_or(0) != 1 {
                return Err(AeCode::EntryListBroken.code());
            }
            k += 1;
        }
        // 清单→册（无缺漏）。
        let mut hit_list = 0usize;
        let mut j = 0;
        while j < API_FAMILIES {
            let id = B01_ENTRY_LIST.get(j).copied().unwrap_or(0xFF);
            if self.find(id).is_some() {
                hit_list += 1;
            }
            j += 1;
        }
        // 册→清单（无多出）：册上每族号必须能在清单找到。
        let mut hit_reg = 0usize;
        let mut m: usize = 0;

        while m < API_FAMILIES {
            if let Some(Some(f)) = self.slots.get(m) {
                let mut in_list = false;
                let mut q = 0;
                while q < API_FAMILIES {
                    if B01_ENTRY_LIST.get(q).copied().unwrap_or(0xFF) == f.family_id {
                        in_list = true;
                    }
                    q += 1;
                }
                if !in_list {
                    return Err(AeCode::ReconcileExtra.code());
                }
                hit_reg += 1;
            }
            m += 1;
        }
        if hit_list != API_FAMILIES {
            return Err(AeCode::ReconcileMissing.code());
        }
        Ok((hit_list, hit_reg))
    }

    /// 计数守恒。
    pub fn conserve_ok(&self) -> bool {
        self.tried == self.accepted.saturating_add(self.rejected)
    }
}

/// 全 12 族按冻结常量登记（语料构造：名/层/位面全取自冻结数组）。
pub fn register_all(reg: &mut ApiRegistry) -> Result<(), u32> {
    let mut fid = 0u8;
    while (fid as usize) < API_FAMILIES {
        let name = FAMILY_NAMES.get(fid as usize).copied().unwrap_or("");
        let tier = FAMILY_TIERS.get(fid as usize).copied().unwrap_or(TIER_USER);
        let mask = FAMILY_DIM_MASKS.get(fid as usize).copied().unwrap_or(0);
        reg.register(fid, name, tier, mask)?;
        fid += 1;
    }
    Ok(())
}

// ── 注入核：四类坏样本 ─────────────────────────────────────────────
/// 坏样本类别。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BadClass {
    /// 类0：族号越界注入。
    IdOverflowInject,
    /// 类1：族重号注入。
    DupFamilyInject,
    /// 类2：空名注入。
    EmptyNameInject,
    /// 类3：位面杂质/零位面注入。
    MaskDirtyInject,
}

impl BadClass {
    /// 类号（注入账槽位）。
    pub fn slot(self) -> usize {
        match self {
            BadClass::IdOverflowInject => 0,
            BadClass::DupFamilyInject => 1,
            BadClass::EmptyNameInject => 2,
            BadClass::MaskDirtyInject => 3,
        }
    }
}

pub const BAD_CLASSES: usize = 4;

/// 注入检出账。
pub struct InjectLedger {
    pub tries: [u32; BAD_CLASSES],
    pub caught: [u32; BAD_CLASSES],
}

impl InjectLedger {
    pub fn new() -> InjectLedger {
        InjectLedger { tries: [0; BAD_CLASSES], caught: [0; BAD_CLASSES] }
    }
    /// 记一注。
    pub fn record(&mut self, slot: usize, hit: bool) {
        if slot >= BAD_CLASSES {
            return;
        }
        if let Some(t) = self.tries.get_mut(slot) {
            *t = t.saturating_add(1);
        }
        if hit {
            if let Some(c) = self.caught.get_mut(slot) {
                *c = c.saturating_add(1);
            }
        }
    }
    /// 全槽全抓。
    pub fn all_caught(&self) -> bool {
        let mut i = 0;
        while i < BAD_CLASSES {
            let (t, c) = (
                self.tries.get(i).copied().unwrap_or(0),
                self.caught.get(i).copied().unwrap_or(0),
            );
            if t == 0 || c != t {
                return false;
            }
            i += 1;
        }
        true
    }
    /// 漏检总数。
    pub fn missed(&self) -> u32 {
        let mut m: u32 = 0;
        let mut i = 0;
        while i < BAD_CLASSES {
            let t = self.tries.get(i).copied().unwrap_or(0);
            let c = self.caught.get(i).copied().unwrap_or(0);
            m = m.saturating_add(t.saturating_sub(c));
            i += 1;
        }
        m
    }
}

/// 单轮四类注入。
pub fn inject_once() -> [bool; BAD_CLASSES] {
    let mut hits = [false; BAD_CLASSES];
    // 类0：族号越界（12 号注册）。
    let mut r0 = ApiRegistry::new();
    if let Some(h) = hits.get_mut(BadClass::IdOverflowInject.slot()) {
        *h = r0.register(API_FAMILIES as u8, "Ghost", TIER_USER, 0b1) == Err(AeCode::FamilyOutOfRange.code());
    }
    // 类1：重号（0 号注册两次）。
    let mut r1 = ApiRegistry::new();
    let a = r1.register(0, "GetVersion", TIER_USER, 0b111111);
    let b = r1.register(0, "GetVersion", TIER_USER, 0b111111);
    if let Some(h) = hits.get_mut(BadClass::DupFamilyInject.slot()) {
        *h = a.is_ok() && b == Err(AeCode::FamilyDuplicated.code());
    }
    // 类2：空名。
    let mut r2 = ApiRegistry::new();
    if let Some(h) = hits.get_mut(BadClass::EmptyNameInject.slot()) {
        *h = r2.register(1, "", TIER_USER, 0b111111) == Err(AeCode::NameEmpty.code());
    }
    // 类3：位面杂质（bit6）+ 零位面 + 服务层越界。
    let mut r3 = ApiRegistry::new();
    if let Some(h) = hits.get_mut(BadClass::MaskDirtyInject.slot()) {
        *h = r3.register(2, "GetVersionExW", TIER_USER, 0b1000000) == Err(AeCode::DimMaskDirty.code()) && r3.register(2, "GetVersionExW", TIER_USER, 0) == Err(AeCode::DimMaskEmpty.code()) && r3.register(2, "GetVersionExW", 3, 0b111111) == Err(AeCode::TierOutOfRange.code());
    }
    hits
}

/// 注入 rounds 轮。
pub fn inject_rounds(rounds: u32) -> InjectLedger {
    let mut led = InjectLedger::new();
    let mut r = 0;
    while r < rounds {
        let hits = inject_once();
        let mut c = 0;
        while c < BAD_CLASSES {
            led.record(c, hits.get(c).copied().unwrap_or(false));
            c += 1;
        }
        r += 1;
    }
    led
}

// ── 黄金全链 ────────────────────────────────────────────────────────
/// 黄金全链：12 族全登记→册满→B01 双向勾稽 12/12→注入全抓。
pub fn golden_run() -> Result<(usize, usize), u32> {
    let mut reg = ApiRegistry::new();
    register_all(&mut reg)?;
    if reg.count() != API_FAMILIES {
        return Err(AeCode::RegistryIncomplete.code());
    }
    let (hl, hr) = reg.reconcile()?;
    if hl != API_FAMILIES || hr != API_FAMILIES {
        return Err(AeCode::ReconcileMissing.code());
    }
    let led = inject_rounds(JUDGE_ROUNDS);
    if !led.all_caught() || led.missed() != 0 {
        return Err(AeCode::ReconcileExtra.code());
    }
    Ok((hl, hr))
}

/// 双跑确定性。
pub fn repro_check() -> Result<(), u32> {
    let a = golden_run()?;
    let b = golden_run()?;
    if a != b {
        return Err(AeCode::NotReproducible.code());
    }
    Ok(())
}

// ── 探针三序 ────────────────────────────────────────────────────────
/// 探针A：单族登记后 find 命中且维度位面可查。
pub fn probe_a() -> Result<(), u32> {
    let mut reg = ApiRegistry::new();
    reg.register(3, "RtlGetVersion", TIER_USER, 0b001111)?;
    let f = match reg.find(3) {
        Some(f) => f,
        None => return Err(AeCode::RegistryIncomplete.code()),
    };
    if f.covers_dim(0) && f.covers_dim(3) && !f.covers_dim(5) {
        Ok(())
    } else {
        Err(AeCode::DimMaskDirty.code())
    }
}

/// 探针B：册空时勾稽 → ReconcileMissing（缺漏检出）。
pub fn probe_b() -> Result<(), u32> {
    let reg = ApiRegistry::new();
    match reg.reconcile() {
        Err(c) if c == AeCode::ReconcileMissing.code() => Ok(()),
        _ => Err(AeCode::ReconcileMissing.code()),
    }
}

/// 探针C：半册（只登 0 号）勾稽仍缺漏（11 缺）。
pub fn probe_c() -> Result<(), u32> {
    let mut reg = ApiRegistry::new();
    reg.register(0, "GetVersion", TIER_USER, 0b111111)?;
    match reg.reconcile() {
        Err(c) if c == AeCode::ReconcileMissing.code() => Ok(()),
        _ => Err(AeCode::ReconcileMissing.code()),
    }
}

// ── 段位守卫 ────────────────────────────────────────────────────────
/// 段位守卫：闭集每码落在 [SEG_BASE, SEG_BASE+0x40) 且段基 0x40 对齐。
pub fn segment_guard() -> bool {
    if SEG_BASE % 0x40 != 0 || !(0x7E500..=0x7E740).contains(&SEG_BASE) {
        return false;
    }
    let mut i = 0;
    while i < AeCode::ALL.len() {
        if let Some(c) = AeCode::ALL.get(i) {
            let v = c.code();
            if v < SEG_BASE || v >= SEG_BASE + 0x40 {
                return false;
            }
        }
        i += 1;
    }
    true
}

// ── 判据收口 ────────────────────────────────────────────────────────
/// run_unx18425_checks：J1 判据 15 条（断言性质，动态明细 ≤8）。
pub fn run_unx18425_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F18425 API 维度枚举（12 族全登记）");
    // J1-1 冻结 12 族：名/层/位面三数组等长且名非空。
    let frozen_ok = FAMILY_NAMES.len() == API_FAMILIES
        && FAMILY_TIERS.len() == API_FAMILIES
        && FAMILY_DIM_MASKS.len() == API_FAMILIES
        && FAMILY_NAMES.iter().all(|n| !n.is_empty());
    s.add("J1-12族闭集冻结", frozen_ok, "名/层/位面三数组 12 等长非空");
    // J1-2 黄金全链：12 族登记 + B01 勾稽 12/12。
    let g = golden_run();
    s.add_dyn(
        "J1-黄金全链12/12",
        matches!(&g, Ok((hl, hr)) if *hl == 12 && *hr == 12),
        &format!("{:?}", g),
    );
    // J1-3 B01 清单自检：12 号两两互异全落 0..11。
    let mut list_seen = [0u8; API_FAMILIES];
    let mut list_ok = true;
    let mut li = 0;
    while li < API_FAMILIES {
        let id = B01_ENTRY_LIST.get(li).copied().unwrap_or(0xFF) as usize;
        if id >= API_FAMILIES {
            list_ok = false;
        } else if let Some(c) = list_seen.get_mut(id) {
            *c = c.saturating_add(1);
        }
        li += 1;
    }
    let mut k = 0;
    while k < API_FAMILIES {
        if list_seen.get(k).copied().unwrap_or(0) != 1 {
            list_ok = false;
        }
        k += 1;
    }
    s.add("J1-B01清单自检", list_ok, "12 号互异且全落闭集");
    // J1-4 注入 10/10。
    let led = inject_rounds(JUDGE_ROUNDS);
    s.add_dyn(
        "J1-注入10/10",
        led.all_caught() && led.missed() == 0,
        &format!("t={:?} c={:?}", led.tries, led.caught),
    );
    // J1-5 注入守恒。
    let tot = led.tries.iter().copied().fold(0u32, |a, b| a.saturating_add(b));
    s.add(
        "J1-注入守恒",
        led.missed() == 0 && tot == JUDGE_ROUNDS * BAD_CLASSES as u32,
        "漏检0 总注入40",
    );
    // J1-6 族名全等闸：冒名登记必拒。
    s.add(
        "J1-族名全等闸",
        make_family(0, "FakeVersion", TIER_USER, 0b1).is_err()
            && make_family(0, "GetVersion", TIER_USER, 0b1).is_ok(),
        "冒名拒/正名过",
    );
    // J1-7 维度位面覆盖语义（冻结表直验）。
    // 魔数推导（与 FAMILY_DIM_MASKS 冻结口径逐位对齐）：
    //   0 号族 GetVersion 系核心四维 = 主/次/Build/SP = 0b001111；
    //   5 号族 VerifyVersionInfo 条件族窄维 = 主/次 = 0b000011；
    //  11 号族 WMI 查询唯一全维 = 0b111111。
    let cover_ok = {
        let mut ok = true;
        let mut i = 0;
        while i < API_FAMILIES {
            let mask = FAMILY_DIM_MASKS.get(i).copied().unwrap_or(0);
            if mask == 0 || (mask & !0b111111) != 0 {
                ok = false;
            }
            i += 1;
        }
        ok && FAMILY_DIM_MASKS.first().copied().unwrap_or(0) == 0b001111
            && FAMILY_DIM_MASKS.get(5).copied().unwrap_or(0) == 0b000011
            && FAMILY_DIM_MASKS.get(11).copied().unwrap_or(0) == 0b111111
    };
    s.add("J1-位面覆盖语义", cover_ok, "核心四维/条件窄维/WMI全维 冻结一致");
    // J1-8 covers_dim 边界：维度 6 越界返回不可查（不误报）。
    let cov_edge = {
        let mut reg = ApiRegistry::new();
        let _ = register_all(&mut reg);
        match reg.find(4) {
            Some(f) => f.covers_dim(QUERY_DIMS as u8) == false && f.covers_dim(4),
            None => false,
        }
    };
    s.add("J1-维度越界covers", cov_edge, "维度 6 越界必 false");
    // J1-9 册守恒：重号注入后 tried=accepted+rejected。
    let cons = {
        let mut r = ApiRegistry::new();
        let _ = r.register(0, "GetVersion", TIER_USER, 0b111111);
        let _ = r.register(0, "GetVersion", TIER_USER, 0b111111);
        r.conserve_ok() && r.rejected == 1
    };
    s.add("J1-册守恒", cons, "tried=accepted+rejected");
    // J1-10 探针三序。
    s.add("J1-探针三序", probe_a().is_ok() && probe_b().is_ok() && probe_c().is_ok(), "命中/缺漏/半册缺漏");
    // J1-11 双跑一致。
    s.add("J2-双跑一致", repro_check().is_ok(), "golden_run 两跑同值");
    // J1-12 服务层闭集：三层数分布（用户 7/外围 3/内核 2，冻结盘点）。
    let tier_count = {
        let mut cu = 0;
        let mut cp = 0;
        let mut ck = 0;
        let mut i = 0;
        while i < API_FAMILIES {
            match FAMILY_TIERS.get(i).copied().unwrap_or(9) {
                TIER_USER => cu += 1,
                TIER_PERIPH => cp += 1,
                TIER_KERNEL => ck += 1,
                _ => {}
            }
            i += 1;
        }
        cu == 7 && cp == 3 && ck == 2
    };
    s.add("J2-服务层分布", tier_count, "用户7 外围3 内核2 冻结盘点");
    // J1-13 文案闭集：12 码逐码非空。
    let docs_ok = {
        let mut ok = true;
        let mut i = 0;
        while i < AeCode::ALL.len() {
            if let Some(c) = AeCode::ALL.get(i) {
                if c.detail().is_empty() {
                    ok = false;
                }
            }
            i += 1;
        }
        ok
    };
    s.add("J2-文案闭集", docs_ok, "12 码中文文案逐码非空");
    // J1-14 拒因留账：越界注入留 FamilyOutOfRange 码。
    let code_left = {
        let mut r = ApiRegistry::new();
        let _ = r.register(API_FAMILIES as u8, "X", TIER_USER, 0b1);
        r.reject_codes.first().copied().unwrap_or(0)
            == AeCode::FamilyOutOfRange.code()
    };
    s.add("J2-拒因留账", code_left, "越界拒因入账可复算");
    // J1-15 段位守卫。
    s.add("J3-段位守卫", segment_guard(), "0x7E600 段 0x40 窗口");
    s
}

// ── 测试（≥8 条，测试区允许 assert） ───────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn t_all_checks_green() {
        let set = run_unx18425_checks();
        assert!(set.all_passed(), "UNX-F18425 判据有红项：{}", set.red_items_text());
    }

    #[test]
    fn t_register_all_and_reconcile() {
        let mut reg = ApiRegistry::new();
        assert!(register_all(&mut reg).is_ok());
        assert_eq!(reg.count(), 12);
        let (hl, hr) = reg.reconcile().unwrap_or((0, 0));
        assert_eq!((hl, hr), (12, 12));
    }

    #[test]
    fn t_family_gates() {
        assert_eq!(
            make_family(12, "Ghost", TIER_USER, 0b1),
            Err(AeCode::FamilyOutOfRange.code())
        );
        assert_eq!(
            make_family(0, "Fake", TIER_USER, 0b1),
            Err(AeCode::NameMismatch.code())
        );
        assert_eq!(make_family(0, "", TIER_USER, 0b1), Err(AeCode::NameEmpty.code()));
        assert_eq!(
            make_family(0, "GetVersion", 3, 0b1),
            Err(AeCode::TierOutOfRange.code())
        );
        assert_eq!(
            make_family(0, "GetVersion", TIER_USER, 0),
            Err(AeCode::DimMaskEmpty.code())
        );
        assert_eq!(
            make_family(0, "GetVersion", TIER_USER, 0b1000000),
            Err(AeCode::DimMaskDirty.code())
        );
    }

    #[test]
    fn t_dup_register_rejected() {
        let mut reg = ApiRegistry::new();
        assert!(reg.register(1, "GetVersionExA", TIER_USER, 0b111111).is_ok());
        assert_eq!(
            reg.register(1, "GetVersionExA", TIER_USER, 0b111111),
            Err(AeCode::FamilyDuplicated.code())
        );
        assert!(reg.conserve_ok());
    }

    #[test]
    fn t_reconcile_missing_on_empty() {
        let reg = ApiRegistry::new();
        assert_eq!(reg.reconcile(), Err(AeCode::ReconcileMissing.code()));
    }

    #[test]
    fn t_dim_coverage() {
        let mut reg = ApiRegistry::new();
        let _ = register_all(&mut reg);
        let wmi = reg.find(11).unwrap_or(ApiFamily {
            family_id: 11,
            name: String::new(),
            tier: 0,
            dim_mask: 0,
        });
        assert!(wmi.covers_dim(5));
        let cond = reg.find(5).unwrap_or_else(|| wmi.clone());
        assert!(!cond.covers_dim(4));
        assert!(!cond.covers_dim(6));
    }

    #[test]
    fn t_inject_ledger() {
        let led = inject_rounds(JUDGE_ROUNDS);
        assert!(led.all_caught());
        assert_eq!(led.missed(), 0);
        for i in 0..BAD_CLASSES {
            assert_eq!(led.tries[i], JUDGE_ROUNDS);
        }
    }

    #[test]
    fn t_repro_and_segment() {
        assert!(repro_check().is_ok());
        assert!(segment_guard());
    }

    #[test]
    fn t_probes() {
        assert!(probe_a().is_ok());
        assert!(probe_b().is_ok());
        assert!(probe_c().is_ok());
    }
}
