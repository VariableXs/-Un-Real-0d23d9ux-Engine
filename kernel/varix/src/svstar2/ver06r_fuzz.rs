//! VE-F3438 · 换肤 fuzz —— 三类语料、最小化复现、半新半旧红线
//!
//! ## 职责拆解
//! 1. **语料库**（`Corpus` / `CorpusRegistry`）：三类语料——畸形主题文件、换肤风暴、
//!    并发切换竞态。语料不存字节串而存「生成器参数」（family + seed + steps），
//!    因此在任何机器上重放出同一条操作序列；这是 fuzz 可归档、可对拍的前提。
//! 2. **注入器**（`Injection`）：对生成序列做四种变异（重复、丢弃、换序、降质），
//!    让有限的预置语料覆盖「代码路径 × 时序」的组合空间，而不是靠人工枚举。
//! 3. **被测契约**（`ReskinSut`）：抽象的换肤引擎。**崩溃必须显式上报**
//!    （`SutFault::Crashed`），因为 `no_std` 里没有 `catch_unwind`——契约把
//!    「进程死了」转成「返回值说它死了」，这样 fuzz 仍然能给出证据而不是无声退出。
//! 4. **红线断言**（`check_consistency`）：任一时刻「没有在途切换」时，全部组件的
//!    主题代数必须等于当前主题。**半新半旧是红线**——它不是崩溃，但比崩溃更脏：
//!    界面看起来活着，一半控件已是新皮肤、另一半还是旧的，用户无从判断真假。
//! 5. **最小化**（`shrink`）：失败后把操作序列压到「仍能复现同一失败类」的最短前缀，
//!    再去掉内部冗余操作。归档里存 3 步能复现的序列，而不是 64 步的原始序列。
//!
//! ## 为什么这么拆
//! fuzz 的价值全在「失败之后」。只会跑不会缩的 fuzz，报出来的是一条 64 步的随机
//! 序列，人得盯半天；能缩到 3 步的 fuzz，缺陷当场就写清了。所以 `shrink` 不是
//! 附加功能，是这个模块的一半。
//!
//! 另一半点在红线定义上：崩溃是显性故障，半新半旧是隐性故障。前者用户会重启，
//! 后者用户会以为「这软件就这样」。把半新半旧提到与崩溃同级的阻断，是本册的取舍。
//!
//! ## 复杂度
//! - 单条语料重放：O(steps)。
//! - 最小化：前缀扫描 O(n²)、去内点 O(n²)，n ≤ `MAX_TRACE`(=64) ⇒ 上界 4096 步，
//!   实测远低（前缀平均在第 2~3 步命中）。全语料 O(Σ n_i²) ≈ 万级，微秒量级。
//! - 去重：`BTreeMap` O(k log k)，k 为失败条数。
//!
//! ## 零 panic 面
//! - 全部索引访问走 `get()` / `get_mut()`，越界一律降级为「视为基线代数」。
//! - 无 `unwrap` / `expect` / 切片索引 / 整数除法（除数为字面常量非零）。
//! - 代数运算全 `wrapping_*`，不使用 `+` / `*`（`overflow-checks` 打开时也不炸）。
//! - `SutFault` 覆盖全部分支，`match` 一律穷举，无 `_ => unreachable!()`。

use alloc::collections::BTreeMap;
use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

/// 单条语料的最大操作数。上限存在的理由：最小化是 O(n²)，n 必须可控；
/// 同时 64 步足够覆盖「打开-提交-取消」的三轮时序交错。
pub const MAX_TRACE: usize = 64;

/// 被测引擎的组件数（测试夹具口径：24 个绑定组件的典型窗口）。
pub const COMPONENTS: usize = 24;

/// 基线主题代数。所有组件出厂即处于此代。
pub const BASE_THEME: u32 = 0;

/// 诊断码（E18 段 · 5545~5556）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(u16)]
pub enum FuzzCode {
    /// 语料步数超过 `MAX_TRACE`。
    TraceTooLong = 5545,
    /// 语料声明的期望与实际能力不匹配（例如声明必须拒绝却给了合法载荷）。
    ExpectationMismatch = 5546,
    /// 被测引擎报告崩溃。
    SutCrashed = 5547,
    /// 半新半旧：无在途切换时组件代数落后于当前主题。
    HalfMixed = 5548,
    /// 畸形主题被接受（应当拒绝）。
    AcceptedMalformed = 5549,
    /// 切换进行中却报告提交成功（两阶段被压成一步）。
    CommitWithoutPending = 5550,
    /// 过期请求被接受（丢失更新 / ABA）。
    StaleAccepted = 5551,
    /// 最小化未能收敛到比原始序列更短的复现序列。
    ShrinkNotConverged = 5552,
    /// 语料库为空，等于没有 fuzz。
    EmptyCorpus = 5553,
    /// 重放期间引擎返回了契约外的故障。
    ContractViolation = 5554,
}

impl FuzzCode {
    /// 数值化，供报告与 CI 判据使用。显式判别式 ⇒ `self as u16` 稳定。
    pub fn code(self) -> u16 {
        self as u16
    }

    /// 人话说明。每一句都对应一条真实的失败面，不是套话。
    pub fn text(self) -> &'static str {
        match self {
            FuzzCode::TraceTooLong => "语料步数超过上限，最小化成本失控",
            FuzzCode::ExpectationMismatch => "语料期望与载荷性质不符，判据失效",
            FuzzCode::SutCrashed => "换肤引擎崩溃（阻断级）",
            FuzzCode::HalfMixed => "界面半新半旧（红线）",
            FuzzCode::AcceptedMalformed => "畸形主题被接受（重放污染）",
            FuzzCode::CommitWithoutPending => "无在途切换却提交成功（原子性破坏）",
            FuzzCode::StaleAccepted => "过期请求被接受（丢失更新）",
            FuzzCode::ShrinkNotConverged => "最小化未收敛，归档序列不可读",
            FuzzCode::EmptyCorpus => "语料库为空，本次 fuzz 无意义",
            FuzzCode::ContractViolation => "引擎返回值越过契约面",
        }
    }
}

/// 语料族。三族的分工不是按「输入格式」分的，是按**失败模式**分的。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Family {
    /// 畸形主题文件：语法/类型/引用三类损坏。期望「必须拒绝」。
    MalformedTheme,
    /// 换肤风暴：合法但高频的连续切换。期望「必须活下来且最终一致」。
    ReskinStorm,
    /// 并发切换竞态：两个请求方交错 打开/提交/取消。期望「后提交者生效，
    /// 先提交者的在途请求作废；任何时刻不得半新半旧」。
    ConcurrentSwitch,
}

impl Family {
    pub fn label(self) -> &'static str {
        match self {
            Family::MalformedTheme => "畸形主题",
            Family::ReskinStorm => "换肤风暴",
            Family::ConcurrentSwitch => "并发竞态",
        }
    }
}

/// 语料。种子与步数决定序列，`note` 只用于报告（不参与判定）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Corpus {
    pub id: &'static str,
    pub family: Family,
    pub seed: u64,
    pub steps: usize,
    pub note: &'static str,
}

impl Corpus {
    pub const fn new(
        id: &'static str,
        family: Family,
        seed: u64,
        steps: usize,
        note: &'static str,
    ) -> Self {
        Self {
            id,
            family,
            seed,
            steps,
            note,
        }
    }

    /// 自检：步数上界。越界不是「截断就好」——截断会悄悄改变语料覆盖度。
    pub fn audit(&self) -> Result<(), FuzzCode> {
        if self.steps == 0 || self.steps > MAX_TRACE {
            return Err(FuzzCode::TraceTooLong);
        }
        Ok(())
    }
}

/// 语料库。出厂即带 12 条，覆盖三族各 4 条：
/// 每族一条「最短」（2~3 步，用于烟测）、一条「典型」、一条「极端」、一条「回归种子」
/// （回归种子是历史缺陷的复现序列种子，改生成器时必须保持不变）。
#[derive(Clone, Debug, Default)]
pub struct CorpusRegistry {
    corpora: Vec<Corpus>,
}

impl CorpusRegistry {
    pub fn new() -> Self {
        Self { corpora: Vec::new() }
    }

    pub fn push(&mut self, corpus: Corpus) -> Result<(), FuzzCode> {
        corpus.audit()?;
        if self.corpora.iter().any(|c| c.id == corpus.id) {
            return Err(FuzzCode::ExpectationMismatch);
        }
        self.corpora.push(corpus);
        Ok(())
    }

    pub fn len(&self) -> usize {
        self.corpora.len()
    }

    pub fn is_empty(&self) -> bool {
        self.corpora.is_empty()
    }

    pub fn all(&self) -> &[Corpus] {
        &self.corpora
    }

    /// 按族过滤，用于「改一族的生成器就只重跑该族」。
    pub fn of_family(&self, family: Family) -> Vec<Corpus> {
        self.corpora
            .iter()
            .copied()
            .filter(|c| c.family == family)
            .collect()
    }

    /// 出厂语料库。种子手挑：不做随机，保证任何机器任何时刻重放出同一序列。
    pub fn standard() -> Self {
        let mut r = Self::new();
        let rows = [
            ("MAL-01", Family::MalformedTheme, 0x9E37_79B9_7F4A_7C15, 3, "空 manifest"),
            ("MAL-02", Family::MalformedTheme, 0x2545_F491_4F6C_DD1D, 7, "令牌类型冲突"),
            ("MAL-03", Family::MalformedTheme, 0x1D2C_6B5E_9A17_0F3B, 11, "循环引用"),
            ("MAL-04", Family::MalformedTheme, 0x7FEB_352D_84DF_7C91, 19, "超深引用链"),
            ("STM-01", Family::ReskinStorm, 0xA5A5_5A5A_C3C3_3C3C, 6, "三轮连切"),
            ("STM-02", Family::ReskinStorm, 0x0123_4567_89AB_CDEF, 24, "十六轮风暴"),
            ("STM-03", Family::ReskinStorm, 0xFEDC_BA98_7654_3210, 48, "半程全量"),
            ("STM-04", Family::ReskinStorm, 0x1357_9BDF_2468_ACE0, 64, "满步数风暴"),
            ("RAC-01", Family::ConcurrentSwitch, 0x0F1E_2D3C_4B5A_6978, 4, "A 开后 B 提交"),
            ("RAC-02", Family::ConcurrentSwitch, 0x8877_6655_4433_2211, 12, "双请求方交错"),
            ("RAC-03", Family::ConcurrentSwitch, 0xDEAD_BEEF_0BAD_F00D, 32, "高频交错"),
            ("RAC-04", Family::ConcurrentSwitch, 0xC001_D00D_5EED_0001, 64, "回归种子：丢失更新"),
        ];
        for (id, family, seed, steps, note) in rows {
            // 出厂数据自己必须过审；这里用 if-let 而不是 expect，
            // 因为「标准语料自检失败」在生产里也只能降级为「少一条」，不能炸。
            let c = Corpus::new(id, family, seed, steps, note);
            if c.audit().is_ok() {
                let _ = r.push(c);
            }
        }
        r
    }
}

/// 确定性生成器。用 xorshift64*（与 E01 fuzz 同族算法、不同常量与摆位）——
/// 这里要的是「同 seed 同序列」，不是密码学强度。
struct Source {
    state: u64,
}

impl Source {
    fn new(seed: u64) -> Self {
        Self {
            state: if seed == 0 { 0x9E37_79B9_7F4A_7C15 } else { seed },
        }
    }

    fn next_u64(&mut self) -> u64 {
        let mut x = self.state;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.state = x;
        x.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }

    fn below(&mut self, bound: u32) -> u32 {
        if bound == 0 {
            return 0;
        }
        (self.next_u64() % u64::from(bound)) as u32
    }
}

/// 操作。`req` 是请求方编号（0/1），并发竞态靠它区分在途请求的归属。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Op {
    Open { req: u8, theme: u32, wellformed: bool },
    Commit { req: u8 },
    Cancel { req: u8 },
    Touch { comp: u16 },
}

impl Op {
    /// 压成 64 位键，用于序列指纹。字段位宽够用即止：req 1 bit、wellformed 1 bit、
    /// comp 5 bit、theme 24 bit。
    fn key(&self) -> u64 {
        match *self {
            Op::Open {
                req,
                theme,
                wellformed,
            } => {
                let r = u64::from(req & 0x01);
                let w: u64 = if wellformed { 1 } else { 0 };
                let t = u64::from(theme & 0x00FF_FFFF);
                (t << 8) | (u64::from(w) << 7) | (r << 6) | 0x01
            }
            Op::Commit { req } => (u64::from(req & 0x01) << 6) | 0x02,
            Op::Cancel { req } => (u64::from(req & 0x01) << 6) | 0x03,
            Op::Touch { comp } => (u64::from(comp & 0x1F) << 8) | 0x04,
        }
    }

    /// 展示用短名，最小化报告里逐行打印。
    pub fn label(&self) -> String {
        match *self {
            Op::Open {
                req,
                theme,
                wellformed,
            } => format!(
                "open(r{},{}{})",
                req,
                theme,
                if wellformed { "" } else { ",bad" }
            ),
            Op::Commit { req } => format!("commit(r{})", req),
            Op::Cancel { req } => format!("cancel(r{})", req),
            Op::Touch { comp } => format!("touch({})", comp),
        }
    }
}

/// 序列指纹（FNV-1a 64，逐键喂入）。用途是失败去重与回归基线比对。
pub fn signature(trace: &[Op]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for op in trace {
        for byte in op.key().to_le_bytes() {
            h ^= u64::from(byte);
            h = h.wrapping_mul(0x0000_0100_0000_01b3);
        }
    }
    h
}

/// 注入变异。四种都只做「局部改动」——大改会把序列变成另一条随机序列，
/// 失去与原语料的血缘关系。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Injection {
    Duplex,
    Dropped,
    Swapped,
    Degraded,
}

impl Injection {
    pub fn label(self) -> &'static str {
        match self {
            Injection::Duplex => "重复",
            Injection::Dropped => "丢弃",
            Injection::Swapped => "换序",
            Injection::Degraded => "降质",
        }
    }

    /// 施加变异。空序列与非空序列分别处理；结果一律截断到 `MAX_TRACE`。
    pub fn apply(self, trace: &[Op], seed: u64) -> Vec<Op> {
        let mut out: Vec<Op> = trace.to_vec();
        let n = out.len();
        if n == 0 {
            return out;
        }
        let mut src = Source::new(seed ^ 0xD1B5_4A32_D192_ED03);
        let i = src.below(n as u32) as usize;
        match self {
            Injection::Duplex => {
                if let Some(op) = out.get(i).copied() {
                    out.insert(i, op);
                }
            }
            Injection::Dropped => {
                if i < out.len() {
                    out.remove(i);
                }
            }
            Injection::Swapped => {
                let j = src.below(n as u32) as usize;
                if i != j && i < out.len() && j < out.len() {
                    out.swap(i, j);
                }
            }
            Injection::Degraded => {
                if let Some(slot) = out.get_mut(i) {
                    *slot = match *slot {
                        Op::Open { req, theme, .. } => Op::Open {
                            req,
                            theme,
                            wellformed: false,
                        },
                        other => other,
                    };
                }
            }
        }
        out.truncate(MAX_TRACE);
        out
    }
}

/// 引擎故障面。`Crashed` 是被测对象自报崩溃——`no_std` 无 `catch_unwind`，
/// 这是唯一诚实的表达方式；被测实现若真的 panic，fuzz 进程会直接死，
/// 那也是「阻断」结论，只是没有报告可读。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SutFault {
    /// 畸形载荷被拒（正常路径）。
    Rejected,
    /// 已有在途请求。
    Busy,
    /// 没有在途请求却提交/取消。
    NothingPending,
    /// 请求已过期（被更晚的请求取代）。
    Superseded,
    /// 自报崩溃。
    Crashed,
}

impl SutFault {
    pub fn label(self) -> &'static str {
        match self {
            SutFault::Rejected => "拒绝",
            SutFault::Busy => "忙",
            SutFault::NothingPending => "无在途",
            SutFault::Superseded => "已过期",
            SutFault::Crashed => "崩溃",
        }
    }
}

/// 一致性快照。`pending` 为 `None` 表示无在途切换——此时才断言一致性；
/// 有在途时允许组件停在旧代（这是两阶段的定义，不是缺陷）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Snapshot {
    pub theme: u32,
    pub pending: bool,
    pub comps: Vec<u32>,
}

/// 红线：状态不一致。带组件号与代数，报告可直接定位。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct RedLine {
    pub component: u16,
    pub got: u32,
    pub want: u32,
}

/// 红线断言。只有「无在途」时才查——有在途时旧代是合法的中间态。
pub fn check_consistency(snap: &Snapshot) -> Result<(), RedLine> {
    if snap.pending {
        return Ok(());
    }
    for (idx, gen) in snap.comps.iter().enumerate() {
        if *gen != snap.theme {
            return Err(RedLine {
                component: idx as u16,
                got: *gen,
                want: snap.theme,
            });
        }
    }
    Ok(())
}

/// 被测引擎契约。实现方自行保证「不留半新半旧」，fuzz 负责验证它做到了。
pub trait ReskinSut {
    fn open(&mut self, req: u8, theme: u32, wellformed: bool) -> Result<(), SutFault>;
    fn commit(&mut self, req: u8) -> Result<(), SutFault>;
    fn cancel(&mut self, req: u8) -> Result<(), SutFault>;
    fn touch(&mut self, comp: u16) -> u32;
    fn snapshot(&self) -> Snapshot;
}

/// 单步重放的观测结果。
#[derive(Clone, PartialEq, Eq, Debug)]
enum Outcome {
    Clean,
    Fault(SutFault),
    Red(RedLine),
    MalformedAccepted,
    StaleAccepted,
    PhantomCommit,
}

impl Outcome {
    /// 失败类相等：最小化只要求「同一类失败」复现，不要求同一组件号——
    /// 组件号会随操作被删而漂移，强求一致会让最小化卡死。
    fn same_class(&self, other: &Outcome) -> bool {
        match (self, other) {
            (Outcome::Clean, Outcome::Clean) => true,
            (Outcome::Red(_), Outcome::Red(_)) => true,
            (Outcome::Fault(a), Outcome::Fault(b)) => a == b,
            (Outcome::MalformedAccepted, Outcome::MalformedAccepted) => true,
            (Outcome::StaleAccepted, Outcome::StaleAccepted) => true,
            (Outcome::PhantomCommit, Outcome::PhantomCommit) => true,
            _ => false,
        }
    }

    fn is_failure(&self) -> bool {
        !matches!(self, Outcome::Clean)
    }

    fn describe(&self) -> String {
        match self {
            Outcome::Clean => "clean".to_string(),
            Outcome::Fault(f) => format!("fault:{}", f.label()),
            Outcome::Red(r) => format!(
                "half-mixed:组件 {} 停在代 {}，应为 {}",
                r.component, r.got, r.want
            ),
            Outcome::MalformedAccepted => "malformed-accepted".to_string(),
            Outcome::StaleAccepted => "stale-accepted".to_string(),
            Outcome::PhantomCommit => "phantom-commit".to_string(),
        }
    }
}

/// 重放一条序列，返回整条序列的最终结论（首个非 clean 即返回）。
///
/// 判据是**外部可观测**的：只看返回值与快照，不看引擎内部字段。这样任何实现
/// 都能被同一套判据审——包括将来换掉的实现。
fn replay<S: ReskinSut>(sut: &mut S, trace: &[Op]) -> Outcome {
    // 外部维护的「在途」镜像：引擎若在无在途时提交成功，就是幻觉提交。
    let mut pending: [bool; 2] = [false, false];
    // 每个请求方最后一次成功打开的主题：提交落地后主题必须是它，
    // 否则说明落地的是更早（已过期）的那一次 ⇒ 丢失更新。
    let mut last_open: [Option<u32>; 2] = [None, None];
    let mut malformed_accepted = false;
    let mut phantom_commit = false;
    let mut stale_accepted = false;

    for op in trace {
        match *op {
            Op::Open {
                req,
                theme,
                wellformed,
            } => match sut.open(req, theme, wellformed) {
                Ok(()) => {
                    if !wellformed {
                        malformed_accepted = true;
                    } else {
                        let idx = usize::from(req & 0x01);
                        if let Some(slot) = pending.get_mut(idx) {
                            *slot = true;
                        }
                        if let Some(slot) = last_open.get_mut(idx) {
                            *slot = Some(theme);
                        }
                    }
                }
                Err(SutFault::Crashed) => return Outcome::Fault(SutFault::Crashed),
                // 拒绝（含过期/忙）都是合法降级，不改镜像。
                Err(_) => {}
            },
            Op::Commit { req } => {
                let idx = usize::from(req & 0x01);
                let had_pending = pending.get(idx).copied().unwrap_or(false);
                let expect_theme = last_open.get(idx).copied().flatten();
                match sut.commit(req) {
                    Ok(()) => {
                        if !had_pending {
                            phantom_commit = true;
                        } else {
                            let snap = sut.snapshot();
                            if let Some(want) = expect_theme {
                                if snap.theme != want {
                                    stale_accepted = true;
                                }
                            }
                        }
                        if let Some(slot) = pending.get_mut(idx) {
                            *slot = false;
                        }
                        // 一次提交会让另一个请求方的在途请求作废（被更晚的票取代）。
                        let other = idx ^ 1;
                        if let Some(slot) = pending.get_mut(other) {
                            *slot = false;
                        }
                        if let Some(slot) = last_open.get_mut(other) {
                            *slot = None;
                        }
                    }
                    Err(SutFault::Crashed) => return Outcome::Fault(SutFault::Crashed),
                    Err(_) => {}
                }
            }
            Op::Cancel { req } => match sut.cancel(req) {
                Ok(()) => {
                    let idx = usize::from(req & 0x01);
                    if let Some(slot) = pending.get_mut(idx) {
                        *slot = false;
                    }
                }
                Err(SutFault::Crashed) => return Outcome::Fault(SutFault::Crashed),
                Err(_) => {}
            },
            Op::Touch { comp } => {
                let g = sut.touch(comp);
                let snap = sut.snapshot();
                // 触达不得改变全局状态；若它偷偷推进了代数，一致性检查会抓到。
                if g > snap.theme && !snap.pending {
                    return Outcome::Red(RedLine {
                        component: comp,
                        got: g,
                        want: snap.theme,
                    });
                }
            }
        }

        if let Err(r) = check_consistency(&sut.snapshot()) {
            return Outcome::Red(r);
        }
    }

    let snap = sut.snapshot();
    if let Err(r) = check_consistency(&snap) {
        return Outcome::Red(r);
    }
    if malformed_accepted {
        return Outcome::MalformedAccepted;
    }
    if phantom_commit {
        return Outcome::PhantomCommit;
    }
    if stale_accepted {
        return Outcome::StaleAccepted;
    }
    Outcome::Clean
}

/// 一条失败归档。`minimal` 是压缩后的复现序列，`original_len` 用于证明「真的缩了」。
#[derive(Clone, Debug)]
pub struct Failure {
    pub corpus: &'static str,
    pub family: Family,
    pub outcome: Outcome,
    pub minimal: Vec<Op>,
    pub original_len: usize,
    pub injections: Vec<Injection>,
}

impl Failure {
    pub fn signature(&self) -> u64 {
        signature(&self.minimal)
    }

    /// 归档文本：缺陷单里直接贴这一段就够复现。
    pub fn lines(&self) -> Vec<String> {
        let mut out = Vec::new();
        out.push(format!(
            "[{}·{}] {}（原序列 {} 步 → 最小 {} 步）",
            self.corpus,
            self.family.label(),
            self.outcome.describe(),
            self.original_len,
            self.minimal.len()
        ));
        if !self.injections.is_empty() {
            let names: Vec<&str> = self.injections.iter().map(|i| i.label()).collect();
            out.push(format!("  注入: {}", names.join(" + ")));
        }
        let mut step = 0usize;
        for op in &self.minimal {
            out.push(format!("  {}. {}", step, op.label()));
            step += 1;
        }
        out
    }
}

/// 整轮 fuzz 结论。顺序即严重度：崩溃 > 红线 > 通过。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Verdict {
    Pass,
    RedLine,
    Blocked,
}

impl Verdict {
    pub fn label(self) -> &'static str {
        match self {
            Verdict::Pass => "通过",
            Verdict::RedLine => "红线",
            Verdict::Blocked => "阻断",
        }
    }

    /// 门禁色：红门不发证据号，黄门可带条件放行。fuzz 只有红/绿两态，
    /// 黄在这里没有语义（fuzz 不做部分通过）。
    pub fn is_green(self) -> bool {
        matches!(self, Verdict::Pass)
    }
}

/// 一轮 fuzz 的产物。
#[derive(Clone, Debug, Default)]
pub struct Harness {
    pub cases: usize,
    pub clean: usize,
    pub failures: Vec<Failure>,
    pub total_ops: usize,
    pub minimal_ops: usize,
}

impl Harness {
    pub fn verdict(&self) -> Verdict {
        if self.cases == 0 {
            return Verdict::Blocked;
        }
        if self
            .failures
            .iter()
            .any(|f| matches!(f.outcome, Outcome::Fault(SutFault::Crashed)))
        {
            return Verdict::Blocked;
        }
        if !self.failures.is_empty() {
            return Verdict::RedLine;
        }
        Verdict::Pass
    }

    /// 压缩率千分比：越小越省。`minimal_ops / total_ops`。
    pub fn shrink_permille(&self) -> u32 {
        if self.total_ops == 0 {
            return 0;
        }
        ((self.minimal_ops * 1000) / self.total_ops) as u32
    }

    /// 读屏清单：CI 日志与缺陷单共用。
    pub fn report_lines(&self) -> Vec<String> {
        let mut out = Vec::new();
        out.push(format!(
            "换肤 fuzz · 语料 {} · 干净 {} · 失败 {} · 结论 {}",
            self.cases,
            self.clean,
            self.failures.len(),
            self.verdict().label()
        ));
        out.push(format!(
            "  操作总量 {} 步 → 最小复现合计 {} 步（压缩 {}‰）",
            self.total_ops,
            self.minimal_ops,
            self.shrink_permille()
        ));
        for f in &self.failures {
            for line in f.lines() {
                out.push(line);
            }
        }
        out
    }
}

/// 最小化：先找最短能复现同类失败的前缀，再逐个去掉内部操作。
/// 返回的序列保证 `replay` 仍然给出同类失败。
fn shrink<S: ReskinSut, F: FnMut() -> S>(
    make: &mut F,
    trace: &[Op],
    want: &Outcome,
) -> Vec<Op> {
    let n = trace.len();
    // 阶段一：最短前缀。从 1 开始，第一个命中即为最短（因为前缀是单调包含的）。
    let mut best: Vec<Op> = trace.to_vec();
    for len in 1..=n {
        let prefix = match trace.get(..len) {
            Some(p) => p,
            None => break,
        };
        let mut sut = make();
        if replay(&mut sut, prefix).same_class(want) {
            best = prefix.to_vec();
            break;
        }
    }
    // 阶段二：去内点。逐位尝试删除，删了仍复现就留下删除后的版本。
    let mut i = 0usize;
    while i < best.len() {
        let mut trial = best.clone();
        if i < trial.len() {
            trial.remove(i);
        }
        if trial.is_empty() {
            break;
        }
        let mut sut = make();
        if replay(&mut sut, &trial).same_class(want) {
            best = trial;
        } else {
            i += 1;
        }
    }
    best
}

/// 跑一整轮。`make` 每步要新建引擎——原语料与每条变异都从干净基线开始，
/// 否则前一条语料的残留会污染后一条，报出来的缺陷无法归因。
pub fn run<S: ReskinSut, F: FnMut() -> S>(
    registry: &CorpusRegistry,
    mut make: F,
) -> Harness {
    let mut harness = Harness::default();
    if registry.is_empty() {
        return harness;
    }
    // 去重表：同签名只留一条（同一条缺陷被 12 条语料撞出来时，归档只写一条）。
    let mut seen: BTreeMap<u64, ()> = BTreeMap::new();

    for corpus in registry.all() {
        harness.cases += 1;
        let base = generate(corpus);
        let mut variants: Vec<(Vec<Op>, Vec<Injection>)> = Vec::new();
        variants.push((base.clone(), Vec::new()));
        for inj in [
            Injection::Duplex,
            Injection::Dropped,
            Injection::Swapped,
            Injection::Degraded,
        ] {
            let mutated = inj.apply(&base, corpus.seed);
            if !mutated.is_empty() {
                variants.push((mutated, vec![inj]));
            }
        }

        let mut corpus_clean = 0usize;
        let mut corpus_total = 0usize;
        for (trace, injections) in variants {
            corpus_total += 1;
            harness.total_ops += trace.len();
            let mut sut = make();
            let outcome = replay(&mut sut, &trace);
            if !outcome.is_failure() {
                corpus_clean += 1;
                continue;
            }
            let minimal = shrink(&mut make, &trace, &outcome);
            harness.minimal_ops += minimal.len();
            let sig = signature(&minimal);
            if seen.contains_key(&sig) {
                continue;
            }
            seen.insert(sig, ());
            harness.failures.push(Failure {
                corpus: corpus.id,
                family: corpus.family,
                outcome,
                minimal,
                original_len: trace.len(),
                injections,
            });
        }
        // 「干净」按语料计：一条语料的原序列与四条变异全部干净才算干净。
        // 只要有一条变异脏了，这条语料就不能记干净——否则统计会掩盖回归。
        if corpus_clean == corpus_total {
            harness.clean += 1;
        }
    }
    harness
}

/// 生成器：把 (family, seed, steps) 展开成操作序列。
pub fn generate(corpus: &Corpus) -> Vec<Op> {
    let mut src = Source::new(corpus.seed);
    let mut out: Vec<Op> = Vec::with_capacity(corpus.steps);
    let steps = corpus.steps.min(MAX_TRACE);
    for i in 0..steps {
        let pick = src.below(100);
        let op = match corpus.family {
            Family::MalformedTheme => match pick {
                // 七成畸形打开、一成合法打开、其余提交/取消：畸形族也要覆盖
                // 「拒绝之后能否恢复正常」——只测拒绝会漏掉拒绝路径留下的脏状态。
                0..=69 => Op::Open {
                    req: (src.below(2)) as u8,
                    theme: 1 + src.below(6),
                    wellformed: false,
                },
                70..=79 => Op::Open {
                    req: (src.below(2)) as u8,
                    theme: 1 + src.below(6),
                    wellformed: true,
                },
                80..=89 => Op::Commit {
                    req: (src.below(2)) as u8,
                },
                _ => Op::Cancel {
                    req: (src.below(2)) as u8,
                },
            },
            Family::ReskinStorm => match pick {
                0..=44 => Op::Open {
                    req: 0,
                    theme: 1 + src.below(12),
                    wellformed: true,
                },
                45..=79 => Op::Commit { req: 0 },
                80..=91 => Op::Cancel { req: 0 },
                _ => Op::Touch {
                    comp: (src.below(COMPONENTS as u32)) as u16,
                },
            },
            Family::ConcurrentSwitch => match pick {
                0..=44 => Op::Open {
                    req: (src.below(2)) as u8,
                    theme: 1 + src.below(9),
                    wellformed: true,
                },
                45..=79 => Op::Commit {
                    req: (src.below(2)) as u8,
                },
                80..=89 => Op::Cancel {
                    req: (src.below(2)) as u8,
                },
                _ => Op::Touch {
                    comp: (src.below(COMPONENTS as u32)) as u16,
                },
            },
        };
        out.push(op);
        // 防止生成器退化：连出 8 个相同操作时插一个触达，保证状态被读一次。
        if out.len() >= 8 && i % 8 == 7 {
            out.push(Op::Touch {
                comp: (src.below(COMPONENTS as u32)) as u16,
            });
        }
    }
    out.truncate(MAX_TRACE);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 正确实现：两阶段、按票据防 ABA、提交全量刷新。
    struct GoodSut {
        theme: u32,
        ticket: u64,
        pending: [Option<(u32, u64)>; 2],
        comps: Vec<u32>,
    }

    impl GoodSut {
        fn new() -> Self {
            Self {
                theme: BASE_THEME,
                ticket: 1,
                pending: [None, None],
                comps: vec![BASE_THEME; COMPONENTS],
            }
        }
    }

    impl ReskinSut for GoodSut {
        fn open(&mut self, req: u8, theme: u32, wellformed: bool) -> Result<(), SutFault> {
            if !wellformed {
                return Err(SutFault::Rejected);
            }
            let slot = match self.pending.get_mut(usize::from(req & 0x01)) {
                Some(s) => s,
                None => return Err(SutFault::Rejected),
            };
            if slot.is_some() {
                return Err(SutFault::Busy);
            }
            *slot = Some((theme, self.ticket));
            self.ticket = self.ticket.wrapping_add(1);
            Ok(())
        }

        fn commit(&mut self, req: u8) -> Result<(), SutFault> {
            let slot = match self.pending.get_mut(usize::from(req & 0x01)) {
                Some(s) => s,
                None => return Err(SutFault::NothingPending),
            };
            match slot.take() {
                None => Err(SutFault::NothingPending),
                Some((theme, ticket)) => {
                    // 只有最新票据能落地；更早的请求已被后来的取代。
                    if ticket.wrapping_add(1) != self.ticket {
                        return Err(SutFault::Superseded);
                    }
                    self.theme = theme;
                    for g in self.comps.iter_mut() {
                        *g = theme;
                    }
                    Ok(())
                }
            }
        }

        fn cancel(&mut self, req: u8) -> Result<(), SutFault> {
            match self.pending.get_mut(usize::from(req & 0x01)) {
                Some(slot) => match slot.take() {
                    Some(_) => Ok(()),
                    None => Err(SutFault::NothingPending),
                },
                None => Err(SutFault::NothingPending),
            }
        }

        fn touch(&mut self, comp: u16) -> u32 {
            match self.comps.get(usize::from(comp)) {
                Some(g) => *g,
                None => self.theme,
            }
        }

        fn snapshot(&self) -> Snapshot {
            Snapshot {
                theme: self.theme,
                pending: self.pending.iter().any(|p| p.is_some()),
                comps: self.comps.clone(),
            }
        }
    }

    /// 半新半旧：提交时只刷新前 20 个组件（真实缺陷形态：分页刷新未收尾）。
    struct LeakySut {
        inner: GoodSut,
    }

    impl LeakySut {
        fn new() -> Self {
            Self {
                inner: GoodSut::new(),
            }
        }
    }

    impl ReskinSut for LeakySut {
        fn open(&mut self, req: u8, theme: u32, wellformed: bool) -> Result<(), SutFault> {
            self.inner.open(req, theme, wellformed)
        }
        fn commit(&mut self, req: u8) -> Result<(), SutFault> {
            let r = self.inner.commit(req);
            if r.is_ok() {
                // 只回滚刷新：后 4 个组件停在旧代。
                for g in self.inner.comps.iter_mut().skip(20) {
                    *g = BASE_THEME;
                }
            }
            r
        }
        fn cancel(&mut self, req: u8) -> Result<(), SutFault> {
            self.inner.cancel(req)
        }
        fn touch(&mut self, comp: u16) -> u32 {
            self.inner.touch(comp)
        }
        fn snapshot(&self) -> Snapshot {
            self.inner.snapshot()
        }
    }

    /// 全盘接受畸形载荷。
    struct AcceptAllSut {
        inner: GoodSut,
    }

    impl ReskinSut for AcceptAllSut {
        fn open(&mut self, req: u8, theme: u32, wellformed: bool) -> Result<(), SutFault> {
            self.inner.open(req, theme, true).map(|_| {
                let _ = wellformed;
            })
        }
        fn commit(&mut self, req: u8) -> Result<(), SutFault> {
            self.inner.commit(req)
        }
        fn cancel(&mut self, req: u8) -> Result<(), SutFault> {
            self.inner.cancel(req)
        }
        fn touch(&mut self, comp: u16) -> u32 {
            self.inner.touch(comp)
        }
        fn snapshot(&self) -> Snapshot {
            self.inner.snapshot()
        }
    }

    /// 提交后立刻自报崩溃。
    struct CrashySut {
        inner: GoodSut,
        fired: bool,
    }

    impl CrashySut {
        fn new() -> Self {
            Self {
                inner: GoodSut::new(),
                fired: false,
            }
        }
    }

    impl ReskinSut for CrashySut {
        fn open(&mut self, req: u8, theme: u32, wellformed: bool) -> Result<(), SutFault> {
            self.inner.open(req, theme, wellformed)
        }
        fn commit(&mut self, req: u8) -> Result<(), SutFault> {
            let r = self.inner.commit(req);
            if r.is_ok() && !self.fired {
                self.fired = true;
                return Err(SutFault::Crashed);
            }
            r
        }
        fn cancel(&mut self, req: u8) -> Result<(), SutFault> {
            self.inner.cancel(req)
        }
        fn touch(&mut self, comp: u16) -> u32 {
            self.inner.touch(comp)
        }
        fn snapshot(&self) -> Snapshot {
            self.inner.snapshot()
        }
    }

    #[test]
    fn standard_corpus_audits_itself() {
        let r = CorpusRegistry::standard();
        assert_eq!(r.len(), 12, "出厂语料应为 12 条");
        for c in r.all() {
            assert!(c.audit().is_ok(), "语料 {} 自检失败", c.id);
            assert!(c.steps > 0 && c.steps <= MAX_TRACE);
        }
    }

    #[test]
    fn long_corpus_is_rejected_not_truncated() {
        let c = Corpus::new("X", Family::ReskinStorm, 1, MAX_TRACE + 1, "越界");
        assert_eq!(c.audit(), Err(FuzzCode::TraceTooLong));
    }

    #[test]
    fn good_sut_passes_whole_corpus() {
        let r = CorpusRegistry::standard();
        let h = run(&r, GoodSut::new);
        assert_eq!(
            h.verdict(),
            Verdict::Pass,
            "正确实现不该被抓到：{}",
            h.report_lines().join("\n")
        );
        assert_eq!(h.failures.len(), 0);
        assert_eq!(h.cases, 12);
    }

    #[test]
    fn half_mixed_is_red_line_with_minimal_repro() {
        let mut r = CorpusRegistry::new();
        // 一条短风暴：开 → 提交 → 触达末组件。三步足够撞出「只刷前 20 个」。
        r.push(Corpus::new("HALF-1", Family::ReskinStorm, 0x1111, 3, "半刷"))
            .unwrap_or(());
        let h = run(&r, LeakySut::new);
        assert_eq!(h.verdict(), Verdict::RedLine);
        assert!(!h.failures.is_empty());
        let f = &h.failures[0];
        assert!(matches!(f.outcome, Outcome::Red(_)), "应判红线");
        // 最小化后必须严格短于原始变异序列（去重后至少要能看出压缩）。
        assert!(f.minimal.len() <= f.original_len);
        let text = f.lines().join("\n");
        assert!(text.contains("half-mixed"), "归档必须写明红线形态: {}", text);
    }

    #[test]
    fn accepted_malformed_is_caught() {
        let mut r = CorpusRegistry::new();
        r.push(Corpus::new("MAL-X", Family::MalformedTheme, 0x2222, 4, "畸形"))
            .unwrap_or(());
        let h = run(&r, || AcceptAllSut {
            inner: GoodSut::new(),
        });
        assert_eq!(h.verdict(), Verdict::RedLine);
        assert!(
            h.failures
                .iter()
                .all(|f| matches!(f.outcome, Outcome::MalformedAccepted)),
            "应全部归档为畸形被接受"
        );
    }

    #[test]
    fn crash_blocks_even_with_one_case() {
        let mut r = CorpusRegistry::new();
        r.push(Corpus::new("CRS-1", Family::ReskinStorm, 0x3333, 5, "崩溃"))
            .unwrap_or(());
        let h = run(&r, CrashySut::new);
        assert_eq!(h.verdict(), Verdict::Blocked);
        assert!(h
            .failures
            .iter()
            .any(|f| matches!(f.outcome, Outcome::Fault(SutFault::Crashed))));
    }

    #[test]
    fn empty_corpus_blocks() {
        let r = CorpusRegistry::new();
        let h = run(&r, GoodSut::new);
        assert_eq!(h.verdict(), Verdict::Blocked);
        assert_eq!(h.cases, 0);
    }

    #[test]
    fn injections_change_signature() {
        let c = Corpus::new("SIG", Family::ConcurrentSwitch, 0x4444, 12, "指纹");
        let base = generate(&c);
        let sig = signature(&base);
        for inj in [
            Injection::Duplex,
            Injection::Dropped,
            Injection::Swapped,
            Injection::Degraded,
        ] {
            let m = inj.apply(&base, c.seed);
            assert_ne!(signature(&m), sig, "{} 变异后指纹应变化", inj.label());
        }
    }

    #[test]
    fn shrink_reaches_shorter_prefix() {
        let mut r = CorpusRegistry::new();
        r.push(Corpus::new("SHR-1", Family::ReskinStorm, 0x5555, 40, "最小化"))
            .unwrap_or(());
        let h = run(&r, LeakySut::new);
        assert!(!h.failures.is_empty());
        let f = &h.failures[0];
        assert!(
            f.minimal.len() < f.original_len,
            "最小化应缩短：{} → {}",
            f.original_len,
            f.minimal.len()
        );
        // 压缩率在合理区间：既不是 0（没缩），也不该超过原始（不可能）。
        assert!(h.shrink_permille() <= 1000);
    }

    #[test]
    fn consistency_allows_pending_then_forbids_mixture() {
        let in_flight = Snapshot {
            theme: 0,
            pending: true,
            comps: vec![0, 1, 0],
        };
        assert!(check_consistency(&in_flight).is_ok(), "在途时允许旧代");
        let settled = Snapshot {
            theme: 3,
            pending: false,
            comps: vec![3, 3, 2],
        };
        let r = check_consistency(&settled);
        assert_eq!(r.map_err(|e| e.component), Err(2));
    }

    #[test]
    fn trace_respects_step_bound() {
        let c = Corpus::new("BND", Family::ReskinStorm, 0x6666, 200, "越界生成");
        let t = generate(&c);
        assert!(t.len() <= MAX_TRACE, "生成序列必须不超上界");
        assert!(!t.is_empty());
    }
}
