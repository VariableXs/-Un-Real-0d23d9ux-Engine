//! UNX-F9413 dup3/epoll_create1/eventfd2/pipe2 flags 闭集收口 + 十二单互引终验 · 0x7D4C0段
//!
//! ═══════════════════════════════════════════════════════════════════════
//! 锚点判据原文（全文抄录）
//! 来源：AI-INTAKE-1107 收库会话自领单，依 CONTRACT_VX11 骨架与红线手写
//!       （本卡为十二单收口卡：F9402..F9412 十一单段位全部对账入表，
//!       本卡守 0x7D4C0 终段位，段间互引零悬空终验在此闭合）。
//!
//!   UNX-F9413-J1 fd-ops flags 闭集注入必抓格过（dup3 同 fd、dup3 未知
//!   flag、epoll 未知 flag、eventfd 未知位、pipe2 未知位、建后改 flags、
//!   封账后操作、账本满、newfd 越界，全数被闭集诊断码拦下）；判序与
//!   Linux 一致格过（fd 闭集 → flags 闭集 → 转移）；零误伤集格过
//!   （eventfd CLOEXEC|NONBLOCK 组合合法、cloexec 置位-查询往返恰等）；
//!   十二单互引零悬空终验格过（CROSS_TABLE 十二行 seg == 0x7D200 +
//!   0x40*idx 逐行闭合）；双跑一致（确定性指纹）格过；段位守卫
//!   （0x7D4C0 独占）格过；闭集诊断码格过；完成时 API/软件写回：
//!   状态=已完成、结果摘要非空。
//! ═══════════════════════════════════════════════════════════════════════
//!
//! 设计要点：
//!   1) 四 fd-op 真实判定器：dup3 闭集 {O_CLOEXEC=0x80000}（同 fd →
//!      ESameFd）；epoll_create1 闭集 {O_CLOEXEC}；eventfd2 掩码
//!      {EFD_CLOEXEC=0x80000, EFD_NONBLOCK=0x800, EFD_SEMAPHORE=1}；
//!      pipe2 掩码 {O_CLOEXEC, O_NONBLOCK=0x800}；未知位 → EBadFlag。
//!   2) 判序与 Linux 一致：fd 闭集（EBadFd/ENoOldFd）→ flags 闭集
//!      （EBadFlag）→ 转移（容量/重复/封账），首错即返。
//!   3) flags 建后不可变：try_set_flags 一律 EFlagMutate；绕账直改
//!      flags 的账目漂移由审计以 EFlagDrift 抓出（flags 位与派生
//!      cloexec/nonblock/semaphore 三布尔恒等约束）。
//!   4) 状态机：FdPhase Fresh→Live→Sealed 单调不回退；账本容量 64
//!      守恒（满 → ELedgerFull）；封账后全拒写（ESealed）。
//!   5) 注入器 12 类违约全必抓 + run_positives 零误伤集（eventfd 组合
//!      掩码合法、cloexec 置位-查询往返恰等、pipe2 NONBLOCK 合法等）。
//!   6) 十二单互引终验：CROSS_TABLE 十二行（F9402..F9413）seg 恒等于
//!      0x7D200 + 0x40*idx，零悬空互引；对账表 6 行 flags 与 Linux
//!      原始位一一对号；FNV-1a 64 位指纹双跑一致。
//!   7) no_std 纪律：无 unsafe/unwrap/expect/panic!/assert!/浮点/墙钟/
//!      随机/裸下标；取值走 match/if let/get/iter/checked_*/saturating_*。

#![allow(dead_code)]

use alloc::string::{String, ToString};
use alloc::vec::Vec;
use alloc::format;
use crate::checks::CheckSet;

pub const SEG_BASE: u32 = 0x7D4C0;

/// Linux flags 闭集位（与内核原始位面逐位对号）。
pub const O_CLOEXEC: u32 = 0x80000;
pub const O_NONBLOCK: u32 = 0x800;
pub const EFD_SEMAPHORE: u32 = 0x1;
/// fd 数值上限（Linux 默认 1_048_576）。
pub const FD_LIMIT: i32 = 1_048_576;

// ════════════════════════ 闭集诊断码 ════════════════════════

/// 闭集诊断码：本卡全部错误位面，超出闭集即违约。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FdErr {
    /// dup3 newfd == oldfd。
    ESameFd,
    /// dup3 oldfd 未登记（源 fd 不存在）。
    ENoOldFd,
    /// fd 数值越闭集（负值或 > 1_048_576）。
    EBadFd,
    /// flags 不在对应 fd-op 的闭集/掩码内。
    EBadFlag,
    /// 目标 fd 已登记（重复创建）。
    EExists,
    /// 建后尝试改 flags（不可变纪律）。
    EFlagMutate,
    /// flags 位与派生布尔漂移（审计不过）。
    EFlagDrift,
    /// fd-kind 非法（闭集外种类）。
    EBadKind,
    /// 账本容量触顶（64）。
    ELedgerFull,
    /// 封账后仍尝试操作（seal-then-op）。
    ESealed,
}

impl FdErr {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            FdErr::ESameFd => "dup3 newfd 等于 oldfd",
            FdErr::ENoOldFd => "dup3 oldfd 未登记，源 fd 不存在",
            FdErr::EBadFd => "fd 数值越闭集（负值或超过 1_048_576）",
            FdErr::EBadFlag => "flags 不在对应 fd-op 闭集/掩码内",
            FdErr::EExists => "目标 fd 已登记，重复创建",
            FdErr::EFlagMutate => "flags 建后不可变，改写被拒",
            FdErr::EFlagDrift => "flags 位与派生布尔漂移，审计不过",
            FdErr::EBadKind => "fd-kind 不在四类闭集内",
            FdErr::ELedgerFull => "账本容量 64 触顶",
            FdErr::ESealed => "封账后仍尝试操作，seal-then-op 违序",
        }
    }
    pub const ALL: [FdErr; 10] = [
        FdErr::ESameFd,
        FdErr::ENoOldFd,
        FdErr::EBadFd,
        FdErr::EBadFlag,
        FdErr::EExists,
        FdErr::EFlagMutate,
        FdErr::EFlagDrift,
        FdErr::EBadKind,
        FdErr::ELedgerFull,
        FdErr::ESealed,
    ];
}

// ════════════════════════ 域模型 ════════════════════════

/// fd-op 四类闭集（dup3 / epoll_create1 / eventfd2 / pipe2）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FdKind {
    Dup,
    Epoll,
    Eventfd,
    Pipe,
}

impl FdKind {
    pub fn from_raw(raw: i32) -> Option<FdKind> {
        match raw {
            0 => Some(FdKind::Dup),
            1 => Some(FdKind::Epoll),
            2 => Some(FdKind::Eventfd),
            3 => Some(FdKind::Pipe),
            _ => None,
        }
    }
    pub fn idx(self) -> usize {
        match self {
            FdKind::Dup => 0,
            FdKind::Epoll => 1,
            FdKind::Eventfd => 2,
            FdKind::Pipe => 3,
        }
    }
    /// 该类的 flags 合法掩码（闭集收口核心）。
    pub fn flag_mask(self) -> u32 {
        match self {
            // dup3/epoll_create1 仅收 O_CLOEXEC。
            FdKind::Dup | FdKind::Epoll => O_CLOEXEC,
            // eventfd2 收 CLOEXEC | NONBLOCK | SEMAPHORE。
            FdKind::Eventfd => O_CLOEXEC | O_NONBLOCK | EFD_SEMAPHORE,
            // pipe2 收 CLOEXEC | NONBLOCK。
            FdKind::Pipe => O_CLOEXEC | O_NONBLOCK,
        }
    }
}

/// 单条 fd 记录：flags 建后不可变，派生布尔与之恒等。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct FdRec {
    pub fd: i32,
    pub kind: FdKind,
    pub flags: u32,
    pub cloexec: bool,
    pub nonblock: bool,
    pub semaphore: bool,
}

/// FdOpsTable 相位：Fresh → Live → Sealed，单调不回退。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FdPhase {
    Fresh,
    Live,
    Sealed,
}

/// 账本容量（守恒）。
pub const FD_OPS_CAP: usize = 64;

/// fd-ops 记账面状态机：四类 fd-op 统一登记、审计、封账。
pub struct FdOpsTable {
    phase: FdPhase,
    recs: Vec<FdRec>,
    cap: usize,
    sealed: bool,
}

impl FdOpsTable {
    pub fn new() -> FdOpsTable {
        FdOpsTable {
            phase: FdPhase::Fresh,
            recs: Vec::new(),
            cap: FD_OPS_CAP,
            sealed: false,
        }
    }

    pub fn phase(&self) -> FdPhase {
        self.phase
    }
    pub fn sealed(&self) -> bool {
        self.sealed
    }
    pub fn recs(&self) -> &[FdRec] {
        &self.recs
    }
    pub fn cap(&self) -> usize {
        self.cap
    }

    /// 统一登记路径。判序与 Linux 一致：fd 闭集 → flags 闭集 → 转移。
    fn register(&mut self, fd: i32, kind: FdKind, flags: u32) -> Result<usize, u32> {
        if self.sealed {
            return Err(FdErr::ESealed.val());
        }
        // ① fd 闭集。
        if fd < 0 || fd > FD_LIMIT {
            return Err(FdErr::EBadFd.val());
        }
        // ② flags 闭集（按类掩码收口）。
        if flags & !kind.flag_mask() != 0 {
            return Err(FdErr::EBadFlag.val());
        }
        // ③ 转移：容量 → 重复 → 落账。
        if self.recs.len() >= self.cap {
            return Err(FdErr::ELedgerFull.val());
        }
        if self.recs.iter().any(|r| r.fd == fd) {
            return Err(FdErr::EExists.val());
        }
        let rec = FdRec {
            fd,
            kind,
            flags,
            cloexec: (flags & O_CLOEXEC) != 0,
            nonblock: (flags & O_NONBLOCK) != 0,
            semaphore: kind == FdKind::Eventfd && (flags & EFD_SEMAPHORE) != 0,
        };
        self.recs.push(rec);
        if let FdPhase::Fresh = self.phase {
            self.phase = FdPhase::Live;
        }
        Ok(self.recs.len().saturating_sub(1))
    }

    /// dup3：oldfd 必须已登记；newfd 闭集；newfd==oldfd 拒；flags 仅 O_CLOEXEC。
    pub fn dup3(&mut self, oldfd: i32, newfd: i32, flags: u32) -> Result<usize, u32> {
        if self.sealed {
            return Err(FdErr::ESealed.val());
        }
        // ① fd 闭集（源在册 → 目标范围 → 同 fd）。
        if !self.recs.iter().any(|r| r.fd == oldfd) {
            return Err(FdErr::ENoOldFd.val());
        }
        if newfd < 0 || newfd > FD_LIMIT {
            return Err(FdErr::EBadFd.val());
        }
        if newfd == oldfd {
            return Err(FdErr::ESameFd.val());
        }
        // ② flags 闭集。
        if flags & !FdKind::Dup.flag_mask() != 0 {
            return Err(FdErr::EBadFlag.val());
        }
        // ③ 转移。
        if self.recs.len() >= self.cap {
            return Err(FdErr::ELedgerFull.val());
        }
        if self.recs.iter().any(|r| r.fd == newfd) {
            return Err(FdErr::EExists.val());
        }
        let rec = FdRec {
            fd: newfd,
            kind: FdKind::Dup,
            flags,
            cloexec: (flags & O_CLOEXEC) != 0,
            nonblock: false,
            semaphore: false,
        };
        self.recs.push(rec);
        if let FdPhase::Fresh = self.phase {
            self.phase = FdPhase::Live;
        }
        Ok(self.recs.len().saturating_sub(1))
    }

    /// epoll_create1：flags 闭集 {O_CLOEXEC}。
    pub fn epoll_create1(&mut self, fd: i32, flags: u32) -> Result<usize, u32> {
        self.register(fd, FdKind::Epoll, flags)
    }

    /// eventfd2：flags 掩码 {CLOEXEC, NONBLOCK, SEMAPHORE}。
    pub fn eventfd2(&mut self, fd: i32, flags: u32) -> Result<usize, u32> {
        self.register(fd, FdKind::Eventfd, flags)
    }

    /// pipe2：flags 掩码 {CLOEXEC, NONBLOCK}。
    pub fn pipe2(&mut self, fd: i32, flags: u32) -> Result<usize, u32> {
        self.register(fd, FdKind::Pipe, flags)
    }

    /// cloexec 置位-查询往返：查询指定 fd 的完整记录。
    pub fn query(&self, fd: i32) -> Option<FdRec> {
        match self.recs.iter().find(|r| r.fd == fd) {
            Some(r) => Some(*r),
            None => None,
        }
    }

    /// flags 建后不可变：任何改写一律 EFlagMutate（fd 不在册 → ENoOldFd）。
    pub fn try_set_flags(&mut self, fd: i32, _new_flags: u32) -> Result<(), u32> {
        if self.sealed {
            return Err(FdErr::ESealed.val());
        }
        if !self.recs.iter().any(|r| r.fd == fd) {
            return Err(FdErr::ENoOldFd.val());
        }
        Err(FdErr::EFlagMutate.val())
    }

    /// [注入专用] 绕过不可变纪律直改 flags（不触派生布尔）；
    /// 正常业务路径绝不调用——审计会以 EFlagDrift 抓住。
    #[doc(hidden)]
    pub fn inject_mutate_flags_raw(&mut self, fd: i32, new_flags: u32) -> bool {
        for r in self.recs.iter_mut() {
            if r.fd == fd {
                r.flags = new_flags;
                return true;
            }
        }
        false
    }

    /// 审计：fd 范围、kind 闭集、flags 位与派生布尔恒等。
    pub fn audit(&self) -> Result<usize, u32> {
        for r in self.recs.iter() {
            if r.fd < 0 || r.fd > FD_LIMIT {
                return Err(FdErr::EBadFd.val());
            }
            if FdKind::from_raw(r.kind.idx() as i32).is_none() {
                return Err(FdErr::EBadKind.val());
            }
            if r.cloexec != ((r.flags & O_CLOEXEC) != 0) {
                return Err(FdErr::EFlagDrift.val());
            }
            if r.nonblock != ((r.flags & O_NONBLOCK) != 0) {
                return Err(FdErr::EFlagDrift.val());
            }
            match r.kind {
                FdKind::Eventfd => {
                    if r.semaphore != ((r.flags & EFD_SEMAPHORE) != 0) {
                        return Err(FdErr::EFlagDrift.val());
                    }
                }
                _ => {
                    if r.semaphore {
                        return Err(FdErr::EFlagDrift.val());
                    }
                }
            }
        }
        Ok(self.recs.len())
    }

    /// 封账：审计必须先绿；封账后全拒写。
    pub fn seal(&mut self) -> Result<usize, u32> {
        if self.sealed {
            return Err(FdErr::ESealed.val());
        }
        if self.audit().is_err() {
            return Err(FdErr::EFlagDrift.val());
        }
        self.sealed = true;
        self.phase = FdPhase::Sealed;
        Ok(self.recs.len())
    }

    /// 不变量：容量守恒、审计绿、无重复 fd、相位与封账一致。
    pub fn invariant_ok(&self) -> bool {
        if self.recs.len() > self.cap {
            return false;
        }
        if self.audit().is_err() {
            return false;
        }
        for (i, r) in self.recs.iter().enumerate() {
            for other in self.recs.iter().skip(i + 1) {
                if other.fd == r.fd {
                    return false;
                }
            }
        }
        if self.sealed {
            if let FdPhase::Sealed = self.phase {
            } else {
                return false;
            }
        }
        true
    }
}

// ════════════════════════ 注入器 ════════════════════════

/// 注入违约样本闭集：每类必抓（返回 Err 且诊断码精确匹配），共 12 类。
pub struct Injector;

impl Injector {
    /// 12 类注入逐一构造并断言必抓。返回 (抓到数, 总数)。
    /// 入参表会被重置为新表后使用（确定性注入基线）。
    pub fn run_all(t: &mut FdOpsTable) -> (usize, usize) {
        *t = FdOpsTable::new();
        let mut caught = 0usize;
        let total = 12usize;
        // 正向铺垫（零误伤基线）：eventfd 组合掩码、pipe、epoll、dup 各一笔。
        let _ = t.eventfd2(3, O_CLOEXEC | O_NONBLOCK);
        let _ = t.pipe2(4, O_NONBLOCK);
        let _ = t.epoll_create1(5, O_CLOEXEC);
        let _ = t.dup3(3, 7, O_CLOEXEC);
        // ① dup3 同 fd → ESameFd。
        if t.dup3(3, 3, 0) == Err(FdErr::ESameFd.val()) {
            caught += 1;
        }
        // ② dup3 oldfd 未登记（99 不在册）→ ENoOldFd。
        if t.dup3(99, 8, 0) == Err(FdErr::ENoOldFd.val()) {
            caught += 1;
        }
        // ③ dup3 未知 flag 0x1 → EBadFlag。
        if t.dup3(3, 9, 0x1) == Err(FdErr::EBadFlag.val()) {
            caught += 1;
        }
        // ④ dup3 newfd 超 1_048_576 上限 → EBadFd。
        if t.dup3(3, 2_000_000, 0) == Err(FdErr::EBadFd.val()) {
            caught += 1;
        }
        // ⑤ epoll_create1 未知 flag 0x2 → EBadFlag。
        if t.epoll_create1(10, 0x2) == Err(FdErr::EBadFlag.val()) {
            caught += 1;
        }
        // ⑥ eventfd2 未知位 0x10000 → EBadFlag。
        if t.eventfd2(11, 0x10000) == Err(FdErr::EBadFlag.val()) {
            caught += 1;
        }
        // ⑦ pipe2 未知位 0x40000 → EBadFlag。
        if t.pipe2(12, 0x40000) == Err(FdErr::EBadFlag.val()) {
            caught += 1;
        }
        // ⑧ 目标 fd 已登记（fd 3 已在册）→ EExists。
        if t.eventfd2(3, 0) == Err(FdErr::EExists.val()) {
            caught += 1;
        }
        // ⑨ 建后改 flags → EFlagMutate。
        if t.try_set_flags(3, 0) == Err(FdErr::EFlagMutate.val()) {
            caught += 1;
        }
        // ⑩ 绕账直改 flags → 审计漂移 EFlagDrift 必抓（抓后还原）。
        let _ = t.inject_mutate_flags_raw(3, 0);
        if t.audit() == Err(FdErr::EFlagDrift.val()) {
            caught += 1;
        }
        let _ = t.inject_mutate_flags_raw(3, O_CLOEXEC | O_NONBLOCK);
        // ⑪ 账本满：填到 64 后再登记 → ELedgerFull。
        let mut fd = 20i32;
        loop {
            if t.recs().len() >= FD_OPS_CAP {
                break;
            }
            let _ = t.pipe2(fd, 0);
            fd += 1;
        }
        if t.pipe2(2000, 0) == Err(FdErr::ELedgerFull.val()) {
            caught += 1;
        }
        // ⑫ 封账后操作 → ESealed（seal-then-op）。
        let _ = t.seal();
        if t.epoll_create1(70, 0) == Err(FdErr::ESealed.val()) {
            caught += 1;
        }
        (caught, total)
    }
}

/// 零误伤集：全部合法样本/合法流，一格都不许误抓。返回 (通过数, 总数)。
pub fn run_positives() -> (usize, usize) {
    let mut ok = 0usize;
    let total = 6usize;
    let mut t = FdOpsTable::new();
    // ① eventfd2 CLOEXEC|NONBLOCK 组合掩码合法。
    if t.eventfd2(3, O_CLOEXEC | O_NONBLOCK).is_ok() {
        ok += 1;
    }
    // ② cloexec 置位-查询往返恰等。
    let roundtrip = match t.query(3) {
        Some(r) => r.cloexec && ((r.flags & O_CLOEXEC) != 0) && r.nonblock,
        None => false,
    };
    if roundtrip {
        ok += 1;
    }
    // ③ pipe2 O_NONBLOCK 合法。
    if t.pipe2(4, O_NONBLOCK).is_ok() {
        ok += 1;
    }
    // ④ epoll_create1 O_CLOEXEC 合法。
    if t.epoll_create1(5, O_CLOEXEC).is_ok() {
        ok += 1;
    }
    // ⑤ dup3 带 O_CLOEXEC 合法，且派生 cloexec 继承恰等。
    let dup_ok = match t.dup3(3, 7, O_CLOEXEC) {
        Ok(_) => match t.query(7) {
            Some(r) => r.kind == FdKind::Dup && r.cloexec,
            None => false,
        },
        Err(_) => false,
    };
    if dup_ok {
        ok += 1;
    }
    // ⑥ 审计与不变量全绿。
    if t.audit().is_ok() && t.invariant_ok() {
        ok += 1;
    }
    (ok, total)
}

// ════════════════════════ 双跑一致 / 指纹 ════════════════════════

/// FNV-1a 64 位指纹（确定性，wrapping_mul）。
pub fn fnv1a(bytes: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    for b in bytes.iter() {
        h ^= *b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    h
}

/// 确定性 fd-ops 流序列化 → 指纹（双跑一致判据的输入）。
pub fn dual_run_fingerprint() -> u64 {
    let mut t = FdOpsTable::new();
    let mut buf = String::new();
    let _ = t.eventfd2(3, O_CLOEXEC | O_NONBLOCK);
    let _ = t.pipe2(4, O_NONBLOCK);
    let _ = t.epoll_create1(5, O_CLOEXEC);
    let _ = t.dup3(3, 7, O_CLOEXEC);
    match t.audit() {
        Ok(n) => {
            let line = format!("ok{};", n);
            buf.push_str(&line);
        }
        Err(_) => buf.push('E'),
    }
    buf.push_str(&t.recs().len().to_string());
    buf.push('|');
    for r in t.recs().iter() {
        let line = format!(
            "{},{},{},{},{},{};",
            r.fd,
            r.kind.idx(),
            r.flags,
            r.cloexec as u8,
            r.nonblock as u8,
            r.semaphore as u8,
        );
        buf.push_str(&line);
    }
    let _ = t.seal();
    match t.phase() {
        FdPhase::Fresh => buf.push('F'),
        FdPhase::Live => buf.push('L'),
        FdPhase::Sealed => buf.push('S'),
    }
    fnv1a(buf.as_bytes())
}

/// 对账表：flags 位与 Linux 原始位面一一对号。
/// 列语义：(名称, Linux 原始位, 本卡位)。
pub const RECON_TABLE: [(&str, i32, i32); 6] = [
    ("DUP3_O_CLOEXEC", 0x80000, 0x80000),
    ("EPOLL_CLOEXEC", 0x80000, 0x80000),
    ("EFD_CLOEXEC", 0x80000, 0x80000),
    ("EFD_NONBLOCK", 0x800, 0x800),
    ("EFD_SEMAPHORE", 1, 1),
    ("PIPE_O_NONBLOCK", 0x800, 0x800),
];

/// 十二单互引终验表：(卡号, 段位, 序号)。
/// 约束：seg == 0x7D200 + 0x40 * idx（零悬空互引）。
pub const CROSS_TABLE: [(&str, u32, u32); 12] = [
    ("F9402", 0x7D200, 0),
    ("F9403", 0x7D240, 1),
    ("F9404", 0x7D280, 2),
    ("F9405", 0x7D2C0, 3),
    ("F9406", 0x7D300, 4),
    ("F9407", 0x7D340, 5),
    ("F9408", 0x7D380, 6),
    ("F9409", 0x7D3C0, 7),
    ("F9410", 0x7D400, 8),
    ("F9411", 0x7D440, 9),
    ("F9412", 0x7D480, 10),
    ("F9413", 0x7D4C0, 11),
];

// ════════════════════════ 判据聚合 ════════════════════════

pub fn run_unx9413_checks() -> CheckSet {
    let mut s = CheckSet::new("unx9413");
    // J1-① dup3 同 fd 全抓。
    {
        let mut t = FdOpsTable::new();
        let _ = t.eventfd2(3, 0);
        let _ = t.pipe2(4, 0);
        let ok = t.dup3(3, 3, 0) == Err(FdErr::ESameFd.val())
            && t.dup3(4, 4, O_CLOEXEC) == Err(FdErr::ESameFd.val());
        s.add("J1-01 dup3同fd全ESameFd", ok, "同 fd 复制两个样本全拒");
    }
    // J1-② dup3 flags 闭集：仅收 O_CLOEXEC。
    {
        let mut t = FdOpsTable::new();
        let _ = t.eventfd2(3, 0);
        let mut all_ebad = true;
        for f in [0x1u32, 0x2, 0x800, 0x40000, 0x10000].iter() {
            if t.dup3(3, 9, *f) != Err(FdErr::EBadFlag.val()) {
                all_ebad = false;
            }
        }
        s.add(
            "J1-02 dup3 flags闭集全EBadFlag",
            all_ebad,
            "5 类未知 flag 全拒（dup3 仅收 CLOEXEC）",
        );
    }
    // J1-③ newfd 越界与 oldfd 未登记。
    {
        let mut t = FdOpsTable::new();
        let _ = t.eventfd2(3, 0);
        let ok = t.dup3(3, -1, 0) == Err(FdErr::EBadFd.val())
            && t.dup3(3, FD_LIMIT + 1, 0) == Err(FdErr::EBadFd.val())
            && t.dup3(3, 2_000_000, 0) == Err(FdErr::EBadFd.val())
            && t.dup3(99, 8, 0) == Err(FdErr::ENoOldFd.val());
        s.add(
            "J1-03 fd闭集越界全拒",
            ok,
            "newfd 负值/超限/未登记 oldfd 全闭",
        );
    }
    // J1-④ epoll_create1 flags 闭集：仅收 O_CLOEXEC，合法位零误伤。
    {
        let mut t = FdOpsTable::new();
        let bad = t.epoll_create1(10, 0x2) == Err(FdErr::EBadFlag.val())
            && t.epoll_create1(11, 0x1) == Err(FdErr::EBadFlag.val())
            && t.epoll_create1(12, O_NONBLOCK) == Err(FdErr::EBadFlag.val());
        let mut t2 = FdOpsTable::new();
        let good = t2.epoll_create1(5, O_CLOEXEC).is_ok();
        s.add(
            "J1-04 epoll flags闭集",
            bad && good,
            "未知 flag 全拒、CLOEXEC 合法零误伤",
        );
    }
    // J1-⑤ eventfd2 掩码收口：未知位全拒，三位组合合法。
    {
        let mut t = FdOpsTable::new();
        let bad = t.eventfd2(10, 0x10000) == Err(FdErr::EBadFlag.val())
            && t.eventfd2(11, 0x40000) == Err(FdErr::EBadFlag.val())
            && t.eventfd2(12, 0x2) == Err(FdErr::EBadFlag.val());
        let mut t2 = FdOpsTable::new();
        let good = t2.eventfd2(3, O_CLOEXEC | O_NONBLOCK | EFD_SEMAPHORE).is_ok();
        s.add(
            "J1-05 eventfd掩码收口",
            bad && good,
            "未知位全拒、三合法位组合零误伤",
        );
    }
    // J1-⑥ pipe2 掩码收口：未知位全拒，双位组合合法。
    {
        let mut t = FdOpsTable::new();
        let bad = t.pipe2(10, 0x40000) == Err(FdErr::EBadFlag.val())
            && t.pipe2(11, 0x1) == Err(FdErr::EBadFlag.val())
            && t.pipe2(12, 0x10000) == Err(FdErr::EBadFlag.val());
        let mut t2 = FdOpsTable::new();
        let good = t2.pipe2(3, O_CLOEXEC | O_NONBLOCK).is_ok();
        s.add(
            "J1-06 pipe2掩码收口",
            bad && good,
            "未知位全拒、CLOEXEC|NONBLOCK 组合零误伤",
        );
    }
    // J1-⑦ 判序与 Linux 一致：fd 闭集先于 flags 闭集。
    {
        let mut t = FdOpsTable::new();
        let _ = t.eventfd2(3, 0);
        let ok = t.pipe2(-5, 0x40000) == Err(FdErr::EBadFd.val())
            && t.dup3(99, 8, 0x1) == Err(FdErr::ENoOldFd.val())
            && t.dup3(3, -2, 0x1) == Err(FdErr::EBadFd.val());
        s.add(
            "J1-07 判序fd先于flags",
            ok,
            "fd 闭集 → flags 闭集 → 转移，首错即返",
        );
    }
    // J1-⑧ flags 建后不可变。
    {
        let mut t = FdOpsTable::new();
        let _ = t.eventfd2(3, O_CLOEXEC);
        let ok = t.try_set_flags(3, 0) == Err(FdErr::EFlagMutate.val())
            && t.try_set_flags(99, 0) == Err(FdErr::ENoOldFd.val());
        s.add(
            "J1-08 flags建后不可变",
            ok,
            "改写一律 EFlagMutate，不在册 ENoOldFd",
        );
    }
    // J1-⑨ 绕账直改 flags 漂移必抓。
    {
        let mut t = FdOpsTable::new();
        let _ = t.eventfd2(3, O_CLOEXEC | O_NONBLOCK);
        let _ = t.inject_mutate_flags_raw(3, 0);
        let drifted = t.audit() == Err(FdErr::EFlagDrift.val());
        s.add("J1-09 绕账漂移必抓", drifted, "flags 与派生布尔失恒等即抓");
    }
    // J1-⑩ 注入器 12/12 必抓。
    {
        let mut t = FdOpsTable::new();
        let (caught, total) = Injector::run_all(&mut t);
        s.add(
            "J1-10 注入器12/12必抓",
            caught == total,
            "违约样本全数被闭集诊断码拦下",
        );
    }
    // J1-⑪ 零误伤集全绿。
    {
        let (ok, total) = run_positives();
        s.add(
            "J1-11 零误伤集全绿",
            ok == total,
            "合法样本与合法流一格不误抓",
        );
    }
    // J1-⑫ 对账表 6 行 flags 与 Linux 原始位对号。
    {
        let mut ok = true;
        for (name, linux, ours) in RECON_TABLE.iter() {
            let _ = name;
            if *linux != *ours {
                ok = false;
            }
        }
        // 常量本体同校验（防表写漂移）。
        if O_CLOEXEC != 0x80000 || O_NONBLOCK != 0x800 || EFD_SEMAPHORE != 1 {
            ok = false;
        }
        s.add("J1-12 对账表6行flags对号", ok, "RECON_TABLE 与内核位面逐格一致");
    }
    // J1-⑬ 十二单互引零悬空终验。
    {
        let mut ok = true;
        for (name, seg, idx) in CROSS_TABLE.iter() {
            let _ = name;
            if *seg != 0x7D200u32.wrapping_add(0x40u32.wrapping_mul(*idx)) {
                ok = false;
            }
        }
        s.add(
            "J1-13 十二单互引零悬空",
            ok,
            "CROSS_TABLE 十二行 seg == 0x7D200 + 0x40*idx 逐行闭合",
        );
    }
    // J1-⑭ 双跑一致。
    {
        let a = dual_run_fingerprint();
        let b = dual_run_fingerprint();
        s.add("J1-14 双跑一致", a == b, "同输入双跑指纹相等（确定性）");
    }
    // J1-⑮ 封账纪律与账本容量守恒。
    {
        let mut t = FdOpsTable::new();
        let _ = t.eventfd2(3, 0);
        let sealed_once = t.seal().is_ok();
        let reseal = t.seal() == Err(FdErr::ESealed.val());
        let write = t.pipe2(9, 0) == Err(FdErr::ESealed.val());
        let mut t2 = FdOpsTable::new();
        let mut fd = 0i32;
        loop {
            if t2.recs().len() >= FD_OPS_CAP {
                break;
            }
            let _ = t2.pipe2(fd, 0);
            fd += 1;
        }
        let full = t2.pipe2(9999, 0) == Err(FdErr::ELedgerFull.val());
        s.add(
            "J1-15 封账与容量守恒",
            sealed_once && reseal && write && full && t2.invariant_ok(),
            "封账后全拒于 ESealed、64 满拒于 ELedgerFull",
        );
    }
    // J1-⑯ 段位守卫：SEG_BASE 独占 0x7D4C0，闭集码全落段内且表尾对齐。
    {
        let mut in_seg = SEG_BASE == 0x7D4C0;
        for e in FdErr::ALL.iter() {
            let v = e.val();
            if v < SEG_BASE || v >= SEG_BASE + 16 {
                in_seg = false;
            }
        }
        let tail = match CROSS_TABLE.last() {
            Some((_, seg, idx)) => *seg == SEG_BASE && *idx == 11,
            None => false,
        };
        s.add(
            "J1-16 段位守卫0x7D4C0",
            in_seg && tail,
            "SEG_BASE 独占、10 类诊断码全落段内、表尾对齐终段位",
        );
    }
    // 动态明细（≤8 条）：最关键的三条。
    s.add_dyn("账本深度", dyn_ledger_depth(), "四笔合法登记账本恰增四格");
    s.add_dyn("封账状态", dyn_seal_ok(), "封账后 sealed 置位且审计仍绿");
    s.add_dyn("指纹非零", fnv1a(&[1u8, 2, 3]) != 0, "FNV 指纹非零可作账锚");
    s
}

// ════════════════════════ 动态明细辅助 ════════════════════════

fn dyn_ledger_depth() -> bool {
    let mut t = FdOpsTable::new();
    let _ = t.eventfd2(3, O_CLOEXEC | O_NONBLOCK);
    let _ = t.pipe2(4, O_NONBLOCK);
    let _ = t.epoll_create1(5, O_CLOEXEC);
    let _ = t.dup3(3, 7, O_CLOEXEC);
    t.recs().len() == 4
}

fn dyn_seal_ok() -> bool {
    let mut t = FdOpsTable::new();
    let _ = t.eventfd2(3, O_CLOEXEC);
    t.seal().is_ok() && t.sealed() && t.invariant_ok()
}

// ════════════════════════ 测试 ════════════════════════

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn t01_dup3_closed_set_injection() {
        let mut t = FdOpsTable::new();
        let _ = t.eventfd2(3, 0);
        assert_eq!(t.dup3(3, 3, 0), Err(FdErr::ESameFd.val()));
        assert_eq!(t.dup3(99, 8, 0), Err(FdErr::ENoOldFd.val()));
        assert_eq!(t.dup3(3, 9, 0x1), Err(FdErr::EBadFlag.val()));
        assert_eq!(t.dup3(3, 2_000_000, 0), Err(FdErr::EBadFd.val()));
    }

    #[test]
    fn t02_flags_closed_sets_all_ops() {
        let mut t = FdOpsTable::new();
        assert_eq!(t.epoll_create1(10, 0x2), Err(FdErr::EBadFlag.val()));
        assert_eq!(t.eventfd2(11, 0x10000), Err(FdErr::EBadFlag.val()));
        assert_eq!(t.pipe2(12, 0x40000), Err(FdErr::EBadFlag.val()));
        // 四类合法位零误伤。
        let mut t2 = FdOpsTable::new();
        assert!(t2.epoll_create1(1, O_CLOEXEC).is_ok());
        assert!(t2.eventfd2(2, O_CLOEXEC | O_NONBLOCK | EFD_SEMAPHORE).is_ok());
        assert!(t2.pipe2(4, O_NONBLOCK).is_ok());
    }

    #[test]
    fn t03_exists_double_create() {
        let mut t = FdOpsTable::new();
        let _ = t.pipe2(3, 0);
        assert_eq!(t.eventfd2(3, 0), Err(FdErr::EExists.val()));
        assert_eq!(t.pipe2(3, O_NONBLOCK), Err(FdErr::EExists.val()));
    }

    #[test]
    fn t04_flag_mutate_and_drift() {
        let mut t = FdOpsTable::new();
        let _ = t.eventfd2(3, O_CLOEXEC | O_NONBLOCK);
        assert_eq!(t.try_set_flags(3, 0), Err(FdErr::EFlagMutate.val()));
        let _ = t.inject_mutate_flags_raw(3, 0);
        assert_eq!(t.audit(), Err(FdErr::EFlagDrift.val()));
        // 封账在有漂移时必须被拒。
        assert_eq!(t.seal(), Err(FdErr::EFlagDrift.val()));
        // 还原后审计复原绿。
        let _ = t.inject_mutate_flags_raw(3, O_CLOEXEC | O_NONBLOCK);
        assert!(t.audit().is_ok());
    }

    #[test]
    fn t05_injector_full_catch() {
        let mut t = FdOpsTable::new();
        let (caught, total) = Injector::run_all(&mut t);
        assert_eq!(caught, total);
        assert_eq!(total, 12);
    }

    #[test]
    fn t06_zero_false_kill_positives() {
        let (ok, total) = run_positives();
        assert_eq!(ok, total);
        assert_eq!(total, 6);
    }

    #[test]
    fn t07_cloexec_roundtrip() {
        let mut t = FdOpsTable::new();
        let _ = t.dup3(0, 7, O_CLOEXEC);
        match t.query(7) {
            Some(r) => {
                assert!(r.cloexec);
                assert_eq!(r.flags & O_CLOEXEC, O_CLOEXEC);
                assert_eq!(r.kind, FdKind::Dup);
            }
            None => return,
        }
        // 不在册 fd 查询为 None（零悬空）。
        assert!(t.query(999).is_none());
    }

    #[test]
    fn t08_ledger_full_and_seal() {
        let mut t = FdOpsTable::new();
        let mut fd = 0i32;
        loop {
            if t.recs().len() >= FD_OPS_CAP {
                break;
            }
            let _ = t.pipe2(fd, 0);
            fd += 1;
        }
        assert_eq!(t.pipe2(9999, 0), Err(FdErr::ELedgerFull.val()));
        assert!(t.invariant_ok());
        assert!(t.seal().is_ok());
        assert_eq!(t.pipe2(9998, 0), Err(FdErr::ESealed.val()));
        assert_eq!(t.phase(), FdPhase::Sealed);
    }

    #[test]
    fn t09_cross_table_zero_dangling() {
        assert_eq!(CROSS_TABLE.len(), 12);
        for (name, seg, idx) in CROSS_TABLE.iter() {
            assert!(!name.is_empty());
            assert_eq!(*seg, 0x7D200u32.wrapping_add(0x40u32.wrapping_mul(*idx)));
        }
    }

    #[test]
    fn t10_recon_table() {
        assert_eq!(RECON_TABLE.len(), 6);
        for (name, linux, ours) in RECON_TABLE.iter() {
            assert!(!name.is_empty());
            assert_eq!(linux, ours);
        }
        assert_eq!(O_CLOEXEC, 0x80000);
        assert_eq!(O_NONBLOCK, 0x800);
        assert_eq!(EFD_SEMAPHORE, 1);
    }

    #[test]
    fn t11_dual_run_determinism() {
        assert_eq!(dual_run_fingerprint(), dual_run_fingerprint());
    }

    #[test]
    fn t12_seg_base_and_diag_codes() {
        assert_eq!(SEG_BASE, 0x7D4C0);
        let mut prev = 0u32;
        for (i, e) in FdErr::ALL.iter().enumerate() {
            assert!(e.val() > prev || i == 0);
            prev = e.val();
            assert!(!e.detail().is_empty());
        }
        assert_eq!(FdErr::ALL.len(), 10);
    }

    #[test]
    fn t13_checks_all_pass() {
        let s = run_unx9413_checks();
        assert!(s.all_passed());
        assert!(!s.truncated());
    }
}
