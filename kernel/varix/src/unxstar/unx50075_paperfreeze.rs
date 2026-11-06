//! UNX-F50075 纸张冻结接口（管线/UI 消费单点面）（0x43580 段）
//!
//! 锚点：UNX-F50075-J1 接口冻结登记；判据锚定 §AI-63 §4。
//!
//! 数据段：`SEG_BASE = 0x43580`，诊断码闭集 `SEG_BASE + 1 ..= SEG_BASE + 13`，
//! 逐项见 [`FreezeError`]。
//!
//! 为什么纸张域需要「冻结接口」：走纸控制同时被三条链路消费——打印管线
//! （按页推进）、面板 UI（显示盒/纸状态）、驱动（查询幅面）。三边各调各的
//! 私有函数时，任何一次重构都会把某一侧悄悄改坏，且只有现场才会发现。
//! 本模块把消费面收敛成**每个关注点恰有一个冻结符号**（single point），
//! 再对整张表算一个清单摘要：摘要是冻结契约的身份，改动登记表即等于
//! 换了一版接口，必须显式走新版本流程。
//!
//! 状态机：`Draft → Frozen`。Draft 允许登记/更新；Frozen 后任何写入都被
//! [`FreezeError::Sealed`] 挡住——冻结不是「建议」，是可判定的状态。
//!
//! 结构清单：
//! * [`Concern`] + [`Stability`] + [`ApiSymbol`]：接口符号与签名哈希；
//! * [`FreezeRegistry`] + [`FreezeState`]：单点登记 + 状态机；
//! * [`Manifest`]：冻结清单（摘要 / 符号数 / 覆盖关注点数）；
//! * [`baseline_symbols`] + [`baseline_manifest`]：冻结基线（唯一事实源）；
//! * [`injection_core`]：6 类坏样本注入核。

use crate::alloc::format;
use crate::alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x43580 独占 0x20）。
pub const SEG_BASE: u32 = 0x43580;

const E_EMPTY: u16 = 1;
const E_BAD_ID: u16 = 2;
const E_BAD_NAME: u16 = 3;
const E_BAD_ARITY: u16 = 4;
const E_VERSION: u16 = 5;
const E_SIG_DRIFT: u16 = 6;
const E_DUPLICATE_ID: u16 = 7;
const E_CONCERN_DUP: u16 = 8;
const E_FULL: u16 = 9;
const E_SEALED: u16 = 10;
const E_NOT_SEALED: u16 = 11;
const E_CONCERN_GAP: u16 = 12;
const E_MANIFEST: u16 = 13;

/// 冻结面错误。`code()` 返回 `SEG_BASE + n`。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FreezeError {
    /// 空符号表。
    Empty,
    /// 符号号 0 或超过 64。
    BadId,
    /// 符号名不是 `paper.` 前缀的 ASCII 标识。
    BadName,
    /// 形参个数越出 1..=8。
    BadArity,
    /// 版本号回退（0 不是合法版本）。
    VersionRegress,
    /// 签名哈希与重算不符。
    SigDrift,
    /// 符号号重复。
    DuplicateId,
    /// 同一关注点出现第二个冻结符号（破坏单点）。
    ConcernDup,
    /// 登记表已满。
    Full,
    /// 已冻结，写入被拒。
    Sealed,
    /// 未冻结就要求清单。
    NotSealed,
    /// 关注点覆盖不全（每点必须恰有一个冻结符号）。
    ConcernGap,
    /// 清单摘要与重算不符。
    ManifestMismatch,
}

impl FreezeError {
    /// 诊断码：`SEG_BASE + n`。
    pub fn code(self) -> u32 {
        let n = match self {
            FreezeError::Empty => E_EMPTY,
            FreezeError::BadId => E_BAD_ID,
            FreezeError::BadName => E_BAD_NAME,
            FreezeError::BadArity => E_BAD_ARITY,
            FreezeError::VersionRegress => E_VERSION,
            FreezeError::SigDrift => E_SIG_DRIFT,
            FreezeError::DuplicateId => E_DUPLICATE_ID,
            FreezeError::ConcernDup => E_CONCERN_DUP,
            FreezeError::Full => E_FULL,
            FreezeError::Sealed => E_SEALED,
            FreezeError::NotSealed => E_NOT_SEALED,
            FreezeError::ConcernGap => E_CONCERN_GAP,
            FreezeError::ManifestMismatch => E_MANIFEST,
        };
        SEG_BASE + n as u32
    }

    /// 一行英文诊断。
    pub fn message(self) -> &'static str {
        match self {
            FreezeError::Empty => "empty api surface",
            FreezeError::BadId => "api symbol id outside 1..=64",
            FreezeError::BadName => "api symbol name not a paper.* identifier",
            FreezeError::BadArity => "api arity outside 1..=8",
            FreezeError::VersionRegress => "api version regressed to zero",
            FreezeError::SigDrift => "signature hash drifted from recompute",
            FreezeError::DuplicateId => "api symbol id registered twice",
            FreezeError::ConcernDup => "second frozen symbol on one concern",
            FreezeError::Full => "freeze registry full",
            FreezeError::Sealed => "api surface is frozen",
            FreezeError::NotSealed => "api surface not frozen yet",
            FreezeError::ConcernGap => "concern without exactly one frozen symbol",
            FreezeError::ManifestMismatch => "manifest digest does not match surface",
        }
    }
}

/// 消费关注点。每个点必须恰有一个冻结符号——这就是「单点面」的定义：
/// 管线拿 Feed 的那个符号，UI 也拿同一个，不可能出现两份实现。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Concern {
    /// 进纸。
    Feed,
    /// 按页推进。
    Advance,
    /// 排纸。
    Eject,
    /// 幅面查询。
    SizeQuery,
    /// 盒选择。
    TraySelect,
    /// 介质状态上报。
    MediaReport,
}

impl Concern {
    /// 全部六个关注点（顺序即清单顺序）。
    pub const ALL: [Concern; 6] = [
        Concern::Feed,
        Concern::Advance,
        Concern::Eject,
        Concern::SizeQuery,
        Concern::TraySelect,
        Concern::MediaReport,
    ];

    /// 关注点码 0..=5。
    pub fn code(self) -> u8 {
        match self {
            Concern::Feed => 0,
            Concern::Advance => 1,
            Concern::Eject => 2,
            Concern::SizeQuery => 3,
            Concern::TraySelect => 4,
            Concern::MediaReport => 5,
        }
    }

    /// 码反解；闭集外返回 [`FreezeError::ConcernGap`]。
    pub fn from_code(c: u8) -> Result<Concern, FreezeError> {
        match c {
            0 => Ok(Concern::Feed),
            1 => Ok(Concern::Advance),
            2 => Ok(Concern::Eject),
            3 => Ok(Concern::SizeQuery),
            4 => Ok(Concern::TraySelect),
            5 => Ok(Concern::MediaReport),
            _ => Err(FreezeError::ConcernGap),
        }
    }

    /// 关注点名。
    pub fn label(self) -> &'static str {
        match self {
            Concern::Feed => "feed",
            Concern::Advance => "advance",
            Concern::Eject => "eject",
            Concern::SizeQuery => "size",
            Concern::TraySelect => "tray",
            Concern::MediaReport => "media",
        }
    }
}

/// 稳定性档。只有 Frozen 占单点位；Deprecated 是迁移期旧符号、Reserved
/// 是预留给下一版的位置，两者都不占点，因此可以共存。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Stability {
    /// 已冻结（唯一可占单点位的档）。
    Frozen,
    /// 已弃用（迁移期共存）。
    Deprecated,
    /// 预留。
    Reserved,
}

impl Stability {
    /// 稳定性码 0..=2。
    pub fn code(self) -> u8 {
        match self {
            Stability::Frozen => 0,
            Stability::Deprecated => 1,
            Stability::Reserved => 2,
        }
    }

    /// 码反解；闭集外返回 [`FreezeError::SigDrift`]（档位不是签名的一部分，
    /// 但越界档说明符号表被写脏，与签名漂移同属「登记内容不可信」）。
    pub fn from_code(c: u8) -> Result<Stability, FreezeError> {
        match c {
            0 => Ok(Stability::Frozen),
            1 => Ok(Stability::Deprecated),
            2 => Ok(Stability::Reserved),
            _ => Err(FreezeError::SigDrift),
        }
    }

    /// 档名。
    pub fn label(self) -> &'static str {
        match self {
            Stability::Frozen => "frozen",
            Stability::Deprecated => "deprecated",
            Stability::Reserved => "reserved",
        }
    }
}

/// 接口符号。`sig` 是签名哈希（名字 / 版本 / 形参数 / 关注点四元组），
/// 只存哈希不存参数字符串——内核里符号表必须是定长可复制的。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ApiSymbol {
    pub id: u16,
    pub name: &'static str,
    pub version: u16,
    pub arity: u8,
    pub concern: Concern,
    pub stability: Stability,
    pub sig: u32,
}

const FNV_OFFSET: u32 = 0x811c_9dc5;
const FNV_PRIME: u32 = 0x0100_0193;

fn fnv_byte(h: u32, b: u8) -> u32 {
    (h ^ b as u32).wrapping_mul(FNV_PRIME)
}

fn fnv_u16(h: u32, v: u16) -> u32 {
    fnv_byte(fnv_byte(h, (v >> 8) as u8), v as u8)
}

fn fnv_str(h: u32, s: &str) -> u32 {
    let bs = s.as_bytes();
    let mut h = h;
    let mut i = 0usize;
    while i < bs.len() {
        if let Some(b) = bs.get(i) {
            h = fnv_byte(h, *b);
        }
        i += 1;
    }
    h
}

/// 计算签名哈希（登记与校验共用同一函数，杜绝两套算法）。
pub fn compute_sig(name: &str, version: u16, arity: u8, concern: Concern) -> u32 {
    let mut h = FNV_OFFSET;
    h = fnv_str(h, name);
    h = fnv_u16(h, version);
    h = fnv_byte(h, arity);
    fnv_byte(h, concern.code())
}

/// 符号号上限（1..=64；上限定死是为了让清单摘要的遍历域有限）。
pub const MAX_ID: u16 = 64;

/// 已发布 API 代次上限。基线符号的版本号必须落在 1..=3：
/// 3 是当前代次，回退到 0 或超前编造都属版本漂移。
pub const MAX_API_VERSION: u16 = 3;

impl ApiSymbol {
    /// 由四元组构造符号（自动签名）。
    pub fn new(
        id: u16,
        name: &'static str,
        version: u16,
        arity: u8,
        concern: Concern,
        stability: Stability,
    ) -> ApiSymbol {
        ApiSymbol {
            id,
            name,
            version,
            arity,
            concern,
            stability,
            sig: compute_sig(name, version, arity, concern),
        }
    }

    /// 符号自检：号域 / 名 / 形参数 / 版本 / 签名。
    pub fn validate(&self) -> Result<(), FreezeError> {
        if self.id == 0 || self.id > MAX_ID {
            return Err(FreezeError::BadId);
        }
        if !self.name.starts_with("paper.") || self.name.len() < 7 {
            return Err(FreezeError::BadName);
        }
        if self.arity == 0 || self.arity > 8 {
            return Err(FreezeError::BadArity);
        }
        if self.version == 0 {
            return Err(FreezeError::VersionRegress);
        }
        if self.sig != compute_sig(self.name, self.version, self.arity, self.concern) {
            return Err(FreezeError::SigDrift);
        }
        Ok(())
    }

    /// 本符号是否占单点位。
    pub fn occupies_point(&self) -> bool {
        self.stability == Stability::Frozen
    }

    /// 一行可读符号。
    pub fn line(&self) -> String {
        format!(
            "#{} {} v{} a{} {} {} {:08x}",
            self.id,
            self.name,
            self.version,
            self.arity,
            self.concern.label(),
            self.stability.label(),
            self.sig
        )
    }
}

/// 冻结状态机。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FreezeState {
    /// 草稿（可登记）。
    Draft,
    /// 已冻结（只读）。
    Frozen,
}

/// 冻结清单：冻结契约的身份。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Manifest {
    pub digest: u32,
    pub symbols: u16,
    pub concerns: u8,
    pub version: u16,
}

impl Manifest {
    /// 自检：关注点必须覆盖全部六个（少一个就不是完整清单）。
    pub fn validate(&self) -> Result<(), FreezeError> {
        if self.concerns != Concern::ALL.len() as u8 {
            return Err(FreezeError::ConcernGap);
        }
        if self.symbols == 0 {
            return Err(FreezeError::Empty);
        }
        Ok(())
    }

    /// 一行可读清单。
    pub fn line(&self) -> String {
        format!(
            "digest={:08x} sym={} concern={} v{}",
            self.digest, self.symbols, self.concerns, self.version
        )
    }
}

/// 单点冻结登记表：`Draft → Frozen`，Frozen 后写入一律被拒。
#[derive(Clone, Debug)]
pub struct FreezeRegistry {
    entries: Vec<ApiSymbol>,
    cap: usize,
    state: FreezeState,
    version: u16,
}

impl FreezeRegistry {
    /// 建空表。
    pub fn new(cap: usize) -> FreezeRegistry {
        FreezeRegistry {
            entries: Vec::with_capacity(cap),
            cap,
            state: FreezeState::Draft,
            version: 1,
        }
    }

    /// 由符号表批量登记；空表返回 [`FreezeError::Empty`]。
    pub fn from_symbols(symbols: &[ApiSymbol]) -> Result<FreezeRegistry, FreezeError> {
        if symbols.is_empty() {
            return Err(FreezeError::Empty);
        }
        let mut r = FreezeRegistry::new(symbols.len());
        let mut i = 0usize;
        while i < symbols.len() {
            if let Some(sym) = symbols.get(i) {
                r.register(*sym)?;
            }
            i += 1;
        }
        Ok(r)
    }

    /// 登记一个符号：封账 → 定容 → 自检 → 号唯一 → 单点唯一。
    ///
    /// 单点闸只看 `Frozen` 档：同一关注点已有冻结符号时，第二个冻结符号
    /// 必须被拒；Deprecated / Reserved 可与冻结符号共存（迁移与预留）。
    pub fn register(&mut self, sym: ApiSymbol) -> Result<(), FreezeError> {
        if self.state == FreezeState::Frozen {
            return Err(FreezeError::Sealed);
        }
        if self.entries.len() >= self.cap {
            return Err(FreezeError::Full);
        }
        sym.validate()?;
        if self.by_id(sym.id).is_some() {
            return Err(FreezeError::DuplicateId);
        }
        if sym.occupies_point() && self.frozen_on(sym.concern).is_some() {
            return Err(FreezeError::ConcernDup);
        }
        self.entries.push(sym);
        Ok(())
    }

    /// 按号查符号。
    pub fn by_id(&self, id: u16) -> Option<&ApiSymbol> {
        let mut i = 0usize;
        while i < self.entries.len() {
            if let Some(s) = self.entries.get(i) {
                if s.id == id {
                    return Some(s);
                }
            }
            i += 1;
        }
        None
    }

    /// 该关注点上的冻结符号。
    pub fn frozen_on(&self, c: Concern) -> Option<&ApiSymbol> {
        let mut i = 0usize;
        while i < self.entries.len() {
            if let Some(s) = self.entries.get(i) {
                if s.concern == c && s.occupies_point() {
                    return Some(s);
                }
            }
            i += 1;
        }
        None
    }

    /// 单点覆盖数（每点最多算一次，故上限 6）。
    pub fn coverage(&self) -> u8 {
        let mut n = 0u8;
        let mut i = 0usize;
        while i < Concern::ALL.len() {
            if let Some(c) = Concern::ALL.get(i) {
                if self.frozen_on(*c).is_some() {
                    n = n.saturating_add(1);
                }
            }
            i += 1;
        }
        n
    }

    /// 清单摘要：按号升序折叠，故与登记顺序无关（集合语义）。
    ///
    /// 遍历域固定为 1..=MAX_ID，而不按插入序——插入序是**操作史**，
    /// 不是**接口身份**；同一套接口换个登记顺序不应换摘要。
    pub fn digest(&self) -> u32 {
        let mut h = FNV_OFFSET;
        let mut id: u16 = 1;
        while id <= MAX_ID {
            if let Some(s) = self.by_id(id) {
                h = fnv_u16(h, s.id);
                h = fnv_str(h, s.name);
                h = fnv_u16(h, s.version);
                h = fnv_byte(h, s.arity);
                h = fnv_byte(h, s.concern.code());
                h = fnv_byte(h, s.stability.code());
                h = fnv_byte(h, (s.sig >> 24) as u8);
                h = fnv_byte(h, (s.sig >> 16) as u8);
                h = fnv_byte(h, (s.sig >> 8) as u8);
                h = fnv_byte(h, s.sig as u8);
            }
            id = id.saturating_add(1);
        }
        h
    }

    /// 冻结：覆盖不足直接拒；冻结后出清单（幂等，重复冻结返回同一摘要）。
    pub fn freeze(&mut self) -> Result<Manifest, FreezeError> {
        let cov = self.coverage();
        if cov != Concern::ALL.len() as u8 {
            return Err(FreezeError::ConcernGap);
        }
        let m = Manifest {
            digest: self.digest(),
            symbols: self.entries.len() as u16,
            concerns: cov,
            version: self.version,
        };
        m.validate()?;
        self.state = FreezeState::Frozen;
        Ok(m)
    }

    /// 校验外部清单与当前登记表一致（供加载方比对基线）。
    pub fn verify(&self, m: &Manifest) -> Result<u8, FreezeError> {
        if self.state != FreezeState::Frozen {
            return Err(FreezeError::NotSealed);
        }
        m.validate()?;
        if m.digest != self.digest() || m.symbols != self.entries.len() as u16 {
            return Err(FreezeError::ManifestMismatch);
        }
        if m.concerns != self.coverage() {
            return Err(FreezeError::ConcernGap);
        }
        Ok(self.coverage())
    }

    /// 当前状态。
    pub fn state(&self) -> FreezeState {
        self.state
    }

    /// 是否已冻结。
    pub fn is_frozen(&self) -> bool {
        self.state == FreezeState::Frozen
    }

    /// 条数。
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// 容量。
    pub fn capacity(&self) -> usize {
        self.cap
    }
}

/// 冻结基线符号表（唯一事实源：管线/UI/驱动三方都按这张表比对）。
///
/// 六个关注点各一个 Frozen 符号；版本号 2/3 是「已发布过一轮」的事实，
/// 不是随手写的数——判据要求版本单调且非零。
pub fn baseline_symbols() -> Vec<ApiSymbol> {
    let mut v = Vec::with_capacity(6);
    v.push(ApiSymbol::new(1, "paper.feed", 3, 2, Concern::Feed, Stability::Frozen));
    v.push(ApiSymbol::new(
        2,
        "paper.advance",
        3,
        1,
        Concern::Advance,
        Stability::Frozen,
    ));
    v.push(ApiSymbol::new(3, "paper.eject", 3, 1, Concern::Eject, Stability::Frozen));
    v.push(ApiSymbol::new(
        4,
        "paper.size",
        2,
        1,
        Concern::SizeQuery,
        Stability::Frozen,
    ));
    v.push(ApiSymbol::new(
        5,
        "paper.tray",
        2,
        2,
        Concern::TraySelect,
        Stability::Frozen,
    ));
    v.push(ApiSymbol::new(
        6,
        "paper.media",
        2,
        1,
        Concern::MediaReport,
        Stability::Frozen,
    ));
    v
}

/// 公开签名哈希表（清单里每个符号的签名字面量），供离线核对：
/// 与 [`baseline_symbols`] 的自动签名必须逐项相等。
pub const BASELINE_SIGS: [u32; 6] = [
    0x38c5_a8d6,
    0x85f4_1e72,
    0xea3d_acec,
    0x5a18_ea1e,
    0xaef1_ba99,
    0x1561_e58f,
];

/// 冻结基线清单摘要（由 [`baseline_symbols`] 离线算出后钉死）。
pub const FROZEN_MANIFEST: u32 = 0x1c82_2a5f;

/// 构建基线登记表（未冻结）。
pub fn baseline_registry() -> Result<FreezeRegistry, FreezeError> {
    FreezeRegistry::from_symbols(&baseline_symbols())
}

/// 构建并冻结基线，返回清单。
pub fn baseline_manifest() -> Result<Manifest, FreezeError> {
    let mut r = baseline_registry()?;
    r.freeze()
}

// ---------------------------------------------------------------------------
// 注入核：6 类坏样本必须全部被抓
// ---------------------------------------------------------------------------

/// 6 类坏样本注入核，返回 (抓住, 总数)。
pub fn injection_core() -> (usize, usize) {
    let mut caught = 0usize;
    let total = 6usize;

    // 1) 空符号表。
    if FreezeRegistry::from_symbols(&[]).err() == Some(FreezeError::Empty) {
        caught += 1;
    }

    // 2) 符号号 0。
    let zero = ApiSymbol::new(0, "paper.zero", 1, 1, Concern::Feed, Stability::Frozen);
    if zero.validate().err() == Some(FreezeError::BadId) {
        caught += 1;
    }

    // 3) 形参数 0（无参符号无法承载页推进语义）。
    let flat = ApiSymbol::new(9, "paper.flat", 1, 0, Concern::Feed, Stability::Frozen);
    if flat.validate().err() == Some(FreezeError::BadArity) {
        caught += 1;
    }

    // 4) 版本回退到 0。
    let old = ApiSymbol::new(9, "paper.old", 0, 1, Concern::Feed, Stability::Frozen);
    if old.validate().err() == Some(FreezeError::VersionRegress) {
        caught += 1;
    }

    // 5) 签名被篡改（其余字段都对）。
    let mut fake = ApiSymbol::new(9, "paper.fake", 1, 1, Concern::Feed, Stability::Frozen);
    fake.sig ^= 0x0001_0000;
    if fake.validate().err() == Some(FreezeError::SigDrift) {
        caught += 1;
    }

    // 6) 单点被破坏：同一关注点第二个冻结符号。
    let mut reg = FreezeRegistry::new(4);
    let ok0 = reg
        .register(ApiSymbol::new(1, "paper.feed", 2, 2, Concern::Feed, Stability::Frozen))
        .is_ok();
    let dup = ApiSymbol::new(2, "paper.feed2", 2, 2, Concern::Feed, Stability::Frozen);
    if ok0 && reg.register(dup).err() == Some(FreezeError::ConcernDup) {
        caught += 1;
    }

    (caught, total)
}

// ---------------------------------------------------------------------------
// 判据
// ---------------------------------------------------------------------------

/// UNX-F50075 自检：17 条判据。
pub fn run_unx50075_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F50075");

    // J1-1 登记规模与单点覆盖：6 个符号、6 个关注点各恰一个冻结符号。
    let reg = baseline_registry();
    let mut cov = 0u8;
    let mut count = 0usize;
    let mut shape_ok = false;
    if let Ok(r) = &reg {
        count = r.len();
        cov = r.coverage();
        shape_ok = count == 6 && cov == 6;
        let mut i = 0usize;
        while i < Concern::ALL.len() {
            if let Some(c) = Concern::ALL.get(i) {
                match r.frozen_on(*c) {
                    Some(sym) => {
                        if sym.concern != *c {
                            shape_ok = false;
                        }
                    }
                    None => shape_ok = false,
                }
            }
            i += 1;
        }
    }
    let det = format!("sym={} cov={}", count, cov);
    s.add_dyn("50075-J1-单点登记", shape_ok, &det);

    // J1-2 基线签名表：6 个自动签名与钉死字面量逐项相等。
    let ok = match &reg {
        Ok(r) => {
            let mut good = true;
            let mut i = 0usize;
            while i < BASELINE_SIGS.len() {
                let id = (i + 1) as u16;
                match (r.by_id(id), BASELINE_SIGS.get(i)) {
                    (Some(sym), Some(want)) => {
                        if sym.sig != *want {
                            good = false;
                        }
                    }
                    _ => good = false,
                }
                i += 1;
            }
            good
        }
        Err(_) => false,
    };
    s.add("50075-J1-签名表一致", ok, if ok { "" } else { "签名与字面量不符" });

    // J1-3 冻结摘要：登记表摘要 == 钉死常量（接口身份零漂移）。
    let manifest = baseline_manifest();
    let digest_ok = match (&reg, &manifest) {
        (Ok(r), Ok(m)) => r.digest() == m.digest && m.digest == FROZEN_MANIFEST,
        _ => false,
    };
    let det = match &manifest {
        Ok(m) => m.line(),
        Err(_) => String::from("no manifest"),
    };
    s.add_dyn("50075-J1-冻结摘要", digest_ok, &det);

    // J1-4 冻结幂等：两次冻结出同一清单（摘要/符号数/版本）。
    let d1 = match &manifest {
        Ok(m) => *m,
        Err(_) => Manifest {
            digest: 0,
            symbols: 0,
            concerns: 0,
            version: 0,
        },
    };
    let mut r2 = match baseline_registry() {
        Ok(v) => v,
        Err(_) => FreezeRegistry::new(1),
    };
    let first = r2.freeze();
    let second = r2.freeze();
    let ok = match (&first, &second) {
        (Ok(a), Ok(b)) => a == b && a.digest == d1.digest && r2.is_frozen(),
        _ => false,
    };
    s.add("50075-J1-冻结幂等", ok, if ok { "" } else { "两次冻结不一致" });

    // J1-5 清单校验：基线清单通过 verify；摘要被改一位必须 ManifestMismatch。
    let ok = match (&manifest, &r2) {
        (Ok(m), reg2) => {
            let good = reg2.verify(m).ok() == Some(6);
            let mut bad = *m;
            bad.digest ^= 0x0000_0100;
            good && reg2.verify(&bad).err() == Some(FreezeError::ManifestMismatch)
        }
        _ => false,
    };
    s.add("50075-J1-清单校验", ok, if ok { "" } else { "清单比对失效" });

    // J1-6 未冻结不得出清单：Draft 状态 verify 必 NotSealed，freeze 前 state 为 Draft。
    let draft = baseline_registry();
    let ok = match &draft {
        Ok(r) => {
            r.state() == FreezeState::Draft
                && !r.is_frozen()
                && r.verify(&d1).err() == Some(FreezeError::NotSealed)
        }
        Err(_) => false,
    };
    s.add("50075-J1-未冻必拒", ok, if ok { "" } else { "Draft 可验清单" });

    // J1-7 冻结后写入必拒：Sealed，且条数不变。
    let mut locked = match baseline_registry() {
        Ok(v) => v,
        Err(_) => FreezeRegistry::new(1),
    };
    let before = locked.len();
    let froze = locked.freeze().is_ok();
    let extra = ApiSymbol::new(7, "paper.extra", 1, 1, Concern::Feed, Stability::Deprecated);
    let ok = froze
        && locked.register(extra).err() == Some(FreezeError::Sealed)
        && locked.len() == before;
    s.add("50075-J1-冻结只读", ok, if ok { "" } else { "冻结后可写" });

    // J1-8 号重复必拒：同号第二次 DuplicateId，表不增长。
    let mut dup = FreezeRegistry::new(4);
    let a = ApiSymbol::new(1, "paper.feed", 2, 2, Concern::Feed, Stability::Frozen);
    let b = ApiSymbol::new(1, "paper.feed-b", 2, 2, Concern::Advance, Stability::Reserved);
    let ok = dup.register(a).is_ok()
        && dup.register(b).err() == Some(FreezeError::DuplicateId)
        && dup.len() == 1;
    s.add("50075-J1-重号必拒", ok, if ok { "" } else { "同号进了表" });

    // J1-9 单点必守：同关注点第二个冻结符号必拒；Deprecated 可共存且不占点。
    let mut sp = FreezeRegistry::new(4);
    let f1 = ApiSymbol::new(1, "paper.feed", 2, 2, Concern::Feed, Stability::Frozen);
    let f2 = ApiSymbol::new(2, "paper.feed-new", 3, 2, Concern::Feed, Stability::Frozen);
    let old = ApiSymbol::new(3, "paper.feed-old", 1, 2, Concern::Feed, Stability::Deprecated);
    let res = ApiSymbol::new(4, "paper.feed-next", 4, 2, Concern::Feed, Stability::Reserved);
    let first_ok = sp.register(f1).is_ok();
    let second_blocked = sp.register(f2).err() == Some(FreezeError::ConcernDup);
    let old_ok = sp.register(old).is_ok();
    let reserved_ok = sp.register(res).is_ok();
    // 单点归属必须在**登记之后**查：登记前该点为空，查出来必然是 None。
    let still_first = match sp.frozen_on(Concern::Feed) {
        Some(sym) => sym.id == 1,
        None => false,
    };
    let ok = first_ok
        && second_blocked
        && old_ok
        && reserved_ok
        && sp.len() == 3
        && sp.coverage() == 1
        && still_first;
    s.add("50075-J1-单点必守", ok, if ok { "" } else { "单点被第二条冻结符号破坏" });

    // J1-10 覆盖缺口：只登记 5 点，freeze 必 ConcernGap；Manifest 自检同样拒。
    let mut gap = FreezeRegistry::new(8);
    let mut i10 = 0usize;
    while i10 < 5 {
        if let Some(c) = Concern::ALL.get(i10) {
            let name = match i10 {
                0 => "paper.feed",
                1 => "paper.advance",
                2 => "paper.eject",
                3 => "paper.size",
                _ => "paper.tray",
            };
            let _ = gap.register(ApiSymbol::new(
                (i10 + 1) as u16,
                name,
                2,
                1,
                *c,
                Stability::Frozen,
            ));
        }
        i10 += 1;
    }
    let short_manifest = Manifest {
        digest: 0,
        symbols: 5,
        concerns: 5,
        version: 1,
    };
    let ok = gap.len() == 5
        && gap.coverage() == 5
        && gap.freeze().err() == Some(FreezeError::ConcernGap)
        && short_manifest.validate().err() == Some(FreezeError::ConcernGap);
    s.add("50075-J1-覆盖缺口", ok, if ok { "" } else { "缺口被冻结" });

    // J1-11 定容：cap=1 第二条 Full。
    let mut tiny = FreezeRegistry::new(1);
    let ok = tiny
        .register(ApiSymbol::new(1, "paper.feed", 1, 1, Concern::Feed, Stability::Frozen))
        .is_ok()
        && tiny
            .register(ApiSymbol::new(
                2,
                "paper.eject",
                1,
                1,
                Concern::Eject,
                Stability::Frozen,
            ))
            .err()
            == Some(FreezeError::Full)
        && tiny.len() == 1;
    s.add("50075-J1-定容必拒", ok, if ok { "" } else { "超容被接受" });

    // J1-12 坏名/坏档/坏签名：三条闭集错误各自命中。
    let bad_name = ApiSymbol::new(9, "feed", 1, 1, Concern::Feed, Stability::Frozen);
    let bad_arity = ApiSymbol::new(9, "paper.x", 1, 9, Concern::Feed, Stability::Frozen);
    let ok = bad_name.validate().err() == Some(FreezeError::BadName)
        && bad_arity.validate().err() == Some(FreezeError::BadArity)
        && Stability::from_code(3).err() == Some(FreezeError::SigDrift)
        && Concern::from_code(6).err() == Some(FreezeError::ConcernGap);
    s.add("50075-J1-坏档必拒", ok, if ok { "" } else { "越界档被接受" });

    // J1-13 摘要与登记顺序无关：倒序登记同一集合，摘要必须相同。
    let mut fwd = FreezeRegistry::new(8);
    let mut rev = FreezeRegistry::new(8);
    let syms = baseline_symbols();
    let mut i13 = 0usize;
    while i13 < syms.len() {
        if let Some(sym) = syms.get(i13) {
            let _ = fwd.register(*sym);
        }
        i13 += 1;
    }
    let mut j13 = syms.len();
    while j13 > 0 {
        j13 -= 1;
        if let Some(sym) = syms.get(j13) {
            let _ = rev.register(*sym);
        }
    }
    let ok = fwd.len() == 6 && rev.len() == 6 && fwd.digest() == rev.digest();
    let det = format!("{:#010x}", fwd.digest());
    s.add_dyn("50075-J1-摘要集合化", ok, &det);

    // J1-14 摘要灵敏度：改一个符号版本，摘要必变（防常量 stub）。
    let mut bump = FreezeRegistry::new(8);
    let mut i14 = 0usize;
    while i14 < syms.len() {
        if let Some(sym) = syms.get(i14) {
            let mut c = *sym;
            if c.id == 4 {
                c.version = c.version.saturating_add(1);
                c.sig = compute_sig(c.name, c.version, c.arity, c.concern);
            }
            let _ = bump.register(c);
        }
        i14 += 1;
    }
    let ok = bump.len() == 6 && bump.digest() != fwd.digest();
    s.add("50075-J1-摘要敏感", ok, if ok { "" } else { "改版本摘要未变" });

    // J1-15 版本单调：每个符号版本非零且不超过已发布代次上限；
    // 清单版本（登记表修订号）必须与之独立且 ≥1。
    let mut version_ok = d1.version >= 1;
    let mut i15 = 0usize;
    while i15 < syms.len() {
        if let Some(sym) = syms.get(i15) {
            if sym.version == 0 || sym.version > MAX_API_VERSION {
                version_ok = false;
            }
        }
        i15 += 1;
    }
    s.add("50075-J1-版本单调", version_ok, if version_ok { "" } else { "版本越界或回退" });

    // J1-16 管线/UI 同点：同一关注点两次查询返回同一符号（单点可复现）。
    let same = match &reg {
        Ok(r) => match (r.frozen_on(Concern::Feed), r.frozen_on(Concern::Feed)) {
            (Some(a), Some(b)) => a.id == b.id && a.sig == b.sig,
            _ => false,
        },
        Err(_) => false,
    };
    s.add("50075-J1-同点同符号", same, if same { "" } else { "同点返回不同符号" });

    // J1-17 六类坏样本全抓。
    let (caught, total) = injection_core();
    let ok = caught == total;
    let det = format!("注入 {}/{}", caught, total);
    s.add_dyn("50075-J1-注入必抓", ok, &det);

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx50075_checks_green() {
        let set = run_unx50075_checks();
        assert!(set.all_passed(), "UNX-F50075 red: {}", set.red_items_text());
    }

    #[test]
    fn unx50075_baseline_is_single_point_and_frozen() {
        let mut r = baseline_registry();
        assert!(r.is_ok());
        if let Ok(reg) = r.as_mut() {
            assert_eq!(reg.len(), 6);
            assert_eq!(reg.coverage(), 6);
            let m = reg.freeze();
            assert!(m.is_ok());
            if let Ok(man) = m {
                assert_eq!(man.digest, FROZEN_MANIFEST);
                assert_eq!(reg.verify(&man).ok(), Some(6));
                let extra =
                    ApiSymbol::new(7, "paper.x", 1, 1, Concern::Feed, Stability::Deprecated);
                assert_eq!(reg.register(extra).err(), Some(FreezeError::Sealed));
            }
        }
    }

    #[test]
    fn unx50075_order_does_not_change_digest() {
        let syms = baseline_symbols();
        let mut fwd = FreezeRegistry::new(8);
        let mut rev = FreezeRegistry::new(8);
        for s in syms.iter() {
            let _ = fwd.register(*s);
        }
        for s in syms.iter().rev() {
            let _ = rev.register(*s);
        }
        assert_eq!(fwd.digest(), rev.digest());
    }

    #[test]
    fn unx50075_injection_full() {
        let (caught, total) = injection_core();
        assert_eq!(caught, total);
        assert_eq!(total, 6);
    }
}
