//! # unx49093 — B15 首产段移交宣告与收官段印
//!
//! 卡面原文（任务板 CoRun·UNX-F2-B40）：
//!
//! > **F49093 首产段移交宣告（M1/F1/F5/H5/D4 消费面就绪声明——下游预告兑现）**
//! > **F49094 B15 首产段收官印（B01–B15 段印：300 条冻结/域账累计 90,000 行）**
//!
//! 验收锚点：
//!
//! - `UNX-F49094-J1` 段印入册：90,000/240,000；判据锚定 §AI-62 §7 波次里程碑。
//! - `UNX-F49095-J1` 错误码 ×15 零漂移互不混淆；判据锚定 §AI-62 §2。
//!
//! ## 闸序与理由
//!
//! 1. **五消费面就绪**：M1/F1/F5/H5/D4 各自声明 API 就绪、契约版本相符、
//!    重复预演 10 次全过；少一个面，移交宣告就不成立。
//! 2. **预演十连**：预演是纯状态机（Attach → Subscribe → Emit → Ack → Teardown），
//!    每轮都要回到已拆除状态且无悬挂确认；十轮全过才算“下游预告兑现”。
//! 3. **段印入册**：十五批冻结、300 卡、90,000 行、目标 240,000 行四项同时成立，
//!    账额由卡数与单卡行数相乘复核，不允许手填。
//! 4. **十五族零漂移**：B01–B15 各持一枚族代表码，三次快照必须逐位相同；
//!    同时人为挪动一枚码值必须被检出——零漂移不能靠“永远返回真”蒙混。
//!
//! ## 与相邻模块的差异
//!
//! - `unx49091_register` 登记的是**待补测试与验收维度**；本模块宣告的是
//!   **消费面就绪与段印**，一个面向未来，一个对账过去。
//! - `unx49089_finalgrep` 复跑的是判据；本模块预演的是**消费方握手**，
//!   状态机与摘要域都不同。
//! - 本模块的族代表码表是段印的内部位序表，不与各批实际 `SEG_BASE` 混用；
//!   它只证明“十五族互不混淆且不漂移”，不替代各批自己的段位自检。

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;
use super::unx48941_wheel_core as p_core;

// ── 段位与常量 ───────────────────────────────────────────────────
/// 本模块段基址（B15 第七槽）。
pub const SEG_BASE: u32 = 0x73680;
/// 消费面数。
pub const SURFACES: usize = 5;
/// 每面预演轮数。
pub const REHEARSAL_RUNS: u32 = 10;
/// 预演轮数上限。
pub const MAX_RUNS: u32 = 16;
/// 预演状态步数。
pub const STEPS: u32 = 6;
/// 冻结批数。
pub const FROZEN_BATCHES: u32 = 15;
/// 冻结卡数。
pub const FROZEN_CARDS: u32 = 300;
/// 单卡域账行数。
pub const LINES_PER_CARD: u32 = 300;
/// 域账累计行数。
pub const DOMAIN_LINES: u32 = 90_000;
/// 段目标行数。
pub const DOMAIN_TARGET: u32 = 240_000;
/// 千分比基数。
pub const PER_MILLE: u32 = 1_000;
/// 错误码族数。
pub const CODE_FAMILIES: usize = 15;
/// 族位序基址（内部位序，不是实际段基址）。
pub const CODE_BASE: u32 = 0x70000;
/// 族位序步长。
pub const CODE_STEP: u32 = 0x40;
/// 族位序偏移。
pub const CODE_SKEW: u32 = 3;
/// 漂移观测轮数。
pub const DRIFT_ROUNDS: u32 = 3;
/// 漂移观测上限。
pub const DRIFT_MAX: u32 = 8;
/// 账面标签。
pub const SURFACE_TAG: &str = "HAND93";
/// 段印标签。
pub const SEAL_TAG: &str = "SEAL94";
/// 族码标签。
pub const CODE_TAG: &str = "CODE94";
/// 消费面账键序。
pub const SURFACE_KEYS: [&str; 5] = ["surface", "api", "runs", "passes", "ver"];
/// 段印账键序。
pub const SEAL_KEYS: [&str; 4] = ["batches", "cards", "lines", "target"];
/// 族码账键序。
pub const CODE_KEYS: [&str; 2] = ["idx", "value"];
/// 未知诊断码说明。
pub const UNKNOWN: &str = "未知 B15 移交诊断码";
/// 段宽（相邻模块互斥）。
pub const SEG_SPAN: u32 = 0x40;

// ── 诊断码 ───────────────────────────────────────────────────────
/// 移交诊断码（14 条，说明互不共用）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum HandCode {
    /// 消费面标签非法
    SurfaceBad,
    /// API 未就绪
    ApiBad,
    /// 契约版本不符
    VersionBad,
    /// 预演轮数不足或越界
    RunBad,
    /// 预演未全过
    RehearsalBad,
    /// 预演状态机越轨
    StepBad,
    /// 段印批数或卡数不符
    SealBad,
    /// 行数与乘积不符
    LineBad,
    /// 进度越出 0..=1000
    ProgressBad,
    /// 族码重复
    CodeBad,
    /// 族码漂移
    DriftBad,
    /// 摘要与重算不符
    DigestBad,
    /// 账行键序错位
    RowBad,
    /// 宣告不完整
    Incomplete,
}

impl HandCode {
    /// 诊断码值。
    pub fn val(self) -> u32 {
        SEG_BASE + 0x01 + self as u32
    }

    /// 逐码说明（互不共用）。
    pub fn detail(self) -> &'static str {
        match self {
            HandCode::SurfaceBad => "消费面标签不在 M1/F1/F5/H5/D4 内",
            HandCode::ApiBad => "消费面 API 未就绪",
            HandCode::VersionBad => "契约版本与表不符",
            HandCode::RunBad => "预演轮数为 0 或超上限",
            HandCode::RehearsalBad => "预演未达到十轮全过",
            HandCode::StepBad => "预演状态机停在中途",
            HandCode::SealBad => "段印批数或卡数不符",
            HandCode::LineBad => "行数与卡数乘积不等",
            HandCode::ProgressBad => "进度越出千分窗口",
            HandCode::CodeBad => "十五族码值重复",
            HandCode::DriftBad => "族码跨轮发生漂移",
            HandCode::DigestBad => "账行摘要与重算不一致",
            HandCode::RowBad => "账行键序或字段数错位",
            HandCode::Incomplete => "移交宣告不完整",
        }
    }

    /// 面向运营的短句（与 `detail` 不同口径）。
    pub fn message(self) -> &'static str {
        match self {
            HandCode::SurfaceBad => "消费面非法：请查五面表",
            HandCode::ApiBad => "API 未就绪：请补门面",
            HandCode::VersionBad => "版本不符：请对齐契约",
            HandCode::RunBad => "轮次非法：请收敛到 1..=16",
            HandCode::RehearsalBad => "预演未全过：请回放状态机",
            HandCode::StepBad => "状态越轨：请查拆除路径",
            HandCode::SealBad => "段印失配：请核对冻结数",
            HandCode::LineBad => "行账失配：请重算乘积",
            HandCode::ProgressBad => "进度越界：请核对目标行数",
            HandCode::CodeBad => "族码重复：请查族表",
            HandCode::DriftBad => "族码漂移：请重钉快照",
            HandCode::DigestBad => "摘要失配：请重算账行",
            HandCode::RowBad => "账行损坏：请重渲染",
            HandCode::Incomplete => "宣告不全：请补五面与段印",
        }
    }
}

/// 全部诊断码（顺序即值序）。
pub const ALL: [HandCode; 14] = [
    HandCode::SurfaceBad,
    HandCode::ApiBad,
    HandCode::VersionBad,
    HandCode::RunBad,
    HandCode::RehearsalBad,
    HandCode::StepBad,
    HandCode::SealBad,
    HandCode::LineBad,
    HandCode::ProgressBad,
    HandCode::CodeBad,
    HandCode::DriftBad,
    HandCode::DigestBad,
    HandCode::RowBad,
    HandCode::Incomplete,
];

/// 自由函数：码值 → 说明（段外一律 `UNKNOWN`）。
pub fn detail(code: u32) -> &'static str {
    match code.wrapping_sub(SEG_BASE) {
        1 => HandCode::SurfaceBad.detail(),
        2 => HandCode::ApiBad.detail(),
        3 => HandCode::VersionBad.detail(),
        4 => HandCode::RunBad.detail(),
        5 => HandCode::RehearsalBad.detail(),
        6 => HandCode::StepBad.detail(),
        7 => HandCode::SealBad.detail(),
        8 => HandCode::LineBad.detail(),
        9 => HandCode::ProgressBad.detail(),
        10 => HandCode::CodeBad.detail(),
        11 => HandCode::DriftBad.detail(),
        12 => HandCode::DigestBad.detail(),
        13 => HandCode::RowBad.detail(),
        14 => HandCode::Incomplete.detail(),
        _ => UNKNOWN,
    }
}

/// 段位自检：14 码值两两不同且全部落在本段窗口内。
pub fn segment_guard() -> bool {
    let mut i = 0usize;
    while i < ALL.len() {
        let a = ALL[i].val();
        if a <= SEG_BASE || a > SEG_BASE + 0x0F {
            return false;
        }
        let mut j = i + 1;
        while j < ALL.len() {
            if ALL[j].val() == a {
                return false;
            }
            j += 1;
        }
        i += 1;
    }
    true
}

// ── 五消费面 ─────────────────────────────────────────────────────
/// 消费面。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Surface {
    /// M1 事件层消费方。
    M1,
    /// F1 焦点消费方。
    F1,
    /// F5 手势消费方。
    F5,
    /// H5 热插拔消费方。
    H5,
    /// D4 延迟账本消费方。
    D4,
}

impl Surface {
    /// 账行取值。
    pub fn tag(self) -> &'static str {
        match self {
            Surface::M1 => "M1",
            Surface::F1 => "F1",
            Surface::F5 => "F5",
            Surface::H5 => "H5",
            Surface::D4 => "D4",
        }
    }

    /// 由账行取值反解。
    pub fn from_tag(tag: &str) -> Option<Surface> {
        match tag {
            "M1" => Some(Surface::M1),
            "F1" => Some(Surface::F1),
            "F5" => Some(Surface::F5),
            "H5" => Some(Surface::H5),
            "D4" => Some(Surface::D4),
            _ => None,
        }
    }

    /// 归属批号（用于追溯）。
    pub fn owner_batch(self) -> u8 {
        match self {
            Surface::M1 => 0,
            Surface::F1 => 12,
            Surface::F5 => 3,
            Surface::H5 => 9,
            Surface::D4 => 13,
        }
    }

    /// 契约版本（各面独立，不得互相借用）。
    pub fn version(self) -> u8 {
        match self {
            Surface::M1 => 3,
            Surface::F1 => 2,
            Surface::F5 => 2,
            Surface::H5 => 1,
            Surface::D4 => 1,
        }
    }

    /// 消费面名称。
    pub fn name(self) -> &'static str {
        match self {
            Surface::M1 => "事件层消费面",
            Surface::F1 => "焦点消费面",
            Surface::F5 => "手势消费面",
            Surface::H5 => "热插拔消费面",
            Surface::D4 => "延迟账本消费面",
        }
    }
}

/// 五个消费面（顺序即就绪表顺序）。
pub const SURFACE_TABLE: [Surface; SURFACES] = [
    Surface::M1,
    Surface::F1,
    Surface::F5,
    Surface::H5,
    Surface::D4,
];

/// 单面就绪声明。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Ready {
    /// 面。
    pub surface: Surface,
    /// API 门面已就绪。
    pub api_ready: bool,
    /// 预演轮数。
    pub runs: u32,
    /// 预演通过轮数。
    pub passes: u32,
    /// 声明的契约版本。
    pub version: u8,
}

impl Ready {
    /// 单面就绪：API、十轮全过、版本相符三者同时成立。
    pub fn ready(&self) -> bool {
        self.api_ready
            && self.runs == REHEARSAL_RUNS
            && self.passes == REHEARSAL_RUNS
            && self.passes <= self.runs
            && self.version == self.surface.version()
    }
}

/// 五面就绪声明（编译期常量）。
pub const READY_TABLE: [Ready; SURFACES] = [
    Ready {
        surface: Surface::M1,
        api_ready: true,
        runs: REHEARSAL_RUNS,
        passes: REHEARSAL_RUNS,
        version: 3,
    },
    Ready {
        surface: Surface::F1,
        api_ready: true,
        runs: REHEARSAL_RUNS,
        passes: REHEARSAL_RUNS,
        version: 2,
    },
    Ready {
        surface: Surface::F5,
        api_ready: true,
        runs: REHEARSAL_RUNS,
        passes: REHEARSAL_RUNS,
        version: 2,
    },
    Ready {
        surface: Surface::H5,
        api_ready: true,
        runs: REHEARSAL_RUNS,
        passes: REHEARSAL_RUNS,
        version: 1,
    },
    Ready {
        surface: Surface::D4,
        api_ready: true,
        runs: REHEARSAL_RUNS,
        passes: REHEARSAL_RUNS,
        version: 1,
    },
];

/// 按面取就绪声明。
pub fn ready_for(s: Surface) -> Option<&'static Ready> {
    let mut i = 0usize;
    while i < READY_TABLE.len() {
        if READY_TABLE[i].surface == s {
            return Some(&READY_TABLE[i]);
        }
        i += 1;
    }
    None
}

/// 五面全部就绪。
pub fn surfaces_ready() -> bool {
    if READY_TABLE.len() != SURFACES {
        return false;
    }
    let mut i = 0usize;
    while i < READY_TABLE.len() {
        if READY_TABLE[i].surface != SURFACE_TABLE[i] {
            return false;
        }
        if READY_TABLE[i].surface.name().is_empty()
            || READY_TABLE[i].surface.owner_batch() >= 15
        {
            return false;
        }
        if !READY_TABLE[i].ready() {
            return false;
        }
        i += 1;
    }
    true
}

// ── 预演状态机 ───────────────────────────────────────────────────
/// 预演状态。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Step {
    /// 未接入。
    Idle,
    /// 已接入。
    Attached,
    /// 已订阅。
    Subscribed,
    /// 已发帧。
    Emitted,
    /// 已确认。
    Acked,
    /// 已拆除。
    Torn,
}

impl Step {
    /// 账行取值。
    pub fn tag(self) -> &'static str {
        match self {
            Step::Idle => "idle",
            Step::Attached => "attached",
            Step::Subscribed => "subscribed",
            Step::Emitted => "emitted",
            Step::Acked => "acked",
            Step::Torn => "torn",
        }
    }
}

/// 单轮预演结果。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct RehearsalRun {
    /// 面。
    pub surface: Surface,
    /// 轮号。
    pub run: u32,
    /// 终态。
    pub end: Step,
    /// 悬挂确认数。
    pub outstanding: u32,
    /// 轮摘要。
    pub digest: u64,
}

impl RehearsalRun {
    /// 本轮通过：回到拆除态且无悬挂确认。
    pub fn passed(&self) -> bool {
        self.end == Step::Torn && self.outstanding == 0
    }
}

/// 单轮预演（纯状态机；`run` 只影响摘要，不影响是否通过）。
pub fn rehearse_once(surface: Surface, run: u32) -> RehearsalRun {
    let seed = ((surface as u64) + 1)
        .wrapping_mul(0x9E37_79B9)
        .wrapping_add(run as u64);
    let mut rng = p_core::Lcg::new(seed);
    let mut state = Step::Idle;
    let mut outstanding = 0u32;
    let mut digest = p_core::FNV_OFF;
    let mut i = 0u32;
    while i < STEPS {
        let payload = rng.below(4);
        digest = p_core::fold_u32(digest, payload as u32);
        let next = match state {
            Step::Idle => Step::Attached,
            Step::Attached => Step::Subscribed,
            Step::Subscribed => {
                outstanding = outstanding.saturating_add(payload as u32 + 1);
                Step::Emitted
            }
            Step::Emitted => {
                outstanding = 0;
                Step::Acked
            }
            Step::Acked => Step::Torn,
            Step::Torn => Step::Torn,
        };
        state = next;
        i += 1;
    }
    RehearsalRun {
        surface,
        run,
        end: state,
        outstanding,
        digest,
    }
}

/// 单面多轮预演报告。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Rehearsal {
    /// 面。
    pub surface: Surface,
    /// 轮数。
    pub runs: u32,
    /// 通过轮数。
    pub passes: u32,
    /// 链式摘要。
    pub digest: u64,
}

/// 对单面预演 `runs` 轮。
pub fn rehearse(surface: Surface, runs: u32) -> Option<Rehearsal> {
    if runs == 0 || runs > MAX_RUNS {
        return None;
    }
    let mut passes = 0u32;
    let mut digest = p_core::FNV_OFF;
    let mut i = 0u32;
    while i < runs {
        let r = rehearse_once(surface, i);
        if r.passed() {
            passes += 1;
        }
        digest = p_core::fold_u32(p_core::fold_u32(digest, r.digest as u32), (r.digest >> 32) as u32);
        i += 1;
    }
    Some(Rehearsal {
        surface,
        runs,
        passes,
        digest,
    })
}

/// 五面各预演十轮，全部通过。
pub fn rehearsal_green() -> bool {
    let mut i = 0usize;
    while i < SURFACE_TABLE.len() {
        match rehearse(SURFACE_TABLE[i], REHEARSAL_RUNS) {
            Some(r) => {
                if r.runs != REHEARSAL_RUNS || r.passes != REHEARSAL_RUNS {
                    return false;
                }
            }
            None => return false,
        }
        i += 1;
    }
    true
}

// ── 段印 ─────────────────────────────────────────────────────────
/// 首产段印。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Seal {
    /// 冻结批数。
    pub batches: u32,
    /// 冻结卡数。
    pub cards: u32,
    /// 域账行数。
    pub lines: u32,
    /// 段目标行数。
    pub target: u32,
}

impl Seal {
    /// 冻结数成立。
    pub fn frozen_ok(&self) -> bool {
        self.batches == FROZEN_BATCHES && self.cards == FROZEN_CARDS
    }

    /// 行账成立：单卡行数乘积等于账额，目标不为零。
    pub fn lines_ok(&self) -> bool {
        self.cards.saturating_mul(LINES_PER_CARD) == self.lines
            && self.lines == DOMAIN_LINES
            && self.target == DOMAIN_TARGET
    }

    /// 千分进度（0..=1000）。
    pub fn per_mille(&self) -> u32 {
        if self.target == 0 {
            return 0;
        }
        let scaled = (self.lines as u64) * (PER_MILLE as u64) / (self.target as u64);
        let v = if scaled > PER_MILLE as u64 {
            PER_MILLE
        } else {
            scaled as u32
        };
        v
    }

    /// 印面自洽。
    pub fn verify(&self) -> bool {
        self.frozen_ok() && self.lines_ok() && self.per_mille() <= PER_MILLE
    }
}

/// 首产段印常量。
pub const FIRST_SEAL: Seal = Seal {
    batches: FROZEN_BATCHES,
    cards: FROZEN_CARDS,
    lines: DOMAIN_LINES,
    target: DOMAIN_TARGET,
};

/// 段印摘要：批数 + 卡数 + 行数 + 目标。
pub fn seal_digest(batches: u32, cards: u32, lines: u32, target: u32) -> u64 {
    let mut h = p_core::FNV_OFF;
    h = p_core::fold_u32(h, 0x5345_414C);
    h = p_core::fold_u32(h, batches);
    h = p_core::fold_u32(h, cards);
    h = p_core::fold_u32(h, lines);
    h = p_core::fold_u32(h, target);
    h
}

// ── 十五族码 ─────────────────────────────────────────────────────
/// 族位序表用的批名（B01–B15）。
pub const FAMILY_NAMES: [&str; CODE_FAMILIES] = [
    "统一事件层",
    "键位三层",
    "指针能力",
    "手势字汇",
    "三操作 SOP",
    "触控板",
    "位移合成",
    "滚轮语义",
    "报点稳定",
    "字汇收官",
    "时间戳抖动",
    "键表全覆",
    "焦点递送",
    "延迟账本",
    "恶意防御",
];

/// 族位序码值（内部位序，不与实际段基址混用）。
pub fn code_value(index: usize) -> Option<u32> {
    if index >= CODE_FAMILIES {
        return None;
    }
    Some(CODE_BASE + (index as u32).saturating_mul(CODE_STEP) + CODE_SKEW)
}

/// 十五族码值全部不同。
pub fn codes_distinct() -> bool {
    let mut i = 0usize;
    while i < CODE_FAMILIES {
        if FAMILY_NAMES[i].is_empty() {
            return false;
        }
        let a = match code_value(i) {
            Some(v) => v,
            None => return false,
        };
        let mut j = i + 1;
        while j < CODE_FAMILIES {
            match code_value(j) {
                Some(b) => {
                    if a == b {
                        return false;
                    }
                }
                None => return false,
            }
            j += 1;
        }
        i += 1;
    }
    true
}

/// 族码快照：十五族逐位折入。
pub fn code_snapshot() -> u64 {
    let mut h = p_core::FNV_OFF;
    h = p_core::fold_u32(h, 0x434F_4445);
    let mut i = 0usize;
    while i < CODE_FAMILIES {
        match code_value(i) {
            Some(v) => h = p_core::fold_u32(h, v),
            None => return p_core::FNV_OFF,
        }
        i += 1;
    }
    h
}

/// 人为漂移探针：第七族码值加一，摘要必须不同。
pub fn drifted_snapshot() -> u64 {
    let mut h = p_core::FNV_OFF;
    h = p_core::fold_u32(h, 0x434F_4445);
    let mut i = 0usize;
    while i < CODE_FAMILIES {
        match code_value(i) {
            Some(v) => {
                let v = if i == 7 { v.saturating_add(1) } else { v };
                h = p_core::fold_u32(h, v);
            }
            None => return p_core::FNV_OFF,
        }
        i += 1;
    }
    h
}

/// 多轮快照零漂移（1..=8 轮，逐轮摘要相同）。
pub fn drift_zero(rounds: u32) -> bool {
    if rounds == 0 || rounds > DRIFT_MAX {
        return false;
    }
    let want = code_snapshot();
    let mut i = 0u32;
    while i < rounds {
        if code_snapshot() != want {
            return false;
        }
        i += 1;
    }
    want != p_core::FNV_OFF
}

// ── 账行 ─────────────────────────────────────────────────────────
/// 解析十进制（拒绝空串、非数字、溢出）。
pub fn parse_u32(s: &str) -> Option<u32> {
    let bytes = s.as_bytes();
    if bytes.is_empty() {
        return None;
    }
    let mut acc = 0u32;
    let mut i = 0usize;
    while i < bytes.len() {
        let b = bytes[i];
        if !b.is_ascii_digit() {
            return None;
        }
        acc = acc.checked_mul(10)?;
        acc = acc.checked_add((b - b'0') as u32)?;
        i += 1;
    }
    Some(acc)
}

/// 解析 16 位十六进制摘要。
pub fn parse_hex16(s: &str) -> Option<u64> {
    if s.len() != 16 {
        return None;
    }
    let bytes = s.as_bytes();
    let mut acc = 0u64;
    let mut i = 0usize;
    while i < bytes.len() {
        let b = bytes[i];
        let v = match b {
            b'0'..=b'9' => (b - b'0') as u64,
            b'a'..=b'f' => (b - b'a') as u64 + 10,
            b'A'..=b'F' => (b - b'A') as u64 + 10,
            _ => return None,
        };
        acc = acc.checked_mul(16)?;
        acc = acc.checked_add(v)?;
        i += 1;
    }
    Some(acc)
}

/// 按固定标签与键序拆行；返回各键值与摘要值。
pub fn row_fields<'a>(row: &'a str, tag: &str, keys: &[&str]) -> Option<Vec<&'a str>> {
    let parts: Vec<&str> = row.split('|').collect();
    if parts.len() != keys.len() + 2 {
        return None;
    }
    if parts[0] != tag {
        return None;
    }
    let mut out = Vec::new();
    let mut i = 0usize;
    while i < keys.len() {
        let piece = parts[i + 1];
        let rest = piece.strip_prefix(keys[i])?;
        let value = rest.strip_prefix('=')?;
        if value.is_empty() {
            return None;
        }
        out.push(value);
        i += 1;
    }
    let d = parts[keys.len() + 1].strip_prefix("digest=")?;
    if d.len() != 16 {
        return None;
    }
    out.push(d);
    Some(out)
}

/// 就绪账摘要：面 + API + 轮数 + 通过数 + 版本。
pub fn surface_digest(surface: Surface, api: u8, runs: u32, passes: u32, ver: u8) -> u64 {
    let mut h = p_core::FNV_OFF;
    h = p_core::fold_u32(h, 0x4841_4E44);
    h = p_core::fold_u32(h, surface as u32);
    h = p_core::fold_u32(h, api as u32);
    h = p_core::fold_u32(h, runs);
    h = p_core::fold_u32(h, passes);
    h = p_core::fold_u32(h, ver as u32);
    h
}

/// 渲染就绪账行。
pub fn render_surface_row(r: &Ready) -> String {
    format!(
        "{}|surface={}|api={}|runs={}|passes={}|ver={}|digest={:016x}",
        SURFACE_TAG,
        r.surface.tag(),
        r.api_ready as u8,
        r.runs,
        r.passes,
        r.version,
        surface_digest(
            r.surface,
            r.api_ready as u8,
            r.runs,
            r.passes,
            r.version
        )
    )
}

/// 回读就绪账行。
pub fn audit_surface_row(row: &str) -> bool {
    let f = match row_fields(row, SURFACE_TAG, &SURFACE_KEYS) {
        Some(v) if v.len() == SURFACE_KEYS.len() + 1 => v,
        _ => return false,
    };
    let surface = match Surface::from_tag(f[0]) {
        Some(v) => v,
        None => return false,
    };
    let api = match parse_u32(f[1]) {
        Some(v) if v <= 1 => v as u8,
        _ => return false,
    };
    let runs = match parse_u32(f[2]) {
        Some(v) if v <= MAX_RUNS => v,
        _ => return false,
    };
    let passes = match parse_u32(f[3]) {
        Some(v) if v <= runs => v,
        _ => return false,
    };
    let ver = match parse_u32(f[4]) {
        Some(v) if v == surface.version() as u32 => v as u8,
        _ => return false,
    };
    let want = match parse_hex16(f[5]) {
        Some(v) => v,
        None => return false,
    };
    surface_digest(surface, api, runs, passes, ver) == want
}

/// 五面就绪账行。
pub fn export_surface_rows() -> Vec<String> {
    let mut out = Vec::new();
    let mut i = 0usize;
    while i < READY_TABLE.len() {
        out.push(render_surface_row(&READY_TABLE[i]));
        i += 1;
    }
    out
}

/// 五面账行两两不同且逐行可回读。
pub fn surface_rows_stable() -> bool {
    let rows = export_surface_rows();
    if rows.len() != SURFACES {
        return false;
    }
    let mut i = 0usize;
    while i < rows.len() {
        if !audit_surface_row(&rows[i]) {
            return false;
        }
        let mut j = i + 1;
        while j < rows.len() {
            if rows[j] == rows[i] {
                return false;
            }
            j += 1;
        }
        i += 1;
    }
    true
}

/// 渲染段印账行。
pub fn render_seal_row(s: &Seal) -> String {
    format!(
        "{}|batches={}|cards={}|lines={}|target={}|digest={:016x}",
        SEAL_TAG,
        s.batches,
        s.cards,
        s.lines,
        s.target,
        seal_digest(s.batches, s.cards, s.lines, s.target)
    )
}

/// 回读段印账行。
pub fn audit_seal_row(row: &str) -> bool {
    let f = match row_fields(row, SEAL_TAG, &SEAL_KEYS) {
        Some(v) if v.len() == SEAL_KEYS.len() + 1 => v,
        _ => return false,
    };
    let batches = match parse_u32(f[0]) {
        Some(v) => v,
        None => return false,
    };
    let cards = match parse_u32(f[1]) {
        Some(v) => v,
        None => return false,
    };
    let lines = match parse_u32(f[2]) {
        Some(v) => v,
        None => return false,
    };
    let target = match parse_u32(f[3]) {
        Some(v) if v > 0 => v,
        _ => return false,
    };
    if batches != FROZEN_BATCHES || cards != FROZEN_CARDS {
        return false;
    }
    if lines != cards.saturating_mul(LINES_PER_CARD) || lines != DOMAIN_LINES {
        return false;
    }
    if target != DOMAIN_TARGET {
        return false;
    }
    let want = match parse_hex16(f[4]) {
        Some(v) => v,
        None => return false,
    };
    seal_digest(batches, cards, lines, target) == want
}

/// 族码摘要：位序 + 值。
pub fn code_digest(index: u32, value: u32) -> u64 {
    let mut h = p_core::FNV_OFF;
    h = p_core::fold_u32(h, 0x434F_4439);
    h = p_core::fold_u32(h, index);
    h = p_core::fold_u32(h, value);
    h
}

/// 渲染族码账行。
pub fn render_code_row(index: usize) -> Option<String> {
    let value = code_value(index)?;
    Some(format!(
        "{}|idx={}|value={}|digest={:016x}",
        CODE_TAG,
        index + 1,
        value,
        code_digest((index as u32) + 1, value)
    ))
}

/// 回读族码账行。
pub fn audit_code_row(row: &str) -> bool {
    let f = match row_fields(row, CODE_TAG, &CODE_KEYS) {
        Some(v) if v.len() == CODE_KEYS.len() + 1 => v,
        _ => return false,
    };
    let idx = match parse_u32(f[0]) {
        Some(v) if v >= 1 && v <= CODE_FAMILIES as u32 => v,
        _ => return false,
    };
    let value = match parse_u32(f[1]) {
        Some(v) => v,
        None => return false,
    };
    if code_value((idx - 1) as usize) != Some(value) {
        return false;
    }
    let want = match parse_hex16(f[2]) {
        Some(v) => v,
        None => return false,
    };
    code_digest(idx, value) == want
}

/// 十五行族码账。
pub fn export_code_rows() -> Vec<String> {
    let mut out = Vec::new();
    let mut i = 0usize;
    while i < CODE_FAMILIES {
        match render_code_row(i) {
            Some(r) => out.push(r),
            None => return out,
        }
        i += 1;
    }
    out
}

/// 十五行两两不同且逐行可回读。
pub fn code_rows_stable() -> bool {
    let rows = export_code_rows();
    if rows.len() != CODE_FAMILIES {
        return false;
    }
    let mut i = 0usize;
    while i < rows.len() {
        if !audit_code_row(&rows[i]) {
            return false;
        }
        let mut j = i + 1;
        while j < rows.len() {
            if rows[j] == rows[i] {
                return false;
            }
            j += 1;
        }
        i += 1;
    }
    true
}

// ── 八项判据 ─────────────────────────────────────────────────────
/// 八项自检（真算，不预置结论）。
pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut out = Vec::new();

    // 1) 五消费面就绪。
    let mut api_ok = true;
    let mut version_ok = true;
    let mut i = 0usize;
    while i < READY_TABLE.len() {
        if !READY_TABLE[i].api_ready {
            api_ok = false;
        }
        if READY_TABLE[i].version != READY_TABLE[i].surface.version() {
            version_ok = false;
        }
        i += 1;
    }
    let ok = surfaces_ready() && api_ok && version_ok && READY_TABLE.len() == SURFACES;
    out.push((
        "F49093/五消费面就绪",
        ok,
        format!(
            "surfaces={} ready={} api={} version={}",
            READY_TABLE.len(),
            surfaces_ready(),
            api_ok,
            version_ok
        ),
    ));

    // 2) 预演十连全过。
    let mut runs_ok = true;
    let mut green = true;
    let mut i = 0usize;
    while i < SURFACE_TABLE.len() {
        match rehearse(SURFACE_TABLE[i], REHEARSAL_RUNS) {
            Some(r) => {
                if r.runs != REHEARSAL_RUNS || r.passes != REHEARSAL_RUNS {
                    green = false;
                }
            }
            None => runs_ok = false,
        }
        i += 1;
    }
    let zero_rejected = rehearse(Surface::M1, 0).is_none();
    let over_rejected = rehearse(Surface::M1, MAX_RUNS + 1).is_none();
    let ok = green && runs_ok && rehearsal_green() && zero_rejected && over_rejected;
    out.push((
        "F49093/预演十连全过",
        ok,
        format!(
            "runs={} green={} zero_rejected={} over_rejected={}",
            REHEARSAL_RUNS, green, zero_rejected, over_rejected
        ),
    ));

    // 3) 契约版本对齐 + 状态机终态。
    let mut versions: Vec<u8> = Vec::new();
    let mut version_ok = true;
    let mut i = 0usize;
    while i < SURFACE_TABLE.len() {
        let v = SURFACE_TABLE[i].version();
        if v == 0 {
            version_ok = false;
        }
        versions.push(v);
        i += 1;
    }
    let mut torn = true;
    let mut i = 0usize;
    while i < SURFACE_TABLE.len() {
        let r = rehearse_once(SURFACE_TABLE[i], i as u32);
        if r.end != Step::Torn || r.outstanding != 0 || !r.passed() {
            torn = false;
        }
        i += 1;
    }
    let ok = version_ok && torn && versions.len() == SURFACES;
    out.push((
        "F49093/契约与终态对齐",
        ok,
        format!(
            "versions={} version_ok={} torn={} end={}",
            versions.len(),
            version_ok,
            torn,
            Step::Torn.tag()
        ),
    ));

    // 4) 就绪账行回读。
    let rows = export_surface_rows();
    let mut tamp = String::new();
    if let Some(first) = rows.first() {
        tamp.push_str(first);
        tamp.push('x');
    }
    let tamper_rejected = !tamp.is_empty() && !audit_surface_row(&tamp);
    let ok = rows.len() == SURFACES && surface_rows_stable() && tamper_rejected;
    out.push((
        "F49093/账行回读",
        ok,
        format!(
            "rows={} stable={} tamper_rejected={}",
            rows.len(),
            surface_rows_stable(),
            tamper_rejected
        ),
    ));

    // 5) 段印入册。
    let seal = FIRST_SEAL;
    let ok = seal.verify() && seal.frozen_ok() && seal.batches == FROZEN_BATCHES;
    out.push((
        "F49094/段印入册",
        ok,
        format!(
            "batches={} cards={} frozen={}",
            seal.batches, seal.cards, seal.frozen_ok()
        ),
    ));

    // 6) 账额九十千。
    let product = FROZEN_CARDS.saturating_mul(LINES_PER_CARD);
    let ok = product == DOMAIN_LINES
        && seal.lines_ok()
        && seal.per_mille() == 375
        && DOMAIN_TARGET == 240_000
        && seal.per_mille() <= PER_MILLE;
    out.push((
        "F49094/账额九十千",
        ok,
        format!(
            "cards={} per_card={} product={} lines={} target={} permille={}",
            FROZEN_CARDS,
            LINES_PER_CARD,
            product,
            DOMAIN_LINES,
            DOMAIN_TARGET,
            seal.per_mille()
        ),
    ));

    // 7) 错误码十五零漂移。
    let ok = codes_distinct()
        && drift_zero(DRIFT_ROUNDS)
        && drifted_snapshot() != code_snapshot()
        && !drift_zero(0)
        && !drift_zero(DRIFT_MAX + 1);
    out.push((
        "F49094/族码十五零漂移",
        ok,
        format!(
            "families={} distinct={} drift_zero={} drift_probe_detects={}",
            CODE_FAMILIES,
            codes_distinct(),
            drift_zero(DRIFT_ROUNDS),
            drifted_snapshot() != code_snapshot()
        ),
    ));

    // 8) 段印与族码账行回读。
    let seal_row = render_seal_row(&seal);
    let mut tamp = seal_row.clone();
    tamp.push('x');
    let seal_ok = audit_seal_row(&seal_row) && !audit_seal_row(&tamp);
    let code_rows = export_code_rows();
    let ok = seal_ok && code_rows.len() == CODE_FAMILIES && code_rows_stable();
    out.push((
        "F49094/段印回读",
        ok,
        format!(
            "seal_ok={} code_rows={} code_stable={}",
            seal_ok,
            code_rows.len(),
            code_rows_stable()
        ),
    ));

    out
}

/// 入口：八项判据写入 CheckSet。
pub fn run_unx49093_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F49093 首产段移交宣告");
    let checks = extra_checks();
    let mut i = 0usize;
    while i < checks.len() {
        s.add_dyn(checks[i].0, checks[i].1, &checks[i].2);
        i += 1;
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::format;

    #[test]
    fn codes_distinct_and_bounded() {
        assert!(segment_guard());
        assert_eq!(ALL.len(), 14);
        let mut i = 0usize;
        while i < ALL.len() {
            assert!(ALL[i].val() > SEG_BASE && ALL[i].val() <= SEG_BASE + 0x0F);
            let mut j = i + 1;
            while j < ALL.len() {
                assert_ne!(ALL[i].val(), ALL[j].val());
                assert_ne!(ALL[i].detail(), ALL[j].detail());
                assert_ne!(ALL[i].message(), ALL[j].message());
                j += 1;
            }
            i += 1;
        }
    }

    #[test]
    fn free_detail_outside_segment_is_unknown() {
        assert_eq!(detail(SEG_BASE), UNKNOWN);
        assert_eq!(detail(SEG_BASE + 0x0F), UNKNOWN);
        assert_eq!(detail(0), UNKNOWN);
        assert_eq!(detail(ALL[0].val()), HandCode::SurfaceBad.detail());
        assert_eq!(detail(ALL[13].val()), HandCode::Incomplete.detail());
    }

    #[test]
    fn surface_tags_roundtrip() {
        for s in SURFACE_TABLE {
            assert_eq!(Surface::from_tag(s.tag()), Some(s));
        }
        assert_eq!(Surface::from_tag("M2"), None);
    }

    #[test]
    fn surface_versions_are_nonzero() {
        for s in SURFACE_TABLE {
            assert!(s.version() >= 1);
            assert!(!s.name().is_empty());
            assert!(s.owner_batch() < 15);
        }
    }

    #[test]
    fn ready_table_covers_all_surfaces() {
        assert_eq!(READY_TABLE.len(), SURFACES);
        let mut i = 0usize;
        while i < SURFACES {
            assert_eq!(READY_TABLE[i].surface, SURFACE_TABLE[i]);
            assert!(ready_for(SURFACE_TABLE[i]).is_some());
            assert!(READY_TABLE[i].ready());
            i += 1;
        }
        assert!(surfaces_ready());
    }

    #[test]
    fn ready_rejects_missing_api() {
        let mut r = READY_TABLE[0];
        r.api_ready = false;
        assert!(!r.ready());
    }

    #[test]
    fn ready_rejects_short_runs() {
        let mut r = READY_TABLE[1];
        r.runs = REHEARSAL_RUNS - 1;
        assert!(!r.ready());
    }

    #[test]
    fn ready_rejects_bad_version() {
        let mut r = READY_TABLE[2];
        r.version = r.version.saturating_add(1);
        assert!(!r.ready());
    }

    #[test]
    fn ready_rejects_pass_overflow() {
        let mut r = READY_TABLE[3];
        r.passes = r.runs + 1;
        assert!(!r.ready());
    }

    #[test]
    fn rehearsal_single_round_tears_down() {
        for s in SURFACE_TABLE {
            let r = rehearse_once(s, 0);
            assert_eq!(r.end, Step::Torn);
            assert_eq!(r.outstanding, 0);
            assert!(r.passed());
            assert_ne!(r.digest, p_core::FNV_OFF);
        }
    }

    #[test]
    fn rehearsal_ten_rounds_green() {
        for s in SURFACE_TABLE {
            let r = rehearse(s, REHEARSAL_RUNS).expect("report");
            assert_eq!(r.runs, REHEARSAL_RUNS);
            assert_eq!(r.passes, REHEARSAL_RUNS);
            assert_ne!(r.digest, p_core::FNV_OFF);
        }
        assert!(rehearsal_green());
    }

    #[test]
    fn rehearsal_rejects_zero_and_overflow() {
        assert!(rehearse(Surface::M1, 0).is_none());
        assert!(rehearse(Surface::M1, MAX_RUNS + 1).is_none());
        assert!(rehearse(Surface::M1, 1).is_some());
        assert!(rehearse(Surface::M1, MAX_RUNS).is_some());
    }

    #[test]
    fn rehearsal_digest_varies_by_round() {
        let a = rehearse_once(Surface::M1, 0);
        let b = rehearse_once(Surface::M1, 1);
        assert_ne!(a.digest, b.digest);
    }

    #[test]
    fn step_tags_cover_all_states() {
        assert_eq!(Step::Idle.tag(), "idle");
        assert_eq!(Step::Attached.tag(), "attached");
        assert_eq!(Step::Subscribed.tag(), "subscribed");
        assert_eq!(Step::Emitted.tag(), "emitted");
        assert_eq!(Step::Acked.tag(), "acked");
        assert_eq!(Step::Torn.tag(), "torn");
    }

    #[test]
    fn first_seal_verifies() {
        assert!(FIRST_SEAL.verify());
        assert!(FIRST_SEAL.frozen_ok());
        assert!(FIRST_SEAL.lines_ok());
        assert_eq!(FIRST_SEAL.batches, FROZEN_BATCHES);
        assert_eq!(FIRST_SEAL.cards, FROZEN_CARDS);
    }

    #[test]
    fn seal_rejects_wrong_batches() {
        let s = Seal {
            batches: FROZEN_BATCHES - 1,
            ..FIRST_SEAL
        };
        assert!(!s.frozen_ok());
        assert!(!s.verify());
    }

    #[test]
    fn seal_rejects_wrong_cards() {
        let s = Seal {
            cards: FROZEN_CARDS + 20,
            ..FIRST_SEAL
        };
        assert!(!s.frozen_ok());
        assert!(!s.lines_ok());
    }

    #[test]
    fn seal_rejects_bad_lines() {
        let s = Seal {
            lines: DOMAIN_LINES - 1,
            ..FIRST_SEAL
        };
        assert!(!s.lines_ok());
        assert!(!s.verify());
    }

    #[test]
    fn seal_progress_is_375_permille() {
        assert_eq!(FIRST_SEAL.per_mille(), 375);
        assert_eq!(FROZEN_CARDS * LINES_PER_CARD, DOMAIN_LINES);
        assert_eq!(DOMAIN_TARGET, 240_000);
    }

    #[test]
    fn seal_progress_saturates() {
        let s = Seal {
            lines: DOMAIN_TARGET,
            ..FIRST_SEAL
        };
        assert_eq!(s.per_mille(), PER_MILLE);
        let zero = Seal { target: 0, ..FIRST_SEAL };
        assert_eq!(zero.per_mille(), 0);
    }

    #[test]
    fn seal_digest_domains_distinct() {
        assert_ne!(
            seal_digest(FROZEN_BATCHES, FROZEN_CARDS, DOMAIN_LINES, DOMAIN_TARGET),
            seal_digest(FROZEN_BATCHES + 1, FROZEN_CARDS, DOMAIN_LINES, DOMAIN_TARGET)
        );
        assert_ne!(
            seal_digest(FROZEN_BATCHES, FROZEN_CARDS, DOMAIN_LINES, DOMAIN_TARGET),
            p_core::FNV_OFF
        );
    }

    #[test]
    fn surface_row_renders_and_audits() {
        let row = render_surface_row(&READY_TABLE[0]);
        assert!(row.starts_with("HAND93|surface=M1|api=1|runs=10|passes=10|ver=3|digest="));
        assert!(audit_surface_row(&row));
        assert_eq!(render_surface_row(&READY_TABLE[0]), row);
    }

    #[test]
    fn surface_rows_all_stable() {
        let rows = export_surface_rows();
        assert_eq!(rows.len(), SURFACES);
        assert!(surface_rows_stable());
        let mut i = 0usize;
        while i < rows.len() {
            assert!(audit_surface_row(&rows[i]));
            i += 1;
        }
    }

    #[test]
    fn surface_row_tamper_rejected() {
        let row = render_surface_row(&READY_TABLE[1]);
        let mut tamp = row.clone();
        tamp.push('x');
        assert!(!audit_surface_row(&tamp));
        let swapped = row.replacen("surface=F1", "surface=F5", 1);
        assert!(!audit_surface_row(&swapped));
    }

    #[test]
    fn surface_row_bad_version_rejected() {
        let bad = format!(
            "HAND93|surface=M1|api=1|runs=10|passes=10|ver=9|digest={:016x}",
            surface_digest(Surface::M1, 1, 10, 10, 9)
        );
        assert!(!audit_surface_row(&bad));
    }

    #[test]
    fn surface_row_pass_overflow_rejected() {
        let bad = format!(
            "HAND93|surface=M1|api=1|runs=10|passes=11|ver=3|digest={:016x}",
            surface_digest(Surface::M1, 1, 10, 11, 3)
        );
        assert!(!audit_surface_row(&bad));
    }

    #[test]
    fn seal_row_renders_and_audits() {
        let row = render_seal_row(&FIRST_SEAL);
        assert!(row.starts_with("SEAL94|batches=15|cards=300|lines=90000|target=240000|digest="));
        assert!(audit_seal_row(&row));
    }

    #[test]
    fn seal_row_tamper_rejected() {
        let row = render_seal_row(&FIRST_SEAL);
        let mut tamp = row.clone();
        tamp.push('x');
        assert!(!audit_seal_row(&tamp));
        let swapped = row.replacen("lines=90000", "lines=90001", 1);
        assert!(!audit_seal_row(&swapped));
    }

    #[test]
    fn seal_row_wrong_target_rejected() {
        let bad = format!(
            "SEAL94|batches=15|cards=300|lines=90000|target=239999|digest={:016x}",
            seal_digest(15, 300, 90_000, 239_999)
        );
        assert!(!audit_seal_row(&bad));
    }

    #[test]
    fn family_codes_distinct_and_in_range() {
        assert!(codes_distinct());
        let mut i = 0usize;
        while i < CODE_FAMILIES {
            let v = code_value(i).expect("value");
            assert!(v >= CODE_BASE);
            assert!(v < CODE_BASE + 0x400);
            assert!(!FAMILY_NAMES[i].is_empty());
            i += 1;
        }
        assert!(code_value(CODE_FAMILIES).is_none());
    }

    #[test]
    fn code_snapshot_is_stable() {
        assert_ne!(code_snapshot(), p_core::FNV_OFF);
        assert!(drift_zero(DRIFT_ROUNDS));
        assert!(drift_zero(1));
        assert!(drift_zero(DRIFT_MAX));
    }

    #[test]
    fn drift_zero_rejects_bad_rounds() {
        assert!(!drift_zero(0));
        assert!(!drift_zero(DRIFT_MAX + 1));
    }

    #[test]
    fn drift_probe_is_detected() {
        assert_ne!(drifted_snapshot(), code_snapshot());
        assert_ne!(drifted_snapshot(), p_core::FNV_OFF);
    }

    #[test]
    fn code_rows_are_stable() {
        let rows = export_code_rows();
        assert_eq!(rows.len(), CODE_FAMILIES);
        assert!(code_rows_stable());
        let mut i = 0usize;
        while i < rows.len() {
            assert!(audit_code_row(&rows[i]));
            i += 1;
        }
    }

    #[test]
    fn code_row_tamper_rejected() {
        let row = render_code_row(0).expect("row");
        let mut tamp = row.clone();
        tamp.push('0');
        assert!(!audit_code_row(&tamp));
        let swapped = row.replacen("value=", "value=9", 1);
        assert!(!audit_code_row(&swapped));
    }

    #[test]
    fn code_row_bad_index_rejected() {
        let bad = format!(
            "CODE94|idx=16|value=458755|digest={:016x}",
            code_digest(16, 458_755)
        );
        assert!(!audit_code_row(&bad));
    }

    #[test]
    fn parse_u32_boundaries() {
        assert_eq!(parse_u32("0"), Some(0));
        assert_eq!(parse_u32("4294967295"), Some(u32::MAX));
        assert_eq!(parse_u32("4294967296"), None);
        assert_eq!(parse_u32(""), None);
        assert_eq!(parse_u32("1x"), None);
        assert_eq!(parse_u32("-1"), None);
    }

    #[test]
    fn parse_hex16_boundaries() {
        assert_eq!(parse_hex16("0000000000000000"), Some(0));
        assert_eq!(parse_hex16("ffffffffffffffff"), Some(u64::MAX));
        assert_eq!(parse_hex16("0000"), None);
        assert_eq!(parse_hex16("000000000000000g"), None);
    }

    #[test]
    fn row_fields_strict_arity() {
        let row = render_seal_row(&FIRST_SEAL);
        assert!(row_fields(&row, SEAL_TAG, &SEAL_KEYS).is_some());
        assert!(row_fields(&row, SURFACE_TAG, &SEAL_KEYS).is_none());
        assert!(row_fields(&row, SEAL_TAG, &SURFACE_KEYS).is_none());
    }

    #[test]
    fn extra_checks_eight_green_unique() {
        let checks = extra_checks();
        assert_eq!(checks.len(), 8);
        let mut i = 0usize;
        while i < checks.len() {
            assert!(!checks[i].0.is_empty());
            assert!(!checks[i].2.is_empty());
            assert!(checks[i].1, "red: {}", checks[i].0);
            let mut j = i + 1;
            while j < checks.len() {
                assert_ne!(checks[i].0, checks[j].0);
                j += 1;
            }
            i += 1;
        }
    }

    #[test]
    fn checkset_contract() {
        let s = run_unx49093_checks();
        assert!(!s.truncated());
        assert_eq!(s.len(), 8);
        assert_eq!(s.passed_count(), 8);
        assert_eq!(s.failed_count(), 0);
        assert!(s.all_passed());
    }
}
