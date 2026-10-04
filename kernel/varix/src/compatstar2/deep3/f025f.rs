//! F025 深化批次四 · 假脱机与配额面（compatstar2/deep3 · G-A-25）。
//!
//! 主层 printpdf.rs 覆盖打印队列面板/PDF 流/产物落盘，批次三深化覆盖
//! DEVMODE 印刷参数与页范围边界；本批补齐主册【功能定义】「全语义对齐」的
//! 序列化/账本面：spool 作业记录序列化（作业头 id/优先级/页数/用户槽位
//! 定长打包 round-trip）、作业优先级队列（定长 16，同优先级 FIFO 稳定）、
//! 每用户配额账（页数配额与已用，超额拒绝并计数）、页数估算器（由页范围
//! 段表计算总页数，重叠段拒绝）。
//!
//! 判据对账：主册 G-A-25（三款开源程序打印到 PDF 全流程绿/打印队列多任务
//! 排队取消重打）+ MS 打印假脱机（spooler job 语义：作业头、优先级调度、
//! 配额拒绝）对拍。与主层 printpdf.rs、deep2/f025e.rs（纸张尺寸/取向份数
//! 协商）语义面互补不重叠。零堆纪律：定长数组 + &'static str，错误显性化。

use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// spool 作业记录序列化（16 字节定长包 round-trip）
// ---------------------------------------------------------------------------

/// 作业头定长包长度（小端：id 4 + priority 1 + pages 2 + user_slot 1 +
/// 预留 8 必零——预留非零即显性 Err，防字段演进被静默吞）。
pub const JOB_REC_LEN: usize = 16;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SpoolJob {
    pub id: u32,
    /// 优先级：数值越大越先出队（MS spooler PRIORITY 口径，1..=9）。
    pub priority: u8,
    pub pages: u16,
    /// 用户槽位（配额账下标）。
    pub user_slot: u8,
}

/// 作业头 → 16 字节定长包（小端序列化）。
pub fn pack_job(j: &SpoolJob) -> [u8; JOB_REC_LEN] {
    let mut out = [0u8; JOB_REC_LEN];
    out[0..4].copy_from_slice(&j.id.to_le_bytes());
    out[4] = j.priority;
    out[5..7].copy_from_slice(&j.pages.to_le_bytes());
    out[7] = j.user_slot;
    out
}

/// 16 字节定长包 → 作业头；长度不符/预留非零显性 Err。
pub fn parse_job(buf: &[u8]) -> Result<SpoolJob, &'static str> {
    if buf.len() != JOB_REC_LEN {
        return Err("bad-record-len");
    }
    if buf[8..16].iter().any(|&b| b != 0) {
        return Err("bad-reserved");
    }
    let mut id_b = [0u8; 4];
    id_b.copy_from_slice(&buf[0..4]);
    let mut pages_b = [0u8; 2];
    pages_b.copy_from_slice(&buf[5..7]);
    Ok(SpoolJob { id: u32::from_le_bytes(id_b), priority: buf[4], pages: u16::from_le_bytes(pages_b), user_slot: buf[7] })
}

// ---------------------------------------------------------------------------
// 作业优先级队列（定长 16，同优先级 FIFO 稳定）
// ---------------------------------------------------------------------------

/// 队列容量。
pub const MAX_QUEUE_JOBS: usize = 16;

/// 优先级队列：内部按优先级降序保位（稳定插入——同优先级新作业排在既有
/// 同级之后，FIFO 保序），队首恒为最高优先级最早入队作业。
pub struct SpoolQueue {
    slots: [Option<SpoolJob>; MAX_QUEUE_JOBS],
    pub count: usize,
}

impl SpoolQueue {
    pub const fn new() -> Self {
        SpoolQueue { slots: [None; MAX_QUEUE_JOBS], count: 0 }
    }

    /// 入队；满则显性 Err（零静默）。
    pub fn enqueue(&mut self, job: SpoolJob) -> Result<(), &'static str> {
        if self.count >= MAX_QUEUE_JOBS {
            return Err("queue-full");
        }
        let mut at = self.count;
        for i in 0..self.count {
            let p = self.slots[i].map(|s| s.priority).unwrap_or(0);
            if p < job.priority {
                at = i;
                break;
            }
        }
        let mut i = self.count;
        while i > at {
            self.slots[i] = self.slots[i - 1];
            i -= 1;
        }
        self.slots[at] = Some(job);
        self.count += 1;
        Ok(())
    }

    /// 出队最高优先级最早作业；空队显性 Err。
    pub fn dequeue(&mut self) -> Result<SpoolJob, &'static str> {
        if self.count == 0 {
            return Err("queue-empty");
        }
        let job = self.slots[0].unwrap_or(SpoolJob { id: 0, priority: 0, pages: 0, user_slot: 0 });
        for i in 1..self.count {
            self.slots[i - 1] = self.slots[i];
        }
        self.slots[self.count - 1] = None;
        self.count -= 1;
        Ok(job)
    }
}

// ---------------------------------------------------------------------------
// 每用户配额账（页数配额与已用，超额拒绝并计数）
// ---------------------------------------------------------------------------

/// 配额账用户槽位容量。
pub const MAX_QUOTA_USERS: usize = 8;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct UserQuota {
    pub page_quota: u32,
    pub pages_used: u32,
    /// 超额拒绝计数（账面即诊断——不静默丢任务）。
    pub over_rejects: u32,
}

/// 每用户配额账。
pub struct QuotaBook {
    users: [Option<UserQuota>; MAX_QUOTA_USERS],
}

impl QuotaBook {
    pub const fn new() -> Self {
        QuotaBook { users: [None; MAX_QUOTA_USERS] }
    }

    /// 注册用户配额；槽位越界/重复注册显性 Err。
    pub fn enroll(&mut self, slot: u8, page_quota: u32) -> Result<(), &'static str> {
        if slot as usize >= MAX_QUOTA_USERS {
            return Err("user-out-of-range");
        }
        if self.users[slot as usize].is_some() {
            return Err("user-exists");
        }
        self.users[slot as usize] = Some(UserQuota { page_quota, pages_used: 0, over_rejects: 0 });
        Ok(())
    }

    /// 尝试消费页数：恰好用满配额放行，超额拒绝并计数（边界含语义：
    /// used + pages <= quota 才放行）。
    pub fn try_print(&mut self, slot: u8, pages: u32) -> Result<u32, &'static str> {
        if slot as usize >= MAX_QUOTA_USERS {
            return Err("user-out-of-range");
        }
        let q = match self.users[slot as usize].as_mut() {
            Some(q) => q,
            None => return Err("user-unknown"),
        };
        if q.pages_used + pages > q.page_quota {
            q.over_rejects += 1;
            return Err("quota-exceeded");
        }
        q.pages_used += pages;
        Ok(q.pages_used)
    }

    pub fn user(&self, slot: u8) -> Result<UserQuota, &'static str> {
        if slot as usize >= MAX_QUOTA_USERS {
            return Err("user-out-of-range");
        }
        self.users[slot as usize].ok_or("user-unknown")
    }
}

// ---------------------------------------------------------------------------
// 页数估算器（页范围段表 → 总页数；重叠/非法段拒绝）
// ---------------------------------------------------------------------------

/// 估算页范围段表（闭区间 [start, end]，页号 1 起算——DEVMODE 页范围口径）。
pub fn estimate_pages(segs: &[(u32, u32)]) -> Result<u32, &'static str> {
    let mut total = 0u32;
    let mut last_end = 0u32;
    for &(start, end) in segs {
        if start == 0 || end < start {
            return Err("bad-range");
        }
        // 段表须按页号递增且互不重叠；start <= last_end 同时捕获乱序与
        // 重叠（含端点相接页——同一页两段计即重叠）。
        if start <= last_end {
            return Err("range-overlap");
        }
        total += end - start + 1;
        last_end = end;
    }
    Ok(total)
}

/// 域自检（深化批次四）。
pub fn run_f025f_checks() -> CheckSet {
    let mut cs = CheckSet::new("F025-spool-d4");
    // 1) 作业头 pack/parse round-trip（字段全保真）。
    let job = SpoolJob { id: 0x0042_0007, priority: 7, pages: 128, user_slot: 3 };
    let back = parse_job(&pack_job(&job)).expect("定长包解析必成");
    cs.add("job_pack_roundtrip", back == job, "");
    // 2) 非法定长/预留非零显性 Err。
    let mut bad = pack_job(&job);
    bad[9] = 0xFF;
    cs.add("job_parse_bad", parse_job(&bad) == Err("bad-reserved")
        && parse_job(&[0u8; 15]) == Err("bad-record-len"), "");
    // 3) 优先级队列 + 同优先级 FIFO 稳定：P1(A) P3(B) P2(C) P3(D) →
    //    出队序 B、D、C、A（B 先于 D——同级按入队序）。
    let mut q = SpoolQueue::new();
    let _ = q.enqueue(SpoolJob { id: 1, priority: 1, pages: 1, user_slot: 0 });
    let _ = q.enqueue(SpoolJob { id: 2, priority: 3, pages: 1, user_slot: 0 });
    let _ = q.enqueue(SpoolJob { id: 3, priority: 2, pages: 1, user_slot: 0 });
    let _ = q.enqueue(SpoolJob { id: 4, priority: 3, pages: 1, user_slot: 0 });
    let seq = [q.dequeue().unwrap_or_default().id, q.dequeue().unwrap_or_default().id,
        q.dequeue().unwrap_or_default().id, q.dequeue().unwrap_or_default().id];
    cs.add("queue_priority_fifo_stable", seq == [2, 4, 3, 1], "");
    // 4) 队满显性 Err。
    let mut q2 = SpoolQueue::new();
    let mut full = true;
    for _ in 0..MAX_QUEUE_JOBS {
        full &= q2.enqueue(SpoolJob { id: 0, priority: 5, pages: 1, user_slot: 0 }).is_ok();
    }
    cs.add("queue_full_err", full && q2.enqueue(SpoolJob { id: 0, priority: 9, pages: 1, user_slot: 0 }) == Err("queue-full"), "");
    // 5) 配额记账：注册/消费/读回。
    let mut book = QuotaBook::new();
    let _ = book.enroll(0, 1000);
    let used = book.try_print(0, 400).expect("配额内必成");
    let u = book.user(0).expect("已注册");
    cs.add("quota_consume_ledger", used == 400 && u.pages_used == 400 && u.page_quota == 1000, "");
    // 6) 超额拒绝并计数；恰好用满放行（边界含）。
    let over = book.try_print(0, 601);
    let edge = book.try_print(0, 600);
    let u2 = book.user(0).expect("已注册");
    cs.add("quota_over_reject_counted", over == Err("quota-exceeded")
        && edge == Ok(1000) && u2.over_rejects == 1 && u2.pages_used == 1000, "");
    // 7) 未注册用户/槽位越界显性 Err。
    let mut book2 = QuotaBook::new();
    let _ = book2.enroll(1, 100);
    cs.add("quota_unknown_err", book2.try_print(0, 1) == Err("user-unknown")
        && book2.enroll(8, 1) == Err("user-out-of-range")
        && book2.enroll(1, 1) == Err("user-exists"), "");
    // 8) 页数估算：1-5 页 + 8-10 页 → 5+3=8。
    let segs = [(1u32, 5u32), (8u32, 10u32)];
    cs.add("estimate_pages_sum", estimate_pages(&segs) == Ok(8), "");
    // 9) 重叠段/乱序段拒绝（5-9 与 1-5 端点相接即重叠）。
    let overlap = [(1u32, 5u32), (5u32, 9u32)];
    let unordered = [(8u32, 10u32), (1u32, 5u32)];
    cs.add("estimate_overlap_reject", estimate_pages(&overlap) == Err("range-overlap")
        && estimate_pages(&unordered) == Err("range-overlap"), "");
    // 10) 非法段（页号 0 起 / start>end）拒绝。
    let zero = [(0u32, 3u32)];
    let inverted = [(7u32, 3u32)];
    cs.add("estimate_bad_range", estimate_pages(&zero) == Err("bad-range")
        && estimate_pages(&inverted) == Err("bad-range"), "");
    cs
}

/// 出队兜底缺省（测试侧 unwrap_or_default 需要；域内不产生此值）。
impl Default for SpoolJob {
    fn default() -> Self {
        SpoolJob { id: 0, priority: 0, pages: 0, user_slot: 0 }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn queue_drains_all_in_stable_order() {
        let mut q = SpoolQueue::new();
        // 混合优先级入队 6 作业，逐个出队须全序稳定且计数归零。
        let _ = q.enqueue(SpoolJob { id: 10, priority: 2, pages: 1, user_slot: 0 });
        let _ = q.enqueue(SpoolJob { id: 11, priority: 5, pages: 1, user_slot: 0 });
        let _ = q.enqueue(SpoolJob { id: 12, priority: 5, pages: 1, user_slot: 0 });
        let _ = q.enqueue(SpoolJob { id: 13, priority: 9, pages: 1, user_slot: 0 });
        let _ = q.enqueue(SpoolJob { id: 14, priority: 2, pages: 1, user_slot: 0 });
        let _ = q.enqueue(SpoolJob { id: 15, priority: 5, pages: 1, user_slot: 0 });
        let order: Vec<u32> = (0..6).map(|_| q.dequeue().expect("非空必成").id).collect();
        assert_eq!(order, vec![13, 11, 12, 15, 10, 14]);
        assert_eq!(q.count, 0);
        assert_eq!(q.dequeue(), Err("queue-empty"));
    }

    #[test]
    fn quota_exact_boundary_then_reject() {
        let mut book = QuotaBook::new();
        book.enroll(3, 50).expect("注册必成");
        assert_eq!(book.try_print(3, 50), Ok(50), "恰好用满配额放行");
        assert_eq!(book.try_print(3, 1), Err("quota-exceeded"), "超一页即拒");
        assert_eq!(book.user(3).expect("已注册").over_rejects, 1);
    }

    #[test]
    fn deep4_checks_all_green() {
        let cs = run_f025f_checks();
        assert!(cs.all_passed() && !cs.truncated());
    }
}
