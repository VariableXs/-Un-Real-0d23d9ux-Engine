//! UNX-F49053 对接面错误码族（焦点丢失/约束冲突/失联 15 条）· 0x73180
//! ═══════════════════════════════════════════════════════════════════════
//! 判据原文（任务板逐字锚定，B13 族第七槽；J1 引用按任务板原文保留）：
//!   UNX-F49053 对接面错误码族（焦点丢失/约束冲突/失联 15 条）:
//!          ① 纯功能代码 ≥280 行；
//!          ② 判据原文：UNX-F49054-J1 联轧 ×5 全绿零漂移；判据锚定 §AI-62 §7；
//!          ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//!   UNX-F49054 B13 段内联轧（对接五路判据交叉复跑）:
//!          ① 纯功能代码 ≥220 行；
//!          ② 判据原文：UNX-F49055-J1 错误码 ×15 零漂移互不混淆；判据锚定 §AI-62 §2；
//!          ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! ─────────────────────────────────────────────────────────────────────
//! 闸门次序（错误码族）：码值 → 词法 → 分类 → 路由 → 快照。
//! 码值先于词法：一张码表若码值本身就不连续、不唯一，后面的名称再工整也没意义——
//!   上层按码值分支，码值一旦撞号，两个语义不同的故障会走同一条恢复路径。
//! 词法先于分类：英文名必须是 UPPER_SNAKE（人读日志、机器 grep 都靠它），
//!   中文说明必须逐条具体——两条码共用一句中文就是「互不混淆」的反例。
//! 分类先于路由：焦点丢失 / 约束冲突 / 失联三类必须先各归其位，
//!   「归属路由」才有唯一答案；先定路由再分类，等于让路由表反推语义。
//! 路由先于快照：快照冻结的是「码值 + 名称 + 中文 + 严重度 + 分类 + 路由」六元组，
//!   前四步没定完就冻结，冻的是一张还没定稿的表。
//! 闸门次序（段内联轧）：单路 → 五路 → 交叉 → 复跑。
//! 单路先于五路：每一路（焦点/约束/失联/时序/渲染）先各自自证五条判据，
//!   单路红着就上交叉，交叉矩阵只会把红点稀释成「25 格里 1 格红」。
//! 五路先于交叉：五路全绿之后，交叉矩阵才有「任一路红则整体红」的判别力。
//! 交叉先于复跑：矩阵定形后再跑第二遍，两遍摘要必须逐位相同；
//!   带红点的矩阵复跑一万次也稳定地红，那不是零漂移，是稳定地错。
//! 关键差异：F49053 只回答「15 条码表的码值/词法/分类/路由是否冻结且互不混淆」，
//!   不做联轧执行；F49054 只回答「五路判据 × 交叉复跑是否全绿」，
//!   不重定码表。两本账的摘要域彼此独立：码表账折六元组，联轧账折 5×5 格。
//!
//! 15 条的构成（口径写死，避免「15」被随手解释）：
//!   焦点丢失 5 条（FOCUS_LOST_*，段内 +0x01..+0x05）
//!   约束冲突 5 条（CONSTRAINT_*，段内 +0x06..+0x0A）
//!   失联     4 条（LINK_*，段内 +0x0B..+0x0E）
//!   未归类哨 1 条（UNKNOWN_CODE，段外 +0x0F，仅作越界兜底，不参与分类）
//! 段内 14 条闭集由 `EcCode::ALL` 承载；哨兵由 `detail()` 兜底分支承载。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;
use super::unx48941_wheel_core as p_core;

/// 诊断段基址（0x73180，B13 族第七槽：F49053/F49054 两卡共用）。
pub const SEG_BASE: u32 = 0x73180;

/// 码表行数（14 段内 + 1 越界哨）。
pub const ROWS: usize = 15;
/// 焦点丢失类条数。
pub const FOCUS_ROWS: u32 = 5;
/// 约束冲突类条数。
pub const CONSTRAINT_ROWS: u32 = 5;
/// 失联类条数。
pub const LINK_ROWS: u32 = 4;
/// 未归类哨条数（恒为 1）。
pub const UNKNOWN_ROWS: u32 = 1;
/// 联轧路数。
pub const LANES: usize = 5;
/// 每路判据条数。
pub const CRITERIA: usize = 5;
/// 交叉矩阵格数（5×5）。
pub const CELLS: usize = LANES * CRITERIA;
/// 文字下限（中文说明至少 8 个字，避免「非法」这种无信息短句）。
pub const CN_MIN_CHARS: usize = 8;
/// 英文名长度下限。
pub const NAME_MIN_LEN: usize = 6;
/// 英文名长度上限。
pub const NAME_MAX_LEN: usize = 40;
/// 病态表注入批数。
pub const FAULT_INJECT: u32 = 10;
/// 六字段账行字段数。
pub const RENDER_FIELDS: usize = 6;
/// 本模块用例编号总数。
pub const CASE_TOTAL: usize = 16;
/// 本模块用例编号基址（0x0734_0400，实际用 +0x01..+0x10）。
pub const CASE_BASE: u32 = 0x0734_0400;
/// 码表账标签。
pub const TABLE_TAG: &str = "ETAB";
/// 联轧账标签。
pub const ROLL_TAG: &str = "ROLL";
/// 分类账标签。
pub const CLASS_TAG: &str = "ECLS";
/// 未知诊断码说明（15 条口径的第 15 行正文）。
pub const UNKNOWN: &str = "未知对接面诊断码";

// ── 内部审计码（本模块私有，不占段内 14 个诊断槽位） ──────────────
//
// 为什么要有它：14 个槽位全部给了「对接面错误码族」本身（5/5/4），
// 码表自审失败（名称不合词法、分类错位……）不能再挤占族内的语义码——
// 否则「表坏了」与「真发生了焦点丢失」会共用一个码，恢复路径无从区分。
// 审计码从 SEG_BASE+0x80 起，**不对外发布**，也不参与 15 条族谱。
pub const AUDIT_BASE: u32 = SEG_BASE + 0x80;
pub const AUDIT_SEGMENT: u32 = AUDIT_BASE + 0x01;
pub const AUDIT_NAME: u32 = AUDIT_BASE + 0x02;
pub const AUDIT_CLASS: u32 = AUDIT_BASE + 0x03;
pub const AUDIT_ROUTE: u32 = AUDIT_BASE + 0x04;
pub const AUDIT_TABLE: u32 = AUDIT_BASE + 0x05;
pub const AUDIT_LANE: u32 = AUDIT_BASE + 0x06;
pub const AUDIT_ROLL: u32 = AUDIT_BASE + 0x07;
pub const AUDIT_RENDER: u32 = AUDIT_BASE + 0x08;

/// 审计码的中文说明（与族内 `detail()` 分开，避免污染 15 条族谱）。
pub fn audit_detail(code: u32) -> &'static str {
    match code {
        AUDIT_SEGMENT => "码表自审：码值漂移或越出段位窗口",
        AUDIT_NAME => "码表自审：英文名或中文说明不合词法",
        AUDIT_CLASS => "码表自审：分类与码值区间不符",
        AUDIT_ROUTE => "码表自审：路由错位或严重度越界",
        AUDIT_TABLE => "码表自审：条数守恒被破坏",
        AUDIT_LANE => "联轧自审：路号越界或路摘要为零",
        AUDIT_ROLL => "联轧自审：矩阵不守恒或复跑漂移",
        AUDIT_RENDER => "码表自审：六字段账行不可解析",
        _ => UNKNOWN,
    }
}

// ── 诊断码闭集（段内 +0x01..+0x0E，共 14 条） ─────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum EcCode {
    /// 焦点丢失：主窗口被合成器摘除
    FocusLostPrimary,
    /// 焦点丢失：副本窗口未同步
    FocusLostReplica,
    /// 焦点丢失：回滚失败
    FocusLostRollback,
    /// 焦点丢失：焦点账已过期
    FocusLostStale,
    /// 焦点丢失：传送窗口无应答
    FocusLostRelay,
    /// 约束冲突：最小尺寸与最大值矛盾
    ConstraintPin,
    /// 约束冲突：几何越出合成器可接受域
    ConstraintGeom,
    /// 约束冲突：堆叠次序成环
    ConstraintStack,
    /// 约束冲突：锚点悬空
    ConstraintAnchor,
    /// 约束冲突：epoch 落后于约束版本
    ConstraintEpoch,
    /// 失联：心跳静默
    LinkSilent,
    /// 失联：缓冲打满
    LinkBufferFull,
    /// 失联：重同步握手失败
    LinkResync,
    /// 失联：超时窗口耗尽
    LinkTimeout,
}

impl EcCode {
    pub const fn val(self) -> u32 {
        SEG_BASE + 0x01 + self as u32
    }

    pub fn detail(self) -> &'static str {
        match self {
            EcCode::FocusLostPrimary => "焦点丢失：主窗口被合成器摘除",
            EcCode::FocusLostReplica => "焦点丢失：副本窗口未同步",
            EcCode::FocusLostRollback => "焦点丢失：回滚失败",
            EcCode::FocusLostStale => "焦点丢失：焦点账已过期",
            EcCode::FocusLostRelay => "焦点丢失：传送窗口无应答",
            EcCode::ConstraintPin => "约束冲突：最小尺寸与最大值矛盾",
            EcCode::ConstraintGeom => "约束冲突：几何越出合成器可接受域",
            EcCode::ConstraintStack => "约束冲突：堆叠次序成环",
            EcCode::ConstraintAnchor => "约束冲突：锚点悬空",
            EcCode::ConstraintEpoch => "约束冲突：epoch 落后于约束版本",
            EcCode::LinkSilent => "失联：心跳静默超窗",
            EcCode::LinkBufferFull => "失联：缓冲已打满拒收",
            EcCode::LinkResync => "失联：重同步握手失败",
            EcCode::LinkTimeout => "失联：超时窗口耗尽",
        }
    }

    pub const ALL: [EcCode; 14] = [
        EcCode::FocusLostPrimary,
        EcCode::FocusLostReplica,
        EcCode::FocusLostRollback,
        EcCode::FocusLostStale,
        EcCode::FocusLostRelay,
        EcCode::ConstraintPin,
        EcCode::ConstraintGeom,
        EcCode::ConstraintStack,
        EcCode::ConstraintAnchor,
        EcCode::ConstraintEpoch,
        EcCode::LinkSilent,
        EcCode::LinkBufferFull,
        EcCode::LinkResync,
        EcCode::LinkTimeout,
    ];
}

/// 按码值取中文说明；越界返回 UNKNOWN。
pub fn detail(code: u32) -> &'static str {
    let mut i = 0usize;
    while i < EcCode::ALL.len() {
        if let Some(c) = EcCode::ALL.get(i) {
            if c.val() == code {
                return c.detail();
            }
        }
        i += 1;
    }
    UNKNOWN
}

pub fn known_code(code: u32) -> bool {
    let mut i = 0usize;
    while i < EcCode::ALL.len() {
        if let Some(c) = EcCode::ALL.get(i) {
            if c.val() == code {
                return true;
            }
        }
        i += 1;
    }
    false
}

/// 段位守卫：14 条码逐位落段且互不重复。
pub fn segment_guard() -> bool {
    let mut i = 0usize;
    while i < EcCode::ALL.len() {
        let c = match EcCode::ALL.get(i) {
            Some(v) => *v,
            None => return false,
        };
        if c.val() != SEG_BASE + 0x01 + i as u32 {
            return false;
        }
        let mut j = i + 1;
        while j < EcCode::ALL.len() {
            if let Some(o) = EcCode::ALL.get(j) {
                if o.val() == c.val() {
                    return false;
                }
            }
            j += 1;
        }
        i += 1;
    }
    true
}

// ── 分类与路由 ────────────────────────────────────────────────────
/// 三类故障 + 未归类。顺序即优先级（用于时序路的迁移白名单下标）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum EcClass {
    FocusLost,
    Constraint,
    LinkDown,
    Unclassified,
}

impl EcClass {
    pub fn index(self) -> u8 {
        match self {
            EcClass::FocusLost => 0,
            EcClass::Constraint => 1,
            EcClass::LinkDown => 2,
            EcClass::Unclassified => 3,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            EcClass::FocusLost => "焦点丢失",
            EcClass::Constraint => "约束冲突",
            EcClass::LinkDown => "失联",
            EcClass::Unclassified => "未归类",
        }
    }

    pub fn from_index(i: u8) -> Option<EcClass> {
        match i {
            0 => Some(EcClass::FocusLost),
            1 => Some(EcClass::Constraint),
            2 => Some(EcClass::LinkDown),
            3 => Some(EcClass::Unclassified),
            _ => None,
        }
    }

    /// 声明的条数（未归类 1 条 = 越界哨）。
    pub fn declared(self) -> u32 {
        match self {
            EcClass::FocusLost => FOCUS_ROWS,
            EcClass::Constraint => CONSTRAINT_ROWS,
            EcClass::LinkDown => LINK_ROWS,
            EcClass::Unclassified => UNKNOWN_ROWS,
        }
    }
}

/// 归属路由：事件层 / 约束层 / vxwm 桥 / 上层 API / 未知。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Route {
    EventLayer,
    ConstraintLayer,
    VxwmBridge,
    UpperApi,
    Nobody,
}

impl Route {
    pub fn index(self) -> u8 {
        match self {
            Route::EventLayer => 0,
            Route::ConstraintLayer => 1,
            Route::VxwmBridge => 2,
            Route::UpperApi => 3,
            Route::Nobody => 4,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Route::EventLayer => "事件层",
            Route::ConstraintLayer => "约束层",
            Route::VxwmBridge => "vxwm 桥",
            Route::UpperApi => "上层 API",
            Route::Nobody => "无人认领",
        }
    }

    pub fn from_index(i: u8) -> Option<Route> {
        match i {
            0 => Some(Route::EventLayer),
            1 => Some(Route::ConstraintLayer),
            2 => Some(Route::VxwmBridge),
            3 => Some(Route::UpperApi),
            4 => Some(Route::Nobody),
            _ => None,
        }
    }
}

// ── 15 条码表 ─────────────────────────────────────────────────────
/// 码表一行：码值 / 英文名 / 中文 / 严重度 / 分类 / 路由。六元组即「冻结单元」。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ErrRow {
    pub code: u32,
    pub name: &'static str,
    pub cn: &'static str,
    pub severity: u8,
    pub class: EcClass,
    pub route: Route,
}

/// 15 条冻结码表（手写；码值按 0x73180 手算，分类与路由逐行显式）。
pub const TABLE: [ErrRow; ROWS] = [
    ErrRow { code: 0x73181, name: "FOCUS_LOST_PRIMARY", cn: "焦点丢失：主窗口被合成器摘除", severity: 3, class: EcClass::FocusLost, route: Route::EventLayer },
    ErrRow { code: 0x73182, name: "FOCUS_LOST_REPLICA", cn: "焦点丢失：副本窗口未同步", severity: 2, class: EcClass::FocusLost, route: Route::EventLayer },
    ErrRow { code: 0x73183, name: "FOCUS_LOST_ROLLBACK", cn: "焦点丢失：回滚失败", severity: 3, class: EcClass::FocusLost, route: Route::EventLayer },
    ErrRow { code: 0x73184, name: "FOCUS_LOST_STALE", cn: "焦点丢失：焦点账已过期", severity: 2, class: EcClass::FocusLost, route: Route::VxwmBridge },
    ErrRow { code: 0x73185, name: "FOCUS_LOST_RELAY", cn: "焦点丢失：传送窗口无应答", severity: 2, class: EcClass::FocusLost, route: Route::UpperApi },
    ErrRow { code: 0x73186, name: "CONSTRAINT_PIN", cn: "约束冲突：最小尺寸与最大值矛盾", severity: 2, class: EcClass::Constraint, route: Route::ConstraintLayer },
    ErrRow { code: 0x73187, name: "CONSTRAINT_GEOM", cn: "约束冲突：几何越出合成器可接受域", severity: 2, class: EcClass::Constraint, route: Route::ConstraintLayer },
    ErrRow { code: 0x73188, name: "CONSTRAINT_STACK", cn: "约束冲突：堆叠次序成环", severity: 3, class: EcClass::Constraint, route: Route::ConstraintLayer },
    ErrRow { code: 0x73189, name: "CONSTRAINT_ANCHOR", cn: "约束冲突：锚点悬空", severity: 2, class: EcClass::Constraint, route: Route::VxwmBridge },
    ErrRow { code: 0x7318A, name: "CONSTRAINT_EPOCH", cn: "约束冲突：epoch 落后于约束版本", severity: 1, class: EcClass::Constraint, route: Route::UpperApi },
    ErrRow { code: 0x7318B, name: "LINK_SILENT", cn: "失联：心跳静默超窗", severity: 3, class: EcClass::LinkDown, route: Route::VxwmBridge },
    ErrRow { code: 0x7318C, name: "LINK_BUFFER_FULL", cn: "失联：缓冲已打满拒收", severity: 2, class: EcClass::LinkDown, route: Route::EventLayer },
    ErrRow { code: 0x7318D, name: "LINK_RESYNC", cn: "失联：重同步握手失败", severity: 3, class: EcClass::LinkDown, route: Route::VxwmBridge },
    ErrRow { code: 0x7318E, name: "LINK_TIMEOUT", cn: "失联：超时窗口耗尽", severity: 2, class: EcClass::LinkDown, route: Route::EventLayer },
    ErrRow { code: 0x7318F, name: "UNKNOWN_CODE", cn: UNKNOWN, severity: 0, class: EcClass::Unclassified, route: Route::Nobody },
];

/// 手写期望码值表（冻结用；与实际码值比较即「零漂移」的硬证据）。
pub const EXPECTED_CODES: [u32; ROWS] = [
    0x73181, 0x73182, 0x73183, 0x73184, 0x73185, 0x73186, 0x73187, 0x73188, 0x73189, 0x7318A,
    0x7318B, 0x7318C, 0x7318D, 0x7318E, 0x7318F,
];

/// 英文名词法：UPPER_SNAKE，[A-Z0-9_]，长度 6..=40，无双下划线、无首尾下划线。
pub fn name_syntax_ok(name: &str) -> bool {
    if name.len() < NAME_MIN_LEN || name.len() > NAME_MAX_LEN {
        return false;
    }
    let bytes = name.as_bytes();
    let first = match bytes.first() {
        Some(v) => *v,
        None => return false,
    };
    if !(b'A'..=b'Z').contains(&first) {
        return false;
    }
    let last = match bytes.last() {
        Some(v) => *v,
        None => return false,
    };
    if !(b'A'..=b'Z').contains(&last) && !(b'0'..=b'9').contains(&last) {
        return false;
    }
    let mut i = 0usize;
    let mut prev_us = false;
    while i < bytes.len() {
        let b = match bytes.get(i) {
            Some(v) => *v,
            None => return false,
        };
        let up = (b'A'..=b'Z').contains(&b);
        let dig = (b'0'..=b'9').contains(&b);
        let us = b == b'_';
        if !up && !dig && !us {
            return false;
        }
        if us && prev_us {
            return false;
        }
        prev_us = us;
        i += 1;
    }
    true
}

/// 中文说明长度（按 UTF-8 字符计，不按字节）：至少 CN_MIN_CHARS 个字。
pub fn cn_len_ok(cn: &str) -> bool {
    cn.chars().count() >= CN_MIN_CHARS
}

/// 按码值取行。
pub fn row_of(code: u32) -> Option<ErrRow> {
    let mut i = 0usize;
    while i < TABLE.len() {
        if let Some(r) = TABLE.get(i) {
            if r.code == code {
                return Some(*r);
            }
        }
        i += 1;
    }
    None
}

/// 按英文名取行（名称唯一性由审计保证）。
pub fn row_by_name(name: &str) -> Option<ErrRow> {
    let mut i = 0usize;
    while i < TABLE.len() {
        if let Some(r) = TABLE.get(i) {
            if r.name == name {
                return Some(*r);
            }
        }
        i += 1;
    }
    None
}

/// 分类（越界码归 Unclassified）。
pub fn class_of(code: u32) -> EcClass {
    match row_of(code) {
        Some(r) => r.class,
        None => EcClass::Unclassified,
    }
}

/// 严重度（越界码为 0）。
pub fn severity_of(code: u32) -> u8 {
    match row_of(code) {
        Some(r) => r.severity,
        None => 0,
    }
}

/// 归属路由（越界码为 Nobody）。
pub fn route_of(code: u32) -> Route {
    match row_of(code) {
        Some(r) => r.route,
        None => Route::Nobody,
    }
}

/// 类内第 idx 行（idx 从 0 起）。
pub fn class_row(class: EcClass, idx: u32) -> Option<ErrRow> {
    let mut n = 0u32;
    let mut i = 0usize;
    while i < TABLE.len() {
        if let Some(r) = TABLE.get(i) {
            if r.class == class {
                if n == idx {
                    return Some(*r);
                }
                n = n.saturating_add(1);
            }
        }
        i += 1;
    }
    None
}

/// 某类实际条数（直接数表，不复用审计累加器）。
pub fn class_count(class: EcClass) -> u32 {
    let mut n = 0u32;
    let mut i = 0usize;
    while i < TABLE.len() {
        if let Some(r) = TABLE.get(i) {
            if r.class == class {
                n = n.saturating_add(1);
            }
        }
        i += 1;
    }
    n
}

/// 某类的码值折叠摘要（互不混淆的旁证：三类摘要必须两两不同）。
pub fn class_digest(class: EcClass) -> u64 {
    let mut h = p_core::FNV_OFF;
    h = p_core::fold_byte(h, class.index());
    let mut i = 0usize;
    while i < TABLE.len() {
        if let Some(r) = TABLE.get(i) {
            if r.class == class {
                h = p_core::fold_u32(h, r.code);
                h = p_core::fold_byte(h, r.severity);
                h = p_core::fold_byte(h, r.route.index());
                let mut k = 0usize;
                while k < r.name.len() {
                    if let Some(b) = r.name.as_bytes().get(k).copied() {
                        h = p_core::fold_byte(h, b);
                    }
                    k += 1;
                }
            }
        }
        i += 1;
    }
    h
}

/// 时序迁移白名单：焦点丢失 ↔ 约束冲突 ↔ 失联。未归类只能在两端出现。
///
/// 语义依据：焦点丢失常伴随约束重算（同一帧内），约束冲突升级为失联；
/// 失联恢复后允许直接回到焦点丢失（此时焦点账往往已过期）。
pub fn class_step_ok(from: EcClass, to: EcClass) -> bool {
    match (from, to) {
        (EcClass::FocusLost, EcClass::FocusLost) => true,
        (EcClass::FocusLost, EcClass::Constraint) => true,
        (EcClass::Constraint, EcClass::Constraint) => true,
        (EcClass::Constraint, EcClass::LinkDown) => true,
        (EcClass::Constraint, EcClass::FocusLost) => true,
        (EcClass::LinkDown, EcClass::LinkDown) => true,
        (EcClass::LinkDown, EcClass::FocusLost) => true,
        (EcClass::FocusLost, EcClass::LinkDown) => true,
        (EcClass::Unclassified, EcClass::Unclassified) => true,
        _ => false,
    }
}

// ── 码表审计 ──────────────────────────────────────────────────────
/// 码表账：总行数、三类 + 哨条数、名称/中文重复数、路由种类数、摘要。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct TableLedger {
    pub rows: u32,
    pub focus: u32,
    pub constraint: u32,
    pub link: u32,
    pub unknown: u32,
    pub dup_names: u32,
    pub dup_cn: u32,
    pub routes: u32,
    pub digest: u64,
}

impl TableLedger {
    /// 守恒：行数 = 三类 + 哨；三类条数与声明一致。
    pub fn conserved(&self) -> bool {
        let sum = self
            .focus
            .saturating_add(self.constraint)
            .saturating_add(self.link)
            .saturating_add(self.unknown);
        if sum != self.rows {
            return false;
        }
        if self.rows != ROWS as u32 {
            return false;
        }
        if self.focus != FOCUS_ROWS
            || self.constraint != CONSTRAINT_ROWS
            || self.link != LINK_ROWS
            || self.unknown != UNKNOWN_ROWS
        {
            return false;
        }
        self.dup_names == 0 && self.dup_cn == 0
    }
}

/// 审计整张码表：码值 → 词法 → 中文 → 分类 → 路由 → 快照。
pub fn audit_table() -> Result<TableLedger, u32> {
    if TABLE.len() != ROWS {
        return Err(AUDIT_SEGMENT);
    }
    let mut focus = 0u32;
    let mut constraint = 0u32;
    let mut link = 0u32;
    let mut unknown = 0u32;
    let mut dup_names = 0u32;
    let mut dup_cn = 0u32;
    let mut routes_mask: u8 = 0;
    let mut h = p_core::FNV_OFF;
    let mut i = 0usize;
    while i < ROWS {
        let r = match TABLE.get(i) {
            Some(v) => *v,
            None => return Err(AUDIT_SEGMENT),
        };
        // ① 码值：逐位与手写期望表比对——这就是零漂移的硬门。
        let want = match EXPECTED_CODES.get(i) {
            Some(v) => *v,
            None => return Err(AUDIT_SEGMENT),
        };
        if r.code != want {
            return Err(AUDIT_SEGMENT);
        }
        if i + 1 < ROWS {
            if r.code != SEG_BASE + 0x01 + i as u32 {
                return Err(AUDIT_SEGMENT);
            }
            if !known_code(r.code) {
                return Err(AUDIT_SEGMENT);
            }
        } else {
            if r.code != SEG_BASE + 0x0F || known_code(r.code) {
                return Err(AUDIT_SEGMENT);
            }
            if r.class != EcClass::Unclassified || r.route != Route::Nobody {
                return Err(AUDIT_ROUTE);
            }
        }
        // ② 词法：英文名 + 中文说明。
        if !name_syntax_ok(r.name) {
            return Err(AUDIT_NAME);
        }
        if !cn_len_ok(r.cn) {
            return Err(AUDIT_NAME);
        }
        // ③ 分类：三类必须与码值区间一致（+01..+05 / +06..+0A / +0B..+0E）。
        let want_class = if i < FOCUS_ROWS as usize {
            EcClass::FocusLost
        } else if i < (FOCUS_ROWS + CONSTRAINT_ROWS) as usize {
            EcClass::Constraint
        } else if i < (FOCUS_ROWS + CONSTRAINT_ROWS + LINK_ROWS) as usize {
            EcClass::LinkDown
        } else {
            EcClass::Unclassified
        };
        if r.class != want_class {
            return Err(AUDIT_CLASS);
        }
        // ④ 路由：严重度 1..=3（哨为 0），路由下标 0..=4 且不属于 Nobody（哨除外）。
        if r.severity > 3 {
            return Err(AUDIT_ROUTE);
        }
        if r.class == EcClass::Unclassified {
            if r.severity != 0 {
                return Err(AUDIT_ROUTE);
            }
        } else if r.severity == 0 {
            return Err(AUDIT_ROUTE);
        }
        if r.route.index() > 4 {
            return Err(AUDIT_ROUTE);
        }
        if r.class != EcClass::Unclassified && r.route == Route::Nobody {
            return Err(AUDIT_ROUTE);
        }
        routes_mask |= 1u8 << r.route.index();
        // ⑤ 唯一性：名称与中文都必须两两不同（互不混淆的文本前提）。
        let mut j = i + 1;
        while j < ROWS {
            if let Some(o) = TABLE.get(j) {
                if o.name == r.name || o.code == r.code {
                    dup_names = dup_names.saturating_add(1);
                }
                if o.cn == r.cn {
                    dup_cn = dup_cn.saturating_add(1);
                }
            }
            j += 1;
        }
        match r.class {
            EcClass::FocusLost => focus = focus.saturating_add(1),
            EcClass::Constraint => constraint = constraint.saturating_add(1),
            EcClass::LinkDown => link = link.saturating_add(1),
            EcClass::Unclassified => unknown = unknown.saturating_add(1),
        }
        h = p_core::fold_u32(h, r.code);
        h = p_core::fold_byte(h, r.severity);
        h = p_core::fold_byte(h, r.class.index());
        h = p_core::fold_byte(h, r.route.index());
        i += 1;
    }
    let mut routes = 0u32;
    let mut bit = 0u8;
    while bit < 8 {
        if routes_mask & (1u8 << bit) != 0 {
            routes = routes.saturating_add(1);
        }
        bit += 1;
    }
    let l = TableLedger {
        rows: ROWS as u32,
        focus,
        constraint,
        link,
        unknown,
        dup_names,
        dup_cn,
        routes,
        digest: h,
    };
    if !l.conserved() {
        return Err(AUDIT_TABLE);
    }
    if l.routes != 5 {
        return Err(AUDIT_ROUTE);
    }
    if l.digest == 0 {
        return Err(AUDIT_TABLE);
    }
    Ok(l)
}

// ── 段内联轧（F49054） ────────────────────────────────────────────
/// 一路的联轧结果：五条判据的真值 + 本路摘要。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct LaneReport {
    pub lane: u8,
    pub cells: [bool; CRITERIA],
    pub digest: u64,
}

impl LaneReport {
    /// 本路全绿。
    pub fn green(&self) -> bool {
        let mut i = 0usize;
        while i < CRITERIA {
            match self.cells.get(i) {
                Some(v) => {
                    if !*v {
                        return false;
                    }
                }
                None => return false,
            }
            i += 1;
        }
        true
    }

    pub fn red_count(&self) -> u32 {
        let mut n = 0u32;
        let mut i = 0usize;
        while i < CRITERIA {
            if let Some(v) = self.cells.get(i) {
                if !*v {
                    n = n.saturating_add(1);
                }
            }
            i += 1;
        }
        n
    }
}

/// 路名（下标即路号）。
pub const LANE_NAMES: [&str; LANES] = ["焦点路", "约束路", "失联路", "时序路", "渲染路"];

/// 判据名（下标即判据号）。
pub const CRITERIA_NAMES: [&str; CRITERIA] = [
    "计数一致",
    "码值连续",
    "词法唯一",
    "分类归属",
    "摘要非零",
];

/// 焦点路：五条判据全都围绕「焦点丢失类」。
pub fn lane_focus() -> Result<LaneReport, u32> {
    let l = audit_table()?;
    let n = class_count(EcClass::FocusLost);
    let mut contiguous = true;
    let mut i = 0u32;
    while i < FOCUS_ROWS {
        let r = match class_row(EcClass::FocusLost, i) {
            Some(v) => v,
            None => {
                contiguous = false;
                break;
            }
        };
        if r.code != SEG_BASE + 0x01 + i {
            contiguous = false;
        }
        i += 1;
    }
    let mut lexical = true;
    i = 0;
    while i < FOCUS_ROWS {
        if let Some(r) = class_row(EcClass::FocusLost, i) {
            if !name_syntax_ok(r.name) || !r.name.starts_with("FOCUS_LOST_") {
                lexical = false;
            }
        } else {
            lexical = false;
        }
        i += 1;
    }
    let mut classified = true;
    i = 0;
    while i < FOCUS_ROWS {
        if let Some(r) = class_row(EcClass::FocusLost, i) {
            if class_of(r.code) != EcClass::FocusLost || route_of(r.code) == Route::Nobody {
                classified = false;
            }
        }
        i += 1;
    }
    let mut h = p_core::FNV_OFF;
    h = p_core::fold_u32(h, n);
    h = p_core::fold_u32(h, l.focus);
    h = p_core::fold_u32(h, (class_digest(EcClass::FocusLost) >> 32) as u32);
    h = p_core::fold_u32(h, class_digest(EcClass::FocusLost) as u32);
    Ok(LaneReport {
        lane: 0,
        cells: [
            n == FOCUS_ROWS && l.focus == FOCUS_ROWS,
            contiguous,
            lexical,
            classified,
            h != 0,
        ],
        digest: h,
    })
}

/// 约束路：五条判据全都围绕「约束冲突类」（重点查严重度分布与路由分散）。
pub fn lane_constraint() -> Result<LaneReport, u32> {
    let l = audit_table()?;
    let n = class_count(EcClass::Constraint);
    let mut contiguous = true;
    let mut i = 0u32;
    while i < CONSTRAINT_ROWS {
        let r = match class_row(EcClass::Constraint, i) {
            Some(v) => v,
            None => {
                contiguous = false;
                break;
            }
        };
        if r.code != SEG_BASE + 0x06 + i {
            contiguous = false;
        }
        i += 1;
    }
    let mut lexical = true;
    i = 0;
    while i < CONSTRAINT_ROWS {
        if let Some(r) = class_row(EcClass::Constraint, i) {
            if !name_syntax_ok(r.name) || !r.name.starts_with("CONSTRAINT_") {
                lexical = false;
            }
            if r.severity == 0 || r.severity > 3 {
                lexical = false;
            }
        } else {
            lexical = false;
        }
        i += 1;
    }
    // 路由分散：约束类不得全部挤在一个路由上（否则路由表退化成常数）。
    let mut mask = 0u8;
    i = 0;
    while i < CONSTRAINT_ROWS {
        if let Some(r) = class_row(EcClass::Constraint, i) {
            mask |= 1u8 << r.route.index();
        }
        i += 1;
    }
    let mut spread = 0u32;
    let mut bit = 0u8;
    while bit < 8 {
        if mask & (1u8 << bit) != 0 {
            spread = spread.saturating_add(1);
        }
        bit += 1;
    }
    let mut h = p_core::FNV_OFF;
    h = p_core::fold_u32(h, n);
    h = p_core::fold_u32(h, l.constraint);
    h = p_core::fold_u32(h, spread);
    h = p_core::fold_u32(h, (class_digest(EcClass::Constraint) >> 32) as u32);
    Ok(LaneReport {
        lane: 1,
        cells: [
            n == CONSTRAINT_ROWS && l.constraint == CONSTRAINT_ROWS,
            contiguous,
            lexical,
            spread >= 3,
            h != 0,
        ],
        digest: h,
    })
}

/// 失联路：五条判据全都围绕「失联类」（重点查与心跳家族的呼应）。
pub fn lane_link() -> Result<LaneReport, u32> {
    let l = audit_table()?;
    let n = class_count(EcClass::LinkDown);
    let mut contiguous = true;
    let mut i = 0u32;
    while i < LINK_ROWS {
        let r = match class_row(EcClass::LinkDown, i) {
            Some(v) => v,
            None => {
                contiguous = false;
                break;
            }
        };
        if r.code != SEG_BASE + 0x0B + i {
            contiguous = false;
        }
        i += 1;
    }
    let mut lexical = true;
    i = 0;
    while i < LINK_ROWS {
        if let Some(r) = class_row(EcClass::LinkDown, i) {
            if !name_syntax_ok(r.name) || !r.name.starts_with("LINK_") {
                lexical = false;
            }
        } else {
            lexical = false;
        }
        i += 1;
    }
    // 失联必须同时压到「事件层」与「vxwm 桥」两侧，否则恢复路径断了半边。
    let mut event_side = false;
    let mut bridge_side = false;
    i = 0;
    while i < LINK_ROWS {
        if let Some(r) = class_row(EcClass::LinkDown, i) {
            if r.route == Route::EventLayer {
                event_side = true;
            }
            if r.route == Route::VxwmBridge {
                bridge_side = true;
            }
        }
        i += 1;
    }
    let mut h = p_core::FNV_OFF;
    h = p_core::fold_u32(h, n);
    h = p_core::fold_u32(h, l.link);
    h = p_core::fold_u32(h, class_digest(EcClass::LinkDown) as u32);
    Ok(LaneReport {
        lane: 2,
        cells: [
            n == LINK_ROWS && l.link == LINK_ROWS,
            contiguous,
            lexical,
            event_side && bridge_side,
            h != 0,
        ],
        digest: h,
    })
}

/// 时序路：用一条确定性故障流压分类迁移白名单（非法迁移即红）。
pub fn lane_timing() -> Result<LaneReport, u32> {
    // 一条手写的合法流：焦点 → 约束 → 失联 → 焦点 → 焦点 → 约束。
    const STREAM: [u32; 6] = [0x73181, 0x73187, 0x7318B, 0x73183, 0x73185, 0x73189];
    let mut ok_step = true;
    let mut ok_class = true;
    let mut prev = EcClass::Unclassified;
    let mut h = p_core::FNV_OFF;
    let mut i = 0usize;
    while i < STREAM.len() {
        let code = match STREAM.get(i) {
            Some(v) => *v,
            None => return Err(AUDIT_SEGMENT),
        };
        let cur = class_of(code);
        if cur == EcClass::Unclassified {
            ok_class = false;
        }
        // 首条是「从无到有」的进入，不参与迁移白名单；从第二条起才考迁移。
        if i > 0 && !class_step_ok(prev, cur) {
            ok_step = false;
        }
        prev = cur;
        h = p_core::fold_u32(h, code);
        h = p_core::fold_u32(h, cur.index() as u32);
        i += 1;
    }
    // 反向证据：白名单必须真的会拒（否则「全绿」是空判）。
    let mut rejects = 0u32;
    if !class_step_ok(EcClass::LinkDown, EcClass::Constraint) {
        rejects = rejects.saturating_add(1);
    }
    if !class_step_ok(EcClass::Unclassified, EcClass::FocusLost) {
        rejects = rejects.saturating_add(1);
    }
    let mut sorted_ok = true;
    let mut k = 1usize;
    while k < STREAM.len() {
        let a = match STREAM.get(k - 1) {
            Some(v) => *v,
            None => return Err(AUDIT_SEGMENT),
        };
        let b = match STREAM.get(k) {
            Some(v) => *v,
            None => return Err(AUDIT_SEGMENT),
        };
        if a == b {
            sorted_ok = false;
        }
        k += 1;
    }
    Ok(LaneReport {
        lane: 3,
        cells: [
            STREAM.len() as u32 == 6,
            ok_step,
            ok_class,
            rejects == 2,
            h != 0 && sorted_ok,
        ],
        digest: h,
    })
}

/// 渲染路：把 15 行码表折成可解析账行，再逐字段回读。
pub fn lane_render() -> Result<LaneReport, u32> {
    let l = audit_table()?;
    let row = table_row(&l);
    let parsed = row_fields(&row);
    let mut field_ok = false;
    let mut back_ok = false;
    let mut digest_ok = false;
    match parsed {
        Some((tag, vals, digest)) => {
            field_ok = tag == TABLE_TAG
                && vals[0] == ROWS as u32
                && vals[1] == FOCUS_ROWS
                && vals[2] == CONSTRAINT_ROWS
                && vals[3] == LINK_ROWS;
            digest_ok = digest == l.digest && digest != 0;
            back_ok = audit_render(&row);
        }
        None => {}
    }
    // 第二张：分类账行（三类摘要折一行），必须与码表账行解析口径一致。
    let cl = class_row_line();
    let cl_ok = audit_render(&cl);
    let mut h = p_core::FNV_OFF;
    h = p_core::fold_u32(h, l.digest as u32);
    h = p_core::fold_u32(h, (l.digest >> 32) as u32);
    Ok(LaneReport {
        lane: 4,
        cells: [
            field_ok,
            back_ok,
            digest_ok,
            cl_ok,
            h != 0,
        ],
        digest: h,
    })
}

/// 联轧矩阵：5 路 × 5 判据的真值格 + 每格摘要 + 总分摘要。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct RollMatrix {
    pub cells: [[bool; CRITERIA]; LANES],
    pub lane_digest: [u64; LANES],
    pub red: u32,
    pub digest: u64,
}

impl RollMatrix {
    pub fn all_green(&self) -> bool {
        let mut i = 0usize;
        while i < LANES {
            let row = match self.cells.get(i) {
                Some(v) => v,
                None => return false,
            };
            let mut j = 0usize;
            while j < CRITERIA {
                match row.get(j) {
                    Some(v) => {
                        if !*v {
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

    /// 任一路红则整体红：只要有一路不是全绿，整批就不是全绿。
    pub fn overall_green(&self) -> bool {
        self.all_green() && self.red == 0
    }

    pub fn lane_green(&self, lane: usize) -> bool {
        let row = match self.cells.get(lane) {
            Some(v) => v,
            None => return false,
        };
        let mut j = 0usize;
        while j < CRITERIA {
            match row.get(j) {
                Some(v) => {
                    if !*v {
                        return false;
                    }
                }
                None => return false,
            }
            j += 1;
        }
        true
    }
}

/// 跑一路（按路号分派）。索引越界给 RouteBad。
pub fn run_lane(lane: u8) -> Result<LaneReport, u32> {
    match lane {
        0 => lane_focus(),
        1 => lane_constraint(),
        2 => lane_link(),
        3 => lane_timing(),
        4 => lane_render(),
        _ => Err(AUDIT_LANE),
    }
}

/// 联轧一次：跑满五路，装成矩阵。
pub fn roll_once() -> Result<RollMatrix, u32> {
    let mut cells = [[false; CRITERIA]; LANES];
    let mut lane_digest = [0u64; LANES];
    let mut red = 0u32;
    let mut h = p_core::FNV_OFF;
    let mut lane = 0u8;
    while (lane as usize) < LANES {
        let rep = run_lane(lane)?;
        if rep.lane != lane {
            return Err(AUDIT_LANE);
        }
        if rep.digest == 0 {
            return Err(AUDIT_ROLL);
        }
        let mut j = 0usize;
        while j < CRITERIA {
            let v = match rep.cells.get(j) {
                Some(x) => *x,
                None => return Err(AUDIT_ROLL),
            };
            if let Some(slot) = cells.get_mut(lane as usize) {
                if let Some(c) = slot.get_mut(j) {
                    *c = v;
                }
            }
            if !v {
                red = red.saturating_add(1);
            }
            j += 1;
        }
        if let Some(slot) = lane_digest.get_mut(lane as usize) {
            *slot = rep.digest;
        }
        h = p_core::fold_u32(h, rep.digest as u32);
        h = p_core::fold_u32(h, (rep.digest >> 32) as u32);
        h = p_core::fold_u32(h, rep.red_count());
        lane = lane.saturating_add(1);
    }
    if h == 0 {
        return Err(AUDIT_ROLL);
    }
    Ok(RollMatrix {
        cells,
        lane_digest,
        red,
        digest: h,
    })
}

/// 交叉复跑：两遍矩阵必须逐格、逐摘要相同。
pub fn roll_replay() -> Result<(RollMatrix, RollMatrix), u32> {
    let a = roll_once()?;
    let b = roll_once()?;
    Ok((a, b))
}

/// 五路互不混淆：五路摘要两两不同（同一份表被五路读成五个视角，不该产同一摘要）。
pub fn lanes_distinct(m: &RollMatrix) -> bool {
    let mut i = 0usize;
    while i < LANES {
        let a = match m.lane_digest.get(i) {
            Some(v) => *v,
            None => return false,
        };
        let mut j = i + 1;
        while j < LANES {
            if let Some(b) = m.lane_digest.get(j) {
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

// ── 病态表注入 ────────────────────────────────────────────────────
/// 十种病态行：每一种都必须被 `audit_row` 拒绝。
pub fn audit_row(r: &ErrRow, idx: usize) -> Result<(), u32> {
    let want = match EXPECTED_CODES.get(idx) {
        Some(v) => *v,
        None => return Err(AUDIT_SEGMENT),
    };
    if r.code != want {
        return Err(AUDIT_SEGMENT);
    }
    if !name_syntax_ok(r.name) {
        return Err(AUDIT_NAME);
    }
    if !cn_len_ok(r.cn) {
        return Err(AUDIT_NAME);
    }
    if r.severity > 3 {
        return Err(AUDIT_ROUTE);
    }
    if r.class != class_of(r.code) {
        return Err(AUDIT_CLASS);
    }
    if r.route != route_of(r.code) {
        return Err(AUDIT_ROUTE);
    }
    Ok(())
}

/// 病态注入：返回 (拒绝数, 接受数)。接受数必须为 0。
pub fn table_faults() -> Result<(u32, u32), u32> {
    let base = match TABLE.first() {
        Some(v) => *v,
        None => return Err(AUDIT_TABLE),
    };
    let mut rejected = 0u32;
    let mut accepted = 0u32;
    let mut i = 0usize;
    while i < FAULT_INJECT as usize {
        let bad = match i {
            // ① 码值偏移
            0 => ErrRow { code: base.code + 1, ..base },
            // ② 英文名小写
            1 => ErrRow { name: "focus_lost_primary", ..base },
            // ③ 英文名带空格
            2 => ErrRow { name: "FOCUS LOST PRIMARY", ..base },
            // ④ 中文过短（不足 8 字）
            3 => ErrRow { cn: "焦点丢失", ..base },
            // ⑤ 严重度越界
            4 => ErrRow { severity: 4, ..base },
            // ⑥ 分类错位
            5 => ErrRow { class: EcClass::Constraint, ..base },
            // ⑦ 路由错位（改成无人认领）
            6 => ErrRow { route: Route::Nobody, ..base },
            // ⑧ 名称全是下划线
            7 => ErrRow { name: "__________", ..base },
            // ⑨ 名称为空
            8 => ErrRow { name: "", ..base },
            // ⑩ 中文为空
            _ => ErrRow { cn: "", ..base },
        };
        if audit_row(&bad, 0).is_err() {
            rejected = rejected.saturating_add(1);
        } else {
            accepted = accepted.saturating_add(1);
        }
        i += 1;
    }
    Ok((rejected, accepted))
}

// ── 六字段账行 ────────────────────────────────────────────────────
pub fn render_row(tag: &str, a: u32, b: u32, c: u32, d: u32, digest: u64) -> String {
    format!(
        "{}|k1={}|k2={}|k3={}|k4={}|digest={:016X}",
        tag, a, b, c, d, digest
    )
}

pub fn parse_u32(s: &str) -> Option<u32> {
    if s.is_empty() || s.len() > 10 {
        return None;
    }
    let mut v: u32 = 0;
    let mut i = 0usize;
    while i < s.len() {
        let b = match s.as_bytes().get(i).copied() {
            Some(v) => v,
            None => return None,
        };
        if !(b'0'..=b'9').contains(&b) {
            return None;
        }
        v = v.checked_mul(10)?.checked_add((b - b'0') as u32)?;
        i += 1;
    }
    Some(v)
}

pub fn parse_hex16(s: &str) -> Option<u64> {
    if s.len() != 16 {
        return None;
    }
    let mut v: u64 = 0;
    let mut i = 0usize;
    while i < 16 {
        let b = match s.as_bytes().get(i).copied() {
            Some(v) => v,
            None => return None,
        };
        let d = match b {
            b'0'..=b'9' => (b - b'0') as u64,
            b'a'..=b'f' => (b - b'a') as u64 + 10,
            b'A'..=b'F' => (b - b'A') as u64 + 10,
            _ => return None,
        };
        v = v.checked_mul(16)?.checked_add(d)?;
        i += 1;
    }
    Some(v)
}

pub fn row_fields(row: &str) -> Option<(&str, [u32; 4], u64)> {
    let mut parts: [Option<&str>; RENDER_FIELDS] = [None; RENDER_FIELDS];
    let mut n = 0usize;
    let mut it = row.split('|');
    while let Some(p) = it.next() {
        if n >= RENDER_FIELDS {
            return None;
        }
        if let Some(slot) = parts.get_mut(n) {
            *slot = Some(p);
        }
        n += 1;
    }
    if n != RENDER_FIELDS {
        return None;
    }
    let tag = match parts.first().and_then(|x| *x) {
        Some(v) if !v.is_empty() => v,
        _ => return None,
    };
    let keys: [&str; 5] = ["k1", "k2", "k3", "k4", "digest"];
    let mut vals = [0u32; 4];
    let mut i = 1usize;
    while i < 5 {
        let p = match parts.get(i).and_then(|x| *x) {
            Some(v) => v,
            None => return None,
        };
        let mut sp = p.split('=');
        let key = match sp.next() {
            Some(v) => v,
            None => return None,
        };
        if Some(key) != keys.get(i - 1).copied() {
            return None;
        }
        let num = match sp.next().and_then(parse_u32) {
            Some(v) => v,
            None => return None,
        };
        if sp.next().is_some() {
            return None;
        }
        if let Some(slot) = vals.get_mut(i - 1) {
            *slot = num;
        }
        i += 1;
    }
    let dpart = match parts.get(5).and_then(|x| *x) {
        Some(v) => v,
        None => return None,
    };
    let mut sp = dpart.split('=');
    if sp.next() != Some("digest") {
        return None;
    }
    let digest = match sp.next().and_then(parse_hex16) {
        Some(v) => v,
        None => return None,
    };
    if sp.next().is_some() {
        return None;
    }
    Some((tag, vals, digest))
}

pub fn audit_render(row: &str) -> bool {
    match row_fields(row) {
        Some((tag, vals, digest)) => render_row(tag, vals[0], vals[1], vals[2], vals[3], digest) == row,
        None => false,
    }
}

/// 码表账行：行数、焦点数、约束数、失联数。
pub fn table_row(l: &TableLedger) -> String {
    render_row(TABLE_TAG, l.rows, l.focus, l.constraint, l.link, l.digest)
}

/// 分类账行：三类摘要低 32 位折进四字段，摘要为三类摘要的折叠。
pub fn class_row_line() -> String {
    let f = class_digest(EcClass::FocusLost);
    let c = class_digest(EcClass::Constraint);
    let l = class_digest(EcClass::LinkDown);
    let mut h = p_core::FNV_OFF;
    h = p_core::fold_u32(h, (f >> 32) as u32);
    h = p_core::fold_u32(h, f as u32);
    h = p_core::fold_u32(h, (c >> 32) as u32);
    h = p_core::fold_u32(h, c as u32);
    h = p_core::fold_u32(h, (l >> 32) as u32);
    h = p_core::fold_u32(h, l as u32);
    render_row(
        CLASS_TAG,
        FOCUS_ROWS,
        CONSTRAINT_ROWS,
        LINK_ROWS,
        UNKNOWN_ROWS,
        h,
    )
}

/// 联轧账行：路数、判据数、红格数、总摘要高位。
pub fn roll_row(m: &RollMatrix) -> String {
    render_row(
        ROLL_TAG,
        LANES as u32,
        CRITERIA as u32,
        m.red,
        (m.digest >> 32) as u32,
        m.digest,
    )
}

// ── 用例编号（跨槽防撞号） ───────────────────────────────────────
/// 前序槽位占用（B12 族实占编号，逐字取自 B12 任务板）。
pub const RESERVED_IDS: [u32; 6] = [
    0x071C_8001,
    0x071C_8002,
    0x071C_8014,
    0x071D_0001,
    0x071D_2001,
    0x071D_2014,
];

/// 本模块 16 个用例编号（0x0734_0401..0x0734_0410）。
pub const CASE_IDS: [u32; CASE_TOTAL] = [
    0x0734_0401,
    0x0734_0402,
    0x0734_0403,
    0x0734_0404,
    0x0734_0405,
    0x0734_0406,
    0x0734_0407,
    0x0734_0408,
    0x0734_0409,
    0x0734_040A,
    0x0734_040B,
    0x0734_040C,
    0x0734_040D,
    0x0734_040E,
    0x0734_040F,
    0x0734_0410,
];

pub fn case_id(i: usize) -> Option<u32> {
    match CASE_IDS.get(i) {
        Some(v) => Some(*v),
        None => None,
    }
}

pub fn case_ids_tile() -> bool {
    let mut i = 0usize;
    while i < CASE_IDS.len() {
        let want = match CASE_BASE.checked_add(1 + i as u32) {
            Some(v) => v,
            None => return false,
        };
        match CASE_IDS.get(i) {
            Some(v) => {
                if *v != want {
                    return false;
                }
            }
            None => return false,
        }
        i += 1;
    }
    true
}

pub fn no_cross_collision() -> bool {
    let mut i = 0usize;
    while i < CASE_IDS.len() {
        let a = match CASE_IDS.get(i) {
            Some(v) => *v,
            None => return false,
        };
        let mut j = 0usize;
        while j < RESERVED_IDS.len() {
            if let Some(r) = RESERVED_IDS.get(j) {
                if a == *r {
                    return false;
                }
            }
            j += 1;
        }
        i += 1;
    }
    let mut k = 0usize;
    while k < CASE_IDS.len() {
        let a = match CASE_IDS.get(k) {
            Some(v) => *v,
            None => return false,
        };
        let mut m = k + 1;
        while m < CASE_IDS.len() {
            if let Some(b) = CASE_IDS.get(m) {
                if *b == a {
                    return false;
                }
            }
            m += 1;
        }
        k += 1;
    }
    true
}

// ── 探针 ─────────────────────────────────────────────────────────
/// 探针甲：码表计数与分类条数。
pub fn probe_seq_a() -> Result<(), u32> {
    let l = audit_table()?;
    if l.rows != 15 || l.focus != 5 || l.constraint != 5 || l.link != 4 || l.unknown != 1 {
        return Err(AUDIT_TABLE);
    }
    if class_count(EcClass::FocusLost) != FOCUS_ROWS
        || class_count(EcClass::Constraint) != CONSTRAINT_ROWS
        || class_count(EcClass::LinkDown) != LINK_ROWS
    {
        return Err(AUDIT_CLASS);
    }
    if !l.conserved() || !segment_guard() {
        return Err(AUDIT_SEGMENT);
    }
    Ok(())
}

/// 探针乙：分类与路由逐码反查。
pub fn probe_seq_b() -> Result<(), u32> {
    let mut i = 0usize;
    while i < EcCode::ALL.len() {
        let c = EcCode::ALL[i];
        let r = match row_of(c.val()) {
            Some(v) => v,
            None => return Err(AUDIT_TABLE),
        };
        if r.cn != c.detail() {
            return Err(AUDIT_NAME);
        }
        if class_of(c.val()) != r.class || route_of(c.val()) != r.route {
            return Err(AUDIT_CLASS);
        }
        if known_code(0x7318F) || class_of(0x7318F) != EcClass::Unclassified {
            return Err(AUDIT_CLASS);
        }
        i += 1;
    }
    if detail(0x7318F) != UNKNOWN || route_of(0x7318F) != Route::Nobody {
        return Err(AUDIT_ROUTE);
    }
    Ok(())
}

/// 探针丙：联轧全绿 + 复跑一致 + 五路互不混淆。
pub fn probe_seq_c() -> Result<(), u32> {
    let m = roll_once()?;
    if !m.overall_green() || m.red != 0 {
        return Err(AUDIT_ROLL);
    }
    if !lanes_distinct(&m) {
        return Err(AUDIT_LANE);
    }
    let (a, b) = roll_replay()?;
    if a.digest != b.digest || a.cells != b.cells {
        return Err(AUDIT_ROLL);
    }
    let row = roll_row(&m);
    if !audit_render(&row) {
        return Err(AUDIT_RENDER);
    }
    Ok(())
}

/// 复跑：码表审计与联轧各跑两遍。
pub fn repro_check() -> Result<(), u32> {
    let a = audit_table()?;
    let b = audit_table()?;
    if a != b {
        return Err(AUDIT_TABLE);
    }
    let (x, y) = roll_replay()?;
    if x.digest != y.digest || x.red != y.red {
        return Err(AUDIT_ROLL);
    }
    if class_digest(EcClass::FocusLost) == class_digest(EcClass::Constraint)
        || class_digest(EcClass::Constraint) == class_digest(EcClass::LinkDown)
        || class_digest(EcClass::FocusLost) == class_digest(EcClass::LinkDown)
    {
        return Err(AUDIT_CLASS);
    }
    Ok(())
}

// ── 判据集（恰好 8 条） ──────────────────────────────────────────
pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut out: Vec<(&'static str, bool, String)> = Vec::new();

    // ① 15 条谱系：行数 5/5/4/1、码值连续、守恒。
    let l = audit_table();
    let mut spec_ok = match &l {
        Ok(v) => v.conserved() && v.rows == 15 && v.digest != 0,
        Err(_) => false,
    };
    let pa = probe_seq_a();
    if pa.is_err() {
        spec_ok = false;
    }
    out.push((
        "F49053/码族15条谱",
        spec_ok,
        match &l {
            Ok(v) => format!(
                "focus={} constraint={} link={} unknown={} digest=0x{:016X}",
                v.focus, v.constraint, v.link, v.unknown, v.digest
            ),
            Err(e) => format!("err=0x{:X} ({})", e, detail(*e)),
        },
    ));

    // ② 词法：英文名 UPPER_SNAKE、中文 ≥8 字、两两不同。
    let mut lex_ok = true;
    let mut bad_name: &'static str = "";
    let mut i = 0usize;
    while i < ROWS {
        let r = TABLE[i];
        if !name_syntax_ok(r.name) || !cn_len_ok(r.cn) {
            lex_ok = false;
            bad_name = r.name;
        }
        i += 1;
    }
    if let Ok(v) = &l {
        if v.dup_names != 0 || v.dup_cn != 0 {
            lex_ok = false;
        }
    } else {
        lex_ok = false;
    }
    out.push((
        "F49053/名称词法唯一",
        lex_ok,
        format!(
            "rows={} dup_names={:?} dup_cn={:?} bad={}",
            ROWS,
            l.as_ref().map(|v| v.dup_names),
            l.as_ref().map(|v| v.dup_cn),
            if bad_name.is_empty() { "无" } else { bad_name }
        ),
    ));

    // ③ 分类与路由：每码唯一类、唯一路由；三类摘要两两不同。
    let pb = probe_seq_b();
    let distinct = class_digest(EcClass::FocusLost) != class_digest(EcClass::Constraint)
        && class_digest(EcClass::Constraint) != class_digest(EcClass::LinkDown)
        && class_digest(EcClass::FocusLost) != class_digest(EcClass::LinkDown);
    out.push((
        "F49053/分类路由唯一",
        pb.is_ok() && distinct,
        match (&l, &pb) {
            (Ok(v), Ok(())) => format!(
                "routes={} sel={} distinct={}",
                v.routes,
                route_of(0x73181).name(),
                distinct
            ),
            _ => format!(
                "err_l=0x{:X} pb={}",
                l.as_ref().err().copied().unwrap_or(0),
                pb.is_ok()
            ),
        },
    ));

    // ④ 零漂移快照：手写期望码值表逐位比对 + 两次审计摘要相同。
    let mut drift_ok = true;
    i = 0;
    while i < ROWS {
        if TABLE[i].code != EXPECTED_CODES[i] {
            drift_ok = false;
        }
        i += 1;
    }
    let twice = match (audit_table(), audit_table()) {
        (Ok(a), Ok(b)) => a.digest == b.digest && a.digest != 0,
        _ => false,
    };
    out.push((
        "F49053/零漂移快照",
        drift_ok && twice,
        format!(
            "expected_first=0x{:X} expected_last=0x{:X} twice={}",
            EXPECTED_CODES[0], EXPECTED_CODES[14], twice
        ),
    ));

    // ⑤ 联轧矩阵全绿（25 格）。
    let m = roll_once();
    let green = match &m {
        Ok(v) => v.overall_green() && v.red == 0,
        Err(_) => false,
    };
    out.push((
        "F49054/联轧矩阵全绿",
        green,
        match &m {
            Ok(v) => format!(
                "cells={} red={} lanes={:?}",
                CELLS,
                v.red,
                [
                    v.lane_green(0),
                    v.lane_green(1),
                    v.lane_green(2),
                    v.lane_green(3),
                    v.lane_green(4)
                ]
            ),
            Err(e) => format!("err=0x{:X} ({})", e, detail(*e)),
        },
    ));

    // ⑥ 交叉复跑零漂移（两遍矩阵逐格相同）。
    let rep = roll_replay();
    let rep_ok = match &rep {
        Ok((a, b)) => a.digest == b.digest && a.cells == b.cells && a.digest != 0,
        Err(_) => false,
    };
    out.push((
        "F49054/交叉复跑零漂移",
        rep_ok,
        match &rep {
            Ok((a, _)) => format!("digest=0x{:016X} red={} cells={}", a.digest, a.red, CELLS),
            Err(e) => format!("err=0x{:X} ({})", e, detail(*e)),
        },
    ));

    // ⑦ 五路互不混淆（五路摘要两两不同 + 路名各有）。
    let dis = match &m {
        Ok(v) => lanes_distinct(v),
        Err(_) => false,
    };
    out.push((
        "F49054/五路互不混淆",
        dis,
        match &m {
            Ok(v) => format!(
                "d0=0x{:016X} d4=0x{:016X} distinct={}",
                v.lane_digest[0], v.lane_digest[4], dis
            ),
            Err(e) => format!("err=0x{:X} ({})", e, detail(*e)),
        },
    ));

    // ⑧ 病态表捕获（10 拒 0 收）+ 段位 + 渲染往返。
    let faults = table_faults();
    let fault_ok = match &faults {
        Ok((r, a)) => *r == FAULT_INJECT && *a == 0,
        Err(_) => false,
    };
    let row = match &l {
        Ok(v) => table_row(v),
        Err(_) => String::new(),
    };
    let render_ok = !row.is_empty() && audit_render(&row) && audit_render(&class_row_line());
    out.push((
        "F49054/病态表捕获",
        fault_ok && render_ok && segment_guard(),
        format!(
            "rejected={:?} accepted={:?} render={} seg={}",
            faults.as_ref().map(|(r, _)| *r),
            faults.as_ref().map(|(_, a)| *a),
            render_ok,
            segment_guard()
        ),
    ));

    out
}

pub fn run_unx49053_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F49053 对接面错误码族 15 条");
    let items = extra_checks();
    let mut i = 0usize;
    while i < items.len() {
        if let Some((n, p, d)) = items.get(i) {
            s.add_dyn(n, *p, d);
        }
        i += 1;
    }
    s
}

// ── 单元判据 ──────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn error_codes_are_closed_and_segmented() {
        assert_eq!(EcCode::ALL.len(), 14);
        assert!(segment_guard());
        let mut i = 0usize;
        while i < EcCode::ALL.len() {
            assert_eq!(EcCode::ALL[i].val(), SEG_BASE + 0x01 + i as u32);
            assert!(known_code(EcCode::ALL[i].val()));
            i += 1;
        }
        assert!(!known_code(SEG_BASE));
        assert!(!known_code(SEG_BASE + 0x0F));
    }

    #[test]
    fn detail_lookup_roundtrip() {
        let mut i = 0usize;
        while i < EcCode::ALL.len() {
            assert_eq!(detail(EcCode::ALL[i].val()), EcCode::ALL[i].detail());
            i += 1;
        }
        assert_eq!(detail(0), UNKNOWN);
    }

    #[test]
    fn table_is_fifteen_and_conserved() {
        let l = audit_table().unwrap();
        assert_eq!(l.rows, 15);
        assert_eq!(l.focus, 5);
        assert_eq!(l.constraint, 5);
        assert_eq!(l.link, 4);
        assert_eq!(l.unknown, 1);
        assert_eq!(l.routes, 5);
        assert_ne!(l.digest, 0);
        assert!(l.conserved());
    }

    #[test]
    fn expected_codes_match_table() {
        let mut i = 0usize;
        while i < ROWS {
            assert_eq!(TABLE[i].code, EXPECTED_CODES[i]);
            i += 1;
        }
        assert_eq!(EXPECTED_CODES[0], 0x73181);
        assert_eq!(EXPECTED_CODES[13], 0x7318E);
        assert_eq!(EXPECTED_CODES[14], 0x7318F);
    }

    #[test]
    fn table_names_are_unique_and_lexical() {
        let mut i = 0usize;
        while i < ROWS {
            assert!(name_syntax_ok(TABLE[i].name), "row {} name {}", i, TABLE[i].name);
            assert!(cn_len_ok(TABLE[i].cn), "row {} cn {}", i, TABLE[i].cn);
            let mut j = i + 1;
            while j < ROWS {
                assert_ne!(TABLE[i].name, TABLE[j].name);
                assert_ne!(TABLE[i].cn, TABLE[j].cn);
                j += 1;
            }
            i += 1;
        }
    }

    #[test]
    fn name_syntax_rejects_shapes() {
        assert!(name_syntax_ok("FOCUS_LOST_PRIMARY"));
        assert!(name_syntax_ok("LINK_SILENT"));
        assert!(!name_syntax_ok("focus_lost"));
        assert!(!name_syntax_ok("FOCUS LOST"));
        assert!(!name_syntax_ok("FOCUS__LOST"));
        assert!(!name_syntax_ok("_FOCUS"));
        assert!(!name_syntax_ok("FOCUS_"));
        assert!(!name_syntax_ok(""));
        assert!(!name_syntax_ok("AB"));
        assert!(!name_syntax_ok("FOCUS-LOST"));
    }

    #[test]
    fn class_partition_is_declared() {
        assert_eq!(class_count(EcClass::FocusLost), 5);
        assert_eq!(class_count(EcClass::Constraint), 5);
        assert_eq!(class_count(EcClass::LinkDown), 4);
        assert_eq!(class_count(EcClass::Unclassified), 1);
        assert_eq!(EcClass::FocusLost.declared(), 5);
        assert_eq!(EcClass::Unclassified.declared(), 1);
    }

    #[test]
    fn class_lookup_is_total() {
        assert_eq!(class_of(0x73181), EcClass::FocusLost);
        assert_eq!(class_of(0x73186), EcClass::Constraint);
        assert_eq!(class_of(0x7318B), EcClass::LinkDown);
        assert_eq!(class_of(0x7318F), EcClass::Unclassified);
        assert_eq!(class_of(0), EcClass::Unclassified);
        assert_eq!(class_of(0x73180), EcClass::Unclassified);
    }

    #[test]
    fn route_lookup_is_total() {
        assert_eq!(route_of(0x73181), Route::EventLayer);
        assert_eq!(route_of(0x73186), Route::ConstraintLayer);
        assert_eq!(route_of(0x7318B), Route::VxwmBridge);
        assert_eq!(route_of(0x73185), Route::UpperApi);
        assert_eq!(route_of(0x7318F), Route::Nobody);
        assert_eq!(route_of(0x7FFF), Route::Nobody);
    }

    #[test]
    fn severity_lookup() {
        assert_eq!(severity_of(0x73181), 3);
        assert_eq!(severity_of(0x7318A), 1);
        assert_eq!(severity_of(0x7318F), 0);
        assert_eq!(severity_of(0x9999), 0);
    }

    #[test]
    fn class_digests_are_distinct() {
        let f = class_digest(EcClass::FocusLost);
        let c = class_digest(EcClass::Constraint);
        let l = class_digest(EcClass::LinkDown);
        assert_ne!(f, 0);
        assert_ne!(f, c);
        assert_ne!(c, l);
        assert_ne!(f, l);
    }

    #[test]
    fn class_rows_are_ordered() {
        let mut i = 0u32;
        while i < FOCUS_ROWS {
            let r = class_row(EcClass::FocusLost, i).unwrap();
            assert_eq!(r.code, SEG_BASE + 0x01 + i);
            i += 1;
        }
        assert_eq!(class_row(EcClass::FocusLost, FOCUS_ROWS), None);
        assert_eq!(class_row(EcClass::LinkDown, 0).unwrap().code, 0x7318B);
    }

    #[test]
    fn class_steps_whitelist() {
        assert!(class_step_ok(EcClass::FocusLost, EcClass::Constraint));
        assert!(class_step_ok(EcClass::Constraint, EcClass::LinkDown));
        assert!(class_step_ok(EcClass::LinkDown, EcClass::FocusLost));
        assert!(!class_step_ok(EcClass::LinkDown, EcClass::Constraint));
        assert!(!class_step_ok(EcClass::Unclassified, EcClass::FocusLost));
    }

    #[test]
    fn lane_focus_is_green() {
        let r = lane_focus().unwrap();
        assert_eq!(r.lane, 0);
        assert!(r.green());
        assert_eq!(r.red_count(), 0);
        assert_ne!(r.digest, 0);
    }

    #[test]
    fn lane_constraint_is_green() {
        let r = lane_constraint().unwrap();
        assert_eq!(r.lane, 1);
        assert!(r.green());
        assert_ne!(r.digest, 0);
    }

    #[test]
    fn lane_link_is_green() {
        let r = lane_link().unwrap();
        assert_eq!(r.lane, 2);
        assert!(r.green());
        assert_ne!(r.digest, 0);
    }

    #[test]
    fn lane_timing_is_green() {
        let r = lane_timing().unwrap();
        assert_eq!(r.lane, 3);
        assert_eq!(r.cells, [true, true, true, true, true]);
        assert!(r.green());
        assert_ne!(r.digest, 0);
    }

    #[test]
    fn lane_render_is_green() {
        let r = lane_render().unwrap();
        assert_eq!(r.lane, 4);
        assert!(r.green());
        assert_ne!(r.digest, 0);
    }

    #[test]
    fn run_lane_rejects_out_of_range() {
        assert_eq!(run_lane(5).err(), Some(AUDIT_LANE));
        assert_eq!(run_lane(255).err(), Some(AUDIT_LANE));
        assert!(run_lane(0).is_ok());
    }

    #[test]
    fn matrix_is_all_green() {
        let m = roll_once().unwrap();
        assert!(m.all_green());
        assert!(m.overall_green());
        assert_eq!(m.red, 0);
        assert_ne!(m.digest, 0);
        assert!(m.lane_green(0));
        assert!(m.lane_green(4));
    }

    #[test]
    fn matrix_lanes_are_distinct() {
        let m = roll_once().unwrap();
        assert!(lanes_distinct(&m));
        let mut i = 0usize;
        while i < LANES {
            let mut j = i + 1;
            while j < LANES {
                assert_ne!(m.lane_digest[i], m.lane_digest[j], "lane {} vs {}", i, j);
                j += 1;
            }
            i += 1;
        }
    }

    #[test]
    fn matrix_overall_red_if_any_lane_red() {
        let mut m = roll_once().unwrap();
        m.cells[2][3] = false;
        m.red = 1;
        assert!(!m.overall_green());
        assert!(!m.all_green());
        assert!(!m.lane_green(2));
        assert!(m.lane_green(0));
    }

    #[test]
    fn matrix_replay_is_identical() {
        let (a, b) = roll_replay().unwrap();
        assert_eq!(a.digest, b.digest);
        assert_eq!(a.cells, b.cells);
        assert_eq!(a.lane_digest, b.lane_digest);
    }

    #[test]
    fn table_faults_are_all_rejected() {
        let (r, a) = table_faults().unwrap();
        assert_eq!(r, FAULT_INJECT);
        assert_eq!(a, 0);
    }

    #[test]
    fn audit_row_accepts_good_rows() {
        let mut i = 0usize;
        while i < ROWS {
            assert_eq!(audit_row(&TABLE[i], i), Ok(()));
            i += 1;
        }
        assert_eq!(audit_row(&TABLE[0], 5).err(), Some(AUDIT_SEGMENT));
    }

    #[test]
    fn row_lookup_by_name() {
        let r = row_by_name("CONSTRAINT_PIN").unwrap();
        assert_eq!(r.code, 0x73186);
        assert_eq!(r.class, EcClass::Constraint);
        assert_eq!(row_by_name("NOPE_NOPE"), None);
        assert_eq!(row_of(0x7318B).unwrap().class, EcClass::LinkDown);
    }

    #[test]
    fn cn_len_uses_chars_not_bytes() {
        assert!(cn_len_ok("约束冲突：堆叠次序成环"));
        assert!(!cn_len_ok("焦点丢失"));
        assert!(!cn_len_ok(""));
        // 8 个汉字正好达线。
        assert!(cn_len_ok("八个汉字刚好达线"));
    }

    #[test]
    fn class_enum_roundtrip() {
        let mut i = 0u8;
        while i < 4 {
            let c = EcClass::from_index(i).unwrap();
            assert_eq!(c.index(), i);
            assert!(!c.name().is_empty());
            i += 1;
        }
        assert_eq!(EcClass::from_index(4), None);
        assert_eq!(Route::from_index(4), Some(Route::Nobody));
        assert_eq!(Route::from_index(5), None);
    }

    #[test]
    fn render_roundtrip() {
        let row = render_row(TABLE_TAG, 15, 5, 5, 4, 0x0123_4567_89AB_CDEF);
        assert!(audit_render(&row));
        let (t, v, d) = row_fields(&row).unwrap();
        assert_eq!(t, TABLE_TAG);
        assert_eq!(v, [15, 5, 5, 4]);
        assert_eq!(d, 0x0123_4567_89AB_CDEF);
        assert!(!audit_render(""));
        assert!(!audit_render("ETAB|k1=15"));
    }

    #[test]
    fn render_rejects_key_drift() {
        assert!(row_fields("ETAB|a=1|k2=2|k3=3|k4=4|digest=0123456789ABCDEF").is_none());
        assert!(row_fields("ETAB|k1=1|k2=2|k3=3|k4=4|dig=0123456789ABCDEF").is_none());
    }

    #[test]
    fn parse_helpers_behave() {
        assert_eq!(parse_u32("15"), Some(15));
        assert_eq!(parse_u32(""), None);
        assert_eq!(parse_u32("1e3"), None);
        assert_eq!(parse_hex16("FFFFFFFFFFFFFFFF"), Some(u64::MAX));
        assert_eq!(parse_hex16("0x1234"), None);
    }

    #[test]
    fn ledger_rows_parse() {
        let l = audit_table().unwrap();
        let row = table_row(&l);
        let (t, v, d) = row_fields(&row).unwrap();
        assert_eq!(t, TABLE_TAG);
        assert_eq!(v, [15, 5, 5, 4]);
        assert_eq!(d, l.digest);
        let cl = class_row_line();
        assert!(audit_render(&cl));
        let m = roll_once().unwrap();
        let rr = roll_row(&m);
        let (t2, v2, d2) = row_fields(&rr).unwrap();
        assert_eq!(t2, ROLL_TAG);
        assert_eq!(v2[0], 5);
        assert_eq!(v2[1], 5);
        assert_eq!(v2[2], 0);
        assert_eq!(d2, m.digest);
    }

    #[test]
    fn case_ids_tile_and_do_not_collide() {
        assert!(case_ids_tile());
        assert!(no_cross_collision());
        assert_eq!(case_id(0), Some(0x0734_0401));
        assert_eq!(case_id(15), Some(0x0734_0410));
        assert_eq!(case_id(16), None);
    }

    #[test]
    fn probes_are_green() {
        assert!(probe_seq_a().is_ok());
        assert!(probe_seq_b().is_ok());
        assert!(probe_seq_c().is_ok());
    }

    #[test]
    fn repro_is_deterministic() {
        assert!(repro_check().is_ok());
    }

    #[test]
    fn checks_are_eight_and_green() {
        let items = extra_checks();
        assert_eq!(items.len(), 8);
        let mut i = 0usize;
        while i < items.len() {
            assert!(items[i].1, "red check: {} {}", items[i].0, items[i].2);
            i += 1;
        }
        let s = run_unx49053_checks();
        assert!(s.all_passed(), "red={}", s.red_items_text());
        assert!(!s.truncated());
        assert_eq!(s.len(), 8);
    }

    #[test]
    fn audit_codes_are_private_and_described() {
        // 审计码位于 +0x80 之后的私有区间，绝不占用族内 14 个槽位。
        assert_eq!(AUDIT_BASE, SEG_BASE + 0x80);
        assert!(AUDIT_SEGMENT > SEG_BASE + 0x0F);
        let codes = [
            AUDIT_SEGMENT,
            AUDIT_NAME,
            AUDIT_CLASS,
            AUDIT_ROUTE,
            AUDIT_TABLE,
            AUDIT_LANE,
            AUDIT_ROLL,
            AUDIT_RENDER,
        ];
        let mut i = 0usize;
        while i < codes.len() {
            assert!(!known_code(codes[i]));
            assert_ne!(audit_detail(codes[i]), UNKNOWN);
            let mut j = i + 1;
            while j < codes.len() {
                assert_ne!(codes[i], codes[j]);
                assert_ne!(audit_detail(codes[i]), audit_detail(codes[j]));
                j += 1;
            }
            i += 1;
        }
        assert_eq!(audit_detail(SEG_BASE + 0x01), UNKNOWN);
        assert_eq!(audit_detail(0), UNKNOWN);
    }

    #[test]
    fn reserved_ids_are_the_six_b12_prior_slots() {
        assert_eq!(
            RESERVED_IDS,
            [
                0x071C_8001,
                0x071C_8002,
                0x071C_8014,
                0x071D_0001,
                0x071D_2001,
                0x071D_2014
            ]
        );
        let mut i = 0usize;
        while i < RESERVED_IDS.len() {
            let mut j = i + 1;
            while j < RESERVED_IDS.len() {
                assert_ne!(RESERVED_IDS[i], RESERVED_IDS[j]);
                j += 1;
            }
            i += 1;
        }
    }

    #[test]
    fn case_id_ordinals_follow_case_base() {
        let mut i = 0usize;
        while i < CASE_IDS.len() {
            assert_eq!(case_id(i), Some(CASE_BASE + 1 + i as u32));
            assert!(CASE_IDS[i] >= 0x0734_0401 && CASE_IDS[i] <= 0x0734_0410);
            i += 1;
        }
        assert_eq!(case_id(CASE_IDS.len()), None);
        assert_eq!(case_id(usize::MAX), None);
    }

    #[test]
    fn table_digest_is_stable_across_calls() {
        let a = audit_table().unwrap();
        let b = audit_table().unwrap();
        assert_eq!(a, b);
        assert_ne!(a.digest, 0);
        // 分类摘要必须与整表摘要分离：整表再折一次也会变，不能拿同一个数冒充两类账。
        let f = class_digest(EcClass::FocusLost);
        let c = class_digest(EcClass::Constraint);
        let l = class_digest(EcClass::LinkDown);
        assert_ne!(f, c);
        assert_ne!(c, l);
        assert_ne!(f, l);
        assert_ne!(f, a.digest);
    }
}
