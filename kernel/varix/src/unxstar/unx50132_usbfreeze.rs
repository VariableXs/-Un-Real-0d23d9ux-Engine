//! UNX-F50132 USB 冻结接口（后端消费单点面）（0x43ca0 段）
//!
//! 锚点：UNX-F50132-J1 接口冻结登记；判据锚定 §AI-63 §4。
//!
//! 为什么要有「冻结面」：USB 打印后端有五个消费点——挂载（Attach）、提交
//! （Submit）、取消（Cancel）、状态（Status）、恢复（Recover）。只要其中任一
//! 消费点各自解读版本、各自补默认值，「同一台设备、两条消费路径、两个结论」
//! 就会成为常态，而且事后无法判断谁读错了。本模块把接口表做成**可审计的登记**：
//! 声明（Draft）→ 冻结（Frozen，产出封印）→ 被新版取代（Superseded）。
//!
//! 三条硬语义：
//! * 冻结后字段不可改写（`retune` 必拒）——冻结的意义就是「之后谁都改不动」；
//! * 封印覆盖整张表（名字/档位/版本/校验和/状态），任何一字节漂移都会让
//!   `audit()` 失配，因此 `import` 可以当作「重启后从持久化快照恢复」的验签口；
//! * 一个后端一条绑定（`ConsumerTable`），重复绑定必须显式 `unbind`——这就是
//!   「消费单点面」的机制化：不允许同一后端挂两条互相矛盾的读取路径。
//!
//! 版本链模型：名字是**族**，同名条目按版本递增登记；同一族任何时刻最多只有
//! 一个非取代态条目，且新版必须在旧版冻结之后才能声明——否则一张表里会同时
//! 躺着两个「当前版」，绑定谁都能自圆其说，冻结面等于没有。
//!
//! 数据结构：
//! * `SurfaceKind` —— 消费面五档（与后端调用点一一对应）；
//! * `FreezeState` —— 条目三态状态机（Draft / Frozen / Superseded）；
//! * `FreezeRegistry` —— 冻结登记表 + 封印 + 冻结/取代计数（幂等重冻结凭据）；
//! * `ConsumerTable` —— 后端消费绑定（版本钉住、陈旧可复算）；
//! * `FreezeAudit` —— 审计结果（封印、复算封印、草稿数、取代数、是否完好）。
//!
//! 错误路径：`FreezeError::code()` 返回 `SEG_BASE + n`，段内闭集；全部码值都在
//! 判据或注入核里被真实构造，不存在「登记了却永远走不到」的假分支。

use crate::alloc::format;
use crate::alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x43ca0 独占 0x20）。
pub const SEG_BASE: u32 = 0x43ca0;

pub const E_EMPTY: u16 = 1;
pub const E_CAPACITY: u16 = 2;
pub const E_DUP_SURFACE: u16 = 3;
pub const E_SEALED: u16 = 4;
pub const E_UNKNOWN: u16 = 5;
pub const E_NOT_FROZEN: u16 = 6;
pub const E_FROZEN_MUTATE: u16 = 7;
pub const E_SUPERSEDED: u16 = 8;
pub const E_VERSION_ORDER: u16 = 9;
pub const E_KIND_MISMATCH: u16 = 10;
pub const E_SEAL_DRIFT: u16 = 11;
pub const E_REBIND: u16 = 12;
pub const E_VERSION_PIN: u16 = 13;
pub const E_DRAFT_OPEN: u16 = 14;
pub const E_RESERVED_KIND: u16 = 15;

/// 单表条目上限。五个消费面 × 最多三版修订，超过即说明调用方在滥用登记表。
pub const MAX_SURFACES: usize = 16;

/// 消费绑定上限（后端消费点总数：枚举器/直通/状态轮询/取消/恢复 + 余量）。
pub const MAX_CONSUMERS: usize = 8;

/// 冻结面错误闭集。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FreezeError {
    Empty,
    Capacity,
    DupSurface,
    Sealed,
    Unknown,
    NotFrozen,
    FrozenMutate,
    Superseded,
    VersionOrder,
    KindMismatch,
    SealDrift,
    Rebind,
    VersionPin,
    DraftOpen,
    ReservedKind,
}

impl FreezeError {
    /// 诊断码：段基址 + 段内序号（闭集，全仓唯一）。
    pub fn code(self) -> u32 {
        let n = match self {
            FreezeError::Empty => E_EMPTY,
            FreezeError::Capacity => E_CAPACITY,
            FreezeError::DupSurface => E_DUP_SURFACE,
            FreezeError::Sealed => E_SEALED,
            FreezeError::Unknown => E_UNKNOWN,
            FreezeError::NotFrozen => E_NOT_FROZEN,
            FreezeError::FrozenMutate => E_FROZEN_MUTATE,
            FreezeError::Superseded => E_SUPERSEDED,
            FreezeError::VersionOrder => E_VERSION_ORDER,
            FreezeError::KindMismatch => E_KIND_MISMATCH,
            FreezeError::SealDrift => E_SEAL_DRIFT,
            FreezeError::Rebind => E_REBIND,
            FreezeError::VersionPin => E_VERSION_PIN,
            FreezeError::DraftOpen => E_DRAFT_OPEN,
            FreezeError::ReservedKind => E_RESERVED_KIND,
        };
        SEG_BASE + n as u32
    }

    /// 一行可读原因（诊断/日志共用）。
    pub fn message(self) -> &'static str {
        match self {
            FreezeError::Empty => "empty name or empty table",
            FreezeError::Capacity => "freeze table or consumer table full",
            FreezeError::DupSurface => "surface name+version already declared",
            FreezeError::Sealed => "first epoch sealed; only revisions accepted",
            FreezeError::Unknown => "surface not declared",
            FreezeError::NotFrozen => "surface is still a draft",
            FreezeError::FrozenMutate => "frozen surface cannot be mutated",
            FreezeError::Superseded => "surface already superseded",
            FreezeError::VersionOrder => "revision version must strictly increase",
            FreezeError::KindMismatch => "revision changed surface kind",
            FreezeError::SealDrift => "recomputed seal differs from claimed seal",
            FreezeError::Rebind => "rebind requires unbind first",
            FreezeError::VersionPin => "consumer pinned to an undeclared version",
            FreezeError::DraftOpen => "snapshot contains open drafts",
            FreezeError::ReservedKind => "surface kind code outside 1..=5",
        }
    }
}

/// 消费面五档。编码写死并与后端调用点对齐，不接受外部传入的顺序。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SurfaceKind {
    Attach,
    Submit,
    Cancel,
    Status,
    Recover,
}

impl SurfaceKind {
    pub const ALL: [SurfaceKind; 5] = [
        SurfaceKind::Attach,
        SurfaceKind::Submit,
        SurfaceKind::Cancel,
        SurfaceKind::Status,
        SurfaceKind::Recover,
    ];

    pub fn code(self) -> u8 {
        match self {
            SurfaceKind::Attach => 1,
            SurfaceKind::Submit => 2,
            SurfaceKind::Cancel => 3,
            SurfaceKind::Status => 4,
            SurfaceKind::Recover => 5,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            SurfaceKind::Attach => "attach",
            SurfaceKind::Submit => "submit",
            SurfaceKind::Cancel => "cancel",
            SurfaceKind::Status => "status",
            SurfaceKind::Recover => "recover",
        }
    }

    /// 码值 → 档位。0 与 >5 都是保留值（0 表示「未定」，不得当成 Attach）。
    pub fn from_code(c: u8) -> Result<SurfaceKind, FreezeError> {
        match c {
            1 => Ok(SurfaceKind::Attach),
            2 => Ok(SurfaceKind::Submit),
            3 => Ok(SurfaceKind::Cancel),
            4 => Ok(SurfaceKind::Status),
            5 => Ok(SurfaceKind::Recover),
            _ => Err(FreezeError::ReservedKind),
        }
    }
}

/// 条目三态。Superseded 是终态：被取代的旧版只保留可追溯性，不再可消费。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FreezeState {
    Draft,
    Frozen,
    Superseded,
}

impl FreezeState {
    fn code(self) -> u8 {
        match self {
            FreezeState::Draft => 0,
            FreezeState::Frozen => 1,
            FreezeState::Superseded => 2,
        }
    }
}

/// 一个消费面条目（定长、可 Copy、无堆）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Surface {
    pub name: &'static str,
    pub kind: SurfaceKind,
    pub version: u16,
    pub checksum: u32,
    pub state: FreezeState,
}

impl Surface {
    /// 按档位/版本/校验和构造一个草稿条目。
    pub fn draft(name: &'static str, kind: SurfaceKind, version: u16, checksum: u32) -> Surface {
        Surface {
            name,
            kind,
            version,
            checksum,
            state: FreezeState::Draft,
        }
    }
}

fn fnv_u8(mut h: u64, b: u8) -> u64 {
    h ^= b as u64;
    h.wrapping_mul(0x100_0000_01b3)
}

fn fnv_u32(h: u64, v: u32) -> u64 {
    let mut r = h;
    let mut i = 0usize;
    while i < 4 {
        r = fnv_u8(r, ((v >> (8 * i)) & 0xff) as u8);
        i += 1;
    }
    r
}

fn fnv_name(mut h: u64, name: &str) -> u64 {
    let b = name.as_bytes();
    let mut i = 0usize;
    while i < b.len() {
        if let Some(c) = b.get(i) {
            h = fnv_u8(h, *c);
        }
        i += 1;
    }
    h
}

/// 表封印：按**插入序**折叠名字、档位、版本、校验和与状态。
///
/// 状态必须进封印——否则「把 Superseded 悄悄改回 Frozen」这种最危险的
/// 回滚动作不会被审计发现。插入序进封印是刻意的：冻结表是一份有次序的
/// 登记文件，重排条目属于另一份表，理应产出另一个封印。
pub fn seal_of(rows: &[Surface]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325 ^ rows.len() as u64;
    let mut i = 0usize;
    while i < rows.len() {
        if let Some(s) = rows.get(i) {
            h = fnv_name(h, s.name);
            h = fnv_u8(h, s.kind.code());
            h = fnv_u32(h, s.version as u32);
            h = fnv_u32(h, s.checksum);
            h = fnv_u8(h, s.state.code());
        }
        i += 1;
    }
    h
}

/// 审计结果。`intact` 是「当前表仍与封印一致」的唯一权威。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FreezeAudit {
    pub entries: usize,
    pub drafts: usize,
    pub superseded: usize,
    pub families: usize,
    pub seal: u64,
    pub recomputed: u64,
    pub intact: bool,
}

/// 冻结登记表。
#[derive(Clone, Debug)]
pub struct FreezeRegistry {
    surfaces: Vec<Surface>,
    cap: usize,
    seal: u64,
    freezes: u16,
    revisions: u16,
}

impl FreezeRegistry {
    /// 新建空登记表。容量限制在 1..=MAX_SURFACES，0 或超限都显式拒绝。
    pub fn new(cap: usize) -> Result<FreezeRegistry, FreezeError> {
        if cap == 0 || cap > MAX_SURFACES {
            return Err(FreezeError::Capacity);
        }
        Ok(FreezeRegistry {
            surfaces: Vec::with_capacity(cap),
            cap,
            seal: 0,
            freezes: 0,
            revisions: 0,
        })
    }

    pub fn len(&self) -> usize {
        self.surfaces.len()
    }

    pub fn is_empty(&self) -> bool {
        self.surfaces.is_empty()
    }

    /// 当前封印（未冻结过时为 0，与「封印值恰为 0」不会混淆：未冻结即无封印）。
    pub fn seal(&self) -> u64 {
        self.seal
    }

    pub fn freezes(&self) -> u16 {
        self.freezes
    }

    pub fn revisions(&self) -> u16 {
        self.revisions
    }

    pub fn surface_at(&self, i: usize) -> Option<Surface> {
        self.surfaces.get(i).copied()
    }

    /// 精确查（名字 + 版本）。名字单独不足以定位：一族有多版。
    pub fn find_exact(&self, name: &str, version: u16) -> Option<usize> {
        let mut i = 0usize;
        while i < self.surfaces.len() {
            if let Some(s) = self.surfaces.get(i) {
                if s.name == name && s.version == version {
                    return Some(i);
                }
            }
            i += 1;
        }
        None
    }

    /// 查同族最新版（版本号最大的那条），用于修订序校验与陈旧判定。
    pub fn latest(&self, name: &str) -> Option<Surface> {
        let mut out: Option<Surface> = None;
        let mut i = 0usize;
        while i < self.surfaces.len() {
            if let Some(s) = self.surfaces.get(i) {
                if s.name == name {
                    let newer = match out {
                        Some(prev) => s.version > prev.version,
                        None => true,
                    };
                    if newer {
                        out = Some(*s);
                    }
                }
            }
            i += 1;
        }
        out
    }

    /// 族数（J1 登记口径：五族 × 各自版本链）。
    pub fn families(&self) -> usize {
        let mut n = 0usize;
        let mut i = 0usize;
        while i < self.surfaces.len() {
            if let Some(s) = self.surfaces.get(i) {
                let mut j = 0usize;
                let mut first = true;
                while j < i {
                    if let Some(t) = self.surfaces.get(j) {
                        if t.name == s.name {
                            first = false;
                        }
                    }
                    j += 1;
                }
                if first {
                    n += 1;
                }
            }
            i += 1;
        }
        n
    }

    /// 声明一个消费面（首版或修订版）。
    ///
    /// 规则顺序有讲究：
    /// 1. 名字/版本先验（空名、版本 0 都是登记噪声，先拒）；
    /// 2. 同 (名字,版本) 重复 → 重复登记；
    /// 3. 同族必须档位一致、版本严格递增；
    /// 4. 同族还有草稿未冻结 → 拒绝（否则一族两个「当前版」）；
    /// 5. 新族在首纪元冻结后出现 → 拒绝（冻结集合不可悄悄扩大）。
    pub fn declare(
        &mut self,
        name: &'static str,
        kind: SurfaceKind,
        version: u16,
        checksum: u32,
    ) -> Result<usize, FreezeError> {
        if name.is_empty() {
            return Err(FreezeError::Empty);
        }
        if version == 0 {
            // 版本 0 保留给「未定版」，冻结表里不允许出现未定版条目。
            return Err(FreezeError::VersionOrder);
        }
        if self.find_exact(name, version).is_some() {
            return Err(FreezeError::DupSurface);
        }
        match self.latest(name) {
            Some(prev) => {
                if prev.kind != kind {
                    return Err(FreezeError::KindMismatch);
                }
                if version <= prev.version {
                    return Err(FreezeError::VersionOrder);
                }
                if prev.state == FreezeState::Draft {
                    return Err(FreezeError::NotFrozen);
                }
            }
            None => {
                if self.freezes > 0 {
                    return Err(FreezeError::Sealed);
                }
            }
        }
        if self.surfaces.len() >= self.cap {
            return Err(FreezeError::Capacity);
        }
        self.surfaces.push(Surface::draft(name, kind, version, checksum));
        Ok(self.surfaces.len() - 1)
    }

    /// 改草稿的校验和。冻结后一律拒绝——这是冻结的核心承诺。
    pub fn retune(&mut self, name: &str, version: u16, checksum: u32) -> Result<(), FreezeError> {
        let k = match self.find_exact(name, version) {
            Some(i) => i,
            None => return Err(FreezeError::Unknown),
        };
        let s = match self.surfaces.get_mut(k) {
            Some(x) => x,
            None => return Err(FreezeError::Unknown),
        };
        if s.state != FreezeState::Draft {
            return Err(FreezeError::FrozenMutate);
        }
        s.checksum = checksum;
        Ok(())
    }

    /// 冻结当前全部草稿并重算封印。
    ///
    /// 幂等：没有新草稿时重复调用产出**同一个封印**（`freezes` 计数照加，
    /// 它是「后端在第几次开机时重新确认过」的凭据，不是封印的输入）。
    pub fn freeze(&mut self) -> Result<u64, FreezeError> {
        if self.surfaces.is_empty() {
            return Err(FreezeError::Empty);
        }
        let mut i = 0usize;
        while i < self.surfaces.len() {
            if let Some(s) = self.surfaces.get_mut(i) {
                if s.state == FreezeState::Draft {
                    s.state = FreezeState::Frozen;
                }
            }
            i += 1;
        }
        let seal = seal_of(&self.surfaces);
        self.seal = seal;
        self.freezes = self.freezes.saturating_add(1);
        Ok(seal)
    }

    /// 用同族的 `new_v` 取代 `old_v`（旧版转 Superseded，封印随之变更）。
    ///
    /// 只允许「已冻结 → 已冻结」的取代：拿草稿去取代等于用未审的接口顶掉
    /// 已审的接口，正是冻结面最该拦住的动作。
    pub fn supersede(&mut self, name: &str, old_v: u16, new_v: u16) -> Result<u64, FreezeError> {
        let oi = match self.find_exact(name, old_v) {
            Some(i) => i,
            None => return Err(FreezeError::Unknown),
        };
        let ni = match self.find_exact(name, new_v) {
            Some(i) => i,
            None => return Err(FreezeError::Unknown),
        };
        let o = match self.surfaces.get(oi) {
            Some(s) => *s,
            None => return Err(FreezeError::Unknown),
        };
        let n = match self.surfaces.get(ni) {
            Some(s) => *s,
            None => return Err(FreezeError::Unknown),
        };
        match o.state {
            FreezeState::Draft => return Err(FreezeError::NotFrozen),
            FreezeState::Superseded => return Err(FreezeError::Superseded),
            FreezeState::Frozen => {}
        }
        if n.state != FreezeState::Frozen {
            return Err(FreezeError::NotFrozen);
        }
        if o.kind != n.kind {
            return Err(FreezeError::KindMismatch);
        }
        if new_v <= old_v {
            return Err(FreezeError::VersionOrder);
        }
        if let Some(s) = self.surfaces.get_mut(oi) {
            s.state = FreezeState::Superseded;
        }
        let seal = seal_of(&self.surfaces);
        self.seal = seal;
        self.revisions = self.revisions.saturating_add(1);
        Ok(seal)
    }

    /// 审计：复算封印并与登记封印对比。冻结表在重启后靠它自证未被篡改。
    pub fn audit(&self) -> FreezeAudit {
        let mut drafts = 0usize;
        let mut superseded = 0usize;
        let mut i = 0usize;
        while i < self.surfaces.len() {
            if let Some(s) = self.surfaces.get(i) {
                if s.state == FreezeState::Draft {
                    drafts += 1;
                } else if s.state == FreezeState::Superseded {
                    superseded += 1;
                }
            }
            i += 1;
        }
        let recomputed = seal_of(&self.surfaces);
        FreezeAudit {
            entries: self.surfaces.len(),
            drafts,
            superseded,
            families: self.families(),
            seal: self.seal,
            recomputed,
            intact: self.seal != 0 && recomputed == self.seal,
        }
    }

    /// 导出快照（按插入序），供持久化与 `import` 验签往返。
    pub fn export(&self) -> Vec<Surface> {
        let mut out = Vec::with_capacity(self.surfaces.len());
        let mut i = 0usize;
        while i < self.surfaces.len() {
            if let Some(s) = self.surfaces.get(i) {
                out.push(*s);
            }
            i += 1;
        }
        out
    }

    /// 从快照恢复并验签。
    ///
    /// 快照里出现草稿即拒绝：持久化的冻结表必须是全终态，否则恢复出来的
    /// 「冻结面」还带着可变条目，冻结就名存实亡。
    pub fn import(rows: &[Surface], claimed_seal: u64) -> Result<FreezeRegistry, FreezeError> {
        if rows.is_empty() {
            return Err(FreezeError::Empty);
        }
        if rows.len() > MAX_SURFACES {
            return Err(FreezeError::Capacity);
        }
        let mut i = 0usize;
        while i < rows.len() {
            let s = match rows.get(i) {
                Some(v) => *v,
                None => return Err(FreezeError::Empty),
            };
            if s.name.is_empty() || s.version == 0 {
                return Err(FreezeError::Empty);
            }
            if s.state == FreezeState::Draft {
                return Err(FreezeError::DraftOpen);
            }
            if SurfaceKind::from_code(s.kind.code()).is_err() {
                return Err(FreezeError::ReservedKind);
            }
            let mut j = 0usize;
            while j < i {
                if let Some(t) = rows.get(j) {
                    if t.name == s.name && t.version == s.version {
                        return Err(FreezeError::DupSurface);
                    }
                }
                j += 1;
            }
            i += 1;
        }
        let seal = seal_of(rows);
        if seal != claimed_seal {
            return Err(FreezeError::SealDrift);
        }
        let mut superseded = 0u16;
        let mut k = 0usize;
        while k < rows.len() {
            if let Some(s) = rows.get(k) {
                if s.state == FreezeState::Superseded {
                    superseded = superseded.saturating_add(1);
                }
            }
            k += 1;
        }
        let mut surfaces = Vec::with_capacity(rows.len());
        let mut m = 0usize;
        while m < rows.len() {
            if let Some(s) = rows.get(m) {
                surfaces.push(*s);
            }
            m += 1;
        }
        Ok(FreezeRegistry {
            surfaces,
            cap: MAX_SURFACES,
            seal,
            freezes: 1,
            revisions: superseded,
        })
    }
}

/// 后端消费绑定。`stale` 由 `refresh` 复算：被取代或版本消失即为陈旧。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Binding {
    pub backend: &'static str,
    pub surface: &'static str,
    pub version: u16,
    pub stale: bool,
}

/// 后端消费表：一个后端一条绑定，只允许消费已冻结且版本匹配的条目。
pub struct ConsumerTable {
    bindings: Vec<Binding>,
    cap: usize,
    rejects: u16,
    stale: u16,
}

impl ConsumerTable {
    pub fn new(cap: usize) -> Result<ConsumerTable, FreezeError> {
        if cap == 0 || cap > MAX_CONSUMERS {
            return Err(FreezeError::Capacity);
        }
        Ok(ConsumerTable {
            bindings: Vec::with_capacity(cap),
            cap,
            rejects: 0,
            stale: 0,
        })
    }

    pub fn len(&self) -> usize {
        self.bindings.len()
    }

    pub fn is_empty(&self) -> bool {
        self.bindings.is_empty()
    }

    pub fn rejects(&self) -> u16 {
        self.rejects
    }

    pub fn stale(&self) -> u16 {
        self.stale
    }

    pub fn get(&self, i: usize) -> Option<&Binding> {
        self.bindings.get(i)
    }

    /// 绑定一个后端到某个已冻结的消费面（按精确版本钉住）。
    ///
    /// 顺序刻意固定为「族存在 → 该版本已声明 → 已冻结 → 唯一性 → 容量」：
    /// 先校验收目标本身是否可消费，再谈绑定关系，报错才指向真正的根因。
    pub fn bind(
        &mut self,
        reg: &FreezeRegistry,
        backend: &'static str,
        surface: &'static str,
        pin: u16,
    ) -> Result<usize, FreezeError> {
        if backend.is_empty() {
            self.rejects = self.rejects.saturating_add(1);
            return Err(FreezeError::Empty);
        }
        if reg.latest(surface).is_none() {
            self.rejects = self.rejects.saturating_add(1);
            return Err(FreezeError::Unknown);
        }
        let k = match reg.find_exact(surface, pin) {
            Some(i) => i,
            None => {
                self.rejects = self.rejects.saturating_add(1);
                return Err(FreezeError::VersionPin);
            }
        };
        let s = match reg.surface_at(k) {
            Some(v) => v,
            None => {
                self.rejects = self.rejects.saturating_add(1);
                return Err(FreezeError::Unknown);
            }
        };
        match s.state {
            FreezeState::Draft => {
                self.rejects = self.rejects.saturating_add(1);
                return Err(FreezeError::NotFrozen);
            }
            FreezeState::Superseded => {
                self.rejects = self.rejects.saturating_add(1);
                return Err(FreezeError::Superseded);
            }
            FreezeState::Frozen => {}
        }
        let mut i = 0usize;
        while i < self.bindings.len() {
            if let Some(b) = self.bindings.get(i) {
                if b.backend == backend {
                    self.rejects = self.rejects.saturating_add(1);
                    // 同一后端已有绑定：即便指向同一消费面也要求先 unbind，
                    // 否则「这一条绑定是什么时候建立的」将无法回答。
                    return Err(FreezeError::Rebind);
                }
            }
            i += 1;
        }
        if self.bindings.len() >= self.cap {
            self.rejects = self.rejects.saturating_add(1);
            return Err(FreezeError::Capacity);
        }
        self.bindings.push(Binding {
            backend,
            surface,
            version: pin,
            stale: false,
        });
        Ok(self.bindings.len() - 1)
    }

    /// 解绑（Rebind 的前置条件）。
    pub fn unbind(&mut self, backend: &str) -> Result<(), FreezeError> {
        let mut i = 0usize;
        while i < self.bindings.len() {
            let hit = match self.bindings.get(i) {
                Some(b) => b.backend == backend,
                None => false,
            };
            if hit {
                self.bindings.remove(i);
                return Ok(());
            }
            i += 1;
        }
        Err(FreezeError::Unknown)
    }

    /// 复算全部绑定的陈旧状态，返回 (live, stale)。
    ///
    /// 必须由调用方在每次取代后调用：宁可让后端读到「stale=true」的显式信号，
    /// 也不要让它继续用旧版接口而自以为新鲜。
    pub fn refresh(&mut self, reg: &FreezeRegistry) -> (usize, usize) {
        let mut live = 0usize;
        let mut stale = 0usize;
        let mut i = 0usize;
        while i < self.bindings.len() {
            let surface = match self.bindings.get(i) {
                Some(b) => b.surface,
                None => "",
            };
            let version = match self.bindings.get(i) {
                Some(b) => b.version,
                None => 0,
            };
            let mut is_stale = true;
            if let Some(k) = reg.find_exact(surface, version) {
                if let Some(s) = reg.surface_at(k) {
                    is_stale = s.state != FreezeState::Frozen;
                }
            }
            if let Some(b) = self.bindings.get_mut(i) {
                b.stale = is_stale;
            }
            if is_stale {
                stale += 1;
            } else {
                live += 1;
            }
            i += 1;
        }
        self.stale = stale as u16;
        (live, stale)
    }
}

/// 诊断渲染（一行，便于落串口日志）。
pub fn render(reg: &FreezeRegistry) -> String {
    let a = reg.audit();
    format!(
        "freeze entries={} families={} drafts={} sup={} seal={:016x} intact={} freezes={}",
        a.entries,
        a.families,
        a.drafts,
        a.superseded,
        a.seal,
        a.intact,
        reg.freezes()
    )
}

// ---------------------------------------------------------------------------
// 基准表与注入核
// ---------------------------------------------------------------------------

/// 五族名字（与 `SurfaceKind::ALL` 同序）。
pub const FAMILY_NAMES: [&str; 5] = [
    "usb.attach",
    "usb.submit",
    "usb.cancel",
    "usb.status",
    "usb.recover",
];

/// 构造一张最小的合法冻结表（五个消费面各一版，全部冻结）。
pub fn sample_registry() -> Result<FreezeRegistry, FreezeError> {
    let mut r = FreezeRegistry::new(8)?;
    let mut i = 0usize;
    while i < SurfaceKind::ALL.len() {
        if let (Some(k), Some(name)) = (SurfaceKind::ALL.get(i), FAMILY_NAMES.get(i)) {
            r.declare(name, *k, 1, 0x1000 + i as u32)?;
        }
        i += 1;
    }
    r.freeze()?;
    Ok(r)
}

/// 注入核：九类坏动作必须被显式拒绝（逐类断言具体码）。
pub fn injection_core() -> (usize, usize) {
    let mut caught = 0usize;
    let total = 9usize;

    // 1 空名字
    let empty_name = FreezeRegistry::new(4).and_then(|mut r| r.declare("", SurfaceKind::Attach, 1, 1));
    if empty_name.err() == Some(FreezeError::Empty) {
        caught += 1;
    }
    // 2 同 (名字,版本) 重复声明
    let dup = FreezeRegistry::new(4).and_then(|mut r| {
        r.declare("a", SurfaceKind::Attach, 1, 1)?;
        r.declare("a", SurfaceKind::Attach, 1, 2)
    });
    if dup.err() == Some(FreezeError::DupSurface) {
        caught += 1;
    }
    // 3 空表冻结
    let empty_freeze = FreezeRegistry::new(4).and_then(|mut r| r.freeze());
    if empty_freeze.err() == Some(FreezeError::Empty) {
        caught += 1;
    }
    let mut reg = match sample_registry() {
        Ok(r) => r,
        Err(_) => return (caught, total),
    };
    // 4 冻结后改写校验和
    if reg.retune("usb.submit", 1, 0xdead).err() == Some(FreezeError::FrozenMutate) {
        caught += 1;
    }
    // 5 冻结后声明新族
    if reg.declare("usb.extra", SurfaceKind::Status, 1, 7).err() == Some(FreezeError::Sealed) {
        caught += 1;
    }
    // 6 同族两个草稿（旧版未冻结就声明新版）
    let two_drafts = FreezeRegistry::new(4).and_then(|mut r| {
        r.declare("b", SurfaceKind::Submit, 1, 1)?;
        r.declare("b", SurfaceKind::Submit, 2, 2)
    });
    if two_drafts.err() == Some(FreezeError::NotFrozen) {
        caught += 1;
    }
    // 7 用草稿取代（旧版仍是 Draft）：先冻结 v1，再声明 v2 但不冻结，
    //   然后尝试「v2 取代 v1」——旧版是草稿，必须被 NotFrozen 拦住。
    let mut drafty = match FreezeRegistry::new(4) {
        Ok(r) => r,
        Err(_) => return (caught, total),
    };
    let d1 = drafty.declare("c", SurfaceKind::Cancel, 1, 1);
    let f1 = drafty.freeze();
    let d2 = drafty.declare("c", SurfaceKind::Cancel, 2, 2);
    let draft_sup = if d1.is_ok() && f1.is_ok() && d2.is_ok() {
        drafty.supersede("c", 2, 1).err()
    } else {
        None
    };
    if draft_sup == Some(FreezeError::NotFrozen) {
        caught += 1;
    }
    // 8 快照验签漂移
    let mut rows = reg.export();
    if let Some(first) = rows.get_mut(0) {
        first.checksum ^= 0xffff_ffff;
    }
    if FreezeRegistry::import(&rows, reg.seal()).err() == Some(FreezeError::SealDrift) {
        caught += 1;
    }
    // 9 快照里带草稿
    let draft_rows = [Surface::draft("usb.attach", SurfaceKind::Attach, 1, 9)];
    let claimed = seal_of(&draft_rows);
    if FreezeRegistry::import(&draft_rows, claimed).err() == Some(FreezeError::DraftOpen) {
        caught += 1;
    }

    (caught, total)
}

// ---------------------------------------------------------------------------
// 判据
// ---------------------------------------------------------------------------

pub fn run_unx50132_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F50132");

    let reg = match sample_registry() {
        Ok(r) => r,
        Err(_) => {
            s.add("50132-J1-基准构建", false, "样例表构建失败");
            return s;
        }
    };
    let a = reg.audit();

    // J1-1 冻结登记：五族五条、草稿归零、全部 Frozen、五档齐全
    let mut kinds = [false; 5];
    let mut all_frozen = true;
    let mut i = 0usize;
    while i < reg.len() {
        if let Some(x) = reg.surface_at(i) {
            let c = x.kind.code();
            if c >= 1 && c <= 5 {
                kinds[(c - 1) as usize] = true;
            }
            if x.state != FreezeState::Frozen {
                all_frozen = false;
            }
        }
        i += 1;
    }
    let kinds_ok = kinds[0] && kinds[1] && kinds[2] && kinds[3] && kinds[4];
    let ok = a.entries == 5 && a.families == 5 && a.drafts == 0 && a.superseded == 0 && all_frozen && kinds_ok;
    let det = format!("n={} fam={} drafts={} kinds={}", a.entries, a.families, a.drafts, kinds_ok);
    s.add_dyn("50132-J1-冻结登记", ok, &det);

    // J1-2 冻结幂等：重复冻结产出同一封印（封印不是「冻结次数」的函数）
    let mut again = reg.clone();
    let sa = again.freeze();
    let sb = again.freeze();
    let ok = match (sa, sb) {
        (Ok(x), Ok(y)) => x == y && x == reg.seal() && x != 0,
        _ => false,
    };
    s.add("50132-J1-冻结幂等", ok, if ok { "" } else { "重冻结封印漂移" });

    // J1-3 封印确定性：同内容同序 → 同封印；换序 → 必不同（封印含次序）
    let twin = sample_registry();
    let mut reordered = match FreezeRegistry::new(8) {
        Ok(r) => r,
        Err(_) => return s,
    };
    let mut k = SurfaceKind::ALL.len();
    while k > 0 {
        k -= 1;
        if let (Some(kind), Some(name)) = (SurfaceKind::ALL.get(k), FAMILY_NAMES.get(k)) {
            let _ = reordered.declare(name, *kind, 1, 0x1000 + k as u32);
        }
    }
    let _ = reordered.freeze();
    let ok = match twin {
        Ok(t) => t.seal() == reg.seal() && reordered.seal() != reg.seal(),
        Err(_) => false,
    };
    s.add("50132-J1-封印确定", ok, if ok { "" } else { "封印不含次序或不确定" });

    // J1-4 冻结只读：改写必拒且封印不动
    let mut mutable = reg.clone();
    let ok = mutable.retune("usb.attach", 1, 0xabcdef).err() == Some(FreezeError::FrozenMutate)
        && mutable.seal() == reg.seal();
    s.add("50132-J1-冻结只读", ok, if ok { "" } else { "冻结条目被改写" });

    // J1-5 修订门槛：版本递增才行；同版重复、档位变更、新族一律拒
    let mut rev = reg.clone();
    let adv = rev.declare("usb.submit", SurfaceKind::Submit, 2, 0x2222);
    let dup_same = rev.declare("usb.submit", SurfaceKind::Submit, 2, 0x3333);
    let wrong_kind = rev.declare("usb.submit", SurfaceKind::Status, 3, 0x4444);
    let new_family = rev.declare("usb.brand.new", SurfaceKind::Status, 1, 0x5555);
    let zero_ver = rev.declare("usb.submit", SurfaceKind::Submit, 0, 0x6666);
    let ok = adv.is_ok()
        && dup_same.err() == Some(FreezeError::DupSurface)
        && wrong_kind.err() == Some(FreezeError::KindMismatch)
        && new_family.err() == Some(FreezeError::Sealed)
        && zero_ver.err() == Some(FreezeError::VersionOrder);
    s.add("50132-J1-修订门槛", ok, if ok { "" } else { "修订通道过宽" });

    // J1-6 草稿不许叠：旧版未冻结时声明新版必拒
    let mut solo = match FreezeRegistry::new(4) {
        Ok(r) => r,
        Err(_) => return s,
    };
    let first = solo.declare("usb.status", SurfaceKind::Status, 1, 1);
    let second = solo.declare("usb.status", SurfaceKind::Status, 2, 2);
    let ok = first.is_ok() && second.err() == Some(FreezeError::NotFrozen) && solo.len() == 1;
    s.add("50132-J1-草稿不叠", ok, if ok { "" } else { "同族出现两个草稿" });

    // J1-7 取代链：v1 冻结 → v2 声明并冻结 → 取代 v1；封印变更、计数 +1
    let mut chain = match sample_registry() {
        Ok(r) => r,
        Err(_) => return s,
    };
    let before = chain.seal();
    let d2 = chain.declare("usb.submit", SurfaceKind::Submit, 2, 0x2222);
    let f2 = chain.freeze();
    let sup1 = chain.supersede("usb.submit", 1, 2);
    let sup_again = chain.supersede("usb.submit", 1, 2);
    let old_state = chain.surface_at(match chain.find_exact("usb.submit", 1) {
        Some(i) => i,
        None => 0,
    });
    let ok = d2.is_ok()
        && f2.is_ok()
        && sup1.is_ok()
        && chain.seal() != before
        && chain.revisions() == 1
        && sup_again.err() == Some(FreezeError::Superseded)
        && match old_state {
            Some(x) => x.state == FreezeState::Superseded,
            None => false,
        };
    let det = format!("seal_changed={} revs={}", chain.seal() != before, chain.revisions());
    s.add_dyn("50132-J1-取代链", ok, &det);

    // J1-8 取代版本序：反向 / 同版 / 未声明版本必拒
    let mut order = match sample_registry() {
        Ok(r) => r,
        Err(_) => return s,
    };
    let _ = order.declare("usb.cancel", SurfaceKind::Cancel, 2, 0x2222);
    let _ = order.freeze();
    let back = order.supersede("usb.cancel", 2, 1);
    let same = order.supersede("usb.cancel", 2, 2);
    let missing = order.supersede("usb.cancel", 1, 9);
    let ok = back.err() == Some(FreezeError::VersionOrder)
        && same.err() == Some(FreezeError::VersionOrder)
        && missing.err() == Some(FreezeError::Unknown);
    s.add("50132-J1-取代版本序", ok, if ok { "" } else { "取代版本序失守" });

    // J1-9 审计自洽：封印 == 复算 == 导入封印，且 intact
    let exported = reg.export();
    let imported = FreezeRegistry::import(&exported, reg.seal());
    let ok = a.intact
        && a.seal == a.recomputed
        && match imported {
            Ok(p) => p.seal() == a.seal && p.audit().intact && p.len() == a.entries,
            Err(_) => false,
        };
    s.add("50132-J1-审计自洽", ok, if ok { "" } else { "审计/导入封印不一致" });

    // J1-10 消费单点：同后端二次绑定必拒，解绑后才可重绑
    let mut consumers = match ConsumerTable::new(4) {
        Ok(c) => c,
        Err(_) => return s,
    };
    let b1 = consumers.bind(&reg, "usb-backend", "usb.submit", 1);
    let b2 = consumers.bind(&reg, "usb-backend", "usb.status", 1);
    let rejects_after_dup = consumers.rejects();
    let un = consumers.unbind("usb-backend");
    let b3 = consumers.bind(&reg, "usb-backend", "usb.status", 1);
    let ok = b1.is_ok()
        && b2.err() == Some(FreezeError::Rebind)
        && rejects_after_dup == 1
        && un.is_ok()
        && b3.is_ok()
        && consumers.len() == 1
        && consumers.unbind("nobody").err() == Some(FreezeError::Unknown);
    s.add("50132-J1-消费单点", ok, if ok { "" } else { "一后端多绑定被放行" });

    // J1-11 版本钉住：不存在的版本 / 不存在的族
    let mut pin_tab = match ConsumerTable::new(4) {
        Ok(c) => c,
        Err(_) => return s,
    };
    let bad_pin = pin_tab.bind(&reg, "b1", "usb.submit", 9);
    let no_family = pin_tab.bind(&reg, "b1", "usb.none", 1);
    let ok = bad_pin.err() == Some(FreezeError::VersionPin)
        && no_family.err() == Some(FreezeError::Unknown)
        && pin_tab.len() == 0;
    s.add("50132-J1-版本钉住", ok, if ok { "" } else { "版本钉住失守" });

    // J1-12 未冻结不可消费：草稿 / 已被取代的版本
    let mut drafty = match FreezeRegistry::new(4) {
        Ok(r) => r,
        Err(_) => return s,
    };
    let _ = drafty.declare("usb.attach", SurfaceKind::Attach, 1, 1);
    let mut tab2 = match ConsumerTable::new(4) {
        Ok(c) => c,
        Err(_) => return s,
    };
    let on_draft = tab2.bind(&drafty, "b1", "usb.attach", 1);
    let mut sup_tab_reg = match sample_registry() {
        Ok(r) => r,
        Err(_) => return s,
    };
    let _ = sup_tab_reg.declare("usb.recover", SurfaceKind::Recover, 2, 0x2222);
    let _ = sup_tab_reg.freeze();
    let _ = sup_tab_reg.supersede("usb.recover", 1, 2);
    let on_superseded = tab2.bind(&sup_tab_reg, "b1", "usb.recover", 1);
    let ok = on_draft.err() == Some(FreezeError::NotFrozen)
        && on_superseded.err() == Some(FreezeError::Superseded);
    s.add("50132-J1-未冻不可消费", ok, if ok { "" } else { "草稿/旧版被消费" });

    // J1-13 陈旧复算：取代后旧绑定必须被标 stale
    let mut stale_reg = match sample_registry() {
        Ok(r) => r,
        Err(_) => return s,
    };
    let mut tab3 = match ConsumerTable::new(4) {
        Ok(c) => c,
        Err(_) => return s,
    };
    let bound = tab3.bind(&stale_reg, "old-backend", "usb.status", 1);
    let live_before = tab3.refresh(&stale_reg);
    let _ = stale_reg.declare("usb.status", SurfaceKind::Status, 2, 0x2222);
    let _ = stale_reg.freeze();
    let _ = stale_reg.supersede("usb.status", 1, 2);
    let after = tab3.refresh(&stale_reg);
    let ok = bound.is_ok() && live_before == (1, 0) && after == (0, 1) && tab3.stale() == 1;
    let det = format!("before={:?} after={:?}", live_before, after);
    s.add_dyn("50132-J1-陈旧复算", ok, &det);

    // J1-14 错误码闭集：15 个码全部互异且落在 [SEG_BASE+1, SEG_BASE+15]
    let codes = [
        FreezeError::Empty,
        FreezeError::Capacity,
        FreezeError::DupSurface,
        FreezeError::Sealed,
        FreezeError::Unknown,
        FreezeError::NotFrozen,
        FreezeError::FrozenMutate,
        FreezeError::Superseded,
        FreezeError::VersionOrder,
        FreezeError::KindMismatch,
        FreezeError::SealDrift,
        FreezeError::Rebind,
        FreezeError::VersionPin,
        FreezeError::DraftOpen,
        FreezeError::ReservedKind,
    ];
    let mut distinct = true;
    let mut all_in_seg = true;
    let mut x = 0usize;
    while x < codes.len() {
        let cx = codes[x].code();
        if cx < SEG_BASE + 1 || cx > SEG_BASE + 15 {
            all_in_seg = false;
        }
        let mut y = x + 1;
        while y < codes.len() {
            if codes[y].code() == cx {
                distinct = false;
            }
            y += 1;
        }
        x += 1;
    }
    // 保留值必须真拒
    let reserved = SurfaceKind::from_code(0).err() == Some(FreezeError::ReservedKind)
        && SurfaceKind::from_code(6).err() == Some(FreezeError::ReservedKind);
    let ok = distinct && all_in_seg && reserved;
    s.add("50132-J1-错误码闭集", ok, if ok { "" } else { "码值重复或越段" });

    // J1-15 注入核：九类坏动作全抓
    let (caught, total) = injection_core();
    let ok = caught == total;
    let det = format!("注入 {}/{}", caught, total);
    s.add_dyn("50132-J1-注入必抓", ok, &det);

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx50132_checks_green() {
        let set = run_unx50132_checks();
        assert!(set.all_passed(), "UNX-F50132 red: {}", set.red_items_text());
    }

    #[test]
    fn unx50132_frozen_surface_is_read_only() {
        let reg = sample_registry();
        assert!(reg.is_ok());
        if let Ok(mut r) = reg {
            let seal = r.seal();
            assert_eq!(r.retune("usb.submit", 1, 0).err(), Some(FreezeError::FrozenMutate));
            assert_eq!(r.seal(), seal);
        }
    }

    #[test]
    fn unx50132_supersede_flips_state() {
        let mut r = sample_registry();
        assert!(r.is_ok());
        if let Ok(mut reg) = r {
            assert!(reg.declare("usb.submit", SurfaceKind::Submit, 2, 0x22).is_ok());
            assert!(reg.freeze().is_ok());
            assert!(reg.supersede("usb.submit", 1, 2).is_ok());
            let a = reg.audit();
            assert_eq!(a.superseded, 1);
            assert!(a.intact);
        }
    }

    #[test]
    fn unx50132_error_codes_stay_in_segment() {
        assert_eq!(FreezeError::Empty.code(), SEG_BASE + 1);
        assert_eq!(FreezeError::ReservedKind.code(), SEG_BASE + 15);
    }
}
