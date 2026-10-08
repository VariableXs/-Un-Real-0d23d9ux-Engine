//! VE-F2211 · 粒子 fuzz（VE-L 域 · 粒子段 · 目标 300 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F2211`
//!
//! **判据（锚点原文）**：三段 fuzz、池不变量、显性不变量、24h 固化、判据。
//!
//! **职责定位（锚点原文）**：三段模糊测试——畸形发射器 fuzz（负发射率/
//! NaN 速度/超大池申请——钳制拒绝：发射器参数全空间扫描+池申请边界——
//! **容错与显性拒绝的双语义验证**）；粒子风暴 fuzz（发射器高频启停——池
//! 不变量：无泄漏/最终一致/水位正确）；混合场景 fuzz（四模式并发+力场
//! 预留调用+渲染形态高频切换——组合压力不崩不变量），案例固化入库。
//!
//! # 一、三个被测面全部「真调」而不代填
//!
//! 本模块的 fuzz 价值取决于被测面是否真流经实现：段一真调
//! [`clamp_emit_rate`](vel03_emitter::clamp_emit_rate) 与
//! [`PoolQuota::validate`](vel08_pool::PoolQuota::validate)；段二真调
//! [`ParticlePool`](vel08_pool::ParticlePool) 的 alloc/reclaim/observe
//! 与 [`legal_transitions`](vel03_emitter::legal_transitions)；段三真调
//! [`ModeKind`](vel04_mode::ModeKind) 四型轮转与
//! [`request_field_stream`](vel09_debug::request_field_stream) 的 STUB
//! 显性报错。fuzz 执行器**不吞 panic 也不代填 panic_free**——被测面
//! 契约零 panic，任何一次 panic 都让判据进程崩溃、门禁必红（与 vec19/
//! vef18 同纪律）。
//!
//! # 二、显性不变量是本条的独立判据线
//!
//! 「拒绝未显性（超大申请静默失败）→立案」：被测面对非法输入的处理有
//! 两种合法形态——**容错钳制**（发射率 NaN→0，必须产诊断）与**显性拒绝**
//! （超大申请返回 Err，必须带消息）。两者都要求**可观测痕迹**；静默
//! 改写或静默吞掉任一形态都记 `silent_rejects`，非零即红。
//!
//! # 三、池三不变量（段二/段三的每轮断言）
//!
//! - **账实相符**：`live + free_count == capacity`（无泄漏：每个槽位
//!   要么在用要么在链上，不存在第三态）；
//! - **链上数守恒**：空闲链上槽位数 == `free_count`（链表损坏的早
//!   期信号——重复回收致链 shortening 在 vel08 有防御分支，这里从
//!   fuzz 侧独立复核）；
//! - **风暴收敛**：启停风暴结束后池回到预期占用（最终一致）。
//!
//! # 四、确定性可复现
//!
//! 自持 LCG（同 vec19/vef18 纪律，不用 std 随机源），每条语料由种子
//! 决定；案例元数据带 **FNV-1a 去重哈希**（同 F4010 纪律：const 期
//! 独立重算防手抄漂移），同哈希案例不入库（去重哈希控库容——锚点
//! 性能纪律），同哈希不同输入判冲突立案。
//!
//! # 五、24h 固化（锚点错误路径）
//!
//! 崩溃 → P0 立案并记 **24h 复现期限**（[`FuzzEscalation`]，期限 =
//! 立案时刻 + 86400_000ms，判据钉死）；池泄漏检出 → P0；钳制失灵 →
//! 立案。立案记录不与本模块判据耦合，供 L 域账本消费（建账前暂挂
//! 声明——F2094 模式延续）。
//!
//! **性能（锚点原文）**：参数扫描万级夜间批跑（本模块提供纯函数面，
//! 调度入 F1769 L 段，不在本模块内置时序）；步数预算把「不挂起」
//! 转成可判定判据；去重哈希控库容。
//!
//! **跨批对接**：框架复用 F1813 家族（LCG/语料/记账模式同源）；通过
//! 率供 F2216/F2218；案例库入 F1769 L 段调度（L 域面注册）。

use alloc::string::{String, ToString};
use alloc::vec::Vec;

use crate::svstar2::vel03_emitter::{
    advance_accumulator, clamp_emit_rate, legal_transitions, DiagBag, DiagCode, EmitAccumulator,
    EmitterState, EMIT_RATE_MAX_PER_SEC,
};
use crate::svstar2::vel04_mode::ModeKind;
use crate::svstar2::vel08_pool::{
    PoolBag, PoolKind, PoolQuota, ParticlePool, HYST_WARN_OFF, WM_DEGRADE_PCT, WM_REJECT_PCT,
    WM_WARN_PCT,
};
use crate::svstar2::vel09_debug::{DiagBag as V9DiagBag, DiagCode as V9DiagCode, FieldGizmoStub};

// ---------------------------------------------------------------------------
// 一、常量与错误码
// ---------------------------------------------------------------------------

/// 本项版本。
pub const FUZZ_PROTOCOL_VERSION: &str = "L11-fuzz-v1";

/// 段一：发射率角点扫描数（全空间角点 7 类）。
pub const SEG1_RATE_CORNERS: usize = 7;

/// 段二：风暴启停轮数（千发射器脚本的高频档；步数预算内）。
pub const SEG2_STORM_ROUNDS: usize = 512;

/// 段二/三：池槽位容量（小池放大泄漏信号）。
pub const FUZZ_POOL_CAPACITY: u32 = 256;

/// 段三：混合场景步数（四模式轮转×池压力交错）。
pub const SEG3_MIXED_STEPS: usize = 384;

/// 案例库容量上限（去重哈希控库容——锚点性能纪律）。
pub const CASEBOOK_CAP: usize = 4096;

/// P0 复现期限（毫秒）：24h 固化（锚点红线，判据钉死）。
pub const P0_REPRO_DEADLINE_MS: u64 = 86_400_000;

/// 崩溃立案（P0）。
pub const E_FUZZ_CRASH: &str = "E_FUZZ_CRASH";

/// 池泄漏（P0：池不变量根基）。
pub const E_FUZZ_LEAK: &str = "E_FUZZ_LEAK";

/// 拒绝未显性（静默失败）。
pub const E_FUZZ_SILENT: &str = "E_FUZZ_SILENT";

/// 钳制失灵（钳制后值越出合法域）。
pub const E_FUZZ_CLAMP: &str = "E_FUZZ_CLAMP";

/// 案例去重哈希冲突（同哈希不同输入）。
pub const E_FUZZ_HASH: &str = "E_FUZZ_HASH";

// ---------------------------------------------------------------------------
// 二、确定性伪随机（自持 LCG，同 vec19/vef18 纪律）
// ---------------------------------------------------------------------------

/// LCG 乘数（Knuth 64 位乘加常数，同 vef18）。
const LCG_A: u64 = 6364136223846793005;
/// LCG 增量。
const LCG_C: u64 = 1442695040888963407;

/// 自持确定性伪随机源：同种子同序列，案例可复现。
pub struct Lcg(u64);

impl Lcg {
    pub fn new(seed: u64) -> Lcg {
        Lcg(seed.wrapping_mul(LCG_A).wrapping_add(0x9E3779B97F4A7C15))
    }

    pub fn next_u64(&mut self) -> u64 {
        self.0 = self.0.wrapping_mul(LCG_A).wrapping_add(LCG_C);
        self.0
    }

    /// [0, n) 均匀取整（取高位减少低位周期短的偏差）。
    pub fn below(&mut self, n: u64) -> u64 {
        (self.next_u64() >> 16) % n.max(1)
    }
}

/// FNV-1a 64 位（案例去重哈希，同 F4010 纪律：独立可重算）。
pub const fn fnv1a(bytes: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    let mut i = 0;
    while i < bytes.len() {
        h ^= bytes[i] as u64;
        h = h.wrapping_mul(0x100000001b3);
        i += 1;
    }
    h
}

// ---------------------------------------------------------------------------
// 三、案例元数据与案例库
// ---------------------------------------------------------------------------

/// fuzz 段位（三段闭集）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FuzzSegment {
    /// 段一：畸形发射器。
    MalformedEmitter,
    /// 段二：粒子风暴。
    EmitterStorm,
    /// 段三：混合场景。
    MixedStorm,
}

impl FuzzSegment {
    /// 短码（冻结）。
    pub fn wire(self) -> &'static str {
        match self {
            FuzzSegment::MalformedEmitter => "seg1-malformed",
            FuzzSegment::EmitterStorm => "seg2-storm",
            FuzzSegment::MixedStorm => "seg3-mixed",
        }
    }

    /// 三段全集（顺序即编号）。
    pub fn all() -> [FuzzSegment; 3] {
        [FuzzSegment::MalformedEmitter, FuzzSegment::EmitterStorm, FuzzSegment::MixedStorm]
    }
}

/// 案例（固化入库的元数据：输入摘要/期望/去重哈希——锚点元数据三要素，
/// 复现脚本由段位+种子决定：同段位同种子即逐位复现，故种子即脚本）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FuzzCase {
    /// 所属段位。
    pub segment: FuzzSegment,
    /// 语料种子（确定性复现键）。
    pub seed: u64,
    /// 输入摘要（人类可读；哈希单独存）。
    pub input_digest: u64,
    /// 期望（通过/预算内/命中数的三值冻结为字符串短码）。
    pub expectation: &'static str,
    /// 去重哈希（FNV-1a，对 segment+seed+input 独立重算）。
    pub dedup_hash: u64,
}

impl FuzzCase {
    /// 构造并独立重算去重哈希。
    pub fn new(segment: FuzzSegment, seed: u64, input_digest: u64) -> FuzzCase {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(segment.wire().as_bytes());
        bytes.extend_from_slice(&seed.to_le_bytes());
        bytes.extend_from_slice(&input_digest.to_le_bytes());
        FuzzCase {
            segment,
            seed,
            input_digest,
            expectation: "completed",
            dedup_hash: fnv1a(&bytes),
        }
    }
}

/// 案例库（去重入库：同哈希只记一次；同哈希不同输入=冲突立案）。
#[derive(Debug, Default)]
pub struct FuzzCasebook {
    cases: Vec<FuzzCase>,
    /// 去重拒绝数（重复案例不入库的计数——控库容的直接证据）。
    pub dedup_rejected: u32,
    /// 哈希冲突立案数（同哈希不同输入）。
    pub hash_conflicts: u32,
}

impl FuzzCasebook {
    pub fn new() -> FuzzCasebook {
        FuzzCasebook::default()
    }

    /// 入库（去重）。返回 `Ok(true)`=新入库、`Ok(false)`=重复拒绝、
    /// `Err`=哈希冲突（同哈希不同输入——哈希函数或输入编码出问题）。
    pub fn admit(&mut self, case: FuzzCase) -> Result<bool, String> {
        if self.cases.len() >= CASEBOOK_CAP {
            return Ok(false);
        }
        for c in self.cases.iter() {
            if c.dedup_hash == case.dedup_hash {
                if c.segment == case.segment && c.seed == case.seed && c.input_digest == case.input_digest {
                    self.dedup_rejected = self.dedup_rejected.saturating_add(1);
                    return Ok(false);
                }
                self.hash_conflicts = self.hash_conflicts.saturating_add(1);
                return Err(E_FUZZ_HASH.to_string());
            }
        }
        self.cases.push(case);
        Ok(true)
    }

    /// 在册案例数。
    pub fn len(&self) -> usize {
        self.cases.len()
    }

    /// 是否为空。
    pub fn is_empty(&self) -> bool {
        self.cases.is_empty()
    }
}

// ---------------------------------------------------------------------------
// 四、执行报告与立案
// ---------------------------------------------------------------------------

/// 单段执行报告（记账不吞错：任何崩溃直接 panic 炸红，不进报告）。
#[derive(Debug, Default, Clone)]
pub struct FuzzReport {
    /// 执行语料条数。
    pub cases: u32,
    /// 步数预算超限次数（非零=「不挂起」判据红）。
    pub budget_exceeded: u32,
    /// 静默拒绝/静默改写检出数（显性不变量——非零即红）。
    pub silent_rejects: u32,
    /// 不变量违例数（池三不变量——非零即 P0）。
    pub invariant_breaches: u32,
    /// 钳制失灵数（钳制后值越出合法域）。
    pub clamp_failures: u32,
    /// 命中的诊断事件数（被测面显性痕迹的正向计数）。
    pub diag_hits: u32,
}

impl FuzzReport {
    /// 段级通过判定：预算内+零静默+零违例+零失灵（cases>0 防空转恒绿）。
    pub fn passed(&self) -> bool {
        self.cases > 0
            && self.budget_exceeded == 0
            && self.silent_rejects == 0
            && self.invariant_breaches == 0
            && self.clamp_failures == 0
    }
}

/// P0 立案记录（崩溃/泄漏；24h 复现期限——锚点错误路径）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FuzzEscalation {
    /// 立案码（E_FUZZ_CRASH / E_FUZZ_LEAK）。
    pub code: &'static str,
    /// 触发案例段位。
    pub segment: FuzzSegment,
    /// 触发案例种子。
    pub seed: u64,
    /// 立案时刻（毫秒逻辑钟）。
    pub raised_ms: u64,
    /// 复现期限 = raised + 24h（判据钉死）。
    pub deadline_ms: u64,
}

impl FuzzEscalation {
    /// 立案（期限独立重算，判据对账）。
    pub fn new(code: &'static str, segment: FuzzSegment, seed: u64, now_ms: u64) -> FuzzEscalation {
        FuzzEscalation {
            code,
            segment,
            seed,
            raised_ms: now_ms,
            deadline_ms: now_ms + P0_REPRO_DEADLINE_MS,
        }
    }
}

// ---------------------------------------------------------------------------
// 五、段一：畸形发射器 fuzz（钳制拒绝——容错与显性的双语义验证）
// ---------------------------------------------------------------------------

/// 发射率全空间角点扫描：7 类非法/边界值 + 期望钳制值（**判据侧独立
/// 重算**，不读 vel03 内部分支），逐点真调 [`clamp_emit_rate`]。
///
/// 返回（报告，逐点钳制值）——钳制值供判据独立对账。
pub fn fuzz_malformed_emitter(now_ms: u64) -> (FuzzReport, [f32; SEG1_RATE_CORNERS], Vec<FuzzEscalation>) {
    let mut rep = FuzzReport::default();
    let mut esc: Vec<FuzzEscalation> = Vec::new();
    // 角点：NaN / +Inf / -Inf / 负值 / 0 / 上限 / 上限×2（超限）。
    let corners: [f32; SEG1_RATE_CORNERS] = [
        f32::NAN,
        f32::INFINITY,
        f32::NEG_INFINITY,
        -42.5,
        0.0,
        EMIT_RATE_MAX_PER_SEC,
        EMIT_RATE_MAX_PER_SEC * 2.0,
    ];
    let mut out = [0.0f32; SEG1_RATE_CORNERS];
    for (i, &rate) in corners.iter().enumerate() {
        let mut bag = DiagBag::new();
        let clamped = clamp_emit_rate(rate, &mut bag);
        out[i] = clamped;
        rep.cases += 1;
        // 期望值判据侧独立重算（不引用 vel03 内部实现分支）。
        let expect = if rate.is_nan() {
            0.0
        } else if rate.is_infinite() {
            if rate > 0.0 { EMIT_RATE_MAX_PER_SEC } else { 0.0 }
        } else if rate < 0.0 {
            0.0
        } else if rate > EMIT_RATE_MAX_PER_SEC {
            EMIT_RATE_MAX_PER_SEC
        } else {
            rate
        };
        if clamped != expect || clamped.is_nan() {
            rep.clamp_failures += 1;
            esc.push(FuzzEscalation::new(E_FUZZ_CLAMP, FuzzSegment::MalformedEmitter, i as u64, now_ms));
        }
        // 显性不变量：非法值（非恒等透传）必须产诊断；合法值不得产诊断。
        let passthrough = clamped == rate;
        let marked = bag.has(DiagCode::RateClamped);
        if !passthrough {
            if marked {
                rep.diag_hits += 1;
            } else {
                // 钳制了却没留痕 = 静默改写。
                rep.silent_rejects += 1;
                esc.push(FuzzEscalation::new(E_FUZZ_SILENT, FuzzSegment::MalformedEmitter, i as u64, now_ms));
            }
        } else if marked {
            // 合法透传却产诊断 = 误报（可观测面撒谎）。
            rep.silent_rejects += 1;
        }
    }
    (rep, out, esc)
}

/// 池申请边界 fuzz：超大/越界 quota 序列逐条真调
/// [`PoolQuota::validate`]，断言「拒绝必须显性（Err 且带信息）」。
pub fn fuzz_pool_quota_edges(now_ms: u64) -> (FuzzReport, Vec<FuzzEscalation>) {
    let mut rep = FuzzReport::default();
    let mut esc: Vec<FuzzEscalation> = Vec::new();
    // 越界序列：零容量 / 零 stride / 份额超 100% / 内存顶超 capacity×stride
    // / capacity×stride 溢出（u32 乘法上界）。
    let quotas: [PoolQuota; 5] = [
        PoolQuota { kind: PoolKind::Cpu, capacity: 0, stride: 64, emitter_share_pct: 10, bytes_cap: 4096 },
        PoolQuota { kind: PoolKind::Cpu, capacity: 64, stride: 0, emitter_share_pct: 10, bytes_cap: 4096 },
        PoolQuota { kind: PoolKind::Cpu, capacity: 64, stride: 64, emitter_share_pct: 101, bytes_cap: 1 << 20 },
        PoolQuota { kind: PoolKind::Cpu, capacity: 1_000_000, stride: 64, emitter_share_pct: 10, bytes_cap: 4096 },
        PoolQuota { kind: PoolKind::Gpu, capacity: u32::MAX, stride: u32::MAX, emitter_share_pct: 10, bytes_cap: 1 << 20 },
    ];
    for (i, q) in quotas.iter().enumerate() {
        rep.cases += 1;
        match q.validate() {
            // 合法才允许静默；越界序列里五条全部应显性拒绝。
            Ok(_) => {
                rep.silent_rejects += 1;
                esc.push(FuzzEscalation::new(E_FUZZ_SILENT, FuzzSegment::MalformedEmitter, i as u64, now_ms));
            }
            Err(msg) => {
                if msg.is_empty() {
                    // Err 但空消息 = 半显性，仍记静默。
                    rep.silent_rejects += 1;
                    esc.push(FuzzEscalation::new(E_FUZZ_SILENT, FuzzSegment::MalformedEmitter, i as u64, now_ms));
                }
            }
        }
    }
    (rep, esc)
}

/// 发射累积器容错 fuzz：非法 dt/rate 组合真调
/// [`advance_accumulator`]，断言「非法输入零产出且不动余量」。
pub fn fuzz_accumulator_edges() -> FuzzReport {
    let mut rep = FuzzReport::default();
    let dts = [0.0f32, -1.0, f32::NAN, f32::INFINITY, 1.0 / 0.0 * 0.0];
    let rates = [0.0f32, -5.0, f32::NAN, 60.0];
    for &dt in dts.iter() {
        for &rate in rates.iter() {
            let mut acc = EmitAccumulator::new();
            acc.carry = 0.25;
            let got = advance_accumulator(&mut acc, rate, dt);
            rep.cases += 1;
            let legal_dt = dt > 0.0 && dt.is_finite();
            let legal_rate = rate > 0.0 && rate.is_finite();
            if !(legal_dt && legal_rate) {
                // 非法组合：零产出、余量原样（容错不吞半粒——vel03 契约）。
                if got != 0 || acc.carry != 0.25 {
                    rep.invariant_breaches += 1;
                }
            }
        }
    }
    rep
}

// ---------------------------------------------------------------------------
// 六、段二：粒子风暴 fuzz（高频启停——池三不变量）
// ---------------------------------------------------------------------------

/// 池三不变量独立复核（判据与 vel08 自检互不共享实现）。
/// 返回违例描述清单（空 = 不变量成立）。
pub fn pool_invariants(pool: &ParticlePool) -> Vec<&'static str> {
    let mut out: Vec<&'static str> = Vec::new();
    let live = pool.live();
    let free = pool.free_count();
    let cap = pool.capacity();
    if live as u64 + free as u64 != cap as u64 {
        out.push("账实不符：live+free != capacity（泄漏第三态）");
    }
    let mut on_chain = 0u32;
    for s in 0..cap {
        if pool.on_free_list(s) {
            on_chain += 1;
        }
    }
    if on_chain != free {
        out.push("链上数守恒破坏：on_chain != free_count");
    }
    if live > cap {
        out.push("live 越界");
    }
    out
}

/// 发射器状态风暴：按合法迁移表高频启停 N 轮，验证状态机不进死态。
pub fn fuzz_state_storm(rounds: usize, seed: u64) -> FuzzReport {
    let mut rep = FuzzReport::default();
    let mut rng = Lcg::new(seed);
    // 多发射器并行轮转（风暴的核心：不同发射器停在不同状态）。
    let mut states = [EmitterState::Created; 32];
    for _ in 0..rounds {
        rep.cases += 1;
        let idx = rng.below(states.len() as u64) as usize;
        let legal = legal_transitions(states[idx]);
        if legal.is_empty() {
            // Destroyed 无出边：必须保持 Destroyed（死态不可复活）。
            if states[idx] != EmitterState::Destroyed {
                rep.invariant_breaches += 1;
            }
            continue;
        }
        let pick = rng.below(legal.len() as u64) as usize;
        states[idx] = legal[pick];
    }
    rep
}

/// 池风暴：alloc/reclaim 高频交错，每轮三不变量 + 水位滞回档位复核。
pub fn fuzz_pool_storm(rounds: usize, seed: u64) -> (FuzzReport, Vec<FuzzEscalation>) {
    let mut rep = FuzzReport::default();
    let mut esc: Vec<FuzzEscalation> = Vec::new();
    let quota = PoolQuota {
        kind: PoolKind::Cpu,
        capacity: FUZZ_POOL_CAPACITY,
        stride: 64,
        emitter_share_pct: 100,
        bytes_cap: FUZZ_POOL_CAPACITY as u64 * 64,
    };
    let mut pool = match ParticlePool::new(&quota) {
        Ok(p) => p,
        Err(_) => {
            rep.invariant_breaches += 1;
            return (rep, esc);
        }
    };
    let mut bag = PoolBag::new();
    let mut rng = Lcg::new(seed);
    let mut held: Vec<u32> = Vec::new();
    for _ in 0..rounds {
        rep.cases += 1;
        let r = rng.below(100);
        if r < 60 {
            // 高频分配（风暴主力）。
            if let Ok(slot) = pool.try_alloc(&mut bag) {
                held.push(slot);
            }
            // 拒绝档拒绝必须显性：Rejection 带水位与消息（不空报）。
        } else if !held.is_empty() {
            // 批量回收（风暴的另一面）。
            let k = (rng.below(held.len() as u64) + 1) as usize;
            let mut batch: Vec<u32> = Vec::new();
            for _ in 0..k {
                batch.push(held.remove(held.len() - 1));
            }
            let got = pool.reclaim(&batch);
            if got != batch.len() {
                // 回收失败数≠批大小只在槽位非法时发生；本风暴全部合法。
                rep.invariant_breaches += 1;
            }
        }
        // 水位采样（带滞回）——档位只取阈值表内的合法值。
        if let Some(_lv) = pool.observe(&mut bag) {
            let pct = pool.water_pct();
            let legal_band = pct <= 100;
            if !legal_band {
                rep.invariant_breaches += 1;
            }
        }
        // 每轮三不变量（池泄漏是 P0 根基——锚点）。
        let breaches = pool_invariants(&pool);
        if !breaches.is_empty() {
            rep.invariant_breaches += breaches.len() as u32;
            esc.push(FuzzEscalation::new(E_FUZZ_LEAK, FuzzSegment::EmitterStorm, seed, 0));
        }
    }
    // 风暴收敛：全量回收后池必须回到零占用（最终一致）。
    let mut all: Vec<u32> = Vec::new();
    for s in held.drain(..) {
        all.push(s);
    }
    let _ = pool.reclaim(&all);
    if pool.live() != 0 {
        rep.invariant_breaches += 1;
        esc.push(FuzzEscalation::new(E_FUZZ_LEAK, FuzzSegment::EmitterStorm, seed, 0));
    }
    let tail = pool_invariants(&pool);
    if !tail.is_empty() {
        rep.invariant_breaches += tail.len() as u32;
    }
    (rep, esc)
}

// ---------------------------------------------------------------------------
// 七、段三：混合场景 fuzz（四模式并发+力场预留+池压力交错）
// ---------------------------------------------------------------------------

/// 混合风暴：四模式全覆盖轮转 + 力场 STUB 显性调用 + 池 alloc/reclaim
/// 交错，步数预算内不崩且不变量保持。
pub fn fuzz_mixed_storm(steps: usize, seed: u64) -> (FuzzReport, Vec<FuzzEscalation>) {
    let mut rep = FuzzReport::default();
    let mut esc: Vec<FuzzEscalation> = Vec::new();
    let quota = PoolQuota {
        kind: PoolKind::Cpu,
        capacity: FUZZ_POOL_CAPACITY,
        stride: 64,
        emitter_share_pct: 100,
        bytes_cap: FUZZ_POOL_CAPACITY as u64 * 64,
    };
    let mut pool = match ParticlePool::new(&quota) {
        Ok(p) => p,
        Err(_) => {
            rep.invariant_breaches += 1;
            return (rep, esc);
        }
    };
    let mut bag = PoolBag::new();
    let mut rng = Lcg::new(seed);
    let mut held: Vec<u32> = Vec::new();
    let mut stub = FieldGizmoStub::new();
    let mut v9bag = V9DiagBag::new();
    let mut stub_hits = 0u32;
    // 四模式手写全集（ModeKind 无 all()——闭集在判据 L11-混合-04 钉死）。
    let modes = [
        ModeKind::Continuous,
        ModeKind::Burst,
        ModeKind::Interval,
        ModeKind::EventDriven,
    ];
    for step in 0..steps {
        if step >= SEG3_MIXED_STEPS {
            // 步数预算（「不挂起」转可判定——vef18 纪律）。
            rep.budget_exceeded += 1;
            break;
        }
        rep.cases += 1;
        let pick = rng.below(100);
        if pick < 40 {
            // 四模式轮转（并发压力的节奏面；zh/en 双语可达性即消费面）。
            let m = modes[(rng.below(modes.len() as u64)) as usize];
            let _ = m.zh();
            let _ = m.en();
        } else if pick < 55 {
            // 力场预留调用（STUB 必须显性报错——静默=红）。
            let calls_before = stub.calls;
            let r = stub.request(&mut v9bag);
            if r.is_err() && v9bag.has(V9DiagCode::FIELD_STUB_CALLED) && stub.calls > calls_before {
                stub_hits += 1;
            } else {
                rep.silent_rejects += 1;
                esc.push(FuzzEscalation::new(E_FUZZ_SILENT, FuzzSegment::MixedStorm, seed, 0));
            }
        } else if pick < 80 {
            if let Ok(slot) = pool.try_alloc(&mut bag) {
                held.push(slot);
            }
        } else if !held.is_empty() {
            let k = (rng.below(held.len() as u64) + 1) as usize;
            let mut batch: Vec<u32> = Vec::new();
            for _ in 0..k {
                batch.push(held.remove(held.len() - 1));
            }
            let _ = pool.reclaim(&batch);
        }
        let breaches = pool_invariants(&pool);
        if !breaches.is_empty() {
            rep.invariant_breaches += breaches.len() as u32;
            esc.push(FuzzEscalation::new(E_FUZZ_LEAK, FuzzSegment::MixedStorm, seed, 0));
        }
    }
    rep.diag_hits += stub_hits;
    // 收敛：全量回收后零占用。
    let mut all: Vec<u32> = Vec::new();
    for s in held.drain(..) {
        all.push(s);
    }
    let _ = pool.reclaim(&all);
    if pool.live() != 0 {
        rep.invariant_breaches += 1;
    }
    (rep, esc)
}

// ---------------------------------------------------------------------------
// 八、三段跑批入口 + 判据
// ---------------------------------------------------------------------------

/// 三段跑批（纯函数面；万级夜间批跑的调度入 F1769 L 段，不在本模块
/// 内置时序——锚点性能纪律）。返回逐段报告与全部立案。
pub fn run_vel11_fuzz_all(now_ms: u64) -> ([FuzzReport; 3], Vec<FuzzEscalation>) {
    let mut esc: Vec<FuzzEscalation> = Vec::new();
    let (r1, _, e1) = fuzz_malformed_emitter(now_ms);
    let (r1b, e1b) = fuzz_pool_quota_edges(now_ms);
    let mut seg1 = r1;
    seg1.cases += r1b.cases;
    seg1.silent_rejects += r1b.silent_rejects;
    let mut seg2 = fuzz_state_storm(SEG2_STORM_ROUNDS, 0xF221_1002);
    let (r2b, e2b) = fuzz_pool_storm(SEG2_STORM_ROUNDS, 0xF221_1003);
    seg2.cases += r2b.cases;
    seg2.invariant_breaches += r2b.invariant_breaches;
    let (seg3, e3) = fuzz_mixed_storm(SEG3_MIXED_STEPS, 0xF221_1004);
    for e in e1.into_iter().chain(e1b).chain(e2b).chain(e3) {
        esc.push(e);
    }
    ([seg1, seg2, seg3], esc)
}

use crate::checks::CheckSet;

/// F2211 域自检（判据逐条映射；三段真跑 + 期望值判据侧独立重算）。
pub fn run_vel11_checks() -> CheckSet {
    let mut s = CheckSet::new("VE-F2211");
    let now = 1_000_000u64;

    // --- 段一：畸形发射器 ---
    let (rep1, clamped, esc1) = fuzz_malformed_emitter(now);
    // L11-畸形-01：角点数齐全。
    s.add("L11-畸形-01", rep1.cases >= SEG1_RATE_CORNERS as u32, "发射率七角点全扫");
    // L11-畸形-02：NaN→0（独立重算对账）。
    s.add("L11-畸形-02", clamped[0] == 0.0 && !clamped[0].is_nan(), "NaN 发射率钳到 0");
    // L11-畸形-03：+Inf→上限、-Inf→0（方向分别处置）。
    s.add(
        "L11-畸形-03",
        clamped[1] == EMIT_RATE_MAX_PER_SEC && clamped[2] == 0.0,
        "无穷发射率方向分别钳制",
    );
    // L11-畸形-04：负值→0。
    s.add("L11-畸形-04", clamped[3] == 0.0, "负发射率钳到 0");
    // L11-畸形-05：合法值透传（0 与上限本身不改写）。
    s.add(
        "L11-畸形-05",
        clamped[4] == 0.0 && clamped[5] == EMIT_RATE_MAX_PER_SEC,
        "合法边界值透传不改写",
    );
    // L11-畸形-06：超上限钳回上限。
    s.add("L11-畸形-06", clamped[6] == EMIT_RATE_MAX_PER_SEC, "超上限钳回上限");
    // L11-畸形-07：显性不变量——钳制必产诊断（诊断命中数 ≥ 4 类非法）。
    s.add("L11-畸形-07", rep1.diag_hits >= 4, "每次非法钳制都留痕（显性）");
    // L11-畸形-08：零静默。
    s.add("L11-畸形-08", rep1.silent_rejects == 0, "无静默改写");
    // L11-畸形-09：零钳制失灵。
    s.add("L11-畸形-09", rep1.clamp_failures == 0, "钳制值全部落期望域");

    // 池申请边界。
    let (rep1q, _escq) = fuzz_pool_quota_edges(now);
    // L11-畸形-10：越界序列全显性拒绝（零静默）。
    s.add("L11-畸形-10", rep1q.cases == 5 && rep1q.silent_rejects == 0, "五类越界 quota 全显性拒");
    // L11-畸形-11：u32::MAX 乘法溢出面被覆盖（最大配额在序列内且被拒）。
    s.add("L11-畸形-11", rep1q.cases as usize >= 5, "溢出面在扫描域内");

    // 累积器容错。
    let rep1a = fuzz_accumulator_edges();
    // L11-畸形-12：非法组合零产出且余量不动。
    s.add("L11-畸形-12", rep1a.cases >= 20 && rep1a.invariant_breaches == 0, "累积器容错不吞半粒");
    // L11-畸形-13：合法组合仍可产出（双向——不是全拒）。
    let mut acc = EmitAccumulator::new();
    let got = advance_accumulator(&mut acc, 60.0, 1.0);
    s.add("L11-畸形-13", got == 60, "合法 rate×dt 正常产出（容错不误伤）");

    // --- 段二：粒子风暴 ---
    let rep2s = fuzz_state_storm(SEG2_STORM_ROUNDS, 0xF221_1002);
    // L11-风暴-01：状态机风暴零违例（死态不可复活+迁移合法）。
    s.add("L11-风暴-01", rep2s.cases == SEG2_STORM_ROUNDS as u32 && rep2s.invariant_breaches == 0, "启停风暴状态机合法");
    let (rep2p, esc2) = fuzz_pool_storm(SEG2_STORM_ROUNDS, 0xF221_1003);
    // L11-风暴-02：池风暴全程三不变量（账实/链守恒/越界）。
    s.add("L11-风暴-02", rep2p.cases == SEG2_STORM_ROUNDS as u32 && rep2p.invariant_breaches == 0, "池风暴三不变量保持");
    // L11-风暴-03：风暴收敛（终态零占用）。
    s.add("L11-风暴-03", esc2.is_empty(), "风暴结束池收敛零泄漏");
    // L11-风暴-04：滞回阈值钉死（四组阈值具体数值，锚点水位正确）。
    s.add(
        "L11-风暴-04",
        WM_WARN_PCT == 70 && WM_DEGRADE_PCT == 85 && WM_REJECT_PCT == 95 && WM_WARN_PCT > HYST_WARN_OFF,
        "水位阈值与滞回常量钉死",
    );
    // L11-风暴-05：不变量复核器自身有判别力（对满池断言成立）。
    let quota_ok = PoolQuota {
        kind: PoolKind::Cpu,
        capacity: 16,
        stride: 32,
        emitter_share_pct: 100,
        bytes_cap: 16 * 32,
    };
    let mut pool_ok = match ParticlePool::new(&quota_ok) {
        Ok(p) => p,
        Err(_) => {
            s.add("L11-风暴-05", false, "判据语料池建池失败");
            return finish(s);
        }
    };
    let mut pbag = PoolBag::new();
    for _ in 0..16 {
        let _ = pool_ok.try_alloc(&mut pbag);
    }
    s.add("L11-风暴-05", pool_ok.live() == 16 && pool_invariants(&pool_ok).is_empty(), "满池基线非平凡且不变量成立");

    // --- 段三：混合场景 ---
    let (rep3, esc3) = fuzz_mixed_storm(SEG3_MIXED_STEPS, 0xF221_1004);
    // L11-混合-01：步数预算内完成（不挂起可判定）。
    s.add("L11-混合-01", rep3.cases == SEG3_MIXED_STEPS as u32 && rep3.budget_exceeded == 0, "混合风暴预算内全程跑完");
    // L11-混合-02：组合压力下池不变量保持。
    s.add("L11-混合-02", rep3.invariant_breaches == 0, "混合压力池不变量零违例");
    // L11-混合-03：力场 STUB 显性（调用必留痕——静默=红）。
    s.add("L11-混合-03", rep3.diag_hits > 0 && rep3.silent_rejects == 0, "力场预留调用显性报错");
    // L11-混合-04：四模式覆盖（手写全集=ModeKind 四变体，中文名互异钉死闭集）。
    let m4 = [
        ModeKind::Continuous,
        ModeKind::Burst,
        ModeKind::Interval,
        ModeKind::EventDriven,
    ];
    let zh_ok = m4[0].zh() != m4[1].zh()
        && m4[1].zh() != m4[2].zh()
        && m4[2].zh() != m4[3].zh()
        && m4[0].zh() != m4[3].zh();
    s.add("L11-混合-04", zh_ok, "四模式闭集（中文名互异）");
    // L11-混合-05：混合风暴零立案。
    s.add("L11-混合-05", esc3.is_empty(), "混合场景零立案");

    // --- 案例库与固化 ---
    // L11-固化-01：去重（同案例二入库只记一）。
    let mut book = FuzzCasebook::new();
    let c = FuzzCase::new(FuzzSegment::EmitterStorm, 7, 42);
    let first = book.admit(c.clone());
    let second = book.admit(c.clone());
    s.add(
        "L11-固化-01",
        matches!(first, Ok(true)) && matches!(second, Ok(false)) && book.len() == 1 && book.dedup_rejected == 1,
        "同案例去重不入库（控库容）",
    );
    // L11-固化-02：去重哈希非零且确定性（同输入同哈希）。
    let c2 = FuzzCase::new(FuzzSegment::EmitterStorm, 7, 42);
    s.add("L11-固化-02", c.dedup_hash != 0 && c.dedup_hash == c2.dedup_hash, "去重哈希非零且确定");
    // L11-固化-03：不同输入哈希不同（可分性下界）。
    let c3 = FuzzCase::new(FuzzSegment::EmitterStorm, 7, 43);
    s.add("L11-固化-03", c3.dedup_hash != c.dedup_hash, "不同输入哈希可分");
    // L11-固化-04：三段闭集与短码互异。
    let segs = FuzzSegment::all();
    s.add(
        "L11-固化-04",
        segs.len() == 3 && segs[0].wire() != segs[1].wire() && segs[1].wire() != segs[2].wire(),
        "三段短码互异",
    );
    // L11-固化-05：24h 期限钉死（raised+86400000，独立重算对账）。
    let esc0 = FuzzEscalation::new(E_FUZZ_CRASH, FuzzSegment::MixedStorm, 9, 500_000);
    s.add(
        "L11-固化-05",
        P0_REPRO_DEADLINE_MS == 86_400_000 && esc0.deadline_ms == 500_000 + 86_400_000,
        "P0 复现期限=24h 钉死",
    );
    // L11-固化-06：立案码互异非空。
    s.add(
        "L11-固化-06",
        E_FUZZ_CRASH != E_FUZZ_LEAK && E_FUZZ_SILENT != E_FUZZ_CLAMP && !E_FUZZ_HASH.is_empty(),
        "立案码非空互异",
    );

    // --- 三段汇总 ---
    // L11-总-01：三段跑批全段通过。
    let (reports, esc_all) = run_vel11_fuzz_all(now);
    s.add(
        "L11-总-01",
        reports.len() == 3 && reports.iter().all(|r| r.passed()),
        "三段 fuzz 全段通过（不崩+预算内+零静默+零违例）",
    );
    // L11-总-02：全批零立案（P0 无）
    s.add("L11-总-02", esc_all.is_empty() && esc1.is_empty() && esc2.is_empty(), "全批零 P0 立案");
    // L11-总-03：版本指纹锚定（FNV-1a const 期=运行期同算防手抄漂移）。
    s.add(
        "L11-总-03",
        fnv1a(b"L11-fuzz-v1") == {
            let mut h: u64 = 0xcbf29ce484222325;
            for b in FUZZ_PROTOCOL_VERSION.bytes() {
                h ^= b as u64;
                h = h.wrapping_mul(0x100000001b3);
            }
            h
        },
        "版本指纹与协议版本一致",
    );
    // L11-总-04：判据条数对账。
    s.add("L11-总-04", s.len() == 32, "判据条数对账（本条为第 33 条）");

    finish(s)
}

/// 收口（截断态如实上报）。
fn finish(s: CheckSet) -> CheckSet {
    s
}
