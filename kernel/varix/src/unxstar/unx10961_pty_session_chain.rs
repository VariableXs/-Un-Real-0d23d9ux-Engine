//! UNX-F10961 vim 联测场搭建（pty+会话+控制终端全链）（0x1d7a0 段）
//!
//! 卡片判据（UNX-F10961-J1）：
//! - 联测场全链搭建：openpty+setsid+TIOCSCTTY 全通（10/10），
//!   链路账对平（10/10）。
//!
//! 域：CoRun·UNX-C4-B29。本模块为 vim 联测场搭建纯功能模型：
//! openpty 产生主从对，setsid 让子进程成为新会话首（已是会话首
//! 则 EPERM），TIOCSCTTY 把从端设为控制终端（非会话首或已有控制
//! 终端则拒）。三步全通后链路账闭环。

use crate::alloc::string::String;
use crate::checks::CheckSet;
use alloc::vec::Vec;
use alloc::vec;

// ---------------------------------------------------------------------------
// 错误码（SEG_BASE = 0x1d000 + (10961-10900)*0x20 = 0x1d7a0）
// ---------------------------------------------------------------------------
const SEG_BASE: u32 = 0x1d7a0;

/// PT_EBADF：fd 无效或已关闭
pub const PT_EBADF: u32 = SEG_BASE + 0x01;
/// PT_EPERM：已是会话首仍 setsid，或非会话首设控制终端
pub const PT_EPERM: u32 = SEG_BASE + 0x02;
/// PT_EBUSY：该从端已有控制终端
pub const PT_EBUSY: u32 = SEG_BASE + 0x03;
/// PT_EINVAL：ioctl 参数非法
pub const PT_EINVAL: u32 = SEG_BASE + 0x04;
/// PT_EMFILE：fd 表满
pub const PT_EMFILE: u32 = SEG_BASE + 0x05;
/// PT_ECHAIN：链路账断裂（自检面）
pub const PT_ECHAIN: u32 = SEG_BASE + 0x06;
/// PT_EIO：pty 对端已关闭
pub const PT_EIO: u32 = SEG_BASE + 0x07;

// ---------------------------------------------------------------------------
// 常量
// ---------------------------------------------------------------------------
/// fd 表容量
const FD_CAP: usize = 16;
/// ioctl 命令：设置控制终端
pub const TIOCSCTTY: u32 = 0x540E;
/// ioctl 命令：取窗口尺寸（建模用）
pub const TIOCGWINSZ: u32 = 0x5413;

// ---------------------------------------------------------------------------
// 数据结构
// ---------------------------------------------------------------------------

/// pty 对：主端+从端
#[derive(Clone, Debug, PartialEq)]
pub struct PtyPair {
    /// 主端 fd
    pub master: i32,
    /// 从端 fd
    pub slave: i32,
    /// 对是否仍然开启
    pub alive: bool,
}

impl PtyPair {
    /// FNV 折叠
    pub fn fold(&self) -> u64 {
        let mut h: u64 = 0xcbf29ce484222325;
        h = (h ^ self.master as u64).wrapping_mul(0x100000001b3);
        h = (h ^ self.slave as u64).wrapping_mul(0x100000001b3);
        h = (h ^ (self.alive as u64)).wrapping_mul(0x100000001b3);
        h
    }
}

/// 进程会话态
#[derive(Clone, Debug)]
pub struct ProcState {
    pub pid: u32,
    /// 会话 id（0 表示未 setsid）
    pub sid: u32,
    /// 是否会话首
    pub is_leader: bool,
    /// 控制终端（从端 fd，-1 表示无）
    pub ctty: i32,
}

/// 联测场搭建台
#[derive(Clone, Debug)]
pub struct PtyRig {
    pub pairs: Vec<PtyPair>,
    /// fd -> (对索引, 是否主端)
    fdtab: Vec<Option<(usize, bool)>>,
    pub proc: ProcState,
    /// 链路账：三步各记账
    pub step_openpty: u32,
    pub step_setsid: u32,
    pub step_ctty: u32,
    /// 错误计数
    pub n_eperm: u32,
    pub n_ebusy: u32,
    pub n_ebadf: u32,
}

impl PtyRig {
    pub fn new() -> Self {
        PtyRig {
            pairs: Vec::new(),
            fdtab: vec![None; FD_CAP],
            proc: ProcState { pid: 100, sid: 0, is_leader: false, ctty: -1 },
            step_openpty: 0,
            step_setsid: 0,
            step_ctty: 0,
            n_eperm: 0,
            n_ebusy: 0,
            n_ebadf: 0,
        }
    }

    /// fd 分配
    fn alloc_fd(&mut self, pair: usize, is_master: bool) -> Result<i32, u32> {
        match self.fdtab.iter().position(|s| s.is_none()) {
            Some(i) => {
                self.fdtab[i] = Some((pair, is_master));
                Ok(i as i32)
            }
            None => Err(PT_EMFILE),
        }
    }

    /// openpty：产生主从对
    pub fn openpty(&mut self) -> Result<PtyPair, u32> {
        if self.pairs.len() >= FD_CAP / 2 {
            return Err(PT_EMFILE);
        }
        let m = self.alloc_fd(self.pairs.len(), true)?;
        let s = self.alloc_fd(self.pairs.len(), false)?;
        let p = PtyPair { master: m, slave: s, alive: true };
        self.pairs.push(p.clone());
        self.step_openpty = self.step_openpty.wrapping_add(1);
        Ok(p)
    }

    /// setsid：新会话首；已是会话首 → EPERM
    pub fn setsid(&mut self) -> Result<u32, u32> {
        if self.proc.is_leader {
            self.n_eperm = self.n_eperm.wrapping_add(1);
            return Err(PT_EPERM);
        }
        self.proc.sid = self.proc.pid;
        self.proc.is_leader = true;
        self.step_setsid = self.step_setsid.wrapping_add(1);
        Ok(self.proc.sid)
    }

    /// TIOCSCTTY：把 fd 指向的从端设为控制终端
    pub fn tiocsctty(&mut self, fd: i32) -> Result<(), u32> {
        if !self.fd_ok(fd) {
            self.n_ebadf = self.n_ebadf.wrapping_add(1);
            return Err(PT_EBADF);
        }
        if !self.proc.is_leader {
            self.n_eperm = self.n_eperm.wrapping_add(1);
            return Err(PT_EPERM);
        }
        // fd 必须指向某个从端
        let which = self.fdtab[fd as usize];
        let (pi, is_master) = match which {
            Some(w) => w,
            None => {
                self.n_ebadf = self.n_ebadf.wrapping_add(1);
                return Err(PT_EBADF);
            }
        };
        if is_master {
            return Err(PT_EINVAL);
        }
        if self.proc.ctty != -1 {
            self.n_ebusy = self.n_ebusy.wrapping_add(1);
            return Err(PT_EBUSY);
        }
        if !self.pairs[pi].alive {
            return Err(PT_EIO);
        }
        self.proc.ctty = fd;
        self.step_ctty = self.step_ctty.wrapping_add(1);
        Ok(())
    }

    /// fd 有效性
    pub fn fd_ok(&self, fd: i32) -> bool {
        if fd < 0 || fd as usize >= FD_CAP {
            return false;
        }
        self.fdtab[fd as usize].is_some()
    }

    /// 全链判定：openpty + setsid + ctty 三步全通且账对平
    pub fn chain_ok(&self) -> bool {
        self.step_openpty > 0
            && self.step_openpty == self.step_setsid
            && self.step_setsid == self.step_ctty
            && self.proc.is_leader
            && self.proc.ctty >= 0
    }

    /// 关闭 fd（对端一起失效面）
    pub fn close(&mut self, fd: i32) -> Result<(), u32> {
        if !self.fd_ok(fd) {
            self.n_ebadf = self.n_ebadf.wrapping_add(1);
            return Err(PT_EBADF);
        }
        let (pi, _) = self.fdtab[fd as usize].unwrap_or((0, true));
        self.fdtab[fd as usize] = None;
        // 从端关闭则对失效；主端关闭亦然（联测场终局）
        if self.proc.ctty == fd {
            self.proc.ctty = -1;
        }
        self.pairs[pi].alive = false;
        Ok(())
    }

    /// 主端写从端读（联测通路）
    pub fn passthru(&mut self, data: u32) -> Result<u32, u32> {
        let p = match self.pairs.last() {
            Some(p) if p.alive => p.clone(),
            _ => return Err(PT_EIO),
        };
        if !self.fd_ok(p.master) || !self.fd_ok(p.slave) {
            return Err(PT_EBADF);
        }
        Ok(data ^ 0xa5a5a5a5)
    }

    /// 账目对平
    pub fn ledger_ok(&self, expect_openpty: u32) -> bool {
        self.step_openpty == expect_openpty
            && self.step_setsid <= self.step_openpty
            && self.step_ctty <= self.step_setsid
    }

    /// 总折叠
    pub fn fold_all(&self) -> u64 {
        let mut h: u64 = 0xcbf29ce484222325;
        for p in &self.pairs {
            h = (h ^ p.fold()).wrapping_mul(0x100000001b3);
        }
        h = (h ^ self.proc.sid as u64).wrapping_mul(0x100000001b3);
        h = (h ^ self.step_openpty as u64).wrapping_mul(0x100000001b3);
        h = (h ^ self.step_setsid as u64).wrapping_mul(0x100000001b3);
        h = (h ^ self.step_ctty as u64).wrapping_mul(0x100000001b3);
        h
    }

    /// 渲染
    pub fn render(&self) -> String {
        crate::alloc::format!(
            "ptyrig:pairs={} sid={} ctty={} chain={}",
            self.pairs.len(),
            self.proc.sid,
            self.proc.ctty,
            self.chain_ok()
        )
    }
}

impl Default for PtyRig {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// 注入四路
// ---------------------------------------------------------------------------
pub fn inject_four() -> [bool; 4] {
    let mut r = PtyRig::new();
    let _ = r.openpty();
    // 路1：未 setsid 就设控制终端 → EPERM
    let r1 = matches!(r.tiocsctty(1), Err(x) if x == PT_EPERM);
    // 路2：setsid 后重复 setsid → EPERM
    let _ = r.setsid();
    let r2 = matches!(r.setsid(), Err(x) if x == PT_EPERM);
    // 路3：坏 fd 设控制终端 → EBADF
    let r3 = matches!(r.tiocsctty(50), Err(x) if x == PT_EBADF);
    // 路4：主端设控制终端 → EINVAL
    let r4 = matches!(r.tiocsctty(0), Err(x) if x == PT_EINVAL);
    [r1, r2, r3, r4]
}

// ---------------------------------------------------------------------------
// 确定性千轮样本
// ---------------------------------------------------------------------------
/// 千轮：每 10 轮一全链（openpty+setsid+ctty），全通记 1 轮。
pub fn sample_thousand() -> u32 {
    let mut good: u32 = 0;
    for r in 0..1000u32 {
        let mut rig = PtyRig::new();
        let p = match rig.openpty() { Ok(p) => p, Err(_) => continue };
        let s1 = rig.setsid().is_ok();
        let s2 = rig.tiocsctty(p.slave).is_ok();
        // 链路账对平：三步计数一致
        let ledger = rig.ledger_ok(1);
        // 重复步必须被拒
        let dup_rej = matches!(rig.setsid(), Err(x) if x == PT_EPERM)
            && matches!(rig.tiocsctty(p.slave), Err(x) if x == PT_EBUSY);
        // 全链通路可用
        let path = rig.passthru(r).is_ok();
        if s1 && s2 && ledger && dup_rej && path && rig.chain_ok() {
            good = good.wrapping_add(1);
        }
    }
    good
}

// ---------------------------------------------------------------------------
// 判据检查
// ---------------------------------------------------------------------------
pub fn run_unx10961_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F10961");

    let inj = inject_four();
    s.add("J1-注入四路全拦截", inj.iter().all(|&b| b), "EPERM/EBADF/EINVAL 坏样本全部命中");

    // J1 全链搭建 10/10
    let mut full10 = 0u32;
    for _ in 0..10 {
        let mut r = PtyRig::new();
        let p = match r.openpty() { Ok(p) => p, Err(_) => break };
        let a = r.setsid().is_ok();
        let b = r.tiocsctty(p.slave).is_ok();
        if a && b && r.chain_ok() {
            full10 = full10.wrapping_add(1);
        }
    }
    s.add(
        "J1-全链搭建10/10",
        full10 == 10,
        "openpty+setsid+TIOCSCTTY 十轮全通",
    );

    // 链路账对平 10/10
    let mut led10 = 0u32;
    for _ in 0..10 {
        let mut r = PtyRig::new();
        let _ = r.openpty();
        let _ = r.setsid();
        let _ = r.tiocsctty(1);
        if r.ledger_ok(1) && r.step_openpty == r.step_setsid && r.step_setsid == r.step_ctty {
            led10 = led10.wrapping_add(1);
        }
    }
    s.add("J2-链路账对平10/10", led10 == 10, "三步账逐位相等");

    // 千轮样本
    let good = sample_thousand();
    s.add("J3-千轮全链", good == 1000, "千轮全链搭建+重复步拒绝+通路全通");

    // 错误码分段
    let codes = [PT_EBADF, PT_EPERM, PT_EBUSY, PT_EINVAL, PT_EMFILE, PT_ECHAIN, PT_EIO];
    let seg_ok = codes.iter().all(|&c| c > SEG_BASE && c < SEG_BASE + 0x20);
    s.add("J4-错误码分段", seg_ok, "七码全部落在 0x1d7a0 段内");

    // 错误码互异
    let mut distinct = true;
    for i in 0..codes.len() {
        for j in (i + 1)..codes.len() {
            if codes[i] == codes[j] {
                distinct = false;
            }
        }
    }
    s.add("J5-错误码互异", distinct, "七码两两互异");

    // 主从对账
    let mut r2 = PtyRig::new();
    let p2 = match r2.openpty() { Ok(p) => p, Err(_) => PtyPair { master: -1, slave: -1, alive: false } };
    s.add(
        "J6-主从对账",
        p2.master >= 0 && p2.slave >= 0 && p2.master != p2.slave && r2.pairs.len() == 1,
        "一对 fd 两个不同槽位",
    );

    // 账目对平
    s.add("J7-账目对平", r2.ledger_ok(1) && r2.step_openpty == 1, "openpty 账与期望一致");

    // 关闭后对失效
    let _ = r2.close(p2.slave);
    let dead = !r2.pairs[0].alive && matches!(r2.passthru(1), Err(x) if x == PT_EIO);
    s.add("J8-关闭面", dead, "从端关闭后通路报 EIO 失效");

    // 判定序：EBADF 先于 EPERM
    let mut r3 = PtyRig::new();
    let seq_ok = matches!(r3.tiocsctty(80), Err(x) if x == PT_EBADF);
    s.add("J9-判定序", seq_ok, "坏 fd 先于会话首判定");

    // fold 稳定
    let h1 = r3.fold_all();
    let h2 = r3.fold_all();
    s.add("J10-fold稳定", h1 == h2, "同一状态两次折叠一致");

    // 渲染
    let r_txt = r3.render();
    s.add(
        "J11-渲染非空",
        r_txt.starts_with("ptyrig:") && r_txt.contains("pairs="),
        "摘要含场内状态",
    );

    // passthru 确定性
    let mut r4 = PtyRig::new();
    let _ = r4.openpty();
    let a = r4.passthru(0x1234);
    let b = r4.passthru(0x1234);
    s.add("J12-通路确定性", a == b && a.is_ok(), "同输入两次通路结果一致");

    // 动态判据
    s.add_dyn(
        "J13-全链账",
        r3.step_openpty == 0 && r3.proc.ctty == -1,
        &crate::alloc::format!("openpty={} setsid={} ctty={}", r3.step_openpty, r3.step_setsid, r3.step_ctty),
    );
    s.add_dyn(
        "J14-千轮通过率",
        good == 1000,
        &crate::alloc::format!("{}//1000", good),
    );
    s.add_dyn(
        "J15-注入计数",
        r3.n_ebadf == 1,
        &crate::alloc::format!("EBADF={}", r3.n_ebadf),
    );

    s
}

// ---------------------------------------------------------------------------
// 测试
// ---------------------------------------------------------------------------
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx10961_pty_session_chain_flow() {
        let mut r = PtyRig::new();
        let p = r.openpty().unwrap();
        assert!(r.setsid().is_ok());
        assert!(r.tiocsctty(p.slave).is_ok());
        assert!(r.chain_ok());
        assert!(matches!(r.tiocsctty(p.slave), Err(x) if x == PT_EBUSY));
        assert!(matches!(r.setsid(), Err(x) if x == PT_EPERM));
        let inj = inject_four();
        assert!(inj.iter().all(|&b| b));
        assert_eq!(sample_thousand(), 1000);
    }

    #[test]
    fn unx10961_checks_green() {
        let s = run_unx10961_checks();
        assert!(s.all_passed(), "UNX-F10961 判据未全绿");
    }
}
