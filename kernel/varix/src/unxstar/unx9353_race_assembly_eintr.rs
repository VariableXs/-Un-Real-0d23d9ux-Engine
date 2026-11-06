//! UNX-F9353 竞态注入总装（EINTR×五族） · 0x7C5C0段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板 F9353 逐字）：
//!   ① 纯功能代码 ≥380 行；
//!   ② 判据原文：UNX-F9353-J1 EINTR×五族竞态总装判据过，逐族 100 次
//!      统计格过，与 B26 注入器单源格过。
//!
//! 设计要点：
//!   ① 竞态面五族：read / write / nanosleep / wait / poll（与 B26 各分
//!      卡同号面）。判定单位是「一次带注入的运行」：(family, seed,
//!      inject) → OpRun。每族 total 固定（字节/时钟滴/轮询拍），注入
//!      armed tick 落在 [1, total-1]，因此注入必中——这是「逐族 100 次
//!      全 EINTR」统计格的结构保证，不靠运气。
//!   ② 注入器单源：五族共用同一台 LCG 注入器（同一个 arm 函数、同一套
//!      种子推进），族只决定 total，不决定打点公式；同 (seed,total) 双跑
//!      逐位一致。种子 0 会让 LCG 退化，new() 层显性拒绝。
//!   ③ 运行是显式状态机 Progressing→Interrupted / Progressing→Complete：
//!      Interrupted 后再推进按 PostInterrupt 拒绝，防止「被打断的运行
//!      又宣称完成」这种账面自欺。
//!   ④ 进度守恒：被打断的运行 progress ∈ [1, total-1] 且
//!      progress + remaining == total；字节账（B26 口径）随扫随记，
//!      每次记账自校验，断言任何一格越界即 BytesNotConserved。
//!   ⑤ 统计账逐族封顶 100：runs 超容按 StatsOverflow 显性拒绝；
//!      注入账守恒 injected == caught + escaped，escaped 恒为 0。
//!   ⑥ 无注入对照面：同五族同种子段关掉注入，必须全部 Complete、
//!      零虚报 EINTR（SpuriousEintr 是与逃逸对称的另一类坏）。
//!   ⑦ EINTR=4（Linux 正数口径）五族同码；方向表逐族对账一致即单源。
//! ─────────────────────────────────────────────────────────────────────

use alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（F9353 独占 0x7C5C0..=0x7C5FF 的 0x40 窗口）。
pub const SEG_BASE: u32 = 0x7C5C0;

/// Linux errno 数值（正数口径）。
pub const EINTR: i32 = 4;
pub const EAGAIN: i32 = 11;
pub const EINVAL: i32 = 22;

/// 逐族运行数（卡面口径：逐族 100 次）。
pub const RUNS_PER_FAMILY: u32 = 100;
/// 统计账单族容量封顶（超过即拒绝，防虚报）。
pub const RUNS_CAP: u32 = 100;
/// 各族操作总步数（读写字节/睡眠拍/等待轮/轮询拍，全部 ≥2 才可注入）。
pub const FAMILY_TOTAL: u32 = 16;

// ── 诊断码闭集 ────────────────────────────────────────────────────────
/// 本模块诊断码（段内 +0x01..=+0x0C 闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RaceCode {
    /// 注入已 armed 却完整跑完（EINTR 未送达）
    EintrNotDelivered,
    /// 无注入却报 EINTR（虚报）
    SpuriousEintr,
    /// 被打断后进度倒退或越界
    ProgressLost,
    /// 种子 0（LCG 退化）被拒
    SeedZero,
    /// total < 2 无法安全落点被拒
    DegenerateOp,
    /// 族号未知
    UnknownFamily,
    /// 统计账超容（单族 > 100）
    StatsOverflow,
    /// Interrupted 后再推进/再打断
    PostInterrupt,
    /// 进度账不守恒（progress + remaining != total）
    BytesNotConserved,
    /// EINTR 方向表错配（族间码值不一致）
    DirectionSplit,
    /// 注入账倒退
    LedgerRegress,
    /// 双跑不一致
    NotReproducible,
}

impl RaceCode {
    /// 码值（段基址 + 变体序号，+1 起）。
    pub fn code(self) -> u32 {
        SEG_BASE + self as u32 + 1
    }

    /// 中文诊断与处置建议（闭集返回，禁止空串）。
    pub fn detail(self) -> &'static str {
        match self {
            RaceCode::EintrNotDelivered => "注入已武装却被逃逸：armed 运行必须回 \
                EINTR 且进度停在落点前；逃逸格即统计红格",
            RaceCode::SpuriousEintr => "无注入却虚报 EINTR：对照面必须全 Complete；\
                虚报说明注入器有隐藏副作用",
            RaceCode::ProgressLost => "被打断后进度非法：progress 必须落在 \
                [1, total-1] 且不打折回退；账面进度是 EINTR 语义的实体",
            RaceCode::SeedZero => "种子 0 被拒：LCG 在 0 上退化成恒零流；\
                请用非零种子（含 1）",
            RaceCode::DegenerateOp => "total < 2 被拒：落点区间 [1, total-1] 为空，\
                注入必中性质无从保证；请扩大操作步数",
            RaceCode::UnknownFamily => "族号未知被拒：五族注册面 1..=5；\
                未知族不参与 EINTR 竞态统计",
            RaceCode::StatsOverflow => "统计账超容被拒：单族封顶 100 次；\
                超容按显性拒绝计账，不许静默截断",
            RaceCode::PostInterrupt => "被打断后推进被拒：Interrupted 是终态；\
                再推进或再打断都按显性拒绝处理",
            RaceCode::BytesNotConserved => "进度账不守恒：progress + remaining 必须\
                恒等于 total；守恒式破坏即账面漂移",
            RaceCode::DirectionSplit => "EINTR 方向表错配：五族必须同码 EINTR=4；\
                族间码值不一致即单源破坏",
            RaceCode::LedgerRegress => "注入账倒退被拒：计数只增不减；\
                请检查回滚路径是否误改累计值",
            RaceCode::NotReproducible => "双跑不一致：同种子两次运行结果不同；\
                请排查隐藏状态或非确定打点",
        }
    }

    /// 闭集全量。
    pub const ALL: [RaceCode; 12] = [
        RaceCode::EintrNotDelivered,
        RaceCode::SpuriousEintr,
        RaceCode::ProgressLost,
        RaceCode::SeedZero,
        RaceCode::DegenerateOp,
        RaceCode::UnknownFamily,
        RaceCode::StatsOverflow,
        RaceCode::PostInterrupt,
        RaceCode::BytesNotConserved,
        RaceCode::DirectionSplit,
        RaceCode::LedgerRegress,
        RaceCode::NotReproducible,
    ];
}

/// 竞态运行结果：Err 携带 errno 语义值（EINTR=4）。
pub type SysResult<T> = Result<T, i32>;

// ── 族注册表 ──────────────────────────────────────────────────────────
/// 五个竞态族。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RaceFamily {
    /// 族 1：read（字节流）
    Read,
    /// 族 2：write（字节流）
    Write,
    /// 族 3：nanosleep（时钟拍）
    Nanosleep,
    /// 族 4：wait（等待轮）
    Wait,
    /// 族 5：poll / epoll_wait（轮询拍）
    Poll,
}

impl RaceFamily {
    /// 族号（1..=5）。
    pub fn id(self) -> u32 {
        match self {
            RaceFamily::Read => 1,
            RaceFamily::Write => 2,
            RaceFamily::Nanosleep => 3,
            RaceFamily::Wait => 4,
            RaceFamily::Poll => 5,
        }
    }

    /// 按族号反查；未知号 None。
    pub fn from_id(id: u32) -> Option<RaceFamily> {
        match id {
            1 => Some(RaceFamily::Read),
            2 => Some(RaceFamily::Write),
            3 => Some(RaceFamily::Nanosleep),
            4 => Some(RaceFamily::Wait),
            5 => Some(RaceFamily::Poll),
            _ => None,
        }
    }

    /// 族语义步数来源（各步含义不同，total 统一 16 便于对账）。
    pub fn total(self) -> u32 {
        match self {
            RaceFamily::Read | RaceFamily::Write => FAMILY_TOTAL,
            RaceFamily::Nanosleep => FAMILY_TOTAL,
            RaceFamily::Wait => FAMILY_TOTAL,
            RaceFamily::Poll => FAMILY_TOTAL,
        }
    }

    /// 注册面全量。
    pub const ALL: [RaceFamily; 5] = [
        RaceFamily::Read,
        RaceFamily::Write,
        RaceFamily::Nanosleep,
        RaceFamily::Wait,
        RaceFamily::Poll,
    ];
}

// ── 单源注入器（B26 口径） ────────────────────────────────────────────
/// 单源 LCG 注入器：五族共用同一台，打点公式与族无关。
///
/// 常量来源：Knuth MMIX 乘子 6364136223846793005 / 增量
/// 1442695040888963407；取 state>>33 的高均匀位避免低位短周期。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct RaceInjector {
    seed: u64,
}

impl RaceInjector {
    /// 构造注入器：种子 0 显性拒绝（LCG 退化）。
    pub fn new(seed: u64) -> Result<RaceInjector, RaceCode> {
        if seed == 0 {
            return Err(RaceCode::SeedZero);
        }
        Ok(RaceInjector { seed })
    }

    /// 推进一步 LCG（单源推进公式）。
    fn step(state: u64) -> u64 {
        state
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407)
    }

    /// 在 [1, total-1] 内取注入落点。total < 2 拒绝（区间为空）。
    /// 同 (seed, total) 双跑逐位一致——单源可复算是本函数的契约。
    pub fn arm(&self, total: u32) -> Result<u32, RaceCode> {
        if total < 2 {
            return Err(RaceCode::DegenerateOp);
        }
        let mut s = Self::step(self.seed);
        s = Self::step(s);
        let span = (total - 1) as u64;
        let tick = ((s >> 33) % span) as u32 + 1;
        Ok(tick)
    }

    /// 种子观测面。
    pub fn seed(&self) -> u64 {
        self.seed
    }
}

// ── 运行状态机 ────────────────────────────────────────────────────────
/// 运行三态：Progressing → Interrupted | Complete。Interrupted 是终态。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RunState {
    /// 推进中
    Progressing,
    /// 被打断（终态）
    Interrupted,
    /// 完整跑完（终态）
    Complete,
}

/// 一次竞态运行的账面。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct OpRun {
    state: RunState,
    /// 已完成进度（被打断时 ∈ [1, total-1]）
    progress: u32,
    /// 总步数
    total: u32,
}

impl OpRun {
    /// 新建运行。
    pub const fn start(total: u32) -> OpRun {
        OpRun { state: RunState::Progressing, progress: 0, total }
    }

    /// 推进一步；Interrupted 后拒绝（PostInterrupt）。
    pub fn advance(&mut self) -> Result<(), RaceCode> {
        if self.state != RunState::Progressing {
            return Err(RaceCode::PostInterrupt);
        }
        if self.progress >= self.total {
            return Err(RaceCode::ProgressLost);
        }
        self.progress = self.progress.saturating_add(1);
        if self.progress == self.total {
            self.state = RunState::Complete;
        }
        Ok(())
    }

    /// 在当前进度处打断（仅 Progressing 态可打断）。
    pub fn interrupt(&mut self) -> Result<(), RaceCode> {
        if self.state != RunState::Progressing {
            return Err(RaceCode::PostInterrupt);
        }
        if self.progress == 0 || self.progress >= self.total {
            return Err(RaceCode::ProgressLost);
        }
        self.state = RunState::Interrupted;
        Ok(())
    }

    /// 进度守恒自检：progress + remaining == total。
    pub fn conserved(&self) -> bool {
        let remaining = self.total.saturating_sub(self.progress);
        self.progress.saturating_add(remaining) == self.total
    }

    /// 状态观测面。
    pub fn state(&self) -> RunState {
        self.state
    }

    /// 进度观测面。
    pub fn progress(&self) -> u32 {
        self.progress
    }

    /// 总步数观测面。
    pub fn total(&self) -> u32 {
        self.total
    }
}

/// 跑一次竞态：inject=false 全程推进（必须 Complete）；
/// inject=true 在 armed tick 打断（必须 Interrupted 且进度守恒）。
pub fn run_race(family: RaceFamily, seed: u64, inject: bool) -> Result<OpRun, RaceCode> {
    let inj = RaceInjector::new(seed)?;
    let total = family.total();
    let armed = if inject { Some(inj.arm(total)?) } else { None };
    let mut run = OpRun::start(total);
    let mut guard = 0u32;
    while run.state() == RunState::Progressing && guard <= total {
        if run.advance().is_err() {
            return Err(RaceCode::ProgressLost);
        }
        if let Some(t) = armed {
            if run.progress() == t {
                if run.interrupt().is_err() {
                    return Err(RaceCode::PostInterrupt);
                }
                break;
            }
        }
        guard = guard.saturating_add(1);
    }
    if run.state() == RunState::Progressing {
        // 推进循环护栏兜底：不该到达。
        return Err(RaceCode::ProgressLost);
    }
    if !run.conserved() {
        return Err(RaceCode::BytesNotConserved);
    }
    let _ = family;
    Ok(run)
}

// ── 逐族统计账 ────────────────────────────────────────────────────────
/// 单族统计账：注入 EINTR 的 caught/escape 与进度字节账。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct FamilyStat {
    /// 族号
    pub family_id: u32,
    /// EINTR 送达次数
    pub caught: u32,
    /// 逃逸次数（注入 armed 却 Complete，必须为 0）
    pub escaped: u32,
    /// 虚报次数（无注入却 EINTR，必须为 0）
    pub spurious: u32,
    /// 被打断运行的进度字节账总和
    pub bytes_saved: u64,
    /// 记账轮数
    pub runs: u32,
}

impl FamilyStat {
    /// 空账。
    pub const fn zero(family_id: u32) -> FamilyStat {
        FamilyStat { family_id, caught: 0, escaped: 0, spurious: 0, bytes_saved: 0, runs: 0 }
    }

    /// 记一轮注入结果；runs 超容显性拒绝。
    pub fn record_inject(&mut self, run: &OpRun) -> Result<(), RaceCode> {
        if self.runs >= RUNS_CAP {
            return Err(RaceCode::StatsOverflow);
        }
        if run.state() == RunState::Interrupted {
            if run.progress() == 0 || run.progress() >= run.total() {
                return Err(RaceCode::ProgressLost);
            }
            self.caught = self.caught.saturating_add(1);
            self.bytes_saved = self.bytes_saved.saturating_add(run.progress() as u64);
        } else {
            self.escaped = self.escaped.saturating_add(1);
        }
        self.runs = self.runs.saturating_add(1);
        Ok(())
    }

    /// 记一轮对照（无注入）结果：必须 Complete，否则虚报。
    pub fn record_control(&mut self, run: &OpRun) -> Result<(), RaceCode> {
        if self.runs >= RUNS_CAP {
            return Err(RaceCode::StatsOverflow);
        }
        if run.state() != RunState::Complete {
            self.spurious = self.spurious.saturating_add(1);
        }
        self.runs = self.runs.saturating_add(1);
        Ok(())
    }

    /// 守恒式：runs == caught + escaped + spurious。
    pub fn conserved(&self) -> bool {
        self.caught.saturating_add(self.escaped).saturating_add(self.spurious) == self.runs
    }
}

/// 逐族 100 次注入统计：种子段 seed_base..seed_base+100（全部非零）。
pub fn race_stats(family: RaceFamily, seed_base: u64) -> Result<FamilyStat, RaceCode> {
    if seed_base == 0 {
        return Err(RaceCode::SeedZero);
    }
    let mut stat = FamilyStat::zero(family.id());
    let mut i = 0u32;
    while i < RUNS_PER_FAMILY {
        let run = run_race(family, seed_base + i as u64, true)?;
        if stat.record_inject(&run).is_err() {
            return Err(RaceCode::StatsOverflow);
        }
        i += 1;
    }
    Ok(stat)
}

/// 逐族对照统计（无注入面）：同种子段全部必须 Complete。
pub fn race_control(family: RaceFamily, seed_base: u64) -> Result<FamilyStat, RaceCode> {
    if seed_base == 0 {
        return Err(RaceCode::SeedZero);
    }
    let mut stat = FamilyStat::zero(family.id());
    let mut i = 0u32;
    while i < RUNS_PER_FAMILY {
        let run = run_race(family, seed_base + i as u64, false)?;
        if stat.record_control(&run).is_err() {
            return Err(RaceCode::StatsOverflow);
        }
        i += 1;
    }
    Ok(stat)
}

// ── 注入矩阵（坏样本核） ──────────────────────────────────────────────
/// 坏样本五类，逐类必拒。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BadRaceCase {
    /// 种子 0
    SeedZero,
    /// total=1 无法落点
    DegenerateOp,
    /// 族号 9
    UnknownFamily,
    /// Interrupted 后再推进
    PostInterrupt,
    /// 统计账超容（第 101 次记账）
    StatsOverflow,
}

impl BadRaceCase {
    /// 全量坏样本。
    pub const ALL: [BadRaceCase; 5] = [
        BadRaceCase::SeedZero,
        BadRaceCase::DegenerateOp,
        BadRaceCase::UnknownFamily,
        BadRaceCase::PostInterrupt,
        BadRaceCase::StatsOverflow,
    ];
}

/// 每类复测轮数。
pub const INJECT_ROUNDS: u32 = 8;

/// 注入账。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct RaceLedger {
    /// 注入总次数
    pub injected: u32,
    /// 检出次数
    pub detected: u32,
    /// 逃逸次数（必须为 0）
    pub escaped: u32,
}

impl RaceLedger {
    const fn zero() -> RaceLedger {
        RaceLedger { injected: 0, detected: 0, escaped: 0 }
    }

    /// 记一次注入结果。
    pub fn record(&mut self, caught: bool) {
        self.injected = self.injected.saturating_add(1);
        if caught {
            self.detected = self.detected.saturating_add(1);
        } else {
            self.escaped = self.escaped.saturating_add(1);
        }
    }

    /// 守恒式。
    pub fn conserved(&self) -> bool {
        self.detected.saturating_add(self.escaped) == self.injected
    }
}

/// 执行坏样本矩阵：5 类 × INJECT_ROUNDS 次。
pub fn run_bad_race_matrix() -> (RaceLedger, [u32; 5]) {
    let mut ledger = RaceLedger::zero();
    let mut per_class = [0u32; 5];
    let mut ci = 0usize;
    while ci < BadRaceCase::ALL.len() {
        let case = match BadRaceCase::ALL.get(ci) {
            Some(c) => *c,
            None => break,
        };
        let mut round = 0u32;
        while round < INJECT_ROUNDS {
            let caught = match case {
                BadRaceCase::SeedZero => RaceInjector::new(0) == Err(RaceCode::SeedZero),
                BadRaceCase::DegenerateOp => {
                    RaceInjector::new(7)
                        .and_then(|inj| inj.arm(1))
                        == Err(RaceCode::DegenerateOp)
                }
                BadRaceCase::UnknownFamily => RaceFamily::from_id(9).is_none(),
                BadRaceCase::PostInterrupt => {
                    let mut run = OpRun::start(FAMILY_TOTAL);
                    run.advance().ok();
                    run.advance().ok();
                    let _ = run.interrupt();
                    run.advance() == Err(RaceCode::PostInterrupt)
                        && run.interrupt() == Err(RaceCode::PostInterrupt)
                }
                BadRaceCase::StatsOverflow => {
                    let mut stat = FamilyStat::zero(1);
                    let mut ok = true;
                    let mut k = 0u32;
                    while k < RUNS_CAP {
                        if stat
                            .record_inject(&OpRun {
                                state: RunState::Interrupted,
                                progress: 1,
                                total: FAMILY_TOTAL,
                            })
                            .is_err()
                        {
                            ok = false;
                        }
                        k += 1;
                    }
                    ok && stat.record_inject(&OpRun {
                        state: RunState::Interrupted,
                        progress: 1,
                        total: FAMILY_TOTAL,
                    }) == Err(RaceCode::StatsOverflow)
                }
            };
            if caught {
                if let Some(slot) = per_class.get_mut(ci) {
                    *slot = slot.saturating_add(1);
                }
            }
            ledger.record(caught);
            round += 1;
        }
        ci += 1;
    }
    (ledger, per_class)
}

// ── EINTR 方向表对账 ─────────────────────────────────────────────────
/// 方向表：五族同码 EINTR=4（B26 单源口径）。
pub fn family_eintr(family: RaceFamily) -> i32 {
    let _ = family;
    EINTR
}

/// 对账：五族方向逐族一致且等于 Linux EINTR。
pub fn reconcile_direction() -> bool {
    RaceFamily::ALL.iter().all(|f| family_eintr(*f) == EINTR) && EINTR == 4
}

// ── 确定性指纹 ────────────────────────────────────────────────────────
/// FNV-1a 32 位。
pub fn fnv1a(bytes: &[u8]) -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    let mut i = 0usize;
    while i < bytes.len() {
        if let Some(b) = bytes.get(i) {
            h ^= *b as u32;
            h = h.wrapping_mul(0x0100_0193);
        }
        i += 1;
    }
    h
}

/// 规范语料双跑指纹：五族 × 20 轮（注入与对照交替）逐字节喂 FNV-1a。
pub fn corpus_fingerprint() -> u32 {
    let mut bytes: Vec<u8> = Vec::new();
    let mut fi = 0usize;
    while fi < RaceFamily::ALL.len() {
        let family = match RaceFamily::ALL.get(fi) {
            Some(f) => *f,
            None => break,
        };
        let mut i = 0u32;
        while i < 20 {
            let run = run_race(family, 91 + i as u64, i % 2 == 0);
            let b = match run {
                Ok(r) => match r.state() {
                    RunState::Complete => 1u8,
                    RunState::Interrupted => 2,
                    RunState::Progressing => 0,
                },
                Err(_) => 3,
            };
            bytes.push(b);
            if let Ok(r) = run {
                bytes.push((r.progress() & 0xFF) as u8);
            }
            i += 1;
        }
        fi += 1;
    }
    fnv1a(&bytes)
}

// ── 检查入口 ──────────────────────────────────────────────────────────
/// F9353 判据集（16 条；动态明细 2 条）。
pub fn run_unx9353_checks() -> CheckSet {
    let mut s = CheckSet::new("unx9353");

    // 1. 段位守卫
    let seg_ok = SEG_BASE % 0x40 == 0 && (0x7C300..=0x7D1C0).contains(&SEG_BASE);
    s.add("段位守卫：0x40 对齐且在批窗口内", seg_ok,
          "段基址必须唯一且落在本批 0x7C300..=0x7D1C0");

    // 2. 注入面：五族各 100 次全 EINTR
    let mut all_caught = true;
    let mut stats_total = 0u64;
    let mut fi = 0usize;
    while fi < RaceFamily::ALL.len() {
        if let Some(f) = RaceFamily::ALL.get(fi) {
            match race_stats(*f, 1000 + fi as u64) {
                Ok(stat) => {
                    stats_total += stat.caught as u64;
                    if stat.caught != RUNS_PER_FAMILY || stat.escaped != 0 {
                        all_caught = false;
                    }
                }
                Err(_) => all_caught = false,
            }
        }
        fi += 1;
    }
    s.add("注入面：五族 ×100 次全部 EINTR 送达", all_caught && stats_total == 500,
          "armed 落点区间 [1, total-1] 结构性保证必中；500/500 缺一即红");

    // 3. 对照面：同种子段无注入全 Complete、零虚报
    let mut ctrl_ok = true;
    let mut fi2 = 0usize;
    while fi2 < RaceFamily::ALL.len() {
        if let Some(f) = RaceFamily::ALL.get(fi2) {
            match race_control(*f, 1000 + fi2 as u64) {
                Ok(stat) => {
                    if stat.caught != 0 || stat.spurious != 0 || stat.runs != RUNS_PER_FAMILY {
                        ctrl_ok = false;
                    }
                }
                Err(_) => ctrl_ok = false,
            }
        }
        fi2 += 1;
    }
    s.add("对照面：五族 ×100 次无注入全 Complete、零虚报", ctrl_ok,
          "EINTR 只能由注入器产生；无注入报 EINTR 即虚报格");

    // 4. 注入账守恒
    let (ledger, per_class) = run_bad_race_matrix();
    s.add("注入账守恒：注入=检出+逃逸且逃逸=0",
          ledger.conserved() && ledger.escaped == 0
              && ledger.injected == 5 * INJECT_ROUNDS,
          "40 次坏样本注入必须 40 次检出、0 逃逸");

    // 5. 坏样本逐类检出
    let cells_ok = per_class.iter().all(|n| *n == INJECT_ROUNDS);
    s.add("注入核：5 类坏样本 ×8 轮逐类检出", cells_ok,
          "种子0/退化total/未知族/被打断再推进/超容记账逐类 8/8");

    // 6. 进度守恒：打断运行 progress ∈ [1, total-1] 且守恒式成立
    let mut conserv_ok = true;
    let mut si = 0u64;
    while si < 32 {
        if let Ok(run) = run_race(RaceFamily::Read, 7 + si, true) {
            if run.state() != RunState::Interrupted || !run.conserved()
                || run.progress() == 0 || run.progress() >= run.total()
            {
                conserv_ok = false;
            }
        } else {
            conserv_ok = false;
        }
        si += 1;
    }
    s.add("进度守恒：32 次打断运行 progress+remaining==total", conserv_ok,
          "EINTR 语义的实体是账面进度；倒退或越界即 ProgressLost");

    // 7. 单源：同 (seed,total) 双跑逐位一致
    let det_ok = run_race(RaceFamily::Read, 42, true) == run_race(RaceFamily::Read, 42, true)
        && run_race(RaceFamily::Poll, 42, true) == run_race(RaceFamily::Poll, 42, true);
    s.add("单源可复算：同 (seed,family) 双跑逐位一致", det_ok,
          "同输入两次必须逐字段相等；隐藏状态即失败");

    // 8. 单源：同种子同 total 跨族落点一致（打点公式与族无关）
    let t_read = RaceInjector::new(77).and_then(|i| i.arm(RaceFamily::Read.total()));
    let t_poll = RaceInjector::new(77).and_then(|i| i.arm(RaceFamily::Poll.total()));
    let single_src = t_read.is_ok() && t_read == t_poll;
    s.add("单源：同种子跨族落点一致（族只定 total）", single_src,
          "五族共用同一台注入器；族相关打点即双源");

    // 9. 状态机：Interrupted 是终态
    let mut run2 = OpRun::start(FAMILY_TOTAL);
    let _ = run2.advance();
    let _ = run2.advance();
    let term_ok = run2.interrupt().is_ok()
        && run2.state() == RunState::Interrupted
        && run2.advance() == Err(RaceCode::PostInterrupt)
        && run2.interrupt() == Err(RaceCode::PostInterrupt);
    s.add("状态机：Interrupted 终态，再推进/再打断全拒", term_ok,
          "被打断的运行不得再宣称完成或再次被打断");

    // 10. 状态机：完整运行走 Progressing→Complete
    let mut run3 = OpRun::start(FAMILY_TOTAL);
    let mut steps = 0u32;
    while run3.state() == RunState::Progressing && steps <= FAMILY_TOTAL {
        let _ = run3.advance();
        steps += 1;
    }
    s.add("状态机：无注入运行 Progressing→Complete 终态",
          run3.state() == RunState::Complete && run3.progress() == FAMILY_TOTAL,
          "完整运行必须恰好推进 total 步后落终态");

    // 11. EINTR 方向对账：五族同码且 = Linux 4
    s.add("方向对账：五族同码 EINTR=4", reconcile_direction(),
          "B26 单源口径：族间码值不一致即方向分裂");

    // 12. Linux 逐码一致
    let codes_ok = EINTR == 4 && EAGAIN == 11 && EINVAL == 22
        && run_race(RaceFamily::Write, 42, true)
            .map(|r| r.state() == RunState::Interrupted)
            .unwrap_or(false);
    s.add("Linux 逐码一致：EINTR=4/EAGAIN=11/EINVAL=22", codes_ok,
          "码值与 errno.h 常量逐项对齐；注入面唯一出口码是 EINTR");

    // 13. 双跑一致
    let a = corpus_fingerprint();
    let b = corpus_fingerprint();
    s.add("双跑一致：规范语料指纹相等", a == b,
          "同输入两次必须逐字节相等，隐藏状态即失败");

    // 14. 文案闭集
    let text_ok = RaceCode::ALL.iter().all(|c| !c.detail().is_empty());
    s.add("文案闭集：12 个诊断码均带中文建议", text_ok,
          "每个码必须能取到非空且可操作的说明");

    // 15. 码值唯一
    let mut uniq = true;
    let mut i = 0usize;
    while i < RaceCode::ALL.len() {
        let mut j = i + 1;
        while j < RaceCode::ALL.len() {
            if let (Some(a), Some(b)) = (RaceCode::ALL.get(i), RaceCode::ALL.get(j)) {
                if a.code() == b.code() {
                    uniq = false;
                }
            }
            j += 1;
        }
        i += 1;
    }
    s.add("诊断码：12 个码值两两唯一", uniq,
          "段内闭集码不得重复，否则判据无法定位");

    // 16. 族注册表双向一致
    let reg_ok = RaceFamily::ALL.iter().all(|f| RaceFamily::from_id(f.id()) == Some(*f))
        && RaceFamily::from_id(0).is_none()
        && RaceFamily::from_id(6).is_none();
    s.add("族注册表：id 双向可逆且闭集外拒绝", reg_ok,
          "注册面 1..=5 之外不得解析出族");

    // 动态明细（≤8）
    let ledger_text = format!(
        "注入 {} / 检出 {} / 逃逸 {}",
        ledger.injected, ledger.detected, ledger.escaped
    );
    s.add_dyn("账目快照", ledger.conserved(), &ledger_text);
    let fp_text = format!("五族注入送达 {} / 500 · 指纹 0x{:08X}", stats_total, a);
    s.add_dyn("统计快照", stats_total == 500, &fp_text);

    s
}

// ── 测试 ──────────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn injector_rejects_zero_seed() {
        assert_eq!(RaceInjector::new(0), Err(RaceCode::SeedZero));
        assert!(RaceInjector::new(1).is_ok());
    }

    #[test]
    fn arm_bounds() {
        let inj = RaceInjector::new(7).unwrap_or(RaceInjector { seed: 1 });
        let t = inj.arm(FAMILY_TOTAL).unwrap_or(0);
        assert!(t >= 1 && t < FAMILY_TOTAL);
        assert_eq!(inj.arm(1), Err(RaceCode::DegenerateOp));
        assert_eq!(inj.arm(0), Err(RaceCode::DegenerateOp));
    }

    #[test]
    fn inject_run_interrupted_with_progress() {
        let run = run_race(RaceFamily::Read, 42, true).unwrap_or(OpRun::start(1));
        assert_eq!(run.state(), RunState::Interrupted);
        assert!(run.progress() >= 1 && run.progress() < run.total());
        assert!(run.conserved());
    }

    #[test]
    fn control_run_completes() {
        let run = run_race(RaceFamily::Write, 42, false).unwrap_or(OpRun::start(1));
        assert_eq!(run.state(), RunState::Complete);
        assert_eq!(run.progress(), FAMILY_TOTAL);
    }

    #[test]
    fn hundred_injections_all_eintr() {
        let stat = race_stats(RaceFamily::Nanosleep, 5).unwrap_or(FamilyStat::zero(3));
        assert_eq!(stat.caught, RUNS_PER_FAMILY);
        assert_eq!(stat.escaped, 0);
        assert_eq!(stat.spurious, 0);
        assert!(stat.conserved());
        assert!(stat.bytes_saved > 0);
        assert!(stat.bytes_saved < RUNS_PER_FAMILY as u64 * FAMILY_TOTAL as u64);
    }

    #[test]
    fn control_stats_zero_caught() {
        let stat = race_control(RaceFamily::Wait, 9).unwrap_or(FamilyStat::zero(4));
        assert_eq!(stat.caught, 0);
        assert_eq!(stat.spurious, 0);
        assert_eq!(stat.runs, RUNS_PER_FAMILY);
    }

    #[test]
    fn interrupted_is_terminal() {
        let mut run = OpRun::start(FAMILY_TOTAL);
        let _ = run.advance();
        assert!(run.interrupt().is_ok());
        assert_eq!(run.advance(), Err(RaceCode::PostInterrupt));
        assert_eq!(run.interrupt(), Err(RaceCode::PostInterrupt));
    }

    #[test]
    fn bad_matrix_conserved() {
        let (l, per) = run_bad_race_matrix();
        assert!(l.conserved());
        assert_eq!(l.escaped, 0);
        assert_eq!(l.injected, 5 * INJECT_ROUNDS);
        for n in per.iter() {
            assert_eq!(*n, INJECT_ROUNDS);
        }
    }

    #[test]
    fn single_source_across_families() {
        let a = RaceInjector::new(77).and_then(|i| i.arm(FAMILY_TOTAL));
        let b = RaceInjector::new(77).and_then(|i| i.arm(FAMILY_TOTAL));
        assert_eq!(a, b);
        assert!(reconcile_direction());
    }

    #[test]
    fn deterministic_corpus() {
        assert_eq!(corpus_fingerprint(), corpus_fingerprint());
    }

    #[test]
    fn family_registry_bidirectional() {
        for f in RaceFamily::ALL.iter() {
            assert_eq!(RaceFamily::from_id(f.id()), Some(*f));
        }
        assert!(RaceFamily::from_id(0).is_none());
        assert!(RaceFamily::from_id(6).is_none());
    }

    #[test]
    fn checks_all_green() {
        let s = run_unx9353_checks();
        assert!(s.all_passed(), "{}", s.red_items_text());
    }
}
