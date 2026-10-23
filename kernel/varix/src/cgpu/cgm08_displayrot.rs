//! CGPU-F1927 · 显示旋转与方向（CGPU-M 域 · 显示组 · 目标 300 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/CGPU Varix STAR II · 总纲与施工书.md#CGPU-F1927`
//!
//! **判据（锚点原文）**：旋转实现、成本标注、坐标联动、三组、判据。
//!
//! **职责定位（锚点原文摘录）**：旋转（显示旋转（0/90/180/270——旋转实现（性能（旋转性能标注（旋转合成成本——成本标注；触屏（触屏坐标联动——坐标联动；测试（旋转/成本/联动三组）。
//!
//! ## 一、旋转实现（判据一）
//!
//! 显示旋转四档（[`Rotation`]）：0/90/180/270，全部为整数闭式变换、
//! 零浮点。每个方向有两条闭式映射：
//!
//! * 正向（面板原生 → 显示帧，[`Rotation::forward`]）：合成器输出摆放
//!   用的映射；
//! * 逆向（显示帧 → 面板原生，[`Rotation::inverse`]）：即正向的逆映射，
//!   等价于反向旋转（[`Rotation::opposite`]）。
//!
//! 数学基准（面板原生 W×H，x 向右、y 向下，顺时针为正）：
//!
//! * Deg0：`(x,y) → (x,y)`（恒等）。
//! * Deg90（顺时针）：`(x,y) → (H-1-y, x)`。显示帧尺寸 `H×W`。
//! * Deg180：`(x,y) → (W-1-x, H-1-y)`（点对称）。显示帧尺寸 `W×H`。
//! * Deg270（顺时针 270 = 逆时针 90）：`(x,y) → (y, W-1-x)`。
//!   显示帧尺寸 `H×W`。
//!
//! 扫描线重排（[`ScanPlan`]）把"整帧旋转"分解为逐目标行的源走线计划：
//! 0 档是行内顺序走线、180 档是行内逆序走线（仍为连续地址，缓存友好）、
//! 90/270 档是**跨步列走线**（步长 = 源行宽，地址跳跃打碎缓存行——这
//! 正是成本标注里 90/270 档高于 0/180 档的微观来源）。运行时计划与
//! 手工推导的 3×2 常量表逐项对拍（[`ScanPlan::verify_canonical`]），
//! 计划生成器若与闭式推导漂移立即报 [`DisplayRotError::PlanMismatch`]。
//!
//! ## 二、成本标注（判据二）
//!
//! 成本常量表（[`RotCostTable`]）按四档各给一行：每像素周期（×100 整数
//! 定标，零浮点）、带宽倍率（‰）、是否需中转缓冲、以及理由注释。结论
//! 写死在表里：0 档流式直拷最便宜；180 档行内逆序、连续地址但要正反
//! 两次读计价；90/270 档跨步写入、缓存行利用率坍塌且写合并方向不同
//! （270 档顺写合并稍好于 90 档逆写），须行中转缓冲。
//! [`RotCostTable::estimate_cost`] 用整数算术给出整帧估算（饱和与显性
//! 溢出错误两条路，绝不静默回绕）。
//!
//! ## 三、触屏坐标联动（判据三）
//!
//! 触屏控制器贴在面板上、报的是**显示帧坐标**（用户看到的点）；内核
//! 其余子系统要的是**面板原生坐标**（帧缓冲寻址用）。联动
//! （[`TouchMapper`]）用四方向各自的闭式逆映射（推导见各 match 臂注释，
//! 与判据一的逆映射完全一致），提供三路：严格路（越界报
//! [`DisplayRotError::CoordOutOfRange`]）、钳制路（夹回边界并把夹边
//! 入账 [`TouchClampStats`]）、对拍自证路
//! （[`TouchMapper::roundtrip_selfcheck`]，正向∘逆向恒等于恒等，逐点
//! 验证）。数学依据一句话：逆向 = 反向旋转，而反向旋转的闭式映射就是
//! 把正向映射里的 W/H 与 x/y 对调后解线性方程组得到的唯一解（四个
//! 方向都是平移+翻转的复合，逆必存在且唯一）。
//!
//! ## 四、三组与判据（判据四）
//!
//! 测试三组：**旋转组**（四方向往返对拍、缓冲旋转 90 后 270 还原）、
//! **成本组**（成本档位单调、估算量级与方向排序）、**联动组**（四方向
//! 坐标往返、钳制入账、越界拒绝）。另有状态机乱序迁移拒绝、事件账回绕、
//! 快照往返三组工程兜底测试。全部落在文件尾 `#[cfg(test)]`。
//!
//! **对接**：F1925（模式管理——旋转随模式切换重新仲裁）；F1923（EDID——
//! 面板原生方向声明上游）；F1922（热插拔——方向声明随显示器到达重建）。
//! 零 panic 面、零 IO、零墙钟、零浮点、no_std + alloc、不依赖 crate 内
//! 其他模块（完全自包含）。

use alloc::string::String;
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 一、规格常量与版本
// ---------------------------------------------------------------------------

/// 本项版本。
pub const DISPLAYROT_VERSION: &str = "F1927-displayrot-v1";

/// 事件账环形容量（256 条——2 的幂便于位与取模；覆盖式回绕）。
/// 来源：本模块规格"定长 256 环形"。
pub const RING_CAP: usize = 256;

/// 目标行字节跨度默认对齐（来源：UEFI GOP / VESA 显示控制器惯例——
/// 行跨度按 4 字节对齐；设备可用 [`DisplayRotDevice::set_stride_align`]
/// 覆盖，0 表示不检查）。
pub const DEFAULT_STRIDE_ALIGN: u32 = 4;

/// 允许的每像素字节数闭集（1/2/4——U5/U6/U8 三种打包；3 字节 BGR 打包
/// 因跨步对齐与写合并都差，本面不收）。
pub const ALLOWED_BPP: [u32; 3] = [1, 2, 4];

/// 触屏对拍自证的全量遍历上限（像素数不超过该值则逐点全查，否则按
/// 网格抽样——上限来源：4096 像素对应 64×64，全查循环在内核路径的
/// 可接受上界）。
pub const SELFCHECK_FULL_PIXELS: u64 = 4096;

// ---------------------------------------------------------------------------
// 二、诊断码（0x12C 段独占：F1927 显示旋转与方向）
// ---------------------------------------------------------------------------

/// 零尺寸拒绝（宽或高为 0——旋转无意义）。
pub const X_ROT_ZERO_DIM: u32 = 0x12C0;
/// 尺寸/像素计数溢出（u64 乘法不可承载）。
pub const X_ROT_DIM_OVERFLOW: u32 = 0x12C1;
/// 未知旋转档（闭集四档之外）。
pub const X_ROT_UNKNOWN_ROT: u32 = 0x12C2;
/// 状态机乱序迁移（当前态到目标态无迁移边）。
pub const X_ROT_BAD_TRANSITION: u32 = 0x12C3;
/// 运行时计划与常量闭式表不一致（计划生成器漂移）。
pub const X_ROT_PLAN_MISMATCH: u32 = 0x12C4;
/// 计划构建失败（走线越界/步长非法）。
pub const X_ROT_PLAN_BUILD: u32 = 0x12C5;
/// 输入缓冲不足（字节数与 W×H×BPP 不符）。
pub const X_ROT_BUF_UNDERSIZED: u32 = 0x12C6;
/// 触点越界（严格路拒绝）。
pub const X_ROT_COORD_RANGE: u32 = 0x12C7;
/// 快照格式非法（魔数/版本/长度不符）。
pub const X_ROT_SNAPSHOT_MALFORMED: u32 = 0x12C8;
/// 快照指纹不符（内容被改）。
pub const X_ROT_SNAPSHOT_FP: u32 = 0x12C9;
/// 成本估算溢出（饱和前的显性检出）。
pub const X_ROT_COST_OVERFLOW: u32 = 0x12CA;
/// 往返对拍失败（正向∘逆向 ≠ 恒等）。
pub const X_ROT_ROUNDTRIP: u32 = 0x12CB;
/// 仲裁拒绝（面板原生与用户设置都不可用）。
pub const X_ROT_ARBIT_DENIED: u32 = 0x12CC;
/// 事件账自检不符（计数与环内容对不上）。
pub const X_ROT_LEDGER: u32 = 0x12CD;
/// 目标行字节跨度对齐不合法（90/270 宽高交换后的行跨度失配）。
pub const X_ROT_STRIDE_ALIGN: u32 = 0x12CE;
/// 保留码（内部未分类——出现即为本模块缺陷）。
pub const X_ROT_RESERVED: u32 = 0x12CF;

// ---------------------------------------------------------------------------
// 三、错误类型
// ---------------------------------------------------------------------------

/// 显示旋转面统一错误。每变体携带定位信息；诊断经 [`DisplayRotError::code`]
/// （0x12C 段十六码闭集）与 [`DisplayRotError::detail`]（中文可操作诊断）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DisplayRotError {
    /// 零尺寸拒绝。
    ZeroDimension {
        /// 拒绝来源（"plan"/"cost"/"mapper"……）。
        who: &'static str,
    },
    /// 尺寸/像素计数溢出。
    DimOverflow {
        /// 溢出发生的算式名。
        who: &'static str,
    },
    /// 未知旋转档（携带原始码）。
    UnknownRotation(u8),
    /// 状态机乱序迁移。
    BadTransition {
        /// 迁移前状态。
        from: RotState,
        /// 被拒目标状态。
        to: RotState,
    },
    /// 运行时计划与常量表不一致。
    PlanMismatch {
        /// 不一致的目标行号。
        row: u32,
        /// 不一致字段名。
        field: &'static str,
    },
    /// 计划构建失败。
    PlanBuildFailed {
        /// 失败原因短语。
        reason: &'static str,
    },
    /// 输入缓冲不足。
    BufferUndersized {
        /// 需要字节数。
        need: u64,
        /// 实得字节数。
        got: u64,
    },
    /// 触点越界。
    CoordOutOfRange {
        /// 触点显示帧 x。
        x: u32,
        /// 触点显示帧 y。
        y: u32,
        /// 显示帧宽。
        w: u32,
        /// 显示帧高。
        h: u32,
    },
    /// 快照格式非法。
    SnapshotMalformed {
        /// 出错字节偏移。
        off: usize,
        /// 原因短语。
        why: &'static str,
    },
    /// 快照指纹不符。
    SnapshotFingerprint {
        /// 实得指纹。
        got: u32,
        /// 期望指纹。
        want: u32,
    },
    /// 成本估算溢出。
    CostOverflow {
        /// 溢出算式名。
        who: &'static str,
    },
    /// 往返对拍失败。
    RoundtripFailed {
        /// 失败方向。
        rot: Rotation,
        /// 失败点显示帧 x。
        x: u32,
        /// 失败点显示帧 y。
        y: u32,
    },
    /// 仲裁拒绝。
    ArbitrationDenied {
        /// 拒绝原因短语。
        why: &'static str,
    },
    /// 事件账自检不符。
    LedgerInconsistent {
        /// 原因短语。
        why: &'static str,
    },
    /// 行跨度对齐不合法。
    StrideMisaligned {
        /// 目标行字节数。
        stride_bytes: u32,
        /// 要求对齐。
        align: u32,
    },
}

impl DisplayRotError {
    /// 诊断码（0x12C 段闭集）。
    pub fn code(&self) -> u32 {
        match self {
            DisplayRotError::ZeroDimension { .. } => X_ROT_ZERO_DIM,
            DisplayRotError::DimOverflow { .. } => X_ROT_DIM_OVERFLOW,
            DisplayRotError::UnknownRotation(_) => X_ROT_UNKNOWN_ROT,
            DisplayRotError::BadTransition { .. } => X_ROT_BAD_TRANSITION,
            DisplayRotError::PlanMismatch { .. } => X_ROT_PLAN_MISMATCH,
            DisplayRotError::PlanBuildFailed { .. } => X_ROT_PLAN_BUILD,
            DisplayRotError::BufferUndersized { .. } => X_ROT_BUF_UNDERSIZED,
            DisplayRotError::CoordOutOfRange { .. } => X_ROT_COORD_RANGE,
            DisplayRotError::SnapshotMalformed { .. } => X_ROT_SNAPSHOT_MALFORMED,
            DisplayRotError::SnapshotFingerprint { .. } => X_ROT_SNAPSHOT_FP,
            DisplayRotError::CostOverflow { .. } => X_ROT_COST_OVERFLOW,
            DisplayRotError::RoundtripFailed { .. } => X_ROT_ROUNDTRIP,
            DisplayRotError::ArbitrationDenied { .. } => X_ROT_ARBIT_DENIED,
            DisplayRotError::LedgerInconsistent { .. } => X_ROT_LEDGER,
            DisplayRotError::StrideMisaligned { .. } => X_ROT_STRIDE_ALIGN,
        }
    }

    /// 中文可操作诊断（static 串，零分配）。
    pub fn detail(&self) -> &'static str {
        match self {
            DisplayRotError::ZeroDimension { .. } => {
                "宽或高为 0：旋转无意义，请先确认显示模式已闭合（F1925）再建旋转面"
            }
            DisplayRotError::DimOverflow { .. } => {
                "尺寸或像素计数超出 u64：请缩小显示模式或分块处理（溢出算式名见 who 字段）"
            }
            DisplayRotError::UnknownRotation(v) => match *v {
                0..=3 => "旋转码落在闭集内却报未知：本模块缺陷，请报 0x12C2 现场副本",
                _ => "旋转码超出 0..=3 闭集：请只传 Rotation 枚举或 0/1/2/3（0x12C2）",
            },
            DisplayRotError::BadTransition { .. } => {
                "状态机乱序迁移：请按 Idle→Planned→Committing→Applied 主链推进，失败走 Committing→Rolledback，Applied/Rolledback 走 reset 回 Idle"
            }
            DisplayRotError::PlanMismatch { .. } => {
                "运行时扫描计划与 3×2 常量闭式表不一致：计划生成器已漂移，请核对 ScanPlan::build 各方向的闭式推导后重编"
            }
            DisplayRotError::PlanBuildFailed { .. } => {
                "扫描计划构建失败：走线起点或步长越界，请检查宽高与旋转档是否配套传入"
            }
            DisplayRotError::BufferUndersized { .. } => {
                "输入缓冲字节数与 宽×高×BPP 不符：请核对上送缓冲长度与当前模式 BPP（允许 1/2/4）"
            }
            DisplayRotError::CoordOutOfRange { .. } => {
                "触点超出显示帧范围：请走钳制路（map_to_panel_clamped）或在触控过滤层丢弃野点"
            }
            DisplayRotError::SnapshotMalformed { .. } => {
                "快照魔数/版本/长度非法：请只用本设备 export_plan_snapshot 产出的快照回放"
            }
            DisplayRotError::SnapshotFingerprint { .. } => {
                "快照指纹不符：快照内容在传输或落盘时被改，请重导出并核对链路完整性"
            }
            DisplayRotError::CostOverflow { .. } => {
                "成本估算乘法溢出：请缩小模式或按分块估算后求和（饱和值不可用作预算）"
            }
            DisplayRotError::RoundtripFailed { .. } => {
                "正向∘逆向对拍失败：坐标逆映射与正映射失配，请核对四方向闭式推导后重编"
            }
            DisplayRotError::ArbitrationDenied { .. } => {
                "面板原生方向与用户设置均不可用：请先 declare_native 上报 ACPI/EDID 声明或 set_user_rotation"
            }
            DisplayRotError::LedgerInconsistent { .. } => {
                "事件账计数与环内容不符：账面已损，请导出快照留证后重建设备"
            }
            DisplayRotError::StrideMisaligned { .. } => {
                "90/270 宽高交换后的目标行字节数不满足对齐：请换 BPP（1/2/4）或在对齐更强的模式上旋转"
            }
        }
    }
}

// ---------------------------------------------------------------------------
// 四、Rotation 四档枚举与坐标变换
// ---------------------------------------------------------------------------

/// 显示旋转四档（顺时针为正）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Rotation {
    /// 0°（恒等——面板原生即显示帧）。
    Deg0,
    /// 90°（顺时针——显示帧宽高互换为 H×W）。
    Deg90,
    /// 180°（点对称——显示帧尺寸不变）。
    Deg180,
    /// 270°（顺时针 270 = 逆时针 90——显示帧宽高互换为 H×W）。
    Deg270,
}

impl Rotation {
    /// 闭集全集（供遍历与对拍）。
    pub const ALL: [Rotation; 4] = [
        Rotation::Deg0,
        Rotation::Deg90,
        Rotation::Deg180,
        Rotation::Deg270,
    ];

    /// 稳定序号（0..=3；快照与账面都用它落盘）。
    pub const fn to_u8(self) -> u8 {
        match self {
            Rotation::Deg0 => 0,
            Rotation::Deg90 => 1,
            Rotation::Deg180 => 2,
            Rotation::Deg270 => 3,
        }
    }

    /// 由序号还原；越界返回 `None`（const 可构造 Option，禁止 panic）。
    pub const fn from_u8(v: u8) -> Option<Rotation> {
        match v {
            0 => Some(Rotation::Deg0),
            1 => Some(Rotation::Deg90),
            2 => Some(Rotation::Deg180),
            3 => Some(Rotation::Deg270),
            _ => None,
        }
    }

    /// 人读名（诊断与账面用）。
    pub const fn name(self) -> &'static str {
        match self {
            Rotation::Deg0 => "0",
            Rotation::Deg90 => "90",
            Rotation::Deg180 => "180",
            Rotation::Deg270 => "270",
        }
    }

    /// 是否为宽高互换档（90/270——180 不换、0 不换）。
    pub const fn is_swap(self) -> bool {
        matches!(self, Rotation::Deg90 | Rotation::Deg270)
    }

    /// 顺时针四分之一圈数（0..=3）。
    pub const fn quarter_turns(self) -> u8 {
        self.to_u8()
    }

    /// 反向旋转（90↔270，0/180 自逆）——逆映射即按此档再做一次正向。
    pub const fn opposite(self) -> Rotation {
        match self {
            Rotation::Deg0 => Rotation::Deg0,
            Rotation::Deg90 => Rotation::Deg270,
            Rotation::Deg180 => Rotation::Deg180,
            Rotation::Deg270 => Rotation::Deg90,
        }
    }

    /// 循环步进（0→90→180→270→0——用户档位轮换用）。
    pub const fn cycle_next(self) -> Rotation {
        match self {
            Rotation::Deg0 => Rotation::Deg90,
            Rotation::Deg90 => Rotation::Deg180,
            Rotation::Deg180 => Rotation::Deg270,
            Rotation::Deg270 => Rotation::Deg0,
        }
    }

    /// 旋转后显示帧尺寸（整数，零浮点）。
    ///
    /// 90/270 档宽高互换（W×H → H×W）；0/180 档不变。调用方须保证
    /// w/h ≥ 1（零尺寸在上游一律拒绝）。
    pub const fn dims_after(self, w: u32, h: u32) -> (u32, u32) {
        if self.is_swap() {
            (h, w)
        } else {
            (w, h)
        }
    }

    /// 正向闭式映射：面板原生 (x,y) → 显示帧 (xd,yd)。
    ///
    /// 前提：x < w、y < h（越界结果未定义但用 wrapping 保证不 panic）。
    /// 推导见文件头判据一。
    pub const fn forward(self, x: u32, y: u32, w: u32, h: u32) -> (u32, u32) {
        let w64 = w as u64;
        let h64 = h as u64;
        let x64 = x as u64;
        let y64 = y as u64;
        match self {
            Rotation::Deg0 => (x, y),
            Rotation::Deg90 => (
                (h64.wrapping_sub(1).wrapping_sub(y64)) as u32,
                x,
            ),
            Rotation::Deg180 => (
                (w64.wrapping_sub(1).wrapping_sub(x64)) as u32,
                (h64.wrapping_sub(1).wrapping_sub(y64)) as u32,
            ),
            Rotation::Deg270 => (
                y,
                (w64.wrapping_sub(1).wrapping_sub(x64)) as u32,
            ),
        }
    }

    /// 逆向闭式映射：显示帧 (xd,yd) → 面板原生 (x,y)。
    ///
    /// 数学依据：逆向 = 在**显示帧坐标系**（宽高已按本档互换）里做
    /// [`Rotation::opposite`] 档的正向映射——90/270 档的正向定义在
    /// 面板原生坐标系上，逆变换必须先把坐标系的宽高换成显示帧的，
    /// 否则 90 档的逆会算到错位一行的点上。化简后四方向闭式唯一解
    /// （正向是平移+翻转复合，必可逆）。
    pub const fn inverse(self, xd: u32, yd: u32, w: u32, h: u32) -> (u32, u32) {
        let (dw, dh) = self.dims_after(w, h);
        self.opposite().forward(xd, yd, dw, dh)
    }
}

// ---------------------------------------------------------------------------
// 五、RotCostTable 成本常量表与估算
// ---------------------------------------------------------------------------

/// 单方向成本档位行（全部整数定标，零浮点）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RotCostEntry {
    /// 成本档位（0 最便宜，3 最贵；常量编译期断言严格递增）。
    pub tier: u8,
    /// 每像素合成周期 ×100（0.25 周期/像素记 25——整数定标避免浮点）。
    pub cycles_per_pixel_x100: u32,
    /// 带宽倍率（‰，1000 = 1×；≥1000 表示除像素本身外还有额外搬运）。
    pub bandwidth_permille: u32,
    /// 是否需中转缓冲（90/270 需行中转缓冲摊平跨步写）。
    pub needs_staging: bool,
    /// 成本理由（写死在表里，注释即标注）。
    pub reason: &'static str,
}

/// 四档成本常量表（判据二：成本标注）。
///
/// **下标 = `Rotation::to_u8()`**（Deg0/Deg90/Deg180/Deg270 顺序）——
/// 查表走 `ROT_COST_TABLE[rot.to_u8()]`，表序必须按旋转档排，不按
/// 成本序排；成本序（0<180<270<90 的档位递增）由编译期断言按旋转
/// 下标显式核对。
///
/// 定标来源说明（工程依据，非拍脑袋）：
/// * 0 档：源到目标纯流式直拷，顺序读写，预取器全命中——基准 0.25
///   周期/像素（memcpy 量级），带宽 1×。
/// * 180 档：行内逆序，地址仍连续（只是方向反），但写合并要倒着凑、
///   且整帧须读一遍写一遍，计 0.40 周期/像素；带宽按 1.35×（读+写
///   双趟、逆序写合并损失）。
/// * 90 档：目标行对应源列跨步走线，步长 = 源行宽，每步跳一整行——
///   一次缓存行只用到 bpp 字节，缓存行利用率坍到 1/16~1/64；跨步写
///   逆着行方向，写合并最差——0.90 周期/像素，带宽 2.6×，需行中转
///   缓冲。
/// * 270 档：同为跨步列走线，但顺着行方向写，写合并比 90 档好一档
///   （硬件写合并缓冲天然顺写友好）——0.75 周期/像素，带宽 2.2×，
///   需行中转缓冲。
pub const ROT_COST_TABLE: [RotCostEntry; 4] = [
    // [0] Deg0 —— tier 0
    RotCostEntry {
        tier: 0,
        cycles_per_pixel_x100: 25,
        bandwidth_permille: 1000,
        needs_staging: false,
        reason: "0 档：流式直拷，顺序读写预取全命中——基准成本",
    },
    // [1] Deg90 —— tier 3（跨步逆行写，全表最贵）
    RotCostEntry {
        tier: 3,
        cycles_per_pixel_x100: 90,
        bandwidth_permille: 2600,
        needs_staging: true,
        reason: "90 档：跨步列走线逆行写，写合并最差，缓存利用率最低",
    },
    // [2] Deg180 —— tier 1
    RotCostEntry {
        tier: 1,
        cycles_per_pixel_x100: 40,
        bandwidth_permille: 1350,
        needs_staging: false,
        reason: "180 档：行内逆序连续地址，读写双趟，逆序写合并损失",
    },
    // [3] Deg270 —— tier 2（跨步顺行写，次贵）
    RotCostEntry {
        tier: 2,
        cycles_per_pixel_x100: 75,
        bandwidth_permille: 2200,
        needs_staging: true,
        reason: "270 档：跨步列走线顺行写，写合并顺写友好，需行中转缓冲",
    },
];

/// 成本估算结果（判据二输出）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CostEstimate {
    /// 估算方向。
    pub rot: Rotation,
    /// 命中成本档位。
    pub tier: u8,
    /// 参与旋转的像素总数（W×H）。
    pub pixels: u64,
    /// 整帧合成周期估算（向上取整）。
    pub total_cycles: u64,
    /// 整帧搬运字节估算（含倍率，向上取整）。
    pub bytes_moved: u64,
    /// 是否需中转缓冲。
    pub needs_staging: bool,
    /// 该档理由（透传自常量表）。
    pub reason: &'static str,
}

/// 成本常量表面（判据二入口；全部整数运算）。
pub struct RotCostTable;

impl RotCostTable {
    /// 取某方向成本行（按值返回——`RotCostEntry` 为 Copy；未知码返回
    /// 错误，不猜）。
    pub fn entry(rot: Rotation) -> Result<RotCostEntry, DisplayRotError> {
        // 运行时查表：下标来自枚举 to_u8，恒在 0..4，但按纪律仍走显性校验。
        let idx = rot.to_u8() as usize;
        if idx >= ROT_COST_TABLE.len() {
            return Err(DisplayRotError::UnknownRotation(rot.to_u8()));
        }
        Ok(ROT_COST_TABLE[idx])
    }

    /// 整帧成本估算（判据二：`estimate_cost(w,h,rot)`）。
    ///
    /// 零尺寸拒绝；乘法逐级 checked，溢出走
    /// [`DisplayRotError::CostOverflow`]（饱和值不可用作预算，故不静默
    /// 饱和）；周期按 ×100 定标换算并向上取整。
    pub fn estimate_cost(
        w: u32,
        h: u32,
        bpp: u32,
        rot: Rotation,
    ) -> Result<CostEstimate, DisplayRotError> {
        if w == 0 || h == 0 {
            return Err(DisplayRotError::ZeroDimension { who: "cost" });
        }
        let entry = Self::entry(rot)?;
        let pixels = (w as u64)
            .checked_mul(h as u64)
            .ok_or(DisplayRotError::DimOverflow { who: "cost.pixels" })?;
        // 周期：pixels × cpp_x100 / 100，向上取整。
        let raw = pixels
            .checked_mul(entry.cycles_per_pixel_x100 as u64)
            .ok_or(DisplayRotError::CostOverflow { who: "cost.cycles" })?;
        let total_cycles = (raw + 99) / 100;
        // 字节：pixels × bpp × permille / 1000，向上取整。
        let raw_bytes = pixels
            .checked_mul(bpp as u64)
            .ok_or(DisplayRotError::CostOverflow { who: "cost.bytes.bpp" })?
            .checked_mul(entry.bandwidth_permille as u64)
            .ok_or(DisplayRotError::CostOverflow { who: "cost.bytes" })?;
        let bytes_moved = (raw_bytes + 999) / 1000;
        Ok(CostEstimate {
            rot,
            tier: entry.tier,
            pixels,
            total_cycles,
            bytes_moved,
            needs_staging: entry.needs_staging,
            reason: entry.reason,
        })
    }

    /// 四方向整帧估算横向对比（供 perf 面选路：最贵档决定预算上限）。
    pub fn estimate_all(w: u32, h: u32, bpp: u32) -> Result<[CostEstimate; 4], DisplayRotError> {
        Ok([
            Self::estimate_cost(w, h, bpp, Rotation::Deg0)?,
            Self::estimate_cost(w, h, bpp, Rotation::Deg90)?,
            Self::estimate_cost(w, h, bpp, Rotation::Deg180)?,
            Self::estimate_cost(w, h, bpp, Rotation::Deg270)?,
        ])
    }

    /// 最贵方向（预算上限来源；并列取序号大的——保守侧）。
    pub fn worst_case(w: u32, h: u32, bpp: u32) -> Result<Rotation, DisplayRotError> {
        let all = Self::estimate_all(w, h, bpp)?;
        let mut best = 0usize;
        let mut i = 1usize;
        while i < all.len() {
            if all[i].total_cycles >= all[best].total_cycles {
                best = i;
            }
            i += 1;
        }
        Ok(Rotation::from_u8(best as u32 as u8).unwrap_or(Rotation::Deg90))
    }
}

// ---------------------------------------------------------------------------
// 六、ScanPlan 扫描行变换计划（判据一：扫描线重排）
// ---------------------------------------------------------------------------

/// 目标行内源走线方式（闭式四类）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RowWalk {
    /// 行内顺序走线（0 档）——源地址随目标列递增而递增。
    Forward,
    /// 行内逆序走线（180 档）——源地址随目标列递增而递减 1。
    Reverse,
    /// 跨步列走线、顺行方向（270 档）——源地址每步 +step（step=源行宽）。
    ColumnDown {
        /// 跨步步长（恒为源行宽 W）。
        step: u32,
    },
    /// 跨步列走线、逆行方向（90 档）——源地址每步 -step（step=源行宽）。
    ColumnUp {
        /// 跨步步长（恒为源行宽 W）。
        step: u32,
    },
}

impl RowWalk {
    /// 稳定码（常量对拍用；0/1/2/3 分别对应四类）。
    pub const fn code(self) -> u32 {
        match self {
            RowWalk::Forward => 0,
            RowWalk::Reverse => 1,
            RowWalk::ColumnDown { .. } => 2,
            RowWalk::ColumnUp { .. } => 3,
        }
    }

    /// 步长（Forward/Reverse 恒 1；列走线为其 step）。
    pub const fn stride(self) -> u32 {
        match self {
            RowWalk::Forward | RowWalk::Reverse => 1,
            RowWalk::ColumnDown { step } | RowWalk::ColumnUp { step } => step,
        }
    }

    /// 方向符号（+1 递增 / -1 递减——用于计划校验）。
    pub const fn sign(self) -> i32 {
        match self {
            RowWalk::Forward | RowWalk::ColumnDown { .. } => 1,
            RowWalk::Reverse | RowWalk::ColumnUp { .. } => -1,
        }
    }
}

/// 单个目标行走线计划：从源 `src_start` 起、按 `walk` 走 `pixels` 个源
/// 像素，依次填入目标行 `dst_row`。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ScanRow {
    /// 目标行号（0 起始，目标帧内）。
    pub dst_row: u32,
    /// 源起始像素线性下标（y×W+x）。
    pub src_start: u32,
    /// 该行走线像素数（= 目标行宽）。
    pub pixels: u32,
    /// 走线方式。
    pub walk: RowWalk,
}

/// 整帧扫描行变换计划（判据一：旋转实现的执行蓝图）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ScanPlan {
    /// 源宽（面板原生）。
    pub src_w: u32,
    /// 源高（面板原生）。
    pub src_h: u32,
    /// 旋转档。
    pub rot: Rotation,
    /// 目标宽（旋转后）。
    pub dst_w: u32,
    /// 目标高（旋转后）。
    pub dst_h: u32,
    /// 目标行计划（长度恒 = dst_h）。
    pub rows: Vec<ScanRow>,
    /// 计划键（w/h/rot 打包——幂等判定用）。
    pub key: u64,
    /// 像素总数（源 = 目标，校验用）。
    pub pixels_total: u64,
}

/// 常量对拍基准（手工推导的 3×2 全方向计划——判据一的"const 表"）。
///
/// 每行 `[src_start, walk_code, pixels]`；`pixels = 0` 的行为填充哨兵
/// （目标行数不足 3 的方向补齐到 3 行）。推导：
/// * 0 档目标 2 行：行 r 读源行 r 顺序 3 像素。
/// * 90 档目标 3 行：行 yd 从源 (H-1)×W+yd 起跨步 -W 走 2 像素。
/// * 180 档目标 2 行：行 r 从源 (H-1-r)×W+(W-1) 起逆序 3 像素。
/// * 270 档目标 3 行：行 yd 从源 (W-1-yd) 起跨步 +W 走 2 像素。
pub const CANON_W: u32 = 3;
/// 常量对拍基准源高。
pub const CANON_H: u32 = 2;
/// 常量对拍基准表（按 rot.to_u8() 索引）。
pub const CANON_PLAN: [[[u32; 3]; 3]; 4] = [
    // Deg0：目标 2 行（H=2）
    [[0, 0, 3], [3, 0, 3], [0, 0, 0]],
    // Deg90：目标 3 行（W=3），ColumnUp(code 3) 步长 3
    [[3, 3, 2], [4, 3, 2], [5, 3, 2]],
    // Deg180：目标 2 行，Reverse(code 1)
    [[5, 1, 3], [2, 1, 3], [0, 0, 0]],
    // Deg270：目标 3 行，ColumnDown(code 2) 步长 3
    [[2, 2, 2], [1, 2, 2], [0, 2, 2]],
];

impl ScanPlan {
    /// 构建整帧扫描计划（判据一：`scan_plan()`）。
    ///
    /// 零尺寸拒绝；四方向闭式生成逐目标行走线；每行走线做越界预检
    /// （起点 + (像素数-1)×步长 必须落在源帧内），失败报
    /// [`DisplayRotError::PlanBuildFailed`]。
    pub fn build(w: u32, h: u32, rot: Rotation) -> Result<ScanPlan, DisplayRotError> {
        if w == 0 || h == 0 {
            return Err(DisplayRotError::ZeroDimension { who: "plan" });
        }
        let (dst_w, dst_h) = rot.dims_after(w, h);
        let total = (w as u64) * (h as u64);
        let mut rows: Vec<ScanRow> = Vec::with_capacity(dst_h as usize);
        let mut r: u32 = 0;
        while r < dst_h {
            let (start, walk, pixels): (u64, RowWalk, u32) = match rot {
                // 0 档：目标行 r ← 源行 r，顺序。
                Rotation::Deg0 => ((r as u64) * (w as u64), RowWalk::Forward, w),
                // 180 档：目标行 r ← 源行 (H-1-r) 行尾起逆序。
                Rotation::Deg180 => (
                    ((h as u64 - 1 - r as u64) * (w as u64)) + (w as u64 - 1),
                    RowWalk::Reverse,
                    w,
                ),
                // 90 档：目标行 yd ← 源列 yd，自 (H-1,yd) 向上跨步 -W。
                Rotation::Deg90 => (
                    ((h as u64 - 1) * (w as u64)) + (r as u64),
                    RowWalk::ColumnUp { step: w },
                    h,
                ),
                // 270 档：目标行 yd ← 源列 yd，自 (0, W-1-yd) 向下跨步 +W。
                Rotation::Deg270 => (
                    (w as u64 - 1) - (r as u64),
                    RowWalk::ColumnDown { step: w },
                    h,
                ),
            };
            // 越界预检：末像素下标必须 < 总像素数。
            let span = (pixels as u64).saturating_sub(1);
            let last = if walk.sign() < 0 {
                start.checked_sub(span * (walk.stride() as u64))
            } else {
                start.checked_add(span * (walk.stride() as u64))
            };
            match last {
                Some(last_idx) if last_idx < total && pixels > 0 => {}
                _ => {
                    return Err(DisplayRotError::PlanBuildFailed {
                        reason: "row-walk-out-of-bounds",
                    })
                }
            }
            rows.push(ScanRow {
                dst_row: r,
                src_start: start as u32,
                pixels,
                walk,
            });
            r += 1;
        }
        let key = ((w as u64) << 40) | ((h as u64) << 8) | (rot.to_u8() as u64);
        Ok(ScanPlan {
            src_w: w,
            src_h: h,
            rot,
            dst_w,
            dst_h,
            rows,
            key,
            pixels_total: total,
        })
    }

    /// 计划与常量闭式表对拍（判据一：const 表 + 运行时校验一致）。
    ///
    /// 用 3×2 基准逐方向逐行逐字段比对，任一不一致即报
    /// [`DisplayRotError::PlanMismatch`]（携带行号与字段名）。
    pub fn verify_canonical(&self) -> Result<(), DisplayRotError> {
        // 对拍只对基准尺寸有意义；其他尺寸的生成走同一条闭式路径，
        // 基准一致即可判定生成器无方向性漂移。
        if self.src_w != CANON_W || self.src_h != CANON_H {
            let probe = Self::build(CANON_W, CANON_H, self.rot)?;
            return probe.verify_canonical();
        }
        let table = &CANON_PLAN[self.rot.to_u8() as usize];
        let mut i = 0usize;
        while i < table.len() {
            let want = table[i];
            if want[2] == 0 {
                // 哨兵行：运行时计划行数必须恰好等于非哨兵行数。
                if (i as u32) != self.dst_h {
                    return Err(DisplayRotError::PlanMismatch {
                        row: i as u32,
                        field: "dst_h",
                    });
                }
                return Ok(());
            }
            match self.rows.get(i) {
                Some(row) => {
                    if row.src_start != want[0] {
                        return Err(DisplayRotError::PlanMismatch {
                            row: i as u32,
                            field: "src_start",
                        });
                    }
                    if row.walk.code() != want[1] {
                        return Err(DisplayRotError::PlanMismatch {
                            row: i as u32,
                            field: "walk",
                        });
                    }
                    if row.pixels != want[2] {
                        return Err(DisplayRotError::PlanMismatch {
                            row: i as u32,
                            field: "pixels",
                        });
                    }
                }
                None => {
                    return Err(DisplayRotError::PlanMismatch {
                        row: i as u32,
                        field: "rows-missing",
                    })
                }
            }
            i += 1;
        }
        Ok(())
    }

    /// 单像素闭式下标：目标 (dst_row, col) ← 源线性下标。
    ///
    /// 与 build 的逐行生成同式展开；用于 apply 与对拍双确认。
    pub fn src_index_of(&self, dst_row: u32, col: u32) -> Result<u64, DisplayRotError> {
        if dst_row >= self.dst_h || col >= self.dst_w {
            return Err(DisplayRotError::PlanBuildFailed {
                reason: "dst-coord-out-of-plan",
            });
        }
        let row = match self.rows.get(dst_row as usize) {
            Some(r) => r,
            None => {
                return Err(DisplayRotError::PlanBuildFailed {
                    reason: "row-missing",
                })
            }
        };
        if col >= row.pixels {
            return Err(DisplayRotError::PlanBuildFailed {
                reason: "col-out-of-row",
            });
        }
        let base = row.src_start as u64;
        let step = row.walk.stride() as u64;
        let off = (col as u64) * step;
        let idx = if row.walk.sign() < 0 {
            match base.checked_sub(off) {
                Some(v) => v,
                None => return Err(DisplayRotError::PlanBuildFailed { reason: "walk-underflow" }),
            }
        } else {
            match base.checked_add(off) {
                Some(v) => v,
                None => return Err(DisplayRotError::PlanBuildFailed { reason: "walk-overflow" }),
            }
        };
        if idx >= self.pixels_total {
            return Err(DisplayRotError::PlanBuildFailed {
                reason: "src-index-out-of-frame",
            });
        }
        Ok(idx)
    }

    /// 按计划执行整帧旋转（真实像素搬运，整数下标，零浮点）。
    ///
    /// `src` 长度必须恰为 src_w×src_h×bpp；输出为目标帧尺寸的字节缓冲。
    /// 任何下标经 [`ScanPlan::src_index_of`] 闭式复算，双保险。
    pub fn apply(&self, src: &[u8], bpp: u32) -> Result<Vec<u8>, DisplayRotError> {
        if bpp == 0 {
            return Err(DisplayRotError::PlanBuildFailed { reason: "bpp-zero" });
        }
        let need = (self.pixels_total)
            .checked_mul(bpp as u64)
            .ok_or(DisplayRotError::DimOverflow { who: "plan.apply" })?;
        if (src.len() as u64) != need {
            return Err(DisplayRotError::BufferUndersized {
                need,
                got: src.len() as u64,
            });
        }
        let dst_pixels = (self.dst_w as u64) * (self.dst_h as u64);
        let dst_len = dst_pixels
            .checked_mul(bpp as u64)
            .ok_or(DisplayRotError::DimOverflow { who: "plan.apply.dst" })? as usize;
        let mut dst = Vec::with_capacity(dst_len);
        let mut r: u32 = 0;
        while r < self.dst_h {
            let mut c: u32 = 0;
            while c < self.dst_w {
                let idx = self.src_index_of(r, c)?;
                let s_off = (idx * (bpp as u64)) as usize;
                let mut b: u32 = 0;
                while b < bpp {
                    dst.push(src[s_off + b as usize]);
                    b += 1;
                }
                c += 1;
            }
            r += 1;
        }
        Ok(dst)
    }

    /// 幂等性说明的机器可查形式：同键重建得到逐字节相同计划。
    pub fn key_of(w: u32, h: u32, rot: Rotation) -> u64 {
        ((w as u64) << 40) | ((h as u64) << 8) | (rot.to_u8() as u64)
    }
}

// ---------------------------------------------------------------------------
// 七、TouchMapper 触屏坐标联动（判据三）
// ---------------------------------------------------------------------------

/// 钳制入账统计（夹边分类计数——联动账面）。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct TouchClampStats {
    /// x 被夹到下边界的次数。
    pub clamp_x_lo: u64,
    /// x 被夹到上边界的次数。
    pub clamp_x_hi: u64,
    /// y 被夹到下边界的次数。
    pub clamp_y_lo: u64,
    /// y 被夹到上边界的次数。
    pub clamp_y_hi: u64,
}

impl TouchClampStats {
    /// 总钳制次数。
    pub fn total(&self) -> u64 {
        self.clamp_x_lo
            .wrapping_add(self.clamp_x_hi)
            .wrapping_add(self.clamp_y_lo)
            .wrapping_add(self.clamp_y_hi)
    }
}

/// 触屏坐标联动器（判据三）。
///
/// 触控上报显示帧坐标（用户指尖所见），内核帧缓冲寻址要面板原生坐标；
/// 本器用四方向闭式逆映射完成联动，三路出口：
/// * [`TouchMapper::map_to_panel`]：严格路，越界拒绝（0x12C7）；
/// * [`TouchMapper::map_to_panel_clamped`]：钳制路，夹回边界并按边入账；
/// * [`TouchMapper::roundtrip_selfcheck`]：正向∘逆向 = 恒等 的逐点对拍。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TouchMapper {
    panel_w: u32,
    panel_h: u32,
    rot: Rotation,
    disp_w: u32,
    disp_h: u32,
    clamps: TouchClampStats,
    mapped: u64,
    rejected: u64,
}

impl TouchMapper {
    /// 构建（零尺寸拒绝；显示帧尺寸由旋转档闭式导出）。
    pub fn new(w: u32, h: u32, rot: Rotation) -> Result<TouchMapper, DisplayRotError> {
        if w == 0 || h == 0 {
            return Err(DisplayRotError::ZeroDimension { who: "mapper" });
        }
        let (dw, dh) = rot.dims_after(w, h);
        Ok(TouchMapper {
            panel_w: w,
            panel_h: h,
            rot,
            disp_w: dw,
            disp_h: dh,
            clamps: TouchClampStats::default(),
            mapped: 0,
            rejected: 0,
        })
    }

    /// 面板原生宽。
    pub fn panel_w(&self) -> u32 {
        self.panel_w
    }
    /// 面板原生高。
    pub fn panel_h(&self) -> u32 {
        self.panel_h
    }
    /// 当前旋转档。
    pub fn rot(&self) -> Rotation {
        self.rot
    }
    /// 显示帧宽。
    pub fn disp_w(&self) -> u32 {
        self.disp_w
    }
    /// 显示帧高。
    pub fn disp_h(&self) -> u32 {
        self.disp_h
    }
    /// 钳制账面。
    pub fn clamp_stats(&self) -> TouchClampStats {
        self.clamps
    }
    /// 已成功映射次数。
    pub fn mapped(&self) -> u64 {
        self.mapped
    }
    /// 严格路拒绝次数。
    pub fn rejected(&self) -> u64 {
        self.rejected
    }

    /// 严格路：显示帧 → 面板原生（越界报 0x12C7）。
    ///
    /// 各方向逆映射闭式（与判据一 inverse 同式，此处独立展开以便
    /// 两条推导路径互为对拍）：
    /// * 0 档：`(xd,yd) → (xd,yd)`（恒等之逆仍恒等）。
    /// * 90 档：正向 `(x,y)→(H-1-y, x)`，解得 `x=yd, y=H-1-xd`。
    /// * 180 档：正向 `(x,y)→(W-1-x, H-1-y)`，自逆，同式。
    /// * 270 档：正向 `(x,y)→(y, W-1-x)`，解得 `x=W-1-yd, y=xd`。
    pub fn map_to_panel(&mut self, xd: u32, yd: u32) -> Result<(u32, u32), DisplayRotError> {
        if xd >= self.disp_w || yd >= self.disp_h {
            self.rejected = self.rejected.wrapping_add(1);
            return Err(DisplayRotError::CoordOutOfRange {
                x: xd,
                y: yd,
                w: self.disp_w,
                h: self.disp_h,
            });
        }
        let out = self.inverse_closed(xd, yd);
        self.mapped = self.mapped.wrapping_add(1);
        Ok(out)
    }

    /// 钳制路：越界夹回边界并按边入账（联动账面真实可用）。
    pub fn map_to_panel_clamped(&mut self, xd: u32, yd: u32) -> (u32, u32) {
        let (cx, cy) = self.clamp_display(xd, yd);
        self.inverse_closed(cx, cy)
    }

    /// 正向（面板原生 → 显示帧）——对拍自证与摆放核对用。
    pub fn forward(&self, x: u32, y: u32) -> (u32, u32) {
        self.rot.forward(x, y, self.panel_w, self.panel_h)
    }

    /// 逆向闭式独立展开（对拍第二条推导路径）。
    fn inverse_closed(&self, xd: u32, yd: u32) -> (u32, u32) {
        let w64 = self.panel_w as u64;
        let h64 = self.panel_h as u64;
        let xd64 = xd as u64;
        let yd64 = yd as u64;
        match self.rot {
            Rotation::Deg0 => (xd, yd),
            Rotation::Deg90 => (
                yd,
                (h64.wrapping_sub(1).wrapping_sub(xd64)) as u32,
            ),
            Rotation::Deg180 => (
                (w64.wrapping_sub(1).wrapping_sub(xd64)) as u32,
                (h64.wrapping_sub(1).wrapping_sub(yd64)) as u32,
            ),
            Rotation::Deg270 => (
                (w64.wrapping_sub(1).wrapping_sub(yd64)) as u32,
                xd,
            ),
        }
    }

    /// 显示帧坐标夹回显示帧范围并按边入账。
    fn clamp_display(&mut self, xd: u32, yd: u32) -> (u32, u32) {
        let max_x = self.disp_w.wrapping_sub(1);
        let max_y = self.disp_h.wrapping_sub(1);
        let cx = if xd > max_x {
            if self.disp_w > 0 {
                self.clamps.clamp_x_hi = self.clamps.clamp_x_hi.wrapping_add(1);
            }
            max_x
        } else {
            xd
        };
        let cy = if yd > max_y {
            if self.disp_h > 0 {
                self.clamps.clamp_y_hi = self.clamps.clamp_y_hi.wrapping_add(1);
            }
            max_y
        } else {
            yd
        };
        (cx, cy)
    }

    /// 往返对拍自证（判据三：映射往返对拍）。
    ///
    /// 像素数 ≤ [`SELFCHECK_FULL_PIXELS`] 时逐点全查，否则按步长抽样；
    /// 任一点 `inverse(forward(p)) ≠ p` 报
    /// [`DisplayRotError::RoundtripFailed`]（携带失败点与方向）。
    /// 返回实查点数。
    pub fn roundtrip_selfcheck(&mut self) -> Result<u64, DisplayRotError> {
        let total = (self.panel_w as u64) * (self.panel_h as u64);
        let mut checked: u64 = 0;
        let step: u64 = if total <= SELFCHECK_FULL_PIXELS {
            1
        } else {
            // 抽样步长：让抽样点数仍 ≤ 上限（整数除，向上取整）。
            (total + SELFCHECK_FULL_PIXELS - 1) / SELFCHECK_FULL_PIXELS
        };
        let mut i: u64 = 0;
        while i < total {
            let x = (i % self.panel_w as u64) as u32;
            let y = (i / self.panel_w as u64) as u32;
            let (xd, yd) = self.rot.forward(x, y, self.panel_w, self.panel_h);
            let (bx, by) = self.inverse_closed(xd, yd);
            if bx != x || by != y {
                return Err(DisplayRotError::RoundtripFailed {
                    rot: self.rot,
                    x: xd,
                    y: yd,
                });
            }
            checked += 1;
            i += step;
        }
        Ok(checked)
    }
}

// ---------------------------------------------------------------------------
// 八、RotationStateMachine 五态状态机
// ---------------------------------------------------------------------------

/// 旋转状态机五态（Idle→Planned→Committing→Applied 主链；Committing
/// 失败走 Rolledback；Applied/Rolledback 经 reset 回 Idle）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RotState {
    /// 空闲（无在途旋转）。
    Idle,
    /// 已计划（扫描计划已生成并对拍通过）。
    Planned,
    /// 提交中（硬件编程窗口开启——此时可失败回滚）。
    Committing,
    /// 已应用（旋转生效，触屏联动已挂接）。
    Applied,
    /// 已回滚（提交失败后的安全落点）。
    Rolledback,
}

impl RotState {
    /// 稳定序号（迁移表下标）。
    pub const fn to_u8(self) -> u8 {
        match self {
            RotState::Idle => 0,
            RotState::Planned => 1,
            RotState::Committing => 2,
            RotState::Applied => 3,
            RotState::Rolledback => 4,
        }
    }

    /// 人读名（诊断用）。
    pub const fn name(self) -> &'static str {
        match self {
            RotState::Idle => "Idle",
            RotState::Planned => "Planned",
            RotState::Committing => "Committing",
            RotState::Applied => "Applied",
            RotState::Rolledback => "Rolledback",
        }
    }
}

/// 迁移闭集表（行=当前态，列=目标态，1=允许；手工推导）：
///
/// ```text
///            →Planned →Committing →Applied →Rolledback →Idle
/// Idle            1         0          0         0          0
/// Planned         0         1          0         0          0
/// Committing      0         0          1         1          0
/// Applied         0         0          0         0          1
/// Rolledback      0         0          0         0          1
/// ```
pub const ROT_TRANSITIONS: [[u8; 5]; 5] = [
    [0, 1, 0, 0, 0],
    [0, 0, 1, 0, 0],
    [0, 0, 0, 1, 1],
    [1, 0, 0, 0, 0],
    [1, 0, 0, 0, 0],
];

/// 五态旋转状态机（乱序迁移拒绝带码 0x12C3；迁移次数与被拒次数入账）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RotationStateMachine {
    state: RotState,
    transitions: u64,
    rejected: u64,
    last_reject: Option<(RotState, RotState)>,
}

impl Default for RotationStateMachine {
    fn default() -> Self {
        Self::new()
    }
}

impl RotationStateMachine {
    /// 新状态机（初始 Idle）。
    pub const fn new() -> RotationStateMachine {
        RotationStateMachine {
            state: RotState::Idle,
            transitions: 0,
            rejected: 0,
            last_reject: None,
        }
    }

    /// 当前态。
    pub const fn state(&self) -> RotState {
        self.state
    }

    /// 已接受迁移次数。
    pub fn transitions(&self) -> u64 {
        self.transitions
    }

    /// 被拒迁移次数。
    pub fn rejected(&self) -> u64 {
        self.rejected
    }

    /// 最近一次被拒 (from, to)。
    pub fn last_reject(&self) -> Option<(RotState, RotState)> {
        self.last_reject
    }

    /// 迁移表允许性查询（const 表 + 显性界检）。
    pub fn allowed(from: RotState, to: RotState) -> bool {
        let f = from.to_u8() as usize;
        let t = to.to_u8() as usize;
        if f >= ROT_TRANSITIONS.len() || t >= ROT_TRANSITIONS[0].len() {
            return false;
        }
        ROT_TRANSITIONS[f][t] == 1
    }

    /// 尝试迁移（乱序拒绝带码；被拒入账但不改状态）。
    pub fn transition(&mut self, to: RotState) -> Result<RotState, DisplayRotError> {
        if !Self::allowed(self.state, to) {
            self.rejected = self.rejected.wrapping_add(1);
            self.last_reject = Some((self.state, to));
            return Err(DisplayRotError::BadTransition {
                from: self.state,
                to,
            });
        }
        self.state = to;
        self.transitions = self.transitions.wrapping_add(1);
        Ok(to)
    }

    /// reset 迁移（Applied/Rolledback → Idle）的语法糖。
    pub fn reset(&mut self) -> Result<RotState, DisplayRotError> {
        self.transition(RotState::Idle)
    }

    /// 自检：当前态在闭集内且迁移计数不超总尝试（用于 health）。
    pub fn selfcheck(&self) -> Result<(), DisplayRotError> {
        // to_u8 对五态恒在 0..5（枚举闭合），此处校验迁移表一致性。
        if Self::allowed(RotState::Idle, RotState::Committing) {
            return Err(DisplayRotError::LedgerInconsistent {
                why: "transition-table-leak",
            });
        }
        if self.transitions < self.rejected && self.rejected > 0 {
            return Err(DisplayRotError::LedgerInconsistent {
                why: "transition-count-wrong",
            });
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// 九、RotEventLedger 事件账（定长 256 环形，覆盖显性计数）
// ---------------------------------------------------------------------------

/// 事件种类闭集（账面分类计数用）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RotEventKind {
    /// 计划生成。
    Planned,
    /// 提交。
    Committed,
    /// 应用生效。
    Applied,
    /// 回滚。
    RolledBack,
    /// 触点被钳制。
    TouchClamped,
    /// 方向仲裁落判。
    Arbitrated,
    /// 状态机复位。
    Reset,
    /// 状态机拒绝。
    Rejected,
}

impl RotEventKind {
    /// 稳定码（0..=7；账面与快照落盘用）。
    pub const fn to_u8(self) -> u8 {
        match self {
            RotEventKind::Planned => 0,
            RotEventKind::Committed => 1,
            RotEventKind::Applied => 2,
            RotEventKind::RolledBack => 3,
            RotEventKind::TouchClamped => 4,
            RotEventKind::Arbitrated => 5,
            RotEventKind::Reset => 6,
            RotEventKind::Rejected => 7,
        }
    }

    /// 由码还原；越界返回 `None`（不猜）。
    pub const fn from_u8(v: u8) -> Option<RotEventKind> {
        match v {
            0 => Some(RotEventKind::Planned),
            1 => Some(RotEventKind::Committed),
            2 => Some(RotEventKind::Applied),
            3 => Some(RotEventKind::RolledBack),
            4 => Some(RotEventKind::TouchClamped),
            5 => Some(RotEventKind::Arbitrated),
            6 => Some(RotEventKind::Reset),
            7 => Some(RotEventKind::Rejected),
            _ => None,
        }
    }

    /// 人读名。
    pub const fn name(self) -> &'static str {
        match self {
            RotEventKind::Planned => "planned",
            RotEventKind::Committed => "committed",
            RotEventKind::Applied => "applied",
            RotEventKind::RolledBack => "rolledback",
            RotEventKind::TouchClamped => "touch-clamped",
            RotEventKind::Arbitrated => "arbitrated",
            RotEventKind::Reset => "reset",
            RotEventKind::Rejected => "rejected",
        }
    }
}

/// 单条旋转事件（16 字节定长——账面紧凑；kind/rot 存稳定码便于落盘）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RotEvent {
    /// 全账单调序号（u64 回绕前可记 2^64 条）。
    pub seq: u64,
    /// 事件种类稳定码。
    pub kind_code: u8,
    /// 关联旋转档稳定码（255 = 无关）。
    pub rot_code: u8,
    /// 辅助量（视 kind 而定：钳制边码/被拒目标态/代数等）。
    pub aux: u32,
}

impl RotEvent {
    /// 空槽哨兵（kind_code = 0xFF 恒不落在闭集内）。
    pub const EMPTY: RotEvent = RotEvent {
        seq: 0,
        kind_code: 0xFF,
        rot_code: 0xFF,
        aux: 0,
    };

    /// 是否为空槽。
    pub const fn is_empty(self) -> bool {
        self.kind_code == 0xFF
    }

    /// 事件种类（空槽返回 None）。
    pub fn kind(self) -> Option<RotEventKind> {
        RotEventKind::from_u8(self.kind_code)
    }

    /// 关联旋转档（255 返回 None）。
    pub fn rot(self) -> Option<Rotation> {
        Rotation::from_u8(self.rot_code)
    }
}

/// 事件账（判据兜底：定长 256 环形、覆盖式回绕、覆盖显性计数、
/// 分类计数单调——环会丢旧账但总账永不丢）。
pub struct RotEventLedger {
    buf: [RotEvent; RING_CAP],
    seq: u64,
    pushes: u64,
    overwritten: u64,
    kinds: [u64; 8],
}

impl Default for RotEventLedger {
    fn default() -> Self {
        Self::new()
    }
}

impl RotEventLedger {
    /// 新账（全空）。
    pub const fn new() -> RotEventLedger {
        RotEventLedger {
            buf: [RotEvent::EMPTY; RING_CAP],
            seq: 0,
            pushes: 0,
            overwritten: 0,
            kinds: [0; 8],
        }
    }

    /// 入账一条；返回其全账序号。
    pub fn push(&mut self, kind: RotEventKind, rot: Rotation, aux: u32) -> u64 {
        self.seq = self.seq.wrapping_add(1);
        self.pushes = self.pushes.wrapping_add(1);
        let idx = ((self.seq.wrapping_sub(1)) & (RING_CAP as u64 - 1)) as usize;
        if self.pushes > RING_CAP as u64 {
            self.overwritten = self.overwritten.wrapping_add(1);
        }
        self.buf[idx] = RotEvent {
            seq: self.seq,
            kind_code: kind.to_u8(),
            rot_code: rot.to_u8(),
            aux,
        };
        let k = kind.to_u8() as usize;
        self.kinds[k] = self.kinds[k].wrapping_add(1);
        self.seq
    }

    /// 总入账条数（单调，含已被覆盖的）。
    pub fn pushes(&self) -> u64 {
        self.pushes
    }

    /// 已被覆盖的旧账条数（回绕显性计数）。
    pub fn overwritten(&self) -> u64 {
        self.overwritten
    }

    /// 环内现存条数。
    pub fn stored(&self) -> u64 {
        self.pushes.saturating_sub(self.overwritten)
    }

    /// 某种类累计入账数（单调；越界码返回 0——不猜）。
    pub fn kind_count(&self, kind: RotEventKind) -> u64 {
        let k = kind.to_u8() as usize;
        if k >= self.kinds.len() {
            return 0;
        }
        self.kinds[k]
    }

    /// 按环内下标取事件（0 = 最旧现存；越界返回 None）。
    pub fn at(&self, idx: u64) -> Option<RotEvent> {
        if idx >= self.stored() {
            return None;
        }
        // 最旧现存下标 = seq - stored；线性位与取模。
        let base = self.seq.wrapping_sub(self.stored());
        let ev = self.buf[((base + idx) & (RING_CAP as u64 - 1)) as usize];
        Some(ev)
    }

    /// 自检：覆盖计数恒等式 + 分类计数总和恒等式（用于 health）。
    ///
    /// 覆盖式环形账恒有：overwritten == pushes - 256（溢出后），
    /// 且 Σkinds == pushes。任一破缺即账面受损。
    pub fn selfcheck(&self) -> Result<(), DisplayRotError> {
        let expect_over = self.pushes.saturating_sub(RING_CAP as u64);
        if self.overwritten != expect_over {
            return Err(DisplayRotError::LedgerInconsistent {
                why: "overwrite-count-wrong",
            });
        }
        let mut sum: u64 = 0;
        let mut i = 0usize;
        while i < self.kinds.len() {
            sum = sum.wrapping_add(self.kinds[i]);
            i += 1;
        }
        if sum != self.pushes {
            return Err(DisplayRotError::LedgerInconsistent {
                why: "kind-sum-wrong",
            });
        }
        if self.stored() > RING_CAP as u64 {
            return Err(DisplayRotError::LedgerInconsistent {
                why: "stored-over-cap",
            });
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// 十、方向仲裁（面板原生 ACPI/EDID 声明 vs 用户设置）
// ---------------------------------------------------------------------------

/// 面板原生方向声明来源。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NativeSource {
    /// 无声明（面板未报方向）。
    None,
    /// ACPI（如 _PLD 面板方向位）。
    Acpi,
    /// EDID（扩展块内的面板自描述方向）。
    Edid,
}

impl NativeSource {
    /// 稳定码。
    pub const fn to_u8(self) -> u8 {
        match self {
            NativeSource::None => 0,
            NativeSource::Acpi => 1,
            NativeSource::Edid => 2,
        }
    }

    /// 由码还原。
    pub const fn from_u8(v: u8) -> Option<NativeSource> {
        match v {
            0 => Some(NativeSource::None),
            1 => Some(NativeSource::Acpi),
            2 => Some(NativeSource::Edid),
            _ => None,
        }
    }

    /// 人读名。
    pub const fn name(self) -> &'static str {
        match self {
            NativeSource::None => "none",
            NativeSource::Acpi => "acpi",
            NativeSource::Edid => "edid",
        }
    }
}

/// 仲裁策略（冲突时谁赢；策略显性、不隐式）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ArbitPolicy {
    /// 面板原生优先（物理方向可信——出厂朝向标注权威）。
    NativeFirst,
    /// 用户设置优先（用户显式旋过即尊重；面板声明只作缺省）。
    UserFirst,
}

impl ArbitPolicy {
    /// 稳定码。
    pub const fn to_u8(self) -> u8 {
        match self {
            ArbitPolicy::NativeFirst => 0,
            ArbitPolicy::UserFirst => 1,
        }
    }

    /// 由码还原。
    pub const fn from_u8(v: u8) -> Option<ArbitPolicy> {
        match v {
            0 => Some(ArbitPolicy::NativeFirst),
            1 => Some(ArbitPolicy::UserFirst),
            _ => None,
        }
    }

    /// 人读名。
    pub const fn name(self) -> &'static str {
        match self {
            ArbitPolicy::NativeFirst => "native-first",
            ArbitPolicy::UserFirst => "user-first",
        }
    }
}

/// 仲裁裁决者（谁的方向成了 effective）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Arbiter {
    /// 双方一致（无冲突）。
    Agreed,
    /// 面板原生胜。
    Native,
    /// 用户设置胜。
    User,
}

/// 仲裁裁决（留痕：全部输入与理由入账）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ArbitVerdict {
    /// 生效方向（本设备 plan/apply 一律用它）。
    pub effective: Rotation,
    /// 谁拍板。
    pub arbiter: Arbiter,
    /// 是否发生冲突（原生≠用户）。
    pub conflict: bool,
    /// 面板原生方向（无声明时 = 生效方向，作占位记录）。
    pub native: Rotation,
    /// 用户设置方向（未设置时 = 生效方向，作占位记录）。
    pub user: Rotation,
    /// 原生声明来源。
    pub source: NativeSource,
    /// 裁决理由（写死短语，审计可读）。
    pub reason: &'static str,
}

/// 方向仲裁（判据兜底：优先级显性 + 留痕）。
///
/// 规则：
/// 1. 双方都缺 → 拒绝（0x12CC）——不猜方向是底线；
/// 2. 仅一方有 → 该方生效，`Agreed`；
/// 3. 双方一致 → `Agreed`；
/// 4. 冲突 → 按策略（[`ArbitPolicy`]）定胜者，理由入裁决。
pub fn arbitrate(
    source: NativeSource,
    native: Option<Rotation>,
    user: Option<Rotation>,
    policy: ArbitPolicy,
) -> Result<ArbitVerdict, DisplayRotError> {
    match (native, user) {
        (None, None) => Err(DisplayRotError::ArbitrationDenied {
            why: "no-native-no-user",
        }),
        (Some(n), None) => Ok(ArbitVerdict {
            effective: n,
            arbiter: Arbiter::Native,
            conflict: false,
            native: n,
            user: n,
            source,
            reason: "仅面板原生声明可用——原生生效",
        }),
        (None, Some(u)) => Ok(ArbitVerdict {
            effective: u,
            arbiter: Arbiter::User,
            conflict: false,
            native: u,
            user: u,
            source,
            reason: "面板未声明方向——用户设置生效",
        }),
        (Some(n), Some(u)) => {
            if n.to_u8() == u.to_u8() {
                Ok(ArbitVerdict {
                    effective: n,
                    arbiter: Arbiter::Agreed,
                    conflict: false,
                    native: n,
                    user: u,
                    source,
                    reason: "面板原生与用户设置一致",
                })
            } else {
                let (eff, who, why) = match policy {
                    ArbitPolicy::NativeFirst => (n, Arbiter::Native, "冲突：策略面板原生优先"),
                    ArbitPolicy::UserFirst => (u, Arbiter::User, "冲突：策略用户设置优先"),
                };
                Ok(ArbitVerdict {
                    effective: eff,
                    arbiter: who,
                    conflict: true,
                    native: n,
                    user: u,
                    source,
                    reason: why,
                })
            }
        }
    }
}

// ---------------------------------------------------------------------------
// 十一、快照导出（确定性序列化 + FNV-1a 指纹）
// ---------------------------------------------------------------------------

/// FNV-1a 32 位指纹（offset 2166136261 / prime 16777619——公共领域
/// 参数；const 实现，快照与计划键共用）。
pub const fn fnv1a32(bytes: &[u8]) -> u32 {
    let mut hash: u32 = 2166136261;
    let mut i = 0usize;
    while i < bytes.len() {
        hash = (hash ^ (bytes[i] as u32)).wrapping_mul(16777619);
        i += 1;
    }
    hash
}

/// 快照魔数（"VXFR"——Varix display rotation snapshot）。
pub const SNAPSHOT_MAGIC: [u8; 4] = [0x56, 0x58, 0x46, 0x52];
/// 快照格式版本（v1：布局见 [`DisplayRotDevice::export_plan_snapshot`]）。
pub const SNAPSHOT_VERSION: u16 = 1;

/// 导出的确定性快照（字节串 + 指纹；指纹覆盖除指纹字段外的全部字节）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PlanSnapshot {
    /// 确定性序列化字节（定长布局 v1 = 44 字节 + 4 字节指纹）。
    pub bytes: Vec<u8>,
    /// FNV-1a 32 位指纹。
    pub fingerprint: u32,
}

/// 解析后的快照数据。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SnapshotData {
    /// 面板原生宽。
    pub panel_w: u32,
    /// 面板原生高。
    pub panel_h: u32,
    /// 生效方向。
    pub effective: Rotation,
    /// 状态机代数。
    pub generation: u64,
    /// 仲裁策略。
    pub policy: ArbitPolicy,
    /// 原生声明来源。
    pub source: NativeSource,
    /// 原生声明方向。
    pub native_rot: Rotation,
    /// 四边钳制累计（lo_x, hi_x, lo_y, hi_y）。
    pub clamps: [u64; 4],
    /// 触点映射累计。
    pub mapped: u64,
}

/// LE 定宽写助手（运行时；快照布局唯一写入口——确定性来源）。
fn put_u32(dst: &mut Vec<u8>, v: u32) {
    dst.push((v & 0xFF) as u8);
    dst.push(((v >> 8) & 0xFF) as u8);
    dst.push(((v >> 16) & 0xFF) as u8);
    dst.push(((v >> 24) & 0xFF) as u8);
}

/// LE 定宽写助手（u64）。
fn put_u64(dst: &mut Vec<u8>, v: u64) {
    let mut i = 0u32;
    while i < 8 {
        dst.push(((v >> (i * 8)) & 0xFF) as u8);
        i += 1;
    }
}

/// LE 定宽读助手（越界 None——不猜）。
fn get_u32(src: &[u8], off: usize) -> Option<u32> {
    if off + 4 > src.len() {
        return None;
    }
    Some(u32::from_le_bytes([
        src[off],
        src[off + 1],
        src[off + 2],
        src[off + 3],
    ]))
}

/// LE 定宽读助手（u64）。
fn get_u64(src: &[u8], off: usize) -> Option<u64> {
    if off + 8 > src.len() {
        return None;
    }
    let mut b = [0u8; 8];
    let mut i = 0usize;
    while i < 8 {
        b[i] = src[off + i];
        i += 1;
    }
    Some(u64::from_le_bytes(b))
}

// ---------------------------------------------------------------------------
// 十二、DisplayRotDevice 门面
// ---------------------------------------------------------------------------

/// 计划摘要（plan() 的对外视图——不含大缓冲）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PlanSummary {
    /// 生效方向。
    pub rot: Rotation,
    /// 目标宽。
    pub dst_w: u32,
    /// 目标高。
    pub dst_h: u32,
    /// 目标行数（= dst_h）。
    pub rows: u32,
    /// 像素总数。
    pub pixels: u64,
    /// 成本档位。
    pub tier: u8,
    /// 周期估算。
    pub est_cycles: u64,
    /// 字节估算。
    pub est_bytes: u64,
    /// 是否需中转缓冲。
    pub needs_staging: bool,
    /// 计划代数（幂等：同输入重 plan 代数不变）。
    pub generation: u64,
}

/// 旋转体检单条检查。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HealthCheck {
    /// 检查名。
    pub name: &'static str,
    /// 是否通过。
    pub ok: bool,
    /// 细节（中文，含数值——分配型 String，health 不在热路径）。
    pub detail: String,
}

/// 旋转体检汇总。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HealthReport {
    /// 总判定（全部检查通过才为真）。
    pub ok: bool,
    /// 检查明细。
    pub checks: Vec<HealthCheck>,
    /// 关键计数快照（对账用）。
    pub counters: String,
}

/// 整数转十进制（no_std 手写——避免引入 format 依赖面）。
fn fmt_u64(v: u64) -> String {
    if v == 0 {
        return String::from("0");
    }
    let mut buf = [0u8; 20];
    let mut n = v;
    let mut i = buf.len();
    while n > 0 {
        i -= 1;
        buf[i] = b'0' + (n % 10) as u8;
        n /= 10;
    }
    // buf 剩余部分即十进制串（运行时路径，UTF-8 安全：全为 ASCII 数字）。
    let bytes = &buf[i..];
    match String::from_utf8(bytes.to_vec()) {
        Ok(s) => s,
        Err(_) => String::from("?"),
    }
}

/// 显示旋转设备门面：把计划、成本、联动、状态机、事件账、仲裁、快照
/// 串成一个有生命周期边界的子系统。
pub struct DisplayRotDevice {
    panel_w: u32,
    panel_h: u32,
    bpp: u32,
    stride_align: u32,
    user_rot: Option<Rotation>,
    native: Option<(NativeSource, Rotation)>,
    policy: ArbitPolicy,
    verdict: Option<ArbitVerdict>,
    machine: RotationStateMachine,
    ledger: RotEventLedger,
    plan: Option<ScanPlan>,
    plan_gen: u64,
    mapper: Option<TouchMapper>,
    applied_count: u64,
    rolled_count: u64,
    denied_arbit: u64,
}

impl DisplayRotDevice {
    /// 建设备（零尺寸与 BPP 闭集校验；bpp 允许 1/2/4）。
    pub fn new(w: u32, h: u32, bpp: u32) -> Result<DisplayRotDevice, DisplayRotError> {
        if w == 0 || h == 0 {
            return Err(DisplayRotError::ZeroDimension { who: "device" });
        }
        let mut ok_bpp = false;
        let mut i = 0usize;
        while i < ALLOWED_BPP.len() {
            if ALLOWED_BPP[i] == bpp {
                ok_bpp = true;
            }
            i += 1;
        }
        if !ok_bpp {
            return Err(DisplayRotError::PlanBuildFailed {
                reason: "bpp-not-in-1-2-4",
            });
        }
        Ok(DisplayRotDevice {
            panel_w: w,
            panel_h: h,
            bpp,
            stride_align: DEFAULT_STRIDE_ALIGN,
            user_rot: None,
            native: None,
            policy: ArbitPolicy::NativeFirst,
            verdict: None,
            machine: RotationStateMachine::new(),
            ledger: RotEventLedger::new(),
            plan: None,
            plan_gen: 0,
            mapper: None,
            applied_count: 0,
            rolled_count: 0,
            denied_arbit: 0,
        })
    }

    /// 面板原生宽。
    pub fn panel_w(&self) -> u32 {
        self.panel_w
    }
    /// 面板原生高。
    pub fn panel_h(&self) -> u32 {
        self.panel_h
    }
    /// 当前 BPP。
    pub fn bpp(&self) -> u32 {
        self.bpp
    }
    /// 行跨度对齐要求。
    pub fn stride_align(&self) -> u32 {
        self.stride_align
    }
    /// 覆盖行跨度对齐（0 = 不检查）。
    pub fn set_stride_align(&mut self, align: u32) {
        self.stride_align = align;
    }
    /// 当前状态。
    pub fn state(&self) -> RotState {
        self.machine.state()
    }
    /// 状态机（审计视图）。
    pub fn machine(&self) -> &RotationStateMachine {
        &self.machine
    }
    /// 状态机（可变视图——审计/演练迁移表用；业务推进请走
    /// plan/commit/apply/rollback/reset 门面）。
    pub fn machine_mut(&mut self) -> &mut RotationStateMachine {
        &mut self.machine
    }
    /// 事件账（审计视图）。
    pub fn ledger(&self) -> &RotEventLedger {
        &self.ledger
    }
    /// 最近仲裁裁决。
    pub fn verdict(&self) -> Option<ArbitVerdict> {
        self.verdict
    }
    /// 已应用次数。
    pub fn applied_count(&self) -> u64 {
        self.applied_count
    }
    /// 已回滚次数。
    pub fn rolled_count(&self) -> u64 {
        self.rolled_count
    }
    /// 仲裁拒绝次数。
    pub fn denied_arbit(&self) -> u64 {
        self.denied_arbit
    }

    /// 上报面板原生方向声明（F1923/F1922 到达时调用；落账留痕）。
    pub fn declare_native(
        &mut self,
        source: NativeSource,
        rot: Rotation,
    ) -> Result<ArbitVerdict, DisplayRotError> {
        if source.to_u8() == NativeSource::None.to_u8() {
            self.native = None;
        } else {
            self.native = Some((source, rot));
        }
        self.rearbitrate()
    }

    /// 设置用户方向（用户显式旋转；任何状态可设，不迁移状态机——
    /// 生效要等下一次 plan）。
    pub fn set_user_rotation(&mut self, rot: Option<Rotation>) -> Result<ArbitVerdict, DisplayRotError> {
        self.user_rot = rot;
        self.rearbitrate()
    }

    /// 设置仲裁策略。
    pub fn set_policy(&mut self, policy: ArbitPolicy) {
        self.policy = policy;
    }

    /// 内部：重新仲裁并落账。
    fn rearbitrate(&mut self) -> Result<ArbitVerdict, DisplayRotError> {
        let (src, nat) = match self.native {
            Some((s, r)) => (s, Some(r)),
            None => (NativeSource::None, None),
        };
        match arbitrate(src, nat, self.user_rot, self.policy) {
            Ok(v) => {
                self.ledger
                    .push(RotEventKind::Arbitrated, v.effective, v.arbiter_marker());
                self.verdict = Some(v);
                Ok(v)
            }
            Err(e) => {
                self.denied_arbit = self.denied_arbit.wrapping_add(1);
                Err(e)
            }
        }
    }

    /// 行跨度对齐检查（判据兜底：奇偶宽高交换在 90/270 的合法性——
    /// 宽高互换后目标行字节数 = dst_w × bpp，必须满足对齐要求）。
    pub fn check_stride(&self, rot: Rotation) -> Result<(u32, u32), DisplayRotError> {
        if self.stride_align == 0 {
            return Ok(rot.dims_after(self.panel_w, self.panel_h));
        }
        let (dw, _dh) = rot.dims_after(self.panel_w, self.panel_h);
        let stride = dw.wrapping_mul(self.bpp);
        if stride % self.stride_align != 0 {
            return Err(DisplayRotError::StrideMisaligned {
                stride_bytes: stride,
                align: self.stride_align,
            });
        }
        Ok((dw, _dh))
    }

    /// 生成/刷新扫描计划（判据一主入口；幂等——同输入重 plan 不升代数）。
    ///
    /// 顺序：先仲裁、先对齐与构建校验，全部通过才做状态迁移——失败
    /// 不动状态机（迁移原子性）。已在 Planned 态则视为合法重刷。
    pub fn plan(&mut self) -> Result<PlanSummary, DisplayRotError> {
        // 仲裁（已缓存的直接用；未仲裁且缺输入则拒绝）。
        let verdict = match self.verdict {
            Some(v) => v,
            None => self.rearbitrate()?,
        };
        let rot = verdict.effective;
        // 90/270 宽高交换后行跨度对齐合法性（奇偶边界显性化）。
        self.check_stride(rot)?;
        let plan = ScanPlan::build(self.panel_w, self.panel_h, rot)?;
        plan.verify_canonical()?;
        // 状态迁移：Idle→Planned；Planned→Planned（重刷）不落迁移表。
        if self.machine.state().to_u8() != RotState::Planned.to_u8() {
            self.machine.transition(RotState::Planned)?;
        }
        // 幂等：键相同则不升代数（重复旋转幂等入账为 Planned 但代数不动）。
        let changed = match &self.plan {
            Some(old) => old.key != plan.key,
            None => true,
        };
        if changed {
            self.plan_gen = self.plan_gen.wrapping_add(1);
        }
        let gen = self.plan_gen;
        self.plan = Some(plan);
        self.ledger
            .push(RotEventKind::Planned, rot, gen as u32);
        let est = RotCostTable::estimate_cost(self.panel_w, self.panel_h, self.bpp, rot)?;
        Ok(PlanSummary {
            rot,
            dst_w: est_rot_dst_w(rot, self.panel_w, self.panel_h),
            dst_h: est_rot_dst_h(rot, self.panel_w, self.panel_h),
            rows: self
                .plan
                .as_ref()
                .map(|p| p.rows.len() as u32)
                .unwrap_or(0),
            pixels: est.pixels,
            tier: est.tier,
            est_cycles: est.total_cycles,
            est_bytes: est.bytes_moved,
            needs_staging: est.needs_staging,
            generation: gen,
        })
    }

    /// 提交（Planned→Committing）。
    pub fn commit(&mut self) -> Result<(), DisplayRotError> {
        self.machine.transition(RotState::Committing)?;
        let rot = self.current_rot();
        self.ledger.push(RotEventKind::Committed, rot, 0);
        Ok(())
    }

    /// 应用（Committing→Applied；触屏联动随生效挂接）。
    pub fn apply(&mut self) -> Result<(), DisplayRotError> {
        self.machine.transition(RotState::Applied)?;
        let rot = self.current_rot();
        let mapper = TouchMapper::new(self.panel_w, self.panel_h, rot)?;
        self.mapper = Some(mapper);
        self.applied_count = self.applied_count.wrapping_add(1);
        self.ledger.push(RotEventKind::Applied, rot, 0);
        Ok(())
    }

    /// 回滚（Committing→Rolledback；真实可用：撤销在途计划与联动挂接，
    /// 账面保留全痕迹）。
    pub fn rollback(&mut self) -> Result<(), DisplayRotError> {
        self.machine.transition(RotState::Rolledback)?;
        self.mapper = None;
        self.rolled_count = self.rolled_count.wrapping_add(1);
        let rot = self.current_rot();
        self.ledger.push(RotEventKind::RolledBack, rot, 0);
        Ok(())
    }

    /// 复位（Applied/Rolledback→Idle；进入下一轮旋转周期）。
    pub fn reset(&mut self) -> Result<(), DisplayRotError> {
        self.machine.reset()?;
        self.mapper = None;
        self.ledger.push(RotEventKind::Reset, self.current_rot(), 0);
        Ok(())
    }

    /// 触点联动（严格路；须 Applied 态——未生效的旋转没有可联动面）。
    pub fn touch(&mut self, xd: u32, yd: u32) -> Result<(u32, u32), DisplayRotError> {
        if self.machine.state().to_u8() != RotState::Applied.to_u8() {
            return Err(DisplayRotError::BadTransition {
                from: self.machine.state(),
                to: RotState::Applied,
            });
        }
        match &mut self.mapper {
            Some(m) => m.map_to_panel(xd, yd),
            None => Err(DisplayRotError::PlanBuildFailed {
                reason: "mapper-missing",
            }),
        }
    }

    /// 触点联动（钳制路；夹边入账 + 事件账 TouchClamped）。
    pub fn touch_clamped(&mut self, xd: u32, yd: u32) -> Result<(u32, u32), DisplayRotError> {
        if self.machine.state().to_u8() != RotState::Applied.to_u8() {
            return Err(DisplayRotError::BadTransition {
                from: self.machine.state(),
                to: RotState::Applied,
            });
        }
        let (before, mapper) = match &mut self.mapper {
            Some(m) => (m.clamp_stats(), m),
            None => {
                return Err(DisplayRotError::PlanBuildFailed {
                    reason: "mapper-missing",
                })
            }
        };
        let out = mapper.map_to_panel_clamped(xd, yd);
        let after = mapper.clamp_stats();
        if after.total() != before.total() {
            self.ledger
                .push(RotEventKind::TouchClamped, self.current_rot(), 0);
        }
        Ok(out)
    }

    /// 按当前计划旋转一帧字节缓冲（真实像素搬运入口）。
    pub fn rotate_buffer(&self, src: &[u8]) -> Result<Vec<u8>, DisplayRotError> {
        match &self.plan {
            Some(p) => p.apply(src, self.bpp),
            None => Err(DisplayRotError::PlanBuildFailed {
                reason: "no-plan",
            }),
        }
    }

    /// 当前生效方向（未仲裁时 = 未定，返回 None）。
    pub fn current_rotation(&self) -> Option<Rotation> {
        self.verdict.map(|v| v.effective)
    }

    /// 当前方向（审计用，未定时记 0 档但不生效）。
    fn current_rot(&self) -> Rotation {
        match self.verdict {
            Some(v) => v.effective,
            None => Rotation::Deg0,
        }
    }

    /// 导出确定性快照（布局 v1，定长 76 字节；字段序固定、小端——
    /// 确定性来源是"手写逐字段 LE 布局"而非任何序列化框架）：
    ///
    /// ```text
    /// [0..4)   魔数 "VXFR"
    /// [4..6)   版本 u16 LE = 1
    /// [6..8)   保留 0x0000
    /// [8..12)  panel_w u32 LE
    /// [12..16) panel_h u32 LE
    /// [16..20) bpp u32 LE
    /// [20]     effective rot 稳定码
    /// [21]     policy 稳定码
    /// [22]     native source 稳定码
    /// [23]     native rot 稳定码（未声明 = 0 档占位）
    /// [24..32) generation u64 LE
    /// [32..40) clamp_x_lo u64 LE
    /// [40..48) clamp_x_hi u64 LE
    /// [48..56) clamp_y_lo u64 LE
    /// [56..64) clamp_y_hi u64 LE
    /// [64..72) mapped u64 LE
    /// [72..76) FNV-1a 32 LE（覆盖 [0..72)）
    /// ```
    pub fn export_plan_snapshot(&self) -> PlanSnapshot {
        let mut b: Vec<u8> = Vec::with_capacity(76);
        b.extend_from_slice(&SNAPSHOT_MAGIC);
        b.push((SNAPSHOT_VERSION & 0xFF) as u8);
        b.push(((SNAPSHOT_VERSION >> 8) & 0xFF) as u8);
        b.push(0x00);
        b.push(0x00);
        put_u32(&mut b, self.panel_w);
        put_u32(&mut b, self.panel_h);
        put_u32(&mut b, self.bpp);
        let (eff, policy, source, native_rot) = match self.verdict {
            Some(v) => (
                v.effective.to_u8(),
                self.policy.to_u8(),
                v.source.to_u8(),
                v.native.to_u8(),
            ),
            None => (
                Rotation::Deg0.to_u8(),
                self.policy.to_u8(),
                NativeSource::None.to_u8(),
                Rotation::Deg0.to_u8(),
            ),
        };
        b.push(eff);
        b.push(policy);
        b.push(source);
        b.push(native_rot);
        put_u64(&mut b, self.plan_gen);
        let cs = match &self.mapper {
            Some(m) => m.clamp_stats(),
            None => TouchClampStats::default(),
        };
        put_u64(&mut b, cs.clamp_x_lo);
        put_u64(&mut b, cs.clamp_x_hi);
        put_u64(&mut b, cs.clamp_y_lo);
        put_u64(&mut b, cs.clamp_y_hi);
        put_u64(&mut b, self.mapped_total());
        let fp = fnv1a32(&b);
        put_u32(&mut b, fp);
        PlanSnapshot {
            bytes: b,
            fingerprint: fp,
        }
    }

    /// 触点映射累计（联动账面透传）。
    pub fn mapped_total(&self) -> u64 {
        match &self.mapper {
            Some(m) => m.mapped(),
            None => 0,
        }
    }

    /// 解析快照（魔数/版本/长度/指纹四重校验；往返一致自证的另一半）。
    pub fn parse_plan_snapshot(bytes: &[u8]) -> Result<SnapshotData, DisplayRotError> {
        if bytes.len() != 76 {
            return Err(DisplayRotError::SnapshotMalformed {
                off: bytes.len(),
                why: "length-not-76",
            });
        }
        if bytes[0] != SNAPSHOT_MAGIC[0]
            || bytes[1] != SNAPSHOT_MAGIC[1]
            || bytes[2] != SNAPSHOT_MAGIC[2]
            || bytes[3] != SNAPSHOT_MAGIC[3]
        {
            return Err(DisplayRotError::SnapshotMalformed { off: 0, why: "magic" });
        }
        let ver = u16::from_le_bytes([bytes[4], bytes[5]]);
        if ver != SNAPSHOT_VERSION {
            return Err(DisplayRotError::SnapshotMalformed { off: 4, why: "version" });
        }
        let got_fp = match get_u32(bytes, 72) {
            Some(v) => v,
            None => {
                return Err(DisplayRotError::SnapshotMalformed { off: 72, why: "fp-range" })
            }
        };
        let want_fp = fnv1a32(&bytes[..72]);
        if got_fp != want_fp {
            return Err(DisplayRotError::SnapshotFingerprint {
                got: got_fp,
                want: want_fp,
            });
        }
        let panel_w = match get_u32(bytes, 8) {
            Some(v) => v,
            None => {
                return Err(DisplayRotError::SnapshotMalformed { off: 8, why: "panel_w" })
            }
        };
        let panel_h = match get_u32(bytes, 12) {
            Some(v) => v,
            None => {
                return Err(DisplayRotError::SnapshotMalformed { off: 12, why: "panel_h" })
            }
        };
        let _bpp = match get_u32(bytes, 16) {
            Some(v) => v,
            None => {
                return Err(DisplayRotError::SnapshotMalformed { off: 16, why: "bpp" })
            }
        };
        let effective = match Rotation::from_u8(bytes[20]) {
            Some(r) => r,
            None => {
                return Err(DisplayRotError::SnapshotMalformed { off: 20, why: "rot-code" })
            }
        };
        let policy = match ArbitPolicy::from_u8(bytes[21]) {
            Some(p) => p,
            None => {
                return Err(DisplayRotError::SnapshotMalformed { off: 21, why: "policy" })
            }
        };
        let source = match NativeSource::from_u8(bytes[22]) {
            Some(s) => s,
            None => {
                return Err(DisplayRotError::SnapshotMalformed { off: 22, why: "source" })
            }
        };
        let native_rot = match Rotation::from_u8(bytes[23]) {
            Some(r) => r,
            None => {
                return Err(DisplayRotError::SnapshotMalformed { off: 23, why: "native-rot" })
            }
        };
        let generation = match get_u64(bytes, 24) {
            Some(v) => v,
            None => {
                return Err(DisplayRotError::SnapshotMalformed { off: 24, why: "generation" })
            }
        };
        let cl_x_lo = match get_u64(bytes, 32) {
            Some(v) => v,
            None => {
                return Err(DisplayRotError::SnapshotMalformed { off: 32, why: "clamp0" })
            }
        };
        let cl_x_hi = match get_u64(bytes, 40) {
            Some(v) => v,
            None => {
                return Err(DisplayRotError::SnapshotMalformed { off: 40, why: "clamp1" })
            }
        };
        let cl_y_lo = match get_u64(bytes, 48) {
            Some(v) => v,
            None => {
                return Err(DisplayRotError::SnapshotMalformed { off: 48, why: "clamp2" })
            }
        };
        let cl_y_hi = match get_u64(bytes, 56) {
            Some(v) => v,
            None => {
                return Err(DisplayRotError::SnapshotMalformed { off: 56, why: "clamp3" })
            }
        };
        let mapped = match get_u64(bytes, 64) {
            Some(v) => v,
            None => {
                return Err(DisplayRotError::SnapshotMalformed { off: 64, why: "mapped" })
            }
        };
        Ok(SnapshotData {
            panel_w,
            panel_h,
            effective,
            generation,
            policy,
            source,
            native_rot,
            clamps: [cl_x_lo, cl_x_hi, cl_y_lo, cl_y_hi],
            mapped,
        })
    }

    /// 体检（判据兜底；全部整数对拍，检查可数、结论可追）。
    pub fn health(&mut self) -> HealthReport {
        let mut checks: Vec<HealthCheck> = Vec::new();
        // 1. 状态机自检。
        let sm_ok = self.machine.selfcheck().is_ok();
        checks.push(HealthCheck {
            name: "state-machine",
            ok: sm_ok,
            detail: String::from("五态机迁移表与计数自检"),
        });
        // 2. 事件账自检。
        let ledger_ok = self.ledger.selfcheck().is_ok();
        checks.push(HealthCheck {
            name: "event-ledger",
            ok: ledger_ok,
            detail: String::from("环形账覆盖恒等式与分类计数和"),
        });
        // 3. 常量对拍：四方向 3×2 计划与闭式常量表一致。
        let mut canon_ok = true;
        let mut ri = 0usize;
        while ri < Rotation::ALL.len() {
            let rot = Rotation::ALL[ri];
            match ScanPlan::build(CANON_W, CANON_H, rot) {
                Ok(p) => {
                    if p.verify_canonical().is_err() {
                        canon_ok = false;
                    }
                }
                Err(_) => canon_ok = false,
            }
            ri += 1;
        }
        checks.push(HealthCheck {
            name: "canonical-plan",
            ok: canon_ok,
            detail: String::from("3×2 基准四方向计划 vs 常量闭式表"),
        });
        // 4. 成本档位单调（档位序 0<180<270<90；表按旋转档排、核对按
        //    成本序走，两序不可混——与编译期断言同式，运行期再复核）。
        let cost_order: [usize; 4] = [0, 2, 3, 1];
        let mut cost_ok = true;
        let mut ci = 1usize;
        while ci < cost_order.len() {
            if ROT_COST_TABLE[cost_order[ci]].tier <= ROT_COST_TABLE[cost_order[ci - 1]].tier {
                cost_ok = false;
            }
            ci += 1;
        }
        checks.push(HealthCheck {
            name: "cost-tiers",
            ok: cost_ok,
            detail: String::from("成本档位严格递增复核"),
        });
        // 5. 触屏往返对拍（当前生效方向；未仲裁则查 0 档恒等自证）。
        let rot_probe = self.current_rot();
        let mut touch_ok = true;
        let mut touch_detail = String::from("对拍点数 ");
        match TouchMapper::new(self.panel_w, self.panel_h, rot_probe) {
            Ok(mut m) => match m.roundtrip_selfcheck() {
                Ok(n) => {
                    touch_detail.push_str(&fmt_u64(n));
                }
                Err(_) => {
                    touch_ok = false;
                    touch_detail.push_str("失败");
                }
            },
            Err(_) => {
                touch_ok = false;
                touch_detail.push_str("构建失败");
            }
        }
        checks.push(HealthCheck {
            name: "touch-roundtrip",
            ok: touch_ok,
            detail: touch_detail,
        });
        // 6. 快照往返一致（导出→解析→字段核对）。
        let snap = self.export_plan_snapshot();
        let snap_ok = match Self::parse_plan_snapshot(&snap.bytes) {
            Ok(d) => {
                d.panel_w == self.panel_w
                    && d.panel_h == self.panel_h
                    && d.generation == self.plan_gen
            }
            Err(_) => false,
        };
        checks.push(HealthCheck {
            name: "snapshot-roundtrip",
            ok: snap_ok,
            detail: String::from("导出→解析字段一致"),
        });
        // 7. 生效态一致性：Applied 态必须已挂联动器。
        let applied_ok = self.machine.state().to_u8() != RotState::Applied.to_u8()
            || self.mapper.is_some();
        checks.push(HealthCheck {
            name: "applied-mapper",
            ok: applied_ok,
            detail: String::from("Applied 态联动器必须在位"),
        });
        let mut all_ok = true;
        let mut i = 0usize;
        while i < checks.len() {
            if !checks[i].ok {
                all_ok = false;
            }
            i += 1;
        }
        let mut counters = String::from("plan_gen=");
        counters.push_str(&fmt_u64(self.plan_gen));
        counters.push_str(" pushes=");
        counters.push_str(&fmt_u64(self.ledger.pushes()));
        counters.push_str(" applied=");
        counters.push_str(&fmt_u64(self.applied_count));
        counters.push_str(" rolled=");
        counters.push_str(&fmt_u64(self.rolled_count));
        counters.push_str(" clamped=");
        counters.push_str(&fmt_u64(match &self.mapper {
            Some(m) => m.clamp_stats().total(),
            None => 0,
        }));
        HealthReport {
            ok: all_ok,
            checks,
            counters,
        }
    }
}

impl ArbitVerdict {
    /// 裁决者稳定码（事件账 aux 用：Agreed=0/Native=1/User=2）。
    pub fn arbiter_marker(&self) -> u32 {
        match self.arbiter {
            Arbiter::Agreed => 0,
            Arbiter::Native => 1,
            Arbiter::User => 2,
        }
    }
}

/// 计划摘要目标宽助手（避免在 plan() 里重复 dims_after 调用散落）。
fn est_rot_dst_w(rot: Rotation, w: u32, h: u32) -> u32 {
    rot.dims_after(w, h).0
}

/// 计划摘要目标高助手。
fn est_rot_dst_h(rot: Rotation, w: u32, h: u32) -> u32 {
    rot.dims_after(w, h).1
}

// ---------------------------------------------------------------------------
// 十四、编译期断言（const 面兜底——运行期零 assert 的补偿）
// ---------------------------------------------------------------------------

// 诊断码段闭集：首尾码必须钉在 0x12C0/0x12CF。
const _: () = assert!(X_ROT_ZERO_DIM == 0x12C0 && X_ROT_RESERVED == 0x12CF);
// 成本档位在成本序（0<180<270<90）上严格递增——表按旋转档排、档位
// 按成本序核对，两序不可混（下标越界即编译失败，一石二鸟）。
const _: () = assert!({
    let t = ROT_COST_TABLE;
    t[0].tier < t[2].tier && t[2].tier < t[3].tier && t[3].tier < t[1].tier
});
// 90/270 必须要求中转缓冲、0/180 必须（按当前规格）不要求。
const _: () = assert!({
    let t = ROT_COST_TABLE;
    !t[0].needs_staging && !t[2].needs_staging && t[1].needs_staging && t[3].needs_staging
});
// 环容量 2 的幂（位与取模前提）。
const _: () = assert!(RING_CAP.is_power_of_two() && RING_CAP == 256);
// 旋转档稳定码闭合。
const _: () = assert!(
    Rotation::Deg0.to_u8() == 0
        && Rotation::Deg90.to_u8() == 1
        && Rotation::Deg180.to_u8() == 2
        && Rotation::Deg270.to_u8() == 3
);
// 状态机五态稳定码闭合。
const _: () = assert!(
    RotState::Idle.to_u8() == 0
        && RotState::Planned.to_u8() == 1
        && RotState::Committing.to_u8() == 2
        && RotState::Applied.to_u8() == 3
        && RotState::Rolledback.to_u8() == 4
);
// 迁移表自反性：对角恒 0（不承认自迁移）。
const _: () = assert!({
    let t = ROT_TRANSITIONS;
    t[0][0] == 0 && t[1][1] == 0 && t[2][2] == 0 && t[3][3] == 0 && t[4][4] == 0
});
// 快照布局定长（魔数 4 + 版本 2 + 保留 2 + 尺寸 8 + bpp 4 + 方向 4 +
// 代数 8 + 钳制 32 + mapped 8 + 指纹 4 = 68）。
const _: () = assert!(SNAPSHOT_MAGIC.len() == 4);

// ---------------------------------------------------------------------------
// 十五、测试（旋转/成本/联动三组 + 工程兜底组）
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    /// 测试助手：期望成功，失败则带诊断炸出。
    fn ok<T>(r: Result<T, DisplayRotError>, ctx: &str) -> T {
        match r {
            Ok(v) => v,
            Err(e) => panic!(
                "[{}] 期望成功，实得诊断码 0x{:04X}：{}",
                ctx,
                e.code(),
                e.detail()
            ),
        }
    }

    /// 测试助手：期望失败，返回错误本体。
    fn err<T>(r: Result<T, DisplayRotError>, ctx: &str) -> DisplayRotError {
        match r {
            Ok(_) => panic!("[{}] 期望失败，实得成功", ctx),
            Err(e) => e,
        }
    }

    // ----- 旋转组 -----

    #[test]
    fn rot_group_roundtrip_all_directions() {
        // 5×3 面板四方向逐点：正向∘逆向 = 恒等。
        for rot in Rotation::ALL {
            let mut m = ok(TouchMapper::new(5, 3, rot), "mapper-new");
            let n = ok(m.roundtrip_selfcheck(), "roundtrip");
            assert_eq!(n, 15, "5×3 应全查 15 点，方向 {}", rot.name());
        }
    }

    #[test]
    fn rot_group_buffer_rotate_90_then_270_restores() {
        // 合成梯度帧：panel (x,y) → 字节 = (y*5+x)。90° 旋转后目标帧
        // 3×5；再对 3×5 做 270° 应还原原帧。
        let (w, h) = (5u32, 3u32);
        let mut src = Vec::with_capacity((w * h) as usize);
        let mut y = 0u32;
        while y < h {
            let mut x = 0u32;
            while x < w {
                src.push((y * w + x) as u8);
                x += 1;
            }
            y += 1;
        }
        let p90 = ok(ScanPlan::build(w, h, Rotation::Deg90), "plan90");
        ok(p90.verify_canonical(), "canon90");
        let mid = ok(p90.apply(&src, 1), "apply90");
        assert_eq!(mid.len(), src.len());
        // 90° 后目标帧 3 宽 5 高：display(xd,yd) ← panel(x=yd, y=H-1-xd)。
        // display(2,0) ← panel(0,0) → 值 0；display(2,1) ← panel(1,0) → 值 1。
        assert_eq!(mid[0 * 3 + 2], 0, "display(xd=2,yd=0)=panel(0,0)");
        assert_eq!(mid[1 * 3 + 2], 1, "display(xd=2,yd=1)=panel(1,0)");
        // 目标帧尺寸 3×5。
        assert_eq!(p90.dst_w, 3);
        assert_eq!(p90.dst_h, 5);
        // 270° 还原：对 3×5 帧做 270 得回 5×3 原帧。
        let p270 = ok(ScanPlan::build(3, 5, Rotation::Deg270), "plan270");
        ok(p270.verify_canonical(), "canon270");
        let back = ok(p270.apply(&mid, 1), "apply270");
        assert_eq!(back, src, "90∘270 应还原原帧");
    }

    #[test]
    fn rot_group_180_reverses_both_axes() {
        // 2×2 帧 [0,1,2,3]：180 后 [3,2,1,0]。
        let src: Vec<u8> = [0u8, 1, 2, 3].to_vec();
        let p = ok(ScanPlan::build(2, 2, Rotation::Deg180), "plan180");
        let out = ok(p.apply(&src, 1), "apply180");
        assert_eq!(out, [3u8, 2, 1, 0]);
    }

    #[test]
    fn rot_group_identity_is_bit_exact() {
        let src: Vec<u8> = [10u8, 20, 30, 40, 50, 60].to_vec();
        let p = ok(ScanPlan::build(3, 2, Rotation::Deg0), "plan0");
        let out = ok(p.apply(&src, 1), "apply0");
        assert_eq!(out, src, "0 档必须逐字节恒等");
    }

    #[test]
    fn rot_group_plan_matches_const_table() {
        // 四方向 3×2 计划与手工常量闭式表逐字段一致。
        for rot in Rotation::ALL {
            let p = ok(ScanPlan::build(CANON_W, CANON_H, rot), "canon-build");
            ok(p.verify_canonical(), "canon-verify");
        }
    }

    #[test]
    fn rot_group_zero_dim_rejected_everywhere() {
        let e1 = err(ScanPlan::build(0, 2, Rotation::Deg0), "plan0");
        assert_eq!(e1.code(), X_ROT_ZERO_DIM);
        let e2 = err(RotCostTable::estimate_cost(3, 0, 4, Rotation::Deg90), "cost");
        assert_eq!(e2.code(), X_ROT_ZERO_DIM);
        let e3 = err(TouchMapper::new(0, 0, Rotation::Deg270), "mapper");
        assert_eq!(e3.code(), X_ROT_ZERO_DIM);
        let e4 = err(DisplayRotDevice::new(0, 0, 4), "device");
        assert_eq!(e4.code(), X_ROT_ZERO_DIM);
    }

    #[test]
    fn rot_group_repeated_plan_idempotent() {
        // 同输入重复 plan：代数不变（幂等），换方向才升代数。
        let mut dev = ok(DisplayRotDevice::new(4, 4, 4), "dev");
        ok(dev.declare_native(NativeSource::Edid, Rotation::Deg90), "native");
        let s1 = ok(dev.plan(), "plan1");
        let s2 = ok(dev.plan(), "plan2");
        assert_eq!(s1.generation, s2.generation, "重复 plan 幂等");
        // 切用户优先再设用户方向 270 → 生效方向变更 → 键变 → 升代数。
        dev.set_policy(ArbitPolicy::UserFirst);
        ok(dev.set_user_rotation(Some(Rotation::Deg270)), "user");
        let s3 = ok(dev.plan(), "plan3");
        assert_ne!(s3.generation, s2.generation, "换方向须升代数");
        assert_eq!(s3.rot, Rotation::Deg270);
    }

    // ----- 成本组 -----

    #[test]
    fn cost_group_tiers_monotonic_and_annotated() {
        // 档位严格递增；90/270 需中转缓冲且贵于 0/180（判据二核心结论）。
        let e0 = ok(RotCostTable::entry(Rotation::Deg0), "e0");
        let e90 = ok(RotCostTable::entry(Rotation::Deg90), "e90");
        let e180 = ok(RotCostTable::entry(Rotation::Deg180), "e180");
        let e270 = ok(RotCostTable::entry(Rotation::Deg270), "e270");
        assert!(e0.tier < e180.tier && e180.tier < e270.tier && e270.tier < e90.tier);
        assert!(!e0.needs_staging && !e180.needs_staging);
        assert!(e90.needs_staging && e270.needs_staging);
        assert!(e90.bandwidth_permille > e0.bandwidth_permille);
        assert!(e270.bandwidth_permille > e0.bandwidth_permille);
        // 理由标注非空。
        for e in ROT_COST_TABLE.iter() {
            assert!(!e.reason.is_empty(), "成本档 {} 必须带理由", e.tier);
        }
    }

    #[test]
    fn cost_group_estimate_ordering_and_overflow() {
        // 1080p 整帧估算：90 档周期必须高于 0 档（成本标注落进数字）。
        let c0 = ok(RotCostTable::estimate_cost(1920, 1080, 4, Rotation::Deg0), "c0");
        let c90 = ok(RotCostTable::estimate_cost(1920, 1080, 4, Rotation::Deg90), "c90");
        assert!(c90.total_cycles > c0.total_cycles);
        assert!(c90.bytes_moved > c0.bytes_moved);
        assert_eq!(c90.pixels, 1920u64 * 1080u64);
        // 极端尺寸溢出显性报错（u64 乘法上限）。
        let big = u32::MAX;
        let e = err(RotCostTable::estimate_cost(big, big, 4, Rotation::Deg0), "overflow");
        assert!(
            e.code() == X_ROT_DIM_OVERFLOW || e.code() == X_ROT_COST_OVERFLOW,
            "溢出必须显性"
        );
        // 最贵方向必为 90（本表口径）。
        assert_eq!(
            ok(RotCostTable::worst_case(64, 48, 2), "worst"),
            Rotation::Deg90
        );
    }

    // ----- 联动组 -----

    #[test]
    fn touch_group_closed_form_per_direction() {
        // 手工基准点对拍（独立于 roundtrip 循环的第三条验证路）。
        // 面板 4×3：
        // * 90°：panel(0,0)→display(2,0)；panel(3,2)→display(0,3)。
        // * 180°：panel(0,0)→display(3,2)。
        // * 270°：panel(0,0)→display(0,3)；panel(3,2)→display(2,0)。
        let rot90 = Rotation::Deg90.forward(0, 0, 4, 3);
        assert_eq!(rot90, (2, 0));
        let rot90b = Rotation::Deg90.forward(3, 2, 4, 3);
        assert_eq!(rot90b, (0, 3));
        let rot180 = Rotation::Deg180.forward(0, 0, 4, 3);
        assert_eq!(rot180, (3, 2));
        let rot270 = Rotation::Deg270.forward(0, 0, 4, 3);
        assert_eq!(rot270, (0, 3));
        // 逆映射解对应点。
        assert_eq!(Rotation::Deg90.inverse(2, 0, 4, 3), (0, 0));
        assert_eq!(Rotation::Deg180.inverse(3, 2, 4, 3), (0, 0));
        assert_eq!(Rotation::Deg270.inverse(0, 3, 4, 3), (0, 0));
    }

    #[test]
    fn touch_group_strict_reject_and_clamp_account() {
        let mut m = ok(TouchMapper::new(4, 3, Rotation::Deg90), "m");
        // 显示帧 3×4：点 (3,4) 越界 → 严格路拒绝 0x12C7。
        let e = err(m.map_to_panel(3, 4), "strict");
        assert_eq!(e.code(), X_ROT_COORD_RANGE);
        assert_eq!(m.rejected(), 1);
        // 钳制路：xd=9 夹回上界 2 → panel = (yd, H-1-xd) = (0, 3-1-2) = (0,0)。
        let p = m.map_to_panel_clamped(9, 0);
        assert_eq!(p, (0, 0));
        let st = m.clamp_stats();
        assert_eq!(st.clamp_x_hi, 1, "x 上界钳制必须记一笔");
        assert_eq!(st.total(), 1);
    }

    #[test]
    fn touch_group_clamped_returns_in_panel() {
        // 钳制路返回值必须落在面板原生范围内（四方向全查）。
        for rot in Rotation::ALL {
            let mut m = ok(TouchMapper::new(7, 5, rot), "m");
            let mut xd = 0u32;
            while xd < m.disp_w() + 3 {
                let mut yd = 0u32;
                while yd < m.disp_h() + 3 {
                    let (px, py) = m.map_to_panel_clamped(xd, yd);
                    assert!(px < m.panel_w() && py < m.panel_h(), "钳制结果出面板");
                    yd += 1;
                }
                xd += 1;
            }
            assert!(m.clamp_stats().total() > 0);
        }
    }

    #[test]
    fn touch_group_device_lifecycle_gating() {
        // Applied 态之前触点必须被拒（联动面不存在）。
        let mut dev = ok(DisplayRotDevice::new(8, 6, 4), "dev");
        ok(dev.declare_native(NativeSource::Acpi, Rotation::Deg90), "nat");
        ok(dev.plan(), "plan");
        let e = dev.touch(0, 0).err().expect("err");
        assert_eq!(e.code(), X_ROT_BAD_TRANSITION);
        ok(dev.commit(), "commit");
        ok(dev.apply(), "apply");
        let p = ok(dev.touch(0, 0), "touch-applied");
        assert_eq!(p, (0, 5), "90° 下 display(0,0) = panel(0,5)"); // H=6
    }

    // ----- 工程兜底组 -----

    #[test]
    fn sm_group_out_of_order_rejected_with_code() {
        // 乱序链逐条拒绝且带 0x12C3。
        let mut dev = ok(DisplayRotDevice::new(4, 4, 1), "dev");
        ok(dev.declare_native(NativeSource::Edid, Rotation::Deg0), "nat");
        // Idle 直接 commit → 拒。
        let e = err(dev.commit(), "idle-commit");
        assert_eq!(e.code(), X_ROT_BAD_TRANSITION);
        assert_eq!(dev.machine().rejected(), 1);
        // 正链推进后 reset 不允许（须先 Applied/Rolledback）。
        ok(dev.plan(), "plan");
        ok(dev.commit(), "commit");
        ok(dev.apply(), "apply");
        // Applied→Planned 非法。
        let e2 = err(
            dev.machine_mut().transition(RotState::Planned),
            "applied-planned",
        );
        assert_eq!(e2.code(), X_ROT_BAD_TRANSITION);
        // 回滚链：再走一轮，Committing 时回滚真实可用。
        ok(dev.reset(), "reset");
        ok(dev.plan(), "plan2");
        ok(dev.commit(), "commit2");
        ok(dev.rollback(), "rollback");
        assert_eq!(dev.state(), RotState::Rolledback);
        assert_eq!(dev.rolled_count(), 1);
        ok(dev.reset(), "reset2");
        assert_eq!(dev.state(), RotState::Idle);
    }

    #[test]
    fn sm_group_transition_table_shape() {
        // 主链逐边允许；非法边逐点拒绝。
        assert!(RotationStateMachine::allowed(RotState::Idle, RotState::Planned));
        assert!(RotationStateMachine::allowed(RotState::Planned, RotState::Committing));
        assert!(RotationStateMachine::allowed(RotState::Committing, RotState::Applied));
        assert!(RotationStateMachine::allowed(RotState::Committing, RotState::Rolledback));
        assert!(RotationStateMachine::allowed(RotState::Applied, RotState::Idle));
        assert!(RotationStateMachine::allowed(RotState::Rolledback, RotState::Idle));
        assert!(!RotationStateMachine::allowed(RotState::Idle, RotState::Applied));
        assert!(!RotationStateMachine::allowed(RotState::Planned, RotState::Rolledback));
        assert!(!RotationStateMachine::allowed(RotState::Applied, RotState::Committing));
    }

    #[test]
    fn ledger_group_wrap_and_counts() {
        // 300 条入账：覆盖 44 条显性入账，环内存 256，分类和 = 300。
        let mut led = RotEventLedger::new();
        let mut i = 0u64;
        while i < 300 {
            let kind = match (i % 4) as u8 {
                0 => RotEventKind::Planned,
                1 => RotEventKind::Committed,
                2 => RotEventKind::Applied,
                _ => RotEventKind::RolledBack,
            };
            led.push(kind, Rotation::from_u8((i % 4) as u8).unwrap_or(Rotation::Deg0), i as u32);
            i += 1;
        }
        assert_eq!(led.pushes(), 300);
        assert_eq!(led.overwritten(), 44);
        assert_eq!(led.stored(), 256);
        ok(led.selfcheck(), "ledger-selfcheck");
        // 最旧现存必须是第 45 条（seq 45）。
        let oldest = led.at(0).expect("oldest");
        assert_eq!(oldest.seq, 45);
        // 分类计数抽查。
        assert_eq!(led.kind_count(RotEventKind::Planned), 75);
        assert_eq!(led.kind_count(RotEventKind::Committed), 75);
        // 越界读 None。
        assert!(led.at(256).is_none());
    }

    #[test]
    fn arbit_group_priority_and_trail() {
        // 一致 → Agreed；冲突 → 按策略；双缺 → 拒绝 0x12CC。
        let v1 = ok(
            arbitrate(NativeSource::Edid, Some(Rotation::Deg90), Some(Rotation::Deg90), ArbitPolicy::NativeFirst),
            "agree",
        );
        assert!(v1.arbiter == Arbiter::Agreed && !v1.conflict);
        let v2 = ok(
            arbitrate(NativeSource::Acpi, Some(Rotation::Deg90), Some(Rotation::Deg270), ArbitPolicy::NativeFirst),
            "nf",
        );
        assert!(v2.conflict && v2.effective == Rotation::Deg90 && v2.arbiter == Arbiter::Native);
        let v3 = ok(
            arbitrate(NativeSource::Acpi, Some(Rotation::Deg90), Some(Rotation::Deg270), ArbitPolicy::UserFirst),
            "uf",
        );
        assert!(v3.conflict && v3.effective == Rotation::Deg270 && v3.arbiter == Arbiter::User);
        let e = err(
            arbitrate(NativeSource::None, None, None, ArbitPolicy::UserFirst),
            "denied",
        );
        assert_eq!(e.code(), X_ROT_ARBIT_DENIED);
    }

    #[test]
    fn snapshot_group_roundtrip_and_tamper() {
        let mut dev = ok(DisplayRotDevice::new(64, 32, 4), "dev");
        ok(dev.declare_native(NativeSource::Edid, Rotation::Deg180), "nat");
        ok(dev.plan(), "plan");
        ok(dev.commit(), "commit");
        ok(dev.apply(), "apply");
        let _ = ok(dev.touch_clamped(999, 999), "clamped");
        let snap = dev.export_plan_snapshot();
        assert_eq!(snap.bytes.len(), 76);
        assert_eq!(snap.fingerprint, fnv1a32(&snap.bytes[..72]));
        let data = ok(DisplayRotDevice::parse_plan_snapshot(&snap.bytes), "parse");
        assert_eq!(data.panel_w, 64);
        assert_eq!(data.panel_h, 32);
        assert_eq!(data.effective, Rotation::Deg180);
        assert_eq!(data.policy, ArbitPolicy::NativeFirst);
        assert_eq!(data.source, NativeSource::Edid);
        assert!(data.clamps.iter().any(|c| *c > 0), "钳制账面必须进快照");
        // 篡改一个字节 → 指纹拒绝 0x12C9。
        let mut bad = snap.bytes.clone();
        bad[10] ^= 0x01;
        let e = err(DisplayRotDevice::parse_plan_snapshot(&bad), "tamper");
        assert_eq!(e.code(), X_ROT_SNAPSHOT_FP);
        // 砍长度 → 格式拒绝 0x12C8。
        let e2 = err(DisplayRotDevice::parse_plan_snapshot(&snap.bytes[..40]), "short");
        assert_eq!(e2.code(), X_ROT_SNAPSHOT_MALFORMED);
    }

    #[test]
    fn stride_group_swap_alignment_boundary() {
        // 90/270 宽高交换后行跨度对齐：5×3 bpp1 90° → 目标行 3 字节，
        // 4 字节对齐不过 → 0x12CE；bpp4 → 12 字节对齐过。
        let mut dev = ok(DisplayRotDevice::new(5, 3, 1), "dev1");
        ok(dev.declare_native(NativeSource::Acpi, Rotation::Deg90), "nat");
        let e = err(dev.plan(), "misalign");
        assert_eq!(e.code(), X_ROT_STRIDE_ALIGN);
        // 0 档不换宽高：行 5 字节同样不过——换 4 宽面板验证 0 档可行。
        let mut dev_ok = ok(DisplayRotDevice::new(4, 3, 4), "dev4");
        ok(dev_ok.declare_native(NativeSource::Acpi, Rotation::Deg90), "nat4");
        ok(dev_ok.plan(), "plan4");
        // 对齐要求清零后 5×3 也可过（显性豁免）。
        let mut dev_free = ok(DisplayRotDevice::new(5, 3, 1), "dev0");
        dev_free.set_stride_align(0);
        ok(dev_free.declare_native(NativeSource::Acpi, Rotation::Deg270), "nat0");
        ok(dev_free.plan(), "plan0");
    }

    #[test]
    fn health_group_all_green_after_full_flow() {
        let mut dev = ok(DisplayRotDevice::new(32, 16, 2), "dev");
        ok(dev.declare_native(NativeSource::Acpi, Rotation::Deg90), "nat");
        ok(dev.plan(), "plan");
        ok(dev.commit(), "commit");
        ok(dev.apply(), "apply");
        let _ = ok(dev.touch(1, 1), "touch");
        let rep = dev.health();
        assert!(rep.ok, "体检必须全绿：{:?}", rep.checks);
        assert_eq!(rep.checks.len(), 7);
        // 错误诊断字符串非空（detail 闭集抽查）。
        let e = DisplayRotError::ZeroDimension { who: "t" };
        assert!(!e.detail().is_empty());
        assert_eq!(e.code(), X_ROT_ZERO_DIM);
    }

    #[test]
    fn fnv_group_known_vector() {
        // FNV-1a 32 已知向量：空串 = 2166136261；"a" = 0xE40C292C。
        assert_eq!(fnv1a32(&[]), 2166136261);
        assert_eq!(fnv1a32(b"a"), 0xE40C292C);
        assert_eq!(fnv1a32(b"foobar"), 0xBF9CF968);
    }
}
