//! VE-F4203 · 契约注册中心（VE-U 域 · 一致性域 · U03 组 · 目标 380 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F4203`
//!
//! **判据（锚点原文）**：单一事实源、五字段冻结、唯一性、引用计数、判据必填、判据。
//!
//! **职责定位（锚点原文）**：契约注册中心（契约单一事实源：注册/检索/引用计数/
//! 生命周期四能力；契约元模型（ID/版本/提供方/消费方/判据五字段冻结）；
//! 注册唯一性断言（同 ID 同版本唯一））。
//!
//! **数据结构（锚点原文·家族格式）**：注册册（元模型五字段）；引用计数表；生命周期机。
//!
//! **错误路径与降级矩阵（锚点原文·家族格式）**：重复注册→拒绝+归并建议；
//! 悬空引用→催办登记；生命周期非法迁移→拒+留痕。
//!
//! **性能逐项分解（锚点原文·家族格式）**：注册 O(1)；检索 O(索引)；断言 O(唯一)。
//!
//! **跨批对接点（锚点原文·家族格式）**：F4201 架构上游；F4221 契约引擎衔接；
//! F4205 变更流程。
//!
//! **无障碍与隐私（锚点原文）**：注册元模型含无障碍判据必填位（域本色）；无隐私面。
//!
//! # 一、单一事实源的真实含义是「**一份契约只能有一个权威条目**」
//!
//! 锚点说本项是「契约单一事实源」。这句话最容易被做成「我这里存了一份，
//! 欢迎大家来抄」——那不叫事实源，那叫**又一份副本**。
//! 事实源的真实含义是反向的：**同一 `ID` + `版本` 在册内只能有一条**，
//! 第二个提供者想注册同键，必须走归并建议而不是覆盖。
//!
//! 为什么必须唯一而不是「后注册覆盖先注册」：覆盖会让**已按旧版编译的下游**
//! 在毫无通知的情况下换掉契约语义。表现是「昨天还好好的，今天行为变了」，
//! 而注册中心日志里只有一条平常的注册记录——查不出来。
//! 故 [`ContractRegistry::register`] 对同键第二次注册恒
//! [`E_CONTRACT_DUP`]，并给出**归并建议**（锚点降级矩阵第一格明文要求）。
//!
//! # 二、版本是元模型的一部分，不是元模型之外的装饰
//!
//! 五字段里 `版本` 与 `ID` 并列，不是 `ID` 的附属。这有一个直接后果：
//! **唯一性的键是 `(ID, 版本)` 而不是 `ID`**。
//!
//! 若唯一性只按 `ID` 判，则版本一升就必然重复 → 版本机制形同虚设；
//! 若唯一性只按 `(ID, 版本)` 判而不同步管「同一 ID 的多个版本共存」，
//! 则会出现两条 `V1-a` 与 `V1-b` 同时在册而无人知道该用哪个。
//! 故本项同时管两件事：[`ContractRegistry::register`] 管**键内唯一**，
//! [`Self::check_version_lineage`] 管**版本谱系唯一**（同一 ID 下不得有两个
//! 同号版本），二者缺一不可。
//!
//! # 三、引用计数为什么不能用「谁在用自己查」（反向索引的必要性）
//!
//! 消费方登记的是「我引用了 X」，但**没人会主动注销**——需求变了、模块被删了，
//! 引用方的生命周期与被引用契约完全解耦。所以引用数只能由**注册中心记账**，
//! 而且必须**在册内可查**（`consumers()`），否则「这个契约还有人用吗」
//! 只能靠全量扫所有模块的声明——而那些声明并不在本项管辖内。
//!
//! 由此推出**撤订（deregister）比注册更危险**：注册错了加一条，撤订错了
//! **计数归零会让仍在用的契约被判死**。故撤订走
//! [`Self::deregister`]，且对**仍有其他消费方**的契约直接拒绝
//! （[`E_DEREG_IN_USE`]），只允许**引用归零后**才真正除册。
//!
//! # 四、生命周期机的非法迁移为什么必须「拒 + 留痕」
//!
//! 生命周期四态：`草拟 → 已注册 → 已冻结 → 已废止`（单向不可回退，
//! 废止为终态）。锚点要求「非法迁移→拒+留痕」。
//!
//! 「留痕」不是可选项：被拒的迁移如果只报错不留痕，那么**反复重试同一个
//! 非法迁移**的调用方看起来就像「没报错但也没生效」——它会一直重试。
//! 留痕（[`TransitionTrace`]）让「谁在什么时候试图做什么非法迁移」可反查，
//! 这在契约治理里是审计要求，不是日志噪音。
//!
//! # 五、判据必填位为什么是**注册时**强制，而不是使用时才查
//!
//! 锚点：「注册元模型含无障碍判据必填位（域本色）」。
//! 若判据缺失留到「规则引擎跑起来才发现」，那么无障碍判据的缺省会
//! **在整个系统跑起来之后**才暴露，而那时补判据要动的是所有消费方。
//! 故 [`Self::register`] 入口即拒空判据——**元模型一旦放宽就再也收不回来**，
//! 这是注册中心作为入口的唯一优势所在，不用它就是浪费。

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

use super::veu01_arch::{fnv1a64_hex, ConsistencyError};
use super::veu02_model::DomainTag;

/// 契约元模型五字段（锚点明文：**ID / 版本 / 提供方 / 消费方 / 判据**）。
///
/// 五字段**冻结**：不多不少。少一项（比如不记提供方）就无法追责，
/// 多一项（比如塞个「备注」）则会给「同一契约不同处写得不一样」留下后门——
/// 备注不进哈希，于是两份同名契约能带着不同备注并存，唯一性断言却判它们相同。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ContractMeta {
    /// 契约 ID（跨域引用凭据）。
    pub id: String,
    /// 版本（与 ID 并列，**唯一性键的一半**）。
    pub version: String,
    /// 提供方（**单一**，谁立的约）。
    pub provider: DomainTag,
    /// 消费方（**可多个**，谁在用）。
    pub consumers: Vec<DomainTag>,
    /// 判据正文（**必填**，无障碍判据在此）。
    pub criteria: String,
}

/// 元模型五字段的冻结清单（`E_FIELD_*` 与本表一一对应）。
pub const META_FIELDS: [&str; 5] = ["id", "version", "provider", "consumers", "criteria"];

impl ContractMeta {
    /// 新建契约元模型（消费方可先空）。
    pub fn new(
        id: &str,
        version: &str,
        provider: DomainTag,
        consumers: Vec<DomainTag>,
        criteria: &str,
    ) -> ContractMeta {
        ContractMeta {
            id: id.to_string(),
            version: version.to_string(),
            provider,
            consumers,
            criteria: criteria.to_string(),
        }
    }

    /// 五字段齐备性（**判据必填**在此强制，见头注§五）。
    pub fn is_complete(&self) -> bool {
        !self.id.trim().is_empty()
            && !self.version.trim().is_empty()
            && !self.criteria.trim().is_empty()
            && !self.consumers.is_empty()
    }

    /// 缺失字段清单（**指名到字段**，不只说「不完整」）。
    pub fn missing_fields(&self) -> Vec<&'static str> {
        let mut out: Vec<&'static str> = Vec::new();
        if self.id.trim().is_empty() {
            out.push("id");
        }
        if self.version.trim().is_empty() {
            out.push("version");
        }
        if self.consumers.is_empty() {
            out.push("consumers");
        }
        if self.criteria.trim().is_empty() {
            out.push("criteria");
        }
        out
    }

    /// 元模型指纹（**只取五字段，不取消费方顺序**——消费方集合无序，
    /// 顺序不同不算两条契约）。
    pub fn digest(&self) -> String {
        let mut cons = self.consumers.clone();
        cons.sort();
        let joined = format!(
            "{}|{}|{:?}|{:?}|{}",
            self.id,
            self.version,
            self.provider,
            cons,
            self.criteria
        );
        fnv1a64_hex(joined.as_bytes())
    }

    /// 读屏单行。
    pub fn screen_line(&self) -> String {
        let cons: Vec<&str> = self.consumers.iter().map(|d| d.zh()).collect();
        format!(
            "契约 {} @{} 由{} 提供，消费方 {} 家（{}），判据 {} 字",
            self.id,
            self.version,
            self.provider.zh(),
            self.consumers.len(),
            cons.join("/"),
            self.criteria.chars().count()
        )
    }
}

/// 契约生命周期四态（锚点：生命周期机；**单向不可回退**）。
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Lifecycle {
    /// 草拟（尚未注册进册）。
    Draft,
    /// 已注册（可被检索与引用）。
    Registered,
    /// 已冻结（判据不可再改——改须走 F4205 变更流程）。
    Frozen,
    /// 已废止（终态，不可复活）。
    Retired,
}

impl Lifecycle {
    /// 全部四态（顺序即迁移序，便于表驱动）。
    pub const ALL: [Lifecycle; 4] = [
        Lifecycle::Draft,
        Lifecycle::Registered,
        Lifecycle::Frozen,
        Lifecycle::Retired,
    ];

    /// 中文名。
    pub fn zh(self) -> &'static str {
        match self {
            Lifecycle::Draft => "草拟",
            Lifecycle::Registered => "已注册",
            Lifecycle::Frozen => "已冻结",
            Lifecycle::Retired => "已废止",
        }
    }

    /// 码。
    pub fn code(self) -> &'static str {
        match self {
            Lifecycle::Draft => "LC-DRAFT",
            Lifecycle::Registered => "LC-REGISTERED",
            Lifecycle::Frozen => "LC-FROZEN",
            Lifecycle::Retired => "LC-RETIRED",
        }
    }

    /// 由码反查（往返一致）。
    pub fn from_code(code: &str) -> Option<Lifecycle> {
        Lifecycle::ALL.iter().copied().find(|l| l.code() == code)
    }

    /// 合法迁移表（**单向阶梯 + 一条提前废止**）。
    ///
    /// 允许的只有：草拟→已注册、已注册→已冻结、已注册→已废止、
    /// 已冻结→已废止。**草拟→已冻结被拒**——没注册过就冻，等于
    /// 凭空出现一份没人登记过的「已冻结契约」，绕过注册中心这个入口；
    /// **任何回退被拒**——单向前进是为了让「上一个状态是什么」永远可推断。
    pub fn may_move_to(self, next: Lifecycle) -> bool {
        matches!(
            (self, next),
            (Lifecycle::Draft, Lifecycle::Registered)
                | (Lifecycle::Registered, Lifecycle::Frozen)
                | (Lifecycle::Registered, Lifecycle::Retired)
                | (Lifecycle::Frozen, Lifecycle::Retired)
        )
    }
}

/// 生命周期迁移留痕（锚点「拒 + 留痕」的落点）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TransitionTrace {
    /// 契约 ID。
    pub id: String,
    /// 尝试的源态。
    pub from: Lifecycle,
    /// 尝试的目标态。
    pub to: Lifecycle,
    /// 是否被拒（**留痕含被拒项**，见头注§四）。
    pub rejected: bool,
    /// 原因（被拒时指名依据）。
    pub reason: String,
}

impl TransitionTrace {
    /// 读屏单行。
    pub fn screen_line(&self) -> String {
        format!(
            "迁移留痕 {}：{} → {}（{}）{}",
            self.id,
            self.from.zh(),
            self.to.zh(),
            if self.rejected { "已拒" } else { "已准" },
            self.reason
        )
    }
}

/// 引用记录（引用计数表的一行；**撤订留痕不删行**——删了就成了没发生过）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RefRecord {
    /// 契约 ID。
    pub id: String,
    /// 版本。
    pub version: String,
    /// 引用方。
    pub consumer: DomainTag,
    /// 是否已撤订（**只标记不删除**，保证「曾引用过」可反查）。
    pub revoked: bool,
}

/// 催办项（锚点「悬空引用→催办登记」）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DunningItem {
    /// 悬空的契约 ID。
    pub id: String,
    /// 引用方。
    pub consumer: DomainTag,
    /// 催办次数（**重复悬空要累加**，不覆盖——覆盖会让「催了没人管」不可见）。
    pub notices: u32,
}

impl DunningItem {
    /// 读屏单行。
    pub fn screen_line(&self) -> String {
        format!(
            "催办 {}：{} 引用了不在册的契约，已催 {} 次",
            self.id,
            self.consumer.zh(),
            self.notices
        )
    }
}

/// 错误码。
pub const E_CONTRACT_DUP: &str = "E_CONTRACT_DUP";
pub const E_CONTRACT_UNKNOWN: &str = "E_CONTRACT_UNKNOWN";
pub const E_META_INCOMPLETE: &str = "E_META_INCOMPLETE";
pub const E_META_CRITERIA_EMPTY: &str = "E_META_CRITERIA_EMPTY";
pub const E_META_FIELDS_DRIFT: &str = "E_META_FIELDS_DRIFT";
pub const E_REF_UNKNOWN: &str = "E_REF_UNKNOWN";
pub const E_REF_ALREADY: &str = "E_REF_ALREADY";
pub const E_DEREG_IN_USE: &str = "E_DEREG_IN_USE";
pub const E_LC_ILLEGAL: &str = "E_LC_ILLEGAL";
pub const E_LC_UNKNOWN: &str = "E_LC_UNKNOWN";
pub const E_VERSION_LINEAGE_DUP: &str = "E_VERSION_LINEAGE_DUP";
pub const E_CAP: &str = "E_CAP";

/// 容量上限（定长内分配）。
pub const MAX_CONTRACTS: usize = 64;
pub const MAX_REFS: usize = 128;
pub const MAX_TRACES: usize = 64;

/// 注册册条目（注册册的一行：元模型 + 生命周期 + 引用计数）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RegistryEntry {
    /// 元模型五字段。
    pub meta: ContractMeta,
    /// 生命周期态。
    pub lifecycle: Lifecycle,
    /// 有效引用数（**只由注册中心记账**，见头注§三）。
    pub ref_count: usize,
}

/// 契约注册中心（四能力：注册 / 检索 / 引用计数 / 生命周期）。
#[derive(Clone, Debug)]
pub struct ContractRegistry {
    entries: Vec<RegistryEntry>,
    refs: Vec<RefRecord>,
    traces: Vec<TransitionTrace>,
    dunning: Vec<DunningItem>,
}

impl ContractRegistry {
    /// 空册。
    pub fn new() -> ContractRegistry {
        ContractRegistry {
            entries: Vec::new(),
            refs: Vec::new(),
            traces: Vec::new(),
            dunning: Vec::new(),
        }
    }

    /// 册内条目只读遍历。
    pub fn entries(&self) -> &[RegistryEntry] {
        &self.entries
    }

    /// 引用记录只读遍历。
    pub fn refs(&self) -> &[RefRecord] {
        &self.refs
    }

    /// 迁移留痕只读遍历（**含被拒项**）。
    pub fn traces(&self) -> &[TransitionTrace] {
        &self.traces
    }

    /// 催办项只读遍历。
    pub fn dunning(&self) -> &[DunningItem] {
        &self.dunning
    }

    /// **能力一·注册**（O(1)：线性小表比对）。
    ///
    /// 拒重复（同 ID 同版本唯一，见头注§一），并给**归并建议**。
    ///
    /// 返回 `Result<(), _>` 而非 `Result<&RegistryEntry, _>`：**不外借册内条目**。
    /// 一旦返回内部引用，调用方就只能在持有该引用的前提下读册，
    /// 而下一个 `register`/`add_consumer` 又要 `&mut` —— 借用冲突会把
    /// 「注册完立刻查一下」这种最自然的写法逼成先 `.clone()` 再查。
    /// 需要读条目请用 [`Self::lookup`]（要 `&self`，不冲突）。
    pub fn register(&mut self, meta: ContractMeta) -> Result<(), ConsistencyError> {
        // 判据必填位在**入口**强制（头注§五）。
        if meta.criteria.trim().is_empty() {
            return Err(ConsistencyError::new(
                E_META_CRITERIA_EMPTY,
                "契约注册被拒：判据正文为空",
                &format!(
                    "契约 {} @{} 的判据为空；元模型五字段之判据位必填",
                    meta.id, meta.version
                ),
                &format!(
                    "补判据正文后重试；无障碍判据不可省（域本色必填位），\
                     参考句式：「本契约保证 {} 行为可被 {} 验证」",
                    meta.id,
                    meta.provider.zh()
                ),
                "契约提供方",
            ));
        }
        if !meta.is_complete() {
            let missing = meta.missing_fields().join("/");
            return Err(ConsistencyError::new(
                E_META_INCOMPLETE,
                "契约注册被拒：元模型五字段残缺",
                &format!("契约 {} @{} 缺字段：{}", meta.id, meta.version, missing),
                "五字段为冻结项（ID/版本/提供方/消费方/判据），缺一不得入册",
                "契约提供方",
            ));
        }
        if self.entries.len() >= MAX_CONTRACTS {
            return Err(ConsistencyError::new(
                E_CAP,
                "契约注册被拒：达到容量上限",
                &format!("注册册 {} 条达到上限 {}", self.entries.len(), MAX_CONTRACTS),
                "先废止并除册无用契约，或拆册",
                "契约注册中心维护方",
            ));
        }
        // 键内唯一：`(ID, 版本)`。
        if let Some(old) = self
            .entries
            .iter()
            .find(|e| e.meta.id == meta.id && e.meta.version == meta.version)
        {
            return Err(ConsistencyError::new(
                E_CONTRACT_DUP,
                "契约注册被拒：同 ID 同版本已存在",
                &format!(
                    "契约 {} @{} 已在册（提供方 {}，指纹 {}），新来提供方 {}，指纹 {}",
                    meta.id,
                    meta.version,
                    old.meta.provider.zh(),
                    old.meta.digest(),
                    meta.provider.zh(),
                    meta.digest()
                ),
                // 降级矩阵第一格：**归并建议**，不许覆盖。
                &format!(
                    "归并建议：① 若语义相同，合并判据后由原提供方 {} 升版；\
                     ② 若语义不同，改用新版本号；\
                     ③ 若只是消费方不同，直接 add_consumer({})，勿重注册",
                    old.meta.provider.zh(),
                    meta.id
                ),
                "契约提供方",
            ));
        }
        // 版本谱系唯一：同一 ID 下不得有两个**自环**重复版本（头注§二）。
        self.check_version_lineage(&meta)?;
        self.entries.push(RegistryEntry {
            meta,
            lifecycle: Lifecycle::Registered,
            ref_count: 0,
        });
        Ok(())
    }

    /// **能力二·检索**（O(索引)：先按 ID 定位再按版本筛）。
    pub fn lookup(&self, id: &str, version: &str) -> Option<&RegistryEntry> {
        self.entries
            .iter()
            .find(|e| e.meta.id == id && e.meta.version == version)
    }

    /// 按 ID 取该契约的全部版本（**当前版本**取最大版本号者）。
    pub fn versions_of(&self, id: &str) -> Vec<String> {
        let mut vs: Vec<String> = self
            .entries
            .iter()
            .filter(|e| e.meta.id == id)
            .map(|e| e.meta.version.clone())
            .collect();
        vs.sort();
        vs
    }

    /// 当前版本条目（取版本号最大者）。
    pub fn current(&self, id: &str) -> Option<&RegistryEntry> {
        self.entries
            .iter()
            .filter(|e| e.meta.id == id)
            .max_by(|a, b| a.meta.version.cmp(&b.meta.version))
    }

    /// 版本谱系检查（**同一 ID 下版本号不得重复**）。
    pub fn check_version_lineage(&self, meta: &ContractMeta) -> Result<(), ConsistencyError> {
        let same = self
            .entries
            .iter()
            .filter(|e| e.meta.id == meta.id && e.meta.version == meta.version)
            .count();
        if same > 0 {
            return Err(ConsistencyError::new(
                E_VERSION_LINEAGE_DUP,
                "版本谱系冲突：同一 ID 同一版本已存在",
                &format!("契约 {} @{} 在册 {} 条", meta.id, meta.version, same),
                "键内唯一由 register 拦；此处为纵深防御，改判据见 E_VERSION_LINEAGE_DUP",
                "契约注册中心维护方",
            ));
        }
        Ok(())
    }

    /// **能力三·引用计数**——加引用（消费方登记）。
    ///
    /// 引用方不在契约声明的消费方列表内时，**先催办**（悬空引用→
    /// 催办登记，降级矩阵第二格），但仍**拒绝**：未声明的消费者
    /// 先要补声明，不能靠引用悄悄把自己加进来。
    pub fn add_consumer(
        &mut self,
        id: &str,
        version: &str,
        consumer: DomainTag,
    ) -> Result<usize, ConsistencyError> {
        let Some(e) = self
            .entries
            .iter_mut()
            .find(|e| e.meta.id == id && e.meta.version == version)
        else {
            return Err(ConsistencyError::new(
                E_CONTRACT_UNKNOWN,
                "引用登记被拒：契约不在册",
                &format!("契约 {} @{} 未注册", id, version),
                "先注册契约（register），再登记引用",
                "引用方",
            ));
        };
        // 已引用过 → 拒绝重复计数（否则计数虚高，永不归零）。
        if self
            .refs
            .iter()
            .any(|r| r.id == id && r.version == version && r.consumer == consumer && !r.revoked)
        {
            return Err(ConsistencyError::new(
                E_REF_ALREADY,
                "引用登记被拒：已引用过",
                &format!("{} 已在引用 {} @{}", consumer.zh(), id, version),
                "同一消费方对同一契约只能有一条有效引用；重复登记会让计数永不归零",
                "引用方",
            ));
        }
        if !e.meta.consumers.contains(&consumer) {
            let provider = e.meta.provider;
            self.notice_dunning(id, consumer);
            return Err(ConsistencyError::new(
                E_REF_UNKNOWN,
                "引用登记被拒：消费方未在元模型声明",
                &format!(
                    "{} 未被 {} 列为消费方（已登记催办）",
                    consumer.zh(),
                    provider.zh()
                ),
                &format!(
                    "请 {} 先把 {} 加入元模型消费方位，再引用；\
                     本项不代改元模型——单一事实源的变更须由提供方发起的变更流程走",
                    provider.zh(),
                    consumer.zh()
                ),
                consumer.zh(),
            ));
        }
        e.ref_count += 1;
        if self.refs.len() >= MAX_REFS {
            return Err(ConsistencyError::new(
                E_CAP,
                "引用登记被拒：达到容量上限",
                &format!("引用表 {} 条达到上限 {}", self.refs.len(), MAX_REFS),
                "先撤订已失效引用",
                "契约注册中心维护方",
            ));
        }
        self.refs.push(RefRecord {
            id: id.to_string(),
            version: version.to_string(),
            consumer,
            revoked: false,
        });
        Ok(e.ref_count)
    }

    /// 撤订（**只标记不删行**，见头注§三）。
    pub fn deregister(
        &mut self,
        id: &str,
        version: &str,
        consumer: DomainTag,
    ) -> Result<usize, ConsistencyError> {
        let Some(rec) = self.refs.iter_mut().find(|r| {
            r.id == id && r.version == version && r.consumer == consumer && !r.revoked
        }) else {
            return Err(ConsistencyError::new(
                E_REF_UNKNOWN,
                "撤订被拒：无此有效引用",
                &format!("{} 对 {} @{} 无有效引用", consumer.zh(), id, version),
                "查 refs() 确认该消费方是否真引用过（撤订记录只标记不删行）",
                "引用方",
            ));
        };
        rec.revoked = true;
        if let Some(e) = self
            .entries
            .iter_mut()
            .find(|e| e.meta.id == id && e.meta.version == version)
        {
            e.ref_count = e.ref_count.saturating_sub(1);
            Ok(e.ref_count)
        } else {
            Ok(0)
        }
    }

    /// 引用方名单（**有效**引用，在册内可查——见头注§三）。
    pub fn consumers(&self, id: &str, version: &str) -> Vec<DomainTag> {
        self.refs
            .iter()
            .filter(|r| r.id == id && r.version == version && !r.revoked)
            .map(|r| r.consumer)
            .collect()
    }

    /// 引用计数（有效数）。
    pub fn ref_count(&self, id: &str, version: &str) -> usize {
        self.refs
            .iter()
            .filter(|r| r.id == id && r.version == version && !r.revoked)
            .count()
    }

    /// 催办登记（**次数累加不覆盖**，见[`DunningItem::notices`]）。
    fn notice_dunning(&mut self, id: &str, consumer: DomainTag) {
        if let Some(d) = self
            .dunning
            .iter_mut()
            .find(|d| d.id == id && d.consumer == consumer)
        {
            d.notices = d.notices.saturating_add(1);
            return;
        }
        self.dunning.push(DunningItem { id: id.to_string(), consumer, notices: 1 });
    }

    /// **能力四·生命周期**——迁移（合法则进，非法则拒 + 留痕，头注§四）。
    pub fn transition(
        &mut self,
        id: &str,
        to: Lifecycle,
        reason: &str,
    ) -> Result<Lifecycle, ConsistencyError> {
        let Some(e) = self.entries.iter_mut().find(|e| e.meta.id == id) else {
            return Err(ConsistencyError::new(
                E_CONTRACT_UNKNOWN,
                "生命周期迁移被拒：契约不在册",
                &format!("契约 {} 未注册，无生命周期可迁", id),
                "先 register 再迁生命周期",
                "生命周期驱动方",
            ));
        };
        let from = e.lifecycle;
        if !from.may_move_to(to) {
            let trace = TransitionTrace {
                id: id.to_string(),
                from,
                to,
                rejected: true,
                reason: format!(
                    "非法迁移 {} → {}；合法迁移表为「草拟→已注册、已注册→已冻结/已废止、已冻结→已废止」",
                    from.zh(),
                    to.zh()
                ),
            };
            self.push_trace(trace);
            return Err(ConsistencyError::new(
                E_LC_ILLEGAL,
                "生命周期迁移被拒：非法迁移",
                &format!(
                    "契约 {} 当前 {}，试图迁到 {}",
                    id,
                    from.zh(),
                    to.zh()
                ),
                "按合法迁移表前进；若确需回退或跳级，走 F4205 变更流程并新开版本",
                "生命周期驱动方",
            ));
        }
        // 废止前须引用归零（**仍在用的契约不能被废止**——否则消费方第二天才发现）。
        if to == Lifecycle::Retired && e.ref_count > 0 {
            let n = e.ref_count;
            let trace = TransitionTrace {
                id: id.to_string(),
                from,
                to,
                rejected: true,
                reason: format!("废止被拒：仍有 {} 条有效引用未撤订", n),
            };
            self.push_trace(trace);
            return Err(ConsistencyError::new(
                E_LC_ILLEGAL,
                "生命周期迁移被拒：仍有有效引用",
                &format!("契约 {} 被废止前引用数为 {}", id, n),
                &format!(
                    "先让 {} 家消费方撤订（引用计数归零）再废止；\
                     否则消费方会在不知情下失去契约",
                    n
                ),
                "生命周期驱动方",
            ));
        }
        e.lifecycle = to;
        self.push_trace(TransitionTrace {
            id: id.to_string(),
            from,
            to,
            rejected: false,
            reason: reason.to_string(),
        });
        Ok(to)
    }

    /// 迁移留痕入册（满则覆写最旧一条——留痕表是审计面，但不可无界增长）。
    fn push_trace(&mut self, t: TransitionTrace) {
        if self.traces.len() >= MAX_TRACES {
            self.traces.remove(0);
        }
        self.traces.push(t);
    }

    /// 元模型五字段冻结自检（**字段清单不得漂移**）。
    pub fn fields_frozen() -> bool {
        META_FIELDS.len() == 5 && META_FIELDS[0] == "id" && META_FIELDS[4] == "criteria"
    }

    /// 注册中心自检（**唯一性 + 谱系 + 计数自洽 + 留痕在册**）。
    pub fn self_audit(&self) -> Vec<String> {
        let mut issues: Vec<String> = Vec::new();
        if !Self::fields_frozen() {
            issues.push(format!("{}:五字段清单漂移", E_META_FIELDS_DRIFT));
        }
        // 键内唯一：两两比对。
        for i in 0..self.entries.len() {
            for j in (i + 1)..self.entries.len() {
                let a = &self.entries[i];
                let b = &self.entries[j];
                if a.meta.id == b.meta.id && a.meta.version == b.meta.version {
                    issues.push(format!(
                        "{}:{} @{} 重复入册",
                        E_CONTRACT_DUP,
                        a.meta.id,
                        a.meta.version
                    ));
                }
            }
        }
        // 计数自洽：条目的 ref_count 须等于引用表里的有效条数。
        for e in self.entries.iter() {
            let real = self.ref_count(&e.meta.id, &e.meta.version);
            if real != e.ref_count {
                issues.push(format!(
                    "{}:{} @{} 引用计数 {} 与引用表实际 {} 不符",
                    E_REF_ALREADY,
                    e.meta.id,
                    e.meta.version,
                    e.ref_count,
                    real
                ));
            }
        }
        issues
    }

    /// 读屏总览。
    pub fn screen_text(&self) -> String {
        let mut s = String::new();
        s.push_str(&format!(
            "契约注册册：{} 条契约、{} 条引用、{} 条迁移留痕、{} 项催办\n",
            self.entries.len(),
            self.refs.len(),
            self.traces.len(),
            self.dunning.len()
        ));
        for e in self.entries.iter() {
            s.push_str(&format!(
                "{}  引用 {}  {}\n",
                e.meta.screen_line(),
                e.ref_count,
                e.lifecycle.zh()
            ));
        }
        for d in self.dunning.iter() {
            s.push_str(&d.screen_line());
            s.push('\n');
        }
        s
    }
}

/// 标准注册册（**四能力均有实测对象的正样本**，兼作回归基线）。
///
/// 构造两条契约：一条已冻结且有两条引用（供撤订/废止闸有对象），
/// 一条已注册但无引用（供「引用归零才可废止」有对照组）。
pub fn standard_registry() -> ContractRegistry {
    let mut r = ContractRegistry::new();
    r.register(ContractMeta::new(
        "U03-CTR-FOCUS",
        "v1",
        DomainTag::S,
        vec![DomainTag::T, DomainTag::U],
        "焦点顺序跨域同源；对比度不低于 4.5:1 且焦点可达，缺一不得发布",
    ))
    .expect("标准契约注册");
    r.register(ContractMeta::new(
        "U03-CTR-THEME",
        "v1",
        DomainTag::T,
        vec![DomainTag::U],
        "主题字段取值 light|dark|auto；切换后焦点不丢失",
    ))
    .expect("标准契约注册");

    r.add_consumer("U03-CTR-FOCUS", "v1", DomainTag::T)
        .expect("引用登记");
    r.add_consumer("U03-CTR-FOCUS", "v1", DomainTag::U)
        .expect("引用登记");
    r.add_consumer("U03-CTR-THEME", "v1", DomainTag::U)
        .expect("引用登记");

    r.transition("U03-CTR-FOCUS", Lifecycle::Frozen, "判据齐备，冻结")
        .expect("冻结迁移");
    r
}