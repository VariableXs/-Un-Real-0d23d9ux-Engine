//! UNX-F49073 延迟账本用例集（20 例入库）· 0x73400
//! ═══════════════════════════════════════════════════════════════════════
//! 判据原文（任务板逐字锚定，B14 族第七槽）：
//!   UNX-F49073 延迟账本用例集（20 例入库）:
//!          ① 纯功能代码 ≥300 行；
//!          ② 判据原文：UNX-F49074-J1 联轧 ×5 全绿零漂移；
//!             判据锚定 §AI-62 §7。
//!   UNX-F49074 B14 段内联轧（五路交叉对账）:
//!          ① 纯功能代码 ≥300 行；
//!          ② 判据原文：UNX-F49075-J1 错误码 ×15 零漂移互不混淆；
//!             判据锚定 §AI-62 §2。
//! ─────────────────────────────────────────────────────────────────────
//! 闸门次序（用例集）：规格 → 编号 → 执行 → 统计 → 入库 → 目录审计。
//! 规格先于编号：用例号只是一把钥匙，规格不合法时这把钥匙开的是空抽屉；
//! 编号先于执行：1..20 连续入库是目录可枚举的前提，先跑后编号会让「缺哪
//!   一号」只能靠翻执行日志才知道；
//! 执行先于统计：分位与和必须来自同一次采样，先统计后执行等于凭空造数；
//! 统计先于入库：一条统计量倒挂的用例进库后，联轧会把倒挂当成基线；
//! 入库先于目录审计：目录审计的对象是**已入库**的二十条，而不是生成器。
//! 闸门次序（联轧）：五路独立复算 → 逐路裁决 → 三轮一致 → 五轮零漂移。
//! 五路先于裁决：条数、和、p50、p99、摘要五路各自独立取值，任何一路不许
//!   借用别路的中间量——借用会让「五路一致」退化成一路自证；
//! 裁决先于三轮：单路失败已经足以否决该用例，不必等三轮跑完；
//! 三轮先于五轮：三轮用于暴露「偶发」漂移，五轮用于确认「稳定」零漂移；
//! 五轮先于成文：只有五轮链头逐位相同，联轧结果才允许写进治理线账。
//! 关键差异：F49073 回答「二十例是否按号入库、每例能否五路回放」；
//! F49074 回答「五轮联轧是否全绿零漂移、十五个错误标签是否互不混淆」。
//! 本模块**不**做对照实验（F49071）、不做告警（F49069）、不做 O1 列（F49063）；
//! 它只经营「用例目录」与「五路联轧」这两件事。
//! 错误码口径说明：本模块的诊断枚举严格为 14 变体（契约闭集），判据原文
//! 的「×15」按**标签面**落账：14 条枚举说明 + 段外哨兵 UNKNOWN 共 15 条，
//! 要求 15 条两两不同、14 条码值与说明的映射双向稳定、哨兵不与任何一条
//! 同文。这样「×15 零漂移互不混淆」验的是整个标签面，而不是把第 15 条
//! 硬塞进枚举破坏 14 变体契约。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;
use super::unx48941_wheel_core as p_core;

/// 诊断段基址（0x73400，B14 族第七槽）。
pub const SEG_BASE: u32 = 0x73400;

/// 延迟链四段。
pub const SEGMENTS: usize = 4;
/// 用例总数（判据口径 ×20）。
pub const CASE_COUNT: usize = 20;
/// 联轧轮数（判据口径 ×5）。
pub const ROLL_TIMES: u32 = 5;
/// 联轧轮数上限。
pub const ROLL_LIMIT: u32 = 16;
/// 交叉对账路数（条数/和/p50/p99/摘要）。
pub const ROADS: usize = 5;
/// 用例样本数下限。
pub const SAMPLES_MIN: u32 = 16;
/// 用例样本数上限。
pub const SAMPLES_MAX: u32 = 256;
/// 基准延迟下限（纳秒）。
pub const BASE_MIN_NS: u32 = 200;
/// 基准延迟上限（纳秒）。
pub const BASE_MAX_NS: u32 = 100_000;
/// 抖动上限（纳秒）。
pub const JITTER_MAX_NS: u32 = 100_000;
/// 长尾概率分母下限。
pub const TAIL_ONE_IN_MIN: u32 = 2;
/// 长尾概率分母上限。
pub const TAIL_ONE_IN_MAX: u32 = 1_024;
/// 长尾放大倍数上限。
pub const TAIL_FACTOR_MAX: u32 = 8;
/// 计时值上限（1 秒，纳秒）。
pub const NS_LIMIT: u32 = 1_000_000_000;
/// 千分比刻度。
pub const PERMILLE: u32 = 1_000;
/// 直方图桶数。
pub const HIST_BUCKETS: usize = 32;
/// 账行字段数（tag + 四键 + digest）。
pub const ROW_FIELDS: usize = 6;
/// 账行标签。
pub const CASE_TAG: &str = "CASE";
/// 未知诊断码说明（同时充当第 15 条标签）。
pub const UNKNOWN: &str = "未知 B14 用例/联轧诊断码";
/// 错误标签面条数（14 枚举 + 1 哨兵）。
pub const ERROR_LABELS: usize = 15;
/// 四段名称（下标即段号）。
pub const SEG_NAMES: [&str; SEGMENTS] = ["interrupt", "dispatch", "compose", "present"];
/// 四段基准附加延迟（纳秒）。
pub const SEG_ADD_NS: [u32; SEGMENTS] = [0, 120, 480, 960];
/// 五路名称（下标即路号）。
pub const ROAD_NAMES: [&str; ROADS] = ["count", "sum", "p50", "p99", "chain"];
/// 账行数字键（固定列序）。
pub const ROW_KEYS: [&str; 4] = ["id", "seg", "n", "p99"];
/// 用例目录标签前缀（诊断文本用）。
pub const CATALOG_LABEL: &str = "B14/CASE20";

// ── 诊断码闭集（窗口 +0x01..+0x0E） ───────────────────────────────
/// 用例集/联轧诊断码（14 条，说明互不重复）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CaseCode {
    /// 用例号为零或超过二十
    IdBad,
    /// 段号越出四段
    SegBad,
    /// 样本条数越界
    CountBad,
    /// 基准延迟越界
    BaseBad,
    /// 抖动/长尾参数越界
    ParamBad,
    /// 用例号重复登记
    DuplicateBad,
    /// 登记号不连续（跳号）
    OrderBad,
    /// 目录不齐（不足二十条）
    CatalogBad,
    /// 用例执行失败或统计缺口
    RunBad,
    /// 联轧漂移（多轮摘要不一致）
    DriftBad,
    /// 行摘要与可见字段不符
    DigestBad,
    /// 账行键序/数字/往返非法
    RenderBad,
    /// 目录容量已满
    CapacityBad,
    /// 五路对账不一致
    CompareBad,
}

impl CaseCode {
    /// 诊断码数值：严格占满 SEG_BASE+0x01..=SEG_BASE+0x0E。
    pub fn val(self) -> u32 {
        SEG_BASE + 0x01 + self as u32
    }

    /// 单条中文说明（互不重复，亦不与 UNKNOWN 同文）。
    pub fn message(self) -> &'static str {
        match self {
            CaseCode::IdBad => "用例号非法：为零或逃出一至二十号窗口",
            CaseCode::SegBad => "段号非法：逃出中断/分发/合成/呈现四段",
            CaseCode::CountBad => "条数非法：样本数少于十六或多于二百五十六",
            CaseCode::BaseBad => "基准非法：基准延迟越出二百纳秒至一百微秒",
            CaseCode::ParamBad => "参数非法：抖动过大或长尾分母/倍数越界",
            CaseCode::DuplicateBad => "重复登记：该用例号已在目录中",
            CaseCode::OrderBad => "登记跳号：新用例号不等于下一条待登号",
            CaseCode::CatalogBad => "目录不齐：入库条数不足二十或号段有洞",
            CaseCode::RunBad => "执行失败：采样中途出错或统计量缺口",
            CaseCode::DriftBad => "联轧漂移：多轮目录摘要不是逐位相同",
            CaseCode::DigestBad => "行摘要不符：摘要与 id/seg/n/p99 复算不等",
            CaseCode::RenderBad => "账行非法：键序错误、数字溢出或往返不等",
            CaseCode::CapacityBad => "容量已满：目录装不下第二十一条用例",
            CaseCode::CompareBad => "五路不一致：条数/和/分位/摘要至少一路失败",
        }
    }

    /// 全量闭集（顺序即码值顺序）。
    pub const ALL: [CaseCode; 14] = [
        CaseCode::IdBad,
        CaseCode::SegBad,
        CaseCode::CountBad,
        CaseCode::BaseBad,
        CaseCode::ParamBad,
        CaseCode::DuplicateBad,
        CaseCode::OrderBad,
        CaseCode::CatalogBad,
        CaseCode::RunBad,
        CaseCode::DriftBad,
        CaseCode::DigestBad,
        CaseCode::RenderBad,
        CaseCode::CapacityBad,
        CaseCode::CompareBad,
    ];
}

/// 按数值反查说明；段外一律 UNKNOWN。
pub fn detail(code: u32) -> &'static str {
    let off = code.wrapping_sub(SEG_BASE);
    if off < 0x01 || off > 0x0E {
        return UNKNOWN;
    }
    let mut i = 0usize;
    while i < CaseCode::ALL.len() {
        if let Some(c) = CaseCode::ALL.get(i) {
            if c.val() == code {
                return c.message();
            }
        }
        i += 1;
    }
    UNKNOWN
}

/// 段位闸门：14 条码恰好占满 [SEG_BASE+1, SEG_BASE+14] 且无重复。
pub fn segment_guard() -> bool {
    let mut i = 0usize;
    while i < CaseCode::ALL.len() {
        let a = match CaseCode::ALL.get(i) {
            Some(c) => c.val(),
            None => return false,
        };
        if a != SEG_BASE + 0x01 + i as u32 {
            return false;
        }
        if a <= SEG_BASE || a > SEG_BASE + 0x0E {
            return false;
        }
        let mut j = i + 1usize;
        while j < CaseCode::ALL.len() {
            if let Some(b) = CaseCode::ALL.get(j) {
                if b.val() == a {
                    return false;
                }
            }
            j += 1;
        }
        if detail(a) == UNKNOWN || detail(a).is_empty() {
            return false;
        }
        i += 1;
    }
    true
}

/// 十五条错误标签：十四条枚举说明 + 段外哨兵，供「×15 互不混淆」判据。
pub fn error_catalog() -> Vec<&'static str> {
    let mut out = Vec::with_capacity(ERROR_LABELS);
    let mut i = 0usize;
    while i < CaseCode::ALL.len() {
        if let Some(c) = CaseCode::ALL.get(i) {
            out.push(c.message());
        }
        i += 1;
    }
    out.push(UNKNOWN);
    out
}

/// 十五条标签两两不同，且每一条都非空。
pub fn labels_distinct() -> bool {
    let labels = error_catalog();
    if labels.len() != ERROR_LABELS {
        return false;
    }
    let mut i = 0usize;
    while i < labels.len() {
        let a = match labels.get(i) {
            Some(v) => *v,
            None => return false,
        };
        if a.is_empty() {
            return false;
        }
        let mut j = i + 1usize;
        while j < labels.len() {
            if let Some(b) = labels.get(j) {
                if *b == a {
                    return false;
                }
            }
            j += 1;
        }
        i += 1;
    }
    true
}

/// 码值→说明双向稳定：两遍取值一致，且段外三处都是哨兵。
pub fn labels_stable() -> bool {
    let mut i = 0usize;
    while i < CaseCode::ALL.len() {
        let c = match CaseCode::ALL.get(i) {
            Some(v) => v,
            None => return false,
        };
        let first = detail(c.val());
        let second = detail(c.val());
        if first != second || first != c.message() {
            return false;
        }
        // 同一说明不得被另一个码值复用。
        let mut j = i + 1usize;
        while j < CaseCode::ALL.len() {
            if let Some(d) = CaseCode::ALL.get(j) {
                if detail(d.val()) == first {
                    return false;
                }
            }
            j += 1;
        }
        i += 1;
    }
    detail(SEG_BASE) == UNKNOWN
        && detail(SEG_BASE + 0x0F) == UNKNOWN
        && detail(0) == UNKNOWN
}

// ── 摘要折叠 ──────────────────────────────────────────────────────
/// 64 位折叠（p_core 只有 32 位折叠）。
pub fn fold_digest(h: u64, v: u64) -> u64 {
    p_core::fold_u32(p_core::fold_u32(h, (v >> 32) as u32), v as u32)
}

/// 行摘要：只折行内可见的 id/seg/n/p99。
pub fn row_digest(id: u32, seg: u32, n: u32, p99: u32) -> u64 {
    let mut h = p_core::FNV_OFF;
    h = p_core::fold_u32(h, id);
    h = p_core::fold_u32(h, seg);
    h = p_core::fold_u32(h, n);
    h = p_core::fold_u32(h, p99);
    h
}

/// 目录链摘要：前链头 + 本用例行摘要。
pub fn chain_digest(prev: u64, digest: u64) -> u64 {
    fold_digest(prev, digest)
}

// ── 用例规格 ──────────────────────────────────────────────────────
/// 一条用例的生成规格：号、段、条数、基准、抖动、长尾。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct CaseSpec {
    pub id: u32,
    pub seg: u8,
    pub samples: u32,
    pub base_ns: u32,
    pub jitter_ns: u32,
    pub tail_one_in: u32,
    pub tail_factor: u32,
}

impl CaseSpec {
    /// 规格合法性：号 → 段 → 条数 → 基准 → 抖动/长尾，逐层拒收。
    pub fn validate(&self) -> Result<(), u32> {
        if self.id == 0 || self.id > CASE_COUNT as u32 {
            return Err(CaseCode::IdBad.val());
        }
        if self.seg as usize >= SEGMENTS {
            return Err(CaseCode::SegBad.val());
        }
        if self.samples < SAMPLES_MIN || self.samples > SAMPLES_MAX {
            return Err(CaseCode::CountBad.val());
        }
        if self.base_ns < BASE_MIN_NS || self.base_ns > BASE_MAX_NS {
            return Err(CaseCode::BaseBad.val());
        }
        if self.jitter_ns > JITTER_MAX_NS {
            return Err(CaseCode::ParamBad.val());
        }
        if self.jitter_ns as u64 > self.base_ns as u64 * 4 {
            return Err(CaseCode::ParamBad.val());
        }
        if self.tail_one_in < TAIL_ONE_IN_MIN || self.tail_one_in > TAIL_ONE_IN_MAX {
            return Err(CaseCode::ParamBad.val());
        }
        if self.tail_factor < 1 || self.tail_factor > TAIL_FACTOR_MAX {
            return Err(CaseCode::ParamBad.val());
        }
        Ok(())
    }

    /// 规格摘要：联轧时把二十条规格折进目录摘要的一部分。
    pub fn digest(&self) -> u64 {
        let mut h = p_core::FNV_OFF;
        h = p_core::fold_u32(h, self.id);
        h = p_core::fold_byte(h, self.seg);
        h = p_core::fold_u32(h, self.samples);
        h = p_core::fold_u32(h, self.base_ns);
        h = p_core::fold_u32(h, self.jitter_ns);
        h = p_core::fold_u32(h, self.tail_one_in);
        h = p_core::fold_u32(h, self.tail_factor);
        h
    }

    /// 段名（诊断文本用）。
    pub fn segment_name(&self) -> Result<&'static str, u32> {
        match SEG_NAMES.get(self.seg as usize) {
            Some(v) => Ok(*v),
            None => Err(CaseCode::SegBad.val()),
        }
    }
}

/// 二十条用例规格（号即下标+1，段按 0..3 轮转）。
///
/// 规格表是**常量**而不是运行时随机生成：用例集要的是「同一条用例在任何
/// 机器上都复现同一批样本」，规格一旦可漂移，联轧的「零漂移」就失去基准。
pub const CASE_SPECS: [CaseSpec; CASE_COUNT] = [
    CaseSpec { id: 1, seg: 0, samples: 64, base_ns: 700, jitter_ns: 180, tail_one_in: 16, tail_factor: 4 },
    CaseSpec { id: 2, seg: 0, samples: 48, base_ns: 820, jitter_ns: 90, tail_one_in: 32, tail_factor: 3 },
    CaseSpec { id: 3, seg: 1, samples: 96, base_ns: 1_100, jitter_ns: 260, tail_one_in: 16, tail_factor: 5 },
    CaseSpec { id: 4, seg: 1, samples: 32, base_ns: 1_250, jitter_ns: 110, tail_one_in: 8, tail_factor: 2 },
    CaseSpec { id: 5, seg: 2, samples: 128, base_ns: 2_600, jitter_ns: 640, tail_one_in: 16, tail_factor: 4 },
    CaseSpec { id: 6, seg: 2, samples: 80, base_ns: 2_400, jitter_ns: 300, tail_one_in: 24, tail_factor: 3 },
    CaseSpec { id: 7, seg: 3, samples: 160, base_ns: 4_400, jitter_ns: 900, tail_one_in: 16, tail_factor: 6 },
    CaseSpec { id: 8, seg: 3, samples: 72, base_ns: 3_900, jitter_ns: 420, tail_one_in: 12, tail_factor: 3 },
    CaseSpec { id: 9, seg: 0, samples: 56, base_ns: 650, jitter_ns: 60, tail_one_in: 64, tail_factor: 2 },
    CaseSpec { id: 10, seg: 1, samples: 112, base_ns: 1_050, jitter_ns: 500, tail_one_in: 16, tail_factor: 4 },
    CaseSpec { id: 11, seg: 2, samples: 144, base_ns: 3_100, jitter_ns: 800, tail_one_in: 20, tail_factor: 5 },
    CaseSpec { id: 12, seg: 3, samples: 88, base_ns: 5_200, jitter_ns: 1_200, tail_one_in: 16, tail_factor: 4 },
    CaseSpec { id: 13, seg: 0, samples: 40, base_ns: 900, jitter_ns: 45, tail_one_in: 128, tail_factor: 2 },
    CaseSpec { id: 14, seg: 1, samples: 104, base_ns: 1_500, jitter_ns: 220, tail_one_in: 32, tail_factor: 3 },
    CaseSpec { id: 15, seg: 2, samples: 120, base_ns: 2_900, jitter_ns: 700, tail_one_in: 10, tail_factor: 4 },
    CaseSpec { id: 16, seg: 3, samples: 152, base_ns: 6_100, jitter_ns: 1_500, tail_one_in: 16, tail_factor: 5 },
    CaseSpec { id: 17, seg: 0, samples: 68, base_ns: 780, jitter_ns: 130, tail_one_in: 18, tail_factor: 3 },
    CaseSpec { id: 18, seg: 1, samples: 92, base_ns: 1_320, jitter_ns: 340, tail_one_in: 22, tail_factor: 4 },
    CaseSpec { id: 19, seg: 2, samples: 136, base_ns: 2_200, jitter_ns: 560, tail_one_in: 14, tail_factor: 3 },
    CaseSpec { id: 20, seg: 3, samples: 100, base_ns: 4_700, jitter_ns: 880, tail_one_in: 16, tail_factor: 6 },
];

/// 按号取规格（1 基；越界 None）。
pub fn spec_of(id: u32) -> Option<&'static CaseSpec> {
    if id == 0 || id > CASE_COUNT as u32 {
        return None;
    }
    CASE_SPECS.get(id as usize - 1)
}

/// 用例主种子：号左移 17 位，避免相邻号只差最低位而被 Lcg::new 的 |1 合并。
pub fn case_seed(id: u32) -> u64 {
    let x = id as u64;
    0x0B14_7300u64 ^ x.wrapping_shl(17) ^ x.wrapping_mul(0x9E37_79B9)
}

// ── 直方图与统计 ──────────────────────────────────────────────────
/// floor(log2(ns)) 桶号；零值不入桶。
pub fn bucket_of(ns: u32) -> usize {
    if ns == 0 {
        return 0;
    }
    let mut b = 0usize;
    let mut v = ns;
    while v > 1 {
        v >>= 1;
        b += 1;
    }
    if b >= HIST_BUCKETS {
        HIST_BUCKETS - 1
    } else {
        b
    }
}

/// 桶下界：桶 b 覆盖 [2^b, 2^(b+1))，b=0 时退化为 1。
pub fn bucket_floor(b: usize) -> u32 {
    if b == 0 {
        return 1;
    }
    if b >= 30 {
        return 1u32 << 30;
    }
    1u32 << b
}

/// 用例直方图：桶计数 + 和 + 极值 + 样本流摘要。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct CaseHist {
    buckets: [u32; HIST_BUCKETS],
    count: u32,
    sum: u64,
    min: u32,
    max: u32,
    stream: u64,
}

impl CaseHist {
    pub const fn new() -> CaseHist {
        CaseHist {
            buckets: [0u32; HIST_BUCKETS],
            count: 0,
            sum: 0,
            min: 0,
            max: 0,
            stream: p_core::FNV_OFF,
        }
    }

    /// 落一条样本；零值与越界值拒收。
    pub fn record(&mut self, ns: u32) -> Result<(), u32> {
        if ns == 0 || ns > NS_LIMIT {
            return Err(CaseCode::RunBad.val());
        }
        let b = bucket_of(ns);
        match self.buckets.get_mut(b) {
            Some(slot) => {
                *slot = slot.saturating_add(1);
            }
            None => return Err(CaseCode::RunBad.val()),
        }
        self.count = match self.count.checked_add(1) {
            Some(v) => v,
            None => return Err(CaseCode::CapacityBad.val()),
        };
        self.sum = match self.sum.checked_add(ns as u64) {
            Some(v) => v,
            None => return Err(CaseCode::CapacityBad.val()),
        };
        if self.min == 0 || ns < self.min {
            self.min = ns;
        }
        if ns > self.max {
            self.max = ns;
        }
        self.stream = p_core::fold_u32(self.stream, ns);
        Ok(())
    }

    pub fn count(&self) -> u32 {
        self.count
    }

    pub fn sum(&self) -> u64 {
        self.sum
    }

    pub fn min(&self) -> u32 {
        self.min
    }

    pub fn max(&self) -> u32 {
        self.max
    }

    pub fn stream(&self) -> u64 {
        self.stream
    }

    /// 最近秩分位（桶下界，保守且单调）。
    pub fn percentile(&self, permille: u32) -> Result<u32, u32> {
        if permille == 0 || permille > PERMILLE {
            return Err(CaseCode::ParamBad.val());
        }
        if self.count == 0 {
            return Err(CaseCode::RunBad.val());
        }
        let scaled = (self.count as u64) * (permille as u64);
        let mut rank = scaled / PERMILLE as u64;
        if scaled % PERMILLE as u64 != 0 {
            rank = rank.saturating_add(1);
        }
        if rank == 0 {
            rank = 1;
        }
        let mut cum = 0u64;
        let mut b = 0usize;
        while b < HIST_BUCKETS {
            if let Some(n) = self.buckets.get(b) {
                cum = cum.saturating_add(*n as u64);
                if cum >= rank {
                    return Ok(bucket_floor(b));
                }
            }
            b += 1;
        }
        Ok(self.max)
    }

    /// 结构自洽：桶和 = 条数，和落在 [min, max×条数]，stream 非零。
    pub fn audit(&self) -> Result<(), u32> {
        let mut total = 0u64;
        let mut i = 0usize;
        while i < HIST_BUCKETS {
            if let Some(n) = self.buckets.get(i) {
                total = total.saturating_add(*n as u64);
            }
            i += 1;
        }
        if total != self.count as u64 {
            return Err(CaseCode::RunBad.val());
        }
        if self.count == 0 || self.min == 0 || self.max < self.min || self.max > NS_LIMIT {
            return Err(CaseCode::RunBad.val());
        }
        if self.sum < self.min as u64 || self.sum > (self.max as u64) * (self.count as u64) {
            return Err(CaseCode::RunBad.val());
        }
        if self.stream == 0 {
            return Err(CaseCode::DigestBad.val());
        }
        Ok(())
    }

    /// 统计摘要：桶 + 条数 + 和 + 样本流。
    pub fn digest(&self) -> u64 {
        let mut h = p_core::FNV_OFF;
        let mut i = 0usize;
        while i < HIST_BUCKETS {
            h = p_core::fold_u32(h, self.buckets.get(i).copied().unwrap_or(0));
            i += 1;
        }
        h = p_core::fold_u32(h, self.count);
        h = fold_digest(h, self.sum);
        fold_digest(h, self.stream)
    }
}

// ── 样本合成 ──────────────────────────────────────────────────────
/// 由规格合成一条样本；高 32 位取随机量，避免低位短周期相关。
pub fn synth_ns(rng: &mut p_core::Lcg, spec: &CaseSpec) -> u32 {
    let add = SEG_ADD_NS.get(spec.seg as usize).copied().unwrap_or(0);
    let base = spec.base_ns.saturating_add(add).min(NS_LIMIT / 2);
    let r1 = rng.next();
    let r2 = rng.next();
    let jitter = ((r2 >> 33) as u32) % (spec.jitter_ns.saturating_add(1));
    let mut ns = base.saturating_add(jitter);
    let roll = ((r1 >> 32) as u32) % spec.tail_one_in.max(1);
    if roll == 0 {
        let mag = 1u32.saturating_add(((r2 >> 17) as u32) % spec.tail_factor.max(1));
        ns = ns.saturating_mul(mag);
    }
    ns.clamp(1, NS_LIMIT)
}

// ── 用例记录 ──────────────────────────────────────────────────────
/// 一条已执行用例的统计记录。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct CaseRecord {
    pub id: u32,
    pub seg: u8,
    pub n: u32,
    pub sum_ns: u64,
    pub p50: u32,
    pub p95: u32,
    pub p99: u32,
    pub stream: u64,
    pub digest: u64,
}

impl CaseRecord {
    /// 由直方图与规格构建记录；分位单调与摘要一次折定。
    pub fn build(spec: &CaseSpec, hist: &CaseHist) -> Result<CaseRecord, u32> {
        spec.validate()?;
        if hist.count() != spec.samples {
            return Err(CaseCode::RunBad.val());
        }
        let p50 = hist.percentile(500)?;
        let p95 = hist.percentile(950)?;
        let p99 = hist.percentile(990)?;
        if p50 == 0 || p50 > p95 || p95 > p99 || p99 > NS_LIMIT {
            return Err(CaseCode::CompareBad.val());
        }
        let digest = row_digest(spec.id, spec.seg as u32, spec.samples, p99);
        Ok(CaseRecord {
            id: spec.id,
            seg: spec.seg,
            n: spec.samples,
            sum_ns: hist.sum(),
            p50,
            p95,
            p99,
            stream: hist.stream(),
            digest,
        })
    }

    /// 单条记录字段自洽（不含目录链）。
    pub fn verify(&self) -> Result<(), u32> {
        if self.id == 0 || self.id > CASE_COUNT as u32 {
            return Err(CaseCode::IdBad.val());
        }
        if self.seg as usize >= SEGMENTS {
            return Err(CaseCode::SegBad.val());
        }
        if self.n < SAMPLES_MIN || self.n > SAMPLES_MAX {
            return Err(CaseCode::CountBad.val());
        }
        if self.p50 == 0 || self.p50 > self.p95 || self.p95 > self.p99 || self.p99 > NS_LIMIT {
            return Err(CaseCode::CompareBad.val());
        }
        if self.sum_ns < self.p99 as u64 {
            return Err(CaseCode::RunBad.val());
        }
        if self.stream == 0 {
            return Err(CaseCode::DigestBad.val());
        }
        let want = row_digest(self.id, self.seg as u32, self.n, self.p99);
        if want != self.digest {
            return Err(CaseCode::DigestBad.val());
        }
        Ok(())
    }
}

/// 执行一条用例；salt 只用于测试种子敏感性，生产路径恒为 0。
pub fn run_case(spec: &CaseSpec, salt: u64) -> Result<CaseRecord, u32> {
    spec.validate()?;
    let mut rng = p_core::Lcg::new(case_seed(spec.id) ^ salt);
    let mut hist = CaseHist::new();
    let mut k = 0u32;
    while k < spec.samples {
        let ns = synth_ns(&mut rng, spec);
        hist.record(ns)?;
        k = k.saturating_add(1);
    }
    hist.audit()?;
    CaseRecord::build(spec, &hist)
}

// ── 用例目录 ──────────────────────────────────────────────────────
/// 二十条用例的入库目录：号段连续、摘要成链。
#[derive(Clone, Debug)]
pub struct CaseCatalog {
    slots: Vec<CaseRecord>,
    head: u64,
    rejected: u32,
    next_id: u32,
}

impl CaseCatalog {
    pub fn new() -> CaseCatalog {
        CaseCatalog {
            slots: Vec::with_capacity(CASE_COUNT),
            head: p_core::FNV_OFF,
            rejected: 0,
            next_id: 1,
        }
    }

    pub fn records(&self) -> &[CaseRecord] {
        &self.slots
    }

    pub fn len(&self) -> usize {
        self.slots.len()
    }

    pub fn is_empty(&self) -> bool {
        self.slots.is_empty()
    }

    pub fn head(&self) -> u64 {
        self.head
    }

    pub fn rejected(&self) -> u32 {
        self.rejected
    }

    pub fn next_id(&self) -> u32 {
        self.next_id
    }

    /// 按号查记录。
    pub fn find(&self, id: u32) -> Option<&CaseRecord> {
        let mut i = 0usize;
        while i < self.slots.len() {
            if let Some(r) = self.slots.get(i) {
                if r.id == id {
                    return Some(r);
                }
            }
            i += 1;
        }
        None
    }

    /// 登记一条用例：规格 → 号域 → 重复 → 顺序 → 容量 → 执行 → 链。
    ///
    /// 重复检查先于顺序与容量：目录满时再登一号，真正的事故是「重复」而
    /// 不是「满了」；若容量闸门在前，值班人只能看到 CapacityBad，得先删
    /// 一条才知道自己撞的是哪一号。重复先报，容量只留给真正的新号。
    pub fn register(&mut self, spec: &CaseSpec) -> Result<CaseRecord, u32> {
        spec.validate()?;
        if self.find(spec.id).is_some() {
            self.rejected = self.rejected.saturating_add(1);
            return Err(CaseCode::DuplicateBad.val());
        }
        if spec.id != self.next_id {
            self.rejected = self.rejected.saturating_add(1);
            return Err(CaseCode::OrderBad.val());
        }
        if self.slots.len() >= CASE_COUNT {
            self.rejected = self.rejected.saturating_add(1);
            return Err(CaseCode::CapacityBad.val());
        }
        let rec = match run_case(spec, 0) {
            Ok(v) => v,
            Err(e) => {
                self.rejected = self.rejected.saturating_add(1);
                return Err(e);
            }
        };
        self.head = chain_digest(self.head, rec.digest);
        self.slots.push(rec);
        self.next_id = self.next_id.saturating_add(1);
        Ok(rec)
    }

    /// 目录审计：二十条、号 1..20、字段自洽、无重复、链逐条衔接。
    pub fn audit(&self) -> Result<(), u32> {
        if self.slots.len() != CASE_COUNT || self.next_id != CASE_COUNT as u32 + 1 {
            return Err(CaseCode::CatalogBad.val());
        }
        let mut head = p_core::FNV_OFF;
        let mut i = 0usize;
        while i < self.slots.len() {
            let rec = match self.slots.get(i) {
                Some(v) => v,
                None => return Err(CaseCode::CatalogBad.val()),
            };
            rec.verify()?;
            if rec.id != i as u32 + 1 {
                return Err(CaseCode::OrderBad.val());
            }
            let spec = match spec_of(rec.id) {
                Some(v) => v,
                None => return Err(CaseCode::CatalogBad.val()),
            };
            if rec.seg != spec.seg || rec.n != spec.samples {
                return Err(CaseCode::CatalogBad.val());
            }
            let mut j = 0usize;
            while j < i {
                if let Some(other) = self.slots.get(j) {
                    if other.id == rec.id {
                        return Err(CaseCode::DuplicateBad.val());
                    }
                }
                j += 1;
            }
            head = chain_digest(head, rec.digest);
            i += 1;
        }
        if head != self.head {
            return Err(CaseCode::DriftBad.val());
        }
        Ok(())
    }

    /// 目录摘要：按入库序折行摘要，再与链头合折。
    pub fn digest(&self) -> u64 {
        let mut h = p_core::FNV_OFF;
        let mut i = 0usize;
        while i < self.slots.len() {
            if let Some(rec) = self.slots.get(i) {
                h = fold_digest(h, rec.digest);
            }
            i += 1;
        }
        fold_digest(h, self.head)
    }

    /// 目录统计：总量、段分布、最慢/最快用例。
    pub fn stats(&self) -> Result<CatalogStats, u32> {
        if self.slots.is_empty() {
            return Err(CaseCode::CatalogBad.val());
        }
        let mut total_samples = 0u64;
        let mut total_sum = 0u64;
        let mut per_seg = [0u32; SEGMENTS];
        let mut slowest = 0u32;
        let mut fastest = 0u32;
        let mut slow_p99 = 0u32;
        let mut fast_p99 = u32::MAX;
        let mut i = 0usize;
        while i < self.slots.len() {
            let rec = match self.slots.get(i) {
                Some(v) => v,
                None => return Err(CaseCode::CatalogBad.val()),
            };
            total_samples = total_samples.saturating_add(rec.n as u64);
            total_sum = total_sum.saturating_add(rec.sum_ns);
            if let Some(cell) = per_seg.get_mut(rec.seg as usize) {
                *cell = cell.saturating_add(1);
            }
            if rec.p99 > slow_p99 {
                slow_p99 = rec.p99;
                slowest = rec.id;
            }
            if rec.p99 < fast_p99 {
                fast_p99 = rec.p99;
                fastest = rec.id;
            }
            i += 1;
        }
        Ok(CatalogStats {
            cases: self.slots.len() as u32,
            total_samples,
            total_sum,
            per_seg,
            slowest_id: slowest,
            slowest_p99: slow_p99,
            fastest_id: fastest,
            fastest_p99: fast_p99,
        })
    }
}

/// 目录统计（整数域，无浮点）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct CatalogStats {
    pub cases: u32,
    pub total_samples: u64,
    pub total_sum: u64,
    pub per_seg: [u32; SEGMENTS],
    pub slowest_id: u32,
    pub slowest_p99: u32,
    pub fastest_id: u32,
    pub fastest_p99: u32,
}

/// 从规格表构建完整目录并当场审计。
pub fn build_catalog() -> Result<CaseCatalog, u32> {
    let mut cat = CaseCatalog::new();
    let mut i = 0usize;
    while i < CASE_COUNT {
        let spec = match CASE_SPECS.get(i) {
            Some(v) => v,
            None => return Err(CaseCode::CatalogBad.val()),
        };
        cat.register(spec)?;
        i += 1;
    }
    cat.audit()?;
    Ok(cat)
}

// ── 五路交叉对账 ──────────────────────────────────────────────────
/// 一条用例的五路裁决：条数 / 和 / p50 / p99 / 摘要。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct RoadReport {
    pub id: u32,
    pub roads: [bool; ROADS],
    pub digest: u64,
}

impl RoadReport {
    pub fn all_green(&self) -> bool {
        let mut i = 0usize;
        while i < ROADS {
            if !self.roads.get(i).copied().unwrap_or(false) {
                return false;
            }
            i += 1;
        }
        true
    }

    pub fn green_count(&self) -> u32 {
        let mut n = 0u32;
        let mut i = 0usize;
        while i < ROADS {
            if self.roads.get(i).copied().unwrap_or(false) {
                n = n.saturating_add(1);
            }
            i += 1;
        }
        n
    }

    /// 失败路名（全绿时返回空串）。
    pub fn first_failed(&self) -> &'static str {
        let mut i = 0usize;
        while i < ROADS {
            if !self.roads.get(i).copied().unwrap_or(false) {
                return ROAD_NAMES.get(i).copied().unwrap_or("road");
            }
            i += 1;
        }
        ""
    }
}

/// 五路交叉对账：三次独立执行，分别供和路、分位路、摘要路使用。
///
/// 三次执行都从同一规格、同一水种出发；「独立」指的是取值路径独立，不是
/// 随机流不同。任何一路若借用另一路的中间变量，五路一致就退化成同义反复。
pub fn five_roads(spec: &CaseSpec) -> Result<RoadReport, u32> {
    spec.validate()?;
    let a = run_case(spec, 0)?;
    let b = run_case(spec, 0)?;
    let c = run_case(spec, 0)?;
    if a.id != spec.id || b.id != spec.id || c.id != spec.id {
        return Err(CaseCode::IdBad.val());
    }
    let row = render_case(&a);
    let row_ok = match audit_render(&row) {
        Ok(d) => d == a.digest,
        Err(_) => false,
    };
    let mut roads = [false; ROADS];
    if let Some(slot) = roads.get_mut(0) {
        *slot = a.n == spec.samples && b.n == a.n && c.n == a.n && a.n > 0;
    }
    if let Some(slot) = roads.get_mut(1) {
        *slot = a.sum_ns == b.sum_ns && a.sum_ns > 0 && b.sum_ns > 0;
    }
    if let Some(slot) = roads.get_mut(2) {
        *slot = a.p50 == c.p50 && a.p50 > 0 && a.p50 <= a.p95;
    }
    if let Some(slot) = roads.get_mut(3) {
        *slot = a.p99 == c.p99 && a.p99 >= a.p95 && a.p99 <= NS_LIMIT;
    }
    if let Some(slot) = roads.get_mut(4) {
        *slot = a.digest == b.digest && b.digest == c.digest && row_ok;
    }
    Ok(RoadReport {
        id: a.id,
        roads,
        digest: a.digest,
    })
}

/// 整目录五路对账结果。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct CrossReport {
    pub cases: u32,
    pub green_cases: u32,
    pub green_roads: u32,
    pub failed_id: u32,
    pub digest: u64,
}

impl CrossReport {
    pub fn all_green(&self) -> bool {
        self.cases == CASE_COUNT as u32
            && self.green_cases == self.cases
            && self.green_roads == self.cases * ROADS as u32
            && self.failed_id == 0
    }
}

/// 对目录内二十条用例逐条跑五路，记录首个失败号与总绿路数。
pub fn cross_catalog(cat: &CaseCatalog) -> Result<CrossReport, u32> {
    if cat.len() != CASE_COUNT {
        return Err(CaseCode::CatalogBad.val());
    }
    let mut rep = CrossReport {
        cases: 0,
        green_cases: 0,
        green_roads: 0,
        failed_id: 0,
        digest: p_core::FNV_OFF,
    };
    let mut i = 0usize;
    while i < cat.len() {
        let rec = match cat.records().get(i) {
            Some(v) => v,
            None => return Err(CaseCode::CatalogBad.val()),
        };
        let spec = match spec_of(rec.id) {
            Some(v) => v,
            None => return Err(CaseCode::CatalogBad.val()),
        };
        let road = five_roads(spec)?;
        rep.cases = rep.cases.saturating_add(1);
        rep.green_roads = rep.green_roads.saturating_add(road.green_count());
        if road.all_green() && road.digest == rec.digest {
            rep.green_cases = rep.green_cases.saturating_add(1);
        } else if rep.failed_id == 0 {
            rep.failed_id = rec.id;
        }
        rep.digest = fold_digest(rep.digest, road.digest);
        i += 1;
    }
    if rep.cases != CASE_COUNT as u32 {
        return Err(CaseCode::CatalogBad.val());
    }
    Ok(rep)
}

// ── 联轧 ×5 ───────────────────────────────────────────────────────
/// 联轧报告：轮数、全绿轮数、用例数、漂移轮数、五轮合摘要。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct RollReport {
    pub runs: u32,
    pub green: u32,
    pub cases: u32,
    pub drift: u32,
    pub digest: u64,
}

/// 联轧：重复整目录构建 + 五路对账，逐轮比较目录摘要。
///
/// 漂移计数只看**摘要**：同一规格表下，条数/和/分位若变了摘要必变；摘要
/// 相同而其它量不同的概率在整数域里不成立——摘要折的正是这些字段。
pub fn roll_catalog(times: u32) -> Result<RollReport, u32> {
    if times == 0 || times > ROLL_LIMIT {
        return Err(CaseCode::DriftBad.val());
    }
    let mut rep = RollReport {
        runs: 0,
        green: 0,
        cases: 0,
        drift: 0,
        digest: p_core::FNV_OFF,
    };
    let mut first: Option<u64> = None;
    let mut run = 0u32;
    while run < times {
        let cat = build_catalog()?;
        cat.audit()?;
        let cross = cross_catalog(&cat)?;
        if !cross.all_green() {
            return Err(CaseCode::CompareBad.val());
        }
        let d = cat.digest();
        match first {
            None => first = Some(d),
            Some(v) => {
                if v != d {
                    rep.drift = rep.drift.saturating_add(1);
                }
            }
        }
        rep.digest = fold_digest(rep.digest, d);
        rep.runs = rep.runs.saturating_add(1);
        rep.green = rep.green.saturating_add(1);
        rep.cases = cross.cases;
        run = run.saturating_add(1);
    }
    if rep.drift != 0 {
        return Err(CaseCode::DriftBad.val());
    }
    if rep.green != times || rep.runs != times {
        return Err(CaseCode::CompareBad.val());
    }
    Ok(rep)
}

// ── 账行渲染 ──────────────────────────────────────────────────────
/// 账行：`CASE|id=..|seg=..|n=..|p99=..|digest=16位小写hex`。
pub fn render_row(tag: &str, a: u32, b: u32, c: u32, d: u32, digest: u64) -> String {
    format!(
        "{}|id={}|seg={}|n={}|p99={}|digest={:016x}",
        tag, a, b, c, d, digest
    )
}

/// 十进制 u32 解析：拒绝空串、非数字、超长与溢出。
pub fn parse_u32(s: &str) -> Result<u32, u32> {
    if s.is_empty() || s.len() > 10 {
        return Err(CaseCode::RenderBad.val());
    }
    let mut v = 0u32;
    let mut i = 0usize;
    while i < s.len() {
        let b = match s.as_bytes().get(i).copied() {
            Some(v) => v,
            None => return Err(CaseCode::RenderBad.val()),
        };
        if !(b'0'..=b'9').contains(&b) {
            return Err(CaseCode::RenderBad.val());
        }
        v = match v
            .checked_mul(10)
            .and_then(|x| x.checked_add((b - b'0') as u32))
        {
            Some(x) => x,
            None => return Err(CaseCode::RenderBad.val()),
        };
        i += 1;
    }
    Ok(v)
}

/// 16 位十六进制解析（无 0x 前缀，大小写皆收）。
pub fn parse_hex16(s: &str) -> Result<u64, u32> {
    if s.len() != 16 {
        return Err(CaseCode::RenderBad.val());
    }
    let mut v = 0u64;
    let mut i = 0usize;
    while i < 16 {
        let b = match s.as_bytes().get(i).copied() {
            Some(v) => v,
            None => return Err(CaseCode::RenderBad.val()),
        };
        let d = match b {
            b'0'..=b'9' => (b - b'0') as u64,
            b'a'..=b'f' => (b - b'a') as u64 + 10,
            b'A'..=b'F' => (b - b'A') as u64 + 10,
            _ => return Err(CaseCode::RenderBad.val()),
        };
        v = match v.checked_mul(16).and_then(|x| x.checked_add(d)) {
            Some(x) => x,
            None => return Err(CaseCode::RenderBad.val()),
        };
        i += 1;
    }
    Ok(v)
}

/// 按固定键序切分六字段账行，返回 (tag, [id,seg,n,p99], digest)。
pub fn row_fields(row: &str) -> Result<(&str, [u32; 4], u64), u32> {
    let mut parts: [Option<&str>; ROW_FIELDS] = [None; ROW_FIELDS];
    let mut n = 0usize;
    let mut it = row.split('|');
    while let Some(p) = it.next() {
        if n >= ROW_FIELDS {
            return Err(CaseCode::RenderBad.val());
        }
        if let Some(slot) = parts.get_mut(n) {
            *slot = Some(p);
        }
        n += 1;
    }
    if n != ROW_FIELDS {
        return Err(CaseCode::RenderBad.val());
    }
    let tag = match parts.first().and_then(|x| *x) {
        Some(v) if !v.is_empty() && !v.contains('=') => v,
        _ => return Err(CaseCode::RenderBad.val()),
    };
    let mut vals = [0u32; 4];
    let mut i = 0usize;
    while i < ROW_KEYS.len() {
        let part = match parts.get(i + 1).and_then(|x| *x) {
            Some(v) => v,
            None => return Err(CaseCode::RenderBad.val()),
        };
        let mut sp = part.split('=');
        let key = match sp.next() {
            Some(v) => v,
            None => return Err(CaseCode::RenderBad.val()),
        };
        let want_key = match ROW_KEYS.get(i) {
            Some(v) => *v,
            None => return Err(CaseCode::RenderBad.val()),
        };
        if key != want_key {
            return Err(CaseCode::RenderBad.val());
        }
        let num = match sp.next().map(parse_u32) {
            Some(Ok(v)) => v,
            _ => return Err(CaseCode::RenderBad.val()),
        };
        if sp.next().is_some() {
            return Err(CaseCode::RenderBad.val());
        }
        if let Some(slot) = vals.get_mut(i) {
            *slot = num;
        }
        i += 1;
    }
    let dpart = match parts.get(ROW_FIELDS - 1).and_then(|x| *x) {
        Some(v) => v,
        None => return Err(CaseCode::RenderBad.val()),
    };
    let mut sp = dpart.split('=');
    if sp.next() != Some("digest") {
        return Err(CaseCode::RenderBad.val());
    }
    let digest = match sp.next().map(parse_hex16) {
        Some(Ok(v)) => v,
        _ => return Err(CaseCode::RenderBad.val()),
    };
    if sp.next().is_some() {
        return Err(CaseCode::RenderBad.val());
    }
    Ok((tag, vals, digest))
}

/// 账行审计：键序（row_fields 内）→ 范围 → 摘要复算 → 渲染往返。
pub fn audit_render(row: &str) -> Result<u64, u32> {
    let (tag, vals, digest) = row_fields(row)?;
    if tag != CASE_TAG {
        return Err(CaseCode::RenderBad.val());
    }
    let id = vals.first().copied().unwrap_or(0);
    let seg = vals.get(1).copied().unwrap_or(0);
    let n = vals.get(2).copied().unwrap_or(0);
    let p99 = vals.get(3).copied().unwrap_or(0);
    if id == 0 || id > CASE_COUNT as u32 {
        return Err(CaseCode::IdBad.val());
    }
    if seg as usize >= SEGMENTS {
        return Err(CaseCode::SegBad.val());
    }
    if n < SAMPLES_MIN || n > SAMPLES_MAX {
        return Err(CaseCode::CountBad.val());
    }
    if p99 == 0 || p99 > NS_LIMIT {
        return Err(CaseCode::RunBad.val());
    }
    let want = row_digest(id, seg, n, p99);
    if want != digest {
        return Err(CaseCode::DigestBad.val());
    }
    let again = render_row(tag, id, seg, n, p99, digest);
    if again.as_str() != row {
        return Err(CaseCode::RenderBad.val());
    }
    Ok(digest)
}

/// 单条用例渲染。
pub fn render_case(rec: &CaseRecord) -> String {
    render_row(
        CASE_TAG,
        rec.id,
        rec.seg as u32,
        rec.n,
        rec.p99,
        rec.digest,
    )
}

/// 整目录逐行渲染 + 审计，返回通过行数。
pub fn audit_all_rows(cat: &CaseCatalog) -> Result<u32, u32> {
    let mut good = 0u32;
    let mut i = 0usize;
    while i < cat.len() {
        let rec = match cat.records().get(i) {
            Some(v) => v,
            None => return Err(CaseCode::RenderBad.val()),
        };
        let row = render_case(rec);
        let got = audit_render(&row)?;
        if got != rec.digest {
            return Err(CaseCode::DriftBad.val());
        }
        good = good.saturating_add(1);
        i += 1;
    }
    if good != CASE_COUNT as u32 {
        return Err(CaseCode::CatalogBad.val());
    }
    Ok(good)
}

// ── 坏输入注入 ────────────────────────────────────────────────────
/// 六路注入账：号 / 段 / 条数 / 重复 / 跳号 / 账行。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct InjectLog {
    pub tries: [u32; 6],
    pub caught: [u32; 6],
}

impl InjectLog {
    pub fn all_caught(&self) -> bool {
        let mut i = 0usize;
        while i < 6 {
            let t = self.tries.get(i).copied().unwrap_or(0);
            let c = self.caught.get(i).copied().unwrap_or(0);
            if t == 0 || c != t {
                return false;
            }
            i += 1;
        }
        true
    }

    pub fn total(&self) -> u32 {
        let mut n = 0u32;
        let mut i = 0usize;
        while i < 6 {
            n = n.saturating_add(self.tries.get(i).copied().unwrap_or(0));
            i += 1;
        }
        n
    }
}

/// 反复注入六类坏输入，全部必须被对应闸门拒收。
pub fn inject_rounds(times: u32) -> Result<InjectLog, u32> {
    if times == 0 {
        return Err(CaseCode::RunBad.val());
    }
    let mut log = InjectLog {
        tries: [0u32; 6],
        caught: [0u32; 6],
    };
    let base_spec = match CASE_SPECS.first() {
        Some(v) => *v,
        None => return Err(CaseCode::RunBad.val()),
    };
    let mut i = 0u32;
    while i < times {
        // ① 用例号为零。
        let bad_id = CaseSpec { id: 0, ..base_spec };
        if bad_id.validate().err() == Some(CaseCode::IdBad.val()) {
            if let Some(c) = log.caught.get_mut(0) {
                *c = c.saturating_add(1);
            }
        }
        if let Some(t) = log.tries.get_mut(0) {
            *t = t.saturating_add(1);
        }
        // ② 段号越界。
        let bad_seg = CaseSpec { seg: SEGMENTS as u8, ..base_spec };
        if bad_seg.validate().err() == Some(CaseCode::SegBad.val()) {
            if let Some(c) = log.caught.get_mut(1) {
                *c = c.saturating_add(1);
            }
        }
        if let Some(t) = log.tries.get_mut(1) {
            *t = t.saturating_add(1);
        }
        // ③ 样本条数为零。
        let bad_count = CaseSpec { samples: 0, ..base_spec };
        if bad_count.validate().err() == Some(CaseCode::CountBad.val()) {
            if let Some(c) = log.caught.get_mut(2) {
                *c = c.saturating_add(1);
            }
        }
        if let Some(t) = log.tries.get_mut(2) {
            *t = t.saturating_add(1);
        }
        // ④ 重复登记一号。
        if let Ok(mut cat) = build_catalog() {
            if let Some(spec) = spec_of(1) {
                if cat.register(spec).err() == Some(CaseCode::DuplicateBad.val()) {
                    if let Some(c) = log.caught.get_mut(3) {
                        *c = c.saturating_add(1);
                    }
                }
            }
            // ⑤ 跳号登记：空目录直接登二号。
            let mut fresh = CaseCatalog::new();
            if let Some(spec) = spec_of(2) {
                if fresh.register(spec).err() == Some(CaseCode::OrderBad.val()) {
                    if let Some(c) = log.caught.get_mut(4) {
                        *c = c.saturating_add(1);
                    }
                }
            }
        }
        if let Some(t) = log.tries.get_mut(3) {
            *t = t.saturating_add(1);
        }
        if let Some(t) = log.tries.get_mut(4) {
            *t = t.saturating_add(1);
        }
        // ⑥ 账行三处篡改：键名、号、摘要。
        let row = render_row(CASE_TAG, 7, 2, 160, 4_096, row_digest(7, 2, 160, 4_096));
        let bad_key = row.replacen("p99=", "p98=", 1);
        let bad_id = row.replacen("id=7", "id=0", 1);
        let bad_dig = row.replacen("digest=", "digest0", 1);
        let rejected = audit_render(&bad_key).is_err()
            && audit_render(&bad_id).is_err()
            && audit_render(&bad_dig).is_err();
        if rejected {
            if let Some(c) = log.caught.get_mut(5) {
                *c = c.saturating_add(1);
            }
        }
        if let Some(t) = log.tries.get_mut(5) {
            *t = t.saturating_add(1);
        }
        i = i.saturating_add(1);
    }
    if !log.all_caught() {
        return Err(CaseCode::CompareBad.val());
    }
    Ok(log)
}

// ── 探针序列 ──────────────────────────────────────────────────────
/// A：二十例按号入库 + 目录审计 + 统计守恒。
pub fn probe_seq_a() -> Result<(), u32> {
    let cat = build_catalog()?;
    cat.audit()?;
    if cat.len() != CASE_COUNT {
        return Err(CaseCode::CatalogBad.val());
    }
    if cat.next_id() != CASE_COUNT as u32 + 1 {
        return Err(CaseCode::OrderBad.val());
    }
    let stats = cat.stats()?;
    if stats.cases != CASE_COUNT as u32 {
        return Err(CaseCode::CatalogBad.val());
    }
    let mut seg_total = 0u32;
    let mut i = 0usize;
    while i < SEGMENTS {
        seg_total = seg_total.saturating_add(stats.per_seg.get(i).copied().unwrap_or(0));
        i += 1;
    }
    if seg_total != CASE_COUNT as u32 {
        return Err(CaseCode::CatalogBad.val());
    }
    if stats.slowest_p99 < stats.fastest_p99 {
        return Err(CaseCode::CompareBad.val());
    }
    Ok(())
}

/// B：五路交叉 20×5 全绿，首个失败号为 0。
pub fn probe_seq_b() -> Result<(), u32> {
    let cat = build_catalog()?;
    let cross = cross_catalog(&cat)?;
    if !cross.all_green() {
        return Err(CaseCode::CompareBad.val());
    }
    if cross.green_roads != CASE_COUNT as u32 * ROADS as u32 {
        return Err(CaseCode::CompareBad.val());
    }
    if cross.failed_id != 0 {
        return Err(CaseCode::CompareBad.val());
    }
    Ok(())
}

/// C：五轮联轧零漂移 + 十五条标签互斥 + 段位闭集。
pub fn probe_seq_c() -> Result<(), u32> {
    let roll = roll_catalog(ROLL_TIMES)?;
    if roll.drift != 0 || roll.green != ROLL_TIMES || roll.runs != ROLL_TIMES {
        return Err(CaseCode::DriftBad.val());
    }
    if !labels_distinct() || !labels_stable() {
        return Err(CaseCode::CompareBad.val());
    }
    if !segment_guard() {
        return Err(CaseCode::RenderBad.val());
    }
    Ok(())
}

/// 复跑：目录两次构建摘要一致；单例两次执行逐字段一致。
pub fn replay_check() -> Result<(), u32> {
    let a = build_catalog()?;
    let b = build_catalog()?;
    if a.digest() != b.digest() || a.head() != b.head() {
        return Err(CaseCode::DriftBad.val());
    }
    let spec = match spec_of(5) {
        Some(v) => v,
        None => return Err(CaseCode::IdBad.val()),
    };
    let r1 = run_case(spec, 0)?;
    let r2 = run_case(spec, 0)?;
    if r1 != r2 {
        return Err(CaseCode::DriftBad.val());
    }
    Ok(())
}

/// 种子敏感：不同用例号必须给出不同记录；同号加盐必须改变样本流。
pub fn salt_sensitivity() -> Result<(), u32> {
    let s1 = match spec_of(1) {
        Some(v) => v,
        None => return Err(CaseCode::IdBad.val()),
    };
    let s2 = match spec_of(2) {
        Some(v) => v,
        None => return Err(CaseCode::IdBad.val()),
    };
    let a = run_case(s1, 0)?;
    let b = run_case(s2, 0)?;
    if a.stream == b.stream || a.digest == b.digest {
        return Err(CaseCode::DriftBad.val());
    }
    let c = run_case(s1, 2)?;
    if c.stream == a.stream {
        return Err(CaseCode::DriftBad.val());
    }
    let d = run_case(s1, 0)?;
    if d.stream != a.stream || d.p99 != a.p99 {
        return Err(CaseCode::DriftBad.val());
    }
    Ok(())
}

// ── 八条检查 ─────────────────────────────────────────────────────
/// 两卡各四条：二十例入库 / 五路回放 / 账行往返 / 坏例拒收 /
/// 联轧五轮 / 五路交叉 / 错误码十五 / 段位闭集。
pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut out = Vec::with_capacity(8);

    let cat = build_catalog();
    let (in_ok, in_detail) = match &cat {
        Ok(c) => {
            let audit = c.audit();
            let ok = audit.is_ok() && c.len() == CASE_COUNT && c.next_id() == CASE_COUNT as u32 + 1;
            (
                ok,
                format!(
                    "cases={} next={} audit={} head=0x{:016x}",
                    c.len(),
                    c.next_id(),
                    if audit.is_ok() { "ok" } else { "bad" },
                    c.head()
                ),
            )
        }
        Err(e) => (false, format!("err=0x{:x}", e)),
    };
    out.push(("F49073/二十例入库", in_ok, in_detail));

    let (road_ok, road_detail) = match &cat {
        Ok(c) => {
            let mut all = true;
            let mut green = 0u32;
            let mut i = 0usize;
            while i < c.len() {
                if let Some(rec) = c.records().get(i) {
                    if let Some(spec) = spec_of(rec.id) {
                        match five_roads(spec) {
                            Ok(r) => {
                                if r.all_green() && r.digest == rec.digest {
                                    green = green.saturating_add(1);
                                } else {
                                    all = false;
                                }
                            }
                            Err(_) => all = false,
                        }
                    } else {
                        all = false;
                    }
                }
                i += 1;
            }
            (
                all && green == CASE_COUNT as u32,
                format!("cases_green={}/{} roads={}", green, CASE_COUNT, ROADS),
            )
        }
        Err(e) => (false, format!("前置失败 err=0x{:x}", e)),
    };
    out.push(("F49073/五路回放", road_ok, road_detail));

    let (row_ok, row_detail) = match &cat {
        Ok(c) => match audit_all_rows(c) {
            Ok(n) => {
                let mut tamper = 0u32;
                if let Some(rec) = c.records().first() {
                    let row = render_case(rec);
                    if audit_render(&row.replacen("id=", "id0=", 1)).is_err() {
                        tamper = tamper.saturating_add(1);
                    }
                    if audit_render(&row.replacen("digest=", "digest0", 1)).is_err() {
                        tamper = tamper.saturating_add(1);
                    }
                }
                (n == CASE_COUNT as u32 && tamper == 2, format!("rows={} tamper={}", n, tamper))
            }
            Err(e) => (false, format!("err=0x{:x}", e)),
        },
        Err(e) => (false, format!("前置失败 err=0x{:x}", e)),
    };
    out.push(("F49073/账行往返", row_ok, row_detail));

    match inject_rounds(3) {
        Ok(inj) => out.push((
            "F49073/坏例拒收",
            inj.all_caught(),
            format!("tries={} caught={}", inj.total(), inj.total()),
        )),
        Err(e) => out.push(("F49073/坏例拒收", false, format!("err=0x{:x}", e))),
    }

    match roll_catalog(ROLL_TIMES) {
        Ok(r) => out.push((
            "F49074/联轧×5零漂移",
            r.drift == 0 && r.green == ROLL_TIMES && r.runs == ROLL_TIMES && r.cases == CASE_COUNT as u32,
            format!("runs={} green={} drift={} d=0x{:016x}", r.runs, r.green, r.drift, r.digest),
        )),
        Err(e) => out.push(("F49074/联轧×5零漂移", false, format!("err=0x{:x}", e))),
    }

    let (cross_ok, cross_detail) = {
        let mut all = true;
        let mut roads = 0u32;
        let mut runs = 0u32;
        let mut digest = p_core::FNV_OFF;
        let mut run = 0u32;
        while run < ROLL_TIMES {
            match build_catalog().and_then(|c| cross_catalog(&c)) {
                Ok(cr) => {
                    if !cr.all_green() {
                        all = false;
                    }
                    roads = roads.saturating_add(cr.green_roads);
                    digest = fold_digest(digest, cr.digest);
                    runs = runs.saturating_add(1);
                }
                Err(_) => all = false,
            }
            run = run.saturating_add(1);
        }
        (
            all && runs == ROLL_TIMES && roads == CASE_COUNT as u32 * ROADS as u32 * ROLL_TIMES,
            format!(
                "runs={} roads={}/{} d=0x{:016x}",
                runs,
                roads,
                CASE_COUNT as u32 * ROADS as u32 * ROLL_TIMES,
                digest
            ),
        )
    };
    out.push(("F49074/五路交叉对账", cross_ok, cross_detail));

    let labels = error_catalog();
    let lc_ok = labels.len() == ERROR_LABELS && labels_distinct() && labels_stable();
    out.push((
        "F49074/错误码15互斥",
        lc_ok,
        format!("labels={} codes={} unknown_ok={}", labels.len(), CaseCode::ALL.len(), labels_stable()),
    ));

    let guard = segment_guard()
        && detail(SEG_BASE) == UNKNOWN
        && detail(SEG_BASE + 0x0F) == UNKNOWN
        && CaseCode::ALL.len() == 14;
    out.push((
        "F49074/段位闭集",
        guard,
        format!("codes={} seg=0x{:04x}", CaseCode::ALL.len(), SEG_BASE),
    ));

    out
}

/// 落账入口：恰好八条，全部走 add_dyn。
pub fn run_unx49073_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F49073");
    let items = extra_checks();
    let mut i = 0usize;
    while i < items.len() {
        if let Some((name, passed, detail)) = items.get(i) {
            s.add_dyn(name, *passed, detail.as_str());
        }
        i += 1;
    }
    s
}

// ── 单元判据 ──────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    fn std_cat() -> CaseCatalog {
        build_catalog().expect("二十例目录必须可建")
    }

    fn spec1() -> CaseSpec {
        *spec_of(1).expect("一号规格在表")
    }

    #[test]
    fn segment_guard_is_green() {
        assert!(segment_guard());
    }

    #[test]
    fn code_values_fill_window() {
        let mut i = 0usize;
        while i < CaseCode::ALL.len() {
            let c = CaseCode::ALL[i];
            assert_eq!(c.val(), SEG_BASE + 0x01 + i as u32);
            assert!(c.val() > SEG_BASE);
            assert!(c.val() <= SEG_BASE + 0x0E);
            i += 1;
        }
        assert_eq!(CaseCode::ALL.len(), 14);
    }

    #[test]
    fn code_details_are_distinct() {
        let mut i = 0usize;
        while i < CaseCode::ALL.len() {
            let a = CaseCode::ALL[i].message();
            assert!(!a.is_empty());
            let mut j = i + 1usize;
            while j < CaseCode::ALL.len() {
                assert_ne!(a, CaseCode::ALL[j].message());
                j += 1;
            }
            i += 1;
        }
    }

    #[test]
    fn detail_unknown_outside_window() {
        assert_eq!(detail(SEG_BASE), UNKNOWN);
        assert_eq!(detail(SEG_BASE + 0x0F), UNKNOWN);
        assert_eq!(detail(0x73410), UNKNOWN);
        assert_eq!(detail(SEG_BASE + 0x01), CaseCode::IdBad.message());
    }

    #[test]
    fn spec_table_is_well_formed() {
        let mut i = 0usize;
        while i < CASE_COUNT {
            let spec = CASE_SPECS[i];
            assert_eq!(spec.id, i as u32 + 1);
            assert_eq!(spec.validate(), Ok(()));
            assert!(spec.seg < SEGMENTS as u8);
            assert!(spec.samples >= SAMPLES_MIN && spec.samples <= SAMPLES_MAX);
            i += 1;
        }
    }

    #[test]
    fn spec_lookup_bounds() {
        assert!(spec_of(0).is_none());
        assert!(spec_of(21).is_none());
        assert_eq!(spec_of(1).map(|s| s.id), Some(1));
        assert_eq!(spec_of(20).map(|s| s.id), Some(20));
    }

    #[test]
    fn case_seeds_are_distinct() {
        let mut i = 1u32;
        while i <= CASE_COUNT as u32 {
            let a = case_seed(i);
            let mut j = i + 1u32;
            while j <= CASE_COUNT as u32 {
                assert_ne!(a, case_seed(j));
                j += 1;
            }
            i += 1;
        }
    }

    #[test]
    fn spec_rejects_bad_fields() {
        let base = spec1();
        let cases = [
            CaseSpec { id: 0, ..base },
            CaseSpec { id: 21, ..base },
            CaseSpec { seg: 4, ..base },
            CaseSpec { samples: 0, ..base },
            CaseSpec { samples: SAMPLES_MAX + 1, ..base },
            CaseSpec { base_ns: 0, ..base },
            CaseSpec { base_ns: BASE_MAX_NS + 1, ..base },
            CaseSpec { jitter_ns: JITTER_MAX_NS + 1, ..base },
            CaseSpec { tail_one_in: 1, ..base },
            CaseSpec { tail_one_in: TAIL_ONE_IN_MAX + 1, ..base },
            CaseSpec { tail_factor: 0, ..base },
            CaseSpec { tail_factor: TAIL_FACTOR_MAX + 1, ..base },
        ];
        let mut i = 0usize;
        while i < cases.len() {
            assert!(cases[i].validate().is_err(), "第 {} 条坏规格未拒", i);
            i += 1;
        }
    }

    #[test]
    fn bucket_mapping_is_monotone() {
        let mut prev = 0usize;
        let mut ns = 1u32;
        while ns < 2_000_000 {
            let b = bucket_of(ns);
            assert!(b >= prev);
            prev = b;
            ns = ns.saturating_mul(2);
        }
        assert_eq!(bucket_of(0), 0);
        assert_eq!(bucket_of(2), 1);
        assert_eq!(bucket_of(64), 6);
        assert_eq!(bucket_of(1_000_000), 19);
    }

    #[test]
    fn percentile_is_monotone() {
        let mut h = CaseHist::new();
        let mut v = 1u32;
        while v <= 200 {
            h.record(v).expect("样本在域内");
            v += 1;
        }
        h.audit().expect("直方图自洽");
        let p50 = h.percentile(500).expect("p50");
        let p95 = h.percentile(950).expect("p95");
        let p99 = h.percentile(990).expect("p99");
        assert!(p50 <= p95);
        assert!(p95 <= p99);
        assert!(p50 >= 64 && p50 <= 128);
        assert_eq!(p99, 128);
    }

    #[test]
    fn histogram_rejects_bad_samples() {
        let mut h = CaseHist::new();
        assert_eq!(h.record(0), Err(CaseCode::RunBad.val()));
        assert_eq!(h.record(NS_LIMIT + 1), Err(CaseCode::RunBad.val()));
        assert_eq!(h.percentile(990), Err(CaseCode::RunBad.val()));
        assert_eq!(h.percentile(0), Err(CaseCode::ParamBad.val()));
    }

    #[test]
    fn histogram_stream_tracks_content() {
        let mut a = CaseHist::new();
        let mut b = CaseHist::new();
        a.record(100).expect("域内");
        b.record(100).expect("域内");
        b.record(200).expect("域内");
        assert_ne!(a.stream(), b.stream());
        assert_eq!(a.sum() + 200, b.sum());
        assert_ne!(a.digest(), b.digest());
    }

    #[test]
    fn synth_is_replayable() {
        let spec = spec1();
        let mut r1 = p_core::Lcg::new(case_seed(1));
        let mut r2 = p_core::Lcg::new(case_seed(1));
        let mut i = 0u32;
        while i < 64 {
            assert_eq!(synth_ns(&mut r1, &spec), synth_ns(&mut r2, &spec));
            i += 1;
        }
    }

    #[test]
    fn run_case_is_deterministic() {
        let spec = spec1();
        let a = run_case(&spec, 0).expect("一号用例可跑");
        let b = run_case(&spec, 0).expect("一号用例可再跑");
        assert_eq!(a, b);
        assert_eq!(a.n, spec.samples);
        assert!(a.p50 <= a.p95 && a.p95 <= a.p99);
    }

    #[test]
    fn run_case_rejects_bad_spec() {
        let bad = CaseSpec { samples: 0, ..spec1() };
        assert_eq!(run_case(&bad, 0), Err(CaseCode::CountBad.val()));
    }

    #[test]
    fn record_verify_catches_digest_tamper() {
        let mut rec = run_case(&spec1(), 0).expect("可跑");
        assert!(rec.verify().is_ok());
        rec.p99 = rec.p99.saturating_add(1);
        assert_eq!(rec.verify(), Err(CaseCode::DigestBad.val()));
    }

    #[test]
    fn catalog_is_complete_and_ordered() {
        let cat = std_cat();
        assert_eq!(cat.len(), CASE_COUNT);
        assert_eq!(cat.next_id(), CASE_COUNT as u32 + 1);
        assert!(cat.audit().is_ok());
        let mut i = 0usize;
        while i < cat.len() {
            assert_eq!(cat.records()[i].id, i as u32 + 1);
            i += 1;
        }
    }

    #[test]
    fn catalog_digest_replays() {
        let a = std_cat();
        let b = std_cat();
        assert_eq!(a.digest(), b.digest());
        assert_eq!(a.head(), b.head());
    }

    #[test]
    fn catalog_rejects_duplicate_id() {
        let mut cat = std_cat();
        assert_eq!(cat.register(&spec1()), Err(CaseCode::DuplicateBad.val()));
    }

    #[test]
    fn catalog_rejects_order_gap() {
        let mut cat = CaseCatalog::new();
        let s2 = *spec_of(2).expect("二号规格");
        assert_eq!(cat.register(&s2), Err(CaseCode::OrderBad.val()));
    }

    #[test]
    fn catalog_rejects_bad_spec_first() {
        let mut cat = CaseCatalog::new();
        let bad = CaseSpec { id: 0, ..spec1() };
        assert_eq!(cat.register(&bad), Err(CaseCode::IdBad.val()));
    }

    #[test]
    fn catalog_find_by_id() {
        let cat = std_cat();
        assert!(cat.find(0).is_none());
        assert!(cat.find(21).is_none());
        assert_eq!(cat.find(13).map(|r| r.id), Some(13));
    }

    #[test]
    fn catalog_stats_conserve_samples() {
        let cat = std_cat();
        let stats = cat.stats().expect("可统计");
        assert_eq!(stats.cases, CASE_COUNT as u32);
        let mut want = 0u64;
        let mut i = 0usize;
        while i < CASE_COUNT {
            want = want.saturating_add(CASE_SPECS[i].samples as u64);
            i += 1;
        }
        assert_eq!(stats.total_samples, want);
        let mut per = 0u32;
        let mut k = 0usize;
        while k < SEGMENTS {
            per = per.saturating_add(stats.per_seg[k]);
            k += 1;
        }
        assert_eq!(per, CASE_COUNT as u32);
        assert!(stats.slowest_p99 >= stats.fastest_p99);
        assert!(stats.slowest_id >= 1 && stats.slowest_id <= CASE_COUNT as u32);
        assert!(stats.fastest_id >= 1 && stats.fastest_id <= CASE_COUNT as u32);
    }

    #[test]
    fn five_roads_are_green_for_all_cases() {
        let mut i = 1u32;
        while i <= CASE_COUNT as u32 {
            let spec = spec_of(i).expect("规格在表");
            let report = five_roads(spec).expect("五路可算");
            assert!(report.all_green(), "用例 {} 路 {}", i, report.first_failed());
            assert_eq!(report.green_count(), 5);
            assert_eq!(report.digest, run_case(spec, 0).expect("可跑").digest);
            i += 1;
        }
    }

    #[test]
    fn cross_catalog_is_all_green() {
        let cross = cross_catalog(&std_cat()).expect("可对账");
        assert!(cross.all_green());
        assert_eq!(cross.cases, 20);
        assert_eq!(cross.green_cases, 20);
        assert_eq!(cross.green_roads, 100);
        assert_eq!(cross.failed_id, 0);
        assert_ne!(cross.digest, 0);
    }

    #[test]
    fn cross_catalog_rejects_partial_catalog() {
        let cat = CaseCatalog::new();
        assert_eq!(cross_catalog(&cat), Err(CaseCode::CatalogBad.val()));
    }

    #[test]
    fn roll_five_has_zero_drift() {
        let r = roll_catalog(5).expect("五轮联轧必须零漂移");
        assert_eq!(r.runs, 5);
        assert_eq!(r.green, 5);
        assert_eq!(r.drift, 0);
        assert_eq!(r.cases, 20);
        assert_ne!(r.digest, 0);
    }

    #[test]
    fn roll_rejects_bad_times() {
        assert_eq!(roll_catalog(0), Err(CaseCode::DriftBad.val()));
        assert_eq!(roll_catalog(ROLL_LIMIT + 1), Err(CaseCode::DriftBad.val()));
    }

    #[test]
    fn render_is_lowercase_sixteen_hex() {
        let row = render_row(CASE_TAG, 1, 0, 64, 2_048, row_digest(1, 0, 64, 2_048));
        assert!(row.starts_with("CASE|id=1|seg=0|n=64|p99=2048|digest="));
        let tail = &row[row.len() - 16..];
        assert_eq!(tail.len(), 16);
        assert!(tail.as_bytes().iter().all(|b| {
            (b'0'..=b'9').contains(b) || (b'a'..=b'f').contains(b)
        }));
    }

    #[test]
    fn parse_u32_accepts_and_rejects() {
        assert_eq!(parse_u32("0"), Ok(0));
        assert_eq!(parse_u32("2048"), Ok(2_048));
        assert_eq!(parse_u32("4294967295"), Ok(u32::MAX));
        assert!(parse_u32("").is_err());
        assert!(parse_u32("12x").is_err());
        assert!(parse_u32("-1").is_err());
        assert!(parse_u32("4294967296").is_err());
        assert!(parse_u32("99999999999").is_err());
    }

    #[test]
    fn parse_hex16_accepts_and_rejects() {
        assert_eq!(parse_hex16("00000000000000ff"), Ok(255));
        assert_eq!(parse_hex16("FFFFFFFFFFFFFFFF"), Ok(u64::MAX));
        assert!(parse_hex16("").is_err());
        assert!(parse_hex16("0x000000000000ff").is_err());
        assert!(parse_hex16("00000000000000fg").is_err());
        assert!(parse_hex16("00000000000000f").is_err());
    }

    #[test]
    fn row_fields_roundtrip() {
        let d = row_digest(9, 1, 96, 3_072);
        let row = render_row(CASE_TAG, 9, 1, 96, 3_072, d);
        let (tag, vals, got) = row_fields(&row).expect("六字段可切");
        assert_eq!(tag, CASE_TAG);
        assert_eq!(vals, [9, 1, 96, 3_072]);
        assert_eq!(got, d);
        assert_eq!(audit_render(&row), Ok(d));
    }

    #[test]
    fn row_fields_rejects_key_order() {
        let d = row_digest(9, 1, 96, 3_072);
        let row = format!("CASE|seg=1|id=9|n=96|p99=3072|digest={:016x}", d);
        assert_eq!(row_fields(&row), Err(CaseCode::RenderBad.val()));
    }

    #[test]
    fn row_fields_rejects_shapes() {
        assert!(row_fields("CASE|id=1").is_err());
        assert!(row_fields("CASE|id=1|seg=0|n=16|p99=1").is_err());
        assert!(row_fields("|id=1|seg=0|n=16|p99=1|digest=0000000000000001").is_err());
        assert!(row_fields("CASE|id=1|seg=0|n=16|p99=1|digest=0000000000000001|x=2").is_err());
        assert!(row_fields("CASE|id=1|seg=0|n=16|p99=1|digest=000000000000000g").is_err());
    }

    #[test]
    fn audit_render_rejects_tamper() {
        let d = row_digest(5, 2, 128, 4_096);
        let row = render_row(CASE_TAG, 5, 2, 128, 4_096, d);
        assert!(audit_render(&row).is_ok());
        let t1 = row.replacen("id=5", "id=6", 1);
        assert!(audit_render(&t1).is_err());
        let t2 = row.replacen("seg=2", "seg=3", 1);
        assert!(audit_render(&t2).is_err());
        let t3 = row.replacen("n=128", "n=129", 1);
        assert!(audit_render(&t3).is_err());
        let t4 = row.replacen("p99=4096", "p99=4097", 1);
        assert!(audit_render(&t4).is_err());
        let t5 = row.replacen("digest=", "digest0", 1);
        assert!(audit_render(&t5).is_err());
    }

    #[test]
    fn audit_render_rejects_out_of_range() {
        let d0 = row_digest(0, 0, 64, 1_024);
        assert_eq!(
            audit_render(&render_row(CASE_TAG, 0, 0, 64, 1_024, d0)),
            Err(CaseCode::IdBad.val())
        );
        let d1 = row_digest(21, 0, 64, 1_024);
        assert_eq!(
            audit_render(&render_row(CASE_TAG, 21, 0, 64, 1_024, d1)),
            Err(CaseCode::IdBad.val())
        );
        let d2 = row_digest(1, 4, 64, 1_024);
        assert_eq!(
            audit_render(&render_row(CASE_TAG, 1, 4, 64, 1_024, d2)),
            Err(CaseCode::SegBad.val())
        );
        let d3 = row_digest(1, 0, 0, 1_024);
        assert_eq!(
            audit_render(&render_row(CASE_TAG, 1, 0, 0, 1_024, d3)),
            Err(CaseCode::CountBad.val())
        );
        let d4 = row_digest(1, 0, 64, NS_LIMIT + 1);
        assert_eq!(
            audit_render(&render_row(CASE_TAG, 1, 0, 64, NS_LIMIT + 1, d4)),
            Err(CaseCode::RunBad.val())
        );
    }

    #[test]
    fn audit_all_rows_covers_catalog() {
        let n = audit_all_rows(&std_cat()).expect("二十行可回读");
        assert_eq!(n, CASE_COUNT as u32);
    }

    #[test]
    fn error_catalog_has_fifteen_labels() {
        let labels = error_catalog();
        assert_eq!(labels.len(), 15);
        assert_eq!(labels.last().copied(), Some(UNKNOWN));
        assert_eq!(labels.first().copied(), Some(CaseCode::IdBad.message()));
    }

    #[test]
    fn labels_are_distinct() {
        assert!(labels_distinct());
    }

    #[test]
    fn labels_are_stable() {
        assert!(labels_stable());
        assert_eq!(detail(SEG_BASE + 0x0E), CaseCode::CompareBad.message());
    }

    #[test]
    fn injection_catches_six_families() {
        let log = inject_rounds(3).expect("注入必须全被捕获");
        assert!(log.all_caught());
        assert_eq!(log.tries, [3, 3, 3, 3, 3, 3]);
        assert_eq!(log.total(), 18);
    }

    #[test]
    fn probe_sequence_is_green() {
        assert!(probe_seq_a().is_ok());
        assert!(probe_seq_b().is_ok());
        assert!(probe_seq_c().is_ok());
    }

    #[test]
    fn replay_and_salt_checks_are_green() {
        assert!(replay_check().is_ok());
        assert!(salt_sensitivity().is_ok());
    }

    #[test]
    fn checks_are_all_green() {
        let s = run_unx49073_checks();
        assert!(s.all_passed(), "red={}", s.red_items_text());
    }

    #[test]
    fn checks_have_exactly_eight_items() {
        let s = run_unx49073_checks();
        assert_eq!(s.len(), 8);
        assert_eq!(s.dyn_used(), 8);
        assert!(!s.truncated());
    }

    #[test]
    fn checks_passed_count_is_eight() {
        let s = run_unx49073_checks();
        assert_eq!(s.passed_count(), 8);
        assert_eq!(s.failed_count(), 0);
    }

    #[test]
    fn check_names_are_unique() {
        let s = run_unx49073_checks();
        let mut i = 0usize;
        while i < s.len() {
            let a = s.get(i).expect("检查项在册").name;
            let mut j = i + 1usize;
            while j < s.len() {
                let b = s.get(j).expect("检查项在册").name;
                assert_ne!(a, b);
                j += 1;
            }
            i += 1;
        }
    }

    #[test]
    fn check_details_are_non_empty() {
        let s = run_unx49073_checks();
        let mut i = 0usize;
        while i < s.len() {
            let d = s.detail_for_check(i);
            assert!(!d.is_empty(), "第 {} 条明细为空", i);
            i += 1;
        }
    }

    #[test]
    fn check_digest_is_recomputable() {
        let cat = std_cat();
        let rec = cat.find(7).expect("七号用例在册");
        let row = render_case(rec);
        let (tag, vals, digest) = row_fields(&row).expect("账行可切");
        assert_eq!(tag, CASE_TAG);
        assert_eq!(vals.first().copied(), Some(7));
        assert_eq!(vals.get(1).copied(), Some(rec.seg as u32));
        assert_eq!(digest, row_digest(7, rec.seg as u32, rec.n, rec.p99));
        assert_eq!(audit_render(&row), Ok(digest));
    }

    #[test]
    fn bad_rows_are_all_rejected() {
        let d = row_digest(12, 3, 88, 8_192);
        let row = render_row(CASE_TAG, 12, 3, 88, 8_192, d);
        let bads = [
            row.replacen("CASE", "CASE0", 1),
            row.replacen("id=12", "id=012", 1),
            row.replacen("seg=3", "seg=03", 1),
            row.replacen("n=88", "n=880", 1),
            row.replacen("p99=8192", "p99=8191", 1),
            row.replacen("digest=", "digest=0", 1),
        ];
        let mut i = 0usize;
        while i < bads.len() {
            assert!(audit_render(&bads[i]).is_err(), "第 {} 条坏行未被拒", i);
            i += 1;
        }
    }
}
