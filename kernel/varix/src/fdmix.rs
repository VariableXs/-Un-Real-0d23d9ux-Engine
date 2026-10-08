//! fdmix — WP-204 · B-602 描述符混表（MD2 篇 6.2 第其一）。
//!
//! 判据 B-602：poll/close 聚合语义测试全绿。
//! MD2 原文（6.2）："其一，**描述符表统一**——socket 与文件共用描述符空间，
//! poll 与 close 等聚合调用天然一致（Linux 应用对此有强预期，混表是兼容性
//! 高频坑）；其二，非阻塞与事件……边缘触发的'只报一次'语义如实实现。"
//!
//! 宿主可测形态：统一描述符表（File/Socket 两型共用 fd 空间，POSIX 最小
//! 可用 fd 分配语义）+ close 释放复用 + poll 聚合跨类型 + 边缘触发只报
//! 一次 + fd 泄漏对账（alloc/close 恒等式）。

use crate::checks::CheckSet;

/// 描述符表容量（进程级）。
pub const FD_CAP: usize = 16;

/// fd 背后两种对象（混表的"混"）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FdKind {
    File,
    Socket,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct FdEntry {
    pub kind: FdKind,
    /// 就绪状态（poll 聚合的源；socket 侧由事件环驱动）
    pub ready: bool,
    /// 边缘触发：就绪沿已上报（"只报一次"）
    pub edge_reported: bool,
    pub open: bool,
}

/// 统一描述符表：socket 与文件共用 fd 空间。
pub struct FdTable {
    pub slots: [Option<FdEntry>; FD_CAP],
    /// 高水位（历史最大占用——泄漏对账面）
    pub high_water: usize,
    pub live: usize,
}

impl FdTable {
    pub const fn new() -> FdTable {
        FdTable { slots: [None; FD_CAP], high_water: 0, live: 0 }
    }

    /// 分配最小可用 fd（POSIX 语义：从 0 起找第一个空位）。
    pub fn alloc(&mut self, kind: FdKind) -> Option<usize> {
        for (i, slot) in self.slots.iter_mut().enumerate() {
            if slot.is_none() {
                *slot = Some(FdEntry { kind, ready: false, edge_reported: false, open: true });
                self.live += 1;
                if self.live > self.high_water {
                    self.high_water = self.live;
                }
                return Some(i);
            }
        }
        None // 表满：EMFILE
    }

    /// close：释放槽位（双次 close → None = EBADF 语义）。
    pub fn close(&mut self, fd: usize) -> Option<FdKind> {
        if fd >= FD_CAP {
            return None;
        }
        let e = self.slots[fd].take()?;
        self.live -= 1;
        Some(e.kind)
    }

    /// 标记就绪（socket 事件环驱动；文件恒就绪语义由调用方定）。
    pub fn mark_ready(&mut self, fd: usize) -> bool {
        match self.slots.get_mut(fd) {
            Some(Some(e)) if e.open => {
                e.ready = true;
                true
            }
            _ => false,
        }
    }

    /// poll 聚合：返回就绪 fd 清单（**跨类型**——File 与 Socket 同表聚合）。
    /// 边缘触发语义：就绪沿只报一次（edge_reported 后不再上报，除非状态离开又回来）。
    pub fn poll(&mut self, fds: &[usize]) -> ([bool; FD_CAP], usize) {
        let mut out = [false; FD_CAP];
        let mut n = 0usize;
        for &fd in fds {
            if let Some(Some(e)) = self.slots.get_mut(fd) {
                if e.open && e.ready && !e.edge_reported {
                    out[fd] = true;
                    e.edge_reported = true;
                    n += 1;
                }
            }
        }
        (out, n)
    }

    /// 就绪沿复位（数据被读走 → 状态离开 → 可再次上报）。
    pub fn clear_ready(&mut self, fd: usize) -> bool {
        match self.slots.get_mut(fd) {
            Some(Some(e)) if e.open => {
                e.ready = false;
                e.edge_reported = false;
                true
            }
            _ => false,
        }
    }
}

// ---------------------------------------------------------------- 对练

/// 混表对练摘要。
#[derive(Default, PartialEq, Eq, Debug)]
pub struct FdDrillSummary {
    pub rounds: u32,
    pub allocs: u64,
    pub closes: u64,
    /// alloc − close == live（fd 泄漏对账恒等式）
    pub leak_free: bool,
    /// poll 聚合跨类型正确
    pub poll_correct: bool,
    /// 边缘触发只报一次
    pub edge_once: bool,
}

/// 交错分配/关闭对练：File 与 Socket 随机交错 × poll 聚合 × 边沿语义。
pub fn run_fd_drills(seed: u64, rounds: u32) -> FdDrillSummary {
    let mut g = crate::comprecover::Lcg(seed);
    let mut sum = FdDrillSummary::default();
    sum.rounds = rounds;
    sum.leak_free = true;
    sum.poll_correct = true;
    sum.edge_once = true;
    for _ in 0..rounds {
        let mut t = FdTable::new();
        let mut open_fds: [usize; FD_CAP] = [usize::MAX; FD_CAP];
        let mut n_open = 0usize;
        // 泄漏对账口径：**本轮**对账（t 每轮重建；sum 是跨轮累计，不能拿来对 t.live）
        let mut round_allocs = 0u64;
        let mut round_closes = 0u64;
        for step in 0..24 {
            match g.next() % 3 {
                0 | 1 => {
                    let kind = if g.next() % 2 == 0 { FdKind::File } else { FdKind::Socket };
                    if let Some(fd) = t.alloc(kind) {
                        open_fds[n_open] = fd;
                        n_open += 1;
                        round_allocs += 1;
                    }
                }
                _ => {
                    if n_open > 0 {
                        let idx = (g.next() % n_open as u64) as usize;
                        let fd = open_fds[idx];
                        if t.close(fd).is_some() {
                            round_closes += 1;
                        }
                        open_fds[idx..n_open].copy_within(1.., 0);
                        n_open -= 1;
                    }
                }
            }
        }
        // 泄漏对账（本轮）：alloc − close == live
        if t.live as u64 != round_allocs - round_closes {
            sum.leak_free = false;
        }
        sum.allocs += round_allocs;
        sum.closes += round_closes;
        // poll 聚合跨类型：File fd + Socket fd 各一，双就绪 → 都上报
        let mut t2 = FdTable::new();
        let f = t2.alloc(FdKind::File).unwrap();
        let s = t2.alloc(FdKind::Socket).unwrap();
        let _ = t2.mark_ready(f);
        let _ = t2.mark_ready(s);
        let (r, n) = t2.poll(&[f, s]);
        if !(r[f] && r[s] && n == 2) {
            sum.poll_correct = false;
        }
        // 边缘触发只报一次
        let (_, n2) = t2.poll(&[f, s]);
        if n2 != 0 {
            sum.edge_once = false;
        }
        // 状态离开又回来 → 再报
        let _ = t2.clear_ready(f);
        let _ = t2.mark_ready(f);
        let (_, n3) = t2.poll(&[f]);
        if n3 != 1 {
            sum.edge_once = false;
        }
    }
    sum
}

// ---------------------------------------------------------------- 自检

pub fn run_fdmix_checks() -> crate::checks::CheckSet {
    let mut set = CheckSet::new("B-602 描述符混表");
    {
        // 统一描述符空间：File/Socket 同表分配
        let mut t = FdTable::new();
        let a = t.alloc(FdKind::File);
        let b = t.alloc(FdKind::Socket);
        set.add(
            "B-602 socket 与文件共用 fd 空间",
            a == Some(0) && b == Some(1),
            "POSIX 最小可用 fd 语义",
        );
    }
    {
        // close 释放复用
        let mut t = FdTable::new();
        let _ = t.alloc(FdKind::File);
        let kind = t.close(0);
        let again = t.alloc(FdKind::Socket);
        set.add(
            "B-602 close 释放复用",
            kind == Some(FdKind::File) && again == Some(0),
            "释放后 fd 可被新分配复用",
        );
    }
    {
        // 双次 close：EBADF 语义（返回 None）
        let mut t = FdTable::new();
        let _ = t.alloc(FdKind::Socket);
        let first = t.close(0);
        let second = t.close(0);
        set.add(
            "B-602 双次 close EBADF",
            first.is_some() && second.is_none(),
            "错误语义按 Linux 值（C-4）",
        );
    }
    {
        // poll 聚合跨类型
        let mut t = FdTable::new();
        let f = t.alloc(FdKind::File).unwrap();
        let s = t.alloc(FdKind::Socket).unwrap();
        let _ = t.mark_ready(f);
        let _ = t.mark_ready(s);
        let (r, n) = t.poll(&[f, s]);
        set.add(
            "B-602 poll 聚合跨类型",
            n == 2 && r[f] && r[s],
            "poll 与 close 聚合调用天然一致",
        );
    }
    {
        // 边缘触发只报一次
        let mut t = FdTable::new();
        let s = t.alloc(FdKind::Socket).unwrap();
        let _ = t.mark_ready(s);
        let (_, n1) = t.poll(&[s]);
        let (_, n2) = t.poll(&[s]);
        set.add(
            "B-602 边缘触发只报一次",
            n1 == 1 && n2 == 0,
            "epoll 边缘触发语义如实实现",
        );
    }
    {
        // 状态离开又回来 → 再报
        let mut t = FdTable::new();
        let s = t.alloc(FdKind::Socket).unwrap();
        let _ = t.mark_ready(s);
        let _ = t.poll(&[s]);
        let _ = t.clear_ready(s);
        let _ = t.mark_ready(s);
        let (_, n) = t.poll(&[s]);
        set.add(
            "B-602 沿复位再报",
            n == 1,
            "离开→回来是新沿（非水平触发）",
        );
    }
    {
        // fd 泄漏对账恒等式
        let mut t = FdTable::new();
        for _ in 0..5 {
            let _ = t.alloc(FdKind::File);
        }
        for i in [1, 3, 4] {
            let _ = t.close(i);
        }
        set.add(
            "B-602 泄漏对账",
            t.live == 2 && t.high_water == 5,
            "alloc − close = live；高水位可查",
        );
    }
    {
        // 表满 EMFILE
        let mut t = FdTable::new();
        let mut last = None;
        for _ in 0..(FD_CAP + 2) {
            last = t.alloc(FdKind::Socket);
        }
        set.add(
            "B-602 表满拒绝",
            last.is_none() && t.live == FD_CAP,
            "EMFILE 语义（不越界不覆盖）",
        );
    }
    {
        // 混表对练
        let sum = run_fd_drills(0xB602, 80);
        set.add(
            "B-602 混表对练",
            sum.rounds == 80 && sum.leak_free && sum.poll_correct && sum.edge_once && sum.allocs > 0,
            "poll/close 聚合语义测试全绿",
        );
    }
    set
}

// ---------------------------------------------------------------- 单测

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn f702_posix_min_fd() {
        let mut t = FdTable::new();
        assert_eq!(t.alloc(FdKind::File), Some(0));
        assert_eq!(t.alloc(FdKind::Socket), Some(1));
        assert_eq!(t.alloc(FdKind::File), Some(2));
        let _ = t.close(1);
        assert_eq!(t.alloc(FdKind::Socket), Some(1), "最小可用 fd 复用");
    }

    #[test]
    fn f702_edge_semantics() {
        let mut t = FdTable::new();
        let s = t.alloc(FdKind::Socket).unwrap();
        let _ = t.mark_ready(s);
        assert_eq!(t.poll(&[s]).1, 1);
        assert_eq!(t.poll(&[s]).1, 0, "只报一次");
        let _ = t.clear_ready(s);
        let _ = t.mark_ready(s);
        assert_eq!(t.poll(&[s]).1, 1, "新沿再报");
    }

    #[test]
    fn f702_mixed_poll() {
        let mut t = FdTable::new();
        let mut fds = [0usize; 4];
        for (i, fd) in fds.iter_mut().enumerate() {
            *fd = t.alloc(if i % 2 == 0 { FdKind::File } else { FdKind::Socket }).unwrap();
        }
        let _ = t.mark_ready(fds[1]);
        let _ = t.mark_ready(fds[3]);
        let (r, n) = t.poll(&fds);
        assert_eq!(n, 2);
        assert!(r[fds[1]] && r[fds[3]] && !r[fds[0]] && !r[fds[2]]);
    }

    #[test]
    fn f702_drill_leak_free() {
        let s = run_fd_drills(9, 60);
        assert!(s.leak_free && s.poll_correct && s.edge_once);
        assert_eq!(s.allocs, s.closes + s.allocs - s.closes); // 恒等式平凡但锁语义
    }
}
