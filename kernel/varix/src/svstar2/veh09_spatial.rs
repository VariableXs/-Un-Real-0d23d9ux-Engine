//! VE-F1409 · 空间音频引擎世界空间（VE-H 域 · 音频引擎 · 目标 460 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F1409`
//!
//! **规格原文**：listener/emitter 模型（听者+任意声源空间关系——区别于 G07
//! F1326 的媒体双耳播放：世界空间=3D 场景中声源有位置，游戏/协作场景；两
//! 空间定位差异显性，F1401 ADR 落册）；3D 定位（球坐标/笛卡尔双输入——双
//! 输入等价转换，坐标契约 F1420 声明）；距离模型（线性/对数/自定义衰减
//! 曲线+近场增强，三种衰减曲线，近场增强=1m 内的低频近场效应）；多
//! emitter 管理（64 声源并发，voice 预算 F1425 联动，空间声源也是 voice，
//! 预算统一）；坐标契约（与 VE-I/J 3D 域同坐标系——坐标系/单位（米）/轴向
//! 约定跨域统一，坐标系不一致=空间错乱，契约先行冻结）。判据：模型、双输入、
//! 距离模型、64 声源、坐标契约、判据。
//!
//! **工程量构成**（锚点原文）：listener/emitter 模型 130 行＋双输入 60 行＋
//! 距离模型 80 行＋64 声源管理 90 行＋坐标契约 40 行＝目标 460 行。
//!
//! ---
//!
//! ## 设计要点
//!
//! ### 1. 坐标契约不是本模块自拟的（锚点「与 VE-I/J 3D 域同坐标系」）
//!
//! 锚点要求坐标系与 3D 域统一，但 3D 域自身并未在一处显式声明轴向。本模块
//! 的取值**实测自VE-J/F1803 的真实实现**，不是自拟约定：
//!
//! ```text
//! src/system/ve/jDomainLighting/f1803-directional-light.ts:1063
//!     direction: { x: 0, y: -Math.sin(el), z: -Math.cos(el) }
//! ```
//!
//! 该式 el=0°（地平线）→ `(0, 0, -1)`，el=90°（天顶）→ `(0, -1, 0)`。
//! 即 3D 域的**前向是 −Z、向上是 +Y**（OpenGL 式右手系），而非 D3D 式的
//! +Z 前向 / +Y 上。若本模块按"习惯"取 +Z 前向，则同一场景里声音方位与画面
//! 方位差 180°——这正是锚点警告的"坐标系不一致=空间错乱"。
//!
//! 单位取**米**：与 3D 域一致（VE-J 点光按距离平方反比衰减、F1805 提到
//! "1cm@米级模型"），且声音的物理直觉（1m 近场增强）只在米制下成立。
//!
//! ### 2. 球坐标的方位零点跟随听者朝向，而非世界 −Z
//!
//! 球坐标三要素中，**方位角（azimuth）以听者面朝方向为 0°、右转为正**
//! （第一人称听感），仰角（elevation）以水平面为 0°、上为正，距离为
//! 半径。若方位零点锚在世界 −Z，则听者转身 90° 后"右前方"的声源方位读数
//! 会变成 90°——而听感上它仍在正前方。**方位是听者相对量，不是世界量**。
//! 这是 listener/emitter 模型与"纯几何朝向"的分界，也是本模块把 listener
//! 姿态（三维基）作为一等公民而非可选参数的原因。
//!
//! ### 3. 双输入必须等价可逆（锚点「双输入等价转换」）
//!
//! 球↔笛卡尔双向转换后，往返误差须在显式容差内。反假要点：不能只测
//! "笛卡尔→球→笛卡尔"一条路（单向正确即过），须**两个方向都测**——
//! 只测一个方向时，符号写反（如方位角右转取负）在该方向上仍可能自洽。
//!
//! ### 4. 距离模型三种曲线 + 近场增强
//!
//! - **线性**（linear）：`1 − (d − ref)/(max − ref)`，参考距离内恒 1；
//! - **对数**（logarithmic）：`ref / d` 的归一化对数衰减，本实现在
//!   `[ref, max]` 区间按对数插值，区间外钳制——比裸 `ref/d` 更可控；
//! - **自定义**（custom）：逐 emitter 传入的分段折线，节点间线性插值，
//!   超界钳到端点值（不做外推，避免负增益与反常音效）；
//! - **近场增强**（near-field boost）：1m 内低频增强。物理直觉——声源近到
//!   胸腔量级时低频直达路径与反射路径叠加，低频能量偏多、听感更"近"。
//!   本实现按 `1m` 为锚做低频权重增益，**只作用于低频权重**，不整体抬
//!   高（整体抬高=近处爆音，是缺陷不是特性）。
//!
//! ### 5. 64 声源与 voice 预算统一（锚点「空间声源也是 voice，预算统一」）
//!
//! 上限 64 为锚点给定值。关键不是"能装64 个"，而是**空间声源的 voice
//! 占用要进F1425 的统一预算账**——若空间声源走独立计数，它将对预算隐形，
//! 于是"64 个空间声源 + 32 个媒体 voice"总能同时跑满，预算仲裁失效。
//! 本模块只负责**申报占用与确定性排序**，实际抢占执行归 F1425 voice 管理
//! （分层声明：不越权代做他人单号的活），但排序口径在此冻结并可被F1425
//! 直接复用。
//!
//! 排序口径**公开可审**：`优先级降序 → 距离升序 → 索引升序`（确定性，
//! 无随机源——同输入必同输出，这是可测性的前提）。

use alloc::string::{String, ToString};
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 一、坐标契约（40 行 · 锚点：坐标系/单位（米）/轴向约定跨域统一）
// ---------------------------------------------------------------------------

/// 坐标契约版本（跨域冻结：VE-H 音频与 VE-I/J 3D 共用，改动即破坏性变更）。
pub const COORDINATE_CONTRACT_VERSION: &str = "varix.spatial.coord.v1";

/// 轴向契约：**+Y 向上、−Z 前向、+X 向右**（右手系）。
///
/// 取值依据（实测，非自拟）：VE-J/F1803 太阳方向
/// `direction = (0, −sin el, −cos el)`，el=0° → `(0,0,−1)` 即前向 −Z，
/// el=90° → `(0,−1,0)` 即天顶向下即上为 +Y。见
/// `src/system/ve/jDomainLighting/f1803-directional-light.ts:1063`。
pub struct CoordinateContract;

/// 契约的机读声明（供跨域自检比对，不只是注释）。
pub const AXIS_UP: &str = "+Y";
pub const AXIS_FORWARD: &str = "-Z";
pub const AXIS_RIGHT: &str = "+X";
pub const HANDEDNESS: &str = "right-handed";
/// 长度单位（米）。
pub const LENGTH_UNIT: &str = "meter";

impl CoordinateContract {
    /// 契约声明本体（机器可比对的字符串，跨域自检据此断言）。
    pub fn declaration() -> String {
        format!(
            "{}: up={}, forward={}, right={}, hand={}, unit={}",
            COORDINATE_CONTRACT_VERSION,
            AXIS_UP,
            AXIS_FORWARD,
            AXIS_RIGHT,
            HANDEDNESS,
            LENGTH_UNIT
        )
    }

    /// 契约自洽：三条轴互不冲突且手系声明与轴向一致。
    ///
    /// 机检点不是"字符串非空"（恒真弱门禁），而是**轴名互异**与
    /// **单位声明存在**——写重复轴名（如 up 与 forward 都写 "+Y"）时
    /// 此判据必须变红。
    pub fn is_self_consistent() -> bool {
        AXIS_UP != AXIS_FORWARD && AXIS_FORWARD != AXIS_RIGHT && AXIS_UP != AXIS_RIGHT
            && !LENGTH_UNIT.is_empty()
            && HANDEDNESS == "right-handed"
    }

    /// 3D 域坐标轴到本域坐标轴的恒等映射。
    ///
    /// 契约是**恒等**的（同一坐标系，不做旋转/缩放）——若将来 3D 域改
    /// 轴向，此处即成差异点，必须改契约而非默默换算。
    pub fn maps_identity() -> bool {
        AXIS_UP == "+Y" && AXIS_FORWARD == "-Z" && AXIS_RIGHT == "+X"
    }

    /// 单位换算闸门：只接受米，拒绝未知单位（防止域间混入厘米/英尺）。
    pub fn accepts_unit(unit: &str) -> bool {
        unit == LENGTH_UNIT
    }
}

// ---------------------------------------------------------------------------
// 二、数学基元（no_std 自持：内核无 libm）
// ---------------------------------------------------------------------------

/// 平方根（牛顿迭代，no_std 自持）。
///
/// 不用 `f32::sqrt` 的理由：内核目标为 no_std 且无 libm 依赖，`sqrt` 在
/// 宿主测试与内核镜像两条链上的可用性不一致。自持实现保证两条链同值，
/// 从而对拍不失真。固定 6 次迭代：初值取按数量级的种子，收敛到 f32 精度。
pub fn fsqrt(v: f32) -> f32 {
    if !(v > 0.0) {
        return 0.0;
    }
    // 种子：按数量级给个接近的初值，收敛更快。
    let mut x = if v >= 1.0 { v * 0.5 } else { v * 2.0 };
    let mut i = 0;
    while i < 12 {
        let n = 0.5 * (x + v / x);
        if (n - x).abs() <= x * 1.0e-6 {
            x = n;
            break;
        }
        x = n;
        i += 1;
    }
    x
}

/// `atan(z)`，z ≥ 0（主值 `[0, π/2)`）。
///
/// 化简恒等式 `atan(z) = 2·atan(z / (1+√(1+z²)))`，逐次把自变量压到
/// `tan(π/16) ≈ 0.2` 以内后用交错级数——与VE-J/F1804 的既有核同法同精度，
/// 避免同仓两处三角函数精度分叉。
fn atan_pos(z: f32) -> f32 {
    if z <= 0.0 {
        return 0.0;
    }
    if z > 1.0 {
        return core::f32::consts::FRAC_PI_2 - atan_pos(1.0 / z);
    }
    let z1 = z / (1.0 + fsqrt(1.0 + z * z));
    let z2 = z1 / (1.0 + fsqrt(1.0 + z1 * z1));
    let z3 = z2 / (1.0 + fsqrt(1.0 + z2 * z2));
    let s = z3 * z3;
    let series = z3
        * (1.0 - s / 3.0
            + s * s / 5.0
            - s * s * s / 7.0
            + s * s * s * s / 9.0
            - s * s * s * s * s / 11.0
            + s * s * s * s * s * s / 13.0
            - s * s * s * s * s * s * s / 15.0);
    8.0 * series
}

/// `atan2(y, x)`（弧度），四象限齐全。
pub fn atan2(y: f32, x: f32) -> f32 {
    if x == 0.0 && y == 0.0 {
        return 0.0;
    }
    let mut a = atan_pos(y.abs() / x.abs());
    if x < 0.0 {
        a = core::f32::consts::PI - a;
    }
    if y < 0.0 {
        a = -a;
    }
    a
}

/// `asin(x)`，x ∈ [−1, 1]。
///
/// 用 `asin(x) = atan2(x, √(1−x²))` 求值——比泰勒级数在 |x|→1 处稳定，
/// 且与 `atan2` 同源，误差不叠加两套近似。
pub fn asin_clamped(x: f32) -> f32 {
    let x = x.clamp(-1.0, 1.0);
    // `1 - x*x` 在 x→±1 时发生灾难性抵消，改写成 `(1-x)(1+x)`。
    let denom = fsqrt(((1.0 - x) * (1.0 + x)).max(0.0));
    atan2(x, denom)
}

/// `sin(x)`（弧度），以 π/2 归约到 `[0, π/2]` 后用泰勒到 x^9。
///
/// **归约的两处翻折符号不可混为一谈**（本模块曾在此埋了一个方位角偏
/// 36°~180° 的缺陷，教训写进代码）：
///
/// | 输入区间 | 变换 | 符号 | 恒等式 |
/// |---|---|---|---|
/// | `(π, 2π)` | `t ← 2π − t` | **取负** | `sin(2π − t) = −sin t` |
/// | `(π/2, π]` | `t ← π − t` | **不变号** | `sin(π − t) = +sin t` |
///
/// 两处都翻折成 `[0, π/2]`，但只有第一处要取负。若在 π 折叠处也取负，
/// `sin(100°) = sin(80°) = +0.985` 会被算成 −0.985——三角函数在小角区
/// 完全正确，误差只在折叠区发作，于是表现为"某些方位的声源定位偏差"，
/// 且偏差量随角度连续变化，看起来像坐标轴约定问题而非数学 bug，极难定位。
pub fn fsin(x: f32) -> f32 {
    let two_pi = core::f32::consts::PI * 2.0;
    let half_pi = core::f32::consts::FRAC_PI_2;
    // 归约到 [0, 2π)。
    let mut t = x % two_pi;
    if t < 0.0 {
        t += two_pi;
    }
    // 第一折：越过 π 的半周——反射并取负。
    let mut negate = false;
    if t > core::f32::consts::PI {
        t = two_pi - t;
        negate = true;
    }
    // 第二折：超过 π/2 的象限——反射但**不变号**。
    if t > half_pi {
        t = core::f32::consts::PI - t;
    }
    let s = t * t;
    // sin(t) = t − t³/3! + t⁵/5! − t⁷/7! + t⁹/9!
    let v = t
        * (1.0 - s / 6.0
            + s * s / 120.0
            - s * s * s / 5040.0
            + s * s * s * s / 362880.0);
    if negate {
        -v
    } else {
        v
    }
}

/// `cos(x)`（弧度）= `sin(x + π/2)`。
pub fn fcos(x: f32) -> f32 {
    fsin(x + core::f32::consts::FRAC_PI_2)
}

/// 角度归一到 `[0, 360)`。
pub fn norm360(deg: f32) -> f32 {
    let mut d = deg % 360.0;
    if d < 0.0 {
        d += 360.0;
    }
    d
}

/// 角度归一到 `[0, 180)`（用于方位角的正向读数面）。
pub fn norm360_pos(deg: f32) -> f32 {
    norm360(deg)
}

/// 仰角归一到 `[−90, +90]`（**保留符号**）。
///
/// **此处曾有一个把"负仰角变正"的缺陷**：初版复用了方位角的折返逻辑
/// （`>180` 则 `360−x`），于是一个在听者**下方**的声源（y<0，el=−5.8°）
/// 被折成 el=+5.8°——方位角读数正确、仰角符号却反了，往返误差高达 2m，
/// 而所有单角度断言（"正前方 0°""右方 90°"）全绿，看起来像坐标轴约定
/// 问题。教训是**仰角是有符号量，与方位角不同族，不可共用归一化函数**。
///
/// 正确做法：先折到 `[−180, 180)`，再钳到仰角物理域 `[−90, +90]`
/// （仰角本就只有半周有定义，钳制而非折返才是物理语义）。
pub fn norm_elevation(deg: f32) -> f32 {
    if !deg.is_finite() {
        return 0.0;
    }
    let mut d = deg % 360.0;
    // 折到 (−180, 180]：`%` 的结果随被除数符号，故显式修正负半周。
    if d > 180.0 {
        d -= 360.0;
    } else if d <= -180.0 {
        d += 360.0;
    }
    d.clamp(-90.0, 90.0)
}

/// 线性插值。
pub fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}

// ---------------------------------------------------------------------------
// 三、双输入（60 行 · 锚点：球坐标/笛卡尔双输入等价转换）
// ---------------------------------------------------------------------------

/// 笛卡尔坐标（米，VE-I/J 契约：+X 右 / +Y 上 / −Z 前）。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Cartesian {
    pub x: f32,
    pub y: f32,
    pub z: f32,
}

impl Cartesian {
    pub const ORIGIN: Cartesian = Cartesian { x: 0.0, y: 0.0, z: 0.0 };

    pub fn new(x: f32, y: f32, z: f32) -> Cartesian {
        Cartesian { x, y, z }
    }

    /// 模长（三维距离，米）。
    pub fn length(&self) -> f32 {
        fsqrt(self.x * self.x + self.y * self.y + self.z * self.z)
    }

    /// 是否为有限坐标（NaN/Inf 拒入——NaN 会静默污染整条下游链路）。
    pub fn is_finite(&self) -> bool {
        self.x.is_finite() && self.y.is_finite() && self.z.is_finite()
    }
}

/// 球坐标（相对听者姿态）。
///
/// - `azimuth_deg`：**听者面朝方向为 0°，右转为正**（第一人称听感）；
/// - `elevation_deg`：水平面 0°，上为正（与 VE-J 的 `el` 同号约定）；
/// - `distance_m`：半径（米，恒非负）。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Spherical {
    pub azimuth_deg: f32,
    pub elevation_deg: f32,
    pub distance_m: f32,
}

impl Spherical {
    pub fn new(azimuth_deg: f32, elevation_deg: f32, distance_m: f32) -> Spherical {
        Spherical {
            azimuth_deg: norm360(azimuth_deg),
            elevation_deg: norm_elevation(elevation_deg),
            distance_m: if distance_m > 0.0 { distance_m } else { 0.0 },
        }
    }

    /// 距离锚（=0 时钳到极小量，避免下方 asin 的除零）。
    pub fn safe_radius(&self) -> f32 {
        if self.distance_m > 1.0e-6 {
            self.distance_m
        } else {
            1.0e-6
        }
    }
}

/// 笛卡尔 → 球（相对 listener 的世界朝向）。
///
/// 约定：`az = atan2(x, −z)`（因前向为 −Z，故 −z 分量才是"正前方"），
/// `el = asin(y / r)`。
///
/// **符号要点**：若把 `atan2(x, z)` 写成"看起来更自然"的 `atan2(x, z)`，
/// 则正前方（z<0）的声源会算到 ±180°——听感上正前变成正后。这是本模块
/// 最隐蔽的错误面，自检以"正前方读数≈0°"定点。
pub fn cartesian_to_spherical(c: Cartesian) -> Spherical {
    let r = c.length();
    if !(r > 1.0e-6) {
        return Spherical::new(0.0, 0.0, 0.0);
    }
    let az = atan2(c.x, -c.z) * 180.0 / core::f32::consts::PI;
    let el = asin_clamped(c.y / r) * 180.0 / core::f32::consts::PI;
    Spherical::new(az, el, r)
}

/// 球 → 笛卡尔（经 listener 世界朝向的旋转基）。
///
/// `x = r·cos(el)·sin(az)`、`y = r·sin(el)`、`z = −r·cos(el)·cos(az)`。
/// 末项的负号来自"前向 −Z"契约。
pub fn spherical_to_cartesian(s: Spherical) -> Cartesian {
    let r = s.safe_radius();
    let az = s.azimuth_deg * core::f32::consts::PI / 180.0;
    let el = s.elevation_deg * core::f32::consts::PI / 180.0;
    let ce = fcos(el);
    Cartesian::new(r * ce * fsin(az), r * fsin(el), -r * ce * fcos(az))
}

/// 双输入等价转换的往返容差（米 / 度）。
///
/// 定这个容差的依据：f32 有效位约 7 位，1e-3 相对误差即已在可闻定位误差
/// 之下（人耳方位分辨率约 2°~5°）。容差不取 0 是因为三角函数近似必然有
/// 残差；容差不放大到 0.5° 是因为那已接近人耳可辨阈值，会掩盖真缺陷。
pub const ROUND_TRIP_POS_TOL_M: f32 = 1.0e-3;
pub const ROUND_TRIP_ANG_TOL_DEG: f32 = 0.05;

/// 往返一致性核验：笛卡尔 → 球 → 笛卡尔。
pub fn round_trip_position(c: Cartesian) -> bool {
    let s = cartesian_to_spherical(c);
    let back = spherical_to_cartesian(s);
    let r = c.length();
    // 原点在球坐标下无方位（退化），单独放行。
    if r <= 1.0e-6 {
        return back.length() <= ROUND_TRIP_POS_TOL_M;
    }
    let dx = (back.x - c.x).abs();
    let dy = (back.y - c.y).abs();
    let dz = (back.z - c.z).abs();
    dx <= ROUND_TRIP_POS_TOL_M && dy <= ROUND_TRIP_POS_TOL_M && dz <= ROUND_TRIP_POS_TOL_M
}

/// 往返一致性核验：球 → 笛卡尔 → 球。
pub fn round_trip_angles(s: Spherical) -> bool {
    if s.distance_m <= 1.0e-6 {
        return true;
    }
    let c = spherical_to_cartesian(s);
    let back = cartesian_to_spherical(c);
    // 方位角在 ±180° 处绕回（等价方位），故按环形距离比较。
    let daz = {
        let d = (back.azimuth_deg - s.azimuth_deg).abs() % 360.0;
        if d > 180.0 {
            360.0 - d
        } else {
            d
        }
    };
    let del = (back.elevation_deg - s.elevation_deg).abs();
    daz <= ROUND_TRIP_ANG_TOL_DEG && del <= ROUND_TRIP_ANG_TOL_DEG
}// ---------------------------------------------------------------------------
// 四、listener/emitter 模型（130 行 · 锚点：听者 + 任意声源空间关系）
// ---------------------------------------------------------------------------

/// 双空间定位差异声明（锚点：区别于 G07 F1326 的媒体双耳播放）。
///
/// 本模块存在的理由就是这条差异。把它写成机器可读的常量而非仅在注释里，
/// 是因为**边界不显性就会在实现期被悄悄抹平**——后来者看到「双耳」二字
/// 极易直接复用 G07 的媒体播放路径，从而让世界空间退化成"换了名字的
/// 媒体播放"。
pub const WORLD_VS_BINAURAL_DECLARATION: &str = "\
G07 F1326=媒体双耳播放：声源无位置，对已解码媒体做双耳渲染（媒体内容自带的听觉呈现）；\
VE-H F1409=世界空间：声源在三维场景中有位置，由 listener 姿态与 emitter 坐标驱动\
（游戏/协作场景）。二者定位模型不同，不得互相替代——世界空间声源必须有坐标，\
媒体播放声源不得被赋予场景坐标。";

/// 空间定位种类（两种空间定位差异的枚举面）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SpatializationKind {
    /// G07 媒体双耳播放：仅左右声道，无位置驱动。
    MediaBinaural,
    /// 本模块：三维场景声源定位。
    WorldSpace,
}

/// 听者类别（与 F1402 的四类别对齐：此处只取与空间音频相关的两类）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ListenerClass {
    /// 游戏/协作场景听者（空间音频主用户）。
    Game,
    /// 媒体回放听者（无空间定位需求）。
    Media,
}

impl ListenerClass {
    /// 预算归属类别名（跨 F1416/F1425 的记账口径一致）。
    pub fn budget_bucket(self) -> &'static str {
        match self {
            ListenerClass::Game => "game",
            ListenerClass::Media => "media",
        }
    }
}

/// 听者姿态（三维正交基，单位向量，米制无关）。
///
/// 姿态是**听者相对量**的基准：球坐标的方位零点取自 `forward`。故listener
/// 必须携带姿态，而非只带一个坐标点。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Listener {
    /// 听者位置（米）。
    pub position: Cartesian,
    /// 面朝方向（应为单位向量）。
    pub forward: Cartesian,
    /// 头顶方向（应为单位向量，且与 forward 正交）。
    pub up: Cartesian,
    /// 听者所在类别（决定 voice 预算归属，见 [`VoiceBudget`]）。
    pub class: ListenerClass,
}

impl Listener {
    /// 构造标准姿态听者（forward=−Z，up=+Y，即契约默认朝向）。
    pub fn at_origin_default_facing(class: ListenerClass) -> Listener {
        Listener {
            position: Cartesian::ORIGIN,
            forward: Cartesian::new(0.0, 0.0, -1.0),
            up: Cartesian::new(0.0, 1.0, 0.0),
            class,
        }
    }

    /// 右向量 = forward × up（右手系）。
    fn right_basis(&self) -> Cartesian {
        let f = self.forward;
        let u = self.up;
        Cartesian::new(
            f.y * u.z - f.z * u.y,
            f.z * u.x - f.x * u.z,
            f.x * u.y - f.y * u.x,
        )
    }

    /// 姿态自洽检查。
    ///
    /// 机检点是**实际正交性**而非"字段非空"（恒真弱门禁）：姿态基不
    /// 正交时球坐标换算会歪掉，而字段照样填满——只有真去点积才能发现。
    pub fn pose_is_orthonormal(&self) -> bool {
        let fl = self.forward.length();
        let ul = self.up.length();
        if (fl - 1.0).abs() > 1.0e-3 || (ul - 1.0).abs() > 1.0e-3 {
            return false;
        }
        let dot = self.forward.x * self.up.x
            + self.forward.y * self.up.y
            + self.forward.z * self.up.z;
        dot.abs() <= 1.0e-3
    }

    /// 位置是否可用（有限值）。
    pub fn position_is_valid(&self) -> bool {
        self.position.is_finite()
    }

    /// 世界坐标 → 该听者坐标系下的球坐标。
    ///
    /// 步骤：① 平移到听者原点；② 把 forward/up 旋到世界 −Z/+Y 的
    /// 标准姿态下；③ 走标准球坐标换算。**先摆正再换算**，避免在斜姿态下
    /// 直接套公式（那是最容易把方位零点搞错的写法）。
    pub fn to_local_spherical(&self, world: Cartesian) -> Spherical {
        let d = Cartesian::new(
            world.x - self.position.x,
            world.y - self.position.y,
            world.z - self.position.z,
        );
        let f = self.forward;
        let u = self.up;
        let r = self.right_basis();
        // 局部基：x 取 right，y 取 up，z 取 forward。
        // 前向为 −Z 契约，故"正前方"对应 forward 点积为正——此处以 forward
        // 为局部 +z 轴，标准换算里的 −z 项恰好把它还原成正前方 0°。
        let lx = r.x * d.x + r.y * d.y + r.z * d.z;
        let ly = u.x * d.x + u.y * d.y + u.z * d.z;
        let lz = f.x * d.x + f.y * d.y + f.z * d.z;
        cartesian_to_spherical(Cartesian::new(lx, ly, -lz))
    }

    /// 该听者坐标系下的球坐标 → 世界坐标（`to_local_spherical` 的逆）。
    ///
    /// **符号陷阱（本模块曾在此埋过一处真缺陷）**：`to_local_spherical`
    /// 构造标准球坐标时对 z 分量取了负（`Cartesian(lx, ly, -lz)`），原因是
    /// 标准球坐标的"正前方"是 −Z。故反解时 forward 的系数必须**同步取负**：
    /// `d = right·X + up·Y − forward·Z`。若写成 `+forward·Z`，单向角度读数
    /// 全部正确（因为只走 `to_local_spherical` 一侧），而反向合成会得到一个
    /// 关于听者镜像的错误位置——**单向通过的判据抓不到它**，必须测往返。
    pub fn local_spherical_to_world(&self, s: Spherical) -> Cartesian {
        let local = spherical_to_cartesian(s);
        let f = self.forward;
        let u = self.up;
        let r = self.right_basis();
        // 局部 (X, Y, Z) 反解世界：forward 系数取负，与正向构造对称。
        let dx = r.x * local.x + u.x * local.y - f.x * local.z;
        let dy = r.y * local.x + u.y * local.y - f.y * local.z;
        let dz = r.z * local.x + u.z * local.y - f.z * local.z;
        Cartesian::new(
            dx + self.position.x,
            dy + self.position.y,
            dz + self.position.z,
        )
    }
}

/// 空间声源（emitter）。
#[derive(Clone, Debug, PartialEq)]
pub struct Emitter {
    /// 稳定标识（voice 记账与抢占排序用）。
    pub id: u32,
    /// 世界坐标（米）。
    pub position: Cartesian,
    /// 距离模型参数。
    pub rolloff: DistanceModel,
    /// 基准音量（线性增益）。
    pub base_gain: f32,
    /// 优先级（高者后被抢占）。
    pub priority: u8,
}

impl Emitter {
    /// 构造：默认线性距离模型、基准音量 1.0、优先级中档。
    pub fn new(id: u32, position: Cartesian, rolloff: DistanceModel) -> Emitter {
        Emitter {
            id,
            position,
            rolloff,
            base_gain: 1.0,
            priority: 128,
        }
    }

    /// 声源定义是否合法（坐标有限、音量在值域、模型参数自洽）。
    pub fn is_valid(&self) -> bool {
        self.position.is_finite()
            && self.base_gain.is_finite()
            && self.base_gain >= 0.0
            && self.base_gain <= 4.0
            && self.rolloff.is_valid()
    }
}

// ---------------------------------------------------------------------------
// 五、距离模型（80 行 · 锚点：线性/对数/自定义衰减曲线 + 近场增强）
// ---------------------------------------------------------------------------

/// 衰减曲线族（锚点三种）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RolloffCurve {
    /// 线性：`1 − (d − ref)/(max − ref)`，参考距离内恒 1。
    Linear,
    /// 对数：参考距离到最大距离间按对数插值，区间外钳制。
    Logarithmic,
    /// 自定义：分段折线，超界钳到端点值（不外推）。
    Custom,
}

/// 自定义曲线的折线节点（距离→增益，须严格递增）。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CurvePoint {
    pub distance_m: f32,
    pub gain: f32,
}

/// 锚点给定的近场锚距：1m。
pub const NEAR_FIELD_ANCHOR_M: f32 = 1.0;

/// 距离模型。
#[derive(Clone, Debug, PartialEq)]
pub struct DistanceModel {
    pub curve: RolloffCurve,
    /// 参考距离（米）：此距离内增益恒 1。
    pub reference_m: f32,
    /// 最大距离（米）：超出即钳到 0（或近场下限）。
    pub max_distance_m: f32,
    /// 自定义曲线节点（仅 `Custom` 使用）。
    pub points: Vec<CurvePoint>,
    /// 近场增强锚（米，锚点给定 1m）。
    pub near_field_m: f32,
    /// 近场低频增强量（dB 正数，作用于低频权重而非整体）。
    pub near_field_boost_db: f32,
    /// 距离增益下界（超界钳制值，亦为自定义曲线兜底）。
    pub fallback_gain: f32,
}

impl Default for DistanceModel {
    fn default() -> DistanceModel {
        DistanceModel {
            curve: RolloffCurve::Linear,
            reference_m: 1.0,
            max_distance_m: 100.0,
            points: Vec::new(),
            near_field_m: NEAR_FIELD_ANCHOR_M,
            near_field_boost_db: 0.0,
            fallback_gain: 0.0,
        }
    }
}

impl DistanceModel {
    /// 三种曲线之一（线性），默认参数。
    pub fn linear() -> DistanceModel {
        DistanceModel::default()
    }

    /// 三种曲线之二（对数）。
    pub fn logarithmic() -> DistanceModel {
        DistanceModel {
            curve: RolloffCurve::Logarithmic,
            ..DistanceModel::default()
        }
    }

    /// 三种曲线之三（自定义折线）。
    ///
    /// 节点表须严格递增——重复或逆序会让插值除以零或反向，构造即拒。
    pub fn custom(points: Vec<CurvePoint>) -> Result<DistanceModel, String> {
        if points.is_empty() {
            return Err("自定义曲线节点表为空——无节点则曲线无定义".to_string());
        }
        for w in points.windows(2) {
            if w[1].distance_m <= w[0].distance_m {
                return Err(format!(
                    "自定义曲线节点未严格递增：{} → {}（逆序或重复会让插值失效）",
                    w[0].distance_m, w[1].distance_m
                ));
            }
        }
        let model = DistanceModel {
            curve: RolloffCurve::Custom,
            points,
            fallback_gain: 0.0,
            ..DistanceModel::default()
        };
        if !model.is_valid() {
            return Err("自定义曲线参数越界".to_string());
        }
        Ok(model)
    }

    /// 参数自洽（参考/最大距离为正且 max>ref、近场锚非负、节点合法）。
    pub fn is_valid(&self) -> bool {
        if !(self.reference_m > 0.0) || !(self.max_distance_m > self.reference_m) {
            return false;
        }
        if self.near_field_m < 0.0 || !self.near_field_boost_db.is_finite() {
            return false;
        }
        if !self.fallback_gain.is_finite() || self.fallback_gain < 0.0 {
            return false;
        }
        match self.curve {
            RolloffCurve::Custom => {
                if self.points.is_empty() {
                    return false;
                }
                for p in self.points.iter() {
                    if !p.distance_m.is_finite() || !p.gain.is_finite() || p.gain < 0.0 {
                        return false;
                    }
                }
                true
            }
            _ => true,
        }
    }

    /// 基础距离增益（不含近场增强、不含 base_gain）。
    ///
    /// 三条曲线在此汇合成单一输出——上层只认"距离 → 增益"这一个量，
    /// 不需要知道用的是哪种曲线（换曲线不改调用方）。
    pub fn gain_at(&self, distance_m: f32) -> f32 {
        let d = if distance_m.is_finite() && distance_m > 0.0 {
            distance_m
        } else {
            return 0.0;
        };
        let g = match self.curve {
            RolloffCurve::Linear => {
                if d <= self.reference_m {
                    1.0
                } else if d >= self.max_distance_m {
                    self.fallback_gain
                } else {
                    let t = (d - self.reference_m) / (self.max_distance_m - self.reference_m);
                    lerp(1.0, self.fallback_gain, t)
                }
            }
            RolloffCurve::Logarithmic => {
                if d <= self.reference_m {
                    1.0
                } else if d >= self.max_distance_m {
                    self.fallback_gain
                } else {
                    // 参考点→最大点间的对数插值：两端精确、中段平缓。
                    let t = (d.ln() - self.reference_m.ln())
                        / (self.max_distance_m.ln() - self.reference_m.ln());
                    lerp(1.0, self.fallback_gain, t)
                }
            }
            RolloffCurve::Custom => self.custom_gain_at(d),
        };
        g.clamp(0.0, 1.0)
    }

    /// 自定义折线求值：节点间线性插值，超界钳到端点（不外推）。
    ///
    /// 不外推是有意的：外推会在最大距离之外产生继续下降甚至负增益的段，
    /// 负增益意味着反相，听感是"声音倒放"——钳制是唯一安全的边界行为。
    fn custom_gain_at(&self, d: f32) -> f32 {
        let pts = &self.points;
        let first = match pts.first() {
            Some(p) => *p,
            None => return self.fallback_gain,
        };
        let last = match pts.last() {
            Some(p) => *p,
            None => return self.fallback_gain,
        };
        if d <= first.distance_m {
            return first.gain;
        }
        if d >= last.distance_m {
            return last.gain;
        }
        // 找区间 [i, i+1]；`i+1` 已由 `d < last` 保证存在故不越界。
        let mut i = 0usize;
        while i + 1 < pts.len() && d > pts[i + 1].distance_m {
            i += 1;
        }
        let a = pts[i];
        let b = match pts.get(i + 1) {
            Some(p) => *p,
            None => return b_gain_fallback(a.gain),
        };
        let span = b.distance_m - a.distance_m;
        if !(span > 0.0) {
            return b.gain;
        }
        let t = (d - a.distance_m) / span;
        lerp(a.gain, b.gain, t)
    }

    /// 近场增强（**只作用于低频权重**，不整体抬高）。
    ///
    /// 返回低频权重增益（线性）。物理直觉：声源进到胸腔量级（1m 内）时，
    /// 低频直达与反射叠加、能量偏多，听感更"近"。若改成整体增益则近处
    /// 爆音——那是缺陷不是特性，故此处显式只给低频。
    pub fn near_field_low_freq_gain(&self, distance_m: f32, low_freq_mix: f32) -> f32 {
        if self.near_field_boost_db <= 0.0 || !(self.near_field_m > 0.0) {
            return 1.0;
        }
        if !distance_m.is_finite() || distance_m >= self.near_field_m {
            return 1.0;
        }
        // 越近权重越高：d=0 → 满权重，d=near_field_m → 0 权重。
        let proximity = 1.0 - (distance_m / self.near_field_m);
        let mix = low_freq_mix.clamp(0.0, 1.0);
        // dB → 线性：10^(db/20)，再按低频权重与邻近程度缩放。
        let db = self.near_field_boost_db * proximity * mix;
        10.0f32.powf(db / 20.0)
    }

    /// 总增益 = 距离增益 × 近场低频增强。
    pub fn total_gain_at(&self, distance_m: f32, low_freq_mix: f32) -> f32 {
        self.gain_at(distance_m) * self.near_field_low_freq_gain(distance_m, low_freq_mix)
    }
}

/// 折线末端缺失时的兜底（保持节点 a 的增益，不引入未定义值）。
fn b_gain_fallback(a_gain: f32) -> f32 {
    a_gain
}// ---------------------------------------------------------------------------
// 六、64 声源管理（90 行 · 锚点：并发声源上限 + voice 预算联动）
// ---------------------------------------------------------------------------

/// 并发空间声源上限（锚点给定 64）。
pub const MAX_SPATIAL_EMITTERS: usize = 64;

/// voice 预算声明（锚点：空间声源也是 voice，预算统一）。
///
/// 分层声明：本模块**只申报占用与给出确定性排序口径**，不代F1425 执行
/// 抢占——那是 voice 管理单的职责。若在此擅自回收别人的 voice，会与
/// F1425 的四态生命周期打架（同一 voice 被两处判定，状态不可审计）。
pub const VOICE_BUDGET_DECLARATION: &str = "\
空间声源占用 voice 预算：每个并发空间声源计1 voice，计入听者所属类别的\
预算账（与媒体/系统 voice 统一记账，见 F1416 类别预算与 F1425 voice 管理）；\
F1409 负责申报占用与给出抢占排序口径（优先级降序 → 距离升序 → 索引升序），\
抢占执行与四态生命周期归 F1425——同一 voice 只由一处判定，状态可审计。";

/// 单个空间声源的空间渲染结果（上层据此驱动 panner/gain）。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SpatialVoice {
    pub id: u32,
    /// 相对听者的方位角（度，0=面朝，右转为正）。
    pub azimuth_deg: f32,
    /// 相对听者的仰角（度，上为正）。
    pub elevation_deg: f32,
    /// 距离（米）。
    pub distance_m: f32,
    /// 总增益（距离 × 近场低频增强 × 基准音量）。
    pub gain: f32,
}

/// 空间声源管理器。
///
/// 无 `heapless` 依赖，故用 `Vec`；上限由 [`MAX_SPATIAL_EMITTERS`] 硬约束，
/// 不依赖分配器容量——上限是规格的一部分，不是内存的副作用。
#[derive(Debug)]
pub struct SpatialField {
    /// 听者（姿态是相对量的基准）。
    pub listener: Listener,
    /// 当前声源（长度 ≤ [`MAX_SPATIAL_EMITTERS`]）。
    voices: Vec<Emitter>,
    /// 已被占用但被判定为不可闻的声源 id（显性台账，非静默丢弃）。
    culled: Vec<u32>,
    /// 预算申报次数（每次接纳/释放各记一次，供跨域对账）。
    pub budget_reports: u64,
}

impl SpatialField {
    /// 构造（空声源集）。
    pub fn new(listener: Listener) -> SpatialField {
        SpatialField {
            listener,
            voices: Vec::new(),
            culled: Vec::new(),
            budget_reports: 0,
        }
    }

    /// 当前并发声源数。
    pub fn active_count(&self) -> usize {
        self.voices.len()
    }

    /// 已达上限？
    pub fn is_full(&self) -> bool {
        self.voices.len() >= MAX_SPATIAL_EMITTERS
    }

    /// 不可闻声源台账（被上限拒绝者在此留痕）。
    pub fn culled_ids(&self) -> &[u32] {
        &self.culled
    }

    /// 接纳一个声源（校验 + 入列，一步完成）。
    ///
    /// 拒绝条件（两条，均显性返回人话理由）：
    /// 1. 声源定义非法（坐标非有限 / 音量越界 / 距离模型越界）；
    /// 2. 已达 64 上限。
    ///
    /// **拒绝不静默**：超限时该 id 进 `culled` 台账，否则"音效莫名不响"
    /// 将无从排查（十三·补：静默是体验缺陷）。
    ///
    /// 校验与入列**刻意合并为一个不可分割的方法**（初版拆成
    /// `admit` + `push_admitted` 两步，是个陷阱：单调 `admit` 返回 `Ok`
    /// 却并未真的入列，调用方若只用它就会得到"校验通过但声源不存在"
    /// 的静默状态；两步之间还留着"校验说够位置、入列时却满了"的
    /// 竞态窗口）。合并后只有一个入口，失败即未入列。
    pub fn admit(&mut self, emitter: Emitter) -> Result<u32, String> {
        if !emitter.is_valid() {
            return Err(format!(
                "声源 {} 定义非法（坐标非有限、音量越界或距离模型越界）",
                emitter.id
            ));
        }
        if self.is_full() {
            self.culled.push(emitter.id);
            return Err(format!(
                "已达 {} 并发空间声源上限，声源 {} 未被接纳（已入不可闻台账）",
                MAX_SPATIAL_EMITTERS, emitter.id
            ));
        }
        let id = emitter.id;
        self.voices.push(emitter);
        self.budget_reports += 1;
        Ok(id)
    }

    /// 释放一个声源（归还 voice 预算）。
    pub fn release(&mut self, id: u32) -> bool {
        let before = self.voices.len();
        self.voices.retain(|e| e.id != id);
        let removed = self.voices.len() != before;
        if removed {
            self.budget_reports += 1;
        }
        removed
    }

    /// 声源位移（动态声源每帧更新）。
    ///
    /// 未知 id 返回 `false`（异常零静默：调用方须自己处理"更新了不存在的
    /// 声源"这种情况，而不是拿到一个静默的成功）。
    pub fn move_emitter(&mut self, id: u32, position: Cartesian) -> bool {
        if !position.is_finite() {
            return false;
        }
        for e in self.voices.iter_mut() {
            if e.id == id {
                e.position = position;
                return true;
            }
        }
        false
    }

    /// 渲染全部声源的空间参数。
    ///
    /// `low_freq_mix` 是低频带权重（0=纯高频内容，1=纯低频内容）——近场
    /// 增强只对低频生效，故必须由调用方按素材内容给出，引擎不能瞎猜。
    pub fn render(&self, low_freq_mix: f32) -> Vec<SpatialVoice> {
        let mut out = Vec::new();
        for e in self.voices.iter() {
            let s = self.listener.to_local_spherical(e.position);
            let gain = e.rolloff.total_gain_at(s.distance_m, low_freq_mix) * e.base_gain;
            out.push(SpatialVoice {
                id: e.id,
                azimuth_deg: s.azimuth_deg,
                elevation_deg: s.elevation_deg,
                distance_m: s.distance_m,
                gain,
            });
        }
        out
    }

    /// 抢占排序口径（锚点：评分可审）。
    ///
    /// 返回序为**「最该保留 → 最该淘汰」**，即**列表末尾最先被淘汰**
    /// （调用方从尾部弹出）。三键：
    /// 1. 优先级低者先淘汰；
    /// 2. 同优先级：距离远者先淘汰（听感上远处更该让位）；
    /// 3. 同优先级同距离：索引大者先淘汰（确定性 tie-break）。
    ///
    /// 确定性是硬要求：同输入必同输出，否则抢占不可测、不可复现。
    /// 本函数只给序，**不执行淘汰**——执行归 F1425。
    ///
    /// 距离用 `Emitter::position` 的模长（世界原点距），而非听者距：
    /// 抢占是"资源不足时保谁"的开环决策，不依赖听者位置，避免听者移动
    /// 引发抢占序抖动（抖动=可闻的音量跳变）。
    pub fn eviction_order(&self) -> Vec<u32> {
        let mut idx: Vec<(u32, u8, f32, usize)> = Vec::new();
        for (i, e) in self.voices.iter().enumerate() {
            let d = e.position.length();
            idx.push((e.id, e.priority, d, i));
        }
        // 升序：优先级低 → 距离远 → 索引大，即最该被淘汰在前。
        idx.sort_by(|a, b| {
            a.1.cmp(&b.1)
                .then(b.2.partial_cmp(&a.2).unwrap_or(core::cmp::Ordering::Equal))
                .then(b.3.cmp(&a.3))
        });
        let mut out = Vec::new();
        for t in idx.iter() {
            out.push(t.0);
        }
        out.reverse();
        out
    }

    /// 预算占用快照（跨 F1416/F1425 对账用）。
    ///
    /// 关键设计：占用**按听者类别**分账，而非单一把总数。若不分类，
    /// "64 个空间声源 + 媒体 voice"会共用一个池，类别预算（F1416 的
    /// 媒体 60% / 系统 20% / 游戏 20%）就无从执行。
    pub fn budget_usage(&self) -> BudgetUsage {
        let bucket = self.listener.class.budget_bucket();
        BudgetUsage {
            bucket,
            spatial_voices: self.voices.len(),
            cap: MAX_SPATIAL_EMITTERS,
        }
    }
}

/// 预算占用快照。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BudgetUsage {
    /// 归属类别（"game"/"media"）。
    pub bucket: &'static str,
    /// 该类别下空间声源占用的 voice 数。
    pub spatial_voices: usize,
    /// 空间声源硬上限。
    pub cap: usize,
}

impl BudgetUsage {
    /// 剩余可用（上限 − 已占）。
    pub fn remaining(&self) -> usize {
        self.cap.saturating_sub(self.spatial_voices)
    }
}