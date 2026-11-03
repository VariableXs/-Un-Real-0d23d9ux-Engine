//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F1985 · governor 注入矩阵（第十六矩阵） · 0x72800
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定，CoRun·UNX-A3-B20）：
//!   ① 纯功能代码 ≥300 行；
//!   ② 判据原文：UNX-F1985-J1 注入矩阵四族八形态全检出（决策震荡/阈值
//!     越界/回调丢失/热换撕裂），预期声明完备率 100%，与 F1788 组织法一致；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! ─────────────────────────────────────────────────────────────────────
//! 设计要点（组织法与 F1788 对齐：先声明 → 再注入 → 后对账）：
//!   · 四族八形态：A 决策震荡（A0 负载抖动翻频 / A1 相位回退）、
//!     B 阈值越界（B0 上行越限 / B1 下行越限）、
//!     C 回调丢失（C0 回调缺席 / C1 回调重号）、
//!     D 热换撕裂（D0 撕裂标志 / D1 表代际错配）；
//!   · 预期声明：8 形态在注入前逐路声明期望诊断码，完备率必须 8/8，
//!     且声明码与形态语义一一对应、段内唯一；
//!   · 检出：注入后统一走 verify_governor 核验器，命中且命中码与
//!     声明码一致才算抓到（错抓同判漏抓）；
//!   · 注入账：10 轮 × 8 形态轮转，10/10 必抓；
//!   · 状态机 Idle→Sampling→Deciding→Notifying 单调推进，步收口回落
//!     Idle，任何倒退显性拒。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x72800，VX3-1103 governor 注入矩阵窗口独占）。
pub const SEG_BASE: u32 = 0x72800;
/// 健康基线负载序列长度（19 步：升 10 步 + 降 9 步）。
pub const BASELINE_STEPS: usize = 19;
/// 注入形态总数（四族 × 两形态 = 8）。
pub const MATRIX_KINDS: usize = 8;
/// 判据迭代轮数（写死：判据 10/10 语义）。
pub const JUDGE_ROUNDS: u32 = 10;
/// 频率下限（MHz，冻结）。
pub const FREQ_MIN_MHZ: u32 = 800;
/// 频率上限（MHz，冻结）。
pub const FREQ_MAX_MHZ: u32 = 3600;
/// 震荡判定窗口（最近 4 步方向）。
pub const OSC_WINDOW: usize = 4;
/// 窗口内翻转上限（>2 次即震荡）。
pub const OSC_FLIP_CAP: usize = 2;

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum GovInjCode {
    /// 决策震荡：窗口内翻频次数超上限，governor 决策面抖动
    Oscillation,
    /// 阈值越界：请求频率落在 [MIN,MAX] 之外，钳制面失效
    ThresholdCross,
    /// 回调丢失：通知回调序号缺席或重号，回调链断裂
    CallbackLost,
    /// 热换撕裂：策略热换半途生效（撕裂标志或表代际错配）
    HotswapTear,
    /// 状态倒退：相位机单调性被破坏（非收口回落的一律拒）
    StateBack,
    /// 预期声明缺失：注入前未声明期望诊断码，完备率跌破 100%
    DeclMissing,
    /// 轨迹破损：步序/相位链不连续，轨迹账不可信
    TraceBroken,
    /// 双跑不一致：golden_run 两次应同值
    NotReproducible,
}

impl GovInjCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            GovInjCode::Oscillation => "决策震荡：窗口内翻频超上限，查负载采样去抖与迟滞带",
            GovInjCode::ThresholdCross => "阈值越界：频率出界，查钳制面与阈值表上下界",
            GovInjCode::CallbackLost => "回调丢失：回调序号缺席/重号，查通知面登记路径",
            GovInjCode::HotswapTear => "热换撕裂：策略半换生效，查代际戳与撕裂守卫",
            GovInjCode::StateBack => "状态倒退：相位机倒退/平移一律拒，只许单调推进",
            GovInjCode::DeclMissing => "预期声明缺失：注入前必须逐路声明期望码，先补声明再跑",
            GovInjCode::TraceBroken => "轨迹破损：步序或相位链断裂，重出轨迹账",
            GovInjCode::NotReproducible => "双跑不一致：复现红线，golden_run 两次应同值",
        }
    }
    pub const ALL: [GovInjCode; 8] = [
        GovInjCode::Oscillation,
        GovInjCode::ThresholdCross,
        GovInjCode::CallbackLost,
        GovInjCode::HotswapTear,
        GovInjCode::StateBack,
        GovInjCode::DeclMissing,
        GovInjCode::TraceBroken,
        GovInjCode::NotReproducible,
    ];
}

/// 诊断码查表（段外码返回统一文案）。
pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < GovInjCode::ALL.len() {
        if GovInjCode::ALL[i].val() == code {
            return GovInjCode::ALL[i].detail();
        }
        i += 1;
    }
    "未知诊断码：不在 0x72800 段闭集内"
}

// ── governor 相位机（Idle→Sampling→Deciding→Notifying 单调） ────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum GovPhase {
    /// 空闲：一步未起。
    Idle,
    /// 采样：负载样已采。
    Sampling,
    /// 决策：频率已定。
    Deciding,
    /// 通知：回调已发。
    Notifying,
}

impl GovPhase {
    /// 单调序值：Idle=0 < Sampling=1 < Deciding=2 < Notifying=3。
    pub fn rank(self) -> u8 {
        match self {
            GovPhase::Idle => 0,
            GovPhase::Sampling => 1,
            GovPhase::Deciding => 2,
            GovPhase::Notifying => 3,
        }
    }
    /// 只许向前推进；平移与倒退一律 false（收口回落走 close_step 专用口）。
    pub fn can_advance(self, to: GovPhase) -> bool {
        to.rank() > self.rank()
    }
    /// 相位名（诊断文案用）。
    pub fn name(self) -> &'static str {
        match self {
            GovPhase::Idle => "空闲",
            GovPhase::Sampling => "采样",
            GovPhase::Deciding => "决策",
            GovPhase::Notifying => "通知",
        }
    }
}

// ── 阈值表（升序冻结；决策取满足负载的最高档） ──────────────────
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ThrRow {
    /// 负载阈值（百分比）。
    pub load_pct: u32,
    /// 该档目标频率（MHz）。
    pub freq_mhz: u32,
}

/// 冻结阈值表（5 档）。
pub const THR_TABLE: [ThrRow; 5] = [
    ThrRow { load_pct: 10, freq_mhz: 800 },
    ThrRow { load_pct: 30, freq_mhz: 1400 },
    ThrRow { load_pct: 50, freq_mhz: 2000 },
    ThrRow { load_pct: 70, freq_mhz: 2800 },
    ThrRow { load_pct: 90, freq_mhz: 3600 },
];

/// 阈值表核验：升序、频率升序、双双落在 [MIN,MAX]。
pub fn verify_table() -> Result<(), u32> {
    if THR_TABLE.len() < 2 {
        return Err(GovInjCode::TraceBroken.val());
    }
    let mut i = 1usize;
    while i < THR_TABLE.len() {
        let a = match THR_TABLE.get(i - 1) {
            Some(r) => *r,
            None => return Err(GovInjCode::TraceBroken.val()),
        };
        let b = match THR_TABLE.get(i) {
            Some(r) => *r,
            None => return Err(GovInjCode::TraceBroken.val()),
        };
        if a.load_pct >= b.load_pct || a.freq_mhz >= b.freq_mhz {
            return Err(GovInjCode::TraceBroken.val());
        }
        if a.freq_mhz < FREQ_MIN_MHZ || b.freq_mhz > FREQ_MAX_MHZ {
            return Err(GovInjCode::ThresholdCross.val());
        }
        i += 1;
    }
    match THR_TABLE.first() {
        Some(r) if r.freq_mhz >= FREQ_MIN_MHZ => {}
        _ => return Err(GovInjCode::ThresholdCross.val()),
    }
    match THR_TABLE.last() {
        Some(r) if r.freq_mhz <= FREQ_MAX_MHZ => {}
        _ => return Err(GovInjCode::ThresholdCross.val()),
    }
    Ok(())
}

/// 决策函数：取负载能达到的最高档；无档可满足则落 MIN。
pub fn decide_freq(load: u32) -> u32 {
    let mut picked = FREQ_MIN_MHZ;
    let mut i = 0usize;
    while i < THR_TABLE.len() {
        if let Some(r) = THR_TABLE.get(i) {
            if load >= r.load_pct {
                picked = r.freq_mhz;
            }
        }
        i += 1;
    }
    picked
}

// ── 步记录与 governor 域模型 ─────────────────────────────────────
/// 单步记录：seq 全轨单调，cb_seq 连续，gen 为策略代际戳。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StepRec {
    pub seq: u64,
    /// 步起点相位（恒 Idle；A1 注入后可异）。
    pub from: GovPhase,
    /// 步终点相位（恒 Notifying；A1 注入后可异）。
    pub to: GovPhase,
    /// 采样负载。
    pub load: u32,
    /// 决策频率。
    pub freq: u32,
    /// 回调序号（连续递增）。
    pub cb_seq: u64,
    /// 策略代际戳（热换撕裂检测锚）。
    pub gen: u32,
}

/// governor：相位机 + 轨迹账 + 方向窗 + 热换守卫。
#[derive(Clone, Debug)]
pub struct Governor {
    pub phase: GovPhase,
    /// 步序号（从 1 起连续）。
    pub seq: u64,
    /// 回调序号（从 1 起连续）。
    pub cb_seq: u64,
    /// 当前频率。
    pub freq: u32,
    /// 策略代际戳。
    pub policy_gen: u32,
    /// 热换撕裂守卫（置位后任何再步进显性拒）。
    pub torn: bool,
    /// 方向序列（+1 升 / -1 降 / 0 平）。
    pub dirs: Vec<i8>,
    /// 轨迹账。
    pub trace: Vec<StepRec>,
}

impl Governor {
    pub fn new() -> Governor {
        Governor {
            phase: GovPhase::Idle,
            seq: 0,
            cb_seq: 0,
            freq: FREQ_MIN_MHZ,
            policy_gen: 0,
            torn: false,
            dirs: Vec::new(),
            trace: Vec::new(),
        }
    }

    /// 相位推进公开口：倒退/平移显性拒。
    pub fn advance(&mut self, to: GovPhase) -> Result<(), u32> {
        if !self.phase.can_advance(to) {
            return Err(GovInjCode::StateBack.val());
        }
        self.phase = to;
        Ok(())
    }

    /// 步收口：Notifying 回落 Idle（合法回落，唯一倒退白名单）。
    pub fn close_step(&mut self) -> Result<(), u32> {
        if self.phase != GovPhase::Notifying {
            return Err(GovInjCode::StateBack.val());
        }
        self.phase = GovPhase::Idle;
        Ok(())
    }

    /// 单步执行：采样 → 决策 → 通知 → 收口；全程留痕。
    pub fn run_step(&mut self, load: u32) -> Result<(), u32> {
        if self.torn {
            return Err(GovInjCode::HotswapTear.val());
        }
        self.advance(GovPhase::Sampling)?;
        self.advance(GovPhase::Deciding)?;
        let f = decide_freq(load);
        self.advance(GovPhase::Notifying)?;
        self.cb_seq = match self.cb_seq.checked_add(1) {
            Some(v) => v,
            None => return Err(GovInjCode::CallbackLost.val()),
        };
        self.seq = match self.seq.checked_add(1) {
            Some(v) => v,
            None => return Err(GovInjCode::TraceBroken.val()),
        };
        // 方向登记（震荡窗的原料）。
        let dir: i8 = if f > self.freq {
            1
        } else if f < self.freq {
            -1
        } else {
            0
        };
        self.dirs.push(dir);
        self.trace.push(StepRec {
            seq: self.seq,
            from: GovPhase::Idle,
            to: GovPhase::Notifying,
            load,
            freq: f,
            cb_seq: self.cb_seq,
            gen: self.policy_gen,
        });
        self.freq = f;
        self.close_step()
    }
}

/// 健康基线负载序列（升 10 步 + 降 9 步，方向翻转 ≤1，远低于震荡阈）。
pub const LOADS: [u32; BASELINE_STEPS] =
    [2, 12, 22, 32, 42, 52, 62, 72, 82, 92, 82, 72, 62, 52, 42, 32, 22, 12, 2];

/// A0 注入专用负载序列（92/2 交替，窗口内翻转必超限）。
pub const OSC_LOADS: [u32; 6] = [92, 2, 92, 2, 92, 2];

/// 按负载序列建跑一个 governor。
pub fn build_governor(loads: &[u32]) -> Result<Governor, u32> {
    let mut g = Governor::new();
    let mut i = 0usize;
    while i < loads.len() {
        let load = match loads.get(i) {
            Some(v) => *v,
            None => return Err(GovInjCode::TraceBroken.val()),
        };
        g.run_step(load)?;
        i += 1;
    }
    Ok(g)
}

// ── 核验器（注入矩阵的统一检出面） ──────────────────────────────
/// 震荡扫描：最近 OSC_WINDOW 步内方向翻转数 > 上限即震荡。
pub fn scan_oscillation(g: &Governor) -> Result<(), u32> {
    let n = g.dirs.len();
    if n < OSC_WINDOW {
        return Ok(());
    }
    let start = n - OSC_WINDOW;
    let mut flips = 0usize;
    let mut i = start + 1;
    while i < n {
        let a = match g.dirs.get(i - 1) {
            Some(v) => *v,
            None => return Err(GovInjCode::TraceBroken.val()),
        };
        let b = match g.dirs.get(i) {
            Some(v) => *v,
            None => return Err(GovInjCode::TraceBroken.val()),
        };
        if a != 0 && b != 0 && a != b {
            flips += 1;
        }
        i += 1;
    }
    if flips > OSC_FLIP_CAP {
        return Err(GovInjCode::Oscillation.val());
    }
    Ok(())
}

/// 全量核验：撕裂守卫 → 逐记录（序/频/代际/回调/单调/链）→ 震荡扫描。
/// 检查序有意固定：结构破损先于语义违约报告，保证八形态命中码唯一。
pub fn verify_governor(g: &Governor) -> Result<(), u32> {
    if g.torn {
        return Err(GovInjCode::HotswapTear.val());
    }
    verify_table()?;
    let mut i = 0usize;
    while i < g.trace.len() {
        let r = match g.trace.get(i) {
            Some(r) => *r,
            None => return Err(GovInjCode::TraceBroken.val()),
        };
        // ① 步序连续。
        if r.seq != (i + 1) as u64 {
            return Err(GovInjCode::TraceBroken.val());
        }
        // ② 频率界内（B0/B1 越界在此抓）。
        if r.freq < FREQ_MIN_MHZ || r.freq > FREQ_MAX_MHZ {
            return Err(GovInjCode::ThresholdCross.val());
        }
        // ③ 代际戳一致（D1 表代际错配在此抓）。
        if r.gen != g.policy_gen {
            return Err(GovInjCode::HotswapTear.val());
        }
        // ④ 回调连续（C0 缺席 / C1 重号在此抓）。
        if r.cb_seq != (i + 1) as u64 {
            return Err(GovInjCode::CallbackLost.val());
        }
        // ⑤ 相位单调（A1 回退在此抓；先于链查，保证命中码唯一）。
        let mono_ok = r.to.rank() > r.from.rank()
            || (r.from == GovPhase::Notifying && r.to == GovPhase::Idle);
        if !mono_ok {
            return Err(GovInjCode::StateBack.val());
        }
        // ⑥ 相位链：步起点要么 Idle（已收口），要么接上一步终点。
        if i == 0 {
            if r.from != GovPhase::Idle {
                return Err(GovInjCode::TraceBroken.val());
            }
        } else {
            let prev_to = match g.trace.get(i - 1) {
                Some(p) => p.to,
                None => return Err(GovInjCode::TraceBroken.val()),
            };
            if r.from != GovPhase::Idle && r.from != prev_to {
                return Err(GovInjCode::TraceBroken.val());
            }
        }
        i += 1;
    }
    scan_oscillation(g)
}

// ── 预期声明表（先声明后注入，F1788 组织法） ────────────────────
/// 单路声明：形态号 + 期望诊断码 + 中文备注。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct InjCase {
    pub kind: u32,
    pub expect: u32,
    pub note: &'static str,
}

/// 形态语义期望码（声明的唯一权威来源）。
pub fn kind_expected(kind: usize) -> u32 {
    match kind % MATRIX_KINDS {
        0 => GovInjCode::Oscillation.val(),   // A0 负载抖动翻频
        1 => GovInjCode::StateBack.val(),     // A1 相位回退
        2 => GovInjCode::ThresholdCross.val(),// B0 上行越限
        3 => GovInjCode::ThresholdCross.val(),// B1 下行越限
        4 => GovInjCode::CallbackLost.val(),  // C0 回调缺席
        5 => GovInjCode::CallbackLost.val(),  // C1 回调重号
        6 => GovInjCode::HotswapTear.val(),   // D0 撕裂标志
        _ => GovInjCode::HotswapTear.val(),   // D1 表代际错配
    }
}

/// 冻结声明表：8/8 逐路声明，注入前完备率必须 100%。
pub const DECLARED: [InjCase; MATRIX_KINDS] = [
    InjCase { kind: 0, expect: SEG_BASE + 0, note: "A0 决策震荡：负载抖动翻频" },
    InjCase { kind: 1, expect: SEG_BASE + 4, note: "A1 决策震荡：相位回退" },
    InjCase { kind: 2, expect: SEG_BASE + 1, note: "B0 阈值越界：上行越限" },
    InjCase { kind: 3, expect: SEG_BASE + 1, note: "B1 阈值越界：下行越限" },
    InjCase { kind: 4, expect: SEG_BASE + 2, note: "C0 回调丢失：回调缺席" },
    InjCase { kind: 5, expect: SEG_BASE + 2, note: "C1 回调丢失：回调重号" },
    InjCase { kind: 6, expect: SEG_BASE + 3, note: "D0 热换撕裂：撕裂标志" },
    InjCase { kind: 7, expect: SEG_BASE + 3, note: "D1 热换撕裂：表代际错配" },
];

/// 从 (kind, expect) 对里算声明完备率：8/8 且逐路与语义期望一致才算齐。
/// probe 用它做「声明缺失」反例探针。
pub fn declared_complete_from(pairs: &[(u32, u32)]) -> bool {
    if pairs.len() != MATRIX_KINDS {
        return false;
    }
    let mut i = 0usize;
    while i < pairs.len() {
        let (k, e) = match pairs.get(i) {
            Some(p) => *p,
            None => return false,
        };
        if k != i as u32 {
            return false;
        }
        if e == 0 || e != kind_expected(i) {
            return false;
        }
        i += 1;
    }
    true
}

/// 冻结声明表完备率（判据原文：预期声明完备率 100%）。
pub fn declared_complete() -> bool {
    let mut pairs: Vec<(u32, u32)> = Vec::new();
    let mut i = 0usize;
    while i < DECLARED.len() {
        if let Some(d) = DECLARED.get(i) {
            pairs.push((d.kind, d.expect));
        }
        i += 1;
    }
    // 声明码还须段内唯一可寻址。
    let window_ok = SEG_BASE + (MATRIX_KINDS as u32) <= SEG_BASE + 0x40;
    declared_complete_from(&pairs) && window_ok && DECLARED.len() == MATRIX_KINDS
}

// ── 注入器（第十六矩阵本体：8 形态逐路失真） ────────────────────
/// 对健康基线注入第 kind 路形态，核验器必须以声明码命中。
/// 返回 Ok(true)=按声明码抓到；Ok(false)=漏抓/错抓（同判违约）。
pub fn inject_and_detect(kind: usize) -> Result<bool, u32> {
    let expect = kind_expected(kind);
    let res: Result<(), u32> = match kind % MATRIX_KINDS {
        // A0：负载抖动翻频（整段重跑，方向窗必超限）。
        0 => {
            let g = build_governor(&OSC_LOADS)?;
            verify_governor(&g)
        }
        // A1：相位回退（改写第 6 步 from/to 成倒退对）。
        1 => {
            let mut g = build_governor(&LOADS)?;
            if let Some(r) = g.trace.get_mut(5) {
                r.from = GovPhase::Notifying;
                r.to = GovPhase::Deciding;
            }
            verify_governor(&g)
        }
        // B0：上行越限（第 6 步频率抬到 MAX+500）。
        2 => {
            let mut g = build_governor(&LOADS)?;
            if let Some(r) = g.trace.get_mut(5) {
                r.freq = FREQ_MAX_MHZ + 500;
            }
            verify_governor(&g)
        }
        // B1：下行越限（第 6 步频率压到 MIN-100）。
        3 => {
            let mut g = build_governor(&LOADS)?;
            if let Some(r) = g.trace.get_mut(5) {
                r.freq = FREQ_MIN_MHZ - 100;
            }
            verify_governor(&g)
        }
        // C0：回调缺席（第 6 步回调序跳 1）。
        4 => {
            let mut g = build_governor(&LOADS)?;
            if let Some(r) = g.trace.get_mut(5) {
                r.cb_seq = r.cb_seq + 1;
            }
            verify_governor(&g)
        }
        // C1：回调重号（第 6 步回调序与第 5 步同号）。
        5 => {
            let mut g = build_governor(&LOADS)?;
            if let Some(r) = g.trace.get_mut(5) {
                r.cb_seq = 5;
            }
            verify_governor(&g)
        }
        // D0：撕裂标志（置位后任何步进显性拒）。
        6 => {
            let mut g = build_governor(&LOADS)?;
            g.torn = true;
            g.run_step(50).map(|_| ())
        }
        // D1：表代际错配（代际戳抬升，旧轨迹全数失配）。
        _ => {
            let mut g = build_governor(&LOADS)?;
            g.policy_gen = g.policy_gen.wrapping_add(1);
            verify_governor(&g)
        }
    };
    Ok(match res {
        Err(e) => e == expect,
        Ok(()) => false,
    })
}

// ── 注入账 ──────────────────────────────────────────────────────
pub struct InjectLedger {
    pub tries: u32,
    pub caught: u32,
    /// 漏抓/错抓的形态号（诊断用）。
    pub miss_kinds: Vec<usize>,
}

impl InjectLedger {
    pub fn new() -> InjectLedger {
        InjectLedger { tries: 0, caught: 0, miss_kinds: Vec::new() }
    }
    pub fn record(&mut self, kind: usize, hit: bool) {
        self.tries += 1;
        if hit {
            self.caught += 1;
        } else {
            self.miss_kinds.push(kind);
        }
    }
    pub fn all_caught(&self) -> bool {
        self.tries > 0 && self.caught == self.tries
    }
}

/// 注入轮：每轮 8 形态逐路注入并核验，轮数 = JUDGE_ROUNDS（10/10）。
/// 首轮先跑健康基线自证核验器不误伤（黄金件必须核验通过）。
pub fn inject_rounds(rounds: u32) -> Result<InjectLedger, u32> {
    let base = build_governor(&LOADS)?;
    verify_governor(&base)?;
    let mut led = InjectLedger::new();
    let mut r = 0u32;
    while r < rounds {
        let kind = (r as usize) % MATRIX_KINDS;
        let hit = inject_and_detect(kind)?;
        led.record(kind, hit);
        r += 1;
    }
    Ok(led)
}

// ── 黄金全链 ────────────────────────────────────────────────────
/// 全链：健康基线 → 核验器 → 声明完备 → 10/10 注入 → 四拒探。
/// 返回 (正旗数, 拒旗数, 轨迹步数, 注入抓到数)。
pub fn golden_run() -> Result<(u32, u32, u64, u32), u32> {
    // 健康基线（决策/阈值/回调/撕裂四路核验器全绿）。
    let g = build_governor(&LOADS)?;
    verify_governor(&g)?;
    let n_steps = g.seq;
    // 声明完备率 100%。
    if !declared_complete() {
        return Err(GovInjCode::DeclMissing.val());
    }
    // 10/10 注入全抓。
    let led = inject_rounds(JUDGE_ROUNDS)?;
    if !led.all_caught() {
        return Err(GovInjCode::Oscillation.val());
    }
    let flags = 1 + (n_steps == BASELINE_STEPS as u64) as u32
        + (led.caught == JUDGE_ROUNDS) as u32
        + verify_table().is_ok() as u32;
    // 四拒探：A0 震荡 / B0 越限 / D0 撕裂 / 声明缺失。
    let rej_a = build_governor(&OSC_LOADS).and_then(|x| verify_governor(&x)).is_err();
    let mut gb = build_governor(&LOADS)?;
    let rej_b = gb.run_step(FREQ_MAX_MHZ + 500).is_err()
        || {
            if let Some(r) = gb.trace.get_mut(0) {
                r.freq = FREQ_MAX_MHZ + 500;
            }
            verify_governor(&gb).is_err()
        };
    let mut gd = build_governor(&LOADS)?;
    gd.torn = true;
    let rej_d = gd.run_step(50).is_err();
    let rej_decl = !declared_complete_from(&[
        (0, kind_expected(0)),
        (1, kind_expected(1)),
        (2, kind_expected(2)),
        (3, 0), // 声明被抹 → 完备率跌破 100%
        (4, kind_expected(4)),
        (5, kind_expected(5)),
        (6, kind_expected(6)),
        (7, kind_expected(7)),
    ]);
    let rejects = rej_a as u32 + rej_b as u32 + rej_d as u32 + rej_decl as u32;
    Ok((flags, rejects, n_steps, led.caught))
}

// ── 探针三序 ────────────────────────────────────────────────────
/// 序 A：黄金全链四正四拒全中。
pub fn probe_seq_a() -> Result<(), u32> {
    match golden_run() {
        Ok((4, 4, _, _)) => Ok(()),
        _ => Err(GovInjCode::NotReproducible.val()),
    }
}

/// 序 B：失真注入 10 轮全抓。
pub fn probe_seq_b() -> Result<(), u32> {
    match inject_rounds(JUDGE_ROUNDS) {
        Ok(l) if l.all_caught() => Ok(()),
        _ => Err(GovInjCode::Oscillation.val()),
    }
}

/// 序 C：冻结常量与段位窗口。
pub fn probe_seq_c() -> Result<(), u32> {
    let in_window = SEG_BASE == 0x72800
        && GovInjCode::NotReproducible.val() < SEG_BASE + 0x40;
    let ok = BASELINE_STEPS == 19 && MATRIX_KINDS == 8 && JUDGE_ROUNDS == 10
        && FREQ_MIN_MHZ == 800 && FREQ_MAX_MHZ == 3600
        && OSC_WINDOW == 4 && OSC_FLIP_CAP == 2 && in_window;
    if ok {
        Ok(())
    } else {
        Err(GovInjCode::NotReproducible.val())
    }
}

// ── 双跑一致（复现红线） ────────────────────────────────────────
pub fn repro_check() -> Result<(), u32> {
    let a = golden_run()?;
    let b = golden_run()?;
    if a != b {
        return Err(GovInjCode::NotReproducible.val());
    }
    Ok(())
}

// ── 段位守卫 ────────────────────────────────────────────────────
pub fn segment_guard() -> bool {
    let mut i = 0;
    while i < GovInjCode::ALL.len() {
        let v = GovInjCode::ALL[i].val();
        if v < SEG_BASE || v >= SEG_BASE + 0x40 {
            return false;
        }
        i += 1;
    }
    true
}

// ── 完成写回（判据③：状态=已完成、结果摘要非空） ────────────────
pub fn writeback_summary() -> Result<String, u32> {
    let g = build_governor(&LOADS)?;
    verify_governor(&g)?;
    let led = inject_rounds(JUDGE_ROUNDS)?;
    if !led.all_caught() || !declared_complete() {
        return Err(GovInjCode::DeclMissing.val());
    }
    Ok(format!(
        "状态=已完成 families=4 forms={} rounds={} caught={}/{} steps={}",
        MATRIX_KINDS, JUDGE_ROUNDS, led.caught, led.tries, g.seq
    ))
}

// ── 判据面 ──────────────────────────────────────────────────────
pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut v: Vec<(&'static str, bool, String)> = Vec::new();
    // J1 健康基线轨迹全绿（核验器不误伤）。
    let base = build_governor(&LOADS);
    let (bp, bd) = match base {
        Ok(g) => match verify_governor(&g) {
            Ok(()) => (true, format!("steps={} freq={}", g.seq, g.freq)),
            Err(e) => (false, format!("核验败 0x{:x} {}", e, detail(e))),
        },
        Err(e) => (false, format!("建跑败 0x{:x} {}", e, detail(e))),
    };
    v.push(("J1-健康基线轨迹", bp, bd));
    // J1 四族八形态全检出（10 轮 10/10）。
    let inj = inject_rounds(JUDGE_ROUNDS);
    let (ip, id) = match &inj {
        Ok(l) => (l.all_caught(), format!("t={} c={} miss={:?}", l.tries, l.caught, l.miss_kinds)),
        Err(e) => (false, format!("err=0x{:x} {}", e, detail(*e))),
    };
    v.push(("J1-四族八形态全检出", ip, id));
    // J1 预期声明完备率 100%。
    v.push((
        "J1-预期声明完备率100%",
        declared_complete(),
        format!("declared={}/{}", MATRIX_KINDS, MATRIX_KINDS),
    ));
    // J1 A 族检出（震荡两形态：A0 负载抖动 + A1 相位回退）。
    let a0 = inject_and_detect(0).unwrap_or(false);
    let a1 = inject_and_detect(1).unwrap_or(false);
    v.push(("J1-A族震荡检出", a0 && a1, format!("A0={} A1={}", a0, a1)));
    // J1 B 族检出（越界两形态：上行 + 下行）。
    let b0 = inject_and_detect(2).unwrap_or(false);
    let b1 = inject_and_detect(3).unwrap_or(false);
    v.push(("J1-B族越界检出", b0 && b1, format!("B0={} B1={}", b0, b1)));
    // J1 C 族检出（回调两形态：缺席 + 重号）。
    let c0 = inject_and_detect(4).unwrap_or(false);
    let c1 = inject_and_detect(5).unwrap_or(false);
    v.push(("J1-C族回调检出", c0 && c1, format!("C0={} C1={}", c0, c1)));
    // J1 D 族检出（撕裂两形态：撕裂标志 + 代际错配）。
    let d0 = inject_and_detect(6).unwrap_or(false);
    let d1 = inject_and_detect(7).unwrap_or(false);
    v.push(("J1-D族撕裂检出", d0 && d1, format!("D0={} D1={}", d0, d1)));
    // J2 相位机单调守卫：推进/平移/倒退/收口四路。
    let mut g2 = Governor::new();
    let m1 = g2.advance(GovPhase::Sampling).is_ok();
    let m2 = g2.advance(GovPhase::Idle).is_err();
    let m3 = g2.advance(GovPhase::Sampling).is_err();
    let m4 = g2.advance(GovPhase::Deciding).is_ok();
    let m5 = g2.close_step().is_err(); // 非 Notifying 收口拒
    let _ = g2.advance(GovPhase::Notifying);
    let m6 = g2.close_step().is_ok();
    v.push((
        "J2-相位机单调守卫",
        m1 && m2 && m3 && m4 && m5 && m6,
        format!("进{} 退拒{} 平拒{} 收口败{} 成{}", m1, m2, m3, m5, m6),
    ));
    // J2 双跑一致。
    v.push(("J2-双跑一致", repro_check().is_ok(), String::from("golden_run 两跑同值")));
    // J3 写回摘要非空。
    let wb = writeback_summary();
    let (wp, wd) = match &wb {
        Ok(s) => (!s.is_empty(), s.clone()),
        Err(e) => (false, format!("err=0x{:x} {}", e, detail(*e))),
    };
    v.push(("J3-写回摘要", wp, wd));
    // J4 段位守卫。
    v.push(("J4-段位守卫", segment_guard(), String::from("0x72800 窗口")));
    v
}

pub fn run_unx1985_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F1985 governor 注入矩阵（第十六矩阵）");
    for (n, p, d) in extra_checks() {
        s.add_dyn(n, p, &d);
    }
    s
}
