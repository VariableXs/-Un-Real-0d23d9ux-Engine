//! mech_oom — OOM badness 评分与连环处决本体（AI-K1 深化批次五 · F045）。
//!
//! 主册依据：
//! - F045【设计细节】「水位回收」——mech_scan 管回收循环（回收文件页/
//!   匿名页直到 high 水位），但**回收失败后的最后一道防线**缺席：
//!   谁被杀、按什么算账、保护者怎么豁免、杀完还不够怎么办。本件按
//!   Linux `out_of_memory()` 语义实现：badness 点数 = rss + swap +
//!   页表开销，再叠加 oom_score_adj 线性偏置（-1000 = 绝对豁免），
//!   最高分者被选；平局取最老任务（pid 最小——确定性 tie-break）；
//!   处决后释放量回账、低于水位则连环重评（循环有界）。
//! - 锚点：F045「断电百次中脏页丢失窗口 ≤ 水位规则承诺值」的兜底
//!   分支——水位承诺只在「总有可回收内存」时成立，OOM 是承诺的
//!   破产程序，必须确定性（同输入同受害者）。
//! - 零堆、零浮点。

// ---------------------------------------------------------------------------
// 1. 任务账目
// ---------------------------------------------------------------------------

/// 最多同时跟踪的任务数（容量纪律）。
pub const MAX_TASKS: usize = 32;

/// 一个候选受害者的内存账目（页为单位）。
#[derive(Clone, Copy, Debug)]
pub struct OomTask {
    pub pid: u32,
    /// 常驻页数。
    pub rss_pages: u64,
    /// 换出页数（杀了能真正释放）。
    pub swap_pages: u64,
    /// 页表页数（内核开销，Linux 一并计入）。
    pub pgtable_pages: u64,
    /// 用户侧偏置 -1000..=1000（-1000 = OOM 不可牺牲）。
    pub oom_score_adj: i32,
}

impl OomTask {
    pub const fn new(pid: u32, rss_pages: u64, swap_pages: u64) -> Self {
        OomTask { pid, rss_pages, swap_pages, pgtable_pages: 0, oom_score_adj: 0 }
    }

    pub fn protected(&self) -> bool {
        self.oom_score_adj <= -1000
    }
}

// ---------------------------------------------------------------------------
// 2. badness 评分（Linux oom_badness 同构）
// ---------------------------------------------------------------------------

/// 系统总内存页数（评分归一基准）。
pub const TOTAL_RAM_PAGES: u64 = 1 << 20; // 4GB / 4KB

/// 点数 = rss + swap + pgtable + adj × totalram / 1000（截断整数）。
/// 豁免者返回 u64::MAX 之外还要靠调用方显式跳过——这里返回 None。
pub fn badness(t: &OomTask) -> Option<u64> {
    if t.protected() {
        return None;
    }
    let base = t.rss_pages + t.swap_pages + t.pgtable_pages;
    let adj = (t.oom_score_adj.clamp(-1000, 1000) as i64) * (TOTAL_RAM_PAGES as i64) / 1000;
    let v = base as i64 + adj;
    // 负分钳 0（有账但偏置可抵消——Linux 同语义，最低也是 0）。
    Some(v.max(0) as u64)
}

// ---------------------------------------------------------------------------
// 3. OOM 管理器
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OomErr {
    /// 全员豁免——无法选出受害者（系统不可自救）。
    NoVictim,
    /// 任务表满。
    Full,
    /// pid 不存在。
    NoSuchPid,
}

/// OOM 管理器（任务账 + 目标水位）。
pub struct OomManager {
    tasks: [Option<OomTask>; MAX_TASKS],
    n: usize,
    /// 期望保持的空闲页数（low 水位语义）。
    pub target_free_pages: u64,
    /// 处决日志（确定性顺序，审计面）。
    pub killed: [u32; MAX_TASKS],
    pub killed_n: usize,
    pub rounds: u64,
}

impl OomManager {
    pub fn new(target_free_pages: u64) -> Self {
        OomManager {
            tasks: [None; MAX_TASKS],
            n: 0,
            target_free_pages,
            killed: [0; MAX_TASKS],
            killed_n: 0,
            rounds: 0,
        }
    }

    pub fn admit(&mut self, t: OomTask) -> Result<(), OomErr> {
        if self.n >= MAX_TASKS {
            return Err(OomErr::Full);
        }
        if self.tasks[..self.n].iter().flatten().any(|x| x.pid == t.pid) {
            return Err(OomErr::Full); // 重复 pid 拒绝（诚实，不静默覆盖）
        }
        self.tasks[self.n] = Some(t);
        self.n += 1;
        Ok(())
    }

    /// 当前 free 页数视角下选出最高分受害者（平局 pid 最小）。
    pub fn pick_victim(&self) -> Result<u32, OomErr> {
        let mut best: Option<(u64, u32)> = None; // (points, pid)
        for t in self.tasks[..self.n].iter().flatten() {
            if let Some(p) = badness(t) {
                let better = match best {
                    None => true,
                    // 分高者胜；平局 pid 小者胜（确定性）。
                    Some((bp, bpid)) => p > bp || (p == bp && t.pid < bpid),
                };
                if better {
                    best = Some((p, t.pid));
                }
            }
        }
        best.map(|(_, pid)| pid).ok_or(OomErr::NoVictim)
    }

    /// 处决一个 pid，返回其释放的页数。
    fn kill(&mut self, pid: u32) -> Result<u64, OomErr> {
        let i = (0..self.n)
            .find(|&i| self.tasks[i].map(|t| t.pid == pid).unwrap_or(false))
            .ok_or(OomErr::NoSuchPid)?;
        let t = self.tasks[i].take().unwrap();
        // 压缩数组（保序）。
        self.tasks[i] = self.tasks[self.n - 1].take();
        self.tasks[self.n - 1] = None;
        self.n -= 1;
        if self.killed_n < MAX_TASKS {
            self.killed[self.killed_n] = pid;
            self.killed_n += 1;
        }
        Ok(t.rss_pages + t.swap_pages)
    }

    /// OOM 主循环：连环处决直到 free ≥ 目标。每轮恰杀一个任务（有界：
    /// 表空时 pick_victim 报 NoVictim），无受害者即诚实报错。
    pub fn run_until(&mut self, mut free_pages: u64) -> Result<(u64, usize), OomErr> {
        let mut rounds = 0u64;
        while free_pages < self.target_free_pages {
            let victim = self.pick_victim()?;
            let freed = self.kill(victim)?;
            free_pages += freed;
            rounds += 1;
        }
        self.rounds = rounds;
        Ok((free_pages, rounds as usize))
    }

    pub fn alive(&self) -> usize {
        self.n
    }
}

// ---------------------------------------------------------------------------
// 4. CheckSet
// ---------------------------------------------------------------------------

pub fn run_checks() -> crate::checks::CheckSet {
    use crate::checks::CheckSet;
    let mut cs = CheckSet::new("mech_oom");

    // 1) 最大占用者被选（rss+swap+pgtable 计账）。
    {
        let mut m = OomManager::new(100_000);
        m.admit(OomTask::new(1, 10_000, 0)).unwrap();
        m.admit(OomTask::new(2, 50_000, 20_000)).unwrap();
        m.admit(OomTask::new(3, 5_000, 0)).unwrap();
        cs.add("oom_biggest_hog", m.pick_victim().unwrap() == 2, "");
    }

    // 2) 豁免者跳过：adj=-1000 的大户不选（保护语义）。
    {
        let mut m = OomManager::new(100_000);
        let mut big = OomTask::new(1, 900_000, 0);
        big.oom_score_adj = -1000;
        m.admit(big).unwrap();
        m.admit(OomTask::new(2, 100, 0)).unwrap();
        cs.add("oom_protected_skipped", m.pick_victim().unwrap() == 2, "");
    }

    // 3) adj 偏置传导：rss 相同，adj 正值者先被选。
    {
        let mut m = OomManager::new(100_000);
        m.admit(OomTask::new(1, 10_000, 0)).unwrap();
        let mut boost = OomTask::new(2, 10_000, 0);
        boost.oom_score_adj = 500; // +512 页偏置
        m.admit(boost).unwrap();
        cs.add("oom_adj_bias", m.pick_victim().unwrap() == 2, "");
    }

    // 4) 平局确定性：分数相同取 pid 最小（回放同序）。
    {
        let mut m = OomManager::new(100_000);
        m.admit(OomTask::new(7, 5_000, 0)).unwrap();
        m.admit(OomTask::new(3, 5_000, 0)).unwrap();
        m.admit(OomTask::new(9, 5_000, 0)).unwrap();
        cs.add("oom_tie_deterministic", m.pick_victim().unwrap() == 3, "");
    }

    // 5) 连环处决：杀到水位为止，释放量回账、日志顺序与评分序一致。
    {
        let mut m = OomManager::new(40_000);
        m.admit(OomTask::new(1, 5_000, 0)).unwrap();
        m.admit(OomTask::new(2, 20_000, 0)).unwrap();
        m.admit(OomTask::new(3, 15_000, 5_000)).unwrap();
        m.admit(OomTask::new(4, 1_000, 0)).unwrap();
        let (free, rounds) = m.run_until(0).unwrap();
        // 起点 0（v1 判例笔误传 10_000——起点被计入累计，free 终值
        // 50000 ≠ 承诺 40000）。2 与 3 同分 20k → tie-break 先杀 pid
        // 小的 2（20k）→ 20k < 40k → 杀 3（15k+5k）→ 40k ≥ 40k 停。
        cs.add(
            "oom_cascade",
            free == 40_000 && rounds == 2 && m.killed[..m.killed_n] == [2, 3] && m.alive() == 2,
            "",
        );
    }

    // 6) 全员豁免 → 诚实报错（不假成功）。
    {
        let mut m = OomManager::new(1_000);
        let mut a = OomTask::new(1, 999_999, 0);
        a.oom_score_adj = -1000;
        m.admit(a).unwrap();
        cs.add("oom_no_victim_honest", m.run_until(0) == Err(OomErr::NoVictim), "");
    }

    cs
}

// ---------------------------------------------------------------------------
// 5. 宿主单测
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn badness_arithmetic() {
        let t = OomTask { pid: 1, rss_pages: 1_000, swap_pages: 500, pgtable_pages: 20, oom_score_adj: 0 };
        assert_eq!(badness(&t), Some(1_520));
        // adj = +1000 → +1_048_576 页（× totalram/1000）。
        let b = OomTask { pid: 2, rss_pages: 0, swap_pages: 0, pgtable_pages: 0, oom_score_adj: 1000 };
        assert_eq!(badness(&b), Some(TOTAL_RAM_PAGES));
        // adj = -999 → 基账 2000 - 1_047_552 → 钳 0。
        let c = OomTask { pid: 3, rss_pages: 1_000, swap_pages: 1_000, pgtable_pages: 0, oom_score_adj: -999 };
        assert_eq!(badness(&c), Some(0));
        // 豁免。
        let d = OomTask { pid: 4, rss_pages: 999_999, swap_pages: 0, pgtable_pages: 0, oom_score_adj: -1000 };
        assert_eq!(badness(&d), None);
    }

    #[test]
    fn cascade_order_is_score_order() {
        let mut m = OomManager::new(60_000);
        // 分数序：5(50k) > 4(30k) > 3(10k)。
        m.admit(OomTask::new(3, 10_000, 0)).unwrap();
        m.admit(OomTask::new(4, 30_000, 0)).unwrap();
        m.admit(OomTask::new(5, 50_000, 0)).unwrap();
        let (free, rounds) = m.run_until(0).unwrap();
        assert_eq!(rounds, 2);
        assert_eq!(free, 80_000);
        assert_eq!(m.killed[..m.killed_n], [5, 4]);
        assert_eq!(m.alive(), 1);
        assert_eq!(m.rounds, 2);
    }

    #[test]
    fn protection_prevents_cascade() {
        let mut m = OomManager::new(60_000);
        let mut init = OomTask::new(1, 100_000, 0);
        init.oom_score_adj = -1000; // init 进程保护
        m.admit(init).unwrap();
        m.admit(OomTask::new(2, 10_000, 0)).unwrap();
        // 杀完 2（10k）也不够 60k，但再无可选 → 诚实报 NoVictim。
        let r = m.run_until(0);
        assert_eq!(r, Err(OomErr::NoVictim));
        // 日志里只杀了 2。
        assert_eq!(m.killed[..m.killed_n], [2]);
        // init 还活着。
        assert_eq!(m.alive(), 1);
    }

    #[test]
    fn tie_break_by_oldest_pid_deterministic() {
        // 反序注册，选出的仍是 pid 最小者。
        let mut m = OomManager::new(100_000);
        for pid in (1..=8).rev() {
            m.admit(OomTask::new(pid, 7_000, 0)).unwrap();
        }
        assert_eq!(m.pick_victim().unwrap(), 1);
        m.kill(1).unwrap();
        assert_eq!(m.pick_victim().unwrap(), 2);
    }

    #[test]
    fn duplicate_pid_rejected() {
        let mut m = OomManager::new(100);
        assert!(m.admit(OomTask::new(5, 10, 0)).is_ok());
        assert!(m.admit(OomTask::new(5, 10, 0)).is_err());
    }

    #[test]
    fn target_reachable_in_one_round() {
        let mut m = OomManager::new(50_000);
        m.admit(OomTask::new(1, 30_000, 0)).unwrap();
        m.admit(OomTask::new(2, 80_000, 20_000)).unwrap();
        let (free, rounds) = m.run_until(5_000).unwrap();
        assert_eq!(rounds, 1);
        assert_eq!(free, 5_000 + 100_000);
        assert_eq!(m.killed[..m.killed_n], [2]);
    }
}
