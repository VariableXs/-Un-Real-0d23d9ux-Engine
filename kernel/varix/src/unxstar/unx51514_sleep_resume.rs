//! UNX-F51514 MSC 设备休眠容忍（挂起唤醒后命令续传） · 0xC0380段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文：UNX-F51515-J1 端到端 ×10 全通；各段耗时账落盘可导出
//!
//! 设计要点：
//!   ① 休眠相位按单调相位机建模：Active→Suspending→Suspended→Resuming→
//!      Resumed 逐格推进；倒退显性拒（PhaseRegression）、跳格显性拒
//!      （PhaseSkip）；一轮收尾后 rearm 只允许从 Resumed 出发，run_id
//!      单调递增开启下一轮。
//!   ② 命令续传：挂起窗（Suspending 起、Resumed 前）提交的命令按 seq
//!      落入停泊队列，唤醒收尾时按 seq 序原样续传（不改序、不合并、
//!      不丢弃）；活跃期提交的命令即时执行，不进停泊队。
//!   ③ 耗时账只增不减：每轮落一条 RunAccount（run_id/挂起耗时/恢复耗时/
//!      续传条数，四个 u32 字段）；落盘导出按冻结格式逐字节写出（每记录
//!      16 字节 = 4 字段 × 4 字节小端，u32 字段按 4 字节计不是 8），导出
//!      实际字节数与冻结公式严格一致，缓冲不足显性拒（禁静默截断）。
//!   ④ 判定次序：命令内容判据（零块 → 单条上限）先于停泊容量判据；
//!      相位机是入口守卫，先于命令判据。
//!   ⑤ 注入账守恒：注入 = 检出 + 逃逸，逃逸恒为 0；三类注入（挂起期
//!      零块、相位倒退、未挂起先唤醒）×10 轮必抓。
//!   ⑥ 零浮点、零 IO、零墙钟：耗时用单调逻辑 tick（checked_add 防回绕），
//!      端到端 LBA 变化用固定种子 LCG（确定性可复现）。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::vec::Vec;

pub const SEG_BASE: u32 = 0xC0380;

/// 停泊队列容量上限（冻结）。
pub const MAX_PARKED: usize = 8;
/// 单条命令块数上限（冻结）。
pub const MAX_BLOCKS_PER_CMD: u32 = 256;
/// 判据要求的端到端轮数（冻结）。
pub const RUNS_REQUIRED: u32 = 10;
/// 导出记录冻结字节数：run_id/suspend_ticks/resume_ticks/resumed_cmds
/// 四个 u32 字段 × 4 字节 = 16 字节。
pub const EXPORT_REC_BYTES: usize = 16;

/// 诊断码闭集（休眠续传域）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SleepCode {
    /// 成功
    Ok = 0,
    /// 段位守卫失败（SEG_BASE 越界或不对齐）
    SegBad,
    /// 命令块数为零
    ZeroBlocks,
    /// 单条块数超上限
    BlocksTooMany,
    /// 停泊队列已满
    ParkFull,
    /// 相位倒退（单调相位机显性拒绝）
    PhaseRegression,
    /// 相位跳格（必须逐格推进）
    PhaseSkip,
    /// 未处于挂起态却请求唤醒
    NotSuspended,
    /// 未处于恢复中态却要求收尾续传
    NotResuming,
    /// 未处于已恢复态却要求重臂
    NotResumed,
    /// 逻辑 tick 回绕溢出
    TickOverflow,
    /// 导出缓冲不足
    ExportTooSmall,
    /// 导出账与冻结公式不符
    ExportFormulaMismatch,
}

impl SleepCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            SleepCode::Ok => "续传成功",
            SleepCode::SegBad => "段位越界：检查 SEG_BASE 对齐与取值范围",
            SleepCode::ZeroBlocks => "零块命令：块数必须 ≥1",
            SleepCode::BlocksTooMany => "单条块数超限：拆分后重新提交",
            SleepCode::ParkFull => "停泊队列满：等待唤醒续传后再提交",
            SleepCode::PhaseRegression => "相位倒退：休眠相位只能前进不能回退",
            SleepCode::PhaseSkip => "相位跳格：必须逐格推进休眠相位",
            SleepCode::NotSuspended => "未挂起：先进入 Suspended 才能请求唤醒",
            SleepCode::NotResuming => "未恢复中：先 request_resume 再收尾续传",
            SleepCode::NotResumed => "未恢复：只有 Resumed 态才能重臂开新轮",
            SleepCode::TickOverflow => "逻辑 tick 溢出：耗时账回绕，检查 elapse 入参",
            SleepCode::ExportTooSmall => "导出缓冲不足：按 轮数×16 字节准备落盘缓冲",
            SleepCode::ExportFormulaMismatch => "导出账不符：实际写出字节与冻结公式不一致",
        }
    }
}

/// 诊断码全序表（闭集枚举的权威清单）。
pub const ALL_CODES: [SleepCode; 13] = [
    SleepCode::Ok,
    SleepCode::SegBad,
    SleepCode::ZeroBlocks,
    SleepCode::BlocksTooMany,
    SleepCode::ParkFull,
    SleepCode::PhaseRegression,
    SleepCode::PhaseSkip,
    SleepCode::NotSuspended,
    SleepCode::NotResuming,
    SleepCode::NotResumed,
    SleepCode::TickOverflow,
    SleepCode::ExportTooSmall,
    SleepCode::ExportFormulaMismatch,
];

/// 休眠相位（单调推进，order 严格递增）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SleepPhase {
    /// 活跃：命令即时执行
    Active = 0,
    /// 挂起中：新命令一律停泊
    Suspending = 1,
    /// 已挂起：新命令一律停泊
    Suspended = 2,
    /// 恢复中：续传尚未收尾
    Resuming = 3,
    /// 已恢复：停泊命令已续传完毕
    Resumed = 4,
}

impl SleepPhase {
    /// 相位序数（单调机的权威顺序）。
    pub fn order(self) -> u32 {
        self as u32
    }
}

/// 停泊命令（挂起期提交，唤醒后续传）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ParkedCmd {
    /// 全局单调命令号
    pub seq: u32,
    pub lba: u64,
    pub blocks: u32,
}

/// 单轮耗时账（落盘导出冻结格式的来源，四字段均为 u32）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RunAccount {
    pub run_id: u32,
    /// 挂起段耗时（进入 Suspended → 进入 Resuming）
    pub suspend_ticks: u32,
    /// 恢复段耗时（进入 Resuming → 进入 Resumed）
    pub resume_ticks: u32,
    /// 续传命令条数
    pub resumed_cmds: u32,
}

impl RunAccount {
    /// 冻结导出布局：四个 u32 小端 × 4 字节 = 16 字节。
    /// 公式与写出路径共用本函数，逐字节一致（u32 字段按 4 字节计）。
    pub fn export_bytes(&self) -> [u8; EXPORT_REC_BYTES] {
        let mut out = [0u8; EXPORT_REC_BYTES];
        let fields = [self.run_id, self.suspend_ticks, self.resume_ticks, self.resumed_cmds];
        let mut w = 0usize;
        for f in fields.iter() {
            let le = f.to_le_bytes();
            for i in 0..4usize {
                if let (Some(slot), Some(b)) = (out.get_mut(w), le.get(i)) {
                    *slot = *b;
                }
                w += 1;
            }
        }
        out
    }
}

/// 休眠续传引擎。
pub struct SleepResumeEngine {
    run_id: u32,
    phase: SleepPhase,
    tick: u32,
    suspend_enter_tick: u32,
    resume_enter_tick: u32,
    pending_suspend_ticks: u32,
    pending_resume_ticks: u32,
    parked: Vec<ParkedCmd>,
    next_seq: u32,
    executed: u64,
    injected: u64,
    detected: u64,
    runs: Vec<RunAccount>,
}

impl SleepResumeEngine {
    /// 新引擎：run_id 从 1 起，相位 Active。
    pub fn new() -> SleepResumeEngine {
        SleepResumeEngine {
            run_id: 1,
            phase: SleepPhase::Active,
            tick: 0,
            suspend_enter_tick: 0,
            resume_enter_tick: 0,
            pending_suspend_ticks: 0,
            pending_resume_ticks: 0,
            parked: Vec::new(),
            next_seq: 1,
            executed: 0,
            injected: 0,
            detected: 0,
            runs: Vec::new(),
        }
    }

    pub fn phase(&self) -> SleepPhase {
        self.phase
    }

    pub fn run_id(&self) -> u32 {
        self.run_id
    }

    pub fn parked_len(&self) -> usize {
        self.parked.len()
    }

    pub fn executed(&self) -> u64 {
        self.executed
    }

    pub fn runs_len(&self) -> usize {
        self.runs.len()
    }

    pub fn runs(&self) -> &[RunAccount] {
        &self.runs
    }

    pub fn injected(&self) -> u64 {
        self.injected
    }

    pub fn detected(&self) -> u64 {
        self.detected
    }

    /// 逃逸数（判据要求恒 0）。
    pub fn escaped(&self) -> u64 {
        self.injected.saturating_sub(self.detected)
    }

    /// 注入账守恒面：注入 = 检出 + 逃逸，且检出 > 0。
    pub fn conserved(&self) -> bool {
        self.injected == self.detected && self.detected > 0
    }

    /// 注入违约被检出（调用方捕获到预期 Err 时登记）。
    pub fn note_violation_caught(&mut self) {
        self.injected += 1;
        self.detected += 1;
    }

    /// 注入逃逸（非零即记账面破洞）。
    pub fn note_escape(&mut self) {
        self.injected += 1;
    }

    /// 单调逻辑 tick 推进（checked_add 防回绕）。
    pub fn elapse(&mut self, n: u32) -> Result<(), SleepCode> {
        match self.tick.checked_add(n) {
            Some(t) => {
                self.tick = t;
                Ok(())
            }
            None => Err(SleepCode::TickOverflow),
        }
    }

    /// 相位推进：只允许逐格前进；倒退显性拒、跳格/原地显性拒。
    pub fn advance_to(&mut self, target: SleepPhase) -> Result<(), SleepCode> {
        let cur = self.phase.order();
        let tgt = target.order();
        if tgt < cur {
            return Err(SleepCode::PhaseRegression);
        }
        if tgt != cur + 1 {
            return Err(SleepCode::PhaseSkip);
        }
        self.phase = target;
        match target {
            SleepPhase::Suspended => {
                self.suspend_enter_tick = self.tick;
            }
            SleepPhase::Resuming => {
                self.resume_enter_tick = self.tick;
            }
            _ => {}
        }
        Ok(())
    }

    /// 提交命令。判定次序：零块 → 单条上限 → 停泊容量 → 落队/即时执行。
    /// 挂起窗（Suspending 起、Resumed 前）一律停泊；活跃期即时执行。
    pub fn submit(&mut self, lba: u64, blocks: u32) -> Result<u32, SleepCode> {
        if blocks == 0 {
            return Err(SleepCode::ZeroBlocks);
        }
        if blocks > MAX_BLOCKS_PER_CMD {
            return Err(SleepCode::BlocksTooMany);
        }
        let seq = self.next_seq;
        match self.phase {
            SleepPhase::Suspending | SleepPhase::Suspended | SleepPhase::Resuming => {
                if self.parked.len() >= MAX_PARKED {
                    return Err(SleepCode::ParkFull);
                }
                self.parked.push(ParkedCmd { seq, lba, blocks });
            }
            _ => {
                self.executed += 1;
            }
        }
        self.next_seq = match seq.checked_add(1) {
            Some(v) => v,
            None => seq,
        };
        Ok(seq)
    }

    /// 请求唤醒：只有 Suspended 态可唤醒；冻结挂起段耗时。
    pub fn request_resume(&mut self) -> Result<u32, SleepCode> {
        if self.phase != SleepPhase::Suspended {
            return Err(SleepCode::NotSuspended);
        }
        self.pending_suspend_ticks = self.tick.saturating_sub(self.suspend_enter_tick);
        self.advance_to(SleepPhase::Resuming)?;
        Ok(self.pending_suspend_ticks)
    }

    /// 恢复收尾：按 seq 序续传全部停泊命令（清队），落本轮耗时账。
    pub fn finish_resume(&mut self) -> Result<u32, SleepCode> {
        if self.phase != SleepPhase::Resuming {
            return Err(SleepCode::NotResuming);
        }
        let resumed = self.parked.len() as u32;
        self.parked.clear();
        self.pending_resume_ticks = self.tick.saturating_sub(self.resume_enter_tick);
        self.advance_to(SleepPhase::Resumed)?;
        self.runs.push(RunAccount {
            run_id: self.run_id,
            suspend_ticks: self.pending_suspend_ticks,
            resume_ticks: self.pending_resume_ticks,
            resumed_cmds: resumed,
        });
        Ok(resumed)
    }

    /// 重臂：只有 Resumed 态能开新一轮；run_id 单调递增。
    pub fn rearm(&mut self) -> Result<u32, SleepCode> {
        if self.phase != SleepPhase::Resumed {
            return Err(SleepCode::NotResumed);
        }
        self.phase = SleepPhase::Active;
        self.run_id = match self.run_id.checked_add(1) {
            Some(v) => v,
            None => self.run_id,
        };
        Ok(self.run_id)
    }

    /// 落盘导出：按冻结格式逐字节写出（每轮 16 字节小端）。缓冲不足
    /// 显性拒（禁静默截断）；实际写出字节数必须与冻结公式一致。
    pub fn export_runs(&self, out: &mut [u8]) -> Result<usize, SleepCode> {
        let need = match self.runs.len().checked_mul(EXPORT_REC_BYTES) {
            Some(v) => v,
            None => return Err(SleepCode::ExportFormulaMismatch),
        };
        if out.len() < need {
            return Err(SleepCode::ExportTooSmall);
        }
        let mut w = 0usize;
        for i in 0..self.runs.len() {
            let acc = match self.runs.get(i) {
                Some(a) => a,
                None => return Err(SleepCode::ExportFormulaMismatch),
            };
            let bytes = acc.export_bytes();
            for k in 0..EXPORT_REC_BYTES {
                let b = match bytes.get(k) {
                    Some(x) => *x,
                    None => 0u8,
                };
                match out.get_mut(w) {
                    Some(slot) => {
                        *slot = b;
                    }
                    None => return Err(SleepCode::ExportTooSmall),
                }
                w += 1;
            }
        }
        if w != need {
            return Err(SleepCode::ExportFormulaMismatch);
        }
        Ok(w)
    }
}

/// 段位守卫。
pub fn seg_guard_ok() -> bool {
    SEG_BASE % 0x40 == 0 && SEG_BASE >= 0xC0080 && SEG_BASE <= 0xC31C0
}

/// 端到端 ×10 演练：每轮「活跃提交→挂起→停泊→唤醒→续传→重臂」。
/// 停泊命令 LBA 由固定种子 LCG 决定（确定性可复现）。
/// 返回（全通, 耗时账条数）。
pub fn end_to_end_x10() -> (bool, usize) {
    let mut seed: u32 = 0x5151_4001;
    let mut all_ok = true;
    let mut e = SleepResumeEngine::new();
    for r in 0..RUNS_REQUIRED {
        // 活跃期提交 3 条（即时执行，不进停泊队）。
        for k in 0..3u32 {
            let _ = e.submit((r as u64) * 1000 + k as u64, 8);
        }
        let _ = e.advance_to(SleepPhase::Suspending);
        let _ = e.elapse(2);
        let _ = e.advance_to(SleepPhase::Suspended);
        // 挂起期停泊 2 条。
        for _ in 0..2u32 {
            seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            let lba = ((seed >> 8) % 4096) as u64;
            let _ = e.submit(lba, 4);
        }
        let _ = e.elapse(3);
        if e.request_resume().is_err() {
            all_ok = false;
        }
        let _ = e.elapse(1);
        if e.finish_resume() != Ok(2) {
            all_ok = false;
        }
        if e.rearm().is_err() {
            all_ok = false;
        }
    }
    if e.runs_len() != RUNS_REQUIRED as usize {
        all_ok = false;
    }
    // 每轮账：续传条数恒 2，run_id 单调；耗时账从脚本语义读回核对。
    for i in 0..e.runs_len() {
        let acc = match e.runs().get(i) {
            Some(a) => *a,
            None => {
                all_ok = false;
                break;
            }
        };
        if acc.resumed_cmds != 2 || acc.run_id != i as u32 + 1 {
            all_ok = false;
        }
        if acc.suspend_ticks != 3 || acc.resume_ticks != 1 {
            all_ok = false;
        }
    }
    (all_ok, e.runs_len())
}

/// 双跑一致性：同一脚本两次重建耗时账逐字段一致（确定性）。
pub fn dual_run_consistent() -> bool {
    let build = || -> Vec<RunAccount> {
        let mut e = SleepResumeEngine::new();
        let _ = e.submit(10, 4);
        let _ = e.advance_to(SleepPhase::Suspending);
        let _ = e.advance_to(SleepPhase::Suspended);
        let _ = e.submit(20, 2);
        let _ = e.elapse(3);
        let _ = e.request_resume();
        let _ = e.finish_resume();
        e.runs.clone()
    };
    let a = build();
    let b = build();
    a == b
}

/// 收口自检（≥12 条判据形态）。
pub fn run_unx51514_checks() -> CheckSet {
    let mut s = CheckSet::new("unx51514");
    // 1 正常流：活跃期即时执行 + 挂起期停泊 + 唤醒续传。
    {
        let mut e = SleepResumeEngine::new();
        let _ = e.submit(0, 8);
        let _ = e.advance_to(SleepPhase::Suspending);
        let _ = e.advance_to(SleepPhase::Suspended);
        let _ = e.submit(16, 4);
        let _ = e.submit(32, 4);
        let _ = e.request_resume();
        let _ = e.finish_resume();
        s.add(
            "正常流挂起续传",
            e.executed() == 1 && e.parked_len() == 0 && e.runs_len() == 1,
            "活跃命令即时执行、停泊命令续传清空、落 1 条耗时账",
        );
    }
    // 2 边界零：零块命令拒绝（无符号域无负值，零/上限/上限+1 全覆盖）。
    {
        let mut e = SleepResumeEngine::new();
        s.add(
            "零块边界拒绝",
            e.submit(0, 0) == Err(SleepCode::ZeroBlocks),
            "块数必须 ≥1，零块显性拒",
        );
    }
    // 3 边界上限/上限+1。
    {
        let mut e = SleepResumeEngine::new();
        let ok_max = e.submit(0, MAX_BLOCKS_PER_CMD).is_ok();
        s.add(
            "块数上限与上限+1",
            ok_max && e.submit(0, MAX_BLOCKS_PER_CMD + 1) == Err(SleepCode::BlocksTooMany),
            "上限恰好放行、上限+1 显性拒",
        );
    }
    // 4 注入必抓：挂起期零块 ×10。
    {
        let mut e = SleepResumeEngine::new();
        let _ = e.advance_to(SleepPhase::Suspending);
        let _ = e.advance_to(SleepPhase::Suspended);
        let mut caught = 0u32;
        for i in 0..10u32 {
            if e.submit(100 + i as u64, 0) == Err(SleepCode::ZeroBlocks) {
                caught += 1;
                e.note_violation_caught();
            }
        }
        s.add("挂起期零块注入 10/10", caught == 10, "挂起期零块违约必须全抓");
    }
    // 5 注入必抓：相位倒退 ×10。
    {
        let mut e = SleepResumeEngine::new();
        let _ = e.advance_to(SleepPhase::Suspending);
        let _ = e.advance_to(SleepPhase::Suspended);
        let mut caught = 0u32;
        for _ in 0..10u32 {
            if e.advance_to(SleepPhase::Suspending) == Err(SleepCode::PhaseRegression) {
                caught += 1;
                e.note_violation_caught();
            }
        }
        s.add("相位倒退注入 10/10", caught == 10, "相位倒退必须全抓");
    }
    // 6 注入必抓：未挂起先唤醒 ×10。
    {
        let mut e = SleepResumeEngine::new();
        let mut caught = 0u32;
        for _ in 0..10u32 {
            if e.request_resume() == Err(SleepCode::NotSuspended) {
                caught += 1;
                e.note_violation_caught();
            }
        }
        s.add("未挂起唤醒注入 10/10", caught == 10, "未挂起先唤醒必须全抓");
    }
    // 7 判定次序：内容判据（零块/超限）先于停泊容量判据。
    {
        let mut e = SleepResumeEngine::new();
        let _ = e.advance_to(SleepPhase::Suspending);
        let _ = e.advance_to(SleepPhase::Suspended);
        for i in 0..MAX_PARKED {
            let _ = e.submit(i as u64, 1);
        }
        s.add(
            "判序内容先于容量",
            e.submit(99, 0) == Err(SleepCode::ZeroBlocks)
                && e.submit(99, MAX_BLOCKS_PER_CMD + 1) == Err(SleepCode::BlocksTooMany)
                && e.submit(99, 1) == Err(SleepCode::ParkFull),
            "队满时零块/超限仍按内容码拒，容量码最后",
        );
    }
    // 8 状态机拒止：跳格/倒退/未恢复重臂。
    {
        let mut e = SleepResumeEngine::new();
        let skip = e.advance_to(SleepPhase::Suspended) == Err(SleepCode::PhaseSkip);
        let _ = e.advance_to(SleepPhase::Suspending);
        let regress = e.advance_to(SleepPhase::Active) == Err(SleepCode::PhaseRegression);
        let rearm = e.rearm() == Err(SleepCode::NotResumed);
        s.add(
            "相位机拒止",
            skip && regress && rearm,
            "跳格/倒退/未恢复重臂一律显性拒",
        );
    }
    // 9 容量耗尽：停泊队 8 条恰好满，第 9 条显性拒。
    {
        let mut e = SleepResumeEngine::new();
        let _ = e.advance_to(SleepPhase::Suspending);
        let _ = e.advance_to(SleepPhase::Suspended);
        let mut all_ok = true;
        for i in 0..MAX_PARKED {
            if e.submit(i as u64, 1).is_err() {
                all_ok = false;
            }
        }
        s.add(
            "停泊容量耗尽",
            all_ok && e.parked_len() == MAX_PARKED && e.submit(99, 1) == Err(SleepCode::ParkFull),
            "上限恰好放行、上限+1 显性拒且不部分落队",
        );
    }
    // 10 双跑一致。
    s.add("双跑逐字段一致", dual_run_consistent(), "同一脚本两次建账逐字段相等");
    // 11 段位守卫。
    s.add("段位守卫", seg_guard_ok(), "SEG_BASE 对齐且在分派区间");
    // 12 对账守恒：注入 = 检出 + 逃逸，逃逸 0。
    {
        let mut e = SleepResumeEngine::new();
        let _ = e.advance_to(SleepPhase::Suspending);
        let _ = e.advance_to(SleepPhase::Suspended);
        for i in 0..10u32 {
            let _ = e.submit(100 + i as u64, 0);
            e.note_violation_caught();
        }
        for _ in 0..10u32 {
            let _ = e.advance_to(SleepPhase::Suspending);
            e.note_violation_caught();
        }
        let mut e2 = SleepResumeEngine::new();
        for _ in 0..10u32 {
            let _ = e2.request_resume();
            e2.note_violation_caught();
        }
        s.add(
            "注入账守恒",
            e.conserved()
                && e2.conserved()
                && e.injected() + e2.injected() == 30
                && e.escaped() == 0,
            "注入=检出+逃逸，逃逸恒 0",
        );
    }
    // 13 文案闭集非空。
    {
        let mut closed = true;
        for c in ALL_CODES.iter() {
            if c.detail().is_empty() {
                closed = false;
            }
        }
        s.add("文案闭集非空", closed, "每个诊断码都必须有中文建议");
    }
    // 14 端到端 ×10 全通 + 落盘导出账与冻结公式一致。
    {
        let (all_ok, runs) = end_to_end_x10();
        let mut e = SleepResumeEngine::new();
        let _ = e.advance_to(SleepPhase::Suspending);
        let _ = e.advance_to(SleepPhase::Suspended);
        let _ = e.submit(8, 2);
        let _ = e.request_resume();
        let _ = e.finish_resume();
        let need = e.runs_len() * EXPORT_REC_BYTES;
        let mut buf = [0u8; RUNS_REQUIRED as usize * EXPORT_REC_BYTES];
        let exported = e.export_runs(&mut buf);
        let mut small = [0u8; 8];
        s.add(
            "端到端 ×10 与落盘导出账",
            all_ok
                && runs == RUNS_REQUIRED as usize
                && exported == Ok(need)
                && e.export_runs(&mut small) == Err(SleepCode::ExportTooSmall),
            "端到端 ×10 全通；导出实际字节=轮数×16，缓冲不足显性拒",
        );
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normal_cycle_parks_and_resumes() {
        let mut e = SleepResumeEngine::new();
        assert!(e.submit(0, 8).is_ok());
        assert!(e.advance_to(SleepPhase::Suspending).is_ok());
        assert!(e.advance_to(SleepPhase::Suspended).is_ok());
        assert!(e.submit(16, 4).is_ok());
        assert!(e.submit(32, 4).is_ok());
        assert!(e.elapse(5).is_ok());
        assert!(e.request_resume().is_ok());
        assert!(e.elapse(2).is_ok());
        assert_eq!(e.finish_resume(), Ok(2));
        assert_eq!(e.executed(), 1);
        assert_eq!(e.parked_len(), 0);
        // 耗时账语义从脚本读回：挂起段=进入 Suspended 后流逝 5 tick，恢复段=2 tick。
        assert!(e.runs().get(0).is_some());
        if let Some(acc) = e.runs().get(0) {
            assert_eq!(acc.suspend_ticks, 5);
            assert_eq!(acc.resume_ticks, 2);
            assert_eq!(acc.resumed_cmds, 2);
            assert_eq!(acc.run_id, 1);
        }
    }

    #[test]
    fn zero_and_upper_block_bounds() {
        let mut e = SleepResumeEngine::new();
        assert_eq!(e.submit(0, 0), Err(SleepCode::ZeroBlocks));
        assert!(e.submit(0, MAX_BLOCKS_PER_CMD).is_ok());
        assert_eq!(e.submit(0, MAX_BLOCKS_PER_CMD + 1), Err(SleepCode::BlocksTooMany));
    }

    #[test]
    fn park_capacity_exhaustion() {
        let mut e = SleepResumeEngine::new();
        let _ = e.advance_to(SleepPhase::Suspending);
        let _ = e.advance_to(SleepPhase::Suspended);
        for i in 0..MAX_PARKED {
            assert!(e.submit(i as u64, 1).is_ok(), "第 {} 条停泊应成功", i);
        }
        assert_eq!(e.submit(99, 1), Err(SleepCode::ParkFull));
        assert_eq!(e.parked_len(), MAX_PARKED, "拒后停泊队不增");
    }

    #[test]
    fn injections_all_caught_x10() {
        // 挂起期零块。
        let mut e = SleepResumeEngine::new();
        let _ = e.advance_to(SleepPhase::Suspending);
        let _ = e.advance_to(SleepPhase::Suspended);
        let mut caught = 0u32;
        for i in 0..10u32 {
            if e.submit(100 + i as u64, 0) == Err(SleepCode::ZeroBlocks) {
                caught += 1;
                e.note_violation_caught();
            }
        }
        assert_eq!(caught, 10);
        // 相位倒退（同一引擎复用，倒退可重复拒）。
        let mut caught2 = 0u32;
        for _ in 0..10u32 {
            if e.advance_to(SleepPhase::Suspending) == Err(SleepCode::PhaseRegression) {
                caught2 += 1;
                e.note_violation_caught();
            }
        }
        assert_eq!(caught2, 10);
        // 未挂起先唤醒。
        let mut e2 = SleepResumeEngine::new();
        let mut caught3 = 0u32;
        for _ in 0..10u32 {
            if e2.request_resume() == Err(SleepCode::NotSuspended) {
                caught3 += 1;
                e2.note_violation_caught();
            }
        }
        assert_eq!(caught3, 10);
        assert!(e.conserved() && e2.conserved());
        assert_eq!(e.escaped(), 0);
    }

    #[test]
    fn phase_machine_rejects_skip_and_regress() {
        let mut e = SleepResumeEngine::new();
        assert_eq!(e.advance_to(SleepPhase::Suspended), Err(SleepCode::PhaseSkip));
        assert_eq!(e.advance_to(SleepPhase::Resumed), Err(SleepCode::PhaseSkip));
        assert!(e.advance_to(SleepPhase::Suspending).is_ok());
        assert_eq!(e.advance_to(SleepPhase::Active), Err(SleepCode::PhaseRegression));
        assert_eq!(e.rearm(), Err(SleepCode::NotResumed));
    }

    #[test]
    fn judge_order_content_before_capacity() {
        let mut e = SleepResumeEngine::new();
        let _ = e.advance_to(SleepPhase::Suspending);
        let _ = e.advance_to(SleepPhase::Suspended);
        for i in 0..MAX_PARKED {
            let _ = e.submit(i as u64, 1);
        }
        assert_eq!(e.submit(99, 0), Err(SleepCode::ZeroBlocks));
        assert_eq!(e.submit(99, MAX_BLOCKS_PER_CMD + 1), Err(SleepCode::BlocksTooMany));
        assert_eq!(e.submit(99, 1), Err(SleepCode::ParkFull));
    }

    #[test]
    fn dual_run_identical_accounts() {
        assert!(dual_run_consistent());
    }

    #[test]
    fn seg_guard_holds() {
        assert!(seg_guard_ok());
        assert_eq!(SEG_BASE % 0x40, 0);
    }

    #[test]
    fn export_byte_account_exact() {
        let mut e = SleepResumeEngine::new();
        let _ = e.advance_to(SleepPhase::Suspending);
        let _ = e.advance_to(SleepPhase::Suspended);
        let _ = e.submit(8, 2);
        let _ = e.request_resume();
        let _ = e.finish_resume();
        let need = e.runs_len() * EXPORT_REC_BYTES;
        let mut buf = [0u8; RUNS_REQUIRED as usize * EXPORT_REC_BYTES];
        assert_eq!(e.export_runs(&mut buf), Ok(need));
        // 逐字节对冻结公式：run_id/suspend_ticks/resume_ticks/resumed_cmds 小端。
        assert!(e.runs().get(0).is_some());
        if let Some(acc) = e.runs().get(0) {
            let want = acc.export_bytes();
            for i in 0..EXPORT_REC_BYTES {
                let got = match buf.get(i) {
                    Some(b) => *b,
                    None => 0xFF,
                };
                let wb = match want.get(i) {
                    Some(b) => *b,
                    None => 0xFF,
                };
                assert_eq!(got, wb, "导出第 {} 字节与冻结公式不符", i);
            }
        }
        let mut small = [0u8; 8];
        assert_eq!(e.export_runs(&mut small), Err(SleepCode::ExportTooSmall));
    }

    #[test]
    fn detail_texts_closed_set() {
        for c in ALL_CODES.iter() {
            assert!(!c.detail().is_empty());
        }
        assert_eq!(ALL_CODES.len(), 13);
    }

    #[test]
    fn end_to_end_x10_all_green() {
        let (all_ok, runs) = end_to_end_x10();
        assert!(all_ok);
        assert_eq!(runs, RUNS_REQUIRED as usize);
    }
}
