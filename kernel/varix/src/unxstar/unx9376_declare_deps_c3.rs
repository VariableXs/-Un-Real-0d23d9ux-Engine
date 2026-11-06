//! UNX-F9376 联调上游依赖登记（C3 未 finalize 依赖声明） · 0x7CB80段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（DISPATCH_VX11_B21 · UNX-F9376 逐字）：
//!   ① 纯功能代码 ≥320 行；
//!   ② UNX-F9376-J1 依赖声明判据过，C3 上游未 finalize 显式登记格过，
//!     fake 开发路径格过（§52 防卡死规则一）；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//!
//! 设计要点（域模型为「上游依赖登记协议」，与样板 fd 矩阵完全不同）：
//!   ① 依赖条目三态闭集：Finalized（上游卡已收口）/ Unfinalized（上游
//!      未 finalize）/ FakePath（开发桩路径）。C3 联调面遇到 Unfinalized
//!      上游被硬依赖时必须显式登记并挂起等待（EAGAIN 语义），不许假装
//!      依赖就绪、也不许死等自旋——这是 §52 防卡死规则一的可执行形态。
//!   ② 登记表状态机：登记 → 确认 → 失效；事件日志 append-only 只增
//!      不减，已失效条目再确认、未登记条目直接确认/失效均按非法倒退
//!      拒绝，拒绝事件同样入账，任何环节不得静默丢失。
//!   ③ 依赖声明判定器 judge_dep 判定次序固定：名非法(EINVAL) →
//!      未登记(ENOENT) → 已失效(ESTALE) → 声明状态与登记状态不符
//!      (EINVAL) → 冻结面引用 FakePath(EROFS) → 状态语义
//!      （Unfinalized→EAGAIN；FakePath 无有效桩→EPERM；其余 Ok）。
//!      次序即契约，测试逐对锁定。
//!   ④ fake 开发路径格：FakePath 依赖必须携带桩指纹且可复算
//!      （declared == stub_digest(name, card)），复算不符按 EPERM 拒绝；
//!      被冻结清单引用的 FakePath 依赖直接 EROFS——桩不得逃逸进冻结面。
//!   ⑤ 依赖账守恒：登记数 = 确认数 + 挂起数 + 拒绝数，每次声明判定
//!      恰好落一个桶；容量耗尽（表满 ENOMEM）计入拒绝桶，账不丢。
//!   ⑥ 注入器三类（未登记硬依赖 / FakePath 无桩 / 冻结面引用 FakePath）
//!      各 10 轮全抓；规范声明语料双跑 FNV-1a 指纹逐字节一致。
//! ─────────────────────────────────────────────────────────────────────

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（F9376 独占 0x40 窗口）。
pub const SEG_BASE: u32 = 0x7CB80;

/// 登记表容量（依赖条目定容上限）。
pub const MAX_DEPS: usize = 32;

/// 依赖名字节上限（含 24；空名与超限均 EINVAL）。
pub const MAX_NAME_BYTES: usize = 24;

/// 单类注入的复测轮数（判据要求 10/10）。
pub const INJECT_ROUNDS: u32 = 10;

/// 注入类别数（未登记硬依赖 / FakePath 无桩 / 冻结面引用 FakePath）。
pub const INJECT_CLASSES: u32 = 3;

/// 固定 LCG 乘子（确定性伪随机，禁用真随机）。
pub const LCG_A: u32 = 1_664_525;
/// 固定 LCG 增量。
pub const LCG_C: u32 = 1_013_904_223;

/// Linux errno 正数口径（声明判定器返回值语义）。
pub const EPERM: i32 = 1;
pub const ENOENT: i32 = 2;
pub const EAGAIN: i32 = 11;
pub const ENOMEM: i32 = 12;
pub const EEXIST: i32 = 17;
pub const EINVAL: i32 = 22;
pub const EROFS: i32 = 30;
pub const ESTALE: i32 = 116;

// ── 诊断码闭集 ────────────────────────────────────────────────────────
/// 本模块诊断码（段内 +0x00..+0x0E 闭集，15 个）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DepCode {
    /// 依赖名为空或超字节上限
    BadName,
    /// 上游卡号为负
    NegativeCard,
    /// 未登记硬依赖：声明了一个登记表里不存在的依赖
    NotRegistered,
    /// 重复登记：同名条目已存在
    DuplicateReg,
    /// 登记表容量耗尽
    CapacityFull,
    /// 已失效条目被继续使用
    StaleEntry,
    /// 声明状态与登记状态不符
    StateMismatch,
    /// 冻结清单引用了 FakePath 依赖（桩逃逸进冻结面）
    FrozenFaceRef,
    /// Unfinalized 上游被硬依赖（挂起等待，EAGAIN 语义）
    UnfinalizedPending,
    /// FakePath 依赖未携带桩声明
    StubMissing,
    /// 桩指纹复算不符
    StubDigestBad,
    /// 登记状态机非法倒退/越级
    IllegalTransition,
    /// 依赖账守恒被破坏
    LedgerRegress,
    /// 双跑不一致
    NotReproducible,
    /// 段位越界
    SegmentViolation,
}

impl DepCode {
    /// 码值（段基址 + 变体序号）。
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }

    /// 中文诊断与可操作建议（闭集返回，禁止空串）。
    pub fn detail(self) -> &'static str {
        match self {
            DepCode::BadName => "依赖名非法被拒：名字为空或超过 24 字节上限；\
                请给依赖一个非空且 ≤ MAX_NAME_BYTES 的稳定短名",
            DepCode::NegativeCard => "负上游卡号被拒：卡号必须为非负整数；\
                请核对登记来源卡，负值一律回 EINVAL",
            DepCode::NotRegistered => "未登记硬依赖被拒：登记表中查无此名；\
                C3 联调面禁止隐式依赖，请先走登记协议再声明",
            DepCode::DuplicateReg => "重复登记被拒：同名依赖条目已存在；\
                请改用 confirm/invalidate 推进既有条目，不要重开一格",
            DepCode::CapacityFull => "登记表容量耗尽：条目数已达 MAX_DEPS；\
                表满按 ENOMEM 计入拒绝桶，请先失效回收再登记",
            DepCode::StaleEntry => "已失效条目被拒：条目处于 Invalidated 态；\
                失效即终态，请重新登记新条目而非复活旧格",
            DepCode::StateMismatch => "声明状态与登记状态不符：判定入参三态与\
                登记条目三态不一致；请以登记表为准修正声明侧状态",
            DepCode::FrozenFaceRef => "冻结面引用 FakePath 被拒：桩不得逃逸进\
                冻结清单；请把该依赖上游卡 finalize 后再入冻结面，回 EROFS",
            DepCode::UnfinalizedPending => "Unfinalized 上游被硬依赖：按 §52 \
                防卡死规则一挂起等待（EAGAIN 语义），显式登记不装就绪、\
                不死等自旋",
            DepCode::StubMissing => "FakePath 依赖无桩声明被拒：fake 开发路径\
                必须显式带桩；请补 stub_declared 声明，否则回 EPERM",
            DepCode::StubDigestBad => "桩指纹复算不符：登记的 declared digest \
                与 stub_digest(name, card) 复算值不一致；请重新计算并登记，\
                不得手抄旧指纹",
            DepCode::IllegalTransition => "状态机非法倒退/越级被拒：确认与失效\
                只能沿 登记→确认→失效 单向推进；重复确认、复活失效格均回 \
                EINVAL",
            DepCode::LedgerRegress => "依赖账守恒被破坏：登记数必须等于确认数+\
                挂起数+拒绝数；请检查是否有判定结果漏记或多记",
            DepCode::NotReproducible => "双跑不一致：同声明语料两次指纹不同；\
                请排查隐藏可变状态或非确定哈希",
            DepCode::SegmentViolation => "段位越界：本模块段基址必须落在 \
                0x7A000..=0x7D1C0 且 0x40 对齐",
        }
    }

    /// 闭集全量（供文案完整性与唯一性检查）。
    pub const ALL: [DepCode; 15] = [
        DepCode::BadName,
        DepCode::NegativeCard,
        DepCode::NotRegistered,
        DepCode::DuplicateReg,
        DepCode::CapacityFull,
        DepCode::StaleEntry,
        DepCode::StateMismatch,
        DepCode::FrozenFaceRef,
        DepCode::UnfinalizedPending,
        DepCode::StubMissing,
        DepCode::StubDigestBad,
        DepCode::IllegalTransition,
        DepCode::LedgerRegress,
        DepCode::NotReproducible,
        DepCode::SegmentViolation,
    ];
}

// ── 依赖条目域模型 ────────────────────────────────────────────────────
/// 上游依赖三态闭集：登记协议只认这三种，此外无第四态。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DepState {
    /// 上游卡已收口（finalize 完成）
    Finalized,
    /// 上游卡未 finalize（C3 联调面必须显式登记并挂起等待）
    Unfinalized,
    /// 开发桩路径（必须带可复算桩指纹，且不得进冻结面）
    FakePath,
}

impl DepState {
    /// 三态闭集全量。
    pub const ALL: [DepState; 3] = [
        DepState::Finalized,
        DepState::Unfinalized,
        DepState::FakePath,
    ];

    /// 稳定编码（指纹用；不得随版本漂移）。
    pub fn as_u8(self) -> u8 {
        match self {
            DepState::Finalized => 1,
            DepState::Unfinalized => 2,
            DepState::FakePath => 3,
        }
    }

    /// 中文标签（闭集，非空）。
    pub fn label(self) -> &'static str {
        match self {
            DepState::Finalized => "已收口",
            DepState::Unfinalized => "未finalize",
            DepState::FakePath => "开发桩",
        }
    }
}

/// 登记表条目生命周期相位：登记 → 确认 → 失效，单向推进不许倒退。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Phase {
    /// 已登记待确认
    Registered,
    /// 已确认（联调面已对齐）
    Confirmed,
    /// 已失效（终态；复活按非法倒退拒绝）
    Invalidated,
}

impl Phase {
    /// 稳定编码（指纹用）。
    pub fn as_u8(self) -> u8 {
        match self {
            Phase::Registered => 1,
            Phase::Confirmed => 2,
            Phase::Invalidated => 3,
        }
    }
}

/// 单个依赖条目。名字与卡号登记后不可变；相位与代际由状态机推进。
#[derive(Clone, Debug)]
pub struct DepEntry {
    /// 依赖名（登记表内唯一键）
    pub name: String,
    /// 上游卡号（非负）
    pub card: i32,
    /// 上游三态（登记时定，判定时与声明入参比对）
    pub state: DepState,
    /// 生命周期相位
    pub phase: Phase,
    /// 是否显式携带开发桩声明（FakePath 必须为 true 才可通过）
    pub stub_declared: bool,
    /// 登记时声明的桩指纹（必须等于 stub_digest 复算值）
    pub declared_digest: u32,
    /// 代际：每次失效自增（只增）
    pub rev: u32,
}

/// 登记表事件类别（append-only 日志的记号）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum EventKind {
    /// 登记成功
    Registered,
    /// 确认成功
    Confirmed,
    /// 失效成功
    Invalidated,
    /// 被拒绝（登记/确认/失效任一违约）
    Rejected,
}

impl EventKind {
    /// 稳定编码（指纹用）。
    pub fn as_u8(self) -> u8 {
        match self {
            EventKind::Registered => 1,
            EventKind::Confirmed => 2,
            EventKind::Invalidated => 3,
            EventKind::Rejected => 4,
        }
    }
}

/// 一笔登记表事件。日志只增不减，序号单调。
#[derive(Clone, Debug)]
pub struct RegEvent {
    /// 单调序号（从 1 起）
    pub seq: u32,
    /// 事件类别
    pub kind: EventKind,
    /// 涉及的依赖名
    pub name: String,
}

// ── 登记表状态机 ──────────────────────────────────────────────────────
/// 上游依赖登记表：条目向量 + append-only 事件日志。
/// 没有任何删除接口——「只增不减」是结构保证，不靠纪律。
#[derive(Clone, Debug)]
pub struct DepRegistry {
    entries: Vec<DepEntry>,
    log: Vec<RegEvent>,
    seq: u32,
}

impl DepRegistry {
    /// 构造空登记表。
    pub fn new() -> DepRegistry {
        DepRegistry { entries: Vec::new(), log: Vec::new(), seq: 0 }
    }

    /// 按名查条目（只读视图）。
    pub fn entry(&self, name: &str) -> Option<&DepEntry> {
        let mut i = 0usize;
        while i < self.entries.len() {
            let hit = match self.entries.get(i) {
                Some(e) => e.name == name,
                None => false,
            };
            if hit {
                return self.entries.get(i);
            }
            i += 1;
        }
        None
    }

    /// 按名查条目下标（供可变更新用）。
    fn entry_index(&self, name: &str) -> Option<usize> {
        let mut i = 0usize;
        while i < self.entries.len() {
            let hit = match self.entries.get(i) {
                Some(e) => e.name == name,
                None => false,
            };
            if hit {
                return Some(i);
            }
            i += 1;
        }
        None
    }

    /// 追加一笔事件（日志只增不减；序号饱和单调）。
    fn push_log(&mut self, kind: EventKind, name: &str) {
        self.seq = self.seq.saturating_add(1);
        let ev = RegEvent { seq: self.seq, kind, name: name.to_string() };
        self.log.push(ev);
    }

    /// 登记准入校验：名合法 → 卡号非负 → 无重名 → 容量未满。
    fn admit(&self, name: &str, card: i32) -> Result<(), i32> {
        if name.is_empty() || name.len() > MAX_NAME_BYTES {
            return Err(EINVAL);
        }
        if card < 0 {
            return Err(EINVAL);
        }
        if self.entry(name).is_some() {
            return Err(EEXIST);
        }
        if self.entries.len() >= MAX_DEPS {
            return Err(ENOMEM);
        }
        Ok(())
    }

    /// 登记：准入通过则落条目（相位 Registered）并记日志；
    /// 任一违约记 Rejected 事件后回错。拒绝也入账，不静默。
    pub fn register(
        &mut self,
        name: &str,
        card: i32,
        state: DepState,
        stub_declared: bool,
        declared_digest: u32,
    ) -> Result<(), i32> {
        let verdict = self.admit(name, card);
        match verdict {
            Err(code) => {
                self.push_log(EventKind::Rejected, name);
                Err(code)
            }
            Ok(()) => {
                let entry = DepEntry {
                    name: name.to_string(),
                    card,
                    state,
                    phase: Phase::Registered,
                    stub_declared,
                    declared_digest,
                    rev: 0,
                };
                self.entries.push(entry);
                self.push_log(EventKind::Registered, name);
                Ok(())
            }
        }
    }

    /// 确认：仅 Registered → Confirmed。重复确认、已失效再确认、
    /// 未登记条目直接确认均按违约拒绝并入账。
    pub fn confirm(&mut self, name: &str) -> Result<(), i32> {
        let verdict = match self.entry(name) {
            None => Err(ENOENT),
            Some(e) => {
                if e.phase == Phase::Registered {
                    Ok(())
                } else {
                    Err(EINVAL)
                }
            }
        };
        match verdict {
            Err(code) => {
                self.push_log(EventKind::Rejected, name);
                Err(code)
            }
            Ok(()) => {
                if let Some(idx) = self.entry_index(name) {
                    if let Some(e) = self.entries.get_mut(idx) {
                        e.phase = Phase::Confirmed;
                    }
                }
                self.push_log(EventKind::Confirmed, name);
                Ok(())
            }
        }
    }

    /// 失效：Registered/Confirmed → Invalidated，代际自增并回新代际。
    /// 已失效条目重复失效按倒退拒绝；未登记条目回 ENOENT。
    pub fn invalidate(&mut self, name: &str) -> Result<u32, i32> {
        let verdict = match self.entry(name) {
            None => Err(ENOENT),
            Some(e) => {
                if e.phase == Phase::Invalidated {
                    Err(EINVAL)
                } else {
                    Ok(())
                }
            }
        };
        match verdict {
            Err(code) => {
                self.push_log(EventKind::Rejected, name);
                Err(code)
            }
            Ok(()) => {
                let mut new_rev = 0u32;
                if let Some(idx) = self.entry_index(name) {
                    if let Some(e) = self.entries.get_mut(idx) {
                        e.phase = Phase::Invalidated;
                        e.rev = e.rev.saturating_add(1);
                        new_rev = e.rev;
                    }
                }
                self.push_log(EventKind::Invalidated, name);
                Ok(new_rev)
            }
        }
    }

    /// 条目数。
    pub fn entry_count(&self) -> usize {
        self.entries.len()
    }

    /// 事件日志笔数（只增不减）。
    pub fn log_len(&self) -> usize {
        self.log.len()
    }

    /// 事件序号（单调）。
    pub fn seq(&self) -> u32 {
        self.seq
    }

    /// 条目只读视图（指纹用；不暴露内部 Vec）。
    pub fn entry_view(&self, idx: usize) -> Option<(&str, u8, u8, u32)> {
        match self.entries.get(idx) {
            Some(e) => Some((e.name.as_str(), e.state.as_u8(), e.phase.as_u8(), e.rev)),
            None => None,
        }
    }

    /// 事件只读视图（指纹用）。
    pub fn log_view(&self, idx: usize) -> Option<u8> {
        match self.log.get(idx) {
            Some(ev) => Some(ev.kind.as_u8()),
            None => None,
        }
    }
}

// ── 冻结清单 ──────────────────────────────────────────────────────────
/// 冻结清单（冻结面）：已收口进冻结镜像的依赖名闭集。
/// FakePath 依赖的名字落在这份清单内即视为「桩逃逸进冻结面」。
pub const FROZEN_MANIFEST: [&str; 4] = ["sched_ticks", "mm_core", "clk_if", "fs_vfs"];

/// 名字是否在冻结清单内。
pub fn in_frozen_manifest(name: &str) -> bool {
    let mut i = 0usize;
    while i < FROZEN_MANIFEST.len() {
        if let Some(n) = FROZEN_MANIFEST.get(i) {
            if *n == name {
                return true;
            }
        }
        i += 1;
    }
    false
}

// ── 确定性哈希与桩指纹 ────────────────────────────────────────────────
/// FNV-1a 32 位（无浮点、无随机、无墙钟）。
pub fn fnv1a(bytes: &[u8]) -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    let mut i = 0usize;
    while i < bytes.len() {
        if let Some(b) = bytes.get(i) {
            h ^= *b as u32;
            h = h.wrapping_mul(0x0100_0193);
        }
        i += 1;
    }
    h
}

/// 固定 LCG：确定性伪随机（禁真随机，判据允许固定 LCG）。
pub fn lcg_next(state: u32) -> u32 {
    state.wrapping_mul(LCG_A).wrapping_add(LCG_C)
}

/// 桩指纹：对 名字 | 卡号 LE4 | "stub" 做 FNV-1a。
/// 「可复算」指：任何持有同名同卡号的一方都能重算出同一指纹。
pub fn stub_digest(name: &str, card: i32) -> u32 {
    let mut bytes: Vec<u8> = Vec::new();
    let nb = name.as_bytes();
    let mut i = 0usize;
    while i < nb.len() {
        if let Some(b) = nb.get(i) {
            bytes.push(*b);
        }
        i += 1;
    }
    bytes.push(b'|');
    let c = card as u32;
    bytes.push((c & 0xFF) as u8);
    bytes.push(((c >> 8) & 0xFF) as u8);
    bytes.push(((c >> 16) & 0xFF) as u8);
    bytes.push(((c >> 24) & 0xFF) as u8);
    bytes.push(b's');
    bytes.push(b't');
    bytes.push(b'u');
    bytes.push(b'b');
    fnv1a(&bytes)
}

/// 桩指纹校验：登记的 declared 是否等于复算值。
pub fn valid_stub(name: &str, card: i32, declared: u32) -> bool {
    declared == stub_digest(name, card)
}

// ── 依赖声明判定器 ────────────────────────────────────────────────────
/// 依赖声明判定（判定次序即契约，逐对锁定在测试里）：
///   ① 名非法（空/超上限）        → EINVAL
///   ② 未登记硬依赖               → ENOENT
///   ③ 条目已失效                 → ESTALE
///   ④ 声明三态 ≠ 登记三态        → EINVAL
///   ⑤ 冻结清单引用 FakePath      → EROFS（桩不得逃逸进冻结面）
///   ⑥ 状态语义：Unfinalized      → EAGAIN（挂起等待，§52 防卡死规则一）
///              FakePath 无桩声明 → EPERM
///              FakePath 指纹复算不符 → EPERM
///              Finalized          → Ok
pub fn judge_dep(reg: &DepRegistry, name: &str, state: DepState) -> Result<(), i32> {
    if name.is_empty() || name.len() > MAX_NAME_BYTES {
        return Err(EINVAL);
    }
    let entry = match reg.entry(name) {
        Some(e) => e,
        None => return Err(ENOENT),
    };
    if entry.phase == Phase::Invalidated {
        return Err(ESTALE);
    }
    if entry.state != state {
        return Err(EINVAL);
    }
    if entry.state == DepState::FakePath && in_frozen_manifest(name) {
        return Err(EROFS);
    }
    match entry.state {
        DepState::Unfinalized => Err(EAGAIN),
        DepState::FakePath => {
            if !entry.stub_declared {
                return Err(EPERM);
            }
            if entry.declared_digest != stub_digest(name, entry.card) {
                return Err(EPERM);
            }
            Ok(())
        }
        DepState::Finalized => Ok(()),
    }
}

// ── 依赖账（守恒） ────────────────────────────────────────────────────
/// 依赖账：每次声明判定恰好落一个桶。
/// 守恒式：登记数 = 确认数 + 挂起数 + 拒绝数。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct DepLedger {
    /// 登记判定总笔数
    pub declared: u32,
    /// 确认数（Ok：Finalized 正常通过 或 FakePath 带合法桩通过）
    pub confirmed: u32,
    /// 挂起数（EAGAIN：Unfinalized 显式登记等待 finalize）
    pub pending: u32,
    /// 拒绝数（含容量耗尽、重名、无桩、指纹不符、冻结面逃逸等）
    pub rejected: u32,
}

impl DepLedger {
    /// 零账。
    pub const fn zero() -> DepLedger {
        DepLedger { declared: 0, confirmed: 0, pending: 0, rejected: 0 }
    }

    /// 记一笔判定结果（恰好落一桶）。
    pub fn record(&mut self, outcome: &Result<(), i32>) {
        self.declared = self.declared.saturating_add(1);
        match *outcome {
            Ok(()) => {
                self.confirmed = self.confirmed.saturating_add(1);
            }
            Err(EAGAIN) => {
                self.pending = self.pending.saturating_add(1);
            }
            Err(_) => {
                self.rejected = self.rejected.saturating_add(1);
            }
        }
    }

    /// 守恒式：登记 = 确认 + 挂起 + 拒绝。
    pub fn conserved(&self) -> bool {
        self.confirmed
            .saturating_add(self.pending)
            .saturating_add(self.rejected)
            == self.declared
    }
}

/// 声明判定全流程：登记（或被拒）→ judge → 记账。
/// register 失败时不进 judge，直接按拒绝入账；一笔判定只落一桶。
pub fn declare_dep(
    reg: &mut DepRegistry,
    ledger: &mut DepLedger,
    name: &str,
    card: i32,
    state: DepState,
    stub_declared: bool,
    declared_digest: u32,
) -> Result<(), i32> {
    let outcome = match reg.register(name, card, state, stub_declared, declared_digest) {
        Ok(()) => judge_dep(reg, name, state),
        Err(code) => Err(code),
    };
    ledger.record(&outcome);
    outcome
}

// ── 注入器：三类违约，各 10 轮全抓 ────────────────────────────────────
/// 注入类别闭集。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum InjectClass {
    /// 未登记硬依赖：声明一个登记表里不存在的依赖
    UnregisteredHardDep,
    /// FakePath 无桩：登记了 FakePath 却不带桩声明
    FakePathNoStub,
    /// 冻结面引用 FakePath：桩指纹完全合法但名字落在冻结清单
    FrozenFaceFakePath,
}

impl InjectClass {
    /// 三类闭集全量。
    pub const ALL: [InjectClass; 3] = [
        InjectClass::UnregisteredHardDep,
        InjectClass::FakePathNoStub,
        InjectClass::FrozenFaceFakePath,
    ];
}

/// 注入账：注入 = 命中 + 逃逸，逃逸必须恒为 0。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct InjectLedger {
    /// 注入总次数
    pub injected: u32,
    /// 命中（被判据抓到）次数
    pub caught: u32,
    /// 逃逸次数（必须为 0）
    pub escaped: u32,
}

impl InjectLedger {
    /// 零账。
    pub const fn zero() -> InjectLedger {
        InjectLedger { injected: 0, caught: 0, escaped: 0 }
    }

    /// 记一次注入结果。
    pub fn record(&mut self, caught: bool) {
        self.injected = self.injected.saturating_add(1);
        if caught {
            self.caught = self.caught.saturating_add(1);
        } else {
            self.escaped = self.escaped.saturating_add(1);
        }
    }

    /// 守恒式：注入 = 命中 + 逃逸。
    pub fn conserved(&self) -> bool {
        self.caught.saturating_add(self.escaped) == self.injected
    }
}

/// 执行三类注入矩阵：每类 INJECT_ROUNDS 轮，返回账本与逐类命中数。
/// 每轮用固定 LCG 派生注入样本，确定性可复跑。
pub fn run_injection_matrix() -> (InjectLedger, [u32; 3]) {
    let mut rng: u32 = 0x9376_9376;
    let mut ledger = InjectLedger::zero();
    let mut per_class = [0u32; 3];
    let mut ci = 0usize;
    while ci < InjectClass::ALL.len() {
        let class = match InjectClass::ALL.get(ci).copied() {
            Some(c) => c,
            None => break,
        };
        // 每类独立登记表，先登记两条合法背景条目
        let mut reg = DepRegistry::new();
        let _ = reg.register("dep-final-a", 1, DepState::Finalized, false, 0);
        let _ = reg.register("dep-unfin-b", 2, DepState::Unfinalized, false, 0);
        let mut round = 0u32;
        while round < INJECT_ROUNDS {
            rng = lcg_next(rng);
            let caught = match class {
                InjectClass::UnregisteredHardDep => {
                    // 每轮派生一个登记表里不可能存在的 ghost 名
                    let ghost = format!("ghost-{:04}", rng % 10_000);
                    judge_dep(&reg, &ghost, DepState::Finalized) == Err(ENOENT)
                }
                InjectClass::FakePathNoStub => {
                    let name = format!("fp-nostub-{:02}", round % 100);
                    let card = (rng & 0x7FFF) as i32;
                    let _ = reg.register(&name, card, DepState::FakePath, false, 0);
                    judge_dep(&reg, &name, DepState::FakePath) == Err(EPERM)
                }
                InjectClass::FrozenFaceFakePath => {
                    // 带完全合法可复算的桩，但名字落在冻结清单 → 必须仍被 EROFS 抓住
                    let digest = stub_digest("clk_if", 7);
                    let _ = reg.register("clk_if", 7, DepState::FakePath, true, digest);
                    judge_dep(&reg, "clk_if", DepState::FakePath) == Err(EROFS)
                }
            };
            ledger.record(caught);
            if caught {
                if let Some(slot) = per_class.get_mut(ci) {
                    *slot = slot.saturating_add(1);
                }
            }
            round += 1;
        }
        ci += 1;
    }
    (ledger, per_class)
}

// ── 规范声明语料与双跑指纹 ────────────────────────────────────────────
/// 规范声明语料：固定 12 笔声明 = 3 确认 + 2 挂起 + 7 拒绝（守恒可复算）。
/// 覆盖：Finalized 正常、冻结名 Finalized 正常、fake 带合法桩、
/// Unfinalized 挂起 ×2、空名、负卡号、名超限、无桩、指纹不符、
/// 冻结面逃逸、重名重复登记。
pub fn run_canonical_declare(reg: &mut DepRegistry, ledger: &mut DepLedger) {
    let good_clean = stub_digest("fp_clean", 21);
    let bad_digest = good_clean ^ 0x5A5A_0000;
    let good_frozen = stub_digest("clk_if", 7);
    let long_name = "abcdefghijklmnopqrstuvwxy"; // 25 字节，超 24 上限
    let _ = declare_dep(reg, ledger, "clk_final", 3, DepState::Finalized, false, 0);
    let _ = declare_dep(reg, ledger, "sched_ticks", 4, DepState::Finalized, false, 0);
    let _ = declare_dep(reg, ledger, "fp_clean", 21, DepState::FakePath, true, good_clean);
    let _ = declare_dep(reg, ledger, "dep_late", 5, DepState::Unfinalized, false, 0);
    let _ = declare_dep(reg, ledger, "dep_wait", 6, DepState::Unfinalized, false, 0);
    let _ = declare_dep(reg, ledger, "", 1, DepState::Finalized, false, 0);
    let _ = declare_dep(reg, ledger, "neg_card", -3, DepState::Finalized, false, 0);
    let _ = declare_dep(reg, ledger, long_name, 2, DepState::Finalized, false, 0);
    let _ = declare_dep(reg, ledger, "fp_nostub", 9, DepState::FakePath, false, 0);
    let _ = declare_dep(reg, ledger, "fp_baddigest", 10, DepState::FakePath, true, bad_digest);
    let _ = declare_dep(reg, ledger, "clk_if", 7, DepState::FakePath, true, good_frozen);
    let _ = declare_dep(reg, ledger, "clk_final", 3, DepState::Finalized, false, 0);
}

/// 规范语料逐字节快照：账四桶 + 条目视图 + 事件视图。
pub fn declare_corpus_bytes() -> Vec<u8> {
    let mut reg = DepRegistry::new();
    let mut ledger = DepLedger::zero();
    run_canonical_declare(&mut reg, &mut ledger);
    let mut bytes: Vec<u8> = Vec::new();
    bytes.push((ledger.declared & 0xFF) as u8);
    bytes.push((ledger.confirmed & 0xFF) as u8);
    bytes.push((ledger.pending & 0xFF) as u8);
    bytes.push((ledger.rejected & 0xFF) as u8);
    bytes.push(reg.entry_count() as u8);
    bytes.push(reg.log_len() as u8);
    let mut i = 0usize;
    while i < reg.entry_count() {
        match reg.entry_view(i) {
            Some((nm, st, ph, rev)) => {
                let nb = nm.as_bytes();
                let mut j = 0usize;
                while j < nb.len() {
                    if let Some(b) = nb.get(j) {
                        bytes.push(*b);
                    }
                    j += 1;
                }
                bytes.push(0x00);
                bytes.push(st);
                bytes.push(ph);
                bytes.push((rev & 0xFF) as u8);
            }
            None => {}
        }
        i += 1;
    }
    let mut k = 0usize;
    while k < reg.log_len() {
        match reg.log_view(k) {
            Some(kind) => bytes.push(kind),
            None => {}
        }
        k += 1;
    }
    bytes
}

/// 规范语料指纹（双跑一致判据的基准值来源）。
pub fn declare_corpus_fingerprint() -> u32 {
    fnv1a(&declare_corpus_bytes())
}

// ── 检查入口 ──────────────────────────────────────────────────────────
/// F9376 判据集（≥14 条 s.add；动态明细 ≤8 条 s.add_dyn）。
pub fn run_unx9376_checks() -> CheckSet {
    let mut s = CheckSet::new("unx9376");

    // 1. 段位守卫
    let seg_ok = SEG_BASE % 0x40 == 0 && (0x7A000..=0x7D1C0).contains(&SEG_BASE);
    s.add("段位守卫：0x40 对齐且在 0x7A000..=0x7D1C0", seg_ok,
          "段基址必须唯一且落在本批窗口；越界即拒绝注册");

    // 2. 正常流：Finalized 上游声明直接确认
    let mut reg_a = DepRegistry::new();
    let mut led_a = DepLedger::zero();
    let r1 = declare_dep(&mut reg_a, &mut led_a, "clk_final", 3, DepState::Finalized, false, 0);
    s.add("正常流：Finalized 上游声明直接确认", r1 == Ok(()),
          "已收口上游不应被挂起也不应被拒，直接 Ok 入确认桶");

    // 3. 正常流：fake 开发路径（带可复算桩）通过
    let mut reg_b = DepRegistry::new();
    let mut led_b = DepLedger::zero();
    let dg = stub_digest("fp_clean", 21);
    let r2 = declare_dep(&mut reg_b, &mut led_b, "fp_clean", 21, DepState::FakePath, true, dg);
    s.add("正常流：FakePath 带可复算桩的开发路径通过", r2 == Ok(()),
          "fake 开发路径格：显式带桩且指纹可复算即放行");

    // 4. C3 上游未 finalize 显式登记格：Unfinalized → EAGAIN 挂起等待
    let r3 = declare_dep(&mut reg_b, &mut led_b, "dep_late", 5, DepState::Unfinalized, false, 0);
    s.add("未 finalize 显式登记：Unfinalized 被硬依赖 → EAGAIN 挂起", r3 == Err(EAGAIN),
          "§52 防卡死规则一：显式登记并挂起等待，不装就绪也不死等自旋");

    // 5. 边界-零：空依赖名
    let r4 = declare_dep(&mut reg_b, &mut led_b, "", 1, DepState::Finalized, false, 0);
    s.add("边界-零：空依赖名 → EINVAL", r4 == Err(EINVAL),
          "零长名字不进登记表；判定器对空名同样先拒");

    // 6. 边界-负：负上游卡号
    let r5 = declare_dep(&mut reg_b, &mut led_b, "neg_card", -1, DepState::Finalized, false, 0);
    s.add("边界-负：负上游卡号 → EINVAL", r5 == Err(EINVAL),
          "卡号为负属非法来源，登记与判定两侧均拒");

    // 7. 边界-上限：名长恰为 24 字节可登记
    let name24 = "abcdefghijklmnopqrstuvwx";
    let r6 = declare_dep(&mut reg_b, &mut led_b, name24, 30, DepState::Finalized, false, 0);
    s.add("边界-上限：名长恰为 24 字节可登记", r6 == Ok(()),
          "名字字节上限含 24；恰好压线必须放行");

    // 8. 边界-上限+1：名长 25 字节拒绝
    let name25 = "abcdefghijklmnopqrstuvwxy";
    let r7 = declare_dep(&mut reg_b, &mut led_b, name25, 31, DepState::Finalized, false, 0);
    s.add("边界-上限+1：名长 25 字节 → EINVAL", r7 == Err(EINVAL),
          "超一字节即拒，边界必须闭合不留缝");

    // 9. 注入三类各 10 轮全抓
    let (iledger, per_class) = run_injection_matrix();
    let all_caught = per_class.iter().all(|n| *n == INJECT_ROUNDS);
    s.add("注入器三类（未登记/无桩/冻结面）各 10/10 全抓", all_caught,
          "三类违约 30 轮必须全部命中，0 逃逸");

    // 10. 注入账守恒
    s.add("注入账守恒：注入=命中+逃逸且逃逸=0",
          iledger.conserved() && iledger.escaped == 0
              && iledger.injected == INJECT_CLASSES * INJECT_ROUNDS,
          "30 次注入必须 30 次命中、0 逃逸");

    // 11. 判定次序：ENOENT > EROFS、EROFS > EPERM、ESTALE > 不符
    let mut reg_c = DepRegistry::new();
    let o1 = judge_dep(&reg_c, "clk_if", DepState::FakePath);
    let _ = reg_c.register("mm_core", 8, DepState::FakePath, false, 0);
    let o2 = judge_dep(&reg_c, "mm_core", DepState::FakePath);
    let _ = reg_c.register("dep_gone", 9, DepState::Unfinalized, false, 0);
    let _ = reg_c.invalidate("dep_gone");
    let o3 = judge_dep(&reg_c, "dep_gone", DepState::Finalized);
    s.add("判定次序：ENOENT>EROFS、EROFS>EPERM、ESTALE>不符",
          o1 == Err(ENOENT) && o2 == Err(EROFS) && o3 == Err(ESTALE),
          "次序即契约：未登记先于冻结面、冻结面先于无桩、失效先于状态不符");

    // 12. 声明三态与登记三态不符 → EINVAL
    let o4 = judge_dep(&reg_c, "mm_core", DepState::Finalized);
    s.add("声明不符：FakePath 条目按 Finalized 声明 → EINVAL", o4 == Err(EINVAL),
          "判定入参三态必须与登记条目一致，防止声明侧撒谎");

    // 13. 桩指纹可复算：篡改指纹 EPERM，合法指纹可复算
    let mut reg_d = DepRegistry::new();
    let good = stub_digest("fp_t", 12);
    let _ = reg_d.register("fp_t", 12, DepState::FakePath, true, good ^ 1);
    let o5 = judge_dep(&reg_d, "fp_t", DepState::FakePath);
    s.add("桩指纹：登记值与复算值不符 → EPERM",
          o5 == Err(EPERM) && valid_stub("fp_t", 12, good),
          "指纹必须可复算：任何一方都能用名字+卡号重算同一指纹");

    // 14. 状态机：合法链 + 非法倒退/越级全部被拒
    let mut reg_e = DepRegistry::new();
    let c1 = reg_e.register("dep_x", 1, DepState::Finalized, false, 0);
    let c2 = reg_e.confirm("dep_x");
    let c3 = reg_e.invalidate("dep_x");
    let c4 = reg_e.confirm("dep_x");
    let c5 = reg_e.invalidate("dep_x");
    let c6 = reg_e.confirm("dep_missing");
    let c7 = reg_e.invalidate("dep_missing");
    s.add("状态机：登记→确认→失效合法，倒退/越级全拒",
          c1.is_ok() && c2.is_ok() && c3.is_ok()
              && c4 == Err(EINVAL) && c5 == Err(EINVAL)
              && c6 == Err(ENOENT) && c7 == Err(ENOENT),
          "失效即终态；重复确认、复活失效格、未登记直接确认/失效均拒");

    // 15. 事件日志只增不减：拒绝也入账且序号单调
    let before_log = reg_e.log_len();
    let before_seq = reg_e.seq();
    let _ = reg_e.confirm("dep_x");
    s.add("事件日志只增不减：拒绝事件也入账且序号单调",
          reg_e.log_len() == before_log + 1 && reg_e.seq() == before_seq + 1,
          "append-only 日志不得丢事件，拒绝同样落账");

    // 16. 依赖账守恒：规范语料 12 笔 = 3 确认 + 2 挂起 + 7 拒绝
    let mut reg_f = DepRegistry::new();
    let mut dledger = DepLedger::zero();
    run_canonical_declare(&mut reg_f, &mut dledger);
    s.add("依赖账守恒：登记=确认+挂起+拒绝（12=3+2+7）",
          dledger.conserved() && dledger.declared == 12
              && dledger.confirmed == 3 && dledger.pending == 2 && dledger.rejected == 7,
          "每笔声明判定恰好落一个桶，账不丢不重");

    // 17. 容量耗尽：表满后登记 → ENOMEM 且账不丢
    let mut reg_g = DepRegistry::new();
    let mut led_g = DepLedger::zero();
    let mut i = 0usize;
    while i < MAX_DEPS {
        let nm = format!("cap-{:02}", i);
        let _ = declare_dep(&mut reg_g, &mut led_g, &nm, i as i32, DepState::Finalized, false, 0);
        i += 1;
    }
    let r8 = declare_dep(&mut reg_g, &mut led_g, "cap-over", 99, DepState::Finalized, false, 0);
    s.add("容量耗尽：表满后登记 → ENOMEM 且账不丢",
          r8 == Err(ENOMEM) && reg_g.entry_count() == MAX_DEPS
              && led_g.confirmed == MAX_DEPS as u32 && led_g.rejected == 1 && led_g.conserved(),
          "表满按 ENOMEM 计入拒绝桶，账面闭合不丢笔");

    // 18. 双跑一致：规范语料指纹相等
    let f1 = declare_corpus_fingerprint();
    let f2 = declare_corpus_fingerprint();
    s.add("双跑一致：规范声明语料指纹相等", f1 == f2,
          "同输入两次必须逐字节相等，隐藏可变状态即失败");

    // 19. 文案闭集：15 个诊断码均有非空建议
    let text_ok = DepCode::ALL.iter().all(|c| !c.detail().is_empty());
    s.add("文案闭集：全部诊断码带非空中文建议", text_ok,
          "每个码必须能取到非空且可操作的说明");

    // 20. 诊断码唯一
    let mut uniq = true;
    let mut pi = 0usize;
    while pi < DepCode::ALL.len() {
        let mut pj = pi + 1;
        while pj < DepCode::ALL.len() {
            if let (Some(a), Some(b)) = (DepCode::ALL.get(pi), DepCode::ALL.get(pj)) {
                if a.val() == b.val() {
                    uniq = false;
                }
            }
            pj += 1;
        }
        pi += 1;
    }
    s.add("诊断码：15 个码值两两唯一", uniq,
          "段内闭集码不得重复，否则判据无法定位");

    // 21. 收口
    let n_before = s.len();
    s.add("收口：判据条数达到声明值", n_before >= 14,
          "本卡声明 ≥14 条判据，实际条数必须达标才允许收口");

    // 22. 注入账单调：追加注入只增不减
    let mut mono = iledger;
    let base = mono.injected;
    mono.record(true);
    mono.record(true);
    s.add("注入账单调：追加命中只增不减", mono.injected == base + 2 && mono.caught == base + 2,
          "累计计数禁止回退或饱和到旧值");

    // 动态明细（≤8）：注入账 / 依赖账 / 指纹 / 日志 四条快照
    let inj_text = format!(
        "注入 {} / 命中 {} / 逃逸 {}",
        iledger.injected, iledger.caught, iledger.escaped
    );
    s.add_dyn("注入账快照", iledger.conserved(), &inj_text);
    let dep_text = format!(
        "登记 {} / 确认 {} / 挂起 {} / 拒绝 {}",
        dledger.declared, dledger.confirmed, dledger.pending, dledger.rejected
    );
    s.add_dyn("依赖账快照", dledger.conserved(), &dep_text);
    let fp_text = format!("声明语料指纹 0x{:08X}", f1);
    s.add_dyn("指纹快照", f1 == f2, &fp_text);
    let log_text = format!(
        "登记表事件 {} 笔 / 条目 {} 项",
        reg_e.log_len(), reg_e.entry_count()
    );
    s.add_dyn("登记表日志快照", reg_e.log_len() >= 7, &log_text);

    s
}

// ── 测试 ──────────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normal_flow_finalized() {
        let mut reg = DepRegistry::new();
        let mut led = DepLedger::zero();
        let r = declare_dep(&mut reg, &mut led, "clk_final", 3, DepState::Finalized, false, 0);
        assert_eq!(r, Ok(()));
        assert_eq!(led.confirmed, 1);
        assert_eq!(led.pending, 0);
        assert!(led.conserved());
    }

    #[test]
    fn fake_path_with_recomputable_stub_passes() {
        let mut reg = DepRegistry::new();
        let d = stub_digest("fp_clean", 21);
        assert!(valid_stub("fp_clean", 21, d));
        let _ = reg.register("fp_clean", 21, DepState::FakePath, true, d);
        assert_eq!(judge_dep(&reg, "fp_clean", DepState::FakePath), Ok(()));
    }

    #[test]
    fn unfinalized_dep_suspends_eagain() {
        let mut reg = DepRegistry::new();
        let _ = reg.register("dep_late", 5, DepState::Unfinalized, false, 0);
        assert_eq!(judge_dep(&reg, "dep_late", DepState::Unfinalized), Err(EAGAIN));
        // 确认只推进生命周期相位，不改变上游未 finalize 的事实，仍挂起
        assert!(reg.confirm("dep_late").is_ok());
        assert_eq!(judge_dep(&reg, "dep_late", DepState::Unfinalized), Err(EAGAIN));
    }

    #[test]
    fn boundary_zero_and_negative() {
        let mut reg = DepRegistry::new();
        assert_eq!(reg.register("", 1, DepState::Finalized, false, 0), Err(EINVAL));
        assert_eq!(reg.register("neg", -1, DepState::Finalized, false, 0), Err(EINVAL));
        assert_eq!(reg.entry_count(), 0);
        assert_eq!(judge_dep(&reg, "", DepState::Finalized), Err(EINVAL));
    }

    #[test]
    fn boundary_length_upper_and_over() {
        let mut reg = DepRegistry::new();
        let n24 = "abcdefghijklmnopqrstuvwx";
        let n25 = "abcdefghijklmnopqrstuvwxy";
        assert_eq!(reg.register(n24, 1, DepState::Finalized, false, 0), Ok(()));
        assert_eq!(reg.register(n25, 2, DepState::Finalized, false, 0), Err(EINVAL));
        assert_eq!(reg.entry_count(), 1);
    }

    #[test]
    fn injection_matrix_three_classes_ten_rounds() {
        let (l, per) = run_injection_matrix();
        assert_eq!(l.injected, INJECT_CLASSES * INJECT_ROUNDS);
        assert_eq!(l.escaped, 0);
        assert!(l.conserved());
        let mut i = 0usize;
        while i < per.len() {
            assert_eq!(per.get(i).copied().unwrap_or(0), INJECT_ROUNDS);
            i += 1;
        }
    }

    #[test]
    fn judge_order_precedence_locked() {
        let mut reg = DepRegistry::new();
        // 未登记的冻结名 → ENOENT（未登记先于冻结面）
        assert_eq!(judge_dep(&reg, "clk_if", DepState::FakePath), Err(ENOENT));
        // 冻结面 FakePath 且无桩 → EROFS（冻结面先于无桩）
        let _ = reg.register("mm_core", 8, DepState::FakePath, false, 0);
        assert_eq!(judge_dep(&reg, "mm_core", DepState::FakePath), Err(EROFS));
        // 已失效 + 声明不符同时成立 → ESTALE（失效先于不符）
        let _ = reg.register("dep_gone", 9, DepState::Unfinalized, false, 0);
        let _ = reg.invalidate("dep_gone");
        assert_eq!(judge_dep(&reg, "dep_gone", DepState::Finalized), Err(ESTALE));
        // 仅声明不符 → EINVAL
        assert_eq!(judge_dep(&reg, "mm_core", DepState::Finalized), Err(EINVAL));
        // 空名最优先 → EINVAL
        assert_eq!(judge_dep(&reg, "", DepState::Finalized), Err(EINVAL));
    }

    #[test]
    fn state_machine_rejects_regression() {
        let mut reg = DepRegistry::new();
        assert!(reg.register("dep_x", 1, DepState::Finalized, false, 0).is_ok());
        assert!(reg.confirm("dep_x").is_ok());
        assert_eq!(reg.confirm("dep_x"), Err(EINVAL));
        assert!(reg.invalidate("dep_x").is_ok());
        assert_eq!(reg.confirm("dep_x"), Err(EINVAL));
        assert_eq!(reg.invalidate("dep_x"), Err(EINVAL));
        assert_eq!(reg.confirm("dep_missing"), Err(ENOENT));
        assert_eq!(reg.invalidate("dep_missing"), Err(ENOENT));
    }

    #[test]
    fn dep_ledger_conserved_canonical() {
        let mut reg = DepRegistry::new();
        let mut led = DepLedger::zero();
        run_canonical_declare(&mut reg, &mut led);
        assert!(led.conserved());
        assert_eq!(led.declared, 12);
        assert_eq!(led.confirmed, 3);
        assert_eq!(led.pending, 2);
        assert_eq!(led.rejected, 7);
    }

    #[test]
    fn capacity_exhaustion_account_kept() {
        let mut reg = DepRegistry::new();
        let mut led = DepLedger::zero();
        let mut i = 0usize;
        while i < MAX_DEPS {
            let nm = format!("cap-{:02}", i);
            let _ = declare_dep(&mut reg, &mut led, &nm, i as i32, DepState::Finalized, false, 0);
            i += 1;
        }
        assert_eq!(reg.entry_count(), MAX_DEPS);
        assert_eq!(
            declare_dep(&mut reg, &mut led, "cap-over", 99, DepState::Finalized, false, 0),
            Err(ENOMEM)
        );
        assert_eq!(reg.entry_count(), MAX_DEPS);
        assert!(led.conserved());
        assert_eq!(led.confirmed, MAX_DEPS as u32);
        assert_eq!(led.rejected, 1);
    }

    #[test]
    fn dual_run_fingerprint_stable() {
        let a = declare_corpus_fingerprint();
        let b = declare_corpus_fingerprint();
        assert_eq!(a, b);
        assert!(a != 0);
    }

    #[test]
    fn segment_guard_window() {
        assert_eq!(SEG_BASE % 0x40, 0);
        assert!((0x7A000..=0x7D1C0).contains(&SEG_BASE));
        assert_eq!(SEG_BASE, 0x7CB80);
        // 诊断码闭集全部落在段窗内
        let last = match DepCode::ALL.last() {
            Some(c) => c.val(),
            None => 0,
        };
        assert!(last < SEG_BASE + 0x40);
    }

    #[test]
    fn stub_digest_tamper_detected() {
        let d = stub_digest("fp_t", 12);
        assert!(valid_stub("fp_t", 12, d));
        assert!(!valid_stub("fp_t", 12, d ^ 1));
        assert!(!valid_stub("fp_t", 13, d));
        let mut reg = DepRegistry::new();
        let _ = reg.register("fp_t", 12, DepState::FakePath, true, d ^ 1);
        assert_eq!(judge_dep(&reg, "fp_t", DepState::FakePath), Err(EPERM));
    }

    #[test]
    fn frozen_face_rejects_fake_path() {
        let mut reg = DepRegistry::new();
        let d = stub_digest("clk_if", 7);
        let _ = reg.register("clk_if", 7, DepState::FakePath, true, d);
        assert_eq!(judge_dep(&reg, "clk_if", DepState::FakePath), Err(EROFS));
        // Finalized 同名依赖不受冻结面影响（冻结面只打 FakePath）
        let mut reg2 = DepRegistry::new();
        let _ = reg2.register("sched_ticks", 4, DepState::Finalized, false, 0);
        assert_eq!(judge_dep(&reg2, "sched_ticks", DepState::Finalized), Ok(()));
    }

    #[test]
    fn state_labels_closed_set() {
        let mut i = 0usize;
        while i < DepState::ALL.len() {
            if let Some(st) = DepState::ALL.get(i) {
                assert!(!st.label().is_empty());
                assert!(st.as_u8() >= 1 && st.as_u8() <= 3);
            }
            i += 1;
        }
    }

    #[test]
    fn duplicate_register_rejected_eexist() {
        let mut reg = DepRegistry::new();
        assert!(reg.register("dup_a", 1, DepState::Finalized, false, 0).is_ok());
        assert_eq!(reg.register("dup_a", 2, DepState::Unfinalized, false, 0), Err(EEXIST));
        assert_eq!(reg.entry_count(), 1);
    }

    #[test]
    fn checks_all_green() {
        let s = run_unx9376_checks();
        assert!(s.all_passed(), "{}", s.red_items_text());
    }
}
