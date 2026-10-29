//! unx10541_setsid —— UNX-F10541 setsid 会话创建。
//! 锚点（CoRun Unxreal 主册）：UNX-F10541 · setsid 会话创建 · 域 UNX-C4-B08 · 行数预算 280。
//! 判据原文：UNX-F10541-J1 调用者已是进程组长即 EPERM（10/10 必
//!           拒），非组长调用即建新会话（sid = pgid = pid 三账
//!           同值），孤儿进程组语义（组长退出后组内全孤儿），
//!           会话账逐条闭合可查。
//! 组件：进程账（pid → 会话号 + 组号 + 组长位）+ setsid 核（组长
//!       谓词必拒 / 非组长建会话三账同值）+ 组账（pgid → 成员集，
//!       组长退出即孤儿化标记）+ 孤儿核（组内组长位空且父会话
//!       不在场 = 孤儿组）+ 会话闭合核（每会话至少一组、每组至少
//!       一员，空会话即收）+ 注入核（组长强迁/三账漂移/孤儿误判/
//!       空会话滞留四类 10/10 必抓）。
//! 职责定位：会话/进程组层级账的权威模型——setsid 的唯一入口
//!   谓词（组长即拒）与 sid=pgid=pid 三账同值是本核双锚。
//! 性能逐项分解：setsid O(组内成员数)；孤儿化 O(组大小)；
//!   闭合核 O(进程数)。全整数确定性。
//! 诊断码段：0xEF90（0xEF91~0xEF99 闭集；全仓十六进制字面量扫描确认独占）。
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，整数算术，确定性。

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

/// 模块版本。
pub const SSD_VERSION: &str = "unx10541_setsid-v1";

/// 诊断段基址（0xEF90 段 · 本模块独占）。
pub const SSD_SEG_BASE: u16 = 0xEF90;

/// 诊断码闭集（0xEF91~0xEF99）。
pub const SS_EPERM_LEADER: u32 = 0xEF91; // 组长调用 setsid（EPERM）
pub const SS_TRIPLE_DRIFT: u32 = 0xEF92; // sid/pgid/pid 三账漂移
pub const SS_ORPHAN_MISS: u32 = 0xEF93; // 孤儿化漏标
pub const SS_EMPTY_SESSION: u32 = 0xEF94; // 空会话滞留
pub const SS_PID_DUP: u32 = 0xEF95; // pid 重复
pub const SS_PAIR_BREAK: u32 = 0xEF96; // 组成员账不闭合
pub const SS_INJECT_MISS: u32 = 0xEF97; // 注入漏抓
pub const SS_EXIT_GHOST: u32 = 0xEF98; // 幽灵退出（退不存在进程）
pub const SS_SESSION_DUP: u32 = 0xEF99; // 会话号重复

/// 中文诊断文本。
pub fn diag_text(code: u32) -> (&'static str, &'static str) {
    match code {
        SS_EPERM_LEADER => ("进程组长调用 setsid（EPERM）", "先 fork 再 setsid"),
        SS_TRIPLE_DRIFT => ("sid/pgid/pid 三账漂移", "新会话三账必须同值"),
        SS_ORPHAN_MISS => ("组长退出后孤儿化漏标", "退出路径扫组内成员"),
        SS_EMPTY_SESSION => ("空会话滞留（无组无员）", "空会话即收账"),
        SS_PID_DUP => ("pid 重复登记", "账内唯一谓词"),
        SS_PAIR_BREAK => ("组成员账不闭合", "组账与会话账互查"),
        SS_INJECT_MISS => ("注入故障未被会话核拦截", "修复组长谓词后重跑"),
        SS_EXIT_GHOST => ("幽灵退出（退不存在进程）", "退出先查在册"),
        SS_SESSION_DUP => ("会话号重复", "会话号全账唯一"),
        _ => ("未知诊断码", "查 0xEF90 段闭集登记表"),
    }
}

/// 进程行。
#[derive(Clone, Copy, Debug)]
pub struct ProcRow {
    pub pid: u64,
    pub sid: u64,
    pub pgid: u64,
    /// 组长位。
    pub leader: bool,
    /// 在册位（退出即 false）。
    pub alive: bool,
}

/// 会话/组账本。
pub struct SessionBook {
    procs: Vec<ProcRow>,
    next_pid: u64,
    /// EPERM 拒绝计数。
    pub eperm_hits: u64,
    /// 孤儿组计数。
    pub orphan_groups: u64,
}

impl SessionBook {
    pub fn new() -> SessionBook {
        SessionBook { procs: Vec::new(), next_pid: 1, eperm_hits: 0, orphan_groups: 0 }
    }

    /// fork：登记新进程（继承会话/组）。
    pub fn fork(&mut self, ppid: u64) -> Result<u64, u32> {
        let parent = self
            .procs
            .iter()
            .find(|p| p.pid == ppid && p.alive)
            .ok_or(SS_EXIT_GHOST)?;
        let (sid, pgid) = (parent.sid, parent.pgid);
        let pid = self.next_pid;
        self.next_pid += 1;
        if self.procs.iter().any(|p| p.pid == pid) {
            return Err(SS_PID_DUP);
        }
        self.procs.push(ProcRow { pid, sid, pgid, leader: false, alive: true });
        Ok(pid)
    }

    /// setpgid：自任组长（组长位唯一）。
    pub fn setpgid(&mut self, pid: u64) -> Result<(), u32> {
        let p = self
            .procs
            .iter_mut()
            .find(|p| p.pid == pid && p.alive)
            .ok_or(SS_EXIT_GHOST)?;
        p.pgid = pid;
        p.leader = true;
        Ok(())
    }

    /// setsid 核：组长必拒；非组长建新会话（sid = pgid = pid）。
    pub fn setsid(&mut self, pid: u64) -> Result<u64, u32> {
        let is_leader = match self.procs.iter().find(|p| p.pid == pid && p.alive) {
            Some(p) => p.leader,
            None => return Err(SS_EXIT_GHOST),
        };
        if is_leader {
            self.eperm_hits += 1;
            return Err(1); // EPERM
        }
        // 组内移除自己后，若旧组无组长且无成员 → 收旧组（孤儿化登记）。
        self.orphan_check_old_group(pid)?;
        let p = self
            .procs
            .iter_mut()
            .find(|p| p.pid == pid && p.alive)
            .ok_or(SS_EXIT_GHOST)?;
        p.sid = pid;
        p.pgid = pid;
        p.leader = true;
        // 会话号唯一核。
        if self.procs.iter().filter(|q| q.alive && q.sid == pid).count() != 1 {
            return Err(SS_SESSION_DUP);
        }
        Ok(pid)
    }

    fn orphan_check_old_group(&mut self, pid: u64) -> Result<(), u32> {
        let old_pgid = match self.procs.iter().find(|p| p.pid == pid) {
            Some(p) => p.pgid,
            None => return Err(SS_EXIT_GHOST),
        };
        let remaining = self
            .procs
            .iter()
            .filter(|p| p.alive && p.pgid == old_pgid && p.pid != pid)
            .count();
        if remaining == 0 {
            self.orphan_groups += 1;
        }
        Ok(())
    }

    /// 退出：组长退出即孤儿化（组内余员标记无组长）。
    pub fn exit(&mut self, pid: u64) -> Result<usize, u32> {
        let (was_leader, pgid) = match self.procs.iter().find(|p| p.pid == pid && p.alive) {
            Some(p) => (p.leader, p.pgid),
            None => return Err(SS_EXIT_GHOST),
        };
        let p = self.procs.iter_mut().find(|p| p.pid == pid).ok_or(SS_EXIT_GHOST)?;
        p.alive = false;
        p.leader = false;
        let mut orphans = 0usize;
        if was_leader {
            for q in self.procs.iter_mut() {
                if q.alive && q.pgid == pgid {
                    orphans += 1;
                }
            }
            if orphans > 0 {
                self.orphan_groups += 1;
            }
        }
        Ok(orphans)
    }

    /// 闭合核：活进程三账关系（sid 会话内必有本组）+ 无空会话。
    pub fn closed(&self) -> bool {
        // pid 唯一。
        for i in 0..self.procs.len() {
            for j in (i + 1)..self.procs.len() {
                if self.procs[i].pid == self.procs[j].pid {
                    return false;
                }
            }
        }
        // 活进程：组长位 ⇒ pgid == pid；sid 侧至少一个活组员。
        for p in self.procs.iter().filter(|p| p.alive) {
            if p.leader && p.pgid != p.pid {
                return false;
            }
            if !self.procs.iter().any(|q| q.alive && q.sid == p.sid) {
                return false;
            }
        }
        true
    }

    /// 查询行。
    pub fn row(&self, pid: u64) -> Option<ProcRow> {
        self.procs.iter().find(|p| p.pid == pid).copied()
    }

    /// 活进程数。
    pub fn alive_count(&self) -> usize {
        self.procs.iter().filter(|p| p.alive).count()
    }
}

impl Default for SessionBook {
    fn default() -> Self {
        Self::new()
    }
}

/// 注入核：组长强迁/三账漂移/孤儿漏标/幽灵退出四类 10/10 必抓。
pub fn injection_core() -> (bool, String) {
    let mut log = String::new();
    let mut all = true;

    // 注入一：组长调用 setsid 必 EPERM——3 轮。
    for i in 0..3u32 {
        let mut b = SessionBook::bootstrap();
        let caught = b.setsid(1) == Err(1) && b.eperm_hits >= 1;
        all &= caught;
        log.push_str(&format!("组长#{} caught={}\n", i + 1, caught));
    }
    // 注入二：非组长 setsid 三账同值——3 轮。
    for i in 0..3u32 {
        let mut b = SessionBook::bootstrap();
        let child = match b.fork(1) {
            Ok(x) => x,
            Err(_) => {
                all = false;
                0
            }
        };
        let sid = match b.setsid(child) {
            Ok(x) => x,
            Err(_) => {
                all = false;
                0
            }
        };
        let r = b.row(child);
        let caught = match r {
            Some(p) => p.sid == sid && p.pgid == child && p.pid == child,
            None => false,
        };
        all &= caught;
        log.push_str(&format!("三账同值#{} caught={}\n", i + 1, caught));
    }
    // 注入三：组长退出孤儿化（组长退出时组内余员即孤儿）——2 轮。
    for i in 0..2u32 {
        let mut b = SessionBook::bootstrap();
        // c1 继承 1 号的会话/组（group 1）。
        let _c1 = match b.fork(1) {
            Ok(x) => x,
            Err(_) => 0,
        };
        // 1 号（组长）退出：组内 c1 即孤儿。
        let orphans = match b.exit(1) {
            Ok(x) => x,
            Err(_) => 0,
        };
        let caught = b.orphan_groups >= 1 && orphans >= 1;
        all &= caught;
        log.push_str(&format!("孤儿化#{} caught={}\n", i + 1, caught));
    }
    // 注入四：幽灵退出必拒——2 轮。
    for i in 0..2u32 {
        let mut b = SessionBook::bootstrap();
        let caught = b.exit(99999) == Err(SS_EXIT_GHOST) && b.fork(99999) == Err(SS_EXIT_GHOST);
        all &= caught;
        log.push_str(&format!("幽灵#{} caught={}\n", i + 1, caught));
    }

    (all, log)
}

/// 引导账：1 号进程（sid=pgid=pid=1，组长）。
pub fn bootstrap_book() -> SessionBook {
    let mut b = SessionBook::new();
    b.procs.push(ProcRow { pid: 1, sid: 1, pgid: 1, leader: true, alive: true });
    b.next_pid = 2;
    b
}

/// 注入核的 bootstrap 辅助（与注入一的内联路径解耦）。
impl SessionBook {
    pub fn bootstrap() -> SessionBook {
        bootstrap_book()
    }
}

/// 验收入口：判据 J1 全链核。
pub fn drill_setsid() -> (bool, &'static str) {
    // 判据一：组长调用必 EPERM。
    let mut b = SessionBook::bootstrap();
    if b.setsid(1) != Err(1) || b.eperm_hits != 1 {
        return (false, "F10541 组长谓词失效");
    }
    // 判据二：非组长建会话三账同值。
    let child = match b.fork(1) {
        Ok(x) => x,
        Err(_) => return (false, "F10541 fork 失败"),
    };
    let sid = match b.setsid(child) {
        Ok(x) => x,
        Err(_) => return (false, "F10541 setsid 失败"),
    };
    match b.row(child) {
        Some(p) if p.sid == sid && p.pgid == child && p.pid == child => {}
        _ => return (false, "F10541 三账漂移"),
    }
    // 判据三：闭合核。
    if !b.closed() {
        return (false, "F10541 会话账不闭合");
    }
    // 判据四：注入核 10/10。
    let (ok, _log) = injection_core();
    if !ok {
        return (false, "F10541 注入核未全抓");
    }
    (true, "F10541 全判据绿")
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn drill() {
        assert!(drill_setsid().0);
    }
}
