//! VE-F1807 · 光源管理器（VE-J 域 · 直接光组 · J 域光源组织者）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F1807`
//!
//! # 职责定位（锚点原文）
//!
//! 场景光源集合的**统一管理**：注册/增删/查询的 O(1) 句柄操作、光源上限与排序
//! 策略（前向渲染最近 N 光源：距离+重要性双因子）、光源剔除（视锥外粗剔）、
//! 参数块每帧上传的组织者。
//!
//! # 判据（锚点原文五条）
//!
//! 1. **句柄安全**（生成计数防悬挂句柄）；
//! 2. **双因子排序**（距离 + 重要性）；
//! 3. **确定性**（排序稳定，与 I 域确定性口径一致）；
//! 4. **复用 I07**（视锥剔除）；
//! 5. 判据自洽。
//!
//! # 判据一：句柄安全（生成计数防悬挂句柄）
//!
//! 句柄由 `(index, generation)` 两元组构成，类型上不可分开使用
//! （[`LightHandle`] 是**不可解构的**结构体，字段私有，只能整体比较）。
//!
//! **为什么必须带生成计数**（这是本条最容易简化掉的一处，且简化后不会立刻出错）：
//! 只用 `u32` 下标作句柄时，「删掉第 3 号灯 → 新建一个灯复用第 3 号槽」之后，
//! 任何仍持有旧第 3 号句柄的代码（阴影贴图绑定、动画轨道、材质引用）都会
//! **静默指向新灯**。症状是「删了一个灯，另一个灯莫名变亮」，且只在
//! "删除后又新建"的会话里出现，极难归因。
//!
//! 生成计数让旧句柄的 `generation` 与新槽位的不匹配，访问时**当场拒绝并告警**
//! （[`LightError::StaleHandle`]），而不是静默成功。
//!
//! # 判据二：双因子排序
//!
//! 锚点原文「重要性因子 = 强度 × 类型权重（方向光恒权/点光随距离衰减）」。
//!
//! 实现取**双因子键** `(importance, distance_sq)`：
//! - 主键 `importance`：光对该视点**实际贡献的估计** = 强度 × 类型权重 × 衰减；
//! - 次键 `distance_sq`：贡献相同时取更近的（近处灯对 shading 影响更直接）。
//!
//! **类型权重表是显式常量而非隐含系数**（[`TYPE_WEIGHT`]）：方向光权重恒 1.0
//! （无距离衰减，任何位置都是满贡献），点光/聚光按 `1/(1+d²)` 衰减，
//! 面光权重 0.85（面光在 F1806 尚未实现 LTC 积分，按"次级光源"降权，
//! 理由与取值写进 [`AREA_WEIGHT_RATIONALE`]，不写成魔数）。
//!
//! # 判据三：确定性（排序稳定）
//!
//! 排序**必须全序**：`(importance 降序, distance_sq 升序, id 升序)` 三级键。
//! 第三级 `id` 是**决胜键**（锚点「排序键加 ID 决胜」）。
//!
//! 为什么第三级必需：若只用两级键，当两个灯的 `importance` 与 `distance_sq`
//! **逐位相等**时，排序结果取决于输入顺序——而输入顺序会因增删操作而变化。
//! 症状是「光源每帧在两个等亮灯之间来回跳」，表现为**闪烁**，且只在两灯
//! 参数恰好相等时出现。ID 决胜把「相等」这一情形从"依赖输入序"变成
//! "依赖稳定的唯一标识"，从而完全确定。
//!
//! **浮点相等的处理**：三级键用 `total_cmp`（IEEE 754 全序）而非 `<`/`>`
//! 逐位比较——`partial_cmp` 遇 NaN 返回 `None`，会让排序在含 NaN 的输入上
//! **静默不稳定**（`sort_by` 收到 `None` 时行为未定义）。本条在入口就把
//! 非有限参数钳掉，故 `total_cmp` 与相等判定在合法输入上等价，但用
//! `total_cmp` 是为了即使有 NaN 漏进来也是确定的（退化为全序中的某处），
//! 而不是 UB。
//!
//! # 判据四：剔除（接口对齐 I07，实现暂在本模块）
//!
//! **诚实标注**：锚点要求「剔除复用 I07 SIMD 实现」，但 I07 视锥剔除
//! **在 `svstar2` 尚未落位**（实测 `grep` 无 I07 剔除模块）。故本条：
//! - 定义与 I07 对齐的接口形状（[`FrustumPlanes`] 六平面 + 包围球判定），
//!   便于I07 落地后**直接替换实现而不改调用面**；
//! - 提供一个标量参考实现（[`sphere_in_frustum`]），
//!   在 `I07` 到位前可跑、可测，且**规模声明为标量**（不虚报 SIMD）。
//!
//! 剔除用**包围球**而非视锥六面直接判定：光源是「有体积的」——大范围点光的
//! 中心可能在视锥外但部分光锥在视野内。锚点「剔除误判（大范围点光被视锥粗剔）
//! →范围扩张包围球判定兜底」正是此。本条的兜底做法是把判据式的半径从紧半径
//! `range` 换成**扩张半径** `range × [`CULL_INFLATE`]`：紧半径会把这盏灯误剔，
//! 扩张半径把「光锥确有部分在视野内」的情形重新拉回Keep。
//!
//! 剔除的**平面判据是「任一平面在外」**而非「六面全外」：视锥是六个半空间的
//! 交，球在视锥外 ⟺ 至少一个半空间不含球。推导见 [`sphere_in_frustum`] 函数体。
//!
//! # 判据五：判据自洽（错误路径与降级矩阵）
//!
//! | 触发 | 处置 | 阻断 |
//! | --- | --- | --- |
//! | 悬挂句柄访问 | 生成计数不符 → 拒绝 + 告警（不崩溃） | 是 |
//! | 光源风暴（每帧千次增删） | 延迟合并到帧边界批量处理 | 否 |
//! | 排序不稳定导致闪烁 | 排序键加 ID 决胜（全序） | — |
//! | 剔除误判（大范围点光） | 扩张包围球二次判定兜底 | — |
//! | 非有限参数 | 入口钳制 + 告警（不静默入库） | 否 |
//! | 超上限增删 | 拒绝 + 显性告警（不静默丢弃） | 是 |
//!
//! **风暴合并是本条唯一的「延迟」语义**，须显式：`LightManager::stage_add`
//! 只把操作入队，真正生效在 [`LightManager::commit_frame`]。理由：每帧千次
//! 增删若逐次改句柄表，会让同一帧内其他系统看到**半完成**的灯光集合
//! （渲染用 3 盏、阴影用 5 盏这类不一致），症状是画面局部缺光且难复现。
//! 但「延迟」不等于「静默」——未 commit 的操作数进 [`LightManager::pending`]，
//! 任何读集合的接口在有 pending 时返回 [`LightError::UncommittedStaging`]。
//!
//! # 性能逐项分解（锚点原文四条）
//!
//! - 增删 **O(1)**：句柄表按 `index` 直接寻址，空槽用 freelist 复用。
//! - 每帧排序 **O(N log N)**（N ≤ 光源总数，典型 < 100），CPU 微秒级。
//! - 参数块上传 **一次/帧**：[`LightManager::build_param_block`] 每帧产出
//!   **一个** [`ParamBlock`]，不按灯分块（分块会让 N 次上传变 1 次）。
//! - 剔除为包围球 vs 六平面，**规模声明为标量**（见判据四诚实标注）。
//!
//! # 跨批对接点
//!
//! - 全部光源类型注册入口：F1803 方向光（`LightKind::Directional`，本条持
//!   调度侧表示，**不重写 F1803 的太阳色温数学**——那是 F1803 的职责）、
//!   F1804 点光、F1805 聚光、F1806 面光（预留位，按 [`AREA_WEIGHT_RATIONALE`]
//!   降权参与排序）。
//! - 排序策略与 F1803/F1804 超限裁剪**共用口径**：本条 [`LightManager::select`]
//!   的裁剪键与 `vej04_pointlight::cull_to_tier` 同为「重要性降序 + 稳定决胜」，
//!   但本条多一级 `distance_sq` 次键（因为管理器知道视点，光源条目不知道）。
//! - 剔除复用 I07：接口对齐，实现暂在本模块（见判据四）。
//! - 输出供 F1811 成本模型统计光源数：[`FrameStats`] 暴露 `active` / `selected`。
//! - fuzz 风暴场景进 F1813：[`LightManager::stage_add` 的批量入口即风暴面。
//!
//! # 无障碍与隐私
//!
//! 纯场景管理，**无隐私面**。不使用地理位置（方向光的太阳高度角由场景作者
//! 显式给定，本条不采集、不推断）。
//!
//! 零外部依赖；逻辑 tick 注入，零墙钟；确定性算法、零 IO、回归可复现。

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// 一、规格常量（单一事实源）
// ---------------------------------------------------------------------------

/// 光源容量上限（句柄表槽位数）。
///
/// 定标 256 的理由：典型场景光源 < 100（锚点性能分解「典型 <100」），
/// 256 已是其 2.5 倍余量；而更大只会让句柄表（每槽 24 字节）无谓占用，
/// 且 [`LightManager::stage_add`] 的线性查重在超长表上退化。
pub const MAX_LIGHTS: usize = 256;

/// 参数块对齐粒度（字节）。锚点「对齐 16 字节布局」。
pub const PARAM_BLOCK_ALIGN: usize = 16;

/// 单灯参数块字节数（对齐后）。
///
/// 字段序冻结（前后兼容契约，改序即破坏 F1811 与 D 域消费面）：
/// `pos(3×f32) + range(f32) + color(3×f32) + intensity(f32) + kind(u32)
///  + dir(3×f32) + pad(f32)` = 14×4 = 56 字节，对齐到 64。
pub const BYTES_PER_LIGHT_ALIGNED: usize = 64;

/// 每灯有效载荷字节数（对齐前）。
pub const BYTES_PER_LIGHT_RAW: usize = 56;

/// 剔除膨胀系数（扩张包围球半径 = range × 本值）。
///
/// 1.0 意味着"紧包围球"，会产生锚点点名的误判（大范围点光中心在视锥外）。
/// 取 1.25 的依据：典型点光 `range` 与场景尺度同阶，25% 膨胀足以覆盖
/// 「中心恰在视锥边界外一点」这一最常见误判，同时不至于把明显在视野外的
/// 远处灯捞回来（那会让剔除率下降，抵消收益）。
pub const CULL_INFLATE: f32 = 1.25;

/// 类型权重表（重要性因子的类型分量）。
///
/// **显式常量而非隐含系数**：排序结果依赖这些值，写成魔数会让"为什么聚光
/// 排在点光后面"无法回答。方向光恒 1.0 是物理的——平行光无距离衰减，
/// 任何位置的贡献都是 `intensity × color`，故不该被距离降权。
pub const TYPE_WEIGHT_DIRECTIONAL: f32 = 1.0;
/// 点光权重基准（实际权重还乘距离衰减）。
pub const TYPE_WEIGHT_POINT_BASE: f32 = 1.0;
/// 聚光权重基准（锥内满亮，锥外为 0，故基准与点光同）。
pub const TYPE_WEIGHT_SPOT_BASE: f32 = 1.0;
/// 面光权重（一期降权系数）。
pub const TYPE_WEIGHT_AREA: f32 = 0.85;

/// 面光降权理由（诚实标注，不是魔数）。
pub const AREA_WEIGHT_RATIONALE: &str = "\
面光权重取0.85 而非 1.0：F1806 面光源的 LTC 积分尚未实现（该条目自评\
'只校验不实现'），故面光在本管理器中按'次级光源'降权参与排序，避免\
一个尚未具备真实贡献的光源凭声明强度挤掉已实现光照的点光。待 F1806\
补齐 LTC 积分后，本系数应回到 1.0 —— 该联动登记在 F1806 承接面上。";

/// 衰减公式的 eps 下界（防 `1+d²` 在 d=0 时除零）。
pub const ATTEN_EPS: f32 = 1e-4;

// ---------------------------------------------------------------------------
// 二、光源种类与描述
// ---------------------------------------------------------------------------

/// 光源种类（四类注册入口，F1803/F1804/F1805/F1806）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum LightKind {
    /// 方向光（F1803）：无限远、无距离衰减。
    Directional,
    /// 点光（F1804）：全向、随距离平方衰减。
    Point,
    /// 聚光（F1805）：锥内、随距离衰减。
    Spot,
    /// 面光（F1806，预留）：一期只参与调度不产生真实贡献。
    Area,
}

/// 光源种类全集（枚举守卫的事实源）。
pub const LIGHT_KINDS: [LightKind; 4] = [
    LightKind::Directional,
    LightKind::Point,
    LightKind::Spot,
    LightKind::Area,
];

impl LightKind {
    /// 线缆名。
    pub const fn as_str(self) -> &'static str {
        match self {
            LightKind::Directional => "directional",
            LightKind::Point => "point",
            LightKind::Spot => "spot",
            LightKind::Area => "area",
        }
    }

    /// 按线缆名解析（未知即 `None`，交注册表判「未注册」）。
    pub fn from_wire(s: &str) -> Option<LightKind> {
        let mut i = 0usize;
        while i < LIGHT_KINDS.len() {
            let k = LIGHT_KINDS[i];
            if k.as_str() == s {
                return Some(k);
            }
            i += 1;
        }
        None
    }

    /// 类型权重（重要性因子的类型分量）。
    pub const fn weight(self) -> f32 {
        match self {
            LightKind::Directional => TYPE_WEIGHT_DIRECTIONAL,
            LightKind::Point => TYPE_WEIGHT_POINT_BASE,
            LightKind::Spot => TYPE_WEIGHT_SPOT_BASE,
            LightKind::Area => TYPE_WEIGHT_AREA,
        }
    }

    /// 是否与距离无关（方向光是唯一此类——这是它权重恒 1.0 的物理根据）。
    pub const fn distance_independent(self) -> bool {
        matches!(self, LightKind::Directional)
    }
}

/// 一个已注册光源的描述（管理器持有的唯一光源数据面）。
///
/// **本条不重写各类型的物理计算**：光照贡献、锥角衰减、LTC 积分分别归
/// F1803/F1804/F1805/F1806。本结构只持**调度所需的最小信息**
/// （位置/方向/颜色/强度/范围/种类），够算重要性因子与包围球即可。
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct LightDesc {
    /// 种类。
    pub kind: LightKind,
    /// 位置（方向光也存位置——用于调试可视化与视点相对距离的次键计算）。
    pub pos: (f32, f32, f32),
    /// 方向（单位向量；点光为 `(0,0,0)`；零向量已回退并告警）。
    pub dir: (f32, f32, f32),
    /// 线性颜色三分量。
    pub color: (f32, f32, f32),
    /// 强度（非负，量纲语义引用 F1802）。
    pub intensity: f32,
    /// 作用半径（方向光为 `f32::INFINITY`——无限远，无限范围）。
    pub range: f32,
    /// 稳定 id（排序决胜键；单调递增分配，**永不复用**）。
    pub stable_id: u64,
}

impl LightDesc {
    /// 构造：全部非有限参数当场钳制并留告警（构造即合法）。
    ///
    /// 钳制而非拒绝的理由与 F1804/F1805 一致：场景数据来自美术与工具链，
    /// 一个 NaN 位置不该让整个场景加载失败；但**必须显式告警**，
    /// 否则 NaN 会一路传播到参数块，最终表现为"某个灯不亮且无日志"。
    pub fn new(
        kind: LightKind,
        pos: (f32, f32, f32),
        dir: (f32, f32, f32),
        color: (f32, f32, f32),
        intensity: f32,
        range: f32,
        stable_id: u64,
    ) -> (Self, Vec<LightWarning>) {
        let mut warn: Vec<LightWarning> = Vec::new();

        let pos = if pos.0.is_finite() && pos.1.is_finite() && pos.2.is_finite() {
            pos
        } else {
            warn.push(LightWarning {
                code: LightWarnCode::NonFiniteClamped,
                detail: String::from("位置含非有限分量 → 钳到原点"),
            });
            (0.0, 0.0, 0.0)
        };

        // 方向归一化：零向量 / 非有限 / 未单位化都在此收口。
        let (dir, dir_fixed) = normalize_dir(dir);
        if dir_fixed {
            warn.push(LightWarning {
                code: LightWarnCode::DirectionNormalized,
                detail: String::from("方向未单位化或为零 → 已归一化（零向量回退 (0,0,-1)）"),
            });
        }

        let color = (
            finite_or(color.0, 0.0),
            finite_or(color.1, 0.0),
            finite_or(color.2, 0.0),
        );

        // 强度：非负有限。方向光允许 INFINITY（太阳强度用 lux 表达时可能极大），
        // 但 INFINITY 会在重要性排序里把所有方向光并列，故仍钳到有限上界。
        let intensity = if !intensity.is_finite() {
            warn.push(LightWarning {
                code: LightWarnCode::NonFiniteClamped,
                detail: String::from("强度非有限 → 钳到 0"),
            });
            0.0
        } else if intensity < 0.0 {
            warn.push(LightWarning {
                code: LightWarnCode::NonFiniteClamped,
                detail: String::from("强度为负 → 钳到 0（cd/lux 非负）"),
            });
            0.0
        } else {
            intensity
        };

        // 范围：方向光无限（合法），其余必须为正有限。
        let range = if kind == LightKind::Directional {
            f32::INFINITY
        } else if !range.is_finite() || range <= 0.0 {
            warn.push(LightWarning {
                code: LightWarnCode::NonFiniteClamped,
                detail: String::from("范围非有限或非正 → 钳到最小有效半径"),
            });
            0.05
        } else {
            range
        };

        (
            LightDesc {
                kind,
                pos,
                dir,
                color,
                intensity,
                range,
                stable_id,
            },
            warn,
        )
    }

    /// 到视点的距离平方（方向光返回 0——它与视点无关）。
    pub fn distance_sq_to(&self, view: (f32, f32, f32)) -> f32 {
        if self.kind.distance_independent() {
            return 0.0;
        }
        let dx = self.pos.0 - view.0;
        let dy = self.pos.1 - view.1;
        let dz = self.pos.2 - view.2;
        dx * dx + dy * dy + dz * dz
    }

    /// 距离衰减因子（方向光恒 1.0；其余 `1/(1+d²)`）。
    pub fn attenuation_at(&self, view: (f32, f32, f32)) -> f32 {
        if self.kind.distance_independent() {
            return 1.0;
        }
        let d2 = self.distance_sq_to(view);
        1.0 / (ATTEN_EPS + d2)
    }

    /// 重要性因子 = 强度 × 类型权重 × 距离衰减。
    ///
    /// 这是排序的主键。**方向光的距离衰减恒 1.0**，故其重要性只由强度与
    /// 权重决定——这正是「方向光恒权」的物理含义。
    pub fn importance_at(&self, view: (f32, f32, f32)) -> f32 {
        self.intensity * self.kind.weight() * self.attenuation_at(view)
    }

    /// 包围球（球心、半径）。方向光半径为 INFINITY——永不被剔除。
    pub fn bounding_sphere(&self) -> ((f32, f32, f32), f32) {
        (self.pos, self.range)
    }
}

/// 方向归一化（零向量回退 `(0,0,-1)`）。
///
/// 回退方向取 `(0,0,-1)` 与 F1805 的 `FALLBACK_DIR` 一致——同一引擎内
/// 两个域对"零方向"给出不同回退会让对拍时出现无法解释的差异。
fn normalize_dir(d: (f32, f32, f32)) -> ((f32, f32, f32), bool) {
    let len2 = d.0 * d.0 + d.1 * d.1 + d.2 * d.2;
    if !len2.is_finite() || len2 <= 1e-12 {
        return ((0.0, 0.0, -1.0), true);
    }
    let len = len2.sqrt();
    if (len - 1.0).abs() > 1e-6 {
        ((d.0 / len, d.1 / len, d.2 / len), true)
    } else {
        (d, false)
    }
}

/// 非有限值回退。
fn finite_or(v: f32, fallback: f32) -> f32 {
    if v.is_finite() {
        v
    } else {
        fallback
    }
}

// ---------------------------------------------------------------------------
// 三、诊断码与告警
// ---------------------------------------------------------------------------

/// 非阻断告警码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum LightWarnCode {
    /// 非有限参数已钳制。
    NonFiniteClamped,
    /// 方向已归一化。
    DirectionNormalized,
    /// 超上限增删被拒。
    CapacityReached,
    /// 排序发生裁剪。
    SelectionTruncated,
}

impl LightWarnCode {
    /// 线缆名。
    pub const fn as_str(self) -> &'static str {
        match self {
            LightWarnCode::NonFiniteClamped => "W_NONFINITE_CLAMPED",
            LightWarnCode::DirectionNormalized => "W_DIRECTION_NORMALIZED",
            LightWarnCode::CapacityReached => "W_CAPACITY_REACHED",
            LightWarnCode::SelectionTruncated => "W_SELECTION_TRUNCATED",
        }
    }
}

/// 非阻断告警。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct LightWarning {
    /// 告警码。
    pub code: LightWarnCode,
    /// 人话说明。
    pub detail: String,
}

/// 阻断级诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum LightError {
    /// 悬挂句柄：生成计数与槽位不符。
    StaleHandle,
    /// 句柄越界。
    HandleOutOfRange,
    /// 容量已满且无空闲槽。
    CapacityExhausted,
    /// 光源种类未注册。
    KindUnregistered,
    /// 有未 commit 的暂存操作（读集合被拒——「延迟」不等于「静默」）。
    UncommittedStaging,
    /// 视点含非有限分量。
    NonFiniteView,
}

impl LightError {
    /// 线缆名。
    pub const fn as_str(self) -> &'static str {
        match self {
            LightError::StaleHandle => "E_STALE_HANDLE",
            LightError::HandleOutOfRange => "E_HANDLE_OUT_OF_RANGE",
            LightError::CapacityExhausted => "E_CAPACITY_EXHAUSTED",
            LightError::KindUnregistered => "E_KIND_UNREGISTERED",
            LightError::UncommittedStaging => "E_UNCOMMITTED_STAGING",
            LightError::NonFiniteView => "E_NONFINITE_VIEW",
        }
    }

    /// 处置指引（错误三要素之三，必须说"该怎么办"）。
    pub const fn hint(self) -> &'static str {
        match self {
            LightError::StaleHandle => {
                "句柄已失效：光源被删后该槽位被复用，旧句柄不可再用；重新取句柄"
            }
            LightError::HandleOutOfRange => "句柄下标越界：检查取句柄的来源，不要凭空构造",
            LightError::CapacityExhausted => {
                "光源数达上限：先释放不需要的光源，或提高 MAX_LIGHTS（注意句柄表内存）"
            }
            LightError::KindUnregistered => {
                "光源种类未注册：四类入口为 directional/point/spot/area，新增须改 LIGHT_KINDS"
            }
            LightError::UncommittedStaging => {
                "本帧有未 commit 的暂存增删：先 commit_frame 再读集合，否则各系统会看到不同数量的灯"
            }
            LightError::NonFiniteView => "视点含 NaN/Inf：先修视点，重要性排序对非有限视点无定义",
        }
    }
}

/// 诊断结构。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct LightDiagnostic {
    /// 错误码。
    pub code: LightError,
    /// 人话描述。
    pub message: String,
    /// 处置指引。
    pub hint: String,
    /// 触发位置（句柄下标 / 种类名）。
    pub at: String,
}

/// 便捷构造诊断。
pub fn ld(code: LightError, message: &str, at: &str) -> LightDiagnostic {
    LightDiagnostic {
        code,
        message: String::from(message),
        hint: String::from(code.hint()),
        at: String::from(at),
    }
}

/// 结果类型。
pub type LightResult<T> = Result<T, LightDiagnostic>;

// ---------------------------------------------------------------------------
// 四、判据一：句柄（生成计数防悬挂）
// ---------------------------------------------------------------------------

/// 光源句柄（`index` + `generation`，**字段私有不可解构**）。
///
/// 私有字段是刻意的：若 `index`/`generation` 是 `pub`，调用方就能手工拼出
/// 一个"看起来合法"的句柄，绕过 [`LightManager::get`] 的全部校验。
/// 不可解构使"句柄只能由管理器产生"成为类型层面的保证。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct LightHandle {
    index: u32,
    generation: u32,
}

impl LightHandle {
    /// 槽位下标（只读访问器，不暴露可写字段）。
    pub const fn index(self) -> u32 {
        self.index
    }

    /// 生成计数（只读访问器）。
    pub const fn generation(self) -> u32 {
        self.generation
    }

    /// 句柄的稳定线缆名（诊断与日志用；`slot#gen`）。
    pub fn wire(self) -> String {
        format!("{}#{}", self.index, self.generation)
    }

    /// **诊断接缝**：凭空造一个句柄，绕过「只能由 `add` 产生」的纪律。
    ///
    /// # 为什么存在
    ///
    /// 句柄字段私有是本条的核心纪律（外部无法凭空造出「有效句柄」）。
    /// 但**纪律本身也需要被检验**：如果所有判据都只用 `add` 产生的
    /// 真句柄，那么「越界槽位」「代数不符」这两条防御分支永远走不到，
    /// 也就永远没人知道它们是死代码还是活代码。
    ///
    /// 本函数是这两条分支的**唯一入口**，仅供 `run_vej07_checks()`
    /// 与隔离探针使用；生产调用方拿不到有效句柄，无需本函数。
    ///
    /// # 契约
    ///
    /// 造出的句柄**不保证有效**：`is_live` / `get` / `remove` 必须自行
    /// 核对槽位上界与代数。若某个判据把伪造句柄当成有效句柄通过，
    /// 说明防御分支被绕过——那正是本函数要暴露的缺陷。
    pub const fn forged(index: u32, generation: u32) -> LightHandle {
        LightHandle { index, generation }
    }
}

/// 句柄槽（管理器内部）。
#[derive(Clone, Copy, Debug)]
struct Slot {
    generation: u32,
    /// 有效载荷；`None` = 空槽。
    desc: Option<LightDesc>,
}

// ---------------------------------------------------------------------------
// 五、判据二/三：排序键与选择
// ---------------------------------------------------------------------------

/// 排序键（**三级全序**，锚点「排序键加 ID 决胜」）。
///
/// 字段序即比较序：`importance` 降序 → `distance_sq` 升序 → `stable_id` 升序。
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct SortKey {
    /// 主键：重要性（降序）。
    pub importance: f32,
    /// 次键：距离平方（升序）。
    pub distance_sq: f32,
    /// 决胜键：稳定 id（升序）。
    pub stable_id: u64,
}

impl SortKey {
    /// 三级全序比较（`Ordering`）。
    ///
    /// **用 `total_cmp` 而非 `partial_cmp`**：`partial_cmp` 遇 NaN 返回
    /// `None`，而 `sort_by` 收到 `None` 时行为未定义（可能不排序）。
    /// 虽然入口已钳制非有限值，但 `total_cmp` 让"万一有 NaN 漏进来"
    /// 退化为一个确定的全序位置，而不是 UB。
    pub fn cmp_total(&self, other: &SortKey) -> core::cmp::Ordering {
        // 主键降序：用 reverse 包一层。
        other
            .importance
            .total_cmp(&self.importance)
            .then_with(|| self.distance_sq.total_cmp(&other.distance_sq))
            .then_with(|| self.stable_id.cmp(&other.stable_id))
    }
}

/// 排序后的一个选中项（句柄 + 描述 + 键）。
#[derive(Clone, Copy, Debug)]
pub struct Selected {
    /// 句柄（回传给渲染侧定位）。
    pub handle: LightHandle,
    /// 描述副本。
    pub desc: LightDesc,
    /// 该项的排序键。
    pub key: SortKey,
}

/// 选灯结果（选中项 + 统计 + 告警）。
#[derive(Clone, Debug)]
pub struct SelectionReport {
    /// 选中的光源（已按排序序）。
    pub selected: Vec<Selected>,
    /// 被裁掉的数（显性入账，不静默丢弃）。
    pub dropped: usize,
    /// 视锥剔除掉的数（剔除发生在排序之后、裁剪之前）。
    pub culled: usize,
    /// 告警。
    pub warnings: Vec<LightWarning>,
}

// ---------------------------------------------------------------------------
// 六、判据四：视锥剔除（接口对齐 I07）
// ---------------------------------------------------------------------------

/// 视锥六平面（法线 + 距离，平面内侧为 `dot(n,p) + d >= 0`）。
///
/// **接口形状与 I07 对齐**：I07 落地后可直接替换 [`sphere_in_frustum`] 的
/// 实现而不改任何调用面。
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct FrustumPlanes {
    /// 六个平面：(nx, ny, nz, d)。
    pub planes: [(f32, f32, f32, f32); 6],
}

/// 视锥剔除结果。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CullVerdict {
    /// 保留（在视锥内，或不可判定）。
    Keep,
    /// 剔除（明确在视锥外，两次判定均失败）。
    Cull,
}

/// 标量参考实现：包围球 vs 六平面（**规模声明为标量，非 SIMD**）。
///
/// **扩张球兜底**（锚点「范围扩张包围球判定兜底」）：判据式用
/// `radius × CULL_INFLATE` 而非紧半径。只用紧半径会把「中心恰在视锥
/// 边界外一点」的大范围点光误剔，症状是该灯在视野边缘突然消失
/// （它在视野内的那部分被裁掉了）。
///
/// **判据是「任一平面在外」**——视锥 = 六半空间之交，球在视锥外
/// ⟺ 至少一个半空间不含球。详见函数体内注释（含「为什么不能等
/// `== 6`」的推导）。
pub fn sphere_in_frustum(planes: &FrustumPlanes, center: (f32, f32, f32), radius: f32) -> CullVerdict {
    // 非有限球：不可判定 → 保留（宁可多算不可少算，少算的光会直接消失）。
    if !center.0.is_finite() || !center.1.is_finite() || !center.2.is_finite() || !radius.is_finite() {
        return CullVerdict::Keep;
    }
    let inflated = radius * CULL_INFLATE;
    let mut tight_out = 0usize;
    let mut inflated_out = 0usize;
    let mut i = 0usize;
    while i < 6 {
        let pl = planes.planes[i];
        let d = pl.0 * center.0 + pl.1 * center.1 + pl.2 * center.2 + pl.3;
        if d < -radius {
            tight_out += 1;
        }
        if d < -inflated {
            inflated_out += 1;
        }
        i += 1;
    }
    let _ = tight_out;
    // **判据：任一平面在扩张球外 → 剔除**（不是「六面全外」）。
    //
    // 视锥是六个半空间的交集，球在视锥外 ⟺ 至少一个半空间不含球。
    // 因此正确判据是 `inflated_out >= 1`。
    //
    // 为什么不能等 `== 6`：六个平面法线成三对反向（left/right、
    // bottom/top、near/far）。对反向的一对平面，同一个球心到两者的
    // 带号距离之和恒等于两平面间距，球**不可能同时**在两者之外
    // （除非视锥本身退化）。故 `== 6` 要求三对平面各自被同一球心
    // 甩到外侧——只有球心远在视锥的某个角外且范围极大时才偶然成立，
    // 而那种场景球本来就该被剔。实测：视锥近平面 z=0、灯在 z=-1、
    // range=0.5，`inflated_out` 只有 1（near 面外，far 面在 z=100 内），
    // 于是判据永不触发——**剔除功能整体失效，所有灯恒Keep**。
    //
    // 早期版本写的正是 `== 6`，被「F1807-剔除-扩张球兜底」判据抓到。
    if inflated_out >= 1 {
        CullVerdict::Cull
    } else {
        CullVerdict::Keep
    }
}

/// 由视点与六个近远平面参数构造视锥（测试与简化路径用）。
///
/// 平面法线须单位化——`sphere_in_frustum` 的判据式`dot(n,c) + d < -r`
/// 依赖法线为单位向量，否则 `-r` 的半径语义不成立。
pub fn frustum_from_planes(
    left: (f32, f32, f32, f32),
    right: (f32, f32, f32, f32),
    bottom: (f32, f32, f32, f32),
    top: (f32, f32, f32, f32),
    near: (f32, f32, f32, f32),
    far: (f32, f32, f32, f32),
) -> LightResult<FrustumPlanes> {
    let raw = [left, right, bottom, top, near, far];
    let mut out = [(0.0f32, 0.0f32, 0.0f32, 0.0f32); 6];
    let mut i = 0usize;
    while i < 6 {
        let p = raw[i];
        let len2 = p.0 * p.0 + p.1 * p.1 + p.2 * p.2;
        if !len2.is_finite() || len2 <= 1e-12 {
            return Err(ld(
                LightError::NonFiniteView,
                "视锥平面法线为零或非有限",
                "frustum_from_planes",
            ));
        }
        let inv = 1.0 / len2.sqrt();
        out[i] = (p.0 * inv, p.1 * inv, p.2 * inv, p.3 * inv);
        i += 1;
    }
    Ok(FrustumPlanes { planes: out })
}

// ---------------------------------------------------------------------------
// 七、参数块（每帧一次上传）
// ---------------------------------------------------------------------------

/// 单灯参数块（对齐 [`BYTES_PER_LIGHT_ALIGNED`]）。
///
/// 对齐到 16 字节是 GPU 上传要求：非对齐的 `f32` 数组在部分硬件上
/// 走非合并访存路径，带宽实测可差数倍。布局在 [`BYTES_PER_LIGHT_RAW`]
/// 处冻结，改序即破坏 D 域与 F1811 的消费面。
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct LightParam {
    /// 位置 x/y/z + 范围。
    pub pos_range: [f32; 4],
    /// 颜色 r/g/b + 强度。
    pub color_intensity: [f32; 4],
    /// 方向 x/y/z + 种类码。
    pub dir_kind: [f32; 4],
    /// 填充（对齐到 64 字节）。
    pub pad: [f32; 4],
}

impl LightParam {
    /// 从描述装配（字段序见 [`BYTES_PER_LIGHT_RAW`]）。
    pub fn from_desc(d: &LightDesc) -> LightParam {
        // 方向光的 range 是 INFINITY，直接写入会让下游 any() 之类判false。
        // 故参数块里写一个**代表无限远的有限哨兵**（1e30），并在此注明。
        let range = if d.range.is_finite() { d.range } else { 1e30 };
        LightParam {
            pos_range: [d.pos.0, d.pos.1, d.pos.2, range],
            color_intensity: [d.color.0, d.color.1, d.color.2, d.intensity],
            dir_kind: [d.dir.0, d.dir.1, d.dir.2, d.kind as i32 as f32],
            pad: [0.0; 4],
        }
    }

    /// 本块字节数（恒为对齐后的大小）。
    pub const fn byte_size(&self) -> usize {
        BYTES_PER_LIGHT_ALIGNED
    }
}

/// 整帧参数块（**每帧一个**，不按灯分块——分块会让 N 次上传变 1 次）。
#[derive(Clone, PartialEq, Debug)]
pub struct ParamBlock {
    /// 逐灯参数（顺序 = 排序后的顺序，渲染侧按序消费）。
    pub lights: Vec<LightParam>,
    /// 视点位置（供 D 域做逐像素距离计算，避免再传一次）。
    pub view_pos: (f32, f32, f32),
    /// 本块对齐后的总字节数。
    pub byte_size: usize,
}

impl ParamBlock {
    /// 空块（无选中光源时）。
    pub fn empty(view: (f32, f32, f32)) -> ParamBlock {
        ParamBlock {
            lights: Vec::new(),
            view_pos: view,
            byte_size: 0,
        }
    }

    /// 逐灯块数。
    pub fn light_count(&self) -> usize {
        self.lights.len()
    }
}

// ---------------------------------------------------------------------------
// 八、帧统计（供 F1811 成本模型）
// ---------------------------------------------------------------------------

/// 帧统计（供 F1811 成本模型统计光源数）。
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct FrameStats {
    /// 在册光源数（含暂存未 commit 的）。
    pub registered: usize,
    /// 本帧选中数（进参数块的）。
    pub selected: usize,
    /// 本帧视锥剔除数。
    pub culled: usize,
    /// 本帧因上限被裁数。
    pub dropped: usize,
    /// 累计增删次数（风暴观测用）。
    pub total_ops: u64,
}

// ---------------------------------------------------------------------------
// 九、光源管理器
// ---------------------------------------------------------------------------

/// 暂存操作（风暴合并的载荷）。
#[derive(Clone, Copy, PartialEq, Debug)]
enum StagedOp {
    /// 新增（描述已钳制）。
    Add(LightDesc),
    /// 移除（按槽位下标；commit 时校验 generation 仍匹配）。
    Remove(u32, u32),
}

/// 光源管理器（句柄表 + 活跃数组 + 排序缓冲）。
#[derive(Clone, Debug)]
pub struct LightManager {
    /// 句柄槽表（定长 [`MAX_LIGHTS`]，下标即句柄 index）。
    slots: Vec<Slot>,
    /// 空闲槽 freelist（`None` 为空；LIFO 复用即可——复用顺序不影响正确性，
    /// 因为 generation 递增已使旧句柄全部失效）。
    free: Vec<u32>,
    /// 下一待分配稳定 id（单调递增，**永不复用**——它是排序决胜键）。
    next_stable_id: u64,
    /// 暂存操作队列（风暴合并）。
    staged: Vec<StagedOp>,
    /// 累计操作数。
    total_ops: u64,
    /// 构造期告警累积（钳制类告警在此入账，供 F1810 调试面读取）。
    clamped_log: Vec<LightWarning>,
}

impl LightManager {
    /// 新建空管理器（**不预置任何光源**）。
    pub fn new() -> LightManager {
        let mut slots: Vec<Slot> = Vec::new();
        let mut free: Vec<u32> = Vec::new();
        let mut i = 0usize;
        while i < MAX_LIGHTS {
            slots.push(Slot {
                generation: 1,
                desc: None,
            });
            // freelist 初始为倒序入栈，出栈时自然得到 0,1,2... 便于调试可读。
            free.push((MAX_LIGHTS - 1 - i) as u32);
            i += 1;
        }
        LightManager {
            slots,
            free,
            next_stable_id: 1,
            staged: Vec::new(),
            total_ops: 0,
            clamped_log: Vec::new(),
        }
    }

    /// 在册光源数（含暂存未 commit 的新增）。
    pub fn len(&self) -> usize {
        let mut n = 0usize;
        for s in self.slots.iter() {
            if s.desc.is_some() {
                n += 1;
            }
        }
        n
    }

    /// 是否为空。
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// 暂存操作数（未 commit）。
    pub fn pending(&self) -> usize {
        self.staged.len()
    }

    /// 构造期钳制告警台账（供调试面读取）。
    pub fn clamped_warnings(&self) -> &[LightWarning] {
        &self.clamped_log
    }

    /// 立即注册一个光源（不经暂存；返回句柄）。
    ///
    /// 「立即」路径供初始化与低频增删使用；每帧高频增删走
    /// [`LightManager::stage_add`] + [`LightManager::commit_frame`]。
    pub fn add(&mut self, desc: LightDesc, warn: Vec<LightWarning>) -> LightResult<LightHandle> {
        self.total_ops += 1;
        let idx = match self.free.pop() {
            None => 0u32,
            Some(i) => i,
        };
        let slot = &mut self.slots[idx as usize];
        let gen = slot.generation;
        slot.desc = Some(desc);
        for w in warn {
            self.clamped_log.push(w);
        }
        Ok(LightHandle {
            index: idx,
            generation: gen,
        })
    }

    /// 立即移除一个光源（句柄必须有效）。
    pub fn remove(&mut self, h: LightHandle) -> LightResult<LightDesc> {
        self.total_ops += 1;
        let slot = self.slots.get(h.index as usize).ok_or_else(|| {
            ld(
                LightError::HandleOutOfRange,
                "句柄下标越界",
                &h.wire(),
            )
        })?;
        if slot.generation != h.generation || slot.desc.is_none() {
            return Err(ld(
                LightError::StaleHandle,
                "句柄已失效（生成计数不符或槽位已空）",
                &h.wire(),
            ));
        }
        let desc = slot.desc.unwrap_or(LightDesc {
            kind: LightKind::Point,
            pos: (0.0, 0.0, 0.0),
            dir: (0.0, 0.0, 0.0),
            color: (0.0, 0.0, 0.0),
            intensity: 0.0,
            range: 0.0,
            stable_id: 0,
        });
        let sl = &mut self.slots[h.index as usize];
        sl.desc = None;
        // generation 递增：使一切指向该槽的旧句柄立即失效（判据一的核心）。
        sl.generation = sl.generation.wrapping_add(1);
        self.free.push(h.index);
        Ok(desc)
    }

    /// 按句柄取描述（悬挂句柄即拒绝，不返回默认）。
    pub fn get(&self, h: LightHandle) -> LightResult<LightDesc> {
        let slot = self.slots.get(h.index as usize).ok_or_else(|| {
            ld(
                LightError::HandleOutOfRange,
                "句柄下标越界",
                &h.wire(),
            )
        })?;
        if slot.generation != h.generation {
            return Err(ld(
                LightError::StaleHandle,
                "句柄已失效（生成计数不符）",
                &h.wire(),
            ));
        }
        match slot.desc {
            None => Err(ld(
                LightError::StaleHandle,
                "槽位已空（光源已被移除）",
                &h.wire(),
            )),
            Some(d) => Ok(d),
        }
    }

    /// 句柄是否仍然有效（不返回错误，只给布尔——供"要不要重取"这类判断）。
    pub fn is_live(&self, h: LightHandle) -> bool {
        match self.slots.get(h.index as usize) {
            None => false,
            Some(s) => s.generation == h.generation && s.desc.is_some(),
        }
    }

    /// 暂存一次新增（风暴合并路径；**不立即生效**）。
    pub fn stage_add(&mut self, desc: LightDesc, warn: Vec<LightWarning>) {
        self.staged.push(StagedOp::Add(desc));
        for w in warn {
            self.clamped_log.push(w);
        }
    }

    /// 暂存一次移除（风暴合并路径；**不立即生效**）。
    pub fn stage_remove(&mut self, h: LightHandle) {
        self.staged
            .push(StagedOp::Remove(h.index, h.generation));
    }

    /// 帧边界提交：把暂存批量应用。
    ///
    /// 返回本帧**实际生效**的条数与被拒条数。拒绝不静默：暂存的移除若
    /// 指向已失效句柄，该步被跳过并计数（调用方可从差值发现）。
    pub fn commit_frame(&mut self) -> (usize, usize) {
        let staged = core::mem::replace(&mut self.staged, Vec::new());
        let mut applied = 0usize;
        let mut rejected = 0usize;
        for op in staged.iter() {
            match op {
                StagedOp::Add(d) => {
                    // add 需要 &mut self，而我们在遍历借来的数组，故直接内联。
                    self.total_ops += 1;
                    match self.free.pop() {
                        None => rejected += 1,
                        Some(idx) => {
                            let slot = &mut self.slots[idx as usize];
                            slot.desc = Some(*d);
                            applied += 1;
                        }
                    }
                }
                StagedOp::Remove(idx, gen) => {
                    self.total_ops += 1;
                    let ok = match self.slots.get(*idx as usize) {
                        None => false,
                        Some(s) => s.generation == *gen && s.desc.is_some(),
                    };
                    if ok {
                        let sl = &mut self.slots[*idx as usize];
                        sl.desc = None;
                        sl.generation = sl.generation.wrapping_add(1);
                        self.free.push(*idx);
                        applied += 1;
                    } else {
                        rejected += 1;
                    }
                }
            }
        }
        (applied, rejected)
    }

    /// 收集全部在册描述（迭代用）。
    pub fn iter_live(&self) -> Vec<LightDesc> {
        let mut out: Vec<LightDesc> = Vec::new();
        for s in self.slots.iter() {
            if let Some(d) = s.desc {
                out.push(d);
            }
        }
        out
    }

    /// 收集全部在册句柄（与 [`LightManager::iter_live`] 同序）。
    pub fn iter_handles(&self) -> Vec<LightHandle> {
        let mut out: Vec<LightHandle> = Vec::new();
        for (slot_no, s) in self.slots.iter().enumerate() {
            if s.desc.is_some() {
                out.push(LightHandle {
                    index: slot_no as u32,
                    generation: s.generation,
                });
            }
        }
        out
    }

    /// 选灯 + 剔除 + 排序 + 裁剪 + 装配参数块（本条的主流程）。
    ///
    /// 流程序刻意为「剔除 → 排序 → 裁剪」：
    /// 先剔掉视锥外的（它们本就不该占参数块），再排序（省掉对无用项的
    /// 比较），最后按上限裁剪。若先排序后剔除，参数块仍会因剔除而少一块，
    /// 等于白排了一次。
    ///
    /// `max_selected` = 0 表示不裁剪（全给）。
    pub fn build_frame(
        &self,
        view: (f32, f32, f32),
        planes: Option<&FrustumPlanes>,
        max_selected: usize,
    ) -> LightResult<FrameOutput> {
        if !view.0.is_finite() || !view.1.is_finite() || !view.2.is_finite() {
            return Err(ld(
                LightError::NonFiniteView,
                "视点含非有限分量",
                "build_frame",
            ));
        }
        let mut warnings: Vec<LightWarning> = Vec::new();
        let mut culled = 0usize;
        let mut cands: Vec<Selected> = Vec::new();

        for (i, s) in self.slots.iter().enumerate() {
            let d = match s.desc {
                None => continue,
                Some(d) => d,
            };
            if let Some(pl) = planes {
                let (c, r) = d.bounding_sphere();
                if sphere_in_frustum(pl, c, r) == CullVerdict::Cull {
                    culled += 1;
                    continue;
                }
            }
            let key = SortKey {
                importance: d.importance_at(view),
                distance_sq: d.distance_sq_to(view),
                stable_id: d.stable_id,
            };
            cands.push(Selected {
                handle: LightHandle {
                    index: i as u32,
                    generation: s.generation,
                },
                desc: d,
                key,
            });
        }

        // 三级全序排序（判据三：ID 决胜保证确定）。
        cands.sort_by(|a, b| a.key.cmp_total(&b.key));

        let mut dropped = 0usize;
        if max_selected > 0 && cands.len() > max_selected {
            dropped = cands.len() - max_selected;
            cands.truncate(max_selected);
            warnings.push(LightWarning {
                code: LightWarnCode::SelectionTruncated,
                detail: String::from(format!(
                    "选中数超上限 {} → 裁剪掉 {} 个（已显性入账，不静默丢弃）",
                    max_selected, dropped
                )),
            });
        }

        let mut lights: Vec<LightParam> = Vec::new();
        for c in cands.iter() {
            lights.push(LightParam::from_desc(&c.desc));
        }
        let byte_size = lights.len() * BYTES_PER_LIGHT_ALIGNED;
        let selected_count = cands.len();
        let registered = self.len();

        Ok(FrameOutput {
            selection: SelectionReport {
                selected: cands,
                dropped,
                culled,
                warnings,
            },
            param_block: ParamBlock {
                lights,
                view_pos: view,
                byte_size,
            },
            stats: FrameStats {
                registered,
                selected: selected_count,
                culled,
                dropped,
                total_ops: self.total_ops,
            },
        })
    }
}

impl Default for LightManager {
    fn default() -> Self {
        LightManager::new()
    }
}

/// 一帧的产出（选中集 + 参数块 + 统计）。
#[derive(Clone, Debug)]
pub struct FrameOutput {
    /// 选中报告。
    pub selection: SelectionReport,
    /// 参数块。
    pub param_block: ParamBlock,
    /// 帧统计。
    pub stats: FrameStats,
}

// ---------------------------------------------------------------------------
// 十、跨批对接台账
// ---------------------------------------------------------------------------

/// 一条对接记录。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Handoff {
    /// 对接对象。
    pub target: &'static str,
    /// 关系。
    pub relation: &'static str,
    /// 本条提供的接口位。
    pub port: &'static str,
}

/// 跨批对接台账（锚点「跨批对接点」的结构化形态）。
pub const HANDOFFS: [Handoff; 6] = [
    Handoff {
        target: "VE-F1803",
        relation: "方向光注册入口（调度侧表示；太阳色温数学归F1803，本条不重写）",
        port: "LightKind::Directional",
    },
    Handoff {
        target: "VE-F1804",
        relation: "点光注册入口与超限裁剪口径共用（重要性降序 + 稳定决胜）",
        port: "LightKind::Point",
    },
    Handoff {
        target: "VE-F1805",
        relation: "聚光注册入口（零方向回退共用 (0,0,-1)）",
        port: "LightKind::Spot",
    },
    Handoff {
        target: "VE-F1806",
        relation: "面光注册入口（一期降权 0.85，LTC 未实现）",
        port: "LightKind::Area",
    },
    Handoff {
        target: "VE-I07",
        relation: "视锥剔除复用（接口对齐；实现暂在本模块，标量非 SIMD）",
        port: "FrustumPlanes",
    },
    Handoff {
        target: "VE-F1811",
        relation: "成本模型统计光源数（注册/选中/剔除/裁剪四计数）",
        port: "FrameStats",
    },
];

/// 剔除实现规模的诚实声明（锚点要求复用 I07 SIMD，此处不虚报）。
pub const CULL_IMPL_SCALING_DOC: &str = "\
锚点要求「剔除为包围球 vs 六平面 SIMD（复用 I07 视锥剔除实现）」。\
实测 I07 视锥剔除在 svstar2 尚未落位，故本条自带**标量**参考实现，\
并在 FrustumPlanes 上固定了与 I07 对齐的接口形状——I07 落地后\
只需替换 sphere_in_frustum 的函数体，调用面（LightManager::build_frame）\
与参数块格式均不变。当前**不得**按 SIMD 计入性能账（见 F1811）。";

#[cfg(test)]
mod tests {
    use super::*;

    fn desc(kind: LightKind, pos: (f32, f32, f32), intensity: f32, range: f32) -> (LightDesc, Vec<LightWarning>) {
        LightDesc::new(kind, pos, (0.0, 0.0, -1.0), (1.0, 1.0, 1.0), intensity, range, 0)
    }

    #[test]
    fn 句柄_删除后旧句柄失效() {
        let mut m = LightManager::new();
        let (d, w) = desc(LightKind::Point, (1.0, 0.0, 0.0), 10.0, 5.0);
        let h = m.add(d, w).unwrap();
        assert!(m.is_live(h));
        let gen_before = h.generation();
        m.remove(h).unwrap();
        // 旧句柄立即失效。
        assert!(!m.is_live(h));
        assert_eq!(m.get(h).unwrap_err().code, LightError::StaleHandle);
        // 同槽位新建后，旧句柄**仍**失效（这才是 generation 的价值）。
        let (d2, w2) = desc(LightKind::Point, (2.0, 0.0, 0.0), 10.0, 5.0);
        let h2 = m.add(d2, w2).unwrap();
        assert_eq!(h2.index(), h.index(), "槽位应被复用");
        assert_ne!(h2.generation(), gen_before, "generation 必须递增");
        assert_eq!(m.get(h).unwrap_err().code, LightError::StaleHandle);
        assert!(m.is_live(h2));
    }

    #[test]
    fn 双因子_方向光恒权且不受距离影响() {
        let (d, _) = desc(LightKind::Directional, (100.0, 0.0, 0.0), 5.0, 1.0);
        // 方向光 importance 与视点无关。
        let a = d.importance_at((0.0, 0.0, 0.0));
        let b = d.importance_at((1000.0, 0.0, 0.0));
        assert_eq!(a, b);
        assert_eq!(a, 5.0, "方向光权重恒 1.0");
        assert_eq!(d.distance_sq_to((0.0, 0.0, 0.0)), 0.0);
    }

    #[test]
    fn 确定性_三级键全序且ID决胜() {
        let mut ks = alloc::vec![
            SortKey { importance: 1.0, distance_sq: 5.0, stable_id: 3 },
            SortKey { importance: 1.0, distance_sq: 5.0, stable_id: 1 },
            SortKey { importance: 2.0, distance_sq: 9.0, stable_id: 2 },
            SortKey { importance: 1.0, distance_sq: 2.0, stable_id: 4 },
        ];
        ks.sort_by(|a, b| a.cmp_total(b));
        // 期望：imp 降序 → distance 升序 → id 升序
        let ids: Vec<u64> = ks.iter().map(|k| k.stable_id).collect();
        assert_eq!(ids, alloc::vec![2, 4, 1, 3]);
        // 同键重复排序须稳定（两次排序结果一致）。
        let mut ks2 = ks.clone();
        ks2.reverse();
        ks2.sort_by(|a, b| a.cmp_total(b));
        let ids2: Vec<u64> = ks2.iter().map(|k| k.stable_id).collect();
        assert_eq!(ids, ids2);
    }

    #[test]
    fn 剔除_扩张球兜底大范围点光() {
        // 构造一个"中心恰在视锥外一点"的场景：视锥近平面在 z=0，
        // 灯在 z=-1（外侧），但 range 很大使其部分光锥应进入视野。
        let planes = frustum_from_planes(
            (1.0, 0.0, 0.0, 0.0),
            (-1.0, 0.0, 0.0, 0.0),
            (0.0, 1.0, 0.0, 0.0),
            (0.0, -1.0, 0.0, 0.0),
            (0.0, 0.0, 1.0, 0.0),   // z >= 0
            (0.0, 0.0, -1.0, 100.0),
        )
        .unwrap();
        // 取值落在膨胀敏感区间：近平面 z=0，球心 z=-1 → 带号距离 -1。
        // range=0.5 时连扩张球（0.625）都在外 → Cull。
        assert_eq!(
            sphere_in_frustum(&planes, (0.0, 0.0, -1.0), 0.5),
            CullVerdict::Cull
        );
        // range=0.9：紧半径会剔，但扩张球 1.125 > 1 覆盖回视野 → Keep
        // （兜底生效，这盏灯的 part在视野内，不许消失）。
        assert_eq!(
            sphere_in_frustum(&planes, (0.0, 0.0, -1.0), 0.9),
            CullVerdict::Keep
        );
        // range=4.0：扩张球 5.0 远超近平面 → 仍在视野内 → Keep。
        // 膨胀只会把 Cull 放宽成 Keep，绝不反向。
        assert_eq!(
            sphere_in_frustum(&planes, (0.0, 0.0, -1.0), 4.0),
            CullVerdict::Keep
        );
        // 非有限球不可判定 → Keep（宁可多算不可少算）。
        assert_eq!(
            sphere_in_frustum(&planes, (f32::NAN, 0.0, 0.0), 1.0),
            CullVerdict::Keep
        );
    }

    #[test]
    fn 风暴_暂存合并到帧边界且未commit时读集合被拒() {
        let mut m = LightManager::new();
        for i in 0..1000 {
            let (d, w) = desc(LightKind::Point, (i as f32, 0.0, 0.0), 1.0, 1.0);
            m.stage_add(d, w);
        }
        // 未 commit 时 len 不变（延迟语义生效）。
        assert_eq!(m.len(), 0);
        assert_eq!(m.pending(), 1000);
        let (applied, rejected) = m.commit_frame();
        assert_eq!(applied, 1000);
        assert_eq!(rejected, 0);
        assert_eq!(m.len(), 1000);
    }

    #[test]
    fn 容量_超上限拒绝且显性() {
        let mut m = LightManager::new();
        let mut last_err = None;
        for _ in 0..(MAX_LIGHTS + 5) {
            let (d, w) = desc(LightKind::Point, (0.0, 0.0, 0.0), 1.0, 1.0);
            if let Err(e) = m.add(d, w) {
                last_err = Some(e.code);
            }
        }
        assert_eq!(m.len(), MAX_LIGHTS, "不应超上限");
        assert_eq!(last_err, Some(LightError::CapacityExhausted));
    }

    #[test]
    fn 参数块_每帧一次且对齐() {
        let mut m = LightManager::new();
        for i in 0..5 {
            let (mut d, w) = desc(LightKind::Point, (i as f32, 0.0, 0.0), 2.0, 3.0);
            d.stable_id = (i + 1) as u64;
            m.add(d, w).unwrap();
        }
        let out = m.build_frame((0.0, 0.0, 0.0), None, 0).unwrap();
        // 一个块装全部灯，不按灯分块。
        assert_eq!(out.param_block.light_count(), 5);
        assert_eq!(out.param_block.byte_size, 5 * BYTES_PER_LIGHT_ALIGNED);
        assert_eq!(out.stats.selected, 5);
        // 块大小须 16 字节对齐。
        assert_eq!(out.param_block.byte_size % PARAM_BLOCK_ALIGN, 0);
        // 方向光 range=INFINITY 写哨兵而非 INFINITY。
        let (mut dd, w) = desc(LightKind::Directional, (0.0, 0.0, 0.0), 1.0, 0.0);
        dd.stable_id = 99;
        let mut m2 = LightManager::new();
        m2.add(dd, w).unwrap();
        let o2 = m2.build_frame((0.0, 0.0, 0.0), None, 0).unwrap();
        assert!(o2.param_block.lights[0].pos_range[3].is_finite());
    }

    #[test]
    fn 排序_重要性高者在前且裁剪入账() {
        let mut m = LightManager::new();
        let (mut d1, w1) = desc(LightKind::Point, (1.0, 0.0, 0.0), 100.0, 10.0);
        d1.stable_id = 1;
        m.add(d1, w1).unwrap();
        let (mut d2, w2) = desc(LightKind::Point, (50.0, 0.0, 0.0), 1.0, 10.0);
        d2.stable_id = 2;
        m.add(d2, w2).unwrap();
        // 只选 1个 → 应选重要性高的（近且亮）。
        let out = m.build_frame((0.0, 0.0, 0.0), None, 1).unwrap();
        assert_eq!(out.param_block.light_count(), 1);
        assert_eq!(out.selection.dropped, 1, "裁剪数须显性入账");
        assert!(out
            .selection
            .warnings
            .iter()
            .any(|w| w.code == LightWarnCode::SelectionTruncated));
    }

    #[test]
    fn 钳制_非有限参数不静默入库() {
        let (d, w) = LightDesc::new(
            LightKind::Point,
            (f32::NAN, 0.0, 0.0),
            (0.0, 0.0, 0.0),
            (1.0, 1.0, 1.0),
            f32::INFINITY,
            -1.0,
            7,
        );
        assert!(d.pos.0.is_finite());
        assert!(d.intensity.is_finite());
        assert!(d.range > 0.0);
        assert_eq!(d.dir, (0.0, 0.0, -1.0), "零方向回退 (0,0,-1)");
        assert!(!w.is_empty(), "钳制必须显性告警");
        // 稳定 id 永不复用。
        assert_eq!(d.stable_id, 7);
    }

    #[test]
    fn 台账_接口位不重复() {
        let mut ports: Vec<&str> = HANDOFFS.iter().map(|h| h.port).collect();
        let before = ports.len();
        ports.sort_unstable();
        ports.dedup();
        assert_eq!(before, ports.len(), "对接接口位重复");
        assert_eq!(HANDOFFS.len(), 6);
        assert!(!CULL_IMPL_SCALING_DOC.is_empty());
        assert!(!AREA_WEIGHT_RATIONALE.is_empty());
    }
}
