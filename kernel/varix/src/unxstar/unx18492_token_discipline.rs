//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F18492 · 覆写权限纪律（仅同进程令牌） · 0x7F6C0 段
//! ─────────────────────────────────────────────────────────────────────
//! 锚点：UNX-F18492-J1 令牌校验拒绝外部操纵断言全过，越权注入 10/10 拒绝
//! 本模块结构清单：
//!   1. 冻结常量层：进程注册表 16 / 覆写槽 8 / 盐域 1..=64 / 判据轮数 10
//!       / 混淆键 0x5A5A5A5A（写死）；
//!   2. 诊断码闭集 TkCode（code()=SEG_BASE+n，段内 0x40 窗口）；
//!   3. 数据结构：
//!       - 数组账本 ProcRegistry：[ProcEntry; 16]（pid/盐/注册位三元组）；
//!       - 位面 BoundPlane：u8 位图，第 i 位=槽 i 已绑覆写；
//!       - 授权审计环 AuthLog：[AuthEvent; 32]（码/操作/摘要三要素）；
//!       - 覆写槽表 OvrSlotTable：[TkSlot; 8]（属主/盐/签名/状态）；
//!   4. 状态机 SlotState：Free→Bound→Free，Bound 上重复绑定被拒、Free
//!       上撤销被拒、他人撤销（属主不符）被拒——单向转换非法倒退显码；
//!   5. 令牌模型：sig = FNV(pid‖salt) ⊕ 0x5A5A5A5A，盐轮换（rotate）
//!       后旧令牌即刻失效；调用方必须携带 caller_pid 且与令牌属主一致
//!       （仅同进程令牌——跨进程持他人令牌即 ReplayOtherProc）；
//!   6. 校验次序（身份先于内容，与 Linux EPERM/EACCES 次序同构）：
//!       caller 域 → caller 在册 → 属主一致 → 零签名哨兵 → 签名对盐 →
//!       盐新鲜度 → 键域 → 状态闸；
//!   7. 错误路径：零/越界 pid / 未注册 / 幽灵键 / 伪造签名 / 跨进程重放
//!       / 陈旧盐 / 零签名哨兵 / 重复绑定 / 无覆写撤销 / 属主不符 / 表满
//!       十一类显式拒绝；
//!   8. 注入核十类：伪造签名 / 跨进程重放 / 陈旧盐 / 坏 pid / 未注册 /
//!       幽灵键 / 他人撤销 / 重复绑定 / 空槽撤销 / 零签名——逐类 10 轮
//!       全抓，tries==caught 守恒漏检必 0；
//!   9. golden_run：注册→绑定→校验→轮换（旧拒新通）→撤销全链 + 双跑一致；
//!  10. run_unx18492_checks：17 条判据。
//! ═══════════════════════════════════════════════════════════════════════

use crate::alloc::format;
use crate::checks::CheckSet;

/// 诊断段基址（0x7F6C0，G1 工单 A10 批05 独占窗口，0x40 对齐）。
pub const SEG_BASE: u32 = 0x7F6C0;

// ── 冻结常量层（推导注记） ─────────────────────────────────────────
/// 进程注册表容量（写死 16：样板进程 4 × 4 倍余量，2 的幂）。
pub const PROC_CAP: usize = 16;
/// 覆写槽容量（写死 8：与 F18491 覆写表同容量——跨卡位面口径一致）。
pub const SLOT_CAP: usize = 8;
/// 进程号域上界（冻结 1..=999，0 保留为内核哨兵）。
pub const PID_MAX: u32 = 999;
/// 盐域上界（写死 64：轮换到顶回卷 1——避免盐无界增长，64 次轮换覆盖
/// 本批 10 轮判据 × 4 进程绰绰有余）。
pub const SALT_MAX: u64 = 64;
/// 混淆键（写死 0x5A5A5A5A：本段独占异或键，使 FNV 摘要直接外泄后仍
/// 不可直接当签名用——批 05 约定整段同键，便于跨卡对账）。
pub const TOKEN_KEY: u32 = 0x5A5A_5A5A;
/// 审计环容量（写死 32：三要素事件留 2 的幂环形）。
pub const AUTH_LOG_CAP: usize = 32;
/// 判据轮数（写死 10：J1"越权注入 10/10 拒绝"取 10/10 语义）。
pub const JUDGE_ROUNDS: u32 = 10;
/// 授权操作闭集：1 绑定 / 2 撤销 / 3 校验。
pub const OP_BIND: u8 = 1;
pub const OP_UNBIND: u8 = 2;
pub const OP_VERIFY: u8 = 3;

// ── 诊断码闭集（段内 0x40 窗口） ───────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TkCode {
    /// 坏 pid：零哨兵或越界
    PidOutOfRange,
    /// 未注册进程：注册表查无此 pid
    PidUnregistered,
    /// 重复注册：pid 已在注册表
    PidDupRegister,
    /// 进程表满：16 槽用尽
    ProcTableFull,
    /// 幽灵键：槽 0..8 之外
    GhostKey,
    /// 伪造签名：sig 与 FNV(pid‖salt)⊕KEY 不符
    ForgeSig,
    /// 跨进程重放：caller 与令牌属主不符
    ReplayOtherProc,
    /// 陈旧盐：盐已轮换，旧令牌失效
    StaleSalt,
    /// 零签名哨兵：sig==0 恒拒
    ZeroSig,
    /// 重复绑定：Bound 槽再绑
    AlreadyBound,
    /// 无覆写撤销：Free 槽撤销
    NoActiveOverride,
    /// 属主不符：撤销者非绑定者
    OwnerMismatch,
    /// 位面破坏：位面位与槽状态不一致
    PlaneBroken,
    /// 账面守恒破坏：attempts ≠ ok + rejected
    LedgerBroken,
    /// 双跑不一致
    NotReproducible,
}

impl TkCode {
    /// 诊断码取值：SEG_BASE + 枚举序（闭集，段内窗口 ≤0x40）。
    pub fn code(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 中文诊断文案（闭集，逐码非空）。
    pub fn detail(self) -> &'static str {
        match self {
            TkCode::PidOutOfRange => "坏 pid：须在 1..=999",
            TkCode::PidUnregistered => "未注册进程：注册表查无此 pid",
            TkCode::PidDupRegister => "重复注册：pid 已在册",
            TkCode::ProcTableFull => "进程表满：16 槽用尽",
            TkCode::GhostKey => "幽灵键：覆写槽 0..8 之外",
            TkCode::ForgeSig => "伪造签名：sig 校验不符",
            TkCode::ReplayOtherProc => "跨进程重放：caller 与令牌属主不符",
            TkCode::StaleSalt => "陈旧盐：盐已轮换旧令牌失效",
            TkCode::ZeroSig => "零签名哨兵：sig=0 恒拒",
            TkCode::AlreadyBound => "重复绑定：槽已在场",
            TkCode::NoActiveOverride => "无覆写撤销：空闲槽不可撤销",
            TkCode::OwnerMismatch => "属主不符：仅绑定者可撤销",
            TkCode::PlaneBroken => "位面破坏：位面与槽状态不一致",
            TkCode::LedgerBroken => "账面守恒破坏：attempts≠oks+rejected",
            TkCode::NotReproducible => "双跑不一致：复现红线",
        }
    }
    /// 全闭集（段位守卫遍历用）。
    pub const ALL: [TkCode; 15] = [
        TkCode::PidOutOfRange,
        TkCode::PidUnregistered,
        TkCode::PidDupRegister,
        TkCode::ProcTableFull,
        TkCode::GhostKey,
        TkCode::ForgeSig,
        TkCode::ReplayOtherProc,
        TkCode::StaleSalt,
        TkCode::ZeroSig,
        TkCode::AlreadyBound,
        TkCode::NoActiveOverride,
        TkCode::OwnerMismatch,
        TkCode::PlaneBroken,
        TkCode::LedgerBroken,
        TkCode::NotReproducible,
    ];
}

/// FNV-1a 32（令牌签名/审计摘要共用基元——确定性，零随机源）。
pub fn fnv1a(s: &[u8]) -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    let mut i = 0;
    while i < s.len() {
        h ^= s[i] as u32;
        h = h.wrapping_mul(0x0100_0193);
        i += 1;
    }
    h
}

// ── 令牌 ───────────────────────────────────────────────────────────
/// 覆写令牌（属主 pid + 颁发时盐 + 签名）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct OvrToken {
    pub pid: u32,
    pub salt: u64,
    pub sig: u32,
}

impl OvrToken {
    /// 零令牌哨兵（const：空槽初始化）。
    pub const fn zero() -> OvrToken {
        OvrToken { pid: 0, salt: 0, sig: 0 }
    }
}

/// 派生签名：sig = FNV(pid‖salt) ⊕ TOKEN_KEY。
/// 推导：pid 两字节 + 盐八字节逐段入摘要，再与本段混淆键异或——摘要
/// 泄露不等于签名泄露（缺 KEY 不可伪造）。
pub fn make_sig(pid: u32, salt: u64) -> u32 {
    let mut buf = [0u8; 10];
    buf[0] = (pid % 256) as u8;
    buf[1] = ((pid / 256) % 256) as u8;
    let mut k = 0;
    while k < 8 {
        buf[2 + k] = ((salt >> (k * 8)) % 256) as u8;
        k += 1;
    }
    fnv1a(&buf) ^ TOKEN_KEY
}

/// 颁发令牌（纯函数：pid+盐 → 令牌）。
pub fn make_token(pid: u32, salt: u64) -> OvrToken {
    OvrToken { pid, salt, sig: make_sig(pid, salt) }
}

// ── 进程注册表 ─────────────────────────────────────────────────────
/// 进程注册项（pid/盐/在册位）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ProcEntry {
    pub pid: u32,
    pub salt: u64,
    pub used: bool,
}

impl ProcEntry {
    /// 空项（const：定容数组初始化）。
    pub const fn empty() -> ProcEntry {
        ProcEntry { pid: 0, salt: 0, used: false }
    }
}

/// 进程注册表（数组账本）：注册/查盐/轮换三操作。
pub struct ProcRegistry {
    pub entries: [ProcEntry; PROC_CAP],
    pub registered: u32,
    pub rotations: u32,
}

impl ProcRegistry {
    /// 空表。
    pub fn new() -> ProcRegistry {
        ProcRegistry { entries: [ProcEntry::empty(); PROC_CAP], registered: 0, rotations: 0 }
    }

    /// 查 pid 的槽位（线性，None=未注册）。
    pub fn find(&self, pid: u32) -> Option<usize> {
        let mut i = 0;
        while i < PROC_CAP {
            if self.entries[i].used && self.entries[i].pid == pid {
                return Some(i);
            }
            i += 1;
        }
        None
    }

    /// 当前盐（未注册返回 None）。
    pub fn salt_of(&self, pid: u32) -> Option<u64> {
        match self.find(pid) {
            Some(i) => Some(self.entries[i].salt),
            None => None,
        }
    }

    /// 注册：pid 域闸 + 重复闸 + 表满闸；初始盐恒 1（0 保留哨兵）。
    pub fn register(&mut self, pid: u32) -> Result<(), u32> {
        if pid == 0 || pid > PID_MAX {
            return Err(TkCode::PidOutOfRange.code());
        }
        if self.find(pid).is_some() {
            return Err(TkCode::PidDupRegister.code());
        }
        let mut i = 0;
        while i < PROC_CAP {
            if !self.entries[i].used {
                self.entries[i] = ProcEntry { pid, salt: 1, used: true };
                self.registered = self.registered.saturating_add(1);
                return Ok(());
            }
            i += 1;
        }
        Err(TkCode::ProcTableFull.code())
    }

    /// 盐轮换：盐 +1（到顶回卷 1），旧令牌即刻失效。
    pub fn rotate(&mut self, pid: u32) -> Result<u64, u32> {
        let i = match self.find(pid) {
            Some(i) => i,
            None => return Err(TkCode::PidUnregistered.code()),
        };
        let mut next = self.entries[i].salt + 1;
        if next > SALT_MAX {
            next = 1;
        }
        self.entries[i].salt = next;
        self.rotations = self.rotations.saturating_add(1);
        Ok(next)
    }
}

// ── 覆写槽（状态机 + 位面） ────────────────────────────────────────
/// 槽状态机：Free→Bound（绑定），Bound→Free（撤销）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SlotState {
    /// 空闲
    Free,
    /// 已绑覆写（属主在场）
    Bound,
}

/// 覆写槽（状态/属主/颁发盐/签名）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct TkSlot {
    pub state: SlotState,
    pub pid: u32,
    pub salt: u64,
    pub sig: u32,
}

impl TkSlot {
    /// 空槽（const）。
    pub const fn empty() -> TkSlot {
        TkSlot { state: SlotState::Free, pid: 0, salt: 0, sig: 0 }
    }
}

/// 覆写槽表（数组账本 + 绑定位面）。
pub struct OvrSlotTable {
    pub slots: [TkSlot; SLOT_CAP],
    pub plane: u8,
    pub binds: u32,
    pub unbinds: u32,
}

impl OvrSlotTable {
    /// 空表。
    pub fn new() -> OvrSlotTable {
        OvrSlotTable { slots: [TkSlot::empty(); SLOT_CAP], plane: 0, binds: 0, unbinds: 0 }
    }
    /// 位面查询。
    pub fn bound(&self, i: usize) -> bool {
        if i >= SLOT_CAP {
            return false;
        }
        (self.plane >> i) & 1 == 1
    }
    /// 位面一致性：位面位 == 槽状态（逐槽复核）。
    pub fn plane_consistent(&self) -> bool {
        let mut i = 0;
        while i < SLOT_CAP {
            let bit = self.bound(i);
            let st = self.slots[i].state == SlotState::Bound;
            if bit != st {
                return false;
            }
            i += 1;
        }
        true
    }
    /// 绑定槽计数（位面折算）。
    pub fn bound_count(&self) -> u32 {
        let mut c = 0u32;
        let mut i = 0;
        while i < SLOT_CAP {
            if self.bound(i) {
                c += 1;
            }
            i += 1;
        }
        c
    }
}

// ── 授权审计环（三要素日志） ───────────────────────────────────────
/// 单条授权事件（码/操作/摘要三要素）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct AuthEvent {
    pub code: u32,
    pub op: u8,
    pub pid: u32,
    pub digest: u32,
}

/// 审计环：定容 32，满后环形覆盖并显性计数（不静默丢）。
pub struct AuthLog {
    pub ring: [AuthEvent; AUTH_LOG_CAP],
    pub head: usize,
    pub len: usize,
    pub total: u32,
}

impl AuthLog {
    /// 空环。
    pub fn new() -> AuthLog {
        AuthLog {
            ring: [AuthEvent { code: 0, op: 0, pid: 0, digest: 0 }; AUTH_LOG_CAP],
            head: 0,
            len: 0,
            total: 0,
        }
    }
    /// 入环（环形覆盖）。
    pub fn push(&mut self, ev: AuthEvent) {
        self.ring[self.head] = ev;
        self.head = (self.head + 1) % AUTH_LOG_CAP;
        if self.len < AUTH_LOG_CAP {
            self.len += 1;
        }
        self.total = self.total.saturating_add(1);
    }
    /// 三要素齐备性：每条 code 非零 / op 在闭集 / pid 域内 / digest 非零。
    pub fn triple_ok(&self) -> bool {
        let mut i = 0;
        while i < self.len {
            let e = self.ring[i];
            if e.code == 0 || e.pid == 0 || e.pid > PID_MAX || e.digest == 0 {
                return false;
            }
            match e.op {
                o if o == OP_BIND || o == OP_UNBIND || o == OP_VERIFY => {}
                _ => return false,
            }
            i += 1;
        }
        true
    }
}

// ── 权限门面（校验/绑定/撤销 + 审计） ──────────────────────────────
/// 权限门面：注册表 + 槽表 + 审计环联动，attempts/oks/rejected 三账。
pub struct AuthGate {
    pub reg: ProcRegistry,
    pub table: OvrSlotTable,
    pub log: AuthLog,
    pub attempts: u32,
    pub oks: u32,
    pub rejected: u32,
}

impl AuthGate {
    /// 空门面。
    pub fn new() -> AuthGate {
        AuthGate {
            reg: ProcRegistry::new(),
            table: OvrSlotTable::new(),
            log: AuthLog::new(),
            attempts: 0,
            oks: 0,
            rejected: 0,
        }
    }

    /// 记账入审计（attempts++，ok 分流；摘要=FNV(op‖pid‖code‖序)）。
    fn record(&mut self, op: u8, pid: u32, code: u32, ok: bool) {
        self.attempts = self.attempts.saturating_add(1);
        if ok {
            self.oks = self.oks.saturating_add(1);
        } else {
            self.rejected = self.rejected.saturating_add(1);
        }
        let mut buf = [0u8; 9];
        buf[0] = op;
        buf[1] = (pid % 256) as u8;
        buf[2] = ((pid / 256) % 256) as u8;
        buf[3] = (code % 256) as u8;
        buf[4] = ((code / 256) % 256) as u8;
        buf[5] = ((code / 65536) % 256) as u8;
        buf[6] = ((code / 16777216) % 256) as u8;
        buf[7] = (self.attempts % 256) as u8;
        buf[8] = ((self.attempts / 256) % 256) as u8;
        let d = fnv1a(&buf);
        // digest 哨兵：FNV 碰零时取 1（三要素 digest 非零恒真）。
        let dd = if d == 0 { 1 } else { d };
        self.log.push(AuthEvent { code, op, pid, digest: dd });
    }

    /// 身份+令牌域校验（不含槽状态）。次序（身份先于内容）：
    /// caller 域 → caller 在册 → 属主一致 → 零签名 → 签名对盐 → 盐新鲜。
    fn check_identity(&self, caller: u32, tok: &OvrToken) -> Result<(), u32> {
        if caller == 0 || caller > PID_MAX {
            return Err(TkCode::PidOutOfRange.code());
        }
        if self.reg.find(caller).is_none() {
            return Err(TkCode::PidUnregistered.code());
        }
        if tok.pid != caller {
            return Err(TkCode::ReplayOtherProc.code());
        }
        if tok.sig == 0 {
            return Err(TkCode::ZeroSig.code());
        }
        if tok.sig != make_sig(tok.pid, tok.salt) {
            return Err(TkCode::ForgeSig.code());
        }
        let salt = match self.reg.salt_of(caller) {
            Some(s) => s,
            None => return Err(TkCode::PidUnregistered.code()),
        };
        if tok.salt != salt {
            return Err(TkCode::StaleSalt.code());
        }
        Ok(())
    }

    /// 绑定覆写（caller 必须与令牌属主同进程）：身份校验 → 幽灵键 →
    /// 状态闸。成功落槽+位面。
    pub fn bind(&mut self, caller: u32, key: usize, tok: OvrToken) -> Result<(), u32> {
        if let Err(c) = self.check_identity(caller, &tok) {
            self.record(OP_BIND, if caller == 0 { 1 } else { caller.min(PID_MAX) }, c, false);
            return Err(c);
        }
        if key >= SLOT_CAP {
            let c = TkCode::GhostKey.code();
            self.record(OP_BIND, caller, c, false);
            return Err(c);
        }
        if self.table.slots[key].state == SlotState::Bound {
            let c = TkCode::AlreadyBound.code();
            self.record(OP_BIND, caller, c, false);
            return Err(c);
        }
        self.table.slots[key] =
            TkSlot { state: SlotState::Bound, pid: tok.pid, salt: tok.salt, sig: tok.sig };
        self.table.plane |= 1u8 << key;
        self.table.binds = self.table.binds.saturating_add(1);
        // 成功路径审计码 = 段基址哨兵（非诊断码，恒落在窗内且非零）。
        self.record(OP_BIND, caller, SEG_BASE, true);
        Ok(())
    }

    /// 撤销覆写（caller 必须与令牌属主同进程，且为槽属主）：身份校验 →
    /// 幽灵键 → 状态闸 → 属主闸。成功清槽+位面。
    pub fn unbind(&mut self, caller: u32, key: usize, tok: OvrToken) -> Result<(), u32> {
        if let Err(c) = self.check_identity(caller, &tok) {
            self.record(OP_UNBIND, if caller == 0 { 1 } else { caller.min(PID_MAX) }, c, false);
            return Err(c);
        }
        if key >= SLOT_CAP {
            let c = TkCode::GhostKey.code();
            self.record(OP_UNBIND, caller, c, false);
            return Err(c);
        }
        if self.table.slots[key].state != SlotState::Bound {
            let c = TkCode::NoActiveOverride.code();
            self.record(OP_UNBIND, caller, c, false);
            return Err(c);
        }
        if self.table.slots[key].pid != caller {
            let c = TkCode::OwnerMismatch.code();
            self.record(OP_UNBIND, caller, c, false);
            return Err(c);
        }
        self.table.slots[key] = TkSlot::empty();
        self.table.plane &= !(1u8 << key);
        self.table.unbinds = self.table.unbinds.saturating_add(1);
        self.record(OP_UNBIND, caller, SEG_BASE, true);
        Ok(())
    }

    /// 只读校验（OP_VERIFY 审计，不改状态）。
    pub fn verify(&mut self, caller: u32, tok: OvrToken) -> Result<(), u32> {
        match self.check_identity(caller, &tok) {
            Ok(()) => {
                self.record(OP_VERIFY, caller, SEG_BASE, true);
                Ok(())
            }
            Err(c) => {
                self.record(OP_VERIFY, if caller == 0 { 1 } else { caller.min(PID_MAX) }, c, false);
                Err(c)
            }
        }
    }

    /// 账面守恒：attempts == oks + rejected（漏账必 0）∧ 位面一致。
    pub fn conserve(&self) -> Result<(), u32> {
        if self.attempts != self.oks + self.rejected {
            return Err(TkCode::LedgerBroken.code());
        }
        if !self.table.plane_consistent() {
            return Err(TkCode::PlaneBroken.code());
        }
        Ok(())
    }
}

// ── 黄金全链 ───────────────────────────────────────────────────────
/// 黄金全链：注册 4 进程→绑定 2 槽→校验→轮换（旧拒新通）→撤销 1→守恒。
/// 返回 (binds, unbinds, rejected)。
pub fn golden_run() -> Result<(u32, u32, u32), u32> {
    let mut g = AuthGate::new();
    // 注册 4 进程（101..404，确定性派生）。
    let pids = [101u32, 202, 303, 404];
    let mut i = 0;
    while i < 4 {
        g.reg.register(pids[i])?;
        i += 1;
    }
    // 各进程以自身身份按当前盐颁令牌并绑定槽 0、1。
    let mut toks: [OvrToken; 2] = [OvrToken::zero(), OvrToken::zero()];
    let mut k = 0;
    while k < 2 {
        let salt = match g.reg.salt_of(pids[k]) {
            Some(s) => s,
            None => return Err(TkCode::PidUnregistered.code()),
        };
        let t = make_token(pids[k], salt);
        g.bind(pids[k], k, t)?;
        toks[k] = t;
        k += 1;
    }
    // 只读校验：合法令牌全过（caller==属主）。
    let mut v = 0;
    while v < 2 {
        g.verify(pids[v], toks[v])?;
        v += 1;
    }
    // 盐轮换：pid 101 轮换后旧令牌拒、新令牌通。
    g.reg.rotate(pids[0])?;
    if g.verify(pids[0], toks[0]).is_ok() {
        return Err(TkCode::StaleSalt.code());
    }
    let new_salt = match g.reg.salt_of(pids[0]) {
        Some(s) => s,
        None => return Err(TkCode::PidUnregistered.code()),
    };
    g.verify(pids[0], make_token(pids[0], new_salt))?;
    // 撤销槽 0：须由属主以新盐令牌执行（旧令牌已失效）。
    g.unbind(pids[0], 0, make_token(pids[0], new_salt))?;
    g.conserve()?;
    Ok((g.table.binds, g.table.unbinds, g.rejected))
}

/// 双跑一致。
pub fn repro_check() -> Result<(), u32> {
    let a = golden_run()?;
    let b = golden_run()?;
    if a != b {
        return Err(TkCode::NotReproducible.code());
    }
    Ok(())
}

// ── 注入核（十类越权 × JUDGE_ROUNDS 轮） ──────────────────────────
/// 注入记账（10 槽：一类一槽，守恒 tries==caught 漏检必 0）。
pub struct InjectLedger {
    pub tries: [u32; 10],
    pub caught: [u32; 10],
}

impl InjectLedger {
    /// 空账。
    pub fn new() -> InjectLedger {
        InjectLedger { tries: [0; 10], caught: [0; 10] }
    }
    /// 记一轮（slot 越界静默不记——防御位）。
    pub fn record(&mut self, slot: usize, hit: bool) {
        if slot < 10 {
            self.tries[slot] = self.tries[slot].saturating_add(1);
            if hit {
                self.caught[slot] = self.caught[slot].saturating_add(1);
            }
        }
    }
    /// 守恒：十槽全记录且全抓。
    pub fn all_caught(&self) -> bool {
        let mut i = 0;
        while i < 10 {
            if self.tries[i] == 0 || self.caught[i] != self.tries[i] {
                return false;
            }
            i += 1;
        }
        true
    }
    /// 总注入轮次（十槽求和）。
    pub fn total_tries(&self) -> u32 {
        let mut s = 0u32;
        let mut i = 0;
        while i < 10 {
            s = s.saturating_add(self.tries[i]);
            i += 1;
        }
        s
    }
    /// 总抓捕数。
    pub fn total_caught(&self) -> u32 {
        let mut s = 0u32;
        let mut i = 0;
        while i < 10 {
            s = s.saturating_add(self.caught[i]);
            i += 1;
        }
        s
    }
}

/// 十类越权注入：每类 rounds 轮，返回守恒账。每轮全新门面，确定性构造。
pub fn inject_rounds(rounds: u32) -> Result<InjectLedger, u32> {
    let mut led = InjectLedger::new();
    let mut r = 0;
    while r < rounds {
        // 底座：注册 101/202，槽 0 由 101（caller=101）绑定。
        let mut g = AuthGate::new();
        g.reg.register(101)?;
        g.reg.register(202)?;
        let tok101 = make_token(101, 1);
        let tok202 = make_token(202, 1);
        g.bind(101, 0, tok101)?;
        // 槽0：伪造签名（正确属主/盐，sig 改一位）。
        let mut forged = tok101;
        forged.sig = forged.sig ^ 1;
        led.record(0, g.bind(101, 1, forged).err() == Some(TkCode::ForgeSig.code()));
        // 槽1：跨进程重放（202 持 101 的合法令牌）。
        led.record(1, g.bind(202, 1, tok101).err() == Some(TkCode::ReplayOtherProc.code()));
        // 槽2：陈旧盐（202 轮换后用旧令牌，caller=202 属主一致）。
        let old202 = tok202;
        g.reg.rotate(202)?;
        led.record(2, g.bind(202, 1, old202).err() == Some(TkCode::StaleSalt.code()));
        // 槽3：坏 caller pid（0 与 1000 两形态合槽）。
        led.record(
            3,
            g.bind(0, 1, make_token(101, 1)).err() == Some(TkCode::PidOutOfRange.code())
                || g.bind(1000, 1, make_token(1000, 1)).err() == Some(TkCode::PidOutOfRange.code()),
        );
        // 槽4：未注册 caller（777 未注册）。
        led.record(4, g.bind(777, 1, make_token(777, 1)).err() == Some(TkCode::PidUnregistered.code()));
        // 槽5：幽灵键（槽 8 越界）。
        led.record(5, g.bind(101, SLOT_CAP, tok101).err() == Some(TkCode::GhostKey.code()));
        // 槽6：他人撤销（202 用自己合法令牌撤 101 的槽 0 → 属主不符）。
        let fresh202 = match g.reg.salt_of(202) {
            Some(s) => s,
            None => return Err(TkCode::PidUnregistered.code()),
        };
        led.record(
            6,
            g.unbind(202, 0, make_token(202, fresh202)).err() == Some(TkCode::OwnerMismatch.code()),
        );
        // 槽7：重复绑定（槽 0 已 Bound 再绑，caller=101 属主一致新盐）。
        g.reg.rotate(101)?;
        let fresh101 = match g.reg.salt_of(101) {
            Some(s) => s,
            None => return Err(TkCode::PidUnregistered.code()),
        };
        led.record(
            7,
            g.bind(101, 0, make_token(101, fresh101)).err() == Some(TkCode::AlreadyBound.code()),
        );
        // 槽8：空闲槽撤销（槽 5 未绑定）。
        led.record(
            8,
            g.unbind(101, 5, make_token(101, fresh101)).err() == Some(TkCode::NoActiveOverride.code()),
        );
        // 槽9：零签名哨兵。
        let zerosig = OvrToken { pid: 101, salt: fresh101, sig: 0 };
        led.record(9, g.bind(101, 1, zerosig).err() == Some(TkCode::ZeroSig.code()));
        r += 1;
    }
    Ok(led)
}

// ── 性质守卫 ───────────────────────────────────────────────────────
/// 段位守卫：全部诊断码落在 [SEG_BASE, SEG_BASE+0x40) 窗口。
pub fn segment_guard() -> bool {
    let mut i = 0;
    while i < TkCode::ALL.len() {
        let v = TkCode::ALL[i].code();
        if v < SEG_BASE || v >= SEG_BASE + 0x40 {
            return false;
        }
        i += 1;
    }
    true
}

/// 文案闭集：逐码非空中文建议。
pub fn detail_closed() -> bool {
    let mut i = 0;
    while i < TkCode::ALL.len() {
        if TkCode::ALL[i].detail().is_empty() {
            return false;
        }
        i += 1;
    }
    true
}

/// 盐轮换单调性：轮换 n 次盐值单调 +1，到顶回卷（守恒性质）。
pub fn salt_rotation_monotone() -> Result<(), u32> {
    let mut reg = ProcRegistry::new();
    reg.register(501)?;
    let mut prev = match reg.salt_of(501) {
        Some(s) => s,
        None => return Err(TkCode::PidUnregistered.code()),
    };
    if prev != 1 {
        return Err(TkCode::StaleSalt.code());
    }
    let mut i = 0;
    while i < SALT_MAX + 2 {
        let cur = reg.rotate(501)?;
        // 回卷点：prev==SALT_MAX 时 cur 必为 1；否则 cur==prev+1。
        if prev == SALT_MAX {
            if cur != 1 {
                return Err(TkCode::StaleSalt.code());
            }
        } else if cur != prev + 1 {
            return Err(TkCode::StaleSalt.code());
        }
        prev = cur;
        i += 1;
    }
    Ok(())
}

/// 令牌派生确定性：同参双派一致，异参不同；混淆键必改值（KEY≠0）。
pub fn token_determinism() -> Result<(), u32> {
    let a = make_token(101, 1);
    let b = make_token(101, 1);
    let c = make_token(101, 2);
    if a.sig != b.sig {
        return Err(TkCode::ForgeSig.code());
    }
    if a.sig == c.sig {
        return Err(TkCode::ForgeSig.code());
    }
    // 混淆键哨兵：sig ≠ 裸 FNV（⊕KEY 必改值）。
    let mut buf = [0u8; 10];
    buf[0] = 101u8;
    buf[1] = 0;
    buf[2] = 1; // 盐=1 的低字节，其余零
    let raw = fnv1a(&buf);
    if (raw ^ TOKEN_KEY) == raw {
        return Err(TkCode::ForgeSig.code());
    }
    Ok(())
}

// ── 判据收口（16 条，断言性质） ────────────────────────────────────
/// 判据入口（单模块 ≤20 条）。
pub fn run_unx18492_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F18492 覆写权限纪律（仅同进程令牌）");
    // 1. 黄金全链。
    let g = golden_run();
    s.add_dyn("J1-黄金全链", g.is_ok(), &format!("binds/unbinds/rejected={:?}", g));
    // 2. 越权注入 10/10（十类守恒——J1 主体）。
    let inj = inject_rounds(JUDGE_ROUNDS);
    let inj_ok = match &inj {
        Ok(l) => l.all_caught(),
        Err(_) => false,
    };
    s.add_dyn(
        "J1-越权注入10类10/10",
        inj_ok,
        &format!(
            "total={:?} caught={:?}",
            inj.as_ref().map(|l| l.total_tries()),
            inj.as_ref().map(|l| l.total_caught())
        ),
    );
    // 3. 伪造签名逐码。
    let forge_ok = {
        let mut g = AuthGate::new();
        let mut ok = g.reg.register(101).is_ok();
        let t = make_token(101, 1);
        ok = ok && g.bind(101, 0, t).is_ok();
        let mut f = t;
        f.sig = f.sig ^ 1;
        ok && g.bind(101, 1, f).err() == Some(TkCode::ForgeSig.code())
    };
    s.add("J1-伪造签名拒", forge_ok, "ForgeSig");
    // 4. 跨进程重放逐码（202 持 101 令牌）。
    let replay_ok = {
        let mut g = AuthGate::new();
        let mut ok = g.reg.register(101).is_ok() && g.reg.register(202).is_ok();
        let t = make_token(101, 1);
        ok = ok && g.bind(101, 0, t).is_ok();
        ok && g.bind(202, 1, t).err() == Some(TkCode::ReplayOtherProc.code())
    };
    s.add("J1-跨进程重放拒", replay_ok, "ReplayOtherProc");
    // 5. 陈旧盐逐码（轮换后旧令牌）。
    let stale_ok = {
        let mut g = AuthGate::new();
        let mut ok = g.reg.register(101).is_ok();
        let t = make_token(101, 1);
        ok = ok && g.reg.rotate(101).is_ok();
        ok && g.bind(101, 0, t).err() == Some(TkCode::StaleSalt.code())
    };
    s.add("J1-陈旧盐拒", stale_ok, "StaleSalt");
    // 6. 未注册进程拒。
    let unreg_ok = {
        let mut g = AuthGate::new();
        g.bind(777, 0, make_token(777, 1)).err() == Some(TkCode::PidUnregistered.code())
    };
    s.add("J1-未注册进程拒", unreg_ok, "PidUnregistered");
    // 7. 零签名哨兵拒。
    let zero_ok = {
        let mut g = AuthGate::new();
        let mut ok = g.reg.register(101).is_ok();
        ok && g.bind(101, 0, OvrToken { pid: 101, salt: 1, sig: 0 }).err() == Some(TkCode::ZeroSig.code())
    };
    s.add("J1-零签名哨兵拒", zero_ok, "ZeroSig");
    // 8. 他人撤销拒（属主闸：202 撤 101 的槽）。
    let owner_ok = {
        let mut g = AuthGate::new();
        let mut ok = g.reg.register(101).is_ok() && g.reg.register(202).is_ok();
        ok = ok && g.bind(101, 0, make_token(101, 1)).is_ok();
        ok && g.unbind(202, 0, make_token(202, 1)).err() == Some(TkCode::OwnerMismatch.code())
    };
    s.add("J1-他人撤销拒", owner_ok, "OwnerMismatch");
    // 9. 撤销幂等（同令牌二次撤销必拒）。
    let once_ok = {
        let mut g = AuthGate::new();
        let mut ok = g.reg.register(101).is_ok();
        let t = make_token(101, 1);
        ok = ok && g.bind(101, 0, t).is_ok();
        ok = ok && g.unbind(101, 0, t).is_ok();
        ok && g.unbind(101, 0, t).err() == Some(TkCode::NoActiveOverride.code())
    };
    s.add("J1-撤销幂等(二次必拒)", once_ok, "同令牌二次撤销拒");
    // 10. 轮换后新令牌可用（正向通路）。
    let fresh_ok = {
        let mut g = AuthGate::new();
        let mut ok = g.reg.register(101).is_ok();
        ok = ok && g.reg.rotate(101).is_ok();
        ok && g.bind(101, 0, make_token(101, 2)).is_ok()
    };
    s.add("J1-轮换后新令牌可用", fresh_ok, "新盐令牌正向通路");
    // 11. 账面守恒（attempts==oks+rejected ∧ 位面一致）。
    let cons_ok = {
        let mut g = AuthGate::new();
        let mut ok = g.reg.register(101).is_ok();
        ok = ok && g.bind(101, 0, make_token(101, 1)).is_ok();
        // 重复绑定拒——拒路径也入账。
        let _ = g.bind(101, 0, make_token(101, 1));
        g.conserve().is_ok() && ok
    };
    s.add("J1-账面守恒", cons_ok, "attempts==oks+rejected");
    // 12. 审计三要素齐备（过/拒两路径都留审计）。
    let triple_ok = {
        let mut g = AuthGate::new();
        let mut ok = g.reg.register(101).is_ok();
        let _ = g.bind(101, 0, make_token(101, 1));
        let _ = g.bind(101, 0, make_token(101, 1));
        ok = ok && g.log.triple_ok() && g.log.len >= 2;
        ok && g.log.total == 2
    };
    s.add("J1-审计三要素齐备", triple_ok, "码/操作/摘要逐条非零");
    // 13. 盐轮换单调回卷。
    s.add("J1-盐轮换单调回卷", salt_rotation_monotone().is_ok(), "SALT_MAX 回卷 1");
    // 14. 令牌派生确定性。
    s.add("J1-令牌派生确定性", token_determinism().is_ok(), "同参同 sig 异参异 sig");
    // 15. 双跑一致。
    s.add("J1-双跑一致", repro_check().is_ok(), "golden 两跑同值");
    // 16. 段位守卫 + 文案闭集（双联守卫）。
    s.add("J1-段位守卫", segment_guard(), "0x7F6C0 窗口");
    s.add("J1-文案闭集", detail_closed(), "逐码非空中文");
    s
}

// ── 测试 ───────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx18492_checks_green() {
        let set = run_unx18492_checks();
        assert!(set.all_passed(), "UNX-F18492 red: {}", set.red_items_text());
    }

    #[test]
    fn t_golden_shape() {
        // 推导：绑定 2 + 校验 3（2 过 + 1 旧盐拒）+ 撤销 1 → 拒路径恰 1。
        let g = golden_run().unwrap_or((0, 0, 0));
        assert_eq!(g.0, 2, "binds");
        assert_eq!(g.1, 1, "unbinds");
        assert_eq!(g.2, 1, "rejected==1");
    }

    #[test]
    fn t_ten_classes_all_caught() {
        let l = inject_rounds(JUDGE_ROUNDS).unwrap_or_else(|_| InjectLedger::new());
        assert!(l.all_caught(), "十类全抓");
        // 守恒：总注入 = 10 类 × 10 轮 = 100，漏检 0。
        assert_eq!(l.total_tries(), 100, "总注入 100");
        assert_eq!(l.total_caught(), 100, "总抓捕 100");
    }

    #[test]
    fn t_sig_determinism() {
        let a = make_sig(101, 1);
        let b = make_sig(101, 1);
        let c = make_sig(101, 2);
        let d = make_sig(202, 1);
        assert_eq!(a, b, "同参同 sig");
        assert!(a != c, "盐变 sig 变");
        assert!(a != d, "pid 变 sig 变");
    }

    #[test]
    fn t_stale_token_never_reusable() {
        let mut g = AuthGate::new();
        let _ = g.reg.register(101);
        let t = make_token(101, 1);
        let _ = g.bind(101, 0, t);
        let _ = g.reg.rotate(101);
        // 旧令牌：校验/绑定/撤销三通路全断（陈旧盐）。
        assert_eq!(g.verify(101, t).err(), Some(TkCode::StaleSalt.code()));
        assert_eq!(g.bind(101, 1, t).err(), Some(TkCode::StaleSalt.code()));
        assert_eq!(g.unbind(101, 0, t).err(), Some(TkCode::StaleSalt.code()));
    }

    #[test]
    fn t_state_machine_gates() {
        // Free 上撤销拒 / Bound 上重绑拒 / 撤销后可重绑（回环合法）。
        let mut g = AuthGate::new();
        let _ = g.reg.register(101);
        let t = make_token(101, 1);
        assert_eq!(g.unbind(101, 0, t).err(), Some(TkCode::NoActiveOverride.code()));
        let _ = g.bind(101, 0, t);
        assert_eq!(g.bind(101, 0, t).err(), Some(TkCode::AlreadyBound.code()));
        let _ = g.unbind(101, 0, t);
        assert!(g.bind(101, 0, t).is_ok(), "撤销后可重绑");
        assert!(g.table.plane_consistent());
    }

    #[test]
    fn t_ghost_key_gate() {
        let mut g = AuthGate::new();
        let _ = g.reg.register(101);
        let t = make_token(101, 1);
        assert_eq!(g.bind(101, SLOT_CAP, t).err(), Some(TkCode::GhostKey.code()));
        assert_eq!(g.unbind(101, SLOT_CAP + 1, t).err(), Some(TkCode::GhostKey.code()));
    }

    #[test]
    fn t_register_gates() {
        // 重复注册拒 / 零 pid 拒 / 越界 pid 拒 / 表满显拒且账不越界。
        let mut g = AuthGate::new();
        let _ = g.reg.register(101);
        assert_eq!(g.reg.register(101).err(), Some(TkCode::PidDupRegister.code()));
        assert_eq!(g.reg.register(0).err(), Some(TkCode::PidOutOfRange.code()));
        assert_eq!(g.reg.register(1000).err(), Some(TkCode::PidOutOfRange.code()));
        let mut i = 0;
        while i < PROC_CAP {
            let _ = g.reg.register(100 + i as u32);
            i += 1;
        }
        assert_eq!(g.reg.register(999).err(), Some(TkCode::ProcTableFull.code()), "表满");
        assert!(g.reg.registered <= PROC_CAP as u32, "账不越界");
    }

    #[test]
    fn t_audit_ring_and_triple() {
        // 审计环：>32 条环形覆盖，len 恒 ≤CAP，三要素逐条齐备。
        let mut g = AuthGate::new();
        let _ = g.reg.register(101);
        let mut i = 0;
        while i < AUTH_LOG_CAP + 5 {
            // 首次过、后续重复绑拒——过/拒路径都入环。
            let _ = g.bind(101, 0, make_token(101, 1));
            i += 1;
        }
        assert_eq!(g.log.len, AUTH_LOG_CAP, "环形覆盖");
        assert_eq!(g.log.total, (AUTH_LOG_CAP + 5) as u32, "总数显性记账");
        assert!(g.log.triple_ok(), "三要素齐备");
    }

    #[test]
    fn t_segment_guard_and_detail() {
        assert!(segment_guard(), "0x7F6C0 窗口");
        assert!(detail_closed(), "文案闭集");
        assert_eq!(TkCode::StaleSalt.code(), SEG_BASE + 7, "码位推导");
    }
}
