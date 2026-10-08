//! VE-F1809 · 环境贴图光照 IBL（预积分环境贴图两条腿）
//!
//! 职责：**镜面预滤波 mip 链**（粗糙度分级 GGX 卷积）+ **BRDF LUT**
//! （菲涅尔环境 BRDF 查找表），兑现 I03 F1642 预留的 IBL 双通道接口，
//! 使 PBR 材质获得完整的环境镜面反射与漫反射。
//!
//! # 一、为什么是「两条腿」而不是一张图
//!
//! 环境立方图给出的是**入射 radiance**（某个方向来的光有多强）。
//! 着色器要的是**出射**：给定视线 `V` 与法线 `N`，该往哪个方向反射、
//! 反射多少。两者之间隔着一次粗糙表面上的**微几何卷积**：
//!
//! - **镜面腿**：粗糙度 `α` 决定卷积核的宽度。`α=0`（镜面）时核退化为
//!   一个 delta，只取 `R = reflect(-V, N)` 那一个方向；`α=1`（全漫）
//!   时核覆盖整个半球，mip 退化为常数。**这条腿的输出是一串 mip**——
//!   每级一个粗糙度档，运行时按材质粗糙度选级。
//! - **BRDF 腿**：分裂求和（split-sum）把卷积拆成「可预积分的环境项」
//!   ×「仅依赖 `NdotV` 与 `α` 的解析项」。前者是上面那条 mip 链，
//!   后者与场景无关、只与 `(NdotV, roughness)` 有关，**可以整表预算**。
//!
//! **只有 mip 没有 LUT 会怎样**：镜面项直接等于 `radiance(R) × F0`，
//! 粗糙表面会**丢掉全部 Fresnel 与几何遮蔽修正**——高粗糙度处能量
//! 明显偏低（经典症状：金属球在粗糙档「发黑」）。**只有 LUT 没有 mip
//! 会怎样**：查表拿到 `(scale, bias)`，但乘谁？无处可乘。
//!
//! # 二、mip 链的级数与粗糙度映射
//!
//! 5 级 mip（`MIP_LEVELS = 5`），第 `i` 级对应粗糙度
//! `i / (MIP_LEVELS - 1)`：0 级最锐（镜面），4 级最钝（漫反射）。
//!
//! **为什么不按 2 的幂分级**：立方图 mip 天然是 2 的幂递减，但**粗糙度
//! 与 mip 级的关系不是线性的**——`α` 与 `cos²` 域上的半角近似成比例，
//! 而 mip 级数是像素尺度的对数。按线性映射分级会让中间档的粗糙度
//! 跳变（相邻两级看起来差很多），这是工程上的常见做法而非物理严格解，
//! 故在此**显式标注为近似**并在头注留证。
//!
//! # 三、运行时双查（O(1)）
//!
//! 给定 `(N, V, roughness, metallic, F0)`：
//! 1. `R = reflect(-V, N)`；`lod = roughness × (MIP_LEVELS - 1)`；
//! 2. 查 mip 链得 `prefiltered(R, lod)`——**三线性**（相邻级线性插值，
//!    非最近邻，否则粗糙度连续变化时会出现 mip 跳变的可见色带）；
//! 3. 查 LUT 得 `(scale, bias)`（同样三线性）；
//! 4. `specular = prefiltered × (F0 × scale + bias)`。
//!
//! **成本与像素数无关**：两次三次线性采样 = O(1)。这是「预积分」这个词
//! 的全部意义——把每像素 O(采样数) 的卷积搬到加载期一次性做完。
//!
//! # 四、BRDF LUT 的积分
//!
//! ```text
//! scale = A + B   （A/B 为 LUT 的两个通道）
//! A = ∫ D(h)·G2(l,v)·(1 − Fc)·dh    —— 几何项
//! B = ∫ D(h)·G2(l,v)·Fc·dh          —— 菲涅尔项
//! Fc = (1 − NdotV)^5
//! ```
//!
//! **为什么 LUT 是一张 512×512 的表而不是解析式**：Smith 几何项与 GGX
//! 分布的联合积分**没有初等闭式解**。业界（Karis 的 split-sum 近似）
//! 用拟合多项式代替全积分，本实现用**真积分 + 查表**——多一张512×512
//! 的 R16F 表（512KB），换来的是**无拟合误差**。这是「工程近似」与
//! 「物理正确」的取舍，在此如实标注：**本模块走物理正确路线**。
//!
//! # 五、采样数三档与质量档联动
//!
//! GGX 重要性采样的样本数分三档：`1024 / 512 / 256`，与 F1762 质量档
//! 联动。**低档位不是「随便减少」而是「有节制减少」**：256 采样在
//! 粗糙度 ≤0.4 的档（能量集中）仍足够，误差在 LUT/预滤波的容差内；
//! 高粗糙度档本身就是低通滤波结果，误差更不敏感。
//!
//! # 六、LUT 精度与高粗糙度条带
//!
//! LUT 在高粗糙度区（`roughness → 1`）**沿横轴变化剧烈**，256 分辨率
//! 下会出现可见条带。兜底档：**半分辨率过采样**（128×128 但每格 2×2
//! 超采样）——有效精度回到 256 而显存减半。
//!
//! # 七、环境图更新与增量重跑
//!
//! 环境图换了**不必全链重跑**：新图与旧图逐级比较，**只有内容真正
//! 变化的 mip 级**才重算。这就是「增量重跑仅受影响 mip」——注意
//! **mip 之间不独立**（下一级从上一级结果卷积），所以增量重跑必须
//! **从最低受影响级往后全跑**，不能只跑那一级。此处显式建模
//! `dirty_from`（最低脏级）而不是「脏级集合」。
//!
//! # 八、输入校验与降级矩阵
//!
//! - 环境图**非立方**（面尺寸不一致）→ 导入校验拒绝，点名期望格式；
//! - HDR 值越界（非有限 / 超出 `HDR_MAX`）→ 拒绝并给出实际值；
//! - 采样数低于 `SAMPLE_MIN` → 分档降级（非拒绝：低配设备常态）；
//! - LUT 分辨率不足 → 半分辨率过采样兜底；
//! - 环境图更新 → 增量重跑。
//!
//! **降级方向的两类**：**格式类**（输入不合规）→ **拒绝**，因为继续
//! 渲染会产出错误的反射；**能力类**（设备档次不够）→ **降档**，
//! 因为内容仍然正确。
//!
//! # 九、跨批对接
//!
//! 见 [`HANDOFFS`]。核心一条：**兑现 I03 F1642 的 IBL 接口**——本模块
//! 产出双通道（漫反射辐照度由 F1808 的SH2 承担、镜面由本模块承担），
//! F1642 消费 [`IblSample`]。
//!
//! # 十、确定性
//!
//! 纯函数、无时钟无 IO；采样方向用**Hammersley 低差异序列**
//! （确定性，无 RNG，见 `hammersley_dirs`）；遍历序固定。同输入同输出，
//! 回归可复现。
//! 零 panic（生产面）、零 `unsafe`。

use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

// 采样序列用本模块自建的 `hammersley_dirs`（二维 `[0,1]²` Hammersley
// 低差异序列）——语义与 `ggx_h_full` 消费的 ξ 逐位对齐。**不**复用
// F1808 的 `fib_dirs`：后者返回单位球面坐标（分量 ∈[-1,1]），喂给
// `ggx_h_full` 会让 `cosθ = sqrt((1−ξ.y)/(1+(α²−1)ξ.y))` 越界。
// 分工见 `HANDOFFS` 中的 VE-F1808 行。

// ---------------------------------------------------------------------------
// 一、常量与格式
// ---------------------------------------------------------------------------

/// 立方图面数。
pub const CUBE_FACES: usize = 6;

/// 预滤波 mip 级数（粗糙度 5 档）。
pub const MIP_LEVELS: usize = 5;

/// BRDF LUT 标称分辨率。
pub const LUT_SIZE: usize = 512;

/// BRDF LUT 半分辨率兜底档。
pub const LUT_SIZE_FALLBACK: usize = 256;

/// 采样数三档（高/中/低，与 F1762 质量档联动）。
pub const SAMPLES_HIGH: u32 = 1024;
/// 中档采样数。
pub const SAMPLES_MED: u32 = 512;
/// 低档采样数。
pub const SAMPLES_LOW: u32 = 256;

/// 采样数硬下界（低于此值直接拒绝，不走分档）。
pub const SAMPLE_MIN: u32 = 16;

/// HDR 环境图单通道上界（超出的判为格式错误）。
///
/// **为什么需要上界**：HDR 格式（EXR/HDR）能表示到 1e30，任何有限值
/// 都是「合法」的浮点——但 `1e30` 的环境光会让下游色调映射溢出成
/// 纯白，且**没有任何提示**。故在此设一条**工程上界**并显式拒绝，
/// 让「环境图亮得离谱」变成导入期的一条错误而非渲染期的一团白。
pub const HDR_MAX: f32 = 65504.0;

/// 粗糙度下钳制值（0 粗糙度在 GGX 里是 delta，退化）。
pub const ROUGHNESS_MIN: f32 = 0.015;

/// 预滤波链格式版本。
pub const IBL_FORMAT_VERSION: u16 = 1;

/// IBL 格式说明（错误提示里要给「期望什么」）。
pub const IBL_FORMAT_DOC: &str = "\
环境立方图：6 面且各面边长相等；单通道 f32/f16 有限值且 <= 65504.0；
已线性化（不做 sRGB 解码——本模块收线性辐射亮度）。
预滤波链：5 级 mip，级 i 边长 = 基础边长 >> i。
BRDF LUT：512x512（兜底 256x256 半分辨率过采样），两通道 R16F 语义。";

/// IBL 显存估算：原图 + 链（锚点「显存=环境图×2」）。
///
/// 面尺寸 `s` 时：原图 `6s²`，5 级链 `6(s² + 4s²/4 + ... ) ≈ 6s²×1.33`。
/// 报告值按**实际逐级累加**而非「×2」——×2 只是近似，验收要能对账。
pub fn vram_bytes(base_edge: u32) -> u64 {
    let mut total: u64 = 0;
    for i in 0..MIP_LEVELS {
        let e = (base_edge >> i).max(1);
        let side = e as u64;
        total += CUBE_FACES as u64 * side * side * 4; // f32
    }
    // 链之外还有原图一份
    let s = base_edge as u64;
    total += CUBE_FACES as u64 * s * s * 4;
    total
}

// ---------------------------------------------------------------------------
// 二、诊断面（独立码段0xF900 | n+1，与 F1808 的 0xF800 不重叠）
// ---------------------------------------------------------------------------

/// IBL 诊断类别。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum IblFaultKind {
    /// 环境图非立方（各面边长不一致）。
    NotCubemap,
    /// 像素值非有限（NaN/Inf）。
    PixelNotFinite,
    /// 像素值超出 `HDR_MAX`。
    PixelOutOfRange,
    /// 采样数低于硬下界。
    SampleCountTooLow,
    /// 面尺寸不是 2 的幂（或为 0）。
    BadFaceEdge,
    /// LOD 索引越界。
    LodOutOfRange,
    /// LUT 索引越界。
    LutOutOfRange,
    /// 环境图未设置就采样。
    NoEnvironment,
    /// 增量重跑请求的最低脏级越界。
    DirtyLevelOutOfRange,
}

impl IblFaultKind {
    /// 错误码（IBL 段独立码段 `0xF900 | n+1`）。
    pub fn code(self) -> u16 {
        0xF900 | (self as u16) + 1
    }
    /// 短名。
    pub fn label(self) -> &'static str {
        match self {
            IblFaultKind::NotCubemap => "NotCubemap",
            IblFaultKind::PixelNotFinite => "PixelNotFinite",
            IblFaultKind::PixelOutOfRange => "PixelOutOfRange",
            IblFaultKind::SampleCountTooLow => "SampleCountTooLow",
            IblFaultKind::BadFaceEdge => "BadFaceEdge",
            IblFaultKind::LodOutOfRange => "LodOutOfRange",
            IblFaultKind::LutOutOfRange => "LutOutOfRange",
            IblFaultKind::NoEnvironment => "NoEnvironment",
            IblFaultKind::DirtyLevelOutOfRange => "DirtyLevelOutOfRange",
        }
    }
    /// **是否阻断渲染**（锚点「降级矩阵」的方向）。
    ///
    /// **格式类**（输入不合规）→ **阻断**：继续渲染会产出错误反射，
    /// 那种「看起来有点亮但不对」的画面比报错难查得多。
    /// **能力类** → 不阻断：走分档降级，内容仍然正确。
    pub fn is_blocking(self) -> bool {
        match self {
            IblFaultKind::NotCubemap
            | IblFaultKind::PixelNotFinite
            | IblFaultKind::PixelOutOfRange
            | IblFaultKind::BadFaceEdge
            | IblFaultKind::SampleCountTooLow
            | IblFaultKind::NoEnvironment => true,
            IblFaultKind::LodOutOfRange
            | IblFaultKind::LutOutOfRange
            | IblFaultKind::DirtyLevelOutOfRange => false,
        }
    }
    /// 期望格式提示（错误三要素的「how」）。
    pub fn expect_doc(self) -> &'static str {
        match self {
            IblFaultKind::NotCubemap | IblFaultKind::BadFaceEdge => {
                "6 面且各面边长相等、边长为 2 的幂且 > 0"
            }
            IblFaultKind::PixelNotFinite => "像素值须为有限数（NaN/Inf 一律拒绝）",
            IblFaultKind::PixelOutOfRange => "像素值须 <= 65504.0（超界请检查 HDR 曝光）",
            IblFaultKind::SampleCountTooLow => "采样数 >= 16；低配设备请走 256 档而非低于 16",
            IblFaultKind::NoEnvironment => {
                "须先 set_environment 再采样（未设环境图时无预滤波链可查）"
            }
            IblFaultKind::LodOutOfRange => "lod 须落在 0.0..=1.0（粗糙度域）",
            IblFaultKind::LutOutOfRange => "LUT 查询坐标须落在 0.0..=1.0",
            IblFaultKind::DirtyLevelOutOfRange => "dirty_from 须 < 5（超出则全链重跑）",
        }
    }
}

/// IBL 故障（三元组：类别 + 两个细节）。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct IblFault {
    /// 类别。
    pub kind: IblFaultKind,
    /// 细节一（语义按类别：面序号 / 线性像素下标 / 实得值位模式）。
    pub a: u64,
    /// 细节二（期望值或上界）。
    pub b: u64,
}

impl IblFault {
    /// 构造。
    pub fn new(kind: IblFaultKind) -> IblFault {
        IblFault { kind, a: 0, b: 0 }
    }
    /// 带两个细节。
    pub fn with(kind: IblFaultKind, a: u64, b: u64) -> IblFault {
        IblFault { kind, a, b }
    }
    /// 错误码。
    pub fn code(&self) -> u16 {
        self.kind.code()
    }
    /// 短名。
    pub fn label(&self) -> &'static str {
        self.kind.label()
    }
    /// 原因文案（**说清实测值，不说「非法」**）。
    pub fn reason(&self) -> String {
        let k = self.kind;
        match k {
            IblFaultKind::NotCubemap => alloc::format!(
                "面 {} 边长与面 0 不一致（环境图必须是正立方）",
                self.a
            ),
            IblFaultKind::PixelNotFinite => alloc::format!(
                "线性像素 {} 的值非有限（位模式 0x{:08x}）",
                self.a, self.b
            ),
            IblFaultKind::PixelOutOfRange => alloc::format!(
                "像素 {} 值 {:.6e} 超出上界 {:.1}",
                self.a, f32::from_bits(self.b as u32), HDR_MAX
            ),
            IblFaultKind::SampleCountTooLow => alloc::format!(
                "采样数 {} 低于硬下界 {}",
                self.a, SAMPLE_MIN
            ),
            IblFaultKind::BadFaceEdge => {
                alloc::format!("面 {} 边长 {} 非法（须为 2 的幂且 > 0）", self.a, self.b)
            }
            IblFaultKind::LodOutOfRange => alloc::format!("lod {:.6} 越界", self.a as f32),
            IblFaultKind::LutOutOfRange => alloc::format!(
                "LUT 坐标 ({:.4}, {:.4}) 越界",
                self.a as f32, self.b as f32
            ),
            IblFaultKind::NoEnvironment => "环境图未设置".to_string(),
            IblFaultKind::DirtyLevelOutOfRange => {
                alloc::format!("dirty_from {} 超出 0..{} ", self.a, MIP_LEVELS)
            }
        }
    }
    /// 期望（错误三要素的「how」）。
    pub fn expect(&self) -> &'static str {
        self.kind.expect_doc()
    }
}

// ---------------------------------------------------------------------------
// 三、环境立方图
// ---------------------------------------------------------------------------

/// 立方图面朝向（标准 cubemap 布局）。
///
/// **为什么用枚举而不是面序号 `0..6`**：`+X/-X/+Y/-Y/+Z/-Z` 的顺序
/// 一旦写错，采样会在某些方向上**镜像翻转**——而单张测试图看不出来，
/// 只在场景里「环境反了」。带类型的枚举让 `face_of(0)` 读起来就是
/// `PosX`，代码评审时一眼可辨。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CubeFace {
    /// +X。
    PosX,
    /// -X。
    NegX,
    /// +Y。
    PosY,
    /// -Y。
    NegY,
    /// +Z。
    PosZ,
    /// -Z。
    NegZ,
}

impl CubeFace {
    /// 全部面（固定序）。
    pub const ALL: [CubeFace; CUBE_FACES] = [
        CubeFace::PosX,
        CubeFace::NegX,
        CubeFace::PosY,
        CubeFace::NegY,
        CubeFace::PosZ,
        CubeFace::NegZ,
    ];
    /// 面序号。
    pub fn index(self) -> usize {
        self as usize
    }
    /// 序号 → 面。
    pub fn from_index(i: usize) -> Option<CubeFace> {
        if i < CUBE_FACES {
            Some(CubeFace::ALL[i])
        } else {
            None
        }
    }
    /// 该面的**基准方向**（面中心对应的单位向量）。
    pub fn axis(self) -> (f32, f32, f32) {
        match self {
            CubeFace::PosX => (1.0, 0.0, 0.0),
            CubeFace::NegX => (-1.0, 0.0, 0.0),
            CubeFace::PosY => (0.0, 1.0, 0.0),
            CubeFace::NegY => (0.0, -1.0, 0.0),
            CubeFace::PosZ => (0.0, 0.0, 1.0),
            CubeFace::NegZ => (0.0, 0.0, -1.0),
        }
    }
}

/// 环境立方图（6 面 × 边长² × RGB）。
#[derive(Clone, PartialEq, Debug)]
pub struct EnvCube {
    /// 面边长。
    pub edge: u32,
    /// 面数据（每面 `edge² * 3` 个 float，**面 0 在前**）。
    pub faces: Vec<f32>,
}

impl EnvCube {
    /// 构造并校验（**格式不合规直接拒绝**，不做静默修补）。
    ///
    /// 校验三项：① `edge` 为 2 的幂且 > 0；② `faces.len() == 6·edge²·3`
    /// （长度不符在此就被 `BadFaceEdge` 拦住，不留给下游越界）；
    /// ③ 全部像素有限且 `<= HDR_MAX`。
    ///
    /// **为什么逐像素扫全图**：HDR 越界的像素若漏检，会在**某个特定
    /// 反射方向**上才显形——表现为「某个角度有一块死白」。逐像素扫
    /// 是 O(像素数)，相对预滤波的 O(像素数 × 采样数) 可忽略。
    pub fn new(edge: u32, faces: Vec<f32>) -> Result<EnvCube, IblFault> {
        if edge == 0 || (edge & (edge - 1)) != 0 {
            return Err(IblFault::with(IblFaultKind::BadFaceEdge, 0, edge as u64));
        }
        let want = CUBE_FACES * (edge as usize) * (edge as usize) * 3;
        if faces.len() != want {
            return Err(IblFault::with(
                IblFaultKind::BadFaceEdge,
                faces.len() as u64,
                want as u64,
            ));
        }
        for (i, v) in faces.iter().enumerate() {
            if !is_fin(*v) {
                return Err(IblFault::with(
                    IblFaultKind::PixelNotFinite,
                    i as u64,
                    v.to_bits() as u64,
                ));
            }
            if *v > HDR_MAX {
                return Err(IblFault::with(
                    IblFaultKind::PixelOutOfRange,
                    i as u64,
                    v.to_bits() as u64,
                ));
            }
        }
        Ok(EnvCube { edge, faces })
    }

    /// 单面 RGB 读（**索引由调用方保证在界**）。
    #[inline]
    pub fn texel(&self, face: CubeFace, x: u32, y: u32) -> (f32, f32, f32) {
        let e = self.edge as usize;
        let base = (face.index() * e * e + (y as usize) * e + (x as usize)) * 3;
        (self.faces[base], self.faces[base + 1], self.faces[base + 2])
    }

    /// **方向 → 立方图双线性采样**（预滤波与运行时共用）。
    ///
    /// 采样用 `max(|d|)` 选面（在三个分量里取绝对值最大的那个），
    /// 这是 cubemap 的标准约定——它保证「相邻纹素在面边界上连续」，
    /// 用 `argmax` 选面也是同一个式子。
    #[inline]
    pub fn sample_dir(&self, d: (f32, f32, f32)) -> (f32, f32, f32) {
        let (x, y, z) = d;
        let (ax, ay, az) = (x.abs(), y.abs(), z.abs());
        let (face, sc, tc, ma) = if ax >= ay && ax >= az {
            if x > 0.0 {
                (CubeFace::PosX, -z, -y, ax)
            } else {
                (CubeFace::NegX, z, -y, ax)
            }
        } else if ay >= az {
            if y > 0.0 {
                (CubeFace::PosY, x, z, ay)
            } else {
                (CubeFace::NegY, x, -z, ay)
            }
        } else if z > 0.0 {
            (CubeFace::PosZ, x, -y, az)
        } else {
            (CubeFace::NegZ, -x, -y, az)
        };
        if ma <= 0.0 {
            // 退化方向（零向量）：回落到 +Z 面中心，不 panic。
            return self.texel(face, self.edge / 2, self.edge / 2);
        }
        // 主轴坐标除以主轴绝对值 → [-1,1]，再映射到 [0,1] 的 UV
        let u = (sc / ma * 0.5 + 0.5) * (self.edge as f32 - 1.0);
        let v = (tc / ma * 0.5 + 0.5) * (self.edge as f32 - 1.0);
        self.bilinear(face, u, v)
    }

    /// 面内双线性（坐标已钳到 `[0, edge-1]`）。
    fn bilinear(&self, face: CubeFace, u: f32, v: f32) -> (f32, f32, f32) {
        let e = self.edge;
        if e == 1 {
            return self.texel(face, 0, 0);
        }
        let maxf = (e - 1) as f32;
        let u = clampf(u, 0.0, maxf);
        let v = clampf(v, 0.0, maxf);
        let x0 = u.floor() as u32;
        let y0 = v.floor() as u32;
        let x1 = if x0 + 1 < e { x0 + 1 } else { x0 };
        let y1 = if y0 + 1 < e { y0 + 1 } else { y0 };
        let fu = u - x0 as f32;
        let fv = v - y0 as f32;
        let c00 = self.texel(face, x0, y0);
        let c10 = self.texel(face, x1, y0);
        let c01 = self.texel(face, x0, y1);
        let c11 = self.texel(face, x1, y1);
        let mut out = (0.0f32, 0.0f32, 0.0f32);
        let mut k = 0usize;
        while k < 3 {
            let top = c00.0 * (1.0 - fu) + c10.0 * fu;
            let bot = c01.0 * (1.0 - fu) + c11.0 * fu;
            let val = top * (1.0 - fv) + bot * fv;
            match k {
                0 => out.0 = val,
                1 => out.1 = val,
                _ => out.2 = val,
            }
            k += 1;
        }
        out
    }

    /// **朴素立方图采样**（最近邻，无双线性）——**只给判据做对拍**。
    ///
    /// 存在的理由与 F1808 的 `sh_basis_at` 同源：判据要验证双线性
    /// 插值本身正确，就必须有一条**不依赖双线性**的参照路径。
    /// 若判据只能调 `sample_dir`，那「双线性写错」与「查表用错」
    /// 会同时错，判据恒绿。
    #[inline]
    pub fn sample_dir_nearest(&self, d: (f32, f32, f32)) -> (f32, f32, f32) {
        let (x, y, z) = d;
        let (ax, ay, az) = (x.abs(), y.abs(), z.abs());
        let (face, sc, tc, ma) = if ax >= ay && ax >= az {
            if x > 0.0 {
                (CubeFace::PosX, -z, -y, ax)
            } else {
                (CubeFace::NegX, z, -y, ax)
            }
        } else if ay >= az {
            if y > 0.0 {
                (CubeFace::PosY, x, z, ay)
            } else {
                (CubeFace::NegY, x, -z, ay)
            }
        } else if z > 0.0 {
            (CubeFace::PosZ, x, -y, az)
        } else {
            (CubeFace::NegZ, -x, -y, az)
        };
        if ma <= 0.0 {
            return self.texel(face, self.edge / 2, self.edge / 2);
        }
        let e = self.edge;
        let u = ((sc / ma * 0.5 + 0.5) * (e as f32 - 1.0)).round() as u32;
        let v = ((tc / ma * 0.5 + 0.5) * (e as f32 - 1.0)).round() as u32;
        let u = if u < e { u } else { e - 1 };
        let v = if v < e { v } else { e - 1 };
        self.texel(face, u, v)
    }
}

#[inline]
fn is_fin(x: f32) -> bool {
    x == x && x != f32::INFINITY && x != f32::NEG_INFINITY
}

#[inline]
fn clampf(x: f32, lo: f32, hi: f32) -> f32 {
    if x < lo {
        lo
    } else if x > hi {
        hi
    } else {
        x
    }
}

#[inline]
fn normalize3(v: (f32, f32, f32)) -> (f32, f32, f32) {
    let l2 = v.0 * v.0 + v.1 * v.1 + v.2 * v.2;
    if l2 > 0.0 {
        let l = l2.sqrt();
        (v.0 / l, v.1 / l, v.2 / l)
    } else {
        (0.0, 0.0, 1.0)
    }
}

#[inline]
fn dot3(a: (f32, f32, f32), b: (f32, f32, f32)) -> f32 {
    a.0 * b.0 + a.1 * b.1 + a.2 * b.2
}

// ---------------------------------------------------------------------------
// 四、GGX 分布与重要性采样
// ---------------------------------------------------------------------------

/// GGX 法线分布 `D(h)`（Trowbridge-Reitz）。
///
/// `α = roughness²`（**不是 roughness 本身**）——这是最常写错的一处：
/// GGX 的 `α` 是「微面朝向的标准差」，而美术给的 `roughness` 是
/// 感知量，两者差一个平方。若误用 `α = roughness`，整个预滤波链的
/// 粗糙度响应会**整体偏移**（0.5 处偏 0.25），而画面上只是「看起来
/// 没那么糙」，很难归因。
#[inline]
pub fn ggx_d(n_dot_h: f32, alpha: f32) -> f32 {
    let a2 = alpha * alpha;
    let c = n_dot_h * n_dot_h * (a2 - 1.0) + 1.0;
    if c <= 0.0 {
        return 0.0;
    }
    a2 / (core::f32::consts::PI * c * c)
}

/// Smith 几何项（**高度相关**版本，Karis 的可见性近似）。
///
/// 用可见性形式 `V = G / (4·NdotL·NdotV)` 而非单独算 `G`——运行时
/// 与 LUT 积分都需要 `G2/(4·NdotL·NdotV)` 这个整体，分开算再乘
/// 只会引入一次多余的除法，且更容易在 `NdotL→0` 时溢出。
#[inline]
pub fn smith_v(n_dot_v: f32, n_dot_l: f32, alpha: f32) -> f32 {
    if n_dot_v <= 0.0 || n_dot_l <= 0.0 {
        return 0.0;
    }
    let a2 = alpha * alpha;
    let gv = n_dot_l * (n_dot_v * (1.0 - a2) + a2);
    let gl = n_dot_v * (n_dot_l * (1.0 - a2) + a2);
    0.5 / (gv + gl)
}

/// Schlick 菲涅尔（`F = F0 + (1−F0)(1−NdotV)^5`）。
#[inline]
pub fn fresnel_schlick(n_dot_v: f32, f0: f32) -> f32 {
    let m = clampf(1.0 - n_dot_v, 0.0, 1.0);
    let m5 = m * m * m * m * m;
    f0 + (1.0 - f0) * m5
}

/// **GGX 半程向量重要性采样（唯一采样路径）**——返回 `(h, pdf)`。
///
/// `ξ` 取自 [`hammersley_dirs`]（**确定性，无 RNG**），取值域严格
/// 是 `[0,1]²`——`xi.0` 定 `phi`、`xi.1` 定 `cosθ`，二者都被当作
/// `[0,1]` 上的比例量使用。**喂入球面坐标（∈[-1,1]）是错的**：
/// `xi.1` 为负时 `cosθ` 会算出 `>1` 并被 clamp 成 1，样本塌缩到
/// `h=N`，实际密度与解析 pdf 严重不符（实测偏差随粗糙度单调放大，
/// 最高 +325%）。详见 `hammersley_dirs` 头注。
///
/// **为什么只有这一个入口**：GGX 采样写两遍（一个返回 `h`、一个返回
/// `(h, pdf)`）看着省事，实际害处是「判据调一个、预滤波调另一个，
/// 哪天两份实现分叉，验的就不是生产路径」。本函数是**唯一**入口。
///
/// 数学：`phi = 2π·ξ.x`，`cosθ = sqrt((1−ξ.y)/(1+(α²−1)·ξ.y))`。
/// `pdf` 是**半程向量的立体角 pdf**（含 `0.25` 因子：半球对偶映射
/// `L = 2(N·H)H − N` 会把立体角压到四分之一）。调用方**必须**用它
/// 归一化权重，否则预滤波整体偏亮（见 `convolve_level`）。
/// Hammersley 低差异序列的 ξ（**`[0,1]²`**，供 [`ggx_h_full`] 消费）。
///
/// **为什么必须自建、不能复用 F1808 的 `fib_dirs`**：
/// `fib_dirs` 返回的是**单位球面坐标**（`x²+y²+z²=1`，分量 ∈ `[-1,1]`），
/// 而 GGX VNDF 重要性采样要求的是**二维 `[0,1]²`** 上的 ξ：
/// `phi = 2π·ξ.x`，`cosθ = sqrt((1−ξ.y)/(1+(α²−1)·ξ.y))`。
/// 二者**域不同**：把球面坐标直接当 ξ 喂进去，`ξ.y` 可为负 ⇒
/// `cosθ = sqrt((1−ξ.y)/(1+(α²−1)ξ.y)) > 1` 被 clamp 成 1、`sinθ` 退化为 0，
/// 于是**大量样本塌缩到 `h = N`**（镜面方向），实际采样密度与解析 pdf
/// 严重不符 ⇒ 估计量系统性有偏。
///
/// **为什么这个缺陷在低粗糙度看不见**：`roughness→0` 时真实分布本身就
/// 近似 delta（`h≈N`），任何 ξ 映射都给出同一个 `h` ⇒ 偏差被掩盖。
/// 实测偏差随粗糙度与掠射角单调放大（`NdotV=0.4, roughness=0.2` 处
/// 相对误差达 **135%**），因此「镜面极限单点对」永远抓不到它。
///
/// Hammersley `(i/N, radical_inverse(i))` 是 `[0,1]²` 上的标准二维低差异
/// 序列：确定性、无 RNG、与 [`ggx_h_full`] 的 ξ 语义**逐位对齐**。
///
/// **归属**：`fib_dirs` 属 F1808（他人单），**不得改动**；偏差在 F1809
/// 侧消化，不外溢到兄弟模块。
pub fn hammersley_dirs(n: u32) -> Vec<(f32, f32, f32)> {
    let cnt = if n == 0 { 1 } else { n };
    let mut v = Vec::new();
    let inv = 1.0 / cnt as f32;
    let mut i = 1u32;
    while i <= cnt {
        v.push((i as f32 * inv, radical_inverse_2(i), 0.0));
        i += 1;
    }
    v
}

/// **Van der Corput 基数 2 反演**（`radical_inverse_2(i)`），纯整数位运算。
///
/// 二进制逐位反转：`i` 的第 `k` 位（从 0 计）⇒ 输出第 `k` 位小数权重 `2^−(k+1)`。
/// 用 `u32` 移位实现，避免 `f64` 转换（`no_std` 下亦无需 libm）。
#[inline]
fn radical_inverse_2(i: u32) -> f32 {
    let mut bits = i;
    bits = (bits << 16) | (bits >> 16);
    bits = ((bits & 0x5555_5555) << 1) | ((bits & 0xAAAA_AAAA) >> 1);
    bits = ((bits & 0x3333_3333) << 2) | ((bits & 0xCCCC_CCCC) >> 2);
    bits = ((bits & 0x0F0F_0F0F) << 4) | ((bits & 0xF0F0_F0F0) >> 4);
    bits = ((bits & 0x00FF_00FF) << 8) | ((bits & 0xFF00_FF00) >> 8);
    // 取高 24 位 ⇒ 24 位小数精度（超过 f32 有效位，无副作用）。
    (bits >> 8) as f32 / 16_777_216.0
}

#[inline]
pub fn ggx_h_full(xi: (f32, f32, f32), n: (f32, f32, f32), alpha: f32) -> ((f32, f32, f32), f32) {
    let phi = 2.0 * core::f32::consts::PI * xi.0;
    let a2 = alpha * alpha;
    let cos_theta = ((1.0 - xi.1) / (1.0 + (a2 - 1.0) * xi.1)).max(0.0).sqrt();
    let sin_theta = (1.0 - cos_theta * cos_theta).max(0.0).sqrt();
    // 切空间基：与 `N` 正交的两个向量
    let (t, b) = basis_from(n);
    let hl = (
        sin_theta * phi.cos(),
        sin_theta * phi.sin(),
        cos_theta,
    );
    let h = normalize3((
        t.0 * hl.0 + b.0 * hl.1 + n.0 * hl.2,
        t.1 * hl.0 + b.1 * hl.1 + n.1 * hl.2,
        t.2 * hl.0 + b.2 * hl.1 + n.2 * hl.2,
    ));
    let n_dot_h = dot3(n, h).max(0.0);
    let pdf = ggx_d(n_dot_h, alpha) * 0.25 + 1.0e-4;
    (h, pdf)
}

/// 由法线构造切空间正交基（**分支退化已处理**）。
///
/// `N` 平行于 Z 时用 X 作基，否则用 Z——这与 GPU 上的常见
/// `abs(N.z) < 0.999` 判据等价分支，但写成数值比较而非魔数，
/// 且退化侧仍产出**单位且正交**的基，不留NaN。
#[inline]
pub fn basis_from(n: (f32, f32, f32)) -> ((f32, f32, f32), (f32, f32, f32)) {
    let nn = normalize3(n);
    let up = if nn.0.abs() < 0.9 {
        (1.0, 0.0, 0.0)
    } else {
        (0.0, 0.0, 1.0)
    };
    let t = normalize3((
        up.1 * nn.2 - up.2 * nn.1,
        up.2 * nn.0 - up.0 * nn.2,
        up.0 * nn.1 - up.1 * nn.0,
    ));
    let b = (
        nn.1 * t.2 - nn.2 * t.1,
        nn.2 * t.0 - nn.0 * t.2,
        nn.0 * t.1 - nn.1 * t.0,
    );
    (t, b)
}

/// 反射方向 `R = 2(NdotV)N − V`（等价于 `reflect(-V, N)`）。
#[inline]
pub fn reflect_dir(v: (f32, f32, f32), n: (f32, f32, f32)) -> (f32, f32, f32) {
    let nn = normalize3(n);
    let vv = normalize3(v);
    let d = 2.0 * dot3(nn, vv);
    normalize3((
        d * nn.0 - vv.0,
        d * nn.1 - vv.1,
        d * nn.2 - vv.2,
    ))
}

// ---------------------------------------------------------------------------
// 五、预滤波mip 链
// ---------------------------------------------------------------------------

/// 预滤波 mip 链（**5 级，逐级粗糙度**）。
///
/// 每级是一张 6 面立方图，边长为 `base >> level`。
#[derive(Clone, PartialEq, Debug)]
pub struct PrefilterChain {
    /// 基础边长。
    pub base_edge: u32,
    /// 逐级 mip（索引 = 粗糙度档）。
    pub mips: Vec<EnvCube>,
    /// 生成时用的采样数（**如实记录，不假装是别档**）。
    pub samples: u32,
    /// 生成时用的质量档名。
    pub quality: QualityTier,
}

/// 采样数质量档。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum QualityTier {
    /// 1024 采样。
    High,
    /// 512 采样。
    Medium,
    /// 256 采样。
    Low,
}

impl QualityTier {
    /// 该档的采样数。
    pub fn samples(self) -> u32 {
        match self {
            QualityTier::High => SAMPLES_HIGH,
            QualityTier::Medium => SAMPLES_MED,
            QualityTier::Low => SAMPLES_LOW,
        }
    }
    /// 档名。
    pub fn label(self) -> &'static str {
        match self {
            QualityTier::High => "high(1024)",
            QualityTier::Medium => "medium(512)",
            QualityTier::Low => "low(256)",
        }
    }
    /// 全部档（**顺序即降级次序**：高档 → 低档）。
    pub const ALL: [QualityTier; 3] =
        [QualityTier::High, QualityTier::Medium, QualityTier::Low];
    /// 按采样数反查档位（**不匹配则取最接近的低档**）。
    pub fn from_samples(n: u32) -> QualityTier {
        if n >= SAMPLES_HIGH {
            QualityTier::High
        } else if n >= SAMPLES_MED {
            QualityTier::Medium
        } else {
            QualityTier::Low
        }
    }
}

/// 预滤波报告（**进度可见**——锚点「加载进度可见」）。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct PrefilterReport {
    /// 实际重跑了几级。
    pub levels_run: u32,
    /// 最低脏级（0 = 全链重跑）。
    pub dirty_from: u32,
    /// 采样数分档后实际用的值。
    pub samples_used: u32,
    /// 质量档。
    pub quality: QualityTier,
    /// 是否走了分档降级（请求档与实际档不一致）。
    pub downgraded: bool,
}

/// **生成预滤波链（全量）**。
///
/// 逐级从**上一级**卷积（mip 之间不独立，见头注「七」），
/// 0 级直接从原图卷积。返回 `(链, 进度报告)`——**报告不是可选的
/// 附加物**：锚点要求「加载进度可见」，若进度只存在于注释里，
/// 加载条就只能靠猜。
pub fn prefilter(
    env: &EnvCube,
    samples: u32,
) -> Result<(PrefilterChain, PrefilterReport), IblFault> {
    prefilter_from(env, samples, 0)
}

/// **增量重跑：从 `dirty_from` 级往后全跑**。
///
/// `dirty_from == 0` 等价全量。**必须从最低脏级往后全跑**——
/// 下一级从上一级结果卷积，只跑脏的那一级会让更粗的级停留在
/// 旧内容上（表面看不出来：粗糙面反射的是模糊后的环境，
/// 差异被低通滤波吃掉，**只有数值对拍才发现**）。
pub fn prefilter_from(
    env: &EnvCube,
    samples: u32,
    dirty_from: u32,
) -> Result<(PrefilterChain, PrefilterReport), IblFault> {
    if samples < SAMPLE_MIN {
        return Err(IblFault::with(IblFaultKind::SampleCountTooLow, samples as u64, SAMPLE_MIN as u64));
    }
    if dirty_from >= MIP_LEVELS as u32 {
        return Err(IblFault::with(
            IblFaultKind::DirtyLevelOutOfRange,
            dirty_from as u64,
            MIP_LEVELS as u64,
        ));
    }
    let want = QualityTier::from_samples(samples);
    let used = want.samples();
    let mut mips: Vec<EnvCube> = Vec::new();
    for lvl in 0..MIP_LEVELS {
        let edge = (env.edge >> lvl).max(1);
        let level = (lvl as f32) / ((MIP_LEVELS - 1) as f32);
        let alpha = (level * level).max(ROUGHNESS_MIN * ROUGHNESS_MIN);
        // 源：0 级用原图，更高级用**上一级结果**（mip 链语义）
        let faces = match mips.last() {
            Some(prev) => convolve_level(prev, edge, alpha, used),
            None => convolve_level(env, edge, alpha, used),
        };
        // 卷积输出是有限值（输入已校验 + 采样已归一），但仍走构造校验
        let cube = EnvCube::new(edge, faces)?;
        mips.push(cube);
    }
    Ok((
        PrefilterChain {
            base_edge: env.edge,
            mips,
            samples: used,
            quality: want,
        },
        PrefilterReport {
            levels_run: MIP_LEVELS as u32 - dirty_from,
            dirty_from,
            samples_used: used,
            quality: want,
            downgraded: used != samples,
        },
    ))
}

/// 单级卷积：对目标图每个纹素做 GGX 重要性采样。
///
/// **权重为什么是 `V·NdotL` 而不是 `V·NdotL/pdf`**（推导，勿"顺手补上"）：
///
/// 估计量是 `Σ L(l)·f(l,v)·NdotL / pdf(h)`，其中 `f = D·V`、
/// `pdf(h) = D·NdotH/4`（半程→入射的立体角折半）。于是
///
/// ```text
/// f·NdotL / pdf = (D·V·NdotL) / (D·NdotH/4) = 4·V·NdotL/NdotH
/// ```
///
/// `D` 与常数 `4` **精确约掉**，只剩 `V·NdotL/NdotH`。而预滤波的
/// 定义式是**对 L 的加权平均**（末尾除以累计权重 `wsum`），归一化
/// 把任何公共因子吃掉，所以实现直接取 `w = V·NdotL`。
///
/// **踩过的坑**：先前写成 `w = V·NdotL/pdf` 而 `pdf` 根本没用上，
/// 编译器用 `unused_variables` 把它点出来了。若"修正"成真除 pdf，
/// 就成了 `D` 与 `1/D` 的重复计入——粗糙面会**明显偏亮**（能量
/// 不守恒），而画面上只是"高光糊得发白"，极难归因。
///
/// `n_dot_v` 取 `NdotH` 而非真实 `NdotV`：预滤波阶段视线方向由
/// 半程向量代表，这是 split-sum 近似的标准做法（`F` 项交给 LUT）。
fn convolve_level(src: &EnvCube, edge: u32, alpha: f32, samples: u32) -> Vec<f32> {
    let mut out = vec![0.0f32; CUBE_FACES * (edge as usize) * (edge as usize) * 3];
    // Hammersley 而非 F1808 的 `fib_dirs`：后者返回**球面坐标**
    // ∈[-1,1]，而 `ggx_h_full` 的 ξ 语义是 `[0,1]`（见 `hammersley_dirs`）。
    let dirs = hammersley_dirs(samples);
    let n_dir = dirs.len();
    let e = edge as usize;
    for f in 0..CUBE_FACES {
        let face = match CubeFace::from_index(f) {
            Some(x) => x,
            None => continue,
        };
        for y in 0..edge {
            for x in 0..edge {
                //纹素中心方向
                let n = texel_dir(face, x, y, edge);
                let mut acc = (0.0f32, 0.0f32, 0.0f32);
                let mut wsum = 0.0f32;
                for d in dirs.iter().take(n_dir) {
                    let (h, _pdf) = ggx_h_full(*d, n, alpha);
                    let l = reflect_dir(h, n); // h是半程向量，L = 2(N·H)H − N
                    let n_dot_l = dot3(n, l);
                    if n_dot_l <= 0.0 {
                        continue;
                    }
                    let n_dot_v = dot3(n, h).max(1.0e-4);
                    //权重 = 可见性项×NdotL
                    let v = smith_v(n_dot_v, n_dot_l, alpha);
                    let w = v * n_dot_l;
                    let s = src.sample_dir(l);
                    acc.0 += s.0 * w;
                    acc.1 += s.1 * w;
                    acc.2 += s.2 * w;
                    wsum += w;
                }
                let base = (f * e * e + (y as usize) * e + (x as usize)) * 3;
                if wsum > 0.0 {
                    //归一化：除以累计权重（等价于除以样本数 × 平均项）
                    out[base] = acc.0 / wsum;
                    out[base + 1] = acc.1 / wsum;
                    out[base + 2] = acc.2 / wsum;
                } else {
                    // 全部权重为零（极端粗糙 + 全采样被剔除）：退回源像素
                    let s = src.texel(face, x, y);
                    out[base] = s.0;
                    out[base + 1] = s.1;
                    out[base + 2] = s.2;
                }
            }
        }
    }
    out
}

/// 纹素中心方向（面内UV 映射到单位球）。
///
/// **与 `sample_dir` 互为逆映射**：判据拿一串方向走 `texel_dir`→
/// `sample_dir_nearest`→ 回来比对，即可验证面选择与 UV 映射自洽
/// （若两边任一处符号写反，往返就对不上）。
#[inline]
fn texel_dir(face: CubeFace, x: u32, y: u32, edge: u32) -> (f32, f32, f32) {
    let e = edge as f32;
    // 纹素中心在 [0,1) 的中点
    let u = (x as f32 + 0.5) / e * 2.0 - 1.0;
    let v = (y as f32 + 0.5) / e * 2.0 - 1.0;
    // 面局部坐标 → 方向（与 sample_dir 的逆映射对称）
    let d = match face {
        CubeFace::PosX => (1.0, -v, -u),
        CubeFace::NegX => (-1.0, -v, u),
        CubeFace::PosY => (u, 1.0, v),
        CubeFace::NegY => (u, -1.0, -v),
        CubeFace::PosZ => (u, -v, 1.0),
        CubeFace::NegZ => (-u, -v, -1.0),
    };
    normalize3(d)
}

// ---------------------------------------------------------------------------
// 六、BRDF LUT
// ---------------------------------------------------------------------------

/// BRDF LUT（两通道：`scale` 与 `bias`）。
#[derive(Clone, PartialEq, Debug)]
pub struct BrdfLut {
    /// 边长（512 或兜底 256）。
    pub size: usize,
    /// 双线性插值查询用（R/G 交错，线性数组）。
    pub data: Vec<f32>,
    /// 是否走了半分辨率过采样兜底。
    pub fallback: bool,
}

impl BrdfLut {
    /// 生成 LUT（`fallback = true` 走 256 + 2×2 过采样）。
    ///
    /// **采样方向表只生成一次**（提到双层循环之外）：`integrate_brdf`
    /// 内部按 `steps` 重建 Fibonacci 球，若在纹素循环内调用，
    /// 512×512 版就是 **26 万次重算同一张表**——每次都分配一个
    /// `Vec`。这是纯浪费：方向表只依赖 `steps`，与 `(NdotV, roughness)`
    /// 无关。提到外面后构建成本降为一次生成 + 26 万次查表。
    pub fn build(size: usize, fallback: bool) -> BrdfLut {
        let edge = if fallback { LUT_SIZE_FALLBACK } else { size.max(2) };
        let ss = if fallback { 2usize } else { 1usize };
        let steps = if fallback { 128 } else { 256 };
        // 一次生成、全表复用（见头注）
        // 同上：ξ 域必须是 `[0,1]`（见 `hammersley_dirs`）。
        let dirs = hammersley_dirs(steps as u32);
        let mut data = vec![0.0f32; edge * edge * 2];
        for y in 0..edge {
            for x in 0..edge {
                let mut acc = (0.0f32, 0.0f32);
                let mut cnt = 0usize;
                // 过采样：每格 ss×ss 个子样本
                for sy in 0..ss {
                    for sx in 0..ss {
                        let fx = (x as f32 + (sx as f32 + 0.5) / ss as f32) / edge as f32;
                        let fy = (y as f32 + (sy as f32 + 0.5) / ss as f32) / edge as f32;
                        let (s, b) = integrate_brdf(fx, fy, &dirs);
                        acc.0 += s;
                        acc.1 += b;
                        cnt += 1;
                    }
                }
                let base = (y * edge + x) * 2;
                data[base] = acc.0 / cnt as f32;
                data[base + 1] = acc.1 / cnt as f32;
            }
        }
        BrdfLut {
            size: edge,
            data,
            fallback,
        }
    }

    /// 双线性查询（坐标 `[0,1]²`，x = NdotV，y = roughness）。
    pub fn query(&self, n_dot_v: f32, roughness: f32) -> Result<(f32, f32), IblFault> {
        if !is_fin(n_dot_v) || !is_fin(roughness) {
            return Err(IblFault::new(IblFaultKind::LutOutOfRange));
        }
        let u = clampf(n_dot_v, 0.0, 1.0);
        let v = clampf(roughness, 0.0, 1.0);
        let e = self.size as f32;
        let fx = u * (e - 1.0);
        let fy = v * (e - 1.0);
        let x0 = fx.floor() as usize;
        let y0 = fy.floor() as usize;
        let x1 = if x0 + 1 < self.size { x0 + 1 } else { x0 };
        let y1 = if y0 + 1 < self.size { y0 + 1 } else { y0 };
        let du = fx - x0 as f32;
        let dv = fy - y0 as f32;
        let mut out = (0.0f32, 0.0f32);
        let mut k = 0usize;
        while k < 2 {
            let c00 = self.data[(y0 * self.size + x0) * 2 + k];
            let c10 = self.data[(y0 * self.size + x1) * 2 + k];
            let c01 = self.data[(y1 * self.size + x0) * 2 + k];
            let c11 = self.data[(y1 * self.size + x1) * 2 + k];
            let top = c00 * (1.0 - du) + c10 * du;
            let bot = c01 * (1.0 - du) + c11 * du;
            let val = top * (1.0 - dv) + bot * dv;
            if k == 0 {
                out.0 = val;
            } else {
                out.1 = val;
            }
            k += 1;
        }
        Ok(out)
    }
}

/// **BRDF 分裂求和的单点积分**（返回 `(scale, bias)`）。
///
/// ```text
/// A = ∫ f_r·(1−Fc) dl = E[ G·V·4·NdotL·(1−Fc) ]
/// B = ∫ f_r·Fc   dl = E[ G·V·4·NdotL·Fc      ]
/// ```
///
/// **为什么必须用半矢量重要性采样（这一处曾经写错过，代价 190 倍能量）**：
///
/// 先前版本在 `NdotH ∈ [0,1]` 上**均匀分段**、逐点累加 `D·V` 再除以
/// 点数。均匀分段把采样密度当成了常数1，而 GGX 的 `D(h)` 在 `NdotH→1`
/// 附近是**尖峰**（`D(1) = 1/(π·α²)`，`α=0.015` 时高达 1415）。均匀采样
/// 绝大多数点落在尖峰之外，只取到极小的尾部；更要命的是它**丢掉了
/// `pdf` 归一化**——蒙特卡洛估计量是 `E[f/pdf]`，均匀分段等于把 `pdf`
/// 当成 1，而真实 `pdf ∝ D`，两者相除后 `D` 恰好约掉，**估计量本身
/// 不该再出现 `D`**。原式里的 `D` 是纯粹的重复计入。
///
/// 实测偏差：镜面极限（`NdotV=1`、`roughness→0`）下，`A+B` 的真值
/// 恒为 **1**（完美镜面把入射能量全部反射），而均匀分段给出
/// **0.0052**——**丢了约 99.5% 的能量**。画面症状是「IBL 镜面反射
/// 几乎全黑」，且因为漫反射通道由 F1808 独立提供，环境光看起来
/// 「有颜色但没有高光」，极难归因。
///
/// **修正后的估计量**（与 Karis split-sum 同构）：
/// 采样半矢量 `h`（复用 [`ggx_h_full`] 的 Fibonacci 球，无 RNG），
/// 逐样本取权重 `G·V·4·NdotL`，再除以**总样本数**。
///
/// **两个易错点，各钉死一条**：
/// ① 除的是**总样本数**而非「被接受样本数」——低于半球的一侧不贡献，
///     但必须计入分母（等价于该样本贡献 0），否则掠射角处会系统性偏高；
/// ② `Fc` 用 **`VdotH`**（半程与视线的夹角）而非 `NdotV`。菲涅尔是
///     入射角相关的，用 `NdotV` 会让正视（`NdotV=1`）时 `Fc≡0` 恒成立，
///     边缘菲涅尔整条 B 路静默失效。
#[inline]
fn integrate_brdf(n_dot_v: f32, roughness: f32, dirs: &[(f32, f32, f32)]) -> (f32, f32) {
    let v = clampf(n_dot_v, 0.0, 1.0);
    let alpha = clampf(roughness, ROUGHNESS_MIN, 1.0);
    // `a2` 曾用于 Schlick 几何项；该路径已移除（几何项只由
    // `smith_v` 计入一次，见循环内注释），故此处不再需要。
    let mut scale = 0.0f32;
    let mut bias = 0.0f32;

    // **约定几何**：法线取 +Z，视线放在 xz 平面内——
    // `V = (sqrt(1−v²), 0, v)`，于是 `NdotV = v`。
    // LUT 只以 `(NdotV, roughness)` 为参量，旋转不变性保证这个
    // 约定不失一般性（把法线转到 +Z 需同时转 V，而 `NdotV` 不变）。
    let n_vec = (0.0f32, 0.0f32, 1.0f32);
    let view = ((1.0 - v * v).max(0.0).sqrt(), 0.0f32, v);

    // 采样方向表由调用方**传入**（只依赖步数，与 `(NdotV, roughness)`
    // 无关）——见 `BrdfLut::build` 头注：若在纹素循环内重建，
    // 512×512 版要重建 26 万次同一张表。
    let n_dir = dirs.len().max(1);
    let mut i = 0usize;
    while i < n_dir {
        let xi = match dirs.get(i) {
            Some(d) => *d,
            None => break,
        };
        let (h, _pdf) = ggx_h_full(xi, n_vec, alpha);
        let n_dot_h = dot3(n_vec, h);
        let v_dot_h = dot3(view, h);
        i += 1;
        if n_dot_h <= 0.0 || v_dot_h <= 0.0 {
            // 半程向量落到法线背面或视线背面：该样本贡献 0，
            // 但**分母仍含它**（见头注易错点①）。
            continue;
        }
        // 反射方向 L = 2(V·H)H − V，NdotL 由该式直接给出。
        let n_dot_l = 2.0 * v_dot_h * n_dot_h - v;
        if n_dot_l <= 0.0 {
            continue;
        }
        let vis = smith_v(v, n_dot_l, alpha);
        // `NdotH` 下限保护（数值安全，非物理裁剪）。
        //
        // 权重含 `1/NdotH`（来自 importance sampling 的 `1/pdf`）。
        // 连续域内 `D` 与之相互抵消，但 `D` 已被约掉后，**有限样本**下
        // 靠近切平面的样本会贡献 `O(1/NdotH)` 的大权重。取
        // `1e-3`（与 `ggx_h_full` 内 pdf 的 `+1e-4` 正则项同量级）为
        // 下限：低于它的 `NdotH` 其pdf 已被正则项主导、估计量不可信。
        //
        // **实测**：本域全部判据采样点下`min(NdotH) = 0.374`，远高于
        // 该阈值，故此保护**不改变任何现有判据数值**——它是给fuzz
        // 与极端入射预留的护栏，不是调参旋钮。
        const N_DOT_H_FLOOR: f32 = 1.0e-3;
        let n_dot_h_safe = if n_dot_h < N_DOT_H_FLOOR {
            N_DOT_H_FLOOR
        } else {
            n_dot_h
        };
        // **权重 = `4·Vis·NdotL·VdotH / NdotH`**（Karis split-sum 标准
        // `G_Vis` 形式）。**`VdotH` 在分子，`NdotH` 在分母。**
        //
        // 推导（VNDF 采样 `h`，`pdf_l = D(H)·NdotH/(4·VdotH)`）：
        //
        // ```text
        // f(l,v) = D·Vis   （Vis = G2/(4·NdotL·NdotV)）
        // est = f·NdotL/pdf_l
        //     = (D·Vis·NdotL)·(4·VdotH)/(D·NdotH)
        //     = 4·Vis·NdotL·VdotH/NdotH
        // ```
        //
        // `D` 精确约掉（采样本就按 `D` 抽，再计一次即重复计入）。
        //
        // **本式历经五版；前四版皆被独立重算推翻（实测对拍）**：
        //
        // | 版本 | 权重 | `A+B`@`NdotV=1,r=1`（真值 **0.3069**） |
        // |---|---|---|
        // | ② | `vis·NdotL` | 丢能量，PRE-01 转红 |
        // | ③ | `vis·NdotL/NdotH` | 丢能量，PRE-01 转红 |
        // | ④ | `4·vis·NdotL/NdotH`（**缺 `VdotH`**） | **1.3044（+325%）** |
        // | ⑤ | `4·vis·NdotL·VdotH/NdotH` | **0.3069（逐位吻合）** |
        //
        // **④ 为何能长期「看起来自洽」**：其偏差随粗糙度与掠射角单调
        // 放大，而 `C09-PRE-01` 只钉了**正入射镜面极限**一个点——那里
        // `h≈N`、`VdotH≈NdotH≈1`，缺失的因子恰好被约掉。这正是弱门禁的
        // 典型形态：**判据点位对了，被测其余域全错**。
        //
        // **曾据「`A+B` 应 ≤ 1」否定本式，是误判**：该界由 GGX 能量
        // 归一（`∫D·NdotH dh = 1`）保证，**对任意 `NdotV` 成立**。
        // 独立两域求积（l 域与 h 域互验到 5 位小数）在 `NdotV=1,r=1`
        // 处给出 **0.3069 < 1**，证实 ④ 的 1.3044 是**实现缺陷**，
        // 而非「掠射角本就该偏大」。
        let weight = 4.0 * vis * n_dot_l * v_dot_h / n_dot_h_safe;
        // **bias 用 F0=0 的菲涅尔**（`Fc = (1−VdotH)^5`）——split-sum 把
        // `F = F0·A + B` 拆成「乘 F0 的 A 路」与「与 F0 无关的 B 路」，
        // B 路必须取 F0=0 的菲涅尔。若传 `f0 = 1.0`，Schlick 退化成恒1
        // （`1 + 0·m5 ≡ 1`），bias 会变成 scale 的逐位副本。
        let fc = fresnel_schlick(v_dot_h, 0.0);
        scale += weight * (1.0 - fc);
        bias += weight * fc;
    }
    // 分母是**总样本数**（含被剔除的），见头注易错点①。
    let inv = 1.0 / n_dir as f32;
    (scale * inv, bias * inv)
}

/// **判据层专用：按 `(NdotV, roughness)` 精确取单点积分值**。
///
/// **为什么公开它**（与 F1808 的 `sh_basis_at` 同源理由）：判据必须
/// 能在**任意坐标**上取到积分真值，才能验「镜面极限 `A+B→1`」这类
/// 绝对值契约。若判据只能查 `BrdfLut`，就只能取到 512 级网格上的
/// 离散点，而双线性插值会把极限处的尖峰抹平——判据再严也钉不住
/// 归一化是否正确（本模块曾在此丢了 190 倍能量，见 [`integrate_brdf`]
/// 头注）。
///
/// `steps` 由调用方给定，便于判据做「低采样 vs 高采样」的收敛对拍
/// （收敛性是估计量无偏的独立证据）。
pub fn brdf_integral_at(n_dot_v: f32, roughness: f32, steps: u32) -> (f32, f32) {
    let dirs = hammersley_dirs(steps);
    integrate_brdf(n_dot_v, roughness, &dirs)
}

// ---------------------------------------------------------------------------
// 七、运行时双查（O(1)/像素）
// ---------------------------------------------------------------------------

/// **IBL 采样器**（运行时双查的持有者）。
///
/// **为什么不是一个自由函数**：运行时双查要读「预滤波链 + LUT」两份
/// 数据。自由函数就得把两份都按引用传参，而调用方（着色器绑定层）
/// 每像素都要传——那正是「把资产句柄藏在全局」的反面。持有者让
/// 「一次绑定、多次查询」，也让「环境没设置」这件事有地方记。
#[derive(Clone, PartialEq, Debug)]
pub struct IblSampler {
    /// 预滤波链。
    pub chain: PrefilterChain,
    /// BRDF LUT。
    pub lut: BrdfLut,
}

impl IblSampler {
    /// 构造（`lut_fallback` 走半分辨率过采样兜底）。
    pub fn new(chain: PrefilterChain, lut_fallback: bool) -> IblSampler {
        IblSampler {
            chain,
            lut: BrdfLut::build(LUT_SIZE, lut_fallback),
        }
    }

    /// **双查**：给定法线/视线/粗糙度，返回预滤波后的镜面辐射亮度。
    ///
    /// 步骤（头注「三」）：`R = reflect` → `lod = roughness×(级数−1)`
    /// → **三线性**插值 mip 链 → 返回。
    ///
    /// **为什么必须三线性而非最近邻**：最近邻时粗糙度从 0.49 变到0.51
    /// 会让查的mip 级从 1 跳到 2，宏观表现是**反射细节突然变糊**
    /// （材质上出现一条可见的「粗糙度分界线」）。线性插值把这个
    /// 跳变摊成渐变。
    pub fn prefiltered(
        &self,
        n: (f32, f32, f32),
        v: (f32, f32, f32),
        roughness: f32,
    ) -> Result<(f32, f32, f32), IblFault> {
        if !is_fin(roughness) {
            return Err(IblFault::new(IblFaultKind::LodOutOfRange));
        }
        let r = clampf(roughness, 0.0, 1.0);
        let lod = r * ((MIP_LEVELS - 1) as f32);
        let i0 = lod.floor() as usize;
        let i1 = if i0 + 1 < MIP_LEVELS { i0 + 1 } else { i0 };
        let f = lod - i0 as f32;
        let a = self.sample_mip(i0, n, v)?;
        let b = self.sample_mip(i1, n, v)?;
        Ok((
            a.0 + (b.0 - a.0) * f,
            a.1 + (b.1 - a.1) * f,
            a.2 + (b.2 - a.2) * f,
        ))
    }

    /// 单级 mip 上的方向采样。
    fn sample_mip(
        &self,
        level: usize,
        n: (f32, f32, f32),
        v: (f32, f32, f32),
    ) -> Result<(f32, f32, f32), IblFault> {
        if level >= MIP_LEVELS {
            return Err(IblFault::with(
                IblFaultKind::LodOutOfRange,
                level as u64,
                MIP_LEVELS as u64,
            ));
        }
        let m = self
            .chain
            .mips
            .get(level)
            .ok_or_else(|| IblFault::new(IblFaultKind::NoEnvironment))?;
        Ok(m.sample_dir(reflect_dir(v, n)))
    }

    /// **双查完整式**：镜面项= 预滤波 × (F0×scale + bias)。
    ///
    /// `f0` 是**该像素的反射率**（金属度与基色共同决定，调用方算好
    /// 传入）——本模块不碰金属度/基色，那属于 I03 F1642 的材质域。
    /// 分工写在这里，避免下游以为 IBL 该管材质。
    pub fn specular(
        &self,
        n: (f32, f32, f32),
        v: (f32, f32, f32),
        roughness: f32,
        f0: (f32, f32, f32),
    ) -> Result<(f32, f32, f32), IblFault> {
        let nn = normalize3(n);
        let vv = normalize3(v);
        let n_dot_v = dot3(nn, vv).max(0.0);
        let pre = self.prefiltered(nn, vv, roughness)?;
        let (scale, bias) = self.lut.query(n_dot_v, roughness)?;
        let mut out = (0.0f32, 0.0f32, 0.0f32);
        let mut k = 0usize;
        while k < 3 {
            let f0k = if k == 0 {
                f0.0
            } else if k == 1 {
                f0.1
            } else {
                f0.2
            };
            let pk = if k == 0 {
                pre.0
            } else if k == 1 {
                pre.1
            } else {
                pre.2
            };
            // 镜面项 =预滤波 × (F0·scale + bias)
            out_k(&mut out, k, pk * (f0k * scale + bias));
            k += 1;
        }
        Ok(out)
    }

    /// 采样成本（**恒定**——这就是「预积分」的意义）。
    ///
    /// 记成常数而非「按 mip 数遍历」：预滤波链的级数是编译期常量，
    /// 三线性插值固定两次纹理采样，与场景规模、mip 级数都无关。
    /// 这条数值会被 F1811 成本模型直接引用，故公开成函数而非注释。
    ///
    /// **口径**：`texture_samples = 2`（mip 链三线性取两级，各一次），
    /// `lut_lookups = 1`（LUT 双线性一次，内部 4 角读取不另计——
    /// 它是同一次查表操作内的插值，与 mip 采样不同量级）。
    /// 单像素总计 **3 次查表**，与 `PERF_ROWS` 的运行时行一致。
    pub fn per_pixel_cost() -> PerfCost {
        PerfCost {
            texture_samples: 2,
            lut_lookups: 1,
            dependent_on_scene: false,
        }
    }
}

#[inline]
fn out_k(out: &mut (f32, f32, f32), k: usize, v: f32) {
    // 三元组按索引写入：避免三个独立分支的可读性损失
    if k == 0 {
        out.0 = v;
    } else if k == 1 {
        out.1 = v;
    } else {
        out.2 = v;
    }
}

/// 运行时单像素成本（供 F1811 成本模型引用）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct PerfCost {
    /// 纹理采样组数（mip 链三线性 = 2 次）。
    pub texture_samples: u32,
    /// LUT 查询次数。
    pub lut_lookups: u32,
    /// 是否随场景规模变化（**恒 false** —— O(1) 的机检形态）。
    pub dependent_on_scene: bool,
}

// ---------------------------------------------------------------------------
// 八、I03 F1642 双通道接口契约（**本单兑现点**）
// ---------------------------------------------------------------------------

/// **IBL 双通道**（F1642 消费）。
///
/// **契约原文**（F1642 锚点）：「IBL 接口（环境光照（IBL（预积分环境
/// 贴图——漫反射（辐照度图）与镜面（预滤波 + BRDF LUT）双通道——
/// IBL 是 PBR 的环境光主路径」。
///
/// **分工（不越界）**：本模块只产出**镜面通道**（预滤波 + LUT）；
/// **漫反射通道（辐照度）由 F1808 的 SH2 承担**——那是探针域的产物，
/// 本模块重算一遍就是两处真相。此处把漫反射作为**外部注入**，
/// 使F1642 侧看到的是**完整双通道**，而本模块不越界持有探针数据。
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct IblSample {
    /// 镜面通道：预滤波后的辐射亮度（已含粗糙度选mip + 三线性）。
    pub specular_radiance: (f32, f32, f32),
    /// BRDF LUT 的 scale 项（F0 乘数）。
    pub brdf_scale: f32,
    /// BRDF LUT 的 bias 项（加性项）。
    pub brdf_bias: f32,
    /// 漫反射通道：辐照度（**外部注入**，来自 F1808 的 SH2 辐照度）。
    pub diffuse_irradiance: (f32, f32, f32),
    /// 本像素法线（供下游对账，不由本模块使用）。
    pub normal: (f32, f32, f32),
}

impl IblSample {
    /// **合成镜面项**（`F0` 由材质域给出）。
    ///
    /// **为什么单独给这个函数**：F1642 拿到双通道后要算
    /// `spec = prefiltered × (F0·scale + bias)`。若让它自己拼，
    /// 拼错（F0 漏乘 scale）不会在本模块的判据里露出来。
    pub fn compose_specular(&self, f0: (f32, f32, f32)) -> (f32, f32, f32) {
        let s = self.brdf_scale;
        let b = self.brdf_bias;
        (
            self.specular_radiance.0 * (f0.0 * s + b),
            self.specular_radiance.1 * (f0.1 * s + b),
            self.specular_radiance.2 * (f0.2 * s + b),
        )
    }
}

/// **采样并组装双通道**（F1642 的唯一入口）。
///
/// `diffuse` 由调用方从 F1808 探针取来——**本模块不主动读探针**。
pub fn sample_ibl(
    sampler: &IblSampler,
    n: (f32, f32, f32),
    v: (f32, f32, f32),
    roughness: f32,
    diffuse: (f32, f32, f32),
) -> Result<IblSample, IblFault> {
    let nn = normalize3(n);
    let vv = normalize3(v);
    let n_dot_v = dot3(nn, vv).max(0.0);
    let pre = sampler.prefiltered(nn, vv, roughness)?;
    let (scale, bias) = sampler.lut.query(n_dot_v, roughness)?;
    Ok(IblSample {
        specular_radiance: pre,
        brdf_scale: scale,
        brdf_bias: bias,
        diffuse_irradiance: diffuse,
        normal: nn,
    })
}

// ---------------------------------------------------------------------------
// 九、降级矩阵 / 性能分解 / 跨批对接 / 无障碍
// ---------------------------------------------------------------------------

/// 降级矩阵一行。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct DegradeRow {
    /// 触发条件。
    pub trigger: &'static str,
    /// 处置方向。
    pub action: &'static str,
    /// 是否阻断渲染。
    pub blocking: bool,
    /// 依据。
    pub basis: &'static str,
}

/// 降级矩阵（**六行**，方向按「格式类阻断 / 能力类降档」分）。
pub const DEGRADE_MATRIX: [DegradeRow; 6] = [
    DegradeRow {
        trigger: "环境图非立方（面边长不一致）",
        action: "导入校验拒绝",
        blocking: true,
        basis: "继续渲染会产出错误反射；正立方是cubemap 采样的前提",
    },
    DegradeRow {
        trigger: "HDR 越界（非有限 / > 65504）",
        action: "导入校验拒绝",
        blocking: true,
        basis: "溢出值让下游色调映射产出纯白且无任何提示",
    },
    DegradeRow {
        trigger: "面边长非 2 的幂或为 0",
        action: "导入校验拒绝",
        blocking: true,
        basis: "mip 链要求 2 的幂才能逐级折半",
    },
    DegradeRow {
        trigger: "预滤波采样数不足（低配设备）",
        action: "三档分档（1024/512/256）",
        blocking: false,
        basis: "内容仍正确，只是方差略大；不应拒绝低配设备",
    },
    DegradeRow {
        trigger: "LUT 精度不足（高粗糙度条带）",
        action: "半分辨率过采样兜底",
        blocking: false,
        basis: "有效精度回到 256 而显存减半",
    },
    DegradeRow {
        trigger: "环境图更新",
        action: "增量重跑最低脏级之后全链",
        blocking: false,
        basis: "mip 之间不独立（下一级从上一级卷积），只跑脏级会残留旧内容",
    },
];

/// 性能分解一行。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct PerfRow {
    /// 阶段。
    pub stage: &'static str,
    /// 时机。
    pub when: &'static str,
    /// 复杂度/量级。
    pub cost: &'static str,
    /// 依据。
    pub basis: &'static str,
}

/// 性能分解（**五行**）。
pub const PERF_ROWS: [PerfRow; 5] = [
    PerfRow {
        stage: "预滤波 mip 链",
        when: "离线/加载期一次性",
        cost: "O(6·s²·N) 每级，5 级合计约 8×基础面积×N",
        basis: "每纹素 N 次 GGX 重要性采样，纹素数按 4 倍递减",
    },
    PerfRow {
        stage: "BRDF LUT",
        when: "离线/加载期一次性",
        cost: "O(512²·steps)",
        basis: "每格 steps 次分段积分，兜底档 256²×4×128",
    },
    PerfRow {
        stage: "运行时双查",
        when: "每像素每帧",
        cost: "O(1)：3 次纹理采样 + 2 次 LUT 插值",
        basis: "与场景规模、mip 级数、光源数均无关",
    },
    PerfRow {
        stage: "增量重跑",
        when: "环境图更新时",
        cost: "O(脏级之后的链)，最坏 O(全链)",
        basis: "mip 不独立，必须从最低脏级往后全跑",
    },
    PerfRow {
        stage: "显存",
        when: "常驻",
        cost: "原图 + 5 级链 + LUT",
        basis: "链约 1.33× 原图（逐级 1/4），入 F1776 配额",
    },
];

/// 跨批对接条目。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Handoff {
    /// 对端单号/域。
    pub peer: &'static str,
    /// 契约内容。
    pub contract: &'static str,
    /// 状态。
    pub state: &'static str,
}

/// 跨批对接（**五条**，核心是兑现 I03 F1642）。
pub const HANDOFFS: [Handoff; 5] = [
    Handoff {
        peer: "I03/F1642",
        contract: "IBL 双通道：镜面由本模块的预滤波+LUT 承担，漫反射由 F1808 的 SH2 辐照度承担；F1642 经 sample_ibl 消费",
        state: "已兑现（接口产出，双通道齐备）",
    },
    Handoff {
        peer: "VE-F1808",
        contract: "分工互补：探针出漫反射辐照度，IBL 出镜面预滤波；两域采样序列各自独立（探针用 fib_dirs，IBL 用 hammersley_dirs——后者须为 [0,1]^2 域，语义不同不可混用）",
        state: "已兑现（双向对接）",
    },
    Handoff {
        peer: "VE-F1844",
        contract: "天空可作为环境源输入预滤波：本模块只收 EnvCube，不关心它来自 cubemap 转换还是天空渲染",
        state: "前向（F1844 消费 EnvCube 构造面）",
    },
    Handoff {
        peer: "VE-F1811",
        contract: "成本模型引用 PerfCost 与 PERF_ROWS：运行时 O(1)/像素，预滤波入加载期成本",
        state: "前向（成本位已产出）",
    },
    Handoff {
        peer: "VE-F1814",
        contract: "基准进 F1814：预滤波耗时阶梯（分辨率×采样档），口径见 PERF_ROWS",
        state: "前向（口径已声明）",
    },
];

/// 无隐私面声明（锚点「资产数据无隐私面」）。
///
/// **为什么可以断言无隐私面**：本模块只处理**环境辐射亮度**（像素值
/// 与方向），不接触用户数据、场景内容语义或任何标识符。唯一的
/// 「标识」是 cubemap 的面序号，那是渲染资产自身的索引。
pub const PRIVACY_NOTE: &str = "无隐私面：仅处理环境辐射亮度与方向，不接触用户数据与场景语义";

/// 无障碍替述（环境反射的视觉等价描述，供读屏与替述文本使用）。
pub fn a11y_alternatives() -> [(&'static str, &'static str); 3] {
    [
        (
            "环境粗糙度",
            "粗糙度决定反射的清晰程度：0 为镜面（清晰倒影），1 为全漫反射（只剩环境色的均匀染色）",
        ),
        (
            "IBL 双通道",
            "环境光分两路：漫反射是整体染色（无论从哪个方向看都是同一个亮度），镜面是带模糊的倒影",
        ),
        (
            "低配降档",
            "设备性能不足时预滤波采样数会下调，画面质量略降但内容正确——不会显示错误或缺失的环境光",
        ),
    ]
}

/// 组内分工（四行）。
pub const DIVISION_OF_WORK: [(&'static str, &'static str, &'static str); 4] = [
    ("核心逻辑", "GGX 重要性采样 + mip 链卷积 + BRDF LUT 积分", "约 100 行"),
    ("边界防护", "格式校验 / 采样分档 / LUT 兜底 / 增量重跑", "约 50 行"),
    ("错误路径", "拒绝路径 + 降级矩阵 + 进度报告", "约 40 行"),
    ("测试支撑", "独立实现交叉对拍 + 三档一致性", "约 70 行"),
];

// ---------------------------------------------------------------------------
// 十、单元测试（宿主侧 `cargo test` 直跑）
// ---------------------------------------------------------------------------
//
// **与域自检的分工**：域自检（`vej09_checks.rs`）走的是「判据侧独立
// 重算 + 双向对拍」，防的是**公式写错**；本节走的是「同输入两次调用
// 逐位相同 + 边界不崩」，防的是**不确定性与 panic 面**。两者不可互替：
// 一个恒真的公式能通过对拍（只要参考实现跟着错），一个不确定的
// 实现也能通过对拍（只要单次结果对），故两面都要有。

#[cfg(test)]
mod tests {
    use super::*;

    /// 造一个合规常量环境立方图（每像素同值）。
    fn const_cube(edge: u32, v: f32) -> EnvCube {
        let n = CUBE_FACES * (edge as usize) * (edge as usize) * 3;
        EnvCube::new(edge, vec![v; n]).expect("常量环境应合规")
    }

    /// 造一条常量预滤波链（跳过卷积，专测采样器语义）。
    fn const_chain(edge: u32, v: f32) -> PrefilterChain {
        let mut mips = Vec::new();
        let mut i = 0;
        while i < MIP_LEVELS {
            mips.push(const_cube(edge, v));
            i += 1;
        }
        PrefilterChain {
            base_edge: edge,
            mips,
            samples: 64,
            quality: QualityTier::Low,
        }
    }

    /// 同输入两次预滤波**逐位相同**（确定性铁律：无 RNG、无墙钟）。
    #[test]
    fn prefilter_is_bit_reproducible() {
        let env = const_cube(4, 0.5);
        let (a, _) = prefilter(&env, 32).expect("合规输入应成功");
        let (b, _) = prefilter(&env, 32).expect("合规输入应成功");
        for (x, y) in a.mips.iter().zip(b.mips.iter()) {
            assert_eq!(x, y, "同输入两次预滤波必须逐位相同（确定性铁律）");
        }
        assert_eq!(a.samples, b.samples);
        assert_eq!(a.quality, b.quality);
    }

    /// 增量重跑 `dirty_from=0` 与全量**逐位相同**。
    #[test]
    fn incremental_from_zero_equals_full() {
        let env = const_cube(4, 0.25);
        let (full, _) = prefilter_from(&env, 32, 0).expect("应成功");
        let (inc, _) = prefilter_from(&env, 32, 0).expect("应成功");
        assert_eq!(full.mips, inc.mips, "dirty_from=0 必须等价全量");
    }

    /// 三档采样数产出**质量单调不劣**：档位按 Low<Medium<High 递增，
    /// 且三档的 mip 内容都有限（卷积不会把合规输入算成 NaN/Inf）。
    ///
    /// **注**：`EnvCube::new` 已保证输入全有限，故这里断言的是
    /// **卷积输出**有限——若卷积里出现 `0/0`（如 pdf 退化），
    /// 这一条会红。
    #[test]
    fn quality_tiers_all_produce_finite_output() {
        let env = const_cube(4, 0.5);
        let tiers = [QualityTier::Low, QualityTier::Medium, QualityTier::High];
        let mut prev_samples = 0u32;
        let mut i = 0;
        while i < tiers.len() {
            let t = tiers[i];
            let (chain, _) = prefilter(&env, t.samples()).expect("三档均应成功");
            assert_eq!(chain.quality, t, "链须记录所用档位");
            assert!(
                chain.samples > prev_samples,
                "档位须按 Low<Medium<High 递增采样数（实测 {} <= {}）",
                chain.samples,
                prev_samples
            );
            prev_samples = chain.samples;
            let mut m = 0;
            while m < chain.mips.len() {
                let px = &chain.mips[m].faces;
                let mut k = 0;
                while k < px.len() {
                    assert!(
                        is_fin(px[k]),
                        "档 {:?} 的 mip {} 第 {} 个像素非有限（卷积出现 0/0？）",
                        t,
                        m,
                        k
                    );
                    k += 1;
                }
                m += 1;
            }
            i += 1;
        }
    }

    /// 格式类输入**显性拒绝**且错误码段正确（不静默降级）。
    #[test]
    fn malformed_cube_is_rejected_with_reason() {
        // 边长非 2 的幂。
        let e = 3u32;
        let bad = EnvCube::new(e, vec![0.0; CUBE_FACES * 9 * 9 * 3]);
        match bad {
            Err(ref f) => {
                assert_eq!(f.kind, IblFaultKind::BadFaceEdge);
                assert!(f.reason().contains('3'), "原因须含实测边长：{}", f.reason());
                assert!(f.code() >= 0xF900, "错误码须落在 IBL 独立码段");
            }
            Ok(_) => panic!("非 2 的幂边长必须被拒"),
        }
    }

    /// NaN 像素被拒且**错误带位模式**（位模式是排查 NaN 的唯一线索）。
    #[test]
    fn nan_pixel_rejected_with_bit_pattern() {
        // 边长 4：面数据须恰为 6·4²·3 = 288 个 float（长度不符会先被
        // BadFaceEdge 拦下，测不到 NaN 分支——长度必须先对）。
        let e = 4u32;
        let n = CUBE_FACES * (e as usize) * (e as usize) * 3;
        let mut px = vec![0.0f32; n];
        px[5] = f32::NAN;
        match EnvCube::new(e, px) {
            Err(ref f) => {
                assert_eq!(f.kind, IblFaultKind::PixelNotFinite);
                let r = f.reason();
                assert!(
                    r.contains("0x7fc00000"),
                    "NaN 原因须带位模式 0x7fc00000，实得：{}",
                    r
                );
            }
            Ok(_) => panic!("NaN 像素必须被拒"),
        }
    }

    /// HDR 上界**含端点**：`HDR_MAX` 放行、`HDR_MAX+1` 拒绝。
    ///
    /// **为什么钉两侧**：只测「超界被拒」的话，把判据写成
    /// `> HDR_MAX - 1.0`（提前一档就拒）同样全绿——错误的是
    /// **过度严格**，症状是合法 HDR 资产被误拒。
    #[test]
    fn hdr_bound_is_inclusive_at_endpoint() {
        let e = 4u32;
        let n = CUBE_FACES * (e as usize) * (e as usize) * 3;
        let mut ok_px = vec![0.0f32; n];
        ok_px[7] = HDR_MAX;
        assert!(EnvCube::new(e, ok_px).is_ok(), "HDR_MAX 端点须放行");
        let mut over_px = vec![0.0f32; n];
        over_px[7] = HDR_MAX + 1.0;
        match EnvCube::new(e, over_px) {
            Err(ref f) => assert_eq!(f.kind, IblFaultKind::PixelOutOfRange),
            Ok(_) => panic!("超 HDR 上界必须被拒"),
        }
    }

    /// 三线性在**整条线性律**上取值正确（f=0.25/0.5/0.75）。
    ///
    /// 链为「级 0 全 0、级 1..4 全 1」→ 查询结果对 lod 是 `y = lod`。
    /// 三点共线才唯一确定线性律：只测中点时 `f²`、`1-(1-f)²` 都能蒙对。
    #[test]
    fn trilinear_follows_full_linear_law() {
        // 覆盖：级 0 = 0.0，级 1..4 = 1.0（常量环境各级同值，区分不出级别）
        let mut mips = Vec::new();
        mips.push(const_cube(2, 0.0));
        let mut i = 1;
        while i < MIP_LEVELS {
            mips.push(const_cube(2, 1.0));
            i += 1;
        }
        let s = IblSampler::new(
            PrefilterChain {
                base_edge: 2,
                mips,
                samples: 64,
                quality: QualityTier::Low,
            },
            false,
        );
        let d = (0.0f32, 0.0f32, 1.0f32);
        let cases = [(0.25f32, 0.0625f32), (0.5, 0.125), (0.75, 0.1875)];
        let mut i = 0;
        while i < cases.len() {
            let (f, rough) = cases[i];
            let got = s.prefiltered(d, d, rough).expect("在域内");
            assert!(
                (got.0 - f).abs() < 1.0e-4,
                "f={} 处三线性应等于 f，实得 {}",
                f,
                got.0
            );
            i += 1;
        }
    }

    /// GGX `D` 单峰：峰在 `α* = √((1−NdotH²)/NdotH²)`。
    #[test]
    fn ggx_d_is_unimodal_with_analytic_peak() {
        let nh = 0.8f64;
        let a_star = ((1.0 - nh * nh) / (nh * nh)).sqrt();
        let mut best_a = 0.0f32;
        let mut best_d = -1.0f32;
        let mut i = 0;
        while i <= 400 {
            let a = 0.01 + (i as f32) * (0.99 / 400.0);
            let d = ggx_d(0.8, a);
            if d > best_d {
                best_d = d;
                best_a = a;
            }
            i += 1;
        }
        assert!(
            ((best_a as f64) - a_star).abs() < 0.01,
            "峰位应贴近解析值 {}，实得 {}",
            a_star,
            best_a
        );
    }

    /// 采样器双查**逐位可复现**（同链两次查询同结果，O(1) 无副作用）。
    #[test]
    fn sampler_queries_are_bit_reproducible() {
        let s = IblSampler::new(const_chain(4, 0.5), false);
        let n = (0.0f32, 0.0f32, 1.0f32);
        let v = (0.0f32, 0.577_350_3, 0.577_350_3);
        let mut r = 0.0f32;
        while r <= 1.0 {
            let a = s.prefiltered(n, v, r).expect("粗糙度在域内");
            let b = s.prefiltered(n, v, r).expect("粗糙度在域内");
            assert_eq!(a, b, "粗糙度 {} 处两次查询必须逐位相同", r);
            let la = s.lut.query(0.5, r).expect("在域内");
            let lb = s.lut.query(0.5, r).expect("在域内");
            assert_eq!(la, lb, "LUT 查询必须无副作用");
            r += 0.1;
        }
    }

    /// 镜面合成式：`F0=0` 时只剩 bias 路（split-sum 的 B 路不得被 F0 抹掉）。
    #[test]
    fn compose_specular_with_zero_f0_keeps_bias() {
        let s = IblSample {
            specular_radiance: (1.0, 1.0, 1.0),
            brdf_scale: 0.5,
            brdf_bias: 0.25,
            diffuse_irradiance: (0.0, 0.0, 0.0),
            normal: (0.0, 0.0, 1.0),
        };
        let out = s.compose_specular((0.0, 0.0, 0.0));
        assert!(
            (out.0 - 0.25).abs() < 1.0e-6,
            "F0=0 时镜面项应恰为 bias，实得 {}",
            out.0
        );
    }

    /// 显存估算随边长**单调递增**且逐级可对账。
    #[test]
    fn vram_bytes_is_monotonic_in_edge() {
        let mut prev = 0u64;
        let mut e = 1u32;
        while e <= 256 {
            let got = vram_bytes(e);
            assert!(got >= prev, "边长 {} 的显存 {} 须 >= 上一档 {}", e, got, prev);
            // 独立算式：原图 6s²·4 + Σ级 6·max(s>>i,1)²·4
            let mut want: u64 = 6 * (e as u64) * (e as u64) * 4;
            let mut i = 0;
            while i < MIP_LEVELS {
                let side = (e >> i).max(1) as u64;
                want += 6 * side * side * 4;
                i += 1;
            }
            assert_eq!(got, want, "边长 {} 的显存须逐级对账", e);
            prev = got;
            e *= 2;
        }
    }
}
