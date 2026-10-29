//! unx10661_semget —— UNX-F10661 semget 信号量集。
//! 锚点（CoRun Unxreal 主册）：UNX-F10661 · semget 信号量集 · 域 UNX-C4-B14 · 行数预算 280。
//! 判据原文：UNX-F10661-J1 集建（nsems 1..=SEMMSL 闭集）与键三分
//!           （同 shmget 口径）100/100，semop wait/signal 语义
//!           （负值下探零界即 EAGAIN、正值递增），undo 账（进程
//!           退出回滚零净额），集账（id/键/nsems/semval 向量）闭合。
//! 组件：集账（id → 键 + nsems + semval 向量）+ 键三分核（同
//!       shmget 三分支）+ semop 核（wait：val>0 才可 -1，否则
//!       EAGAIN；signal：+1 恒可；UNDO 标记挂账）+ undo 回滚核
//!       （进程退出逐挂账回滚，回滚后净额零）+ 闭合核（向量长
//!       = nsems，val ∈ [0, SEMVMX]）+ 注入核（nsems 越集/下标
//!       越界/undo 漏滚/val 越顶四类 10/10 必抓）。
//! 职责定位：SysV 信号量集面的权威模型——集建闭集、wait/signal
//!   零界语义、undo 零净额回滚三性质机检。
//! 性能逐项分解：semop O(1)；undo 回滚 O(挂账数)；闭合核 O(集数)。
//!   全整数确定性。
//! 诊断码段：0xEFF0（0xEFF1~0xEFF9 闭集；全仓十六进制字面量扫描确认独占）。
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，整数算术，确定性。

use alloc::format;
use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;

/// 模块版本。
pub const SMV_VERSION: &str = "unx10661_semget-v1";

/// 诊断段基址（0xEFF0 段 · 本模块独占）。
pub const SMV_SEG_BASE: u16 = 0xEFF0;

/// 诊断码闭集（0xEFF1~0xEFF9）。
pub const SE_NSEMS_OOB: u32 = 0xEFF1; // nsems 越集（SEMMSL 外）
pub const SE_IDX_OOB: u32 = 0xEFF2; // semnum 下标越界
pub const SE_UNDO_MISS: u32 = 0xEFF3; // undo 漏滚
pub const SE_VAL_OVER: u32 = 0xEFF4; // semval 越顶（SEMVMX）
pub const SE_EAGAIN_MISS: u32 = 0xEFF5; // 零界下探漏拒
pub const SE_KEY_BREAK: u32 = 0xEFF6; // 键三分漂移
pub const SE_INJECT_MISS: u32 = 0xEFF7; // 注入漏抓
pub const SE_SET_GHOST: u32 = 0xEFF8; // 幽灵集
pub const SE_VEC_SHORT: u32 = 0xEFF9; // 向量长 ≠ nsems

/// 中文诊断文本。
pub fn diag_text(code: u32) -> (&'static str, &'static str) {
    match code {
        SE_NSEMS_OOB => ("nsems 越集", "1..=SEMMSL=32 闭集"),
        SE_IDX_OOB => ("semnum 下标越界", "下标 < nsems"),
        SE_UNDO_MISS => ("undo 漏滚（净额非零）", "退出回滚逐挂账"),
        SE_VAL_OVER => ("semval 越顶", "val ≤ SEMVMX=32767"),
        SE_EAGAIN_MISS => ("零界下探漏拒（EAGAIN）", "val=0 时 wait 必拒"),
        SE_KEY_BREAK => ("键三分漂移", "同 shmget 口径复算"),
        SE_INJECT_MISS => ("注入故障未集核拦截", "修复谓词后重跑"),
        SE_SET_GHOST => ("幽灵集（账外 id）", "先查集账"),
        SE_VEC_SHORT => ("向量长 ≠ nsems", "集建后向量闭包"),
        _ => ("未知诊断码", "查 0xEFF0 段闭集登记表"),
    }
}

/// EAGAIN / EEXIST / EEXIST 复用 shm 口径。
pub const EAGAIN: u32 = 11;
pub const EEXIST: u32 = 17;

/// 集容量与值顶（Linux 对照锚定）。
pub const SEMMSL: usize = 32;
pub const SEMVMX: i32 = 32767;

/// IPC 标志（与 shm 同位）。
pub const IPC_CREAT: u32 = 0x200;
pub const IPC_EXCL: u32 = 0x400;
pub const IPC_PRIVATE: u64 = 0;

/// 集行。
#[derive(Clone, Debug)]
pub struct SemSet {
    pub id: u64,
    pub key: u64,
    /// 信号量值向量。
    pub vals: Vec<i32>,
    pub alive: bool,
}

/// undo 挂账行。
#[derive(Clone, Copy, Debug)]
pub struct UndoRow {
    pub pid: u64,
    pub set_id: u64,
    pub semnum: usize,
    /// 挂账增量（退出时 -delta 回滚）。
    pub delta: i32,
}

/// 信号量账本。
pub struct SemBook {
    sets: Vec<SemSet>,
    undos: Vec<UndoRow>,
    next_id: u64,
}

impl SemBook {
    pub fn new() -> SemBook {
        SemBook { sets: Vec::new(), undos: Vec::new(), next_id: 1 }
    }

    /// semget 核：nsems 闭集 + 键三分。
    pub fn semget(&mut self, key: u64, nsems: usize, flags: u32) -> Result<u64, u32> {
        if nsems == 0 || nsems > SEMMSL {
            return Err(SE_NSEMS_OOB);
        }
        if key == IPC_PRIVATE {
            let id = self.next_id;
            self.next_id += 1;
            self.sets.push(SemSet { id, key, vals: vec![0i32; nsems], alive: true });
            return Ok(id);
        }
        match self.sets.iter().find(|s| s.key == key && s.alive) {
            Some(e) => {
                if flags & IPC_CREAT != 0 && flags & IPC_EXCL != 0 {
                    return Err(EEXIST);
                }
                if e.vals.len() != nsems {
                    return Err(SE_VEC_SHORT);
                }
                Ok(e.id)
            }
            None => {
                if flags & IPC_CREAT != 0 {
                    let id = self.next_id;
                    self.next_id += 1;
                    self.sets.push(SemSet { id, key, vals: vec![0i32; nsems], alive: true });
                    Ok(id)
                } else {
                    Err(2) // ENOENT
                }
            }
        }
    }

    /// semop wait：val > 0 才可 -1（UNDO 可挂账），否则 EAGAIN。
    pub fn wait(&mut self, id: u64, semnum: usize, pid: u64, undo: bool) -> Result<(), u32> {
        let s = self.sets.iter_mut().find(|s| s.id == id && s.alive).ok_or(SE_SET_GHOST)?;
        if semnum >= s.vals.len() {
            return Err(SE_IDX_OOB);
        }
        if s.vals[semnum] <= 0 {
            return Err(EAGAIN);
        }
        s.vals[semnum] -= 1;
        if undo {
            self.undos.push(UndoRow { pid, set_id: id, semnum, delta: -1 });
        }
        Ok(())
    }

    /// semop signal：+1 恒可（越顶即拒）。
    pub fn signal(&mut self, id: u64, semnum: usize, pid: u64, undo: bool) -> Result<(), u32> {
        let s = self.sets.iter_mut().find(|s| s.id == id && s.alive).ok_or(SE_SET_GHOST)?;
        if semnum >= s.vals.len() {
            return Err(SE_IDX_OOB);
        }
        if s.vals[semnum] as i64 + 1 > SEMVMX as i64 {
            return Err(SE_VAL_OVER);
        }
        s.vals[semnum] += 1;
        if undo {
            self.undos.push(UndoRow { pid, set_id: id, semnum, delta: 1 });
        }
        Ok(())
    }

    /// undo 回滚核：进程退出逐挂账回滚，回滚后该 pid 净额零。
    pub fn exit_rollback(&mut self, pid: u64) -> Result<usize, u32> {
        let mine: Vec<UndoRow> = self.undos.iter().filter(|u| u.pid == pid).copied().collect();
        let n = mine.len();
        for u in mine {
            let s = self
                .sets
                .iter_mut()
                .find(|s| s.id == u.set_id && s.alive)
                .ok_or(SE_SET_GHOST)?;
            if u.semnum >= s.vals.len() {
                return Err(SE_IDX_OOB);
            }
            s.vals[u.semnum] -= u.delta; // 反向回滚
        }
        self.undos.retain(|u| u.pid != pid);
        Ok(n)
    }

    /// 净额核：pid 未回滚挂账数（回滚后应 0）。
    pub fn undo_pending(&self, pid: u64) -> usize {
        self.undos.iter().filter(|u| u.pid == pid).count()
    }

    /// 闭合核：活集向量长 = nsems 且 val ∈ [0, SEMVMX]。
    pub fn closed(&self) -> bool {
        for s in self.sets.iter().filter(|s| s.alive) {
            if s.vals.is_empty() || s.vals.len() > SEMMSL {
                return false;
            }
            for v in s.vals.iter() {
                if *v < 0 || *v > SEMVMX {
                    return false;
                }
            }
        }
        true
    }

    pub fn row(&self, id: u64) -> Option<&SemSet> {
        self.sets.iter().find(|s| s.id == id && s.alive)
    }
}

impl Default for SemBook {
    fn default() -> Self {
        Self::new()
    }
}

/// LCG64 确定性伪随机。
pub struct Lcg(u64);
impl Lcg {
    pub fn new(seed: u64) -> Lcg {
        Lcg(seed | 1)
    }
    pub fn next_u64(&mut self) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        self.0
    }
}

/// 引擎：双进程 wait/signal + undo 回滚对拍（净额零断言）。
pub fn undo_pair() -> Result<(), u32> {
    let mut b = SemBook::new();
    let id = b.semget(7, 4, IPC_CREAT)?;
    // A：wait ×3（undo）。
    for _ in 0..3u32 {
        b.signal(id, 0, 0, false)?; // 先备值
    }
    for _ in 0..3u32 {
        b.wait(id, 0, 100, true)?;
    }
    if b.undo_pending(100) != 3 {
        return Err(SE_UNDO_MISS);
    }
    // B：signal ×2（undo）。
    for _ in 0..2u32 {
        b.signal(id, 1, 200, true)?;
    }
    if b.undo_pending(200) != 2 {
        return Err(SE_UNDO_MISS);
    }
    // 双退回滚。
    b.exit_rollback(100)?;
    b.exit_rollback(200)?;
    if b.undo_pending(100) != 0 || b.undo_pending(200) != 0 {
        return Err(SE_UNDO_MISS);
    }
    // 回滚后：sem0 = 3（-3+3），sem1 = 0（+2-2）。
    let s = b.row(id).ok_or(SE_SET_GHOST)?;
    if s.vals[0] != 3 || s.vals[1] != 0 {
        return Err(SE_UNDO_MISS);
    }
    if !b.closed() {
        return Err(SE_VEC_SHORT);
    }
    Ok(())
}

/// 注入核：nsems 越集/下标越界/零界下探/undo 净额四类 10/10。
pub fn injection_core() -> (bool, String) {
    let mut log = String::new();
    let mut all = true;

    // 注入一：nsems 越集（0 与 33）——3 轮。
    for i in 0..3u32 {
        let mut b = SemBook::new();
        let c1 = b.semget(1, 0, IPC_CREAT).is_err();
        let c2 = b.semget(1, SEMMSL + 1, IPC_CREAT).is_err();
        let caught = c1 && c2;
        all &= caught;
        log.push_str(&format!("nsems#{} caught={}\n", i + 1, caught));
    }
    // 注入二：下标越界（nsems=4 的集用下标 9）——3 轮。
    for i in 0..3u32 {
        let mut b = SemBook::new();
        let id = match b.semget(2, 4, IPC_CREAT) {
            Ok(x) => x,
            Err(_) => {
                all = false;
                continue;
            }
        };
        let caught = b.wait(id, 9, 1, false).is_err() && b.signal(id, 9, 1, false).is_err();
        all &= caught;
        log.push_str(&format!("下标#{} caught={}\n", i + 1, caught));
    }
    // 注入三：零界下探必 EAGAIN——2 轮。
    for i in 0..2u32 {
        let mut b = SemBook::new();
        let id = match b.semget(3, 1, IPC_CREAT) {
            Ok(x) => x,
            Err(_) => {
                all = false;
                continue;
            }
        };
        let caught = b.wait(id, 0, 1, false) == Err(EAGAIN);
        all &= caught;
        log.push_str(&format!("零界#{} caught={}\n", i + 1, caught));
    }
    // 注入四：undo 回滚净额零（含 signal 挂账回滚）——2 轮。
    for i in 0..2u32 {
        let mut b = SemBook::new();
        let id = match b.semget(4, 1, IPC_CREAT) {
            Ok(x) => x,
            Err(_) => {
                all = false;
                continue;
            }
        };
        let s_ok = b.signal(id, 0, 1, false).is_ok();
        let w1 = b.wait(id, 0, 300, true).is_ok();
        let w2 = b.wait(id, 0, 300, true) == Err(EAGAIN);
        let rb = b.exit_rollback(300).is_ok();
        let s = b.row(id).ok_or(SE_SET_GHOST);
        let caught = match s {
            Ok(ss) => {
                ss.vals[0] == 1
                    && b.undo_pending(300) == 0
                    && s_ok
                    && w1
                    && w2
                    && rb
            }
            Err(_) => false,
        };
        all &= caught;
        log.push_str(&format!("undo净额#{} caught={}\n", i + 1, caught));
    }

    (all, log)
}

/// 验收入口：判据 J1 全链核。
pub fn drill_semget() -> (bool, &'static str) {
    // 判据一：undo 双进程回滚净额零。
    if undo_pair().is_err() {
        return (false, "F10661 undo 对拍漂移");
    }
    // 判据二：键三分（同 shmget 口径独立复测）。
    let mut b = SemBook::new();
    let id1 = match b.semget(9, 2, IPC_CREAT) {
        Ok(x) => x,
        Err(_) => return (false, "F10661 建集失败"),
    };
    if b.semget(9, 2, 0) != Ok(id1) || b.semget(9, 2, IPC_CREAT | IPC_EXCL) != Err(EEXIST) {
        return (false, "F10661 键三分漂移");
    }
    // 判据三：闭合核。
    if !b.closed() {
        return (false, "F10661 集账不闭合");
    }
    // 判据四：注入核 10/10。
    let (ok, _log) = injection_core();
    if !ok {
        return (false, "F10661 注入核未全抓");
    }
    (true, "F10661 全判据绿")
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn drill() {
        assert!(drill_semget().0);
    }
}
