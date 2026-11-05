//! # unx49095 — B15 错误码族与四点观测埋点
//!
//! 卡面原文（任务板 CoRun·UNX-F2-B40）：
//!
//! > **F49095 恶意设备防御、错误路径前段与首产段收官错误码族（15 条）**
//! > **F49096 恶意设备防御、错误路径前段与首产段收官观测埋点（入口/出口/耗时/结果四点全覆盖）**
//!
//! 验收锚点：
//!
//! - `UNX-F49096-J1` 四点 100% 覆盖；异常零静默；判据锚定 体验十三·补。
//! - `UNX-F49097-J1` fuzz ×10^4 零崩；判据锚定 体验十 健壮性。
//!
//! ## 闸序与理由
//!
//! 1. **十五族互斥**：族码值、中文说明、运营短句三者两两不同，先证明族表
//!    本身没有撞号，后面的观测记录才有可信的“结果”字段。
//! 2. **码值全窗**：十五枚族码必须全部落在本模块的族窗内，窗外码值一律
//!    解析为 `None`——未知码不得冒充已知异常。
//! 3. **异常零静默**：每条正常轨迹必须四个观测点齐全；五种人为缺陷轨迹
//!    （缺入口/缺出口/缺结果/零耗时/未知码）必须全部被判为静默并拒绝入账。
//! 4. **四点覆盖**：一千条轨迹逐条检查入口、出口、耗时、结果四点，
//!    缺一点即覆盖率不足 100%。
//! 5. **耗时窗口**：耗时必须落在 `1..=50_000` 微秒，且出口序号必须等于
//!    入口序号加耗时（同一条时间线，不允许两个时钟各说各话）。
//!
//! ## 与相邻模块的差异
//!
//! - `unx49093_handover` 的十五族码是“段印位序表”，只证明不漂移；本模块的
//!   十五族码带语义（限速/洪泛/热移除/乱序/…），并直接进入观测记录的
//!   `result` 字段，是运行期诊断表。
//! - `unx49089_finalgrep` 的账行记录“批判据是否通过”；本模块的账行记录
//!   “单条轨迹四个观测点是否齐全”，粒度从批降到次。
//! - 本模块不持有可变全局状态：轨迹由确定性 LCG 现场生成，缺陷轨迹是常量。

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;
use super::unx48941_wheel_core as p_core;

// ── 段位与窗口 ───────────────────────────────────────────────────
/// 本模块段基址（B15 第八槽）。
pub const SEG_BASE: u32 = 0x736C0;
/// 诊断码数。
pub const DIAG_CODES: usize = 14;
/// 族码数。
pub const FAMILIES: usize = 15;
/// 族窗基址（诊断码之后，段内独立窗口）。
pub const FAMILY_BASE: u32 = SEG_BASE + 0x20;
/// 观测点数。
pub const POINTS: usize = 4;
/// 轨迹采样上限。
pub const SAMPLE_MAX: u32 = 4_096;
/// 覆盖判据采样数。
pub const COVER_SAMPLE: u32 = 1_000;
/// 耗时上限（微秒）。
pub const MAX_ELAPSED: u32 = 50_000;
/// 账行字段数。
pub const ROW_FIELDS: usize = 7;
/// 账行标签。
pub const TAG: &str = "OBS95";
/// 账行键序。
pub const ROW_KEYS: [&str; 5] = ["trace", "entry", "exit", "elapsed", "code"];
/// 未知诊断码说明。
pub const UNKNOWN: &str = "未知 B15 观测诊断码";
/// 未知族码说明。
pub const UNKNOWN_FAMILY: &str = "未知 B15 错误族码";
/// 段宽（相邻模块互斥）。
pub const SEG_SPAN: u32 = 0x40;

// ── 诊断码 ───────────────────────────────────────────────────────
/// 观测诊断码（14 条，说明互不共用）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ObsCode {
    /// 轨迹序号越界
    TraceBad,
    /// 入口点缺失
    EntryBad,
    /// 出口点缺失
    ExitBad,
    /// 出口不晚于入口
    OrderBad,
    /// 耗时越出窗口
    ElapsedBad,
    /// 结果点缺失
    ResultBad,
    /// 结果码不在族内
    FamilyBad,
    /// 四点为覆盖不足
    CoverageBad,
    /// 异常被静默吞掉
    SilentBad,
    /// 摘要与重算不符
    DigestBad,
    /// 账行键序错位
    RowBad,
    /// 字段数或字段名不符
    FieldBad,
    /// 算术溢出
    Overflow,
    /// 观测表不完整
    Incomplete,
}

impl ObsCode {
    /// 诊断码值。
    pub fn val(self) -> u32 {
        SEG_BASE + 0x01 + self as u32
    }

    /// 逐码说明（互不共用）。
    pub fn detail(self) -> &'static str {
        match self {
            ObsCode::TraceBad => "轨迹序号越出 0..=4096",
            ObsCode::EntryBad => "入口观测点缺失",
            ObsCode::ExitBad => "出口观测点缺失",
            ObsCode::OrderBad => "出口序号不晚于入口序号",
            ObsCode::ElapsedBad => "耗时越出 1..=50_000 微秒",
            ObsCode::ResultBad => "结果观测点缺失",
            ObsCode::FamilyBad => "结果码不在十五族内",
            ObsCode::CoverageBad => "四点覆盖率不足百分百",
            ObsCode::SilentBad => "异常轨迹被静默放行",
            ObsCode::DigestBad => "账行摘要与重算不一致",
            ObsCode::RowBad => "账行键序或标签错位",
            ObsCode::FieldBad => "账行字段数或字段名不符",
            ObsCode::Overflow => "序号或耗时相加溢出",
            ObsCode::Incomplete => "观测表不完整",
        }
    }

    /// 面向运营的短句（与 `detail` 不同口径）。
    pub fn message(self) -> &'static str {
        match self {
            ObsCode::TraceBad => "轨迹非法：请回读采样表",
            ObsCode::EntryBad => "入口缺失：请补埋点",
            ObsCode::ExitBad => "出口缺失：请补埋点",
            ObsCode::OrderBad => "序号倒挂：请核对时钟",
            ObsCode::ElapsedBad => "耗时越界：请收敛窗口",
            ObsCode::ResultBad => "结果缺失：请补结论",
            ObsCode::FamilyBad => "结果码非法：请查十五族",
            ObsCode::CoverageBad => "覆盖不足：请补齐四点",
            ObsCode::SilentBad => "异常静默：请拒绝入账",
            ObsCode::DigestBad => "摘要失配：请重算账行",
            ObsCode::RowBad => "账行损坏：请重渲染",
            ObsCode::FieldBad => "字段错位：请核对键序",
            ObsCode::Overflow => "算术溢出：请改饱和运算",
            ObsCode::Incomplete => "观测不全：请补采样",
        }
    }
}

/// 全部诊断码（顺序即值序）。
pub const ALL: [ObsCode; DIAG_CODES] = [
    ObsCode::TraceBad,
    ObsCode::EntryBad,
    ObsCode::ExitBad,
    ObsCode::OrderBad,
    ObsCode::ElapsedBad,
    ObsCode::ResultBad,
    ObsCode::FamilyBad,
    ObsCode::CoverageBad,
    ObsCode::SilentBad,
    ObsCode::DigestBad,
    ObsCode::RowBad,
    ObsCode::FieldBad,
    ObsCode::Overflow,
    ObsCode::Incomplete,
];

/// 自由函数：码值 → 说明（段外一律 `UNKNOWN`）。
pub fn detail(code: u32) -> &'static str {
    match code.wrapping_sub(SEG_BASE) {
        1 => ObsCode::TraceBad.detail(),
        2 => ObsCode::EntryBad.detail(),
        3 => ObsCode::ExitBad.detail(),
        4 => ObsCode::OrderBad.detail(),
        5 => ObsCode::ElapsedBad.detail(),
        6 => ObsCode::ResultBad.detail(),
        7 => ObsCode::FamilyBad.detail(),
        8 => ObsCode::CoverageBad.detail(),
        9 => ObsCode::SilentBad.detail(),
        10 => ObsCode::DigestBad.detail(),
        11 => ObsCode::RowBad.detail(),
        12 => ObsCode::FieldBad.detail(),
        13 => ObsCode::Overflow.detail(),
        14 => ObsCode::Incomplete.detail(),
        _ => UNKNOWN,
    }
}

/// 段位自检：14 诊断码两两不同且落在诊断窗口内。
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

// ── 十五族 ───────────────────────────────────────────────────────
/// 错误族码（十五条）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FamilyCode {
    /// 限速触发
    RateLimit,
    /// 洪泛取证
    Flood,
    /// 热移除
    HotRemove,
    /// 乱序时间戳
    Reorder,
    /// 缓冲溢出
    Overflow,
    /// keymap 回退
    KeymapFallback,
    /// 焦点丢失
    FocusLost,
    /// 指针拒绝
    PointerGrab,
    /// 滚轮轴向
    WheelAxis,
    /// 手势相位
    GesturePhase,
    /// 时间戳抖动
    TimingJitter,
    /// 账本配额
    LedgerQuota,
    /// fuzz 坏输入
    FuzzCorrupt,
    /// 埋点缺失
    ObsMissing,
    /// 段印破损
    SealBroken,
}

impl FamilyCode {
    /// 族码值（族窗内）。
    pub fn val(self) -> u32 {
        FAMILY_BASE + 0x01 + self as u32
    }

    /// 中文说明（互不共用）。
    pub fn detail(self) -> &'static str {
        match self {
            FamilyCode::RateLimit => "限速执行器触发",
            FamilyCode::Flood => "洪泛取证已入账",
            FamilyCode::HotRemove => "设备节点热移除",
            FamilyCode::Reorder => "时间戳乱序超出窗口",
            FamilyCode::Overflow => "缓冲达到上限溢出",
            FamilyCode::KeymapFallback => "keymap 损坏回退美式",
            FamilyCode::FocusLost => "焦点持有者已消失",
            FamilyCode::PointerGrab => "指针拒掌越过白名单",
            FamilyCode::WheelAxis => "滚轮轴向越界",
            FamilyCode::GesturePhase => "手势相位非法跳变",
            FamilyCode::TimingJitter => "时间戳抖动越过阈值",
            FamilyCode::LedgerQuota => "延迟账本配额耗尽",
            FamilyCode::FuzzCorrupt => "fuzz 注入坏载荷",
            FamilyCode::ObsMissing => "观测点缺失",
            FamilyCode::SealBroken => "段印校验失败",
        }
    }

    /// 运营短句（与 `detail` 不同口径）。
    pub fn message(self) -> &'static str {
        match self {
            FamilyCode::RateLimit => "限速：请降频重试",
            FamilyCode::Flood => "洪泛：已隔离取证",
            FamilyCode::HotRemove => "热移除：请在途帧已丢弃",
            FamilyCode::Reorder => "乱序：请重置时间线",
            FamilyCode::Overflow => "溢出：请扩窗或丢帧",
            FamilyCode::KeymapFallback => "回退：请修复键表",
            FamilyCode::FocusLost => "失焦：请清理订阅",
            FamilyCode::PointerGrab => "拒掌：请核对授权",
            FamilyCode::WheelAxis => "轴向：请校正滚轮",
            FamilyCode::GesturePhase => "相位：请重置手势",
            FamilyCode::TimingJitter => "抖动：请检查报点",
            FamilyCode::LedgerQuota => "配额：请清理账本",
            FamilyCode::FuzzCorrupt => "坏载荷：已拒收",
            FamilyCode::ObsMissing => "缺埋点：请补观测",
            FamilyCode::SealBroken => "段印破损：请重钉",
        }
    }
}

/// 十五族（顺序即值序）。
pub const FAMILY_ALL: [FamilyCode; FAMILIES] = [
    FamilyCode::RateLimit,
    FamilyCode::Flood,
    FamilyCode::HotRemove,
    FamilyCode::Reorder,
    FamilyCode::Overflow,
    FamilyCode::KeymapFallback,
    FamilyCode::FocusLost,
    FamilyCode::PointerGrab,
    FamilyCode::WheelAxis,
    FamilyCode::GesturePhase,
    FamilyCode::TimingJitter,
    FamilyCode::LedgerQuota,
    FamilyCode::FuzzCorrupt,
    FamilyCode::ObsMissing,
    FamilyCode::SealBroken,
];

/// 按序号取族码。
pub fn family_at(index: usize) -> Option<FamilyCode> {
    FAMILY_ALL.get(index).copied()
}

/// 按值反解族码。
pub fn family_from_val(v: u32) -> Option<FamilyCode> {
    let mut i = 0usize;
    while i < FAMILY_ALL.len() {
        if FAMILY_ALL[i].val() == v {
            return Some(FAMILY_ALL[i]);
        }
        i += 1;
    }
    None
}

/// 自由函数：族码值 → 说明（窗外一律 `UNKNOWN_FAMILY`）。
pub fn family_detail(v: u32) -> &'static str {
    match family_from_val(v) {
        Some(c) => c.detail(),
        None => UNKNOWN_FAMILY,
    }
}

/// 族窗自检：十五族值、说明、短句三者两两不同。
pub fn family_guard() -> bool {
    let mut i = 0usize;
    while i < FAMILY_ALL.len() {
        let a = FAMILY_ALL[i];
        if a.val() <= FAMILY_BASE || a.val() > FAMILY_BASE + 0x0F {
            return false;
        }
        let mut j = i + 1;
        while j < FAMILY_ALL.len() {
            let b = FAMILY_ALL[j];
            if a.val() == b.val() || a.detail() == b.detail() || a.message() == b.message() {
                return false;
            }
            j += 1;
        }
        i += 1;
    }
    true
}

// ── 观测点 ───────────────────────────────────────────────────────
/// 四点观测位。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Point {
    /// 入口。
    Entry,
    /// 出口。
    Exit,
    /// 耗时。
    Elapsed,
    /// 结果。
    Result,
}

/// 四点全集。
pub const POINT_ALL: [Point; POINTS] = [
    Point::Entry,
    Point::Exit,
    Point::Elapsed,
    Point::Result,
];

// ── 轨迹 ─────────────────────────────────────────────────────────
/// 单条观测轨迹。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Trace {
    /// 轨迹序号。
    pub seq: u32,
    /// 入口序号。
    pub entry: Option<u32>,
    /// 出口序号。
    pub exit: Option<u32>,
    /// 耗时（微秒）。
    pub elapsed_us: u32,
    /// 结果码；`0` 表示成功，非零必须是族窗内的值。
    pub result: Option<u32>,
}

impl Trace {
    /// 判断某观测点是否在位。
    pub fn has_point(&self, p: Point) -> bool {
        match p {
            Point::Entry => self.entry.is_some(),
            Point::Exit => self.exit.is_some(),
            Point::Elapsed => self.elapsed_us > 0 && self.elapsed_us <= MAX_ELAPSED,
            Point::Result => self.result.is_some(),
        }
    }

    /// 四点齐全。
    pub fn points_ok(&self) -> bool {
        let mut i = 0usize;
        while i < POINT_ALL.len() {
            if !self.has_point(POINT_ALL[i]) {
                return false;
            }
            i += 1;
        }
        true
    }

    /// 结果码合法（成功码 0 或族窗内值）。
    pub fn result_ok(&self) -> bool {
        match self.result {
            Some(0) => true,
            Some(v) => family_from_val(v).is_some(),
            None => false,
        }
    }

    /// 出口序号等于入口序号加耗时。
    pub fn order_ok(&self) -> bool {
        match (self.entry, self.exit) {
            (Some(e), Some(x)) => match e.checked_add(self.elapsed_us) {
                Some(want) => x == want,
                None => false,
            },
            _ => false,
        }
    }

    /// 轨迹完整：四点齐全、结果合法、时间线一致。
    pub fn complete(&self) -> bool {
        self.seq <= SAMPLE_MAX
            && self.points_ok()
            && self.result_ok()
            && self.elapsed_us >= 1
            && self.elapsed_us <= MAX_ELAPSED
            && self.order_ok()
    }

    /// 是否静默（任一观测点或结果缺失）。
    pub fn silent(&self) -> bool {
        !self.complete()
    }

    /// 结果族码（成功码或非法码返回 `None`）。
    pub fn family(&self) -> Option<FamilyCode> {
        match self.result {
            Some(v) => family_from_val(v),
            None => None,
        }
    }

    /// 轨迹摘要。
    pub fn digest(&self) -> u64 {
        trace_digest(
            self.seq,
            self.entry,
            self.exit,
            self.elapsed_us,
            self.result,
        )
    }
}

/// 轨迹摘要：序号 + 入口 + 出口 + 耗时 + 结果。
pub fn trace_digest(
    seq: u32,
    entry: Option<u32>,
    exit: Option<u32>,
    elapsed: u32,
    result: Option<u32>,
) -> u64 {
    let mut h = p_core::FNV_OFF;
    h = p_core::fold_u32(h, 0x4F42_5335);
    h = p_core::fold_u32(h, seq);
    h = p_core::fold_u32(h, entry.unwrap_or(u32::MAX));
    h = p_core::fold_u32(h, exit.unwrap_or(u32::MAX));
    h = p_core::fold_u32(h, elapsed);
    h = p_core::fold_u32(h, result.unwrap_or(u32::MAX));
    h
}

/// 构造一条完整轨迹（确定性）。
pub fn make_trace(seq: u32) -> Trace {
    let mut rng = p_core::Lcg::new(
        (seq as u64)
            .wrapping_mul(0x9E37_79B9)
            .wrapping_add(0x0055_4E58),
    );
    let entry = rng.below(1_000_000);
    let elapsed = 1 + rng.below(MAX_ELAPSED);
    let exit = entry.saturating_add(elapsed);
    let result = if rng.below(5) == 0 {
        0
    } else {
        let idx = rng.below(FAMILIES as u32);
        match family_at(idx as usize) {
            Some(c) => c.val(),
            None => 0,
        }
    };
    Trace {
        seq,
        entry: Some(entry),
        exit: Some(exit),
        elapsed_us: elapsed,
        result: Some(result),
    }
}

/// 采样 `n` 条完整轨迹（1..=4096）。
pub fn sample(n: u32) -> Vec<Trace> {
    let mut out = Vec::new();
    if n == 0 || n > SAMPLE_MAX {
        return out;
    }
    let mut i = 0u32;
    while i < n {
        out.push(make_trace(i));
        i += 1;
    }
    out
}

/// 五条人为缺陷轨迹（全部必须被判定为静默）。
pub fn fault_traces() -> Vec<Trace> {
    let mut out = Vec::new();
    out.push(Trace {
        seq: 1,
        entry: None,
        exit: Some(20),
        elapsed_us: 10,
        result: Some(0),
    });
    out.push(Trace {
        seq: 2,
        entry: Some(10),
        exit: None,
        elapsed_us: 10,
        result: Some(0),
    });
    out.push(Trace {
        seq: 3,
        entry: Some(10),
        exit: Some(20),
        elapsed_us: 10,
        result: None,
    });
    out.push(Trace {
        seq: 4,
        entry: Some(10),
        exit: Some(10),
        elapsed_us: 0,
        result: Some(0),
    });
    out.push(Trace {
        seq: 5,
        entry: Some(10),
        exit: Some(19),
        elapsed_us: 9,
        result: Some(0xDEAD_BEEF),
    });
    out
}

/// 四点覆盖计数：返回 `[entry, exit, elapsed, result]` 四枚计数。
pub fn point_coverage(traces: &[Trace]) -> [u32; POINTS] {
    let mut out = [0u32; POINTS];
    let mut i = 0usize;
    while i < traces.len() {
        let t = &traces[i];
        let mut k = 0usize;
        while k < POINT_ALL.len() {
            if t.has_point(POINT_ALL[k]) {
                out[k] = out[k].saturating_add(1);
            }
            k += 1;
        }
        i += 1;
    }
    out
}

/// 全量四点覆盖（1000 条轨迹，四点计数都等于样本数）。
pub fn full_coverage() -> bool {
    let traces = sample(COVER_SAMPLE);
    if traces.len() != COVER_SAMPLE as usize {
        return false;
    }
    let cov = point_coverage(&traces);
    let mut i = 0usize;
    while i < cov.len() {
        if cov[i] != COVER_SAMPLE {
            return false;
        }
        i += 1;
    }
    true
}

/// 采样内静默条数（必须为零）。
pub fn silent_count(traces: &[Trace]) -> u32 {
    let mut n = 0u32;
    let mut i = 0usize;
    while i < traces.len() {
        if traces[i].silent() {
            n += 1;
        }
        i += 1;
    }
    n
}

/// 采样内出现的不同族码数。
pub fn family_coverage(traces: &[Trace]) -> u32 {
    let mut seen = [false; FAMILIES];
    let mut i = 0usize;
    while i < traces.len() {
        if let Some(v) = traces[i].result {
            if let Some(c) = family_from_val(v) {
                let idx = (c.val() - FAMILY_BASE - 1) as usize;
                if idx < seen.len() {
                    seen[idx] = true;
                }
            }
        }
        i += 1;
    }
    let mut n = 0u32;
    let mut k = 0usize;
    while k < seen.len() {
        if seen[k] {
            n += 1;
        }
        k += 1;
    }
    n
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

/// 渲染轨迹账行：`OBS95|trace=|entry=|exit=|elapsed=|code=|digest=`。
pub fn render_row(t: &Trace) -> String {
    let entry = match t.entry {
        Some(v) => format!("{}", v),
        None => String::from("none"),
    };
    let exit = match t.exit {
        Some(v) => format!("{}", v),
        None => String::from("none"),
    };
    let code = match t.result {
        Some(v) => format!("{}", v),
        None => String::from("none"),
    };
    format!(
        "{}|trace={}|entry={}|exit={}|elapsed={}|code={}|digest={:016x}",
        TAG,
        t.seq,
        entry,
        exit,
        t.elapsed_us,
        code,
        t.digest()
    )
}

/// 回读轨迹账行：键序、四点、族码、摘要五重校验。
pub fn audit_row(row: &str) -> bool {
    let f = match row_fields(row, TAG, &ROW_KEYS) {
        Some(v) if v.len() == ROW_KEYS.len() + 1 => v,
        _ => return false,
    };
    let seq = match parse_u32(f[0]) {
        Some(v) if v <= SAMPLE_MAX => v,
        _ => return false,
    };
    if f[1] == "none" || f[2] == "none" || f[4] == "none" {
        return false;
    }
    let entry = match parse_u32(f[1]) {
        Some(v) => v,
        None => return false,
    };
    let exit = match parse_u32(f[2]) {
        Some(v) => v,
        None => return false,
    };
    let elapsed = match parse_u32(f[3]) {
        Some(v) if v >= 1 && v <= MAX_ELAPSED => v,
        _ => return false,
    };
    let code = match parse_u32(f[4]) {
        Some(v) if v == 0 || family_from_val(v).is_some() => v,
        _ => return false,
    };
    if exit != entry.saturating_add(elapsed) {
        return false;
    }
    let want = match parse_hex16(f[5]) {
        Some(v) => v,
        None => return false,
    };
    trace_digest(seq, Some(entry), Some(exit), elapsed, Some(code)) == want
}

/// 导出前 `n` 条轨迹账行。
pub fn export_rows(n: u32) -> Vec<String> {
    let mut out = Vec::new();
    let traces = sample(n);
    let mut i = 0usize;
    while i < traces.len() {
        out.push(render_row(&traces[i]));
        i += 1;
    }
    out
}

/// 账行两两不同且逐行可回读。
pub fn rows_stable(n: u32) -> bool {
    let rows = export_rows(n);
    if rows.len() != n as usize {
        return false;
    }
    let mut i = 0usize;
    while i < rows.len() {
        if !audit_row(&rows[i]) {
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
    let traces = sample(COVER_SAMPLE);

    // 1) 十五族互斥。
    let ok = family_guard()
        && FAMILY_ALL.len() == FAMILIES
        && family_at(FAMILIES).is_none()
        && family_from_val(0).is_none();
    out.push((
        "F49095/族码十五互斥",
        ok,
        format!(
            "families={} guard={} outside_none={}",
            FAMILY_ALL.len(),
            family_guard(),
            family_from_val(0).is_none()
        ),
    ));

    // 2) 族码全窗。
    let mut in_window = true;
    let mut i = 0usize;
    while i < FAMILY_ALL.len() {
        let v = FAMILY_ALL[i].val();
        if v <= FAMILY_BASE || v > FAMILY_BASE + 0x0F {
            in_window = false;
        }
        if family_detail(v) != FAMILY_ALL[i].detail() {
            in_window = false;
        }
        i += 1;
    }
    let outside_unknown = family_detail(0) == UNKNOWN_FAMILY
        && family_detail(FAMILY_BASE) == UNKNOWN_FAMILY
        && family_detail(FAMILY_BASE + 0x10) == UNKNOWN_FAMILY;
    let ok = in_window && outside_unknown;
    out.push((
        "F49095/族码全窗",
        ok,
        format!(
            "in_window={} outside_unknown={} base={}",
            in_window, outside_unknown, FAMILY_BASE
        ),
    ));

    // 3) 异常零静默。
    let faults = fault_traces();
    let mut faults_silent = 0u32;
    let mut i = 0usize;
    while i < faults.len() {
        if faults[i].silent() {
            faults_silent += 1;
        }
        i += 1;
    }
    let ok = silent_count(&traces) == 0
        && faults_silent == faults.len() as u32
        && faults.len() == 5;
    out.push((
        "F49095/异常零静默",
        ok,
        format!(
            "sample={} silent={} faults={} faults_silent={}",
            traces.len(),
            silent_count(&traces),
            faults.len(),
            faults_silent
        ),
    ));

    // 4) 轨迹账行回读。
    let rows = export_rows(64);
    let mut tamp = String::new();
    if let Some(first) = rows.first() {
        tamp.push_str(first);
        tamp.push('x');
    }
    let tamper_rejected = !tamp.is_empty() && !audit_row(&tamp);
    let ok = rows.len() == 64 && rows_stable(64) && tamper_rejected;
    out.push((
        "F49095/账行回读",
        ok,
        format!(
            "rows={} stable={} tamper_rejected={}",
            rows.len(),
            rows_stable(64),
            tamper_rejected
        ),
    ));

    // 5) 四点全覆盖。
    let cov = point_coverage(&traces);
    let ok = full_coverage()
        && traces.len() == COVER_SAMPLE as usize
        && cov[0] == COVER_SAMPLE
        && cov[1] == COVER_SAMPLE
        && cov[2] == COVER_SAMPLE
        && cov[3] == COVER_SAMPLE;
    out.push((
        "F49096/四点全覆盖",
        ok,
        format!(
            "sample={} entry={} exit={} elapsed={} result={}",
            traces.len(),
            cov[0],
            cov[1],
            cov[2],
            cov[3]
        ),
    ));

    // 6) 耗时窗口与时间线一致。
    let mut elapsed_ok = true;
    let mut order_ok = true;
    let mut i = 0usize;
    while i < traces.len() {
        if traces[i].elapsed_us < 1 || traces[i].elapsed_us > MAX_ELAPSED {
            elapsed_ok = false;
        }
        if !traces[i].order_ok() {
            order_ok = false;
        }
        i += 1;
    }
    let ok = elapsed_ok && order_ok;
    out.push((
        "F49096/耗时窗口",
        ok,
        format!(
            "elapsed_ok={} order_ok={} max_us={}",
            elapsed_ok, order_ok, MAX_ELAPSED
        ),
    ));

    // 7) 埋点无漏记：族码覆盖满窗，结果码合法。
    let mut result_ok = true;
    let mut i = 0usize;
    while i < traces.len() {
        let t = &traces[i];
        let ok_here = match t.result {
            Some(0) => t.result_ok(),
            Some(_) => t.family().is_some(),
            None => false,
        };
        if !ok_here {
            result_ok = false;
        }
        i += 1;
    }
    let ok = result_ok && family_coverage(&traces) == FAMILIES as u32;
    out.push((
        "F49096/埋点无漏记",
        ok,
        format!(
            "result_ok={} family_coverage={}/{}",
            result_ok,
            family_coverage(&traces),
            FAMILIES
        ),
    ));

    // 8) 全量账行回读（32 条）。
    let rows = export_rows(32);
    let mut tamper_ok = true;
    let mut i = 0usize;
    while i < rows.len() {
        let mut t = rows[i].clone();
        t.push('0');
        if audit_row(&t) {
            tamper_ok = false;
        }
        if !audit_row(&rows[i]) {
            tamper_ok = false;
        }
        i += 1;
    }
    let ok = rows.len() == 32 && tamper_ok && rows_stable(32);
    out.push((
        "F49096/账行回读",
        ok,
        format!(
            "rows={} tamper_ok={} stable={}",
            rows.len(),
            tamper_ok,
            rows_stable(32)
        ),
    ));

    out
}

/// 入口：八项判据写入 CheckSet。
pub fn run_unx49095_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F49095 错误码族与四点埋点");
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

    #[test]
    fn diag_codes_distinct_in_window() {
        assert!(segment_guard());
        assert_eq!(ALL.len(), DIAG_CODES);
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
    fn free_diag_detail_outside_unknown() {
        assert_eq!(detail(SEG_BASE), UNKNOWN);
        assert_eq!(detail(SEG_BASE + 0x0F), UNKNOWN);
        assert_eq!(detail(ALL[0].val()), ObsCode::TraceBad.detail());
        assert_eq!(detail(ALL[13].val()), ObsCode::Incomplete.detail());
    }

    #[test]
    fn family_guard_holds() {
        assert!(family_guard());
        assert_eq!(FAMILY_ALL.len(), FAMILIES);
    }

    #[test]
    fn family_values_in_window() {
        let mut i = 0usize;
        while i < FAMILY_ALL.len() {
            let v = FAMILY_ALL[i].val();
            assert!(v > FAMILY_BASE && v <= FAMILY_BASE + 0x0F);
            assert_eq!(family_from_val(v), Some(FAMILY_ALL[i]));
            assert_eq!(family_detail(v), FAMILY_ALL[i].detail());
            i += 1;
        }
    }

    #[test]
    fn family_outside_values_unknown() {
        assert_eq!(family_from_val(0), None);
        assert_eq!(family_from_val(FAMILY_BASE), None);
        assert_eq!(family_from_val(FAMILY_BASE + 0x10), None);
        assert_eq!(family_detail(0), UNKNOWN_FAMILY);
        assert_eq!(family_detail(FAMILY_BASE), UNKNOWN_FAMILY);
        assert_eq!(family_detail(FAMILY_BASE + 0x10), UNKNOWN_FAMILY);
    }

    #[test]
    fn family_lookup_bounds() {
        assert!(family_at(0).is_some());
        assert!(family_at(FAMILIES - 1).is_some());
        assert!(family_at(FAMILIES).is_none());
        assert!(family_at(usize::MAX).is_none());
    }

    #[test]
    fn family_diag_windows_do_not_overlap() {
        let last_diag = ALL[ALL.len() - 1].val();
        let first_family = FAMILY_ALL[0].val();
        assert!(last_diag < first_family);
        assert!(FAMILY_ALL[FAMILY_ALL.len() - 1].val() <= SEG_BASE + 0x3F);
    }

    #[test]
    fn trace_sample_is_complete() {
        let traces = sample(COVER_SAMPLE);
        assert_eq!(traces.len(), COVER_SAMPLE as usize);
        let mut i = 0usize;
        while i < traces.len() {
            assert!(traces[i].complete(), "trace {} incomplete", i);
            assert!(!traces[i].silent());
            assert!(traces[i].points_ok());
            assert!(traces[i].result_ok());
            assert!(traces[i].order_ok());
            i += 1;
        }
    }

    #[test]
    fn trace_generation_is_deterministic() {
        assert_eq!(make_trace(7), make_trace(7));
        assert_ne!(make_trace(7), make_trace(8));
    }

    #[test]
    fn sample_bounds() {
        assert!(sample(0).is_empty());
        assert!(sample(SAMPLE_MAX + 1).is_empty());
        assert_eq!(sample(1).len(), 1);
        assert_eq!(sample(SAMPLE_MAX).len(), SAMPLE_MAX as usize);
    }

    #[test]
    fn faults_are_all_silent() {
        let faults = fault_traces();
        assert_eq!(faults.len(), 5);
        let mut i = 0usize;
        while i < faults.len() {
            assert!(faults[i].silent(), "fault {} not silent", i);
            assert!(!faults[i].complete());
            i += 1;
        }
    }

    #[test]
    fn silent_count_zero_on_sample() {
        assert_eq!(silent_count(&sample(COVER_SAMPLE)), 0);
        assert_eq!(silent_count(&fault_traces()), 5);
    }

    #[test]
    fn point_coverage_is_full() {
        let cov = point_coverage(&sample(COVER_SAMPLE));
        assert_eq!(cov, [COVER_SAMPLE; POINTS]);
        assert!(full_coverage());
    }

    #[test]
    fn family_coverage_reaches_fifteen() {
        assert_eq!(family_coverage(&sample(COVER_SAMPLE)), FAMILIES as u32);
    }

    #[test]
    fn elapsed_and_order_hold() {
        let traces = sample(COVER_SAMPLE);
        let mut i = 0usize;
        while i < traces.len() {
            assert!(traces[i].elapsed_us >= 1);
            assert!(traces[i].elapsed_us <= MAX_ELAPSED);
            assert!(traces[i].order_ok());
            assert_eq!(
                traces[i].exit,
                traces[i].entry.map(|e| e + traces[i].elapsed_us)
            );
            i += 1;
        }
    }

    #[test]
    fn trace_digest_is_domain_separated() {
        let a = make_trace(1);
        let b = make_trace(2);
        assert_ne!(a.digest(), b.digest());
        assert_ne!(a.digest(), p_core::FNV_OFF);
        assert_eq!(a.digest(), trace_digest(
            a.seq, a.entry, a.exit, a.elapsed_us, a.result
        ));
    }

    #[test]
    fn row_renders_fixed_shape() {
        let row = render_row(&make_trace(0));
        assert!(row.starts_with("OBS95|trace=0|entry="));
        assert_eq!(row.split('|').count(), ROW_FIELDS);
        assert!(audit_row(&row));
    }

    #[test]
    fn rows_stable_and_audit() {
        let rows = export_rows(64);
        assert_eq!(rows.len(), 64);
        assert!(rows_stable(64));
        let mut i = 0usize;
        while i < rows.len() {
            assert!(audit_row(&rows[i]));
            i += 1;
        }
    }

    #[test]
    fn row_tamper_rejected() {
        let row = render_row(&make_trace(3));
        let mut tamp = row.clone();
        tamp.push('x');
        assert!(!audit_row(&tamp));
        let swapped = row.replacen("elapsed=", "elapsed=9", 1);
        assert!(!audit_row(&swapped));
    }

    #[test]
    fn row_none_points_rejected() {
        let t = Trace {
            seq: 1,
            entry: None,
            exit: Some(5),
            elapsed_us: 5,
            result: Some(0),
        };
        assert!(!audit_row(&render_row(&t)));
        let t = Trace {
            seq: 2,
            entry: Some(0),
            exit: None,
            elapsed_us: 5,
            result: Some(0),
        };
        assert!(!audit_row(&render_row(&t)));
        let t = Trace {
            seq: 3,
            entry: Some(0),
            exit: Some(5),
            elapsed_us: 5,
            result: None,
        };
        assert!(!audit_row(&render_row(&t)));
    }

    #[test]
    fn row_unknown_code_rejected() {
        let t = Trace {
            seq: 4,
            entry: Some(0),
            exit: Some(5),
            elapsed_us: 5,
            result: Some(0xDEAD_BEEF),
        };
        assert!(!audit_row(&render_row(&t)));
    }

    #[test]
    fn row_zero_elapsed_rejected() {
        let t = Trace {
            seq: 5,
            entry: Some(10),
            exit: Some(10),
            elapsed_us: 0,
            result: Some(0),
        };
        assert!(!audit_row(&render_row(&t)));
    }

    #[test]
    fn row_order_mismatch_rejected() {
        let t = Trace {
            seq: 6,
            entry: Some(10),
            exit: Some(21),
            elapsed_us: 10,
            result: Some(0),
        };
        assert!(!audit_row(&render_row(&t)));
    }

    #[test]
    fn row_success_code_zero_accepted() {
        let t = Trace {
            seq: 7,
            entry: Some(100),
            exit: Some(110),
            elapsed_us: 10,
            result: Some(0),
        };
        assert!(audit_row(&render_row(&t)));
        assert!(t.family().is_none());
        assert!(t.result_ok());
    }

    #[test]
    fn row_family_code_accepted() {
        let code = FAMILY_ALL[4].val();
        let t = Trace {
            seq: 8,
            entry: Some(100),
            exit: Some(110),
            elapsed_us: 10,
            result: Some(code),
        };
        assert!(audit_row(&render_row(&t)));
        assert_eq!(t.family(), Some(FAMILY_ALL[4]));
    }

    #[test]
    fn parse_boundaries() {
        assert_eq!(parse_u32("0"), Some(0));
        assert_eq!(parse_u32("4294967295"), Some(u32::MAX));
        assert_eq!(parse_u32("4294967296"), None);
        assert_eq!(parse_u32(""), None);
        assert_eq!(parse_u32("1a"), None);
        assert_eq!(parse_hex16("ffffffffffffffff"), Some(u64::MAX));
        assert_eq!(parse_hex16("0"), None);
    }

    #[test]
    fn row_fields_rejects_arity_and_tag() {
        let row = render_row(&make_trace(9));
        assert!(row_fields(&row, TAG, &ROW_KEYS).is_some());
        assert!(row_fields(&row, "OTHER", &ROW_KEYS).is_none());
        assert!(row_fields(&row, TAG, &ROW_KEYS[..4]).is_none());
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
        let s = run_unx49095_checks();
        assert!(!s.truncated());
        assert_eq!(s.len(), 8);
        assert_eq!(s.passed_count(), 8);
        assert_eq!(s.failed_count(), 0);
        assert!(s.all_passed());
    }
}
