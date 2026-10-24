//! CGPU-F0198 · 异步编译与渲染重叠（判据段 0x0600）
//! 着色器编译与渲染的异步重叠：编译不阻塞渲染（新 shader 编译期间
//! 用旧 shader 继续——编译完成后原子切换 F0024 联动）、编译任务在
//! 帧图中作为后台优先级节点（编译池独立——不占渲染线程池）、编译
//! 完成回调（编译完成→管线创建→热切换的全链自动化）。判据：异步
//! 编译链、后台优先级、原子切换、编译渲染重叠收益、零卡顿断言。
//!
//! 诊断码段 0x0600（本模块独占：0x0600~0x060F）。
//! no_std + alloc 零 unwrap，全部 Result 化。

use alloc::vec;
use alloc::vec::Vec;

/// 诊断码段基址。
pub const SEG_BASE: u16 = 0x0600;

/// 渲染线程池与编译池隔离：编译池占比上限（千分比，≤250‰）。
pub const COMPILE_POOL_MAX_PERMILLE: u32 = 250;

/// 编译优先级（后台优先级节点：低于全部渲染任务）。
pub const COMPILE_PRIO: u8 = 255;

/// 渲染任务最低优先级。
pub const RENDER_PRIO_MIN: u8 = 0;

/// Shader 编译任务（判据「异步编译链」原子量）。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct CompileJob {
    /// 任务号。
    pub job_id: u32,
    /// 关联管线号。
    pub pipeline: u32,
    /// 预估编译耗时（微秒）。
    pub compile_us: u32,
    /// 是否热切换目标（替换正在使用的旧管线）。
    pub hot_swap: bool,
}

impl CompileJob {
    /// 合法性。
    pub fn validate(&self) -> Result<(), u16> {
        if self.job_id == 0 || self.pipeline == 0 || self.compile_us == 0 {
            return Err(SEG_BASE);
        }
        Ok(())
    }
}

/// 编译池（判据「后台优先级」载体）：独立于渲染线程池的队列。
#[derive(Default)]
pub struct CompilePool {
    queue: Vec<CompileJob>,
    /// 已完成并等待切换的作业。
    ready: Vec<CompileJob>,
    /// 池总容量份额（微秒/帧预算）。
    pub budget_us: u32,
}

impl CompilePool {
    /// 建池（预算合法性）。
    pub fn new(budget_us: u32) -> Result<Self, u16> {
        if budget_us == 0 {
            return Err(SEG_BASE + 1);
        }
        Ok(Self { queue: Vec::new(), ready: Vec::new(), budget_us })
    }
    /// 入队（重复 job 拒绝）。
    pub fn submit(&mut self, j: CompileJob) -> Result<(), u16> {
        j.validate()?;
        if self.queue.iter().chain(self.ready.iter()).any(|x| x.job_id == j.job_id) {
            return Err(SEG_BASE + 2);
        }
        self.queue.push(j);
        Ok(())
    }
    /// 队列深度。
    pub fn depth(&self) -> usize {
        self.queue.len()
    }
    /// 就绪数。
    pub fn ready_count(&self) -> usize {
        self.ready.len()
    }
    /// 执行一帧的编译配额：按 FIFO 消费预算，返回本轮完成的作业
    /// （判据「后台优先级」——编译只消费配额，绝不挤占渲染）。
    pub fn run_quota(&mut self, frame_budget_us: u32) -> Result<Vec<CompileJob>, u16> {
        if frame_budget_us > self.budget_us {
            return Err(SEG_BASE + 3);
        }
        let mut spent: u32 = 0;
        let mut done: Vec<CompileJob> = Vec::new();
        loop {
            let Some(front) = self.queue.first() else { break };
            if spent + front.compile_us > frame_budget_us {
                break;
            }
            let j = self.queue.remove(0);
            spent += j.compile_us;
            done.push(j);
        }
        for j in done.iter() {
            self.ready.push(j.clone());
        }
        Ok(done)
    }
}

/// 双缓冲管线槽位（判据「原子切换」载体）：旧管线号 + 新管线号，
/// 切换是一次性原子翻转。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct PipelineSlot {
    /// 正在使用的管线号。
    pub active: u32,
    /// 编译中的新管线号（0 = 无）。
    pub pending: u32,
}

impl PipelineSlot {
    /// 槽位合法性。
    pub fn validate(&self) -> Result<(), u16> {
        if self.active == 0 {
            return Err(SEG_BASE + 4);
        }
        if self.pending != 0 && self.pending == self.active {
            return Err(SEG_BASE + 5);
        }
        Ok(())
    }
}

/// 原子切换（判据「原子切换」核心）：pending 编译完成且有效才允许
/// 翻转；翻转后 pending 清零、active 换新。单步完成 = 原子性（无
/// 中间态暴露）。
pub fn atomic_swap(slot: &mut PipelineSlot, done_pipeline: u32) -> Result<u32, u16> {
    slot.validate()?;
    if slot.pending == 0 {
        return Err(SEG_BASE + 6);
    }
    if done_pipeline != slot.pending {
        return Err(SEG_BASE + 7);
    }
    let old = slot.active;
    slot.active = done_pipeline;
    slot.pending = 0;
    Ok(old)
}

/// 编译-渲染重叠收益（判据「编译渲染重叠收益」核心量）：编译耗时
/// 与渲染耗时可并行重叠时，总耗时 = max(编译, 渲染) 而非和。返回
/// 节省千分比。
pub fn overlap_saving_permille(compile_us: u64, render_us: u64, serialized_us: u64) -> Result<u32, u16> {
    if compile_us == 0 || render_us == 0 {
        return Err(SEG_BASE + 8);
    }
    let overlapped = compile_us.max(render_us);
    if serialized_us != compile_us + render_us {
        return Err(SEG_BASE + 9);
    }
    if overlapped >= serialized_us {
        return Err(SEG_BASE + 10);
    }
    Ok((((serialized_us - overlapped) * 1000) / serialized_us) as u32)
}

/// 零卡顿断言（判据「零卡顿断言」主判据）：渲染帧时间不受编译池
/// 活动影响的恒等式——帧耗时仅由渲染任务决定，编译池每帧消费
/// 不得触及渲染预算。传入帧渲染耗时与编译消费，断言帧时间平稳
/// （帧间波动 ≤ 50‰）。
pub fn assert_no_stall(frame_times_us: &[u64], compile_spent_us: &[u32]) -> Result<u32, u16> {
    if frame_times_us.len() != compile_spent_us.len() || frame_times_us.len() < 3 {
        return Err(SEG_BASE + 11);
    }
    if frame_times_us.iter().any(|&t| t == 0) {
        return Err(SEG_BASE + 12);
    }
    let sum: u64 = frame_times_us.iter().sum();
    let avg = sum / frame_times_us.len() as u64;
    let max_dev = frame_times_us
        .iter()
        .map(|&t| t.abs_diff(avg))
        .max()
        .unwrap_or(0);
    let dev_permille = (max_dev * 1000) / avg;
    // 波动超限且编译消费非零 → 卡顿嫌疑。
    let any_compile = compile_spent_us.iter().any(|&c| c > 0);
    if dev_permille > 50 && any_compile {
        return Err(SEG_BASE + 13);
    }
    Ok(dev_permille as u32)
}

/// 编译完成回调链（判据「编译完成回调」）：编译完成 → 管线创建 →
/// 热切换登记的三段式链表：每段有前置段号（0 = 链头），链完整性
/// 校验——段序号连续且无断链。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct CallbackStage {
    /// 段号（1 起）。
    pub stage: u32,
    /// 前置段号（0 = 链头）。
    pub prev: u32,
    /// 动作类别：0=编译完成 1=管线创建 2=热切换登记。
    pub action: u8,
}

impl CallbackStage {
    /// 合法性。
    pub fn validate(&self) -> Result<(), u16> {
        if self.stage == 0 || self.action > 2 {
            return Err(SEG_BASE + 14);
        }
        if self.prev >= self.stage {
            return Err(SEG_BASE + 15);
        }
        Ok(())
    }
}

/// 回调链完整性校验（判据「编译完成回调」主判据）：段必须从 1
/// 连续编号、除链头外每段前置恰好是上一段、且动作类别呈 0→1→2
/// 循环序（同管线）。返回链长。
pub fn callback_chain_valid(chain: &[CallbackStage]) -> Result<usize, u16> {
    if chain.is_empty() {
        return Err(SEG_BASE + 16);
    }
    for (i, s) in chain.iter().enumerate() {
        s.validate()?;
        if s.stage != (i + 1) as u32 {
            return Err(SEG_BASE + 17);
        }
        let expect_prev = if i == 0 { 0 } else { (i) as u32 };
        if s.prev != expect_prev {
            return Err(SEG_BASE + 18);
        }
        let expect_action = (i % 3) as u8;
        if s.action != expect_action {
            return Err(SEG_BASE + 19);
        }
    }
    Ok(chain.len())
}

/// 编译池占比审计（判据「后台优先级」反证面）：编译池每帧消费占
/// 渲染预算的比例不得超过 COMPILE_POOL_MAX_PERMILLE。
pub fn pool_share_audit(compile_spent_us: u32, render_budget_us: u32) -> Result<u32, u16> {
    if render_budget_us == 0 {
        return Err(SEG_BASE + 20);
    }
    let share = (compile_spent_us as u64 * 1000 / render_budget_us as u64) as u32;
    if share > COMPILE_POOL_MAX_PERMILLE {
        return Err(SEG_BASE + 21);
    }
    Ok(share)
}

/// 编译池账指纹（可审计面）。
pub fn compile_pool_fp(p: &CompilePool) -> u64 {
    let mut acc: u64 = 0xcbf29ce484222325;
    for j in p.queue.iter().chain(p.ready.iter()) {
        acc = acc.wrapping_mul(0x100000001b3) ^ j.job_id as u64;
        acc = acc.wrapping_mul(0x100000001b3) ^ j.pipeline as u64;
        acc = acc.wrapping_mul(0x100000001b3) ^ j.compile_us as u64;
    }
    acc
}

/// 重叠时序模型（独立函数面）：渲染主轴时间线上插入编译后台波，
/// 返回帧终点时间 = 渲染总耗时（编译并行不延长主轴）——这是
/// 「编译不阻塞渲染」的时序恒等式。
pub fn timeline_overlap_model(
    render_segments_us: &[u64],
    compile_us: u64,
) -> Result<(u64, u64), u16> {
    if render_segments_us.is_empty() || render_segments_us.iter().any(|&s| s == 0) {
        return Err(SEG_BASE + 22);
    }
    if compile_us == 0 {
        return Err(SEG_BASE + 23);
    }
    let render_total: u64 = render_segments_us.iter().sum();
    Ok((render_total, compile_us))
}

#[cfg(test)]
mod tests {
    use super::*;
    fn job(id: u32, pipe: u32, us: u32) -> CompileJob {
        CompileJob { job_id: id, pipeline: pipe, compile_us: us, hot_swap: true }
    }
    #[test]
    fn pool_quota_fifo() {
        let mut p = CompilePool::new(1000).unwrap();
        p.submit(job(1, 10, 400)).unwrap();
        p.submit(job(2, 10, 400)).unwrap();
        p.submit(job(3, 11, 400)).unwrap();
        let done = p.run_quota(900).unwrap();
        assert_eq!(done.len(), 2);
        assert_eq!(p.depth(), 1);
        assert_eq!(p.ready_count(), 2);
    }
    #[test]
    fn pool_rejects_dup() {
        let mut p = CompilePool::new(1000).unwrap();
        p.submit(job(1, 10, 100)).unwrap();
        assert!(p.submit(job(1, 10, 100)).is_err());
    }
    #[test]
    fn atomic_swap_flips_once() {
        let mut s = PipelineSlot { active: 7, pending: 9 };
        let old = atomic_swap(&mut s, 9).unwrap();
        assert_eq!(old, 7);
        assert_eq!(s.active, 9);
        assert_eq!(s.pending, 0);
        // 无 pending 再切 → 拒绝。
        assert!(atomic_swap(&mut s, 9).is_err());
    }
    #[test]
    fn overlap_saving_math() {
        // 编译 800 + 渲染 1000 串行 1800，重叠后 1000 → 节省 444‰。
        let s = overlap_saving_permille(800, 1000, 1800).unwrap();
        assert_eq!(s, 444);
        assert!(overlap_saving_permille(800, 1000, 1700).is_err());
    }
    #[test]
    fn no_stall_assertion() {
        let frames = vec![10_000, 10_100, 9_950, 10_050];
        let spent = vec![200, 200, 200, 200];
        let dev = assert_no_stall(&frames, &spent).unwrap();
        assert!(dev <= 50);
        // 编译挤占导致帧时间抖动 → 拒绝。
        let bad = vec![10_000, 15_000, 9_000, 10_500];
        assert!(assert_no_stall(&bad, &spent).is_err());
    }
    #[test]
    fn callback_chain_three_stage() {
        let chain = vec![
            CallbackStage { stage: 1, prev: 0, action: 0 },
            CallbackStage { stage: 2, prev: 1, action: 1 },
            CallbackStage { stage: 3, prev: 2, action: 2 },
        ];
        assert_eq!(callback_chain_valid(&chain).unwrap(), 3);
        // 断链 → 拒绝。
        let broken = vec![
            CallbackStage { stage: 1, prev: 0, action: 0 },
            CallbackStage { stage: 2, prev: 5, action: 1 },
        ];
        assert!(callback_chain_valid(&broken).is_err());
    }
    #[test]
    fn pool_share_cap() {
        assert_eq!(pool_share_audit(200, 1000).unwrap(), 200);
        assert!(pool_share_audit(300, 1000).is_err());
    }
    #[test]
    fn timeline_overlap_identity() {
        let (r, c) = timeline_overlap_model(&[300, 400, 300], 700).unwrap();
        assert_eq!((r, c), (1000, 700));
    }
}
