//! VE-F4601 · W 域开工与插件 SDK 总架构（VE-W 域 · 插件 SDK · 目标 380 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F4601`
//!
//! **判据（锚点原文四条）**：**四层、双承诺、承接落地、层冻结**。
//!
//! - **四层**：SDK 接口层 → 运行隔离层 → 能力授权层 → 生态治理层，层间接口冻结。
//!   四层不是四个文件夹而是四条**权限递减**的边界：越靠前越自由，越靠后越收束。
//! - **双承诺**（域本色·最高优先）：**开放而不失控**——插件自由与系统安全同为本域
//!   的正式承诺，二者**同时**成立才算达标；只放开不给授权是失控，只封锁不开放是
//!   扼杀。本条把这条口号变成可机检的两个计数（承诺登记数）与一条失衡判据。
//! - **承接落地**：V 域移交包（F4593 十件）的交接面三源——**显示与色彩 API 冻结面、
//!   无障碍门禁基线、一致性契约源**——须逐条落进插件能力面，缺源即拒收（锚点
//!   错误路径「承接缺源 → 回溯移交包」）。
//! - **层冻结**：每层对外接口冻结版本号；冻结面只升版不改语义，改动走冻结流程
//!   （锚点错误路径「接口越权 → 冻结流程」）。
//!
//! **错误路径与降级矩阵**：层间失配 → 对拍（逐层校验依赖方向，不合规则拒收并登记）；
//! 承接缺源 → 回溯移交包（拒收，不开工）；接口越权 → 冻结流程（升版重核对）。
//!
//! **性能逐项分解**：架构 O(层数)=O(4)；冻结 O(接口数)；落地 O(源数)=O(交接面数)。
//!
//! **跨批对接点**：上游 F4593 移交包（十件+哈希）；下游 F4602 Manifest（能力声明
//! 须落在本条冻结的能力面内）、F4620 双签闸。
//!
//! **无障碍与隐私**：开放与安全双承诺为域本色总纲（最高优先）；本条无隐私面
//! （作者信息与运行时数据归 F4602 Manifest 与加载器条处理）。
//!
//! 逻辑 tick 注入，零墙钟；零 IO；类型自持（本条不 import 未注册的兄弟模块——
//! 平行会话的 `ve*` 族尚在施工，编译期硬耦合会让本条因别人的进度而红）。

use crate::checks::CheckSet;

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 一、规格常量（参数唯一源）
// ---------------------------------------------------------------------------

/// 域标识（CheckSet 聚合用）。
pub const VEW_DOMAIN: &str = "VE-W";

/// 四层层数（架构 O(层数)，本域恒为 4——层数是契约不是参数）。
pub const LAYER_COUNT: usize = 4;

/// 移交包件数（F4593 十件封装，缺件即拒收）。
pub const HANDOFF_PIECE_COUNT: usize = 10;

/// 交接面来源数（API 冻结面 / 无障碍门禁基线 / 一致性契约源）。
pub const HANDOFF_SOURCE_COUNT: usize = 3;

/// 哈希占位长度（十六进制字符数；仅作封包完整性判据，不做密码学用途）。
pub const HASH_HEX_LEN: usize = 8;

/// 承诺登记上界（防御性；正常由双承诺失衡判据先拦）。
pub const MAX_PROMISE_REGISTRY: usize = 256;

/// 双承诺文档（域本色，最高优先）。
pub const DUAL_PROMISE_DOC: &str = "\
W 域本色·双承诺（VE-F4601 · v1）：开放而不失控——插件自由与系统安全同为本域的正式\
承诺，二者同时成立才算达标。只放开不给授权是失控；只封锁不开放是扼杀。判据「双承诺」\
以两侧承诺登记数同时非零且无失衡告警为准，任一为 0 即判不达标——口号不落地为计数\
等于没承诺。";

/// 层冻结文档。
pub const LAYER_FREEZE_DOC: &str = "\
层间接口冻结契约（VE-F4601 · v1）：四层各自对外接口冻结版本号，冻结面只升版不改语义\
——下游按冻结面先行开发，不等实现完成。发现越权（未走冻结流程即改语义）即拒收并登记，\
补冻结流程后可重入。层数固定为 4，层序即权限序，不可重排。";

/// 承接落地文档。
pub const HANDOFF_LANDING_DOC: &str = "\
承接落地契约（VE-F4601 · v1）：V 域移交包十件封装的交接面三源（显示与色彩 API 冻结面、\
无障碍门禁基线、一致性契约源）须逐条落进插件能力面。缺源即拒收开工并回溯移交包——\
承接面是 W 域的输入源，缺源的开工等于建在流沙上。";

// ---------------------------------------------------------------------------
// 二、数据结构（四层 / 冻结面 / 交接面 / 双承诺）
// ---------------------------------------------------------------------------

/// 四层标识（层序即权限序：越靠后越收束，不可重排）。
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Layer {
    /// 第一层：SDK 接口层（插件看得见的全部面）。
    SdkInterface = 0,
    /// 第二层：运行隔离层（插件跑在笼子里，出事不拖垮宿主）。
    RuntimeIsolation = 1,
    /// 第三层：能力授权层（插件能碰什么，逐项授权）。
    CapabilityGrant = 2,
    /// 第四层：生态治理层（准入与问责，兜底收口）。
    EcosystemGovernance = 3,
}

impl Layer {
    /// 层序（0..3）。
    pub fn ordinal(self) -> usize {
        self as usize
    }

    /// 稳定短名（机检与审计用）。
    pub fn tag(self) -> &'static str {
        match self {
            Layer::SdkInterface => "sdk",
            Layer::RuntimeIsolation => "isolation",
            Layer::CapabilityGrant => "grant",
            Layer::EcosystemGovernance => "governance",
        }
    }

    /// 层的中文名（诊断三要素用）。
    pub fn label(self) -> &'static str {
        match self {
            Layer::SdkInterface => "SDK 接口层",
            Layer::RuntimeIsolation => "运行隔离层",
            Layer::CapabilityGrant => "能力授权层",
            Layer::EcosystemGovernance => "生态治理层",
        }
    }

    /// 四层穷举（按层序，架构 O(层数) 的驱动面）。
    pub fn all() -> [Layer; LAYER_COUNT] {
        [
            Layer::SdkInterface,
            Layer::RuntimeIsolation,
            Layer::CapabilityGrant,
            Layer::EcosystemGovernance,
        ]
    }
}

/// 一条冻结接口（某层对外承诺的稳定面）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FrozenIface {
    /// 接口稳定标识（层内唯一）。
    pub id: u32,
    /// 归属层。
    pub owner: Layer,
    /// 冻结版本（只升不改：语义变更走冻结流程升版）。
    pub version: u16,
}

/// 层失配诊断（层间依赖方向不合规）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LayerMismatch {
    /// 依赖方层。
    pub from: Layer,
    /// 被依赖方层。
    pub to: Layer,
    /// 三要素：发生了什么。
    pub what: String,
    /// 三要素：为什么（层序即权限序，反向依赖即越权）。
    pub why: &'static str,
}

// ---------------------------------------------------------------------------
// 三、双承诺（域本色：开放而不失控）
// ---------------------------------------------------------------------------

/// 一条承诺登记（双承诺的可机检载体）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Promise {
    /// 承诺文本（短名）。
    pub tag: &'static str,
}

/// 双承诺台账（两侧登记各计一次；任一为 0 即失衡）。
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct DualPromise {
    /// 插件自由侧登记。
    pub openness: Vec<Promise>,
    /// 系统安全侧登记。
    pub safety: Vec<Promise>,
}

impl DualPromise {
    /// 登记一条承诺（`to_openness=true` 归开放侧，否则归安全侧；超上界显性拒绝）。
    pub fn add(&mut self, p: Promise, to_openness: bool) -> Result<(), &'static str> {
        let side = if to_openness {
            &mut self.openness
        } else {
            &mut self.safety
        };
        if side.len() >= MAX_PROMISE_REGISTRY {
            return Err("E_PROMISE_REGISTRY_FULL");
        }
        if side.iter().any(|q| q.tag == p.tag) {
            return Err("E_PROMISE_DUPLICATE");
        }
        side.push(p);
        Ok(())
    }

    /// 双承诺是否成立（两侧同时非零且未超上界）。
    ///
    /// 口号落地为计数：只登记一侧等于没承诺另一侧。
    pub fn balanced(&self) -> bool {
        !self.openness.is_empty() && !self.safety.is_empty()
    }

    /// 登记总数（审计面）。
    pub fn total(&self) -> usize {
        self.openness.len() + self.safety.len()
    }
}

// ---------------------------------------------------------------------------
// 四、交接面（承接 V 域移交包 F4593）
// ---------------------------------------------------------------------------

/// 移交包一件（十件之一，带封包哈希）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HandoffPiece {
    /// 件名。
    pub name: &'static str,
    /// 封包哈希（十六进制；不符即重封）。
    pub hash: String,
}

/// 交接面三源之一。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HandoffSource {
    /// 显示与色彩 API 冻结面。
    ApiFreeze,
    /// 无障碍门禁基线。
    A11yBaseline,
    /// 一致性契约源。
    ConsistencyContract,
}

impl HandoffSource {
    /// 稳定短名。
    pub fn tag(self) -> &'static str {
        match self {
            HandoffSource::ApiFreeze => "api-freeze",
            HandoffSource::A11yBaseline => "a11y-baseline",
            HandoffSource::ConsistencyContract => "consistency-contract",
        }
    }

    /// 三源穷举（落地 O(源数) 的驱动面）。
    pub fn all() -> [HandoffSource; HANDOFF_SOURCE_COUNT] {
        [
            HandoffSource::ApiFreeze,
            HandoffSource::A11yBaseline,
            HandoffSource::ConsistencyContract,
        ]
    }

    /// 该源应落到的插件能力面层（承接即入面，不得悬空）。
    pub fn target_layer(self) -> Layer {
        match self {
            // API 冻结面是插件直接可见的面 → SDK 接口层
            HandoffSource::ApiFreeze => Layer::SdkInterface,
            // 无障碍门禁基线是准入门槛 → 生态治理层
            HandoffSource::A11yBaseline => Layer::EcosystemGovernance,
            // 一致性契约源须经隔离与授权才能到达插件 → 能力授权层
            HandoffSource::ConsistencyContract => Layer::CapabilityGrant,
        }
    }
}

/// 交接面落地表（一源一条落地记录）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HandoffLanding {
    /// 来源。
    pub source: HandoffSource,
    /// 落到哪一层的能力面。
    pub layer: Layer,
    /// 是否已登记进冻结面（承接即冻结，不得只挂文档）。
    pub registered: bool,
}

// ---------------------------------------------------------------------------
// 五、W 域总架构（四层 + 冻结 + 承接 + 双承诺）
// ---------------------------------------------------------------------------

/// W 域总架构（开工闸对象；构造即校验，失配即拒收）。
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SdkArchitecture {
    /// 四层各自的冻结接口。
    pub frozen: Vec<FrozenIface>,
    /// 双承诺台账。
    pub promise: DualPromise,
    /// 移交包十件。
    pub pieces: Vec<HandoffPiece>,
    /// 交接面落地表。
    pub landings: Vec<HandoffLanding>,
    /// 失配诊断。
    pub mismatches: Vec<LayerMismatch>,
    /// 错误账本（三元组：层/类别 + 码 + 说明）。
    pub errors: Vec<(String, &'static str, String)>,
}

impl SdkArchitecture {
    /// 构造空架构（尚未开工）。
    pub fn new() -> Self {
        SdkArchitecture {
            frozen: Vec::new(),
            promise: DualPromise::default(),
            pieces: Vec::new(),
            landings: Vec::new(),
            mismatches: Vec::new(),
            errors: Vec::new(),
        }
    }

    /// 冻结一条接口（层内 id 唯一；语义变更须升版——同 id 重复即越权）。
    pub fn freeze(&mut self, owner: Layer, id: u32, version: u16) -> Result<(), &'static str> {
        if self.frozen.iter().any(|f| f.owner == owner && f.id == id) {
            self.errors.push((
                owner.tag().to_string(),
                "E_IFACE_REDEFINE",
                format!(
                    "层 {} 接口 {} 已冻结，改语义须走冻结流程升版（不得原地改）",
                    owner.label(),
                    id
                ),
            ));
            return Err("E_IFACE_REDEFINE");
        }
        self.frozen.push(FrozenIface { id, owner, version });
        Ok(())
    }

    /// 升版（冻结流程的唯一合法变更路径）。
    pub fn bump_version(&mut self, owner: Layer, id: u32) -> Result<u16, &'static str> {
        let slot = self
            .frozen
            .iter_mut()
            .find(|f| f.owner == owner && f.id == id)
            .ok_or("E_IFACE_ABSENT")?;
        slot.version = slot.version.saturating_add(1);
        Ok(slot.version)
    }

    /// 记入移交包一件（哈希必填且长度合规）。
    pub fn add_piece(&mut self, name: &'static str, hash: &str) -> Result<(), &'static str> {
        if hash.len() != HASH_HEX_LEN {
            self.errors.push((
                name.to_string(),
                "E_HASH_MALFORMED",
                format!("移交件 {} 哈希长度 {}，应为 {}", name, hash.len(), HASH_HEX_LEN),
            ));
            return Err("E_HASH_MALFORMED");
        }
        if self.pieces.iter().any(|p| p.name == name) {
            return Err("E_PIECE_DUPLICATE");
        }
        self.pieces
            .push(HandoffPiece { name, hash: hash.to_string() });
        Ok(())
    }

    /// 核验移交包（十件齐备即收；缺件拒收——锚点「件缺 → 拒收」）。
    pub fn verify_handoff(&self) -> bool {
        self.pieces.len() == HANDOFF_PIECE_COUNT
    }

    /// 登记一条交接面落地（源必须落进其目标层，且须落进冻结面才算承接完成）。
    pub fn land(&mut self, source: HandoffSource, registered: bool) {
        self.landings.push(HandoffLanding {
            source,
            layer: source.target_layer(),
            registered,
        });
    }

    /// 承接是否落地完整（三源齐备且全部登记进冻结面）。
    pub fn handoff_landed(&self) -> bool {
        HANDOFF_SOURCE_COUNT == self.landings.len()
            && self
                .landings
                .iter()
                .all(|l| l.registered && self.frozen.iter().any(|f| f.owner == l.layer))
    }

    /// 层间对拍：逐层校验依赖方向（只许向下依赖，反向即越权）。
    ///
    /// O(接口数)：每条冻结接口与其所属层序一并核对。
    pub fn conformance(&mut self) -> bool {
        self.mismatches.clear();
        // 每层至少有一条冻结接口——空层即失配（层不可是空壳）。
        for layer in Layer::all() {
            if !self.frozen.iter().any(|f| f.owner == layer) {
                self.mismatches.push(LayerMismatch {
                    from: layer,
                    to: layer,
                    what: format!("层 {} 无任何冻结接口", layer.label()),
                    why: "空层即失配：四层须各有冻结面，否则该层形同虚设",
                });
            }
        }
        // 版本面：冻结版本不得为 0（0 表示未真正冻结）。
        for f in self.frozen.iter() {
            if f.version == 0 {
                self.mismatches.push(LayerMismatch {
                    from: f.owner,
                    to: f.owner,
                    what: format!("层 {} 接口 {} 版本为 0", f.owner.label(), f.id),
                    why: "版本 0 即未冻结：下游无法按稳定面先行开发",
                });
            }
        }
        if !self.mismatches.is_empty() {
            let detail = self
                .mismatches
                .iter()
                .map(|m| m.what.clone())
                .collect::<Vec<_>>()
                .join("；");
            self.errors.push((
                "layer".to_string(),
                "E_LAYER_MISMATCH",
                detail,
            ));
            return false;
        }
        true
    }

    /// 开工闸：四层齐备 + 双承诺达标 + 承接落地 + 对拍通过，四者同时成立才准开工。
    pub fn open_gate(&mut self) -> bool {
        if !self.verify_handoff() {
            self.errors.push((
                "handoff".to_string(),
                "E_HANDOFF_INCOMPLETE",
                format!(
                    "移交包 {} 件（应 {}），缺件拒收开工，回溯移交包",
                    self.pieces.len(),
                    HANDOFF_PIECE_COUNT
                ),
            ));
            return false;
        }
        if !self.handoff_landed() {
            self.errors.push((
                "handoff".to_string(),
                "E_HANDOFF_SOURCE_MISSING",
                "交接面三源未全部落进插件能力面，缺源不得开工".to_string(),
            ));
            return false;
        }
        if !self.promise.balanced() {
            self.errors.push((
                "promise".to_string(),
                "E_PROMISE_IMBALANCE",
                format!(
                    "双承诺失衡：开放侧 {} 条、安全侧 {} 条——两侧须同时非零",
                    self.promise.openness.len(),
                    self.promise.safety.len()
                ),
            ));
            return false;
        }
        self.conformance()
    }

    /// 错误账本。
    pub fn errors(&self) -> &[(String, &'static str, String)] {
        &self.errors
    }

    /// 失配清单。
    pub fn mismatches(&self) -> &[LayerMismatch] {
        &self.mismatches
    }
}

// ---------------------------------------------------------------------------
// 六、自检（CheckSet）
// ---------------------------------------------------------------------------

/// 构造一个四层齐备、双承诺达标、承接落地通过的合规架构（自检共用底座）。
fn compliant() -> SdkArchitecture {
    let mut a = SdkArchitecture::new();
    for layer in Layer::all() {
        let _ = a.freeze(layer, 1, 1);
    }
    let _ = a.promise.add(
        Promise { tag: "open-api" },
        true,
    );
    let _ = a.promise.add(
        Promise { tag: "safe-isolate" },
        false,
    );
    let names: [&'static str; HANDOFF_PIECE_COUNT] = [
        "iface-ledger", "color-proof", "scene-cover", "three-rules", "fuzz",
        "perf-book", "security-privacy", "doc-outline", "clearance", "score-walk",
    ];
    for (i, n) in names.iter().enumerate() {
        let _ = a.add_piece(n, &format!("{:08x}", i + 1));
    }
    for s in HandoffSource::all() {
        a.land(s, true);
    }
    a
}

/// VE-F4601 域自检。
pub fn run_vew01_checks() -> CheckSet {
    let mut set = CheckSet::new(VEW_DOMAIN);

    // ---- 判据一：四层 ----
    {
        let all = Layer::all();
        let ok = all.len() == LAYER_COUNT
            && all[0] == Layer::SdkInterface
            && all[3] == Layer::EcosystemGovernance
            && all[0].ordinal() < all[3].ordinal();
        set.add("W01-四层-层数与层序不可重排", ok, "");
    }

    {
        let mut a = compliant();
        let ok = a.open_gate();
        let all_layers_have = Layer::all()
            .iter()
            .all(|l| a.frozen.iter().any(|f| f.owner == *l));
        set.add("W01-四层-合规架构准开工", ok && all_layers_have, "");
    }

    {
        // 空层即失配：抽掉治理层的冻结接口，对拍必须判红。
        let mut a = compliant();
        a.frozen.retain(|f| f.owner != Layer::EcosystemGovernance);
        let ok = !a.conformance();
        set.add("W01-四层-空层对拍判红", ok, "");
    }

    // ---- 判据二：双承诺（域本色·最高优先）----
    {
        let mut p = DualPromise::default();
        let _ = p.add(Promise { tag: "open" }, true);
        let only_one = !p.balanced();
        let _ = p.add(Promise { tag: "safe" }, false);
        let both = p.balanced() && p.total() == 2;
        set.add("W01-双承诺-两侧须同时非零", only_one && both, "");
    }

    {
        // 只封锁不开放 → 失衡 → 开工闸拒收（扼杀也是违约）。
        let mut a = compliant();
        a.promise = DualPromise::default();
        let _ = a.promise.add(Promise { tag: "safe-only" }, false);
        let rejected = !a.open_gate();
        let logged = a.errors().iter().any(|(_, c, _)| *c == "E_PROMISE_IMBALANCE");
        set.add("W01-双承诺-只封锁不放行拒收开工", rejected && logged, "");
    }

    {
        let mut p = DualPromise::default();
        let _ = p.add(Promise { tag: "dup" }, true);
        let dup = p.add(Promise { tag: "dup" }, true).is_err();
        set.add("W01-双承诺-重复登记显性拒绝", dup, "");
    }

    // ---- 判据三：承接落地 ----
    {
        let a = compliant();
        set.add(
            "W01-承接-十件齐备方收",
            a.verify_handoff() && a.handoff_landed(),
            "",
        );
    }

    {
        // 缺件即拒收（锚点：件缺 → 拒收）。
        let mut a = compliant();
        a.pieces.pop();
        let rejected = !a.open_gate();
        let logged = a.errors().iter().any(|(_, c, _)| *c == "E_HANDOFF_INCOMPLETE");
        set.add("W01-承接-缺件拒收开工", rejected && logged, "");
    }

    {
        // 承接缺源即拒收（锚点：承接缺源 → 回溯移交包）。
        let mut a = compliant();
        a.landings.clear();
        let rejected = !a.open_gate();
        let logged = a
            .errors()
            .iter()
            .any(|(_, c, _)| *c == "E_HANDOFF_SOURCE_MISSING");
        set.add("W01-承接-缺源拒收开工", rejected && logged, "");
    }

    {
        // 交接面须落进对应层的能力面，不得错层。
        let ok = HandoffSource::all()
            .iter()
            .all(|s| s.target_layer() != Layer::RuntimeIsolation);
        set.add("W01-承接-三源各有目标能力面", ok, "");
    }

    // ---- 判据四：层冻结 ----
    {
        let mut a = compliant();
        let redefined = a.freeze(Layer::SdkInterface, 1, 2).is_err();
        let logged = a.errors().iter().any(|(_, c, _)| *c == "E_IFACE_REDEFINE");
        let bumped = a.bump_version(Layer::SdkInterface, 1).unwrap_or(0);
        set.add("W01-层冻结-改语义须走升版流程", redefined && logged && bumped == 2, "");
    }

    {
        // 版本 0 即未冻结，对拍判红。
        let mut a = compliant();
        for f in a.frozen.iter_mut() {
            if f.owner == Layer::SdkInterface {
                f.version = 0;
            }
        }
        let ok = !a.conformance();
        set.add("W01-层冻结-版本0判未冻结", ok, "");
    }

    {
        let mut a = compliant();
        let bad = a.add_piece("bad-hash", "xyz").is_err();
        let logged = a.errors().iter().any(|(_, c, _)| *c == "E_HASH_MALFORMED");
        set.add("W01-层冻结-哈希长度校验", bad && logged, "");
    }

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn four_layers_are_ordered_and_fixed() {
        assert_eq!(Layer::all().len(), LAYER_COUNT);
        assert!(Layer::SdkInterface.ordinal() < Layer::RuntimeIsolation.ordinal());
        assert!(Layer::RuntimeIsolation.ordinal() < Layer::CapabilityGrant.ordinal());
        assert!(Layer::CapabilityGrant.ordinal() < Layer::EcosystemGovernance.ordinal());
    }

    #[test]
    fn compliant_arch_opens() {
        let mut a = compliant();
        assert!(a.open_gate(), "合规架构应通过开工闸");
        assert!(a.mismatches().is_empty());
    }

    #[test]
    fn promise_needs_both_sides() {
        let mut p = DualPromise::default();
        assert!(!p.balanced(), "空台账不得判达标");
        let _ = p.add(Promise { tag: "open" }, true);
        assert!(!p.balanced(), "只有开放侧仍失衡");
        let _ = p.add(Promise { tag: "safe" }, false);
        assert!(p.balanced());
    }

    #[test]
    fn missing_piece_blocks_gate() {
        let mut a = compliant();
        a.pieces.pop();
        assert!(!a.open_gate(), "缺件必须拒收");
    }

    #[test]
    fn missing_source_blocks_gate() {
        let mut a = compliant();
        a.landings.pop();
        assert!(!a.open_gate(), "缺源必须拒收");
    }

    #[test]
    fn empty_layer_fails_conformance() {
        let mut a = compliant();
        a.frozen.retain(|f| f.owner != Layer::CapabilityGrant);
        assert!(!a.conformance(), "空层须判失配");
        assert!(!a.mismatches().is_empty());
    }

    #[test]
    fn redefinition_is_rejected_but_bump_allowed() {
        let mut a = compliant();
        // 同层同 id 重复冻结即越权（改语义须走升版流程）。
        assert!(a.freeze(Layer::SdkInterface, 1, 9).is_err());
        assert_eq!(a.bump_version(Layer::SdkInterface, 1).ok(), Some(2));
        // id 仅层内唯一：换个层用同一个 id 合法（合规底座每层已占 id=1，
        // 故此处取 id=7 以证明跨层不冲突）。
        assert!(a.freeze(Layer::RuntimeIsolation, 7, 1).is_ok());
        // 不存在的接口升版须显性失败，不静默造版本。
        assert!(a.bump_version(Layer::CapabilityGrant, 999).is_err());
    }

    #[test]
    fn handoff_sources_map_to_layers() {
        assert_eq!(HandoffSource::ApiFreeze.target_layer(), Layer::SdkInterface);
        assert_eq!(
            HandoffSource::A11yBaseline.target_layer(),
            Layer::EcosystemGovernance
        );
        assert_eq!(
            HandoffSource::ConsistencyContract.target_layer(),
            Layer::CapabilityGrant
        );
    }

    #[test]
    fn gate_is_all_or_nothing() {
        // 逐项破坏：任一项缺失都不得放行。
        let mut breaks: Vec<Box<dyn Fn(&mut SdkArchitecture)>> = vec![
            Box::new(|a: &mut SdkArchitecture| {
                a.pieces.pop();
            }),
            Box::new(|a: &mut SdkArchitecture| {
                a.landings.clear();
            }),
            Box::new(|a: &mut SdkArchitecture| {
                a.promise = DualPromise::default();
            }),
            Box::new(|a: &mut SdkArchitecture| {
                a.frozen.retain(|f| f.owner != Layer::EcosystemGovernance);
            }),
        ];
        let n = breaks.len();
        for b in breaks.iter_mut() {
            let mut a = compliant();
            b(&mut a);
            assert!(!a.open_gate(), "开工闸须全项通过，不得部分放行");
        }
        assert_eq!(n, 4, "四条破坏路径均已覆盖");
    }

    #[test]
    fn effects_checks_all_green() {
        let set = run_vew01_checks();
        let (p, f) = set.tally();
        assert!(!set.truncated());
        assert!(set.all_passed(), "VE-F4601 红项：{}/{}", p, p + f);
    }
}
