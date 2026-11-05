//! # unx49097 — B15 fuzz 面与判据用例集
//!
//! 卡面原文（任务板 CoRun·UNX-F2-B40）：
//!
//! > **F49097 恶意设备防御、错误路径前段与首产段收官 fuzz 面（乱参数注入不崩）**
//! > **F49098 恶意设备防御、错误路径前段与首产段收官判据用例集（20 例入库可复跑）**
//!
//! 验收锚点：
//!
//! - `UNX-F49098-J1` ×20 全绿入库；判据锚定 §AI-62 §8。
//! - `UNX-F49099-J1` 批收口防重留痕零撞号；判据锚定 §6.1。
//!
//! ## 闸序与理由
//!
//! 1. **万轮零崩**：fuzz 只调用纯解码器，任意 32 位载荷都必须返回
//!    `Accept` 或 `Reject`，不允许 panic、不允许死循环——这条是后面所有
//!    统计的前提，不先立住，守恒账就是假的。
//! 2. **记账守恒**：接受数 + 拒绝数必须等于轮数；未决队列必须归零。
//!    少记一条或把拒绝悄悄丢掉，都会让守恒式当场失效。
//! 3. **留痕稳定**：同一轮数重复跑三次，摘要必须逐位相同；fuzz 不是
//!    随机表演，回归时得能逐位对账。
//! 4. **用例二十入库**：二十例逐例写明输入与期望，覆盖五种合法相位、
//!    四种拒绝原因与边界字段；少一例或期望写反都判红。
//! 5. **用例全绿**：二十例实跑结果必须与期望一致；用例不是清单摆设，
//!    每一条都要真跑。
//!
//! ## 与相邻模块的差异
//!
//! - `unx49095_observe` 记录的是“一条已发生轨迹的四个点”；本模块制造
//!   **未发生的乱参数**，重点是解码器在任意输入下都不崩。
//! - `unx49089_finalgrep` 复跑的是十五批判据；本模块复跑的是
//!   **二十条输入用例**，粒度到单条载荷。
//! - 解码器只依赖整数位运算，不分配、不递归、不索引切片，天然无 panic 点。

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;
use super::unx48941_wheel_core as p_core;

// ── 段位与常量 ───────────────────────────────────────────────────
/// 本模块段基址（B15 第九槽）。
pub const SEG_BASE: u32 = 0x73700;
/// fuzz 默认轮数（×10^4）。
pub const FUZZ_ROUNDS: u32 = 10_000;
/// fuzz 轮数上限。
pub const FUZZ_MAX: u32 = 20_000;
/// 留痕复跑轮数。
pub const REPLAY_ROUNDS: u32 = 3;
/// 合法相位上限（含）。
pub const PHASE_MAX: u8 = 4;
/// 合法类别上限（含）。
pub const KIND_MAX: u8 = 8;
/// 终止相位。
pub const PHASE_TERMINAL: u8 = 4;
/// 终止类别。
pub const KIND_TERMINAL: u8 = 8;
/// 用例数。
pub const CASES: usize = 20;
/// 用例输入上限（32 位全窗）。
pub const INPUT_MAX: u32 = u32::MAX;
/// 账行字段数。
pub const ROW_FIELDS: usize = 6;
/// fuzz 账标签。
pub const FUZZ_TAG: &str = "FUZZ97";
/// 用例账标签。
pub const CASE_TAG: &str = "CASE98";
/// fuzz 账键序。
pub const FUZZ_KEYS: [&str; 3] = ["round", "accepted", "rejected"];
/// 用例账键序。
pub const CASE_KEYS: [&str; 4] = ["case", "input", "expect", "got"];
/// 未知诊断码说明。
pub const UNKNOWN: &str = "未知 B15 fuzz 诊断码";
/// 段宽（相邻模块互斥）。
pub const SEG_SPAN: u32 = 0x40;

// ── 诊断码 ───────────────────────────────────────────────────────
/// fuzz 诊断码（14 条，说明互不共用）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FzCode {
    /// 轮数为 0 或超上限
    RoundsBad,
    /// 接受数与拒绝数之和不等轮数
    SumBad,
    /// 未决队列不为零
    PendingBad,
    /// 解码器返回了非法相位
    PhaseBad,
    /// 解码器返回了非法类别
    KindBad,
    /// 终止相位类别不匹配
    TerminalBad,
    /// 载荷与类别冲突
    PayloadBad,
    /// 用例编号越界
    CaseBad,
    /// 用例期望与实际不符
    ExpectBad,
    /// 用例覆盖不足
    CoverageBad,
    /// 摘要与重算不符
    DigestBad,
    /// 账行键序错位
    RowBad,
    /// 字段数或字段名不符
    FieldBad,
    /// 用例表不完整
    Incomplete,
}

impl FzCode {
    /// 诊断码值。
    pub fn val(self) -> u32 {
        SEG_BASE + 0x01 + self as u32
    }

    /// 逐码说明（互不共用）。
    pub fn detail(self) -> &'static str {
        match self {
            FzCode::RoundsBad => "fuzz 轮数为 0 或超 20_000",
            FzCode::SumBad => "接受数加拒绝数不等于轮数",
            FzCode::PendingBad => "未决队列在收尾时不为零",
            FzCode::PhaseBad => "相位越出 0..=4",
            FzCode::KindBad => "类别越出 0..=8",
            FzCode::TerminalBad => "终止相位未配终止类别",
            FzCode::PayloadBad => "零类别携带非零载荷",
            FzCode::CaseBad => "用例编号越出 1..=20",
            FzCode::ExpectBad => "用例实跑与期望不符",
            FzCode::CoverageBad => "用例未覆盖四类拒绝",
            FzCode::DigestBad => "摘要与重算不一致",
            FzCode::RowBad => "账行键序或标签错位",
            FzCode::FieldBad => "账行字段数或字段名不符",
            FzCode::Incomplete => "用例表不完整",
        }
    }

    /// 面向运营的短句（与 `detail` 不同口径）。
    pub fn message(self) -> &'static str {
        match self {
            FzCode::RoundsBad => "轮次非法：请收敛到 1..=20_000",
            FzCode::SumBad => "守恒失配：请补记账",
            FzCode::PendingBad => "未决未清：请清空队列",
            FzCode::PhaseBad => "相位越界：已拒收",
            FzCode::KindBad => "类别越界：已拒收",
            FzCode::TerminalBad => "终止错配：已拒收",
            FzCode::PayloadBad => "载荷冲突：已拒收",
            FzCode::CaseBad => "用例非法：请回读用例表",
            FzCode::ExpectBad => "期望失配：请复跑用例",
            FzCode::CoverageBad => "覆盖不足：请补四类拒绝",
            FzCode::DigestBad => "摘要失配：请重算账行",
            FzCode::RowBad => "账行损坏：请重渲染",
            FzCode::FieldBad => "字段错位：请核对键序",
            FzCode::Incomplete => "用例不全：请补足二十例",
        }
    }
}

/// 全部诊断码（顺序即值序）。
pub const ALL: [FzCode; 14] = [
    FzCode::RoundsBad,
    FzCode::SumBad,
    FzCode::PendingBad,
    FzCode::PhaseBad,
    FzCode::KindBad,
    FzCode::TerminalBad,
    FzCode::PayloadBad,
    FzCode::CaseBad,
    FzCode::ExpectBad,
    FzCode::CoverageBad,
    FzCode::DigestBad,
    FzCode::RowBad,
    FzCode::FieldBad,
    FzCode::Incomplete,
];

/// 自由函数：码值 → 说明（段外一律 `UNKNOWN`）。
pub fn detail(code: u32) -> &'static str {
    match code.wrapping_sub(SEG_BASE) {
        1 => FzCode::RoundsBad.detail(),
        2 => FzCode::SumBad.detail(),
        3 => FzCode::PendingBad.detail(),
        4 => FzCode::PhaseBad.detail(),
        5 => FzCode::KindBad.detail(),
        6 => FzCode::TerminalBad.detail(),
        7 => FzCode::PayloadBad.detail(),
        8 => FzCode::CaseBad.detail(),
        9 => FzCode::ExpectBad.detail(),
        10 => FzCode::CoverageBad.detail(),
        11 => FzCode::DigestBad.detail(),
        12 => FzCode::RowBad.detail(),
        13 => FzCode::FieldBad.detail(),
        14 => FzCode::Incomplete.detail(),
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

// ── 解码器 ───────────────────────────────────────────────────────
/// 拒绝原因（四类）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Reject {
    /// 相位越界。
    Phase,
    /// 类别越界。
    Kind,
    /// 终止错配。
    Terminal,
    /// 载荷冲突。
    Payload,
}

impl Reject {
    /// 账行取值。
    pub fn tag(self) -> &'static str {
        match self {
            Reject::Phase => "phase",
            Reject::Kind => "kind",
            Reject::Terminal => "terminal",
            Reject::Payload => "payload",
        }
    }

    /// 由账行取值反解。
    pub fn from_tag(tag: &str) -> Option<Reject> {
        match tag {
            "phase" => Some(Reject::Phase),
            "kind" => Some(Reject::Kind),
            "terminal" => Some(Reject::Terminal),
            "payload" => Some(Reject::Payload),
            _ => None,
        }
    }
}

/// 合法帧。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Frame {
    /// 相位 0..=4。
    pub phase: u8,
    /// 类别 0..=8。
    pub kind: u8,
    /// 载荷 0..=65535。
    pub value: u16,
    /// 时间戳 0..=511。
    pub ts: u16,
}

/// 解码结果。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Outcome {
    /// 接收。
    Accept(Frame),
    /// 拒绝。
    Reject(Reject),
}

impl Outcome {
    /// 是否为接收。
    pub fn accepted(&self) -> bool {
        matches!(self, Outcome::Accept(_))
    }

    /// 是否为拒绝。
    pub fn rejected(&self) -> bool {
        matches!(self, Outcome::Reject(_))
    }

    /// 拒绝原因（接收时为 `None`）。
    pub fn reason(&self) -> Option<Reject> {
        match self {
            Outcome::Accept(_) => None,
            Outcome::Reject(r) => Some(*r),
        }
    }

    /// 账行取值。
    pub fn tag(&self) -> &'static str {
        match self {
            Outcome::Accept(_) => "accept",
            Outcome::Reject(r) => r.tag(),
        }
    }
}

/// 组合一条原始载荷：相位 3 位、类别 4 位、载荷 16 位、时间戳 9 位。
pub fn compose(phase: u8, kind: u8, value: u16, ts: u16) -> u32 {
    let mut raw = 0u32;
    raw |= (phase as u32) & 0x7;
    raw |= ((kind as u32) & 0xF) << 3;
    raw |= ((value as u32) & 0xFFFF) << 7;
    raw |= ((ts as u32) & 0x1FF) << 23;
    raw
}

/// 纯解码器：任意 32 位输入都必须返回一个结果。
pub fn decode(raw: u32) -> Outcome {
    let phase = (raw & 0x7) as u8;
    let kind = ((raw >> 3) & 0xF) as u8;
    let value = ((raw >> 7) & 0xFFFF) as u16;
    let ts = ((raw >> 23) & 0x1FF) as u16;
    if phase > PHASE_MAX {
        return Outcome::Reject(Reject::Phase);
    }
    if kind > KIND_MAX {
        return Outcome::Reject(Reject::Kind);
    }
    if phase == PHASE_TERMINAL && kind != KIND_TERMINAL {
        return Outcome::Reject(Reject::Terminal);
    }
    if kind == 0 && value != 0 {
        return Outcome::Reject(Reject::Payload);
    }
    Outcome::Accept(Frame {
        phase,
        kind,
        value,
        ts,
    })
}

// ── fuzz 统计 ────────────────────────────────────────────────────
/// fuzz 报告。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct FuzzReport {
    /// 轮数。
    pub rounds: u32,
    /// 接受数。
    pub accepted: u32,
    /// 拒绝数。
    pub rejected: u32,
    /// 未决队列（收尾必须为零）。
    pub pending: u32,
    /// 链式摘要。
    pub digest: u64,
}

impl FuzzReport {
    /// 记账守恒：接受 + 拒绝 == 轮数，未决为零。
    pub fn conserved(&self) -> bool {
        self.accepted.saturating_add(self.rejected) == self.rounds && self.pending == 0
    }
}

/// 单轮摘要：轮号 + 原始载荷 + 结果标签。
pub fn round_digest(round: u32, raw: u32, outcome: &Outcome) -> u64 {
    let mut h = p_core::FNV_OFF;
    h = p_core::fold_u32(h, 0x4655_5A5A);
    h = p_core::fold_u32(h, round);
    h = p_core::fold_u32(h, raw);
    let tag = outcome.tag();
    let bytes = tag.as_bytes();
    let mut i = 0usize;
    while i < bytes.len() {
        h = p_core::fold_byte(h, bytes[i]);
        i += 1;
    }
    h
}

/// 跑 `rounds` 轮 fuzz（轮数非法时返回 `None`）。
pub fn fuzz(rounds: u32) -> Option<FuzzReport> {
    if rounds == 0 || rounds > FUZZ_MAX {
        return None;
    }
    let mut rng = p_core::Lcg::new(0x0055_5A5A_97F0_0001);
    let mut accepted = 0u32;
    let mut rejected = 0u32;
    let mut digest = p_core::FNV_OFF;
    let mut i = 0u32;
    while i < rounds {
        let raw = rng.next() as u32;
        let outcome = decode(raw);
        if outcome.accepted() {
            accepted = accepted.saturating_add(1);
        } else {
            rejected = rejected.saturating_add(1);
        }
        let d = round_digest(i, raw, &outcome);
        digest = p_core::fold_u32(p_core::fold_u32(digest, d as u32), (d >> 32) as u32);
        i += 1;
    }
    Some(FuzzReport {
        rounds,
        accepted,
        rejected,
        pending: 0,
        digest,
    })
}

/// 万轮零崩：报告存在、守恒成立、摘要非零。
pub fn fuzz_green() -> bool {
    match fuzz(FUZZ_ROUNDS) {
        Some(r) => r.conserved() && r.digest != p_core::FNV_OFF,
        None => false,
    }
}

/// 留痕稳定：同一轮数重复 `times` 次摘要一致。
pub fn replay_stable(times: u32) -> bool {
    if times == 0 || times > 8 {
        return false;
    }
    let want = match fuzz(FUZZ_ROUNDS) {
        Some(r) => r.digest,
        None => return false,
    };
    let mut i = 0u32;
    while i < times {
        match fuzz(FUZZ_ROUNDS) {
            Some(r) => {
                if r.digest != want {
                    return false;
                }
            }
            None => return false,
        }
        i += 1;
    }
    want != p_core::FNV_OFF
}

// ── 二十例用例集 ─────────────────────────────────────────────────
/// 用例期望。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Expect {
    /// 期望接收。
    Accept,
    /// 期望拒绝。
    Reject,
}

impl Expect {
    /// 账行取值。
    pub fn tag(self) -> &'static str {
        match self {
            Expect::Accept => "accept",
            Expect::Reject => "reject",
        }
    }

    /// 由账行取值反解。
    pub fn from_tag(tag: &str) -> Option<Expect> {
        match tag {
            "accept" => Some(Expect::Accept),
            "reject" => Some(Expect::Reject),
            _ => None,
        }
    }
}

/// 单条用例。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Case {
    /// 编号 1..=20。
    pub id: u8,
    /// 用例名。
    pub name: &'static str,
    /// 原始载荷。
    pub raw: u32,
    /// 期望。
    pub expect: Expect,
}

impl Case {
    /// 实跑结果。
    pub fn outcome(&self) -> Outcome {
        decode(self.raw)
    }

    /// 是否与期望一致。
    pub fn passed(&self) -> bool {
        match self.expect {
            Expect::Accept => self.outcome().accepted(),
            Expect::Reject => self.outcome().rejected(),
        }
    }
}

/// 二十例（覆盖五种合法相位、四类拒绝与字段边界）。
pub const CASE_TABLE: [Case; CASES] = [
    Case { id: 1, name: "基础接收", raw: 1 | (1 << 3) | (5 << 7) | (1 << 23), expect: Expect::Accept },
    Case { id: 2, name: "零载荷接收", raw: 1 | (2 << 3) | (2 << 23), expect: Expect::Accept },
    Case { id: 3, name: "终止帧接收", raw: 4 | (8 << 3) | (3 << 23), expect: Expect::Accept },
    Case { id: 4, name: "终止帧带载荷", raw: 4 | (8 << 3) | (3 << 7) | (4 << 23), expect: Expect::Accept },
    Case { id: 5, name: "相位三接收", raw: 3 | (0 << 3) | (4 << 23), expect: Expect::Accept },
    Case { id: 6, name: "相位越界", raw: 5, expect: Expect::Reject },
    Case { id: 7, name: "相位七越界", raw: 7, expect: Expect::Reject },
    Case { id: 8, name: "类别越界", raw: (9 << 3), expect: Expect::Reject },
    Case { id: 9, name: "类别十五越界", raw: (15 << 3), expect: Expect::Reject },
    Case { id: 10, name: "零类别带载荷", raw: 1 | (1 << 7), expect: Expect::Reject },
    Case { id: 11, name: "零类别满载荷", raw: 1 | (0xFFFF << 7), expect: Expect::Reject },
    Case { id: 12, name: "终止错配", raw: 4 | (7 << 3), expect: Expect::Reject },
    Case { id: 13, name: "终止零类别", raw: 4, expect: Expect::Reject },
    Case { id: 14, name: "满载荷接收", raw: 2 | (3 << 3) | (0xFFFF << 7), expect: Expect::Accept },
    Case { id: 15, name: "满时间戳接收", raw: 2 | (3 << 3) | (1 << 7) | (0x1FF << 23), expect: Expect::Accept },
    Case { id: 16, name: "零时间戳接收", raw: 0 | (2 << 3), expect: Expect::Accept },
    Case { id: 17, name: "类别八接收", raw: 1 | (8 << 3), expect: Expect::Accept },
    Case { id: 18, name: "相位四类别零", raw: 4 | (0 << 3), expect: Expect::Reject },
    Case { id: 19, name: "类别七相位三", raw: 3 | (7 << 3) | (9 << 7), expect: Expect::Accept },
    Case { id: 20, name: "类别十相位二", raw: 2 | (10 << 3), expect: Expect::Reject },
];

/// 全部用例。
pub fn cases() -> &'static [Case] {
    &CASE_TABLE
}

/// 按编号取用例。
pub fn case_for(id: u8) -> Option<&'static Case> {
    let mut i = 0usize;
    while i < CASE_TABLE.len() {
        if CASE_TABLE[i].id == id {
            return Some(&CASE_TABLE[i]);
        }
        i += 1;
    }
    None
}

/// 用例全绿（二十例实跑与期望一致）。
pub fn suite_green() -> bool {
    if CASE_TABLE.len() != CASES {
        return false;
    }
    let mut i = 0usize;
    while i < CASE_TABLE.len() {
        if CASE_TABLE[i].id != (i as u8) + 1 || !CASE_TABLE[i].passed() {
            return false;
        }
        i += 1;
    }
    true
}

/// 拒绝原因覆盖数（必须为四）。
pub fn reject_coverage() -> u32 {
    let mut seen = [false; 4];
    let mut i = 0usize;
    while i < CASE_TABLE.len() {
        if let Some(r) = CASE_TABLE[i].outcome().reason() {
            let idx = match r {
                Reject::Phase => 0,
                Reject::Kind => 1,
                Reject::Terminal => 2,
                Reject::Payload => 3,
            };
            seen[idx] = true;
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

/// 用例摘要：编号 + 输入 + 期望 + 实跑。
pub fn case_digest(id: u8, raw: u32, expect: Expect, outcome: &Outcome) -> u64 {
    let mut h = p_core::FNV_OFF;
    h = p_core::fold_u32(h, 0x4341_5345);
    h = p_core::fold_u32(h, id as u32);
    h = p_core::fold_u32(h, raw);
    let e = expect.tag();
    let bytes = e.as_bytes();
    let mut i = 0usize;
    while i < bytes.len() {
        h = p_core::fold_byte(h, bytes[i]);
        i += 1;
    }
    let g = outcome.tag();
    let bytes = g.as_bytes();
    let mut i = 0usize;
    while i < bytes.len() {
        h = p_core::fold_byte(h, bytes[i]);
        i += 1;
    }
    h
}

/// 用例集链式摘要。
pub fn suite_digest() -> u64 {
    let mut h = p_core::FNV_OFF;
    let mut i = 0usize;
    while i < CASE_TABLE.len() {
        let c = &CASE_TABLE[i];
        let d = case_digest(c.id, c.raw, c.expect, &c.outcome());
        h = p_core::fold_u32(p_core::fold_u32(h, d as u32), (d >> 32) as u32);
        i += 1;
    }
    h
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

/// 渲染 fuzz 账行。
pub fn render_fuzz_row(r: &FuzzReport) -> String {
    format!(
        "{}|round={}|accepted={}|rejected={}|digest={:016x}",
        FUZZ_TAG, r.rounds, r.accepted, r.rejected, r.digest
    )
}

/// 回读 fuzz 账行（重跑同轮数并比对）。
pub fn audit_fuzz_row(row: &str) -> bool {
    let f = match row_fields(row, FUZZ_TAG, &FUZZ_KEYS) {
        Some(v) if v.len() == FUZZ_KEYS.len() + 1 => v,
        _ => return false,
    };
    let rounds = match parse_u32(f[0]) {
        Some(v) if v >= 1 && v <= FUZZ_MAX => v,
        _ => return false,
    };
    let accepted = match parse_u32(f[1]) {
        Some(v) => v,
        _ => return false,
    };
    let rejected = match parse_u32(f[2]) {
        Some(v) => v,
        _ => return false,
    };
    if accepted.saturating_add(rejected) != rounds {
        return false;
    }
    let want = match parse_hex16(f[3]) {
        Some(v) => v,
        None => return false,
    };
    match fuzz(rounds) {
        Some(r) => r.accepted == accepted && r.rejected == rejected && r.digest == want,
        None => false,
    }
}

/// 渲染用例账行。
pub fn render_case_row(c: &Case) -> String {
    format!(
        "{}|case={}|input={}|expect={}|got={}|digest={:016x}",
        CASE_TAG,
        c.id,
        c.raw,
        c.expect.tag(),
        c.outcome().tag(),
        case_digest(c.id, c.raw, c.expect, &c.outcome())
    )
}

/// 回读用例账行。
pub fn audit_case_row(row: &str) -> bool {
    let f = match row_fields(row, CASE_TAG, &CASE_KEYS) {
        Some(v) if v.len() == CASE_KEYS.len() + 1 => v,
        _ => return false,
    };
    let id = match parse_u32(f[0]) {
        Some(v) if v >= 1 && v <= CASES as u32 => v as u8,
        _ => return false,
    };
    let raw = match parse_u32(f[1]) {
        Some(v) => v,
        _ => return false,
    };
    let expect = match Expect::from_tag(f[2]) {
        Some(v) => v,
        None => return false,
    };
    let got = match Outcome::tag_of(f[3]) {
        Some(v) => v,
        None => return false,
    };
    let outcome = decode(raw);
    if outcome.tag() != got.tag() {
        return false;
    }
    let want = match parse_hex16(f[4]) {
        Some(v) => v,
        None => return false,
    };
    case_digest(id, raw, expect, &outcome) == want
}

impl Outcome {
    /// 由账行取值反解（仅接收/四种拒绝）。
    pub fn tag_of(tag: &str) -> Option<Outcome> {
        if tag == "accept" {
            return Some(Outcome::Accept(Frame {
                phase: 0,
                kind: 0,
                value: 0,
                ts: 0,
            }));
        }
        match Reject::from_tag(tag) {
            Some(r) => Some(Outcome::Reject(r)),
            None => None,
        }
    }
}

/// 导出二十例账行。
pub fn export_case_rows() -> Vec<String> {
    let mut out = Vec::new();
    let mut i = 0usize;
    while i < CASE_TABLE.len() {
        out.push(render_case_row(&CASE_TABLE[i]));
        i += 1;
    }
    out
}

/// 二十行两两不同且逐行可回读。
pub fn case_rows_stable() -> bool {
    let rows = export_case_rows();
    if rows.len() != CASES {
        return false;
    }
    let mut i = 0usize;
    while i < rows.len() {
        if !audit_case_row(&rows[i]) {
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

    // 1) 万轮零崩。
    let report = fuzz(FUZZ_ROUNDS);
    let crash_free = match report {
        Some(r) => r.conserved() && r.digest != p_core::FNV_OFF,
        None => false,
    };
    let bounds_rejected = fuzz(0).is_none() && fuzz(FUZZ_MAX + 1).is_none();
    let ok = fuzz_green() && crash_free && bounds_rejected;
    out.push((
        "F49097/万轮零崩",
        ok,
        format!(
            "rounds={} crash_free={} bounds_rejected={}",
            FUZZ_ROUNDS, crash_free, bounds_rejected
        ),
    ));

    // 2) 记账守恒。
    let ok = match report {
        Some(r) => {
            r.accepted.saturating_add(r.rejected) == r.rounds
                && r.pending == 0
                && r.accepted > 0
                && r.rejected > 0
        }
        None => false,
    };
    out.push((
        "F49097/记账守恒",
        ok,
        match report {
            Some(r) => format!(
                "accepted={} rejected={} sum={} pending={}",
                r.accepted,
                r.rejected,
                r.accepted.saturating_add(r.rejected),
                r.pending
            ),
            None => String::from("no report"),
        },
    ));

    // 3) 留痕稳定。
    let ok = replay_stable(REPLAY_ROUNDS) && !replay_stable(0);
    out.push((
        "F49097/留痕稳定",
        ok,
        match report {
            Some(r) => format!("replays={} digest={:016x}", REPLAY_ROUNDS, r.digest),
            None => String::from("no report"),
        },
    ));

    // 4) fuzz 账行回读。
    let row_ok = match report {
        Some(r) => {
            let row = render_fuzz_row(&r);
            let mut tamp = row.clone();
            tamp.push('x');
            audit_fuzz_row(&row) && !audit_fuzz_row(&tamp)
        }
        None => false,
    };
    out.push((
        "F49097/账行回读",
        row_ok,
        format!("row_ok={} rounds={}", row_ok, FUZZ_ROUNDS),
    ));

    // 5) 用例二十入库。
    let mut ids_ok = CASE_TABLE.len() == CASES;
    let mut names_ok = true;
    let mut i = 0usize;
    while i < CASE_TABLE.len() {
        if CASE_TABLE[i].id != (i as u8) + 1 || CASE_TABLE[i].name.is_empty() {
            ids_ok = false;
        }
        let mut j = i + 1;
        while j < CASE_TABLE.len() {
            if CASE_TABLE[j].name == CASE_TABLE[i].name {
                names_ok = false;
            }
            j += 1;
        }
        i += 1;
    }
    let ok = ids_ok && names_ok && case_for(0).is_none() && case_for((CASES as u8) + 1).is_none();
    out.push((
        "F49098/用例二十入库",
        ok,
        format!("cases={} ids_ok={} names_ok={}", CASE_TABLE.len(), ids_ok, names_ok),
    ));

    // 6) 用例全绿。
    let mut red = 0u32;
    let mut i = 0usize;
    while i < CASE_TABLE.len() {
        if !CASE_TABLE[i].passed() {
            red += 1;
        }
        i += 1;
    }
    let ok = suite_green() && red == 0 && suite_digest() != p_core::FNV_OFF;
    out.push((
        "F49098/用例全绿",
        ok,
        format!("red={} suite={} digest={:016x}", red, suite_green(), suite_digest()),
    ));

    // 7) 用例覆盖四类拒绝。
    let ok = reject_coverage() == 4;
    out.push((
        "F49098/用例覆盖四类",
        ok,
        format!("reject_kinds={}", reject_coverage()),
    ));

    // 8) 用例账行回读。
    let rows = export_case_rows();
    let mut tamp = String::new();
    if let Some(first) = rows.first() {
        tamp.push_str(first);
        tamp.push('x');
    }
    let tamper_rejected = !tamp.is_empty() && !audit_case_row(&tamp);
    let ok = rows.len() == CASES && case_rows_stable() && tamper_rejected;
    out.push((
        "F49098/账行回读",
        ok,
        format!(
            "rows={} stable={} tamper_rejected={}",
            rows.len(),
            case_rows_stable(),
            tamper_rejected
        ),
    ));

    out
}

/// 入口：八项判据写入 CheckSet。
pub fn run_unx49097_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F49097 fuzz 面与用例集");
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

    // ── 段位与诊断码 ─────────────────────────────────────────────

    #[test]
    fn codes_in_window() {
        assert!(segment_guard());
        assert_eq!(ALL.len(), 14);
        let mut i = 0usize;
        while i < ALL.len() {
            assert!(ALL[i].val() > SEG_BASE);
            assert!(ALL[i].val() < SEG_BASE + SEG_SPAN);
            i += 1;
        }
    }

    #[test]
    fn codes_values_distinct_and_ordered() {
        let mut i = 0usize;
        while i < ALL.len() {
            assert_eq!(ALL[i].val(), SEG_BASE + 1 + i as u32);
            let mut j = i + 1;
            while j < ALL.len() {
                assert_ne!(ALL[j].val(), ALL[i].val());
                j += 1;
            }
            i += 1;
        }
    }

    #[test]
    fn details_distinct() {
        let mut i = 0usize;
        while i < ALL.len() {
            let mut j = i + 1;
            while j < ALL.len() {
                assert_ne!(ALL[j].detail(), ALL[i].detail());
                j += 1;
            }
            i += 1;
        }
    }

    #[test]
    fn messages_distinct() {
        let mut i = 0usize;
        while i < ALL.len() {
            let mut j = i + 1;
            while j < ALL.len() {
                assert_ne!(ALL[j].message(), ALL[i].message());
                j += 1;
            }
            i += 1;
        }
    }

    #[test]
    fn detail_free_function_matches_table() {
        let mut i = 0usize;
        while i < ALL.len() {
            assert_eq!(detail(ALL[i].val()), ALL[i].detail());
            i += 1;
        }
    }

    #[test]
    fn detail_unknown_outside_segment() {
        assert_eq!(detail(SEG_BASE), UNKNOWN);
        assert_eq!(detail(SEG_BASE + 0x0F), UNKNOWN);
        assert_eq!(detail(SEG_BASE + SEG_SPAN), UNKNOWN);
        assert_eq!(detail(0), UNKNOWN);
    }

    // ── 拒绝原因与期望 ───────────────────────────────────────────

    #[test]
    fn reject_tag_roundtrip() {
        let all = [
            Reject::Phase,
            Reject::Kind,
            Reject::Terminal,
            Reject::Payload,
        ];
        let mut i = 0usize;
        while i < all.len() {
            assert_eq!(Reject::from_tag(all[i].tag()), Some(all[i]));
            i += 1;
        }
        assert_eq!(Reject::from_tag(""), None);
        assert_eq!(Reject::from_tag("accept"), None);
    }

    #[test]
    fn reject_tags_distinct() {
        let all = [
            Reject::Phase,
            Reject::Kind,
            Reject::Terminal,
            Reject::Payload,
        ];
        let mut i = 0usize;
        while i < all.len() {
            let mut j = i + 1;
            while j < all.len() {
                assert_ne!(all[j].tag(), all[i].tag());
                j += 1;
            }
            i += 1;
        }
    }

    #[test]
    fn expect_tag_roundtrip() {
        assert_eq!(Expect::from_tag(Expect::Accept.tag()), Some(Expect::Accept));
        assert_eq!(Expect::from_tag(Expect::Reject.tag()), Some(Expect::Reject));
        assert_eq!(Expect::from_tag("phase"), None);
    }

    // ── 编解码 ──────────────────────────────────────────────────

    #[test]
    fn compose_decode_roundtrip() {
        let raw = compose(2, 3, 0x1234, 0x1AB);
        match decode(raw) {
            Outcome::Accept(f) => {
                assert_eq!(f.phase, 2);
                assert_eq!(f.kind, 3);
                assert_eq!(f.value, 0x1234);
                assert_eq!(f.ts, 0x1AB);
            }
            Outcome::Reject(_) => panic!("合法帧被拒"),
        }
    }

    #[test]
    fn compose_masks_fields() {
        assert_eq!(compose(0, 0, 0, 0), 0);
        assert_eq!(compose(7, 15, 0xFFFF, 0x1FF), 0xFFFF_FFFF);
    }

    #[test]
    fn decode_phase_above_max_rejected() {
        assert_eq!(decode(compose(5, 0, 0, 0)), Outcome::Reject(Reject::Phase));
        assert_eq!(decode(compose(7, 8, 0, 0)), Outcome::Reject(Reject::Phase));
    }

    #[test]
    fn decode_kind_above_max_rejected() {
        assert_eq!(decode(compose(0, 9, 0, 0)), Outcome::Reject(Reject::Kind));
        assert_eq!(decode(compose(0, 15, 0, 0)), Outcome::Reject(Reject::Kind));
    }

    #[test]
    fn decode_terminal_mismatch_rejected() {
        assert_eq!(decode(compose(4, 7, 0, 0)), Outcome::Reject(Reject::Terminal));
        assert_eq!(decode(compose(4, 0, 0, 0)), Outcome::Reject(Reject::Terminal));
    }

    #[test]
    fn decode_terminal_with_terminal_kind_accepted() {
        match decode(compose(4, 8, 9, 5)) {
            Outcome::Accept(f) => {
                assert_eq!(f.phase, PHASE_TERMINAL);
                assert_eq!(f.kind, KIND_TERMINAL);
                assert_eq!(f.value, 9);
            }
            Outcome::Reject(_) => panic!("终止帧被拒"),
        }
    }

    #[test]
    fn decode_zero_kind_with_payload_rejected() {
        assert_eq!(decode(compose(1, 0, 1, 0)), Outcome::Reject(Reject::Payload));
        assert_eq!(
            decode(compose(1, 0, 0xFFFF, 0)),
            Outcome::Reject(Reject::Payload)
        );
    }

    #[test]
    fn decode_zero_kind_zero_payload_accepted() {
        assert!(decode(compose(1, 0, 0, 0)).accepted());
    }

    #[test]
    fn decode_timestamp_full_window() {
        match decode(compose(0, 1, 0, 0x1FF)) {
            Outcome::Accept(f) => assert_eq!(f.ts, 0x1FF),
            Outcome::Reject(_) => panic!("满时间戳被拒"),
        }
    }

    #[test]
    fn outcome_predicates() {
        let ok = decode(compose(0, 1, 0, 0));
        assert!(ok.accepted());
        assert!(!ok.rejected());
        assert_eq!(ok.reason(), None);
        assert_eq!(ok.tag(), "accept");
        let bad = decode(compose(5, 0, 0, 0));
        assert!(!bad.accepted());
        assert!(bad.rejected());
        assert_eq!(bad.reason(), Some(Reject::Phase));
        assert_eq!(bad.tag(), "phase");
    }

    #[test]
    fn tag_of_roundtrip() {
        let tags = ["accept", "phase", "kind", "terminal", "payload"];
        let mut i = 0usize;
        while i < tags.len() {
            let o = match Outcome::tag_of(tags[i]) {
                Some(v) => v,
                None => panic!("标签反解失败"),
            };
            assert_eq!(o.tag(), tags[i]);
            i += 1;
        }
        assert_eq!(Outcome::tag_of("bogus"), None);
    }

    // ── fuzz 统计 ───────────────────────────────────────────────

    #[test]
    fn fuzz_rejects_illegal_rounds() {
        assert_eq!(fuzz(0), None);
        assert_eq!(fuzz(FUZZ_MAX + 1), None);
        assert!(fuzz(u32::MAX).is_none());
    }

    #[test]
    fn fuzz_one_round_conserved() {
        let r = fuzz(1).expect("一轮 fuzz 应成立");
        assert_eq!(r.rounds, 1);
        assert!(r.conserved());
        assert_eq!(r.pending, 0);
    }

    #[test]
    fn fuzz_max_rounds_conserved() {
        let r = fuzz(FUZZ_MAX).expect("上限轮数应成立");
        assert_eq!(r.rounds, FUZZ_MAX);
        assert!(r.conserved());
        assert!(r.accepted > 0);
        assert!(r.rejected > 0);
    }

    #[test]
    fn fuzz_ten_thousand_green() {
        assert!(fuzz_green());
        let r = fuzz(FUZZ_ROUNDS).expect("万轮应成立");
        assert_eq!(r.accepted.saturating_add(r.rejected), FUZZ_ROUNDS);
        assert_ne!(r.digest, p_core::FNV_OFF);
    }

    #[test]
    fn fuzz_deterministic() {
        let a = fuzz(4096).expect("首跑");
        let b = fuzz(4096).expect("复跑");
        assert_eq!(a, b);
    }

    #[test]
    fn replay_stable_bounds() {
        assert!(replay_stable(1));
        assert!(replay_stable(REPLAY_ROUNDS));
        assert!(replay_stable(8));
        assert!(!replay_stable(0));
        assert!(!replay_stable(9));
    }

    #[test]
    fn conserved_rejects_pending() {
        let bad = FuzzReport {
            rounds: 4,
            accepted: 2,
            rejected: 2,
            pending: 1,
            digest: 1,
        };
        assert!(!bad.conserved());
        let short = FuzzReport {
            rounds: 4,
            accepted: 2,
            rejected: 1,
            pending: 0,
            digest: 1,
        };
        assert!(!short.conserved());
        let good = FuzzReport {
            rounds: 3,
            accepted: 2,
            rejected: 1,
            pending: 0,
            digest: 1,
        };
        assert!(good.conserved());
    }

    #[test]
    fn round_digest_depends_on_round() {
        let o = decode(compose(0, 1, 0, 0));
        assert_ne!(round_digest(1, 7, &o), round_digest(2, 7, &o));
    }

    #[test]
    fn round_digest_depends_on_payload() {
        let o = decode(compose(0, 1, 0, 0));
        assert_ne!(round_digest(1, 7, &o), round_digest(1, 8, &o));
    }

    #[test]
    fn round_digest_depends_on_outcome() {
        let a = decode(compose(0, 1, 0, 0));
        let b = decode(compose(5, 0, 0, 0));
        assert_ne!(round_digest(1, 7, &a), round_digest(1, 7, &b));
    }

    // ── 用例集 ──────────────────────────────────────────────────

    #[test]
    fn case_table_size() {
        assert_eq!(cases().len(), CASES);
        assert_eq!(CASES, 20);
    }

    #[test]
    fn case_ids_sequential() {
        let mut i = 0usize;
        while i < CASE_TABLE.len() {
            assert_eq!(CASE_TABLE[i].id, (i as u8) + 1);
            i += 1;
        }
    }

    #[test]
    fn case_names_distinct_and_nonempty() {
        let mut i = 0usize;
        while i < CASE_TABLE.len() {
            assert!(!CASE_TABLE[i].name.is_empty());
            let mut j = i + 1;
            while j < CASE_TABLE.len() {
                assert_ne!(CASE_TABLE[j].name, CASE_TABLE[i].name);
                j += 1;
            }
            i += 1;
        }
    }

    #[test]
    fn case_expectations_hold() {
        assert!(suite_green());
        let mut i = 0usize;
        while i < CASE_TABLE.len() {
            assert!(CASE_TABLE[i].passed());
            i += 1;
        }
    }

    #[test]
    fn case_accept_and_reject_balanced() {
        let mut acc = 0u32;
        let mut rej = 0u32;
        let mut i = 0usize;
        while i < CASE_TABLE.len() {
            if CASE_TABLE[i].outcome().accepted() {
                acc += 1;
            } else {
                rej += 1;
            }
            i += 1;
        }
        assert_eq!(acc, 10);
        assert_eq!(rej, 10);
    }

    #[test]
    fn case_lookup_bounds() {
        assert!(case_for(1).is_some());
        assert!(case_for(CASES as u8).is_some());
        assert_eq!(case_for(0), None);
        assert_eq!(case_for((CASES as u8) + 1), None);
    }

    #[test]
    fn case_lookup_returns_same() {
        let c = case_for(7).expect("用例七");
        assert_eq!(c.id, 7);
        assert_eq!(c.name, "相位七越界");
    }

    #[test]
    fn reject_coverage_four_kinds() {
        assert_eq!(reject_coverage(), 4);
        assert!(CASE_TABLE
            .iter()
            .any(|c| c.outcome().reason() == Some(Reject::Phase)));
        assert!(CASE_TABLE
            .iter()
            .any(|c| c.outcome().reason() == Some(Reject::Kind)));
        assert!(CASE_TABLE
            .iter()
            .any(|c| c.outcome().reason() == Some(Reject::Terminal)));
        assert!(CASE_TABLE
            .iter()
            .any(|c| c.outcome().reason() == Some(Reject::Payload)));
    }

    #[test]
    fn suite_digest_stable() {
        let a = suite_digest();
        let b = suite_digest();
        assert_eq!(a, b);
        assert_ne!(a, p_core::FNV_OFF);
    }

    #[test]
    fn case_digest_depends_on_id() {
        let o = decode(compose(0, 1, 0, 0));
        assert_ne!(
            case_digest(1, 7, Expect::Accept, &o),
            case_digest(2, 7, Expect::Accept, &o)
        );
    }

    #[test]
    fn case_digest_depends_on_expect() {
        let o = decode(compose(0, 1, 0, 0));
        assert_ne!(
            case_digest(1, 7, Expect::Accept, &o),
            case_digest(1, 7, Expect::Reject, &o)
        );
    }

    // ── 账行 ────────────────────────────────────────────────────

    #[test]
    fn case_row_field_count() {
        let row = render_case_row(&CASE_TABLE[0]);
        assert_eq!(row.split('|').count(), ROW_FIELDS);
        assert!(row.starts_with(CASE_TAG));
    }

    #[test]
    fn case_rows_export_and_stable() {
        let rows = export_case_rows();
        assert_eq!(rows.len(), CASES);
        assert!(case_rows_stable());
    }

    #[test]
    fn case_rows_pairwise_distinct() {
        let rows = export_case_rows();
        let mut i = 0usize;
        while i < rows.len() {
            let mut j = i + 1;
            while j < rows.len() {
                assert_ne!(rows[j], rows[i]);
                j += 1;
            }
            i += 1;
        }
    }

    #[test]
    fn case_row_audit_rejects_edge_tamper() {
        let row = render_case_row(&CASE_TABLE[0]);
        let mut tampered = row.clone();
        tampered.push('x');
        assert!(!audit_case_row(&tampered));
        let swapped = row.replace("case=1|", "case=2|");
        assert!(!audit_case_row(&swapped));
    }

    #[test]
    fn fuzz_row_render_audit() {
        let r = fuzz(128).expect("128 轮");
        let row = render_fuzz_row(&r);
        assert!(row.starts_with(FUZZ_TAG));
        assert_eq!(row.split('|').count(), FUZZ_KEYS.len() + 2);
        assert!(audit_fuzz_row(&row));
    }

    #[test]
    fn fuzz_row_audit_rejects_bad_numbers() {
        let r = fuzz(64).expect("64 轮");
        let row = render_fuzz_row(&r);
        let bad = row.replace("round=64", "round=65");
        assert!(!audit_fuzz_row(&bad));
        let mut tampered = row.clone();
        tampered.push('0');
        assert!(!audit_fuzz_row(&tampered));
    }

    #[test]
    fn fuzz_row_rejects_out_of_range_rounds() {
        assert!(!audit_fuzz_row(
            "FUZZ97|round=0|accepted=0|rejected=0|digest=0000000000000000"
        ));
        assert!(!audit_fuzz_row(
            "FUZZ97|round=20001|accepted=0|rejected=0|digest=0000000000000000"
        ));
    }

    #[test]
    fn parse_u32_boundaries() {
        assert_eq!(parse_u32("0"), Some(0));
        assert_eq!(parse_u32("4294967295"), Some(INPUT_MAX));
        assert_eq!(parse_u32("4294967296"), None);
        assert_eq!(parse_u32(""), None);
        assert_eq!(parse_u32("12a"), None);
        assert_eq!(parse_u32("-1"), None);
    }

    #[test]
    fn parse_hex16_boundaries() {
        assert_eq!(parse_hex16("0000000000000000"), Some(0));
        assert_eq!(parse_hex16("0123456789abcdef"), Some(0x0123_4567_89AB_CDEF));
        assert_eq!(parse_hex16("0123456789ABCDEF"), Some(0x0123_4567_89AB_CDEF));
        assert_eq!(parse_hex16("0123456789abcde"), None);
        assert_eq!(parse_hex16("0123456789abcdef0"), None);
        assert_eq!(parse_hex16("0123456789abcdeg"), None);
    }

    #[test]
    fn row_fields_rejects_bad_shape() {
        let good = render_fuzz_row(&fuzz(8).expect("8 轮"));
        assert_eq!(row_fields(&good, FUZZ_TAG, &FUZZ_KEYS).map(|v| v.len()), Some(4));
        assert_eq!(row_fields(&good, CASE_TAG, &FUZZ_KEYS), None);
        assert_eq!(row_fields(&good, FUZZ_TAG, &CASE_KEYS), None);
        assert_eq!(row_fields("FUZZ97", FUZZ_TAG, &FUZZ_KEYS), None);
        let mut extra = good.clone();
        extra.push('|');
        extra.push_str("tail=1");
        assert_eq!(row_fields(&extra, FUZZ_TAG, &FUZZ_KEYS), None);
    }

    // ── CheckSet 契约 ───────────────────────────────────────────

    #[test]
    fn checkset_has_eight_green() {
        let s = run_unx49097_checks();
        assert_eq!(s.len(), 8);
        assert_eq!(s.passed_count(), 8);
        assert_eq!(s.failed_count(), 0);
        assert!(s.all_passed());
        assert!(!s.truncated());
        assert!(s.red_items_text().is_empty());
    }

    #[test]
    fn checkset_names_stable() {
        let s = run_unx49097_checks();
        let text = s.red_items_text();
        assert!(text.is_empty());
        let items = extra_checks();
        assert_eq!(items.len(), 8);
        let mut i = 0usize;
        while i < items.len() {
            assert!(items[i].1);
            assert!(
                items[i].0.starts_with("F49097/") || items[i].0.starts_with("F49098/")
            );
            assert!(!items[i].2.is_empty());
            i += 1;
        }
    }
}
