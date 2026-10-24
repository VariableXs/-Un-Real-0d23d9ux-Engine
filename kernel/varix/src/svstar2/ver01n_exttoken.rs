//! VE-F3414 · 第三方令牌扩展 —— 命名空间隔离 + 官方冲突检测 + 两律同守 + 优先级仲裁。
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F3414`
//!
//! # 职责（锚点原文拆解）
//!
//! - **扩展作者的自定义令牌注册**：任何插件作者都能往界面上加令牌，
//!   但只能加在自己的命名空间里——`neon.brand` 可以，`color.accent`
//!   不行；命名空间是扩展唯一的护栏，不是建议；
//! - **与官方令牌冲突检测**：三重冲突闸——路径撞官方（`OfficialConflict`）、
//!   字面量抄官方（`ShadowAttempt`，这是对单源律最狠的一刀：两个令牌
//!   同一份真值，改一处漏一处）、扩展内重名（`PeerConflict`）；
//! - **扩展令牌的两律同守**：官方令牌守的两律，扩展令牌一样守。
//!   律一「令牌单源」= 每条扩展令牌必须声明唯一 `canonical_site`，
//!   一个定义点不许站两个真值；律二「变更有迹」= 注册即落
//!   [`ChangeLedger`](crate::svstar2::ver01_arch::ChangeLedger)，
//!   缺主体或缺理由直接拒（无痕变更在架构层没有入口）；
//! - **优先级仲裁说明**：官方恒定优先，扩展只在官方没声明的路径上生效。
//!   这条规则写死在 [`PRIORITY_RULE`] 里，不靠运行时临时决定；
//! - **降级矩阵三行**：冲突→拒绝注册（不入隔离台账）；越界→隔离
//!   （入隔离台账，可查可读屏）；违律→治理（立案累计，达阈值停权）。
//!
//! # 为什么冲突与越界分两种去处
//!
//! 冲突是**令牌从未获得存在资格**——它的路径/字面量根本不是它的，
//! 把它记进隔离台账等于给一个幽灵建户口；越界与违律是**令牌想存在
//! 但不该以这个形态存在**——隔离台账正是它该待的地方，收进去才有
//! 治理可做、才有人在读屏清单里看得见它被拒。分错这两种去处，隔离
//! 台账要么变成垃圾场，要么变成空账。
//!
//! # 为什么字面量抄官方要判最重
//!
//! 路径冲突是明面上的（一眼能看出来），字面量抄官方是暗面的：扩展
//! 声明 `neon.brand = "#4c8dff"`，官方 `color.accent = "#4c8dff"`，
//! 两条路径都不撞，改官方时扩展不会跟着变，界面上同一处颜色就有了
//! 两个真值出处。**单源被绕过的方式从来不是撞名，是抄值**——所以
//! 抄值直接进隔离台账并计治理，不是普通拒绝。
//!
//! # 跨批对接点：W01 沙箱
//!
//! 注册令牌要伸手进显示与色彩面（[`Facet::DisplayColor`]）。沙箱没授权
//! 就是 [`SandboxDenied`]——不授权就不给，最小权限与官方一致（`vew04`
//! 同一纪律）。进程控制面在 `vew04` 里恒不可授权，本模块不重复声明，
//! 只消费它的裁决结果。
//!
//! # 复杂度
//!
//! - `register`：O(官方令牌数 + 扩展令牌数)，锚点口径 O(扩展)——官方快照
//!   是常量级参照表，扩展侧为线性；
//! - `resolve` / `arbitrate`：O(官方 + 扩展)，命中即返；
//! - `governance_scan`：O(隔离台账长)，按命名空间分桶一次过。
//!
//! # 零 panic 面
//!
//! `[i]` / `unwrap()` / `expect()` 只出现在 `#[cfg(test)]`；判据区与
//! 实现区一律 match 记红。

use alloc::vec;
use crate::svstar2::vew04_sandbox::{Facet, SandboxHost};
use crate::svstar2::ver01_arch::{ChangeAction, ChangeLedger, ChangeRequest, KindId, TokenDef};

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 一、诊断码（自建；E16 段独占，与 E15/F3413、E14/F3412 既有段零重叠）
// ---------------------------------------------------------------------------

/// 第三方令牌扩展域诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ExtCode {
    /// 命名空间非法（空/超长/字符集违规）。
    NsInvalid,
    /// 命名空间撞保留词（官方/平台根）。
    NsReserved,
    /// 命名空间已因治理停权。
    NsSuspended,
    /// 路径越界（不在自己命名空间下、含 `..`、越长、空）。
    PathEscape,
    /// 路径与官方令牌撞名。
    OfficialConflict,
    /// 扩展命名空间内路径重名。
    PeerConflict,
    /// 影子注册（字面量抄官方真值，绕开单源）。
    ShadowAttempt,
    /// 违律（缺定义点、缺说明、定义点重占、两律缺项）。
    LawViolation,
    /// W01 沙箱未授权显示与色彩面。
    SandboxDenied,
    /// 作者信任等级不足（未验签作者不许激活令牌）。
    TrustLow,
    /// 容量上限（扩展数 / 单扩展令牌数）。
    RegistryFull,
    /// 令牌处于隔离态，不可解析取用。
    Quarantined,
    /// 路径既无官方声明也无扩展声明（不猜来源）。
    NotFound,
    /// 治理立案（记账类，显性不静默）。
    GovernanceOpen,
}

impl ExtCode {
    /// 全部码（判据据此核对无遗漏）。
    pub const ALL: [ExtCode; 14] = [
        ExtCode::NsInvalid,
        ExtCode::NsReserved,
        ExtCode::NsSuspended,
        ExtCode::PathEscape,
        ExtCode::OfficialConflict,
        ExtCode::PeerConflict,
        ExtCode::ShadowAttempt,
        ExtCode::LawViolation,
        ExtCode::SandboxDenied,
        ExtCode::TrustLow,
        ExtCode::RegistryFull,
        ExtCode::Quarantined,
        ExtCode::NotFound,
        ExtCode::GovernanceOpen,
    ];

    /// 线上短码（E16 段独占）。
    pub const fn code(self) -> &'static str {
        match self {
            ExtCode::NsInvalid => "E16-NS-INVALID",
            ExtCode::NsReserved => "E16-NS-RESERVED",
            ExtCode::NsSuspended => "E16-NS-SUSPENDED",
            ExtCode::PathEscape => "E16-PATH-ESCAPE",
            ExtCode::OfficialConflict => "E16-OFFICIAL-CONFLICT",
            ExtCode::PeerConflict => "E16-PEER-CONFLICT",
            ExtCode::ShadowAttempt => "E16-SHADOW-ATTEMPT",
            ExtCode::LawViolation => "E16-LAW-VIOLATION",
            ExtCode::SandboxDenied => "E16-SANDBOX-DENIED",
            ExtCode::TrustLow => "E16-TRUST-LOW",
            ExtCode::RegistryFull => "E16-REGISTRY-FULL",
            ExtCode::Quarantined => "E16-QUARANTINED",
            ExtCode::NotFound => "E16-NOT-FOUND",
            ExtCode::GovernanceOpen => "E16-GOVERNANCE-OPEN",
        }
    }

    /// 是否阻断。
    pub const fn blocking(self) -> bool {
        !matches!(self, ExtCode::GovernanceOpen)
    }

    /// 是否记账类（治理立案；显性记账不是失败）。
    pub const fn eventful(self) -> bool {
        matches!(self, ExtCode::GovernanceOpen)
    }

    /// 是否落入隔离台账（越界/违律有治理对象；纯冲突无）。
    ///
    /// **这是本域最容易被做糊的一刀**：冲突令牌从未取得存在资格，
    /// 收进台账等于给幽灵建户口；越界与违律令牌是想存在但形态不对，
    /// 收进台账才有人看得见、才有的可治。
    pub const fn quarantinable(self) -> bool {
        matches!(
            self,
            ExtCode::NsInvalid
                | ExtCode::NsReserved
                | ExtCode::PathEscape
                | ExtCode::ShadowAttempt
                | ExtCode::LawViolation
                | ExtCode::SandboxDenied
                | ExtCode::TrustLow
        )
    }

    /// 读屏可达句子。
    pub fn spoken(self) -> String {
        let s = match self {
            ExtCode::NsInvalid => "命名空间非法。",
            ExtCode::NsReserved => "命名空间撞保留词，已拒绝。",
            ExtCode::NsSuspended => "该扩展命名空间已被治理停权。",
            ExtCode::PathEscape => "令牌路径越出本扩展命名空间，已隔离。",
            ExtCode::OfficialConflict => "路径与官方令牌撞名，注册被拒。",
            ExtCode::PeerConflict => "扩展命名空间内路径重名，注册被拒。",
            ExtCode::ShadowAttempt => "影子注册：字面量抄自官方真值，已隔离立案。",
            ExtCode::LawViolation => "违律：两律要件缺项，已隔离。",
            ExtCode::SandboxDenied => "W01 沙箱未授权显示与色彩面，已隔离。",
            ExtCode::TrustLow => "作者信任等级不足，已隔离。",
            ExtCode::RegistryFull => "扩展注册表已满，注册被拒。",
            ExtCode::Quarantined => "令牌处于隔离态，不可解析。",
            ExtCode::NotFound => "该路径既无官方声明也无扩展声明。",
            ExtCode::GovernanceOpen => "治理立案已记账。",
        };
        format!("{}{}", s, self.code())
    }
}

// ---------------------------------------------------------------------------
// 二、契约常量、命名空间语法与信任等级
// ---------------------------------------------------------------------------

/// 契约版本（冻结）。
pub const EXT_CONTRACT: &str = "E01-ext-token-v1";

/// 官方根命名空间（保留，任何扩展不许占）。
pub const OFFICIAL_NS: &str = "var";

/// 保留命名空间闭集（官方根 + 平台根 + 工具根）。
///
/// 这三个是内核自己留的位置，不是"暂时没人用"——占了它们，扩展就能
/// 冒充内核说话，而用户分不出来。
pub const RESERVED_NS: [&str; 3] = ["var", "sys", "platform"];

/// 命名空间标识字节上限（恰上限放行）。
pub const NS_MAX: usize = 32;

/// 令牌路径字节上限（恰上限放行）。
pub const PATH_MAX: usize = 128;

/// 定义点（`文件:行`）字节上限。
pub const SITE_MAX: usize = 96;

/// 扩展命名空间数上限。
pub const MAX_EXTENSIONS: usize = 32;

/// 单扩展令牌数上限。
pub const MAX_TOKENS_PER_EXT: usize = 256;

/// 全表扩展令牌数上限（防御性总量闸）。
pub const MAX_ACTIVE_TOTAL: usize = 1024;

/// 治理停权阈值（隔离立案累计达此数即停权）。
pub const SUSPEND_THRESHOLD: usize = 3;

/// 隔离台账上限（防御性；满后只记不追加并计溢出）。
pub const QUARANTINE_CAP: usize = 64;

/// 优先级仲裁规则（写死说明，判据据此对拍）。
///
/// **官方恒定优先**：官方令牌路径由内核独占，扩展只在官方未声明的
/// 路径上生效；同名时官方胜出且扩展注册当场被拒——不靠运行时"谁后
/// 写入谁赢"，那种规则下插件装得越多界面越不可预测。
pub const PRIORITY_RULE: &str = "官方令牌恒定优先：扩展只在官方未声明的路径上生效；同名官方胜出，扩展注册当场拒绝，不做运行时抢占。";

/// 命名空间标识是否合法字符（小写字母 / 数字 / 连字符，首字符必为字母）。
pub fn ns_char_ok(c: char, first: bool) -> bool {
    if first {
        c.is_ascii_lowercase()
    } else {
        c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-'
    }
}

/// 命名空间语法校验（空/超长/字符集/尾连字符）。
pub fn validate_ns(id: &str) -> Result<(), ExtCode> {
    if id.is_empty() || id.len() > NS_MAX {
        return Err(ExtCode::NsInvalid);
    }
    if id.ends_with('-') {
        return Err(ExtCode::NsInvalid);
    }
    for (i, c) in id.chars().enumerate() {
        if !ns_char_ok(c, i == 0) {
            return Err(ExtCode::NsInvalid);
        }
    }
    for r in RESERVED_NS.iter() {
        if id == *r {
            return Err(ExtCode::NsReserved);
        }
    }
    Ok(())
}

/// 作者信任等级（决定能不能激活令牌，与沙箱授权正交）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Trust {
    /// 未验签作者：可占命名空间，不许激活令牌。
    Untrusted,
    /// 已验签作者：可注册激活令牌。
    Verified,
    /// 官方委派作者：可注册，且在仲裁说明里被点名（非官方级）。
    Core,
}

impl Trust {
    /// 全部等级（三态穷举完备性机检驱动面）。
    pub const ALL: [Trust; 3] = [Trust::Untrusted, Trust::Verified, Trust::Core];

    /// 短名。
    pub const fn code(self) -> &'static str {
        match self {
            Trust::Untrusted => "untrusted",
            Trust::Verified => "verified",
            Trust::Core => "core",
        }
    }

    /// 中文名（读屏用）。
    pub const fn zh(self) -> &'static str {
        match self {
            Trust::Untrusted => "未验签",
            Trust::Verified => "已验签",
            Trust::Core => "官方委派",
        }
    }

    /// 能否激活令牌（`Untrusted` 只能占名，不能生效）。
    pub const fn may_activate(self) -> bool {
        matches!(self, Trust::Verified | Trust::Core)
    }
}

/// 命名空间（扩展身份；受理时校验一次，之后不再改）。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Namespace {
    /// 标识（如 `neon`）。
    pub id: String,
    /// 作者标识。
    pub author: String,
    /// 信任等级。
    pub trust: Trust,
    /// 扩展自述的站点（作者主页/仓库；清单可读屏引用）。
    pub home: String,
}

impl Namespace {
    /// 读屏可读单行。
    pub fn screen_line(&self) -> String {
        format!(
            "扩展命名空间 {}，作者 {}，信任等级 {}，站点 {}",
            self.id,
            self.author,
            self.trust.zh(),
            self.home
        )
    }
}

// ---------------------------------------------------------------------------
// 三、扩展令牌与隔离台账
// ---------------------------------------------------------------------------

/// 扩展令牌定义（真值出处由 `canonical_site` 声明——律一的地基）。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct ExtToken {
    /// 所属命名空间。
    pub ns: String,
    /// 全路径（须以 `ns.` 开头）。
    pub path: String,
    /// 规范字面量（真值本身）。
    pub literal: String,
    /// 类型句柄（语义归 F3405）。
    pub kind_id: KindId,
    /// 唯一定义点（`文件:行`；**必填非空**——律一据此豁免这一处）。
    pub canonical_site: String,
    /// 用途说明（令牌没有说明就等于没有共识）。
    pub doc: String,
    /// 注册逻辑 tick（回归可复现，不用墙钟）。
    pub tick: u64,
}

impl ExtToken {
    /// 构造扩展令牌（省掉调用方逐字段摆弄）。
    pub fn new(
        ns: &str,
        path: &str,
        literal: &str,
        kind_id: KindId,
        canonical_site: &str,
        doc: &str,
        tick: u64,
    ) -> ExtToken {
        ExtToken {
            ns: String::from(ns),
            path: String::from(path),
            literal: String::from(literal),
            kind_id,
            canonical_site: String::from(canonical_site),
            doc: String::from(doc),
            tick,
        }
    }

    /// 路径去掉命名空间前缀后的段名（如 `neon.brand.primary` → `brand.primary`）。
    ///
    /// 前缀不匹配（含切口不落字符边界）时返回空串——不猜、不 panic；
    /// 零分配（`strip_prefix` 只borrow）。
    pub fn local_path(&self) -> &str {
        match self.path.strip_prefix(self.ns.as_str()) {
            Some(rest) => rest.strip_prefix('.').unwrap_or(""),
            None => "",
        }
    }

    /// 读屏可读单行（清单逐条播报用）。
    pub fn screen_line(&self) -> String {
        format!(
            "扩展令牌 {}，值 {}，定义点 {}，说明 {}",
            self.path, self.literal, self.canonical_site, self.doc
        )
    }
}

/// 隔离台账条目（越界/违律/未授权的令牌在此留底——可查可读屏）。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct QuarantineRecord {
    /// 声明的命名空间（未准入时可能压根不是合法命名空间）。
    pub ns: String,
    /// 声明的路径。
    pub path: String,
    /// 隔离原因码。
    pub reason: ExtCode,
    /// 发起主体（留底：谁提交的这颗幽灵令牌）。
    pub actor: String,
    /// 逻辑 tick。
    pub tick: u64,
}

impl QuarantineRecord {
    /// 读屏可读单行。
    pub fn screen_line(&self) -> String {
        format!(
            "隔离条目：命名空间 {}，路径 {}，原因 {}，主体 {}，tick {}",
            self.ns,
            self.path,
            self.reason.code(),
            self.actor,
            self.tick
        )
    }
}

/// 治理立案（按命名空间分桶）。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct GovernanceCase {
    /// 命名空间。
    pub ns: String,
    /// 隔离立案累计。
    pub violations: usize,
    /// 是否已达停权阈值。
    pub suspended: bool,
}

impl GovernanceCase {
    /// 读屏可读单行。
    pub fn screen_line(&self) -> String {
        format!(
            "治理立案：命名空间 {}，隔离 {} 次，{}",
            self.ns,
            self.violations,
            if self.suspended { "已停权" } else { "继续观察" }
        )
    }
}

/// 解析来源（优先级仲裁的结论形态）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Source {
    /// 官方令牌（恒定优先）。
    Official,
    /// 扩展令牌（官方未声明该路径时）。
    Extension,
}

impl Source {
    /// 中文名（读屏用）。
    pub const fn zh(self) -> &'static str {
        match self {
            Source::Official => "官方令牌",
            Source::Extension => "扩展令牌",
        }
    }
}

// ---------------------------------------------------------------------------
// 四、扩展注册表（全闸链 + 隔离治理 + 清单）
// ---------------------------------------------------------------------------

/// 第三方令牌扩展注册表。
#[derive(Clone, Debug)]
pub struct ExtRegistry {
    /// 官方令牌快照（冲突检测的参照系；律一真值的另一半）。
    official: Vec<TokenDef>,
    /// 已准入的扩展命名空间。
    admitted: Vec<Namespace>,
    /// 已受理并激活的扩展令牌。
    active: Vec<ExtToken>,
    /// 隔离台账。
    quarantine: Vec<QuarantineRecord>,
    /// 治理立案计数（只计违律类隔离：律 violation 与影子）。
    governance: u64,
    /// 台账溢出计数（满后只记不追加——丢计数不丢静默）。
    quarantine_dropped: u64,
    /// 律二变更账本（注册与治理落账的唯一去处）。
    ledger: ChangeLedger,
}

impl ExtRegistry {
    /// 构造注册表（官方快照随建；官方令牌不带命名空间前缀）。
    pub fn new(official: Vec<TokenDef>) -> ExtRegistry {
        ExtRegistry {
            official,
            admitted: Vec::new(),
            active: Vec::new(),
            quarantine: Vec::new(),
            governance: 0,
            quarantine_dropped: 0,
            ledger: ChangeLedger::new(),
        }
    }

    /// 官方快照条数。
    pub fn official_count(&self) -> usize {
        self.official.len()
    }

    /// 已准入命名空间数。
    pub fn namespace_count(&self) -> usize {
        self.admitted.len()
    }

    /// 已激活扩展令牌总数。
    pub fn active_count(&self) -> usize {
        self.active.len()
    }

    /// 某命名空间的已激活令牌数。
    pub fn active_of(&self, ns: &str) -> usize {
        self.active.iter().filter(|t| t.ns == ns).count()
    }

    /// 隔离台账长度。
    pub fn quarantine_count(&self) -> usize {
        self.quarantine.len()
    }

    /// 治理立案累计。
    pub fn governance_count(&self) -> u64 {
        self.governance
    }

    /// 台账溢出计数。
    pub fn quarantine_dropped(&self) -> u64 {
        self.quarantine_dropped
    }

    /// 律二账本只读引用（审计对拍用）。
    pub fn ledger(&self) -> &ChangeLedger {
        &self.ledger
    }

    /// 已准入的命名空间列表。
    pub fn namespaces(&self) -> &[Namespace] {
        &self.admitted
    }

    /// 已激活的扩展令牌列表。
    pub fn active_tokens(&self) -> &[ExtToken] {
        &self.active
    }

    /// 隔离台账只读引用。
    pub fn quarantine_log(&self) -> &[QuarantineRecord] {
        &self.quarantine
    }

    /// 取命名空间（未准入 None——不猜）。
    pub fn namespace(&self, id: &str) -> Option<&Namespace> {
        self.admitted.iter().find(|n| n.id == id)
    }

    /// 取官方令牌（常量级参照表的线性查）。
    pub fn official_token(&self, path: &str) -> Option<&TokenDef> {
        self.official.iter().find(|d| d.path == path)
    }

    /// 取扩展令牌（隔离态不入此表）。
    pub fn ext_token(&self, path: &str) -> Option<&ExtToken> {
        self.active.iter().find(|t| t.path == path)
    }

    /// 查某路径的隔离记录（最后一次立案）。
    pub fn quarantine_of(&self, path: &str) -> Option<&QuarantineRecord> {
        self.quarantine.iter().rev().find(|q| q.path == path)
    }

    // -- 闸链一：命名空间受理 ------------------------------------------------

    /// 受理一个扩展命名空间（语法闸 + 保留词闸 + 容量闸）。
    ///
    /// 幂等：同名同作者重复受理返回 `Ok`（不重复占容量）；
    /// 同名异作者**拒**——冒名接管比撞名更危险。
    pub fn admit(&mut self, id: &str, author: &str, trust: Trust, home: &str) -> Result<(), ExtCode> {
        validate_ns(id)?;
        if author.trim().is_empty() {
            return Err(ExtCode::NsInvalid);
        }
        if let Some(existing) = self.namespace(id) {
            return if existing.author == author {
                Ok(())
            } else {
                Err(ExtCode::NsReserved)
            };
        }
        if self.admitted.len() >= MAX_EXTENSIONS {
            return Err(ExtCode::RegistryFull);
        }
        self.admitted.push(Namespace {
            id: String::from(id),
            author: String::from(author),
            trust,
            home: String::from(home),
        });
        Ok(())
    }

    // -- 闸链二：令牌注册（七闸顺序敏感，判据逐闸对拍）----------------------

    /// 注册一条扩展令牌。闸链顺序：
    ///
    /// 1. **命名空间闸**——未准入 / 语法非法 → `NsInvalid`；
    /// 2. **停权闸**——治理已停权 → `NsSuspended`；
    /// 3. **信任闸**——未验签作者 → `TrustLow`；
    /// 4. **沙箱闸**（W01 对接）——未授权显示与色彩面 → `SandboxDenied`；
    /// 5. **边界闸**——越出本命名空间 / `..` / 空 / 超长 → `PathEscape`；
    /// 6. **律一闸**——缺定义点 / 缺说明 / 定义点重占 → `LawViolation`；
    /// 7. **冲突闸**——撞官方路径 → `OfficialConflict`；抄官方字面量 →
    ///    `ShadowAttempt`；扩展内重名 → `PeerConflict`；
    /// 8. **容量闸**——`RegistryFull`；
    /// 9. **律二闸**——落 `ChangeLedger`，缺主体/理由即拒。
    ///
    /// 越界与违律（`quarantinable()` 为真者）额外写入隔离台账；
    /// 纯冲突不写台账——冲突令牌从未取得存在资格。
    pub fn register(
        &mut self,
        host: &SandboxHost,
        def: &ExtToken,
        actor: &str,
        reason: &str,
        tick: u64,
    ) -> Result<u32, ExtCode> {
        match self.gate(host, def) {
            Ok(()) => {}
            Err(code) => {
                self.record_rejection(def, code, actor, tick);
                return Err(code);
            }
        }
        let target = String::from(&def.path);
        let after = String::from(&def.literal);
        // 律二：落账在最后一步——前面任一闸没过都不该留"变更"痕迹，
        // 因为这次变更从未成立。
        if let Err(_) = self.ledger.commit(ChangeRequest::new(
            actor,
            ChangeAction::Define,
            &target,
            "",
            &after,
            reason,
            tick,
        )) {
            let code = ExtCode::LawViolation;
            self.record_rejection(def, code, actor, tick);
            return Err(code);
        }
        self.active.push(def.clone());
        Ok(self.active.len() as u32)
    }

    /// 七闸本体（`register` 的纯判定部分，便于判据逐闸单测）。
    fn gate(&self, host: &SandboxHost, def: &ExtToken) -> Result<(), ExtCode> {
        // 闸 1：命名空间。
        validate_ns(&def.ns)?;
        let ns = match self.namespace(&def.ns) {
            Some(n) => n,
            None => return Err(ExtCode::NsInvalid),
        };
        // 闸 2：治理停权。
        if self.is_suspended(&def.ns) {
            return Err(ExtCode::NsSuspended);
        }
        // 闸 3：信任等级。
        if !ns.trust.may_activate() {
            return Err(ExtCode::TrustLow);
        }
        // 闸 4：W01 沙箱（扩展要伸手进显示与色彩面，没授权就不给）。
        if !host.can_reach(&def.ns, Facet::DisplayColor) {
            return Err(ExtCode::SandboxDenied);
        }
        // 闸 5：边界。
        self.check_boundary(&def.ns, &def.path)?;
        // 闸 6：律一（单源）——唯一定义点 + 说明齐备，且一个定义点只站一个真值。
        if def.canonical_site.trim().is_empty()
            || def.canonical_site.len() > SITE_MAX
            || def.doc.trim().is_empty()
            || def.literal.trim().is_empty()
        {
            return Err(ExtCode::LawViolation);
        }
        if self
            .active
            .iter()
            .any(|t| t.canonical_site == def.canonical_site && t.path != def.path)
        {
            return Err(ExtCode::LawViolation);
        }
        // 闸 7：冲突（三重）——路径撞官方 / 字面量抄官方 / 扩展内重名。
        if self.official_token(&def.path).is_some() {
            return Err(ExtCode::OfficialConflict);
        }
        if self
            .official
            .iter()
            .any(|d| d.literal == def.literal)
        {
            return Err(ExtCode::ShadowAttempt);
        }
        if self.active.iter().any(|t| t.path == def.path) {
            return Err(ExtCode::PeerConflict);
        }
        // 闸 8：容量。
        if self.active.len() >= MAX_ACTIVE_TOTAL {
            return Err(ExtCode::RegistryFull);
        }
        if self.active_of(&def.ns) >= MAX_TOKENS_PER_EXT {
            return Err(ExtCode::RegistryFull);
        }
        Ok(())
    }

    /// 边界闸：路径须在本命名空间下、无 `..`、非空、不过长。
    pub fn check_boundary(&self, ns: &str, path: &str) -> Result<(), ExtCode> {
        if path.is_empty() || path.len() > PATH_MAX {
            return Err(ExtCode::PathEscape);
        }
        if path.contains("..") {
            return Err(ExtCode::PathEscape);
        }
        let head = format!("{}.", ns);
        if !path.starts_with(&head) {
            return Err(ExtCode::PathEscape);
        }
        if path.len() == head.len() {
            return Err(ExtCode::PathEscape);
        }
        Ok(())
    }

    /// 拒收记账：可隔离者入隔离台账（违律类另计治理立案），其余只计丢弃。
    fn record_rejection(&mut self, def: &ExtToken, code: ExtCode, actor: &str, tick: u64) {
        if !code.quarantinable() {
            return;
        }
        if matches!(code, ExtCode::LawViolation | ExtCode::ShadowAttempt) {
            self.governance = self.governance.saturating_add(1);
        }
        if self.quarantine.len() >= QUARANTINE_CAP {
            self.quarantine_dropped = self.quarantine_dropped.saturating_add(1);
            return;
        }
        self.quarantine.push(QuarantineRecord {
            ns: String::from(&def.ns),
            path: String::from(&def.path),
            reason: code,
            actor: String::from(actor),
            tick,
        });
    }

    // -- 闸链三：解析与优先级仲裁 -------------------------------------------

    /// 优先级仲裁：官方恒定优先。
    ///
    /// 官方声明过的路径恒判 [`Source::Official`]（扩展即便在册也判官方），
    /// 官方没声明而扩展声明了才判 [`Source::Extension`]。
    pub fn arbitrate(&self, path: &str) -> Source {
        if self.official_token(path).is_some() {
            Source::Official
        } else {
            Source::Extension
        }
    }

    /// 解析令牌路径 → 来源（隔离态拒解析）。
    ///
    /// 隔离令牌即便路径在册也拒——隔离的意义就在"看得见但用不得"。
    pub fn resolve(&self, path: &str) -> Result<Source, ExtCode> {
        if self.quarantine_of(path).is_some() {
            return Err(ExtCode::Quarantined);
        }
        if self.official_token(path).is_some() {
            return Ok(Source::Official);
        }
        if self.ext_token(path).is_some() {
            return Ok(Source::Extension);
        }
        Err(ExtCode::NotFound)
    }

    /// 仲裁说明（写死在契约里的那条规则，供 UI/文档引用）。
    pub fn priority_note(&self) -> &'static str {
        PRIORITY_RULE
    }

    // -- 闸链四：治理扫描（O(隔离台账长)）---------------------------------

    /// 命名空间是否已停权（隔离立案累计达 `SUSPEND_THRESHOLD`）。
    pub fn is_suspended(&self, ns: &str) -> bool {
        self.case_of(ns).map(|c| c.suspended).unwrap_or(false)
    }

    /// 取某命名空间的治理立案（无立案 `None`）。
    pub fn case_of(&self, ns: &str) -> Option<GovernanceCase> {
        let violations = self.quarantine.iter().filter(|q| q.ns == ns).count();
        if violations == 0 {
            None
        } else {
            Some(GovernanceCase {
                ns: String::from(ns),
                violations,
                suspended: violations >= SUSPEND_THRESHOLD,
            })
        }
    }

    /// 治理扫描：分桶全部立案并回写停权名单。
    ///
    /// 停权不是"扫描时顺手判一下"——扫描把达阈值的命名空间标进
    /// `suspended`，此后闸 2 直接拦。反复违规的扩展要么整改，要么
    /// 走申诉，没有第三条路。
    pub fn governance_scan(&mut self) -> Vec<GovernanceCase> {
        let mut cases: Vec<GovernanceCase> = Vec::new();
        for n in self.admitted.iter() {
            if let Some(c) = self.case_of(&n.id) {
                cases.push(c);
            }
        }
        // 未准入但留了隔离条目的命名空间也要立案——冒名提交的幽灵令牌
        // 同样得有人治，否则换个大写就能再试一次。
        for q in self.quarantine.iter() {
            if !cases.iter().any(|c| c.ns == q.ns) {
                if let Some(c) = self.case_of(&q.ns) {
                    cases.push(c);
                }
            }
        }
        cases
    }

    /// 治理立案回执：把某命名空间的隔离累计升格为治理案。
    ///
    /// 返回 [`ExtCode::GovernanceOpen`] 与案卷——**治理立案是记账
    /// 不是失败**，所以它不阻断任何注册，只是必须显性出现在读屏清单
    /// 与审计面上。无立案时返回 `None`：不凭空立案。
    pub fn open_case(&self, ns: &str) -> Option<(ExtCode, GovernanceCase)> {
        self.case_of(ns).map(|c| (ExtCode::GovernanceOpen, c))
    }

    // -- 无障碍：清单读屏 ---------------------------------------------------

    /// 扩展令牌清单（逐条读屏行；官方令牌不在此列——清单是扩展的账）。
    pub fn manifest_lines(&self) -> Vec<String> {
        let mut out: Vec<String> = Vec::new();
        for n in self.admitted.iter() {
            out.push(n.screen_line());
        }
        for t in self.active.iter() {
            out.push(t.screen_line());
        }
        for q in self.quarantine.iter() {
            out.push(q.screen_line());
        }
        out
    }

    /// 清单摘要（读屏首行：官方数/扩展数/活跃数/隔离数/治理数）。
    pub fn manifest_spoken(&self) -> String {
        format!(
            "第三方令牌清单：官方令牌 {} 条，扩展命名空间 {} 个，活跃扩展令牌 {} 条，隔离 {} 条（治理立案 {} 次）；优先级：{}；契约 {}",
            self.official_count(),
            self.namespace_count(),
            self.active_count(),
            self.quarantine_count(),
            self.governance_count(),
            PRIORITY_RULE,
            EXT_CONTRACT
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn off(path: &str, lit: &str) -> TokenDef {
        TokenDef {
            path: String::from(path),
            kind_id: 1,
            literal: String::from(lit),
            canonical_site: format!("core/{}.rs:1", path),
            doc: String::from("官方令牌"),
        }
    }

    fn host_with(ns: &str, grant: bool) -> SandboxHost {
        let mut h = SandboxHost::new();
        h.instantiate(ns).unwrap();
        if grant {
            h.grant(ns, Facet::DisplayColor).unwrap();
        }
        h
    }

    fn board(grant: bool) -> (ExtRegistry, SandboxHost) {
        let mut r = ExtRegistry::new(vec![
            off("color.accent.default", "#4c8dff"),
            off("space.md", "16px"),
        ]);
        r.admit("neon", "acme", Trust::Verified, "acme.invalid/neon")
            .unwrap();
        (r, host_with("neon", grant))
    }

    fn tok(path: &str, lit: &str, site: &str) -> ExtToken {
        ExtToken::new("neon", path, lit, 1, site, "扩展说明", 7)
    }

    #[test]
    fn gate_order_and_conflict() {
        let (mut r, h) = board(true);
        assert!(r
            .register(&h, &tok("neon.brand", "#ff00aa", "src/brand.rs:9"), "acme", "扩展新令牌", 7)
            .is_ok());
        assert_eq!(r.active_count(), 1);
        assert_eq!(r.ledger().len(), 1);
        // 撞官方路径：扩展占与官方首段同名的命名空间，路径正好落在官方头上。
        r.admit("color", "acme", Trust::Verified, "acme.invalid/color")
            .unwrap();
        let h_color = host_with("color", true);
        let hit = r.register(
            &h_color,
            &ExtToken::new("color", "color.accent.default", "#123456", 1, "src/a.rs:1", "说明", 7),
            "acme",
            "r",
            7,
        );
        assert_eq!(hit, Err(ExtCode::OfficialConflict));
        assert_eq!(r.active_count(), 1);
        assert_eq!(r.ledger().len(), 1);
        // 字面量抄官方 → 影子（隔离台账 + 治理立案）。
        assert_eq!(
            r.register(&h, &tok("neon.copy", "#4c8dff", "src/c.rs:1"), "acme", "r", 7),
            Err(ExtCode::ShadowAttempt)
        );
        assert_eq!(r.governance_count(), 1);
        assert_eq!(r.quarantine_count(), 1);
        assert_eq!(r.active_count(), 1);
        // 扩展内重名 → 同 ns 同路径二次注册。
        assert_eq!(
            r.register(&h, &tok("neon.brand", "#00ff00", "src/brand2.rs:9"), "acme", "r", 7),
            Err(ExtCode::PeerConflict)
        );
        // 定义点重占（一个定义点站两个真值 = 破坏单源）。
        assert_eq!(
            r.register(&h, &tok("neon.other", "#00ff00", "src/brand.rs:9"), "acme", "r", 7),
            Err(ExtCode::LawViolation)
        );
        assert_eq!(r.active_count(), 1);
        assert_eq!(r.governance_count(), 2);
    }

    #[test]
    fn boundary_quarantine_governance() {
        let (mut r, h) = board(true);
        assert_eq!(
            r.register(&h, &tok("other.x", "#010101", "src/x.rs:1"), "acme", "r", 7),
            Err(ExtCode::PathEscape)
        );
        assert_eq!(
            r.register(
                &h,
                &ExtToken::new("neon", "neon.a...b", "#020202", 1, "src/y.rs:1", "d", 7),
                "acme",
                "r",
                7
            ),
            Err(ExtCode::PathEscape)
        );
        assert_eq!(r.quarantine_count(), 2);
        assert_eq!(r.governance_count(), 0);
        let cases = r.governance_scan();
        assert_eq!(cases.len(), 1);
        assert_eq!(cases[0].violations, 2);
        assert!(!cases[0].suspended);
        // 未授权沙箱 → 隔离，凑满阈值。
        let (_, un) = board(false);
        assert_eq!(
            r.register(&un, &tok("neon.z", "#030303", "src/z.rs:1"), "acme", "r", 7),
            Err(ExtCode::SandboxDenied)
        );
        assert_eq!(r.quarantine_count(), 3);
        let cases2 = r.governance_scan();
        assert!(cases2[0].suspended);
        assert!(r.is_suspended("neon"));
        assert_eq!(
            r.register(&h, &tok("neon.v", "#050505", "src/v.rs:1"), "acme", "r", 7),
            Err(ExtCode::NsSuspended)
        );
        // 停权拒收不入台账（无治理对象，纯拒）。
        assert_eq!(r.quarantine_count(), 3);
    }

    #[test]
    fn arbitration_and_manifest() {
        let (mut r, h) = board(true);
        r.register(&h, &tok("neon.brand", "#ff00aa", "src/brand.rs:9"), "acme", "扩展新令牌", 7)
            .unwrap();
        assert_eq!(r.arbitrate("color.accent.default"), Source::Official);
        assert_eq!(r.arbitrate("neon.brand"), Source::Extension);
        assert_eq!(r.resolve("color.accent.default"), Ok(Source::Official));
        assert_eq!(r.resolve("neon.brand"), Ok(Source::Extension));
        assert_eq!(r.resolve("nothing.here"), Err(ExtCode::NotFound));
        let (_, un) = board(false);
        assert_eq!(
            r.register(&un, &tok("neon.q", "#060606", "src/q.rs:1"), "acme", "r", 7),
            Err(ExtCode::SandboxDenied)
        );
        assert_eq!(r.resolve("neon.q"), Err(ExtCode::Quarantined));
        let s = r.manifest_spoken();
        assert!(s.contains("官方令牌 2 条"));
        assert!(s.contains("隔离 1 条"));
        assert!(s.contains(EXT_CONTRACT));
        assert_eq!(r.manifest_lines().len(), 3);
    }
}