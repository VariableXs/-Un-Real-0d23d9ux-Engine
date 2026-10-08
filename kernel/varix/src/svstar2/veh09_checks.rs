//! VE-F1409 · 域自检（判据逐条对应，见 `veh09_spatial.rs` 头注）
//!
//! 判据映射（锚点原文 → 自检项）：
//! - 坐标契约 → `H09-契约-*`
//! - listener/emitter 模型 → `H09-模型-*`
//! - 双输入 → `H09-双输入-*`
//! - 距离模型 → `H09-距离-*`
//! - 64 声源 + voice 预算 → `H09-声源-*`

use super::veh09_spatial::*;
use crate::checks::CheckSet;

use alloc::{format, vec};

/// 构造一个「绕 Y 轴右转 deg 度」的听者姿态（偏航，用于方位相关判据）。
fn turned_listener(deg: f32) -> Listener {
    let a = deg * core::f32::consts::PI / 180.0;
    Listener {
        position: Cartesian::ORIGIN,
        forward: Cartesian::new(fsin(a), 0.0, -fcos(a)),
        up: Cartesian::new(0.0, 1.0, 0.0),
        class: ListenerClass::Game,
    }
}

/// 构造一个「俯仰 pitch 度 + 偏航 yaw 度」的听者姿态。
///
/// **为什么必须有俯仰**：纯偏航的听者其 `forward.y` 恒为 0，故姿态反解里
/// `forward.y` 那一项在任何偏航场景中都不参与运算——只测偏航等于没测该项，
/// 于是"该项符号写反"这类缺陷可以全绿通过（本模块的变异测试 M4 实测复现）。
/// 听者抬头/低头是游戏与协作场景的常态（看天空、看地面），必须入测。
fn pitched_listener(yaw: f32, pitch: f32) -> Listener {
    let y = yaw * core::f32::consts::PI / 180.0;
    let p = pitch * core::f32::consts::PI / 180.0;
    // forward：先按 pitch 抬头/低头，再按 yaw 偏航（单位向量构造）。
    let f = Cartesian::new(fcos(p) * fsin(y), fsin(p), -fcos(p) * fcos(y));
    // right = normalize(worldUp × forward)：worldUp=(0,1,0) 与 f 的叉积
    // 退化为 (f.z, 0, −f.x)，其模长恰为 cos(pitch)——**必须归一化**，
    // 否则俯仰越大 right 越短，up 随之缩放，最终姿态非正交。
    //
    // 这一点是本函数初版的真缺陷：漏掉归一化后 `up.len()` = cos(pitch)
    // ≠ 1，于是所有非零俯仰的用例都被 `pose_is_orthonormal` 判为非法、
    // 被 `continue` 跳过——判据「因自己的输入非法而红」，看着像抓到了
    // 缺陷，实则一个用例都没跑（假门禁）。故此处显式归一化。
    let r0 = Cartesian::new(f.z, 0.0, -f.x);
    let rl = r0.length();
    let right = if rl > 1.0e-6 {
        Cartesian::new(r0.x / rl, 0.0, r0.z / rl)
    } else {
        // pitch = ±90°（直视天顶/地面）：right 退化，取任意水平切向。
        Cartesian::new(1.0, 0.0, 0.0)
    };
    // up = right × forward（两向量皆单位且正交 ⇒ up 亦单位）。
    let up = Cartesian::new(
        right.y * f.z - right.z * f.y,
        right.z * f.x - right.x * f.z,
        right.x * f.y - right.y * f.x,
    );
    Listener {
        position: Cartesian::ORIGIN,
        forward: f,
        up,
        class: ListenerClass::Game,
    }
}

/// 构造指定优先级与距离的声源。
fn em(id: u32, prio: u8, dist: f32) -> Emitter {
    let mut e = Emitter::new(
        id,
        Cartesian::new(0.0, 0.0, -dist),
        DistanceModel::linear(),
    );
    e.priority = prio;
    e
}

/// VE-F1409 域自检。
pub fn run_veh09_checks() -> CheckSet {
    let mut set = CheckSet::new("svstar2-veh09");

    // ---- 判据：坐标契约（轴向/单位/手系跨域统一，契约先行冻结）----

    {
        // 机检点是**轴名互异**而非"声明非空"（后者恒真弱门禁）。
        let consistent = CoordinateContract::is_self_consistent();
        let identity = CoordinateContract::maps_identity();
        let decl = CoordinateContract::declaration();
        let declared = decl.contains("up=+Y")
            && decl.contains("forward=-Z")
            && decl.contains("right=+X")
            && decl.contains("right-handed")
            && decl.contains("unit=meter");
        set.add(
            "H09-契约-轴向手系单位一致",
            consistent && identity && declared,
            "",
        );
    }
    {
        // 单位闸门：米接受，厘米/英尺拒绝（域间混单位=空间错乱）。
        let gate = CoordinateContract::accepts_unit("meter")
            && !CoordinateContract::accepts_unit("centimeter")
            && !CoordinateContract::accepts_unit("foot");
        set.add("H09-契约-仅接受米制", gate, "");
    }
    {
        // 契约声明含版本号（跨域冻结：改轴向即破坏性变更，须显式升版）。
        let decl = CoordinateContract::declaration();
        set.add(
            "H09-契约-版本号冻结",
            decl.starts_with(COORDINATE_CONTRACT_VERSION),
            "",
        );
    }

    // ---- 判据：双输入（球坐标/笛卡尔等价转换，两个方向都测）----

    {
        // 正前方读数 ≈0°：这是「前向 −Z」契约的可观测后果。
        // 若误用 +Z 前向，此处会读出 180°。
        let front = cartesian_to_spherical(Cartesian::new(0.0, 0.0, -5.0));
        let az0 = front.azimuth_deg.abs() <= ROUND_TRIP_ANG_TOL_DEG
            && front.elevation_deg.abs() <= ROUND_TRIP_ANG_TOL_DEG
            && (front.distance_m - 5.0).abs() < 1.0e-3;
        // 右方 +X 读数 ≈90°，上方 +Y 读数 ≈+90° 仰角。
        let right = cartesian_to_spherical(Cartesian::new(5.0, 0.0, 0.0));
        let right90 = (right.azimuth_deg - 90.0).abs() <= ROUND_TRIP_ANG_TOL_DEG;
        let up = cartesian_to_spherical(Cartesian::new(0.0, 5.0, 0.0));
        let up90 = (up.elevation_deg - 90.0).abs() <= ROUND_TRIP_ANG_TOL_DEG;
        set.add("H09-双输入-前右上方基准读数", az0 && right90 && up90, "");
    }
    {
        // **下方**仰角必须为负（此处曾埋「负仰角被折成正」的缺陷）。
        let below = cartesian_to_spherical(Cartesian::new(0.0, -5.0, 0.0));
        let below_neg = below.elevation_deg < -89.0;
        let back = cartesian_to_spherical(spherical_to_cartesian(below));
        let below_rt = (back.elevation_deg - below.elevation_deg).abs() <= ROUND_TRIP_ANG_TOL_DEG;
        set.add("H09-双输入-下方仰角保号往返", below_neg && below_rt, "");
    }
    {
        // 越界仰角必须被钳到 ±90°（球坐标定义域），否则「双输入等价」在越界输入
        // 上失效：仰角 120° 不是合法球坐标，方位/仰角读数会互相污染。
        // 本项由变体测试逼出来——去掉 `norm_elevation` 末尾的 clamp 后，
        // 原有 30 项全绿（无一项覆盖越界输入），属真实门禁缺口。
        let over = norm_elevation(120.0);
        let under = norm_elevation(-120.0);
        let in_range_kept = (norm_elevation(45.0) - 45.0).abs() < 1.0e-4;
        let nonfinite_zero = norm_elevation(f32::NAN) == 0.0 && norm_elevation(f32::INFINITY) == 0.0;
        set.add(
            "H09-双输入-越界仰角钳到±90",
            over == 90.0 && under == -90.0 && in_range_kept && nonfinite_zero,
            "",
        );
        // 越界输入经构造器后往返仍自洽（构造器即钳制，不留脏值）。
        // **极点是方位角退化点**：仰角 ±90° 时 x、z 同为 0，`atan2(0,0)` 读数
        // 必为 0，方位角在该点本就无信息（此为球坐标固有奇点，非缺陷）。
        // 故此处只核验仰角往返，方位角在极点上不作要求——首版自检误把方位角
        // 也纳入断言，被实测读数（back.az=0 而非 30）判红，属**判据错**而非
        // 被测物错，已按几何事实更正。
        let s = Spherical::new(30.0, 120.0, 4.0);
        let c = spherical_to_cartesian(s);
        let back = cartesian_to_spherical(c);
        let polar_ok = (back.elevation_deg - 90.0).abs() <= ROUND_TRIP_ANG_TOL_DEG;
        set.add(
            "H09-双输入-越界仰角往返自洽（极点方位退化不参与）",
            s.elevation_deg == 90.0 && polar_ok,
            "",
        );
        // 非极点的越界输入：**方位角**越界折回后必须完整往返（仰角越界一律钳到
        // ±90 极点，故非极点场景只能由方位角构造——此为双输入等价的强断言）。
        let s2 = Spherical::new(400.0, 20.0, 4.0);
        set.add(
            "H09-双输入-越界方位角往返",
            (s2.azimuth_deg - 40.0).abs() < 1.0e-3 && round_trip_angles(s2),
            "",
        );
    }
    {
        // 方位角归一到 [0,360)：负角与超界角都必须折回，跨圈差值要按圈数算。
        let neg = norm360_pos(-90.0);
        let over = norm360_pos(450.0);
        let kept = (norm360_pos(180.0) - 180.0).abs() < 1.0e-4;
        set.add(
            "H09-双输入-方位角归一到0-360",
            (neg - 270.0).abs() < 1.0e-3 && (over - 90.0).abs() < 1.0e-3 && kept,
            "",
        );
    }
    {
        // 笛卡尔 → 球 → 笛卡尔。
        let pts = [
            Cartesian::new(1.0, 2.0, -3.0),
            Cartesian::new(-4.0, 0.5, 2.0),
            Cartesian::new(0.0, 0.0, -10.0),
            Cartesian::new(7.0, -1.0, 7.0),
            Cartesian::new(0.3, -2.5, -0.7),
            Cartesian::new(100.0, 50.0, -70.0),
            Cartesian::new(-0.5, -8.0, -0.25),
        ];
        let mut all_pos = true;
        for p in pts.iter() {
            if !round_trip_position(*p) {
                all_pos = false;
            }
        }
        set.add("H09-双输入-笛卡儿往返一致", all_pos, "");
    }
    {
        // 球 → 笛卡尔 → 球（**反方向也要测**：单向自洽在符号写反时仍可能全绿）。
        let sph = [
            Spherical::new(0.0, 0.0, 5.0),
            Spherical::new(90.0, 0.0, 3.0),
            Spherical::new(45.0, -30.0, 12.5),
            Spherical::new(210.0, 60.0, 0.4),
            Spherical::new(359.0, -80.0, 100.0),
        ];
        let mut all_ang = true;
        for s in sph.iter() {
            if !round_trip_angles(*s) {
                all_ang = false;
            }
        }
        set.add("H09-双输入-球坐标往返一致", all_ang, "");
    }

    // ---- 判据：listener/emitter 模型（听者姿态为相对量基准）----

    {
        let l = Listener::at_origin_default_facing(ListenerClass::Game);
        let ok = l.pose_is_orthonormal() && l.position_is_valid();
        set.add("H09-模型-标准姿态自洽", ok, "");
    }
    {
        // 姿态非法必须被拒：forward 与 up 平行、以及非单位向量。
        let parallel = Listener {
            position: Cartesian::ORIGIN,
            forward: Cartesian::new(0.0, 1.0, 0.0),
            up: Cartesian::new(0.0, 1.0, 0.0),
            class: ListenerClass::Game,
        };
        let non_unit = Listener {
            position: Cartesian::ORIGIN,
            forward: Cartesian::new(0.0, 0.0, -2.0),
            up: Cartesian::new(0.0, 1.0, 0.0),
            class: ListenerClass::Game,
        };
        let rejected = !parallel.pose_is_orthonormal() && !non_unit.pose_is_orthonormal();
        set.add("H09-模型-非正交与非单位姿态被拒", rejected, "");
    }
    {
        // 听者右转 90° 后：原正前方读作左方，且新正前方读作 0°。
        // 这条同时证明"方位是听者相对量，不是世界量"。
        let l = turned_listener(90.0);
        let old_front = l.to_local_spherical(Cartesian::new(0.0, 0.0, -10.0));
        let new_front = l.to_local_spherical(Cartesian::new(10.0, 0.0, 0.0));
        let ok = old_front.azimuth_deg > 269.0
            && old_front.azimuth_deg < 271.0
            && new_front.azimuth_deg.abs() <= ROUND_TRIP_ANG_TOL_DEG;
        set.add("H09-模型-方位随听者朝向旋转", ok, "");
    }
    {
        // 姿态往返：**偏航 × 俯仰**双轴，多姿态下 世界 → 局部球 → 世界
        // 误差在容差内。（反解的 forward 符号曾写错，单向角度判据抓不到，
        // 只有往返能抓；而只测偏航又抓不到 forward.y 项——变异测试 M4
        // 实测复现过"纯偏航全绿、符号写反仍全绿"，故必须含俯仰。）
        let mut worst: f32 = 0.0;
        let mut pose_count = 0usize;
        let mut all_poses_ok = true;
        for yaw in [0.0f32, 37.0, 90.0, 200.0, 330.0] {
            for pitch in [0.0f32, -60.0, -30.0, 25.0, 70.0] {
                let l = pitched_listener(yaw, pitch);
                if !l.pose_is_orthonormal() {
                    all_poses_ok = false;
                    continue;
                }
                pose_count += 1;
                for p in [
                    Cartesian::new(1.0, 2.0, -13.0),
                    Cartesian::new(-9.0, 5.0, 4.0),
                    Cartesian::new(0.5, -3.0, -7.0),
                    Cartesian::new(2.0, 8.0, 1.0),
                ] {
                    let b = l.local_spherical_to_world(l.to_local_spherical(p));
                    let e = fsqrt(
                        (b.x - p.x) * (b.x - p.x)
                            + (b.y - p.y) * (b.y - p.y)
                            + (b.z - p.z) * (b.z - p.z),
                    );
                    if e > worst {
                        worst = e;
                    }
                }
            }
        }
        // `pose_count` 是**防空转**的闸门：姿态构造出错时会全被
        // `continue` 跳过，此时 `all_poses_ok=false` 已使其为红；但若
        // 有人日后把姿态校验收紧成警告，这里会拦住"零用例却全绿"。
        set.add(
            "H09-模型-偏航俯仰双轴往返一致",
            all_poses_ok && pose_count == 25 && worst <= 1.0e-3,
            "",
        );
    }
    {
        // 双空间定位差异必须显性：声明文本点名 F1326 与本单，且两种
        // 定位种类互不等价（用 Eq 断言，不靠"声明非空"）。
        let decl_ok = WORLD_VS_BINAURAL_DECLARATION.contains("F1326")
            && WORLD_VS_BINAURAL_DECLARATION.contains("F1409");
        let distinct =
            SpatializationKind::MediaBinaural != SpatializationKind::WorldSpace;
        set.add("H09-模型-世界空间与媒体双耳差异显性", decl_ok && distinct, "");
    }
    {
        // 声源合法性：坐标非有限必须被拒（NaN 会静默污染整条链路）。
        let mut bad = Emitter::new(1, Cartesian::ORIGIN, DistanceModel::linear());
        bad.position = Cartesian::new(f32::NAN, 0.0, 0.0);
        let too_loud = Emitter::new(2, Cartesian::ORIGIN, DistanceModel::linear());
        let mut tl = too_loud;
        tl.base_gain = 9.0;
        set.add(
            "H09-模型-非法声源被拒",
            !bad.is_valid() && !tl.is_valid(),
            "",
        );
    }

    // ---- 判据：距离模型（三种衰减曲线 + 近场增强）----

    {
        let lin = DistanceModel::linear();
        // 参考距离内恒 1（含边界 ref 本身）；最大距离处钳到下界 0；
        // **算术中点**处恰为 0.5（线性插值的定义性特征）。
        let ok = lin.is_valid()
            && (lin.gain_at(0.5) - 1.0).abs() < 1.0e-4
            && (lin.gain_at(1.0) - 1.0).abs() < 1.0e-4
            && (lin.gain_at(50.5) - 0.5).abs() < 1.0e-3
            && lin.gain_at(100.0).abs() < 1.0e-4
            && lin.gain_at(1000.0).abs() < 1.0e-4;
        set.add("H09-距离-线性曲线三段正确", ok, "");
    }
    {
        // 对数：插值在线性化的**对数距离**域上完成，故 0.5 交点落在
        // `√(ref·max) = √(1×100) = 10`，而非算术中点 50.5——这条
        // 「交点位置差异」正是对数与线性的可观测区分。
        //
        // （初版此处断言"对数中段高于线性"是**判据错**：对数域插值把更多
        // 量程分配给低增益区，故 d=10 时对数反而低于线性。实现无误、
        // 断言方向错了——记录在此以免复犯"断言红了先怀疑实现"的惯性。）
        let lg = DistanceModel::logarithmic();
        let lin = DistanceModel::linear();
        let log_mid = (lg.gain_at(10.0) - 0.5).abs() < 1.0e-3;
        let lin_mid = (lin.gain_at(10.0) - 0.909091).abs() < 1.0e-3;
        // 两端仍与线性一致（曲线族共有的端点约定）：参考距离处 **1.0**
        // （不是 0——此处初版误写为 ≈0，是同类的"参考点取值想当然"错），
        // 最大距离处 0.0。
        let same_ends = (lg.gain_at(1.0) - 1.0).abs() < 1.0e-4
            && lg.gain_at(100.0).abs() < 1.0e-4
            && (lg.gain_at(1.0) - lin.gain_at(1.0)).abs() < 1.0e-4;
        set.add(
            "H09-距离-对数交点在对数中点",
            log_mid && lin_mid && same_ends,
            "",
        );
    }
    {
        // 自定义折线：节点处精确、中间线性插值、超界钳端点不外推。
        let cus = match DistanceModel::custom(vec![
            CurvePoint { distance_m: 1.0, gain: 1.0 },
            CurvePoint { distance_m: 10.0, gain: 0.5 },
            CurvePoint { distance_m: 50.0, gain: 0.1 },
        ]) {
            Ok(m) => m,
            Err(_) => {
                set.add("H09-距离-自定义折线插值与钳制", false, "构造被拒");
                return set;
            }
        };
        let at_node = (cus.gain_at(10.0) - 0.5).abs() < 1.0e-4;
        let mid = (cus.gain_at(5.5) - 0.75).abs() < 1.0e-4;
        // 超界不外推：100m 处等于末端节点值 0.1，而非继续下降。
        let clamped = (cus.gain_at(100.0) - 0.1).abs() < 1.0e-4
            && (cus.gain_at(500.0) - 0.1).abs() < 1.0e-4;
        set.add("H09-距离-自定义折线插值与钳制", at_node && mid && clamped, "");
    }
    {
        // 自定义节点逆序/重复必须构造即拒（插值会除零或反向）。
        let rev = DistanceModel::custom(vec![
            CurvePoint { distance_m: 10.0, gain: 1.0 },
            CurvePoint { distance_m: 1.0, gain: 0.5 },
        ]);
        let dup = DistanceModel::custom(vec![
            CurvePoint { distance_m: 5.0, gain: 1.0 },
            CurvePoint { distance_m: 5.0, gain: 0.5 },
        ]);
        let empty = DistanceModel::custom(vec![]);
        set.add(
            "H09-距离-非法节点表构造即拒",
            rev.is_err() && dup.is_err() && empty.is_err(),
            "",
        );
    }
    {
        // 三条曲线必须**互不相同**（若实现退化到同一函数，本判据变红）。
        let lin = DistanceModel::linear();
        let lg = DistanceModel::logarithmic();
        let cus = DistanceModel::custom(vec![
            CurvePoint { distance_m: 1.0, gain: 1.0 },
            CurvePoint { distance_m: 10.0, gain: 0.5 },
            CurvePoint { distance_m: 100.0, gain: 0.0 },
        ])
        .unwrap();
        let d1 = (lin.gain_at(20.0) - lg.gain_at(20.0)).abs();
        let d2 = (lin.gain_at(20.0) - cus.gain_at(20.0)).abs();
        let d3 = (lg.gain_at(20.0) - cus.gain_at(20.0)).abs();
        set.add(
            "H09-距离-三曲线互不雷同",
            d1 > 1.0e-3 && d2 > 1.0e-3 && d3 > 1.0e-3,
            "",
        );
    }
    {
        // 近场增强：1m 内生效、1m 外不生效、且**只随低频权重**。
        let mut nf = DistanceModel::linear();
        nf.near_field_boost_db = 6.0;
        let near_low = nf.near_field_low_freq_gain(0.5, 1.0);
        let near_high = nf.near_field_low_freq_gain(0.5, 0.0);
        let outside = nf.near_field_low_freq_gain(2.0, 1.0);
        let at_edge = nf.near_field_low_freq_gain(1.0, 1.0);
        let ok = near_low > 1.05 && (near_high - 1.0).abs() < 1.0e-6
            && (outside - 1.0).abs() < 1.0e-6
            && (at_edge - 1.0).abs() < 1.0e-6;
        set.add("H09-距离-近场增强仅低频且限1m内", ok, "");
    }
    {
        // 近场增强随距离单调（越近越强），且全关闭时恒 1。
        let mut nf = DistanceModel::linear();
        nf.near_field_boost_db = 9.0;
        let g0 = nf.near_field_low_freq_gain(0.1, 1.0);
        let g5 = nf.near_field_low_freq_gain(0.5, 1.0);
        let g9 = nf.near_field_low_freq_gain(0.9, 1.0);
        let mut off = DistanceModel::linear();
        off.near_field_boost_db = 0.0;
        let off_ok = (off.near_field_low_freq_gain(0.1, 1.0) - 1.0).abs() < 1.0e-6;
        set.add(
            "H09-距离-近场增强单调且可关闭",
            g0 > g5 && g5 > g9 && off_ok,
            "",
        );
    }
    {
        // 非有限距离不得产生 NaN 增益（NaN 增益会在混音图里静默传播）。
        let lin = DistanceModel::linear();
        let bad_in = lin.gain_at(f32::NAN).is_finite();
        let bad_zero = lin.gain_at(0.0).is_finite();
        let bad_neg = lin.gain_at(-5.0).is_finite();
        set.add(
            "H09-距离-非有限距离不产NaN",
            bad_in && bad_zero && bad_neg,
            "",
        );
    }

    // ---- 判据：64 声源管理 + voice 预算联动 ----

    {
        // 上限恒为 64（锚点给定），且满员后拒绝并留痕。
        let mut f = SpatialField::new(Listener::at_origin_default_facing(ListenerClass::Game));
        for i in 0..64u32 {
            if f.admit(em(i, 128, 5.0)).is_err() {
                set.add("H09-声源-64并发可纳满", false, "提前拒绝");
                return set;
            }
        }
        let full = f.active_count() == MAX_SPATIAL_EMITTERS && f.is_full();
        let over = f.admit(em(999, 128, 5.0));
        let rejected = over.is_err();
        // 拒绝不静默：进不可闻台账（否则「音效莫名不响」无从排查）。
        let traced = f.culled_ids().contains(&999);
        set.add("H09-声源-64上限拒绝且留痕", full && rejected && traced, "");
    }
    {
        // 释放后可再纳入（上限不是终身配额）。
        let mut f = SpatialField::new(Listener::at_origin_default_facing(ListenerClass::Game));
        for i in 0..64u32 {
            let _ = f.admit(em(i, 128, 5.0));
        }
        let released = f.release(7);
        let readmitted = f.admit(em(1000, 128, 5.0)).is_ok();
        set.add(
            "H09-声源-释放后可再纳入",
            released && readmitted && f.active_count() == MAX_SPATIAL_EMITTERS,
            "",
        );
    }
    {
        // 预算统一账：占用按听者类别分桶，且计入 voice 预算声明。
        let mut fg = SpatialField::new(Listener::at_origin_default_facing(ListenerClass::Game));
        for i in 0..10u32 {
            let _ = fg.admit(em(i, 128, 5.0));
        }
        let mut fm = SpatialField::new(Listener::at_origin_default_facing(ListenerClass::Media));
        for i in 0..3u32 {
            let _ = fm.admit(em(i, 128, 5.0));
        }
        let ug = fg.budget_usage();
        let um = fm.budget_usage();
        let ok = ug.bucket == "game"
            && um.bucket == "media"
            && ug.spatial_voices == 10
            && um.spatial_voices == 3
            && ug.remaining() == MAX_SPATIAL_EMITTERS - 10
            && VOICE_BUDGET_DECLARATION.contains("F1416")
            && VOICE_BUDGET_DECLARATION.contains("F1425");
        set.add("H09-声源-空间voice计入类别预算", ok, "");
    }
    {
        // 抢占排序确定性 + 三键口径：优先级低者先淘汰，
        // 同优先级时距离远者先淘汰。
        let l = Listener::at_origin_default_facing(ListenerClass::Game);
        let mut f = SpatialField::new(l);
        let _ = f.admit(em(1, 200, 1.0));
        let _ = f.admit(em(2, 100, 50.0));
        let _ = f.admit(em(3, 200, 40.0));
        let _ = f.admit(em(4, 100, 2.0));
        let ord = f.eviction_order();
        let last = ord[ord.len() - 1];
        // id=2：优先级最低且最远 → 末尾（最先淘汰）。
        let ranked = last == 2;
        // 幂等：同输入必同序（否则抢占不可测）。
        let again = f.eviction_order();
        let deterministic = ord == again;
        set.add(
            "H09-声源-抢占排序三键且确定",
            ranked && deterministic && ord.len() == 4,
            "",
        );
    }
    {
        // 排序须真的按优先级分层：序为「最该保留 → 最该淘汰」，
        // 故优先级降序出现在序的前段。三个样本的优先级序是
        // id1(250) → id3(128) → id2(10)（**不是** id 升序 1,2,3——
        // 按 id 序比较会得到恒真断言，本判据必须按优先级序比对）。
        let mut f = SpatialField::new(Listener::at_origin_default_facing(ListenerClass::Game));
        let _ = f.admit(em(1, 250, 90.0));
        let _ = f.admit(em(2, 10, 1.0));
        let _ = f.admit(em(3, 128, 5.0));
        let ord = f.eviction_order();
        let pos = |id: u32| ord.iter().position(|x| *x == id);
        let layered = match (pos(1), pos(3), pos(2)) {
            (Some(a), Some(b), Some(c)) => a < b && b < c,
            _ => false,
        };
        // 反面对照：若分层失效（按 id 排序），ord 将是 [1,2,3]，此项必红。
        set.add("H09-声源-优先级分层生效", layered, "");
    }
    {
        // 渲染面：方位/仰角/距离/增益四元组齐备且增益随距离衰减。
        let mut f = SpatialField::new(Listener::at_origin_default_facing(ListenerClass::Game));
        let _ = f.admit(em(1, 128, 2.0));
        let _ = f.admit(em(2, 128, 40.0));
        let r = f.render(0.0);
        let ok = r.len() == 2
            && r[0].distance_m < r[1].distance_m
            && r[0].gain > r[1].gain
            && r[0].azimuth_deg.abs() <= ROUND_TRIP_ANG_TOL_DEG;
        set.add("H09-声源-渲染四元组随距离衰减", ok, "");
    }
    {
        // 动态声源位移：已知 id 生效，未知 id 与非有限坐标显式拒绝。
        let mut f = SpatialField::new(Listener::at_origin_default_facing(ListenerClass::Game));
        let _ = f.admit(em(1, 128, 5.0));
        let moved = f.move_emitter(1, Cartesian::new(3.0, 0.0, -4.0));
        let unknown = f.move_emitter(77, Cartesian::ORIGIN);
        let nan_pos = f.move_emitter(1, Cartesian::new(f32::NAN, 0.0, 0.0));
        let after = f.render(0.0);
        let dist_ok = (after[0].distance_m - 5.0).abs() < 1.0e-3;
        set.add(
            "H09-声源-位移生效且非法目标被拒",
            moved && !unknown && !nan_pos && dist_ok,
            "",
        );
    }

    // ---- 读屏与确定性 ----

    {
        let mut f = SpatialField::new(Listener::at_origin_default_facing(ListenerClass::Game));
        let _ = f.admit(em(1, 128, 3.0));
        let r = f.render(0.0);
        let v = r[0];
        let summary = format!(
            "声源 {}：方位 {:.0}° 仰角 {:.0}° 距离 {:.1} 米 增益 {:.2}",
            v.id, v.azimuth_deg, v.elevation_deg, v.distance_m, v.gain
        );
        set.add(
            "H09-读屏-空间声源摘要可播",
            summary.contains('1') && summary.contains("米") && summary.contains("增益"),
            "",
        );
    }
    {
        // 同操作必同账面（可复现性：抢占/排序类逻辑的硬要求）。
        let run = || {
            let mut f = SpatialField::new(Listener::at_origin_default_facing(ListenerClass::Game));
            for i in 0..8u32 {
                let _ = f.admit(em(i, 100 + i as u8, i as f32 + 1.0));
            }
            (f.eviction_order(), f.render(0.0), f.budget_usage())
        };
        let a = run();
        let b = run();
        set.add("H09-确定-同操作同账面", a == b, "");
    }

    set
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::format;

    /// 域自检必须全绿——红项即施工未完成。
    #[test]
    fn veh09_checks_all_green() {
        let set = run_veh09_checks();
        let (passed, failed) = set.tally();
        if !set.all_passed() {
            let (items, n) = set.red_items();
            let mut msg = format!("VE-H09 域自检红项：{}/{} 绿", passed, passed + failed);
            for it in items.iter().take(n) {
                if let Some(c) = it {
                    if !c.passed {
                        msg.push_str(&format!("\n  [红] {} — {}", c.name, c.detail));
                    }
                }
            }
            panic!("{}", msg);
        }
    }

    /// 三处历史缺陷的定点回归：折叠符号、仰角保号、姿态反解符号。
    ///
    /// 这三条不是重复自检——它们各自对应一次"全绿判据漏掉的真缺陷"，
    /// 保留为具名回归以防重犯。
    #[test]
    fn trig_and_pose_regressions() {
        // ① π 折叠不变号：sin(100°) ≈ +0.985
        let s100 = 100.0f32 * core::f32::consts::PI / 180.0;
        assert!(
            (fsin(s100) - 0.98481).abs() < 1.0e-4,
            "sin(100°)={}，π 折叠取了负号",
            fsin(s100)
        );
        // 跨半周：sin(270°) ≈ −1.0
        let s270 = 270.0f32 * core::f32::consts::PI / 180.0;
        assert!((fsin(s270) + 1.0).abs() < 1.0e-4, "sin(270°)={}", fsin(s270));

        // ② 下方仰角保号
        let below = cartesian_to_spherical(Cartesian::new(0.0, -5.0, 0.0));
        assert!(below.elevation_deg < 0.0, "下方声源仰角应为负");

        // ③ 姿态反解的 forward 符号（**含俯仰**：纯偏航下 forward.y 恒 0，
        // 该项不参与运算，符号写反也测不出来——变异测试实测过此盲区）。
        let mut worst: f32 = 0.0;
        for yaw in [0.0f32, 37.0, 90.0, 200.0, 330.0] {
            for pitch in [0.0f32, -60.0, -30.0, 25.0, 70.0] {
                let l = pitched_listener(yaw, pitch);
                assert!(
                    l.pose_is_orthonormal(),
                    "俯仰 {}° 偏航 {}° 姿态不正交",
                    pitch,
                    yaw
                );
                for world in [
                    Cartesian::new(-3.0, 2.0, -6.0),
                    Cartesian::new(1.0, -9.0, 3.0),
                    Cartesian::new(0.5, 7.0, 0.5),
                ] {
                    let back = l.local_spherical_to_world(l.to_local_spherical(world));
                    let err = fsqrt(
                        (back.x - world.x).powi(2)
                            + (back.y - world.y).powi(2)
                            + (back.z - world.z).powi(2),
                    );
                    if err > worst {
                        worst = err;
                    }
                }
            }
        }
        assert!(worst < 1.0e-3, "姿态往返误差 {} 米（反解符号可能写反）", worst);
    }

    /// 64 声源满员后仍确定性稳定（排序不依赖分配顺序）。
    #[test]
    fn full_field_is_deterministic() {
        let mk = || {
            let mut f =
                SpatialField::new(Listener::at_origin_default_facing(ListenerClass::Game));
            for i in 0..MAX_SPATIAL_EMITTERS as u32 {
                let _ = f.admit(em(i, (i % 8) as u8, (i % 13) as f32 + 1.0));
            }
            f
        };
        let a = mk();
        let b = mk();
        assert_eq!(a.eviction_order(), b.eviction_order());
        assert_eq!(a.render(0.0).len(), MAX_SPATIAL_EMITTERS);
    }

    /// 非法输入零 panic：NaN/Inf 坐标、零距离、超界距离全部安全降级。
    #[test]
    fn hostile_inputs_do_not_panic() {
        let mut f = SpatialField::new(Listener::at_origin_default_facing(ListenerClass::Game));
        let mut nan_e = Emitter::new(1, Cartesian::ORIGIN, DistanceModel::linear());
        nan_e.position = Cartesian::new(f32::INFINITY, 0.0, 0.0);
        assert!(f.admit(nan_e).is_err());
        let _ = f.admit(em(2, 128, 5.0));
        // 渲染不得产出 NaN
        for v in f.render(0.0).iter() {
            assert!(v.gain.is_finite() && v.azimuth_deg.is_finite());
        }
        // 模型侧非有限距离
        let lin = DistanceModel::linear();
        assert!(lin.gain_at(f32::INFINITY).is_finite());
        assert!(lin.gain_at(f32::NAN).is_finite());
    }
}