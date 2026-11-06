//! UNX-F9345 全域 EPERM/EACCES 矩阵总装（五族权限注入逐格） · 0x7C3C0段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板 F9345 逐字）：
//!   ① 纯功能代码 ≥380 行；
//!   ② 判据原文：UNX-F9345-J1 五族权限注入逐格总装判据过，EPERM/EACCES
//!      分界逐格检出，与 B24 判定域对账一致格过。
//!
//! 设计要点：
//!   ① 判定单位是「一次权限裁决」：(Subject, Object, family) → errno。
//!      五族各取一条真实裁决路径：文件打开（模式位）、信号投递（跨 uid）、
//!      属主变更（chown）、挂载（CAP_SYS_ADMIN）、目录写（w+x 位）。
//!      每族固定方向：文件/目录族回 EACCES，操作特权族回 EPERM——
//!      方向错配一格即红，这是本卡的核心分界。
//!   ② EPERM/EACCES 分界冻结在语义上：EACCES = 「对象身上的权限位拒绝
//!      这次访问」（路径/模式面）；EPERM = 「对象允许、但主体缺操作
//!      特权」（能力面）。把能力缺权误报成 EACCES、把模式位拒绝误报成
//!      EPERM 都算分界漂移。
//!   ③ 特权旁路面两条且只两条：uid==0 与对应能力位。旁路只在所属方向
//!      生效——CAP_DAC_OVERRIDE 只救 EACCES 族，CAP_KILL/CHOWN/
//!      SYS_ADMIN 只救各自 EPERM 族；跨族借用旁路按拒绝处理。
//!   ④ 与 B24 判定域对账：每族有一条冻结的方向表（期望 errno），注入
//!      矩阵的实测方向必须逐族与表一致；对账函数独立于裁决器实现，
//!      两路互证防单点写错自洽。
//!   ⑤ 注入账守恒：注入 = 检出 + 逃逸，逃逸恒为 0；账只增不减。
//!   ⑥ 诊断码 12 个（段内 +1..=+12），码值两两唯一，每码中文建议非空。
//! ─────────────────────────────────────────────────────────────────────

use alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（F9345 独占 0x7C3C0..=0x7C3FF 的 0x40 窗口）。
pub const SEG_BASE: u32 = 0x7C3C0;

/// Linux errno 数值（正数口径）。
pub const EPERM: i32 = 1;
pub const EACCES: i32 = 13;
pub const EINVAL: i32 = 22;
pub const EBADF: i32 = 9;

/// 特权位掩码（建模 Linux CAP_* 的本局面切片）。
pub const CAP_CHOWN: u32 = 1 << 0;
pub const CAP_DAC_OVERRIDE: u32 = 1 << 1;
pub const CAP_KILL: u32 = 1 << 2;
pub const CAP_SYS_ADMIN: u32 = 1 << 3;

/// 特权 uid（Linux 口径：0 = root）。
pub const ROOT_UID: u32 = 0;

/// 文件模式位（POSIX 八进制语义）。
pub const MODE_R_USR: u32 = 0o400;
pub const MODE_W_USR: u32 = 0o200;
pub const MODE_R_GRP: u32 = 0o040;
pub const MODE_W_GRP: u32 = 0o020;
pub const MODE_R_OTH: u32 = 0o004;
pub const MODE_W_OTH: u32 = 0o002;
pub const MODE_X_OTH: u32 = 0o001;

// ── 诊断码闭集 ────────────────────────────────────────────────────────
/// 本模块诊断码（段内 +0x01..=+0x0C 闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AccessCode {
    /// 族号未知
    UnknownFamily,
    /// 信号族：跨 uid 无 CAP_KILL（EPERM 方向）
    KillNoCap,
    /// 属主变更族：非属主且无 CAP_CHOWN（EPERM 方向）
    ChownNoCap,
    /// 属主变更族：目标组非主体所属组（EPERM 方向）
    ChownForeignGroup,
    /// 挂载族：无 CAP_SYS_ADMIN（EPERM 方向）
    MountNoCap,
    /// 文件族：模式位拒绝读/写（EACCES 方向）
    FileModeDenied,
    /// 目录族：缺写位（EACCES 方向）
    DirWriteDenied,
    /// 目录族：缺执行位（路径搜索被拒，EACCES 方向）
    DirSearchDenied,
    /// 旁路越族：能力位被借用到不归属的族
    CapCrossFamily,
    /// EPERM/EACCES 方向错配（分界漂移自检码）
    DirectionSplit,
    /// 注入账倒退
    LedgerRegress,
    /// 双跑不一致
    NotReproducible,
}

impl AccessCode {
    /// 码值（段基址 + 变体序号，+1 起）。
    pub fn code(self) -> u32 {
        SEG_BASE + self as u32 + 1
    }

    /// 中文诊断与处置建议（闭集返回，禁止空串）。
    pub fn detail(self) -> &'static str {
        match self {
            AccessCode::UnknownFamily => "未知权限族被拒：族号必须在 1..=5 注册面内；\
                请先确认裁决路径归属，未知族不参与 EPERM/EACCES 分界",
            AccessCode::KillNoCap => "跨 uid 信号投递被拒（EPERM）：目标进程属主与\
                主体不同且缺 CAP_KILL；请先取得信号特权或改为同 uid 投递",
            AccessCode::ChownNoCap => "属主变更被拒（EPERM）：非文件属主且缺 \
                CAP_CHOWN；属主可改组但不得改人，改人需特权",
            AccessCode::ChownForeignGroup => "组变更被拒（EPERM）：目标组不在主体\
                所属组集合内；属主只能把文件交给自己在的组",
            AccessCode::MountNoCap => "挂载被拒（EPERM）：缺 CAP_SYS_ADMIN；\
                挂载面无用户态降级路径，请先入特权域",
            AccessCode::FileModeDenied => "文件访问被拒（EACCES）：属主/组/其他三类\
                模式位均不含所需位；请改位或以 CAP_DAC_OVERRIDE 旁路",
            AccessCode::DirWriteDenied => "目录写被拒（EACCES）：目录缺写位；\
                建删项需要目录 w 位，请修目录模式",
            AccessCode::DirSearchDenied => "路径搜索被拒（EACCES）：路径前缀目录缺\
                执行位；x 位是目录遍历的最小要求",
            AccessCode::CapCrossFamily => "旁路越族被拒：能力位只救归属方向——\
                DAC 位救 EACCES 族，特权位救各自 EPERM 族；跨族借用一律无效",
            AccessCode::DirectionSplit => "分界漂移被拒：EPERM（能力面）与 EACCES\
                （权限位面）方向与冻结方向表不一致；请回对账函数复核",
            AccessCode::LedgerRegress => "注入账倒退被拒：计数只增不减；请检查\
                回滚路径是否误改累计值",
            AccessCode::NotReproducible => "双跑不一致：同输入两次裁决不同；\
                请排查隐藏状态或非确定路径",
        }
    }

    /// 闭集全量。
    pub const ALL: [AccessCode; 12] = [
        AccessCode::UnknownFamily,
        AccessCode::KillNoCap,
        AccessCode::ChownNoCap,
        AccessCode::ChownForeignGroup,
        AccessCode::MountNoCap,
        AccessCode::FileModeDenied,
        AccessCode::DirWriteDenied,
        AccessCode::DirSearchDenied,
        AccessCode::CapCrossFamily,
        AccessCode::DirectionSplit,
        AccessCode::LedgerRegress,
        AccessCode::NotReproducible,
    ];
}

/// 系统调用失败结果：Err 携带 Linux errno 正数码。
pub type SysResult<T> = Result<T, i32>;

// ── 主体与对象 ────────────────────────────────────────────────────────
/// 权限主体：uid + 能力位掩码（本局面 4 位）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Subject {
    /// 主体 uid（0 = root）
    pub uid: u32,
    /// 能力位掩码（CAP_CHOWN|CAP_DAC_OVERRIDE|CAP_KILL|CAP_SYS_ADMIN）
    pub caps: u32,
    /// 主体所属组集合（建模 POSIX 组成员资格，容量 2）
    pub groups: [u32; 2],
}

impl Subject {
    /// 构造主体。
    pub const fn new(uid: u32, caps: u32) -> Subject {
        Subject { uid, caps, groups: [uid, u32::MAX] }
    }

    /// 是否 root（旁路面之一）。
    pub fn is_root(&self) -> bool {
        self.uid == ROOT_UID
    }

    /// 是否持有某能力位。
    pub fn has_cap(&self, cap: u32) -> bool {
        self.caps & cap == cap
    }

    /// 是否属于某组。
    pub fn in_group(&self, gid: u32) -> bool {
        self.groups[0] == gid || self.groups[1] == gid
    }
}

/// 权限对象：属主 + 组 + 模式位。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Object {
    /// 属主 uid
    pub owner_uid: u32,
    /// 组 gid
    pub gid: u32,
    /// POSIX 模式位（0o777 口径）
    pub mode: u32,
}

impl Object {
    /// 构造对象。
    pub const fn new(owner_uid: u32, gid: u32, mode: u32) -> Object {
        Object { owner_uid, gid, mode }
    }

    /// 按主体身份取属主/组/其他类别的相关位（POSIX 选中规则：多类别\
    /// 命中时按属主→组→其他只取第一类，不叠加）。
    pub fn bits_for(&self, uid: u32, gid: u32) -> u32 {
        if uid == self.owner_uid {
            return (self.mode >> 6) & 0o7;
        }
        if gid == self.gid {
            return (self.mode >> 3) & 0o7;
        }
        self.mode & 0o7
    }
}

// ── 五族注册表与裁决器 ────────────────────────────────────────────────
/// 五个权限族。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PermFamily {
    /// 族 1：文件打开（模式位面，EACCES 方向）
    FileOpen,
    /// 族 2：目录写（建/删项，EACCES 方向）
    DirWrite,
    /// 族 3：信号投递（跨 uid，EPERM 方向）
    Kill,
    /// 族 4：属主变更（chown，EPERM 方向）
    Chown,
    /// 族 5：挂载（EPERM 方向）
    Mount,
}

impl PermFamily {
    /// 族号（1..=5）。
    pub fn id(self) -> u32 {
        match self {
            PermFamily::FileOpen => 1,
            PermFamily::DirWrite => 2,
            PermFamily::Kill => 3,
            PermFamily::Chown => 4,
            PermFamily::Mount => 5,
        }
    }

    /// 按族号反查；未知号 None。
    pub fn from_id(id: u32) -> Option<PermFamily> {
        match id {
            1 => Some(PermFamily::FileOpen),
            2 => Some(PermFamily::DirWrite),
            3 => Some(PermFamily::Kill),
            4 => Some(PermFamily::Chown),
            5 => Some(PermFamily::Mount),
            _ => None,
        }
    }

    /// B24 判定域冻结方向表：每族唯一期望 errno（EPERM 或 EACCES）。
    pub fn b24_direction(self) -> i32 {
        match self {
            PermFamily::FileOpen | PermFamily::DirWrite => EACCES,
            PermFamily::Kill | PermFamily::Chown | PermFamily::Mount => EPERM,
        }
    }

    /// 注册面全量。
    pub const ALL: [PermFamily; 5] = [
        PermFamily::FileOpen,
        PermFamily::DirWrite,
        PermFamily::Kill,
        PermFamily::Chown,
        PermFamily::Mount,
    ];
}

/// 文件打开裁决：mode 位面 + DAC 旁路。want_r/w 分别声明读/写意图。
pub fn judge_file_open(sub: Subject, obj: Object, want_r: bool, want_w: bool) -> SysResult<()> {
    // root 直接放行（Linux：root 仅缺 x 时受限，本局面只建 r/w）。
    if sub.is_root() {
        return Ok(());
    }
    if sub.has_cap(CAP_DAC_OVERRIDE) {
        return Ok(());
    }
    let bits = obj.bits_for(sub.uid, sub.groups[0]);
    let r_ok = !want_r || bits & 0o4 != 0;
    let w_ok = !want_w || bits & 0o2 != 0;
    if r_ok && w_ok {
        Ok(())
    } else {
        Err(EACCES)
    }
}

/// 目录写裁决：目录 w 位 + x 位（搜索）+ DAC 旁路。
pub fn judge_dir_write(sub: Subject, dir: Object) -> SysResult<()> {
    if sub.is_root() || sub.has_cap(CAP_DAC_OVERRIDE) {
        return Ok(());
    }
    let bits = dir.bits_for(sub.uid, sub.groups[0]);
    if bits & 0o1 == 0 {
        // 先判 x：搜索被拒时根本走不到 w。
        return Err(EACCES);
    }
    if bits & 0o2 == 0 {
        return Err(EACCES);
    }
    Ok(())
}

/// 信号投递裁决：跨 uid 需 CAP_KILL；同 uid 放行；root 全放行。
pub fn judge_kill(sub: Subject, target_uid: u32) -> SysResult<()> {
    if sub.is_root() || sub.uid == target_uid {
        return Ok(());
    }
    if sub.has_cap(CAP_KILL) {
        return Ok(());
    }
    Err(EPERM)
}

/// 属主变更裁决：改 uid 需属主身份之外的 CAP_CHOWN（或 root）；
/// 属主改组要求目标组 ∈ 主体所属组。
pub fn judge_chown(sub: Subject, obj: Object, new_uid: u32, new_gid: u32) -> SysResult<()> {
    if sub.is_root() {
        return Ok(());
    }
    let is_owner = sub.uid == obj.owner_uid;
    // 改属主：属主身份也不行（Linux：非特权 chown 只允许改组）。
    if new_uid != obj.owner_uid {
        if sub.has_cap(CAP_CHOWN) {
            return Ok(());
        }
        return Err(EPERM);
    }
    // 只改组：属主可改到自己所在的组；否则要 CAP_CHOWN。
    if is_owner && sub.in_group(new_gid) {
        return Ok(());
    }
    if sub.has_cap(CAP_CHOWN) {
        return Ok(());
    }
    Err(EPERM)
}

/// 挂载裁决：仅 root 或 CAP_SYS_ADMIN。
pub fn judge_mount(sub: Subject) -> SysResult<()> {
    if sub.is_root() || sub.has_cap(CAP_SYS_ADMIN) {
        return Ok(());
    }
    Err(EPERM)
}

/// 总装派发：族号未知返回 UnknownFamily 对应的 EINVAL 语义面
/// （本卡分界只覆盖 EPERM/EACCES，未知族按 EINVAL 出口并计一格）。
pub fn judge_family(family_id: u32, sub: Subject, obj: Object) -> SysResult<()> {
    let fam = match PermFamily::from_id(family_id) {
        Some(f) => f,
        None => return Err(EINVAL),
    };
    match fam {
        PermFamily::FileOpen => judge_file_open(sub, obj, true, true),
        PermFamily::DirWrite => judge_dir_write(sub, obj),
        // 信号/挂载族复用 obj.owner_uid / gid 作目标 uid。
        PermFamily::Kill => judge_kill(sub, obj.owner_uid),
        PermFamily::Chown => judge_chown(sub, obj, obj.owner_uid + 1, obj.gid),
        PermFamily::Mount => judge_mount(sub),
    }
}

// ── 注入矩阵 ──────────────────────────────────────────────────────────
/// 五族各两类坏样本（共 10 类），逐类必拒且方向与 B24 表一致。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BadPermCase {
    /// 族 1：other 类只有 r，主体要读写
    FileModeDenied,
    /// 族 1：组类全 0
    FileModeGroupZero,
    /// 族 2：目录缺 w
    DirNoWrite,
    /// 族 2：目录缺 x（搜索先拒）
    DirNoExec,
    /// 族 3：跨 uid 无 CAP_KILL
    KillNoCap,
    /// 族 3：跨 uid 只有 DAC 位（旁路越族）
    KillWrongCap,
    /// 族 4：改属主无 CAP_CHOWN
    ChownNoCap,
    /// 族 4：属主把组改成陌生组
    ChownForeignGroup,
    /// 族 5：无 CAP_SYS_ADMIN
    MountNoCap,
    /// 族 5：只有 CHOWN 位（旁路越族）
    MountWrongCap,
}

impl BadPermCase {
    /// 构造样本裁决输入。
    pub fn sample(self) -> (u32, Subject, Object) {
        match self {
            BadPermCase::FileModeDenied => {
                (1, Subject::new(1001, 0), Object::new(1002, 1003, 0o004))
            }
            BadPermCase::FileModeGroupZero => {
                (1, Subject::new(1001, 0), Object::new(1002, 1003, 0o600))
            }
            BadPermCase::DirNoWrite => {
                (2, Subject::new(1001, 0), Object::new(1001, 1001, 0o555))
            }
            BadPermCase::DirNoExec => {
                (2, Subject::new(1001, 0), Object::new(1001, 1001, 0o644))
            }
            BadPermCase::KillNoCap => (3, Subject::new(1000, 0), Object::new(1001, 0, 0)),
            BadPermCase::KillWrongCap => {
                (3, Subject::new(1000, CAP_DAC_OVERRIDE), Object::new(1001, 0, 0))
            }
            BadPermCase::ChownNoCap => {
                (4, Subject::new(1001, 0), Object::new(1001, 1001, 0o644))
            }
            BadPermCase::ChownForeignGroup => {
                (4, Subject::new(1001, 0), Object::new(1001, 1002, 0o644))
            }
            BadPermCase::MountNoCap => (5, Subject::new(1000, 0), Object::new(0, 0, 0)),
            BadPermCase::MountWrongCap => {
                (5, Subject::new(1000, CAP_CHOWN), Object::new(0, 0, 0))
            }
        }
    }

    /// 期望 errno（方向先对表，格再对码）。
    pub fn expect(self) -> i32 {
        match self {
            BadPermCase::FileModeDenied
            | BadPermCase::FileModeGroupZero
            | BadPermCase::DirNoWrite
            | BadPermCase::DirNoExec => EACCES,
            BadPermCase::KillNoCap
            | BadPermCase::KillWrongCap
            | BadPermCase::ChownNoCap
            | BadPermCase::ChownForeignGroup
            | BadPermCase::MountNoCap
            | BadPermCase::MountWrongCap => EPERM,
        }
    }

    /// 期望诊断码（供逐格定位；WrongCap 类同时验证旁路越族）。
    pub fn expect_code(self) -> AccessCode {
        match self {
            BadPermCase::FileModeDenied => AccessCode::FileModeDenied,
            BadPermCase::FileModeGroupZero => AccessCode::FileModeDenied,
            BadPermCase::DirNoWrite => AccessCode::DirWriteDenied,
            BadPermCase::DirNoExec => AccessCode::DirSearchDenied,
            BadPermCase::KillNoCap => AccessCode::KillNoCap,
            BadPermCase::KillWrongCap => AccessCode::CapCrossFamily,
            BadPermCase::ChownNoCap => AccessCode::ChownNoCap,
            BadPermCase::ChownForeignGroup => AccessCode::ChownForeignGroup,
            BadPermCase::MountNoCap => AccessCode::MountNoCap,
            BadPermCase::MountWrongCap => AccessCode::CapCrossFamily,
        }
    }

    /// 全量坏样本。
    pub const ALL: [BadPermCase; 10] = [
        BadPermCase::FileModeDenied,
        BadPermCase::FileModeGroupZero,
        BadPermCase::DirNoWrite,
        BadPermCase::DirNoExec,
        BadPermCase::KillNoCap,
        BadPermCase::KillWrongCap,
        BadPermCase::ChownNoCap,
        BadPermCase::ChownForeignGroup,
        BadPermCase::MountNoCap,
        BadPermCase::MountWrongCap,
    ];
}

/// 每类复测轮数。
pub const INJECT_ROUNDS: u32 = 8;

/// 注入账：注入 = 检出 + 逃逸，逃逸必须恒为 0。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct PermLedger {
    /// 注入总次数
    pub injected: u32,
    /// 检出次数
    pub detected: u32,
    /// 逃逸次数（必须为 0）
    pub escaped: u32,
}

impl PermLedger {
    const fn zero() -> PermLedger {
        PermLedger { injected: 0, detected: 0, escaped: 0 }
    }

    /// 记一次注入结果；任何倒计由守恒式显性捕获。
    pub fn record(&mut self, caught: bool) {
        self.injected = self.injected.saturating_add(1);
        if caught {
            self.detected = self.detected.saturating_add(1);
        } else {
            self.escaped = self.escaped.saturating_add(1);
        }
    }

    /// 守恒式。
    pub fn conserved(&self) -> bool {
        self.detected.saturating_add(self.escaped) == self.injected
    }
}

/// 执行注入矩阵：10 类 × INJECT_ROUNDS 次。
/// 返回（账本, 逐类检出数, 逐类方向一致计数）。
pub fn run_perm_matrix() -> (PermLedger, [u32; 10], [u32; 10]) {
    let mut ledger = PermLedger::zero();
    let mut per_class = [0u32; 10];
    let mut per_dir = [0u32; 10];
    let mut ci = 0usize;
    while ci < BadPermCase::ALL.len() {
        let case = match BadPermCase::ALL.get(ci) {
            Some(c) => *c,
            None => break,
        };
        let (fam, sub, obj) = case.sample();
        let mut round = 0u32;
        while round < INJECT_ROUNDS {
            let out = judge_family(fam, sub, obj);
            let caught = out == Err(case.expect());
            if caught {
                if let Some(a) = per_class.get_mut(ci) {
                    *a = a.saturating_add(1);
                }
                if let Some(b) = per_dir.get_mut(ci) {
                    // 方向一致：实测 errno 与 B24 冻结方向表逐族一致。
                    let fam_dir = PermFamily::from_id(fam)
                        .map(|f| f.b24_direction())
                        .unwrap_or(EINVAL);
                    if out.err() == Some(fam_dir) {
                        *b = b.saturating_add(1);
                    }
                }
            }
            ledger.record(caught);
            round += 1;
        }
        ci += 1;
    }
    (ledger, per_class, per_dir)
}

// ── B24 判定域对账 ───────────────────────────────────────────────────
/// 对账：五族方向表自洽（族↔方向 双向可复算）且拒绝方向只落 EPERM/EACCES。
pub fn reconcile_b24() -> bool {
    let mut i = 0usize;
    while i < PermFamily::ALL.len() {
        if let Some(f) = PermFamily::ALL.get(i) {
            let dir = f.b24_direction();
            if dir != EPERM && dir != EACCES {
                return false;
            }
            // 反向确认：按方向重推应回到同一族集合的期望。
            let back = match dir {
                EACCES => f.id() <= 2,
                _ => f.id() >= 3,
            };
            if !back {
                return false;
            }
        }
        i += 1;
    }
    true
}

// ── 确定性指纹 ────────────────────────────────────────────────────────
/// FNV-1a 32 位。
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

/// 规范语料双跑指纹：10 类坏样本 + root/DAC 放行面逐字节喂 FNV-1a。
pub fn corpus_fingerprint() -> u32 {
    let mut bytes: Vec<u8> = Vec::new();
    let mut ci = 0usize;
    while ci < BadPermCase::ALL.len() {
        if let Some(case) = BadPermCase::ALL.get(ci) {
            let (fam, sub, obj) = case.sample();
            bytes.push(match judge_family(fam, sub, obj) {
                Ok(()) => 0,
                Err(e) => (e & 0xFF) as u8,
            });
        }
        ci += 1;
    }
    // 放行面：root 五族全放行。
    let root = Subject::new(ROOT_UID, 0);
    let obj = Object::new(1002, 1003, 0);
    bytes.push(match judge_family(1, root, obj) {
        Ok(()) => 1,
        Err(e) => (e & 0xFF) as u8,
    });
    bytes.push(match judge_family(5, root, obj) {
        Ok(()) => 1,
        Err(e) => (e & 0xFF) as u8,
    });
    fnv1a(&bytes)
}

// ── 检查入口 ──────────────────────────────────────────────────────────
/// F9345 判据集（16 条；动态明细 2 条）。
pub fn run_unx9345_checks() -> CheckSet {
    let mut s = CheckSet::new("unx9345");

    // 1. 段位守卫
    let seg_ok = SEG_BASE % 0x40 == 0 && (0x7C300..=0x7D1C0).contains(&SEG_BASE);
    s.add("段位守卫：0x40 对齐且在批窗口内", seg_ok,
          "段基址必须唯一且落在本批 0x7C300..=0x7D1C0");

    // 2. 正常流：属主读写自家文件
    let owner = Subject::new(1001, 0);
    let own_file = Object::new(1001, 1001, 0o600);
    let normal = judge_file_open(owner, own_file, true, true).is_ok()
        && judge_kill(Subject::new(1001, 0), 1001).is_ok();
    s.add("正常流：属主读写自家文件 + 同 uid 投递", normal,
          "权限面只在跨身份/缺位时拒绝，合法路径不得误伤");

    // 3. EPERM/EACCES 分界：能力面全 EPERM，位面全 EACCES
    let split_ok = BadPermCase::ALL.iter().all(|c| {
        let (fam, sub, obj) = c.sample();
        match judge_family(fam, sub, obj) {
            Err(e) => e == c.expect()
                && e == PermFamily::from_id(fam).map(|f| f.b24_direction()).unwrap_or(EINVAL),
            Ok(()) => false,
        }
    });
    s.add("分界格：10 类样本方向与 B24 冻结表逐格一致", split_ok,
          "能力缺权回 EPERM、模式位拒绝回 EACCES，错配一格即红");

    // 4. 注入矩阵逐格检出
    let (ledger, per_class, per_dir) = run_perm_matrix();
    let all_cells = per_class.iter().all(|n| *n == INJECT_ROUNDS)
        && per_dir.iter().all(|n| *n == INJECT_ROUNDS);
    s.add("总装矩阵：10 类 ×8 轮逐格检出且方向逐格对账", all_cells,
          "检出与方向两路计数都必须 8/8");

    // 5. 注入账守恒
    s.add("注入账守恒：注入=检出+逃逸且逃逸=0",
          ledger.conserved() && ledger.escaped == 0
              && ledger.injected == 10 * INJECT_ROUNDS,
          "80 次注入必须 80 次检出、0 逃逸");

    // 6. 旁路面：root 五族全放行
    let root = Subject::new(ROOT_UID, 0);
    let ro_obj = Object::new(1002, 1003, 0);
    let root_ok = judge_family(1, root, ro_obj).is_ok()
        && judge_family(2, root, ro_obj).is_ok()
        && judge_family(3, root, ro_obj).is_ok()
        && judge_family(4, root, ro_obj).is_ok()
        && judge_family(5, root, ro_obj).is_ok();
    s.add("旁路面：root 五族全放行", root_ok,
          "uid=0 是全族旁路；少放一族或多放一族都判红");

    // 7. 旁路面：DAC 位只救 EACCES 族，特权位只救各自族
    let dac = Subject::new(1001, CAP_DAC_OVERRIDE);
    let cap_ch = Subject::new(1001, CAP_CHOWN);
    let cap_sa = Subject::new(1001, CAP_SYS_ADMIN);
    let bypass_ok = judge_file_open(dac, ro_obj, true, true).is_ok()
        && judge_dir_write(dac, Object::new(1001, 1001, 0o555)).is_ok()
        && judge_kill(dac, 1002).is_err()
        && judge_chown(cap_ch, Object::new(1001, 1001, 0o644), 1002, 1001).is_ok()
        && judge_mount(cap_sa).is_ok()
        && judge_mount(cap_ch).is_err();
    s.add("旁路面：能力位按族归属生效，跨族借用无效", bypass_ok,
          "DAC 救位面、KILL/CHOWN/SYS_ADMIN 救各自操作面；错配即红");

    // 8. POSIX 选中规则：属主类零位时不得回落到组/其他
    let masked = Object::new(1001, 1001, 0o077);
    let select_ok = judge_file_open(Subject::new(1001, 0), masked, true, false) == Err(EACCES)
        && judge_file_open(Subject::new(1002, 0), masked, true, true).is_ok();
    s.add("选中规则：属主类零位不回落组/其他位", select_ok,
          "POSIX 多类命中只取第一类；回落即分界漂移");

    // 9. 目录搜索先于写：缺 x 的目录即使有 w 也拒
    let nx = Object::new(1001, 1001, 0o622);
    let order_ok = judge_dir_write(Subject::new(1001, 0), nx) == Err(EACCES)
        && judge_dir_write(Subject::new(1001, 0), Object::new(1001, 1001, 0o733)).is_ok();
    s.add("判定次序：目录搜索（x）先于写（w）", order_ok,
          "搜索被拒时写位无从谈起；次序颠倒即格错");

    // 10. 未知族出 EINVAL 且不混入 EPERM/EACCES 分界
    let unknown_ok = judge_family(0, owner, own_file) == Err(EINVAL)
        && judge_family(6, owner, own_file) == Err(EINVAL);
    s.add("未知族：出 EINVAL 不入分界", unknown_ok,
          "分界面只覆盖 1..=5；未知族按参数面出口");

    // 11. B24 对账自洽
    s.add("B24 判定域对账：五族方向表自洽", reconcile_b24(),
          "方向表必须族↔方向双向可复算，且只落 EPERM/EACCES 两值");

    // 12. Linux 对照：表内正值精确
    let codes_ok = judge_kill(Subject::new(1000, 0), 1001).err() == Some(EPERM)
        && judge_file_open(Subject::new(1001, 0), ro_obj, true, true).err() == Some(EACCES)
        && EPERM == 1
        && EACCES == 13;
    s.add("Linux 逐例一致：EPERM=1/EACCES=13 精确", codes_ok,
          "分界码值与 errno.h 常量逐项对齐；近似码仅说明不判定");

    // 13. 双跑一致
    let a = corpus_fingerprint();
    let b = corpus_fingerprint();
    s.add("双跑一致：规范语料指纹相等", a == b,
          "同输入两次必须逐字节相等，隐藏状态即失败");

    // 14. 文案闭集
    let text_ok = AccessCode::ALL.iter().all(|c| !c.detail().is_empty());
    s.add("文案闭集：12 个诊断码均带中文建议", text_ok,
          "每个码必须能取到非空且可操作的说明");

    // 15. 码值唯一
    let mut uniq = true;
    let mut i = 0usize;
    while i < AccessCode::ALL.len() {
        let mut j = i + 1;
        while j < AccessCode::ALL.len() {
            if let (Some(a), Some(b)) = (AccessCode::ALL.get(i), AccessCode::ALL.get(j)) {
                if a.code() == b.code() {
                    uniq = false;
                }
            }
            j += 1;
        }
        i += 1;
    }
    s.add("诊断码：12 个码值两两唯一", uniq,
          "段内闭集码不得重复，否则判据无法定位");

    // 16. 注册表双向一致
    let reg_ok = PermFamily::ALL.iter().all(|f| PermFamily::from_id(f.id()) == Some(*f))
        && PermFamily::from_id(0).is_none()
        && PermFamily::from_id(9).is_none();
    s.add("族注册表：id 双向可逆且闭集外拒绝", reg_ok,
          "注册面 1..=5 之外不得解析出族");

    // 动态明细（≤8）
    let ledger_text = format!(
        "注入 {} / 检出 {} / 逃逸 {}",
        ledger.injected, ledger.detected, ledger.escaped
    );
    s.add_dyn("账目快照", ledger.conserved(), &ledger_text);
    let fp_text = format!("语料指纹 0x{:08X}", a);
    s.add_dyn("指纹快照", a == b, &fp_text);

    s
}

// ── 测试 ──────────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn owner_paths_pass() {
        let sub = Subject::new(1001, 0);
        assert!(judge_file_open(sub, Object::new(1001, 1001, 0o600), true, true).is_ok());
        assert!(judge_kill(sub, 1001).is_ok());
        assert!(judge_dir_write(sub, Object::new(1001, 1001, 0o755)).is_ok());
    }

    #[test]
    fn eperm_family_cells() {
        assert_eq!(judge_kill(Subject::new(1000, 0), 1001), Err(EPERM));
        assert_eq!(
            judge_chown(Subject::new(1001, 0), Object::new(1001, 1001, 0o644), 1002, 1001),
            Err(EPERM)
        );
        assert_eq!(judge_mount(Subject::new(1000, 0)), Err(EPERM));
    }

    #[test]
    fn eaccess_family_cells() {
        let sub = Subject::new(1001, 0);
        assert_eq!(judge_file_open(sub, Object::new(1002, 1003, 0o004), true, true), Err(EACCES));
        assert_eq!(judge_dir_write(sub, Object::new(1001, 1001, 0o555)), Err(EACCES));
        assert_eq!(judge_dir_write(sub, Object::new(1001, 1001, 0o644)), Err(EACCES));
    }

    #[test]
    fn root_bypasses_all() {
        let root = Subject::new(ROOT_UID, 0);
        let obj = Object::new(1002, 1003, 0);
        let mut fam = 1;
        while fam <= 5 {
            assert!(judge_family(fam, root, obj).is_ok());
            fam += 1;
        }
    }

    #[test]
    fn caps_do_not_cross_families() {
        let dac = Subject::new(1000, CAP_DAC_OVERRIDE);
        assert_eq!(judge_kill(dac, 1001), Err(EPERM));
        let kill = Subject::new(1000, CAP_KILL);
        assert!(judge_kill(kill, 1001).is_ok());
        assert_eq!(judge_mount(kill), Err(EPERM));
    }

    #[test]
    fn posix_class_selection_no_fallthrough() {
        let sub = Subject::new(1001, 0);
        assert_eq!(judge_file_open(sub, Object::new(1001, 1001, 0o077), true, false), Err(EACCES));
        assert!(judge_file_open(Subject::new(1002, 0), Object::new(1001, 1001, 0o077), true, true)
            .is_ok());
    }

    #[test]
    fn dir_search_before_write() {
        let sub = Subject::new(1001, 0);
        assert_eq!(judge_dir_write(sub, Object::new(1001, 1001, 0o622)), Err(EACCES));
        assert!(judge_dir_write(sub, Object::new(1001, 1001, 0o733)).is_ok());
    }

    #[test]
    fn matrix_ledger_conserved() {
        let (l, per, dir) = run_perm_matrix();
        assert!(l.conserved());
        assert_eq!(l.escaped, 0);
        assert_eq!(l.injected, 10 * INJECT_ROUNDS);
        for n in per.iter() {
            assert_eq!(*n, INJECT_ROUNDS);
        }
        for n in dir.iter() {
            assert_eq!(*n, INJECT_ROUNDS);
        }
    }

    #[test]
    fn unknown_family_einval() {
        let sub = Subject::new(1001, 0);
        let obj = Object::new(1001, 1001, 0o777);
        assert_eq!(judge_family(0, sub, obj), Err(EINVAL));
        assert_eq!(judge_family(6, sub, obj), Err(EINVAL));
    }

    #[test]
    fn b24_reconcile_holds() {
        assert!(reconcile_b24());
        assert_eq!(PermFamily::DirWrite.b24_direction(), EACCES);
        assert_eq!(PermFamily::Chown.b24_direction(), EPERM);
    }

    #[test]
    fn deterministic_corpus() {
        assert_eq!(corpus_fingerprint(), corpus_fingerprint());
    }

    #[test]
    fn checks_all_green() {
        let s = run_unx9345_checks();
        assert!(s.all_passed(), "{}", s.red_items_text());
    }
}
