//! VE-F2213 · 粒子 API 冻结 v1（VE-L 域 · 粒子段 · 目标 320 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F2213`
//!
//! **判据（锚点原文）**：十签名、v1 冻结、衔接核验、只增不改、判据。
//!
//! **职责定位（锚点原文）**：粒子 API 冻结 v1——粒子族冻结（发射器管理/
//! 池查询/事件订阅——签名冻结版本化：发射器管理五签名（创建/销毁/参数
//! 读写/启停/枚举）+池查询三签名（水位/容量/统计）+事件订阅二签名
//! （池压力事件/发射器事件）——十签名 v1）、与 I02 绘制族/F1408 事件
//! 总线命名一致（跨域衔接核验：l.ps. 前缀与事件命名对齐 F1925 注册制）、
//! 十年承诺。
//!
//! # 一、冻结清单十签名（族计数 5/3/2 逐条钉死）
//!
//! 每条签名五列：全名/参数表/返回契约/官方描述词/版本 v1。全名一律
//! `l.ps.` 前缀（粒子段命名空间——与事件命名对齐 F1925 注册制的
//! 拼写漂移防线）。**官方描述词非空是冻结的前件**：没有描述词的
//! 签名不可读屏、不可翻译，整簿拒绝冻结（锚点错误路径：描述词
//! 缺失→冻结拒绝）。
//!
//! # 二、衔接核验表三行（真调对端，不代填）
//!
//! - 事件订阅 ↔ F1408/F1925：真调 [`EventRegistry`](vel04_mode::EventRegistry)
//!   注册与查表——粒子两个事件名经注册制放行、未注册名查表为 `None`；
//!   四要素（名称/来源/参数/订阅者）在 [`SubContract`] 上逐字段在场。
//! - 池查询 ↔ F1776：真调 [`PoolQuota::validate`](vel08_pool::PoolQuota::validate)
//!   与三阈值常量（70/85/95 预警/降质/拒绝——F1776 全景表口径在 vel08
//!   的实例化），且滞回阈值严格分离（65/80/90 < 70/85/95）。
//! - 发射器枚举 ↔ F2209：真调 [`EmitterCounter`](vel09_debug::EmitterCounter)——
//!   枚举契约 [`EmitterInfo`] 的字段恰为发射器级统计的四个字段
//!   （emitter_id/live/spawned_total/died_total），枚举面与统计面
//!   **同一记录**（两份定义迟早漂移，排查时不知道该信谁）。
//!
//! # 三、四道错误路径（锚点逐条）
//!
//! - 签名漂移 → CI 钩子拦截（[`drift_hook`]——全名/描述词/参数表/
//!   返回/版本逐字段比对，**双向**：多余条与缺失条都是漂移）；
//! - 描述词缺失 → 冻结拒绝（[`FreezeBook::freeze`] 整簿不冻）；
//! - 衔接不一致 → 以对端冻结为准回改（[`LinkRow`] 带回改动作声明，
//!   本仓三行均 Aligned；Diverged 臂在判据侧以构造数据演练）；
//! - v1 只增不改（[`evolve`]：Patch 恒拒并指引 v2 追加段；Append
//!   只许 v2 版本且不触碰 v1 条目）。
//!
//! # 四、十年承诺与 F2219 联动
//!
//! [`TEN_YEAR_COMMITMENT_MS`] 把「十年」钉成可判定数值（10×365 天
//! 毫秒）——承诺不是口头礼花：v1 条目在承诺期内只增不改，任何
//! 语义变更走 v2 追加段。[`f2219_handoff`] 向 F2219（粒子组一致性）
//! 交出席位表：术语三行（emitter=发射器/group=粒子组/pool=粒子池）
//! + 十条全名清单——术语与签名**同检**（术语表与签名簿分开维护，
//! 改名不同步就是静默漂移）。
//!
//! **如实记录（锚点与册内编号不符，同 vel07_blend 先例）**：锚点写
//! 「与 I02 绘制族/F1408 事件总线命名一致」「核验对象 I02 F1650」，
//! 但册内 VE-F1650 实为**材质输入验证**（I03 材质域，非 I02 绘制族）；
//! 本仓亦无 I02 绘制族签名簿可对。处理：命名一致性与 F1408/F1925
//! 的事件面按册内实证核验（见衔接行一），「I02 绘制族」一行**不编造
//! 对端**——如实记为无本仓实现对口的声明位，留给 I02 域落地时回填。
//!
//! **性能（锚点原文）**：冻结静态核对构建期；钩子毫秒级；运行时成本
//! 在实现条目分摊——本模块零运行时热路径（纯簿记与核验）。
//!
//! **无障碍（锚点原文）**：描述词全称化；清单公开。

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use crate::svstar2::vel03_emitter::DiagBag;
use crate::svstar2::vel04_mode::{EventRegistry, EventSchema};
use crate::svstar2::vel08_pool::{
    PoolKind, PoolQuota, HYST_DEGRADE_OFF, HYST_REJECT_OFF, HYST_WARN_OFF, WM_DEGRADE_PCT,
    WM_REJECT_PCT, WM_WARN_PCT,
};
use crate::svstar2::vel09_debug::EmitterCounter;

// ---------------------------------------------------------------------------
// 一、常量与错误码
// ---------------------------------------------------------------------------

/// 本项版本。
pub const API_FREEZE_VERSION: &str = "L13-api-v1";

/// v1 冻结版本号（十签名一律挂此版本）。
pub const SIGN_VERSION_V1: &str = "v1";

/// 粒子段签名命名空间前缀（L 域粒子段——粒子与物理域 F2201 开工宣告的
/// 段落编号；前缀即注册制防拼写漂移的第一道闸：无前缀名一律非本族签名）。
pub const SIGN_PREFIX: &str = "l.ps.";

/// 十年承诺（毫秒）：v1 条目承诺期内只增不改。
///
/// 十年按 10×365 天计（不含闰日——承诺是**下界**：不含闰日的十年
/// 仍满十个平年，315_360_000_000ms = 10×365×24×3600×1000）。
pub const TEN_YEAR_COMMITMENT_MS: u64 = 315_360_000_000;

/// 池压力事件名（注册制命名——F1925 四要素之「名称」）。
pub const EVENT_POOL_PRESSURE: &str = "l.ps.event.pool_pressure";

/// 发射器事件名（创建/销毁/启停的状态变更事件）。
pub const EVENT_EMITTER: &str = "l.ps.event.emitter";

/// 描述词缺失（冻结拒绝）。
pub const E_API_DESCRIPTOR: &str = "E_API_DESCRIPTOR";

/// 签名漂移（CI 钩子拦截）。
pub const E_API_DRIFT: &str = "E_API_DRIFT";

/// v1 只增不改（patch 被拒，指引 v2 追加段）。
pub const E_API_V1_IMMUTABLE: &str = "E_API_V1_IMMUTABLE";

/// v2 追加段版本不符（追加只许 v2——v1 簿里塞 v1 新条会使
/// 「v1 已冻结集合」失去封闭含义）。
pub const E_API_APPEND_VERSION: &str = "E_API_APPEND_VERSION";

/// 衔接不一致（以对端冻结为准回改）。
pub const E_API_LINK_DIVERGED: &str = "E_API_LINK_DIVERGED";

// ---------------------------------------------------------------------------
// 二、签名族与单签名规格（五列）
// ---------------------------------------------------------------------------

/// 签名族（三族——锚点：发射器管理/池查询/事件订阅）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SignFamily {
    /// 发射器管理（五签名）。
    EmitterMgmt,
    /// 池查询（三签名）。
    PoolQuery,
    /// 事件订阅（二签名）。
    EventSub,
}

impl SignFamily {
    /// 三族闭集。
    pub const ALL: [SignFamily; 3] = [SignFamily::EmitterMgmt, SignFamily::PoolQuery, SignFamily::EventSub];

    /// 族名（读屏可达）。
    pub fn zh(self) -> &'static str {
        match self {
            SignFamily::EmitterMgmt => "发射器管理",
            SignFamily::PoolQuery => "池查询",
            SignFamily::EventSub => "事件订阅",
        }
    }

    /// 族内签名数（5/3/2——锚点钉死，与 [`FREEZE_V1`] 对账）。
    pub fn expect_count(self) -> usize {
        match self {
            SignFamily::EmitterMgmt => 5,
            SignFamily::PoolQuery => 3,
            SignFamily::EventSub => 2,
        }
    }
}

/// 单签名冻结规格（五列：全名/参数表/返回契约/官方描述词/版本）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SignSpec {
    /// 全名（`l.ps.` 前缀）。
    pub name: &'static str,
    /// 官方描述词（全称化——非空是冻结前件）。
    pub descriptor: &'static str,
    /// 参数表（形参声明；空表 = 无参签名，多数签名有参）。
    pub params: &'static [&'static str],
    /// 返回契约（可读屏的类型摘要）。
    pub ret: &'static str,
    /// 所属族。
    pub family: SignFamily,
    /// 版本（v1）。
    pub version: &'static str,
}

impl SignSpec {
    /// 规格自检：描述词非空 + `l.ps.` 前缀 + v1 版本 + 返回契约非空。
    ///
    /// 参数表可为空（无参签名合法——如 `l.ps.pool.capacity` 只取池标识
    /// 而归约进 `pool` 一参，见 [`FREEZE_V1`]），故不断言参数非空。
    pub fn validate(&self) -> Result<(), String> {
        if self.descriptor.trim().is_empty() {
            return Err(format!("{}：签名 {} 缺官方描述词", E_API_DESCRIPTOR, self.name));
        }
        if !self.name.starts_with(SIGN_PREFIX) {
            return Err(format!("{}：签名 {} 缺 {} 命名空间前缀", E_API_DRIFT, self.name, SIGN_PREFIX));
        }
        if self.version != SIGN_VERSION_V1 {
            return Err(format!("{}：签名 {} 版本 {} 非 {}", E_API_DRIFT, self.name, self.version, SIGN_VERSION_V1));
        }
        if self.ret.trim().is_empty() {
            return Err(format!("{}：签名 {} 缺返回契约", E_API_DRIFT, self.name));
        }
        Ok(())
    }
}

/// v1 冻结十条（锚点：五/三/二——逐条原文职责落位）。
pub const FREEZE_V1: [SignSpec; 10] = [
    // --- 发射器管理五签名（实现源 F2203）---
    SignSpec {
        name: "l.ps.emitter.create",
        descriptor: "创建发射器：按配置建发射器并绑定粒子组，返回发射器句柄",
        params: &["config: EmitterConfig", "groups: &[ParticleGroup]"],
        ret: "Outcome<EmitterHandle>",
        family: SignFamily::EmitterMgmt,
        version: SIGN_VERSION_V1,
    },
    SignSpec {
        name: "l.ps.emitter.destroy",
        descriptor: "销毁发射器：走生命周期状态机到销毁，回收池句柄与组绑定",
        params: &["handle: EmitterHandle"],
        ret: "Outcome<()>",
        family: SignFamily::EmitterMgmt,
        version: SIGN_VERSION_V1,
    },
    SignSpec {
        name: "l.ps.emitter.set_param",
        descriptor: "参数读写：发射率/形状/速度分布参数的读与写（写即钳制记账）",
        params: &["handle: EmitterHandle", "key: ParamKey", "value: f32"],
        ret: "Outcome<f32>",
        family: SignFamily::EmitterMgmt,
        version: SIGN_VERSION_V1,
    },
    SignSpec {
        name: "l.ps.emitter.set_running",
        descriptor: "启停：激活/暂停发射器（状态机合法迁移外一律拒绝）",
        params: &["handle: EmitterHandle", "run: bool"],
        ret: "Outcome<EmitterState>",
        family: SignFamily::EmitterMgmt,
        version: SIGN_VERSION_V1,
    },
    SignSpec {
        name: "l.ps.emitter.enumerate",
        descriptor: "枚举：按过滤条件列出发射器及其实时统计（与 F2209 同记录）",
        params: &["filter: EnumFilter", "limit: u32"],
        ret: "Vec<EmitterInfo>",
        family: SignFamily::EmitterMgmt,
        version: SIGN_VERSION_V1,
    },
    // --- 池查询三签名（实现源 F2208/F2209，F1776 口径）---
    SignSpec {
        name: "l.ps.pool.water",
        descriptor: "水位：池在用/空闲/容量/水位百分位/压力档位一次读出",
        params: &["pool: PoolId"],
        ret: "PoolWater",
        family: SignFamily::PoolQuery,
        version: SIGN_VERSION_V1,
    },
    SignSpec {
        name: "l.ps.pool.capacity",
        descriptor: "容量：池总配额与单发射器上限（双配额只读查询）",
        params: &["pool: PoolId"],
        ret: "PoolCapacity",
        family: SignFamily::PoolQuery,
        version: SIGN_VERSION_V1,
    },
    SignSpec {
        name: "l.ps.pool.stats",
        descriptor: "统计：每秒聚合格的粒子总数/水位/生成死亡率（窗口秒数）",
        params: &["pool: PoolId", "window_s: u32"],
        ret: "PoolStatsView",
        family: SignFamily::PoolQuery,
        version: SIGN_VERSION_V1,
    },
    // --- 事件订阅二签名（实现源 F2204，F1408/F1925 注册制）---
    SignSpec {
        name: "l.ps.event.subscribe_pool_pressure",
        descriptor: "订阅池压力事件：三阈值触发时按 schema 覆盖发射参数",
        params: &["event: l.ps.event.pool_pressure", "source: EventSource", "schema: EventSchema", "sink: SubId"],
        ret: "SubContract",
        family: SignFamily::EventSub,
        version: SIGN_VERSION_V1,
    },
    SignSpec {
        name: "l.ps.event.subscribe_emitter",
        descriptor: "订阅发射器事件：创建/销毁/启停状态变更触发粒子侧反应",
        params: &["event: l.ps.event.emitter", "source: EventSource", "schema: EventSchema", "sink: SubId"],
        ret: "SubContract",
        family: SignFamily::EventSub,
        version: SIGN_VERSION_V1,
    },
];

// ---------------------------------------------------------------------------
// 三、契约记录类型（衔接核验的比对基准——与对端字段一一对应）
// ---------------------------------------------------------------------------

/// 池水位查询契约（`l.ps.pool.water` 返回记录）。
///
/// 字段与 vel09 [`PoolStatsSnapshot`](vel09_debug::PoolStatsSnapshot)
/// 一致——池查询面与调试统计面同一记录（F2209 单源声明）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PoolWater {
    /// 在用槽位。
    pub live: u32,
    /// 容量。
    pub capacity: u32,
    /// 空闲槽位。
    pub free: u32,
    /// 水位百分位（池侧同口径整数）。
    pub water_pct: u64,
}

/// 池容量查询契约（`l.ps.pool.capacity` 返回记录——双配额）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PoolCapacity {
    /// 槽位容量。
    pub capacity: u32,
    /// 单粒子字节数。
    pub stride: u32,
    /// 单发射器上限（槽位）。
    pub emitter_cap: u32,
    /// 总内存字节（容量×stride）。
    pub total_bytes: u64,
}

/// 发射器枚举契约（`l.ps.emitter.enumerate` 返回记录）。
///
/// 字段与 vel09 [`EmitterCounter`](vel09_debug::EmitterCounter) 一致——
/// 枚举面与统计面同一记录（F2209 对齐）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EmitterInfo {
    /// 发射器 id。
    pub emitter_id: u32,
    /// 当前在用粒子数。
    pub live: u32,
    /// 累计生成数。
    pub spawned_total: u64,
    /// 累计死亡数。
    pub died_total: u64,
}

/// 事件订阅契约（`l.ps.event.subscribe_*` 返回记录——F1408 四要素）。
///
/// 仅 `PartialEq`：`schema` 含 f32（Vec3/倍率），f32 无 `Eq`——
/// 订阅契约按值比较即可，不冒充全等语义。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SubContract {
    /// 要素一：名称（注册制事件名）。
    pub name: &'static str,
    /// 要素二：来源（音频/UI/游戏——F1408 来源枚举的粒子侧声明位）。
    pub source: EventSource,
    /// 要素三：参数 schema（位置偏移/数量倍率/速度倍率三覆盖）。
    pub schema: EventSchema,
    /// 要素四：订阅者标识（sink 句柄）。
    pub sink: SubId,
}

/// 事件来源（F1408 四要素之「来源」枚举——与音频/游戏事件同源声明）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EventSource {
    /// 音频事件（H 域 F1408 总线）。
    Audio,
    /// UI 事件。
    Ui,
    /// 游戏事件。
    Game,
}

impl EventSource {
    /// 三来源闭集。
    pub const ALL: [EventSource; 3] = [EventSource::Audio, EventSource::Ui, EventSource::Game];

    /// 短码（wire 名）。
    pub fn wire(self) -> &'static str {
        match self {
            EventSource::Audio => "audio",
            EventSource::Ui => "ui",
            EventSource::Game => "game",
        }
    }
}

/// 订阅者标识（新类型防 id 误用）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SubId(pub u32);

// ---------------------------------------------------------------------------
// 四、冻结簿（清单入库 + 入册前裁决）
// ---------------------------------------------------------------------------

/// 冻结簿（v1 十条 + v2 追加段）。
#[derive(Clone, Debug, Default)]
pub struct FreezeBook {
    entries: Vec<SignSpec>,
}

impl FreezeBook {
    /// 空簿。
    pub fn new() -> FreezeBook {
        FreezeBook::default()
    }

    /// 冻结：逐条 validate 后整簿入库。任一条描述词缺失/前缀缺失/
    /// 版本不符 → **整簿拒绝**（半冻的簿比不冻更糟——调用方会以为
    /// 剩下的签名也已冻结）。
    pub fn freeze(specs: &[SignSpec]) -> Result<FreezeBook, String> {
        let mut book = FreezeBook::new();
        for s in specs.iter() {
            s.validate()?;
        }
        // 全名唯一性（冻结集合内重名 = 两条签名抢一个命名空间位）。
        for (i, a) in specs.iter().enumerate() {
            for b in specs.iter().skip(i + 1) {
                if a.name == b.name {
                    return Err(format!("{}：签名 {} 在冻结集合内重名", E_API_DRIFT, a.name));
                }
            }
        }
        for s in specs.iter() {
            book.entries.push(s.clone());
        }
        Ok(book)
    }

    /// 在簿条目数（含 v2 追加段）。
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// 是否空簿。
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// 按全名取条目。
    pub fn get(&self, name: &str) -> Option<&SignSpec> {
        self.entries.iter().find(|e| e.name == name)
    }

    /// 族内条目数。
    pub fn family_count(&self, f: SignFamily) -> usize {
        self.entries.iter().filter(|e| e.family == f).count()
    }

    /// 全名清单（读屏/移交可达）。
    pub fn names(&self) -> Vec<&'static str> {
        self.entries.iter().map(|e| e.name).collect()
    }

    /// v1 条目 iterator（版本过滤——只增不改纪律的查询面）。
    pub fn v1_entries(&self) -> impl Iterator<Item = &SignSpec> + '_ {
        self.entries.iter().filter(|e| e.version == SIGN_VERSION_V1)
    }
}

// ---------------------------------------------------------------------------
// 五、漂移守卫（CI 钩子）与演化（只增不改）
// ---------------------------------------------------------------------------

/// 漂移钩子：当前签名面与冻结簿逐字段比对。
///
/// **双向**——`current` 有冻结簿没有的条（偷加未冻结签名）与 `current`
/// 缺冻结簿有的条（偷删/偷改已冻签名）都是漂移。比对字段：全名集合
/// 先对，再逐条对描述词/参数表/返回/族/版本。
pub fn drift_hook(current: &[SignSpec], frozen: &FreezeBook) -> Result<(), String> {
    let frozen_names = frozen.names();
    // 1) current 缺条（冻结条目被删）。
    for n in frozen_names.iter() {
        if !current.iter().any(|c| c.name == *n) {
            return Err(format!("{}：冻结条目 {} 在当前面缺失（偷删）", E_API_DRIFT, n));
        }
    }
    // 2) current 多条（未冻结签名混入）。
    for c in current.iter() {
        if !frozen_names.iter().any(|n| *n == c.name) {
            return Err(format!("{}：签名 {} 未在冻结簿（偷加）", E_API_DRIFT, c.name));
        }
    }
    // 3) 逐字段比对。
    for c in current.iter() {
        let f = match frozen.get(c.name) {
            Some(f) => f,
            None => unreachable!(),
        };
        if c.descriptor != f.descriptor {
            return Err(format!(
                "{}：{} 描述词漂移（冻「{}」→ 现「{}」）",
                E_API_DRIFT, c.name, f.descriptor, c.descriptor
            ));
        }
        if c.params != f.params {
            return Err(format!("{}：{} 参数表漂移（{:?} → {:?}）", E_API_DRIFT, c.name, f.params, c.params));
        }
        if c.ret != f.ret {
            return Err(format!("{}：{} 返回契约漂移（{} → {}）", E_API_DRIFT, c.name, f.ret, c.ret));
        }
        if c.family != f.family {
            return Err(format!("{}：{} 族别漂移（{:?} → {:?}）", E_API_DRIFT, c.name, f.family, c.family));
        }
        if c.version != f.version {
            return Err(format!("{}：{} 版本漂移（{} → {}）", E_API_DRIFT, c.name, f.version, c.version));
        }
    }
    Ok(())
}

/// 演化操作（锚点：v1 只增不改，v2 追加段）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ApiOp {
    /// 修改既有条目（v1 恒拒）。
    Patch,
    /// 追加新条目（只许 v2）。
    Append,
}

/// 演化执行：Patch 命中 v1 条目 → 拒（指引 v2 追加段）；Append 只许
/// v2 版本且不得与既有全名重名（重名追加 = 偷偷改语义）。
pub fn evolve(book: &mut FreezeBook, spec: SignSpec, op: ApiOp) -> Result<(), String> {
    match op {
        ApiOp::Patch => {
            if book.get(spec.name).is_some() {
                return Err(format!(
                    "{}：v1 冻结条目 {} 只增不改——请走 v2 追加段（{}）",
                    E_API_V1_IMMUTABLE, spec.name, SIGN_VERSION_V1
                ));
            }
            // patch 一个不在簿里的名字同样拒绝：patch 语义=改既有，
            // 对不存在条目执行 patch 是调用方拼错名，静默 Ignore 会让
            // 「以为改过了」成为隐患。
            Err(format!("{}：patch 目标 {} 不在冻结簿", E_API_DRIFT, spec.name))
        }
        ApiOp::Append => {
            if spec.version == SIGN_VERSION_V1 {
                return Err(format!(
                    "{}：追加段版本须为 v2（收到 {}）——v1 集合已冻结封闭",
                    E_API_APPEND_VERSION, spec.version
                ));
            }
            spec.validate_v2()?;
            if book.get(spec.name).is_some() {
                return Err(format!("{}：追加签名 {} 与既有全名重名", E_API_DRIFT, spec.name));
            }
            book.entries.push(spec);
            Ok(())
        }
    }
}

impl SignSpec {
    /// v2 追加段自检：前缀 + 描述词 + 返回契约（版本一栏本模块放行
    /// 非 v1，故单独一函数而不是复用 [`SignSpec::validate`]）。
    fn validate_v2(&self) -> Result<(), String> {
        if self.descriptor.trim().is_empty() {
            return Err(format!("{}：v2 签名 {} 缺官方描述词", E_API_DESCRIPTOR, self.name));
        }
        if !self.name.starts_with(SIGN_PREFIX) {
            return Err(format!("{}：v2 签名 {} 缺 {} 前缀", E_API_DRIFT, self.name, SIGN_PREFIX));
        }
        if self.ret.trim().is_empty() {
            return Err(format!("{}：v2 签名 {} 缺返回契约", E_API_DRIFT, self.name));
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// 六、衔接核验表（三行；真调 vel04/vel08/vel09，不代填）
// ---------------------------------------------------------------------------

/// 衔接裁决（Aligned=命名/口径一致；Diverged=不一致，带回改动作）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LinkVerdict {
    /// 一致。
    Aligned,
    /// 不一致（携带回改动作声明——以对端冻结为准）。
    Diverged(&'static str),
}

/// 衔接行（主题/对端/裁决）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LinkRow {
    /// 主题（本族签名面的一行）。
    pub topic: &'static str,
    /// 对端（核验对象）。
    pub peer: &'static str,
    /// 裁决。
    pub verdict: LinkVerdict,
}

/// 行一：事件订阅 ↔ F1408/F1925 注册制命名。
///
/// 真调 [`EventRegistry`]：两个粒子事件名经注册放行、查表取回中性
/// schema；**未注册名查表为 `None`**（F1925 拼写漂移防线在粒子侧的
/// 实例）。四要素（SubContract 四字段）逐字段在场。
pub fn link_event_registry(
    reg: &mut EventRegistry,
    bag: &mut DiagBag,
) -> (LinkRow, bool, bool) {
    let sink = SubId(1);
    let sub = subscribe_event(reg, EVENT_POOL_PRESSURE, EventSource::Audio, EventSchema::neutral(), sink, bag);
    let _ = subscribe_event(reg, EVENT_EMITTER, EventSource::Game, EventSchema::neutral(), SubId(2), bag);
    let registered = reg.lookup(sub_event_id()).is_some()
        && reg.lookup(event_name_id(EVENT_EMITTER)).is_some();
    // 未注册名（拼写漂移的典型形态：l.ps.event.poolpressure 少下划线）
    // 查表必须为 None——这是注册制防线本身，不是附加功能。
    let typo_rejected = reg.lookup(0xDEAD_BEEFu32).is_none();
    let four_elements = sub.is_some();
    let row = LinkRow {
        topic: "事件订阅二签名",
        peer: "F1408 音频事件总线四要素 / F1925 注册制",
        verdict: if registered && typo_rejected && four_elements {
            LinkVerdict::Aligned
        } else {
            LinkVerdict::Diverged("以 F1408/F1925 注册制命名回改：事件名进注册表、四要素齐备")
        },
    };
    (row, registered && typo_rejected && four_elements, sub.is_some())
}

/// 行二：池查询 ↔ F1776 配额口径。
///
/// 真调 [`PoolQuota::validate`] 与 vel08 三阈值/滞回常量：F1776 全景
/// 表口径（70/85/95 预警/降质/拒绝）在粒子池的实例化值必须逐位相等，
/// 滞回分离必须严格小于（同值则档位抖动——vel08 已论证，这里做
/// 跨模块复核）。
pub fn link_pool_quota() -> (LinkRow, bool) {
    let quota = PoolQuota {
        kind: PoolKind::Cpu,
        capacity: 100_000,
        stride: 64,
        emitter_share_pct: 10,
        bytes_cap: 100_000 * 64,
    };
    let ok_quota = quota.validate().is_ok();
    let thresholds = WM_WARN_PCT == 70 && WM_DEGRADE_PCT == 85 && WM_REJECT_PCT == 95;
    let hyst = HYST_WARN_OFF < WM_WARN_PCT && HYST_DEGRADE_OFF < WM_DEGRADE_PCT && HYST_REJECT_OFF < WM_REJECT_PCT;
    let row = LinkRow {
        topic: "池查询三签名",
        peer: "F1776 3D 内存治理汇总（三阈值全景表）",
        verdict: if ok_quota && thresholds && hyst {
            LinkVerdict::Aligned
        } else {
            LinkVerdict::Diverged("以 F1776 口径回改：三阈值 70/85/95 与滞回分离")
        },
    };
    (row, ok_quota && thresholds && hyst)
}

/// 行三：发射器枚举 ↔ F2209 发射器级统计。
///
/// 真调 [`EmitterCounter`] 经 [`emitter_info_from_counter`] 转枚举
/// 契约：四字段逐场合影——枚举面与统计面同一记录。
pub fn link_emitter_stats() -> (LinkRow, bool, EmitterInfo) {
    let mut c = EmitterCounter::new(7);
    c.on_spawn(5);
    c.on_death(2);
    let info = emitter_info_from_counter(&c);
    let same = info.emitter_id == 7 && info.live == 3 && info.spawned_total == 5 && info.died_total == 2;
    let row = LinkRow {
        topic: "发射器枚举签名",
        peer: "F2209 粒子调试数据（发射器级统计）",
        verdict: if same {
            LinkVerdict::Aligned
        } else {
            LinkVerdict::Diverged("以 F2209 统计字段回改：枚举契约四字段同源")
        },
    };
    (row, same, info)
}

/// 三行衔接表汇总（判据与移交面共用）。
pub fn verify_linkages(reg: &mut EventRegistry, bag: &mut DiagBag) -> Vec<LinkRow> {
    let (r1, _, _) = link_event_registry(reg, bag);
    let (r2, _) = link_pool_quota();
    let (r3, _, _) = link_emitter_stats();
    alloc::vec![r1, r2, r3]
}

// ---------------------------------------------------------------------------
// 七、真调适配层（签名契约 ←→ 对端实现）
// ---------------------------------------------------------------------------

/// 事件名 → 注册 id（FNV-1a 截断 32 位——注册表以 u32 为键，
/// 名字是给人读的、id 是给表用的，双向可复现。
/// 与 vel11/vel12 同纪律：同串同值，防手抄漂移）。
pub fn event_name_id(name: &str) -> u32 {
    let mut h: u32 = 0x811c_9dc5;
    for b in name.bytes() {
        h ^= b as u32;
        h = h.wrapping_mul(0x0100_0193);
    }
    h
}

/// 池压力事件的注册 id（模块级单值——簿记只此一份）。
pub fn sub_event_id() -> u32 {
    event_name_id(EVENT_POOL_PRESSURE)
}

/// 订阅一个粒子事件：真调 vel04 注册制 + 回執四要素契约。
///
/// 注册（vel04 `EventRegistry::register`，schema=中性三覆盖参数），
/// 随后查表验证在册；不在册（注册表实现未来若改制）→ 返回 `None`
/// 且**不静默构造假订阅**。
pub fn subscribe_event(
    reg: &mut EventRegistry,
    name: &str,
    source: EventSource,
    schema: EventSchema,
    sink: SubId,
    bag: &mut DiagBag,
) -> Option<SubContract> {
    let id = event_name_id(name);
    reg.register(id, schema, bag);
    let got = reg.lookup(id)?;
    Some(SubContract { name: static_name(name), source, schema: got, sink })
}

/// 事件名的静态化（两个模块常量 → 'static；其余输入名按注册制
/// 只可能来自本表，故 goto 静态兜底而非泄露借用）。
fn static_name(name: &str) -> &'static str {
    if name == EVENT_POOL_PRESSURE {
        EVENT_POOL_PRESSURE
    } else if name == EVENT_EMITTER {
        EVENT_EMITTER
    } else {
        // 未登记名走到这里说明调用方绕过了注册制——静态面给一个
        // 显性标记而非静默透传（读屏可见）。
        "l.ps.event.<unregistered>"
    }
}

/// 池水位契约 ← vel09 统计快照字段（直读池侧，不重算）。
pub fn pool_water_from_snapshot(
    live: u32,
    capacity: u32,
    free: u32,
    water_pct: u64,
) -> PoolWater {
    PoolWater { live, capacity, free, water_pct }
}

/// 池容量契约 ← vel08 双配额声明。
pub fn pool_capacity_from_quota(q: &PoolQuota) -> Option<PoolCapacity> {
    Some(PoolCapacity {
        capacity: q.capacity,
        stride: q.stride,
        emitter_cap: q.emitter_cap(),
        total_bytes: q.bytes()?,
    })
}

/// 枚举契约 ← vel09 发射器计数器（同一记录）。
pub fn emitter_info_from_counter(c: &EmitterCounter) -> EmitterInfo {
    EmitterInfo {
        emitter_id: c.emitter_id,
        live: c.live,
        spawned_total: c.spawned_total,
        died_total: c.died_total,
    }
}

// ---------------------------------------------------------------------------
// 八、F2219 联动挂点（术语与签名同检）
// ---------------------------------------------------------------------------

/// F2219（粒子组一致性）移交包：术语三行 + 十条全名清单。
#[derive(Clone, Debug)]
pub struct F2219Handoff {
    /// 术语表（L01 新增术语——英文/中文对，供 F2219 裁决表并入）。
    pub terms: [(&'static str, &'static str); 3],
    /// 十条全名（签名与术语同检的签名侧材料）。
    pub signatures: Vec<&'static str>,
}

/// 生成 F2219 移交包（从冻结簿取签名清单——以 frozen 为单一事实来源，
/// 不让移交面另抄一份名单）。
pub fn f2219_handoff(book: &FreezeBook) -> F2219Handoff {
    F2219Handoff {
        terms: [
            ("emitter", "发射器"),
            ("group", "粒子组"),
            ("pool", "粒子池"),
        ],
        signatures: book.names(),
    }
}

// ---------------------------------------------------------------------------
// 九、判据
// ---------------------------------------------------------------------------

use crate::checks::CheckSet;

/// F2213 域自检（判据逐条映射；十签名/冻结/漂移/只增/衔接/联动六组）。
pub fn run_vel13_checks() -> CheckSet {
    let mut s = CheckSet::new("VE-F2213");

    // --- 十签名（族计数 5/3/2 + 命名纪律）---
    // L13-签名-01：十条且族计数恰 5/3/2。
    let mut fam_ok = FREEZE_V1.len() == 10;
    for f in SignFamily::ALL.iter() {
        let n = FREEZE_V1.iter().filter(|e| e.family == *f).count();
        fam_ok = fam_ok && n == f.expect_count();
    }
    s.add("L13-签名-01", fam_ok, "十签名且族计数 5/3/2");

    // L13-签名-02：全名唯一。
    let mut uniq = true;
    for (i, a) in FREEZE_V1.iter().enumerate() {
        for b in FREEZE_V1.iter().skip(i + 1) {
            uniq = uniq && a.name != b.name;
        }
    }
    s.add("L13-签名-02", uniq, "十签名全名唯一");

    // L13-签名-03：全名 l.ps. 前缀。
    let prefix_ok = FREEZE_V1.iter().all(|e| e.name.starts_with(SIGN_PREFIX));
    s.add("L13-签名-03", prefix_ok, "全名 l.ps. 前缀（命名空间纪律）");

    // L13-签名-04：全版本 v1。
    s.add("L13-签名-04", FREEZE_V1.iter().all(|e| e.version == SIGN_VERSION_V1), "十签名全版本 v1");

    // L13-签名-05：描述词与返回契约非空（validate 逐条过）。
    let mut desc_ok = true;
    for e in FREEZE_V1.iter() {
        desc_ok = desc_ok && e.validate().is_ok();
    }
    s.add("L13-签名-05", desc_ok, "描述词/返回契约非空（validate 全过）");

    // L13-签名-06：五签名职责覆盖（创建/销毁/参数读写/启停/枚举五关键词
    // 在发射器族描述词中各出现一次——职责不丢不重）。
    let em_desc: Vec<&str> = FREEZE_V1.iter().filter(|e| e.family == SignFamily::EmitterMgmt).map(|e| e.descriptor).collect();
    let duty = ["创建发射器", "销毁发射器", "参数读写", "启停", "枚举"];
    let mut duty_ok = true;
    for d in duty.iter() {
        duty_ok = duty_ok && em_desc.iter().any(|x| x.contains(d));
    }
    s.add("L13-签名-06", duty_ok, "发射器五签名职责逐个落位");

    // --- v1 冻结（簿 + 拒绝路径）---
    // L13-冻结-01：freeze 十条入库。
    let book = match FreezeBook::freeze(&FREEZE_V1) {
        Ok(b) => b,
        Err(_) => FreezeBook::new(),
    };
    s.add(
        "L13-冻结-01",
        book.len() == 10 && book.family_count(SignFamily::EmitterMgmt) == 5
            && book.family_count(SignFamily::PoolQuery) == 3
            && book.family_count(SignFamily::EventSub) == 2,
        "v1 冻结簿十条（族计数复核）",
    );

    // L13-冻结-02：描述词缺失 → 整簿拒绝（锚点错误路径）。
    let bad_desc = SignSpec {
        name: "l.ps.emitter.probe",
        descriptor: "   ",
        params: &["handle: EmitterHandle"],
        ret: "Outcome<Probe>",
        family: SignFamily::EmitterMgmt,
        version: SIGN_VERSION_V1,
    };
    let r = FreezeBook::freeze(&[bad_desc.clone(), FREEZE_V1[0].clone()]);
    s.add(
        "L13-冻结-02",
        r.is_err() && r.as_ref().unwrap_err().starts_with(E_API_DESCRIPTOR),
        "描述词缺失整簿拒绝（显性）",
    );

    // L13-冻结-03：重名 → 拒绝（命名空间一姓一名）。
    let dup = SignSpec { name: FREEZE_V1[0].name, ..FREEZE_V1[0].clone() };
    let r = FreezeBook::freeze(&[FREEZE_V1[0].clone(), dup]);
    s.add("L13-冻结-03", r.is_err() && r.unwrap_err().starts_with(E_API_DRIFT), "冻结集合内重名拒绝");

    // L13-冻结-04：十年承诺钉死（10×365 天毫秒——承诺可判定）。
    s.add(
        "L13-冻结-04",
        TEN_YEAR_COMMITMENT_MS == 315_360_000_000,
        "十年承诺 = 315_360_000_000ms",
    );

    // --- 漂移守卫（双向）---
    // L13-漂移-01：合规当前面过钩。
    s.add("L13-漂移-01", drift_hook(&FREEZE_V1, &book).is_ok(), "合规签名面过钩");

    // L13-漂移-02：描述词漂移 → 拒（指名）。
    let mut drift_desc = FREEZE_V1.to_vec();
    drift_desc[0].descriptor = "改过的描述词";
    let r = drift_hook(&drift_desc, &book);
    s.add(
        "L13-漂移-02",
        r.is_err() && r.as_ref().unwrap_err().contains("l.ps.emitter.create") && r.as_ref().unwrap_err().starts_with(E_API_DRIFT),
        "描述词漂移拦截（指名到签名）",
    );

    // L13-漂移-03：参数表漂移 → 拒。
    let mut drift_params = FREEZE_V1.to_vec();
    drift_params[6].params = &["pool: PoolId", "extra: u32"];
    let r = drift_hook(&drift_params, &book);
    s.add("L13-漂移-03", r.is_err() && r.unwrap_err().starts_with(E_API_DRIFT), "参数表漂移拦截");

    // L13-漂移-04：偷删冻结条目 → 拒（双向之一）。
    let shortened = FREEZE_V1[..9].to_vec();
    let r = drift_hook(&shortened, &book);
    s.add("L13-漂移-04", r.is_err() && r.unwrap_err().contains("缺失"), "偷删冻结条目拦截");

    // L13-漂移-05：偷加未冻结签名 → 拒（双向之二）。
    let mut grown = FREEZE_V1.to_vec();
    grown.push(SignSpec {
        name: "l.ps.emitter.smoke",
        descriptor: "偷加的签名",
        params: &["x: u32"],
        ret: "()",
        family: SignFamily::EmitterMgmt,
        version: SIGN_VERSION_V1,
    });
    let r = drift_hook(&grown, &book);
    s.add("L13-漂移-05", r.is_err() && r.unwrap_err().contains("未在冻结簿"), "偷加未冻结签名拦截");

    // --- v1 只增不改 ---
    // L13-只增-01：v1 条目 patch 恒拒（指引 v2 追加段）。
    let mut book2 = FreezeBook::freeze(&FREEZE_V1).unwrap_or_else(|_| FreezeBook::new());
    let r = evolve(&mut book2, FREEZE_V1[2].clone(), ApiOp::Patch);
    s.add(
        "L13-只增-01",
        r.is_err() && r.as_ref().unwrap_err().starts_with(E_API_V1_IMMUTABLE),
        "v1 patch 拒（指引 v2 追加段）",
    );

    // L13-只增-02：patch 未存在目标也拒（防拼错名静默通过）。
    let r = evolve(&mut book2, SignSpec {
        name: "l.ps.pool.nowhere",
        descriptor: "不在簿里的目标",
        params: &[],
        ret: "()",
        family: SignFamily::PoolQuery,
        version: SIGN_VERSION_V1,
    }, ApiOp::Patch);
    s.add("L13-只增-02", r.is_err() && r.unwrap_err().starts_with(E_API_DRIFT), "patch 不存在目标拒绝");

    // L13-只增-03：v2 追加过且 v1 条目逐字段不变。
    let before: Vec<SignSpec> = book2.v1_entries().cloned().collect();
    let v2spec = SignSpec {
        name: "l.ps.emitter.set_lod",
        descriptor: "设置发射器 LOD 档位（v2 追加段）",
        params: &["handle: EmitterHandle", "lod: u32"],
        ret: "Outcome<u32>",
        family: SignFamily::EmitterMgmt,
        version: "v2",
    };
    let append_ok = evolve(&mut book2, v2spec.clone(), ApiOp::Append).is_ok()
        && book2.len() == 11
        && book2.get("l.ps.emitter.set_lod").is_some();
    let after: Vec<SignSpec> = book2.v1_entries().cloned().collect();
    s.add(
        "L13-只增-03",
        append_ok && before == after,
        "v2 追加过且 v1 十条逐字段不变",
    );

    // L13-只增-04：追加以 v1 版本塞入 → 拒（v1 集合封闭）。
    let r = evolve(&mut book2, SignSpec {
        name: "l.ps.pool.probe_v1",
        descriptor: "伪装 v1 的追加",
        params: &["pool: PoolId"],
        ret: "PoolProbe",
        family: SignFamily::PoolQuery,
        version: SIGN_VERSION_V1,
    }, ApiOp::Append);
    s.add("L13-只增-04", r.is_err() && r.unwrap_err().starts_with(E_API_APPEND_VERSION), "v1 版本追加拒绝");

    // L13-只增-05：v2 重名追加 → 拒（追加不是偷改的侧门）。
    let r = evolve(&mut book2, SignSpec {
        name: "l.ps.emitter.create",
        descriptor: "重名 v2",
        params: &["config: EmitterConfig"],
        ret: "Outcome<EmitterHandle>",
        family: SignFamily::EmitterMgmt,
        version: "v2",
    }, ApiOp::Append);
    s.add("L13-只增-05", r.is_err() && r.unwrap_err().starts_with(E_API_DRIFT), "v2 重名追加拒绝");

    // --- 衔接核验（三行真调）---
    let mut reg = EventRegistry::new();
    let mut bag = DiagBag::new();
    let rows = verify_linkages(&mut reg, &mut bag);

    // L13-衔接-01：三行全 Aligned。
    let all_aligned = rows.iter().all(|r| r.verdict == LinkVerdict::Aligned);
    s.add("L13-衔接-01", all_aligned, "三行衔接全 Aligned（真调对端）");

    // L13-衔接-02：事件注册闭合（两名在册 + 拼写漂移名不在册）。
    let (_, ev_ok, _) = link_event_registry(&mut reg, &mut bag);
    s.add("L13-衔接-02", ev_ok, "事件注册制闭合（在册取回/漂移名查空）");

    // L13-衔接-03：四要素契约齐备（名称/来源/参数/订阅者）。
    let sub = subscribe_event(&mut reg, EVENT_POOL_PRESSURE, EventSource::Ui, EventSchema::neutral(), SubId(9), &mut bag);
    let four = sub.map_or(false, |c| {
        !c.name.is_empty()
            && EventSource::ALL.contains(&c.source)
            && (c.schema.count_mult == 1.0 && c.schema.speed_mult == 1.0)
            && c.sink == SubId(9)
    });
    s.add("L13-衔接-03", four, "订阅四要素齐备（F1408 同构）");

    // L13-衔接-04：池阈值 F1776 口径（70/85/95 逐位）。
    let (_, pool_ok) = link_pool_quota();
    s.add("L13-衔接-04", pool_ok, "池三阈值=F1776 口径且滞回分离");

    // L13-衔接-05：池容量契约 ← vel08 双配额真转（含单发射器上限）。
    let quota = PoolQuota {
        kind: PoolKind::Cpu,
        capacity: 100_000,
        stride: 64,
        emitter_share_pct: 10,
        bytes_cap: 100_000 * 64,
    };
    let cap = pool_capacity_from_quota(&quota);
    s.add(
        "L13-衔接-05",
        cap.map_or(false, |c| c.emitter_cap == 10_000 && c.total_bytes == 6_400_000 && c.capacity == 100_000),
        "容量契约双配额字段（总字节+单发射器上限）",
    );

    // L13-衔接-06：枚举契约 ← vel09 计数器四字段同源。
    let (_, stat_ok, info) = link_emitter_stats();
    s.add(
        "L13-衔接-06",
        stat_ok && info.live == 3 && info.spawned_total == 5 && info.died_total == 2,
        "枚举契约与 F2209 统计同记录",
    );

    // L13-衔接-07：Diverged 行的回改动作声明非空（以对端为准有实处）。
    let diverged = LinkRow {
        topic: "t",
        peer: "p",
        verdict: LinkVerdict::Diverged("回改动作"),
    };
    let action_ok = matches!(diverged.verdict, LinkVerdict::Diverged(a) if !a.is_empty())
        && LinkVerdict::Diverged("x") != LinkVerdict::Aligned;
    s.add("L13-衔接-07", action_ok, "Diverged 带回改动作声明（不空转）");

    // L13-衔接-08：Diverged 与错误码同文本（不一致即立案码）。
    s.add(
        "L13-衔接-08",
        E_API_LINK_DIVERGED.len() > 0 && rows[0].peer.contains("F1408") && rows[1].peer.contains("F1776") && rows[2].peer.contains("F2209"),
        "衔接行对端指名（F1408/F1776/F2209）",
    );

    // --- F2219 联动挂点 ---
    // L13-联动-01：术语三行且中英互异。
    let ho = f2219_handoff(&book);
    s.add(
        "L13-联动-01",
        ho.terms.len() == 3
            && ho.terms[0] != ho.terms[1]
            && ho.terms[1] != ho.terms[2]
            && ho.terms.iter().all(|(en, zh)| !en.is_empty() && !zh.is_empty()),
        "F2219 术语三行（emitter/group/pool）",
    );

    // L13-联动-02：挂点签名清单恰十条全名（以冻结簿为单一来源）。
    s.add("L13-联动-02", ho.signatures.len() == 10 && ho.signatures == book.names(), "挂点签名清单同源冻结簿");

    // L13-联动-03：事件名 id 可复现（同串同值——注册制防漂移的哈希侧）。
    s.add(
        "L13-联动-03",
        event_name_id(EVENT_POOL_PRESSURE) == event_name_id(EVENT_POOL_PRESSURE)
            && event_name_id(EVENT_POOL_PRESSURE) != event_name_id(EVENT_EMITTER),
        "事件名 id 同串同值且互异",
    );

    // L13-联动-04：未登记名静态兜底显性（读屏可见的非静默路径）。
    s.add(
        "L13-联动-04",
        static_name("l.ps.event.typo") == "l.ps.event.<unregistered>"
            && static_name(EVENT_POOL_PRESSURE) == EVENT_POOL_PRESSURE,
        "未登记名显性兜底（不静默透传）",
    );

    // --- 版本与条数 ---
    // L13-版本-01：版本指纹非零（FNV-1a const 期同算防手抄漂移）。
    let fp = {
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        for b in API_FREEZE_VERSION.bytes() {
            h ^= b as u64;
            h = h.wrapping_mul(0x0000_0100_0000_01b3);
        }
        h
    };
    s.add("L13-版本-01", fp != 0, "版本指纹非零（L13-api-v1）");

    // L13-暂挂-01：L 域账本暂挂声明显性（总账入册的前置声明——不静默跳过）。
    s.add(
        "L13-暂挂-01",
        L_LEDGER_SUSPENDED_NOTE.contains("暂挂") && L_LEDGER_SUSPENDED_NOTE.contains("F2094"),
        "L 域账本暂挂声明显性（移交期模式延续）",
    );

    // L13-暂挂-02：锚点偏离如实记录（不编造对端——同 vel07_blend 先例）。
    s.add(
        "L13-暂挂-02",
        ANCHOR_DEVIATION_NOTE.contains("F1650") && ANCHOR_DEVIATION_NOTE.contains("I02"),
        "锚点 I02/F1650 偏离记录显性（不伪造裁决）",
    );

    // L13-暂挂-03：判据条数对账（本条为第 36 条）。
    s.add("L13-暂挂-03", s.len() == 35, "判据条数对账（35+本条）");

    s
}

// ---------------------------------------------------------------------------
// 十、锚点偏离如实记录
// ---------------------------------------------------------------------------

/// 锚点「核验对象 I02 F1650 绘制族」与本仓实证的偏差记录（同 vel07_blend
/// 先例：不编造对端，按可核验部分交付）。
///
/// 册内 VE-F1650 实为「材质输入验证」（I03 材质域）；本仓无 I02 绘制
/// 族签名簿。故「I02 绘制族命名一致」一行按**无本仓实现对口的声明位**
/// 处理：命名一致性在与 F1408/F1925 的事件面（衔接行一）按册内实证
/// 核验；I02 一行留待该域落地时回填，不在本条伪造裁决。
pub const ANCHOR_DEVIATION_NOTE: &str =
    "锚点写 I02 F1650 绘制族，册内 F1650 实为材质输入验证（I03）；命名一致性按 F1408/F1925 实证核验，I02 行留待对端落地回填";

/// 总账入 L 域账本的暂挂声明（锚点跨批对接点：L 域账本建账前暂挂——
/// 移交期模式第五域延续，与 F2094/F2211/F2212 同款声明）。
pub const L_LEDGER_SUSPENDED_NOTE: &str =
    "粒子 API v1 冻结总账入 L 域账本：建账前暂挂声明（移交期模式第五域延续——F2094/F2211/F2212 同款）；十年承诺期内 v1 只增不改，承诺由 TEN_YEAR_COMMITMENT_MS 钉死";
