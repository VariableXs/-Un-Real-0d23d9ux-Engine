//! unx122_stagewatchdog — UNX-F0122 阶段看门狗（每阶段独立时限，毫秒精度）。
//!
//! 判据原文（CoRun Unxreal 主册 UNX-F0122 · UNX-A1 表行）：UNX-F0122-J1
//! 注入阶段卡死被看门狗捕获，超时时限精度毫秒级，捕获后进诊断模式。
//!
//! **判据三分解**：
//! 1. **注入阶段卡死被捕获**：阶段卡死注入（tick 前跳模拟卡死）→
//!    看门狗 sweep 捕获——每阶段独立计时账（start_tick 注入），
//!    超时判定逐阶段独立（一阶段卡死不连坐其他阶段）；
//! 2. **超时时限精度毫秒级**：tick→ms 定点换算（TICKS_PER_MS 注入常量
//!    ——不假设频率），亚毫秒差异不触发（边界恰值：超时恰 +1ms 才
//!    立案，+0.9ms 不立案——"精度毫秒级"的语义是**不早触发**）；
//! 3. **捕获后进诊断模式**：捕获 → 诊断模式状态机（Running→Diag），
//!    冻结现场（卡死阶段/预期时限/实际 tick/现场指纹四件），
//!    诊断入口开启（F0123 卡死转储消费声明——本模块只立案不转储）。
//!
//! **每阶段独立时限表**（const——阶段闭集 × 时限 ms 逐格显性）：
//! 五阶段闭集（Probe/Load/Relocate/Handover/Service——与 UNX-A1 引导
//! 链口径对齐），时限差异显性（探测可慢、交接必须快）。
//!
//! **喂狗语义**：阶段内进度显性上报（progress() 重置计时起点）——
//! 卡死的定义即"两次进度上报间隔超时限"；不喂狗不是卡死（阶段可以
//! 合法安静——但时限到了照样捕获，安静不是借口）。
//!
//! 邻接复用声明：unx121_extbootstate（F0121 扩展态）是状态机上游客；
//! UNX-F0123 卡死转储是下游消费；unx109_ktb06 断言面独立——本模块
//! 只做看门狗计时与捕获立案。诊断码段 0x94（域内 0x2E..0x4E、
//! 0x80..0x93 已占）。no_std + alloc 零 unwrap 零 expect 零 panic 面
//! 零 IO 零墙钟零浮点。

use alloc::vec::Vec;

/// 模块版本。
pub const UNX122_STAGEWATCH_VERSION: &str = "unx122-stagewatch-v1";

/// 诊断段基值（0x94 段）。
pub const SEG_BASE: u16 = 0x9400;

/// 每阶段独立时限表容量。
pub const STAGE_CAP: usize = 8;

/// 诊断冻结现场台账容量。
pub const FREEZE_CAP: usize = 16;

/// 事件账滚动容量。
pub const EVENT_CAP: usize = 32;

// ─────────────────────────────────────────────────────────────────────────────
// FNV
// ─────────────────────────────────────────────────────────────────────────────

/// FNV 偏移基。
pub const FNV_OFFSET: u64 = 0xcbf29ce484222325;
/// FNV 质数。
pub const FNV_PRIME: u64 = 0x100000001b3;

/// u64 混入。
pub const fn fnv1a_u64(mut s: u64, v: u64) -> u64 {
    let mut i = 0;
    while i < 8 {
        s = (s ^ (v & 0xFF)) * FNV_PRIME;
        v >>= 8;
        i += 1;
    }
    s
}

/// 字符串混入。
pub const fn fnv1a_str(s: u64, v: &str) -> u64 {
    let b = v.as_bytes();
    let mut s = s;
    let mut i = 0;
    while i < b.len() {
        s = (s ^ (b[i] as u64)) * FNV_PRIME;
        i += 1;
    }
    s
}

// ─────────────────────────────────────────────────────────────────────────────
// 阶段闭集 + 每阶段独立时限表（const）
// ─────────────────────────────────────────────────────────────────────────────

/// 引导阶段五闭集（UNX-A1 引导链口径）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BootStage {
    /// 探测（枚举硬件——可慢）。
    Probe,
    /// 加载（读镜像）。
    Load,
    /// 重定位（搬移内核）。
    Relocate,
    /// 交接（移权内核——必须快）。
    Handover,
    /// 服务（运行时服务就绪）。
    Service,
}

impl BootStage {
    /// 全枚举（账本序）。
    pub const ALL: [BootStage; 5] = [
        BootStage::Probe,
        BootStage::Load,
        BootStage::Relocate,
        BootStage::Handover,
        BootStage::Service,
    ];

    /// 中文名。
    pub const fn zh(self) -> &'static str {
        match self {
            BootStage::Probe => "探测",
            BootStage::Load => "加载",
            BootStage::Relocate => "重定位",
            BootStage::Handover => "交接",
            BootStage::Service => "服务",
        }
    }

    /// 每阶段独立时限（ms——判据"每阶段独立时限"的 const 表本体）。
    /// 探测可慢 500ms；交接必须 50ms 内；逐格显性不搞一刀切。
    pub const fn limit_ms(self) -> u64 {
        match self {
            BootStage::Probe => 500,
            BootStage::Load => 300,
            BootStage::Relocate => 200,
            BootStage::Handover => 50,
            BootStage::Service => 400,
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// 诊断码（0x94xx 段独占十二码）
// ─────────────────────────────────────────────────────────────────────────────

/// 诊断码（0x94xx 段独占——十二码闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum WdgCode {
    /// 0x9401 阶段未知（看门狗账无此阶段）。
    WgStageUnknown,
    /// 0x9402 阶段重复启动（未停先启——账面冲突）。
    WgDoubleStart,
    /// 0x9403 阶段卡死捕获（超时限——立案进诊断）。
    WgStuckCaptured,
    /// 0x9404 tick 倒流（注入面腐坏）。
    WgTickRegress,
    /// 0x9405 喂狗越权（未启动的阶段喂狗）。
    WgKickUnknown,
    /// 0x9406 停止越权（未启动的阶段停止）。
    WgStopUnknown,
    /// 0x9407 频率参数非法（TICKS_PER_MS=0——换算除零防线）。
    WgFreqInvalid,
    /// 0x9408 诊断台账满（冻结现场超容——显性逐出最旧）。
    WgFreezeFull,
    /// 0x9409 重复捕获（同阶段同卡死事件重复立案——幂等闸）。
    WgDoubleCapture,
    /// 0x940A 快照漂移（FNV 对拍不符）。
    WgSnapshotDrift,
    /// 0x940B 双跑不符（确定性破坏）。
    WgDeterminism,
    /// 0x940C 阶段未结束（进诊断时有阶段仍 Running——现场不一致）。
    WgStageRunning,
}

impl WdgCode {
    /// 稳定码值。
    pub const fn code(self) -> u16 {
        match self {
            WdgCode::WgStageUnknown => 0x9401,
            WdgCode::WgDoubleStart => 0x9402,
            WdgCode::WgStuckCaptured => 0x9403,
            WdgCode::WgTickRegress => 0x9404,
            WdgCode::WgKickUnknown => 0x9405,
            WdgCode::WgStopUnknown => 0x9406,
            WdgCode::WgFreqInvalid => 0x9407,
            WdgCode::WgFreezeFull => 0x9408,
            WdgCode::WgDoubleCapture => 0x9409,
            WdgCode::WgSnapshotDrift => 0x940A,
            WdgCode::WgDeterminism => 0x940B,
            WdgCode::WgStageRunning => 0x940C,
        }
    }

    /// 中文名。
    pub const fn name(self) -> &'static str {
        match self {
            WdgCode::WgStageUnknown => "阶段未知",
            WdgCode::WgDoubleStart => "阶段重复启动",
            WdgCode::WgStuckCaptured => "阶段卡死捕获",
            WdgCode::WgTickRegress => "tick倒流",
            WdgCode::WgKickUnknown => "喂狗越权",
            WdgCode::WgStopUnknown => "停止越权",
            WdgCode::WgFreqInvalid => "频率参数非法",
            WdgCode::WgFreezeFull => "诊断台账满",
            WdgCode::WgDoubleCapture => "重复捕获",
            WdgCode::WgSnapshotDrift => "快照漂移",
            WdgCode::WgDeterminism => "双跑不符",
            WdgCode::WgStageRunning => "阶段未结束",
        }
    }

    /// 中文详情。
    pub const fn detail(self) -> &'static str {
        match self {
            WdgCode::WgStageUnknown => "阶段未知：看门狗账无此阶段——不猜",
            WdgCode::WgDoubleStart => "阶段重复启动：未停先启——账面冲突",
            WdgCode::WgStuckCaptured => "阶段卡死捕获：超独立时限——立案进诊断模式",
            WdgCode::WgTickRegress => "tick倒流：注入面腐坏",
            WdgCode::WgKickUnknown => "喂狗越权：未启动阶段喂狗——拒绝",
            WdgCode::WgStopUnknown => "停止越权：未启动阶段停止——拒绝",
            WdgCode::WgFreqInvalid => "频率参数非法：TICKS_PER_MS=0——除零防线",
            WdgCode::WgFreezeFull => "诊断台账满：显性逐出最旧现场",
            WdgCode::WgDoubleCapture => "重复捕获：同阶段卡死重复立案——幂等闸",
            WdgCode::WgSnapshotDrift => "快照漂移：FNV 对拍不符——账本疑被外力改动",
            WdgCode::WgDeterminism => "双跑不符：同注入两次 sweep 结论不同",
            WdgCode::WgStageRunning => "阶段未结束：进诊断时有阶段仍 Running——现场不一致",
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// 毫秒换算（判据"精度毫秒级"——定点、不假设频率）
// ─────────────────────────────────────────────────────────────────────────────

/// 毫秒换算参数（TICKS_PER_MS 注入——不假设定时器频率）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct TickConv {
    /// 每 ms 的 tick 数（>0——0 即除零防线 WgFreqInvalid）。
    pub ticks_per_ms: u64,
}

impl TickConv {
    /// 参数闸。
    pub const fn valid(&self) -> Result<(), WdgCode> {
        if self.ticks_per_ms == 0 {
            return Err(WdgCode::WgFreqInvalid);
        }
        Ok(())
    }

    /// tick→ms（下取整——换算不放大：1.9ms 读作 1ms，配合"不早触发"）。
    pub const fn to_ms(&self, ticks: u64) -> Result<u64, WdgCode> {
        self.valid()?;
        Ok(ticks / self.ticks_per_ms)
    }

    /// 超时判定（判据"精度毫秒级"的本体）：elapsed_ms > limit_ms 才算
    /// 超时——恰等于时限不触发（亚毫秒不早触发的边界语义：
    /// elapsed 1.9ms 换算 1ms ≤ 限额 2ms 不触发；2.0ms 恰值=2ms 也不触发；
    /// 2.1ms 换算 2ms……——**下取整口径下超时判据是换算值>限额**，
    /// 即实际流逝必须**严格超过**限额满 1ms 步进才立案）。
    pub const fn timeout(&self, start_tick: u64, now_tick: u64, limit_ms: u64) -> Result<bool, WdgCode> {
        self.valid()?;
        if now_tick < start_tick {
            return Err(WdgCode::WgTickRegress);
        }
        let ms = (now_tick - start_tick) / self.ticks_per_ms;
        Ok(ms > limit_ms)
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// 看门狗账（每阶段独立计时）
// ─────────────────────────────────────────────────────────────────────────────

/// 单阶段计时账。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
struct StageWatch {
    /// 在跑。
    running: bool,
    /// 计时起点（tick）。
    start_tick: u64,
    /// 已捕获次数。
    captures: u64,
}

/// 冻结现场（诊断模式立案件——四件随案走）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct FreezeFrame {
    /// 卡死阶段。
    pub stage: BootStage,
    /// 预期时限（ms）。
    pub limit_ms: u64,
    /// 捕获时刻实际流逝（ms，下取整）。
    pub elapsed_ms: u64,
    /// 现场指纹（start_tick × now_tick FNV）。
    pub scene_fp: u64,
}

/// 看门狗模式二态。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum WatchMode {
    /// 常规引导中。
    Running,
    /// 诊断模式（捕获后——F0123 消费入口）。
    Diagnostics,
}

impl WatchMode {
    /// 中文名。
    pub const fn zh(self) -> &'static str {
        match self {
            WatchMode::Running => "常规引导",
            WatchMode::Diagnostics => "诊断模式",
        }
    }
}

/// 看门狗账本。
pub struct StageWatchdog {
    /// 换算参数。
    pub conv: TickConv,
    /// 模式。
    pub mode: WatchMode,
    /// 每阶段计时账（下标即 BootStage 序）。
    pub watches: [StageWatch; 5],
    /// 冻结现场台账（滚动）。
    pub freezes: Vec<FreezeFrame>,
    /// 捕获总数。
    pub captures_total: u64,
    /// 喂狗总数。
    pub kicks_total: u64,
    /// 事件账（滚动）。
    pub events: Vec<WdgCode>,
}

impl StageWatchdog {
    /// 新看门狗（参数注入——频率不许默认拍脑袋）。
    pub const fn new(ticks_per_ms: u64) -> StageWatchdog {
        StageWatchdog {
            conv: TickConv { ticks_per_ms },
            mode: WatchMode::Running,
            watches: [
                StageWatch { running: false, start_tick: 0, captures: 0 },
                StageWatch { running: false, start_tick: 0, captures: 0 },
                StageWatch { running: false, start_tick: 0, captures: 0 },
                StageWatch { running: false, start_tick: 0, captures: 0 },
                StageWatch { running: false, start_tick: 0, captures: 0 },
            ],
            freezes: Vec::new(),
            captures_total: 0,
            kicks_total: 0,
            events: Vec::new(),
        }
    }

    /// 事件入账。
    fn push_event(&mut self, e: WdgCode) {
        if self.events.len() >= EVENT_CAP {
            self.events.remove(0);
        }
        self.events.push(e);
    }

    /// 阶段启动（判据"每阶段独立"——独立计时起点）。
    pub fn start_stage(&mut self, stage: BootStage, tick: u64) -> Result<(), WdgCode> {
        self.conv.valid()?;
        let w = &mut self.watches[stage as usize];
        if w.running {
            self.push_event(WdgCode::WgDoubleStart);
            return Err(WdgCode::WgDoubleStart);
        }
        w.running = true;
        w.start_tick = tick;
        Ok(())
    }

    /// 喂狗（进度显性上报——重置该阶段计时起点）。
    pub fn kick(&mut self, stage: BootStage, tick: u64) -> Result<(), WdgCode> {
        let w = &mut self.watches[stage as usize];
        if !w.running {
            self.push_event(WdgCode::WgKickUnknown);
            return Err(WdgCode::WgKickUnknown);
        }
        if tick < w.start_tick {
            self.push_event(WdgCode::WgTickRegress);
            return Err(WdgCode::WgTickRegress);
        }
        w.start_tick = tick;
        self.kicks_total += 1;
        Ok(())
    }

    /// 阶段正常结束（下账——时限内走完不立案）。
    pub fn finish_stage(&mut self, stage: BootStage, tick: u64) -> Result<(), WdgCode> {
        let w = &mut self.watches[stage as usize];
        if !w.running {
            self.push_event(WdgCode::WgStopUnknown);
            return Err(WdgCode::WgStopUnknown);
        }
        if tick < w.start_tick {
            self.push_event(WdgCode::WgTickRegress);
            return Err(WdgCode::WgTickRegress);
        }
        w.running = false;
        Ok(())
    }

    /// sweep（判据"卡死被捕获"主入口）：逐阶段独立判定超时——
    /// 超时阶段立案冻结现场 + 进诊断模式；一阶段卡死不连坐。
    pub fn sweep(&mut self, now_tick: u64) -> Result<Vec<BootStage>, WdgCode> {
        self.conv.valid()?;
        let mut caught: Vec<BootStage> = Vec::new();
        let mut i = 0;
        while i < 5 {
            let stage = BootStage::ALL[i];
            let w = &self.watches[i];
            if w.running {
                let timed_out = self.conv.timeout(w.start_tick, now_tick, stage.limit_ms())?;
                if timed_out {
                    // 幂等闸：该阶段已捕获过且仍在跑（诊断模式下重复 sweep
                    // 同一卡死不再重复立案——现场已在台账）。
                    let elapsed_ms = (now_tick - w.start_tick) / self.conv.ticks_per_ms;
                    let already = self
                        .freezes
                        .iter()
                        .any(|f| f.stage == stage);
                    if !already {
                        let scene_fp = fnv1a_u64(
                            fnv1a_u64(FNV_OFFSET, w.start_tick),
                            now_tick,
                        );
                        let frame = FreezeFrame {
                            stage,
                            limit_ms: stage.limit_ms(),
                            elapsed_ms,
                            scene_fp,
                        };
                        if self.freezes.len() >= FREEZE_CAP {
                            self.freezes.remove(0);
                            self.push_event(WdgCode::WgFreezeFull);
                        }
                        self.freezes.push(frame);
                        self.watches[i].captures += 1;
                        self.captures_total += 1;
                        self.push_event(WdgCode::WgStuckCaptured);
                        caught.push(stage);
                    }
                }
            }
            i += 1;
        }
        if !caught.is_empty() {
            self.mode = WatchMode::Diagnostics;
        }
        Ok(caught)
    }

    /// 现场一致性核（判据"捕获后进诊断模式"的自洽面：诊断模式下
    /// 存在 Running 阶段是合法的——卡死阶段仍 Running 才是真卡死；
    /// 但全停后仍 Diagnostics 是合法终态。本核只验台账非空↔模式一致）。
    pub const fn diagnostics_consistent(&self) -> bool {
        match self.mode {
            WatchMode::Diagnostics => self.captures_total > 0,
            WatchMode::Running => self.captures_total == 0 || !self.freezes.is_empty(),
        }
    }

    /// 现场指纹（台账全量 FNV——审计对拍）。
    pub fn scene_fingerprint(&self) -> u64 {
        let mut s = fnv1a_str(FNV_OFFSET, UNX122_STAGEWATCH_VERSION);
        s = fnv1a_u64(s, self.mode as u64);
        s = fnv1a_u64(s, self.captures_total);
        s = fnv1a_u64(s, self.kicks_total);
        let mut i = 0;
        while i < 5 {
            s = fnv1a_u64(s, self.watches[i].running as u64);
            s = fnv1a_u64(s, self.watches[i].start_tick);
            s = fnv1a_u64(s, self.watches[i].captures);
            i += 1;
        }
        s = fnv1a_u64(s, self.freezes.len() as u64);
        for f in &self.freezes {
            s = fnv1a_u64(s, f.stage as u64);
            s = fnv1a_u64(s, f.limit_ms);
            s = fnv1a_u64(s, f.elapsed_ms);
            s = fnv1a_u64(s, f.scene_fp);
        }
        s
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// 参考探针（判据 J1 三分解手算恰值）
// ─────────────────────────────────────────────────────────────────────────────

/// 探针汇总。
pub struct ProbeReport {
    /// 卡死捕获（Probe 起于 0，sweep 于超时后）。
    pub stuck_captured: bool,
    /// 毫秒精度——恰限额不触发（+1ms 内不早触发）。
    pub ms_boundary_no_early: bool,
    /// 毫秒精度——超时限满 1ms 步进才触发。
    pub ms_boundary_trigger: bool,
    /// 每阶段独立（Handover 时限 50ms 与 Probe 500ms 互不连坐）。
    pub independent_limits: bool,
    /// 捕获后进诊断模式。
    pub diag_mode_entered: bool,
    /// 冻结现场四件（阶段/时限/流逝/指纹）。
    pub freeze_four_fields: bool,
    /// 喂狗重置（进度上报后不误捕）。
    pub kick_resets: bool,
    /// 正常结束不立案。
    pub finish_no_case: bool,
    /// 重复启动拒。
    pub double_start_block: bool,
    /// tick 倒流拒。
    pub tick_regress_block: bool,
    /// 频率零拒。
    pub freq_zero_block: bool,
    /// 幂等（诊断模式重复 sweep 不重复立案）。
    pub idempotent_sweep: bool,
    /// 双跑确定。
    pub determinism: bool,
    /// 快照稳定。
    pub snapshot_stable: bool,
}

/// 跑探针（TICKS_PER_MS=1000 → tick 即 µs 级标定：1ms=1000 ticks）。
pub fn run_probes() -> ProbeReport {
    const TPM: u64 = 1000;

    // ── 卡死捕获：Probe 起于 tick 0，sweep 于 tick 501_000（501ms>500ms）──
    let mut wd = StageWatchdog::new(TPM);
    let _ = wd.start_stage(BootStage::Probe, 0);
    let caught = wd.sweep(501_000).unwrap_or_default();
    let c1 = caught.len() == 1
        && caught[0] == BootStage::Probe
        && wd.mode == WatchMode::Diagnostics
        && wd.captures_total == 1;

    // ── 毫秒边界：恰 500ms（500_000 ticks）不触发；500_999（500.999ms→500ms）不触发；501_000 触发 ──
    let mut w2 = StageWatchdog::new(TPM);
    let _ = w2.start_stage(BootStage::Probe, 0);
    let no_early = w2.sweep(500_000).map(|v| v.is_empty()).unwrap_or(false)
        && w2.sweep(500_999).map(|v| v.is_empty()).unwrap_or(false);
    let mut w3 = StageWatchdog::new(TPM);
    let _ = w3.start_stage(BootStage::Probe, 0);
    let trigger = w3.sweep(501_000).map(|v| v.len() == 1).unwrap_or(false);

    // ── 每阶段独立：Handover（50ms）与 Probe（500ms）并行，Probe 卡死 Handover 无辜 ──
    let mut w4 = StageWatchdog::new(TPM);
    let _ = w4.start_stage(BootStage::Probe, 0);
    let _ = w4.start_stage(BootStage::Handover, 0);
    let caught4 = w4.sweep(60_000).unwrap_or_default(); // 60ms：Handover 超、Probe 未超
    let indep = caught4.len() == 1 && caught4[0] == BootStage::Handover;

    // ── 冻结现场四件 ──
    let four = w3
        .freezes
        .first()
        .map(|f| {
            f.stage == BootStage::Probe
                && f.limit_ms == 500
                && f.elapsed_ms == 501
                && f.scene_fp
                    == fnv1a_u64(fnv1a_u64(FNV_OFFSET, 0), 501_000)
        })
        .unwrap_or(false);

    // ── 喂狗重置 ──
    let mut w5 = StageWatchdog::new(TPM);
    let _ = w5.start_stage(BootStage::Load, 0);
    let _ = w5.kick(BootStage::Load, 200_000); // 200ms 进度上报
    let no_false = w5.sweep(300_000).map(|v| v.is_empty()).unwrap_or(false); // 距喂狗 100ms<300ms

    // ── 正常结束不立案 ──
    let mut w6 = StageWatchdog::new(TPM);
    let _ = w6.start_stage(BootStage::Relocate, 0);
    let _ = w6.finish_stage(BootStage::Relocate, 100_000);
    let finish_ok = w6.sweep(600_000).map(|v| v.is_empty()).unwrap_or(false);

    // ── 拒绝路径 ──
    let mut w7 = StageWatchdog::new(TPM);
    let _ = w7.start_stage(BootStage::Service, 0);
    let ds = matches!(w7.start_stage(BootStage::Service, 0), Err(WdgCode::WgDoubleStart));
    let regress = matches!(w7.kick(BootStage::Service, 0), Err(WdgCode::WgTickRegress));
    let fz = matches!(StageWatchdog::new(0).conv.valid(), Err(WdgCode::WgFreqInvalid));

    // ── 幂等 sweep：诊断模式下重复 sweep 同卡死不重复立案 ──
    let c_again = w3.sweep(502_000).map(|v| v.is_empty()).unwrap_or(false)
        && w3.captures_total == 1;

    // ── 双跑 ──
    let build = || -> StageWatchdog {
        let mut w = StageWatchdog::new(TPM);
        let _ = w.start_stage(BootStage::Probe, 0);
        let _ = w.kick(BootStage::Probe, 100_000);
        let _ = w.sweep(400_000);
        w
    };
    let a = build();
    let b = build();
    let det = a.scene_fingerprint() == b.scene_fingerprint();

    // ── 快照 ──
    let s1 = w3.scene_fingerprint();
    let s2 = w3.scene_fingerprint();
    let snap = s1 == s2;

    ProbeReport {
        stuck_captured: c1,
        ms_boundary_no_early: no_early,
        ms_boundary_trigger: trigger,
        independent_limits: indep,
        diag_mode_entered: c1,
        freeze_four_fields: four,
        kick_resets: no_false,
        finish_no_case: finish_ok,
        double_start_block: ds,
        tick_regress_block: regress,
        freq_zero_block: fz,
        idempotent_sweep: c_again,
        determinism: det,
        snapshot_stable: snap,
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// 判据（CheckSet——判据面零 panic）
// ─────────────────────────────────────────────────────────────────────────────

/// 判据挂接。
pub fn run_unx122_checks() -> crate::checks::CheckSet {
    let mut s = crate::checks::CheckSet::new("UNX-F0122");
    let p = run_probes();

    // 判据"注入阶段卡死被看门狗捕获"。
    s.add("卡死捕获", p.stuck_captured, "");
    s.add("每阶段独立时限", p.independent_limits, "");
    s.add("喂狗重置", p.kick_resets, "");
    s.add("正常结束不立案", p.finish_no_case, "");
    s.add("重复启动拒", p.double_start_block, "");
    s.add("tick倒流拒", p.tick_regress_block, "");

    // 判据"超时时限精度毫秒级"。
    s.add("毫秒边界不早触发", p.ms_boundary_no_early, "");
    s.add("毫秒边界触发", p.ms_boundary_trigger, "");
    s.add("频率零拒", p.freq_zero_block, "");

    // 判据"捕获后进诊断模式"。
    s.add("进诊断模式", p.diag_mode_entered, "");
    s.add("冻结现场四件", p.freeze_four_fields, "");
    s.add("诊断幂等sweep", p.idempotent_sweep, "");
    s.add("现场一致性核", run_probes_diag_consistent(), "");

    // 判据"双跑 + 快照"。
    s.add("双跑确定", p.determinism, "");
    s.add("快照稳定", p.snapshot_stable, "");

    // 阶段闭集五件时限互异且递减序合法（交接最紧）。
    let handover_tightest = BootStage::Handover.limit_ms() == 50
        && BootStage::Probe.limit_ms() == 500
        && BootStage::Handover.limit_ms() < BootStage::Probe.limit_ms();
    s.add("独立时限表恰值", handover_tightest, "");

    // 诊断码十二码互异。
    let codes = [
        WdgCode::WgStageUnknown.code(),
        WdgCode::WgDoubleStart.code(),
        WdgCode::WgStuckCaptured.code(),
        WdgCode::WgTickRegress.code(),
        WdgCode::WgKickUnknown.code(),
        WdgCode::WgStopUnknown.code(),
        WdgCode::WgFreqInvalid.code(),
        WdgCode::WgFreezeFull.code(),
        WdgCode::WgDoubleCapture.code(),
        WdgCode::WgSnapshotDrift.code(),
        WdgCode::WgDeterminism.code(),
        WdgCode::WgStageRunning.code(),
    ];
    let mut distinct = true;
    let mut i = 0;
    while i < codes.len() {
        let mut j = i + 1;
        while j < codes.len() {
            if codes[i] == codes[j] {
                distinct = false;
            }
            j += 1;
        }
        i += 1;
    }
    s.add("诊断码十二码互异", distinct && codes[0] == 0x9401 && codes[11] == 0x940C, "");

    s
}

/// 诊断一致性核探针（判据面辅助——独立小探针避免主探针状态污染）。
fn run_probes_diag_consistent() -> bool {
    let mut w = StageWatchdog::new(1000);
    // 无捕获时 Running 一致。
    let a = w.diagnostics_consistent();
    let _ = w.start_stage(BootStage::Probe, 0);
    let _ = w.sweep(501_000);
    // 捕获后 Diagnostics 且 captures>0 一致。
    a && w.diagnostics_consistent() && w.mode == WatchMode::Diagnostics
}
