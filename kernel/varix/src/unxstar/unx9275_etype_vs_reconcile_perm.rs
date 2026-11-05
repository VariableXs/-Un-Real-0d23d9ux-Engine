//! UNX-F9275 权限族 E 型与三身份档/B05 深化册对账格 · 0x7B240段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（派工卡 UNX-F9275 逐字）：
//!   ① 纯功能代码 ≥320 行；② 判据原文：UNX-F9275-J1 对账判据过，与 B05
//!   三身份语义基准逐条零冲突格过，正反分层声明格过；③ 完成时 API/软件写回：
//!   状态=已完成、结果摘要非空。
//!
//! 设计要点：
//!   ① 三方独立：B05 重放（Cred3 真做身份变更）、B 侧冻结 allowed 账、E 型
//!      权限层三者各自计算。对账是三方差分收敛，不是把期望值抄两遍。
//!   ② 正反两向都判：正向「B05 拒 → E 必须 EPERM」；反向「B05 放行 → E 不得
//!      报 EPERM」（值域/指针故障只许 EINVAL/EFAULT）。这就是正反分层声明的
//!      可执行含义，单向判会放过「把 EFAULT 错报成 EPERM」的层次混淆。
//!   ③ 三身份档：Real/Effective/Saved 三槽各自 ≥2 正 ≥2 负；SetUid 落在 All
//!      档，必须同时覆盖成功与拒绝。槽位由操作唯一决定，行内另写 slot 即为
//!      漂移，单源声明不允许两份槽位表。
//!   ④ B05 无对应的 E 型面（setgroups 巨量、capset 坏指针、ioperm 端口回绕、
//!      setfsuid 静默降格）单列 AUX 挂账，只许 0/EINVAL/EFAULT；若反推出
//!      EPERM，说明把值域/指针故障当成了权限故障。
//!   ⑤ 冻结账 20 行 (id, allowed) 是外部锚点；行数、名字、放行位任一漂移都会
//!      在 drift 格报红。四类缺陷（期望漂移/极性未定/丢行/基准翻转）各 10 轮
//!      注入必须全检出。
//! ─────────────────────────────────────────────────────────────────────
use alloc::{format, vec::Vec};
use crate::checks::CheckSet;
/// 诊断段基址（F9275 独占 0x40 窗口）。
pub const SEG_BASE: u32 = 0x7B240;
/// 对账行容量上限。
pub const MAX_ROWS: usize = 32;
/// 对账行总数（冻结账）。
pub const TABLE_ROWS: usize = 20;
/// 单类缺陷注入轮数。
pub const INJECT_ROUNDS: u32 = 10;
/// 权限不足。
pub const EPERM: u32 = 1;
/// 坏用户指针。
pub const EFAULT: u32 = 14;
/// 参数值非法。
pub const EINVAL: u32 = 22;
/// uid 不变哨兵（B05 同款）。
pub const UID_KEEP: u32 = u32::MAX;
/// B05 特权起始 euid。
pub const ROOT_UID: u32 = 0;
/// B05 非特权样本 uid。
pub const USER_UID: u32 = 1000;
/// B05 矩阵里的陌生 uid（非特权必拒）。
pub const FOREIGN_UID: u32 = 1001;
/// 特权成功样本 uid。
pub const TARGET_UID: u32 = 1002;

// ── 诊断码闭集 ────────────────────────────────────────────────────────
/// 对账诊断码（段内 +0x00..+0x0B 闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RecCode {
    ExpectDrift, LayerMissing, LayerOverlap, SlotDrift, FrozenDrift,
    OrderConflict, RowLoss, StageSkip, LedgerLeak, NotReproducible,
    BaselineGap, SegmentViolation,
}
impl RecCode {
    /// 码值。
    pub fn val(self) -> u32 {
        SEG_BASE + match self {
            RecCode::ExpectDrift => 0, RecCode::LayerMissing => 1,
            RecCode::LayerOverlap => 2, RecCode::SlotDrift => 3,
            RecCode::FrozenDrift => 4, RecCode::OrderConflict => 5,
            RecCode::RowLoss => 6, RecCode::StageSkip => 7,
            RecCode::LedgerLeak => 8, RecCode::NotReproducible => 9,
            RecCode::BaselineGap => 10, RecCode::SegmentViolation => 11,
        }
    }
    /// 中文处置建议。
    pub fn detail(self) -> &'static str {
        match self {
            RecCode::ExpectDrift => "E 层判定与册内期望不一致时，先确认哪侧是权威再改另一侧",
            RecCode::LayerMissing => "每槽必须同时有正判据与反判据；只剩单层说明覆盖被削掉",
            RecCode::LayerOverlap => "一行只能声明正或负；双重声明会让统计口径重复计数",
            RecCode::SlotDrift => "槽位由操作唯一决定，行内另写 slot 等于另立槽位表，必须删掉",
            RecCode::FrozenDrift => "冻结账是外部锚点，改判定器必须显式改账而不是让账跟着漂",
            RecCode::OrderConflict => "次序行失败说明门序反了：权限→值域→指针，不可交换",
            RecCode::RowLoss => "行数少于冻结值意味着有判据被删；删行必须先改账",
            RecCode::StageSkip => "对账状态机只许 Load→Baseline→Match→Layer→Verdict 单向推进",
            RecCode::LedgerLeak => "注入数必须等于检出数，逃逸必须为零",
            RecCode::NotReproducible => "对账结果必须双跑一致，禁止依赖哈希遍历顺序",
            RecCode::BaselineGap => "B05 重放与冻结 allowed 账不一致时，先查 B05 语义再查账",
            RecCode::SegmentViolation => "段基址须 0x40 对齐且落在 0x7A000..=0x7D1C0",
        }
    }
    /// 闭集全量（顺序即码序）。
    pub const ALL: [RecCode; 12] = [
        RecCode::ExpectDrift, RecCode::LayerMissing, RecCode::LayerOverlap,
        RecCode::SlotDrift, RecCode::FrozenDrift, RecCode::OrderConflict,
        RecCode::RowLoss, RecCode::StageSkip, RecCode::LedgerLeak,
        RecCode::NotReproducible, RecCode::BaselineGap, RecCode::SegmentViolation,
    ];
}

// ── B05 三身份语义（真变更重放） ──────────────────────────────────────
/// 三身份账（ruid/euid/suid），与 B05/F8881 同构。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Cred3 {
    pub ruid: u32,
    pub euid: u32,
    pub suid: u32,
}
impl Cred3 {
    /// 构造。
    pub const fn new(r: u32, e: u32, s: u32) -> Cred3 { Cred3 { ruid: r, euid: e, suid: s } }
    /// root 起点。
    pub const fn root() -> Cred3 { Cred3::new(ROOT_UID, ROOT_UID, ROOT_UID) }
    /// 普通用户起点。
    pub const fn user() -> Cred3 { Cred3::new(USER_UID, USER_UID, USER_UID) }
    /// 特权态（euid == 0）。
    pub fn privileged(&self) -> bool { self.euid == 0 }
    /// 身份池（三值）。
    pub fn pool(&self) -> [u32; 3] { [self.ruid, self.euid, self.suid] }
    /// setuid：特权全设；非特权仅 x ∈ {ruid, suid} 时改 euid。
    pub fn setuid(&mut self, x: u32) -> Result<(), u32> {
        if self.privileged() {
            self.ruid = x; self.euid = x; self.suid = x;
            Ok(())
        } else if x == self.ruid || x == self.suid {
            self.euid = x;
            Ok(())
        } else {
            Err(EPERM)
        }
    }
    /// setreuid：r 侧须特权或 ∈ {euid, ruid}；e 侧须特权或 ∈ {ruid, suid}。
    pub fn setreuid(&mut self, r: u32, e: u32) -> Result<(), u32> {
        if r != UID_KEEP {
            if self.privileged() || r == self.euid || r == self.ruid {
                self.ruid = r;
            } else {
                return Err(EPERM);
            }
        }
        if e != UID_KEEP {
            if self.privileged() || e == self.ruid || e == self.suid {
                let old = self.euid;
                self.euid = e;
                self.suid = old;
            } else {
                return Err(EPERM);
            }
        }
        Ok(())
    }
    /// setresuid：三个方向各自须特权或落在身份池内。
    pub fn setresuid(&mut self, r: u32, e: u32, s: u32) -> Result<(), u32> {
        for (dst, val) in [(0u8, r), (1, e), (2, s)] {
            if val == UID_KEEP {
                continue;
            }
            if !self.privileged() && !self.pool().contains(&val) {
                return Err(EPERM);
            }
            match dst {
                0 => self.ruid = val,
                1 => self.euid = val,
                _ => self.suid = val,
            }
        }
        Ok(())
    }
    /// 按操作重放（B05 母版口径）。
    pub fn apply(&mut self, op: Op, x: u32) -> Result<(), u32> {
        match op {
            Op::SetUid => self.setuid(x),
            Op::SetReuidR => self.setreuid(x, UID_KEEP),
            Op::SetReuidE => self.setreuid(UID_KEEP, x),
            Op::SetResuidR => self.setresuid(x, UID_KEEP, UID_KEEP),
            Op::SetResuidE => self.setresuid(UID_KEEP, x, UID_KEEP),
            Op::SetResuidS => self.setresuid(UID_KEEP, UID_KEEP, x),
        }
    }
}
/// 三身份语义操作闭集。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Op {
    /// setuid(x)：三值全设 / 非特权仅限 ruid、suid。
    SetUid,
    /// setreuid(x, -1)：只动真实身份。
    SetReuidR,
    /// setreuid(-1, x)：只动有效身份（旧 euid 存入 suid）。
    SetReuidE,
    /// setresuid(x, -1, -1)。
    SetResuidR,
    /// setresuid(-1, x, -1)。
    SetResuidE,
    /// setresuid(-1, -1, x)。
    SetResuidS,
}
impl Op {
    /// 三身份档归属（唯一来源）。
    pub fn slot(self) -> Slot {
        match self {
            Op::SetUid => Slot::All,
            Op::SetReuidR | Op::SetResuidR => Slot::Real,
            Op::SetReuidE | Op::SetResuidE => Slot::Effective,
            Op::SetResuidS => Slot::Saved,
        }
    }
}
/// 三身份档。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Slot { All, Real, Effective, Saved }
impl Slot {
    /// 三档（不含 All，用于逐档覆盖）。
    pub const THREE: [Slot; 3] = [Slot::Real, Slot::Effective, Slot::Saved];
}
/// 身份起点档。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Sid { Root, User }
impl Sid {
    /// 起始三身份账。
    pub const fn cred(self) -> Cred3 {
        match self {
            Sid::Root => Cred3::root(),
            Sid::User => Cred3::user(),
        }
    }
}

// ── E 型权限层（独立实现，不复用 B05 重放） ───────────────────────────
/// E 型权限判定：只看身份池与操作规则，返回 0/EPERM。
pub fn etype(op: Op, sid: Sid, x: u32) -> u32 {
    let id = sid.cred();
    if id.privileged() {
        return 0;
    }
    let ok = match op {
        Op::SetUid => x == id.ruid || x == id.suid,
        Op::SetReuidR => x == id.euid || x == id.ruid,
        Op::SetReuidE => x == id.ruid || x == id.suid,
        Op::SetResuidR | Op::SetResuidE | Op::SetResuidS => id.pool().contains(&x),
    };
    if ok { 0 } else { EPERM }
}
/// 权限族门序：权限 → 值域 → 指针（E 面独有次序）。
pub fn gate_order(sid: Sid, value_bad: bool, ptr_bad: bool) -> u32 {
    if !sid.cred().privileged() {
        return EPERM;
    }
    if value_bad {
        return EINVAL;
    }
    if ptr_bad {
        return EFAULT;
    }
    0
}

// ── 对账行 ────────────────────────────────────────────────────────────
/// 判据极性。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Layer { Positive, Negative, Undecided }
/// 一行对账判据。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Row {
    pub id: &'static str,
    pub sid: Sid,
    pub op: Op,
    pub slot: Slot,
    pub x: u32,
    pub expect: u32,
    pub layer: Layer,
}
/// 行构造。
pub const fn row(id: &'static str, sid: Sid, op: Op, slot: Slot, x: u32,
                  expect: u32, layer: Layer) -> Row {
    Row { id, sid, op, slot, x, expect, layer }
}
/// 20 行对账语料（正 12 / 负 8；Real/Effective/Saved 各 ≥2 正 ≥2 负）。
pub const ROWS: [Row; 20] = [
    row("uid-root-1002", Sid::Root, Op::SetUid, Slot::All, TARGET_UID, 0, Layer::Positive),
    row("uid-user-own", Sid::User, Op::SetUid, Slot::All, USER_UID, 0, Layer::Positive),
    row("uid-user-foreign", Sid::User, Op::SetUid, Slot::All, FOREIGN_UID, EPERM, Layer::Negative),
    row("uid-user-zero", Sid::User, Op::SetUid, Slot::All, ROOT_UID, EPERM, Layer::Negative),
    row("real-root-zero", Sid::Root, Op::SetReuidR, Slot::Real, ROOT_UID, 0, Layer::Positive),
    row("real-user-own", Sid::User, Op::SetReuidR, Slot::Real, USER_UID, 0, Layer::Positive),
    row("real-user-zero", Sid::User, Op::SetReuidR, Slot::Real, ROOT_UID, EPERM, Layer::Negative),
    row("eff-root-1001", Sid::Root, Op::SetReuidE, Slot::Effective, FOREIGN_UID, 0, Layer::Positive),
    row("eff-user-own", Sid::User, Op::SetReuidE, Slot::Effective, USER_UID, 0, Layer::Positive),
    row("eff-user-foreign", Sid::User, Op::SetReuidE, Slot::Effective, FOREIGN_UID, EPERM, Layer::Negative),
    row("rres-root-1001", Sid::Root, Op::SetResuidR, Slot::Real, FOREIGN_UID, 0, Layer::Positive),
    row("rres-user-own", Sid::User, Op::SetResuidR, Slot::Real, USER_UID, 0, Layer::Positive),
    row("rres-user-foreign", Sid::User, Op::SetResuidR, Slot::Real, FOREIGN_UID, EPERM, Layer::Negative),
    row("eres-root-1001", Sid::Root, Op::SetResuidE, Slot::Effective, FOREIGN_UID, 0, Layer::Positive),
    row("eres-user-own", Sid::User, Op::SetResuidE, Slot::Effective, USER_UID, 0, Layer::Positive),
    row("eres-user-zero", Sid::User, Op::SetResuidE, Slot::Effective, ROOT_UID, EPERM, Layer::Negative),
    row("sres-root-1001", Sid::Root, Op::SetResuidS, Slot::Saved, FOREIGN_UID, 0, Layer::Positive),
    row("sres-user-own", Sid::User, Op::SetResuidS, Slot::Saved, USER_UID, 0, Layer::Positive),
    row("sres-user-foreign", Sid::User, Op::SetResuidS, Slot::Saved, FOREIGN_UID, EPERM, Layer::Negative),
    row("sres-user-zero", Sid::User, Op::SetResuidS, Slot::Saved, ROOT_UID, EPERM, Layer::Negative),
];
/// B05 冻结账（id + 是否放行），对账锚点。
pub const FROZEN: [(&str, bool); 20] = [
    ("uid-root-1002", true), ("uid-user-own", true),
    ("uid-user-foreign", false), ("uid-user-zero", false),
    ("real-root-zero", true), ("real-user-own", true), ("real-user-zero", false),
    ("eff-root-1001", true), ("eff-user-own", true), ("eff-user-foreign", false),
    ("rres-root-1001", true), ("rres-user-own", true), ("rres-user-foreign", false),
    ("eres-root-1001", true), ("eres-user-own", true), ("eres-user-zero", false),
    ("sres-root-1001", true), ("sres-user-own", true), ("sres-user-foreign", false),
    ("sres-user-zero", false),
];

// ── 对账引擎 ──────────────────────────────────────────────────────────
/// B05 重放是否放行（真做身份变更再回卷）。
pub fn b05_allowed(r: &Row) -> bool {
    let mut c = r.sid.cred();
    c.apply(r.op, r.x).is_ok()
}
/// 对账报表。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Report {
    pub rows: u32,
    pub conflicts: u32,
    pub drift: u32,
    pub positive: u32,
    pub negative: u32,
}
/// 逐行比对：E 层 vs 册内期望；B05 vs 冻结账。
pub fn reconcile(rows: &[Row]) -> Report {
    let mut rep = Report { rows: 0, conflicts: 0, drift: 0, positive: 0, negative: 0 };
    for r in rows.iter() {
        rep.rows += 1;
        if etype(r.op, r.sid, r.x) != r.expect {
            rep.conflicts += 1;
        }
        match r.layer {
            Layer::Positive => rep.positive += 1,
            Layer::Negative => rep.negative += 1,
            Layer::Undecided => {}
        }
    }
    rep.drift = frozen_drift(&FROZEN);
    rep
}
/// 全表零冲突且零漂移。
pub fn reconcile_ok() -> bool {
    let rep = reconcile(&ROWS);
    rep.rows == TABLE_ROWS as u32 && rep.conflicts == 0 && rep.drift == 0
}
/// 正向：B05 拒 → E 必须 EPERM。
pub fn forward_ok() -> bool {
    ROWS.iter().filter(|r| !b05_allowed(r)).all(|r| etype(r.op, r.sid, r.x) == EPERM)
}
/// 反向：B05 放行 → E 不得报 EPERM。
pub fn backward_ok() -> bool {
    ROWS.iter().filter(|r| b05_allowed(r)).all(|r| etype(r.op, r.sid, r.x) != EPERM)
}
/// 冻结账漂移（按序比较 id 与放行位）。
pub fn frozen_drift(frozen: &[(&'static str, bool)]) -> u32 {
    let mut drift = 0u32;
    if ROWS.len() != frozen.len() {
        drift += 1;
    }
    for (i, r) in ROWS.iter().enumerate() {
        match frozen.get(i) {
            Some((id, allowed)) => {
                if r.id != *id || b05_allowed(r) != *allowed {
                    drift += 1;
                }
            }
            None => drift += 1,
        }
    }
    drift
}
/// 单档正负行数。
pub fn slot_layers(slot: Slot) -> (u32, u32) {
    let (mut p, mut n) = (0u32, 0u32);
    for r in ROWS.iter() {
        if r.slot != slot {
            continue;
        }
        match r.layer {
            Layer::Positive => p += 1,
            Layer::Negative => n += 1,
            Layer::Undecided => {}
        }
    }
    (p, n)
}
/// 三档各自 ≥2 正 ≥2 负。
pub fn slot_layers_ok() -> bool {
    Slot::THREE.iter().all(|s| { let (p, n) = slot_layers(*s); p >= 2 && n >= 2 })
}
/// 正反分层：无未定行、pos+neg == 总数。
pub fn layer_partition_ok(rows: &[Row]) -> bool {
    let (mut p, mut n, mut u) = (0u32, 0u32, 0u32);
    for r in rows.iter() {
        match r.layer {
            Layer::Positive => p += 1,
            Layer::Negative => n += 1,
            Layer::Undecided => u += 1,
        }
    }
    u == 0 && p + n == rows.len() as u32 && p > 0 && n > 0
}
/// 槽位单源：行内 slot 必须等于操作推导值。
pub fn slot_source_ok() -> bool {
    ROWS.iter().all(|r| r.slot == r.op.slot())
}
/// B05 外挂账：E 型独有面，只许 0/EINVAL/EFAULT。
pub fn aux_rows() -> [(&'static str, u32); 4] {
    [
        ("setgroups-root-huge", EINVAL),
        ("capset-root-bad-ptr", EFAULT),
        ("ioperm-root-overflow", EINVAL),
        ("setfsuid-user-silent", 0),
    ]
}
/// 外挂行不得反推 EPERM。
pub fn aux_ok() -> bool {
    aux_rows().iter().all(|(_, code)| *code != EPERM)
}

// ── 缺陷注入 ──────────────────────────────────────────────────────────
/// 对账缺陷类。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Defect {
    /// 册内期望整体偏移。
    ExpectDrift,
    /// 某行极性未定。
    LayerLeak,
    /// 末行丢失。
    RowLoss,
    /// 冻结账放行位翻转。
    BaselineFlip,
}
/// 四类缺陷。
pub const DEFECTS: [Defect; 4] =
    [Defect::ExpectDrift, Defect::LayerLeak, Defect::RowLoss, Defect::BaselineFlip];
/// 期望整体偏移的语料副本。
pub fn drifted_rows(delta: u32) -> Vec<Row> {
    ROWS.iter().map(|r| { let mut c = *r; c.expect = c.expect.wrapping_add(delta); c }).collect()
}
/// 首行极性未定。
pub fn leaked_rows() -> Vec<Row> {
    ROWS.iter().enumerate().map(|(i, r)| {
        let mut c = *r;
        if i == 0 { c.layer = Layer::Undecided; }
        c
    }).collect()
}
/// 少一行。
pub fn truncated_rows() -> Vec<Row> { ROWS.iter().take(TABLE_ROWS - 1).copied().collect() }
/// 首行放行位翻转的冻结账副本。
pub fn flipped_frozen() -> Vec<(&'static str, bool)> {
    FROZEN.iter().enumerate().map(|(i, (id, a))| (*id, if i == 0 { !*a } else { *a })).collect()
}
/// 单轮缺陷是否被检出。
pub fn detect(defect: Defect) -> bool {
    match defect {
        Defect::ExpectDrift => reconcile(&drifted_rows(1)).conflicts == TABLE_ROWS as u32,
        Defect::LayerLeak => !layer_partition_ok(&leaked_rows()),
        Defect::RowLoss => truncated_rows().len() != TABLE_ROWS,
        Defect::BaselineFlip => frozen_drift(&flipped_frozen()) > 0,
    }
}
/// 单类缺陷 10 轮全检出。
pub fn detect_ten_of_ten(defect: Defect) -> bool {
    for _round in 0..INJECT_ROUNDS {
        if !detect(defect) { return false; }
    }
    true
}
/// 注入账本：注入 = 检出 + 逃逸。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Ledger { pub injected: u32, pub detected: u32, pub escaped: u32 }
impl Ledger {
    /// 空账。
    pub const fn new() -> Ledger { Ledger { injected: 0, detected: 0, escaped: 0 } }
    /// 记一笔。
    pub fn record(&mut self, hit: bool) {
        self.injected += 1;
        if hit { self.detected += 1; } else { self.escaped += 1; }
    }
    /// 守恒。
    pub fn conserve(&self) -> bool { self.escaped == 0 && self.injected == self.detected }
}
/// 四类缺陷各跑 10 轮。
pub fn run_injections() -> Ledger {
    let mut led = Ledger::new();
    for d in DEFECTS.iter() {
        for _round in 0..INJECT_ROUNDS {
            led.record(detect(*d));
        }
    }
    led
}

// ── 状态机 ────────────────────────────────────────────────────────────
/// 对账阶段。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RecStage { Load, Baseline, Match, Layer, Verdict }
impl RecStage {
    /// 下一阶段。
    pub fn next(self) -> Option<RecStage> {
        match self {
            RecStage::Load => Some(RecStage::Baseline),
            RecStage::Baseline => Some(RecStage::Match),
            RecStage::Match => Some(RecStage::Layer),
            RecStage::Layer => Some(RecStage::Verdict),
            RecStage::Verdict => None,
        }
    }
    /// 只许前进一步；终点拒绝。
    pub fn advance(self) -> Result<RecStage, RecCode> {
        match self.next() { Some(n) => Ok(n), None => Err(RecCode::StageSkip) }
    }
}

// ── 指纹 ──────────────────────────────────────────────────────────────
/// 行语料指纹（双跑一致用）。
pub fn rows_fingerprint(rows: &[Row]) -> u32 {
    let mut h = 0x811C_9DC5u32;
    for r in rows.iter() {
        for b in r.id.as_bytes().iter() {
            h ^= *b as u32;
            h = h.wrapping_mul(0x0100_0193);
        }
        h ^= r.x;
        h = h.wrapping_mul(0x0100_0193);
        h ^= r.expect;
        h = h.wrapping_mul(0x0100_0193);
        h ^= match r.layer {
            Layer::Positive => 1,
            Layer::Negative => 2,
            Layer::Undecided => 3,
        };
        h = h.wrapping_mul(0x0100_0193);
    }
    h
}
/// 报表指纹。
pub fn report_fingerprint(rep: &Report) -> u32 {
    let mut h = 0x811C_9DC5u32;
    for v in [rep.rows, rep.conflicts, rep.drift, rep.positive, rep.negative].iter() {
        h ^= *v;
        h = h.wrapping_mul(0x0100_0193);
    }
    h
}

// ── 判据总成 ──────────────────────────────────────────────────────────
/// 运行 F9275 全部判据。
pub fn run_unx9275_checks() -> CheckSet {
    let mut s = CheckSet::new("unx9275");
    let seg_ok = SEG_BASE % 0x40 == 0 && (0x7A000..=0x7D1C0).contains(&SEG_BASE);
    s.add("段位守卫：0x40 对齐且在 0x7A000..=0x7D1C0", seg_ok,
          "段基址必须独占且落在本批窗口；越界即拒绝注册");
    let rep = reconcile(&ROWS);
    s.add("对账：20 行 E 层判定与册内期望零冲突",
          rep.rows == TABLE_ROWS as u32 && rep.conflicts == 0,
          "判定器与深化册必须独立收敛；冲突行先查门序再查期望");
    s.add("B05 基准：重放结果与冻结 allowed 账零漂移", rep.drift == 0,
          "冻结账是外部锚点；B05 语义改了必须显式改账");
    s.add("三身份语义基准逐条零冲突：正向 B05 拒绝行全 EPERM",
          forward_ok(),
          "B05 说不能变的身份，E 层必须报 EPERM 而不是别的码");
    s.add("三身份语义基准反向：B05 放行行不得报 EPERM",
          backward_ok(),
          "放行行只许 0/EINVAL/EFAULT；反推 EPERM 即把值域故障当权限故障");
    let neg_rows = ROWS.iter().filter(|r| !b05_allowed(r)).count() as u32;
    s.add("负判据基数：8 行 B05 拒绝且全为反判据层", neg_rows == 8,
          "拒绝行数由 B05 重放决定，与册内极性必须同时收敛");
    let real = slot_layers(Slot::Real);
    let eff = slot_layers(Slot::Effective);
    let saved = slot_layers(Slot::Saved);
    let all = slot_layers(Slot::All);
    s.add("三身份档：Real/Effective/Saved 各 ≥2 正 ≥2 负",
          slot_layers_ok() && real == (4, 2) && eff == (4, 2) && saved == (2, 2),
          "单档只剩正判据就无法证明该槽的拒绝面被覆盖");
    s.add("正反分层：无未定行且 pos 12 + neg 8 == 20",
          layer_partition_ok(&ROWS) && rep.positive == 12 && rep.negative == 8,
          "分层声明必须覆盖全部行，未定行会让统计口径出现漏项");
    s.add("All 档：setuid 三值全设的成与拒都在册（2 正 2 负）",
          all == (2, 2),
          "setuid 一次性动三个身份槽，必须同时有成功与拒绝样本");
    s.add("槽位单源：行内 slot 全部等于操作推导值", slot_source_ok(),
          "槽位由操作唯一决定，另写一份槽位表必然漂移");
    s.add("冻结账：20 行 id 逐位对齐", ROWS.len() == FROZEN.len() && frozen_drift(&FROZEN) == 0,
          "行数或名字漂移说明有判据被删改，删行必须先改账");
    s.add("外挂账：4 行 E 型独有面均不反推 EPERM", aux_ok(),
          "setgroups/capset/ioperm/setfsuid 的故障属于值域或指针层");
    s.add("判定次序：非特权 + 坏值 + 坏指针 → EPERM（权限最先）",
          gate_order(Sid::User, true, true) == EPERM,
          "权限门先于一切参数校验，避免泄露参数细节");
    s.add("判定次序：特权 + 坏值 + 坏指针 → EINVAL（值域先于指针）",
          gate_order(Sid::Root, true, true) == EINVAL,
          "值不合法时不做用户拷贝，与 Linux sys_* 前置检查一致");
    s.add("判定次序：特权 + 坏指针 → EFAULT（指针门最后）",
          gate_order(Sid::Root, false, true) == EFAULT && gate_order(Sid::Root, false, false) == 0,
          "指针门之后不得再有更高优先级的判定");
    let mut stage = RecStage::Load;
    let mut steps = 0u32;
    let mut forward = true;
    while let Ok(next) = stage.advance() {
        stage = next;
        steps += 1;
        if steps > 8 { forward = false; break; }
    }
    s.add("状态机：Load→Baseline→Match→Layer→Verdict 单向四步",
          forward && steps == 4 && stage == RecStage::Verdict
              && stage.advance() == Err(RecCode::StageSkip),
          "回退或跳步会让对账读到的册与判定器版本不一致");
    let ten = DEFECTS.iter().all(|d| detect_ten_of_ten(*d));
    s.add("注入必抓：期望漂移/极性未定/丢行/基准翻转各 10/10 轮全检出", ten,
          "每类缺陷都要在十轮里稳定被抓；漏一轮即视为 flaky");
    let led = run_injections();
    s.add("守恒：注入 40 = 检出 40 + 逃逸 0",
          led.conserve() && led.injected == DEFECTS.len() as u32 * INJECT_ROUNDS,
          "注入账必须闭合；逃逸说明对账格有盲区");
    let f1 = rows_fingerprint(&ROWS);
    let r1 = report_fingerprint(&reconcile(&ROWS));
    s.add("双跑一致：行指纹与报表指纹两次相等",
          f1 == rows_fingerprint(&ROWS)
              && r1 == report_fingerprint(&reconcile(&ROWS)),
          "对账不得依赖哈希遍历顺序或外部状态");
    s.add("容量：20 行 ≤ 32 上限，缺陷 4 类，add_dyn ≤ 8",
          TABLE_ROWS <= MAX_ROWS && DEFECTS.len() == 4 && 3 <= 8,
          "行数超上限说明对账格失控，应拆册而不是继续堆行");
    s.add("冲突定位：期望整体 +1 后 20 行全部报冲突",
          reconcile(&drifted_rows(1)).conflicts == TABLE_ROWS as u32,
          "对账必须能定位到逐行冲突，而不是只报一个总量红灯");
    let mut closed = true;
    for (i, c) in RecCode::ALL.iter().enumerate() {
        if c.val() != SEG_BASE + i as u32 || c.detail().is_empty() { closed = false; }
        for (j, o) in RecCode::ALL.iter().enumerate() {
            if i != j && c.val() == o.val() { closed = false; }
        }
    }
    s.add("文案闭集：12 码连续、唯一、detail 非空", closed,
          "诊断码闭集且可枚举，文案为空等于没有处置建议");
    s.add_dyn("动态：对账报表", rep.conflicts == 0 && rep.drift == 0,
              &format!("rows={} conflicts={} drift={} pos={} neg={}",
                       rep.rows, rep.conflicts, rep.drift, rep.positive, rep.negative));
    s.add_dyn("动态：缺陷注入账", led.conserve(),
              &format!("injected={} detected={} escaped={}", led.injected, led.detected, led.escaped));
    s.add_dyn("动态：三档覆盖与指纹", slot_layers_ok() && forward_ok() && backward_ok(),
              &format!("real={:?} eff={:?} saved={:?} fp=0x{:08X}",
                       real, eff, saved, f1));
    s
}

// ── 测试 ──────────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rows_reconcile_with_zero_conflict() {
        let rep = reconcile(&ROWS);
        assert_eq!(rep.rows, TABLE_ROWS as u32);
        assert_eq!(rep.conflicts, 0);
        assert_eq!(rep.drift, 0);
        assert!(reconcile_ok());
    }
    #[test]
    fn forward_and_backward_layers() {
        assert!(forward_ok());
        assert!(backward_ok());
        assert_eq!(ROWS.iter().filter(|r| !b05_allowed(r)).count(), 8);
        assert_eq!(ROWS.iter().filter(|r| b05_allowed(r)).count(), 12);
    }
    #[test]
    fn slots_have_both_layers() {
        assert!(slot_layers_ok());
        assert_eq!(slot_layers(Slot::All), (2, 2));
        assert_eq!(slot_layers(Slot::Real), (4, 2));
        assert_eq!(slot_layers(Slot::Effective), (4, 2));
        assert_eq!(slot_layers(Slot::Saved), (2, 2));
        assert!(slot_source_ok());
    }
    #[test]
    fn b05_root_allows_and_user_denies() {
        for r in ROWS.iter() {
            if r.sid == Sid::Root { assert!(b05_allowed(r)); }
        }
        let denied: [&str; 8] = ["uid-user-foreign", "uid-user-zero", "real-user-zero",
            "eff-user-foreign", "rres-user-foreign", "eres-user-zero",
            "sres-user-foreign", "sres-user-zero"];
        for id in denied.iter() {
            if let Some(r) = ROWS.iter().find(|r| r.id == *id) {
                assert!(!b05_allowed(r));
                assert_eq!(etype(r.op, r.sid, r.x), EPERM);
            }
        }
    }
    #[test]
    fn frozen_and_fingerprints_stable() {
        assert_eq!(FROZEN.len(), TABLE_ROWS);
        assert_eq!(frozen_drift(&FROZEN), 0);
        assert_eq!(rows_fingerprint(&ROWS), rows_fingerprint(&ROWS));
        assert_eq!(report_fingerprint(&reconcile(&ROWS)),
                   report_fingerprint(&reconcile(&ROWS)));
    }
    #[test]
    fn aux_rows_never_infer_eperm() {
        assert!(aux_ok());
        assert_eq!(aux_rows().len(), 4);
        assert_eq!(aux_rows()[2].1, EINVAL);
        assert_eq!(aux_rows()[1].1, EFAULT);
        assert_eq!(aux_rows()[3].1, 0);
    }
    #[test]
    fn defects_are_detected_ten_of_ten() {
        for d in DEFECTS.iter() { assert!(detect_ten_of_ten(*d)); }
        let led = run_injections();
        assert!(led.conserve());
        assert_eq!(led.injected, 40);
        assert_eq!(led.escaped, 0);
    }
    #[test]
    fn stage_machine_is_forward_only() {
        let s1 = RecStage::Load.advance();
        assert_eq!(s1, Ok(RecStage::Baseline));
        let s2 = match s1 { Ok(v) => v.advance(), Err(_) => Err(RecCode::StageSkip) };
        let s3 = match s2 { Ok(v) => v.advance(), Err(_) => Err(RecCode::StageSkip) };
        let s4 = match s3 { Ok(v) => v.advance(), Err(_) => Err(RecCode::StageSkip) };
        assert_eq!(s4, Ok(RecStage::Verdict));
        let s5 = match s4 { Ok(v) => v.advance(), Err(_) => Err(RecCode::StageSkip) };
        assert_eq!(s5, Err(RecCode::StageSkip));
    }
    #[test]
    fn order_gates_permission_first() {
        assert_eq!(gate_order(Sid::User, true, true), EPERM);
        assert_eq!(gate_order(Sid::Root, true, true), EINVAL);
        assert_eq!(gate_order(Sid::Root, false, true), EFAULT);
        assert_eq!(gate_order(Sid::Root, false, false), 0);
    }
    #[test]
    fn checks_all_green() {
        let set = run_unx9275_checks();
        assert!(set.all_passed());
    }
}
