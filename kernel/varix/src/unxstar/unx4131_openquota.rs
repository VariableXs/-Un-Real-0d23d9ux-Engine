//! unx4131_openquota — UNX-F4131 打开计数与进程配额（fake C1）（诊断段 0x9240）。
//!
//! 判据原文：UNX-F4131-J1 进程级打开数配额耗尽注入返回 EMFILE，全局配额返回 ENFILE
//!           ——两级配额独立断言通过。
//!   J2 进程门先于全局门（EMFILE 先于 ENFILE）。
//!   J3 拒收零副作用：被拒 open 不占全局号。
//!   J4 global_open 恒等于各进程打开数之和。
//! 设计纪律：
//!   1. 两级配额判定的门序固定：先进程级（EMFILE 24），再全局（ENFILE 23）。门序
//!      与 Linux alloc_fd/expand_files 路径一致：先查 RLIMIT_NOFILE，再查
//!      fs.nr_open 与系统文件表余量。
//!   2. 两级账必须独立：进程级耗尽不得污染全局计数（被拒的 open 不占全局号），
//!      全局耗尽也不得把进程计数打高——「拒收即零副作用」是本模块的核心不变式。
//!   3. 计数守恒：global_open == Σ proc.open；granted == released + global_open；
//!      任一等式不成立即 E_ACCT。
//!   4. 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。
//! 诊断段 0x9240 本单独占。

use crate::alloc::vec::Vec;

pub const SEG_BASE: u16 = 0x9240;

// ── 诊断码闭集 ───────────────────────────────────────────────
const E_MFILE: u16 = SEG_BASE + 1; // 对应 EMFILE(24)：进程级打开数达限
const E_NFILE: u16 = SEG_BASE + 2; // 对应 ENFILE(23)：全局打开数达限
const E_NOPROC: u16 = SEG_BASE + 3; // 进程未登记
const E_BADPID: u16 = SEG_BASE + 4; // pid 非法（0 保留）
const E_UNDER: u16 = SEG_BASE + 5; // close 时计数已为 0（穿底）
const E_ACCT: u16 = SEG_BASE + 6; // 两级账不平
const E_DUP: u16 = SEG_BASE + 7; // 进程重复登记
const E_LIMIT0: u16 = SEG_BASE + 8; // 配额设为 0（拒服务而非隐式封禁）

/// EMFILE / ENFILE 的 errno 原值（POSIX errno.h）。
pub const EMFILE_ERRNO: u16 = 24;
pub const ENFILE_ERRNO: u16 = 23;

/// 进程级配额默认值：取 1/8 于全局，模拟 RLIMIT_NOFILE 的常见配比。
pub const PROC_LIMIT_DEFAULT: u32 = 64;
/// 全局配额默认值（fake C1 表容量）。
pub const GLOBAL_LIMIT_DEFAULT: u32 = 512;

/// 配额判定结果状态机：Grant 与两种耗尽态互斥，Released 仅由 close 产生。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum QuotaVerdict {
    Grant,
    Emfile,
    Enfile,
    Released,
}

/// 单进程打开计数：pid + 当前打开数 + 本级上限。
pub struct ProcQuota {
    /// 进程号（1..=u32::MAX-1）
    pub pid: u32,
    /// 当前打开文件数
    pub open: u32,
    /// 进程级上限
    pub limit: u32,
    /// 本进程被 EMFILE 拒收次数
    pub emfile_hits: u32,
}

impl ProcQuota {
    pub fn new(pid: u32, limit: u32) -> Result<ProcQuota, u16> {
        if pid == 0 {
            return Err(E_BADPID);
        }
        if limit == 0 {
            return Err(E_LIMIT0);
        }
        Ok(ProcQuota {
            pid,
            open: 0,
            limit,
            emfile_hits: 0,
        })
    }

    pub fn full(&self) -> bool {
        self.open >= self.limit
    }
}

/// 两级配额表：进程表 + 全局计数 + 三路计次。
pub struct QuotaTable {
    /// 进程配额表
    pub procs: Vec<ProcQuota>,
    /// 全局已打开数
    pub global_open: u32,
    /// 全局上限
    pub global_limit: u32,
    /// 成功授予次数
    pub granted: u32,
    /// 释放次数
    pub released: u32,
    /// 全局耗尽拒收次数
    pub enfiles: u32,
}

impl QuotaTable {
    pub fn new(global_limit: u32) -> Result<QuotaTable, u16> {
        if global_limit == 0 {
            return Err(E_LIMIT0);
        }
        Ok(QuotaTable {
            procs: Vec::new(),
            global_open: 0,
            global_limit,
            granted: 0,
            released: 0,
            enfiles: 0,
        })
    }

    /// 登记进程：pid 唯一，重复登记即拒。
    pub fn register(&mut self, pid: u32, limit: u32) -> Result<(), u16> {
        if self.find(pid).is_some() {
            return Err(E_DUP);
        }
        let p = ProcQuota::new(pid, limit)?;
        self.procs.push(p);
        Ok(())
    }

    fn find(&self, pid: u32) -> Option<usize> {
        let mut i = 0usize;
        while i < self.procs.len() {
            if self.procs[i].pid == pid {
                return Some(i);
            }
            i += 1;
        }
        None
    }

    /// open：先进程门（EMFILE），再全局门（ENFILE）。拒收不改任何计数。
    pub fn open(&mut self, pid: u32) -> Result<QuotaVerdict, u16> {
        let idx = match self.find(pid) {
            Some(i) => i,
            None => return Err(E_NOPROC),
        };
        if self.procs[idx].full() {
            self.procs[idx].emfile_hits += 1;
            return Ok(QuotaVerdict::Emfile);
        }
        if self.global_open >= self.global_limit {
            self.enfiles += 1;
            return Ok(QuotaVerdict::Enfile);
        }
        self.procs[idx].open += 1;
        self.global_open += 1;
        self.granted += 1;
        Ok(QuotaVerdict::Grant)
    }

    /// close：进程计数与全局计数同步减一；穿底即拒。
    pub fn close(&mut self, pid: u32) -> Result<QuotaVerdict, u16> {
        let idx = match self.find(pid) {
            Some(i) => i,
            None => return Err(E_NOPROC),
        };
        if self.procs[idx].open == 0 {
            return Err(E_UNDER);
        }
        if self.global_open == 0 {
            return Err(E_ACCT);
        }
        self.procs[idx].open -= 1;
        self.global_open -= 1;
        self.released += 1;
        Ok(QuotaVerdict::Released)
    }

    /// 进程余量：上限减已用（不饱和，调用方须先确认 used ≤ limit）。
    pub fn headroom(&self, pid: u32) -> Result<u32, u16> {
        let idx = match self.find(pid) {
            Some(i) => i,
            None => return Err(E_NOPROC),
        };
        let p = &self.procs[idx];
        if p.open > p.limit {
            return Err(E_ACCT);
        }
        Ok(p.limit - p.open)
    }

    /// 全局余量。
    pub fn global_headroom(&self) -> u32 {
        if self.global_open >= self.global_limit {
            0
        } else {
            self.global_limit - self.global_open
        }
    }

    /// 两级账对账：global_open == Σ open；granted == released + global_open。
    pub fn audit(&self) -> bool {
        let mut sum = 0u32;
        let mut i = 0usize;
        while i < self.procs.len() {
            sum += self.procs[i].open;
            if self.procs[i].open > self.procs[i].limit {
                return false; // 越限即账坏
            }
            i += 1;
        }
        if sum != self.global_open {
            return false;
        }
        if self.granted != self.released + self.global_open {
            return false;
        }
        self.global_open <= self.global_limit
    }

    /// 批级打开：一次尝试 n 个，逐笔入账，返回实际授予数（不整体回滚）。
    pub fn open_batch(&mut self, pid: u32, n: u32) -> Result<u32, u16> {
        let mut ok = 0u32;
        let mut i = 0u32;
        while i < n {
            match self.open(pid)? {
                QuotaVerdict::Grant => ok += 1,
                QuotaVerdict::Emfile | QuotaVerdict::Enfile => break,
                QuotaVerdict::Released => return Err(E_ACCT),
            }
            i += 1;
        }
        Ok(ok)
    }
}

/// 诊断码解释。
pub fn detail(code: u16) -> &'static str {
    match code {
        E_MFILE => "进程级打开数达限（EMFILE 24）",
        E_NFILE => "全局打开数达限（ENFILE 23）",
        E_NOPROC => "进程未登记",
        E_BADPID => "pid 非法（0 保留）",
        E_UNDER => "close 时计数已为 0（穿底）",
        E_ACCT => "两级账不平",
        E_DUP => "进程重复登记",
        E_LIMIT0 => "配额设为 0（拒服务）",
        _ => "未知诊断码",
    }
}

/// 确定性演练：两级独立 → EMFILE 10/10 → ENFILE → 释放复位 → 对账。
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;

    // R1：两进程各配额 4/10，全局 12，正常授予。
    let mut t = QuotaTable::new(12)?;
    t.register(0x11, 4)?; // 进程 A
    t.register(0x22, 10)?; // 进程 B（本级上限高于全局余量，便于分离两级）
    if t.headroom(0x11)? != 4 || t.global_headroom() != 12 {
        return Err(E_ACCT);
    }
    if t.open_batch(0x11, 2)? != 2 {
        return Err(E_ACCT);
    }
    if t.open_batch(0x22, 3)? != 3 {
        return Err(E_ACCT);
    }
    if t.global_open != 5 || !t.audit() {
        return Err(E_ACCT);
    }
    hits += 1;

    // R2：A 用满 4 → 第 5 次起 EMFILE 10/10 次（进程门先于全局门）。
    if t.open_batch(0x11, 2)? != 2 {
        return Err(E_ACCT);
    }
    if t.headroom(0x11)? != 0 {
        return Err(E_ACCT);
    }
    let mut emfile = 0u32;
    let mut i = 0;
    while i < 10 {
        if t.open(0x11)? != QuotaVerdict::Emfile {
            return Err(E_MFILE);
        }
        emfile += 1;
        i += 1;
    }
    if emfile != 10 {
        return Err(E_MFILE);
    }
    hits += 1;

    // R3：EMFILE 拒收零副作用——全局计数与 B 的计数都不动（两级独立）。
    if t.global_open != 7 {
        return Err(E_ACCT);
    }
    if t.procs[0].open != 4 || t.procs[0].emfile_hits != 10 {
        return Err(E_ACCT);
    }
    if t.enfiles != 0 {
        return Err(E_ACCT); // 进程门先拒，未走到全局门
    }
    if !t.audit() {
        return Err(E_ACCT);
    }
    hits += 1;

    // R4：B 继续用满全局剩余 5 个，第 6 次落 ENFILE（全局门）。
    if t.open_batch(0x22, 5)? != 5 {
        return Err(E_ACCT);
    }
    if t.global_open != 12 || t.global_headroom() != 0 {
        return Err(E_ACCT);
    }
    if t.open(0x22)? != QuotaVerdict::Enfile {
        return Err(E_NFILE);
    }
    if t.enfiles != 1 {
        return Err(E_ACCT);
    }
    hits += 1;

    // R5：B 已达全局门但仍未达自身 10 的限额（8<10），说明两级判据确实分离。
    if t.procs[1].open != 8 || t.procs[1].limit != 10 {
        return Err(E_ACCT);
    }
    if t.procs[1].emfile_hits != 0 {
        return Err(E_ACCT); // B 一次 EMFILE 都不该有
    }
    hits += 1;

    // R6：释放 3 个 → 全局回 9，A 仍满（释放 B 不动 A）。
    t.close(0x22)?;
    t.close(0x22)?;
    t.close(0x22)?;
    if t.global_open != 9 || t.procs[1].open != 5 {
        return Err(E_ACCT);
    }
    if t.procs[0].open != 4 || t.headroom(0x11)? != 0 {
        return Err(E_ACCT);
    }
    if t.open(0x11)? != QuotaVerdict::Emfile {
        return Err(E_MFILE);
    }
    hits += 1;

    // I1：close 穿底 → E_UNDER（不得把计数打成负数语义）。
    let mut k = 0;
    while k < 5 {
        t.close(0x22)?;
        k += 1;
    }
    if t.procs[1].open != 0 {
        return Err(E_ACCT);
    }
    if t.close(0x22) != Err(E_UNDER) {
        return Err(E_UNDER);
    }
    hits += 1;

    // I2：未登记进程 → E_NOPROC（不得隐式登记）。
    match t.open(0x33) {
        Err(E_NOPROC) => {}
        _ => return Err(E_NOPROC),
    }
    hits += 1;

    // I3：重复登记同 pid → E_DUP；pid=0 → E_BADPID；配额 0 → E_LIMIT0。
    if t.register(0x11, 4) != Err(E_DUP) {
        return Err(E_DUP);
    }
    if t.register(0, 4) != Err(E_BADPID) {
        return Err(E_BADPID);
    }
    match QuotaTable::new(0) {
        Err(E_LIMIT0) => {}
        _ => return Err(E_LIMIT0),
    }
    hits += 1;

    // I4：全局满时 A 若有余量也必须落 ENFILE（门序的逆向验证）。
    let mut t2 = QuotaTable::new(2)?;
    t2.register(0x41, 8)?;
    t2.open_batch(0x41, 2)?;
    if t2.open(0x41)? != QuotaVerdict::Enfile {
        return Err(E_NFILE);
    }
    if t2.procs[0].emfile_hits != 0 {
        return Err(E_ACCT);
    }
    hits += 1;

    // C1：两级账守恒终审。
    if !t.audit() || !t2.audit() {
        return Err(E_ACCT);
    }
    if t.granted != t.released + t.global_open {
        return Err(E_ACCT);
    }
    // granted：2+3+2+5 = 12；released：3+5 = 8；global_open = 4
    if t.granted != 12 || t.released != 8 || t.global_open != 4 {
        return Err(E_ACCT);
    }
    hits += 1;

    Ok(hits)
}
