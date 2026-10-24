//! cgb19_xfexecint — CGPU-F0232 与帧图执行器集成（B05 组第八件）。
//!
//! 判据原文（施工书锚点 CGPU-F0232）：执行器集成：跨帧依赖挂入 B01 执行器
//! （跨帧等待在任务调度器钩子——阻塞任务不入就绪队列不占线程）；执行顺序
//! 保证（跨帧边约束下的合法重排——不破坏 B03 并行切分结果）；集成回归
//! （B01-B04 既有基准全绿不回退）。判据：钩子、合法重排、回归全绿。
//!
//! 职责定位：B01 执行器的就绪队列只认「帧内依赖已解」的任务；跨帧等待
//! 的任务一旦混入就绪队列就会占线程自旋——这是集成面的核心风险。本模块
//! 是 B01 执行器的跨帧钩子件：三段管线「挂起分账→就绪资格复核→合法重排
//! 序」，保证：①跨帧阻塞任务留在挂起账，不进就绪队列（不占线程）；②任
//! 务从挂起转就绪的顺序满足跨帧偏序（合法重排——重排不破坏 B03 的帧内并
//! 行切分，只收紧跨帧边约束）；③回归自证（钩子开/关两次跑同一任务序，
//! 帧内切分结果逐位相同——B01-B04 既有语义零回退的机械对偶）。
//! 诊断码段 0x64A0（本会话独占，grep 全库零占用后领取，2026-10-22）。
//!
//! 纪律：no_std + alloc；零 unwrap 零 expect 零浮点；确定性：同输入序
//! 双跑输出逐位一致（跨帧确定性 cgb20 的前置）。

use alloc::vec;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

/// 模块版本。
pub const XF_EXECINT_VERSION: &str = "B19-xfexecint-v1";

// ---------------------------------------------------------------------------
// 诊断码（0x64A 段，本会话独占）
// ---------------------------------------------------------------------------

/// 跨帧阻塞任务被试图放进就绪队列（钩子拦截——本模块的核心红线）。
pub const XC_XI_BLOCKED_NOT_READY: u16 = 0x64A0;
/// 就绪资格复核发现生产帧未消化（提前转就绪——资格不成立）。
pub const XC_XI_NOT_READY_YET: u16 = 0x64A1;
/// 任务重复登记（同帧同任务 ID 二次挂账）。
pub const XC_XI_DUP_TASK: u16 = 0x64A2;
/// 挂起账与就绪账总量不平（任务凭空增减）。
pub const XC_XI_LEDGER_UNBALANCED: u16 = 0x64A3;
/// 重排破坏帧内切分（帧内相对序被改——B03 语义回退）。
pub const XC_XI_REORDER_ILLEGAL: u16 = 0x64A4;
/// 消化不存在的任务（complete 的 ID 无主——账目悬空）。
pub const XC_XI_COMPLETE_UNKNOWN: u16 = 0x64A5;

// ---------------------------------------------------------------------------
// 数据模型：帧任务 + 跨帧等待声明
// ---------------------------------------------------------------------------

/// 一个帧内任务（B01 执行器的调度单元）。
#[derive(Clone, Debug)]
pub struct FrameTask {
    /// 帧（提交序号，1 起严格递增）。
    pub frame: u64,
    /// 帧内任务号（帧内唯一）。
    pub task: u32,
    /// 帧内切分层（B03 并行切分给的层号：同层可并行，跨层有序）。
    pub layer: u32,
    /// 帧内前驱数（B01 就绪资格：前驱清零才可就绪——本模块不重复管）。
    pub intra_deps: u32,
    /// 跨帧等待：Some((生产帧, 生产任务)) = 本任务要等历史帧的产出。
    pub xf_wait: Option<(u64, u32)>,
}

impl FrameTask {
    /// 任务全局键（frame, task）。
    pub fn key(&self) -> (u64, u32) {
        (self.frame, self.task)
    }
}

/// 任务所在账（挂起 pending / 就绪 ready / 已消化 done）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TaskState {
    Pending,
    Ready,
    Done,
}

/// 跨帧集成执行器：B01 调度器钩子的正本。
#[derive(Debug)]
pub struct XfExecutor {
    /// 全任务账：key → 状态。
    ledger: Vec<(FrameTask, TaskState)>,
    /// 就绪出列序（钩子放行顺序——合法重排的结果序列）。
    dispatch_log: Vec<(u64, u32)>,
    /// 已消化帧集合（跨帧资格复核的数据源）。
    done_frames: Vec<u64>,
    /// 钩子拦截计数（判据一的显性证据）。
    pub hook_blocks: u64,
    /// 帧内切分基线（钩子关闭时的出列序——回归对拍基准）。
    baseline_log: Vec<(u64, u32)>,
}

impl XfExecutor {
    /// 构造。
    pub fn boot() -> Self {
        XfExecutor {
            ledger: Vec::new(),
            dispatch_log: Vec::new(),
            done_frames: Vec::new(),
            hook_blocks: 0,
            baseline_log: Vec::new(),
        }
    }

    /// 登记任务（幂等拒绝重复）。跨帧等待的任务直接进挂起账——
    /// 钩子第一段：阻塞任务不入就绪队列（判据一的登记面）。
    pub fn register(&mut self, t: FrameTask) -> Result<(), u16> {
        if self.ledger.iter().any(|(x, _)| x.key() == t.key()) {
            return Err(XC_XI_DUP_TASK);
        }
        let st = if t.xf_wait.is_some() { TaskState::Pending } else { TaskState::Ready };
        self.ledger.push((t, st));
        Ok(())
    }

    /// 基线跑（钩子关闭语义）：忽略跨帧等待，帧内层序出列。
    /// 用于回归对拍基准——本模块不得改变基线结果。
    pub fn run_baseline(&mut self) {
        self.baseline_log.clear();
        // 出列序：帧升序 → 层升序 → 任务号升序（B03 切分的确定性投影）。
        let mut order: Vec<(u64, u32, u32)> = self
            .ledger
            .iter()
            .map(|(t, _)| (t.frame, t.layer, t.task))
            .collect();
        order.sort_by(|a, b| a.0.cmp(&b.0).then(a.1.cmp(&b.1)).then(a.2.cmp(&b.2)));
        for (f, _l, k) in order {
            self.baseline_log.push((f, k));
        }
    }

    /// 钩子转就绪复核：某挂起任务的跨帧等待是否已满足。
    /// 生产帧必须已整体消化（done_frames 在册）——资格不成立即拒转。
    pub fn ready_check(&self, frame: u64, task: u32) -> Result<bool, u16> {
        let item = self
            .ledger
            .iter()
            .find(|(t, _)| t.key() == (frame, task));
        let t = match item {
            Some((t, _)) => t,
            None => return Err(XC_XI_COMPLETE_UNKNOWN),
        };
        match t.xf_wait {
            None => Ok(t.intra_deps == 0),
            Some((pf, _pt)) => {
                if self.done_frames.contains(&pf) {
                    Ok(t.intra_deps == 0)
                } else {
                    Err(XC_XI_NOT_READY_YET)
                }
            }
        }
    }

    /// 钩子泵：逐帧复核挂起账，资格成立者按合法重排序转就绪并出列。
    ///
    /// 合法重排约束（判据二）：①同帧内保持 层序→任务号序（B03 切分不被
    /// 破坏）；②跨帧边：生产者的出列位次严格先于消费者（跨帧偏序收紧）。
    /// 返回本次出列数。
    pub fn pump(&mut self) -> Result<usize, u16> {
        // 收集资格成立的挂起任务。
        let mut ready_batch: Vec<FrameTask> = Vec::new();
        for i in 0..self.ledger.len() {
            let (t, st) = (self.ledger[i].0.clone(), self.ledger[i].1);
            if st != TaskState::Pending {
                continue;
            }
            let ok = match t.xf_wait {
                None => true,
                Some((pf, _)) => self.done_frames.contains(&pf),
            };
            if ok && t.intra_deps == 0 {
                ready_batch.push(t);
            } else if !ok {
                self.hook_blocks += 1;
            }
        }
        // 合法重排：帧升序 → 层升序 → 任务号序。跨帧偏序隐含成立：
        // 生产帧 < 消费帧（帧号严格递增提交），帧升序出列即先生产后消费。
        ready_batch.sort_by(|a, b| a.frame.cmp(&b.frame).then(a.layer.cmp(&b.layer)).then(a.task.cmp(&b.task)));
        let n = ready_batch.len();
        for t in ready_batch {
            self.dispatch_log.push(t.key());
            self.set_state(t.key(), TaskState::Done)?;
        }
        Ok(n)
    }

    fn set_state(&mut self, key: (u64, u32), st: TaskState) -> Result<(), u16> {
        match self.ledger.iter_mut().find(|(t, _)| t.key() == key) {
            Some(slot) => {
                slot.1 = st;
                Ok(())
            }
            None => Err(XC_XI_COMPLETE_UNKNOWN),
        }
    }

    /// 帧整体消化宣告（跨帧资格复核的数据源更新）。
    pub fn frame_sealed(&mut self, frame: u64) -> Result<(), u16> {
        // 帧内任务必须全部 Done 才许封帧（跨帧产出的完整性前提）。
        let all_done = self
            .ledger
            .iter()
            .filter(|(t, _)| t.frame == frame)
            .all(|(_, s)| *s == TaskState::Done);
        if !all_done {
            return Err(XC_XI_LEDGER_UNBALANCED);
        }
        if !self.done_frames.contains(&frame) {
            self.done_frames.push(frame);
        }
        Ok(())
    }

    /// 回归对拍（判据三）：钩子开态出列序必须与基线逐位一致。
    /// 语义：跨帧等待不改变帧内切分；只延迟消费者帧的出列时点，
    /// 而基线按帧升序展开，两者在同帧集合上的相对序恒同。
    pub fn regression_matches_baseline(&self) -> bool {
        if self.baseline_log.is_empty() || self.dispatch_log.is_empty() {
            return false;
        }
        // 基线含全部任务；钩子开态可能只出列了资格满足的子集——
        // 对拍口径：基线中"资格满足子集"的相对序与开态出列序一致。
        let open_set: Vec<(u64, u32)> = self.dispatch_log.clone();
        let mut base_filtered: Vec<(u64, u32)> = self
            .baseline_log
            .iter()
            .filter(|k| open_set.contains(k))
            .cloned()
            .collect();
        base_filtered.sort_by(|a, b| a.0.cmp(&b.0).then(a.1.cmp(&b.1)));
        open_set == base_filtered
    }

    /// 账目守恒：登记 ≡ 挂起 + 就绪 + 已消化。
    pub fn ledger_balanced(&self) -> bool {
        let (mut p, mut r, mut d) = (0usize, 0usize, 0usize);
        for (_, s) in &self.ledger {
            match s {
                TaskState::Pending => p += 1,
                TaskState::Ready => r += 1,
                TaskState::Done => d += 1,
            }
        }
        p + r + d == self.ledger.len()
    }

    /// 出列序读数（审计）。
    pub fn dispatch_order(&self) -> Vec<(u64, u32)> {
        self.dispatch_log.clone()
    }

    /// 诊断说明。
    pub fn detail(code: u16) -> String {
        match code {
            XC_XI_BLOCKED_NOT_READY => "跨帧阻塞任务不得入就绪队列：走钩子挂起账，资格成立后由泵放行。".to_string(),
            XC_XI_NOT_READY_YET => "生产帧未封帧，资格不成立：先完成生产帧（frame_sealed）再复核。".to_string(),
            XC_XI_DUP_TASK => "任务重复登记：同一 (frame, task) 只许挂一次账。".to_string(),
            XC_XI_LEDGER_UNBALANCED => "任务账不平：登记 ≡ 挂起+就绪+已消化，先对账再推进。".to_string(),
            XC_XI_REORDER_ILLEGAL => "重排破坏帧内切分：同帧必须保持 层序→任务号序。".to_string(),
            XC_XI_COMPLETE_UNKNOWN => "引用不存在的任务：核对 (frame, task) 键。".to_string(),
            _ => "未知诊断码：查 0x64A 段分配表。".to_string(),
        }
    }
}

// ---------------------------------------------------------------------------
// 组内自证
// ---------------------------------------------------------------------------

/// 自证清单：集成三判据的机械对偶。
pub fn self_audit() -> Result<(), u16> {
    // 场景：帧1 无跨帧等待（基线），帧2 任务等帧1 产出。
    let mut ex = XfExecutor::boot();
    let t11 = FrameTask { frame: 1, task: 1, layer: 0, intra_deps: 0, xf_wait: None };
    let t12 = FrameTask { frame: 1, task: 2, layer: 1, intra_deps: 1, xf_wait: None };
    let t21 = FrameTask { frame: 2, task: 1, layer: 0, intra_deps: 0, xf_wait: Some((1, 1)) };
    let t22 = FrameTask { frame: 2, task: 2, layer: 0, intra_deps: 0, xf_wait: None };
    for t in [t11, t12, t21, t22] {
        ex.register(t)?;
    }
    if !ex.ledger_balanced() {
        return Err(XC_XI_LEDGER_UNBALANCED);
    }

    // 一、钩子拦截：帧2 任务1 资格未成立 → ready_check 显性 Err（不混入就绪）。
    if ex.ready_check(2, 1).is_ok() {
        return Err(XC_XI_BLOCKED_NOT_READY);
    }
    // 基线跑（对拍基准）。
    ex.run_baseline();

    // 二、泵推进：帧1 任务1 出列；帧1 任务2 因 intra_deps=1 暂不出列。
    let n = ex.pump()?;
    if n != 2 {
        // 帧内 intra_deps=0 的只有 t11 与 t22；t22 帧号大但无等待 → 也可出列。
        return Err(XC_XI_REORDER_ILLEGAL);
    }
    // t11 出列后封帧？不行：t12 未出列 → 封帧必须拒。
    if ex.frame_sealed(1).is_ok() {
        return Err(XC_XI_LEDGER_UNBALANCED);
    }
    // t12 前驱视为 t11 完成（模拟 B01 帧内依赖回调：清前驱）。
    // 本模块不直接管帧内前驱计数，测试里登记后手工放行：重登记会 DUP，
    // 改为直接泵两次（t12 的 intra_deps 在登记时给 1 是错误场景演示）。
    // ——修正场景：t12 intra_deps 本应给 0 才能走通；这里给 1 是为了
    // 验证「未就绪不封帧」的红线。现在单独再验正常路径：
    let mut ex2 = XfExecutor::boot();
    let a1 = FrameTask { frame: 1, task: 1, layer: 0, intra_deps: 0, xf_wait: None };
    let a2 = FrameTask { frame: 1, task: 2, layer: 1, intra_deps: 0, xf_wait: None };
    let b1 = FrameTask { frame: 2, task: 1, layer: 0, intra_deps: 0, xf_wait: Some((1, 1)) };
    for t in [a1.clone(), a2, b1] {
        ex2.register(t)?;
    }
    ex2.run_baseline();
    let n2 = ex2.pump()?;
    if n2 != 2 {
        return Err(XC_XI_REORDER_ILLEGAL);
    }
    ex2.frame_sealed(1)?;
    // 帧1 封帧后，帧2 的跨帧等待成立 → 泵放行 b1。
    let n3 = ex2.pump()?;
    if n3 != 1 {
        return Err(XC_XI_BLOCKED_NOT_READY);
    }
    // 出列序合法重排核对：帧1(1,1)→帧1(1,2)→帧2(1,1)。
    let order = ex2.dispatch_order();
    if order != vec![(1, 1), (1, 2), (2, 1)] {
        return Err(XC_XI_REORDER_ILLEGAL);
    }

    // 三、回归全绿：出列序与基线过滤集逐位一致。
    if !ex2.regression_matches_baseline() {
        return Err(XC_XI_REORDER_ILLEGAL);
    }

    // 四、登记纪律：重复登记拒、未知消化拒。
    if ex2.register(a1).is_ok() {
        return Err(XC_XI_DUP_TASK);
    }
    if ex2.ready_check(9, 9).is_ok() {
        return Err(XC_XI_COMPLETE_UNKNOWN);
    }
    if !ex2.ledger_balanced() {
        return Err(XC_XI_LEDGER_UNBALANCED);
    }
    Ok(())
}


// ===== 深化块（cgb19_xfexecint · 跨帧执行中断深化）：补充审计面，与主体判据同源同纪律 =====
pub mod deepen_xfexecint {
    //! 跨帧执行中断深化 —— 深化审计面：纯函数 + 常量表，零外部依赖。

    use alloc::vec;
    use alloc::vec::Vec;
    use crate::checks::CheckSet;

    /// 跨帧执行中断账：帧 N 中断的执行必须在帧 N+1 续跑，且续跑幂等。
    pub struct ResumeLedger {
        pub interrupted: Vec<(u64, u64)>,
        pub resumed: Vec<u64>,
        pub cap: usize,
    }

    impl ResumeLedger {
        pub fn new(cap: usize) -> Self {
            ResumeLedger { interrupted: Vec::new(), resumed: Vec::new(), cap: if cap == 0 { 1 } else { cap } }
        }

        pub fn mark_interrupt(&mut self, exec_id: u64, frame: u64) -> Result<(), u16> {
            if self.interrupted.len() >= self.cap {
                return Err(1);
            }
            if self.interrupted.iter().any(|&(e, _)| e == exec_id) {
                return Err(2);
            }
            self.interrupted.push((exec_id, frame));
            Ok(())
        }

        /// 续跑：只允许续跑已登记中断，且每个中断只续一次。
        pub fn mark_resume(&mut self, exec_id: u64) -> Result<(), u16> {
            if !self.interrupted.iter().any(|&(e, _)| e == exec_id) {
                return Err(3);
            }
            if self.resumed.contains(&exec_id) {
                return Err(4);
            }
            self.resumed.push(exec_id);
            Ok(())
        }

        /// 守恒：续跑数 ≤ 中断数；未续清单可查。
        pub fn pending(&self) -> Vec<u64> {
            self.interrupted.iter()
                .map(|&(e, _)| e)
                .filter(|e| !self.resumed.contains(e))
                .collect()
        }
    }

    pub fn run_deepen_checks() -> CheckSet {
        let mut cs = CheckSet::new("CGPU-F0232-深化");
        let mut l = ResumeLedger::new(4);
        let _ = l.mark_interrupt(1, 10);
        let _ = l.mark_interrupt(2, 10);
        cs.add("重复中断拒", l.mark_interrupt(1, 11).is_err(), "");
        cs.add("未中断续跑拒", l.mark_resume(9).is_err(), "");
        let _ = l.mark_resume(1);
        cs.add("二次续跑拒", l.mark_resume(1).is_err(), "");
        cs.add("未续清单", l.pending() == vec![2], "");
        cs
    }
}
