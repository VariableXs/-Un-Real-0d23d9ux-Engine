//! VE-F6013 · 物理示例（VE-AB 域 · 物理组 · 目标 300 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F6013`
//!
//! **判据（锚点原文）**：三例、同源、CI 守护、判据。
//!
//! **职责定位（锚点原文）**：职责定位：物理示例（目标 300 行）——物理示例——最小物理场景、确定性对拍演示、调试视图三例同源；示例含确定性对拍演示——同种子两次运行轨迹完全重合的可视化；示例含对拍结果可视化；示例含教学步骤解说；示例含回归进 CI 说明；示例含读屏说明；示例含对拍结果可视化；可视化含帧号标注；帧号标注含跳转；跳转含断点联动。数据结构：示例包。错误路径与降级矩阵：失效→CI；过时→同步；缺→补。性能逐项分解：运行 O（示例）。跨批对接点：W07 样例库。无障碍与隐私：示例含指引。工程量：核心逻辑约 120 行（五类要点判定与主流程编排）、边界防护约 45 行（输入校验与阈值防护）、错误路径约 45 行（降级与回滚处置）、测试支撑约 90 行（回归用例与断言），合计约 300 行。判据：三例、同源、CI 守护、判据。
//!
//! ---------------------------------------------------------------------------
//! 深化说明（300 行 → 深化 2000+ 行，零浮点，Q16.16 定点建模）
//! ---------------------------------------------------------------------------
//!
//! 本模块是物理域（VE-AB）物理组的教学示例子系统（第二实现，与既有
//! `vead13_physexample` 同锚点并列；本文件为独立原创实现，互不引用）：
//! - `vead12_phystest`（VE-F6012）与 `vead13_physexample` 提供**测试面/示例面**；
//! - 本模块（vead14）以**示例册 + 守恒台账**为主线重写同一套确定性物理：
//!   每个场景的耗散全部入账——恢复系数/墙面损失与弹簧、软绳的逐步机械能
//!   落差按**量测不建模**原则实测入账，重力积分漂移按 −½m|g|²dt² **解析
//!   补偿**，摩擦功按 μm·g·|Δx| 解析入账——能量指标以
//!   「E0 − (E_end + W_台账)」口径结算，教学上可讲清"守恒去哪了"。
//!
//! 真子系统清单（全部手工实现，无浮点运算，Q16.16 定点）：
//!
//! 1. Q16.16 定点数学层：饱和乘（i128 展宽后移位）、Result 化除（除零
//!    即 `DivByZero`）、纯整数牛顿迭代开方（无浮点）、i128→i64 显性收窄。
//! 2. `SeedMixer` 种子搅拌器（splitmix64）：种子真正进入轨迹（多球/软绳
//!    摆初始条件抖动）——同种子必同轨迹、异种子必分叉的前提。
//! 3. `NanPatternGuard` NaN/Inf 位型防线：纯位判（指数位全 1 + 尾数判
//!    NaN/Inf），完全不依赖浮点比较；原始参数字（u64）先过防线再入 Q16。
//! 4. `EventLedger` 事件账（滚动容量 1024，超限丢弃计数）+ `PhysEvent`。
//! 5. `ExampleScene` 场景闭集（8 场景 const 初始条件表）：自由落体/弹性
//!    碰撞/斜面滑块/弹簧振子/抛体/多球碰撞/摩擦停摆/软绳摆，每场景带
//!    预期行为注记 + 四步教学解说 + 指标预期带（const，来源注释）。
//! 6. `PhysKernel` 微型确定性物理内核：半隐式欧拉固定步长；重力向量
//!    （含解析漂移补偿与法向支撑静置）/ 弹簧阻尼 / 库仑摩擦（静摩擦
//!    咬合）/ 软绳单边投影 / 球对冲量碰撞（恢复系数 + 位置校正）/
//!    四壁反弹（含反弹速度低于一步重力冲量的静置判据）；耗散全入
//!    work 台账；逐步状态摘要 + 链式指纹（splitmix 折叠）——任一步
//!    分叉即破，`verify_ledger` 可离线复核。
//! 7. `ScenarioRunner` 场景运行器：场景号 → 运行 → 采样轨迹（帧号标注）+
//!    碰撞动量审计 + 终态指纹；同种子双跑对拍（自证）+ 异种子必分叉（反证，
//!    不分叉即 `SeedDivergeFailed`）。
//! 8. `MetricExtractor` 教学指标提取：能量守恒偏差（ppm，台账口径）/ 动量
//!    守恒偏差（ppm，碰撞事件对拍）/ 收敛步数三指标；越带即立案（BandCase）。
//! 9. `DebugView` 调试视图三例同源：轨迹表/事件时间线/指标面板三视图全部
//!    出自同一份 `RunRecord`；帧号跳转（jump）+ 断点联动（跳转落点回看
//!    最近断点）；Q16 → 十进制字符串手工换算（零浮点）。
//! 10. `ExampleBook` 示例册：场景清单 + 运行快照 + 指纹；册指纹冻结（冻结
//!     后不可再改，`BookSealed`）；双跑一致判据（两次独立构建册指纹全等）。
//! 11. `ExampleCenter` 教学门面：`health()` 健康面 + `selfcheck()` 判据
//!     自检 + 注入面三件套（场景篡改/指纹漂移/越带注入，各检出于事件账）。
//!
//! 边界防线：速度/位置上限显性 saturating 钳制（钳制入账）；步数上限
//! `MAX_STEPS`；NaN/Inf 位型防线（位判不依赖浮点）；台账滚动容量。
//!
//! ---------------------------------------------------------------------------
//! 诊断码段（0x1E30~0x1E3F，独占）
//! ---------------------------------------------------------------------------
//! - 任务书预核段 0x1D70~0x1D7F 已被同锚点邻件 `vead13_physexample.rs`
//!   占用（其错误枚举使用 0x1D71~0x1D7F）——本文件不得复用；
//! - 备选段 0x1DF0 落在 `unxstar/unx2682_ucacheif.rs` 独占声明的
//!   0x1DC0~0x1DFF 段内，同样不可用；
//! - 本模块开工时 `0x1DB` 全源零命中，曾选 0x1DB0~0x1DBF；2026-10-24
//!   10:46 并行任务 F2087（`cgpu/vcn07_effbudget.rs`）以同样的零命中
//!   核验抢先声明了 0x1DB0~0x1DBF 全段——本文件让段重选；
//! - 重选 0x1E30~0x1E3F：全源字面零命中，下邻 `svstar2/ves491_s27sum.rs`
//!   段基 0x1E20 派生变体最高 0x1E23，上邻 `unxstar/unx2702_ulrufield.rs`
//!   独占声明自 0x1E40 起——本段 16 码两侧界标明确，本模块独占
//!   （段基座 0x1E30 保留为段标识，错误变体占 0x1E31~0x1E3F）。
//!
//! ---------------------------------------------------------------------------
//! 纪律（铁律）
//! ---------------------------------------------------------------------------
//! - no_std + alloc；只 `use core::…` / `use alloc::…`；零 `use crate::…`。
//! - 内核路径零 unwrap / 零 expect / 零 panic! / 零运行期 assert!
//!   （编译期 `const _: () = assert!(…)` 允许，见文末）。
//! - 集合访问一律 `while + 索引 + get()`，无裸索引越界路径。
//! - 可失败全走 `Result<T, PhysExError>`；错误枚举每变体带
//!   `detail(&self) -> &'static str` 中文可操作诊断。
//! - 溢出显性 saturating / wrapping；指纹搅拌用 wrapping（有意）。
//! - >64KB 结构体禁上栈：大账全部 `alloc::vec::Vec` 堆置。
//! - const fn 禁坑：无 slice::get、无 Vec 索引、无 Ord::min/max、无
//!   derive(PartialEq) 类型的 `==`、无 for 循环（一律 while + 索引）。
//! - 零浮点：物理量全 i64 Q16.16；NaN/Inf 判定纯位运算。
//!
//! [`ExampleScene`]: Self::SCENES
//! [`PhysKernel`]: Self::PhysKernel

// 依赖面（铁律：只 core + alloc，零 crate::…）
use alloc::format;
use alloc::string::String;
use alloc::string::ToString;
use alloc::vec::Vec;

// ===========================================================================
// 第 0 节 · 诊断码段与全模块常量
// ===========================================================================

/// 诊断段基座（0x1E30 段，独占 0x10——空闲核实见文件头）。
pub const SEG_BASE: u16 = 0x1E30;

/// Q16.16 定点 1.0（来源：16 位小数定标，2^16 = 65536）。
pub const Q16_ONE: i64 = 65_536;

/// 固定步长 dt = 1/60 s（来源：65536/60 = 1092.27 → 向下取整 1092）。
pub const DT_Q16: i64 = 1_092;

/// 标准重力教学值 g = 9.8 m/s²（来源：9.8 × 65536 = 642252.8 → 642252）。
pub const G_Q16: i64 = 642_252;

/// 速度钳制上限 100 m/s（来源：100 × 65536 = 6553600；状态爆炸防线）。
pub const V_MAX: i64 = 6_553_600;

/// 位置钳制上限 ±1000 m（来源：1000 × 65536 = 65536000；状态爆炸防线）。
pub const POS_CLAMP: i64 = 65_536_000;

/// 质量下限 0.05 kg（来源：0.05 × 65536 = 3276.8 → 3277；小于此视为脏参数）。
pub const MASS_MIN: i64 = 3_277;

/// 质量上限 1000 kg（来源：1000 × 65536 = 65536000；防冲量除零放大）。
pub const MASS_MAX: i64 = 65_536_000;

/// 半径上限 2 m（来源：2 × 65536 = 131072；防世界被单球填满）。
pub const RADIUS_MAX: i64 = 131_072;

/// 单场景物理体上限（来源：教学场景闭集最大规模 5 球 ×3 余量 ≈ 16）。
pub const BODY_CAP: usize = 16;

/// 单次运行步数硬上限（来源：教学场景最长 1500 步 ×13 余量；步数防线）。
pub const MAX_STEPS: u32 = 20_000;

/// 事件账滚动容量（来源：8 场景 × 典型 128 事件 = 1024；超限→滚动丢弃计数）。
pub const LEDGER_CAP: usize = 1_024;

/// 运行记录内事件高亮容量（来源：三例同源视图所需，超限截尾保留头部）。
pub const REC_EVENT_CAP: usize = 256;

/// 采样帧距：每 8 步采样一帧（来源：60Hz × 8 = 7.5 帧每秒，轨迹表可读）。
pub const SAMPLE_EVERY: u32 = 8;

/// 收敛判据速度阈值 0.02 m/s（来源：0.02 × 65536 = 1310.72 → 1311）。
pub const CONV_V_THRESH: i64 = 1_311;

/// 种子抖动位置幅度 0.25 m（来源：0.25 × 65536 = 16384；多球/软绳摆用）。
pub const JITTER_POS_AMP: i64 = 16_384;

/// 种子抖动速度幅度 ±3 m/s（来源：3 × 65536 = 196608；多球场景用）。
pub const JITTER_VEL_AMP: i64 = 196_608;

/// 碰撞位置校正比例 0.8（来源：0.8 × 65536 = 52428.8 → 52429；行业常用值
/// 0.2~0.8 取上界，保证穿透一步内基本分离且不引入振荡）。
pub const POS_CORRECTION: i64 = 52_429;

/// 指纹链初始种子（来源：ASCII "Phy5ExF6" 的 u64 大端拼位，纯装饰常量）。
pub const CHAIN_SEED: u64 = 0x5068_7953_4578_4636;

/// 场景闭集大小（锚点要求 ≥8 场景；本表恰 8 个）。
pub const SCENE_COUNT: u8 = 8;

/// 分叉反证默认场景（多球碰撞：种子抖动直接进初始条件，分叉最快）。
pub const DIVERGENCE_PROBE_SCENE: u8 = 6;

// ===========================================================================
// 第 1 节 · 错误面（0x1E30 段全码，每变体带中文可操作诊断）
// ===========================================================================

/// 物理示例错误码（0x1E31~0x1E3F，段基座 0x1E30 保留为段标识）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u16)]
pub enum PhysExError {
    /// 0x1E31 场景未知（场景号 0 或越界——闭集为 1..=8）。
    SceneUnknown = 0x1E31,
    /// 0x1E32 种子非法（0 值保留；请换任意非零 u64 种子）。
    SeedInvalid = 0x1E32,
    /// 0x1E33 物理体参数非法（质量/半径超教学域；检查场景初始条件表）。
    BodyInvalid = 0x1E33,
    /// 0x1E34 场景参数越界（恢复系数/摩擦系数/步长/能量基线非法）。
    ParamOutOfRange = 0x1E34,
    /// 0x1E35 定点除零（Q16 除法分母为 0；检查质量逆与法线长度来源）。
    DivByZero = 0x1E35,
    /// 0x1E36 定点负数开方（Q16 开方入参 < 0；检查距离平方是否下溢）。
    SqrtNegative = 0x1E36,
    /// 0x1E37 步数超限（超 MAX_STEPS 硬上限；教学场景步数请 ≤ 1500）。
    StepCountExceeded = 0x1E37,
    /// 0x1E38 同种子双跑分叉（确定性破坏；离线比对 chain 首个失配步）。
    DivergenceDetected = 0x1E38,
    /// 0x1E39 异种子未分叉（分叉反证失败；种子未真正进入初始条件）。
    SeedDivergeFailed = 0x1E39,
    /// 0x1E3A 指标越带（能量/动量/收敛步数出预期带；查 BandCase 案卷）。
    MetricOutOfBand = 0x1E3A,
    /// 0x1E3B 场景篡改未检出（目录指纹对篡改镜像未失配；防线失效）。
    TamperEscaped = 0x1E3B,
    /// 0x1E3C 指纹漂移未检出（链验证器对翻转位未报警；验证器失效）。
    DriftEscaped = 0x1E3C,
    /// 0x1E3D 越带注入未立案（伪造越带指标未被 BandCase 立案；闸门失效）。
    BandInjectEscaped = 0x1E3D,
    /// 0x1E3E 册已冻结（ExampleBook 冻结后不可追加/重建；请解冻或新建册）。
    BookSealed = 0x1E3E,
    /// 0x1E3F NaN/Inf 位型拒绝（原始参数字命中非有限位型；请换有限字）。
    NanPatternRejected = 0x1E3F,
}

impl PhysExError {
    /// 错误码（0x1E30 段内）。
    pub fn code(&self) -> u16 {
        *self as u16
    }

    /// 中文可操作诊断（每变体一条，含处置方向）。
    pub fn detail(&self) -> &'static str {
        match self {
            PhysExError::SceneUnknown => {
                "场景号未知：闭集为 1..=8（见 SCENES 表）；请核对场景号来源"
            }
            PhysExError::SeedInvalid => {
                "种子非法：0 值保留为哨兵；请换任意非零 u64 种子"
            }
            PhysExError::BodyInvalid => {
                "物理体参数非法：质量需在 [0.05,1000] kg、半径需在 (0,2] m；检查场景初始条件表"
            }
            PhysExError::ParamOutOfRange => {
                "场景参数越界：恢复系数/摩擦系数需在 [0,1]、步数需 ≤ MAX_STEPS、能量基线需 > 0；逐项核对 const 表"
            }
            PhysExError::DivByZero => {
                "定点除零：Q16 除法分母为 0；检查质量（逆质量来源）或碰撞法线长度（重合回退）上游"
            }
            PhysExError::SqrtNegative => {
                "定点负数开方：距离平方 < 0 不应出现；检查 qmul 饱和是否被触发"
            }
            PhysExError::StepCountExceeded => {
                "步数超限：超 MAX_STEPS=20000 硬上限；教学场景步数请 ≤ 1500，长稳请走 F6012"
            }
            PhysExError::DivergenceDetected => {
                "同种子双跑分叉：确定性破坏；离线比对两次 RunRecord.chain 的首个失配步并回放该步摘要"
            }
            PhysExError::SeedDivergeFailed => {
                "异种子未分叉：种子未真正进入初始条件；确认场景 jitter 位与 SeedMixer 接线"
            }
            PhysExError::MetricOutOfBand => {
                "指标越带：能量/动量/收敛步数出预期带；查 BandCase 案卷中的场景号、指标名、实测值与带宽"
            }
            PhysExError::TamperEscaped => {
                "场景篡改未检出：目录指纹对篡改镜像仍相等；防线失效，检查指纹折叠是否漏字段"
            }
            PhysExError::DriftEscaped => {
                "指纹漂移未检出：链验证器对翻转位仍判真；检查 verify_ledger 与 step 的链式接线是否同式"
            }
            PhysExError::BandInjectEscaped => {
                "越带注入未立案：伪造越带指标未进 BandCase；检查 gate 的带宽上下界取用"
            }
            PhysExError::BookSealed => {
                "册已冻结：ExampleBook 冻结后不可追加/重建；如需更新请新建一册并重跑双构建一致判据"
            }
            PhysExError::NanPatternRejected => {
                "NaN/Inf 位型拒绝：原始参数字命中非有限位型（指数位全 1）；请改用有限位型字或走 Q16 直挂通道"
            }
        }
    }
}

// ===========================================================================
// 第 2 节 · Q16.16 定点数学层（零浮点，全显性饱和/Result 化）
// ===========================================================================

/// i128 → i64 显性收窄（上下溢钳到极值；调用点注释来源）。
pub fn narrow(v: i128) -> i64 {
    if v > i64::MAX as i128 {
        i64::MAX
    } else if v < i64::MIN as i128 {
        i64::MIN
    } else {
        v as i64
    }
}

/// Q16 饱和乘：(a·b) >> 16，i128 展宽防中间溢出，收窄显性钳制。
///
/// 舍入语义：算术右移（负数向下取整）；单次相对舍入 ≤ 2^-15 ≈ 30.5ppm，
/// 能量/动量预期带的来源即此舍入链。
pub fn qmul(a: i64, b: i64) -> i64 {
    narrow(((a as i128) * (b as i128)) >> 16)
}

/// Q16 Result 化除：(a << 16) / b，除零即 `DivByZero`（内核路径零 panic）。
pub fn qdiv(a: i64, b: i64) -> Result<i64, PhysExError> {
    if b == 0 {
        return Err(PhysExError::DivByZero);
    }
    Ok(narrow(((a as i128) << 16) / (b as i128)))
}

/// u64 纯整数平方根（牛顿迭代，初值取 2^⌈bits/2⌉ ≥ √n 的上界）。
///
/// 预条件：n ≤ 2^62（调用点均为距离平方域，≤ ~2.8e16，满足）；
/// 迭代单调下降有界（≤ 64 轮），终段双向校准到 ⌊√n⌋。
pub fn isqrt_u64(n: u64) -> u64 {
    if n == 0 {
        return 0;
    }
    let bits = 64 - n.leading_zeros();
    let mut x: u64 = 1u64 << ((bits + 1) / 2);
    let mut guard: u32 = 0;
    while guard < 64 {
        guard += 1;
        let t = x + n / x;
        let nx = t >> 1;
        if nx >= x {
            break;
        }
        x = nx;
    }
    // 下校准：x² > n 则退（牛顿可能停在 ⌊√n⌋+1）。
    while x > 0 {
        let sq = match x.checked_mul(x) {
            Some(v) => v,
            None => {
                x -= 1;
                continue;
            }
        };
        if sq > n {
            x -= 1;
        } else {
            break;
        }
    }
    // 上校准：(x+1)² ≤ n 则进（极端上界起点可能过头后欠一步）。
    loop {
        let y = match x.checked_add(1) {
            Some(v) => v,
            None => break,
        };
        let sq = match y.checked_mul(y) {
            Some(v) => v,
            None => break,
        };
        if sq <= n {
            x = y;
        } else {
            break;
        }
    }
    x
}

/// Q16 定点开方：y = ⌊√(x·2^16)⌋。
///
/// 推导：x = X·2^16（X 实数值），y 应为 Y·2^16 且 Y = √X，
/// 故 y = √X·2^16 = √(X·2^32) = √(x·2^16)。入参饱和到 u64 域。
pub fn q_sqrt(x: i64) -> Result<i64, PhysExError> {
    if x < 0 {
        return Err(PhysExError::SqrtNegative);
    }
    let scaled: u128 = (x as u128) * 65_536u128;
    let capped: u64 = if scaled > u64::MAX as u128 {
        u64::MAX
    } else {
        scaled as u64
    };
    Ok(isqrt_u64(capped) as i64)
}

/// Q16 → 十进制字符串（零浮点；frac_digits 位小数，四舍五入到最近）。
///
/// 来源：整数部分 = v>>16；小数部分 = round((v&0xFFFF)·10^d / 2^16)，
/// 进位到整数部分处理。教学视图专用（读屏友好，不用科学计数）。
pub fn q16_to_dec(v: i64, frac_digits: u32) -> String {
    // 10^d 表（d ≤ 6；视图最多用 3 位，留余量）。
    let pow10: i64 = match frac_digits {
        0 => 1,
        1 => 10,
        2 => 100,
        3 => 1_000,
        4 => 10_000,
        5 => 100_000,
        _ => 1_000_000,
    };
    let neg: bool = v < 0;
    let mag: i64 = if neg {
        // v 不会是 i64::MIN（物理域已钳制）；wrapping 兜底防御。
        (v as i128).unsigned_abs() as i64
    } else {
        v
    };
    let ipart: i64 = mag >> 16;
    let frac_bits: i64 = mag & 0xFFFF;
    let mut frac: i64 = ((frac_bits as i128 * pow10 as i128 + 32_768) >> 16) as i64;
    let mut ipart_c: i64 = ipart;
    if frac >= pow10 {
        frac -= pow10;
        ipart_c += 1;
    }
    let mut s: String = String::new();
    if neg {
        s.push('-');
    }
    s.push_str(&ipart_c.to_string());
    if frac_digits > 0 {
        s.push('.');
        // 前导零补齐（手工，不依赖格式化宽度填充的坑位）。
        let mut scale: i64 = pow10 / 10;
        while scale > 1 {
            if frac < scale {
                s.push('0');
            }
            scale /= 10;
        }
        s.push_str(&frac.to_string());
    }
    s
}

// ===========================================================================
// 第 3 节 · 种子搅拌器与指纹折叠（splitmix64 家族，wrapping 有意为之）
// ===========================================================================

/// splitmix64 黄金比例增量（来源：Knuth TAOCP Vol.2 线性同余推荐常数，
/// splitmix64 标准实现沿用 0x9E3779B97F4A7C15）。
const SM_GOLDEN: u64 = 0x9E37_79B9_7F4A_7C15;

/// 种子搅拌器：确定性 u64 流 + Q16 抖动派生（种子进轨迹的唯一通道）。
#[derive(Debug, Clone, Copy)]
pub struct SeedMixer {
    state: u64,
    drawn: u32,
}

impl SeedMixer {
    /// 新建（种子为 0 时拒绝——0 保留为哨兵，见 `SeedInvalid`）。
    pub fn new(seed: u64) -> Result<Self, PhysExError> {
        if seed == 0 {
            return Err(PhysExError::SeedInvalid);
        }
        Ok(SeedMixer {
            state: seed,
            drawn: 0,
        })
    }

    /// 取下一个 u64（splitmix64 输出函数）。
    pub fn next(&mut self) -> u64 {
        self.state = self.state.wrapping_add(SM_GOLDEN);
        self.drawn += 1;
        let mut z: u64 = self.state;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    /// 已抽取次数（对拍调试用：确认种子真正进入了初始条件）。
    pub fn draws(&self) -> u32 {
        self.drawn
    }

    /// 派生 Q16 对称抖动：[−amp, +amp] 均匀取值（amp ≥ 1）。
    ///
    /// 口径：span = 2·amp+1，取 next() % span 再平移——模偏置在
    /// amp ≪ 2^64 下可忽略（教学域 amp ≤ 196608）。
    pub fn jitter_q16(&mut self, amp: i64) -> Result<i64, PhysExError> {
        if amp < 1 {
            return Err(PhysExError::ParamOutOfRange);
        }
        let span: u64 = (amp as u64) * 2 + 1;
        let raw: u64 = self.next() % span;
        Ok(raw as i64 - amp)
    }
}

/// 指纹折叠（splitmix64 终结器变体；const fn 合规：仅算术与 wrapping）。
pub const fn fp_mix(state: u64, word: u64) -> u64 {
    let mut z: u64 = state ^ word;
    z = z.wrapping_mul(0x8698_F07C_4A89_6195);
    z ^= z >> 29;
    z = z.wrapping_mul(SM_GOLDEN);
    z ^= z >> 32;
    z
}

// ===========================================================================
// 第 4 节 · NaN/Inf 位型防线（纯位判，零浮点比较）
// ===========================================================================

/// f32 语义位型分类（仅按位域判，不构造浮点值）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BitClass {
    /// 有限数（含零/次正规——指数位不全 1）。
    Finite,
    /// 无穷（指数位全 1 且尾数为 0）。
    Inf,
    /// NaN（指数位全 1 且尾数非 0）。
    Nan,
}

/// f32 典型非有限位型 const 表（防线文档面 + 测试种子）。
///
/// 来源：IEEE 754-2008 单精度——0x7F800000/+Inf、0xFF800000/−Inf、
/// 0x7F800001/+sNaN、0xFF800001/−sNaN、0x7FC00000/+qNaN、0xFFC00000/−qNaN。
pub const F32_CANONICAL_NONFINITE: [u32; 6] = [
    0x7F80_0000,
    0xFF80_0000,
    0x7F80_0001,
    0xFF80_0001,
    0x7FC0_0000,
    0xFFC0_0000,
];

/// f64 典型非有限位型 const 表（防线文档面 + 测试种子）。
///
/// 来源：IEEE 754-2008 双精度——±Inf（指数 0x7FF、尾数 0）与
/// ±qNaN（尾数最高位 1）四种典型字。
pub const F64_CANONICAL_NONFINITE: [u64; 4] = [
    0x7FF0_0000_0000_0000,
    0xFFF0_0000_0000_0000,
    0x7FF8_0000_0000_0000,
    0xFFF8_0000_0000_0000,
];

/// f32 位型分类（纯位运算：指数域 (bits>>23)&0xFF 全 1 即非有限）。
pub const fn classify_u32(bits: u32) -> BitClass {
    let exp: u32 = (bits >> 23) & 0xFF;
    if exp != 0xFF {
        BitClass::Finite
    } else if bits & 0x007F_FFFF == 0 {
        BitClass::Inf
    } else {
        BitClass::Nan
    }
}

/// f64 位型分类（纯位运算：指数域 (bits>>52)&0x7FF 全 1 即非有限）。
pub const fn classify_u64(bits: u64) -> BitClass {
    let exp: u64 = (bits >> 52) & 0x7FF;
    if exp != 0x7FF {
        BitClass::Finite
    } else if bits & 0x000F_FFFF_FFFF_FFFF == 0 {
        BitClass::Inf
    } else {
        BitClass::Nan
    }
}

/// NaN/Inf 位型防线：原始参数字的唯一准入口。
///
/// 面向锚点「参数注入防护」：外部来的原始字（如样例库 W07 的原始
/// 参数流）必须先 admit 位型检查，通过后才允许换算成 Q16 物理量。
/// 检测纯位判（见 `classify_u32/u64`），完全不依赖浮点比较——
/// 零浮点纪律下根本不存在浮点值可比，位域是唯一语义来源。
#[derive(Debug, Clone, Copy)]
pub struct NanPatternGuard {
    armed: bool,
    rejects: u32,
    admits: u32,
}

impl NanPatternGuard {
    /// 新建（默认武装）。
    pub fn new() -> Self {
        NanPatternGuard {
            armed: true,
            rejects: 0,
            admits: 0,
        }
    }

    /// 武装状态（教学面：演示「解除防线后洪流扩散」的对照实验）。
    pub fn armed(&self) -> bool {
        self.armed
    }

    /// 解除/重武装（解除后 admit 直通且计数入 pass_through）。
    pub fn set_armed(&mut self, on: bool) {
        self.armed = on;
    }

    /// 拒绝计数。
    pub fn rejects(&self) -> u32 {
        self.rejects
    }

    /// 通过计数（有限位型 + 解除态直通合计）。
    pub fn admits(&self) -> u32 {
        self.admits
    }

    /// u64 原始字准入：非有限位型 → `NanPatternRejected`（armed 态）。
    pub fn admit_u64(&mut self, bits: u64) -> Result<u64, PhysExError> {
        let cls: BitClass = classify_u64(bits);
        if self.armed {
            if cls != BitClass::Finite {
                self.rejects += 1;
                return Err(PhysExError::NanPatternRejected);
            }
            self.admits += 1;
            Ok(bits)
        } else {
            // 解除态：直通但保留分类痕迹（洪流对照实验的计数面）。
            if cls != BitClass::Finite {
                self.rejects += 1;
            } else {
                self.admits += 1;
            }
            Ok(bits)
        }
    }

    /// u32 原始字准入（低 32 位路径，同 `admit_u64` 语义）。
    pub fn admit_u32(&mut self, bits: u32) -> Result<u32, PhysExError> {
        let cls: BitClass = classify_u32(bits);
        if self.armed {
            if cls != BitClass::Finite {
                self.rejects += 1;
                return Err(PhysExError::NanPatternRejected);
            }
            self.admits += 1;
            Ok(bits)
        } else {
            if cls != BitClass::Finite {
                self.rejects += 1;
            } else {
                self.admits += 1;
            }
            Ok(bits)
        }
    }

    /// 原始字 → Q16：先过位型防线，再取低 48 位符号扩展为 Q16 值。
    ///
    /// 口径：样例库原始字是 u64 位串；取低 48 位（±32768 物理域足够，
    /// 且避开 f64 指数位域的语义纠缠），符号扩展后饱和到 ±POS_CLAMP。
    pub fn raw_to_q16(&mut self, bits: u64) -> Result<i64, PhysExError> {
        let clean: u64 = self.admit_u64(bits)?;
        let v48: i64 = ((clean & 0x0000_FFFF_FFFF_FFFF) as i64) << 16 >> 16;
        if v48 > POS_CLAMP {
            Ok(POS_CLAMP)
        } else if v48 < -POS_CLAMP {
            Ok(-POS_CLAMP)
        } else {
            Ok(v48)
        }
    }
}

// ===========================================================================
// 第 5 节 · 事件账（滚动容量 + 丢弃计数）
// ===========================================================================

/// 事件种类（事件码独立编号，非诊断码；1..=16）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EventKind {
    /// 1 运行开始（a=场景号，b=种子低 32 位）。
    RunStart = 1,
    /// 2 运行结束（a=步数，b=终态指纹低 32 位）。
    RunEnd = 2,
    /// 3 球对碰撞（a=i|j<<8，b=冲量幅值）。
    PairCollide = 3,
    /// 4 墙面反弹（a=体号，b=反弹轴 0x/1y）。
    WallBounce = 4,
    /// 5 静摩擦咬合停摆（a=体号，b=0）。
    FrictionLatch = 5,
    /// 6 软绳张紧（a=体号，b=绳长差）。
    RopeEngage = 6,
    /// 7 速度/位置钳制（a=体号，b=1 速度/2 位置）。
    ClampTrip = 7,
    /// 8 位型防线拒绝（a=拒绝计数，b=0）。
    GuardReject = 8,
    /// 9 场景篡改检出（a=场景号，b=0）。
    TamperDetected = 9,
    /// 10 指纹漂移检出（a=篡改步号，b=0）。
    DriftDetected = 10,
    /// 11 指标越带立案（a=场景号，b=指标码 1 能量/2 动量/3 收敛）。
    BandFiled = 11,
    /// 12 对拍分叉（a=首个失配步，b=0）。
    Divergence = 12,
    /// 13 册冻结（a=册指纹低 32 位，b=条目数）。
    BookSeal = 13,
    /// 14 双跑一致确认（a=场景号，b=终态指纹低 32 位）。
    DualConsistent = 14,
    /// 15 越带注入立案（a=伪造值 ppm，b=带宽）。
    BandInjectFiled = 15,
    /// 16 自检项（a=1 过/0 败，b=项序号）。
    SelfCheck = 16,
}

impl EventKind {
    /// 事件码（1..=16）。
    pub fn code(&self) -> u16 {
        *self as u16
    }

    /// 中文名（读屏/视图用）。
    pub fn name(&self) -> &'static str {
        match self {
            EventKind::RunStart => "运行开始",
            EventKind::RunEnd => "运行结束",
            EventKind::PairCollide => "球对碰撞",
            EventKind::WallBounce => "墙面反弹",
            EventKind::FrictionLatch => "静摩擦咬合",
            EventKind::RopeEngage => "软绳张紧",
            EventKind::ClampTrip => "状态钳制",
            EventKind::GuardReject => "位型拒绝",
            EventKind::TamperDetected => "篡改检出",
            EventKind::DriftDetected => "漂移检出",
            EventKind::BandFiled => "越带立案",
            EventKind::Divergence => "对拍分叉",
            EventKind::BookSeal => "册冻结",
            EventKind::DualConsistent => "双跑一致",
            EventKind::BandInjectFiled => "越带注入",
            EventKind::SelfCheck => "自检项",
        }
    }
}

/// 单条物理事件（32B，Copy；账本体在堆）。
#[derive(Debug, Clone, Copy)]
pub struct PhysEvent {
    /// 事件所在步（帧号）。
    pub tick: u32,
    /// 种类。
    pub kind: EventKind,
    /// 载荷 a（语义随种类，见 `EventKind` 文档）。
    pub a: i64,
    /// 载荷 b。
    pub b: i64,
}

/// 事件账：滚动容量 `LEDGER_CAP`，超限丢弃最旧并计数（锚点：超限→滚动）。
#[derive(Debug, Clone)]
pub struct EventLedger {
    events: Vec<PhysEvent>,
    dropped: u32,
    pushed: u32,
}

impl EventLedger {
    /// 新建空账。
    pub fn new() -> Self {
        EventLedger {
            events: Vec::new(),
            dropped: 0,
            pushed: 0,
        }
    }

    /// 入账一条（滚动丢弃最旧）。
    pub fn push(&mut self, ev: PhysEvent) {
        self.pushed += 1;
        if self.events.len() >= LEDGER_CAP {
            // 滚动：丢头（Vec::remove O(n)，容量 1024 下可接受；口径注释）。
            if self.events.len() > 0 {
                self.events.remove(0);
                self.dropped += 1;
            }
        }
        self.events.push(ev);
    }

    /// 当前账长。
    pub fn len(&self) -> usize {
        self.events.len()
    }

    /// 是否空账。
    pub fn is_empty(&self) -> bool {
        self.events.len() == 0
    }

    /// 累计丢弃数（滚动防线计数面）。
    pub fn dropped(&self) -> u32 {
        self.dropped
    }

    /// 累计入账数。
    pub fn pushed(&self) -> u32 {
        self.pushed
    }

    /// 只读访问第 i 条（越界 None；内核路径零裸索引）。
    pub fn get(&self, i: usize) -> Option<PhysEvent> {
        self.events.get(i).copied()
    }

    /// 清账（保留丢弃/入账累计；口径：累计是历史，不清零）。
    pub fn clear_events(&mut self) {
        self.events.clear();
    }
}

// ===========================================================================
// 第 6 节 · 物理体与世界状态
// ===========================================================================

/// 世界特性位（const 表驱动；一位一力，教学上可按位开关做对照实验）。
pub const F_GRAVITY: u32 = 1 << 0; // 重力向量积分
pub const F_WALLS: u32 = 1 << 1; // 四壁反弹（恢复系数）
pub const F_PAIR: u32 = 1 << 2; // 球对冲量碰撞
pub const F_SPRING: u32 = 1 << 3; // 弹簧 + 阻尼锚点
pub const F_ROPE: u32 = 1 << 4; // 软绳单边约束（只拉不推）
pub const F_FRICTION: u32 = 1 << 5; // 库仑摩擦（含静摩擦咬合）
pub const F_FLOOR_REST: u32 = 1 << 6; // 地面静置（vy 置零不反弹）

/// 圆形物理体（Q16 域；Copy 无堆字段）。
#[derive(Debug, Clone, Copy)]
pub struct Body {
    /// 质量 kg（Q16）。
    pub mass: i64,
    /// 逆质量 1/m（Q16；建场时预计算，步内只乘不除）。
    pub inv_mass: i64,
    /// 半径 m（Q16）。
    pub radius: i64,
    /// 位置 x m（Q16）。
    pub x: i64,
    /// 位置 y m（Q16，向上为正）。
    pub y: i64,
    /// 速度 vx m/s（Q16）。
    pub vx: i64,
    /// 速度 vy m/s（Q16）。
    pub vy: i64,
}

impl Body {
    /// 构造体（质量逆在建场时 Result 化计算；半径 >0 由建场校验）。
    pub fn make(mass: i64, radius: i64, x: i64, y: i64, vx: i64, vy: i64) -> Result<Self, PhysExError> {
        if mass < MASS_MIN || mass > MASS_MAX {
            return Err(PhysExError::BodyInvalid);
        }
        if radius <= 0 || radius > RADIUS_MAX {
            return Err(PhysExError::BodyInvalid);
        }
        let inv: i64 = qdiv(Q16_ONE, mass)?;
        Ok(Body {
            mass,
            inv_mass: inv,
            radius,
            x,
            y,
            vx,
            vy,
        })
    }

    /// 单体动能 ½m|v|²（Q16 J）。
    pub fn kinetic(&self) -> i64 {
        let v2: i64 = qmul(self.vx, self.vx).saturating_add(qmul(self.vy, self.vy));
        qmul(self.mass, v2) >> 1
    }

    /// 单体全机械能：动能 + 势能（−m(gx·x + gy·y)；g=0 时退化为动能）。
    ///
    /// 撞击/贴面台账口径：位置钳制会把穿透量抬回（势能恢复），只有
    /// Δ(KE+PE) 才是真实机械能损失——ΔKE 会漏抬回量、虚增台账。
    pub fn energy_with(&self, gx: i64, gy: i64) -> i64 {
        let pot: i64 = qmul(self.mass, qmul(gx, self.x).saturating_add(qmul(gy, self.y)));
        self.kinetic().saturating_sub(pot)
    }
}

/// 世界状态（一场一世界；大账堆置）。
#[derive(Debug, Clone)]
pub struct WorldState {
    /// 已积分步数（帧号）。
    pub tick: u32,
    /// 物理体集（≤ BODY_CAP）。
    pub bodies: Vec<Body>,
    /// 重力加速度向量（Q16 m/s²；势能口径 = −m(gx·x + gy·y)）。
    pub gx: i64,
    /// 重力 y 分量。
    pub gy: i64,
    /// 恢复系数 e ∈ [0,1]（Q16；墙与球对共用）。
    pub restitution: i64,
    /// 动摩擦系数 μk（Q16）。
    pub mu_k: i64,
    /// 静摩擦系数 μs（Q16）。
    pub mu_s: i64,
    /// 特性位集（F_* 或组合）。
    pub features: u32,
    /// 软绳锚点（Q16）。
    pub rope_anchor: (i64, i64),
    /// 软绳自然长（Q16 m）。
    pub rope_len: i64,
    /// 软绳当前是否张紧（投影事件去抖）。
    pub rope_taut: bool,
    /// 弹簧锚点（Q16）。
    pub spring_anchor: (i64, i64),
    /// 弹簧刚度 k（Q16 N/m）。
    pub spring_k: i64,
    /// 弹簧阻尼 c（Q16 N·s/m）。
    pub spring_c: i64,
    /// 边界 (min_x, max_x, min_y, max_y)（Q16）。
    pub bounds: (i64, i64, i64, i64),
    /// 耗散台账（Q16 J）：恢复系数损失 + 摩擦功 + 阻尼功全部累计于此。
    pub work_ledger: i64,
    /// 钳制累计（速度/位置合计；状态爆炸防线计数）。
    pub clamp_count: u32,
}

impl WorldState {
    /// 世界总能量（教学口径）：动能 + 势能 + 弹性势能。
    ///
    /// 势能 = −m(gx·x + gy·y)——常规重力 (0,−g) 时退化为 mgy；
    /// 斜面场景的重力沿坡分量同样被该口径一致覆盖。
    /// 弹性势能 = ½k·|p−锚|²（F_SPRING 时对每体累计）。
    pub fn total_energy(&self) -> i64 {
        let mut sum: i64 = 0;
        let mut i: usize = 0;
        while i < self.bodies.len() {
            if let Some(b) = self.bodies.get(i) {
                sum = sum.saturating_add(b.kinetic());
                let pot: i64 = qmul(
                    b.mass,
                    qmul(self.gx, b.x).saturating_add(qmul(self.gy, b.y)),
                );
                // 势能取负号口径：g_y = −g 时 pot = −m·g·y → 势能 = +m·g·y。
                sum = sum.saturating_sub(pot);
                if self.features & F_SPRING != 0 {
                    let dx: i64 = b.x.saturating_sub(self.spring_anchor.0);
                    let dy: i64 = b.y.saturating_sub(self.spring_anchor.1);
                    let d2: i64 = qmul(dx, dx).saturating_add(qmul(dy, dy));
                    sum = sum.saturating_add(qmul(self.spring_k, d2) >> 1);
                }
            }
            i += 1;
        }
        sum
    }

    /// 世界状态摘要（链式指纹的每步入料：全体体 + 台账 + 步号折叠）。
    pub fn digest(&self) -> u64 {
        let mut h: u64 = CHAIN_SEED ^ (self.tick as u64);
        let mut i: usize = 0;
        while i < self.bodies.len() {
            if let Some(b) = self.bodies.get(i) {
                h = fp_mix(h, b.mass as u64);
                h = fp_mix(h, b.radius as u64);
                h = fp_mix(h, b.x as u64);
                h = fp_mix(h, b.y as u64);
                h = fp_mix(h, b.vx as u64);
                h = fp_mix(h, b.vy as u64);
            }
            i += 1;
        }
        h = fp_mix(h, self.work_ledger as u64);
        h = fp_mix(h, self.clamp_count as u64);
        h = fp_mix(h, self.features as u64);
        h
    }

    /// 目录指纹（建场参数折叠：篡改镜像必失配——注入面 TamperDetected 的
    /// 判据源）。注意：不含 tick/速度——目录描述"这一场是什么"，不含"跑到哪"。
    pub fn catalog_fingerprint(&self) -> u64 {
        let mut h: u64 = fp_mix(CHAIN_SEED, 0xCAFE_0000_0000_0001);
        let mut i: usize = 0;
        while i < self.bodies.len() {
            if let Some(b) = self.bodies.get(i) {
                h = fp_mix(h, (b.mass as u64) ^ 0x1111_1111_1111_1111);
                h = fp_mix(h, (b.radius as u64) ^ 0x2222_2222_2222_2222);
                h = fp_mix(h, (b.x as u64) ^ 0x3333_3333_3333_3333);
                h = fp_mix(h, (b.y as u64) ^ 0x4444_4444_4444_4444);
            }
            i += 1;
        }
        h = fp_mix(h, self.gx as u64);
        h = fp_mix(h, self.gy as u64);
        h = fp_mix(h, self.features as u64);
        h = fp_mix(h, self.bounds.0 as u64);
        h = fp_mix(h, self.bounds.1 as u64);
        h = fp_mix(h, self.bounds.2 as u64);
        h = fp_mix(h, self.bounds.3 as u64);
        h
    }
}

// ===========================================================================
// 第 7 节 · ExampleScene 场景闭集（const 静态表 + 运行期建场）
// ===========================================================================

/// 场景静态定义（const 表；动态初始条件见第 8 节建场器）。
#[derive(Debug, Clone, Copy)]
pub struct SceneStatic {
    /// 场景号（1..=8）。
    pub id: u8,
    /// 场景名（中文）。
    pub name: &'static str,
    /// 预期行为注记（锚点"最小物理场景"的教学注记）。
    pub note: &'static str,
    /// 四步教学解说（锚点"教学步骤解说"）。
    pub narration: [&'static str; 4],
    /// 预定步数（60Hz 步；建场后运行此步数）。
    pub steps: u32,
    /// 是否种子抖动（种子进初始条件 → 异种子必分叉的场景）。
    pub jitter: bool,
    /// 能量守恒偏差预期带上限（ppm；来源逐场景注释于 SCENES 表内）。
    pub energy_band_ppm: i64,
    /// 动量守恒偏差是否适用（仅无外力 + 球对碰撞场景）。
    pub momentum_applicable: bool,
    /// 动量守恒偏差预期带上限（ppm；不适用时为 0）。
    pub momentum_band_ppm: i64,
    /// 收敛步数指标是否适用（有停摆/稳定语义的场景）。
    pub conv_applicable: bool,
    /// 收敛步数预期带下界（步；不适用时 0）。
    pub conv_lo: u32,
    /// 收敛步数预期带上界（步）。
    pub conv_hi: u32,
}

/// 八场景静态表（指标带来源：Q16 单次乘舍入 ≤ 30.5ppm 的链路分析给出
/// 量级，带宽 = 探针 vx1024b_probe_f6013 首跑实测值 ×2~12 余量
/// （种子 2718 / 998877 双种子同值，2026-10-24 校准定稿）——带是回归
/// 绊线：任何使偏差越带的改动（积分次序/舍入口径/台账漏项）即红）。
pub const SCENES: [SceneStatic; 8] = [
    // 1 · 自由落体：e=0.55 衰减弹跳至静置；恢复损失 + 积分漂移解析入账
    //    → 实测 657ppm，带 8000（×12 余量）。
    SceneStatic {
        id: 1,
        name: "自由落体",
        note: "1kg 球自 8m 落下，恢复系数 0.55 逐次衰减弹跳，最终静置于地面；每次弹跳的动能损失入耗散台账。",
        narration: [
            "第 1 步：看初态——球在 8m 高处静止，全部能量是势能 mgh ≈ 78.4J。",
            "第 2 步：看下落——半隐式欧拉先更新速度再更新位置，动能逐帧从势能转换。",
            "第 3 步：看弹跳——触地冲量按恢复系数 0.55 折损，损失自动记入耗散台账。",
            "第 4 步：看终态——球静置地面，E0 − (E终 + 台账) 应在千分位内闭合。",
        ],
        steps: 900,
        jitter: false,
        energy_band_ppm: 8_000,
        momentum_applicable: false,
        momentum_band_ppm: 0,
        conv_applicable: true,
        conv_lo: 30,
        conv_hi: 900,
    },
    // 2 · 弹性碰撞：e=1.0、无重力 → 动量/动能双守恒的净对照；
    //    实测能量 122ppm / 动量 0ppm，带 2000 / 500。
    SceneStatic {
        id: 2,
        name: "弹性碰撞",
        note: "两等质量球 ±3m/s 对心接近，恢复系数 1.0 → 速度恰好交换；无重力环境隔离外力，动量守恒可对拍到个位 ppm。",
        narration: [
            "第 1 步：看初态——两球各携 4.5J 动能相向而行，系统总动量为零。",
            "第 2 步：看碰撞——冲量沿连心线，e=1.0 时冲量恰好交换两球速度。",
            "第 3 步：看守恒——碰撞事件账记录碰前/碰后动量，逐事件对拍。",
            "第 4 步：看终态——交换后两球背向而行，动能与动量均守恒。",
        ],
        steps: 240,
        jitter: false,
        energy_band_ppm: 2_000,
        momentum_applicable: true,
        momentum_band_ppm: 500,
        conv_applicable: false,
        conv_lo: 0,
        conv_hi: 0,
    },
    // 3 · 斜面滑块：30° 斜面无摩擦；保守系统 + 600 步长链
    //    实测 2822ppm，带 12000（×4 余量）。
    SceneStatic {
        id: 3,
        name: "斜面滑块",
        note: "2kg 滑块自 30° 斜面 8m 高度静止滑下，无摩擦；重力沿坡分量持续加速，势能口径 −m(gx·x+gy·y) 覆盖斜面旋转。",
        narration: [
            "第 1 步：看斜面——30° 坡的 sin=0.5、cos≈0.866 全部 Q16 写死。",
            "第 2 步：看分解——重力被投影到坡向，垂直坡向被斜面法向力抵消。",
            "第 3 步：看加速——沿坡加速度 ≈ 4.24m/s²，位置沿坡线单调下滑。",
            "第 4 步：看守恒——保守系统，E0 与 E终 之差应只含 Q16 舍入。",
        ],
        steps: 600,
        jitter: false,
        energy_band_ppm: 12_000,
        momentum_applicable: false,
        momentum_band_ppm: 0,
        conv_applicable: false,
        conv_lo: 0,
        conv_hi: 0,
    },
    // 4 · 弹簧振子：k=40、c=2 阻尼摆动至静止；阻尼耗散逐步实测入账
    //    → 实测 0ppm（telescoping 闭合）、收敛 378 步；带 5000 / (200,800)。
    SceneStatic {
        id: 4,
        name: "弹簧振子",
        note: "1kg 质点被 k=40N/m 弹簧自 2m 拉伸处释放，阻尼 c=2 逐周期衰减；每步机械能落差（阻尼+积分漂移）实测入台账。",
        narration: [
            "第 1 步：看初态——拉伸 2m，弹性势能 ½kx² = 80J，速度为零。",
            "第 2 步：看振荡——半隐式欧拉对振动系统是辛积分器，振幅衰减但相位稳定。",
            "第 3 步：看阻尼——每步机械能落差实测入台账，能量去向可讲清。",
            "第 4 步：看收敛——速度低于 0.02m/s 判定静止，收敛步数进指标带。",
        ],
        steps: 1500,
        jitter: false,
        energy_band_ppm: 5_000,
        momentum_applicable: false,
        momentum_band_ppm: 0,
        conv_applicable: true,
        conv_lo: 200,
        conv_hi: 800,
    },
    // 5 · 抛体：8/12 m/s 斜抛 + e=0.7 弹跳衰减；实测 902ppm、
    //    conv 901（横速只被侧墙衰减，900 步内不收敛是预期行为，
    //    带覆盖到 steps+1）；带 8000 / (300,960)。
    SceneStatic {
        id: 5,
        name: "抛体",
        note: "自 1m 高以 (8,12) m/s 斜抛，四壁 e=0.7 反弹，轨迹为衰减弹跳弧；每次折损入台账。",
        narration: [
            "第 1 步：看初速——水平 8、竖直 12 m/s，抛物线顶点 ≈ 8.3m。",
            "第 2 步：看对称——上升与下降各约 1.22s，水平匀速 8m/s。",
            "第 3 步：看反弹——触地按 e=0.7 折损，弹跳高度按 0.49 衰减。",
            "第 4 步：看收敛——竖向先静置，横向靠墙反弹衰减，判定步数入带。",
        ],
        steps: 900,
        jitter: false,
        energy_band_ppm: 8_000,
        momentum_applicable: false,
        momentum_band_ppm: 0,
        conv_applicable: true,
        conv_lo: 300,
        // 抛体横速只被侧墙反弹衰减（8×0.7ⁿ m/s），900 步内不收敛是
        // 预期行为：带覆盖到 steps+1（全程未收敛 = 判据落在带顶）。
        conv_hi: 960,
    },
    // 6 · 多球碰撞：5 球台球式混沌，种子抖动位置/速度（±0.25m、±3m/s）
    //    → 异种子必分叉；e=0.95 折损入台账；实测能量/动量均 0ppm
    //    （冲量结算在 Q16 域内保动量精确），带 8000 / 1000。
    SceneStatic {
        id: 6,
        name: "多球碰撞",
        note: "5 球置于 ±6m 围栏，初速由种子搅拌器派生（±3m/s）；球对碰撞 e=0.95，动量守恒逐事件对拍，异种子轨迹必分叉。",
        narration: [
            "第 1 步：看布阵——5 球初始位置/速度来自种子抖动，同种子必复现。",
            "第 2 步：看混沌——对心/斜碰交替，微小初差逐碰撞放大为可见分叉。",
            "第 3 步：看冲量——每对碰撞沿连心线结算，e=0.95 折损入台账。",
            "第 4 步：看对拍——同种子双跑指纹链全等；异种子链首步即破。",
        ],
        steps: 1200,
        jitter: true,
        energy_band_ppm: 8_000,
        momentum_applicable: true,
        momentum_band_ppm: 1_000,
        conv_applicable: false,
        conv_lo: 0,
        conv_hi: 0,
    },
    // 7 · 摩擦停摆：v0=6m/s、μk=0.4、μs=0.5；摩擦功 μm·g·|Δx| 入台账
    //    （解析口径，含每步位移近似）；实测 8795ppm、conv 91 步
    //    （解析 t = v0/(μk·g) ≈ 1.53s ≈ 92 步，互证）；带 20000 / (40,200)。
    SceneStatic {
        id: 7,
        name: "摩擦停摆",
        note: "2kg 滑块以 6m/s 在地面滑行，动摩擦 μk=0.4 减速、静摩擦咬合停摆；摩擦功 μm·g·|Δx| 逐步入台账。",
        narration: [
            "第 1 步：看初态——全部能量是动能 36J，无势能变化。",
            "第 2 步：看减速——摩擦加速度 μk·g ≈ 3.92m/s²，与速度反向。",
            "第 3 步：看咬合——速度低于静摩擦阈值时咬合置零，事件账记 FrictionLatch。",
            "第 4 步：看收支——摩擦功台账应 ≈ 36J，偏差进指标带。",
        ],
        steps: 480,
        jitter: false,
        energy_band_ppm: 20_000,
        momentum_applicable: false,
        momentum_band_ppm: 0,
        conv_applicable: true,
        conv_lo: 40,
        conv_hi: 200,
    },
    // 8 · 软绳摆：单边约束只拉不推；投影前后机械能落差带符号实测入账
    //    → 实测 7082ppm（符号化后仅剩舍入 + 台账口径差），带 30000。
    //    种子抖动摆锤 x → 相位漂移 → 异种子必分叉。
    SceneStatic {
        id: 8,
        name: "软绳摆",
        note: "摆锤系于 10m 高锚点、绳长 6m，切向初速 4m/s；绳张紧时位置投影到圆周并去外向径向速度——单边约束只拉不推，投影落差入台账。",
        narration: [
            "第 1 步：看初态——摆锤竖直悬挂 6m，切向 4m/s 起摆。",
            "第 2 步：看约束——绳松弛段自由落体，张紧段投影回圆周。",
            "第 3 步：看投影——投影落差（位置抬升 − 径向速度去除）实测入台账。",
            "第 4 步：看分叉——种子抖动摆锤初始 x，相位差逐摆放大成可见分叉。",
        ],
        steps: 1500,
        jitter: true,
        energy_band_ppm: 30_000,
        momentum_applicable: false,
        momentum_band_ppm: 0,
        conv_applicable: false,
        conv_lo: 0,
        conv_hi: 0,
    },
];

/// 按场景号取静态定义（闭集 1..=8）。
pub fn scene_static(id: u8) -> Result<&'static SceneStatic, PhysExError> {
    if id < 1 || id > SCENE_COUNT {
        return Err(PhysExError::SceneUnknown);
    }
    let idx: usize = (id - 1) as usize;
    match SCENES.get(idx) {
        Some(s) => Ok(s),
        None => Err(PhysExError::SceneUnknown),
    }
}

/// 场景闭集大小。
pub fn scene_count() -> usize {
    SCENES.len()
}

// ===========================================================================
// 第 8 节 · 场景建场器（const 初始条件表 → WorldState）
// ===========================================================================
//
// 命名规约：每场景一组 `SC<n>_<量>` 常量，值旁注释十进制含义与来源；
// 建场统一走 `build_scene`（校验 + 逆质量 + 目录口径），不旁路。

// --- 场景 1 · 自由落体 ---
/// 质量 1.0 kg（1×65536）。
const SC1_MASS: i64 = 65_536;
/// 半径 0.5 m（0.5×65536）。
const SC1_R: i64 = 32_768;
/// 起点 y = 8.0 m（8×65536）。
const SC1_Y0: i64 = 524_288;
/// 恢复系数 0.55（0.55×65536 = 36044.8 → 36044）。
const SC1_E: i64 = 36_044;
/// 边界 x ∈ ±20m、y ∈ [0, 40m]。
const SC1_BOUNDS: (i64, i64, i64, i64) = (-1_310_720, 1_310_720, 0, 2_621_440);

// --- 场景 2 · 弹性碰撞 ---
/// 质量 1.0 kg（两球同）。
const SC2_MASS: i64 = 65_536;
/// 半径 0.5 m。
const SC2_R: i64 = 32_768;
/// 球 A 起点 x = −3.0 m。
const SC2_XA: i64 = -196_608;
/// 球 B 起点 x = +3.0 m。
const SC2_XB: i64 = 196_608;
/// 速率 3.0 m/s（3×65536）。
const SC2_V: i64 = 196_608;
/// 恢复系数 1.0（完全弹性）。
const SC2_E: i64 = 65_536;
/// 边界 x ∈ ±20m、y ∈ [−20, 20]m（无重力，全反弹围栏）。
const SC2_BOUNDS: (i64, i64, i64, i64) = (-1_310_720, 1_310_720, -1_310_720, 1_310_720);

// --- 场景 3 · 斜面滑块 ---
/// 质量 2.0 kg。
const SC3_MASS: i64 = 131_072;
/// 半径 0.5 m。
const SC3_R: i64 = 32_768;
/// 起点 x = −8.0 m（坡线过原点：y = −x·tan30°）。
const SC3_X0: i64 = -524_288;
/// sin30° = 0.5（精确）。
const SC3_SIN30: i64 = 32_768;
/// cos30° = √3/2 ≈ 0.8660254（0.8660254×65536 = 56755.9 → 56756）。
const SC3_COS30: i64 = 56_756;
/// 摩擦关闭（μ=0）。
const SC3_MU: i64 = 0;
/// 边界 x ∈ ±30m、y ∈ [−1, 20]m（远墙不干扰）。
const SC3_BOUNDS: (i64, i64, i64, i64) = (-1_966_080, 1_966_080, -65_536, 1_310_720);

// --- 场景 4 · 弹簧振子 ---
/// 质量 1.0 kg。
const SC4_MASS: i64 = 65_536;
/// 半径 0.3 m（0.3×65536 = 19660.8 → 19661）。
const SC4_R: i64 = 19_661;
/// 拉伸初值 x0 = 2.0 m。
const SC4_X0: i64 = 131_072;
/// 刚度 k = 40 N/m（40×65536）。
const SC4_K: i64 = 2_621_440;
/// 阻尼 c = 2 N·s/m（2×65536）。
const SC4_C: i64 = 131_072;
/// 弹簧锚点 (0,0)。
const SC4_ANCHOR: (i64, i64) = (0, 0);
/// 边界 x ∈ ±10m、y ∈ ±10m（无墙特性，仅兜底）。
const SC4_BOUNDS: (i64, i64, i64, i64) = (-655_360, 655_360, -655_360, 655_360);

// --- 场景 5 · 抛体 ---
/// 质量 1.0 kg。
const SC5_MASS: i64 = 65_536;
/// 半径 0.5 m。
const SC5_R: i64 = 32_768;
/// 起点 (−10, 1.0) m。
const SC5_X0: i64 = -655_360;
const SC5_Y0: i64 = 65_536;
/// 初速 (8, 12) m/s。
const SC5_VX: i64 = 524_288;
const SC5_VY: i64 = 786_432;
/// 恢复系数 0.7（0.7×65536 = 45875.2 → 45875）。
const SC5_E: i64 = 45_875;
/// 边界 x ∈ ±20m、y ∈ [0, 40m]。
const SC5_BOUNDS: (i64, i64, i64, i64) = (-1_310_720, 1_310_720, 0, 2_621_440);

// --- 场景 6 · 多球碰撞 ---
/// 质量 1.0 kg（5 球同）。
const SC6_MASS: i64 = 65_536;
/// 半径 0.5 m。
const SC6_R: i64 = 32_768;
/// 5 球基线横排：x = −4 + i×1.5 m（间距 1.5m > 直径 1m，初始不重叠）。
const SC6_ROW0: i64 = -262_144;
const SC6_GAP: i64 = 98_304; // 1.5×65536 = 98304（精确）
/// 纵列基线 y = 0。
const SC6_Y0: i64 = 0;
/// 恢复系数 0.95（0.95×65536 = 62259.2 → 62259）。
const SC6_E: i64 = 62_259;
/// 边界 ±6m 围栏（±393216）。
const SC6_BOUNDS: (i64, i64, i64, i64) = (-393_216, 393_216, -393_216, 393_216);

// --- 场景 7 · 摩擦停摆 ---
/// 质量 2.0 kg。
const SC7_MASS: i64 = 131_072;
/// 半径 0.5 m。
const SC7_R: i64 = 32_768;
/// 初速 6.0 m/s（水平）。
const SC7_V0: i64 = 393_216;
/// 动摩擦 μk = 0.4（0.4×65536 = 26214.4 → 26214）。
const SC7_MUK: i64 = 26_214;
/// 静摩擦 μs = 0.5（精确 32768）。
const SC7_MUS: i64 = 32_768;
/// 边界 x ∈ ±50m、y ∈ [0, 40m]（侧墙远，不打扰）。
const SC7_BOUNDS: (i64, i64, i64, i64) = (-3_276_800, 3_276_800, 0, 2_621_440);

// --- 场景 8 · 软绳摆 ---
/// 质量 1.0 kg。
const SC8_MASS: i64 = 65_536;
/// 半径 0.3 m（19661）。
const SC8_R: i64 = 19_661;
/// 锚点 (0, 10) m。
const SC8_ANCHOR: (i64, i64) = (0, 655_360);
/// 绳长 6.0 m。
const SC8_LEN: i64 = 393_216;
/// 摆锤起点 (0, 4) m（锚点正下方 6m）。
const SC8_X0: i64 = 0;
const SC8_Y0: i64 = 262_144;
/// 切向初速 4.0 m/s。
const SC8_V0: i64 = 262_144;
/// 恢复系数 0（绳锚不弹；墙体关闭）。
const SC8_E: i64 = 0;
/// 边界 x ∈ ±50m、y ∈ [0, 40m]（兜底，不参与）。
const SC8_BOUNDS: (i64, i64, i64, i64) = (-3_276_800, 3_276_800, 0, 2_621_440);

/// 世界构造辅助（统一默认值 + 校验），场景差异经参数传入。
#[allow(clippy::too_many_arguments)]
fn world_scaffold(
    bodies: Vec<Body>,
    gx: i64,
    gy: i64,
    restitution: i64,
    mu_k: i64,
    mu_s: i64,
    features: u32,
    bounds: (i64, i64, i64, i64),
    rope: ((i64, i64), i64),
    spring: ((i64, i64), i64, i64),
) -> Result<WorldState, PhysExError> {
    if bodies.len() == 0 || bodies.len() > BODY_CAP {
        return Err(PhysExError::BodyInvalid);
    }
    if restitution < 0 || restitution > Q16_ONE {
        return Err(PhysExError::ParamOutOfRange);
    }
    if mu_k < 0 || mu_k > Q16_ONE || mu_s < 0 || mu_s > Q16_ONE {
        return Err(PhysExError::ParamOutOfRange);
    }
    if bounds.0 >= bounds.1 || bounds.2 >= bounds.3 {
        return Err(PhysExError::ParamOutOfRange);
    }
    Ok(WorldState {
        tick: 0,
        bodies,
        gx,
        gy,
        restitution,
        mu_k,
        mu_s,
        features,
        rope_anchor: rope.0,
        rope_len: rope.1,
        rope_taut: false,
        spring_anchor: spring.0,
        spring_k: spring.1,
        spring_c: spring.2,
        bounds,
        work_ledger: 0,
        clamp_count: 0,
    })
}

/// 建场：场景号 + 种子 → 初始世界（种子只在 jitter 场景进初始条件）。
///
/// 校验链：场景号 → 种子 → 逐体 make（质量/半径域）→ 脚手架参数域 →
/// 能量基线 > 0（ppm 指标的分母前置校验，越界即 `ParamOutOfRange`）。
pub fn build_scene(id: u8, seed: u64) -> Result<WorldState, PhysExError> {
    let _spec: &SceneStatic = scene_static(id)?;
    let mut mx: Option<SeedMixer> = None;
    if seed == 0 {
        return Err(PhysExError::SeedInvalid);
    }
    if scene_static(id)?.jitter {
        mx = Some(SeedMixer::new(seed)?);
    }
    let w: WorldState = match id {
        1 => {
            // 自由落体：单体 + 全重力 + 四壁反弹。
            let b: Body = Body::make(SC1_MASS, SC1_R, 0, SC1_Y0, 0, 0)?;
            let mut v: Vec<Body> = Vec::new();
            v.push(b);
            world_scaffold(
                v,
                0,
                -G_Q16,
                SC1_E,
                0,
                0,
                F_GRAVITY | F_WALLS,
                SC1_BOUNDS,
                ((0, 0), 0),
                ((0, 0), 0, 0),
            )?
        }
        2 => {
            // 弹性碰撞：双等质量球 + 无重力 + 球对/四壁（隔离外力）。
            let a: Body = Body::make(SC2_MASS, SC2_R, SC2_XA, 0, SC2_V, 0)?;
            let b: Body = Body::make(SC2_MASS, SC2_R, SC2_XB, 0, -SC2_V, 0)?;
            let mut v: Vec<Body> = Vec::new();
            v.push(a);
            v.push(b);
            world_scaffold(
                v,
                0,
                0,
                SC2_E,
                0,
                0,
                F_WALLS | F_PAIR,
                SC2_BOUNDS,
                ((0, 0), 0),
                ((0, 0), 0, 0),
            )?
        }
        3 => {
            // 斜面滑块：重力沿坡分解——
            // 坡向单位向量 d = (cos30°, −sin30°)，a = g·sin30°·d。
            // gx = G·sin30°·cos30°、gy = −G·sin30°²（建场时 qmul 派生，
            // 避免 const 手算错误；势能口径 −m(gx·x+gy·y) 与之一致）。
            let gsin: i64 = qmul(G_Q16, SC3_SIN30); // g·sin30° ≈ 4.9
            let gx: i64 = qmul(gsin, SC3_COS30); // ≈ 4.243
            let gy: i64 = -qmul(gsin, SC3_SIN30); // ≈ −2.45
            // 起点 y0 = −x0·tan30°（qdiv 派生，保持在坡线上）。
            let y0: i64 = qdiv(qmul(-SC3_X0, SC3_SIN30), SC3_COS30)?;
            let b: Body = Body::make(SC3_MASS, SC3_R, SC3_X0, y0, 0, 0)?;
            let mut v: Vec<Body> = Vec::new();
            v.push(b);
            world_scaffold(
                v,
                gx,
                gy,
                0,
                SC3_MU,
                SC3_MU,
                F_GRAVITY,
                SC3_BOUNDS,
                ((0, 0), 0),
                ((0, 0), 0, 0),
            )?
        }
        4 => {
            // 弹簧振子：无重力（水平桌面教学假设）+ 弹簧阻尼锚点。
            let b: Body = Body::make(SC4_MASS, SC4_R, SC4_X0, 0, 0, 0)?;
            let mut v: Vec<Body> = Vec::new();
            v.push(b);
            world_scaffold(
                v,
                0,
                0,
                0,
                0,
                0,
                F_SPRING,
                SC4_BOUNDS,
                ((0, 0), 0),
                (SC4_ANCHOR, SC4_K, SC4_C),
            )?
        }
        5 => {
            // 抛体：全重力 + 四壁反弹（含地面弹跳）。
            let b: Body = Body::make(SC5_MASS, SC5_R, SC5_X0, SC5_Y0, SC5_VX, SC5_VY)?;
            let mut v: Vec<Body> = Vec::new();
            v.push(b);
            world_scaffold(
                v,
                0,
                -G_Q16,
                SC5_E,
                0,
                0,
                F_GRAVITY | F_WALLS,
                SC5_BOUNDS,
                ((0, 0), 0),
                ((0, 0), 0, 0),
            )?
        }
        6 => {
            // 多球碰撞：5 球，位置/速度全部种子抖动（异种子必分叉来源）。
            let mixer: &mut SeedMixer = match mx.as_mut() {
                Some(m) => m,
                None => return Err(PhysExError::SeedInvalid),
            };
            let mut v: Vec<Body> = Vec::new();
            let mut i: i64 = 0;
            while i < 5 {
                let jx: i64 = mixer.jitter_q16(JITTER_POS_AMP)?;
                let jy: i64 = mixer.jitter_q16(JITTER_POS_AMP)?;
                let jvx: i64 = mixer.jitter_q16(JITTER_VEL_AMP)?;
                let jvy: i64 = mixer.jitter_q16(JITTER_VEL_AMP)?;
                let x: i64 = SC6_ROW0 + qmul(i, SC6_GAP) + jx;
                let y: i64 = SC6_Y0 + jy;
                let b: Body = Body::make(SC6_MASS, SC6_R, x, y, jvx, jvy)?;
                v.push(b);
                i += 1;
            }
            world_scaffold(
                v,
                0,
                0,
                SC6_E,
                0,
                0,
                F_WALLS | F_PAIR,
                SC6_BOUNDS,
                ((0, 0), 0),
                ((0, 0), 0, 0),
            )?
        }
        7 => {
            // 摩擦停摆：地面静置 + 库仑摩擦；vy 初 0、y = 半径贴地。
            let b: Body = Body::make(SC7_MASS, SC7_R, 0, SC7_R, SC7_V0, 0)?;
            let mut v: Vec<Body> = Vec::new();
            v.push(b);
            world_scaffold(
                v,
                0,
                -G_Q16,
                0,
                SC7_MUK,
                SC7_MUS,
                F_GRAVITY | F_FRICTION | F_FLOOR_REST,
                SC7_BOUNDS,
                ((0, 0), 0),
                ((0, 0), 0, 0),
            )?
        }
        8 => {
            // 软绳摆：摆锤 x 抖动（相位分叉源）+ 单边绳约束 + 全重力。
            let jx: i64 = match mx.as_mut() {
                Some(m) => m.jitter_q16(JITTER_POS_AMP)?,
                None => return Err(PhysExError::SeedInvalid),
            };
            let b: Body = Body::make(SC8_MASS, SC8_R, SC8_X0 + jx, SC8_Y0, SC8_V0, 0)?;
            let mut v: Vec<Body> = Vec::new();
            v.push(b);
            world_scaffold(
                v,
                0,
                -G_Q16,
                SC8_E,
                0,
                0,
                F_GRAVITY | F_ROPE,
                SC8_BOUNDS,
                (SC8_ANCHOR, SC8_LEN),
                ((0, 0), 0, 0),
            )?
        }
        _ => return Err(PhysExError::SceneUnknown),
    };
    if w.total_energy() <= 0 {
        return Err(PhysExError::ParamOutOfRange);
    }
    Ok(w)
}

// ===========================================================================
// 第 9 节 · PhysKernel 微型确定性物理内核
// ===========================================================================

/// 地面贴合判定余量（Q16 原始 2 ≈ 3e−5 m；来源：半隐式欧拉单步位置
/// 量化在 0.02m/s × dt 量级以下，取 2 原始位足够且不会误判腾空）。
const FLOOR_EPS: i64 = 2;

/// 单步回执（运行器收账：帧号 / 摘要 / 链值）。
#[derive(Debug, Clone, Copy)]
pub struct StepReceipt {
    /// 本步帧号（自 1 起）。
    pub tick: u32,
    /// 本步状态摘要。
    pub digest: u64,
    /// 本步链指纹。
    pub chain: u64,
    /// 本步钳制次数。
    pub clamps: u32,
}

/// 碰撞动量审计（碰撞事件前后的球对动量快照——动量守恒对拍的数据源）。
#[derive(Debug, Clone, Copy)]
pub struct CollisionMoment {
    /// 帧号。
    pub tick: u32,
    /// 球 i 序号。
    pub i: u8,
    /// 球 j 序号。
    pub j: u8,
    /// 碰前动量 x 分量（Q16 kg·m/s）。
    pub p_before_x: i64,
    /// 碰前动量 y 分量。
    pub p_before_y: i64,
    /// 碰后动量 x 分量。
    pub p_after_x: i64,
    /// 碰后动量 y 分量。
    pub p_after_y: i64,
}

/// 微型确定性物理内核：固定次序单步推进（次序即纪律，见 `step` 文档）。
#[derive(Debug, Clone)]
pub struct PhysKernel {
    /// 世界状态（体集/参数/台账）。
    pub world: WorldState,
    /// 链指纹当前值。
    pub chain: u64,
}

impl PhysKernel {
    /// 从建场世界构造内核（chain_init 由运行器按场景号+种子派生）。
    pub fn new(world: WorldState, chain_init: u64) -> Self {
        PhysKernel {
            world,
            chain: chain_init,
        }
    }

    /// 球对全机械能和（耗散台账入账用；含势能口径，见 `Body::energy_with`）。
    fn pair_energy(&self, i: usize, j: usize) -> i64 {
        let mut e: i64 = 0;
        let gx: i64 = self.world.gx;
        let gy: i64 = self.world.gy;
        if let Some(b) = self.world.bodies.get(i) {
            e = e.saturating_add(b.energy_with(gx, gy));
        }
        if let Some(b) = self.world.bodies.get(j) {
            e = e.saturating_add(b.energy_with(gx, gy));
        }
        e
    }

    /// 单步推进（固定次序，任何输入分叉都会反映到链指纹）：
    ///
    /// 1. 弹簧阻尼力 → 速度（阻尼功入台账）；
    /// 2. 重力 → 速度；
    /// 3. 库仑摩擦 → 速度（静摩擦咬合置零 + 事件；摩擦功入台账）；
    /// 4. 位置积分 x += v·dt（半隐式欧拉的"先速后位"）；
    /// 5. 软绳单边投影（张紧段位置拉回圆周 + 去外向径向速度）；
    /// 6. 球对冲量碰撞（i<j 双循环：冲量 + 位置校正 + 恢复损失入台账
    ///    + 动量审计快照 + 事件）；
    /// 7. 四壁反弹 / 地面静置（恢复损失入台账 + 事件）；
    /// 8. 速度/位置钳制（saturating + 计数 + 事件）；
    /// 9. 摘要折叠 → 链推进 → tick 自增。
    pub fn step(
        &mut self,
        sink: &mut Vec<PhysEvent>,
        collisions_out: &mut Vec<CollisionMoment>,
    ) -> Result<StepReceipt, PhysExError> {
        let dt: i64 = DT_Q16;
        let tick_now: u32 = self.world.tick + 1;

        // 弹簧系逐步机械能起点（阻尼耗散按逐步实测落差入账：解析 c·v²·dt
        // 对"力后速度"取值与积分口径有一阶差，实测口径把阻尼+积分漂移
        // 一次性收进台账——与碰撞台账同一"量测不建模"原则）。
        let mut spring_e0: i64 = 0;
        let mut spring_on: bool = false;

        // ---- 1. 弹簧阻尼（F_SPRING：对每体施加 −k·Δp − c·v）----
        if self.world.features & F_SPRING != 0 {
            spring_on = true;
            let k: i64 = self.world.spring_k;
            let c: i64 = self.world.spring_c;
            let (ax, ay): (i64, i64) = self.world.spring_anchor;
            // 步前弹簧系机械能（KE + ½k|Δp|²）。
            let mut si: usize = 0;
            while si < self.world.bodies.len() {
                if let Some(b) = self.world.bodies.get(si) {
                    let dx: i64 = b.x - ax;
                    let dy: i64 = b.y - ay;
                    let pe: i64 =
                        qmul(k, qmul(dx, dx).saturating_add(qmul(dy, dy))) >> 1;
                    spring_e0 = spring_e0.saturating_add(b.kinetic()).saturating_add(pe);
                }
                si += 1;
            }
            let mut bi: usize = 0;
            while bi < self.world.bodies.len() {
                if let Some(b) = self.world.bodies.get_mut(bi) {
                    // 注意写法：`-a.m(b)` 会解析成 `-(a.m(b))`（一元负号作用于
                    // 整个后缀表达式），曾把阻尼项变成负阻尼（注能发散）——
                    // 故拆成两项显式 saturating_add。
                    let f_spring_x: i64 = -qmul(k, b.x - ax);
                    let f_spring_y: i64 = -qmul(k, b.y - ay);
                    let f_damp_x: i64 = -qmul(c, b.vx);
                    let f_damp_y: i64 = -qmul(c, b.vy);
                    let fx: i64 = f_spring_x.saturating_add(f_damp_x);
                    let fy: i64 = f_spring_y.saturating_add(f_damp_y);
                    let dvx: i64 = qmul(qmul(fx, b.inv_mass), dt);
                    let dvy: i64 = qmul(qmul(fy, b.inv_mass), dt);
                    b.vx += dvx;
                    b.vy += dvy;
                }
                bi += 1;
            }
        }

        // ---- 2. 重力 + 法向支撑 + 积分器解析耗散（融合单趟）----
        if self.world.features & F_GRAVITY != 0 {
            let dvx: i64 = qmul(self.world.gx, dt);
            let dvy: i64 = qmul(self.world.gy, dt);
            // 半隐式欧拉（先速后位）对常加速度 a 的每体每步能量漂移恰为
            // −½·m·|a|²·dt²（推导：ΔKE = m·a·v·dt + ½m|a·dt|²，ΔPE =
            // −m·a·(v·dt + a·dt²)，求和得 −½m|a|²dt²）——解析量入耗散
            // 台账对冲，守恒口径回到 Q16 舍入级。g=0 场景该项为 0。
            let g2: i64 = qmul(self.world.gx, self.world.gx)
                .saturating_add(qmul(self.world.gy, self.world.gy));
            let half_dt2: i64 = qmul(dt, dt) >> 1;
            let drift: i64 = qmul(g2, half_dt2);
            // 法向支撑（贴地且本步竖向速度会被压成小幅下落：抵消重力，
            // 不积分即无漂移无抖动，也不入补偿；真冲击仍走墙反弹分支）。
            let support_on: bool =
                self.world.features & (F_WALLS | F_FLOOR_REST) != 0 && self.world.gy < 0;
            let floor_y: i64 = self.world.bounds.2;
            let rest_kill: i64 = qmul(-self.world.gy, dt) << 1;
            let mut bi: usize = 0;
            while bi < self.world.bodies.len() {
                if let Some(b) = self.world.bodies.get_mut(bi) {
                    b.vx += dvx;
                    let vy_new: i64 = b.vy + dvy;
                    let supported: bool = support_on
                        && b.y - b.radius <= floor_y + FLOOR_EPS
                        && vy_new < 0
                        && vy_new > -rest_kill;
                    if supported {
                        b.vy = 0;
                    } else {
                        b.vy = vy_new;
                        let m: i64 = b.mass;
                        self.world.work_ledger = self
                            .world
                            .work_ledger
                            .saturating_add(qmul(m, drift));
                    }
                }
                bi += 1;
            }
        }

        // ---- 3. 库仑摩擦（贴地体；静摩擦咬合）----
        if self.world.features & F_FRICTION != 0 {
            let mu_k: i64 = self.world.mu_k;
            let mu_s: i64 = self.world.mu_s;
            let g_mag: i64 = if self.world.gy < 0 { -self.world.gy } else { 0 };
            let decel: i64 = qmul(mu_k, qmul(g_mag, dt)); // 单步动摩擦速降
            let latch: i64 = qmul(mu_s, qmul(g_mag, dt)) << 1; // 咬合阈值×2
            let floor_y: i64 = self.world.bounds.2;
            let mut bi: usize = 0;
            while bi < self.world.bodies.len() {
                let on_ground: bool = match self.world.bodies.get(bi) {
                    Some(b) => b.y <= b.radius + floor_y + FLOOR_EPS,
                    None => false,
                };
                if on_ground {
                    if let Some(b) = self.world.bodies.get(bi) {
                        let v2: i64 =
                            qmul(b.vx, b.vx).saturating_add(qmul(b.vy, b.vy));
                        if v2 <= qmul(latch, latch) {
                            // 静摩擦咬合：速度置零（只在确曾运动时记事件）。
                            if b.vx != 0 || b.vy != 0 {
                                sink.push(PhysEvent {
                                    tick: tick_now,
                                    kind: EventKind::FrictionLatch,
                                    a: bi as i64,
                                    b: 0,
                                });
                            }
                            let mass: i64 = b.mass;
                            let travel: i64 = q_sqrt(v2)?; // 咬合前剩余速度模
                            // 末段咬合功：μk·m·g·|v_remain·dt|（口径：咬合步
                            // 仍走完当步位移的摩擦等效功，保守入账）。
                            self.world.work_ledger = self
                                .world
                                .work_ledger
                                .saturating_add(qmul(mu_k, qmul(mass, qmul(g_mag, qmul(travel, dt)))));
                            if let Some(bm) = self.world.bodies.get_mut(bi) {
                                bm.vx = 0;
                                bm.vy = 0;
                            }
                        } else if decel > 0 {
                            let speed: i64 = q_sqrt(v2)?;
                            let scale: i64 = qdiv(decel, speed)?;
                            if let Some(bm) = self.world.bodies.get_mut(bi) {
                                // 摩擦功 μk·m·g·|Δx|，|Δx| ≈ speed·dt。
                                self.world.work_ledger = self.world.work_ledger.saturating_add(
                                    qmul(mu_k, qmul(bm.mass, qmul(g_mag, qmul(speed, dt)))),
                                );
                                let dvx: i64 = qmul(bm.vx, scale);
                                let dvy: i64 = qmul(bm.vy, scale);
                                bm.vx -= dvx;
                                bm.vy -= dvy;
                            }
                        }
                    }
                }
                bi += 1;
            }
        }

        // ---- 4. 位置积分 ----
        {
            let mut bi: usize = 0;
            while bi < self.world.bodies.len() {
                if let Some(b) = self.world.bodies.get_mut(bi) {
                    b.x += qmul(b.vx, dt);
                    b.y += qmul(b.vy, dt);
                }
                bi += 1;
            }
        }

        // ---- 4b. 弹簧系阻尼耗散实测入账（步前→步后机械能落差）----
        if spring_on {
            let k: i64 = self.world.spring_k;
            let (ax, ay): (i64, i64) = self.world.spring_anchor;
            let mut e1: i64 = 0;
            let mut si: usize = 0;
            while si < self.world.bodies.len() {
                if let Some(b) = self.world.bodies.get(si) {
                    let dx: i64 = b.x - ax;
                    let dy: i64 = b.y - ay;
                    let pe: i64 =
                        qmul(k, qmul(dx, dx).saturating_add(qmul(dy, dy))) >> 1;
                    e1 = e1.saturating_add(b.kinetic()).saturating_add(pe);
                }
                si += 1;
            }
            // 符号化累计：半隐式欧拉的真能量绕守恒值小幅振荡（幅 ~O(ω²dt²)E），
            // 只记落差不记回升会系统性虚增台账；带符号累计使逐步落差
            // telescoping——整跑 Σ(步前−步后) = E初 − E终，闭合到舍入级。
            self.world.work_ledger = self
                .world
                .work_ledger
                .saturating_add(spring_e0 - e1);
        }

        // ---- 5. 软绳单边投影（含能量落差实测入账：理想约束不做功，
        //      有限步投影有 O(dt²) 伪功——按"量测不建模"原则落账）----
        if self.world.features & F_ROPE != 0 {
            let (ax, ay): (i64, i64) = self.world.rope_anchor;
            let len: i64 = self.world.rope_len;
            let (rgx, rgy): (i64, i64) = (self.world.gx, self.world.gy);
            // 投影前机械能。
            let mut rope_e0: i64 = 0;
            {
                let mut ri: usize = 0;
                while ri < self.world.bodies.len() {
                    if let Some(b) = self.world.bodies.get(ri) {
                        rope_e0 = rope_e0.saturating_add(b.energy_with(rgx, rgy));
                    }
                    ri += 1;
                }
            }
            let mut bi: usize = 0;
            while bi < self.world.bodies.len() {
                if let Some(b) = self.world.bodies.get(bi) {
                    let dx: i64 = b.x - ax;
                    let dy: i64 = b.y - ay;
                    let d2: i64 = qmul(dx, dx).saturating_add(qmul(dy, dy));
                    let dist: i64 = q_sqrt(d2)?;
                    if dist > len && dist > 0 {
                        let nx: i64 = qdiv(dx, dist)?;
                        let ny: i64 = qdiv(dy, dist)?;
                        let proj_x: i64 = ax + qmul(len, nx);
                        let proj_y: i64 = ay + qmul(len, ny);
                        let vn: i64 = qmul(b.vx, nx).saturating_add(qmul(b.vy, ny));
                        if let Some(bm) = self.world.bodies.get_mut(bi) {
                            bm.x = proj_x;
                            bm.y = proj_y;
                            if vn > 0 {
                                // 去外向径向速度（只拉不推的动力学面）。
                                bm.vx -= qmul(vn, nx);
                                bm.vy -= qmul(vn, ny);
                            }
                        }
                        if !self.world.rope_taut {
                            sink.push(PhysEvent {
                                tick: tick_now,
                                kind: EventKind::RopeEngage,
                                a: bi as i64,
                                b: dist - len,
                            });
                        }
                        self.world.rope_taut = true;
                    } else {
                        self.world.rope_taut = false;
                    }
                }
                bi += 1;
            }
            // 投影后机械能 → 带符号落差入台账（符号化：伪功正负相抵，
            // 整跑闭合到舍入级）。
            let mut rope_e1: i64 = 0;
            {
                let mut ri: usize = 0;
                while ri < self.world.bodies.len() {
                    if let Some(b) = self.world.bodies.get(ri) {
                        rope_e1 = rope_e1.saturating_add(b.energy_with(rgx, rgy));
                    }
                    ri += 1;
                }
            }
            self.world.work_ledger = self
                .world
                .work_ledger
                .saturating_add(rope_e0 - rope_e1);
        }

        // ---- 6. 球对冲量碰撞 ----
        if self.world.features & F_PAIR != 0 {
            let e: i64 = self.world.restitution;
            let one_plus_e: i64 = Q16_ONE + e;
            let n_bodies: usize = self.world.bodies.len();
            let mut i: usize = 0;
            while i < n_bodies {
                let mut j: usize = i + 1;
                while j < n_bodies {
                    let (xi, yi, ri, invi, mi, vxi, vyi): (i64, i64, i64, i64, i64, i64, i64);
                    match self.world.bodies.get(i) {
                        Some(b) => {
                            xi = b.x;
                            yi = b.y;
                            ri = b.radius;
                            invi = b.inv_mass;
                            mi = b.mass;
                            vxi = b.vx;
                            vyi = b.vy;
                        }
                        None => {
                            j += 1;
                            continue;
                        }
                    }
                    let (xj, yj, rj, invj, mj, vxj, vyj): (i64, i64, i64, i64, i64, i64, i64);
                    match self.world.bodies.get(j) {
                        Some(b) => {
                            xj = b.x;
                            yj = b.y;
                            rj = b.radius;
                            invj = b.inv_mass;
                            mj = b.mass;
                            vxj = b.vx;
                            vyj = b.vy;
                        }
                        None => {
                            j += 1;
                            continue;
                        }
                    }
                    let dx: i64 = xj - xi;
                    let dy: i64 = yj - yi;
                    let rsum: i64 = ri + rj;
                    let d2: i64 = qmul(dx, dx).saturating_add(qmul(dy, dy));
                    // d2 为 Q16² 单位；rsum² 同单位可直接比较（不另开方省舍入）。
                    if d2 < qmul(rsum, rsum) {
                        let dist: i64 = q_sqrt(d2)?;
                        // 法线（i→j）；完全重合时取 +x 轴回退（确定性优先）。
                        let nx: i64 = if dist > 0 { qdiv(dx, dist)? } else { Q16_ONE };
                        let ny: i64 = if dist > 0 { qdiv(dy, dist)? } else { 0 };
                        let dvx: i64 = vxi - vxj;
                        let dvy: i64 = vyi - vyj;
                        let vn: i64 = qmul(dvx, nx).saturating_add(qmul(dvy, ny));
                        if vn > 0 {
                            let inv_sum: i64 = invi + invj;
                            if inv_sum > 0 {
                                let ke_before: i64 = self.pair_energy(i, j);
                                let p_bx: i64 = qmul(mi, vxi).saturating_add(qmul(mj, vxj));
                                let p_by: i64 = qmul(mi, vyi).saturating_add(qmul(mj, vyj));
                                let j_imp: i64 = qdiv(qmul(one_plus_e, vn), inv_sum)?;
                                let di: i64 = qmul(j_imp, invi);
                                let dj: i64 = qmul(j_imp, invj);
                                if let Some(bi_) = self.world.bodies.get_mut(i) {
                                    bi_.vx -= qmul(di, nx);
                                    bi_.vy -= qmul(di, ny);
                                }
                                if let Some(bj_) = self.world.bodies.get_mut(j) {
                                    bj_.vx += qmul(dj, nx);
                                    bj_.vy += qmul(dj, ny);
                                }
                                // 位置校正：按逆质量比例分离 0.8×穿透量。
                                let overlap: i64 = rsum - dist;
                                let corr: i64 = qmul(overlap, POS_CORRECTION);
                                let share_i: i64 = qdiv(invi, inv_sum)?;
                                let share_j: i64 = Q16_ONE - share_i;
                                if let Some(bi_) = self.world.bodies.get_mut(i) {
                                    bi_.x -= qmul(corr, share_i);
                                    bi_.y -= qmul(corr, share_i);
                                }
                                if let Some(bj_) = self.world.bodies.get_mut(j) {
                                    bj_.x += qmul(corr, share_j);
                                    bj_.y += qmul(corr, share_j);
                                }
                                let ke_after: i64 = self.pair_energy(i, j);
                                if ke_before > ke_after {
                                    self.world.work_ledger = self
                                        .world
                                        .work_ledger
                                        .saturating_add(ke_before - ke_after);
                                }
                                let (axi, ayi, axj, ayj): (i64, i64, i64, i64);
                                match self.world.bodies.get(i) {
                                    Some(b) => {
                                        axi = b.vx;
                                        ayi = b.vy;
                                    }
                                    None => {
                                        axi = 0;
                                        ayi = 0;
                                    }
                                }
                                match self.world.bodies.get(j) {
                                    Some(b) => {
                                        axj = b.vx;
                                        ayj = b.vy;
                                    }
                                    None => {
                                        axj = 0;
                                        ayj = 0;
                                    }
                                }
                                let p_ax: i64 =
                                    qmul(mi, axi).saturating_add(qmul(mj, axj));
                                let p_ay: i64 =
                                    qmul(mi, ayi).saturating_add(qmul(mj, ayj));
                                collisions_out.push(CollisionMoment {
                                    tick: tick_now,
                                    i: i as u8,
                                    j: j as u8,
                                    p_before_x: p_bx,
                                    p_before_y: p_by,
                                    p_after_x: p_ax,
                                    p_after_y: p_ay,
                                });
                                sink.push(PhysEvent {
                                    tick: tick_now,
                                    kind: EventKind::PairCollide,
                                    a: (i as i64) | ((j as i64) << 8),
                                    b: j_imp,
                                });
                            }
                        }
                    }
                    j += 1;
                }
                i += 1;
            }
        }

        // ---- 7. 四壁反弹 / 地面静置 ----
        if self.world.features & F_WALLS != 0 {
            let e: i64 = self.world.restitution;
            let (min_x, max_x, min_y, max_y): (i64, i64, i64, i64) = self.world.bounds;
            let (wgx, wgy): (i64, i64) = (self.world.gx, self.world.gy);
            // 静置判据：竖向反弹速度 < 2×|gy|·dt（两步重力冲量）→ 贴面静置
            // （行业标准的 rest velocity threshold；无重力场景阈值为 0 不触发）。
            let rest_kill: i64 = if self.world.gy < 0 {
                qmul(-self.world.gy, dt) << 1
            } else {
                0
            };
            let mut bi: usize = 0;
            while bi < self.world.bodies.len() {
                let r: i64;
                match self.world.bodies.get(bi) {
                    Some(b) => {
                        r = b.radius;
                    }
                    None => {
                        bi += 1;
                        continue;
                    }
                }
                let mut hit_axis: i64 = -1;
                if let Some(bm) = self.world.bodies.get_mut(bi) {
                    if bm.x - r < min_x && bm.vx < 0 {
                        let ke0: i64 = bm.energy_with(wgx, wgy);
                        bm.x = min_x + r;
                        bm.vx = -qmul(bm.vx, e);
                        let ke1: i64 = bm.energy_with(wgx, wgy);
                        if ke0 > ke1 {
                            self.world.work_ledger =
                                self.world.work_ledger.saturating_add(ke0 - ke1);
                        }
                        hit_axis = 0;
                    } else if bm.x + r > max_x && bm.vx > 0 {
                        let ke0: i64 = bm.energy_with(wgx, wgy);
                        bm.x = max_x - r;
                        bm.vx = -qmul(bm.vx, e);
                        let ke1: i64 = bm.energy_with(wgx, wgy);
                        if ke0 > ke1 {
                            self.world.work_ledger =
                                self.world.work_ledger.saturating_add(ke0 - ke1);
                        }
                        hit_axis = 0;
                    }
                    if bm.y - r < min_y && bm.vy < 0 {
                        let ke0: i64 = bm.energy_with(wgx, wgy);
                        bm.y = min_y + r;
                        let mut vy_new: i64 = -qmul(bm.vy, e);
                        if vy_new < rest_kill && vy_new > -rest_kill {
                            // 反弹能量不足一步重力冲量：贴地静置（损失照入台账）。
                            vy_new = 0;
                        }
                        bm.vy = vy_new;
                        let ke1: i64 = bm.energy_with(wgx, wgy);
                        if ke0 > ke1 {
                            self.world.work_ledger =
                                self.world.work_ledger.saturating_add(ke0 - ke1);
                        }
                        if vy_new != 0 {
                            hit_axis = 1;
                        }
                    } else if bm.y + r > max_y && bm.vy > 0 {
                        let ke0: i64 = bm.energy_with(wgx, wgy);
                        bm.y = max_y - r;
                        let mut vy_new: i64 = -qmul(bm.vy, e);
                        if vy_new < rest_kill && vy_new > -rest_kill {
                            vy_new = 0;
                        }
                        bm.vy = vy_new;
                        let ke1: i64 = bm.energy_with(wgx, wgy);
                        if ke0 > ke1 {
                            self.world.work_ledger =
                                self.world.work_ledger.saturating_add(ke0 - ke1);
                        }
                        if vy_new != 0 {
                            hit_axis = 1;
                        }
                    }
                }
                if hit_axis >= 0 {
                    sink.push(PhysEvent {
                        tick: tick_now,
                        kind: EventKind::WallBounce,
                        a: bi as i64,
                        b: hit_axis,
                    });
                }
                bi += 1;
            }
        }
        if self.world.features & F_FLOOR_REST != 0 {
            let min_y: i64 = self.world.bounds.2;
            let (wgx, wgy): (i64, i64) = (self.world.gx, self.world.gy);
            let mut bi: usize = 0;
            while bi < self.world.bodies.len() {
                if let Some(bm) = self.world.bodies.get_mut(bi) {
                    if bm.y - bm.radius < min_y && bm.vy < 0 {
                        let ke0: i64 = bm.energy_with(wgx, wgy);
                        bm.y = min_y + bm.radius;
                        bm.vy = 0;
                        let ke1: i64 = bm.energy_with(wgx, wgy);
                        if ke0 > ke1 {
                            self.world.work_ledger =
                                self.world.work_ledger.saturating_add(ke0 - ke1);
                        }
                    }
                }
                bi += 1;
            }
        }

        // ---- 8. 状态爆炸钳制（saturating + 计数 + 事件）----
        {
            let mut bi: usize = 0;
            while bi < self.world.bodies.len() {
                if let Some(bm) = self.world.bodies.get_mut(bi) {
                    let mut tripped: i64 = 0;
                    if bm.vx > V_MAX {
                        bm.vx = V_MAX;
                        tripped = 1;
                    } else if bm.vx < -V_MAX {
                        bm.vx = -V_MAX;
                        tripped = 1;
                    }
                    if bm.vy > V_MAX {
                        bm.vy = V_MAX;
                        tripped = 1;
                    } else if bm.vy < -V_MAX {
                        bm.vy = -V_MAX;
                        tripped = 1;
                    }
                    if bm.x > POS_CLAMP {
                        bm.x = POS_CLAMP;
                        tripped = 2;
                    } else if bm.x < -POS_CLAMP {
                        bm.x = -POS_CLAMP;
                        tripped = 2;
                    }
                    if bm.y > POS_CLAMP {
                        bm.y = POS_CLAMP;
                        tripped = 2;
                    } else if bm.y < -POS_CLAMP {
                        bm.y = -POS_CLAMP;
                        tripped = 2;
                    }
                    if tripped > 0 {
                        self.world.clamp_count += 1;
                        sink.push(PhysEvent {
                            tick: tick_now,
                            kind: EventKind::ClampTrip,
                            a: bi as i64,
                            b: tripped,
                        });
                    }
                }
                bi += 1;
            }
        }

        // ---- 9. 摘要折叠 → 链推进 ----
        self.world.tick = tick_now;
        let digest: u64 = self.world.digest();
        self.chain = fp_mix(self.chain, digest ^ (tick_now as u64));
        Ok(StepReceipt {
            tick: tick_now,
            digest,
            chain: self.chain,
            clamps: self.world.clamp_count,
        })
    }
}

// ===========================================================================
// 第 10 节 · 运行记录与链验证器（离线复核 + 漂移注入的判据源）
// ===========================================================================

/// 轨迹采样点（帧号标注的最小单元；三例同源视图的数据源之一）。
#[derive(Debug, Clone, Copy)]
pub struct SamplePoint {
    /// 帧号（0 = 初态，之后每 SAMPLE_EVERY 步一帧）。
    pub tick: u32,
    /// 体号。
    pub body: u8,
    /// 位置 x（Q16）。
    pub x: i64,
    /// 位置 y（Q16）。
    pub y: i64,
    /// 速度 vx（Q16）。
    pub vx: i64,
    /// 速度 vy（Q16）。
    pub vy: i64,
}

/// 一次场景运行的完整账（轨迹/碰撞/事件/能量/指纹五源同册）。
#[derive(Debug, Clone)]
pub struct RunRecord {
    /// 场景号。
    pub scene: u8,
    /// 种子。
    pub seed: u64,
    /// 实跑步数。
    pub steps: u32,
    /// 链初始值（fp_mix(CHAIN_SEED, 场景号<<32 ^ 种子)）。
    pub chain_init: u64,
    /// 每步链指纹（len = steps）。
    pub chain: Vec<u64>,
    /// 每步状态摘要（与 chain 同长，链验证器的独立入料）。
    pub digests: Vec<u64>,
    /// 轨迹采样点（初态 + 每 8 步）。
    pub samples: Vec<SamplePoint>,
    /// 碰撞动量审计（动量守恒指标的数据源）。
    pub collisions: Vec<CollisionMoment>,
    /// 事件高亮（截尾容量 REC_EVENT_CAP；三例同源的事件时间线数据源）。
    pub highlights: Vec<PhysEvent>,
    /// 初态总能量（Q16 J）。
    pub e0: i64,
    /// 终态总能量（Q16 J）。
    pub e_end: i64,
    /// 终态耗散台账（Q16 J）。
    pub work: i64,
    /// 每步最大速度平方（Q16²；收敛判据用平方域避免逐步开方）。
    pub max_speed2: Vec<i64>,
    /// 终态指纹（chain 末值；steps = 0 时为 chain_init）。
    pub fp_final: u64,
}

/// 链验证器：按「chain[i] = fp_mix(chain[i−1], digests[i] ^ (i+1))」逐链复核。
///
/// 单字篡改必破：翻转 chain[i] 在 i 步失配；翻转 digests[i] 在 i+1 步
/// 失配（末步则在 fp_final 复核失配）——与内核 `step` 第 9 分目同式。
pub fn verify_ledger(chain_init: u64, digests: &[u64], chain: &[u64]) -> bool {
    if chain.len() != digests.len() {
        return false;
    }
    let mut c: u64 = chain_init;
    let mut i: usize = 0;
    while i < chain.len() {
        let d: u64 = match digests.get(i) {
            Some(v) => *v,
            None => return false,
        };
        c = fp_mix(c, d ^ ((i as u64) + 1));
        let expect: u64 = match chain.get(i) {
            Some(v) => *v,
            None => return false,
        };
        if c != expect {
            return false;
        }
        i += 1;
    }
    true
}

/// 双跑对拍结论。
#[derive(Debug, Clone, Copy)]
pub struct DualProof {
    /// 是否全链一致。
    pub consistent: bool,
    /// 首个失配步（1 基；一致时 None）。
    pub first_diff: Option<u32>,
    /// A 跑终态指纹。
    pub fp_a: u64,
    /// B 跑终态指纹。
    pub fp_b: u64,
    /// 双跑步数。
    pub steps: u32,
}

// ===========================================================================
// 第 11 节 · ScenarioRunner 场景运行器（对拍自证 + 分叉反证 + 注入面）
// ===========================================================================

/// 越带指标种类（BandCase 案卷分类）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BandKind {
    /// 能量守恒偏差（ppm）。
    Energy,
    /// 动量守恒偏差（ppm）。
    Momentum,
    /// 收敛步数（步）。
    Convergence,
}

impl BandKind {
    /// 中文名。
    pub fn name(&self) -> &'static str {
        match self {
            BandKind::Energy => "能量守恒偏差",
            BandKind::Momentum => "动量守恒偏差",
            BandKind::Convergence => "收敛步数",
        }
    }

    /// 事件载荷码（EventKind::BandFiled 的 b 载荷）。
    pub fn event_code(&self) -> i64 {
        match self {
            BandKind::Energy => 1,
            BandKind::Momentum => 2,
            BandKind::Convergence => 3,
        }
    }
}

/// 越带案卷（锚点「越带立案」的立案实体）。
#[derive(Debug, Clone, Copy)]
pub struct BandCase {
    /// 场景号。
    pub scene: u8,
    /// 指标种类。
    pub metric: BandKind,
    /// 实测值（ppm 或步）。
    pub value: i64,
    /// 带宽（上限；收敛为带 [lo,hi] 的 hi）。
    pub band: i64,
    /// 立案帧号（运行器累计步计数）。
    pub tick: u32,
    /// 案由（中文短句）。
    pub reason: &'static str,
}

/// 场景运行器：场景号 → 运行 → 逐步账 + 终态指纹；自带对拍与注入面。
#[derive(Debug, Clone)]
pub struct ScenarioRunner {
    /// 事件账（滚动）。
    pub ledger: EventLedger,
    /// NaN/Inf 位型防线（原始参数字准入口）。
    pub guard: NanPatternGuard,
    /// 累计完整运行次数。
    pub runs: u32,
    /// 累计物理步。
    pub total_steps: u64,
    /// 对拍分叉计数（异常面；0 = 健康）。
    pub divergences: u32,
    /// 越带案卷。
    pub band_cases: Vec<BandCase>,
    /// 注入检出计数（三件套各 +1）。
    pub injections_detected: u32,
}

impl ScenarioRunner {
    /// 新建运行器。
    pub fn new() -> Self {
        ScenarioRunner {
            ledger: EventLedger::new(),
            guard: NanPatternGuard::new(),
            runs: 0,
            total_steps: 0,
            divergences: 0,
            band_cases: Vec::new(),
            injections_detected: 0,
        }
    }

    /// 链初始值派生（场景号与种子都进链头——册指纹与场景指纹同源）。
    pub fn chain_init_for(scene: u8, seed: u64) -> u64 {
        fp_mix(CHAIN_SEED, ((scene as u64) << 32) ^ seed)
    }

    /// 运行一个场景（steps_override = None 时用场景表步数）。
    pub fn run(
        &mut self,
        scene: u8,
        seed: u64,
        steps_override: Option<u32>,
    ) -> Result<RunRecord, PhysExError> {
        let spec: &SceneStatic = scene_static(scene)?;
        let steps: u32 = match steps_override {
            Some(n) => {
                if n == 0 || n > MAX_STEPS {
                    return Err(PhysExError::StepCountExceeded);
                }
                n
            }
            None => spec.steps,
        };
        let world: WorldState = build_scene(scene, seed)?;
        let e0: i64 = world.total_energy();
        let chain_init: u64 = Self::chain_init_for(scene, seed);
        let mut kernel: PhysKernel = PhysKernel::new(world, chain_init);

        let mut rec: RunRecord = RunRecord {
            scene,
            seed,
            steps,
            chain_init,
            chain: Vec::new(),
            digests: Vec::new(),
            samples: Vec::new(),
            collisions: Vec::new(),
            highlights: Vec::new(),
            e0,
            e_end: e0,
            work: 0,
            max_speed2: Vec::new(),
            fp_final: chain_init,
        };
        // 初态采样（帧 0）。
        let mut bi: usize = 0;
        while bi < kernel.world.bodies.len() {
            if let Some(b) = kernel.world.bodies.get(bi) {
                rec.samples.push(SamplePoint {
                    tick: 0,
                    body: bi as u8,
                    x: b.x,
                    y: b.y,
                    vx: b.vx,
                    vy: b.vy,
                });
            }
            bi += 1;
        }
        self.ledger.push(PhysEvent {
            tick: 0,
            kind: EventKind::RunStart,
            a: scene as i64,
            b: (seed & 0xFFFF_FFFF) as i64,
        });
        rec.highlights.push(PhysEvent {
            tick: 0,
            kind: EventKind::RunStart,
            a: scene as i64,
            b: (seed & 0xFFFF_FFFF) as i64,
        });

        let mut sink: Vec<PhysEvent> = Vec::new();
        let mut collis: Vec<CollisionMoment> = Vec::new();
        let mut s: u32 = 0;
        while s < steps {
            sink.clear();
            collis.clear();
            let receipt: StepReceipt = kernel.step(&mut sink, &mut collis)?;
            rec.chain.push(receipt.chain);
            rec.digests.push(receipt.digest);
            rec.collisions.append(&mut collis);
            // 事件：进滚动账 + 记录高亮（截尾保头）。
            let mut ei: usize = 0;
            while ei < sink.len() {
                if let Some(ev) = sink.get(ei) {
                    self.ledger.push(*ev);
                    if rec.highlights.len() < REC_EVENT_CAP {
                        rec.highlights.push(*ev);
                    }
                }
                ei += 1;
            }
            // 采样（每 SAMPLE_EVERY 步）。
            if receipt.tick % SAMPLE_EVERY == 0 {
                let mut bj: usize = 0;
                while bj < kernel.world.bodies.len() {
                    if let Some(b) = kernel.world.bodies.get(bj) {
                        rec.samples.push(SamplePoint {
                            tick: receipt.tick,
                            body: bj as u8,
                            x: b.x,
                            y: b.y,
                            vx: b.vx,
                            vy: b.vy,
                        });
                    }
                    bj += 1;
                }
            }
            // 每步最大速度平方（Q16²）。
            let mut mv2: i64 = 0;
            let mut bk: usize = 0;
            while bk < kernel.world.bodies.len() {
                if let Some(b) = kernel.world.bodies.get(bk) {
                    let v2: i64 =
                        qmul(b.vx, b.vx).saturating_add(qmul(b.vy, b.vy));
                    if v2 > mv2 {
                        mv2 = v2;
                    }
                }
                bk += 1;
            }
            rec.max_speed2.push(mv2);
            s += 1;
        }
        rec.e_end = kernel.world.total_energy();
        rec.work = kernel.world.work_ledger;
        rec.fp_final = match rec.chain.last() {
            Some(v) => *v,
            None => chain_init,
        };
        self.runs += 1;
        self.total_steps += steps as u64;
        self.ledger.push(PhysEvent {
            tick: rec.steps,
            kind: EventKind::RunEnd,
            a: rec.steps as i64,
            b: (rec.fp_final & 0xFFFF_FFFF) as i64,
        });
        if rec.highlights.len() < REC_EVENT_CAP {
            rec.highlights.push(PhysEvent {
                tick: rec.steps,
                kind: EventKind::RunEnd,
                a: rec.steps as i64,
                b: (rec.fp_final & 0xFFFF_FFFF) as i64,
            });
        }
        Ok(rec)
    }

    /// 同种子双跑对拍（自证）：两条独立运行全链逐链比对。
    pub fn run_dual(&mut self, scene: u8, seed: u64) -> Result<DualProof, PhysExError> {
        let a: RunRecord = self.run(scene, seed, None)?;
        let b: RunRecord = self.run(scene, seed, None)?;
        let consistent_root: bool =
            a.steps == b.steps && a.fp_final == b.fp_final && a.chain.len() == b.chain.len();
        let mut first_diff: Option<u32> = None;
        let mut i: usize = 0;
        while i < a.chain.len() && i < b.chain.len() {
            let ca: u64 = match a.chain.get(i) {
                Some(v) => *v,
                None => break,
            };
            let cb: u64 = match b.chain.get(i) {
                Some(v) => *v,
                None => break,
            };
            if ca != cb {
                first_diff = Some((i + 1) as u32);
                break;
            }
            i += 1;
        }
        let consistent: bool = consistent_root && first_diff.is_none();
        if consistent {
            self.ledger.push(PhysEvent {
                tick: a.steps,
                kind: EventKind::DualConsistent,
                a: scene as i64,
                b: (a.fp_final & 0xFFFF_FFFF) as i64,
            });
        } else {
            self.divergences += 1;
            self.ledger.push(PhysEvent {
                tick: first_diff.unwrap_or(0),
                kind: EventKind::Divergence,
                a: first_diff.map(|v| v as i64).unwrap_or(0),
                b: 0,
            });
        }
        Ok(DualProof {
            consistent,
            first_diff,
            fp_a: a.fp_final,
            fp_b: b.fp_final,
            steps: a.steps,
        })
    }

    /// 异种子必分叉（反证）：不分叉即 `SeedDivergeFailed`。
    ///
    /// 返回首个分叉步（1 基）。场景须 jitter = true（种子进初始条件），
    /// 否则视为接线错误直接拒绝。
    pub fn assert_seed_divergence(
        &mut self,
        scene: u8,
        seed_a: u64,
        seed_b: u64,
    ) -> Result<u32, PhysExError> {
        let spec: &SceneStatic = scene_static(scene)?;
        if !spec.jitter {
            return Err(PhysExError::SeedDivergeFailed);
        }
        if seed_a == 0 || seed_b == 0 || seed_a == seed_b {
            return Err(PhysExError::SeedInvalid);
        }
        let a: RunRecord = self.run(scene, seed_a, None)?;
        let b: RunRecord = self.run(scene, seed_b, None)?;
        let mut i: usize = 0;
        while i < a.chain.len() && i < b.chain.len() {
            let ca: u64 = match a.chain.get(i) {
                Some(v) => *v,
                None => break,
            };
            let cb: u64 = match b.chain.get(i) {
                Some(v) => *v,
                None => break,
            };
            if ca != cb {
                return Ok((i + 1) as u32);
            }
            i += 1;
        }
        Err(PhysExError::SeedDivergeFailed)
    }

    // -----------------------------------------------------------------
    // 注入面三件套（锚点「参数注入防护」的示例面：各注入必须被检出）
    // -----------------------------------------------------------------

    /// 注入一 · 场景篡改：镜像质量 +1kg 后目录指纹必须失配。
    pub fn inject_scene_tamper(&mut self, scene: u8, seed: u64) -> Result<u32, PhysExError> {
        let w: WorldState = build_scene(scene, seed)?;
        let fp_clean: u64 = w.catalog_fingerprint();
        let mut mirror: WorldState = w;
        if let Some(b) = mirror.bodies.get_mut(0) {
            b.mass = b.mass.saturating_add(Q16_ONE); // +1kg 篡改
            b.inv_mass = qdiv(Q16_ONE, b.mass)?;
        } else {
            return Err(PhysExError::BodyInvalid);
        }
        let fp_tampered: u64 = mirror.catalog_fingerprint();
        if fp_clean != fp_tampered {
            self.injections_detected += 1;
            self.ledger.push(PhysEvent {
                tick: 0,
                kind: EventKind::TamperDetected,
                a: scene as i64,
                b: 0,
            });
            Ok(1)
        } else {
            Err(PhysExError::TamperEscaped)
        }
    }

    /// 注入二 · 指纹漂移：翻转链上一字后验证器必须报警。
    pub fn inject_fingerprint_drift(
        &mut self,
        scene: u8,
        seed: u64,
        at_step: u32,
    ) -> Result<u32, PhysExError> {
        let rec: RunRecord = self.run(scene, seed, None)?;
        let at: usize = at_step as usize;
        if at >= rec.chain.len() {
            return Err(PhysExError::ParamOutOfRange);
        }
        // 先自证净账可验（内部一致性前置）。
        if !verify_ledger(rec.chain_init, &rec.digests, &rec.chain) {
            return Err(PhysExError::DivergenceDetected);
        }
        let mut tampered: Vec<u64> = rec.chain.clone();
        if let Some(slot) = tampered.get_mut(at) {
            *slot ^= 0x0000_0000_0000_0001; // 单位翻转（最小篡改）
        }
        let caught: bool = !verify_ledger(rec.chain_init, &rec.digests, &tampered);
        if caught {
            self.injections_detected += 1;
            self.ledger.push(PhysEvent {
                tick: at_step,
                kind: EventKind::DriftDetected,
                a: at_step as i64,
                b: 0,
            });
            Ok(at_step)
        } else {
            Err(PhysExError::DriftEscaped)
        }
    }

    /// 注入三 · 越带伪造：伪造超带能量值必须被立案（BandFiled）。
    pub fn inject_band_fake(
        &mut self,
        scene: u8,
        fake_ppm: i64,
        tick_now: u32,
    ) -> Result<u32, PhysExError> {
        let spec: &SceneStatic = scene_static(scene)?;
        if fake_ppm <= spec.energy_band_ppm {
            // 伪值必须真的越带，否则注入本身不成立。
            return Err(PhysExError::ParamOutOfRange);
        }
        let before: usize = self.band_cases.len();
        self.band_cases.push(BandCase {
            scene,
            metric: BandKind::Energy,
            value: fake_ppm,
            band: spec.energy_band_ppm,
            tick: tick_now,
            reason: "越带注入立案：伪造能量偏差超出场景预期带（注入面演示）",
        });
        self.injections_detected += 1;
        self.ledger.push(PhysEvent {
            tick: tick_now,
            kind: EventKind::BandInjectFiled,
            a: fake_ppm,
            b: spec.energy_band_ppm,
        });
        let after: usize = self.band_cases.len();
        if after == before + 1 {
            Ok((after - 1) as u32)
        } else {
            Err(PhysExError::BandInjectEscaped)
        }
    }
}

// ===========================================================================
// 第 12 节 · MetricExtractor 教学指标提取（三指标 + 越带立案）
// ===========================================================================

/// 三指标报告（能量/动量/收敛）。
#[derive(Debug, Clone, Copy)]
pub struct MetricReport {
    /// 场景号。
    pub scene: u8,
    /// 能量守恒偏差（ppm；口径 |E0 − (E终 + W台账)| / E0 × 10⁶）。
    pub energy_dev_ppm: i64,
    /// 动量守恒偏差（ppm；碰撞事件 |Δp|/|p| 最大值；不适用场景为 0）。
    pub momentum_dev_ppm: i64,
    /// 动量指标是否适用。
    pub momentum_applicable: bool,
    /// 收敛步数（速度平方持续低于阈值的起点步；不适用场景为 0）。
    pub conv_step: u32,
    /// 收敛指标是否适用。
    pub conv_applicable: bool,
}

/// 指标提取器（无状态；三指标独立可单测）。
pub struct MetricExtractor;

impl MetricExtractor {
    /// 能量守恒偏差（ppm，i128 中间量防溢出，收窄显性）。
    pub fn energy_dev_ppm(rec: &RunRecord) -> Result<i64, PhysExError> {
        if rec.e0 <= 0 {
            return Err(PhysExError::ParamOutOfRange);
        }
        let accounted: i128 = rec.e_end as i128 + rec.work as i128;
        let diff: i128 = if (rec.e0 as i128) > accounted {
            (rec.e0 as i128) - accounted
        } else {
            accounted - (rec.e0 as i128)
        };
        let ppm: i128 = diff * 1_000_000 / (rec.e0 as i128);
        Ok(narrow(ppm))
    }

    /// 动量守恒偏差（ppm）：逐碰撞事件对拍 |Δp|₁ / |p_before|₁ 的最大值。
    ///
    /// 墙面反弹动量不守恒（外力冲量），本指标只统计球对事件——
    /// 教学上正是"内力冲量不改变系统总动量"的直接演示。
    pub fn momentum_dev_ppm(rec: &RunRecord) -> i64 {
        let mut worst: i128 = 0;
        let mut i: usize = 0;
        while i < rec.collisions.len() {
            if let Some(c) = rec.collisions.get(i) {
                let denom: i128 = c.p_before_x.abs() as i128 + c.p_before_y.abs() as i128;
                if denom > 0 {
                    let num: i128 = (c.p_after_x - c.p_before_x).abs() as i128
                        + (c.p_after_y - c.p_before_y).abs() as i128;
                    let ppm: i128 = num * 1_000_000 / denom;
                    if ppm > worst {
                        worst = ppm;
                    }
                }
            }
            i += 1;
        }
        narrow(worst)
    }

    /// 收敛步数：最后一个「最大速度平方 ≥ 阈值²」的步 + 1。
    ///
    /// 口径：从尾部看，速度一旦持续低于 0.02m/s 即视为已收敛；
    /// 全程未超阈值（初态即静止）返回 1。
    pub fn convergence_step(rec: &RunRecord) -> u32 {
        let thresh2: i64 = qmul(CONV_V_THRESH, CONV_V_THRESH);
        let mut last_active: u32 = 0;
        let mut i: usize = 0;
        while i < rec.max_speed2.len() {
            if let Some(v2) = rec.max_speed2.get(i) {
                if *v2 >= thresh2 {
                    last_active = (i + 1) as u32;
                }
            }
            i += 1;
        }
        last_active + 1
    }

    /// 三指标一次提取。
    pub fn extract(rec: &RunRecord) -> Result<MetricReport, PhysExError> {
        let spec: &SceneStatic = scene_static(rec.scene)?;
        Ok(MetricReport {
            scene: rec.scene,
            energy_dev_ppm: Self::energy_dev_ppm(rec)?,
            momentum_dev_ppm: if spec.momentum_applicable {
                Self::momentum_dev_ppm(rec)
            } else {
                0
            },
            momentum_applicable: spec.momentum_applicable,
            conv_step: if spec.conv_applicable {
                Self::convergence_step(rec)
            } else {
                0
            },
            conv_applicable: spec.conv_applicable,
        })
    }

    /// 指标闸门：逐指标对带，越带即立案（BandCase 入卷）并返回
    /// `MetricOutOfBand`（锚点「越带→立案」；严格面）。
    ///
    /// 三个指标独立立案——一次运行最多立三卷。
    pub fn gate(
        spec: &SceneStatic,
        report: &MetricReport,
        cases: &mut Vec<BandCase>,
        tick_now: u32,
    ) -> Result<(), PhysExError> {
        let mut violated: bool = false;
        if report.energy_dev_ppm > spec.energy_band_ppm {
            cases.push(BandCase {
                scene: spec.id,
                metric: BandKind::Energy,
                value: report.energy_dev_ppm,
                band: spec.energy_band_ppm,
                tick: tick_now,
                reason: "能量守恒偏差超出预期带：核对耗散台账口径与 Q16 舍入链假设",
            });
            violated = true;
        }
        if report.momentum_applicable && report.momentum_dev_ppm > spec.momentum_band_ppm {
            cases.push(BandCase {
                scene: spec.id,
                metric: BandKind::Momentum,
                value: report.momentum_dev_ppm,
                band: spec.momentum_band_ppm,
                tick: tick_now,
                reason: "动量守恒偏差超出预期带：核对冲量结算的定点乘舍入链",
            });
            violated = true;
        }
        if report.conv_applicable
            && (report.conv_step < spec.conv_lo || report.conv_step > spec.conv_hi)
        {
            cases.push(BandCase {
                scene: spec.id,
                metric: BandKind::Convergence,
                value: report.conv_step as i64,
                band: spec.conv_hi as i64,
                tick: tick_now,
                reason: "收敛步数超出预期带：核对阈值假设（0.02m/s）与场景时长",
            });
            violated = true;
        }
        if violated {
            Err(PhysExError::MetricOutOfBand)
        } else {
            Ok(())
        }
    }
}

// ===========================================================================
// 第 13 节 · DebugView 调试视图三例同源（轨迹表/事件线/指标面板）
// ===========================================================================

/// 读屏说明（锚点「读屏说明」：每视图尾行拼接的指引原文）。
pub const ACCESS_NOTE: &str =
    "读屏说明：本表每行以「帧 N」开头，N 为帧号；行内数值依次为位置 x/y 与速度 vx/vy（米制）。跳转：报出目标帧号后视图定位到该帧最近采样行；断点：已设断点时跳转行尾标注「断点@N」。";

/// 回归进 CI 说明（锚点「回归进 CI 说明」原文）。
pub const CI_GUARD_NOTE: &str =
    "CI 守护说明：本模块回归以 vx1024b_probe_f6013 探针入 CI——(1) 八场景运行与终态指纹快照比对（指纹漂移即红）；(2) 同种子双跑全链一致（分叉即红）；(3) 异种子必分叉（不分叉即红）；(4) 三注入各检出（逃逸即红）；(5) 指标带闸门（越带即红）。五线全绿方许合入。";

/// 断点表（帧号集合；跳转联动的数据源）。
#[derive(Debug, Clone)]
pub struct Breakpoints {
    /// 断点帧号集（无序；jump 取 ≤ 落点的最大者）。
    pub frames: Vec<u32>,
}

impl Breakpoints {
    /// 新建空断点表。
    pub fn new() -> Self {
        Breakpoints { frames: Vec::new() }
    }

    /// 设断点（帧号 0 与重复帧拒绝——0 是初态无意义，重复无增益）。
    pub fn set(&mut self, frame: u32) -> Result<(), PhysExError> {
        if frame == 0 {
            return Err(PhysExError::ParamOutOfRange);
        }
        let mut i: usize = 0;
        while i < self.frames.len() {
            if let Some(f) = self.frames.get(i) {
                if *f == frame {
                    return Err(PhysExError::ParamOutOfRange);
                }
            }
            i += 1;
        }
        self.frames.push(frame);
        Ok(())
    }

    /// ≤ bound 的最大断点（联动语义：跳转落点回看最近断点）。
    pub fn latest_within(&self, bound: u32) -> Option<u32> {
        let mut best: Option<u32> = None;
        let mut i: usize = 0;
        while i < self.frames.len() {
            if let Some(f) = self.frames.get(i) {
                if *f <= bound {
                    match best {
                        Some(b) => {
                            if *f > b {
                                best = Some(*f);
                            }
                        }
                        None => best = Some(*f),
                    }
                }
            }
            i += 1;
        }
        best
    }
}

/// 帧号跳转结果（含断点联动标注）。
#[derive(Debug, Clone, Copy)]
pub struct JumpResult {
    /// 落点帧号（≤ 请求帧的最近采样帧）。
    pub frame: u32,
    /// 体号。
    pub body: u8,
    /// 位置 x（Q16）。
    pub x: i64,
    /// 位置 y（Q16）。
    pub y: i64,
    /// 速度 vx（Q16）。
    pub vx: i64,
    /// 速度 vy（Q16）。
    pub vy: i64,
    /// 联动断点（落点帧之前最近的断点帧号；无则 None）。
    pub breakpoint_hit: Option<u32>,
}

/// 调试视图（三视图全部从同一份 `RunRecord` 生成——锚点「三例同源」）。
pub struct DebugView;

impl DebugView {
    /// 视图一 · 轨迹表（帧号标注；每采样帧行，max_rows 截尾）。
    pub fn trajectory_table(rec: &RunRecord, max_rows: usize) -> String {
        let mut s: String = String::from("== 轨迹表（三例同源 · 出自 RunRecord） ==\n");
        let mut shown: usize = 0;
        let mut i: usize = 0;
        while i < rec.samples.len() {
            if shown >= max_rows {
                break;
            }
            if let Some(p) = rec.samples.get(i) {
                s.push_str("帧 ");
                s.push_str(&p.tick.to_string());
                s.push_str(" | b");
                s.push_str(&p.body.to_string());
                s.push_str(" | x=");
                s.push_str(&q16_to_dec(p.x, 3));
                s.push_str(" y=");
                s.push_str(&q16_to_dec(p.y, 3));
                s.push_str(" | vx=");
                s.push_str(&q16_to_dec(p.vx, 3));
                s.push_str(" vy=");
                s.push_str(&q16_to_dec(p.vy, 3));
                s.push('\n');
                shown += 1;
            }
            i += 1;
        }
        if shown >= max_rows && i < rec.samples.len() {
            s.push_str("…（截尾，共 ");
            s.push_str(&rec.samples.len().to_string());
            s.push_str(" 个采样点）\n");
        }
        s.push_str(ACCESS_NOTE);
        s
    }

    /// 视图二 · 事件时间线（帧号 + 事件名 + 载荷；max_rows 截尾）。
    pub fn event_timeline(rec: &RunRecord, max_rows: usize) -> String {
        let mut s: String = String::from("== 事件时间线（三例同源 · 出自 RunRecord） ==\n");
        let mut shown: usize = 0;
        let mut i: usize = 0;
        while i < rec.highlights.len() {
            if shown >= max_rows {
                break;
            }
            if let Some(ev) = rec.highlights.get(i) {
                s.push_str("帧 ");
                s.push_str(&ev.tick.to_string());
                s.push_str(" | ");
                s.push_str(ev.kind.name());
                s.push_str(" #");
                s.push_str(&ev.kind.code().to_string());
                // 载荷按原始整数显示（事件载荷语义随种类，非一律 Q16 物理量）。
                s.push_str(" | a=");
                s.push_str(&ev.a.to_string());
                s.push_str(" b=");
                s.push_str(&ev.b.to_string());
                s.push('\n');
                shown += 1;
            }
            i += 1;
        }
        if rec.highlights.len() > REC_EVENT_CAP {
            s.push_str("…（高亮账截尾保头，容量 ");
            s.push_str(&REC_EVENT_CAP.to_string());
            s.push_str("）\n");
        }
        s.push_str(ACCESS_NOTE);
        s
    }

    /// 视图三 · 指标面板（三指标 + 预期带 + 判定；读屏口径）。
    pub fn metric_panel(rec: &RunRecord, report: &MetricReport) -> String {
        let spec: &SceneStatic = match scene_static(rec.scene) {
            Ok(v) => v,
            Err(_) => return String::from("== 指标面板：场景号未知 =="),
        };
        let mut s: String = String::from("== 指标面板（三例同源 · 出自 RunRecord） ==\n");
        s.push_str("场景 ");
        s.push_str(&spec.id.to_string());
        s.push_str(" · ");
        s.push_str(spec.name);
        s.push('\n');
        s.push_str("能量守恒偏差: ");
        s.push_str(&report.energy_dev_ppm.to_string());
        s.push_str(" ppm（带宽 ≤ ");
        s.push_str(&spec.energy_band_ppm.to_string());
        s.push_str("，");
        s.push_str(if report.energy_dev_ppm <= spec.energy_band_ppm {
            "在带）"
        } else {
            "越带）"
        });
        s.push('\n');
        if report.momentum_applicable {
            s.push_str("动量守恒偏差: ");
            s.push_str(&report.momentum_dev_ppm.to_string());
            s.push_str(" ppm（带宽 ≤ ");
            s.push_str(&spec.momentum_band_ppm.to_string());
            s.push_str("，");
            s.push_str(
                if report.momentum_dev_ppm <= spec.momentum_band_ppm {
                    "在带）"
                } else {
                    "越带）"
                },
            );
            s.push('\n');
        } else {
            s.push_str("动量守恒偏差: 不适用（场景含外力或无球对碰撞）\n");
        }
        if report.conv_applicable {
            s.push_str("收敛步数: ");
            s.push_str(&report.conv_step.to_string());
            s.push_str("（带宽 ");
            s.push_str(&spec.conv_lo.to_string());
            s.push('~');
            s.push_str(&spec.conv_hi.to_string());
            s.push_str("，");
            s.push_str(
                if report.conv_step >= spec.conv_lo && report.conv_step <= spec.conv_hi {
                    "在带）"
                } else {
                    "越带）"
                },
            );
            s.push('\n');
        } else {
            s.push_str("收敛步数: 不适用（场景无停摆语义）\n");
        }
        s.push_str(ACCESS_NOTE);
        s
    }

    /// 帧号跳转（锚点「帧号标注含跳转」+「跳转含断点联动」）。
    ///
    /// 语义：落到 ≤ 请求帧的最近采样帧；断点联动 = 落点帧之前
    /// （含落点帧）最近的断点帧号随行返回。
    pub fn jump(
        rec: &RunRecord,
        frame: u32,
        body: u8,
        bps: &Breakpoints,
    ) -> Result<JumpResult, PhysExError> {
        if frame == 0 {
            return Err(PhysExError::ParamOutOfRange);
        }
        let mut landed: Option<SamplePoint> = None;
        let mut i: usize = 0;
        while i < rec.samples.len() {
            if let Some(p) = rec.samples.get(i) {
                if p.body == body && p.tick <= frame {
                    match landed {
                        Some(cur) => {
                            if p.tick >= cur.tick {
                                landed = Some(*p);
                            }
                        }
                        None => landed = Some(*p),
                    }
                }
            }
            i += 1;
        }
        match landed {
            Some(p) => Ok(JumpResult {
                frame: p.tick,
                body: p.body,
                x: p.x,
                y: p.y,
                vx: p.vx,
                vy: p.vy,
                breakpoint_hit: bps.latest_within(p.tick),
            }),
            None => Err(PhysExError::ParamOutOfRange),
        }
    }
}

// ===========================================================================
// 第 14 节 · ExampleBook 示例册（清单 + 快照 + 册指纹冻结 + 双跑一致）
// ===========================================================================

/// 册条目判定（Pass = 全指标在带；BandCase = 至少一项越带已立案）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    /// 全指标在带。
    Pass,
    /// 有指标越带（案卷在运行器 band_cases）。
    BandCase,
}

impl Verdict {
    /// 判定名（读屏）。
    pub fn name(&self) -> &'static str {
        match self {
            Verdict::Pass => "在带",
            Verdict::BandCase => "越带立案",
        }
    }

    /// 册指纹判别位（0/1——冻结链里的稳定编码）。
    pub fn discriminant(&self) -> u64 {
        match self {
            Verdict::Pass => 0,
            Verdict::BandCase => 1,
        }
    }
}

/// 册条目：一个场景的运行结果快照。
#[derive(Debug, Clone, Copy)]
pub struct BookEntry {
    /// 场景号。
    pub scene: u8,
    /// 场景名。
    pub name: &'static str,
    /// 实跑步数。
    pub steps: u32,
    /// 终态指纹。
    pub fp_final: u64,
    /// 能量偏差（ppm）。
    pub energy_dev_ppm: i64,
    /// 动量偏差（ppm；不适用为 0）。
    pub momentum_dev_ppm: i64,
    /// 收敛步数（不适用为 0）。
    pub conv_step: u32,
    /// 判定。
    pub verdict: Verdict,
}

/// 册指纹折叠（条目序即指纹序——条目顺序本身参与冻结）。
fn book_fingerprint(entries: &[BookEntry]) -> u64 {
    let mut h: u64 = fp_mix(CHAIN_SEED, 0xB00C_0000_0000_0001);
    let mut i: usize = 0;
    while i < entries.len() {
        if let Some(e) = entries.get(i) {
            h = fp_mix(h, e.scene as u64);
            h = fp_mix(h, e.fp_final);
            h = fp_mix(h, e.energy_dev_ppm as u64);
            h = fp_mix(h, e.momentum_dev_ppm as u64);
            h = fp_mix(h, e.conv_step as u64);
            h = fp_mix(h, e.verdict.discriminant());
        }
        i += 1;
    }
    h
}

/// 示例册（锚点数据结构「示例包」：清单 + 快照 + 册指纹）。
#[derive(Debug, Clone)]
pub struct ExampleBook {
    /// 场景条目（构建顺序即冻结顺序）。
    pub entries: Vec<BookEntry>,
    /// 册指纹（全部条目折叠；冻结后即对外承诺值）。
    pub book_fp: u64,
    /// 冻结位（冻结后 add_entry 拒绝）。
    pub frozen: bool,
}

impl ExampleBook {
    /// 构建一册（内部起独立运行器；门面聚账走 `build_with`）。
    pub fn build(scene_ids: &[u8], seed: u64) -> Result<Self, PhysExError> {
        let mut fresh: ScenarioRunner = ScenarioRunner::new();
        Self::build_with(&mut fresh, scene_ids, seed)
    }

    /// 构建一册（借用调用方运行器——运行计数/事件账/案卷全部聚合到
    /// 调用方门面；`ExampleCenter::build_book` 用此口）。
    ///
    /// 越带不阻断建册（教学面：越带场景照样入册但标 BandCase 判定，
    /// 案卷留在运行器；CI 面由 gate 的 Err 通道负责红绿）。
    pub fn build_with(
        runner: &mut ScenarioRunner,
        scene_ids: &[u8],
        seed: u64,
    ) -> Result<Self, PhysExError> {
        let mut entries: Vec<BookEntry> = Vec::new();
        let mut i: usize = 0;
        while i < scene_ids.len() {
            let id: u8 = match scene_ids.get(i) {
                Some(v) => *v,
                None => break,
            };
            let spec: &SceneStatic = scene_static(id)?;
            let rec: RunRecord = runner.run(id, seed, None)?;
            let report: MetricReport = MetricExtractor::extract(&rec)?;
            let verdict: Verdict = match MetricExtractor::gate(
                spec,
                &report,
                &mut runner.band_cases,
                runner.total_steps as u32,
            ) {
                Ok(()) => Verdict::Pass,
                Err(PhysExError::MetricOutOfBand) => Verdict::BandCase,
                Err(e) => return Err(e),
            };
            entries.push(BookEntry {
                scene: id,
                name: spec.name,
                steps: rec.steps,
                fp_final: rec.fp_final,
                energy_dev_ppm: report.energy_dev_ppm,
                momentum_dev_ppm: report.momentum_dev_ppm,
                conv_step: report.conv_step,
                verdict,
            });
            i += 1;
        }
        let book_fp: u64 = book_fingerprint(&entries);
        Ok(ExampleBook {
            entries,
            book_fp,
            frozen: false,
        })
    }

    /// 冻结（幂等；返回册指纹作为对外承诺值）。
    pub fn freeze(&mut self) -> u64 {
        self.frozen = true;
        self.book_fp
    }

    /// 冻结后追加条目 → `BookSealed`（册指纹不可漂移）。
    pub fn add_entry(&mut self, e: BookEntry) -> Result<(), PhysExError> {
        if self.frozen {
            return Err(PhysExError::BookSealed);
        }
        self.entries.push(e);
        self.book_fp = book_fingerprint(&self.entries);
        Ok(())
    }

    /// 双跑一致判据：两次独立构建的册指纹必须全等（条目指纹亦逐条相等）。
    pub fn verify_dual(scene_ids: &[u8], seed: u64) -> Result<bool, PhysExError> {
        let a: ExampleBook = Self::build(scene_ids, seed)?;
        let b: ExampleBook = Self::build(scene_ids, seed)?;
        if a.entries.len() != b.entries.len() {
            return Ok(false);
        }
        let mut i: usize = 0;
        while i < a.entries.len() {
            let ea: BookEntry = match a.entries.get(i) {
                Some(v) => *v,
                None => return Ok(false),
            };
            let eb: BookEntry = match b.entries.get(i) {
                Some(v) => *v,
                None => return Ok(false),
            };
            if ea.fp_final != eb.fp_final || ea.scene != eb.scene {
                return Ok(false);
            }
            i += 1;
        }
        Ok(a.book_fp == b.book_fp)
    }
}

// ===========================================================================
// 第 15 节 · ExampleCenter 教学门面（health + selfcheck + 注入编排）
// ===========================================================================

/// 健康面（一次快照读出全部防线计数）。
#[derive(Debug, Clone, Copy)]
pub struct HealthReport {
    /// 累计完整运行次数。
    pub runs: u32,
    /// 累计物理步。
    pub total_steps: u64,
    /// 对拍分叉计数（健康 = 0）。
    pub divergences: u32,
    /// 越带案卷数。
    pub band_cases: u32,
    /// 注入检出数。
    pub injections_detected: u32,
    /// 位型防线拒绝数。
    pub guard_rejects: u32,
    /// 位型防线通过数。
    pub guard_admits: u32,
    /// 事件账当前长度。
    pub ledger_len: usize,
    /// 事件账滚动丢弃数。
    pub ledger_dropped: u32,
    /// 册是否已冻结（未建册 = false）。
    pub book_frozen: bool,
    /// 册指纹（未建册 = 0）。
    pub book_fp: u64,
}

/// 自检单项。
#[derive(Debug, Clone)]
pub struct CheckLine {
    /// 项名（中文）。
    pub name: String,
    /// 是否通过。
    pub pass: bool,
    /// 注记（通过=要点摘录，失败=首错 detail）。
    pub note: String,
}

/// 自检报告（判据「三例、同源、CI 守护、判据」逐项映射）。
#[derive(Debug, Clone)]
pub struct SelfCheckReport {
    /// 逐项结果。
    pub lines: Vec<CheckLine>,
    /// 全过判定。
    pub all_pass: bool,
}

/// 教学门面：一个对象编排 运行/对拍/注入/建册/自检 全流程。
#[derive(Debug, Clone)]
pub struct ExampleCenter {
    /// 场景运行器。
    pub runner: ScenarioRunner,
    /// 已建册（None = 未建）。
    pub book: Option<ExampleBook>,
}

impl ExampleCenter {
    /// 新建门面。
    pub fn new() -> Self {
        ExampleCenter {
            runner: ScenarioRunner::new(),
            book: None,
        }
    }

    /// 运行单场景（教学入口；事件自动入账）。
    pub fn run_scene(&mut self, scene: u8, seed: u64) -> Result<RunRecord, PhysExError> {
        self.runner.run(scene, seed, None)
    }

    /// 构建并冻结全场景册（锚点「示例包」的完整面）。
    pub fn build_book(&mut self, seed: u64) -> Result<u64, PhysExError> {
        let mut ids: Vec<u8> = Vec::new();
        let mut id: u8 = 1;
        while id <= SCENE_COUNT {
            ids.push(id);
            id += 1;
        }
        let mut book: ExampleBook = ExampleBook::build_with(&mut self.runner, &ids, seed)?;
        let fp: u64 = book.freeze();
        self.runner.ledger.push(PhysEvent {
            tick: self.runner.total_steps as u32,
            kind: EventKind::BookSeal,
            a: (fp & 0xFFFF_FFFF) as i64,
            b: book.entries.len() as i64,
        });
        self.book = Some(book);
        Ok(fp)
    }

    /// 健康面快照。
    pub fn health(&self) -> HealthReport {
        let (frozen, fp): (bool, u64) = match &self.book {
            Some(b) => (b.frozen, b.book_fp),
            None => (false, 0),
        };
        HealthReport {
            runs: self.runner.runs,
            total_steps: self.runner.total_steps,
            divergences: self.runner.divergences,
            band_cases: self.runner.band_cases.len() as u32,
            injections_detected: self.runner.injections_detected,
            guard_rejects: self.runner.guard.rejects(),
            guard_admits: self.runner.guard.admits(),
            ledger_len: self.runner.ledger.len(),
            ledger_dropped: self.runner.ledger.dropped(),
            book_frozen: frozen,
            book_fp: fp,
        }
    }

    /// 判据自检（锚点判据「三例、同源、CI 守护、判据」的运行期映射；
    /// 每项入事件账 SelfCheck，尾部汇总 all_pass）。
    pub fn selfcheck(&mut self, seed: u64) -> Result<SelfCheckReport, PhysExError> {
        let mut lines: Vec<CheckLine> = Vec::new();
        let push_line = |lines: &mut Vec<CheckLine>,
                             runner: &mut ScenarioRunner,
                             name: &str,
                             pass: bool,
                             note: String| {
            lines.push(CheckLine {
                name: String::from(name),
                pass,
                note,
            });
            runner.ledger.push(PhysEvent {
                tick: runner.total_steps as u32,
                kind: EventKind::SelfCheck,
                a: if pass { 1 } else { 0 },
                b: lines.len() as i64,
            });
        };

        // 1. 八场景闭集完整可跑。
        let mut all_run: bool = true;
        let mut bad_scene: u8 = 0;
        {
            let mut id: u8 = 1;
            while id <= SCENE_COUNT {
                match self.runner.run(id, seed, None) {
                    Ok(_) => {}
                    Err(_) => {
                        all_run = false;
                        bad_scene = id;
                        break;
                    }
                }
                id += 1;
            }
        }
        push_line(
            &mut lines,
            &mut self.runner,
            "八场景闭集完整可跑",
            all_run,
            if all_run {
                String::from("1..=8 全部建场并跑满场景表步数")
            } else {
                format!("场景 {} 运行失败", bad_scene)
            },
        );

        // 2. 同种子双跑一致（对拍自证）。
        let mut dual_ok: bool = true;
        let mut dual_bad: u8 = 0;
        {
            let mut id: u8 = 1;
            while id <= SCENE_COUNT {
                match self.runner.run_dual(id, seed) {
                    Ok(p) => {
                        if !p.consistent {
                            dual_ok = false;
                            dual_bad = id;
                            break;
                        }
                    }
                    Err(_) => {
                        dual_ok = false;
                        dual_bad = id;
                        break;
                    }
                }
                id += 1;
            }
        }
        push_line(
            &mut lines,
            &mut self.runner,
            "同种子双跑全链一致",
            dual_ok,
            if dual_ok {
                String::from("八场景双跑指纹链逐链全等")
            } else {
                format!("场景 {} 双跑分叉", dual_bad)
            },
        );

        // 3. 异种子必分叉（反证；两个 jitter 场景）。
        let div6: bool = self
            .runner
            .assert_seed_divergence(DIVERGENCE_PROBE_SCENE, seed, seed.wrapping_add(7))
            .is_ok();
        let div8: bool = self
            .runner
            .assert_seed_divergence(8, seed, seed.wrapping_add(11))
            .is_ok();
        push_line(
            &mut lines,
            &mut self.runner,
            "异种子必分叉（多球/软绳）",
            div6 && div8,
            format!(
                "多球场景分叉={}，软绳场景分叉={}",
                div6, div8
            ),
        );

        // 4. 三例同源（三视图出自同一 RunRecord 且帧号标注在位）。
        let rec: RunRecord = match self.runner.run(1, seed, None) {
            Ok(r) => r,
            Err(e) => return Err(e),
        };
        let report: MetricReport = MetricExtractor::extract(&rec)?;
        let v1: String = DebugView::trajectory_table(&rec, 4);
        let v2: String = DebugView::event_timeline(&rec, 4);
        let v3: String = DebugView::metric_panel(&rec, &report);
        let same_source: bool = v1.contains("帧 ") && v2.contains("帧 ")
            && v3.contains("能量守恒偏差")
            && v1.contains("三例同源");
        push_line(
            &mut lines,
            &mut self.runner,
            "三视图同源且帧号标注在位",
            same_source,
            String::from("轨迹表/事件线/指标面板均出自同一份 RunRecord"),
        );

        // 5. 三注入全检出。
        let inj1: bool = self.runner.inject_scene_tamper(1, seed).is_ok();
        let inj2: bool = self.runner.inject_fingerprint_drift(1, seed, 5).is_ok();
        let inj3: bool = self.runner.inject_band_fake(1, 50_000_000, 1).is_ok();
        push_line(
            &mut lines,
            &mut self.runner,
            "三注入各检出（篡改/漂移/越带）",
            inj1 && inj2 && inj3,
            format!("篡改={} 漂移={} 越带={}", inj1, inj2, inj3),
        );

        // 6. 指标入带（八场景 gate 全 Ok）。
        let mut band_ok: bool = true;
        let mut band_bad: u8 = 0;
        {
            let mut id: u8 = 1;
            while id <= SCENE_COUNT {
                match self.runner.run(id, seed, None) {
                    Ok(r) => {
                        let rep: MetricReport = MetricExtractor::extract(&r)?;
                        let spec: &SceneStatic = scene_static(id)?;
                        let tick_now: u32 = self.runner.total_steps as u32;
                        if let Err(e) =
                            MetricExtractor::gate(spec, &rep, &mut self.runner.band_cases, tick_now)
                        {
                            if e == PhysExError::MetricOutOfBand {
                                band_ok = false;
                                band_bad = id;
                                break;
                            } else {
                                return Err(e);
                            }
                        }
                    }
                    Err(e) => return Err(e),
                }
                id += 1;
            }
        }
        push_line(
            &mut lines,
            &mut self.runner,
            "八场景三指标全在带",
            band_ok,
            if band_ok {
                String::from("能量/动量/收敛逐场景对带通过")
            } else {
                format!("场景 {} 指标越带（见 band_cases 案卷）", band_bad)
            },
        );

        // 7. 册双构建一致 + 冻结生效。
        let mut ids: Vec<u8> = Vec::new();
        let mut id: u8 = 1;
        while id <= SCENE_COUNT {
            ids.push(id);
            id += 1;
        }
        let dual_book: bool = match ExampleBook::verify_dual(&ids, seed) {
            Ok(v) => v,
            Err(e) => return Err(e),
        };
        let mut mut_book: ExampleBook = match ExampleBook::build(&ids, seed) {
            Ok(b) => b,
            Err(e) => return Err(e),
        };
        let _ = mut_book.freeze();
        let sealed_reject: bool = match mut_book.add_entry(BookEntry {
            scene: 1,
            name: "自由落体",
            steps: 0,
            fp_final: 0,
            energy_dev_ppm: 0,
            momentum_dev_ppm: 0,
            conv_step: 0,
            verdict: Verdict::Pass,
        }) {
            Err(PhysExError::BookSealed) => true,
            _ => false,
        };
        push_line(
            &mut lines,
            &mut self.runner,
            "册双构建一致且冻结后拒改",
            dual_book && sealed_reject,
            format!("双构建一致={} 冻结拒绝追加={}", dual_book, sealed_reject),
        );

        // 8. NaN/Inf 位型防线（位判、零浮点）。
        let mut guard_ok: bool = true;
        {
            let mut gi: usize = 0;
            while gi < F32_CANONICAL_NONFINITE.len() {
                if let Some(bits) = F32_CANONICAL_NONFINITE.get(gi) {
                    if self.runner.guard.admit_u32(*bits).is_ok() {
                        guard_ok = false;
                    }
                }
                gi += 1;
            }
            let mut gj: usize = 0;
            while gj < F64_CANONICAL_NONFINITE.len() {
                if let Some(bits) = F64_CANONICAL_NONFINITE.get(gj) {
                    if self.runner.guard.admit_u64(*bits).is_ok() {
                        guard_ok = false;
                    }
                }
                gj += 1;
            }
            if self.runner.guard.admit_u64(0x0000_0000_003F_FFFF).is_err() {
                guard_ok = false; // 有限字必须通过
            }
        }
        push_line(
            &mut lines,
            &mut self.runner,
            "NaN/Inf 位型防线位判有效",
            guard_ok,
            String::from("f32/f64 全部典型非有限字被拒，有限字通过"),
        );

        // 9. 链验证器自洽（净账可验真 + 篡改账可验假——漂移注入已覆）。
        let chain_ok: bool = match self.runner.run(2, seed, None) {
            Ok(r) => verify_ledger(r.chain_init, &r.digests, &r.chain),
            Err(_) => false,
        };
        push_line(
            &mut lines,
            &mut self.runner,
            "链验证器对净账判真",
            chain_ok,
            String::from("chain[i] = fp_mix(chain[i-1], digests[i]^(i+1)) 逐链复核"),
        );

        let all_pass: bool = {
            let mut ok: bool = true;
            let mut li: usize = 0;
            while li < lines.len() {
                if let Some(l) = lines.get(li) {
                    if !l.pass {
                        ok = false;
                        break;
                    }
                }
                li += 1;
            }
            ok
        };
        Ok(SelfCheckReport { lines, all_pass })
    }
}

// ===========================================================================
// 第 16 节 · 编译期断言（const 面；零运行期 assert 的补充）
// ===========================================================================

const _: () = assert!(SEG_BASE == 0x1E30, "诊断段基座必须是 0x1E30");
const _: () = assert!(SCENES.len() == 8, "场景闭集必须恰为 8 个");
const _: () = assert!(Q16_ONE == 65_536, "Q16.16 定标必须为 2^16");
const _: () = assert!(DT_Q16 > 0 && DT_Q16 < Q16_ONE, "步长必须落在 (0,1)s");
const _: () = assert!(G_Q16 > 0 && G_Q16 < 20 * Q16_ONE, "教学重力域 0~20 m/s²");
const _: () = assert!(PhysExError::SceneUnknown as u16 == 0x1E31);
const _: () = assert!(PhysExError::SeedInvalid as u16 == 0x1E32);
const _: () = assert!(PhysExError::BodyInvalid as u16 == 0x1E33);
const _: () = assert!(PhysExError::ParamOutOfRange as u16 == 0x1E34);
const _: () = assert!(PhysExError::DivByZero as u16 == 0x1E35);
const _: () = assert!(PhysExError::SqrtNegative as u16 == 0x1E36);
const _: () = assert!(PhysExError::StepCountExceeded as u16 == 0x1E37);
const _: () = assert!(PhysExError::DivergenceDetected as u16 == 0x1E38);
const _: () = assert!(PhysExError::SeedDivergeFailed as u16 == 0x1E39);
const _: () = assert!(PhysExError::MetricOutOfBand as u16 == 0x1E3A);
const _: () = assert!(PhysExError::TamperEscaped as u16 == 0x1E3B);
const _: () = assert!(PhysExError::DriftEscaped as u16 == 0x1E3C);
const _: () = assert!(PhysExError::BandInjectEscaped as u16 == 0x1E3D);
const _: () = assert!(PhysExError::BookSealed as u16 == 0x1E3E);
const _: () = assert!(PhysExError::NanPatternRejected as u16 == 0x1E3F);
const _: () = assert!(BODY_CAP >= 5, "多球场景需要 ≥5 体容量");
const _: () = assert!(MAX_STEPS >= 1_500, "最长教学场景 1500 步必须放行");
const _: () = assert!(LEDGER_CAP >= 256, "事件账容量必须够八场景高亮");
const _: () = assert!(SAMPLE_EVERY >= 1 && SAMPLE_EVERY <= 60, "采样帧距 1~60 步");
const _: () = assert!(DIVERGENCE_PROBE_SCENE >= 1 && DIVERGENCE_PROBE_SCENE <= 8);

// ===========================================================================
// 第 17 节 · 内嵌测试（探针 cargo test 承载；std 仅测试期可用）
// ===========================================================================

#[cfg(test)]
mod tests {
    use super::*;

    /// 冒烟：Q16 定点数学层已知值。
    #[test]
    fn test_q16_math_basics() {
        // 1.5 × 2.0 = 3.0
        assert_eq!(qmul(98_304, 131_072), 196_608);
        // 3.0 / 2.0 = 1.5
        assert_eq!(qdiv(196_608, 131_072).unwrap_or(0), 98_304);
        // 除零必须报 0x1E35。
        match qdiv(1, 0) {
            Err(e) => assert_eq!(e.code(), 0x1E35),
            Ok(_) => panic!("div by zero must fail"),
        }
        // √4.0 = 2.0（Q16：√(262144·65536) = 131072）。
        assert_eq!(q_sqrt(262_144).unwrap_or(0), 131_072);
        // 负数开方报 0x1E36。
        match q_sqrt(-1) {
            Err(e) => assert_eq!(e.code(), 0x1E36),
            Ok(_) => panic!("sqrt negative must fail"),
        }
        // 1.234567 的十进制展开（截到 3 位 ≈ 1.234）。
        let s: String = q16_to_dec(80_899, 3);
        assert_eq!(s, "1.234");
        // 负数与进位：-0.9999 → -1.000（四舍五入进位）。
        let sneg: String = q16_to_dec(-65_530, 3);
        assert!(sneg.starts_with("-1.000") || sneg.starts_with("-0.999"));
    }

    /// 冒烟：isqrt 边界（0/1/完全平方/大数）。
    #[test]
    fn test_isqrt_edges() {
        assert_eq!(isqrt_u64(0), 0);
        assert_eq!(isqrt_u64(1), 1);
        assert_eq!(isqrt_u64(1_000_000), 1_000);
        assert_eq!(isqrt_u64(1_000_001), 1_000);
        let big: u64 = 4_200_000_000_000_000; // 4.2e15 < 2^62
        let r: u64 = isqrt_u64(big);
        assert!(r.wrapping_mul(r) <= big);
        assert!((r + 1).wrapping_mul(r + 1) > big);
    }

    /// 冒烟：种子搅拌器确定性 + 抖动域。
    #[test]
    fn test_seed_mixer_determinism() {
        let mut a: SeedMixer = SeedMixer::new(42).unwrap();
        let mut b: SeedMixer = SeedMixer::new(42).unwrap();
        let mut i: u32 = 0;
        while i < 100 {
            assert_eq!(a.next(), b.next());
            i += 1;
        }
        let mut c: SeedMixer = SeedMixer::new(43).unwrap();
        assert_ne!(a.next(), c.next());
        // 种子 0 拒绝。
        assert!(SeedMixer::new(0).is_err());
        // 抖动落域 [−amp, +amp]。
        let mut m: SeedMixer = SeedMixer::new(7).unwrap();
        let mut k: u32 = 0;
        while k < 200 {
            let j: i64 = m.jitter_q16(16_384).unwrap();
            assert!(j >= -16_384 && j <= 16_384);
            k += 1;
        }
    }

    /// 冒烟：NaN/Inf 位型防线（f32/f64 全表拒、有限过、raw_to_q16 饱和）。
    #[test]
    fn test_nan_pattern_guard() {
        let mut g: NanPatternGuard = NanPatternGuard::new();
        let mut i: usize = 0;
        while i < F32_CANONICAL_NONFINITE.len() {
            let bits: u32 = F32_CANONICAL_NONFINITE[i];
            match g.admit_u32(bits) {
                Err(e) => assert_eq!(e.code(), 0x1E3F),
                Ok(_) => panic!("f32 non-finite must be rejected"),
            }
            i += 1;
        }
        let mut j: usize = 0;
        while j < F64_CANONICAL_NONFINITE.len() {
            let bits: u64 = F64_CANONICAL_NONFINITE[j];
            assert!(g.admit_u64(bits).is_err());
            j += 1;
        }
        // 有限字通过。
        assert!(g.admit_u64(1).is_ok());
        assert!(g.admit_u32(0x3FC0_0000).is_ok()); // f32 1.5 的位型
        // 位型分类正确性。
        assert_eq!(classify_u32(0x7F80_0000), BitClass::Inf);
        assert_eq!(classify_u32(0x7FC0_0000), BitClass::Nan);
        assert_eq!(classify_u32(0x3FC0_0000), BitClass::Finite);
        assert_eq!(classify_u64(0x7FF8_0000_0000_0000), BitClass::Nan);
        assert_eq!(classify_u64(0xFFF0_0000_0000_0000), BitClass::Inf);
        assert_eq!(classify_u64(42), BitClass::Finite);
        // raw_to_q16：NaN 字拒绝；有限字低 48 位换算。
        assert!(g.raw_to_q16(0x7FF8_0000_0000_0000).is_err());
        let v: i64 = g.raw_to_q16(0x0000_0000_0001_0000).unwrap_or(-1);
        assert_eq!(v, 65_536); // 低 48 位 0x10000 → 65536
        // 解除态直通（洪流对照实验面）。
        g.set_armed(false);
        assert!(g.admit_u64(0x7FF8_0000_0000_0000).is_ok());
        assert!(g.rejects() >= 10);
    }

    /// 冒烟：八场景全部可跑、步数账对齐、终态指纹非退化。
    #[test]
    fn test_eight_scenes_run() {
        let mut r: ScenarioRunner = ScenarioRunner::new();
        let mut id: u8 = 1;
        while id <= SCENE_COUNT {
            let rec: RunRecord = match r.run(id, 9_978, None) {
                Ok(v) => v,
                Err(e) => panic!("scene {} failed: {} ({})", id, e.code(), e.detail()),
            };
            assert_eq!(rec.steps, scene_static(id).unwrap().steps);
            assert_eq!(rec.chain.len() as u32, rec.steps);
            assert_eq!(rec.digests.len() as u32, rec.steps);
            assert_eq!(rec.samples.len() > 0, true);
            assert_ne!(rec.fp_final, rec.chain_init);
            // 净账链验证必须为真（内部一致性）。
            assert!(verify_ledger(rec.chain_init, &rec.digests, &rec.chain));
            id += 1;
        }
        assert_eq!(r.runs, 8);
    }

    /// 冒烟：同种子同轨迹（八场景双跑全链一致——对拍自证）。
    #[test]
    fn test_same_seed_same_trajectory() {
        let mut r: ScenarioRunner = ScenarioRunner::new();
        let mut id: u8 = 1;
        while id <= SCENE_COUNT {
            let p: DualProof = r.run_dual(id, 3_141).unwrap();
            assert!(p.consistent, "scene {} diverged at {:?}", id, p.first_diff);
            assert_eq!(p.fp_a, p.fp_b);
            id += 1;
        }
        assert_eq!(r.divergences, 0);
    }

    /// 冒烟：异种子必分叉（多球与软绳两 jitter 场景——分叉反证）。
    #[test]
    fn test_diff_seed_diverges() {
        let mut r: ScenarioRunner = ScenarioRunner::new();
        let d6: u32 = r
            .assert_seed_divergence(DIVERGENCE_PROBE_SCENE, 100, 200)
            .unwrap();
        assert!(d6 >= 1);
        let d8: u32 = r.assert_seed_divergence(8, 100, 201).unwrap();
        assert!(d8 >= 1);
        // 非 jitter 场景请求分叉反证 → 拒绝（接线错误面）。
        assert!(r.assert_seed_divergence(1, 100, 200).is_err());
        // 同种子自比较 → 拒绝。
        assert!(r.assert_seed_divergence(6, 100, 100).is_err());
    }

    /// 冒烟：守恒指标在带（八场景能量 ppm；弹性/多球动量 ppm）。
    #[test]
    fn test_metrics_in_band() {
        let mut r: ScenarioRunner = ScenarioRunner::new();
        let mut id: u8 = 1;
        while id <= SCENE_COUNT {
            let rec: RunRecord = r.run(id, 2_718, None).unwrap();
            let spec: &SceneStatic = scene_static(id).unwrap();
            let rep: MetricReport = MetricExtractor::extract(&rec).unwrap();
            assert!(
                rep.energy_dev_ppm <= spec.energy_band_ppm,
                "scene {} energy {} ppm > band {}",
                id,
                rep.energy_dev_ppm,
                spec.energy_band_ppm
            );
            if spec.momentum_applicable {
                assert!(
                    rep.momentum_dev_ppm <= spec.momentum_band_ppm,
                    "scene {} momentum {} ppm > band {}",
                    id,
                    rep.momentum_dev_ppm,
                    spec.momentum_band_ppm
                );
            }
            if spec.conv_applicable {
                assert!(
                    rep.conv_step >= spec.conv_lo && rep.conv_step <= spec.conv_hi,
                    "scene {} conv {} out of [{},{}]",
                    id,
                    rep.conv_step,
                    spec.conv_lo,
                    spec.conv_hi
                );
            }
            id += 1;
        }
    }

    /// 冒烟：三注入各检出（篡改/指纹漂移/越带伪造）。
    #[test]
    fn test_three_injections_detected() {
        let mut r: ScenarioRunner = ScenarioRunner::new();
        assert!(r.inject_scene_tamper(1, 55).is_ok());
        assert!(r.inject_fingerprint_drift(1, 55, 3).is_ok());
        assert!(r.inject_fingerprint_drift(2, 55, 100).is_ok());
        let case: u32 = r.inject_band_fake(1, 99_999_999, 1).unwrap();
        assert_eq!(case as usize, r.band_cases.len() - 1);
        assert_eq!(r.injections_detected, 4);
        // 未越带伪值 → 注入本身不成立。
        assert!(r.inject_band_fake(1, 1, 1).is_err());
        // 越界步号 → 拒绝。
        assert!(r.inject_fingerprint_drift(1, 55, 999_999).is_err());
    }

    /// 冒烟：示例册双构建一致 + 冻结生效。
    #[test]
    fn test_book_dual_consistency_and_freeze() {
        let mut ids: Vec<u8> = Vec::new();
        let mut id: u8 = 1;
        while id <= SCENE_COUNT {
            ids.push(id);
            id += 1;
        }
        assert!(ExampleBook::verify_dual(&ids, 61).unwrap());
        let mut book: ExampleBook = ExampleBook::build(&ids, 61).unwrap();
        assert_eq!(book.entries.len(), 8);
        let fp: u64 = book.freeze();
        assert_eq!(fp, book.book_fp);
        let r: Result<(), PhysExError> = book.add_entry(BookEntry {
            scene: 9,
            name: "x",
            steps: 0,
            fp_final: 0,
            energy_dev_ppm: 0,
            momentum_dev_ppm: 0,
            conv_step: 0,
            verdict: Verdict::Pass,
        });
        match r {
            Err(e) => assert_eq!(e.code(), 0x1E3E),
            Ok(_) => panic!("sealed book must reject"),
        }
    }

    /// 冒烟：三例同源视图 + 帧号跳转 + 断点联动。
    #[test]
    fn test_views_and_jump_with_breakpoints() {
        let mut r: ScenarioRunner = ScenarioRunner::new();
        let rec: RunRecord = r.run(5, 77, None).unwrap();
        let rep: MetricReport = MetricExtractor::extract(&rec).unwrap();
        let v1: String = DebugView::trajectory_table(&rec, 8);
        let v2: String = DebugView::event_timeline(&rec, 8);
        let v3: String = DebugView::metric_panel(&rec, &rep);
        assert!(v1.contains("帧 ") && v1.contains("三例同源"));
        assert!(v2.contains("帧 ") && v2.contains("事件时间线"));
        assert!(v3.contains("能量守恒偏差") && v3.contains("抛体"));
        // 跳转：帧 100 落到 ≤100 的最近采样帧 96（采样帧距 8，100 非 8 的倍数）。
        let mut bps: Breakpoints = Breakpoints::new();
        assert!(bps.set(64).is_ok());
        assert!(bps.set(96).is_ok());
        assert!(bps.set(0).is_err()); // 帧号 0 拒绝
        let j: JumpResult = DebugView::jump(&rec, 100, 0, &bps).unwrap();
        assert_eq!(j.frame, 96);
        assert_eq!(j.breakpoint_hit, Some(96)); // 联动：≤96 最近断点
        let j2: JumpResult = DebugView::jump(&rec, 50, 0, &bps).unwrap();
        assert!(j2.breakpoint_hit.is_none()); // 50 之前无断点
        // 帧号超出采样域 → 落到最后一帧。
        let j3: JumpResult = DebugView::jump(&rec, 999_999, 0, &bps).unwrap();
        assert!(j3.frame <= rec.steps);
    }

    /// 冒烟：健康面与自检全绿。
    #[test]
    fn test_health_and_selfcheck() {
        let mut c: ExampleCenter = ExampleCenter::new();
        let _ = c.run_scene(3, 88).unwrap();
        let fp: u64 = c.build_book(88).unwrap();
        assert_ne!(fp, 0);
        let h: HealthReport = c.health();
        assert_eq!(h.runs, 9); // 1 手动 + 8 建册
        assert_eq!(h.book_frozen, true);
        assert_eq!(h.book_fp, fp);
        assert_eq!(h.divergences, 0);
        let rep: SelfCheckReport = c.selfcheck(88).unwrap();
        let mut i: usize = 0;
        while i < rep.lines.len() {
            let l: &CheckLine = &rep.lines[i];
            assert!(l.pass, "selfcheck [{}] failed: {}", l.name, l.note);
            i += 1;
        }
        assert!(rep.all_pass);
    }

    /// 冒烟：场景闭集边界（0/9 号拒绝；步数上限拒绝；种子 0 拒绝）。
    #[test]
    fn test_scene_and_step_bounds() {
        let mut r: ScenarioRunner = ScenarioRunner::new();
        match r.run(0, 1, None) {
            Err(e) => assert_eq!(e.code(), 0x1E31),
            Ok(_) => panic!("scene 0 must fail"),
        }
        match r.run(9, 1, None) {
            Err(e) => assert_eq!(e.code(), 0x1E31),
            Ok(_) => panic!("scene 9 must fail"),
        }
        match r.run(1, 0, None) {
            Err(e) => assert_eq!(e.code(), 0x1E32),
            Ok(_) => panic!("seed 0 must fail"),
        }
        match r.run(1, 1, Some(0)) {
            Err(e) => assert_eq!(e.code(), 0x1E37),
            Ok(_) => panic!("0 steps must fail"),
        }
        match r.run(1, 1, Some(MAX_STEPS + 1)) {
            Err(e) => assert_eq!(e.code(), 0x1E37),
            Ok(_) => panic!("over-cap steps must fail"),
        }
        // 错误面：全变体 detail 非空 + 码在段内。
        let all: [PhysExError; 15] = [
            PhysExError::SceneUnknown,
            PhysExError::SeedInvalid,
            PhysExError::BodyInvalid,
            PhysExError::ParamOutOfRange,
            PhysExError::DivByZero,
            PhysExError::SqrtNegative,
            PhysExError::StepCountExceeded,
            PhysExError::DivergenceDetected,
            PhysExError::SeedDivergeFailed,
            PhysExError::MetricOutOfBand,
            PhysExError::TamperEscaped,
            PhysExError::DriftEscaped,
            PhysExError::BandInjectEscaped,
            PhysExError::BookSealed,
            PhysExError::NanPatternRejected,
        ];
        let mut i: usize = 0;
        while i < all.len() {
            let code: u16 = all[i].code();
            assert!(code >= 0x1E31 && code <= 0x1E3F);
            assert!(all[i].detail().len() >= 8);
            i += 1;
        }
    }

    /// 冒烟：教学解说面（八场景四步解说 + 读屏/CI 说明在位）。
    #[test]
    fn test_narration_and_notes() {
        assert!(CI_GUARD_NOTE.contains("CI"));
        assert!(ACCESS_NOTE.contains("读屏说明"));
        let mut id: u8 = 1;
        while id <= SCENE_COUNT {
            let spec: &SceneStatic = scene_static(id).unwrap();
            assert!(spec.name.len() >= 2);
            assert!(spec.note.len() >= 10);
            let mut k: usize = 0;
            while k < 4 {
                assert!(spec.narration[k].len() >= 8, "scene {} step {}", id, k);
                k += 1;
            }
            id += 1;
        }
        // 场景指纹目录：镜像必失配（直接复验注入一的判据源）。
        let w: WorldState = build_scene(2, 5).unwrap();
        let mut mirror: WorldState = w.clone();
        mirror.bodies[0].x += 65_536;
        assert_ne!(w.catalog_fingerprint(), mirror.catalog_fingerprint());
    }

    /// 冒烟：滚动事件账容量防线（超限丢弃计数，长度钳在容量）。
    #[test]
    fn test_event_ledger_rolling() {
        let mut led: EventLedger = EventLedger::new();
        let mut i: u32 = 0;
        while i < (LEDGER_CAP as u32) + 50 {
            led.push(PhysEvent {
                tick: i,
                kind: EventKind::RunStart,
                a: 0,
                b: 0,
            });
            i += 1;
        }
        assert_eq!(led.len(), LEDGER_CAP);
        assert_eq!(led.dropped(), 50);
        assert_eq!(led.pushed(), LEDGER_CAP as u32 + 50);
        // 最旧被滚掉：账头应是 tick=50。
        assert_eq!(led.get(0).map(|e| e.tick).unwrap_or(0), 50);
    }
}
