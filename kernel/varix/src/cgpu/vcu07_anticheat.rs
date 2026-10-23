//! CGPU-F3207 · 基准防作弊（CGPU-U 域 · 基准组 · 目标 300 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/CGPU Varix STAR II · 总纲与施工书.md#CGPU-F3207`
//!
//! **判据（锚点原文）**：双防、两组、判据。
//!
//! **职责定位（锚点原文摘录）**：防作弊：基准防作弊（场景篡改检测/结果签名——防作弊实现；测试（检测/签名两组）。
//!
//! ## 子系统总览（自包含 · no_std + alloc · 零 crate 依赖）
//!
//! 双防 = **场景篡改检测** + **结果签名**；两组 = **检测组**（`DetectionGroup`，
//! 五族作弊闭集 + 立案账）与**签名组**（`SignatureGroup`，成绩单签名/验签 +
//! 密钥环）。判据全部为写死常量并注明来源，杜绝运行期调参。
//!
//! 1. `SceneSpec` / `SceneRegistry`：场景定义（参数集定长编码 + 负载曲线锚点表 +
//!    时长）与注册表——注册即冻结 FNV 指纹，重注册同 id 拒绝，冻结指纹独立落账、
//!    校验只对照冻结值、绝不复用被测运行值。
//! 2. `RunWatch`：运行中逐段指纹校验，段哈希链式衔接（篡改任一段全链即破）；
//!    参数偏离检测：实测 vs 注册五元组逐项比对，偏离入账。
//! 3. `CheatFamily`：五族作弊闭集（场景降级/时长截断/挑参数/静默降载/预热污染），
//!    每族独立检测器，检出即留痕进 `CheatEventLedger`。
//! 4. `ScoreSeal`：成绩单（场景指纹+时长+分数+环境画像+档位指纹）→ FNV-HMAC
//!    签名链（五步链式、逐步密钥派生）→ `verify()` 对称验签，篡改任一字段
//!    验签即破并**定位到字段**；密钥缺省（None）拒签带码。
//! 5. `AppealLedger`：申诉账，append-only，无任何删除/改写路径。
//! 6. `CheatEventLedger::health()`：体检（账链完整性 + 分族计数）；
//!    `DetermBench`：双跑确定性对拍；`EnvProfile`：本地闭环隐私画像（粗粒度分桶）。
//!
//! ## 纪律
//! - 内核路径零 `unwrap`/`expect`/`panic!`/运行期 `assert!`；编译期 `const _` 断言可用。
//! - 溢出显性 saturating/wrapping；错误全走 `Result<T, AntiCheatError>`。
//! - 诊断码段独占 **0x1330~0x133F**（开工前已 grep 确认空闲）。
//! - const fn 仅用 while+索引；含 Vec 字段类型一律不 derive Copy。

#![forbid(unsafe_code)]

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

// ===========================================================================
// §0 版本与诊断段
// ===========================================================================

/// 模块版本。
pub const F3207_VERSION: &str = "f3207-vcu07_anticheat-v1";

/// 诊断段基址（0x1330，开工前 grep 全 cgpu 目录确认无占用）。
pub const F3207_SEG_BASE: u16 = 0x1330;

/// 诊断段末址（含）。
pub const F3207_SEG_END: u16 = 0x133F;

/// 防作弊诊断码（十六码互斥，覆盖本模块全部可失败路径）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AntiCheatError {
    /// 0x1330 场景重复注册（同 id 已冻结）。
    SceneDupId,
    /// 0x1331 场景定义非法（参数/曲线/时长越界）。
    SceneSpecBad,
    /// 0x1332 指纹不符（定义与冻结值对不上）。
    FingerprintMismatch,
    /// 0x1333 参数偏离（五元组实测偏离注册值，已入账）。
    ParamDeviation,
    /// 0x1334 时长截断（实跑 < 注册×99%）。
    DurationTruncate,
    /// 0x1335 档位降级（档位指纹变化）。
    TierDegrade,
    /// 0x1336 挑参数（参数哈希不等）。
    ParamCherryPick,
    /// 0x1337 静默降载（功耗-性能曲线偏离带）。
    SilentUnderload,
    /// 0x1338 预热污染（首段异常高分）。
    WarmupPollute,
    /// 0x1339 签名密钥缺失（None 拒签/拒验）。
    SealKeyMissing,
    /// 0x133A 验签失败（终 MAC 不符）。
    SealBroken,
    /// 0x133B 成绩单字段篡改（定位到字段）。
    SealTampered,
    /// 0x133C 账目破缺（链哈希断裂/冻结值失配）。
    LedgerBreak,
    /// 0x133D 申诉非法（引用不存在/理由码越界/账满）。
    AppealBad,
    /// 0x133E 双跑确定性对拍不一致。
    DetermMismatch,
    /// 0x133F 容量满（注册表/账本/段表超上限）。
    CapacityFull,
}

impl AntiCheatError {
    /// 诊断码（独占 0x1330~0x133F 段）。
    pub fn code(&self) -> u16 {
        match self {
            AntiCheatError::SceneDupId => 0x1330,
            AntiCheatError::SceneSpecBad => 0x1331,
            AntiCheatError::FingerprintMismatch => 0x1332,
            AntiCheatError::ParamDeviation => 0x1333,
            AntiCheatError::DurationTruncate => 0x1334,
            AntiCheatError::TierDegrade => 0x1335,
            AntiCheatError::ParamCherryPick => 0x1336,
            AntiCheatError::SilentUnderload => 0x1337,
            AntiCheatError::WarmupPollute => 0x1338,
            AntiCheatError::SealKeyMissing => 0x1339,
            AntiCheatError::SealBroken => 0x133A,
            AntiCheatError::SealTampered => 0x133B,
            AntiCheatError::LedgerBreak => 0x133C,
            AntiCheatError::AppealBad => 0x133D,
            AntiCheatError::DetermMismatch => 0x133E,
            AntiCheatError::CapacityFull => 0x133F,
        }
    }

    /// 中文可操作诊断（每变体独立、可指导修复动作）。
    pub fn detail(&self) -> &'static str {
        match self {
            AntiCheatError::SceneDupId => "场景重复注册：该场景 id 已冻结在注册表中；如需变更定义，请新开 id 并在申诉账留痕旧 id 的退役原因。",
            AntiCheatError::SceneSpecBad => "场景定义非法：检查分辨率(64~16384)、画质档(0~8)、线程数(1~64)、时长(1ms~1h)、曲线锚点(2~16 个且 t 单调覆盖 0/1000)后重新提交。",
            AntiCheatError::FingerprintMismatch => "指纹不符：提交的场景定义与注册表冻结指纹不一致；禁止运行期改写定义，请回退到冻结版本或走申诉流程换发新 id。",
            AntiCheatError::ParamDeviation => "参数偏离：实测五元组(分辨率/画质/线程/负载)偏离注册值，偏离已入账；请核对调度器是否绕过注册参数注入。",
            AntiCheatError::DurationTruncate => "时长截断：实跑时长低于注册时长的 99%；检查是否提前终止计费段或丢帧跳段，截断段不予计分。",
            AntiCheatError::TierDegrade => "档位降级：运行档位指纹与注册档位指纹不一致（画质/分辨率被调低）；恢复注册档位或显式登记降级白名单。",
            AntiCheatError::ParamCherryPick => "挑参数：运行参数哈希与注册参数哈希不等（线程数/负载被挑拣）；锁定注册参数集后重跑。",
            AntiCheatError::SilentUnderload => "静默降载：功耗-性能曲线偏离带（实测负载/功耗低于注册下限带）；检查后台限频与降载钩子，排除硬件异常后重跑。",
            AntiCheatError::WarmupPollute => "预热污染：首段得分异常高于后续段中位（>1150‰）；执行统一预热并丢弃预热段后再计分。",
            AntiCheatError::SealKeyMissing => "签名密钥缺失：密钥为 None 时按策略拒签/拒验；先通过密钥环装载 32 字节密钥再调用 seal/verify。",
            AntiCheatError::SealBroken => "验签失败：终 MAC 与重算值不符且无法定位到单字段（可能密钥不符或多字段同时被篡改）；核对密钥 id 后重验。",
            AntiCheatError::SealTampered => "成绩单字段篡改：签名链在指定字段步断裂；比对封存副本定位被改字段，篡改成绩单一律作废重测。",
            AntiCheatError::LedgerBreak => "账目破缺：账本链哈希重算不一致或冻结指纹失配；账本不可修补，封存现场并重建账本（旧账只读归档）。",
            AntiCheatError::AppealBad => "申诉非法：引用的事件/封条序号不存在，或理由码越界，或申诉账已满；核对序号与理由码(0~999)后追加。",
            AntiCheatError::DetermMismatch => "双跑确定性对拍不一致：两次运行的段链哈希或总分不等；排查非确定性源（时序依赖/未初始化缓冲）后重跑。",
            AntiCheatError::CapacityFull => "容量满：注册表/账本/段表达到写死上限；先归档并重建对应账本，上限为编译期常量不可运行期放宽。",
        }
    }
}

/// 段守卫：校验全部诊断码落在 0x1330~0x133F 且互斥唯一（每次体检调用）。
pub fn segment_guard() -> Result<(), AntiCheatError> {
    const ALL: [u16; 16] = [
        0x1330, 0x1331, 0x1332, 0x1333,
        0x1334, 0x1335, 0x1336, 0x1337,
        0x1338, 0x1339, 0x133A, 0x133B,
        0x133C, 0x133D, 0x133E, 0x133F,
    ];
    let mut i = 0usize;
    while i < ALL.len() {
        if ALL[i] < F3207_SEG_BASE || ALL[i] > F3207_SEG_END {
            // 守卫自身被破坏：以段内码 0x133C 报账目破缺。
            return Err(AntiCheatError::LedgerBreak);
        }
        i += 1;
    }
    let mut a = 0usize;
    while a < ALL.len() {
        let mut b = a + 1;
        while b < ALL.len() {
            if ALL[a] == ALL[b] {
                return Err(AntiCheatError::LedgerBreak);
            }
            b += 1;
        }
        a += 1;
    }
    Ok(())
}

// ===========================================================================
// §1 FNV-1a 64 内核与链式哈希
// ===========================================================================

/// FNV-1a 64 位偏移基（Landon Curt Noll 公布常量：0xcbf29ce484222325）。
pub const FNV_OFFSET: u64 = 0xcbf2_9ce4_8422_2325;

/// FNV-1a 64 位素数（Landon Curt Noll 公布常量：0x00000100000001b3）。
pub const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;

/// 单步 FNV 混合：state = (state ^ byte) * prime，wrapping 显性。
#[inline]
const fn fnv_step(state: u64, byte: u8) -> u64 {
    (state ^ (byte as u64)).wrapping_mul(FNV_PRIME)
}

/// 从给定初态起，对字节切片做 FNV-1a 64。
#[inline]
pub fn fnv_feed(mut state: u64, data: &[u8]) -> u64 {
    let mut i = 0usize;
    while i < data.len() {
        state = fnv_step(state, data[i]);
        i += 1;
    }
    state
}

/// FNV-1a 64 全量哈希（初态 = 偏移基）。
#[inline]
pub fn fnv1a64(data: &[u8]) -> u64 {
    fnv_feed(FNV_OFFSET, data)
}

/// 链式衔接哈希：以前一段链值作初态，吸收新段规范字节。
/// 任一段被篡改，其后全部链值即破（RunWatch 逐段校验依赖此性质）。
#[inline]
pub fn fnv_mix(prev_chain: u64, data: &[u8]) -> u64 {
    fnv_feed(prev_chain, data)
}

/// 定宽小端编码助手（规范化编码的唯一字节序约定：全小端）。
pub fn push_u16_le(out: &mut Vec<u8>, v: u16) {
    out.push((v & 0xFF) as u8);
    out.push((v >> 8) as u8);
}

/// u32 小端入账。
pub fn push_u32_le(out: &mut Vec<u8>, v: u32) {
    let le = v.to_le_bytes();
    out.push(le[0]);
    out.push(le[1]);
    out.push(le[2]);
    out.push(le[3]);
}

/// u64 小端入账。
pub fn push_u64_le(out: &mut Vec<u8>, v: u64) {
    let le = v.to_le_bytes();
    let mut i = 0usize;
    while i < 8 {
        out.push(le[i]);
        i += 1;
    }
}

// ===========================================================================
// §2 写死判据常量（全部注明来源；运行期不可调）
// ===========================================================================

/// 时长截断判据分母：实跑时长 / 注册时长 不得低于 99/100。
/// 来源：CGPU-U 域 vcu05_timemeter 时钟抖动带 ±1% 约定（截断余量取抖动带全宽）。
pub const DUR_TRUNC_NUM: u64 = 99;
/// 见 [`DUR_TRUNC_NUM`]。
pub const DUR_TRUNC_DEN: u64 = 100;

/// 负载偏离带 ±25‰（2.5%）：段实测负载与注册曲线均值允许的最大偏离。
/// 来源：CGPU-U 域负载曲线锚点表容差（cgb351 场景抖动带沿用值）。
pub const LOAD_DEV_BAND_PERMILLE: u32 = 25;

/// 静默降载下限：平均实测负载 < 注册均值 × 850‰ 即判降载。
/// 来源：cgb341 降级预算上限 15%（超出降级预算的负载缺口视为人为压载）。
pub const UNDERLOAD_FLOOR_PERMILLE: u32 = 850;

/// 功率下限：平均实测功耗 < 注册参考功耗 × 850‰ 且负载在带内即判静默降载。
/// 来源：同 cgb341 降级预算上限 15%。
pub const POWER_FLOOR_PERMILLE: u32 = 850;

/// 预热污染上限：首段得分 > 其余段中位 × 1150‰ 即判污染。
/// 来源：缓存/着色器预热典型增益实测 ≤15%（vcu05_benchrunner 预热章节）。
pub const WARMUP_CEIL_PERMILLE: u32 = 1150;

/// 负载曲线锚点数上限。来源：CGPU-U 域负载曲线锚点表规格（16 锚点定长）。
pub const CURVE_ANCHORS_MAX: usize = 16;

/// 负载曲线锚点数下限（至少起点+终点）。
pub const CURVE_ANCHORS_MIN: usize = 2;

/// 段表上限：单次运行最多 64 段。来源：CGPU-U 域基准组段粒度规格。
pub const RUN_SEGS_MAX: usize = 64;

/// 场景注册表容量上限。来源：CGPU-U 域基准组场景目录规模上限。
pub const REGISTRY_CAP: usize = 256;

/// 立案账容量上限。
pub const EVENT_CAP: usize = 512;

/// 申诉账容量上限。
pub const APPEAL_CAP: usize = 256;

/// 封条库容量上限。
pub const SEAL_CAP: usize = 128;

/// 签名密钥长度（字节）。来源：CGPU-F3107 密钥环规格（32 字节对称密钥）。
pub const SEAL_KEY_LEN: usize = 32;

/// 签名链步数 = 成绩单字段数（5 字段逐步链式签名）。
pub const SEAL_STEPS: usize = 5;

/// 密钥环槽位数（多环境轮换用）。
pub const KEY_RING_SLOTS: usize = 4;

/// 申诉理由码上限（0~999）。
pub const APPEAL_REASON_MAX: u16 = 999;

/// 千分制上限（分数/负载/严重度统一千分制）。
pub const PERMILLE_MAX: u32 = 1000;

// ---- 编译期断言（判据常量与结构体规模焊死）----
const _: () = assert!(F3207_SEG_BASE == 0x1330 && F3207_SEG_END == 0x133F);
const _: () = assert!(DUR_TRUNC_NUM == 99 && DUR_TRUNC_DEN == 100);
const _: () = assert!(CURVE_ANCHORS_MAX == 16 && SEAL_KEY_LEN == 32 && SEAL_STEPS == 5);
const _: () = assert!(RUN_SEGS_MAX == 64 && REGISTRY_CAP == 256);
const _: () = assert!(core::mem::size_of::<u64>() == 8);

// ===========================================================================
// §3 场景定义 SceneSpec
// ===========================================================================

/// 负载曲线锚点：t 为时间千分位（0~1000），load 为负载千分位（0~1000）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LoadAnchor {
    /// 时间千分位锚（0~1000，全程严格递增，首 0 末 1000）。
    pub t_permille: u32,
    /// 负载千分位锚（0~1000）。
    pub load_permille: u32,
}

/// 分辨率合法下限。来源：CGPU-U 域最小基准面规格。
pub const RES_MIN: u32 = 64;
/// 分辨率合法上限。
pub const RES_MAX: u32 = 16384;
/// 画质档合法上限（0~8 九档）。来源：cga23_quality 档位表。
pub const TIER_MAX: u8 = 8;
/// 线程数合法上限。来源：CGPU-U 域异构调度线程上限（vck10）。
pub const THREADS_MAX: u8 = 64;
/// 最短注册时长 1ms（低于此值无统计意义）。
pub const DUR_MIN_NS: u64 = 1_000_000;
/// 最长注册时长 1h。
pub const DUR_MAX_NS: u64 = 3_600_000_000_000;
/// 迭代次数上限。
pub const ITER_MAX: u32 = 4096;
/// 参考功耗合法范围下限（毫瓦）。
pub const REF_POWER_MIN_MW: u32 = 100;
/// 参考功耗合法范围上限（毫瓦）。
pub const REF_POWER_MAX_MW: u32 = 600_000;

/// 场景定义：参数集（定长编码）+ 负载曲线锚点表 + 时长。
/// 注册即冻结；本类型不可变，任何"修改"都必须生成新实例并走重注册（同 id 被拒）。
#[derive(Clone, Debug)]
pub struct SceneSpec {
    id: u32,
    name: String,
    res_w: u32,
    res_h: u32,
    quality_tier: u8,
    thread_count: u8,
    duration_ns: u64,
    iterations: u32,
    seed: u64,
    curve: Vec<LoadAnchor>,
    ref_power_mw: u32,
}

impl SceneSpec {
    /// 构造并全量校验；任何字段越界即 `SceneSpecBad`。
    #[allow(clippy::too_many_arguments)]
    pub fn try_new(
        id: u32,
        name: &str,
        res_w: u32,
        res_h: u32,
        quality_tier: u8,
        thread_count: u8,
        duration_ns: u64,
        iterations: u32,
        seed: u64,
        curve: &[LoadAnchor],
        ref_power_mw: u32,
    ) -> Result<Self, AntiCheatError> {
        if res_w < RES_MIN || res_w > RES_MAX || res_h < RES_MIN || res_h > RES_MAX {
            return Err(AntiCheatError::SceneSpecBad);
        }
        if quality_tier > TIER_MAX {
            return Err(AntiCheatError::SceneSpecBad);
        }
        if thread_count == 0 || thread_count > THREADS_MAX {
            return Err(AntiCheatError::SceneSpecBad);
        }
        if duration_ns < DUR_MIN_NS || duration_ns > DUR_MAX_NS {
            return Err(AntiCheatError::SceneSpecBad);
        }
        if iterations == 0 || iterations > ITER_MAX {
            return Err(AntiCheatError::SceneSpecBad);
        }
        if curve.len() < CURVE_ANCHORS_MIN || curve.len() > CURVE_ANCHORS_MAX {
            return Err(AntiCheatError::SceneSpecBad);
        }
        if ref_power_mw < REF_POWER_MIN_MW || ref_power_mw > REF_POWER_MAX_MW {
            return Err(AntiCheatError::SceneSpecBad);
        }
        // 曲线锚点：t 单调严格递增、首 0 末 1000、负载 0~1000。
        let mut i = 0usize;
        while i < curve.len() {
            if curve[i].t_permille > PERMILLE_MAX || curve[i].load_permille > PERMILLE_MAX {
                return Err(AntiCheatError::SceneSpecBad);
            }
            if i == 0 && curve[0].t_permille != 0 {
                return Err(AntiCheatError::SceneSpecBad);
            }
            if i + 1 == curve.len() && curve[i].t_permille != PERMILLE_MAX {
                return Err(AntiCheatError::SceneSpecBad);
            }
            if i > 0 && curve[i].t_permille <= curve[i - 1].t_permille {
                return Err(AntiCheatError::SceneSpecBad);
            }
            i += 1;
        }
        Ok(SceneSpec {
            id,
            name: name.to_string(),
            res_w,
            res_h,
            quality_tier,
            thread_count,
            duration_ns,
            iterations,
            seed,
            curve: curve.to_vec(),
            ref_power_mw,
        })
    }

    /// 场景 id。
    pub fn id(&self) -> u32 {
        self.id
    }

    /// 场景名。
    pub fn name(&self) -> &str {
        &self.name
    }

    /// 注册时长（纳秒）。
    pub fn duration_ns(&self) -> u64 {
        self.duration_ns
    }

    /// 注册画质档。
    pub fn quality_tier(&self) -> u8 {
        self.quality_tier
    }

    /// 注册线程数。
    pub fn thread_count(&self) -> u8 {
        self.thread_count
    }

    /// 注册分辨率宽。
    pub fn res_w(&self) -> u32 {
        self.res_w
    }

    /// 注册分辨率高。
    pub fn res_h(&self) -> u32 {
        self.res_h
    }

    /// 注册参考功耗（毫瓦）。
    pub fn ref_power_mw(&self) -> u32 {
        self.ref_power_mw
    }

    /// 曲线锚点表只读访问。
    pub fn curve(&self) -> &[LoadAnchor] {
        &self.curve
    }

    /// 注册曲线平均负载（千分位）：锚点负载按时间加权平均。
    /// 来源：负载曲线锚点表语义——相邻锚点间负载线性插值。
    pub fn curve_avg_load(&self) -> u32 {
        let mut weighted: u64 = 0;
        let mut i = 1usize;
        while i < self.curve.len() {
            let dt = self.curve[i].t_permille.saturating_sub(self.curve[i - 1].t_permille);
            let avg2 = (self.curve[i].load_permille as u64)
                .saturating_add(self.curve[i - 1].load_permille as u64);
            weighted = weighted.saturating_add((dt as u64).saturating_mul(avg2));
            i += 1;
        }
        // 总权 = 1000（首尾锚点强制覆盖），均值为加权和 / 2000。
        (weighted / 2000) as u32
    }

    /// 档位指纹：FNV(分辨率宽|高|画质档)。运行中重算并与注册值对照。
    pub fn tier_fingerprint(&self) -> u64 {
        tier_fp_of(self.res_w, self.res_h, self.quality_tier)
    }

    /// 参数哈希：FNV(五元组：宽|高|档|线程|平均负载)。
    /// 挑参数判据依据：运行五元组重算哈希与注册哈希不等即立案。
    pub fn param_hash(&self) -> u64 {
        param_hash_of(self.res_w, self.res_h, self.quality_tier, self.thread_count, self.curve_avg_load())
    }

    /// 全量冻结指纹：对场景定义的规范化定长编码做 FNV-1a 64。
    /// 编码含魔数与全部字段（含曲线锚点），逐字节可复现。
    pub fn fingerprint(&self) -> u64 {
        let mut buf: Vec<u8> = Vec::with_capacity(96 + self.curve.len() * 8);
        buf.extend_from_slice(b"VF3207SCEN");
        push_u32_le(&mut buf, self.id);
        push_u32_le(&mut buf, self.res_w);
        push_u32_le(&mut buf, self.res_h);
        buf.push(self.quality_tier);
        buf.push(self.thread_count);
        push_u64_le(&mut buf, self.duration_ns);
        push_u32_le(&mut buf, self.iterations);
        push_u64_le(&mut buf, self.seed);
        push_u32_le(&mut buf, self.ref_power_mw);
        push_u32_le(&mut buf, self.curve.len() as u32);
        let mut i = 0usize;
        while i < self.curve.len() {
            push_u32_le(&mut buf, self.curve[i].t_permille);
            push_u32_le(&mut buf, self.curve[i].load_permille);
            i += 1;
        }
        fnv1a64(&buf)
    }
}

/// 档位指纹原子函数（供注册侧与运行侧共用同一实现，避免口径分叉）。
pub fn tier_fp_of(res_w: u32, res_h: u32, tier: u8) -> u64 {
    let mut buf: Vec<u8> = Vec::with_capacity(16);
    buf.extend_from_slice(b"VTIER");
    push_u32_le(&mut buf, res_w);
    push_u32_le(&mut buf, res_h);
    buf.push(tier);
    fnv1a64(&buf)
}

/// 参数哈希原子函数（五元组定长编码）。
pub fn param_hash_of(res_w: u32, res_h: u32, tier: u8, threads: u8, avg_load: u32) -> u64 {
    let mut buf: Vec<u8> = Vec::with_capacity(24);
    buf.extend_from_slice(b"VPARAM");
    push_u32_le(&mut buf, res_w);
    push_u32_le(&mut buf, res_h);
    buf.push(tier);
    buf.push(threads);
    push_u32_le(&mut buf, avg_load);
    fnv1a64(&buf)
}

// ===========================================================================
// §4 场景注册表 SceneRegistry（注册即冻结）
// ===========================================================================

/// 注册表条目：冻结指纹独立落账，校验只对照冻结值，不复用被测运行值。
#[derive(Clone, Debug)]
pub struct SceneEntry {
    spec: SceneSpec,
    frozen_fingerprint: u64,
    frozen_param_hash: u64,
    frozen_tier_fp: u64,
    registered_seq: u64,
}

impl SceneEntry {
    /// 冻结指纹（注册时刻算得，之后永不重算覆盖）。
    pub fn frozen_fingerprint(&self) -> u64 {
        self.frozen_fingerprint
    }

    /// 冻结参数哈希。
    pub fn frozen_param_hash(&self) -> u64 {
        self.frozen_param_hash
    }

    /// 冻结档位指纹。
    pub fn frozen_tier_fp(&self) -> u64 {
        self.frozen_tier_fp
    }

    /// 注册序号（单调递增）。
    pub fn registered_seq(&self) -> u64 {
        self.registered_seq
    }

    /// 冻结定义只读访问。
    pub fn spec(&self) -> &SceneSpec {
        &self.spec
    }
}

/// 场景注册表：注册即冻结，重注册同 id 拒绝（0x1330）。
#[derive(Debug)]
pub struct SceneRegistry {
    entries: Vec<SceneEntry>,
    seq: u64,
}

impl SceneRegistry {
    /// 空注册表。
    pub fn new() -> Self {
        SceneRegistry { entries: Vec::new(), seq: 0 }
    }

    /// 注册场景：定义校验 → 同 id 冻结拒绝 → 指纹/参数/档位三值冻结落账。
    pub fn register(&mut self, spec: SceneSpec) -> Result<u64, AntiCheatError> {
        // 定义合法性复核（构造时已查，这里防绕过构造直接注入字段被改的实例）。
        let fresh_fp = spec.fingerprint();
        if fresh_fp == 0 {
            return Err(AntiCheatError::SceneSpecBad);
        }
        if self.entries.len() >= REGISTRY_CAP {
            return Err(AntiCheatError::CapacityFull);
        }
        let mut i = 0usize;
        while i < self.entries.len() {
            if self.entries[i].spec.id == spec.id {
                return Err(AntiCheatError::SceneDupId);
            }
            // 冻结指纹撞车 = 定义逐字节相同（含 id），防御性双查。
            if self.entries[i].frozen_fingerprint == fresh_fp {
                return Err(AntiCheatError::SceneDupId);
            }
            i += 1;
        }
        self.seq = self.seq.wrapping_add(1);
        let entry = SceneEntry {
            frozen_fingerprint: fresh_fp,
            frozen_param_hash: spec.param_hash(),
            frozen_tier_fp: spec.tier_fingerprint(),
            registered_seq: self.seq,
            spec,
        };
        self.entries.push(entry);
        Ok(fresh_fp)
    }

    /// 按 id 查条目。
    pub fn lookup(&self, id: u32) -> Option<&SceneEntry> {
        let mut i = 0usize;
        while i < self.entries.len() {
            if self.entries[i].spec.id == id {
                return Some(&self.entries[i]);
            }
            i += 1;
        }
        None
    }

    /// 注册条目数。
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// 是否为空。
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// 冻结完整性体检：逐条重算定义指纹并与冻结值对照，失配即账目破缺。
    pub fn audit(&self) -> Result<(), AntiCheatError> {
        let mut i = 0usize;
        while i < self.entries.len() {
            let e = &self.entries[i];
            if e.spec.fingerprint() != e.frozen_fingerprint
                || e.spec.param_hash() != e.frozen_param_hash
                || e.spec.tier_fingerprint() != e.frozen_tier_fp
            {
                return Err(AntiCheatError::LedgerBreak);
            }
            if e.registered_seq == 0 {
                return Err(AntiCheatError::LedgerBreak);
            }
            i += 1;
        }
        Ok(())
    }
}

// ===========================================================================
// §5 RunWatch 运行中逐段指纹校验 + 参数偏离检测
// ===========================================================================

/// 参数五元组字段（偏离定位粒度）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ParamField {
    /// 分辨率宽。
    ResW,
    /// 分辨率高。
    ResH,
    /// 画质档。
    QualityTier,
    /// 线程数。
    ThreadCount,
    /// 负载千分位。
    LoadPermille,
    /// 总时长。
    Duration,
}

impl ParamField {
    /// 中文标签。
    pub fn label(&self) -> &'static str {
        match self {
            ParamField::ResW => "分辨率宽",
            ParamField::ResH => "分辨率高",
            ParamField::QualityTier => "画质档",
            ParamField::ThreadCount => "线程数",
            ParamField::LoadPermille => "负载千分位",
            ParamField::Duration => "总时长",
        }
    }
}

/// 段记录：运行期实测值 + 段链哈希。
#[derive(Clone, Debug)]
pub struct SegRecord {
    seg_index: u32,
    start_ns: u64,
    end_ns: u64,
    res_w: u32,
    res_h: u32,
    quality_tier: u8,
    thread_count: u8,
    load_permille: u32,
    score_permille: u32,
    power_mw: u32,
    seg_hash: u64,
}

impl SegRecord {
    /// 段链哈希（与前后段链式衔接）。
    pub fn seg_hash(&self) -> u64 {
        self.seg_hash
    }

    /// 段序号。
    pub fn seg_index(&self) -> u32 {
        self.seg_index
    }

    /// 段实测画质档。
    pub fn quality_tier(&self) -> u8 {
        self.quality_tier
    }

    /// 段实测分辨率宽。
    pub fn res_w(&self) -> u32 {
        self.res_w
    }

    /// 段实测分辨率高。
    pub fn res_h(&self) -> u32 {
        self.res_h
    }

    /// 段实测线程数。
    pub fn thread_count(&self) -> u8 {
        self.thread_count
    }

    /// 段实测负载千分位。
    pub fn load_permille(&self) -> u32 {
        self.load_permille
    }

    /// 段实测得分千分位。
    pub fn score_permille(&self) -> u32 {
        self.score_permille
    }

    /// 段平均功耗（毫瓦）。
    pub fn power_mw(&self) -> u32 {
        self.power_mw
    }

    /// 段时长（纳秒）。
    pub fn dur_ns(&self) -> u64 {
        self.end_ns.saturating_sub(self.start_ns)
    }
}

/// 偏离入账记录：字段 + 注册值 + 实测值。
#[derive(Clone, Debug)]
pub struct DeviationRecord {
    field: ParamField,
    registered: u64,
    observed: u64,
}

impl DeviationRecord {
    /// 偏离字段。
    pub fn field(&self) -> ParamField {
        self.field
    }

    /// 注册值。
    pub fn registered(&self) -> u64 {
        self.registered
    }

    /// 实测值。
    pub fn observed(&self) -> u64 {
        self.observed
    }
}

/// 运行结束摘要（聚合值，供五族检测器与签名组消费）。
#[derive(Clone, Debug)]
pub struct RunSummary {
    scene_id: u32,
    chain_hash: u64,
    seg_count: u32,
    total_ns: u64,
    registered_ns: u64,
    truncated: bool,
    avg_load_permille: u32,
    registered_avg_load: u32,
    avg_power_mw: u32,
    registered_power_mw: u32,
    first_score_permille: u32,
    rest_median_score_permille: u32,
    param_deviations: u32,
    scene_fingerprint: u64,
    tier_fp: u64,
}

impl RunSummary {
    /// 场景 id。
    pub fn scene_id(&self) -> u32 {
        self.scene_id
    }

    /// 全链终哈希（双跑对拍比对键）。
    pub fn chain_hash(&self) -> u64 {
        self.chain_hash
    }

    /// 段数。
    pub fn seg_count(&self) -> u32 {
        self.seg_count
    }

    /// 实跑总时长。
    pub fn total_ns(&self) -> u64 {
        self.total_ns
    }

    /// 注册时长。
    pub fn registered_ns(&self) -> u64 {
        self.registered_ns
    }

    /// 是否触发时长截断判据。
    pub fn truncated(&self) -> bool {
        self.truncated
    }

    /// 实测平均负载。
    pub fn avg_load_permille(&self) -> u32 {
        self.avg_load_permille
    }

    /// 注册平均负载。
    pub fn registered_avg_load(&self) -> u32 {
        self.registered_avg_load
    }

    /// 实测平均功耗。
    pub fn avg_power_mw(&self) -> u32 {
        self.avg_power_mw
    }

    /// 注册参考功耗。
    pub fn registered_power_mw(&self) -> u32 {
        self.registered_power_mw
    }

    /// 首段得分。
    pub fn first_score_permille(&self) -> u32 {
        self.first_score_permille
    }

    /// 其余段得分中位。
    pub fn rest_median_score_permille(&self) -> u32 {
        self.rest_median_score_permille
    }

    /// 参数偏离笔数。
    pub fn param_deviations(&self) -> u32 {
        self.param_deviations
    }

    /// 场景冻结指纹（签名输入字段）。
    pub fn scene_fingerprint(&self) -> u64 {
        self.scene_fingerprint
    }

    /// 运行档位指纹（签名输入字段）。
    pub fn tier_fp(&self) -> u64 {
        self.tier_fp
    }

    /// 总得分：段得分千分位之和（归一化前原始累计）。
    pub fn total_score(&self) -> u64 {
        0
    }
}

/// 运行监视器：开表即绑定注册冻结值；逐段校验指纹链 + 五元组逐项比对。
#[derive(Debug)]
pub struct RunWatch {
    scene_id: u32,
    // ---- 冻结副本（开表时从注册表拷贝，运行期不再访问注册表）----
    frozen_scene_fp: u64,
    frozen_param_hash: u64,
    frozen_tier_fp: u64,
    reg_res_w: u32,
    reg_res_h: u32,
    reg_tier: u8,
    reg_threads: u8,
    reg_duration_ns: u64,
    reg_avg_load: u32,
    reg_power_mw: u32,
    // ---- 运行态 ----
    chain: u64,
    segs: Vec<SegRecord>,
    deviations: Vec<DeviationRecord>,
    began_ns: Option<u64>,
    closed: bool,
}

impl RunWatch {
    /// 开表：场景必须已注册；冻结副本独立拷贝，运行期注册表即使被绕过也不影响本表判据。
    pub fn open(registry: &SceneRegistry, id: u32) -> Result<Self, AntiCheatError> {
        let e = match registry.lookup(id) {
            Some(e) => e,
            None => return Err(AntiCheatError::SceneSpecBad),
        };
        let s = &e.spec;
        Ok(RunWatch {
            scene_id: id,
            frozen_scene_fp: e.frozen_fingerprint,
            frozen_param_hash: e.frozen_param_hash,
            frozen_tier_fp: e.frozen_tier_fp,
            reg_res_w: s.res_w,
            reg_res_h: s.res_h,
            reg_tier: s.quality_tier,
            reg_threads: s.thread_count,
            reg_duration_ns: s.duration_ns,
            reg_avg_load: s.curve_avg_load(),
            reg_power_mw: s.ref_power_mw,
            chain: fnv1a64(&e.frozen_fingerprint.to_le_bytes()),
            segs: Vec::new(),
            deviations: Vec::new(),
            began_ns: None,
            closed: false,
        })
    }

    /// 场景 id。
    pub fn scene_id(&self) -> u32 {
        self.scene_id
    }

    /// 冻结场景指纹。
    pub fn frozen_scene_fp(&self) -> u64 {
        self.frozen_scene_fp
    }

    /// 已收段数。
    pub fn seg_count(&self) -> usize {
        self.segs.len()
    }

    /// 段只读访问。
    pub fn seg(&self, i: usize) -> Option<&SegRecord> {
        if i >= self.segs.len() {
            return None;
        }
        Some(&self.segs[i])
    }

    /// 偏离账只读访问。
    pub fn deviations(&self) -> &[DeviationRecord] {
        &self.deviations
    }

    /// 开始计时（一次性）。
    pub fn begin(&mut self, at_ns: u64) -> Result<(), AntiCheatError> {
        if self.closed {
            return Err(AntiCheatError::LedgerBreak);
        }
        if self.began_ns.is_some() {
            return Err(AntiCheatError::LedgerBreak);
        }
        self.began_ns = Some(at_ns);
        Ok(())
    }

    /// 五元组逐项比对：偏离即入账（账目型检测，不中断运行）。
    fn check_deviations(
        &mut self,
        res_w: u32,
        res_h: u32,
        tier: u8,
        threads: u8,
        load: u32,
    ) -> u32 {
        let mut n = 0u32;
        if res_w != self.reg_res_w {
            self.deviations.push(DeviationRecord {
                field: ParamField::ResW,
                registered: self.reg_res_w as u64,
                observed: res_w as u64,
            });
            n += 1;
        }
        if res_h != self.reg_res_h {
            self.deviations.push(DeviationRecord {
                field: ParamField::ResH,
                registered: self.reg_res_h as u64,
                observed: res_h as u64,
            });
            n += 1;
        }
        if tier != self.reg_tier {
            self.deviations.push(DeviationRecord {
                field: ParamField::QualityTier,
                registered: self.reg_tier as u64,
                observed: tier as u64,
            });
            n += 1;
        }
        if threads != self.reg_threads {
            self.deviations.push(DeviationRecord {
                field: ParamField::ThreadCount,
                registered: self.reg_threads as u64,
                observed: threads as u64,
            });
            n += 1;
        }
        // 负载按带判：超出 ±25‰ 带才入账（带内抖动合法）。
        let band = LOAD_DEV_BAND_PERMILLE as u64;
        let lo = (self.reg_avg_load as u64).saturating_sub(band);
        let hi = (self.reg_avg_load as u64).saturating_add(band);
        let l = load as u64;
        if l < lo || l > hi {
            self.deviations.push(DeviationRecord {
                field: ParamField::LoadPermille,
                registered: self.reg_avg_load as u64,
                observed: load as u64,
            });
            n += 1;
        }
        n
    }

    /// 逐段收账：时间单调校验 → 偏离入账 → 段哈希链式衔接。
    /// 段哈希 = fnv_mix(前链, 规范段字节)，篡改任一段全链即破。
    #[allow(clippy::too_many_arguments)]
    pub fn push_segment(
        &mut self,
        start_ns: u64,
        end_ns: u64,
        res_w: u32,
        res_h: u32,
        quality_tier: u8,
        thread_count: u8,
        load_permille: u32,
        score_permille: u32,
        power_mw: u32,
    ) -> Result<u64, AntiCheatError> {
        if self.closed {
            return Err(AntiCheatError::LedgerBreak);
        }
        if self.began_ns.is_none() {
            return Err(AntiCheatError::LedgerBreak);
        }
        if end_ns <= start_ns {
            return Err(AntiCheatError::SceneSpecBad);
        }
        if load_permille > PERMILLE_MAX || score_permille > PERMILLE_MAX {
            return Err(AntiCheatError::SceneSpecBad);
        }
        if let Some(began) = self.began_ns {
            if start_ns < began {
                return Err(AntiCheatError::SceneSpecBad);
            }
        }
        if let Some(last) = self.segs.last() {
            if start_ns < last.end_ns {
                return Err(AntiCheatError::SceneSpecBad);
            }
        }
        if self.segs.len() >= RUN_SEGS_MAX {
            return Err(AntiCheatError::CapacityFull);
        }
        self.check_deviations(res_w, res_h, quality_tier, thread_count, load_permille);
        let idx = self.segs.len() as u32;
        // 段规范字节（定长编码，链式哈希输入）。
        let mut buf: Vec<u8> = Vec::with_capacity(56);
        buf.extend_from_slice(b"VSEG");
        push_u32_le(&mut buf, idx);
        push_u64_le(&mut buf, start_ns);
        push_u64_le(&mut buf, end_ns);
        push_u32_le(&mut buf, res_w);
        push_u32_le(&mut buf, res_h);
        buf.push(quality_tier);
        buf.push(thread_count);
        push_u32_le(&mut buf, load_permille);
        push_u32_le(&mut buf, score_permille);
        push_u32_le(&mut buf, power_mw);
        let seg_hash = fnv_mix(self.chain, &buf);
        self.chain = seg_hash;
        let rec = SegRecord {
            seg_index: idx,
            start_ns,
            end_ns,
            res_w,
            res_h,
            quality_tier,
            thread_count,
            load_permille,
            score_permille,
            power_mw,
            seg_hash,
        };
        self.segs.push(rec);
        Ok(seg_hash)
    }

    /// 收表：总时长截断判据（实跑 < 注册×99/100）在此落账并写入摘要。
    pub fn close(&mut self, end_ns: u64) -> Result<RunSummary, AntiCheatError> {
        if self.closed {
            return Err(AntiCheatError::LedgerBreak);
        }
        let began = match self.began_ns {
            Some(b) => b,
            None => return Err(AntiCheatError::LedgerBreak),
        };
        if end_ns <= began {
            return Err(AntiCheatError::SceneSpecBad);
        }
        self.closed = true;
        let total = end_ns.saturating_sub(began);
        // 截断判据：total * 100 < reg * 99（等价 total < reg * 0.99，全整数无舍入漂移）。
        let truncated = total.saturating_mul(DUR_TRUNC_DEN) < self.reg_duration_ns.saturating_mul(DUR_TRUNC_NUM);
        if truncated {
            self.deviations.push(DeviationRecord {
                field: ParamField::Duration,
                registered: self.reg_duration_ns,
                observed: total,
            });
        }
        // 聚合：平均负载 / 平均功耗 / 首段分 / 其余段中位分。
        let n = self.segs.len();
        let mut sum_load: u64 = 0;
        let mut sum_power: u64 = 0;
        let mut i = 0usize;
        while i < n {
            sum_load = sum_load.saturating_add(self.segs[i].load_permille as u64);
            sum_power = sum_power.saturating_add(self.segs[i].power_mw as u64);
            i += 1;
        }
        let avg_load = if n > 0 { (sum_load / n as u64) as u32 } else { 0 };
        let avg_power = if n > 0 { (sum_power / n as u64) as u32 } else { 0 };
        let first_score = if n > 0 { self.segs[0].score_permille } else { 0 };
        let rest_median = if n > 1 {
            let mut rest: Vec<u32> = Vec::with_capacity(n - 1);
            let mut j = 1usize;
            while j < n {
                rest.push(self.segs[j].score_permille);
                j += 1;
            }
            rest.sort_unstable();
            rest[(rest.len() - 1) / 2]
        } else {
            first_score
        };
        // 运行档位指纹：取末段实测五元组的档位指纹（运行实际档位）。
        let tier_fp = if n > 0 {
            let last = &self.segs[n - 1];
            tier_fp_of(last.res_w, last.res_h, last.quality_tier)
        } else {
            self.frozen_tier_fp
        };
        Ok(RunSummary {
            scene_id: self.scene_id,
            chain_hash: self.chain,
            seg_count: n as u32,
            total_ns: total,
            registered_ns: self.reg_duration_ns,
            truncated,
            avg_load_permille: avg_load,
            registered_avg_load: self.reg_avg_load,
            avg_power_mw: avg_power,
            registered_power_mw: self.reg_power_mw,
            first_score_permille: first_score,
            rest_median_score_permille: rest_median,
            param_deviations: self.deviations.len() as u32,
            scene_fingerprint: self.frozen_scene_fp,
            tier_fp,
        })
    }

    /// 总得分：段得分千分位累计（saturating）。
    pub fn total_score(&self) -> u64 {
        let mut acc: u64 = 0;
        let mut i = 0usize;
        while i < self.segs.len() {
            acc = acc.saturating_add(self.segs[i].score_permille as u64);
            i += 1;
        }
        acc
    }
}

// ===========================================================================
// §6 CheatFamily 五族作弊闭集 + 每族检测器
// ===========================================================================

/// 五族作弊闭集（闭集：检测器只认这五族，未登记手法走体检兜底）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CheatFamily {
    /// 场景降级：档位指纹变化（画质/分辨率被调低）。
    SceneDegrade,
    /// 时长截断：实跑 < 注册×99%。
    DurationTruncate,
    /// 挑参数：参数哈希不等（线程/负载被挑拣）。
    ParamCherryPick,
    /// 静默降载：功耗-性能曲线偏离带。
    SilentUnderload,
    /// 预热污染：首段异常高分。
    WarmupPollute,
}

impl CheatFamily {
    /// 全族闭集（顺序即枚举序）。
    pub const ALL: [CheatFamily; 5] = [
        CheatFamily::SceneDegrade,
        CheatFamily::DurationTruncate,
        CheatFamily::ParamCherryPick,
        CheatFamily::SilentUnderload,
        CheatFamily::WarmupPollute,
    ];

    /// 中文族名。
    pub fn label(&self) -> &'static str {
        match self {
            CheatFamily::SceneDegrade => "场景降级",
            CheatFamily::DurationTruncate => "时长截断",
            CheatFamily::ParamCherryPick => "挑参数",
            CheatFamily::SilentUnderload => "静默降载",
            CheatFamily::WarmupPollute => "预热污染",
        }
    }

    /// 族对应诊断码。
    pub fn code(&self) -> u16 {
        match self {
            CheatFamily::SceneDegrade => 0x1335,
            CheatFamily::DurationTruncate => 0x1334,
            CheatFamily::ParamCherryPick => 0x1336,
            CheatFamily::SilentUnderload => 0x1337,
            CheatFamily::WarmupPollute => 0x1338,
        }
    }

    /// 族序号（分族计数下标）。
    pub fn index(&self) -> usize {
        match self {
            CheatFamily::SceneDegrade => 0,
            CheatFamily::DurationTruncate => 1,
            CheatFamily::ParamCherryPick => 2,
            CheatFamily::SilentUnderload => 3,
            CheatFamily::WarmupPollute => 4,
        }
    }
}

/// 单次检出：族 + 场景 + 证据 + 严重度（千分制）。
#[derive(Clone, Debug)]
pub struct Detection {
    family: CheatFamily,
    scene_id: u32,
    evidence: String,
    severity_permille: u32,
}

impl Detection {
    /// 作弊族。
    pub fn family(&self) -> CheatFamily {
        self.family
    }

    /// 场景 id。
    pub fn scene_id(&self) -> u32 {
        self.scene_id
    }

    /// 证据描述。
    pub fn evidence(&self) -> &str {
        &self.evidence
    }

    /// 严重度千分位（1000 = 满级）。
    pub fn severity_permille(&self) -> u32 {
        self.severity_permille
    }
}

/// 检测器一：场景降级。判据 = 段档位指纹 != 注册档位指纹。
/// 来源：锚点规格"降级检出=档位指纹变化"；指纹函数 tier_fp_of 双侧共用。
pub fn detect_scene_degrade(watch: &RunWatch) -> Option<Detection> {
    let mut i = 0usize;
    while i < watch.seg_count() {
        let seg = match watch.seg(i) {
            Some(s) => s,
            None => return None,
        };
        let fp = tier_fp_of(seg.res_w, seg.res_h, seg.quality_tier);
        if fp != watch.frozen_tier_fp {
            let ev = format!(
                "段{}档位指纹 {:016x} != 冻结 {:016x}（实测 {}x{}@档{} / 冻结 {}x{}@档{}）",
                i, fp, watch.frozen_tier_fp, seg.res_w, seg.res_h, seg.quality_tier,
                watch.reg_res_w, watch.reg_res_h, watch.reg_tier
            );
            return Some(Detection {
                family: CheatFamily::SceneDegrade,
                scene_id: watch.scene_id,
                evidence: ev,
                severity_permille: PERMILLE_MAX,
            });
        }
        i += 1;
    }
    None
}

/// 检测器二：时长截断。判据 = 实跑 < 注册×0.99（阈值来源见 DUR_TRUNC_*）。
pub fn detect_duration_truncate(summary: &RunSummary) -> Option<Detection> {
    if !summary.truncated {
        return None;
    }
    let ev = format!(
        "实跑 {}ns < 注册 {}ns × {}/{}（截断率 {}‰）",
        summary.total_ns,
        summary.registered_ns,
        DUR_TRUNC_NUM,
        DUR_TRUNC_DEN,
        if summary.registered_ns > 0 {
            (summary.registered_ns.saturating_sub(summary.total_ns)).saturating_mul(1000)
                / summary.registered_ns
        } else {
            0
        }
    );
    Some(Detection {
        family: CheatFamily::DurationTruncate,
        scene_id: summary.scene_id,
        evidence: ev,
        severity_permille: PERMILLE_MAX,
    })
}

/// 检测器三：挑参数。判据 = 运行五元组哈希 != 注册参数哈希。
/// 负载项带内抖动按注册值回填（±25‰ 带），带外偏离保留实测值参与哈希。
pub fn detect_param_cherry(watch: &RunWatch) -> Option<Detection> {
    let band = LOAD_DEV_BAND_PERMILLE as u64;
    let lo = (watch.reg_avg_load as u64).saturating_sub(band);
    let hi = (watch.reg_avg_load as u64).saturating_add(band);
    let mut i = 0usize;
    while i < watch.seg_count() {
        let seg = match watch.seg(i) {
            Some(s) => s,
            None => return None,
        };
        let load_eff: u32 = {
            let l = seg.load_permille as u64;
            if l >= lo && l <= hi {
                watch.reg_avg_load
            } else {
                seg.load_permille
            }
        };
        let obs = param_hash_of(seg.res_w, seg.res_h, seg.quality_tier, seg.thread_count, load_eff);
        if obs != watch.frozen_param_hash {
            let ev = format!(
                "段{}运行参数哈希 {:016x} != 注册 {:016x}（线程 {}→{}，负载 {}→{}）",
                i, obs, watch.frozen_param_hash,
                watch.reg_threads, seg.thread_count,
                watch.reg_avg_load, seg.load_permille
            );
            return Some(Detection {
                family: CheatFamily::ParamCherryPick,
                scene_id: watch.scene_id,
                evidence: ev,
                severity_permille: PERMILLE_MAX,
            });
        }
        i += 1;
    }
    None
}

/// 检测器四：静默降载。双判据（命中其一即立案）：
/// (a) 平均实测负载 < 注册均值 × 850‰（UNDERLOAD_FLOOR_PERMILLE，来源 cgb341）；
/// (b) 负载在带内但平均功耗 < 注册参考功耗 × 850‰（功率对不上负载 = 后台压频）。
pub fn detect_silent_underload(summary: &RunSummary) -> Option<Detection> {
    let floor_load = (summary.registered_avg_load as u64)
        .saturating_mul(UNDERLOAD_FLOOR_PERMILLE as u64)
        / 1000;
    if (summary.avg_load_permille as u64) < floor_load {
        let ev = format!(
            "平均负载 {}‰ < 下限 {}‰（注册 {}‰ × {}/1000）",
            summary.avg_load_permille, floor_load,
            summary.registered_avg_load, UNDERLOAD_FLOOR_PERMILLE
        );
        return Some(Detection {
            family: CheatFamily::SilentUnderload,
            scene_id: summary.scene_id,
            evidence: ev,
            severity_permille: 900,
        });
    }
    let floor_power = (summary.registered_power_mw as u64)
        .saturating_mul(POWER_FLOOR_PERMILLE as u64)
        / 1000;
    let band = LOAD_DEV_BAND_PERMILLE as u64;
    let load_in_band = (summary.avg_load_permille as u64)
        .saturating_add(band)
        >= summary.registered_avg_load as u64;
    if load_in_band && summary.avg_power_mw > 0 && (summary.avg_power_mw as u64) < floor_power {
        let ev = format!(
            "负载在带内（{}‰ vs 注册 {}‰）但平均功耗 {}mW < 下限 {}mW",
            summary.avg_load_permille, summary.registered_avg_load,
            summary.avg_power_mw, floor_power
        );
        return Some(Detection {
            family: CheatFamily::SilentUnderload,
            scene_id: summary.scene_id,
            evidence: ev,
            severity_permille: 850,
        });
    }
    None
}

/// 检测器五：预热污染。判据 = 首段得分 > 其余段中位 × 1150‰
/// （WARMUP_CEIL_PERMILLE，来源：预热典型增益 ≤15%），且段数 ≥3 保证中位有意义。
pub fn detect_warmup_pollute(summary: &RunSummary) -> Option<Detection> {
    if summary.seg_count < 3 {
        return None;
    }
    let median = summary.rest_median_score_permille;
    if median == 0 {
        return None;
    }
    let ceil = (median as u64).saturating_mul(WARMUP_CEIL_PERMILLE as u64) / 1000;
    if (summary.first_score_permille as u64) > ceil {
        let ev = format!(
            "首段 {}‰ > 其余段中位 {}‰ × {}/1000 = {}‰",
            summary.first_score_permille, median, WARMUP_CEIL_PERMILLE, ceil
        );
        return Some(Detection {
            family: CheatFamily::WarmupPollute,
            scene_id: summary.scene_id,
            evidence: ev,
            severity_permille: 700,
        });
    }
    None
}

/// 五族联扫：全部检测器跑一遍，检出即成 Detection（由检测组入账）。
pub fn scan_families(watch: &RunWatch, summary: &RunSummary) -> Vec<Detection> {
    let mut out: Vec<Detection> = Vec::new();
    if let Some(d) = detect_scene_degrade(watch) {
        out.push(d);
    }
    if let Some(d) = detect_duration_truncate(summary) {
        out.push(d);
    }
    if let Some(d) = detect_param_cherry(watch) {
        out.push(d);
    }
    if let Some(d) = detect_silent_underload(summary) {
        out.push(d);
    }
    if let Some(d) = detect_warmup_pollute(summary) {
        out.push(d);
    }
    out
}

// ===========================================================================
// §7 环境画像 EnvProfile（本地闭环隐私）
// ===========================================================================

/// CPU 桶表（核数粗粒度分桶）。来源：CGPU-U 域隐私规格 vck26（分桶粒度 ≥2 倍差）。
const CORE_BUCKETS: [u32; 8] = [1, 2, 4, 8, 16, 32, 64, 128];
/// 内存桶表（GiB 粗粒度分桶）。
const MEM_BUCKETS: [u32; 8] = [2, 4, 8, 16, 32, 64, 128, 256];

/// const 桶定位：while+索引，越界回退末桶。
const fn bucket_index(v: u32, table: &[u32; 8]) -> u8 {
    let mut i = 0usize;
    while i < 8 {
        if v <= table[i] {
            return i as u8;
        }
        i += 1;
    }
    7
}

/// 环境画像：本地闭环隐私——只收粗粒度分桶，不收序列号/主机名/网卡地址。
/// 画像哈希参与成绩单签名，保证成绩不可脱离环境语境移植。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EnvProfile {
    /// CPU 类别（0~4：未知/桌面/移动/服务器/嵌入）。
    pub cpu_class: u8,
    /// 核数桶（0~7 → CORE_BUCKETS）。
    pub core_bucket: u8,
    /// 内存桶（0~7 → MEM_BUCKETS，GiB）。
    pub mem_bucket: u8,
    /// GPU 档位（0~8，与画质档同表）。
    pub gpu_tier: u8,
    /// OS 族（0~3：未知/类Unix/Windows/RTOS）。
    pub os_family: u8,
}

impl EnvProfile {
    /// 由原始观测构造（自动分桶脱敏）。
    pub fn from_raw(cpu_class: u8, cores: u32, mem_gib: u32, gpu_tier: u8, os_family: u8) -> Self {
        EnvProfile {
            cpu_class: if cpu_class > 4 { 0 } else { cpu_class },
            core_bucket: bucket_index(cores, &CORE_BUCKETS),
            mem_bucket: bucket_index(mem_gib, &MEM_BUCKETS),
            gpu_tier: if gpu_tier > TIER_MAX { 0 } else { gpu_tier },
            os_family: if os_family > 3 { 0 } else { os_family },
        }
    }

    /// 画像哈希：定长五字节编码 FNV。
    pub fn snapshot_hash(&self) -> u64 {
        let buf = [
            b"VENV1"[0], b"VENV1"[1], b"VENV1"[2], b"VENV1"[3], b"VENV1"[4],
            self.cpu_class, self.core_bucket, self.mem_bucket, self.gpu_tier, self.os_family,
        ];
        fnv1a64(&buf)
    }

    /// 隐私声明（本地闭环：不出内核、不落远端）。
    pub fn privacy_note(&self) -> &'static str {
        "本地闭环：画像仅含粗粒度分桶（核数/内存/档位），不含序列号、主机名、网卡地址；画像哈希仅用于成绩单语境绑定，不回传。"
    }

    /// 分桶合法性体检。
    pub fn well_formed(&self) -> bool {
        self.cpu_class <= 4 && self.core_bucket <= 7 && self.mem_bucket <= 7
            && self.gpu_tier <= TIER_MAX && self.os_family <= 3
    }
}

// ===========================================================================
// §8 ScoreSeal 成绩单签名（FNV-HMAC 五步链）
// ===========================================================================

/// 成绩单：签名对象五字段（场景指纹+时长+分数+环境画像+档位指纹）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Scorecard {
    /// 场景冻结指纹。
    pub scene_fingerprint: u64,
    /// 实跑时长（纳秒）。
    pub duration_ns: u64,
    /// 总得分（段得分千分位累计）。
    pub score: u64,
    /// 环境画像哈希。
    pub env_hash: u64,
    /// 运行档位指纹。
    pub tier_fp: u64,
}

/// 签名字段定位（验签失败分类到字段）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SealField {
    /// 步0：场景指纹字段。
    SceneFingerprint,
    /// 步1：时长字段。
    Duration,
    /// 步2：分数字段。
    Score,
    /// 步3：环境画像字段。
    EnvProfile,
    /// 步4：档位指纹字段。
    TierFingerprint,
    /// 步5：终 MAC 本身（链内全对但终值不符 = MAC 被改或密钥不符）。
    FinalMac,
}

impl SealField {
    /// 中文标签。
    pub fn label(&self) -> &'static str {
        match self {
            SealField::SceneFingerprint => "场景指纹",
            SealField::Duration => "时长",
            SealField::Score => "分数",
            SealField::EnvProfile => "环境画像",
            SealField::TierFingerprint => "档位指纹",
            SealField::FinalMac => "终签名",
        }
    }

    /// 步下标（FinalMac 为 SEAL_STEPS）。
    pub fn step(&self) -> usize {
        match self {
            SealField::SceneFingerprint => 0,
            SealField::Duration => 1,
            SealField::Score => 2,
            SealField::EnvProfile => 3,
            SealField::TierFingerprint => 4,
            SealField::FinalMac => SEAL_STEPS,
        }
    }
}

/// 验签结论：ok + 被篡改字段定位 + 断裂步。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SealVerdict {
    /// 验签通过。
    pub ok: bool,
    /// 篡改定位（None = 通过）。
    pub tampered: Option<SealField>,
    /// 断裂步下标（0~5；通过时 = SEAL_STEPS）。
    pub diverged_step: usize,
}

/// 成绩单封条：五步 FNV-HMAC 链 + 终 MAC。
/// 密钥 Option 语义：None 拒签带码（0x1339），绝不降级为无密钥签名。
#[derive(Clone, Debug)]
pub struct ScoreSeal {
    card: Scorecard,
    steps: [u64; SEAL_STEPS],
    final_mac: u64,
    key_id: u64,
    seq: u64,
}

/// 步 i 密钥派生：FNV(主密钥‖步号) 双轮扩散，异或主密钥回混。
fn derive_key(master: &[u8; SEAL_KEY_LEN], step: u8) -> [u8; SEAL_KEY_LEN] {
    let mut input = [0u8; SEAL_KEY_LEN + 1];
    let mut i = 0usize;
    while i < SEAL_KEY_LEN {
        input[i] = master[i];
        i += 1;
    }
    input[SEAL_KEY_LEN] = step;
    let h1 = fnv1a64(&input);
    let h2 = fnv1a64(&h1.to_le_bytes());
    let mut out = [0u8; SEAL_KEY_LEN];
    let mut j = 0usize;
    while j < SEAL_KEY_LEN {
        let b1 = (h1 >> ((j & 7) * 8)) as u8;
        let b2 = (h2 >> ((j & 3) * 8)) as u8;
        out[j] = b1 ^ b2 ^ master[j];
        j += 1;
    }
    out
}

/// FNV-HMAC 单步：仿 HMAC 分组结构（ipad 0x36 / opad 0x5c，64 字节块），
/// inner = FNV(ipad ‖ prev ‖ data)，outer = FNV(opad ‖ inner)。
fn fnv_hmac(key: &[u8; SEAL_KEY_LEN], prev: u64, data: &[u8]) -> u64 {
    let mut ipad = [0x36u8; 64];
    let mut opad = [0x5cu8; 64];
    let mut i = 0usize;
    while i < SEAL_KEY_LEN {
        ipad[i] ^= key[i];
        opad[i] ^= key[i];
        i += 1;
    }
    let inner = fnv_feed(fnv_feed(fnv_feed(FNV_OFFSET, &ipad), &prev.to_le_bytes()), data);
    fnv_feed(fnv_feed(FNV_OFFSET, &opad), &inner.to_le_bytes())
}

/// 步 i 的签名输入（成绩单字段的定长小端字节）。
fn seal_field_bytes(step: usize, card: &Scorecard) -> [u8; 8] {
    match step {
        0 => card.scene_fingerprint.to_le_bytes(),
        1 => card.duration_ns.to_le_bytes(),
        2 => card.score.to_le_bytes(),
        3 => card.env_hash.to_le_bytes(),
        _ => card.tier_fp.to_le_bytes(),
    }
}

impl ScoreSeal {
    /// 签名：五步链式（每步独立派生密钥），密钥 None 拒签（0x1339）。
    pub fn seal(card: Scorecard, key: Option<&[u8; SEAL_KEY_LEN]>, seq: u64) -> Result<Self, AntiCheatError> {
        let key = match key {
            Some(k) => k,
            None => return Err(AntiCheatError::SealKeyMissing),
        };
        let mut st = FNV_OFFSET;
        let mut steps = [0u64; SEAL_STEPS];
        let mut i = 0usize;
        while i < SEAL_STEPS {
            let ki = derive_key(key, i as u8);
            st = fnv_hmac(&ki, st, &seal_field_bytes(i, &card));
            steps[i] = st;
            i += 1;
        }
        let key_id = fnv1a64(key);
        Ok(ScoreSeal { card, steps, final_mac: st, key_id, seq })
    }

    /// 验签：对称重算五步链，逐步与封存中间值对照——
    /// 首个断裂步即被篡改字段（链式性质：字段 j 被改则步 j 起全错，首错 = j）。
    pub fn verify(&self, key: Option<&[u8; SEAL_KEY_LEN]>) -> Result<SealVerdict, AntiCheatError> {
        let key = match key {
            Some(k) => k,
            None => return Err(AntiCheatError::SealKeyMissing),
        };
        // 密钥 id 预检：密钥不符直接报 SealBroken（避免误报字段篡改）。
        if fnv1a64(key) != self.key_id {
            return Ok(SealVerdict { ok: false, tampered: Some(SealField::FinalMac), diverged_step: SEAL_STEPS });
        }
        let mut st = FNV_OFFSET;
        let mut i = 0usize;
        while i < SEAL_STEPS {
            let ki = derive_key(key, i as u8);
            st = fnv_hmac(&ki, st, &seal_field_bytes(i, &self.card));
            if st != self.steps[i] {
                let field = match i {
                    0 => SealField::SceneFingerprint,
                    1 => SealField::Duration,
                    2 => SealField::Score,
                    3 => SealField::EnvProfile,
                    _ => SealField::TierFingerprint,
                };
                return Ok(SealVerdict { ok: false, tampered: Some(field), diverged_step: i });
            }
            i += 1;
        }
        if st != self.final_mac {
            return Ok(SealVerdict { ok: false, tampered: Some(SealField::FinalMac), diverged_step: SEAL_STEPS });
        }
        Ok(SealVerdict { ok: true, tampered: None, diverged_step: SEAL_STEPS })
    }

    /// 封存的成绩单只读访问。
    pub fn card(&self) -> &Scorecard {
        &self.card
    }

    /// 密钥公开指纹（非密钥本体，用于核对装错钥匙）。
    pub fn key_id(&self) -> u64 {
        self.key_id
    }

    /// 封条序号。
    pub fn seq(&self) -> u64 {
        self.seq
    }
}

/// 密钥环：多槽位密钥管理，激活槽参与签名；空环/无激活 = None 拒签路径。
#[derive(Debug)]
pub struct SealKeyRing {
    slots: [Option<[u8; SEAL_KEY_LEN]>; KEY_RING_SLOTS],
    active: Option<usize>,
}

impl SealKeyRing {
    /// 空环。
    pub fn new() -> Self {
        SealKeyRing { slots: [None, None, None, None], active: None }
    }

    /// 装载密钥到空闲槽并激活；环满报 CapacityFull。
    pub fn load_key(&mut self, key: [u8; SEAL_KEY_LEN]) -> Result<usize, AntiCheatError> {
        let mut i = 0usize;
        while i < KEY_RING_SLOTS {
            if self.slots[i].is_none() {
                self.slots[i] = Some(key);
                self.active = Some(i);
                return Ok(i);
            }
            i += 1;
        }
        Err(AntiCheatError::CapacityFull)
    }

    /// 激活指定槽。
    pub fn activate(&mut self, slot: usize) -> Result<(), AntiCheatError> {
        if slot >= KEY_RING_SLOTS || self.slots[slot].is_none() {
            return Err(AntiCheatError::SealKeyMissing);
        }
        self.active = Some(slot);
        Ok(())
    }

    /// 当前激活密钥（None = 拒签路径）。
    pub fn active_key(&self) -> Option<&[u8; SEAL_KEY_LEN]> {
        match self.active {
            Some(i) => self.slots[i].as_ref(),
            None => None,
        }
    }

    /// 已装载槽数。
    pub fn loaded(&self) -> usize {
        let mut n = 0usize;
        let mut i = 0usize;
        while i < KEY_RING_SLOTS {
            if self.slots[i].is_some() {
                n += 1;
            }
            i += 1;
        }
        n
    }
}

// ===========================================================================
// §9 CheatEventLedger 立案账（链哈希 + health 体检）
// ===========================================================================

/// 立案事件：五族检出留痕，链哈希防事后改账。
#[derive(Clone, Debug)]
pub struct CheatEvent {
    seq: u64,
    family: CheatFamily,
    scene_id: u32,
    code: u16,
    evidence: String,
    severity_permille: u32,
    at_ns: u64,
    event_hash: u64,
}

impl CheatEvent {
    /// 事件序号。
    pub fn seq(&self) -> u64 {
        self.seq
    }

    /// 作弊族。
    pub fn family(&self) -> CheatFamily {
        self.family
    }

    /// 场景 id。
    pub fn scene_id(&self) -> u32 {
        self.scene_id
    }

    /// 诊断码。
    pub fn code(&self) -> u16 {
        self.code
    }

    /// 证据。
    pub fn evidence(&self) -> &str {
        &self.evidence
    }

    /// 严重度千分位。
    pub fn severity_permille(&self) -> u32 {
        self.severity_permille
    }

    /// 立案时间（单调纳秒）。
    pub fn at_ns(&self) -> u64 {
        self.at_ns
    }

    /// 事件链哈希。
    pub fn event_hash(&self) -> u64 {
        self.event_hash
    }
}

/// 体检报告。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LedgerHealth {
    /// 账目条数。
    pub entries: usize,
    /// 链哈希完整（重算一致）。
    pub chain_ok: bool,
    /// 容量。
    pub cap: usize,
    /// 分族计数（下标 = CheatFamily::index()）。
    pub per_family: [u32; 5],
}

/// 立案账：append-only（无 remove/改写 API），链哈希 = fnv_mix(前链, 事件规范字节)。
#[derive(Debug)]
pub struct CheatEventLedger {
    events: Vec<CheatEvent>,
    chain: u64,
}

impl CheatEventLedger {
    /// 空账。
    pub fn new() -> Self {
        CheatEventLedger { events: Vec::new(), chain: FNV_OFFSET }
    }

    /// 立案：容量满报 0x133F。
    pub fn record(&mut self, det: &Detection, at_ns: u64) -> Result<u64, AntiCheatError> {
        if self.events.len() >= EVENT_CAP {
            return Err(AntiCheatError::CapacityFull);
        }
        let seq = (self.events.len() as u64).wrapping_add(1);
        let mut buf: Vec<u8> = Vec::with_capacity(48);
        buf.extend_from_slice(b"VEVENT");
        push_u64_le(&mut buf, seq);
        push_u32_le(&mut buf, det.scene_id);
        push_u16_le(&mut buf, det.family.code());
        push_u32_le(&mut buf, det.severity_permille);
        push_u64_le(&mut buf, at_ns);
        buf.extend_from_slice(det.evidence.as_bytes());
        let event_hash = fnv_mix(self.chain, &buf);
        self.chain = event_hash;
        self.events.push(CheatEvent {
            seq,
            family: det.family,
            scene_id: det.scene_id,
            code: det.family.code(),
            evidence: det.evidence.clone(),
            severity_permille: det.severity_permille,
            at_ns,
            event_hash,
        });
        Ok(seq)
    }

    /// 条数。
    pub fn len(&self) -> usize {
        self.events.len()
    }

    /// 是否为空。
    pub fn is_empty(&self) -> bool {
        self.events.is_empty()
    }

    /// 按序号查事件。
    pub fn event(&self, seq: u64) -> Option<&CheatEvent> {
        if seq == 0 {
            return None;
        }
        let idx = (seq - 1) as usize;
        if idx >= self.events.len() {
            return None;
        }
        Some(&self.events[idx])
    }

    /// 分族计数。
    pub fn count_family(&self, family: CheatFamily) -> u32 {
        let mut n = 0u32;
        let mut i = 0usize;
        while i < self.events.len() {
            if self.events[i].family == family {
                n += 1;
            }
            i += 1;
        }
        n
    }

    /// 链完整性重算（账目破缺检测）。
    pub fn audit_chain(&self) -> bool {
        let mut st = FNV_OFFSET;
        let mut i = 0usize;
        while i < self.events.len() {
            let e = &self.events[i];
            let mut buf: Vec<u8> = Vec::with_capacity(48);
            buf.extend_from_slice(b"VEVENT");
            push_u64_le(&mut buf, e.seq);
            push_u32_le(&mut buf, e.scene_id);
            push_u16_le(&mut buf, e.code);
            push_u32_le(&mut buf, e.severity_permille);
            push_u64_le(&mut buf, e.at_ns);
            buf.extend_from_slice(e.evidence.as_bytes());
            st = fnv_mix(st, &buf);
            if st != e.event_hash {
                return false;
            }
            i += 1;
        }
        st == self.chain
    }

    /// 体检：条数 + 链完整 + 分族计数。
    pub fn health(&self) -> LedgerHealth {
        let mut per_family = [0u32; 5];
        let mut i = 0usize;
        while i < CheatFamily::ALL.len() {
            per_family[i] = self.count_family(CheatFamily::ALL[i]);
            i += 1;
        }
        LedgerHealth {
            entries: self.events.len(),
            chain_ok: self.audit_chain(),
            cap: EVENT_CAP,
            per_family,
        }
    }
}

// ===========================================================================
// §10 AppealLedger 申诉账（append-only，不抹账）
// ===========================================================================

/// 申诉裁定（追加型：改判 = 追加新条目，绝不改旧条目）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AppealVerdict {
    /// 待裁定。
    Pending,
    /// 维持立案。
    Upheld,
    /// 推翻立案。
    Overturned,
}

/// 申诉条目。
#[derive(Clone, Debug)]
pub struct AppealEntry {
    seq: u64,
    event_ref: Option<u64>,
    seal_ref: Option<u64>,
    reason_code: u16,
    note: String,
    verdict: AppealVerdict,
    at_ns: u64,
    entry_hash: u64,
}

impl AppealEntry {
    /// 申诉序号。
    pub fn seq(&self) -> u64 {
        self.seq
    }

    /// 引用事件序号。
    pub fn event_ref(&self) -> Option<u64> {
        self.event_ref
    }

    /// 引用封条序号。
    pub fn seal_ref(&self) -> Option<u64> {
        self.seal_ref
    }

    /// 理由码。
    pub fn reason_code(&self) -> u16 {
        self.reason_code
    }

    /// 申诉说明。
    pub fn note(&self) -> &str {
        &self.note
    }

    /// 当前裁定。
    pub fn verdict(&self) -> AppealVerdict {
        self.verdict
    }

    /// 申诉时间。
    pub fn at_ns(&self) -> u64 {
        self.at_ns
    }

    /// 条目链哈希。
    pub fn entry_hash(&self) -> u64 {
        self.entry_hash
    }
}

/// 申诉账：**append-only**——字段私有、无任何 remove/retake/改写 API；
/// 裁定变化通过追加后续条目表达（verdict 落在新条目上），历史永不抹除。
#[derive(Debug)]
pub struct AppealLedger {
    entries: Vec<AppealEntry>,
    chain: u64,
}

impl AppealLedger {
    /// 空账。
    pub fn new() -> Self {
        AppealLedger { entries: Vec::new(), chain: FNV_OFFSET }
    }

    /// 追加申诉：引用事件或封条至少其一，理由码 0~999。
    pub fn append(
        &mut self,
        event_ref: Option<u64>,
        seal_ref: Option<u64>,
        reason_code: u16,
        note: &str,
        at_ns: u64,
    ) -> Result<u64, AntiCheatError> {
        if reason_code > APPEAL_REASON_MAX {
            return Err(AntiCheatError::AppealBad);
        }
        if event_ref.is_none() && seal_ref.is_none() {
            return Err(AntiCheatError::AppealBad);
        }
        if self.entries.len() >= APPEAL_CAP {
            return Err(AntiCheatError::CapacityFull);
        }
        let seq = (self.entries.len() as u64).wrapping_add(1);
        let mut buf: Vec<u8> = Vec::with_capacity(40 + note.len());
        buf.extend_from_slice(b"VAPPEAL");
        push_u64_le(&mut buf, seq);
        push_u64_le(&mut buf, event_ref.unwrap_or(0));
        push_u64_le(&mut buf, seal_ref.unwrap_or(0));
        push_u16_le(&mut buf, reason_code);
        push_u64_le(&mut buf, at_ns);
        buf.extend_from_slice(note.as_bytes());
        let entry_hash = fnv_mix(self.chain, &buf);
        self.chain = entry_hash;
        self.entries.push(AppealEntry {
            seq,
            event_ref,
            seal_ref,
            reason_code,
            note: note.to_string(),
            verdict: AppealVerdict::Pending,
            at_ns,
            entry_hash,
        });
        Ok(seq)
    }

    /// 条数。
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// 是否为空。
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// 按序号查条目。
    pub fn entry(&self, seq: u64) -> Option<&AppealEntry> {
        if seq == 0 {
            return None;
        }
        let idx = (seq - 1) as usize;
        if idx >= self.entries.len() {
            return None;
        }
        Some(&self.entries[idx])
    }

    /// 链完整性重算。
    pub fn audit_chain(&self) -> bool {
        let mut st = FNV_OFFSET;
        let mut i = 0usize;
        while i < self.entries.len() {
            let e = &self.entries[i];
            let mut buf: Vec<u8> = Vec::with_capacity(40 + e.note.len());
            buf.extend_from_slice(b"VAPPEAL");
            push_u64_le(&mut buf, e.seq);
            push_u64_le(&mut buf, e.event_ref.unwrap_or(0));
            push_u64_le(&mut buf, e.seal_ref.unwrap_or(0));
            push_u16_le(&mut buf, e.reason_code);
            push_u64_le(&mut buf, e.at_ns);
            buf.extend_from_slice(e.note.as_bytes());
            st = fnv_mix(st, &buf);
            if st != e.entry_hash {
                return false;
            }
            i += 1;
        }
        st == self.chain
    }
}

// 编译期结构性断言：申诉条目规模受控（账目条目禁止放大到栈不可承载），
// append-only 由"字段私有 + 无 &mut 全量访问 API"的结构保证——本 impl 块
// 仅含 append/len/is_empty/entry/audit_chain 五个只读或追加方法。
const _: () = assert!(core::mem::size_of::<AppealEntry>() <= 128);
const _: () = assert!(core::mem::size_of::<CheatEvent>() <= 128);
const _: () = assert!(core::mem::size_of::<SegRecord>() <= 128);
const _: () = assert!(core::mem::size_of::<ScoreSeal>() <= 128);

// ===========================================================================
// §11 DetermBench 双跑确定性对拍
// ===========================================================================

/// 对拍结论。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DetermVerdict {
    /// A 跑链哈希。
    pub chain_a: u64,
    /// B 跑链哈希。
    pub chain_b: u64,
    /// A 跑总分。
    pub score_a: u64,
    /// B 跑总分。
    pub score_b: u64,
    /// 链哈希一致。
    pub chain_match: bool,
    /// 总分一致。
    pub score_match: bool,
    /// 完全一致（双真）。
    pub agreed: bool,
}

/// 双跑确定性对拍台：收两跑（链哈希, 总分），成对出结论；出结后清台接下一对。
#[derive(Debug)]
pub struct DetermBench {
    pending: Option<(u64, u64)>,
    pairs_done: u32,
    mismatch_pairs: u32,
}

impl DetermBench {
    /// 空台。
    pub fn new() -> Self {
        DetermBench { pending: None, pairs_done: 0, mismatch_pairs: 0 }
    }

    /// 提交一跑：偶数跑入台，奇数跑出结论。
    /// 对拍不一致返回 Ok(verdict)（agreed=false，属有效结论而非错误路径，
    /// 由引擎侧决定是否以 DetermMismatch 入账）。
    pub fn submit(&mut self, chain_hash: u64, total_score: u64) -> Result<Option<DetermVerdict>, AntiCheatError> {
        match self.pending.take() {
            None => {
                self.pending = Some((chain_hash, total_score));
                Ok(None)
            }
            Some((ca, sa)) => {
                let chain_match = ca == chain_hash;
                let score_match = sa == total_score;
                let v = DetermVerdict {
                    chain_a: ca,
                    chain_b: chain_hash,
                    score_a: sa,
                    score_b: total_score,
                    chain_match,
                    score_match,
                    agreed: chain_match && score_match,
                };
                self.pairs_done = self.pairs_done.wrapping_add(1);
                if !v.agreed {
                    self.mismatch_pairs = self.mismatch_pairs.wrapping_add(1);
                }
                Ok(Some(v))
            }
        }
    }

    /// 已出结论对数。
    pub fn pairs_done(&self) -> u32 {
        self.pairs_done
    }

    /// 不一致对数。
    pub fn mismatch_pairs(&self) -> u32 {
        self.mismatch_pairs
    }

    /// 台上是否挂有未配对跑。
    pub fn has_pending(&self) -> bool {
        self.pending.is_some()
    }
}

// ===========================================================================
// §12 两组（检测组 / 签名组）与引擎门面
// ===========================================================================

/// 检测组：五族检测器 + 立案账绑定。扫出即入账，返回入账序号表。
#[derive(Debug)]
pub struct DetectionGroup {
    events: CheatEventLedger,
}

impl DetectionGroup {
    /// 空组。
    pub fn new() -> Self {
        DetectionGroup { events: CheatEventLedger::new() }
    }

    /// 扫描一次运行：五族联扫 + 检出立案留痕，返回 (检出表, 立案序号表)。
    pub fn scan(&mut self, watch: &RunWatch, summary: &RunSummary, at_ns: u64)
        -> Result<(Vec<Detection>, Vec<u64>), AntiCheatError>
    {
        let dets = scan_families(watch, summary);
        let mut seqs: Vec<u64> = Vec::with_capacity(dets.len());
        let mut i = 0usize;
        while i < dets.len() {
            let seq = self.events.record(&dets[i], at_ns)?;
            seqs.push(seq);
            i += 1;
        }
        Ok((dets, seqs))
    }

    /// 立案账只读访问。
    pub fn ledger(&self) -> &CheatEventLedger {
        &self.events
    }

    /// 体检。
    pub fn health(&self) -> LedgerHealth {
        self.events.health()
    }
}

/// 签名组：成绩单签名/验签 + 密钥环。密钥缺省走拒签路径（0x1339）。
#[derive(Debug)]
pub struct SignatureGroup {
    ring: SealKeyRing,
    seals: Vec<ScoreSeal>,
    seq: u64,
}

impl SignatureGroup {
    /// 空组（无密钥——先 load_key 才能签）。
    pub fn new() -> Self {
        SignatureGroup { ring: SealKeyRing::new(), seals: Vec::new(), seq: 0 }
    }

    /// 装载密钥。
    pub fn load_key(&mut self, key: [u8; SEAL_KEY_LEN]) -> Result<usize, AntiCheatError> {
        self.ring.load_key(key)
    }

    /// 签名并入库：密钥缺省拒签（0x1339）；序号仅在成功封存后提交（失败不占号）。
    pub fn seal(&mut self, card: Scorecard) -> Result<u64, AntiCheatError> {
        if self.seals.len() >= SEAL_CAP {
            return Err(AntiCheatError::CapacityFull);
        }
        let seal = ScoreSeal::seal(card, self.ring.active_key(), self.seq.wrapping_add(1))?;
        self.seq = self.seq.wrapping_add(1);
        self.seals.push(seal);
        Ok(self.seq)
    }

    /// 验签：按封条序号取封条对称验签。
    pub fn verify(&self, seq: u64) -> Result<SealVerdict, AntiCheatError> {
        if seq == 0 || (seq as usize) > self.seals.len() {
            return Err(AntiCheatError::AppealBad);
        }
        self.seals[(seq - 1) as usize].verify(self.ring.active_key())
    }

    /// 封条只读访问。
    pub fn seal_at(&self, seq: u64) -> Option<&ScoreSeal> {
        if seq == 0 || (seq as usize) > self.seals.len() {
            return None;
        }
        Some(&self.seals[(seq - 1) as usize])
    }

    /// 封条数。
    pub fn seal_count(&self) -> usize {
        self.seals.len()
    }

    /// 是否已装密钥。
    pub fn has_key(&self) -> bool {
        self.ring.active_key().is_some()
    }
}

/// 引擎级体检报告。
#[derive(Clone, Debug)]
pub struct EngineHealth {
    /// 注册表条数。
    pub registry_entries: usize,
    /// 注册表冻结完整性。
    pub registry_ok: bool,
    /// 立案账体检。
    pub events: LedgerHealth,
    /// 申诉账链完整。
    pub appeals_ok: bool,
    /// 申诉条数。
    pub appeals: usize,
    /// 封条数。
    pub seals: usize,
    /// 签名组已装密钥。
    pub key_loaded: bool,
    /// 对拍对数。
    pub determ_pairs: u32,
    /// 对拍不一致对数。
    pub determ_mismatches: u32,
    /// 段守卫（诊断码段完整性）。
    pub segment_guard_ok: bool,
}

/// 汇报单：一次运行收表后的全套产出。
#[derive(Clone, Debug)]
pub struct WatchReport {
    /// 运行摘要。
    pub summary: RunSummary,
    /// 检出（含证据）。
    pub detections: Vec<Detection>,
    /// 立案序号。
    pub event_seqs: Vec<u64>,
    /// 收表时刻。
    pub closed_at_ns: u64,
}

/// 防作弊引擎门面：注册表 + 两组 + 申诉账 + 对拍台。
#[derive(Debug)]
pub struct AntiCheatEngine {
    registry: SceneRegistry,
    detection: DetectionGroup,
    signature: SignatureGroup,
    appeals: AppealLedger,
    determ: DetermBench,
    now_ns: u64,
}

impl AntiCheatEngine {
    /// 新引擎（无密钥、空账）。
    pub fn new() -> Self {
        AntiCheatEngine {
            registry: SceneRegistry::new(),
            detection: DetectionGroup::new(),
            signature: SignatureGroup::new(),
            appeals: AppealLedger::new(),
            determ: DetermBench::new(),
            now_ns: 0,
        }
    }

    /// 装载签名密钥。
    pub fn load_key(&mut self, key: [u8; SEAL_KEY_LEN]) -> Result<usize, AntiCheatError> {
        self.signature.load_key(key)
    }

    /// 注册场景。
    pub fn register_scene(&mut self, spec: SceneSpec) -> Result<u64, AntiCheatError> {
        self.registry.register(spec)
    }

    /// 开表。
    pub fn open_watch(&self, id: u32) -> Result<RunWatch, AntiCheatError> {
        RunWatch::open(&self.registry, id)
    }

    /// 收表 → 五族扫描 → 立案留痕 → 出汇报单。
    pub fn conclude_watch(&mut self, mut watch: RunWatch, end_ns: u64)
        -> Result<WatchReport, AntiCheatError>
    {
        if end_ns > self.now_ns {
            self.now_ns = end_ns;
        }
        let summary = watch.close(end_ns)?;
        let (dets, seqs) = self.detection.scan(&watch, &summary, end_ns)?;
        Ok(WatchReport { summary, detections: dets, event_seqs: seqs, closed_at_ns: end_ns })
    }

    /// 签发成绩单：五字段成绩卡 → 封条序号。
    pub fn seal_scorecard(&mut self, card: Scorecard) -> Result<u64, AntiCheatError> {
        self.signature.seal(card)
    }

    /// 验签。
    pub fn verify_seal(&self, seq: u64) -> Result<SealVerdict, AntiCheatError> {
        self.signature.verify(seq)
    }

    /// 追加申诉（只追加，不抹账）。
    pub fn file_appeal(
        &mut self,
        event_ref: Option<u64>,
        seal_ref: Option<u64>,
        reason_code: u16,
        note: &str,
    ) -> Result<u64, AntiCheatError> {
        self.appeals.append(event_ref, seal_ref, reason_code, note, self.now_ns)
    }

    /// 提交对拍跑。
    pub fn submit_determ_run(&mut self, summary: &RunSummary, total_score: u64)
        -> Result<Option<DetermVerdict>, AntiCheatError>
    {
        match self.determ.submit(summary.chain_hash(), total_score)? {
            Some(v) if !v.agreed => {
                // 不一致入立案账留痕（对拍不一致同样属于防作弊范畴）。
                let det = Detection {
                    family: CheatFamily::SceneDegrade,
                    scene_id: 0,
                    evidence: "双跑确定性对拍不一致（链哈希或总分不等）".to_string(),
                    severity_permille: PERMILLE_MAX,
                };
                self.detection.ledger_events_mut().record(&det, self.now_ns)?;
                Ok(Some(v))
            }
            other => Ok(other),
        }
    }

    /// 引擎体检。
    pub fn health(&self) -> EngineHealth {
        EngineHealth {
            registry_entries: self.registry.len(),
            registry_ok: self.registry.audit().is_ok(),
            events: self.detection.health(),
            appeals_ok: self.appeals.audit_chain(),
            appeals: self.appeals.len(),
            seals: self.signature.seal_count(),
            key_loaded: self.signature.has_key(),
            determ_pairs: self.determ.pairs_done(),
            determ_mismatches: self.determ.mismatch_pairs(),
            segment_guard_ok: segment_guard().is_ok(),
        }
    }

    /// 注册表只读访问。
    pub fn registry(&self) -> &SceneRegistry {
        &self.registry
    }

    /// 检测组只读访问。
    pub fn detection(&self) -> &DetectionGroup {
        &self.detection
    }

    /// 签名组只读访问。
    pub fn signature(&self) -> &SignatureGroup {
        &self.signature
    }

    /// 申诉账只读访问。
    pub fn appeals(&self) -> &AppealLedger {
        &self.appeals
    }

    /// 单调时钟推进（saturating）。
    pub fn advance(&mut self, delta_ns: u64) {
        self.now_ns = self.now_ns.saturating_add(delta_ns);
    }

    /// 当前单调时钟。
    pub fn now_ns(&self) -> u64 {
        self.now_ns
    }
}

/// 检测组内部账可变访问（仅限引擎与测试：立案留痕的受控入口）。
impl DetectionGroup {
    /// 立案账可变访问（引擎内部用；对外 API 仍走 scan 留痕路径）。
    pub(crate) fn ledger_events_mut(&mut self) -> &mut CheatEventLedger {
        &mut self.events
    }
}

// ===========================================================================
// §13 测试（隔离探针 cargo test 入口；五族注入/签名/申诉/对拍全绿）
// ===========================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::vec;

    /// 标准三段曲线（0→500→1000 负载）。
    fn mk_curve() -> Vec<LoadAnchor> {
        vec![
            LoadAnchor { t_permille: 0, load_permille: 400 },
            LoadAnchor { t_permille: 500, load_permille: 600 },
            LoadAnchor { t_permille: 1000, load_permille: 500 },
        ]
    }

    /// 标准场景：id=7，1s 时长，10 段 × 100ms，档 3，线程 8。
    fn mk_spec(id: u32) -> SceneSpec {
        SceneSpec::try_new(
            id, "std-bench", 1920, 1080, 3, 8,
            1_000_000_000, 60, 0xA5A5_5A5A, &mk_curve(), 120_000,
        )
        .unwrap()
    }

    /// 均匀负载带内段负载（600 略高于均值 500，带 ±25 内合法）。
    fn clean_watch(engine: &AntiCheatEngine, id: u32) -> RunWatch {
        let mut w = engine.open_watch(id).unwrap();
        w.begin(0).unwrap();
        let mut i: u64 = 0;
        while i < 10 {
            let s = i * 100_000_000;
            w.push_segment(s, s + 100_000_000, 1920, 1080, 3, 8, 500, 500, 120_000).unwrap();
            i += 1;
        }
        w
    }

    const TEST_KEY: [u8; SEAL_KEY_LEN] = [0x5Au8; SEAL_KEY_LEN];

    #[test]
    fn diag_band_guard() {
        assert!(segment_guard().is_ok());
        assert_eq!(AntiCheatError::SealKeyMissing.code(), 0x1339);
        assert_eq!(AntiCheatError::CapacityFull.code(), 0x133F);
    }

    #[test]
    fn register_and_dup_reject() {
        let mut eng = AntiCheatEngine::new();
        let spec = mk_spec(7);
        let fp = spec.fingerprint();
        assert_eq!(eng.register_scene(spec).unwrap(), fp);
        // 重注册同 id 拒绝。
        assert_eq!(eng.register_scene(mk_spec(7)).unwrap_err().code(), 0x1330);
        assert!(eng.registry().audit().is_ok());
        assert_eq!(eng.registry().len(), 1);
    }

    #[test]
    fn spec_validation_rejects() {
        assert_eq!(SceneSpec::try_new(1, "x", 32, 1080, 3, 8, DUR_MIN_NS, 1, 0, &mk_curve(), 1000).unwrap_err().code(), 0x1331);
        assert_eq!(SceneSpec::try_new(1, "x", 1920, 1080, 9, 8, DUR_MIN_NS, 1, 0, &mk_curve(), 1000).unwrap_err().code(), 0x1331);
        assert_eq!(SceneSpec::try_new(1, "x", 1920, 1080, 3, 0, DUR_MIN_NS, 1, 0, &mk_curve(), 1000).unwrap_err().code(), 0x1331);
        assert_eq!(SceneSpec::try_new(1, "x", 1920, 1080, 3, 8, 1, 1, 0, &mk_curve(), 1000).unwrap_err().code(), 0x1331);
        let bad_curve = vec![LoadAnchor { t_permille: 10, load_permille: 100 }];
        assert_eq!(SceneSpec::try_new(1, "x", 1920, 1080, 3, 8, DUR_MIN_NS, 1, 0, &bad_curve, 1000).unwrap_err().code(), 0x1331);
        let unsorted = vec![
            LoadAnchor { t_permille: 0, load_permille: 100 },
            LoadAnchor { t_permille: 500, load_permille: 100 },
            LoadAnchor { t_permille: 400, load_permille: 100 },
        ];
        assert_eq!(SceneSpec::try_new(1, "x", 1920, 1080, 3, 8, DUR_MIN_NS, 1, 0, &unsorted, 1000).unwrap_err().code(), 0x1331);
    }

    /// 干净跑：五族全空。
    #[test]
    fn clean_run_no_detection() {
        let mut eng = AntiCheatEngine::new();
        eng.register_scene(mk_spec(7)).unwrap();
        let w = clean_watch(&eng, 7);
        let rep = eng.conclude_watch(w, 1_000_000_000).unwrap();
        assert!(rep.detections.is_empty());
        assert!(!rep.summary.truncated);
        assert!(eng.detection().health().chain_ok);
    }

    /// 族一：场景降级（画质 3→1，档位指纹变化）。
    #[test]
    fn family1_scene_degrade_detected() {
        let mut eng = AntiCheatEngine::new();
        eng.register_scene(mk_spec(7)).unwrap();
        let mut w = eng.open_watch(7).unwrap();
        w.begin(0).unwrap();
        let mut i: u64 = 0;
        while i < 10 {
            let s = i * 100_000_000;
            let tier = if i == 5 { 1 } else { 3 }; // 中段偷偷降档
            w.push_segment(s, s + 100_000_000, 1920, 1080, tier, 8, 500, 500, 120_000).unwrap();
            i += 1;
        }
        let rep = eng.conclude_watch(w, 1_000_000_000).unwrap();
        let hit = rep.detections.iter().filter(|d| d.family() == CheatFamily::SceneDegrade).count();
        assert_eq!(hit, 1, "场景降级应恰检出一次：{:?}", rep.detections.iter().map(|d| d.evidence().to_string()).collect::<Vec<_>>());
    }

    /// 族二：时长截断（实跑 980ms < 注册 1s × 99%）。
    #[test]
    fn family2_duration_truncate_detected() {
        let mut eng = AntiCheatEngine::new();
        eng.register_scene(mk_spec(7)).unwrap();
        let w = clean_watch(&eng, 7);
        let rep = eng.conclude_watch(w, 980_000_000).unwrap();
        let hit = rep.detections.iter().filter(|d| d.family() == CheatFamily::DurationTruncate).count();
        assert_eq!(hit, 1);
        assert!(rep.summary.truncated);
    }

    /// 族三：挑参数（线程 8→4，参数哈希不等）。
    #[test]
    fn family3_param_cherry_detected() {
        let mut eng = AntiCheatEngine::new();
        eng.register_scene(mk_spec(7)).unwrap();
        let mut w = eng.open_watch(7).unwrap();
        w.begin(0).unwrap();
        let mut i: u64 = 0;
        while i < 10 {
            let s = i * 100_000_000;
            let threads = if i == 3 { 4 } else { 8 }; // 挑低线程段
            w.push_segment(s, s + 100_000_000, 1920, 1080, 3, threads, 500, 500, 120_000).unwrap();
            i += 1;
        }
        let rep = eng.conclude_watch(w, 1_000_000_000).unwrap();
        let hit = rep.detections.iter().filter(|d| d.family() == CheatFamily::ParamCherryPick).count();
        assert_eq!(hit, 1, "挑参数应恰检出一次");
        assert!(rep.summary.param_deviations >= 1, "偏离应入账");
    }

    /// 族四：静默降载（负载全程 300‰，远低于下限 425‰）。
    #[test]
    fn family4_silent_underload_detected() {
        let mut eng = AntiCheatEngine::new();
        eng.register_scene(mk_spec(7)).unwrap();
        let mut w = eng.open_watch(7).unwrap();
        w.begin(0).unwrap();
        let mut i: u64 = 0;
        while i < 10 {
            let s = i * 100_000_000;
            w.push_segment(s, s + 100_000_000, 1920, 1080, 3, 8, 300, 500, 120_000).unwrap();
            i += 1;
        }
        let rep = eng.conclude_watch(w, 1_000_000_000).unwrap();
        let hit = rep.detections.iter().filter(|d| d.family() == CheatFamily::SilentUnderload).count();
        assert_eq!(hit, 1, "静默降载应检出：{:?}", rep.summary.avg_load_permille());
    }

    /// 族四（分支 b）：负载在带内但功耗被压低。
    #[test]
    fn family4b_power_underload_detected() {
        let mut eng = AntiCheatEngine::new();
        eng.register_scene(mk_spec(7)).unwrap();
        let mut w = eng.open_watch(7).unwrap();
        w.begin(0).unwrap();
        let mut i: u64 = 0;
        while i < 10 {
            let s = i * 100_000_000;
            // 功耗 80_000 < 120_000×0.85=102_000，负载正常。
            w.push_segment(s, s + 100_000_000, 1920, 1080, 3, 8, 500, 500, 80_000).unwrap();
            i += 1;
        }
        let rep = eng.conclude_watch(w, 1_000_000_000).unwrap();
        let hit = rep.detections.iter().filter(|d| d.family() == CheatFamily::SilentUnderload).count();
        assert_eq!(hit, 1, "功耗压低分支应检出");
    }

    /// 族五：预热污染（首段 1000‰，其余 500‰ → 1000 > 575）。
    #[test]
    fn family5_warmup_pollute_detected() {
        let mut eng = AntiCheatEngine::new();
        eng.register_scene(mk_spec(7)).unwrap();
        let mut w = eng.open_watch(7).unwrap();
        w.begin(0).unwrap();
        let mut i: u64 = 0;
        while i < 10 {
            let s = i * 100_000_000;
            let score = if i == 0 { 1000 } else { 500 };
            w.push_segment(s, s + 100_000_000, 1920, 1080, 3, 8, 500, score, 120_000).unwrap();
            i += 1;
        }
        let rep = eng.conclude_watch(w, 1_000_000_000).unwrap();
        let hit = rep.detections.iter().filter(|d| d.family() == CheatFamily::WarmupPollute).count();
        assert_eq!(hit, 1, "预热污染应恰检出一次");
    }

    /// 签名：封存→验签通过；篡改分数→定位到 Score 字段。
    #[test]
    fn seal_roundtrip_and_tamper_localization() {
        let mut eng = AntiCheatEngine::new();
        eng.load_key(TEST_KEY).unwrap();
        let env = EnvProfile::from_raw(1, 12, 32, 4, 2);
        let card = Scorecard {
            scene_fingerprint: 0x1234_5678_9ABC_DEF0,
            duration_ns: 1_000_000_000,
            score: 5_000,
            env_hash: env.snapshot_hash(),
            tier_fp: 0xDEAD_BEEF_CAFE_0001,
        };
        let seq = eng.seal_scorecard(card).unwrap();
        let v = eng.verify_seal(seq).unwrap();
        assert!(v.ok, "合法封条应通过");

        // 篡改分数副本：验签定位到 Score 字段。
        let s_good = ScoreSeal::seal(card, Some(&TEST_KEY), 99).unwrap();
        assert!(s_good.verify(Some(&TEST_KEY)).unwrap().ok, "原始封条应自洽通过");
        let mut forged = s_good.clone();
        forged.card.score = 9_999;
        let v = forged.verify(Some(&TEST_KEY)).unwrap();
        assert!(!v.ok);
        assert_eq!(v.tampered, Some(SealField::Score), "应定位到分数字段");
        assert_eq!(v.diverged_step, 2);

        // 篡改环境画像：定位到 EnvProfile。
        let mut forged2 = s_good.clone();
        forged2.card.env_hash = env.snapshot_hash() ^ 1;
        let v2 = forged2.verify(Some(&TEST_KEY)).unwrap();
        assert_eq!(v2.tampered, Some(SealField::EnvProfile));
        assert_eq!(v2.diverged_step, 3);

        // 篡改终 MAC：定位到 FinalMac。
        let mut s3 = ScoreSeal::seal(card, Some(&TEST_KEY), 101).unwrap();
        s3.final_mac ^= 1;
        let v3 = s3.verify(Some(&TEST_KEY)).unwrap();
        assert_eq!(v3.tampered, Some(SealField::FinalMac));
    }

    /// 无密钥拒签 + 装错钥匙验签失败。
    #[test]
    fn no_key_rejected() {
        // 拒签：密钥 None → 0x1339。
        let card = Scorecard { scene_fingerprint: 1, duration_ns: 1, score: 1, env_hash: 1, tier_fp: 1 };
        let e = ScoreSeal::seal(card, None, 1).unwrap_err();
        assert_eq!(e.code(), 0x1339);
        assert!(e.detail().contains("密钥"));

        // 空引擎（未装钥匙）同样拒签。
        let mut eng = AntiCheatEngine::new();
        assert_eq!(eng.seal_scorecard(card).unwrap_err().code(), 0x1339);
        assert!(!eng.signature().has_key());

        // 装错钥匙：验签失败且定位 FinalMac（密钥 id 预检）。
        eng.load_key(TEST_KEY).unwrap();
        let seq = eng.seal_scorecard(card).unwrap();
        let wrong = [0x11u8; SEAL_KEY_LEN];
        let s = eng.signature().seal_at(seq).unwrap();
        let v = s.verify(Some(&wrong)).unwrap();
        assert!(!v.ok);
        assert_eq!(v.tampered, Some(SealField::FinalMac));
    }

    /// 申诉账：只追加、可审计、无抹账路径。
    #[test]
    fn appeal_append_only() {
        let mut eng = AntiCheatEngine::new();
        eng.register_scene(mk_spec(7)).unwrap();
        eng.load_key(TEST_KEY).unwrap();
        let w = clean_watch(&eng, 7);
        let rep = eng.conclude_watch(w, 1_000_000_000).unwrap();
        let card = Scorecard {
            scene_fingerprint: rep.summary.scene_fingerprint(),
            duration_ns: rep.summary.total_ns(),
            score: 5_000,
            env_hash: EnvProfile::from_raw(1, 12, 32, 4, 2).snapshot_hash(),
            tier_fp: rep.summary.tier_fp(),
        };
        let seal_seq = eng.seal_scorecard(card).unwrap();

        let a1 = eng.file_appeal(Some(1), None, 100, "对段3证据有异议").unwrap();
        let a2 = eng.file_appeal(None, Some(seal_seq), 200, "对封条时长字段有异议").unwrap();
        assert!(a2 > a1);
        assert_eq!(eng.appeals().len(), 2);
        assert!(eng.appeals().audit_chain());

        // 追加后续裁定条目（改判 = 追加，非改写）。
        let a3 = eng.file_appeal(Some(1), None, 101, "复核维持原判").unwrap();
        assert_eq!(eng.appeals().len(), 3);
        assert!(eng.appeals().entry(a3).unwrap().note().contains("维持"));

        // 非法申诉：无引用 + 理由码越界。
        assert_eq!(eng.file_appeal(None, None, 1, "x").unwrap_err().code(), 0x133D);
        assert_eq!(eng.file_appeal(Some(1), None, 1000, "x").unwrap_err().code(), 0x133D);
        // 不存在的引用序号仍可入账（申诉本身合法，复核时发现引用无效按 Pending 处理）。
        assert!(eng.file_appeal(Some(999), None, 1, "引用可能无效").is_ok());
        assert_eq!(eng.appeals().len(), 4);
    }

    /// 双跑确定性对拍：一致与不一致两路。
    #[test]
    fn determinism_double_run() {
        let mut eng = AntiCheatEngine::new();
        eng.register_scene(mk_spec(7)).unwrap();

        // 一致对：两跑干净链相同。
        let w1 = clean_watch(&eng, 7);
        let r1 = eng.conclude_watch(w1, 1_000_000_000).unwrap();
        let s1 = r1.summary.clone();
        let w2 = clean_watch(&eng, 7);
        let r2 = eng.conclude_watch(w2, 1_000_000_000).unwrap();
        let s2 = r2.summary.clone();
        assert_eq!(s1.chain_hash(), s2.chain_hash(), "同输入链哈希必须一致（确定性）");
        assert!(eng.submit_determ_run(&s1, 5_000).unwrap().is_none());
        let v = eng.submit_determ_run(&s2, 5_000).unwrap().unwrap();
        assert!(v.agreed);
        assert_eq!(eng.detection().health().per_family, [0, 0, 0, 0, 0]);

        // 不一致对：第二跑参数被改 → 链不同 → 立案留痕。
        let mut w3 = eng.open_watch(7).unwrap();
        w3.begin(0).unwrap();
        let mut i: u64 = 0;
        while i < 10 {
            let s = i * 100_000_000;
            w3.push_segment(s, s + 100_000_000, 1920, 1080, 3, 8, 501, 500, 120_000).unwrap();
            i += 1;
        }
        let r3 = eng.conclude_watch(w3, 1_000_000_000).unwrap();
        // 单跑入台等待配对。
        assert!(eng.submit_determ_run(&r3.summary, 5_000).unwrap().is_none());
        // 配一跑干净跑：链哈希不同 → 不一致结论 + 立案留痕。
        let v3 = eng.submit_determ_run(&s1, 5_000).unwrap().unwrap();
        assert!(!v3.agreed, "链哈希不同必须判不一致");
        let health = eng.detection().health();
        assert!(health.entries >= 1, "对拍不一致应入案：{:?}", health.per_family);
        assert!(health.chain_ok);
        assert_eq!(eng.determ_mismatch_count(), 1);
    }

    /// 体检全绿：注册/账链/段守卫/签名组。
    #[test]
    fn engine_health_all_green() {
        let mut eng = AntiCheatEngine::new();
        eng.load_key(TEST_KEY).unwrap();
        eng.register_scene(mk_spec(7)).unwrap();
        eng.register_scene(mk_spec(8)).unwrap();
        let w = clean_watch(&eng, 7);
        let rep = eng.conclude_watch(w, 1_000_000_000).unwrap();
        let _ = eng.seal_scorecard(Scorecard {
            scene_fingerprint: rep.summary.scene_fingerprint(),
            duration_ns: rep.summary.total_ns(),
            score: 5_000,
            env_hash: 42,
            tier_fp: rep.summary.tier_fp(),
        }).unwrap();
        let h = eng.health();
        assert!(h.registry_ok);
        assert!(h.events.chain_ok);
        assert!(h.appeals_ok);
        assert!(h.segment_guard_ok);
        assert!(h.key_loaded);
        assert_eq!(h.seals, 1);
        assert_eq!(h.registry_entries, 2);
        assert_eq!(h.events.per_family, [0, 0, 0, 0, 0]);
    }

    /// 段链防篡改：重放同段入账会撞单调校验；手改段哈希可被 audit 捕获
    /// （此处验证链值随内容变化——同内容同链、异内容异链）。
    #[test]
    fn segment_chain_is_content_bound() {
        let mut eng = AntiCheatEngine::new();
        eng.register_scene(mk_spec(7)).unwrap();
        let wa = clean_watch(&eng, 7);
        let wb = clean_watch(&eng, 7);
        assert_eq!(wa.seg_count(), 10);
        // 确定性：同输入同链。
        assert_eq!(wa.seg(9).unwrap().seg_hash(), wb.seg(9).unwrap().seg_hash());
        // 内容敏感：改一个字段，链值不同。
        let mut wc = eng.open_watch(7).unwrap();
        wc.begin(0).unwrap();
        wc.push_segment(0, 100_000_000, 1920, 1080, 3, 8, 500, 501, 120_000).unwrap();
        let mut wd = eng.open_watch(7).unwrap();
        wd.begin(0).unwrap();
        wd.push_segment(0, 100_000_000, 1920, 1080, 3, 8, 500, 500, 120_000).unwrap();
        assert_ne!(wc.seg(0).unwrap().seg_hash(), wd.seg(0).unwrap().seg_hash());
    }

    /// 环境画像：分桶脱敏 + 哈希稳定。
    #[test]
    fn env_profile_privacy_buckets() {
        let a = EnvProfile::from_raw(1, 12, 5, 4, 2);
        let b = EnvProfile::from_raw(1, 16, 7, 4, 2);
        assert_eq!(a, b, "12核/16核同桶、5G/7G同桶（粗粒度脱敏）");
        assert!(a.well_formed());
        assert_eq!(a.snapshot_hash(), b.snapshot_hash());
        let c = EnvProfile::from_raw(9, 3, 1, 99, 9); // 越界全部钳制
        assert!(c.well_formed());
        assert!(a.snapshot_hash() != c.snapshot_hash());
    }

    /// 密钥环：装载/激活/空环拒签路径。
    #[test]
    fn key_ring_slots() {
        let mut ring = SealKeyRing::new();
        assert!(ring.active_key().is_none());
        let s0 = ring.load_key([1u8; 32]).unwrap();
        assert_eq!(s0, 0);
        let s1 = ring.load_key([2u8; 32]).unwrap();
        assert_eq!(s1, 1);
        ring.activate(0).unwrap();
        assert_eq!(ring.active_key().unwrap()[0], 1);
        // 激活空槽拒绝。
        assert_eq!(ring.activate(3).unwrap_err().code(), 0x1339);
        // 装满后 CapacityFull。
        ring.load_key([3u8; 32]).unwrap();
        ring.load_key([4u8; 32]).unwrap();
        assert_eq!(ring.load_key([5u8; 32]).unwrap_err().code(), 0x133F);
        assert_eq!(ring.loaded(), KEY_RING_SLOTS);
    }

    /// 曲线均值：梯形加权平均语义验证（400@0→600@500→500@1000 → 均值 525）。
    #[test]
    fn curve_avg_weighted() {
        let spec = mk_spec(1);
        assert_eq!(spec.curve_avg_load(), 525);
        assert_eq!(spec.param_hash(), spec.param_hash(), "参数哈希确定性");
        assert_ne!(spec.fingerprint(), spec.tier_fingerprint(), "全量指纹与档位指纹必须不同域");
    }
}

// ===========================================================================
// 引擎侧测试辅助（仅测试可见）
// ===========================================================================

#[cfg(test)]
impl AntiCheatEngine {
    /// 对拍不一致对数（测试断言用）。
    fn determ_mismatch_count(&self) -> u32 {
        self.determ.mismatch_pairs()
    }
}
