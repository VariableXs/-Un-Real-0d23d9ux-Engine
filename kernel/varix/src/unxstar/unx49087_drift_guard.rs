//! # unx49087 — B15 内存漂移前哨与十五批错误码合并表
//!
//! 卡面原文（任务板 CoRun·UNX-F2-B40）：
//!
//! > **F49087 内存漂移前哨账（宿主侧 2× 峰值 ×1 小时前哨账；7×24 零崩溃+漂移 <8MB 随闸门补测登记——诚实三态）**
//! > **F49088 事件层错误码全域合并表（B01–B15 十五批错误码总成零重复）**
//!
//! 验收锚点：
//!
//! - `UNX-F49087-J1` 前哨 ×1 轮漂移 <8MB 等比登记（7×24 不虚报）；判据锚定 §AI-62 §5 J-5 诚实三态。
//! - `UNX-F49088-J1` 合并表零重复断言；判据锚定 §AI-62 §2。
//!
//! ## 闸序与理由
//!
//! 1. **三态诚实**：每一条登记只能落在“已达成 / 已登记补测 / 未知”之一，
//!    且“已达成”必须同时满足加载倍率、时长、零崩溃、漂移四项证据；
//!    少一项就是虚报——这是本模块的第一性约束。
//! 2. **漂移限额**：前哨单轮漂移必须小于 8 MB；超限不是失败，而是转入
//!    “已登记补测”，把失败说成通过才是失败。
//! 3. **7×24 不虚报**：宿主侧一小时前哨无法证明 7×24，故该条只能是
//!    “已登记补测”，并写明原因；谁把它改成“已达成”，闸门当场判红。
//! 4. **合并零重复**：十五批 × 十五族 = 225 枚码必须两两不同，且每枚落在
//!    所属批的合并窗内；撞号意味着事件层无法定位来源批。
//! 5. **账行回读**：漂移账与合并账都要能渲染、回读、拒篡改；账不是展示品，
//!    是回归时唯一能逐位对账的凭据。
//!
//! ## 与相邻模块的差异
//!
//! - `unx49091_register` 登记的是“随闸门补测总清单”，粒度到条目；
//!   本模块的 F49087 只登记**内存漂移前哨**这一台仪器的三态与样本。
//! - `unx49089_finalgrep` 查的是关键词族撞号；本模块的 F49088 查的是
//!   **数值码**撞号，前者按字符串，后者按段窗数值，互补而不重叠。
//! - 合并窗 `0x80000..` 是本模块自建的**对账视图**，不占用任何已注册段位，
//!   也不改写各批原有码值。

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;
use super::unx48941_wheel_core as p_core;

// ── 段位与常量 ───────────────────────────────────────────────────
/// 本模块段基址（B15 第四槽）。
pub const SEG_BASE: u32 = 0x735C0;
/// 段宽（相邻模块互斥）。
pub const SEG_SPAN: u32 = 0x40;
/// 批数。
pub const BATCHES: usize = 15;
/// 错误族数。
pub const FAMILIES: usize = 15;
/// 合并码总数。
pub const MERGED: usize = BATCHES * FAMILIES;
/// 合并窗基址（独立对账视图，不占用已注册段位）。
pub const MERGE_BASE: u32 = 0x8_0000;
/// 单批合并窗宽。
pub const BATCH_SPAN: u32 = 0x100;
/// 漂移限额（千字节，8 MB）。
pub const DRIFT_LIMIT_KB: u32 = 8 * 1024;
/// 前哨时长门槛（分钟）。
pub const WINDOW_MIN: u16 = 60;
/// 加载倍率门槛。
pub const LOAD_MULT_MIN: u8 = 2;
/// 7×24 分钟数。
pub const SEVEN_DAY_MIN: u32 = 7 * 24 * 60;
/// 前哨条目数。
pub const SENTINELS: usize = 6;
/// 漂移账行标签。
pub const DRIFT_TAG: &str = "DRIFT87";
/// 合并账行标签。
pub const MERGE_TAG: &str = "MERGE88";
/// 漂移账键序。
pub const DRIFT_KEYS: [&str; 3] = ["item", "claim", "drift"];
/// 合并账键序。
pub const MERGE_KEYS: [&str; 3] = ["batch", "families", "base"];
/// 未知诊断码说明。
pub const UNKNOWN: &str = "未知 B15 漂移/合并诊断码";

// ── 诊断码 ───────────────────────────────────────────────────────
/// 漂移与合并诊断码（14 条，说明互不共用）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DmCode {
    /// 条目编号越界
    ItemBad,
    /// 三态取值不合法
    ClaimBad,
    /// 已达成却缺样本
    SampleMissing,
    /// 样本漂移超限
    DriftOver,
    /// 样本时长不足
    WindowShort,
    /// 样本加载倍率不足
    LoadShort,
    /// 样本非零崩溃
    CrashSeen,
    /// 非达成条目缺原因
    ReasonMissing,
    /// 7×24 被虚报为达成
    SevenDayLie,
    /// 合并批号越界
    BatchBad,
    /// 族号越界
    FamilyBad,
    /// 合并码撞号或出窗
    MergeClash,
    /// 摘要与重算不符
    DigestBad,
    /// 账行键序或字段错位
    RowBad,
}

impl DmCode {
    /// 诊断码值。
    pub fn val(self) -> u32 {
        SEG_BASE + 0x01 + self as u32
    }

    /// 逐码说明（互不共用）。
    pub fn detail(self) -> &'static str {
        match self {
            DmCode::ItemBad => "前哨条目编号越出 1..=6",
            DmCode::ClaimBad => "三态取值不在 met/deferred/unknown 内",
            DmCode::SampleMissing => "已达成条目缺少前哨样本",
            DmCode::DriftOver => "前哨漂移达到 8 MB 限额",
            DmCode::WindowShort => "前哨时长不足六十分钟",
            DmCode::LoadShort => "前哨加载倍率不足两倍",
            DmCode::CrashSeen => "前哨轮内出现崩溃",
            DmCode::ReasonMissing => "补测或未知条目未写原因",
            DmCode::SevenDayLie => "7×24 条目被虚报为已达成",
            DmCode::BatchBad => "合并批号越出 0..=14",
            DmCode::FamilyBad => "族号越出 0..=14",
            DmCode::MergeClash => "合并码撞号或越出本批合并窗",
            DmCode::DigestBad => "账行摘要与重算不一致",
            DmCode::RowBad => "账行标签、键序或字段数错位",
        }
    }

    /// 面向运营的短句（与 `detail` 不同口径）。
    pub fn message(self) -> &'static str {
        match self {
            DmCode::ItemBad => "条目越界：请回读前哨表",
            DmCode::ClaimBad => "三态非法：请改写登记",
            DmCode::SampleMissing => "样本缺失：请补测后登记",
            DmCode::DriftOver => "漂移超限：请转补测",
            DmCode::WindowShort => "时长不足：请延长前哨",
            DmCode::LoadShort => "倍率不足：请加到两倍",
            DmCode::CrashSeen => "崩溃出现：请转补测",
            DmCode::ReasonMissing => "原因缺失：请写明补测理由",
            DmCode::SevenDayLie => "7×24 虚报：请改为补测",
            DmCode::BatchBad => "批号越界：请核对十五批",
            DmCode::FamilyBad => "族号越界：请核对十五族",
            DmCode::MergeClash => "合并撞号：请重排合并窗",
            DmCode::DigestBad => "摘要失配：请重算账行",
            DmCode::RowBad => "账行损坏：请重渲染",
        }
    }
}

/// 全部诊断码（顺序即值序）。
pub const ALL: [DmCode; 14] = [
    DmCode::ItemBad,
    DmCode::ClaimBad,
    DmCode::SampleMissing,
    DmCode::DriftOver,
    DmCode::WindowShort,
    DmCode::LoadShort,
    DmCode::CrashSeen,
    DmCode::ReasonMissing,
    DmCode::SevenDayLie,
    DmCode::BatchBad,
    DmCode::FamilyBad,
    DmCode::MergeClash,
    DmCode::DigestBad,
    DmCode::RowBad,
];

/// 自由函数：码值 → 说明（段外一律 `UNKNOWN`）。
pub fn detail(code: u32) -> &'static str {
    match code.wrapping_sub(SEG_BASE) {
        1 => DmCode::ItemBad.detail(),
        2 => DmCode::ClaimBad.detail(),
        3 => DmCode::SampleMissing.detail(),
        4 => DmCode::DriftOver.detail(),
        5 => DmCode::WindowShort.detail(),
        6 => DmCode::LoadShort.detail(),
        7 => DmCode::CrashSeen.detail(),
        8 => DmCode::ReasonMissing.detail(),
        9 => DmCode::SevenDayLie.detail(),
        10 => DmCode::BatchBad.detail(),
        11 => DmCode::FamilyBad.detail(),
        12 => DmCode::MergeClash.detail(),
        13 => DmCode::DigestBad.detail(),
        14 => DmCode::RowBad.detail(),
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

// ── 诚实三态 ─────────────────────────────────────────────────────
/// 登记三态。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Claim {
    /// 本轮已有完整证据。
    Met,
    /// 已登记，转随闸门补测。
    Deferred,
    /// 尚无任何测量。
    Unknown,
}

impl Claim {
    /// 账行取值。
    pub fn tag(self) -> &'static str {
        match self {
            Claim::Met => "met",
            Claim::Deferred => "deferred",
            Claim::Unknown => "unknown",
        }
    }

    /// 由账行取值反解。
    pub fn from_tag(tag: &str) -> Option<Claim> {
        match tag {
            "met" => Some(Claim::Met),
            "deferred" => Some(Claim::Deferred),
            "unknown" => Some(Claim::Unknown),
            _ => None,
        }
    }

    /// 中文短名。
    pub fn name(self) -> &'static str {
        match self {
            Claim::Met => "已达成",
            Claim::Deferred => "已登记补测",
            Claim::Unknown => "未知",
        }
    }
}

/// 单轮前哨样本。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Sample {
    /// 空载基线（千字节）。
    pub base_kb: u32,
    /// 两倍负载峰值（千字节）。
    pub peak_kb: u32,
    /// 采样时长（分钟）。
    pub minutes: u16,
    /// 加载倍率。
    pub load_mult: u8,
    /// 轮内是否零崩溃。
    pub crash_free: bool,
}

impl Sample {
    /// 漂移量（峰值减基线，基线高于峰值时记零）。
    pub fn drift_kb(&self) -> u32 {
        self.peak_kb.saturating_sub(self.base_kb)
    }

    /// 是否落在限额内。
    pub fn under_limit(&self) -> bool {
        self.drift_kb() < DRIFT_LIMIT_KB
    }

    /// 证据是否完整（倍率、时长、零崩溃、限额四项同时成立）。
    pub fn complete(&self) -> bool {
        self.load_mult >= LOAD_MULT_MIN
            && self.minutes >= WINDOW_MIN
            && self.crash_free
            && self.under_limit()
    }
}

/// 由样本推导三态（无样本即未知）。
pub fn derive_claim(sample: Option<&Sample>) -> Claim {
    match sample {
        None => Claim::Unknown,
        Some(s) => {
            if s.complete() {
                Claim::Met
            } else {
                Claim::Deferred
            }
        }
    }
}

/// 前哨条目。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Sentinel {
    /// 编号 1..=6。
    pub id: u8,
    /// 条目名。
    pub name: &'static str,
    /// 登记三态。
    pub claim: Claim,
    /// 样本（未知条目为空）。
    pub sample: Option<Sample>,
    /// 补测原因（达成条目为空串）。
    pub reason: &'static str,
}

/// 六条前哨条目。
pub const SENTINEL_TABLE: [Sentinel; SENTINELS] = [
    Sentinel {
        id: 1,
        name: "两倍峰值一小时前哨",
        claim: Claim::Met,
        sample: Some(Sample {
            base_kb: 40_000,
            peak_kb: 43_000,
            minutes: 60,
            load_mult: 2,
            crash_free: true,
        }),
        reason: "",
    },
    Sentinel {
        id: 2,
        name: "漂移超限条",
        claim: Claim::Deferred,
        sample: Some(Sample {
            base_kb: 40_000,
            peak_kb: 49_500,
            minutes: 60,
            load_mult: 2,
            crash_free: true,
        }),
        reason: "漂移 9,500 KB 达到 8 MB 限额",
    },
    Sentinel {
        id: 3,
        name: "时长不足条",
        claim: Claim::Deferred,
        sample: Some(Sample {
            base_kb: 40_000,
            peak_kb: 40_100,
            minutes: 5,
            load_mult: 2,
            crash_free: true,
        }),
        reason: "采样仅五分钟，不足一小时",
    },
    Sentinel {
        id: 4,
        name: "倍率与崩溃条",
        claim: Claim::Deferred,
        sample: Some(Sample {
            base_kb: 40_000,
            peak_kb: 60_000,
            minutes: 60,
            load_mult: 1,
            crash_free: false,
        }),
        reason: "仅一倍加载且出现一次崩溃",
    },
    Sentinel {
        id: 5,
        name: "7×24 零崩溃",
        claim: Claim::Deferred,
        sample: None,
        reason: "宿主侧未跑满 168 小时，随闸门补测",
    },
    Sentinel {
        id: 6,
        name: "真机延迟阈值",
        claim: Claim::Unknown,
        sample: None,
        reason: "真机尚未到货，无测量",
    },
];

/// 全部前哨条目。
pub fn sentinels() -> &'static [Sentinel] {
    &SENTINEL_TABLE
}

/// 按编号取条目。
pub fn sentinel_for(id: u8) -> Option<&'static Sentinel> {
    let mut i = 0usize;
    while i < SENTINEL_TABLE.len() {
        if SENTINEL_TABLE[i].id == id {
            return Some(&SENTINEL_TABLE[i]);
        }
        i += 1;
    }
    None
}

/// 条目是否诚实：三态与样本推导一致；非达成必写原因；达成不得留原因；
/// 7×24 条目不得为达成。
pub fn honest(e: &Sentinel) -> bool {
    if e.id == 0 || e.id as usize > SENTINELS || e.name.is_empty() {
        return false;
    }
    match e.claim {
        Claim::Met => {
            e.sample.is_some()
                && e.reason.is_empty()
                && derive_claim(e.sample.as_ref()) == Claim::Met
        }
        // 无样本的补测条目（如 7×24）允许“已登记补测”，但必须写明原因；
        // 有样本且样本完整时不得再标补测——那是压低进度，同样是失真。
        Claim::Deferred => {
            !e.reason.is_empty() && derive_claim(e.sample.as_ref()) != Claim::Met
        }
        Claim::Unknown => !e.reason.is_empty() && e.sample.is_none(),
    }
}

/// 全表诚实。
pub fn ledger_honest() -> bool {
    if SENTINEL_TABLE.len() != SENTINELS {
        return false;
    }
    let mut i = 0usize;
    while i < SENTINEL_TABLE.len() {
        if SENTINEL_TABLE[i].id != (i as u8) + 1 || !honest(&SENTINEL_TABLE[i]) {
            return false;
        }
        i += 1;
    }
    true
}

/// 三态计数：达成 / 补测 / 未知。
pub fn claim_counts() -> (u32, u32, u32) {
    let mut met = 0u32;
    let mut deferred = 0u32;
    let mut unknown = 0u32;
    let mut i = 0usize;
    while i < SENTINEL_TABLE.len() {
        match SENTINEL_TABLE[i].claim {
            Claim::Met => met += 1,
            Claim::Deferred => deferred += 1,
            Claim::Unknown => unknown += 1,
        }
        i += 1;
    }
    (met, deferred, unknown)
}

/// 7×24 条目必须补测。
pub fn seven_day_deferred() -> bool {
    match sentinel_for(5) {
        Some(e) => e.claim == Claim::Deferred && !e.reason.is_empty(),
        None => false,
    }
}

// ── 十五批合并 ───────────────────────────────────────────────────
/// 十五批名。
pub const BATCH_NAMES: [&str; BATCHES] = [
    "B01 焦点", "B02 生命周期", "B03 手势", "B04 消费", "B05 心跳", "B06 错误码",
    "B07 错误族", "B08 fuzz", "B09 批印", "B10 延迟核", "B11 O1 对照", "B12 风暴取证",
    "B13 导出自检", "B14 延迟实验", "B15 首产收官",
];

/// 十五族名（错误码合并表列名）。
pub const FAMILY_NAMES: [&str; FAMILIES] = [
    "限速", "洪泛", "热移除", "乱序", "溢出", "回退", "fuzz", "漂移", "合并", "观测",
    "告警", "收口", "移交", "段印", "批印",
];

/// 批号 → 合并窗基址。
pub fn batch_seg(batch: u8) -> Option<u32> {
    if batch as usize >= BATCHES {
        return None;
    }
    Some(MERGE_BASE + (batch as u32) * BATCH_SPAN)
}

/// 批名。
pub fn batch_name(batch: u8) -> Option<&'static str> {
    if batch as usize >= BATCHES {
        return None;
    }
    Some(BATCH_NAMES[batch as usize])
}

/// 族名。
pub fn family_name(family: u8) -> Option<&'static str> {
    if family as usize >= FAMILIES {
        return None;
    }
    Some(FAMILY_NAMES[family as usize])
}

/// 合并码：批窗基址 + 1 + 族号。
pub fn merged_code(batch: u8, family: u8) -> Option<u32> {
    let seg = batch_seg(batch)?;
    if family as usize >= FAMILIES {
        return None;
    }
    Some(seg + 1 + family as u32)
}

/// 合并表（225 行三元组：批、族、码）。
pub fn merged_table() -> Vec<(u8, u8, u32)> {
    let mut out = Vec::new();
    let mut b = 0u8;
    while (b as usize) < BATCHES {
        let mut f = 0u8;
        while (f as usize) < FAMILIES {
            if let Some(code) = merged_code(b, f) {
                out.push((b, f, code));
            }
            f += 1;
        }
        b += 1;
    }
    out
}

/// 合并码零重复：225 码两两不同，且每码落在本批窗 `seg+1..=seg+15`。
pub fn zero_duplicates() -> bool {
    let table = merged_table();
    if table.len() != MERGED {
        return false;
    }
    let mut i = 0usize;
    while i < table.len() {
        let (b, f, code) = table[i];
        let seg = match batch_seg(b) {
            Some(s) => s,
            None => return false,
        };
        if code <= seg || code > seg + FAMILIES as u32 || f as usize >= FAMILIES {
            return false;
        }
        let mut j = i + 1;
        while j < table.len() {
            if table[j].2 == code {
                return false;
            }
            j += 1;
        }
        i += 1;
    }
    true
}

/// 十五批名与十五族名两两不同且非空。
pub fn names_guard() -> bool {
    let mut i = 0usize;
    while i < BATCHES {
        if BATCH_NAMES[i].is_empty() || FAMILY_NAMES[i].is_empty() {
            return false;
        }
        let mut j = i + 1;
        while j < BATCHES {
            if BATCH_NAMES[j] == BATCH_NAMES[i] || FAMILY_NAMES[j] == FAMILY_NAMES[i] {
                return false;
            }
            j += 1;
        }
        i += 1;
    }
    true
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

/// 漂移账摘要：条目号 + 三态 + 漂移。
pub fn drift_digest(id: u8, claim: Claim, drift: u32) -> u64 {
    let mut h = p_core::FNV_OFF;
    h = p_core::fold_u32(h, 0x4452_4946);
    h = p_core::fold_u32(h, id as u32);
    let bytes = claim.tag().as_bytes();
    let mut i = 0usize;
    while i < bytes.len() {
        h = p_core::fold_byte(h, bytes[i]);
        i += 1;
    }
    p_core::fold_u32(h, drift)
}

/// 条目的账行漂移值（无样本记满窗哨兵）。
pub fn drift_value(e: &Sentinel) -> u32 {
    match e.sample {
        Some(s) => s.drift_kb(),
        None => u32::MAX,
    }
}

/// 渲染漂移账行。
pub fn render_drift_row(e: &Sentinel) -> String {
    let drift = drift_value(e);
    format!(
        "{}|item={}|claim={}|drift={}|digest={:016x}",
        DRIFT_TAG,
        e.id,
        e.claim.tag(),
        drift,
        drift_digest(e.id, e.claim, drift)
    )
}

/// 回读漂移账行（对照前哨表重算）。
pub fn audit_drift_row(row: &str) -> bool {
    let f = match row_fields(row, DRIFT_TAG, &DRIFT_KEYS) {
        Some(v) if v.len() == DRIFT_KEYS.len() + 1 => v,
        _ => return false,
    };
    let id = match parse_u32(f[0]) {
        Some(v) if v >= 1 && v <= SENTINELS as u32 => v as u8,
        _ => return false,
    };
    let e = match sentinel_for(id) {
        Some(e) => e,
        None => return false,
    };
    let claim = match Claim::from_tag(f[1]) {
        Some(c) => c,
        None => return false,
    };
    let drift = match parse_u32(f[2]) {
        Some(v) => v,
        None => return false,
    };
    let want = match parse_hex16(f[3]) {
        Some(v) => v,
        None => return false,
    };
    claim == e.claim && drift == drift_value(e) && drift_digest(id, claim, drift) == want
}

/// 导出六条漂移账行。
pub fn export_drift_rows() -> Vec<String> {
    let mut out = Vec::new();
    let mut i = 0usize;
    while i < SENTINEL_TABLE.len() {
        out.push(render_drift_row(&SENTINEL_TABLE[i]));
        i += 1;
    }
    out
}

/// 六行两两不同且逐行可回读。
pub fn drift_rows_stable() -> bool {
    let rows = export_drift_rows();
    if rows.len() != SENTINELS {
        return false;
    }
    let mut i = 0usize;
    while i < rows.len() {
        if !audit_drift_row(&rows[i]) {
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

/// 单批合并账摘要：批号 + 基址 + 十五码。
pub fn merge_digest(batch: u8) -> Option<u64> {
    let seg = batch_seg(batch)?;
    let mut h = p_core::FNV_OFF;
    h = p_core::fold_u32(h, 0x4D45_5247);
    h = p_core::fold_u32(h, batch as u32);
    h = p_core::fold_u32(h, seg);
    let mut f = 0u8;
    while (f as usize) < FAMILIES {
        match merged_code(batch, f) {
            Some(code) => h = p_core::fold_u32(h, code),
            None => return None,
        }
        f += 1;
    }
    Some(h)
}

/// 渲染单批合并账行。
pub fn render_merge_row(batch: u8) -> Option<String> {
    let seg = batch_seg(batch)?;
    let digest = merge_digest(batch)?;
    Some(format!(
        "{}|batch={}|families={}|base={}|digest={:016x}",
        MERGE_TAG, batch, FAMILIES, seg, digest
    ))
}

/// 回读合并账行。
pub fn audit_merge_row(row: &str) -> bool {
    let f = match row_fields(row, MERGE_TAG, &MERGE_KEYS) {
        Some(v) if v.len() == MERGE_KEYS.len() + 1 => v,
        _ => return false,
    };
    let batch = match parse_u32(f[0]) {
        Some(v) if (v as usize) < BATCHES => v as u8,
        _ => return false,
    };
    let families = match parse_u32(f[1]) {
        Some(v) => v,
        _ => return false,
    };
    let base = match parse_u32(f[2]) {
        Some(v) => v,
        _ => return false,
    };
    let want = match parse_hex16(f[3]) {
        Some(v) => v,
        None => return false,
    };
    let seg = match batch_seg(batch) {
        Some(s) => s,
        None => return false,
    };
    families == FAMILIES as u32 && base == seg && merge_digest(batch) == Some(want)
}

/// 十五批合并账行。
pub fn export_merge_rows() -> Vec<String> {
    let mut out = Vec::new();
    let mut b = 0u8;
    while (b as usize) < BATCHES {
        match render_merge_row(b) {
            Some(row) => out.push(row),
            None => return out,
        }
        b += 1;
    }
    out
}

/// 十五行两两不同且逐行可回读。
pub fn merge_rows_stable() -> bool {
    let rows = export_merge_rows();
    if rows.len() != BATCHES {
        return false;
    }
    let mut i = 0usize;
    while i < rows.len() {
        if !audit_merge_row(&rows[i]) {
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

    // 1) 三态诚实。
    let (met, deferred, unknown) = claim_counts();
    let mut tamper = SENTINEL_TABLE[1];
    tamper.claim = Claim::Met;
    let lie_rejected = !honest(&tamper);
    let mut no_reason = SENTINEL_TABLE[2];
    no_reason.reason = "";
    let reason_rejected = !honest(&no_reason);
    let ok = ledger_honest()
        && met == 1
        && deferred == 4
        && unknown == 1
        && lie_rejected
        && reason_rejected;
    out.push((
        "F49087/三态诚实",
        ok,
        format!(
            "met={} deferred={} unknown={} lie_rejected={} reason_rejected={}",
            met, deferred, unknown, lie_rejected, reason_rejected
        ),
    ));

    // 2) 漂移限额与证据四项。
    let mut probe = Sample {
        base_kb: 1_000,
        peak_kb: 1_000 + DRIFT_LIMIT_KB,
        minutes: WINDOW_MIN,
        load_mult: LOAD_MULT_MIN,
        crash_free: true,
    };
    let over_rejected = !probe.complete();
    probe.peak_kb = 1_000 + DRIFT_LIMIT_KB - 1;
    let inside_ok = probe.complete();
    probe.minutes = WINDOW_MIN - 1;
    let short_rejected = !probe.complete();
    probe.minutes = WINDOW_MIN;
    probe.load_mult = LOAD_MULT_MIN - 1;
    let load_rejected = !probe.complete();
    probe.load_mult = LOAD_MULT_MIN;
    probe.crash_free = false;
    let crash_rejected = !probe.complete();
    let ok = over_rejected
        && inside_ok
        && short_rejected
        && load_rejected
        && crash_rejected
        && derive_claim(None) == Claim::Unknown;
    out.push((
        "F49087/漂移限额",
        ok,
        format!(
            "over_rejected={} inside_ok={} short={} load={} crash={}",
            over_rejected, inside_ok, short_rejected, load_rejected, crash_rejected
        ),
    ));

    // 3) 7×24 不虚报。
    let seven = match sentinel_for(5) {
        Some(e) => e,
        None => {
            out.push((
                "F49087/7×24 不虚报",
                false,
                String::from("条目缺失"),
            ));
            return extra_checks_guard(out);
        }
    };
    let sample_short = match seven.sample {
        Some(s) => (s.minutes as u32) < SEVEN_DAY_MIN,
        None => true,
    };
    let ok = seven.claim == Claim::Deferred && seven_day_deferred() && sample_short;
    out.push((
        "F49087/7×24 不虚报",
        ok,
        format!(
            "claim={} sample_short={} reason_len={}",
            seven.claim.tag(),
            sample_short,
            seven.reason.len()
        ),
    ));

    // 4) 漂移账行回读。
    let rows = export_drift_rows();
    let mut tamp = String::new();
    if let Some(first) = rows.first() {
        tamp.push_str(first);
        tamp.push('x');
    }
    let tamper_rejected = !tamp.is_empty() && !audit_drift_row(&tamp);
    let ok = rows.len() == SENTINELS && drift_rows_stable() && tamper_rejected;
    out.push((
        "F49087/账行回读",
        ok,
        format!(
            "rows={} stable={} tamper_rejected={}",
            rows.len(),
            drift_rows_stable(),
            tamper_rejected
        ),
    ));

    // 5) 合并零重复。
    let table = merged_table();
    let ok = table.len() == MERGED
        && zero_duplicates()
        && names_guard()
        && merged_code(BATCHES as u8, 0).is_none()
        && merged_code(0, FAMILIES as u8).is_none();
    out.push((
        "F49088/合并零重复",
        ok,
        format!(
            "codes={} zero_dup={} names_guard={}",
            table.len(),
            zero_duplicates(),
            names_guard()
        ),
    ));

    // 6) 十五批齐全。
    let mut batch_ok = true;
    let mut b = 0u8;
    while (b as usize) < BATCHES {
        match (batch_name(b), batch_seg(b), merge_digest(b)) {
            (Some(n), Some(s), Some(d)) => {
                if n.is_empty() || s != MERGE_BASE + (b as u32) * BATCH_SPAN || d == p_core::FNV_OFF {
                    batch_ok = false;
                }
            }
            _ => batch_ok = false,
        }
        b += 1;
    }
    let mut family_ok = true;
    let mut f = 0u8;
    while (f as usize) < FAMILIES {
        if family_name(f).map(|n| n.is_empty()).unwrap_or(true) {
            family_ok = false;
        }
        f += 1;
    }
    let ok = batch_ok && family_ok && family_name(FAMILIES as u8).is_none()
        && merge_digest(0) == merge_digest(0)
        && merge_digest(0) != merge_digest(1);
    out.push((
        "F49088/十五批齐全",
        ok,
        format!("batches={} families={} batch_ok={}", BATCHES, FAMILIES, batch_ok),
    ));

    // 7) 合并账行回读。
    let mrows = export_merge_rows();
    let mut mtamp = String::new();
    if let Some(first) = mrows.first() {
        mtamp.push_str(first);
        mtamp.push_str("9");
    }
    let m_tamper = !mtamp.is_empty() && !audit_merge_row(&mtamp);
    let ok = mrows.len() == BATCHES && merge_rows_stable() && m_tamper;
    out.push((
        "F49088/账行回读",
        ok,
        format!(
            "rows={} stable={} tamper_rejected={}",
            mrows.len(),
            merge_rows_stable(),
            m_tamper
        ),
    ));

    extra_checks_guard(out)
}

/// 尾项：异常码守卫（拆函数避免主函数过长）。
fn extra_checks_guard(mut out: Vec<(&'static str, bool, String)>) -> Vec<(&'static str, bool, String)> {
    // 8) 异常码守卫。
    let mut distinct = true;
    let mut a = 0usize;
    while a < ALL.len() {
        let mut b = a + 1;
        while b < ALL.len() {
            if ALL[a].val() == ALL[b].val()
                || ALL[a].detail() == ALL[b].detail()
                || ALL[a].message() == ALL[b].message()
            {
                distinct = false;
            }
            b += 1;
        }
        a += 1;
    }
    let free_ok = detail(SEG_BASE) == UNKNOWN
        && detail(SEG_BASE + 0x0F) == UNKNOWN
        && detail(ALL[0].val()) == DmCode::ItemBad.detail();
    let merge_outside = MERGE_BASE + (BATCHES as u32) * BATCH_SPAN;
    let window_ok = MERGE_BASE > SEG_BASE + SEG_SPAN && merge_outside > MERGE_BASE;
    let ok = segment_guard() && distinct && free_ok && window_ok;
    out.push((
        "F49087/异常码守卫",
        ok,
        format!(
            "codes={} guard={} distinct={} free={} merge_window={}",
            ALL.len(),
            segment_guard(),
            distinct,
            free_ok,
            window_ok
        ),
    ));

    out
}

/// 入口：八项判据写入 CheckSet。
pub fn run_unx49087_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F49087 漂移前哨与合并表");
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

    fn good_sample() -> Sample {
        Sample {
            base_kb: 2_000,
            peak_kb: 4_000,
            minutes: WINDOW_MIN,
            load_mult: LOAD_MULT_MIN,
            crash_free: true,
        }
    }

    // ── 诊断码 ──────────────────────────────────────────────────

    #[test]
    fn codes_in_window() {
        assert!(segment_guard());
        assert_eq!(ALL.len(), 14);
        let mut i = 0usize;
        while i < ALL.len() {
            assert!(ALL[i].val() > SEG_BASE);
            assert!(ALL[i].val() < SEG_BASE + SEG_SPAN);
            assert_eq!(ALL[i].val(), SEG_BASE + 1 + i as u32);
            i += 1;
        }
    }

    #[test]
    fn codes_text_distinct() {
        let mut i = 0usize;
        while i < ALL.len() {
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
    fn detail_free_matches_table() {
        let mut i = 0usize;
        while i < ALL.len() {
            assert_eq!(detail(ALL[i].val()), ALL[i].detail());
            i += 1;
        }
        assert_eq!(detail(SEG_BASE), UNKNOWN);
        assert_eq!(detail(SEG_BASE + 0x0F), UNKNOWN);
        assert_eq!(detail(SEG_BASE + SEG_SPAN), UNKNOWN);
    }

    // ── 三态 ────────────────────────────────────────────────────

    #[test]
    fn claim_roundtrip() {
        let all = [Claim::Met, Claim::Deferred, Claim::Unknown];
        let mut i = 0usize;
        while i < all.len() {
            assert_eq!(Claim::from_tag(all[i].tag()), Some(all[i]));
            assert!(!all[i].name().is_empty());
            i += 1;
        }
        assert_eq!(Claim::from_tag("MET"), None);
        assert_eq!(Claim::from_tag(""), None);
    }

    #[test]
    fn claim_tags_distinct() {
        assert_ne!(Claim::Met.tag(), Claim::Deferred.tag());
        assert_ne!(Claim::Deferred.tag(), Claim::Unknown.tag());
        assert_ne!(Claim::Met.tag(), Claim::Unknown.tag());
    }

    // ── 样本 ────────────────────────────────────────────────────

    #[test]
    fn sample_drift_floor() {
        let mut s = good_sample();
        assert_eq!(s.drift_kb(), 2_000);
        s.peak_kb = 1_000;
        assert_eq!(s.drift_kb(), 0);
    }

    #[test]
    fn sample_limit_boundary() {
        let mut s = good_sample();
        s.peak_kb = s.base_kb + DRIFT_LIMIT_KB;
        assert!(!s.under_limit());
        assert!(!s.complete());
        s.peak_kb = s.base_kb + DRIFT_LIMIT_KB - 1;
        assert!(s.under_limit());
        assert!(s.complete());
    }

    #[test]
    fn sample_evidence_four_items() {
        let mut s = good_sample();
        assert!(s.complete());
        s.minutes = WINDOW_MIN - 1;
        assert!(!s.complete());
        s.minutes = WINDOW_MIN;
        s.load_mult = LOAD_MULT_MIN - 1;
        assert!(!s.complete());
        s.load_mult = LOAD_MULT_MIN;
        s.crash_free = false;
        assert!(!s.complete());
        s.crash_free = true;
        assert!(s.complete());
    }

    #[test]
    fn derive_claim_rules() {
        assert_eq!(derive_claim(None), Claim::Unknown);
        let s = good_sample();
        assert_eq!(derive_claim(Some(&s)), Claim::Met);
        let mut bad = s;
        bad.minutes = 1;
        assert_eq!(derive_claim(Some(&bad)), Claim::Deferred);
    }

    // ── 前哨表 ──────────────────────────────────────────────────

    #[test]
    fn sentinel_table_shape() {
        assert_eq!(sentinels().len(), SENTINELS);
        let mut i = 0usize;
        while i < SENTINEL_TABLE.len() {
            assert_eq!(SENTINEL_TABLE[i].id, (i as u8) + 1);
            assert!(!SENTINEL_TABLE[i].name.is_empty());
            assert!(sentinel_for(SENTINEL_TABLE[i].id).is_some());
            i += 1;
        }
        assert_eq!(sentinel_for(0), None);
        assert_eq!(sentinel_for(SENTINELS as u8 + 1), None);
    }

    #[test]
    fn sentinel_names_distinct() {
        let mut i = 0usize;
        while i < SENTINEL_TABLE.len() {
            let mut j = i + 1;
            while j < SENTINEL_TABLE.len() {
                assert_ne!(SENTINEL_TABLE[i].name, SENTINEL_TABLE[j].name);
                j += 1;
            }
            i += 1;
        }
    }

    #[test]
    fn ledger_is_honest() {
        assert!(ledger_honest());
        let mut i = 0usize;
        while i < SENTINEL_TABLE.len() {
            assert!(honest(&SENTINEL_TABLE[i]));
            i += 1;
        }
    }

    #[test]
    fn claim_counts_match_table() {
        let (met, deferred, unknown) = claim_counts();
        assert_eq!(met, 1);
        assert_eq!(deferred, 4);
        assert_eq!(unknown, 1);
        assert_eq!(met + deferred + unknown, SENTINELS as u32);
    }

    #[test]
    fn seven_day_must_be_deferred() {
        assert!(seven_day_deferred());
        let seven = match sentinel_for(5) {
            Some(e) => e,
            None => panic!("条目五缺失"),
        };
        assert_eq!(seven.claim, Claim::Deferred);
        assert_eq!(seven.sample, None);
        assert!(!seven.reason.is_empty());
    }

    #[test]
    fn inflated_met_rejected() {
        let mut lie = SENTINEL_TABLE[1];
        lie.claim = Claim::Met;
        assert!(!honest(&lie));
    }

    #[test]
    fn met_with_reason_rejected() {
        let mut bad = SENTINEL_TABLE[0];
        bad.reason = "多余原因";
        assert!(!honest(&bad));
    }

    #[test]
    fn deferred_without_reason_rejected() {
        let mut bad = SENTINEL_TABLE[2];
        bad.reason = "";
        assert!(!honest(&bad));
    }

    #[test]
    fn honest_rejects_bad_id_and_name() {
        let bad_id = Sentinel {
            id: 0,
            name: "x",
            claim: Claim::Unknown,
            sample: None,
            reason: "r",
        };
        assert!(!honest(&bad_id));
        let bad_name = Sentinel {
            id: 1,
            name: "",
            claim: Claim::Unknown,
            sample: None,
            reason: "r",
        };
        assert!(!honest(&bad_name));
    }

    #[test]
    fn honest_rejects_unknown_claim_without_reason() {
        let bad = Sentinel {
            id: 6,
            name: "x",
            claim: Claim::Unknown,
            sample: None,
            reason: "",
        };
        assert!(!honest(&bad));
    }

    // ── 合表 ────────────────────────────────────────────────────

    #[test]
    fn batch_and_family_names() {
        assert_eq!(BATCH_NAMES.len(), BATCHES);
        assert_eq!(FAMILY_NAMES.len(), FAMILIES);
        assert!(names_guard());
        assert!(batch_name(0).is_some());
        assert!(batch_name(BATCHES as u8).is_none());
        assert!(family_name(FAMILIES as u8 - 1).is_some());
        assert!(family_name(FAMILIES as u8).is_none());
    }

    #[test]
    fn batch_segments_contiguous() {
        let mut b = 0u8;
        while (b as usize) < BATCHES {
            let seg = match batch_seg(b) {
                Some(s) => s,
                None => panic!("批窗缺失"),
            };
            assert_eq!(seg, MERGE_BASE + (b as u32) * BATCH_SPAN);
            if b > 0 {
                let prev = match batch_seg(b - 1) {
                    Some(s) => s,
                    None => panic!("前批缺失"),
                };
                assert_eq!(seg - prev, BATCH_SPAN);
            }
            b += 1;
        }
        assert_eq!(batch_seg(BATCHES as u8), None);
    }

    #[test]
    fn merged_code_shape() {
        let mut b = 0u8;
        while (b as usize) < BATCHES {
            let mut f = 0u8;
            while (f as usize) < FAMILIES {
                let code = match merged_code(b, f) {
                    Some(c) => c,
                    None => panic!("码缺失"),
                };
                let seg = match batch_seg(b) {
                    Some(s) => s,
                    None => panic!("批窗缺失"),
                };
                assert_eq!(code, seg + 1 + f as u32);
                assert!(code > seg && code <= seg + FAMILIES as u32);
                f += 1;
            }
            b += 1;
        }
        assert_eq!(merged_code(BATCHES as u8, 0), None);
        assert_eq!(merged_code(0, FAMILIES as u8), None);
    }

    #[test]
    fn merged_table_size_and_unique() {
        let table = merged_table();
        assert_eq!(table.len(), MERGED);
        assert_eq!(MERGED, 225);
        let mut i = 0usize;
        while i < table.len() {
            let mut j = i + 1;
            while j < table.len() {
                assert_ne!(table[i].2, table[j].2);
                j += 1;
            }
            i += 1;
        }
    }

    #[test]
    fn zero_duplicates_true() {
        assert!(zero_duplicates());
    }

    #[test]
    fn merged_digest_deterministic() {
        let mut b = 0u8;
        while (b as usize) < BATCHES {
            let a = match merge_digest(b) {
                Some(d) => d,
                None => panic!("摘要缺失"),
            };
            assert_eq!(Some(a), merge_digest(b));
            assert_ne!(a, p_core::FNV_OFF);
            if b > 0 {
                assert_ne!(a, merge_digest(b - 1).unwrap_or(0));
            }
            b += 1;
        }
        assert_eq!(merge_digest(BATCHES as u8), None);
    }

    #[test]
    fn drift_digest_sensitive() {
        let a = drift_digest(1, Claim::Met, 3_000);
        assert_eq!(a, drift_digest(1, Claim::Met, 3_000));
        assert_ne!(a, drift_digest(2, Claim::Met, 3_000));
        assert_ne!(a, drift_digest(1, Claim::Deferred, 3_000));
        assert_ne!(a, drift_digest(1, Claim::Met, 3_001));
        assert_ne!(a, p_core::FNV_OFF);
    }

    #[test]
    fn drift_value_for_sentinels() {
        let mut i = 0usize;
        while i < SENTINEL_TABLE.len() {
            let e = &SENTINEL_TABLE[i];
            match e.sample {
                Some(s) => assert_eq!(drift_value(e), s.drift_kb()),
                None => assert_eq!(drift_value(e), u32::MAX),
            }
            i += 1;
        }
    }

    // ── 账行 ────────────────────────────────────────────────────

    #[test]
    fn drift_rows_stable_and_distinct() {
        let rows = export_drift_rows();
        assert_eq!(rows.len(), SENTINELS);
        assert!(drift_rows_stable());
        let mut i = 0usize;
        while i < rows.len() {
            let mut j = i + 1;
            while j < rows.len() {
                assert_ne!(rows[i], rows[j]);
                j += 1;
            }
            i += 1;
        }
    }

    #[test]
    fn drift_row_tamper_rejected() {
        let rows = export_drift_rows();
        let first = match rows.first() {
            Some(r) => r,
            None => panic!("无账行"),
        };
        assert!(audit_drift_row(first));
        let mut tamp = first.clone();
        tamp.push('x');
        assert!(!audit_drift_row(&tamp));
        assert!(!audit_drift_row("DRIFT87|item=1|claim=met|drift=0|digest=0000000000000000"));
        assert!(!audit_drift_row("DRIFT87|item=7|claim=met|drift=0|digest=0000000000000000"));
    }

    #[test]
    fn merge_rows_stable_and_distinct() {
        let rows = export_merge_rows();
        assert_eq!(rows.len(), BATCHES);
        assert!(merge_rows_stable());
        let mut i = 0usize;
        while i < rows.len() {
            let mut j = i + 1;
            while j < rows.len() {
                assert_ne!(rows[i], rows[j]);
                j += 1;
            }
            i += 1;
        }
    }

    #[test]
    fn merge_row_tamper_rejected() {
        let rows = export_merge_rows();
        let first = match rows.first() {
            Some(r) => r,
            None => panic!("无账行"),
        };
        assert!(audit_merge_row(first));
        let mut tamp = first.clone();
        tamp.push('1');
        assert!(!audit_merge_row(&tamp));
        assert!(!audit_merge_row("MERGE88|batch=15|families=15|base=524288|digest=0000000000000000"));
        assert!(!audit_merge_row("MERGE88|batch=0|families=14|base=524288|digest=0000000000000000"));
    }

    #[test]
    fn parse_u32_boundaries() {
        assert_eq!(parse_u32("0"), Some(0));
        assert_eq!(parse_u32("4294967295"), Some(u32::MAX));
        assert_eq!(parse_u32("4294967296"), None);
        assert_eq!(parse_u32(""), None);
        assert_eq!(parse_u32("9a"), None);
    }

    #[test]
    fn parse_hex16_boundaries() {
        assert_eq!(parse_hex16("0000000000000000"), Some(0));
        assert_eq!(parse_hex16("0123456789abcdef"), Some(0x0123_4567_89AB_CDEF));
        assert_eq!(parse_hex16("0123456789ABCDEF"), Some(0x0123_4567_89AB_CDEF));
        assert_eq!(parse_hex16("0123456789abcde"), None);
        assert_eq!(parse_hex16("0123456789abcdef0"), None);
        assert_eq!(parse_hex16("zz23456789abcdef"), None);
    }

    #[test]
    fn row_fields_shape() {
        let row = match render_merge_row(0) {
            Some(r) => r,
            None => panic!("无账行"),
        };
        assert_eq!(
            row_fields(&row, MERGE_TAG, &MERGE_KEYS).map(|v| v.len()),
            Some(4)
        );
        assert_eq!(row_fields(&row, DRIFT_TAG, &MERGE_KEYS), None);
        assert_eq!(row_fields(&row, MERGE_TAG, &DRIFT_KEYS), None);
        assert_eq!(row_fields(MERGE_TAG, MERGE_TAG, &MERGE_KEYS), None);
    }

    // ── CheckSet 契约 ───────────────────────────────────────────

    #[test]
    fn checkset_eight_green() {
        let s = run_unx49087_checks();
        assert_eq!(s.len(), 8);
        assert_eq!(s.passed_count(), 8);
        assert_eq!(s.failed_count(), 0);
        assert!(s.all_passed());
        assert!(!s.truncated());
        assert!(s.red_items_text().is_empty());
    }

    #[test]
    fn checks_names_and_details() {
        let items = extra_checks();
        assert_eq!(items.len(), 8);
        let mut i = 0usize;
        while i < items.len() {
            assert!(items[i].1);
            assert!(items[i].0.starts_with("F49087/") || items[i].0.starts_with("F49088/"));
            assert!(!items[i].2.is_empty());
            i += 1;
        }
    }
}
