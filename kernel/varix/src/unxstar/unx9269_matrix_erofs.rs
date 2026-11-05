//! UNX-F9269 只读文件系统 EROFS 矩阵 · 0x7B0C0段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（plan.json id=9269 逐字摘录）：
//!   ① 纯功能代码 ≥320 行；② 判据原文：UNX-F9269-J1 EROFS 注入判据过，
//!   写路径全 EROFS 逐格检出，与 EACCES 判序（先挂载只读后权限）格过；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//!
//! 设计要点：
//!   ① 挂载态三分：Rw（可写）/ Remounting（重挂载中）/ Ro（只读）。写门
//!      在权限判定之前：Ro 一律 EROFS=30，Remounting 一律 EBUSY=16，只有
//!      Rw 才轮到权限位说话。判序反了就是把 EACCES 当成了 EROFS。
//!   ② 12 类写操作按目标分成目录写（Create/Mkdir/Unlink/Rmdir/Rename/
//!      Link/Symlink）与文件写（Truncate/Write/Setxattr），另加 Owner
//!      （Chmod）与 Chown（root-only），逐格判定，不留「默认放行」。
//!   ③ EROFS 注入：12 类 × 10 轮 = 120 次，只读挂载上以 root + 0777 的
//!      「最强权限」注入，仍必须逐格回 EROFS；同时 Rw 同格必须 Ok，二者
//!      同时成立才算检出。
//!   ④ 重挂载状态机 Rw→Remounting→Ro 单向；begin 两次、Rw 上 finish、
//!      Ro 上 abort/begin 均记非法。epoch 单调递增，abort 可回 Rw 恢复写。
//!   ⑤ 拒绝不改态：写操作被拒时 inode 的写代数 gen 不增；只有真正通过
//!      写门的操作才 +1，只读语义用数据本身证明。
//!   ⑥ 重挂载日志容量恒 16，溢出单独计数；双跑指纹、12 行对账、
//!      注入账守恒三条互相印证。
//! ─────────────────────────────────────────────────────────────────────

use alloc::format;
use crate::checks::CheckSet;

/// 诊断段基址（F9269 独占 0x40 窗口）。
pub const SEG_BASE: u32 = 0x7B0C0;

/// 写操作类数。
pub const OP_COUNT: usize = 12;
/// 单操作注入轮数。
pub const INJECT_ROUNDS: u32 = 10;
/// 重挂载日志容量。
pub const LOG_CAP: usize = 16;

/// 只读文件系统。
pub const EROFS: i32 = 30;
/// 权限不足。
pub const EACCES: i32 = 13;
/// 重挂载中，设备忙。
pub const EBUSY: i32 = 16;
/// 状态迁移非法。
pub const EINVAL: i32 = 22;

// ── 诊断码闭集 ────────────────────────────────────────────────────────
/// F9269 段内诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ErofsCode {
    /// 只读挂载未回 EROFS
    NotErofs,
    /// 写路径漏检一格
    WriteLeak,
    /// Rw 下拒绝码错
    RwDenyWrong,
    /// 重挂载期未回 EBUSY
    BusyLost,
    /// 先权限后挂载，判序倒置
    OrderBroken,
    /// 只读读路径被误伤
    ReadBlocked,
    /// 被拒仍改了 inode
    MutationLeak,
    /// 状态机非法迁移未计
    StageOrder,
    /// epoch 不单调
    EpochBroken,
    /// 重挂载日志越界
    LogOverflow,
    /// 容量不符
    Capacity,
    /// 注入漏检
    InjectionMiss,
    /// 注入账不守恒
    LedgerBroken,
    /// 双跑不一致
    NotReproducible,
    /// 文案为空
    TextEmpty,
    /// 段位越界
    SegmentViolation,
}

impl ErofsCode {
    /// 码值。
    pub fn val(self) -> u32 { SEG_BASE + self as u32 }

    /// 中文诊断与可操作建议。
    pub fn detail(self) -> &'static str {
        match self {
            ErofsCode::NotErofs => "只读未拒：Ro 挂载上写操作必须回 EROFS=30；请检查写门位置",
            ErofsCode::WriteLeak => "写路径漏检：12 类写操作逐格都过写门，不许有旁路",
            ErofsCode::RwDenyWrong => "Rw 拒绝码错：可写挂载上权限不足必须 EACCES=13",
            ErofsCode::BusyLost => "重挂载期未拒：Remounting 必须 EBUSY=16，不得回 EROFS",
            ErofsCode::OrderBroken => "判序倒置：先判挂载只读，再判权限；反了会把 EACCES 暴露成 EROFS",
            ErofsCode::ReadBlocked => "读被误伤：只读只拦写，读路径仍按权限位放行",
            ErofsCode::MutationLeak => "拒绝仍改态：写门拒绝后 inode 写代数必须不变",
            ErofsCode::StageOrder => "状态机倒置：Rw→Remounting→Ro 单向，非法迁移必须计数",
            ErofsCode::EpochBroken => "epoch 不单调：每轮 begin 自增 1，abort 不得回退 epoch",
            ErofsCode::LogOverflow => "日志越界：容量恒 16，溢出单独计数不得覆盖旧条目",
            ErofsCode::Capacity => "容量不符：写操作闭集 12、日志槽 16，多一个少一个都算错",
            ErofsCode::InjectionMiss => "注入漏检：最强权限下仍应 EROFS，未拒即漏",
            ErofsCode::LedgerBroken => "注入账不守恒：注入 = 检出 + 漏检，请检查记账点",
            ErofsCode::NotReproducible => "双跑不一致：同挂载态两次指纹不同；请排查隐藏状态",
            ErofsCode::TextEmpty => "文案为空：诊断码必须带非空建议",
            ErofsCode::SegmentViolation => "段位越界：SEG_BASE=0x7B0C0 须 0x40 对齐且在 0x7A000..=0x7D1C0",
        }
    }

    /// 闭集全量。
    pub const ALL: [ErofsCode; 16] = [
        ErofsCode::NotErofs, ErofsCode::WriteLeak, ErofsCode::RwDenyWrong,
        ErofsCode::BusyLost, ErofsCode::OrderBroken, ErofsCode::ReadBlocked,
        ErofsCode::MutationLeak, ErofsCode::StageOrder, ErofsCode::EpochBroken,
        ErofsCode::LogOverflow, ErofsCode::Capacity, ErofsCode::InjectionMiss,
        ErofsCode::LedgerBroken, ErofsCode::NotReproducible, ErofsCode::TextEmpty,
        ErofsCode::SegmentViolation,
    ];
}

// ── 主体模型 ──────────────────────────────────────────────────────────
/// 主体：uid/gid 二元组。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Cred {
    /// 用户号
    pub uid: u32,
    /// 组号
    pub gid: u32,
}

impl Cred {
    /// 构造。
    pub const fn new(uid: u32, gid: u32) -> Cred { Cred { uid, gid } }
    /// root。
    pub const fn root() -> Cred { Cred { uid: 0, gid: 0 } }
}

/// 被写目标：目录或普通文件。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Inode {
    /// 属主
    pub uid: u32,
    /// 属组
    pub gid: u32,
    /// 权限位（不含类型位）
    pub mode: u32,
    /// 是否目录
    pub is_dir: bool,
    /// 写代数：只有真正落盘的写才自增
    pub gen: u32,
}

impl Inode {
    /// 目录。
    pub const fn dir(uid: u32, gid: u32, mode: u32) -> Inode {
        Inode { uid, gid, mode, is_dir: true, gen: 0 }
    }
    /// 普通文件。
    pub const fn file(uid: u32, gid: u32, mode: u32) -> Inode {
        Inode { uid, gid, mode, is_dir: false, gen: 0 }
    }
}

/// 权限需求类别。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PermNeed {
    /// 目录写 + 搜索
    DirWrite,
    /// 文件写
    FileWrite,
    /// 属主（chmod）
    Owner,
    /// root-only（chown）
    Chown,
}

/// 12 类写操作。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum WriteOp {
    /// 建文件
    Create,
    /// 建目录
    Mkdir,
    /// 删文件
    Unlink,
    /// 删目录
    Rmdir,
    /// 改名
    Rename,
    /// 硬链接
    Link,
    /// 软链接
    Symlink,
    /// 截断
    Truncate,
    /// 写入
    Write,
    /// 改权限
    Chmod,
    /// 改属主
    Chown,
    /// 写扩展属性
    Setxattr,
}

impl WriteOp {
    /// 闭集全量（顺序即注入序）。
    pub const ALL: [WriteOp; OP_COUNT] = [
        WriteOp::Create, WriteOp::Mkdir, WriteOp::Unlink, WriteOp::Rmdir,
        WriteOp::Rename, WriteOp::Link, WriteOp::Symlink, WriteOp::Truncate,
        WriteOp::Write, WriteOp::Chmod, WriteOp::Chown, WriteOp::Setxattr,
    ];

    /// 操作名。
    pub fn name(self) -> &'static str {
        match self {
            WriteOp::Create => "create",
            WriteOp::Mkdir => "mkdir",
            WriteOp::Unlink => "unlink",
            WriteOp::Rmdir => "rmdir",
            WriteOp::Rename => "rename",
            WriteOp::Link => "link",
            WriteOp::Symlink => "symlink",
            WriteOp::Truncate => "truncate",
            WriteOp::Write => "write",
            WriteOp::Chmod => "chmod",
            WriteOp::Chown => "chown",
            WriteOp::Setxattr => "setxattr",
        }
    }

    /// 权限需求。
    pub fn need(self) -> PermNeed {
        match self {
            WriteOp::Create => PermNeed::DirWrite,
            WriteOp::Mkdir => PermNeed::DirWrite,
            WriteOp::Unlink => PermNeed::DirWrite,
            WriteOp::Rmdir => PermNeed::DirWrite,
            WriteOp::Rename => PermNeed::DirWrite,
            WriteOp::Link => PermNeed::DirWrite,
            WriteOp::Symlink => PermNeed::DirWrite,
            WriteOp::Truncate => PermNeed::FileWrite,
            WriteOp::Write => PermNeed::FileWrite,
            WriteOp::Setxattr => PermNeed::FileWrite,
            WriteOp::Chmod => PermNeed::Owner,
            WriteOp::Chown => PermNeed::Chown,
        }
    }

    /// 是否目录类操作。
    pub fn targets_dir(self) -> bool { self.need() == PermNeed::DirWrite }
}

// ── 挂载状态机 ────────────────────────────────────────────────────────
/// 挂载阶段。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum MountStage {
    /// 可写
    Rw,
    /// 重挂载中（唯一会回 EBUSY 的窗口）
    Remounting,
    /// 只读
    Ro,
}

/// 卷：挂载态 + epoch + 迁移计数。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Volume {
    stage: MountStage,
    epoch: u32,
    transitions: u32,
    illegal: u32,
}

impl Volume {
    /// 指定阶段（测试与注入用）。
    pub const fn at(stage: MountStage, epoch: u32) -> Volume {
        Volume { stage, epoch, transitions: 0, illegal: 0 }
    }
    /// 新建可写卷。
    pub const fn rw() -> Volume { Volume::at(MountStage::Rw, 0) }
    /// 阶段。
    pub fn stage(&self) -> MountStage { self.stage }
    /// epoch。
    pub fn epoch(&self) -> u32 { self.epoch }
    /// 成功迁移数。
    pub fn transitions(&self) -> u32 { self.transitions }
    /// 非法迁移数。
    pub fn illegal(&self) -> u32 { self.illegal }

    /// 写门：Rw 放行，Remounting EBUSY，Ro EROFS。
    pub fn write_gate(&self) -> Result<(), i32> {
        match self.stage {
            MountStage::Rw => Ok(()),
            MountStage::Remounting => Err(EBUSY),
            MountStage::Ro => Err(EROFS),
        }
    }

    fn reject(&mut self) -> Result<u32, i32> {
        self.illegal = self.illegal.saturating_add(1);
        Err(EINVAL)
    }

    /// 开始重挂载：仅 Rw 可进，epoch 自增。
    pub fn begin(&mut self) -> Result<u32, i32> {
        if self.stage != MountStage::Rw { return self.reject(); }
        self.stage = MountStage::Remounting;
        self.epoch = self.epoch.saturating_add(1);
        self.transitions = self.transitions.saturating_add(1);
        Ok(self.epoch)
    }

    /// 完成重挂载：仅 Remounting 可进，进 Ro。
    pub fn finish(&mut self) -> Result<u32, i32> {
        if self.stage != MountStage::Remounting { return self.reject(); }
        self.stage = MountStage::Ro;
        self.transitions = self.transitions.saturating_add(1);
        Ok(self.epoch)
    }

    /// 放弃重挂载：仅 Remounting 可进，回 Rw，epoch 不回退。
    pub fn abort(&mut self) -> Result<u32, i32> {
        if self.stage != MountStage::Remounting { return self.reject(); }
        self.stage = MountStage::Rw;
        self.transitions = self.transitions.saturating_add(1);
        Ok(self.epoch)
    }
}

// ── 权限判定 ──────────────────────────────────────────────────────────
/// 选类后的三位权限。
pub fn class_bits(inode: &Inode, cred: &Cred) -> u32 {
    if cred.uid == inode.uid {
        (inode.mode >> 6) & 0o7
    } else if cred.gid == inode.gid {
        (inode.mode >> 3) & 0o7
    } else {
        inode.mode & 0o7
    }
}

/// 权限位是否覆盖请求位。
pub fn mode_allows(inode: &Inode, cred: &Cred, bit: u32) -> bool {
    class_bits(inode, cred) & bit != 0
}

/// 权限判定（不含挂载门）。
pub fn perm_grants(op: WriteOp, inode: &Inode, cred: &Cred) -> bool {
    match op.need() {
        PermNeed::DirWrite => {
            inode.is_dir && mode_allows(inode, cred, 0o2) && mode_allows(inode, cred, 0o1)
        }
        PermNeed::FileWrite => !inode.is_dir && mode_allows(inode, cred, 0o2),
        PermNeed::Owner => inode.uid == cred.uid,
        PermNeed::Chown => cred.uid == 0,
    }
}

/// 写判定：先挂载门，后权限位。判序即本模块的核心。
pub fn judge_write(vol: &Volume, op: WriteOp, inode: &Inode, cred: &Cred) -> Result<(), i32> {
    vol.write_gate()?;
    if perm_grants(op, inode, cred) { Ok(()) } else { Err(EACCES) }
}

/// 读判定：只读不拦读，权限位照常生效。
pub fn judge_read(_vol: &Volume, inode: &Inode, cred: &Cred) -> Result<(), i32> {
    if mode_allows(inode, cred, 0o4) { Ok(()) } else { Err(EACCES) }
}

/// 落盘：被拒时不动 gen，通过才 +1。
pub fn apply_write(vol: &Volume, op: WriteOp, inode: &mut Inode, cred: &Cred) -> Result<u32, i32> {
    judge_write(vol, op, inode, cred)?;
    inode.gen = inode.gen.wrapping_add(1);
    Ok(inode.gen)
}

// ── 重挂载日志（固定容量） ────────────────────────────────────────────
/// 容量恒 16 的 epoch 日志，溢出单独计数。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct RemountLog {
    slots: [u32; LOG_CAP],
    len: usize,
    overflow: u32,
}

impl RemountLog {
    /// 空日志。
    pub const fn new() -> RemountLog {
        RemountLog { slots: [0u32; LOG_CAP], len: 0, overflow: 0 }
    }
    /// 条数。
    pub fn len(&self) -> usize { self.len }
    /// 溢出计数。
    pub fn overflow(&self) -> u32 { self.overflow }
    /// 取第 i 条。
    pub fn get(&self, i: usize) -> Option<u32> { self.slots.get(i).copied().filter(|_| i < self.len) }

    /// 追加：满了只计溢出，不覆盖旧条目。
    pub fn push(&mut self, epoch: u32) -> bool {
        if self.len >= LOG_CAP {
            self.overflow = self.overflow.saturating_add(1);
            return false;
        }
        if let Some(slot) = self.slots.get_mut(self.len) {
            *slot = epoch;
            self.len += 1;
            true
        } else {
            self.overflow = self.overflow.saturating_add(1);
            false
        }
    }
}

// ── 注入矩阵 ──────────────────────────────────────────────────────────
/// 注入账。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Ledger {
    /// 注入总数
    pub injected: u32,
    /// 检出数
    pub detected: u32,
    /// 漏检数
    pub missed: u32,
}

impl Ledger {
    /// 零账。
    pub const fn zero() -> Ledger { Ledger { injected: 0, detected: 0, missed: 0 } }
    /// 记一次。
    pub fn record(&mut self, caught: bool) {
        self.injected = self.injected.saturating_add(1);
        if caught {
            self.detected = self.detected.saturating_add(1);
        } else {
            self.missed = self.missed.saturating_add(1);
        }
    }
    /// 守恒。
    pub fn conserved(&self) -> bool {
        self.detected.saturating_add(self.missed) == self.injected
    }
}

/// 12 类写操作 × 10 轮：Ro 上最强权限必须 EROFS，Rw 同格必须 Ok。
pub fn run_erofs_matrix() -> (Ledger, [u32; OP_COUNT]) {
    let ro = Volume::at(MountStage::Ro, 1);
    let root = Cred::root();
    let dir = Inode::dir(0, 0, 0o777);
    let file = Inode::file(0, 0, 0o666);
    let mut ledger = Ledger::zero();
    let mut per = [0u32; OP_COUNT];
    let mut i = 0usize;
    while i < WriteOp::ALL.len() {
        if let Some(op) = WriteOp::ALL.get(i).copied() {
            let target = if op.targets_dir() { &dir } else { &file };
            let mut round = 0u32;
            while round < INJECT_ROUNDS {
                let blocked = judge_write(&ro, op, target, &root) == Err(EROFS);
                let allowed = judge_write(&Volume::at(MountStage::Rw, 1), op, target, &root) == Ok(());
                let caught = blocked && allowed;
                ledger.record(caught);
                if caught {
                    if let Some(slot) = per.get_mut(i) { *slot = slot.saturating_add(1); }
                }
                round += 1;
            }
        }
        i += 1;
    }
    (ledger, per)
}

/// 判序探针：同一无权限主体，Ro 上回 EROFS，Rw 上回 EACCES。
pub fn run_order_probe() -> (u32, u32) {
    let ro = Volume::at(MountStage::Ro, 1);
    let rw = Volume::at(MountStage::Rw, 1);
    let nobody = Cred::new(65534, 65534);
    let dir = Inode::dir(0, 0, 0o000);
    let file = Inode::file(0, 0, 0o000);
    let mut mount_first = 0u32;
    let mut perm_on_rw = 0u32;
    let mut i = 0usize;
    while i < WriteOp::ALL.len() {
        if let Some(op) = WriteOp::ALL.get(i).copied() {
            let target = if op.targets_dir() { &dir } else { &file };
            if judge_write(&ro, op, target, &nobody) == Err(EROFS) {
                mount_first = mount_first.saturating_add(1);
            }
            if judge_write(&rw, op, target, &nobody) == Err(EACCES) {
                perm_on_rw = perm_on_rw.saturating_add(1);
            }
        }
        i += 1;
    }
    (mount_first, perm_on_rw)
}

/// 重挂载窗口：12 类写操作必须全回 EBUSY。
pub fn run_busy_probe() -> u32 {
    let busy = Volume::at(MountStage::Remounting, 1);
    let root = Cred::root();
    let dir = Inode::dir(0, 0, 0o777);
    let file = Inode::file(0, 0, 0o666);
    let mut hit = 0u32;
    let mut i = 0usize;
    while i < WriteOp::ALL.len() {
        if let Some(op) = WriteOp::ALL.get(i).copied() {
            let target = if op.targets_dir() { &dir } else { &file };
            if judge_write(&busy, op, target, &root) == Err(EBUSY) {
                hit = hit.saturating_add(1);
            }
        }
        i += 1;
    }
    hit
}

// ── 对账与指纹 ────────────────────────────────────────────────────────
/// 对账表：(操作名, 是否目录类)。
pub const OP_TABLE: [(&str, bool); OP_COUNT] = [
    ("create", true), ("mkdir", true), ("unlink", true), ("rmdir", true),
    ("rename", true), ("link", true), ("symlink", true), ("truncate", false),
    ("write", false), ("chmod", false), ("chown", false), ("setxattr", false),
];

/// 对账：12 行名字、类别与行为（Ro→EROFS，Rw 无权限→EACCES）。
pub fn reconcile_ops() -> (u32, u32) {
    let ro = Volume::at(MountStage::Ro, 1);
    let rw = Volume::at(MountStage::Rw, 1);
    let root = Cred::root();
    let nobody = Cred::new(65534, 65534);
    let dir = Inode::dir(0, 0, 0o755);
    let file = Inode::file(0, 0, 0o644);
    let mut ok = 0u32;
    let mut i = 0usize;
    while i < WriteOp::ALL.len() {
        if let (Some(op), Some(row)) = (WriteOp::ALL.get(i).copied(), OP_TABLE.get(i)) {
            let target = if op.targets_dir() { &dir } else { &file };
            let ro_ok = table_target_ok(op, target, &root, &ro, false);
            let deny_target = if op.targets_dir() { &dir } else { &file };
            let rw_ok = judge_write(&rw, op, deny_target, &nobody) == Err(EACCES);
            if op.name() == row.0 && op.targets_dir() == row.1 && ro_ok && rw_ok {
                ok = ok.saturating_add(1);
            }
        }
        i += 1;
    }
    (ok, OP_TABLE.len() as u32)
}

fn table_target_ok(op: WriteOp, target: &Inode, cred: &Cred, vol: &Volume, _flag: bool) -> bool {
    judge_write(vol, op, target, cred) == Err(EROFS)
}

/// 确定性指纹：12 类操作的三种判定结果折叠。
pub fn erofs_fingerprint() -> u32 {
    let ro = Volume::at(MountStage::Ro, 1);
    let rw = Volume::at(MountStage::Rw, 1);
    let root = Cred::root();
    let nobody = Cred::new(65534, 65534);
    let dir = Inode::dir(0, 0, 0o755);
    let file = Inode::file(0, 0, 0o644);
    let mut h: u32 = 0x811C_9DC5;
    let mut i = 0usize;
    while i < WriteOp::ALL.len() {
        if let Some(op) = WriteOp::ALL.get(i).copied() {
            let target = if op.targets_dir() { &dir } else { &file };
            let blocked = judge_write(&ro, op, target, &root) == Err(EROFS);
            let allowed = judge_write(&rw, op, target, &root) == Ok(());
            let denied = judge_write(&rw, op, target, &nobody) == Err(EACCES);
            h ^= (op as u32).wrapping_mul(0x9E37_79B9) ^ if blocked && allowed && denied { 1 } else { 0 };
            h = h.wrapping_mul(0x0100_0193);
        }
        i += 1;
    }
    h
}

// ── 检查入口 ──────────────────────────────────────────────────────────
/// F9269 判据集（18 条静态 + 2 条动态明细）。
pub fn run_unx9269_checks() -> CheckSet {
    let mut s = CheckSet::new("unx9269");

    let seg_ok = SEG_BASE == 0x7B0C0 && SEG_BASE % 0x40 == 0
        && SEG_BASE >= 0x7A000 && SEG_BASE <= 0x7D1C0;
    s.add("段位守卫：0x7B0C0 且 0x40 对齐落在 0x7A000..=0x7D1C0", seg_ok,
          "段基址须独占且对齐；越界即整册拒绝注册");

    let root = Cred::root();
    let rw = Volume::at(MountStage::Rw, 1);
    let dir = Inode::dir(0, 0, 0o777);
    let file = Inode::file(0, 0, 0o666);
    let mut rw_ok = true;
    let mut i = 0usize;
    while i < WriteOp::ALL.len() {
        if let Some(op) = WriteOp::ALL.get(i).copied() {
            let target = if op.targets_dir() { &dir } else { &file };
            if judge_write(&rw, op, target, &root) != Ok(()) { rw_ok = false; }
        }
        i += 1;
    }
    s.add("正常流：Rw 挂载且权限足，12 类写操作全放行", rw_ok,
          "可写挂载是基线，写门不得误拦");

    let ro = Volume::at(MountStage::Ro, 1);
    let ro_dir444 = Inode::dir(0, 0, 0o555);
    let ro_file444 = Inode::file(0, 0, 0o444);
    let ro_file000 = Inode::file(0, 0, 0o000);
    let read_ok = judge_read(&ro, &ro_file444, &root) == Ok(())
        && judge_read(&ro, &ro_dir444, &root) == Ok(())
        && judge_read(&ro, &ro_file000, &root) == Err(EACCES);
    s.add("只读不拦读：Ro 上 0444 可读、0000 仍 EACCES", read_ok,
          "只读只约束写，读路径继续按权限位判定");

    let nobody = Cred::new(65534, 100);
    let deny_dir = Inode::dir(0, 0, 0o755);
    let deny_file = Inode::file(0, 0, 0o644);
    let mut deny_ok = true;
    let mut i = 0usize;
    while i < WriteOp::ALL.len() {
        if let Some(op) = WriteOp::ALL.get(i).copied() {
            let target = if op.targets_dir() { &deny_dir } else { &deny_file };
            if judge_write(&rw, op, target, &nobody) != Err(EACCES) { deny_ok = false; }
        }
        i += 1;
    }
    s.add("边界：Rw 上无权主体 12 类写全回 EACCES", deny_ok,
          "可写不等于随便写，权限位仍要逐格生效");

    let (ledger, per) = run_erofs_matrix();
    let mut per_ok = true;
    let mut pi = 0usize;
    while pi < per.len() {
        if per.get(pi) != Some(&INJECT_ROUNDS) { per_ok = false; }
        pi += 1;
    }
    s.add("注入必抓：12 类 × 10 轮 = 120 次全 EROFS", per_ok,
          "只读注入必须逐格检出，缺一格就是漏洞");

    let mut leak_ok = true;
    let mut i = 0usize;
    while i < WriteOp::ALL.len() {
        if let Some(op) = WriteOp::ALL.get(i).copied() {
            let target = if op.targets_dir() { &dir } else { &file };
            if judge_write(&ro, op, target, &root) != Err(EROFS) { leak_ok = false; }
        }
        i += 1;
    }
    s.add("写路径全 EROFS：Ro 上 root + 0777 也逐格拒绝", leak_ok,
          "最强权限都不能写，才算真正的只读");

    let (mount_first, perm_rw) = run_order_probe();
    s.add("判序：先挂载只读后权限（Ro→EROFS 12/12，Rw→EACCES 12/12）",
          mount_first == OP_COUNT as u32 && perm_rw == OP_COUNT as u32,
          "挂载门必须先于权限位，否则错误码会串");

    let busy_hits = run_busy_probe();
    s.add("重挂载窗口：Remounting 上 12 类写全回 EBUSY", busy_hits == OP_COUNT as u32,
          "重挂载期不是只读，回 EROFS 会误导调用方");

    let mut v = Volume::at(MountStage::Rw, 0);
    let begin_ok = v.begin() == Ok(1);
    let begin_twice = v.begin() == Err(EINVAL);
    let finish_ok = v.finish() == Ok(1);
    let abort_after_ro = v.abort() == Err(EINVAL);
    let begin_after_ro = v.begin() == Err(EINVAL);
    s.add("状态机：Rw→Remounting→Ro 单向，封只读后迁移全拒",
          begin_ok && begin_twice && finish_ok && abort_after_ro && begin_after_ro
              && v.stage() == MountStage::Ro && v.illegal() == 3 && v.transitions() == 2,
          "同一时刻只允许一个方向的迁移");

    let mut ab = Volume::at(MountStage::Rw, 0);
    let e1 = ab.begin();
    let a1 = ab.abort();
    let e2 = ab.begin();
    let f2 = ab.finish();
    let epoch_seq = e1 == Ok(1) && a1 == Ok(1) && e2 == Ok(2) && f2 == Ok(2);
    let abort_rw = ab.stage() == MountStage::Ro;
    let mut ab2 = Volume::at(MountStage::Rw, 5);
    let _ = ab2.begin();
    let _ = ab2.abort();
    let resumed = ab2.stage() == MountStage::Rw
        && judge_write(&ab2, WriteOp::Write, &file, &root) == Ok(())
        && ab2.epoch() == 6;
    s.add("epoch 单调：1→2 不回退，abort 回 Rw 后写恢复",
          epoch_seq && abort_rw && resumed,
          "abort 只回落挂载态，不回落 epoch");

    let mut log = RemountLog::new();
    let mut k = 0u32;
    while k < 20 {
        log.push(k + 1);
        k += 1;
    }
    let log_ok = log.len() == LOG_CAP && log.overflow() == 4
        && log.get(0) == Some(1) && log.get(LOG_CAP - 1) == Some(LOG_CAP as u32)
        && log.get(LOG_CAP).is_none();
    s.add("容量：重挂载日志恒 16 槽，溢出 4 次不覆盖旧条目", log_ok,
          "固定容量模型，溢出必须可数");

    let mut probe = Inode::file(0, 0, 0o666);
    let blocked = apply_write(&ro, WriteOp::Write, &mut probe, &root) == Err(EROFS)
        && probe.gen == 0;
    let landed = apply_write(&Volume::at(MountStage::Rw, 1), WriteOp::Write,
                             &mut probe, &root) == Ok(1)
        && probe.gen == 1;
    s.add("拒绝不改态：Ro 被拒 gen 不变，Rw 放行才 +1", blocked && landed,
          "写门拒绝必须在落盘之前，取消后不得有半截副作用");

    let errno_ok = EROFS == 30 && EACCES == 13 && EBUSY == 16 && EINVAL == 22
        && EROFS != EACCES && EROFS != EBUSY && EACCES != EBUSY;
    s.add("错误码闭集：EROFS=30 / EACCES=13 / EBUSY=16 / EINVAL=22 互异",
          errno_ok,
          "三个错误码指向三种原因，不许复用一个");

    let f1 = erofs_fingerprint();
    let f2 = erofs_fingerprint();
    s.add("双跑一致：12 类指纹相等且非零", f1 == f2 && f1 != 0,
          "无时钟无随机，同挂载态两次必须同指纹");

    let (rok, rtotal) = reconcile_ops();
    s.add("对账：12 行操作名、类别、行为全中", rok == rtotal && rtotal == 12,
          "名字与类别错一行，整张矩阵就错一格");

    let (mount_first2, perm_rw2) = run_order_probe();
    s.add("守恒：注入 = 检出 + 漏检，漏检恒 0 且探针可复现",
          ledger.conserved() && ledger.missed == 0
              && ledger.injected == (OP_COUNT as u32) * INJECT_ROUNDS
              && mount_first2 == OP_COUNT as u32 && perm_rw2 == OP_COUNT as u32,
          "120 次注入必须 120 检出 0 漏检");

    let mut text_ok = true;
    let mut ti = 0usize;
    while ti < ErofsCode::ALL.len() {
        if let Some(code) = ErofsCode::ALL.get(ti).copied() {
            if code.detail().is_empty() { text_ok = false; }
        }
        ti += 1;
    }
    s.add("文案闭集：16 个诊断码均带非空中文建议", text_ok,
          "红项必须能直接读到处置建议，不许返回空串");

    let mut uniq = true;
    let mut i = 0usize;
    while i < ErofsCode::ALL.len() {
        let mut j = i + 1;
        while j < ErofsCode::ALL.len() {
            if let (Some(a), Some(b)) = (ErofsCode::ALL.get(i), ErofsCode::ALL.get(j)) {
                if a.val() == b.val() { uniq = false; }
            }
            j += 1;
        }
        i += 1;
    }
    s.add("诊断码：16 个码值两两唯一", uniq,
          "段内闭集码重复会让红项无法定位");

    let ledger_text = format!("注入 {} / 检出 {} / 漏检 {} / 写操作 {}",
                              ledger.injected, ledger.detected, ledger.missed,
                              OP_COUNT);
    s.add_dyn("账目快照", ledger.conserved() && ledger.missed == 0, &ledger_text);
    let fp_text = format!("erofs 指纹 0x{:08X} 对账 {}/{} 判序 {}/{}",
                          f1, rok, rtotal, mount_first, perm_rw);
    s.add_dyn("指纹快照", f1 == f2 && rok == rtotal, &fp_text);

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn segment_is_anchored() {
        assert_eq!(SEG_BASE, 0x7B0C0);
        assert_eq!(SEG_BASE % 0x40, 0);
    }

    #[test]
    fn rw_owner_writes() {
        let v = Volume::at(MountStage::Rw, 1);
        let root = Cred::root();
        let dir = Inode::dir(0, 0, 0o755);
        let file = Inode::file(0, 0, 0o644);
        assert_eq!(judge_write(&v, WriteOp::Create, &dir, &root), Ok(()));
        assert_eq!(judge_write(&v, WriteOp::Write, &file, &root), Ok(()));
        assert_eq!(judge_write(&v, WriteOp::Chmod, &file, &root), Ok(()));
        assert_eq!(judge_write(&v, WriteOp::Chown, &file, &root), Ok(()));
    }

    #[test]
    fn ro_blocks_all_writes() {
        let ro = Volume::at(MountStage::Ro, 1);
        let root = Cred::root();
        let dir = Inode::dir(0, 0, 0o777);
        let file = Inode::file(0, 0, 0o666);
        let mut i = 0usize;
        while i < WriteOp::ALL.len() {
            if let Some(op) = WriteOp::ALL.get(i).copied() {
                let target = if op.targets_dir() { &dir } else { &file };
                assert_eq!(judge_write(&ro, op, target, &root), Err(EROFS));
            }
            i += 1;
        }
    }

    #[test]
    fn mount_gate_precedes_permission() {
        let ro = Volume::at(MountStage::Ro, 1);
        let rw = Volume::at(MountStage::Rw, 1);
        let nobody = Cred::new(65534, 65534);
        let file = Inode::file(0, 0, 0o000);
        assert_eq!(judge_write(&ro, WriteOp::Write, &file, &nobody), Err(EROFS));
        assert_eq!(judge_write(&rw, WriteOp::Write, &file, &nobody), Err(EACCES));
    }

    #[test]
    fn remounting_is_busy() {
        let busy = Volume::at(MountStage::Remounting, 3);
        let root = Cred::root();
        let dir = Inode::dir(0, 0, 0o777);
        assert_eq!(judge_write(&busy, WriteOp::Mkdir, &dir, &root), Err(EBUSY));
        assert_eq!(run_busy_probe(), OP_COUNT as u32);
    }

    #[test]
    fn state_machine_is_monotonic() {
        let mut v = Volume::at(MountStage::Rw, 0);
        assert_eq!(v.begin(), Ok(1));
        assert_eq!(v.begin(), Err(EINVAL));
        assert_eq!(v.finish(), Ok(1));
        assert_eq!(v.stage(), MountStage::Ro);
        assert_eq!(v.finish(), Err(EINVAL));
        assert_eq!(v.illegal(), 2);
    }

    #[test]
    fn abort_restores_rw() {
        let mut v = Volume::at(MountStage::Rw, 4);
        assert_eq!(v.begin(), Ok(5));
        assert_eq!(v.abort(), Ok(5));
        assert_eq!(v.stage(), MountStage::Rw);
        assert_eq!(v.epoch(), 5);
        let file = Inode::file(0, 0, 0o644);
        assert_eq!(judge_write(&v, WriteOp::Truncate, &file, &Cred::root()), Ok(()));
    }

    #[test]
    fn log_is_capped() {
        let mut log = RemountLog::new();
        let mut k = 0u32;
        while k < LOG_CAP as u32 + 4 {
            log.push(k + 1);
            k += 1;
        }
        assert_eq!(log.len(), LOG_CAP);
        assert_eq!(log.overflow(), 4);
        assert_eq!(log.get(0), Some(1));
        assert_eq!(log.get(LOG_CAP), None);
    }

    #[test]
    fn matrix_conserved_and_reconciled() {
        let (ledger, per) = run_erofs_matrix();
        assert!(ledger.conserved());
        assert_eq!(ledger.injected, 120);
        assert_eq!(per, [INJECT_ROUNDS; OP_COUNT]);
        let (ok, total) = reconcile_ops();
        assert_eq!(ok, total);
        assert_eq!(total, 12);
    }

    #[test]
    fn checks_all_green() {
        let s = run_unx9269_checks();
        assert!(s.all_passed(), "{}", s.red_items_text());
    }
}
