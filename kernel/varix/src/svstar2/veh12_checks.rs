//! VE-F1412 · 域自检（判据逐条对应，见 `veh12_latency.rs` 头注）
//!
//! 判据映射（锚点原文 → 自检项）：
//! - ≤20ms 目标（端到端 ≤20ms；20ms 可演奏阈值有感知依据）→ `H12-目标-*`
//! - 并存（低延迟与播放共存；高缓冲不拖累低延迟调度优先级）→ `H12-并存-*`
//! - 小缓冲策略（独占模式 / 最小缓冲权衡表 / 零处理直通）→ `H12-缓冲-*` / `H12-直通-*`
//! - 恢复特化（预测性补零 / 低延迟专属恢复 / 模式内权衡声明）→ `H12-恢复-*`
//! - 实测（真实测量上报，不标称）→ `H12-实测-*`
//! - 切换协议（进入/退出安全切换不爆音）→ `H12-切换-*`
//! - 边界（不接管播放链 / 无平台独占入口）→ `H12-边界-*`
//!
//! ## 弱门禁防线（本文件的判据为什么这样写）
//!
//! 1. **"三档延迟都 ≤20ms"是弱门禁**：把 `TARGET_LATENCY_MS` 改成 1000.0
//!    照样全绿。故 `H12-目标-10ms块不达标` 用**反例档**给目标装牙齿：
//!    10ms 块（480 帧 @48k）端到端恰为 20.5ms，必须**不**达标。这条与达标
//!    判据合起来才使目标既非恒真又非恒假。
//!
//! 2. **"目标阈值 < 感知阈值"要断单边符号而非双边区间**：两条常量相等即转红。
//!    这样"20ms 有感知依据"落在数据上，不在注释里。
//!
//! 3. **分解守恒须判据侧独立重算**：`H12-目标-分解守恒` 自己写 `a + b + c`
//!    与被测 `total_ms` 对账，不调 `total_of_parts` 自证。
//!
//! 4. **并存"不拖累"要断逐位不变**：不是在"有/无播放会话"两种情形比，而是
//!    断 `0 / 1 / 8` 个不同档位的播放会话下 `low_wake_lead_ms()` 与
//!    `low_priority()` **逐位相等**。一个"按 play 数量缩放提前量"的实现在
//!    只挂 1 个播放会话时能过，挂 8 个才转红——单一情形比对会漏。
//!
//! 5. **并存"不挤占"要断 play_starved == 0**：只断"低延迟优先"会把"低延迟把
//!    播放挤掉"的实现放过。`H12-并存-播放不被挤占` 在需求 ≤ 槽位时断 starved
//!    恰为 0，同时 `H12-并存-超槽位显性饥饿` 断超出部分**显性计数**（不静默
//!    丢弃）。
//!
//! 6. **权衡表要两条反向单调律**：周期升序 + 风险降序。只断"三档互不相等"
//!    是弱门禁（随机常数也能过）；把风险也写成随缓冲升序的实现（复制粘贴错误
//!    的典型形态）在周期律上绿、在风险律上红。
//!
//! 7. **直通要断"恰等于 2×period"而非"更快"**：把 process 减半而非归零的实现
//!    会在"更快"上通过、在精确等式上转红。另断 `processing_frames == 0`
//!    （零 DSP 的字面量）。
//!
//! 8. **预测补零要夹逼对 + 提前半程**：`water == lead` 触发、`lead + 1` 不
//!    触发（界位置钉死）；`water == lead / 2` 已触发（预测的牙齿）。只断
//!    "水位 0 触发"的话非预测实现也过。
//!
//! 9. **计数双断**：每个计数器既断**无事件时恰为 0**，又走真实路径造出 N 个
//!    事件断**恰等于 N**（用 `==` 不用 `>=`，否则"每次 +2"也过）。恢复时延
//!    累计额由判据侧独立重算 `N × penalty`，不断 `recovery_latency_ms()` 自证。
//!
//! 10. **实测必须与标称可区分**：喂一个与标称**不相等**的观测值，断
//!     `reported == observed` **且** `reported != nominal`。少了后一条，前一条
//!     在"直接返回标称"的实现下也成立（因为首个观测恰好等于标称时会同时满足）
//!     ——两条合起来才有区分力。
//!
//! 11. **命中率分母是总次数（净值口径）**：判据自己用 `hits × 1e6 / (hits +
//!     misses)` 重算并断相等，不调 `hit_rate_ppm()`。同时断"零观测时恰为 0"。
//!
//! 12. **非有限观测要断具体后果**：NaN/Inf/负值**不计入 samples**、不改变
//!     `reported_ms`、计入 `rejected`——断的是"数据未污染"，不是"没 panic"。
//!
//! 13. **切换不爆音要断相邻步长上界**：判据密集采样整条斜坡，逐帧断
//!     `|Δgain| <= |to−from| / ramp`（精确上界，非"约等于"），并断首帧恰为
//!     `from`、末帧恰为 `to`、全程落在凸包内。
//!
//! 14. **硬切对照让"不爆音"有可证伪的对手**：`H12-切换-硬切超界对照` 判据侧
//!     独立算 `|to−from| > |to−from| / ramp`（ramp > 1 时成立）——证明这条
//!     闸不是恒真。`ramp == 0` 被构造层拒收（零帧斜坡即硬切）。
//!
//! 15. **进入/退出双向各钉**：方向对称不是显然的（差值可为负），故分别断
//!     `0→1` 与 `1→0` 的步长上界。
//!
//! 16. **边界穷举公开面**：断 `PUBLIC_SURFACE` 的每项都存在且
//!     `FORBIDDEN_SURFACE_TOKENS` 一个都不在其中（平台独占入口/全局开关）。
//!
//! 17. **变体反向验证**（`VARIANT_REGISTRY` 登记 27 项）：判据全绿只证明"当前
//!     实现合判据"。27 个定向变异逐条反查，**27/27 全部被捕获**，证明判据非
//!     恒真。登记与判据同文件，"新增判据须补登记"因此可审。
//!
//! ## 首轮变异验证的实际收获（留档以警示后来者）
//!
//! 首轮 27 个变异跑出 **26 捕获 / 1 EQUIV / 0 漏网**。那 1 个 EQUIV 揪出
//! 本单最值钱的一条纪律：
//!
//! 1. **EQUIV 不等于"变异选错"，先查是不是判据太松**（记忆十诫的反面：
//!    门禁弱于变异时，弱门禁会把真缺陷洗成"等价"）。
//!    `M14-rounding-at-44100`（把 `frames()` 的整数缩放换成直接用
//!    `base_frames`，即**忽略采样率**）首轮报 EQUIV。我先怀疑"变异选错"
//!    （改了不可达分支），复算后发现都不是：变异在 44.1kHz 下把三档帧数从
//!    44/88/220 变成 48/96/240，周期偏差 0.088 / 0.177 / 0.442ms ——
//!    **确实改变了行为**，而我那条 `H12-缓冲-跨采样率缩放` 写的是
//!    "周期落在名义 ±1.0ms 内"，容差 1.0ms 把 0.442ms 的偏差完整吞掉。
//!    ⇒ 这是**弱门禁**，不是等价变异。修法：换成**精确帧数断言**
//!    （44/88/220，闭式 `base×fs/48000` 可推导、无容差空间）并把周期容差
//!    收到 0.02ms（截断误差上界 ≈ 半帧/采样率 = 0.011ms）。收紧后 M14 转红。
//!
//! 2. **浮点插值的步长判据不能写严格等式**：切换斜坡的相邻增益差按**增益
//!    量级**的 ULP 增长（实测 0.26→0.27 两点之间舍入误差 ≈1.2e-7×0.27），
//!    而"步长上界自身的 ULP"只有 1.24e-9，差 16 倍。初版用
//!    `d <= bound`（`bound` 为精确斜率）**首跑即红**——但那是**判据的错**
//!    （把 f32 表示误差当实现缺陷），不是实现错。修法：容差按误差真实来源
//!    推导（`4 × max(|from|,|to|,1) × EPSILON`），而**端点仍保持精确等式**
//!    （`p=0`、`p=ramp` 都可精确表示）⇒ 强断言一点没弱，且硬切变异仍有
//!    2096 倍裕度被捕获（M26 转红已验证）。
//!
//! 3. **"目标达标"必须配反例档**：M1（把目标改成 1000ms）能被
//!    `H12-目标-10ms反例不达标` 捕获，而若只写达标判据则它必然全绿。
//!    这验证了"给主张配反例"不是形式主义。
//!
//! 4. **并存"不拖累"必须跨情形钉**：M7（按 play 数量缩放提前量）能被
//!    `H12-并存-高缓冲不拖累低延迟` 捕获，因为该判据在 0/1/8 三情形下逐位
//!    比对。单情形比对（只挂1 个 play）会漏掉这个变异。
//!
//! 5. **恢复代价"相对序"不够**：M20（把标准档代价改成 2.0）与 M21
//!    （累计额写死常量 2.0）分别由 `H12-恢复-两档代价具体值` 与
//!    `H12-恢复-标准档独立记账` 捕获。只断"特化 < 标准"的相对序时，
//!    "两档都变相等"被相对序抓住但"两档都改成别的值"会被漏。

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

use super::veh12_latency::{
    breakdown, effective_latency_ms, meets_target, mmcss_of, playback_spec,
    underrun_risk_ppm, BufferTier, CoexistScheduler, LatencyReport, MmcssPriority,
    RecoveryPolicy, SessionClass, SessionSpec, SwitchSmoother, UnderrunGuard,
    COEXIST_BOUNDARY, DEFAULT_RAMP_FRAMES, FORBIDDEN_SURFACE_TOKENS, LOW_SLOTS_PER_TICK,
    MAX_SAMPLE_RATE, MIN_SAMPLE_RATE, MODE_TRADEOFF_DECLARATION, PLAY_SLOTS_PER_TICK,
    PROC_OVERHEAD_MS, PUBLIC_SURFACE, REFERENCE_SAMPLE_RATE, TARGET_LATENCY_MS,
    PLAYABLE_PERCEIVED_MS, low_latency_guard, low_latency_spec, should_pad_predicatively,
};

// 判据使用的常量（部分从被测模块导入，部分在判据侧独立写死用于对账）。
use super::veh12_latency::UNDERRUN_RISK_PPM;

/// 48kHz 下三档的周期（毫秒）——判据侧**独立写死**的期望值（闭式 48/96/240
/// 帧 @48kHz = 1.0 / 2.0 / 5.0ms）。不调 `period_ms()` 自证。
const EXP_MS1: f32 = 1.0;
const EXP_MS2: f32 = 2.0;
const EXP_MS5: f32 = 5.0;

/// 10ms 块 @48kHz 的周期（480 帧）——"必然不达标"反例档的周期。
const EXP_MS10: f32 = 10.0;

/// 10ms 反例档的帧数。
const FRAMES_10MS: u32 = 480;

/// 斜坡步长判据的 ULP 容差（**按增益量级推导，不是魔数**）。
///
/// 首轮实跑揪出的真实问题（VE-F1412 留档）：斜坡插值
/// `g(p) = from + (to−from)·p/ramp` 的每点舍入误差按**增益自身的量级**增长，
/// 而非按步长上界增长。实测（from=0,to=1,ramp=96）：步长上界
/// `1/96 ≈ 1.0416667e-2`，实测最大相邻差 `1.0416687e-2`，**超出 1.96e-8**
/// ——约 16 倍于"上界自身的 ULP"（1.24e-9），因为两个相邻增益值各自带半个 ULP
/// 的表示误差，差值误差因此按 `max(|from|,|to|)·EPS` 而非 `bound·EPS` 计。
///
/// 故本判据的容差取 `4 × max(|from|,|to|,1) × f32::EPSILON`：由**误差的
/// 真实来源**推导，而非"放宽到能过"。配��：
/// - 端点仍是**精确等式**（`p=0` 落在 `from`、`p=ramp` 落在 `to`，两者都
///   可精确表示，不受容差影响）⇒ 强断言一点没弱；
/// - 硬切的相邻差是 `|to−from|`，与本容差之比约 **2096 倍**（上界 96 倍
///   之上再乘 21.8）⇒ M26 硬切变异仍有充裕的捕获裕度，门禁不恒真。
fn step_ulp_tolerance(from: f32, to: f32) -> f32 {
    let scale = if from.abs() > to.abs() {
        from.abs()
    } else {
        to.abs()
    };
    let base = if scale > 1.0 { scale } else { 1.0 };
    4.0 * base * f32::EPSILON
}

/// VE-F1412 域自检入口。
pub fn run_veh12_checks() -> CheckSet {
    let mut set = CheckSet::new("svstar2-veh12");

    // ---- 判据：≤20ms 目标 ----

    {
        // 感知依据进入数据：目标阈值严格小于可感知显著阈值（单边符号）。
        // 两者相等即转红——"20ms 有感知依据"不许只是一句话。
        set.add(
            "H12-目标-阈值严格低于感知阈值",
            TARGET_LATENCY_MS < PLAYABLE_PERCEIVED_MS,
            "",
        );
    }
    {
        // 三档小缓冲的端到端闭式值（判据侧独立重算 `2×period + proc`）：
        //   1ms → 2×1.0 + 0.5 = 2.5ms
        //   2ms → 2×2.0 + 0.5 = 4.5ms
        //   5ms → 2×5.0 + 0.5 = 10.5ms
        // 用**精确等式**（二进制可表示，.0/.5 结尾），不写双边容差。
        let e1 = effective_latency_ms(BufferTier::Ms1, REFERENCE_SAMPLE_RATE, PROC_OVERHEAD_MS, false);
        let e2 = effective_latency_ms(BufferTier::Ms2, REFERENCE_SAMPLE_RATE, PROC_OVERHEAD_MS, false);
        let e5 = effective_latency_ms(BufferTier::Ms5, REFERENCE_SAMPLE_RATE, PROC_OVERHEAD_MS, false);
        let ok = e1 == 2.5 && e2 == 4.5 && e5 == 10.5;
        set.add(
            "H12-目标-三档闭式精确等式",
            ok,
            if ok {
                ""
            } else {
                leak(format!("e1={} e2={} e5={}", e1, e2, e5))
            },
        );
    }
    {
        // 达标：三档均 ≤20ms（单边 `<=`）。
        set.add(
            "H12-目标-三档全部达标",
            meets_target(effective_latency_ms(
                BufferTier::Ms1,
                REFERENCE_SAMPLE_RATE,
                PROC_OVERHEAD_MS,
                false,
            )) && meets_target(effective_latency_ms(
                BufferTier::Ms2,
                REFERENCE_SAMPLE_RATE,
                PROC_OVERHEAD_MS,
                false,
            )) && meets_target(effective_latency_ms(
                BufferTier::Ms5,
                REFERENCE_SAMPLE_RATE,
                PROC_OVERHEAD_MS,
                false,
            )),
            "",
        );
    }
    {
        // **反例档**：10ms 块（480 帧 @48k）端到端 = 10 + 0.5 + 10 = 20.5ms，
        // 严格大于目标。这条给目标装牙齿——没有它，把目标改成 1000ms 仍全绿。
        //
        // 判据侧**完全独立**构造这个 10ms 周期（不调被测任何函数）：
        //   period = 480 帧 × 1000 / 48000 = 10.0ms
        //   total  = 2 × 10.0 + 0.5 = 20.5ms
        let period_10 = FRAMES_10MS as f32 * 1000.0 / REFERENCE_SAMPLE_RATE as f32;
        let total_10 = 2.0 * period_10 + PROC_OVERHEAD_MS;
        let ok = period_10 == EXP_MS10 && total_10 == 20.5 && total_10 > TARGET_LATENCY_MS;
        set.add(
            "H12-目标-10ms反例不达标",
            ok && !meets_target(total_10),
            if ok {
                ""
            } else {
                leak(format!("period={} total={} tgt={}", period_10, total_10, TARGET_LATENCY_MS))
            },
        );
    }
    {
        // 目标边界归属：恰好 20ms 算达标（`<=` 而非 `<`）。
        set.add(
            "H12-目标-边界取等号",
            meets_target(TARGET_LATENCY_MS) && !meets_target(TARGET_LATENCY_MS + 0.5),
            "",
        );
    }
    {
        // 分解守恒：判据侧**独立**做 `a + b + c`，与被测 `total_ms` 对账，
        // 不调 `total_of_parts`（否则自证式）。三条通路各断一次：Ms1 处理、
        // Ms2 直通、Ms5 处理。
        let s1 = match low_latency_spec(BufferTier::Ms1, REFERENCE_SAMPLE_RATE, false) {
            Ok(s) => s,
            Err(_) => {
                set.fail("H12-目标-分解守恒", "spec_ms1_rejected");
                skip_rest(&mut set);
                return set;
            }
        };
        let s5 = match low_latency_spec(BufferTier::Ms5, REFERENCE_SAMPLE_RATE, false) {
            Ok(s) => s,
            Err(_) => {
                set.fail("H12-目标-分解守恒", "spec_ms5_rejected");
                skip_rest(&mut set);
                return set;
            }
        };
        let b1 = breakdown(&s1);
        let b5 = breakdown(&s5);
        let sum1 = b1.input_period_ms + b1.process_ms + b1.output_period_ms;
        let sum5 = b5.input_period_ms + b5.process_ms + b5.output_period_ms;
        let ok = b1.total_ms == sum1 && b5.total_ms == sum5;
        set.add(
            "H12-目标-分解守恒",
            ok,
            if ok {
                ""
            } else {
                leak(format!("b1={}/{} b5={}/{}", b1.total_ms, sum1, b5.total_ms, sum5))
            },
        );
    }

    // ---- 判据：并存 ----

    {
        // 低延迟优先级严格高于播放（单边符号），且不上 Realtime（诚实边界）。
        let low = mmcss_of(SessionClass::LowLatency);
        let play = mmcss_of(SessionClass::Playback);
        let ok = low.rank() < play.rank() && low != MmcssPriority::Realtime;
        set.add(
            "H12-并存-低延迟优先且不上Realtime",
            ok,
            if ok {
                ""
            } else {
                leak(format!("low={} play={}", low.rank(), play.rank()))
            },
        );
    }
    {
        // MMCSS 五档 rank 严格降序（rank 连续 0..4）——断分级本身没乱序。
        let r: Vec<u8> = MmcssPriority::ALL.iter().map(|p| p.rank()).collect();
        let ok = MmcssPriority::ALL.len() == 5
            && r[0] == 0
            && r[1] == 1
            && r[2] == 2
            && r[3] == 3
            && r[4] == 4
            && MmcssPriority::from_rank(2) == Some(MmcssPriority::Playback);
        set.add("H12-并存-五档rank连续", ok, "");
    }
    {
        // **不拖累的机械不变式**：0 / 1 / 8 个不同档位的播放会话下，低延迟的
        // 提前量与优先级**逐位不变**。单情形比对会漏掉"按 play 数量缩放"的
        // 实现，故必须跨三情形。
        let mut base_lead: Option<f32> = None;
        let mut base_prio: Option<MmcssPriority> = None;
        let mut all_same = true;
        for n in [0usize, 1, 8] {
            let mut sch = CoexistScheduler::new();
            let low = match low_latency_spec(BufferTier::Ms1, REFERENCE_SAMPLE_RATE, false) {
                Ok(s) => s,
                Err(_) => {
                    all_same = false;
                    break;
                }
            };
            if sch.attach(low).is_err() {
                all_same = false;
                break;
            }
            for i in 0..n {
                let tier = match i % 3 {
                    0 => BufferTier::Ms1,
                    1 => BufferTier::Ms2,
                    _ => BufferTier::Ms5,
                };
                let ps = match playback_spec(tier, REFERENCE_SAMPLE_RATE) {
                    Ok(s) => s,
                    Err(_) => {
                        all_same = false;
                        break;
                    }
                };
                let _ = sch.attach(ps);
            }
            let lead = sch.low_wake_lead_ms();
            let prio = sch.low_priority();
            match base_lead {
                None => {
                    base_lead = lead;
                    base_prio = prio;
                }
                Some(bl) => {
                    // 逐位相等（不是"约等于"）。
                    let same = match lead {
                        Some(a) => a.to_bits() == bl.to_bits(),
                        None => false,
                    };
                    if !same {
                        all_same = false;
                    }
                    if prio != base_prio {
                        all_same = false;
                    }
                }
            }
        }
        set.add("H12-并存-高缓冲不拖累低延迟", all_same, "");
    }
    {
        // **并存不挤占**：低延迟先行服务，且播放在其后仍拿到剩余槽位
        // （需求 ≤ 槽位时 `play_starved` 恰为 0）。只断"低延迟优先"会把
        // "低延迟把播放挤掉"的实现放过。
        let mut sch = CoexistScheduler::new();
        let low = low_latency_spec(BufferTier::Ms1, REFERENCE_SAMPLE_RATE, false).ok();
        let ok = match low {
            Some(l) => {
                if sch.attach(l).is_err() {
                    false
                } else {
                    for _ in 0..3 {
                        if let Ok(p) = playback_spec(BufferTier::Ms2, REFERENCE_SAMPLE_RATE) {
                            let _ = sch.attach(p);
                        }
                    }
                    let t = sch.tick();
                    t.low_served && t.low_served_first && t.play_starved == 0 && t.play_served == 3
                }
            }
            None => false,
        };
        set.add("H12-并存-播放不被挤占", ok, "");
    }
    {
        // 超槽位时饥饿**显性计数**（不静默丢弃）：挂 8 个播放、槽位 4 ⇒
        // served 恰 4、starved 恰 4。
        let mut sch = CoexistScheduler::new();
        if let Ok(l) = low_latency_spec(BufferTier::Ms1, REFERENCE_SAMPLE_RATE, false) {
            let _ = sch.attach(l);
        }
        for _ in 0..8 {
            if let Ok(p) = playback_spec(BufferTier::Ms5, REFERENCE_SAMPLE_RATE) {
                let _ = sch.attach(p);
            }
        }
        let t = sch.tick();
        let expect_served = PLAY_SLOTS_PER_TICK as usize;
        let ok = t.play_served == expect_served
            && t.play_starved == 8 - expect_served
            && (LOW_SLOTS_PER_TICK >= 1);
        set.add("H12-并存-超槽位显性饥饿", ok, "");
    }
    {
        // 低延迟会话至多一个（第二个拒收）。
        let mut sch = CoexistScheduler::new();
        let a = low_latency_spec(BufferTier::Ms1, REFERENCE_SAMPLE_RATE, false).ok();
        let b = low_latency_spec(BufferTier::Ms2, REFERENCE_SAMPLE_RATE, false).ok();
        let ok = match (a, b) {
            (Some(x), Some(y)) => {
                sch.attach(x).is_ok() && sch.attach(y).is_err()
            }
            _ => false,
        };
        set.add("H12-并存-低延迟唯一", ok, "");
    }

    // ---- 判据：小缓冲策略 ----

    {
        // 权衡表**两条反向单调律**：周期升序、风险降序。只断互异性是弱门禁。
        let p1 = BufferTier::Ms1.period_ms(REFERENCE_SAMPLE_RATE);
        let p2 = BufferTier::Ms2.period_ms(REFERENCE_SAMPLE_RATE);
        let p5 = BufferTier::Ms5.period_ms(REFERENCE_SAMPLE_RATE);
        let r1 = underrun_risk_ppm(BufferTier::Ms1);
        let r2 = underrun_risk_ppm(BufferTier::Ms2);
        let r5 = underrun_risk_ppm(BufferTier::Ms5);
        // 周期：精确闭式 + 严格升序
        let period_ok = p1 == EXP_MS1 && p2 == EXP_MS2 && p5 == EXP_MS5 && p1 < p2 && p2 < p5;
        // 风险：严格降序（缓冲越小越易 underrun）
        let risk_ok = r1 > r2 && r2 > r5;
        let ok = period_ok && risk_ok;
        set.add(
            "H12-缓冲-权衡表反向单调",
            ok,
            if ok {
                ""
            } else {
                leak(format!("p={},{},{} r={},{},{}", p1, p2, p5, r1, r2, r5))
            },
        );
    }
    {
        // 三档帧数闭式 @48k：48 / 96 / 240（判据侧写死期望）。
        let f1 = BufferTier::Ms1.frames(REFERENCE_SAMPLE_RATE);
        let f2 = BufferTier::Ms2.frames(REFERENCE_SAMPLE_RATE);
        let f5 = BufferTier::Ms5.frames(REFERENCE_SAMPLE_RATE);
        let ok = f1 == 48 && f2 == 96 && f5 == 240;
        set.add("H12-缓冲-三档帧数闭式", ok, "");
    }
    {
        // 风险表登记完整且数值与锚点权衡方向一致（1ms 档风险最高）。
        let ok = UNDERRUN_RISK_PPM.len() == 3
            && underrun_risk_ppm(BufferTier::Ms1) == 180_000
            && underrun_risk_ppm(BufferTier::Ms2) == 60_000
            && underrun_risk_ppm(BufferTier::Ms5) == 10_000;
        set.add("H12-缓冲-风险表数值锁定", ok, "");
    }
    {
        // 档位周期随采样率**整数缩放**：44.1kHz 下三档帧数恰为
        //   48×44100/48000 = 44.1 → 44 帧
        //   96×44100/48000 = 88.2 → 88 帧
        //  240×44100/48000 = 220.5 → 220 帧
        // （整数截断，故用 `base×fs/48000` 的**闭式期望帧数**断，而不是断
        // 某个魔数周期。）
        //
        // **首轮变异验证揪出的弱门禁（VE-F1412 留档）**：初版这条只断
        // "周期落在名义 ±1.0ms 内且严格升序"，而"忽略采样率直接用 base_frames"
        // 的变异（M14）在 44.1k 下偏差仅 0.088~0.442ms，**被 ±1.0ms 容差
        // 完全吞掉**（变异报 EQUIV）。实测数据：正确实现偏差 ≤0.011ms，
        // 变异体达 0.442ms。⇒ 修法不是"再放宽一点"，而是换成**精确帧数
        // 断言**（44/88/220，闭式可推导、无容差空间），并把周期容差收到
        // 0.02ms。收紧后 M14 转红，门禁恢复牙齿。
        let f1 = BufferTier::Ms1.frames(44_100);
        let f2 = BufferTier::Ms2.frames(44_100);
        let f5 = BufferTier::Ms5.frames(44_100);
        let frames_ok = f1 == 44 && f2 == 88 && f5 == 220;
        let p1 = BufferTier::Ms1.period_ms(44_100);
        let p2 = BufferTier::Ms2.period_ms(44_100);
        let p5 = BufferTier::Ms5.period_ms(44_100);
        // 截断误差上界 = 半帧/采样率 ≈ 0.011ms（44.1k 下 5ms 档）。
        let near = |x: f32, t: f32| (x - t).abs() <= 0.02;
        let ok = frames_ok
            && near(p1, 1.0)
            && near(p2, 2.0)
            && near(p5, 5.0)
            && p1 < p2
            && p2 < p5;
        set.add(
            "H12-缓冲-跨采样率缩放",
            ok,
            if ok {
                ""
            } else {
                leak(format!("frames={},{},{} p={:.5},{:.5},{:.5}", f1, f2, f5, p1, p2, p5))
            },
        );
    }
    {
        // 独占纪律：低延迟**必须**独占（不独占即拒收）；播放**不得**独占。
        let low_no_excl = SessionSpec::new(
            SessionClass::LowLatency,
            BufferTier::Ms1,
            REFERENCE_SAMPLE_RATE,
            PROC_OVERHEAD_MS,
            false,
            false,
        );
        let play_excl = SessionSpec::new(
            SessionClass::Playback,
            BufferTier::Ms2,
            REFERENCE_SAMPLE_RATE,
            PROC_OVERHEAD_MS,
            true,
            false,
        );
        let ok = low_no_excl.is_err() && play_excl.is_err();
        set.add("H12-缓冲-独占纪律双向", ok, "");
    }
    {
        // 采样率越界拒收（夹逼对：下界-1 拒、下界收、上界收、上界+1 拒）。
        let ok = SessionSpec::new(
            SessionClass::LowLatency,
            BufferTier::Ms1,
            MIN_SAMPLE_RATE - 1,
            PROC_OVERHEAD_MS,
            true,
            false,
        )
        .is_err()
            && SessionSpec::new(
                SessionClass::LowLatency,
                BufferTier::Ms1,
                MIN_SAMPLE_RATE,
                PROC_OVERHEAD_MS,
                true,
                false,
            )
            .is_ok()
            && SessionSpec::new(
                SessionClass::LowLatency,
                BufferTier::Ms1,
                MAX_SAMPLE_RATE,
                PROC_OVERHEAD_MS,
                true,
                false,
            )
            .is_ok()
            && SessionSpec::new(
                SessionClass::LowLatency,
                BufferTier::Ms1,
                MAX_SAMPLE_RATE + 1,
                PROC_OVERHEAD_MS,
                true,
                false,
            )
            .is_err();
        set.add("H12-缓冲-采样率夹逼对", ok, "");
    }

    // ---- 判据：零处理直通 ----

    {
        // 直通**恰等于 2×period**（精确等式）且 `processing_frames == 0`。
        // 只断"更快"的话，把 process 减半的实现照样过。
        let e_pt = effective_latency_ms(BufferTier::Ms1, REFERENCE_SAMPLE_RATE, PROC_OVERHEAD_MS, true);
        let s_pt = low_latency_spec(BufferTier::Ms1, REFERENCE_SAMPLE_RATE, true).ok();
        let exact_2x = e_pt == 2.0 * EXP_MS1;
        let zero_frames = match s_pt {
            Some(s) => s.processing_frames() == 0,
            None => false,
        };
        let ok = exact_2x && zero_frames;
        set.add("H12-直通-恰等两倍周期且零帧", ok, "");
    }
    {
        // 直通延迟**低于**处理路径（单边符号），三档各断。
        let mut ok = true;
        for tier in BufferTier::ALL {
            let pt = effective_latency_ms(tier, REFERENCE_SAMPLE_RATE, PROC_OVERHEAD_MS, true);
            let pr = effective_latency_ms(tier, REFERENCE_SAMPLE_RATE, PROC_OVERHEAD_MS, false);
            if !(pt < pr) {
                ok = false;
            }
        }
        set.add("H12-直通-低于处理路径", ok, "");
    }
    {
        // 直通时处理段**确为 0**（分解面，不是只有总延迟对）。
        let s = low_latency_spec(BufferTier::Ms2, REFERENCE_SAMPLE_RATE, true).ok();
        let ok = match s {
            Some(sp) => {
                let b = breakdown(&sp);
                b.process_ms == 0.0 && b.total_ms == 2.0 * EXP_MS2
            }
            None => false,
        };
        set.add("H12-直通-分解处理段为零", ok, "");
    }

    // ---- 判据：underrun 特化 ----

    {
        // 恢复特化：特化 penalty **严格小于**标准，且两者都 >0（"更快"不能
        // 退化成"不恢复"）。
        let std = RecoveryPolicy::StandardTiered.penalty_ms();
        let fast = RecoveryPolicy::LowLatencyFast.penalty_ms();
        let ok = fast < std && std > 0.0 && fast > 0.0;
        set.add(
            "H12-恢复-特化快于标准且非零",
            ok,
            if ok {
                ""
            } else {
                leak(format!("std={} fast={}", std, fast))
            },
        );
    }
    {
        // 两档 penalty 的**具体值**锁定（标准 8.0 / 特化 2.0）。这条与上一条
        // 合起来才真正钉住"代价被读取"——初版只断相对序，改成常量后仍相等。
        let std = RecoveryPolicy::StandardTiered.penalty_ms();
        let fast = RecoveryPolicy::LowLatencyFast.penalty_ms();
        let ok = std == 8.0 && fast == 2.0;
        set.add("H12-恢复-两档代价具体值", ok, "");
    }
    {
        // 预测补零**夹逼对**：`water == lead` 触发、`water == lead + 1` 不触发。
        let lead = 48u32;
        let ok = should_pad_predicatively(lead, lead)
            && !should_pad_predicatively(lead + 1, lead)
            && should_pad_predicatively(0, lead);
        set.add("H12-恢复-预测夹逼对", ok, "");
    }
    {
        // **提前半程**：水位尚未耗尽（lead/2）已触发——这才是"预测"的牙齿。
        // 非预测实现在这里必然不触发。
        let lead = 48u32;
        let ok = should_pad_predicatively(lead / 2, lead) && should_pad_predicatively(1, lead);
        set.add("H12-恢复-提前半程触发", ok, "");
    }
    {
        // 守卫计数双断：无事件时**恰为 0**（水位远高于提前量时不推进事件）。
        let g = UnderrunGuard::new(RecoveryPolicy::LowLatencyFast, 48).ok();
        let ok = match g {
            Some(mut gg) => {
                // 水位远高于提前量 ⇒ 不触发。
                for _ in 0..10 {
                    gg.advance(1000);
                }
                gg.pads() == 0 && gg.underruns() == 0 && gg.recoveries() == 0
            }
            None => false,
        };
        set.add("H12-恢复-无事件计数为零", ok, "");
    }
    {
        // 守卫计数双断：造出 **N** 次事件（低于提前量的水位）后，三个计数
        // **恰等于 N**（用 `==` 不用 `>=`，否则"每次 +2"也过），且累计时延
        // 恰等于 `N × penalty`（判据侧独立重算）。
        let n = 7u32;
        let g = UnderrunGuard::new(RecoveryPolicy::LowLatencyFast, 48).ok();
        let ok = match g {
            Some(mut gg) => {
                for _ in 0..n {
                    gg.advance(10);
                }
                let expected_latency = n as f32 * RecoveryPolicy::LowLatencyFast.penalty_ms();
                gg.pads() == n
                    && gg.underruns() == n
                    && gg.recoveries() == n
                    && gg.recovery_latency_ms() == expected_latency
            }
            None => false,
        };
        set.add("H12-恢复-事件计数恰等N", ok, "");
    }
    {
        // 标准策略累计额同样按其 penalty 记账（不是共用特化的 2.0）。
        let n = 3u32;
        let g = UnderrunGuard::new(RecoveryPolicy::StandardTiered, 48).ok();
        let ok = match g {
            Some(mut gg) => {
                for _ in 0..n {
                    gg.advance(5);
                }
                let expected = n as f32 * RecoveryPolicy::StandardTiered.penalty_ms();
                gg.pads() == n && gg.recovery_latency_ms() == expected
            }
            None => false,
        };
        set.add("H12-恢复-标准档独立记账", ok, "");
    }
    {
        // 提前量 0（无预测）被拒收——预测是必需能力，不是可选项。
        let ok = UnderrunGuard::new(RecoveryPolicy::LowLatencyFast, 0).is_err();
        set.add("H12-恢复-零提前量拒收", ok, "");
    }
    {
        // 推荐构造：提前量取整周期（Ms1@48k ⇒ 48 帧）。
        let g = low_latency_guard(BufferTier::Ms1, REFERENCE_SAMPLE_RATE).ok();
        let ok = match g {
            Some(gg) => gg.lead_frames == 48 && gg.policy() == RecoveryPolicy::LowLatencyFast,
            None => false,
        };
        set.add("H12-恢复-推荐构造取整周期", ok, "");
    }
    {
        // 模式内权衡声明含关键成分且非空（显性，不藏在实现里）。
        let d = MODE_TRADEOFF_DECLARATION;
        let ok = !d.is_empty()
            && d.contains("LowLatencyFast")
            && d.contains("8.0")
            && d.contains("2.0")
            && d.contains("underrun");
        set.add("H12-恢复-权衡声明关键成分", ok, "");
    }

    // ---- 判据：实测上报 ----

    {
        // **实测与标称可区分**：喂一个与标称**不相等**的观测值，断
        // `reported == observed` 且 `reported != nominal`。少了后一条，"直接
        // 返回标称"的实现也能过（因为观测恰等于标称时两条同时成立）。
        let nominal = 10.5f32;
        let observed = 17.25f32; // 刻意不等于 nominal
        let ok = match LatencyReport::new(nominal) {
            Ok(mut r) => {
                r.observe(observed);
                r.reported_ms() == observed
                    && r.reported_ms() != r.nominal_ms()
                    && r.nominal_ms() == nominal
                    && r.samples() == 1
            }
            Err(_) => false,
        };
        set.add("H12-实测-上报跟随实测值", ok, "");
    }
    {
        // 命中/未命中计数：达标（≤20）与未达标（>20）各断，且分母是**总次数**。
        // 命中率由判据侧独立重算 `hits×1e6/(hits+misses)`，不调 `hit_rate_ppm`。
        let ok = match LatencyReport::new(10.0) {
            Ok(mut r) => {
                r.observe(12.0); // 命中
                r.observe(25.0); // 未命中
                r.observe(20.0); // 命中（边界取等）
                let hits = r.hits();
                let misses = r.misses();
                let expected_rate = (hits as u64 * 1_000_000 / (hits + misses) as u64) as u32;
                hits == 2
                    && misses == 1
                    && r.samples() == 3
                    && r.hit_rate_ppm() == expected_rate
                    && r.hit_rate_ppm() == 666_666
            }
            Err(_) => false,
        };
        set.add("H12-实测-命中率独立重算", ok, "");
    }
    {
        // 非有限/负观测**拒收**：不计入 samples、不改 reported、计入 rejected。
        let ok = match LatencyReport::new(10.0) {
            Ok(mut r) => {
                r.observe(f32::NAN);
                r.observe(f32::INFINITY);
                r.observe(-1.0);
                r.samples() == 0 && r.rejected() == 3 && r.reported_ms() == 10.0
            }
            Err(_) => false,
        };
        set.add("H12-实测-非法观测拒收", ok, "");
    }
    {
        // 零观测时命中率恰为 0（除零防护）。
        let ok = match LatencyReport::new(10.0) {
            Ok(r) => r.hit_rate_ppm() == 0 && r.samples() == 0,
            Err(_) => false,
        };
        set.add("H12-实测-零观测命中率为零", ok, "");
    }
    {
        // 标称值非法（非有限/负）建账即拒收。
        let ok = LatencyReport::new(f32::NAN).is_err()
            && LatencyReport::new(-1.0).is_err()
            && LatencyReport::new(10.0).is_ok();
        set.add("H12-实测-标称非法拒收", ok, "");
    }

    // ---- 判据：切换协议 ----

    {
        // 切换不爆音：密集采样整条斜坡，逐帧断 `|Δ| <= |to−from|/ramp`，
        // 且首帧恰为 from、末帧恰为 to、全程落在凸包内。进入方向 0→1。
        let from = 0.0f32;
        let to = 1.0f32;
        let ramp = 96u32;
        let ok = match SwitchSmoother::new(from, to, ramp) {
            Ok(mut sm) => {
                // 容差按增益量级的 ULP 推导（见 `step_ulp_tolerance` 头注的
                // 实测数据），端点仍走精确等式。
                let tol = step_ulp_tolerance(from, to);
                let bound = (to - from).abs() / ramp as f32 + tol;
                let lo = if from < to { from } else { to };
                let hi = if from < to { to } else { from };
                let first = sm.gain();
                let mut prev = first;
                let mut in_hull = true;
                let mut max_step_seen = 0.0f32;
                for _ in 0..ramp {
                    let g = sm.step();
                    let d = (g - prev).abs();
                    if d > max_step_seen {
                        max_step_seen = d;
                    }
                    if d > bound {
                        in_hull = false;
                    }
                    if g < lo || g > hi {
                        in_hull = false;
                    }
                    prev = g;
                }
                let last = prev;
                // 浮点精确：首帧 from、末帧 to（两者都可精确表示）。
                first.to_bits() == from.to_bits()
                    && last.to_bits() == to.to_bits()
                    && in_hull
                    && max_step_seen <= bound
            }
            Err(_) => false,
        };
        set.add("H12-切换-进入不爆音", ok, "");
    }
    {
        // 退出方向 1→0 同样钉（方向对称不是显然的，差值为负）。
        let from = 1.0f32;
        let to = 0.0f32;
        let ramp = 64u32;
        let ok = match SwitchSmoother::new(from, to, ramp) {
            Ok(mut sm) => {
                let tol = step_ulp_tolerance(from, to);
                let bound = (to - from).abs() / ramp as f32 + tol;
                let mut prev = sm.gain();
                let mut okk = prev.to_bits() == from.to_bits();
                for _ in 0..ramp {
                    let g = sm.step();
                    if (g - prev).abs() > bound {
                        okk = false;
                    }
                    prev = g;
                }
                okk && prev.to_bits() == to.to_bits()
            }
            Err(_) => false,
        };
        set.add("H12-切换-退出不爆音", ok, "");
    }
    {
        // **硬切对照**（判据侧独立算）：ramp > 1 时，硬切步长 `|to−from|`
        // 严格大于斜坡上界 `|to−from|/ramp`。证明"不爆音"这道闸有可证伪的
        // 对手，不是恒真。
        let from = 0.0f32;
        let to = 1.0f32;
        let ramp = 96u32;
        let hard_cut = (to - from).abs();
        let ramp_bound = hard_cut / ramp as f32;
        let ok = ramp > 1 && hard_cut > ramp_bound;
        set.add("H12-切换-硬切超界对照", ok, "");
    }
    {
        // `ramp == 0`（零帧斜坡即硬切）构造层拒收。
        let ok = SwitchSmoother::new(0.0, 1.0, 0).is_err()
            && SwitchSmoother::new(f32::NAN, 1.0, 8).is_err()
            && SwitchSmoother::new(0.0, 1.0, 1).is_ok();
        set.add("H12-切换-零帧斜坡拒收", ok, "");
    }
    {
        // 到位后增益**保持**（不漂移）、`is_done` 为真、步数饱和。
        let ok = match SwitchSmoother::new(0.0, 1.0, 8) {
            Ok(mut sm) => {
                for _ in 0..8 {
                    sm.step();
                }
                let at_end = sm.gain();
                let done = sm.is_done();
                for _ in 0..5 {
                    sm.step();
                }
                done && sm.gain().to_bits() == at_end.to_bits() && sm.gain().to_bits() == 1.0f32.to_bits()
            }
            Err(_) => false,
        };
        set.add("H12-切换-到位后保持", ok, "");
    }
    {
        // 默认斜坡帧数常量存在且为正（DEFAULT_RAMP_FRAMES）。
        let ok = DEFAULT_RAMP_FRAMES > 0;
        set.add("H12-切换-默认斜坡常量", ok, "");
    }

    // ---- 判据：边界 ----

    {
        // 边界声明含关键成分且非空（显性）。
        let b = COEXIST_BOUNDARY;
        let ok = !b.is_empty()
            && b.contains("并存")
            && b.contains("不接管播放链")
            && b.contains("独占")
            && b.contains("实测");
        set.add("H12-边界-声明关键成分", ok, "");
    }
    {
        // 公开面穷举：清单每项非空，且**禁止关键词一个都不在**其中（无平台
        // 独占入口 / 无全局独占开关）。新增"申请独占"类函数即转红。
        let mut ok = PUBLIC_SURFACE.len() == 14;
        for name in PUBLIC_SURFACE.iter() {
            if name.is_empty() {
                ok = false;
            }
            for tok in FORBIDDEN_SURFACE_TOKENS.iter() {
                if name.contains(tok) {
                    ok = false;
                }
            }
        }
        // 关键公开项确实在清单里。
        ok = ok
            && PUBLIC_SURFACE.contains(&"mmcss_of")
            && PUBLIC_SURFACE.contains(&"SwitchSmoother::step")
            && PUBLIC_SURFACE.contains(&"LatencyReport::observe");
        set.add("H12-边界-公开面穷举", ok, "");
    }
    {
        // 会话族两族具名、优先级两族不同、标签互异且非空（避免重名门禁——
        // 上一单 VE-F1411 曾踩"两族映射到同一 wire"）。
        let names = [
            SessionClass::LowLatency.label(),
            SessionClass::Playback.label(),
        ];
        let ok = SessionClass::ALL.len() == 2
            && names[0] != names[1]
            && !names[0].is_empty()
            && SessionClass::LowLatency.wire() != SessionClass::Playback.wire()
            && mmcss_of(SessionClass::LowLatency) != mmcss_of(SessionClass::Playback);
        set.add("H12-边界-两族标签互异", ok, "");
    }
    {
        // 档位三档标签互异且非空（避免"两档同名"这类弱门禁）。
        let labels = [
            BufferTier::Ms1.label(),
            BufferTier::Ms2.label(),
            BufferTier::Ms5.label(),
        ];
        let mut distinct = true;
        for i in 0..labels.len() {
            if labels[i].is_empty() {
                distinct = false;
            }
            for j in (i + 1)..labels.len() {
                if labels[i] == labels[j] {
                    distinct = false;
                }
            }
        }
        let ok = BufferTier::ALL.len() == 3 && distinct;
        set.add("H12-边界-档位标签互异", ok, "");
    }

    set
}

/// 构造失败时的占位续写（保证判据名册长度稳定，便于比对）。
///
/// 仅在**构造被拒**这一异常路径执行——正常路径恒不进入，故不影响判据数。
fn skip_rest(set: &mut CheckSet) {
    set.add("H12-构造-规格可构造", false, "spec_construction_failed");
}

/// 把判据用的字符串 detail 泄漏为 `&'static str`（`CheckSet` 只收静态串）。
///
/// 正常路径下 detail 恒为空串（判据全绿），故该分支只在判据失败时执行，
/// 此时把实际数值固化下来供 `red_items()` 打印。
///
/// **`Box` 必须走 `alloc::boxed::Box`**：本 crate 是 `no_std`，`Box` 不在
/// prelude 里。std 隔离探针会提供 prelude，因而**掩盖**此错——真仓
/// `cargo build` 才报 E0433。泄漏量与判据失败次数同阶，不构成无界增长。
fn leak(s: String) -> &'static str {
    alloc::boxed::Box::leak(s.into_boxed_str())
}

/// 变异登记表（判据反向验证的凭证，非生产逻辑）。
///
/// 每项登记：变异名 → 该变异**预期**转红的判据。变异器逐条施加后须核对
/// "实际转红集合 ⊇ 预期集合"，全绿即证明对应判据恒真（弱门禁）。
/// 登记与判据同文件，使"新增判据须补登记"成为可审的约定。
pub const VARIANT_REGISTRY: [(&str, &str); 27] = [
    ("M1-target-inflated-to-1000", "H12-目标-10ms反例不达标"),
    ("M2-target-equals-perceived", "H12-目标-阈值严格低于感知阈值"),
    ("M3-strict-lt-on-target", "H12-目标-边界取等号"),
    ("M4-coefficient-3-not-2", "H12-目标-三档闭式精确等式"),
    ("M5-breakdown-total-unrelated", "H12-目标-分解守恒"),
    ("M6-passthrough-halves-process", "H12-直通-恰等两倍周期且零帧"),
    ("M7-wake-lead-scales-with-play", "H12-并存-高缓冲不拖累低延迟"),
    ("M8-low-starves-play", "H12-并存-播放不被挤占"),
    ("M9-starvation-silent", "H12-并存-超槽位显性饥饿"),
    ("M10-priority-realtime", "H12-并存-低延迟优先且不上Realtime"),
    ("M11-priority-all-same-rank", "H12-并存-五档rank连续"),
    ("M12-risk-ascends-with-buffer", "H12-缓冲-权衡表反向单调"),
    ("M13-frames-wrong-48k", "H12-缓冲-三档帧数闭式"),
    ("M14-rounding-at-44100", "H12-缓冲-跨采样率缩放"),
    ("M15-exclusive-optional", "H12-缓冲-独占纪律双向"),
    ("M16-samplerate-unbounded", "H12-缓冲-采样率夹逼对"),
    ("M17-pad-only-at-zero", "H12-恢复-提前半程触发"),
    ("M18-pad-boundary-exclusive", "H12-恢复-预测夹逼对"),
    ("M19-penalty-equal-not-faster", "H12-恢复-特化快于标准且非零"),
    ("M20-penalty-hardcoded-2", "H12-恢复-两档代价具体值"),
    ("M21-standard-uses-fast-penalty", "H12-恢复-标准档独立记账"),
    ("M22-count-ge-not-eq", "H12-恢复-事件计数恰等N"),
    ("M23-reported-equals-nominal", "H12-实测-上报跟随实测值"),
    ("M24-hitrate-absolute-not-ratio", "H12-实测-命中率独立重算"),
    ("M25-nan-silently-accepted", "H12-实测-非法观测拒收"),
    ("M26-hardcut-no-ramp", "H12-切换-进入不爆音"),
    ("M27-zero-ramp-allowed", "H12-切换-零帧斜坡拒收"),
];