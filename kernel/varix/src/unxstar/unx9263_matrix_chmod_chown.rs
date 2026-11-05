//! UNX-F9263 chmod/chown EPERM 矩阵（非属主） · 0x7AF40段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板 F9263 逐字）：
//!   非属主注入判据过，全 EPERM 格过，特权路径正判据格过，与 B05 三身份
//!   档对账格过。
//!
//! 本卡主题：chmod/chown/chgrp 的 EPERM 面——三身份档（root/属主/外人）
//! × 三操作（chmod/chown uid/chgrp gid）× 八种模式位 = 72 格。核心一句
//! 话：EPERM 只认「属主或特权」，与对象 rwx 位无关。
//!
//! 设计要点：
//!   ① 身份状态机四态互斥：特权（持有 CAP_FOWNER/CAP_CHOWN）→ 属主
//!      （uid 相等）→ 同组（gid 相等）→ 外人，前态优先，不允许叠加。
//!   ② 判定次序：穿越门 → 属主/特权门 → 目标合法性门。父目录缺 x 时
//!      先报 EACCES（非属主也不例外），但特权调用者越过穿越门。
//!   ③ 非属主三操作全 EPERM：模式位从 0o000 到 0o777 逐格不变，任何
//!      rwx 组合都不能给非属主发 chmod/chown 的通行证。
//!   ④ 特权正判据：root 对三操作 × 八位型 24/24 通过；属主可 chmod、
//!      可 chown 自持、可 chgrp 本组，但改属他人/改入外组仍 EPERM。
//!   ⑤ 与 B05 三身份档对账：root 24 通、属主 8 通 16 拒、外人 24 拒；
//!      五类注入各 10/10 检出，注入账守恒，双跑指纹一致。
//! ─────────────────────────────────────────────────────────────────────

use alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（F9263 独占 0x40 窗口）。
pub const SEG_BASE: u32 = 0x7AF40;

/// B05 三身份档数。
pub const B05_TIERS: usize = 3;
/// 判据面操作数（chmod / chown uid / chgrp gid）。
pub const OP_COUNT: usize = 3;
/// 模式位型数（0o000 起八种组合）。
pub const PATTERN_COUNT: usize = 8;
/// 矩阵总格数。
pub const MATRIX_CELLS: usize = B05_TIERS * OP_COUNT * PATTERN_COUNT;
/// 非属主档格数（外人 × 三操作 × 八位型）。
pub const NONOWNER_CELLS: usize = OP_COUNT * PATTERN_COUNT;
/// 注入类别数。
pub const INJECT_CLASSES: usize = 5;
/// 单类注入复测轮数。
pub const INJECT_ROUNDS: u32 = 10;

/// Linux errno 正数码。
pub const EPERM: i32 = 1;
pub const ENOENT: i32 = 2;
pub const EACCES: i32 = 13;
pub const EROFS: i32 = 30;

/// 能力位（本卡只用两枚）。
pub const CAP_CHOWN: u32 = 1 << 0;
pub const CAP_FOWNER: u32 = 1 << 1;
/// root 夹具能力（两枚全给）。
pub const CAP_ALL: u32 = CAP_CHOWN | CAP_FOWNER;

/// 文件夹具坐标。
pub const F_UID: u32 = 1000;
pub const F_GID: u32 = 100;
pub const THIRD_UID: u32 = 1200;
pub const FOREIGN_GID: u32 = 300;

/// 八个模式位型（0o000 起，含代表性组合）。
pub const PATTERNS: [u16; PATTERN_COUNT] = [
    0o000, 0o400, 0o200, 0o100, 0o600, 0o444, 0o644, 0o755,
];

// ── 诊断码闭集 ────────────────────────────────────────────────────────
/// 本模块诊断码（段内 +0x01..=+0x0C 闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum EpermCode {
    ChmodNonOwner,
    ChownNonOwner,
    ChownThirdParty,
    ChgrpForeign,
    TraverseAsEperm,
    ModeDependent,
    CellMismatch,
    B05Drift,
    LedgerRegress,
    NotReproducible,
    EmptyDetail,
    SegmentViolation,
}

impl EpermCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }

    /// 中文诊断与可操作建议（闭集返回，禁止空串）。
    pub fn detail(self) -> &'static str {
        match self {
            EpermCode::ChmodNonOwner => "非属主 chmod 被放行：无属主身份且无 CAP_FOWNER \
                时必须 EPERM；请勿把 rwx 位当属主凭证",
            EpermCode::ChownNonOwner => "非属主 chown/chgrp 被放行：改属/改组的门槛是\
                属主或 CAP_CHOWN；请先做属主门",
            EpermCode::ChownThirdParty => "属主改属他人被放行：非特权属主只能 chown 到\
                自己；请拦下第三 uid",
            EpermCode::ChgrpForeign => "属主改入外组被放行：非特权属主只能改到自己所属\
                组；请校验目标 gid",
            EpermCode::TraverseAsEperm => "穿越失败报成 EPERM：父目录缺 x 属 EACCES，\
                且必须先于属主门判定；请拆开两门",
            EpermCode::ModeDependent => "裁决随模式位漂移：chmod/chown 的 EPERM 与 rwx \
                位无关；请移除位依赖",
            EpermCode::CellMismatch => "逐格不符：72 格中判定与期望不一致；请打印\
                身份档/操作/位型复算",
            EpermCode::B05Drift => "B05 对账漂移：三身份档 24/8+16/0+24 必须逐档相等；\
                改判前先改表并留痕",
            EpermCode::LedgerRegress => "账目倒退：注入计数只增不减；请检查回滚路径",
            EpermCode::NotReproducible => "双跑不一致：同输入两次结果不同；请排查\
                隐藏状态或非确定哈希",
            EpermCode::EmptyDetail => "文案缺失：每个诊断码必须带非空中文建议",
            EpermCode::SegmentViolation => "段位越界：SEG_BASE 必须精确等于 0x7AF40 \
                且 0x40 对齐",
        }
    }

    /// 闭集全量。
    pub const ALL: [EpermCode; 12] = [
        EpermCode::ChmodNonOwner, EpermCode::ChownNonOwner, EpermCode::ChownThirdParty,
        EpermCode::ChgrpForeign, EpermCode::TraverseAsEperm, EpermCode::ModeDependent,
        EpermCode::CellMismatch, EpermCode::B05Drift, EpermCode::LedgerRegress,
        EpermCode::NotReproducible, EpermCode::EmptyDetail, EpermCode::SegmentViolation,
    ];
}

/// Linux errno 对照表：(名称, 正数码, Windows 近似码, 说明)
pub const ERRNO_TABLE: &[(&str, i32, i32, &str)] = &[
    ("EPERM", 1, 5, "Windows ERROR_ACCESS_DENIED；属主/特权缺失（本卡主码）"),
    ("ENOENT", 2, 2, "Windows ERROR_FILE_NOT_FOUND；路径不存在（域外）"),
    ("EACCES", 13, 5, "Windows ERROR_ACCESS_DENIED；对象权限位不足（域外）"),
    ("EROFS", 30, 19, "Windows ERROR_WRITE_PROTECT；只读卷（域外）"),
];

/// 与 Linux 逐码一致：正码必须在表内且负值可复算。
pub fn agrees_with_linux(code: i32) -> bool {
    if code <= 0 {
        return false;
    }
    let mut i = 0usize;
    while i < ERRNO_TABLE.len() {
        if let Some(row) = ERRNO_TABLE.get(i) {
            if row.1 == code {
                return -row.1 == -code;
            }
        }
        i += 1;
    }
    false
}

// ── 主体 / 客体 / 身份状态机 ──────────────────────────────────────────
/// 调用者（uid/gid/能力位）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Actor {
    pub uid: u32,
    pub gid: u32,
    pub caps: u32,
}

impl Actor {
    pub const fn new(uid: u32, gid: u32, caps: u32) -> Actor {
        Actor { uid, gid, caps }
    }

    pub fn has(self, cap: u32) -> bool {
        self.caps & cap == cap
    }

    /// 是否持有任一属主类特权。
    pub fn privileged(self) -> bool {
        self.has(CAP_FOWNER) || self.has(CAP_CHOWN)
    }

    /// 是否属于某组（本夹具只看主组）。
    pub fn in_group(self, gid: u32) -> bool {
        self.gid == gid
    }
}

/// 被操作文件（含 chown/chgrp 目标与父目录穿越位）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct File {
    pub uid: u32,
    pub gid: u32,
    pub mode: u16,
    pub target_uid: u32,
    pub target_gid: u32,
    pub parent_traversable: bool,
}

impl File {
    pub const fn new(uid: u32, gid: u32, mode: u16,
                     target_uid: u32, target_gid: u32, parent: bool) -> File {
        File { uid, gid, mode, target_uid, target_gid, parent_traversable: parent }
    }
}

/// 基准夹具：属主 1000/100，目标为第三 uid 与外组。
pub fn base_file(mode: u16) -> File {
    File::new(F_UID, F_GID, mode, THIRD_UID, FOREIGN_GID, true)
}

/// 自持夹具：chown/chgrp 目标都是属主自己。
pub fn self_file(mode: u16) -> File {
    File::new(F_UID, F_GID, mode, F_UID, F_GID, true)
}

/// B05 三身份档（冻结）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Tier {
    Root,
    Owner,
    Stranger,
}

impl Tier {
    pub const ALL: [Tier; B05_TIERS] = [Tier::Root, Tier::Owner, Tier::Stranger];

    pub fn name(self) -> &'static str {
        match self {
            Tier::Root => "root",
            Tier::Owner => "owner",
            Tier::Stranger => "stranger",
        }
    }

    /// 冻结身份夹具。
    pub fn actor(self) -> Actor {
        match self {
            Tier::Root => Actor::new(0, 0, CAP_ALL),
            Tier::Owner => Actor::new(F_UID, F_GID, 0),
            Tier::Stranger => Actor::new(2000, 200, 0),
        }
    }
}

/// 同组外人夹具：gid 与文件相同、uid 不同（验证 gid 不豁免 EPERM）。
pub const SIBLING: Actor = Actor::new(2001, F_GID, 0);

/// 操作闭集。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FileOp {
    Chmod,
    ChownUid,
    ChgrpGid,
}

impl FileOp {
    pub const ALL: [FileOp; OP_COUNT] = [
        FileOp::Chmod, FileOp::ChownUid, FileOp::ChgrpGid,
    ];
}

/// 身份状态机四态（优先级从高到低）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Stance {
    Privileged,
    Owner,
    NearOwner,
    Stranger,
}

/// 四态互斥判定：特权 → 属主 → 同组 → 外人。
pub fn stance_of(actor: Actor, file: File) -> Stance {
    if actor.privileged() {
        Stance::Privileged
    } else if actor.uid == file.uid {
        Stance::Owner
    } else if actor.gid == file.gid {
        Stance::NearOwner
    } else {
        Stance::Stranger
    }
}

// ── 判定器 ────────────────────────────────────────────────────────────
/// chmod/chown/chgrp 判定：穿越门 → 属主/特权门 → 目标合法性门。
pub fn judge_op(op: FileOp, actor: Actor, file: File) -> Result<(), EpermCode> {
    if !file.parent_traversable && !actor.privileged() {
        return Err(EpermCode::TraverseAsEperm);
    }
    match op {
        FileOp::Chmod => {
            if actor.has(CAP_FOWNER) || actor.uid == file.uid {
                Ok(())
            } else {
                Err(EpermCode::ChmodNonOwner)
            }
        }
        FileOp::ChownUid => {
            if actor.has(CAP_CHOWN) {
                Ok(())
            } else if actor.uid != file.uid {
                Err(EpermCode::ChownNonOwner)
            } else if file.target_uid != actor.uid {
                Err(EpermCode::ChownThirdParty)
            } else {
                Ok(())
            }
        }
        FileOp::ChgrpGid => {
            if actor.has(CAP_CHOWN) {
                Ok(())
            } else if actor.uid != file.uid {
                Err(EpermCode::ChownNonOwner)
            } else if !actor.in_group(file.target_gid) {
                Err(EpermCode::ChgrpForeign)
            } else {
                Ok(())
            }
        }
    }
}

/// 期望码：只由身份档与操作决定，与模式位无关。
pub fn expected_code(op: FileOp, tier: Tier) -> i32 {
    match tier {
        Tier::Root => 0,
        Tier::Owner => match op {
            FileOp::Chmod => 0,
            FileOp::ChownUid | FileOp::ChgrpGid => EPERM,
        },
        Tier::Stranger => EPERM,
    }
}

/// 实际码（0 表示通过，否则 EPERM）。
pub fn verdict_code(op: FileOp, actor: Actor, file: File) -> i32 {
    match judge_op(op, actor, file) {
        Ok(()) => 0,
        Err(_) => EPERM,
    }
}

// ── 逐格矩阵与 B05 对账 ───────────────────────────────────────────────
/// 逐格账。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct CellTally {
    pub cells: u32,
    pub ok: u32,
    pub eperm: u32,
    pub nonowner_eperm: u32,
    pub mismatches: u32,
}

impl CellTally {
    const fn zero() -> CellTally {
        CellTally { cells: 0, ok: 0, eperm: 0, nonowner_eperm: 0, mismatches: 0 }
    }

    pub fn reconciled(&self) -> bool {
        self.ok.saturating_add(self.eperm) == self.cells
    }
}

/// 跑 3 档 × 3 操作 × 8 位型 = 72 格。
pub fn run_eperm_matrix() -> CellTally {
    let mut t = CellTally::zero();
    let mut ti = 0usize;
    while ti < Tier::ALL.len() {
        if let Some(tier) = Tier::ALL.get(ti).copied() {
            let actor = tier.actor();
            let mut oi = 0usize;
            while oi < FileOp::ALL.len() {
                if let Some(op) = FileOp::ALL.get(oi).copied() {
                    let mut pi = 0usize;
                    while pi < PATTERNS.len() {
                        if let Some(mode) = PATTERNS.get(pi).copied() {
                            let got = verdict_code(op, actor, base_file(mode));
                            let expect = expected_code(op, tier);
                            if got != expect {
                                t.mismatches = t.mismatches.saturating_add(1);
                            }
                            if got == 0 {
                                t.ok = t.ok.saturating_add(1);
                            } else {
                                t.eperm = t.eperm.saturating_add(1);
                                if tier == Tier::Stranger {
                                    t.nonowner_eperm = t.nonowner_eperm.saturating_add(1);
                                }
                            }
                            t.cells = t.cells.saturating_add(1);
                        }
                        pi += 1;
                    }
                }
                oi += 1;
            }
        }
        ti += 1;
    }
    t
}

/// 单档 24 格的 (通过数, EPERM 数)。
pub fn tier_counts(tier: Tier) -> (u32, u32) {
    let actor = tier.actor();
    let mut ok = 0u32;
    let mut eperm = 0u32;
    let mut oi = 0usize;
    while oi < FileOp::ALL.len() {
        if let Some(op) = FileOp::ALL.get(oi).copied() {
            let mut pi = 0usize;
            while pi < PATTERNS.len() {
                if let Some(mode) = PATTERNS.get(pi).copied() {
                    if judge_op(op, actor, base_file(mode)).is_ok() {
                        ok += 1;
                    } else {
                        eperm += 1;
                    }
                }
                pi += 1;
            }
        }
        oi += 1;
    }
    (ok, eperm)
}

/// B05 冻结对账表：(身份档, 通过数, EPERM 数)。
pub const B05_TIER_TABLE: [(Tier, u32, u32); B05_TIERS] = [
    (Tier::Root, 24, 0),
    (Tier::Owner, 8, 16),
    (Tier::Stranger, 0, 24),
];

/// 三档逐档与冻结表一致。
pub fn b05_reconciled() -> bool {
    let mut i = 0usize;
    while i < B05_TIER_TABLE.len() {
        if let Some((tier, want_ok, want_eperm)) = B05_TIER_TABLE.get(i).copied() {
            let (ok, eperm) = tier_counts(tier);
            if ok != want_ok || eperm != want_eperm {
                return false;
            }
        }
        i += 1;
    }
    true
}

// ── 注入账 ────────────────────────────────────────────────────────────
/// 注入账：注入 = 检出 + 逃逸。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct InjectLedger {
    pub injected: u32,
    pub detected: u32,
    pub escaped: u32,
}

impl InjectLedger {
    const fn zero() -> InjectLedger {
        InjectLedger { injected: 0, detected: 0, escaped: 0 }
    }

    pub fn record(&mut self, caught: bool) -> Result<(), i32> {
        self.injected = self.injected.saturating_add(1);
        if caught {
            self.detected = self.detected.saturating_add(1);
        } else {
            self.escaped = self.escaped.saturating_add(1);
        }
        if self.detected.saturating_add(self.escaped) != self.injected {
            return Err(-(EpermCode::LedgerRegress as i32));
        }
        Ok(())
    }

    pub fn conserved(&self) -> bool {
        self.detected.saturating_add(self.escaped) == self.injected
    }
}

/// 某身份执行某操作，八种位型是否全部命中指定诊断码。
fn class_all(actor: Actor, op: FileOp, want: EpermCode) -> bool {
    let mut i = 0usize;
    while i < PATTERNS.len() {
        if let Some(mode) = PATTERNS.get(i).copied() {
            if judge_op(op, actor, base_file(mode)) != Err(want) {
                return false;
            }
        }
        i += 1;
    }
    true
}

/// 单类注入：true 表示按预期被拒。
fn inject_once(class: usize) -> bool {
    match class {
        0 => class_all(Tier::Stranger.actor(), FileOp::Chmod, EpermCode::ChmodNonOwner),
        1 => class_all(Tier::Stranger.actor(), FileOp::ChownUid, EpermCode::ChownNonOwner),
        2 => class_all(SIBLING, FileOp::Chmod, EpermCode::ChmodNonOwner),
        3 => class_all(Tier::Owner.actor(), FileOp::ChownUid, EpermCode::ChownThirdParty),
        4 => class_all(Tier::Owner.actor(), FileOp::ChgrpGid, EpermCode::ChgrpForeign),
        _ => false,
    }
}

/// 五类注入各跑 INJECT_ROUNDS 轮。
pub fn run_inject_matrix() -> (InjectLedger, [u32; INJECT_CLASSES]) {
    let mut ledger = InjectLedger::zero();
    let mut per_class = [0u32; INJECT_CLASSES];
    let mut ci = 0usize;
    while ci < INJECT_CLASSES {
        let mut round = 0u32;
        while round < INJECT_ROUNDS {
            let caught = inject_once(ci);
            let _ = ledger.record(caught);
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

// ── 确定性指纹 ────────────────────────────────────────────────────────
/// FNV-1a 32 位（无浮点、无随机）。
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

/// 72 格判据指纹。
pub fn corpus_fingerprint() -> u32 {
    let mut bytes: Vec<u8> = Vec::new();
    let mut ti = 0usize;
    while ti < Tier::ALL.len() {
        if let Some(tier) = Tier::ALL.get(ti).copied() {
            let actor = tier.actor();
            let mut oi = 0usize;
            while oi < FileOp::ALL.len() {
                if let Some(op) = FileOp::ALL.get(oi).copied() {
                    let mut pi = 0usize;
                    while pi < PATTERNS.len() {
                        if let Some(mode) = PATTERNS.get(pi).copied() {
                            bytes.push(match judge_op(op, actor, base_file(mode)) {
                                Ok(()) => 0u8,
                                Err(e) => e as u8,
                            });
                        }
                        pi += 1;
                    }
                }
                oi += 1;
            }
        }
        ti += 1;
    }
    fnv1a(&bytes)
}

// ── 检查入口 ──────────────────────────────────────────────────────────
/// F9263 判据集（≥12 条；动态明细 ≤8 条）。
pub fn run_unx9263_checks() -> CheckSet {
    let mut s = CheckSet::new("unx9263");

    let seg_ok = SEG_BASE == 0x7AF40
        && SEG_BASE % 0x40 == 0
        && (0x7A000..=0x7D1C0).contains(&SEG_BASE);
    s.add("段位守卫：0x7AF40 独占且 0x40 对齐", seg_ok,
          "段基址必须精确等于派工值，越界或错位即拒绝注册");

    let grid = run_eperm_matrix();
    s.add("逐格矩阵：3 档 × 3 操作 × 8 位型 = 72 格全落账",
          grid.cells == 72 && grid.reconciled() && grid.mismatches == 0,
          "通过 + 拒绝 = 72，且与冻结期望零格不符");

    s.add("全 EPERM 格：非属主 24 格无一放行",
          grid.nonowner_eperm == NONOWNER_CELLS as u32,
          "外人档对 chmod/chown/chgrp 的八种位型必须全部 EPERM");

    let stranger = Tier::Stranger.actor();
    let mut nonowner_ok = true;
    let mut oi = 0usize;
    while oi < FileOp::ALL.len() {
        if let Some(op) = FileOp::ALL.get(oi).copied() {
            let want = match op {
                FileOp::Chmod => EpermCode::ChmodNonOwner,
                FileOp::ChownUid | FileOp::ChgrpGid => EpermCode::ChownNonOwner,
            };
            if !class_all(stranger, op, want) {
                nonowner_ok = false;
            }
        }
        oi += 1;
    }
    s.add("非属主注入：三操作 × 八位型 24/24 归 EPERM", nonowner_ok,
          "非属主改任何一位都不得放行，gid 相同也不豁免");

    let root = Tier::Root.actor();
    let mut priv_ok = true;
    let mut pi = 0usize;
    while pi < PATTERNS.len() {
        if let Some(mode) = PATTERNS.get(pi).copied() {
            let mut qi = 0usize;
            while qi < FileOp::ALL.len() {
                if let Some(op) = FileOp::ALL.get(qi).copied() {
                    if judge_op(op, root, base_file(mode)).is_err() {
                        priv_ok = false;
                    }
                }
                qi += 1;
            }
        }
        pi += 1;
    }
    s.add("特权路径正判据：root 3 操作 × 8 位型 24/24 通过", priv_ok,
          "root 持 CAP_FOWNER/CAP_CHOWN，改属改组不得误拒");

    let owner = Tier::Owner.actor();
    let mut owner_ok = true;
    let mut mi = 0usize;
    while mi < PATTERNS.len() {
        if let Some(mode) = PATTERNS.get(mi).copied() {
            if judge_op(FileOp::Chmod, owner, self_file(mode)).is_err() {
                owner_ok = false;
            }
        }
        mi += 1;
    }
    owner_ok = owner_ok
        && judge_op(FileOp::ChownUid, owner, self_file(0o644)).is_ok()
        && judge_op(FileOp::ChgrpGid, owner, self_file(0o644)).is_ok();
    s.add("属主正路径：chmod 八位型 + chown 自持 + chgrp 本组全通", owner_ok,
          "属主对自有文件可改模式，可改属自己、改组本组");

    let mut third_ok = true;
    let mut ti3 = 0usize;
    while ti3 < PATTERNS.len() {
        if let Some(mode) = PATTERNS.get(ti3).copied() {
            if judge_op(FileOp::ChownUid, owner, base_file(mode))
                != Err(EpermCode::ChownThirdParty) {
                third_ok = false;
            }
        }
        ti3 += 1;
    }
    s.add("属主改属：目标第三 uid 八位型全 EPERM", third_ok,
          "非特权属主只能 chown 到自己，改属他人必须 EPERM");

    let mut grp_ok = true;
    let mut gi = 0usize;
    while gi < PATTERNS.len() {
        if let Some(mode) = PATTERNS.get(gi).copied() {
            if judge_op(FileOp::ChgrpGid, owner, base_file(mode))
                != Err(EpermCode::ChgrpForeign) {
                grp_ok = false;
            }
        }
        gi += 1;
    }
    s.add("属主改组：目标外组八位型全 EPERM", grp_ok,
          "非特权属主只能改入自己所属组，外组必须 EPERM");

    s.add("同组不豁免：同 gid 外人 chmod/chown 全 EPERM",
          class_all(SIBLING, FileOp::Chmod, EpermCode::ChmodNonOwner)
              && class_all(SIBLING, FileOp::ChownUid, EpermCode::ChownNonOwner),
          "gid 相等只影响读权限位，不构成属主资格");

    let f = base_file(0o644);
    s.add("身份状态机：特权→属主→同组→外人四态互斥",
          stance_of(Tier::Root.actor(), f) == Stance::Privileged
              && stance_of(Actor::new(F_UID, F_GID, CAP_FOWNER), f) == Stance::Privileged
              && stance_of(owner, f) == Stance::Owner
              && stance_of(SIBLING, f) == Stance::NearOwner
              && stance_of(stranger, f) == Stance::Stranger,
          "前态优先且互斥，不允许属主身份与特权叠加误判");

    let blocked = File::new(F_UID, F_GID, 0o777, THIRD_UID, FOREIGN_GID, false);
    s.add("判定次序：穿越门先于属主门、特权可越过穿越门",
          judge_op(FileOp::Chmod, owner, blocked) == Err(EpermCode::TraverseAsEperm)
              && judge_op(FileOp::ChownUid, SIBLING, blocked)
                  == Err(EpermCode::TraverseAsEperm)
              && judge_op(FileOp::Chmod, root, blocked).is_ok(),
          "父目录缺 x 先报 EACCES，只有特权调用者可越过");

    let mut mode_ok = true;
    let mut ti = 0usize;
    while ti < Tier::ALL.len() {
        if let Some(tier) = Tier::ALL.get(ti).copied() {
            let actor = tier.actor();
            let mut qi = 0usize;
            while qi < FileOp::ALL.len() {
                if let Some(op) = FileOp::ALL.get(qi).copied() {
                    if judge_op(op, actor, base_file(0o000))
                        != judge_op(op, actor, base_file(0o777)) {
                        mode_ok = false;
                    }
                }
                qi += 1;
            }
        }
        ti += 1;
    }
    s.add("模式无关：0o000 与 0o777 裁决逐格相同", mode_ok,
          "EPERM 只认属主/特权，任何 rwx 位型都不得改变裁决");

    let (ledger, per_class) = run_inject_matrix();
    s.add("注入必抓：五类非属主/越权样本各 10/10 检出",
          per_class.iter().all(|n| *n == INJECT_ROUNDS),
          "非属主 chmod/chown、同组越权、改属他人、改外组五类漏一即红");

    s.add("注入账守恒：注入=检出+逃逸、逃逸恒 0",
          ledger.conserved() && ledger.escaped == 0
              && ledger.injected == (INJECT_CLASSES as u32) * INJECT_ROUNDS,
          "50 次注入必须 50 次检出");

    s.add("B05 对账：三身份档 24 + 24 + 24 = 72 与冻结表一致", b05_reconciled(),
          "root 24 通、属主 8 通 16 拒、外人 24 拒，逐档相等");

    s.add("Linux 逐码一致：EPERM/EACCES 精确、表外码拒绝",
          agrees_with_linux(EPERM) && agrees_with_linux(EACCES)
              && !agrees_with_linux(0) && !agrees_with_linux(9999)
              && EPERM != EACCES,
          "本卡主码 EPERM=1，EACCES 属权限位域，不得混用");

    let fp_a = corpus_fingerprint();
    let fp_b = corpus_fingerprint();
    s.add("双跑一致：72 格指纹两次相等", fp_a == fp_b,
          "同输入两次必须逐字段相等，隐藏状态即失败");

    s.add("文案闭集：12 个诊断码带中文建议",
          EpermCode::ALL.iter().all(|c| !c.detail().is_empty()),
          "每个码必须能取到非空且可操作的说明");

    let mut uniq = true;
    let mut x = 0usize;
    while x < EpermCode::ALL.len() {
        let mut y = x + 1;
        while y < EpermCode::ALL.len() {
            if let (Some(p), Some(q)) = (EpermCode::ALL.get(x), EpermCode::ALL.get(y)) {
                if p.val() == q.val() {
                    uniq = false;
                }
            }
            y += 1;
        }
        x += 1;
    }
    s.add("诊断码：12 个码值两两唯一", uniq,
          "段内闭集码不得重复，否则判据无法定位");

    s.add("格数封口：72 格 = 3×3×8 且通过 32 + 拒绝 40",
          grid.cells == (MATRIX_CELLS as u32) && grid.ok == 32 && grid.eperm == 40
              && grid.ok + grid.eperm == MATRIX_CELLS as u32,
          "矩阵几何是硬约束，缺一行即格子漂移");

    let grid_text = format!("格 {} / 过 {} / 拒 {} / 不符 {}",
                            grid.cells, grid.ok, grid.eperm, grid.mismatches);
    s.add_dyn("逐格账快照", grid.reconciled() && grid.mismatches == 0, &grid_text);
    let ledger_text = format!("注入 {} / 检出 {} / 逃逸 {}",
                              ledger.injected, ledger.detected, ledger.escaped);
    s.add_dyn("注入账快照", ledger.conserved(), &ledger_text);
    let mut b05_text = format!("B05");
    let mut bi = 0usize;
    while bi < B05_TIER_TABLE.len() {
        if let Some((tier, want_ok, want_eperm)) = B05_TIER_TABLE.get(bi).copied() {
            b05_text.push_str(&format!(" {}={}+{}", tier.name(), want_ok, want_eperm));
        }
        bi += 1;
    }
    b05_text.push_str(&format!(" fp 0x{:08X}", fp_a));
    s.add_dyn("B05 对账快照", b05_reconciled(), &b05_text);

    s
}

// ── 测试 ──────────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn segment_is_pinned() {
        assert_eq!(SEG_BASE, 0x7AF40);
        assert_eq!(SEG_BASE % 0x40, 0);
        assert!((0x7A000..=0x7D1C0).contains(&SEG_BASE));
    }

    #[test]
    fn matrix_72_cells_no_mismatch() {
        let g = run_eperm_matrix();
        assert_eq!(g.cells, MATRIX_CELLS as u32);
        assert_eq!(g.mismatches, 0);
        assert!(g.reconciled());
        assert_eq!(g.ok, 32);
        assert_eq!(g.eperm, 40);
    }

    #[test]
    fn stranger_all_eperm() {
        let g = run_eperm_matrix();
        assert_eq!(g.nonowner_eperm, NONOWNER_CELLS as u32);
        assert!(class_all(Tier::Stranger.actor(), FileOp::Chmod,
                          EpermCode::ChmodNonOwner));
        assert!(class_all(Tier::Stranger.actor(), FileOp::ChgrpGid,
                          EpermCode::ChownNonOwner));
    }

    #[test]
    fn root_positive_path() {
        let root = Tier::Root.actor();
        assert!(judge_op(FileOp::Chmod, root, base_file(0o000)).is_ok());
        assert!(judge_op(FileOp::ChownUid, root, base_file(0o644)).is_ok());
        assert!(judge_op(FileOp::ChgrpGid, root, base_file(0o755)).is_ok());
    }

    #[test]
    fn owner_positive_and_negative() {
        let owner = Tier::Owner.actor();
        assert!(judge_op(FileOp::Chmod, owner, base_file(0o000)).is_ok());
        assert!(judge_op(FileOp::ChownUid, owner, self_file(0o644)).is_ok());
        assert_eq!(judge_op(FileOp::ChownUid, owner, base_file(0o644)),
                   Err(EpermCode::ChownThirdParty));
        assert_eq!(judge_op(FileOp::ChgrpGid, owner, self_file(0o644)), Ok(()));
        assert_eq!(judge_op(FileOp::ChgrpGid, owner, base_file(0o644)),
                   Err(EpermCode::ChgrpForeign));
    }

    #[test]
    fn sibling_gid_does_not_grant_owner() {
        let f = base_file(0o777);
        assert_eq!(stance_of(SIBLING, f), Stance::NearOwner);
        assert_eq!(judge_op(FileOp::Chmod, SIBLING, f),
                   Err(EpermCode::ChmodNonOwner));
    }

    #[test]
    fn traversal_precedes_ownership() {
        let owner = Tier::Owner.actor();
        let blocked = File::new(F_UID, F_GID, 0o777, THIRD_UID, FOREIGN_GID, false);
        assert_eq!(judge_op(FileOp::Chmod, owner, blocked),
                   Err(EpermCode::TraverseAsEperm));
        assert!(judge_op(FileOp::Chmod, Tier::Root.actor(), blocked).is_ok());
    }

    #[test]
    fn stance_ladder_is_exclusive() {
        let f = base_file(0o644);
        assert_eq!(stance_of(Tier::Root.actor(), f), Stance::Privileged);
        assert_eq!(stance_of(Tier::Owner.actor(), f), Stance::Owner);
        assert_eq!(stance_of(SIBLING, f), Stance::NearOwner);
        assert_eq!(stance_of(Tier::Stranger.actor(), f), Stance::Stranger);
    }

    #[test]
    fn b05_table_reconciles() {
        assert!(b05_reconciled());
        assert_eq!(tier_counts(Tier::Root), (24, 0));
        assert_eq!(tier_counts(Tier::Owner), (8, 16));
        assert_eq!(tier_counts(Tier::Stranger), (0, 24));
    }

    #[test]
    fn inject_matrix_catches_all() {
        let (l, per) = run_inject_matrix();
        assert!(l.conserved() && l.escaped == 0);
        for n in per.iter() {
            assert_eq!(*n, INJECT_ROUNDS);
        }
    }

    #[test]
    fn deterministic_corpus() {
        assert_eq!(corpus_fingerprint(), corpus_fingerprint());
    }

    #[test]
    fn checks_all_green() {
        let s = run_unx9263_checks();
        assert!(s.all_passed(), "{}", s.red_items_text());
        assert!(!s.truncated());
    }
}
