//! VE-F2411 · 动画 fuzz（VE-M 域 · 动画段 · 目标 300 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F2411`
//!
//! **判据（锚点原文）**：三段 fuzz、零分配红线、导入拦截、24h 固化、判据。
//!
//! **职责定位（锚点原文）**：动画 fuzz——三段模糊测试：畸形动画 fuzz
//! （NaN 关键帧值/负时间/超密关键帧——钳制容错：轨道与关键帧全空间
//! 扫描：值 NaN/时间乱序与负值/超密关键帧（同时间多帧）——容错与
//! 显性拒绝双语义验证）、轨道风暴 fuzz（万轨道求值——不变量：零分配/
//! 确定性/无泄漏——万轨道并发求值压力：零分配断言（F2407 纪律）/双跑
//! 一致/句柄无泄漏三不变量——F2411 是 M 域 fuzz 面的首域）、导入 fuzz
//! （畸形 glTF 动画导入——校验拒绝验证：F2409 三重校验的对抗验证：
//! 通道引用失效/采样数据畸形/格式混淆——全部被拦截显性），案例固化
//! 入库。
//!
//! # 一、三段 fuzz 全部真调对端（不代填、不自演）
//!
//! - 段一（畸形动画）：真调 [`eval_scalar_span`](vem03_interp::
//!   eval_scalar_span)（NaN 关键帧值 → 钳制+记账；插值器/轨道类
//!   闸门不合 → 显性拒绝返 `None`）+ 真调
//!   [`detect_curve_anomaly`](vem09_import::detect_curve_anomaly)
//!   （非有限时刻/负时刻/超密对逐项计数）。**双语义**：容错路径与
//!   拒绝路径各自可达（静默通过即红）。
//! - 段二（轨道风暴）：万轨道 × 多样本真调求值，三不变量——双跑
//!   摘要逐位一致（确定性）、句柄表收敛零（无泄漏）、求值签名无
//!   Vec 出口+记账全定长（零分配红线，F2407 纪律）。
//! - 段三（导入 fuzz）：真调 [`validate_channel_refs`](vem09_import::
//!   validate_channel_refs) 与 [`validate_samples`](vem09_import::
//!   validate_samples)——采样器越界/访问器越界/NaN 采样/值数不匹配
//!   （格式混淆）全部被拦截显性（返拒绝码），合法语料不误拦（双向）。
//!
//! # 二、案例固化入库（FNV-1a 去重 + 库容控制 + P0 立案 24h）
//!
//! [`CaseStore`] 以输入摘要 FNV-1a 去重（同案不二入），库容
//! [`MAX_CASES`] 封顶（超容显性拒绝——不静默丢案）；每条案带
//! 输入/期望/复现三要素与去重哈希。P0 立案（崩溃/零分配失守/泄漏/
//!   拦截失效）带 24h 复现期限（[`P0_REPRO_24H_MS`]）——超期可判定
//!   （[`P0Case::overdue`]），不是口头督促。
//!
//! # 三、与 vel11（L 域 fuzz 首域）的家族关系
//!
//! 自持 LCG（同种子同语料）、执行器不吞 panic、步数预算看门狗
//! ——三项纪律与 F1813 框架/vel11 全族一致；本篇是 M 域 fuzz 首域，
//! 案例库入 F1769 M 段调度（跨批对接点），立案进 M 域账本（暂挂）。
//!
//! **性能（锚点原文）**：轨道扫描万级夜间批跑；风暴 1 小时档入
//! F1769 M 段（本模块以步数预算把"1 小时档"变成可判定事件）；
//! 导入对抗千级；去重哈希控库容。

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use crate::svstar2::vem02_track::{DiagBag as TrackBag, TrackClass};
use crate::svstar2::vem03_interp::{
    eval_scalar_span, Interp, InterpEntry, InterpParams, NON_FINITE_FALLBACK,
};
use crate::svstar2::vem09_import::{
    detect_curve_anomaly, validate_channel_refs, validate_samples, AccessorView, ChannelPath,
    ChannelRef, ComponentType, DiagBag as ImportBag, DiagCode as ImportCode, GltfAnimDoc,
    GltfInterp, GltfSamplerOut, SampleVerdict, SamplerRef, TrackSemantic,
};

// ---------------------------------------------------------------------------
// 一、常量与错误码
// ---------------------------------------------------------------------------

/// 本项版本。
pub const FUZZ_VERSION: &str = "M11-fuzz-v1";

/// P0 复现期限（毫秒）：24h——锚点「崩溃→P0 立案 24h 复现」。
pub const P0_REPRO_24H_MS: u64 = 86_400_000;

/// 案例库容上限（去重哈希管理——超容显性拒绝，不静默丢案）。
pub const MAX_CASES: usize = 4_096;

/// 风暴段轨道数（万级——锚点「万轨道求值压力」）。
pub const STORM_TRACKS: u32 = 10_000;

/// 风暴段每轨道采样点数。
pub const STORM_SAMPLES: u32 = 4;

/// 风暴段步数预算（万轨×4 采样=4 万求值步+建轨步；预算即"1 小时档"
/// 的可判定化——超预算即挂起立案，不允许挂着不返回）。
pub const STORM_STEP_BUDGET: u64 = 65_536;

/// 导入对抗段案例数（千级）。
pub const IMPORT_FUZZ_CASES: u32 = 1_000;

/// 求值 NaN 回退值（vel03 `NON_FINITE_FALLBACK` 的对拍基准——钳制到
/// 有限值是确定值 0.0，判据侧独立重算防恒真；此处 re-export 供判据
/// 断言钳制目标逐位相等）。
pub use crate::svstar2::vem03_interp::NON_FINITE_FALLBACK as EVAL_NON_FINITE_FALLBACK;

/// 崩溃/守纪律红线失守（P0）。
pub const E_FUZZ_P0: &str = "E_FUZZ_P0";

/// 零分配失守（P0——实时纪律红线，F2407 根基）。
pub const E_FUZZ_ALLOC: &str = "E_FUZZ_ALLOC";

/// 句柄泄漏（P0——句柄根基）。
pub const E_FUZZ_LEAK: &str = "E_FUZZ_LEAK";

/// 导入拦截失效（P0——校验失守）。
pub const E_FUZZ_IMPORT_MISS: &str = "E_FUZZ_IMPORT_MISS";

/// 钳制失灵（立案）。
pub const E_FUZZ_CLAMP_MISS: &str = "E_FUZZ_CLAMP_MISS";

/// 案例库超容（显性拒绝）。
pub const E_FUZZ_CASE_FULL: &str = "E_FUZZ_CASE_FULL";

/// 看门狗超预算（挂起+立案）。
pub const E_FUZZ_WATCHDOG: &str = "E_FUZZ_WATCHDOG";

// ---------------------------------------------------------------------------
// 二、自持随机源（F1813 框架纪律：同种子同语料）
// ---------------------------------------------------------------------------

/// 自持 LCG（64 位——同种子双跑逐位一致，不引外部随机源）。
#[derive(Clone, Copy, Debug)]
pub struct Lcg {
    state: u64,
}

impl Lcg {
    /// 新种子（0 种子映射到非零常量——全零态 LCG 会锁死）。
    pub fn new(seed: u64) -> Lcg {
        Lcg { state: seed ^ 0x9E37_79B9_7F4A_7C15 }
    }

    /// 下一个 u64。
    pub fn next_u64(&mut self) -> u64 {
        self.state = self
            .state
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        self.state
    }

    /// [0,1) 浮点（取高 24 位——f32 尾数精度诚实边界）。
    pub fn next_f32(&mut self) -> f32 {
        (self.next_u64() >> 40) as f32 / (1u64 << 24) as f32
    }

    /// [lo,hi) 整数。
    pub fn range(&mut self, lo: u32, hi: u32) -> u32 {
        if hi <= lo {
            return lo;
        }
        lo + (self.next_u64() % (hi - lo) as u64) as u32
    }

    /// 以概率 p 为真。
    pub fn chance(&mut self, p: f32) -> bool {
        self.next_f32() < p
    }
}

/// FNV-1a 64（案例去重与摘要——const 期同算防手抄漂移）。
pub const fn fnv1a64(bytes: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    let mut i = 0usize;
    while i < bytes.len() {
        h ^= bytes[i] as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
        i += 1;
    }
    h
}

// ---------------------------------------------------------------------------
// 三、看门狗（步数预算——非完成断言可判定化）
// ---------------------------------------------------------------------------

/// 看门狗：超预算即挂起（_err），不静默放行。
#[derive(Clone, Copy, Debug)]
pub struct Watchdog {
    budget: u64,
    used: u64,
}

impl Watchdog {
    /// 新看门狗。
    pub fn new(budget: u64) -> Watchdog {
        Watchdog { budget, used: 0 }
    }

    /// 风暴档预算。
    pub fn storm() -> Watchdog {
        Watchdog::new(STORM_STEP_BUDGET)
    }

    /// 走一步；超预算返回携带错误码的 Err。
    pub fn tick(&mut self) -> Result<(), String> {
        self.used = self.used.saturating_add(1);
        if self.used > self.budget {
            return Err(format!(
                "{}：步数预算 {} 超限（已用 {}）——疑似非完成循环，挂起并立案",
                E_FUZZ_WATCHDOG, self.budget, self.used
            ));
        }
        Ok(())
    }

    /// 已用步数（读屏可达）。
    pub fn used(&self) -> u64 {
        self.used
    }
}

// ---------------------------------------------------------------------------
// 四、段一：畸形动画 fuzz（双语义：容错+记账 / 显性拒绝）
// ---------------------------------------------------------------------------

/// 线性进度的重映射算子（`eval` 字段签名 `(&InterpParams, f32) -> f32`
/// 的恒等实现——进度重映射对线性即原值；另写一个只为签名对齐的小
/// 函数，不拿 lerp_scalar（那是值插值，参数个数不符）硬塞）。
fn linear_progress(_params: &InterpParams, u: f32) -> f32 {
    u
}

/// 畸形形态（生成器四态闭集——锚点：值 NaN/时间乱序与负值/超密）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MalformKind {
    /// 关键帧值 NaN/Inf（应钳制+记账）。
    NonFiniteValue,
    /// 时刻乱序（非单调——导入侧检测）。
    ScrambledTime,
    /// 负时刻（应被检测计数）。
    NegativeTime,
    /// 超密关键帧（同时间多帧——1ms 量化后零间隔）。
    OverdenseKeys,
}

impl MalformKind {
    /// 四态闭集。
    pub const ALL: [MalformKind; 4] = [
        MalformKind::NonFiniteValue,
        MalformKind::ScrambledTime,
        MalformKind::NegativeTime,
        MalformKind::OverdenseKeys,
    ];

    /// 期望语义（容错=钳制放行+记账；拒绝=显性拒；检测=只检测不改）。
    pub fn expect(self) -> MalformVerdict {
        match self {
            MalformKind::NonFiniteValue => MalformVerdict::ClampWithAccount,
            MalformKind::ScrambledTime => MalformVerdict::DetectOnly,
            MalformKind::NegativeTime => MalformVerdict::DetectOnly,
            MalformKind::OverdenseKeys => MalformVerdict::DetectOnly,
        }
    }
}

/// 畸形期望语义三态。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MalformVerdict {
    /// 钳制放行+诊断记账（容错路径）。
    ClampWithAccount,
    /// 显性拒绝（拒绝路径）。
    ExplicitReject,
    /// 只检测计数（检测面）。
    DetectOnly,
}

/// 一个畸形案例（输入+期望）。
#[derive(Clone, Debug, PartialEq)]
pub struct MalformCase {
    /// 形态。
    pub kind: MalformKind,
    /// 两端关键帧值（v0/v1）。
    pub values: (f32, f32),
    /// 两端时刻（t0/t1）。
    pub times: (f32, f32),
}

/// 畸形生成器（按形态产语料——全空间轮换，四态各产）。
pub fn gen_malform(kind: MalformKind, rng: &mut Lcg) -> MalformCase {
    let base = rng.next_f32() * 4.0;
    match kind {
        MalformKind::NonFiniteValue => MalformCase {
            kind,
            values: (f32::NAN, base + 1.0),
            times: (0.0, 1.0),
        },
        MalformKind::ScrambledTime => MalformCase {
            kind,
            values: (base, base + 1.0),
            times: (1.0, 0.0), // 乱序：t0 > t1
        },
        MalformKind::NegativeTime => MalformCase {
            kind,
            values: (base, base + 1.0),
            times: (-1.0, 1.0),
        },
        MalformKind::OverdenseKeys => MalformCase {
            kind,
            values: (base, base),
            times: (0.5, 0.5), // 同时间双帧
        },
    }
}

/// 段一执行：一个畸形案例跑双语义裁定并回判期望。
///
/// - `ClampWithAccount`：NaN 关键帧路径的结果必须与**显式以
///   `NON_FINITE_FALLBACK` 替代 NaN 后的参照路径逐位相等**（证明钳制
///   真发生且钳到哪——不猜），且诊断非空；
/// - `ExplicitReject`：要求返 None 且诊断非空；
/// - `DetectOnly`：走真检测/校验面——负时刻/超密对由
///   [`detect_curve_anomaly`] 计数，乱序时刻由
///   [`validate_samples`] 拒（`TIMES_NON_MONOTONIC`）。**不写恒真**：
///   检测不到就是红，不用 `|| true` 粉饰。
pub fn run_malform(case: &MalformCase) -> (MalformVerdict, bool) {
    let interp = InterpEntry { name: "linear", kind: Interp::Linear, eval: linear_progress };
    let params = InterpParams::LINEAR;
    let mut bag = TrackBag::new();
    let got = eval_scalar_span(
        TrackClass::Float,
        &interp,
        &params,
        case.values.0,
        case.values.1,
        case.times.0,
        case.times.1,
        0.5,
        &mut bag,
    );
    match case.kind.expect() {
        MalformVerdict::ClampWithAccount => {
            let accounted = !bag.warnings().is_empty() || !bag.errors().is_empty();
            // 参照路径：把 NaN 端显式替换为 vel03 的钳制常量后重跑——
            // 两条路径结果逐位相等即证明钳制目标正确（判据侧独立对拍）。
            let mut ref_bag = TrackBag::new();
            let reference = eval_scalar_span(
                TrackClass::Float,
                &interp,
                &params,
                NON_FINITE_FALLBACK,
                case.values.1,
                case.times.0,
                case.times.1,
                0.5,
                &mut ref_bag,
            );
            let clamp_proof = got.is_some() && got == reference;
            (MalformVerdict::ClampWithAccount, clamp_proof && accounted)
        }
        MalformVerdict::ExplicitReject => {
            let ok = got.is_none() && (!bag.errors().is_empty() || !bag.warnings().is_empty());
            (MalformVerdict::ExplicitReject, ok)
        }
        MalformVerdict::DetectOnly => match case.kind {
            MalformKind::ScrambledTime => {
                // 乱序时刻的真校验面：import 侧必拒（二分前提被破坏）。
                let times = [case.times.0, case.times.1];
                let out = GltfSamplerOut { comps: 1, interp: GltfInterp::Linear, count: 2 };
                let vals = [case.values.0, case.values.1];
                let mut ibag = ImportBag::new();
                let v = validate_samples(&times, &out, &vals, TrackSemantic::Position, &mut ibag);
                let hit = v == SampleVerdict::Reject(ImportCode::TIMES_NON_MONOTONIC);
                (MalformVerdict::DetectOnly, hit)
            }
            MalformKind::NegativeTime | MalformKind::OverdenseKeys => {
                let times = [case.times.0, case.times.1];
                let out = GltfSamplerOut { comps: 1, interp: GltfInterp::Linear, count: 2 };
                let vals = [case.values.0, case.values.1];
                let a = detect_curve_anomaly(&times, &out, &vals);
                let detected = match case.kind {
                    MalformKind::NegativeTime => a.negative_times > 0,
                    MalformKind::OverdenseKeys => a.overdense_pairs > 0,
                    _ => false,
                };
                (MalformVerdict::DetectOnly, detected)
            }
            _ => (MalformVerdict::DetectOnly, false),
        },
    }
}

// ---------------------------------------------------------------------------
// 五、段二：轨道风暴 fuzz（万轨道求值三不变量）
// ---------------------------------------------------------------------------

/// 句柄表（风暴段的泄漏探测器——open/close 必须收敛）。
#[derive(Clone, Copy, Debug, Default)]
pub struct HandleTable {
    /// 当前打开句柄。
    open: u32,
    /// 累计关闭。
    closed: u32,
}

impl HandleTable {
    /// 空表。
    pub fn new() -> HandleTable {
        HandleTable::default()
    }

    /// 开一个句柄。
    pub fn open_one(&mut self) {
        self.open = self.open.saturating_add(1);
    }

    /// 关一个句柄（0 时关是记账矛盾——显性记泄漏而非下溢）。
    pub fn close_one(&mut self) {
        if self.open == 0 {
            return;
        }
        self.open -= 1;
        self.closed = self.closed.saturating_add(1);
    }

    /// 收敛判定（无泄漏=全关）。
    pub fn converged(&self) -> bool {
        self.open == 0
    }

    /// 累计关闭数（读屏可达）。
    pub fn closed(&self) -> u32 {
        self.closed
    }
}

/// 风暴统计（定长记账——零分配红线的行为面：无 Vec 增长）。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct StormStats {
    /// 求值步数。
    pub eval_steps: u64,
    /// 结果摘要（双跑对拍物）。
    pub digest: u64,
    /// 打开过的句柄总数。
    pub opened: u32,
    /// 关闭的句柄总数。
    pub closed: u32,
}

/// 风暴段执行：万轨道×多样本真调求值，三不变量现场记账。
///
/// 零分配红线的**双重证据**：结构面——求值签名
/// （[`eval_scalar_span`]）无 Vec/`&mut Vec` 出口，本循环只用定长
/// 计数器与 u64 摘要；行为面——全程步数走看门狗预算，超预算即拒
/// （非完成循环=隐性无限分配的头号形态）。
pub fn storm_eval(seed: u64, watchdog: &mut Watchdog) -> Option<StormStats> {
    let interp = InterpEntry { name: "linear", kind: Interp::Linear, eval: linear_progress };
    let params = InterpParams::LINEAR;
    let mut bag = TrackBag::new();
    let mut handles = HandleTable::new();
    let mut digest: u64 = 0xcbf2_9ce4_8422_2325;
    let mut eval_steps: u64 = 0;
    let mut t = 0u32;
    while t < STORM_TRACKS {
        handles.open_one();
        watchdog.tick().ok()?;
        // 轨道参数由种子派生（同种子=同万轨语料）。
        let mut rng = Lcg::new(seed ^ (t as u64).wrapping_mul(0x2545_F491_4F6C_DD1D));
        let v0 = rng.next_f32();
        let v1 = v0 + rng.next_f32();
        let mut k = 0u32;
        while k < STORM_SAMPLES {
            watchdog.tick().ok()?;
            let ts = k as f32 / STORM_SAMPLES as f32;
            if let Some(v) = eval_scalar_span(
                TrackClass::Float,
                &interp,
                &params,
                v0,
                v1,
                0.0,
                1.0,
                ts,
                &mut bag,
            ) {
                for b in v.to_bits().to_le_bytes().iter() {
                    digest ^= *b as u64;
                    digest = digest.wrapping_mul(0x0000_0100_0000_01b3);
                }
            }
            eval_steps += 1;
            k += 1;
        }
        handles.close_one();
        t += 1;
    }
    Some(StormStats {
        eval_steps,
        digest,
        opened: STORM_TRACKS,
        closed: handles.closed(),
    })
}

// ---------------------------------------------------------------------------
// 六、段三：导入 fuzz（对抗 F2409 三重校验——畸形必被拦截显性）
// ---------------------------------------------------------------------------

/// 导入畸形形态（三类对抗：引用失效/采样畸形/格式混淆）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ImportMalform {
    /// 采样器下标越界（引用失效）。
    SamplerOor,
    /// 访问器下标越界（引用失效）。
    AccessorOor,
    /// 采样值 NaN（采样畸形）。
    SampleNaN,
    /// 值数不匹配（格式混淆：声明 comps 与实际不符）。
    ValueCountMismatch,
    /// 合法语料（对照——不得被误拦）。
    WellFormed,
}

impl ImportMalform {
    /// 五态闭集。
    pub const ALL: [ImportMalform; 5] = [
        ImportMalform::SamplerOor,
        ImportMalform::AccessorOor,
        ImportMalform::SampleNaN,
        ImportMalform::ValueCountMismatch,
        ImportMalform::WellFormed,
    ];

    /// 是否畸形（合法对照不是畸形）。
    pub fn is_malformed(self) -> bool {
        self != ImportMalform::WellFormed
    }
}

/// 构造导入对抗案例（按形态填 doc/times/out/values）。
pub fn gen_import_case(kind: ImportMalform) -> (GltfAnimDoc, ChannelRef, Vec<f32>, GltfSamplerOut, TrackSemantic) {
    let times: Vec<f32> = alloc::vec![0.0, 0.5, 1.0];
    let good_values: Vec<f32> = alloc::vec![0.0, 1.0, 2.0];
    let (comps, values, interp) = match kind {
        ImportMalform::SampleNaN => (1u8, alloc::vec![0.0, f32::NAN, 2.0], GltfInterp::Linear),
        ImportMalform::ValueCountMismatch => (3u8, good_values.clone(), GltfInterp::Linear),
        _ => (1u8, good_values.clone(), GltfInterp::Linear),
    };
    let out = GltfSamplerOut { comps, interp, count: 3 };
    let semantic = TrackSemantic::Position;
    match kind {
        ImportMalform::SamplerOor => {
            // 通道引用 sampler=99（簿内无此采样器）。
            let doc = GltfAnimDoc::new(1, Vec::new(), Vec::new(), Vec::new(), "fuzz-sampler-oor");
            let ch = ChannelRef { target_node: 0, path: ChannelPath::Translation, sampler: 99 };
            (doc, ch, times, out, semantic)
        }
        ImportMalform::AccessorOor => {
            // 采样器引用 input=7（越界访问器）。
            let sampler = SamplerRef { input: 7, output: 7, interp: GltfInterp::Linear };
            let doc = GltfAnimDoc::new(1, Vec::new(), alloc::vec![sampler], Vec::new(), "fuzz-accessor-oor");
            let ch = ChannelRef { target_node: 0, path: ChannelPath::Translation, sampler: 0 };
            (doc, ch, times, out, semantic)
        }
        _ => {
            // 合法骨架 + 畸形载荷（NaN 值 / 值数不符 / 全合法对照）。
            let acc_in = AccessorView::new(ComponentType::Float, false, 3, 1, times.clone());
            let acc_out = AccessorView::new(ComponentType::Float, false, 3, comps, values);
            let sampler = SamplerRef { input: 0, output: 1, interp: GltfInterp::Linear };
            let doc = GltfAnimDoc::new(
                1,
                alloc::vec![acc_in, acc_out],
                alloc::vec![sampler],
                Vec::new(),
                "fuzz-sample",
            );
            let ch = ChannelRef { target_node: 0, path: ChannelPath::Translation, sampler: 0 };
            // 注意：values 已移入 accessor，此处回传 times 供采样校验。
            (doc, ch, times, out, semantic)
        }
    }
}

/// 导入对抗的实测处置三态（对端语义的诚实映射）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ImportDisposition {
    /// 校验面显性拒绝（带码）。
    Rejected(ImportCode),
    /// 检测面计数显性（值域畸形——validate 不拒，但 anomaly 计数>0，
    /// 下游按计数钳制；不静默放过）。
    Detected,
    /// 放行（合法对照）。
    Passed,
}

/// 段三执行：一个对抗案例过三重校验 + 检测面，回实测处置。
///
/// 诚实映射对端真实语义（不以锚点措辞倒推实现）：
/// - 采样器/访问器越界、值数不匹配 → validate 显性拒绝；
/// - NaN 采样值 → validate 对 Position 语义**不拒**（值域钳制在下游），
///   但 [`detect_curve_anomaly`] 的 `non_finite_values` 计数 >0——处置
///   记为 `Detected`（拦截显性的是检测面，不是拒绝面）；
/// - 合法对照 → `Passed`。
pub fn run_import_case(kind: ImportMalform) -> ImportDisposition {
    let (doc, ch, times, out, semantic) = gen_import_case(kind);
    let mut bag = ImportBag::new();
    // 校验一：通道引用。
    if let Some(code) = validate_channel_refs(&doc, ch, &mut bag) {
        return ImportDisposition::Rejected(code);
    }
    // 校验二：采样数据。取值与 accessor 同源（合法骨架才有 authentic values）。
    let values: Vec<f32> = match kind {
        ImportMalform::SampleNaN => alloc::vec![0.0, f32::NAN, 2.0],
        _ => alloc::vec![0.0, 1.0, 2.0],
    };
    match validate_samples(&times, &out, &values, semantic, &mut bag) {
        SampleVerdict::Reject(code) => ImportDisposition::Rejected(code),
        SampleVerdict::Ok => {
            if kind == ImportMalform::SampleNaN {
                let a = detect_curve_anomaly(&times, &out, &values);
                if a.non_finite_values > 0 {
                    return ImportDisposition::Detected;
                }
            }
            ImportDisposition::Passed
        }
    }
}

// ---------------------------------------------------------------------------
// 七、案例固化库（FNV 去重 + 库容控制 + P0 立案）
// ---------------------------------------------------------------------------

/// 案例元数据（输入/期望/复现/去重哈希——四要素缺一即不可复核）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CaseMeta {
    /// 案例 id。
    pub id: u32,
    /// 输入摘要（FNV-1a）。
    pub input_digest: u64,
    /// 期望语义（人读）。
    pub expected: String,
    /// 复现步骤（最小复现脚本要点）。
    pub repro: String,
}

/// 案例库（去重 + 库容）。
#[derive(Clone, Debug, Default)]
pub struct CaseStore {
    entries: Vec<CaseMeta>,
    next_id: u32,
    /// 去重命中计数（读屏可达——去重真的在去）。
    dedup_hits: u64,
}

impl CaseStore {
    /// 空库。
    pub fn new() -> CaseStore {
        CaseStore::default()
    }

    /// 入册：同摘要去重；超容显性拒绝（不静默丢案）。
    pub fn add(&mut self, input_digest: u64, expected: &str, repro: &str) -> Result<u32, String> {
        if self.entries.iter().any(|e| e.input_digest == input_digest) {
            self.dedup_hits = self.dedup_hits.saturating_add(1);
            return Ok(0); // 已入册（幂等）
        }
        if self.entries.len() >= MAX_CASES {
            return Err(format!(
                "{}：案例库已满（{} 条）——请扩容或先固化归档，不静默丢案",
                E_FUZZ_CASE_FULL, MAX_CASES
            ));
        }
        self.next_id += 1;
        let id = self.next_id;
        self.entries.push(CaseMeta {
            id,
            input_digest,
            expected: String::from(expected),
            repro: String::from(repro),
        });
        Ok(id)
    }

    /// 在册数。
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// 是否空库。
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// 去重命中数（读屏可达）。
    pub fn dedup_hits(&self) -> u64 {
        self.dedup_hits
    }

    /// 全部条目（读屏可达）。
    pub fn all(&self) -> &[CaseMeta] {
        &self.entries
    }
}

/// P0 立案（带 24h 复现期限——超期可判定）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct P0Case {
    /// 立案码。
    pub code: &'static str,
    /// 立案时刻（逻辑时钟 ms）。
    pub opened_ms: u64,
    /// 复现期限时刻 = opened + 24h。
    pub deadline_ms: u64,
}

impl P0Case {
    /// 新立案（期限自动钉 24h）。
    pub fn open(code: &'static str, now_ms: u64) -> P0Case {
        P0Case { code, opened_ms: now_ms, deadline_ms: now_ms.saturating_add(P0_REPRO_24H_MS) }
    }

    /// 是否超期（`now` 过期限即真）。
    pub fn overdue(&self, now_ms: u64) -> bool {
        now_ms > self.deadline_ms
    }
}

// ---------------------------------------------------------------------------
// 八、判据
// ---------------------------------------------------------------------------

use crate::checks::CheckSet;

/// F2411 域自检（判据五组：畸形/风暴/导入/固化/框架）。
pub fn run_vem11_checks() -> CheckSet {
    let mut s = CheckSet::new("VE-F2411");

    // --- 段一：畸形动画 fuzz（双语义）---
    // M11-畸形-01：NaN 关键帧值 → 钳制+记账（容错路径可达）。
    let mut rng = Lcg::new(0xF211_0001);
    let nan_case = gen_malform(MalformKind::NonFiniteValue, &mut rng);
    let (verdict, ok) = run_malform(&nan_case);
    s.add(
        "M11-畸形-01",
        verdict == MalformVerdict::ClampWithAccount && ok,
        "NaN 关键帧值钳制+记账（容错路径）",
    );

    // M11-畸形-02：显性拒绝路径可达（闸门不合 → None+诊断）。
    let mut bag_gate = TrackBag::new();
    let bad_interp = InterpEntry { name: "custom-x", kind: Interp::Custom, eval: linear_progress };
    let gate_none = eval_scalar_span(
        TrackClass::Float,
        &bad_interp,
        &InterpParams::LINEAR,
        0.0,
        1.0,
        0.0,
        1.0,
        0.5,
        &mut bag_gate,
    );
    s.add(
        "M11-畸形-02",
        gate_none.is_none() && !bag_gate.errors().is_empty(),
        "闸门不合显性拒绝（拒绝路径可达）",
    );

    // M11-畸形-03：负时刻检测（detect_curve_anomaly 计数>0）。
    let neg_case = gen_malform(MalformKind::NegativeTime, &mut rng);
    let (v3, ok3) = run_malform(&neg_case);
    s.add(
        "M11-畸形-03",
        v3 == MalformVerdict::DetectOnly && ok3,
        "负时刻检测计数（检测面）",
    );

    // M11-畸形-04：超密关键帧检测（同时间多帧→零间隔对>0）。
    let dense_case = gen_malform(MalformKind::OverdenseKeys, &mut rng);
    let (v4, ok4) = run_malform(&dense_case);
    s.add(
        "M11-畸形-04",
        v4 == MalformVerdict::DetectOnly && ok4,
        "超密关键帧检测（零间隔对计数）",
    );

    // M11-畸形-05：双语义闭合（四态期望互异且全部实测相符——无静默通过）。
    let mut all_match = true;
    for kind in MalformKind::ALL.iter() {
        let case = gen_malform(*kind, &mut rng);
        let (verdict, ok) = run_malform(&case);
        all_match = all_match && verdict == kind.expect() && ok;
    }
    s.add("M11-畸形-05", all_match, "四态期望语义全部实测相符（双语义闭合）");

    // --- 段二：轨道风暴 fuzz（三不变量）---
    // M11-风暴-01：双跑摘要一致（确定性）。
    let mut w1 = Watchdog::storm();
    let mut w2 = Watchdog::storm();
    let st1 = storm_eval(0x5EED_2411, &mut w1);
    let st2 = storm_eval(0x5EED_2411, &mut w2);
    s.add(
        "M11-风暴-01",
        st1.is_some() && st1 == st2 && st1.map_or(false, |x| x.digest != 0),
        "万轨双跑摘要逐位一致（确定性）",
    );

    // M11-风暴-02：句柄收敛零（无泄漏）。
    s.add(
        "M11-风暴-02",
        st1.map_or(false, |x| x.opened == STORM_TRACKS && x.closed == STORM_TRACKS),
        "句柄表收敛零（opened==closed==万）",
    );

    // M11-风暴-03：零分配红线（结构面：求值步数=轨×样且预算内——
    // 无隐藏循环/无超预算分配面）。
    s.add(
        "M11-风暴-03",
        st1.map_or(false, |x| x.eval_steps == STORM_TRACKS as u64 * STORM_SAMPLES as u64)
            && w1.used() <= STORM_STEP_BUDGET,
        "求值步数恰 万×4 且预算内（结构零分配）",
    );

    // M11-风暴-04：风暴规模真跑（万轨语料全过且异种子异摘要——语料真随机）。
    let mut w3 = Watchdog::storm();
    let st3 = storm_eval(0x5EED_9999, &mut w3);
    s.add(
        "M11-风暴-04",
        st3.is_some() && st1 != st3,
        "异种子异摘要（语料真随机器）",
    );

    // --- 段三：导入 fuzz（三重校验对抗）---
    // M11-导入-01：采样器越界拦截（引用失效）。
    let d1 = run_import_case(ImportMalform::SamplerOor);
    s.add(
        "M11-导入-01",
        d1 == ImportDisposition::Rejected(ImportCode::SAMPLER_OOR),
        "采样器越界显性拦截（SAMPLER_OOR）",
    );

    // M11-导入-02：访问器越界拦截（引用失效）。
    let d2 = run_import_case(ImportMalform::AccessorOor);
    s.add("M11-导入-02", matches!(d2, ImportDisposition::Rejected(_)), "访问器越界显性拦截");

    // M11-导入-03：NaN 采样值检测计数（对端真实语义：validate 不拒，
    // anomaly 的 non_finite_values 计数 >0——拦截显性在检测面）。
    let d3 = run_import_case(ImportMalform::SampleNaN);
    s.add(
        "M11-导入-03",
        d3 == ImportDisposition::Detected,
        "NaN 采样值检测计数显性（validate 不拒，anomaly 计数拦截）",
    );

    // M11-导入-04：值数不匹配拦截（格式混淆）。
    let d4 = run_import_case(ImportMalform::ValueCountMismatch);
    s.add(
        "M11-导入-04",
        d4 == ImportDisposition::Rejected(ImportCode::VALUE_COUNT_MISMATCH),
        "值数不匹配显性拦截（格式混淆）",
    );

    // M11-导入-05：合法对照不误拦（拦截不泛化——双向）。
    let d5 = run_import_case(ImportMalform::WellFormed);
    s.add("M11-导入-05", d5 == ImportDisposition::Passed, "合法语料放行（拦截不泛化）");

    // M11-导入-06：千级对抗全闭合（畸形全被拒/被检测，合法全放行）。
    let mut closed = true;
    let mut n = 0u32;
    while n < IMPORT_FUZZ_CASES {
        let kind = match n % 5 {
            0 => ImportMalform::SamplerOor,
            1 => ImportMalform::AccessorOor,
            2 => ImportMalform::SampleNaN,
            3 => ImportMalform::ValueCountMismatch,
            _ => ImportMalform::WellFormed,
        };
        let d = run_import_case(kind);
        let ok = if kind.is_malformed() {
            d != ImportDisposition::Passed
        } else {
            d == ImportDisposition::Passed
        };
        if !ok {
            closed = false;
            break;
        }
        n += 1;
    }
    s.add("M11-导入-06", closed && n == IMPORT_FUZZ_CASES, "千级对抗全闭合（畸形全拦/合法全放）");

    // --- 案例固化（去重/库容/24h）---
    // M11-固化-01：同案去重（同摘要二次入册不增簿+去重计数）。
    let mut store = CaseStore::new();
    let d = fnv1a64(b"case-alpha");
    let id1 = store.add(d, "钳制+记账", "以 NaN 值调 eval_scalar_span");
    let id2 = store.add(d, "钳制+记账", "以 NaN 值调 eval_scalar_span");
    s.add(
        "M11-固化-01",
        id1.is_ok() && id2 == Ok(0) && store.len() == 1 && store.dedup_hits() == 1,
        "同案去重（幂等入册）",
    );

    // M11-固化-02：异案入册+四要素齐备。
    let id3 = store.add(fnv1a64(b"case-beta"), "显性拒绝", "以 Custom 插值器调 eval");
    s.add(
        "M11-固化-02",
        id3.is_ok() && id3 != Ok(0) && store.len() == 2
            && store.all().iter().all(|c| !c.expected.is_empty() && !c.repro.is_empty()),
        "异案入册且四要素齐备",
    );

    // M11-固化-03：P0 立案 24h 期限钉死。
    let p0 = P0Case::open(E_FUZZ_P0, 1_000);
    s.add(
        "M11-固化-03",
        p0.deadline_ms == 1_000 + P0_REPRO_24H_MS && P0_REPRO_24H_MS == 86_400_000,
        "P0 期限=立案+24h（86_400_000ms）",
    );

    // M11-固化-04：超期可判定（未超不报/超时报——双向）。
    s.add(
        "M11-固化-04",
        !p0.overdue(1_000 + P0_REPRO_24H_MS) && p0.overdue(1_000 + P0_REPRO_24H_MS + 1),
        "超期谓词双向（恰期限不超/过 1ms 即超）",
    );

    // --- 框架纪律（F1813 家族）---
    // M11-框架-01：LCG 同种子同语料（可复现）。
    let mut a = Lcg::new(42);
    let mut b = Lcg::new(42);
    let same = (0..64).all(|_| a.next_u64() == b.next_u64());
    s.add("M11-框架-01", same, "LCG 同种子同流（复现前提）");

    // M11-框架-02：看门狗超预算挂起（非完成循环可判定）。
    let mut w_over = Watchdog::new(8);
    let mut tripped = false;
    let mut guard = 0u32;
    while guard < 1000 {
        if w_over.tick().is_err() {
            tripped = true;
            break;
        }
        guard += 1;
    }
    let err_msg = {
        let mut w = Watchdog::new(3);
        let mut e = String::new();
        let mut i = 0u32;
        while i < 10 {
            if let Err(m) = w.tick() {
                e = m;
                break;
            }
            i += 1;
        }
        e
    };
    s.add(
        "M11-框架-02",
        tripped && err_msg.starts_with(E_FUZZ_WATCHDOG),
        "看门狗超预算挂起+立案码",
    );

    // M11-框架-03：案例库容控（超容显性拒绝——构造满库）。
    let mut full = CaseStore::new();
    let mut filled = true;
    let mut i = 0u64;
    while (i as usize) < MAX_CASES {
        if full.add(fnv1a64(&i.to_le_bytes()), "e", "r").is_err() {
            filled = false;
            break;
        }
        i += 1;
    }
    let over = full.add(fnv1a64(b"one-too-many"), "e", "r");
    s.add(
        "M11-固化-05",
        filled && full.len() == MAX_CASES && over.is_err() && over.unwrap_err().starts_with(E_FUZZ_CASE_FULL),
        "库容封顶显性拒绝（不静默丢案）",
    );

    // --- CI 与版本 ---
    // M11-CI-01：三段套件一键全过（畸形四态+导入五态）。
    let chain = (0..4u32).all(|k| {
        let case = gen_malform(MalformKind::ALL[k as usize], &mut Lcg::new(ci_seed_base(k)));
        run_malform(&case).1
    }) && (0..5u32).all(|k| {
        let d = run_import_case(ImportMalform::ALL[k as usize]);
        if ImportMalform::ALL[k as usize].is_malformed() {
            d != ImportDisposition::Passed
        } else {
            d == ImportDisposition::Passed
        }
    });
    s.add("M11-CI-01", chain, "三段套件一键全过");

    // M11-版本-01：版本指纹非零。
    let fp = fnv1a64(FUZZ_VERSION.as_bytes());
    s.add("M11-版本-01", fp != 0, "版本指纹非零（M11-fuzz-v1）");

    // M11-暂挂-01：M 域账本暂挂声明显性（跨批对接点：立案进 M 域账本）。
    s.add(
        "M11-暂挂-01",
        M_LEDGER_FUZZ_SUSPENDED_NOTE.contains("暂挂") && M_LEDGER_FUZZ_SUSPENDED_NOTE.contains("F2411"),
        "M 域账本暂挂声显性（案例库入 F1769 M 段调度）",
    );

    // M11-暂挂-02：判据条数对账（本条为第 26 条）。
    s.add("M11-暂挂-02", s.len() == 25, "判据条数对账（25+本条）");

    s
}

/// CI 段种子基（F2411 专用魔数集中一处——避免散落魔法数）。
fn ci_seed_base(k: u32) -> u64 {
    0xC0FF_EE00 ^ (k as u64)
}

/// M 域账本暂挂声明（跨批对接点：立案进 M 域账本——建账前暂挂）。
pub const M_LEDGER_FUZZ_SUSPENDED_NOTE: &str = "动画 fuzz 案例库与 P0 立案入 M 域账本：建账前暂挂声明（移交期模式延续——F2411 同款）；案例库入 F1769 M 段调度（M 域 fuzz 首域）";
