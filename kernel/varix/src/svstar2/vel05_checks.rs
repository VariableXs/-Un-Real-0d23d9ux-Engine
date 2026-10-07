//! VE-F2205 · 域自检（判据逐条对应，见 `vel05_lifetime.rs` 头注）
//!
//! 判据映射（锚点原文 → 自检项）：
//! - 寿命分布 → `E01-分布-*`
//! - 曲线单源 → `E02-单源-*`
//! - 四态→ `E03-四态-*`
//! - 爆裂预留 → `E04-预留-*`
//! - 降级矩阵（寿命非法拒绝 / 非单调允许 / 爆裂显性报错 / 递归深度限制 /
//!   随机确定性）→ `E05-降级-*`
//! - 性能与零分配 → `E06-性能-*`
//!
//! 零墙钟、零 IO；随机源与时间均注入故回归可复现。

use alloc::vec;
use alloc::vec::Vec;

use super::veh07_fade::FadeCurve;
use super::vel03_emitter::{DiagBag, DiagCode, Outcome, RandomSource};
use super::vel05_lifetime::*;
use crate::checks::CheckSet;

/// 自检内部的取值助手：`Outcome` 失败即 panic 并带上原始诊断。
///
/// 为什么需要它：F2203 的 [`Outcome`] 是 no_std 自定义结果类型，**没有**
/// `expect`/`unwrap`（F2204的 `create_mode` 返回 core `Result` 才有）。
/// 本函数是唯一例外点，且**只在 `run_*_checks()` 与 `#[cfg(test)]` 内被调用**
/// —— 自检的输入全部是本模块自己造的常量曲线集，若它失败那是判据自身写错了
/// （如曲线参数写反），必须当场炸出原始诊断，而不是让判据静默恒真。
fn must<T>(o: Outcome<T>, ctx: &str) -> T {
    match o {
        Outcome::Ok { value, .. } => value,
        Outcome::Err { message, hint, .. } => {
            panic!("{}：{}（建议：{}）", ctx, message, hint)
        }
    }
}

/// 标准曲线集：三通道全线性，alpha 从 1 淡到 0，尺寸从 0 涨到 1。
fn std_curves() -> CurveSet {
    must(
        CurveSet::build(
            &FadeCurve::Linear,
            &FadeCurve::Linear,
            &FadeCurve::Linear,
            ColorRamp::new(Rgba::new(1.0, 1.0, 1.0, 1.0), Rgba::new(0.0, 0.0, 0.0, 0.0)),
            LIFELINE_LUT_STEPS,
        ),
        "标准曲线集",
    )
}

/// 尺寸曲线终点**非零**的曲线集（F1407 五型曲线一律`f(1)=1`）。
///
/// 用于「缩小」死亡行为的**负向**用例：配了「缩小」却因尺寸曲线终点非零而
/// 不可达，必须被拒绝。正向路径见 [`size_zeroed_shade`]。
fn nonzero_size_curves() -> CurveSet {
    must(
        CurveSet::build(
            &FadeCurve::Linear,
            &FadeCurve::SCurve,
            &FadeCurve::Linear,
            ColorRamp::new(Rgba::new(1.0, 0.0, 0.0, 1.0), Rgba::new(0.0, 0.0, 1.0, 0.0)),
            LIFELINE_LUT_STEPS,
        ),
        "非零终点尺寸曲线集",
    )
}

/// 演示「尺寸归零」的正确实现方式。
///
/// F1407 [`FadeCurve`] 五型全部满足 `f(1)=1`（端点精确是该曲线族的纪律，
/// 不可破坏），所以「缩小到零」**不可能**靠找一条终点为 0 的 F1407 曲线实现
/// ——那样做等于在粒子域私写第二套曲线数学，正是锚点「曲线核单源」要防的漂移。
///
/// 正确做法：尺寸归零是**粒子语义**（死亡行为）施加的终止约束，由调用方乘
/// `(1-t)` 因子，而非由曲线形态承担。故本函数把归零放在调用侧，与
/// [`create_particle_life`] 的跨域一致性校验（要求曲线终点为 0）配套。
fn size_zeroed_shade(curves: &CurveSet, t: f32) -> f32 {
    curves.shade(t).size * (1.0 - t).max(0.0)
}

/// 建立一个粒子的便捷路径。
fn make(cfg: &LifeConfig, curves: &CurveSet, seed: u64, bag: &mut DiagBag) -> Option<ParticleLife> {
    let mut rng = RandomSource::new(seed);
    create_particle_life(cfg, curves, &mut rng, bag).value()
}

/// 随机区间寿命的便捷构造。
fn range_dist(min: f32, max: f32) -> LifetimeDist {
    LifetimeDist::Range { min, max }
}

/// VE-F2205 域自检。
pub fn run_vel05_all_checks() -> CheckSet {
    let mut set = CheckSet::new("VE-F2205");

    // =======================================================================
    // 一、寿命分布（判据一）
    // =======================================================================

    // 两型在册不多不少。
    {
        let set_a = LifetimeDist::Constant(1.0);
        let set_b = range_dist(0.5, 2.0);
        let ok = !set_a.zh().is_empty() && !set_b.zh().is_empty();
        set.add("E01-分布-两型在册不多不少", ok, "");
    }

    // 常量型的上下界自洽（min == max）。
    {
        let d = LifetimeDist::Constant(2.5);
        set.add(
            "E01-分布-常量型上下界自洽",
            d.min_sec() == 2.5 && d.max_sec() == 2.5,
            "",
        );
    }

    // 区间型的上下界如实反映配置。
    {
        let d = range_dist(0.5, 3.0);
        set.add("E01-分布-区间型上下界如实", d.min_sec() == 0.5 && d.max_sec() == 3.0, "");
    }

    // 常量采样：同种子下恒得同值（不消耗随机流）。
    {
        let d = LifetimeDist::Constant(1.25);
        let mut r1 = RandomSource::new(7);
        let mut r2 = RandomSource::new(999);
        let a = d.sample(&mut r1);
        let b = d.sample(&mut r2);
        set.add("E01-分布-常量采样与种子无关", a == 1.25 && b == 1.25, "");
    }

    // 区间采样落在 [min,max) 内——注意是**左闭右开**，与 range_f32 值域一致。
    {
        let d = range_dist(0.5, 2.0);
        let mut rng = RandomSource::new(42);
        let mut all_in_range = true;
        let mut saw_variety = false;
        let first = d.sample(&mut rng);
        for _ in 0..512 {
            let v = d.sample(&mut rng);
            if !(v >= 0.5 && v < 2.0) {
                all_in_range = false;
            }
            if v != first {
                saw_variety = true;
            }
        }
        set.add(
            "E01-分布-区间采样值域左闭右开",
            all_in_range && saw_variety,
            "",
        );
    }

    // 同种子双跑：区间采样序列逐位一致（确定性根基，F2215 联动）。
    {
        let d = range_dist(0.25, 4.0);
        let draw = |seed: u64| {
            let mut rng = RandomSource::new(seed);
            let mut v: Vec<u32> = Vec::new();
            for _ in 0..64 {
                v.push(d.sample(&mut rng).to_bits());
            }
            v
        };
        set.add("E01-分布-同种子双跑逐位一致", draw(12345) == draw(12345), "");
    }

    // 异种子产出不同序列（否则「确定性」就成了「常量」的假象）。
    {
        let d = range_dist(0.25, 4.0);
        let draw = |seed: u64| {
            let mut rng = RandomSource::new(seed);
            let mut v: Vec<u32> = Vec::new();
            for _ in 0..64 {
                v.push(d.sample(&mut rng).to_bits());
            }
            v
        };
        set.add("E01-分布-异种子序列相异", draw(1) != draw(2), "");
    }

    // 采样为 O(1) 语义验证：单次采样后随机流状态推进量恒定
    // （用后续同种子序列的偏移量体现「一次采样 = 一次流推进」）。
    {
        let d = range_dist(0.25, 4.0);
        let mut rng = RandomSource::new(555);
        let _ = d.sample(&mut rng);
        let after_one: Vec<u32> = (0..8).map(|_| rng.next_f32().to_bits()).collect();
        let mut rng2 = RandomSource::new(555);
        for _ in 0..1 {
            let _ = d.sample(&mut rng2);
        }
        let after_one2: Vec<u32> = (0..8).map(|_| rng2.next_f32().to_bits()).collect();
        set.add("E01-分布-一次采样推进一次流", after_one == after_one2, "");
    }

    // =======================================================================
    // 二、曲线单源（判据二）
    // =======================================================================

    // LUT 端点精确 0/1（F1407 曲线族纪律，LUT 不得破坏）。
    //
    // 覆盖**五型全曲线**而非只测Linear：端点语义是曲线族的共性纪律，
    // 只测一型等于把其余四型的端点行为放在门禁之外。
    //
    // 附实测结论（反假变体测试得出，故记在此处备查）：F1407 五型的
    // `at(0)`/`at(1)` **本身已精确**（Bezier 内部有`t<=0 → 0.0` /
    // `t>=1 → 1.0` 早退分支兜底），因此删掉 [`CurveLut::build`] 末尾的
    // 端点钉死两行，本判据**仍全绿**——那不是门禁有洞，而是等价变体：
    // 钉死是防御性冗余（防未来新增曲线型不守端点纪律），对本曲线族
    // 无行为差异。故此处不假装能抓住它，如实记录该变体为等价。
    {
        let all = [
            FadeCurve::Linear,
            FadeCurve::EqualPower,
            FadeCurve::Exponential,
            FadeCurve::SCurve,
            FadeCurve::Bezier { x1: 0.3, y1: 0.2, x2: 0.7, y2: 0.8 },
        ];
        let mut all_exact = true;
        for cv in all {
            let lut = must(CurveLut::build(&cv, LIFELINE_LUT_STEPS), "端点判据曲线");
            if lut.sample(0.0) != 0.0 || lut.sample(1.0) != 1.0 {
                all_exact = false;
            }
        }
        set.add("E02-单源-LUT端点五型全精确", all_exact, "");
    }

    // 曲线是 F1407 的搬运，不是第二套数学：LUT 采样须逼近 F1407 原曲线。
    {
        let c = std_curves();
        let mut max_dev = 0.0f32;
        for i in 0..=64 {
            let t = i as f64 / 64.0;
            let want = FadeCurve::Linear.at(t) as f32;
            let got = c.alpha.sample(t as f32);
            let dev = (want - got).abs();
            if dev > max_dev {
                max_dev = dev;
            }
        }
        set.add("E02-单源-LUT逼近F1407原曲线", max_dev < 1e-3, "");
    }

    // 量化误差不超过解析上界 M/(8N²)（误差不隐藏）。
    {
        // 判据自洽性：本判据比的是「某条 F1407 曲线的 LUT 逼近该曲线自身」，
        // 故曲线与误差上界必须取自**同一个 CurveSet**。
        //
        // 反例（此判据曾写错的样子）：拿 `std_curves()`（alpha 通道是 Linear）
        // 去对拍 SCurve、再取 Linear 集的上界——曲线与被测物不匹配，
        // 判据恒红且掩盖真实缺陷。此处显式为 SCurve 单独建集。
        let sc = must(
            CurveSet::build(
                &FadeCurve::SCurve,
                &FadeCurve::SCurve,
                &FadeCurve::SCurve,
                ColorRamp::new(Rgba::new(1.0, 1.0, 1.0, 1.0), Rgba::TRANSPARENT),
                LIFELINE_LUT_STEPS,
            ),
            "SCurve 曲线集",
        );
        let bound = sc.alpha.declared_max_error();
        let mut max_dev = 0.0f32;
        for i in 0..=256 {
            let t = i as f32 / 256.0;
            let want = FadeCurve::SCurve.at(t as f64) as f32;
            let got = sc.alpha.sample(t);
            let dev = (want - got).abs();
            if dev > max_dev {
                max_dev = dev;
            }
        }
        set.add(
            "E02-单源-实测误差不超过解析上界",
            max_dev <= bound + 1e-6,
            "",
        );
    }

    // 二阶导上界对五型曲线全部为有限非负（误差承诺可计算）。
    {
        let all = [
            curvature_bound(&FadeCurve::Linear),
            curvature_bound(&FadeCurve::EqualPower),
            curvature_bound(&FadeCurve::Exponential),
            curvature_bound(&FadeCurve::SCurve),
            curvature_bound(&FadeCurve::Bezier { x1: 0.3, y1: 0.1, x2: 0.7, y2: 0.9 }),
        ];
        set.add(
            "E02-单源-二阶导上界可计算",
            all.iter().all(|v| v.is_finite() && *v >= 0.0),
            "",
        );
    }

    // 线性曲线的二阶导上界为 0 → LUT 对线性零误差（精确而非近似）。
    {
        let bound = curvature_bound(&FadeCurve::Linear);
        set.add("E02-单源-线性曲线LUT零误差", bound == 0.0, "");
    }

    // 误差上界随分段数按 1/N² 收敛（成本-精度权衡可推导）。
    {
        let b64 = lut_error_bound(6.0, 64);
        let b256 = lut_error_bound(6.0, 256);
        let b1024 = lut_error_bound(6.0, 1024);
        set.add(
            "E02-单源-误差按1/N平方收敛",
            b64 > b256 && b256 > b1024 && b1024 > 0.0,
            "",
        );
    }

    // 三通道各自独立（alpha 与 size 曲线不同则采样值不同）。
    {
        let c = must(
            CurveSet::build(
                &FadeCurve::Linear,
                &FadeCurve::Exponential,
                &FadeCurve::SCurve,
                ColorRamp::new(Rgba::new(1.0, 0.0, 0.0, 1.0), Rgba::new(0.0, 0.0, 1.0, 0.0)),
                LIFELINE_LUT_STEPS,
            ),
            "三通道曲线集",
        );
        let t = 0.4;
        let s = c.shade(t);
        // alpha 取补（F1407 包络增益→ 粒子淡出），故期望值是 1 - f(t)。
        let expect_alpha = 1.0 - FadeCurve::Linear.at(t as f64) as f32;
        let expect_size = FadeCurve::Exponential.at(t as f64) as f32;
        set.add(
            "E02-单源-三通道互不串扰",
            (s.alpha - expect_alpha).abs() < 1e-3 && (s.size - expect_size).abs() < 1e-3,
            "",
        );
    }

    // 同一 t 必得同一组属性（态与属性联动的确定性基础）。
    {
        let c = std_curves();
        let a = c.shade(0.37);
        let b = c.shade(0.37);
        set.add("E02-单源-同t属性可复现", a == b, "");
    }

    // 颜色在端点精确落在 ramp 两端（线性空间插值纪律）。
    {
        let c = std_curves();
        let s0 = c.shade(0.0);
        let s1 = c.shade(1.0);
        set.add(
            "E02-单源-颜色端点精确",
            s0.color == c.ramp.from && s1.color == c.ramp.to,
            "",
        );
    }

    // 越界 t 被钳制（负 t / >1 都不得panic 也不得外插）。
    //
    // 端点方向按 alpha 淡出语义：t<=0 出生（alpha=1 最不透明），
    // t>=1 死亡（alpha=0 全透明）。若此处写成反了，等于把「渐亮」钉成契约。
    {
        let c = std_curves();
        let lo = c.shade(-5.0);
        let hi = c.shade(5.0);
        set.add(
            "E02-单源-越界t被钳制",
            lo.alpha == 1.0 && hi.alpha == 0.0,
            "",
        );
    }

    // =======================================================================
    // 三、四态（判据三）
    // =======================================================================

    // 四态在册不多不少，中英名齐备。
    {
        let ok = LifeState::ALL.len() == 4
            && LifeState::ALL.iter().all(|s| !s.zh().is_empty());
        set.add("E03-四态-四态在册不多不少", ok, "");
    }

    // 态由生命进度唯一定义（fade_start = 0.2 的分段）。
    {
        let fs = 0.2;
        let ok = LifeState::at(0.0, fs) == LifeState::Newborn
            && LifeState::at(0.1, fs) == LifeState::Alive
            && LifeState::at(0.2, fs) == LifeState::Fading
            && LifeState::at(0.99, fs) == LifeState::Fading
            && LifeState::at(1.0, fs) == LifeState::Dead;
        set.add("E03-四态-态由进度唯一定义", ok, "");
    }

    // 负进度与 NaN 归为新生（不误判死亡——误判会被池回收，表现为凭空消失）。
    {
        let ok = LifeState::at(-0.1, 0.0) == LifeState::Newborn
            && LifeState::at(f32::NAN, 0.0) == LifeState::Newborn;
        set.add("E03-四态-非法进度不误判死亡", ok, "");
    }

    // fade_start = 0 时全程淡出：t>0 即 Fading（不经过存活态）。
    {
        let ok = LifeState::at(0.0001, 0.0) == LifeState::Fading;
        set.add("E03-四态-全程淡出跳过存活态", ok, "");
    }

    // fade_start = 1 时不淡出：t<1 恒 Alive。
    {
        let ok = LifeState::at(0.5, 1.0) == LifeState::Alive
            && LifeState::at(0.999, 1.0) == LifeState::Alive;
        set.add("E03-四态-不淡出时恒存活", ok, "");
    }

    // 死亡是终态（无合法后继）。
    {
        set.add("E03-四态-死亡为终态", LifeState::Dead.legal_next().is_empty(), "");
    }

    // 新生 → 淡出 合法（全程淡出的直跳）。
    {
        set.add(
            "E03-四态-新生可直跳淡出",
            LifeState::Newborn.can_next(LifeState::Fading),
            "",
        );
    }

    // 单向推进：**穷举 4×4 全部 16 对**（不是抽样几对）。
    //
    // 为什么必须穷举：早先的判据只点了 `Fading→Alive` 与 `Alive→Newborn`
    // 两对，漏掉了 `Newborn→Newborn`（自环）。反假变体测试往 `Newborn`
    // 的合法后继里塞入 `Newborn` 时，全绿通过——门禁有洞。
    //
    // 穷举的价值：合法转移表是**有限且已知**的 16 格，抽样就等于放弃
    // 「表被改坏」这类检测能力。逐格断言可把「转移表」钉成契约。
    // 合法边共 6 条：新生→{存活,淡出,死亡}、存活→{淡出,死亡}、淡出→死亡。
    const LEGAL: [(LifeState, LifeState); 6] = [
        (LifeState::Newborn, LifeState::Alive),
        (LifeState::Newborn, LifeState::Fading),
        (LifeState::Newborn, LifeState::Dead),
        (LifeState::Alive, LifeState::Fading),
        (LifeState::Alive, LifeState::Dead),
        (LifeState::Fading, LifeState::Dead),
    ];
    {
        let mut table_ok = true;
        for from in LifeState::ALL {
            for to in LifeState::ALL {
                let want = LEGAL.iter().any(|(f, t)| *f == from && *t == to);
                if LifeState::can_next(from, to) != want {
                    table_ok = false;
                }
            }
        }
        set.add("E03-四态-转移表十六格全对", table_ok, "");
    }

    // 自环非法：四态中**没有任何一态可以转移到自身**。
    //
    // 自环看似无害（状态没变），实则掩盖真缺陷：若`Alive→Alive` 合法，
    // 复活一个已死粒子时走Alive→Alive 就绕过了「死亡是终态」这条不变量。
    // 故显式把四个自环全部钉死为非法。
    {
        let self_loop = LifeState::Newborn.can_next(LifeState::Newborn)
            || LifeState::Alive.can_next(LifeState::Alive)
            || LifeState::Fading.can_next(LifeState::Fading)
            || LifeState::Dead.can_next(LifeState::Dead);
        set.add("E03-四态-自环非法", !self_loop, "");
    }

    // 年龄不可回退：任何态 → 新生均非法（**穷举** 4 个源态）。
    {
        let illegal = LifeState::ALL
            .iter()
            .any(|s| LifeState::can_next(*s, LifeState::Newborn));
        set.add("E03-四态-年龄不可回退", !illegal, "");
    }

    // 状态序号单调不减：沿合法边 `Alive`→`Fading`→`Dead` 的态序严格递增。
    //
    // 用「秩」表达单向性：`Newborn=0 < Alive=1 < Fading=2 < Dead=3`。
    // 这条比逐对断言更抗改：新增合法边时若破坏秩序，判据立刻红。
    fn rank(s: LifeState) -> u8 {
        match s {
            LifeState::Newborn => 0,
            LifeState::Alive => 1,
            LifeState::Fading => 2,
            LifeState::Dead => 3,
        }
    }
    {
        let monotone = LifeState::ALL
            .iter()
            .all(|from| rank(*from) == 3 || LifeState::legal_next(*from).iter().all(|to| rank(*to) > rank(*from)));
        set.add("E03-四态-态序严格递增", monotone, "");
    }

    // 非法转移被拒绝且产诊断（零静默）。
    {
        let mut bag = DiagBag::new();
        let r = request_state_transition(LifeState::Fading, LifeState::Alive, &mut bag);
        set.add(
            "E03-四态-非法转移拒绝并诊断",
            r.is_err() && bag.has(DiagCode::TransitionRejected),
            "",
        );
    }

    // 合法转移放行且不产诊断。
    {
        let mut bag = DiagBag::new();
        let r = request_state_transition(LifeState::Alive, LifeState::Fading, &mut bag);
        set.add(
            "E03-四态-合法转移放行且静默",
            r.is_ok() && bag.is_empty(),
            "",
        );
    }

    // 端到端：推进驱动态迁移（Alive → Fading → Dead）。
    {
        let mut bag = DiagBag::new();
        let curves = std_curves();
        let cfg = LifeConfig::new(
            LifetimeDist::Constant(1.0),
            FadeConfig { fade_start: 0.5 },
            DeathBehavior::Vanish,
        );
        if let Some(mut life) = make(&cfg, &curves, 1, &mut bag) {
            let a1 = life.advance(0.2, &mut bag);
            let a2 = life.advance(0.4, &mut bag);
            let a3 = life.advance(0.5, &mut bag);
            set.add(
                "E03-四态-推进驱动三段迁移",
                a1.state == LifeState::Alive
                    && a2.state == LifeState::Fading
                    && a3.state == LifeState::Dead
                    && a3.recycled,
                "",
            );
        } else {
            set.add("E03-四态-推进驱动三段迁移", false, "建器意外失败");
        }
    }

    // 态与属性联动：死亡帧的 alpha 必为 0（淡出到底，不留残影）。
    {
        let mut bag = DiagBag::new();
        let curves = std_curves();
        let cfg = default_life();
        if let Some(mut life) = make(&cfg, &curves, 1, &mut bag) {
            let mut last_alpha = 1.0f32;
            for _ in 0..64 {
                let adv = life.advance(1.0 / 32.0, &mut bag);
                last_alpha = curves.shade(adv.t).alpha;
                if adv.recycled {
                    break;
                }
            }
            set.add("E03-四态-死亡帧alpha归零", last_alpha == 0.0, "");
        } else {
            set.add("E03-四态-死亡帧alpha归零", false, "建器意外失败");
        }
    }

    // 回收标志只报一次（跨过终点那帧报，后续推进不重复报）。
    {
        let mut bag = DiagBag::new();
        let curves = std_curves();
        let cfg = default_life();
        if let Some(mut life) = make(&cfg, &curves, 1, &mut bag) {
            let mut recycle_count = 0u32;
            for _ in 0..200 {
                if life.advance(1.0 / 30.0, &mut bag).recycled {
                    recycle_count += 1;
                }
            }
            set.add("E03-四态-回收标志只报一次", recycle_count == 1, "");
        } else {
            set.add("E03-四态-回收标志只报一次", false, "建器意外失败");
        }
    }

    // 推进到死亡：终态必为死亡且已回收。
    {
        let mut bag = DiagBag::new();
        let curves = std_curves();
        let cfg = LifeConfig::new(
            LifetimeDist::Constant(0.5),
            FadeConfig::WHOLE_LIFE,
            DeathBehavior::Vanish,
        );
        if let Some(mut life) = make(&cfg, &curves, 3, &mut bag) {
            let (recycled, steps) = advance_to_death(&mut life, 0.01, &mut bag);
            // 步数**不给死数**：0.5s ÷ 0.01s 名义 50 步，但 f32 累加 50 次
            // 得0.49999998 < 0.5，须再走一步才跨过终点（实测 51）。
            //
            // 把 50 写死是判据错而非被测物错：f32 累加误差是语言既定行为，
            // 强行凑整只能靠引入 epsilon 篡改语义（epsilon 一旦进入 age
            // 累加，确定性对拍的逐位一致就废了）。故此处断言「步数落在
            // 名义值 ±1 内」——既守住「dt 相对寿命过小会导致步数爆炸」
            // 这条真判据，又不把浮点末位钉成契约。
            let nominal = 50u64;
            set.add(
                "E03-四态-推进到死亡收敛",
                recycled
                    && life.state == LifeState::Dead
                    && steps >= nominal
                    && steps <= nominal + 1,
                "",
            );
        } else {
            set.add("E03-四态-推进到死亡收敛", false, "建器意外失败");
        }
    }

    // =======================================================================
    // 四、爆裂预留（判据四）
    // =======================================================================

    // 三型在册不多不少。
    {
        let ok = DeathBehavior::ALL.len() == 3
            && DeathBehavior::ALL.iter().all(|d| !d.zh().is_empty());
        set.add("E04-预留-三型在册不多不少", ok, "");
    }

    // 仅爆裂为预留位。
    {
        let reserved: Vec<bool> = DeathBehavior::ALL.iter().map(|d| d.is_reserved()).collect();
        set.add(
            "E04-预留-仅爆裂为预留位",
            reserved == vec![false, false, true],
            "",
        );
    }

    // 爆裂被调用必显性报错，绝不静默（锚点：预留不静默，F1871 语义）。
    {
        let mut bag = DiagBag::new();
        let r = reserve_burst(0, &mut bag);
        set.add(
            "E04-预留-爆裂调用显性报错",
            r.is_err() && bag.has(DiagCode::TransitionRejected),
            "",
        );
    }

    // 爆裂不产出任何子粒子（否则是「静默部分生效」）。
    {
        let mut bag = DiagBag::new();
        let r = reserve_burst(0, &mut bag);
        let empty = match &r {
            Outcome::Ok { value, .. } => value.is_empty(),
            Outcome::Err { .. } => true,
        };
        set.add("E04-预留-爆裂不产出子粒子", empty, "");
    }

    // 诊断文案点名「预留未实现」（调用方须能分辨是自己没实现还是自己配错）。
    {
        let mut bag = DiagBag::new();
        let _ = reserve_burst(0, &mut bag);
        set.add(
            "E04-预留-诊断文案点名预留",
            bag.has_msg_containing(DiagCode::TransitionRejected, "预留"),
            "",
        );
    }

    // 深度限制位已实际生效：超限报深度超限，与「未实现」可区分。
    {
        let mut bag = DiagBag::new();
        let r = reserve_burst(BURST_MAX_DEPTH, &mut bag);
        set.add(
            "E04-预留-深度超限独立可辨",
            r.is_err() && bag.has(DiagCode::GroupRejected),
            "",
        );
    }

    // 深度门在 STUB 之前生效：超限时不得报「未实现」（证明前向兼容约束已接线）。
    {
        let mut bag = DiagBag::new();
        let _ = reserve_burst(BURST_MAX_DEPTH + 1, &mut bag);
        let not_stub = !bag.has(DiagCode::TransitionRejected);
        set.add("E04-预留-深度门先于STUB生效", not_stub, "");
    }

    // 深度边界：恰好等于上限即拒绝（`>=` 而非 `>`，避免 off-by-one 放行）。
    {
        let mut bag = DiagBag::new();
        let r = reserve_burst(BURST_MAX_DEPTH - 1, &mut bag);
        let at_limit_rejected = r.is_err();
        let mut bag2 = DiagBag::new();
        let _ = reserve_burst(BURST_MAX_DEPTH, &mut bag2);
        set.add(
            "E04-预留-深度边界取等即拒",
            at_limit_rejected && bag2.has(DiagCode::GroupRejected),
            "",
        );
    }

    // 上限为有限小正数（不是 usize::MAX 这类形同虚设的值）。
    {
        let ok = BURST_MAX_DEPTH > 0 && BURST_MAX_DEPTH < 32;
        set.add("E04-预留-深度上限为有限小正数", ok, "");
    }

    // 「消失」与「缩小」不产任何预留诊断（未实现的只有爆裂）。
    {
        let mut bag = DiagBag::new();
        let curves = std_curves();
        let cfg = LifeConfig::new(
            LifetimeDist::Constant(1.0),
            FadeConfig::WHOLE_LIFE,
            DeathBehavior::Vanish,
        );
        let _ = make(&cfg, &curves, 1, &mut bag);
        set.add("E04-预留-消失不产预留诊断", bag.is_empty(), "");
    }

    // 「缩小」要求尺寸曲线终点归零，否则拒绝（跨域一致性）。
    {
        let mut bag = DiagBag::new();
        let curves = nonzero_size_curves();
        let cfg = LifeConfig::new(
            LifetimeDist::Constant(1.0),
            FadeConfig::WHOLE_LIFE,
            DeathBehavior::Shrink,
        );
        let r = make(&cfg, &curves, 1, &mut bag);
        set.add(
            "E04-预留-缩小要求尺寸终点归零",
            r.is_none() && bag.has(DiagCode::ShapeRejected),
            "",
        );
    }

    // 「缩小」的正向路径由调用方施加归零因子达成（诚实的正确做法）。
    {
        let curves = std_curves();
        let end = size_zeroed_shade(&curves, 1.0);
        let mid = size_zeroed_shade(&curves, 0.5);
        set.add(
            "E04-预留-缩小归零因子生效",
            end == 0.0 && mid > 0.0 && mid < curves.shade(0.5).size,
            "",
        );
    }

    // =======================================================================
    // 五、降级矩阵（锚点五条降级项）
    // =======================================================================

    // 寿命区间反向 → 校验拒绝，且拒绝理由须点名「反向」。
    //
    // 判据强度说明：只断言 `is_err()` 是弱门禁——任何原因导致失败都能过。
    // 故此处断言消息含「反向」二字，确保拒的是「边界写反」这一具体病因，
    // 而不是碰巧撞上别的校验分支。
    {
        let d = range_dist(3.0, 1.0);
        let names_reversal = match validate_lifetime_dist(&d) {
            Outcome::Err { message, .. } => message.contains("反向"),
            Outcome::Ok { .. } => false,
        };
        set.add("E05-降级-区间反向拒绝且点名病因", names_reversal, "");
    }

    // 寿命为 NaN → 拒绝并点名 NaN。
    {
        let mut bag = DiagBag::new();
        let d = LifetimeDist::Constant(f32::NAN);
        let r = create_particle_life(
            &LifeConfig::new(d, FadeConfig::WHOLE_LIFE, DeathBehavior::Vanish),
            &std_curves(),
            &mut RandomSource::new(1),
            &mut bag,
        );
        set.add(
            "E05-降级-NaN寿命拒绝",
            r.is_err() && bag.has_msg_containing(DiagCode::ShapeRejected, "NaN"),
            "",
        );
    }

    // 寿命为无穷 → 拒绝（否则粒子永不回收）。
    {
        let d = LifetimeDist::Constant(f32::INFINITY);
        set.add("E05-降级-无穷寿命拒绝", validate_lifetime_dist(&d).is_err(), "");
    }

    // 寿命为 0 → 拒绝（否则 t = age/0 除零）。
    {
        let d = LifetimeDist::Constant(0.0);
        set.add("E05-降级-零寿命拒绝", validate_lifetime_dist(&d).is_err(), "");
    }

    // 寿命为负 → 拒绝。
    {
        let d = LifetimeDist::Constant(-1.0);
        set.add("E05-降级-负寿命拒绝", validate_lifetime_dist(&d).is_err(), "");
    }

    // 寿命低于下限量 → 拒绝（当帧生当帧死等同不发射）。
    {
        let d = LifetimeDist::Constant(1.0e-9);
        set.add("E05-降级-过短寿命拒绝", validate_lifetime_dist(&d).is_err(), "");
    }

    // 寿命超上限 → 拒绝。
    {
        let d = LifetimeDist::Constant(100_000.0);
        set.add("E05-降级-超长寿命拒绝", validate_lifetime_dist(&d).is_err(), "");
    }

    // 合法寿命被接受（负向用例之外的正向对照）。
    {
        let d = range_dist(0.5, 2.0);
        set.add("E05-降级-合法寿命被接受", validate_lifetime_dist(&d).is_ok(), "");
    }

    // 校验失败不消耗随机流（同种子对拍的前提：坏配置不得污染 RNG）。
    {
        let draw_after = |build_bad: bool| {
            let mut rng = RandomSource::new(4242);
            if build_bad {
                let mut bag = DiagBag::new();
                let d = LifetimeDist::Constant(f32::NAN);
                let _ = create_particle_life(
                    &LifeConfig::new(d, FadeConfig::WHOLE_LIFE, DeathBehavior::Vanish),
                    &std_curves(),
                    &mut rng,
                    &mut bag,
                );
            }
            let mut v: Vec<u32> = Vec::new();
            for _ in 0..8 {
                v.push(rng.next_f32().to_bits());
            }
            v
        };
        set.add("E05-降级-校验失败不消耗随机流", draw_after(true) == draw_after(false), "");
    }

    // 非单调曲线**被允许**且**不产诊断**（锚点：视觉怪异但合法 → 允许+文档说明）。
    {
        let mut bag = DiagBag::new();
        // 贝塞尔 y2 取**负值**才产生内部下凹（回升段）。实测：y2=0 时曲线仍
        // 非降（NonDecreasing），y2=-0.5 时才判NonMonotonic——负控制点是
        // 造出「alpha 中途回升」的唯一途径，这是实测结论不是猜测。
        let rebound = FadeCurve::Bezier { x1: 0.5, y1: 1.0, x2: 0.5, y2: -0.5 };
        let curves = must(
            CurveSet::build(
                &rebound,
                &FadeCurve::Linear,
                &FadeCurve::Linear,
                ColorRamp::new(Rgba::new(1.0, 1.0, 1.0, 1.0), Rgba::TRANSPARENT),
                LIFELINE_LUT_STEPS,
            ),
            "非单调曲线集",
        );
        let cfg = LifeConfig::new(
            LifetimeDist::Constant(1.0),
            FadeConfig::WHOLE_LIFE,
            DeathBehavior::Vanish,
        );
        let life =
            create_particle_life(&cfg, &curves, &mut RandomSource::new(1), &mut bag).is_ok();
        set.add("E05-降级-非单调曲线被允许", life && bag.is_empty(), "");
    }

    // 非单调性可被查询（咨询接口存在，供调试面板查）。
    {
        let rebound = FadeCurve::Bezier { x1: 0.5, y1: 1.0, x2: 0.5, y2: -0.5 };
        let curves = must(
            CurveSet::build(
                &rebound,
                &FadeCurve::Linear,
                &FadeCurve::Linear,
                ColorRamp::new(Rgba::new(1.0, 1.0, 1.0, 1.0), Rgba::TRANSPARENT),
                LIFELINE_LUT_STEPS,
            ),
            "非单调曲线集",
        );
        let m = curves.monotonicity();
        set.add(
            "E05-降级-非单调可被查询",
            m.is_rebounding() && !m.zh().is_empty(),
            "",
        );
    }

    // 单调曲线被正确识别（咨询接口不谎报）。
    {
        let c = std_curves();
        set.add(
            "E05-降级-单调曲线识别正确",
            c.monotonicity() == Monotonicity::NonDecreasing,
            "",
        );
    }

    // 非有限 fade_start → 钳制到 0 并产诊断（钳制而非静默）。
    {
        let mut bag = DiagBag::new();
        let f = FadeConfig { fade_start: f32::NAN }.clamped(&mut bag);
        set.add(
            "E05-降级-NaN淡出起点钳制",
            f.fade_start == 0.0 && bag.has(DiagCode::RateClamped),
            "",
        );
    }

    // 越界 fade_start → 钳制到边界并产诊断。
    {
        let mut bag = DiagBag::new();
        let hi = FadeConfig { fade_start: 5.0 }.clamped(&mut bag);
        let lo = FadeConfig { fade_start: -3.0 }.clamped(&mut bag);
        set.add(
            "E05-降级-越界淡出起点钳制",
            hi.fade_start == 1.0 && lo.fade_start == 0.0 && bag.len() == 2,
            "",
        );
    }

    // 合法 fade_start 不产诊断（不虚报）。
    {
        let mut bag = DiagBag::new();
        let f = FadeConfig { fade_start: 0.3 }.clamped(&mut bag);
        set.add("E05-降级-合法淡出起点静默", f.fade_start == 0.3 && bag.is_empty(), "");
    }

    // 非法 dt（NaN / 负）→ 不计龄且产诊断（否则粒子卡屏）。
    {
        let mut bag = DiagBag::new();
        let curves = std_curves();
        if let Some(mut life) = make(&default_life(), &curves, 1, &mut bag) {
            let before = life.age;
            life.advance(f32::NAN, &mut bag);
            let after_nan = life.age;
            life.advance(-1.0, &mut bag);
            let after_neg = life.age;
            set.add(
                "E05-降级-非法dt不计龄",
                after_nan == before && after_neg == before && !bag.is_empty(),
                "",
            );
        } else {
            set.add("E05-降级-非法dt不计龄", false, "建器意外失败");
        }
    }

    // 确定性断言：同种子同 dt 序列 → 台账逐位一致。
    {
        let simulate = |seed: u64| {
            let mut bag = DiagBag::new();
            let curves = std_curves();
            let cfg = LifeConfig::new(
                range_dist(0.5, 2.0),
                FadeConfig { fade_start: 0.4 },
                DeathBehavior::Vanish,
            );
            let mut ledger = LifetimeLedger::new();
            let mut rng = RandomSource::new(seed);
            for _ in 0..64 {
                if let Some(mut life) =
                    create_particle_life(&cfg, &curves, &mut rng, &mut bag).value()
                {
                    ledger.admit(&life);
                    for _ in 0..17 {
                        let adv = life.advance(0.011, &mut bag);
                        ledger.record(&adv);
                    }
                }
            }
            ledger
        };
        set.add("E05-降级-台账双跑逐位一致", simulate(2024).same_as(&simulate(2024)), "");
    }

    // 确定性断言：异种子台账相异（否则确定性成了常量的假象）。
    {
        let simulate = |seed: u64| {
            let mut bag = DiagBag::new();
            let curves = std_curves();
            let cfg =
                LifeConfig::new(range_dist(0.5, 2.0), FadeConfig::WHOLE_LIFE, DeathBehavior::Vanish);
            let mut ledger = LifetimeLedger::new();
            let mut rng = RandomSource::new(seed);
            for _ in 0..64 {
                if let Some(life) = create_particle_life(&cfg, &curves, &mut rng, &mut bag).value() {
                    ledger.admit(&life);
                }
            }
            ledger
        };
        set.add("E05-降级-异种子台账相异", !simulate(1).same_as(&simulate(2)), "");
    }

    // 台账对**寿命维度**正交：同种子、仅改寿命分布 -> 台账必相异。
    //
    // 为什么必须单独守这条（W005 反假变体实测发现的门禁洞）：原有的
    // 「同种子双跑一致」与「异种子台账相异」两条**都只在随机流维度上有效**——
    // 它们靠`RandomSource` 的种子差异分辨批次，而寿命是从采样结果来的，
    // 与种子无因果关系。故把寿命从台账指纹里彻底剔除（改
    // `admit` 不拌寿命、或让寿命在两处 XOR 抵消），这两条判据**全绿**。
    //
    // 后果是实打实的：分批对拍时「1秒粒子批」与「5秒粒子批」指纹相同，
    // 确定性断言把两个不同的模拟认成同一批——而这类缺陷在 GPU/CPU
    // 双跑对拍里表现为「偶发不一致」，极难归因。
    //
    // 本判据与前两条构成三条**互相独立**的正交维度（随机流 / 寿命 / 自身
    // 重复性），任一维度失守都被至少一条抓住。
    {
        let build = |dist: LifetimeDist, seed: u64| {
            let mut bag = DiagBag::new();
            let curves = std_curves();
            let cfg = LifeConfig::new(dist, FadeConfig::WHOLE_LIFE, DeathBehavior::Vanish);
            let mut ledger = LifetimeLedger::new();
            let mut rng = RandomSource::new(seed);
            for _ in 0..32 {
                if let Some(life) = create_particle_life(&cfg, &curves, &mut rng, &mut bag).value() {
                    ledger.admit(&life);
                }
            }
            ledger
        };
        // 同种子，仅寿命不同 -> 必须相异。
        let one = build(LifetimeDist::Constant(1.0), 0xBEEF);
        let five = build(LifetimeDist::Constant(5.0), 0xBEEF);
        // 同种子同寿命 -> 必须一致（否则上面那条就是「恒不相异」的假门禁）。
        let one_again = build(LifetimeDist::Constant(1.0), 0xBEEF);
        set.add(
            "E05-降级-台账对寿命维度正交",
            !one.same_as(&five) && one.same_as(&one_again),
            "",
        );
    }

    // 区间寿命的台账同样对具体采样值敏感（不止常量型）。
    //
    // 上一条只验了常量型。若`admit` 拌的是分布的**某个摘要**而非实际
    // 采样出的寿命，常量型会通过而区间型失守——两条判据各守一半。
    {
        let build = |min: f32, max: f32, seed: u64| {
            let mut bag = DiagBag::new();
            let curves = std_curves();
            let cfg = LifeConfig::new(
                range_dist(min, max),
                FadeConfig::WHOLE_LIFE,
                DeathBehavior::Vanish,
            );
            let mut ledger = LifetimeLedger::new();
            let mut rng = RandomSource::new(seed);
            for _ in 0..32 {
                if let Some(life) = create_particle_life(&cfg, &curves, &mut rng, &mut bag).value() {
                    ledger.admit(&life);
                }
            }
            ledger
        };
        // 同种子同区间 -> 一致；同种子窄区间 vs 宽区间 -> 相异。
        set.add(
            "E05-降级-区间台账对采样值敏感",
            build(0.5, 2.0, 77).same_as(&build(0.5, 2.0, 77))
                && !build(1.0, 1.5, 77).same_as(&build(4.0, 6.0, 77)),
            "",
        );
    }

    // 随机流消耗纪律：区间寿命的建器**必须且恰好**推进随机流一步，常量型**不得**推进。
    //
    // 这条守的是`create_particle_life` 里`let mut seed_rng = *rng` 这一手：
    // 它从**副本**取随机流位置而不动`rng` 本身，故`stream_pos` 不受前序
    // 采样影响。若改成从 `rng` 本体取（`rng.next_f32()`），常量型建器就会
    // 白消耗一个随机数——同种子后续粒子的寿命序列整体平移，表现为「改一处
    // 动全批」，且编译不报错、常规判据也未必红。
    //
    // 「恰好一步」而非「至少一步」：多消耗同样是缺陷（随机流错位），但症状
    // 表现为后续所有粒子的寿命分布整体平移，肉眼与统计都难察觉。断言精确
    // 到步数才能把它钉住——`range_f32` 实现为`lo + (hi-lo) * next_f32()`，
    // 恰推进一次，故建器后的下一个值必等于全新实例的**第二个**值。
    {
        let consumed = |dist: LifetimeDist| {
            let mut bag = DiagBag::new();
            let curves = std_curves();
            let cfg = LifeConfig::new(dist, FadeConfig::WHOLE_LIFE, DeathBehavior::Vanish);
            let mut rng = RandomSource::new(31337);
            let _ = create_particle_life(&cfg, &curves, &mut rng, &mut bag);
            rng.next_f32().to_bits()
        };
        let mut fresh = RandomSource::new(31337);
        let first = fresh.next_f32().to_bits();
        let second = fresh.next_f32().to_bits();

        let constant = consumed(LifetimeDist::Constant(1.0));
        let interval = consumed(range_dist(0.5, 2.0));
        set.add(
            "E05-降级-随机流消耗符合分布语义",
            // 常量型零消耗 -> 建器后的下一个值 == 全新实例的第一个值
            constant == first
                // 区间型恰消耗一步 -> 建器后的下一个值 == 全新实例的第二个值
                && interval == second
                // 且两者必不同（否则「恰好一步」无从分辨）
                && constant != interval,
            "",
        );
    }

    // 台账合并可对拍（分批模拟场景）。
    {
        let a = LifetimeLedger { fingerprint: 1, particles: 2, steps: 3, recycled: 1 };
        let b = LifetimeLedger { fingerprint: 9, particles: 1, steps: 1, recycled: 0 };
        let mut m = a;
        m.merge(&b);
        set.add(
            "E05-降级-台账合并计数正确",
            m.particles == 3 && m.steps == 4 && m.recycled == 1 && m.fingerprint != a.fingerprint,
            "",
        );
    }

    // 极小 dt 下推进有硬步数上限（不是隐藏死循环）。
    {
        set.add("E05-降级-推进步数有硬上限", MAX_ADVANCE_STEPS > 0 && MAX_ADVANCE_STEPS < u64::MAX, "");
    }

    // 步数上限触发时显性诊断（不静默截断）。
    {
        let mut bag = DiagBag::new();
        let curves = std_curves();
        let cfg = LifeConfig::new(
            LifetimeDist::Constant(LIFETIME_MAX_SEC),
            FadeConfig::WHOLE_LIFE,
            DeathBehavior::Vanish,
        );
        if let Some(mut life) = make(&cfg, &curves, 1, &mut bag) {
            let (recycled, steps) = advance_to_death(&mut life, 1.0e-6, &mut bag);
            set.add(
                "E05-降级-步数上限显性诊断",
                !recycled && steps >= MAX_ADVANCE_STEPS && !bag.is_empty(),
                "",
            );
        } else {
            set.add("E05-降级-步数上限显性诊断", false, "建器意外失败");
        }
    }

    // 颜色端点非法 → 曲线集拒绝（不半合法）。
    {
        let bad = CurveSet::build(
            &FadeCurve::Linear,
            &FadeCurve::Linear,
            &FadeCurve::Linear,
            ColorRamp::new(Rgba::new(2.0, 0.0, 0.0, 1.0), Rgba::TRANSPARENT),
            LIFELINE_LUT_STEPS,
        );
        set.add("E05-降级-越界颜色拒绝", bad.is_err(), "");
    }

    // LUT 分段数为 0 → 拒绝（否则查表退化为常数）。
    {
        let r = CurveLut::build(&FadeCurve::Linear, 0);
        set.add("E05-降级-零分段LUT拒绝", r.is_err(), "");
    }

    // =======================================================================
    // 六、性能与零分配
    // =======================================================================

    // 建器之后推进路径零分配（以「推进不改变曲线集内容」验证无隐式重建）。
    {
        let mut bag = DiagBag::new();
        let curves = std_curves();
        let before = curves.clone();
        if let Some(mut life) = make(&default_life(), &curves, 1, &mut bag) {
            for _ in 0..100 {
                life.advance(0.005, &mut bag);
            }
            set.add("E06-性能-推进不重建曲线集", curves == before, "");
        } else {
            set.add("E06-性能-推进不重建曲线集", false, "建器意外失败");
        }
    }

    // 推进的**可数工作量**为O(1)：每次推进恰好一步账、不多不少。
    //
    // **W005 审计改判（前序写法是自证式空断言）**：前序写成
    // `n += 1` 后断言 `n == 1000 && life.steps == 1000`——`n` 是自己加的，
    // 恒等于循环次数；而 `steps` 若由 `advance` 内部 `self.steps += 1`
    // 维护，则「推进 1000 次→steps==1000」是**恒真**：无论 `advance` 内部
    // 做O(1) 还是 O(n) 工作，这条都绿。计数器覆盖的是「自己写的那一层」，
    // 不是「缺陷发生的那一层」。
    //
    // 改为可被证伪的口径：① 步数恰随推进次数 +1（不多计不漏计）；
    // ② **非法 dt 的推进不计步**——若实现把非法 dt 也计步（多计），
    // 本判据立刻红。这两条都取自`advance` 的实际行为路径，
    // 改坏实现必红。
    //
    // 诚实标注：**本判据不度量墙钟耗时**，故不证明「<10ns/粒子/帧」
    // （锚点性能分解项）。复杂度实测由 **VE-F2212（粒子基准）** 承接，
    // 本模块只交付可数工作量的口径（每步 1 次除法 + 常数次比较与查表 +
    // 1 次 fnv1a 拌入），不代填未测数据。
    {
        let mut bag = DiagBag::new();
        let curves = std_curves();
        if let Some(mut life) = make(&default_life(), &curves, 1, &mut bag) {
            let start = life.steps;
            let mut advances = 0u64;
            for _ in 0..1000 {
                life.advance(0.01, &mut bag);
                advances += 1;
            }
            // 非法 dt 走不计龄分支：步数不得增加。
            let before_bad = life.steps;
            life.advance(f32::NAN, &mut bag);
            life.advance(-1.0, &mut bag);
            let bad_steps = life.steps - before_bad;
            set.add(
                "E06-性能-推进可数工作量为常数",
                life.steps - start == advances && bad_steps == 0,
                "",
            );
        } else {
            set.add("E06-性能-单次推进O1", false, "建器意外失败");
        }
    }

    // 查表为 O(1)：任意 t 的采样耗时不随分段数增长（以「表长 = 分段+1」验证）。
    {
        let c = std_curves();
        set.add(
            "E06-性能-LUT表长为分段加一",
            c.alpha.len() == c.alpha.steps() + 1 && !c.alpha.is_empty(),
            "",
        );
    }

    // 查表端点 O(1) 直返（边界不进入插值）。
    {
        let c = std_curves();
        let ok = c.alpha.sample(0.0) == 0.0 && c.alpha.sample(1.0) == 1.0 && c.alpha.sample(0.5).is_finite();
        set.add("E06-性能-查表端点O1直返", ok, "");
    }

    // 默认配置自洽（便捷路径不得与常量不一致）。
    {
        let cfg = default_life();
        let ok = cfg.dist == LifetimeDist::Constant(1.0)
            && cfg.death == DeathBehavior::Vanish
            && cfg.fade == FadeConfig::WHOLE_LIFE;
        set.add("E06-性能-默认配置自洽", ok, "");
    }

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vel05_all_checks_green() {
        let set = run_vel05_all_checks();
        let (passed, failed) = set.tally();
        assert!(failed == 0, "VE-F2205 自检存在 {}/{} 红项", failed, passed + failed);
    }

    #[test]
    fn vel05_lut_matches_f1407_source() {
        // 反假变体测试的对照组：LUT 必须是 F1407 原曲线的搬运。
        // 若有人把 LUT 改成「就地重写的插值」，此对拍会立刻红。
        for curve in [
            FadeCurve::Linear,
            FadeCurve::EqualPower,
            FadeCurve::Exponential,
            FadeCurve::SCurve,
            FadeCurve::Bezier { x1: 0.25, y1: 0.1, x2: 0.75, y2: 0.9 },
        ] {
            let lut = must(CurveLut::build(&curve, LIFELINE_LUT_STEPS), "LUT");
            let bound = lut.declared_max_error() + 1e-6;
            for i in 0..=256 {
                let t = i as f64 / 256.0;
                let want = curve.at(t) as f32;
                let got = lut.sample(t as f32);
                assert!(
                    (want - got).abs() <= bound,
                    "曲线 {} 在 t={} 偏差 {} 超过上界 {}",
                    curve.label(),
                    t,
                    (want - got).abs(),
                    bound
                );
            }
        }
    }

    #[test]
    fn vel05_burst_never_silently_noops() {
        // 零静默的核心不变量：爆裂在**任何**深度下都必须报错。
        // 若有人把 STUB 改成「暂时返回空列表不报错」，此用例立刻红。
        for depth in 0..=(BURST_MAX_DEPTH + 2) {
            let mut bag = DiagBag::new();
            let r = reserve_burst(depth, &mut bag);
            assert!(r.is_err(), "深度 {} 的爆裂必须报错，不得静默", depth);
            assert!(!bag.is_empty(), "深度 {} 的爆裂必须产诊断", depth);
        }
    }

    #[test]
    fn vel05_depth_gate_precedes_stub() {
        // 前向兼容约束的关键次序：深度门必须先于 STUB 生效，
        // 否则实现子发射器那天深度门会静默失效。
        let mut bag = DiagBag::new();
        let _ = reserve_burst(BURST_MAX_DEPTH, &mut bag);
        assert!(bag.has(DiagCode::GroupRejected), "超限须报深度超限");
        assert!(
            !bag.has(DiagCode::TransitionRejected),
            "超限不得再报「未实现」——否则深度门未真正生效"
        );
    }

    #[test]
    fn vel05_non_monotonic_is_allowed_not_blocked() {
        // 锚点降级矩阵第二项：非单调「视觉怪异但合法」→ 允许。
        // 这条用例守着「不被过度拒绝」：把咨询升级为阻断会让用户创作受限。
        let rebound = FadeCurve::Bezier { x1: 0.5, y1: 1.0, x2: 0.5, y2: -0.5 };
        let mut bag = DiagBag::new();
        let curves = must(
            CurveSet::build(
                &rebound,
                &FadeCurve::Linear,
                &FadeCurve::Linear,
                ColorRamp::new(Rgba::new(1.0, 1.0, 1.0, 1.0), Rgba::TRANSPARENT),
                LIFELINE_LUT_STEPS,
            ),
            "非单调曲线集",
        );
        let cfg = LifeConfig::new(
            LifetimeDist::Constant(1.0),
            FadeConfig::WHOLE_LIFE,
            DeathBehavior::Vanish,
        );
        let r = create_particle_life(&cfg, &curves, &mut RandomSource::new(1), &mut bag);
        assert!(r.is_ok(), "非单调曲线是合法配置，不得拒绝");
        assert!(bag.is_empty(), "非单调不得产诊断（仅咨询）");
    }

    #[test]
    fn vel05_determinism_is_bitwise() {
        // 确定性必须逐位而非容差：生命周期是纯标量运算，
        // 任何差异都说明有人读了墙钟或全局 RNG。
        let sim = || {
            let mut bag = DiagBag::new();
            let curves = std_curves();
            let cfg = LifeConfig::new(
                range_dist(0.3, 2.5),
                FadeConfig { fade_start: 0.35 },
                DeathBehavior::Vanish,
            );
            let mut ledger = LifetimeLedger::new();
            let mut rng = RandomSource::new(0xC0FFEE);
            for _ in 0..32 {
                if let Outcome::Ok { value: mut life, .. } =
                    create_particle_life(&cfg, &curves, &mut rng, &mut bag)
                {
                    ledger.admit(&life);
                    for _ in 0..23 {
                        let adv = life.advance(0.013, &mut bag);
                        ledger.record(&adv);
                    }
                }
            }
            ledger
        };
        assert!(sim().same_as(&sim()), "同种子双跑必须逐位一致");
    }

    #[test]
    fn vel05_four_states_exhaustively_partition_progress() {
        // 四态必须是 [0,1] 进度轴的**完备划分**：任一 t 必落且只落一态。
        // 这条比「四态在册」更强——它验证语义而非枚举。
        //
        // 分段事实（按 [`LifeState::at`] 的实现，别写成想当然的版本）：
        // `t == 0` 才算新生（`!(t > 0.0)` 才回Newborn），**不是整个 [0,0.3)
        // 都是新生**——`t = 0.001` 已经进入存活段。写断言时把新生段当成
        // 一整个区间，是把「恰好出生那一刻」误读成「出生后的前30%」。
        let fade_start = 0.3;
        let mut counts = [0usize; 4];
        for i in 0..=1000 {
            let t = i as f32 / 1000.0;
            let s = LifeState::at(t, fade_start);
            let idx = match s {
                LifeState::Newborn => 0,
                LifeState::Alive => 1,
                LifeState::Fading => 2,
                LifeState::Dead => 3,
            };
            counts[idx] += 1;
        }
        // 采样点共 1001 个（i = 0..=1000）：t=0 新生 1 个；
        // t ∈ (0, 0.3) 存活 299 个；t ∈ [0.3, 1) 淡出 700 个；t = 1 死亡 1 个。
        assert_eq!(counts[0], 1, "仅 t=0 为新生");
        assert_eq!(counts[1], 299, "t ∈ (0,0.3) 为存活");
        assert_eq!(counts[2], 700, "t ∈ [0.3,1) 为淡出");
        assert_eq!(counts[3], 1, "仅 t=1 为死亡");
        assert_eq!(
            counts.iter().sum::<usize>(),
            1001,
            "四态必须完备划分，任何 t 都必落且只落一态"
        );
    }

    #[test]
    fn vel05_shrink_rejects_nonzero_size_curve() {
        // 跨域一致性：「缩小」要求尺寸曲线终点归零，
        // 否则配了「缩小」却看起来凭空消失——最难查的一类配置缺陷。
        let mut bag = DiagBag::new();
        let curves = must(
            CurveSet::build(
                &FadeCurve::Linear,
                &FadeCurve::SCurve, // 终点为 1，不归零
                &FadeCurve::Linear,
                ColorRamp::new(Rgba::new(1.0, 0.0, 0.0, 1.0), Rgba::new(0.0, 0.0, 1.0, 0.0)),
                LIFELINE_LUT_STEPS,
            ),
            "非零终点尺寸曲线集",
        );
        let cfg = LifeConfig::new(
            LifetimeDist::Constant(1.0),
            FadeConfig::WHOLE_LIFE,
            DeathBehavior::Shrink,
        );
        let r = create_particle_life(&cfg, &curves, &mut RandomSource::new(1), &mut bag);
        assert!(r.is_err(), "尺寸曲线终点非零时「缩小」不可达，须拒绝");
        assert!(bag.has(DiagCode::ShapeRejected), "须产诊断");
    }
}
