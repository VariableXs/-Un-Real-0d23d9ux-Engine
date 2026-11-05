//! UNX-F9271 身份翻转 EPERM 矩阵（联 B20 对账） · 0x7B140段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板 F9271 逐字）：
//!   ① 纯功能代码 ≥320 行；② 判据原文：UNX-F9271-J1 非特权提限注入判据过，
//!   全 EPERM 逐格检出，与 B20 身份矩阵零重复分层格过；③ 完成时 API/软件写回：
//!   状态=已完成、结果摘要非空。
//!
//! 设计要点：
//!   ① 「身份翻转」不是赋值，是一次带门的迁移：先值域门（uid 越界 EINVAL），
//!      再权限门（无权 EPERM），最后池门（目标是否 ∈ {ruid,euid,suid}）。越界
//!      uid 即使无特权也报 EINVAL——错误码必须指向真正的失败原因，不能因为
//!      「反正要拒」就先报 EPERM。
//!   ② 特权由 euid==0 或 CAP_SETUID 推导，不缓存；且 euid 由 0 变非 0 时按
//!      Linux SECBIT_KEEP_CAPS 未置位的口径清空能力位，于是「先降权再提权」
//!      必然 EPERM，不存在残留特权窗口。
//!   ③ setfsuid 是唯一「不报 EPERM」的翻转：无权时静默丢弃并返回 0。把它混进
//!      EPERM 格统计会把正常调用判成逃逸，矩阵里单列一格钉死该语义。
//!   ④ cap_setgid 与 CAP_SETUID 是两个独立位：持有 CAP_SETUID 的身份能翻 euid，
//!      但 setgroups 仍 EPERM。按「一个 cap 管全族」建模会放过越权。
//!   ⑤ 与 B20 对账按「键分层」而非「值相等」：每行以 (身份档, 操作, 入参) 为键，
//!      B20 冻结行与本模块矩阵行键集交集必须为空——同一语义被两张表同时覆盖
//!      就是重复分层，重复即报红。三身份守恒：翻转后每个 id 要么保持旧值，要么
//!      取自入参；注入账 6 格 × 10 轮 = 60 次，逃逸恒 0。
//! ─────────────────────────────────────────────────────────────────────
use alloc::format;

use crate::checks::CheckSet;

/// 段基址：F9271 独占 0x40 窗口。
pub const SEG_BASE: u32 = 0x7B140;
pub const INJECT_ROUNDS: u32 = 10; // 单格复测轮数
pub const MATRIX_CELLS: usize = 18; // 三身份档 × 六操作
pub const B20_ROWS: usize = 5; // B20 冻结对账行
pub const EPERM_CELLS: usize = 6; // 期望 EPERM 的格数

pub const EPERM: i32 = 1; // 权限不足
pub const EINVAL: i32 = 22; // uid 越界
pub const UID_KEEP: u32 = u32::MAX; // -1 哨兵：保持不变
pub const UID_MAX: u32 = u32::MAX - 1; // 合法 uid 上界
pub const UID_ROOT: u32 = 0;
pub const UID_USER: u32 = 1000;
pub const UID_OTHER: u32 = 1001;

/// B20 侧账本标识（联对账锚点）与本模块分层名。
pub const B20_BOOK: &str = "UNX-C2-B20-identity";
pub const LAYER_NAME: &str = "F9271-cap-gate";

// ── 诊断码闭集 ────────────────────────────────────────────────────────
/// 本模块诊断码（段内 +0x00..+0x0B 闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FlipCode {
    EscalateDenied,   // 非特权提限被拒（EPERM）
    ValueGateLate,    // 值域门晚于权限门
    CapLeak,          // cap 位串用（setgroups 越权）
    FsUidSpurious,    // setfsuid 被误报 EPERM
    FsUidLost,        // setfsuid 有权却未生效
    IdInvented,       // 三身份守恒被破坏
    B20Conflict,      // 与 B20 行重复分层
    B20Drift,         // B20 行判定与冻结期望不符
    LayerGap,         // 分层覆盖缺口
    StageRegress,     // 状态机回退
    LedgerLeak,       // 注入账漏检
    SegmentViolation, // 段位越界
}

impl FlipCode {
    /// 码值（段基址 + 变体序号）。
    pub fn val(self) -> u32 {
        SEG_BASE + match self {
            FlipCode::EscalateDenied => 0, FlipCode::ValueGateLate => 1, FlipCode::CapLeak => 2,
            FlipCode::FsUidSpurious => 3, FlipCode::FsUidLost => 4, FlipCode::IdInvented => 5,
            FlipCode::B20Conflict => 6, FlipCode::B20Drift => 7, FlipCode::LayerGap => 8,
            FlipCode::StageRegress => 9, FlipCode::LedgerLeak => 10, FlipCode::SegmentViolation => 11,
        }
    }
    /// 中文诊断与可操作建议（闭集返回，禁止空串）。
    pub fn detail(self) -> &'static str {
        match self {
            FlipCode::EscalateDenied => "提限被拒（EPERM）：非特权身份向池外 uid 翻转；请改用池内 uid 或以 CAP_SETUID 授权后再试",
            FlipCode::ValueGateLate => "值域门次序错：uid 越界必须先报 EINVAL 再谈权限；先报 EPERM 会掩盖越界参数",
            FlipCode::CapLeak => "能力位串用：CAP_SETUID 不等于 CAP_SETGID；setgroups 必须单独持位，否则越权",
            FlipCode::FsUidSpurious => "setfsuid 误报 EPERM：该调用无权时静默丢弃并回 0，任何 EPERM 都是语义错误",
            FlipCode::FsUidLost => "setfsuid 未生效：有权或被允许的翻转必须真的写入 fsuid，否则文件访问身份错位",
            FlipCode::IdInvented => "三身份守恒被破坏：ruid/euid/suid 只能保持旧值或取入参，凭空生成即为缺陷",
            FlipCode::B20Conflict => "与 B20 身份矩阵重复：同一 (档,操作,入参) 键只能由一张表覆盖，重复即分层失效",
            FlipCode::B20Drift => "B20 行判定漂移：冻结期望与判定器不一致，先用 B20 侧权威值校正再改判定器",
            FlipCode::LayerGap => "分层缺口：UidPool 与 CapGate 两层都必须有行，缺一层即对账不完整",
            FlipCode::StageRegress => "状态机非法推进：Flip 只许 Idle→Inspect→Decide→Apply→Commit 单向",
            FlipCode::LedgerLeak => "注入漏检：注入数必须等于检出数，逃逸恒为零",
            FlipCode::SegmentViolation => "段位越界：本模块段基址必须落在 0x7A000..=0x7D1C0 且 0x40 对齐",
        }
    }
    /// 闭集全量（顺序即码序）。
    pub const ALL: [FlipCode; 12] = [
        FlipCode::EscalateDenied, FlipCode::ValueGateLate, FlipCode::CapLeak, FlipCode::FsUidSpurious,
        FlipCode::FsUidLost, FlipCode::IdInvented, FlipCode::B20Conflict, FlipCode::B20Drift,
        FlipCode::LayerGap, FlipCode::StageRegress, FlipCode::LedgerLeak, FlipCode::SegmentViolation,
    ];
}

// ── 身份档与身份账 ────────────────────────────────────────────────────
/// 三身份档：root / 普通用户 / 带文件能力的普通用户。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Profile {
    Root,
    User,
    CapHolder,
}

impl Profile {
    pub fn index(self) -> u8 {
        match self { Profile::Root => 0, Profile::User => 1, Profile::CapHolder => 2 }
    }
    pub fn name(self) -> &'static str {
        match self { Profile::Root => "root", Profile::User => "user", Profile::CapHolder => "cap-holder" }
    }
    /// 初始身份账。
    pub fn initial(self) -> Cred {
        match self {
            Profile::Root => Cred::new(UID_ROOT, UID_ROOT, UID_ROOT, UID_ROOT, true, true),
            Profile::User => Cred::new(UID_USER, UID_USER, UID_USER, UID_USER, false, false),
            Profile::CapHolder => Cred::new(UID_USER, UID_USER, UID_USER, UID_USER, true, false),
        }
    }
    pub const ALL: [Profile; 3] = [Profile::Root, Profile::User, Profile::CapHolder];
}

/// 身份账：ruid/euid/suid 三身份 + fsuid + 两个能力位。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Cred {
    pub ruid: u32,
    pub euid: u32,
    pub suid: u32,
    pub fsuid: u32,
    pub cap_setuid: bool,
    pub cap_setgid: bool,
}

impl Cred {
    pub const fn new(r: u32, e: u32, s: u32, f: u32, cu: bool, cg: bool) -> Cred {
        Cred { ruid: r, euid: e, suid: s, fsuid: f, cap_setuid: cu, cap_setgid: cg }
    }
    /// 特权推导（不缓存：每次现算）。
    pub fn privileged(&self) -> bool {
        self.euid == 0 || self.cap_setuid
    }
    pub fn triple(&self) -> (u32, u32, u32) {
        (self.ruid, self.euid, self.suid)
    }
    /// euid 由 0 变非 0 且未置 KEEP_CAPS → 清空能力位。
    fn drop_caps_after(&mut self, old_euid: u32) {
        if old_euid == 0 && self.euid != 0 {
            self.cap_setuid = false;
            self.cap_setgid = false;
        }
    }
    /// setuid(x)：特权全设；非特权仅当 x ∈ {ruid,suid} 改 euid；否则 EPERM。
    pub fn op_setuid(&mut self, x: u32) -> i32 {
        if x > UID_MAX { return EINVAL; }
        let old = self.euid;
        if self.privileged() {
            self.ruid = x; self.euid = x; self.suid = x;
            self.drop_caps_after(old);
            return 0;
        }
        if x == self.ruid || x == self.suid {
            self.euid = x;
            self.drop_caps_after(old);
            0
        } else { EPERM }
    }
    /// seteuid(x)：只动有效身份，不碰真实/保存身份。
    pub fn op_seteuid(&mut self, x: u32) -> i32 {
        if x > UID_MAX { return EINVAL; }
        if self.privileged() || x == self.ruid || x == self.suid {
            let old = self.euid;
            self.euid = x;
            self.drop_caps_after(old);
            0
        } else { EPERM }
    }
    /// setreuid(r,e)：先整体校验后落值，任一方向不合法则身份完全不变（原子）。
    pub fn op_setreuid(&mut self, r: u32, e: u32) -> i32 {
        if r != UID_KEEP && r > UID_MAX { return EINVAL; }
        if e != UID_KEEP && e > UID_MAX { return EINVAL; }
        let prived = self.privileged();
        if r != UID_KEEP && !prived && r != self.euid && r != self.ruid { return EPERM; }
        if e != UID_KEEP && !prived && e != self.ruid && e != self.suid { return EPERM; }
        if r != UID_KEEP { self.ruid = r; }
        if e != UID_KEEP {
            let old = self.euid;
            self.euid = e;
            self.suid = old; // euid 变化时旧值进保存身份
            self.drop_caps_after(old);
        }
        0
    }
    /// setresuid(r,e,s)：池门逐向校验，任一方向越池即整体拒绝。
    pub fn op_setresuid(&mut self, r: u32, e: u32, s: u32) -> i32 {
        let want = [r, e, s];
        let mut i = 0usize;
        while i < want.len() {
            if let Some(v) = want.get(i) {
                if *v != UID_KEEP && *v > UID_MAX { return EINVAL; }
            }
            i += 1;
        }
        let pool = [self.ruid, self.euid, self.suid];
        let prived = self.privileged();
        let mut j = 0usize;
        while j < want.len() {
            if let Some(v) = want.get(j) {
                if *v != UID_KEEP && !prived && !pool.contains(v) { return EPERM; }
            }
            j += 1;
        }
        if r != UID_KEEP { self.ruid = r; }
        if e != UID_KEEP {
            let old = self.euid;
            self.euid = e;
            self.drop_caps_after(old);
        }
        if s != UID_KEEP { self.suid = s; }
        0
    }
    /// setfsuid(x)：唯一不报 EPERM 的翻转；无权时静默丢弃仍回 0。
    pub fn op_setfsuid(&mut self, x: u32) -> i32 {
        if x > UID_MAX { return EINVAL; }
        let allowed = self.privileged() || x == self.ruid || x == self.euid
            || x == self.suid || x == self.fsuid;
        if allowed { self.fsuid = x; }
        0
    }
    /// setgroups：只认 CAP_SETGID，CAP_SETUID 不算数。
    pub fn op_setgroups(&mut self, _x: u32) -> i32 {
        if self.cap_setgid { 0 } else { EPERM }
    }
}

// ── 操作闭集 ──────────────────────────────────────────────────────────
/// 六操作闭集：四路 uid 翻转 + fsuid + 组身份。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FlipOp {
    SetUid,
    SetEuid,
    SetReUid,
    SetResUid,
    SetFsUid,
    SetGroups,
}

impl FlipOp {
    pub fn index(self) -> u8 {
        match self {
            FlipOp::SetUid => 0, FlipOp::SetEuid => 1, FlipOp::SetReUid => 2,
            FlipOp::SetResUid => 3, FlipOp::SetFsUid => 4, FlipOp::SetGroups => 5,
        }
    }
    pub fn name(self) -> &'static str {
        match self {
            FlipOp::SetUid => "setuid", FlipOp::SetEuid => "seteuid", FlipOp::SetReUid => "setreuid",
            FlipOp::SetResUid => "setresuid", FlipOp::SetFsUid => "setfsuid", FlipOp::SetGroups => "setgroups",
        }
    }
    pub const ALL: [FlipOp; 6] = [
        FlipOp::SetUid, FlipOp::SetEuid, FlipOp::SetReUid,
        FlipOp::SetResUid, FlipOp::SetFsUid, FlipOp::SetGroups,
    ];
}
/// 统一分派：入参收敛到一个 x（setreuid/setresuid 用同值展开）。
pub fn apply(cred: &mut Cred, op: FlipOp, x: u32) -> i32 {
    match op {
        FlipOp::SetUid => cred.op_setuid(x),
        FlipOp::SetEuid => cred.op_seteuid(x),
        FlipOp::SetReUid => cred.op_setreuid(x, x),
        FlipOp::SetResUid => cred.op_setresuid(x, x, x),
        FlipOp::SetFsUid => cred.op_setfsuid(x),
        FlipOp::SetGroups => cred.op_setgroups(x),
    }
}

// ── 矩阵格 ────────────────────────────────────────────────────────────
/// 一格：身份档 + 操作 + 入参 + 期望码 + 期望三身份 + 期望 fsuid。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct FlipCell {
    pub profile: Profile,
    pub op: FlipOp,
    pub x: u32,
    pub expect: i32,
    pub after: (u32, u32, u32),
    pub after_fsuid: u32,
}

impl FlipCell {
    /// 分层键：身份档 × 操作 × 入参。
    pub fn key(self) -> (u8, u8, u32) {
        (self.profile.index(), self.op.index(), self.x)
    }
}

/// 十八格矩阵：三档 × 六操作（提限格 x=0、池内格 x=1000、跨池格 x=1001）。
pub const MATRIX: [FlipCell; MATRIX_CELLS] = [
    // root：特权全通（x=1001 全部落地）
    FlipCell { profile: Profile::Root, op: FlipOp::SetUid, x: UID_OTHER, expect: 0, after: (1001, 1001, 1001), after_fsuid: 0 },
    FlipCell { profile: Profile::Root, op: FlipOp::SetEuid, x: UID_OTHER, expect: 0, after: (0, 1001, 0), after_fsuid: 0 },
    FlipCell { profile: Profile::Root, op: FlipOp::SetReUid, x: UID_OTHER, expect: 0, after: (1001, 1001, 0), after_fsuid: 0 },
    FlipCell { profile: Profile::Root, op: FlipOp::SetResUid, x: UID_OTHER, expect: 0, after: (1001, 1001, 1001), after_fsuid: 0 },
    FlipCell { profile: Profile::Root, op: FlipOp::SetFsUid, x: UID_OTHER, expect: 0, after: (0, 0, 0), after_fsuid: 1001 },
    FlipCell { profile: Profile::Root, op: FlipOp::SetGroups, x: 0, expect: 0, after: (0, 0, 0), after_fsuid: 0 },
    // user：四路 uid 提限全 EPERM，setfsuid 静默，setgroups 无位 EPERM
    FlipCell { profile: Profile::User, op: FlipOp::SetUid, x: UID_ROOT, expect: EPERM, after: (1000, 1000, 1000), after_fsuid: 1000 },
    FlipCell { profile: Profile::User, op: FlipOp::SetEuid, x: UID_ROOT, expect: EPERM, after: (1000, 1000, 1000), after_fsuid: 1000 },
    FlipCell { profile: Profile::User, op: FlipOp::SetReUid, x: UID_ROOT, expect: EPERM, after: (1000, 1000, 1000), after_fsuid: 1000 },
    FlipCell { profile: Profile::User, op: FlipOp::SetResUid, x: UID_ROOT, expect: EPERM, after: (1000, 1000, 1000), after_fsuid: 1000 },
    FlipCell { profile: Profile::User, op: FlipOp::SetFsUid, x: UID_ROOT, expect: 0, after: (1000, 1000, 1000), after_fsuid: 1000 },
    FlipCell { profile: Profile::User, op: FlipOp::SetGroups, x: 0, expect: EPERM, after: (1000, 1000, 1000), after_fsuid: 1000 },
    // user 池内：x 等于自身 uid，非特权也允许
    FlipCell { profile: Profile::User, op: FlipOp::SetUid, x: UID_USER, expect: 0, after: (1000, 1000, 1000), after_fsuid: 1000 },
    FlipCell { profile: Profile::User, op: FlipOp::SetEuid, x: UID_USER, expect: 0, after: (1000, 1000, 1000), after_fsuid: 1000 },
    FlipCell { profile: Profile::User, op: FlipOp::SetResUid, x: UID_USER, expect: 0, after: (1000, 1000, 1000), after_fsuid: 1000 },
    // cap-holder：CAP_SETUID 放行 uid 提限，CAP_SETGID 缺失仍拒 setgroups
    FlipCell { profile: Profile::CapHolder, op: FlipOp::SetUid, x: UID_ROOT, expect: 0, after: (0, 0, 0), after_fsuid: 1000 },
    FlipCell { profile: Profile::CapHolder, op: FlipOp::SetResUid, x: UID_ROOT, expect: 0, after: (0, 0, 0), after_fsuid: 1000 },
    FlipCell { profile: Profile::CapHolder, op: FlipOp::SetGroups, x: 0, expect: EPERM, after: (1000, 1000, 1000), after_fsuid: 1000 },
];

/// B20 冻结对账行：只覆盖「KEEP 哨兵不改身份」与「root 归零」这一层。
/// 键集与本模块矩阵必须零交集（见 `overlap_keys`）。
pub const B20_FROZEN: [FlipCell; B20_ROWS] = [
    FlipCell { profile: Profile::Root, op: FlipOp::SetReUid, x: UID_KEEP, expect: 0, after: (0, 0, 0), after_fsuid: 0 },
    FlipCell { profile: Profile::User, op: FlipOp::SetReUid, x: UID_KEEP, expect: 0, after: (1000, 1000, 1000), after_fsuid: 1000 },
    FlipCell { profile: Profile::Root, op: FlipOp::SetResUid, x: UID_KEEP, expect: 0, after: (0, 0, 0), after_fsuid: 0 },
    FlipCell { profile: Profile::User, op: FlipOp::SetResUid, x: UID_KEEP, expect: 0, after: (1000, 1000, 1000), after_fsuid: 1000 },
    FlipCell { profile: Profile::Root, op: FlipOp::SetUid, x: UID_ROOT, expect: 0, after: (0, 0, 0), after_fsuid: 0 },
];

/// 执行一格，回传错误码与终态身份账。
pub fn run_cell(cell: FlipCell) -> (i32, Cred) {
    let mut cred = cell.profile.initial();
    let got = apply(&mut cred, cell.op, cell.x);
    (got, cred)
}

/// 单元格是否完全命中：码、三身份、fsuid 三项同时相等。
pub fn cell_ok(cell: FlipCell) -> bool {
    let (code, cred) = run_cell(cell);
    code == cell.expect && cred.triple() == cell.after && cred.fsuid == cell.after_fsuid
}

/// 矩阵命中数。
pub fn matrix_hits() -> u32 {
    let mut hit = 0u32;
    let mut i = 0usize;
    while i < MATRIX.len() {
        if let Some(c) = MATRIX.get(i).copied() {
            if cell_ok(c) { hit = hit.saturating_add(1); }
        }
        i += 1;
    }
    hit
}

/// B20 行命中数。
pub fn b20_hits() -> u32 {
    let mut hit = 0u32;
    let mut i = 0usize;
    while i < B20_FROZEN.len() {
        if let Some(c) = B20_FROZEN.get(i).copied() {
            if cell_ok(c) { hit = hit.saturating_add(1); }
        }
        i += 1;
    }
    hit
}

/// 期望 EPERM 的格数（注入账分母）。
pub fn eperm_cell_count() -> u32 {
    let mut n = 0u32;
    let mut i = 0usize;
    while i < MATRIX.len() {
        if let Some(c) = MATRIX.get(i) {
            if c.expect == EPERM { n = n.saturating_add(1); }
        }
        i += 1;
    }
    n
}

// ── 分层与零重复对账 ──────────────────────────────────────────────────
/// 对账分层：uid 池层（B20 权威）与能力门层（本模块深化）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Layer {
    UidPool,
    CapGate,
}

impl Layer {
    pub fn name(self) -> &'static str {
        match self { Layer::UidPool => "uid-pool", Layer::CapGate => "cap-gate" }
    }
}

/// 矩阵格的分层归属：带能力档属能力门层，其余属 uid 池层。
pub fn layer_of(cell: FlipCell) -> Layer {
    match cell.profile {
        Profile::CapHolder => Layer::CapGate,
        _ => Layer::UidPool,
    }
}

/// 键集交集：>0 即重复分层。
pub fn overlap_keys() -> u32 {
    let mut n = 0u32;
    let mut i = 0usize;
    while i < MATRIX.len() {
        if let Some(a) = MATRIX.get(i).copied() {
            let mut j = 0usize;
            while j < B20_FROZEN.len() {
                if let Some(b) = B20_FROZEN.get(j).copied() {
                    if a.key() == b.key() { n = n.saturating_add(1); }
                }
                j += 1;
            }
        }
        i += 1;
    }
    n
}

/// 分层完备：两层各有行，且 B20 侧全部落在 uid 池层。
pub fn layer_complete() -> bool {
    let mut pool = 0u32;
    let mut cap = 0u32;
    let mut i = 0usize;
    while i < MATRIX.len() {
        if let Some(c) = MATRIX.get(i).copied() {
            match layer_of(c) {
                Layer::UidPool => pool = pool.saturating_add(1),
                Layer::CapGate => cap = cap.saturating_add(1),
            }
        }
        i += 1;
    }
    let b20_pool = B20_FROZEN.iter().all(|c| layer_of(*c) == Layer::UidPool);
    pool > 0 && cap > 0 && b20_pool
}

// ── 三身份守恒 ────────────────────────────────────────────────────────
/// 守恒式：结果每个 id 只能是旧值或入参之一。
pub fn ids_sourced(before: (u32, u32, u32), after: (u32, u32, u32), x: u32) -> bool {
    let src = [before.0, before.1, before.2, x, UID_KEEP];
    let got = [after.0, after.1, after.2];
    let mut i = 0usize;
    while i < got.len() {
        if let Some(v) = got.get(i) {
            if !src.contains(v) { return false; }
        }
        i += 1;
    }
    true
}

/// 全矩阵守恒检查（含 fsuid）。
pub fn conservation_ok() -> bool {
    let mut i = 0usize;
    while i < MATRIX.len() {
        if let Some(c) = MATRIX.get(i).copied() {
            let (_, cred) = run_cell(c);
            let init = c.profile.initial();
            if !ids_sourced(init.triple(), cred.triple(), c.x) { return false; }
            if cred.fsuid != init.fsuid && cred.fsuid != c.x { return false; }
        }
        i += 1;
    }
    true
}

// ── 状态机 ────────────────────────────────────────────────────────────
/// 翻转状态机：观察 → 判定 → 落地 → 提交，单向逐档。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Stage {
    Idle,
    Inspect,
    Decide,
    Apply,
    Commit,
}

impl Stage {
    pub fn step(self) -> Option<Stage> {
        match self {
            Stage::Idle => Some(Stage::Inspect),
            Stage::Inspect => Some(Stage::Decide),
            Stage::Decide => Some(Stage::Apply),
            Stage::Apply => Some(Stage::Commit),
            Stage::Commit => None,
        }
    }
}

/// 只接受恰好下一档；回退与跳档都拒。
pub fn advance(from: Stage, to: Stage) -> Result<Stage, FlipCode> {
    match from.step() {
        Some(next) => if next == to { Ok(to) } else { Err(FlipCode::StageRegress) },
        None => Err(FlipCode::StageRegress),
    }
}

/// 被拒格停在 Decide，成功格走到 Commit。
pub fn stage_path(ok: bool) -> Result<Stage, FlipCode> {
    let mut st = Stage::Idle;
    st = advance(st, Stage::Inspect)?;
    st = advance(st, Stage::Decide)?;
    if ok {
        st = advance(st, Stage::Apply)?;
        st = advance(st, Stage::Commit)?;
    }
    Ok(st)
}

// ── 注入账 ────────────────────────────────────────────────────────────
/// 注入账：注入 = 检出 + 逃逸。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Ledger {
    pub injected: u32,
    pub detected: u32,
    pub escaped: u32,
}

impl Ledger {
    pub const fn zero() -> Ledger {
        Ledger { injected: 0, detected: 0, escaped: 0 }
    }
    pub fn record(&mut self, caught: bool) -> Result<(), FlipCode> {
        self.injected = self.injected.saturating_add(1);
        if caught { self.detected = self.detected.saturating_add(1); }
        else { self.escaped = self.escaped.saturating_add(1); }
        if self.detected.saturating_add(self.escaped) != self.injected {
            return Err(FlipCode::LedgerLeak);
        }
        Ok(())
    }
    pub fn conserved(&self) -> bool {
        self.detected.saturating_add(self.escaped) == self.injected
    }
}

/// EPERM 六格 × 10 轮提限注入；回传账本与逐格检出数。
pub fn run_escalation_matrix() -> (Ledger, [u32; MATRIX_CELLS]) {
    let mut ledger = Ledger::zero();
    let mut per = [0u32; MATRIX_CELLS];
    let mut i = 0usize;
    while i < MATRIX.len() {
        if let Some(c) = MATRIX.get(i).copied() {
            if c.expect == EPERM {
                let mut round = 0u32;
                while round < INJECT_ROUNDS {
                    let (code, _) = run_cell(c);
                    let caught = code == EPERM;
                    let _ = ledger.record(caught);
                    if caught {
                        if let Some(slot) = per.get_mut(i) { *slot = slot.saturating_add(1); }
                    }
                    round += 1;
                }
            }
        }
        i += 1;
    }
    (ledger, per)
}

// ── 指纹 ──────────────────────────────────────────────────────────────
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

/// 语料指纹：矩阵终态 + B20 命中 + 重叠键数。
pub fn corpus_fingerprint() -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    let mut i = 0usize;
    while i < MATRIX.len() {
        if let Some(c) = MATRIX.get(i).copied() {
            let (code, cred) = run_cell(c);
            h ^= fnv1a(c.op.name().as_bytes());
            h = h.wrapping_mul(0x0100_0193);
            h ^= code as u32;
            h = h.wrapping_mul(0x0100_0193);
            h ^= cred.ruid ^ cred.euid.rotate_left(7) ^ cred.suid.rotate_left(13);
            h = h.wrapping_mul(0x0100_0193);
            h ^= cred.fsuid;
        }
        i += 1;
    }
    h ^= b20_hits();
    h = h.wrapping_mul(0x0100_0193);
    h ^= overlap_keys();
    h
}

// ── 检查入口 ──────────────────────────────────────────────────────────
/// F9271 判据集（≥12 条；动态明细 ≤8 条）。
pub fn run_unx9271_checks() -> CheckSet {
    let mut s = CheckSet::new("unx9271");

    // 1. 段位守卫
    let seg_ok = SEG_BASE % 0x40 == 0 && (0x7A000..=0x7D1C0).contains(&SEG_BASE);
    s.add("段位守卫：0x40 对齐且落在 0x7A000..=0x7D1C0", seg_ok,
          "段基址必须独占且落在本批窗口；越界即拒绝注册");

    // 2. 正常流：root 提限成功且三身份同步落地
    let mut root = Profile::Root.initial();
    let root_ok = apply(&mut root, FlipOp::SetUid, UID_OTHER) == 0
        && root.triple() == (UID_OTHER, UID_OTHER, UID_OTHER);
    s.add("正常流：root setuid(1001) 三身份同步翻转", root_ok,
          "特权档的翻转必须一次落齐 ruid/euid/suid，不许只改 euid");

    // 3. 全 EPERM 逐格检出
    let mut eperm_hit = 0u32;
    let mut i = 0usize;
    while i < MATRIX.len() {
        if let Some(c) = MATRIX.get(i).copied() {
            if c.expect == EPERM {
                let (code, cred) = run_cell(c);
                if code == EPERM && cred.triple() == c.after {
                    eperm_hit = eperm_hit.saturating_add(1);
                }
            }
        }
        i += 1;
    }
    s.add("全 EPERM 逐格：6 格全部 EPERM 且身份零变化",
          eperm_hit == EPERM_CELLS as u32 && eperm_cell_count() == EPERM_CELLS as u32,
          "被拒的翻转必须一个 id 都不动，动一格即提权逃逸");

    // 4. 非特权提限注入 10/10
    let (ledger, per) = run_escalation_matrix();
    let mut all_ten = true;
    let mut k = 0usize;
    while k < per.len() {
        if let Some(c) = MATRIX.get(k).copied() {
            if c.expect == EPERM {
                if let Some(n) = per.get(k) {
                    if *n != INJECT_ROUNDS { all_ten = false; }
                }
            }
        }
        k += 1;
    }
    s.add("非特权提限注入：六格各 10/10 全检出", all_ten,
          "每格每轮都必须复现 EPERM，否则提限判据不成立");

    // 5. 注入账守恒
    s.add("注入账守恒：注入=检出+逃逸且逃逸=0",
          ledger.conserved() && ledger.escaped == 0
              && ledger.injected == EPERM_CELLS as u32 * INJECT_ROUNDS,
          "60 次注入对应 60 次检出，逃逸恒为零");

    // 6. 判定次序：越界 uid 先回 EINVAL
    let mut user = Profile::User.initial();
    let inv = apply(&mut user, FlipOp::SetUid, UID_KEEP);
    let order_ok = inv == EINVAL && user.triple() == (UID_USER, UID_USER, UID_USER);
    s.add("判定次序：越界 uid 报 EINVAL 而非 EPERM", order_ok,
          "值域门先于权限门；先报 EPERM 会掩盖越界参数");

    // 7. 能力位不串用：CAP_SETUID 不等于 CAP_SETGID
    let mut cap = Profile::CapHolder.initial();
    let cap_uid_ok = apply(&mut cap, FlipOp::SetEuid, UID_ROOT) == 0 && cap.euid == UID_ROOT;
    let mut cap2 = Profile::CapHolder.initial();
    let cap_gid_denied = apply(&mut cap2, FlipOp::SetGroups, 0) == EPERM;
    s.add("能力面：CAP_SETUID 放行 uid 翻转，setgroups 仍 EPERM", cap_uid_ok && cap_gid_denied,
          "能力位逐位授权，一个 cap 管全族会造成越权");

    // 8. setfsuid 静默：无权不报错也不生效
    let mut u2 = Profile::User.initial();
    let fs_silent = apply(&mut u2, FlipOp::SetFsUid, UID_ROOT) == 0
        && u2.fsuid == UID_USER && u2.triple() == (UID_USER, UID_USER, UID_USER);
    let mut r2 = Profile::Root.initial();
    let fs_landed = apply(&mut r2, FlipOp::SetFsUid, UID_OTHER) == 0 && r2.fsuid == UID_OTHER;
    s.add("fsuid 面：无权静默丢弃、有权立即生效", fs_silent && fs_landed,
          "setfsuid 从不回 EPERM；算进 EPERM 格会把正常调用判成逃逸");

    // 9. 特权不缓存：降权后能力位清空，再次提限 EPERM
    let mut flip_back = Profile::Root.initial();
    let dropped = apply(&mut flip_back, FlipOp::SetUid, UID_OTHER) == 0
        && !flip_back.privileged()
        && apply(&mut flip_back, FlipOp::SetUid, UID_ROOT) == EPERM;
    s.add("特权不缓存：降权后清空能力位，再次提限 EPERM", dropped,
          "权限由当前 euid/cap 现算，缓存会留出残留特权窗口");

    // 10. 三身份守恒
    s.add("三身份守恒：结果 id 只取旧值或入参", conservation_ok(),
          "凭空生成的 uid 无法溯源，等价于身份伪造");

    // 11. 矩阵十八格全命中
    let hits = matrix_hits();
    s.add("矩阵十八格：码/三身份/fsuid 三项全等", hits == MATRIX_CELLS as u32,
          "任一格漂移即判定器与冻结矩阵失去同步");

    // 12. B20 对账：五行全命中
    let b20 = b20_hits();
    s.add("B20 对账：五行冻结期望全命中", b20 == B20_ROWS as u32,
          "B20 是外部权威，漂移时必须先校正本模块");

    // 13. 零重复分层
    let overlap = overlap_keys();
    s.add("零重复分层：B20 键集与本矩阵交集为空", overlap == 0,
          "同一 (档,操作,入参) 键由两张表覆盖即重复分层");

    // 14. 分层完备
    s.add("分层完备：uid 池层与能力门层各有行，B20 全在池层", layer_complete(),
          "缺层等于对账盲区，CapGate 无行则能力语义无人覆盖");

    // 15. 状态机单向
    let regress = advance(Stage::Commit, Stage::Apply).is_err();
    let skip = advance(Stage::Inspect, Stage::Commit).is_err();
    let stage_ok = regress && skip && stage_path(true) == Ok(Stage::Commit)
        && stage_path(false) == Ok(Stage::Decide);
    s.add("状态机：单向逐档，回退/跳档均拒", stage_ok,
          "被拒格停 Decide、成功格进 Commit，档位不可逆");

    // 16. 双跑一致
    let a = corpus_fingerprint();
    let b = corpus_fingerprint();
    s.add("双跑一致：语料指纹相等", a == b, "同输入两次必须逐格相等，隐藏状态即失败");

    // 17. 文案闭集
    s.add("文案闭集：12 个诊断码均有非空中文建议",
          FlipCode::ALL.iter().all(|c| !c.detail().is_empty()),
          "每个码必须能取到可操作说明，空串即判据不可读");

    // 18. 诊断码唯一
    let mut uniq = true;
    let mut x = 0usize;
    while x < FlipCode::ALL.len() {
        let mut y = x + 1;
        while y < FlipCode::ALL.len() {
            if let (Some(p), Some(q)) = (FlipCode::ALL.get(x), FlipCode::ALL.get(y)) {
                if p.val() == q.val() { uniq = false; }
            }
            y += 1;
        }
        x += 1;
    }
    s.add("诊断码：12 个码值两两唯一", uniq, "闭集码重复会让判据无法定位到具体缺陷");

    // 动态明细（≤8）
    let ledger_text = format!("注入 {} / 检出 {} / 逃逸 {}", ledger.injected, ledger.detected, ledger.escaped);
    s.add_dyn("账目快照", ledger.conserved() && ledger.escaped == 0, &ledger_text);
    let fp_text = format!("语料指纹 0x{:08X}", a);
    s.add_dyn("指纹快照", a == b, &fp_text);
    let layer_text = format!("{} 行命中 · 重叠键 {} · 层 {}", b20, overlap, LAYER_NAME);
    s.add_dyn("对账快照", b20 == B20_ROWS as u32 && overlap == 0, &layer_text);

    s
}

// ── 测试 ──────────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn root_flip_sets_all_three() {
        let mut c = Profile::Root.initial();
        assert_eq!(apply(&mut c, FlipOp::SetUid, UID_OTHER), 0);
        assert_eq!(c.triple(), (UID_OTHER, UID_OTHER, UID_OTHER));
    }

    #[test]
    fn user_escalation_all_eperm() {
        let cases = [
            (FlipOp::SetUid, UID_ROOT), (FlipOp::SetEuid, UID_ROOT),
            (FlipOp::SetReUid, UID_ROOT), (FlipOp::SetResUid, UID_ROOT),
        ];
        for (op, x) in cases.iter() {
            let mut c = Profile::User.initial();
            assert_eq!(apply(&mut c, *op, *x), EPERM, "op {}", op.name());
            assert_eq!(c.triple(), (UID_USER, UID_USER, UID_USER));
        }
    }

    #[test]
    fn cap_holder_uid_ok_groups_denied() {
        let mut c = Profile::CapHolder.initial();
        assert_eq!(apply(&mut c, FlipOp::SetUid, UID_ROOT), 0);
        assert_eq!(c.triple(), (0, 0, 0));
        let mut d = Profile::CapHolder.initial();
        assert_eq!(apply(&mut d, FlipOp::SetGroups, 0), EPERM);
    }

    #[test]
    fn setfsuid_silent_and_landed() {
        let mut u = Profile::User.initial();
        assert_eq!(apply(&mut u, FlipOp::SetFsUid, UID_ROOT), 0);
        assert_eq!(u.fsuid, UID_USER);
        let mut r = Profile::Root.initial();
        assert_eq!(apply(&mut r, FlipOp::SetFsUid, UID_OTHER), 0);
        assert_eq!(r.fsuid, UID_OTHER);
    }

    #[test]
    fn value_gate_before_permission() {
        let mut c = Profile::User.initial();
        assert_eq!(apply(&mut c, FlipOp::SetUid, UID_KEEP), EINVAL);
        assert_eq!(apply(&mut c, FlipOp::SetFsUid, UID_KEEP), EINVAL);
        assert_eq!(c.triple(), (UID_USER, UID_USER, UID_USER));
    }

    #[test]
    fn keep_sentinel_is_noop() {
        let mut r = Profile::Root.initial();
        assert_eq!(apply(&mut r, FlipOp::SetReUid, UID_KEEP), 0);
        assert_eq!(r.triple(), (0, 0, 0));
        let mut u = Profile::User.initial();
        assert_eq!(apply(&mut u, FlipOp::SetResUid, UID_KEEP), 0);
        assert_eq!(u.triple(), (UID_USER, UID_USER, UID_USER));
    }

    #[test]
    fn privilege_not_cached() {
        let mut c = Profile::Root.initial();
        assert_eq!(apply(&mut c, FlipOp::SetUid, UID_OTHER), 0);
        assert!(!c.privileged());
        assert_eq!(apply(&mut c, FlipOp::SetUid, UID_ROOT), EPERM);
    }

    #[test]
    fn matrix_and_b20_all_hit() {
        assert_eq!(matrix_hits(), MATRIX_CELLS as u32);
        assert_eq!(b20_hits(), B20_ROWS as u32);
        for c in MATRIX.iter() {
            assert!(cell_ok(*c), "{} {}", c.profile.name(), c.op.name());
        }
        for c in B20_FROZEN.iter() {
            assert!(cell_ok(*c), "b20 {} {}", c.profile.name(), c.op.name());
        }
    }

    #[test]
    fn layers_disjoint_and_complete() {
        assert_eq!(overlap_keys(), 0);
        assert!(layer_complete());
        let mut pool = 0u32;
        let mut cap = 0u32;
        for c in MATRIX.iter() {
            match layer_of(*c) {
                Layer::UidPool => pool += 1,
                Layer::CapGate => cap += 1,
            }
        }
        assert!(pool > 0 && cap > 0);
    }

    #[test]
    fn conservation_holds() {
        assert!(conservation_ok());
        assert!(!ids_sourced((1000, 1000, 1000), (1000, 0, 1000), UID_USER));
        assert!(ids_sourced((1000, 1000, 1000), (1000, 1000, 1000), UID_ROOT));
    }

    #[test]
    fn injection_ledger_conserved() {
        let (l, per) = run_escalation_matrix();
        assert!(l.conserved());
        assert_eq!(l.escaped, 0);
        assert_eq!(l.injected, EPERM_CELLS as u32 * INJECT_ROUNDS);
        for (i, c) in MATRIX.iter().enumerate() {
            let n = per.get(i).copied().unwrap_or(0);
            if c.expect == EPERM { assert_eq!(n, INJECT_ROUNDS); }
            else { assert_eq!(n, 0); }
        }
    }

    #[test]
    fn stage_machine_monotonic() {
        assert_eq!(stage_path(true), Ok(Stage::Commit));
        assert_eq!(stage_path(false), Ok(Stage::Decide));
        assert!(advance(Stage::Commit, Stage::Apply).is_err());
        assert!(advance(Stage::Inspect, Stage::Commit).is_err());
        assert_eq!(corpus_fingerprint(), corpus_fingerprint());
    }

    #[test]
    fn checks_all_green() {
        let s = run_unx9271_checks();
        assert!(s.all_passed(), "{}", s.red_items_text());
    }
}
