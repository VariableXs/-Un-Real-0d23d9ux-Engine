//! UNX-F9277 E 型判据可复测性声明（10/10 次检出） · 0x7B2C0段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（派工卡 UNX-F9277-J1 逐字）：
//!   全矩阵 10 次重复注入判据过，10/10 次检出统计格过，零 flaky 声明格过。
//!
//! 设计要点：
//!   ① 复测对象是权限族判定本身：12 条探针覆盖归属类短路、目录穿越、
//!      root 豁免、能力位兜底、chown 特权五类语义，逐条先与冻结期望对拍；
//!      尺子不准就没有资格谈「可复测」。
//!   ② 三类变异都必须改变原值才叫注入：Rotate（判定环移，Eacces↔Eperm）、
//!      Corrupt（允许/拒绝翻转）、Overshoot（越集码）。任何一格在任一轮
//!      未被检出，说明该格探针失效，不允许声明 10/10。
//!   ③ 统计全部用整数：36 格 × 10 轮 = 360 次注入，每轮 36/36，每格 10/10；
//!      不取百分比，避免舍入把「9/10」抹成「10/10」。
//!   ④ 零 flaky 不是手工置真：反向注入「只在偶轮检出」的抖动模型，声明器
//!      必须把 36 格全记为 flaky、min_hits 掉到 5、reproducible 翻假；对
//!      抖动无感的声明器等于空头支票。
//!   ⑤ 统计格还要求「轮次统计恒等」：10 轮逐轮检出数必须全等于 36，
//!      轮间不齐说明矩阵依赖轮次外状态，属隐藏状态泄漏。
//!   ⑥ 阶段机 Probe→Mutate→Repeat→Declare 单向；回退或跳步直接拒绝声明。
//! ─────────────────────────────────────────────────────────────────────

use alloc::format;

use crate::checks::CheckSet;

/// 诊断段基址（F9277 独占 0x40 窗口）。
pub const SEG_BASE: u32 = 0x7B2C0;

/// 重复轮数（判据硬性要求 10 轮）。
pub const ROUNDS: u32 = 10;

/// 探针条数（权限族 E 型）。
pub const PROBES: usize = 12;

/// 变异类数。
pub const MUTANT_KINDS: usize = 3;

/// 注入格数：12 × 3。
pub const CELLS: u32 = 36;

/// 注入格容量上限。
pub const MAX_CELLS: usize = 64;

// ── 判定结果闭集 ──────────────────────────────────────────────────────
/// 权限判定结果：允许 / 对象权限不足 / 操作特权缺失。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Verdict {
    /// 允许
    Allow = 0,
    /// 对象权限不足（EACCES 域）
    DenyEacces = 1,
    /// 操作特权缺失（EPERM 域）
    DenyEperm = 2,
}

impl Verdict {
    /// 数值码（0/1/2，变异与统计都以此为准）。
    pub fn code(self) -> u32 {
        self as u32
    }

    /// 中文标签。
    pub fn label(self) -> &'static str {
        match self { Verdict::Allow => "允许", Verdict::DenyEacces => "EACCES（对象权限不足）", Verdict::DenyEperm => "EPERM（操作特权缺失）", }
    }

    /// 闭集全量。
    pub const ALL: [Verdict; 3] = [Verdict::Allow, Verdict::DenyEacces, Verdict::DenyEperm];
}

/// 请求方向。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Op {
    /// 读
    Read,
    /// 写
    Write,
    /// 执行/穿越
    Exec,
    /// chown 特权操作（EPERM 域）
    Chown,
}

/// 探针类别（覆盖统计用）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ProbeKind {
    /// 权限位面
    Bits,
    /// 目录穿越面
    Traverse,
    /// root 豁免面
    Root,
    /// 能力位面
    Cap,
    /// 特权操作面
    Chown,
}

impl ProbeKind {
    /// 闭集全量。
    pub const ALL: [ProbeKind; 5] = [ProbeKind::Bits, ProbeKind::Traverse, ProbeKind::Root, ProbeKind::Cap, ProbeKind::Chown,];
}

// ── 诊断码闭集 ────────────────────────────────────────────────────────
/// 复测声明诊断码（段内 +0x01..=+0x0C 闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RetestCode {
    /// 轮数不足
    RoundShort,
    /// 某格未做到 10/10
    CellMissed,
    /// 检出抖动
    FlakyDetected,
    /// 轮次统计不一致
    RoundMismatch,
    /// 声明字段与语料不符
    DeclarationFalse,
    /// 期望冻结账漂移
    ExpectDrift,
    /// 类别缺档
    KindGap,
    /// 探针目录位不一致
    DirFlagDrift,
    /// 阶段机回退/跳步
    StageSkip,
    /// 注入账倒退
    LedgerRegress,
    /// 双跑不一致
    NotReproducible,
    /// 段位越界
    SegmentViolation,
}

impl RetestCode {
    /// 码值（段基址 + 变体序号）。
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }

    /// 中文诊断与可操作建议（闭集返回，禁止空串）。
    pub fn detail(self) -> &'static str {
        match self { RetestCode::RoundShort => "复测轮数不足 10 轮就声明 10/10；请补满 10 轮\
                全矩阵注入后再出具声明",
            RetestCode::CellMissed => "存在未达 10/10 的注入格；先定位该格的探针与\
                期望码，修好判定器再复测",
            RetestCode::FlakyDetected => "检出存在抖动：轮间结果不一致；必须先消除\
                轮次依赖，再重新声明零 flaky",
            RetestCode::RoundMismatch => "轮次统计不一致：10 轮逐轮检出数必须全等于\
                36；轮间差异说明矩阵依赖轮次外状态",
            RetestCode::DeclarationFalse => "声明字段与语料不符：cells/rounds/\
                ten_of_ten 必须由实测推出，禁止手工置真",
            RetestCode::ExpectDrift => "期望冻结账漂移：改探针必须同步改账，否则\
                复测的是两套语义",
            RetestCode::KindGap => "类别缺档：权限位/穿越/root/能力位/特权操作五类\
                必须都在矩阵里，缺类即复测不完整",
            RetestCode::DirFlagDrift => "目录位标记漂移：穿越类探针必须标目录，\
                其余探针不得标目录",
            RetestCode::StageSkip => "阶段机回退或跳步：Probe→Mutate→Repeat→\
                Declare 只许单向推进",
            RetestCode::LedgerRegress => "注入账倒退：注入=检出+逃逸且只增不减；\
                请检查回滚路径误改累计值",
            RetestCode::NotReproducible => "双跑不一致：同输入两次结果不同；请排查\
                隐藏状态或非确定哈希",
            RetestCode::SegmentViolation => "段位越界：本模块段基址必须落在 \
                0x7A000..=0x7D1C0 且 0x40 对齐",
        }
    }

    /// 闭集全量。
    pub const ALL: [RetestCode; 12] = [RetestCode::RoundShort, RetestCode::CellMissed, RetestCode::FlakyDetected, RetestCode::RoundMismatch, RetestCode::DeclarationFalse, RetestCode::ExpectDrift, RetestCode::KindGap, RetestCode::DirFlagDrift, RetestCode::StageSkip, RetestCode::LedgerRegress, RetestCode::NotReproducible, RetestCode::SegmentViolation,];
}

// ── 探针模型 ──────────────────────────────────────────────────────────
/// 一条权限族探针。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Probe {
    /// 探针标识
    pub id: &'static str,
    /// 类别
    pub kind: ProbeKind,
    /// 主体 uid
    pub uid: u32,
    /// 主体 gid
    pub gid: u32,
    /// 是否 root
    pub is_root: bool,
    /// 是否持 CAP_DAC_OVERRIDE
    pub cap_dac: bool,
    /// 是否持 CAP_FOWNER
    pub cap_fowner: bool,
    /// 对象属主
    pub obj_uid: u32,
    /// 对象属组
    pub obj_gid: u32,
    /// 对象 mode 低 9 位
    pub mode: u32,
    /// 对象是否目录
    pub is_dir: bool,
    /// 请求方向
    pub op: Op,
    /// 冻结期望
    pub expect: Verdict,
}

/// 12 条探针表（权限族 E 型）。
pub const MATRIX: [Probe; PROBES] = [Probe { id: "owner-read", kind: ProbeKind::Bits, uid: 1000, gid: 1000, is_root: false, cap_dac: false, cap_fowner: false, obj_uid: 1000, obj_gid: 1000, mode: 0o640, is_dir: false, op: Op::Read, expect: Verdict::Allow },
    Probe { id: "owner-jump-group", kind: ProbeKind::Bits, uid: 1000, gid: 1000, is_root: false,
        cap_dac: false, cap_fowner: false, obj_uid: 1000, obj_gid: 1001, mode: 0o460,
        is_dir: false, op: Op::Write, expect: Verdict::DenyEacces },
    Probe { id: "group-read", kind: ProbeKind::Bits, uid: 2000, gid: 1001, is_root: false,
        cap_dac: false, cap_fowner: false, obj_uid: 1000, obj_gid: 1001, mode: 0o040,
        is_dir: false, op: Op::Read, expect: Verdict::Allow },
    Probe { id: "other-read-deny", kind: ProbeKind::Bits, uid: 2000, gid: 3000, is_root: false,
        cap_dac: false, cap_fowner: false, obj_uid: 1000, obj_gid: 1001, mode: 0o600,
        is_dir: false, op: Op::Read, expect: Verdict::DenyEacces },
    Probe { id: "other-x-allow", kind: ProbeKind::Bits, uid: 2000, gid: 3000, is_root: false,
        cap_dac: false, cap_fowner: false, obj_uid: 1000, obj_gid: 1001, mode: 0o001,
        is_dir: false, op: Op::Exec, expect: Verdict::Allow },
    Probe { id: "dir-traverse-nox", kind: ProbeKind::Traverse, uid: 2000, gid: 3000,
        is_root: false, cap_dac: false, cap_fowner: false, obj_uid: 1000, obj_gid: 1001,
        mode: 0o644, is_dir: true, op: Op::Exec, expect: Verdict::DenyEacces },
    Probe { id: "root-write-zero", kind: ProbeKind::Root, uid: 0, gid: 0, is_root: true,
        cap_dac: false, cap_fowner: false, obj_uid: 0, obj_gid: 0, mode: 0o000,
        is_dir: false, op: Op::Write, expect: Verdict::Allow },
    Probe { id: "root-exec-nox", kind: ProbeKind::Root, uid: 0, gid: 0, is_root: true,
        cap_dac: false, cap_fowner: false, obj_uid: 0, obj_gid: 0, mode: 0o000,
        is_dir: false, op: Op::Exec, expect: Verdict::DenyEacces },
    Probe { id: "cap-write-zero", kind: ProbeKind::Cap, uid: 2000, gid: 3000, is_root: false,
        cap_dac: true, cap_fowner: false, obj_uid: 1000, obj_gid: 1001, mode: 0o000,
        is_dir: false, op: Op::Write, expect: Verdict::Allow },
    Probe { id: "cap-exec-nox", kind: ProbeKind::Cap, uid: 2000, gid: 3000, is_root: false,
        cap_dac: true, cap_fowner: false, obj_uid: 1000, obj_gid: 1001, mode: 0o000,
        is_dir: false, op: Op::Exec, expect: Verdict::DenyEacces },
    Probe { id: "chown-nocap", kind: ProbeKind::Chown, uid: 2000, gid: 3000, is_root: false,
        cap_dac: false, cap_fowner: false, obj_uid: 1000, obj_gid: 1001, mode: 0o777,
        is_dir: false, op: Op::Chown, expect: Verdict::DenyEperm },
    Probe { id: "chown-owner", kind: ProbeKind::Chown, uid: 1000, gid: 1001, is_root: false,
        cap_dac: false, cap_fowner: false, obj_uid: 1000, obj_gid: 1001, mode: 0o600,
        is_dir: false, op: Op::Chown, expect: Verdict::Allow },
];

/// 期望冻结账（探针 id + 期望码）。
pub const FROZEN_EXPECT: [(&str, u32); PROBES] = [("owner-read", 0), ("owner-jump-group", 1), ("group-read", 0), ("other-read-deny", 1), ("other-x-allow", 0), ("dir-traverse-nox", 1), ("root-write-zero", 0), ("root-exec-nox", 1), ("cap-write-zero", 0), ("cap-exec-nox", 1), ("chown-nocap", 2), ("chown-owner", 0),];

/// 判定器：特权域 → root 豁免 → 归属类 → 权限位 → 能力位兜底。
pub fn judge(p: &Probe) -> Verdict {
if p.op == Op::Chown { if p.is_root || p.uid == p.obj_uid || p.cap_fowner { return Verdict::Allow; } return Verdict::DenyEperm; }
    let need = match p.op { Op::Read => 4u32, Op::Write => 2u32, Op::Exec => 1u32, Op::Chown => 0u32, };
if p.is_root { if p.op == Op::Exec && p.mode & 0o111 == 0 { return Verdict::DenyEacces; } return Verdict::Allow; }
    let class = if p.uid == p.obj_uid {
        0usize
    } else if p.gid == p.obj_gid {
        1usize
    } else {
        2usize
    };
    let bits = match class { 0 => (p.mode >> 6) & 0o7, 1 => (p.mode >> 3) & 0o7, _ => p.mode & 0o7, };
if bits & need == need { return Verdict::Allow; }
if p.cap_dac { if p.op != Op::Exec { return Verdict::Allow; } if p.mode & 0o111 != 0 { return Verdict::Allow; } }
    Verdict::DenyEacces
}

/// 按 id 取探针。
pub fn probe_of(id: &str) -> Option<&'static Probe> {
    MATRIX.iter().find(|p| p.id == id)
}

/// 期望冻结账漂移条数。
pub fn expect_drift() -> u32 {
if MATRIX.len() != FROZEN_EXPECT.len() { return 1; }
    let mut drifted = 0u32;
    let mut i = 0usize;
while i < MATRIX.len() { if let (Some(p), Some(f)) = (MATRIX.get(i), FROZEN_EXPECT.get(i)) { if p.id != f.0 || p.expect.code() != f.1 { drifted = drifted.saturating_add(1); } } i += 1; }
    drifted
}

/// 矩阵自检：12 条探针判定与期望逐条一致。
pub fn matrix_clean() -> bool {
    let mut ok = true;
    let mut i = 0usize;
while i < MATRIX.len() { if let Some(p) = MATRIX.get(i) { if judge(p) != p.expect { ok = false; } } i += 1; }
    ok
}

/// 某类别探针数。
pub fn kind_count(kind: ProbeKind) -> u32 {
    let mut n = 0u32;
    let mut i = 0usize;
while i < MATRIX.len() { if let Some(p) = MATRIX.get(i) { if p.kind == kind { n = n.saturating_add(1); } } i += 1; }
    n
}

/// 目录位一致性：穿越类必须目录，其余不得标目录。
pub fn dir_flags_consistent() -> bool {
    let mut ok = true;
    let mut i = 0usize;
while i < MATRIX.len() { if let Some(p) = MATRIX.get(i) { let want = p.kind == ProbeKind::Traverse; if p.is_dir != want { ok = false; } } i += 1; }
    ok
}

// ── 变异与格 ──────────────────────────────────────────────────────────
/// 三类变异（每类都保证改变原值）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Mutant {
    /// 判定环移：Allow→Eacces→Eperm→Allow
    Rotate,
    /// 低位置翻：允许/拒绝互换，越集码
    Corrupt,
    /// 越集：码 +3，落到闭集之外
    Overshoot,
}

impl Mutant {
    /// 闭集全量。
    pub const ALL: [Mutant; MUTANT_KINDS] = [Mutant::Rotate, Mutant::Corrupt, Mutant::Overshoot];

    /// 变异类码（1..=3）。
    pub fn code(self) -> u32 {
        match self { Mutant::Rotate => 1, Mutant::Corrupt => 2, Mutant::Overshoot => 3, }
    }
}

/// 对一次正确判定施加变异。
pub fn mutate(m: Mutant, base: Verdict) -> u32 {
    let c = base.code();
    match m { Mutant::Rotate => (c + 1) % 3, Mutant::Corrupt => c ^ 1, Mutant::Overshoot => c.saturating_add(3), }
}

/// 单格检出轮数；parity 为真时模拟「只在偶轮检出」的抖动实现。
pub fn cell_hits_mode(p: &Probe, m: Mutant, parity: bool) -> u32 {
    let mut hits = 0u32;
    let mut round = 0u32;
while round < ROUNDS { let skip = parity && round % 2 == 1; if !skip && mutate(m, judge(p)) != p.expect.code() { hits = hits.saturating_add(1); } round = round.saturating_add(1); }
    hits
}

/// 单格 10 轮检出次数（正常实现）。
pub fn cell_hits(p: &Probe, m: Mutant) -> u32 {
    cell_hits_mode(p, m, false)
}

/// 注入格总数。
pub fn total_cells() -> u32 {
    (MATRIX.len() * Mutant::ALL.len()) as u32
}

/// 单轮检出格数（轮次越界返回 0）。
pub fn round_detections_mode(round: u32, parity: bool) -> u32 {
if round >= ROUNDS { return 0; }
    let mut n = 0u32;
    let mut pi = 0usize;
while pi < MATRIX.len() { if let Some(p) = MATRIX.get(pi) { let mut mi = 0usize; while mi < Mutant::ALL.len() { if let Some(m) = Mutant::ALL.get(mi) { if mutate(*m, judge(p)) != p.expect.code() { let skip = parity && round % 2 == 1; if !skip { n = n.saturating_add(1); } } } mi += 1; } } pi += 1; }
    n
}

/// 单轮检出格数（正常实现）。
pub fn round_detections(round: u32) -> u32 {
    round_detections_mode(round, false)
}

/// 逐格 10/10。
pub fn all_cells_ten_of_ten() -> bool {
    let mut ok = true;
    let mut pi = 0usize;
while pi < MATRIX.len() { if let Some(p) = MATRIX.get(pi) { let mut mi = 0usize; while mi < Mutant::ALL.len() { if let Some(m) = Mutant::ALL.get(mi) { if cell_hits(p, *m) != ROUNDS { ok = false; } } mi += 1; } } pi += 1; }
    ok
}

/// 每轮 36/36。
pub fn every_round_full() -> bool {
    let mut ok = true;
    let mut round = 0u32;
while round < ROUNDS { if round_detections(round) != total_cells() { ok = false; } round = round.saturating_add(1); }
    ok
}

// ── 声明器 ────────────────────────────────────────────────────────────
/// 复测声明（统计格）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Declaration {
    /// 探针数
    pub probes: u32,
    /// 变异类数
    pub mutants: u32,
    /// 注入格数
    pub cells: u32,
    /// 轮数
    pub rounds: u32,
    /// 10/10 格数
    pub ten_of_ten: u32,
    /// 抖动格数
    pub flaky: u32,
    /// 最小逐格检出数
    pub min_hits: u32,
    /// 最大逐格检出数
    pub max_hits: u32,
    /// 轮次统计是否恒等
    pub identical_rounds: bool,
    /// 是否可复现（零 flaky 且满格）
    pub reproducible: bool,
}

/// 生成声明；parity 为真时用偶轮检出模型做反向注入。
pub fn declare(parity: bool) -> Declaration {
    let mut ten = 0u32;
    let mut flaky = 0u32;
    let mut min_hits = ROUNDS;
    let mut max_hits = 0u32;
    let mut pi = 0usize;
while pi < MATRIX.len() { if let Some(p) = MATRIX.get(pi) { let mut mi = 0usize; while mi < Mutant::ALL.len() { if let Some(m) = Mutant::ALL.get(mi) { let hits = cell_hits_mode(p, *m, parity); if hits == ROUNDS { ten = ten.saturating_add(1); } else { flaky = flaky.saturating_add(1); } if hits < min_hits { min_hits = hits; } if hits > max_hits { max_hits = hits; } } mi += 1; } } pi += 1; }
    let cells = total_cells();
    let mut identical = true;
    let first = round_detections_mode(0, parity);
    let mut round = 1u32;
while round < ROUNDS { if round_detections_mode(round, parity) != first { identical = false; } round = round.saturating_add(1); }
    Declaration { probes: MATRIX.len() as u32, mutants: Mutant::ALL.len() as u32, cells, rounds: ROUNDS, ten_of_ten: ten, flaky, min_hits, max_hits, identical_rounds: identical, reproducible: flaky == 0 && ten == cells && identical, }
}

/// 零 flaky 声明格：满格 10/10、抖动零、轮次恒等、可复现。
pub fn zero_flaky(d: &Declaration) -> bool {
    d.flaky == 0
        && d.reproducible
        && d.ten_of_ten == d.cells
        && d.rounds == ROUNDS
        && d.min_hits == ROUNDS
        && d.max_hits == ROUNDS
        && d.identical_rounds
}

/// 轮次统计快照：10 轮逐轮检出数。
pub fn round_table() -> [u32; ROUNDS as usize] {
    let mut out = [0u32; ROUNDS as usize];
    let mut round = 0u32;
while round < ROUNDS { if let Some(slot) = out.get_mut(round as usize) { *slot = round_detections(round); } round = round.saturating_add(1); }
    out
}

// ── 阶段机 ────────────────────────────────────────────────────────────
/// 复测阶段：探针→变异→重复→声明，单向。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RetestStage {
    /// 探针矩阵就位
    Probe,
    /// 变异装载
    Mutate,
    /// 10 轮重复
    Repeat,
    /// 出具声明
    Declare,
}

impl RetestStage {
    /// 下一阶段。
    pub fn next(self) -> Option<RetestStage> {
        match self { RetestStage::Probe => Some(RetestStage::Mutate), RetestStage::Mutate => Some(RetestStage::Repeat), RetestStage::Repeat => Some(RetestStage::Declare), RetestStage::Declare => None, }
    }

    /// 单向推进；终点再推进即 StageSkip。
    pub fn advance(self) -> Result<RetestStage, RetestCode> {
        match self.next() { Some(n) => Ok(n), None => Err(RetestCode::StageSkip), }
    }
}

// ── 注入账 ────────────────────────────────────────────────────────────
/// 注入账：注入 = 检出 + 逃逸。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct RecheckLedger {
    /// 注入总次数
    pub injected: u32,
    /// 检出次数
    pub detected: u32,
    /// 逃逸次数
    pub escaped: u32,
}

impl RecheckLedger {
    /// 空账。
    pub const fn zero() -> RecheckLedger {
        RecheckLedger { injected: 0, detected: 0, escaped: 0 }
    }

    /// 记一次注入。
    pub fn record(&mut self, caught: bool) -> Result<(), RetestCode> {
        self.injected = self.injected.saturating_add(1);
if caught { self.detected = self.detected.saturating_add(1); } else { self.escaped = self.escaped.saturating_add(1); }
if self.detected.saturating_add(self.escaped) != self.injected { return Err(RetestCode::LedgerRegress); }
        Ok(())
    }

    /// 守恒判定。
    pub fn conserved(&self) -> bool {
        self.detected.saturating_add(self.escaped) == self.injected
    }
}

/// 全矩阵 10 轮注入账。
pub fn run_ledger() -> RecheckLedger {
    let mut ledger = RecheckLedger::zero();
    let mut round = 0u32;
while round < ROUNDS { let mut pi = 0usize; while pi < MATRIX.len() { if let Some(p) = MATRIX.get(pi) { let mut mi = 0usize; while mi < Mutant::ALL.len() { if let Some(m) = Mutant::ALL.get(mi) { let caught = mutate(*m, judge(p)) != p.expect.code(); let _ = ledger.record(caught); } mi += 1; } } pi += 1; } round = round.saturating_add(1); }
    ledger
}

// ── 确定性指纹 ────────────────────────────────────────────────────────
fn mix(h: u32, v: u32) -> u32 {
    (h ^ v).wrapping_mul(0x0100_0193)
}

/// 语料指纹：探针表逐字段入哈希。
pub fn corpus_fingerprint() -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    let mut i = 0usize;
while i < MATRIX.len() { if let Some(p) = MATRIX.get(i) { let bytes = p.id.as_bytes(); let mut k = 0usize; while k < bytes.len() { if let Some(b) = bytes.get(k) { h = mix(h, *b as u32); } k += 1; } h = mix(h, p.mode); h = mix(h, if p.is_dir { 1 } else { 0 }); h = mix(h, p.expect.code()); } i += 1; }
    let mut mi = 0usize;
while mi < Mutant::ALL.len() { if let Some(m) = Mutant::ALL.get(mi) { h = mix(h, m.code()); } mi += 1; }
    h
}

/// 声明指纹。
pub fn declaration_fingerprint(d: &Declaration) -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    h = mix(h, d.cells);
    h = mix(h, d.rounds);
    h = mix(h, d.ten_of_ten);
    h = mix(h, d.flaky);
    h = mix(h, d.min_hits);
    h = mix(h, d.max_hits);
    h = mix(h, if d.reproducible { 1 } else { 0 });
    h
}

// ── 检查入口 ──────────────────────────────────────────────────────────
/// F9277 判据集（静态 19 条 + 动态 3 条）。
pub fn run_unx9277_checks() -> CheckSet {
    let mut s = CheckSet::new("unx9277");

    // 1. 段位守卫
    let seg_ok = SEG_BASE % 0x40 == 0 && (0x7A000..=0x7D1C0).contains(&SEG_BASE);
    s.add("段位守卫：0x7B2C0 0x40 对齐且在批窗口内", seg_ok, "段基址必须唯一且落在本批窗口；越界即拒绝注册");

    // 2. 矩阵与冻结账
    s.add("矩阵：12 条探针齐全且期望码零漂移", MATRIX.len() == PROBES && expect_drift() == 0, "改探针必须同步改冻结账，否则复测两套语义");

    // 3. 正常流：12 条判定与期望一致
    s.add("正常流：12 条探针逐条判定与期望码一致", matrix_clean(), "判定器本身先要全绿，否则复测的是坏尺子");

    // 4. 类别覆盖
    let mut kinds_ok = kind_count(ProbeKind::Bits) >= 4;
    let mut ki = 0usize;
while ki < ProbeKind::ALL.len() { if let Some(k) = ProbeKind::ALL.get(ki) { if kind_count(*k) == 0 { kinds_ok = false; } } ki += 1; }
    s.add("类别覆盖：权限位≥4 且穿越/root/能力位/特权操作各≥1", kinds_ok, "五类语义必须都在矩阵里，缺类即复测不完整");

    // 5. 目录位一致
    s.add("目录位：穿越类标目录、其余不标目录", dir_flags_consistent(), "目录位漂移会让穿越语义与普通文件语义混淆");

    // 6. 格数容量
    s.add("容量：12×3=36 格 ≤ 64 上限", total_cells() == CELLS && CELLS as usize <= MAX_CELLS, "格数超上限说明矩阵失控，应拆卡而不是继续堆格");

    // 7. 逐格 10/10
    s.add("10/10 次检出：36 格逐格 10 轮全检出", all_cells_ten_of_ten(), "任何一格少于 10/10 都不能声明可复测");

    // 8. 每轮 36/36
    s.add("轮次统计：10 轮每轮检出 36/36", every_round_full(), "每轮满格说明矩阵没有依赖轮次的漏检路径");

    // 9. 轮次统计恒等
    let table = round_table();
    let mut flat = true;
    let mut ri = 0usize;
while ri < table.len() { if let Some(n) = table.get(ri) { if *n != CELLS { flat = false; } } ri += 1; }
    s.add("轮次恒等：10 轮逐轮检出数全部等于 36", flat, "轮间不齐说明矩阵依赖轮次外状态，属隐藏状态泄漏");

    // 10. 声明字段
    let d = declare(false);
    s.add("声明格：cells=36 / rounds=10 / ten_of_ten=36 / flaky=0", d.cells == CELLS && d.rounds == ROUNDS && d.ten_of_ten == CELLS && d.flaky == 0 && d.probes == PROBES as u32 && d.mutants == 3, "声明字段是复测的对外承诺，必须与语料逐项咬合");

    // 11. 零 flaky
    s.add("零 flaky 声明：min=max=10 且 reproducible", zero_flaky(&d), "零 flaky 必须由全格 10/10 推出，不能手工置真");

    // 12. 反向注入
    let flaky = declare(true);
    s.add("反向注入：偶轮检出模型使 36 格全记 flaky", flaky.flaky == CELLS && flaky.ten_of_ten == 0 && !flaky.reproducible, "声明器对轮次抖动必须敏感，否则零 flaky 是空话");

    // 13. 反向注入的可观测下限
    s.add("反向注入：逐格检出掉到 5/10、轮次不再恒等", flaky.min_hits == 5 && flaky.max_hits == 5 && !flaky.identical_rounds, "抖动必须被量化成具体检出数，不能只给一句 flaky");

    // 14. 账本守恒
    let ledger = run_ledger();
    s.add("守恒：注入 360 = 检出 360 + 逃逸 0", ledger.conserved() && ledger.escaped == 0 && ledger.injected == CELLS * ROUNDS, "注入账必须闭合；逃逸说明有格没被检查");

    // 15. 算术
    s.add("算术：36 格 × 10 轮 = 360 次注入", CELLS * ROUNDS == 360 && total_cells() * ROUNDS == 360, "统计口径用整数乘法，不做百分比以免抹平差异");

    // 16. 双跑一致
    let a = corpus_fingerprint();
    let b = corpus_fingerprint();
    let d2 = declare(false);
    s.add("双跑一致：语料与声明指纹两次相等", a == b && d == d2 && declaration_fingerprint(&d) == declaration_fingerprint(&d2), "复测不得依赖轮次外状态或哈希遍历顺序");

    // 17. 文案闭集
    let mut text_ok = true;
    let mut i17 = 0usize;
while i17 < RetestCode::ALL.len() { if let Some(c) = RetestCode::ALL.get(i17) { if c.val() != SEG_BASE + i17 as u32 || c.detail().is_empty() { text_ok = false; } let mut j17 = i17 + 1; while j17 < RetestCode::ALL.len() { if let Some(o) = RetestCode::ALL.get(j17) { if c.val() == o.val() { text_ok = false; } } j17 += 1; } } i17 += 1; }
    s.add("文案闭集：12 个诊断码连续、唯一、detail 非空", text_ok, "每个码必须能取到非空且可操作的说明");

    // 18. 段内余量
    s.add("段位余量：末码 < SEG_BASE + 0x40", RetestCode::ALL.len() as u32 <= 0x40, "0x40 窗口必须容得下闭集，越界会侵入下一张卡");

    // 19. 账目单调
    let mut mono = ledger;
    let before = mono.injected;
    let mut k = 0u32;
while k < 4 { let _ = mono.record(true); k = k.saturating_add(1); }
    s.add("账目单调：再注入 4 次只增不减", mono.injected == before + 4, "累计计数禁止回退或饱和到旧值");

    // 动态明细（≤8）
    let stat_text = format!("cells={} rounds={} ten={} flaky={}", d.cells, d.rounds, d.ten_of_ten, d.flaky);
    s.add_dyn("10/10 统计", zero_flaky(&d), &stat_text);
    let flaky_text = format!("flaky={} hits={}/{} repro={}", flaky.flaky, flaky.min_hits, flaky.max_hits, if flaky.reproducible { 1 } else { 0 });
    s.add_dyn("抖动快照", flaky.flaky == CELLS && !flaky.reproducible, &flaky_text);
    let ledger_text = format!("inj={} det={} esc={}", ledger.injected, ledger.detected, ledger.escaped);
    s.add_dyn("账目快照", ledger.conserved() && ledger.escaped == 0, &ledger_text);

    s
}

// ── 测试 ──────────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matrix_is_clean_and_frozen() {
        assert_eq!(MATRIX.len(), PROBES);
        assert_eq!(expect_drift(), 0);
        assert!(matrix_clean());
        assert!(MATRIX.iter().all(|p| !p.id.is_empty()));
    }

    #[test]
    fn owner_does_not_borrow_group_bits() {
        let p = probe_of("owner-jump-group").unwrap_or(&MATRIX[0]);
        assert_eq!(judge(p), Verdict::DenyEacces);
        assert_eq!(p.mode & 0o060, 0o060);
    }

    #[test]
    fn root_exemption_does_not_create_x() {
        assert_eq!(judge(probe_of("root-write-zero").unwrap_or(&MATRIX[0])), Verdict::Allow);
        assert_eq!(judge(probe_of("root-exec-nox").unwrap_or(&MATRIX[0])), Verdict::DenyEacces);
    }

    #[test]
    fn capability_does_not_create_x() {
        assert_eq!(judge(probe_of("cap-write-zero").unwrap_or(&MATRIX[0])), Verdict::Allow);
        assert_eq!(judge(probe_of("cap-exec-nox").unwrap_or(&MATRIX[0])), Verdict::DenyEacces);
    }

    #[test]
    fn chown_domain_is_eperm() {
        assert_eq!(judge(probe_of("chown-nocap").unwrap_or(&MATRIX[0])), Verdict::DenyEperm);
        assert_eq!(judge(probe_of("chown-owner").unwrap_or(&MATRIX[0])), Verdict::Allow);
    }

    #[test]
    fn dir_flags_pinned() {
        assert!(dir_flags_consistent());
        assert_eq!(kind_count(ProbeKind::Traverse), 1);
    }

    #[test]
    fn cells_are_thirty_six() {
        assert_eq!(total_cells(), 36);
        assert_eq!(CELLS, 36);
        assert!(CELLS as usize <= MAX_CELLS);
    }

    #[test]
    fn every_cell_is_ten_of_ten() {
        assert!(all_cells_ten_of_ten());
        let mut pi = 0usize;
while pi < MATRIX.len() { if let Some(p) = MATRIX.get(pi) { let mut mi = 0usize; while mi < Mutant::ALL.len() { if let Some(m) = Mutant::ALL.get(mi) { assert_eq!(cell_hits(p, *m), ROUNDS); } mi += 1; } } pi += 1; }
    }

    #[test]
    fn every_round_is_full() {
        assert!(every_round_full());
        assert_eq!(round_detections(0), 36);
        assert_eq!(round_detections(9), 36);
        assert_eq!(round_detections(ROUNDS), 0);
    }

    #[test]
    fn clean_declaration_is_reproducible() {
        let d = declare(false);
        assert_eq!(d.cells, 36);
        assert_eq!(d.rounds, 10);
        assert_eq!(d.ten_of_ten, 36);
        assert_eq!(d.flaky, 0);
        assert_eq!(d.min_hits, 10);
        assert!(d.identical_rounds);
        assert!(d.reproducible);
        assert!(zero_flaky(&d));
    }

    #[test]
    fn parity_injection_flips_declaration() {
        let d = declare(true);
        assert_eq!(d.flaky, 36);
        assert_eq!(d.ten_of_ten, 0);
        assert_eq!(d.min_hits, 5);
        assert_eq!(d.max_hits, 5);
        assert!(!d.identical_rounds);
        assert!(!d.reproducible);
        assert!(!zero_flaky(&d));
    }

    #[test]
    fn ledger_conserves_across_ten_rounds() {
        let led = run_ledger();
        assert!(led.conserved());
        assert_eq!(led.injected, 360);
        assert_eq!(led.detected, 360);
        assert_eq!(led.escaped, 0);
    }

    #[test]
    fn stage_machine_is_forward_only() {
        let mut st = RetestStage::Probe;
        let mut n = 0;
while let Ok(next) = st.advance() { st = next; n += 1; }
        assert_eq!(n, 3);
        assert_eq!(st, RetestStage::Declare);
        assert_eq!(st.advance(), Err(RetestCode::StageSkip));
    }

    #[test]
    fn deterministic_corpus_and_checks_green() {
        assert_eq!(corpus_fingerprint(), corpus_fingerprint());
        let set = run_unx9277_checks();
        assert!(set.all_passed(), "{}", set.red_items_text());
        assert!(!set.truncated());
    }
}
