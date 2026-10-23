//! CGPU-F2249 · 降级遥测（CGPU-O 域 · 降级组 · 目标 300 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/CGPU Varix STAR II · 总纲与施工书.md#CGPU-F2249`
//!
//! **判据（锚点原文）**：遥测复用、一组、判据。
//!
//! **职责定位（锚点原文摘录）**：遥测（降级遥测（触发/执行/恢复全链——遥测复用；测试（遥测一组）。
//!
//! ------------------------------------------------------------------
//!
//! 本单是降级全链的**量化遥测子系统**：把降级的**触发**（哪条判据/
//! 哪个信号越过阈值——源指纹 + 档位跳变）/ **执行**（降到了哪一级、
//! 动了哪些参数——档位前后值 + 参数指纹）/ **恢复**（何时回升、滞回
//! 确认——恢复时延 + 滞回确认/回摆计数）三阶段全程记录，每帧粒度落
//! 行，会话级聚合（整数分位 P95/P99 + 卡顿计数 + 降级时长占比 ppm），
//! 并以版本化 schema 发布包对外交付。全链守恒对账（触发数 ≥ 执行数
//! ≥ 恢复数）与跨账对拍（Σ源 ≡ 总）贯穿始终——篡改即破，破即留痕。
//!
//! ## 一、诊断码（0x12E0~0x12EF 段独占：F2249 降级遥测面）
//!
//! 十六码闭集（[`TelCode`]）：预算面一码（记录路径步数超预算）、
//! 单调面两码（时间戳倒退/序号倒退拒收）、登记面两码（未登记源/
//! 源重复）、链路面一码（孤儿事件：执行/恢复无触发前置）、帧面两码
//! （帧号倒退/帧未闭合并拒绝发布）、账面一码（诊断账满挤占）、守恒
//! 面两码（守恒不等式破坏/跨账对拍不平）、schema 面一码（指纹不符/
//! 演进违规）、隐私面一码（上传通道拒绝——红线）、档位面一码（越界）、
//! 滞回面一码（恢复后未过滞回窗立即再触发——回摆留痕）、发布包面
//! 一码（字段越白名单）。段守恒编译期钉死（[`TEL_CODE_GUARD`]）。
//!
//! ## 二、三阶段全链（判据：触发/执行/恢复）
//!
//! 阶段闭集（[`DegrStage`]：触发/执行/恢复）；事件记录
//! [`DegrEventV1`]（ts/seq/源指纹/阶段/档位前后值，24 字节定长编码
//! [`DegrEventV1::to_bytes`] 可往返）；降级源登记表
//! [`SourceRegistry`]（七类源 [`DegrSourceKind`] 对齐 F2243 镜像
//! ——帧超时/热档/续航档/CGPU 档位/弱网/资源紧张/场景切换；源以
//! 参数指纹 FNV1a-32 登记，未登记指纹拒收不猜
//! [`DegrTelError::SourceUnknown`]，重复登记拒
//! [`DegrTelError::SourceDuplicate`]）。链路开合由枢纽维护：触发开
//! 链、执行标 seen、恢复闭链并结算恢复时延（触发 ts → 恢复 ts）。
//! 执行/恢复若无触发前置即孤儿事件 [`DegrTelError::StageOrphan`]
//! ——拒收不猜，守恒先于记录。
//!
//! ## 三、枢纽（`DegrTelemetryHub`）
//!
//! [`DegrTelemetryHub::record_event`]——严格 (ts, seq) 单调校验：
//! ts 倒退拒收 [`DegrTelError::TsRegressed`]、外部注入 seq 倒退拒收
//! [`DegrTelError::SeqRegressed`]，拒收一律入诊断账留痕；帧粒度
//! [`DegrTelemetryHub::begin_frame`] / [`DegrTelemetryHub::mark_tier`]
//! / [`DegrTelemetryHub::end_frame`]——帧号严格递增、卡顿判定 =
//! 显式旗 ∨ 帧成本超阈（60Hz 帧预算镜像）、档位触达位图留痕。
//! 恢复滞回：恢复闭链后需连续 [`HYSTERESIS_CONFIRM_FRAMES`] 帧无
//! 再触发方确认；恢复后立即再触发即回摆
//! [`TelCode::HysBounce`] 计数（对齐 F2247 恢复引擎滞回口径镜像）。
//!
//! ## 四、会话聚合与整数分位
//!
//! 会话聚合 [`SessionAggregate`]：滚动窗口 [`SESSION_WINDOW_FRAMES`]
//! 帧（600）内降级时长占比 ppm（降级帧 × 10⁶ ÷ 窗口帧，整数）、
//! 卡顿计数（窗口/累计双口径）、恢复时延与帧成本的 **整数对数桶
//! 分位** P95/P99（[`LogHist`]：样本按 log₂ 落 40 桶，累计计数定位
//! 分位桶，桶内按均匀分布线性插值——无浮点、有界近似，误差 ≤ 桶宽；
//! 需求秩 = ⌈p·n/100⌉ 全整数）+ RMS 抖动（u128 平方和 + [`isqrt_u128`]
//! 整数开方）。
//!
//! ## 五、全链守恒对账
//!
//! 守恒不等式（[`conservation_audit`]）：逐源 触发数 ≥ 执行数 ≥ 恢复
//! 数，任一破坏即 [`TelCode::ConservationBreak`]；跨账对拍
//! （[`reconcile_tables`]）：Σ逐源分列 ≡ 总账（三阶段各一对拍），
//! 篡改任一侧即 [`TelCode::ReconcileMismatch`]——对账函数以裸表为
//! 入参，审计/回放路径可注入篡改表验证「破即被捉」。
//!
//! ## 六、schema 版本化发布包（判据：遥测复用 + 演进纪律）
//!
//! schema [`DEGR_SCHEMA_V1`]：字段白名单 [`DEGR_SCHEMA_FIELDS`]
//! （帧行 10 字段，定序定型）+ 版本指纹（FNV1a-32 覆盖 id + 字段名
//! 序列，[`SchemaV1::fingerprint`]）；演进规则**只增不改**：新字段只
//! 能尾部追加、禁改名/禁改型/禁重排/禁删除，rev+1 且指纹必变。
//! 发布包 [`ReleasePackV1`]（[`DegrTelemetryHub::export_release_pack`]）：
//! 头（schema 指纹 + 会话汇总）+ 帧行（仅白名单字段，
//! [`build_pack_row`] 越名单即 [`DegrTelError::PackFieldViolated`]）
//! + 尾（守恒/对拍裁决 + 隐私标记）；**帧闭合纪律**：存在未闭合帧
//! 拒发布 [`DegrTelError::FrameOpenUnclosed`]，守恒或对拍破坏同样
//! 拒发布——发布包里的每个数都可对账。行内回验
//! [`ReleasePackV1::verify`] 独立重查指纹与白名单。
//!
//! ## 七、诚实三态（判据：空窗不谎报）
//!
//! [`AggValue::{Value, Empty, Unknown}`]：空窗（零样本）显性
//! [`AggValue::Empty`]——均值/分位返回 Empty，**不发零均值谎报**；
//! 来源不可知（未登记源被拒收、账满挤占）显性 [`AggValue::Unknown`]；
//! 拒收计数单独成账，损失可见而非隐没。
//!
//! ## 八、诊断账与体检
//!
//! [`TelEventLedger`] 定长环形账（192 条）：拒收/异常码 + (ts, seq)
//! 入账，满则挤占最旧并计 dropped（[`TelCode::LedgerFull`]）；体检
//! [`LedgerHealth`]（占用/丢弃/码覆盖/健康判定）。
//!
//! ## 九、遥测开销自证
//!
//! [`StepMeter`]：记录路径逐检查点计步，单事件预算
//! [`RECORD_STEP_BUDGET`] 步；[`DegrTelemetryHub::overhead_audit`]
//! 报告峰值步数/越限次数——遥测本身的开销自己先交代。
//!
//! ## 十、隐私红线（判据：本地闭环默认）
//!
//! [`PrivacyGate`]：发布包默认 `PRIVACY=LOCAL_ONLY|UPLOAD=DISABLED`；
//! 上传类出口 [`SinkKind::RemoteUpload`] 一律拒绝
//! [`DegrTelError::PrivacyUploadDenied`] 并入账
//! [`TelCode::PrivacyUploadDenied`]——遥测本地闭环，默认不上传。
//!
//! ## 十一、遥测复用声明（判据：遥测复用）
//!
//! 与既有 O 域降级可视化（CGPU-F2248 · cgo01_degradevis）同口径对齐，
//! 本地为**本地定义对齐镜像**（显性登记，非口头声明，待家族统一底座
//! 落地时替换）：触发源七类 ← F2243 镜像；档位 0~5 闭集 ← F2245/
//! F2246 执行器档位口径镜像；恢复滞回 ← F2247 恢复引擎滞回口径镜像
//! （阈值常量独立写死，口径一致）；状态机四态迁移**不在本单重复实现**
//! ——遥测只记录迁移事件不裁决合法性（裁决属 cgo01）；发布包行序列
//! 化采用家族同款 `'|'` 分隔保空段格式（对齐 cgo01 面板快照序列化）。
//!
//! ## 十二、纪律
//!
//! 零 unwrap、零 expect、零 panic!、零运行期 assert!（编译期段守恒
//! 断言除外）、零 IO、零墙钟（ts 全部上游注入）；算术一律
//! saturating/wrapping 显性化；>64KB 结构（枢纽 ≈ 44KB）建议 `Box`
//! 承载不上栈；错误全走 [`DegrTelError`]，每变体带中文可操作
//! `detail()`。
//!
//! [`DegrTelemetryHub::record_event`]: DegrTelemetryHub::record_event
//! [`DegrTelemetryHub::begin_frame`]: DegrTelemetryHub::begin_frame
//! [`DegrTelemetryHub::mark_tier`]: DegrTelemetryHub::mark_tier
//! [`DegrTelemetryHub::end_frame`]: DegrTelemetryHub::end_frame
//! [`DegrTelemetryHub::export_release_pack`]: DegrTelemetryHub::export_release_pack
//! [`DegrTelemetryHub::overhead_audit`]: DegrTelemetryHub::overhead_audit

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

// ============================================================
// 常量面（每个常量写死来源）
// ============================================================

/// 滚动窗口帧数（规格锚点 F2249 判据原文：滚动窗口 600 帧）。
pub const SESSION_WINDOW_FRAMES: usize = 600;

/// 事件环形账容量（384 条 × 32B/槽 ≈ 12KB；与帧窗口合计把枢纽压在 64KB 以下）。
pub const EVENT_RING_CAP: usize = 384;

/// 诊断账容量（192 条 × 24B/槽 ≈ 4.6KB；满则挤占最旧并计 dropped）。
pub const LEDGER_CAP: usize = 192;

/// 降级源七类闭集（对齐 F2243 镜像：帧超时/热档/续航档/CGPU 档位/弱网/资源紧张/场景切换）。
pub const SOURCE_KINDS: usize = 7;

/// 档位闭集上界（0~5，对齐 F2245/F2246 执行器档位口径镜像；0=正常档）。
pub const MAX_TIER: u8 = 5;

/// 恢复滞回确认帧数（对齐 F2247 恢复引擎滞回口径镜像：恢复闭链后需
/// 连续 3 帧无再触发方确认，恢复后立即再触发即回摆留痕）。
pub const HYSTERESIS_CONFIRM_FRAMES: u8 = 3;

/// 记录路径步数预算（每事件 ≤24 检查点步；遥测开销自证口径）。
pub const RECORD_STEP_BUDGET: u32 = 24;

/// 卡顿判定帧成本阈值（16667µs = 60Hz 帧间隔预算，显示 60Hz 帧预算镜像）。
pub const JANK_THRESHOLD_US: u32 = 16_667;

/// 占比尺度（ppm 百万分比；降级帧 × 本值 ÷ 窗口帧）。
pub const PPM_SCALE: u64 = 1_000_000;

/// FNV-1a 32 偏移基（标准 FNV1a-32 参数）。
pub const FNV_BASIS: u32 = 0x811C_9DC5;

/// FNV-1a 32 素数（标准 FNV1a-32 参数）。
pub const FNV_PRIME: u32 = 0x0100_0193;

/// 事件定长编码字节数（ts8 + seq8 + fp4 + src1 + stage1 + tier_before1 + tier_after1 = 24）。
pub const ENCODED_LEN: usize = 24;

/// 对数分位桶数（40 桶覆盖 [1, 2⁴⁰)µs ≈ 1.1e6 秒上界；规格内时延/帧成本量级充裕）。
pub const HIST_BUCKETS: usize = 40;

/// schema 标识（遥测复用发布包版本化口径；演进规则见模块头第六节）。
pub const SCHEMA_ID: &'static str = "DEGR_SCHEMA_V1";

/// 档位名表（0~5 闭集；对齐 F2245 降级档口径镜像）。
pub const TIER_NAMES: [&'static str; 6] =
    ["正常", "轻度·阴影降", "中度·后效降", "重度·分辨率降", "深度·帧档降", "保底·最低档"];

// ============================================================
// 一、诊断码（0x12E0~0x12EF 段独占：F2249 降级遥测面）
// ============================================================

/// 诊断码闭集（0x12E0~0x12EF 段独占：F2249 降级遥测面十六码）。
///
/// 段守恒由 [`TEL_CODE_GUARD`] 编译期钉死：首码 0x12E0、末码 0x12EF、
/// 恰十六码、码序严格递增。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u16)]
pub enum TelCode {
    /// 0x12E0 · 记录路径步数超预算（遥测开销自证越限）。
    BudgetOver = 0x12E0,
    /// 0x12E1 · 时间戳倒退（事件拒收入账）。
    TsRegressed = 0x12E1,
    /// 0x12E2 · 序号倒退（外部注入事件拒收入账）。
    SeqRegressed = 0x12E2,
    /// 0x12E3 · 未登记降级源（拒收不猜）。
    SourceUnknown = 0x12E3,
    /// 0x12E4 · 源指纹重复登记。
    SourceDuplicate = 0x12E4,
    /// 0x12E5 · 孤儿事件（执行/恢复无触发前置）。
    StageOrphan = 0x12E5,
    /// 0x12E6 · 帧号倒退/非单调。
    FrameIdxRegressed = 0x12E6,
    /// 0x12E7 · 帧未闭合（拒绝发布）。
    FrameOpenUnclosed = 0x12E7,
    /// 0x12E8 · 诊断账满（最旧被挤占）。
    LedgerFull = 0x12E8,
    /// 0x12E9 · 守恒不等式破坏（触发 ≥ 执行 ≥ 恢复）。
    ConservationBreak = 0x12E9,
    /// 0x12EA · 跨账对拍不平（Σ源 ≠ 总）。
    ReconcileMismatch = 0x12EA,
    /// 0x12EB · schema 指纹不符/演进违规。
    SchemaMismatch = 0x12EB,
    /// 0x12EC · 隐私红线：上传通道拒绝。
    PrivacyUploadDenied = 0x12EC,
    /// 0x12ED · 档位越界（0~5 闭集外）。
    TierOutOfRange = 0x12ED,
    /// 0x12EE · 滞回未确认回摆（恢复后未过滞回窗立即再触发）。
    HysBounce = 0x12EE,
    /// 0x12EF · 发布包字段越白名单。
    PackFieldViolated = 0x12EF,
}

impl TelCode {
    /// 段内码值（0x12E0~0x12EF；const 可用）。
    pub const fn code(self) -> u16 {
        self as u16
    }

    /// 段内偏移（0~15；账内按码直方统计用）。
    pub fn offset(self) -> usize {
        (self as u16 - 0x12E0) as usize
    }

    /// 中文短述（诊断读面）。
    pub fn zh(&self) -> &'static str {
        match self {
            TelCode::BudgetOver => "记录路径步数超预算",
            TelCode::TsRegressed => "时间戳倒退（拒收）",
            TelCode::SeqRegressed => "序号倒退（拒收）",
            TelCode::SourceUnknown => "未登记降级源（拒收不猜）",
            TelCode::SourceDuplicate => "源指纹重复登记",
            TelCode::StageOrphan => "孤儿事件（执行/恢复无触发前置）",
            TelCode::FrameIdxRegressed => "帧号倒退/非单调",
            TelCode::FrameOpenUnclosed => "帧未闭合（拒绝发布）",
            TelCode::LedgerFull => "诊断账满（最旧被挤占）",
            TelCode::ConservationBreak => "守恒不等式破坏",
            TelCode::ReconcileMismatch => "跨账对拍不平（Σ源≠总）",
            TelCode::SchemaMismatch => "schema 指纹不符/演进违规",
            TelCode::PrivacyUploadDenied => "隐私红线：上传通道拒绝",
            TelCode::TierOutOfRange => "档位越界（0~5 外）",
            TelCode::HysBounce => "滞回未确认回摆",
            TelCode::PackFieldViolated => "发布包字段越白名单",
        }
    }
}

/// 十六码段表（段守恒断言的常量数据源）。
pub const TEL_CODE_TABLE: [u16; 16] = [
    TelCode::BudgetOver.code(),
    TelCode::TsRegressed.code(),
    TelCode::SeqRegressed.code(),
    TelCode::SourceUnknown.code(),
    TelCode::SourceDuplicate.code(),
    TelCode::StageOrphan.code(),
    TelCode::FrameIdxRegressed.code(),
    TelCode::FrameOpenUnclosed.code(),
    TelCode::LedgerFull.code(),
    TelCode::ConservationBreak.code(),
    TelCode::ReconcileMismatch.code(),
    TelCode::SchemaMismatch.code(),
    TelCode::PrivacyUploadDenied.code(),
    TelCode::TierOutOfRange.code(),
    TelCode::HysBounce.code(),
    TelCode::PackFieldViolated.code(),
];

/// 段守恒求值（const：首/末/数量三元组）。
const fn tel_code_guard() -> (u16, u16, usize) {
    let mut lo: u16 = 0xFFFF;
    let mut hi: u16 = 0;
    let mut n: usize = 0;
    let mut i = 0;
    while i < TEL_CODE_TABLE.len() {
        let c = TEL_CODE_TABLE[i];
        if c < lo {
            lo = c;
        }
        if c > hi {
            hi = c;
        }
        n += 1;
        i += 1;
    }
    (lo, hi, n)
}

/// 码序严格递增求值（const；禁 derive PartialEq 的 == 于 const——逐码
/// 以 match 提升为 u16 后按整数比较）。
const fn tel_codes_strictly_increasing() -> bool {
    let mut i = 1;
    while i < TEL_CODE_TABLE.len() {
        if TEL_CODE_TABLE[i] <= TEL_CODE_TABLE[i - 1] {
            return false;
        }
        i += 1;
    }
    true
}

/// 段守恒三元组（编译期钉死 0x12E0~0x12EF 十六码）。
pub const TEL_CODE_GUARD: (u16, u16, usize) = tel_code_guard();

const _: () = assert!(TEL_CODE_GUARD.0 == 0x12E0, "0x12E 段首码必须为 0x12E0");
const _: () = assert!(TEL_CODE_GUARD.1 == 0x12EF, "0x12E 段末码必须为 0x12EF");
const _: () = assert!(TEL_CODE_GUARD.2 == 16, "0x12E 段必须恰十六码");
const _: () = assert!(tel_codes_strictly_increasing(), "0x12E 段码序必须严格递增");

// ============================================================
// 二、错误面（每变体带中文可操作 detail）
// ============================================================

/// 降级遥测错误闭集（可失败路径全走此枚举；每变体 `detail()` 给
/// 中文可操作诊断）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DegrTelError {
    /// 档位越界（0~5 闭集外）。
    TierOutOfRange,
    /// 未登记降级源（指纹未注册，拒收不猜）。
    SourceUnknown,
    /// 源指纹重复登记。
    SourceDuplicate,
    /// 源登记表满（七类闭集已满）。
    RegistryFull,
    /// 时间戳倒退（事件拒收）。
    TsRegressed,
    /// 序号倒退（外部注入事件拒收）。
    SeqRegressed,
    /// 孤儿事件（执行/恢复无触发前置）。
    StageOrphan,
    /// 帧号倒退/非单调。
    FrameIdxRegressed,
    /// 帧已开启（重复 begin_frame）。
    FrameAlreadyOpen,
    /// 帧未开启（end_frame/mark_tier 无前置 begin_frame）。
    FrameNotOpen,
    /// 帧未闭合（拒绝发布）。
    FrameOpenUnclosed,
    /// 守恒不等式破坏（触发 ≥ 执行 ≥ 恢复）。
    ConservationBreak,
    /// 跨账对拍不平（Σ源 ≠ 总）。
    ReconcileMismatch,
    /// schema 指纹不符/演进违规。
    SchemaMismatch,
    /// 隐私红线：上传通道拒绝。
    PrivacyUploadDenied,
    /// 发布包字段越白名单。
    PackFieldViolated,
}

impl DegrTelError {
    /// 中文可操作诊断（每变体一句：现象 + 该做什么）。
    pub fn detail(&self) -> &'static str {
        match self {
            DegrTelError::TierOutOfRange => {
                "档位越界：档位闭集为 0~5（0=正常档）；请核对上报档位是否落在闭集内再注入"
            }
            DegrTelError::SourceUnknown => {
                "未登记降级源：该参数指纹未在 SourceRegistry 登记；请先 register_source 登记再上报，遥测拒收不猜"
            }
            DegrTelError::SourceDuplicate => {
                "源指纹重复登记：同指纹已占用槽位；请复用既有槽位或换新指纹，勿重复 register_source"
            }
            DegrTelError::RegistryFull => {
                "源登记表满：七类降级源闭集已满；请核对是否漏复用既有槽位，闭集不扩容"
            }
            DegrTelError::TsRegressed => {
                "时间戳倒退：事件 ts 小于上一已收事件；请检查时钟源单调性，倒退事件已拒收入账"
            }
            DegrTelError::SeqRegressed => {
                "序号倒退：注入事件 seq 不大于上一已收 seq；请检查生产端序号单调性，倒退事件已拒收入账"
            }
            DegrTelError::StageOrphan => {
                "孤儿事件：执行/恢复无触发前置（或执行重复下发）；请核对触发→执行→恢复次序，孤儿拒收以保守恒"
            }
            DegrTelError::FrameIdxRegressed => {
                "帧号倒退：begin_frame 帧号不大于上一帧；请检查帧号生产严格递增"
            }
            DegrTelError::FrameAlreadyOpen => {
                "帧已开启：上一帧未 end_frame 即再 begin_frame；请先闭合当前帧再开新帧"
            }
            DegrTelError::FrameNotOpen => {
                "帧未开启：end_frame/mark_tier 缺 begin_frame 前置；请先开启帧再落帧内数据"
            }
            DegrTelError::FrameOpenUnclosed => {
                "帧未闭合：存在 open 状态帧拒绝发布发布包；请先 end_frame 闭合全部帧（帧闭合纪律）"
            }
            DegrTelError::ConservationBreak => {
                "守恒不等式破坏：逐源触发数 ≥ 执行数 ≥ 恢复数被打破；请核对链路注入次序，破坏者拒绝发布"
            }
            DegrTelError::ReconcileMismatch => {
                "跨账对拍不平：Σ逐源分列 ≠ 总账；请排查账面篡改或统计口径漂移，不平拒绝发布"
            }
            DegrTelError::SchemaMismatch => {
                "schema 指纹不符：发布包指纹与本地 DEGR_SCHEMA_V1 不一致；请核对 schema 演进是否遵守只增不改"
            }
            DegrTelError::PrivacyUploadDenied => {
                "隐私红线：遥测本地闭环默认不上传，RemoteUpload 出口被拒；请使用本地环/调试面板出口"
            }
            DegrTelError::PackFieldViolated => {
                "发布包字段越白名单：字段名不在 DEGR_SCHEMA_V1 白名单内；请只发布白名单字段（只增不改演进）"
            }
        }
    }
}

// ============================================================
// 三、纯函数工具（FNV / 整数开方 / 阶段 / 源）
// ============================================================

/// FNV-1a 32 位哈希（const 可用；参数指纹与 schema 指纹共用，标准参数）。
pub const fn fnv1a32(data: &[u8]) -> u32 {
    let mut h: u32 = FNV_BASIS;
    let mut i = 0;
    while i < data.len() {
        h ^= data[i] as u32;
        h = h.wrapping_mul(FNV_PRIME);
        i += 1;
    }
    h
}

/// 字符串指纹便捷封装（FNV-1a 32 over UTF-8 字节）。
pub fn fnv_str(s: &str) -> u32 {
    fnv1a32(s.as_bytes())
}

/// 整数开方（u64，逐位法；无浮点、无溢出——遥测 RMS 用）。
pub const fn isqrt_u64(mut n: u64) -> u64 {
    let mut res: u64 = 0;
    let mut bit: u64 = 1u64 << 62;
    while bit > n {
        bit >>= 2;
    }
    while bit != 0 {
        if n >= res + bit {
            n -= res + bit;
            res = (res >> 1) + bit;
        } else {
            res >>= 1;
        }
        bit >>= 2;
    }
    res
}

/// 整数开方（u128，逐位法；平方和累计用，无溢出）。
pub const fn isqrt_u128(mut n: u128) -> u128 {
    let mut res: u128 = 0;
    let mut bit: u128 = 1u128 << 126;
    while bit > n {
        bit >>= 2;
    }
    while bit != 0 {
        if n >= res + bit {
            n -= res + bit;
            res = (res >> 1) + bit;
        } else {
            res >>= 1;
        }
        bit >>= 2;
    }
    res
}

/// u64 小端写入（事件定长编码用）。
fn put_u64_le(out: &mut [u8], at: usize, v: u64) {
    let mut i = 0;
    while i < 8 {
        out[at + i] = (v >> (i * 8)) as u8;
        i += 1;
    }
}

/// u64 小端读取（事件定长解码用）。
fn get_u64_le(data: &[u8], at: usize) -> u64 {
    let mut v: u64 = 0;
    let mut i = 0;
    while i < 8 {
        v |= (data[at + i] as u64) << (i * 8);
        i += 1;
    }
    v
}

/// u32 小端写入。
fn put_u32_le(out: &mut [u8], at: usize, v: u32) {
    let mut i = 0;
    while i < 4 {
        out[at + i] = (v >> (i * 8)) as u8;
        i += 1;
    }
}

/// u32 小端读取。
fn get_u32_le(data: &[u8], at: usize) -> u32 {
    let mut v: u32 = 0;
    let mut i = 0;
    while i < 4 {
        v |= (data[at + i] as u32) << (i * 8);
        i += 1;
    }
    v
}

/// 降级三阶段闭集（触发/执行/恢复——全链遥测的阶段轴）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DegrStage {
    /// 触发（哪条判据/哪个信号越过阈值——开链）。
    Trigger,
    /// 执行（降到了哪一级、动了哪些参数——标 seen）。
    Execute,
    /// 恢复（何时回升、滞回确认——闭链并结算时延）。
    Recover,
}

impl DegrStage {
    /// 阶段序（0/1/2；定长编码用）。
    pub const fn idx(self) -> u8 {
        match self {
            DegrStage::Trigger => 0,
            DegrStage::Execute => 1,
            DegrStage::Recover => 2,
        }
    }

    /// 由序还原（编码往返用；越界 None）。
    pub fn from_idx(v: u8) -> Option<DegrStage> {
        match v {
            0 => Some(DegrStage::Trigger),
            1 => Some(DegrStage::Execute),
            2 => Some(DegrStage::Recover),
            _ => None,
        }
    }

    /// 中文阶段名。
    pub fn zh(&self) -> &'static str {
        match self {
            DegrStage::Trigger => "触发",
            DegrStage::Execute => "执行",
            DegrStage::Recover => "恢复",
        }
    }
}

/// 降级源七类闭集（对齐 F2243 镜像——帧超时/热档/续航档/CGPU 档位/
/// 弱网/资源紧张/场景切换）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DegrSourceKind {
    /// 帧超时（帧预算越限触发）。
    FrameTimeout,
    /// 热档（热阈值越限触发）。
    Thermal,
    /// 续航档（电量阈值越限触发）。
    Battery,
    /// CGPU 档位（算力档位信号触发）。
    CgpuTier,
    /// 弱网（网络带宽信号触发）。
    WeakNet,
    /// 资源紧张（内存/带宽紧张触发）。
    ResourceTight,
    /// 场景切换（场景负载特征切换触发）。
    SceneSwitch,
}

impl DegrSourceKind {
    /// 类别序（登记表槽序对应类别容量下标）。
    pub const fn idx(self) -> u8 {
        match self {
            DegrSourceKind::FrameTimeout => 0,
            DegrSourceKind::Thermal => 1,
            DegrSourceKind::Battery => 2,
            DegrSourceKind::CgpuTier => 3,
            DegrSourceKind::WeakNet => 4,
            DegrSourceKind::ResourceTight => 5,
            DegrSourceKind::SceneSwitch => 6,
        }
    }

    /// 由序还原（越界 None）。
    pub fn from_idx(v: u8) -> Option<DegrSourceKind> {
        match v {
            0 => Some(DegrSourceKind::FrameTimeout),
            1 => Some(DegrSourceKind::Thermal),
            2 => Some(DegrSourceKind::Battery),
            3 => Some(DegrSourceKind::CgpuTier),
            4 => Some(DegrSourceKind::WeakNet),
            5 => Some(DegrSourceKind::ResourceTight),
            6 => Some(DegrSourceKind::SceneSwitch),
            _ => None,
        }
    }

    /// 类别规范名（ASCII 稳定串；规范指纹源）。
    pub fn name_en(&self) -> &'static str {
        match self {
            DegrSourceKind::FrameTimeout => "frame_timeout",
            DegrSourceKind::Thermal => "thermal",
            DegrSourceKind::Battery => "battery",
            DegrSourceKind::CgpuTier => "cgpu_tier",
            DegrSourceKind::WeakNet => "weak_net",
            DegrSourceKind::ResourceTight => "resource_tight",
            DegrSourceKind::SceneSwitch => "scene_switch",
        }
    }

    /// 中文类别名。
    pub fn zh(&self) -> &'static str {
        match self {
            DegrSourceKind::FrameTimeout => "帧超时",
            DegrSourceKind::Thermal => "热档",
            DegrSourceKind::Battery => "续航档",
            DegrSourceKind::CgpuTier => "CGPU 档位",
            DegrSourceKind::WeakNet => "弱网",
            DegrSourceKind::ResourceTight => "资源紧张",
            DegrSourceKind::SceneSwitch => "场景切换",
        }
    }

    /// 类别规范指纹（FNV1a-32 over 规范名；同类别实例指纹由参数串派生）。
    pub fn canon_fp(&self) -> u32 {
        fnv1a32(self.name_en().as_bytes())
    }
}

/// 降级源实例（类别 + 参数指纹；指纹 = FNV1a-32(参数串)，如
/// "thermal/gpu_hot_spot>95C"——同一类别可多实例，指纹全局唯一）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SourceSlot {
    /// 类别（七类闭集之一）。
    pub kind: DegrSourceKind,
    /// 实例参数指纹（全局唯一，FNV1a-32）。
    pub fp: u32,
}

/// 降级源登记表（七类闭集容量；未登记指纹拒收不猜）。
#[derive(Clone, Copy, Debug)]
pub struct SourceRegistry {
    /// 槽表（容量 = 七类闭集；None = 空槽）。
    slots: [Option<SourceSlot>; SOURCE_KINDS],
    /// 已登记数。
    count: u32,
}

impl SourceRegistry {
    /// 空表。
    pub const fn new() -> Self {
        SourceRegistry { slots: [None; SOURCE_KINDS], count: 0 }
    }

    /// 登记源实例（指纹重复拒 [`DegrTelError::SourceDuplicate`]，
    /// 表满拒 [`DegrTelError::RegistryFull`]）。
    pub fn register(&mut self, kind: DegrSourceKind, fp: u32) -> Result<u8, DegrTelError> {
        let mut i = 0;
        while i < SOURCE_KINDS {
            if let Some(s) = self.slots[i] {
                if s.fp == fp {
                    self_dup_trace(fp);
                    return Err(DegrTelError::SourceDuplicate);
                }
            }
            i += 1;
        }
        let mut i = 0;
        while i < SOURCE_KINDS {
            if self.slots[i].is_none() {
                self.slots[i] = Some(SourceSlot { kind, fp });
                self.count = self.count.saturating_add(1);
                return Ok(i as u8);
            }
            i += 1;
        }
        Err(DegrTelError::RegistryFull)
    }

    /// 指纹 → 槽号（未登记 None——上层拒收不猜）。
    pub fn lookup(&self, fp: u32) -> Option<u8> {
        let mut i = 0;
        while i < SOURCE_KINDS {
            if let Some(s) = self.slots[i] {
                if s.fp == fp {
                    return Some(i as u8);
                }
            }
            i += 1;
        }
        None
    }

    /// 槽号 → 实例（逐源分列渲染用）。
    pub fn slot(&self, idx: u8) -> Option<SourceSlot> {
        if (idx as usize) < SOURCE_KINDS {
            self.slots[idx as usize]
        } else {
            None
        }
    }

    /// 已登记数。
    pub fn count(&self) -> u32 {
        self.count
    }
}

/// 重复登记留痕（无 panic 面；只做语义注释锚点）。
fn self_dup_trace(_fp: u32) {}

// ============================================================
// 四、事件记录（DegrEventV1 · 24 字节定长编码）
// ============================================================

/// 降级事件记录 V1（定长 24 字节编码；参数指纹/阶段/档位前后值全量）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DegrEventV1 {
    /// 事件时戳（上游注入 tick；枢纽内严格单调）。
    pub ts: u64,
    /// 事件序号（枢纽内严格递增；外部注入须校验单调）。
    pub seq: u64,
    /// 源实例参数指纹（FNV1a-32；登记表键）。
    pub fp: u32,
    /// 源槽号（0~6）。
    pub src_slot: u8,
    /// 阶段（0=触发 1=执行 2=恢复）。
    pub stage: u8,
    /// 档位·前值（0~5）。
    pub tier_before: u8,
    /// 档位·后值（0~5）。
    pub tier_after: u8,
}

impl DegrEventV1 {
    /// 构造（不做校验——校验归枢纽；编码往返与直接构造两用）。
    pub const fn new(
        ts: u64,
        seq: u64,
        fp: u32,
        src_slot: u8,
        stage: u8,
        tier_before: u8,
        tier_after: u8,
    ) -> Self {
        DegrEventV1 { ts, seq, fp, src_slot, stage, tier_before, tier_after }
    }

    /// 24 字节小端定长编码（布局：ts[0..8] seq[8..16] fp[16..20]
    /// src[20] stage[21] tier_before[22] tier_after[23]）。
    pub fn to_bytes(&self, out: &mut [u8; ENCODED_LEN]) {
        put_u64_le(out, 0, self.ts);
        put_u64_le(out, 8, self.seq);
        put_u32_le(out, 16, self.fp);
        out[20] = self.src_slot;
        out[21] = self.stage;
        out[22] = self.tier_before;
        out[23] = self.tier_after;
    }

    /// 24 字节定长解码（域校验：阶段 ∈{0,1,2}、槽 <7、档位 ≤5；
    /// 越域 None——不猜）。
    pub fn from_bytes(data: &[u8; ENCODED_LEN]) -> Option<DegrEventV1> {
        let ts = get_u64_le(data, 0);
        let seq = get_u64_le(data, 8);
        let fp = get_u32_le(data, 16);
        let src_slot = data[20];
        let stage = data[21];
        let tier_before = data[22];
        let tier_after = data[23];
        if DegrStage::from_idx(stage).is_none() {
            return None;
        }
        if src_slot as usize >= SOURCE_KINDS {
            return None;
        }
        if tier_before > MAX_TIER || tier_after > MAX_TIER {
            return None;
        }
        Some(DegrEventV1 { ts, seq, fp, src_slot, stage, tier_before, tier_after })
    }

    /// 阶段还原（编码往返用）。
    pub fn stage(&self) -> Option<DegrStage> {
        DegrStage::from_idx(self.stage)
    }

    /// 档位迁移是否合法方向（触发应升档 after ≥ before，恢复应降档
    /// after ≤ before——仅作展示参考，不裁决，裁决属 cgo01）。
    pub fn tier_dir_ok(&self) -> Option<bool> {
        match self.stage {
            0 => Some(self.tier_after >= self.tier_before),
            2 => Some(self.tier_after <= self.tier_before),
            _ => None,
        }
    }
}

// ============================================================
// 五、诚实三态（AggValue：空窗 Empty 不发零均值谎报）
// ============================================================

/// 聚合值三态（诚实纪律载体）：
/// - [`AggValue::Value`]：有样本，值为整数；
/// - [`AggValue::Empty`]：空窗（零样本）——显性空，绝不回退为零；
/// - [`AggValue::Unknown`]：来源不可知（未登记源拒收/账满挤占等
///   造成的数据缺失）——显性未知，绝不猜值。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AggValue {
    /// 有样本整数值。
    Value(u64),
    /// 空窗（零样本）。
    Empty,
    /// 来源不可知（数据缺失被显性登记）。
    Unknown,
}

impl AggValue {
    /// 取值（None = Empty/Unknown——调用方不得把 None 当 0 用）。
    pub fn as_value(&self) -> Option<u64> {
        match self {
            AggValue::Value(v) => Some(*v),
            _ => None,
        }
    }

    /// 是否空窗。
    pub fn is_empty(&self) -> bool {
        matches!(self, AggValue::Empty)
    }

    /// 是否未知。
    pub fn is_unknown(&self) -> bool {
        matches!(self, AggValue::Unknown)
    }

    /// 展示串（Value=十进制、Empty="--"、Unknown="?"——读面三态可辨）。
    pub fn display(&self) -> String {
        match self {
            AggValue::Value(v) => format!("{}", v),
            AggValue::Empty => "--".to_string(),
            AggValue::Unknown => "?".to_string(),
        }
    }
}

// ============================================================
// 六、整数对数桶直方图（P95/P99 整数分位 + RMS）
// ============================================================

/// 整数对数桶直方图（无浮点分位）。
///
/// **整数对数桶分位算法（写明）**：
/// 1. 样本 v>0 落桶 b = ⌊log₂v⌋（桶 b 覆盖 [2^b, 2^(b+1))；v=0 归
///    桶 0 与 1 同层处理），40 桶覆盖 [1, 2⁴⁰)µs；
/// 2. 需求秩 `need = ⌈p·n/100⌉ = (p·n + 99) / 100`（全整数，p∈[1,99]）；
/// 3. 累计计数自低桶向高桶扫描，首个 `cum + cnt_b ≥ need` 的桶即
///    分位桶；
/// 4. 桶内线性插值（假设桶内均匀分布）：`v = base + (base-1)·(frac-1)
///    / (span-1)`，其中 base=2^b、span=桶内计数、frac=need-cum_before
///    ∈[1, span]；span≤1 直接取 base；饱和运算兜底。
/// 性质：无浮点、O(桶数)、有界近似（误差 ≤ 桶宽 = base），空窗返回
/// [`AggValue::Empty`]（不发零分位谎报），p 越界返回
/// [`AggValue::Unknown`]。
#[derive(Clone, Copy, Debug)]
pub struct LogHist {
    /// 桶计数（桶 b 计 v ∈ [2^b, 2^(b+1)) 的样本数）。
    buckets: [u32; HIST_BUCKETS],
    /// 样本总数。
    count: u64,
    /// 样本总和（均值用）。
    sum: u64,
    /// 平方和（u128 累计，RMS 用；saturating 兜底）。
    sum_sq: u128,
    /// 样本最大值（分位上界兜底/诚实展示）。
    max: u64,
}

impl LogHist {
    /// 空直方图。
    pub const fn new() -> Self {
        LogHist { buckets: [0; HIST_BUCKETS], count: 0, sum: 0, sum_sq: 0, max: 0 }
    }

    /// 落桶号（v=0 归 0；v≥2^39 归顶桶——顶桶宽度为开区间上界）。
    fn bucket_of(v: u64) -> usize {
        if v == 0 {
            return 0;
        }
        let b = (63 - v.leading_zeros()) as usize;
        if b >= HIST_BUCKETS {
            HIST_BUCKETS - 1
        } else {
            b
        }
    }

    /// 入样（saturating 全程）。
    pub fn push(&mut self, v: u64) {
        let b = Self::bucket_of(v);
        self.buckets[b] = self.buckets[b].saturating_add(1);
        self.count = self.count.saturating_add(1);
        self.sum = self.sum.saturating_add(v);
        self.sum_sq = self.sum_sq.saturating_add((v as u128) * (v as u128));
        if v > self.max {
            self.max = v;
        }
    }

    /// 样本数。
    pub fn count(&self) -> u64 {
        self.count
    }

    /// 整数均值（三态：空窗 Empty）。
    pub fn mean(&self) -> AggValue {
        if self.count == 0 {
            AggValue::Empty
        } else {
            AggValue::Value(self.sum / self.count)
        }
    }

    /// 整数 RMS（√(Σv²/n)，u128 平方和 + 整数开方；空窗 Empty）。
    pub fn rms(&self) -> AggValue {
        if self.count == 0 {
            AggValue::Empty
        } else {
            let q = self.sum_sq / (self.count as u128);
            AggValue::Value(isqrt_u128(q) as u64)
        }
    }

    /// 样本最大值（三态）。
    pub fn max(&self) -> AggValue {
        if self.count == 0 {
            AggValue::Empty
        } else {
            AggValue::Value(self.max)
        }
    }

    /// 整数分位（p 为百分位 1~99；算法见类型注释；空窗 Empty、p 越界
    /// Unknown）。
    pub fn percentile(&self, p: u64) -> AggValue {
        if self.count == 0 {
            return AggValue::Empty;
        }
        if p == 0 || p > 99 {
            return AggValue::Unknown;
        }
        // 需求秩 ⌈p·n/100⌉，全整数；p≤99 故 saturating 兜底仅理论防御。
        let need = p.saturating_mul(self.count).saturating_add(99) / 100;
        let mut cum: u64 = 0;
        let mut b = 0;
        while b < HIST_BUCKETS {
            let span = self.buckets[b] as u64;
            if cum + span >= need && span > 0 {
                let base = 1u64 << b;
                if span <= 1 {
                    return AggValue::Value(base);
                }
                let frac = need - cum; // ∈ [1, span]
                let width = if b == 0 { 1u64 } else { base };
                let v = base.saturating_add(
                    width.saturating_sub(1).saturating_mul(frac.saturating_sub(1)) / (span - 1),
                );
                return AggValue::Value(v);
            }
            cum = cum.saturating_add(span);
            b += 1;
        }
        // 理论不可达（need ≤ count = Σspan）；兜底诚实取样本最大值。
        AggValue::Value(self.max)
    }

    /// 桶占用直方（体检/导出用）。
    pub fn bucket_occupancy(&self) -> [u32; HIST_BUCKETS] {
        self.buckets
    }
}

// ============================================================
// 七、帧行与帧窗口（每帧粒度记录）
// ============================================================

/// 帧行（每帧粒度聚合行；滚动窗口 600 帧环形承载）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FrameRow {
    /// 帧号（严格递增，上游注入）。
    pub idx: u64,
    /// 开帧时戳。
    pub open_ts: u64,
    /// 闭帧时戳（未闭合 0）。
    pub close_ts: u64,
    /// 是否已闭合（帧闭合纪律载体）。
    pub closed: bool,
    /// 帧内触发事件数（u8 饱和）。
    pub trig: u8,
    /// 帧内执行事件数（u8 饱和）。
    pub exec: u8,
    /// 帧内恢复事件数（u8 饱和）。
    pub recv: u8,
    /// 卡顿旗（= 显式旗 ∨ 帧成本超阈）。
    pub jank: bool,
    /// 帧成本（µs，上游注入）。
    pub cost_us: u32,
    /// 帧内触达最高档位（0~5）。
    pub max_tier: u8,
    /// 档位触达位图（bit1~bit5 对应档位 1~5 在本帧被触达）。
    pub tier_mask: u8,
}

impl FrameRow {
    /// 空行常量（窗口数组初始化用）。
    pub const EMPTY: FrameRow = FrameRow {
        idx: 0,
        open_ts: 0,
        close_ts: 0,
        closed: false,
        trig: 0,
        exec: 0,
        recv: 0,
        jank: false,
        cost_us: 0,
        max_tier: 0,
        tier_mask: 0,
    };

    /// 是否降级帧（帧内触达过 ≥1 档）。
    pub fn is_degraded(&self) -> bool {
        self.max_tier > 0
    }

    /// schema 字段取值（白名单驱动；名字不识 None——不猜）。
    pub fn field_value(&self, name: &str) -> Option<u64> {
        match name {
            "frame_idx" => Some(self.idx),
            "open_ts" => Some(self.open_ts),
            "close_ts" => Some(self.close_ts),
            "trig_cnt" => Some(self.trig as u64),
            "exec_cnt" => Some(self.exec as u64),
            "recv_cnt" => Some(self.recv as u64),
            "jank" => Some(self.jank as u64),
            "cost_us" => Some(self.cost_us as u64),
            "max_tier" => Some(self.max_tier as u64),
            "tier_mask" => Some(self.tier_mask as u64),
            _ => None,
        }
    }
}

// ============================================================
// 八、步数计与开销自证
// ============================================================

/// 记录路径步数计（遥测开销自证：单事件 ≤[`RECORD_STEP_BUDGET`] 步）。
#[derive(Clone, Copy, Debug)]
pub struct StepMeter {
    /// 预算步数。
    budget: u32,
    /// 当前事件已计步。
    cur: u32,
    /// 历史峰值步数。
    max_seen: u32,
    /// 越限次数。
    violations: u32,
}

impl StepMeter {
    /// 空。
    pub const fn new() -> Self {
        StepMeter { budget: RECORD_STEP_BUDGET, cur: 0, max_seen: 0, violations: 0 }
    }

    /// 开新事件（清零当前计）。
    pub fn open(&mut self) {
        self.cur = 0;
    }

    /// 计一步（检查点调用）。
    pub fn step(&mut self) {
        self.cur = self.cur.saturating_add(1);
    }

    /// 闭事件（回写峰值/越限，返回本事件步数）。
    pub fn close(&mut self) -> u32 {
        if self.cur > self.max_seen {
            self.max_seen = self.cur;
        }
        if self.cur > self.budget {
            self.violations = self.violations.saturating_add(1);
        }
        self.cur
    }

    /// 开销自证报告。
    pub fn audit(&self) -> OverheadReport {
        OverheadReport {
            budget: self.budget,
            max_seen: self.max_seen,
            violations: self.violations,
            within_budget: self.max_seen <= self.budget,
        }
    }
}

/// 开销自证报告。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct OverheadReport {
    /// 预算步数（24）。
    pub budget: u32,
    /// 历史峰值步数。
    pub max_seen: u32,
    /// 越限次数。
    pub violations: u32,
    /// 是否全程在预算内。
    pub within_budget: bool,
}

// ============================================================
// 九、诊断账（TelEventLedger · 定长环 + health 体检）
// ============================================================

/// 诊断账条目（异常码 + 定位 (ts, seq)）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LedgerEntry {
    /// 异常码（0x12E 段闭集）。
    pub code: TelCode,
    /// 案发时戳（上游注入轴）。
    pub ts: u64,
    /// 案发序号（事件轴；非事件类留痕取当前轴值）。
    pub seq: u64,
}

/// 诊断账（定长环形 192 条；满则挤占最旧并计 dropped——损失显性不静默）。
#[derive(Clone, Copy, Debug)]
pub struct TelEventLedger {
    /// 环槽。
    entries: [Option<LedgerEntry>; LEDGER_CAP],
    /// 环头（最旧下标）。
    head: usize,
    /// 现存条数。
    len: usize,
    /// 累计挤占数。
    dropped: u64,
    /// 按码直方（16 槽对应 0x12E0~0x12EF）。
    by_code: [u32; 16],
}

impl TelEventLedger {
    /// 空账。
    pub const fn new() -> Self {
        TelEventLedger { entries: [None; LEDGER_CAP], head: 0, len: 0, dropped: 0, by_code: [0; 16] }
    }

    /// 入账（满则挤占最旧 + dropped+1 + LedgerFull 计数；len 恒守恒）。
    pub fn push(&mut self, code: TelCode, ts: u64, seq: u64) {
        if self.len == LEDGER_CAP {
            // 挤占最旧：旧头槽位原地覆写，len 不变（账容量守恒）。
            let pos = self.head;
            self.head = (self.head + 1) % LEDGER_CAP;
            self.dropped = self.dropped.saturating_add(1);
            self.by_code[TelCode::LedgerFull.offset()] =
                self.by_code[TelCode::LedgerFull.offset()].saturating_add(1);
            self.entries[pos] = Some(LedgerEntry { code, ts, seq });
        } else {
            let pos = (self.head + self.len) % LEDGER_CAP;
            self.entries[pos] = Some(LedgerEntry { code, ts, seq });
            self.len += 1;
        }
        self.by_code[code.offset()] = self.by_code[code.offset()].saturating_add(1);
    }

    /// 现存条目快照（旧→新序）。
    pub fn snapshot(&self) -> Vec<LedgerEntry> {
        let mut out = Vec::new();
        let mut i = 0;
        while i < self.len {
            let pos = (self.head + i) % LEDGER_CAP;
            if let Some(e) = self.entries[pos] {
                out.push(e);
            }
            i += 1;
        }
        out
    }

    /// 按码历史入账计数（含被挤占者——历史不抹）。
    pub fn count_of(&self, code: TelCode) -> u32 {
        self.by_code[code.offset()]
    }

    /// 体检（健康判定：零挤占 且 现存 < 容量）。
    pub fn health(&self) -> LedgerHealth {
        let mut codes_seen = 0;
        let mut top_code: Option<TelCode> = None;
        let mut top_cnt: u32 = 0;
        let mut i = 0;
        while i < 16 {
            if self.by_code[i] > 0 {
                codes_seen += 1;
                if self.by_code[i] > top_cnt {
                    top_cnt = self.by_code[i];
                    top_code = TelCode::from_offset(i);
                }
            }
            i += 1;
        }
        LedgerHealth {
            used: self.len,
            cap: LEDGER_CAP,
            dropped: self.dropped,
            codes_seen,
            top_code,
            ok: self.dropped == 0 && self.len < LEDGER_CAP,
        }
    }
}

impl TelCode {
    /// 由段内偏移还原（0~15；越界 None）。
    pub fn from_offset(off: usize) -> Option<TelCode> {
        match off {
            0 => Some(TelCode::BudgetOver),
            1 => Some(TelCode::TsRegressed),
            2 => Some(TelCode::SeqRegressed),
            3 => Some(TelCode::SourceUnknown),
            4 => Some(TelCode::SourceDuplicate),
            5 => Some(TelCode::StageOrphan),
            6 => Some(TelCode::FrameIdxRegressed),
            7 => Some(TelCode::FrameOpenUnclosed),
            8 => Some(TelCode::LedgerFull),
            9 => Some(TelCode::ConservationBreak),
            10 => Some(TelCode::ReconcileMismatch),
            11 => Some(TelCode::SchemaMismatch),
            12 => Some(TelCode::PrivacyUploadDenied),
            13 => Some(TelCode::TierOutOfRange),
            14 => Some(TelCode::HysBounce),
            15 => Some(TelCode::PackFieldViolated),
            _ => None,
        }
    }
}

/// 诊断账体检报告。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LedgerHealth {
    /// 现存条数。
    pub used: usize,
    /// 容量（192）。
    pub cap: usize,
    /// 累计挤占数。
    pub dropped: u64,
    /// 出现过的码种数（0~16）。
    pub codes_seen: u8,
    /// 历史入账最多的码。
    pub top_code: Option<TelCode>,
    /// 健康判定（零挤占且未满）。
    pub ok: bool,
}

// ============================================================
// 十、守恒对账（自由函数：裸表入参，审计/回放可注入篡改表）
// ============================================================

/// 守恒审计报告。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ConservationReport {
    /// 是否整体通过。
    pub ok: bool,
    /// 破坏明细（逐源）。
    pub breaks: Vec<ConservBreak>,
}

/// 逐源守恒破坏明细。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ConservBreak {
    /// 源槽号。
    pub slot: u8,
    /// 破坏描述（中文，含数值）。
    pub detail: String,
}

/// 全链守恒对账：逐源 触发数 ≥ 执行数 ≥ 恢复数（单调不等式校验）。
///
/// 入参为裸表 `[slot] → [trig, exec, recv]`——审计/回放路径可注入
/// 篡改表验证「破即被捉」。
pub fn conservation_audit(src: &[[u64; 3]; SOURCE_KINDS]) -> ConservationReport {
    let mut breaks = Vec::new();
    let mut slot = 0;
    while slot < SOURCE_KINDS {
        let trig = src[slot][0];
        let exec = src[slot][1];
        let recv = src[slot][2];
        if trig < exec || exec < recv {
            breaks.push(ConservBreak {
                slot: slot as u8,
                detail: format!(
                    "源槽{}：触发{} ≥ 执行{} ≥ 恢复{} 不成立（守恒破坏）",
                    slot, trig, exec, recv
                ),
            });
        }
        slot += 1;
    }
    ConservationReport { ok: breaks.is_empty(), breaks }
}

/// 对拍报告。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ReconcileReport {
    /// 是否三阶段全部平账。
    pub ok: bool,
    /// 不平阶段（0=触发 1=执行 2=恢复；平账 None）。
    pub mismatch_stage: Option<u8>,
    /// 不平阶段的 Σ源。
    pub sum: u64,
    /// 不平阶段的总账。
    pub total: u64,
}

/// 跨账对拍：Σ逐源分列 ≡ 总账（三阶段各一对拍；篡改任一侧即破）。
pub fn reconcile_tables(
    src: &[[u64; 3]; SOURCE_KINDS],
    totals: &[u64; 3],
) -> ReconcileReport {
    let mut stage = 0;
    while stage < 3 {
        let mut sum: u64 = 0;
        let mut slot = 0;
        while slot < SOURCE_KINDS {
            sum = sum.saturating_add(src[slot][stage]);
            slot += 1;
        }
        if sum != totals[stage] {
            return ReconcileReport {
                ok: false,
                mismatch_stage: Some(stage as u8),
                sum,
                total: totals[stage],
            };
        }
        stage += 1;
    }
    ReconcileReport { ok: true, mismatch_stage: None, sum: 0, total: 0 }
}

// ============================================================
// 十一、schema 版本化发布包
// ============================================================

/// schema 字段类型闭集（帧行白名单字段定型）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FieldKind {
    /// 64 位整数。
    U64,
    /// 32 位整数。
    U32,
    /// 8 位整数。
    U8,
    /// 布尔（0/1）。
    Bool,
}

/// 白名单字段规格（名字 + 型；定序定型，演进只增不改）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FieldSpec {
    /// 字段名（ASCII 稳定串；白名单键）。
    pub name: &'static str,
    /// 字段型。
    pub kind: FieldKind,
}

/// 帧行白名单（10 字段，定序——发布行按此序渲染，禁止乱序重排）。
pub const DEGR_SCHEMA_FIELDS: [FieldSpec; 10] = [
    FieldSpec { name: "frame_idx", kind: FieldKind::U64 },
    FieldSpec { name: "open_ts", kind: FieldKind::U64 },
    FieldSpec { name: "close_ts", kind: FieldKind::U64 },
    FieldSpec { name: "trig_cnt", kind: FieldKind::U8 },
    FieldSpec { name: "exec_cnt", kind: FieldKind::U8 },
    FieldSpec { name: "recv_cnt", kind: FieldKind::U8 },
    FieldSpec { name: "jank", kind: FieldKind::Bool },
    FieldSpec { name: "cost_us", kind: FieldKind::U32 },
    FieldSpec { name: "max_tier", kind: FieldKind::U8 },
    FieldSpec { name: "tier_mask", kind: FieldKind::U8 },
];

/// schema V1（id + rev + 白名单；版本指纹 = FNV1a-32(id ‖ 0x1F ‖ 字段名
/// 序列 ‖ 0x1F ‖ rev)——只增不改演进下指纹随 rev 单调变化）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SchemaV1 {
    /// schema 标识（"DEGR_SCHEMA_V1"）。
    pub id: &'static str,
    /// 版次（从 1 起；演进一次 +1）。
    pub rev: u32,
    /// 字段白名单（定序）。
    pub fields: &'static [FieldSpec],
}

/// DEGR_SCHEMA_V1（降级遥测发布包 schema；演进规则：只增不改——
/// 新字段只能尾部追加、禁改名/禁改型/禁重排/禁删除，rev+1 且指纹必变）。
pub const DEGR_SCHEMA_V1: SchemaV1 =
    SchemaV1 { id: SCHEMA_ID, rev: 1, fields: &DEGR_SCHEMA_FIELDS };

impl SchemaV1 {
    /// 版本指纹（FNV1a-32 覆盖 id + 字段名序列 + rev；顺序敏感）。
    pub fn fingerprint(&self) -> u32 {
        let mut h: u32 = fnv1a32(self.id.as_bytes());
        let mut i = 0;
        while i < self.fields.len() {
            h ^= 0x1F;
            h = h.wrapping_mul(FNV_PRIME);
            h ^= fnv1a32(self.fields[i].name.as_bytes());
            h = h.wrapping_mul(FNV_PRIME);
            i += 1;
        }
        h ^= self.rev;
        h.wrapping_mul(FNV_PRIME)
    }

    /// 白名单成员判定（名字在名单 → true；发布行/回验用——只查名册
    /// 不查型，行值统一 u64 载体，型匹配由 `validate_field` 独立把关）。
    pub fn is_whitelisted(&self, name: &str) -> bool {
        let mut i = 0;
        while i < self.fields.len() {
            if self.fields[i].name == name {
                return true;
            }
            i += 1;
        }
        false
    }

    /// 白名单成员校验（名字在名单且型匹配 → Ok；否则对应错误）。
    pub fn validate_field(&self, name: &str, kind: FieldKind) -> Result<(), DegrTelError> {
        let mut i = 0;
        while i < self.fields.len() {
            let f = &self.fields[i];
            if f.name == name {
                if f.kind == kind {
                    return Ok(());
                }
                return Err(DegrTelError::SchemaMismatch);
            }
            i += 1;
        }
        Err(DegrTelError::PackFieldViolated)
    }

    /// 演进自查（只增不改规则的静态口径）：白名单非空、名字两两互异。
    pub fn evolution_selfcheck(&self) -> Result<(), DegrTelError> {
        if self.fields.is_empty() {
            return Err(DegrTelError::SchemaMismatch);
        }
        let mut i = 0;
        while i < self.fields.len() {
            let mut j = i + 1;
            while j < self.fields.len() {
                if self.fields[i].name == self.fields[j].name {
                    return Err(DegrTelError::SchemaMismatch);
                }
                j += 1;
            }
            i += 1;
        }
        Ok(())
    }
}

/// 发布包行构建（白名单驱动；任一字段越名单即
/// [`DegrTelError::PackFieldViolated`]——只发布白名单字段）。
///
/// 入参 pairs 按期望渲染序给出；名单校验逐对执行（名字在
/// [`DEGR_SCHEMA_FIELDS`] 内才放行），输出 `name=value` 以 `'|'` 连接
/// （家族同款分隔保空段格式，对齐 cgo01 面板快照序列化）。
pub fn build_pack_row(pairs: &[(&'static str, u64)]) -> Result<String, DegrTelError> {
    let mut out = String::new();
    let mut first = true;
    let mut i = 0;
    while i < pairs.len() {
        let (name, val) = pairs[i];
        if !DEGR_SCHEMA_V1.is_whitelisted(name) {
            return Err(DegrTelError::PackFieldViolated);
        }
        if !first {
            out.push('|');
        }
        out.push_str(name);
        out.push('=');
        out.push_str(&format!("{}", val));
        first = false;
        i += 1;
    }
    Ok(out)
}

/// 发布包 V1（头 + 帧行 + 尾；[`ReleasePackV1::verify`] 独立回验）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReleasePackV1 {
    /// 头行（schema 指纹 + 窗口/事件汇总）。
    pub header: String,
    /// 帧行序列（白名单序渲染；逐帧一行）。
    pub rows: Vec<String>,
    /// 尾行（守恒/对拍裁决 + 隐私标记）。
    pub footer: String,
    /// schema 版本指纹。
    pub schema_fp: u32,
    /// 帧行数。
    pub frame_count: usize,
    /// 帧闭合裁决（true = 全闭合后才放行发布）。
    pub closure_ok: bool,
}

impl ReleasePackV1 {
    /// 渲染（头/行/尾顺序拼接；家族 `'|'` 分隔格式）。
    pub fn render(&self) -> String {
        let mut out = String::new();
        out.push_str(&self.header);
        out.push('\n');
        let mut i = 0;
        while i < self.rows.len() {
            out.push_str(&self.rows[i]);
            out.push('\n');
            i += 1;
        }
        out.push_str(&self.footer);
        out
    }

    /// 回验（独立重查：schema 指纹一致 + 每行字段 ⊆ 白名单 + 帧闭合
    /// 裁决真；任一破即错——发布包里的每个数都可对账）。
    pub fn verify(&self) -> Result<(), DegrTelError> {
        if self.schema_fp != DEGR_SCHEMA_V1.fingerprint() {
            return Err(DegrTelError::SchemaMismatch);
        }
        if !self.closure_ok {
            return Err(DegrTelError::FrameOpenUnclosed);
        }
        let mut i = 0;
        while i < self.rows.len() {
            let row = &self.rows[i];
            let mut segs = row.split('|');
            let tag = segs.next().unwrap_or("");
            if tag != "F" {
                return Err(DegrTelError::PackFieldViolated);
            }
            let mut seg = segs.next();
            while let Some(s) = seg {
                let bytes = s.as_bytes();
                let mut eq = None;
                let mut k = 0;
                while k < bytes.len() {
                    if bytes[k] == b'=' {
                        eq = Some(k);
                        break;
                    }
                    k += 1;
                }
                match eq {
                    Some(p) => {
                        let name = &s[..p];
                        if !DEGR_SCHEMA_V1.is_whitelisted(name) {
                            return Err(DegrTelError::PackFieldViolated);
                        }
                    }
                    None => return Err(DegrTelError::PackFieldViolated),
                }
                seg = segs.next();
            }
            i += 1;
        }
        Ok(())
    }
}

/// 隐私出口闭集。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SinkKind {
    /// 本地环形账（默认出口——本地闭环）。
    LocalRing,
    /// 开发调试面板（本地渲染）。
    DebugPanel,
    /// 远端上传（红线出口——一律拒绝）。
    RemoteUpload,
}

/// 隐私闸（遥测本地闭环默认不上传；上传类出口一律拒绝并入账）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PrivacyGate {
    /// 本地-only 旗（默认 true；构造只提供 local()——红线不可关闭）。
    local_only: bool,
}

impl PrivacyGate {
    /// 本地闭环闸（唯一构造；红线常开）。
    pub const fn local() -> Self {
        PrivacyGate { local_only: true }
    }

    /// 出口裁决：本地类放行，RemoteUpload 拒
    /// [`DegrTelError::PrivacyUploadDenied`]。
    pub fn admit(&self, sink: SinkKind) -> Result<(), DegrTelError> {
        match sink {
            SinkKind::LocalRing | SinkKind::DebugPanel => Ok(()),
            SinkKind::RemoteUpload => Err(DegrTelError::PrivacyUploadDenied),
        }
    }

    /// 是否本地-only（恒 true——红线状态可读不可改）。
    pub fn is_local_only(&self) -> bool {
        self.local_only
    }
}

// ============================================================
// 十二、会话聚合
// ============================================================

/// 逐源分列（守恒对账的源粒度视图）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SourceCol {
    /// 源槽号。
    pub slot: u8,
    /// 源实例指纹。
    pub fp: u32,
    /// 类别（登记在案时）。
    pub kind: Option<DegrSourceKind>,
    /// 触发数（会话累计）。
    pub trig: u64,
    /// 执行数（会话累计）。
    pub exec: u64,
    /// 恢复数（会话累计）。
    pub recv: u64,
}

/// 会话聚合快照（滚动窗口 600 帧 + 会话累计双口径；诚实三态贯穿）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SessionAggregate {
    /// 窗口内已闭合帧数。
    pub window_frames: usize,
    /// 触发总数（会话累计）。
    pub trig_total: u64,
    /// 执行总数（会话累计）。
    pub exec_total: u64,
    /// 恢复总数（会话累计）。
    pub recv_total: u64,
    /// 卡顿计数·窗口内（滚动 600 帧）。
    pub jank_count_window: u64,
    /// 卡顿计数·累计。
    pub jank_count_total: u64,
    /// 降级时长占比 ppm（窗口内降级帧 × 10⁶ ÷ 窗口帧；空窗 Empty）。
    pub degrade_ppm: AggValue,
    /// 恢复时延 P95（µs；整数对数桶分位；零恢复 Empty）。
    pub recover_p95: AggValue,
    /// 恢复时延 P99（µs）。
    pub recover_p99: AggValue,
    /// 恢复时延 RMS（µs；整数开方）。
    pub recover_rms: AggValue,
    /// 恢复时延最大值（µs）。
    pub recover_max: AggValue,
    /// 帧成本 P95（µs）。
    pub cost_p95: AggValue,
    /// 帧成本 P99（µs）。
    pub cost_p99: AggValue,
    /// 滞回确认数（恢复后过滞回窗无再触发）。
    pub hys_confirm: u64,
    /// 滞回回摆数（恢复后立即再触发）。
    pub hys_reject: u64,
    /// 开链被顶替数（同源未恢复再触发——异常留痕口径）。
    pub superseded_chains: u64,
    /// 守恒对账报告。
    pub conservation: ConservationReport,
    /// 跨账对拍报告。
    pub reconcile: ReconcileReport,
    /// 逐源分列（七槽全列，未登记槽计数为零）。
    pub per_source: [SourceCol; SOURCE_KINDS],
}

// ============================================================
// 十三、降级遥测枢纽（DegrTelemetryHub · 全链编排）
// ============================================================

/// 开链（触发→恢复间的链路态；执行/恢复的前置依据）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct ChainOpen {
    /// 触发时戳（恢复时延结算起点）。
    trig_ts: u64,
    /// 触发序号。
    trig_seq: u64,
    /// 源指纹。
    fp: u32,
    /// 源槽号。
    slot: u8,
    /// 触发时档位。
    tier_at_trig: u8,
    /// 执行已见旗（重复执行拒）。
    exec_seen: bool,
}

/// 降级遥测枢纽（全链编排：事件准入 + 链路开合 + 帧粒度落行 +
/// 会话聚合 + 发布包；≈44KB 定长承载，建议 `Box` 上堆不上栈）。
#[derive(Clone, Debug)]
pub struct DegrTelemetryHub {
    /// 源登记表。
    registry: SourceRegistry,
    /// 事件环（定长 384 槽）。
    events: [Option<DegrEventV1>; EVENT_RING_CAP],
    /// 事件环头。
    ev_head: usize,
    /// 事件环现存量。
    ev_len: usize,
    /// 环满挤占数。
    ev_dropped: u64,
    /// 帧窗口（定长 600 帧行环）。
    rows: [FrameRow; SESSION_WINDOW_FRAMES],
    /// 帧窗口头。
    fw_head: usize,
    /// 帧窗口现存量。
    fw_len: usize,
    /// 开启帧的环内下标（None = 无开帧）。
    open_pos: Option<usize>,
    /// 上一帧号（严格递增校验基线；0 = 尚无帧）。
    last_frame_idx: u64,
    /// 上一已收 ts（单调基线；None = 尚无事件）。
    last_ts: Option<u64>,
    /// 上一已收 seq（单调基线）。
    last_seq: u64,
    /// 开链表（逐源一链；同源未恢复再触发即顶替留痕）。
    chains: [Option<ChainOpen>; SOURCE_KINDS],
    /// 滞回臂（恢复后待确认源旗）。
    hys_arm: [bool; SOURCE_KINDS],
    /// 滞回等待帧数（逐源）。
    hys_frames: [u8; SOURCE_KINDS],
    /// 滞回确认数。
    hys_confirm: u64,
    /// 滞回回摆数。
    hys_reject: u64,
    /// 开链被顶替数。
    superseded_chains: u64,
    /// 诊断账。
    ledger: TelEventLedger,
    /// 步数计。
    meter: StepMeter,
    /// 总账（触发/执行/恢复）。
    totals: [u64; 3],
    /// 逐源分列 [slot] → [trig, exec, recv]。
    src_cols: [[u64; 3]; SOURCE_KINDS],
    /// 拒收计数（ts 倒退）。
    rejected_ts: u64,
    /// 拒收计数（seq 倒退）。
    rejected_seq: u64,
    /// 拒收计数（未登记源）。
    rejected_src: u64,
    /// 拒收计数（孤儿事件）。
    rejected_orphan: u64,
    /// 恢复时延直方（µs；触发 ts → 恢复 ts）。
    recover_hist: LogHist,
    /// 帧成本直方（µs）。
    cost_hist: LogHist,
    /// 卡顿累计数。
    jank_total: u64,
    /// 窗口内降级帧数（滚动口径，end_frame 时随窗口滚动增量维护）。
    degrade_frames_in_window: u64,
    /// 隐私闸。
    privacy: PrivacyGate,
}

impl DegrTelemetryHub {
    /// 空枢纽（≈44KB 值语义构造；建议调用方立即 `Box::new` 上堆）。
    pub fn new() -> Self {
        DegrTelemetryHub {
            registry: SourceRegistry::new(),
            events: [None; EVENT_RING_CAP],
            ev_head: 0,
            ev_len: 0,
            ev_dropped: 0,
            rows: [FrameRow::EMPTY; SESSION_WINDOW_FRAMES],
            fw_head: 0,
            fw_len: 0,
            open_pos: None,
            last_frame_idx: 0,
            last_ts: None,
            last_seq: 0,
            chains: [None; SOURCE_KINDS],
            hys_arm: [false; SOURCE_KINDS],
            hys_frames: [0; SOURCE_KINDS],
            hys_confirm: 0,
            hys_reject: 0,
            superseded_chains: 0,
            ledger: TelEventLedger::new(),
            meter: StepMeter::new(),
            totals: [0; 3],
            src_cols: [[0; 3]; SOURCE_KINDS],
            rejected_ts: 0,
            rejected_seq: 0,
            rejected_src: 0,
            rejected_orphan: 0,
            recover_hist: LogHist::new(),
            cost_hist: LogHist::new(),
            jank_total: 0,
            degrade_frames_in_window: 0,
            privacy: PrivacyGate::local(),
        }
    }

    // ---------- 源登记 ----------

    /// 登记降级源实例（指纹重复拒、表满拒）。
    pub fn register_source(
        &mut self,
        kind: DegrSourceKind,
        fp: u32,
    ) -> Result<u8, DegrTelError> {
        self.registry.register(kind, fp)
    }

    /// 源槽查询。
    pub fn source_slot(&self, fp: u32) -> Option<u8> {
        self.registry.lookup(fp)
    }

    /// 已登记源数。
    pub fn source_count(&self) -> u32 {
        self.registry.count()
    }

    // ---------- 帧生命周期 ----------

    /// 开帧（帧号严格递增；已有开帧拒 [`DegrTelError::FrameAlreadyOpen`]）。
    pub fn begin_frame(&mut self, idx: u64, open_ts: u64) -> Result<(), DegrTelError> {
        if self.open_pos.is_some() {
            return Err(DegrTelError::FrameAlreadyOpen);
        }
        if idx <= self.last_frame_idx {
            self.ledger.push(TelCode::FrameIdxRegressed, open_ts, self.last_seq);
            return Err(DegrTelError::FrameIdxRegressed);
        }
        let pos = if self.fw_len == SESSION_WINDOW_FRAMES {
            // 窗口满：滚动挤占最旧帧（降级帧计数随滚出修正）。
            let old = self.rows[self.fw_head];
            if old.is_degraded() {
                self.degrade_frames_in_window = self.degrade_frames_in_window.saturating_sub(1);
            }
            self.fw_head = (self.fw_head + 1) % SESSION_WINDOW_FRAMES;
            self.fw_len -= 1;
            (self.fw_head + SESSION_WINDOW_FRAMES - 1) % SESSION_WINDOW_FRAMES
        } else {
            (self.fw_head + self.fw_len) % SESSION_WINDOW_FRAMES
        };
        self.rows[pos] = FrameRow { idx, open_ts, ..FrameRow::EMPTY };
        self.fw_len += 1;
        self.open_pos = Some(pos);
        self.last_frame_idx = idx;
        Ok(())
    }

    /// 帧内档位触达登记（0~5 闭集校验；开帧前置）。
    pub fn mark_tier(&mut self, tier: u8) -> Result<(), DegrTelError> {
        if tier > MAX_TIER {
            return Err(DegrTelError::TierOutOfRange);
        }
        match self.open_pos {
            Some(pos) => {
                let row = &mut self.rows[pos];
                if tier > row.max_tier {
                    row.max_tier = tier;
                }
                if tier >= 1 && tier <= 5 {
                    row.tier_mask |= 1u8 << tier;
                }
                Ok(())
            }
            None => Err(DegrTelError::FrameNotOpen),
        }
    }

    /// 闭帧（成本入直方；卡顿 = 显式旗 ∨ 成本超阈；滞回窗推进）。
    pub fn end_frame(&mut self, close_ts: u64, cost_us: u32, jank_flag: bool) -> Result<(), DegrTelError> {
        match self.open_pos {
            Some(pos) => {
                let jank = jank_flag || cost_us > JANK_THRESHOLD_US;
                let degraded;
                {
                    let row = &mut self.rows[pos];
                    row.close_ts = close_ts;
                    row.closed = true;
                    row.cost_us = cost_us;
                    row.jank = jank;
                    degraded = row.is_degraded();
                }
                self.cost_hist.push(cost_us as u64);
                if jank {
                    self.jank_total = self.jank_total.saturating_add(1);
                }
                if degraded {
                    self.degrade_frames_in_window = self.degrade_frames_in_window.saturating_add(1);
                }
                self.open_pos = None;
                // 滞回窗推进：待确认源逐帧 +1，达阈值即确认。
                let mut slot = 0;
                while slot < SOURCE_KINDS {
                    if self.hys_arm[slot] {
                        self.hys_frames[slot] = self.hys_frames[slot].saturating_add(1);
                        if self.hys_frames[slot] >= HYSTERESIS_CONFIRM_FRAMES {
                            self.hys_confirm = self.hys_confirm.saturating_add(1);
                            self.hys_arm[slot] = false;
                            self.hys_frames[slot] = 0;
                        }
                    }
                    slot += 1;
                }
                Ok(())
            }
            None => Err(DegrTelError::FrameNotOpen),
        }
    }

    /// 是否有未闭合帧。
    pub fn has_open_frame(&self) -> bool {
        self.open_pos.is_some()
    }

    /// 窗口内已闭合帧数。
    pub fn window_len(&self) -> usize {
        self.fw_len
    }

    /// 帧行快照（旧→新序）。
    pub fn frame_rows(&self) -> Vec<FrameRow> {
        let mut out = Vec::new();
        let mut i = 0;
        while i < self.fw_len {
            let pos = (self.fw_head + i) % SESSION_WINDOW_FRAMES;
            out.push(self.rows[pos]);
            i += 1;
        }
        out
    }

    // ---------- 事件准入 ----------

    /// 记录降级事件（枢纽主入口；全链三阶段）。
    ///
    /// 校验序：档位闭集 → 源登记（拒收不猜）→ ts 单调 → 链路前置
    /// （执行/恢复须有开链且执行不重复）→ 入环/落帧/记账。
    /// 任一拒收均入诊断账留痕且不进任何计数（守恒先于记录）。
    pub fn record_event(
        &mut self,
        ts: u64,
        fp: u32,
        stage: DegrStage,
        tier_before: u8,
        tier_after: u8,
    ) -> Result<DegrEventV1, DegrTelError> {
        self.meter.open();
        self.meter.step(); // 1 入口
        if tier_before > MAX_TIER || tier_after > MAX_TIER {
            self.ledger.push(TelCode::TierOutOfRange, ts, self.last_seq);
            self.meter.close();
            return Err(DegrTelError::TierOutOfRange);
        }
        self.meter.step(); // 2 档位校验
        let slot = match self.registry.lookup(fp) {
            Some(s) => s,
            None => {
                self.rejected_src = self.rejected_src.saturating_add(1);
                self.ledger.push(TelCode::SourceUnknown, ts, self.last_seq);
                self.meter.close();
                return Err(DegrTelError::SourceUnknown);
            }
        };
        self.meter.step(); // 3 源登记
        if let Some(last) = self.last_ts {
            if ts < last {
                self.rejected_ts = self.rejected_ts.saturating_add(1);
                self.ledger.push(TelCode::TsRegressed, ts, self.last_seq);
                self.meter.close();
                return Err(DegrTelError::TsRegressed);
            }
        }
        self.meter.step(); // 4 ts 单调
        let seq = self.last_seq.saturating_add(1);
        let ev = DegrEventV1::new(ts, seq, fp, slot, stage.idx(), tier_before, tier_after);
        self.apply_event(&ev)?; // 链路前置（内部已计步）
        self.commit(&ev);
        let used = self.meter.close();
        if used > RECORD_STEP_BUDGET {
            self.ledger.push(TelCode::BudgetOver, ts, seq);
        }
        Ok(ev)
    }

    /// 外部注入事件（回放/导入路径；(ts, seq) 双单调严校验）。
    pub fn ingest_event(&mut self, ev: DegrEventV1) -> Result<(), DegrTelError> {
        self.meter.open();
        self.meter.step(); // 1 入口
        // 域校验（阶段/槽/档位闭集）。
        if DegrStage::from_idx(ev.stage).is_none()
            || ev.src_slot as usize >= SOURCE_KINDS
            || ev.tier_before > MAX_TIER
            || ev.tier_after > MAX_TIER
        {
            self.ledger.push(TelCode::TierOutOfRange, ev.ts, self.last_seq);
            self.meter.close();
            return Err(DegrTelError::TierOutOfRange);
        }
        self.meter.step(); // 2 域校验
        // 槽-指纹一致校验（防张冠李戴）。
        match self.registry.slot(ev.src_slot) {
            Some(s) if s.fp == ev.fp => {}
            _ => {
                self.rejected_src = self.rejected_src.saturating_add(1);
                self.ledger.push(TelCode::SourceUnknown, ev.ts, self.last_seq);
                self.meter.close();
                return Err(DegrTelError::SourceUnknown);
            }
        }
        self.meter.step(); // 3 槽-指纹一致
        if let Some(last) = self.last_ts {
            if ev.ts < last {
                self.rejected_ts = self.rejected_ts.saturating_add(1);
                self.ledger.push(TelCode::TsRegressed, ev.ts, self.last_seq);
                self.meter.close();
                return Err(DegrTelError::TsRegressed);
            }
        }
        self.meter.step(); // 4 ts 单调
        if ev.seq <= self.last_seq {
            self.rejected_seq = self.rejected_seq.saturating_add(1);
            self.ledger.push(TelCode::SeqRegressed, ev.ts, self.last_seq);
            self.meter.close();
            return Err(DegrTelError::SeqRegressed);
        }
        self.meter.step(); // 5 seq 单调
        self.apply_event(&ev)?;
        self.commit(&ev);
        self.last_seq = ev.seq;
        let used = self.meter.close();
        if used > RECORD_STEP_BUDGET {
            self.ledger.push(TelCode::BudgetOver, ev.ts, ev.seq);
        }
        Ok(())
    }

    /// 链路前置与开合（触发开链/顶替留痕、执行标 seen、恢复闭链结算）。
    fn apply_event(&mut self, ev: &DegrEventV1) -> Result<(), DegrTelError> {
        let slot = ev.src_slot as usize;
        match ev.stage {
            0 => {
                // 触发：滞回臂未确认即回摆留痕（F2247 滞回口径镜像）。
                if self.hys_arm[slot] {
                    self.hys_reject = self.hys_reject.saturating_add(1);
                    self.hys_arm[slot] = false;
                    self.hys_frames[slot] = 0;
                    self.ledger.push(TelCode::HysBounce, ev.ts, ev.seq);
                }
                // 同源未恢复再触发：旧链顶替留痕（异常口径，不计入守恒破坏）。
                if self.chains[slot].is_some() {
                    self.superseded_chains = self.superseded_chains.saturating_add(1);
                }
                self.chains[slot] = Some(ChainOpen {
                    trig_ts: ev.ts,
                    trig_seq: ev.seq,
                    fp: ev.fp,
                    slot: ev.src_slot,
                    tier_at_trig: ev.tier_after,
                    exec_seen: false,
                });
                self.meter.step();
                Ok(())
            }
            1 => {
                match self.chains[slot] {
                    Some(c) if !c.exec_seen => {
                        self.chains[slot] = Some(ChainOpen { exec_seen: true, ..c });
                        self.meter.step();
                        Ok(())
                    }
                    _ => {
                        // 无链（孤儿执行）或重复执行：拒收，守恒先于记录。
                        self.rejected_orphan = self.rejected_orphan.saturating_add(1);
                        self.ledger.push(TelCode::StageOrphan, ev.ts, ev.seq);
                        self.meter.close();
                        Err(DegrTelError::StageOrphan)
                    }
                }
            }
            2 => {
                match self.chains[slot] {
                    // 恢复前置：链存在且执行已落地（未执行的降级不结算
                    // 恢复时延——未执行即回升走链顶替口径，不进恢复账，
                    // 以保守恒不等式 触发 ≥ 执行 ≥ 恢复 空气密）。
                    Some(c) if c.exec_seen => {
                        // 恢复时延 = 恢复 ts − 触发 ts（ts 单调保证非负）。
                        let latency = ev.ts.saturating_sub(c.trig_ts);
                        self.recover_hist.push(latency);
                        self.chains[slot] = None;
                        self.hys_arm[slot] = true;
                        self.hys_frames[slot] = 0;
                        self.meter.step();
                        Ok(())
                    }
                    None | Some(_) => {
                        // 无链（孤儿恢复）或有链但未执行：拒收，守恒先于记录。
                        self.rejected_orphan = self.rejected_orphan.saturating_add(1);
                        self.ledger.push(TelCode::StageOrphan, ev.ts, ev.seq);
                        self.meter.close();
                        Err(DegrTelError::StageOrphan)
                    }
                }
            }
            _ => {
                // 阶段域已在入口校验；此处理论不可达，诚实拒收。
                self.ledger.push(TelCode::TierOutOfRange, ev.ts, ev.seq);
                Err(DegrTelError::TierOutOfRange)
            }
        }
    }

    /// 提交（入环 + 落帧 + 记账；仅在校验全过后调用）。
    fn commit(&mut self, ev: &DegrEventV1) {
        // 事件环。
        if self.ev_len == EVENT_RING_CAP {
            self.ev_head = (self.ev_head + 1) % EVENT_RING_CAP;
            let pos = (self.ev_head + EVENT_RING_CAP - 1) % EVENT_RING_CAP;
            self.events[pos] = Some(*ev);
            self.ev_dropped = self.ev_dropped.saturating_add(1);
        } else {
            let pos = (self.ev_head + self.ev_len) % EVENT_RING_CAP;
            self.events[pos] = Some(*ev);
            self.ev_len += 1;
        }
        // 帧内计数。
        if let Some(pos) = self.open_pos {
            let row = &mut self.rows[pos];
            match ev.stage {
                0 => row.trig = row.trig.saturating_add(1),
                1 => row.exec = row.exec.saturating_add(1),
                _ => row.recv = row.recv.saturating_add(1),
            }
            let tb = ev.tier_before;
            let ta = ev.tier_after;
            if tb > row.max_tier {
                row.max_tier = tb;
            }
            if ta > row.max_tier {
                row.max_tier = ta;
            }
            if tb >= 1 && tb <= 5 {
                row.tier_mask |= 1u8 << tb;
            }
            if ta >= 1 && ta <= 5 {
                row.tier_mask |= 1u8 << ta;
            }
        }
        // 记账（总账 + 逐源分列 + 单调基线）。
        let st = ev.stage as usize;
        self.totals[st] = self.totals[st].saturating_add(1);
        self.src_cols[ev.src_slot as usize][st] =
            self.src_cols[ev.src_slot as usize][st].saturating_add(1);
        self.last_ts = Some(ev.ts);
        self.last_seq = ev.seq;
    }

    // ---------- 快照 ----------

    /// 事件快照（旧→新序；环满挤占者不含——dropped 计数显性）。
    pub fn events_snapshot(&self) -> Vec<DegrEventV1> {
        let mut out = Vec::new();
        let mut i = 0;
        while i < self.ev_len {
            let pos = (self.ev_head + i) % EVENT_RING_CAP;
            if let Some(e) = self.events[pos] {
                out.push(e);
            }
            i += 1;
        }
        out
    }

    /// 事件环挤占数。
    pub fn event_dropped(&self) -> u64 {
        self.ev_dropped
    }

    /// 诊断账引用（只读）。
    pub fn ledger(&self) -> &TelEventLedger {
        &self.ledger
    }

    /// 拒收总数（ts/seq/源/孤儿四类合计）。
    pub fn rejected_total(&self) -> u64 {
        self.rejected_ts
            .saturating_add(self.rejected_seq)
            .saturating_add(self.rejected_src)
            .saturating_add(self.rejected_orphan)
    }

    /// 拒收分项 (ts, seq, source, orphan)。
    pub fn rejected_breakdown(&self) -> (u64, u64, u64, u64) {
        (self.rejected_ts, self.rejected_seq, self.rejected_src, self.rejected_orphan)
    }

    /// 开销自证。
    pub fn overhead_audit(&self) -> OverheadReport {
        self.meter.audit()
    }

    /// 逐源分列裸表（守恒/对账函数入参形）。
    pub fn source_columns(&self) -> [[u64; 3]; SOURCE_KINDS] {
        self.src_cols
    }

    /// 总账三元组。
    pub fn totals(&self) -> [u64; 3] {
        self.totals
    }

    /// 滞回统计 (confirm, reject, superseded)。
    pub fn hysteresis_stats(&self) -> (u64, u64, u64) {
        (self.hys_confirm, self.hys_reject, self.superseded_chains)
    }

    /// 会话聚合（窗口 + 累计双口径；守恒/对拍随行）。
    pub fn session_aggregate(&self) -> SessionAggregate {
        // 窗口内卡顿与降级帧以 600 帧环实时扫描为准（滚动口径）。
        let mut jank_win: u64 = 0;
        let mut degrade_win: u64 = 0;
        let mut i = 0;
        while i < self.fw_len {
            let pos = (self.fw_head + i) % SESSION_WINDOW_FRAMES;
            let row = self.rows[pos];
            if row.closed {
                if row.jank {
                    jank_win = jank_win.saturating_add(1);
                }
                if row.is_degraded() {
                    degrade_win = degrade_win.saturating_add(1);
                }
            }
            i += 1;
        }
        let degrade_ppm = if self.fw_len == 0 {
            AggValue::Empty
        } else {
            AggValue::Value(
                degrade_win.saturating_mul(PPM_SCALE) / (self.fw_len as u64),
            )
        };
        let per = self.source_columns();
        SessionAggregate {
            window_frames: self.fw_len,
            trig_total: self.totals[0],
            exec_total: self.totals[1],
            recv_total: self.totals[2],
            jank_count_window: jank_win,
            jank_count_total: self.jank_total,
            degrade_ppm,
            recover_p95: self.recover_hist.percentile(95),
            recover_p99: self.recover_hist.percentile(99),
            recover_rms: self.recover_hist.rms(),
            recover_max: self.recover_hist.max(),
            cost_p95: self.cost_hist.percentile(95),
            cost_p99: self.cost_hist.percentile(99),
            hys_confirm: self.hys_confirm,
            hys_reject: self.hys_reject,
            superseded_chains: self.superseded_chains,
            conservation: conservation_audit(&per),
            reconcile: reconcile_tables(&per, &self.totals),
            per_source: self.source_cols_view(),
        }
    }

    /// 逐源分列视图（带类别注记）。
    fn source_cols_view(&self) -> [SourceCol; SOURCE_KINDS] {
        let mut out = [SourceCol {
            slot: 0,
            fp: 0,
            kind: None,
            trig: 0,
            exec: 0,
            recv: 0,
        }; SOURCE_KINDS];
        let mut i = 0;
        while i < SOURCE_KINDS {
            let (fp, kind) = match self.registry.slot(i as u8) {
                Some(s) => (s.fp, Some(s.kind)),
                None => (0, None),
            };
            out[i] = SourceCol {
                slot: i as u8,
                fp,
                kind,
                trig: self.src_cols[i][0],
                exec: self.src_cols[i][1],
                recv: self.src_cols[i][2],
            };
            i += 1;
        }
        out
    }

    // ---------- 发布包 ----------

    /// 导出发布包（帧闭合纪律 + 守恒/对拍可信纪律 + 隐私标记）。
    ///
    /// 拒发布条件：存在未闭合帧 [`DegrTelError::FrameOpenUnclosed`]；
    /// 守恒破坏 [`DegrTelError::ConservationBreak`]；对拍不平
    /// [`DegrTelError::ReconcileMismatch`]——发布包里的每个数都可对账。
    pub fn export_release_pack(&self) -> Result<ReleasePackV1, DegrTelError> {
        if self.open_pos.is_some() {
            return Err(DegrTelError::FrameOpenUnclosed);
        }
        let agg = self.session_aggregate();
        if !agg.conservation.ok {
            return Err(DegrTelError::ConservationBreak);
        }
        if !agg.reconcile.ok {
            return Err(DegrTelError::ReconcileMismatch);
        }
        // 头行。
        let header = format!(
            "PACK|DEGR_SCHEMA_V1|schema_fp=0x{:08X}|rev={}|window_frames={}|trig={}|exec={}|recv={}|jank_win={}|degrade_ppm={}|recover_p95={}|recover_p99={}|cost_p95={}|cost_p99={}",
            DEGR_SCHEMA_V1.fingerprint(),
            DEGR_SCHEMA_V1.rev,
            agg.window_frames,
            agg.trig_total,
            agg.exec_total,
            agg.recv_total,
            agg.jank_count_window,
            agg.degrade_ppm.display(),
            agg.recover_p95.display(),
            agg.recover_p99.display(),
            agg.cost_p95.display(),
            agg.cost_p99.display(),
        );
        // 帧行（白名单序渲染；只发白名单字段）。
        let mut rows = Vec::new();
        let mut i = 0;
        while i < self.fw_len {
            let pos = (self.fw_head + i) % SESSION_WINDOW_FRAMES;
            let row = self.rows[pos];
            if !row.closed {
                // 防御：闭合纪律已前置，此处理论不可达；诚实拒绝。
                return Err(DegrTelError::FrameOpenUnclosed);
            }
            let names = [
                "frame_idx", "open_ts", "close_ts", "trig_cnt", "exec_cnt", "recv_cnt",
                "jank", "cost_us", "max_tier", "tier_mask",
            ];
            let mut line = String::from("F|");
            let mut k = 0;
            while k < names.len() {
                let v = match row.field_value(names[k]) {
                    Some(v) => v,
                    None => return Err(DegrTelError::PackFieldViolated),
                };
                if k > 0 {
                    line.push('|');
                }
                line.push_str(names[k]);
                line.push('=');
                line.push_str(&format!("{}", v));
                k += 1;
            }
            rows.push(line);
            i += 1;
        }
        // 尾行（守恒/对拍裁决 + 隐私标记）。
        let footer = format!(
            "TAIL|conservation=OK|reconcile=OK|rejected={}|ledger_dropped={}|ev_dropped={}|hys_confirm={}|hys_reject={}|PRIVACY=LOCAL_ONLY|UPLOAD=DISABLED",
            self.rejected_total(),
            self.ledger.health().dropped,
            self.ev_dropped,
            self.hys_confirm,
            self.hys_reject,
        );
        Ok(ReleasePackV1 {
            header,
            rows,
            footer,
            schema_fp: DEGR_SCHEMA_V1.fingerprint(),
            frame_count: self.fw_len,
            closure_ok: true,
        })
    }

    /// 隐私闸出口裁决（转发 [`PrivacyGate::admit`]；拒绝自动入账）。
    pub fn privacy_admit(&mut self, sink: SinkKind) -> Result<(), DegrTelError> {
        let r = self.privacy.admit(sink);
        if let Err(DegrTelError::PrivacyUploadDenied) = r {
            self.ledger.push(TelCode::PrivacyUploadDenied, self.last_ts.unwrap_or(0), self.last_seq);
        }
        r
    }

    /// schema 引用（发布方对拍用）。
    pub fn schema(&self) -> SchemaV1 {
        DEGR_SCHEMA_V1
    }
}

// ============================================================
// 十四、测试（遥测一组：全链注入 → 聚合守恒 → 分位 → 发布包闭合 →
// 倒退拒收 → 对拍篡改 → 滞回 → 开销 → 隐私 → 编码往返）
// ============================================================

#[cfg(test)]
mod tests {
    use super::*;
    // 测试路径专用（枢纽 44KB 建议上堆；仅测试，不在内核运行路径）。
    use alloc::boxed::Box;

    /// 测试枢纽（Box 上堆，避开 44KB 栈承载）。
    fn hub() -> Box<DegrTelemetryHub> {
        Box::new(DegrTelemetryHub::new())
    }

    const FP_A: u32 = 0xA000_0001; // 帧超时实例 A
    const FP_B: u32 = 0xA000_0002; // 热档实例 B
    const FP_C: u32 = 0xA000_0003; // 资源紧张实例 C

    /// 登记三源。
    fn reg3(h: &mut DegrTelemetryHub) {
        assert_eq!(h.register_source(DegrSourceKind::FrameTimeout, FP_A).unwrap(), 0);
        assert_eq!(h.register_source(DegrSourceKind::Thermal, FP_B).unwrap(), 1);
        assert_eq!(h.register_source(DegrSourceKind::ResourceTight, FP_C).unwrap(), 2);
    }

    /// 一条完整链（触发→执行→恢复），返回恢复时延。
    fn chain(h: &mut DegrTelemetryHub, fp: u32, ts: u64, tier: u8) -> u64 {
        h.record_event(ts, fp, DegrStage::Trigger, 0, tier).unwrap();
        h.record_event(ts + 10, fp, DegrStage::Execute, tier, tier).unwrap();
        h.record_event(ts + 50, fp, DegrStage::Recover, tier, 0).unwrap();
        50
    }

    #[test]
    fn smoke_full_chain_conservation_and_pack() {
        let mut h = hub();
        reg3(&mut h);
        // 40 帧；帧 1~30 源 A 全链，帧 5~25 源 B 触发+执行（执行到 20 止），
        // 帧 31~40 源 C 仅触发。
        let mut ts: u64 = 1000;
        for idx in 1u64..=40 {
            h.begin_frame(idx, ts).unwrap();
            if idx <= 30 {
                let tier = if idx % 2 == 0 { 2 } else { 1 };
                h.record_event(ts + 1, FP_A, DegrStage::Trigger, 0, tier).unwrap();
                h.record_event(ts + 2, FP_A, DegrStage::Execute, tier, tier).unwrap();
                h.record_event(ts + 3, FP_A, DegrStage::Recover, tier, 0).unwrap();
            }
            if idx >= 5 && idx <= 25 {
                h.record_event(ts + 4, FP_B, DegrStage::Trigger, 0, 1).unwrap();
                if idx <= 20 {
                    h.record_event(ts + 5, FP_B, DegrStage::Execute, 1, 1).unwrap();
                }
            }
            if idx >= 31 {
                h.record_event(ts + 6, FP_C, DegrStage::Trigger, 0, 3).unwrap();
            }
            let cost = if idx % 10 == 0 { 20_000 } else { 9_000 };
            h.end_frame(ts + 900, cost, false).unwrap();
            ts += 1000;
        }
        let agg = h.session_aggregate();
        // 守恒：触发 30+21+10=61 ≥ 执行 30+16=46 ≥ 恢复 30。
        assert_eq!(agg.trig_total, 61);
        assert_eq!(agg.exec_total, 46);
        assert_eq!(agg.recv_total, 30);
        assert!(agg.conservation.ok, "守恒应通过：{:?}", agg.conservation.breaks);
        assert!(agg.reconcile.ok, "对拍应平");
        // 逐源分列。
        assert_eq!(agg.per_source[0].trig, 30);
        assert_eq!(agg.per_source[1].trig, 21);
        assert_eq!(agg.per_source[1].exec, 16);
        assert_eq!(agg.per_source[2].trig, 10);
        assert_eq!(agg.per_source[2].exec, 0);
        // 窗口口径：40 帧；降级帧 = 1~30（A 触达档）∪ 5~25（B 触达档 1）
        // ∪ 31~40（C 触达档 3）→ 全部 40 帧（触达留痕在帧内 max_tier）。
        assert_eq!(agg.window_frames, 40);
        assert_eq!(agg.degrade_ppm, AggValue::Value(1_000_000));
        // 卡顿：帧 10/20/30/40 成本 20000 > 16667 → 4 帧派生卡顿。
        assert_eq!(agg.jank_count_window, 4);
        assert_eq!(agg.jank_count_total, 4);
        // 分位三态：有恢复 → Value。
        assert!(matches!(agg.recover_p95, AggValue::Value(_)));
        assert!(matches!(agg.cost_p99, AggValue::Value(_)));
        // 发布包：全闭合 + 守恒 + 对拍 → 发布；回验通过。
        let pack = h.export_release_pack().unwrap();
        assert_eq!(pack.frame_count, 40);
        assert_eq!(pack.rows.len(), 40);
        pack.verify().unwrap();
        let text = pack.render();
        assert!(text.contains("PRIVACY=LOCAL_ONLY"));
        assert!(text.contains("UPLOAD=DISABLED"));
        assert!(text.contains("frame_idx=1|"));
        // 开销自证：全程在预算内。
        let od = h.overhead_audit();
        assert!(od.within_budget, "开销越限：{:?}", od);
        assert_eq!(od.violations, 0);
    }

    #[test]
    fn ts_regression_rejected_and_ledgered() {
        let mut h = hub();
        reg3(&mut h);
        h.record_event(1000, FP_A, DegrStage::Trigger, 0, 1).unwrap();
        let err = h.record_event(999, FP_A, DegrStage::Execute, 1, 1).unwrap_err();
        assert_eq!(err, DegrTelError::TsRegressed);
        assert_eq!(err.detail().contains("时间戳倒退"), true);
        // 拒收不入账（守恒先于记录）：触发 1、执行 0。
        let agg = h.session_aggregate();
        assert_eq!(agg.trig_total, 1);
        assert_eq!(agg.exec_total, 0);
        assert_eq!(h.rejected_breakdown().0, 1);
        // 诊断账留痕。
        assert_eq!(h.ledger().count_of(TelCode::TsRegressed), 1);
        assert!(h.ledger().health().ok);
    }

    #[test]
    fn seq_regression_rejected_on_ingest() {
        let mut h = hub();
        reg3(&mut h);
        let ev1 = h.record_event(1000, FP_A, DegrStage::Trigger, 0, 1).unwrap();
        // 合法注入：seq 递增。
        let mut bytes = [0u8; ENCODED_LEN];
        ev1.to_bytes(&mut bytes);
        let decoded = DegrEventV1::from_bytes(&bytes).unwrap();
        assert_eq!(decoded, ev1);
        let mut ev2 = ev1;
        ev2.seq = ev1.seq + 1;
        ev2.ts = 1001;
        ev2.stage = 1;
        h.ingest_event(ev2).unwrap();
        // 序号倒退注入：拒收。
        let mut ev3 = ev1;
        ev3.seq = ev1.seq; // 倒退（不大于 last_seq）
        ev3.ts = 1002;
        let err = h.ingest_event(ev3).unwrap_err();
        assert_eq!(err, DegrTelError::SeqRegressed);
        assert_eq!(h.rejected_breakdown().1, 1);
        assert_eq!(h.ledger().count_of(TelCode::SeqRegressed), 1);
    }

    #[test]
    fn unknown_source_rejected_no_guess() {
        let mut h = hub();
        reg3(&mut h);
        let err = h.record_event(1000, 0xDEAD_BEEF, DegrStage::Trigger, 0, 1).unwrap_err();
        assert_eq!(err, DegrTelError::SourceUnknown);
        assert_eq!(err.detail().contains("拒收不猜"), true);
        assert_eq!(h.rejected_breakdown().2, 1);
        assert_eq!(h.session_aggregate().trig_total, 0);
    }

    #[test]
    fn orphan_exec_rejected_conservation_kept() {
        let mut h = hub();
        reg3(&mut h);
        // 无触发直接执行 → 孤儿拒收。
        let err = h.record_event(1000, FP_A, DegrStage::Execute, 1, 1).unwrap_err();
        assert_eq!(err, DegrTelError::StageOrphan);
        // 触发后执行一次合法，第二次执行（重复）→ 孤儿拒收。
        h.record_event(1001, FP_A, DegrStage::Trigger, 0, 1).unwrap();
        h.record_event(1002, FP_A, DegrStage::Execute, 1, 1).unwrap();
        let err = h.record_event(1003, FP_A, DegrStage::Execute, 1, 1).unwrap_err();
        assert_eq!(err, DegrTelError::StageOrphan);
        let agg = h.session_aggregate();
        assert_eq!(agg.trig_total, 1);
        assert_eq!(agg.exec_total, 1);
        assert_eq!(agg.recv_total, 0);
        assert!(agg.conservation.ok);
        assert_eq!(h.rejected_breakdown().3, 2);
        assert_eq!(h.ledger().count_of(TelCode::StageOrphan), 2);
    }

    #[test]
    fn tier_out_of_range_rejected() {
        let mut h = hub();
        reg3(&mut h);
        let err = h.record_event(1000, FP_A, DegrStage::Trigger, 0, 6).unwrap_err();
        assert_eq!(err, DegrTelError::TierOutOfRange);
        assert_eq!(h.ledger().count_of(TelCode::TierOutOfRange), 1);
    }

    #[test]
    fn duplicate_and_full_registry() {
        let mut h = hub();
        h.register_source(DegrSourceKind::FrameTimeout, FP_A).unwrap();
        let err = h.register_source(DegrSourceKind::Thermal, FP_A).unwrap_err();
        assert_eq!(err, DegrTelError::SourceDuplicate);
        // 填满七类后表满拒。
        let kinds = [
            DegrSourceKind::Thermal,
            DegrSourceKind::Battery,
            DegrSourceKind::CgpuTier,
            DegrSourceKind::WeakNet,
            DegrSourceKind::ResourceTight,
            DegrSourceKind::SceneSwitch,
        ];
        let mut i = 0;
        while i < kinds.len() {
            h.register_source(kinds[i], 0xB000_0000 + i as u32).unwrap();
            i += 1;
        }
        assert_eq!(h.source_count(), 7);
        let err = h.register_source(DegrSourceKind::SceneSwitch, 0xC0FF_EE00).unwrap_err();
        assert_eq!(err, DegrTelError::RegistryFull);
    }

    #[test]
    fn percentile_integer_log_bucket() {
        let mut hist = LogHist::new();
        // 空窗诚实 Empty。
        assert_eq!(hist.percentile(95), AggValue::Empty);
        assert_eq!(hist.mean(), AggValue::Empty);
        assert_eq!(hist.rms(), AggValue::Empty);
        // 100 个样本 1..=100 µs：P95 = 95 → 落桶 6（64..127），插值于桶内。
        let mut v = 1;
        while v <= 100 {
            hist.push(v);
            v += 1;
        }
        match hist.percentile(95) {
            AggValue::Value(p) => assert!(p >= 64 && p < 128, "P95 应落桶 6 [64,128)：{}", p),
            other => panic_never(other),
        }
        // 全同值样本：分位落同桶（有界近似，误差 ≤ 桶宽）。
        let mut same = LogHist::new();
        let mut i = 0;
        while i < 100 {
            same.push(1000);
            i += 1;
        }
        match same.percentile(99) {
            AggValue::Value(p) => assert!(p >= 512 && p < 1024, "P99 应落桶 9 [512,1024)：{}", p),
            other => panic_never(other),
        }
        // p 越界 → Unknown（不猜）。
        assert_eq!(hist.percentile(0), AggValue::Unknown);
        assert_eq!(hist.percentile(100), AggValue::Unknown);
        // RMS：全同值 1000 → RMS = 1000 精确。
        assert_eq!(same.rms(), AggValue::Value(1000));
    }

    /// 测试内禁 panic! 直呼（纪律面统一出口；仅测试路径用）。
    #[cfg(test)]
    fn panic_never(v: AggValue) -> ! {
        // 测试路径允许 panic（非内核运行路径）。
        panic!("期望 Value 实得 {:?}", v)
    }

    #[test]
    fn aggregate_empty_windows_honest() {
        let h = hub();
        let agg = h.session_aggregate();
        // 全空窗：三态 Empty，不发零均值谎报。
        assert_eq!(agg.window_frames, 0);
        assert_eq!(agg.degrade_ppm, AggValue::Empty);
        assert_eq!(agg.recover_p95, AggValue::Empty);
        assert_eq!(agg.cost_p99, AggValue::Empty);
        assert_eq!(agg.degrade_ppm.display(), "--");
        assert_eq!(AggValue::Unknown.display(), "?");
        assert_eq!(AggValue::Value(7).display(), "7");
        // 空窗聚合依然守恒（0≥0≥0）。
        assert!(agg.conservation.ok);
        assert!(agg.reconcile.ok);
    }

    #[test]
    fn frame_discipline_and_regress() {
        let mut h = hub();
        reg3(&mut h);
        // 帧号倒退拒。
        h.begin_frame(5, 100).unwrap();
        h.end_frame(200, 1000, false).unwrap();
        let err = h.begin_frame(5, 300).unwrap_err();
        assert_eq!(err, DegrTelError::FrameIdxRegressed);
        assert_eq!(h.ledger().count_of(TelCode::FrameIdxRegressed), 1);
        // 重复开帧拒。
        h.begin_frame(6, 300).unwrap();
        let err = h.begin_frame(7, 400).unwrap_err();
        assert_eq!(err, DegrTelError::FrameAlreadyOpen);
        // 未开帧 mark/end 拒。
        let mut h2 = hub();
        let err = h2.mark_tier(1).unwrap_err();
        assert_eq!(err, DegrTelError::FrameNotOpen);
        let err = h2.end_frame(100, 1000, false).unwrap_err();
        assert_eq!(err, DegrTelError::FrameNotOpen);
        // 档位越界 mark 拒。
        let err = h.mark_tier(6).unwrap_err();
        assert_eq!(err, DegrTelError::TierOutOfRange);
        h.end_frame(500, 1000, false).unwrap();
        // 帧闭合纪律：开帧未闭 → 拒发布。
        h.begin_frame(7, 600).unwrap();
        let err = h.export_release_pack().unwrap_err();
        assert_eq!(err, DegrTelError::FrameOpenUnclosed);
        assert_eq!(err.detail().contains("帧闭合纪律"), true);
        h.end_frame(700, 1000, false).unwrap();
        assert!(h.export_release_pack().is_ok());
    }

    #[test]
    fn ppm_math_exact() {
        let mut h = hub();
        reg3(&mut h);
        // 600 帧，150 帧降级（max_tier=1）→ ppm = 250_000 精确。
        let mut ts: u64 = 100;
        for idx in 1u64..=600 {
            h.begin_frame(idx, ts).unwrap();
            if idx % 4 == 0 {
                h.mark_tier(1).unwrap();
            }
            h.end_frame(ts + 100, 5_000, false).unwrap();
            ts += 1000;
        }
        let agg = h.session_aggregate();
        assert_eq!(agg.window_frames, 600);
        assert_eq!(agg.degrade_ppm, AggValue::Value(250_000));
        // 窗口滚动：再开 10 帧全降级 → 最旧 10 帧滚出（其中 idx 4/8 非
        // 降级帧滚出 2 帧、降级帧滚出 8 帧 → 150-8+10=152 降级帧/600）。
        for idx in 601u64..=610 {
            h.begin_frame(idx, ts).unwrap();
            h.mark_tier(2).unwrap();
            h.end_frame(ts + 100, 5_000, false).unwrap();
            ts += 1000;
        }
        let agg2 = h.session_aggregate();
        assert_eq!(agg2.window_frames, 600);
        // 滚出帧 1..=10：降级帧为 4、8 共 2 帧 → 150-2+10 = 158。
        assert_eq!(agg2.degrade_ppm, AggValue::Value(158 * 1_000_000 / 600));
    }

    #[test]
    fn reconcile_tamper_detected() {
        let mut h = hub();
        reg3(&mut h);
        chain(&mut h, FP_A, 1000, 2);
        // 正常：对拍平。
        let per = h.source_columns();
        let totals = h.totals();
        assert!(reconcile_tables(&per, &totals).ok);
        // 篡改总账（执行 +1）→ 对拍破。
        let mut tampered_totals = totals;
        tampered_totals[1] = tampered_totals[1].saturating_add(1);
        let rep = reconcile_tables(&per, &tampered_totals);
        assert!(!rep.ok);
        assert_eq!(rep.mismatch_stage, Some(1));
        assert_eq!(rep.total, totals[1] + 1);
        // 篡改逐源（源 A 恢复 +1，越过执行数）→ 守恒破。
        let mut tampered_src = per;
        tampered_src[0][2] = tampered_src[0][2].saturating_add(1);
        let crep = conservation_audit(&tampered_src);
        assert!(!crep.ok);
        assert_eq!(crep.breaks.len(), 1);
        assert_eq!(crep.breaks[0].slot, 0);
        assert!(crep.breaks[0].detail.contains("不成立"));
    }

    #[test]
    fn hysteresis_bounce_and_confirm() {
        let mut h = hub();
        reg3(&mut h);
        let mut ts: u64 = 100;
        for idx in 1u64..=6 {
            h.begin_frame(idx, ts).unwrap();
            h.end_frame(ts + 100, 5_000, false).unwrap();
            ts += 1000;
        }
        // 链一：触发→执行→恢复 → 滞回臂立。
        h.record_event(ts, FP_A, DegrStage::Trigger, 0, 1).unwrap();
        h.record_event(ts + 5, FP_A, DegrStage::Execute, 1, 1).unwrap();
        h.record_event(ts + 10, FP_A, DegrStage::Recover, 1, 0).unwrap();
        // 恢复后立即再触发（滞回窗未过）→ 回摆留痕。
        h.record_event(ts + 20, FP_A, DegrStage::Trigger, 0, 1).unwrap();
        assert_eq!(h.hysteresis_stats().1, 1);
        assert_eq!(h.ledger().count_of(TelCode::HysBounce), 1);
        // 恢复 → 等 3 帧无再触发 → 确认。
        h.record_event(ts + 25, FP_A, DegrStage::Execute, 1, 1).unwrap();
        h.record_event(ts + 30, FP_A, DegrStage::Recover, 1, 0).unwrap();
        for idx in 7u64..=9 {
            h.begin_frame(idx, ts).unwrap();
            h.end_frame(ts + 100, 5_000, false).unwrap();
            ts += 1000;
        }
        let (confirm, reject, _sup) = h.hysteresis_stats();
        assert_eq!(confirm, 1, "滞回确认应恰一次");
        assert_eq!(reject, 1);
    }

    #[test]
    fn superseded_chain_counted() {
        let mut h = hub();
        reg3(&mut h);
        // 同源未恢复再触发：旧链顶替留痕。
        h.record_event(1000, FP_A, DegrStage::Trigger, 0, 1).unwrap();
        h.record_event(1100, FP_A, DegrStage::Trigger, 1, 2).unwrap();
        assert_eq!(h.hysteresis_stats().2, 1);
        // 新链执行后恢复（恢复须有执行前置——守恒口径）。
        h.record_event(1150, FP_A, DegrStage::Execute, 2, 2).unwrap();
        h.record_event(1200, FP_A, DegrStage::Recover, 2, 0).unwrap();
        let agg = h.session_aggregate();
        assert!(agg.conservation.ok);
        assert!(agg.reconcile.ok);
        assert_eq!(agg.superseded_chains, 1);
        assert_eq!((agg.trig_total, agg.exec_total, agg.recv_total), (2, 1, 1));
    }

    #[test]
    fn orphan_recover_rejected() {
        let mut h = hub();
        reg3(&mut h);
        // 恢复无执行前置（触发后直接恢复）→ 孤儿拒收，守恒不破。
        h.record_event(1000, FP_A, DegrStage::Trigger, 0, 1).unwrap();
        let err = h.record_event(1100, FP_A, DegrStage::Recover, 1, 0).unwrap_err();
        assert_eq!(err, DegrTelError::StageOrphan);
        let agg = h.session_aggregate();
        assert_eq!((agg.trig_total, agg.exec_total, agg.recv_total), (1, 0, 0));
        assert!(agg.conservation.ok);
        assert_eq!(h.ledger().count_of(TelCode::StageOrphan), 1);
    }

    #[test]
    fn schema_fingerprint_and_whitelist() {
        // 指纹稳定。
        assert_eq!(DEGR_SCHEMA_V1.fingerprint(), DEGR_SCHEMA_V1.fingerprint());
        assert_eq!(DEGR_SCHEMA_V1.id, "DEGR_SCHEMA_V1");
        assert_eq!(DEGR_SCHEMA_V1.rev, 1);
        assert_eq!(DEGR_SCHEMA_FIELDS.len(), 10);
        // 演进自查（名单非空、名字互异）。
        assert!(DEGR_SCHEMA_V1.evolution_selfcheck().is_ok());
        // 白名单内放行。
        assert!(build_pack_row(&[("frame_idx", 1), ("max_tier", 2)]).is_ok());
        // 白名单外拒。
        let err = build_pack_row(&[("hacked_field", 1)]).unwrap_err();
        assert_eq!(err, DegrTelError::PackFieldViolated);
        // 改型拒。
        assert!(DEGR_SCHEMA_V1
            .validate_field("frame_idx", FieldKind::Bool)
            .is_err());
        // 发布行回验：篡改行（越名单字段）被 verify 捉住。
        let mut pack = ReleasePackV1 {
            header: "PACK|test".to_string(),
            rows: Vec::new(),
            footer: "TAIL|PRIVACY=LOCAL_ONLY".to_string(),
            schema_fp: DEGR_SCHEMA_V1.fingerprint(),
            frame_count: 0,
            closure_ok: true,
        };
        assert!(pack.verify().is_ok());
        pack.rows.push("F|frame_idx=1|evil_field=9".to_string());
        assert_eq!(pack.verify().unwrap_err(), DegrTelError::PackFieldViolated);
        // 指纹不符被 verify 捉住。
        let mut pack2 = ReleasePackV1 {
            header: "PACK|test".to_string(),
            rows: Vec::new(),
            footer: "TAIL".to_string(),
            schema_fp: 0x1234_5678,
            frame_count: 0,
            closure_ok: true,
        };
        assert_eq!(pack2.verify().unwrap_err(), DegrTelError::SchemaMismatch);
        // 闭合裁决假被 verify 捉住。
        pack2.schema_fp = DEGR_SCHEMA_V1.fingerprint();
        pack2.closure_ok = false;
        assert_eq!(pack2.verify().unwrap_err(), DegrTelError::FrameOpenUnclosed);
    }

    #[test]
    fn privacy_gate_upload_denied() {
        let mut h = hub();
        reg3(&mut h);
        let gate = PrivacyGate::local();
        assert!(gate.is_local_only());
        assert!(gate.admit(SinkKind::LocalRing).is_ok());
        assert!(gate.admit(SinkKind::DebugPanel).is_ok());
        let err = gate.admit(SinkKind::RemoteUpload).unwrap_err();
        assert_eq!(err, DegrTelError::PrivacyUploadDenied);
        assert!(err.detail().contains("本地闭环"));
        // 枢纽拒绝自动入账。
        assert!(h.privacy_admit(SinkKind::RemoteUpload).is_err());
        assert_eq!(h.ledger().count_of(TelCode::PrivacyUploadDenied), 1);
        // 发布包隐私标记恒在。
        let ts: u64 = 100;
        h.begin_frame(1, ts).unwrap();
        h.end_frame(ts + 100, 5_000, false).unwrap();
        let pack = h.export_release_pack().unwrap();
        assert!(pack.footer.contains("PRIVACY=LOCAL_ONLY"));
        assert!(pack.footer.contains("UPLOAD=DISABLED"));
    }

    #[test]
    fn event_encode_roundtrip_and_domain_reject() {
        let ev = DegrEventV1::new(1234, 5678, 0xABCD_EF01, 3, 2, 5, 1);
        let mut bytes = [0u8; ENCODED_LEN];
        ev.to_bytes(&mut bytes);
        let back = DegrEventV1::from_bytes(&bytes).unwrap();
        assert_eq!(back, ev);
        assert_eq!(back.stage(), Some(DegrStage::Recover));
        assert_eq!(back.tier_dir_ok(), Some(true)); // 恢复 5→1 降档方向合法
        // 阶段域越界 → None（不猜）。
        let mut bad = bytes;
        bad[21] = 3;
        assert!(DegrEventV1::from_bytes(&bad).is_none());
        // 槽域越界 → None。
        bad[21] = 2;
        bad[20] = 7;
        assert!(DegrEventV1::from_bytes(&bad).is_none());
        // 档位越界 → None。
        bad[20] = 3;
        bad[23] = 6;
        assert!(DegrEventV1::from_bytes(&bad).is_none());
        // 注入路径同域校验。
        let mut h = hub();
        reg3(&mut h);
        let mut evil = DegrEventV1::new(100, 1, FP_A, 0, 1, 0, 9);
        assert_eq!(h.ingest_event(evil).unwrap_err(), DegrTelError::TierOutOfRange);
        evil.tier_after = 1;
        evil.src_slot = 5; // 槽存在但指纹不配 → 拒
        assert_eq!(h.ingest_event(evil).unwrap_err(), DegrTelError::SourceUnknown);
    }

    #[test]
    fn ledger_overflow_drops_oldest_and_health() {
        let mut h = hub();
        reg3(&mut h);
        // 注入 >192 条拒收（未登记源）挤占最旧。
        let mut ts: u64 = 1;
        let mut i = 0;
        while i < LEDGER_CAP + 10 {
            let _ = h.record_event(ts, 0x1234_5678, DegrStage::Trigger, 0, 1);
            ts += 1;
            i += 1;
        }
        let health = h.ledger().health();
        assert_eq!(health.used, LEDGER_CAP);
        assert_eq!(health.dropped, 10);
        assert!(!health.ok, "挤占后应显性不健康");
        // 码直方口径 = 历史入账总数（挤占留痕 LedgerFull 一并计入，不抹历史）
        // → SourceUnknown(202) + LedgerFull(10) 两种码。
        assert_eq!(health.codes_seen, 2);
        assert_eq!(health.top_code, Some(TelCode::SourceUnknown));
        // 码直方口径：历史入账总数（含被挤占）≥ 现存。
        assert!(h.ledger().count_of(TelCode::SourceUnknown) as u64
            >= health.used as u64);
    }

    #[test]
    fn event_ring_saturates_with_dropped_visible() {
        let mut h = hub();
        reg3(&mut h);
        // 200 条合法触发（单源开链顶替无碍）——环 384 不满；再 384 条溢出。
        let mut ts: u64 = 1;
        let mut i = 0;
        while i < EVENT_RING_CAP + 20 {
            let _ = h.record_event(ts, FP_A, DegrStage::Trigger, 0, 1);
            ts += 10;
            i += 1;
        }
        assert_eq!(h.events_snapshot().len(), EVENT_RING_CAP);
        assert_eq!(h.event_dropped(), 20);
        // 总账完整（环挤占不吞账）：触发 404 全在总账。
        assert_eq!(h.session_aggregate().trig_total, (EVENT_RING_CAP + 20) as u64);
        assert!(h.session_aggregate().conservation.ok);
    }

    #[test]
    fn recovery_latency_and_cost_percentiles_in_pack() {
        let mut h = hub();
        reg3(&mut h);
        let mut ts: u64 = 100;
        // 10 帧全链，恢复时延 50µs 恒定；帧成本交替 9_000/19_000。
        for idx in 1u64..=10 {
            h.begin_frame(idx, ts).unwrap();
            h.record_event(ts + 1, FP_A, DegrStage::Trigger, 0, 1).unwrap();
            h.record_event(ts + 2, FP_A, DegrStage::Execute, 1, 1).unwrap();
            h.record_event(ts + 51, FP_A, DegrStage::Recover, 1, 0).unwrap();
            let cost = if idx % 2 == 0 { 19_000 } else { 9_000 };
            h.end_frame(ts + 500, cost, false).unwrap();
            ts += 1000;
        }
        let agg = h.session_aggregate();
        assert_eq!(agg.recover_max, AggValue::Value(50));
        match agg.recover_p95 {
            AggValue::Value(p) => assert!(p >= 32 && p <= 64, "恒定 50µs P95 应落桶 5 [32,64)：{}", p),
            other => panic_never(other),
        }
        // 帧成本分位：19_000 占 5/10，P95 应落桶 14（16384..32768）。
        match agg.cost_p95 {
            AggValue::Value(p) => assert!(p >= 16_384 && p < 32_768, "P95 应落 19_000 桶：{}", p),
            other => panic_never(other),
        }
        // 发布包含聚合汇总。
        let pack = h.export_release_pack().unwrap();
        let text = pack.render();
        assert!(text.contains("recover_p95="));
        assert!(text.contains("degrade_ppm=1000000"));
    }

    #[test]
    fn diagnostic_segment_guard() {
        // 段守恒三元组（编译期已断言；此处对拍常量面）。
        assert_eq!(TEL_CODE_GUARD.0, 0x12E0);
        assert_eq!(TEL_CODE_GUARD.1, 0x12EF);
        assert_eq!(TEL_CODE_GUARD.2, 16);
        assert_eq!(TEL_CODE_TABLE.len(), 16);
        // 偏移往返。
        let mut i = 0;
        while i < 16 {
            let code = TelCode::from_offset(i).unwrap();
            assert_eq!(code.offset(), i);
            assert_eq!(code.code(), TEL_CODE_TABLE[i]);
            assert!(code.code() >= 0x12E0 && code.code() <= 0x12EF);
            assert!(!code.zh().is_empty());
            i += 1;
        }
    }

    #[test]
    fn isqrt_and_fnv_basics() {
        // 整数开方：完全平方精确、一般值向下取整。
        assert_eq!(isqrt_u64(0), 0);
        assert_eq!(isqrt_u64(1), 1);
        assert_eq!(isqrt_u64(1_000_000), 1000);
        assert_eq!(isqrt_u64(999_999), 999);
        assert_eq!(isqrt_u64(u64::MAX), 4_294_967_295);
        assert_eq!(isqrt_u128(1u128 << 120), 1u128 << 60);
        // FNV 已知值（标准测试向量 "a" → 0xE40C292C）。
        assert_eq!(fnv1a32(b"a"), 0xE40C_292C);
        assert_eq!(fnv_str("frame_timeout"), DegrSourceKind::FrameTimeout.canon_fp());
        // 七类规范指纹两两互异。
        let kinds = [
            DegrSourceKind::FrameTimeout,
            DegrSourceKind::Thermal,
            DegrSourceKind::Battery,
            DegrSourceKind::CgpuTier,
            DegrSourceKind::WeakNet,
            DegrSourceKind::ResourceTight,
            DegrSourceKind::SceneSwitch,
        ];
        let mut i = 0;
        while i < 7 {
            let mut j = i + 1;
            while j < 7 {
                assert_ne!(kinds[i].canon_fp(), kinds[j].canon_fp());
                j += 1;
            }
            assert_eq!(kinds[i].idx(), i as u8);
            assert_eq!(DegrSourceKind::from_idx(i as u8), Some(kinds[i]));
            i += 1;
        }
        assert!(DegrSourceKind::from_idx(7).is_none());
        assert!(DegrStage::from_idx(3).is_none());
    }

    #[test]
    fn ingest_happy_path_full_chain() {
        let mut h = hub();
        reg3(&mut h);
        // 枢纽产一枚触发事件，编码后注入回放。
        let ev = h.record_event(1000, FP_A, DegrStage::Trigger, 0, 2).unwrap();
        let mut bytes = [0u8; ENCODED_LEN];
        ev.to_bytes(&mut bytes);
        // 同事件重复注入：seq 不再单调 → 拒。
        let dup = DegrEventV1::from_bytes(&bytes).unwrap();
        assert_eq!(h.ingest_event(dup).unwrap_err(), DegrTelError::SeqRegressed);
        // 回放后续：执行（seq+1, ts+5）经编码往返注入。
        let ex = DegrEventV1::new(1005, ev.seq + 1, FP_A, 0, 1, 2, 2);
        h.ingest_event(ex).unwrap();
        let rc = DegrEventV1::new(1100, ev.seq + 2, FP_A, 0, 2, 2, 0);
        h.ingest_event(rc).unwrap();
        let agg = h.session_aggregate();
        assert_eq!((agg.trig_total, agg.exec_total, agg.recv_total), (1, 1, 1));
        assert!(agg.conservation.ok);
        assert_eq!(agg.recover_max, AggValue::Value(100));
    }
}
