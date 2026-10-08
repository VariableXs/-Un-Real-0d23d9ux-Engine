//! VE-F2203 · 域自检（判据逐条对应，见 `vel03_emitter.rs` 头注）
//!
//! 判据映射（锚点原文 → 自检项）：
//! - 五形状 → `E01-五形状-*`
//! - 确定性累积 → `E01-累积-*`
//! - 两级层级 → `E01-层级-*`
//! - 状态机 → `E01-状态机-*`
//! - 降级矩阵（发射率钳制 / 形状拒绝 / 退化跳过 / 转移拒绝 / 风暴合并）→ `E01-降级-*`
//! - 性能分解（发射 O(新粒子数) / 网格采样 O(logN) / 状态转移 O(1)）→ `E01-复杂度-*`
//! - 零静默 → `E01-零静默-*`
//!
//! 零墙钟、零 IO，随机源显式注入故回归可复现。

use super::vel03_emitter::*;
use crate::checks::CheckSet;

use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;

/// 三角形：直角三角形，面积 = 0.5。
fn unit_tri(origin: Vec3, u: Vec3, v: Vec3) -> Triangle {
    Triangle {
        a: origin,
        b: add_v3(origin, u),
        c: add_v3(origin, v),
    }
}

/// 退化的三点共线三角形（面积为 0）。
fn degenerate_tri(origin: Vec3, dir: Vec3) -> Triangle {
    Triangle { a: origin, b: add_v3(origin, scale_v3(dir, 1.0)), c: add_v3(origin, scale_v3(dir, 2.0)) }
}

/// 标准配置：球形状 + 均匀速度 + 单组。
fn std_config() -> EmitterConfig {
    EmitterConfig {
        rate_per_sec: 10.0,
        shape: ShapeParams::Sphere {
            center: Vec3::ZERO,
            radius: 2.0,
            mode: SphereMode::Surface,
        },
        velocity: VelocityDistParams::Uniform { min_speed: 1.0, max_speed: 2.0 },
        groups: vec![ParticleGroup::new(1, 1.0)],
        seed: 0xC0FFEE,
        lifetime: 2.0,
    }
}

/// VE-F2203 域自检。
pub fn run_vel03_checks() -> CheckSet {
    let mut set = CheckSet::new("VE-F2203");

    // =======================================================================
    // 一、五形状（判据一）
    // =======================================================================

    // 五型在册不多不少，且中英名齐备（读屏与文档都要用）。
    {
        let ok = ShapeKind::ALL.len() == 5
            && ShapeKind::ALL
                .iter()
                .all(|k| !k.zh().is_empty() && !k.en().is_empty());
        set.add("E01-五形状-五型在册不多不少", ok, "");
    }

    // 五型英文名唯一（唯一标识是契约，重复即歧义）。
    {
        let mut names: Vec<&str> = ShapeKind::ALL.iter().map(|k| k.en()).collect();
        names.sort_unstable();
        let mut uniq = names.clone();
        uniq.dedup();
        set.add("E01-五形状-英文名唯一", names.len() == uniq.len(), "");
    }

    // 点形状：恒产出原点，且对同一 rng 逐位可复现。
    {
        let mut bag = DiagBag::new();
        let p = ShapeParams::Point { origin: Vec3::new(1.0, 2.0, 3.0) };
        let mut r1 = RandomSource::new(7);
        let mut r2 = RandomSource::new(7);
        let a = sample_shape(&p, &mut r1, &mut bag);
        let b = sample_shape(&p, &mut r2, &mut bag);
        let ok = matches!(a, Outcome::Ok { value: Vec3 { x: 1.0, y: 2.0, z: 3.0 }, .. })
            && a == b;
        set.add("E01-五形状-点恒产出原点且可复现", ok, "");
    }

    // 线形状：采样点全部落在端点之间，且覆盖两端。
    {
        let mut bag = DiagBag::new();
        let p = ShapeParams::Line { from: Vec3::ZERO, to: Vec3::new(10.0, 0.0, 0.0) };
        let mut rng = RandomSource::new(11);
        let mut all_inside = true;
        let mut min_x = f32::MAX;
        let mut max_x = f32::MIN;
        for _ in 0..512 {
            if let Outcome::Ok { value: v, .. } = sample_shape(&p, &mut rng, &mut bag) {
                if v.x < -1e-4 || v.x > 10.0 + 1e-4 || v.y.abs() > 1e-4 || v.z.abs() > 1e-4 {
                    all_inside = false;
                }
                if v.x < min_x {
                    min_x = v.x;
                }
                if v.x > max_x {
                    max_x = v.x;
                }
            }
        }
        // 覆盖两端：512 次采样必横跨大部分区间（余量极小，断言留1e-3 容差）。
        set.add(
            "E01-五形状-线落区间且覆盖两端",
            all_inside && min_x < 0.5 && max_x > 9.5,
            "",
        );
    }

    // 球表面：模长恒等于半径（不多不少，不少不少）。
    {
        let mut bag = DiagBag::new();
        let p = ShapeParams::Sphere { center: Vec3::ZERO, radius: 3.0, mode: SphereMode::Surface };
        let mut rng = RandomSource::new(13);
        let mut all_on_surface = true;
        for _ in 0..512 {
            if let Outcome::Ok { value: v, .. } = sample_shape(&p, &mut rng, &mut bag) {
                // 容差 1e-3 的来历：cos/sin 替身实测误差 1.0e-4，乘半径 3.0 得 3e-4，
                // 留 3 倍余量。不按 f32 机器精度写 1e-7——三角函数是级数近似，
                // 不是精确值，写 1e-7 会让判据恒红且掩盖真实精度。
                let d = (length_v3(v) - 3.0).abs();
                if d > 1e-3 {
                    all_on_surface = false;
                }
            }
        }
        set.add("E01-五形状-球表面模长恒等半径", all_on_surface, "");
    }

    // 球体积：半径分布覆盖 [0, r]，且内部点确实少于表面点（开立方的效果）。
    {
        let mut bag = DiagBag::new();
        let p = ShapeParams::Sphere { center: Vec3::ZERO, radius: 1.0, mode: SphereMode::Volume };
        let mut rng = RandomSource::new(17);
        let mut has_inner = false;
        let mut has_outer = false;
        for _ in 0..512 {
            if let Outcome::Ok { value: v, .. } = sample_shape(&p, &mut rng, &mut bag) {
                let d = length_v3(v);
                if d < 0.5 {
                    has_inner = true;
                }
                if d > 0.9 {
                    has_outer = true;
                }
            }
        }
        set.add("E01-五形状-球体积含内外分层", has_inner && has_outer, "");
    }

    // 锥形状：全部落在张角内（与轴夹角 ≤ half_angle），且轴向前进为正。
    {
        let mut bag = DiagBag::new();
        let half = 0.5f32;
        let p = ShapeParams::Cone {
            apex: Vec3::ZERO,
            axis: Vec3::new(0.0, 1.0, 0.0),
            half_angle: half,
            length: 5.0,
            mode: ConeAxisMode::Base,
        };
        let mut rng = RandomSource::new(19);
        let mut within_angle = true;
        let mut forward = true;
        for _ in 0..512 {
            if let Outcome::Ok { value: v, .. } = sample_shape(&p, &mut rng, &mut bag) {
                let len = length_v3(v);
                if len < 1e-4 {
                    continue;
                }
                // cos(夹角) >= cos(half) 即夹角 <= half。
                let cos_a = dot_v3(v, Vec3::new(0.0, 1.0, 0.0)) / len;
                if cos_a < cos_approx(half) - 5e-3 {
                    within_angle = false;
                }
                if v.y <= 0.0 {
                    forward = false;
                }
            }
        }
        set.add("E01-五形状-锥落张角内且轴向为正", within_angle && forward, "");
    }

    // 网格表面：面积加权 —— 大三角被抽中次数显著多于小三角（均匀采样会反之）。
    {
        let mut bag = DiagBag::new();
        // 小三角面积 0.5，大三角面积 50（100 倍）。
        let tris = vec![
            unit_tri(Vec3::ZERO, Vec3::new(1.0, 0.0, 0.0), Vec3::new(0.0, 1.0, 0.0)),
            unit_tri(
                Vec3::new(0.0, 0.0, 10.0),
                Vec3::new(10.0, 0.0, 0.0),
                Vec3::new(0.0, 10.0, 0.0),
            ),
        ];
        let surface = match build_cumulative_area(&tris, &mut bag) {
            Some(s) => s,
            None => {
                set.add("E01-五形状-网格面积加权", false, "建表失败");
                return set;
            }
        };
        let p = ShapeParams::Mesh { surface };
        let mut rng = RandomSource::new(23);
        let mut small_hits = 0u32;
        for _ in 0..2000 {
            if let Outcome::Ok { value: v, .. } = sample_shape(&p, &mut rng, &mut bag) {
                if v.z < 5.0 {
                    small_hits += 1;
                }
            }
        }
        // 面积占比 1/101 ≈ 1%，2000 次期望约 20 次；断言小三角被抽中 < 10%
        // 即证明是按面积而非按三角形个数均匀抽。
        set.add(
            "E01-五形状-网格面积加权非均匀按个",
            small_hits > 0 && small_hits < 200,
            "",
        );
    }

    // 网格采样 O(log N)：累计面积表长度与三角形数一致且单调递增。
    {
        let mut bag = DiagBag::new();
        let tris = vec![
            unit_tri(Vec3::ZERO, Vec3::new(1.0, 0.0, 0.0), Vec3::new(0.0, 1.0, 0.0)),
            unit_tri(Vec3::new(2.0, 0.0, 0.0), Vec3::new(1.0, 0.0, 0.0), Vec3::new(0.0, 1.0, 0.0)),
            unit_tri(Vec3::new(4.0, 0.0, 0.0), Vec3::new(1.0, 0.0, 0.0), Vec3::new(0.0, 1.0, 0.0)),
        ];
        let s = build_cumulative_area(&tris, &mut bag).expect("建表");
        let mut monotone = true;
        for i in 1..s.cum_area.len() {
            if s.cum_area[i] < s.cum_area[i - 1] {
                monotone = false;
            }
        }
        set.add(
            "E01-五形状-累计面积表定长且单调",
            s.cum_area.len() == tris.len() + 1 && monotone && s.cum_area[0] == 0.0,
            "",
        );
    }

    // =======================================================================
    // 二、确定性累积（判据二）
    // =======================================================================

    // 累积器：rate×dt 不足一粒子时余量保留，不取整不丢弃。
    {
        let mut acc = EmitAccumulator::new();
        // 0.5/s，10fps → 每帧 0.05 粒。20 帧（2 秒）应恰好 1 粒。
        let mut total = 0u32;
        for _ in 0..20 {
            total += advance_accumulator(&mut acc, 0.5, 0.1);
        }
        set.add(
            "E01-累积-低发射率跨帧不丢粒子",
            total == 1 && acc.emitted_total == 1,
            "",
        );
    }

    // 关键反例：低发射率不得「每帧交替 0/1」抖动。
    {
        let mut acc = EmitAccumulator::new();
        let mut pattern = String::new();
        for _ in 0..20 {
            let n = advance_accumulator(&mut acc, 0.5, 0.1);
            pattern.push(if n == 0 { '0' } else { '1' });
        }
        // 抖动形态是"010101..."；正确形态是 "000...01"。
        let leading_zeros = pattern.chars().take_while(|c| *c == '0').count();
        set.add(
            "E01-累积-不呈交替抖动形态",
            leading_zeros == 19,
            "",
        );
    }

    // 高发射率：rate×dt ≥ 1 时本帧即产出对应整数。
    {
        let mut acc = EmitAccumulator::new();
        let n = advance_accumulator(&mut acc, 100.0, 0.05);
        set.add("E01-累积-高发射率当帧足额", n == 5 && acc.emitted_total == 5, "");
    }

    // 帧率无关性：同一总时长下，10fps/60fps/144fps 的总产出差 < 1 粒。
    {
        let fps_counts = [10u32, 60, 144];
        let rate = 3.7f32;
        // 每种帧率都跑满 1 秒（帧数 = fps，dt = 1/fps），故总时长恒为 1 秒。
        let mut totals: Vec<u64> = Vec::new();
        for fps in fps_counts.iter() {
            let mut acc = EmitAccumulator::new();
            let dt = 1.0 / (*fps as f32);
            let frames = *fps;
            for _ in 0..frames {
                advance_accumulator(&mut acc, rate, dt);
            }
            totals.push(acc.emitted_total);
        }
        let lo = *totals.iter().min().unwrap_or(&0);
        let hi = *totals.iter().max().unwrap_or(&0);
        set.add(
            "E01-累积-跨帧率总产出差小于一",
            hi.saturating_sub(lo) <= 1,
            "",
        );
    }

    // dt 非法（负/零/NaN）时不动余量——静默清零等于吞粒子。
    {
        let mut acc = EmitAccumulator::new();
        acc.carry = 0.7;
        for bad in [0.0f32, -1.0, f32::NAN] {
            let n = advance_accumulator(&mut acc, 10.0, bad);
            if n != 0 || (acc.carry - 0.7).abs() > 1e-6 {
                set.add("E01-累积-非法dt不动余量", false, "");
                return set;
            }
        }
        set.add("E01-累积-非法dt不动余量", true, "");
    }

    // =======================================================================
    // 三、两级层级（判据三）
    // =======================================================================

    // 权重归一化：份额和为 1，且与权重成正比。
    {
        let groups = vec![
            ParticleGroup::new(1, 1.0),
            ParticleGroup::new(2, 3.0),
        ];
        let n = match normalize_group_weights(&groups) {
            Outcome::Ok { value: v, .. } => v,
            _ => {
                set.add("E01-层级-权重归一化成正比", false, "归一化失败");
                return set;
            }
        };
        let sum: f32 = n.iter().sum();
        set.add(
            "E01-层级-权重归一化成正比",
            (sum - 1.0).abs() < 1e-5 && (n[0] - 0.25).abs() < 1e-5 && (n[1] - 0.75).abs() < 1e-5,
            "",
        );
    }

    // 抽样分布：权重 1:9 的两组，20000 次里大权重组应占九成上下（期望占比语义）。
    {
        let groups = vec![
            ParticleGroup::new(1, 1.0),
            ParticleGroup::new(2, 9.0),
        ];
        let n = match normalize_group_weights(&groups) {
            Outcome::Ok { value: v, .. } => v,
            _ => {
                set.add("E01-层级-抽样贴近期望占比", false, "归一化失败");
                return set;
            }
        };
        let mut rng = RandomSource::new(31);
        let mut big = 0u32;
        for _ in 0..20000 {
            if let Some(g) = pick_group(&groups, &n, &mut rng) {
                if g.id == 2 {
                    big += 1;
                }
            }
        }
        let ratio = big as f32 / 20000.0;
        set.add(
            "E01-层级-抽样贴近期望占比",
            (ratio - 0.9).abs() < 0.02,
            "",
        );
    }

    // 小权重组不被硬配额抹掉：权重 0.001 也必被抽中过（期望占比 ≠ 硬配额）。
    {
        let groups = vec![
            ParticleGroup::new(1, 999.0),
            ParticleGroup::new(2, 1.0),
        ];
        let n = match normalize_group_weights(&groups) {
            Outcome::Ok { value: v, .. } => v,
            _ => {
                set.add("E01-层级-小权重组不被抹掉", false, "归一化失败");
                return set;
            }
        };
        let mut rng = RandomSource::new(37);
        let mut small_hits = 0u32;
        for _ in 0..20000 {
            if let Some(g) = pick_group(&groups, &n, &mut rng) {
                if g.id == 2 {
                    small_hits += 1;
                }
            }
        }
        set.add("E01-层级-小权重组不被抹掉", small_hits > 0, "");
    }

    // 每粒必归一组：不允许出现 None（抽样必落在某组）。
    {
        let groups = vec![
            ParticleGroup::new(1, 1.0),
            ParticleGroup::new(2, 1.0),
            ParticleGroup::new(3, 1.0),
        ];
        let n = match normalize_group_weights(&groups) {
            Outcome::Ok { value: v, .. } => v,
            _ => Vec::new(),
        };
        let mut rng = RandomSource::new(41);
        let mut none_count = 0u32;
        for _ in 0..5000 {
            if pick_group(&groups, &n, &mut rng).is_none() {
                none_count += 1;
            }
        }
        set.add("E01-层级-每粒必归一组", none_count == 0, "");
    }

    set
}

// ===========================================================================
// 状态机与降级矩阵自检（独立函数，保持主函数可读）
// ===========================================================================

/// 状态机 + 降级矩阵 + 零静默自检。
pub fn run_vel03_lifecycle_checks() -> CheckSet {
    let mut set = CheckSet::new("VE-F2203-lifecycle");

    // 四态在册不多不少，中文标签齐备。
    {
        let ok = EmitterState::ALL.len() == 4 && EmitterState::ALL.iter().all(|s| !s.zh().is_empty());
        set.add("E01-状态机-四态在册不多不少", ok, "");
    }

    // 资源持有：仅销毁态不持有（销毁必须回收池句柄）。
    {
        let holds: Vec<bool> = EmitterState::ALL.iter().map(|s| s.holds_resources()).collect();
        set.add(
            "E01-状态机-仅销毁态不持资源",
            holds == vec![true, true, true, false],
            "",
        );
    }

    // 合法转移：创建→激活/销毁；激活⇄暂停+销毁；暂停→激活/销毁；销毁为终态。
    {
        let ok = can_transition(EmitterState::Created, EmitterState::Active)
            && can_transition(EmitterState::Active, EmitterState::Paused)
            && can_transition(EmitterState::Paused, EmitterState::Active)
            && can_transition(EmitterState::Active, EmitterState::Destroyed)
            && legal_transitions(EmitterState::Destroyed).is_empty();
        set.add("E01-状态机-合法转移表齐备", ok, "");
    }

    // 非法转移被拒：销毁不可复活；未创建即暂停不可；同态自转不可。
    {
        let r1 = can_transition(EmitterState::Destroyed, EmitterState::Active);
        let r2 = can_transition(EmitterState::Created, EmitterState::Paused);
        let r3 = can_transition(EmitterState::Active, EmitterState::Active);
        let out = request_transition(EmitterState::Destroyed, EmitterState::Active);
        let rejected = matches!(out, Outcome::Err { code: DiagCode::TransitionRejected, .. });
        set.add(
            "E01-状态机-非法转移一律拒绝",
            !r1 && !r2 && !r3 && rejected,
            "",
        );
    }

    // 拒绝消息含三态标签（可定位：哪一步想转到哪）。
    {
        if let Outcome::Err { message, hint, .. } =
            request_transition(EmitterState::Destroyed, EmitterState::Active)
        {
            let ok = message.contains("已销毁")
                && message.contains("激活中")
                && !hint.is_empty();
            set.add("E01-状态机-拒绝消息含来源目标与出路", ok, "");
        } else {
            set.add("E01-状态机-拒绝消息含来源目标与出路", false, "");
        }
    }

    // apply_transition 非法时**不改状态**（不静默纠正）。
    {
        let mut em = match create_emitter(std_config()) {
            Outcome::Ok { value: e, .. } => e,
            _ => {
                set.add("E01-状态机-非法转移不改状态", false, "建器失败");
                return set;
            }
        };
        let mut bag = DiagBag::new();
        let applied = apply_transition(&mut em, EmitterState::Destroyed, &mut bag);
        let after_destroy = em.state;
        // 已销毁后再激活：必须失败且状态不变。
        let applied2 = apply_transition(&mut em, EmitterState::Active, &mut bag);
        set.add(
            "E01-状态机-非法转移不改状态",
            applied && after_destroy == EmitterState::Destroyed
                && !applied2
                && em.state == EmitterState::Destroyed
                && bag.has(DiagCode::TransitionRejected),
            "",
        );
    }

    // 非激活态不发射；激活后正常发射。
    {
        let mut em = match create_emitter(std_config()) {
            Outcome::Ok { value: e, .. } => e,
            _ => {
                set.add("E01-状态机-仅激活态发射", false, "建器失败");
                return set;
            }
        };
        let mut bag = DiagBag::new();
        let created_spawn = step_emitter(&mut em, 0.1, &mut bag);
        let mut bag2 = DiagBag::new();
        apply_transition(&mut em, EmitterState::Active, &mut bag2);
        let active_spawn = step_emitter(&mut em, 0.1, &mut bag2);
        set.add(
            "E01-状态机-仅激活态发射",
            created_spawn.is_empty() && active_spawn.len() == 1,
            "",
        );
    }

    // 暂停不清余量：暂停期间累计的余量在恢复后仍能凑出粒子。
    {
        let mut cfg = std_config();
        cfg.rate_per_sec = 10.0;
        let mut em = match create_emitter(cfg) {
            Outcome::Ok { value: e, .. } => e,
            _ => {
                set.add("E01-状态机-暂停不清累积余量", false, "建器失败");
                return set;
            }
        };
        let mut bag = DiagBag::new();
        apply_transition(&mut em, EmitterState::Active, &mut bag);
        let _ = step_emitter(&mut em, 0.05, &mut bag); // 0.5 粒 → 余量 0.5
        apply_transition(&mut em, EmitterState::Paused, &mut bag);
        let carry_before = em.acc.carry;
        for _ in 0..10 {
            let _ = step_emitter(&mut em, 0.1, &mut bag); // 暂停期间不动余量
        }
        let carry_after = em.acc.carry;
        apply_transition(&mut em, EmitterState::Active, &mut bag);
        let resumed = step_emitter(&mut em, 0.1, &mut bag);
        set.add(
            "E01-状态机-暂停不清累积余量",
            (carry_before - carry_after).abs() < 1e-6 && resumed.len() == 1,
            "",
        );
    }

    // ======================= 降级矩阵 =========================

    // 发射率负/NaN/无穷/超上限 → 钳制，且诊断写明原值。
    {
        let mut bag = DiagBag::new();
        let neg = clamp_emit_rate(-1.0, &mut bag);
        let nan = clamp_emit_rate(f32::NAN, &mut bag);
        let inf = clamp_emit_rate(f32::INFINITY, &mut bag);
        let over = clamp_emit_rate(EMIT_RATE_MAX_PER_SEC * 2.0, &mut bag);
        let ok = neg == 0.0
            && nan == 0.0
            && inf == EMIT_RATE_MAX_PER_SEC
            && over == EMIT_RATE_MAX_PER_SEC
            && bag.has(DiagCode::RateClamped)
            && bag.has_msg_containing(DiagCode::RateClamped, "NaN");
        set.add("E01-降级-发射率四态钳制且诊断写原值", ok, "");
    }

    // 合法发射率原样通过，不产诊断（不无事生非）。
    {
        let mut bag = DiagBag::new();
        let v = clamp_emit_rate(120.5, &mut bag);
        set.add("E01-降级-合法发射率零诊断", v == 120.5 && bag.is_empty(), "");
    }

    // 形状参数非法 → 拒绝（锥角越界/球半径非正/线退化/点非有限）。
    {
        let bad_cone = ShapeParams::Cone {
            apex: Vec3::ZERO,
            axis: Vec3::new(0.0, 1.0, 0.0),
            half_angle: core::f32::consts::PI, // ≥π/2 越界
            length: 1.0,
            mode: ConeAxisMode::Base,
        };
        let bad_sphere = ShapeParams::Sphere {
            center: Vec3::ZERO,
            radius: 0.0,
            mode: SphereMode::Surface,
        };
        let bad_line = ShapeParams::Line { from: Vec3::ZERO, to: Vec3::ZERO };
        let bad_point = ShapeParams::Point { origin: Vec3::new(f32::NAN, 0.0, 0.0) };
        let all_rejected = validate_shape(&bad_cone).is_err()
            && validate_shape(&bad_sphere).is_err()
            && validate_shape(&bad_line).is_err()
            && validate_shape(&bad_point).is_err();
        set.add("E01-降级-四类非法形状一律拒绝", all_rejected, "");
    }

    // 锥轴向退化（零向量）→ 拒绝。
    {
        let bad = ShapeParams::Cone {
            apex: Vec3::ZERO,
            axis: Vec3::ZERO,
            half_angle: 0.5,
            length: 1.0,
            mode: ConeAxisMode::Base,
        };
        set.add("E01-降级-锥零轴向拒绝", validate_shape(&bad).is_err(), "");
    }

    // 网格退化三角形 → 跳过 + 计数 + 诊断；全退化 → 拒绝而非退化成点。
    {
        let mut bag = DiagBag::new();
        let tris = vec![
            unit_tri(Vec3::ZERO, Vec3::new(1.0, 0.0, 0.0), Vec3::new(0.0, 1.0, 0.0)),
            degenerate_tri(Vec3::ZERO, Vec3::new(1.0, 0.0, 0.0)),
        ];
        let s = build_cumulative_area(&tris, &mut bag).expect("应保留有效三角形");
        let ok = s.skipped_degenerate == 1
            && s.triangles.len() == 1
            && bag.has(DiagCode::DegenerateSkipped);
        // 全退化
        let all_bad = vec![degenerate_tri(Vec3::ZERO, Vec3::new(1.0, 0.0, 0.0))];
        let mut bag2 = DiagBag::new();
        let none = build_cumulative_area(&all_bad, &mut bag2);
        set.add(
            "E01-降级-退化跳过计数全退化拒绝",
            ok && none.is_none() && bag2.has(DiagCode::MeshEmpty),
            "",
        );
    }

    // 网格表面采样点落在三角形平面内（不越界到空间）。
    {
        let mut bag = DiagBag::new();
        let tris = vec![unit_tri(
            Vec3::ZERO,
            Vec3::new(1.0, 0.0, 0.0),
            Vec3::new(0.0, 1.0, 0.0),
        )];
        let s = build_cumulative_area(&tris, &mut bag).expect("建表");
        let p = ShapeParams::Mesh { surface: s };
        let mut rng = RandomSource::new(53);
        let mut in_tri = true;
        for _ in 0..500 {
            if let Outcome::Ok { value: v, .. } = sample_shape(&p, &mut rng, &mut bag) {
                if v.x < -1e-4 || v.y < -1e-4 || (v.x + v.y) > 1.0 + 1e-3 || v.z.abs() > 1e-4 {
                    in_tri = false;
                }
            }
        }
        set.add("E01-降级-网格采样点在三角形内", in_tri, "");
    }

    // 单帧截断 → 记账 + 诊断（不静默丢粒子）。
    {
        let mut cfg = std_config();
        cfg.rate_per_sec = 1_000_000.0;
        let mut em = match create_emitter(cfg) {
            Outcome::Ok { value: e, .. } => e,
            _ => {
                set.add("E01-降级-单帧截断记账", false, "建器失败");
                return set;
            }
        };
        let mut bag = DiagBag::new();
        apply_transition(&mut em, EmitterState::Active, &mut bag);
        let out = step_emitter(&mut em, 1.0, &mut bag);
        set.add(
            "E01-降级-单帧截断记账",
            out.len() == MAX_SPAWN_PER_FRAME as usize
                && em.truncated_total > 0
                && bag.has(DiagCode::RateClamped),
            "",
        );
    }

    // 发射器风暴 → 帧边界合并 + 计数饱和（合并了几次可查）。
    {
        let mut em = match create_emitter(std_config()) {
            Outcome::Ok { value: e, .. } => e,
            _ => {
                set.add("E01-降级-风暴帧边界合并", false, "建器失败");
                return set;
            }
        };
        let mut bag = DiagBag::new();
        for _ in 0..(CHURN_STORM_THRESHOLD + 3) {
            note_churn(&mut em);
        }
        let seen = coalesce_churn(&mut em, 1, &mut bag); // 帧 1：累加
        let merged = coalesce_churn(&mut em, 0, &mut bag); // 帧 0：结算
        set.add(
            "E01-降级-风暴帧边界合并",
            seen == (CHURN_STORM_THRESHOLD + 3) as u64
                && merged > CHURN_STORM_THRESHOLD as u64
                && bag.has(DiagCode::ChurnCoalesced),
            "",
        );
    }

    // 风暴计数不超阈值不产诊断（不无事生非）。
    {
        let mut em = match create_emitter(std_config()) {
            Outcome::Ok { value: e, .. } => e,
            _ => {
                set.add("E01-降级-低频增删零诊断", false, "建器失败");
                return set;
            }
        };
        let mut bag = DiagBag::new();
        for _ in 0..3 {
            note_churn(&mut em);
        }
        coalesce_churn(&mut em, 1, &mut bag);
        coalesce_churn(&mut em, 0, &mut bag);
        set.add("E01-降级-低频增删零诊断", !bag.has(DiagCode::ChurnCoalesced), "");
    }

    // 非法速度分布 → 拒绝（区间反了/负速率/锥角越界/零轴）。
    {
        let flipped = VelocityDistParams::Uniform { min_speed: 5.0, max_speed: 1.0 };
        let negative = VelocityDistParams::Uniform { min_speed: -1.0, max_speed: 1.0 };
        let bad_cone = VelocityDistParams::Cone {
            axis: Vec3::new(0.0, 1.0, 0.0),
            half_angle: core::f32::consts::PI,
            min_speed: 1.0,
            max_speed: 2.0,
        };
        let zero_axis = VelocityDistParams::Cone {
            axis: Vec3::ZERO,
            half_angle: 0.5,
            min_speed: 1.0,
            max_speed: 2.0,
        };
        let all_rejected = validate_velocity_dist(&flipped).is_err()
            && validate_velocity_dist(&negative).is_err()
            && validate_velocity_dist(&bad_cone).is_err()
            && validate_velocity_dist(&zero_axis).is_err();
        set.add("E01-降级-四类非法速度分布拒绝", all_rejected, "");
    }

    // 区间反了不自动交换（静默交换会掩盖写反的 bug）。
    {
        if let Outcome::Err { message, .. } =
            validate_velocity_dist(&VelocityDistParams::Uniform { min_speed: 5.0, max_speed: 1.0 })
        {
            set.add(
                "E01-降级-速率区间反了不自动交换",
                message.contains("反了"),
                "",
            );
        } else {
            set.add("E01-降级-速率区间反了不自动交换", false, "");
        }
    }

    // 三分布采样速率落在区间内。
    {
        let uniform = VelocityDistParams::Uniform { min_speed: 2.0, max_speed: 5.0 };
        let cone = VelocityDistParams::Cone {
            axis: Vec3::new(0.0, 1.0, 0.0),
            half_angle: 0.3,
            min_speed: 3.0,
            max_speed: 4.0,
        };
        let sphere = VelocityDistParams::Sphere { speed: 7.0 };
        let mut rng = RandomSource::new(59);
        let mut uniform_ok = true;
        let mut cone_ok = true;
        let mut sphere_ok = true;
        for _ in 0..500 {
            let v = sample_velocity(&uniform, &mut rng);
            let s = length_v3(v);
            if s < 1.9 || s > 5.1 {
                uniform_ok = false;
            }
            let v = sample_velocity(&cone, &mut rng);
            let s = length_v3(v);
            if s < 2.9 || s > 4.1 {
                cone_ok = false;
            }
            let v = sample_velocity(&sphere, &mut rng);
            if (length_v3(v) - 7.0).abs() > 1e-3 {
                sphere_ok = false;
            }
        }
        set.add("E01-降级-三分布速率守区间", uniform_ok && cone_ok && sphere_ok, "");
    }

    // 建器拦坏配置：非法寿命/非法形状/非法速度/非法组。
    {
        let mut bad_life = std_config();
        bad_life.lifetime = 0.0;
        let mut bad_shape = std_config();
        bad_shape.shape = ShapeParams::Sphere {
            center: Vec3::ZERO,
            radius: -1.0,
            mode: SphereMode::Surface,
        };
        let mut bad_vel = std_config();
        bad_vel.velocity = VelocityDistParams::Sphere { speed: f32::NAN };
        let mut bad_group = std_config();
        bad_group.groups = Vec::new();
        let all_rejected = create_emitter(bad_life).is_err()
            && create_emitter(bad_shape).is_err()
            && create_emitter(bad_vel).is_err()
            && create_emitter(bad_group).is_err();
        set.add("E01-降级-建器拦四类坏配置", all_rejected, "");
    }

    // 建器时发射率即钳制（首帧行为与稳态一致）。
    {
        let mut cfg = std_config();
        cfg.rate_per_sec = -5.0;
        if let Outcome::Ok { value: em, .. } = create_emitter(cfg) {
            set.add(
                "E01-降级-建器钳制首帧一致",
                em.config.rate_per_sec == 0.0,
                "",
            );
        } else {
            set.add("E01-降级-建器钳制首帧一致", false, "");
        }
    }

    // =======================================================================
    // 零静默与确定性
    // =======================================================================

    // 全部诊断码都有中文标签且不共用处置方向相反的语义。
    {
        let all = [
            DiagCode::RateClamped,
            DiagCode::ShapeRejected,
            DiagCode::VelocityRejected,
            DiagCode::MeshEmpty,
            DiagCode::DegenerateSkipped,
            DiagCode::TransitionRejected,
            DiagCode::GroupRejected,
            DiagCode::ChurnCoalesced,
            DiagCode::RngDegraded,
        ];
        let all_labelled = all.iter().all(|c| !c.zh().is_empty());
        // 拒绝类与改写类不可混淆：is_mutation 只对改写类为真。
        let mutation_disjoint = !DiagCode::ShapeRejected.is_mutation()
            && !DiagCode::TransitionRejected.is_mutation()
            && DiagCode::RateClamped.is_mutation();
        set.add("E01-零静默-诊断码标签齐且语义不混", all_labelled && mutation_disjoint, "");
    }

    // 同种子双跑逐位一致（发射器出流确定性）。
    {
        let run = || -> Vec<(Vec3, Vec3, u32)> {
            let mut em = match create_emitter(std_config()) {
                Outcome::Ok { value: e, .. } => e,
                _ => return Vec::new(),
            };
            let mut bag = DiagBag::new();
            apply_transition(&mut em, EmitterState::Active, &mut bag);
            let mut out: Vec<(Vec3, Vec3, u32)> = Vec::new();
            for _ in 0..20 {
                for s in step_emitter(&mut em, 0.05, &mut bag) {
                    out.push((s.position, s.velocity, s.group_id));
                }
            }
            out
        };
        let a = run();
        let b = run();
        set.add(
            "E01-零静默-同种子双跑逐位一致",
            // 期望粒数：10 粒/秒 × 0.05 秒 × 20 帧 = 10 粒。
            // （原写 20 是算术错误：把帧数当成了粒数。逐位一致本身一直成立。）
            a.len() == 10 && a == b,
            "",
        );
    }

    // 产出计数与累积计数一致（两套计数互相对拍）。
    {
        let mut em = match create_emitter(std_config()) {
            Outcome::Ok { value: e, .. } => e,
            _ => {
                set.add("E01-零静默-产出与累积计数对拍", false, "建器失败");
                return set;
            }
        };
        let mut bag = DiagBag::new();
        apply_transition(&mut em, EmitterState::Active, &mut bag);
        let mut emitted = 0u64;
        for _ in 0..100 {
            emitted += step_emitter(&mut em, 0.01, &mut bag).len() as u64;
        }
        set.add(
            "E01-零静默-产出与累积计数对拍",
            emitted == em.spawned_total && em.acc.emitted_total == emitted,
            "",
        );
    }

    // 产出请求含寿命与组（下游池侧直接可用，无需回查发射器）。
    {
        let mut em = match create_emitter(std_config()) {
            Outcome::Ok { value: e, .. } => e,
            _ => {
                set.add("E01-零静默-产出请求自足", false, "建器失败");
                return set;
            }
        };
        let mut bag = DiagBag::new();
        apply_transition(&mut em, EmitterState::Active, &mut bag);
        let out = step_emitter(&mut em, 0.1, &mut bag);
        let all_complete = out.iter().all(|s| {
            s.lifetime == 2.0 && s.group_id == 1 && s.position.is_finite() && s.velocity.is_finite()
        });
        set.add("E01-零静默-产出请求自足", !out.is_empty() && all_complete, "");
    }

    // 复杂度声明与实现相符（O(新粒子数)：产出数与帧 dt 成正比）。
    {
        let mut em = match create_emitter(std_config()) {
            Outcome::Ok { value: e, .. } => e,
            _ => {
                set.add("E01-复杂度-发射与新粒子数成正比", false, "建器失败");
                return set;
            }
        };
        let mut bag = DiagBag::new();
        apply_transition(&mut em, EmitterState::Active, &mut bag);
        let mut counts: Vec<usize> = Vec::new();
        for _ in 0..5 {
            counts.push(step_emitter(&mut em, 0.1, &mut bag).len());
        }
        // 10/s × 0.1s = 1 粒/帧，故每帧恒 1 粒。
        set.add(
            "E01-复杂度-发射与新粒子数成正比",
            counts.iter().all(|c| *c == 1),
            "",
        );
    }

    set
}

/// 域自检聚合：两套自检合并为一份 [`CheckSet`]。
pub fn run_vel03_all_checks() -> CheckSet {
    let mut set = CheckSet::new("VE-F2203");
    for (tag, sub) in [
        ("shapes", run_vel03_checks()),
        ("lifecycle", run_vel03_lifecycle_checks()),
    ]
    .iter()
    {
        let passed = sub.all_passed() && !sub.truncated();
        set.add(tag, passed, if passed { "" } else { "子集有红项" });
    }
    set
}
#[cfg(test)]
mod tests {
    use super::*;

    /// 列出所有红项名（失败时便于定位）。
    fn reds(set: &CheckSet) -> Vec<&'static str> {
        let (r, n) = set.red_items();
        (0..n)
            .filter_map(|i| r[i].as_ref().filter(|c| !c.passed).map(|c| c.name))
            .collect()
    }

    #[test]
    fn vel03_shape_checks_all_green() {
        let set = run_vel03_checks();
        let (passed, failed) = set.tally();
        assert!(
            set.all_passed(),
            "五形状/累积/层级自检有红项：{}/{} 绿，红项：{:?}",
            passed,
            passed + failed,
            reds(&set)
        );
    }

    #[test]
    fn vel03_lifecycle_checks_all_green() {
        let set = run_vel03_lifecycle_checks();
        let (passed, failed) = set.tally();
        assert!(
            set.all_passed(),
            "状态机/降级自检有红项：{}/{} 绿，红项：{:?}",
            passed,
            passed + failed,
            reds(&set)
        );
    }

    #[test]
    fn vel03_domain_aggregate_green() {
        let set = run_vel03_all_checks();
        assert!(set.all_passed(), "域聚合自检有红项：{:?}", reds(&set));
    }

    #[test]
    fn vel03_determinism_is_bit_exact() {
        // 同种子双跑逐位一致（锚点「确定性累积」的存在性理由：F2215 的前置）。
        let run = || -> Vec<(Vec3, Vec3, u32, f32)> {
            let mut em = match create_emitter(std_config()) {
                Outcome::Ok { value: e, .. } => e,
                _ => return Vec::new(),
            };
            let mut bag = DiagBag::new();
            assert!(apply_transition(&mut em, EmitterState::Active, &mut bag));
            let mut out = Vec::new();
            for _ in 0..30 {
                for s in step_emitter(&mut em, 0.05, &mut bag) {
                    out.push((s.position, s.velocity, s.group_id, s.lifetime));
                }
            }
            out
        };
        let a = run();
        let b = run();
        assert_eq!(a.len(), 15, "10/s × 0.05s × 30 帧 = 15 粒，实得 {}", a.len());
        assert!(a == b, "同种子双跑必须逐位一致");
    }

    #[test]
    fn vel03_accumulator_never_loses_or_invents() {
        // 判据「不丢粒子也不多发」：长跑下累计产出 == rate × 总时长（误差 < 1）。
        let rate = 7.3f32;
        let secs = 3.0f32;
        let mut acc = EmitAccumulator::new();
        let frames = 300u32;
        let dt = secs / (frames as f32);
        for _ in 0..frames {
            advance_accumulator(&mut acc, rate, dt);
        }
        let expect = rate * secs;
        let diff = (acc.emitted_total as f32 - expect).abs();
        assert!(
            diff < 1.0,
            "累计产出 {} 与期望 {} 偏差 {}，须 <1 粒",
            acc.emitted_total,
            expect,
            diff
        );
    }
}
