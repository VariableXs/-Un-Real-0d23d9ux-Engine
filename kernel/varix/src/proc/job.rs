//! 任务42 · Job 限额——语义对齐宿主 `src-tauri/src/shell/isolation.rs` 的
//! `IsolationLimits`（memory_bytes / cpu_percent / kill_on_drop≡KILL_ON_JOB_CLOSE）。
//!
//! 三语义（与宿主逐条对齐）：
//! - **memory_bytes**：成员页账本字节记账（job_mem_charge/release 由 ring3
//!   装载/回收路径调用）；超限 charge 返回 false=分配被拒（Job Office 拒绝
//!   语义，宿主同款「限额无效即拒绝」）。
//! - **cpu_percent**：每 `CPU_WINDOW_TICKS` 调度 tick 一个滚动窗口，预算 =
//!   percent × 窗口 / 100；超额线程由 `on_tick` 把 slice_left 清零自然抢占
//!   （不引入新抢占路径）。窗口按全局 tick 单调滚动重置。
//! - **KILL_ON_JOB_CLOSE**：job_close 即杀——全部成员 kill flag 置位，
//!   syscall 边界（syscall_common 头）查获 → user_exit；页账本走正常退出
//!   回收（"空间随进程消亡，一页不留"同款）。
//!
//! 栈纪律：全表 static，零堆大物化；成员表定长（JOB_MEMBERS_MAX）。

use core::sync::atomic::{AtomicU32, Ordering};

/// Job 槽数（10 软件并发压力留余量）。
pub const JOB_MAX: usize = 16;
/// 单 Job 成员上限。
pub const JOB_MEMBERS_MAX: usize = 16;
/// 成员（tid）→ Job 映射表宽度 = 调度器 MAX_THREADS。
pub const MEMBER_MAP_MAX: usize = 64;
/// CPU 限速窗口（调度 tick）。
pub const CPU_WINDOW_TICKS: u32 = 100;

/// 限额三元组（宿主 IsolationLimits 的内核对齐面；io_priority 在内核侧
/// 由 SchedClass 权重承担，不重复建模）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct JobLimits {
    pub memory_bytes: u64,
    pub cpu_percent: u32,
    pub kill_on_close: bool,
}

impl JobLimits {
    pub const fn new(memory_bytes: u64, cpu_percent: u32, kill_on_close: bool) -> Self {
        JobLimits { memory_bytes, cpu_percent, kill_on_close }
    }

    /// 校验：对齐宿主 validate()——0/非法值拒绝。
    pub fn valid(&self) -> bool {
        self.memory_bytes > 0 && self.cpu_percent > 0 && self.cpu_percent <= 100
    }

    /// 本窗口 CPU 预算（tick 数）。
    pub const fn window_budget(&self) -> u32 {
        self.cpu_percent * CPU_WINDOW_TICKS / 100
    }
}

struct JobEntry {
    used: bool,
    limits: JobLimits,
    mem_used: u64,
    /// 本窗口已耗 CPU tick。
    cpu_used: u32,
    /// 窗口起点（全局 tick）；越过窗口即重置 cpu_used。
    window_start: u64,
    members: [u32; JOB_MEMBERS_MAX],
    member_count: usize,
}

impl JobEntry {
    const fn new() -> JobEntry {
        JobEntry {
            used: false,
            limits: JobLimits::new(0, 0, false),
            mem_used: 0,
            cpu_used: 0,
            window_start: 0,
            members: [0; JOB_MEMBERS_MAX],
            member_count: 0,
        }
    }
}

static JOBS: crate::cpu::sync::SpinProtected<[JobEntry; JOB_MAX]> =
    crate::cpu::sync::SpinProtected::new([const { JobEntry::new() }; JOB_MAX]);
/// tid → job slot + 1（0 = 无 Job）。
static MEMBER_JOB: [AtomicU32; MEMBER_MAP_MAX] = {
    #[allow(clippy::declare_interior_mutable_const)]
    const Z: AtomicU32 = AtomicU32::new(0);
    [Z; MEMBER_MAP_MAX]
};
/// kill flag（tid 槽位）：job_close 置位，syscall 边界查获清零。
static KILL_FLAG: [AtomicU32; MEMBER_MAP_MAX] = {
    #[allow(clippy::declare_interior_mutable_const)]
    const Z: AtomicU32 = AtomicU32::new(0);
    [Z; MEMBER_MAP_MAX]
};
/// 全局调度 tick（窗口滚动用；on_tick 挂钩喂入）。
static GLOBAL_TICK: AtomicU32 = AtomicU32::new(0);

fn member_slot(tid: u32) -> Option<usize> {
    (tid > 0 && tid < MEMBER_MAP_MAX as u32).then_some(tid as usize)
}

/// 创建 Job（返回 1-based id；0=表满或限额非法——拒绝语义对齐宿主 validate）。
pub fn job_create(limits: JobLimits) -> u32 {
    if !limits.valid() {
        return 0;
    }
    let mut jobs = JOBS.lock();
    for (i, e) in jobs.iter_mut().enumerate() {
        if !e.used {
            *e = JobEntry {
                used: true,
                limits,
                mem_used: 0,
                cpu_used: 0,
                window_start: GLOBAL_TICK.load(Ordering::Relaxed) as u64,
                members: [0; JOB_MEMBERS_MAX],
                member_count: 0,
            };
            return (i + 1) as u32;
        }
    }
    0
}

/// 成员加入（重复加入幂等拒绝返回 false；tid 越界/Job 不存在拒绝）。
pub fn job_assign(job_id: u32, tid: u32) -> bool {
    let Some(ms) = member_slot(tid) else { return false };
    if job_id == 0 || job_id as usize > JOB_MAX {
        return false;
    }
    let mut jobs = JOBS.lock();
    let e = &mut jobs[job_id as usize - 1];
    if !e.used || e.member_count >= JOB_MEMBERS_MAX {
        return false;
    }
    if e.members[..e.member_count].contains(&tid) {
        return false;
    }
    e.members[e.member_count] = tid;
    e.member_count += 1;
    MEMBER_JOB[ms].store(job_id, Ordering::Release);
    true
}

/// tid 所属 Job（0 = 无）。
pub fn job_of(tid: u32) -> u32 {
    member_slot(tid).map_or(0, |ms| MEMBER_JOB[ms].load(Ordering::Acquire))
}

/// 内存记账：超限 false（分配拒绝）。无 Job = 不限额（true）。
pub fn job_mem_charge(tid: u32, bytes: u64) -> bool {
    let job = job_of(tid);
    if job == 0 {
        return true;
    }
    let mut jobs = JOBS.lock();
    let e = &mut jobs[job as usize - 1];
    if !e.used {
        return true;
    }
    match e.mem_used.checked_add(bytes) {
        Some(next) if next <= e.limits.memory_bytes => {
            e.mem_used = next;
            true
        }
        _ => false,
    }
}

/// 内存归还（页账本回收路径）。
pub fn job_mem_release(tid: u32, bytes: u64) {
    let job = job_of(tid);
    if job == 0 {
        return;
    }
    let mut jobs = JOBS.lock();
    let e = &mut jobs[job as usize - 1];
    if e.used {
        e.mem_used = e.mem_used.saturating_sub(bytes);
    }
}

/// Job 内存水位 (used, limit)。
pub fn job_mem_usage(job_id: u32) -> Option<(u64, u64)> {
    if job_id == 0 || job_id as usize > JOB_MAX {
        return None;
    }
    let jobs = JOBS.lock();
    let e = &jobs[job_id as usize - 1];
    e.used.then_some((e.mem_used, e.limits.memory_bytes))
}

/// CPU 限速记账：窗口滚动重置；超额 false（on_tick 据此清 slice 抢占）。
/// 无 Job 成员也推进全局钟（窗口时钟与调度 tick 同源）。
pub fn job_cpu_charge(tid: u32, ticks: u32) -> bool {
    let job = job_of(tid);
    if job == 0 {
        GLOBAL_TICK.fetch_add(ticks, Ordering::Relaxed);
        return true;
    }
    let now = GLOBAL_TICK.fetch_add(ticks, Ordering::Relaxed) as u64;
    let mut jobs = JOBS.lock();
    let e = &mut jobs[job as usize - 1];
    if !e.used {
        return true;
    }
    if now >= e.window_start + CPU_WINDOW_TICKS as u64 {
        e.window_start = now;
        e.cpu_used = 0;
    }
    e.cpu_used = e.cpu_used.saturating_add(ticks);
    e.cpu_used <= e.limits.window_budget()
}

/// KILL_ON_JOB_CLOSE：关闭 Job（杀全部成员）。返回被杀成员数。
/// 关闭后槽数据清空、成员映射与 kill flag 落位（syscall 边界查获）。
pub fn job_close(job_id: u32) -> usize {
    if job_id == 0 || job_id as usize > JOB_MAX {
        return 0;
    }
    let mut jobs = JOBS.lock();
    let e = &mut jobs[job_id as usize - 1];
    if !e.used {
        return 0;
    }
    let killed = e.member_count;
    for i in 0..e.member_count {
        let tid = e.members[i];
        if let Some(ms) = member_slot(tid) {
            MEMBER_JOB[ms].store(0, Ordering::Release);
            if e.limits.kill_on_close {
                KILL_FLAG[ms].store(job_id, Ordering::Release);
            }
        }
    }
    *e = JobEntry::new();
    killed
}

/// syscall 边界查获：kill flag 置位即读走（清零）并返回 true → 调用方 user_exit。
pub fn job_kill_pending(tid: u32) -> bool {
    let Some(ms) = member_slot(tid) else { return false };
    KILL_FLAG[ms].swap(0, Ordering::AcqRel) != 0
}

/// 演示收官清理：探针进程全部收割后，残留 kill flag 属于已死成员（再无 syscall
/// 可消费），不清会误杀下一个进入 ring3 的进程 —— 任务27 实机教训：ushell 的
/// 首个 SYS_FRAME 被任务42 jobkill 演示的残留 flag 误杀（exit(0)）。
pub fn job_flags_reset_for_demo() {
    for f in KILL_FLAG.iter() {
        f.store(0, Ordering::Release);
    }
}

/// 成员主动退出（正常 exit 路径）：摘除映射（不动 kill flag 语义）。
pub fn job_detach(tid: u32) {
    let Some(ms) = member_slot(tid) else { return };
    let job = MEMBER_JOB[ms].swap(0, Ordering::AcqRel);
    if job == 0 {
        return;
    }
    let mut jobs = JOBS.lock();
    let e = &mut jobs[job as usize - 1];
    if e.used {
        if let Some(pos) = e.members[..e.member_count].iter().position(|&t| t == tid) {
            e.members.copy_within(pos + 1..e.member_count, pos);
            e.member_count -= 1;
        }
    }
}

/// 成员数（压力与泄漏断言用）。
pub fn job_member_count(job_id: u32) -> usize {
    if job_id == 0 || job_id as usize > JOB_MAX {
        return 0;
    }
    let jobs = JOBS.lock();
    let e = &jobs[job_id as usize - 1];
    if e.used { e.member_count } else { 0 }
}

/// 占用槽数（句柄泄漏断言：全部 close 后应归零）。
pub fn job_slots_used() -> usize {
    JOBS.lock().iter().filter(|e| e.used).count()
}

// ---------------------------------------------------------------------------
// 宿主测试（语义矩阵全量；实机链路见 ring3/job 探针）
// ---------------------------------------------------------------------------
#[cfg(test)]
mod tests {
    use super::*;

    // 测试用独立 tid 段，避免与其他模块用例串扰。
    const T0: u32 = 40;
    fn tid(k: u32) -> u32 {
        T0 + k
    }

    #[test]
    fn limits_validate_rejects_zero_and_over() {
        assert!(!JobLimits::new(0, 30, true).valid());
        assert!(!JobLimits::new(1024, 0, true).valid());
        assert!(!JobLimits::new(1024, 101, true).valid());
        assert!(JobLimits::new(1024, 100, true).valid());
    }

    #[test]
    fn create_full_table_and_reuse() {
        let mut made = Vec::new();
        for _ in 0..JOB_MAX {
            let id = job_create(JobLimits::new(1 << 20, 50, true));
            assert!(id != 0);
            made.push(id);
        }
        assert_eq!(job_create(JobLimits::new(1 << 20, 50, true)), 0); // 表满
        assert_eq!(job_slots_used(), JOB_MAX);
        for id in made {
            assert_eq!(job_close(id), 0);
        }
        assert_eq!(job_slots_used(), 0); // 全关零泄漏
    }

    #[test]
    fn assign_member_and_no_leak_on_close() {
        let id = job_create(JobLimits::new(4096, 50, true));
        assert!(job_assign(id, tid(1)));
        assert!(!job_assign(id, tid(1))); // 重复幂等拒绝
        assert!(job_assign(id, tid(2)));
        assert_eq!(job_member_count(id), 2);
        assert_eq!(job_of(tid(1)), id);
        // close 杀 2 成员 + 映射清空 + kill flag 落位
        assert_eq!(job_close(id), 2);
        assert_eq!(job_member_count(id), 0);
        assert_eq!(job_of(tid(1)), 0);
        assert!(job_kill_pending(tid(1)));
        assert!(job_kill_pending(tid(2)));
        assert!(!job_kill_pending(tid(1))); // swap 读走即清零
    }

    #[test]
    fn mem_charge_over_limit_denied_and_release_recovers() {
        let id = job_create(JobLimits::new(4096, 50, true));
        assert!(job_assign(id, tid(3)));
        assert!(job_mem_charge(tid(3), 4096));
        assert!(!job_mem_charge(tid(3), 1)); // 超限拒绝
        job_mem_release(tid(3), 2048);
        assert!(job_mem_charge(tid(3), 2048));
        assert_eq!(job_mem_usage(id), Some((4096, 4096)));
        job_mem_release(tid(3), 4096);
        assert_eq!(job_mem_usage(id), Some((0, 4096)));
        job_close(id);
    }

    #[test]
    fn no_job_is_unlimited() {
        assert!(job_mem_charge(tid(9), u64::MAX / 2));
        assert!(job_cpu_charge(tid(9), 1_000_000));
        assert_eq!(job_of(tid(9)), 0);
    }

    #[test]
    fn cpu_rate_window_refill() {
        let id = job_create(JobLimits::new(1 << 20, 30, true)); // 30/100 tick
        assert!(job_assign(id, tid(4)));
        for _ in 0..30 {
            assert!(job_cpu_charge(tid(4), 1));
        }
        assert!(!job_cpu_charge(tid(4), 1)); // 第 31 tick 超额
        // 窗口滚动（全局 tick 前进 >100）→ 预算重置
        for _ in 0..CPU_WINDOW_TICKS {
            job_cpu_charge(tid(5), 1); // 无 Job 成员只推全局钟
        }
        assert!(job_cpu_charge(tid(4), 1));
        job_close(id);
    }

    #[test]
    fn kill_on_close_false_leaves_no_flag() {
        let id = job_create(JobLimits::new(1 << 20, 50, false)); // 不带 KILL
        assert!(job_assign(id, tid(6)));
        assert_eq!(job_close(id), 1);
        assert!(!job_kill_pending(tid(6))); // kill_on_close=false → 无 flag
    }

    #[test]
    fn detach_removes_member_cleanly() {
        let id = job_create(JobLimits::new(1 << 20, 50, true));
        assert!(job_assign(id, tid(7)));
        assert!(job_assign(id, tid(8)));
        job_detach(tid(7));
        assert_eq!(job_member_count(id), 1);
        assert_eq!(job_of(tid(7)), 0);
        assert!(!job_kill_pending(tid(7))); // 正常退出无 kill flag
        job_detach(tid(8));
        assert_eq!(job_member_count(id), 0);
        job_close(id);
    }

    #[test]
    fn boundary_and_bogus_args() {
        assert_eq!(job_create(JobLimits::new(0, 50, true)), 0);
        assert!(!job_assign(0, tid(1)));
        assert!(!job_assign(999, tid(1)));
        assert!(!job_assign(1, 0)); // tid 0 非法
        assert!(!job_assign(1, 10_000)); // 越界
        assert_eq!(job_close(0), 0);
        assert_eq!(job_close(999), 0);
        assert_eq!(job_mem_usage(999), None);
        assert!(!job_kill_pending(0));
        assert!(!job_kill_pending(10_000));
    }

    #[test]
    fn multi_job_isolation() {
        let a = job_create(JobLimits::new(1024, 20, true));
        let b = job_create(JobLimits::new(2048, 90, true));
        assert!(job_assign(a, tid(10)));
        assert!(job_assign(b, tid(11)));
        assert!(job_mem_charge(tid(10), 1024));
        assert!(!job_mem_charge(tid(11), 4096)); // b 限 2048
        assert!(job_mem_charge(tid(11), 2048));
        assert_eq!(job_mem_usage(a), Some((1024, 1024)));
        assert_eq!(job_mem_usage(b), Some((2048, 2048)));
        assert_eq!(job_close(a), 1);
        assert_eq!(job_close(b), 1);
    }
}
