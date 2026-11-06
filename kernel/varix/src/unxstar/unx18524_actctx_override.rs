//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F18524 · ActCtx 覆写探针（10 例） · 0x7FEC0段
//! ─────────────────────────────────────────────────────────────────────
//! 判据锚点：UNX-F18524-J1 10 例覆写场景判定断言全过，撤销态复验一致
//! ─────────────────────────────────────────────────────────────────────
//! 本模块结构清单：
//!   - 数据结构：OverrideEntry（覆写条目：kind/from/to/seq）、
//!     ActCtxEngine（16 槽覆写引擎：槽位账 + 三态状态账 + 操作计数账）、
//!     ScenExpect（10 场景冻结期望裁决表）；
//!   - 状态机：OvrState 三态（Empty→Active→Revoked→Active 重覆写），
//!     迁移闭集 3 条合法、其余 6 组合显式拒；
//!   - 账本：操作账（applies/revokes/rejects）、槽位守恒账
//!     （applies = active_now + revoked_ever）、注入检出账；
//!   - 错误路径：ActxCode 十二码闭集，code()=SEG_BASE+n，中文 detail；
//!   - 注入核：三类违约（Active 重复覆写 / 非 Active 撤销 / 零 tag）
//!     逐类 10 轮断言被显式拒绝，漏检恒零。
//! 设计要点：
//!   - 覆写语义：query(kind, from) 在 Active 时返回覆写目标 to_tag，
//!     撤销后回落基值（None），复验两次必一致（撤销态复验红线）；
//!   - 同 (kind, from_tag) 只允许一条 Active 覆写（重复 apply 显式拒，
//!     不静默覆盖——静默覆盖是 ActCtx 经典缺陷面）；
//!   - seq 单调递增（同槽复活 seq+1，审计可排序）；
//!   - 10 场景期望表冻结：覆写中 query = Some(to_tag)，撤销后 = None，
//!     重覆写后 = Some(to_tag) 且 seq 严格 +1；
//!   - 常量推导：0x7FEC0 = 0x7FE80 + 0x40（A13 批第 4 卡独占窗）；
//!     判据轮数 10 来自 J1「10 例」与注入 10/10 语义；
//!   - no_std + alloc；零 unwrap/expect/panic/裸越界下标/浮点/IO。
//! ═══════════════════════════════════════════════════════════════════════

use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x7FEC0，A13 批第 4 卡独占区，0x40 对齐）。
pub const SEG_BASE: u32 = 0x7FEC0;

// ── 冻结常量 ────────────────────────────────────────────────────
/// 覆写场景数（冻结：10，J1 主锚点）。
pub const SCEN_COUNT: usize = 10;
/// 覆写引擎槽位数（冻结：8）。
pub const SLOT_COUNT: usize = 16;
/// 判据迭代轮数（推导：注入 10/10 语义）。
pub const JUDGE_ROUNDS: u32 = 10;
/// 注入类数（重复覆写/非法撤销/零 tag 三类）。
pub const INJECT_CLASSES: usize = 3;
/// from_tag 基址（场景确定性生成：0x1000 + 场景号）。
pub const FROM_TAG_BASE: u64 = 0x1000;
/// to_tag 基址（场景确定性生成：0x2000 + 场景号×3）。
pub const TO_TAG_BASE: u64 = 0x2000;

// ── FNV-1a 64 位指纹 ────────────────────────────────────────────
/// FNV-1a（覆写账本指纹唯一哈希来源，确定性）。
pub fn fnv1a(bytes: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    let mut i = 0;
    while i < bytes.len() {
        h ^= bytes[i] as u64;
        h = h.wrapping_mul(0x100000001b3);
        i += 1;
    }
    h
}

// ── 覆写类别（四类闭集） ────────────────────────────────────────
/// ActCtx 覆写四类（Manifest 路径 / 程序集重定向 / 窗口类 / ProgId）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum OverrideKind {
    /// 0 · manifest 路径覆写
    ManifestPath,
    /// 1 · 程序集绑定重定向
    AssemblyRedirect,
    /// 2 · 窗口类名覆写
    WindowClass,
    /// 3 · ProgId 覆写
    ProgId,
}

/// 四类全量（场景确定性轮转用）。
pub const ALL_KINDS: [OverrideKind; 4] = [
    OverrideKind::ManifestPath,
    OverrideKind::AssemblyRedirect,
    OverrideKind::WindowClass,
    OverrideKind::ProgId,
];

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ActxCode {
    /// 零 tag（from/to 为 0 非法）
    ZeroTag,
    /// 同 (kind, from) 已有 Active 覆写（重复覆写拒）
    AlreadyActive,
    /// 目标不存在或不处于 Active（非法撤销）
    NotActive,
    /// 槽位满（16 槽上限）
    TableFull,
    /// 状态机非法迁移
    StateIllegalTransit,
    /// 场景号越界（0 或 >10）
    IdOutOfRange,
    /// 槽位守恒破坏（applies ≠ active_now + revoked_ever）
    ConservationBroken,
    /// 双跑不一致
    NotReproducible,
    /// 撤销态复验漂移（两次撤销后查询不一致）
    RevocationDrift,
    /// 场景期望表缺行
    ScenarioMissing,
    /// 槽位丢失（账内条目蒸发）
    SlotLost,
    /// 注入漏检
    InjectMissed,
}

impl ActxCode {
    /// 诊断码 = SEG_BASE + 枚举序（0x7FEC0..0x7FF00）。
    pub fn code(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 中文诊断明细（闭集非空）。
    pub fn detail(self) -> &'static str {
        match self {
            ActxCode::ZeroTag => "零 tag：from/to 为 0 非法",
            ActxCode::AlreadyActive => "重复覆写：同 (kind,from) 已有 Active 条目",
            ActxCode::NotActive => "非法撤销：目标不存在或不处于 Active",
            ActxCode::TableFull => "槽位满：16 槽上限显性拒绝",
            ActxCode::StateIllegalTransit => "状态机非法迁移：越级/回退全拒",
            ActxCode::IdOutOfRange => "场景号越界：合法闭集 1..=10",
            ActxCode::ConservationBroken => "槽位守恒破坏：applies ≠ active + revoked",
            ActxCode::NotReproducible => "双跑不一致：确定性红线",
            ActxCode::RevocationDrift => "撤销态复验漂移：两次查询不一致",
            ActxCode::ScenarioMissing => "场景期望表缺行：sid 不在 10 行闭集",
            ActxCode::SlotLost => "槽位丢失：账内条目蒸发",
            ActxCode::InjectMissed => "注入漏检：检出 < 注入，验收红线",
        }
    }
    /// 全码闭集（12 条）。
    pub const ALL: [ActxCode; 12] = [
        ActxCode::ZeroTag,
        ActxCode::AlreadyActive,
        ActxCode::NotActive,
        ActxCode::TableFull,
        ActxCode::StateIllegalTransit,
        ActxCode::IdOutOfRange,
        ActxCode::ConservationBroken,
        ActxCode::NotReproducible,
        ActxCode::RevocationDrift,
        ActxCode::ScenarioMissing,
        ActxCode::SlotLost,
        ActxCode::InjectMissed,
    ];
}

/// u32 诊断码反查中文明细。
pub fn detail_of(code: u32) -> &'static str {
    let mut i = 0;
    while i < ActxCode::ALL.len() {
        if let Some(c) = ActxCode::ALL.get(i) {
            if c.code() == code {
                return c.detail();
            }
        }
        i += 1;
    }
    "未知诊断码：不在 0x7FEC0 段闭集内"
}

// ── 状态机（三态） ──────────────────────────────────────────────
/// 覆写槽位三态：Empty→Active→Revoked→(Active 复活)。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum OvrState {
    /// 空槽
    Empty,
    /// 覆写生效中
    Active,
    /// 已撤销（条目保留，可复活）
    Revoked,
}

/// 迁移闭集：合法恰 3 条（Empty→Active / Active→Revoked /
/// Revoked→Active），其余 6 组合显式拒。
pub fn can_transit(from: OvrState, to: OvrState) -> bool {
    match (from, to) {
        (OvrState::Empty, OvrState::Active) => true,
        (OvrState::Active, OvrState::Revoked) => true,
        (OvrState::Revoked, OvrState::Active) => true,
        _ => false,
    }
}

// ── 数据结构 ────────────────────────────────────────────────────
/// 覆写条目（kind/from/to/seq 四列，冻结结构）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OverrideEntry {
    pub kind: OverrideKind,
    pub from_tag: u64,
    pub to_tag: u64,
    /// 同槽复活计数（单调递增，审计排序键）。
    pub seq: u32,
}

/// ActCtx 覆写引擎（16 槽 + 三态账 + 操作账）。
#[derive(Clone, Debug)]
pub struct ActCtxEngine {
    /// 槽位条目（SLOT_COUNT 槽，None=无历史）。
    pub slots: Vec<Option<OverrideEntry>>,
    /// 槽位状态（与 slots 同长）。
    pub states: [OvrState; SLOT_COUNT],
    /// 累计 apply 成功次数。
    pub applies: u32,
    /// 累计 revoke 成功次数。
    pub revokes: u32,
    /// 累计显式拒绝次数（操作账，不静默）。
    pub rejects: u32,
    /// 全局 seq 计数器（单调）。
    pub next_seq: u32,
}

impl ActCtxEngine {
    /// 新引擎：8 空槽，操作账归零。
    pub fn new() -> ActCtxEngine {
        ActCtxEngine {
            slots: (0..SLOT_COUNT).map(|_| None).collect(),
            states: [OvrState::Empty; SLOT_COUNT],
            applies: 0,
            revokes: 0,
            rejects: 0,
            next_seq: 1,
        }
    }

    /// apply：空槽插入或 Revoked 槽复活；Active 同键重复 → 显式拒。
    pub fn apply(&mut self, kind: OverrideKind, from_tag: u64, to_tag: u64) -> Result<u32, u32> {
        if from_tag == 0 || to_tag == 0 {
            self.rejects = self.rejects.saturating_add(1);
            return Err(ActxCode::ZeroTag.code());
        }
        // 同键 Active 已存在 → 拒（不静默覆盖）。
        let mut i = 0;
        while i < SLOT_COUNT {
            let st = match self.states.get(i) {
                Some(s) => *s,
                None => return Err(ActxCode::SlotLost.code()),
            };
            if st == OvrState::Active {
                let same = match self.slots.get(i) {
                    Some(Some(e)) => e.kind == kind && e.from_tag == from_tag,
                    _ => false,
                };
                if same {
                    self.rejects = self.rejects.saturating_add(1);
                    return Err(ActxCode::AlreadyActive.code());
                }
            }
            i += 1;
        }
        // 先找 Revoked 同键复活槽（复活优先于占新槽——否则撤销态条目
        // 永久占槽，10 场景轮转必撑爆槽位表）。
        let mut target: Option<usize> = None;
        let mut find2 = 0;
        while find2 < SLOT_COUNT {
            let st = match self.states.get(find2) {
                Some(s) => *s,
                None => break,
            };
            if st == OvrState::Revoked {
                let same = match self.slots.get(find2) {
                    Some(Some(e)) => e.kind == kind && e.from_tag == from_tag,
                    _ => false,
                };
                if same {
                    target = Some(find2);
                    break;
                }
            }
            find2 += 1;
        }
        // 无复活槽 → 占空槽。
        if target.is_none() {
            let mut find = 0;
            while find < SLOT_COUNT {
                let st = match self.states.get(find) {
                    Some(s) => *s,
                    None => break,
                };
                if st == OvrState::Empty {
                    target = Some(find);
                    break;
                }
                find += 1;
            }
        }
        let slot = match target {
            Some(s) => s,
            None => {
                self.rejects = self.rejects.saturating_add(1);
                return Err(ActxCode::TableFull.code());
            }
        };
        // 迁移闸：Empty→Active 或 Revoked→Active。
        let cur = match self.states.get(slot) {
            Some(s) => *s,
            None => return Err(ActxCode::SlotLost.code()),
        };
        if !can_transit(cur, OvrState::Active) {
            self.rejects = self.rejects.saturating_add(1);
            return Err(ActxCode::StateIllegalTransit.code());
        }
        let seq = self.next_seq;
        self.next_seq = self.next_seq.saturating_add(1);
        let entry = OverrideEntry { kind, from_tag, to_tag, seq };
        if let Some(cell) = self.slots.get_mut(slot) {
            *cell = Some(entry);
        }
        if let Some(st) = self.states.get_mut(slot) {
            *st = OvrState::Active;
        }
        self.applies = self.applies.saturating_add(1);
        Ok(seq)
    }

    /// revoke：Active 条目 → Revoked；目标缺失/非 Active → 显式拒。
    pub fn revoke(&mut self, kind: OverrideKind, from_tag: u64) -> Result<u32, u32> {
        let mut target: Option<usize> = None;
        let mut i = 0;
        while i < SLOT_COUNT {
            let st = match self.states.get(i) {
                Some(s) => *s,
                None => break,
            };
            if st == OvrState::Active {
                let same = match self.slots.get(i) {
                    Some(Some(e)) => e.kind == kind && e.from_tag == from_tag,
                    _ => false,
                };
                if same {
                    target = Some(i);
                    break;
                }
            }
            i += 1;
        }
        let slot = match target {
            Some(s) => s,
            None => {
                self.rejects = self.rejects.saturating_add(1);
                return Err(ActxCode::NotActive.code());
            }
        };
        if !can_transit(OvrState::Active, OvrState::Revoked) {
            return Err(ActxCode::StateIllegalTransit.code());
        }
        let seq = match self.slots.get(slot) {
            Some(Some(e)) => e.seq,
            _ => return Err(ActxCode::SlotLost.code()),
        };
        if let Some(st) = self.states.get_mut(slot) {
            *st = OvrState::Revoked;
        }
        self.revokes = self.revokes.saturating_add(1);
        Ok(seq)
    }

    /// query：Active → Some(to_tag)；撤销/未覆写 → None（回落基值）。
    pub fn query(&self, kind: OverrideKind, from_tag: u64) -> Option<u64> {
        let mut i = 0;
        while i < SLOT_COUNT {
            let st = match self.states.get(i) {
                Some(s) => *s,
                None => return None,
            };
            if st == OvrState::Active {
                if let Some(Some(e)) = self.slots.get(i) {
                    if e.kind == kind && e.from_tag == from_tag {
                        return Some(e.to_tag);
                    }
                }
            }
            i += 1;
        }
        None
    }

    /// 当前 Active 条目数。
    pub fn active_now(&self) -> u32 {
        let mut n = 0u32;
        let mut i = 0;
        while i < SLOT_COUNT {
            if let Some(s) = self.states.get(i) {
                if *s == OvrState::Active {
                    n = n.saturating_add(1);
                }
            }
            i += 1;
        }
        n
    }

    /// 槽位守恒：applies == active_now + revokes（revokes 即历史撤销总数，
    /// 每条 Active 都来自一次 apply 且未被撤销，撤销后不在 active 计）。
    pub fn conserve(&self) -> bool {
        self.applies == self.active_now().saturating_add(self.revokes)
    }

    /// 引擎账本指纹（slots+states+计数拼 FNV，往返判据用）。
    pub fn engine_fnv(&self) -> u64 {
        let mut bytes: Vec<u8> = Vec::new();
        let mut i = 0;
        while i < SLOT_COUNT {
            let st = match self.states.get(i) {
                Some(s) => *s,
                None => OvrState::Empty,
            };
            let code = match st {
                OvrState::Empty => 0u8,
                OvrState::Active => 1,
                OvrState::Revoked => 2,
            };
            bytes.push(code);
            if let Some(Some(e)) = self.slots.get(i) {
                bytes.push(e.kind as u8);
                let mut t = e.from_tag;
                let mut sh = 0;
                while sh < 64 {
                    bytes.push(((t >> sh) & 0xFF) as u8);
                    sh += 8;
                }
                t = e.to_tag;
                sh = 0;
                while sh < 64 {
                    bytes.push(((t >> sh) & 0xFF) as u8);
                    sh += 8;
                }
            }
            i += 1;
        }
        bytes.push((self.applies & 0xFF) as u8);
        bytes.push((self.revokes & 0xFF) as u8);
        bytes.push((self.next_seq & 0xFF) as u8);
        fnv1a(&bytes)
    }
}

// ── 场景期望裁决表（10 行冻结） ─────────────────────────────────
/// 场景期望（sid, 覆写中 query, 撤销后 query, 复活 seq 增量）。
///
/// 期望推导：sid 1..10 轮转四类 kind；from = 0x1000+sid；
/// to = 0x2000+sid×3；覆写中 query=Some(to)，撤销后两次 query=None，
/// 复活后 query=Some(to) 且 seq 恰 +1。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ScenExpect {
    pub sid: u32,
    pub kind: OverrideKind,
    pub mid_tag: u64,
    pub after_revoked_tag: u64,
    pub revive_seq_delta: u32,
}

/// 10 行冻结期望表。
pub const SCEN_TABLE: [ScenExpect; SCEN_COUNT] = [
    ScenExpect { sid: 1, kind: OverrideKind::ManifestPath, mid_tag: 0x2003, after_revoked_tag: 0, revive_seq_delta: 1 },
    ScenExpect { sid: 2, kind: OverrideKind::AssemblyRedirect, mid_tag: 0x2006, after_revoked_tag: 0, revive_seq_delta: 1 },
    ScenExpect { sid: 3, kind: OverrideKind::WindowClass, mid_tag: 0x2009, after_revoked_tag: 0, revive_seq_delta: 1 },
    ScenExpect { sid: 4, kind: OverrideKind::ProgId, mid_tag: 0x200C, after_revoked_tag: 0, revive_seq_delta: 1 },
    ScenExpect { sid: 5, kind: OverrideKind::ManifestPath, mid_tag: 0x200F, after_revoked_tag: 0, revive_seq_delta: 1 },
    ScenExpect { sid: 6, kind: OverrideKind::AssemblyRedirect, mid_tag: 0x2012, after_revoked_tag: 0, revive_seq_delta: 1 },
    ScenExpect { sid: 7, kind: OverrideKind::WindowClass, mid_tag: 0x2015, after_revoked_tag: 0, revive_seq_delta: 1 },
    ScenExpect { sid: 8, kind: OverrideKind::ProgId, mid_tag: 0x2018, after_revoked_tag: 0, revive_seq_delta: 1 },
    ScenExpect { sid: 9, kind: OverrideKind::ManifestPath, mid_tag: 0x201B, after_revoked_tag: 0, revive_seq_delta: 1 },
    ScenExpect { sid: 10, kind: OverrideKind::AssemblyRedirect, mid_tag: 0x201E, after_revoked_tag: 0, revive_seq_delta: 1 },
];

/// 场景参数生成（sid → (kind, from, to)）。
pub fn scen_params(sid: u32) -> Result<(OverrideKind, u64, u64), u32> {
    if sid == 0 || sid > SCEN_COUNT as u32 {
        return Err(ActxCode::IdOutOfRange.code());
    }
    let kind = match ALL_KINDS.get(((sid - 1) % 4) as usize) {
        Some(k) => *k,
        None => return Err(ActxCode::IdOutOfRange.code()),
    };
    let from = FROM_TAG_BASE.saturating_add(sid as u64);
    let to = TO_TAG_BASE.saturating_add((sid as u64).saturating_mul(3));
    Ok((kind, from, to))
}

// ── 场景执行（单场景全链：覆写→查→撤销→复验→复活→查） ──────────
/// 单场景五步链，返回 (mid_ok, revoke_ok, revive_ok) 三断言全过标志。
pub fn run_scenario(sid: u32, eng: &mut ActCtxEngine) -> Result<(bool, bool, bool), u32> {
    if sid == 0 || sid > SCEN_COUNT as u32 {
        return Err(ActxCode::IdOutOfRange.code());
    }
    let row = match SCEN_TABLE.get((sid - 1) as usize) {
        Some(r) => *r,
        None => return Err(ActxCode::ScenarioMissing.code()),
    };
    let (kind, from, to) = scen_params(sid)?;
    // 断言一：覆写生效中 query == Some(to)。
    let seq1 = eng.apply(kind, from, to)?;
    let mid = eng.query(kind, from);
    let mid_ok = mid == Some(to) && to == row.mid_tag && mid.is_some();
    // 断言二：撤销后两次复验一致（None 恒等）。
    let _ = eng.revoke(kind, from)?;
    let q1 = eng.query(kind, from);
    let q2 = eng.query(kind, from);
    let revoke_ok = q1.is_none() && q2.is_none() && q1 == q2 && row.after_revoked_tag == 0;
    // 断言三：复活后 query == Some(to) 且 seq 恰 +1（单调）。
    let seq2 = eng.apply(kind, from, to)?;
    let after = eng.query(kind, from);
    let revive_ok = after == Some(to) && seq2 == seq1.saturating_add(row.revive_seq_delta);
    Ok((mid_ok, revoke_ok, revive_ok))
}

/// 黄金全链：10 场景逐一跑全过 + 槽位守恒。
pub fn golden_run() -> Result<(u32, u32, u32), u32> {
    let mut eng = ActCtxEngine::new();
    let mut n_ok = 0u32;
    let mut sid = 1u32;
    while sid <= SCEN_COUNT as u32 {
        let (m, r, v) = run_scenario(sid, &mut eng)?;
        if m && r && v {
            n_ok = n_ok.saturating_add(1);
        } else {
            return Err(ActxCode::RevocationDrift.code());
        }
        sid += 1;
    }
    if !eng.conserve() {
        return Err(ActxCode::ConservationBroken.code());
    }
    Ok((n_ok, eng.applies, eng.revokes))
}

// ── 注入核：三类违约 ────────────────────────────────────────────
/// 类①：同键重复覆写（Active 再 apply）→ AlreadyActive 显式拒。
pub fn inject_double_apply() -> Result<bool, u32> {
    let mut eng = ActCtxEngine::new();
    let s1 = eng.apply(OverrideKind::WindowClass, 0x3001, 0x4001)?;
    let hit = match eng.apply(OverrideKind::WindowClass, 0x3001, 0x4002) {
        Err(c) => c == ActxCode::AlreadyActive.code(),
        Ok(_) => false,
    };
    // 幂等保护生效：原条目未被覆盖。
    let intact = eng.query(OverrideKind::WindowClass, 0x3001) == Some(0x4001);
    Ok(hit && intact && s1 > 0)
}

/// 类②：非法撤销（未覆写键 / Revoked 再撤）→ NotActive 显式拒。
pub fn inject_bad_revoke() -> Result<bool, u32> {
    let mut eng = ActCtxEngine::new();
    // 未覆写键撤销。
    let h1 = match eng.revoke(OverrideKind::ProgId, 0x5001) {
        Err(c) => c == ActxCode::NotActive.code(),
        Ok(_) => false,
    };
    // 覆写后撤销，再撤销一次（Revoked 态不可再撤）。
    let _ = eng.apply(OverrideKind::ProgId, 0x5002, 0x6002);
    let _ = eng.revoke(OverrideKind::ProgId, 0x5002);
    let h2 = match eng.revoke(OverrideKind::ProgId, 0x5002) {
        Err(c) => c == ActxCode::NotActive.code(),
        Ok(_) => false,
    };
    Ok(h1 && h2)
}

/// 类③：零 tag（from=0 / to=0）→ ZeroTag 显式拒。
pub fn inject_zero_tag() -> Result<bool, u32> {
    let mut eng = ActCtxEngine::new();
    let h1 = match eng.apply(OverrideKind::ManifestPath, 0, 0x7001) {
        Err(c) => c == ActxCode::ZeroTag.code(),
        Ok(_) => false,
    };
    let h2 = match eng.apply(OverrideKind::ManifestPath, 0x7002, 0) {
        Err(c) => c == ActxCode::ZeroTag.code(),
        Ok(_) => false,
    };
    Ok(h1 && h2)
}

/// 注入检出账。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InjectLedger {
    pub tries: [u32; INJECT_CLASSES],
    pub caught: [u32; INJECT_CLASSES],
}

impl InjectLedger {
    pub fn new() -> InjectLedger {
        InjectLedger { tries: [0; INJECT_CLASSES], caught: [0; INJECT_CLASSES] }
    }
    pub fn record(&mut self, slot: usize, hit: bool) {
        if let Some(t) = self.tries.get_mut(slot) {
            *t = t.saturating_add(1);
        }
        if hit {
            if let Some(c) = self.caught.get_mut(slot) {
                *c = c.saturating_add(1);
            }
        }
    }
    pub fn all_caught(&self) -> bool {
        let mut i = 0;
        while i < INJECT_CLASSES {
            let t = match self.tries.get(i) {
                Some(v) => *v,
                None => return false,
            };
            let c = match self.caught.get(i) {
                Some(v) => *v,
                None => return false,
            };
            if t == 0 || c != t {
                return false;
            }
            i += 1;
        }
        true
    }
    pub fn missed_total(&self) -> u32 {
        let mut m = 0u32;
        let mut i = 0;
        while i < INJECT_CLASSES {
            let t = match self.tries.get(i) {
                Some(v) => *v,
                None => 0,
            };
            let c = match self.caught.get(i) {
                Some(v) => *v,
                None => 0,
            };
            m = m.saturating_add(t.saturating_sub(c));
            i += 1;
        }
        m
    }
}

/// 注入 10/10：三类违约逐类断言被显式拒绝。
pub fn inject_rounds(rounds: u32) -> Result<InjectLedger, u32> {
    let mut led = InjectLedger::new();
    let mut r = 0;
    while r < rounds {
        led.record(0, inject_double_apply()?);
        led.record(1, inject_bad_revoke()?);
        led.record(2, inject_zero_tag()?);
        r += 1;
    }
    Ok(led)
}

// ── 对账辅助判据 ────────────────────────────────────────────────
/// 双跑一致：黄金全链 + 引擎指纹两次相等。
pub fn repro_check() -> Result<(), u32> {
    let a = golden_run()?;
    let b = golden_run()?;
    if a != b {
        return Err(ActxCode::NotReproducible.code());
    }
    let mut e1 = ActCtxEngine::new();
    let mut e2 = ActCtxEngine::new();
    let mut sid = 1u32;
    while sid <= SCEN_COUNT as u32 {
        let _ = run_scenario(sid, &mut e1);
        let _ = run_scenario(sid, &mut e2);
        sid += 1;
    }
    if e1.engine_fnv() != e2.engine_fnv() {
        return Err(ActxCode::NotReproducible.code());
    }
    Ok(())
}

/// 槽位满边界：8 个异键 Active 后第 9 个 apply → TableFull 且旧账不丢。
pub fn table_full_boundary() -> Result<bool, u32> {
    let mut eng = ActCtxEngine::new();
    let mut i = 0u32;
    while i < SLOT_COUNT as u32 {
        let from = 0x8000u64.saturating_add(i as u64);
        let to = 0x9000u64.saturating_add(i as u64);
        eng.apply(OverrideKind::ManifestPath, from, to)?;
        i += 1;
    }
    // 第 9 个异键 → 满。
    let full_hit = match eng.apply(OverrideKind::ManifestPath, 0xA000, 0xB000) {
        Err(c) => c == ActxCode::TableFull.code(),
        Ok(_) => false,
    };
    // 旧账不丢：16 槽 Active 各自可查。
    let mut intact = full_hit;
    let mut j = 0u32;
    while j < SLOT_COUNT as u32 {
        let from = 0x8000u64.saturating_add(j as u64);
        let to = 0x9000u64.saturating_add(j as u64);
        if eng.query(OverrideKind::ManifestPath, from) != Some(to) {
            intact = false;
        }
        j += 1;
    }
    Ok(intact)
}

/// 段位守卫：12 码全落 [SEG_BASE, SEG_BASE+0x40)。
pub fn segment_guard() -> bool {
    let mut i = 0;
    while i < ActxCode::ALL.len() {
        if let Some(c) = ActxCode::ALL.get(i) {
            let v = c.code();
            if v < SEG_BASE || v >= SEG_BASE.saturating_add(0x40) {
                return false;
            }
        }
        i += 1;
    }
    true
}

// ── 判据收口 ────────────────────────────────────────────────────
/// UNX-F18524 自检判据（15 条 ≤ 20 上限；动态明细 ≤ 8 槽）。
pub fn run_unx18524_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F18524 ActCtx 覆写探针（10 例）");

    s.add("J3-段位守卫", segment_guard(), "0x7FEC0 窗口内 12 码全落位");

    // J1 主锚点：10 场景判定全过 + 撤销态复验一致。
    let gold = golden_run();
    let (g_ok, g_txt) = match &gold {
        Ok((n, a, r)) => (*n == SCEN_COUNT as u32, format!("全过={} applies={} revokes={}", n, a, r)),
        Err(c) => (false, format!("黄金链断:{}", detail_of(*c))),
    };
    s.add_dyn("J1-10场景断言全过", g_ok, &g_txt);

    // 撤销态复验一致（独立复验：每场景撤销后双查相等）。
    let revoke_ok = {
        let mut eng = ActCtxEngine::new();
        let mut ok = true;
        let mut sid = 1u32;
        while sid <= SCEN_COUNT as u32 {
            let (_, r, _) = match run_scenario(sid, &mut eng) {
                Ok(x) => x,
                Err(_) => { ok = false; break; }
            };
            if !r {
                ok = false;
                break;
            }
            sid += 1;
        }
        ok
    };
    s.add("J1-撤销态复验一致", revoke_ok, "每场景撤销后双查 None 恒等");

    // 槽位守恒（黄金链后 applies = active + revokes）。
    let conserve_ok = {
        let mut eng = ActCtxEngine::new();
        let mut sid = 1u32;
        let mut ok = true;
        while sid <= SCEN_COUNT as u32 {
            if run_scenario(sid, &mut eng).is_err() {
                ok = false;
                break;
            }
            sid += 1;
        }
        ok && eng.conserve()
    };
    s.add("J2-槽位守恒", conserve_ok, "applies = active_now + revokes");

    let inj = inject_rounds(JUDGE_ROUNDS);
    s.add(
        "J2-注入①重复覆写10/10",
        match &inj { Ok(l) => l.caught[0] == l.tries[0] && l.tries[0] == JUDGE_ROUNDS, Err(_) => false },
        "AlreadyActive 拒 + 原条目不被覆盖",
    );
    s.add(
        "J2-注入②非法撤销10/10",
        match &inj { Ok(l) => l.caught[1] == l.tries[1] && l.tries[1] == JUDGE_ROUNDS, Err(_) => false },
        "未覆写键与 Revoked 再撤全拒",
    );
    s.add(
        "J2-注入③零tag10/10",
        match &inj { Ok(l) => l.caught[2] == l.tries[2] && l.tries[2] == JUDGE_ROUNDS, Err(_) => false },
        "from=0 / to=0 全拒",
    );

    s.add(
        "J2-漏检恒零",
        match &inj { Ok(l) => l.missed_total() == 0, Err(_) => false },
        "tries = caught + missed，missed=0",
    );

    // 状态机：迁移闭集合法恰 3 条（3 态 9 组合枚举）。
    let states = [OvrState::Empty, OvrState::Active, OvrState::Revoked];
    let mut legal_n = 0usize;
    let mut a = 0;
    while a < states.len() {
        let mut b = 0;
        while b < states.len() {
            let fa = match states.get(a) { Some(x) => *x, None => break };
            let fb = match states.get(b) { Some(x) => *x, None => break };
            if can_transit(fa, fb) {
                legal_n = legal_n.saturating_add(1);
            }
            b += 1;
        }
        a += 1;
    }
    s.add("J2-迁移闭集合法恰3条", legal_n == 3, "3 态 9 组合枚举：合法 3 / 非法 6");

    // 边界：槽满 + 旧账不丢 + 场景号越界。
    let boundary = match table_full_boundary() {
        Ok(v) => v,
        Err(_) => false,
    } && scen_params(0).is_err()
        && scen_params(SCEN_COUNT as u32 + 1).is_err();
    s.add("J3-槽满越界边界全拒", boundary, "16 槽满 TableFull + 旧账不丢 + sid 0/11");

    // seq 单调：复活恰 +1 且全局计数器不回退。
    let seq_mono = {
        let mut eng = ActCtxEngine::new();
        let s1 = eng.apply(OverrideKind::WindowClass, 0xC001, 0xD001).unwrap_or(0);
        let _ = eng.revoke(OverrideKind::WindowClass, 0xC001);
        let s2 = eng.apply(OverrideKind::WindowClass, 0xC001, 0xD001).unwrap_or(0);
        s2 == s1.saturating_add(1) && eng.next_seq >= s2
    };
    s.add("J2-seq单调复活+1", seq_mono, "同槽复活 seq 严格 +1，计数器不回退");

    // 双跑一致。
    s.add("J2-双跑一致", repro_check().is_ok(), "golden_run + engine_fnv 两跑同值");

    // 文案闭集。
    let mut detail_full = true;
    let mut di = 0;
    while di < ActxCode::ALL.len() {
        if let Some(c) = ActxCode::ALL.get(di) {
            if c.detail().len() == 0 || detail_of(c.code()).len() == 0 {
                detail_full = false;
            }
        }
        di += 1;
    }
    s.add("J3-文案闭集非空", detail_full, "12 码双查非空");

    // 场景期望表 10 行在位（行数冻结）。
    s.add("J1-期望表10行冻结", SCEN_TABLE.len() == SCEN_COUNT, "裁决表行数 == 场景数 == 10");

    // 收口。
    s.add("J3-判据条数≤20收口", s.len() < 20, "本模块判据上限纪律（含本条）");

    s
}

// ── 测试 ────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn t01_all_checks_green() {
        let set = run_unx18524_checks();
        assert!(set.all_passed(), "UNX-F18524 全判据应全绿: {}", set.red_items_text());
    }

    #[test]
    fn t02_golden_ten_scenarios() {
        let g = golden_run();
        assert!(g.is_ok());
        let (n, applies, revokes) = g.unwrap_or((0, 0, 0));
        assert_eq!(n, 10);
        assert_eq!(applies, 20); // 每场景 apply 2 次（覆写+复活）
        assert_eq!(revokes, 10); // 每场景撤销 1 次
    }

    #[test]
    fn t03_override_and_revoke_semantics() {
        let mut eng = ActCtxEngine::new();
        // 覆写中 query = Some(to)。
        assert!(eng.apply(OverrideKind::AssemblyRedirect, 0x111, 0x222).is_ok());
        assert_eq!(eng.query(OverrideKind::AssemblyRedirect, 0x111), Some(0x222));
        // 撤销后回落基值。
        assert!(eng.revoke(OverrideKind::AssemblyRedirect, 0x111).is_ok());
        assert_eq!(eng.query(OverrideKind::AssemblyRedirect, 0x111), None);
        // 未覆写键 query = None。
        assert_eq!(eng.query(OverrideKind::ProgId, 0x333), None);
    }

    #[test]
    fn t04_double_apply_reject_and_intact() {
        let mut eng = ActCtxEngine::new();
        let _ = eng.apply(OverrideKind::WindowClass, 0x10, 0x20);
        assert!(eng.apply(OverrideKind::WindowClass, 0x10, 0x99).is_err());
        assert_eq!(eng.query(OverrideKind::WindowClass, 0x10), Some(0x20));
    }

    #[test]
    fn t05_injection_all_caught() {
        let inj = inject_rounds(JUDGE_ROUNDS).unwrap_or(InjectLedger::new());
        assert!(inj.all_caught());
        assert_eq!(inj.missed_total(), 0);
    }

    #[test]
    fn t06_table_full_boundary() {
        assert!(table_full_boundary().unwrap_or(false));
        let mut eng = ActCtxEngine::new();
        let mut i = 0;
        while i < SLOT_COUNT {
            let from = 0xE000u64 + i as u64;
            assert!(eng.apply(OverrideKind::ProgId, from, from + 1).is_ok());
            i += 1;
        }
        assert!(eng.apply(OverrideKind::ProgId, 0xF000, 0xF001).is_err());
    }

    #[test]
    fn t07_state_machine_closed_set() {
        // 合法恰 3 条。
        assert!(can_transit(OvrState::Empty, OvrState::Active));
        assert!(can_transit(OvrState::Active, OvrState::Revoked));
        assert!(can_transit(OvrState::Revoked, OvrState::Active));
        // 非法 6 组合。
        assert!(!can_transit(OvrState::Empty, OvrState::Revoked));
        assert!(!can_transit(OvrState::Active, OvrState::Active));
        assert!(!can_transit(OvrState::Revoked, OvrState::Revoked));
        assert!(!can_transit(OvrState::Empty, OvrState::Empty));
        assert!(!can_transit(OvrState::Active, OvrState::Empty));
        assert!(!can_transit(OvrState::Revoked, OvrState::Empty));
    }

    #[test]
    fn t08_revocation_no_drift() {
        let mut eng = ActCtxEngine::new();
        let s0 = eng.apply(OverrideKind::ManifestPath, 0x77, 0x88).unwrap_or(0);
        let _ = eng.revoke(OverrideKind::ManifestPath, 0x77);
        let q1 = eng.query(OverrideKind::ManifestPath, 0x77);
        let q2 = eng.query(OverrideKind::ManifestPath, 0x77);
        assert_eq!(q1, None);
        assert_eq!(q1, q2);
        // 复活（Revoked 同键槽优先复用）且 seq +1。
        let s1 = eng.apply(OverrideKind::ManifestPath, 0x77, 0x88).unwrap_or(0);
        assert_eq!(s1, s0 + 1);
        assert_eq!(eng.query(OverrideKind::ManifestPath, 0x77), Some(0x88));
        // 已 Active 再 apply → 显式拒（不静默覆盖）。
        assert!(eng.apply(OverrideKind::ManifestPath, 0x77, 0x88).is_err());
    }

    #[test]
    fn t09_double_run_deterministic() {
        assert!(repro_check().is_ok());
        let a = golden_run();
        let b = golden_run();
        assert_eq!(a, b);
    }

    #[test]
    fn t10_segment_and_detail() {
        assert!(segment_guard());
        let mut i = 0;
        while i < ActxCode::ALL.len() {
            if let Some(c) = ActxCode::ALL.get(i) {
                assert_eq!(detail_of(c.code()), c.detail());
                assert!(c.detail().len() > 0);
            }
            i += 1;
        }
        assert_eq!(detail_of(0xDEAD), "未知诊断码：不在 0x7FEC0 段闭集内");
    }
}
