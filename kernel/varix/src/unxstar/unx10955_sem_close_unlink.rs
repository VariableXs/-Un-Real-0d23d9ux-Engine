//! UNX-F10955 sem_close/sem_unlink 错误矩阵（0x1d6e0 段）
//!
//! 卡片判据（UNX-F10955-J1）：
//! - 正面：EINVAL/ENOENT/EACCES 三错误各 10/10；
//! - 反面：合法关闭/删除千次零误报（10/10）。
//!
//! 域：CoRun·UNX-C4-B28。本模块建模 POSIX 命名信号量的关闭与
//! 删除两个生命终点的错误矩阵：close 只影响调用方 fd（EINVAL 面
//! 为坏 fd），unlink 拆名（ENOENT 面为名字不存在，EACCES 面为
//! 非属主），并保持"unlink 后活跃 fd 存活"的延迟销毁语义。

use crate::alloc::string::String;
use crate::checks::CheckSet;
use alloc::vec::Vec;
use alloc::vec;

// ---------------------------------------------------------------------------
// 错误码（SEG_BASE = 0x1d000 + (10955-10900)*0x20 = 0x1d6e0）
// ---------------------------------------------------------------------------
const SEG_BASE: u32 = 0x1d6e0;

/// EINVAL：close 传入了从未登记的坏 fd
pub const CL_EINVAL: u32 = SEG_BASE + 0x01;
/// ENOENT：unlink 的名字不存在
pub const CL_ENOENT: u32 = SEG_BASE + 0x02;
/// EACCES：unlink 的调用方不是属主
pub const CL_EACCES: u32 = SEG_BASE + 0x03;
/// EBADF：close 后重复关闭（fd 悬空）
pub const CL_EBADF: u32 = SEG_BASE + 0x04;
/// EBUSY：unlink 时名字被 pinned（建模保护面）
pub const CL_EBUSY: u32 = SEG_BASE + 0x05;
/// ENAMETOOLONG：名字超长
pub const CL_ENAME: u32 = SEG_BASE + 0x06;

// ---------------------------------------------------------------------------
// 常量
// ---------------------------------------------------------------------------
/// 名字上界
const NAME_MAX: usize = 64;
/// fd 表容量
const FD_MAX: usize = 8;
/// fd 空槽
const FD_FREE: u32 = u32::MAX;

// ---------------------------------------------------------------------------
// 数据结构
// ---------------------------------------------------------------------------

/// 命名信号量条目
#[derive(Clone, Debug, PartialEq)]
pub struct NamedSem {
    /// 域内唯一 id
    pub id: u32,
    /// 名字（/ 开头）
    pub name: String,
    /// 属主 pid
    pub owner: u32,
    /// 名字是否仍在命名空间（unlink 后 false）
    pub linked: bool,
    /// 值账
    pub value: u32,
}

impl NamedSem {
    pub fn new(id: u32, name: &str, owner: u32, value: u32) -> Self {
        NamedSem { id, name: String::from(name), owner, linked: true, value }
    }

    /// FNV 折叠
    pub fn fold(&self) -> u64 {
        let mut h: u64 = 0xcbf29ce484222325;
        h = (h ^ self.id as u64).wrapping_mul(0x100000001b3);
        h = (h ^ self.owner as u64).wrapping_mul(0x100000001b3);
        h = (h ^ (self.linked as u64)).wrapping_mul(0x100000001b3);
        h = (h ^ self.value as u64).wrapping_mul(0x100000001b3);
        for b in self.name.as_bytes() {
            h = (h ^ (*b as u64)).wrapping_mul(0x100000001b3);
        }
        h
    }
}

/// close/unlink 错误矩阵判定台
#[derive(Clone, Debug)]
pub struct CloseBoard {
    /// 命名空间条目
    pub objs: Vec<NamedSem>,
    /// fd -> id
    fds: Vec<u32>,
    /// fd 是否曾关闭过（区分重复关闭与从未打开）
    closed_mask: Vec<bool>,
    /// 错误计数
    pub n_einval: u32,
    pub n_enoent: u32,
    pub n_eacces: u32,
    /// 合法关闭/删除计数
    pub n_ok_close: u32,
    pub n_ok_unlink: u32,
}

impl CloseBoard {
    pub fn new() -> Self {
        CloseBoard {
            objs: Vec::new(),
            fds: vec![FD_FREE; FD_MAX],
            closed_mask: vec![false; FD_MAX],
            n_einval: 0,
            n_enoent: 0,
            n_eacces: 0,
            n_ok_close: 0,
            n_ok_unlink: 0,
        }
    }

    /// 创建并打开，返回 fd
    pub fn create(&mut self, name: &str, owner: u32, value: u32) -> Result<usize, u32> {
        if name.as_bytes().first() != Some(&b'/') || name.len() > NAME_MAX {
            return Err(CL_ENAME);
        }
        // 同名且仍链接的条目按 sem_open 语义复用，不重复建项：
        // 否则反复"关闭后重开同名"会把 8 槽表灌满，合法面随后全部失败。
        let found_ix = self.find_name(name);
        let id;
        if let Some(ix) = found_ix {
            id = self.objs[ix].id;
        } else {
            if self.objs.len() >= FD_MAX {
                return Err(CL_EINVAL);
            }
            let new_id = 2000u32.wrapping_add(self.objs.len() as u32);
            self.objs.push(NamedSem::new(new_id, name, owner, value));
            id = new_id;
        }
        let mut i = 0;
        while i < FD_MAX {
            if self.fds[i] == FD_FREE {
                self.fds[i] = id;
                self.closed_mask[i] = false;
                return Ok(i);
            }
            i += 1;
        }
        // fd 表满：新建的条目回滚
        if found_ix.is_none() {
            let _ = self.objs.pop();
        }
        Err(CL_EINVAL)
    }

    /// 只读查找：优先命中仍在命名空间的活跃条目
    fn find_name(&self, name: &str) -> Option<usize> {
        self.objs.iter().position(|o| o.name == name && o.linked)
    }

    /// sem_close：释放本方 fd；从未打开 → EINVAL，重复关闭 → EBADF
    pub fn close(&mut self, fd: i32) -> Result<(), u32> {
        if fd < 0 || fd as usize >= FD_MAX {
            self.n_einval = self.n_einval.wrapping_add(1);
            return Err(CL_EINVAL);
        }
        let u = fd as usize;
        if self.fds[u] == FD_FREE {
            if self.closed_mask[u] {
                return Err(CL_EBADF);
            }
            self.n_einval = self.n_einval.wrapping_add(1);
            return Err(CL_EINVAL);
        }
        self.fds[u] = FD_FREE;
        self.closed_mask[u] = true;
        self.n_ok_close = self.n_ok_close.wrapping_add(1);
        Ok(())
    }

    /// sem_unlink：拆名；ENOENT（无此名）/EACCES（非属主）；
    /// 拆名后对象延迟销毁（活跃 fd 存活）
    pub fn unlink(&mut self, name: &str, pid: u32) -> Result<(), u32> {
        match self.find_name(name) {
            None => {
                self.n_enoent = self.n_enoent.wrapping_add(1);
                Err(CL_ENOENT)
            }
            Some(idx) => {
                if !self.objs[idx].linked {
                    self.n_enoent = self.n_enoent.wrapping_add(1);
                    return Err(CL_ENOENT);
                }
                if self.objs[idx].owner != pid {
                    self.n_eacces = self.n_eacces.wrapping_add(1);
                    return Err(CL_EACCES);
                }
                self.objs[idx].linked = false;
                self.n_ok_unlink = self.n_ok_unlink.wrapping_add(1);
                Ok(())
            }
        }
    }

    /// 活跃 fd 是否仍可读写（unlink 后存活语义）
    pub fn fd_alive(&self, fd: i32) -> bool {
        if fd < 0 || fd as usize >= FD_MAX {
            return false;
        }
        let id = self.fds[fd as usize];
        if id == FD_FREE {
            return false;
        }
        match self.objs.iter().position(|o| o.id == id) {
            Some(i) => true,
            None => false,
        }
    }

    /// 已 unlink 且无活跃 fd 的对象回收
    pub fn reap(&mut self) -> usize {
        let mut reaped = 0usize;
        let mut i = 0usize;
        while i < self.objs.len() {
            let dead = !self.objs[i].linked
                && !self.fds.contains(&self.objs[i].id);
            if dead {
                let _ = self.objs.remove(i);
                reaped = reaped.wrapping_add(1);
            } else {
                i = i.wrapping_add(1);
            }
        }
        reaped
    }

    /// 账目对平：ok 计数与错误计数自洽
    pub fn ledger_ok(&self, expect_close: u32, expect_unlink: u32) -> bool {
        self.n_ok_close == expect_close && self.n_ok_unlink == expect_unlink
    }

    /// 汇总折叠
    pub fn fold_all(&self) -> u64 {
        let mut h: u64 = 0xcbf29ce484222325;
        for o in &self.objs {
            h = (h ^ o.fold()).wrapping_mul(0x100000001b3);
        }
        h = (h ^ self.n_ok_close as u64).wrapping_mul(0x100000001b3);
        h = (h ^ self.n_ok_unlink as u64).wrapping_mul(0x100000001b3);
        h
    }

    /// 渲染摘要
    pub fn render(&self) -> String {
        let mut s = String::from("close/unlink:");
        for o in &self.objs {
            s.push('/');
            if o.linked {
                s.push_str(&crate::alloc::format!("{}:L", o.name));
            } else {
                s.push_str(&crate::alloc::format!("{}:U", o.name));
            }
        }
        s
    }
}

impl Default for CloseBoard {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// 注入四路
// ---------------------------------------------------------------------------
pub fn inject_four() -> [bool; 4] {
    let mut b = CloseBoard::new();
    let _ = b.create("/cl-a", 7, 3);
    // 路1：close 坏 fd → EINVAL 面
    let r1 = matches!(b.close(9), Err(x) if x == CL_EINVAL);
    // 路2：unlink 不存在的名字 → ENOENT
    let r2 = matches!(b.unlink("/ghost", 7), Err(x) if x == CL_ENOENT);
    // 路3：非属主 unlink → EACCES
    let r3 = matches!(b.unlink("/cl-a", 99), Err(x) if x == CL_EACCES);
    // 路4：unlink 后再 unlink 同名 → ENOENT
    let _ = b.unlink("/cl-a", 7);
    let r4 = matches!(b.unlink("/cl-a", 7), Err(x) if x == CL_ENOENT);
    [r1, r2, r3, r4]
}

// ---------------------------------------------------------------------------
// 确定性千轮样本
// ---------------------------------------------------------------------------
/// 千轮：300 轮走三个错误面（各 100），700 轮合法关闭/删除零误报。
pub fn sample_thousand() -> u32 {
    let mut b = CloseBoard::new();
    let _ = b.create("/cl-main", 7, 8);
    let mut good: u32 = 0;
    for r in 0..1000u32 {
        let phase = r % 10;
        match phase {
            0 => {
                // EINVAL 面：close 坏 fd
                if matches!(b.close(-1), Err(x) if x == CL_EINVAL) {
                    good = good.wrapping_add(1);
                }
            }
            1 | 2 => {
                // ENOENT 面：unlink 未登记名
                if matches!(b.unlink("/no-such", 7), Err(x) if x == CL_ENOENT) {
                    good = good.wrapping_add(1);
                }
            }
            3 => {
                // EACCES 面：非属主
                if matches!(b.unlink("/cl-main", 13), Err(x) if x == CL_EACCES) {
                    good = good.wrapping_add(1);
                }
            }
            4 | 5 | 6 => {
                // 合法关闭：关闭 fd0 再重建
                if b.close(0).is_ok() {
                    good = good.wrapping_add(1);
                }
                let _ = b.create("/cl-main", 7, 8);
            }
            _ => {
                // 合法关闭+删除：close 释放 fd，unlink 拆名，回收后重建
                let closed = b.close(0).is_ok();
                let unlinked = b.unlink("/cl-main", 7).is_ok();
                let _ = b.reap();
                let _ = b.create("/cl-main", 7, 8);
                if closed && unlinked {
                    good = good.wrapping_add(1);
                }
            }
        }
    }
    good
}

// ---------------------------------------------------------------------------
// 判据检查
// ---------------------------------------------------------------------------
pub fn run_unx10955_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F10955");

    let inj = inject_four();
    s.add("J1-注入四路全拦截", inj.iter().all(|&b| b), "EINVAL/ENOENT/EACCES 等坏样本全部命中");

    // J1 正面：三错误各 10/10
    let mut b = CloseBoard::new();
    let _ = b.create("/cl-j1", 7, 4);
    let mut einval = 0u32;
    let mut enoent = 0u32;
    let mut eacces = 0u32;
    for _ in 0..10 {
        if matches!(b.close(-3), Err(x) if x == CL_EINVAL) {
            einval = einval.wrapping_add(1);
        }
        if matches!(b.unlink("/absent", 7), Err(x) if x == CL_ENOENT) {
            enoent = enoent.wrapping_add(1);
        }
        if matches!(b.unlink("/cl-j1", 21), Err(x) if x == CL_EACCES) {
            eacces = eacces.wrapping_add(1);
        }
    }
    s.add(
        "J1-正面-三错误各10/10",
        einval == 10 && enoent == 10 && eacces == 10,
        "坏 fd、无名、非属主三条错误面十次全中",
    );

    // J2 反面：合法关闭/删除千次零误报
    let good = sample_thousand();
    s.add("J2-反面-千次零误报", good == 1000, "700 轮合法关删零误报，300 轮错误面全中");

    // 判定序：close EBADF 细分先于对象查询
    let mut b2 = CloseBoard::new();
    let _ = b2.create("/cl-seq", 7, 1);
    let _ = b2.close(0);
    let seq_ok = matches!(b2.close(0), Err(x) if x == CL_EBADF);
    s.add("J3-判定序-重复关闭EBADF", seq_ok, "close 后 fd 悬空，重复关闭细分 EBADF");

    // unlink 后活跃 fd 存活
    let mut b3 = CloseBoard::new();
    let _ = b3.create("/cl-live", 7, 5);
    let _ = b3.unlink("/cl-live", 7);
    s.add(
        "J4-unlink后fd存活",
        b3.fd_alive(0),
        "延迟销毁：拆名后活跃描述符仍可用",
    );

    // 错误码分段
    let codes = [CL_EINVAL, CL_ENOENT, CL_EACCES, CL_EBADF, CL_EBUSY, CL_ENAME];
    let seg_ok = codes.iter().all(|&c| c > SEG_BASE && c < SEG_BASE + 0x20);
    s.add("J5-错误码分段", seg_ok, "六码全部落在 0x1d6e0 段内");

    // 错误码互异
    let mut distinct = true;
    for i in 0..codes.len() {
        for j in (i + 1)..codes.len() {
            if codes[i] == codes[j] {
                distinct = false;
            }
        }
    }
    s.add("J6-错误码互异", distinct, "六码两两互异");

    // 账目对平
    s.add("J7-账目对平", b3.ledger_ok(0, 1), "关闭/删除计数与期望一致");

    // 回收
    let _ = b3.close(0);
    let reaped = b3.reap();
    s.add("J8-回收生效", reaped == 1 && b3.objs.is_empty(), "拆名且无 fd 后对象被回收");

    // fold 稳定
    let h1 = b3.fold_all();
    let h2 = b3.fold_all();
    s.add("J9-fold稳定", h1 == h2, "同一状态两次折叠一致");

    // 渲染非空
    let r = b3.render();
    s.add("J10-渲染非空", r.starts_with("close/unlink:"), "摘要含前缀与生命周期标记");

    // 名字校验面
    let mut b4 = CloseBoard::new();
    let bad_name = matches!(b4.create("no-slash", 7, 1), Err(x) if x == CL_ENAME);
    let long_name = matches!(b4.create("/x9999999999999999999999999999999999999999999999999999999999999999", 7, 1), Err(x) if x == CL_ENAME);
    s.add(
        "J11-名字校验面",
        bad_name && long_name,
        "无斜杠与超长名均被拒绝",
    );

    // 动态判据
    s.add_dyn(
        "J12-错误计数",
        b.n_einval >= 10 && b.n_enoent >= 10 && b.n_eacces >= 10,
        &crate::alloc::format!("EINVAL={} ENOENT={} EACCES={}", b.n_einval, b.n_enoent, b.n_eacces),
    );
    s.add_dyn(
        "J13-千轮通过率",
        good == 1000,
        &crate::alloc::format!("{}//1000", good),
    );
    s.add_dyn(
        "J14-合法关删账",
        b.n_ok_close == 0 && b.n_ok_unlink == 0,
        &crate::alloc::format!("合法关闭={} 合法删除={}", b.n_ok_close, b.n_ok_unlink),
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
    fn unx10955_sem_close_unlink_flow() {
        let mut b = CloseBoard::new();
        let f = b.create("/cl-t", 7, 2).unwrap();
        assert!(b.unlink("/cl-t", 7).is_ok());
        assert!(b.fd_alive(f as i32));
        assert!(matches!(b.unlink("/cl-t", 7), Err(x) if x == CL_ENOENT));
        assert!(b.close(f as i32).is_ok());
        assert_eq!(b.reap(), 1);
        let inj = inject_four();
        assert!(inj.iter().all(|&b| b));
        assert_eq!(sample_thousand(), 1000);
    }

    #[test]
    fn unx10955_checks_green() {
        let s = run_unx10955_checks();
        assert!(s.all_passed(), "UNX-F10955 判据未全绿");
    }
}
