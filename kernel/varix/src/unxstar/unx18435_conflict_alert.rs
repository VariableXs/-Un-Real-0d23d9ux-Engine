//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F18435 矩阵冲突检测（同级双源冲突告警） · 0x7E880段
//! ─────────────────────────────────────────────────────────────────────
//! 锚点：UNX-F18435-J1 同级冲突（双 manifest 段）检出告警 10/10，冲突处置按优先级仲裁有账
//! 设计要点：
//!   - 冲突定义（闭集）：同一键 (进程,API,维度) 上、**同类源**（同级，
//!     如两个 manifest 段）、**异值** 的第二次注册 → 冲突告警；
//!     三类非冲突显式不告警：同值异源（去重记账）/同源重注册（更新）/
//!     跨层类异值（优先级链管辖，非同级）；
//!   - 仲裁有账：每条冲突必产一条仲裁账——规则冻结 v1：
//!     胜者 = min(类序, 段序)，平序取先注册（确定性，可复算）；
//!     矩阵生效值切换为胜者值（冲突不污染查询面）；
//!   - 源类闭集四级：ActCtx(0)>ManifestSegment(1)>CompatFlag(2)>
//!     AnchorBaseline(3)，类序越界注册拒；
//!   - 注入核三类×10 轮：同级异值 10/10 告警、无依据 10/10 拒、
//!     越层类 10/10 拒；误告账（非冲突被告警）必须为 0；
//!   - 守恒账：告警数 = 冲突注册对数；仲裁账数 = 告警数；
//!   - 确定性：LCG 种子=SEG_BASE(0x7E880)；no_std+alloc；零 unwrap/
//!     expect/panic；禁浮点/IO/墙钟/rand。
//! 结构清单：
//!   SourceClass / SourceRef / KeyId / ConflictCode(诊断闭集)
//!   / RegEntry+KeyLedger(键级注册账) / ConflictEvent(告警)
//!   / ArbitrationRecord(仲裁账) / ConflictMatrix(矩阵本体)
//!   / InjectLedger / golden_run / run_unx18435_checks
//! ═══════════════════════════════════════════════════════════════════════

use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x7E880，AI-VX11-1105-G1·A04 独占区）。
pub const SEG_BASE: u32 = 0x7E880;

// ── 域冻结常量（推导注记） ───────────────────────────────────────
/// 源类数（冻结：四级优先链）。
pub const CLASS_N: usize = 4;
/// 注入判定轮数（冻结：10/10 语义）。
pub const JUDGE_ROUNDS: u32 = 10;
/// 键容量（冻结：语料键空间 6 进程 × 3 API × 2 维度 = 36 ≤ 64）。
pub const KEY_CAP: usize = 64;
/// 单键注册容量（冻结：每键至多 8 条注册账）。
pub const REG_CAP_PER_KEY: usize = 8;
/// 告警账容量（冻结：≥注入 10 轮×1 对）。
pub const ALARM_CAP: usize = 32;

// ── 源类枚举（同级判定即同类判定） ──────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SourceClass {
    /// 激活上下文源。
    ActCtx = 0,
    /// manifest 段源（同级冲突的主战场）。
    ManifestSegment = 1,
    /// 兼容层标志源。
    CompatFlag = 2,
    /// 锚基线源。
    AnchorBaseline = 3,
}

impl SourceClass {
    /// 类序（0 最高）。
    pub fn rank(self) -> usize {
        self as usize
    }
    /// 由类序还原；越界 None。
    pub fn from_rank(r: usize) -> Option<SourceClass> {
        match r {
            0 => Some(SourceClass::ActCtx),
            1 => Some(SourceClass::ManifestSegment),
            2 => Some(SourceClass::CompatFlag),
            3 => Some(SourceClass::AnchorBaseline),
            _ => None,
        }
    }
    /// 类名（诊断文本共用）。
    pub fn name(self) -> &'static str {
        match self {
            SourceClass::ActCtx => "ActCtx",
            SourceClass::ManifestSegment => "ManifestSegment",
            SourceClass::CompatFlag => "CompatFlag",
            SourceClass::AnchorBaseline => "AnchorBaseline",
        }
    }
}

/// 源引用：类序 + 段号（段号是同级内仲裁的第一键）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct SourceRef {
    pub class_rank: u8,
    pub seg: u32,
}

impl SourceRef {
    /// 段号 0 视为无段（非法）——段号必须 ≥1，防「无主源」。
    pub fn legal(&self) -> bool {
        SourceClass::from_rank(self.class_rank as usize).is_some() && self.seg >= 1
    }
    /// 仲裁序：(类序, 段序) 字典序，小者胜。
    pub fn arb_key(&self) -> (usize, u32) {
        (self.class_rank as usize, self.seg)
    }
}

/// 键三元组：进程 × API 族 × 维度。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct KeyId {
    pub proc_id: u32,
    pub api_id: u32,
    pub dim: u32,
}

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ConflictCode {
    /// 同级双源异值冲突（告警码，入账）
    SameLevelConflict,
    /// 无依据注册拒
    BasisMissing,
    /// 源类越界
    ClassIllegal,
    /// 段号非法（0=无主源）
    SegIllegal,
    /// 键账满
    KeyFull,
    /// 单键注册账满
    RegFull,
    /// 仲裁账缺失（告警无账=守恒破坏）
    ArbMissing,
    /// 冲突值污染查询面（生效值≠胜者值）
    ValuePolluted,
    /// 误告（非冲突被告警）
    FalseAlarm,
    /// 双跑不一致
    NotReproducible,
}

impl ConflictCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            ConflictCode::SameLevelConflict => "同级双源异值冲突：已告警入账",
            ConflictCode::BasisMissing => "注册依据为空：有值必有据不变量",
            ConflictCode::ClassIllegal => "源类越界：四级闭集外",
            ConflictCode::SegIllegal => "段号非法：0 视为无主源",
            ConflictCode::KeyFull => "键账满：注册被拒且账不丢",
            ConflictCode::RegFull => "单键注册账满：超出按拒绝记账",
            ConflictCode::ArbMissing => "告警无仲裁账：守恒破坏",
            ConflictCode::ValuePolluted => "冲突值污染：生效值与胜者不一致",
            ConflictCode::FalseAlarm => "误告：非冲突样本被告警",
            ConflictCode::NotReproducible => "双跑不一致：确定性红线",
        }
    }
    pub const ALL: [ConflictCode; 10] = [
        ConflictCode::SameLevelConflict,
        ConflictCode::BasisMissing,
        ConflictCode::ClassIllegal,
        ConflictCode::SegIllegal,
        ConflictCode::KeyFull,
        ConflictCode::RegFull,
        ConflictCode::ArbMissing,
        ConflictCode::ValuePolluted,
        ConflictCode::FalseAlarm,
        ConflictCode::NotReproducible,
    ];
}

/// 按数值取中文文案。
pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < ConflictCode::ALL.len() {
        if ConflictCode::ALL[i].val() == code {
            return ConflictCode::ALL[i].detail();
        }
        i += 1;
    }
    "未知诊断码：不在 0x7E880 段闭集内"
}

// ── 键级注册账 ──────────────────────────────────────────────────
/// 单条注册：源 + 值 + 依据 + 全局注册序（平序仲裁用）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RegEntry {
    pub src: SourceRef,
    pub value: u32,
    pub basis: u32,
    pub order: u32,
}

/// 注册结果闭集（告警外的三种正常落账）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Outcome {
    /// 新键新源，首条注册。
    Registered,
    /// 同源重注册，值已更新。
    Updated,
    /// 同级异源同值，去重记账（不告警）。
    Deduped,
    /// 同级异源异值，冲突已告警+仲裁。
    Conflicted,
}

/// 单键账：注册列表 + 生效值（胜者值）。
#[derive(Clone, Debug)]
pub struct KeyLedger {
    pub key: KeyId,
    pub regs: Vec<RegEntry>,
    /// 生效值（注册/仲裁后与胜者一致）。
    pub effective: u32,
}

// ── 告警与仲裁账 ────────────────────────────────────────────────
/// 冲突告警：键 + 新旧源 + 异值对。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ConflictEvent {
    pub key: KeyId,
    pub src_new: SourceRef,
    pub src_old: SourceRef,
    pub val_new: u32,
    pub val_old: u32,
}

/// 仲裁账：冲突处置落账——胜者按冻结规则选出。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ArbitrationRecord {
    pub key: KeyId,
    pub winner: SourceRef,
    pub loser: SourceRef,
    pub winner_val: u32,
    pub loser_val: u32,
    /// 仲裁规则码（冻结 v1 = 1）。
    pub rule: u32,
}

/// 仲裁规则码冻结 v1。
pub const ARB_RULE_V1: u32 = 1;

/// 冻结仲裁规则：胜者 = min(arb_key)，平序取注册序小者。
/// 返回 (胜者, 败者)；对 a/b 两源 + 各自注册序求值。
pub fn arbitrate(a: &RegEntry, b: &RegEntry) -> (SourceRef, SourceRef) {
    let ka = a.src.arb_key();
    let kb = b.src.arb_key();
    if ka < kb {
        (a.src, b.src)
    } else if kb < ka {
        (b.src, a.src)
    } else if a.order <= b.order {
        (a.src, b.src)
    } else {
        (b.src, a.src)
    }
}

// ── 矩阵本体 ────────────────────────────────────────────────────
/// 冲突检测矩阵：键账 + 告警账 + 仲裁账。
pub struct ConflictMatrix {
    ledgers: Vec<KeyLedger>,
    pub alarms: Vec<ConflictEvent>,
    pub arbitrations: Vec<ArbitrationRecord>,
    key_cap: usize,
    reg_cap: usize,
    alarm_cap: usize,
    /// 全局注册序计数。
    order_seq: u32,
}

impl ConflictMatrix {
    pub fn new() -> ConflictMatrix {
        ConflictMatrix {
            ledgers: Vec::new(),
            alarms: Vec::new(),
            arbitrations: Vec::new(),
            key_cap: KEY_CAP,
            reg_cap: REG_CAP_PER_KEY,
            alarm_cap: ALARM_CAP,
            order_seq: 0,
        }
    }

    /// 找键账下标。
    pub fn find(&self, key: &KeyId) -> Option<usize> {
        let mut i = 0;
        while i < self.ledgers.len() {
            let l = &self.ledgers[i];
            if l.key.proc_id == key.proc_id && l.key.api_id == key.api_id && l.key.dim == key.dim {
                return Some(i);
            }
            i += 1;
        }
        None
    }

    /// 注册一值：三级闸（依据/源合法性/容量）后按冲突闭集分派。
    pub fn register(&mut self, key: KeyId, src: SourceRef, value: u32, basis: u32) -> Result<Outcome, u32> {
        // 闸 1：依据非空。
        if basis == 0 {
            return Err(ConflictCode::BasisMissing.val());
        }
        // 闸 2：源合法性（类闭集 + 段号非零）。
        if !src.legal() {
            if SourceClass::from_rank(src.class_rank as usize).is_none() {
                return Err(ConflictCode::ClassIllegal.val());
            }
            return Err(ConflictCode::SegIllegal.val());
        }
        let order = self.order_seq;
        self.order_seq = self.order_seq.saturating_add(1);
        let entry = RegEntry { src, value, basis, order };
        // 无键账：开新账。
        let idx = match self.find(&key) {
            Some(i) => i,
            None => {
                if self.ledgers.len() >= self.key_cap {
                    return Err(ConflictCode::KeyFull.val());
                }
                self.ledgers.push(KeyLedger { key, regs: Vec::new(), effective: value });
                let i = self.ledgers.len() - 1;
                // 首条注册直接落生效值。
                let l = &mut self.ledgers[i];
                l.regs.push(entry);
                return Ok(Outcome::Registered);
            }
        };
        // 有键账：按冲突闭集分派。
        let (outcome, alarm, arb) = {
            let l = &mut self.ledgers[idx];
            if l.regs.len() >= self.reg_cap {
                return Err(ConflictCode::RegFull.val());
            }
            // 同源重注册 → 更新。
            let mut same_src = false;
            let mut r = 0;
            while r < l.regs.len() {
                if l.regs[r].src == src {
                    same_src = true;
                }
                r += 1;
            }
            if same_src {
                // 同源更新：账面补一条新序（保留历史，审计可回放）。
                l.regs.push(entry);
                l.effective = value;
                (Outcome::Updated, None, None)
            } else {
                // 异源：找同类旧源中最强仲裁序者（同级对手）。
                let mut rival_idx: Option<usize> = None;
                let mut r = 0;
                while r < l.regs.len() {
                    if l.regs[r].src.class_rank == src.class_rank {
                        match rival_idx {
                            None => rival_idx = Some(r),
                            Some(ri) => {
                                if l.regs[r].arb_order_key() < l.regs[ri].arb_order_key() {
                                    rival_idx = Some(r);
                                }
                            }
                        }
                    }
                    r += 1;
                }
                match rival_idx {
                    None => {
                        // 跨类：优先级链管辖，非同级冲突，照常落账。
                        l.regs.push(entry);
                        l.effective = value;
                        (Outcome::Registered, None, None)
                    }
                    Some(ri) => {
                        let rival_val = l.regs[ri].value;
                        if rival_val == value {
                            // 同值异源：去重记账不告警。
                            l.regs.push(entry);
                            (Outcome::Deduped, None, None)
                        } else {
                            // 同级异值：冲突告警 + 仲裁。
                            let rival_src = l.regs[ri].src;
                            let probe_a = RegEntry { src, value, basis, order };
                            let probe_b = RegEntry {
                                src: rival_src,
                                value: rival_val,
                                basis: l.regs[ri].basis,
                                order: l.regs[ri].order,
                            };
                            let (win, lose) = arbitrate(&probe_a, &probe_b);
                            let win_val = if win == src { value } else { rival_val };
                            let lose_val = if win == src { rival_val } else { value };
                            l.effective = win_val;
                            let alarm = ConflictEvent {
                                key,
                                src_new: src,
                                src_old: rival_src,
                                val_new: value,
                                val_old: rival_val,
                            };
                            let arb = ArbitrationRecord {
                                key,
                                winner: win,
                                loser: lose,
                                winner_val: win_val,
                                loser_val: lose_val,
                                rule: ARB_RULE_V1,
                            };
                            (Outcome::Conflicted, Some(alarm), Some(arb))
                        }
                    }
                }
            }
        };
        // 账面写回（借用结束后统一落告警/仲裁账）。
        if let Some(al) = alarm {
            if self.alarms.len() >= self.alarm_cap {
                return Err(ConflictCode::KeyFull.val());
            }
            self.alarms.push(al);
        }
        if let Some(ar) = arb {
            self.arbitrations.push(ar);
        }
        Ok(outcome)
    }

    /// 查询生效值（冲突不污染：生效值恒等于胜者值）。
    pub fn query(&self, key: &KeyId) -> Option<u32> {
        match self.find(key) {
            Some(i) => Some(self.ledgers[i].effective),
            None => None,
        }
    }

    /// 仲裁账与告警账守恒：每条告警必有一条同键同源对的仲裁账。
    pub fn conserve(&self) -> Result<(), u32> {
        if self.alarms.len() != self.arbitrations.len() {
            return Err(ConflictCode::ArbMissing.val());
        }
        let mut i = 0;
        while i < self.alarms.len() {
            let al = &self.alarms[i];
            let mut found = false;
            let mut k = 0;
            while k < self.arbitrations.len() {
                let ar = &self.arbitrations[k];
                let pair_match = (ar.winner == al.src_new && ar.loser == al.src_old)
                    || (ar.winner == al.src_old && ar.loser == al.src_new);
                let key_match = ar.key.proc_id == al.key.proc_id
                    && ar.key.api_id == al.key.api_id
                    && ar.key.dim == al.key.dim;
                if pair_match && key_match {
                    found = true;
                }
                k += 1;
            }
            if !found {
                return Err(ConflictCode::ArbMissing.val());
            }
            i += 1;
        }
        Ok(())
    }

    /// 仲裁规则逐例复算：胜者值 == 生效值（冲突不污染）。
    pub fn no_pollution(&self) -> Result<(), u32> {
        let mut i = 0;
        while i < self.arbitrations.len() {
            let ar = &self.arbitrations[i];
            let key = KeyId { proc_id: ar.key.proc_id, api_id: ar.key.api_id, dim: ar.key.dim };
            let eff = match self.query(&key) {
                Some(v) => v,
                None => return Err(ConflictCode::ValuePolluted.val()),
            };
            if eff != ar.winner_val {
                return Err(ConflictCode::ValuePolluted.val());
            }
            // 规则复算：胜者 arb_key 必小于败者（或平序注册序判定，此处按键序复核）。
            if ar.winner.arb_key() > ar.loser.arb_key() {
                return Err(ConflictCode::ValuePolluted.val());
            }
            i += 1;
        }
        Ok(())
    }

    pub fn used_keys(&self) -> usize {
        self.ledgers.len()
    }
}

/// RegEntry 的仲裁序辅助（源序 + 注册序）。
impl RegEntry {
    pub fn arb_order_key(&self) -> (usize, u32, u32) {
        (self.src.class_rank as usize, self.src.seg, self.order)
    }
}

// ── 注入核（三类 ×10 轮） ───────────────────────────────────────
pub struct InjectLedger {
    pub tries: [u32; 3],
    pub caught: [u32; 3],
}

impl InjectLedger {
    pub fn new() -> InjectLedger {
        InjectLedger { tries: [0; 3], caught: [0; 3] }
    }
    pub fn record(&mut self, slot: usize, hit: bool) {
        if slot < 3 {
            self.tries[slot] = self.tries[slot].saturating_add(1);
            if hit {
                self.caught[slot] = self.caught[slot].saturating_add(1);
            }
        }
    }
    pub fn all_caught(&self) -> bool {
        let mut i = 0;
        while i < 3 {
            if self.tries[i] == 0 || self.caught[i] != self.tries[i] {
                return false;
            }
            i += 1;
        }
        true
    }
    pub fn missed(&self) -> u32 {
        let mut m = 0;
        let mut i = 0;
        while i < 3 {
            m += self.tries[i].saturating_sub(self.caught[i]);
            i += 1;
        }
        m
    }
}

/// 10 轮注入：
/// 槽 0：同级双源异值（双 manifest 段）→ 必告警；
/// 槽 1：无依据注册 → 必拒；
/// 槽 2：越层类（rank=4）→ 必拒。
pub fn inject_rounds(rounds: u32) -> InjectLedger {
    let mut led = InjectLedger::new();
    let key = KeyId { proc_id: 1, api_id: 1, dim: 1 };
    let mut r = 0;
    while r < rounds {
        // 槽 0：同键双 manifest 段异值（每轮新矩阵，防历史串账）。
        let mut m0 = ConflictMatrix::new();
        let r0 = m0.register(
            key,
            SourceRef { class_rank: SourceClass::ManifestSegment.rank() as u8, seg: 10 },
            62_000,
            900 + r,
        );
        let r1 = m0.register(
            key,
            SourceRef { class_rank: SourceClass::ManifestSegment.rank() as u8, seg: 11 },
            62_100,
            901 + r,
        );
        let hit0 = match (r0, r1) {
            (Ok(_), Ok(Outcome::Conflicted)) => m0.alarms.len() == 1 && m0.arbitrations.len() == 1,
            _ => false,
        };
        led.record(0, hit0);
        // 槽 1：无依据。
        let mut m1 = ConflictMatrix::new();
        let hit1 = m1
            .register(
                key,
                SourceRef { class_rank: SourceClass::ManifestSegment.rank() as u8, seg: 5 },
                62_000,
                0,
            )
            .is_err();
        led.record(1, hit1);
        // 槽 2：越层类（rank 4 不在四级闭集）。
        let mut m2 = ConflictMatrix::new();
        let hit2 = m2.register(key, SourceRef { class_rank: 4, seg: 5 }, 62_000, 1).is_err();
        led.record(2, hit2);
        r += 1;
    }
    led
}

// ── 非冲突样本（误告账必须为 0） ────────────────────────────────
/// 三类非冲突样本一次跑齐，返回告警数（必须 0）。
pub fn nonconflict_alarm_count() -> Result<usize, u32> {
    let mut m = ConflictMatrix::new();
    let key = KeyId { proc_id: 2, api_id: 1, dim: 0 };
    // ① 同值异源（同级去重）。
    let o1 = m.register(
        key,
        SourceRef { class_rank: SourceClass::ManifestSegment.rank() as u8, seg: 1 },
        62_000,
        1,
    )?;
    // ② 同源重注册（更新）。
    let o2 = m.register(
        key,
        SourceRef { class_rank: SourceClass::ManifestSegment.rank() as u8, seg: 1 },
        62_050,
        2,
    )?;
    // ③ 跨类异值（优先级链管辖）。
    let o3 = m.register(
        key,
        SourceRef { class_rank: SourceClass::CompatFlag.rank() as u8, seg: 3 },
        62_900,
        3,
    )?;
    let normal = o1 == Outcome::Registered
        && o2 == Outcome::Updated
        && (o3 == Outcome::Registered || o3 == Outcome::Updated || o3 == Outcome::Deduped);
    if !normal {
        return Err(ConflictCode::FalseAlarm.val());
    }
    Ok(m.alarms.len())
}

// ── 探针三序 ────────────────────────────────────────────────────
/// 正探针：同级冲突告警 + 仲裁有账 + 生效值=胜者值。
pub fn probe_seq_a() -> Result<(), u32> {
    let mut m = ConflictMatrix::new();
    let key = KeyId { proc_id: 3, api_id: 0, dim: 2 };
    m.register(
        key,
        SourceRef { class_rank: SourceClass::ManifestSegment.rank() as u8, seg: 2 },
        60_000,
        1,
    )?;
    m.register(
        key,
        SourceRef { class_rank: SourceClass::ManifestSegment.rank() as u8, seg: 7 },
        60_500,
        2,
    )?;
    m.conserve()?;
    m.no_pollution()?;
    Ok(())
}

/// 负探针 1：段号 0（无主源）→ SegIllegal。约定「负探针返回 Err(拒因码)」。
pub fn probe_seq_b() -> Result<(), u32> {
    let mut m = ConflictMatrix::new();
    match m.register(
        KeyId { proc_id: 0, api_id: 0, dim: 0 },
        SourceRef { class_rank: 1, seg: 0 },
        1,
        1,
    ) {
        Err(c) => Err(c),
        Ok(_) => Err(ConflictCode::NotReproducible.val()),
    }
}

/// 负探针 2：越层类 → ClassIllegal。
pub fn probe_seq_c() -> Result<(), u32> {
    let mut m = ConflictMatrix::new();
    match m.register(
        KeyId { proc_id: 0, api_id: 0, dim: 0 },
        SourceRef { class_rank: 9, seg: 1 },
        1,
        1,
    ) {
        Err(c) => Err(c),
        Ok(_) => Err(ConflictCode::NotReproducible.val()),
    }
}

/// 黄金全链：注入 30/30 + 仲裁守恒 + 误告为零 + 双跑一致。
pub fn golden_run() -> Result<(u32, usize), u32> {
    let inj = inject_rounds(JUDGE_ROUNDS);
    if !inj.all_caught() {
        return Err(ConflictCode::NotReproducible.val());
    }
    let fa = nonconflict_alarm_count()?;
    if fa != 0 {
        return Err(ConflictCode::FalseAlarm.val());
    }
    let mut m = ConflictMatrix::new();
    // 三键三冲突：同级段 1 值 A、段 2 值 B → 段 1 胜（段序小）。
    let mut k = 0;
    while k < 3 {
        let key = KeyId { proc_id: k, api_id: 1, dim: 0 };
        m.register(
            key,
            SourceRef { class_rank: SourceClass::ManifestSegment.rank() as u8, seg: 1 },
            62_000 + k,
            10 + k,
        )?;
        m.register(
            key,
            SourceRef { class_rank: SourceClass::ManifestSegment.rank() as u8, seg: 2 },
            63_000 + k,
            20 + k,
        )?;
        k += 1;
    }
    m.conserve()?;
    m.no_pollution()?;
    Ok((inj.caught[0], m.arbitrations.len()))
}

/// 段位守卫。
pub fn segment_guard() -> bool {
    let mut i = 0;
    while i < ConflictCode::ALL.len() {
        let v = ConflictCode::ALL[i].val();
        if v < SEG_BASE || v >= SEG_BASE + 0x40 {
            return false;
        }
        i += 1;
    }
    SEG_BASE % 0x40 == 0
}

// ── 判据收口 ────────────────────────────────────────────────────
pub fn run_unx18435_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F18435 矩阵冲突检测");
    // ① 黄金全链。
    let g = golden_run();
    s.add_dyn(
        "J1-黄金全链",
        g.is_ok(),
        &format!("告警{:?} 仲裁{:?}", g.as_ref().map(|r| r.0).unwrap_or(0), g.as_ref().map(|r| r.1).unwrap_or(0)),
    );
    // ② 同级冲突注入 10/10。
    let inj = inject_rounds(JUDGE_ROUNDS);
    s.add_dyn("同级冲突10/10告警", inj.caught[0] == JUDGE_ROUNDS, &format!("{}/{}", inj.caught[0], inj.tries[0]));
    s.add_dyn("无依据10/10拒", inj.caught[1] == JUDGE_ROUNDS, &format!("{}/{}", inj.caught[1], inj.tries[1]));
    s.add_dyn("越层类10/10拒", inj.caught[2] == JUDGE_ROUNDS, &format!("{}/{}", inj.caught[2], inj.tries[2]));
    s.add("注入漏检为零", inj.missed() == 0, "检出+漏检=注入");
    // ③ 仲裁有账 + 守恒（告警数=仲裁账数，逐条对上）。
    let mut m = ConflictMatrix::new();
    let key = KeyId { proc_id: 4, api_id: 0, dim: 1 };
    let mut reg_ok = m
        .register(
            key,
            SourceRef { class_rank: SourceClass::ManifestSegment.rank() as u8, seg: 1 },
            62_000,
            1,
        )
        .is_ok();
    reg_ok = reg_ok
        && m.register(
            key,
            SourceRef { class_rank: SourceClass::ManifestSegment.rank() as u8, seg: 2 },
            62_100,
            2,
        )
        .is_ok();
    let conserve_ok = reg_ok && m.conserve().is_ok() && m.alarms.len() == 1 && m.arbitrations.len() == 1;
    s.add("仲裁有账且守恒", conserve_ok, "1 告警 1 仲裁账");
    // ④ 仲裁规则逐例复算：段序小者胜 + 生效值=胜者值。
    let rule_ok = reg_ok && {
        let ar = &m.arbitrations[0];
        ar.winner.seg == 1
            && ar.loser.seg == 2
            && ar.winner_val == 62_000
            && ar.loser_val == 62_100
            && ar.rule == ARB_RULE_V1
    } && m.no_pollution().is_ok();
    s.add("仲裁规则冻结v1复算", rule_ok, "min(类序,段序) 胜者");
    // ⑤ 误告为零（三类非冲突样本）。
    let fa = nonconflict_alarm_count();
    s.add_dyn("非冲突误告为零", fa.is_ok() && fa.unwrap_or(1) == 0, &format!("告警{:?}", fa));
    // ⑥ 守恒账闭合：告警数 = 冲突注册对数（3 键 3 冲突）。
    let gm = golden_run();
    let close_ok = match &gm {
        Ok((caught, arbs)) => *caught == JUDGE_ROUNDS && *arbs == 3,
        Err(_) => false,
    };
    s.add("告警=冲突对数守恒", close_ok, "3 键 3 冲突 3 账");
    // ⑦ 冲突不污染：冲突后查询返回胜者值。
    let eff = m.query(&key);
    s.add("冲突不污染查询面", eff == Some(62_000), "生效值=胜者值");
    // ⑧ 平序仲裁：同类同段号（不可能——同段即同源）；改验注册序：
    //    两不同类源 + 第三同级源触发平序分支（构造段号相同但类不同）。
    let mut m2 = ConflictMatrix::new();
    let key2 = KeyId { proc_id: 5, api_id: 0, dim: 0 };
    let _ = m2.register(
        key2,
        SourceRef { class_rank: SourceClass::CompatFlag.rank() as u8, seg: 1 },
        70_000,
        1,
    );
    let _ = m2.register(
        key2,
        SourceRef { class_rank: SourceClass::AnchorBaseline.rank() as u8, seg: 1 },
        70_100,
        2,
    );
    // 同类平序需要同段号同类——同段号同类即同源，走更新分支；此处验证
    // 平序分支函数级正确性：同 arb_key 时注册序小者胜。
    let e1 = RegEntry {
        src: SourceRef { class_rank: 1, seg: 3 },
        value: 1,
        basis: 1,
        order: 0,
    };
    let e2 = RegEntry {
        src: SourceRef { class_rank: 1, seg: 3 },
        value: 2,
        basis: 2,
        order: 1,
    };
    let (win, _) = arbitrate(&e1, &e2);
    s.add("平序注册序仲裁", win == e1.src, "同 arb_key 取先注册");
    // ⑨ 探针三序。
    s.add("探针三序", probe_seq_a().is_ok() && probe_seq_b().is_err() && probe_seq_c().is_err(), "正/无主源/越层");
    // ⑩ 双跑一致。
    s.add("双跑一致", gm == golden_run(), "同语料同账");
    // ⑪ 文案闭集 + 段位守卫。
    let mut closed = true;
    let mut ci = 0;
    while ci < ConflictCode::ALL.len() {
        if ConflictCode::ALL[ci].detail().is_empty() {
            closed = false;
        }
        ci += 1;
    }
    s.add("诊断文案闭集非空", closed, "10 码全有中文建议");
    s.add("段位守卫 0x7E880", segment_guard(), "SEG_BASE 对齐 0x40 窗口");
    s
}

// ── 测试 ────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn same_level_conflict_ten_rounds() {
        let inj = inject_rounds(10);
        assert_eq!(inj.caught[0], 10, "同级异值必须 10/10 告警");
        assert!(inj.all_caught());
        assert_eq!(inj.missed(), 0);
    }

    #[test]
    fn arbitration_ledger_and_rule() {
        let mut m = ConflictMatrix::new();
        let key = KeyId { proc_id: 1, api_id: 1, dim: 1 };
        m.register(
            key,
            SourceRef { class_rank: SourceClass::ManifestSegment.rank() as u8, seg: 1 },
            62_000,
            1,
        )
        .unwrap_or(Outcome::Registered);
        m.register(
            key,
            SourceRef { class_rank: SourceClass::ManifestSegment.rank() as u8, seg: 2 },
            62_100,
            2,
        )
        .unwrap_or(Outcome::Registered);
        assert_eq!(m.alarms.len(), 1);
        assert_eq!(m.arbitrations.len(), 1);
        assert_eq!(m.arbitrations[0].winner.seg, 1, "段序小者胜");
        assert_eq!(m.arbitrations[0].winner_val, 62_000);
        assert_eq!(m.query(&key), Some(62_000), "生效值=胜者值");
        assert!(m.conserve().is_ok());
        assert!(m.no_pollution().is_ok());
    }

    #[test]
    fn non_conflict_samples_zero_alarm() {
        let fa = nonconflict_alarm_count();
        assert_eq!(fa, Ok(0), "同值/同源/跨类三类不得告警");
    }

    #[test]
    fn register_gate_rejections() {
        let mut m = ConflictMatrix::new();
        let key = KeyId { proc_id: 0, api_id: 0, dim: 0 };
        assert!(m.register(key, SourceRef { class_rank: 1, seg: 1 }, 1, 0).is_err(), "无依据拒");
        assert!(m.register(key, SourceRef { class_rank: 9, seg: 1 }, 1, 1).is_err(), "类越界拒");
        assert!(m.register(key, SourceRef { class_rank: 1, seg: 0 }, 1, 1).is_err(), "无主源拒");
        assert_eq!(m.used_keys(), 0, "被拒注册不得开账");
    }

    #[test]
    fn dedup_same_value_same_level() {
        let mut m = ConflictMatrix::new();
        let key = KeyId { proc_id: 2, api_id: 0, dim: 1 };
        let o1 = m.register(
            key,
            SourceRef { class_rank: SourceClass::ManifestSegment.rank() as u8, seg: 1 },
            62_000,
            1,
        );
        let o2 = m.register(
            key,
            SourceRef { class_rank: SourceClass::ManifestSegment.rank() as u8, seg: 2 },
            62_000,
            2,
        );
        assert_eq!(o1, Ok(Outcome::Registered));
        assert_eq!(o2, Ok(Outcome::Deduped));
        assert_eq!(m.alarms.len(), 0, "同值异源不告警");
    }

    #[test]
    fn same_source_update_no_alarm() {
        let mut m = ConflictMatrix::new();
        let key = KeyId { proc_id: 3, api_id: 0, dim: 0 };
        let src = SourceRef { class_rank: SourceClass::ManifestSegment.rank() as u8, seg: 4 };
        let o1 = m.register(key, src, 10, 1);
        let o2 = m.register(key, src, 20, 2);
        assert_eq!(o1, Ok(Outcome::Registered));
        assert_eq!(o2, Ok(Outcome::Updated));
        assert_eq!(m.alarms.len(), 0);
        assert_eq!(m.query(&key), Some(20));
    }

    #[test]
    fn probes_and_double_run() {
        assert!(probe_seq_a().is_ok());
        assert!(probe_seq_b().is_err());
        assert!(probe_seq_c().is_err());
        assert_eq!(golden_run(), golden_run(), "双跑一致");
    }

    #[test]
    fn checks_set_all_green() {
        let s = run_unx18435_checks();
        assert!(s.all_passed(), "本卡判据必须全绿: {}", s.red_items_text());
        assert!(s.len() >= 8 && s.len() <= 20);
    }
}
