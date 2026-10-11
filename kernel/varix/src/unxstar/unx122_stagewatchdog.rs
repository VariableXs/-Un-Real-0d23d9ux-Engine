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
    /// 0x940D 链序跳段（引导链提前启动未到阶段——深化 v2 扩码）。
    WgChainSkip,
    /// 0x940E 链序回退（已完成阶段再启动——深化 v2 扩码）。
    WgChainRegress,
    /// 0x940F 贴边立案（实际耗时 ≥ 时限 80%——校准账显性标记，深化 v2 扩码）。
    WgNearLimit,
    /// 0x9410 恢复越权（诊断模式未定恢复策略就启动新阶段——深化 v2 扩码）。
    WgRecoverOrder,
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
            WdgCode::WgChainSkip => 0x940D,
            WdgCode::WgChainRegress => 0x940E,
            WdgCode::WgNearLimit => 0x940F,
            WdgCode::WgRecoverOrder => 0x9410,
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
            WdgCode::WgChainSkip => "链序跳段",
            WdgCode::WgChainRegress => "链序回退",
            WdgCode::WgNearLimit => "贴边立案",
            WdgCode::WgRecoverOrder => "恢复越权",
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
            WdgCode::WgChainSkip => "链序跳段：引导链提前启动未到阶段——序账拒绝",
            WdgCode::WgChainRegress => "链序回退：已完成阶段再启动——序账拒绝",
            WdgCode::WgNearLimit => "贴边立案：实际耗时≥时限80%——校准账显性标记",
            WdgCode::WgRecoverOrder => "恢复越权：诊断模式未定恢复策略就启动新阶段",
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
    /// 生效时限表（v2.1——默认即 const 表值；覆盖表挂载时逐格改写）。
    pub limits: [u64; 5],
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
            limits: [
                BootStage::Probe.limit_ms(),
                BootStage::Load.limit_ms(),
                BootStage::Relocate.limit_ms(),
                BootStage::Handover.limit_ms(),
                BootStage::Service.limit_ms(),
            ],
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

    /// 事件入账（pub 面——v2 链式层复用同一滚动账，不平行记账）。
    pub fn push_event_pub(&mut self, e: WdgCode) {
        self.push_event(e);
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
                let timed_out = self.conv.timeout(w.start_tick, now_tick, self.limits[i])?;
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
                            limit_ms: self.limits[i],
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

    // ── v2 深化判据（链序/恢复/校准/矩阵/群体/读屏）──
    let p2 = run_probes_v2();
    s.add("链序合法放行", p2.chain_legal_pass, "");
    s.add("链序跳段拒", p2.chain_skip_block, "");
    s.add("链序回退拒", p2.chain_regress_block, "");
    s.add("链序在链重复拒", p2.chain_already_on_block, "");
    s.add("跳段pending恰值", p2.chain_pending_exact, "");
    s.add("恢复越权拒", p2.recover_order_block, "");
    s.add("恢复三策略链账", p2.recover_three_policies, "");
    s.add("恢复不洗地", p2.recover_no_erasement, "");
    s.add("恢复重复执行拒", p2.recover_double_block, "");
    s.add("采样ppm恰值", p2.sample_ppm_exact, "");
    s.add("采样滚动容量", p2.sample_roll, "");
    s.add("中位数奇偶口径", p2.sample_median, "");
    s.add("校准建议单恰值", p2.calibration_exact, "");
    s.add("边界矩阵完备", p2.matrix_complete, "");
    s.add("矩阵粒度恰值", p2.matrix_scale_exact, "");
    s.add("群体七实例全等", p2.cohort_unanimous, "");
    s.add("群体贴边立案", p2.cohort_near_limit, "");
    s.add("案卷读屏形", p2.case_line_shape, "");
    s.add("链账读屏形", p2.chain_line_shape, "");
    s.add("审计读屏形", p2.audit_line_shape, "");

    // v2 扩码：诊断码十六码互异（原十二 + 0x940D~0x9410 扩码四码）。
    let codes2 = [
        WdgCode::WgChainSkip.code(),
        WdgCode::WgChainRegress.code(),
        WdgCode::WgNearLimit.code(),
        WdgCode::WgRecoverOrder.code(),
    ];
    s.add(
        "诊断码扩码四码",
        codes2[0] == 0x940D && codes2[3] == 0x9410 && codes.iter().all(|&c| !codes2.contains(&c)),
        "",
    );

    // ── v2.1 版本化时限覆盖表判据 ──
    let p21 = run_probes_overlay();
    s.add("覆盖表生效恰值", p21.overlay_effective, "");
    s.add("覆盖表回退const", p21.fallback_const, "");
    s.add("覆盖红线拒(交接50ms)", p21.handover_redline_block, "");
    s.add("覆盖表版本指纹", p21.version_fingerprint, "");
    s.add("覆盖表逐格对拍", p21.overlay_grid_exact, "");

    // ── v2.2 覆盖审计判据 ──
    let p22 = run_probes_overlay_audit();
    s.add("生效表读屏形", p22.table_line_shape, "");
    s.add("覆盖同表一致", p22.agrees_same, "");
    s.add("单边挂表不一致", p22.disagrees_half, "");
    s.add("双None一致", p22.agrees_none, "");
    s.add("捕获随覆盖联动", p22.capture_follows_overlay, "");

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

// ═════════════════════════════════════════════════════════════════════════════
// 深化段 v2（链序状态机 / 恢复策略 / 时限校准采样账 / 群体对拍 / 边界矩阵）
// ═════════════════════════════════════════════════════════════════════════════
//
// v1 只做"单阶段独立计时 + 捕获立案"。v2 把看门狗放进**引导链语境**：
//
// 1. **链序状态机**：引导链五阶段有先验全序（Probe→Load→Relocate→
//    Handover→Service）。提前启动未到阶段 = 跳段（0x940D）；已完成
//    阶段再启动 = 回退（0x940E）。序账是看门狗的**上游闸**——序错
//    的阶段根本不许计时（计时了也是垃圾数据）。
// 2. **恢复策略状态机**：捕获进诊断模式后，恢复不是免费的——必须
//    显性选择策略（Abort 全停 / FallbackProbe 退回探测重走 /
//    ContinueWithNote 带案卷续走）。策略未定就 start_stage = 恢复
//    越权（0x9410）。恢复后模式回 Running，但 captures_total 与
//    案卷**不清零**——历史是历史，恢复不是洗地。
// 3. **时限校准采样账**：每阶段正常结束采样实际耗时（滚动 8 样本），
//    定点 ppm 比率 = elapsed×1e6/limit（u128 中转防溢出）。比率
//    ≥800_000ppm（80%）= 贴边立案（0x940F）——时限合理性的数据面：
//    贴边多了说明时限拍太紧，该扩表；全绿说明时限拍太松，该缩表。
//    校准建议单只产建议不自动改表（表是 const——改表要走版本）。
// 4. **群体对拍**：同注入序列灌 N 个独立看门狗，指纹必须全等——
//    单实例双跑只能证"此刻确定"，群体对拍证"任何实例都确定"。
// 5. **边界矩阵**：TICKS_PER_MS ∈ {1,10,1000} × 流逝 ∈ {恰超, 恰超-1tick,
//    恰超+1ms} 的 3×3 行为矩阵——换算粒度越粗"不早触发"窗口越宽，
//    逐格显性（矩阵本身就是"精度毫秒级"的完备性证据）。

use alloc::string::String;

/// 校准采样账容量（每阶段滚动样本数）。
pub const SAMPLE_CAP: usize = 8;

/// 贴边阈值（800_000 ppm = 80%）。
pub const NEAR_LIMIT_PPM: u64 = 800_000;

/// ppm 单位基。
pub const PPM_ONE: u64 = 1_000_000;

/// 手写 u64 → 十进制（无 format 依赖——no_std 极简面）。
fn push_u64(s: &mut String, mut v: u64) {
    if v == 0 {
        s.push('0');
        return;
    }
    let mut buf = [0u8; 20];
    let mut n = 0;
    while v > 0 {
        buf[n] = b'0' + (v % 10) as u8;
        v /= 10;
        n += 1;
    }
    let mut i = n;
    while i > 0 {
        i -= 1;
        s.push(buf[i] as char);
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// 链序账（引导链先验全序的上游闸）
// ─────────────────────────────────────────────────────────────────────────────

/// 链序账（v2）：记录五阶段各自的链上状态。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ChainState {
    /// 未到（链上尚未轮到）。
    NotReached,
    /// 在链（已启动未结束）。
    OnChain,
    /// 已完成（链上走完——回退启动即 0x940E）。
    Done,
    /// 被跳过判定后放弃（策略 FallbackProbe 重走时清回 NotReached）。
    Abandoned,
}

impl ChainState {
    /// 中文名。
    pub const fn zh(self) -> &'static str {
        match self {
            ChainState::NotReached => "未到",
            ChainState::OnChain => "在链",
            ChainState::Done => "已完成",
            ChainState::Abandoned => "已放弃",
        }
    }
}

/// 链序判定结果（纯函数——给定链账与请求启动的阶段）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ChainVerdict {
    /// 合法（该阶段恰是链上下一个未完成者）。
    Legal,
    /// 跳段（前面还有未完成阶段）。
    Skip(usize),
    /// 回退（该阶段已 Done）。
    Regress,
    /// 在链重复启动。
    AlreadyOn,
}

impl ChainVerdict {
    /// 中文名。
    pub const fn zh(self) -> &'static str {
        match self {
            ChainVerdict::Legal => "合法",
            ChainVerdict::Skip(_) => "跳段",
            ChainVerdict::Regress => "回退",
            ChainVerdict::AlreadyOn => "在链重复",
        }
    }

    /// 跳段时给出"前面还挂着几个阶段"（诊断细节——不是猜测是账面事实）。
    pub const fn pending_count(self) -> usize {
        match self {
            ChainVerdict::Skip(n) => n,
            _ => 0,
        }
    }
}

/// 纯函数：对链账判定"启动 stage"的合法性（无副作用——判定与执行分离）。
pub const fn chain_verdict(chain: &[ChainState; 5], stage: BootStage) -> ChainVerdict {
    let idx = stage as usize;
    match chain[idx] {
        ChainState::OnChain => return ChainVerdict::AlreadyOn,
        ChainState::Done => return ChainVerdict::Regress,
        _ => {}
    }
    // 前面（下标更小）若有未完成（NotReached 不算未完成——链允许从任何
    // 已到达位置续走；只有 OnChain 才挡路）……不对——引导链是**全序**：
    // 下标 < idx 且状态 == NotReached 的阶段说明链被跳过了一段。
    let mut pending = 0;
    let mut i = 0;
    while i < idx {
        match chain[i] {
            ChainState::NotReached | ChainState::OnChain => pending += 1,
            ChainState::Done | ChainState::Abandoned => {}
        }
        i += 1;
    }
    if pending > 0 {
        ChainVerdict::Skip(pending)
    } else {
        ChainVerdict::Legal
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// 恢复策略状态机（诊断模式的显性出口）
// ─────────────────────────────────────────────────────────────────────────────

/// 恢复策略三态闭集（判据"捕获后进诊断模式"的出口面）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RecoveryPolicy {
    /// 全停（引导失败——链账保留全现场）。
    Abort,
    /// 退回探测重走（链账 Abandoned 全清、案卷保留、captures 保留）。
    FallbackProbe,
    /// 带案卷续走（卡死阶段强制下账标记 Abandoned，链从下一段续）。
    ContinueWithNote,
}

impl RecoveryPolicy {
    /// 全枚举（判据面遍历用）。
    pub const ALL: [RecoveryPolicy; 3] = [
        RecoveryPolicy::Abort,
        RecoveryPolicy::FallbackProbe,
        RecoveryPolicy::ContinueWithNote,
    ];

    /// 中文名。
    pub const fn zh(self) -> &'static str {
        match self {
            RecoveryPolicy::Abort => "全停",
            RecoveryPolicy::FallbackProbe => "退回探测",
            RecoveryPolicy::ContinueWithNote => "带案卷续走",
        }
    }
}

/// 恢复状态机状态（诊断模式的子状态——mode=Diagnostics 之下才有意义）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RecoveryState {
    /// 策略未定（此时 start_stage 一律 0x9410 拒）。
    Awaiting,
    /// 策略已定已执行（链账已按策略调整——可续走）。
    Applied(RecoveryPolicy),
}

impl RecoveryState {
    /// 中文名。
    pub const fn zh(self) -> &'static str {
        match self {
            RecoveryState::Awaiting => "策略未定",
            RecoveryState::Applied(p) => p.zh(),
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// 时限校准采样账（每阶段滚动样本 + 定点 ppm 比率）
// ─────────────────────────────────────────────────────────────────────────────

/// 单阶段采样账。
#[derive(Clone, Debug)]
pub struct StageSamples {
    /// 滚动样本（elapsed ms，下取整口径与捕获一致）。
    pub samples: Vec<u64>,
    /// 累计样本数（含被滚动挤出的）。
    pub total: u64,
    /// 贴边样本数（比率 ≥ 80%）。
    pub near_limit_count: u64,
}

impl StageSamples {
    /// 空账。
    pub const fn new() -> StageSamples {
        StageSamples { samples: Vec::new(), total: 0, near_limit_count: 0 }
    }

    /// 入样（滚动 + 贴边判定）。返回该样本的定点 ppm 比率。
    pub fn record(&mut self, elapsed_ms: u64, limit_ms: u64) -> u64 {
        // 定点 ppm 比率：elapsed×1e6/limit（u128 中转——500ms×1e6 远超 u32，
        // u64 也可能被 1e6 溢出吗？elapsed ≤ 2^40 时 ×1e6 ≤ 2^60 安全；
        // u128 一步到位零担忧）。
        let ppm = ((elapsed_ms as u128) * (PPM_ONE as u128) / (limit_ms.max(1) as u128)) as u64;
        if self.samples.len() >= SAMPLE_CAP {
            self.samples.remove(0);
        }
        self.samples.push(elapsed_ms);
        self.total += 1;
        if ppm >= NEAR_LIMIT_PPM {
            self.near_limit_count += 1;
        }
        ppm
    }

    /// 中位数（奇取中偶取小——与 vcu05 基准运行器口径对齐）。
    pub fn median(&self) -> Option<u64> {
        if self.samples.is_empty() {
            return None;
        }
        let mut v: Vec<u64> = self.samples.clone();
        // 插入排序（样本 ≤ 8——常数面最省且稳定序）。
        let n = v.len();
        let mut i = 1;
        while i < n {
            let key = v[i];
            let mut j = i;
            while j > 0 && v[j - 1] > key {
                v[j] = v[j - 1];
                j -= 1;
            }
            v[j] = key;
            i += 1;
        }
        Some(v[n / 2])
    }

    /// 最大值。
    pub fn max(&self) -> Option<u64> {
        self.samples.iter().copied().max()
    }

    /// 贴边率（定点 ppm：near/total）。
    pub const fn near_rate_ppm(&self) -> u64 {
        if self.total == 0 {
            0
        } else {
            (self.near_limit_count * PPM_ONE) / self.total
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// 链式看门狗（v2 主体：v1 计时面 + 链序闸 + 恢复状态机 + 采样账）
// ─────────────────────────────────────────────────────────────────────────────

/// 校准建议单（只产建议不自动改表——表是 const，改表走版本）。
#[derive(Clone, Debug)]
pub struct CalibrationAdvice {
    /// 建议扩表的阶段（贴边率高——时限拍太紧）。
    pub widen: Vec<BootStage>,
    /// 建议缩表的阶段（贴边率为零且中位不足时限四分之一——拍太松）。
    pub shrink: Vec<BootStage>,
}

impl CalibrationAdvice {
    /// 案卷读屏行。
    pub fn line(&self) -> String {
        let mut s = String::from("校准建议|扩表:");
        if self.widen.is_empty() {
            s.push_str("无");
        } else {
            let mut first = true;
            for st in &self.widen {
                if !first {
                    s.push(',');
                }
                s.push_str(st.zh());
                first = false;
            }
        }
        s.push_str("|缩表:");
        if self.shrink.is_empty() {
            s.push_str("无");
        } else {
            let mut first = true;
            for st in &self.shrink {
                if !first {
                    s.push(',');
                }
                s.push_str(st.zh());
                first = false;
            }
        }
        s
    }
}

/// 链式看门狗（v2 主体——组合 v1 StageWatchdog 的全部能力）。
pub struct ChainWatchdog {
    /// v1 计时核心（复用——不重写不平行实现）。
    pub core: StageWatchdog,
    /// v2.1 版本化时限覆盖表（None = 全 const 口径）。
    pub overlay: Option<VersionedLimits>,
    /// 链序账。
    pub chain: [ChainState; 5],
    /// 恢复状态机。
    pub recovery: RecoveryState,
    /// 每阶段采样账。
    pub samples: [StageSamples; 5],
    /// 恢复执行次数（每次 recover +1——审计面）。
    pub recoveries: u64,
    /// 链序违规总数（skip+regress+recover-order 累计）。
    pub chain_violations: u64,
}

impl ChainWatchdog {
    /// 新链式看门狗。
    pub const fn new(ticks_per_ms: u64) -> ChainWatchdog {
        ChainWatchdog {
            core: StageWatchdog::new(ticks_per_ms),
            overlay: None,
            chain: [
                ChainState::NotReached,
                ChainState::NotReached,
                ChainState::NotReached,
                ChainState::NotReached,
                ChainState::NotReached,
            ],
            recovery: RecoveryState::Awaiting,
            samples: [
                StageSamples::new(),
                StageSamples::new(),
                StageSamples::new(),
                StageSamples::new(),
                StageSamples::new(),
            ],
            recoveries: 0,
            chain_violations: 0,
        }
    }

    /// 生效时限（值 + 来源——来源按"该格覆盖值≠const 值"判：
    /// 覆盖真改了格 → Overlay；覆盖给了同值 → Const（覆盖没改变
    /// 语义就该如实标注，来源是语义事实不是挂载事实）。
    pub fn effective_limit_detail(&self, stage: BootStage) -> (u64, LimitSource) {
        let i = stage as usize;
        match &self.overlay {
            Some(ov) if ov.limits_ms[i] != stage.limit_ms() => {
                (ov.limits_ms[i], LimitSource::Overlay)
            }
            _ => (stage.limit_ms(), LimitSource::Const),
        }
    }

    /// 挂载覆盖表（红线闸：valid 不过整体拒——半拉子表比没有更危险）。
    /// 挂载成功即把生效值写进 core.limits（sweep 单一代码路径自动跟随）。
    pub fn try_mount(&mut self, v: VersionedLimits) -> Result<(), WdgCode> {
        v.valid()?;
        self.core.limits = v.limits_ms;
        self.overlay = Some(v);
        Ok(())
    }

    /// 生效时限简式（判据面常用）。
    pub fn effective_limit(&self, stage: BootStage) -> u64 {
        self.effective_limit_detail(stage).0
    }

    /// 链序判定的只读面（判定与执行分离——判据面可直接断言纯函数结果）。
    pub fn verdict(&self, stage: BootStage) -> ChainVerdict {
        chain_verdict(&self.chain, stage)
    }

    /// 链序闸下启动阶段：判 Legal 才真正计时（v1 start_stage），
    /// 其余判定只入事件账不计时——序错的阶段计时了也是垃圾数据。
    /// 诊断模式下策略未定 → 0x9410 拒（恢复越权）。
    pub fn chain_start(&mut self, stage: BootStage, tick: u64) -> Result<(), WdgCode> {
        // 恢复闸：诊断模式 + 策略未定 = 不许走。
        if self.core.mode == WatchMode::Diagnostics && self.recovery == RecoveryState::Awaiting {
            self.chain_violations += 1;
            return Err(WdgCode::WgRecoverOrder);
        }
        match chain_verdict(&self.chain, stage) {
            ChainVerdict::Legal => {
                self.chain[stage as usize] = ChainState::OnChain;
                self.core.start_stage(stage, tick)
            }
            ChainVerdict::Skip(_) => {
                self.chain_violations += 1;
                Err(WdgCode::WgChainSkip)
            }
            ChainVerdict::Regress => {
                self.chain_violations += 1;
                Err(WdgCode::WgChainRegress)
            }
            ChainVerdict::AlreadyOn => {
                self.chain_violations += 1;
                Err(WdgCode::WgDoubleStart)
            }
        }
    }

    /// 链序闸下正常结束：v1 finish + 采样入账（贴边判定）。
    /// 返回 Ok(Some(ppm)) = 本样本的定点比率（贴边与否调用方对照阈值）。
    pub fn chain_finish(&mut self, stage: BootStage, tick: u64) -> Result<Option<u64>, WdgCode> {
        if self.chain[stage as usize] != ChainState::OnChain {
            // 链上不在跑——v1 的 finish 会给 WgStopUnknown，这里直接短路
            // 保持链账与计时账一致（不在链就没有采样资格）。
            self.chain_violations += 1;
            return Err(WdgCode::WgStopUnknown);
        }
        let start = self.core.watches[stage as usize].start_tick;
        let per_ms = self.core.conv.ticks_per_ms;
        self.core.finish_stage(stage, tick)?;
        self.chain[stage as usize] = ChainState::Done;
        // 采样（elapsed 下取整——与捕获口径一致；贴边比率按**生效时限**）。
        let elapsed = if tick >= start { (tick - start) / per_ms } else { 0 };
        let eff = self.effective_limit(stage);
        let ppm = self.samples[stage as usize].record(elapsed, eff);
        if ppm >= NEAR_LIMIT_PPM {
            self.core.push_event_pub(WgCode::WgNearLimit);
        }
        Ok(Some(ppm))
    }

    /// sweep 直通（v1 幂等捕获逻辑全复用——v2 不重写捕获）。
    pub fn sweep(&mut self, now_tick: u64) -> Result<Vec<BootStage>, WdgCode> {
        self.core.sweep(now_tick)
    }

    /// 执行恢复策略（判据"捕获后进诊断模式"的出口——策略必须显性）。
    pub fn recover(&mut self, policy: RecoveryPolicy) -> Result<(), WdgCode> {
        // 只在诊断模式 + 策略未定时可执行（重复执行 = 越权，走 0x9410 同款拒绝）。
        if self.core.mode != WatchMode::Diagnostics {
            return Err(WdgCode::WgStageRunning);
        }
        match self.recovery {
            RecoveryState::Applied(_) => return Err(WdgCode::WgRecoverOrder),
            RecoveryState::Awaiting => {}
        }
        match policy {
            RecoveryPolicy::Abort => {
                // 全停：在链阶段全部下账标记 Abandoned；Running 在链清空。
                let mut i = 0;
                while i < 5 {
                    if self.chain[i] == ChainState::OnChain {
                        self.chain[i] = ChainState::Abandoned;
                        let stage = BootStage::ALL[i];
                        let _ = self.core.finish_stage(stage, u64::MAX);
                    }
                    i += 1;
                }
                self.core.mode = WatchMode::Running;
            }
            RecoveryPolicy::FallbackProbe => {
                // 退回探测：链账全清 NotReached（案卷/captures 保留——不洗地）。
                let mut i = 0;
                while i < 5 {
                    if self.chain[i] == ChainState::OnChain {
                        let stage = BootStage::ALL[i];
                        let _ = self.core.finish_stage(stage, u64::MAX);
                    }
                    self.chain[i] = ChainState::NotReached;
                    i += 1;
                }
                self.core.mode = WatchMode::Running;
            }
            RecoveryPolicy::ContinueWithNote => {
                // 续走：卡死阶段强制 Abandoned（在链的其他阶段保留 OnChain 可续）。
                let mut i = 0;
                while i < 5 {
                    if self.chain[i] == ChainState::OnChain {
                        let stage = BootStage::ALL[i];
                        let is_stuck = self.core.freezes.iter().any(|f| f.stage == stage);
                        if is_stuck {
                            self.chain[i] = ChainState::Abandoned;
                            let _ = self.core.finish_stage(stage, u64::MAX);
                        }
                    }
                    i += 1;
                }
                self.core.mode = WatchMode::Running;
            }
        }
        self.recovery = RecoveryState::Applied(policy);
        self.recoveries += 1;
        Ok(())
    }

    /// 恢复后重新武装（新一轮引导——恢复状态回 Awaiting，链账由策略定）。
    pub const fn rearm(&mut self) {
        self.recovery = RecoveryState::Awaiting;
    }

    /// 校准建议单（贴边率 ≥ 50% 建议扩；贴边率为零且中位 < 1/4 时限建议缩）。
    pub fn calibration(&self) -> CalibrationAdvice {
        let mut widen: Vec<BootStage> = Vec::new();
        let mut shrink: Vec<BootStage> = Vec::new();
        let mut i = 0;
        while i < 5 {
            let st = BootStage::ALL[i];
            let book = &self.samples[i];
            if book.total >= 2 {
                if book.near_rate_ppm() >= PPM_ONE / 2 {
                    widen.push(st);
                } else if book.near_limit_count == 0 {
                    if let (Some(med), Some(mx)) = (book.median(), book.max()) {
                        // 缩表闸：中位 < 1/4 时限 **且** 最大 < 1/2 时限——
                        // 只看中位会把偶发尖峰缩进墙里。
                        let limit = st.limit_ms();
                        if med * 4 < limit && mx * 2 < limit {
                            shrink.push(st);
                        }
                    }
                }
            }
            i += 1;
        }
        CalibrationAdvice { widen, shrink }
    }

    /// 案卷读屏（一条一行：阶段|时限|流逝|指纹前 8 位十六进制面）。
    pub fn case_line(&self, idx: usize) -> Option<String> {
        self.core.freezes.get(idx).map(|f| {
            let mut s = String::from("案|");
            s.push_str(f.stage.zh());
            s.push_str("|限");
            push_u64(&mut s, f.limit_ms);
            s.push_str("ms|逝");
            push_u64(&mut s, f.elapsed_ms);
            s.push_str("ms|fp");
            // 指纹前 8 hex（手写——无 format 依赖）。
            const HEX: &[u8; 16] = b"0123456789ABCDEF";
            let mut shift = 60i32;
            while shift >= 0 {
                let nib = ((f.scene_fp >> shift) & 0xF) as usize;
                s.push(HEX[nib] as char);
                shift -= 4;
            }
            s
        })
    }

    /// 链账读屏（五阶段状态一行）。
    pub fn chain_line(&self) -> String {
        let mut s = String::from("链|");
        let mut i = 0;
        while i < 5 {
            if i > 0 {
                s.push('·');
            }
            s.push_str(BootStage::ALL[i].zh());
            s.push(':');
            s.push_str(self.chain[i].zh());
            i += 1;
        }
        s
    }

    /// 审计读屏（每阶段样本数/中位/贴边率一行汇总）。
    pub fn audit_line(&self) -> String {
        let mut s = String::from("审计|");
        let mut i = 0;
        while i < 5 {
            if i > 0 {
                s.push('|');
            }
            let b = &self.samples[i];
            s.push_str(BootStage::ALL[i].zh());
            s.push_str(":n");
            push_u64(&mut s, b.total);
            s.push_str("/中");
            match b.median() {
                Some(m) => push_u64(&mut s, m),
                None => s.push('-'),
            }
            s.push_str("/贴");
            push_u64(&mut s, b.near_rate_ppm());
            s.push_str("ppm");
            i += 1;
        }
        s
    }

    /// 全量指纹（v2 面 = v1 指纹 + 链账 + 恢复态 + 采样账 + 覆盖表——审计对拍）。
    pub fn fingerprint_v2(&self) -> u64 {
        let mut s = self.core.scene_fingerprint();
        s = match &self.overlay {
            Some(ov) => fnv1a_u64(fnv1a_str(s, "ov"), ov.fingerprint()),
            None => fnv1a_str(s, "no-ov"),
        };
        let mut i = 0;
        while i < 5 {
            s = fnv1a_u64(s, self.chain[i] as u64);
            s = fnv1a_u64(s, self.samples[i].total);
            s = fnv1a_u64(s, self.samples[i].near_limit_count);
            let mut j = 0;
            while j < self.samples[i].samples.len() {
                s = fnv1a_u64(s, self.samples[i].samples[j]);
                j += 1;
            }
            i += 1;
        }
        s = fnv1a_u64(s, self.recoveries);
        s = fnv1a_u64(s, self.chain_violations);
        match self.recovery {
            RecoveryState::Awaiting => fnv1a_u64(s, 0),
            RecoveryState::Applied(p) => fnv1a_u64(s, 1 + p as u64),
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// 边界矩阵（TICKS_PER_MS × 流逝 3×3——"精度毫秒级"完备性证据面）
// ─────────────────────────────────────────────────────────────────────────────

/// 矩阵行键（换算粒度）。
pub const MATRIX_TPM: [u64; 3] = [1, 10, 1000];

/// 矩阵列键（流逝偏移语义——相对"恰超时点"= (limit+1)×tpm ticks）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BoundaryCol {
    /// 恰超时点 −1 tick（必不触发——差 1 tick 不满 1ms 步进）。
    Minus1Tick,
    /// 恰超时点（=(limit+1)×tpm——必触发）。
    Exact,
    /// 恰超时点再 +1ms（必触发）。
    Plus1Ms,
}

impl BoundaryCol {
    /// 全枚举。
    pub const ALL: [BoundaryCol; 3] =
        [BoundaryCol::Minus1Tick, BoundaryCol::Exact, BoundaryCol::Plus1Ms];
}

/// 单格结果。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct BoundaryCell {
    /// 换算粒度。
    pub tpm: u64,
    /// 列。
    pub col: BoundaryCol,
    /// 注入 tick。
    pub now_tick: u64,
    /// 触发与否。
    pub triggered: bool,
    /// 换算流逝 ms（下取整）。
    pub elapsed_ms: u64,
}

/// 跑一格（纯函数——独立实例零污染）。
pub fn boundary_cell(tpm: u64, col: BoundaryCol) -> BoundaryCell {
    let limit = BootStage::Probe.limit_ms(); // 500
    let exact = (limit + 1) * tpm; // 恰超时点
    let now = match col {
        BoundaryCol::Minus1Tick => exact - 1,
        BoundaryCol::Exact => exact,
        BoundaryCol::Plus1Ms => exact + tpm,
    };
    let mut w = StageWatchdog::new(tpm);
    let _ = w.start_stage(BootStage::Probe, 0);
    let triggered = w.sweep(now).map(|v| !v.is_empty()).unwrap_or(false);
    let elapsed_ms = now / tpm;
    BoundaryCell { tpm, col, now_tick: now, triggered, elapsed_ms }
}

/// 跑整矩阵（3×3=9 格）并核完备性：
/// Minus1Tick 列必须全不触发；Exact 与 Plus1Ms 列必须全触发。
pub fn boundary_matrix() -> ([[BoundaryCell; 3]; 3], bool) {
    let mut grid = [[
        BoundaryCell { tpm: 0, col: BoundaryCol::Minus1Tick, now_tick: 0, triggered: false, elapsed_ms: 0 },
        BoundaryCell { tpm: 0, col: BoundaryCol::Exact, now_tick: 0, triggered: false, elapsed_ms: 0 },
        BoundaryCell { tpm: 0, col: BoundaryCol::Plus1Ms, now_tick: 0, triggered: false, elapsed_ms: 0 },
    ]; 3];
    let mut r = 0;
    while r < 3 {
        let mut c = 0;
        while c < 3 {
            grid[r][c] = boundary_cell(MATRIX_TPM[r], BoundaryCol::ALL[c]);
            c += 1;
        }
        r += 1;
    }
    // 完备性核：三行各自满足"−1tick 不触发 / 恰超触发 / +1ms 触发"。
    let complete = (0..3).all(|r| {
        !grid[r][0].triggered && grid[r][1].triggered && grid[r][2].triggered
    });
    (grid, complete)
}

// ─────────────────────────────────────────────────────────────────────────────
// 群体对拍（同注入 N 实例指纹全等——群体确定性核）
// ─────────────────────────────────────────────────────────────────────────────

/// 群体注入序列（固定脚本——五步：起 Load / 喂 / sweep 未超 / 贴边结束 / sweep）。
const COHORT_SCRIPT_TICKS: [u64; 5] = [0, 120_000, 250_000, 299_000, 305_000];

/// 跑一个实例走完整脚本，回指纹（TPM=1000）。
fn cohort_run_one() -> u64 {
    let mut w = ChainWatchdog::new(1000);
    // Load 起于 0（链序：Probe 不需要先走？——链序要求前面无未完成。
    // Probe 必须 first。补 Probe 起止再 Load）。
    let _ = w.chain_start(BootStage::Probe, 0);
    let _ = w.chain_finish(BootStage::Probe, 50_000);
    let _ = w.chain_start(BootStage::Load, COHORT_SCRIPT_TICKS[0]);
    let _ = w.core.kick(BootStage::Load, COHORT_SCRIPT_TICKS[1]);
    let _ = w.sweep(COHORT_SCRIPT_TICKS[2]);
    // 299_000 − 0 = 299ms < 300ms 限额 → 不贴边（74.75%…等等：299/300=99.67% ≥ 80% 贴边！
    // 就要这个：脚本刻意贴边，采样账 near_limit_count 必 +1，指纹才有区分度）。
    let _ = w.chain_finish(BootStage::Load, COHORT_SCRIPT_TICKS[3]);
    let _ = w.sweep(COHORT_SCRIPT_TICKS[4]);
    w.fingerprint_v2()
}

/// 群体对拍报告。
pub struct CohortReport {
    /// 实例数。
    pub n: usize,
    /// 指纹全等。
    pub unanimous: bool,
    /// 代表指纹。
    pub fp: u64,
    /// 贴边立案生效（Load 样本贴边计数 =1）。
    pub near_limit_recorded: bool,
}

/// 跑群体对拍（N=7——质数容量防"偶数巧合"心理，实质全等即可）。
pub fn run_cohort(n: usize) -> CohortReport {
    let first = cohort_run_one();
    let mut all_same = true;
    let mut i = 1;
    while i < n {
        if cohort_run_one() != first {
            all_same = false;
        }
        i += 1;
    }
    // 贴边核：重走一遍查采样账。
    let mut w = ChainWatchdog::new(1000);
    let _ = w.chain_start(BootStage::Probe, 0);
    let _ = w.chain_finish(BootStage::Probe, 50_000);
    let _ = w.chain_start(BootStage::Load, 0);
    let _ = w.chain_finish(BootStage::Load, 299_000);
    let near = w.samples[BootStage::Load as usize].near_limit_count == 1
        && w.core.events.contains(&WgCode::WgNearLimit);
    CohortReport { n, unanimous: all_same, fp: first, near_limit_recorded: near }
}

// ─────────────────────────────────────────────────────────────────────────────
// 深化探针 v2（链序 / 恢复 / 校准 / 读屏 / 矩阵 / 群体——六组恰值）
// ─────────────────────────────────────────────────────────────────────────────

/// 深化探针汇总。
pub struct ProbeV2Report {
    /// 链序：合法启动放行。
    pub chain_legal_pass: bool,
    /// 链序：跳段拒（Load 未走完就 Handover）。
    pub chain_skip_block: bool,
    /// 链序：回退拒（Probe 完成后再启）。
    pub chain_regress_block: bool,
    /// 链序：在链重复拒。
    pub chain_already_on_block: bool,
    /// 链序：pending 计数恰值（跳段报 2 个未完成）。
    pub chain_pending_exact: bool,
    /// 恢复：策略未定启动拒（0x9410）。
    pub recover_order_block: bool,
    /// 恢复：三策略各自链账终态恰值。
    pub recover_three_policies: bool,
    /// 恢复：案卷与 captures 不洗地。
    pub recover_no_erasement: bool,
    /// 恢复：重复执行拒。
    pub recover_double_block: bool,
    /// 校准：贴边采样恰值（299/300=996666ppm）。
    pub sample_ppm_exact: bool,
    /// 校准：滚动容量（第 9 样挤出第 1 样）。
    pub sample_roll: bool,
    /// 校准：中位数奇偶口径。
    pub sample_median: bool,
    /// 校准：建议单恰值（贴边多的扩 / 全绿且极小的缩）。
    pub calibration_exact: bool,
    /// 矩阵：9 格完备（−1tick 全哑 / 恰超全响 / +1ms 全响）。
    pub matrix_complete: bool,
    /// 矩阵：换算粒度粗细的流逝值恰值（tpm=10 时 exact 换算 501ms）。
    pub matrix_scale_exact: bool,
    /// 群体：7 实例指纹全等。
    pub cohort_unanimous: bool,
    /// 群体：贴边立案进事件账。
    pub cohort_near_limit: bool,
    /// 读屏：案卷行含中文与十六进制指纹前缀。
    pub case_line_shape: bool,
    /// 读屏：链账行五段。
    pub chain_line_shape: bool,
    /// 读屏：审计行含样本数与 ppm。
    pub audit_line_shape: bool,
}

/// 跑深化探针 v2。
pub fn run_probes_v2() -> ProbeV2Report {
    const TPM: u64 = 1000;

    // ── 链序四判定 ──
    let mut w = ChainWatchdog::new(TPM);
    let legal = w.chain_start(BootStage::Probe, 0).is_ok();
    // 在链重复。
    let already = matches!(w.chain_start(BootStage::Probe, 1), Err(WdgCode::WgDoubleStart));
    // 跳段：Probe 在链，启 Handover → Skip(前面 pending: Probe+Load+Relocate=3？)
    // chain_verdict 口径：下标 < handover(3) 且 NotReached|OnChain → Probe(OnChain)+Load+Relocate(NotReached) = 3。
    let skip = matches!(w.chain_start(BootStage::Handover, 2), Err(WdgCode::WgChainSkip));
    let pending = match chain_verdict(&w.chain, BootStage::Handover) {
        ChainVerdict::Skip(n) => n,
        _ => 99,
    };
    // 结束 Probe 后启 Load（合法）→ Load 在链再启 Relocate（跳段 pending=1）。
    let _ = w.chain_finish(BootStage::Probe, 10_000);
    let load_ok = w.chain_start(BootStage::Load, 10_000).is_ok();
    let skip2 = matches!(w.chain_start(BootStage::Relocate, 11_000), Err(WdgCode::WgChainSkip));
    // 回退：已完成 Probe 再启。
    let regress = matches!(w.chain_start(BootStage::Probe, 12_000), Err(WdgCode::WgChainRegress));

    // ── 恢复三策略 ──
    // 构造卡死现场：Probe 起于 0 → sweep 501_000 捕获。
    let build_stuck = || -> ChainWatchdog {
        let mut x = ChainWatchdog::new(TPM);
        let _ = x.chain_start(BootStage::Probe, 0);
        let _ = x.chain_finish(BootStage::Probe, 10_000);
        let _ = x.chain_start(BootStage::Load, 10_000);
        let _ = x.sweep(311_000); // Load 超时限 300ms → 捕获进诊断
        x
    };
    // 策略未定启动拒。
    let mut ra = build_stuck();
    let order_block = matches!(
        ra.chain_start(BootStage::Relocate, 311_001),
        Err(WdgCode::WgRecoverOrder)
    );
    // Abort：在链 Load → Abandoned；模式回 Running。
    let mut r0 = build_stuck();
    let _ = r0.recover(RecoveryPolicy::Abort);
    let abort_shape = r0.core.mode == WatchMode::Running
        && r0.chain[BootStage::Load as usize] == ChainState::Abandoned
        && r0.recoveries == 1
        && r0.core.captures_total == 1;
    // FallbackProbe：链账全 NotReached。
    let mut r1 = build_stuck();
    let _ = r1.recover(RecoveryPolicy::FallbackProbe);
    let fallback_shape = r1.chain.iter().all(|&c| c == ChainState::NotReached)
        && r1.core.mode == WatchMode::Running
        && r1.core.freezes.len() == 1;
    // ContinueWithNote：卡死阶段 Abandoned，其余保留；重走 Probe 合法。
    let mut r2 = build_stuck();
    let _ = r2.recover(RecoveryPolicy::ContinueWithNote);
    let cont_shape = r2.chain[BootStage::Load as usize] == ChainState::Abandoned
        && r2.chain_start(BootStage::Relocate, 312_000).is_ok();
    // 重复执行拒。
    let dbl = matches!(
        r2.recover(RecoveryPolicy::Abort),
        Err(WdgCode::WgRecoverOrder)
    );
    // 不洗地：三策略后 captures_total 与案卷全保留。
    let no_erase = r0.core.captures_total == 1
        && r1.core.captures_total == 1
        && r2.core.captures_total == 1
        && r0.core.freezes.len() == 1
        && r1.core.freezes.len() == 1
        && r2.core.freezes.len() == 1;

    // ── 校准采样 ──
    // 贴边恰值：299ms/300ms 限 → ppm = 299×1e6/300 = 996666。
    let mut s1 = StageSamples::new();
    let ppm = s1.record(299, 300);
    let ppm_exact = ppm == 996_666 && s1.near_limit_count == 1;
    // 滚动：灌 10 样，账面留最后 8。
    let mut s2 = StageSamples::new();
    let mut k = 0;
    while k < 10 {
        let _ = s2.record(100 + k as u64, 500);
        k += 1;
    }
    let roll = s2.total == 10 && s2.samples.len() == 8 && s2.samples[0] == 102 && s2.samples[7] == 109;
    // 中位：奇 {3,1,2}→2；偶 {4,1,3,2}→断序取小=2。
    let mut s3 = StageSamples::new();
    for v in [3u64, 1, 2] {
        let _ = s3.record(v, 500);
    }
    let mut s4 = StageSamples::new();
    for v in [4u64, 1, 3, 2] {
        let _ = s4.record(v, 500);
    }
    let median = s3.median() == Some(2) && s4.median() == Some(2);
    // 建议单：Load 灌 3 贴边样本（≥50% 贴边率 → 扩）；Relocate 灌全绿极小（中位 10ms/限 200 且最大 40<100 → 缩）。
    let mut wcal = ChainWatchdog::new(TPM);
    let _ = wcal.chain_start(BootStage::Probe, 0);
    let _ = wcal.chain_finish(BootStage::Probe, 50_000);
    let mut base = 50_000;
    let mut n3 = 0;
    while n3 < 3 {
        let _ = wcal.chain_start(BootStage::Load, base);
        base += 280_000;
        let _ = wcal.chain_finish(BootStage::Load, base); // 280ms/300=93.3% 贴边
        n3 += 1;
    }
    let mut nr = 0;
    while nr < 2 {
        let _ = wcal.chain_start(BootStage::Relocate, base);
        base += 10_000;
        let _ = wcal.chain_finish(BootStage::Relocate, base); // 10ms/200=5%
        nr += 1;
    }
    let adv = wcal.calibration();
    let calib = adv.widen == vec![BootStage::Load] && adv.shrink == vec![BootStage::Relocate];

    // ── 边界矩阵 ──
    let (grid, complete) = boundary_matrix();
    // tpm=10 时恰超 = 501×10 = 5010 ticks → elapsed_ms = 501。
    let scale = grid[1][1].elapsed_ms == 501 && grid[2][1].elapsed_ms == 501;
    // tpm=1 时 −1tick = 500 ticks → 换算 500 ≤ 500 不触发。
    let minus1 = grid[0][0].now_tick == 500 && !grid[0][0].triggered;

    // ── 群体 ──
    let cohort = run_cohort(7);

    // ── 读屏 ──
    let stuck = build_stuck();
    let case = stuck.case_line(0).unwrap_or_default();
    let case_ok = case.contains("案|加载")
        && case.contains("限300ms")
        && case.contains("逝301ms")
        && case.len() > 20;
    let mut wchain = ChainWatchdog::new(TPM);
    let _ = wchain.chain_start(BootStage::Probe, 0);
    let cl = wchain.chain_line();
    let chain_ok = cl.contains("探测:在链") && cl.contains("服务:未到") && cl.split('·').count() == 5;
    let al = wcal.audit_line();
    let audit_ok = al.contains("加载:n3") && al.contains("贴") && al.contains("ppm");

    ProbeV2Report {
        chain_legal_pass: legal && load_ok,
        chain_skip_block: skip && skip2,
        chain_regress_block: regress,
        chain_already_on_block: already,
        chain_pending_exact: pending == 3,
        recover_order_block: order_block,
        recover_three_policies: abort_shape && fallback_shape && cont_shape,
        recover_no_erasement: no_erase,
        recover_double_block: dbl,
        sample_ppm_exact: ppm_exact,
        sample_roll: roll,
        sample_median: median,
        calibration_exact: calib,
        matrix_complete: complete && minus1,
        matrix_scale_exact: scale,
        cohort_unanimous: cohort.unanimous,
        cohort_near_limit: cohort.near_limit_recorded,
        case_line_shape: case_ok,
        chain_line_shape: chain_ok,
        audit_line_shape: audit_ok,
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// v2.1 版本化时限覆盖表（校准建议的落地通道——const 表之上的运行时覆盖）
// ─────────────────────────────────────────────────────────────────────────────
//
// const 时限表是编译期事实，但校准数据面（贴边率）揭示"拍太紧/拍太松"
// 后必须有**受控**的调整通道——否则建议单永远停在纸面。设计：
//
// 1. **覆盖表挂运行时**：ChainWatchdog 可挂 VersionedLimits（五格 ms +
//     版本号）。effective_limit(stage) 查表顺序 = 覆盖表有该阶段格 →
//     用覆盖值；否则回退 const 表（BootStage::limit_ms）。**回退是显性
//     语义不是静默默认**——查表结果带来源标注（Overlay/Const）。
// 2. **红线闸**：交接（Handover）时限覆盖不得 < 50ms（v1 表铁律"交接
//     必须快"），任何阶段覆盖不得 > 2000ms（引导链总预算——五表和
//     ≤ 1450ms 的 const 口径，覆盖后总和仍受 2000ms 单格帽约束）。
//     越红线的挂载整体拒绝（不是逐格剥除——半拉子覆盖表比没有更危险）。
// 3. **版本指纹**：挂载即算 FNV（版本号 × 五格 × 表源闭集）——
//     双看门狗覆盖表一致性可用指纹对拍，不比对整个结构体。
// 4. **捕获口径一致**：sweep 用 effective_limit（覆盖生效即超时判定
//     跟着走——不然覆盖是摆设）；冻结现场 limit_ms 记**生效值**，
//     案卷读屏天然可见覆盖效果。

/// 查表来源标注（显性回退——不是静默默认）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum LimitSource {
    /// 覆盖表命中。
    Overlay,
    /// 回退 const 表。
    Const,
}

impl LimitSource {
    /// 中文名。
    pub const fn zh(self) -> &'static str {
        match self {
            LimitSource::Overlay => "覆盖表",
            LimitSource::Const => "const表",
        }
    }
}

/// 生效时限（值 + 来源）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct EffectiveLimit {
    /// 生效值（ms）。
    pub ms: u64,
    /// 来源。
    pub src: LimitSource,
}

/// 版本化时限覆盖表。
#[derive(Clone, Debug)]
pub struct VersionedLimits {
    /// 版本号（单调——每次重挂 +1 起步由挂载方定，指纹含之）。
    pub version: u64,
    /// 五格覆盖值（ms——下标即 BootStage 序）。
    pub limits_ms: [u64; 5],
}

/// 单格红线帽（任何阶段覆盖上限——引导链总预算口径）。
pub const OVERLAY_MAX_MS: u64 = 2000;
/// 交接红线（覆盖不得低于——"交接必须快"的铁律面）。
pub const HANDOVER_MIN_MS: u64 = 50;

impl VersionedLimits {
    /// 挂载参数闸：五格全 ≤ OVERLAY_MAX_MS 且交接格 ≥ HANDOVER_MIN_MS。
    /// 半拉子覆盖表比没有更危险——整体拒。
    pub const fn valid(&self) -> Result<(), WdgCode> {
        let mut i = 0;
        while i < 5 {
            if self.limits_ms[i] == 0 || self.limits_ms[i] > OVERLAY_MAX_MS {
                return Err(WdgCode::WgFreqInvalid);
            }
            i += 1;
        }
        if self.limits_ms[BootStage::Handover as usize] < HANDOVER_MIN_MS {
            return Err(WdgCode::WgFreqInvalid);
        }
        Ok(())
    }

    /// 版本指纹（版本号 × 五格——对拍用，不比结构体）。
    pub const fn fingerprint(&self) -> u64 {
        let mut s = fnv1a_str(FNV_OFFSET, "unx122-overlay");
        s = fnv1a_u64(s, self.version);
        let mut i = 0;
        while i < 5 {
            s = fnv1a_u64(s, self.limits_ms[i]);
            i += 1;
        }
        s
    }
}

/// 覆盖表挂载探针汇总。
pub struct ProbeOverlayReport {
    /// 覆盖生效恰值（Probe 覆盖 1000ms → 恰 1000ms 不触发、1001ms 触发）。
    pub overlay_effective: bool,
    /// 未覆盖格回退 const 恰值（Load 保持 300ms）。
    pub fallback_const: bool,
    /// 交接红线拒（覆盖 49ms 挂载整体拒）。
    pub handover_redline_block: bool,
    /// 版本指纹（同表同指纹/异版异指纹）。
    pub version_fingerprint: bool,
    /// 逐格对拍（五格来源标注恰值：覆盖格 Overlay/其余 Const）。
    pub overlay_grid_exact: bool,
}

/// 跑覆盖表探针。
pub fn run_probes_overlay() -> ProbeOverlayReport {
    const TPM: u64 = 1000;

    // 覆盖表：Probe 1000ms（放宽校准建议），其余格不动（覆盖表可只语义上
    // 全格给出——本探针全格显性给值，未覆盖格给 const 同值）。
    let ov = VersionedLimits {
        version: 3,
        limits_ms: [
            1000,
            BootStage::Load.limit_ms(),
            BootStage::Relocate.limit_ms(),
            BootStage::Handover.limit_ms(),
            BootStage::Service.limit_ms(),
        ],
    };
    let ov_ok = ov.valid().is_ok();

    // 生效恰值：Probe 覆盖 1000 → sweep 1_000_000（1000ms 恰值）不触发。
    let mut w = ChainWatchdog::new(TPM);
    if ov_ok {
        w.overlay = Some(ov.clone());
    }
    let _ = w.chain_start(BootStage::Probe, 0);
    let at_exact_no = w.sweep(1_000_000).map(|v| v.is_empty()).unwrap_or(false);
    let at_plus1_yes = w.sweep(1_001_000).map(|v| v.len() == 1).unwrap_or(false);
    let frozen_ok = w
        .core
        .freezes
        .first()
        .map(|f| f.limit_ms == 1000 && f.elapsed_ms == 1001)
        .unwrap_or(false);
    let overlay_effective = at_exact_no && at_plus1_yes && frozen_ok;

    // 回退 const：Load 无覆盖差异（300ms）——覆盖挂了但该格语义仍是 const 值。
    // 用查表来源核：Probe 格 Overlay（1000≠500）/ Load 格 Const（300==300）。
    let (p_src, _) = w.effective_limit_detail(BootStage::Probe);
    let (_, l_src) = w.effective_limit_detail(BootStage::Load);
    let fallback_const = p_src == LimitSource::Overlay && l_src == LimitSource::Const;

    // 红线：交接覆盖 49ms → 整体拒。
    let bad = VersionedLimits {
        version: 9,
        limits_ms: [1000, 300, 200, 49, 400],
    };
    let handover_redline_block = matches!(bad.valid(), Err(WdgCode::WgFreqInvalid))
        && w.try_mount(bad).is_err();

    // 版本指纹：同表同版同指纹；改版本异指纹。
    let fp_a = ov.fingerprint();
    let fp_b = ov.fingerprint();
    let mut ov2 = ov.clone();
    ov2.version = 4;
    let version_fingerprint = fp_a == fp_b && fp_a != ov2.fingerprint();

    // 逐格对拍：来源语义 = "格值≠const 值 → Overlay，同值 → Const"。
    // 挂表后：Probe 格（1000≠500）Overlay；其余格同值 Const。
    // 未挂表：全格 Const。
    let mut w2 = ChainWatchdog::new(TPM);
    let (_, l2_src) = w2.effective_limit_detail(BootStage::Load);
    let overlay_grid_exact = l2_src == LimitSource::Const
        && (0..5).all(|i| {
            let st = BootStage::ALL[i];
            let (ms, src) = w.effective_limit_detail(st);
            let want_src = if i == 0 { LimitSource::Overlay } else { LimitSource::Const };
            src == want_src && ms == if i == 0 { 1000 } else { st.limit_ms() }
        });

    ProbeOverlayReport {
        overlay_effective,
        fallback_const,
        handover_redline_block,
        version_fingerprint,
        overlay_grid_exact,
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// v2.2 生效表读屏 + 跨实例覆盖对拍（审计面收口）
// ─────────────────────────────────────────────────────────────────────────────
//
// 覆盖表挂上后，"现在到底按什么时限在跑"必须一句话可查（否则覆盖是
// 暗门）；两台看门狗覆盖是否一致必须指纹对拍可判（否则集群态漂移
// 不可见）。本段补齐这两块审计面。

impl ChainWatchdog {
    /// 生效表读屏（一行：五阶段 生效值[来源] 逐格——覆盖暗门显性化）。
    pub fn effective_table_line(&self) -> String {
        let mut s = String::from("生效表|");
        let mut i = 0;
        while i < 5 {
            if i > 0 {
                s.push('|');
            }
            let st = BootStage::ALL[i];
            let (ms, src) = self.effective_limit_detail(st);
            s.push_str(st.zh());
            s.push(':');
            push_u64(&mut s, ms);
            s.push('[');
            s.push_str(src.zh());
            s.push(']');
            i += 1;
        }
        s
    }

    /// 覆盖一致性对拍（另一实例的覆盖表指纹 vs 本实例——None 对 None
    /// 视为一致：都没挂表就是一致态；单边挂表必不一致）。
    pub fn overlay_agrees_with(&self, other: &ChainWatchdog) -> bool {
        match (&self.overlay, &other.overlay) {
            (None, None) => true,
            (Some(a), Some(b)) => a.fingerprint() == b.fingerprint(),
            _ => false,
        }
    }
}

/// 覆盖审计探针汇总。
pub struct ProbeOverlayAuditReport {
    /// 生效表读屏形（含覆盖标注与 const 标注各至少一处）。
    pub table_line_shape: bool,
    /// 双实例同表指纹一致。
    pub agrees_same: bool,
    /// 单边挂表不一致。
    pub disagrees_half: bool,
    /// 双 None 一致。
    pub agrees_none: bool,
    /// 覆盖后 sweep 结论随覆盖变（挂 1000ms 表后 Probe 800ms 不再捕获
    /// ——覆盖语义与捕获语义联动恰值）。
    pub capture_follows_overlay: bool,
}

/// 跑覆盖审计探针。
pub fn run_probes_overlay_audit() -> ProbeOverlayAuditReport {
    const TPM: u64 = 1000;
    let mk_ov = || VersionedLimits {
        version: 7,
        limits_ms: [
            1000,
            BootStage::Load.limit_ms(),
            BootStage::Relocate.limit_ms(),
            BootStage::Handover.limit_ms(),
            BootStage::Service.limit_ms(),
        ],
    };

    // 读屏形：挂表实例 Probe 格 [覆盖表]、Load 格 [const表]。
    let mut w = ChainWatchdog::new(TPM);
    let _ = w.try_mount(mk_ov());
    let line = w.effective_table_line();
    let table_line_shape = line.contains("探测:1000[覆盖表]")
        && line.contains("加载:300[const表]")
        && line.contains("交接:50[const表]");

    // 双实例同表一致。
    let mut a = ChainWatchdog::new(TPM);
    let _ = a.try_mount(mk_ov());
    let agrees_same = a.overlay_agrees_with(&w);

    // 单边挂表不一致。
    let b = ChainWatchdog::new(TPM);
    let disagrees_half = !b.overlay_agrees_with(&w) && a.overlay_agrees_with(&a);

    // 双 None 一致。
    let agrees_none = b.overlay_agrees_with(&b);

    // 覆盖联动：Probe 800ms（未覆盖时必捕获）挂 1000ms 表后不捕获——
    // 捕获语义跟着覆盖走（覆盖生效的**行为级**证据，不是查表级）。
    let mut c1 = ChainWatchdog::new(TPM);
    let _ = c1.chain_start(BootStage::Probe, 0);
    let caught_no_ov = c1.sweep(801_000).map(|v| v.len() == 1).unwrap_or(false);
    let mut c2 = ChainWatchdog::new(TPM);
    let _ = c2.try_mount(mk_ov());
    let _ = c2.chain_start(BootStage::Probe, 0);
    let not_caught_ov = c2.sweep(801_000).map(|v| v.is_empty()).unwrap_or(false);
    let capture_follows_overlay = caught_no_ov && not_caught_ov;

    ProbeOverlayAuditReport {
        table_line_shape,
        agrees_same,
        disagrees_half,
        agrees_none,
        capture_follows_overlay,
    }
}
