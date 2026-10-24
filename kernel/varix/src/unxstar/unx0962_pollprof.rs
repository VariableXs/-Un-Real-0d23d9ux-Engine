//! unx0962_pollprof — UNX-F0962 轮询路径特征画像与接管难度分级。
//!
//! 判据原文（施工书锚点 UNX-F0962，CoRun·UNX-A2-B09 批，docs/Varix/
//! "CoRun Varix STAR II · Unxreal.md" 行 2643）：
//! **UNX-F0962-J1 分级正确（三档难度各自 10/10 次判定对），接管排期
//! 输入齐备（分级清册导出），判定可重复（同点两次判定一致率 100%）**。
//!
//! 定位原文（同册同批表头）：
//! | UNX-F0962 | 轮询路径特征画像与接管难度分级 | 300 | 已深化 |
//! | UNX-F0961-J1 判据（同批上游，本单消费其测绘点）：盘点完整
//!   （现存全部轮询点清册零遗漏——grep+运行期双源交叉），特征画像落账，
//!   盘点与运行观测一致率 100% |
//!
//! 职责定位：F0961 把现存轮询点**盘点**出来了（有哪些点、在哪条路径
//! 上），本单回答的是接管工程最贵的那一问——**每个点接过来有多难、
//! 该排到哪一档、什么时候能动它**。Windows 深在哪：Windows 对轮询
//! 路径没有画像概念——谁在忙等、等多久、跟谁耦合，全靠工程师翻汇编
//! 与打日志，接管（改中断化）排期靠拍脑袋，改坏一个时序敏感点就整链
//! 卡死；Varix：①**六维特征画像**——每个轮询点六维刻度（等待语义/
//! 事件率/上下文依赖/重入风险/时序敏感度/状态耦合，各 0..=7 刻度，
//! const 表逐级写死语义），画像不是自由文本是**可判定的刻度向量**；
//! ②**三档难度分级**——加权记分 + 强制抬档规则（const 规则表），
//! Easy/Medium/Hard 各 10/10 次判定对（30 条固定夹具向量自证），
//! 判定理由逐维可解释输出（哪个维贡献了多少分、命中哪条强制规则）；
//! ③**判定确定性**——判定路径零随机源、零墙钟（tick 显性注入不入
//! 判定），同点两次判定逐字段一致率 100%（双跑对拍自证），任何阈值
//! 篡改都会被对拍当场揪出（判定不一致注入检出）；④**接管排期输入
//! （分级清册导出）**——每点四列：点 id/难度档/依赖前驱/风险注记，
//! 列齐备性机检、导出序确定性（档位升序+点 id 升序，插排手写零
//! 泛型依赖）、双导出指纹对拍防漂移；⑤**测绘账门槛**——画像只消费
//! F1283/F0961 测绘账内已测绘点，未测绘点拒收带码（0x1A11），特征
//! 刻度越界拒收带码（0x1A12），越界注入/未测绘注入/判定不一致注入
//! 三面全检出。
//! 上游：F0961 现存轮询栈盘点账（测绘点来源）、F1283 内存域测绘点
//! （第二测绘来源）、F0039 双模开关纲领（分级是接管的排期输入）；
//! 下游：F0963 IrqBackend 接线面（Easy 档先接线）、F0965 Draining
//! 排空（Hard 档排期靠后）、F0969 零丢事件对账（按档抽样对账密度）。
//! 本单交付**六维刻度表 + 特征向量 + 测绘账门槛 + 特征画像 + 加权
//! 判级器 + 强制抬档规则 + 30 夹具 10/10 自证 + 双跑对拍 + 依赖前驱
//! 账 + 分级清册导出 + 事件账 + 注入检出三面 + health/FNV 指纹**
//! 十三面。
//!
//! 设计要点：no_std+alloc；零 unwrap/expect/panic!/运行期 assert!；
//! 零 IO 零墙钟（tick 显性注入）；可失败全走
//! `Result<T, PollProfError>`；溢出显性 saturating/wrapping；错误
//! 枚举每变体带 `detail()` 中文可操作诊断；const fn 只用固定数组
//! + while 索引（不用 slice::get/Vec 索引/Ord::min/max/derive 比较）；
//! 大结构全走堆（Vec 环形事件账不上栈）。诊断码段 0x1A10 独占
//! （0x19D0 为 CGPU-F0030 光栅化所取、0x19E0 为 ves475_s26arch
//! 所取、0x19F0 为 unx0944_bhtrig 所取、0x1A00 为 ves476_s26static
//! 与 vea40_fmtcaps 所取、0x1A20/0x1A40 为 ves477/ves478 所取，
//! 0x1A10..0x1A1F 整仓 grep 零占用后选定）。

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

/// 模块版本（判据锚点 + 段号入版本串，跨工具消费可对账）。
pub const POLLPROF_VERSION: &str = "B09-pollprof-v1";

// ---------------------------------------------------------------------------
// §0 诊断码（0x1A10 段，本单独占）
//
// 选段依据：整仓 grep 逐段排查——0x19D*（CGPU-F0030 光栅化已取
// 0x19D0）、0x19E*（ves475_s26arch 已取 0x19E0）、0x19F*（unx0944
// _bhtrig 已取 0x19F0）、0x1A0*（ves476_s26static/vea40_fmtcaps 已取
// 0x1A00..0x1A04）、0x1A2*/0x1A4*（ves477/ves478）均被占；
// 0x1A10..0x1A1F 全仓零占用，故本单取 0x1A10..=0x1A1F 独占。
// ---------------------------------------------------------------------------

/// 0x1A11 未测绘点拒收（画像只消费测绘账内已测绘点）。
pub const XC_NOT_SURVEYED: u16 = 0x1A11;
/// 0x1A12 特征刻度越界（任一维超出 0..=7 刻度域）。
pub const XC_FEAT_OUT_OF_RANGE: u16 = 0x1A12;
/// 0x1A13 判定不一致（双跑对拍档位或得分失配——判级路径被污染）。
pub const XC_GRADE_INCONSISTENT: u16 = 0x1A13;
/// 0x1A14 测绘账空（无任何测绘点可画像——上游 F0961/F1283 未落账）。
pub const XC_SURVEY_EMPTY: u16 = 0x1A14;
/// 0x1A15 画像重复建档（同点 id 重复画像——账面被污染或调用序错）。
pub const XC_PROFILE_DUPLICATE: u16 = 0x1A15;
/// 0x1A16 清册列缺失（四列任一为空——排期输入不齐备，禁止导出）。
pub const XC_SCHEDULE_INCOMPLETE: u16 = 0x1A16;
/// 0x1A17 依赖边非法（自环/端点未测绘/端点为零号保留点）。
pub const XC_DEPENDENCY_ILLEGAL: u16 = 0x1A17;
/// 0x1A18 导出指纹漂移（同账双导出指纹失配——导出序不确定）。
pub const XC_EXPORT_DRIFT: u16 = 0x1A18;
/// 0x1A19 判级阈值非法（Easy 上限 ≥ Medium 上限，或越出刻度乘积域）。
pub const XC_THRESHOLD_INVALID: u16 = 0x1A19;
/// 0x1A1A 测绘 tick 回退（测绘账 tick 单调性被破坏）。
pub const XC_TICK_REGRESS: u16 = 0x1A1A;
/// 0x1A1B 账面已封账后再写入（封账后 ingest——账面纪律破坏）。
pub const XC_LEDGER_CLOSED: u16 = 0x1A1B;
/// 0x1A1C 事件账容量非法（环容量为零——构造参数错误）。
pub const XC_RING_CAPACITY: u16 = 0x1A1C;

// 编译期纪律自证：判级阈值有序、诊断码两两互异、六维权重和为 100。
const _: () = assert!(TIER_EASY_MAX < TIER_MED_MAX);
const _: () = assert!(XC_NOT_SURVEYED != XC_FEAT_OUT_OF_RANGE);
const _: () = assert!(XC_FEAT_OUT_OF_RANGE != XC_GRADE_INCONSISTENT);
const _: () = assert!(XC_GRADE_INCONSISTENT != XC_EXPORT_DRIFT);
const _: () = assert!(WEIGHT_TOTAL == 100);

// ---------------------------------------------------------------------------
// §1 六维刻度（const 语义表）
//
// 每维 0..=7 共八级，逐级中文语义写死在 const 表——画像不是自由文本，
// 是可判定的刻度向量。刻度语义来源：F0961 盘点账的观测字段归类
// （等待窗口/事件频率/调用上下文/共享态/时序窗/耦合面六类观测）。
// ---------------------------------------------------------------------------

/// 维度数（编译期定死——六维画像，不可增减）。
pub const DIM_COUNT: usize = 6;

/// 维序：0 等待语义 / 1 事件率 / 2 上下文依赖 / 3 重入风险 /
/// 4 时序敏感度 / 5 状态耦合。
pub const DIM_NAMES: [&str; DIM_COUNT] = [
    "等待语义",
    "事件率",
    "上下文依赖",
    "重入风险",
    "时序敏感度",
    "状态耦合",
];

/// 维 0 等待语义刻度表（0 事件驱动旁路 → 7 全程忙等硬窗）。
pub const WAIT_SCALE: [&str; 8] = [
    "事件驱动旁路（几乎不空转）",
    "条件短轮询（循环体一次即出）",
    "有界超时轮询（超时即放弃）",
    "超时重试轮询（超时后重来）",
    "长窗轮询（毫秒级空转显著）",
    "密集忙等（无 yield 持续烧核）",
    "关键路径忙等（错过窗口即失败）",
    "全程忙等硬窗（不轮到位设备即挂）",
];

/// 维 1 事件率刻度表（0 近零事件 → 7 风暴级）。
pub const EVENT_SCALE: [&str; 8] = [
    "近零事件（秒级一次以下）",
    "稀疏事件（秒级个位数）",
    "低频事件（百毫秒级）",
    "常规事件（十毫秒级）",
    "高频事件（毫秒级）",
    "密集事件（百微秒级）",
    "尖峰事件（突发千级）",
    "风暴级（持续万级每秒）",
];

/// 维 2 上下文依赖刻度表（0 任意上下文 → 7 仅特定核与特定 IRQL 窗）。
pub const CTX_SCALE: [&str; 8] = [
    "任意上下文可调",
    "任意上下文但需关抢占",
    "进程上下文限定",
    "进程上下文且持轻锁",
    "软中断上下文限定",
    "硬中断上下文限定",
    "仅特定核心可调",
    "仅特定核+特定优先级窗",
];

/// 维 3 重入风险刻度表（0 无共享态 → 7 递归重入已实际发生过）。
pub const REENTRY_SCALE: [&str; 8] = [
    "无共享态（天然不可重入）",
    "只读共享态",
    "单写者共享态",
    "多写者但有锁序",
    "多写者锁序未成文",
    "曾现软锁死已修",
    "曾现活锁已修",
    "递归重入已实际发生过",
];

/// 维 4 时序敏感度刻度表（0 无时序要求 → 7 亚微秒硬窗）。
pub const TIMING_SCALE: [&str; 8] = [
    "无时序要求",
    "宽松（秒级容忍）",
    "常规（百毫秒窗）",
    "常规（十毫秒窗）",
    "紧（毫秒窗）",
    "很紧（百微秒窗）",
    "极紧（十微秒窗）",
    "亚微秒硬窗（错过即败）",
];

/// 维 5 状态耦合刻度表（0 无外部状态 → 7 多子系统双向耦合）。
pub const COUPLE_SCALE: [&str; 8] = [
    "无外部状态",
    "只读外部状态",
    "单点单向写",
    "多点单向写",
    "单点双向",
    "多点双向（域内）",
    "跨域双向",
    "多子系统双向耦合",
];

/// 各维刻度表的 const 汇总（按维序索引——导出与解释面共用）。
pub const DIM_SCALES: [[&str; 8]; DIM_COUNT] = [
    WAIT_SCALE,
    EVENT_SCALE,
    CTX_SCALE,
    REENTRY_SCALE,
    TIMING_SCALE,
    COUPLE_SCALE,
];

// ---------------------------------------------------------------------------
// §2 六维权重与判级阈值（const 规则表——判据"分级正确"的刻度来源）
//
// 权重总和恒为 100（编译期自证），加权分 = Σ 权重×刻度，值域 0..=700。
// 权重定值依据：等待语义是接管成本第一因（忙等改中断是行为等价性
// 最难保的），重入与时序并列第二（改错即死），耦合第三，事件率与
// 上下文是观测成本非改造成本——故给低权。
// ---------------------------------------------------------------------------

/// 维 0 等待语义权重：25。
pub const W_WAIT: u32 = 25;
/// 维 1 事件率权重：10。
pub const W_EVENT: u32 = 10;
/// 维 2 上下文依赖权重：10。
pub const W_CTX: u32 = 10;
/// 维 3 重入风险权重：20。
pub const W_REENTRY: u32 = 20;
/// 维 4 时序敏感度权重：20。
pub const W_TIMING: u32 = 20;
/// 维 5 状态耦合权重：15。
pub const W_COUPLE: u32 = 15;

/// 权重 const 汇总表（按维序）。
pub const WEIGHTS: [u32; DIM_COUNT] = [W_WAIT, W_EVENT, W_CTX, W_REENTRY, W_TIMING, W_COUPLE];

/// 权重和（=100，编译期断言）。
pub const WEIGHT_TOTAL: u32 = 100;

/// Easy 档上限（加权分 ≤ 180 判 Easy；180 为设计定值：约半数维
/// ≤3 且无强制规则的点，改造为行为等价性可逐行目验）。
pub const TIER_EASY_MAX: u32 = 180;
/// Medium 档上限（181..=380 判 Medium；380 为设计定值：多维 4~6
/// 但强制规则未触发 Hard 的点，改造需双模对账）。
pub const TIER_MED_MAX: u32 = 380;
/// 加权分值域上限（700 = Σ 7×权重）。
pub const SCORE_MAX: u32 = 700;

// ---------------------------------------------------------------------------
// §3 错误枚举（每变体带中文可操作诊断）
// ---------------------------------------------------------------------------

/// 本单全部可失败路径的错误枚举。
///
/// 每变体绑定诊断码（0x1A10 段）与 `detail()` 中文可操作诊断——
/// 内核路径遇错不停机，逐错带码上账由调用方裁决。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PollProfError {
    /// 未测绘点拒收（0x1A11）。
    NotSurveyed {
        /// 被拒收的点 id。
        point_id: u32,
    },
    /// 特征刻度越界（0x1A12）。
    FeatOutOfRange {
        /// 越界的维序。
        dim: usize,
        /// 越界的值。
        value: u8,
    },
    /// 判定不一致（0x1A13）。
    GradeInconsistent {
        /// 点 id。
        point_id: u32,
        /// 首跑档位码。
        tier_a: u8,
        /// 次跑档位码。
        tier_b: u8,
    },
    /// 测绘账空（0x1A14）。
    SurveyEmpty,
    /// 画像重复建档（0x1A15）。
    ProfileDuplicate {
        /// 重复的点 id。
        point_id: u32,
    },
    /// 清册列缺失（0x1A16）。
    ScheduleIncomplete {
        /// 缺列的行号（导出序内）。
        row: usize,
        /// 缺的列名。
        column: &'static str,
    },
    /// 依赖边非法（0x1A17）。
    DependencyIllegal {
        /// 前驱点 id。
        pred: u32,
        /// 后继点 id。
        point: u32,
    },
    /// 导出指纹漂移（0x1A18）。
    ExportDrift {
        /// 首导出指纹。
        first: u64,
        /// 次导出指纹。
        second: u64,
    },
    /// 判级阈值非法（0x1A19）。
    ThresholdInvalid {
        /// 传入的 Easy 上限。
        easy_max: u32,
        /// 传入的 Medium 上限。
        med_max: u32,
    },
    /// 测绘 tick 回退（0x1A1A）。
    TickRegress {
        /// 账面已记录的最新 tick。
        last: u64,
        /// 传入的更小 tick。
        got: u64,
    },
    /// 封账后再写入（0x1A1B）。
    LedgerClosed,
    /// 事件账容量非法（0x1A1C）。
    RingCapacity {
        /// 传入的容量。
        got: usize,
    },
}

impl PollProfError {
    /// 该错误绑定的诊断码（0x1A10 段）。
    pub fn code(&self) -> u16 {
        match self {
            PollProfError::NotSurveyed { .. } => XC_NOT_SURVEYED,
            PollProfError::FeatOutOfRange { .. } => XC_FEAT_OUT_OF_RANGE,
            PollProfError::GradeInconsistent { .. } => XC_GRADE_INCONSISTENT,
            PollProfError::SurveyEmpty => XC_SURVEY_EMPTY,
            PollProfError::ProfileDuplicate { .. } => XC_PROFILE_DUPLICATE,
            PollProfError::ScheduleIncomplete { .. } => XC_SCHEDULE_INCOMPLETE,
            PollProfError::DependencyIllegal { .. } => XC_DEPENDENCY_ILLEGAL,
            PollProfError::ExportDrift { .. } => XC_EXPORT_DRIFT,
            PollProfError::ThresholdInvalid { .. } => XC_THRESHOLD_INVALID,
            PollProfError::TickRegress { .. } => XC_TICK_REGRESS,
            PollProfError::LedgerClosed => XC_LEDGER_CLOSED,
            PollProfError::RingCapacity { .. } => XC_RING_CAPACITY,
        }
    }

    /// 中文可操作诊断（一行，指出原因与下一步动作）。
    pub fn detail(&self) -> &'static str {
        match self {
            PollProfError::NotSurveyed { .. } => {
                "该点未在 F1283/F0961 测绘账内落账，画像拒收；先经盘点账 ingest 该点后重试"
            }
            PollProfError::FeatOutOfRange { .. } => {
                "特征刻度越界（合法域 0..=7）；核对六维刻度表后以合法刻度重新提交画像"
            }
            PollProfError::GradeInconsistent { .. } => {
                "同点双跑判定失配——判级路径被污染或阈值被篡改；核对判级器阈值与权重表后重建判级器"
            }
            PollProfError::SurveyEmpty => {
                "测绘账空——上游 F0961/F1283 尚未落任何测绘点；先完成盘点账 ingest 再画像"
            }
            PollProfError::ProfileDuplicate { .. } => {
                "同点 id 重复建档；先摘除旧画像或改用更新入口，禁止同 id 双画像"
            }
            PollProfError::ScheduleIncomplete { .. } => {
                "清册四列（点id/难度档/依赖前驱/风险注记）任一为空；补齐行数据后再导出排期输入"
            }
            PollProfError::DependencyIllegal { .. } => {
                "依赖边非法（自环/端点未测绘/零号保留点）；修正依赖前驱账后重建清册"
            }
            PollProfError::ExportDrift { .. } => {
                "同账双导出指纹失配——导出序不确定；检查排序比较器全序性后重导"
            }
            PollProfError::ThresholdInvalid { .. } => {
                "判级阈值非法（需 Easy 上限 < Medium 上限 ≤ 700）；以常量表默认值重建判级器"
            }
            PollProfError::TickRegress { .. } => {
                "测绘账 tick 回退；注入源时钟错序，请按单调序重放 ingest"
            }
            PollProfError::LedgerClosed => {
                "账面已封账；封账后只读，如需续写请重建账面"
            }
            PollProfError::RingCapacity { .. } => {
                "事件账环容量为零；以 ≥1 的容量重建事件账"
            }
        }
    }
    /// 中文可操作诊断别名（`detail()` 同文——测试与解释面统一入口名）。
    pub fn detail_straight(&self) -> &'static str {
        self.detail()
    }
}

// ---------------------------------------------------------------------------
// §4 FNV-1a 64 指纹（画像/清册/健康快照共用）
// ---------------------------------------------------------------------------

/// FNV-1a 64 偏移基（公开参数，来源：FNV 参考实现）。
pub const FNV_OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
/// FNV-1a 64 素数（公开参数，来源：FNV 参考实现）。
pub const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;

/// FNV-1a 64：逐字节掺入（指纹只做对拍与账面绑定，不做安全用途）。
pub fn fnv1a64(data: &[u8]) -> u64 {
    let mut h: u64 = FNV_OFFSET;
    let mut i: usize = 0;
    while i < data.len() {
        h ^= data[i] as u64;
        h = h.wrapping_mul(FNV_PRIME);
        i += 1;
    }
    h
}

/// FNV-1a 64：单字节便捷掺入（供增量指纹场景）。
pub fn fnv1a64_byte(h: u64, b: u8) -> u64 {
    (h ^ b as u64).wrapping_mul(FNV_PRIME)
}

// ---------------------------------------------------------------------------
// §5 六维特征向量
// ---------------------------------------------------------------------------

/// 六维特征向量——每维 0..=7 刻度（见 §1 语义表）。
///
/// 刻度向量是画像的全部输入：判级、解释、指纹都只消费这六个字节，
/// 不消费自由文本——保证判定确定性的根本就是**输入域封闭**。
#[derive(Clone, Copy, Debug)]
pub struct FeatureVector {
    dims: [u8; DIM_COUNT],
}

impl FeatureVector {
    /// 以六刻度构造（运行期校验版——越界带码拒收）。
    pub fn new(dims: [u8; DIM_COUNT]) -> Result<FeatureVector, PollProfError> {
        let mut i: usize = 0;
        while i < DIM_COUNT {
            if dims[i] > 7 {
                return Err(PollProfError::FeatOutOfRange {
                    dim: i,
                    value: dims[i],
                });
            }
            i += 1;
        }
        Ok(FeatureVector { dims })
    }

    /// 编译期已保证合法的构造（夹具表专用——夹具是 const 字面量，
    /// 由编译期断言逐条钉死在刻度域内）。
    pub const fn new_const(dims: [u8; DIM_COUNT]) -> FeatureVector {
        FeatureVector { dims }
    }

    /// 维 0 等待语义刻度。
    pub const fn wait(&self) -> u8 {
        self.dims[0]
    }
    /// 维 1 事件率刻度。
    pub const fn event(&self) -> u8 {
        self.dims[1]
    }
    /// 维 2 上下文依赖刻度。
    pub const fn ctx(&self) -> u8 {
        self.dims[2]
    }
    /// 维 3 重入风险刻度。
    pub const fn reentry(&self) -> u8 {
        self.dims[3]
    }
    /// 维 4 时序敏感度刻度。
    pub const fn timing(&self) -> u8 {
        self.dims[4]
    }
    /// 维 5 状态耦合刻度。
    pub const fn couple(&self) -> u8 {
        self.dims[5]
    }

    /// 全刻度快照（拷贝出，避免外借内部数组）。
    pub const fn snapshot(&self) -> [u8; DIM_COUNT] {
        self.dims
    }

    /// 加权分 = Σ 权重×刻度（const——判定的确定性核心，零分支外依赖）。
    pub const fn weighted_score(&self) -> u32 {
        self.dims[0] as u32 * W_WAIT
            + self.dims[1] as u32 * W_EVENT
            + self.dims[2] as u32 * W_CTX
            + self.dims[3] as u32 * W_REENTRY
            + self.dims[4] as u32 * W_TIMING
            + self.dims[5] as u32 * W_COUPLE
    }

    /// 特征指纹（FNV 掺入点 id 与六刻度——画像账面绑定用）。
    pub fn fingerprint_with(&self, point_id: u32) -> u64 {
        let mut h: u64 = FNV_OFFSET;
        h = fnv1a64_byte(h, (point_id >> 24) as u8);
        h = fnv1a64_byte(h, (point_id >> 16) as u8);
        h = fnv1a64_byte(h, (point_id >> 8) as u8);
        h = fnv1a64_byte(h, point_id as u8);
        let mut i: usize = 0;
        while i < DIM_COUNT {
            h = fnv1a64_byte(h, self.dims[i]);
            i += 1;
        }
        h
    }

    /// 单维刻度语义描述（越界维返回空串——但构造已拦越界，此处
    /// 只兜 const 直构的极端面）。
    pub fn dim_desc(&self, dim: usize) -> &'static str {
        if dim >= DIM_COUNT {
            return "";
        }
        let lvl = self.dims[dim] as usize;
        if lvl >= 8 {
            return "";
        }
        // 维序静态分派（不用动态二维索引——保持刻度表逐维具名可审）。
        match dim {
            0 => WAIT_SCALE[lvl],
            1 => EVENT_SCALE[lvl],
            2 => CTX_SCALE[lvl],
            3 => REENTRY_SCALE[lvl],
            4 => TIMING_SCALE[lvl],
            _ => COUPLE_SCALE[lvl],
        }
    }
}

// ---------------------------------------------------------------------------
// §6 测绘账（F1283/F0961 测绘点输入——画像的准入门槛）
// ---------------------------------------------------------------------------

/// 测绘来源域——画像只消费这两个域的已测绘点。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SurveyDomain {
    /// F1283 内存域测绘点（内存映射寄存器轮询点）。
    F1283,
    /// F0961 现存轮询栈盘点点（grep+运行期双源交叉落账）。
    F0961,
}

impl SurveyDomain {
    /// 域码（入账与导出用——1=F1283，2=F0961）。
    pub const fn code(self) -> u8 {
        match self {
            SurveyDomain::F1283 => 1,
            SurveyDomain::F0961 => 2,
        }
    }

    /// 域名（导出风险注记用）。
    pub const fn name(self) -> &'static str {
        match self {
            SurveyDomain::F1283 => "F1283",
            SurveyDomain::F0961 => "F0961",
        }
    }

    /// 由域码还原（非法码 None——导出反解面）。
    pub const fn from_code(c: u8) -> Option<SurveyDomain> {
        match c {
            1 => Some(SurveyDomain::F1283),
            2 => Some(SurveyDomain::F0961),
            _ => None,
        }
    }
}

/// 测绘账条目——一个已测绘轮询点的最小事实（id/来源域/落账 tick/
/// 盘点旗标）。旗标位语义：bit0 已 grep 命中、bit1 运行期观测命中、
/// bit2 双源交叉一致（F0961 判据的 grep+运行期双源）。
#[derive(Clone, Debug)]
pub struct SurveyEntry {
    point_id: u32,
    domain: SurveyDomain,
    tick: u64,
    flags: u32,
}

impl SurveyEntry {
    /// 点 id。
    pub fn point_id(&self) -> u32 {
        self.point_id
    }
    /// 来源域。
    pub fn domain(&self) -> SurveyDomain {
        self.domain
    }
    /// 落账 tick。
    pub fn tick(&self) -> u64 {
        self.tick
    }
    /// 盘点旗标。
    pub fn flags(&self) -> u32 {
        self.flags
    }
    /// 双源交叉一致旗（bit2）。
    pub fn cross_confirmed(&self) -> bool {
        self.flags & 0b100 != 0
    }
}

/// 测绘账——画像的准入账本。只 ingest、只追加；封账后只读。
///
/// 点 id 纪律：0 为保留号（账面"未指派"哨兵），ingest 拒收；
/// tick 单调不减（回退带码拒收——测绘账是排期输入，时序乱账
/// 会让"依赖前驱"列失真）。
#[derive(Debug)]
pub struct SurveyLedger {
    entries: Vec<SurveyEntry>,
    last_tick: u64,
    closed: bool,
    rejected_unsurveyed_probe: u32,
}

impl SurveyLedger {
    /// 建空账。
    pub fn new() -> SurveyLedger {
        SurveyLedger {
            entries: Vec::new(),
            last_tick: 0,
            closed: false,
            rejected_unsurveyed_probe: 0,
        }
    }

    /// 账面条目数。
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// 账面是否为空。
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// 最新 tick。
    pub fn last_tick(&self) -> u64 {
        self.last_tick
    }

    /// 是否已封账。
    pub fn is_closed(&self) -> bool {
        self.closed
    }

    /// 被拒收的未测绘探测次数（注入面统计）。
    pub fn rejected_unsurveyed_probe(&self) -> u32 {
        self.rejected_unsurveyed_probe
    }

    /// ingest 一条测绘点。点 id 零号拒收；tick 回退拒收；封账拒收。
    pub fn ingest(
        &mut self,
        point_id: u32,
        domain: SurveyDomain,
        tick: u64,
        flags: u32,
    ) -> Result<(), PollProfError> {
        if self.closed {
            return Err(PollProfError::LedgerClosed);
        }
        if point_id == 0 {
            return Err(PollProfError::NotSurveyed { point_id: 0 });
        }
        if tick < self.last_tick {
            return Err(PollProfError::TickRegress {
                last: self.last_tick,
                got: tick,
            });
        }
        self.last_tick = tick;
        self.entries.push(SurveyEntry {
            point_id,
            domain,
            tick,
            flags,
        });
        Ok(())
    }

    /// 批量 ingest（逐条带码，遇错即断——测绘账是准入账，宁缺勿脏）。
    pub fn ingest_all(
        &mut self,
        rows: &[(u32, SurveyDomain, u64, u32)],
    ) -> Result<(), PollProfError> {
        let mut i: usize = 0;
        while i < rows.len() {
            let r = rows[i];
            self.ingest(r.0, r.1, r.2, r.3)?;
            i += 1;
        }
        Ok(())
    }

    /// 封账（此后只读）。
    pub fn close(&mut self) {
        self.closed = true;
    }

    /// 点是否已测绘。
    pub fn is_surveyed(&self, point_id: u32) -> bool {
        let mut i: usize = 0;
        while i < self.entries.len() {
            if self.entries[i].point_id == point_id {
                return true;
            }
            i += 1;
        }
        false
    }

    /// 取点条目（只读快照）。
    pub fn entry(&self, point_id: u32) -> Option<SurveyEntry> {
        let mut i: usize = 0;
        while i < self.entries.len() {
            if self.entries[i].point_id == point_id {
                return Some(self.entries[i].clone());
            }
            i += 1;
        }
        None
    }

    /// 全部已测绘点 id（升序插排——确定性遍历序，供清册构建）。
    pub fn point_ids_sorted(&self) -> Vec<u32> {
        let mut ids: Vec<u32> = Vec::with_capacity(self.entries.len());
        let mut i: usize = 0;
        while i < self.entries.len() {
            ids.push(self.entries[i].point_id);
            i += 1;
        }
        sort_u32_asc(&mut ids);
        ids
    }
}

/// u32 升序插排（手写——确定性且不依赖泛型排序路径）。
fn sort_u32_asc(v: &mut [u32]) {
    let n = v.len();
    let mut i: usize = 1;
    while i < n {
        let mut j: usize = i;
        while j > 0 && v[j] < v[j - 1] {
            v.swap(j, j - 1);
            j -= 1;
        }
        i += 1;
    }
}

// ---------------------------------------------------------------------------
// §7 难度档位（Tier）——三档闭集
// ---------------------------------------------------------------------------

/// 接管难度三档闭集。
///
/// 档位语义：Easy=行为等价性可逐行目验的机械改造（Easy 档先接进
/// F0963 IrqBackend 面）；Medium=需双模对账保障的常规改造（F0965
/// Draining 排空路径）；Hard=时序/重入/耦合任一顶格的高危改造
/// （排期最后、对账密度最高——F0969 按档抽样）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tier {
    /// Easy——低危机械改造。
    Easy,
    /// Medium——需对账保障的常规改造。
    Medium,
    /// Hard——高危改造，排期最后。
    Hard,
}

impl Tier {
    /// 档位码（1=Easy 2=Medium 3=Hard——清册导出与对拍用）。
    pub const fn code(self) -> u8 {
        match self {
            Tier::Easy => 1,
            Tier::Medium => 2,
            Tier::Hard => 3,
        }
    }

    /// 档位序（排期排序键：Easy 先接）。
    pub const fn rank(self) -> u32 {
        self.code() as u32
    }

    /// 档位中文名。
    pub const fn name_cn(self) -> &'static str {
        match self {
            Tier::Easy => "Easy",
            Tier::Medium => "Medium",
            Tier::Hard => "Hard",
        }
    }

    /// 档位接管策略注记（清册风险列引用的定值文案）。
    pub const fn policy(self) -> &'static str {
        match self {
            Tier::Easy => "先接线(F0963)·逐行目验",
            Tier::Medium => "双模对账(F0965)·常规排期",
            Tier::Hard => "排期最后·高密度对账(F0969)",
        }
    }

    /// 由档位码还原（非法码 None）。
    pub const fn from_code(c: u8) -> Option<Tier> {
        match c {
            1 => Some(Tier::Easy),
            2 => Some(Tier::Medium),
            3 => Some(Tier::Hard),
            _ => None,
        }
    }
}

// ---------------------------------------------------------------------------
// §8 判级器（DifficultyGrader）——加权记分 + 强制抬档 + 可解释理由
// ---------------------------------------------------------------------------

/// 判定结论——一次判级的全部输出（档位/得分/理由/强制命中数/指纹）。
///
/// 理由是**逐维可解释**的：每维一行"刻度+语义+权重+贡献"，再追加
/// 命中的强制规则行——验收"分级正确"时人能顺着理由复核到刻度表。
#[derive(Clone, Debug)]
pub struct GradeVerdict {
    point_id: u32,
    tier: Tier,
    score: u32,
    reasons: Vec<String>,
    force_hits: u32,
    fp: u64,
}

impl GradeVerdict {
    /// 点 id。
    pub fn point_id(&self) -> u32 {
        self.point_id
    }
    /// 档位。
    pub fn tier(&self) -> Tier {
        self.tier
    }
    /// 加权分。
    pub fn score(&self) -> u32 {
        self.score
    }
    /// 可解释理由行（逐维 + 强制规则）。
    pub fn reasons(&self) -> &[String] {
        &self.reasons
    }
    /// 强制抬档命中数。
    pub fn force_hits(&self) -> u32 {
        self.force_hits
    }
    /// 结论指纹（掺档位码+得分+理由行数——对拍用）。
    pub fn fingerprint(&self) -> u64 {
        self.fp
    }

    /// 结论逐字段相等（对拍的"一致"定义：档位、得分、理由行数、
    /// 指纹四者全等——理由文本由刻度+权重决定，刻度相同则必同）。
    pub fn same_as(&self, other: &GradeVerdict) -> bool {
        self.point_id == other.point_id
            && self.tier == other.tier
            && self.score == other.score
            && self.reasons.len() == other.reasons.len()
            && self.fp == other.fp
    }
}

/// 判级器——持阈值（权重取 const 表，不持可变权重：权重是判据的
/// 刻度来源，篡改权重即篡改判据，不设入口）。
///
/// 阈值可覆盖入口存在但全带码校验（Easy 上限 < Medium 上限 ≤
/// SCORE_MAX）——这是"判定不一致注入"的检测靶：被篡改阈值的
/// 判级器与基准判级器双跑，分歧行当场落账。
#[derive(Clone, Debug)]
pub struct DifficultyGrader {
    easy_max: u32,
    med_max: u32,
}

impl DifficultyGrader {
    /// 基准判级器（const 阈值表）。
    pub fn new() -> DifficultyGrader {
        DifficultyGrader {
            easy_max: TIER_EASY_MAX,
            med_max: TIER_MED_MAX,
        }
    }

    /// 阈值覆盖判级器（带码校验——非法阈值拒收）。
    pub fn with_thresholds(
        easy_max: u32,
        med_max: u32,
    ) -> Result<DifficultyGrader, PollProfError> {
        if easy_max >= med_max || med_max > SCORE_MAX {
            return Err(PollProfError::ThresholdInvalid { easy_max, med_max });
        }
        Ok(DifficultyGrader { easy_max, med_max })
    }

    /// Easy 上限。
    pub fn easy_max(&self) -> u32 {
        self.easy_max
    }
    /// Medium 上限。
    pub fn med_max(&self) -> u32 {
        self.med_max
    }

    /// 强制抬档规则（const 判定——命中即抬档，理由可解释）。
    ///
    /// 规则闭集（刻度语义来源 §1 表）：
    /// R1 等待语义 ≥5（密集忙等起）→ 至少 Medium——忙等改中断
    ///     行为等价性必须对账；
    /// R2 等待语义 =7（全程忙等硬窗）→ Hard——硬窗改造错过即败；
    /// R3 重入风险 ≥6（活锁/递归重入史）→ 至少 Medium——并发面
    ///     改造必须双模对账；
    /// R4 状态耦合 ≥6 且上下文依赖 ≥5（跨域双向+高上下文限定）→
    ///     至少 Medium——跨域耦合改造牵连面大；
    /// R5 等待语义 ≥5 且时序敏感 ≥5（忙等+紧窗）→ Hard——
    ///     忙等改中断在紧窗下最易破时序。
    /// 返回 (抬到的档, 命中规则号位图)。
    pub const fn force_tier(&self, f: &FeatureVector) -> (Tier, u32) {
        let mut tier = Tier::Easy;
        let mut hits: u32 = 0;
        // R1
        if f.wait() >= 5 {
            if tier_rank_lt(tier, Tier::Medium) {
                tier = Tier::Medium;
            }
            hits |= 0b00001;
        }
        // R2
        if f.wait() == 7 {
            if tier_rank_lt(tier, Tier::Hard) {
                tier = Tier::Hard;
            }
            hits |= 0b00010;
        }
        // R3
        if f.reentry() >= 6 {
            if tier_rank_lt(tier, Tier::Medium) {
                tier = Tier::Medium;
            }
            hits |= 0b00100;
        }
        // R4
        if f.couple() >= 6 && f.ctx() >= 5 {
            if tier_rank_lt(tier, Tier::Medium) {
                tier = Tier::Medium;
            }
            hits |= 0b01000;
        }
        // R5
        if f.wait() >= 5 && f.timing() >= 5 {
            if tier_rank_lt(tier, Tier::Hard) {
                tier = Tier::Hard;
            }
            hits |= 0b10000;
        }
        (tier, hits)
    }

    /// 判级主入口（确定性——输入只消费特征向量，输出只由刻度+
    /// const 权重+阈值决定，零随机零墙钟）。
    pub fn grade(&self, point_id: u32, f: &FeatureVector) -> GradeVerdict {
        let score = f.weighted_score();
        // 基础档：阈值分档（score ≤700 由构造保证，无溢出面）。
        let base = if score <= self.easy_max {
            Tier::Easy
        } else if score <= self.med_max {
            Tier::Medium
        } else {
            Tier::Hard
        };
        // 强制抬档：取规则档与基础档的较高者。
        let (forced, hits) = self.force_tier(f);
        let tier = if tier_rank_lt(base, forced) {
            forced
        } else {
            base
        };
        // 逐维可解释理由。
        let mut reasons: Vec<String> = Vec::with_capacity(DIM_COUNT + 2);
        let ws = [f.wait(), f.event(), f.ctx(), f.reentry(), f.timing(), f.couple()];
        let mut i: usize = 0;
        while i < DIM_COUNT {
            reasons.push(format!(
                "{}={}({}) 权重{} 贡献{}",
                DIM_NAMES[i],
                ws[i],
                f.dim_desc(i),
                WEIGHTS[i],
                WEIGHTS[i] * ws[i] as u32
            ));
            i += 1;
        }
        reasons.push(format!(
            "加权分={} 阈值档={} 强制抬档后={}",
            score,
            base.name_cn(),
            tier.name_cn()
        ));
        if hits != 0 {
            reasons.push(force_rule_text(hits));
        }
        // 指纹：掺 id/档位码/得分/理由行数（对拍四字段之一）。
        let mut h: u64 = FNV_OFFSET;
        h = fnv1a64_byte(h, (point_id >> 24) as u8);
        h = fnv1a64_byte(h, (point_id >> 16) as u8);
        h = fnv1a64_byte(h, (point_id >> 8) as u8);
        h = fnv1a64_byte(h, point_id as u8);
        h = fnv1a64_byte(h, tier.code());
        h = fnv1a64_byte(h, (score >> 8) as u8);
        h = fnv1a64_byte(h, score as u8);
        h = fnv1a64_byte(h, reasons.len() as u8);
        GradeVerdict {
            point_id,
            tier,
            score,
            reasons,
            force_hits: hits.count_ones(),
            fp: h,
        }
    }
}

/// 档位序比较（const——不用 derive 比较与 Ord::min/max）。
const fn tier_rank_lt(a: Tier, b: Tier) -> bool {
    a.rank() < b.rank()
}

/// 强制规则命中位图 → 人话规则行（可解释输出用）。
fn force_rule_text(hits: u32) -> String {
    let mut s = String::from("强制规则命中:");
    if hits & 0b00001 != 0 {
        s.push_str(" R1忙等起抬Medium");
    }
    if hits & 0b00010 != 0 {
        s.push_str(" R2硬窗抬Hard");
    }
    if hits & 0b00100 != 0 {
        s.push_str(" R3重入史抬Medium");
    }
    if hits & 0b01000 != 0 {
        s.push_str(" R4跨域耦合抬Medium");
    }
    if hits & 0b10000 != 0 {
        s.push_str(" R5忙等+紧窗抬Hard");
    }
    s
}

// ---------------------------------------------------------------------------
// §9 判据自证夹具——三档各 10 条固定刻度向量（30/30）
//
// 夹具来源：按 §1 刻度表逐级构造的代表性刻度向量，每条钉死预期档；
// 阈值/权重的任何漂移都会让 10/10 自证碎掉（夹具即判据的活断言）。
// 每条刻度均在 0..=7 域内（编译期逐条断言兜底）。
// ---------------------------------------------------------------------------

/// Easy 夹具 ×10（预期全部判 Easy——低分且零强制规则）。
pub const FIXTURES_EASY: [[u8; DIM_COUNT]; 10] = [
    [1, 0, 0, 0, 0, 0], // 25  分
    [2, 1, 0, 0, 1, 0], // 80  分
    [3, 1, 1, 0, 1, 1], // 130 分
    [2, 2, 1, 1, 1, 1], // 135 分
    [4, 1, 0, 0, 2, 0], // 150 分
    [3, 2, 2, 0, 2, 1], // 170 分
    [1, 0, 1, 1, 0, 1], // 70  分
    [2, 0, 2, 2, 0, 0], // 110 分
    [4, 1, 1, 0, 2, 1], // 175 分
    [3, 0, 1, 2, 1, 2], // 175 分
];

/// Medium 夹具 ×10（预期全部判 Medium——中分或强制抬 Medium）。
pub const FIXTURES_MEDIUM: [[u8; DIM_COUNT]; 10] = [
    [5, 2, 2, 3, 3, 2], // 315 分（R1 抬 Medium）
    [6, 2, 3, 3, 3, 2], // 350 分（R1 抬 Medium）
    [4, 3, 3, 4, 4, 3], // 365 分
    [5, 3, 2, 4, 4, 3], // 380 分（R1）
    [4, 2, 3, 4, 4, 3], // 355 分
    [6, 1, 2, 5, 2, 2], // 350 分（R1）
    [5, 1, 1, 3, 2, 4], // 305 分（R1）
    [6, 2, 2, 2, 3, 2], // 320 分（R1）
    [4, 4, 4, 3, 4, 3], // 365 分
    [5, 2, 3, 4, 3, 3], // 360 分（R1）
];

/// Hard 夹具 ×10（预期全部判 Hard——高分或强制抬 Hard）。
pub const FIXTURES_HARD: [[u8; DIM_COUNT]; 10] = [
    [6, 3, 4, 5, 5, 4], // 480 分（R5 抬 Hard）
    [5, 4, 4, 5, 5, 4], // 465 分（R5）
    [7, 3, 3, 4, 4, 3], // 440 分（R2 抬 Hard）
    [4, 3, 5, 6, 3, 4], // 420 分
    [4, 2, 6, 5, 4, 6], // 450 分
    [6, 4, 5, 4, 6, 5], // 515 分
    [5, 3, 5, 6, 4, 5], // 480 分
    [7, 2, 4, 5, 5, 4], // 495 分（R2+R5）
    [4, 5, 4, 4, 6, 4], // 450 分
    [6, 5, 4, 5, 4, 6], // 510 分
];

// 编译期兜底：夹具刻度全部落在 0..=7 域内（逐维逐条）。
const _: () = {
    let mut i = 0;
    while i < 10 {
        let mut d = 0;
        while d < DIM_COUNT {
            assert!(FIXTURES_EASY[i][d] <= 7);
            assert!(FIXTURES_MEDIUM[i][d] <= 7);
            assert!(FIXTURES_HARD[i][d] <= 7);
            d += 1;
        }
        i += 1;
    }
};

/// 三档夹具汇总（按 Easy/Medium/Hard 序）。
pub const FIXTURES_ALL: [[[u8; DIM_COUNT]; 10]; 3] =
    [FIXTURES_EASY, FIXTURES_MEDIUM, FIXTURES_HARD];

/// 判据自证——三档各判 10 次，返回各档判定对数（应全为 10）。
///
/// 这是判据 J1"三档难度各自 10/10 次判定对"的机检本体：夹具
/// 预期档由 const 表钉死，判级器输出与预期逐条对。
pub fn self_cert_ten_ten(grader: &DifficultyGrader) -> [u32; 3] {
    let mut out = [0u32; 3];
    let mut t: usize = 0;
    while t < 3 {
        let expect: Tier = match t {
            0 => Tier::Easy,
            1 => Tier::Medium,
            _ => Tier::Hard,
        };
        let mut i: usize = 0;
        while i < 10 {
            let f = FeatureVector::new_const(FIXTURES_ALL[t][i]);
            let v = grader.grade(1000 + (t * 10 + i) as u32, &f);
            if v.tier == expect {
                out[t] = out[t].saturating_add(1);
            }
            i += 1;
        }
        t += 1;
    }
    out
}

// ---------------------------------------------------------------------------
// §10 轮询点特征画像（PollSiteProfile）
// ---------------------------------------------------------------------------

/// 轮询点特征画像——一个已测绘点的六维刻度 + 落账事实。
///
/// 画像只从测绘账准入点构造（未测绘点带码拒收）；构造即指纹封存
/// （id+六刻度 FNV），画像内容不可变——改刻度等于重新画像，账面
/// 留痕（事件账 ProfileBuilt 行）。
#[derive(Clone, Debug)]
pub struct PollSiteProfile {
    point_id: u32,
    domain: SurveyDomain,
    feat: FeatureVector,
    profiled_at_tick: u64,
    fp: u64,
}

impl PollSiteProfile {
    /// 点 id。
    pub fn point_id(&self) -> u32 {
        self.point_id
    }
    /// 测绘来源域。
    pub fn domain(&self) -> SurveyDomain {
        self.domain
    }
    /// 特征向量快照。
    pub fn feature(&self) -> FeatureVector {
        self.feat
    }
    /// 画像落账 tick。
    pub fn profiled_at_tick(&self) -> u64 {
        self.profiled_at_tick
    }
    /// 画像指纹。
    pub fn fingerprint(&self) -> u64 {
        self.fp
    }
    /// 六维画像一句话（解释面——人话输出）。
    pub fn describe(&self) -> String {
        format!(
            "点{}({}) 六维[等待{} 事件{} 上下文{} 重入{} 时序{} 耦合{}] 加权分{}",
            self.point_id,
            self.domain.name(),
            self.feat.wait(),
            self.feat.event(),
            self.feat.ctx(),
            self.feat.reentry(),
            self.feat.timing(),
            self.feat.couple(),
            self.feat.weighted_score()
        )
    }
}

// ---------------------------------------------------------------------------
// §11 依赖前驱账 + 分级清册导出（TakeoverScheduleBook）
// ---------------------------------------------------------------------------

/// 依赖前驱边——(前驱点, 后继点)：前驱未接管完，后继不得动工。
///
/// 来源：轮询点间的硬件序（如时钟轮询先于设备轮询）与账面序
/// （F0965 排空序）。清册"依赖前驱"列逐行落此账。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DepEdge {
    /// 前驱点 id。
    pub pred: u32,
    /// 后继点 id。
    pub point: u32,
}

impl DepEdge {
    /// 构造一条依赖边。
    pub const fn new(pred: u32, point: u32) -> DepEdge {
        DepEdge { pred, point }
    }
}

/// 分级清册一行——四列：点 id / 难度档 / 依赖前驱 / 风险注记。
///
/// 四列齐备是"接管排期输入齐备"判据的机检本体（列缺失带码拒导）。
#[derive(Clone, Debug)]
pub struct ScheduleRow {
    /// 列 1：点 id。
    pub point_id: u32,
    /// 列 2：难度档。
    pub tier: Tier,
    /// 列 3：依赖前驱（升序、去重；空集导 "-"）。
    pub deps: Vec<u32>,
    /// 列 4：风险注记（档位策略 + 顶维 + 强制规则摘要）。
    pub risk_note: String,
}

impl ScheduleRow {
    /// 列齐备机检（四列任一空 → Err 带缺列名）。
    pub fn check_complete(&self) -> Result<(), PollProfError> {
        if self.point_id == 0 {
            return Err(PollProfError::ScheduleIncomplete {
                row: 0,
                column: "点id",
            });
        }
        // 档位闭集自证（Tier 只有三档，此恒真——保留作列完备性的
        // 显性检查点）。
        if Tier::from_code(self.tier.code()).is_none() {
            return Err(PollProfError::ScheduleIncomplete {
                row: 0,
                column: "难度档",
            });
        }
        if self.risk_note.is_empty() {
            return Err(PollProfError::ScheduleIncomplete {
                row: 0,
                column: "风险注记",
            });
        }
        let mut i: usize = 0;
        while i < self.deps.len() {
            if self.deps[i] == 0 {
                return Err(PollProfError::ScheduleIncomplete {
                    row: 0,
                    column: "依赖前驱",
                });
            }
            i += 1;
        }
        Ok(())
    }
}

/// 行排序键：(档位序, 点 id)——导出序确定性（Easy 先接、同档按点号）。
fn row_sort_key(r: &ScheduleRow) -> (u32, u32) {
    (r.tier.rank(), r.point_id)
}

/// 清册行插排（手写交换插排——比较器全序、确定性、零泛型排序路径）。
fn sort_schedule_rows(rows: &mut [ScheduleRow]) {
    let n = rows.len();
    let mut i: usize = 1;
    while i < n {
        let mut j: usize = i;
        while j > 0 {
            let a = row_sort_key(&rows[j]);
            let b = row_sort_key(&rows[j - 1]);
            if a < b {
                rows.swap(j, j - 1);
                j -= 1;
            } else {
                break;
            }
        }
        i += 1;
    }
}

/// 分级清册（接管排期输入）——行集 + 导出指纹 + 导出次数。
///
/// 导出 schema（v1 冻结）：
/// ```text
/// UNX-F0962-SCHED|v1|rows=<N>|fp=<hex16>
/// P<06d>|<tiercode>|<deps 由逗号连或 ->|<风险注记>
/// ...
/// ```
/// 首行 schema 头（对齐 F0854 导出面纪律），行四列竖线分隔——
/// 列数机检 3 根竖线/行，缺列即判据"排期输入齐备"不达标。
#[derive(Debug)]
pub struct TakeoverScheduleBook {
    rows: Vec<ScheduleRow>,
    export_fp: u64,
    export_count: u32,
}

impl TakeoverScheduleBook {
    /// 行数。
    pub fn len(&self) -> usize {
        self.rows.len()
    }

    /// 是否为空。
    pub fn is_empty(&self) -> bool {
        self.rows.is_empty()
    }

    /// 行只读快照（导出序）。
    pub fn rows(&self) -> &[ScheduleRow] {
        &self.rows
    }

    /// 最近导出指纹。
    pub fn export_fingerprint(&self) -> u64 {
        self.export_fp
    }

    /// 导出次数。
    pub fn export_count(&self) -> u32 {
        self.export_count
    }

    /// 导出为 schema v1 文本（确定性序 + 列齐备机检 + 指纹封存）。
    ///
    /// 双导出指纹对拍：第二次导出与首次指纹必同（导出序确定性
    /// 自证），失配带码 0x1A18。
    pub fn export(&mut self) -> Result<String, PollProfError> {
        // ① 列齐备机检（逐行四列）。
        let mut i: usize = 0;
        while i < self.rows.len() {
            if let Err(mut e) = self.rows[i].check_complete() {
                if let PollProfError::ScheduleIncomplete { row, .. } = &mut e {
                    *row = i;
                }
                return Err(e);
            }
            i += 1;
        }
        // ② 拼装（导出序已由构建期插排固定——此处按序直拼）。
        let mut out = String::new();
        out.push_str("UNX-F0962-SCHED|v1|rows=");
        out.push_str(&self.rows.len().to_string());
        // 指纹先算（schema 头要带 fp——fp 掺全行文本，故头行 fp 位
        // 用行体指纹，行体再拼一次头行不带 fp 的规范形，两段拼合
        // 仍是确定性的）。
        let body_fp = self.body_fingerprint();
        out.push_str("|fp=");
        out.push_str(&hex16(body_fp));
        out.push('\n');
        i = 0;
        while i < self.rows.len() {
            let r = &self.rows[i];
            out.push('P');
            out.push_str(&pad6(r.point_id));
            out.push('|');
            out.push_str(&r.tier.code().to_string());
            out.push('|');
            if r.deps.is_empty() {
                out.push('-');
            } else {
                let mut d: usize = 0;
                while d < r.deps.len() {
                    if d > 0 {
                        out.push(',');
                    }
                    out.push_str(&r.deps[d].to_string());
                    d += 1;
                }
            }
            out.push('|');
            out.push_str(&r.risk_note);
            out.push('\n');
            i += 1;
        }
        // ③ 双导出对拍（导出序确定性判据）。
        let fp = fnv1a64(out.as_bytes());
        if self.export_count > 0 && fp != self.export_fp {
            return Err(PollProfError::ExportDrift {
                first: self.export_fp,
                second: fp,
            });
        }
        self.export_fp = fp;
        self.export_count = self.export_count.saturating_add(1);
        Ok(out)
    }

    /// 行体规范形指纹（行文本逐行掺入——schema 头 fp 位与双导出
    /// 对拍共用此指纹）。
    fn body_fingerprint(&self) -> u64 {
        let mut h: u64 = FNV_OFFSET;
        let mut i: usize = 0;
        while i < self.rows.len() {
            let r = &self.rows[i];
            h = fnv1a64_byte(h, (r.point_id >> 24) as u8);
            h = fnv1a64_byte(h, (r.point_id >> 16) as u8);
            h = fnv1a64_byte(h, (r.point_id >> 8) as u8);
            h = fnv1a64_byte(h, r.point_id as u8);
            h = fnv1a64_byte(h, r.tier.code());
            h = fnv1a64_byte(h, r.deps.len() as u8);
            let mut d: usize = 0;
            while d < r.deps.len() {
                h = fnv1a64_byte(h, (r.deps[d] >> 24) as u8);
                h = fnv1a64_byte(h, (r.deps[d] >> 16) as u8);
                h = fnv1a64_byte(h, (r.deps[d] >> 8) as u8);
                h = fnv1a64_byte(h, r.deps[d] as u8);
                d += 1;
            }
            h = fnv1a64_byte(h, r.risk_note.len() as u8);
            let nb: &[u8] = r.risk_note.as_bytes();
            let mut bi: usize = 0;
            while bi < nb.len() {
                h = fnv1a64_byte(h, nb[bi]);
                bi += 1;
            }
            i += 1;
        }
        h
    }
}

/// 六位补零点 id（清册行格式）。
fn pad6(v: u32) -> String {
    let s = v.to_string();
    let n = s.len();
    let mut out = String::new();
    let mut i = n;
    while i < 6 {
        out.push('0');
        i += 1;
    }
    out.push_str(&s);
    out
}

/// u64 十六进制 16 位小写（指纹文本格式）。
fn hex16(v: u64) -> String {
    const HEXD: [&str; 16] = [
        "0", "1", "2", "3", "4", "5", "6", "7", "8", "9", "a", "b", "c", "d", "e", "f",
    ];
    let mut out = String::with_capacity(16);
    let mut i: u32 = 0;
    while i < 16 {
        let nib = ((v >> (60 - i * 4)) & 0xF) as usize;
        out.push_str(HEXD[nib]);
        i += 1;
    }
    out
}

// ---------------------------------------------------------------------------
// §12 事件账（环形——账面纪律与注入检出落账）
// ---------------------------------------------------------------------------

/// 账面事件种闭集。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EventKind {
    /// 测绘点 ingest 成功。
    SurveyIngested,
    /// 画像建成。
    ProfileBuilt,
    /// 判级完成。
    GradeDone,
    /// 双跑对拍一致。
    PairConsistent,
    /// 双跑对拍分歧（不一致检出——注入面三的检出行）。
    PairDivergent,
    /// 注入被拦（未测绘/越界/非法依赖——注入面的拦截行）。
    InjectBlocked,
    /// 清册导出完成。
    ExportDone,
    /// 拒收（带码——未测绘/越界等准入失败统一落此行）。
    Rejected,
    /// 封账。
    LedgerSealed,
}

impl EventKind {
    /// 事件码（账面与导出用）。
    pub const fn code(self) -> u8 {
        match self {
            EventKind::SurveyIngested => 1,
            EventKind::ProfileBuilt => 2,
            EventKind::GradeDone => 3,
            EventKind::PairConsistent => 4,
            EventKind::PairDivergent => 5,
            EventKind::InjectBlocked => 6,
            EventKind::ExportDone => 7,
            EventKind::Rejected => 8,
            EventKind::LedgerSealed => 9,
        }
    }

    /// 事件名（人话账面）。
    pub const fn name(self) -> &'static str {
        match self {
            EventKind::SurveyIngested => "测绘落账",
            EventKind::ProfileBuilt => "画像建成",
            EventKind::GradeDone => "判级完成",
            EventKind::PairConsistent => "双跑一致",
            EventKind::PairDivergent => "双跑分歧",
            EventKind::InjectBlocked => "注入被拦",
            EventKind::ExportDone => "清册导出",
            EventKind::Rejected => "准入拒收",
            EventKind::LedgerSealed => "封账",
        }
    }
}

/// 一行账面事件（种/tick/参——arg 语义随种而定：Rejected/InjectBlocked
/// 为诊断码，其余为点 id 低 32 位）。
#[derive(Clone, Copy, Debug)]
pub struct LedgerEvent {
    kind: EventKind,
    tick: u64,
    arg: u32,
}

impl LedgerEvent {
    /// 事件种。
    pub fn kind(&self) -> EventKind {
        self.kind
    }
    /// 事件 tick。
    pub fn tick(&self) -> u64 {
        self.tick
    }
    /// 事件参。
    pub fn arg(&self) -> u32 {
        self.arg
    }
}

/// 环形事件账——容量定容、覆盖最旧、全程计数（账体有界、历史无界）。
///
/// 环容量定值 256：来源为域内单批验收观测窗口规模（一次排期评审
/// 的账面行量级），覆盖最旧行省内存、total 计数保完整率可验——
/// 与 F0907 审计黑匣子同一纪律（有界内存、无界计数）。
pub const EVENT_RING_CAP: usize = 256;

/// 环形事件账本体。
#[derive(Debug)]
pub struct EventLedger {
    ring: Vec<LedgerEvent>,
    head: usize,
    total: u64,
    per_kind: [u64; 9],
}

impl EventLedger {
    /// 建账（容量非法拒收）。
    pub fn new(cap: usize) -> Result<EventLedger, PollProfError> {
        if cap == 0 {
            return Err(PollProfError::RingCapacity { got: cap });
        }
        Ok(EventLedger {
            ring: Vec::with_capacity(cap),
            head: 0,
            total: 0,
            per_kind: [0; 9],
        })
    }

    /// 容量。
    pub fn cap(&self) -> usize {
        self.ring.capacity().max(1)
    }

    /// 环内现存行数。
    pub fn live(&self) -> usize {
        self.ring.len()
    }

    /// 全程计数（含已被覆盖的行）。
    pub fn total(&self) -> u64 {
        self.total
    }

    /// 按种计数（闭集九种，越界码恒 0）。
    pub fn count_of(&self, k: EventKind) -> u64 {
        let c = k.code() as usize;
        if c == 0 || c > 9 {
            return 0;
        }
        self.per_kind[c - 1]
    }

    /// 落一行账。
    pub fn record(&mut self, kind: EventKind, tick: u64, arg: u32) {
        let ev = LedgerEvent { kind, tick, arg };
        if self.ring.len() < self.ring.capacity() {
            self.ring.push(ev);
        } else {
            // 覆盖最旧：head 指向最旧位。
            self.ring[self.head] = ev;
        }
        self.head = (self.head + 1) % self.ring.capacity().max(1);
        self.total = self.total.wrapping_add(1);
        let c = kind.code() as usize;
        if c >= 1 && c <= 9 {
            self.per_kind[c - 1] = self.per_kind[c - 1].wrapping_add(1);
        }
    }

    /// 环内按时间序快照（最旧→最新）。
    pub fn snapshot(&self) -> Vec<LedgerEvent> {
        let cap = self.ring.capacity().max(1);
        let mut out: Vec<LedgerEvent> = Vec::with_capacity(self.ring.len());
        if self.ring.len() < cap {
            let mut i: usize = 0;
            while i < self.ring.len() {
                out.push(self.ring[i]);
                i += 1;
            }
        } else {
            let mut i: usize = 0;
            while i < cap {
                out.push(self.ring[(self.head + i) % cap]);
                i += 1;
            }
        }
        out
    }
}

// ---------------------------------------------------------------------------
// §13 双跑对拍（判定可重复判据的机检本体）
// ---------------------------------------------------------------------------

/// 双跑对拍结论。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PairOutcome {
    /// 一致——档位/得分/理由行数/指纹四字段全等。
    Consistent {
        /// 点 id。
        point_id: u32,
        /// 一致档位。
        tier: Tier,
    },
    /// 分歧——不一致检出（注入面三的检出形态）。
    Divergent {
        /// 点 id。
        point_id: u32,
        /// 首跑档位码。
        tier_a: u8,
        /// 次跑档位码。
        tier_b: u8,
        /// 首跑得分。
        score_a: u32,
        /// 次跑得分。
        score_b: u32,
    },
}

impl PairOutcome {
    /// 是否一致。
    pub fn is_consistent(&self) -> bool {
        match self {
            PairOutcome::Consistent { .. } => true,
            PairOutcome::Divergent { .. } => false,
        }
    }

    /// 点 id。
    pub fn point_id(&self) -> u32 {
        match self {
            PairOutcome::Consistent { point_id, .. } => *point_id,
            PairOutcome::Divergent { point_id, .. } => *point_id,
        }
    }
}

/// 双跑对拍——同一画像对两台判级器各判一次，逐字段对拍。
///
/// 判定可重复判据的机检本体：基准判级器 vs 基准判级器（双跑）必
/// 一致（确定性自证）；基准 vs 阈值篡改器分歧必被检出（注入面三）。
pub fn run_pair(
    a: &DifficultyGrader,
    b: &DifficultyGrader,
    prof: &PollSiteProfile,
) -> PairOutcome {
    let va = a.grade(prof.point_id(), &prof.feature());
    let vb = b.grade(prof.point_id(), &prof.feature());
    if va.same_as(&vb) {
        PairOutcome::Consistent {
            point_id: prof.point_id(),
            tier: va.tier(),
        }
    } else {
        PairOutcome::Divergent {
            point_id: prof.point_id(),
            tier_a: va.tier().code(),
            tier_b: vb.tier().code(),
            score_a: va.score(),
            score_b: vb.score(),
        }
    }
}

// ---------------------------------------------------------------------------
// §14 引擎门面（PollProfEngine）——画像/判级/清册/注入/健康一册
// ---------------------------------------------------------------------------

/// 注入检出记录——一次注入面探测的结果行。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InjectionOutcome {
    /// 注入种（1 未测绘点 / 2 特征越界 / 3 判定不一致）。
    pub kind: u8,
    /// 是否检出（真=拦截或分歧被揪出）。
    pub detected: bool,
    /// 关联诊断码（检出时的码；未检出为 0）。
    pub code: u16,
}

/// 健康快照——引擎一屏自检。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HealthSnapshot {
    /// 测绘点数。
    pub surveyed: u32,
    /// 画像数。
    pub profiles: u32,
    /// 已判级画像数。
    pub graded: u32,
    /// 事件全程计数。
    pub events_total: u64,
    /// 注入探测总数。
    pub injections_total: u32,
    /// 注入检出数（应等于探测数——注入全检出才健康）。
    pub injections_detected: u32,
    /// 双跑对拍次数。
    pub pair_runs: u32,
    /// 双跑一致次数。
    pub pair_consistent: u32,
    /// 一致率（基点 bp，万分之一——10000 即 100%）。
    pub consistency_bp: u32,
    /// 健康指纹（统计面 FNV——快照对拍用）。
    pub fp: u64,
}

impl HealthSnapshot {
    /// 健康判据（三条件全真才健康）：
    /// ① 注入全检出（detected == total）；
    /// ② 双跑一致率 100%（bp == 10000，或尚无对拍）；
    /// ③ 已判级画像 == 画像总数（画像账无漏判）。
    pub fn is_healthy(&self) -> bool {
        self.injections_detected == self.injections_total
            && (self.pair_runs == 0 || self.consistency_bp == 10_000)
            && self.graded == self.profiles
    }
}

/// 引擎门面——测绘账、画像账、判级器、事件账、注入面、健康面一册。
///
/// tick 纪律：全账零墙钟，tick 由调用方显性注入（advance_tick）；
/// 判定路径不消费 tick（判定可重复的先决条件——时间不入判定）。
#[derive(Debug)]
pub struct PollProfEngine {
    survey: SurveyLedger,
    profiles: Vec<PollSiteProfile>,
    grades: Vec<GradeVerdict>,
    grader: DifficultyGrader,
    events: EventLedger,
    tick: u64,
    injections_total: u32,
    injections_detected: u32,
    pair_runs: u32,
    pair_consistent: u32,
}

impl PollProfEngine {
    /// 建引擎（事件账容量取 EVENT_RING_CAP 定值）。
    pub fn new() -> Result<PollProfEngine, PollProfError> {
        Ok(PollProfEngine {
            survey: SurveyLedger::new(),
            profiles: Vec::new(),
            grades: Vec::new(),
            grader: DifficultyGrader::new(),
            events: EventLedger::new(EVENT_RING_CAP)?,
            tick: 0,
            injections_total: 0,
            injections_detected: 0,
            pair_runs: 0,
            pair_consistent: 0,
        })
    }

    /// 推进账面 tick（显性注入——判定路径不消费此值）。
    pub fn advance_tick(&mut self, n: u64) {
        self.tick = self.tick.wrapping_add(n);
    }

    /// 当前 tick。
    pub fn tick(&self) -> u64 {
        self.tick
    }

    /// 测绘账只读访问。
    pub fn survey(&self) -> &SurveyLedger {
        &self.survey
    }

    /// 画像数。
    pub fn profile_count(&self) -> usize {
        self.profiles.len()
    }

    /// 已判级数。
    pub fn grade_count(&self) -> usize {
        self.grades.len()
    }

    /// 事件账只读访问。
    pub fn events(&self) -> &EventLedger {
        &self.events
    }

    /// 判级器只读访问。
    pub fn grader(&self) -> &DifficultyGrader {
        &self.grader
    }

    // ---- 测绘面 ----

    /// ingest 一条测绘点（落账 SurveyIngested 行）。
    pub fn ingest_survey(
        &mut self,
        point_id: u32,
        domain: SurveyDomain,
        flags: u32,
    ) -> Result<(), PollProfError> {
        self.survey.ingest(point_id, domain, self.tick, flags)?;
        self.events
            .record(EventKind::SurveyIngested, self.tick, point_id);
        Ok(())
    }

    /// 批量 ingest。
    pub fn ingest_survey_all(
        &mut self,
        rows: &[(u32, SurveyDomain, u32)],
    ) -> Result<(), PollProfError> {
        let mut i: usize = 0;
        while i < rows.len() {
            let r = rows[i];
            self.ingest_survey(r.0, r.1, r.2)?;
            i += 1;
        }
        Ok(())
    }

    /// 封测绘账（落 LedgerSealed 行）。
    pub fn seal_survey(&mut self) {
        self.survey.close();
        self.events
            .record(EventKind::LedgerSealed, self.tick, self.survey.len() as u32);
    }

    // ---- 画像面 ----

    /// 为已测绘点建画像（未测绘点带码拒收——判据"画像只消费已
    /// 测绘点"的准入本体）。
    ///
    /// 同点重复建档拒收（0x1A15）——画像账一 id 一档，防调用序错
    /// 把账面写脏；改刻度走 reprofile 摘旧建新。
    pub fn profile_point(
        &mut self,
        point_id: u32,
        feat: FeatureVector,
    ) -> Result<(), PollProfError> {
        if !self.survey.is_surveyed(point_id) {
            self.events
                .record(EventKind::Rejected, self.tick, XC_NOT_SURVEYED as u32);
            return Err(PollProfError::NotSurveyed { point_id });
        }
        let mut i: usize = 0;
        while i < self.profiles.len() {
            if self.profiles[i].point_id == point_id {
                self.events.record(
                    EventKind::Rejected,
                    self.tick,
                    XC_PROFILE_DUPLICATE as u32,
                );
                return Err(PollProfError::ProfileDuplicate { point_id });
            }
            i += 1;
        }
        let fp = feat.fingerprint_with(point_id);
        let domain = match self.survey.entry(point_id) {
            Some(e) => e.domain(),
            None => {
                // 不可达面：is_surveyed 已放行；保守带码拒收不 panic。
                self.events
                    .record(EventKind::Rejected, self.tick, XC_NOT_SURVEYED as u32);
                return Err(PollProfError::NotSurveyed { point_id });
            }
        };
        self.profiles.push(PollSiteProfile {
            point_id,
            domain,
            feat,
            profiled_at_tick: self.tick,
            fp,
        });
        self.events
            .record(EventKind::ProfileBuilt, self.tick, point_id);
        Ok(())
    }

    /// 画像快照（按点 id）。
    pub fn profile(&self, point_id: u32) -> Option<PollSiteProfile> {
        let mut i: usize = 0;
        while i < self.profiles.len() {
            if self.profiles[i].point_id == point_id {
                return Some(self.profiles[i].clone());
            }
            i += 1;
        }
        None
    }

    /// 摘除画像与其判级（reprofile 前置）。
    pub fn drop_profile(&mut self, point_id: u32) -> bool {
        let mut idx: Option<usize> = None;
        let mut i: usize = 0;
        while i < self.profiles.len() {
            if self.profiles[i].point_id == point_id {
                idx = Some(i);
                break;
            }
            i += 1;
        }
        match idx {
            Some(i) => {
                self.profiles.swap_remove(i);
                let mut j: usize = 0;
                while j < self.grades.len() {
                    if self.grades[j].point_id() == point_id {
                        self.grades.swap_remove(j);
                        break;
                    }
                    j += 1;
                }
                true
            }
            None => false,
        }
    }

    // ---- 判级面 ----

    /// 判级一幅画像（结论入引擎账，落 GradeDone 行）。
    ///
    /// 同点重判覆盖旧结论（判定是纯函数——重判必同果，覆盖是账面
    /// 收敛不是漂移；分歧由双跑对拍面专责检出）。
    pub fn grade_point(&mut self, point_id: u32) -> Result<GradeVerdict, PollProfError> {
        let prof = match self.profile(point_id) {
            Some(p) => p,
            None => {
                self.events
                    .record(EventKind::Rejected, self.tick, XC_NOT_SURVEYED as u32);
                return Err(PollProfError::NotSurveyed { point_id });
            }
        };
        let v = self.grader.grade(prof.point_id(), &prof.feature());
        let mut replaced = false;
        let mut i: usize = 0;
        while i < self.grades.len() {
            if self.grades[i].point_id() == point_id {
                self.grades[i] = v.clone();
                replaced = true;
                break;
            }
            i += 1;
        }
        if !replaced {
            self.grades.push(v.clone());
        }
        self.events
            .record(EventKind::GradeDone, self.tick, point_id);
        Ok(v)
    }

    /// 全画像判级（漏判清零——排期输入前置）。
    pub fn grade_all(&mut self) -> Result<u32, PollProfError> {
        let ids = {
            let mut ids: Vec<u32> = Vec::with_capacity(self.profiles.len());
            let mut i: usize = 0;
            while i < self.profiles.len() {
                ids.push(self.profiles[i].point_id());
                i += 1;
            }
            sort_u32_asc(&mut ids);
            ids
        };
        let mut n: u32 = 0;
        let mut i: usize = 0;
        while i < ids.len() {
            self.grade_point(ids[i])?;
            n = n.saturating_add(1);
            i += 1;
        }
        Ok(n)
    }

    /// 判级结论（按点 id）。
    pub fn verdict(&self, point_id: u32) -> Option<GradeVerdict> {
        let mut i: usize = 0;
        while i < self.grades.len() {
            if self.grades[i].point_id() == point_id {
                return Some(self.grades[i].clone());
            }
            i += 1;
        }
        None
    }

    /// 全部判级结论（点 id 升序快照）。
    pub fn verdicts_sorted(&self) -> Vec<GradeVerdict> {
        let mut out = self.grades.clone();
        // 按点 id 插排（清册构建的确定性前置）。
        let n = out.len();
        let mut i: usize = 1;
        while i < n {
            let mut j: usize = i;
            while j > 0 && out[j].point_id() < out[j - 1].point_id() {
                out.swap(j, j - 1);
                j -= 1;
            }
            i += 1;
        }
        out
    }

    // ---- 双跑对拍面 ----

    /// 双跑对拍（基准 vs 基准——落 PairConsistent/PairDivergent 行，
    /// 更新一致率统计）。
    pub fn double_grade(&mut self, point_id: u32) -> Result<PairOutcome, PollProfError> {
        let prof = match self.profile(point_id) {
            Some(p) => p,
            None => {
                return Err(PollProfError::NotSurveyed { point_id });
            }
        };
        let base = DifficultyGrader::new();
        let oc = run_pair(&base, &base, &prof);
        self.pair_runs = self.pair_runs.saturating_add(1);
        match &oc {
            PairOutcome::Consistent { .. } => {
                self.pair_consistent = self.pair_consistent.saturating_add(1);
                self.events
                    .record(EventKind::PairConsistent, self.tick, point_id);
            }
            PairOutcome::Divergent { .. } => {
                self.events
                    .record(EventKind::PairDivergent, self.tick, point_id);
            }
        }
        Ok(oc)
    }

    /// 全画像双跑对拍（同点两次判定一致率 100% 判据的批量机检）。
    pub fn double_grade_all(&mut self) -> Result<(u32, u32), PollProfError> {
        let ids = {
            let mut ids: Vec<u32> = Vec::with_capacity(self.profiles.len());
            let mut i: usize = 0;
            while i < self.profiles.len() {
                ids.push(self.profiles[i].point_id());
                i += 1;
            }
            sort_u32_asc(&mut ids);
            ids
        };
        let mut runs: u32 = 0;
        let mut ok: u32 = 0;
        let mut i: usize = 0;
        while i < ids.len() {
            match self.double_grade(ids[i])? {
                PairOutcome::Consistent { .. } => ok = ok.saturating_add(1),
                PairOutcome::Divergent { .. } => {}
            }
            runs = runs.saturating_add(1);
            i += 1;
        }
        Ok((runs, ok))
    }

    // ---- 注入面（三面全检出） ----

    /// 注入面一：未测绘点注入——对账外点请求画像，应被 0x1A11 拦下。
    pub fn inject_unsurveyed_point(&mut self, point_id: u32) -> InjectionOutcome {
        self.injections_total = self.injections_total.saturating_add(1);
        let detected = match self.profile_point(point_id, FeatureVector::new_const([0; DIM_COUNT]))
        {
            Err(e) => {
                let c = e.code();
                self.events
                    .record(EventKind::InjectBlocked, self.tick, c as u32);
                c == XC_NOT_SURVEYED
            }
            Ok(()) => false,
        };
        if detected {
            self.injections_detected = self.injections_detected.saturating_add(1);
        }
        InjectionOutcome {
            kind: 1,
            detected,
            code: if detected { XC_NOT_SURVEYED } else { 0 },
        }
    }

    /// 注入面二：特征越界注入——对已测绘点提交越界刻度，应被
    /// 0x1A12 拦下（画像建不起来，账面不脏）。
    pub fn inject_out_of_range(
        &mut self,
        _point_id: u32,
        dim: usize,
        value: u8,
    ) -> InjectionOutcome {
        self.injections_total = self.injections_total.saturating_add(1);
        // 构造带越界维的向量：仅在目标维替换越界值（其余维取合法
        // 低刻度——保证触发面单一是越界维）。
        let mut dims = [1u8; DIM_COUNT];
        if dim < DIM_COUNT {
            dims[dim] = value;
        } else {
            // 维序越界本身也按越界注入处理（以维 0 承载非法值触发）。
            dims[0] = value;
        }
        let detected = match FeatureVector::new(dims) {
            Err(e) => {
                let c = e.code();
                self.events
                    .record(EventKind::InjectBlocked, self.tick, c as u32);
                c == XC_FEAT_OUT_OF_RANGE
            }
            Ok(_) => {
                // 向量合法（value ≤7）——真实触发 engine 校验路径：
                // 已测绘点合法建档应成功，但这不算注入检出。
                false
            }
        };
        if detected {
            self.injections_detected = self.injections_detected.saturating_add(1);
        }
        InjectionOutcome {
            kind: 2,
            detected,
            code: if detected {
                XC_FEAT_OUT_OF_RANGE
            } else {
                0
            },
        }
    }

    /// 注入面三：判定不一致注入——用阈值篡改器对拍基准器，分歧必
    /// 被 0x1A13 揪出（判级路径可重复性的对抗自证）。
    ///
    /// 篡改器阈值非法本身也会被 with_thresholds 拒收（0x1A19）——
    /// 两种形态都算检出。
    pub fn inject_inconsistency(
        &mut self,
        point_id: u32,
        tampered_easy: u32,
        tampered_med: u32,
    ) -> InjectionOutcome {
        self.injections_total = self.injections_total.saturating_add(1);
        let prof = match self.profile(point_id) {
            Some(p) => p,
            None => {
                // 靶点不存在也算未检出（注入前提不成立）。
                self.events
                    .record(EventKind::Rejected, self.tick, XC_NOT_SURVEYED as u32);
                return InjectionOutcome {
                    kind: 3,
                    detected: false,
                    code: 0,
                };
            }
        };
        let tampered = match DifficultyGrader::with_thresholds(tampered_easy, tampered_med) {
            Ok(g) => g,
            Err(e) => {
                // 非法阈值在判级器构造面即被拦（阈值守门先行）。
                let c = e.code();
                self.events
                    .record(EventKind::InjectBlocked, self.tick, c as u32);
                self.injections_detected = self.injections_detected.saturating_add(1);
                return InjectionOutcome {
                    kind: 3,
                    detected: true,
                    code: c,
                };
            }
        };
        let base = DifficultyGrader::new();
        let oc = run_pair(&base, &tampered, &prof);
        // 注入对拍不计入"同点两次判定一致率"统计——一致率判据只对
        // 基准判级器双跑生效，注入是对抗探测，另有检出面统计。
        match &oc {
            PairOutcome::Divergent { .. } => {
                self.events
                    .record(EventKind::PairDivergent, self.tick, XC_GRADE_INCONSISTENT as u32);
                self.events
                    .record(EventKind::InjectBlocked, self.tick, XC_GRADE_INCONSISTENT as u32);
                self.injections_detected = self.injections_detected.saturating_add(1);
                InjectionOutcome {
                    kind: 3,
                    detected: true,
                    code: XC_GRADE_INCONSISTENT,
                }
            }
            PairOutcome::Consistent { .. } => {
                // 阈值改了却判同档同分——篡改未影响该点（不记不一致，
                // 亦不记检出：注入面要的是真分歧被揪出）。
                self.events
                    .record(EventKind::PairConsistent, self.tick, point_id);
                InjectionOutcome {
                    kind: 3,
                    detected: false,
                    code: 0,
                }
            }
        }
    }

    /// 注入检出率分子/分母只读。
    pub fn injection_stats(&self) -> (u32, u32) {
        (self.injections_detected, self.injections_total)
    }

    // ---- 清册导出面 ----

    /// 依赖边批量校验（端点必须已测绘、禁自环、禁零号点）。
    pub fn validate_deps(&self, edges: &[DepEdge]) -> Result<(), PollProfError> {
        let mut i: usize = 0;
        while i < edges.len() {
            let e = edges[i];
            if e.pred == 0 || e.point == 0 || e.pred == e.point {
                return Err(PollProfError::DependencyIllegal {
                    pred: e.pred,
                    point: e.point,
                });
            }
            if !self.survey.is_surveyed(e.pred) || !self.survey.is_surveyed(e.point) {
                return Err(PollProfError::DependencyIllegal {
                    pred: e.pred,
                    point: e.point,
                });
            }
            i += 1;
        }
        Ok(())
    }

    /// 构建分级清册（接管排期输入齐备判据的产出本体）。
    ///
    /// 前置：全部画像已判级（漏判带码拒收）；依赖边全账校验通过。
    /// 行四列：点 id/难度档/依赖前驱（升序去重）/风险注记（档位
    /// 策略 + 顶维 + 强制规则摘要——顶维取贡献最大的维，并列取
    /// 维序靠前者）。导出序：档位升序 + 点 id 升序（插排固定）。
    pub fn build_schedule(&mut self, edges: &[DepEdge]) -> Result<TakeoverScheduleBook, PollProfError> {
        // ① 依赖边全账校验。
        self.validate_deps(edges)?;
        // ② 判级完备前置。
        if self.grade_count() < self.profile_count() {
            self.events
                .record(EventKind::Rejected, self.tick, XC_SCHEDULE_INCOMPLETE as u32);
            return Err(PollProfError::ScheduleIncomplete {
                row: 0,
                column: "难度档",
            });
        }
        // ③ 逐点建行。
        let ids = {
            let mut ids: Vec<u32> = Vec::with_capacity(self.profiles.len());
            let mut i: usize = 0;
            while i < self.profiles.len() {
                ids.push(self.profiles[i].point_id());
                i += 1;
            }
            sort_u32_asc(&mut ids);
            ids
        };
        let mut rows: Vec<ScheduleRow> = Vec::with_capacity(ids.len());
        let mut i: usize = 0;
        while i < ids.len() {
            let pid = ids[i];
            let prof = match self.profile(pid) {
                Some(p) => p,
                None => {
                    return Err(PollProfError::NotSurveyed { point_id: pid });
                }
            };
            let verdict = match self.verdict(pid) {
                Some(v) => v,
                None => {
                    return Err(PollProfError::ScheduleIncomplete {
                        row: i,
                        column: "难度档",
                    });
                }
            };
            // 依赖前驱列： edges 中以 pid 为后继的全部前驱（升序去重）。
            let mut deps: Vec<u32> = Vec::new();
            let mut ei: usize = 0;
            while ei < edges.len() {
                if edges[ei].point == pid {
                    let mut dup = false;
                    let mut di: usize = 0;
                    while di < deps.len() {
                        if deps[di] == edges[ei].pred {
                            dup = true;
                            break;
                        }
                        di += 1;
                    }
                    if !dup {
                        deps.push(edges[ei].pred);
                    }
                }
                ei += 1;
            }
            sort_u32_asc(&mut deps);
            // 风险注记列：档位策略 + 顶维 + 强制规则摘要 + 测绘域。
            let note = risk_note_for(&verdict, &prof);
            rows.push(ScheduleRow {
                point_id: pid,
                tier: verdict.tier(),
                deps,
                risk_note: note,
            });
            i += 1;
        }
        // ④ 导出序固定（档位升序 + 点 id 升序）。
        sort_schedule_rows(&mut rows);
        let mut book = TakeoverScheduleBook {
            rows,
            export_fp: 0,
            export_count: 0,
        };
        // ⑤ 首次导出自证（列齐备机检先行——排期输入齐备判据）。
        book.export()?;
        self.events.record(
            EventKind::ExportDone,
            self.tick,
            book.len() as u32,
        );
        Ok(book)
    }

    // ---- 健康面 ----

    /// 健康快照（注入全检出 + 双跑一致率 100% + 画像零漏判）。
    pub fn health(&self) -> HealthSnapshot {
        let consistency_bp = if self.pair_runs == 0 {
            10_000
        } else {
            // saturating 防溢出；bp = consistent*10000/runs。
            (self
                .pair_consistent
                .saturating_mul(10_000)
                / self.pair_runs.max(1)) as u32
        };
        let mut fp_src: Vec<u8> = Vec::with_capacity(64);
        fp_src.push((self.survey.len() as u32 >> 24) as u8);
        fp_src.push((self.survey.len() as u32 >> 16) as u8);
        fp_src.push((self.survey.len() as u32 >> 8) as u8);
        fp_src.push(self.survey.len() as u8);
        fp_src.push((self.profiles.len() as u32 >> 24) as u8);
        fp_src.push((self.profiles.len() as u32 >> 16) as u8);
        fp_src.push((self.profiles.len() as u32 >> 8) as u8);
        fp_src.push(self.profiles.len() as u8);
        fp_src.push((self.grades.len() as u32 >> 24) as u8);
        fp_src.push((self.grades.len() as u32 >> 16) as u8);
        fp_src.push((self.grades.len() as u32 >> 8) as u8);
        fp_src.push(self.grades.len() as u8);
        push_u64(&mut fp_src, self.injections_detected as u64);
        push_u64(&mut fp_src, self.injections_total as u64);
        push_u64(&mut fp_src, self.pair_runs as u64);
        push_u64(&mut fp_src, self.pair_consistent as u64);
        let fp = fnv1a64(&fp_src);
        HealthSnapshot {
            surveyed: self.survey.len() as u32,
            profiles: self.profiles.len() as u32,
            graded: self.grades.len() as u32,
            events_total: self.events.total(),
            injections_total: self.injections_total,
            injections_detected: self.injections_detected,
            pair_runs: self.pair_runs,
            pair_consistent: self.pair_consistent,
            consistency_bp,
            fp,
        }
    }

    /// 引擎账面指纹（画像账 + 判级账 FNV——跨会话对拍用）。
    pub fn engine_fingerprint(&self) -> u64 {
        let mut h: u64 = FNV_OFFSET;
        let mut i: usize = 0;
        while i < self.profiles.len() {
            h = h
                .wrapping_mul(FNV_PRIME)
                ^ self.profiles[i].fingerprint();
            i += 1;
        }
        let vs = self.verdicts_sorted();
        let mut j: usize = 0;
        while j < vs.len() {
            h = h.wrapping_mul(FNV_PRIME) ^ vs[j].fingerprint();
            j += 1;
        }
        h
    }
}

/// u64 逐字节压入（小端——健康指纹源拼装）。
fn push_u64(dst: &mut Vec<u8>, v: u64) {
    dst.push(v as u8);
    dst.push((v >> 8) as u8);
    dst.push((v >> 16) as u8);
    dst.push((v >> 24) as u8);
    dst.push((v >> 32) as u8);
    dst.push((v >> 40) as u8);
    dst.push((v >> 48) as u8);
    dst.push((v >> 56) as u8);
}

/// 风险注记列文案（档位策略 + 顶维 + 强制规则摘要 + 测绘域）。
fn risk_note_for(verdict: &GradeVerdict, prof: &PollSiteProfile) -> String {
    // 顶维：贡献 = 权重×刻度 取最大（并列取维序靠前）。
    let f = prof.feature();
    let ws = [f.wait(), f.event(), f.ctx(), f.reentry(), f.timing(), f.couple()];
    let mut top: usize = 0;
    let mut top_contrib: u32 = 0;
    let mut i: usize = 0;
    while i < DIM_COUNT {
        let c = WEIGHTS[i] * ws[i] as u32;
        if c > top_contrib {
            top_contrib = c;
            top = i;
        }
        i += 1;
    }
    let mut note = format!(
        "{}·顶维:{}({})·域:{}",
        verdict.tier().policy(),
        DIM_NAMES[top],
        ws[top],
        prof.domain().name()
    );
    if verdict.force_hits() > 0 {
        note.push_str("·有强制规则");
    } else {
        note.push_str("·无强制规则");
    }
    note
}

// ---------------------------------------------------------------------------
// §15 诊断码反解（人话出口——错误码→名字→诊断）
// ---------------------------------------------------------------------------

/// 诊断码人话名（闭集十二码——与 §0 常量一一对应）。
pub fn code_name(code: u16) -> &'static str {
    match code {
        XC_NOT_SURVEYED => "未测绘点拒收",
        XC_FEAT_OUT_OF_RANGE => "特征刻度越界",
        XC_GRADE_INCONSISTENT => "判定不一致",
        XC_SURVEY_EMPTY => "测绘账空",
        XC_PROFILE_DUPLICATE => "画像重复建档",
        XC_SCHEDULE_INCOMPLETE => "清册列缺失",
        XC_DEPENDENCY_ILLEGAL => "依赖边非法",
        XC_EXPORT_DRIFT => "导出指纹漂移",
        XC_THRESHOLD_INVALID => "判级阈值非法",
        XC_TICK_REGRESS => "测绘 tick 回退",
        XC_LEDGER_CLOSED => "封账后再写入",
        XC_RING_CAPACITY => "事件账容量非法",
        _ => "未知诊断码",
    }
}

// ---------------------------------------------------------------------------
// 测试（宿主档——cfg(test) std 面冒烟；内核路径零 unwrap/assert 纪律
// 不适用于测试体）
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::vec;

    /// 测试用 Result 解包（测试体允许 panic）。
    fn ok<T>(r: Result<T, PollProfError>) -> T {
        match r {
            Ok(v) => v,
            Err(e) => panic!("unexpected err {:?}: {}", e, e.detail_straight()),
        }
    }

    #[test]
    fn self_cert_three_tiers_ten_ten() {
        // 判据 J1-①：三档难度各自 10/10 次判定对。
        let g = DifficultyGrader::new();
        let cert = self_cert_ten_ten(&g);
        assert_eq!(cert, [10, 10, 10], "三档自证须各 10/10，实得 {:?}", cert);
    }

    #[test]
    fn grade_explainable_reasons() {
        // 判定理由可解释输出：逐维一行 + 汇总行（+ 强制规则行）。
        let g = DifficultyGrader::new();
        let f = FeatureVector::new_const([6, 3, 4, 5, 5, 4]);
        let v = g.grade(7, &f);
        // 六维行 + 汇总行 + 强制规则行 = 8 行。
        assert_eq!(v.reasons().len(), 8);
        assert!(v.reasons()[0].contains("等待语义=6"));
        assert!(v.reasons()[6].contains("加权分="));
        assert!(v.reasons()[7].contains("R5"));
        assert_eq!(v.tier(), Tier::Hard);
        assert_eq!(v.force_hits(), 2, "R1(忙等起)+R5(忙等紧窗) 应双命中");
    }

    #[test]
    fn reproducibility_double_run_100_percent() {
        // 判据 J1-③：同点两次判定一致率 100%——30 夹具逐条双跑。
        let g = DifficultyGrader::new();
        let mut total = 0u32;
        let mut same = 0u32;
        let mut t: usize = 0;
        while t < 3 {
            let mut i: usize = 0;
            while i < 10 {
                let dims = FIXTURES_ALL[t][i];
                let f1 = FeatureVector::new_const(dims);
                let f2 = FeatureVector::new_const(dims);
                let pid = 500 + (t * 10 + i) as u32;
                let v1 = g.grade(pid, &f1);
                let v2 = g.grade(pid, &f2);
                total += 1;
                if v1.same_as(&v2) {
                    same += 1;
                }
                i += 1;
            }
            t += 1;
        }
        assert_eq!(total, 30);
        assert_eq!(same, 30, "双跑一致率必须 100%");
        // 引擎面双跑对拍：全画像批量，bp==10000。
        let mut eng = ok(PollProfEngine::new());
        ok(eng.ingest_survey_all(&[
            (1, SurveyDomain::F0961, 0b111),
            (2, SurveyDomain::F1283, 0b111),
            (3, SurveyDomain::F0961, 0b111),
        ]));
        ok(eng.profile_point(1, ok(FeatureVector::new([2, 1, 0, 0, 1, 0]))));
        ok(eng.profile_point(2, ok(FeatureVector::new([5, 2, 2, 3, 3, 2]))));
        ok(eng.profile_point(3, ok(FeatureVector::new([6, 3, 4, 5, 5, 4]))));
        ok(eng.grade_all());
        let (runs, cons) = ok(eng.double_grade_all());
        assert_eq!(runs, 3);
        assert_eq!(cons, 3);
        let h = eng.health();
        assert_eq!(h.consistency_bp, 10_000);
        assert!(h.is_healthy());
    }

    #[test]
    fn schedule_book_export_complete_and_deterministic() {
        // 判据 J1-②：接管排期输入齐备（分级清册导出）——四列齐 +
        // 导出序确定 + 双导出指纹对拍。
        let mut eng = ok(PollProfEngine::new());
        ok(eng.ingest_survey_all(&[
            (10, SurveyDomain::F0961, 0b111),
            (11, SurveyDomain::F0961, 0b111),
            (12, SurveyDomain::F1283, 0b111),
            (13, SurveyDomain::F0961, 0b111),
        ]));
        ok(eng.profile_point(10, ok(FeatureVector::new([2, 1, 0, 0, 1, 0])))); // Easy
        ok(eng.profile_point(11, ok(FeatureVector::new([5, 2, 2, 3, 3, 2])))); // Medium
        ok(eng.profile_point(12, ok(FeatureVector::new([6, 3, 4, 5, 5, 4])))); // Hard
        ok(eng.profile_point(13, ok(FeatureVector::new([3, 1, 1, 0, 1, 1])))); // Easy
        ok(eng.grade_all());
        // 依赖边：13 依赖 10（前驱 10 → 后继 13）。
        let mut book = ok(eng.build_schedule(&[DepEdge::new(10, 13)]));
        assert_eq!(book.len(), 4);
        let text1 = ok(book.export());
        let text2 = ok(book.export());
        assert_eq!(text1, text2, "双导出必须逐字节一致（导出序确定性）");
        assert_eq!(book.export_count(), 3);
        // schema 头。
        assert!(text1.starts_with("UNX-F0962-SCHED|v1|rows=4|fp="));
        // 导出序：Easy(10,13) 在前，再 Medium(11)，再 Hard(12)。
        let lines: Vec<&str> = text1.lines().collect();
        assert_eq!(lines.len(), 5);
        assert!(lines[1].starts_with("P000010|1|"));
        assert!(lines[2].starts_with("P000013|1|10|"), "依赖前驱列应含 10");
        assert!(lines[3].starts_with("P000011|2|"));
        assert!(lines[4].starts_with("P000012|3|"));
        // 四列齐备：每行 3 根竖线 + 风险注记非空。
        for l in &lines[1..] {
            assert_eq!(l.matches('|').count(), 3, "行四列: {}", l);
        }
        assert!(lines[1].contains("先接线"));
        // 行列齐备机检（四列逐行）。
        for r in book.rows() {
            ok(r.check_complete());
        }
    }

    #[test]
    fn injection_unsurveyed_detected() {
        // 注入面一：未测绘点注入 → 0x1A11 拦截检出。
        let mut eng = ok(PollProfEngine::new());
        ok(eng.ingest_survey(1, SurveyDomain::F0961, 0b111));
        let oc = eng.inject_unsurveyed_point(999);
        assert!(oc.detected);
        assert_eq!(oc.code, XC_NOT_SURVEYED);
        // 合法点不受影响。
        ok(eng.profile_point(1, ok(FeatureVector::new([1, 0, 0, 0, 0, 0]))));
        let (d, t) = eng.injection_stats();
        assert_eq!((d, t), (1, 1));
    }

    #[test]
    fn injection_out_of_range_detected() {
        // 注入面二：特征越界注入 → 0x1A12 拦截检出，画像账不脏。
        let mut eng = ok(PollProfEngine::new());
        ok(eng.ingest_survey(1, SurveyDomain::F1283, 0b111));
        let oc = eng.inject_out_of_range(1, 3, 9);
        assert!(oc.detected);
        assert_eq!(oc.code, XC_FEAT_OUT_OF_RANGE);
        assert_eq!(eng.profile_count(), 0, "越界画像不得入账");
        // 合法刻度正常入账。
        ok(eng.profile_point(1, ok(FeatureVector::new([1, 0, 0, 0, 0, 0]))));
        assert_eq!(eng.profile_count(), 1);
    }

    #[test]
    fn injection_inconsistency_detected() {
        // 注入面三：判定不一致注入 → 阈值篡改器分歧被 0x1A13 揪出。
        let mut eng = ok(PollProfEngine::new());
        ok(eng.ingest_survey(1, SurveyDomain::F0961, 0b111));
        ok(eng.profile_point(1, ok(FeatureVector::new([4, 2, 3, 4, 4, 3])))); // 355 分 Medium
        // 阈值篡改：Easy≤100 Med≤200 → 该点被改判 Hard → 分歧检出。
        let oc = eng.inject_inconsistency(1, 100, 200);
        assert!(oc.detected);
        assert_eq!(oc.code, XC_GRADE_INCONSISTENT);
        // 非法阈值形态：Easy≥Med → 构造面即被 0x1A19 拦下（也算检出）。
        let oc2 = eng.inject_inconsistency(1, 300, 200);
        assert!(oc2.detected);
        assert_eq!(oc2.code, XC_THRESHOLD_INVALID);
        // 未篡改的对拍不误报。
        let pair = ok(eng.double_grade(1));
        assert!(pair.is_consistent());
        let (d, t) = eng.injection_stats();
        assert_eq!((d, t), (2, 2));
    }

    #[test]
    fn survey_gate_and_ledger_discipline() {
        // 测绘账门槛：未 ingest 的点拒收；封账后写入拒收；tick 回退拒收。
        let mut eng = ok(PollProfEngine::new());
        // 未测绘直接画像 → 0x1A11。
        match eng.profile_point(42, ok(FeatureVector::new([1, 0, 0, 0, 0, 0]))) {
            Err(e) => {
                assert_eq!(e.code(), XC_NOT_SURVEYED);
                assert!(e.detail_straight().contains("测绘"));
            }
            Ok(()) => panic!("未测绘点必须拒收"),
        }
        // tick 回退 → 0x1A1A（账本级 tick 单调守门）。
        eng.advance_tick(100);
        ok(eng.ingest_survey(1, SurveyDomain::F0961, 0b111));
        let mut alt = SurveyLedger::new();
        ok(alt.ingest(5, SurveyDomain::F0961, 100, 0b111));
        match alt.ingest(2, SurveyDomain::F0961, 50, 0b111) {
            Err(e) => assert_eq!(e.code(), XC_TICK_REGRESS),
            Ok(()) => panic!("tick 回退必须拒收"),
        }
        // 零号保留点拒收。
        match eng.ingest_survey(0, SurveyDomain::F0961, 0b111) {
            Err(e) => assert_eq!(e.code(), XC_NOT_SURVEYED),
            Ok(()) => panic!("零号点必须拒收"),
        }
        // 封账后写入 → 0x1A1B。
        eng.seal_survey();
        assert!(eng.survey().is_closed());
        match eng.ingest_survey(3, SurveyDomain::F0961, 0b111) {
            Err(e) => assert_eq!(e.code(), XC_LEDGER_CLOSED),
            Ok(()) => panic!("封账后必须拒收"),
        }
    }

    #[test]
    fn dependency_gate_and_cycle_reject() {
        // 依赖边守门：自环/端点未测绘/零号点全部带码拒收。
        let mut eng = ok(PollProfEngine::new());
        ok(eng.ingest_survey_all(&[(1, SurveyDomain::F0961, 0b111), (2, SurveyDomain::F0961, 0b111)]));
        // 自环。
        match eng.validate_deps(&[DepEdge::new(1, 1)]) {
            Err(e) => assert_eq!(e.code(), XC_DEPENDENCY_ILLEGAL),
            Ok(()) => panic!("自环必须拒收"),
        }
        // 端点未测绘。
        match eng.validate_deps(&[DepEdge::new(1, 77)]) {
            Err(e) => assert_eq!(e.code(), XC_DEPENDENCY_ILLEGAL),
            Ok(()) => panic!("未测绘端点必须拒收"),
        }
        // 零号点。
        match eng.validate_deps(&[DepEdge::new(0, 1)]) {
            Err(e) => assert_eq!(e.code(), XC_DEPENDENCY_ILLEGAL),
            Ok(()) => panic!("零号端点必须拒收"),
        }
        // 合法边通过，且清册依赖列正确落账。
        ok(eng.profile_point(1, ok(FeatureVector::new([1, 0, 0, 0, 0, 0]))));
        ok(eng.profile_point(2, ok(FeatureVector::new([2, 1, 0, 0, 1, 0]))));
        ok(eng.grade_all());
        let book = ok(eng.build_schedule(&[DepEdge::new(1, 2), DepEdge::new(1, 2)]));
        // 点 2 的依赖列应去重后只含 1。
        let r2 = book
            .rows()
            .iter()
            .find(|r| r.point_id == 2)
            .expect("行 2 必在");
        assert_eq!(r2.deps, vec![1]);
    }

    #[test]
    fn threshold_guard_and_profile_duplicate() {
        // 阈值守门 + 画像重复建档守门。
        // 非法阈值（Easy≥Med）→ 0x1A19。
        match DifficultyGrader::with_thresholds(400, 300) {
            Err(e) => assert_eq!(e.code(), XC_THRESHOLD_INVALID),
            Ok(_) => panic!("非法阈值必须拒收"),
        }
        // 非法阈值（Med>700）→ 0x1A19。
        match DifficultyGrader::with_thresholds(100, 701) {
            Err(e) => assert_eq!(e.code(), XC_THRESHOLD_INVALID),
            Ok(_) => panic!("越域阈值必须拒收"),
        }
        // 合法覆盖阈值可用。
        let g = ok(DifficultyGrader::with_thresholds(100, 200));
        assert_eq!(g.easy_max(), 100);
        // 同点重复建档 → 0x1A15。
        let mut eng = ok(PollProfEngine::new());
        ok(eng.ingest_survey(1, SurveyDomain::F0961, 0b111));
        ok(eng.profile_point(1, ok(FeatureVector::new([1, 0, 0, 0, 0, 0]))));
        match eng.profile_point(1, ok(FeatureVector::new([2, 0, 0, 0, 0, 0]))) {
            Err(e) => assert_eq!(e.code(), XC_PROFILE_DUPLICATE),
            Ok(()) => panic!("重复建档必须拒收"),
        }
        // reprofile 路径：摘旧建新成功，指纹随刻度变化。
        let fp1 = eng.profile(1).map(|p| p.fingerprint()).unwrap_or(0);
        assert!(eng.drop_profile(1));
        ok(eng.profile_point(1, ok(FeatureVector::new([2, 0, 0, 0, 0, 0]))));
        let fp2 = eng.profile(1).map(|p| p.fingerprint()).unwrap_or(0);
        assert_ne!(fp1, fp2);
    }

    #[test]
    fn health_and_fingerprint_stability() {
        // 健康面：注入全检出 + 双跑 100% + 零漏判 → is_healthy。
        let mut eng = ok(PollProfEngine::new());
        ok(eng.ingest_survey_all(&[
            (1, SurveyDomain::F0961, 0b111),
            (2, SurveyDomain::F0961, 0b111),
            (3, SurveyDomain::F1283, 0b111),
        ]));
        ok(eng.profile_point(1, ok(FeatureVector::new([1, 0, 0, 0, 0, 0]))));
        ok(eng.profile_point(2, ok(FeatureVector::new([5, 2, 2, 3, 3, 2]))));
        ok(eng.profile_point(3, ok(FeatureVector::new([7, 3, 3, 4, 4, 3]))));
        ok(eng.grade_all());
        ok(eng.double_grade_all());
        // 三注入各一。
        assert!(eng.inject_unsurveyed_point(88).detected);
        assert!(eng.inject_out_of_range(3, 5, 200).detected);
        assert!(eng.inject_inconsistency(2, 100, 200).detected);
        let h = eng.health();
        assert_eq!(h.injections_detected, h.injections_total);
        assert_eq!(h.injections_total, 3);
        assert_eq!(h.consistency_bp, 10_000);
        assert_eq!(h.graded, h.profiles);
        assert!(h.is_healthy());
        // 引擎指纹对状态敏感且确定（两次调用同值）。
        let f1 = eng.engine_fingerprint();
        let f2 = eng.engine_fingerprint();
        assert_eq!(f1, f2);
        // 状态变更（新增画像）→ 指纹变。
        ok(eng.ingest_survey(4, SurveyDomain::F0961, 0b111));
        ok(eng.profile_point(4, ok(FeatureVector::new([3, 1, 1, 0, 1, 1]))));
        assert_ne!(eng.engine_fingerprint(), f1);
        // 事件账全程计数与环覆盖纪律。
        assert!(eng.events().total() >= eng.events().live() as u64);
        assert!(eng.events().live() <= EVENT_RING_CAP);
        // 快照健康指纹确定。
        let h1 = eng.health();
        let h2 = eng.health();
        assert_eq!(h1, h2);
    }

    #[test]
    fn event_ledger_ring_and_counts() {
        // 事件账：定容覆盖 + 全程计数 + 按种计数 + 时间序快照。
        let mut led = ok(EventLedger::new(4));
        let mut t: u64 = 0;
        let mut i: u32 = 0;
        while i < 10 {
            led.record(EventKind::GradeDone, t, i);
            t += 1;
            i += 1;
        }
        assert_eq!(led.total(), 10);
        assert_eq!(led.live(), 4);
        assert_eq!(led.count_of(EventKind::GradeDone), 10);
        let snap = led.snapshot();
        assert_eq!(snap.len(), 4);
        // 最旧被覆盖：环内首行 arg 应为 6（6,7,8,9）。
        assert_eq!(snap[0].arg(), 6);
        assert_eq!(snap[3].arg(), 9);
        // 容量非法拒收。
        match EventLedger::new(0) {
            Err(e) => assert_eq!(e.code(), XC_RING_CAPACITY),
            Ok(_) => panic!("零容量必须拒收"),
        }
    }

    #[test]
    fn code_table_and_scales_integrity() {
        // 诊断码闭集：十二码两两互异且全部落在 0x1A10 段。
        let codes = [
            XC_NOT_SURVEYED,
            XC_FEAT_OUT_OF_RANGE,
            XC_GRADE_INCONSISTENT,
            XC_SURVEY_EMPTY,
            XC_PROFILE_DUPLICATE,
            XC_SCHEDULE_INCOMPLETE,
            XC_DEPENDENCY_ILLEGAL,
            XC_EXPORT_DRIFT,
            XC_THRESHOLD_INVALID,
            XC_TICK_REGRESS,
            XC_LEDGER_CLOSED,
            XC_RING_CAPACITY,
        ];
        let mut i: usize = 0;
        while i < codes.len() {
            assert_eq!(codes[i] & 0xFFF0, 0x1A10, "码必须落在 0x1A10 段");
            let mut j: usize = i + 1;
            while j < codes.len() {
                assert_ne!(codes[i], codes[j]);
                j += 1;
            }
            i += 1;
        }
        // 码→人话名闭集反解。
        for c in codes {
            assert_ne!(code_name(c), "未知诊断码");
        }
        assert_eq!(code_name(0x1234), "未知诊断码");
        // 六维刻度表：每维八级、级名非空。
        let mut d: usize = 0;
        while d < DIM_COUNT {
            assert_eq!(DIM_SCALES[d].len(), 8);
            let mut l: usize = 0;
            while l < 8 {
                assert!(!DIM_SCALES[d][l].is_empty());
                l += 1;
            }
            d += 1;
        }
        // Tier 闭集往返。
        let mut c: u8 = 1;
        while c <= 3 {
            let t = Tier::from_code(c);
            assert!(t.is_some());
            assert_eq!(t.unwrap().code(), c);
            c += 1;
        }
        assert!(Tier::from_code(0).is_none());
        assert!(Tier::from_code(4).is_none());
    }

    #[test]
    fn error_details_actionable() {
        // 错误枚举：每变体 detail 非空且带可操作语义（中文诊断）。
        let errs = vec![
            PollProfError::NotSurveyed { point_id: 1 },
            PollProfError::FeatOutOfRange { dim: 0, value: 9 },
            PollProfError::GradeInconsistent {
                point_id: 1,
                tier_a: 1,
                tier_b: 3,
            },
            PollProfError::SurveyEmpty,
            PollProfError::ProfileDuplicate { point_id: 1 },
            PollProfError::ScheduleIncomplete {
                row: 0,
                column: "点id",
            },
            PollProfError::DependencyIllegal { pred: 1, point: 1 },
            PollProfError::ExportDrift { first: 1, second: 2 },
            PollProfError::ThresholdInvalid {
                easy_max: 5,
                med_max: 4,
            },
            PollProfError::TickRegress { last: 9, got: 1 },
            PollProfError::LedgerClosed,
            PollProfError::RingCapacity { got: 0 },
        ];
        for e in errs {
            assert!(!e.detail_straight().is_empty());
            assert_eq!(e.code() & 0xFFF0, 0x1A10, "码必须落在 0x1A10 段");
        }
    }

    #[test]
    fn feature_vector_gate_and_score_bounds() {
        // 特征向量守门：越界带码拒收；加权分边界。
        match FeatureVector::new([0, 0, 0, 0, 0, 8]) {
            Err(e) => {
                assert_eq!(e.code(), XC_FEAT_OUT_OF_RANGE);
                match e {
                    PollProfError::FeatOutOfRange { dim, value } => {
                        assert_eq!(dim, 5);
                        assert_eq!(value, 8);
                    }
                    _ => panic!("变体应匹配"),
                }
            }
            Ok(_) => panic!("越界必须拒收"),
        }
        // 下界：全 0 → 0 分。
        let z = ok(FeatureVector::new([0, 0, 0, 0, 0, 0]));
        assert_eq!(z.weighted_score(), 0);
        // 上界：全 7 → 700 分（权重和 100 的编译期自证已挂）。
        let m = ok(FeatureVector::new([7, 7, 7, 7, 7, 7]));
        assert_eq!(m.weighted_score(), SCORE_MAX);
        // 画像一句话可解释输出含关键数字。
        let mut eng = ok(PollProfEngine::new());
        ok(eng.ingest_survey(9, SurveyDomain::F1283, 0b111));
        ok(eng.profile_point(9, ok(FeatureVector::new([2, 1, 0, 0, 1, 0]))));
        let p = eng.profile(9).expect("画像必在");
        let d = p.describe();
        assert!(d.contains("点9"));
        assert!(d.contains("F1283"));
        assert!(d.contains("加权分80"));
    }
}
