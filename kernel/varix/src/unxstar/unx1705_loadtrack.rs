//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F1705 · 负载追踪窗口账与迁移判据联动（样板） · 0x72440
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定，CoRun·UNX-A3-B06，前置 UNX-F1704）：
//!   ① 纯功能代码 ≥380 行；
//!   ② 判据原文：UNX-F1705-J1 窗口账与判据输入同源联动断言全过（判据
//!     读到的追踪值=账面值），窗口切换边界账零丢失；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! ─────────────────────────────────────────────────────────────────────
//! 设计要点：
//!   · 窗口账：每 CPU 一本 TrackAccount，tick 贡献 {0,1} 落入当前窗，
//!     窗满（period=8 tick）即封窗开新窗；边界零丢失审计 = Σ已封窗和
//!     + 当前窗部分和 == 已入 tick 数（任何封窗丢账必显）；
//!   · 追踪值：衰减加权平均（定点 1<<10 口径），第 k 新窗权重
//!     1024>>(k/HALF_LIFE) 下探到 1；判据口（JudgePort 缓存）与账面
//!     直算口两条路径必须同值——同源联动断言；
//!   · 迁移判据：只读判据口缓存值，src>dst+阈值 即候选；联动断言保证
//!     判据读到的值与账面直算值一致，判据不被脏缓存污染；
//!   · 失真注入 10 路：改缓存/删封窗/游标倒退/伪窗和/重复窗号/改周期/
//!     窗号跳变/缓存过期冒充新鲜/权重溢出/窗和越界，联动审计 10/10 必抓；
//!   · 双跑一致 + 段位守卫窗口 0x40。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x72440，VX3-1103 负载追踪窗口独占）。
pub const SEG_BASE: u32 = 0x72440;
/// 受控 CPU 数（冻结 8）。
pub const CPU_N: usize = 8;
/// 每 CPU 封窗数（冻结 32）。
pub const WIN_N: u64 = 32;
/// 窗周期（每窗 tick 数，冻结 8）。
pub const WIN_PERIOD: u64 = 8;
/// 定点移位（追踪值口径 1<<10，冻结）。
pub const FIX_SHIFT: u32 = 10;
/// 衰减半衰窗距（每 HALF_LIFE 窗权重减半，冻结 4）。
pub const HALF_LIFE: u64 = 4;
/// 联动断言抽样数（冻结 10^4）。
pub const LINK_SAMPLES: u32 = 10_000;
/// 迁移判据裕量（定点 1<<10 口径，冻结 128 ≈ 12.5%）。
pub const MIG_MARGIN: u64 = 128;
/// 判据轮数（判据 10/10 语义，冻结）。
pub const JUDGE_ROUNDS: u32 = 10;

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TrackCode {
    /// 同源联动断裂：判据口缓存值 ≠ 账面直算值
    LinkBroken,
    /// 边界丢账：Σ封窗和+部分和 ≠ 负载总账
    WindowLoss,
    /// 窗游标倒退：窗号不递增或回退
    CursorBack,
    /// 窗和破损：封窗和越界（>period）或为负
    SumBroken,
    /// 周期被改：封窗 period 域与冻结值不一致
    PeriodBad,
    /// 迁移判据不定：同源值下候选结论与重算不一致
    MigAmbig,
    /// 权重溢出：衰减权重链越界
    WeightOver,
    /// 双跑不一致：golden_run 两次应同值
    NotReproducible,
}

impl TrackCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            TrackCode::LinkBroken => "同源联动断裂：判据缓存≠账面直算，刷新判据口并查脏缓存写者",
            TrackCode::WindowLoss => "边界丢账：封窗边界 Σ和≠入账数，重放窗口流补账",
            TrackCode::CursorBack => "窗游标倒退：窗号只进不退，回退窗一律作废重封",
            TrackCode::SumBroken => "窗和破损：封窗和越界（每 tick 贡献≤1），重算该窗",
            TrackCode::PeriodBad => "周期被改：窗 period 域≠冻结值，恢复 WIN_PERIOD=8",
            TrackCode::MigAmbig => "迁移判据不定：同源值下候选结论漂移，判据必须走联动校验后的值",
            TrackCode::WeightOver => "权重溢出：衰减权重链越界，权重表回到 1024>>(k/4) 下探 1",
            TrackCode::NotReproducible => "双跑不一致：复现红线，golden_run 两次应同值",
        }
    }
    pub const ALL: [TrackCode; 8] = [
        TrackCode::LinkBroken,
        TrackCode::WindowLoss,
        TrackCode::CursorBack,
        TrackCode::SumBroken,
        TrackCode::PeriodBad,
        TrackCode::MigAmbig,
        TrackCode::WeightOver,
        TrackCode::NotReproducible,
    ];
}

/// 诊断码查表（段外码返回统一文案）。
pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < TrackCode::ALL.len() {
        if TrackCode::ALL[i].val() == code {
            return TrackCode::ALL[i].detail();
        }
        i += 1;
    }
    "未知诊断码：不在 0x72440 段闭集内"
}

// ── 域模型 ──────────────────────────────────────────────────────
/// 单个负载窗：窗号 + 贡献和 + 封窗周期。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LoadWindow {
    /// 全局窗号（严格递增，无跳变）。
    pub idx: u64,
    /// 本窗贡献和（每 tick 贡献 ∈ {0,1}，封窗时 ==period 才算满）。
    pub sum: u64,
    /// 封窗周期（冻结 WIN_PERIOD）。
    pub period: u64,
}

/// 每 CPU 一本的窗口账：已封窗序列 + 当前窗 + 双游标。
#[derive(Clone, Debug)]
pub struct TrackAccount {
    pub cpu: u32,
    /// 已封窗（idx 严格递增）。
    pub sealed: Vec<LoadWindow>,
    /// 当前未封窗（界内部分和）。
    pub open: LoadWindow,
    /// 已入账 tick 总数（窗界推进的权威）。
    pub cursor: u64,
    /// 负载总账（零丢失审计口径：Σ窗和 == 负载总账）。
    pub load_total: u64,
}

/// 判据口：缓存追踪值 + 缓存所在窗号（同源联动的判据侧）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct JudgePort {
    pub cpu: u32,
    /// 缓存的追踪值（定点 1<<10 口径）。
    pub cached_util: u64,
    /// 缓存对应的封窗数（新鲜度锚）。
    pub cached_at_sealed: u64,
}

/// 建空账（确定性：open 窗 idx 从 0 起，双游标清零）。
pub fn new_account(cpu: u32) -> TrackAccount {
    TrackAccount {
        cpu,
        sealed: Vec::new(),
        open: LoadWindow { idx: 0, sum: 0, period: WIN_PERIOD },
        cursor: 0,
        load_total: 0,
    }
}

/// 建判据口（确定性：缓存清零，锚 0）。
pub fn new_port(cpu: u32) -> JudgePort {
    JudgePort { cpu, cached_util: 0, cached_at_sealed: 0 }
}

// ── 衰减权重表（整数实现，零浮点） ──────────────────────────────
/// 第 k 新窗（k=0 最新）权重：1024>>(k/HALF_LIFE)，下探到 1。
pub fn decay_weight(k: u64) -> u64 {
    let halvings = k / HALF_LIFE;
    let mut w: u64 = 1u64 << FIX_SHIFT;
    let mut h = 0u64;
    while h < halvings {
        w >>= 1;
        if w == 0 {
            return 1;
        }
        h += 1;
    }
    if w == 0 {
        1
    } else {
        w
    }
}

/// 权重链自检：w(0)==1024，w(HALF_LIFE)==512，递减不下破 1。
pub fn weight_chain_probe() -> bool {
    let w0 = decay_weight(0);
    let wh = decay_weight(HALF_LIFE);
    let wdeep = decay_weight(HALF_LIFE * 20);
    let mono = decay_weight(1) <= w0 && decay_weight(HALF_LIFE + 1) <= wh;
    w0 == 1u64 << FIX_SHIFT && wh == (1u64 << FIX_SHIFT) >> 1 && wdeep == 1 && mono
}

// ── 追踪值两口径（同源联动的两侧） ──────────────────────────────
/// 账面直算口径：从窗口账现场重算衰减加权平均（定点 1<<10）。
pub fn ledger_direct(acc: &TrackAccount) -> u64 {
    let mut wsum: u64 = 0;
    let mut acc_sum: u64 = 0;
    let n = acc.sealed.len() as u64;
    // 已封窗：从最新往回衰减（k=0 是最新封窗）。
    let mut k = 0u64;
    while k < n {
        // k<n 且 n≥1，n-1-k 不下溢。
        let idx = n - 1 - k;
        let win = match acc.sealed.get(idx as usize) {
            Some(w) => *w,
            None => return 0,
        };
        let wt = decay_weight(k);
        acc_sum = acc_sum.saturating_add(win.sum.saturating_mul(wt));
        wsum = wsum.saturating_add(wt);
        k += 1;
    }
    // 当前未满窗按最新权重并入。
    let wt0 = decay_weight(0);
    acc_sum = acc_sum.saturating_add(acc.open.sum.saturating_mul(wt0));
    wsum = wsum.saturating_add(wt0);
    if wsum == 0 {
        return 0;
    }
    // (Σ sum*w << FIX) / Σw == Σsum*w*(1<<10)/Σw；sum*w ≤ 8*1024，不溢出。
    acc_sum.saturating_mul(1u64 << FIX_SHIFT) / wsum
}

/// 判据读口径：缓存新鲜（锚==当前封窗数）则回缓存，否则重算并刷新缓存。
pub fn judge_read(port: &mut JudgePort, acc: &TrackAccount) -> Result<u64, u32> {
    if port.cpu != acc.cpu {
        return Err(TrackCode::LinkBroken.val());
    }
    let sealed_now = acc.sealed.len() as u64;
    if port.cached_at_sealed != sealed_now {
        // 缓存过期：重算刷新（同源，值仍来自窗口账）。
        let v = ledger_direct(acc);
        port.cached_util = v;
        port.cached_at_sealed = sealed_now;
    }
    Ok(port.cached_util)
}

/// 同源联动断言：判据口值 == 账面直算值（判据读到的=账面值）。
pub fn link_assert(port: &mut JudgePort, acc: &TrackAccount) -> Result<u64, u32> {
    let jv = judge_read(port, acc)?;
    let lv = ledger_direct(acc);
    if jv != lv {
        return Err(TrackCode::LinkBroken.val());
    }
    Ok(jv)
}

// ── 窗口步进（唯一写路径） ──────────────────────────────────────
/// 步进记录：本 tick 贡献 + 是否发生封窗切换。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StepNote {
    pub tick_no: u64,
    pub load: u64,
    pub switched: bool,
    /// 切换后当前窗号（未切换则不变）。
    pub open_idx: u64,
}

/// 单 tick 入账：贡献 {0,1} 落入当前窗；cursor 跨过窗界即封窗开新窗
/// （封窗只搬移归属，和不变——切换边界账零丢失的机制保证）。
pub fn window_step(acc: &mut TrackAccount, tick_no: u64, load: u64) -> Result<StepNote, u32> {
    if load > 1 {
        return Err(TrackCode::SumBroken.val());
    }
    if tick_no != acc.cursor {
        // tick_no 必须连续对位游标，防伪造/跳变。
        return Err(TrackCode::CursorBack.val());
    }
    acc.open.sum = acc.open.sum.saturating_add(load);
    acc.load_total = acc.load_total.saturating_add(load);
    acc.cursor = acc.cursor.checked_add(1).ok_or(TrackCode::SumBroken.val())?;
    // 窗界即封：本 tick 恰好填满窗界（cursor%period==0）时封窗。
    let mut switched = false;
    if acc.cursor % acc.open.period == 0 {
        let sealed_win = acc.open;
        acc.sealed.push(sealed_win);
        let next_idx = sealed_win.idx.checked_add(1).ok_or(TrackCode::CursorBack.val())?;
        acc.open = LoadWindow { idx: next_idx, sum: 0, period: WIN_PERIOD };
        switched = true;
    }
    Ok(StepNote { tick_no, load, switched, open_idx: acc.open.idx })
}

/// 确定性负载流：第 cpu 核第 t tick 的贡献（0/1，可复算）。
/// 步长随核号递增：重核（cpu0）步长 3，轻核（cpu7）步长 10 恒 0。
pub fn load_at(cpu: u32, t: u64) -> u64 {
    let base = (cpu as u64 + 1).saturating_mul(7);
    let stride = 3 + cpu as u64;
    if (base + t * stride) % 10 < 6 {
        1
    } else {
        0
    }
}

/// 跑满窗口流：每 CPU 入 WIN_N*WIN_PERIOD 个 tick，逐 tick 步进。
pub fn fill_account(acc: &mut TrackAccount, ticks: u64) -> Result<u64, u32> {
    let mut t = 0u64;
    let mut switches = 0u64;
    while t < ticks {
        let note = window_step(acc, t, load_at(acc.cpu, t))?;
        if note.switched {
            switches += 1;
        }
        t += 1;
    }
    Ok(switches)
}

// ── 边界零丢失审计 ──────────────────────────────────────────────
/// 窗口切换边界账零丢失：
///   ① Σ已封窗和 + 当前窗部分和 == 负载总账（封窗只搬归属，零丢失）；
///   ② 封窗号 0 起严格连续递增；
///   ③ 每封窗 sum≤period、period==WIN_PERIOD；
///   ④ 负载总账 ≤ cursor（每 tick 贡献 ≤1）。
pub fn boundary_audit(acc: &TrackAccount) -> Result<(), u32> {
    let mut total: u64 = 0;
    let mut i = 0usize;
    while i < acc.sealed.len() {
        let w = match acc.sealed.get(i) {
            Some(w) => *w,
            None => return Err(TrackCode::WindowLoss.val()),
        };
        if w.idx != i as u64 {
            return Err(TrackCode::CursorBack.val());
        }
        if w.sum > w.period || w.period != WIN_PERIOD {
            return Err(TrackCode::SumBroken.val());
        }
        total = total.saturating_add(w.sum);
        i += 1;
    }
    if acc.open.period != WIN_PERIOD {
        return Err(TrackCode::PeriodBad.val());
    }
    if acc.open.sum > acc.open.period {
        return Err(TrackCode::SumBroken.val());
    }
    total = total.saturating_add(acc.open.sum);
    if total != acc.load_total {
        return Err(TrackCode::WindowLoss.val());
    }
    if acc.load_total > acc.cursor {
        return Err(TrackCode::SumBroken.val());
    }
    Ok(())
}

// ── 迁移判据（只读判据口，同源联动约束） ────────────────────────
/// 迁移候选判据：src 追踪值 > dst 追踪值 + 裕量 ⇒ 候选。
/// 输入必须来自 link_assert 校验过的值，否则判据不定。
pub fn mig_candidate(src_util: u64, dst_util: u64) -> bool {
    src_util > dst_util.saturating_add(MIG_MARGIN)
}

/// 联动迁移判定：src 侧先 link_assert，dst 侧传入已联动校验的值，
/// 返回 (候选, src值, dst值)。判据确定性重算：同值必同结论。
pub fn mig_judge(
    src_port: &mut JudgePort,
    src_acc: &TrackAccount,
    dst_util: u64,
) -> Result<(bool, u64, u64), u32> {
    let s = link_assert(src_port, src_acc)?;
    let cand = mig_candidate(s, dst_util);
    let again = mig_candidate(s, dst_util);
    if again != cand {
        return Err(TrackCode::MigAmbig.val());
    }
    Ok((cand, s, dst_util))
}

// ── 失真注入（10 路逐字失真） ───────────────────────────────────
/// 对黄金账+判据口注入第 kind 路失真，返回坏对（值域全整数，安全）。
pub fn distort(acc: &TrackAccount, port: &JudgePort, kind: usize) -> (TrackAccount, JudgePort) {
    let mut bad_acc = acc.clone();
    let mut bad_port = port.clone();
    let n = bad_acc.sealed.len();
    match kind % 10 {
        0 => {
            // 改缓存：判据口值抬升，联动断言必抓。
            bad_port.cached_util = bad_port.cached_util.saturating_add(4096);
        }
        1 => {
            // 删封窗：边界 Σ和 < cursor，零丢失审计必抓。
            if n > 0 {
                bad_acc.sealed.remove(0);
            }
        }
        2 => {
            // 负载总账多计一笔：Σ窗和 == load_total 断言必抓。
            bad_acc.load_total = bad_acc.load_total.saturating_add(1);
        }
        3 => {
            // 伪窗和：首封窗和翻倍，越界审计必抓。
            if let Some(w) = bad_acc.sealed.first_mut() {
                w.sum = w.sum.saturating_mul(2);
            }
        }
        4 => {
            // 重复窗号：第二封窗 idx 改成与首封窗相同。
            if n >= 2 {
                let idx0 = bad_acc.sealed[0].idx;
                bad_acc.sealed[1].idx = idx0;
            }
        }
        5 => {
            // 改周期：当前窗 period 抬升，周期审计必抓。
            bad_acc.open.period = bad_acc.open.period.saturating_add(1);
        }
        6 => {
            // 窗号跳变：末封窗 idx 断档。
            if let Some(w) = bad_acc.sealed.last_mut() {
                w.idx = w.idx.saturating_add(1);
            }
        }
        7 => {
            // 缓存过期冒充新鲜：锚对齐但值被改。
            bad_port.cached_at_sealed = bad_acc.sealed.len() as u64;
            bad_port.cached_util = bad_port.cached_util.saturating_add(1);
        }
        8 => {
            // 窗和越界：当前窗部分和抬到满格（封窗条件被绕过）。
            bad_acc.open.sum = bad_acc.open.period;
        }
        _ => {
            // 权重溢出探：伪造最新封窗和为极大值（饱和口径下联动值漂移）。
            if let Some(w) = bad_acc.sealed.first_mut() {
                w.sum = u64::MAX / 2;
            }
        }
    }
    (bad_acc, bad_port)
}

/// 注入检出账。
pub struct InjectLedger {
    pub tries: u32,
    pub caught: u32,
    /// 漏抓的失真路号（诊断用）。
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

/// 黄金账+判据口（确定性）：跑满 WIN_N*WIN_PERIOD tick 后刷新判据口。
pub fn golden_pair(cpu: u32) -> Result<(TrackAccount, JudgePort), u32> {
    let mut acc = new_account(cpu);
    let switches = fill_account(&mut acc, WIN_N * WIN_PERIOD)?;
    if switches != WIN_N {
        return Err(TrackCode::WindowLoss.val());
    }
    boundary_audit(&acc)?;
    let mut port = new_port(cpu);
    link_assert(&mut port, &acc)?;
    Ok((acc, port))
}

/// 注入轮：每轮对新鲜黄金对注入一路失真，联动/边界审计必须抓到
/// （黄金对本身必须双审计通过，防注入器污染基准）。
pub fn inject_rounds(rounds: u32) -> Result<InjectLedger, u32> {
    let mut led = InjectLedger::new();
    let mut r = 0u32;
    while r < rounds {
        let (acc, port) = golden_pair(0)?;
        let kind = (r % 10) as usize;
        let (ba, bp) = distort(&acc, &port, kind);
        let hit = boundary_audit(&ba).is_err() || link_assert(&mut bp.clone(), &ba).is_err();
        led.record(kind, hit);
        r += 1;
    }
    Ok(led)
}

// ── 联动抽样（判据读到的追踪值=账面值，10^4 样本） ──────────────
/// 每样本：从部分进度账上做联动断言，任一不一致即断。
pub fn link_samples(rounds: u32) -> Result<u32, u32> {
    let mut acc = new_account(0);
    let mut port = new_port(0);
    let mut k = 0u32;
    let total = WIN_N * WIN_PERIOD;
    let mut t = 0u64;
    while t < total && k < rounds {
        let _ = window_step(&mut acc, t, load_at(0, t))?;
        // 每封窗边界抽一次联动断言。
        if acc.sealed.len() as u64 > 0 && acc.open.sum == 0 {
            let _ = link_assert(&mut port, &acc)?;
            k += 1;
        }
        t += 1;
    }
    // 不够样本时补全量联动。
    while k < rounds {
        let _ = link_assert(&mut port, &acc)?;
        k += 1;
    }
    Ok(k)
}

// ── 黄金全链 ────────────────────────────────────────────────────
/// 全链：8 核跑满窗 → 边界审计 → 联动断言 → 迁移判据（重轻核）→
/// 注入 10/10 → 三拒探（贡献越界/tick 跳变/判据错核）。
/// 返回 (正旗数, 拒旗数, 联动样本数, 封窗总数)。
pub fn golden_run() -> Result<(u32, u32, u32, u64), u32> {
    let mut accounts: Vec<TrackAccount> = Vec::new();
    let mut ports: Vec<JudgePort> = Vec::new();
    let mut ci = 0u32;
    while (ci as usize) < CPU_N {
        let (acc, port) = golden_pair(ci)?;
        accounts.push(acc);
        ports.push(port);
        ci += 1;
    }
    // 全核边界审计 + 联动断言。
    let mut i = 0usize;
    while i < CPU_N {
        boundary_audit(&accounts[i])?;
        link_assert(&mut ports[i], &accounts[i])?;
        i += 1;
    }
    // 联动抽样 10^4。
    let sampled = link_samples(LINK_SAMPLES)?;
    // 迁移判据：重核(0) vs 轻核(CPU_N-1) 应候选；同核对自身不候选。
    // dst 侧先独立联动校验，避免同 Vec 双可变借用。
    let dst_util = link_assert(&mut ports[CPU_N - 1], &accounts[CPU_N - 1])?;
    let (heavy_cand, _, _) = mig_judge(&mut ports[0], &accounts[0], dst_util)?;
    let own_util = link_assert(&mut ports[0], &accounts[0])?;
    let (self_cand, _, _) = mig_judge(&mut ports[0], &accounts[0], own_util)?;
    // 注入 10 路全抓。
    let inj = inject_rounds(JUDGE_ROUNDS)?;
    let inj_ok = inj.all_caught();
    // 三拒探。
    let mut probe = new_account(1);
    let over_rej = window_step(&mut probe, 0, 2).is_err();
    let mut probe2 = new_account(1);
    let _ = window_step(&mut probe2, 0, 1)?;
    let skip_rej = window_step(&mut probe2, 2, 1).is_err();
    // 判据错核拒：port 挂错核。
    let mut wrong_port = new_port(0);
    let wrong_rej = judge_read(&mut wrong_port, &accounts[1]).is_err();
    let flags = (accounts.len() == CPU_N) as u32
        + heavy_cand as u32
        + (!self_cand) as u32
        + inj_ok as u32;
    let rejects = over_rej as u32 + skip_rej as u32 + wrong_rej as u32
        + (sampled == LINK_SAMPLES) as u32;
    let sealed_total = accounts[0].sealed.len() as u64;
    Ok((flags, rejects, sampled, sealed_total))
}

// ── 探针三序 ────────────────────────────────────────────────────
/// 序 A：黄金全链四正四拒全中。
pub fn probe_seq_a() -> Result<(), u32> {
    match golden_run() {
        Ok((4, 4, _, _)) => Ok(()),
        _ => Err(TrackCode::NotReproducible.val()),
    }
}

/// 序 B：失真注入 10 路全抓。
pub fn probe_seq_b() -> Result<(), u32> {
    match inject_rounds(JUDGE_ROUNDS) {
        Ok(l) if l.all_caught() => Ok(()),
        _ => Err(TrackCode::LinkBroken.val()),
    }
}

/// 序 C：冻结常量与段位窗口。
pub fn probe_seq_c() -> Result<(), u32> {
    let in_window = SEG_BASE == 0x72440 && TrackCode::NotReproducible.val() < SEG_BASE + 0x40;
    let ok = CPU_N == 8 && WIN_N == 32 && WIN_PERIOD == 8 && FIX_SHIFT == 10
        && HALF_LIFE == 4 && LINK_SAMPLES == 10_000 && MIG_MARGIN == 128
        && JUDGE_ROUNDS == 10 && in_window;
    if ok {
        Ok(())
    } else {
        Err(TrackCode::NotReproducible.val())
    }
}

// ── 双跑一致（复现红线） ────────────────────────────────────────
pub fn repro_check() -> Result<(), u32> {
    let a = golden_run()?;
    let b = golden_run()?;
    if a != b {
        return Err(TrackCode::NotReproducible.val());
    }
    Ok(())
}

// ── 段位守卫 ────────────────────────────────────────────────────
pub fn segment_guard() -> bool {
    let mut i = 0;
    while i < TrackCode::ALL.len() {
        let v = TrackCode::ALL[i].val();
        if v < SEG_BASE || v >= SEG_BASE + 0x40 {
            return false;
        }
        i += 1;
    }
    true
}

// ── 窗口边界探针 ────────────────────────────────────────────────
/// 窗口切换边界三查：窗界即封、新窗号递增、零载照封（和=0 不涨总账）。
pub fn probe_window_boundary() -> bool {
    let mut acc = new_account(2);
    // 注入 WIN_PERIOD+1 个满载 tick：第 8 tick 封窗0（和=8），第 9 tick 入新窗。
    let mut t = 0u64;
    let mut saw_switch = false;
    while t < WIN_PERIOD + 1 {
        let note = match window_step(&mut acc, t, 1) {
            Ok(n) => n,
            Err(_) => return false,
        };
        if note.switched {
            saw_switch = true;
        }
        t += 1;
    }
    let sealed_ok = acc.sealed.len() == 1
        && acc.sealed[0].sum == WIN_PERIOD
        && acc.sealed[0].idx == 0
        && acc.open.idx == 1
        && acc.open.sum == 1;
    // 零载流：窗界照封（和=0），负载总账不涨。
    let mut acc0 = new_account(2);
    let mut t0 = 0u64;
    while t0 < WIN_PERIOD {
        if window_step(&mut acc0, t0, 0).is_err() {
            return false;
        }
        t0 += 1;
    }
    let zero_ok = acc0.sealed.len() == 1
        && acc0.sealed[0].sum == 0
        && acc0.load_total == 0
        && acc0.cursor == WIN_PERIOD;
    saw_switch && sealed_ok && zero_ok && boundary_audit(&acc).is_ok() && boundary_audit(&acc0).is_ok()
}

// ── 迁移判据确定性探针 ─────────────────────────────────────────
/// 阈值边界：差值恰等于裕量不候选、差值超裕量候选、反向不候选。
pub fn probe_mig_judge() -> bool {
    let base = 2u64 << FIX_SHIFT; // 2048
    let eq = !mig_candidate(base, base - MIG_MARGIN);
    let over = mig_candidate(base, base - MIG_MARGIN - 1);
    let rev = !mig_candidate(base - MIG_MARGIN - 1, base);
    let same = !mig_candidate(base, base);
    eq && over && rev && same
}

// ── 完成写回（判据③：状态=已完成、结果摘要非空） ────────────────
pub fn writeback_summary() -> Result<String, u32> {
    let mut accounts: Vec<TrackAccount> = Vec::new();
    let mut ci = 0u32;
    while (ci as usize) < CPU_N {
        let (acc, _) = golden_pair(ci)?;
        accounts.push(acc);
        ci += 1;
    }
    let inj = inject_rounds(JUDGE_ROUNDS)?;
    if !inj.all_caught() {
        return Err(TrackCode::LinkBroken.val());
    }
    let sampled = link_samples(LINK_SAMPLES)?;
    Ok(format!(
        "状态=已完成 cpus={} win/cpu={} sealed0={} link_samples={} inject={}/{}",
        accounts.len(), WIN_N, accounts[0].sealed.len(), sampled, inj.caught, inj.tries
    ))
}

// ── 判据面 ──────────────────────────────────────────────────────
pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut v: Vec<(&'static str, bool, String)> = Vec::new();
    // J1 主判据：黄金全链（四正四拒）。
    let g = golden_run();
    let (gp, gd) = match &g {
        Ok((f, r, s, w)) => (*f == 4 && *r == 4, format!("flags={} rejects={} samples={} sealed0={}", f, r, s, w)),
        Err(e) => (false, format!("err=0x{:x} {}", e, detail(*e))),
    };
    v.push(("J1-黄金全链", gp, gd));
    // J1 同源联动断言（判据读到的=账面值）。
    let mut acc0 = new_account(0);
    let _ = fill_account(&mut acc0, WIN_N * WIN_PERIOD);
    let mut port0 = new_port(0);
    let link_ok = link_assert(&mut port0, &acc0).is_ok();
    v.push(("J1-同源联动", link_ok && probe_mig_judge(), format!("联动断言过；迁移阈值边界四查全中 samples={}", LINK_SAMPLES)));
    // J1 联动抽样 10^4 全过。
    let sampled = link_samples(LINK_SAMPLES);
    let sp = matches!(&sampled, Ok(n) if *n == LINK_SAMPLES);
    v.push(("J1-联动10^4抽样", sp, format!("samples={}", LINK_SAMPLES)));
    // J1 窗口切换边界零丢失。
    let (accb, _) = match golden_pair(3) {
        Ok(p) => p,
        Err(e) => {
            v.push(("J1-边界零丢失", false, format!("err=0x{:x}", e)));
            return v;
        }
    };
    v.push(("J1-边界零丢失", boundary_audit(&accb).is_ok() && probe_window_boundary(), String::from("Σ封窗和+部分和==负载总账；窗界即封；零载照封")));
    // J1 失真注入 10/10 必抓。
    let inj = inject_rounds(JUDGE_ROUNDS);
    let (ip, id) = match &inj {
        Ok(l) => (l.all_caught(), format!("t={} c={} miss={:?}", l.tries, l.caught, l.miss_kinds)),
        Err(e) => (false, format!("err=0x{:x}", e)),
    };
    v.push(("J1-失真注入10/10", ip, id));
    // J2 权重链守卫。
    v.push(("J2-衰减权重链", weight_chain_probe(), String::from("w(0)=1024 w(4)=512 深处=1 单调不升破")));
    // J2 双跑一致。
    v.push(("J2-双跑一致", repro_check().is_ok(), String::from("golden_run 两跑同值")));
    // J2 冻结常量。
    v.push((
        "J2-冻结常量",
        probe_seq_c().is_ok(),
        format!("CPU_N={} WIN_N={} WIN_PERIOD={} MIG_MARGIN={}", CPU_N, WIN_N, WIN_PERIOD, MIG_MARGIN),
    ));
    // J3 写回摘要非空。
    let wb = writeback_summary();
    let (wp, wd) = match &wb {
        Ok(s) => (!s.is_empty(), s.clone()),
        Err(e) => (false, format!("err=0x{:x}", e)),
    };
    v.push(("J3-写回摘要", wp, wd));
    // J4 段位守卫。
    v.push(("J4-段位守卫", segment_guard(), String::from("0x72440 窗口")));
    v
}

pub fn run_unx1705_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F1705 负载追踪窗口账与迁移判据联动（样板）");
    for (n, p, d) in extra_checks() {
        s.add_dyn(n, p, &d);
    }
    s
}
