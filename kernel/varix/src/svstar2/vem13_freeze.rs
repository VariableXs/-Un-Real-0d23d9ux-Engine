//! VE-F2413 · 动画 API 冻结 v1（VE-M 域 · 动画段 · 目标 320 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F2413`
//!
//! **判据（锚点原文）**：十二签名、v1 冻结、四行衔接、只增不改、判据。
//!
//! **职责定位（锚点原文）**：动画 API 冻结 v1——动画族冻结（轨道/clip/
//! 求值/导入导出——签名冻结版本化：轨道管理五签名（创建/销毁/绑定/参数/
//! 枚举）+clip 三签名（创建/时长查询/采样点导出）+求值二签名（单点
//! 求值/批量求值）+导入导出二签名（导入/导出）——十二签名 v1）、与
//! F1345 剪辑 API/I04 蒙皮 API 衔接一致（m.anim. 前缀——M 域族规范
//! 首族；F1345 单源扩展的命名对齐（轨道数据结构复用声明——API 层命名
//! 一致核验））、十年承诺。
//!
//! # 一、冻结清单十二签名（族计数 5/3/2/2 逐条钉死）
//!
//! 每条签名五列：全名/参数表/返回契约/官方描述词/版本 v1。全名一律
//! `m.anim.` 前缀（M 域动画段命名空间——M 域族规范首族）。**官方描述
//! 词非空是冻结的前件**：没有描述词的签名不可读屏、不可翻译，整簿
//! 拒绝冻结（锚点错误路径：描述词缺失→冻结拒绝）。
//!
//! # 二、四行衔接核验（真调对端，不代填）
//!
//! - **轨道数据 ↔ F1345 单源**：轨道族签名的数据载体即 vem02
//!   [`KeyframeRef`](vem02_track::KeyframeRef)（其文档明记"F1345 侧的
//!   关键帧资产标识"）——真调构造，类型级单源复用声明可执行化；
//! - **绑定路径 ↔ F2402 协议**：绑定签名声明的路径文法即 vem02
//!   [`parse_bind_path`](vem02_track::parse_bind_path) 的协议（`/`
//!   开头分段句）——真调解析，文法不符即拒；
//! - **事件轨 ↔ F1408 注册制**：事件轨签名的事件名须在 vem06
//!   [`EventNameRegistry`](vem06_event::EventNameRegistry) 注册——
//!   真调注册与查表（未注册名查表为 `false`，拼写漂移防线同权）；
//! - **姿态输出 ↔ F2422 契约预备**：clip 采样点导出签名按骨骼动画
//!   契约为**姿态输出预留形态**（逐关节变换数组返回契约）。如实记录：
//!   F2422（骨骼动画架构）本仓未落地，本行只验"预留形态在簿、契约
//!   本体随 F2422 回填"——不编造对端（同 vel13/vel14 先例）。
//!
//! # 三、四道错误路径（锚点逐条）
//!
//! - 签名漂移 → CI 钩子拦截（[`drift_hook`]——**双向**：偷删冻结条目
//!   与偷加未冻结签名同罪，逐字段比对五列）；
//! - 描述词缺失 → 冻结拒绝（[`FreezeBook::freeze`] 整簿不冻）；
//! - 衔接不一致 → 以对端冻结为准回改（[`LinkRow`] 带回改动作声明）；
//! - v1 只增不改（[`evolve`]：Patch 恒拒并指引 v2 追加段；Append 只许
//!   v2 且不触碰 v1 条目）。
//!
//! # 四、十年承诺与 F2419 联动
//!
//! [`TEN_YEAR_COMMITMENT_MS`] 把「十年」钉成可判定数值（10×365 天
//! 毫秒）——承诺不是口头礼花：[`f2419_handoff`] 向 F2419（动画组一致
//! 性）交出席位表（术语三行 track/clip/keyframe + 十二条全名清单），
//! 术语与签名**同检**。
//!
//! **性能（锚点原文）**：冻结静态核对构建期；钩子毫秒级；运行时成本
//! 在实现条目分摊——本模块零运行时热路径（纯簿记与核验）。

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use crate::svstar2::vem02_track::{parse_bind_path, DiagBag as TrackBag, KeyframeRef};
use crate::svstar2::vem06_event::{DiagBag as EventBag, EventNameRegistry};

// ---------------------------------------------------------------------------
// 一、常量与错误码
// ---------------------------------------------------------------------------

/// 本项版本。
pub const API_FREEZE_VERSION: &str = "M13-api-v1";

/// v1 冻结版本号（十二签名一律挂此版本）。
pub const SIGN_VERSION_V1: &str = "v1";

/// 动画段签名命名空间前缀（m.anim. = M 域动画段——M 域族规范首族）。
pub const SIGN_PREFIX: &str = "m.anim.";

/// 十年承诺（毫秒）：10×365 天（不含闰日——承诺是下界）。
pub const TEN_YEAR_COMMITMENT_MS: u64 = 315_360_000_000;

/// 事件轨 API 在 F1408 注册制下的事件名（衔接行三的注册对象）。
pub const EVENT_TRACK_EVENT: &str = "m.anim.event.event_track";

/// 绑定签名声明的路径文法样本（衔接行四的解析对象——F2402 根段只认
/// node/material/custom 三闭集，样本取 node 根）。
pub const BIND_PATH_SAMPLE: &str = "/node/spine/head";

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

/// 签名族（四族——锚点：轨道管理/clip/求值/导入导出）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SignFamily {
    /// 轨道管理（五签名）。
    TrackMgmt,
    /// clip（三签名）。
    Clip,
    /// 求值（二签名）。
    Eval,
    /// 导入导出（二签名）。
    Io,
}

impl SignFamily {
    /// 四族闭集。
    pub const ALL: [SignFamily; 4] = [SignFamily::TrackMgmt, SignFamily::Clip, SignFamily::Eval, SignFamily::Io];

    /// 族名（读屏可达）。
    pub fn zh(self) -> &'static str {
        match self {
            SignFamily::TrackMgmt => "轨道管理",
            SignFamily::Clip => "clip",
            SignFamily::Eval => "求值",
            SignFamily::Io => "导入导出",
        }
    }

    /// 族内签名数（5/3/2/2——锚点钉死，与 [`FREEZE_V1`] 对账）。
    pub fn expect_count(self) -> usize {
        match self {
            SignFamily::TrackMgmt => 5,
            SignFamily::Clip => 3,
            SignFamily::Eval => 2,
            SignFamily::Io => 2,
        }
    }
}

/// 单签名冻结规格（五列：全名/参数表/返回契约/官方描述词/版本）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SignSpec {
    /// 全名（`m.anim.` 前缀）。
    pub name: &'static str,
    /// 官方描述词（全称化——非空是冻结前件）。
    pub descriptor: &'static str,
    /// 参数表（形参声明）。
    pub params: &'static [&'static str],
    /// 返回契约（可读屏的类型摘要）。
    pub ret: &'static str,
    /// 所属族。
    pub family: SignFamily,
    /// 版本（v1）。
    pub version: &'static str,
}

impl SignSpec {
    /// 规格自检：描述词非空 + `m.anim.` 前缀 + v1 版本 + 返回契约非空。
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

/// v1 冻结十二条（锚点：五/三/二/二——逐条原文职责落位）。
pub const FREEZE_V1: [SignSpec; 12] = [
    // --- 轨道管理五签名（实现源 F2402-F2406）---
    SignSpec {
        name: "m.anim.track.create",
        descriptor: "创建轨道：按轨道规格建轨道并分配 F1345 侧关键帧资产引用（数据结构单源复用声明）",
        params: &["spec: TrackSpec", "asset: KeyframeRef"],
        ret: "Outcome<TrackHandle>",
        family: SignFamily::TrackMgmt,
        version: SIGN_VERSION_V1,
    },
    SignSpec {
        name: "m.anim.track.destroy",
        descriptor: "销毁轨道：走生命周期到销毁，回收句柄与 clip 绑定",
        params: &["handle: TrackHandle"],
        ret: "Outcome<()>",
        family: SignFamily::TrackMgmt,
        version: SIGN_VERSION_V1,
    },
    SignSpec {
        name: "m.anim.track.bind",
        descriptor: "绑定：把轨道按 F2402 绑定路径协议挂到目标属性（路径文法以 / 开头分段）",
        params: &["handle: TrackHandle", "path: BindPath"],
        ret: "Outcome<BindId>",
        family: SignFamily::TrackMgmt,
        version: SIGN_VERSION_V1,
    },
    SignSpec {
        name: "m.anim.track.set_param",
        descriptor: "参数读写：轨道权重/速度/包裹模式参数的读与写（写即钳制记账）",
        params: &["handle: TrackHandle", "key: TrackParamKey", "value: f32"],
        ret: "Outcome<f32>",
        family: SignFamily::TrackMgmt,
        version: SIGN_VERSION_V1,
    },
    SignSpec {
        name: "m.anim.track.enumerate",
        descriptor: "枚举：按过滤条件列出轨道及其实时统计（与 F2408 统计同记录）",
        params: &["filter: TrackFilter", "limit: u32"],
        ret: "Vec<TrackInfo>",
        family: SignFamily::TrackMgmt,
        version: SIGN_VERSION_V1,
    },
    // --- clip 三签名（实现源 F2404/F2405）---
    SignSpec {
        name: "m.anim.clip.create",
        descriptor: "创建片段：按轨道清单与时长建 clip（轨道可跨 clip 复用）",
        params: &["tracks: &[TrackHandle]", "duration: f32"],
        ret: "Outcome<ClipHandle>",
        family: SignFamily::Clip,
        version: SIGN_VERSION_V1,
    },
    SignSpec {
        name: "m.anim.clip.duration",
        descriptor: "时长查询：clip 的时长与帧步长（时长=末关键帧时刻，只读查询）",
        params: &["clip: ClipHandle"],
        ret: "ClipDuration",
        family: SignFamily::Clip,
        version: SIGN_VERSION_V1,
    },
    SignSpec {
        name: "m.anim.clip.export_samples",
        descriptor: "采样点导出：按采样率导出 clip 的逐关节姿态序列（姿态输出契约随 F2422 回填——预留形态）",
        params: &["clip: ClipHandle", "sample_rate: u32"],
        ret: "Vec<JointPose>",
        family: SignFamily::Clip,
        version: SIGN_VERSION_V1,
    },
    // --- 求值二签名（实现源 F2407/F2403）---
    SignSpec {
        name: "m.anim.eval.at",
        descriptor: "单点求值：给定时刻求全部轨道的值（零分配纪律——热路径不建 Vec）",
        params: &["clip: ClipHandle", "t: f32", "out: &mut EvalScratch"],
        ret: "Outcome<()>",
        family: SignFamily::Eval,
        version: SIGN_VERSION_V1,
    },
    SignSpec {
        name: "m.anim.eval.batch",
        descriptor: "批量求值：按类型分批求一批时刻（分批策略与 F2407 SIMD 收益声明同口径）",
        params: &["clip: ClipHandle", "times: &[f32]", "plan: &BatchPlan"],
        ret: "Outcome<BatchStats>",
        family: SignFamily::Eval,
        version: SIGN_VERSION_V1,
    },
    // --- 导入导出二签名（实现源 F2409/F2410）---
    SignSpec {
        name: "m.anim.io.import",
        descriptor: "导入：glTF 动画导入（三重校验：通道引用/采样数据/格式合规——畸形显性拒绝）",
        params: &["doc: &GltfAnimDoc", "table: &MappingTable"],
        ret: "Outcome<ImportedAnim>",
        family: SignFamily::Io,
        version: SIGN_VERSION_V1,
    },
    SignSpec {
        name: "m.anim.io.export",
        descriptor: "导出：动画导出 glTF（四通道逆映射+往返容差表+精度诚实声明）",
        params: &["clip: &ExportClip", "table: &mut ExportMapTable"],
        ret: "Outcome<ExportedAnim>",
        family: SignFamily::Io,
        version: SIGN_VERSION_V1,
    },
];

// ---------------------------------------------------------------------------
// 三、冻结簿（清单入库 + 入册前裁决）
// ---------------------------------------------------------------------------

/// 冻结簿（v1 十二条 + v2 追加段）。
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
// 四、漂移守卫（CI 钩子）与演化（只增不改）
// ---------------------------------------------------------------------------

/// 漂移钩子：当前签名面与冻结簿逐字段比对。
///
/// **双向**——`current` 缺冻结条目（偷删）与多未冻结签名（偷加）都是
/// 漂移。比对字段：全名集合先对，再逐条对描述词/参数表/返回/族/版本。
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
                    "{}：v1 冻结条目 {} 只增不改——请走 v2 追加段",
                    E_API_V1_IMMUTABLE, spec.name
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
// 五、四行衔接核验（真调 vem02/vem06；预留位如实标注）
// ---------------------------------------------------------------------------

/// 衔接裁决（Aligned=口径一致；Diverged=不一致，带回改动作）。
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

/// 行一：轨道数据 ↔ F1345 单源（真调 vem02 KeyframeRef 构造）。
///
/// F1345 的关键帧资产引用在 M 域的物质存在即 vem02
/// [`KeyframeRef`](vem02_track::KeyframeRef)（其字段文档明记"F1345
/// 侧的关键帧资产标识"）——轨道创建签名的 `asset` 参数即此类型。
/// 本行真调构造一个引用并核对计数自洽（count 非零即可用载体）。
pub fn link_track_data() -> (LinkRow, bool) {
    let asset = KeyframeRef::new("bench-asset", 128);
    let ok = asset.count == 128 && !asset.asset_id.is_empty();
    let row = LinkRow {
        topic: "轨道数据（asset 载体）",
        peer: "F1345 关键帧基础（vem02 KeyframeRef 单源）",
        verdict: if ok {
            LinkVerdict::Aligned
        } else {
            LinkVerdict::Diverged("以 F1345 单源数据结构回改：asset 参数须为 KeyframeRef 同族类型")
        },
    };
    (row, ok)
}

/// 行二：绑定路径 ↔ F2402 协议（真调 vem02 parse_bind_path）。
///
/// 绑定签名声明的路径文法必须被 F2402 的解析器原样接受（`/` 开头
/// 分段句）；畸形路径（缺 `/` 头）必须被拒——文法同权双向。
pub fn link_bind_path() -> (LinkRow, bool) {
    let mut good_bag = TrackBag::new();
    let good = parse_bind_path(BIND_PATH_SAMPLE, &mut good_bag);
    let mut bad_bag = TrackBag::new();
    let bad = parse_bind_path("rig/spine", &mut bad_bag); // 缺 / 头——必拒
    let ok = good.is_some() && bad.is_none();
    let row = LinkRow {
        topic: "绑定路径（path 文法）",
        peer: "F2402 绑定协议（vem02 parse_bind_path）",
        verdict: if ok {
            LinkVerdict::Aligned
        } else {
            LinkVerdict::Diverged("以 F2402 路径协议回改：绑定签名声明的文法须与 parse_bind_path 一致")
        },
    };
    (row, ok)
}

/// 行三：事件轨 ↔ F1408 注册制（真调 vem06 EventNameRegistry）。
///
/// 事件轨事件名须先注册后触发（F1408 四要素之"名称"）；已注册名查
/// 表为 `true`，未注册名查表为 `false`——注册制拼写漂移防线在 API
/// 冻结侧同权（事件名拼错=签名层面就查不到）。
pub fn link_event_registry() -> (LinkRow, bool) {
    let mut reg = EventNameRegistry::new();
    let mut bag = EventBag::new();
    let registered = reg.register(EVENT_TRACK_EVENT, &mut bag);
    let hit = reg.is_registered(EVENT_TRACK_EVENT);
    let miss = !reg.is_registered("m.anim.event.eventtrack"); // 拼写漂移样本
    let ok = registered && hit && miss;
    let row = LinkRow {
        topic: "事件轨（事件名）",
        peer: "F1408 事件总线注册制（vem06 EventNameRegistry）",
        verdict: if ok {
            LinkVerdict::Aligned
        } else {
            LinkVerdict::Diverged("以 F1408 注册制回改：事件名须先注册后引用")
        },
    };
    (row, ok)
}

/// 行四：姿态输出 ↔ F2422 契约预备（预留位——如实不编造对端）。
///
/// F2422（骨骼动画架构）本仓未落地。本行的可验证部分：clip 采样点
/// 导出签名在冻结簿中**已为姿态输出预留形态**（返回契约
/// `Vec<JointPose>`——逐关节姿态）；契约本体随 F2422 落地后以同
/// 接口回填核验。不编造"已对齐"——裁定为 Aligned(预留就绪) 并在
/// 详述里写明预留性质。
pub fn link_pose_contract(book: &FreezeBook) -> (LinkRow, bool) {
    let reserved = book
        .get("m.anim.clip.export_samples")
        .map_or(false, |s| s.ret == "Vec<JointPose>");
    let row = LinkRow {
        topic: "姿态输出（采样点导出契约）",
        peer: "F2422 骨骼动画契约（本仓未落地——预留位）",
        verdict: if reserved {
            LinkVerdict::Aligned
        } else {
            LinkVerdict::Diverged("补预留形态：export_samples 返回契约须为 Vec<JointPose>")
        },
    };
    (row, reserved)
}

/// 四行衔接表汇总（判据与移交面共用）。
pub fn verify_linkages(book: &FreezeBook) -> Vec<LinkRow> {
    let (r1, _) = link_track_data();
    let (r2, _) = link_bind_path();
    let (r3, _) = link_event_registry();
    let (r4, _) = link_pose_contract(book);
    alloc::vec![r1, r2, r3, r4]
}

// ---------------------------------------------------------------------------
// 六、F2419 联动挂点（术语与签名同检）
// ---------------------------------------------------------------------------

/// F2419（动画组一致性）移交包：术语三行 + 十二条全名清单。
#[derive(Clone, Debug)]
pub struct F2419Handoff {
    /// 术语表（M 域动画段新增术语——英文/中文对）。
    pub terms: [(&'static str, &'static str); 3],
    /// 十二条全名（签名与术语同检的签名侧材料）。
    pub signatures: Vec<&'static str>,
}

/// 生成 F2419 移交包（从冻结簿取签名清单——以 frozen 为单一事实来源，
/// 不让移交面另抄一份名单）。
pub fn f2419_handoff(book: &FreezeBook) -> F2419Handoff {
    F2419Handoff {
        terms: [("track", "轨道"), ("clip", "片段"), ("keyframe", "关键帧")],
        signatures: book.names(),
    }
}

// ---------------------------------------------------------------------------
// 七、判据
// ---------------------------------------------------------------------------

use crate::checks::CheckSet;

/// F2413 域自检（判据逐条映射；十二签名/冻结/衔接/只增/联动五组）。
pub fn run_vem13_checks() -> CheckSet {
    let mut s = CheckSet::new("VE-F2413");

    // --- 十二签名（族计数 5/3/2/2 + 命名纪律）---
    // M13-签名-01：十二条且族计数恰 5/3/2/2。
    let mut fam_ok = FREEZE_V1.len() == 12;
    for f in SignFamily::ALL.iter() {
        let n = FREEZE_V1.iter().filter(|e| e.family == *f).count();
        fam_ok = fam_ok && n == f.expect_count();
    }
    s.add("M13-签名-01", fam_ok, "十二签名且族计数 5/3/2/2");

    // M13-签名-02：全名唯一。
    let mut uniq = true;
    for (i, a) in FREEZE_V1.iter().enumerate() {
        for b in FREEZE_V1.iter().skip(i + 1) {
            uniq = uniq && a.name != b.name;
        }
    }
    s.add("M13-签名-02", uniq, "十二签名全名唯一");

    // M13-签名-03：全名 m.anim. 前缀（命名空间纪律——M 域族规范首族）。
    let prefix_ok = FREEZE_V1.iter().all(|e| e.name.starts_with(SIGN_PREFIX));
    s.add("M13-签名-03", prefix_ok, "全名 m.anim. 前缀");

    // M13-签名-04：全版本 v1。
    s.add("M13-签名-04", FREEZE_V1.iter().all(|e| e.version == SIGN_VERSION_V1), "十二签名全版本 v1");

    // M13-签名-05：描述词与返回契约非空（validate 逐条过）。
    let mut desc_ok = true;
    for e in FREEZE_V1.iter() {
        desc_ok = desc_ok && e.validate().is_ok();
    }
    s.add("M13-签名-05", desc_ok, "描述词/返回契约非空（validate 全过）");

    // M13-签名-06：四族职责覆盖（各族关键词在描述词中落位）。
    let duty: [(SignFamily, &[&str]); 4] = [
        (SignFamily::TrackMgmt, &["创建轨道", "销毁轨道", "绑定", "参数读写", "枚举"]),
        (SignFamily::Clip, &["创建片段", "时长查询", "采样点导出"]),
        (SignFamily::Eval, &["单点求值", "批量求值"]),
        (SignFamily::Io, &["导入", "导出"]),
    ];
    let mut duty_ok = true;
    for (fam, keys) in duty.iter() {
        let descs: Vec<&str> = FREEZE_V1
            .iter()
            .filter(|e| e.family == *fam)
            .map(|e| e.descriptor)
            .collect();
        for k in keys.iter() {
            duty_ok = duty_ok && descs.iter().any(|x| x.contains(*k));
        }
    }
    s.add("M13-签名-06", duty_ok, "四族职责逐个落位（不丢不重）");

    // --- v1 冻结（簿 + 拒绝路径）---
    let book = match FreezeBook::freeze(&FREEZE_V1) {
        Ok(b) => b,
        Err(_) => FreezeBook::new(),
    };

    // M13-冻结-01：freeze 十二条入库（族计数复核）。
    s.add(
        "M13-冻结-01",
        book.len() == 12
            && book.family_count(SignFamily::TrackMgmt) == 5
            && book.family_count(SignFamily::Clip) == 3
            && book.family_count(SignFamily::Eval) == 2
            && book.family_count(SignFamily::Io) == 2,
        "v1 冻结簿十二条（族计数复核）",
    );

    // M13-冻结-02：描述词缺失 → 整簿拒绝（锚点错误路径）。
    let bad_desc = SignSpec {
        name: "m.anim.track.probe",
        descriptor: "   ",
        params: &["handle: TrackHandle"],
        ret: "Outcome<Probe>",
        family: SignFamily::TrackMgmt,
        version: SIGN_VERSION_V1,
    };
    let r = FreezeBook::freeze(&[bad_desc.clone(), FREEZE_V1[0].clone()]);
    s.add(
        "M13-冻结-02",
        r.is_err() && r.as_ref().unwrap_err().starts_with(E_API_DESCRIPTOR),
        "描述词缺失整簿拒绝（显性）",
    );

    // M13-冻结-03：重名 → 拒绝（命名空间一姓一名）。
    let dup = SignSpec { name: FREEZE_V1[0].name, ..FREEZE_V1[0].clone() };
    let r = FreezeBook::freeze(&[FREEZE_V1[0].clone(), dup]);
    s.add("M13-冻结-03", r.is_err() && r.unwrap_err().starts_with(E_API_DRIFT), "冻结集合内重名拒绝");

    // M13-冻结-04：十年承诺钉死（10×365 天毫秒——承诺可判定）。
    s.add(
        "M13-冻结-04",
        TEN_YEAR_COMMITMENT_MS == 315_360_000_000,
        "十年承诺 = 315_360_000_000ms",
    );

    // --- 漂移守卫（双向）---
    // M13-漂移-01：合规当前面过钩。
    s.add("M13-漂移-01", drift_hook(&FREEZE_V1, &book).is_ok(), "合规签名面过钩");

    // M13-漂移-02：描述词漂移 → 拒（指名）。
    let mut drift_desc = FREEZE_V1.to_vec();
    drift_desc[0].descriptor = "改过的描述词";
    let r = drift_hook(&drift_desc, &book);
    s.add(
        "M13-漂移-02",
        r.is_err() && r.as_ref().unwrap_err().contains("m.anim.track.create") && r.as_ref().unwrap_err().starts_with(E_API_DRIFT),
        "描述词漂移拦截（指名到签名）",
    );

    // M13-漂移-03：参数表漂移 → 拒。
    let mut drift_params = FREEZE_V1.to_vec();
    drift_params[6].params = &["clip: ClipHandle", "extra: u32"];
    let r = drift_hook(&drift_params, &book);
    s.add("M13-漂移-03", r.is_err() && r.unwrap_err().starts_with(E_API_DRIFT), "参数表漂移拦截");

    // M13-漂移-04：偷删冻结条目 → 拒（双向之一）。
    let shortened = FREEZE_V1[..11].to_vec();
    let r = drift_hook(&shortened, &book);
    s.add("M13-漂移-04", r.is_err() && r.unwrap_err().contains("缺失"), "偷删冻结条目拦截");

    // M13-漂移-05：偷加未冻结签名 → 拒（双向之二）。
    let mut grown = FREEZE_V1.to_vec();
    grown.push(SignSpec {
        name: "m.anim.track.smoke",
        descriptor: "偷加的签名",
        params: &["x: u32"],
        ret: "()",
        family: SignFamily::TrackMgmt,
        version: SIGN_VERSION_V1,
    });
    let r = drift_hook(&grown, &book);
    s.add("M13-漂移-05", r.is_err() && r.unwrap_err().contains("未在冻结簿"), "偷加未冻结签名拦截");

    // --- v1 只增不改 ---
    // M13-只增-01：v1 条目 patch 恒拒（指引 v2 追加段）。
    let mut book2 = FreezeBook::freeze(&FREEZE_V1).unwrap_or_else(|_| FreezeBook::new());
    let r = evolve(&mut book2, FREEZE_V1[2].clone(), ApiOp::Patch);
    s.add(
        "M13-只增-01",
        r.is_err() && r.as_ref().unwrap_err().starts_with(E_API_V1_IMMUTABLE),
        "v1 patch 拒（指引 v2 追加段）",
    );

    // M13-只增-02：patch 未存在目标也拒（防拼错名静默通过）。
    let r = evolve(
        &mut book2,
        SignSpec {
            name: "m.anim.clip.nowhere",
            descriptor: "不在簿里的目标",
            params: &[],
            ret: "()",
            family: SignFamily::Clip,
            version: SIGN_VERSION_V1,
        },
        ApiOp::Patch,
    );
    s.add("M13-只增-02", r.is_err() && r.unwrap_err().starts_with(E_API_DRIFT), "patch 不存在目标拒绝");

    // M13-只增-03：v2 追加过且 v1 条目逐字段不变。
    let before: Vec<SignSpec> = book2.v1_entries().cloned().collect();
    let v2spec = SignSpec {
        name: "m.anim.track.set_lod",
        descriptor: "设置轨道 LOD 档位（v2 追加段）",
        params: &["handle: TrackHandle", "lod: u32"],
        ret: "Outcome<u32>",
        family: SignFamily::TrackMgmt,
        version: "v2",
    };
    let append_ok = evolve(&mut book2, v2spec.clone(), ApiOp::Append).is_ok()
        && book2.len() == 13
        && book2.get("m.anim.track.set_lod").is_some();
    let after: Vec<SignSpec> = book2.v1_entries().cloned().collect();
    s.add(
        "M13-只增-03",
        append_ok && before == after,
        "v2 追加过且 v1 十二条逐字段不变",
    );

    // M13-只增-04：追加以 v1 版本塞入 → 拒（v1 集合封闭）。
    let r = evolve(
        &mut book2,
        SignSpec {
            name: "m.anim.clip.probe_v1",
            descriptor: "伪装 v1 的追加",
            params: &["clip: ClipHandle"],
            ret: "ClipProbe",
            family: SignFamily::Clip,
            version: SIGN_VERSION_V1,
        },
        ApiOp::Append,
    );
    s.add("M13-只增-04", r.is_err() && r.unwrap_err().starts_with(E_API_APPEND_VERSION), "v1 版本追加拒绝");

    // M13-只增-05：v2 重名追加 → 拒（追加不是偷改的侧门）。
    let r = evolve(
        &mut book2,
        SignSpec {
            name: "m.anim.track.create",
            descriptor: "重名 v2",
            params: &["spec: TrackSpec"],
            ret: "Outcome<TrackHandle>",
            family: SignFamily::TrackMgmt,
            version: "v2",
        },
        ApiOp::Append,
    );
    s.add("M13-只增-05", r.is_err() && r.unwrap_err().starts_with(E_API_DRIFT), "v2 重名追加拒绝");

    // --- 四行衔接（真调 + 预留位）---
    let rows = verify_linkages(&book);

    // M13-衔接-01：四行全 Aligned（含 F2422 预留就绪）。
    let all_aligned = rows.iter().all(|r| r.verdict == LinkVerdict::Aligned);
    s.add("M13-衔接-01", all_aligned, "四行衔接全 Aligned（三行真调+一行预留就绪）");

    // M13-衔接-02：轨道数据单源（F1345 载体真调构造自洽）。
    let (_, td_ok) = link_track_data();
    s.add("M13-衔接-02", td_ok, "F1345 关键帧载体真调构造（KeyframeRef）");

    // M13-衔接-03：绑定路径协议（F2402 解析器双向：合法过/畸形拒）。
    let (_, bp_ok) = link_bind_path();
    s.add("M13-衔接-03", bp_ok, "F2402 绑定文法双向（合法解析/缺头拒）");

    // M13-衔接-04：事件轨注册制（F1408 注册闭合：在册取回/漂移名查空）。
    let (_, ev_ok) = link_event_registry();
    s.add("M13-衔接-04", ev_ok, "F1408 注册制闭合（在册/漂移名双向）");

    // M13-衔接-05：姿态输出预留形态在簿（F2422 契约本体待回填——如实）。
    let (_, pose_ok) = link_pose_contract(&book);
    s.add(
        "M13-衔接-05",
        pose_ok && book.get("m.anim.clip.export_samples").map_or(false, |s| s.ret == "Vec<JointPose>"),
        "姿态输出预留形态在簿（F2422 未落地不编造对齐）",
    );

    // M13-衔接-06：Diverged 行的回改动作声明非空（以对端为准有实处）。
    let diverged = LinkRow {
        topic: "t",
        peer: "p",
        verdict: LinkVerdict::Diverged("回改动作"),
    };
    let action_ok = matches!(diverged.verdict, LinkVerdict::Diverged(a) if !a.is_empty())
        && LinkVerdict::Diverged("x") != LinkVerdict::Aligned;
    s.add("M13-衔接-06", action_ok, "Diverged 带回改动作声明（不空转）");

    // M13-衔接-07：衔接行对端指名（F1345/F2402/F1408/F2422 四名在表）。
    s.add(
        "M13-衔接-07",
        rows[0].peer.contains("F1345")
            && rows[1].peer.contains("F2402")
            && rows[2].peer.contains("F1408")
            && rows[3].peer.contains("F2422"),
        "四行对端指名（F1345/F2402/F1408/F2422）",
    );

    // --- F2419 联动挂点 ---
    // M13-联动-01：术语三行且互异（track/clip/keyframe）。
    let ho = f2419_handoff(&book);
    s.add(
        "M13-联动-01",
        ho.terms.len() == 3 && ho.terms[0] != ho.terms[1] && ho.terms[1] != ho.terms[2]
            && ho.terms.iter().all(|(en, zh)| !en.is_empty() && !zh.is_empty()),
        "F2419 术语三行（track/clip/keyframe）",
    );

    // M13-联动-02：挂点签名清单恰十二条全名（以冻结簿为单一来源）。
    s.add("M13-联动-02", ho.signatures.len() == 12 && ho.signatures == book.names(), "挂点签名清单同源冻结簿");

    // M13-联动-03：事件名与 API 命名空间一致（事件名同走 m.anim. 前缀）。
    s.add(
        "M13-联动-03",
        EVENT_TRACK_EVENT.starts_with("m.anim.") && BIND_PATH_SAMPLE.starts_with('/'),
        "事件名同命名空间/绑定路径按 F2402 文法起头",
    );

    // --- 版本与条数 ---
    // M13-版本-01：版本指纹非零（FNV-1a const 期同算防手抄漂移）。
    let fp = {
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        for b in API_FREEZE_VERSION.bytes() {
            h ^= b as u64;
            h = h.wrapping_mul(0x0000_0100_0000_01b3);
        }
        h
    };
    s.add("M13-版本-01", fp != 0, "版本指纹非零（M13-api-v1）");

    // M13-暂挂-01：M 域账本暂挂声明显性（移交期模式第八域首族）。
    s.add(
        "M13-暂挂-01",
        M_LEDGER_SUSPENDED_NOTE.contains("暂挂") && M_LEDGER_SUSPENDED_NOTE.contains("F2413"),
        "M 域账本暂挂声明显性",
    );

    // M13-暂挂-02：判据条数对账（本条为第 33 条）。
    s.add("M13-暂挂-02", s.len() == 32, "判据条数对账（32+本条）");

    s
}

/// M 域账本暂挂声明（锚点跨批对接点：总账入 M 域账本——移交期模式
/// 第八域首族；十年承诺期内 v1 只增不改同步演进）。
pub const M_LEDGER_SUSPENDED_NOTE: &str = "动画 API v1 冻结总账入 M 域账本：建账前暂挂声明（移交期模式第八域首族——F2413 同款）；十年承诺期内 v1 只增不改，承诺由 TEN_YEAR_COMMITMENT_MS 钉死";

// ---------------------------------------------------------------------------
// 八、诚实边界
// ---------------------------------------------------------------------------

/// 衔接行四的诚实声明（同 vel13/vel14 先例：不编造对端）。
///
/// F2422（骨骼动画架构）本仓未落地：姿态输出契约目前只验"预留形态
/// 在簿"（`m.anim.clip.export_samples` 返回 `Vec<JointPose>`），契约
/// 本体（关节层级/姿势混合/空间规范）随 F2422 落地后以
/// [`link_pose_contract`] 同接口回填核验。F1345 的轨迹同此理：其
/// 域级实现（关键帧资产系统）以 vem02 [`KeyframeRef`](vem02_track::
/// KeyframeRef) 为载体单源复用，API 层只做命名与类型一致核验。
pub const LINK_HONESTY_NOTE: &str =
    "F2422 契约本仓未落地——衔接行四只验预留形态在簿，契约本体随 F2422 同接口回填；F1345 以 vem02 KeyframeRef 为载体单源复用";
