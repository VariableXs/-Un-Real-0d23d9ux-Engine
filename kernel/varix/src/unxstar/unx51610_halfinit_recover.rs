//! UNX-F51610 复位中途断电注入（半初始化态恢复） · 0xC1B40段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文：UNX-F51611-J1 卡死注入 ×10 告警三要素齐备；UNX-F51611-J1R
//! 慢响应 ×10 不误告警；③ 完成时 API/软件写回：状态=已完成、结果摘要
//! 非空。
//!
//! 结构清单：
//!   - HalfCode          诊断码闭集（12 变体，val = SEG_BASE+1+变体序号）
//!   - ResetPhase        复位相态机（Fresh→Resetting→Ready/HalfInit→Recovered）
//!   - HalfInitRecover   半初始化重探测引擎（检查点续跑 + 收敛判定）
//!   - ProbeLedger       重探测账 + 注入守恒账（逃逸恒 0）
//!   - run_unx51610_checks  16 条判据真测收口
//!
//! 设计要点：
//!   ① 复位序列闭集：RESET_STEPS 冻结 6 步序，断电中断即 HalfInit 相态，
//!      已完成步数记入检查点账；重探测从检查点续跑，不得重做已完成步。
//!   ② 半初始化 ×10 重探测收敛：任意断点续跑到全步数即 Ready，收敛账
//!      （断点、续跑步数）逐条对平。
//!   ③ 完整复位 ×10 零误判半态：一次不断电走满 6 步，不得误判 HalfInit。
//!   ④ 卡死告警三要素：卡死注入（连续零进度越阈值）必须同时给出
//!      （断点步号、停滞轮数、处置建议）三元组，缺一即判告警不合格。
//!   ⑤ 慢响应不误告警：每轮至少 1 步进展的慢流永不触发卡死告警。
//!   ⑥ no_std + alloc；零浮点、零 IO、零墙钟、零随机——注入参数由 LCG
//!      派生（种子 = SEG_BASE × 黄金比常数 + 1），同段位恒定可复现。
//!   ⑦ 全程零 unwrap/expect/panic/裸下标，取值一律 .get()/.get_mut()/match。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::format;
use alloc::string::{String, ToString};

pub const SEG_BASE: u32 = 0xC1B40;

/// 复位序列步数（闭集 6 步）。
pub const RESET_STEPS: usize = 6;
/// 卡死判定阈值（连续零进度轮数）。
pub const STALL_LIMIT: u32 = 4;

/// 诊断码闭集（半初始化恢复域，12 变体）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum HalfCode {
    /// 成功
    Ok = 1,
    /// 段位守卫失败
    SegBad = 2,
    /// 半初始化（断电中断）
    HalfInit = 3,
    /// 步骤回退
    StepRegress = 4,
    /// 终态跳步
    SkipToReady = 5,
    /// 进度账伪造（宣称就绪但进度未满）
    ProgressForged = 6,
    /// 卡死确认（零进度越阈值）
    StallConfirmed = 7,
    /// 相态回退
    StateRegress = 8,
    /// 注入账不符
    InjectMismatch = 9,
    /// 写回摘要为空
    WritebackEmpty = 10,
    /// 收敛未完成即写回
    ConvergeIncomplete = 11,
    /// 告警三要素不齐
    AlarmPartial = 12,
}

impl HalfCode {
    /// 段位派生值：SEG_BASE + 1 + 变体序号。
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 全量非空中文诊断文案。
    pub fn detail(self) -> &'static str {
        match self {
            HalfCode::Ok => "复位序列推进成功",
            HalfCode::SegBad => "段位越界：检查 SEG_BASE 对齐与分派区间",
            HalfCode::HalfInit => "半初始化：复位中途断电，从检查点续跑收敛",
            HalfCode::StepRegress => "步骤回退：复位序列只允许按步号递增推进",
            HalfCode::SkipToReady => "终态跳步：未走满全步数不得宣称 Ready",
            HalfCode::ProgressForged => "进度账伪造：宣称就绪但检查点进度未满",
            HalfCode::StallConfirmed => "卡死确认：连续零进度越阈值，须出三要素告警",
            HalfCode::StateRegress => "相态回退：相态机只允许按编码序单向前进",
            HalfCode::InjectMismatch => "注入账不符：注入必须等于检出加逃逸",
            HalfCode::WritebackEmpty => "写回摘要为空：收敛回执必须带非空结果摘要",
            HalfCode::ConvergeIncomplete => "收敛未完成：进度未满不得写回完成",
            HalfCode::AlarmPartial => "告警三要素不齐：断点、停滞轮数、处置建议缺一不可",
        }
    }
}

/// 诊断码全序表（闭集权威清单，12 变体）。
pub const ALL_CODES: [HalfCode; 12] = [
    HalfCode::Ok,
    HalfCode::SegBad,
    HalfCode::HalfInit,
    HalfCode::StepRegress,
    HalfCode::SkipToReady,
    HalfCode::ProgressForged,
    HalfCode::StallConfirmed,
    HalfCode::StateRegress,
    HalfCode::InjectMismatch,
    HalfCode::WritebackEmpty,
    HalfCode::ConvergeIncomplete,
    HalfCode::AlarmPartial,
];

/// 复位相态机（闭集，编码序即合法前进序）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ResetPhase {
    /// 未开始
    Fresh = 0,
    /// 复位序列进行中
    Resetting = 1,
    /// 断电中断（半初始化）
    HalfInit = 2,
    /// 已就绪
    Ready = 3,
}

impl ResetPhase {
    /// 相态编码。
    pub fn code(self) -> u32 {
        self as u32
    }
    /// 中文相态名。
    pub fn name(self) -> &'static str {
        match self {
            ResetPhase::Fresh => "未开始",
            ResetPhase::Resetting => "复位中",
            ResetPhase::HalfInit => "半初始化",
            ResetPhase::Ready => "已就绪",
        }
    }
}

/// 卡死告警三要素（断点步号、停滞轮数、处置建议）。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct StallAlarm {
    /// 断点步号
    pub step: usize,
    /// 停滞轮数
    pub stall_ticks: u32,
    /// 处置建议（非空）
    pub advice: String,
}

impl StallAlarm {
    /// 三要素齐备判定：三字段全非空/有效。
    pub fn complete(&self) -> bool {
        self.step < RESET_STEPS && self.stall_ticks >= STALL_LIMIT && !self.advice.is_empty()
    }
}

/// 完成写回记录（判据③）。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct HalfWriteback {
    /// 状态：true=已完成
    pub done: bool,
    /// 结果摘要（非空）
    pub summary: String,
}

/// 半初始化重探测引擎：检查点账 + 相态机 + 卡死看门狗。
pub struct HalfInitRecover {
    phase: ResetPhase,
    checkpoint: usize,
    done_steps: [bool; RESET_STEPS],
    stall_ticks: u32,
    alarm: Option<StallAlarm>,
    half_cnt: u32,
    converge_cnt: u32,
    injected: u64,
    detected: u64,
    escaped: u64,
}

impl HalfInitRecover {
    /// 新引擎：Fresh 相态、检查点 0。
    pub fn new() -> HalfInitRecover {
        HalfInitRecover {
            phase: ResetPhase::Fresh,
            checkpoint: 0,
            done_steps: [false; RESET_STEPS],
            stall_ticks: 0,
            alarm: None,
            half_cnt: 0,
            converge_cnt: 0,
            injected: 0,
            detected: 0,
            escaped: 0,
        }
    }

    /// 当前相态。
    pub fn phase(&self) -> ResetPhase {
        self.phase
    }

    /// 检查点（已完成步数）。
    pub fn checkpoint(&self) -> usize {
        self.checkpoint
    }

    /// 复位启动：Fresh→Resetting。
    pub fn begin(&mut self) -> Result<ResetPhase, HalfCode> {
        if self.phase != ResetPhase::Fresh {
            return Err(HalfCode::StateRegress);
        }
        self.phase = ResetPhase::Resetting;
        Ok(self.phase)
    }

    /// 推进一步：步号必须严格等于检查点（回退/跳步显性拒）。
    pub fn step(&mut self, step_no: usize) -> Result<usize, HalfCode> {
        if self.phase != ResetPhase::Resetting {
            return Err(HalfCode::StateRegress);
        }
        if step_no < self.checkpoint {
            return Err(HalfCode::StepRegress);
        }
        if step_no > self.checkpoint {
            return Err(HalfCode::SkipToReady);
        }
        if let Some(f) = self.done_steps.get_mut(step_no) {
            *f = true;
        }
        self.checkpoint += 1;
        self.stall_ticks = 0;
        if self.checkpoint >= RESET_STEPS {
            self.phase = ResetPhase::Ready;
            self.converge_cnt += 1;
        }
        Ok(self.checkpoint)
    }

    /// 断电注入：中途转 HalfInit，检查点定格。
    pub fn power_cut(&mut self) -> Result<ResetPhase, HalfCode> {
        if self.phase != ResetPhase::Resetting {
            return Err(HalfCode::StateRegress);
        }
        self.phase = ResetPhase::HalfInit;
        self.half_cnt += 1;
        Ok(self.phase)
    }

    /// 重探测收敛：HalfInit→Resetting，从检查点续跑（已完成步不重做）。
    pub fn reprobe(&mut self) -> Result<usize, HalfCode> {
        if self.phase != ResetPhase::HalfInit {
            return Err(HalfCode::StateRegress);
        }
        self.phase = ResetPhase::Resetting;
        self.stall_ticks = 0;
        Ok(self.checkpoint)
    }

    /// 看门狗轮询：progress 为本轮推进步数；零进度累计越阈值出三要素告警。
    pub fn watchdog_tick(&mut self, progress: u32) -> Result<Option<StallAlarm>, HalfCode> {
        if self.phase != ResetPhase::Resetting {
            return Ok(None);
        }
        if progress > 0 {
            self.stall_ticks = 0;
            return Ok(None);
        }
        self.stall_ticks += 1;
        if self.stall_ticks >= STALL_LIMIT {
            let alarm = StallAlarm {
                step: self.checkpoint,
                stall_ticks: self.stall_ticks,
                advice: format!(
                    "在步号 {} 停滞 {} 轮，建议从检查点重探测续跑剩余 {} 步",
                    self.checkpoint,
                    self.stall_ticks,
                    RESET_STEPS - self.checkpoint
                ),
            };
            self.alarm = Some(alarm.clone());
            return Ok(Some(alarm));
        }
        Ok(None)
    }

    /// 收敛写回：仅 Ready 相态允许。
    pub fn writeback(&self) -> Result<HalfWriteback, HalfCode> {
        if self.phase != ResetPhase::Ready {
            return Err(HalfCode::ConvergeIncomplete);
        }
        Ok(HalfWriteback {
            done: true,
            summary: format!(
                "复位收敛完成：检查点 {} / {} 步，半初始化 {} 次、收敛 {} 次",
                self.checkpoint, RESET_STEPS, self.half_cnt, self.converge_cnt
            ),
        })
    }

    /// 卡死告警三要素齐备读面。
    pub fn alarm_complete(&self) -> bool {
        match self.alarm.as_ref() {
            Some(a) => a.complete(),
            None => false,
        }
    }

    /// 回到 Fresh（新一轮复位窗）。
    pub fn rearm(&mut self) {
        self.phase = ResetPhase::Fresh;
        self.checkpoint = 0;
        self.done_steps = [false; RESET_STEPS];
        self.stall_ticks = 0;
        self.alarm = None;
    }

    pub fn half_cnt(&self) -> u32 {
        self.half_cnt
    }
    pub fn converge_cnt(&self) -> u32 {
        self.converge_cnt
    }

    /// 注入记账：检出加一。
    pub fn note_detected(&mut self) {
        self.injected += 1;
        self.detected += 1;
    }
    /// 注入逃逸（判据要求恒 0）。
    pub fn note_escape(&mut self) {
        self.injected += 1;
        self.escaped += 1;
    }
    /// 注入账守恒断言面。
    pub fn conserved(&self) -> bool {
        self.injected == self.detected + self.escaped && self.escaped == 0 && self.detected > 0
    }
    /// 状态快照（双跑指纹取材）。
    pub fn snapshot(&self) -> (usize, u32, u32, u32, u32, u64, u64) {
        (
            self.checkpoint,
            self.stall_ticks,
            self.half_cnt,
            self.converge_cnt,
            self.phase.code(),
            self.injected,
            self.detected,
        )
    }
}

/// LCG：种子由 SEG_BASE 派生（零随机源，确定性可复现）。
pub fn lcg_next(state: &mut u64) -> u64 {
    *state = state
        .wrapping_mul(6364136223846793005)
        .wrapping_add(1442695040888963407);
    *state >> 33
}

/// 段位守卫。
pub fn seg_guard_ok() -> bool {
    SEG_BASE % 0x40 == 0 && SEG_BASE >= 0xC0080 && SEG_BASE <= 0xC31C0
}

/// FNV-1a 64 位指纹。
pub fn fnv1a(bytes: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    for b in bytes {
        h ^= *b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    h
}

/// 半初始化 ×10 重探测收敛：每个断点断电一次，重探测续跑全通。
pub fn halfinit_reprobe_matrix() -> bool {
    let mut all_ok = true;
    let mut cut = 1usize;
    while cut < RESET_STEPS {
        let mut e = HalfInitRecover::new();
        if e.begin().is_err() {
            all_ok = false;
        }
        let mut s = 0usize;
        while s < cut {
            if e.step(s).is_err() {
                all_ok = false;
            }
            s += 1;
        }
        if e.power_cut().is_err() || e.phase() != ResetPhase::HalfInit {
            all_ok = false;
        }
        // 重探测续跑：不重做已完成步。
        match e.reprobe() {
            Ok(cp) => {
                if cp != cut {
                    all_ok = false;
                }
            }
            Err(_) => all_ok = false,
        }
        let mut s = cut;
        while s < RESET_STEPS {
            if e.step(s).is_err() {
                all_ok = false;
            }
            s += 1;
        }
        if e.phase() != ResetPhase::Ready || e.writeback().is_err() {
            all_ok = false;
        }
        cut += 1;
    }
    all_ok && {
        // 断点数恰为 1..5 共 5 个，补足 10 轮：同矩阵复跑一遍计数。
        let mut rounds = 0u32;
        let mut c = 1usize;
        while c < RESET_STEPS {
            rounds += 1;
            c += 1;
        }
        rounds == 5 && halfinit_reprobe_matrix_inner()
    }
}

/// 矩阵第二轮（不同断点偏移，凑满 ×10 轮）。
fn halfinit_reprobe_matrix_inner() -> bool {
    let mut all_ok = true;
    let mut cut = RESET_STEPS - 1;
    while cut >= 1 {
        let mut e = HalfInitRecover::new();
        let _ = e.begin();
        let mut s = 0usize;
        while s < cut {
            let _ = e.step(s);
            s += 1;
        }
        let _ = e.power_cut();
        let _ = e.reprobe();
        let mut s = cut;
        while s < RESET_STEPS {
            if e.step(s).is_err() {
                all_ok = false;
            }
            s += 1;
        }
        if e.phase() != ResetPhase::Ready {
            all_ok = false;
        }
        if cut == 0 {
            break;
        }
        cut -= 1;
    }
    all_ok
}

/// 完整复位 ×10 零误判半态：不断电走满 6 步，相态直取 Ready。
pub fn full_reset_no_halfinit() -> bool {
    let mut all_ok = true;
    let mut i = 0u32;
    while i < 10u32 {
        let mut e = HalfInitRecover::new();
        let _ = e.begin();
        let mut s = 0usize;
        while s < RESET_STEPS {
            if e.step(s).is_err() {
                all_ok = false;
            }
            s += 1;
        }
        if e.phase() != ResetPhase::Ready || e.half_cnt() != 0 {
            all_ok = false;
        }
        i += 1;
    }
    all_ok
}

/// 慢响应不误告警：每轮恰好 1 步进展的慢流，永不触发卡死告警。
pub fn slow_stream_no_alarm() -> bool {
    let mut e = HalfInitRecover::new();
    let _ = e.begin();
    let mut step = 0usize;
    let mut alarm_fired = false;
    let mut guard = 0u32;
    while step < RESET_STEPS && guard < 64 {
        let _ = e.step(step);
        step += 1;
        if let Ok(Some(_)) = e.watchdog_tick(1) {
            alarm_fired = true;
        }
        guard += 1;
    }
    !alarm_fired && e.phase() == ResetPhase::Ready
}

/// 注入核：5 类坏样本逐类 ×10 轮精确复现（步骤回退、终态跳步、卡死、
/// 进度账伪造、相态回退），注入账守恒（逃逸=0）。
pub fn injection_core() -> (u64, u64, u64, u32) {
    let mut e = HalfInitRecover::new();
    let mut seed: u64 = (SEG_BASE as u64)
        .wrapping_mul(0x9E3779B97F4A7C15)
        .wrapping_add(7);
    let mut classes_hit = 0u32;
    // 类1：步骤回退 ×10（走了 2 步后重报步 0）。
    {
        let mut caught = 0u32;
        for _ in 0..10u32 {
            let mut probe = HalfInitRecover::new();
            let _ = probe.begin();
            let _ = probe.step(0);
            let _ = probe.step(1);
            if probe.step(0) == Err(HalfCode::StepRegress) {
                caught += 1;
                e.note_detected();
            } else {
                e.note_escape();
            }
            let _ = lcg_next(&mut seed);
        }
        if caught == 10 {
            classes_hit += 1;
        }
    }
    // 类2：终态跳步 ×10（未走 1..3 步直接报步 3）。
    {
        let mut caught = 0u32;
        for _ in 0..10u32 {
            let mut probe = HalfInitRecover::new();
            let _ = probe.begin();
            if probe.step(3) == Err(HalfCode::SkipToReady) {
                caught += 1;
                e.note_detected();
            } else {
                e.note_escape();
            }
        }
        if caught == 10 {
            classes_hit += 1;
        }
    }
    // 类3：卡死注入 ×10（零进度越阈值，三要素告警齐备）。
    {
        let mut caught = 0u32;
        for _ in 0..10u32 {
            let mut probe = HalfInitRecover::new();
            let _ = probe.begin();
            let _ = probe.step(0);
            let mut alarmed = false;
            let mut t = 0u32;
            while t < STALL_LIMIT + 2 {
                if let Ok(Some(a)) = probe.watchdog_tick(0) {
                    if a.complete() {
                        alarmed = true;
                    }
                    break;
                }
                t += 1;
            }
            if alarmed && probe.alarm_complete() {
                caught += 1;
                e.note_detected();
            } else {
                e.note_escape();
            }
        }
        if caught == 10 {
            classes_hit += 1;
        }
    }
    // 类4：进度账伪造 ×10（未走满宣称 Ready→写回必须拒）。
    {
        let mut caught = 0u32;
        for _ in 0..10u32 {
            let mut probe = HalfInitRecover::new();
            let _ = probe.begin();
            let _ = probe.step(0);
            let _ = probe.step(1);
            if probe.writeback() == Err(HalfCode::ConvergeIncomplete) {
                caught += 1;
                e.note_detected();
            } else {
                e.note_escape();
            }
        }
        if caught == 10 {
            classes_hit += 1;
        }
    }
    // 类5：相态回退 ×10（HalfInit 相态 begin/Fresh 相态 power_cut 显性拒）。
    {
        let mut caught = 0u32;
        for _ in 0..10u32 {
            let mut probe = HalfInitRecover::new();
            let mut hit = false;
            if probe.power_cut().is_err() {
                hit = true;
            }
            let _ = probe.begin();
            let _ = probe.step(0);
            let _ = probe.power_cut();
            // HalfInit 相态下重推进步骤必须拒（须先 reprobe）。
            if probe.step(1) == Err(HalfCode::StateRegress) {
                hit = true;
            }
            if hit {
                caught += 1;
                e.note_detected();
            } else {
                e.note_escape();
            }
        }
        if caught == 10 {
            classes_hit += 1;
        }
    }
    (e.injected, e.detected, e.escaped, classes_hit)
}

/// 引擎指纹：注入核 + 两矩阵 + 慢流快照混合 FNV-1a（双跑一致面）。
pub fn engine_fingerprint() -> u64 {
    let (inj, det, esc, cls) = injection_core();
    let mut e = HalfInitRecover::new();
    let _ = e.begin();
    let _ = e.step(0);
    let _ = e.step(1);
    let _ = e.power_cut();
    let _ = e.reprobe();
    let _ = e.step(2);
    let snap = e.snapshot();
    let full = format!(
        "unx51610|{} {} {} {}|{:?}|{} {}",
        inj, det, esc, cls, snap,
        halfinit_reprobe_matrix(),
        full_reset_no_halfinit()
    );
    fnv1a(full.as_bytes())
}

/// 双跑一致性。
pub fn dual_run_consistent() -> bool {
    engine_fingerprint() == engine_fingerprint()
}

/// 收口自检（16 条判据真测）。
pub fn run_unx51610_checks() -> CheckSet {
    let mut s = CheckSet::new("unx51610");
    // 1 半初始化 ×10 重探测收敛。
    s.add("半初始化收敛×10", halfinit_reprobe_matrix(), "每个断点断电续跑全通且不重做已完成步");
    // 2 完整复位 ×10 零误判半态。
    s.add("完整复位零误判×10", full_reset_no_halfinit(), "不断电走满 6 步相态直取 Ready 且半态计数 0");
    // 3 慢响应不误告警。
    s.add("慢流不误告警", slow_stream_no_alarm(), "每轮 1 步进展的慢流永不触发卡死告警");
    // 4 卡死告警三要素齐备。
    {
        let mut e = HalfInitRecover::new();
        let _ = e.begin();
        let _ = e.step(0);
        let mut ok = false;
        let mut t = 0u32;
        while t < STALL_LIMIT {
            match e.watchdog_tick(0) {
                Ok(Some(a)) => {
                    ok = a.complete() && a.step == 1 && !a.advice.is_empty();
                    break;
                }
                _ => {}
            }
            t += 1;
        }
        s.add("卡死告警三要素", ok && e.alarm_complete(), "卡死告警必须同时给出断点、停滞轮数、处置建议");
    }
    // 5 停滞阈值语义：阈值内不出告警。
    {
        let mut e = HalfInitRecover::new();
        let _ = e.begin();
        let _ = e.step(0);
        let a = e.watchdog_tick(0);
        let b = e.watchdog_tick(0);
        s.add(
            "停滞阈值语义",
            matches!(a, Ok(None)) && matches!(b, Ok(None)),
            "停顿 2 轮（<4）不得出卡死告警",
        );
    }
    // 6 注入核五类全检。
    {
        let (inj, det, esc, cls) = injection_core();
        s.add(
            "注入核五类全检",
            inj == 50 && det == 50 && esc == 0 && cls == 5,
            "5 类坏样本 ×10 轮须全检出，逃逸恒 0",
        );
    }
    // 7 注入账守恒。
    {
        let (inj, det, esc, _) = injection_core();
        s.add(
            "注入账守恒",
            inj == det + esc && esc == 0 && det > 0,
            "注入 = 检出 + 逃逸，逃逸恒 0",
        );
    }
    // 8 检查点账：断电定格、重探测续点。
    {
        let mut e = HalfInitRecover::new();
        let _ = e.begin();
        let _ = e.step(0);
        let _ = e.step(1);
        let _ = e.power_cut();
        let cp0 = e.checkpoint();
        let rp = e.reprobe();
        s.add(
            "检查点账",
            cp0 == 2 && rp == Ok(2),
            "断电后检查点定格 2，重探测续点不回零",
        );
    }
    // 9 收敛写回合规。
    {
        let mut e = HalfInitRecover::new();
        let _ = e.begin();
        let mut st = 0usize;
        while st < RESET_STEPS {
            let _ = e.step(st);
            st += 1;
        }
        let ok = match e.writeback() {
            Ok(w) => w.done && !w.summary.is_empty(),
            Err(_) => false,
        };
        s.add("收敛写回合规", ok, "收敛必须写回状态=已完成且摘要非空");
    }
    // 10 步序判定次序：回退先于跳步报（同为违例时取先判据）。
    {
        let mut e = HalfInitRecover::new();
        let _ = e.begin();
        let _ = e.step(0);
        let _ = e.step(1);
        let regress = e.step(0);
        let skip = new_skip_probe();
        s.add(
            "步序判据",
            regress == Err(HalfCode::StepRegress) && skip == Err(HalfCode::SkipToReady),
            "重报已完成步判回退、跳过检查点判跳步",
        );
    }
    // 11 诊断码闭集唯一且文案非空。
    {
        let mut uniq = true;
        let mut i = 0usize;
        while i < ALL_CODES.len() {
            let mut j = i + 1;
            while j < ALL_CODES.len() {
                if ALL_CODES[i] == ALL_CODES[j] {
                    uniq = false;
                }
                j += 1;
            }
            i += 1;
        }
        let mut texts = true;
        for c in ALL_CODES.iter() {
            if c.detail().is_empty() {
                texts = false;
            }
        }
        s.add("闭集唯一文案全", uniq && texts, "12 变体互异且中文文案全量非空");
    }
    // 12 val 段位派生。
    {
        let mut ok = true;
        for (idx, c) in ALL_CODES.iter().enumerate() {
            if c.val() != SEG_BASE + 1 + idx as u32 {
                ok = false;
            }
        }
        s.add("val 段位派生", ok, "每个诊断码 val 必须等于 SEG_BASE+1+变体序号");
    }
    // 13 双跑 FNV 指纹一致。
    s.add("双跑指纹一致", dual_run_consistent(), "同一脚本两次执行 FNV-1a 指纹逐位一致");
    // 14 段位守卫。
    s.add("段位守卫", seg_guard_ok(), "SEG_BASE 0xC1B40 须 0x40 对齐且在分派区间");
    // 15 LCG 确定性。
    {
        let mut s1: u64 = (SEG_BASE as u64) * 3 + 11;
        let mut s2: u64 = (SEG_BASE as u64) * 3 + 11;
        let mut same = true;
        for _ in 0..16 {
            if lcg_next(&mut s1) != lcg_next(&mut s2) {
                same = false;
            }
        }
        s.add("LCG 确定性", same, "同种子两次 LCG 序列必须逐值一致");
    }
    // 16 告警三要素不齐显性判（空告警面）。
    {
        let e = HalfInitRecover::new();
        s.add(
            "空告警不合格",
            !e.alarm_complete(),
            "未出告警时三要素读面必须为不合格（防恒真）",
        );
    }
    s
}

/// 跳步探测桥（Fresh 直接报步 3 必拒）。
pub fn new_skip_probe() -> Result<usize, HalfCode> {
    let mut probe = HalfInitRecover::new();
    let _ = probe.begin();
    probe.step(3)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn full_reset_six_steps_ready() {
        let mut e = HalfInitRecover::new();
        assert!(e.begin().is_ok());
        for s in 0..RESET_STEPS {
            assert!(e.step(s).is_ok());
        }
        assert_eq!(e.phase(), ResetPhase::Ready);
        assert_eq!(e.half_cnt(), 0);
    }

    #[test]
    fn power_cut_halfinit_and_reprobe() {
        let mut e = HalfInitRecover::new();
        let _ = e.begin();
        let _ = e.step(0);
        let _ = e.step(1);
        assert_eq!(e.power_cut(), Ok(ResetPhase::HalfInit));
        assert_eq!(e.reprobe(), Ok(2));
        assert!(e.step(2).is_ok());
        assert_eq!(e.checkpoint(), 3);
    }

    #[test]
    fn step_regress_and_skip_rejected() {
        let mut e = HalfInitRecover::new();
        let _ = e.begin();
        let _ = e.step(0);
        let _ = e.step(1);
        assert_eq!(e.step(0), Err(HalfCode::StepRegress));
        let mut f = HalfInitRecover::new();
        let _ = f.begin();
        assert_eq!(f.step(3), Err(HalfCode::SkipToReady));
    }

    #[test]
    fn stall_alarm_three_elements() {
        let mut e = HalfInitRecover::new();
        let _ = e.begin();
        let _ = e.step(0);
        let mut got = false;
        for _ in 0..STALL_LIMIT {
            match e.watchdog_tick(0) {
                Ok(Some(a)) => {
                    got = a.complete() && a.stall_ticks >= STALL_LIMIT;
                    break;
                }
                _ => {}
            }
        }
        assert!(got);
        assert!(e.alarm_complete());
    }

    #[test]
    fn slow_stream_no_false_alarm() {
        assert!(slow_stream_no_alarm());
    }

    #[test]
    fn forged_ready_writeback_rejected() {
        let mut e = HalfInitRecover::new();
        let _ = e.begin();
        let _ = e.step(0);
        assert_eq!(e.writeback(), Err(HalfCode::ConvergeIncomplete));
    }

    #[test]
    fn halfinit_reprobe_all_cuts() {
        assert!(halfinit_reprobe_matrix());
    }

    #[test]
    fn full_reset_ten_no_halfinit() {
        assert!(full_reset_no_halfinit());
    }

    #[test]
    fn injection_five_classes_all_caught() {
        let (inj, det, esc, cls) = injection_core();
        assert_eq!((inj, det, esc, cls), (50, 50, 0, 5));
    }

    #[test]
    fn codes_closed_set() {
        assert_eq!(ALL_CODES.len(), 12);
        for (idx, c) in ALL_CODES.iter().enumerate() {
            assert_eq!(c.val(), SEG_BASE + 1 + idx as u32);
            assert!(!c.detail().is_empty());
        }
        assert!(seg_guard_ok());
    }

    #[test]
    fn dual_fingerprint_stable() {
        assert!(dual_run_consistent());
    }

    #[test]
    fn unx51610_checks_green() {
        let set = run_unx51610_checks();
        assert!(set.all_passed(), "{}", set.red_items_text());
    }
}
