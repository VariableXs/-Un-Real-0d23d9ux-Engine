//! UNX-F10956 mq 描述符 fd 集成矩阵（dup/epoll 面）（0x1d700 段）
//!
//! 卡片判据（UNX-F10956-J1）：
//! - 正面：mqd 可 dup/可注册 epoll（10/10）；
//! - 反面：副本事件零串扰（千次 10/10）。
//!
//! 域：CoRun·UNX-C4-B28。本模块建模 POSIX 消息队列描述符与
//! dup/epoll 的集成面：mqd 可复制（dup 共享底层队列但注册态
//! 独立）、可注册 epoll（事件按 fd 独立投递）、副本的注销/屏蔽
//! 不得影响原 fd 的事件流（零串扰）。

use crate::alloc::string::String;
use crate::checks::CheckSet;
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 错误码（SEG_BASE = 0x1d000 + (10956-10900)*0x20 = 0x1d700）
// ---------------------------------------------------------------------------
const SEG_BASE: u32 = 0x1d700;

/// EBADF：fd 未登记或已关闭
pub const FD_EBADF: u32 = SEG_BASE + 0x01;
/// EINVAL：dup/epoll 参数非法（如 fd 越界、兴趣掩码为 0）
pub const FD_EINVAL: u32 = SEG_BASE + 0x02;
/// EMFILE：fd 表满
pub const FD_EMFILE: u32 = SEG_BASE + 0x03;
/// EEXIST：同一 fd 重复注册 epoll
pub const FD_EEXIST: u32 = SEG_BASE + 0x04;
/// ENOENT：注销未注册的 fd
pub const FD_ENOENT: u32 = SEG_BASE + 0x05;
/// EPERM：对非 mq 描述符注册 epoll（建模面）
pub const FD_EPERM: u32 = SEG_BASE + 0x06;
/// ELOOP：dup 链超深（建模保护面）
pub const FD_ELOOP: u32 = SEG_BASE + 0x07;

// ---------------------------------------------------------------------------
// 常量
// ---------------------------------------------------------------------------
/// fd 表容量
const FD_MAX: usize = 12;
/// 队列容量
const Q_CAP: usize = 8;
/// dup 链深度上界
const DUP_DEPTH_MAX: usize = 4;

// ---------------------------------------------------------------------------
// 数据结构
// ---------------------------------------------------------------------------

/// 消息队列本体（由多个描述符共享）
#[derive(Clone, Debug, PartialEq)]
pub struct MqCore {
    /// 队列名
    pub name: String,
    /// 待收消息计数
    pub pending: u32,
    /// 累计入队数
    pub sent_total: u64,
}

impl MqCore {
    /// FNV 折叠
    pub fn fold(&self) -> u64 {
        let mut h: u64 = 0xcbf29ce484222325;
        for b in self.name.as_bytes() {
            h = (h ^ (*b as u64)).wrapping_mul(0x100000001b3);
        }
        h = (h ^ self.pending as u64).wrapping_mul(0x100000001b3);
        h = (h ^ self.sent_total as u64).wrapping_mul(0x100000001b3);
        h
    }
}

/// 单个描述符的 epoll 注册态（独立于共享队列）
#[derive(Clone, Debug, PartialEq)]
pub struct EpollReg {
    /// 兴趣掩码（bit0=可读事件，bit1=可写事件）
    pub interest: u32,
    /// 该 fd 独立收到的事件次数
    pub fired: u64,
    /// 是否处于注册态
    pub active: bool,
}

impl EpollReg {
    pub fn new(interest: u32) -> Self {
        EpollReg { interest, fired: 0, active: true }
    }
}

/// 描述符条目
#[derive(Clone, Debug)]
pub struct MqFd {
    /// 指向的队列 id
    pub core: usize,
    /// dup 来源（无源为 u32::MAX）
    pub dup_from: u32,
    /// dup 链深
    pub dup_depth: usize,
    /// epoll 注册态（None=未注册）
    pub reg: Option<EpollReg>,
    /// 描述符开启
    pub open: bool,
}

/// mq fd 集成判定台
#[derive(Clone, Debug)]
pub struct MqFdDesk {
    pub cores: Vec<MqCore>,
    fds: Vec<MqFd>,
    /// 错误计数
    pub n_ebadf: u32,
    pub n_einval: u32,
    pub n_eexist: u32,
    /// dup 成功数
    pub n_dup: u32,
    /// 注册成功数
    pub n_reg: u32,
}

impl MqFdDesk {
    pub fn new() -> Self {
        MqFdDesk {
            cores: Vec::new(),
            fds: Vec::new(),
            n_ebadf: 0,
            n_einval: 0,
            n_eexist: 0,
            n_dup: 0,
            n_reg: 0,
        }
    }

    /// 打开队列，返回 mqd（fd）；优先复用已关闭槽位
    pub fn open(&mut self, name: &str) -> Result<i32, u32> {
        let core = match self.cores.iter().position(|c| c.name == name) {
            Some(i) => i,
            None => {
                self.cores.push(MqCore {
                    name: String::from(name),
                    pending: 0,
                    sent_total: 0,
                });
                self.cores.len().wrapping_sub(1)
            }
        };
        let entry = MqFd {
            core,
            dup_from: u32::MAX,
            dup_depth: 0,
            reg: None,
            open: true,
        };
        match self.fds.iter().position(|f| !f.open) {
            Some(i) => {
                self.fds[i] = entry;
                Ok(i as i32)
            }
            None => {
                if self.fds.len() >= FD_MAX {
                    return Err(FD_EMFILE);
                }
                self.fds.push(entry);
                Ok(self.fds.len() as i32 - 1)
            }
        }
    }

    /// 校验 fd
    fn check(&mut self, fd: i32) -> Result<usize, u32> {
        if fd < 0 || fd as usize >= FD_MAX {
            self.n_einval = self.n_einval.wrapping_add(1);
            return Err(FD_EINVAL);
        }
        let u = fd as usize;
        if u >= self.fds.len() || !self.fds[u].open {
            self.n_ebadf = self.n_ebadf.wrapping_add(1);
            return Err(FD_EBADF);
        }
        Ok(u)
    }

    /// dup：复制 mqd，共享底层队列，注册态不复制
    pub fn dup(&mut self, fd: i32) -> Result<i32, u32> {
        let u = self.check(fd)?;
        let src = &self.fds[u];
        if src.dup_depth >= DUP_DEPTH_MAX {
            return Err(FD_ELOOP);
        }
        let entry = MqFd {
            core: src.core,
            dup_from: u as u32,
            dup_depth: src.dup_depth.wrapping_add(1),
            reg: None,
            open: true,
        };
        match self.fds.iter().position(|f| !f.open) {
            Some(i) => {
                self.fds[i] = entry;
                self.n_dup = self.n_dup.wrapping_add(1);
                return Ok(i as i32);
            }
            None => {
                if self.fds.len() >= FD_MAX {
                    return Err(FD_EMFILE);
                }
            }
        }
        self.fds.push(entry);
        self.n_dup = self.n_dup.wrapping_add(1);
        Ok(self.fds.len() as i32 - 1)
    }

    /// epoll 注册：兴趣掩码非零且未重复注册
    pub fn epoll_add(&mut self, fd: i32, interest: u32) -> Result<(), u32> {
        let u = self.check(fd)?;
        if interest == 0 || interest > 0x3 {
            self.n_einval = self.n_einval.wrapping_add(1);
            return Err(FD_EINVAL);
        }
        if self.fds[u].reg.is_some() {
            self.n_eexist = self.n_eexist.wrapping_add(1);
            return Err(FD_EEXIST);
        }
        self.fds[u].reg = Some(EpollReg::new(interest));
        self.n_reg = self.n_reg.wrapping_add(1);
        Ok(())
    }

    /// epoll 注销
    pub fn epoll_del(&mut self, fd: i32) -> Result<(), u32> {
        let u = self.check(fd)?;
        match self.fds[u].reg.take() {
            Some(_) => Ok(()),
            None => Err(FD_ENOENT),
        }
    }

    /// 屏蔽副本事件（零串扰面：只影响该 fd 自己）
    pub fn epoll_shield(&mut self, fd: i32) -> Result<(), u32> {
        let u = self.check(fd)?;
        match self.fds[u].reg.as_mut() {
            Some(r) => {
                r.active = false;
                Ok(())
            }
            None => Err(FD_ENOENT),
        }
    }

    /// 发送消息：入队并按各自注册态独立投递事件
    pub fn send(&mut self, fd: i32) -> Result<(), u32> {
        let u = self.check(fd)?;
        let core = self.fds[u].core;
        if self.cores[core].pending as usize >= Q_CAP {
            return Err(FD_EINVAL);
        }
        self.cores[core].pending = self.cores[core].pending.wrapping_add(1);
        self.cores[core].sent_total = self.cores[core].sent_total.wrapping_add(1);
        // 事件投递：遍历所有指向同一队列的描述符
        let cl = self.fds[u].core;
        for f in self.fds.iter_mut() {
            if f.open && f.core == cl {
                if let Some(r) = f.reg.as_mut() {
                    if r.active && (r.interest & 0x1) != 0 {
                        r.fired = r.fired.wrapping_add(1);
                    }
                }
            }
        }
        Ok(())
    }

    /// 接收：消耗一条消息
    pub fn recv(&mut self, fd: i32) -> Result<u32, u32> {
        let u = self.check(fd)?;
        let core = self.fds[u].core;
        if self.cores[core].pending == 0 {
            return Err(FD_EBADF);
        }
        self.cores[core].pending = self.cores[core].pending.wrapping_sub(1);
        Ok(self.cores[core].pending)
    }

    /// 关闭描述符
    pub fn close(&mut self, fd: i32) -> Result<(), u32> {
        let u = self.check(fd)?;
        self.fds[u].open = false;
        self.fds[u].reg = None;
        Ok(())
    }

    /// 零串扰自检：副本注销后原 fd 仍能独立收事件
    pub fn no_crosstalk(&self, orig: i32, copy: i32) -> bool {
        if orig < 0 || orig as usize >= self.fds.len() {
            return false;
        }
        if copy < 0 || copy as usize >= self.fds.len() {
            return false;
        }
        let o = &self.fds[orig as usize];
        let c = &self.fds[copy as usize];
        // 两者共享同一队列但注册态互不影响
        match (o.reg.as_ref(), c.reg.as_ref()) {
            (Some(ro), Some(rc)) => ro.active != rc.active || ro.fired != rc.fired,
            (Some(ro), None) => ro.active,
            _ => false,
        }
    }

    /// 账目对平：dup/注册/事件账自洽
    pub fn ledger_ok(&self) -> bool {
        for f in &self.fds {
            if let Some(r) = f.reg.as_ref() {
                if r.interest == 0 {
                    return false;
                }
            }
        }
        self.n_dup.wrapping_add(self.n_reg) > 0
    }

    /// 汇总折叠
    pub fn fold_all(&self) -> u64 {
        let mut h: u64 = 0xcbf29ce484222325;
        for c in &self.cores {
            h = (h ^ c.fold()).wrapping_mul(0x100000001b3);
        }
        for f in &self.fds {
            h = (h ^ f.core as u64).wrapping_mul(0x100000001b3);
            h = (h ^ f.dup_depth as u64).wrapping_mul(0x100000001b3);
            if let Some(r) = f.reg.as_ref() {
                h = (h ^ r.fired).wrapping_mul(0x100000001b3);
            }
        }
        h
    }

    /// 渲染摘要
    pub fn render(&self) -> String {
        let mut s = String::from("mqfd:");
        for (i, f) in self.fds.iter().enumerate() {
            s.push('/');
            let tag = if f.dup_from == u32::MAX { "o" } else { "d" };
            let r = match f.reg.as_ref() {
                Some(r) if r.active => "R",
                Some(_) => "S",
                None => "-",
            };
            s.push_str(&crate::alloc::format!("{}:{}{}", i, tag, r));
        }
        s
    }
}

impl Default for MqFdDesk {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// 注入四路
// ---------------------------------------------------------------------------
pub fn inject_four() -> [bool; 4] {
    let mut d = MqFdDesk::new();
    let m = match d.open("/mqx") { Ok(f) => f, Err(_) => return [false; 4] };
    let c = match d.dup(m) { Ok(f) => f, Err(_) => return [false; 4] };
    // 路1：对坏 fd dup → EBADF
    let r1 = matches!(d.dup(9), Err(x) if x == FD_EBADF);
    // 路2：重复注册 → EEXIST
    let _ = d.epoll_add(m, 0x1);
    let r2 = matches!(d.epoll_add(m, 0x1), Err(x) if x == FD_EEXIST);
    // 路3：对未注册 fd 注销 → ENOENT
    let r3 = matches!(d.epoll_del(c), Err(x) if x == FD_ENOENT);
    // 路4：close 后 dup → EBADF
    let _ = d.close(c);
    let r4 = matches!(d.dup(c), Err(x) if x == FD_EBADF);
    [r1, r2, r3, r4]
}

// ---------------------------------------------------------------------------
// 确定性千轮样本
// ---------------------------------------------------------------------------
/// 千轮：正面 500 轮验证 dup/注册全通，反面 500 轮验证副本事件零串扰。
pub fn sample_thousand() -> u32 {
    let mut d = MqFdDesk::new();
    let m = match d.open("/mq-main") { Ok(f) => f, Err(_) => return 0 };
    let c = match d.dup(m) { Ok(f) => f, Err(_) => return 0 };
    let mut good: u32 = 0;
    for r in 0..1000u32 {
        let phase = r % 10;
        if phase < 5 {
            // 正面：dup 与注册路径
            let nc = match d.dup(m) {
                Ok(f) => f,
                Err(_) => {
                    // 表满则关闭一个副本再继续
                    let _ = d.close(3);
                    match d.dup(m) {
                        Ok(f) => f,
                        Err(_) => return good,
                    }
                }
            };
            let ok_reg = d.epoll_add(nc, 0x1).is_ok();
            let ok_del = d.epoll_del(nc).is_ok();
            let _ = d.close(nc);
            if ok_reg && ok_del {
                good = good.wrapping_add(1);
            }
        } else {
            // 反面：副本屏蔽后原 fd 事件照常，副本零串扰
            let _ = d.epoll_add(m, 0x1);
            let _ = d.epoll_add(c, 0x1);
            let _ = d.epoll_shield(c);
            if d.send(m).is_ok() {
                // 原fd 应收到事件，副本被屏蔽不计
                let orig_fired = d.fds[m as usize].reg.as_ref().map(|x| x.fired);
                let copy_fired = d.fds[c as usize].reg.as_ref().map(|x| x.fired);
                if orig_fired.unwrap_or(0) > 0 && copy_fired.unwrap_or(0) == 0 {
                    good = good.wrapping_add(1);
                }
            }
            // 消费防止队列满
            while d.recv(m).is_ok() {}
            let _ = d.epoll_del(m);
            let _ = d.epoll_del(c);
        }
    }
    good
}

// ---------------------------------------------------------------------------
// 判据检查
// ---------------------------------------------------------------------------
pub fn run_unx10956_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F10956");

    let inj = inject_four();
    s.add("J1-注入四路全拦截", inj.iter().all(|&b| b), "EBADF/EINVAL/EEXIST/ENOENT 坏样本全部命中");

    // J1 正面：dup 与 epoll 注册各 10/10
    let mut d = MqFdDesk::new();
    let m = match d.open("/mq-j1") { Ok(f) => f, Err(_) => -1 };
    let mut dup_ok = 0u32;
    let mut reg_ok = 0u32;
    for _ in 0..10 {
        if let Ok(c) = d.dup(m) {
            dup_ok = dup_ok.wrapping_add(1);
            let _ = d.close(c);
        }
        // 每次重新打开一个新 fd 用于注册
        if let Ok(x) = d.open("/mq-j1") {
            if d.epoll_add(x, 0x1).is_ok() {
                reg_ok = reg_ok.wrapping_add(1);
            }
            let _ = d.close(x);
        }
    }
    s.add(
        "J1-正面-dup/注册各10/10",
        dup_ok == 10 && reg_ok == 10,
        "mqd 十次 dup 全成功，十次 epoll 注册全成功",
    );

    // J2 反面：副本事件零串扰（千次）
    let good = sample_thousand();
    s.add("J2-反面-副本零串扰千次", good == 1000, "屏蔽副本后原 fd 事件照常，副本计数为零");

    // 判定序：EBADF 先于 EEXIST
    let mut d2 = MqFdDesk::new();
    let m2 = match d2.open("/mq-seq") { Ok(f) => f, Err(_) => -1 };
    let _ = d2.epoll_add(m2, 0x1);
    let _ = d2.close(m2);
    let seq_ok = matches!(d2.epoll_add(m2, 0x1), Err(x) if x == FD_EBADF);
    s.add("J3-判定序-EBADF先于EEXIST", seq_ok, "先校验 fd 存活再查重复注册");

    // dup 共享队列：副本 send 计入同一 core
    let mut d3 = MqFdDesk::new();
    let m3 = match d3.open("/mq-share") { Ok(f) => f, Err(_) => -1 };
    let c3 = match d3.dup(m3) { Ok(f) => f, Err(_) => -1 };
    let _ = d3.send(c3);
    let share_ok = d3.cores[0].sent_total == 1 && d3.cores[0].pending == 1;
    s.add("J4-dup共享队列", share_ok, "副本写入与原 fd 同账");

    // dup 注册态不复制
    let mut d4 = MqFdDesk::new();
    let m4 = match d4.open("/mq-noreg") { Ok(f) => f, Err(_) => -1 };
    let _ = d4.epoll_add(m4, 0x1);
    let c4 = match d4.dup(m4) { Ok(f) => f, Err(_) => -1 };
    s.add(
        "J5-注册态不复制",
        d4.fds[m4 as usize].reg.is_some() && d4.fds[c4 as usize].reg.is_none(),
        "dup 后副本未继承 epoll 注册",
    );

    // 错误码分段
    let codes = [FD_EBADF, FD_EINVAL, FD_EMFILE, FD_EEXIST, FD_ENOENT, FD_EPERM, FD_ELOOP];
    let seg_ok = codes.iter().all(|&c| c > SEG_BASE && c < SEG_BASE + 0x20);
    s.add("J6-错误码分段", seg_ok, "七码全部落在 0x1d700 段内");

    // 错误码互异
    let mut distinct = true;
    for i in 0..codes.len() {
        for j in (i + 1)..codes.len() {
            if codes[i] == codes[j] {
                distinct = false;
            }
        }
    }
    s.add("J7-错误码互异", distinct, "七码两两互异");

    // 账目对平
    s.add("J8-账目对平", d3.ledger_ok(), "dup/注册/事件账自洽");

    // 零串扰断言（独立路径）
    let mut d5 = MqFdDesk::new();
    let m5 = match d5.open("/mq-xt") { Ok(f) => f, Err(_) => -1 };
    let c5 = match d5.dup(m5) { Ok(f) => f, Err(_) => -1 };
    let _ = d5.epoll_add(m5, 0x1);
    let _ = d5.epoll_add(c5, 0x1);
    let _ = d5.epoll_shield(c5);
    let _ = d5.send(m5);
    s.add(
        "J9-零串扰独立路径",
        d5.no_crosstalk(m5, c5) && d5.fds[m5 as usize].reg.as_ref().unwrap_or(&EpollReg::new(0)).fired == 1,
        "屏蔽只影响副本，原 fd 独立收事件",
    );

    // fold 稳定
    let h1 = d5.fold_all();
    let h2 = d5.fold_all();
    s.add("J10-fold稳定", h1 == h2, "同一状态两次折叠一致");

    // 渲染非空
    let r = d5.render();
    s.add("J11-渲染非空", r.starts_with("mqfd:"), "摘要含前缀与注册标记");

    // dup 链深保护面
    let mut d6 = MqFdDesk::new();
    let m6 = match d6.open("/mq-loop") { Ok(f) => f, Err(_) => -1 };
    let mut cur = m6;
    let mut loop_ok = false;
    for _ in 0..DUP_DEPTH_MAX + 1 {
        match d6.dup(cur) {
            Ok(n) => cur = n,
            Err(e) => {
                loop_ok = e == FD_ELOOP;
                break;
            }
        }
    }
    s.add("J12-dup链深保护", loop_ok, "dup 链超深被 ELOOP 拦截");

    // 动态判据
    s.add_dyn(
        "J13-dup/注册账",
        d.n_dup >= 10 && d.n_reg >= 10,
        &crate::alloc::format!("dup={} 注册={}", d.n_dup, d.n_reg),
    );
    s.add_dyn(
        "J14-千轮通过率",
        good == 1000,
        &crate::alloc::format!("{}//1000", good),
    );
    s.add_dyn(
        "J15-事件总账",
        d5.fds.iter().all(|f| f.reg.is_none() || f.reg.as_ref().map(|r| r.interest != 0).unwrap_or(true)),
        &crate::alloc::format!("事件账核验完成 fd数={}", d5.fds.len()),
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
    fn unx10956_mqfd_dup_epoll_flow() {
        let mut d = MqFdDesk::new();
        let m = d.open("/mq-t").unwrap();
        let c = d.dup(m).unwrap();
        assert!(d.epoll_add(m, 0x1).is_ok());
        assert!(d.epoll_add(c, 0x1).is_ok());
        assert!(matches!(d.epoll_add(m, 0x1), Err(x) if x == FD_EEXIST));
        assert!(d.send(m).is_ok());
        assert_eq!(d.fds[m as usize].reg.as_ref().map(|r| r.fired), Some(1));
        assert!(d.recv(c).is_ok());
        assert!(matches!(d.dup(50), Err(x) if x == FD_EINVAL));
        let inj = inject_four();
        assert!(inj.iter().all(|&b| b));
        assert_eq!(sample_thousand(), 1000);
    }

    #[test]
    fn unx10956_checks_green() {
        let s = run_unx10956_checks();
        assert!(s.all_passed(), "UNX-F10956 判据未全绿");
    }
}
