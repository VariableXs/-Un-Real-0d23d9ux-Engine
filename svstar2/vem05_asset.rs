//! VE-F2405 · 动画曲线资产（判据逐条对应，见下表）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F2405`
//!
//! 判据映射（锚点原文 → 本模块落点）：
//! - **`m.anim.` 段**（曲线段/轨道段/clip 段/元数据段 + 头 magic/版本/哈希）→
//!   [`SegmentKind`]、[`AnimAsset`]、[`export_container`]、[`import_container`]、
//!   [`CONTAINER_MAGIC`] / [`CONTAINER_VERSION`] / 段内 `__h` 与根 `content_hash`；
//! - **生态单点**（F1942 段枚举注册，L 域 `l.fluid.`/`l.timeline.` 之后的第三域段）→
//!   [`ECO_SEGMENTS`] + [`is_registered`] + [`register_index`]；
//! - **三件套复用**（F1948 签名分级/导入清洗/审计留痕 + F1956 版本链/迁移器/备份）→
//!   [`classify_signature`] / [`SigTier`]、[`scrub_for_export`]、[`AuditEntry`]、
//!   [`MIGRATION_TABLE`] / [`migrate_json`]、[`backup`] / [`restore`]；
//! - **往返零损失**（导出→导入→再导出，两次导出逐位 diff=0）→
//!   [`round_trip`] + [`RoundTripVerdict`]（含漂移定位到丢失段）。
//!
//! 错误路径与降级矩阵（锚点原文五条）：
//! 1. 容器校验失败（哈希/段损坏）→ **三要素拒绝 + 定位段**（[`SegmentHashMismatch`]
//!    带 `segment` 字段，不只说"校验失败"）；
//! 2. 版本过旧无迁移链 → **显性拒绝**（[`NoMigrationPath`]），不猜、不回落默认版；
//! 3. 段类型未知（未来版本）→ **跳过 + 兼容声明**（[`SegmentUnknown`] 为告警，
//!    不阻断——前向兼容家族）；
//! 4. 签名缺失 → **未审提示不阻断**（[`SignatureAbsent`] 为告警；这是 F1948 的
//!    平衡语义：阻断会让未署名资产完全不可用，那是隐私红线的过度执行）；
//! 5. 往返不一致 → **P1 立案**（[`RoundTripDrift`] 为错误，[`localize_drift`]
//!    定位到具体丢失段）。
//!
//! # 关键决策
//!
//! **1. 曲线段存「值的展平数组」，不存每帧一个数组。**
//! `values.len() == times.len() × comps`，四分量曲线按 `[t0c0,t0c1,t0c2,t0c3,t1c0,…]`
//! 顺序排开。理由：JSON 与二进制都只能表达一维数组，若写成"帧数组的数组"，
//! 解析时每帧要单独分配，且往返时帧内分量顺序极易写反（`c0c1` 与 `c1c0` 拼错
//! 在数值上仍"看起来合理"，是动画资产最隐蔽的数据损坏）。展平后分量下标是
//! `i*comps + c` 这一条纯算术，无歧义可写断言对拍。
//!
//! **2. 往返保真按「字节级」而非「数值等价」判定。**
//! 两次导出的 JSON 文本必须**逐字节相同**，不是"解析后数值相等"。数值相等更宽松，
//! 会放过两类真实损坏：`-0.0` 变 `0.0`（符号位丢了，某些插值器在除法里会翻转
//! 结果方向），以及分量顺序错位（数值集合相同、语义不同）。字节级判定把这两类
//! 一次抓住，代价是格式器必须确定性——见下条。
//!
//! **3. 序列化器必须确定性：保序对象 + 定长十六进制哈希 + 最短往返浮点。**
//! - 对象用 `Vec<(String, Json)>` 保序而非 `BTreeMap` 排序：段序（meta→curves→
//!   tracks→clips）是**语义**（读的人按这个顺序看），排序会把它变成字典序；
//! - 哈希一律 `{:016x}` 定长十六进制：变长哈希（去掉前导零）在数值相等时可能
//!   产生两种文本，往返就会假报漂移；
//! - 浮点用 Rust `Display`（最短往返表示，`parse` 回来逐位相同），**但导出前
//!   必须拒绝非有限值**——`f32::INFINITY` 的 Display 是 `inf`，那不是 JSON 数字，
//!   解析器读到会当场失败。宁可导出前拒绝，也不产出自己读不回来的文件。
//!
//! **4. 段哈希与内容哈希都排除自身字段后计算（无循环依赖）。**
//! 段内 `__h` 字段的哈希按「`__h` 置空后的规范序列化」计算；根 `content_hash`
//! 只覆盖 `magic + version + segments` 子树（`content_hash` 本身在子树之外）。
//! 这样"校验自己"不构成循环，且改任何一个字节都能被定位到具体段。
//!
//! **5. 未知段跳过而非报错，是前向兼容家族的规定动作，但仍要留下声明。**
//! 未来版本加了新段，老版本读不到就整文件拒绝，等于把格式锁死在单版本。跳过 +
//! 告警的代价是"我以为读全了"——所以跳过必须产生一条**指名段名**的告警，
//! 调用方能看见少了什么。
//!
//! **6. 签名缺失不阻断，但清洗照做。**
//! [`scrub_for_export`] 无条件剥离绝对路径与机器 id（隐私红线，与签名在不在
//! 无关）；而 [`classify_signature`] 产出的 `Absent` 只降为告警。理由：隐私风险
//! 来自**携带本机信息**，不是来自"没署名"；把两者绑在一起会让匿名资产无法分享。
//!
//! **7. 迁移链是显式表，不是 `while version < target { version += 1 }`。**
//! [`MIGRATION_TABLE`] 逐跳声明可达关系；不可达即 [`NoMigrationPath`] 显式拒绝。
//! 隐式逐版本升级会在某版本「没有迁移器但恰好不需要改」时静默通过——那正是
//! 数据悄悄变形的地方。
//!
//! # 单源纪律
//!
//! 本模块**不定义**轨道类、插值器族、绑定路径语法、轨道容器——那四样全部来自
//! [`vem02_track`](crate::svstar2::vem02_track)（F1345 扩展，F2402 落位）。
//! 本模块只做一件事：把关键帧序列**搬运**成开放 JSON 容器，再搬运回来。
//! 搬运完用 [`remount`] 把资产重挂回 F2402 的 `TrackContainer`——这是「搬运回来
//! 的东西真的能被上游接住」的唯一有效证明，光比对 JSON 文本不够。
//!
//! 零墙钟、零 IO、零外部依赖、零全局可变状态；时间用 `u32` tick 注入。

extern crate alloc;

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

use crate::svstar2::vem02_track::{
    parse_bind_path, DiagBag, InterpKind, KeyframeRef, MountInput, TrackClass, TrackContainer,
    TrackPayload, WEIGHT_MAX, WEIGHT_MIN,
};

// ===========================================================================
// §0 诊断码与结果面
// ===========================================================================

/// 资产诊断码全集。
///
/// **分立纪律**：处置方向相反的状态不得共用码。
/// - [`SegmentHashMismatch`]（拒绝）与 [`SegmentUnknown`]（跳过放行）方向相反；
/// - [`SignatureAbsent`]（告警不阻断）与 [`SignatureWeak`]（告警）虽同向但语义
///   不同（一个是没填、一个是填了但不全），分开才能统计"该补的"而非"没填的"；
/// - [`ValueOutOfDomain`]（拒绝装载）与 [`NonFiniteValue`]（拒绝导出）分立：
///   前者是别人的文件坏了，后者是我们自己要产出坏文件。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum AssetDiag {
    /// JSON 语法非法。
    SyntaxMalformed,
    /// 嵌套深度超限（防栈溢出）。
    DepthExceeded,
    /// 根 `schema` 字段不是 `m.anim.`。
    SchemaMismatch,
    /// 魔数不匹配（不是本容器格式）。
    MagicMismatch,
    /// 版本高于当前实现（前向：接受 + 声明）。
    VersionFromFuture,
    /// 版本过旧且迁移链不可达。
    NoMigrationPath,
    /// 迁移器执行失败。
    MigrationFailed,
    /// 未注册的段（跳过 + 兼容声明）。
    SegmentUnknown,
    /// 声明的四类段缺一。
    SegmentMissing,
    /// 段内容哈希失配（损坏）。
    SegmentHashMismatch,
    /// 段形状非法（该段不是对象 / 缺必填字段）。
    SegmentShapeInvalid,
    /// 曲线值条数与 `times.len() × comps` 不一致。
    CurveLengthMismatch,
    /// 曲线时间戳非严格递增。
    CurveTimeNotMonotonic,
    /// 轨道引用的曲线不存在。
    CurveRefUnknown,
    /// 轨道 id 重复。
    TrackDuplicateId,
    /// 轨道权重越界。
    TrackWeightInvalid,
    /// 绑定路径非法（F2402 单源解析器拒绝）。
    TrackBindInvalid,
    /// 轨道类名不在六类封闭集内。
    TrackClassUnknown,
    /// 插值器名不在三值域内。
    InterpUnknown,
    /// clip 引用的轨道不存在。
    ClipTrackRefUnknown,
    /// clip 时长非法（非正，或短于所引曲线的末帧时间）。
    ClipDurationInvalid,
    /// 数值非有限（NaN / ±inf）——导出前拒绝。
    NonFiniteValue,
    /// 数值越出定义域（如 `comps` 不在 1..=4）。
    ValueOutOfDomain,
    /// 签名缺失（告警，不阻断）。
    SignatureAbsent,
    /// 签名不完整（告警）。
    SignatureWeak,
    /// 清洗剥离了隐私字段（留痕）。
    SignatureScrubbed,
    /// 两次导出文本不一致（保真红线，P1）。
    RoundTripDrift,
    /// 漂移定位到的丢失段。
    RoundTripLossySegment,
    /// 配额超限。
    QuotaExceeded,
}

impl AssetDiag {
    /// 码的稳定字符串（跨会话回归比对用）。
    pub const fn as_str(self) -> &'static str {
        match self {
            AssetDiag::SyntaxMalformed => "ASSET_SYNTAX_MALFORMED",
            AssetDiag::DepthExceeded => "ASSET_DEPTH_EXCEEDED",
            AssetDiag::SchemaMismatch => "ASSET_SCHEMA_MISMATCH",
            AssetDiag::MagicMismatch => "ASSET_MAGIC_MISMATCH",
            AssetDiag::VersionFromFuture => "ASSET_VERSION_FROM_FUTURE",
            AssetDiag::NoMigrationPath => "ASSET_NO_MIGRATION_PATH",
            AssetDiag::MigrationFailed => "ASSET_MIGRATION_FAILED",
            AssetDiag::SegmentUnknown => "ASSET_SEGMENT_UNKNOWN",
            AssetDiag::SegmentMissing => "ASSET_SEGMENT_MISSING",
            AssetDiag::SegmentHashMismatch => "ASSET_SEGMENT_HASH_MISMATCH",
            AssetDiag::SegmentShapeInvalid => "ASSET_SEGMENT_SHAPE_INVALID",
            AssetDiag::CurveLengthMismatch => "ASSET_CURVE_LENGTH_MISMATCH",
            AssetDiag::CurveTimeNotMonotonic => "ASSET_CURVE_TIME_NOT_MONOTONIC",
            AssetDiag::CurveRefUnknown => "ASSET_CURVE_REF_UNKNOWN",
            AssetDiag::TrackDuplicateId => "ASSET_TRACK_DUPLICATE_ID",
            AssetDiag::TrackWeightInvalid => "ASSET_TRACK_WEIGHT_INVALID",
            AssetDiag::TrackBindInvalid => "ASSET_TRACK_BIND_INVALID",
            AssetDiag::TrackClassUnknown => "ASSET_TRACK_CLASS_UNKNOWN",
            AssetDiag::InterpUnknown => "ASSET_INTERP_UNKNOWN",
            AssetDiag::ClipTrackRefUnknown => "ASSET_CLIP_TRACK_REF_UNKNOWN",
            AssetDiag::ClipDurationInvalid => "ASSET_CLIP_DURATION_INVALID",
            AssetDiag::NonFiniteValue => "ASSET_NON_FINITE_VALUE",
            AssetDiag::ValueOutOfDomain => "ASSET_VALUE_OUT_OF_DOMAIN",
            AssetDiag::SignatureAbsent => "ASSET_SIGNATURE_ABSENT",
            AssetDiag::SignatureWeak => "ASSET_SIGNATURE_WEAK",
            AssetDiag::SignatureScrubbed => "ASSET_SIGNATURE_SCRUBBED",
            AssetDiag::RoundTripDrift => "ASSET_ROUND_TRIP_DRIFT",
            AssetDiag::RoundTripLossySegment => "ASSET_ROUND_TRIP_LOSSY_SEGMENT",
            AssetDiag::QuotaExceeded => "ASSET_QUOTA_EXCEEDED",
        }
    }

    /// 默认处置方向：`true` = 阻断（错误），`false` = 放行（告警/留痕）。
    ///
    /// **分诊表由码本身决定**，调用方不得自行决定"这个要不要拦"——那正是零静默
    /// 纪律要消灭的私自放行。
    pub const fn is_blocking(self) -> bool {
        !matches!(
            self,
            AssetDiag::SegmentUnknown
                | AssetDiag::VersionFromFuture
                | AssetDiag::SignatureAbsent
                | AssetDiag::SignatureWeak
                | AssetDiag::SignatureScrubbed
        )
    }
}

/// 诊断严重度。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Severity {
    /// 阻断（错误）。
    Error,
    /// 放行 + 提示（告警）。
    Warn,
    /// 放行 + 留痕（信息）。
    Info,
}

/// 一条诊断。
///
/// `segment` 是**定位段**：三要素拒绝必须能指名是哪一段坏了，只说"校验失败"
/// 等于让读日志的人自己去二分定位。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AssetNote {
    /// 诊断码。
    pub code: AssetDiag,
    /// 严重度。
    pub severity: Severity,
    /// 定位段（`None` = 容器级，如魔数/版本）。
    pub segment: Option<&'static str>,
    /// 人话描述。
    pub message: String,
    /// 处置建议。
    pub hint: String,
}

/// 审计留痕条目（F1948 三件套之「审计」）。
///
/// **留痕与诊断是两回事**：诊断说"这次不行"，留痕说"我们动了作者的什么"。
/// 清洗剥离了绝对路径是**正确行为**，但作者有权知道自己的机器路径没进文件。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AuditEntry {
    /// 发生阶段（`export` / `import` / `migrate` / `scrub`）。
    pub stage: &'static str,
    /// 相关码。
    pub code: AssetDiag,
    /// 被触碰的字段名。
    pub field: &'static str,
}

/// 诊断袋 + 审计留痕（一次操作的完整痕迹）。
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct AssetReport {
    /// 全部诊断（含告警与留痕）。
    pub notes: Vec<AssetNote>,
    /// 审计留痕。
    pub audit: Vec<AuditEntry>,
}

impl AssetReport {
    /// 空报告。
    pub fn new() -> Self {
        AssetReport {
            notes: Vec::new(),
            audit: Vec::new(),
        }
    }

    /// 记一条阻断诊断（三要素：码 / 段 / 人话 + 建议）。
    pub fn error(
        &mut self,
        code: AssetDiag,
        segment: Option<&'static str>,
        message: &str,
        hint: &str,
    ) {
        self.notes.push(AssetNote {
            code,
            severity: Severity::Error,
            segment,
            message: message.to_string(),
            hint: hint.to_string(),
        });
    }

    /// 记一条告警（放行 + 提示）。
    pub fn warn(&mut self, code: AssetDiag, segment: Option<&'static str>, message: &str, hint: &str) {
        self.notes.push(AssetNote {
            code,
            severity: Severity::Warn,
            segment,
            message: message.to_string(),
            hint: hint.to_string(),
        });
    }

    /// 记一条留痕（放行 + 审计）。
    pub fn info(&mut self, code: AssetDiag, stage: &'static str, field: &'static str, message: &str) {
        self.notes.push(AssetNote {
            code,
            severity: Severity::Info,
            segment: None,
            message: message.to_string(),
            hint: String::new(),
        });
        self.audit.push(AuditEntry {
            stage,
            code,
            field,
        });
    }

    /// 是否有阻断项。
    pub fn has_errors(&self) -> bool {
        self.notes.iter().any(|n| n.severity == Severity::Error)
    }

    /// 阻断项条数。
    pub fn error_count(&self) -> usize {
        self.notes
            .iter()
            .filter(|n| n.severity == Severity::Error)
            .count()
    }

    /// 全部阻断项。
    pub fn errors(&self) -> Vec<&AssetNote> {
        self.notes
            .iter()
            .filter(|n| n.severity == Severity::Error)
            .collect()
    }

    /// 全部告警项。
    pub fn warnings(&self) -> Vec<&AssetNote> {
        self.notes
            .iter()
            .filter(|n| n.severity == Severity::Warn)
            .collect()
    }

    /// 首个阻断码。
    pub fn first_code(&self) -> Option<AssetDiag> {
        self.errors().first().map(|n| n.code)
    }

    /// 并入另一份报告（导入时把子校验的痕迹一并带出）。
    pub fn absorb(&mut self, other: &AssetReport) {
        self.notes.extend(other.notes.iter().cloned());
        self.audit.extend(other.audit.iter().cloned());
    }

    /// 诊断 + 留痕总条数。
    pub fn total(&self) -> usize {
        self.notes.len() + self.audit.len()
    }

    /// 按 [`AssetDiag::is_blocking`] 分诊记一条诊断（**严重度由码决定，不经调用方**）。
    ///
    /// **这条路径是分诊表的唯一消费点**：若调用方各自挑 `error`/`warn`，
    /// [`AssetDiag::is_blocking`] 就退化成一份没人看的文档——改它不影响任何
    /// 行为，于是「处置方向由码决定」这条纪律实际无人执行。故凡涉及
    /// 「这一类到底拦不拦」的判定，一律走本函数。
    pub fn push(&mut self, code: AssetDiag, segment: Option<&'static str>, message: &str, hint: &str) {
        if code.is_blocking() {
            self.error(code, segment, message, hint);
        } else {
            self.warn(code, segment, message, hint);
        }
    }
}

/// 资产操作结果面。
#[derive(Clone, Debug, PartialEq)]
pub enum AssetOutcome<T> {
    /// 成功（可能带告警与留痕）。
    Ok {
        /// 结果值。
        value: T,
        /// 本次操作的完整痕迹。
        report: AssetReport,
    },
    /// 失败（阻断面非空）。
    Err {
        /// 完整痕迹（含全部阻断项，不止首个——归因要看全集）。
        report: AssetReport,
    },
}

impl<T> AssetOutcome<T> {
    /// 取值（失败面 `None`，不 panic）。
    pub fn ok(&self) -> Option<&T> {
        match self {
            AssetOutcome::Ok { value, .. } => Some(value),
            AssetOutcome::Err { .. } => None,
        }
    }

    /// 是否成功。
    pub fn is_ok(&self) -> bool {
        matches!(self, AssetOutcome::Ok { .. })
    }

    /// 取痕迹（两面都可取——失败时也要能读诊断）。
    pub fn report(&self) -> &AssetReport {
        match self {
            AssetOutcome::Ok { report, .. } => report,
            AssetOutcome::Err { report } => report,
        }
    }

    /// 把报告封成结果面：有阻断项即 `Err`。
    pub fn seal(value: T, report: &AssetReport) -> AssetOutcome<T> {
        if report.has_errors() {
            AssetOutcome::Err {
                report: report.clone(),
            }
        } else {
            AssetOutcome::Ok {
                value,
                report: report.clone(),
            }
        }
    }

    /// 失败时构造一个占位值（调用方在 `Err` 面必须读 `report()`，取值即 misuse）。
    pub fn err_from(report: &AssetReport) -> AssetOutcome<T> {
        AssetOutcome::Err {
            report: report.clone(),
        }
    }
}

// ===========================================================================
// §1 JSON 值面（无依赖最小实现；保序 + 确定性序列化）
// ===========================================================================

/// JSON 值。
///
/// **为什么不用任何 serde 类设施**：本仓内核 `Cargo.toml` 的 `[dependencies]` 为空
/// （`[[bin]]` 带 `required-features = ["kernel-image"]`），引入任何外部 crate
/// 都要动构建链——而构建链是引导红线覆盖的范围。自己写一个 300 行的最小
/// JSON 值面反而更安全：**确定性序列化是往返保真的前提**，外部库的行为我们
/// 控制不了。
#[derive(Clone, Debug, PartialEq)]
pub enum Json {
    /// `null`。
    Null,
    /// `true` / `false`。
    Bool(bool),
    /// 整数（tick 数、时长、版本号——整数不经浮点，避开精度损失）。
    Int(i64),
    /// 浮点数值（曲线值）。**导出前必须确认有限**。
    Num(f32),
    /// 字符串。
    Str(String),
    /// 数组。
    Arr(Vec<Json>),
    /// 对象（**保序**，不是排序表）。
    Obj(Vec<(String, Json)>),
}

impl Json {
    /// 取对象字段（`None` = 缺该键或不是对象）。
    pub fn get(&self, key: &str) -> Option<&Json> {
        match self {
            Json::Obj(fields) => fields.iter().find(|(k, _)| k == key).map(|(_, v)| v),
            _ => None,
        }
    }

    /// 取对象字段字符串值（缺键或非字符串返回 `None`）。
    pub fn get_str(&self, key: &str) -> Option<&str> {
        match self.get(key) {
            Some(Json::Str(s)) => Some(s.as_str()),
            _ => None,
        }
    }

    /// 取整数（缺键、非整数返回 `None`）。
    pub fn get_int(&self, key: &str) -> Option<i64> {
        match self.get(key) {
            Some(Json::Int(v)) => Some(*v),
            _ => None,
        }
    }

    /// 取布尔（缺键、非布尔返回 `None`）。
    pub fn get_bool(&self, key: &str) -> Option<bool> {
        match self.get(key) {
            Some(Json::Bool(v)) => Some(*v),
            _ => None,
        }
    }

    /// 取数组（缺键、非数组返回 `None`）。
    pub fn get_arr(&self, key: &str) -> Option<&Vec<Json>> {
        match self.get(key) {
            Some(Json::Arr(v)) => Some(v),
            _ => None,
        }
    }

    /// 取数值数组，元素允许 `Int` 或 `Num`（时间戳与曲线值共路）。
    ///
    /// 允许 `Int` 元素读成 `f32` 是必须的：时间戳 `[0,10,20]` 导出成整数数组，
    /// 若只认 `Num` 则导出→导入必失败。
    pub fn get_num_arr(&self, key: &str) -> Option<Vec<f32>> {
        let arr = self.get_arr(key)?;
        let mut out = Vec::with_capacity(arr.len());
        for item in arr.iter() {
            match item {
                Json::Num(v) => out.push(*v),
                Json::Int(v) => out.push(*v as f32),
                _ => return None,
            }
        }
        Some(out)
    }

    /// 取整数数组。
    pub fn get_int_arr(&self, key: &str) -> Option<Vec<i64>> {
        let arr = self.get_arr(key)?;
        let mut out = Vec::with_capacity(arr.len());
        for item in arr.iter() {
            match item {
                Json::Int(v) => out.push(*v),
                _ => return None,
            }
        }
        Some(out)
    }

    /// 取字符串数组。
    pub fn get_str_arr(&self, key: &str) -> Option<Vec<String>> {
        let arr = self.get_arr(key)?;
        let mut out = Vec::with_capacity(arr.len());
        for item in arr.iter() {
            match item {
                Json::Str(s) => out.push(s.clone()),
                _ => return None,
            }
        }
        Some(out)
    }

    /// 是否为对象。
    pub fn is_obj(&self) -> bool {
        matches!(self, Json::Obj(_))
    }

    /// 字段数（对象）。
    pub fn field_len(&self) -> usize {
        match self {
            Json::Obj(f) => f.len(),
            _ => 0,
        }
    }
}

/// 嵌套深度上限（防栈溢出）。
///
/// **语义是「实际层数」**：`depth` 在进入容器时自增，故判据用 `>=`——
/// 写成 `>` 会让实际可嵌套层数比这个数字多一层，文档与行为对不上，
/// 而这类偏差会让门禁在错误的位置取样（我在这上面踩过一次：
/// 自检按 `MAX+1` 取样，实际放行了 `MAX+1` 层，门禁变恒真）。
pub const MAX_JSON_DEPTH: usize = 24;

/// JSON 解析器（递归下降；全路径零 panic）。
struct Parser<'a> {
    bytes: &'a [u8],
    pos: usize,
    depth: usize,
}

impl<'a> Parser<'a> {
    fn new(bytes: &'a [u8]) -> Self {
        Parser {
            bytes,
            pos: 0,
            depth: 0,
        }
    }

    /// 读一个字节（越界返回 `None`，不 panic）。
    fn peek(&self) -> Option<u8> {
        self.bytes.get(self.pos).copied()
    }

    /// 跳空白。
    fn skip_ws(&mut self) {
        while let Some(c) = self.peek() {
            if c == b' ' || c == b'\t' || c == b'\n' || c == b'\r' {
                self.pos += 1;
            } else {
                break;
            }
        }
    }

    /// 吃一个期望字节（不匹配即语法错）。
    fn eat(&mut self, want: u8) -> Result<(), String> {
        match self.peek() {
            Some(c) if c == want => {
                self.pos += 1;
                Ok(())
            }
            Some(c) => Err(format!("期望字节 {:?}，实得 {:?}", want as char, c as char)),
            None => Err(String::from("输入意外结束")),
        }
    }

    /// 解析一个值。
    fn value(&mut self, report: &mut AssetReport) -> Result<Json, String> {
        if self.depth >= MAX_JSON_DEPTH {
            report.error(
                AssetDiag::DepthExceeded,
                None,
                &format!("嵌套深度超过 {}", MAX_JSON_DEPTH),
                "嵌套过深多半是损坏或恶意文件；提高上限前先确认不是解析器失控",
            );
            return Err(String::from("深度超限"));
        }
        self.skip_ws();
        let c = match self.peek() {
            Some(c) => c,
            None => return Err(String::from("输入意外结束")),
        };
        match c {
            b'{' => self.object(report),
            b'[' => self.array(report),
            b'"' => Ok(Json::Str(self.string()?)),
            b't' => self.literal("true", Json::Bool(true)),
            b'f' => self.literal("false", Json::Bool(false)),
            b'n' => self.literal("null", Json::Null),
            b'-' | b'0'..=b'9' => self.number(),
            other => Err(format!("非法起始字节 {:?}", other as char)),
        }
    }

    /// 匹配一个字面量。
    fn literal(&mut self, word: &str, v: Json) -> Result<Json, String> {
        let wb = word.as_bytes();
        if self.bytes.len() >= self.pos + wb.len() && &self.bytes[self.pos..self.pos + wb.len()] == wb
        {
            self.pos += wb.len();
            Ok(v)
        } else {
            Err(format!("期望字面量 {}", word))
        }
    }

    /// 解析对象。
    fn object(&mut self, report: &mut AssetReport) -> Result<Json, String> {
        self.depth += 1;
        self.eat(b'{')?;
        let mut fields: Vec<(String, Json)> = Vec::new();
        self.skip_ws();
        match self.peek() {
            Some(b'}') => {
                self.pos += 1;
                self.depth -= 1;
                return Ok(Json::Obj(fields));
            }
            None => {
                self.depth -= 1;
                return Err(String::from("对象未闭合"));
            }
            _ => {}
        }
        loop {
            self.skip_ws();
            let key = self.string()?;
            if fields.iter().any(|(k, _)| *k == key) {
                // 重复键：保留最后一个（与主流解析器一致），但**必须报出来**——
                // 重复键是"两段数据拼错位置"的典型痕迹，静默取后者会掩盖它。
                report.error(
                    AssetDiag::SegmentShapeInvalid,
                    None,
                    &format!("对象含重复键 \"{}\"", key),
                    "重复键通常意味着拼接错位或恶意构造；请检查文件来源",
                );
                fields.retain(|(k, _)| *k != key);
            }
            self.skip_ws();
            self.eat(b':')?;
            let v = self.value(report)?;
            fields.push((key, v));
            self.skip_ws();
            match self.peek() {
                Some(b',') => {
                    self.pos += 1;
                }
                Some(b'}') => {
                    self.pos += 1;
                    break;
                }
                _ => return Err(String::from("对象缺 , 或 }")),
            }
        }
        self.depth -= 1;
        Ok(Json::Obj(fields))
    }

    /// 解析数组。
    fn array(&mut self, report: &mut AssetReport) -> Result<Json, String> {
        self.depth += 1;
        self.eat(b'[')?;
        let mut items: Vec<Json> = Vec::new();
        self.skip_ws();
        match self.peek() {
            Some(b']') => {
                self.pos += 1;
                self.depth -= 1;
                return Ok(Json::Arr(items));
            }
            None => {
                self.depth -= 1;
                return Err(String::from("数组未闭合"));
            }
            _ => {}
        }
        loop {
            let v = self.value(report)?;
            items.push(v);
            self.skip_ws();
            match self.peek() {
                Some(b',') => {
                    self.pos += 1;
                }
                Some(b']') => {
                    self.pos += 1;
                    break;
                }
                _ => return Err(String::from("数组缺 , 或 ]")),
            }
        }
        self.depth -= 1;
        Ok(Json::Arr(items))
    }

    /// 解析字符串（含转义）。
    fn string(&mut self) -> Result<String, String> {
        self.eat(b'"')?;
        let mut out = String::new();
        loop {
            let c = match self.peek() {
                Some(c) => c,
                None => return Err(String::from("字符串未闭合")),
            };
            self.pos += 1;
            match c {
                b'"' => return Ok(out),
                b'\\' => {
                    let e = match self.peek() {
                        Some(e) => e,
                        None => return Err(String::from("转义未完成")),
                    };
                    self.pos += 1;
                    match e {
                        b'"' => out.push('"'),
                        b'\\' => out.push('\\'),
                        b'/' => out.push('/'),
                        b'b' => out.push('\u{0008}'),
                        b'f' => out.push('\u{000C}'),
                        b'n' => out.push('\n'),
                        b'r' => out.push('\r'),
                        b't' => out.push('\t'),
                        b'u' => {
                            let hi = self.hex4()?;
                            let ch = if (0xD800..0xDC00).contains(&hi) {
                                // 高代理：必须紧跟低代理，否则是非法 UTF-16
                                if self.peek() != Some(b'\\') {
                                    return Err(String::from("孤立的高代理项"));
                                }
                                self.pos += 1;
                                if self.peek() != Some(b'u') {
                                    return Err(String::from("孤立的高代理项"));
                                }
                                self.pos += 1;
                                let lo = self.hex4()?;
                                if !(0xDC00..0xE000).contains(&lo) {
                                    return Err(String::from("代理对不匹配"));
                                }
                                let cp = 0x10000 + ((hi - 0xD800) << 10) + (lo - 0xDC00);
                                char::from_u32(cp).ok_or_else(|| String::from("代理对解出非法码点"))?
                            } else {
                                char::from_u32(hi).ok_or_else(|| String::from("非法码点"))?
                            };
                            out.push(ch);
                        }
                        other => return Err(format!("非法转义 {:?}", other as char)),
                    }
                }
                _ => {
                    // 非 ASCII 字节：按 UTF-8 序列原样收集。
                    let start = self.pos - 1;
                    let len = utf8_len(c);
                    if len == 0 || start + len > self.bytes.len() {
                        return Err(String::from("非法 UTF-8 起始字节"));
                    }
                    self.pos = start + len;
                    match core::str::from_utf8(&self.bytes[start..self.pos]) {
                        Ok(s) => out.push_str(s),
                        Err(_) => return Err(String::from("非法 UTF-8 序列")),
                    }
                }
            }
        }
    }

    /// 读 4 位十六进制。
    fn hex4(&mut self) -> Result<u32, String> {
        let mut v: u32 = 0;
        for _ in 0..4 {
            let c = self.peek().ok_or("\\u 转义被截断")?;
            self.pos += 1;
            let d = match c {
                b'0'..=b'9' => (c - b'0') as u32,
                b'a'..=b'f' => (c - b'a') as u32 + 10,
                b'A'..=b'F' => (c - b'A') as u32 + 10,
                _ => return Err(String::from("\\u 转义含非十六进制字符")),
            };
            v = (v << 4) | d;
        }
        Ok(v)
    }

    /// 解析数字。
    ///
    /// **整数优先**：不含 `.`/`e`/`E` 时先试 `i64`，失败（溢出）再回落 `f32`。
    /// 反过来会让 `1e20f32` 被读成 `Int` 而溢出——而 tick 数与曲线值里
    /// 大数并不罕见。
    fn number(&mut self) -> Result<Json, String> {
        let start = self.pos;
        let mut has_frac = false;
        let mut has_exp = false;
        while let Some(c) = self.peek() {
            match c {
                b'-' | b'+' | b'0'..=b'9' => self.pos += 1,
                b'.' if !has_frac && !has_exp => {
                    has_frac = true;
                    self.pos += 1;
                }
                b'e' | b'E' if !has_exp => {
                    has_exp = true;
                    self.pos += 1;
                }
                _ => break,
            }
        }
        let raw = core::str::from_utf8(&self.bytes[start..self.pos]).map_err(|_| "非法数字字节")?;
        if raw.is_empty() {
            return Err(String::from("空数字"));
        }
        if !has_frac && !has_exp {
            if let Ok(v) = raw.parse::<i64>() {
                // **负零必须走浮点臂**：`-0` 解析成 `i64` 得 `0`，符号位就此丢失。
                // 曲线值里的 `-0.0` 会被读成 `0.0`，而某些插值/除法路径会因此
                // 翻转结果方向——这类损坏在数值上完全等价，只能靠符号位抓住。
                let is_neg_zero = v == 0 && raw.starts_with('-');
                if !is_neg_zero {
                    return Ok(Json::Int(v));
                }
                return Ok(Json::Num(-0.0f32));
            }
        }
        match raw.parse::<f32>() {
            Ok(v) => {
                if v.is_finite() {
                    Ok(Json::Num(v))
                } else {
                    // JSON 数字字面量解析出非有限值（溢出为 inf）：显性拒绝，
                    // 不静默塞一个 inf 进资产。
                    Err(String::from("数字字面量解析结果非有限"))
                }
            }
            Err(_) => Err(String::from("数字字面量非法")),
        }
    }
}

/// UTF-8 起始字节的续字节数（`0` = 非法起始）。
const fn utf8_len(first: u8) -> usize {
    if first < 0x80 {
        1
    } else if first >= 0xC2 && first < 0xE0 {
        2
    } else if first >= 0xE0 && first < 0xF0 {
        3
    } else if first >= 0xF0 && first < 0xF5 {
        4
    } else {
        0
    }
}

/// 解析 JSON 文本（全路径零 panic；语法错与深度错都进报告）。
pub fn parse_json(text: &str, report: &mut AssetReport) -> Result<Json, String> {
    let mut p = Parser::new(text.as_bytes());
    let v = p.value(report)?;
    p.skip_ws();
    if p.pos != p.bytes.len() {
        return Err(String::from("尾部有残余字节"));
    }
    Ok(v)
}

/// 紧凑序列化（确定性：无空格、保序）。
pub fn write_json(v: &Json, out: &mut String) {
    match v {
        Json::Null => out.push_str("null"),
        Json::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
        Json::Int(i) => {
            out.push_str(&format!("{}", i));
        }
        Json::Num(f) => {
            // 调用方须已确认有限；此处再兜一层：非有限写 `null` 而不是 `inf`，
            // 让读回方的失败点落在"值缺失"而非"语法错"，可诊断性更好。
            if f.is_finite() {
                out.push_str(&format!("{}", f));
            } else {
                out.push_str("null");
            }
        }
        Json::Str(s) => write_json_str(s, out),
        Json::Arr(items) => {
            out.push('[');
            for (i, item) in items.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                write_json(item, out);
            }
            out.push(']');
        }
        Json::Obj(fields) => {
            out.push('{');
            for (i, (k, val)) in fields.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                write_json_str(k, out);
                out.push(':');
                write_json(val, out);
            }
            out.push('}');
        }
    }
}

/// 转义输出字符串。
fn write_json_str(s: &str, out: &mut String) {
    out.push('"');
    for ch in s.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => {
                out.push_str(&format!("\\u{:04x}", c as u32));
            }
            c => out.push(c),
        }
    }
    out.push('"');
}

/// 值面 → 紧凑文本。
pub fn write_to_string(v: &Json) -> String {
    let mut s = String::new();
    write_json(v, &mut s);
    s
}

// ===========================================================================
// §2 哈希与段信封
// ===========================================================================

/// FNV-1a 64 位（自持实现，理由同 §1）。
///
/// **选 FNV-1a 而不是任何加密哈希**：这里要的是"改一个字节就能看出来"的
/// 完整性校验，不是抗攻击的真实性证明。资产文件来自本地磁盘与他人分享，
/// 威胁模型是**意外损坏**而非恶意篡改；引入 SHA 家族要么加依赖，要么手写
/// 一堆常数表，都是净亏损。
pub fn fnv1a64(bytes: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in bytes.iter() {
        h ^= *b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    h
}

/// `u64` → 定长 16 位十六进制。
pub fn hex16(v: u64) -> String {
    const D: &[u8; 16] = b"0123456789abcdef";
    let mut s = String::with_capacity(16);
    let mut shift = 60;
    loop {
        let nib = ((v >> shift) & 0xF) as usize;
        let ch = match D.get(nib) {
            Some(b) => *b,
            None => b'?',
        };
        s.push(ch as char);
        if shift == 0 {
            break;
        }
        shift -= 4;
    }
    s
}

/// 定长十六进制 → `u64`（长度不符或含非十六进制字符返回 `None`）。
pub fn parse_hex16(s: &str) -> Option<u64> {
    if s.len() != 16 {
        return None;
    }
    let mut v: u64 = 0;
    for ch in s.chars() {
        let d = match ch {
            '0'..='9' => ch as u64 - '0' as u64,
            'a'..='f' => ch as u64 - 'a' as u64 + 10,
            'A'..='F' => ch as u64 - 'A' as u64 + 10,
            _ => return None,
        };
        v = (v << 4) | d;
    }
    Some(v)
}

/// 容器魔数（8 字节；`"m.anim."` 段的 ASCII 形）。
pub const CONTAINER_MAGIC: [u8; 8] = *b"VARIXANM";

/// 容器 schema 标签（生态单点的段名，F1942 段枚举）。
pub const SEGMENT_SCHEMA: &str = "m.anim.";

/// 当前容器版本。
///
/// **版本历史**：v1 = 四类段但曲线无 `comps`、元数据无 `license`；
/// v2 = 加 `blended` 与 `loop_default`；v3 = 加 `comps` 与 `license`（当前）。
pub const CONTAINER_VERSION: i64 = 3;

/// 段内哈希字段名（**故意取得很别扭**：`_` 前缀且带 `h`，与作者字段不撞名）。
pub const SEG_HASH_FIELD: &str = "__h";

/// 根内容哈希字段名。
pub const CONTENT_HASH_FIELD: &str = "content_hash";

/// 把对象里的 `__h` 换成给定值（其余字段保序，末尾追加）。
fn seg_with_hash_field(seg: &Json, value: &str) -> Json {
    match seg {
        Json::Obj(fields) => {
            let mut out: Vec<(String, Json)> = Vec::with_capacity(fields.len() + 1);
            for (k, v) in fields.iter() {
                if k != SEG_HASH_FIELD {
                    out.push((k.clone(), v.clone()));
                }
            }
            out.push((SEG_HASH_FIELD.to_string(), Json::Str(value.to_string())));
            Json::Obj(out)
        }
        // 非对象段原样返回（形状校验会另行报错）。
        other => other.clone(),
    }
}

/// 段的规范摘要（哈希输入 = `__h` 置空后的紧凑序列化）。
///
/// **排除自身是必需的**：否则"段内容哈希段自己"是循环定义，只能靠试错解方程。
fn segment_digest(seg: &Json) -> u64 {
    let canon = write_to_string(&seg_with_hash_field(seg, ""));
    fnv1a64(canon.as_bytes())
}

/// 给段封上哈希字段。
pub fn seal_segment(seg: &Json) -> Json {
    let h = segment_digest(seg);
    seg_with_hash_field(seg, &hex16(h))
}

/// 就地重封容器的四类段（编辑器改完段内容后调用）。
///
/// **必须重封**：段哈希是「导出即封」的，绕过 [`seal_segment`] 直接改段内容
/// 会让文件在自己实现眼里都是坏的。这是显式函数而非自动钩子的理由——哈希
/// 重算必须是作者看得见的一步。
pub fn reseal_segments(root: &mut Json) {
    let segments = match root.get("segments") {
        Some(s) if s.is_obj() => s.clone(),
        _ => return,
    };
    let mut out: Vec<(String, Json)> = Vec::with_capacity(segments.field_len());
    for kind in SegmentKind::ALL.into_iter() {
        if let Some(seg) = segments.get(kind.key()) {
            out.push((kind.key().to_string(), seal_segment(seg)));
        }
    }
    // 保留未注册段（跳过不删——删掉就等于替作者做了前向兼容的决定）。
    if let Json::Obj(fs) = &segments {
        for (k, v) in fs.iter() {
            if SegmentKind::from_key(k).is_none() {
                out.push((k.clone(), v.clone()));
            }
        }
    }
    if let Json::Obj(fields) = root {
        for (k, v) in fields.iter_mut() {
            if k == "segments" {
                *v = Json::Obj(out.clone());
            }
        }
    }
}

/// 读段内声明的哈希。
fn declared_segment_hash(seg: &Json) -> Option<u64> {
    parse_hex16(seg.get_str(SEG_HASH_FIELD)?)
}

/// 校验段哈希。
fn verify_segment(seg: &Json, name: &'static str, report: &mut AssetReport) -> bool {
    let declared = match declared_segment_hash(seg) {
        Some(v) => v,
        None => {
            report.error(
                AssetDiag::SegmentShapeInvalid,
                Some(name),
                &format!("段 {} 缺少合法的 {} 字段", name, SEG_HASH_FIELD),
                "该段不是合法容器产出；请确认文件未被手工编辑",
            );
            return false;
        }
    };
    let actual = segment_digest(seg);
    if declared != actual {
        report.error(
            AssetDiag::SegmentHashMismatch,
            Some(name),
            &format!(
                "段 {} 内容哈希失配（声明 {} 实测 {}）",
                name,
                hex16(declared),
                hex16(actual)
            ),
            "该段内容已损坏或被改动；导出→导入往返会因此报 P1，先修该段再重试",
        );
        return false;
    }
    true
}

// ===========================================================================
// §3 生态段注册（F1942 生态单点）
// ===========================================================================

/// 生态段注册条目。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EcoSegment {
    /// 属域（`L` / `M`）。
    pub domain: &'static str,
    /// 段名（F1942 段枚举的键）。
    pub segment: &'static str,
    /// 引入版本说明。
    pub since: &'static str,
    /// 该段承载什么（人话）。
    pub note: &'static str,
}

/// 生态段枚举（`m.anim.` 是 L 域两段之后的第三域段——生态单点声明）。
///
/// **顺序即注册顺序**，且这是跨域共享的登记面：新增段只能追加在尾部，
/// 中间插队会让按序号对账的跨批工具全量错位。
pub const ECO_SEGMENTS: [EcoSegment; 3] = [
    EcoSegment {
        domain: "L",
        segment: "l.fluid.",
        since: "L 域物理预设段",
        note: "流体预设模板（时间无关的参数组合）",
    },
    EcoSegment {
        domain: "L",
        segment: "l.timeline.",
        since: "L 域时间轴编排段",
        note: "时间轴事件编排与轨道片段",
    },
    EcoSegment {
        domain: "M",
        segment: SEGMENT_SCHEMA,
        since: "VE-F2405 动画曲线资产段",
        note: "曲线/轨道/clip/元数据四段打包，开放可分享",
    },
];

/// 该段名是否已注册。
pub fn is_registered(segment: &str) -> bool {
    ECO_SEGMENTS.iter().any(|e| e.segment == segment)
}

/// 该段名的注册序号（未注册 `None`）。
///
/// 门禁设计的教训：查表函数若只用**表内**元素验证，等于恒真弱门禁。故这里
/// 额外要求未注册名返回 `None`——域级自检会拿表外的 `x.unknown.` 来验。
pub fn register_index(segment: &str) -> Option<usize> {
    ECO_SEGMENTS.iter().position(|e| e.segment == segment)
}

/// 注册序号 + 期望属域（供跨域对账；未注册返回 `None`）。
pub fn expect_domain(segment: &str) -> Option<&'static str> {
    register_index(segment).and_then(|i| ECO_SEGMENTS.get(i).map(|e| e.domain))
}

// ===========================================================================
// §4 资产数据面（锚点「容器格式」四段）
// ===========================================================================

/// 段类型（四类：元数据/曲线/轨道/clip）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SegmentKind {
    /// 元数据段。
    Meta,
    /// 曲线段。
    Curves,
    /// 轨道段。
    Tracks,
    /// clip 段。
    Clips,
}

impl SegmentKind {
    /// 四类全集（**顺序即容器内顺序**）。
    pub const ALL: [SegmentKind; 4] = [
        SegmentKind::Meta,
        SegmentKind::Curves,
        SegmentKind::Tracks,
        SegmentKind::Clips,
    ];

    /// 段键名（容器 JSON 的字段名）。
    pub const fn key(self) -> &'static str {
        match self {
            SegmentKind::Meta => "meta",
            SegmentKind::Curves => "curves",
            SegmentKind::Tracks => "tracks",
            SegmentKind::Clips => "clips",
        }
    }

    /// 按段键名查（未注册 `None`）。
    pub fn from_key(key: &str) -> Option<SegmentKind> {
        SegmentKind::ALL.into_iter().find(|k| k.key() == key)
    }

    /// 人话名。
    pub const fn display_name(self) -> &'static str {
        match self {
            SegmentKind::Meta => "元数据",
            SegmentKind::Curves => "曲线",
            SegmentKind::Tracks => "轨道",
            SegmentKind::Clips => "动画片段",
        }
    }
}

/// clip 循环模式。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LoopMode {
    /// 播一次。
    Once,
    /// 循环。
    Loop,
    /// 往复。
    PingPong,
}

impl LoopMode {
    /// 稳定字符串名。
    pub const fn as_str(self) -> &'static str {
        match self {
            LoopMode::Once => "once",
            LoopMode::Loop => "loop",
            LoopMode::PingPong => "pingpong",
        }
    }

    /// 中文显示名。
    pub const fn display_name(self) -> &'static str {
        match self {
            LoopMode::Once => "单次",
            LoopMode::Loop => "循环",
            LoopMode::PingPong => "往复",
        }
    }

    /// 按名查（未注册 `None`）。
    pub fn from_name(s: &str) -> Option<LoopMode> {
        match s {
            "once" => Some(LoopMode::Once),
            "loop" => Some(LoopMode::Loop),
            "pingpong" => Some(LoopMode::PingPong),
            _ => None,
        }
    }
}

/// 配额（防导入面资源耗尽；与 F2416 双硬顶联动）。
pub const MAX_CURVES: usize = 512;
pub const MAX_TRACKS: usize = 4096;
pub const MAX_CLIPS: usize = 256;
pub const MAX_KEYS_PER_CURVE: usize = 16384;

/// 曲线段条目（一条 = 一个属性的关键帧序列，值展平）。
#[derive(Clone, Debug, PartialEq)]
pub struct CurveSegment {
    /// 曲线 id（容器内唯一）。
    pub curve_id: String,
    /// 分量数（1 标量 / 3 三分量 / 4 四分量或四元数）。
    pub comps: usize,
    /// 时间戳（严格递增，`u32` tick）。
    pub times: Vec<u32>,
    /// 展平值（`len == times.len() × comps`）。
    pub values: Vec<f32>,
}

impl CurveSegment {
    /// 帧数。
    pub fn sample_count(&self) -> usize {
        self.times.len()
    }

    /// 取第 `frame` 帧的第 `comp` 个分量（越界返回 `None`）。
    pub fn component(&self, frame: usize, comp: usize) -> Option<f32> {
        if comp >= self.comps {
            return None;
        }
        self.values.get(frame * self.comps + comp).copied()
    }

    /// 构造并做形状校验（`report` 收到全部问题，不止首个）。
    pub fn build(
        curve_id: &str,
        comps: usize,
        times: Vec<u32>,
        values: Vec<f32>,
        report: &mut AssetReport,
    ) -> Self {
        let seg = CurveSegment {
            curve_id: curve_id.to_string(),
            comps,
            times,
            values,
        };
        seg.validate(report);
        seg
    }

    /// 形状与域校验。
    pub fn validate(&self, report: &mut AssetReport) {
        let seg_name = SegmentKind::Curves.key();
        if !(1..=4).contains(&self.comps) {
            report.error(
                AssetDiag::ValueOutOfDomain,
                Some(seg_name),
                &format!("曲线 {} 的分量数 {} 不在 1..=4", self.curve_id, self.comps),
                "分量数只能是 1（标量）/3（向量）/4（颜色或四元数）",
            );
        }
        if self.times.len() != self.values.len() / self.comps.max(1) {
            report.error(
                AssetDiag::CurveLengthMismatch,
                Some(seg_name),
                &format!(
                    "曲线 {} 值条数 {} 与 times.len()×comps = {} 不符",
                    self.curve_id,
                    self.values.len(),
                    self.times.len() * self.comps
                ),
                "展平值数组长度必须等于帧数乘分量数；缺一个分量就会整体错位",
            );
        }
        for (i, pair) in self.times.windows(2).enumerate() {
            let (a, b) = (pair.first().copied().unwrap_or(0), pair.get(1).copied().unwrap_or(0));
            if b <= a {
                report.error(
                    AssetDiag::CurveTimeNotMonotonic,
                    Some(seg_name),
                    &format!("曲线 {} 第 {} 帧时间未严格递增（{} → {}）", self.curve_id, i, a, b),
                    "二分求值依赖严格递增；同刻多帧须先合并或改用阶梯轨",
                );
                break;
            }
        }
        if self.times.len() > MAX_KEYS_PER_CURVE {
            report.error(
                AssetDiag::QuotaExceeded,
                Some(seg_name),
                &format!(
                    "曲线 {} 帧数 {} 超上限 {}",
                    self.curve_id,
                    self.times.len(),
                    MAX_KEYS_PER_CURVE
                ),
                "拆成多条曲线或裁剪关键帧；导入面不接受无界规模",
            );
        }
        for v in self.values.iter() {
            if !v.is_finite() {
                report.error(
                    AssetDiag::NonFiniteValue,
                    Some(seg_name),
                    &format!("曲线 {} 含非有限值", self.curve_id),
                    "NaN/inf 无法写成 JSON 数字；请先在编辑器里清理该关键帧",
                );
                break;
            }
        }
    }
}

/// 轨道段条目。
#[derive(Clone, Debug, PartialEq)]
pub struct TrackSegment {
    /// 轨道 id（容器内唯一）。
    pub track_id: String,
    /// 所属实体。
    pub owner: String,
    /// 轨道类（F2402 六类封闭集）。
    pub class: TrackClass,
    /// 插值器。
    pub interp: InterpKind,
    /// 所引曲线 id。
    pub curve_id: String,
    /// 绑定路径原文（F2402 单源解析器校验）。
    pub bind_raw: String,
    /// 权重（`WEIGHT_MIN..=WEIGHT_MAX`，单源来自 F2402）。
    pub weight: f32,
    /// 混合标记。
    pub blended: bool,
}

/// clip 段条目。
#[derive(Clone, Debug, PartialEq)]
pub struct ClipSegment {
    /// clip id（容器内唯一）。
    pub clip_id: String,
    /// 显示名。
    pub name: String,
    /// 时长（tick）。
    pub duration_ticks: u32,
    /// 所引轨道 id（保序）。
    pub track_ids: Vec<String>,
    /// 循环模式。
    pub loop_mode: LoopMode,
}

/// 元数据段（含隐私字段 `origin_path` / `machine_id`）。
///
/// **隐私字段进资产模型是有意的**：只有先在这里建模，清洗才有明确的剥离目标。
/// 若建模时不收这两个字段，作者写的机器路径就会一路走到导出文本里——
///
/// 那些字段是资产里唯一不该分享的东西，也是导出前唯一必须无条件抹掉的。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MetaSegment {
    /// 资产名。
    pub asset_name: String,
    /// 作者（署名；缺失只告警）。
    pub author: Option<String>,
    /// 许可（缺失只告警）。
    pub license: Option<String>,
    /// 来源 URL（缺失只告警）。
    pub source_url: Option<String>,
    /// 本机绝对路径（**导出必剥离**，隐私红线）。
    pub origin_path: Option<String>,
    /// 机器 id（**导出必剥离**，隐私红线）。
    pub machine_id: Option<String>,
    /// 默认循环模式。
    pub loop_default: LoopMode,
}

/// 完整动画资产（四段的集合）。
#[derive(Clone, Debug, PartialEq)]
pub struct AnimAsset {
    /// 元数据段。
    pub meta: MetaSegment,
    /// 曲线段。
    pub curves: Vec<CurveSegment>,
    /// 轨道段。
    pub tracks: Vec<TrackSegment>,
    /// clip 段。
    pub clips: Vec<ClipSegment>,
}

impl AnimAsset {
    /// 构造空资产（合法：四段皆空）。
    pub fn empty() -> Self {
        AnimAsset {
            meta: MetaSegment {
                asset_name: String::new(),
                author: None,
                license: None,
                source_url: None,
                origin_path: None,
                machine_id: None,
                loop_default: LoopMode::Once,
            },
            curves: Vec::new(),
            tracks: Vec::new(),
            clips: Vec::new(),
        }
    }

    /// 按 id 查曲线。
    pub fn curve(&self, id: &str) -> Option<&CurveSegment> {
        self.curves.iter().find(|c| c.curve_id == id)
    }

    /// 按 id 查轨道。
    pub fn track(&self, id: &str) -> Option<&TrackSegment> {
        self.tracks.iter().find(|t| t.track_id == id)
    }

    /// 按 id 查 clip。
    pub fn clip(&self, id: &str) -> Option<&ClipSegment> {
        self.clips.iter().find(|c| c.clip_id == id)
    }

    /// 全资产结构校验（四段各自校验 + 跨段引用校验）。
    ///
    /// **跨段引用必须在这里查**：曲线段自己不知道自己被谁引用，轨道段只看得到
    /// 曲线 id 字符串。「轨道指向不存在的曲线」这种损坏只有跨段才看得见，
    /// 而它正是导入后运行时才炸的那一类。
    pub fn validate(&self, report: &mut AssetReport) {
        let meta_seg = SegmentKind::Meta.key();
        let curve_seg = SegmentKind::Curves.key();
        let track_seg = SegmentKind::Tracks.key();
        let clip_seg = SegmentKind::Clips.key();

        if self.curves.len() > MAX_CURVES {
            report.error(
                AssetDiag::QuotaExceeded,
                Some(curve_seg),
                &format!("曲线条数 {} 超上限 {}", self.curves.len(), MAX_CURVES),
                "拆分资产或裁剪无用曲线",
            );
        }
        if self.tracks.len() > MAX_TRACKS {
            report.error(
                AssetDiag::QuotaExceeded,
                Some(track_seg),
                &format!("轨道条数 {} 超上限 {}", self.tracks.len(), MAX_TRACKS),
                "按实体拆分资产",
            );
        }
        if self.clips.len() > MAX_CLIPS {
            report.error(
                AssetDiag::QuotaExceeded,
                Some(clip_seg),
                &format!("clip 条数 {} 超上限 {}", self.clips.len(), MAX_CLIPS),
                "按用途拆分资产",
            );
        }

        // --- 曲线段 ---
        let mut seen_curves: Vec<&str> = Vec::new();
        for c in self.curves.iter() {
            c.validate(report);
            if seen_curves.contains(&c.curve_id.as_str()) {
                report.error(
                    AssetDiag::ValueOutOfDomain,
                    Some(curve_seg),
                    &format!("曲线 id 重复：{}", c.curve_id),
                    "曲线 id 是轨道引用键；重名会让引用指向不确定的那条",
                );
            } else {
                seen_curves.push(c.curve_id.as_str());
            }
        }

        // --- 轨道段 ---
        let mut seen_tracks: Vec<&str> = Vec::new();
        for t in self.tracks.iter() {
            if seen_tracks.contains(&t.track_id.as_str()) {
                report.error(
                    AssetDiag::TrackDuplicateId,
                    Some(track_seg),
                    &format!("轨道 id 重复：{}", t.track_id),
                    "轨道 id 唯一性由 F2402 容器保证；此处须拦在进容器之前",
                );
            } else {
                seen_tracks.push(t.track_id.as_str());
            }
            if self.curve(&t.curve_id).is_none() {
                report.error(
                    AssetDiag::CurveRefUnknown,
                    Some(track_seg),
                    &format!("轨道 {} 引用了不存在的曲线 {}", t.track_id, t.curve_id),
                    "补齐被引曲线或改指；悬空引用在求值期才会炸成空值",
                );
            }
            if !t.weight.is_finite() || t.weight < WEIGHT_MIN || t.weight > WEIGHT_MAX {
                report.error(
                    AssetDiag::TrackWeightInvalid,
                    Some(track_seg),
                    &format!(
                        "轨道 {} 权重 {} 不在 {}..={}",
                        t.track_id, t.weight, WEIGHT_MIN, WEIGHT_MAX
                    ),
                    "权重域单源来自 F2402；越界权重会让混合结果不可预测",
                );
            }
            // 绑定路径交给 F2402 单源解析器判，本域不自己发明语法。
            let mut bag = DiagBag::new();
            if parse_bind_path(&t.bind_raw, &mut bag).is_none() {
                report.error(
                    AssetDiag::TrackBindInvalid,
                    Some(track_seg),
                    &format!("轨道 {} 的绑定路径 \"{}\" 非法", t.track_id, t.bind_raw),
                    "路径语法单源在 F2402（如 /node/anim/pos）；本域不自造语法",
                );
            }
        }

        // --- clip 段 ---
        for c in self.clips.iter() {
            if c.duration_ticks == 0 {
                report.error(
                    AssetDiag::ClipDurationInvalid,
                    Some(clip_seg),
                    &format!("clip {} 时长为 0", c.clip_id),
                    "零时长 clip 无法求值；请给至少一帧",
                );
            }
            for tid in c.track_ids.iter() {
                let track = match self.track(tid) {
                    Some(t) => t,
                    None => {
                        report.error(
                            AssetDiag::ClipTrackRefUnknown,
                            Some(clip_seg),
                            &format!("clip {} 引用了不存在的轨道 {}", c.clip_id, tid),
                            "补齐被引轨道或从 clip 里摘掉该 id",
                        );
                        continue;
                    }
                };
                // 时长必须容得下所引曲子的末帧——否则求值会落在外推区，
                // 而外推是否允许是逐轨规格决定的（`Track::extrap_allowed`）。
                if let Some(curve) = self.curve(&track.curve_id) {
                    let last = curve.times.last().copied().unwrap_or(0);
                    if last > c.duration_ticks {
                        report.error(
                            AssetDiag::ClipDurationInvalid,
                            Some(clip_seg),
                            &format!(
                                "clip {} 时长 {} 短于轨道 {} 末帧时间 {}",
                                c.clip_id, c.duration_ticks, tid, last
                            ),
                            "延长 clip 或裁掉越界关键帧；否则末段只能靠外推",
                        );
                    }
                }
            }
        }

        // --- 元数据段签名分级 ---
        classify_signature_into(&self.meta, report, Some(meta_seg));
    }

    /// 无障碍替述：一句人话涵盖资产的全部可感知要素（不含视觉位置依赖）。
    ///
    /// 替述必须**完整到能替代看图**——只说"有 3 条曲线"是不够的，作者需要知道
    /// 动的是什么属性、往哪个方向动、循环不循环。
    pub fn alt_text(&self) -> String {
        let mut s = String::new();
        s.push_str(&format!("动画资产《{}》。", self.meta.asset_name));
        s.push_str(&format!(
            "含 {} 条曲线、{} 条轨道、{} 个动画片段。",
            self.curves.len(),
            self.tracks.len(),
            self.clips.len()
        ));
        for t in self.tracks.iter() {
            let curve = self.curve(&t.curve_id);
            let frames = curve.map(|c| c.sample_count()).unwrap_or(0);
            let target = match curve {
                Some(c) => format!(
                    "共 {} 帧、每帧 {} 个分量",
                    c.sample_count(),
                    c.comps
                ),
                None => String::from("所引曲线缺失"),
            };
            let _ = frames;
            s.push_str(&format!(
                "轨道「{}」驱动实体 {} 的{}属性，绑定 {}，{} 插值，权重 {:.2}，{}。",
                t.track_id,
                t.owner,
                t.class.display_name(),
                t.bind_raw,
                t.interp.as_str(),
                t.weight,
                target
            ));
        }
        for c in self.clips.iter() {
            s.push_str(&format!(
                "片段「{}」时长 {} 帧，{} 播放，含 {} 条轨道。",
                c.name,
                c.duration_ticks,
                c.loop_mode.display_name(),
                c.track_ids.len()
            ));
        }
        s
    }
}

// ===========================================================================
// §5 签名清洗三件套（F1948 家族复用）
// ===========================================================================

/// 签名分级。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SigTier {
    /// 三项齐备（作者 + 许可 + 来源）。
    Strong,
    /// 有但不齐。
    Weak,
    /// 一项都没有。
    Absent,
}

impl SigTier {
    /// 稳定字符串名。
    pub const fn as_str(self) -> &'static str {
        match self {
            SigTier::Strong => "strong",
            SigTier::Weak => "weak",
            SigTier::Absent => "absent",
        }
    }

    /// 人话说明。
    pub const fn display_name(self) -> &'static str {
        match self {
            SigTier::Strong => "署名完整",
            SigTier::Weak => "署名不完整",
            SigTier::Absent => "未署名",
        }
    }

    /// 是否阻断分发（F1948 平衡语义：**都不阻断**，只影响可见性）。
    pub const fn blocks_distribution(self) -> bool {
        false
    }
}

/// 三项中非空且非纯空白的项数。
fn filled_count(meta: &MetaSegment) -> usize {
    let mut n = 0;
    for v in [meta.author.as_deref(), meta.license.as_deref(), meta.source_url.as_deref()].iter() {
        if let Some(s) = v {
            if !s.trim().is_empty() {
                n += 1;
            }
        }
    }
    n
}

/// 签名分级。
pub fn classify_signature(meta: &MetaSegment) -> SigTier {
    match filled_count(meta) {
        0 => SigTier::Absent,
        3 => SigTier::Strong,
        _ => SigTier::Weak,
    }
}

/// 分级并落诊断（`Absent`/`Weak` 均为**告警不阻断**）。
fn classify_signature_into(
    meta: &MetaSegment,
    report: &mut AssetReport,
    seg: Option<&'static str>,
) {
    let tier = classify_signature(meta);
    match tier {
        SigTier::Absent => report.push(
            AssetDiag::SignatureAbsent,
            seg,
            &format!("资产《{}》未署名", meta.asset_name),
            "不阻断导入或分享（F1948 平衡语义）；但他人无法判断可否再分发，补齐作者与许可更稳妥",
        ),
        SigTier::Weak => report.push(
            AssetDiag::SignatureWeak,
            seg,
            &format!("资产《{}》署名不完整（{}/3 项）", meta.asset_name, filled_count(meta)),
            "缺的那项会影响再分发判断；请补齐后再对外分享",
        ),
        SigTier::Strong => {}
    }
}

/// 判断字符串是否 Windows/Unix 绝对路径。
///
/// **为什么要自己判而不是找现成函数**：内核无 `std::path` 的
/// `is_absolute`（那也是按平台来的，交叉编译时行为会漂），而本域要的是
/// 「这段文本会不会泄露目录结构」——那是**文本形态**问题，不是文件系统问题。
/// 判形态更稳：换平台也一致。
pub fn looks_absolute_path(s: &str) -> bool {
    let bytes = s.as_bytes();
    let first = match bytes.first() {
        Some(c) => *c,
        None => return false,
    };
    if first == b'/' && bytes.len() > 1 && bytes.get(1) != Some(&b'/') {
        return true;
    }
    if first == b'/' && bytes.len() > 2 {
        // `//server/share` 也是绝对（UNC）
        return true;
    }
    // 盘符 + 冒号：`C:\...` / `C:/...`，冒号后须是分隔符才认（否则 `a:b` 是别的意思）
    if bytes.len() >= 3 && first.is_ascii_alphabetic() && bytes.get(1) == Some(&b':') {
        if let Some(sep) = bytes.get(2) {
            return *sep == b'\\' || *sep == b'/';
        }
    }
    false
}

/// 导出前清洗：无条件剥离隐私字段。
///
/// **无条件**是本函数的全部纪律——不按签名分级、不按有没有许可。
/// 理由见头注决策 6：隐私风险来自携带本机信息，不来自署名完整度。
pub fn scrub_for_export(meta: &mut MetaSegment, report: &mut AssetReport) {
    let seg = SegmentKind::Meta.key();
    let path = meta.origin_path.clone();
    if let Some(p) = path {
        if looks_absolute_path(&p) {
            meta.origin_path = None;
            report.info(
                AssetDiag::SignatureScrubbed,
                "scrub",
                "origin_path",
                &format!("剥离本机绝对路径（{} 字节）", p.len()),
            );
        }
    }
    let mid = meta.machine_id.clone();
    if let Some(m) = mid {
        if !m.trim().is_empty() {
            meta.machine_id = None;
            report.info(
                AssetDiag::SignatureScrubbed,
                "scrub",
                "machine_id",
                "剥离机器标识",
            );
        }
    }
    let _ = seg;
}

/// 序列化用元数据（清洗后的副本，不改原资产）。
///
/// **不改原资产**是刻意的：清洗是"导出这一份文本"的属性，不是"修改作者的资产"
/// 的动作。若就地抹掉，作者下次导出未分享的私有备份时隐私字段就没了。
pub fn meta_for_export(meta: &MetaSegment, report: &mut AssetReport) -> MetaSegment {
    let mut m = meta.clone();
    scrub_for_export(&mut m, report);
    m
}

// ===========================================================================
// §6 版本化三件套（F1956 家族复用）
// ===========================================================================

/// 迁移表（显式逐跳可达；不可达即显性拒绝）。
///
/// **每跳都对应一次真实的格式变更**：
/// - v1 → v2：轨道补 `blended`（v1 用「同实体同类后挂者即混合」的隐式规则，
///   v2 改成显式位）；元数据补 `loop_default`（v1 隐含 `once`）。
/// - v2 → v3：曲线补 `comps`（v2 隐含 1，即只支持标量）；元数据补 `license`。
pub const MIGRATION_TABLE: &[(i64, i64)] = &[(1, 2), (2, 3)];

/// 迁移一跳是否存在。
pub fn has_migration(from: i64, to: i64) -> bool {
    MIGRATION_TABLE.iter().any(|(f, t)| *f == from && *t == to)
}

/// 迁移表是否覆盖 `from` 的出边（表驱动的「有这一步」判据）。
///
/// **单独暴露是有原因的**：门禁必须能区分「按表逐步走」与「按版本号 +1 猜」。
/// 若只看 [`has_migration`]，一个 `cur+1` 的实现也能给出同样的 `path(1,3)`，
/// 于是迁移门禁恒真——那条纪律就成了没人验证的承诺。
pub fn has_migration_from(from: i64) -> bool {
    MIGRATION_TABLE.iter().any(|(f, _)| *f == from)
}

/// 求从 `from` 到 `to` 的迁移路径（`vec![(1,2),(2,3)]`）。
///
/// 返回 `None` = 不可达。**不做贪心假设**：只认表里逐跳相接的链。
pub fn migration_path(from: i64, to: i64) -> Option<Vec<(i64, i64)>> {
    if from == to {
        return Some(Vec::new());
    }
    // 降级方向（to < from）不可达：迁移链只前进，不回退。
    if to < from {
        return None;
    }
    let mut path: Vec<(i64, i64)> = Vec::new();
    let mut cur = from;
    // 循环上限**只作防环护栏，不承担语义**：若把它设成 `表长`，那么一个
    // 「按版本号 +1 猜」的伪实现会在爬到表底时恰好耗尽配额而返回 `None`，
    // 与真表驱动的「无此边」不可区分——迁移门禁于是恒真。故给足余量，
    // 让**只有表**能决定路径是否存在。
    for _ in 0..MAX_MIGRATION_HOPS {
        if cur == to {
            return Some(path);
        }
        // 只认表里以 `cur` 为起点的边——不按「版本号 +1」猜。
        let step = MIGRATION_TABLE.iter().find(|(f, _)| *f == cur).map(|(_, t)| *t);
        match step {
            Some(next) => {
                path.push((cur, next));
                cur = next;
            }
            None => return None,
        }
    }
    None
}

/// 迁移步数防环上限（远大于任何真实迁移链；表无环时不会触到）。
pub const MAX_MIGRATION_HOPS: usize = 64;

/// 执行一跳迁移（就地改 Json 树）。
fn apply_migration_step(root: &mut Json, from: i64, report: &mut AssetReport) -> bool {
    let segments = match root.get("segments") {
        Some(s) if s.is_obj() => s.clone(),
        _ => {
            report.error(
                AssetDiag::MigrationFailed,
                None,
                "容器缺 segments 子树，无法迁移",
                "文件不是本格式；请确认来源",
            );
            return false;
        }
    };

    let mut tracks = segments
        .get("tracks")
        .and_then(|t| t.get_arr("items").cloned())
        .unwrap_or_default();
    let mut meta = segments.get("meta").cloned();

    match from {
        1 => {
            for t in tracks.iter_mut() {
                if t.get_bool("blended").is_none() {
                    if let Json::Obj(f) = t {
                        f.push(("blended".to_string(), Json::Bool(false)));
                    }
                }
            }
            if let Some(Json::Obj(f)) = meta.as_mut() {
                if f.iter().all(|(k, _)| k != "loop_default") {
                    f.push(("loop_default".to_string(), Json::Str("once".to_string())));
                }
            }
            report.info(
                AssetDiag::SignatureScrubbed,
                "migrate",
                "blended",
                "v1→v2：轨道补显式混合标记",
            );
        }
        2 => {
            for t in tracks.iter_mut() {
                if t.get("comps").is_none() {
                    if let Json::Obj(f) = t {
                        f.push(("comps".to_string(), Json::Int(1)));
                    }
                }
            }
            if let Some(Json::Obj(f)) = meta.as_mut() {
                if f.iter().all(|(k, _)| k != "license") {
                    f.push(("license".to_string(), Json::Null));
                }
            }
            report.info(
                AssetDiag::SignatureScrubbed,
                "migrate",
                "comps",
                "v2→v3：曲线补分量数（v2 隐含 1）",
            );
        }
        other => {
            report.error(
                AssetDiag::MigrationFailed,
                None,
                &format!("没有从 v{} 出发的迁移器", other),
                "这是迁移表缺口；补 MIGRATION_TABLE 或显式拒绝该版本",
            );
            return false;
        }
    }

    // 把改动写回 root。
    let mut new_segments = segments.clone();
    if let Json::Obj(fs) = &mut new_segments {
        for (k, v) in fs.iter_mut() {
            if k == "tracks" {
                *v = Json::Obj(vec![("items".to_string(), Json::Arr(tracks.clone()))]);
            }
            if k == "meta" {
                *v = meta.clone().unwrap_or(Json::Obj(Vec::new()));
            }
        }
    }
    if let Json::Obj(rootf) = root {
        for (k, v) in rootf.iter_mut() {
            if k == "segments" {
                *v = new_segments.clone();
            }
        }
    }
    true
}

/// 把容器树从 `from` 迁到 `to`。
pub fn migrate_json(root: &mut Json, from: i64, to: i64, report: &mut AssetReport) -> bool {
    let path = match migration_path(from, to) {
        Some(p) => p,
        None => {
            report.error(
                AssetDiag::NoMigrationPath,
                None,
                &format!("v{} 到 v{} 无迁移链", from, to),
                "补迁移器或显式拒绝；不得按「逐版本 +1」静默放行",
            );
            return false;
        }
    };
    for (f, t) in path.iter() {
        if !apply_migration_step(root, *f, report) {
            return false;
        }
        set_int_field(root, "version", *t);
    }
    true
}

/// 写整数字段（存在则改，不存在则追加末尾）。
fn set_int_field(root: &mut Json, key: &str, value: i64) {
    if let Json::Obj(fields) = root {
        let mut found = false;
        for (k, v) in fields.iter_mut() {
            if k == key {
                *v = Json::Int(value);
                found = true;
            }
        }
        if !found {
            fields.push((key.to_string(), Json::Int(value)));
        }
    }
}

// ===========================================================================
// §7 序列化（导出）
// ===========================================================================

fn opt_str(v: &Option<String>) -> Json {
    match v {
        Some(s) => Json::Str(s.clone()),
        None => Json::Null,
    }
}

fn meta_to_json(meta: &MetaSegment) -> Json {
    Json::Obj(vec![
        ("asset_name".to_string(), Json::Str(meta.asset_name.clone())),
        ("author".to_string(), opt_str(&meta.author)),
        ("license".to_string(), opt_str(&meta.license)),
        ("source_url".to_string(), opt_str(&meta.source_url)),
        ("origin_path".to_string(), opt_str(&meta.origin_path)),
        ("machine_id".to_string(), opt_str(&meta.machine_id)),
        (
            "loop_default".to_string(),
            Json::Str(meta.loop_default.as_str().to_string()),
        ),
    ])
}

fn curves_to_json(curves: &[CurveSegment]) -> Json {
    let mut items: Vec<Json> = Vec::with_capacity(curves.len());
    for c in curves.iter() {
        let times: Vec<Json> = c.times.iter().map(|t| Json::Int(*t as i64)).collect();
        let values: Vec<Json> = c.values.iter().map(|v| Json::Num(*v)).collect();
        items.push(Json::Obj(vec![
            ("curve_id".to_string(), Json::Str(c.curve_id.clone())),
            ("comps".to_string(), Json::Int(c.comps as i64)),
            ("times".to_string(), Json::Arr(times)),
            ("values".to_string(), Json::Arr(values)),
        ]));
    }
    Json::Obj(vec![("items".to_string(), Json::Arr(items))])
}

fn tracks_to_json(tracks: &[TrackSegment]) -> Json {
    let mut items: Vec<Json> = Vec::with_capacity(tracks.len());
    for t in tracks.iter() {
        items.push(Json::Obj(vec![
            ("track_id".to_string(), Json::Str(t.track_id.clone())),
            ("owner".to_string(), Json::Str(t.owner.clone())),
            ("class".to_string(), Json::Str(t.class.as_str().to_string())),
            ("interp".to_string(), Json::Str(t.interp.as_str().to_string())),
            ("curve_id".to_string(), Json::Str(t.curve_id.clone())),
            ("bind".to_string(), Json::Str(t.bind_raw.clone())),
            ("weight".to_string(), Json::Num(t.weight)),
            ("blended".to_string(), Json::Bool(t.blended)),
        ]));
    }
    Json::Obj(vec![("items".to_string(), Json::Arr(items))])
}

fn clips_to_json(clips: &[ClipSegment]) -> Json {
    let mut items: Vec<Json> = Vec::with_capacity(clips.len());
    for c in clips.iter() {
        let refs: Vec<Json> = c
            .track_ids
            .iter()
            .map(|s| Json::Str(s.clone()))
            .collect();
        items.push(Json::Obj(vec![
            ("clip_id".to_string(), Json::Str(c.clip_id.clone())),
            ("name".to_string(), Json::Str(c.name.clone())),
            ("duration".to_string(), Json::Int(c.duration_ticks as i64)),
            ("tracks".to_string(), Json::Arr(refs)),
            (
                "loop".to_string(),
                Json::Str(c.loop_mode.as_str().to_string()),
            ),
        ]));
    }
    Json::Obj(vec![("items".to_string(), Json::Arr(items))])
}

/// 资产 → 容器 Json 树（**未封哈希**；`export_container` 负责封）。
fn asset_to_json(asset: &AnimAsset, export_meta: &MetaSegment) -> Json {
    let mut segments: Vec<(String, Json)> = Vec::with_capacity(4);
    for kind in SegmentKind::ALL.into_iter() {
        let body = match kind {
            SegmentKind::Meta => meta_to_json(export_meta),
            SegmentKind::Curves => curves_to_json(&asset.curves),
            SegmentKind::Tracks => tracks_to_json(&asset.tracks),
            SegmentKind::Clips => clips_to_json(&asset.clips),
        };
        segments.push((kind.key().to_string(), seal_segment(&body)));
    }
    let mut root: Vec<(String, Json)> = Vec::with_capacity(5);
    root.push((
        "schema".to_string(),
        Json::Str(SEGMENT_SCHEMA.to_string()),
    ));
    root.push((
        "magic".to_string(),
        Json::Str(String::from_utf8_lossy(&CONTAINER_MAGIC).to_string()),
    ));
    root.push(("version".to_string(), Json::Int(CONTAINER_VERSION)));
    root.push(("segments".to_string(), Json::Obj(segments)));
    // `content_hash` 留空待填——它按已封好的 segments 计算。
    root.push((CONTENT_HASH_FIELD.to_string(), Json::Str(String::new())));
    Json::Obj(root)
}

/// 填根内容哈希（覆盖 `magic + version + segments`，不含自身）。
pub fn seal_content_hash(root: &mut Json) {
    let magic = match root.get_str("magic") {
        Some(m) => m.to_string(),
        None => String::new(),
    };
    let version = match root.get("version") {
        Some(Json::Int(v)) => *v,
        _ => 0,
    };
    let segs_text = match root.get("segments") {
        Some(s) => write_to_string(s),
        None => String::from(""),
    };
    let payload = format!("{}|{}|{}", magic, version, segs_text);
    let h = hex16(fnv1a64(payload.as_bytes()));
    let mut filled = false;
    if let Json::Obj(fields) = root {
        for (k, v) in fields.iter_mut() {
            if k == CONTENT_HASH_FIELD {
                *v = Json::Str(h.clone());
                filled = true;
            }
        }
    }
    if !filled {
        // 缺 content_hash 字段：显性补上（静默不填等于让根校验永远缺席）。
        if let Json::Obj(fields) = root {
            fields.push((CONTENT_HASH_FIELD.to_string(), Json::Str(h)));
        }
    }
}

/// 容器文本 → 资产（含校验、迁移、签名、跨段引用）。
///
/// 流程顺序是**刻意的**：
/// 1. 语法 → 2. schema/魔数 → 3. 版本判定与迁移 → 4. 段哈希校验（定位损坏段）
/// → 5. 段形状与内容 → 6. 跨段引用 → 7. 签名分级。
///
/// 为什么哈希校验排在形状校验**之前**：形状错可能只是我们不认识的未来字段，
/// 而哈希失配意味着文件确实被动过——那是要立刻拒的，不能"尽力解析一部分"。
pub fn import_container(text: &str) -> AssetOutcome<AnimAsset> {
    let mut report = AssetReport::new();
    let root = match parse_json(text, &mut report) {
        Ok(v) => v,
        Err(e) => {
            report.error(
                AssetDiag::SyntaxMalformed,
                None,
                &format!("JSON 语法错：{}", e),
                "文件可能被截断或不是本格式；确认传输完整",
            );
            return AssetOutcome::err_from(&report);
        }
    };
    if report.has_errors() {
        return AssetOutcome::err_from(&report);
    }
    if !root.is_obj() {
        report.error(
            AssetDiag::SchemaMismatch,
            None,
            "容器根不是对象",
            "m.anim. 容器根必须是对象",
        );
        return AssetOutcome::err_from(&report);
    }

    // --- schema 与魔数 ---
    match root.get_str("schema") {
        Some(s) if s == SEGMENT_SCHEMA => {}
        Some(other) => {
            report.error(
                AssetDiag::SchemaMismatch,
                None,
                &format!("schema 为 \"{}\"，不是 \"{}\"", other, SEGMENT_SCHEMA),
                "生态段名不匹配；确认文件属于哪一册哪一域",
            );
            return AssetOutcome::err_from(&report);
        }
        None => {
            report.error(
                AssetDiag::SchemaMismatch,
                None,
                "容器缺 schema 字段",
                "无 schema 无法确认属域；拒绝猜测",
            );
            return AssetOutcome::err_from(&report);
        }
    }
    match root.get_str("magic") {
        Some(m) if m.as_bytes() == CONTAINER_MAGIC => {}
        Some(other) => {
            report.error(
                AssetDiag::MagicMismatch,
                None,
                &format!("魔数为 \"{}\"，不是 \"{}\"", other, String::from_utf8_lossy(&CONTAINER_MAGIC)),
                "文件不是 m.anim. 容器；勿把其它格式当动画资产导入",
            );
            return AssetOutcome::err_from(&report);
        }
        None => {
            report.error(
                AssetDiag::MagicMismatch,
                None,
                "容器缺 magic 字段",
                "无魔数无法排除格式混淆",
            );
            return AssetOutcome::err_from(&report);
        }
    }

    // --- 版本与迁移 ---
    let raw_version = match root.get("version") {
        Some(Json::Int(v)) => *v,
        Some(_) => {
            report.error(
                AssetDiag::SchemaMismatch,
                None,
                "version 不是整数",
                "版本号必须是整数；非整数多半是字段错位",
            );
            return AssetOutcome::err_from(&report);
        }
        None => {
            report.error(
                AssetDiag::SchemaMismatch,
                None,
                "容器缺 version 字段",
                "无版本无法决定迁移路径；拒绝按最新版猜测",
            );
            return AssetOutcome::err_from(&report);
        }
    };
let mut root = root;
    // 版本先只做「未来 / 当期 / 过旧」的**分诊**，迁移一律推迟到校验之后。
    //
    // **顺序是红线**：完整性必须对着「文件写成什么样」来验，而不是对着
    // 「迁移后它该变成什么样」来验。迁移会改写段内容（补字段）并抬高 version，
    // 而哈希是把这两样都算进输入的——先迁移再校验，等于拿新内容去比旧哈希，
    // **任何旧版本文件都必然失配**，版本化能力形同虚设。
    let version = raw_version;
    if version > CONTAINER_VERSION {
    report.push(
            AssetDiag::VersionFromFuture,
            None,
            &format!(
                "容器版本 v{} 高于当前实现 v{}",
                version, CONTAINER_VERSION
            ),
            "按前向兼容处理：能识别的段照读，未知段跳过并声明；请升级以完整支持",
        );
    }

    // --- 段收集与未知段跳过 ---
    let segments = match root.get("segments") {
        Some(s) if s.is_obj() => s.clone(),
        _ => {
            report.error(
                AssetDiag::SegmentShapeInvalid,
                None,
                "容器缺 segments 子树或子树不是对象",
                "四类段都装在 segments 下",
            );
            return AssetOutcome::err_from(&report);
        }
    };
    let seg_fields: Vec<(String, Json)> = match &segments {
        Json::Obj(f) => f.clone(),
        _ => Vec::new(),
    };
    for (k, _) in seg_fields.iter() {
        if SegmentKind::from_key(k).is_none() {
            report.push(
                AssetDiag::SegmentUnknown,
                None,
                &format!("跳过未知段 \"{}\"（前向兼容）", k),
                "该段来自更高版本；当前实现会忽略它，回写导出时该段数据将丢失",
            );
        }
    }

    // --- 内容哈希校验（用文件里的 version，不是迁移后的） ---
    let declared_root = parse_hex16(root.get_str(CONTENT_HASH_FIELD).unwrap_or(""));
    let magic = root.get_str("magic").unwrap_or("").to_string();
    let segs_text = write_to_string(&segments);
    let actual_root = fnv1a64(format!("{}|{}|{}", magic, version, segs_text).as_bytes());
    match declared_root {
        Some(d) => {
            if d != actual_root {
                // **不在此早退**：根哈希只说「有一段被改过」，锚点要求
                // 「三要素拒绝 + 定位段」。早退会让段名永远丢失，读日志的人
                // 只能自己去二分。这里只记账，随后逐段校验去指名。
                report.error(
                    AssetDiag::SegmentHashMismatch,
                    None,
                    &format!(
                        "容器内容哈希失配（声明 {} 实测 {}）",
                        hex16(d),
                        hex16(actual_root)
                    ),
                    "根哈希覆盖 segments 全子树；失配说明至少一段被改过，逐段校验给出段名",
                );
            }
        }
        None => {
            report.error(
                AssetDiag::SegmentShapeInvalid,
                None,
                &format!("容器 {} 字段不是合法 16 位十六进制", CONTENT_HASH_FIELD),
                "内容哈希缺失或格式错；拒绝在无完整性校验下装载",
            );
            return AssetOutcome::err_from(&report);
        }
    }

    // --- 四类段齐备 + 逐段哈希（根哈希失配时由这里指名段） ---
    // 按段键取值而非按下标：下标法要求「`ALL.len()` 恰好等于数组长度」这条
    // 不变式由人肉保证，一次段枚举扩容就会静默越界。键法自带此保证。
    let empty = Json::Obj(Vec::new());
    let mut seg_meta: Option<Json> = None;
    let mut seg_curves: Option<Json> = None;
    let mut seg_tracks: Option<Json> = None;
    let mut seg_clips: Option<Json> = None;
    for kind in SegmentKind::ALL.into_iter() {
        match segments.get(kind.key()) {
            Some(seg) => {
                if !verify_segment(seg, kind.key(), &mut report) {
                    return AssetOutcome::err_from(&report);
                }
                let slot = match kind {
                    SegmentKind::Meta => &mut seg_meta,
                    SegmentKind::Curves => &mut seg_curves,
                    SegmentKind::Tracks => &mut seg_tracks,
                    SegmentKind::Clips => &mut seg_clips,
                };
                *slot = Some(seg.clone());
            }
            None => {
                report.error(
                    AssetDiag::SegmentMissing,
                    Some(kind.key()),
                    &format!("容器缺 {} 段", kind.display_name()),
                    "四类段是容器契约的一部分；空段也要显式写出",
                );
            }
        }
    }
    if report.has_errors() {
        return AssetOutcome::err_from(&report);
    }

    // --- 完整性通过后才迁移（旧版本文件此时才被改写） ---
    if version < CONTAINER_VERSION {
        if !migrate_json(&mut root, version, CONTAINER_VERSION, &mut report) {
            return AssetOutcome::err_from(&report);
        }
        // 迁移改了段内容，须重取一次段：迁移后的形状才是下面解析的对象。
        let migrated = match root.get("segments") {
            Some(s) if s.is_obj() => s.clone(),
            _ => {
                report.error(
                    AssetDiag::MigrationFailed,
                    None,
                    "迁移后容器缺 segments 子树",
                    "迁移器破坏了容器结构；这是迁移器缺陷",
                );
                return AssetOutcome::err_from(&report);
            }
        };
        let mut re_meta: Option<Json> = None;
        let mut re_curves: Option<Json> = None;
        let mut re_tracks: Option<Json> = None;
        let mut re_clips: Option<Json> = None;
        for kind in SegmentKind::ALL.into_iter() {
            let seg = match migrated.get(kind.key()) {
                Some(s) => s.clone(),
                None => continue,
            };
            let slot = match kind {
                SegmentKind::Meta => &mut re_meta,
                SegmentKind::Curves => &mut re_curves,
                SegmentKind::Tracks => &mut re_tracks,
                SegmentKind::Clips => &mut re_clips,
            };
            *slot = Some(seg);
        }
        seg_meta = re_meta;
        seg_curves = re_curves;
        seg_tracks = re_tracks;
        seg_clips = re_clips;
    }

    // --- 形状解析 ---
    let meta_seg = seg_meta.unwrap_or(empty.clone());
    let curve_seg = seg_curves.unwrap_or(empty.clone());
    let track_seg = seg_tracks.unwrap_or(empty.clone());
    let clip_seg = seg_clips.unwrap_or(empty);

    let meta = parse_meta(&meta_seg, &mut report);
    let curves = parse_curves(&curve_seg, &mut report);
    let tracks = parse_tracks(&track_seg, &mut report);
    let clips = parse_clips(&clip_seg, &mut report);

    if report.has_errors() {
        return AssetOutcome::err_from(&report);
    }

    let asset = AnimAsset {
        meta,
        curves,
        tracks,
        clips,
    };
    asset.validate(&mut report);
    if report.has_errors() {
        return AssetOutcome::err_from(&report);
    }
    AssetOutcome::seal(asset, &report)
}

fn parse_meta(seg: &Json, report: &mut AssetReport) -> MetaSegment {
    let seg_name = SegmentKind::Meta.key();
    let name = match seg.get_str("asset_name") {
        Some(s) => s.to_string(),
        None => {
            report.error(
                AssetDiag::SegmentShapeInvalid,
                Some(seg_name),
                "元数据段缺 asset_name",
                "资产名是必填；缺失会让资产库里全是无名条目",
            );
            String::new()
        }
    };
    let take_opt = |k: &str| -> Option<String> {
        match seg.get(k) {
            Some(Json::Str(s)) => Some(s.clone()),
            _ => None,
        }
    };
    let loop_default = match seg.get_str("loop_default") {
        Some(s) => match LoopMode::from_name(s) {
            Some(m) => m,
            None => {
                report.error(
                    AssetDiag::SegmentShapeInvalid,
                    Some(seg_name),
                    &format!("loop_default \"{}\" 不是 once/loop/pingpong", s),
                    "三值封闭集；勿写入自定义模式",
                );
                LoopMode::Once
            }
        },
        None => LoopMode::Once,
    };
    MetaSegment {
        asset_name: name,
        author: take_opt("author"),
        license: take_opt("license"),
        source_url: take_opt("source_url"),
        origin_path: take_opt("origin_path"),
        machine_id: take_opt("machine_id"),
        loop_default,
    }
}

fn parse_curves(seg: &Json, report: &mut AssetReport) -> Vec<CurveSegment> {
    let seg_name = SegmentKind::Curves.key();
    let items = match seg.get_arr("items") {
        Some(v) => v.clone(),
        None => {
            report.error(
                AssetDiag::SegmentShapeInvalid,
                Some(seg_name),
                "曲线段缺 items 数组",
                "空曲线段也要写 items: []",
            );
            return Vec::new();
        }
    };
    let mut out = Vec::with_capacity(items.len());
    for item in items.iter() {
        let id = item.get_str("curve_id").unwrap_or("").to_string();
        if id.is_empty() {
            report.error(
                AssetDiag::SegmentShapeInvalid,
                Some(seg_name),
                "曲线条目缺 curve_id",
                "id 是轨道引用键，不可为空",
            );
            continue;
        }
        let comps = match item.get_int("comps") {
            Some(v) if v >= 0 => v as usize,
            _ => {
                report.error(
                    AssetDiag::SegmentShapeInvalid,
                    Some(seg_name),
                    &format!("曲线 {} 缺合法 comps", id),
                    "v3 起 comps 为必填；缺它会默认为标量而错解四分量曲线",
                );
                1
            }
        };
        let times_raw = item.get_int_arr("times").unwrap_or_default();
        let values_raw = item.get_num_arr("values").unwrap_or_default();
        let times: Vec<u32> = times_raw
            .iter()
            .filter(|v| **v >= 0 && **v <= u32::MAX as i64)
            .map(|v| *v as u32)
            .collect();
        if times.len() != times_raw.len() {
            report.error(
                AssetDiag::ValueOutOfDomain,
                Some(seg_name),
                &format!("曲线 {} 含越界时间戳（须落在 0..=4294967295）", id),
                "tick 是 u32；越界值会静默截断成另一个时刻",
            );
        }
        out.push(CurveSegment::build(&id, comps, times, values_raw, report));
    }
    out
}

fn parse_tracks(seg: &Json, report: &mut AssetReport) -> Vec<TrackSegment> {
    let seg_name = SegmentKind::Tracks.key();
    let items = match seg.get_arr("items") {
        Some(v) => v.clone(),
        None => {
            report.error(
                AssetDiag::SegmentShapeInvalid,
                Some(seg_name),
                "轨道段缺 items 数组",
                "空轨道段也要写 items: []",
            );
            return Vec::new();
        }
    };
    let mut out = Vec::with_capacity(items.len());
    for item in items.iter() {
        let id = item.get_str("track_id").unwrap_or("").to_string();
        if id.is_empty() {
            report.error(
                AssetDiag::SegmentShapeInvalid,
                Some(seg_name),
                "轨道条目缺 track_id",
                "id 是 clip 引用键，不可为空",
            );
            continue;
        }
        let class = match item.get_str("class").and_then(TrackClass::from_name) {
            Some(c) => c,
            None => {
                report.error(
                    AssetDiag::TrackClassUnknown,
                    Some(seg_name),
                    &format!(
                        "轨道 {} 的 class \"{}\" 不在六类封闭集内",
                        id,
                        item.get_str("class").unwrap_or("<缺>")
                    ),
                    "六类：position/rotation/scale/color/float/bool；新增类须改 F2402 规格表",
                );
                TrackClass::Float
            }
        };
        let interp = match item.get_str("interp") {
            Some("linear") => InterpKind::Linear,
            Some("slerp") => InterpKind::Slerp,
            Some("step") => InterpKind::Step,
            Some(other) => {
                report.error(
                    AssetDiag::InterpUnknown,
                    Some(seg_name),
                    &format!("轨道 {} 的 interp \"{}\" 未知", id, other),
                    "三值域：linear/slerp/step；自定义插值器不入容器（走 F2403 注册面）",
                );
                InterpKind::Linear
            }
            None => {
                report.error(
                    AssetDiag::InterpUnknown,
                    Some(seg_name),
                    &format!("轨道 {} 缺 interp", id),
                    "插值器必须显式写出；缺省会让读回方按实现顺序猜",
                );
                InterpKind::Linear
            }
        };
        out.push(TrackSegment {
            track_id: id.clone(),
            owner: item.get_str("owner").unwrap_or("").to_string(),
            class,
            interp,
            curve_id: item.get_str("curve_id").unwrap_or("").to_string(),
            bind_raw: item.get_str("bind").unwrap_or("").to_string(),
            weight: match item.get("weight") {
                Some(Json::Num(v)) => *v,
                Some(Json::Int(v)) => *v as f32,
                _ => {
                    report.error(
                        AssetDiag::TrackWeightInvalid,
                        Some(seg_name),
                        &format!("轨道 {} 缺 weight", id),
                        "权重必填；缺省会与「权重 0（不参与混合）」混淆",
                    );
                    1.0
                }
            },
            blended: item.get_bool("blended").unwrap_or(false),
        });
    }
    out
}

fn parse_clips(seg: &Json, report: &mut AssetReport) -> Vec<ClipSegment> {
    let seg_name = SegmentKind::Clips.key();
    let items = match seg.get_arr("items") {
        Some(v) => v.clone(),
        None => {
            report.error(
                AssetDiag::SegmentShapeInvalid,
                Some(seg_name),
                "clip 段缺 items 数组",
                "空 clip 段也要写 items: []",
            );
            return Vec::new();
        }
    };
    let mut out = Vec::with_capacity(items.len());
    for item in items.iter() {
        let id = item.get_str("clip_id").unwrap_or("").to_string();
        if id.is_empty() {
            report.error(
                AssetDiag::SegmentShapeInvalid,
                Some(seg_name),
                "clip 条目缺 clip_id",
                "id 不可为空",
            );
            continue;
        }
        let duration = match item.get_int("duration") {
            Some(v) if v >= 0 && v <= u32::MAX as i64 => v as u32,
            Some(v) => {
                report.error(
                    AssetDiag::ClipDurationInvalid,
                    Some(seg_name),
                    &format!("clip {} 的 duration {} 越界", id, v),
                    "时长是 u32 tick",
                );
                0
            }
            None => {
                report.error(
                    AssetDiag::ClipDurationInvalid,
                    Some(seg_name),
                    &format!("clip {} 缺 duration", id),
                    "时长必填；零时长 clip 无法求值",
                );
                0
            }
        };
        let loop_mode = match item.get_str("loop") {
            Some(s) => match LoopMode::from_name(s) {
                Some(m) => m,
                None => {
                    report.error(
                        AssetDiag::SegmentShapeInvalid,
                        Some(seg_name),
                        &format!("clip {} 的 loop \"{}\" 未知", id, s),
                        "三值封闭集：once/loop/pingpong",
                    );
                    LoopMode::Once
                }
            },
            None => LoopMode::Once,
        };
        out.push(ClipSegment {
            clip_id: id,
            name: item.get_str("name").unwrap_or("").to_string(),
            duration_ticks: duration,
            track_ids: item.get_str_arr("tracks").unwrap_or_default(),
            loop_mode,
        });
    }
    out
}

// ===========================================================================
// §8 导出与往返断言
// ===========================================================================

/// 导出为容器文本。
///
/// **导出前必做两件事**：清洗隐私字段（非有限值在 `validate` 里已拦）。
/// 产出的是**确定性文本**——同一资产两次导出逐字节相同，这是往返断言的基础。
pub fn export_container(asset: &AnimAsset) -> AssetOutcome<String> {
    let mut report = AssetReport::new();
    asset.validate(&mut report);
    if report.has_errors() {
        return AssetOutcome::err_from(&report);
    }
    let export_meta = meta_for_export(&asset.meta, &mut report);
    let mut root = asset_to_json(asset, &export_meta);
    seal_content_hash(&mut root);
    AssetOutcome::seal(write_to_string(&root), &report)
}

/// 往返判定结果。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RoundTripVerdict {
    /// 两次导出文本是否逐字节相同。
    pub identical: bool,
    /// 首个差异字节下标（相同则 `None`）。
    pub first_diff: Option<usize>,
    /// 定位到的丢失段（漂移时给出）。
    pub lost_segment: Option<&'static str>,
    /// 两次文本长度。
    pub first_len: usize,
    pub second_len: usize,
}

/// 首次字节差异下标。
pub fn first_diff_byte(a: &str, b: &str) -> Option<usize> {
    let ab = a.as_bytes();
    let bb = b.as_bytes();
    let n = if ab.len() < bb.len() { ab.len() } else { bb.len() };
    for i in 0..n {
        if ab.get(i) != bb.get(i) {
            return Some(i);
        }
    }
    if ab.len() != bb.len() {
        return Some(n);
    }
    None
}

/// 定位漂移发生在哪一段（逐段比对两棵树的段子树文本）。
///
/// **定位到段是锚点明确要求的**（"定位丢失段"）。只报"两次不一致"等于把
/// 二分查找的活推给读日志的人。
pub fn localize_drift(a: &str, b: &str) -> Option<&'static str> {
    let mut ra = AssetReport::new();
    let mut rb = AssetReport::new();
    let ta = parse_json(a, &mut ra);
    let tb = parse_json(b, &mut rb);
    if ta.is_err() || tb.is_err() {
        return None;
    }
    let (va, vb) = (ta.ok()?, tb.ok()?);
    for kind in SegmentKind::ALL.into_iter() {
        let sa = va.get("segments").and_then(|s| s.get(kind.key()));
        let sb = vb.get("segments").and_then(|s| s.get(kind.key()));
        let ta_seg = sa.map(write_to_string);
        let tb_seg = sb.map(write_to_string);
        if ta_seg != tb_seg {
            return Some(kind.key());
        }
    }
    // 段全同 → 差异在段外（schema/magic/version/content_hash）。
    let ha = va.get(CONTENT_HASH_FIELD);
    let hb = vb.get(CONTENT_HASH_FIELD);
    if ha != hb {
        return Some("content_hash");
    }
    None
}

/// 往返断言：导出 → 导入 → 再导出，两次导出逐位 diff。
///
/// 返回 `Err` 表示**往返过程本身失败**（导出或导入被拒）；返回 `Ok` 且
/// `identical = false` 才是「跑通了但数据丢了」——那是要立案的 P1。
///
/// 这两件事必须分开报：把「导入被拒」和「往返有损」混成一个失败态，
/// 会让定位从"哪个环节拒了"退化成"总之失败了"。
pub fn round_trip(asset: &AnimAsset) -> AssetOutcome<RoundTripVerdict> {
    let mut report = AssetReport::new();
    let first = match export_container(asset) {
        AssetOutcome::Ok { value, report: r } => {
            report.absorb(&r);
            value
        }
        AssetOutcome::Err { report: r } => {
            report.absorb(&r);
            return AssetOutcome::Err { report };
        }
    };
    let imported = match import_container(&first) {
        AssetOutcome::Ok { value, report: r } => {
            report.absorb(&r);
            value
        }
        AssetOutcome::Err { report: r } => {
            report.absorb(&r);
            return AssetOutcome::Err { report };
        }
    };
    let second = match export_container(&imported) {
        AssetOutcome::Ok { value, report: r } => {
            report.absorb(&r);
            value
        }
        AssetOutcome::Err { report: r } => {
            report.absorb(&r);
            return AssetOutcome::Err { report };
        }
    };
    let verdict = audit_round_trip(&first, &second, &mut report);
    AssetOutcome::seal(verdict, &report)
}

/// 往返裁决：比对两份容器文本，逐字节判定并**在漂移时把 P1 立案写进 `report`**。
///
/// **为什么独立成函数**：[`round_trip`] 的入口是「资产」，于是它天然只能测
/// 「自己导出的东西能不能回来」。而**有损的那一类**恰恰发生在我方实现之外
/// ——例如容器带未知段（前向兼容跳过）时，导入再导出必然丢掉那段，往返必然
/// 有损。若没有一条能直接喂两份文本的裁决路径，这条最重要的红线就永远测不到，
/// 「往返判红」本身会退化成一句空话。
pub fn audit_round_trip(first: &str, second: &str, report: &mut AssetReport) -> RoundTripVerdict {
    let diff = first_diff_byte(first, second);
    let verdict = RoundTripVerdict {
        identical: diff.is_none(),
        first_diff: diff,
        lost_segment: if diff.is_some() {
            localize_drift(first, second)
        } else {
            None
        },
        first_len: first.len(),
        second_len: second.len(),
    };
    if !verdict.identical {
        report.error(
            AssetDiag::RoundTripDrift,
            verdict.lost_segment,
            &format!(
                "往返不一致：首差字节 {:?}（长度 {} vs {}）",
                verdict.first_diff, verdict.first_len, verdict.second_len
            ),
            "保真红线：定位到的段须逐字段比对；这是 P1，先修实现再谈资产",
        );
        report.error(
            AssetDiag::RoundTripLossySegment,
            verdict.lost_segment,
            &match verdict.lost_segment {
                Some(s) => format!("丢失段定位到 {}", s),
                None => String::from("丢失段无法定位到具体段（差异在段外字段）"),
            },
            "若差异在 content_hash，说明段内容一致而哈希算法或输入拼接有问题",
        );
    }
    verdict
}

// ===========================================================================
// §9 版本化之备份/还原
// ===========================================================================

/// 资产备份（版本化三件套之「备份」）。
///
/// 备份存**文本**而非结构体：备份的意义是"将来某个版本的实现读得回来"。
/// 存结构体的话，实现一改字段布局，旧备份就再也读不出来了。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AssetBackup {
    /// 备份时的 schema。
    pub schema: &'static str,
    /// 备份时的版本。
    pub version: i64,
    /// 容器文本。
    pub text: String,
}

/// 生成备份。
pub fn backup(asset: &AnimAsset) -> AssetOutcome<AssetBackup> {
    match export_container(asset) {
        AssetOutcome::Ok { value, report } => AssetOutcome::Ok {
            value: AssetBackup {
                schema: SEGMENT_SCHEMA,
                version: CONTAINER_VERSION,
                text: value,
            },
            report,
        },
        AssetOutcome::Err { report } => AssetOutcome::Err { report },
    }
}

/// 从备份还原（走完整导入管线，校验一步不少）。
pub fn restore(b: &AssetBackup) -> AssetOutcome<AnimAsset> {
    if b.schema != SEGMENT_SCHEMA {
        let mut report = AssetReport::new();
        report.error(
            AssetDiag::SchemaMismatch,
            None,
            &format!("备份 schema 为 \"{}\"，不是 \"{}\"", b.schema, SEGMENT_SCHEMA),
            "跨段备份不可直接还原；须先转段",
        );
        return AssetOutcome::Err { report };
    }
    import_container(&b.text)
}

// ===========================================================================
// §10 与 F2402 轨道容器的接回（单源纪律的物质证明）
// ===========================================================================

/// 由曲线段构造 F2402 载荷（分量数 ↔ 轨道类一致性在此暴露）。
fn payload_from_curve(curve: &CurveSegment, class: TrackClass) -> TrackPayload {
    let n = curve.times.len();
    match class {
        TrackClass::Bool => {
            let mut v: Vec<bool> = Vec::with_capacity(n);
            for i in 0..n {
                let x = curve.component(i, 0).unwrap_or(0.0);
                v.push(x >= 0.5);
            }
            TrackPayload::Bool(v)
        }
        TrackClass::Position | TrackClass::Scale => {
            let mut v: Vec<[f32; 3]> = Vec::with_capacity(n);
            for i in 0..n {
                v.push([
                    curve.component(i, 0).unwrap_or(0.0),
                    curve.component(i, 1).unwrap_or(0.0),
                    curve.component(i, 2).unwrap_or(0.0),
                ]);
            }
            TrackPayload::Vec3(v)
        }
        TrackClass::Color => {
            let mut v: Vec<[f32; 4]> = Vec::with_capacity(n);
            for i in 0..n {
                v.push([
                    curve.component(i, 0).unwrap_or(0.0),
                    curve.component(i, 1).unwrap_or(0.0),
                    curve.component(i, 2).unwrap_or(0.0),
                    curve.component(i, 3).unwrap_or(1.0),
                ]);
            }
            TrackPayload::Vec4(v)
        }
        TrackClass::Rotation => {
            let mut v: Vec<[f32; 4]> = Vec::with_capacity(n);
            for i in 0..n {
                v.push([
                    curve.component(i, 0).unwrap_or(0.0),
                    curve.component(i, 1).unwrap_or(0.0),
                    curve.component(i, 2).unwrap_or(0.0),
                    curve.component(i, 3).unwrap_or(1.0),
                ]);
            }
            TrackPayload::Quat(v)
        }
        TrackClass::Float => TrackPayload::Scalar(curve.values.clone()),
    }
}

/// 把资产重挂回 F2402 的 `TrackContainer`——「搬运回来的东西接得住」的唯一证明。
///
/// 光比对 JSON 文本不够：文本一致但载荷形状对不上容器（分量数与轨道类不符、
/// 绑定路径解析失败），求值期才会炸。这里把每条轨真的 `mount` 一次，
/// 让 F2402 的三路校验替我们把关。
pub fn remount(asset: &AnimAsset) -> AssetOutcome<TrackContainer> {
    let mut report = AssetReport::new();
    asset.validate(&mut report);
    if report.has_errors() {
        return AssetOutcome::err_from(&report);
    }
    let mut container = TrackContainer::new();
    for t in asset.tracks.iter() {
        let curve = match asset.curve(&t.curve_id) {
            Some(c) => c,
            None => {
                report.error(
                    AssetDiag::CurveRefUnknown,
                    Some(SegmentKind::Tracks.key()),
                    &format!("重挂时轨道 {} 的曲线缺失", t.track_id),
                    "先补齐曲线",
                );
                continue;
            }
        };
        let payload = payload_from_curve(curve, t.class);
        let inp = MountInput::new(
            &t.track_id,
            &t.owner,
            t.class.as_str(),
            payload,
            KeyframeRef::new(&t.curve_id, curve.times.len()),
            &t.bind_raw,
        )
        .with_interp(t.interp)
        .with_weight(t.weight)
        .with_blended(t.blended);
        match container.mount(inp) {
            crate::svstar2::vem02_track::Outcome::Ok { .. } => {}
            crate::svstar2::vem02_track::Outcome::Err(f) => {
                report.error(
                    AssetDiag::TrackBindInvalid,
                    Some(SegmentKind::Tracks.key()),
                    &format!("轨道 {} 被 F2402 容器拒绝：{}（{}）", t.track_id, f.message, f.hint),
                    "容器侧拒绝说明资产形状与轨道规格不符；按提示改轨道类或插值器",
                );
            }
        }
    }
    AssetOutcome::seal(container, &report)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn demo_asset() -> AnimAsset {
        let mut report = AssetReport::new();
        let mut a = AnimAsset::empty();
        a.meta = MetaSegment {
            asset_name: "走".to_string(),
            author: Some("作者甲".to_string()),
            license: Some("CC-BY".to_string()),
            source_url: None,
            origin_path: Some("D:\\private\\rig\\walk.json".to_string()),
            machine_id: Some("MACHINE-7".to_string()),
            loop_default: LoopMode::Loop,
        };
        let times = vec![0u32, 10, 20, 30];
        let vals: Vec<f32> = (0..4).map(|i| i as f32 * 0.5).collect();
        a.curves.push(CurveSegment::build("c_pos", 1, times.clone(), vals, &mut report));
        let times2 = vec![0u32, 15, 30];
        let mut q: Vec<f32> = Vec::new();
        for _ in 0..3 {
            q.push(0.0);
            q.push(0.0);
            q.push(0.0);
            q.push(1.0);
        }
        a.curves.push(CurveSegment::build("c_rot", 4, times2, q, &mut report));
        a.tracks.push(TrackSegment {
            track_id: "t_pos".to_string(),
            owner: "e1".to_string(),
            class: TrackClass::Float,
            interp: InterpKind::Linear,
            curve_id: "c_pos".to_string(),
            bind_raw: "/node/anim/pos".to_string(),
            weight: 1.0,
            blended: false,
        });
        a.tracks.push(TrackSegment {
            track_id: "t_rot".to_string(),
            owner: "e1".to_string(),
            class: TrackClass::Rotation,
            interp: InterpKind::Slerp,
            curve_id: "c_rot".to_string(),
            bind_raw: "/node/anim/rot".to_string(),
            weight: 0.5,
            blended: true,
        });
        a.clips.push(ClipSegment {
            clip_id: "clip_walk".to_string(),
            name: "走".to_string(),
            duration_ticks: 30,
            track_ids: vec!["t_pos".to_string(), "t_rot".to_string()],
            loop_mode: LoopMode::Loop,
        });
        a
    }

    #[test]
    fn fnv_and_hex_round_trip() {
        let h = fnv1a64(b"abc");
        assert_eq!(parse_hex16(&hex16(h)), Some(h));
        assert_eq!(parse_hex16("short"), None);
        assert_eq!(parse_hex16("zzzzzzzzzzzzzzzz"), None);
    }

    #[test]
    fn json_round_trip_basic() {
        let mut r = AssetReport::new();
        let src = "{\"a\":[1,2.5,\"x\\u00e9\"],\"b\":{\"c\":true},\"d\":null}";
        let v = parse_json(src, &mut r).expect("parse");
        assert!(!r.has_errors());
        let out = write_to_string(&v);
        assert_eq!(out, "{\"a\":[1,2.5,\"xé\"],\"b\":{\"c\":true},\"d\":null}");
    }

    #[test]
    fn json_f32_display_is_round_trip_exact() {
        let mut r = AssetReport::new();
        for v in [0.1f32, 1.0, -0.0, 1.0 / 3.0, f32::MIN_POSITIVE, 3.4e38] {
            let text = format!("{}", v);
            let back: f32 = text.parse().expect("parse f32");
            assert_eq!(back.to_bits(), v.to_bits(), "f32 {} 往返不逐位", text);
        }
        let j = write_to_string(&Json::Num(0.1));
        let back = parse_json(&j, &mut r).expect("parse json");
        match back {
            Json::Num(v) => assert_eq!(v.to_bits(), 0.1f32.to_bits()),
            _ => panic!("应为数值"),
        }
    }

    #[test]
    fn export_import_round_trip_identical() {
        let a = demo_asset();
        let v = round_trip(&a);
        assert!(v.is_ok(), "往返应成功: {:?}", v.report().notes);
        let verdict = v.ok().expect("value");
        assert!(verdict.identical, "往返应逐位一致，首差 {:?}", verdict.first_diff);
        assert_eq!(verdict.lost_segment, None);
    }

    #[test]
    fn export_scrubs_privacy_fields() {
        let a = demo_asset();
        let text = export_container(&a).ok().expect("导出").clone();
        assert!(!text.contains("private"), "绝对路径必须被剥离");
        assert!(!text.contains("MACHINE-7"), "机器 id 必须被剥离");
        assert!(text.contains("作者甲"), "署名应保留");
        // 原资产不被就地修改。
        assert!(a.meta.origin_path.is_some());
    }

    #[test]
    fn signature_absent_is_warning_not_error() {
        let mut a = demo_asset();
        a.meta.author = None;
        a.meta.license = None;
        a.meta.source_url = None;
        assert_eq!(classify_signature(&a.meta), SigTier::Absent);
        let out = export_container(&a);
        assert!(out.is_ok(), "未署名不阻断");
        assert!(!out.report().warnings().is_empty());
    }

    #[test]
    fn segment_corruption_is_located() {
        let a = demo_asset();
        let text = export_container(&a).ok().expect("导出").clone();
        // 篡改 clips 段里的一个字符（clip 名 "走" → "足"）。
        let bad = text.replace("\"走\"", "\"足\"");
        assert_ne!(text, bad);
        let out = import_container(&bad);
        assert!(!out.is_ok(), "损坏必须被拒");
        let codes: Vec<AssetDiag> = out.report().errors().iter().map(|n| n.code).collect();
        assert!(codes.contains(&AssetDiag::SegmentHashMismatch));
        let seg = out
            .report()
            .errors()
            .iter()
            .find_map(|n| n.segment);
        assert!(seg.is_some(), "必须定位到段");
    }

    #[test]
    fn unknown_segment_is_skipped_with_notice() {
        let a = demo_asset();
        let text = export_container(&a).ok().expect("导出").clone();
        // 手工插一个未知段并重算根哈希。
        let mut r = AssetReport::new();
        let mut root = parse_json(&text, &mut r).expect("parse");
        if let Json::Obj(fields) = &mut root {
            if let Some((_, segs)) = fields.iter_mut().find(|(k, _)| k == "segments") {
                if let Json::Obj(sf) = segs {
                    sf.push(("m.future.".to_string(), Json::Obj(vec![])));
                }
            }
        }
        seal_content_hash(&mut root);
        let bad = write_to_string(&root);
        let out = import_container(&bad);
        assert!(out.is_ok(), "未知段应跳过而非拒绝: {:?}", out.report().notes);
        let warned = out
            .report()
            .warnings()
            .iter()
            .any(|n| n.code == AssetDiag::SegmentUnknown);
        assert!(warned, "跳过必须留下声明");
    }

    #[test]
    fn old_version_without_migration_path_is_rejected() {
        let mut r = AssetReport::new();
        let mut root = parse_json("{\"schema\":\"m.anim.\"}", &mut r).expect("parse");
        set_int_field(&mut root, "version", 0);
        let out = migrate_json(&mut root, 0, CONTAINER_VERSION, &mut r);
        assert!(!out);
        assert_eq!(r.first_code(), Some(AssetDiag::NoMigrationPath));
    }

    #[test]
    fn migration_path_is_explicit() {
        assert_eq!(
            migration_path(1, 3),
            Some(vec![(1, 2), (2, 3)])
        );
        assert_eq!(migration_path(2, 2), Some(Vec::new()));
        assert_eq!(migration_path(1, 4), None);
        assert!(has_migration(1, 2));
        assert!(!has_migration(1, 3), "不得跨跳");
    }

    #[test]
    fn ecosystem_registry_rejects_unregistered() {
        assert!(is_registered("m.anim."));
        assert!(!is_registered("x.unknown."));
        assert_eq!(register_index("x.unknown."), None);
        assert_eq!(expect_domain("l.timeline."), Some("L"));
        assert_eq!(expect_domain("m.anim."), Some("M"));
    }

    #[test]
    fn remount_into_f2402_container() {
        let a = demo_asset();
        let c = remount(&a);
        assert!(c.is_ok(), "重挂应成功: {:?}", c.report().notes);
        let container = c.ok().expect("容器");
        assert_eq!(container.total_tracks(), 2);
    }

    #[test]
    fn backup_restore_preserves_bytes() {
        let a = demo_asset();
        let b = backup(&a).ok().expect("备份").clone();
        let r = restore(&b);
        assert!(r.is_ok());
        let text2 = export_container(r.ok().expect("资产")).ok().expect("导出").clone();
        assert_eq!(text2, b.text, "备份还原应逐字节回到原文本");
    }

    #[test]
    fn non_finite_value_blocks_export() {
        let mut r = AssetReport::new();
        let c = CurveSegment::build("c", 1, vec![0, 1], vec![0.0, f32::NAN], &mut r);
        assert!(r.has_errors());
        assert_eq!(r.first_code(), Some(AssetDiag::NonFiniteValue));
        let mut a = AnimAsset::empty();
        a.curves.push(c);
        assert!(!export_container(&a).is_ok(), "NaN 不得导出");
    }

    #[test]
    fn absolute_path_detection() {
        assert!(looks_absolute_path("D:\\a\\b"));
        assert!(looks_absolute_path("/home/x/y"));
        assert!(looks_absolute_path("/tmp/f"));
        assert!(!looks_absolute_path("relative/path"));
        assert!(!looks_absolute_path("a:b"), "冒号后非分隔符不算绝对路径");
        assert!(!looks_absolute_path(""));
    }

    #[test]
    fn alt_text_mentions_all_tracks() {
        let a = demo_asset();
        let t = a.alt_text();
        assert!(t.contains("t_pos"));
        assert!(t.contains("t_rot"));
        assert!(t.contains("clip_walk") || t.contains("走"));
    }
}